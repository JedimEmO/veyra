//! The household's scripts and the stills' set-ups: `--script-progression`
//! walks the household's path without a key or a screen, learning and
//! swapping directly; `--script-crafting` adds a stop at the workbench
//! ([`script_bench`]), the crafting chain ([`script_craft`]), a fabricator's
//! run ([`script_factory`]), a survey ([`script_survey`]) and a sale
//! ([`script_sale`]); `--script-quest` plays the first questline. The headless
//! test of each drives the same systems the game installs.

#![allow(
    clippy::wildcard_imports,
    reason = "the files are one module split for size, and each sees what the others own"
)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use strata::core::WorldPos;
use strata::prelude::*;
use strata::registry::Attribute;
use strata::registry::GameRegistries;
use strata::registry::item::{Fitted, ItemCatalogue, ItemKind, Record, STORE_GRADE};
use strata::registry::progression::SkillGraph;
use strata::registry::schematic::Hand;
use strata::sim::blueprint::{self, Blueprints};
use strata::sim::combat::{Action, CombatEncounter, CombatIntent, Combatant, Weapon, WeaponSpec};
use strata::sim::crafting::{
    self, CraftAnswer, CraftDone, CraftInbox, CraftRequest, CraftState, Holder,
};
use strata::sim::deeds::Deeds;
use strata::sim::deposits::{
    self, Deposit, DepositId, NearDeposits, PingAnswer, PingDone, Purse, SampleAnswer, SampleDone,
    SurveyInbox, SurveyRequest,
};
use strata::sim::equip::{EquipAnswer, EquipInbox, EquipRequest};
use strata::sim::factory::{
    self, Benched, FactoryAnswer, FactoryDone, FactoryInbox, FactoryNotice, FactoryRequest,
    FactoryState,
};
use strata::sim::fitting::{FitAnswer, FitInbox, FitRequest};
use strata::sim::items::{
    self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockInbox, StockRequest,
};
use strata::sim::progression::XpGained;
use strata::sim::progression::{CharacterSheet, Interact, Interaction, InteractionKind, Reach};
use strata::sim::rework::{ReworkAnswer, ReworkInbox, ReworkRequest};
use strata::sim::view::ProgressCue;

use crate::DevArgs;
use veyra_game::out;

pub mod script;
pub mod script_crafting;
pub mod script_quest;
pub mod shots;

pub use veyra_game::household::*;

pub use self::script::*;
pub use self::script_crafting::*;
pub use self::shots::*;

/// The blade the scripted fight cuts with: the scripted player cannot cut
/// with a gun in hand.
pub const SCRIPT_BLADE: &str = "veyra:weapon.service_blade";

/// What the development flags say about the household's start, on top of the
/// file and `--fresh`, which are the game's own.
pub fn dress_file(file: &mut HouseholdFile, dev: &DevArgs) {
    file.fitted.clone_from(&dev.fitted);
    file.in_hand = if dev.in_hand.is_empty() && dev.combat_script {
        vec!["-".to_owned(), SCRIPT_BLADE.to_owned()]
    } else {
        dev.in_hand.clone()
    };
    file.learn.clone_from(&dev.learn);
    file.grant.clone_from(&dev.grant);
    file.condition = dev.condition;
    file.member.clone_from(&dev.member);
    file.crafted_carbine = dev
        .crafted_carbine
        .map(|moddability| (moddability, dev.open_fitting.is_some()));
    file.blanks = dev.blanks;
    file.give.clone_from(&dev.give);
    file.bike = dev.bike;
    file.bike_condition = dev.bike_condition;
    file.purse = dev.purse;
    file.saddle_stock = dev.saddle_stock;
    file.harvester = dev.harvester;
    file.harvest_hours = dev.harvest_hours;
    file.open_harvester = dev.open_harvester;
    file.quest_stage.clone_from(&dev.quest_stage);
}

/// The household's scripts and the shots' systems, beside the headless
/// systems `veyra_game::GamePlugin` adds: only in combat play, as the
/// household is (the caller asks).
#[allow(
    clippy::too_many_lines,
    reason = "one list of every system the scripts and the stills add"
)]
pub fn install(app: &mut App, dev: &DevArgs, headless: bool) {
    app.add_systems(
        SimTick,
        (script_learn, script_swap)
            .chain()
            .run_if(resource_exists::<ProgressionScript>)
            .run_if(resource_exists::<Household>)
            .in_set(SimSet::Input)
            .after(strata::sim::player::apply_scripted_intents),
    )
    .add_systems(
        SimTick,
        script_hurt
            .run_if(resource_exists::<ProgressionScript>)
            .in_set(SimSet::Input)
            .after(strata::sim::player::apply_scripted_intents),
    );
    install_factory_shots(app, dev);
    if let Some(ending) = &dev.script_quest {
        script_quest::install(app, dev, ending);
    }
    if dev.script_progression {
        app.insert_resource(ProgressionScript {
            open_sheet: dev.open_sheet,
            open_bench: dev.open_bench,
            crafting: dev.script_crafting,
            hurts: dev.script_hurt,
            ..default()
        });
    }
    if dev.script_crafting {
        // A screen is opened only where there is one: a headless run has
        // no UI for the flag to open.
        let open_crafting = dev
            .open_crafting
            .clone()
            .filter(|_| !headless)
            .map(|schematic| (schematic, dev.crafting_still()));
        app.insert_resource(CraftingScript::new(
            dev.script_report.clone(),
            dev.open_fitting.is_some(),
            open_crafting,
        ))
        .add_systems(
            SimTick,
            (
                script_carbine,
                script_bench,
                script_craft,
                script_factory,
                script_survey,
                script_sale,
            )
                .chain()
                .run_if(resource_exists::<ProgressionScript>)
                .in_set(SimSet::Input)
                .after(strata::sim::player::apply_scripted_intents),
        );
    }
}
