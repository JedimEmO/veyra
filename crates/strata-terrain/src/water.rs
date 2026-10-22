//! The sea: one translucent plane at sea level that follows the observer.
//!
//! There is no water simulation in the slice (PLAN section 1); what the sea
//! needs to do is read as water where the terrain dips below sea level, so
//! oceans and lakes stop looking like sand. A single large quad does that.
//! It has a `WorldPosition` like everything else and is placed by the
//! render-origin sync (ADR 0002): every frame the follow system writes
//! the primary observer's `x` and `z` and the world's sea level into it, so
//! the plane is always centred under the camera and its edge is never in
//! view.
//!
//! The water's opacity is a function of depth, read per fragment from the
//! camera's depth prepass (`shaders/water.wgsl`): a shoreline fades in from
//! nothing and deep water hides its floor. That needs a `DepthPrepass` on
//! the camera, which this plugin adds, at the cost of drawing the terrain's
//! depth once more per frame.
//!
//! The surface (the lighting plan's phase 5a) is two layers of one
//! generated, tileable wave image (`wave_image`) scrolled downwind at two
//! scales and two headings, so their interference never repeats visibly;
//! the image's height channel puts foam on the crests and its fourth
//! channel is the foam's own noise, read a third time, slower. Past
//! [`WaterConfig::detail_fade`] the layers fade to a flat surface and the
//! roughness rises in their place, which keeps the sun's track on far water
//! without a normal map aliasing at a grazing angle. The sky's reflection
//! is the shader's own read of the environment map at the sky's radiance,
//! not Bevy's: Bevy scales that map by the sky fill
//! ([`WaterLook`]'s note says why that matters), which made the sea a
//! white sheet. The image moves by the sim's clock along the sim's wind
//! direction, wrapped in `f64` with the render origin (`update_water`),
//! so a rebase does not shift the waves and two clients see the same sea.
//!
//! Those parameters change every frame, so they are not the material's: a
//! uniform in the material asset made Bevy re-prepare the material (a new
//! buffer and bind group) on every frame, 0.12 to 0.17 ms and up to 10 ms
//! in the frames that upload terrain (docs/perf/streaming-2026-10-04.md,
//! cost 6). They live in one buffer of their own instead, made with the
//! sea and bound by the material once, which the render world rewrites
//! from the extracted [`WaterParams`], as the foliage's wind buffer is.
//! Written there rather than from the main world, the bytes a frame draws
//! with are the ones extracted with its transforms, so a rebase moves the
//! plane and its wave offsets in the same frame.
//! The wind's strength does not calm it: that is a plant's sway, and a sea
//! with no swell at all reads as a sheet again.
//!
//! The shore's foam follows the land, not only the depth: the world's
//! coasts mostly drop to the sea bed within a metre of the water's edge, so
//! a band of shallow water is a pixel wide from a walker's eye, and the
//! shader also looks a few metres along the world's axes in the depth
//! prepass for ground over the water's level. The air in front of the water
//! is the shader's too, since Bevy's aerial perspective covers only opaque
//! depth.
//!
//! The plane stays hidden until the first terrain mesh is on screen: for
//! the seconds the texture arrays take to build, a sea across the whole
//! window is a worse loading screen than the sky alone.

use bevy::app::{App, Plugin, PostUpdate, Startup, Update};
use bevy::asset::{Asset, AssetServer, Assets, Handle, RenderAssetUsages};
use bevy::camera::Camera3d;
use bevy::camera::visibility::Visibility;
use bevy::color::Color;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::ecs::prelude::*;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::material::AlphaMode;
use bevy::math::primitives::Plane3d;
use bevy::math::{DVec2, Vec4};
use bevy::mesh::{Mesh, Mesh3d, Meshable};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialPlugin, MeshMaterial3d, StandardMaterial,
};
use bevy::reflect::Reflect;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::encase::StorageBuffer;
use bevy::render::render_resource::{
    AsBindGroup, Buffer, BufferInitDescriptor, BufferUsages, Extent3d, ShaderType,
    TextureDataOrder, TextureDimension, TextureFormat,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::{Shader, ShaderRef};
use bevy::state::condition::in_state;
use bevy::state::state::{OnEnter, OnExit};
use strata_app::{
    AppState, Observers, OriginSet, RenderOrigin, SimClock, Wind, WorldPosition, WorldSourceRes,
};
use strata_core::{WorldPos, mix64};
use strata_stream::StreamConfig;

use crate::TerrainStats;
use crate::material::TerrainParams;
use crate::river::{RiverConfig, RiverMaterial, RiverMaterialHandle, RiverSurface};

/// The water shader, relative to the asset root.
pub const WATER_SHADER_PATH: &str = "shaders/water.wgsl";

/// The look the sea and the rivers share, relative to the asset root.
/// Loaded by the plugin so its `#define_import_path` is known before either
/// pipeline compiles.
pub const WATER_COMMON_SHADER_PATH: &str = "shaders/water_common.wgsl";

/// Texels along a side of the wave image. At the swell's 37 m tile one
/// texel is 14 cm, finer than the shortest wave in it (eight texels).
pub const WAVE_SIZE: usize = 256;

/// Waves summed into the image. Forty with random headings and lengths
/// read as a sea, not as a grid of crossing sines.
const WAVE_COUNT: u64 = 40;

/// The shortest and the longest wave in the image, in cycles per tile. The
/// shortest is eight texels, so mip zero holds it without aliasing.
const WAVE_CYCLES: (f32, f32) = (3.0, 32.0);

/// How far, in radians, a wave's heading strays from the wind's. Wide, so
/// the crests are short and broken; a few run against it for chop.
const WAVE_SPREAD: f32 = 1.1;

/// How the sea looks. Insert before [`WaterPlugin`] to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct WaterConfig {
    /// Side of the plane, metres. The plane is centred on the observer
    /// every frame, so this is twice the distance its edge sits at. It must
    /// reach past the atmosphere's spherical horizon from any height the
    /// slice flies at, or a strip of the planet's bare ground shows between
    /// the sea's edge and the sky: the edge at distance `D` from height `h`
    /// sits `h / D` below level, the horizon `sqrt(2h / R)`, so `D` has to
    /// exceed `sqrt(h R / 2)`, which is 200 km at 12 km up.
    pub extent: f32,
    /// The water's colour, linear RGB: what deep water looks like.
    pub color: [f32; 3],
    /// Absorption per metre of view path through the water. At 0.35 the
    /// floor is a third visible under three metres of water and gone under
    /// ten, which reads as a real shoreline at the slice's scale.
    pub absorption: f32,
    /// Opacity of the deepest water. Just under one keeps a hint of the
    /// floor's shading in the shallows next to the drop-off.
    pub max_alpha: f32,
    /// Perceptual roughness of near water. Low, so the sun's reflection is
    /// a sharp glitter on each facet the waves turn towards it.
    pub roughness: f32,
    /// Perceptual roughness of water past [`detail_fade`](Self::detail_fade),
    /// where the waves are no longer drawn: the slopes they would have had
    /// spread the sun's reflection into a track instead of a point.
    pub far_roughness: f32,
    /// Bevy's `reflectance`: 0.35 is water's own 2 % at normal incidence;
    /// a little over it, so the sky reads in the near water as well as at
    /// a grazing angle.
    pub reflectance: f32,
    /// Bevy's `specular_tint`, linear RGB: the reflection leans towards the
    /// water's blue, so a white sky does not mirror grey.
    pub specular_tint: [f32; 3],
    /// The long layer: tile side (metres), steepest slope it reaches (rise
    /// over run), speed downwind (metres a second).
    pub swell: [f32; 3],
    /// The short layer, likewise: smaller, shallower and slower, so the two
    /// interfere instead of moving as one.
    pub chop: [f32; 3],
    /// The short layer's heading off the wind's, radians.
    pub chop_angle: f32,
    /// The foam noise's tile side and its speed downwind: larger and slower
    /// than the waves, so foam drifts rather than races.
    pub foam_tile: [f32; 2],
    /// Distance from the camera, metres, over which the waves fade out: in
    /// full under the first, flat (at the far roughness) past the second.
    pub detail_fade: [f32; 2],
    /// Water shallower than this, metres straight down, foams at the shore.
    pub shore_foam_depth: f32,
    /// Water nearer the land than this, metres, foams at the shore too: a
    /// coast that drops into deep water at once has no shallows for the
    /// depth's band, and the land's distance is found in the depth prepass
    /// instead (`land_near` in the shader).
    pub shore_foam_reach: f32,
    /// Combined crest height (0 to 1, the image's height channel) over
    /// which a crest starts to foam.
    pub crest_threshold: f32,
    /// The foam's albedo. Bright, but a lit patch stays under the bloom's
    /// threshold, as sand does at noon.
    pub foam_albedo: f32,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self {
            extent: 400_000.0,
            color: [0.02, 0.07, 0.13],
            absorption: 0.35,
            max_alpha: 0.985,
            roughness: 0.12,
            far_roughness: 0.28,
            reflectance: 0.5,
            specular_tint: [0.86, 0.95, 1.0],
            swell: [37.0, 0.22, 1.3],
            chop: [9.7, 0.14, 0.7],
            chop_angle: 0.7,
            foam_tile: [23.0, 0.25],
            detail_fade: [90.0, 300.0],
            shore_foam_depth: 2.5,
            shore_foam_reach: 8.0,
            crest_threshold: 0.77,
            foam_albedo: 0.55,
        }
    }
}

/// The look's water tunables, carried here from the atmosphere crate's
/// `RenderLook` (`water_reflection`, `water_foam`) by the client, since
/// this crate does not see it.
///
/// The reflection is the sky's own radiance, not the environment map
/// light's: that light's intensity is the look's sky fill (4 by default),
/// which lifts the shade's diffuse light and, through Bevy's path, scaled
/// the sea's mirror image of the sky four times over. That was the white
/// sheet on the lakes.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct WaterLook {
    /// How much of the sky and the sun the surface reflects, 0 to 1: one is
    /// the Fresnel of [`WaterConfig::reflectance`], zero a matte sea.
    pub reflection: f32,
    /// How much foam there is at the shore and on the crests, 0 to 1.
    pub foam: f32,
}

impl Default for WaterLook {
    fn default() -> Self {
        Self {
            reflection: 1.0,
            foam: 1.0,
        }
    }
}

/// The water's per-frame parameters, mirrored by `WaterParams` in the
/// shader. Each layer's frame is wrapped on the CPU in `f64`, so neither
/// the render origin nor the time played loses the `f32` the shader gets.
/// The look's and the config's terms ride along with the moving ones: they
/// cost nothing in a buffer that is written every frame anyway, and one
/// struct keeps the shader's mirror in one place.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct WaterParams {
    /// Absorption per metre in `x`, the deepest water's opacity in `y`, the
    /// look's reflection in `z` and its foam in `w`.
    pub optics: Vec4,
    /// The swell's frame: its texture offset in `xy`, one over its tile in
    /// metres in `z`, its steepest slope in `w`.
    pub swell: Vec4,
    /// The chop's frame, likewise.
    pub chop: Vec4,
    /// The swell's heading (a unit `xz` vector) in `xy`, the chop's in `zw`.
    pub axes: Vec4,
    /// The foam noise's frame: offset in `xy`, one over its tile in `z`; the
    /// shore band's depth in metres in `w`.
    pub foam: Vec4,
    /// Where the waves start and finish fading out in `xy`, the far
    /// roughness in `z`, the foam's albedo in `w`.
    pub fade: Vec4,
    /// The crest's foam threshold in `x`, the shore foam's reach from the
    /// land in metres in `y`; `zw` unused.
    pub crest: Vec4,
    /// The rivers' flow map ([`crate::river`]): the render origin's `x` and
    /// `z` wrapped to their ripples' tile in `xy`, so a rebase does not move
    /// the ripples; one over the tile in `z`; the flow cycle's phase, 0 to
    /// 1, in `w`.
    pub flow: Vec4,
}

/// The `MaterialExtension` half of [`WaterMaterial`]: depth-based opacity,
/// the wave image and the reflection over the standard lighting. Binding
/// 100 onwards keeps clear of the base's.
///
/// Nothing in it changes after the sea is spawned, so Bevy prepares it
/// once: the parameters that move are in the buffer it binds, not in it.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(from_reflect = false)]
pub struct WaterDepth {
    /// Opacity, the layers' frames and the foam: one [`WaterParams`], which
    /// the render world rewrites every frame (the module's note).
    #[storage(100, read_only, buffer)]
    #[reflect(ignore)]
    pub params: Buffer,
    /// The wave image (`wave_image`), repeated, with mips.
    #[texture(101)]
    #[sampler(102)]
    pub waves: Handle<Image>,
}

impl MaterialExtension for WaterDepth {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER_PATH.into()
    }
}

/// The sea's material as an asset type.
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterDepth>;

/// The sea plane.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WaterPlane;

/// The sea's parameter buffer and what it should hold this frame. The main
/// world's copy is set by `update_water` and changes only when the values
/// do; the render world's is extracted from it on a change and written to
/// the buffer in its prepare step.
#[derive(Resource, Debug, Clone)]
struct WaterFrame {
    /// The buffer [`WaterDepth::params`] binds.
    buffer: Buffer,
    /// What the shader should read from it.
    params: WaterParams,
}

impl ExtractResource for WaterFrame {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// Spawns the sea when the world exists and keeps it under the observer.
#[derive(Debug, Clone, Copy, Default)]
pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WaterConfig>()
            .init_resource::<WaterLook>()
            .init_resource::<RiverConfig>()
            .init_resource::<crate::falls::FallConfig>()
            .init_resource::<crate::falls::FallsDrawn>()
            .add_systems(Startup, load_water_library)
            .add_systems(
                Update,
                crate::falls::stream_falls.run_if(in_state(AppState::InWorld)),
            )
            .add_systems(OnExit(AppState::InWorld), crate::falls::clear_falls)
            .add_systems(OnEnter(AppState::InWorld), spawn_water)
            .add_systems(
                Update,
                (follow_observer, reveal_water, add_depth_prepass)
                    .run_if(in_state(AppState::InWorld)),
            )
            // After the rebase, so the layers' offsets and the transforms
            // agree on one origin when both are extracted.
            .add_systems(
                PostUpdate,
                update_water
                    .after(OriginSet::Rebase)
                    .before(OriginSet::Sync),
            );
        // The material type only exists with a renderer; the plugin is
        // still usable in a headless test app.
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins((
                MaterialPlugin::<WaterMaterial>::default(),
                MaterialPlugin::<RiverMaterial>::default(),
                MaterialPlugin::<crate::falls::FallMaterial>::default(),
                ExtractResourcePlugin::<WaterFrame>::default(),
            ));
            if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
                render_app.add_systems(
                    Render,
                    write_water_frame.in_set(RenderSystems::PrepareResources),
                );
            }
        }
    }
}

/// [`WaterParams`] in the shader's storage layout.
fn water_bytes(params: &WaterParams) -> Vec<u8> {
    let mut bytes = StorageBuffer::new(Vec::new());
    // Writing a fixed-size struct into a growable `Vec` cannot run out of
    // room, which is the only way `write` fails.
    bytes
        .write(params)
        .expect("a WaterParams fits a growable buffer");
    bytes.into_inner()
}

/// Copies the extracted parameters into the buffer before this frame's
/// draw, only when they changed: every frame while the sim's clock runs,
/// none while it is paused.
fn write_water_frame(frame: Option<Res<WaterFrame>>, queue: Res<RenderQueue>) {
    if let Some(frame) = frame
        && frame.is_changed()
    {
        queue.write_buffer(&frame.buffer, 0, &water_bytes(&frame.params));
    }
}

/// Keeps the shared water shader loaded for as long as the app runs; an
/// unloaded import path is a pipeline compile error.
#[derive(Resource, Debug, Clone)]
pub struct WaterShaderLibrary(pub Handle<Shader>);

/// Loads the shared water shader. Skipped where there is no `Shader` asset
/// type, which is any app without a renderer.
fn load_water_library(
    mut commands: Commands,
    asset_server: Option<Res<AssetServer>>,
    shaders: Option<Res<Assets<Shader>>>,
) {
    if let (Some(asset_server), Some(_)) = (asset_server, shaders) {
        commands.insert_resource(WaterShaderLibrary(
            asset_server.load::<Shader>(WATER_COMMON_SHADER_PATH),
        ));
    }
}

/// One quad at sea level, the buffer its material reads, and the rivers'
/// material over the same buffer and waves ([`RiverMaterialHandle`]).
/// Without mesh assets or a render device (a headless app) there is nothing
/// to draw and nothing is spawned.
#[allow(
    clippy::too_many_arguments,
    reason = "the sea and the rivers are made together from one buffer"
)]
fn spawn_water(
    mut commands: Commands,
    config: Res<WaterConfig>,
    river: Res<RiverConfig>,
    stream: Option<Res<StreamConfig>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: (
        Option<ResMut<Assets<WaterMaterial>>>,
        Option<ResMut<Assets<RiverMaterial>>>,
    ),
    images: Option<ResMut<Assets<Image>>>,
    device: Option<Res<RenderDevice>>,
    source: Option<Res<WorldSourceRes>>,
) {
    let (Some(mut meshes), (Some(mut materials), Some(mut rivers)), Some(mut images), Some(device)) =
        (meshes, materials, images, device)
    else {
        return;
    };
    let sea32 = source.as_ref().map_or(0.0, |s| s.source().sea_level());
    let sea_level = f64::from(sea32);
    let [r, g, b] = config.color;
    let [tr, tg, tb] = config.specular_tint;
    let waves = images.add(wave_image());
    let params = water_params(
        &config,
        &river,
        WaterLook::default(),
        DVec2::ZERO,
        DVec2::X,
        0.0,
    );
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("water params"),
        contents: &water_bytes(&params),
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });
    commands.insert_resource(WaterFrame {
        buffer: buffer.clone(),
        params,
    });
    // The rivers' material: the sea's base with the rivers' colour, and
    // the terrain's morph band, which the streamer's split factor sets.
    #[allow(clippy::cast_possible_truncation, reason = "a split factor of a few")]
    let morph = TerrainParams::morph_for_split_factor(
        stream.map_or(2.5, |stream| stream.split_factor as f32),
    );
    let [rr, rg, rb] = river.color;
    let river_material = rivers.add(RiverMaterial {
        base: StandardMaterial {
            base_color: Color::linear_rgb(rr, rg, rb),
            perceptual_roughness: config.roughness,
            metallic: 0.0,
            reflectance: config.reflectance,
            specular_tint: Color::linear_rgb(tr, tg, tb),
            alpha_mode: AlphaMode::Premultiplied,
            depth_bias: crate::river::RIVER_SORT_BIAS,
            ..Default::default()
        },
        extension: RiverSurface {
            params: buffer.clone(),
            waves: waves.clone(),
            river: river.params(morph, sea32),
        },
    });
    commands.insert_resource(RiverMaterialHandle(river_material));
    commands.spawn((
        Name::new("sea"),
        WaterPlane,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(config.extent, config.extent))),
        MeshMaterial3d(materials.add(WaterMaterial {
            base: StandardMaterial {
                base_color: Color::linear_rgb(r, g, b),
                perceptual_roughness: config.roughness,
                metallic: 0.0,
                reflectance: config.reflectance,
                specular_tint: Color::linear_rgb(tr, tg, tb),
                // Premultiplied, so the reflection is not dimmed with the
                // water's opacity: shallow water over sand still mirrors
                // the sky, and only what lies under it fades in with depth.
                alpha_mode: AlphaMode::Premultiplied,
                ..Default::default()
            },
            extension: WaterDepth {
                params: buffer,
                waves,
            },
        })),
        WorldPosition::new(0.0, sea_level, 0.0),
        Visibility::Hidden,
        // A translucent plane still casts an opaque shadow, which would put
        // the whole sea floor within the cascade range in the dark and draw
        // a line across the sea where the cascades end. It receives the
        // sun's shadow, though: without it the sun's glitter shone on water
        // under a hill that hides the sun at dusk (0.03 ms with the sea
        // filling the frame, 2026-10-03).
        NotShadowCaster,
    ));
}

/// The shader's parameters for an origin (world `x` and `z`), a unit wind
/// heading and the sim's seconds.
fn water_params(
    config: &WaterConfig,
    river: &RiverConfig,
    look: WaterLook,
    origin: DVec2,
    wind: DVec2,
    elapsed: f64,
) -> WaterParams {
    let chop_heading = DVec2::from_angle(f64::from(config.chop_angle)).rotate(wind);
    let frame = |heading: DVec2, tile: f32, speed: f32| {
        let inv_tile = 1.0 / f64::from(tile.max(0.01));
        // The image's coordinate at render-space `p` is `(p . heading,
        // p . perp(heading)) / tile` plus this: the origin puts render space
        // back into the world and the drift carries the waves downwind.
        let along = origin.dot(heading) - f64::from(speed) * elapsed;
        let across = origin.dot(heading.perp());
        (DVec2::new(along, across) * inv_tile).map(|v| v.rem_euclid(1.0))
    };
    let [swell_tile, swell_slope, swell_speed] = config.swell;
    let [chop_tile, chop_slope, chop_speed] = config.chop;
    let [foam_tile, foam_speed] = config.foam_tile;
    let swell = frame(wind, swell_tile, swell_speed);
    let chop = frame(chop_heading, chop_tile, chop_speed);
    let foam = frame(wind, foam_tile, foam_speed);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "offsets are wrapped to a tile and headings are unit vectors"
    )]
    let f = |v: f64| v as f32;
    WaterParams {
        optics: Vec4::new(
            config.absorption,
            config.max_alpha,
            look.reflection.clamp(0.0, 1.0),
            look.foam.clamp(0.0, 1.0),
        ),
        swell: Vec4::new(f(swell.x), f(swell.y), 1.0 / swell_tile, swell_slope),
        chop: Vec4::new(f(chop.x), f(chop.y), 1.0 / chop_tile, chop_slope),
        axes: Vec4::new(f(wind.x), f(wind.y), f(chop_heading.x), f(chop_heading.y)),
        foam: Vec4::new(
            f(foam.x),
            f(foam.y),
            1.0 / foam_tile,
            config.shore_foam_depth,
        ),
        fade: Vec4::new(
            config.detail_fade[0],
            config.detail_fade[1],
            config.far_roughness,
            config.foam_albedo,
        ),
        crest: Vec4::new(config.crest_threshold, config.shore_foam_reach, 0.0, 0.0),
        flow: {
            let tile = f64::from(river.ripple[0].max(0.01));
            let cycle = f64::from(river.cycle_s.max(0.1));
            Vec4::new(
                f(origin.x.rem_euclid(tile)),
                f(origin.y.rem_euclid(tile)),
                f(1.0 / tile),
                f((elapsed / cycle).rem_euclid(1.0)),
            )
        },
    }
}

/// Sets where the wave image lies this frame and the look's strengths. The
/// resource is marked changed only when the parameters differ, which is
/// every frame while the sim's clock runs; the material is never touched.
fn update_water(
    config: Res<WaterConfig>,
    river: Res<RiverConfig>,
    look: Res<WaterLook>,
    origin: Option<Res<RenderOrigin>>,
    clock: Option<Res<SimClock>>,
    wind: Option<Res<Wind>>,
    frame: Option<ResMut<WaterFrame>>,
) {
    let Some(mut frame) = frame else {
        return;
    };
    let origin = origin.map_or(WorldPos::ZERO, |o| o.0);
    let elapsed = clock.map_or(0.0, |c| c.elapsed_secs());
    let [dx, dz] = wind.map_or([1.0, 0.0], |w| w.direction);
    let wind = DVec2::new(f64::from(dx), f64::from(dz))
        .try_normalize()
        .unwrap_or(DVec2::X);
    let params = water_params(
        &config,
        &river,
        *look,
        DVec2::new(origin.x(), origin.z()),
        wind,
        elapsed,
    );
    if frame.params != params {
        frame.params = params;
    }
}

/// One wave image texel before encoding: the height's slope along and
/// across the wind per tile, the height, the foam noise.
type WaveTexel = [f32; 4];

/// A uniform `[0, 1)` from a hash of `key`.
#[allow(
    clippy::cast_precision_loss,
    reason = "24 bits of the hash are exact in an f32"
)]
fn unit(key: u64) -> f32 {
    (mix64(key) >> 40) as f32 / (1u64 << 24) as f32
}

/// The wave image's texels at mip zero, row-major, `v` down the rows.
///
/// The height is a sum of [`WAVE_COUNT`] cosines with whole cycles per
/// tile, so it repeats exactly; headings within [`WAVE_SPREAD`] of the
/// tile's `+u` (the wind's heading in the shader), lengths log-uniform over
/// [`WAVE_CYCLES`] and amplitudes falling as the length to the 1.5, so the
/// short waves roughen without dominating the slope. The phases index one
/// table of a cycle, since every wave's phase at a texel is a whole number
/// of texels. The foam noise is three octaves of value noise on lattices
/// that divide the tile. Hashes of fixed keys, and the transcendentals
/// through `libm` rather than the platform's: the same image on every
/// machine and every run, so a wave number never rounds the other way.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "cycle counts and texel indices are small"
)]
fn wave_texels() -> Vec<WaveTexel> {
    let n = WAVE_SIZE;
    let tau = std::f32::consts::TAU;
    let table: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let t = tau * i as f32 / n as f32;
            (libm::sinf(t), libm::cosf(t))
        })
        .collect();
    // (cycles along u, along v, amplitude, phase's cos, phase's sin)
    let mut waves = Vec::new();
    for i in 0..WAVE_COUNT {
        let heading = (unit(i * 4 + 1) * 2.0 - 1.0) * WAVE_SPREAD;
        // One in eight runs against the wind, which breaks the crests.
        let heading = if unit(i * 4 + 2) < 0.125 {
            heading + std::f32::consts::PI
        } else {
            heading
        };
        let (lo, hi) = WAVE_CYCLES;
        let cycles = lo * libm::powf(hi / lo, unit(i * 4 + 3));
        let ku = (cycles * libm::cosf(heading)).round() as i64;
        let kv = (cycles * libm::sinf(heading)).round() as i64;
        if ku == 0 && kv == 0 {
            continue;
        }
        let k = ((ku * ku + kv * kv) as f32).sqrt();
        let phase = tau * unit(i * 4 + 4);
        let (s, c) = (libm::sinf(phase), libm::cosf(phase));
        waves.push((ku, kv, libm::powf(k, -1.5), c, s));
    }
    let size = n as i64;
    let mut texels = vec![[0.0_f32; 4]; n * n];
    for row in 0..n {
        for col in 0..n {
            let (mut h, mut du, mut dv) = (0.0, 0.0, 0.0);
            for &(ku, kv, a, pc, ps) in &waves {
                let m = (ku * col as i64 + kv * row as i64).rem_euclid(size) as usize;
                let (ts, tc) = table[m];
                // cos(t + p) and its derivative -sin(t + p).
                let cos = tc * pc - ts * ps;
                let sin = ts * pc + tc * ps;
                h += a * cos;
                du -= a * tau * ku as f32 * sin;
                dv -= a * tau * kv as f32 * sin;
            }
            let (u, v) = (col as f32 / n as f32, row as f32 / n as f32);
            let foam = 0.5 * value_noise(u, v, 8, 11)
                + 0.3 * value_noise(u, v, 16, 12)
                + 0.2 * value_noise(u, v, 32, 13);
            texels[row * n + col] = [du, dv, h, foam];
        }
    }
    // Slopes as a share of the steepest, heights over their range.
    let steepest = texels
        .iter()
        .map(|t| t[0].abs().max(t[1].abs()))
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let (low, high) = texels
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(t[2]), h.max(t[2])));
    let range = (high - low).max(1e-6);
    for t in &mut texels {
        t[0] /= steepest;
        t[1] /= steepest;
        t[2] = (t[2] - low) / range;
    }
    texels
}

/// Value noise at `(u, v)` in the tile on a `period` by `period` lattice,
/// repeating with the tile, smoothly interpolated, in `[0, 1)`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "lattice indices are small and non-negative"
)]
fn value_noise(u: f32, v: f32, period: u64, salt: u64) -> f32 {
    let x = u * period as f32;
    let y = v * period as f32;
    let (ix, iy) = (x.floor(), y.floor());
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(x - ix), smooth(y - iy));
    let at = |dx: u64, dy: u64| {
        let cx = (ix as u64 + dx) % period;
        let cy = (iy as u64 + dy) % period;
        unit(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (cy * period + cx))
    };
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

/// A `[0, 1]` value as a byte, with a signed slope mapped from `[-1, 1]`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a byte's range first"
)]
fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The wave image as linear `Rgba8` (slope along the wind, slope across
/// it, height, foam noise; the slopes centred on one half) with every mip
/// down to one texel, repeated, linear and anisotropic. The mips average
/// the slopes, which is the far water flattening by itself before the
/// shader's fade.
fn wave_image() -> Image {
    let n = WAVE_SIZE;
    let mut level = wave_texels();
    let mut width = n;
    let mut data = Vec::new();
    let mut mips = 0_u32;
    loop {
        for t in &level {
            data.extend([
                to_byte(0.5 + 0.5 * t[0]),
                to_byte(0.5 + 0.5 * t[1]),
                to_byte(t[2]),
                to_byte(t[3]),
            ]);
        }
        mips += 1;
        if width == 1 {
            break;
        }
        let half = width / 2;
        let mut next = vec![[0.0; 4]; half * half];
        for row in 0..half {
            for col in 0..half {
                let at = |r: usize, c: usize, k: usize| level[r * width + c][k];
                next[row * half + col] = std::array::from_fn(|k| {
                    0.25 * (at(2 * row, 2 * col, k)
                        + at(2 * row, 2 * col + 1, k)
                        + at(2 * row + 1, 2 * col, k)
                        + at(2 * row + 1, 2 * col + 1, k))
                });
            }
        }
        level = next;
        width = half;
    }
    #[allow(clippy::cast_possible_truncation, reason = "a 256 texel side")]
    let side = n as u32;
    let mut image = Image::new_uninit(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.label = Some("water_waves");
    image.texture_descriptor.mip_level_count = mips;
    image.data_order = TextureDataOrder::LayerMajor;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("water_waves".to_owned()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    image
}

/// Centres the plane under the primary observer. Runs in `Update`, before
/// the origin sync in `PostUpdate` derives the transform.
fn follow_observer(
    observers: Res<Observers>,
    mut planes: Query<&mut WorldPosition, With<WaterPlane>>,
) {
    let Some(observer) = observers.primary else {
        return;
    };
    for mut position in &mut planes {
        let next = WorldPos::new(observer.x(), position.0.y(), observer.z());
        if next != position.0 {
            position.0 = next;
        }
    }
}

/// Shows the sea once there is terrain to show it against.
fn reveal_water(
    stats: Option<Res<TerrainStats>>,
    mut planes: Query<&mut Visibility, With<WaterPlane>>,
) {
    if stats.is_none_or(|s| s.built == 0) {
        return;
    }
    for mut visibility in &mut planes {
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}

/// The water shader reads the depth prepass; every 3D camera that looks at
/// the world gets one.
fn add_depth_prepass(
    mut commands: Commands,
    cameras: Query<
        Entity,
        (
            With<Camera3d>,
            Without<DepthPrepass>,
            Without<crate::OffWorld>,
        ),
    >,
) {
    for camera in &cameras {
        commands.entity(camera).insert(DepthPrepass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_app::PrimaryObserver;

    #[test]
    fn the_plane_follows_the_observer_without_a_renderer() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, strata_app::AppPlugin, WaterPlugin));
        app.world_mut()
            .spawn((PrimaryObserver, WorldPosition::new(500.0, 30.0, -200.0)));
        for _ in 0..4 {
            app.update();
        }
        // No mesh assets: nothing spawned, nothing panicked.
        let mut planes = app
            .world_mut()
            .query_filtered::<&WorldPosition, With<WaterPlane>>();
        assert_eq!(planes.iter(app.world()).count(), 0);
        // With a plane spawned by hand it follows.
        app.world_mut()
            .spawn((WaterPlane, WorldPosition::new(0.0, 0.0, 0.0)));
        app.update();
        let pos = planes.single(app.world()).expect("one plane");
        assert_eq!(pos.0, WorldPos::new(500.0, 0.0, -200.0));
    }

    #[test]
    fn the_plane_is_hidden_until_terrain_is_built() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, strata_app::AppPlugin, WaterPlugin));
        app.init_resource::<TerrainStats>();
        for _ in 0..3 {
            app.update();
        }
        let plane = app
            .world_mut()
            .spawn((WaterPlane, WorldPosition::default(), Visibility::Hidden))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(plane),
            Some(&Visibility::Hidden)
        );
        app.world_mut().resource_mut::<TerrainStats>().built = 1;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(plane),
            Some(&Visibility::Inherited)
        );
    }

    /// The image repeats: the step from the last column (row) to the first
    /// is no larger than the steps inside the tile, in every channel, so the
    /// tile's seam does not show on the sea.
    #[test]
    fn the_wave_image_tiles_without_a_seam() {
        let texels = wave_texels();
        let n = WAVE_SIZE;
        let at = |row: usize, col: usize| texels[row * n + col];
        for k in 0..4 {
            let mut inside = 0.0_f32;
            let mut seam = 0.0_f32;
            for i in 0..n {
                for j in 0..n {
                    let step_c = (at(i, (j + 1) % n)[k] - at(i, j)[k]).abs();
                    let step_r = (at((i + 1) % n, j)[k] - at(i, j)[k]).abs();
                    if j + 1 == n {
                        seam = seam.max(step_c);
                    } else {
                        inside = inside.max(step_c);
                    }
                    if i + 1 == n {
                        seam = seam.max(step_r);
                    } else {
                        inside = inside.max(step_r);
                    }
                }
            }
            assert!(
                seam <= inside + 1e-4,
                "channel {k}: seam {seam} inside {inside}"
            );
        }
    }

    /// Every channel spans its range: the slopes reach one, the height and
    /// the foam noise cover most of `[0, 1]`, so the shader's thresholds
    /// mean the same on any seed of the constants.
    #[test]
    fn the_wave_image_fills_its_channels() {
        let texels = wave_texels();
        let span = |k: usize| {
            texels
                .iter()
                .fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(t[k]), h.max(t[k])))
        };
        let (su, sv) = (span(0), span(1));
        assert!((su.0.abs().max(su.1).max(sv.0.abs()).max(sv.1) - 1.0).abs() < 1e-5);
        assert_eq!(span(2), (0.0, 1.0));
        let foam = span(3);
        assert!(
            foam.0 >= 0.0 && foam.1 < 1.0 && foam.1 - foam.0 > 0.5,
            "{foam:?}"
        );
        assert_eq!(texels, wave_texels(), "the image is the same every time");
    }

    /// The buffer holds the shader's `WaterParams` as it declares it: eight
    /// `vec4`s in the struct's order, nothing between them, so a field
    /// added on one side only shows here and not as a wrong-looking sea.
    #[test]
    fn the_parameter_bytes_are_the_shaders_layout() {
        let params = water_params(
            &WaterConfig::default(),
            &RiverConfig::default(),
            WaterLook::default(),
            DVec2::new(1_234.5, -678.9),
            DVec2::new(0.6, 0.8),
            321.0,
        );
        let bytes = water_bytes(&params);
        assert_eq!(bytes.len(), 8 * 16);
        let fields = [
            params.optics,
            params.swell,
            params.chop,
            params.axes,
            params.foam,
            params.fade,
            params.crest,
            params.flow,
        ];
        for (i, field) in fields.iter().enumerate() {
            for (j, v) in field.to_array().iter().enumerate() {
                let at = i * 16 + j * 4;
                let read = f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
                assert_eq!(read.to_bits(), v.to_bits(), "field {i} lane {j}");
            }
        }
    }

    /// The layers' frames are wrapped to a tile, and a rebase leaves the
    /// waves where they were: the texture coordinate of one world point is
    /// the same before and after the origin moves under it.
    #[test]
    fn the_layers_follow_the_world_not_the_origin() {
        let config = WaterConfig::default();
        let look = WaterLook::default();
        let wind = DVec2::new(0.8, 0.6);
        let river = RiverConfig::default();
        let a = water_params(&config, &river, look, DVec2::ZERO, wind, 12_345.6);
        let shift = DVec2::new(2_000.0, -1_500.0);
        let b = water_params(&config, &river, look, shift, wind, 12_345.6);
        for (layer_a, layer_b, heading) in [
            (a.swell, b.swell, a.axes.truncate().truncate()),
            (a.chop, b.chop, bevy::math::Vec2::new(a.axes.z, a.axes.w)),
        ] {
            for v in [layer_a.x, layer_a.y, layer_b.x, layer_b.y] {
                assert!((0.0..1.0).contains(&v), "{v}");
            }
            // A world point at the old render-space p is at p - shift now.
            let heading = heading.as_dvec2();
            let p = DVec2::new(300.0, 40.0);
            let inv = f64::from(layer_a.z);
            let uv = |q: DVec2, l: Vec4| {
                DVec2::new(q.dot(heading), q.dot(heading.perp())) * inv
                    + DVec2::new(f64::from(l.x), f64::from(l.y))
            };
            let before = uv(p, layer_a);
            let after = uv(p - shift, layer_b);
            let d = (before - after).map(|v| (v - v.round()).abs());
            assert!(d.x < 1e-4 && d.y < 1e-4, "{d:?}");
        }
    }
}
