//! Ground fog: a full-screen pass that draws where `strata-weather`'s fog
//! field says fog lies (ticket 0003).
//!
//! Nothing here is registered unless the game inserts [`GroundFog`] with a
//! non-zero [`GroundFog::scale`] before it adds [`crate::AtmospherePlugin`]:
//! with `--fog 0` there is no fog pass and no fog system, which is how the
//! benches run, so their numbers stay comparable with those taken before
//! it. With it registered, a camera only gets the pass while the render
//! look allows it ([`crate::look::RenderLook::ground_fog`], the F6 panel's
//! "Valley fog") and it is not multisampled.
//!
//! The medium, per column of the world: the field's layer from its base to
//! its top at its density (read per step from the clipmap in
//! [`crate::fog_clipmap`]), under a top that a height field of three noise
//! octaves moves up and down by a share of the layer's depth, with a soft
//! falloff through it.
//! The pass clips each view ray to the band between the lowest base and the
//! highest top anywhere in the clipmap and to the clipmap's square, then
//! marches a fixed number of steps packed towards the camera, with the
//! extinction of each step integrated exactly, so a step longer than the
//! visibility is not biased dark.
//!
//! Where it runs: a system of its own in the `Core3d` schedule after
//! `Core3dSystems::MainPass` and before `Core3dSystems::EarlyPostProcess`,
//! where Bevy's volumetric fog sits: after the transparent pass, before TAA
//! and tonemapping. `bevy_core_pipeline::fullscreen_material` is not enough:
//! its bind group is the colour target, a sampler and one uniform, with no
//! depth and no view. After the transparent pass because the sea is a
//! blended plane (`AlphaMode::Blend`) that writes no depth: drawn after the
//! fog it would show through it crisp. With the fog after it and no fog
//! under sea level, the depth under the water is the sea bed and the march
//! stops at the surface, so the water is fogged as if it had written depth.
//! The cost is that the atmosphere's aerial perspective, which Bevy
//! composites after the opaque pass, is already in the colour under the fog;
//! the pass puts it back over the fog from the atmosphere's own aerial-view
//! and transmittance lookup tables at the fog's mean distance, so far fog is
//! hazed like the ground around it.
//!
//! The light: the sun and the moon as the atmosphere lights them (their
//! illuminance times the transmittance table at the sample's height), with a
//! forward-scattering phase, and an ambient term that is the sky's own
//! radiance, read from the atmosphere's sky-view table over the upper
//! hemisphere and weighted by the cosine to the zenith (the light a level
//! top lets in), so at dawn the fog is lit by a dim blue sky and not by a
//! constant. The sun's share of the light scattered many times enters
//! through the top too, so it is the sun's light on a level surface: a
//! tenth of the noon light at dawn, which is what keeps a dawn layer from
//! glowing as at noon. The fog shadows itself: near its top a second read of the
//! medium a step along the sun finds whether a billow stands between the
//! point and the sun, at any sun height, and the sky's ambient is dimmed by
//! the fog over a point and by how deep in a trough of the top it lies. The
//! world shadows it too: each step reads the sun's cascaded shadow map (one
//! hardware-compared tap in the cascade the step's view depth falls in), so
//! hills and buildings cut shafts through it; beyond the last cascade the
//! fog's own shadow is all there is.
//!
//! The air floor: a thin exponential height fog everywhere, full below
//! [`RenderLook::valley_floor_height`] and thinning above it, integrated
//! in closed form along the whole ray (it is too thin and too wide to be
//! worth steps, and the steps are spent only where the basins can be),
//! split at the basin fog's mean distance so what lies behind a bank is
//! hidden by it. It is lit once by the sun and the moon and by half the
//! sky's light, with no sky fill, as the atmosphere's own haze is. A ray
//! that meets nothing ends at the sea, or where the planet's curve lifts it
//! two falloffs over the floor, so the horizon does not take 150 km of it.
//! Its light reaches the eye through the air in front of it, as the
//! atmosphere's own in-scattering does: the floor's light over a stretch of
//! the ray is dimmed by the air's mean transmittance over that stretch.
//!
//! The clouds (`clouds`): each step's sunlight is what the cloud layer's
//! shadow mask lets through where the ray from the step towards the sun
//! crosses the layer, read with the same projection Bevy's light texture
//! uses on the ground, so fog under a cloud's shadow is shaded with the
//! ground under it. The air floor's sunlight takes the mask's mean.
//!
//! The order: the pass runs in the main pass's set, after the transparent
//! pass, so it composites before Bevy's volumetric fog (the local fog
//! volumes, which Bevy adds after the main pass's set): the basin fog and
//! the air floor are the far medium and the local volumes lie in front of
//! them.
//!
//! What it does not handle: MSAA (a multisampled camera gets no pass, with
//! one warning, and while no camera can draw it no tile is baked), a camera without Bevy's atmosphere (the pass needs its tables;
//! skipped with one warning), viewports, cloud with a base above the ground
//! (the field has none yet), and removing itself when the resource is
//! removed.

use crate::clouds::{CloudField, CloudProjection, fog_shadow, update_layer};
use crate::fog_clipmap::{
    ClipmapTextures, ExtractedClipmap, FogClipmap, LevelTexels, extract_clipmap, init_textures,
    update_clipmap, upload_levels,
};
use crate::look::RenderLook;
use crate::{Moon, SkyCamera, SkySettings, Sun};
use bevy::app::{App, PostUpdate, Update};
use bevy::asset::{AssetId, AssetServer, Handle};
use bevy::camera::Camera3d;
use bevy::core_pipeline::core_3d::main_transparent_pass_3d;
use bevy::core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::light::{Atmosphere, DirectionalLight};
use bevy::math::{UVec4, Vec3, Vec4};
use bevy::pbr::resources::AtmosphereTextures;
use bevy::pbr::{
    AtmosphereSettings, GpuLights, LightMeta, ShadowSamplers, ViewLightsUniformOffset,
    ViewShadowBindings,
};
use bevy::render::Extract;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_2d_array, texture_3d, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    BlendComponent, BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, Operations,
    PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp,
    TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureView, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{FallbackImage, GpuImage};
use bevy::render::view::{
    ExtractedView, Msaa, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms,
};
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bevy::transform::components::Transform;
use std::collections::HashMap;
use std::sync::Arc;
use strata_app::{RenderOrigin, WorldSourceRes};
use strata_weather::Weather;

/// The shader, loaded from disk like the terrain's.
pub const FOG_SHADER_PATH: &str = "shaders/fog.wgsl";

/// Texels along a side of the noise texture the top's relief is read from.
const NOISE_TEXELS: u32 = 256;

/// The noise's cell, metres: [`GroundFog::noise_cell`] rounded to a power of
/// two, so the finer octaves (a quarter and an eighth of it) tile the same
/// period.
fn noise_cell_m(cell: f32) -> f64 {
    f64::from(cell.clamp(8.0, 4096.0)).log2().round().exp2()
}

/// The period, in metres, over which the render origin is wrapped before it
/// reaches the shader: the noise texture's side in cells, so the lattice
/// tiles it and the noise at a world point is the same whatever the origin.
/// For a 256 m cell it is 65.5 km, which an `f32` holds to a few millimetres.
fn noise_period_m(cell: f32) -> f64 {
    f64::from(NOISE_TEXELS) * noise_cell_m(cell)
}

/// The noise texture's texels: a fixed hash of the index, so every run and
/// every machine reads the same relief.
fn noise_texels() -> Vec<u8> {
    (0..NOISE_TEXELS * NOISE_TEXELS)
        .map(|i| {
            let mut h = u64::from(i).wrapping_add(0x9e37_79b9_7f4a_7c15);
            h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            h ^= h >> 31;
            h.to_le_bytes()[0]
        })
        .collect()
}

/// The ground fog's settings. Its presence with a non-zero
/// [`scale`](Self::scale), before the atmosphere plugin is added, turns the
/// pass on.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct GroundFog {
    /// Scale on the field's density: `1` draws the field as it is, `0`
    /// registers nothing at all.
    pub scale: f32,
    /// The weather the field is sampled under. Until the ticket's step 3
    /// there is no weather over time, so it is fixed for the run.
    pub weather: Weather,
    /// Raymarch steps per pixel, spent inside the band the fog can occupy.
    pub steps: u32,
    /// Extinction per metre at the field's full density. Three over it is
    /// the distance at which the far side has faded to five percent.
    pub extinction: f32,
    /// The share of extinction that scatters rather than absorbs.
    pub albedo: f32,
    /// Henyey-Greenstein asymmetry of the forward lobe.
    pub asymmetry: f32,
    /// Gain on the isotropic half of the scattered sun and moon light. The
    /// march scatters once, and a real fog is brighter than that by all the
    /// light it scatters more than once, which reaches the eye from every
    /// direction alike; the forward lobe is left as it is. The sun's part
    /// follows its light through the layer's top (the height of the sun),
    /// so this gain is what a noon sun gets.
    pub light: f32,
    /// Gain on the sky's light from above, over the look's sky fill
    /// ([`SkySettings::environment_intensity`], which lifts the ground's
    /// shade by the same factor, so shaded ground and shaded fog keep their
    /// balance when the fill changes in the F6 panel). The light itself is
    /// the sky's radiance over the upper hemisphere weighted by the cosine
    /// to the zenith (its irradiance on a level surface over pi), read from
    /// the atmosphere's sky-view table, so it follows the hour (dim and
    /// blue at dawn) rather than a constant. A thick, nearly white layer
    /// under an even sky glows with about that sky's radiance, which is a
    /// gain of one with no fill.
    pub ambient: f32,
    /// Scale on the fog's own optical depth over a point when it dims the
    /// sun and the sky reaching that point. Under 1 because light that has
    /// scattered forward still arrives.
    pub self_shadow: f32,
    /// How far a ray that meets nothing is marched, metres, before the
    /// clipmap's square cuts it shorter. The sea writes no depth, so its
    /// pixels count as meeting nothing.
    pub far: f32,
    /// Step placement: the step boundaries are `(i / steps) ^ step_curve`
    /// of the clipped ray. Even steps over a ray that runs kilometres
    /// through the band spend the whole count far away; at 3 the first
    /// steps are metres long wherever the ray enters it.
    pub step_curve: f32,
    /// How far the noise moves the top, as a share of the layer's depth
    /// (its top less its base), so a 12 m trough billows by a few metres
    /// and a 60 m bay by a dozen.
    pub noise_share: f32,
    /// The most the noise moves the top, metres.
    pub noise_max: f32,
    /// Size of the noise's larger cells, metres, rounded to a power of
    /// two; the other octaves are a quarter and an eighth of it.
    pub noise_cell: f32,
    /// Half the height over which the density falls from full to nothing
    /// through the local top, metres, at least; a share of the noise's
    /// reach above that.
    pub soft: f32,
    /// How dark the sky's light is at the bottom of a trough in the top,
    /// as a share of what reaches a crest: the ambient occlusion that makes
    /// the top read as a surface when the sun is high.
    pub trough: f32,
}

impl GroundFog {
    /// The weather drawn until the ticket's step 3 brings the day's curve:
    /// every basin full to its top, which is what the field's pictures are
    /// measured under, and the sea's banks at half, separate banks with
    /// clear water between them, so a coast shows both.
    pub const WEATHER: Weather = Weather {
        fog: 1.0,
        sea_fog: 0.5,
        drift: [0.0, 0.0],
    };
}

impl Default for GroundFog {
    fn default() -> Self {
        Self {
            scale: 1.0,
            weather: Self::WEATHER,
            steps: 12,
            // Fifty metres to five percent at full density: the wet
            // hollows' fog (about 0.6) leaves a quarter of a tree forty
            // metres off, a grey shape, and hides the far side.
            extinction: 0.06,
            albedo: 0.95,
            asymmetry: 0.7,
            // 4.5 before the sun's part followed its height (2026-10-02);
            // 5.5 times a noon sun's 0.82 is the same noon fog.
            light: 5.5,
            // Against the sky's mean radiance (2026-10-02): an overall gain of
            // 3 on it (0.75 of the sky fill of 4) draws the noon fog in the
            // plain's hollow as bright as the earlier ambient (1.25 on the
            // multiple-scattering table) did, and the fog in a hill's shadow
            // at dawn a dim blue, under the sky's own brightness.
            ambient: 0.75,
            self_shadow: 0.3,
            far: 150_000.0,
            step_curve: 3.0,
            noise_share: 0.3,
            noise_max: 18.0,
            noise_cell: 256.0,
            soft: 3.0,
            trough: 0.35,
        }
    }
}

/// How far the falloff reaches through the local top, as a share of how far
/// the noise moves it. `soft_of` in `shaders/fog.wgsl` uses the same share,
/// and `could_hold` there adds one to it for the noise's own reach; the
/// band [`update_uniforms`] clips rays to assumes both.
const SOFT_SHARE: f32 = 0.35;

/// Whether any camera will draw the pass: the look allows it and a sky
/// camera is not multisampled. The pass cannot draw into a multisampled
/// view, so under MSAA nothing is baked for it either.
fn fog_drawn(look: Res<RenderLook>, cameras: Query<Option<&Msaa>, With<SkyCamera>>) -> bool {
    look.ground_fog && cameras.iter().any(single_sampled)
}

/// Whether a camera's view is single-sampled. Every Bevy camera carries a
/// [`Msaa`] (it is required by `Camera`); one without is taken as not
/// multisampled.
fn single_sampled(msaa: Option<&Msaa>) -> bool {
    msaa.is_none_or(|m| *m == Msaa::Off)
}

/// The pass's per-camera uniform. Mirrored field for field by `FogPass` in
/// `shaders/fog.wgsl`.
#[derive(Component, Clone, Copy, Default, Debug, ShaderType, ExtractComponent)]
pub struct FogPassUniform {
    /// The render origin: `x` and `z` wrapped to the noise period, `y` as
    /// it is; `w` is the period.
    origin: Vec4,
    /// Unit vector towards the sun.
    sun_direction: Vec4,
    /// The sun's linear colour times its illuminance, lux.
    sun_color: Vec4,
    /// Unit vector towards the moon.
    moon_direction: Vec4,
    /// The moon's linear colour times its illuminance, lux.
    moon_color: Vec4,
    /// The fine level: its `-x -z` corner in render space, one over its
    /// extent, and one when it holds data.
    fine: Vec4,
    /// The coarse level, likewise.
    coarse: Vec4,
    /// The band the fog can occupy: lowest base, highest top with the
    /// noise and the falloff, sea level, and the noise's most.
    slab: Vec4,
    /// Extinction at full density (times the scale), albedo, asymmetry,
    /// light.
    medium: Vec4,
    /// The sky's gain (ambient times the sky fill), self-shadow, one over
    /// the noise's cell, noise share.
    shape: Vec4,
    /// Far distance, step curve, soft falloff, trough darkness.
    march: Vec4,
    /// Steps, frame, and two unused.
    counts: UVec4,
    /// The atmosphere: inner and outer radius, aerial-view distance and
    /// slices.
    atmosphere: Vec4,
    /// The multiple-scattering table's size in `xy` (bound, not read), the
    /// sky-view table's in `zw`.
    lut_sizes: Vec4,
    /// The look's shadow strength, the air floor's extinction per metre,
    /// the world height it is full up to and the metres it thins by `e`
    /// over above that.
    lighting: Vec4,
    /// The cloud shadow's projection: one over the tile's size and the
    /// sun's horizontal run per metre climbed times it, and one while the
    /// sun carries the mask ([`crate::clouds`]).
    clouds: Vec4,
    /// The mask's coordinate offset in `xy`, its mean (the air floor's
    /// share of the sun) in `z`, and one unused.
    cloud_shift: Vec4,
}

/// Registers the pass when the ground fog is on; does nothing otherwise.
pub(crate) fn register(app: &mut App) {
    let on = app
        .world()
        .get_resource::<GroundFog>()
        .is_some_and(|fog| fog.scale > 0.0);
    if !on {
        return;
    }
    app.add_plugins((
        ExtractComponentPlugin::<FogPassUniform>::default(),
        UniformComponentPlugin::<FogPassUniform>::default(),
    ))
    .add_systems(Update, (update_clipmap.run_if(fog_drawn), log_gpu_time))
    .add_systems(
        PostUpdate,
        update_uniforms
            .after(bevy::transform::TransformSystems::Propagate)
            .after(update_layer),
    );
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<ExtractedClipmap>()
        .init_resource::<CloudMask>()
        .add_systems(RenderStartup, (init_pipeline, init_textures))
        .add_systems(ExtractSchedule, (extract_clipmap, extract_cloud_mask))
        .add_systems(
            Render,
            (
                readable_depth.in_set(RenderSystems::PrepareViews),
                prepare_pipelines.in_set(RenderSystems::Prepare),
                upload_levels.in_set(RenderSystems::PrepareResources),
            ),
        )
        // In the main pass's set, after the transparent pass: Bevy's
        // volumetric fog (the local volumes) runs after the set, so it is
        // composited over this far medium and not in an order the executor
        // picks.
        .add_systems(
            Core3d,
            fog_pass
                .in_set(Core3dSystems::MainPass)
                .in_set(GroundFogPass)
                .after(main_transparent_pass_3d),
        );
}

/// The ground fog's render pass in the `Core3d` schedule, for what has to
/// run after it (the local fog's timing span).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct GroundFogPass;

/// The cloud shadow's mask in the render world, `None` before the clouds
/// are baked (the pass binds a white image then).
#[derive(Resource, Default)]
struct CloudMask(Option<AssetId<Image>>);

fn extract_cloud_mask(field: Extract<Option<Res<CloudField>>>, mut mask: ResMut<CloudMask>) {
    let id = field.as_ref().map(|f| f.mask().id());
    if mask.0 != id {
        mask.0 = id;
    }
}

/// A level's `-x -z` corner in render space, one over its extent, and
/// whether it holds data, for the uniform. Computed from the same
/// [`LevelTexels`] the render world uploads in the same extract.
fn level_uniform(level: Option<&Arc<LevelTexels>>, origin: &RenderOrigin) -> Vec4 {
    let Some(level) = level else {
        return Vec4::ZERO;
    };
    let corner = level.spec.origin(level.centre);
    // Within the clipmap's 32 km of the render origin: an f32 keeps
    // millimetres.
    #[allow(clippy::cast_possible_truncation)]
    Vec4::new(
        (corner[0] - origin.0.x()) as f32,
        (corner[1] - origin.0.z()) as f32,
        (1.0 / level.spec.extent_m()) as f32,
        1.0,
    )
}

/// The band the fog can occupy over the shown levels, for the uniform's
/// `slab`: the lowest base, the highest top with the noise and the falloff
/// over it, sea level and the noise's most. An empty band (bottom over top)
/// when no level has fog.
fn band(fog: &GroundFog, shown: &[Option<Arc<LevelTexels>>; 2], soft: f32, sea_level: f32) -> Vec4 {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for level in shown.iter().flatten() {
        if level.has_fog() {
            lo = lo.min(level.min_base);
            hi = hi.max(level.max_top);
        }
    }
    if hi >= lo {
        // The noise lifts the top by its most, and the falloff reaches
        // above the lifted top by as much again as it has below.
        let reach = fog.noise_max + soft + SOFT_SHARE * fog.noise_max;
        Vec4::new(lo.max(sea_level), hi + reach, sea_level, fog.noise_max)
    } else {
        Vec4::new(1.0, 0.0, sea_level, fog.noise_max)
    }
}

/// The uniform's `lighting`: the look's shadow strength, and the air
/// floor's extinction per metre (scaled with the basins'), the world height
/// it is full up to and its falloff.
fn lighting(fog: &GroundFog, look: &RenderLook, sea_level: f32) -> Vec4 {
    Vec4::new(
        look.valley_fog_shadow,
        look.valley_floor_density * 0.001 * fog.scale,
        sea_level + look.valley_floor_height,
        look.valley_floor_falloff.max(1.0),
    )
}

/// Writes every sky camera's uniform from this frame's sun, moon, origin,
/// clipmap and atmosphere, or takes it away when the look has no ground
/// fog or the camera is multisampled (so neither the pass nor its readable
/// depth runs for it). After transform propagation, so the origin is the
/// one the camera's transform was derived from.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn update_uniforms(
    mut commands: Commands,
    fog: Res<GroundFog>,
    look: Res<RenderLook>,
    sky: Option<Res<SkySettings>>,
    origin: Res<RenderOrigin>,
    clipmap: Option<Res<FogClipmap>>,
    source: Option<Res<WorldSourceRes>>,
    cameras: Query<
        (
            Entity,
            Option<&AtmosphereSettings>,
            Option<&Msaa>,
            Has<FogPassUniform>,
        ),
        With<SkyCamera>,
    >,
    planets: Query<&Atmosphere>,
    clouds: Option<Res<CloudProjection>>,
    suns: Query<(&Transform, &DirectionalLight), (With<Sun>, Without<Moon>)>,
    moons: Query<(&Transform, &DirectionalLight), (With<Moon>, Without<Sun>)>,
    mut frame: Local<u32>,
    mut warned: Local<bool>,
) {
    for (camera, _, msaa, has) in &cameras {
        let multisampled = !single_sampled(msaa);
        if look.ground_fog && multisampled && !*warned {
            *warned = true;
            tracing::warn!("ground fog skips a multisampled camera; use TAA or no AA to see it");
        }
        if has && (!look.ground_fog || multisampled) {
            commands.entity(camera).remove::<FogPassUniform>();
        }
    }
    if !look.ground_fog {
        return;
    }
    *frame = frame.wrapping_add(1);
    let light = |transform: &Transform, light: &DirectionalLight| {
        let towards = transform.rotation * Vec3::Z;
        let color = light.color.to_linear();
        (
            towards.extend(0.0),
            Vec4::new(color.red, color.green, color.blue, 0.0) * light.illuminance,
        )
    };
    let (sun_direction, sun_color) = suns
        .iter()
        .next()
        .map_or((Vec4::Y, Vec4::ZERO), |(t, l)| light(t, l));
    let (moon_direction, moon_color) = moons
        .iter()
        .next()
        .map_or((Vec4::Y, Vec4::ZERO), |(t, l)| light(t, l));
    let (inner_radius, outer_radius) = planets
        .iter()
        .next()
        .map_or((6_360_000.0, 6_460_000.0), |a| {
            (a.inner_radius, a.outer_radius)
        });
    let sea_level = source.map_or(0.0, |s| s.source().sea_level());
    let shown = clipmap.as_ref().map_or([None, None], |c| c.shown());
    let soft = fog.soft.max(0.5);
    let slab = band(&fog, &shown, soft, sea_level);
    // Wrapped into [0, period), which f32 holds to millimetres.
    let period = noise_period_m(fog.noise_cell);
    #[allow(clippy::cast_possible_truncation)]
    let wrapped = Vec4::new(
        origin.0.x().rem_euclid(period) as f32,
        origin.0.y() as f32,
        origin.0.z().rem_euclid(period) as f32,
        period as f32,
    );
    #[allow(
        clippy::cast_possible_truncation,
        reason = "one over a power of two from 8 to 4096, exact in f32"
    )]
    let per_cell = (1.0 / noise_cell_m(fog.noise_cell)) as f32;
    let sky_gain = fog.ambient * sky.map_or(1.0, |s| s.environment_intensity);
    let lighting = lighting(&fog, &look, sea_level);
    let [clouds, cloud_shift] = fog_shadow(clouds.as_deref(), sun_direction.truncate());
    let fine = level_uniform(shown[0].as_ref(), &origin);
    let coarse = level_uniform(shown[1].as_ref(), &origin);
    for (camera, settings, msaa, _) in &cameras {
        if !single_sampled(msaa) {
            continue;
        }
        let settings = settings.cloned().unwrap_or_default();
        #[allow(clippy::cast_precision_loss, reason = "LUT sizes are small")]
        let uniform = FogPassUniform {
            origin: wrapped,
            sun_direction,
            sun_color,
            moon_direction,
            moon_color,
            fine,
            coarse,
            slab,
            medium: Vec4::new(
                fog.extinction * fog.scale,
                fog.albedo,
                fog.asymmetry,
                fog.light,
            ),
            shape: Vec4::new(sky_gain, fog.self_shadow, per_cell, fog.noise_share),
            march: Vec4::new(fog.far, fog.step_curve.max(0.1), soft, fog.trough),
            counts: UVec4::new(fog.steps.max(1), *frame, 0, 0),
            atmosphere: Vec4::new(
                inner_radius,
                outer_radius,
                settings.aerial_view_lut_max_distance,
                settings.aerial_view_lut_size.z as f32,
            ),
            lut_sizes: settings
                .multiscattering_lut_size
                .as_vec2()
                .extend(settings.sky_view_lut_size.x as f32)
                .extend(settings.sky_view_lut_size.y as f32),
            lighting,
            clouds,
            cloud_shift,
        };
        commands.entity(camera).insert(uniform);
    }
}

/// The pass's layout, sampler and shaders, and its pipelines by target
/// format.
#[derive(Resource)]
struct FogPassPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    noise: TextureView,
    noise_sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
    pipelines: HashMap<TextureFormat, CachedRenderPipelineId>,
}

/// The pipeline chosen for a view.
#[derive(Component)]
struct FogPassPipelineId(CachedRenderPipelineId);

fn init_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    asset_server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "fog_pass_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<FogPassUniform>(true),
                texture_depth_2d(),
                // The atmosphere's transmittance, multiple-scattering and
                // aerial-view tables.
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                // The clipmap's fine and coarse levels.
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                // The noise the top's relief is read from, and its
                // repeating sampler.
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                // Bevy's lights (the sun's cascades), its directional
                // shadow maps and their comparison sampler: the shafts.
                uniform_buffer::<GpuLights>(true),
                texture_2d_array(TextureSampleType::Depth),
                sampler(SamplerBindingType::Comparison),
                // The atmosphere's sky-view table: the sky's radiance that
                // lights the fog from above (the multiple-scattering table
                // above is bound but no longer read).
                texture_2d(TextureSampleType::Float { filterable: true }),
                // The cloud shadow's mask and its own repeating sampler.
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    // Linear and clamped to the edge: the clipmap's edges are faded in the
    // shader, never wrapped.
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("fog_pass_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    // Value noise from one filtered read per octave: the shader moves the
    // read inside its cell along a smoothstep, which is what eight hashes
    // and their blend cost before (four times this pass's budget).
    let noise = render_device
        .create_texture_with_data(
            &render_queue,
            &TextureDescriptor {
                label: Some("fog_noise"),
                size: Extent3d {
                    width: NOISE_TEXELS,
                    height: NOISE_TEXELS,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R8Unorm,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &noise_texels(),
        )
        .create_view(&TextureViewDescriptor::default());
    let noise_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("fog_noise_sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    commands.insert_resource(FogPassPipeline {
        layout,
        sampler,
        noise,
        noise_sampler,
        fullscreen: fullscreen.clone(),
        shader: asset_server.load(FOG_SHADER_PATH),
        pipelines: HashMap::new(),
    });
}

/// The pass samples the depth attachment, which is only readable when the
/// camera asks for it. Bevy's atmosphere asks already; this keeps the pass
/// from depending on that.
fn readable_depth(mut cameras: Query<&mut Camera3d, With<FogPassUniform>>) {
    for mut camera in &mut cameras {
        let usage = TextureUsages::TEXTURE_BINDING.bits();
        if camera.depth_texture_usages.0 & usage == 0 {
            camera.depth_texture_usages.0 |= usage;
        }
    }
}

fn prepare_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<FogPassPipeline>,
    views: Query<(Entity, &ExtractedView), With<FogPassUniform>>,
) {
    for (entity, view) in &views {
        let format = view.target_format;
        let FogPassPipeline {
            layout,
            fullscreen,
            shader,
            pipelines,
            ..
        } = &mut *pipeline;
        let id = *pipelines.entry(format).or_insert_with(|| {
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("fog_pass_pipeline".into()),
                layout: vec![layout.clone()],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    targets: vec![Some(ColorTargetState {
                        format,
                        // The shader returns the light the fog adds in
                        // `rgb` and what it lets through in `a`:
                        // `out = src.rgb + dst * src.a`.
                        blend: Some(BlendState {
                            color: BlendComponent {
                                src_factor: BlendFactor::One,
                                dst_factor: BlendFactor::SrcAlpha,
                                operation: BlendOperation::Add,
                            },
                            alpha: BlendComponent {
                                src_factor: BlendFactor::Zero,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                        }),
                        write_mask: ColorWrites::ALL,
                    })],
                    ..Default::default()
                }),
                ..Default::default()
            })
        });
        commands.entity(entity).insert(FogPassPipelineId(id));
    }
}

/// Draws the fog over the view's colour, reading its depth and the clipmap.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn fog_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &ViewUniformOffset,
        &FogPassUniform,
        &DynamicUniformIndex<FogPassUniform>,
        &FogPassPipelineId,
        Option<&AtmosphereTextures>,
        &Msaa,
        Option<(&ViewLightsUniformOffset, &ViewShadowBindings)>,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<FogPassPipeline>,
    view_uniforms: Res<ViewUniforms>,
    fog_uniforms: Res<ComponentUniforms<FogPassUniform>>,
    light_meta: Res<LightMeta>,
    shadow_samplers: Res<ShadowSamplers>,
    textures: Option<Res<ClipmapTextures>>,
    cloud_mask: Res<CloudMask>,
    images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    mut warned: Local<bool>,
    mut ctx: RenderContext,
) {
    let (target, depth, view_offset, _, fog_offset, pipeline_id, atmosphere, msaa, lights) =
        view.into_inner();
    // `update_uniforms` gives a multisampled camera no uniform, so this is
    // only the frame on which a camera's MSAA changed.
    if *msaa != Msaa::Off {
        return;
    }
    let (Some(atmosphere), Some((lights_offset, shadows))) = (atmosphere, lights) else {
        if !*warned {
            *warned = true;
            tracing::warn!("ground fog skips a camera without Bevy's atmosphere or lights");
        }
        return;
    };
    let (
        Some(render_pipeline),
        Some(view_binding),
        Some(fog_binding),
        Some(lights_binding),
        Some(textures),
    ) = (
        pipeline_cache.get_render_pipeline(pipeline_id.0),
        view_uniforms.uniforms.binding(),
        fog_uniforms.uniforms().binding(),
        light_meta.view_gpu_lights.binding(),
        textures,
    )
    else {
        return;
    };
    // White until the clouds are baked; the shader reads it only while the
    // uniform says the sun carries the mask.
    let mask = cloud_mask
        .0
        .and_then(|id| images.get(id))
        .unwrap_or(&fallback.d2);
    let bind_group = ctx.render_device().create_bind_group(
        "fog_pass_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            view_binding,
            fog_binding,
            depth.view(),
            &atmosphere.transmittance_lut.default_view,
            &atmosphere.multiscattering_lut.default_view,
            &atmosphere.aerial_view_lut.default_view,
            &pipeline.sampler,
            &textures.levels[0].view,
            &textures.levels[1].view,
            &pipeline.noise,
            &pipeline.noise_sampler,
            lights_binding,
            &shadows.directional_light_depth_texture_view,
            &shadow_samplers.directional_light_comparison_sampler,
            &atmosphere.sky_view_lut.default_view,
            &mask.texture_view,
            &mask.sampler,
        )),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let encoder = ctx.command_encoder();
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("fog_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target.main_texture_view(),
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let span = diagnostics.pass_span(&mut pass, "ground_fog");
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(
        0,
        &bind_group,
        &[view_offset.offset, fog_offset.index(), lights_offset.offset],
    );
    pass.draw(0..3, 0..1);
    span.end(&mut pass);
}

/// Every two seconds, the pass's own GPU time, when Bevy's render
/// diagnostics are on (the client's statistics turn them on): what the
/// step count is tuned against.
fn log_gpu_time(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Res<bevy::time::Time<bevy::time::Real>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(ms) = diagnostics.as_ref().and_then(|d| {
        d.iter()
            .find(|d| d.path().as_str().ends_with("/ground_fog/elapsed_gpu"))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    }) else {
        return;
    };
    tracing::info!("ground fog: {ms:.3} ms of GPU time");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_registered_without_fog() {
        for fog in [None, Some(0.0)] {
            let mut app = App::new();
            if let Some(scale) = fog {
                app.insert_resource(GroundFog {
                    scale,
                    ..Default::default()
                });
            }
            register(&mut app);
            assert!(
                !app.is_plugin_added::<UniformComponentPlugin<FogPassUniform>>(),
                "{fog:?}"
            );
        }
        let mut app = App::new();
        app.insert_resource(GroundFog::default());
        register(&mut app);
        assert!(app.is_plugin_added::<UniformComponentPlugin<FogPassUniform>>());
    }

    #[test]
    fn a_rebase_moves_no_level() {
        use strata_core::WorldPos;
        use strata_weather::FogField;
        let spec = crate::fog_clipmap::LevelSpec::COARSE;
        let centre = spec.snapped_centre(-16_000.0, -13_700.0);
        let level = Arc::new(crate::fog_clipmap::sample_level(
            &FogField::new(strata_core::Seed(1)),
            spec,
            centre,
            Weather::CLEAR,
            1,
        ));
        let corner = spec.origin(centre);
        // The origin rebases every 2 km; wherever it is, the corner the
        // shader gets plus the origin is the same world point.
        for (x, z) in [(0.0, 0.0), (-16_000.0, -14_000.0), (-18_000.0, -12_000.0)] {
            let origin = RenderOrigin(WorldPos::new(x, 37.0, z));
            let u = level_uniform(Some(&level), &origin);
            assert!((f64::from(u.x) + x - corner[0]).abs() < 1e-2, "{u:?}");
            assert!((f64::from(u.y) + z - corner[1]).abs() < 1e-2, "{u:?}");
            assert!((f64::from(u.z) * spec.extent_m() - 1.0).abs() < 1e-6);
            assert!((u.w - 1.0).abs() < f32::EPSILON);
        }
        assert_eq!(
            level_uniform(None, &RenderOrigin(WorldPos::new(5.0, 0.0, 5.0))),
            Vec4::ZERO
        );
    }

    #[test]
    fn a_multisampled_camera_gets_no_pass_and_nothing_is_baked_for_it() {
        use bevy::render::view::Msaa;
        struct Flat;
        impl strata_worldgen::WorldSource for Flat {
            fn sample(&self, _x: f64, _z: f64) -> strata_worldgen::Sample {
                strata_worldgen::Sample {
                    height: 10.0,
                    biomes: [
                        (strata_registry::BiomeId(0), 1.0),
                        (strata_registry::BiomeId(0), 0.0),
                    ],
                    provinces: strata_worldgen::Sample::NO_PROVINCES,
                    material: 0,
                    moisture: 0.5,
                    hydration: 0.5,
                    cover: 0.5,
                    landform: strata_worldgen::Landform::default(),
                    water: None,
                }
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, strata_app::AppPlugin))
            .insert_resource(GroundFog::default())
            .insert_resource(RenderLook::default())
            .insert_resource(WorldSourceRes(Arc::new(Flat)))
            .insert_resource(strata_app::WorldSeed(strata_core::Seed(3)))
            .add_systems(Update, (update_clipmap.run_if(fog_drawn), update_uniforms));
        let camera = app.world_mut().spawn((SkyCamera, Msaa::Sample4)).id();
        app.update();
        app.update();
        assert!(app.world().get::<FogPassUniform>(camera).is_none());
        assert!(app.world().get_resource::<FogClipmap>().is_none());
        // TAA or no AA: the camera draws it and the clipmap starts.
        app.world_mut().entity_mut(camera).insert(Msaa::Off);
        app.update();
        app.update();
        assert!(app.world().get::<FogPassUniform>(camera).is_some());
        assert!(app.world().get_resource::<FogClipmap>().is_some());
        // And back to MSAA: the uniform goes again.
        app.world_mut().entity_mut(camera).insert(Msaa::Sample4);
        app.update();
        assert!(app.world().get::<FogPassUniform>(camera).is_none());
    }

    /// `fog.wgsl` with Bevy's two imports replaced by the parts of them it
    /// uses, as Bevy 0.19.1 declares them (`bevy_render/src/view/view.wgsl`
    /// and `bevy_core_pipeline/src/fullscreen_vertex_shader/fullscreen.wgsl`).
    fn fog_shader_module() -> naga::Module {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/",
            "shaders/fog.wgsl"
        );
        let source = std::fs::read_to_string(path).expect("the shader is on disk");
        let stubs = "struct FullscreenVertexOutput {\n\
             @builtin(position) position: vec4<f32>,\n\
             @location(0) uv: vec2<f32>,\n\
             };\n\
             struct View {\n\
             world_from_clip: mat4x4<f32>,\n\
             world_from_view: mat4x4<f32>,\n\
             world_position: vec3<f32>,\n\
             exposure: f32,\n\
             };\n";
        let mut imports = 0;
        let body: String = source
            .lines()
            .filter(|line| {
                let import = line.starts_with("#import");
                imports += usize::from(import);
                !import
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(imports, 2, "the stubs stand for exactly two imports");
        naga::front::wgsl::parse_str(&format!("{stubs}{body}"))
            .unwrap_or_else(|e| panic!("fog.wgsl does not parse: {}", e.emit_to_string(&body)))
    }

    #[test]
    fn the_fog_shader_parses_and_validates() {
        let module = fog_shader_module();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("fog.wgsl does not validate: {e:?}"));
        assert!(module.entry_points.iter().any(|e| e.name == "fragment"));
    }

    #[test]
    fn the_shader_and_the_band_agree_on_the_falloff() {
        // The band rays are clipped to assumes the shader's falloff share.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/",
            "shaders/fog.wgsl"
        );
        let source = std::fs::read_to_string(path).expect("the shader is on disk");
        assert!(source.contains(&format!("{SOFT_SHARE} * col.amp")));
        assert!(source.contains(&format!("fog.slab.w * {}", 1.0 + SOFT_SHARE)));
    }

    #[test]
    fn the_uniform_matches_the_shader_struct_field_for_field() {
        let module = fog_shader_module();
        let (members, span) = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, span }
                    if ty.name.as_deref() == Some("FogPass") =>
                {
                    Some((members.clone(), *span))
                }
                _ => None,
            })
            .expect("fog.wgsl declares FogPass");
        let shader: Vec<&str> = members.iter().filter_map(|m| m.name.as_deref()).collect();
        // The Rust side's field names, in order, from its `Debug`.
        let debug = format!("{:?}", FogPassUniform::default());
        let rust: Vec<&str> = debug
            .split(['{', ','])
            .filter_map(|part| part.split_once(':').map(|(name, _)| name.trim()))
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .collect();
        assert_eq!(rust, shader);
        assert_eq!(
            u64::from(span),
            <FogPassUniform as ShaderType>::min_size().get(),
            "the shader's struct and the uniform differ in size"
        );
    }

    /// The size naga gives a struct the shader declares.
    fn shader_struct_size(module: &naga::Module, name: &str) -> u64 {
        module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { span, .. } if ty.name.as_deref() == Some(name) => {
                    Some(u64::from(*span))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("fog.wgsl declares {name}"))
    }

    #[test]
    fn the_shader_reads_bevy_s_lights_as_bevy_lays_them_out() {
        use bevy::pbr::{
            GpuDirectionalLight, GpuLights, MAX_CASCADES_PER_LIGHT, MAX_DIRECTIONAL_LIGHTS,
        };
        let module = fog_shader_module();
        // The array lengths the shader writes out are Bevy's constants.
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/shaders/fog.wgsl"
        ))
        .expect("the shader is on disk");
        assert!(source.contains(&format!("array<ShadowCascade, {MAX_CASCADES_PER_LIGHT}u>")));
        assert!(source.contains(&format!("array<ShadowLight, {MAX_DIRECTIONAL_LIGHTS}u>")));
        assert!(source.contains(&format!(
            "MAX_DIRECTIONAL_LIGHTS: u32 = {MAX_DIRECTIONAL_LIGHTS}u"
        )));
        // A light is laid out exactly as Bevy writes it, so the array
        // strides agree; the shader's head of `Lights` fits in Bevy's.
        assert_eq!(
            shader_struct_size(&module, "ShadowLight"),
            <GpuDirectionalLight as ShaderType>::min_size().get()
        );
        assert!(
            shader_struct_size(&module, "ShadowLights")
                <= <GpuLights as ShaderType>::min_size().get()
        );
    }

    #[test]
    fn the_looks_fog_light_is_range_checked() {
        use crate::look::LookFile;
        assert!(LookFile::default().validate().is_ok());
        let defaults = RenderLook::default();
        assert!((defaults.valley_fog_shadow - 1.0).abs() < f32::EPSILON);
        assert!((defaults.valley_floor_density - 0.004).abs() < f32::EPSILON);
        for edit in [
            |l: &mut RenderLook| l.valley_fog_shadow = 1.5,
            |l: &mut RenderLook| l.valley_fog_shadow = -0.1,
            |l: &mut RenderLook| l.valley_floor_density = f32::NAN,
            |l: &mut RenderLook| l.valley_floor_density = 0.2,
            |l: &mut RenderLook| l.valley_floor_height = 9_000.0,
            |l: &mut RenderLook| l.valley_floor_falloff = 0.0,
        ] {
            let mut file = LookFile::default();
            edit(&mut file.look);
            assert!(file.validate().is_err(), "{:?}", file.look);
        }
        // Zero is valid and means no shadow lookup and no floor.
        let mut off = LookFile::default();
        off.look.valley_fog_shadow = 0.0;
        off.look.valley_floor_density = 0.0;
        assert!(off.validate().is_ok());
        // A look exported before these fields existed loads with them at
        // their defaults.
        let text = LookFile::default()
            .to_ron()
            .expect("export")
            .lines()
            .filter(|line| !line.contains("valley_fog_shadow") && !line.contains("valley_floor"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("valley_"));
        assert_eq!(LookFile::from_ron(&text).expect("import").look, defaults);
    }

    #[test]
    fn a_look_without_ground_fog_takes_the_uniform_away() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, strata_app::AppPlugin))
            .insert_resource(GroundFog::default())
            .insert_resource(RenderLook::default())
            .add_systems(Update, update_uniforms);
        let camera = app.world_mut().spawn(SkyCamera).id();
        app.update();
        assert!(app.world().get::<FogPassUniform>(camera).is_some());
        app.world_mut().resource_mut::<RenderLook>().ground_fog = false;
        app.update();
        assert!(app.world().get::<FogPassUniform>(camera).is_none());
    }
}
