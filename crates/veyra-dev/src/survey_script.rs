//! `--script-survey [class]` (ticket 0009, step 3): a member finds a deposit
//! by pings alone and samples it.
//!
//! The run starts on dry ground a few hundred metres from the nearest
//! deposit of the class (`deposits::survey_start`, the set-up and not the
//! script), looking along the heading the script will keep. It walks that
//! heading, pings every [`LEG_M`] metres until a ping meets something, then
//! walks to the lead (the strongest point of the last ping) and pings
//! again, and repeats until a ping's strongest point is under its feet or
//! within [`ARRIVED_M`] of them, and samples [`SAMPLE_UNITS`]. Nothing here
//! reads where the deposit is: the script is a client of the sim like any
//! other, and sees what a player's screen would, the ping's result.
//!
//! What it did goes to the log and to `--script-report`.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use strata::physics::character::CharacterController as Controller;
use strata::prelude::*;
use strata::registry::Namespaced;
use strata::registry::progression::XpKind;
use strata::sim::Registries;
use strata::sim::deposits::{
    PingAnswer, PingDone, SampleAnswer, SampleDone, SurveyInbox, SurveyRequest,
};
use strata::sim::items::Pack;
use strata::sim::progression::CharacterSheet;

use crate::DevArgs;

/// Metres walked between pings while nothing has been met.
pub const LEG_M: f64 = 100.0;
/// A lead within this many metres of the player is the player's own place:
/// the ping's strongest point is where they stand.
pub const ARRIVED_M: f64 = 20.0;
/// How near a lead the walk to it ends, metres.
pub const AT_LEAD_M: f64 = 3.0;
/// The units sampled before the script stops.
pub const SAMPLE_UNITS: u32 = 15;
/// The ground under the player settles for this long before the first ping.
pub const SETTLE_SECONDS: f32 = 3.0;
/// Pings after which the script gives up: a lead is followed monotonically
/// to a peak, so this is many times what it takes.
pub const MOST_PINGS: u32 = 40;
/// Played seconds after which the script gives up.
pub const MOST_SECONDS: f32 = 600.0;
/// A walk to a lead that takes longer than this, played seconds, is given up
/// and pinged from where it is: something stands in the way.
pub const LEG_SECONDS: f32 = 60.0;

/// What `--script-survey` did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SurveyReport {
    /// Pings made.
    pub pings: u32,
    /// Metres walked over the ground.
    pub walked_m: f64,
    /// Played seconds from the script's start to its last unit.
    pub seconds: f64,
    /// The deposit sampled, as `3,-2`; empty when none was met.
    pub deposit: String,
    /// Its place, `Calder flats`.
    pub place: String,
    /// Its concentration at its centre, per-mille.
    pub peak_pm: u16,
    /// The concentration where it was sampled, per-mille.
    pub sampled_pm: u16,
    /// Units sampled.
    pub units: u32,
    /// The run got its units.
    pub ok: bool,
    /// Survey XP the run earned, from the sheet's own ledger.
    #[serde(default)]
    pub survey_xp: u32,
    /// The reach of the member's first ping, metres, as the sim's ping
    /// result measured it.
    #[serde(default)]
    pub reach_m: f64,
    /// Units a sampling cycle gives them at 1000 per-mille: the skill's own
    /// figure, the same for any ground.
    #[serde(default)]
    pub units_cycle: u32,
}

/// Where the script is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for ground under the player.
    Settle,
    /// A ping is asked for or under way.
    Pinging,
    /// Walking the heading, nothing met yet.
    Searching,
    /// Walking to the last ping's lead.
    Homing,
    /// Sampling.
    Sampling,
    /// Stop is sent; the lot is in the pack.
    Stopping,
    /// Written; the run may exit.
    Done,
}

/// The script's state and what it has measured.
#[derive(Resource, Debug)]
pub struct ScriptSurvey {
    stage: Stage,
    class: Namespaced,
    /// The heading kept while nothing is met: the player's own at the start.
    yaw: Option<f32>,
    /// Played seconds since the script began, and in the stage.
    total: f32,
    since: f32,
    asked: bool,
    /// Metres walked on this leg.
    leg_m: f64,
    last: Option<(f64, f64)>,
    /// Where the walk to the lead ends.
    lead: Option<(f64, f64)>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "native files only"))]
    out: Option<PathBuf>,
    /// The sheet's Survey XP when the script began, so the report is what
    /// the run earned and not what the member arrived with.
    xp_from: Option<u32>,
    report: SurveyReport,
}

impl ScriptSurvey {
    fn new(class: Namespaced, out: Option<PathBuf>) -> Self {
        Self {
            stage: Stage::Settle,
            class,
            yaw: None,
            total: 0.0,
            since: 0.0,
            asked: false,
            leg_m: 0.0,
            last: None,
            lead: None,
            out,
            xp_from: None,
            report: SurveyReport::default(),
        }
    }

    /// Whether the script has written its report.
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.since = 0.0;
        self.asked = false;
    }
}

pub use veyra_game::deposit_start::class_of;

/// Installs the script when it is asked for.
pub fn install(app: &mut App, cli: &DevArgs) {
    let Some(spec) = &cli.script_survey else {
        return;
    };
    let class = class_of(spec).unwrap_or_else(|error| panic!("--script-survey: {error}"));
    app.insert_resource(ScriptSurvey::new(class, cli.script_report.clone()))
        .add_systems(
            SimTick,
            drive_survey
                .in_set(SimSet::Input)
                .before(strata::sim::player::apply_scripted_intents),
        );
}

/// The heading that faces `to` from `from`: zero looks down `-z`.
#[allow(clippy::cast_possible_truncation, reason = "a heading in radians")]
pub fn facing(from: (f64, f64), to: (f64, f64)) -> f32 {
    (-(to.0 - from.0)).atan2(-(to.1 - from.1)) as f32
}

#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the sim's inbox and answers and the player in; an intent and a report out"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one state machine, read top to bottom"
)]
pub fn drive_survey(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    mut script: ResMut<ScriptSurvey>,
    mut inbox: ResMut<SurveyInbox>,
    mut pings: MessageReader<PingAnswer>,
    mut samples: MessageReader<SampleAnswer>,
    registries: Option<Res<Registries>>,
    mut players: Query<
        (
            Entity,
            &WorldPosition,
            &Controller,
            &PlayerIntent,
            Option<&mut ScriptedIntent>,
            &Pack,
            Option<&CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
) {
    let dt = time.delta_secs();
    let Ok((player, at, controller, live, intent, pack, sheet)) = players.single_mut() else {
        return;
    };
    let Some(mut intent) = intent else {
        // Looking where the start looks: the heading the script keeps.
        commands.entity(player).insert(ScriptedIntent(PlayerIntent {
            yaw: live.yaw,
            ..default()
        }));
        return;
    };
    if let Some(sheet) = sheet {
        let now = sheet.xp(XpKind::Survey);
        let from = *script.xp_from.get_or_insert(now);
        script.report.survey_xp = now - from;
        if script.report.units_cycle == 0
            && let Some(registries) = registries.as_deref()
            && let Some(skill) = strata::sim::deposits::skill_of(&registries.0, pack, sheet)
        {
            script.report.units_cycle = skill.units;
        }
    }
    let pinged: Vec<PingAnswer> = pings.read().cloned().collect();
    let cycles: Vec<SampleAnswer> = samples.read().copied().collect();
    let here = (at.0.x(), at.0.z());
    let yaw = *script.yaw.get_or_insert(intent.0.yaw);
    intent.0.yaw = yaw;
    intent.0.move_dir = Vec2::ZERO;
    if script.stage == Stage::Done {
        return;
    }
    script.total += dt;
    script.since += dt;
    let step = script
        .last
        .map_or(0.0, |last| (here.0 - last.0).hypot(here.1 - last.1));
    script.last = Some(here);
    if matches!(script.stage, Stage::Searching | Stage::Homing) {
        script.report.walked_m += step;
        script.leg_m += step;
    }
    if script.total > MOST_SECONDS || script.report.pings > MOST_PINGS {
        error!("--script-survey: given up after {:.0} s", script.total);
        finish(&mut script);
        return;
    }
    match script.stage {
        Stage::Settle => {
            if script.since >= SETTLE_SECONDS && controller.grounded {
                script.enter(Stage::Pinging);
            }
        }
        Stage::Pinging => {
            if !script.asked {
                script.asked = true;
                inbox.push(SurveyRequest::Ping {
                    class: script.class.clone(),
                });
            }
            for answer in &pinged {
                match &answer.outcome {
                    Ok(PingDone::Met(result)) => {
                        script.report.pings += 1;
                        if script.report.reach_m == 0.0 {
                            script.report.reach_m = result.reach_m();
                        }
                        match result.lead() {
                            None => {
                                info!(
                                    "--script-survey: ping {} met nothing; on along the heading",
                                    script.report.pings
                                );
                                script.leg_m = 0.0;
                                script.enter(Stage::Searching);
                            }
                            Some(lead) => {
                                let away = (lead.at[0] - here.0).hypot(lead.at[1] - here.1);
                                if let Some(met) = result
                                    .found
                                    .iter()
                                    .find(|met| met.deposit.id == lead.deposit)
                                {
                                    script.report.deposit =
                                        format!("{},{}", met.deposit.id.0[0], met.deposit.id.0[1]);
                                    script.report.place.clone_from(&met.deposit.place);
                                    script.report.peak_pm = met.deposit.peak;
                                }
                                info!(
                                    "--script-survey: ping {} met {} at {} per-mille, {away:.0} m to its strongest point",
                                    script.report.pings, script.report.place, lead.pm
                                );
                                if away <= ARRIVED_M {
                                    script.enter(Stage::Sampling);
                                } else {
                                    script.lead = Some((lead.at[0], lead.at[1]));
                                    script.leg_m = 0.0;
                                    script.enter(Stage::Homing);
                                }
                            }
                        }
                    }
                    Ok(PingDone::Started) => {}
                    Err(refusal) => {
                        error!("--script-survey: the ping was refused: {refusal}");
                        finish(&mut script);
                        return;
                    }
                }
            }
        }
        Stage::Searching => {
            intent.0.move_dir = Vec2::Y;
            if script.leg_m >= LEG_M {
                script.enter(Stage::Pinging);
                intent.0.move_dir = Vec2::ZERO;
            }
        }
        Stage::Homing => {
            let target = script.lead.unwrap_or(here);
            let away = (target.0 - here.0).hypot(target.1 - here.1);
            if away <= AT_LEAD_M || script.since > LEG_SECONDS {
                script.enter(Stage::Pinging);
            } else {
                intent.0.yaw = facing(here, target);
                intent.0.move_dir = Vec2::Y;
            }
        }
        Stage::Sampling => {
            if !script.asked {
                script.asked = true;
                inbox.push(SurveyRequest::Sample);
            }
            for answer in &cycles {
                match answer.outcome {
                    Ok(SampleDone::Started { pm, .. }) => script.report.sampled_pm = pm,
                    Ok(SampleDone::Cycle { total, .. }) => {
                        script.report.units = total;
                        if total >= SAMPLE_UNITS && script.stage == Stage::Sampling {
                            script.report.seconds = f64::from(script.total);
                            inbox.push(SurveyRequest::Stop);
                            script.enter(Stage::Stopping);
                        }
                    }
                    Err(refusal) => {
                        error!("--script-survey: the sample was refused: {refusal}");
                        finish(&mut script);
                        return;
                    }
                }
            }
        }
        Stage::Stopping => {
            // The stop is consumed in the next tick.
            if script.since > 0.2 {
                finish(&mut script);
            }
        }
        Stage::Done => {}
    }
}

/// Logs the report and writes it.
pub fn finish(script: &mut ScriptSurvey) {
    if script.report.seconds == 0.0 {
        script.report.seconds = f64::from(script.total);
    }
    script.report.ok = script.report.units >= SAMPLE_UNITS;
    let report = &script.report;
    info!("--script-survey: {report:?}");
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &script.out {
        match ron::ser::to_string_pretty(report, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(error) = std::fs::write(path, text) {
                    error!("--script-survey: {}: {error}", path.display());
                }
            }
            Err(error) => error!("--script-survey: the report does not serialise: {error}"),
        }
    }
    script.stage = Stage::Done;
}

/// The deposit a report names, for a test.
pub fn deposit_of(report: &SurveyReport) -> Option<strata::sim::deposits::DepositId> {
    let (x, z) = report.deposit.split_once(',')?;
    Some(strata::sim::deposits::DepositId([
        x.parse().ok()?,
        z.parse().ok()?,
    ]))
}
