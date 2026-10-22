//! `--script-quest <ending>` (ticket 0014): the first questline walked
//! headless to one of its endings.
//!
//! The script is a client of the sim like the other scripts: it opens each
//! conversation with a `DialogueRequest`, picks the replies the ending needs
//! by what they say, and meets each objective through the request a screen
//! would send (a ping and a sample, an assembly and a finish at the bench, a
//! join and a switch at the terminal, a learn at the trainer, the bike bought
//! at the board and stabled from the pack, the company's ending sold at the
//! board). Two things it
//! does not do: it does not walk (it is put beside each person and place, the
//! distance counted for the report), and it does not swing a blade (a kill is
//! the lethal `CombatImpact` the blow would have written; the fight itself is
//! `--script-progression`'s). What it did goes to the log and to
//! `--script-report`.

use std::collections::VecDeque;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use strata::registry::Namespaced;
use strata::registry::quest::{Place, Role};
use strata::sim::combat::{CombatImpact, GuardOutcome};
use strata::sim::crafting::{CraftAnswer, CraftDone, CraftInbox, CraftRequest};
use strata::sim::deposits::{
    PingAnswer, PingDone, SampleAnswer, SampleDone, Survey, SurveyInbox, SurveyNotice,
    SurveyRequest,
};
use strata::sim::encounters::EncounterId;
use strata::sim::procurement::{ProcurementInbox, ProcurementRequest, PurchaseAnswer, SaleAnswer};
use strata::sim::quests::{
    DialogueInbox, DialogueRefused, DialogueRequest, DialogueView, QuestEvent, QuestFoe, QuestLog,
    QuestPlaces,
};
use strata::sim::ride::{RideAnswer, RideInbox, RideRequest};
use strata::worldgen::WorldSource;

use super::people::{Stands, standing_at};
use super::*;

/// The conversation turns a player is assumed to take four seconds over,
/// reading and choosing: the script's own, for the report's minutes.
pub const READ_S: f64 = 4.0;

/// How long a step may take, played seconds, before the script gives up.
pub const STEP_LIMIT_S: f32 = 240.0;

/// The ground settles under the player for this long after a move.
pub const SETTLE_S: f32 = 2.0;

/// How far beside a person or a prop the script stands, metres.
pub const BESIDE_M: f64 = 1.5;

/// How the household meets Pell's crew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pell {
    /// Hand it over, or we take it.
    Fight,
    /// Two hundred credits.
    Pay,
    /// Tell him what the processor's intake is doing to the river.
    Talk,
}

impl Pell {
    pub fn parse(word: &str) -> Self {
        match word {
            "fight" => Self::Fight,
            "talk" => Self::Talk,
            _ => Self::Pay,
        }
    }
}

/// What the run did, as `--script-report` writes it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct QuestReport {
    /// The ending asked for.
    pub ending: String,
    /// Pell's branch.
    pub pell: String,
    /// Wim's branch.
    pub wim: String,
    /// The quest log's ending when the run stopped.
    pub reached: Option<String>,
    /// The run reached it.
    pub ok: bool,
    /// Why not, when it did not.
    pub failure: String,
    /// The steps taken, in order, as the log said them.
    pub steps: Vec<String>,
    /// Metres the household was moved, in all.
    pub travel_m: f64,
    /// Conversation turns taken.
    pub turns: u32,
    /// Played seconds sampling took.
    pub sampled_s: f64,
    /// Minutes from the start to the ending, walking.
    pub walking_minutes: f64,
    /// The same, sprinting.
    pub sprinting_minutes: f64,
    /// The same, on the courier bike at its ground speed.
    pub riding_minutes: f64,
    /// The deeds' score by role at the end, whole.
    pub deeds: Vec<(String, i64)>,
    /// The purse at the end.
    pub purse: u32,
}

/// One thing the script does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Talk to a person: the replies to pick, by what they say.
    Talk(&'static str, Vec<String>),
    /// Stand beside a person.
    Meet(&'static str),
    /// Stand at a quest place.
    Place(Place),
    /// Stand before a prop of the city's bench place: its slot along the
    /// row, in spacings.
    Bench(i8),
    /// Stand before a prop of Teague's field camp at the station: `0` its
    /// terminal, `1` its workbench.
    Camp(u8),
    /// Stand at the training ground and kill its sentry.
    Sentry,
    /// Learn a box at the trainer.
    Learn,
    /// Ping and sample the nearest conductor deposit to the station.
    Sample,
    /// Everyone who can joins the bench.
    Join,
    /// Make a harness at the workbench.
    Harness,
    /// Switch to a member, by name.
    Switch(&'static str),
    /// Kill a crew, by id.
    Crew(&'static str),
    /// Buy the item a Buy objective names at the board in reach.
    Buy,
    /// Make the bought bike the household's ride, from the pack.
    Stable,
    /// Sell the item a Sell objective names at the board in reach.
    Sell,
}

/// Where the current step is.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Phase {
    #[default]
    Start,
    Settling(f32),
    Opening,
    Talking,
    Pinging,
    Sampling,
    Making,
    Finishing,
    /// A purchase, a stabling or a sale asked, its answer awaited.
    Trading,
    Pausing(f32),
}

/// The script's state and what it has measured.
#[derive(Resource, Debug)]
pub struct QuestScript {
    steps: VecDeque<Step>,
    phase: Phase,
    /// Played seconds in the step.
    since: f32,
    /// The step's own scratch: the replies not yet said, and the request
    /// sent and not yet answered.
    say: VecDeque<String>,
    asked: bool,
    /// Bodies the step killed, removed once the log has heard of it.
    fallen: Vec<Entity>,
    /// The conversation's latest view, and whether a request awaits its next.
    view: Option<DialogueView>,
    waiting: bool,
    /// The `since` before which the next reply is not said, so a player who
    /// reads a line for a moment is walked too: what the reply set in motion
    /// (a crew folding) happens in between.
    reads_until: f32,
    /// Deposits that were not safe to sample, left out of the search.
    avoid: Vec<[f64; 2]>,
    /// Where the household is now, for the distance counted.
    at: Option<[f64; 2]>,
    /// Pending where the ground must settle.
    target: Option<[f64; 2]>,
    started: f64,
    sampling_from: Option<f32>,
    report: QuestReport,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "native files only"))]
    out: Option<PathBuf>,
    finished: bool,
}

impl QuestScript {
    pub const fn done(&self) -> bool {
        self.finished
    }

    fn fail(&mut self, why: impl Into<String>) {
        let why = why.into();
        error!("--script-quest: {why}");
        self.report.failure = why;
        self.finish();
    }

    fn finish(&mut self) {
        let report = &self.report;
        info!("--script-quest: {report:?}");
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.out {
            match ron::ser::to_string_pretty(report, ron::ser::PrettyConfig::default()) {
                Ok(text) => {
                    if let Err(error) = std::fs::write(path, text) {
                        error!("--script-quest: {}: {error}", path.display());
                    }
                }
                Err(error) => error!("--script-quest: the report does not serialise: {error}"),
            }
        }
        self.finished = true;
    }
}

/// The replies a person's conversation takes, by what they say, for one
/// ending and one branch of Pell's.
#[allow(
    clippy::too_many_lines,
    reason = "the whole questline's path, once, in order"
)]
pub fn plan(ending: &str, pell: Pell) -> (Vec<Step>, &'static str) {
    let say = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
    let talk = |person: &'static str, words: &[&str]| Step::Talk(person, say(words));
    let mut steps = vec![
        talk(
            "veyra:person.maren_coyle",
            &[
                "That's us. Where do we start",
                "Where should we go first",
                "We'll go and find her",
            ],
        ),
        // Ilse joins the bench before the questline asks for her, as a player
        // who read Maren's talk of the bench would: the mending stage counts
        // her already there.
        Step::Bench(0),
        // Teague's test: asked of the company when Pell is to be talked round.
        talk(
            "veyra:person.anouk_teague",
            if pell == Pell::Talk {
                &["make of your station", "We'll be back"]
            } else {
                &["We'll be back"]
            },
        ),
        Step::Sentry,
        Step::Bench(1),
        Step::Learn,
        talk("veyra:person.anouk_teague", &["We're on our way"]),
        // The bike: Corin's credit, bought at the board, made the ride.
        talk(
            "veyra:person.corin_hale",
            &["We'll take the newcomer's credit", "Thanks, Corin"],
        ),
        Step::Bench(2),
        Step::Buy,
        Step::Stable,
        Step::Place(Place::Station),
    ];
    match pell {
        Pell::Fight => steps.extend([
            talk("veyra:person.pell", &["Hand over the core"]),
            Step::Crew("pell"),
        ]),
        Pell::Pay => steps.push(talk(
            "veyra:person.pell",
            &["Two hundred credits", "Nobody will"],
        )),
        Pell::Talk => steps.push(talk(
            "veyra:person.pell",
            &["Teague says the processor's intake", "That's worth a lot"],
        )),
    }
    steps.extend([
        // The mending is done at the camp beside the mast, never in the city.
        Step::Sample,
        Step::Camp(1),
        Step::Harness,
        Step::Meet("veyra:person.station_mast"),
        talk(
            "veyra:person.station_mast",
            &["Fit the harness you made", "Copy the readings"],
        ),
        Step::Camp(0),
        // Ilse is recruited here, at the camp terminal, so the mending's
        // Recruit is walked on site.
        Step::Join,
        Step::Switch("Tovan Reyes"),
        Step::Place(Place::Channel),
    ]);
    // Wim: the keepers' run shuts the pump, the assembly's promises to leave
    // it, the company's threatens him into a fight.
    match ending {
        "keepers" => steps.extend([
            talk(
                "veyra:person.wim_larrow",
                &["How much is your channel taking", "We'll decide who hears"],
            ),
            Step::Meet("veyra:person.wim_pump"),
            talk("veyra:person.wim_pump", &["Close the valve"]),
        ]),
        "assembly" => steps.push(talk(
            "veyra:person.wim_larrow",
            &["We'll leave your pump running", "We'll tell them"],
        )),
        _ => steps.extend([
            talk("veyra:person.wim_larrow", &["Shut the channel now"]),
            Step::Crew("wim_larrow"),
        ]),
    }
    match ending {
        "keepers" => steps.push(talk(
            "veyra:person.anouk_teague",
            &["Publish it", "Thank you, Doctor"],
        )),
        "assembly" => steps.push(talk(
            "veyra:person.maren_coyle",
            &["Take the record", "Good luck with the council"],
        )),
        _ => steps.extend([
            talk(
                "veyra:person.corin_hale",
                &["Sell it to", "We'll take it to the board"],
            ),
            Step::Bench(2),
            Step::Sell,
        ]),
    }
    let wim = match ending {
        "keepers" => "shut",
        "assembly" => "leave",
        _ => "threaten",
    };
    (steps, wim)
}

/// Add the script to the app: the plan for the ending and the system that
/// walks it.
pub fn install(app: &mut App, cli: &crate::DevArgs, ending: &str) {
    let pell = Pell::parse(&cli.script_pell);
    let (steps, wim) = plan(ending, pell);
    app.insert_resource(QuestScript {
        steps: steps.into(),
        phase: Phase::Start,
        since: 0.0,
        reads_until: 0.0,
        avoid: Vec::new(),
        say: VecDeque::new(),
        asked: false,
        fallen: Vec::new(),
        view: None,
        waiting: false,
        at: None,
        target: None,
        started: 0.0,
        sampling_from: None,
        report: QuestReport {
            ending: ending.to_owned(),
            pell: format!("{pell:?}").to_lowercase(),
            wim: wim.to_owned(),
            ..default()
        },
        out: cli.script_report.clone(),
        finished: false,
    })
    .add_systems(
        SimTick,
        drive_quest
            .run_if(resource_exists::<Household>)
            .run_if(resource_exists::<Stands>)
            .in_set(SimSet::Input),
    );
}

/// Everything the driver reads and writes, as one parameter.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Env<'w, 's> {
    time: Res<'w, Time<Fixed>>,
    registries: Res<'w, strata::sim::Registries>,
    reach: Res<'w, Reach>,
    log: Res<'w, QuestLog>,
    stands: Res<'w, Stands>,
    places: Res<'w, QuestPlaces>,
    source: Res<'w, WorldSourceRes>,
    staging: Option<Res<'w, veyra_game::hostiles::StagingSpawn>>,
    settlements: Option<Res<'w, strata::sim::Settlements>>,
    store: Res<'w, HouseholdStore>,
    survey: Option<Res<'w, Survey>>,
    inbox: ResMut<'w, DialogueInbox>,
    fights: ResMut<'w, strata::sim::encounters::Fights>,
    survey_inbox: ResMut<'w, SurveyInbox>,
    craft_inbox: ResMut<'w, CraftInbox>,
    trade_inbox: ResMut<'w, ProcurementInbox>,
    ride_inbox: ResMut<'w, RideInbox>,
    purchases: MessageReader<'w, 's, PurchaseAnswer>,
    sales: MessageReader<'w, 's, SaleAnswer>,
    rides: MessageReader<'w, 's, RideAnswer>,
    views: MessageReader<'w, 's, DialogueView>,
    refused: MessageReader<'w, 's, DialogueRefused>,
    pings: MessageReader<'w, 's, PingAnswer>,
    samples: MessageReader<'w, 's, SampleAnswer>,
    notices: MessageReader<'w, 's, SurveyNotice>,
    crafts: MessageReader<'w, 's, CraftAnswer>,
    events: MessageWriter<'w, QuestEvent>,
    impacts: MessageWriter<'w, CombatImpact>,
    clock: Option<Res<'w, strata::sim::encounters::PlayedClock>>,
    deeds: Option<Res<'w, Deeds>>,
    cast: Res<'w, strata::sim::quests::QuestCast>,
    purse: Res<'w, strata::sim::deposits::Purse>,
    encounter: Option<Res<'w, CombatEncounter>>,
    run: Option<Res<'w, FactoryState>>,
    commands: Commands<'w, 's>,
}

/// Put the player at `(x, z)` on the ground there.
pub fn put(commands: &mut Commands, player: Entity, source: &dyn WorldSource, at: [f64; 2]) {
    let ground = f64::from(source.sample(at[0], at[1]).height.max(source.sea_level()));
    commands.queue(move |world: &mut bevy::ecs::world::World| {
        let half = world
            .get::<CharacterController>(player)
            .map_or(0.9, |controller| f64::from(controller.half_height()));
        if let Some(mut position) = world.get_mut::<WorldPosition>(player) {
            position.0 = WorldPos::new(at[0], ground + 0.5 + half, at[1]);
        }
        if let Some(mut previous) = world.get_mut::<PreviousWorldPosition>(player) {
            previous.0 = None;
        }
        if let Some(mut controller) = world.get_mut::<CharacterController>(player) {
            controller.vertical_velocity = 0.;
            controller.horizontal_velocity = Vec3::ZERO;
        }
    });
}

/// The point before a prop of the city's bench place, `slot` spacings along.
pub fn before_prop(settlements: Option<&strata::sim::Settlements>, slot: f64) -> Option<[f64; 2]> {
    let ((x, z), yaw) = city_place(settlements)?;
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    let along = slot * CITY_SPACING_M;
    Some([
        x + f64::from(right.x) * along - f64::from(forward.x) * BESIDE_M,
        z + f64::from(right.z) * along - f64::from(forward.z) * BESIDE_M,
    ])
}

/// The point before a prop of the field camp: its terminal or its workbench.
pub fn before_camp(w: &Env<'_, '_>, prop: u8) -> Option<[f64; 2]> {
    let rules = w.registries.0.quests.rules()?;
    let station = *w.places.0.get(&Place::Station)?;
    let city = w.settlements.as_ref()?.0.first()?.site.centre;
    let camp = strata::sim::quests::camp(w.source.source(), rules, station, city)?;
    let (terminal, bench, yaw) = (camp.terminal, camp.bench, camp.yaw);
    let at = if prop == 0 { terminal } else { bench };
    let forward = PlayerIntent::forward(yaw);
    Some([
        at[0] - f64::from(forward.x) * BESIDE_M,
        at[1] - f64::from(forward.z) * BESIDE_M,
    ])
}

/// What a step needs the player to stand at, if it is a move.
pub fn goal(step: &Step, w: &Env<'_, '_>, avoid: &[[f64; 2]]) -> Option<[f64; 2]> {
    let beside = |at: [f64; 2]| Some([at[0] + BESIDE_M, at[1]]);
    match step {
        Step::Talk(person, _) | Step::Meet(person) => {
            let id = Namespaced::parse(person).ok()?;
            standing_at(&w.stands, &id).and_then(beside)
        }
        Step::Place(place) => w.places.0.get(place).copied().and_then(beside),
        Step::Bench(slot) => before_prop(w.settlements.as_deref(), f64::from(*slot)),
        Step::Camp(prop) => before_camp(w, *prop),
        Step::Sentry => w
            .staging
            .as_ref()
            .map(|staging| [staging.at.x(), staging.at.z()]),
        Step::Sample => {
            // The nearest conductor deposit to the station, from the survey.
            let field = &w.survey.as_ref()?.0;
            let station = w.places.0.get(&Place::Station)?;
            let class = Namespaced::parse("strata:conductor").ok()?;
            let range = field.set().rules().register.range_m;
            // A deposit a camp stands on is passed over once it has hit us.
            let found = field.nearest(station[0], station[1], range, |deposit| {
                w.registries.0.resources.class_of(deposit.ty).id == class
                    && !avoid.contains(&deposit.at)
            })?;
            Some(found.at)
        }
        Step::Crew(_)
        | Step::Buy
        | Step::Stable
        | Step::Sell
        | Step::Learn
        | Step::Join
        | Step::Harness
        | Step::Switch(_) => None,
    }
}

/// Where a step goes once it is where it is meant to be.
pub const fn next_phase(step: &Step) -> Phase {
    match step {
        Step::Sample => Phase::Pinging,
        Step::Harness => Phase::Making,
        _ => Phase::Opening,
    }
}

/// Written as one long state machine, read top to bottom.
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "one state machine over every request the questline's objectives are met by"
)]
pub fn drive_quest(
    mut script: ResMut<QuestScript>,
    mut w: Env,
    mut household: ResMut<Household>,
    mut players: Query<PlayerParts, With<LocalPlayer>>,
    controllers: Query<(Entity, &WorldPosition, &CharacterController), With<LocalPlayer>>,
    foes: Query<(Entity, &QuestFoe, &EncounterId, &WorldPosition)>,
    staging: Query<
        (Entity, &EncounterId, &WorldPosition, &Combatant),
        (With<QuestFoe>, Without<LocalPlayer>),
    >,
) {
    if script.finished {
        return;
    }
    let Ok((player, here, controller)) = controllers.single() else {
        return;
    };
    let dt = w.time.delta_secs();
    script.since += dt;
    let registries = w.registries.0.clone();
    if script.started == 0.0 {
        script.started = w.clock.as_ref().map_or(1.0, |clock| clock.seconds.max(1.0));
    }
    if script.since > STEP_LIMIT_S && !matches!(script.phase, Phase::Pausing(_)) {
        let step = script.steps.front().cloned();
        script.fail(format!("given up on {step:?} after {STEP_LIMIT_S} s"));
        return;
    }
    if script.steps.is_empty() {
        let done = w.log.ending.clone();
        script.report.reached.clone_from(&done);
        script.report.ok = done.as_deref() == Some(script.report.ending.as_str());
        if !script.report.ok {
            script.fail(format!("the questline ended as {done:?}"));
            return;
        }
        // The report's figures.
        let actor = &strata::sim::combat_tables::current().actor;
        let walk = f64::from(actor.walk_speed).max(0.1);
        let sprint = f64::from(actor.sprint_speed).max(0.1);
        let fixed = f64::from(script.report.turns) * READ_S + script.report.sampled_s;
        script.report.walking_minutes = (script.report.travel_m / walk + fixed) / 60.0;
        script.report.sprinting_minutes = (script.report.travel_m / sprint + fixed) / 60.0;
        if let Some(bike) = registries.vehicles.iter().next() {
            let ride = f64::from(bike.speed.ground).max(0.1);
            script.report.riding_minutes = (script.report.travel_m / ride + fixed) / 60.0;
        }
        script.report.purse = w.purse.0;
        let now = w.clock.as_ref().map_or(0.0, |clock| clock.seconds);
        if let (Some(deeds), Some(rules)) = (w.deeds.as_ref(), registries.factions.deeds()) {
            for (role, word) in [
                (Role::Holder, "holder"),
                (Role::Assembly, "assembly"),
                (Role::Keepers, "keepers"),
            ] {
                if let Some(faction) = w.cast.factions.get(&role) {
                    #[allow(clippy::cast_possible_truncation, reason = "a score for a report")]
                    let score =
                        strata::sim::deeds::score(deeds, *faction, now, rules).round() as i64;
                    script.report.deeds.push((word.to_owned(), score));
                }
            }
        }
        script.finish();
        return;
    }
    let Some(step) = script.steps.front().cloned() else {
        return;
    };
    let ground_of = |at: [f64; 2]| f64::from(w.source.source().sample(at[0], at[1]).height);
    match script.phase.clone() {
        Phase::Start => {
            script.since = 0.0;
            script.reads_until = 0.0;
            script.asked = false;
            script.waiting = false;
            script.view = None;
            // What an earlier conversation left unread is not this one's.
            w.views.read().for_each(drop);
            w.refused.read().for_each(drop);
            w.purchases.read().for_each(drop);
            w.sales.read().for_each(drop);
            w.rides.read().for_each(drop);
            script
                .report
                .steps
                .push(format!("{step:?}").chars().take(80).collect());
            if let Step::Talk(_, words) = &step {
                script.say = words.iter().cloned().collect();
            }
            if let Some(at) = goal(&step, &w, &script.avoid) {
                let from = script.at.unwrap_or([here.0.x(), here.0.z()]);
                script.report.travel_m += (at[0] - from[0]).hypot(at[1] - from[1]);
                script.at = Some(at);
                script.target = Some(at);
                put(&mut w.commands, player, w.source.source(), at);
                script.phase = Phase::Settling(0.0);
            } else {
                script.phase = next_phase(&step);
            }
        }
        Phase::Settling(waited) => {
            // Held in place until the ground is under the player; a player
            // that has fallen through is put back on it.
            if let Some(at) = script.target
                && here.0.y() < ground_of(at) - 4.0
            {
                put(&mut w.commands, player, w.source.source(), at);
            }
            let waited = if controller.grounded {
                waited + dt
            } else {
                0.0
            };
            if waited < SETTLE_S {
                script.phase = Phase::Settling(waited);
                return;
            }
            script.target = None;
            script.since = 0.0;
            script.reads_until = 0.0;
            script.phase = next_phase(&step);
        }
        Phase::Opening => match &step {
            Step::Talk(person, _) => {
                let Ok(id) = Namespaced::parse(person) else {
                    script.fail(format!("`{person}` is no person id"));
                    return;
                };
                // Waiting for the person to stand and the reach to see them.
                if w.reach.people.contains(&id) {
                    w.inbox.push(DialogueRequest::Open(id));
                    script.waiting = true;
                    script.phase = Phase::Talking;
                }
            }
            Step::Meet(person) => {
                let Ok(id) = Namespaced::parse(person) else {
                    script.fail(format!("`{person}` is no person id"));
                    return;
                };
                if w.reach.people.contains(&id) {
                    script.phase = Phase::Pausing(0.0);
                }
            }
            Step::Place(_) | Step::Bench(_) | Step::Camp(_) | Step::Sample | Step::Harness => {
                script.phase = Phase::Pausing(0.0);
            }
            Step::Sentry => {
                // The training fight stands once its ground has streamed.
                let sentry = foes
                    .iter()
                    .find(|(_, foe, id, _)| {
                        foe.kind == strata::registry::FoeKind::Sentry
                            && matches!(id, EncounterId::Staging)
                    })
                    .map(|(entity, _, _, at)| (entity, at.0));
                let Some((victim, at)) = sentry else {
                    return;
                };
                let sentry_health = staging.get(victim).map_or(0.0, |(.., actor)| actor.health);
                // The whole training fight falls, each for its full health,
                // so the XP is what the fight pays and a box can be learned.
                w.impacts
                    .write(lethal(player, victim, here.0, at, sentry_health));
                for (entity, id, at, actor) in &staging {
                    if matches!(id, EncounterId::Staging) && entity != victim {
                        w.impacts
                            .write(lethal(player, entity, here.0, at.0, actor.health));
                    }
                    if matches!(id, EncounterId::Staging) {
                        script.fallen.push(entity);
                    }
                }
                script.phase = Phase::Pausing(0.0);
            }
            Step::Learn => {
                let Ok(mut parts) = players.single_mut() else {
                    return;
                };
                let trees = vec!["blade".to_owned()];
                match learn_at_trainer(
                    &trees,
                    &registries.progression,
                    &w.reach,
                    out!(parts),
                    &mut household,
                ) {
                    Ok(id) => {
                        w.events.write(QuestEvent::Learned(id));
                        script.phase = Phase::Pausing(0.0);
                    }
                    Err(why) => script.fail(format!("nothing learned: {why}")),
                }
            }
            Step::Join => {
                for name in join_all(&mut household) {
                    w.events.write(QuestEvent::Recruited(name));
                }
                script.phase = Phase::Pausing(0.0);
            }
            Step::Switch(name) => {
                let Some(index) = household.characters.iter().position(|c| c.name == *name) else {
                    script.fail(format!("nobody called {name} on the bench"));
                    return;
                };
                let Ok(mut parts) = players.single_mut() else {
                    return;
                };
                match swap_to(
                    &mut household,
                    index,
                    &registries,
                    w.encounter.as_deref(),
                    w.run.as_deref().and_then(|run| run.job.as_ref()),
                    out!(parts),
                ) {
                    Ok(()) => {
                        w.events.write(QuestEvent::Switched((*name).to_owned()));
                        script.phase = Phase::Pausing(0.0);
                    }
                    // A hit just taken wears off; the step limit ends a wait
                    // that does not.
                    // The camp left behind at the deposit is let go, as F5
                    // lets a player's.
                    Err(why) if why == "not while fighting" && script.since < 20.0 => {
                        w.fights.abandon_camps();
                    }
                    Err(why) if why == "not while fighting" => {
                        let (engaged, left) = w.fights.headline();
                        script.fail(format!(
                            "the switch was refused: fighting (engaged {engaged}, {left} left, action {:?})",
                            parts.1.action
                        ));
                    }
                    Err(why) => script.fail(format!("the switch was refused: {why}")),
                }
            }
            Step::Crew(crew) => {
                // The crew has turned on the household; the blows that
                // would end it are written whole.
                let id = super::people::encounter_of(crew);
                let list: Vec<_> = foes
                    .iter()
                    .filter(|(_, foe, found, _)| {
                        foe.crew.as_deref() == Some(*crew) && **found == id
                    })
                    .map(|(entity, _, _, at)| (entity, at.0))
                    .collect();
                if list.is_empty() {
                    return;
                }
                for (victim, at) in list {
                    w.impacts.write(lethal(player, victim, here.0, at, 0.0));
                    script.fallen.push(victim);
                }
                script.phase = Phase::Pausing(0.0);
            }
            Step::Buy | Step::Sell => {
                let Some(buyer) = w.reach.board.clone() else {
                    // The board's prop stands once its ground has streamed.
                    return;
                };
                let (Some(item), Some(def)) = (
                    quest_item(&registries, &step),
                    quest_item(&registries, &step).and_then(|item| registries.items.get(&item)),
                ) else {
                    script.fail(format!("no objective names what {step:?} trades"));
                    return;
                };
                let request = if step == Step::Buy {
                    ProcurementRequest::Buy { buyer, item }
                } else {
                    let Some(line) = registries.buyers.get(&buyer).and_then(|buyer| {
                        buyer.lines().find_map(|(_, line)| {
                            (line.wants == strata::registry::procurement::Wants::Item(item.clone()))
                                .then(|| line.id.clone())
                        })
                    }) else {
                        script.fail(format!("the board buys no `{item}`"));
                        return;
                    };
                    ProcurementRequest::Sell {
                        buyer,
                        line,
                        kind: strata::sim::crafting::bought(def),
                        count: 1,
                    }
                };
                w.trade_inbox.push(request);
                script.phase = Phase::Trading;
            }
            Step::Stable => {
                let Some(def) = quest_item(&registries, &Step::Buy)
                    .and_then(|item| registries.items.get(&item))
                else {
                    script.fail("no Buy objective names the bike");
                    return;
                };
                w.ride_inbox.push(RideRequest::Stable {
                    kind: strata::sim::crafting::bought(def),
                });
                script.phase = Phase::Trading;
            }
        },
        Phase::Talking => {
            if !matches!(step, Step::Talk(..)) {
                return;
            }
            if let Some(refusal) = w.refused.read().next() {
                let why = format!("{:?} refused: {:?}", refusal.request, refusal.why);
                script.fail(why);
                return;
            }
            if let Some(view) = w.views.read().last() {
                script.view = Some(view.clone());
                script.waiting = false;
            }
            if script.waiting || script.since < script.reads_until {
                return;
            }
            let Some(view) = script.view.clone() else {
                return;
            };
            if !view.open {
                script.say.clear();
                script.phase = Phase::Pausing(0.0);
                return;
            }
            let pick = script.say.front().and_then(|words| {
                view.replies.iter().position(|reply| {
                    reply.blocked.is_none() && reply.text.contains(words.as_str())
                })
            });
            script.report.turns += 1;
            if let Some(place) = pick {
                script.say.pop_front();
                script.waiting = true;
                script.reads_until = script.since + SETTLE_S;
                w.inbox.push(DialogueRequest::Choose(place));
            } else {
                if !script.say.is_empty() {
                    warn!(
                        "--script-quest: no reply with `{}` among {:?}",
                        script.say.front().map_or("", String::as_str),
                        view.replies
                            .iter()
                            .map(|r| r.text.as_str())
                            .collect::<Vec<_>>()
                    );
                }
                script.say.clear();
                w.inbox.push(DialogueRequest::Close);
                script.phase = Phase::Pausing(0.0);
            }
        }
        Phase::Pinging => {
            if !script.asked {
                script.asked = true;
                let Ok(class) = Namespaced::parse("strata:conductor") else {
                    return;
                };
                w.survey_inbox.push(SurveyRequest::Ping { class });
            }
            for answer in w.pings.read() {
                match &answer.outcome {
                    Ok(PingDone::Met(result)) if result.lead().is_some() => {
                        w.survey_inbox.push(SurveyRequest::Sample);
                        script.sampling_from = Some(script.since);
                        script.phase = Phase::Sampling;
                    }
                    Ok(PingDone::Met(_)) => {
                        script.fail("the ping met nothing at the deposit");
                        return;
                    }
                    Ok(PingDone::Started) => {}
                    Err(why) => {
                        script.fail(format!("the ping was refused: {why}"));
                        return;
                    }
                }
            }
        }
        Phase::Sampling => {
            if let Some(SurveyNotice::SampleEnded(why)) = w.notices.read().last() {
                // A camp on the deposit: try the next nearest.
                if matches!(why, strata::sim::deposits::SampleEnd::Hit)
                    && let Some(at) = script.at
                    && script.avoid.len() < 6
                {
                    script.avoid.push(at);
                    // The camp the household leaves does not stay engaged.
                    w.fights.abandon_camps();
                    script.report.steps.pop();
                    script.phase = Phase::Start;
                    return;
                }
                script.fail(format!("sampling ended early: {why:?}"));
                return;
            }
            for answer in w.samples.read() {
                match &answer.outcome {
                    Ok(SampleDone::Cycle { total, .. }) => {
                        let wanted = sampling_units(&registries);
                        if *total >= wanted {
                            w.survey_inbox.push(SurveyRequest::Stop);
                            script.report.sampled_s +=
                                f64::from(script.since - script.sampling_from.unwrap_or(0.0));
                            script.phase = Phase::Pausing(0.0);
                        }
                    }
                    Ok(SampleDone::Started { .. }) => {}
                    Err(why) => {
                        script.fail(format!("the sample was refused: {why}"));
                        return;
                    }
                }
            }
        }
        Phase::Making => {
            let Ok(parts) = players.single() else {
                return;
            };
            let pack = &parts.6;
            let answers: Vec<CraftAnswer> = w.crafts.read().cloned().collect();
            for answer in answers {
                match answer.outcome {
                    Ok(CraftDone::Assembled { .. }) => {
                        w.craft_inbox.push(CraftRequest::Finish);
                        script.phase = Phase::Finishing;
                    }
                    Ok(_) => {}
                    Err(why) => {
                        script.fail(format!("the bench refused {:?}: {why}", answer.request));
                        return;
                    }
                }
            }
            if !script.asked && script.phase == Phase::Making {
                let Some(schematic) = Namespaced::parse("veyra:schematic.harness")
                    .ok()
                    .and_then(|id| registries.schematics.get(&id))
                else {
                    script.fail("the data set has no harness schematic");
                    return;
                };
                let picks =
                    strata::sim::crafting::best_stock(&registries, pack, Some(&w.store), schematic);
                script.asked = true;
                w.craft_inbox.push(CraftRequest::Assemble {
                    schematic: schematic.def.id.clone(),
                    picks,
                });
            }
        }
        Phase::Finishing => {
            let answers: Vec<CraftAnswer> = w.crafts.read().cloned().collect();
            for answer in answers {
                match answer.outcome {
                    Ok(CraftDone::Finished { .. }) => {
                        script.phase = Phase::Pausing(0.0);
                    }
                    Ok(_) => {}
                    Err(why) => {
                        script.fail(format!("the bench refused {:?}: {why}", answer.request));
                        return;
                    }
                }
            }
        }
        Phase::Trading => {
            let refused = match step {
                Step::Buy => w
                    .purchases
                    .read()
                    .last()
                    .map(|answer| answer.result.as_ref().err().map(|why| format!("{why:?}"))),
                Step::Sell => w
                    .sales
                    .read()
                    .last()
                    .map(|answer| answer.result.as_ref().err().map(|why| format!("{why:?}"))),
                _ => w
                    .rides
                    .read()
                    .last()
                    .map(|answer| answer.result.err().map(|why| format!("{why:?}"))),
            };
            match refused {
                None => {}
                Some(None) => script.phase = Phase::Pausing(0.0),
                Some(Some(why)) => script.fail(format!("{step:?} was refused: {why}")),
            }
        }
        Phase::Pausing(waited) => {
            // A step's effects reach the log in the next ticks; the next step
            // starts once they have.
            let waited = waited + dt;
            if waited < 0.5 {
                script.phase = Phase::Pausing(waited);
                return;
            }
            script.steps.pop_front();
            for body in std::mem::take(&mut script.fallen) {
                w.commands.entity(body).try_despawn();
            }
            script.phase = Phase::Start;
            script.say.clear();
        }
    }
}

/// The item a Buy step buys or a Sell step sells: the one the quest's own
/// Buy or Sell objective names.
pub fn quest_item(registries: &GameRegistries, step: &Step) -> Option<Namespaced> {
    registries
        .quests
        .quests()
        .iter()
        .flat_map(|quest| &quest.stages)
        .flat_map(|stage| &stage.objectives)
        .find_map(|objective| match (step, objective) {
            (Step::Buy, strata::registry::quest::Objective::Buy(item))
            | (Step::Sell, strata::registry::quest::Objective::Sell(item)) => Some(item.clone()),
            _ => None,
        })
}

/// The units the sampling objective wants, read from the quest's own data.
pub fn sampling_units(registries: &GameRegistries) -> u32 {
    registries
        .quests
        .quests()
        .iter()
        .flat_map(|quest| &quest.stages)
        .flat_map(|stage| &stage.objectives)
        .find_map(|objective| match objective {
            strata::registry::quest::Objective::Sample { units, .. } => Some(*units),
            _ => None,
        })
        .unwrap_or(0)
}

/// The blow that ends `victim`, as the combat would have written it.
pub fn lethal(
    attacker: Entity,
    victim: Entity,
    from: WorldPos,
    point: WorldPos,
    dealt: f32,
) -> CombatImpact {
    CombatImpact {
        attacker,
        victim: Some(victim),
        from,
        point,
        melee: true,
        dealt,
        lethal: true,
        guard: GuardOutcome::Open,
        shot: None,
    }
}
