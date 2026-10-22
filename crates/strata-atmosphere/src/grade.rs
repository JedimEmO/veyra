//! Colour scripting (lighting plan phase 4a): a grade per biome and per
//! time of day, blended under the observer and written on the camera's
//! colour grading.
//!
//! GW2's art direction calls colour "a language in itself": soft greens and
//! golds for serenity, cold blues for foreboding. Here the coast is cool
//! and clear, the woodland green-gold, dusk warm and night blue. The biome
//! grades are data beside the biomes (`assets/biomes/*.biome.ron`,
//! `strata_registry::Grade`), the time keys `assets/look/times.ron`; this
//! module only blends them. Both are offsets from the neutral grade, so the
//! observer's grade is neutral plus the biome pair's weighted mean plus the
//! time's, scaled by [`RenderLook::grade`] and laid over the look's own
//! contrast and saturation as factors, never in place of them.
//!
//! It costs nothing on the GPU that was not already paid: Bevy applies
//! `ColorGrading` in the tonemapping pass every frame. The white balance
//! is one 3x3 matrix there and the sectional grading (lift, gain, the
//! contrast) a few more operations a pixel; the hue rotation, the dearest
//! part, is compiled out while every grade leaves the hue at zero, as the
//! shipped ones do. On the main thread the world is sampled at the
//! observer every 100 ms (one `WorldSource::sample`), the rest is a few
//! dozen float operations a frame.

use crate::look::RenderLook;
use crate::study::StudySettings;
use crate::{DayCycle, NoSky, SkyCamera, darkness};
use bevy::app::{App, Startup, Update};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::render::view::ColorGrading;
use bevy::time::{Real, Time};
use serde::Deserialize;
use strata_app::{Observers, Registries, TimeOfDay, WorldSourceRes};
use strata_core::WorldPos;
use strata_registry::{BiomeId, Grade};

/// The time keys' file under the asset root.
pub const TIMES_FILE: &str = "look/times.ron";

/// The time keys as shipped, compiled in so a tool or a test without the
/// asset directory grades the same; the file under the asset root takes
/// over at startup, so an edit to it needs a restart, not a rebuild.
const SHIPPED_TIMES: &str = include_str!("../../../assets/look/times.ron");

/// How often the world is sampled under the observer, seconds. A biome
/// border is hundreds of metres wide, so a tenth of a second at a flight's
/// 200 m/s is still a fraction of one.
const SAMPLE_INTERVAL: f64 = 0.1;

/// The time constant the biome grade eases towards a new sample with,
/// seconds: 95 % of a change in about a second, so a border never pops and
/// a sample that lands a step further on is not seen as one.
const EASE: f32 = 0.35;

/// A jump between two samples, metres, past which the grade is the new
/// place's at once: a teleport or a camera cut, not a walk.
const CUT: f64 = 300.0;

/// The four time-of-day keys of `assets/look/times.ron`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeGrades {
    /// The sun just over the horizon in the morning.
    pub dawn: Grade,
    /// The sun high: the light the materials were judged under.
    pub day: Grade,
    /// The sun just over the horizon in the evening.
    pub dusk: Grade,
    /// The sun well down.
    pub night: Grade,
}

impl Default for TimeGrades {
    fn default() -> Self {
        Self::shipped()
    }
}

impl TimeGrades {
    /// The keys as shipped in `assets/look/times.ron`.
    ///
    /// # Panics
    ///
    /// If the shipped file does not parse or validate, which a test rules
    /// out.
    #[must_use]
    pub fn shipped() -> Self {
        Self::from_ron(SHIPPED_TIMES).expect("assets/look/times.ron is valid")
    }

    /// Parses and validates a times file.
    ///
    /// # Errors
    ///
    /// The parse error, or the first key whose grade is out of range.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let times: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        for (key, grade) in times.keys() {
            if let Some((field, range)) = grade.invalid_field() {
                return Err(format!("{key}.{field} must be finite and in {range}"));
            }
        }
        Ok(times)
    }

    fn keys(&self) -> [(&'static str, &Grade); 4] {
        [
            ("dawn", &self.dawn),
            ("day", &self.day),
            ("dusk", &self.dusk),
            ("night", &self.night),
        ]
    }

    /// The keys blended by [`time_weights`].
    #[must_use]
    pub fn at(&self, weights: [f32; 4]) -> Grade {
        Grade::sum(
            self.keys()
                .into_iter()
                .zip(weights)
                .map(|((_, grade), weight)| (grade, weight)),
        )
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much each time key weighs with the sun along `to_sun` (a unit
/// vector from the ground, [`DayCycle::sun_direction`]), in the order
/// dawn, day, dusk, night; they sum to one.
///
/// Night is [`darkness`], the same easing exposure adapts by, so the grade
/// turns blue as the eye adapts and not before. The golden hour is the
/// rest of the light while the sun is under about 30 degrees, gone by
/// then: the dusk street's fixture (15 degrees) is two thirds dusk, the
/// canopy's 08:00 (24 degrees) a tenth dawn. Dawn and dusk split it by the
/// sun's side of the sky (it rises at +x and sets at -x), which is
/// continuous through noon and midnight where the split does not matter.
#[must_use]
pub fn time_weights(to_sun: Vec3) -> [f32; 4] {
    let night = darkness(to_sun.y);
    let golden = (1.0 - night) * (1.0 - smoothstep(0.1, 0.5, to_sun.y));
    let morning = (0.5 + to_sun.x).clamp(0.0, 1.0);
    let day = (1.0 - night - golden).max(0.0);
    [golden * morning, day, golden * (1.0 - morning), night]
}

/// The two biomes' grades by their shares, normalised so a pair whose
/// shares do not quite sum to one is still a mean.
#[must_use]
pub fn mix_biomes(biomes: [(&Grade, f32); 2]) -> Grade {
    let total = (biomes[0].1 + biomes[1].1).max(1e-6);
    Grade::sum(biomes.map(|(grade, weight)| (grade, weight / total)))
}

/// The camera's grading from the observer's biome grade and the time's,
/// at `strength` (0 is the look's own grading, 1 the grades in full).
///
/// The two are summed as offsets from neutral and the sum scaled; the
/// look's contrast (on the shadows and midtones, as
/// [`look::apply_cameras`](crate::look) writes it) and saturation multiply
/// the grade's, so the look stays a global multiplier the F6 panel moves
/// on top of any place and hour.
#[must_use]
pub fn compose(biome: &Grade, time: &Grade, strength: f32, look: &RenderLook) -> ColorGrading {
    let g = Grade::sum([(biome, strength), (time, strength)]);
    let mut grading = ColorGrading::default();
    grading.global.temperature = g.temperature;
    grading.global.tint = g.tint;
    grading.global.hue = g.hue;
    grading.global.post_saturation = look.saturation * g.saturation;
    grading.midtones.contrast = look.contrast * g.contrast;
    grading.shadows.contrast = look.contrast * g.contrast;
    grading.shadows.lift = g.lift;
    grading.highlights.gain = g.gain;
    grading
}

/// The Rec. 709 luminance weights, for the tint's normalisation.
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// The colour of the shade at the observer, the biome's and the time's
/// [`Grade::shade`] summed as offsets at `strength` like the rest of the
/// grade, then divided by its own luminance: a tint that moves the shade's
/// hue and never its brightness, so exposure and the lit surfaces stay put.
#[must_use]
pub fn shade_tint(biome: &Grade, time: &Grade, strength: f32) -> Vec3 {
    let g = Grade::sum([(biome, strength), (time, strength)]);
    let tint = Vec3::from(g.shade).max(Vec3::splat(0.05));
    tint / tint.dot(LUMA)
}

/// What the look writes once a frame for every material we own to read
/// (visual.md F4): a colour per quantity, each a linear multiplier on a
/// term the material already has. Only the shade is written yet; the sun's
/// tint and the air's colour are here so the CPU side keeps one shape; each
/// material's own uniform still needs a row per quantity.
///
/// The client carries it to each material's own parameters
/// (`strata_client::ui::render`), because a bind group shared by terrain,
/// trees, grass, impostors and ashlar would need a pipeline layout change in
/// each of them for the same one multiply.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct LookUniform {
    /// Multiplies `diffuse_occlusion`, so only indirect diffuse (the sky and
    /// the probes, a shaded surface's whole light) takes the colour.
    /// Luminance one.
    pub shade_tint: Vec3,
    /// Reserved: a tint on the direct sun. Not read yet.
    pub sun_tint: Vec3,
    /// Reserved: the air's colour. Not read yet.
    pub air_colour: Vec3,
}

impl Default for LookUniform {
    fn default() -> Self {
        Self {
            shade_tint: Vec3::ONE,
            sun_tint: Vec3::ONE,
            air_colour: Vec3::ONE,
        }
    }
}

/// The whole blend in one: the biome pair by its shares, the time keys by
/// their weights, at `strength`, over the look.
#[must_use]
pub fn blend(
    biomes: [(&Grade, f32); 2],
    times: &TimeGrades,
    time_weights: [f32; 4],
    strength: f32,
    look: &RenderLook,
) -> ColorGrading {
    compose(&mix_biomes(biomes), &times.at(time_weights), strength, look)
}

/// The biome grade under the observer, eased towards the latest sample.
#[derive(Resource, Debug, Clone, Default)]
pub struct BiomeGrade {
    /// What the frame is graded with now.
    pub current: Grade,
    /// The latest sample's blend, which `current` eases towards.
    pub target: Grade,
    /// The latest sample's biomes and shares, for the log.
    pub biomes: Option<[(BiomeId, f32); 2]>,
    /// Where and when (real seconds) the world was last sampled.
    last: Option<(WorldPos, f64)>,
    /// Main-thread microseconds the last sample took, for the log.
    pub sample_us: f32,
}

/// The height gradient of built surfaces (`assets/look/buildings.ron`, visual.md
/// F7), read at start-up over the shipped dials. The client writes it to every
/// building material; it lives beside the shade because both are the grade's
/// word to materials that do not see the atmosphere.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct BuildingGradient(pub strata_registry::BuildingLook);

pub(crate) fn register(app: &mut App) {
    app.init_resource::<TimeGrades>()
        .init_resource::<BuildingGradient>()
        .init_resource::<BiomeGrade>()
        .init_resource::<LookUniform>()
        .add_systems(Startup, (load_times, load_buildings))
        .add_systems(
            Update,
            (
                track_biome,
                apply_grade.after(crate::look::apply_cameras),
                log_grade,
            )
                .chain(),
        );
}

/// Reads the time keys from the asset root, when the app has one, over the
/// compiled-in copy; a broken file is reported and the shipped keys kept.
fn load_times(root: Option<Res<strata_app::WorldAssetRoot>>, mut times: ResMut<TimeGrades>) {
    let Some(root) = root else {
        return;
    };
    match root.0.read_string(TIMES_FILE) {
        Ok(text) => match TimeGrades::from_ron(&text) {
            Ok(read) => *times = read,
            Err(error) => tracing::warn!("{TIMES_FILE}: {error}; the shipped keys stay"),
        },
        Err(error) => tracing::debug!("{TIMES_FILE}: {error}; the shipped keys stay"),
    }
}

/// Reads the buildings' gradient from the asset root, when the app has one,
/// over the compiled-in dials; a broken file is reported and the shipped dials
/// kept.
fn load_buildings(
    root: Option<Res<strata_app::WorldAssetRoot>>,
    mut gradient: ResMut<BuildingGradient>,
) {
    use strata_registry::{BUILDINGS_FILE, BuildingLook};
    let Some(root) = root else {
        return;
    };
    match root.0.read_string(BUILDINGS_FILE) {
        Ok(text) => match BuildingLook::parse(&text) {
            Ok(read) => gradient.0 = read,
            Err(error) => tracing::warn!("{BUILDINGS_FILE}: {error}; the shipped dials stay"),
        },
        Err(error) => tracing::debug!("{BUILDINGS_FILE}: {error}; the shipped dials stay"),
    }
}

/// Samples the world's biomes under the observer every
/// [`SAMPLE_INTERVAL`] and eases the biome grade towards their blend.
fn track_biome(
    time: Res<Time<Real>>,
    observers: Res<Observers>,
    source: Option<Res<WorldSourceRes>>,
    registries: Option<Res<Registries>>,
    mut state: ResMut<BiomeGrade>,
) {
    let now = time.elapsed_secs_f64();
    if let (Some(at), Some(source), Some(registries)) = (observers.primary, source, registries)
        && state
            .last
            .is_none_or(|(_, then)| now - then >= SAMPLE_INTERVAL)
    {
        let started = strata_core::platform::time::Instant::now();
        let sample = source.source().sample(at.x(), at.z());
        state.sample_us = started.elapsed().as_secs_f32() * 1e6;
        let grade = |id: BiomeId| {
            registries
                .0
                .biomes
                .get(id)
                .map_or(Grade::NEUTRAL, |def| def.grade)
        };
        let [(a, wa), (b, wb)] = sample.biomes;
        let (ga, gb) = (grade(a), grade(b));
        state.target = mix_biomes([(&ga, wa), (&gb, wb)]);
        state.biomes = Some(sample.biomes);
        let cut = state.last.is_none_or(|(then, _)| {
            let (dx, dz) = (at.x() - then.x(), at.z() - then.z());
            dx.hypot(dz) > CUT
        });
        if cut {
            state.current = state.target;
        }
        state.last = Some((at, now));
    }
    let t = 1.0 - (-time.delta_secs() / EASE).exp();
    if t > 0.0 && state.current != state.target {
        let (current, target) = (state.current, state.target);
        state.current = Grade::sum([(&current, 1.0 - t), (&target, t)]);
    }
}

/// Whether two gradings would draw the same frame. `ColorGrading` has no
/// `PartialEq`; comparing what this module writes is enough.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn same(a: &ColorGrading, b: &ColorGrading) -> bool {
    a.global.temperature == b.global.temperature
        && a.global.tint == b.global.tint
        && a.global.hue == b.global.hue
        && a.global.exposure == b.global.exposure
        && a.global.post_saturation == b.global.post_saturation
        && a.global.midtones_range == b.global.midtones_range
        && a.shadows == b.shadows
        && a.midtones == b.midtones
        && a.highlights == b.highlights
}

/// Writes the blended grade on every sky camera, after the look has
/// written its own (which this replaces with the composition of both).
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn apply_grade(
    look: Res<RenderLook>,
    times: Res<TimeGrades>,
    biome: Res<BiomeGrade>,
    cycle: Res<DayCycle>,
    time_of_day: Res<TimeOfDay>,
    study: Res<StudySettings>,
    mut uniform: ResMut<LookUniform>,
    mut cameras: Query<&mut ColorGrading, (With<SkyCamera>, Without<NoSky>)>,
) {
    let to_sun = cycle.sun_direction(study.visual_time.unwrap_or(time_of_day.0));
    let time = times.at(time_weights(to_sun));
    let shade = shade_tint(&biome.current, &time, look.grade);
    // Written only past a quantum of a 256th, so a settled frame marks
    // nothing changed and the materials are not touched.
    if (shade - uniform.shade_tint).abs().max_element() > 1.0 / 256.0 {
        uniform.shade_tint = shade;
    }
    let wanted = compose(&biome.current, &time, look.grade, &look);
    for mut grading in &mut cameras {
        if !same(&grading, &wanted) {
            *grading = wanted.clone();
        }
    }
}

/// Every two seconds with the statistics on (Bevy's render diagnostics):
/// the grade the camera has, what it is made of, and the tonemapping pass's
/// GPU time, which is where the grade is paid for.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn log_grade(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Res<Time<Real>>,
    state: Res<BiomeGrade>,
    registries: Option<Res<Registries>>,
    cycle: Res<DayCycle>,
    time_of_day: Res<TimeOfDay>,
    study: Res<StudySettings>,
    cameras: Query<&ColorGrading, (With<SkyCamera>, Without<NoSky>)>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(tonemapping) = diagnostics.and_then(|d| {
        d.iter()
            .find(|d| d.path().as_str().ends_with("/tonemapping/elapsed_gpu"))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    }) else {
        return;
    };
    let Some(g) = cameras.iter().next() else {
        return;
    };
    let name = |id: BiomeId| {
        registries
            .as_ref()
            .and_then(|r| r.0.biomes.get(id))
            .map_or_else(|| format!("{id:?}"), |def| def.name.clone())
    };
    let biomes = state.biomes.map_or_else(
        || "-".to_owned(),
        |[(a, wa), (b, wb)]| format!("{} {wa:.2} {} {wb:.2}", name(a), name(b)),
    );
    let w = time_weights(cycle.sun_direction(study.visual_time.unwrap_or(time_of_day.0)));
    tracing::info!(
        "colour grade: biomes {biomes}; dawn {:.2} day {:.2} dusk {:.2} night {:.2}; \
         temperature {:.4} tint {:.4} hue {:.3} post_saturation {:.3} contrast {:.3} \
         lift {:.4} gain {:.3}; sample {:.1} us; tonemapping {tonemapping:.3} ms of GPU time",
        w[0],
        w[1],
        w[2],
        w[3],
        g.global.temperature,
        g.global.tint,
        g.global.hue,
        g.global.post_saturation,
        g.midtones.contrast,
        g.shadows.lift,
        g.highlights.gain,
        state.sample_us,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coast() -> Grade {
        Grade {
            temperature: -0.006,
            saturation: 0.95,
            contrast: 1.04,
            ..Grade::NEUTRAL
        }
    }

    fn forest() -> Grade {
        Grade {
            temperature: 0.006,
            tint: -0.004,
            saturation: 1.06,
            lift: 0.002,
            ..Grade::NEUTRAL
        }
    }

    const NOON: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

    fn look() -> RenderLook {
        RenderLook {
            contrast: 1.12,
            saturation: 0.92,
            ..RenderLook::default()
        }
    }

    #[test]
    fn the_shipped_times_validate_and_a_wrong_one_is_refused() {
        let times = TimeGrades::shipped();
        assert!(times.day.is_neutral(), "midday is the neutral light");
        assert!(times.night.saturation <= 0.8, "night crushes the colour");
        assert!(times.night.temperature < 0.0, "night is blue");
        assert!(times.dusk.temperature > 0.0 && times.dawn.temperature > 0.0);
        assert!(TimeGrades::from_ron("(dawn: (), day: (), dusk: ())").is_err());
        let hot = SHIPPED_TIMES.replace("dusk: (temperature: 0.02", "dusk: (temperature: 0.2");
        assert_ne!(hot, SHIPPED_TIMES, "the test edits the dusk key");
        let error = TimeGrades::from_ron(&hot).expect_err("out of range");
        assert!(error.contains("dusk.temperature"), "{error}");
    }

    #[test]
    fn every_shipped_biome_has_a_valid_grade() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        let registries = strata_registry::load_from_dir(root).expect("the assets load");
        for (_, name, def) in registries.biomes.iter() {
            assert!(!def.grade.is_neutral(), "{name} has no grade");
            assert!(def.grade.invalid_field().is_none(), "{name}");
        }
    }

    #[test]
    fn strength_zero_is_the_looks_own_grading() {
        let look = look();
        let g = blend(
            [(&coast(), 0.7), (&forest(), 0.3)],
            &TimeGrades::shipped(),
            [0.0, 0.2, 0.3, 0.5],
            0.0,
            &look,
        );
        let mut own = ColorGrading::default();
        own.midtones.contrast = look.contrast;
        own.shadows.contrast = look.contrast;
        own.global.post_saturation = look.saturation;
        assert!(same(&g, &own), "{g:?}");
    }

    #[test]
    fn a_pure_coast_at_noon_is_the_coasts_grade_over_the_look() {
        let look = look();
        let g = blend(
            [(&coast(), 1.0), (&forest(), 0.0)],
            &TimeGrades::shipped(),
            NOON,
            1.0,
            &look,
        );
        let c = coast();
        assert!((g.global.temperature - c.temperature).abs() < 1e-7);
        assert!((g.global.post_saturation - look.saturation * c.saturation).abs() < 1e-6);
        assert!((g.midtones.contrast - look.contrast * c.contrast).abs() < 1e-6);
        assert!((g.shadows.contrast - look.contrast * c.contrast).abs() < 1e-6);
        assert!((g.highlights.contrast - 1.0).abs() < 1e-7);
    }

    #[test]
    fn the_midpoint_of_two_biomes_is_their_mean() {
        let look = RenderLook::default();
        let g = blend(
            [(&coast(), 0.5), (&forest(), 0.5)],
            &TimeGrades::shipped(),
            NOON,
            1.0,
            &look,
        );
        let (a, b) = (coast(), forest());
        assert!((g.global.temperature - f32::midpoint(a.temperature, b.temperature)).abs() < 1e-7);
        assert!((g.global.tint - f32::midpoint(a.tint, b.tint)).abs() < 1e-7);
        assert!(
            (g.global.post_saturation - f32::midpoint(a.saturation, b.saturation)).abs() < 1e-6
        );
        assert!((g.shadows.lift - f32::midpoint(a.lift, b.lift)).abs() < 1e-7);
        // Shares that do not sum to one are still a mean.
        let h = blend(
            [(&coast(), 0.4), (&forest(), 0.4)],
            &TimeGrades::shipped(),
            NOON,
            1.0,
            &look,
        );
        assert!(same(&g, &h));
    }

    #[test]
    fn night_crushes_saturation_and_turns_blue() {
        let midnight = DayCycle::default().sun_direction(0.0);
        let w = time_weights(midnight);
        assert!((w[3] - 1.0).abs() < 1e-6, "{w:?}");
        let look = RenderLook::default();
        let night = blend(
            [(&forest(), 1.0), (&coast(), 0.0)],
            &TimeGrades::shipped(),
            w,
            1.0,
            &look,
        );
        let noon = blend(
            [(&forest(), 1.0), (&coast(), 0.0)],
            &TimeGrades::shipped(),
            NOON,
            1.0,
            &look,
        );
        assert!(night.global.post_saturation < 0.85 * noon.global.post_saturation);
        assert!(night.global.temperature < 0.0);
    }

    #[test]
    fn time_weights_sum_to_one_and_split_dawn_from_dusk() {
        let cycle = DayCycle::default();
        for i in 0..=96 {
            let w = time_weights(cycle.sun_direction(f64::from(i) / 96.0));
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{i}: {w:?}");
            assert!(w.iter().all(|v| *v >= 0.0), "{i}: {w:?}");
        }
        let noon = time_weights(cycle.sun_direction(0.5));
        assert!((noon[1] - 1.0).abs() < 1e-6, "{noon:?}");
        let morning = time_weights(cycle.sun_direction(0.26));
        assert!(morning[0] > 0.5 && morning[2] < 1e-6, "{morning:?}");
        // The dusk street's fixture, 16:48 with the sun 15 degrees up.
        let evening = time_weights(cycle.sun_direction(0.70));
        assert!(
            evening[2] > 0.5 && evening[0] < 1e-6 && evening[3] < 1e-6,
            "{evening:?}"
        );
    }

    #[test]
    fn the_camera_takes_the_grade_over_the_look_and_eases_across_a_border() {
        use bevy::camera::Camera3d;
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;

        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(look())
            .init_resource::<crate::SkySettings>()
            .init_resource::<StudySettings>()
            .init_resource::<DayCycle>()
            .insert_resource(TimeOfDay(0.5))
            .init_resource::<Observers>()
            .add_systems(Update, crate::look::apply_cameras);
        register(&mut app);
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        // No world: the time's grade alone (neutral at noon), so the look's.
        let g = app.world().get::<ColorGrading>(camera).expect("graded");
        assert!((g.global.post_saturation - 0.92).abs() < 1e-6, "{g:?}");
        assert!((g.midtones.contrast - 1.12).abs() < 1e-6, "{g:?}");

        // A grade under the observer is eased in, not popped.
        {
            let mut state = app.world_mut().resource_mut::<BiomeGrade>();
            state.target = forest();
        }
        app.update();
        let g = app.world().get::<ColorGrading>(camera).expect("graded");
        let f = forest();
        assert!(
            g.global.temperature > 0.0 && g.global.temperature < 0.5 * f.temperature,
            "{g:?}"
        );
        for _ in 0..40 {
            app.update();
        }
        let g = app.world().get::<ColorGrading>(camera).expect("graded");
        assert!((g.global.temperature - f.temperature).abs() < 1e-4, "{g:?}");
        assert!(
            (g.global.post_saturation - 0.92 * f.saturation).abs() < 1e-3,
            "{g:?}"
        );

        // At strength 0 the look's own grading comes back.
        app.world_mut().resource_mut::<RenderLook>().grade = 0.0;
        app.update();
        let g = app.world().get::<ColorGrading>(camera).expect("graded");
        assert!(g.global.temperature.abs() < 1e-9, "{g:?}");
        assert!((g.global.post_saturation - 0.92).abs() < 1e-6, "{g:?}");
    }

    #[test]
    fn the_shade_tint_has_luminance_one_and_neutral_grades_leave_it_white() {
        let neutral = shade_tint(&Grade::NEUTRAL, &Grade::NEUTRAL, 1.0);
        assert!((neutral - Vec3::ONE).abs().max_element() < 1e-6);
        let violet = Grade {
            shade: [1.0, 0.93, 1.10],
            ..Grade::NEUTRAL
        };
        let tint = shade_tint(&violet, &Grade::NEUTRAL, 1.0);
        assert!((tint.dot(LUMA) - 1.0).abs() < 1e-5);
        assert!(tint.z > tint.y, "blue over green: {tint}");
        // Strength zero is the look's own, untinted shade.
        let off = shade_tint(&violet, &Grade::NEUTRAL, 0.0);
        assert!((off - Vec3::ONE).abs().max_element() < 1e-6);
    }

    #[test]
    fn shipped_shades_are_valid_and_a_shade_alone_writes_no_grade() {
        let times = TimeGrades::shipped();
        for (_, g) in times.keys() {
            assert!(g.invalid_field().is_none());
        }
        let only = Grade {
            shade: [1.0, 0.9, 1.1],
            ..Grade::NEUTRAL
        };
        assert!(only.is_neutral());
    }
}
