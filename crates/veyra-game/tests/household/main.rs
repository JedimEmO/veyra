//! The household's tests that need no game binary: the file and its upgrades,
//! the bench and the fall, the runs, the harvesters, the props and the city's
//! walkability audit. The ones that build the whole app from a command line
//! or drive the scripts are `apps/game`'s own.

#![allow(
    clippy::wildcard_imports,
    reason = "the tests are split by subject and each sees what the others import"
)]

use bevy::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use strata::core::WorldPos;
use strata::prelude::*;
use strata::registry::GameRegistries;
use strata::registry::item::{Record, SavedPack};
use strata::sim::blueprint::Blueprints;
use strata::sim::combat::{CombatIntent, Weapon};
use strata::sim::crafting::CraftState;
use strata::sim::deeds::{CommonsTaken, Deeds, Leases};
use strata::sim::deposits::{Deposit, DepositId, Purse, Survey, Surveyed};
use strata::sim::encounters::{Cleared, PlayedClock};
use strata::sim::factory::{self, Benched, FactoryState};
use strata::sim::fitting::{self};
use strata::sim::harvest::Harvesters;
use strata::sim::items::{self, Hands, HouseholdStore, Pack, Stock};
use strata::sim::procurement::ProcurementState;
use strata::sim::progression::{CharacterSheet, Interaction, InteractionKind, Reach};

use strata::registry::schematic::Hand;
use strata::sim::factory::FactoryNotice;
use strata::sim::{blueprint, crafting};

use veyra_game::test_support::*;

use veyra_game::household::*;
// Named: `bevy::prelude::*` carries an `Out` of its own wherever picking is on.
use veyra_game::household::Out;

mod fall;
mod harvest;
mod nav;
mod props;
mod rework;
mod runs;
mod save;
