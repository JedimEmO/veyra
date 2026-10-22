//! The data stage: files in, a frozen [`GameRegistries`] out. The world half
//! (biomes to settlements) is `strata_data::load_world_from_source`; this adds
//! the game's own sets over the frozen world.
//!
//! The layout under the asset root is flat rather than slotted's
//! `data/<modid>/<kind>/`, because the slice has one data set and no mods:
//!
//! ```text
//! biomes/*.biome.ron                        one BiomeDef per file
//! biomes/patches/{base,updates,final_fixes}/*.ron    PatchList over biomes
//! terrain/materials.ron                     a list of TerrainMaterialDef
//! terrain/patches/{base,updates,final_fixes}/*.ron   PatchList over materials
//! flora/*.flora.ron                          one FloraDef per file
//! flora/patches/{base,updates,final_fixes}/*.ron     PatchList over flora
//! geology/*.geology.ron                      one ProvinceDef per file
//! geology/patches/{base,updates,final_fixes}/*.ron   PatchList over provinces
//! settlements/*.settlement.ron               one SettlementDef per file
//! sites/*.site.ron                           one SiteKindDef per file
//! sites/patches/{base,updates,final_fixes}/*.ron     PatchList over site kinds
//! progression/trees.ron                      the skill graph, checked
//! progression/{opponents,player,household}.ron authored sheets, checked against it
//! items/*.item.ron                           one ItemDefinition per file
//! items/carry.ron                            the pack's and the store's sizes
//! items/starting.ron                         the first day's stock, checked against both
//! crafting/fitting.ron                       how a fitted module's quality becomes a factor
//! resources/classes.ron                      the families and classes of what the ground holds
//! resources/*.resource.ron                   one TypeDef per file
//! resources/deposits.ron                     where deposits lie and how a lot is drawn
//! schematics/*.schematic.ron                 one SchematicDef per file
//! crafting/session.ron                       the crafting session's dials
//! procurement/*.buyer.ron                    one BuyerDef per file
//! crafting/factory.ron                       the factory's dials: blueprints and runs
//! crafting/rework.ron                        how a smith raises a weapon's moddability
//! vehicles/*.vehicle.ron                     one VehicleDef per file: what a household rides
//! encounters/*.encounter.ron                  one EncounterDef per file: who stands at a place
//! loot/*.loot.ron                            one LootDef per file: what a cleared place leaves
//! household/fall.ron                         what a fall in the world costs
//! people/*.person.ron                        one PersonDef per file: who stands where and what they open
//! dialogue/*.dialogue.ron                    nodes of what is said, with replies, conditions and effects
//! quests/*.quest.ron                         one QuestDef per file: stages, objectives, endings
//! procurement/postings.ron                   what a board posts about the camps near it
//! ```
//!
//! Files parse to a raw RON `Value` first, patches run over the raw values
//! in slotted's three rounds, and only then does anything become a typed
//! def. That order is what lets a later mod add a field the base data does
//! not know about, and it is why the def types keep to the `Value`-safe
//! rule. IO goes through slotted's [`AssetSource`] port so a pack layer can
//! replace the directory later without this module noticing.

use std::path::Path;

use slotted_model::Namespaced;
use slotted_registry::loader::{AssetSource, DirSource, SourceError};
use slotted_registry::patch::PatchError;
use slotted_registry::registry::RegistryError;
use strata_data::FreezeError;

use crate::encounter::{
    ENCOUNTER_SUFFIX, ENCOUNTERS_DIR, EncounterDef, EncounterError, EncounterSet,
};
use crate::faction::{
    ANCHORS_FILE, AnchorsFile, CLAIM_RULES_FILE, ClaimRules, DEED_RULES_FILE, DeedRules,
    FACTION_RULES_FILE, FACTION_SUFFIX, FACTIONS_DIR, FactionError, FactionKindDef, FactionRules,
    FactionSet,
};
use crate::fall::{FALL_FILE, FallRules};
use crate::icons::{CATALOGUE_FILE, IconCatalogue, IconError};
use crate::item::{
    CarryRules, FittingRules, ItemCatalogue, ItemDefinition, ItemError, StartingStock,
};
use crate::loot::{LOOT_DIR, LOOT_SUFFIX, LootDef, LootError, LootSet};
use crate::posting::{POSTINGS_FILE, PostingRules};
use crate::procurement::{BUYER_SUFFIX, BUYERS_DIR, BuyerDef, BuyerError, BuyerSet};
use crate::progression::{AuthoredSheets, SkillGraph};
use crate::quest::{
    DIALOGUE_DIR, DIALOGUE_SUFFIX, PEOPLE_DIR, PERSON_SUFFIX, PLACES_FILE, PlaceRules,
    QUEST_SUFFIX, QUESTS_DIR, QuestContext, QuestError, QuestFiles, QuestSet,
};
use crate::registries::GameRegistries;
use crate::resource::{
    CLASSES_FILE, ClassTree, DEPOSITS_FILE, DepositRules, HARVEST_FILE, HarvestRules,
    RESOURCE_SUFFIX, RESOURCES_DIR, ResourceError, ResourceSet, SURVEY_FILE, SurveyRules, TypeDef,
};
use crate::rework::{REWORK_FILE, ReworkRules};
use crate::schematic::{
    FACTORY_FILE, FactoryRules, SCHEMATIC_SUFFIX, SCHEMATICS_DIR, SESSION_FILE, SchematicDef,
    SchematicError, SchematicSet, SessionRules,
};
use crate::vehicle::{VEHICLE_SUFFIX, VEHICLES_DIR, VehicleDef, VehicleError, VehicleSet};
use crate::wear::WearRules;

pub use strata_data::load::{
    BIOME_SUFFIX, BIOMES_DIR, FLORA_DIR, FLORA_SUFFIX, GEOLOGY_DIR, GEOLOGY_SUFFIX, MATERIALS_FILE,
    SETTLEMENT_SUFFIX, SETTLEMENTS_DIR, SITE_SUFFIX, SITES_DIR, TERRAIN_DIR,
};

/// The directory of `.item.ron` files, relative to the asset root. Absent
/// for a data set with nothing to carry.
pub const ITEMS_DIR: &str = "items";
/// The suffix an item file must carry to be read.
pub const ITEM_SUFFIX: &str = ".item.ron";
/// The carrying rules, relative to the asset root.
pub const CARRY_FILE: &str = "items/carry.ron";
/// The first day's stock, relative to the asset root.
pub const STARTING_FILE: &str = "items/starting.ron";
/// The fitting rules, relative to the asset root. Absent, quality moves no
/// factor and no module may have an effect.
pub const FITTING_FILE: &str = "crafting/fitting.ron";

/// The wear rules.
pub const WEAR_FILE: &str = "crafting/wear.ron";

/// Why the data stage stopped. Every variant names the file, because the
/// first thing anyone needs is which file is broken.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The source could not read or list something.
    #[error("{0}")]
    Source(#[from] SourceError),
    /// A file is not UTF-8.
    #[error("`{path}` is not UTF-8")]
    NotUtf8 {
        /// The file.
        path: String,
    },
    /// A file is not RON.
    #[error("`{path}`: {source}")]
    Parse {
        /// The file.
        path: String,
        /// The parser's complaint, with a line and column. Boxed because
        /// it is by far the largest variant and every loader function
        /// returns this type.
        source: Box<ron::error::SpannedError>,
    },
    /// A file has no `id: "namespace:path"` field, or an invalid one.
    #[error("`{path}`: {reason}")]
    MissingId {
        /// The file.
        path: String,
        /// What was found instead.
        reason: String,
    },
    /// Two files claim one id.
    #[error("`{path}` registers `{id}`, which `{first}` already registered")]
    Duplicate {
        /// The id.
        id: Namespaced,
        /// The file that had it first.
        first: String,
        /// The file that had it second.
        path: String,
    },
    /// A patch could not be applied.
    #[error("patch file `{path}`: {source}")]
    Patch {
        /// The patch file.
        path: String,
        /// What went wrong.
        source: PatchError,
    },
    /// A raw entry, after patching, does not fit its def type.
    #[error("`{path}` (`{id}`): {message}")]
    Deserialize {
        /// The file the entry came from.
        path: String,
        /// The entry.
        id: Namespaced,
        /// serde's complaint.
        message: String,
    },
    /// Registering an entry failed. Should not happen after the duplicate
    /// check, but the registry's own error is kept rather than unwrapped.
    #[error("{0}")]
    Registry(#[from] RegistryError),
    /// The set loaded but is not consistent.
    #[error("{0}")]
    Freeze(#[from] FreezeError),
    /// A progression file (the skill graph or a sheet) does not parse or
    /// breaks the design's rules.
    #[error("`{path}`: {source}")]
    Progression {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: crate::progression::ParseError,
    },
    /// An item file, the carrying rules or the first day's stock does not
    /// parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Items {
        /// The file.
        path: String,
        /// What is wrong with it. Boxed for the reason the parser's
        /// complaint is: every loader function returns this type.
        source: Box<ItemError>,
    },
    /// The class tree, a resource type or the deposits' dials does not
    /// parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Resources {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<ResourceError>,
    },
    /// A schematic or the crafting session's dials does not parse or breaks
    /// a rule.
    #[error("`{path}`: {source}")]
    Schematics {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<SchematicError>,
    },
    /// A vehicle does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Vehicles {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<VehicleError>,
    },
    /// The icon catalogue does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Icons {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: IconError,
    },
    /// A buyer does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Buyers {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<BuyerError>,
    },
    /// An encounter does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Encounters {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<EncounterError>,
    },
    /// A loot file does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Loot {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<LootError>,
    },
    /// A person, a conversation or a quest does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Quests {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<QuestError>,
    },
    /// The fall's dials do not parse or break a rule.
    #[error("`{path}`: {reason}")]
    Fall {
        /// The file.
        path: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The postings' dials do not parse or break a rule.
    #[error("`{path}`: {reason}")]
    Postings {
        /// The file.
        path: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A faction file does not parse or breaks a rule.
    #[error("`{path}`: {source}")]
    Factions {
        /// The file.
        path: String,
        /// What is wrong with it.
        source: Box<FactionError>,
    },
}

/// Loads and freezes the registries under `root`, a directory laid out as
/// the module docs describe.
///
/// # Errors
///
/// [`LoadError`] for any unreadable, unparsable, duplicate or inconsistent
/// file.
pub fn load_from_dir(root: impl AsRef<Path>) -> Result<GameRegistries, LoadError> {
    load_from_source(&DirSource::new(root.as_ref()))
}

/// Loads and freezes the registries from any [`AssetSource`] with the same
/// layout. Tests use an in-memory source; a pack layer will use its own.
///
/// # Errors
///
/// [`LoadError`] for any unreadable, unparsable, duplicate or inconsistent
/// file.
pub fn load_from_source(source: &dyn AssetSource) -> Result<GameRegistries, LoadError> {
    let mut frozen = GameRegistries {
        world: strata_data::load_world_from_source(source)?,
        ..GameRegistries::default()
    };
    (frozen.progression, frozen.sheets) = progression(source)?;
    // After the freeze, whose provinces, biomes and site kinds a resource
    // names, and before the items, which gain a lot a type.
    frozen.resources = resources(source, &frozen)?;
    frozen.items = items(source, &frozen.sheets, &frozen.resources)?;
    // Last: a schematic names the items it takes and makes and the classes
    // of resource its slots hold.
    frozen.schematics = schematics(source, &frozen.items, &frozen.resources)?;
    frozen
        .schematics
        .check_professions(&frozen.progression)
        .map_err(|source| LoadError::Schematics {
            path: SCHEMATICS_DIR.to_owned(),
            source: Box::new(source),
        })?;
    bikes(source, &mut frozen)?;
    frozen.buyers = buyers(source, &frozen.items, &frozen.resources)?;
    frozen.rework = rework(source, &frozen)?;
    frozen.icons = icons(source, &frozen.items)?;
    // After the icons, whose library a kind's symbol is held to, and the
    // settlements and site kinds an anchor and a kind name.
    frozen.factions = factions(source, &frozen)?;
    after_play(source, &mut frozen)?;
    Ok(frozen)
}

/// What the world's people do, read last: the encounters, the loot they
/// leave, which names encounters, items and resource classes, and the fall's
/// dials.
fn after_play(source: &dyn AssetSource, frozen: &mut GameRegistries) -> Result<(), LoadError> {
    frozen.encounters = encounters(source, frozen)?;
    frozen.loot = loot(source, frozen)?;
    frozen.fall = fall(source)?;
    frozen.postings = postings(source)?;
    frozen.quests = quests(source, frozen)?;
    Ok(())
}

/// The people, their conversations and the quests, checked against the
/// items, the skill graph, the resources and the bench. No directory is no
/// questline.
fn quests(source: &dyn AssetSource, frozen: &GameRegistries) -> Result<QuestSet, LoadError> {
    let mut files = QuestFiles::default();
    let list = |dir: &str| match source.list(dir) {
        Ok(paths) => Ok(paths),
        Err(SourceError::NotFound(_)) => Ok(Vec::new()),
        Err(err) => Err(LoadError::from(err)),
    };
    let ron_error = |path: &str, error: ron::error::SpannedError| LoadError::Quests {
        path: path.to_owned(),
        source: Box::new(QuestError::Ron(error.to_string())),
    };
    for path in list(PEOPLE_DIR)? {
        if path.ends_with(PERSON_SUFFIX) {
            let text = read_utf8(source, &path)?;
            let def = ron::from_str(&text).map_err(|e| ron_error(&path, e))?;
            files.people.push((path, def));
        }
    }
    for path in list(DIALOGUE_DIR)? {
        if path.ends_with(DIALOGUE_SUFFIX) {
            let text = read_utf8(source, &path)?;
            let def = ron::from_str(&text).map_err(|e| ron_error(&path, e))?;
            files.dialogues.push((path, def));
        }
    }
    for path in list(QUESTS_DIR)? {
        if path.ends_with(QUEST_SUFFIX) {
            let text = read_utf8(source, &path)?;
            let def = ron::from_str(&text).map_err(|e| ron_error(&path, e))?;
            files.quests.push((path, def));
        }
    }
    let members: Vec<String> = frozen
        .sheets
        .household
        .as_ref()
        .map(|household| household.recruits.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default();
    QuestSet::build(
        files,
        &QuestContext {
            items: &frozen.items,
            graph: &frozen.progression,
            resources: &frozen.resources,
            members: &members,
        },
    )
    .map_err(|(path, source)| LoadError::Quests {
        path,
        source: Box::new(source),
    })
    .and_then(|set| match read_utf8(source, PLACES_FILE) {
        Ok(text) => PlaceRules::parse(&text)
            .map(|rules| set.with_rules(rules))
            .map_err(|reason| LoadError::Quests {
                path: PLACES_FILE.to_owned(),
                source: Box::new(QuestError::Ron(reason)),
            }),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(set),
        Err(err) => Err(err),
    })
}

/// The loot tables, checked against the encounters, the items and the
/// resources. No directory is no loot.
fn loot(source: &dyn AssetSource, frozen: &GameRegistries) -> Result<LootSet, LoadError> {
    let paths = match source.list(LOOT_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(LOOT_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: LootDef = ron::from_str(&text).map_err(|error| LoadError::Loot {
            path: path.clone(),
            source: Box::new(LootError::Ron(error.to_string())),
        })?;
        defs.push((path, def));
    }
    LootSet::build(defs, &frozen.encounters, &frozen.items, &frozen.resources).map_err(
        |(path, source)| LoadError::Loot {
            path,
            source: Box::new(source),
        },
    )
}

/// The fall's dials. No file is no fall.
fn fall(source: &dyn AssetSource) -> Result<Option<FallRules>, LoadError> {
    let text = match read_utf8(source, FALL_FILE) {
        Ok(text) => text,
        Err(LoadError::Source(SourceError::NotFound(_))) => return Ok(None),
        Err(err) => return Err(err),
    };
    FallRules::parse(&text)
        .map(Some)
        .map_err(|reason| LoadError::Fall {
            path: FALL_FILE.to_owned(),
            reason,
        })
}

/// The postings' dials. No file is no postings.
fn postings(source: &dyn AssetSource) -> Result<Option<PostingRules>, LoadError> {
    let text = match read_utf8(source, POSTINGS_FILE) {
        Ok(text) => text,
        Err(LoadError::Source(SourceError::NotFound(_))) => return Ok(None),
        Err(err) => return Err(err),
    };
    PostingRules::parse(&text)
        .map(Some)
        .map_err(|reason| LoadError::Postings {
            path: POSTINGS_FILE.to_owned(),
            reason,
        })
}

/// The encounters, checked against the site kinds. No directory is no
/// encounters.
fn encounters(
    source: &dyn AssetSource,
    frozen: &GameRegistries,
) -> Result<EncounterSet, LoadError> {
    let paths = match source.list(ENCOUNTERS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(ENCOUNTER_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: EncounterDef = ron::from_str(&text).map_err(|error| LoadError::Encounters {
            path: path.clone(),
            source: Box::new(EncounterError::Ron(error.to_string())),
        })?;
        defs.push((path, def));
    }
    EncounterSet::build(defs, |kind| frozen.site_kind_id(kind).is_some()).map_err(
        |(path, source)| LoadError::Encounters {
            path,
            source: Box::new(source),
        },
    )
}

/// The faction kinds, the anchors and the rules. No directory is no
/// factions.
fn factions(source: &dyn AssetSource, frozen: &GameRegistries) -> Result<FactionSet, LoadError> {
    let paths = match source.list(FACTIONS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let ron_error = |path: &str, error: ron::error::SpannedError| LoadError::Factions {
        path: path.to_owned(),
        source: Box::new(FactionError::Ron(error.to_string())),
    };
    let mut kinds = Vec::new();
    for path in paths {
        if !path.ends_with(FACTION_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: FactionKindDef = ron::from_str(&text).map_err(|error| ron_error(&path, error))?;
        kinds.push((path, def));
    }
    let optional = |path: &str| match read_utf8(source, path) {
        Ok(text) => Ok(Some(text)),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(None),
        Err(err) => Err(err),
    };
    let anchors = optional(ANCHORS_FILE)?
        .map(|text| ron::from_str::<AnchorsFile>(&text).map_err(|e| ron_error(ANCHORS_FILE, e)))
        .transpose()?;
    let rules = optional(FACTION_RULES_FILE)?
        .map(|text| {
            ron::from_str::<FactionRules>(&text).map_err(|e| ron_error(FACTION_RULES_FILE, e))
        })
        .transpose()?;
    let claims = optional(CLAIM_RULES_FILE)?
        .map(|text| ron::from_str::<ClaimRules>(&text).map_err(|e| ron_error(CLAIM_RULES_FILE, e)))
        .transpose()?;
    let deeds = optional(DEED_RULES_FILE)?
        .map(|text| ron::from_str::<DeedRules>(&text).map_err(|e| ron_error(DEED_RULES_FILE, e)))
        .transpose()?;
    let refused = |(path, source): (String, FactionError)| LoadError::Factions {
        path,
        source: Box::new(source),
    };
    let set = FactionSet::build(kinds, anchors, rules, claims, deeds, frozen).map_err(refused)?;
    set.check_symbols(&frozen.icons).map_err(refused)?;
    Ok(set)
}

/// The icon catalogue. No file is no icons.
fn icons(source: &dyn AssetSource, items: &ItemCatalogue) -> Result<IconCatalogue, LoadError> {
    let text = match read_utf8(source, CATALOGUE_FILE) {
        Ok(text) => text,
        Err(LoadError::Source(SourceError::NotFound(_))) => return Ok(IconCatalogue::default()),
        Err(err) => return Err(err),
    };
    let catalogue = IconCatalogue::parse(&text).map_err(|source| LoadError::Icons {
        path: CATALOGUE_FILE.to_owned(),
        source,
    })?;
    // An item names its symbol, so the library is read after the items and
    // the check is made here.
    items
        .check_symbols(&catalogue)
        .map_err(|source| LoadError::Items {
            path: ITEMS_DIR.to_owned(),
            source: Box::new(source),
        })?;
    Ok(catalogue)
}

/// The rework rules, checked against the items and the skill graph. No file
/// is no rework.
fn rework(
    source: &dyn AssetSource,
    registries: &GameRegistries,
) -> Result<Option<ReworkRules>, LoadError> {
    let text = match read_utf8(source, REWORK_FILE) {
        Ok(text) => text,
        Err(LoadError::Source(SourceError::NotFound(_))) => return Ok(None),
        Err(err) => return Err(err),
    };
    let refused = |source: ItemError| LoadError::Items {
        path: REWORK_FILE.to_owned(),
        source: Box::new(source),
    };
    let rules = ReworkRules::parse(&text).map_err(refused)?;
    rules
        .check(&registries.items, &registries.progression)
        .map_err(refused)?;
    Ok(Some(rules))
}

/// The vehicles, and the items that are bikes of them.
fn bikes(source: &dyn AssetSource, frozen: &mut GameRegistries) -> Result<(), LoadError> {
    frozen.vehicles = vehicles(source)?;
    frozen
        .items
        .check_vehicles(&frozen.vehicles)
        .map_err(|source| LoadError::Items {
            path: ITEMS_DIR.to_owned(),
            source: Box::new(source),
        })
}

/// The vehicles. No directory is no vehicles.
fn vehicles(source: &dyn AssetSource) -> Result<VehicleSet, LoadError> {
    let paths = match source.list(VEHICLES_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(VEHICLE_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: VehicleDef = ron::from_str(&text).map_err(|error| LoadError::Vehicles {
            path: path.clone(),
            source: Box::new(VehicleError::Ron(error.to_string())),
        })?;
        defs.push((path, def));
    }
    VehicleSet::build(defs).map_err(|(path, source)| LoadError::Vehicles {
        path,
        source: Box::new(source),
    })
}

/// The buyers, checked against the items and the resources. No directory is
/// no buyers.
pub(crate) fn buyers(
    source: &dyn AssetSource,
    items: &ItemCatalogue,
    resources: &ResourceSet,
) -> Result<BuyerSet, LoadError> {
    let paths = match source.list(BUYERS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(BUYER_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: BuyerDef = ron::from_str(&text).map_err(|error| LoadError::Buyers {
            path: path.clone(),
            source: Box::new(BuyerError::Ron(error.to_string())),
        })?;
        defs.push((path, def));
    }
    BuyerSet::build(defs, items, resources).map_err(|(path, source)| LoadError::Buyers {
        path,
        source: Box::new(source),
    })
}

/// The schematics, the crafting session's dials and the factory's, checked
/// against the items and the resources. No directory is no schematics, as
/// no item directory is no items; a data set with any needs the session's
/// dials, since a schematic's complexity is held to the points they give.
/// The factory's are their own file and may be absent: then nothing is
/// recorded as a blueprint.
pub(crate) fn schematics(
    source: &dyn AssetSource,
    items: &ItemCatalogue,
    resources: &ResourceSet,
) -> Result<SchematicSet, LoadError> {
    let refused = |path: &str| {
        let path = path.to_owned();
        move |source: SchematicError| LoadError::Schematics {
            path,
            source: Box::new(source),
        }
    };
    let paths = match source.list(SCHEMATICS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(SCHEMATIC_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: SchematicDef = ron::from_str(&text)
            .map_err(|error| SchematicError::Ron(error.to_string()))
            .map_err(refused(&path))?;
        defs.push((path, def));
    }
    let rules = match read_utf8(source, SESSION_FILE) {
        Ok(text) => SessionRules::parse(&text).map_err(refused(SESSION_FILE))?,
        Err(LoadError::Source(SourceError::NotFound(_))) => SessionRules::default(),
        Err(err) => return Err(err),
    };
    let set = SchematicSet::build(defs, rules, items, resources).map_err(|(path, source)| {
        LoadError::Schematics {
            path,
            source: Box::new(source),
        }
    })?;
    match read_utf8(source, FACTORY_FILE) {
        Ok(text) => FactoryRules::parse(&text)
            .and_then(|factory| set.with_factory(factory, items))
            .map_err(refused(FACTORY_FILE)),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(set),
        Err(err) => Err(err),
    }
}

/// The class tree, the resource types and the deposits' dials. No directory
/// (or one with none of the three kinds of file) is no resources, as no
/// item directory is no items; a directory with any of them needs the tree
/// and the dials.
pub(crate) fn resources(
    source: &dyn AssetSource,
    ids: &GameRegistries,
) -> Result<ResourceSet, LoadError> {
    let refused = |path: &str| {
        let path = path.to_owned();
        move |source: ResourceError| LoadError::Resources {
            path,
            source: Box::new(source),
        }
    };
    let optional = |path: &str| match read_utf8(source, path) {
        Ok(text) => Ok(Some(text)),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(None),
        Err(err) => Err(err),
    };
    let paths = match source.list(RESOURCES_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut types = Vec::new();
    for path in paths {
        if !path.ends_with(RESOURCE_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: TypeDef = ron::from_str(&text)
            .map_err(|error| ResourceError::Ron(error.to_string()))
            .map_err(refused(&path))?;
        types.push((path, def));
    }
    let (tree, rules) = (optional(CLASSES_FILE)?, optional(DEPOSITS_FILE)?);
    if types.is_empty() && tree.is_none() && rules.is_none() {
        return Ok(ResourceSet::default());
    }
    let tree: ClassTree = ron::from_str(
        &tree.ok_or_else(|| refused(CLASSES_FILE)(ResourceError::Missing(CLASSES_FILE)))?,
    )
    .map_err(|error| ResourceError::Ron(error.to_string()))
    .map_err(refused(CLASSES_FILE))?;
    let rules: DepositRules = ron::from_str(
        &rules.ok_or_else(|| refused(DEPOSITS_FILE)(ResourceError::Missing(DEPOSITS_FILE)))?,
    )
    .map_err(|error| ResourceError::Ron(error.to_string()))
    .map_err(refused(DEPOSITS_FILE))?;
    let set = ResourceSet::build(types, tree, rules, ids).map_err(|(path, source)| {
        LoadError::Resources {
            path,
            source: Box::new(source),
        }
    })?;
    // The survey dials are a file of their own; a data set that has deposits
    // has them.
    let survey: SurveyRules = ron::from_str(
        &optional(SURVEY_FILE)?
            .ok_or_else(|| refused(SURVEY_FILE)(ResourceError::Missing(SURVEY_FILE)))?,
    )
    .map_err(|error| ResourceError::Ron(error.to_string()))
    .map_err(refused(SURVEY_FILE))?;
    let set = set.with_survey(survey).map_err(refused(SURVEY_FILE))?;
    let harvest: HarvestRules = ron::from_str(
        &optional(HARVEST_FILE)?
            .ok_or_else(|| refused(HARVEST_FILE)(ResourceError::Missing(HARVEST_FILE)))?,
    )
    .map_err(|error| ResourceError::Ron(error.to_string()))
    .map_err(refused(HARVEST_FILE))?;
    set.with_harvest(harvest).map_err(refused(HARVEST_FILE))
}

/// The items, the carrying rules and the first day's stock, with a lot
/// item for every type of `resources`. No directory is no items, as no
/// graph is an empty one, and the stock is then checked against nothing
/// and must be absent too.
pub(crate) fn items(
    source: &dyn AssetSource,
    sheets: &AuthoredSheets,
    resources: &ResourceSet,
) -> Result<ItemCatalogue, LoadError> {
    let refused = |path: &str| {
        let path = path.to_owned();
        move |source: ItemError| LoadError::Items {
            path,
            source: Box::new(source),
        }
    };
    let optional = |path: &str| match read_utf8(source, path) {
        Ok(text) => Ok(Some(text)),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(None),
        Err(err) => Err(err),
    };
    let paths = match source.list(ITEMS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(ITEM_SUFFIX) {
            continue;
        }
        let text = read_utf8(source, &path)?;
        let def: ItemDefinition = ron::from_str(&text)
            .map_err(|error| ItemError::Ron(error.to_string()))
            .map_err(refused(&path))?;
        defs.push((path, def));
    }
    let carry = match optional(CARRY_FILE)? {
        Some(text) => CarryRules::parse(&text).map_err(refused(CARRY_FILE))?,
        None => CarryRules::default(),
    };
    let fitting = match optional(FITTING_FILE)? {
        Some(text) => FittingRules::parse(&text).map_err(refused(FITTING_FILE))?,
        None => FittingRules::default(),
    };
    let wear = match optional(WEAR_FILE)? {
        Some(text) => WearRules::parse(&text).map_err(refused(WEAR_FILE))?,
        None => WearRules::default(),
    };
    let catalogue = ItemCatalogue::build(defs, carry, fitting, resources)
        .map_err(|(path, source)| LoadError::Items { path, source })?
        .with_wear(wear);
    let Some(text) = optional(STARTING_FILE)? else {
        return Ok(catalogue);
    };
    let starting: StartingStock = ron::from_str(&text)
        .map_err(|error| ItemError::Ron(error.to_string()))
        .map_err(refused(STARTING_FILE))?;
    // A pack for a name nobody has would be a typo that gives nobody
    // anything. Without a household file the game names the arrival
    // itself, so there is nothing here to hold the names to.
    if let Some(household) = &sheets.household {
        let known = |name: &String| {
            std::iter::once(&household.arrival)
                .chain(&household.recruits)
                .any(|member| member.name == *name)
        };
        if let Some(stranger) = starting.packs.keys().find(|name| !known(name)) {
            return Err(refused(STARTING_FILE)(ItemError::UnknownMember(
                stranger.clone(),
            )));
        }
    }
    catalogue
        .with_starting(starting)
        .map_err(refused(STARTING_FILE))
}

/// The skill graph and the authored sheets. No graph is an empty one, as
/// no settlements is none; the sheets are then checked against nothing
/// and must be absent too.
fn progression(source: &dyn AssetSource) -> Result<(SkillGraph, AuthoredSheets), LoadError> {
    let optional = |path: &str| match read_utf8(source, path) {
        Ok(text) => Ok(Some(text)),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(None),
        Err(err) => Err(err),
    };
    let graph = match optional(SkillGraph::FILE)? {
        Some(text) => SkillGraph::parse(&text).map_err(|source| LoadError::Progression {
            path: SkillGraph::FILE.to_owned(),
            source,
        })?,
        None => SkillGraph::default(),
    };
    // Each file is parsed on its own so an error names the file it is in.
    let refused = |path: &str| {
        let path = path.to_owned();
        move |source| LoadError::Progression { path, source }
    };
    let opponents = match optional(AuthoredSheets::OPPONENTS_FILE)? {
        Some(text) => AuthoredSheets::parse_opponents(&graph, &text)
            .map_err(refused(AuthoredSheets::OPPONENTS_FILE))?,
        None => std::collections::BTreeMap::default(),
    };
    let player = optional(AuthoredSheets::PLAYER_FILE)?
        .map(|text| {
            AuthoredSheets::parse_player(&graph, &text)
                .map_err(refused(AuthoredSheets::PLAYER_FILE))
        })
        .transpose()?;
    let household = optional(AuthoredSheets::HOUSEHOLD_FILE)?
        .map(|text| {
            AuthoredSheets::parse_household(&graph, &text)
                .map_err(refused(AuthoredSheets::HOUSEHOLD_FILE))
        })
        .transpose()?;
    let sheets = AuthoredSheets {
        opponents,
        player,
        household,
    };
    Ok((graph, sheets))
}

impl From<strata_data::LoadError> for LoadError {
    fn from(error: strata_data::LoadError) -> Self {
        use strata_data::LoadError as World;
        match error {
            World::Source(e) => Self::Source(e),
            World::NotUtf8 { path } => Self::NotUtf8 { path },
            World::Parse { path, source } => Self::Parse { path, source },
            World::MissingId { path, reason } => Self::MissingId { path, reason },
            World::Duplicate { id, first, path } => Self::Duplicate { id, first, path },
            World::Patch { path, source } => Self::Patch { path, source },
            World::Deserialize { path, id, message } => Self::Deserialize { path, id, message },
            World::Registry(e) => Self::Registry(e),
            World::Freeze(e) => Self::Freeze(e),
        }
    }
}

/// [`strata_data::load::read_utf8`] in this loader's error type, so the
/// "not found is an empty set" matches below keep reading the same variants.
fn read_utf8(source: &dyn AssetSource, path: &str) -> Result<String, LoadError> {
    strata_data::load::read_utf8(source, path).map_err(LoadError::from)
}
