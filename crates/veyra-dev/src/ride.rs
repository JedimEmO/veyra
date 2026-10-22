//! `--mounted` and `--script-ride` (ticket 0009, step 2): the bike the
//! household owns (`--bike`, see `household`), starting on it, and the
//! scripted ride along Port Serein's road that measures it, from the road's
//! far end in (`--at road`, the default of the script).
//!
//! The script is a client of the sim like any other: it puts a
//! [`RideRequest::Call`] in the inbox, waits out the channel, and then
//! steers by writing the player's [`ScriptedIntent`] to follow the road's
//! line, so what it measures is the sim's own bike on the real ground,
//! streamed as it would be in play.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::Serialize;
use strata::physics::character::CharacterController as Controller;
use strata::prelude::*;
use strata::sim::Registries;
use strata::sim::Settlements;
use strata::sim::progression::CharacterSheet;
use strata::sim::ride::{
    self, Calling, RideInbox, RideNotice, RideRequest, Riding, Stable, ThrownWhy,
};

use crate::DevArgs;

/// How long the ride is, played seconds.
const RIDE_SECONDS: f32 = 40.0;
/// How long after the mount the first stretch lasts, which a stall is not
/// counted in: the bike is gathering speed.
const GATHER_SECONDS: f32 = 5.0;
/// Below this a bike counts as stopped, metres a second.
const STALLED_BELOW: f64 = 2.0;
/// How far ahead on the road the script aims: a fixed stretch and a share of
/// the speed, metres and seconds, as a driver looks further ahead the faster
/// they go.
const LOOK_AHEAD_M: f64 = 25.0;
const LOOK_AHEAD_S: f64 = 0.6;
/// How near a vertex counts as reached: a fixed stretch and a share of the
/// speed, metres and seconds, since a bike at road speed cannot turn inside a
/// tight circle and would wheel round a corner it never touches.
const ADVANCE_M: f64 = 12.0;
const ADVANCE_S: f64 = 0.5;
/// How near the road's far end counts as reached, metres. A bike at road
/// speed overshoots a tighter circle and wheels round the end for the rest of
/// the ride, which is no road speed and risks a crash.
const END_REACHED_M: f64 = 80.0;
/// The ground under the player settles for this long before the call.
const SETTLE_SECONDS: f32 = 3.0;

/// What `--script-ride` measured.
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct RideReport {
    /// The call was answered with a mount.
    pub mounted: bool,
    /// Played seconds on the bike.
    pub seconds: f64,
    /// Metres covered over the ground.
    pub distance_m: f64,
    /// `distance_m` over `seconds`, metres a second.
    pub mean_mps: f64,
    /// The fastest tick, metres a second.
    pub top_mps: f64,
    /// Seconds below 2 m/s after the first five on the bike.
    pub stalled_s: f64,
    /// The longest unbroken stall, seconds.
    pub longest_stall_s: f64,
    /// Times the rider was thrown by a crash.
    pub crashes: u32,
    /// Times the rider was thrown for any reason.
    pub thrown: u32,
    /// The rider reached the road's far end.
    pub reached_end: bool,
}

/// Where `--script-ride` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for ground under the player.
    Settle,
    /// The call is made, the channel under way.
    Calling,
    /// On the bike.
    Riding,
    /// Written; the run may exit.
    Done,
}

/// The script's state and what it has measured.
#[derive(Resource, Debug)]
pub struct ScriptRide {
    stage: Stage,
    since: f32,
    asked: bool,
    /// Where the report is written.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "native files only"))]
    out: Option<PathBuf>,
    /// The line to follow: the road from its far end in.
    line: Vec<(f64, f64)>,
    /// The segment of `line` the rider is on.
    seg: usize,
    ticks: u32,
    last: Option<(f64, f64)>,
    stalled_run: f64,
    report: RideReport,
}

impl ScriptRide {
    fn new(out: Option<PathBuf>) -> Self {
        Self {
            stage: Stage::Settle,
            since: 0.0,
            asked: false,
            out,
            line: Vec::new(),
            seg: 0,
            ticks: 0,
            last: None,
            stalled_run: 0.0,
            report: RideReport::default(),
        }
    }

    /// Whether the script has written its report.
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }
}

/// Installs the flags. The scripted ride runs on the real clock, a 60 Hz
/// sim tick a 60 Hz frame: the measure is whether the ground streams in
/// ahead of a bike at road speed, and a clock that outran the streamer
/// would measure the machine's load instead.
pub fn install(app: &mut App, cli: &DevArgs, look: Option<f32>) {
    if cli.mounted {
        // `--look` turns the camera and leaves the bike pointing north, so a
        // still can show it from the side; without it the bike points where
        // the player would have faced.
        app.insert_resource(MountHeading(look.map(|_| 0.0)))
            .add_systems(Update, mount_at_start);
    }
    if cli.script_ride {
        app.insert_resource(ScriptRide::new(cli.script_report.clone()))
            .add_systems(
                SimTick,
                drive_ride
                    .in_set(SimSet::Input)
                    .before(strata::sim::player::apply_scripted_intents),
            );
    }
}

/// The heading `--mounted` starts on, when `--look` has taken the camera's.
#[derive(Resource, Debug, Clone, Copy)]
struct MountHeading(Option<f32>);

/// `--mounted`: once the player and the household's stable exist, the
/// rider is on the bike, with no call.
fn mount_at_start(
    mut commands: Commands,
    heading: Res<MountHeading>,
    stable: Option<Res<Stable>>,
    registries: Option<Res<Registries>>,
    players: Query<
        (Entity, &PlayerIntent, Option<&CharacterSheet>),
        (With<LocalPlayer>, Without<Riding>),
    >,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let (Some(stable), Some(registries), Ok((player, intent, sheet))) =
        (stable, registries, players.single())
    else {
        return;
    };
    *done = true;
    let Some((def, bike)) = stable.vehicle(&registries.0.items, &registries.0.vehicles) else {
        warn!("--mounted: the household owns no bike");
        return;
    };
    let dials = ride::dials(
        def,
        bike,
        registries.0.items.wear(),
        &registries.0.progression,
        sheet.unwrap_or(&CharacterSheet::default()),
    );
    commands.entity(player).insert(Riding::with_dials(
        def,
        dials,
        heading.0.unwrap_or(intent.yaw),
    ));
}

/// The point `along` metres further down `line` than the rider's
/// projection on segment `seg`, and the rider's distance to the line's end.
fn ahead(line: &[(f64, f64)], seg: usize, at: (f64, f64), along: f64) -> ((f64, f64), f64) {
    let project = |i: usize| {
        let (a, b) = (line[i], line[i + 1]);
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let len2 = (dx * dx + dz * dz).max(1e-9);
        let t = (((at.0 - a.0) * dx + (at.1 - a.1) * dz) / len2).clamp(0.0, 1.0);
        (a.0 + dx * t, a.1 + dz * t)
    };
    let mut point = project(seg);
    let mut left = along;
    let mut i = seg;
    loop {
        let end = line[i + 1];
        let reach = ((end.0 - point.0).powi(2) + (end.1 - point.1).powi(2)).sqrt();
        if reach >= left || i + 2 >= line.len() {
            let t = if reach > 1e-9 {
                (left / reach).min(1.0)
            } else {
                1.0
            };
            return (
                (
                    point.0 + (end.0 - point.0) * t,
                    point.1 + (end.1 - point.1) * t,
                ),
                0.0,
            );
        }
        left -= reach;
        point = end;
        i += 1;
    }
}

/// `SimTick`, `SimSet::Input`: the scripted ride.
#[allow(
    clippy::too_many_arguments,
    reason = "the world, the sim's inbox and the player in; an intent and a report out"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one state machine, read top to bottom"
)]
fn drive_ride(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut script: ResMut<ScriptRide>,
    settlements: Option<Res<Settlements>>,
    mut inbox: ResMut<RideInbox>,
    mut notices: MessageReader<RideNotice>,
    mut players: Query<
        (
            Entity,
            &WorldPosition,
            &Controller,
            Option<&Riding>,
            Option<&Calling>,
            Option<&mut ScriptedIntent>,
        ),
        With<LocalPlayer>,
    >,
) {
    let dt = time.delta_secs();
    let Ok((player, at, controller, riding, calling, intent)) = players.single_mut() else {
        return;
    };
    let Some(mut intent) = intent else {
        commands.entity(player).insert(ScriptedIntent::default());
        return;
    };
    let Some(settlement) = settlements.as_ref().and_then(|s| s.0.first()) else {
        return;
    };
    if script.line.is_empty() {
        // From the road's far end in, as the bench's route walks it: the
        // street's own approach to the road's city end crosses 560 m of
        // terraces and buildings that no straight line follows.
        script.line.extend(
            settlement
                .site
                .road
                .points
                .iter()
                .rev()
                .map(|p| (p[0], p[1])),
        );
    }
    let here = (at.0.x(), at.0.z());
    let speed_now = script.last.map_or(0.0, |last| {
        ((here.0 - last.0).hypot(here.1 - last.1)) / f64::from(dt)
    });
    let seg = script.seg.min(script.line.len() - 2);
    // Advance to the next segment once its projection is nearer.
    if seg + 2 < script.line.len() {
        let a = script.line[seg + 1];
        if (here.0 - a.0).hypot(here.1 - a.1) < ADVANCE_M + ADVANCE_S * speed_now {
            script.seg = seg + 1;
        }
    }
    let seg = script.seg.min(script.line.len() - 2);
    let look = LOOK_AHEAD_M + LOOK_AHEAD_S * speed_now;
    let (target, _) = ahead(&script.line, seg, here, look);
    #[allow(clippy::cast_possible_truncation, reason = "a heading in radians")]
    let yaw = (-(target.0 - here.0)).atan2(-(target.1 - here.1)) as f32;
    let end = *script.line.last().expect("the line has points");
    let to_end = (here.0 - end.0).hypot(here.1 - end.1);
    script.since += dt;

    match script.stage {
        Stage::Settle => {
            intent.0.yaw = yaw;
            intent.0.move_dir = Vec2::ZERO;
            if script.since >= SETTLE_SECONDS && controller.grounded {
                script.stage = Stage::Calling;
                script.since = 0.0;
            }
        }
        Stage::Calling => {
            intent.0.yaw = yaw;
            intent.0.move_dir = Vec2::ZERO;
            if riding.is_some() {
                script.stage = Stage::Riding;
                script.since = 0.0;
                script.report.mounted = true;
                script.last = Some(here);
            } else if !script.asked {
                inbox.push(RideRequest::Call);
                script.asked = true;
            } else if calling.is_none() && script.since > 0.5 {
                error!("--script-ride: the call was refused or cancelled");
                finish(&mut script);
            }
        }
        Stage::Riding => {
            intent.0.yaw = yaw;
            intent.0.move_dir = Vec2::Y;
            let step = script
                .last
                .map_or(0.0, |last| (here.0 - last.0).hypot(here.1 - last.1));
            script.last = Some(here);
            let mps = step / f64::from(dt);
            script.report.distance_m += step;
            script.report.top_mps = script.report.top_mps.max(mps);
            if script.since > GATHER_SECONDS && mps < STALLED_BELOW {
                script.stalled_run += f64::from(dt);
                script.report.stalled_s += f64::from(dt);
                script.report.longest_stall_s =
                    script.report.longest_stall_s.max(script.stalled_run);
            } else {
                script.stalled_run = 0.0;
            }
            for notice in notices.read() {
                if let RideNotice::Thrown { why, speed, .. } = notice {
                    script.report.thrown += 1;
                    if *why == ThrownWhy::Crash {
                        script.report.crashes += 1;
                    }
                    warn!("--script-ride: thrown ({why:?}) at {speed:.1} m/s");
                }
            }
            script.ticks += 1;
            if script.ticks.is_multiple_of(120) {
                debug!(
                    "--script-ride: t {:.1} at ({:.1}, {:.1}) {mps:.1} m/s seg {} aiming ({:.1}, {:.1}) heading {:.2}",
                    script.since, here.0, here.1, script.seg, target.0, target.1, yaw
                );
            }
            script.report.seconds = f64::from(script.since);
            script.report.reached_end = to_end < END_REACHED_M;
            if script.since >= RIDE_SECONDS || script.report.reached_end || riding.is_none() {
                intent.0.move_dir = Vec2::ZERO;
                finish(&mut script);
            }
        }
        Stage::Done => {
            intent.0.move_dir = Vec2::ZERO;
        }
    }
    if script.stage != Stage::Riding {
        script.last = Some(here);
    }
}

/// Fills the means, logs the report and writes it.
fn finish(script: &mut ScriptRide) {
    let report = &mut script.report;
    report.mean_mps = if report.seconds > 0.0 {
        report.distance_m / report.seconds
    } else {
        0.0
    };
    info!("--script-ride: {report:?}");
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &script.out {
        match ron::ser::to_string_pretty(&script.report, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(error) = std::fs::write(path, text) {
                    error!("--script-ride: {}: {error}", path.display());
                }
            }
            Err(error) => error!("--script-ride: the report does not serialise: {error}"),
        }
    }
    script.stage = Stage::Done;
}
