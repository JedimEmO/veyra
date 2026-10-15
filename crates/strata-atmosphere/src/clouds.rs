//! A 2D cloud layer and the shadows it casts on the ground (the lighting
//! plan's phase 2b): the moving light across the land and the sky's scale,
//! for a fraction of what a ray-marched volume would cost.
//!
//! One image, generated at start-up from the world seed ([`density_field`]),
//! serves every half. Its detail density (heaps of three sizes, domain
//! warped, with a ridged crease) spans one tile of [`SkySettings::cloud_scale`]
//! metres; its coverage field spans [`COVER_TILES`] tiles and gates the
//! detail, so clouds gather in groups kilometres across with clear sky
//! between them; its slope channel fades every edge over the same distance.
//! [`opacity`] turns the three into how opaque the layer is, the same
//! function in the shader and on the CPU:
//!
//! - **The layer** is a horizontal quad [`LAYER_ALTITUDE`] above sea level
//!   that follows the observer as the planet does, drawn in the transparent
//!   pass by [`CloudMaterial`] (`shaders/clouds.wgsl`): premultiplied alpha,
//!   no depth write, no shadow. It lights itself from the scene's
//!   directional lights through the atmosphere's transmittance at its own
//!   height, so a dawn sun reddens and dims it, and fades by the
//!   transmittance between the camera and the cloud, so a far cloud sinks
//!   into the haze. The quad's edge never shows: the cloud fades to nothing
//!   over the last ten degrees above the horizon.
//! - **The volume** is shading, not geometry. The shader treats the coarse
//!   density over the threshold as a height field
//!   [`SkySettings::cloud_thickness`] tall: three coarse reads round each
//!   point give its height and slope, so a face turned to the sun is lit by
//!   its normal and one turned away (a base under a high sun, a heap's
//!   wall against a low one) by the light through the body, dim through a
//!   deep heart and bright through a thin rim, with a forward lobe through
//!   the thinnest parts for the silver lining. A small parallax step pushes
//!   a base towards the eye. The sky lights a top and, through it, a base;
//!   the sunlit ground bounces a touch of its colour onto a base.
//! - **The upper deck** is a second quad of the same image at
//!   [`UPPER_ALTITUDE`], on a smaller tile, shifted, thinner, softer and at
//!   [`SkySettings::cloud_upper_cover`] of the cover, drifting
//!   [`SkySettings::cloud_shear`] times as fast along a wind veered 20
//!   degrees, so the two decks slide past each other. The transparent sort
//!   draws the deck farther from the eye first ([`CloudMaterial::sort_bias`]),
//!   so the nearer one composes over it. It casts no shadow; the sun's veil
//!   reads both.
//! - **The shadow** is Bevy's light texture on the sun
//!   ([`DirectionalLightTexture`], `pbr_light_textures`): the layer's
//!   opacity at the cover's threshold over the coverage's whole repeat
//!   ([`MASK_SIZE`] squared, made on the async compute pool when the cover
//!   changes), blurred a little, read
//!   per fragment by every material in the main pass. Bevy projects it
//!   along the light's own `x` and `y` through the inverse of the light's
//!   `GlobalTransform`. A rotation and a scale would project the mask in the
//!   plane square to the sun, which does not match a horizontal layer: the
//!   shadow would stretch and swing with the sun while the clouds above it
//!   did not. So once Bevy has built the cascades and the light's frusta
//!   from the sun's rotation, `project_shadow` replaces the sun's
//!   `GlobalTransform` with an affine map whose first two rows send a point
//!   on the ground to the layer's texture coordinates of the cloud that
//!   stands between it and the sun, and whose third column is the direction
//!   to the sun, which is all Bevy's lighting reads of it. A cloud's shadow
//!   therefore lies exactly where the cloud blocks the sun, ten kilometres
//!   away at dawn, and moves with it. `restore_sun_transform` puts the
//!   rotation back before the next frame's transform propagation. Under
//!   [`MIN_SUN_HEIGHT`] the sun carries no texture and keeps its rotation:
//!   the projection would have to lift the light's direction over the
//!   horizon there, and Bevy lights the scene, the sky and the sun disc
//!   along it.
//! - **The fog** under a cloud is shaded too: the valley fog's pass
//!   (`fog_pass`) reads the same mask at each step through
//!   `fog_shadow`'s copy of the projection, and its air floor takes the
//!   mask's mean. Bevy's own volumetric pass, which draws the local fog
//!   volumes, does not read light textures, so those stay sunlit under a
//!   cloud; they are tens of metres across and under the canopies.
//! - **The sun under cloud**: [`SunVeil`] is what both decks let through
//!   where the line of sight to the sun crosses them ([`sun_clear`]),
//!   worked out on the CPU each frame; the sun disc (and the bloom's halo
//!   that grows from it) and the light rays are scaled by it, so they go
//!   behind a cloud and come back as it clears.
//!
//! The wind moves the image by the sim's clock along the sim's wind
//! direction ([`strata_sim::Wind`], fixed today), at
//! [`SkySettings::cloud_speed`], so two clients at the same tick see the
//! same shadows. The finer read that frays a cloud's edge slides through it
//! across the wind at [`SkySettings::cloud_evolution`], so shapes change as
//! they pass; the shadow and the veil leave that read out, so it never
//! takes them out of step with the cloud. [`SkySettings::cloud_cover`] picks the threshold as a
//! quantile of each texel's [`half_opaque`] threshold ([`cover_ladder`]),
//! so 0.45 is at least half-opaque cloud over 45 % of the sky; at zero the
//! layer is hidden and the sun loses its texture.
//!
//! The generator, the ladder and the mask use only additions,
//! multiplications, divisions, `floor` and `sqrt` (the mask's exponential
//! is ten squarings), all correctly rounded, so they are the same bits on
//! every machine, as the world's own noise is.

use crate::{SkySettings, Sun};
use bevy::app::{App, PostUpdate, Update};
use bevy::asset::{Asset, AssetEventSystems, Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::{Visibility, VisibilitySystems};
use bevy::ecs::prelude::*;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{
    DirectionalLightTexture, NotShadowCaster, NotShadowReceiver, SimulationLightSystems,
};
use bevy::material::AlphaMode;
use bevy::math::primitives::Plane3d;
use bevy::math::{Affine3A, DMat3, DVec2, DVec3, Mat3A, Vec3, Vec3A, Vec4};
use bevy::mesh::{Mesh, Mesh3d, MeshVertexBufferLayoutRef, Meshable};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshMaterial3d};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDataOrder, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::TransformSystems;
use bevy::transform::components::{GlobalTransform, Transform};
use std::sync::Arc;
use strata_core::{Seed, WorldPos, mix64};
use strata_sim::{
    Observers, OriginSet, RenderOrigin, SimClock, Wind, WorldPosition, WorldSeed, WorldSourceRes,
};

/// The layer's shader, loaded from disk like the fog's.
pub const CLOUD_SHADER_PATH: &str = "shaders/clouds.wgsl";

/// Texels along a side of the density image. At the default 6 km tile one
/// texel is 11.7 m, finer than a cloud shadow's own soft edge.
pub const DENSITY_SIZE: usize = 512;

/// The layer's height above sea level, metres: the base of fair-weather
/// cumulus over a temperate coast, and under the ranges' highest peaks, so
/// a summit can stand in it.
pub const LAYER_ALTITUDE: f64 = 1_800.0;

/// The upper deck's height above sea level, metres: a thinner, faster
/// layer of altocumulus 800 m over the cumulus, drawn behind it, so the
/// two move against each other and the sky has depth.
pub const UPPER_ALTITUDE: f64 = 2_600.0;

/// The upper deck's tile as a share of [`SkySettings::cloud_scale`]: the
/// same image smaller, so its heaps read as a finer, more distant deck and
/// not as a copy of the lower one.
const UPPER_SCALE: f64 = 0.75;

/// Where the upper deck's image lies against the lower's, metres, so the
/// two never line up.
const UPPER_SHIFT: DVec2 = DVec2::new(2_380.0, -3_910.0);

/// How far the wind turns between the decks, radians: it veers with
/// height, so the upper deck crosses the lower one at an angle.
const UPPER_VEER: f64 = 0.35;

/// The upper deck's optical depth as a share of the lower's: altocumulus
/// is a few hundred metres of thin cloud, greyer and more translucent.
const UPPER_TAU: f32 = 0.7;

/// The upper deck's cover ramp as a multiple of [`COVER_RAMP`]: at its
/// higher threshold only the heaps' tops pass it, and the lower deck's
/// ramp cut them out like paper.
const UPPER_RAMP: f32 = 1.8;

/// How far a base bulges under the deck, as a share of
/// [`SkySettings::cloud_thickness`]: a cumulus is flatter below than above.
const BASE_BULGE: f32 = 0.4;

/// The parallax step, as `clouds.wgsl` has it: the lift as a share of the
/// base's bulge, and the most the step runs per metre of lift.
const PARALLAX: f64 = 0.1;
const PARALLAX_RUN: f64 = 2.0;

/// How far the deck nearer the camera is pulled forward in the transparent
/// pass's sort, metres of view depth: more than the decks' 800 m apart, so
/// it is drawn after the far one and composes over it, from under both
/// decks and from over both. The near deck is pulled forward rather than
/// the far one pushed back: pushed back, the lower deck seen steeply from
/// over both sorts behind the sea and the sea is blended over it, while
/// the near deck is always the one closest to the eye.
const DECK_SORT: f32 = 1_000.0;

/// The side of the layer's quad, metres. From sea level its edge is under
/// a degree above the horizon, where the fade has taken the cloud already.
const LAYER_EXTENT: f32 = 240_000.0;

/// How far the cover's ramp runs, in density: a cloud goes from nothing to
/// fully grown over this much density over the threshold, which sets how
/// soft its edge is. The shader reads it from the uniform.
const COVER_RAMP: f32 = 0.12;

/// The sine of the band over the horizon the layer fades out in (ten
/// degrees), as `clouds.wgsl` has it.
const HORIZON_BAND: f32 = 0.173_65;

/// The ramp at a group's fringe, as a multiple of [`COVER_RAMP`]: the
/// clouds at the edge of a group grow over a wider band of density, so they
/// stay thin and wispy where the group's middle is full.
const EDGE_RAMP: f32 = 1.8;

/// The density's slope, per tile, that the image's slope channel holds as
/// one.
const SLOPE_RANGE: f32 = 24.0;

/// How far inside its edge a cloud reaches full opacity, in tiles: 0.02 is
/// 120 m at the default 6 km tile, so an edge is soft over the same
/// distance where the noise rises steeply and where it rises slowly.
const EDGE_FADE: f32 = 0.02;

/// The optical depth at a group's fringe as a share of that in its middle:
/// a group thins towards its edge.
const THIN_GROUP: f32 = 0.55;

/// Optical depth at a cloud's edge and at its densest.
const TAU_EDGE: f32 = 2.0;
const TAU_HEART: f32 = 10.0;

/// The domain warp's reach, in tiles: the detail noise is read up to this
/// far from where it is drawn, which drags round heaps into ragged ones.
const WARP: f64 = 0.12;

/// The share of the sun a fully grown cloud takes off the ground. Not all:
/// a cumulus a few hundred metres thick still passes some diffuse light
/// down, and a shadow that blacked out the sun would read as night under
/// the sky fill's blue.
const SHADOW_STRENGTH: f32 = 0.85;

/// The coverage field's repeat, in tiles of the density image: the groups
/// it gathers the clouds into are several kilometres across, larger than a
/// tile, and the tile's own repeat no longer shows in where clouds gather.
pub const COVER_TILES: usize = 4;

/// Texels along a side of the shadow mask, which spans [`COVER_TILES`]
/// tiles: 23 m a texel at the default 6 km tile.
pub const MASK_SIZE: usize = 1024;

/// Density texels per mask texel along a side.
const MASK_STEP: usize = DENSITY_SIZE * COVER_TILES / MASK_SIZE;

/// The share of the detail density the gaps between groups keep: low
/// enough that a gap is mostly clear sky at the default cover, high enough
/// that a big heap still stands in it alone.
const GATE_FLOOR: f32 = 0.62;

/// Texels of box blur, applied twice, on the shadow mask: a sun half a
/// degree wide over a cloud two kilometres up softens its shadow's edge by
/// about twenty metres, one 23 m texel.
const MASK_BLUR: usize = 1;

/// The lowest sun height (the `y` of the unit vector towards it) at which
/// the sun carries the cloud shadow. The projection's third column is the
/// light's direction, and under this height it could only keep the shadow
/// finite by lifting that direction, which Bevy would light the scene, draw
/// the sky and place the sun disc by: the sun would shine on for minutes
/// after it set. A shadow cast from a cloud a hundred times its height away
/// is worth nothing anyway, so under it the sun keeps its own rotation and
/// carries no texture.
pub const MIN_SUN_HEIGHT: f32 = 0.01;

/// The layer's per-frame parameters. Mirrored field for field by
/// `CloudParams` in `shaders/clouds.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct CloudParams {
    /// The texture coordinate offset in `xy` (render-space `xz` times `z`
    /// plus this is the image's coordinate), one over the tile's size in
    /// metres in `z`, the layer's height in render space in `w`.
    pub layer: Vec4,
    /// The cover's threshold in density in `x`, the cover's ramp in `y`, half
    /// the quad's side in `z`, the cover in `w`.
    pub shape: Vec4,
    /// The unit vector towards the sun in `xyz`.
    pub sun: Vec4,
    /// How the edges are drawn, the constants [`opacity`] uses, so the
    /// shader and the CPU's mask and veil cannot drift apart:
    /// `EDGE_RAMP`, `SLOPE_RANGE`, `EDGE_FADE` and `THIN_GROUP`.
    pub edge: Vec4,
    /// `TAU_EDGE` and `TAU_HEART` in `xy`, `HORIZON_BAND` in `z`, the
    /// deck's own scale on both in `w` (one for the lower deck,
    /// `UPPER_TAU` for the upper).
    pub depth: Vec4,
    /// The coverage field's frame, which repeats every [`COVER_TILES`]
    /// tiles: its texture coordinate offset in `xy` and one over its size in
    /// metres in `z`, as `layer` has the tile's; `GATE_FLOOR` in `w`.
    pub cover: Vec4,
    /// The height field the shader shades the density as: its height at a
    /// heart in metres in `x`, the fraying read's offset in its own
    /// repeats in `yz` (the evolution), `BASE_BULGE` in `w`.
    pub relief: Vec4,
}

/// The layer's material: the density image and how to read it this frame.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CloudMaterial {
    /// Where the image lies and how much of it is cloud.
    #[uniform(0)]
    pub params: CloudParams,
    /// The density image, repeated, with mips for the far layer.
    #[texture(1)]
    #[sampler(2)]
    pub density: Handle<Image>,
    /// Added to the deck's view depth in the transparent pass's sort, so
    /// the nearer deck is drawn last (`DECK_SORT`); not bound.
    pub sort_bias: f32,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        CLOUD_SHADER_PATH.into()
    }

    /// Premultiplied so the haze between the camera and a cloud can tint
    /// it channel by channel while one alpha lets the sky through.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn depth_bias(&self) -> f32 {
        self.sort_bias
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// Both faces: the layer is seen from below on the ground and from
    /// above in flight.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Which of the two decks a cloud quad draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CloudDeck {
    /// The cumulus at [`LAYER_ALTITUDE`], which casts the shadow.
    #[default]
    Lower,
    /// The thinner, faster deck at [`UPPER_ALTITUDE`], drawn behind it.
    Upper,
}

/// A cloud deck's quad.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CloudLayer(pub CloudDeck);

/// The generated image and what is derived from it.
#[derive(Resource)]
pub struct CloudField {
    /// The baked image, shared with the mask's task.
    texels: Arc<CloudTexels>,
    /// Each mask texel's [`half_opaque`] threshold, sorted, so a cover is
    /// a quantile of it.
    sorted: Arc<Vec<f32>>,
    /// The shadow mask being made for a new cover, off the frame.
    remask: Option<Task<Remask>>,
    /// The shadow mask the sun carries.
    mask: Handle<Image>,
    /// The lower deck's material.
    material: Handle<CloudMaterial>,
    /// The upper deck's.
    upper: Handle<CloudMaterial>,
    /// The cover the mask and the threshold were last made for, `None`
    /// before the first.
    cover: Option<f32>,
    /// The threshold for that cover.
    threshold: f32,
    /// The mask's mean, `0` to `1`: the share of the sun that gets through
    /// the layer on average, for the valley fog's air floor, which is too
    /// long a path to read the mask along.
    mean_light: f32,
}

impl CloudField {
    /// The shadow mask the sun carries, which the fog pass binds too.
    #[must_use]
    pub fn mask(&self) -> &Handle<Image> {
        &self.mask
    }
}

/// A shadow mask made for a cover on the async compute pool: a mask of
/// 1024 x 1024 and its blur take tens of milliseconds, a hitch on the F6
/// panel's slider, so the layer keeps the old cover's threshold and mask
/// until both are ready and then takes them together.
struct Remask {
    cover: f32,
    threshold: f32,
    mask: Vec<u8>,
    mean_light: f32,
}

/// Where the shadow mask lies this frame, for the sun's projection and the
/// fog pass's read of the mask: the coverage field's frame, which the mask
/// spans.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CloudProjection {
    /// Whether the sun should carry the mask.
    active: bool,
    /// The mask's texture coordinate offset, as in [`CloudParams::cover`].
    offset: DVec2,
    /// One over the mask's size, metres.
    inv_scale: f64,
    /// The layer's height in render space.
    height: f64,
    /// The mask's mean, as [`CloudField`] keeps it.
    mean_light: f32,
}

/// How much of the sun the cloud layer lets through towards the observer
/// this frame: one less the layer's [`opacity`] where the line of sight to
/// the sun crosses it, faded out with the layer over the horizon's band.
/// The sun disc's intensity, and with it the bloom's halo, which grows from
/// the disc, and the light rays' strength are scaled by it, so a sun behind
/// a cloud loses its disc, its glare and its shafts and gets them back as
/// the cloud drifts off. The layer's alpha already dims the disc drawn
/// behind it, but a disc ten thousand times the sky's brightness still
/// glared through a cloud at 99 % opacity.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct SunVeil {
    /// `0` behind a full cloud, `1` in clear sky.
    pub clear: f32,
}

impl Default for SunVeil {
    fn default() -> Self {
        Self { clear: 1.0 }
    }
}

/// The density being generated off the frame.
#[derive(Resource)]
struct CloudBake(Task<(CloudTexels, Vec<f32>)>);

pub(crate) fn register(app: &mut App) {
    app.init_resource::<CloudProjection>()
        .init_resource::<SunVeil>()
        .add_systems(Update, (start_bake, finish_bake))
        .add_systems(
            PostUpdate,
            (
                update_layer
                    .after(OriginSet::Rebase)
                    .before(OriginSet::Sync)
                    .before(AssetEventSystems),
                restore_sun_transform.before(TransformSystems::Propagate),
                project_shadow
                    .after(update_layer)
                    .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                    .after(SimulationLightSystems::UpdateLightFrusta)
                    .after(SimulationLightSystems::CheckLightVisibility)
                    .after(VisibilitySystems::CheckVisibility),
            ),
        );
    // The material only exists with a renderer; the plugin is still usable
    // in a headless test app.
    if app.is_plugin_added::<bevy::render::RenderPlugin>() {
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default());
    }
}

/// Starts generating the density once the world's seed is known, on the
/// async compute pool: about half a second of work (the image and the
/// cover's ladder) that would otherwise be a hitch.
fn start_bake(
    mut commands: Commands,
    seed: Option<Res<WorldSeed>>,
    field: Option<Res<CloudField>>,
    bake: Option<Res<CloudBake>>,
    materials: Option<Res<Assets<CloudMaterial>>>,
) {
    let Some(seed) = seed else {
        return;
    };
    if field.is_some() || bake.is_some() || materials.is_none() {
        return;
    }
    let seed = seed.0;
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
    commands.insert_resource(CloudBake(pool.spawn(async move {
        let texels = density_field(seed);
        let sorted = cover_ladder(&texels);
        (texels, sorted)
    })));
}

/// Builds the images, the material and the layer once the density is in.
fn finish_bake(
    mut commands: Commands,
    bake: Option<ResMut<CloudBake>>,
    images: Option<ResMut<Assets<Image>>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<CloudMaterial>>>,
) {
    let Some(mut bake) = bake else {
        return;
    };
    let Some((texels, sorted)) = block_on(poll_once(&mut bake.0)) else {
        return;
    };
    commands.remove_resource::<CloudBake>();
    let (Some(mut images), Some(mut meshes), Some(mut materials)) = (images, meshes, materials)
    else {
        return;
    };
    let density = images.add(density_image(&texels));
    let mask = images.add(mask_image(vec![255; MASK_SIZE * MASK_SIZE]));
    let quad = meshes.add(Plane3d::default().mesh().size(LAYER_EXTENT, LAYER_EXTENT));
    let mut deck = |name: &'static str, which: CloudDeck, altitude: f64| {
        let material = materials.add(CloudMaterial {
            params: CloudParams::default(),
            density: density.clone(),
            sort_bias: 0.0,
        });
        commands.spawn((
            Name::new(name),
            CloudLayer(which),
            Mesh3d(quad.clone()),
            MeshMaterial3d(material.clone()),
            WorldPosition::new(0.0, altitude, 0.0),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Visibility::Hidden,
            NotShadowCaster,
            NotShadowReceiver,
        ));
        material
    };
    let material = deck("cloud layer", CloudDeck::Lower, LAYER_ALTITUDE);
    let upper = deck("upper cloud layer", CloudDeck::Upper, UPPER_ALTITUDE);
    commands.insert_resource(CloudField {
        texels: Arc::new(texels),
        sorted: Arc::new(sorted),
        remask: None,
        mask,
        material,
        upper,
        cover: None,
        threshold: 1.0,
        mean_light: 1.0,
    });
}

/// Keeps the decks over the observer, remakes the shadow mask when the
/// cover changes, and writes where each deck's image lies this frame. Runs
/// after the origin's rebase and before the sync, so the decks' transforms,
/// their texture offsets and the sun's projection agree on one origin, and
/// before the asset events, so the materials' new offsets reach this frame.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
pub(crate) fn update_layer(
    settings: Res<SkySettings>,
    observers: Res<Observers>,
    origin: Option<Res<RenderOrigin>>,
    clock: Option<Res<SimClock>>,
    wind: Option<Res<Wind>>,
    source: Option<Res<WorldSourceRes>>,
    field: Option<ResMut<CloudField>>,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<CloudMaterial>>>,
    suns: Query<&Transform, With<Sun>>,
    mut layers: Query<(&CloudLayer, &mut WorldPosition, &mut Visibility)>,
    mut projection: ResMut<CloudProjection>,
    mut veil: ResMut<SunVeil>,
) {
    let (Some(mut field), Some(mut images), Some(mut materials)) = (field, images, materials)
    else {
        return;
    };
    let cover = settings.cloud_cover.clamp(0.0, 1.0);
    refresh_mask(&mut field, &mut images, cover);
    let sea_level = source.map_or(0.0, |s| f64::from(s.source().sea_level()));
    let origin = origin.map_or(WorldPos::ZERO, |o| o.0);
    let elapsed = clock.map_or(0.0, |c| c.elapsed_secs());
    let [dx, dz] = wind.map_or([1.0, 0.0], |w| w.direction);
    // Drawn with the cover the threshold was made for, so the layer and its
    // shadow change together when a new cover's mask is ready.
    let shown = field.cover.unwrap_or(cover);
    let active = shown > 0.0;
    let upper_cover = (shown * settings.cloud_upper_cover.max(0.0)).min(1.0);
    let [lower, upper] = deck_frames(
        &settings,
        DVec2::new(f64::from(dx), f64::from(dz)),
        elapsed,
        sea_level,
        [field.threshold, cover_threshold(&field.sorted, upper_cover)],
    );
    // The fraying read slides across the wind through the cloud, in its
    // own repeats, wrapped in f64 as the offsets are.
    let fray = (DVec2::new(f64::from(dx), f64::from(dz)).perp()
        * f64::from(settings.cloud_evolution)
        * elapsed)
        .map(|v| v.rem_euclid(1.0));
    // The shadow mask spans the lower deck's coverage frame.
    #[allow(clippy::cast_precision_loss, reason = "a small count of tiles")]
    let cover_inv_scale = lower.inv_scale / COVER_TILES as f64;
    *projection = CloudProjection {
        active,
        offset: ((DVec2::new(origin.x(), origin.z()) - lower.drift) * cover_inv_scale)
            .map(|v| v.rem_euclid(1.0)),
        inv_scale: cover_inv_scale,
        height: lower.altitude - origin.y(),
        mean_light: field.mean_light,
    };
    let to_sun = suns.iter().next().map_or(Vec3::Y, |t| t.rotation * Vec3::Z);
    // The disc goes behind a cloud of either deck; the ground's shadow is
    // the lower deck's alone.
    let clear = match observers.primary {
        Some(eye) if active => {
            let eye = DVec3::new(eye.x(), eye.y(), eye.z());
            sun_clear(&field.texels, &lower, to_sun, eye)
                * sun_clear(&field.texels, &upper, to_sun, eye)
        }
        _ => 1.0,
    };
    veil.set_if_neq(SunVeil { clear });
    // The deck nearer the eye is sorted forward, so it composes over the
    // far one whichever side of them the eye is on.
    let eye_y = observers.primary.map_or(origin.y(), WorldPos::y);
    let lower_far = eye_y > f64::midpoint(lower.altitude, upper.altitude);
    for (frame, handle, far) in [
        (&lower, &field.material, lower_far),
        (&upper, &field.upper, !lower_far),
    ] {
        let params = deck_params(frame, origin, to_sun, shown, fray);
        let sort_bias = if far { 0.0 } else { DECK_SORT };
        // `AssetMut` marks the material changed only when written through.
        if let Some(mut material) = materials.get_mut(handle)
            && (material.params != params || material.sort_bias.to_bits() != sort_bias.to_bits())
        {
            material.params = params;
            material.sort_bias = sort_bias;
        }
    }
    for (CloudLayer(deck), mut position, mut visibility) in &mut layers {
        let (frame, drawn) = match deck {
            CloudDeck::Lower => (&lower, active),
            CloudDeck::Upper => (&upper, active && upper_cover > 0.0),
        };
        if let Some(observer) = observers.primary {
            let next = WorldPos::new(observer.x(), frame.altitude, observer.z());
            if next != position.0 {
                position.0 = next;
            }
        }
        visibility.set_if_neq(if drawn {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Where one deck's image lies on the CPU: what its uniform says, in the
/// world's own metres, for the sun's veil and the deck's parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckFrame {
    /// The cover's threshold the deck is drawn with, from [`cover_ladder`].
    pub threshold: f32,
    /// The deck's height, world `y`.
    pub altitude: f64,
    /// How far the image has moved, metres: the wind's drift since the
    /// clock started, and the upper deck's fixed shift against the lower.
    pub drift: DVec2,
    /// One over the deck's tile, metres.
    pub inv_scale: f64,
    /// The height field's height at a heart, metres; a base bulges
    /// `BASE_BULGE` of it.
    pub thickness: f64,
    /// The deck's optical depth as a share of the lower deck's.
    pub tau: f32,
    /// The deck's cover ramp as a multiple of the lower deck's.
    pub ramp: f32,
}

/// The two decks' frames at `elapsed` seconds of sim time, under a wind
/// blowing along the unit `wind`: the lower deck at
/// [`SkySettings::cloud_speed`], the upper one [`SkySettings::cloud_shear`]
/// times as fast, the wind veered by `UPPER_VEER`, on a tile `UPPER_SCALE`
/// as large and shifted by `UPPER_SHIFT`. The drift is in f64, so neither
/// the distance travelled nor the time played loses the f32 the shader
/// gets once the offsets are wrapped.
#[must_use]
pub fn deck_frames(
    settings: &SkySettings,
    wind: DVec2,
    elapsed: f64,
    sea_level: f64,
    [lower_threshold, upper_threshold]: [f32; 2],
) -> [DeckFrame; 2] {
    let speed = f64::from(settings.cloud_speed);
    let inv_scale = 1.0 / f64::from(settings.cloud_scale.max(1.0));
    let thickness = f64::from(settings.cloud_thickness.max(0.0));
    [
        DeckFrame {
            threshold: lower_threshold,
            altitude: sea_level + LAYER_ALTITUDE,
            drift: wind * speed * elapsed,
            inv_scale,
            thickness,
            tau: 1.0,
            ramp: 1.0,
        },
        DeckFrame {
            threshold: upper_threshold,
            altitude: sea_level + UPPER_ALTITUDE,
            drift: DVec2::from_angle(UPPER_VEER).rotate(wind)
                * speed
                * f64::from(settings.cloud_shear)
                * elapsed
                + UPPER_SHIFT,
            inv_scale: inv_scale / UPPER_SCALE,
            thickness: thickness * UPPER_SCALE,
            tau: UPPER_TAU,
            ramp: UPPER_RAMP,
        },
    ]
}

/// A deck's uniform: its image's offsets wrapped to a tile (render-space
/// `xz` times the scale plus the offset is the image's coordinate; the
/// origin puts render space back into the world, the drift carries the
/// clouds downwind) and its height in render space.
fn deck_params(
    frame: &DeckFrame,
    origin: WorldPos,
    to_sun: Vec3,
    shown: f32,
    fray: DVec2,
) -> CloudParams {
    let shifted = DVec2::new(origin.x(), origin.z()) - frame.drift;
    let offset = (shifted * frame.inv_scale).map(|v| v.rem_euclid(1.0));
    #[allow(clippy::cast_precision_loss, reason = "a small count of tiles")]
    let cover_inv_scale = frame.inv_scale / COVER_TILES as f64;
    let cover_offset = (shifted * cover_inv_scale).map(|v| v.rem_euclid(1.0));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the offsets are wrapped to a tile, the height and scale are metres"
    )]
    CloudParams {
        layer: Vec4::new(
            offset.x as f32,
            offset.y as f32,
            frame.inv_scale as f32,
            (frame.altitude - origin.y()) as f32,
        ),
        shape: Vec4::new(
            frame.threshold,
            COVER_RAMP * frame.ramp,
            0.5 * LAYER_EXTENT,
            shown,
        ),
        sun: to_sun.extend(0.0),
        edge: Vec4::new(EDGE_RAMP, SLOPE_RANGE, EDGE_FADE, THIN_GROUP),
        depth: Vec4::new(TAU_EDGE, TAU_HEART, HORIZON_BAND, frame.tau),
        cover: Vec4::new(
            cover_offset.x as f32,
            cover_offset.y as f32,
            cover_inv_scale as f32,
            GATE_FLOOR,
        ),
        relief: Vec4::new(
            frame.thickness as f32,
            fray.x as f32,
            fray.y as f32,
            BASE_BULGE,
        ),
    }
}

/// Takes a finished shadow mask and its cover's threshold, and starts a
/// new one on the async compute pool when the cover has moved since.
fn refresh_mask(field: &mut CloudField, images: &mut Assets<Image>, cover: f32) {
    if let Some(done) = field.remask.as_mut().and_then(|t| block_on(poll_once(t))) {
        field.remask = None;
        if images.insert(&field.mask, mask_image(done.mask)).is_err() {
            tracing::warn!("the cloud shadow's image was dropped; the shadow keeps its old cover");
        }
        field.cover = Some(done.cover);
        field.threshold = done.threshold;
        field.mean_light = done.mean_light;
    }
    if field.remask.is_none() && field.cover.is_none_or(|c| c.to_bits() != cover.to_bits()) {
        let (texels, sorted) = (field.texels.clone(), field.sorted.clone());
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
        field.remask = Some(pool.spawn(async move {
            let threshold = cover_threshold(&sorted, cover);
            let mask = shadow_mask(&texels, threshold);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "a mean of a million bytes, kept as a share in 0..=1"
            )]
            let mean_light = (mask.iter().map(|&m| f64::from(m)).sum::<f64>()
                / (255.0 * mask.len() as f64)) as f32;
            Remask {
                cover,
                threshold,
                mask,
                mean_light,
            }
        }));
    }
}

/// How much of the sun a world-space `eye` sees past a deck: one less the
/// deck's opacity where the ray from the eye towards the sun meets it,
/// times the horizon's fade the deck is drawn with. The shader's parallax
/// step is taken too: the ray meets a bulging base before the deck's
/// height, so the image is read that much nearer the eye. Read at the image
/// coordinates the shader gives that point (world `xz` less the deck's
/// drift, times its scale for the tile and that over [`COVER_TILES`] for
/// the coverage), without the finer read that frays the edge. One for an
/// eye above the deck and for a sun under the horizon, whose rays never
/// cross it.
#[must_use]
pub fn sun_clear(texels: &CloudTexels, deck: &DeckFrame, to_sun: Vec3, eye: DVec3) -> f32 {
    if to_sun.y <= 0.0 || eye.y >= deck.altitude {
        return 1.0;
    }
    let rise = f64::from(to_sun.y);
    let run = DVec2::new(f64::from(to_sun.x), f64::from(to_sun.z)) / rise;
    let crossing = DVec2::new(eye.x, eye.z) + run * (deck.altitude - eye.y);
    #[allow(clippy::cast_precision_loss, reason = "a small count of tiles")]
    let tiles = COVER_TILES as f64;
    let cover_uv = (crossing - deck.drift) * (deck.inv_scale / tiles);
    let [lift_density, group, _] =
        texels.sample((crossing - deck.drift) * deck.inv_scale, cover_uv);
    let lift = f64::from(relief_height(lift_density, group, deck.threshold))
        * deck.thickness
        * f64::from(BASE_BULGE)
        * PARALLAX;
    let shift = -run
        * (PARALLAX_RUN / run.length().max(PARALLAX_RUN))
        * lift
        * f64::from(smoothstep(0.1, 0.25, to_sun.y));
    let [density, group, slope] =
        texels.sample((crossing + shift - deck.drift) * deck.inv_scale, cover_uv);
    let horizon = smoothstep(0.0, HORIZON_BAND, to_sun.y);
    1.0 - deck_opacity(density, group, slope, deck.threshold, [deck.tau, deck.ramp]) * horizon
}

/// The height field's profile the shader shades a deck as, `0` to `1`:
/// zero at the threshold, rising steeply at the edge and flattening over
/// the heart, as a heap of cumulus does. `clouds.wgsl`'s `relief_height`.
#[must_use]
pub fn relief_height(density: f32, group: f32, threshold: f32) -> f32 {
    let gate = GATE_FLOOR + (1.0 - GATE_FLOOR) * group;
    let m = ((density * gate - threshold) / (1.0 - threshold).max(0.05)).clamp(0.0, 1.0);
    m * (2.0 - m)
}

/// Gives the sun back the `GlobalTransform` its `Transform` describes,
/// before propagation, so a frame whose sun did not move still builds its
/// cascades from a rotation and not from the last frame's projection.
fn restore_sun_transform(mut suns: Query<(&Transform, &mut GlobalTransform), With<Sun>>) {
    for (transform, mut global) in &mut suns {
        let clean = GlobalTransform::from(*transform);
        if *global != clean {
            *global = clean;
        }
    }
}

/// Puts the shadow mask on the sun, or takes it off, and replaces the
/// sun's `GlobalTransform` with the projection of the layer (see the
/// module's documentation). Runs after every system of Bevy's that reads
/// the light's rotation from it.
fn project_shadow(
    mut commands: Commands,
    projection: Res<CloudProjection>,
    field: Option<Res<CloudField>>,
    mut suns: Query<
        (
            Entity,
            &Transform,
            &mut GlobalTransform,
            Option<&DirectionalLightTexture>,
        ),
        With<Sun>,
    >,
) {
    for (entity, transform, mut global, texture) in &mut suns {
        let to_sun = transform.rotation * Vec3::Z;
        // Under the lowest sun height the sun keeps its rotation (put back
        // by `restore_sun_transform` before propagation) and drops the mask.
        let Some(field) = field
            .as_ref()
            .filter(|_| projection.active && casts_shadow(to_sun))
        else {
            if texture.is_some() {
                commands.entity(entity).remove::<DirectionalLightTexture>();
            }
            continue;
        };
        if texture.is_none_or(|t| t.image != field.mask || !t.tiled) {
            commands.entity(entity).insert(DirectionalLightTexture {
                image: field.mask.clone(),
                tiled: true,
            });
        }
        *global = GlobalTransform::from(shadow_projection(
            to_sun,
            projection.height,
            projection.offset,
            projection.inv_scale,
        ));
    }
}

/// Whether a sun in this direction carries the cloud shadow: whether it
/// stands at least [`MIN_SUN_HEIGHT`] over the horizon.
#[must_use]
pub fn casts_shadow(to_sun: Vec3) -> bool {
    to_sun.y >= MIN_SUN_HEIGHT
}

/// The cloud shadow for the valley fog's pass, as its uniform takes it:
/// `(s, s ax, s az, on)` and `(cu, cv, mean, 0)`, so the mask's coordinate
/// at a render-space point `p` is
/// `(s p.x - s ax p.y + cu, s p.z - s az p.y + cv)`, the same as
/// [`shadow_projection`] gives Bevy's lookup, with `s` one over the tile's
/// size, `(ax, az)` the sun's horizontal run per metre it climbs and
/// `(cu, cv)` the layer's height and the image's offset folded in and
/// wrapped to a tile. `on` is one while the sun carries the mask (zero
/// hands the fog the full sun), `mean` the mask's mean for the air floor
/// (one without clouds, or before they are baked).
#[must_use]
pub(crate) fn fog_shadow(projection: Option<&CloudProjection>, to_sun: Vec3) -> [Vec4; 2] {
    let Some(projection) = projection.filter(|p| p.active && casts_shadow(to_sun)) else {
        return [Vec4::ZERO, Vec4::new(0.0, 0.0, 1.0, 0.0)];
    };
    let s = projection.inv_scale;
    let (ax, az) = (
        f64::from(to_sun.x) / f64::from(to_sun.y),
        f64::from(to_sun.z) / f64::from(to_sun.y),
    );
    let cu = (s * ax * projection.height + projection.offset.x).rem_euclid(1.0);
    let cv = (s * az * projection.height + projection.offset.y).rem_euclid(1.0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "per-metre rates and offsets wrapped to a tile, as the shader takes them"
    )]
    [
        Vec4::new(s as f32, (s * ax) as f32, (s * az) as f32, 1.0),
        Vec4::new(cu as f32, cv as f32, projection.mean_light, 0.0),
    ]
}

/// How far out of the unit box the sun's `local.z` is pushed, metres of
/// the sun's direction: past any render-space point the camera can see.
///
/// Bevy 0.19.1 carries a light's texture in the clustered decals' table,
/// and its decal pass blends a decal's base colour over any standard
/// material whose `local` falls inside the decal's box (`|local| <= 0.5`
/// on every axis), which this light's does for some fragments though it is
/// no decal. With the third row along the sun that box is a metre-thick
/// slab square to the sun through the render origin, inside a quarter of
/// the cloud tile, sliding with the clouds; there the mask, a one-channel
/// image read as `(r, 0, 0, 1)`, painted rails, posts and stones a flat red
/// (ticket 0005, step 7's red patch). The light's own lookup reads `x` and
/// `y` alone, so a constant on `z` moves nothing it draws and keeps every
/// fragment out of the box.
pub const OUT_OF_DECAL_BOX: f64 = 1.0e5;

/// The sun's world-from-light transform that makes Bevy's light texture a
/// horizontal layer's shadow.
///
/// Bevy reads the texture at `uv = local.xy * (-0.5, 0.5) + 0.5`, wrapped,
/// with `local` the inverse of this transform applied to the shaded point,
/// and lights along its third column. The inverse built here sends a point
/// `g` to the `local` whose `uv` is the layer's image coordinate where the
/// ray from `g` towards the sun crosses the layer at `height`; its rows are
/// square to the sun, so the third column, which is the light's direction,
/// is the sun's. Meant for a sun at [`MIN_SUN_HEIGHT`] or over it, where
/// the third column is exactly that sun; under it the clamp keeps the map
/// finite and no more, and `project_shadow` does not use it there.
///
/// Its `local.z` is the distance along the sun plus [`OUT_OF_DECAL_BOX`],
/// so no point is ever inside the box Bevy's decal pass tests.
#[must_use]
pub fn shadow_projection(to_sun: Vec3, height: f64, offset: DVec2, inv_scale: f64) -> Affine3A {
    let sun = DVec3::new(
        f64::from(to_sun.x),
        f64::from(to_sun.y.max(MIN_SUN_HEIGHT)),
        f64::from(to_sun.z),
    );
    let (ax, az) = (sun.x / sun.y, sun.z / sun.y);
    let s = inv_scale;
    // u = s (g.x + ax (height - g.y)) + offset.x, local.x = 1 - 2u;
    // v = s (g.z + az (height - g.y)) + offset.y, local.y = 2v - 1.
    let rows = DMat3::from_cols(
        DVec3::new(-2.0 * s, 2.0 * s * ax, 0.0),
        DVec3::new(0.0, -2.0 * s * az, 2.0 * s),
        sun.normalize(),
    )
    .transpose();
    // A shift of two in `local` is one tile, so the constants are wrapped
    // to keep them small in f32.
    let tx = (1.0 - 2.0 * (s * ax * height + offset.x)).rem_euclid(2.0);
    let ty = (2.0 * (s * az * height + offset.y) - 1.0).rem_euclid(2.0);
    let local_from_world = DMat3::inverse(&rows);
    let translation = -(local_from_world * DVec3::new(tx, ty, OUT_OF_DECAL_BOX));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Bevy's transforms are f32; the f64 above keeps the wrap exact"
    )]
    let narrow = |v: DVec3| Vec3A::new(v.x as f32, v.y as f32, v.z as f32);
    Affine3A {
        matrix3: Mat3A::from_cols(
            narrow(local_from_world.x_axis),
            narrow(local_from_world.y_axis),
            narrow(local_from_world.z_axis),
        ),
        translation: narrow(translation),
    }
}

/// The threshold over which a texel's cloud is at least half opaque: the
/// cover's threshold at which the texel turns from sky to cloud. Zero for a
/// texel too thin to be half opaque at any threshold.
#[must_use]
pub fn half_opaque(density: f32, group: f32, slope: f32) -> f32 {
    if opacity(density, group, slope, 0.0) <= 0.5 {
        return 0.0;
    }
    // Opacity falls as the threshold rises, so the crossing is bracketed.
    let (mut lo, mut hi) = (0.0_f32, density);
    for _ in 0..20 {
        let mid = f32::midpoint(lo, hi);
        if opacity(density, group, slope, mid) > 0.5 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The [`half_opaque`] threshold of every texel the shadow mask is made
/// of, sorted: the ladder [`cover_threshold`] reads a cover off, so the
/// cover is the share of the sky the drawn cloud covers at least half,
/// thin fringes not counted.
#[must_use]
pub fn cover_ladder(texels: &CloudTexels) -> Vec<f32> {
    let mut ladder: Vec<f32> = (0..MASK_SIZE * MASK_SIZE)
        .map(|i| {
            let [d, g, s] = texels.at((i / MASK_SIZE) * MASK_STEP, (i % MASK_SIZE) * MASK_STEP);
            half_opaque(d, g, s)
        })
        .collect();
    ladder.sort_by(f32::total_cmp);
    ladder
}

/// The threshold under which `cover` of the sky is clear, from
/// [`cover_ladder`]'s `sorted` thresholds: their `1 - cover` quantile. At
/// zero nothing is cloud.
#[must_use]
pub fn cover_threshold(sorted: &[f32], cover: f32) -> f32 {
    if cover <= 0.0 || sorted.is_empty() {
        return 1.0 + COVER_RAMP;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a million thresholds, from a fraction in 0..=1"
    )]
    let index = ((1.0 - cover.min(1.0)) * (sorted.len() - 1) as f32).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// How much of the sun gets through at each texel of the [`MASK_SIZE`]
/// mask, which spans [`COVER_TILES`] tiles, `255` for all of it: the
/// layer's [`opacity`] at the shadow's strength, blurred, so a thin cloud
/// at a group's fringe casts a lighter shadow than a full one.
#[must_use]
pub fn shadow_mask(texels: &CloudTexels, threshold: f32) -> Vec<u8> {
    let n = MASK_SIZE;
    let mut shade: Vec<f32> = (0..n * n)
        .map(|i| {
            let [d, g, s] = texels.at((i / n) * MASK_STEP, (i % n) * MASK_STEP);
            SHADOW_STRENGTH * opacity(d, g, s, threshold)
        })
        .collect();
    for _ in 0..2 {
        shade = box_blur(&box_blur(&shade, n, true), n, false);
    }
    shade
        .iter()
        .map(|&s| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0..=255 first"
            )]
            let byte = ((1.0 - s) * 255.0).round().clamp(0.0, 255.0) as u8;
            byte
        })
        .collect()
}

/// One pass of a wrapped box blur along rows or columns.
fn box_blur(values: &[f32], n: usize, along_rows: bool) -> Vec<f32> {
    let radius = MASK_BLUR;
    #[allow(clippy::cast_precision_loss, reason = "a kernel of a few texels")]
    let weight = 1.0 / (2 * radius + 1) as f32;
    let mut out = vec![0.0; values.len()];
    for row in 0..n {
        for col in 0..n {
            let mut sum = 0.0;
            for k in 0..=2 * radius {
                let (r, c) = if along_rows {
                    (row, (col + n + k - radius) % n)
                } else {
                    ((row + n + k - radius) % n, col)
                };
                sum += values[r * n + c];
            }
            out[row * n + col] = sum * weight;
        }
    }
    out
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The baked cloud image: three [`DENSITY_SIZE`] squared channels, row by
/// row, which the layer reads as one `Rgba8` texture and the shadow mask
/// and the sun's veil read here. The density and its slope span one tile;
/// the coverage spans [`COVER_TILES`] tiles at a coarser texel, so the
/// groups it gathers clouds into are larger than a tile.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CloudTexels {
    /// The clouds' detail density, `0` to `1`, before the coverage gates
    /// it.
    pub density: Vec<f32>,
    /// The coverage field, `0` in a gap between groups of clouds and `1` in
    /// a group's middle: it gates the density (down to `GATE_FLOOR` of
    /// it), widens the edge's ramp and thins the cloud towards a group's
    /// edge.
    pub group: Vec<f32>,
    /// The detail density's slope, change per tile, over `SLOPE_RANGE`
    /// and clamped to `0..=1`. The density over the threshold divided by
    /// its slope is how far inside the cloud a point is, which fades every
    /// edge over the same distance however steep the noise is there.
    pub slope: Vec<f32>,
}

impl CloudTexels {
    /// The density, the coverage and the slope at texel `(row, col)` of the
    /// coverage's repeat, each in `0..DENSITY_SIZE * COVER_TILES`.
    fn at(&self, row: usize, col: usize) -> [f32; 3] {
        let n = DENSITY_SIZE;
        let tile = (row % n) * n + col % n;
        let cover = (row / COVER_TILES) * n + col / COVER_TILES;
        [self.density[tile], self.group[cover], self.slope[tile]]
    }

    /// The density, the coverage and the slope at the tile's texture
    /// coordinate `uv` and the coverage's `cover_uv`, bilinear and wrapped,
    /// as the GPU's linear sampler reads mip zero.
    #[must_use]
    pub fn sample(&self, uv: DVec2, cover_uv: DVec2) -> [f32; 3] {
        let n = DENSITY_SIZE;
        if [&self.density, &self.group, &self.slope]
            .iter()
            .any(|c| c.len() != n * n)
        {
            return [0.0; 3];
        }
        let [density, slope] = bilinear([&self.density, &self.slope], uv);
        let [group] = bilinear([&self.group], cover_uv);
        [density, group, slope]
    }
}

/// Channels of a [`DENSITY_SIZE`] squared image read bilinearly at a
/// wrapped texture coordinate, texel centres at half-texel offsets as the
/// GPU has them.
fn bilinear<const K: usize>(channels: [&[f32]; K], uv: DVec2) -> [f32; K] {
    let width = DENSITY_SIZE;
    #[allow(clippy::cast_precision_loss, reason = "a 512 texel side is exact")]
    let size = width as f64;
    let col = uv.x.rem_euclid(1.0) * size - 0.5;
    let row = uv.y.rem_euclid(1.0) * size - 0.5;
    let (col_floor, row_floor) = (col.floor(), row.floor());
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "wrapped into the image before the cast"
    )]
    let wrap = |v: f64| v.rem_euclid(size) as usize;
    let (c0, r0) = (wrap(col_floor), wrap(row_floor));
    let (c1, r1) = ((c0 + 1) % width, (r0 + 1) % width);
    #[allow(clippy::cast_possible_truncation, reason = "bilinear weights in 0..1")]
    let (across, down) = ((col - col_floor) as f32, (row - row_floor) as f32);
    std::array::from_fn(|k| {
        let channel = channels[k];
        let top = channel[r0 * width + c0]
            + (channel[r0 * width + c1] - channel[r0 * width + c0]) * across;
        let bottom = channel[r1 * width + c0]
            + (channel[r1 * width + c1] - channel[r1 * width + c0]) * across;
        top + (bottom - top) * down
    })
}

/// How opaque the layer is where the image holds the detail `density`, the
/// coverage `group` and the `slope`, at the cover's `threshold`: what
/// `clouds.wgsl` draws, less its finer read that frays the edge. The shadow
/// mask, the cover's ladder and the sun's veil are made from it, so the
/// shadow, the cover and the dimmed sun agree with the drawn cloud.
#[must_use]
pub fn opacity(density: f32, group: f32, slope: f32, threshold: f32) -> f32 {
    deck_opacity(density, group, slope, threshold, [1.0, 1.0])
}

/// [`opacity`] with the deck's optical depth and cover ramp scaled, as the
/// shader's `depth.w` and `shape.y` scale them: the upper deck is thinner
/// and softer.
fn deck_opacity(
    density: f32,
    group: f32,
    slope: f32,
    threshold: f32,
    [tau, ramp_scale]: [f32; 2],
) -> f32 {
    let gate = GATE_FLOOR + (1.0 - GATE_FLOOR) * group;
    let excess = density * gate - threshold;
    if excess <= 0.0 {
        return 0.0;
    }
    let ramp = COVER_RAMP * ramp_scale * (EDGE_RAMP + (1.0 - EDGE_RAMP) * group);
    let grown = smoothstep(0.0, ramp, excess);
    let inside = excess / (slope * gate * SLOPE_RANGE).max(1e-3);
    let edge = smoothstep(0.0, EDGE_FADE, inside);
    let heart = smoothstep(0.0, 1.0, excess / (1.0 - threshold).max(0.05));
    let depth = grown
        * edge
        * (TAU_EDGE + (TAU_HEART - TAU_EDGE) * heart)
        * (THIN_GROUP + (1.0 - THIN_GROUP) * group)
        * tau;
    1.0 - exp_neg(depth)
}

/// `e^-t` for `t` in `0..1024` from multiplications alone, as
/// `(1 - t / 1024)^1024` by ten squarings, so a mask derived from the seed
/// is the same bits on every machine. Within `3e-4` of the true value
/// everywhere, far under a byte of the mask.
fn exp_neg(t: f32) -> f32 {
    let mut v = 1.0 - t.clamp(0.0, 1023.0) / 1024.0;
    for _ in 0..10 {
        v *= v;
    }
    v
}

/// The tileable cloud image for a world seed.
///
/// The detail, one tile: heaps of three sizes (domes over jittered points
/// with radii of their own, at periods 5, 11 and 24 to the tile, about two
/// kilometres down to a hundred metres across at the default tile), merged
/// by a smooth max so touching heaps round into one, plus a fine fractal
/// and a ridged octave that crease the edge, all read through a domain warp
/// of up to `WARP` of the tile that bends whole clouds and a short one a
/// fifth as far that frays them. The coverage, [`COVER_TILES`] tiles: two
/// octaves of value noise, at periods 3 and 6 to its repeat (cells of 8 and
/// 4 km at the default tile), warped to round their square cells and eased
/// into a gate, so clouds gather in groups with clear sky between them.
/// Every lattice wraps at its image's edge and the warps are tileable, so
/// both tile.
#[must_use]
pub fn density_field(seed: Seed) -> CloudTexels {
    let n = DENSITY_SIZE;
    let salt = |label: &str| seed.derive(label);
    let (cover_salt, warp_x, warp_y) = (
        salt("clouds/coverage"),
        salt("clouds/warp-x"),
        salt("clouds/warp-y"),
    );
    let (heap_salt, fine_salt, ridge_salt) = (
        salt("clouds/heaps"),
        salt("clouds/fine"),
        salt("clouds/ridge"),
    );
    #[allow(
        clippy::cast_precision_loss,
        reason = "texel indices under 512 are exact"
    )]
    let size = n as f64;
    let texel = |index: usize| {
        #[allow(
            clippy::cast_precision_loss,
            reason = "texel indices under 512 are exact"
        )]
        let (col, row) = ((index % n) as f64, (index / n) as f64);
        ((col + 0.5) / size, (row + 0.5) / size)
    };
    let mut detail = Vec::with_capacity(n * n);
    let mut group = Vec::with_capacity(n * n);
    for index in 0..n * n {
        let (u, v) = texel(index);
        // A broad warp that bends whole clouds and a short one, a fifth as
        // far, that frays their edges.
        let (wu, wv) = (
            u + WARP * (2.0 * fractal(u, v, &[2, 4], warp_x) - 1.0),
            v + WARP * (2.0 * fractal(u, v, &[2, 4], warp_y) - 1.0),
        );
        let (fu, fv) = (
            wu + 0.2 * WARP * (2.0 * fractal(wu, wv, &[20, 40], warp_x ^ 2) - 1.0),
            wv + 0.2 * WARP * (2.0 * fractal(wu, wv, &[20, 40], warp_y ^ 2) - 1.0),
        );
        let mut heaps: f64 = 0.0;
        for (i, (period, height)) in [(5_i64, 1.0), (11, 0.8), (24, 0.55)]
            .into_iter()
            .enumerate()
        {
            #[allow(clippy::cast_precision_loss, reason = "periods are small integers")]
            let p = period as f64;
            heaps = heaps.max(height * cell_heap(fu * p, fv * p, period, heap_salt ^ i as u64));
        }
        let fine = fractal(fu, fv, &[16, 32, 64], fine_salt);
        let ridge = 1.0 - (2.0 * octave(fu, fv, 22, ridge_salt) - 1.0).abs();
        detail.push(0.8 * heaps + 0.14 * fine + 0.06 * ridge * ridge);
        // The coverage, in its own frame of COVER_TILES tiles, through a
        // warp of its own.
        let (cu, cv) = (
            u + WARP * (2.0 * octave(u, v, 2, cover_salt ^ 2) - 1.0),
            v + WARP * (2.0 * octave(u, v, 2, cover_salt ^ 3) - 1.0),
        );
        let coverage =
            0.65 * octave(cu, cv, 3, cover_salt) + 0.35 * octave(cu, cv, 6, cover_salt ^ 1);
        group.push(smoothstep64(0.36, 0.64, coverage));
    }
    let (lo, hi) = detail
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &d| (lo.min(d), hi.max(d)));
    let span = (hi - lo).max(1e-9);
    let density: Vec<f64> = detail.into_iter().map(|d| (d - lo) / span).collect();
    // Central differences across the wrap, in density per tile.
    let mut slope = Vec::with_capacity(n * n);
    for row in 0..n {
        for col in 0..n {
            let at = |r: usize, c: usize| density[(r % n) * n + c % n];
            let du = (at(row, col + 1) - at(row, col + n - 1)) * 0.5 * size;
            let dv = (at(row + 1, col) - at(row + n - 1, col)) * 0.5 * size;
            slope.push(((du * du + dv * dv).sqrt() / f64::from(SLOPE_RANGE)).min(1.0));
        }
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "values in 0..=1 kept as f32"
    )]
    let narrow = |v: Vec<f64>| v.into_iter().map(|d| d as f32).collect();
    CloudTexels {
        density: narrow(density),
        group: narrow(group),
        slope: narrow(slope),
    }
}

/// Heaps of cloud, one per cell at a jittered point with a radius of its
/// own between 0.3 and 0.9 cell, `period` cells to the tile: the highest
/// dome over the point, `1` at a heap's centre and `0` past its radius.
/// A max of domes keeps every heap distinct where a sum would smear them;
/// rounded, so touching heaps merge without a crease; and the radii vary
/// a heap's size within the octave.
fn cell_heap(x: f64, y: f64, period: i64, salt: u64) -> f64 {
    let (fx, fy) = (x.floor(), y.floor());
    #[allow(
        clippy::cast_possible_truncation,
        reason = "cell indices are under a few hundred"
    )]
    let (ix, iy) = (fx as i64, fy as i64);
    let mut highest: f64 = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let hash = lattice(cx, cy, period, salt);
            // Kept off the cell's border and under a cell in radius, so no
            // heap from beyond the three by three cells reaches the point.
            #[allow(
                clippy::cast_precision_loss,
                reason = "cell indices are under a few hundred"
            )]
            let (px, py) = (
                cx as f64 + 0.15 + 0.7 * unit(hash),
                cy as f64 + 0.15 + 0.7 * unit(mix64(hash)),
            );
            let radius = 0.3 + 0.6 * unit(mix64(hash ^ 0x9e37_79b9_7f4a_7c15));
            let d2 = ((px - x) * (px - x) + (py - y) * (py - y)) / (radius * radius);
            highest = smooth_max(highest, 1.0 - d2, 0.3);
        }
    }
    highest.max(0.0)
}

/// The larger of two values with the corner rounded over a width of `k`,
/// so two heaps that touch merge into one rounded heap rather than meet
/// along a crease.
fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.max(b) + h * h * k * 0.25
}

/// One octave of tileable value noise at `period` cells to the tile, at a
/// texture coordinate.
fn octave(u: f64, v: f64, period: i64, salt: u64) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "periods are small integers")]
    let p = period as f64;
    value_noise(u * p, v * p, period, salt)
}

/// Octaves of value noise at the given periods, each half the weight of
/// the one before, normalised to `0..1`.
fn fractal(u: f64, v: f64, periods: &[i64], salt: u64) -> f64 {
    let (mut sum, mut total, mut weight) = (0.0, 0.0, 1.0);
    for (i, &period) in periods.iter().enumerate() {
        sum += weight * octave(u, v, period, salt ^ mix64(i as u64 + 1));
        total += weight;
        weight *= 0.5;
    }
    sum / total
}

fn smoothstep64(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A lattice value in `0..1` at integer `(ix, iy)`, wrapped every `period`.
fn lattice(ix: i64, iy: i64, period: i64, salt: u64) -> u64 {
    #[allow(
        clippy::cast_sign_loss,
        reason = "rem_euclid of a positive period is never negative"
    )]
    let (x, y) = (ix.rem_euclid(period) as u64, iy.rem_euclid(period) as u64);
    mix64(salt ^ mix64((x << 32) | y))
}

/// The top 53 bits of a hash as a fraction in `0..1`.
fn unit(hash: u64) -> f64 {
    #[allow(clippy::cast_precision_loss, reason = "53 bits are exact in f64")]
    let top = (hash >> 11) as f64;
    top * (1.0 / 9_007_199_254_740_992.0)
}

/// One octave of value noise with a quintic fade, `period` cells to the
/// tile.
fn value_noise(x: f64, y: f64, period: i64, salt: u64) -> f64 {
    let (fx, fy) = (x.floor(), y.floor());
    #[allow(
        clippy::cast_possible_truncation,
        reason = "cell indices are under a few hundred"
    )]
    let (ix, iy) = (fx as i64, fy as i64);
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (tx, ty) = (fade(x - fx), fade(y - fy));
    let at = |dx: i64, dy: i64| unit(lattice(ix + dx, iy + dy, period, salt));
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

/// The image as `Rgba8` (density, group, slope, unused) with every mip
/// down to one texel, repeated, linear and anisotropic, since the far
/// layer is seen at a grazing angle.
fn density_image(texels: &CloudTexels) -> Image {
    let n = DENSITY_SIZE;
    let mut level: Vec<[f32; 3]> = (0..n * n)
        .map(|i| [texels.density[i], texels.group[i], texels.slope[i]])
        .collect();
    let mut width = n;
    let mut data = Vec::new();
    let mut mips = 0_u32;
    loop {
        for texel in &level {
            data.extend([to_byte(texel[0]), to_byte(texel[1]), to_byte(texel[2]), 255]);
        }
        mips += 1;
        if width == 1 {
            break;
        }
        let half = width / 2;
        let mut next = vec![[0.0; 3]; half * half];
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
    #[allow(clippy::cast_possible_truncation, reason = "a 512 texel side")]
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
    image.texture_descriptor.label = Some("cloud_density");
    image.texture_descriptor.mip_level_count = mips;
    image.data_order = TextureDataOrder::LayerMajor;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("cloud_density".to_owned()),
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

/// The shadow mask as an `R8` image. Repeated and linear: Bevy reads light
/// textures through the first decal image's own sampler at mip zero, so
/// this sampler is the one the shadow is filtered with, and its filtering
/// is the shadow's softness past the blur.
fn mask_image(mask: Vec<u8>) -> Image {
    #[allow(clippy::cast_possible_truncation, reason = "a 1024 texel side")]
    let side = MASK_SIZE as u32;
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        mask,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.label = Some("cloud_shadow");
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("cloud_shadow".to_owned()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    image
}

fn to_byte(d: f32) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 first"
    )]
    let byte = (d * 255.0).round().clamp(0.0, 255.0) as u8;
    byte
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_density_is_the_same_bits_for_a_seed_and_differs_between_seeds() {
        let a = density_field(Seed(7));
        let b = density_field(Seed(7));
        let c = density_field(Seed(8));
        let bits = |t: &CloudTexels| {
            [&t.density, &t.group, &t.slope]
                .into_iter()
                .flat_map(|v| v.iter().map(|x| x.to_bits()))
                .collect::<Vec<_>>()
        };
        for channel in [&a.density, &a.group, &a.slope] {
            assert_eq!(channel.len(), DENSITY_SIZE * DENSITY_SIZE);
            assert!(channel.iter().all(|d| (0.0..=1.0).contains(d)));
        }
        assert_eq!(bits(&a), bits(&b));
        assert_ne!(bits(&a), bits(&c));
    }

    #[test]
    fn the_density_tiles() {
        // Across the wrap the image changes no more than between any two
        // neighbouring columns or rows inside it.
        let n = DENSITY_SIZE;
        let texels = density_field(Seed(3));
        for field in [&texels.density, &texels.group, &texels.slope] {
            let at = |r: usize, c: usize| field[r * n + c];
            let mut inside = 0.0_f32;
            for r in 0..n {
                for c in 0..n - 1 {
                    inside = inside.max((at(r, c) - at(r, c + 1)).abs());
                    inside = inside.max((at(c, r) - at(c + 1, r)).abs());
                }
            }
            let mut seam = 0.0_f32;
            for i in 0..n {
                seam = seam.max((at(i, n - 1) - at(i, 0)).abs());
                seam = seam.max((at(n - 1, i) - at(0, i)).abs());
            }
            assert!(seam <= inside, "seam {seam} against {inside} inside");
        }
    }

    #[test]
    fn a_cover_is_that_share_of_the_sky() {
        let texels = density_field(Seed(11));
        let field = &texels.density;
        let sorted = cover_ladder(&texels);
        // Measured on the texels between those the ladder was made from,
        // over the coverage's whole repeat.
        let opaque = |threshold: f32| {
            (0..MASK_SIZE * MASK_SIZE)
                .map(|i| {
                    let [d, g, s] = texels.at(
                        (i / MASK_SIZE) * MASK_STEP + 1,
                        (i % MASK_SIZE) * MASK_STEP + 1,
                    );
                    opacity(d, g, s, threshold)
                })
                .collect::<Vec<_>>()
        };
        for cover in [0.2_f32, 0.45, 0.8] {
            let seen = opaque(cover_threshold(&sorted, cover));
            #[allow(clippy::cast_precision_loss, reason = "a count of texels")]
            let share = seen.iter().filter(|&&o| o > 0.5).count() as f32 / seen.len() as f32;
            assert!((share - cover).abs() < 0.01, "{cover}: {share}");
        }
        assert!(field.iter().all(|&d| d < cover_threshold(&sorted, 0.0)));
        let clear = shadow_mask(&texels, cover_threshold(&sorted, 0.0));
        assert!(clear.iter().all(|&m| m == 255));
        let mask = shadow_mask(&texels, cover_threshold(&sorted, 0.45));
        assert!(mask.contains(&255));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a byte"
        )]
        let darkest = ((1.0 - SHADOW_STRENGTH) * 255.0).round() as u8;
        assert!(mask.iter().all(|&m| m >= darkest - 1));
        assert!(mask.iter().any(|&m| m < 128));
    }

    /// The clouds' shapes, at the default cover over the coverage's whole
    /// repeat: they gather where the coverage is high and leave clear gaps
    /// where it is low, and their sizes span an order of magnitude.
    #[test]
    fn clouds_gather_in_groups_and_vary_in_size() {
        let texels = density_field(Seed(5));
        let threshold = cover_threshold(&cover_ladder(&texels), 0.45);
        let n = MASK_SIZE;
        let cloud: Vec<bool> = (0..n * n)
            .map(|i| {
                let [d, g, s] = texels.at((i / n) * MASK_STEP, (i % n) * MASK_STEP);
                opacity(d, g, s, threshold) > 0.5
            })
            .collect();
        let group = |i: usize| texels.at((i / n) * MASK_STEP, (i % n) * MASK_STEP)[1];
        let share = |pick: &dyn Fn(f32) -> bool| {
            let (mut hits, mut count) = (0_u32, 0_u32);
            for (i, &c) in cloud.iter().enumerate() {
                if pick(group(i)) {
                    count += 1;
                    hits += u32::from(c);
                }
            }
            f64::from(hits) / f64::from(count.max(1))
        };
        let (inside, gaps) = (share(&|g| g > 0.8), share(&|g| g < 0.2));
        assert!(
            inside > 0.6 && gaps < 0.3,
            "{inside} in groups, {gaps} in gaps"
        );
        // Each cloud's area in texels, by a flood fill across the wrap.
        let mut seen = vec![false; n * n];
        let mut sizes = Vec::new();
        for start in 0..n * n {
            if !cloud[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            let (mut stack, mut size) = (vec![start], 0_usize);
            while let Some(i) = stack.pop() {
                size += 1;
                let (r, c) = (i / n, i % n);
                for (dr, dc) in [(1, 0), (n - 1, 0), (0, 1), (0, n - 1)] {
                    let j = ((r + dr) % n) * n + (c + dc) % n;
                    if cloud[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            sizes.push(size);
        }
        // Clouds of more than a few texels (100 m across at 6 km a tile).
        sizes.retain(|&s| s >= 16);
        sizes.sort_unstable();
        let (small, large) = (sizes[sizes.len() / 10], sizes[sizes.len() * 9 / 10]);
        // Ten times the area is three times the width; the largest groups
        // are far larger again.
        assert!(sizes.len() > 50, "{} clouds", sizes.len());
        assert!(large >= 10 * small, "{small} to {large} texels");
    }

    #[test]
    fn opacity_is_nothing_under_the_threshold_and_falls_as_it_rises() {
        assert!(opacity(0.4, 1.0, 0.3, 0.5).abs() < f32::EPSILON);
        assert!(opacity(0.9, 1.0, 0.3, 1.0 + COVER_RAMP).abs() < f32::EPSILON);
        for (d, g, s) in [(0.9, 1.0, 0.2), (0.7, 0.3, 0.5), (0.6, 0.0, 0.05)] {
            let mut last = 1.0;
            for step in 0..=50_u8 {
                let o = opacity(d, g, s, f32::from(step) / 50.0);
                assert!(o <= last + 1e-6 && (0.0..=1.0).contains(&o), "{d} {g} {s}");
                last = o;
            }
        }
        // A full group's middle is opaque; its fringe is thinner.
        assert!(opacity(1.0, 1.0, 0.1, 0.3) > 0.99);
        assert!(opacity(1.0, 0.0, 0.1, 0.3 * GATE_FLOOR) < opacity(1.0, 1.0, 0.1, 0.3));
        for t in [0.0_f32, 0.5, 2.0, 10.0] {
            assert!((exp_neg(t) - (-t).exp()).abs() < 3e-4, "{t}");
        }
    }

    #[test]
    fn the_sun_is_veiled_by_the_cloud_on_the_line_of_sight_to_it() {
        let texels = density_field(Seed(9));
        let threshold = cover_threshold(&cover_ladder(&texels), 0.45);
        let inv_scale = 1.0 / 6_000.0;
        let to_sun = Vec3::new(0.3, 0.8, -0.2).normalize();
        let layer_y = 1_800.0;
        let eye_y = 20.0;
        // Find a cloud's heart and a clear gap on the tile, then stand
        // where the line to the sun crosses each.
        let side = DENSITY_SIZE * COVER_TILES;
        let (mut heart, mut gap) = (None, None);
        for row in (0..side).step_by(7) {
            for col in (0..side).step_by(7) {
                let [density, group, slope] = texels.at(row, col);
                let seen = opacity(density, group, slope, threshold);
                #[allow(clippy::cast_precision_loss, reason = "texel indices")]
                let at = DVec2::new(col as f64 + 0.5, row as f64 + 0.5) / 512.0 / inv_scale;
                // Kept clear of the edges, since the coverage is read
                // between its own texels here.
                if seen > 0.999 && group > 0.99 && heart.is_none() {
                    heart = Some(at);
                }
                if density < threshold - 0.05 && gap.is_none() {
                    gap = Some(at);
                }
            }
        }
        let reach = (layer_y - eye_y) / f64::from(to_sun.y);
        let eye_under = |point: DVec2| {
            DVec3::new(
                point.x - f64::from(to_sun.x) * reach,
                eye_y,
                point.y - f64::from(to_sun.z) * reach,
            )
        };
        // A flat deck, so the read is where the line crosses its height.
        let deck = DeckFrame {
            threshold,
            altitude: layer_y,
            drift: DVec2::ZERO,
            inv_scale,
            thickness: 0.0,
            tau: 1.0,
            ramp: 1.0,
        };
        let clear = |eye: DVec3, sun: Vec3| sun_clear(&texels, &deck, sun, eye);
        let (heart, gap) = (heart.expect("a cloud's heart"), gap.expect("a clear gap"));
        let (behind, beside) = (eye_under(heart), eye_under(gap));
        assert!(clear(behind, to_sun) < 0.05, "{}", clear(behind, to_sun));
        assert!((clear(beside, to_sun) - 1.0).abs() < 1e-6);
        // Above the layer, or with the sun set, nothing is in the way.
        let above = DVec3::new(behind.x, 2_500.0, behind.z);
        assert!((clear(above, to_sun) - 1.0).abs() < f32::EPSILON);
        let set = Vec3::new(0.99, -0.05, 0.0).normalize();
        assert!((clear(behind, set) - 1.0).abs() < f32::EPSILON);
        // The wind carries the cloud off the sun: drifted by the step
        // from the gap to the heart, the gap stands where the heart was.
        let drifted = sun_clear(
            &texels,
            &DeckFrame {
                drift: heart - gap,
                ..deck
            },
            to_sun,
            behind,
        );
        assert!((drifted - 1.0).abs() < 1e-6, "{drifted}");
        // A bulging base meets the line before the deck's height, nearer
        // the eye: under a heart's edge the veil moves with it. Some eye
        // under the gap's fringe sees a different sun once the base has
        // height, and none sees more than the shader's reach of change.
        let tall = DeckFrame {
            thickness: 2_000.0,
            ..deck
        };
        let moved = (0..64)
            .map(|i| {
                let at = gap + (heart - gap) * (f64::from(i) / 64.0);
                let eye = eye_under(at);
                (clear(eye, to_sun) - sun_clear(&texels, &tall, to_sun, eye)).abs()
            })
            .fold(0.0_f32, f32::max);
        assert!(moved > 0.05, "{moved}");
    }

    #[test]
    fn the_decks_drift_apart_and_the_shadow_keeps_the_lower_one() {
        let settings = SkySettings::default();
        let wind = DVec2::new(0.8, 0.6);
        let [lower, upper] = deck_frames(&settings, wind, 10.0, 0.0, [0.5, 0.6]);
        // The lower deck moves at the wind's speed along it.
        assert!((lower.drift - wind * f64::from(settings.cloud_speed) * 10.0).length() < 1e-9);
        // The upper deck faster and veered, from its own fixed place.
        let [_, still] = deck_frames(&settings, wind, 0.0, 0.0, [0.5, 0.6]);
        let travel = upper.drift - still.drift;
        let ratio = travel.length() / lower.drift.length();
        assert!(
            (ratio - f64::from(settings.cloud_shear)).abs() < 1e-9,
            "{ratio}"
        );
        let turned = travel.angle_to(lower.drift).abs();
        assert!((turned - UPPER_VEER).abs() < 1e-9, "{turned}");
        assert!(upper.altitude > lower.altitude && upper.inv_scale > lower.inv_scale);
        assert!(upper.tau < lower.tau && upper.thickness < lower.thickness);

        // The shadow's projection is the lower deck's coverage frame, wound
        // on by its drift, the evolution aside: one cloud stays over one
        // patch of shadow.
        let mut app = App::new();
        app.init_resource::<SkySettings>()
            .init_resource::<Observers>()
            .init_resource::<CloudProjection>()
            .init_resource::<SunVeil>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<CloudMaterial>>()
            .insert_resource(Wind {
                direction: [0.8, 0.6],
                ..Wind::default()
            })
            .insert_resource(SimClock(strata_core::SimTime {
                tick: 6_000,
                step: 1.0 / 60.0,
            }))
            .add_systems(Update, update_layer);
        let texels = density_field(Seed(3));
        let sorted = cover_ladder(&texels);
        let blank = || CloudMaterial {
            params: CloudParams::default(),
            density: Handle::default(),
            sort_bias: 0.0,
        };
        let mut materials = app.world_mut().resource_mut::<Assets<CloudMaterial>>();
        let (material, upper) = (materials.add(blank()), materials.add(blank()));
        app.insert_resource(CloudField {
            texels: Arc::new(texels),
            sorted: Arc::new(sorted),
            remask: None,
            mask: Handle::default(),
            material: material.clone(),
            upper: upper.clone(),
            cover: Some(0.45),
            threshold: 0.5,
            mean_light: 0.8,
        });
        app.update();
        let projection = *app.world().resource::<CloudProjection>();
        let materials = app.world().resource::<Assets<CloudMaterial>>();
        let (drawn, above) = (
            materials.get(&material).expect("lower").params,
            materials.get(&upper).expect("upper").params,
        );
        let offset = projection.offset.as_vec2();
        assert!((drawn.cover.truncate().truncate() - offset).length() < 1e-6);
        assert!((f64::from(drawn.cover.z) - projection.inv_scale).abs() < 1e-9);
        assert!((projection.height - LAYER_ALTITUDE).abs() < 1e-9);
        // After 100 s the lower deck has moved 2.2 km; the upper one is
        // elsewhere, the fraying has moved, and the cover is less above.
        assert!((above.cover.truncate().truncate() - offset).length() > 1e-3);
        assert!(drawn.relief.y.abs() + drawn.relief.z.abs() > 1e-3);
        assert!(above.shape.x > drawn.shape.x);
        // From the ground the upper deck is the far one and the lower one
        // is sorted forward over it.
        assert!(above.depth.w < 1.0);
        let biases = (
            materials.get(&material).expect("lower").sort_bias,
            materials.get(&upper).expect("upper").sort_bias,
        );
        assert_eq!(biases, (DECK_SORT, 0.0));
    }

    /// Bevy's light texture lookup, `pbr_lighting.wgsl`: the inverse of
    /// the light's transform, then `uv = local.xy * (-0.5, 0.5) + 0.5`.
    fn bevy_uv(world_from_light: Affine3A, point: Vec3) -> (f64, f64) {
        let local = world_from_light.inverse().transform_point3(point);
        let uv = (
            f64::from(local.x) * -0.5 + 0.5,
            f64::from(local.y) * 0.5 + 0.5,
        );
        (uv.0 - uv.0.floor(), uv.1 - uv.1.floor())
    }

    fn wrapped_distance(a: f64, b: f64) -> f64 {
        let d = (a - b).rem_euclid(1.0);
        d.min(1.0 - d)
    }

    #[test]
    fn the_sun_s_texture_falls_where_the_layer_blocks_the_sun() {
        let height = 1_750.0;
        let offset = DVec2::new(0.37, 0.81);
        let inv_scale = 1.0 / 6_000.0;
        for to_sun in [
            Vec3::new(0.3, 0.8, -0.2).normalize(),
            Vec3::new(0.95, 0.1, -0.29).normalize(),
            Vec3::Y,
        ] {
            let transform = shadow_projection(to_sun, height, offset, inv_scale);
            // Bevy lights along the third column; it must be the sun.
            let back = Vec3::from(transform.matrix3.z_axis).normalize();
            assert!(back.abs_diff_eq(to_sun, 1e-5), "{back} against {to_sun}");
            for ground in [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(812.0, 35.0, -1_400.0),
                Vec3::new(-1_900.0, 260.0, 1_200.0),
            ] {
                let up = (height - f64::from(ground.y)) / f64::from(to_sun.y);
                let cloud = DVec3::from(ground) + DVec3::from(to_sun) * up;
                let want = (
                    (cloud.x * inv_scale + offset.x).rem_euclid(1.0),
                    (cloud.z * inv_scale + offset.y).rem_euclid(1.0),
                );
                let got = bevy_uv(transform, ground);
                // Never inside the box Bevy's decal pass would paint.
                let local = transform.inverse().transform_point3(ground);
                assert!(local.z.abs() > 1_000.0, "{to_sun} at {ground}: {local}");
                // A texel is 1/512; f32 keeps it well inside one.
                assert!(
                    wrapped_distance(got.0, want.0) < 1e-4
                        && wrapped_distance(got.1, want.1) < 1e-4,
                    "{to_sun} at {ground}: {got:?} against {want:?}"
                );
            }
        }
    }

    /// The app `project_shadow` runs in, with a cloud field over a sun
    /// pointing at `to_sun`.
    fn shadowed_sun(to_sun: Vec3) -> (App, Entity, Transform) {
        let mut app = App::new();
        app.add_systems(Update, project_shadow);
        app.insert_resource(CloudProjection {
            active: true,
            offset: DVec2::new(0.25, 0.5),
            inv_scale: 1.0 / 6_000.0,
            height: 1_750.0,
            mean_light: 0.8,
        });
        app.insert_resource(CloudField {
            texels: Arc::default(),
            sorted: Arc::default(),
            remask: None,
            mask: Handle::default(),
            material: Handle::default(),
            upper: Handle::default(),
            cover: Some(0.45),
            threshold: 0.5,
            mean_light: 0.8,
        });
        let transform = Transform::IDENTITY.looking_to(-to_sun, Vec3::Y);
        let sun = app
            .world_mut()
            .spawn((Sun, transform, GlobalTransform::from(transform)))
            .id();
        app.update();
        (app, sun, transform)
    }

    #[test]
    fn a_set_sun_keeps_its_direction_and_drops_the_shadow() {
        // From sunset to the sun's light going out, and at midnight: the
        // projection would have lifted the light's direction over the
        // horizon, so Bevy would have lit the scene by a sun that had set.
        for to_sun in [
            Vec3::new(0.99, 0.0, -0.1).normalize(),
            Vec3::new(0.99, 0.009, -0.1).normalize(),
            Vec3::new(0.99, -0.05, -0.1).normalize(),
            Vec3::new(0.99, -0.12, -0.1).normalize(),
            Vec3::new(0.1, -0.99, 0.0).normalize(),
        ] {
            let (app, sun, transform) = shadowed_sun(to_sun);
            let world = app.world();
            assert_eq!(
                world.get::<GlobalTransform>(sun),
                Some(&GlobalTransform::from(transform)),
                "{to_sun}"
            );
            let back = world.get::<GlobalTransform>(sun).expect("sun").back();
            assert!(back.abs_diff_eq(to_sun, 1e-5), "{back} against {to_sun}");
            assert!(
                world.get::<DirectionalLightTexture>(sun).is_none(),
                "{to_sun}"
            );
            let [rates, _] = fog_shadow(Some(world.resource::<CloudProjection>()), to_sun);
            assert!(
                rates.w.abs() < f32::EPSILON,
                "the fog takes no shadow at {to_sun}"
            );
        }
    }

    #[test]
    fn a_risen_sun_carries_the_shadow_along_its_own_direction() {
        for to_sun in [
            Vec3::new(0.99, 0.011, -0.1).normalize(),
            Vec3::new(0.3, 0.8, -0.2).normalize(),
        ] {
            let (app, sun, _) = shadowed_sun(to_sun);
            let world = app.world();
            assert!(
                world.get::<DirectionalLightTexture>(sun).is_some(),
                "{to_sun}"
            );
            let back = world.get::<GlobalTransform>(sun).expect("sun").back();
            assert!(back.abs_diff_eq(to_sun, 1e-5), "{back} against {to_sun}");
        }
    }

    #[test]
    fn the_fog_reads_the_mask_where_bevy_does() {
        let projection = CloudProjection {
            active: true,
            offset: DVec2::new(0.37, 0.81),
            inv_scale: 1.0 / 6_000.0,
            height: 1_750.0,
            mean_light: 0.7,
        };
        for to_sun in [
            Vec3::new(0.3, 0.8, -0.2).normalize(),
            Vec3::new(0.95, 0.1, -0.29).normalize(),
            Vec3::new(-0.9, 0.02, 0.4).normalize(),
        ] {
            let transform = shadow_projection(
                to_sun,
                projection.height,
                projection.offset,
                projection.inv_scale,
            );
            let [rates, shift] = fog_shadow(Some(&projection), to_sun);
            assert!((rates.w - 1.0).abs() < f32::EPSILON);
            assert!((shift.z - 0.7).abs() < f32::EPSILON);
            // Fog lies from the sea to a few hundred metres up, and as far
            // out as the clipmap reaches.
            for point in [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(812.0, 35.0, -1_400.0),
                Vec3::new(-24_000.0, 260.0, 31_000.0),
            ] {
                let fog = (
                    f64::from(rates.x * point.x - rates.y * point.y + shift.x),
                    f64::from(rates.x * point.z - rates.z * point.y + shift.y),
                );
                let fog = (fog.0 - fog.0.floor(), fog.1 - fog.1.floor());
                let bevy = bevy_uv(transform, point);
                // Within a tenth of a 512-texel mask's texel.
                assert!(
                    wrapped_distance(fog.0, bevy.0) < 2e-4
                        && wrapped_distance(fog.1, bevy.1) < 2e-4,
                    "{to_sun} at {point}: {fog:?} against {bevy:?}"
                );
            }
        }
        // No cover: the fog gets the full sun and the floor all of it.
        let clear = CloudProjection {
            active: false,
            ..projection
        };
        let [rates, shift] = fog_shadow(Some(&clear), Vec3::Y);
        assert!(rates.w.abs() < f32::EPSILON && (shift.z - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_cloud_tunables_are_range_checked_and_round_trip() {
        use crate::look::LookFile;
        let file = LookFile::default();
        assert!((file.sky.cloud_cover - 0.45).abs() < 1e-6);
        assert_eq!(
            LookFile::from_ron(&file.to_ron().expect("export")).expect("import"),
            file
        );
        let bad: [fn(&mut LookFile); 14] = [
            |f| f.sky.cloud_cover = -0.01,
            |f| f.sky.cloud_cover = 1.01,
            |f| f.sky.cloud_scale = 100.0,
            |f| f.sky.cloud_scale = f32::NAN,
            |f| f.sky.cloud_speed = -1.0,
            |f| f.sky.cloud_speed = 61.0,
            |f| f.sky.cloud_shear = -0.1,
            |f| f.sky.cloud_shear = 4.1,
            |f| f.sky.cloud_upper_cover = -0.1,
            |f| f.sky.cloud_upper_cover = 2.1,
            |f| f.sky.cloud_thickness = -1.0,
            |f| f.sky.cloud_thickness = f32::INFINITY,
            |f| f.sky.cloud_evolution = -0.01,
            |f| f.sky.cloud_evolution = 0.51,
        ];
        for (i, edit) in bad.iter().enumerate() {
            let mut edited = file.clone();
            edit(&mut edited);
            assert!(edited.validate().is_err(), "edit {i}");
        }
        // Zero cover and a still sky are looks of their own.
        let mut clear = file.clone();
        clear.sky.cloud_cover = 0.0;
        clear.sky.cloud_speed = 0.0;
        clear.sky.cloud_upper_cover = 0.0;
        clear.sky.cloud_thickness = 0.0;
        clear.sky.cloud_evolution = 0.0;
        assert!(clear.validate().is_ok());
        // A look saved before the clouds existed loads with the default.
        let old = file
            .to_ron()
            .expect("export")
            .lines()
            .filter(|line| !line.contains("cloud_"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(LookFile::from_ron(&old).expect("import").sky, file.sky);
    }

    #[test]
    fn the_sun_gets_its_rotation_back_before_propagation() {
        let mut app = App::new();
        app.add_systems(Update, restore_sun_transform);
        let transform = Transform::from_rotation(bevy::math::Quat::from_rotation_x(-0.7));
        let sun = app
            .world_mut()
            .spawn((
                Sun,
                transform,
                GlobalTransform::from(shadow_projection(
                    Vec3::new(0.2, 0.9, 0.1).normalize(),
                    1_800.0,
                    DVec2::ZERO,
                    1.0 / 6_000.0,
                )),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<GlobalTransform>(sun),
            Some(&GlobalTransform::from(transform))
        );
    }
}
