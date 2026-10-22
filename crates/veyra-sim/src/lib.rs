//! The game's simulation rules over the engine core in [`strata_app`]: one fixed-step world that the server runs authoritatively
//! and the client runs the same systems of, then renders (ADR 0001).
//!
//! The rules themselves (the plain functions and state types of combat, gear,
//! crafting, the land and the people) are `veyra-rules`; what lives here is
//! their Bevy half: each module is the rules' glob re-export plus the request
//! types, `consume` systems, plugins and system parameters that need
//! `bevy_app` or the engine core, with [`order::RequestSet`] the one order the
//! requests are answered in. The engine core itself (the app states, the
//! [`SimTick`] schedule and its sets, the sim clock, the `f64`
//! [`WorldPosition`] component, the [`Player`] and its intents, the
//! [`Authority`] port and the render origin) is `strata-app`, re-exported
//! here at its old paths; the `WorldPosition -> Transform` sync that reads the
//! origin sits below the render crates (ADR 0002).
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
pub mod combat;
pub mod crafting;
pub mod deeds;
pub mod deposits;
pub mod encounters;
pub mod equip;
pub mod factions;
pub mod factory;
pub mod fitting;
pub mod harvest;
pub mod items;
pub mod loot;
pub mod order;
pub mod postings;
pub mod procurement;
pub mod progression;
pub mod quests;
pub mod rework;
pub mod ride;
pub mod view;
pub mod wear;

// The plain rules moved to `veyra-rules`; the modules that are rules and nothing
// else are re-exported whole, the ones with an adapter half are files here that
// glob-import theirs.
pub use veyra_rules::{
    blueprint, chart, combat_ai, combat_curves, combat_loadout, combat_tables, combat_tiers, fall,
    household, standing,
};

// The engine core moved to `strata-app` so the render crates stop rebuilding
// with every game rule; the paths here stay so the crates above the sim need
// no change.
pub use progression::{
    CharacterSheet, EffectiveTables, ForgetRefused, Interact, Interaction, InteractionKind,
    LearnRefused, Reach, XpGained,
};
pub use strata_app::{Actor, AppRequestExt, Inbox, Request};
pub use strata_app::{
    AppPlugin, AppState, DayLength, DayStart, LoadingHold, Registries, SimClock, SimSet, SimTick,
    TimeOfDay, Wind, WorldSeed, authority, crossings, footprints, load, origin, player, position,
    request, roadside, sites, store, water,
};
pub use strata_app::{
    Authority, GeneratedRoot, Intent, IntentRejected, LastSynced, LocalAuthority, LocalPlayer,
    MassFetch, MassFetcher, Observers, OriginConfig, OriginSet, PREPARE_WATER_RADIUS_M,
    PlacedSettlement, Player, PlayerCommand, PlayerId, PlayerIntent, PrepareWater,
    PreviousWorldPosition, PrimaryObserver, ProceduralWorldRes, RenderOrigin, RenderOriginChanged,
    ScriptedIntent, Settlements, SimAuthority, TreeLibraryRes, WATER_INDEX, WEB_PREPARE_RADIUS_M,
    WaterFetcher, WaterHere, WorldAssetRoot, WorldLoadPlugin, WorldObserver, WorldPosition,
    WorldSourceRes, prepare_water_before_start, prepare_water_before_start_from, water_here,
    world_params, world_version, world_version_from,
};

use bevy::app::{App, Plugin};

/// Registers [`AppPlugin`] and then every game plugin: combat, progression,
/// the stock, the bench, the field and the world's people.
///
/// Kept as the one plugin the game, the server and the harness add, so the
/// engine core and the rules arrive together exactly as before the split.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AppPlugin)
            .add_plugins(order::OrderPlugin)
            .add_plugins((
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
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::state::state::State;
    use bevy::time::TimeUpdateStrategy;
    use strata_core::Seed;

    use super::*;

    /// An app that reaches `InWorld` with every game plugin and runs exactly
    /// one fixed step per update from then on.
    pub(crate) fn app_in_world() -> App {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.insert_resource(WorldSeed(Seed(42)));
        app.update();
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::InWorld
        );
        app
    }
}
