//! The game's headless half: the household (its characters, stock, survey
//! record and file), the questline's people, the outlaw camps and the
//! factions' posts, the props the household's places stand and the hostiles'
//! making, over `veyra-sim` and the headless facade.
//!
//! It is the part of the game binary a dedicated server needs as well:
//! without it a server has no household, no people and no camps, and `Reach`
//! is never filled by a workbench, so the crafting, factory, board and quest
//! rules it links can never be satisfied. `apps/game` links it beside its
//! window, scripts, stills and the art of the props; `apps/server` links it
//! beside nothing that draws. `just server-check` asserts that nothing in its
//! graph does.
//!
//! The household is the game's, not the sim's, and local: it is saved as RON
//! to a file, nothing in it is replicated, and the sim knows one local player
//! with one sheet, pack and store. A server runs it for that one player and
//! leaves the many for the next structural step (household resources to
//! components).

pub mod camps;
pub mod deposit_start;
pub mod deposits;
pub mod factions;
pub mod hostiles;
pub mod household;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use bevy::prelude::*;
use strata::prelude::{AppState, SimSet, SimTick};

pub use household::HouseholdFile;

/// Height of the player's feet above the ground at spawn: a short drop, so
/// the first thing the character does is land.
pub const PLAYER_DROP: f64 = 2.0;

/// The player's capsule height in the combat build, metres: a head taller
/// than the controller's default, which the city's walkability audit stands
/// in every doorway and stair as the player would.
pub const PLAYER_HEIGHT_M: f32 = 1.8;

/// The frame a still opens its screen on: late enough for the terrain around
/// the spawn to be drawn under it.
pub const OPEN_PAUSE_FRAME: u32 = 120;

/// `--hold-guard`: set while a still holds the guard up, which the scripted
/// fight reads so its own presses do not feed the latch. Written by `veyra-dev`.
#[derive(Resource, Default)]
pub struct GuardHold(pub bool);

/// What the command line asked of the headless side. The game binary builds
/// it from its flags and a server from its own; the systems here read this
/// and never a binary's `Cli`.
#[derive(Resource, Debug, Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one field a flag of the run, each its own switch"
)]
pub struct Launch {
    /// Combat play: the household exists. A connected client or a flycam has
    /// none.
    pub combat: bool,
    /// No window: the notices that are the screens' are not written.
    pub headless: bool,
    /// `--frames`: the run ends after this many.
    pub frames: Option<u32>,
    /// `--at`: a named place or deposit to start at, cleared once resolved.
    pub at: Option<String>,
    /// `--spawn`: an exact start.
    pub spawn: Option<(f64, f64)>,
    /// The scripted combat run.
    pub combat_script: bool,
    /// `--script-survey`: the class it surveys.
    pub script_survey: Option<String>,
    /// `--script-ride`.
    pub script_ride: bool,
    /// `--script-quest`: the ending it walks to.
    pub script_quest: Option<String>,
    /// `--open-dialogue`: the person whose conversation is a still.
    pub open_dialogue: Option<String>,
    /// `--quest-stage`: a quest and the stage it starts at.
    pub quest_stage: Option<(String, String)>,
    /// `--fall`: the member falls at a fixed frame.
    pub fall: bool,
    /// `--injured`: the member who starts hurt.
    pub injured: Option<String>,
    /// `--camp-tier`: the tier every camp is forced to.
    pub camp_tier: Option<u8>,
    /// The sim is handed the world's deposits: the household draws its first
    /// lots from them, or the run starts at one.
    pub deposits: bool,
}

/// The order of the headless side's start and exit, for a binary to put its
/// own systems between.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameSet {
    /// On entering the world: the deposits and the factions are handed to the
    /// sim.
    World,
    /// On entering the world, after [`GameSet::World`]: the household is read
    /// or started and the questline's cast is found.
    Household,
    /// In `Last`: the household is written out when the app ends.
    SaveOnExit,
}

/// The headless game: the household's systems, the camps and posts, and the
/// start chain. Needs the headless facade's plugins under it.
#[derive(Debug, Clone)]
pub struct GamePlugin {
    launch: Launch,
    file: HouseholdFile,
}

impl GamePlugin {
    /// The plugin for a run that asked for `launch`, with the household kept
    /// as `file` says.
    #[must_use]
    pub const fn new(launch: Launch, file: HouseholdFile) -> Self {
        Self { launch, file }
    }
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        let launch = &self.launch;
        app.insert_resource(launch.clone());
        app.configure_sets(
            OnEnter(AppState::InWorld),
            (GameSet::World, GameSet::Household).chain(),
        )
        .add_systems(
            OnEnter(AppState::InWorld),
            (deposits::init, factions::init)
                .chain()
                .in_set(GameSet::World),
        )
        .add_systems(
            OnEnter(AppState::InWorld),
            (
                household::init.run_if(resource_exists::<HouseholdFile>),
                household::people::locate.run_if(resource_exists::<HouseholdFile>),
            )
                .chain()
                .in_set(GameSet::Household),
        );
        if !launch.combat {
            return;
        }
        app.insert_resource(self.file.clone());
        household::install(app, launch);
        let forced = launch.camp_tier;
        app.init_resource::<camps::Waiting>()
            .add_systems(
                Startup,
                move |mut near: ResMut<strata::sim::encounters::NearEncounters>| {
                    near.forced_tier = forced;
                },
            )
            .add_systems(
                Update,
                camps::stage_camps
                    .after(strata::sim::encounters::update_near_encounters)
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                SimTick,
                camps::sync_post_teams
                    .in_set(SimSet::Input)
                    .before(strata::sim::player::apply_scripted_intents),
            );
    }
}
