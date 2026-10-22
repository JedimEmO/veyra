//! The game's frozen registries: the world's own (`strata-data`) with the
//! game's sets beside them.

use std::ops::{Deref, DerefMut};

use strata_data::WorldDefs;

/// Every registry the game reads, frozen.
///
/// Built once in the `LoadingRegistries` state and then shared read-only.
/// The world half is [`WorldDefs`], which the world crates take on their
/// own; this derefs to it, so `registries.biomes` and `registries.sites`
/// read as they always did and a `&GameRegistries` passes where the world
/// crates ask for `&WorldDefs`. The loader replaces the deref with two
/// resources when the closed struct goes (review step three, item 3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameRegistries {
    /// The world's sets: biomes, materials, flora, provinces, site kinds and
    /// settlements.
    pub world: WorldDefs,
    /// The skill graph (`progression/trees.ron`), checked. Empty, with no
    /// trees and no budget, for a data set without one.
    pub progression: crate::progression::SkillGraph,
    /// The authored sheets beside it: the encounter's opponents and the
    /// arrival's start.
    pub sheets: crate::progression::AuthoredSheets,
    /// The items (`items/*.item.ron`), frozen, with the carrying rules and
    /// the first day's stock. Empty for a data set without them.
    pub items: crate::item::ItemCatalogue,
    /// What the ground holds (`resources/`): the class tree, the types and
    /// the deposits' dials, checked and resolved. Empty for a data set
    /// without them. In no world version: a deposit moves no ground.
    pub resources: crate::resource::ResourceSet,
    /// What a bench can make (`schematics/`) and the crafting session's
    /// dials, checked against the items and the resources. Empty for a
    /// data set without them. In no world version.
    pub schematics: crate::schematic::SchematicSet,
    /// Who buys what the household makes (`procurement/`), checked against
    /// the items and the resources. Empty for a data set without them. In
    /// no world version and no handshake.
    pub buyers: crate::procurement::BuyerSet,
    /// How a smith reworks a weapon's mounts (`crafting/rework.ron`),
    /// checked against the items and the skill graph. `None` for a data
    /// set without the file: then nothing can be reworked. In no world
    /// version and no handshake.
    pub rework: Option<crate::rework::ReworkRules>,
    /// What a household can ride (`vehicles/`), each dial checked. Empty
    /// for a data set without them. In no world version and no handshake.
    pub vehicles: crate::vehicle::VehicleSet,
    /// The art library (`icons/catalogue.json`): painted squares and
    /// symbols by id, checked at load. Empty for a data set without it. In
    /// no world version and no handshake.
    pub icons: crate::icons::IconCatalogue,
    /// The kinds of faction a world makes its own from (`factions/`), the
    /// authored holders and the territory dial, checked against the site
    /// kinds, the settlements and the icon library. Empty for a data set
    /// without them. In no world version and no handshake.
    pub factions: crate::faction::FactionSet,
    /// Who stands at a place of a site kind (`encounters/`), checked
    /// against the site kinds. Empty for a data set without them. In no
    /// world version and no handshake.
    pub encounters: crate::encounter::EncounterSet,
    /// What a cleared place leaves (`loot/`), checked against the
    /// encounters, the items and the resources. Empty for a data set without
    /// them. In no world version and no handshake.
    pub loot: crate::loot::LootSet,
    /// What a fall in the world costs (`household/fall.ron`). `None` for a
    /// data set without the file: then a fall is not handled. In no world
    /// version and no handshake.
    pub fall: Option<crate::fall::FallRules>,
    /// What a holder's board posts about the camps near it
    /// (`procurement/postings.ron`). `None` for a data set without the file:
    /// then no board posts work. In no world version and no handshake.
    pub postings: Option<crate::posting::PostingRules>,
    /// The people, what they say and the quests (`people/`, `dialogue/`,
    /// `quests/`), checked against the items, the skill graph, the resources
    /// and the bench. Empty for a data set without them. In no world
    /// version and no handshake.
    pub quests: crate::quest::QuestSet,
}
impl Deref for GameRegistries {
    type Target = WorldDefs;

    fn deref(&self) -> &WorldDefs {
        &self.world
    }
}

impl DerefMut for GameRegistries {
    fn deref_mut(&mut self) -> &mut WorldDefs {
        &mut self.world
    }
}
