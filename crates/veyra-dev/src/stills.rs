//! The stills' systems: a flag that opens a screen, holds a key, pings or
//! takes a picture, each installed only when its flag is set (so a played
//! session carries no system per flag that reads the flags to return), and
//! the exit rule and the benchmark's summary of a measured run.

use std::path::PathBuf;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use strata::client::flight::FlightPath;
use strata::client::stats::BenchRecorder;
use strata::prelude::*;

use crate::{DevArgs, RunFacts};

/// Installs the systems that serve a flag for a still or a script, each only
/// when its flag is set, so a played session does not carry a system per flag
/// that reads `Res<DevArgs>` every frame to return.
pub fn install(app: &mut App, dev: &DevArgs) {
    if dev.screenshot.is_some() {
        app.add_systems(Update, take_screenshots);
    }
    if dev.open_pause {
        app.add_systems(Update, open_pause_for_shot);
    }
    // `open_screens_for_shot` is a no-op under the progression script.
    let opens_a_screen = dev.open_survey
        || dev.open_map.is_some()
        || dev.open_register
        || dev.stores_filter.is_some()
        || dev.open_sheet
        || dev.open_loadout
        || dev.open_bench
        || dev.open_stores
        || dev.open_inventory
        || dev.open_fitting.is_some()
        || dev.crafting_filter.is_some()
        || dev.open_crafting.is_some()
        || dev.open_factory
        || dev.open_board
        || dev.open_journal;
    if opens_a_screen && !dev.script_progression {
        app.add_systems(Update, open_screens_for_shot);
    }
    if dev.open_dialogue.is_some() {
        app.add_systems(Update, open_dialogue_for_shot);
    }
    if dev.open_render_panel {
        app.add_systems(Update, open_render_for_shot);
    }
    if dev.track.is_some() {
        app.add_systems(Update, track_for_shot);
    }
    if dev.ping.is_some() {
        app.add_systems(Update, ping_for_shot);
    }
    if dev.sampling {
        app.add_systems(Update, sampling_for_shot);
    }
    // After the camera set, which writes the trigger from the mouse (and
    // clears it in an unfocused window), so the held trigger is the last word.
    let after_camera = strata::client::ClientSet::Camera;
    if dev.hold_fire.is_some() {
        app.add_systems(Update, hold_fire_for_shot.after(after_camera));
    }
    if dev.hold_guard.is_some() {
        app.add_systems(Update, hold_guard_for_shot.after(after_camera));
    }
    if dev.hold_aim.is_some() {
        app.add_systems(Update, hold_aim_for_shot.after(after_camera));
    }
}

pub fn open_render_for_shot(mut commands: Commands, dev: Res<DevArgs>, frame: Res<FrameCount>) {
    if dev.open_render_panel && frame.0 == 120 {
        strata::client::ui::render::open(&mut commands);
    }
}

pub use veyra_game::OPEN_PAUSE_FRAME;

/// `--open-pause`: the pause screen over the world, once, as Escape would.
pub fn open_pause_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    mut commands: Commands,
    mut opened: Local<bool>,
) {
    if !dev.open_pause || *opened || frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    *opened = true;
    strata::client::ui::open_pause(&mut commands);
}

/// `--press`: the places asked for are pressed on their frames, written
/// where the keys write them, so the sim's rule answers a press as it
/// answers a hand's.
pub fn press_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    for (place, _) in dev.press.iter().filter(|(_, frame)| *frame == frames.0) {
        for mut input in &mut players {
            input.ability = Some(*place);
        }
    }
}

/// `--hold-fire`: the player's trigger is down for the given seconds from
/// frame 120, written where the mouse button would write it.
pub fn hold_fire_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut players: Query<
        (
            &mut strata::sim::combat::CombatIntent,
            &strata::sim::WorldPosition,
        ),
        With<LocalPlayer>,
    >,
    hostiles: Query<
        (&strata::sim::WorldPosition, &strata::sim::combat::Combatant),
        Without<LocalPlayer>,
    >,
) {
    let Some(seconds) = dev.hold_fire else {
        return;
    };
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    for (mut input, at) in &mut players {
        // A window with no focus gives the read of the mouse no aim to keep,
        // so a still's shots go at the nearest hostile's chest.
        if let Some((target, _)) = hostiles
            .iter()
            .filter(|(_, actor)| actor.health > 0.)
            .min_by(|(a, _), (b, _)| a.0.distance(at.0).total_cmp(&b.0.distance(at.0)))
        {
            input.aim_point = WorldPos::new(target.0.x(), target.0.y() + 0.2, target.0.z());
        }
        input.fire = true;
    }
}

/// `--hold-aim`: the player's aim button is down for the given seconds from
/// frame 120, written where the right mouse button would write it.
pub fn hold_aim_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    let Some(seconds) = dev.hold_aim else {
        return;
    };
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    for mut input in &mut players {
        input.aim = true;
    }
}

/// `--hold-guard`: the player's aim button is down for the given seconds
/// from frame 120, which with a melee weapon in hand is the guard.
pub fn hold_guard_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut hold: ResMut<veyra_game::GuardHold>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    let Some(seconds) = dev.hold_guard else {
        return;
    };
    hold.0 = false;
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    hold.0 = true;
    for mut input in &mut players {
        input.aim = true;
    }
}

/// `--track`: the nearest deposit of the type to the default start is
/// tracked, once the survey is there to ask. Asked of the sim as the
/// register's row asks, so what is tracked is the sim's and is saved with
/// the household like any other.
pub fn track_for_shot(
    dev: Res<DevArgs>,
    survey: Option<Res<strata::sim::deposits::Survey>>,
    source: Option<Res<WorldSourceRes>>,
    settlements: Option<Res<strata::sim::Settlements>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut asked: Local<bool>,
) {
    let (Some(wanted), Some(survey), Some(source), false) =
        (dev.track.as_deref(), survey, source, *asked)
    else {
        return;
    };
    *asked = true;
    let from = veyra_game::hostiles::default_spawn(&source, settlements.as_deref())
        .map_or((0.0, 0.0), |(x, _, z)| (x, z));
    let Some(deposit) = veyra_game::deposit_start::nearest_of(&survey, wanted, from) else {
        warn!("--track {wanted}: no such deposit near the default start");
        return;
    };
    info!(
        "deposits: tracking {} at ({:.0}, {:.0}), {:.0} m from the default start",
        deposit.place,
        deposit.at[0],
        deposit.at[1],
        deposit.distance_m(from.0, from.1)
    );
    inbox.push(strata::sim::deposits::SurveyRequest::Track(Some(
        deposit.id,
    )));
}

/// The frame `--ping` waits for before it asks: the player is spawned and
/// stands in the world.
pub const PING_FRAME: u32 = 70;

/// `--ping <class>`: the ping the survey screen's button would send, once
/// the player is in the world. The class is an id, taken as the `strata`
/// namespace's when it names none. Sent once, asked of the sim like any
/// other, so the channel plays and the household learns what it meets.
pub fn ping_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    players: Query<(), With<LocalPlayer>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut answers: MessageReader<strata::sim::deposits::PingAnswer>,
    mut asked: Local<bool>,
) {
    for answer in answers.read() {
        match &answer.outcome {
            Ok(strata::sim::deposits::PingDone::Met(result)) => info!(
                "--ping: {} deposits met, the strongest {}",
                result.found.len(),
                result.found.first().map_or(0, |met| met.pm)
            ),
            Ok(strata::sim::deposits::PingDone::Started) => {}
            Err(refusal) => warn!("--ping: {refusal}"),
        }
    }
    let Some(class) = dev.ping.as_deref() else {
        return;
    };
    if *asked || frames.0 < PING_FRAME || players.is_empty() {
        return;
    }
    *asked = true;
    let full = if class.contains(':') {
        class.to_owned()
    } else {
        format!("strata:{class}")
    };
    if let Ok(class) = full.parse() {
        inbox.push(strata::sim::deposits::SurveyRequest::Ping { class });
    } else {
        warn!("--ping {class}: not a class id");
    }
}

/// `--sampling`: Sample is asked once, when the ping `--ping` asked for has
/// been answered (or, with no `--ping`, once the player stands in the
/// world), from where the player stands; the sim says whether there is
/// anything found to sample here.
pub fn sampling_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    last: Res<strata::sim::deposits::LastPing>,
    players: Query<(), With<LocalPlayer>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut answers: MessageReader<strata::sim::deposits::SampleAnswer>,
    mut asked: Local<bool>,
) {
    for answer in answers.read() {
        if let Err(refusal) = answer.outcome {
            warn!("--sampling: {refusal}");
        }
    }
    if !dev.sampling || *asked || players.is_empty() {
        return;
    }
    let ready = if dev.ping.is_some() {
        last.0.is_some()
    } else {
        frames.0 >= PING_FRAME
    };
    if ready {
        *asked = true;
        inbox.push(strata::sim::deposits::SurveyRequest::Sample);
    }
}

/// `--rework-first`: the first hand's weapon is reworked once, by the request
/// the Rework button sends, and the sim's answer is said in the log.
pub fn rework_first_for_shot(
    frames: Res<FrameCount>,
    mut inbox: ResMut<strata::sim::rework::ReworkInbox>,
    mut answers: MessageReader<strata::sim::rework::ReworkAnswer>,
    mut asked: Local<bool>,
) {
    use strata::sim::rework::ReworkRequest;
    for answer in answers.read() {
        match &answer.result {
            Ok(done) => info!(
                "--rework-first: {:?}, moddability {} to {}",
                done.outcome, done.from, done.to
            ),
            Err(refusal) => warn!("--rework-first: {refusal}"),
        }
    }
    if !*asked && frames.0 >= CRAFT_FIRST_FRAME {
        inbox.push(ReworkRequest::Rework { hand: 0 });
        *asked = true;
    }
}

/// `--craft-first`: where the making stands.
#[derive(Resource, Debug, Default)]
pub struct CraftFirst {
    /// A request is with the sim.
    pub asked: bool,
    /// The thing is made, and recorded when `--record-first` asks for it,
    /// or cannot be.
    pub done: bool,
}

/// The frame `--craft-first` starts on: the world is up and the household
/// has handed its stock to the sim.
pub const CRAFT_FIRST_FRAME: u32 = 40;

/// `--craft-first`: one of the named schematic is made before the still's
/// screen opens, by the requests a screen would send and the sim's own
/// choice of stock: Assemble from the best stock, each point steady on the
/// first line with room, Finish, and with `--record-first` Record. Each
/// request waits for its answer; one the sim refuses ends it, said in the
/// log.
#[allow(
    clippy::too_many_arguments,
    reason = "the flag, the clock, the stock the sim would pick from and the two ends of a request"
)]
pub fn craft_first_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    registries: Option<Res<strata::sim::Registries>>,
    craft: Res<strata::sim::crafting::CraftState>,
    store: Res<strata::sim::items::HouseholdStore>,
    players: Query<&strata::sim::items::Pack, With<strata::sim::LocalPlayer>>,
    mut inbox: ResMut<strata::sim::crafting::CraftInbox>,
    mut answers: MessageReader<strata::sim::crafting::CraftAnswer>,
    mut first: ResMut<CraftFirst>,
) {
    use strata::registry::schematic::Hand;
    use strata::sim::crafting::{self, CraftDone, CraftRequest};
    for answer in answers.read() {
        first.asked = false;
        match &answer.outcome {
            Ok(CraftDone::Finished { product }) => {
                info!(
                    "--craft-first: made {} batch {}",
                    product.item, product.batch
                );
                // `--record-first`: the thing just made is recorded by
                // the request the Record box sends, and that is waited on.
                if let Some(percent) = dev.record_first {
                    inbox.push(CraftRequest::Record {
                        batch: product.batch,
                        derate: percent.saturating_mul(10),
                    });
                    first.asked = true;
                } else {
                    first.done = true;
                }
            }
            Ok(CraftDone::Recorded {
                blueprint, runs, ..
            }) => {
                info!("--record-first: blueprint {blueprint}, {runs} runs");
                first.done = true;
            }
            Ok(_) => {}
            Err(refusal) => {
                warn!("--craft-first: {refusal}");
                first.done = true;
            }
        }
    }
    if first.done || first.asked || frames.0 < CRAFT_FIRST_FRAME {
        return;
    }
    let (Some(name), Some(registries), Ok(pack)) =
        (dev.craft_first.as_deref(), registries, players.single())
    else {
        return;
    };
    let registries = &registries.0;
    let request = match &craft.session {
        None => {
            let schematic = veyra_ui::crafting::schematic_named(registries, name)
                .and_then(|id| registries.schematics.get(&id));
            let Some(schematic) = schematic else {
                warn!("--craft-first {name}: no such schematic");
                first.done = true;
                return;
            };
            CraftRequest::Assemble {
                schematic: schematic.def.id.clone(),
                picks: crafting::best_stock(registries, pack, Some(&store), schematic),
            }
        }
        Some(session) => session
            .lines
            .iter()
            .find(|line| session.points > 0 && !line.at_ceiling())
            .map_or(CraftRequest::Finish, |line| CraftRequest::Roll {
                line: line.line.clone(),
                hand: Hand::Steady,
            }),
    };
    inbox.push(request);
    first.asked = true;
}

/// `--open-sheet`, `--open-bench`, `--open-stores`, `--open-inventory`,
/// `--open-survey`, `--open-register`, `--open-fitting`, `--open-crafting`
/// and `--open-factory` without the script: the screen over the world on
/// the same frame as the pause would be, or once `--craft-first` has made
/// its thing and `--factory-run` has started its run (or, with
/// `--factory-finish`, finished it) and `--factory-poor-lot` has stocked
/// its lot. With the script the
/// script opens the sheet, the bench, the fitting and the crafting where
/// they belong, and has no stop for the rest.
#[allow(
    clippy::too_many_arguments,
    reason = "the flag, the clock, the four things a still waits on, and the reach a bench opens by"
)]
pub fn open_screens_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    first: Option<Res<CraftFirst>>,
    run: Option<Res<crate::household::FactoryShot>>,
    lot: Option<Res<crate::household::PoorLotShot>>,
    joined: Option<Res<crate::household::JoinShot>>,
    board: Option<Res<crate::household::BoardShot>>,
    reach: Option<Res<strata::sim::progression::Reach>>,
    mut commands: Commands,
    mut opened: Local<bool>,
) {
    if dev.script_progression || *opened || frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    if first.is_some_and(|first| !first.done)
        || run.is_some_and(|run| !run.done)
        || lot.is_some_and(|lot| !lot.done)
        || joined.is_some_and(|joined| !joined.done)
        || board.is_some_and(|board| !board.stocked)
    {
        return;
    }
    *opened = true;
    if dev.open_survey {
        veyra_ui::survey::open(&mut commands);
    }
    if let Some(scale) = &dev.open_map {
        if let Some(scale) = veyra_ui::map::Scale::parse(scale) {
            veyra_ui::map::set_holders(&mut commands, dev.map_holders);
            veyra_ui::map::open_at(&mut commands, Some(scale));
        } else {
            warn!("--open-map takes near, region or far, not {scale}");
        }
    }
    if dev.open_register {
        veyra_ui::bench::open_on(&mut commands, false, veyra_ui::bench::BenchTab::Register);
    }
    if let Some(filter) = dev.stores_filter {
        veyra_ui::stores::show_nth(&mut commands, filter, dev.stores_pin);
    }
    if dev.open_sheet {
        // On the tree of the trainer stood before, as their F opens it.
        let tree = reach.as_ref().and_then(|reach| match &reach.nearest {
            Some((_, strata::sim::progression::InteractionKind::Trainer { trees })) => {
                trees.first().cloned()
            }
            _ => None,
        });
        veyra_ui::sheet::open(&mut commands, tree);
    }
    if dev.open_loadout {
        veyra_ui::sheet::open_loadout(&mut commands);
    }
    if dev.open_bench {
        // Live where a terminal's F would open it (`--at terminal` stands
        // at one), a reading anywhere else.
        let at_terminal = reach.is_some_and(|reach| {
            matches!(
                reach.nearest,
                Some((_, strata::sim::progression::InteractionKind::Terminal))
            )
        });
        veyra_ui::bench::open(&mut commands, at_terminal);
    }
    if dev.open_stores {
        veyra_ui::stores::open(&mut commands, true);
    } else if dev.open_inventory {
        veyra_ui::stores::open(&mut commands, false);
    }
    if let Some(mount) = &dev.open_fitting {
        veyra_ui::fitting::open_at(&mut commands, dev.fitting_hand, Some(mount.clone()));
        if dev.fitting_pick {
            veyra_ui::fitting::pick_first(&mut commands);
        }
    }
    if let Some(filter) = dev.crafting_filter {
        veyra_ui::crafting::set_filter(&mut commands, filter.filter());
    }
    if let Some(schematic) = &dev.open_crafting {
        veyra_ui::crafting::open_for_still(&mut commands, schematic, dev.crafting_still());
    }
    if dev.open_factory {
        veyra_ui::factory::open(&mut commands);
    }
    if dev.open_board {
        veyra_ui::board::open(&mut commands, crate::household::pelagic_works());
    }
    if dev.open_journal {
        veyra_ui::journal::open(&mut commands);
    }
}

/// `--open-dialogue`: ask for the person's conversation once the household
/// is in their reach, as their F does, on or after frame 120.
pub fn open_dialogue_for_shot(
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    reach: Option<Res<strata::sim::progression::Reach>>,
    inbox: Option<ResMut<strata::sim::quests::DialogueInbox>>,
    mut opened: Local<bool>,
) {
    let (Some(name), Some(reach), Some(mut inbox)) = (dev.open_dialogue.as_deref(), reach, inbox)
    else {
        return;
    };
    if *opened || frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let Ok(id) = format!("veyra:person.{name}").parse() else {
        return;
    };
    if reach.people.contains(&id) {
        inbox.push(strata::sim::quests::DialogueRequest::Open(id));
        *opened = true;
    }
}

/// Screenshot bookkeeping: how many were requested and how many are on
/// disk.
#[derive(Resource, Default)]
pub struct Shots {
    requested: u32,
    saved: u32,
}

/// The frame a screenshot is taken on when nothing says otherwise.
pub const DEFAULT_SHOT_FRAME: u32 = 60;

impl DevArgs {
    /// The frames `--screenshot` captures on.
    pub fn shot_frames(&self, frames: Option<u32>) -> Vec<u32> {
        if !self.shot_frames.is_empty() {
            return self.shot_frames.clone();
        }
        vec![frames.unwrap_or(DEFAULT_SHOT_FRAME)]
    }

    /// The screenshot path for `frame`, with `{frame}` substituted.
    pub fn shot_path(&self, frame: u32) -> Option<PathBuf> {
        let pattern = self.screenshot.as_ref()?.to_string_lossy().into_owned();
        Some(PathBuf::from(
            pattern.replace("{frame}", &frame.to_string()),
        ))
    }
}

/// `--screenshot`: capture on each requested frame.
///
/// The capture is asynchronous (a GPU readback), so completion is counted
/// by the observer rather than assumed a fixed number of frames later.
/// `save_to_disk` writes synchronously inside the observer, so when the
/// count goes up the file exists.
pub fn take_screenshots(
    mut commands: Commands,
    dev: Res<DevArgs>,
    frames: Res<FrameCount>,
    mut shots: ResMut<Shots>,
    run: Res<RunFacts>,
) {
    if dev.screenshot.is_none() {
        return;
    }
    let due = dev
        .shot_frames(run.frames)
        .into_iter()
        .filter(|f| *f <= frames.0)
        .count();
    // Frames are counted, not summed; the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    let due = due as u32;
    while shots.requested < due {
        let frame = dev.shot_frames(run.frames)[shots.requested as usize];
        shots.requested += 1;
        let Some(path) = dev.shot_path(frame) else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        info!("capturing frame {} to {}", frames.0, path.display());
        let mut save = save_to_disk(path);
        commands.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>, mut shots: ResMut<Shots>| {
                save(captured);
                shots.saved += 1;
            },
        );
    }
}

/// Exits once everything that was asked for has happened: `--frames` have
/// run, every screenshot is saved, and a scripted flight has lasted
/// `--flight-secs`. With none of those asked for, runs forever.
#[cfg_attr(
    target_arch = "wasm32",
    allow(unused_variables, reason = "only the native bench summary reads them")
)]
pub fn exit_when_done(
    (dev, run): (Res<DevArgs>, Res<RunFacts>),
    frames: Res<FrameCount>,
    shots: Res<Shots>,
    flights: Query<&FlightPath>,
    script: Option<Res<crate::household::ProgressionScript>>,
    // What the benchmark summary reads when the run ends, and the scripted
    // ride's report.
    (loaded, bench, entities, clock, ride, survey, quest): (
        Option<Res<LoadedRegions>>,
        Option<Res<BenchRecorder>>,
        Query<Entity>,
        Option<Res<strata::client::stats::WorkClock>>,
        Option<Res<crate::ride::ScriptRide>>,
        Option<Res<crate::survey_script::ScriptSurvey>>,
        Option<Res<crate::household::script_quest::QuestScript>>,
    ),
    mut exit: MessageWriter<AppExit>,
) {
    let mut asked = false;
    let mut done = true;
    if let Some(limit) = run.frames {
        asked = true;
        done &= frames.0 >= limit;
    }
    if dev.screenshot.is_some() {
        asked = true;
        // Frames are counted, not summed; the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let wanted = dev.shot_frames(run.frames).len() as u32;
        done &= shots.saved >= wanted;
    }
    if run.flight && run.frames.is_none() {
        asked = true;
        done &= flights.iter().any(|f| f.elapsed >= run.flight_secs);
    }
    if dev.script_progression && run.frames.is_none() {
        asked = true;
        done &= script.is_some_and(|s| s.done());
    }
    if dev.script_ride && run.frames.is_none() {
        asked = true;
        done &= ride.is_some_and(|ride| ride.done());
    }
    if dev.script_survey.is_some() && run.frames.is_none() {
        asked = true;
        done &= survey.is_some_and(|survey| survey.done());
    }
    if dev.script_quest.is_some() && run.frames.is_none() {
        asked = true;
        done &= quest.is_some_and(|quest| quest.done());
    }
    if let Some(wanted) = dev.bench_frames {
        asked = true;
        done &= bench
            .as_deref()
            .is_some_and(|b| b.measured_frames() >= wanted);
    }
    if let Some(wanted) = dev.bench_secs {
        asked = true;
        done &= bench
            .as_deref()
            .is_some_and(|b| b.measured_secs() >= wanted);
    }
    if asked && done {
        let regions = loaded.map_or(0, |l| l.len());
        info!(
            "done after {} frames with {regions} regions loaded, exiting",
            frames.0
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(path), Some(bench)) = (&dev.bench_out, bench)
            && let Err(error) = write_bench(
                path,
                &bench,
                clock.as_deref(),
                regions,
                entities.iter().count(),
            )
        {
            error!(
                "writing the benchmark summary to {}: {error}",
                path.display()
            );
            exit.write(AppExit::error());
            return;
        }
        exit.write(AppExit::Success);
    }
}

/// The benchmark summary as a flat JSON object, for `tools/bench.py`.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_bench(
    path: &std::path::Path,
    bench: &BenchRecorder,
    clock: Option<&strata::client::stats::WorkClock>,
    regions: usize,
    entities: usize,
) -> std::io::Result<()> {
    let summary = bench.summary_with(clock).ok_or_else(|| {
        std::io::Error::other("no frame was measured; the run ended inside its warm-up")
    })?;
    let mut fields = vec![
        ("frames", summary.frames.to_string()),
        ("mean_ms", format!("{:.3}", summary.mean_ms)),
        ("p50_ms", format!("{:.3}", summary.p50_ms)),
        ("p95_ms", format!("{:.3}", summary.p95_ms)),
        ("p99_ms", format!("{:.3}", summary.p99_ms)),
        ("worst_ms", format!("{:.3}", summary.worst_ms)),
        ("over_33ms", summary.over_33ms.to_string()),
        ("gpu_mean_ms", format!("{:.3}", summary.gpu_mean_ms)),
        ("main_p50_ms", format!("{:.3}", summary.main_ms[0])),
        ("main_p95_ms", format!("{:.3}", summary.main_ms[1])),
        ("main_p99_ms", format!("{:.3}", summary.main_ms[2])),
        ("main_max_ms", format!("{:.3}", summary.main_ms[4])),
        ("render_p50_ms", format!("{:.3}", summary.render_ms[0])),
        ("render_p95_ms", format!("{:.3}", summary.render_ms[1])),
        ("render_p99_ms", format!("{:.3}", summary.render_ms[2])),
        ("render_max_ms", format!("{:.3}", summary.render_ms[4])),
        ("regions", regions.to_string()),
        ("entities", entities.to_string()),
        ("warmup_s", format!("{:.2}", summary.warmup_s)),
        ("warm_up", format!("\"{}\"", summary.warm_up.name())),
    ];
    if let Some(held) = summary.held_s {
        fields.push(("held_s", format!("{held:.2}")));
    }
    // The terrain and collider streamers over the whole measured stretch:
    // rates from totals, not medians of the log's two-second intervals.
    let s = &summary.stream;
    fields.extend([
        (
            "regions_pending_mean",
            format!("{:.2}", s.regions_pending_mean),
        ),
        ("regions_pending_max", s.regions_pending_max.to_string()),
        ("terrain_tasks_mean", format!("{:.2}", s.terrain_tasks_mean)),
        ("terrain_tasks_max", s.terrain_tasks_max.to_string()),
        (
            "terrain_uploads_per_frame",
            format!("{:.3}", s.uploads_per_frame),
        ),
        ("terrain_uploads_max_frame", s.uploads_max_frame.to_string()),
        ("terrain_uploads_per_s", format!("{:.2}", s.uploads_per_s)),
        (
            "terrain_cancelled_per_s",
            format!("{:.2}", s.cancelled_per_s),
        ),
        ("heightfields_per_s", format!("{:.2}", s.heightfields_per_s)),
        (
            "colliders_added_per_s",
            format!("{:.2}", s.colliders_added_per_s),
        ),
        (
            "colliders_removed_per_s",
            format!("{:.2}", s.colliders_removed_per_s),
        ),
    ]);
    // How far the observer went, which `tools/bench.py` holds a moving
    // fixture to.
    if let Some(travel) = summary.travel {
        fields.push(("travel_m", format!("{:.1}", travel.metres)));
        fields.push(("speed_mps", format!("{:.3}", travel.speed_mps)));
        if let Some(slowest) = travel.slowest_span_m {
            fields.push(("slowest_5s_m", format!("{slowest:.2}")));
        }
    }
    // The cost line's medians over the measured stretch, when one was
    // written after the warm-up.
    if let Some(cost) = summary.cost {
        for (key, value) in strata::client::stats::COST_KEYS.iter().zip(cost) {
            fields.push((*key, format!("{value:.3}")));
        }
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("\"{key}\": {value}"))
        .collect();
    let json = format!("{{{}}}\n", body.join(", "));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

/// A still of the stores is taken wherever the run stands, which is seldom at
/// a locker, so for that run the store is reached from anywhere; the same for
/// a still of the fitting or crafting screen and the workbench, and of the
/// factory and the fabricator, unless the script walks to one. The rules
/// themselves are the sim's and stay the rules.
pub fn install_access(app: &mut App, dev: &DevArgs) {
    if dev.open_stores {
        app.insert_resource(strata::sim::items::StoreAccess::Anywhere);
    }
    if (dev.open_fitting.is_some()
        || dev.open_crafting.is_some()
        || dev.craft_first.is_some()
        || dev.open_factory
        || dev.open_board
        || dev.factory_run.is_some()
        || dev.factory_poor_lot)
        && !dev.script_progression
        && !dev.script_crafting
    {
        app.insert_resource(strata::sim::fitting::WorkbenchAccess::Anywhere);
    }
    if dev.rework_first && !dev.script_progression {
        app.add_systems(Update, rework_first_for_shot);
    }
    if dev.craft_first.is_some() && !dev.script_progression {
        app.init_resource::<CraftFirst>()
            .add_systems(Update, craft_first_for_shot);
    }
    if !dev.press.is_empty() {
        app.add_systems(
            Update,
            press_for_shot.after(strata::client::ClientSet::Camera),
        );
    }
}
