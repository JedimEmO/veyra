//! The household in the game binary: what is left of it here is what the server
//! has no use for. The household itself (the characters, their file, the
//! bench, the fall, the questline's people, the props' placement) is
//! `veyra_game::household`, re-exported below so the stills that drive it name
//! it as they always did; here are the art and labels of the props and the
//! harvesters. The scripts (`--script-progression`, `--script-crafting`,
//! `--script-quest`) and the shots' set-ups are `veyra_dev::household`,
//! behind the `dev-tools` feature.

#![allow(
    clippy::wildcard_imports,
    reason = "the files are one module split for size, and each sees what the others own"
)]

use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::progression::{Interact, Interaction, InteractionKind};

mod harvester_art;
mod props_art;
#[cfg(all(test, feature = "dev-tools"))]
mod tests;

pub(crate) use veyra_game::household::*;

#[cfg(all(test, feature = "dev-tools"))]
pub(crate) use veyra_dev::household::*;

/// What the household's tests name unqualified: the sim's types the scripts
/// and the stills were written against.
#[cfg(all(test, feature = "dev-tools"))]
mod test_prelude {
    #![allow(unused_imports, reason = "each test file names its own share")]
    pub use bevy::prelude::*;
    pub use serde::{Deserialize, Serialize};
    pub use std::collections::BTreeMap;
    pub use std::path::PathBuf;
    pub use strata::core::WorldPos;
    pub use strata::prelude::*;
    pub use strata::registry::Attribute;
    pub use strata::registry::GameRegistries;
    pub use strata::registry::item::{Fitted, ItemCatalogue, ItemKind, Record, STORE_GRADE};
    pub use strata::registry::progression::SkillGraph;
    pub use strata::registry::schematic::Hand;
    pub use strata::sim::blueprint::{self, Blueprints};
    pub use strata::sim::combat::{
        Action, CombatEncounter, CombatIntent, Combatant, Weapon, WeaponSpec,
    };
    pub use strata::sim::crafting::{
        self, CraftAnswer, CraftDone, CraftInbox, CraftRequest, CraftState, Holder,
    };
    pub use strata::sim::deeds::Deeds;
    pub use strata::sim::deposits::{
        self, Deposit, DepositId, NearDeposits, PingAnswer, PingDone, Purse, SampleAnswer,
        SampleDone, SurveyInbox, SurveyRequest,
    };
    pub use strata::sim::equip::{EquipAnswer, EquipInbox, EquipRequest};
    pub use strata::sim::factory::{
        self, Benched, FactoryAnswer, FactoryDone, FactoryInbox, FactoryNotice, FactoryRequest,
        FactoryState,
    };
    pub use strata::sim::fitting::{FitAnswer, FitInbox, FitRequest};
    pub use strata::sim::items::{
        self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockInbox, StockRequest,
    };
    pub use strata::sim::progression::XpGained;
    pub use strata::sim::progression::{
        CharacterSheet, Interact, Interaction, InteractionKind, Reach,
    };
    pub use strata::sim::rework::{ReworkAnswer, ReworkInbox, ReworkRequest};
    pub use strata::sim::view::ProgressCue;

    pub use veyra_game::out;
}

#[cfg(all(test, feature = "dev-tools"))]
pub(crate) use self::test_prelude::*;

use self::harvester_art::*;
use self::props_art::*;

pub(crate) use self::props_art::PropKit;

/// What the command line says about the household's file: the file and
/// `--fresh` are the game's, the rest the development flags'.
pub(crate) fn household_file(cli: &super::Cli) -> HouseholdFile {
    #[allow(unused_mut, reason = "only the development flags dress it")]
    let mut file = HouseholdFile {
        path: cli.household.clone(),
        fresh: cli.fresh,
        ..default()
    };
    #[cfg(feature = "dev-tools")]
    veyra_dev::household::dress_file(&mut file, &cli.dev);
    file
}

/// The household's art and labels, in a window and in combat play, as the
/// household is.
pub(super) fn install(app: &mut App, cli: &super::Cli) {
    if !cli.combat {
        return;
    }
    // The art, the labels and the screens a flag opens are the window's.
    if !cli.headless {
        app.add_systems(Startup, load_prop_art)
            .add_systems(Update, dress_props.run_if(resource_exists::<PropArt>))
            .add_systems(
                Update,
                (
                    label_props.run_if(strata::client::ui::game_input_allowed),
                    hide_labels.run_if(not(strata::client::ui::game_input_allowed)),
                )
                    .run_if(in_state(AppState::InWorld)),
            );
        app.add_systems(Update, show_props);
        install_harvester_art(app);
    }
}

/// The props wear `Visibility` for the art parented to them; the headless
/// placement stands them without it, since `Visibility` is the camera crate's.
pub(super) fn show_props(
    mut commands: Commands,
    added: Query<Entity, (Added<Prop>, Without<Visibility>)>,
) {
    for prop in &added {
        commands.entity(prop).insert(Visibility::default());
    }
}
