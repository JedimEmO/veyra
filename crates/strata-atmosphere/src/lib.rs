//! Sky, sun, fog and the day cycle.
//!
//! The sun's direction is a function of the sim's `TimeOfDay`, so the
//! server and every client agree on the time of day without sending it, and
//! a screenshot at tick N looks the same on every machine. This crate never
//! reads the clock itself: the sim owns "noon".
//!
//! What this plugin puts in the world:
//!
//! - One [`Planet`] entity carrying `Atmosphere::earth`. Bevy's atmosphere
//!   is physically based and gives the sky, aerial perspective to the
//!   horizon and the reddening of the sun's light at low elevation, all from
//!   the sun's direction. The medium's density is scaled by
//!   [`SkySettings::haze`]: Earth's own aerosols whiten a mountain ten
//!   kilometres away more than the slice wants, and a thinner medium keeps
//!   distant biomes readable. The planet's centre is a world position like
//!   every other rendered thing (ADR 0002), one planet radius below sea
//!   level, so a render-origin rebase, which moves the origin in `y` as well
//!   as `x` and `z`, cannot shift the modelled surface up to the camera. It
//!   follows the observer horizontally, because the world is flat and the
//!   planet is round: kept at the world origin, a camera fifty kilometres
//!   out would read as two hundred metres higher than it is.
//! - One directional light, the [`Sun`], with cascaded shadow maps and a
//!   `SunDisk` drawn in the sky, whose transform follows [`DayCycle`] and
//!   the time of day.
//! - On every `Camera3d`: `AtmosphereSettings`, an `Exposure` that keeps
//!   the sky from clipping, `Tonemapping::AcesFitted`, the render look's
//!   effects ([`look`]: a thresholded bloom by default, so the sun, the sky
//!   by it and emissives glow and the lit ground does not), the sun's
//!   screen-space [`light_rays`], and an
//!   `AtmosphereEnvironmentMapLight` so shadowed ground is lit by the sky
//!   rather than a flat ambient. `DistanceFog` is available behind
//!   [`SkySettings::fog_visibility`] but off by default: the atmosphere's
//!   aerial perspective already fades terrain into the sky, and fog on top
//!   of it reads as haze in the foreground. The look's sky fill is copied
//!   on to the environment map Bevy generates from it whenever the two
//!   differ ([`look::sync_sky_fill`]), because Bevy copies it once.
//! - Ground fog, when the game inserts [`fog_pass::GroundFog`] with a
//!   non-zero scale (ticket 0003): [`fog_clipmap`] keeps `strata-weather`'s
//!   fog field around the observer in two textures, baked and sampled on the
//!   async compute pool, and [`fog_pass`] marches it in a full-screen pass
//!   after the transparent pass, on every single-sampled sky camera whose
//!   render look allows it.
//!
//! Cameras are dressed by presence of `Camera3d`, not by a marker, because
//! the slice has one camera and a marker would be one more thing the client
//! crate has to remember. A camera that must not get the sky (a UI camera,
//! a portrait render) adds [`NoSky`].

// The 2D cloud layer and its shadows (lighting plan phase 2b).
/// Whether the sun's soft shadows were compiled in (the `pcss` feature), so a
/// panel does not offer a control the light ignores.
pub const SOFT_SHADOWS: bool = cfg!(feature = "pcss");

pub mod clouds;
pub mod diagnostics;
// Exposure and the lens (lighting plan phase 4b).
pub mod exposure;
pub mod fog_clipmap;
// Colour scripting (lighting plan phase 4a).
pub mod fog_pass;
pub mod grade;
pub mod light_rays;
// Local fog volumes (lighting plan phase 2c).
mod local_fog;
pub mod look;
mod prepass;
pub mod render_scale;
pub mod study;

use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

use bevy::app::{App, Plugin, PostUpdate, Startup, Update};
use bevy::asset::Assets;
use bevy::camera::{Camera3d, Exposure};
use bevy::color::Color;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::prelude::*;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfig, CascadeShadowConfigBuilder,
    DirectionalLight, DirectionalLightShadowMap, GlobalAmbientLight, SunDisk, light_consts,
};
use bevy::math::{Quat, UVec2, Vec3};
use bevy::pbr::{AtmosphereSettings, DistanceFog, FogFalloff};
use bevy::state::condition::in_state;
use bevy::transform::components::Transform;
use strata_app::{AppState, Observers, TimeOfDay, WorldPosition, WorldSourceRes};
use strata_core::WorldPos;
use tracing::warn;

/// The day cycle: how a time of day maps to a sun position.
///
/// A resource rather than a constant so the game can set a planet's
/// latitude from data. The length of the day and where it starts are sim
/// state (`strata_app::DayLength`, `strata_app::DayStart`).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct DayCycle {
    /// Tilt of the sun's path from the zenith, in radians. At noon the sun
    /// is this far from straight up, towards -z; at 0 it passes overhead
    /// and shadows vanish, so the default is mid-latitude.
    pub tilt: f32,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            tilt: 35f32.to_radians(),
        }
    }
}

impl DayCycle {
    /// Unit vector from the ground towards the sun at `fraction` of the day.
    ///
    /// The sun rises at +x, passes noon towards -z at `tilt` from the
    /// zenith, and sets at -x. Below the horizon `y` is negative.
    pub fn sun_direction(&self, fraction: f64) -> Vec3 {
        // The fraction is in [0, 1) so f32 loses nothing that matters.
        #[allow(clippy::cast_possible_truncation)]
        let hour_angle = (fraction as f32 - 0.5) * TAU;
        let (sin_h, cos_h) = hour_angle.sin_cos();
        let (sin_t, cos_t) = self.tilt.sin_cos();
        Vec3::new(-sin_h, cos_h * cos_t, -cos_h * sin_t).normalize()
    }
}

/// Camera and sun settings. Insert before [`AtmospherePlugin`] to override;
/// camera exposure, sky fill and haze also update live. Shadow cascade
/// geometry is configured at startup.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkySettings {
    /// EV100 for every dressed camera. Bevy's atmosphere example uses 13
    /// for a raw-sunlight sun; the slice's ground is brighter than that
    /// scene (sand and snow), so it sits a little higher, and a little
    /// higher again for the stronger sky fill below.
    pub ev100: f32,
    /// Artistic multiplier on sunlight, also respected by the day cycle.
    pub sun_intensity: f32,
    /// Strength of diffuse and reflected sky lighting. At 1 the shaded side
    /// of a street, a slab or a tree went nearly black under the fitted ACES
    /// curve; 4 (chosen on 2026-09-27 over `AgX` variants, which opened the
    /// shade further but paled the city's paint) lifts it while keeping the
    /// colour and contrast.
    pub environment_intensity: f32,
    /// Add a `DistanceFog` with this visibility in metres. `None` relies on
    /// aerial perspective alone.
    pub fog_visibility: Option<f32>,
    /// Whether the sun casts shadows at all. Off is a debugging aid: the
    /// terrain's self-shadowing is the first suspect for any speckle.
    pub shadows: bool,
    /// How far, in metres, the sun's shadow cascades reach. Beyond it the
    /// ground is lit as if unshadowed, which at that distance is what
    /// aerial perspective shows anyway, and it keeps the last cascade's
    /// texels from flickering across far slopes. Past it the baked sky
    /// visibility and the irradiance probes still darken what a ridge
    /// hides from the sky. It was 20 km, which put every building and
    /// terrain region from 660 m out into the two far cascades: 26 649
    /// caster draws a frame at the city and the largest GPU item there
    /// (`docs/perf/profile-2026-10-03.md`, 4.3).
    pub shadow_distance: f32,
    /// Where the first cascade ends, metres. The near cascade is what the
    /// player stands in, so it is kept small and sharp.
    pub first_cascade_far_bound: f32,
    /// Shadow map side in texels, a power of two. 4096 on a desktop GPU is
    /// four times the memory of Bevy's default and the difference between
    /// crisp and blocky shadows under the camera.
    pub shadow_map_size: usize,
    /// Density multiplier on the Earth scattering medium. 1 is Earth; below
    /// it the air is clearer and distant ground keeps more of its colour.
    pub haze: f32,
    /// How far the aerial perspective is evaluated, metres. Geometry beyond
    /// it takes the last slice's value.
    pub aerial_view_distance: f32,
    /// EV100 once the sun is well down: the eye adapted to the night, so a
    /// moonlit street is dim but readable and a lit window glows.
    pub night_ev100: f32,
    /// The moon's illuminance at the ground, lux. Several times a real
    /// full moon's, as games light their nights, so forms read.
    pub moon_lux: f32,
    /// Brightness of the sun's disc in the sky, a multiplier on
    /// `SunDisk::EARTH`'s (Bevy's physical value). The disc is what bloom
    /// and the light rays grow from; zero hides it, and since Bevy draws
    /// `EARTH` for a sun with no `SunDisk` at all, hiding it is the
    /// component at zero intensity, not its absence.
    pub sun_disk_intensity: f32,
    /// Percentage-closer soft shadows from the sun, which widen a shadow
    /// with its distance from what casts it, so a trunk's foot stays sharp
    /// and the far end of its shadow goes soft. Not a sun size in metres:
    /// Bevy's directional PCSS searches this many metres of the shadow map
    /// for blockers and blurs by `max(size * (blocker - receiver) / receiver,
    /// 0.5)` in the cascade's 0 to 1 depth, against 1 for the plain
    /// temporal filter, so below about 2 it is never softer than that
    /// filter and only from about 10 (Bevy's own example) does a long
    /// shadow visibly soften (measured 2026-10-02). `None` is Bevy's
    /// ordinary filtered shadow. Off by default: the trial found it free
    /// within noise but the softening too slight to choose.
    pub soft_shadow_size: Option<f32>,
    // The cloud layer (lighting plan phase 2b, `clouds`).
    /// The share of the sky under cloud, `0` to `1`: the cloud image's
    /// quantile its density has to pass. Zero hides the layer and takes the
    /// cloud shadow off the sun. With the look's `gate_floor` (`assets/look/clouds.ron`) the
    /// gaps between groups never cloud over, so the sky stops filling near
    /// 0.58 and an overcast day needs a raised floor, not this dial.
    pub cloud_cover: f32,
    /// The metres one repeat of the cloud image covers. At 6 km a heap of
    /// cumulus is about a kilometre across and the repeat is not seen from
    /// the ground, where the layer fades into the haze within a few tiles.
    pub cloud_scale: f32,
    /// How fast the clouds and their shadows drift downwind, metres a
    /// second of sim time, along the sim's wind direction. 22 is a brisk
    /// day at the layer's height: a cloud a kilometre across passes
    /// overhead in 45 seconds, so the sky is seen to move; at 6 it took
    /// three minutes and the layer read as still.
    pub cloud_speed: f32,
    /// The upper deck's speed as a multiple of `cloud_speed`, along a wind
    /// veered 20 degrees. Two decks sliding past each other are what makes
    /// the sky read as depth rather than a painted dome, and the eye takes
    /// the slower one for the farther: at 0.8 the deck at 2.6 km crosses
    /// the sky at 0.55 of the lower one's rate. At 1.5 the two cross it at
    /// about the same rate and the depth is lost.
    pub cloud_shear: f32,
    /// The upper layer's cover as a share of `cloud_cover`: a thinner,
    /// higher deck behind the cumulus, so it hides with the cover's zero.
    pub cloud_upper_cover: f32,
    /// How tall a cloud's heart stands over its edge, metres. The layer is
    /// flat; its density is shaded as a height field this tall, so sunward
    /// slopes catch the light, the lee falls into the body's shade and a
    /// base bulges towards the eye. Zero shades it flat.
    pub cloud_thickness: f32,
    /// How fast a cloud's frayed edge changes as it drifts, in repeats of
    /// the fraying noise a second: it slides through the body across the
    /// wind, so shapes boil rather than only translate. The shadow and the
    /// sun's veil leave the fraying out, so they stay with the cloud.
    pub cloud_evolution: f32,
    // The sun's cascades and the sky's environment map (lighting plan
    // phase 6, the profile's 4.3).
    /// How many cascades the sun's shadow is split into, 1 to 4 (Bevy's
    /// most). The bounds are geometric from `first_cascade_far_bound` to
    /// `shadow_distance`, so a cascade fewer stretches the middle ones: at
    /// three over 120 m to 4 km they end at 120, 693 and 4 000 m. Three,
    /// because the look fixtures drew the same with three as with four and
    /// a cascade is a shadow view whose batch sets each cost a bin unpacking
    /// bind group and dispatch a frame; the second cascade's texels are
    /// coarser (693 m against 387), which no fixture showed.
    pub shadow_cascades: u32,
    /// Buildings cast into this many of the sun's cascades and no further:
    /// a settlement piece past the last of them casts nothing, as the far
    /// trees' levels already do not (ADR 0005). At `shadow_cascades` or more
    /// every piece casts, which is the default: whether a building's shadow
    /// lands in the near cascades depends on the sun's height and the
    /// building's, not its distance (a 150 m tower under a 5 degree sun
    /// casts 1.7 km), and with [`cascade_trim`] on, which already drops a
    /// caster whose swept shadow misses a slice, the cut saved nothing at
    /// the city street under a high sun and 6 % of the casters from 120 m
    /// up at a low one. A lower count is a trial on the F6 panel.
    pub building_shadow_cascades: u32,
    /// The side, in texels, of each face of the cubemap Bevy draws the sky
    /// into for the environment light, a power of two. Bevy's default is
    /// 512; the cubemap is filtered into the diffuse and specular maps every
    /// frame it is drawn, and the sky is smooth, so most of those texels
    /// carry nothing a blurred read could tell apart. Read when a camera is
    /// dressed, so a change takes a restart.
    pub environment_map_size: u32,
    /// How far the sun must turn, degrees, before the sky's environment map
    /// is filtered again; zero filters it every frame, as Bevy does. The
    /// sky it is filtered from changes with the sun, the observer's height
    /// and the medium, and with none of them moving the filtered maps of a
    /// minute ago are the same ones.
    pub environment_map_refresh_degrees: f32,
    /// Whether each of the sun's cascades draws only the casters whose
    /// shadow can reach the slice of the view it shades
    /// ([`cascade_trim`]). Off is Bevy's own culling, which hands every
    /// cascade the ground under the camera.
    pub trim_shadow_casters: bool,
}

impl Default for SkySettings {
    fn default() -> Self {
        Self {
            ev100: 14.4,
            sun_intensity: 1.0,
            environment_intensity: 4.0,
            fog_visibility: None,
            shadows: true,
            shadow_distance: 4_000.0,
            first_cascade_far_bound: 120.0,
            shadow_map_size: 4096,
            haze: 0.55,
            aerial_view_distance: 48_000.0,
            night_ev100: 3.0,
            moon_lux: 11.0,
            sun_disk_intensity: 1.0,
            soft_shadow_size: None,
            // The cloud layer (lighting plan phase 2b).
            cloud_cover: 0.32,
            cloud_scale: 6_000.0,
            cloud_speed: 22.0,
            cloud_shear: 0.8,
            cloud_upper_cover: 0.6,
            cloud_thickness: 400.0,
            cloud_evolution: 0.02,
            // The sun's cascades and the sky's environment map (lighting
            // plan phase 6).
            shadow_cascades: 3,
            building_shadow_cascades: 4,
            environment_map_size: 256,
            environment_map_refresh_degrees: 1.0,
            trim_shadow_casters: true,
        }
    }
}

impl SkySettings {
    /// The sun's cascades as these settings ask for them.
    #[must_use]
    pub fn cascades(&self) -> CascadeShadowConfig {
        CascadeShadowConfigBuilder {
            num_cascades: self.shadow_cascades as usize,
            minimum_distance: 0.5,
            maximum_distance: self.shadow_distance,
            first_cascade_far_bound: self.first_cascade_far_bound,
            overlap_proportion: 0.15,
        }
        .build()
    }

    /// How far buildings cast, metres of view depth: the far bound of the
    /// last cascade they cast into, or `None` when they cast into every one.
    #[must_use]
    pub fn building_shadow_reach(&self, cascades: &CascadeShadowConfig) -> Option<f32> {
        let count = self.building_shadow_cascades as usize;
        if count >= cascades.bounds.len() {
            return None;
        }
        cascades.bounds.get(count.checked_sub(1)?).copied()
    }
}

/// `Atmosphere::earth`'s inner radius, metres: where sea level sits on the
/// modelled sphere.
const EARTH_RADIUS: f32 = 6_360_000.0;

/// The directional light that follows the day cycle.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Sun;

/// The moon: a dim, cool directional light opposite the sun, casting no
/// shadows, which takes over the lighting once the sun is down.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Moon;

/// How dark it is, `0` in daylight and `1` at night, from the sun's height:
/// what exposure adapts by and what a game dims or lights its lamps by.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct Night {
    /// `0` to `1`.
    pub darkness: f32,
}

/// How dark it is with the sun at height `sun_y` (the `y` of the unit vector
/// towards it): day until it is low, night once it is a little below the
/// horizon, eased between.
#[must_use]
pub fn darkness(sun_y: f32) -> f32 {
    let t = ((0.12 - sun_y) / 0.30).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The planet whose centre the atmosphere is measured from. Its
/// `WorldPosition` is kept one radius below sea level under the primary
/// observer by the plugin's follow system; the render-origin sync derives its
/// `Transform` from that like any other rendered entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Planet {
    /// Distance from the centre to sea level, metres.
    pub radius: f64,
}

/// Opt a `Camera3d` out of the sky, exposure and tonemapping; it lives in
/// `strata-app` so a screen crate can mark its camera without linking the
/// atmosphere.
pub use strata_app::NoSky;

/// Marks a camera this plugin has dressed, so it is done once.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SkyCamera;

/// Registers the atmosphere, the sun and the camera dressing.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        diagnostics::register(app);
        prepass::register(app);
        fog_pass::register(app);
        light_rays::register(app);
        // Local fog volumes (lighting plan phase 2c).
        local_fog::register(app);
        // The 2D cloud layer and its shadows (lighting plan phase 2b).
        clouds::register(app);
        // Colour scripting (lighting plan phase 4a).
        grade::register(app);
        // Exposure and the lens (lighting plan phase 4b).
        exposure::register(app);
        // The sky's environment map, filtered when the sky changes
        // (lighting plan phase 6).
        environment_hold::register(app);
        // The sun's cascades, each with only the casters that can shade
        // its slice (lighting plan phase 6).
        cascade_trim::register(app);
        // The 3D camera at a share of the window's size (review F8).
        render_scale::register(app);
        app.init_resource::<DayCycle>()
            .init_resource::<Night>()
            .init_resource::<SkySettings>()
            .init_resource::<look::RenderLook>()
            .init_resource::<study::StudySettings>()
            // The sky environment map replaces Bevy's flat ambient; both at
            // once lifts shadows to grey.
            .insert_resource(GlobalAmbientLight::NONE)
            .add_systems(Startup, spawn_sky)
            .add_systems(
                PostUpdate,
                (
                    look::reset_camera_cuts.after(bevy::transform::TransformSystems::Propagate),
                    look::sync_sky_fill,
                    // Irradiance probes (lighting plan phase 3b): before
                    // visibility is checked, so a hidden probe is out of
                    // this frame's view.
                    look::apply_irradiance
                        .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
                ),
            )
            .add_systems(
                Update,
                (
                    (dress_cameras, look::apply_cameras, move_sun, adapt_exposure).chain(),
                    look::apply_medium,
                    // The sun's cascades (lighting plan phase 6).
                    apply_cascades,
                    look::log_shadow_gpu_time,
                    study::update_volumes,
                    follow_observer.run_if(in_state(AppState::InWorld)),
                ),
            );
    }
}

/// The planet's atmosphere and the sun.
///
/// `Assets<ScatteringMedium>` only exists with Bevy's render plugins; a
/// headless app that adds this plugin (the tests do) gets the planet and
/// the sun and no sky rather than a panic.
fn spawn_sky(
    mut commands: Commands,
    mediums: Option<ResMut<Assets<ScatteringMedium>>>,
    settings: Res<SkySettings>,
) {
    // Bevy's `Atmosphere` hook would place the entity one planet radius
    // below the render origin on its own; giving it a `Transform` and a
    // `WorldPosition` instead puts it under the origin sync (ADR 0002), so
    // the camera's altitude above the planet is its true height above sea
    // level whatever the origin is. Until the sync first runs, the spawn
    // transform is what the hook would have written.
    let radius = EARTH_RADIUS;
    let mut planet = commands.spawn((
        Name::new("planet"),
        Planet {
            radius: f64::from(radius),
        },
        WorldPosition::new(0.0, -f64::from(radius), 0.0),
        Transform::from_translation(-Vec3::Y * radius),
    ));
    if let Some(mut mediums) = mediums {
        let medium =
            mediums.add(ScatteringMedium::earth(256, 256).with_density_multiplier(settings.haze));
        let mut atmosphere = Atmosphere::earth(medium);
        // The planet entity's position is derived from this radius, so the
        // two are the same number by construction.
        atmosphere.inner_radius = radius;
        // Whatever shows below the horizon where nothing is drawn is far
        // sea, so the planet's bare ground is sea-coloured rather than
        // Bevy's grey.
        atmosphere.ground_albedo = Vec3::new(0.02, 0.06, 0.1);
        planet.insert(atmosphere);
    } else {
        warn!("no Assets<ScatteringMedium>: sun only, no atmosphere");
    }

    commands.insert_resource(DirectionalLightShadowMap {
        size: settings.shadow_map_size,
    });
    commands.spawn((
        Name::new("sun"),
        Sun,
        DirectionalLight {
            color: Color::WHITE,
            // The atmosphere attenuates this by transmittance per fragment,
            // so it is the sun's raw value, not what reaches the ground.
            illuminance: light_consts::lux::RAW_SUNLIGHT,
            shadow_maps_enabled: settings.shadows,
            #[cfg(feature = "pcss")]
            soft_shadow_size: settings.soft_shadow_size,
            ..Default::default()
        },
        sun_disk(&settings),
        settings.cascades(),
        Transform::IDENTITY,
    ));
    commands.spawn((
        Name::new("moon"),
        Moon,
        DirectionalLight {
            color: Color::srgb(0.84, 0.88, 1.0),
            illuminance: 0.0,
            shadow_maps_enabled: false,
            ..Default::default()
        },
        Transform::IDENTITY,
    ));
}

/// Keeps the planet's centre one radius below sea level, under the primary
/// observer. Runs in `Update` so the origin sync in `PostUpdate` derives
/// the transform the same frame.
fn follow_observer(
    observers: Res<Observers>,
    source: Option<Res<WorldSourceRes>>,
    mut planets: Query<(&Planet, &mut WorldPosition)>,
) {
    let Some(observer) = observers.primary else {
        return;
    };
    let sea_level = source.map_or(0.0, |s| f64::from(s.source().sea_level()));
    for (planet, mut position) in &mut planets {
        let next = WorldPos::new(observer.x(), sea_level - planet.radius, observer.z());
        if next != position.0 {
            position.0 = next;
        }
    }
}

/// Puts the sky on every new `Camera3d` that has not opted out.
fn dress_cameras(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<SkyCamera>, Without<NoSky>)>,
    settings: Res<SkySettings>,
) {
    for entity in &cameras {
        let mut camera = commands.entity(entity);
        camera.insert((
            SkyCamera,
            AtmosphereSettings {
                aerial_view_lut_max_distance: settings.aerial_view_distance,
                ..Default::default()
            },
            Exposure {
                ev100: settings.ev100,
            },
            Tonemapping::AcesFitted,
            environment_map_light(&settings),
        ));
        if let Some(visibility) = settings.fog_visibility {
            camera.insert(DistanceFog {
                color: Color::srgb(0.7, 0.78, 0.9),
                directional_light_color: Color::srgb(1.0, 0.95, 0.85),
                directional_light_exponent: 30.0,
                falloff: FogFalloff::from_visibility(visibility),
            });
        }
    }
}

/// The sun's disc as the settings ask for it.
fn sun_disk(settings: &SkySettings) -> SunDisk {
    SunDisk {
        intensity: SunDisk::EARTH.intensity * settings.sun_disk_intensity,
        ..SunDisk::EARTH
    }
}

/// The environment light a camera is dressed with, at the settings' cubemap
/// size. Bevy reads the size once, when it first sees the camera, so every
/// writer of this component on a camera must carry the same size, or the
/// one Bevy happened to see first would win.
#[must_use]
pub fn environment_map_light(settings: &SkySettings) -> AtmosphereEnvironmentMapLight {
    AtmosphereEnvironmentMapLight {
        size: UVec2::splat(settings.environment_map_size),
        ..Default::default()
    }
}

/// Keeps the sun's cascades on what the settings ask for, so the F6 panel's
/// shadow distance, first cascade and cascade count are tried live. Bevy
/// builds each frame's cascade frusta from the component, so writing it is
/// all a change takes.
fn apply_cascades(
    settings: Res<SkySettings>,
    mut suns: Query<&mut CascadeShadowConfig, With<Sun>>,
) {
    if !settings.is_changed() {
        return;
    }
    let wanted = settings.cascades();
    for mut config in &mut suns {
        if config.bounds != wanted.bounds
            || config.overlap_proportion.to_bits() != wanted.overlap_proportion.to_bits()
            || config.minimum_distance.to_bits() != wanted.minimum_distance.to_bits()
        {
            *config = wanted.clone();
        }
    }
}

/// Points the sun by the sim's time of day. Runs in every state so a scene
/// rendered before `InWorld` is lit; the time only advances once the sim
/// runs.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn move_sun(
    time_of_day: Res<TimeOfDay>,
    cycle: Res<DayCycle>,
    settings: Res<SkySettings>,
    look: Res<look::RenderLook>,
    study: Res<study::StudySettings>,
    // The sun under cloud (lighting plan phase 2b follow-up): how much of
    // the disc the cloud layer lets through this frame.
    veil: Option<Res<clouds::SunVeil>>,
    mut night: ResMut<Night>,
    mut suns: Query<
        (&mut Transform, &mut DirectionalLight, Option<&mut SunDisk>),
        (With<Sun>, Without<Moon>),
    >,
    mut moons: Query<(&mut Transform, &mut DirectionalLight), (With<Moon>, Without<Sun>)>,
) {
    let to_sun = cycle.sun_direction(study.visual_time.unwrap_or(time_of_day.0));
    let rotation = sun_rotation(to_sun);
    // The atmosphere handles dusk; this only stops the light shining up
    // through the ground once the sun is well below the horizon.
    let up = ((to_sun.y + 0.12) / 0.12).clamp(0.0, 1.0);
    let mut disk = sun_disk(&settings);
    // The sun under cloud: the disc, and the bloom's halo that grows from
    // it, go behind a cloud and come back as it clears.
    disk.intensity *= veil.map_or(1.0, |v| v.clear);
    for (mut transform, mut light, sun_disk) in &mut suns {
        transform.rotation = rotation;
        if let Some(mut sun_disk) = sun_disk
            && sun_disk.intensity.to_bits() != disk.intensity.to_bits()
        {
            sun_disk.intensity = disk.intensity;
        }
        light.illuminance = light_consts::lux::RAW_SUNLIGHT * up * settings.sun_intensity;
        light.shadow_maps_enabled = settings.shadows;
        #[cfg(feature = "pcss")]
        {
            light.soft_shadow_size = settings.soft_shadow_size;
        }
        light.contact_shadows_enabled = look.contact_shadows;
    }
    let dark = darkness(to_sun.y);
    night.set_if_neq(Night { darkness: dark });
    // The moon rides opposite the sun, a little off its path so its light
    // does not fall along the same lines the day's did.
    let to_moon = Vec3::new(-to_sun.x * 0.8, -to_sun.y, -to_sun.z + 0.35).normalize();
    for (mut transform, mut light) in &mut moons {
        transform.rotation = sun_rotation(to_moon);
        light.illuminance = settings.moon_lux * dark * to_moon.y.max(0.0).sqrt();
    }
}

/// Adapts every sky camera's exposure to the dark, the way an eye does: the
/// look's daylight EV100 by day, [`SkySettings::night_ev100`] by night.
fn adapt_exposure(
    night: Res<Night>,
    settings: Res<SkySettings>,
    mut cameras: Query<&mut Exposure, With<SkyCamera>>,
) {
    let ev100 = settings.ev100 + (settings.night_ev100 - settings.ev100) * night.darkness;
    for mut exposure in &mut cameras {
        if (exposure.ev100 - ev100).abs() > 1e-3 {
            exposure.ev100 = ev100;
        }
    }
}

/// A directional light shines down its -z, so the rotation looks along the
/// direction from the sun to the ground. The up hint avoids the degenerate
/// case of a sun exactly overhead, which a tilt of 0 would produce at noon.
fn sun_rotation(to_sun: Vec3) -> Quat {
    let up = if to_sun.abs_diff_eq(Vec3::Y, 1e-3) {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Transform::IDENTITY.looking_to(-to_sun, up).rotation
}

// The sun's cascades (lighting plan phase 6).
/// Takes out of each of the sun's cascades the casters whose shadow cannot
/// reach the slice of the view that cascade shades.
///
/// Bevy 0.19.1 gives a cascade every caster inside the light-space box
/// around its slice of the view, open towards the sun. That box is
/// axis-aligned to the light and holds the slice's whole height (1.6 km
/// above and below the eye at 4 km out), so it takes in the ground under
/// the camera: at the city street every piece in band was drawn into the
/// far cascades too, 8 897 and 9 128 caster draws in the two far ones against
/// 1 786 in the near one. A fragment picks its cascade by its view
/// depth, so a caster matters to a cascade only where its shadow, its box
/// swept away from the sun, meets that slice of the view frustum. This
/// tests the swept box against the slice's six planes: each plane allows an
/// interval of the sweep, and an empty intersection is a caster whose
/// shadow leaves the view (under its bottom edge, out at a side) before it
/// reaches the slice, or one wholly past the slice's far end. Conservative:
/// the ground in between does not stop the sweep, so a low sun behind the
/// camera keeps nearly everything, and nothing that could shade a fragment
/// on screen, or a step of the fog marched through the view, is taken out.
pub mod cascade_trim {
    use bevy::app::{App, PostUpdate};
    use bevy::camera::primitives::{Aabb, Frustum};
    use bevy::camera::visibility::CascadesVisibleEntities;
    use bevy::ecs::prelude::*;
    use bevy::light::{CascadeShadowConfig, DirectionalLight, SimulationLightSystems};
    use bevy::math::{Affine3A, Vec3A};
    use bevy::transform::components::GlobalTransform;

    use super::SkySettings;

    /// A plane as `normal . x + d >= 0` inside.
    #[derive(Clone, Copy, Debug)]
    pub struct Plane {
        /// Points into the kept side.
        pub normal: Vec3A,
        /// The offset.
        pub d: f32,
    }

    /// Metres of slack on the slice's near and far planes, against float
    /// error at kilometres from the origin.
    const SLACK: f32 = 1.0;

    pub(super) fn register(app: &mut App) {
        app.add_systems(
            PostUpdate,
            trim.after(SimulationLightSystems::CheckLightVisibility),
        );
    }

    /// Whether the box `aabb` placed by `affine`, swept from `t = 0` along
    /// `dir` without end, meets every one of `planes`' kept sides at a
    /// common `t`. Each plane is met from the sweep's farthest-forward
    /// corner, so the test can keep a box that misses but never drops one
    /// that meets.
    #[must_use]
    pub fn sweep_meets(aabb: &Aabb, affine: &Affine3A, dir: Vec3A, planes: &[Plane]) -> bool {
        let centre = affine.transform_point3a(aabb.center);
        let axes = affine.matrix3;
        let (mut low, mut high) = (0.0_f32, f32::INFINITY);
        for plane in planes {
            let n = plane.normal;
            let radius = n.dot(axes.x_axis).abs() * aabb.half_extents.x
                + n.dot(axes.y_axis).abs() * aabb.half_extents.y
                + n.dot(axes.z_axis).abs() * aabb.half_extents.z;
            // The box's farthest reach into the kept side at t = 0.
            let reach = n.dot(centre) + plane.d + radius;
            let along = n.dot(dir);
            if along > 1e-6 {
                low = low.max(-reach / along);
            } else if along < -1e-6 {
                high = high.min(reach / -along);
            } else if reach < 0.0 {
                return false;
            }
            if low > high {
                return false;
            }
        }
        true
    }

    /// The planes of the slice of a view between view depths `near` and
    /// `far`: the view frustum's four sides and two at those depths.
    #[must_use]
    pub fn slice(frustum: &Frustum, view: &GlobalTransform, near: f32, far: f32) -> [Plane; 6] {
        let forward = Vec3A::from(view.forward().as_vec3());
        let eye = view.translation_vec3a();
        let side = |i: usize| Plane {
            normal: frustum.half_spaces[i].normal(),
            d: frustum.half_spaces[i].d(),
        };
        [
            side(0),
            side(1),
            side(2),
            side(3),
            Plane {
                normal: forward,
                d: -forward.dot(eye) - (near - SLACK),
            },
            Plane {
                normal: -forward,
                d: forward.dot(eye) + far + SLACK,
            },
        ]
    }

    /// Trims every shadow-casting directional light's cascades, after Bevy
    /// has filled them for the frame and before they are extracted.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "a Bevy system's parameters are its dependencies"
    )]
    fn trim(
        settings: Res<SkySettings>,
        mut lights: Query<(
            &DirectionalLight,
            &GlobalTransform,
            &CascadeShadowConfig,
            &mut CascadesVisibleEntities,
        )>,
        views: Query<(&GlobalTransform, &Frustum)>,
        casters: Query<(&GlobalTransform, &Aabb)>,
    ) {
        if !settings.trim_shadow_casters {
            return;
        }
        for (light, transform, config, mut visible) in &mut lights {
            if !light.shadow_maps_enabled {
                continue;
            }
            let dir = Vec3A::from(transform.forward().as_vec3());
            for (view, cascades) in &mut visible.entities {
                let Ok((view, frustum)) = views.get(*view) else {
                    continue;
                };
                for (i, cascade) in cascades.iter_mut().enumerate() {
                    let (Some(&far), near) = (
                        config.bounds.get(i),
                        i.checked_sub(1)
                            .map_or(config.minimum_distance, |previous| {
                                config.bounds[previous] * (1.0 - config.overlap_proportion)
                            }),
                    ) else {
                        continue;
                    };
                    let planes = slice(frustum, view, near, far);
                    cascade.entities.retain(|entity| {
                        casters.get(*entity).map_or(true, |(at, aabb)| {
                            sweep_meets(aabb, &at.affine(), dir, &planes)
                        })
                    });
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::camera::primitives::Frustum;
        use bevy::camera::{PerspectiveProjection, Projection};
        use bevy::math::primitives::ViewFrustum;
        use bevy::math::{Vec3, Vec3A};
        use bevy::transform::components::Transform;

        /// A camera at eye height looking along -z with the slice's planes
        /// from `near` to `far`.
        fn planes(near: f32, far: f32) -> [Plane; 6] {
            let view = GlobalTransform::from(
                Transform::from_xyz(0.0, 1.8, 0.0).looking_to(Vec3::NEG_Z, Vec3::Y),
            );
            let projection = Projection::Perspective(PerspectiveProjection {
                aspect_ratio: 16.0 / 9.0,
                ..Default::default()
            });
            let frustum = Frustum(ViewFrustum::from_clip_from_world_custom_far(
                &(projection.get_clip_from_view() * view.to_matrix().inverse()),
                &view.translation(),
                &view.back(),
                projection.far(),
            ));
            slice(&frustum, &view, near, far)
        }

        fn house(at: Vec3) -> (Aabb, Affine3A) {
            (
                Aabb {
                    center: Vec3A::ZERO,
                    half_extents: Vec3A::new(5.0, 10.0, 5.0),
                },
                Affine3A::from_translation(at),
            )
        }

        #[test]
        fn a_near_house_under_a_high_sun_leaves_the_far_slices() {
            // The sun high and behind the camera: shadows fall forward and
            // steeply down, out of the frustum's bottom long before 330 m.
            let dir = Vec3A::from(Vec3::new(0.0, -0.8, -0.6).normalize());
            let (aabb, at) = house(Vec3::new(0.0, 10.0, -100.0));
            assert!(sweep_meets(&aabb, &at, dir, &planes(0.5, 120.0)));
            assert!(!sweep_meets(&aabb, &at, dir, &planes(330.0, 1_245.0)));
            // A house inside the far slice keeps its place there.
            let (aabb, at) = house(Vec3::new(0.0, 10.0, -600.0));
            assert!(sweep_meets(&aabb, &at, dir, &planes(330.0, 1_245.0)));
            // And one wholly past the slice's far end leaves it.
            assert!(!sweep_meets(&aabb, &at, dir, &planes(0.5, 120.0)));
        }

        #[test]
        fn a_low_sun_behind_the_camera_keeps_a_near_house_everywhere() {
            // Five degrees up: the shadow runs on along the view, inside the
            // frustum, and without the ground the test cannot know where it
            // stops.
            let dir = Vec3A::from(Vec3::new(0.0, -0.087, -0.996).normalize());
            let (aabb, at) = house(Vec3::new(0.0, 10.0, -100.0));
            assert!(sweep_meets(&aabb, &at, dir, &planes(330.0, 1_245.0)));
        }

        #[test]
        fn a_house_behind_the_camera_with_the_sun_ahead_is_dropped() {
            // Sun in front, shadows towards the camera and past it.
            let dir = Vec3A::from(Vec3::new(0.0, -0.5, 0.866).normalize());
            let (aabb, at) = house(Vec3::new(0.0, 10.0, 50.0));
            assert!(!sweep_meets(&aabb, &at, dir, &planes(0.5, 120.0)));
        }
    }
}

// The sky's environment map, filtered when the sky changes (lighting plan
// phase 6).
/// Filters the sky's environment map only when the sky has changed.
///
/// Bevy 0.19.1 draws the sky into the cubemap and filters it into the
/// diffuse and specular maps every frame, with no control over when: the
/// filter's bind groups alone were 0.41 ms of the render thread a frame
/// (`docs/perf/profile-2026-10-03.md`). Its filtering systems work on every
/// render-world camera carrying `RenderEnvironmentMap`, which
/// `extract_generated_environment_map_entities` inserts afresh each frame
/// and nothing else reads, so on a frame the sky has not changed this takes
/// it off again before the prepare systems run: no intermediate texture, no
/// bind groups, no downsampling or convolution, and the filtered maps of the
/// last refresh stay bound. The sky's own draw into the cubemap goes on each
/// frame (its components are private to `bevy_pbr`), which at the smaller
/// size is a few thousand texels a face.
mod environment_hold {
    use bevy::app::{App, PostUpdate};
    use bevy::ecs::entity::EntityHashMap;
    use bevy::ecs::prelude::*;
    use bevy::math::Vec3;
    use bevy::pbr::generate::{GeneratorPipelines, RenderEnvironmentMap};
    use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
    use bevy::render::render_resource::{
        CachedPipelineState, PipelineCache, PipelineDescriptor, TextureId,
    };
    use bevy::render::{Render, RenderApp, RenderSystems};
    use bevy::transform::components::Transform;
    use strata_app::Observers;
    use strata_core::WorldPos;

    use super::{SkySettings, Sun};

    /// How far the observer must climb or descend, metres, before the sky is
    /// filtered again: the sky seen from a few hundred metres up is a
    /// little paler towards the horizon, and a flight changes height a
    /// metre a frame at most.
    const HEIGHT_STEP: f64 = 50.0;

    /// Frames the filter runs after a change once its pipelines are ready:
    /// it reads the cubemap the sky drew the frame before, so one frame
    /// would filter the stale sky, and the first refresh also waits out the
    /// sky's lookup tables settling.
    const SETTLE_AFTER_CHANGE: u32 = 4;
    const SETTLE_AT_START: u32 = 60;

    /// The sky's own pipelines, by label, which `bevy_pbr` keeps private:
    /// until they are compiled the cubemap the filter reads is blank, and a
    /// blank map held until the sun next moved a degree would light the
    /// shade black.
    const SKY_PIPELINES: [&str; 5] = [
        "environment_pipeline",
        "transmittance_lut_pipeline",
        "multi_scattering_lut_pipeline",
        "sky_view_lut_pipeline",
        "aerial_view_lut_pipeline",
    ];

    /// What the sky's environment map is drawn from, as the main world sees
    /// it this frame.
    #[derive(Resource, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
    pub(super) struct SkyKey {
        /// Unit vector towards the sun.
        to_sun: Vec3,
        /// The primary observer's height, metres.
        height: f64,
        /// Bumped whenever the sky settings change (the medium, the sun's
        /// strength), which the other two do not see.
        revision: u64,
        /// [`SkySettings::environment_map_refresh_degrees`], as a cosine;
        /// `None` refreshes every frame.
        turn_cos: Option<f32>,
    }

    /// What the render world last filtered, and how far it has settled.
    #[derive(Resource, Default)]
    struct Held {
        last: Option<SkyKey>,
        settled: u32,
        settle: u32,
        sky_ready: bool,
        /// The cameras filtered so far, each with the specular map it was
        /// filtered into: the settle is global, so a camera that appears
        /// after it (a slow load, a `--connect` client) or whose maps were
        /// rebuilt would otherwise keep its zero fill until the sun moved.
        targets: EntityHashMap<(TextureId, TextureId)>,
    }

    pub(super) fn register(app: &mut App) {
        app.init_resource::<SkyKey>()
            .add_plugins(ExtractResourcePlugin::<SkyKey>::default())
            .add_systems(PostUpdate, publish);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.init_resource::<Held>().add_systems(
                Render,
                hold.after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::PrepareResources),
            );
        }
    }

    /// Writes this frame's [`SkyKey`], in `PostUpdate` after the sun has
    /// been pointed in `Update`.
    fn publish(
        settings: Res<SkySettings>,
        observers: Res<Observers>,
        suns: Query<&Transform, With<Sun>>,
        mut key: ResMut<SkyKey>,
    ) {
        let revision = key.revision + u64::from(settings.is_changed());
        let degrees = settings.environment_map_refresh_degrees;
        let next = SkyKey {
            to_sun: suns
                .iter()
                .next()
                .map_or(Vec3::Y, |sun| sun.rotation * Vec3::Z),
            height: observers.primary.map_or(0.0, WorldPos::y),
            revision,
            turn_cos: (degrees > 0.0).then(|| degrees.to_radians().cos()),
        };
        if *key != next {
            *key = next;
        }
    }

    /// Whether the sky has moved from `last` to `now` enough to filter again.
    fn changed(last: &SkyKey, now: &SkyKey, turn_cos: f32) -> bool {
        last.revision != now.revision
            || (last.height - now.height).abs() > HEIGHT_STEP
            || last.to_sun.dot(now.to_sun) < turn_cos
    }

    /// Takes `RenderEnvironmentMap` off every camera on a frame the sky has
    /// not changed, once the filter has settled on the current sky.
    fn hold(
        mut commands: Commands,
        key: Res<SkyKey>,
        cache: Res<PipelineCache>,
        filter: Option<Res<GeneratorPipelines>>,
        maps: Query<(Entity, &RenderEnvironmentMap)>,
        mut held: ResMut<Held>,
    ) {
        let Some(turn_cos) = key.turn_cos else {
            held.last = None;
            return;
        };
        if !held.sky_ready {
            held.sky_ready = SKY_PIPELINES.iter().all(|label| {
                cache.pipelines().any(|pipeline| {
                    matches!(pipeline.state, CachedPipelineState::Ok(_))
                        && matches!(&pipeline.descriptor,
                            PipelineDescriptor::ComputePipelineDescriptor(d)
                                if d.label.as_deref() == Some(*label))
                })
            });
        }
        let filter_ready = filter.is_some_and(|p| {
            [
                p.copy,
                p.downsample_first,
                p.downsample_second,
                p.radiance,
                p.irradiance,
            ]
            .into_iter()
            .all(|id| cache.get_compute_pipeline(id).is_some())
        });
        if !held.sky_ready || !filter_ready {
            return;
        }
        // Measured from the sky last filtered, not last frame's, so a sun
        // turning a hundredth of a degree a frame is still caught up with.
        let fresh = retarget(&mut held.targets, &maps);
        let settle = match held.last {
            None => Some(SETTLE_AT_START),
            Some(last) if fresh || changed(&last, &key, turn_cos) => Some(SETTLE_AFTER_CHANGE),
            Some(_) => None,
        };
        if let Some(settle) = settle {
            held.last = Some(*key);
            held.settle = settle;
            held.settled = 0;
        }
        if held.settled < held.settle {
            held.settled += 1;
            return;
        }
        for (map, _) in &maps {
            commands.entity(map).remove::<RenderEnvironmentMap>();
        }
    }

    /// Brings `targets` up to this frame's cameras and says whether any of
    /// them is new or now filters into other textures.
    fn retarget(
        targets: &mut EntityHashMap<(TextureId, TextureId)>,
        maps: &Query<(Entity, &RenderEnvironmentMap)>,
    ) -> bool {
        let mut fresh = false;
        let mut now = EntityHashMap::default();
        for (entity, map) in maps {
            let ids = (map.diffuse_map.texture.id(), map.specular_map.texture.id());
            fresh |= targets.get(&entity) != Some(&ids);
            now.insert(entity, ids);
        }
        *targets = now;
        fresh
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_sky_is_filtered_again_past_a_degree_a_climb_or_an_edit() {
            let base = SkyKey {
                to_sun: Vec3::new(0.0, 0.5, -1.0).normalize(),
                height: 100.0,
                revision: 3,
                turn_cos: Some(1f32.to_radians().cos()),
            };
            let cos = base.turn_cos.expect("set");
            let turned = |degrees: f32| SkyKey {
                to_sun: bevy::math::Quat::from_rotation_y(degrees.to_radians()) * base.to_sun,
                ..base
            };
            assert!(!changed(&base, &turned(0.5), cos));
            assert!(changed(&base, &turned(1.5), cos));
            assert!(!changed(
                &base,
                &SkyKey {
                    height: 140.0,
                    ..base
                },
                cos
            ));
            assert!(changed(
                &base,
                &SkyKey {
                    height: 160.0,
                    ..base
                },
                cos
            ));
            assert!(changed(
                &base,
                &SkyKey {
                    revision: 4,
                    ..base
                },
                cos
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_over_the_sim() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            strata_app::AppPlugin,
            AtmospherePlugin,
        ));
        app.update();
    }

    #[test]
    fn it_is_light_by_day_dark_by_night_and_eased_between() {
        assert!(darkness(0.8).abs() < 1e-6);
        assert!((darkness(-0.5) - 1.0).abs() < 1e-6);
        let dusk = darkness(0.0);
        assert!(dusk > 0.1 && dusk < 0.9, "{dusk}");
        assert!(darkness(-0.05) > darkness(0.05));
    }

    #[test]
    fn noon_is_high_and_midnight_is_below() {
        let cycle = DayCycle::default();
        let noon = cycle.sun_direction(0.5);
        assert!(noon.y > 0.8, "{noon}");
        assert!(noon.z < 0.0, "noon is towards -z: {noon}");
        let midnight = cycle.sun_direction(0.0);
        assert!(midnight.y < -0.8, "{midnight}");
        let sunrise = cycle.sun_direction(0.25);
        assert!(sunrise.y.abs() < 1e-5 && sunrise.x > 0.99, "{sunrise}");
        let sunset = cycle.sun_direction(0.75);
        assert!(sunset.y.abs() < 1e-5 && sunset.x < -0.99, "{sunset}");
    }

    #[test]
    fn the_sun_follows_the_sims_time_of_day() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            strata_app::AppPlugin,
            AtmospherePlugin,
        ));
        app.insert_resource(TimeOfDay(0.25));
        app.update();
        app.update();
        let mut suns = app.world_mut().query_filtered::<&Transform, With<Sun>>();
        let transform = suns.single(app.world()).expect("one sun");
        // Sunrise: the light shines along -x, so its forward is -x.
        let forward = transform.rotation * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(Vec3::NEG_X, 1e-4), "{forward}");
    }

    #[test]
    fn a_visual_preview_freezes_the_sun_without_overwriting_simulation_time() {
        let mut app = App::new();
        app.init_resource::<DayCycle>()
            .init_resource::<Night>()
            .init_resource::<SkySettings>()
            .init_resource::<look::RenderLook>()
            .insert_resource(study::StudySettings {
                visual_time: Some(0.25),
                ..Default::default()
            })
            .insert_resource(TimeOfDay(0.75))
            .add_systems(Update, move_sun);
        let sun = app
            .world_mut()
            .spawn((Sun, DirectionalLight::default()))
            .id();
        app.update();
        let transform = app.world().get::<Transform>(sun).expect("sun transform");
        assert!((transform.rotation * Vec3::NEG_Z).abs_diff_eq(Vec3::NEG_X, 1e-4));
        assert!((app.world().resource::<TimeOfDay>().0 - 0.75).abs() < 1e-6);
        app.world_mut()
            .resource_mut::<study::StudySettings>()
            .visual_time = None;
        app.update();
        let transform = app.world().get::<Transform>(sun).expect("sun transform");
        assert!((transform.rotation * Vec3::NEG_Z).abs_diff_eq(Vec3::X, 1e-4));
    }

    #[test]
    fn the_planet_is_rebased_with_everything_else() {
        use bevy::time::TimeUpdateStrategy;
        use strata_app::{PrimaryObserver, RenderOrigin};

        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            strata_app::AppPlugin,
            AtmospherePlugin,
        ));
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        // Boot -> LoadingRegistries -> InWorld, one hop per update.
        for _ in 0..3 {
            app.update();
        }
        let camera = app
            .world_mut()
            .spawn((PrimaryObserver, WorldPosition::new(100.0, 300.0, -50.0)))
            .id();
        app.update();
        let mut planets = app
            .world_mut()
            .query_filtered::<(&WorldPosition, &Transform), With<Planet>>();
        let (pos, transform) = planets.single(app.world()).expect("one planet");
        assert_eq!(pos.0, WorldPos::new(100.0, -6_360_000.0, -50.0));
        // No rebase yet: the origin is the world origin, so render y is
        // minus the radius and the camera sits 300 m above the surface.
        assert_eq!(app.world().resource::<RenderOrigin>().0, WorldPos::ZERO);
        assert!(
            (transform.translation.y + 6_360_000.0).abs() < 1.0,
            "{transform:?}"
        );
        assert!(
            (transform.translation.x - 100.0).abs() < 1e-3,
            "{transform:?}"
        );

        // Past the rebase threshold: the origin moves to the camera, in y
        // too, and the planet's transform is rewritten so the camera is
        // still 300 m above its surface.
        app.world_mut()
            .get_mut::<WorldPosition>(camera)
            .expect("camera")
            .0 = WorldPos::new(5_000.0, 300.0, 0.0);
        app.update();
        let origin = app.world().resource::<RenderOrigin>().0;
        assert_eq!(origin, WorldPos::new(5_000.0, 300.0, 0.0));
        let (pos, transform) = planets.single(app.world()).expect("one planet");
        assert_eq!(pos.0, WorldPos::new(5_000.0, -6_360_000.0, 0.0));
        assert!(transform.translation.x.abs() < 1e-3, "{transform:?}");
        assert!(
            (transform.translation.y + 6_360_300.0).abs() < 1.0,
            "{transform:?}"
        );
    }

    #[test]
    fn sun_rotation_points_down_the_sun_direction() {
        let to_sun = Vec3::new(0.3, 0.8, -0.2).normalize();
        let rot = sun_rotation(to_sun);
        let forward = rot * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(-to_sun, 1e-5), "{forward} vs {to_sun}");
        // Overhead does not panic and still shines down.
        let down = sun_rotation(Vec3::Y) * Vec3::NEG_Z;
        assert!(down.abs_diff_eq(Vec3::NEG_Y, 1e-5), "{down}");
    }
}
