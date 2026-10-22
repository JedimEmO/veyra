//! The order the household's requests are answered in, written once.
//!
//! Every request kind has an inbox drained by a `consume` system in
//! [`SimSet::Input`]. Which of them sees the stock
//! first decides who wins a click that two of them could answer, so the
//! order is a rule of the game and not a property of the module that
//! happens to be built last. It used to be spelled as twelve `.after`
//! edges, each naming another module's `consume`, which made every module
//! depend on its neighbour just to be ordered against it; now each module
//! puts its systems in its own [`RequestSet`] and the one `chain` below
//! says how they follow each other.
//!
//! The order is the union of the edges it replaces: the stock, then the
//! fitting, the survey, the harvest, the bench, the factory, the wear, the
//! hands, the board, the bar, the rework, the loadout read back from the
//! hands, and the ride last. Sets that no edge ordered before (the harvest
//! against the bench, say) now follow the position given here.

use bevy::app::{App, Plugin};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};

use crate::{SimSet, SimTick, player};

/// One request kind's systems, in the order they run in the `Input` set of
/// the tick. A set holds a module's `consume` and the systems that must
/// follow it directly (a harvester's `run`, a bar's `follow_sheet`).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestSet {
    /// `items::consume`: a click on the pack, the store or a hand.
    Stock,
    /// `fitting::consume`: a part fitted or taken off.
    Fitting,
    /// `deposits::consume`: a ping, a sample, a track.
    Survey,
    /// `harvest::consume` and the harvesters' run.
    Harvest,
    /// `loot::consume`: a cache emptied into the pack.
    Loot,
    /// `crafting::consume`: the bench's session, its rolls and its buys.
    Crafting,
    /// `factory::consume` and the run it starts.
    Factory,
    /// `wear::consume`: a repair.
    Wear,
    /// `equip::consume`: a weapon taken into a hand.
    Equip,
    /// `postings::consume`: a job taken or collected, before the board
    /// reads the purse.
    Postings,
    /// `procurement::consume`: a sale, a purchase, a lease.
    Procurement,
    /// `ability::consume`: the bar's slots.
    Ability,
    /// `rework::consume`: a mount reworked.
    Rework,
    /// `fitting::sync_loadout`: the combatant's row read back from the
    /// hands, after everything that changes a hand or a part.
    Loadout,
    /// `ride::consume`: a call or a dismount.
    Ride,
}

/// The household's copy of what the sim changed (`veyra-game`'s tracking
/// chain: the sheet, the bar, the stock, the quest log, then the file). A
/// screen that words the sim's answer runs after it, so a roll's counter is
/// in the file by the time its outcome can be read. A set here and not a
/// system name because the screens link the sim and not the household.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HouseholdTracked;

/// Declares the order of [`RequestSet`] once; added by `SimPlugin` before
/// any module's plugin.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrderPlugin;

impl Plugin for OrderPlugin {
    fn build(&self, app: &mut App) {
        use RequestSet::{
            Ability, Crafting, Equip, Factory, Fitting, Harvest, Loadout, Loot, Postings,
            Procurement, Rework, Ride, Stock, Survey, Wear,
        };
        app.configure_sets(
            SimTick,
            (
                Stock,
                Fitting,
                Survey,
                Harvest,
                Loot,
                Crafting,
                Factory,
                Wear,
                Equip,
                Postings,
                Procurement,
                Ability,
                Rework,
                Loadout,
                Ride,
            )
                .chain()
                .in_set(SimSet::Input)
                .after(player::consume_player_intents),
        );
    }
}
