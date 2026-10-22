//! Reproducible render looks and their live application to sky cameras.

use crate::{NoSky, SkyCamera, SkySettings, Sun};
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::Assets;
use bevy::camera::{Camera3d, Exposure};
use bevy::color::Color;
use bevy::core_pipeline::{
    prepass::{MotionVectorPrepass, NormalPrepass},
    tonemapping::Tonemapping,
};
use bevy::ecs::prelude::*;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, AtmosphereEnvironmentMapLight, EnvironmentMapLight, GeneratedEnvironmentMapLight,
    ShadowFilteringMethod, VolumetricFog, VolumetricLight,
};
// Irradiance probes (lighting plan phase 3b).
use bevy::camera::visibility::Visibility;
use bevy::light::IrradianceVolume;
use bevy::pbr::{
    AtmosphereSettings, ContactShadows, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::{
    bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
    dof::{DepthOfField, DepthOfFieldMode},
    motion_blur::MotionBlur,
};
use bevy::render::{
    camera::{MipBias, TemporalJitter},
    view::{ColorGrading, Msaa},
};
use serde::{Deserialize, Serialize};

/// The stable names accepted by the CLI and exported files.
pub const PRESETS: [&str; 4] = [
    "baseline",
    "clear-contrast",
    "sunlit-mist",
    "cinematic-woodland",
];

/// How the camera filters the sun's shadow edges, Bevy's
/// [`ShadowFilteringMethod`] by a name a look file can carry.
///
/// Serialised as its lowercase name in a string, so an exported look reads
/// `shadow_filter: "temporal"` and a misspelt name is a load error, not a
/// silently different look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ShadowFilter {
    /// One hardware comparison of four texels: the cheapest and the most
    /// aliased; the reference the other two are measured against.
    Hardware2x2,
    /// A fixed nine-tap blur standing in for a 5x5 kernel, for a camera
    /// without TAA, which would otherwise see the temporal filter's noise.
    Gaussian,
    /// Eight taps in a spiral rotated per pixel and per frame, which TAA
    /// averages into a wider, smoother edge than the Gaussian's for about
    /// its cost. Also the filter PCSS ([`SkySettings::soft_shadow_size`])
    /// wants, since its blur is the same rotated pattern.
    #[default]
    Temporal,
}

impl ShadowFilter {
    /// Every filter in the order the F6 panel lists them.
    pub const ALL: [Self; 3] = [Self::Hardware2x2, Self::Gaussian, Self::Temporal];

    /// The name in a look file and on the panel.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hardware2x2 => "hardware2x2",
            Self::Gaussian => "gaussian",
            Self::Temporal => "temporal",
        }
    }

    /// The filter a camera gets: the temporal one only under TAA, since
    /// without it nothing averages its per-frame rotation and the edge
    /// crawls; the Gaussian is the same cost and stable.
    #[must_use]
    pub fn on_camera(self, taa: bool) -> ShadowFilteringMethod {
        match self {
            Self::Hardware2x2 => ShadowFilteringMethod::Hardware2x2,
            Self::Temporal if taa => ShadowFilteringMethod::Temporal,
            Self::Gaussian | Self::Temporal => ShadowFilteringMethod::Gaussian,
        }
    }
}

impl From<ShadowFilter> for String {
    fn from(filter: ShadowFilter) -> Self {
        filter.as_str().into()
    }
}

impl TryFrom<String> for ShadowFilter {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|f| f.as_str() == name)
            .ok_or_else(|| {
                format!("unknown shadow filter {name:?}: hardware2x2, gaussian or temporal")
            })
    }
}

/// Camera effects; zero strengths disable the corresponding render pass.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent render passes need independent switches"
)]
pub struct RenderLook {
    /// Descriptive source preset; edits keep this for Reset.
    pub preset: String,
    /// `AgX` instead of the baseline ACES display transform.
    pub agx: bool,
    /// Default antialiasing when the camera has no explicit override.
    pub taa: bool,
    /// No antialiasing unless the camera's own override asks for one. Only
    /// the graphics preset writes it (`strata_client::ui::graphics`): MSAA
    /// or TAA is the one cost the presets can drop that no other field says.
    pub no_aa: bool,
    /// The share of the window's width and height the 3D camera renders at,
    /// 0.5 to 1.0 ([`crate::render_scale`]). Under 1 the camera draws to an
    /// image that a second camera stretches over the window and sharpens, so
    /// every pass that reads the view's size follows the smaller image and
    /// the UI stays at the window's. A look saved before the field existed
    /// renders at the window's size.
    pub render_scale: f32,
    /// Contrast around middle grey, before tonemapping.
    pub contrast: f32,
    /// Saturation after tonemapping.
    pub saturation: f32,
    /// Contact shadows from the sun, in addition to its shadow maps.
    pub contact_shadows: bool,
    /// Medium-quality ambient occlusion; only active without MSAA.
    pub ssao: bool,
    /// Bloom strength: with a [`bloom_threshold`](Self::bloom_threshold),
    /// the share of the light over it that is added back blurred; at a
    /// threshold of zero, Bevy's energy-conserving scatter of everything.
    pub bloom: f32,
    /// Density of the authored study volumes; zero disables volumetrics.
    pub fog_density: f32,
    /// Fraction of light scattered by the local mist.
    pub fog_scattering: f32,
    /// Artistic multiplier on light scattered by the local mist.
    pub fog_light_intensity: f32,
    /// Forward scattering makes shafts strongest toward the sun.
    pub fog_asymmetry: f32,
    /// Quality of the fog raymarch.
    pub fog_steps: u32,
    /// Gaussian lens blur, independently switchable.
    pub dof: bool,
    /// Manual focus distance in metres.
    pub focus_distance: f32,
    /// Lens aperture in f-stops; independent from scene exposure.
    pub aperture: f32,
    /// Maximum lens blur diameter in pixels.
    pub blur_diameter: f32,
    /// Fraction of a frame exposed for motion blur; zero disables it.
    pub motion_blur: f32,
    /// The ground fog pass (ticket 0003), where the game turned it on with
    /// `--fog`. Every preset draws it, `baseline` (the default, and what a
    /// missing export falls back to) included, so the default look, a
    /// comparison against `baseline` and Reset from a fresh start all agree.
    /// The benches keep their numbers comparable with the ledger's older
    /// ones with `--fog 0`, which registers no pass at all. A look saved
    /// before the switch existed has no entry and draws it.
    pub ground_fog: bool,
    /// The brightness, in the camera's exposed HDR units (1 is what the
    /// tonemapper maps to near white), over which a pixel blooms, with a
    /// soft knee half as wide. Bevy's first bloom downsample runs before
    /// the threshold: it damps each of its five tap groups to a luminance
    /// of about 4, so what reaches the threshold tops out near 20 for a
    /// broad bright patch and near 16 for a lone texel like the sun's
    /// disc, and it averages a small light with its dark surround, so a
    /// lit window a few pixels wide needs to be well over the threshold;
    /// one under the lit ground's brightness blurs the ground.
    /// Zero is Bevy's physical, unthresholded, energy-conserving bloom,
    /// which haloes all of it alike.
    pub bloom_threshold: f32,
    /// Strength of the screen-space light rays from the sun
    /// ([`crate::light_rays`]); zero takes the pass away.
    pub light_rays_strength: f32,
    /// How much each of the rays' steps towards the sun weighs over the
    /// one before: under 1 a pixel's ray is shaped most by what is near
    /// it.
    pub light_rays_decay: f32,
    /// Steps per pixel from it towards the sun, at half resolution.
    pub light_rays_steps: u32,
    /// Linear tint on the rays, which otherwise take the sun's colour
    /// through the atmosphere.
    pub light_rays_tint: [f32; 3],
    /// How shadow edges are filtered. `temporal` falls back to `gaussian`
    /// on a camera without TAA ([`ShadowFilter::on_camera`]), so the one
    /// default serves both antialiasing choices. A look saved before the
    /// field existed loads as `temporal`; until then every camera had
    /// Bevy's default, the Gaussian, TAA or not.
    pub shadow_filter: ShadowFilter,
    // Local fog volumes (lighting plan phase 2c).
    /// Bevy's `VolumetricFog` on the camera and the sun as a
    /// `VolumetricLight`, so the volumes the city and the foliage spawn
    /// (under the market's canopies, under dense crowns) draw: dusty air
    /// that the sun's shadow cuts into shafts. Off, they are not marched at
    /// all. The study scene's own mist takes the camera over while its
    /// [`fog_density`](Self::fog_density) is over zero. A look saved before
    /// the field existed draws them.
    pub local_fog: bool,
    /// Steps per pixel through each local volume. A camera inside a volume
    /// marches every pixel of the screen, so this is the cost: measured
    /// 2026-10-03 at 2560x1440 inside a market run (median GPU time of the
    /// pass), 0.26 ms at 4, 0.34 at 6 and 0.43 at 8, against the phase's
    /// 0.4 ms ceiling. Jitter and TAA hide the step pattern of 6 steps
    /// across a box a few metres deep (the captures at 6 and 8 differ by
    /// under one level of 255 on average); Bevy's default of 64 is for a
    /// valley.
    pub local_fog_steps: u32,
    // The valley fog's light (lighting plan phase 2a). In the look, not in
    // `GroundFog`, so the F6 panel and an export reach them.
    /// How much the sun's shadow map darkens the valley fog's sunlight, 0
    /// to 1: hills and buildings cut shafts through it. Zero skips the
    /// lookup. Beyond the last cascade the fog's own shadow is all there is.
    pub valley_fog_shadow: f32,
    /// Extinction per kilometre of the thin height fog under every basin's
    /// layer, so a valley has some air at dusk and far ground softens.
    /// Not thin beside the atmosphere's own air: the default 0.004 is 1.6
    /// times the sea-level aerosol extinction at the default haze and,
    /// nearly white (albedo 0.95), scatters about half as much again as the
    /// whole sea-level air in green, so near the ground it adds about half
    /// to the aerial perspective, more in red. A ray to the sky's horizon
    /// takes about 110 km of it from under its height (optical depth 0.44).
    /// What keeps it a few levels of 255 (2026-10-03, the plain looking into
    /// a 9 degree sun: the horizon band +4, the sky +1.4) is that its light
    /// reaches the eye through the air in front of it. Zero takes it away.
    /// The valley fog's pass draws it, so with `--fog 0` (the benches, the
    /// look fixtures but `dawn`) there is none either.
    pub valley_floor_density: f32,
    /// Metres above sea level up to which that floor is at full density.
    pub valley_floor_height: f32,
    /// Metres over which the floor thins by `e` above its height.
    pub valley_floor_falloff: f32,
    // Irradiance probes (lighting plan phase 3b, `strata_terrain::probes`).
    /// Strength of the irradiance probes the terrain bakes per region (the
    /// sky by direction and one bounce off the ground), as the intensity
    /// of every `IrradianceVolume`, times the sky fill
    /// ([`SkySettings::environment_intensity`]) since the probes take the
    /// environment map's diffuse light over. At zero the probes are hidden
    /// and the environment map lights the shade as before. A look saved
    /// before the field existed draws them at 1.
    pub irradiance: f32,
    // Sky visibility (lighting plan phase 3a).
    /// How much the sky visibility baked into the ground (per vertex, by
    /// the terrain mesher) and under the crowns (per plant and blade, by
    /// the flora placer) darkens the sky's fill and, on the ground, turns
    /// its lookup towards the open sky, 0 to 1: gullies and the ground
    /// under crowns darker than ridges. Zero is the look before the bake;
    /// free per frame either way. A look saved before the field existed
    /// draws it at 1.
    pub sky_visibility: f32,
    // Colour scripting (lighting plan phase 4a, `crate::grade`).
    /// Strength of the colour grade by biome and time of day
    /// (`assets/biomes/*.biome.ron` and `assets/look/times.ron`), 0 to 1:
    /// at 0 the camera has the look's own contrast and saturation alone,
    /// at 1 the grades in full over them. A look saved before the field
    /// existed grades at 1.
    pub grade: f32,
    // Exposure and the lens (lighting plan phase 4b, `crate::exposure`).
    /// The manual EV curve alone, or with Bevy's histogram auto exposure
    /// correcting it inside [`auto_exposure_range_ev`](Self::auto_exposure_range_ev).
    pub exposure_mode: crate::exposure::ExposureMode,
    /// How far, in EV, auto exposure may move from the manual curve: the
    /// first the most it brightens (negative EV), the second the most it
    /// darkens. The manual curve stays the anchor, so the time of day's
    /// key is kept and auto only wanders round it; at a darkening end of
    /// zero a scene brighter than the key is the manual curve's, so the
    /// city at dusk cannot be pushed past what it was tuned at.
    pub auto_exposure_range_ev: [f32; 2],
    /// The metered mean log2 luminance, of the frame as the manual curve
    /// exposed it, at which auto exposure leaves the manual curve alone;
    /// a darker frame is brightened and a brighter one darkened from there.
    pub auto_exposure_key: f32,
    /// Stops a second auto exposure moves when the scene got brighter
    /// (out of the trees into a field): slow enough that the field's glare
    /// is felt before it settles.
    pub auto_exposure_speed_brighten: f32,
    /// Stops a second when the scene got darker (into shade): quicker, so
    /// the forest floor does not sit murky while the eye catches up.
    pub auto_exposure_speed_darken: f32,
    /// How far the frame's corners darken, 0 to 1 (Bevy's `Vignette`
    /// intensity): holds the eye in and frames the edges. Zero takes the
    /// pass away.
    pub vignette: f32,
    /// The vignette's radius, in Bevy's units of the frame's shorter half
    /// diagonal: where the darkening has fully arrived.
    pub vignette_radius: f32,
    /// The vignette's falloff exponent: higher keeps more of the frame
    /// clear and darkens only the corners.
    pub vignette_smoothness: f32,
    /// The chromatic aberration a fresh hurt reaches, 0 to 1 (the client
    /// drives it from the hit's envelope, and half of it while down); 1 is
    /// [`crate::exposure::ABERRATION_FULL`]. Nothing at rest.
    pub aberration: f32,
    // Auto exposure by night (lighting plan phase 4, after review).
    /// The share of auto exposure's brightening end
    /// ([`auto_exposure_range_ev`](Self::auto_exposure_range_ev)) kept at
    /// full [`crate::Night::darkness`], 0 to 1, eased with it through dusk.
    /// The key is a daylight scene's, so at 1 a dark wood at midnight is
    /// lifted towards a cloudy morning and undoes `night_ev100` and the
    /// night grade; at 0 the night is the manual curve's.
    pub auto_exposure_night: f32,
    // The water (lighting plan phase 5a, `strata_terrain::water`).
    /// How much of the sky and the sun the sea reflects, 0 to 1: one is
    /// Schlick's Fresnel from the waves' normal with the water's
    /// reflectance, zero a matte sea that shows only its body and its
    /// floor. The sky is reflected at its own radiance, not the sky fill's.
    /// A look saved before the field existed reflects at 1.
    pub water_reflection: f32,
    /// How much foam the sea draws, 0 to 1: the band along the shore (where
    /// the water is shallow or the land a few metres off) and the high
    /// crests near the camera. A look saved before the field existed foams
    /// at 1.
    pub water_foam: f32,
    // Grass against the low sun (lighting plan phase 5b).
    /// How much sunlight comes through a blade seen against the sun and
    /// rims its edges, 0 to 2: the thin-leaf transmission Bevy's own
    /// `diffuse_transmission` cannot give a blade lit by the ground's
    /// normal. Shadowed by the sun's cascades and darkened under crowns, so
    /// only grass in the sun glows. Zero is the grass before the term, and
    /// skips its shadow lookup. A look saved before the field existed rims
    /// at 1.
    pub grass_rim: f32,
    // The ground painted (ticket 0006, pass 1; `strata_terrain`'s
    // `TerrainParams` has each one's meaning, and its defaults are these).
    // A look saved before the fields existed paints the ground at them.
    /// How far the lush and dry patches shift the living cover's hue
    /// (grass, forest floor), 0 to 2; zero is the ground before the pass.
    pub ground_colour: f32,
    /// How far the warm and cool patches shift it, 0 to 2.
    pub ground_warmth: f32,
    /// The share of the way the near ground's hue is pulled to its biome's
    /// palette colour, 0 to 1.
    pub ground_near_tint: f32,
    /// The share of that pull that moves the near ground's value too, 0 to
    /// 1.
    pub ground_near_value: f32,
    /// The ground texture's detail contrast about its mean, 1 as baked.
    pub ground_grain: f32,
    /// The strength of the ground's normal maps, 1 as baked.
    pub ground_normal: f32,
    /// How far the ground's roughness goes towards matte, 0 to 1.
    pub ground_matte: f32,
    /// How dark the drawn edge where two ground layers meet is, 0 to 1.
    pub ground_seam: f32,
    /// How much air the far ground takes, as a share of the terrain's own
    /// lift and loss of saturation from 2 km, 0 to 2; zero is none.
    pub ground_air: f32,
}

impl Default for RenderLook {
    fn default() -> Self {
        Self {
            preset: "baseline".into(),
            agx: false,
            // Ambient occlusion grounds a building where it meets the street
            // more than any geometry does, and Bevy's needs TAA in place of
            // MSAA (chosen 2026-09-27).
            taa: true,
            no_aa: false,
            render_scale: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            contact_shadows: false,
            ssao: true,
            // Only what is over the threshold blooms: the sun, the sky by
            // it and emissives (chosen 2026-10-02, the lighting plan's
            // phase 1); the study measured 0.23 ms for bloom at 0.025 to
            // 0.04.
            bloom: 0.03,
            fog_density: 0.0,
            fog_scattering: 0.6,
            fog_light_intensity: 4.0,
            fog_asymmetry: 0.6,
            fog_steps: 32,
            dof: false,
            focus_distance: 10.0,
            aperture: 2.8,
            blur_diameter: 8.0,
            motion_blur: 0.0,
            ground_fog: true,
            // Measured 2026-10-02 at 1600x900 against captures without
            // bloom. At 0.6 the coast's sand and sea at noon (up to 1.8)
            // bloomed and haloed the bushes on the sand; at 2 the coast and
            // the city's pale walls at noon differ from no bloom no more
            // than two captures without it differ from each other, while
            // the city's emissive strips at night keep half their glow at
            // 0.6. Snow, about 1.4 times sand's colour, is near the knee.
            bloom_threshold: 2.0,
            light_rays_strength: 0.35,
            light_rays_decay: 0.96,
            light_rays_steps: 24,
            light_rays_tint: [1.0, 0.94, 0.84],
            // Every preset runs TAA, which the temporal filter is made for.
            shadow_filter: ShadowFilter::Temporal,
            // Local fog volumes (lighting plan phase 2c, 2026-10-02).
            local_fog: true,
            local_fog_steps: 6,
            // The valley fog's light (lighting plan phase 2a, 2026-10-02).
            valley_fog_shadow: 1.0,
            valley_floor_density: 0.004,
            valley_floor_height: 400.0,
            valley_floor_falloff: 300.0,
            // Irradiance probes (lighting plan phase 3b, 2026-10-03).
            irradiance: 1.0,
            // Sky visibility (lighting plan phase 3a, 2026-10-03).
            sky_visibility: 1.0,
            // Colour scripting (lighting plan phase 4a, 2026-10-03).
            grade: 1.0,
            // Exposure and the lens (lighting plan phase 4b, 2026-10-03,
            // `docs/research/render-look-study.md`). Auto, because the
            // forest floor at 08:00 metered 1.4 to 1.9 stops under the open
            // field the manual curve was tuned on (a mean of 20 of 255) and
            // auto lifts it while the field, the city and the coast stay as
            // tuned. The band only brightens: with a darkening end of 1.5
            // the coast at noon lost 1.55 stops, the dusk street 0.78, and
            // a cloud's shadow crossing a field was half compensated away.
            // The key is the open field at 08:00 in a cloud's shadow, so
            // sunlit ground, a sunlit street and the night city sit over it
            // and are left to the manual curve.
            exposure_mode: crate::exposure::ExposureMode::Auto,
            auto_exposure_range_ev: [-1.5, 0.0],
            auto_exposure_key: -4.0,
            auto_exposure_speed_brighten: 1.5,
            auto_exposure_speed_darken: 3.0,
            vignette: 0.18,
            vignette_radius: 0.75,
            vignette_smoothness: 5.0,
            aberration: 0.6,
            // Auto exposure by night (lighting plan phase 4, after review,
            // 2026-10-03): none, so the night stays the manual curve's.
            auto_exposure_night: 0.0,
            // The water (lighting plan phase 5a, 2026-10-03,
            // `docs/research/render-look-study.md`).
            water_reflection: 1.0,
            water_foam: 1.0,
            // Grass against the low sun (lighting plan phase 5b,
            // 2026-10-03, `docs/research/render-look-study.md`).
            grass_rim: 1.0,
            // The ground painted (ticket 0006, pass 1, 2026-10-10).
            ground_colour: 1.4,
            ground_warmth: 0.75,
            ground_near_tint: 0.95,
            ground_near_value: 0.0,
            ground_grain: 0.7,
            ground_normal: 0.6,
            ground_matte: 0.55,
            ground_seam: 0.35,
            ground_air: 1.0,
        }
    }
}

/// A complete export, including the sky parameters the camera effects depend on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookFile {
    /// Exposure, sun and atmosphere.
    pub sky: SkySettings,
    /// Grading and independently controlled passes.
    pub look: RenderLook,
}

impl LookFile {
    /// The presets intentionally share ACES so lighting differences remain apparent.
    pub fn preset(name: &str) -> Result<Self, String> {
        if !PRESETS.contains(&name) {
            return Err(format!("unknown render look {name:?}"));
        }
        let mut result = Self::default();
        result.look.preset = name.into();
        if name == "baseline" {
            return Ok(result);
        }
        result.sky.ev100 = 14.45;
        result.sky.environment_intensity = 0.75;
        result.look.taa = true;
        result.look.contrast = 1.12;
        result.look.saturation = 0.92;
        result.look.contact_shadows = false;
        result.look.ssao = true;
        if name != "clear-contrast" {
            result.look.fog_density = 0.018;
            result.look.bloom = 0.025;
        }
        if name == "cinematic-woodland" {
            result.sky.ev100 = 14.65;
            result.sky.environment_intensity = 0.6;
            result.look.contrast = 1.18;
            result.look.saturation = 0.85;
            result.look.fog_density = 0.028;
            result.look.bloom = 0.04;
            result.look.dof = true;
            result.look.motion_blur = 0.25;
        }
        Ok(result)
    }

    /// Reject invalid shader inputs instead of silently exporting a different look.
    #[allow(
        clippy::too_many_lines,
        reason = "one table of every tunable's range reads better than split ones"
    )]
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.sky;
        let l = &self.look;
        for (name, value, min, max) in [
            ("exposure", s.ev100, 8.0, 20.0),
            ("night exposure", s.night_ev100, 0.0, 14.0),
            ("moon", s.moon_lux, 0.0, 200.0),
            ("sun", s.sun_intensity, 0.0, 4.0),
            ("sky fill", s.environment_intensity, 0.0, 4.0),
            ("haze", s.haze, 0.01, 3.0),
            ("contrast", l.contrast, 0.5, 2.0),
            ("saturation", l.saturation, 0.0, 2.0),
            ("bloom", l.bloom, 0.0, 0.3),
            ("bloom threshold", l.bloom_threshold, 0.0, 10.0),
            ("light rays", l.light_rays_strength, 0.0, 2.0),
            ("light ray decay", l.light_rays_decay, 0.8, 1.0),
            ("light ray red", l.light_rays_tint[0], 0.0, 2.0),
            ("light ray green", l.light_rays_tint[1], 0.0, 2.0),
            ("light ray blue", l.light_rays_tint[2], 0.0, 2.0),
            ("sun disc", s.sun_disk_intensity, 0.0, 10.0),
            (
                "soft shadow size",
                s.soft_shadow_size.unwrap_or(0.0),
                0.0,
                16.0,
            ),
            ("fog density", l.fog_density, 0.0, 0.1),
            ("fog light", l.fog_light_intensity, 0.0, 16.0),
            ("fog scattering", l.fog_scattering, 0.0, 1.0),
            ("fog asymmetry", l.fog_asymmetry, 0.0, 0.95),
            ("focus distance", l.focus_distance, 0.1, 10_000.0),
            ("aperture", l.aperture, 0.5, 32.0),
            ("blur diameter", l.blur_diameter, 0.0, 32.0),
            ("motion blur", l.motion_blur, 0.0, 1.0),
            ("shadow distance", s.shadow_distance, 1.0, 100_000.0),
            // Above the cascades' 0.5 m near bound, which Bevy's builder
            // asserts the first cascade ends past; the F6 panel now rebuilds
            // the cascades live, so a slider at the floor would panic.
            (
                "first cascade",
                s.first_cascade_far_bound,
                1.0,
                s.shadow_distance,
            ),
            // The sky's environment map (lighting plan phase 6).
            (
                "environment map refresh",
                s.environment_map_refresh_degrees,
                0.0,
                30.0,
            ),
            ("aerial distance", s.aerial_view_distance, 1.0, 100_000.0),
            // The valley fog's light (lighting plan phase 2a).
            ("valley fog shadow", l.valley_fog_shadow, 0.0, 1.0),
            ("valley air floor", l.valley_floor_density, 0.0, 0.1),
            (
                "valley air floor height",
                l.valley_floor_height,
                -500.0,
                5_000.0,
            ),
            (
                "valley air floor falloff",
                l.valley_floor_falloff,
                10.0,
                5_000.0,
            ),
            // Irradiance probes (lighting plan phase 3b).
            ("irradiance", l.irradiance, 0.0, 4.0),
            // Sky visibility (lighting plan phase 3a).
            ("sky visibility", l.sky_visibility, 0.0, 1.0),
            // Colour scripting (lighting plan phase 4a).
            ("colour grade", l.grade, 0.0, 1.0),
            // The cloud layer (lighting plan phase 2b, `clouds`).
            ("cloud cover", s.cloud_cover, 0.0, 1.0),
            ("cloud scale", s.cloud_scale, 500.0, 50_000.0),
            ("cloud speed", s.cloud_speed, 0.0, 60.0),
            ("cloud shear", s.cloud_shear, 0.0, 4.0),
            ("upper cloud cover", s.cloud_upper_cover, 0.0, 2.0),
            ("cloud thickness", s.cloud_thickness, 0.0, 2_000.0),
            ("cloud evolution", s.cloud_evolution, 0.0, 0.5),
            // Exposure and the lens (lighting plan phase 4b).
            (
                "auto exposure brightest",
                l.auto_exposure_range_ev[0],
                -4.0,
                0.0,
            ),
            (
                "auto exposure darkest",
                l.auto_exposure_range_ev[1],
                0.0,
                4.0,
            ),
            ("auto exposure key", l.auto_exposure_key, -8.0, 2.0),
            (
                "auto exposure brighten speed",
                l.auto_exposure_speed_brighten,
                0.1,
                20.0,
            ),
            (
                "auto exposure darken speed",
                l.auto_exposure_speed_darken,
                0.1,
                20.0,
            ),
            ("vignette", l.vignette, 0.0, 1.0),
            ("vignette radius", l.vignette_radius, 0.1, 2.0),
            ("vignette smoothness", l.vignette_smoothness, 0.1, 10.0),
            ("aberration", l.aberration, 0.0, 1.0),
            // Auto exposure by night (lighting plan phase 4, after review).
            ("auto exposure by night", l.auto_exposure_night, 0.0, 1.0),
            ("render scale", l.render_scale, 0.5, 1.0),
            // The water (lighting plan phase 5a).
            ("water reflection", l.water_reflection, 0.0, 1.0),
            ("water foam", l.water_foam, 0.0, 1.0),
            // Grass against the low sun (lighting plan phase 5b).
            ("grass rim", l.grass_rim, 0.0, 2.0),
            // The ground painted (ticket 0006, pass 1).
            ("ground colour", l.ground_colour, 0.0, 2.0),
            ("ground warmth", l.ground_warmth, 0.0, 2.0),
            ("ground near tint", l.ground_near_tint, 0.0, 1.0),
            ("ground near value", l.ground_near_value, 0.0, 1.0),
            ("ground air", l.ground_air, 0.0, 2.0),
            ("ground grain", l.ground_grain, 0.0, 1.5),
            ("ground normal", l.ground_normal, 0.0, 1.5),
            ("ground matte", l.ground_matte, 0.0, 1.0),
            ("ground seam", l.ground_seam, 0.0, 1.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name} must be finite and in {min}..={max}"));
            }
        }
        if !(8..=128).contains(&l.fog_steps) {
            return Err("fog steps must be in 8..=128".into());
        }
        if !(8..=48).contains(&l.light_rays_steps) {
            return Err("light ray steps must be in 8..=48".into());
        }
        // Local fog volumes (lighting plan phase 2c).
        if !(4..=64).contains(&l.local_fog_steps) {
            return Err("local fog steps must be in 4..=64".into());
        }
        if !s.shadow_map_size.is_power_of_two() || !(256..=8192).contains(&s.shadow_map_size) {
            return Err("shadow map size must be a power of two in 256..=8192".into());
        }
        // The sun's cascades (lighting plan phase 6): Bevy draws at most
        // four a light.
        if !(1..=4).contains(&s.shadow_cascades) {
            return Err("shadow cascades must be in 1..=4".into());
        }
        if !(1..=4).contains(&s.building_shadow_cascades) {
            return Err("building shadow cascades must be in 1..=4".into());
        }
        if !s.environment_map_size.is_power_of_two()
            || !(32..=1024).contains(&s.environment_map_size)
        {
            return Err("environment map size must be a power of two in 32..=1024".into());
        }
        if s.fog_visibility.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err("fog visibility must be positive and finite".into());
        }
        if !PRESETS.contains(&l.preset.as_str()) {
            return Err("unknown source preset".into());
        }
        Ok(())
    }

    /// Validates a RON export before it reaches the renderer.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let result: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        result.validate()?;
        Ok(result)
    }

    /// Human-editable export with all effective sky and effect values.
    pub fn to_ron(&self) -> Result<String, String> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| e.to_string())
    }
}

/// An explicit camera AA choice takes precedence over preset selection.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaOverride {
    /// Multisampling, with SSAO suppressed.
    Msaa4,
    /// No antialiasing, useful for isolating effects.
    Off,
    /// Temporal antialiasing.
    Taa,
}

/// Resolves the same compatibility rule for the renderer and the tuning UI.
pub fn effective_aa(look: &RenderLook, choice: Option<AaOverride>) -> (Msaa, bool, bool) {
    let choice = choice.unwrap_or(if look.no_aa {
        AaOverride::Off
    } else if look.taa {
        AaOverride::Taa
    } else {
        AaOverride::Msaa4
    });
    let msaa = if choice == AaOverride::Msaa4 {
        Msaa::Sample4
    } else {
        Msaa::Off
    };
    (
        msaa,
        choice == AaOverride::Taa,
        look.ssao && msaa == Msaa::Off,
    )
}

#[derive(Component)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "records independent pre-existing camera components"
)]
pub(crate) struct AppliedLook {
    file: LookFile,
    aa: Option<AaOverride>,
    keep_normal: bool,
    keep_motion: bool,
    keep_jitter: bool,
    keep_mip_bias: bool,
    study: bool,
}

/// Applies only when effective settings change, including camera creation.
#[allow(
    clippy::too_many_arguments,
    reason = "one writer coordinates camera passes and their shared prepasses"
)]
pub(crate) fn apply_cameras(
    mut commands: Commands,
    look: Res<RenderLook>,
    sky: Res<SkySettings>,
    study: Res<crate::study::StudySettings>,
    cameras: Query<
        (
            Entity,
            Option<&AppliedLook>,
            Option<&AaOverride>,
            Has<NormalPrepass>,
            Has<MotionVectorPrepass>,
            Has<TemporalJitter>,
            Has<MipBias>,
            Option<&AtmosphereEnvironmentMapLight>,
        ),
        (With<Camera3d>, With<SkyCamera>, Without<NoSky>),
    >,
) {
    let file = LookFile {
        sky: *sky,
        look: look.clone(),
    };
    for (entity, old, aa, normal, motion, jitter, mip_bias, environment) in &cameras {
        let aa = aa.copied();
        let in_study = study.scene.is_some();
        if old.is_some_and(|v| v.file == file && v.aa == aa && v.study == in_study) {
            continue;
        }
        let (msaa, taa, ssao) = effective_aa(&look, aa);
        let keep_normal = old.map_or(normal, |v| v.keep_normal);
        let keep_motion = old.map_or(motion, |v| v.keep_motion);
        let keep_jitter = old.map_or(jitter, |v| v.keep_jitter);
        let keep_mip_bias = old.map_or(mip_bias, |v| v.keep_mip_bias);
        let mut c = commands.entity(entity);
        let mut grading = ColorGrading::default();
        grading.midtones.contrast = look.contrast;
        grading.shadows.contrast = look.contrast;
        grading.global.post_saturation = look.saturation;
        c.insert((
            Exposure { ev100: sky.ev100 },
            grading,
            msaa,
            if look.agx {
                Tonemapping::AgX
            } else {
                Tonemapping::AcesFitted
            },
            AtmosphereEnvironmentMapLight {
                intensity: sky.environment_intensity,
                // Bevy makes the cubemap once per camera but sizes the sky's
                // draw into it from this component every frame, so a size
                // changed live would leave most of each face frozen: the
                // camera keeps the size it was dressed with until a restart.
                size: environment
                    .map_or_else(|| crate::environment_map_light(&sky).size, |e| e.size),
                ..crate::environment_map_light(&sky)
            },
            AtmosphereSettings {
                aerial_view_lut_max_distance: sky.aerial_view_distance,
                ..Default::default()
            },
        ));
        if taa {
            c.insert(TemporalAntiAliasing { reset: true });
        } else {
            c.remove::<TemporalAntiAliasing>();
            if !keep_jitter {
                c.remove::<TemporalJitter>();
            }
            if !keep_mip_bias {
                c.remove::<MipBias>();
            }
        }
        c.insert(look.shadow_filter.on_camera(taa));
        if ssao {
            c.insert(ScreenSpaceAmbientOcclusion {
                quality_level: ScreenSpaceAmbientOcclusionQualityLevel::Medium,
                ..Default::default()
            });
        } else {
            c.remove::<ScreenSpaceAmbientOcclusion>();
            if !keep_normal {
                c.remove::<NormalPrepass>();
            }
        }
        apply_post_effects(&mut c, &look);
        if !taa && look.motion_blur == 0.0 && !keep_motion {
            c.remove::<MotionVectorPrepass>();
        }
        if let Some(fog) = volumetric_fog(&look, in_study, taa) {
            c.insert(fog);
        } else {
            c.remove::<VolumetricFog>();
        }
        if let Some(visibility) = sky.fog_visibility {
            c.insert(DistanceFog {
                color: Color::srgb(0.7, 0.78, 0.9),
                directional_light_color: Color::srgb(1.0, 0.95, 0.85),
                directional_light_exponent: 30.0,
                falloff: FogFalloff::from_visibility(visibility),
            });
        } else {
            c.remove::<DistanceFog>();
        }
        c.insert(AppliedLook {
            file: file.clone(),
            aa,
            keep_normal,
            keep_motion,
            keep_jitter,
            keep_mip_bias,
            study: in_study,
        });
    }
}

/// The camera's volumetric fog: the study's mist while the look gives it a
/// density, else the local fog volumes' (lighting plan phase 2c) while the
/// look draws them.
fn volumetric_fog(look: &RenderLook, in_study: bool, taa: bool) -> Option<VolumetricFog> {
    if in_study && look.fog_density > 0.0 {
        return Some(VolumetricFog {
            step_count: look.fog_steps,
            ambient_color: Color::srgb(0.55, 0.65, 0.8),
            ambient_intensity: 0.0,
            jitter: if taa { 0.2 } else { 0.0 },
        });
    }
    // No ambient: Bevy 0.19.1 adds it unexposed and unweighted by density,
    // a flat veil over the whole box that the density texture cannot soften.
    look.local_fog.then_some(VolumetricFog {
        step_count: look.local_fog_steps,
        ambient_color: Color::BLACK,
        ambient_intensity: 0.0,
        jitter: if taa { 0.5 } else { 0.0 },
    })
}

/// Keeps the sky fill the look asks for on the environment map Bevy draws.
///
/// Bevy 0.19.1's `prepare_atmosphere_probe_components` (Update) copies
/// `AtmosphereEnvironmentMapLight::intensity` into the
/// `GeneratedEnvironmentMapLight` it adds once, the first frame it sees the
/// camera, and `generate_environment_map_light` copies that into the
/// `EnvironmentMapLight` once more; neither looks again. `dress_cameras`
/// gives a new camera the default intensity (1) and `apply_cameras` the
/// look's sky fill ([`SkySettings::environment_intensity`], 4) a sync point
/// later, so which one was copied depended on how the executor ordered the
/// ambiguous systems that frame. Adding or removing unrelated systems (the
/// ground fog's) flipped it, and a sky fill changed in the F6 panel never
/// reached the frame. This copies the look's value down both steps
/// whenever they differ, in `PostUpdate`, after both of Bevy's copies.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
pub fn sync_sky_fill(
    mut cameras: Query<(
        &AtmosphereEnvironmentMapLight,
        Option<&mut GeneratedEnvironmentMapLight>,
        Option<&mut EnvironmentMapLight>,
    )>,
) {
    for (wanted, generated, drawn) in &mut cameras {
        if let Some(mut generated) = generated
            && generated.intensity != wanted.intensity
        {
            generated.intensity = wanted.intensity;
        }
        if let Some(mut drawn) = drawn
            && drawn.intensity != wanted.intensity
        {
            drawn.intensity = wanted.intensity;
        }
    }
}

// Irradiance probes (lighting plan phase 3b).
/// Gives every irradiance volume (the terrain's per-region probes and
/// their sky fallback, `strata_terrain::probes`) the look's strength
/// times the sky fill, and hides them all at zero: with no volume in a
/// view Bevy's shader goes back to the environment map's diffuse light.
/// Written only when it differs, so an unchanged look touches nothing.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
pub(crate) fn apply_irradiance(
    look: Res<RenderLook>,
    sky: Res<SkySettings>,
    mut volumes: Query<(&mut IrradianceVolume, &mut Visibility)>,
) {
    let intensity = look.irradiance * sky.environment_intensity;
    let shown = if look.irradiance > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut volume, mut visibility) in &mut volumes {
        if volume.intensity != intensity {
            volume.intensity = intensity;
        }
        visibility.set_if_neq(shown);
    }
}

fn apply_post_effects(c: &mut EntityCommands, look: &RenderLook) {
    if look.contact_shadows {
        c.insert(ContactShadows {
            thickness: 0.015,
            length: 0.1,
            ..Default::default()
        });
    } else {
        c.remove::<ContactShadows>();
    }
    if look.bloom > 0.0 {
        c.insert(bloom(look));
    } else {
        c.remove::<Bloom>();
    }
    if look.dof {
        c.insert(DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: look.focus_distance,
            aperture_f_stops: look.aperture,
            max_circle_of_confusion_diameter: look.blur_diameter,
            ..Default::default()
        });
    } else {
        c.remove::<DepthOfField>();
    }
    if look.motion_blur > 0.0 {
        c.insert(MotionBlur {
            shutter_angle: look.motion_blur,
            samples: 1,
        });
    } else {
        c.remove::<MotionBlur>();
    }
}

/// The bloom a look asks for.
///
/// With a threshold, additive: Bevy's energy-conserving composite blends the
/// frame towards its blur by the intensity, so with a threshold the
/// darker part of the frame, whose blur is black, would lose that share of
/// its light. Without one, the physical bloom the study measured.
fn bloom(look: &RenderLook) -> Bloom {
    if look.bloom_threshold > 0.0 {
        Bloom {
            intensity: look.bloom,
            // At 0.03 alone the glow round the sun or an emissive strip is
            // a pixel or two; the boost widens it into a halo. Only what
            // passed the threshold is in the blur, so it hazes nothing else.
            low_frequency_boost: 0.5,
            prefilter: BloomPrefilter {
                threshold: look.bloom_threshold,
                threshold_softness: 0.5,
            },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        }
    } else {
        Bloom {
            intensity: look.bloom,
            low_frequency_boost: 0.0,
            ..Bloom::NATURAL
        }
    }
}

/// Every two seconds, the GPU time of the passes that sample the sun's
/// shadow maps, when Bevy's render diagnostics are on (the client's
/// statistics turn them on). The shadow filter and PCSS cost nothing of
/// their own: they are a few more taps per shaded fragment inside these
/// passes, and the whole frame's GPU time drifts by more than that between
/// runs, so the trial of `shadow_filter` and `soft_shadow_size` is measured
/// here.
pub(crate) fn log_shadow_gpu_time(
    diagnostics: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
    look: Res<RenderLook>,
    sky: Res<SkySettings>,
    time: Res<bevy::time::Time<bevy::time::Real>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let ms = |span: &str| {
        let suffix = format!("/{span}/elapsed_gpu");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&suffix))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    };
    let (Some(opaque), transparent) = (ms("main_opaque_pass_3d"), ms("main_transparent_pass_3d"))
    else {
        return;
    };
    tracing::info!(
        "shadowed passes ({} filter, soft shadow {:?}), ms of GPU time: opaque {opaque:.3}, transparent {:.3}",
        look.shadow_filter.as_str(),
        sky.soft_shadow_size,
        transparent.unwrap_or(0.0)
    );
}

/// Rebuild the medium only when haze changes, never while moving the sun.
pub(crate) fn apply_medium(
    mut commands: Commands,
    sky: Res<SkySettings>,
    look: Res<RenderLook>,
    mut last_haze: Local<Option<f32>>,
    mediums: Option<ResMut<Assets<ScatteringMedium>>>,
    planets: Query<&Atmosphere>,
    suns: Query<Entity, With<Sun>>,
) {
    for entity in &suns {
        if look.is_changed() {
            // The local fog volumes (lighting plan phase 2c) scatter the sun
            // too; `local_fog` keeps the moon out of its way.
            if look.fog_density > 0.0 || look.local_fog {
                commands.entity(entity).insert(VolumetricLight);
            } else {
                commands.entity(entity).remove::<VolumetricLight>();
            }
        }
    }
    if *last_haze == Some(sky.haze) {
        return;
    }
    let Some(mut mediums) = mediums else {
        return;
    };
    for atmosphere in &planets {
        if let Some(mut medium) = mediums.get_mut(&atmosphere.medium) {
            *medium = ScatteringMedium::earth(256, 256).with_density_multiplier(sky.haze);
        }
    }
    *last_haze = Some(sky.haze);
}

/// Camera cuts and rebases invalidate the previous screen-space history.
pub(crate) fn reset_camera_cuts(
    mut cameras: Query<(
        Entity,
        &bevy::transform::components::GlobalTransform,
        &mut TemporalAntiAliasing,
    )>,
    mut previous: Local<std::collections::HashMap<Entity, (bevy::math::Vec3, bevy::math::Quat)>>,
) {
    previous.retain(|entity, _| cameras.contains(*entity));
    for (entity, transform, mut taa) in &mut cameras {
        let (_, rotation, position) = transform.to_scale_rotation_translation();
        if previous.get(&entity).is_some_and(|(p, r)| {
            position.distance_squared(*p) > 2500.0 || rotation.dot(*r).abs() < 0.866
        }) {
            taa.reset = true;
        }
        previous.insert(entity, (position, rotation));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::app::{App, Update};
    use bevy::core_pipeline::prepass::DepthPrepass;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<RenderLook>()
            .init_resource::<SkySettings>()
            .init_resource::<crate::study::StudySettings>()
            .add_systems(Update, apply_cameras);
        app
    }
    fn select(app: &mut App, name: &str) {
        let file = LookFile::preset(name).expect("known preset");
        app.insert_resource(file.sky).insert_resource(file.look);
        app.update();
    }

    #[test]
    fn large_camera_jumps_reset_temporal_history() {
        use bevy::transform::components::GlobalTransform;
        let mut app = App::new();
        app.add_systems(Update, reset_camera_cuts);
        let camera = app
            .world_mut()
            .spawn((
                GlobalTransform::default(),
                TemporalAntiAliasing { reset: false },
            ))
            .id();
        app.update();
        app.world_mut()
            .entity_mut(camera)
            .insert(GlobalTransform::from_translation(
                bevy::math::Vec3::X * 5000.0,
            ));
        app.update();
        assert!(
            app.world()
                .get::<TemporalAntiAliasing>(camera)
                .expect("TAA")
                .reset
        );
        app.world_mut()
            .get_mut::<TemporalAntiAliasing>(camera)
            .expect("TAA")
            .reset = false;
        app.world_mut()
            .entity_mut(camera)
            .insert(GlobalTransform::from(
                bevy::transform::components::Transform::from_translation(
                    bevy::math::Vec3::X * 5000.0,
                )
                .with_rotation(bevy::math::Quat::from_rotation_y(core::f32::consts::PI)),
            ));
        app.update();
        assert!(
            app.world()
                .get::<TemporalAntiAliasing>(camera)
                .expect("TAA")
                .reset
        );
    }

    #[test]
    fn the_looks_sky_fill_reaches_the_environment_map_bevy_draws() {
        let mut app = App::new();
        app.add_systems(Update, sync_sky_fill);
        // What Bevy's two one-time copies leave when they caught the
        // dressing's default before the look's sky fill.
        let camera = app
            .world_mut()
            .spawn((
                AtmosphereEnvironmentMapLight {
                    intensity: 4.0,
                    ..Default::default()
                },
                GeneratedEnvironmentMapLight {
                    intensity: 1.0,
                    ..Default::default()
                },
                EnvironmentMapLight {
                    intensity: 1.0,
                    ..Default::default()
                },
            ))
            .id();
        app.update();
        let intensities = |app: &App| {
            (
                app.world()
                    .get::<GeneratedEnvironmentMapLight>(camera)
                    .expect("generated")
                    .intensity,
                app.world()
                    .get::<EnvironmentMapLight>(camera)
                    .expect("drawn")
                    .intensity,
            )
        };
        assert_eq!(intensities(&app), (4.0, 4.0));
        // And a sky fill changed later (the F6 panel) follows.
        app.world_mut()
            .get_mut::<AtmosphereEnvironmentMapLight>(camera)
            .expect("wanted")
            .intensity = 2.5;
        app.update();
        assert_eq!(intensities(&app), (2.5, 2.5));
    }

    #[test]
    fn the_default_look_is_baseline_and_every_look_draws_the_ground_fog() {
        // The F6 panel compares against the default, Reset reloads the
        // preset by name, and a missing export falls back to the default:
        // all three are "baseline", so they must be one look.
        assert_eq!(
            LookFile::default(),
            LookFile::preset("baseline").expect("preset")
        );
        for preset in PRESETS {
            assert!(
                LookFile::preset(preset).expect("preset").look.ground_fog,
                "{preset}"
            );
        }
        // A look saved before the switch existed draws it too.
        let text = LookFile::default()
            .to_ron()
            .expect("export")
            .replace("ground_fog: true,", "");
        assert!(!text.contains("ground_fog"));
        assert!(LookFile::from_ron(&text).expect("import").look.ground_fog);
    }

    #[test]
    fn the_sun_s_tunables_are_range_checked() {
        let ok = LookFile::default();
        assert!(ok.validate().is_ok());
        let bad: [fn(&mut LookFile); 9] = [
            |f| f.look.light_rays_steps = 4,
            |f| f.look.light_rays_steps = 64,
            |f| f.look.light_rays_decay = 0.5,
            |f| f.look.light_rays_decay = 1.01,
            |f| f.look.light_rays_strength = -0.1,
            |f| f.look.light_rays_strength = f32::NAN,
            |f| f.look.light_rays_tint[1] = 3.0,
            |f| f.look.bloom_threshold = -1.0,
            |f| f.sky.sun_disk_intensity = -1.0,
        ];
        for (i, edit) in bad.iter().enumerate() {
            let mut file = ok.clone();
            edit(&mut file);
            assert!(file.validate().is_err(), "edit {i}");
        }
        let mut edges = ok.clone();
        edges.look.light_rays_steps = 8;
        edges.look.light_rays_decay = 1.0;
        edges.look.light_rays_strength = 0.0;
        edges.look.bloom_threshold = 0.0;
        edges.sky.sun_disk_intensity = 0.0;
        assert!(edges.validate().is_ok());
        edges.look.light_rays_steps = 48;
        assert!(edges.validate().is_ok());
    }

    // The sun's cascades and the sky's environment map (lighting plan
    // phase 6).
    #[test]
    fn the_cascades_and_the_environment_map_are_range_checked() {
        let ok = LookFile::default();
        let bad: [fn(&mut LookFile); 9] = [
            |f| f.sky.shadow_cascades = 0,
            |f| f.sky.shadow_cascades = 5,
            |f| f.sky.building_shadow_cascades = 0,
            |f| f.sky.environment_map_size = 300,
            |f| f.sky.environment_map_size = 16,
            |f| f.sky.environment_map_size = 2048,
            |f| f.sky.environment_map_refresh_degrees = -1.0,
            |f| f.sky.environment_map_refresh_degrees = f32::NAN,
            |f| f.sky.first_cascade_far_bound = 0.5,
        ];
        for (i, edit) in bad.iter().enumerate() {
            let mut file = ok.clone();
            edit(&mut file);
            assert!(file.validate().is_err(), "edit {i}");
        }
        // Every valid count builds without tripping Bevy's assertions; by
        // default buildings cast into every cascade, and asked for two they
        // cast to the second's bound.
        for count in 1..=4 {
            let mut file = ok.clone();
            file.sky.shadow_cascades = count;
            assert!(file.validate().is_ok());
            assert_eq!(file.sky.cascades().bounds.len(), count as usize);
        }
        assert_eq!(ok.sky.building_shadow_reach(&ok.sky.cascades()), None);
        let mut two = ok.clone();
        two.sky.building_shadow_cascades = 2;
        let bounds = two.sky.cascades().bounds;
        assert_eq!(
            two.sky.building_shadow_reach(&two.sky.cascades()),
            Some(bounds[1])
        );
        // A look saved before the fields loads with their defaults.
        let text = ok.to_ron().expect("export");
        let old: String = text
            .lines()
            .filter(|line| {
                !["shadow_cascades", "environment_map_"]
                    .iter()
                    .any(|field| line.contains(field))
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(LookFile::from_ron(&old).expect("import"), ok);
    }

    #[test]
    fn the_shadow_filter_is_a_named_string_and_temporal_needs_taa() {
        let file = LookFile::default();
        assert_eq!(file.look.shadow_filter, ShadowFilter::Temporal);
        let text = file.to_ron().expect("export");
        assert!(text.contains("shadow_filter: \"temporal\""), "{text}");
        for filter in ShadowFilter::ALL {
            let swapped = text.replace("\"temporal\"", &format!("{:?}", filter.as_str()));
            let back = LookFile::from_ron(&swapped).expect("import");
            assert_eq!(back.look.shadow_filter, filter);
        }
        assert!(LookFile::from_ron(&text.replace("\"temporal\"", "\"pcf\"")).is_err());
        // A look exported before the field existed takes the default.
        let old = text.replace("shadow_filter: \"temporal\",", "");
        assert!(!old.contains("shadow_filter"));
        assert_eq!(
            LookFile::from_ron(&old).expect("import").look.shadow_filter,
            ShadowFilter::Temporal
        );
        let mut app = app();
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        assert_eq!(
            app.world().get::<ShadowFilteringMethod>(camera),
            Some(&ShadowFilteringMethod::Temporal)
        );
        app.world_mut().entity_mut(camera).insert(AaOverride::Msaa4);
        app.update();
        assert_eq!(
            app.world().get::<ShadowFilteringMethod>(camera),
            Some(&ShadowFilteringMethod::Gaussian)
        );
        app.world_mut().resource_mut::<RenderLook>().shadow_filter = ShadowFilter::Hardware2x2;
        app.update();
        assert_eq!(
            app.world().get::<ShadowFilteringMethod>(camera),
            Some(&ShadowFilteringMethod::Hardware2x2)
        );
    }

    #[test]
    fn soft_shadows_are_off_by_default_and_range_checked() {
        let mut file = LookFile::default();
        assert_eq!(file.sky.soft_shadow_size, None);
        for (size, ok) in [
            (0.0, true),
            (0.25, true),
            (10.0, true),
            (16.0, true),
            (-0.1, false),
            (16.5, false),
            (f32::NAN, false),
        ] {
            file.sky.soft_shadow_size = Some(size);
            assert_eq!(file.validate().is_ok(), ok, "{size}");
        }
        file.sky.soft_shadow_size = Some(0.25);
        assert_eq!(
            LookFile::from_ron(&file.to_ron().expect("export")).expect("import"),
            file
        );
    }

    #[test]
    fn exports_round_trip_and_reject_invalid_shader_inputs() {
        for preset in PRESETS {
            let file = LookFile::preset(preset).expect("preset");
            assert_eq!(
                LookFile::from_ron(&file.to_ron().expect("export")).expect("import"),
                file
            );
        }
        let mut file = LookFile::default();
        file.look.fog_asymmetry = 1.0;
        assert!(file.validate().is_err());
        file.look.fog_asymmetry = f32::NAN;
        assert!(file.validate().is_err());
        assert!(LookFile::from_ron("(unknown: 1)").is_err());
        assert!(LookFile::preset("missing").is_err());
    }

    #[test]
    fn returning_to_baseline_removes_owned_effects_but_keeps_water_depth() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<crate::study::StudySettings>()
            .scene = Some("woodland".into());
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), SkyCamera, DepthPrepass))
            .id();
        select(&mut app, "cinematic-woodland");
        assert!(app.world().get::<Bloom>(camera).is_some());
        assert!(app.world().get::<DepthOfField>(camera).is_some());
        assert!(app.world().get::<MotionBlur>(camera).is_some());
        assert!(
            app.world()
                .get::<ScreenSpaceAmbientOcclusion>(camera)
                .is_some()
        );
        assert!(app.world().get::<VolumetricFog>(camera).is_some());
        select(&mut app, "baseline");
        let c = app.world().entity(camera);
        assert!(!c.contains::<DepthOfField>() && !c.contains::<MotionBlur>());
        // Baseline blooms since the lighting plan's phase 1, thresholded
        // and additive, so only what is over the threshold glows.
        let bloom = c.get::<Bloom>().expect("baseline blooms");
        assert!((bloom.intensity - 0.03).abs() < 1e-6);
        assert!(bloom.prefilter.threshold > 0.0);
        assert_eq!(bloom.composite_mode, BloomCompositeMode::Additive);
        // Baseline marches the local fog volumes since the lighting plan's
        // phase 2c, at their own step count, no longer the study's.
        let fog = c.get::<VolumetricFog>().expect("local fog");
        assert_eq!(fog.step_count, RenderLook::default().local_fog_steps);
        // The baseline grounds the scene with ambient occlusion, which needs
        // TAA and its prepasses in place of MSAA.
        assert!(
            c.contains::<ScreenSpaceAmbientOcclusion>() && c.contains::<TemporalAntiAliasing>()
        );
        assert!(c.contains::<NormalPrepass>() && c.contains::<DepthPrepass>());
        assert_eq!(c.get::<Msaa>(), Some(&Msaa::Off));
    }

    // Local fog volumes (lighting plan phase 2c).
    #[test]
    fn local_fog_is_on_by_default_switchable_and_its_steps_range_checked() {
        let file = LookFile::default();
        assert!(file.look.local_fog);
        let text = file.to_ron().expect("export");
        // A look exported before the fields existed draws the volumes.
        let old = text
            .replace("local_fog: true,", "")
            .replace("local_fog_steps: 6,", "");
        assert!(!old.contains("local_fog"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for (steps, ok) in [(3, false), (4, true), (64, true), (65, false)] {
            let mut file = file.clone();
            file.look.local_fog_steps = steps;
            assert_eq!(file.validate().is_ok(), ok, "{steps}");
        }
        let mut app = app();
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        let fog = app.world().get::<VolumetricFog>(camera).expect("local fog");
        assert_eq!(fog.step_count, 6);
        assert!(fog.ambient_intensity.abs() < 1e-9);
        app.world_mut().resource_mut::<RenderLook>().local_fog = false;
        app.update();
        assert!(app.world().get::<VolumetricFog>(camera).is_none());
    }

    // Irradiance probes (lighting plan phase 3b).
    #[test]
    fn the_probes_strength_is_range_checked_and_zero_hides_them() {
        let mut file = LookFile::default();
        assert!((file.look.irradiance - 1.0).abs() < f32::EPSILON);
        let text = file.to_ron().expect("export");
        let old = text.replace("irradiance: 1.0,", "");
        assert!(!old.contains("irradiance"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for bad in [-0.1, 4.5, f32::NAN] {
            file.look.irradiance = bad;
            assert!(file.validate().is_err(), "{bad}");
        }
        let mut app = App::new();
        app.init_resource::<RenderLook>()
            .init_resource::<SkySettings>()
            .add_systems(Update, apply_irradiance);
        let probe = app
            .world_mut()
            .spawn((IrradianceVolume::default(), Visibility::Inherited))
            .id();
        app.update();
        let volume = app.world().get::<IrradianceVolume>(probe).expect("volume");
        assert!((volume.intensity - SkySettings::default().environment_intensity).abs() < 1e-6);
        app.world_mut().resource_mut::<RenderLook>().irradiance = 0.0;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(probe),
            Some(&Visibility::Hidden)
        );
    }

    // Sky visibility (lighting plan phase 3a).
    #[test]
    fn the_sky_visibility_is_on_by_default_old_looks_load_it_and_it_is_range_checked() {
        let mut file = LookFile::default();
        assert!((file.look.sky_visibility - 1.0).abs() < f32::EPSILON);
        let text = file.to_ron().expect("export");
        let old = text.replace("sky_visibility: 1.0,", "");
        assert!(!old.contains("sky_visibility"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for preset in PRESETS {
            let look = LookFile::preset(preset).expect("preset");
            assert!(
                (look.look.sky_visibility - 1.0).abs() < f32::EPSILON,
                "{preset}"
            );
        }
        for bad in [-0.1, 1.5, f32::NAN] {
            file.look.sky_visibility = bad;
            assert!(file.validate().is_err(), "{bad}");
        }
    }

    // Exposure and the lens (lighting plan phase 4b).
    #[test]
    fn the_lens_fields_load_from_old_looks_name_their_mode_and_are_range_checked() {
        use crate::exposure::ExposureMode;
        let file = LookFile::default();
        let text = file.to_ron().expect("export");
        // The default is auto since the phase's trial; manual is a word in
        // a look file away.
        assert!(text.contains("exposure_mode: \"auto\""), "{text}");
        let manual = LookFile::from_ron(&text.replace("\"auto\"", "\"manual\"")).expect("import");
        assert_eq!(manual.look.exposure_mode, ExposureMode::Manual);
        assert!(LookFile::from_ron(&text.replace("\"auto\"", "\"eye\"")).is_err());
        // A look exported before the fields existed takes their defaults,
        // auto exposure included, and every preset shares them.
        let old = LookFile::from_ron("(look: (preset: \"baseline\"))").expect("import");
        assert_eq!(old, file);
        for preset in PRESETS {
            let look = LookFile::preset(preset).expect("preset").look;
            assert_eq!(look.exposure_mode, ExposureMode::Auto, "{preset}");
            assert!(
                look.auto_exposure_range_ev[1].abs() < f32::EPSILON,
                "{preset}"
            );
        }
        // Auto exposure by night (after review): none by default, and a
        // look saved before the field existed takes that too.
        assert!(file.look.auto_exposure_night.abs() < f32::EPSILON);
        let bad: [fn(&mut LookFile); 9] = [
            |f| f.look.auto_exposure_range_ev[0] = 0.5,
            |f| f.look.auto_exposure_range_ev[1] = -0.5,
            |f| f.look.auto_exposure_key = f32::NAN,
            |f| f.look.auto_exposure_speed_darken = 0.0,
            |f| f.look.vignette = 1.5,
            |f| f.look.vignette_radius = 0.0,
            |f| f.look.aberration = -0.1,
            |f| f.look.auto_exposure_night = -0.1,
            |f| f.look.auto_exposure_night = 1.5,
        ];
        for (i, edit) in bad.iter().enumerate() {
            let mut file = file.clone();
            edit(&mut file);
            assert!(file.validate().is_err(), "edit {i}");
        }
    }

    // Grass against the low sun (lighting plan phase 5b).
    #[test]
    fn the_grass_rim_is_on_by_default_old_looks_load_it_and_it_is_range_checked() {
        let mut file = LookFile::default();
        assert!((file.look.grass_rim - 1.0).abs() < f32::EPSILON);
        let text = file.to_ron().expect("export");
        let old = text.replace("grass_rim: 1.0,", "");
        assert!(!old.contains("grass_rim"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for preset in PRESETS {
            let look = LookFile::preset(preset).expect("preset");
            assert!((look.look.grass_rim - 1.0).abs() < f32::EPSILON, "{preset}");
        }
        for bad in [-0.1, 2.5, f32::NAN] {
            file.look.grass_rim = bad;
            assert!(file.validate().is_err(), "{bad}");
        }
    }

    // The water (lighting plan phase 5a).
    #[test]
    fn the_water_reflects_and_foams_by_default_old_looks_load_it_and_it_is_range_checked() {
        let mut file = LookFile::default();
        assert!((file.look.water_reflection - 1.0).abs() < f32::EPSILON);
        assert!((file.look.water_foam - 1.0).abs() < f32::EPSILON);
        let text = file.to_ron().expect("export");
        let old = text
            .replace("water_reflection: 1.0,", "")
            .replace("water_foam: 1.0,", "");
        assert!(!old.contains("water_"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for preset in PRESETS {
            let look = LookFile::preset(preset).expect("preset").look;
            assert!(
                (look.water_reflection - 1.0).abs() < f32::EPSILON
                    && (look.water_foam - 1.0).abs() < f32::EPSILON,
                "{preset}"
            );
        }
        for bad in [-0.1, 1.5, f32::NAN] {
            let mut reflection = file.clone();
            reflection.look.water_reflection = bad;
            assert!(reflection.validate().is_err(), "reflection {bad}");
            file.look.water_foam = bad;
            assert!(file.validate().is_err(), "foam {bad}");
        }
    }

    // Colour scripting (lighting plan phase 4a).
    #[test]
    fn the_colour_grade_is_full_by_default_old_looks_load_it_and_it_is_range_checked() {
        let mut file = LookFile::default();
        assert!((file.look.grade - 1.0).abs() < f32::EPSILON);
        let text = file.to_ron().expect("export");
        let old = text.replace("grade: 1.0,", "");
        assert!(!old.contains("grade:"));
        assert_eq!(LookFile::from_ron(&old).expect("import"), file);
        for preset in PRESETS {
            let look = LookFile::preset(preset).expect("preset");
            assert!((look.look.grade - 1.0).abs() < f32::EPSILON, "{preset}");
        }
        for bad in [-0.1, 1.5, f32::NAN] {
            file.look.grade = bad;
            assert!(file.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn explicit_msaa_suppresses_ssao_and_new_cameras_receive_the_current_look() {
        let mut app = app();
        select(&mut app, "sunlit-mist");
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                SkyCamera,
                AaOverride::Msaa4,
                NormalPrepass,
            ))
            .id();
        let excluded = app
            .world_mut()
            .spawn((Camera3d::default(), SkyCamera, NoSky))
            .id();
        app.update();
        assert!(app.world().get::<Bloom>(camera).is_some());
        assert!(
            app.world()
                .get::<ScreenSpaceAmbientOcclusion>(camera)
                .is_none()
        );
        assert!(app.world().get::<NormalPrepass>(camera).is_some());
        assert!(app.world().get::<AppliedLook>(excluded).is_none());
        app.world_mut().entity_mut(camera).insert(AaOverride::Taa);
        app.update();
        assert!(
            app.world()
                .get::<ScreenSpaceAmbientOcclusion>(camera)
                .is_some()
        );
        select(&mut app, "baseline");
        assert!(app.world().get::<NormalPrepass>(camera).is_some());
        assert!(app.world().get::<TemporalAntiAliasing>(camera).is_some());
    }

    // The sky map's size takes a restart (review of lighting plan phase 6).
    #[test]
    fn a_dressed_camera_keeps_its_sky_map_size_when_the_look_changes() {
        let mut app = app();
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        let size = |app: &App| {
            app.world()
                .get::<AtmosphereEnvironmentMapLight>(camera)
                .expect("dressed")
                .size
        };
        let first = size(&app);
        assert_eq!(first.x, SkySettings::default().environment_map_size);
        let mut sky = SkySettings {
            environment_map_size: first.x / 2,
            ..SkySettings::default()
        };
        sky.environment_intensity += 1.0;
        app.insert_resource(sky);
        app.update();
        assert_eq!(size(&app), first);
        // The rest of the look still reaches the camera.
        let intensity = app
            .world()
            .get::<AtmosphereEnvironmentMapLight>(camera)
            .expect("dressed")
            .intensity;
        assert!((intensity - sky.environment_intensity).abs() < 1e-6);
    }
}
