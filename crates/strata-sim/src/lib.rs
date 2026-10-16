//! The simulation: one fixed-step world that the server runs authoritatively
//! and the client runs the same systems of, then renders (ADR 0001).
//!
//! What lives here is what both ends share and neither renders: the app
//! states, the [`SimTick`] schedule and its sets, the sim clock, the `f64`
//! [`WorldPosition`] component, the [`Player`] and its intents, the
//! [`Authority`] port and the render origin. The render origin is a client concern, but the
//! `WorldPosition -> Transform` sync that reads it has to sit below the
//! render crates (ADR 0002), so it is declared and run from here.
//!
//! # Schedules
//!
//! ```text
//! FixedUpdate (Time<Fixed> at SIM_TICK_HZ, 20 Hz)
//!   run_sim_tick                    runs the SimTick schedule once, in InWorld
//!     SimTick
//!       SimSet::Input               previous positions recorded, accepted intents applied,
//!                                   PlayerIntent consumed into PlayerCommand
//!       SimSet::Step                the clock advances, time of day is derived; physics
//!                                   (strata-physics) moves the players here
//!       SimSet::Observe             Observers is refreshed from WorldObserver entities
//!
//! PostUpdate, before TransformSystems::Propagate, in InWorld
//!   OriginSet::Observe              Observers refreshed again (the camera moved in Update)
//!   OriginSet::Rebase               RenderOrigin follows the primary observer past the threshold
//!   OriginSet::Sync                 Transform.translation = (world - origin) as f32
//! ```
//!
//! [`SimTick`] is its own schedule rather than systems placed straight in
//! `FixedUpdate` so the client can later run it ahead of the server for
//! prediction by calling `run_schedule(SimTick)` extra times; `FixedUpdate`
//! merely drives it at the fixed rate.
//!
//! # Positions
//!
//! Anything that exists in the world carries a [`WorldPosition`]. An entity
//! that also renders carries a `Transform`, which [`OriginSet::Sync`] writes
//! every frame from the position and the [`RenderOrigin`]. Writing such a
//! `Transform.translation` by hand is a bug, and a debug-build assertion in
//! the sync system says so. Move things by writing `WorldPosition`.
//!
//! # Loading
//!
//! `Boot` leaves immediately. `LoadingRegistries` leaves once nothing holds
//! a [`LoadingHold`]: a plugin that loads game data takes the hold when it
//! is built, inserts [`Registries`] when its data is frozen, and releases
//! it. [`WorldLoadPlugin`] is that plugin for the game: it reads the asset
//! root, freezes the registries, builds the world source and publishes it
//! as [`WorldSourceRes`]. With no loader at all (a bare test app) the state
//! falls through and an empty [`Registries`] is inserted so `InWorld`
//! systems can rely on it.

pub mod ability;
pub mod authority;
pub mod blueprint;
pub mod chart;
pub mod combat;
pub mod combat_ai;
pub mod combat_curves;
pub mod combat_tables;
pub mod combat_tiers;
pub mod crafting;
pub mod crossings;
pub mod deeds;
pub mod deposits;
pub mod encounters;
pub mod equip;
pub mod factions;
pub mod factory;
pub mod fall;
pub mod fitting;
pub mod footprints;
pub mod harvest;
pub mod items;
pub mod load;
pub mod loot;
pub mod origin;
pub mod player;
pub mod position;
pub mod postings;
pub mod procurement;
pub mod progression;
pub mod quests;
pub mod rework;
pub mod ride;
pub mod roadside;
pub mod sites;
pub mod store;
pub mod water;
pub mod wear;

use std::sync::Arc;

use bevy::app::{App, FixedUpdate, Plugin, PostUpdate, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::{IntoScheduleConfigs, ScheduleLabel, SystemSet};
use bevy::state::app::{AppExtStates, StatesPlugin};
use bevy::state::condition::in_state;
use bevy::state::state::{NextState, OnEnter, States};
use bevy::time::{Fixed, Time};
use bevy::transform::TransformSystems;
use strata_core::{SIM_TICK_HZ, Seed, SimTime};
use strata_registry::GameRegistries;

pub use authority::{Authority, Intent, IntentRejected, LocalAuthority, SimAuthority};
pub use load::{
    GeneratedRoot, PREPARE_WATER_RADIUS_M, PlacedSettlement, PrepareWater, ProceduralWorldRes,
    Settlements, TreeLibraryRes, WATER_FILE, WorldAssetRoot, WorldLoadPlugin, WorldSourceRes,
    bake_water_file, prepare_water_before_start, prepare_water_before_start_from, world_params,
    world_version, world_version_from,
};
pub use origin::{LastSynced, OriginConfig, OriginSet, RenderOrigin, RenderOriginChanged};
pub use player::{
    LocalPlayer, Player, PlayerCommand, PlayerId, PlayerIntent, PreviousWorldPosition,
    ScriptedIntent,
};
pub use position::{Observers, PrimaryObserver, WorldObserver, WorldPosition};
pub use progression::{
    CharacterSheet, EffectiveTables, ForgetRefused, Interact, Interaction, InteractionKind,
    LearnRefused, Reach, XpGained,
};
pub use water::{WaterHere, water_here};

/// The lifecycle of the app, headless or not.
///
/// `Boot`: plugins register, the seed and launch options become resources.
/// `LoadingRegistries`: game data loads and freezes; the world source is
/// built. `InWorld`: the loop in PLAN section 4.1 runs. Nothing samples the
/// world before `InWorld`, and the registries phase is where that promise is
/// enforced.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Plugins registered; resources being inserted.
    #[default]
    Boot,
    /// Registries loading and freezing.
    LoadingRegistries,
    /// The world exists.
    InWorld,
}

/// The fixed-step simulation schedule, run once per fixed step from
/// `FixedUpdate` while [`AppState::InWorld`].
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SimTick;

/// The simulation sets, in order, in [`SimTick`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// Apply accepted intents to sim state.
    Input,
    /// Movement, time of day, later AI and combat.
    Step,
    /// Update observer positions from sim state, for the streamer.
    Observe,
}

/// The world seed, as a resource. Inserted by the app, not by [`SimPlugin`],
/// because it comes from the command line or a save.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSeed(pub Seed);

/// The simulation clock, advanced once per fixed step in [`SimSet::Step`].
///
/// A tick count, not wall time: two machines that have stepped the same
/// number of times agree exactly (see [`SimTime`]).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct SimClock(pub SimTime);

impl SimClock {
    /// Ticks since the world started.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.0.tick
    }

    /// Seconds since the world started.
    #[must_use]
    pub fn elapsed_secs(&self) -> f64 {
        self.0.elapsed_secs()
    }
}

/// How long a day lasts, in seconds of simulation time.
///
/// 24 minutes by default, so a session sees a full cycle. The atmosphere
/// crate reads [`TimeOfDay`], which is derived from the clock and this every
/// step, rather than computing it from the clock itself, so there is one
/// definition of "noon".
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct DayLength(pub f64);

impl DayLength {
    /// 24 minutes.
    pub const DEFAULT_SECS: f64 = 24.0 * 60.0;
}

impl Default for DayLength {
    fn default() -> Self {
        Self(Self::DEFAULT_SECS)
    }
}

/// The wind, as the simulation owns it (ADR 0005): a client draws foliage
/// bending to it now, a server's weather will drive it later. Direction is
/// horizontal `(x, z)` and unit length; strength is metres of tip
/// displacement for a one metre plant at full gust; the gust scale is the
/// metres per cell of the travelling gust field.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Wind {
    /// Unit horizontal direction, `x` then `z`.
    pub direction: [f32; 2],
    /// Metres at the tip of a one metre plant at full gust.
    pub strength: f32,
    /// Metres per gust cell.
    pub gust_scale_m: f32,
}

impl Default for Wind {
    fn default() -> Self {
        Self {
            direction: [0.8, 0.6],
            strength: 0.35,
            gust_scale_m: 40.0,
        }
    }
}

/// Where in the day tick 0 falls, in `[0, 1)`: 0 is midnight, 0.25
/// sunrise, 0.5 noon. Sim state, not a client setting: a server that starts
/// its world at dawn tells every client so through this, and a debug scrub
/// of the time of day moves it.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct DayStart(pub f64);

impl Default for DayStart {
    fn default() -> Self {
        // Mid-morning: high enough for a bright day, low enough that the
        // sun casts shadows worth looking at.
        Self(0.42)
    }
}

/// Where in the day the simulation is, in `[0, 1)`: 0 is midnight, 0.5 is
/// noon. Derived in [`SimSet::Step`] from [`SimClock`], [`DayLength`] and
/// [`DayStart`]; the atmosphere reads it and never the clock.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct TimeOfDay(pub f64);

impl TimeOfDay {
    /// The fraction `elapsed` seconds into a day of `length` seconds that
    /// began at `start`.
    #[must_use]
    pub fn at(elapsed_secs: f64, length: &DayLength, start: &DayStart) -> Self {
        Self((elapsed_secs / length.0 + start.0).rem_euclid(1.0))
    }

    /// Hours and minutes on a 24 hour clock, for a HUD.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the fraction is in [0, 1) so the minute count fits"
    )]
    pub fn hours_minutes(&self) -> (u32, u32) {
        let minutes = (self.0.rem_euclid(1.0) * 24.0 * 60.0) as u32;
        (minutes / 60, minutes % 60)
    }
}

/// Outstanding loaders keeping the app in [`AppState::LoadingRegistries`].
///
/// A loading plugin increments this in `Boot` and decrements it when its
/// data is frozen; the state leaves when the count is zero. A count rather
/// than a flag so two loaders (biomes, materials) can wait independently.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadingHold(pub u32);

/// The frozen game registries, shared read-only across threads.
///
/// Inserted by the loader before it releases its [`LoadingHold`], or as an
/// empty set on entering `InWorld` when nothing loaded anything, so systems
/// in the world can take `Res<Registries>` unconditionally. An `Arc` because
/// terrain meshing tasks on the compute pool need to read it as well.
#[derive(Resource, Debug, Clone)]
pub struct Registries(pub Arc<GameRegistries>);

impl Default for Registries {
    fn default() -> Self {
        Self(Arc::new(GameRegistries::default()))
    }
}

/// Registers the states, the [`SimTick`] schedule and its sets, the clock,
/// the authority, the observers and the render origin, and sets the fixed
/// timestep to [`SIM_TICK_HZ`].
///
/// Adds Bevy's `StatesPlugin` when the app has not, so the same plugin works
/// under `MinimalPlugins` (headless) and `DefaultPlugins` (client). The
/// origin sets are ordered before `TransformSystems::Propagate`, which only
/// exists when `TransformPlugin` is present; Bevy tolerates ordering against
/// an empty set, so a headless app is unaffected.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            combat::CombatPlugin,
            ability::AbilityPlugin,
            progression::ProgressionPlugin,
            items::ItemsPlugin,
            fitting::FittingPlugin,
            wear::WearPlugin,
            equip::EquipPlugin,
            rework::ReworkPlugin,
            ride::RidePlugin,
            procurement::ProcurementPlugin,
            deposits::DepositsPlugin,
            factions::FactionsPlugin,
            crafting::CraftingPlugin,
            factory::FactoryPlugin,
            harvest::HarvestPlugin,
        ))
        .add_plugins((
            encounters::EncountersPlugin,
            deeds::DeedsPlugin,
            loot::LootPlugin,
            postings::PostingsPlugin,
            quests::QuestsPlugin,
        ));
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.init_state::<AppState>()
            .insert_resource(Time::<Fixed>::from_hz(SIM_TICK_HZ))
            .init_resource::<SimClock>()
            .init_resource::<DayLength>()
            .init_resource::<Wind>()
            .init_resource::<DayStart>()
            .init_resource::<TimeOfDay>()
            .init_resource::<SimAuthority>()
            .init_resource::<Observers>()
            .init_resource::<RenderOrigin>()
            .init_resource::<OriginConfig>()
            .add_message::<RenderOriginChanged>()
            .init_schedule(SimTick)
            .configure_sets(
                SimTick,
                (SimSet::Input, SimSet::Step, SimSet::Observe).chain(),
            )
            .add_systems(
                SimTick,
                (
                    (
                        player::record_previous_positions,
                        authority::apply_intents,
                        player::apply_scripted_intents,
                        player::consume_player_intents,
                    )
                        .chain()
                        .in_set(SimSet::Input),
                    (advance_clock, derive_time_of_day)
                        .chain()
                        .in_set(SimSet::Step),
                    position::collect_observers.in_set(SimSet::Observe),
                ),
            )
            .add_systems(
                FixedUpdate,
                run_sim_tick.run_if(in_state(AppState::InWorld)),
            )
            .configure_sets(
                PostUpdate,
                (OriginSet::Observe, OriginSet::Rebase, OriginSet::Sync)
                    .chain()
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                PostUpdate,
                (
                    position::collect_observers.in_set(OriginSet::Observe),
                    origin::rebase_origin.in_set(OriginSet::Rebase),
                    origin::sync_transforms.in_set(OriginSet::Sync),
                ),
            )
            .add_systems(
                Update,
                (
                    leave_boot.run_if(in_state(AppState::Boot)),
                    finish_loading.run_if(in_state(AppState::LoadingRegistries)),
                ),
            )
            .add_systems(OnEnter(AppState::InWorld), ensure_registries);
    }
}

/// One fixed step of the simulation.
fn run_sim_tick(world: &mut World) {
    world.run_schedule(SimTick);
}

fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.0.advance();
}

fn derive_time_of_day(
    clock: Res<SimClock>,
    day: Res<DayLength>,
    start: Res<DayStart>,
    mut tod: ResMut<TimeOfDay>,
) {
    *tod = TimeOfDay::at(clock.elapsed_secs(), &day, &start);
}

fn leave_boot(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::LoadingRegistries);
}

/// Leaves `LoadingRegistries` once no loader holds it. A missing
/// [`LoadingHold`] means nothing registered a loader, which is the bare
/// test app and, until the data phase lands, the game itself.
fn finish_loading(hold: Option<Res<LoadingHold>>, mut next: ResMut<NextState<AppState>>) {
    if hold.is_none_or(|h| h.0 == 0) {
        next.set(AppState::InWorld);
    }
}

fn ensure_registries(mut commands: Commands, registries: Option<Res<Registries>>) {
    if registries.is_none() {
        tracing::info!("no registries were loaded; entering the world with an empty set");
        commands.init_resource::<Registries>();
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use bevy::state::state::State;
    use bevy::time::TimeUpdateStrategy;
    use strata_core::WorldPos;

    fn state(app: &App) -> AppState {
        *app.world().resource::<State<AppState>>().get()
    }

    #[allow(clippy::cast_precision_loss, reason = "test tick counts are tiny")]
    fn seconds(tick: u64) -> f64 {
        tick as f64 / SIM_TICK_HZ
    }

    /// An app that reaches `InWorld` and runs exactly one fixed step per
    /// update from then on. The first `TimePlugin` update has a zero delta,
    /// which the three state hops absorb.
    pub(crate) fn app_in_world() -> App {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.insert_resource(WorldSeed(Seed(42)));
        app.update();
        app.update();
        app.update();
        assert_eq!(state(&app), AppState::InWorld);
        app
    }

    #[test]
    fn boots_into_the_world() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, SimPlugin));
        app.insert_resource(WorldSeed(Seed(42)));
        // A `NextState` set in `Update` applies in the next frame's
        // `StateTransition`, so each hop is visible one update later.
        assert_eq!(state(&app), AppState::Boot);
        app.update();
        assert_eq!(state(&app), AppState::Boot);
        app.update();
        assert_eq!(state(&app), AppState::LoadingRegistries);
        app.update();
        assert_eq!(state(&app), AppState::InWorld);
        assert_eq!(app.world().resource::<RenderOrigin>().0, WorldPos::ZERO);
        assert!(app.world().get_resource::<Registries>().is_some());
    }

    #[test]
    fn a_loading_hold_keeps_the_app_out_of_the_world() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, SimPlugin));
        app.insert_resource(LoadingHold(1));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(state(&app), AppState::LoadingRegistries);
        assert!(app.world().get_resource::<Registries>().is_none());
        app.world_mut().resource_mut::<LoadingHold>().0 = 0;
        app.update();
        app.update();
        assert_eq!(state(&app), AppState::InWorld);
        assert!(app.world().get_resource::<Registries>().is_some());
    }

    #[test]
    fn the_clock_ticks_once_per_fixed_step() {
        let mut app = app_in_world();
        let before = app.world().resource::<SimClock>().tick();
        for _ in 0..20 {
            app.update();
        }
        let clock = *app.world().resource::<SimClock>();
        assert_eq!(clock.tick(), before + 20);
        assert!((clock.elapsed_secs() - seconds(clock.tick())).abs() < 1e-12);
    }

    #[test]
    fn fixed_steps_follow_the_fixed_clock_not_the_frame_rate() {
        let mut app = app_in_world();
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(3));
        let before = app.world().resource::<SimClock>().tick();
        app.update();
        assert_eq!(app.world().resource::<SimClock>().tick(), before + 3);
    }

    #[test]
    fn time_of_day_wraps_over_the_day_length() {
        let mut app = app_in_world();
        // A four-second day: after ten updates at 20 Hz (half a second) the
        // day is an eighth done; after eighty it has wrapped.
        app.insert_resource(DayLength(4.0));
        app.insert_resource(DayStart(0.25));
        let start = app.world().resource::<SimClock>().tick();
        for _ in 0..10 {
            app.update();
        }
        let tod = app.world().resource::<TimeOfDay>().0;
        let expected = (seconds(start + 10) / 4.0 + 0.25).rem_euclid(1.0);
        assert!((tod - expected).abs() < 1e-12, "{tod} vs {expected}");
        for _ in 0..80 {
            app.update();
        }
        let tod = app.world().resource::<TimeOfDay>().0;
        assert!((0.0..1.0).contains(&tod));
        assert_eq!(TimeOfDay(0.5).hours_minutes(), (12, 0));
        assert_eq!(TimeOfDay(0.42).hours_minutes(), (10, 4));
    }

    #[test]
    fn nothing_ticks_before_the_world_exists() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.insert_resource(LoadingHold(1));
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<SimClock>().tick(), 0);
    }
}
