//! Exposure and the lens (lighting plan phase 4b): Bevy's histogram auto
//! exposure as a bounded correction on the manual curve, a resting vignette
//! and the chromatic aberration the client drives on a hurt.
//!
//! The manual curve ([`SkySettings::ev100`] lerped towards the night's by
//! `adapt_exposure`) stays on the camera in both modes. Bevy's PBR, sky and
//! fog shaders multiply by that `Exposure` before they write the HDR target,
//! so the histogram Bevy's auto exposure builds is of light already exposed
//! by the manual curve, and what it adds to the colour grading's exposure is
//! a correction on top of it. Its target is fixed by Bevy: it moves the
//! metered mean log2 luminance to zero plus a compensation curve's value.
//! [`compensation_points`] is that curve, built so the correction is zero
//! while the metered mean is at the look's key
//! ([`RenderLook::auto_exposure_key`]) and follows the scene one stop for
//! one stop until it reaches the look's band
//! ([`RenderLook::auto_exposure_range_ev`]), where it holds. The histogram
//! itself spans sixteen stops round the key, so a forest floor below the
//! band still counts as dark instead of falling into Bevy's zeroth bin,
//! which its mean leaves out.
//!
//! The key is a daylight scene's, so by night the brightening end of the
//! band closes with [`Night::darkness`](crate::Night) down to the look's
//! [`RenderLook::auto_exposure_night`] share of it ([`night_band`]): a dark
//! wood at midnight is the manual night curve's, not a cloudy morning's.
//!
//! [`SkySettings::ev100`]: crate::SkySettings::ev100

use crate::look::RenderLook;
use crate::{Night, NoSky, SkyCamera};
use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle};
use bevy::camera::Camera3d;
use bevy::color::Color;
use bevy::diagnostic::{Diagnostic, DiagnosticsStore};
use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use bevy::math::cubic_splines::LinearSpline;
use bevy::post_process::auto_exposure::{
    AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin,
};
/// Re-exported for the client, which writes its intensity from the hurt
/// and otherwise names nothing of Bevy's post-processing.
pub use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::post_process::effect_stack::Vignette;
use bevy::render::RenderApp;
use serde::{Deserialize, Serialize};

/// Whether the camera's exposure is the manual curve alone or the curve
/// with Bevy's auto exposure correcting it inside a band.
///
/// Serialised as its lowercase name in a string, like the shadow filter,
/// so a misspelt mode is a load error rather than a different look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ExposureMode {
    /// The manual EV curve by the sun's height, the look the slice was
    /// tuned under.
    #[default]
    Manual,
    /// The manual curve plus a histogram-metered correction, bounded by
    /// [`RenderLook::auto_exposure_range_ev`].
    Auto,
}

impl ExposureMode {
    /// Both modes, in the order a panel lists them.
    pub const ALL: [Self; 2] = [Self::Manual, Self::Auto];

    /// The name in a look file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

impl From<ExposureMode> for String {
    fn from(mode: ExposureMode) -> Self {
        mode.as_str().into()
    }
}

impl TryFrom<String> for ExposureMode {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|m| m.as_str() == name)
            .ok_or_else(|| format!("unknown exposure mode {name:?}: manual or auto"))
    }
}

/// Stops of the histogram below the key: the shade under a crown sits two
/// to four stops under open ground, and anything darker than ten is black
/// on screen whichever way the exposure goes.
const HISTOGRAM_BELOW: f32 = 10.0;
/// Stops above the key: the sun's disc and its halo are far over, and the
/// percentile filter drops the brightest tenth before the mean anyway.
const HISTOGRAM_ABOVE: f32 = 6.0;

/// The share of the frame's darkest and brightest pixels the mean leaves
/// out: Bevy's default, so a patch of sky through the canopy or the black
/// under a trunk does not swing the exposure.
const FILTER: std::ops::RangeInclusive<f32> = 0.10..=0.90;

/// Bevy's chromatic aberration intensity at a strength of 1: the streak at
/// the window's edge is this fraction of the window (Bevy's own default,
/// 25 px at 2560 wide), which at the look's 0.6 reads as a knock without
/// smearing the target in the middle, where the streak is nothing.
pub const ABERRATION_FULL: f32 = 0.02;

/// Taps per pixel along the aberration's streak, Bevy's default 8 halved.
/// The streak is at most a few texels at the look's strength, and the
/// effect stack pass with it on took 0.09 to 0.10 ms at 2560x1440 at 8
/// against 0.03 to 0.04 for the vignette alone.
const ABERRATION_SAMPLES: u32 = 4;

/// Bevy's chromatic aberration intensity for a strength from 0 to 1.
#[must_use]
pub fn aberration_intensity(strength: f32) -> f32 {
    strength.clamp(0.0, 1.0) * ABERRATION_FULL
}

/// The correction, in stops added to the image, that the auto exposure
/// settles on for a frame whose metered mean log2 luminance is `metered`:
/// the key minus the mean, held inside the band. The band is in EV, where
/// up is darker, so its low end is the most the image is brightened.
#[must_use]
pub fn correction(key: f32, range_ev: [f32; 2], metered: f32) -> f32 {
    (key - metered).clamp(-range_ev[1], -range_ev[0])
}

/// Steps the night's closing of the band is rounded to, so dusk rebuilds
/// the compensation curve (a 256-texel asset) sixteen times, not every
/// frame the sun moves.
const NIGHT_STEPS: f32 = 16.0;

/// The band auto exposure works in at a [`Night::darkness`](crate::Night)
/// of `darkness`: the look's band by day, its brightening end scaled down
/// towards `night_share` of itself at full darkness, rounded to sixteenths
/// of the way. The darkening end is kept: a lit square at night that
/// meters over the key is still held to the manual curve by it.
#[must_use]
pub fn night_band(range_ev: [f32; 2], night_share: f32, darkness: f32) -> [f32; 2] {
    let open = 1.0 - darkness.clamp(0.0, 1.0) * (1.0 - night_share.clamp(0.0, 1.0));
    let open = (open * NIGHT_STEPS).round() / NIGHT_STEPS;
    [range_ev[0] * open, range_ev[1]]
}

/// What [`compensation_points`] adds to every value so Bevy's storage
/// rounds rather than truncates.
///
/// Bevy keeps the curve as 256 entries of 8 bits and reads both by
/// truncation: an entry stands for the mean at its left end and holds its
/// value rounded down. On the curve's one-for-one parts, where a scene
/// brighter than the key is to keep the manual exposure, that settled
/// 0.05 stops darker than the manual curve (2 of 255 on the look
/// fixtures, 2026-10-03). Half a step of each makes both a rounding.
#[must_use]
pub fn rounding_offset(range_ev: [f32; 2]) -> f32 {
    let x_span = HISTOGRAM_BELOW + HISTOGRAM_ABOVE;
    let y_span = x_span - range_ev[1] + range_ev[0];
    (x_span + y_span) / 255.0 / 2.0
}

/// The compensation curve's points, as (metered mean, compensation): Bevy
/// settles on `compensation - mean`, so a compensation of `mean +
/// correction` is [`correction`], plus the [`rounding_offset`]. Linear
/// between them, flat at the key inside the band and one for one outside
/// it; the band's two ends coincide when it is closed and one is dropped,
/// since the spline's x has to increase.
#[must_use]
pub fn compensation_points(key: f32, range_ev: [f32; 2]) -> Vec<Vec2> {
    let low = key - HISTOGRAM_BELOW;
    let high = key + HISTOGRAM_ABOVE;
    let offset = rounding_offset(range_ev);
    let mut points: Vec<Vec2> = Vec::with_capacity(4);
    for x in [low, key + range_ev[0], key + range_ev[1], high] {
        let point = Vec2::new(x, x + correction(key, range_ev, x) + offset);
        if points.last().is_none_or(|last| x > last.x + 1e-4) {
            points.push(point);
        }
    }
    points
}

/// The auto exposure the look asks for, given its compensation curve.
fn auto_exposure(look: &RenderLook, curve: Handle<AutoExposureCompensationCurve>) -> AutoExposure {
    let key = look.auto_exposure_key;
    AutoExposure {
        range: (key - HISTOGRAM_BELOW)..=(key + HISTOGRAM_ABOVE),
        filter: FILTER,
        speed_brighten: look.auto_exposure_speed_brighten,
        speed_darken: look.auto_exposure_speed_darken,
        compensation_curve: curve,
        ..Default::default()
    }
}

/// The resting vignette, from the look.
fn vignette(look: &RenderLook) -> Vignette {
    Vignette {
        intensity: look.vignette,
        radius: look.vignette_radius,
        smoothness: look.vignette_smoothness,
        color: Color::BLACK,
        ..Default::default()
    }
}

/// What [`apply_lens`] last gave a camera, so an unchanged look touches
/// nothing; the aberration's live intensity is the client's and not here.
#[derive(Component, Clone, PartialEq)]
pub(crate) struct AppliedLens {
    mode: ExposureMode,
    curve: [f32; 3],
    speeds: [f32; 2],
    vignette: [f32; 3],
    aberration: bool,
}

impl AppliedLens {
    fn of(look: &RenderLook, band: [f32; 2]) -> Self {
        Self {
            mode: look.exposure_mode,
            curve: [look.auto_exposure_key, band[0], band[1]],
            speeds: [
                look.auto_exposure_speed_brighten,
                look.auto_exposure_speed_darken,
            ],
            vignette: [
                look.vignette,
                look.vignette_radius,
                look.vignette_smoothness,
            ],
            aberration: look.aberration > 0.0,
        }
    }
}

pub(crate) fn register(app: &mut App) {
    // The plugin needs the asset server and the render world; a headless
    // app (the server, the tests) gets the vignette and aberration
    // components, which nothing draws, and no auto exposure.
    if app.get_sub_app(RenderApp).is_some() && !app.is_plugin_added::<AutoExposurePlugin>() {
        app.add_plugins(AutoExposurePlugin);
    }
    app.add_systems(Update, (apply_lens, log_gpu_time));
}

/// Gives every sky camera the look's exposure mode, vignette and an
/// aberration at rest, when the look or the night's band changes or a
/// camera is new.
///
/// Bevy keeps a view's adapted exposure across a changed `AutoExposure`
/// (it rewrites the settings and skips the state), so the band closing
/// through dusk moves the target without a jump.
///
/// The aberration is inserted at zero and its intensity left alone after
/// that: the client writes it every frame from the hurt, and Bevy skips the
/// effect at zero, so at rest it costs nothing beyond the vignette's pass.
#[allow(
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its dependencies"
)]
#[allow(
    clippy::float_cmp,
    reason = "the curve's inputs are copies of the look's: any difference is a change"
)]
pub(crate) fn apply_lens(
    mut commands: Commands,
    look: Res<RenderLook>,
    night: Option<Res<Night>>,
    curves: Option<ResMut<Assets<AutoExposureCompensationCurve>>>,
    mut curve: Local<Option<([f32; 3], Handle<AutoExposureCompensationCurve>)>>,
    cameras: Query<
        (Entity, Option<&AppliedLens>, Has<ChromaticAberration>),
        (With<Camera3d>, With<SkyCamera>, Without<NoSky>),
    >,
) {
    let band = night_band(
        look.auto_exposure_range_ev,
        look.auto_exposure_night,
        night.map_or(0.0, |n| n.darkness),
    );
    let wanted = AppliedLens::of(&look, band);
    let mut curves = curves;
    for (entity, applied, has_aberration) in &cameras {
        if applied == Some(&wanted) {
            continue;
        }
        let mut c = commands.entity(entity);
        match (look.exposure_mode, curves.as_deref_mut()) {
            // A closed band corrects nothing, so the night (at the default
            // share) drops the histogram pass, 0.11 ms at 2560x1440. Bevy
            // starts a returning view's correction at zero, which is where
            // a closed band had settled it, so dawn opens it without a jump.
            (ExposureMode::Auto, Some(curves)) if band != [0.0, 0.0] => {
                let handle = match &*curve {
                    Some((key, handle)) if *key == wanted.curve => handle.clone(),
                    _ => {
                        let built = AutoExposureCompensationCurve::from_curve(LinearSpline::new(
                            compensation_points(look.auto_exposure_key, band),
                        ));
                        let Ok(built) = built else {
                            tracing::warn!("auto exposure: the compensation curve did not build");
                            continue;
                        };
                        let handle = curves.add(built);
                        *curve = Some((wanted.curve, handle.clone()));
                        handle
                    }
                };
                c.insert(auto_exposure(&look, handle));
            }
            _ => {
                c.remove::<AutoExposure>();
            }
        }
        if look.vignette > 0.0 {
            c.insert(vignette(&look));
        } else {
            c.remove::<Vignette>();
        }
        if look.aberration <= 0.0 {
            c.remove::<ChromaticAberration>();
        } else if !has_aberration {
            c.insert(ChromaticAberration {
                intensity: 0.0,
                max_samples: ABERRATION_SAMPLES,
                ..Default::default()
            });
        }
        c.insert(wanted.clone());
    }
}

/// Every two seconds, the GPU time of the auto exposure's compute pass and
/// of Bevy's effect stack (the vignette and the aberration share one
/// full-screen pass), against the tonemapping pass for scale, when Bevy's
/// render diagnostics are on (`--stats`).
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
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let ms = |span: &str| {
        let suffix = format!("/{span}/elapsed_gpu");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&suffix))
            .and_then(Diagnostic::smoothed)
    };
    let (auto, stack, tonemapping) = (ms("auto_exposure"), ms("postprocessing"), ms("tonemapping"));
    if auto.is_none() && stack.is_none() {
        return;
    }
    let show = |v: Option<f64>| v.map_or_else(|| "-".to_owned(), |v| format!("{v:.3}"));
    tracing::info!(
        "lens passes, ms of GPU time: auto exposure {}, effect stack {}, tonemapping {}",
        show(auto),
        show(stack),
        show(tonemapping)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compensation the curve gives at `x`, linear between its points.
    fn compensation_at(points: &[Vec2], x: f32) -> f32 {
        let pair = points
            .windows(2)
            .find(|w| x >= w[0].x && x <= w[1].x)
            .expect("inside the curve");
        let t = (x - pair[0].x) / (pair[1].x - pair[0].x);
        pair[0].y + (pair[1].y - pair[0].y) * t
    }

    #[test]
    fn the_curve_is_the_manual_exposure_inside_the_band_and_holds_at_its_ends() {
        let key = -2.5;
        let band = [-1.5, 1.5];
        let points = compensation_points(key, band);
        assert_eq!(points.len(), 4);
        // Bevy settles on compensation minus the metered mean.
        for (metered, settled) in [
            (key, 0.0),
            (key - 1.0, 1.0),
            (key + 0.5, -0.5),
            (key - 4.0, 1.5),
            (key + 5.0, -1.5),
        ] {
            let got = compensation_at(&points, metered) - metered - rounding_offset(band);
            assert!((got - settled).abs() < 1e-5, "{metered}: {got}");
            assert!((correction(key, band, metered) - settled).abs() < 1e-6);
        }
        // An asymmetric band: brighten at most half a stop, darken two.
        let band = [-0.5, 2.0];
        let points = compensation_points(key, band);
        let offset = rounding_offset(band);
        assert!((compensation_at(&points, key - 3.0) - (key - 3.0) - offset - 0.5).abs() < 1e-5);
        assert!((compensation_at(&points, key + 3.0) - (key + 3.0) - offset + 2.0).abs() < 1e-5);
        // Half an 8-bit step of a curve 16 stops wide, and of its values.
        assert!((rounding_offset([-1.5, 0.0]) - (16.0 + 14.5) / 510.0).abs() < 1e-6);
        // A closed band (auto exposure that never moves) keeps the
        // spline's x increasing.
        let points = compensation_points(key, [0.0, 0.0]);
        assert_eq!(points.len(), 3);
        assert!(points.windows(2).all(|w| w[1].x > w[0].x));
        assert!(AutoExposureCompensationCurve::from_curve(LinearSpline::new(points)).is_ok());
        assert!(
            AutoExposureCompensationCurve::from_curve(LinearSpline::new(compensation_points(
                key, band
            )))
            .is_ok()
        );
    }

    #[test]
    fn the_mode_is_a_named_string() {
        for mode in ExposureMode::ALL {
            assert_eq!(ExposureMode::try_from(String::from(mode)), Ok(mode));
        }
        assert!(ExposureMode::try_from("eye".to_owned()).is_err());
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "sixteenths of halves are exact in f32, so the band is too"
    )]
    fn the_brightening_end_closes_by_night_and_the_darkening_end_is_kept() {
        let band = [-1.5, 0.5];
        assert_eq!(night_band(band, 0.0, 0.0), band);
        assert_eq!(night_band(band, 0.0, 1.0), [0.0, 0.5]);
        assert_eq!(night_band(band, 1.0, 1.0), band);
        assert_eq!(night_band(band, 0.5, 1.0), [-0.75, 0.5]);
        // Rounded to sixteenths, so dusk is a handful of curves.
        assert_eq!(night_band(band, 0.0, 0.5), [-0.75, 0.5]);
        assert_eq!(night_band(band, 0.0, 0.51), [-0.75, 0.5]);
        // A closed band at night still builds a curve Bevy accepts, which
        // leaves every scene to the manual curve.
        let night = night_band([-1.5, 0.0], 0.0, 1.0);
        let points = compensation_points(-4.0, night);
        assert!(AutoExposureCompensationCurve::from_curve(LinearSpline::new(points)).is_ok());
        for metered in [-9.0, -4.0, -1.0] {
            assert!(correction(-4.0, night, metered).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn the_night_closes_the_band_on_the_camera() {
        let mut app = App::new();
        app.init_resource::<RenderLook>()
            .init_resource::<Night>()
            .init_resource::<Assets<AutoExposureCompensationCurve>>()
            .add_systems(Update, apply_lens);
        app.world_mut().resource_mut::<RenderLook>().exposure_mode = ExposureMode::Auto;
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        let day = app
            .world()
            .get::<AppliedLens>(camera)
            .expect("applied")
            .curve;
        assert!((day[1] - RenderLook::default().auto_exposure_range_ev[0]).abs() < 1e-6);
        app.world_mut().resource_mut::<Night>().darkness = 1.0;
        app.update();
        let night = app
            .world()
            .get::<AppliedLens>(camera)
            .expect("applied")
            .curve;
        assert!(night[1].abs() < 1e-6, "{night:?}");
        // Closed, the pass is dropped; the dawn brings it back.
        assert!(app.world().get::<AutoExposure>(camera).is_none());
        app.world_mut().resource_mut::<Night>().darkness = 0.0;
        app.update();
        assert!(app.world().get::<AutoExposure>(camera).is_some());
    }

    #[test]
    fn the_aberration_is_scaled_and_clamped() {
        assert!(aberration_intensity(0.0).abs() < f32::EPSILON);
        assert!((aberration_intensity(1.0) - ABERRATION_FULL).abs() < f32::EPSILON);
        assert!((aberration_intensity(4.0) - ABERRATION_FULL).abs() < f32::EPSILON);
    }

    #[test]
    fn cameras_get_the_vignette_a_resting_aberration_and_auto_exposure_only_with_render() {
        let mut app = App::new();
        app.init_resource::<RenderLook>()
            .add_systems(Update, apply_lens);
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        let excluded = app
            .world_mut()
            .spawn((Camera3d::default(), SkyCamera, NoSky))
            .id();
        app.update();
        let c = app.world().entity(camera);
        let v = c.get::<Vignette>().expect("vignette");
        assert!((v.intensity - RenderLook::default().vignette).abs() < f32::EPSILON);
        let a = c.get::<ChromaticAberration>().expect("aberration");
        assert!(a.intensity.abs() < f32::EPSILON);
        assert!(!c.contains::<AutoExposure>());
        assert!(!app.world().entity(excluded).contains::<Vignette>());
        // The client's live intensity is not reset by an unrelated change.
        app.world_mut()
            .get_mut::<ChromaticAberration>(camera)
            .expect("aberration")
            .intensity = 0.01;
        app.world_mut().resource_mut::<RenderLook>().vignette = 0.3;
        app.update();
        let c = app.world().entity(camera);
        assert!((c.get::<ChromaticAberration>().expect("kept").intensity - 0.01).abs() < 1e-6);
        assert!((c.get::<Vignette>().expect("vignette").intensity - 0.3).abs() < 1e-6);
        // Zero takes each away.
        {
            let mut look = app.world_mut().resource_mut::<RenderLook>();
            look.vignette = 0.0;
            look.aberration = 0.0;
            // Without the render world's curve assets the auto mode is a
            // no-op, not a panic.
            look.exposure_mode = ExposureMode::Auto;
        }
        app.update();
        let c = app.world().entity(camera);
        assert!(!c.contains::<Vignette>() && !c.contains::<ChromaticAberration>());
        assert!(!c.contains::<AutoExposure>());
    }

    #[test]
    fn auto_mode_puts_the_band_on_the_camera() {
        let mut app = App::new();
        app.init_resource::<RenderLook>()
            .init_resource::<Assets<AutoExposureCompensationCurve>>()
            .add_systems(Update, apply_lens);
        app.world_mut().resource_mut::<RenderLook>().exposure_mode = ExposureMode::Auto;
        let camera = app.world_mut().spawn((Camera3d::default(), SkyCamera)).id();
        app.update();
        let look = RenderLook::default();
        let auto = app
            .world()
            .get::<AutoExposure>(camera)
            .expect("auto exposure");
        assert!((auto.range.start() - (look.auto_exposure_key - HISTOGRAM_BELOW)).abs() < 1e-6);
        assert!((auto.speed_darken - look.auto_exposure_speed_darken).abs() < 1e-6);
        assert!(
            app.world()
                .resource::<Assets<AutoExposureCompensationCurve>>()
                .get(&auto.compensation_curve)
                .is_some()
        );
        app.world_mut().resource_mut::<RenderLook>().exposure_mode = ExposureMode::Manual;
        app.update();
        assert!(app.world().get::<AutoExposure>(camera).is_none());
    }
}
