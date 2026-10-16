//! The data stage: files in, a frozen [`GameRegistries`] out.
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
use slotted_registry::patch::{self, PatchError, PatchList, RawEntries, Round};
use slotted_registry::registry::RegistryError;

use crate::biome::BiomeDef;
use crate::encounter::{
    ENCOUNTER_SUFFIX, ENCOUNTERS_DIR, EncounterDef, EncounterError, EncounterSet,
};
use crate::faction::{
    ANCHORS_FILE, AnchorsFile, CLAIM_RULES_FILE, ClaimRules, DEED_RULES_FILE, DeedRules,
    FACTION_RULES_FILE, FACTION_SUFFIX, FACTIONS_DIR, FactionError, FactionKindDef, FactionRules,
    FactionSet,
};
use crate::fall::{FALL_FILE, FallRules};
use crate::flora::FloraDef;
use crate::geology::ProvinceDef;
use crate::icons::{CATALOGUE_FILE, IconCatalogue, IconError};
use crate::item::{
    CarryRules, FittingRules, ItemCatalogue, ItemDefinition, ItemError, StartingStock,
};
use crate::loot::{LOOT_DIR, LOOT_SUFFIX, LootDef, LootError, LootSet};
use crate::material::TerrainMaterialDef;
use crate::posting::{POSTINGS_FILE, PostingRules};
use crate::procurement::{BUYER_SUFFIX, BUYERS_DIR, BuyerDef, BuyerError, BuyerSet};
use crate::progression::{AuthoredSheets, SkillGraph};
use crate::quest::{
    DIALOGUE_DIR, DIALOGUE_SUFFIX, PEOPLE_DIR, PERSON_SUFFIX, PLACES_FILE, PlaceRules,
    QUEST_SUFFIX, QUESTS_DIR, QuestContext, QuestError, QuestFiles, QuestSet,
};
use crate::registries::{FreezeError, GameRegistries, OpenRegistries};
use crate::resource::{
    CLASSES_FILE, ClassTree, DEPOSITS_FILE, DepositRules, HARVEST_FILE, HarvestRules,
    RESOURCE_SUFFIX, RESOURCES_DIR, ResourceError, ResourceSet, SURVEY_FILE, SurveyRules, TypeDef,
};
use crate::rework::{REWORK_FILE, ReworkRules};
use crate::schematic::{
    FACTORY_FILE, FactoryRules, SCHEMATIC_SUFFIX, SCHEMATICS_DIR, SESSION_FILE, SchematicDef,
    SchematicError, SchematicSet, SessionRules,
};
use crate::settlement::SettlementDef;
use crate::site::SiteKindDef;
use crate::vehicle::{VEHICLE_SUFFIX, VEHICLES_DIR, VehicleDef, VehicleError, VehicleSet};
use crate::wear::WearRules;

/// The directory of `.biome.ron` files, relative to the asset root.
pub const BIOMES_DIR: &str = "biomes";
/// The suffix a biome file must carry to be read.
pub const BIOME_SUFFIX: &str = ".biome.ron";
/// The material list, relative to the asset root.
pub const MATERIALS_FILE: &str = "terrain/materials.ron";
/// The directory of material patch rounds, relative to the asset root.
pub const TERRAIN_DIR: &str = "terrain";
/// The directory of `.flora.ron` files, relative to the asset root. Absent
/// for a world with nothing growing in it.
pub const FLORA_DIR: &str = "flora";
/// The suffix a flora file must carry to be read.
pub const FLORA_SUFFIX: &str = ".flora.ron";
/// The directory of `.geology.ron` files, relative to the asset root.
/// Absent for a world without geology, whose province field is off.
pub const GEOLOGY_DIR: &str = "geology";
/// The suffix a province file must carry to be read.
pub const GEOLOGY_SUFFIX: &str = ".geology.ron";
/// The directory of `.settlement.ron` files, relative to the asset root.
/// Absent for a world with no cities.
pub const SETTLEMENTS_DIR: &str = "settlements";
/// The suffix a settlement file must carry to be read.
pub const SETTLEMENT_SUFFIX: &str = ".settlement.ron";
/// The directory of `.site.ron` files, relative to the asset root. Absent
/// for a world that places no sites (ADR 0012).
pub const SITES_DIR: &str = "sites";
/// The suffix a site kind file must carry to be read.
pub const SITE_SUFFIX: &str = ".site.ron";
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
    let mut biomes = RawRegistry::default();
    for path in source.list(BIOMES_DIR)? {
        if !path.ends_with(BIOME_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        biomes.insert(&path, value)?;
    }

    let mut materials = RawRegistry::default();
    match read_utf8(source, MATERIALS_FILE) {
        Ok(text) => {
            let list: ron::Value = ron::from_str(&text).map_err(|source| LoadError::Parse {
                path: MATERIALS_FILE.to_owned(),
                source: Box::new(source),
            })?;
            let ron::Value::Seq(entries) = list else {
                return Err(LoadError::MissingId {
                    path: MATERIALS_FILE.to_owned(),
                    reason: "expected a list of materials".to_owned(),
                });
            };
            for value in entries {
                materials.insert(MATERIALS_FILE, value)?;
            }
        }
        // No materials is legal for a data set of nothing but ocean; a biome
        // that names one will fail at freeze with the biome's own id.
        Err(LoadError::Source(SourceError::NotFound(_))) => {}
        Err(err) => return Err(err),
    }

    let mut flora = RawRegistry::default();
    for path in source.list(FLORA_DIR)? {
        if !path.ends_with(FLORA_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        flora.insert(&path, value)?;
    }

    let mut provinces = RawRegistry::default();
    for path in source.list(GEOLOGY_DIR)? {
        if !path.ends_with(GEOLOGY_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        provinces.insert(&path, value)?;
    }

    let mut sites = RawRegistry::default();
    for path in source.list(SITES_DIR)? {
        if !path.ends_with(SITE_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        sites.insert(&path, value)?;
    }

    for round in Round::ALL {
        biomes.apply_round(source, BIOMES_DIR, round)?;
        materials.apply_round(source, TERRAIN_DIR, round)?;
        flora.apply_round(source, FLORA_DIR, round)?;
        provinces.apply_round(source, GEOLOGY_DIR, round)?;
        sites.apply_round(source, SITES_DIR, round)?;
    }

    let mut open = OpenRegistries::new();
    for (id, path, value) in materials.drain() {
        let def: TerrainMaterialDef = typed(&path, &id, value)?;
        open.add_material(def)?;
    }
    for (id, path, value) in biomes.drain() {
        let def: BiomeDef = typed(&path, &id, value)?;
        open.add_biome(def)?;
    }
    for (id, path, value) in flora.drain() {
        let def: FloraDef = typed(&path, &id, value)?;
        open.add_flora(def)?;
    }
    for (id, path, value) in provinces.drain() {
        let def: ProvinceDef = typed(&path, &id, value)?;
        open.add_province_from(def, &path)?;
    }
    for (id, path, value) in sites.drain() {
        let def: SiteKindDef = typed(&path, &id, value)?;
        open.add_site_kind_from(def, &path)?;
    }
    let mut frozen = open.freeze()?;
    frozen.settlements = settlements(source)?;
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

/// Every settlement file, typed and checked, in id order.
fn settlements(source: &dyn AssetSource) -> Result<Vec<SettlementDef>, LoadError> {
    let paths = match source.list(SETTLEMENTS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(SETTLEMENT_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        let id = id_of(&path, &value)?;
        let def: SettlementDef = typed(&path, &id, value)?;
        if let Some(reason) = def.invalid_reason() {
            return Err(LoadError::MissingId {
                path,
                reason: reason.to_owned(),
            });
        }
        defs.push(def);
    }
    defs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(defs)
}

/// One registry's entries between parsing and typing, with the file each
/// came from for error messages.
#[derive(Default)]
struct RawRegistry {
    entries: RawEntries,
    origins: Vec<(Namespaced, String)>,
}

impl RawRegistry {
    fn insert(&mut self, path: &str, value: ron::Value) -> Result<(), LoadError> {
        let id = id_of(path, &value)?;
        if self.entries.contains_key(&id) {
            let first = self.origin(&id);
            return Err(LoadError::Duplicate {
                id,
                first,
                path: path.to_owned(),
            });
        }
        self.origins.push((id.clone(), path.to_owned()));
        self.entries.insert(id, value);
        Ok(())
    }

    fn apply_round(
        &mut self,
        source: &dyn AssetSource,
        dir: &str,
        round: Round,
    ) -> Result<(), LoadError> {
        let dir = format!("{dir}/patches/{}", round.dir());
        for path in source.list(&dir)? {
            let is_ron = Path::new(&path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ron"));
            if !is_ron {
                continue;
            }
            let text = read_utf8(source, &path)?;
            let list: PatchList = ron::from_str(&text).map_err(|source| LoadError::Parse {
                path: path.clone(),
                source: Box::new(source),
            })?;
            for patch in &list {
                patch::apply(&mut self.entries, patch).map_err(|source| LoadError::Patch {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        Ok(())
    }

    /// The file `id` was read from, or empty for an entry no file declared.
    fn origin(&self, id: &Namespaced) -> String {
        self.origins
            .iter()
            .find(|(other, _)| other == id)
            .map(|(_, path)| path.clone())
            .unwrap_or_default()
    }

    fn drain(self) -> Vec<(Namespaced, String, ron::Value)> {
        let origins = self.origins;
        self.entries
            .into_iter()
            .map(|(id, value)| {
                let path = origins
                    .iter()
                    .find(|(other, _)| *other == id)
                    .map(|(_, path)| path.clone())
                    .unwrap_or_default();
                (id, path, value)
            })
            .collect()
    }
}

fn read_utf8(source: &dyn AssetSource, path: &str) -> Result<String, LoadError> {
    let bytes = source.read(path)?;
    String::from_utf8(bytes).map_err(|_| LoadError::NotUtf8 {
        path: path.to_owned(),
    })
}

fn parse_file(source: &dyn AssetSource, path: &str) -> Result<ron::Value, LoadError> {
    let text = read_utf8(source, path)?;
    ron::from_str(&text).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: Box::new(source),
    })
}

/// The `id` field of a raw entry, which is the registry key.
fn id_of(path: &str, value: &ron::Value) -> Result<Namespaced, LoadError> {
    let missing = |reason: &str| LoadError::MissingId {
        path: path.to_owned(),
        reason: reason.to_owned(),
    };
    let ron::Value::Map(map) = value else {
        return Err(missing("expected a struct with an `id` field"));
    };
    let key = ron::Value::String("id".to_owned());
    let Some(ron::Value::String(id)) = map.get(&key) else {
        return Err(missing("expected an `id: \"namespace:path\"` field"));
    };
    Namespaced::parse(id).map_err(|err| missing(&err.to_string()))
}

fn typed<T: serde::de::DeserializeOwned>(
    path: &str,
    id: &Namespaced,
    value: ron::Value,
) -> Result<T, LoadError> {
    value.clone().into_rust().map_err(|err| {
        let mut message = err.to_string();
        // serde names a missing or misspelt field but not one of the wrong
        // type ("expected f32 but found a string"), and a file of twenty
        // numbers needs to say which.
        if let Some(field) = blame::<T>(&value) {
            message = format!("{message} (at `{field}`)");
        }
        LoadError::Deserialize {
            path: path.to_owned(),
            id: id.clone(),
            message,
        }
    })
}

/// The field of a raw entry that stops it typing, found by leaving fields
/// out one at a time: the one whose absence makes the rest type (or fail
/// only for its absence) is the culprit. Looks two levels down, into a
/// nested struct or the structs of a list (`rock.cover`, `relief[1].shape`),
/// which is as deep as any def goes. Only run once a def has failed, so its
/// cost is nobody's.
///
/// It tells "only the field's absence" from other errors by the words
/// "field named `x`" in the missing-field message as ron 0.12.2 prints it.
/// A ron that rewords it
/// degrades every registry's message to serde's bare one without failing a
/// load; the wrong-type cases of `a_wrong_province_names_its_file_and_field`
/// catch that.
fn blame<T: serde::de::DeserializeOwned>(value: &ron::Value) -> Option<String> {
    let clears = |candidate: ron::Value, name: &str| match candidate.into_rust::<T>() {
        Ok(_) => true,
        Err(err) => err.to_string().contains(&format!("field named `{name}`")),
    };
    let ron::Value::Map(map) = value else {
        return None;
    };
    for (key, inner) in map.iter() {
        let ron::Value::String(name) = key else {
            continue;
        };
        let mut without = map.clone();
        without.remove(key);
        if !clears(ron::Value::Map(without), name) {
            continue;
        }
        // Which part of it, when it has parts.
        let nested: Vec<(String, ron::Map)> = match inner {
            ron::Value::Map(fields) => vec![(name.clone(), fields.clone())],
            ron::Value::Seq(items) => items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| match item {
                    ron::Value::Map(fields) => Some((format!("{name}[{i}]"), fields.clone())),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for (at, fields) in nested {
            for (sub_key, _) in fields.iter() {
                let ron::Value::String(sub) = sub_key else {
                    continue;
                };
                let mut trimmed = fields.clone();
                trimmed.remove(sub_key);
                let mut candidate = map.clone();
                let replacement = match inner {
                    ron::Value::Seq(items) => {
                        let index = at
                            .rsplit('[')
                            .next()
                            .and_then(|rest| rest.trim_end_matches(']').parse::<usize>().ok());
                        let mut items = items.clone();
                        if let Some(item) = index.and_then(|i| items.get_mut(i)) {
                            *item = ron::Value::Map(trimmed);
                        }
                        ron::Value::Seq(items)
                    }
                    _ => ron::Value::Map(trimmed),
                };
                candidate.insert(key.clone(), replacement);
                if clears(ron::Value::Map(candidate), sub) {
                    return Some(format!("{at}.{sub}"));
                }
            }
        }
        return Some(name.clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;
    use slotted_registry::loader::InMemorySource;

    use super::*;
    use crate::biome::{BiomeId, ReliefKind};
    use crate::material::TerrainMaterialId;

    const GRASS_AND_ROCK: &str = r#"[
        (id: "strata:rock", texture_set: "rock", tiling_m: 6.0),
        (id: "strata:grass", texture_set: "grass", tiling_m: 4.0),
    ]"#;

    fn plains(material: &str) -> String {
        format!(
            r#"(
                id: "strata:plains",
                name: "Plains",
                climate: (
                    temperature: (min: 0.3, max: 0.8),
                    moisture: (min: 0.2, max: 0.7),
                    elevation: (min: 0.5, max: 0.62),
                ),
                relief: (kind: "plains", amplitude_m: 12.0, wavelength_m: 900.0, octaves: 3),
                surface: (material: "{material}", slope_material: "strata:rock"),
                tint: (0.45, 0.62, 0.28),
            )"#
        )
    }

    fn minimal() -> InMemorySource {
        InMemorySource::new()
            .with("biomes/plains.biome.ron", plains("strata:grass"))
            .with("biomes/notes.txt", "ignored")
            .with("terrain/materials.ron", GRASS_AND_ROCK)
    }

    #[test]
    fn a_minimal_set_loads_and_freezes() {
        let frozen = load_from_source(&minimal()).expect("loads");
        assert_eq!(frozen.biomes.len(), 1);
        assert_eq!(frozen.materials.len(), 2);
        let plains = frozen
            .biome_id(&Namespaced::parse("strata:plains").expect("valid"))
            .expect("registered");
        assert_eq!(plains, BiomeId(0));
        let def = frozen.biomes.get(plains).expect("registered");
        assert_eq!(def.relief.kind, ReliefKind::Plains);
        assert!(
            (def.relief.persistence - 0.5).abs() < f32::EPSILON,
            "default applied"
        );
        let surface = frozen.surface(plains).expect("resolved");
        assert_eq!(
            surface.material,
            TerrainMaterialId(0),
            "grass sorts before rock"
        );
        assert_eq!(surface.slope_material, TerrainMaterialId(1));
        assert_eq!(surface.slope_material.layer(), 1);
    }

    #[test]
    fn flora_files_load_and_patch() {
        let source = minimal()
            .with(
                "flora/meadow.flora.ron",
                r#"(
                    id: "strata:meadow",
                    name: "Meadow grass",
                    kind: "grass",
                    biomes: [(biome: "strata:plains", weight: 0.9)],
                    density_per_m2: 6.0,
                    tint_dry: (0.5, 0.5, 0.2),
                    tint_lush: (0.2, 0.5, 0.1),
                )"#,
            )
            .with(
                "flora/patches/updates/thin.ron",
                r#"[(target: "strata:meadow", op: Merge({"density_per_m2": 2.0}))]"#,
            );
        let frozen = load_from_source(&source).expect("loads");
        let id = frozen
            .flora_id(&Namespaced::parse("strata:meadow").expect("valid"))
            .expect("registered");
        let def = frozen.flora.get(id).expect("registered");
        assert!((def.density_per_m2 - 2.0).abs() < f32::EPSILON, "patched");
        assert!((frozen.flora_fit(id).expect("fit").weight(BiomeId(0)) - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn patch_rounds_rewrite_raw_entries_before_typing() {
        let source = minimal()
            .with(
                "biomes/patches/base/tune.ron",
                r#"[(target: "strata:plains", op: Merge({"relief": {"amplitude_m": 40.0}}))]"#,
            )
            .with(
                "biomes/patches/final_fixes/last_word.ron",
                r#"[(target: "strata:plains", op: Merge({"name": "Steppe"}))]"#,
            )
            .with(
                "terrain/patches/updates/tiling.ron",
                r#"[(target: "strata:grass", op: Merge({"tiling_m": 8.0}))]"#,
            );
        let frozen = load_from_source(&source).expect("loads");
        let plains = frozen.biomes.get(BiomeId(0)).expect("registered");
        assert!((plains.relief.amplitude_m - 40.0).abs() < f32::EPSILON);
        assert_eq!(plains.name, "Steppe");
        let grass = frozen
            .materials
            .get(TerrainMaterialId(0))
            .expect("registered");
        assert!((grass.tiling_m - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_patch_can_remove_an_entry() {
        let source = minimal().with(
            "terrain/patches/base/drop.ron",
            r#"[(target: "strata:rock", op: Remove)]"#,
        );
        let err = load_from_source(&source).expect_err("plains needs rock for slopes");
        assert!(
            matches!(err, LoadError::Freeze(FreezeError::UnknownMaterial { .. })),
            "{err}"
        );
    }

    #[test]
    fn errors_name_the_file() {
        let broken = InMemorySource::new().with("biomes/broken.biome.ron", "(id: 42)");
        let err = load_from_source(&broken).expect_err("no id");
        assert!(err.to_string().contains("biomes/broken.biome.ron"), "{err}");

        let bad = InMemorySource::new().with("biomes/bad.biome.ron", "not ron at all (");
        let err = load_from_source(&bad).expect_err("not ron");
        assert!(matches!(err, LoadError::Parse { .. }), "{err}");

        let source = minimal().with("biomes/again.biome.ron", plains("strata:grass"));
        let err = load_from_source(&source).expect_err("duplicate id");
        assert!(matches!(err, LoadError::Duplicate { .. }), "{err}");

        let source = minimal().with(
            "biomes/patches/base/bad.ron",
            r#"[(target: "strata:nope", op: Remove)]"#,
        );
        let err = load_from_source(&source).expect_err("unknown patch target");
        assert!(matches!(err, LoadError::Patch { .. }), "{err}");

        let source = InMemorySource::new()
            .with(
                "biomes/plains.biome.ron",
                plains("strata:grass").replace("\"plains\"", "\"volcano\""),
            )
            .with("terrain/materials.ron", GRASS_AND_ROCK);
        let err = load_from_source(&source).expect_err("unknown relief kind");
        assert!(matches!(err, LoadError::Deserialize { .. }), "{err}");
        assert!(err.to_string().contains("volcano"), "{err}");
    }

    const ROCK_KIT: &str = r#"(
        id: "strata:granite_stone",
        name: "Granite stone",
        kind: "rock",
        biomes: [(biome: "strata:plains", weight: 0.2)],
        spacing_m: 6.0,
        tint_dry: (0.5, 0.5, 0.5),
        tint_lush: (0.5, 0.5, 0.5),
        tree: (height_m: (min: 0.3, max: 0.8)),
    )"#;

    fn province(id: &str, extra: &str) -> String {
        format!(
            r#"(
                id: "{id}",
                name: "A province",
                tint: (0.5, 0.4, 0.3),
                rock: (face: "strata:rock", cover: 0.5),
                {extra}
            )"#
        )
    }

    fn with_geology() -> InMemorySource {
        minimal()
            .with("flora/granite_stone.flora.ron", ROCK_KIT)
            .with(
                "geology/granite.geology.ron",
                province(
                    "strata:granite",
                    r#"kit: [(species: "strata:granite_stone", weight: 0.7)],"#,
                ),
            )
            .with(
                "geology/sandstone.geology.ron",
                province("strata:sandstone", r#"in_ranges: Some("strata:granite"),"#),
            )
            .with("geology/notes.txt", "ignored")
    }

    #[test]
    fn provinces_load_resolve_and_fit_their_kits() {
        let frozen = load_from_source(&with_geology()).expect("loads");
        assert_eq!(frozen.provinces.len(), 2);
        let granite = frozen
            .province_id(&Namespaced::parse("strata:granite").expect("valid"))
            .expect("registered");
        let sandstone = frozen
            .province_id(&Namespaced::parse("strata:sandstone").expect("valid"))
            .expect("registered");
        assert_eq!((granite.0, sandstone.0), (0, 1), "sorted by id");
        let traits = frozen.province(sandstone).expect("resolved");
        assert_eq!(traits.in_ranges, granite);
        assert_eq!(traits.rock.face, TerrainMaterialId(1), "rock sorts second");
        assert!((traits.rock.cover - 0.5).abs() < f32::EPSILON);
        assert_eq!(
            frozen.province(granite).expect("resolved").in_ranges,
            granite,
            "none named is itself"
        );
        let stone = frozen
            .flora_id(&Namespaced::parse("strata:granite_stone").expect("valid"))
            .expect("registered");
        let fit = frozen.flora_fit(stone).expect("fit");
        assert!(fit.in_a_kit());
        assert!((fit.province_weight(granite) - 0.7).abs() < f32::EPSILON);
        assert!(fit.province_weight(sandstone).abs() < f32::EPSILON);
        assert!(fit.province_weight(crate::ProvinceId::NONE).abs() < f32::EPSILON);
        assert_eq!(frozen.province(crate::ProvinceId::NONE), None);
    }

    #[test]
    fn a_world_without_geology_has_no_provinces() {
        let frozen = load_from_source(&minimal()).expect("loads");
        assert!(frozen.provinces.is_empty());
        assert_eq!(frozen.province_ids().count(), 0);
    }

    #[test]
    fn province_patches_run_in_three_rounds() {
        let source = with_geology()
            .with(
                "geology/patches/base/soften.ron",
                r#"[(target: "strata:granite", op: Merge({"rock": {"cover": 0.25}}))]"#,
            )
            .with(
                "geology/patches/updates/rename.ron",
                r#"[(target: "strata:sandstone", op: Merge({"name": "Redrock"}))]"#,
            )
            .with(
                "geology/patches/final_fixes/drop.ron",
                r#"[(target: "strata:sandstone", op: Merge({"in_ranges": None}))]"#,
            );
        let frozen = load_from_source(&source).expect("loads");
        let granite = frozen.provinces.get(crate::ProvinceId(0)).expect("there");
        assert!((granite.rock.cover - 0.25).abs() < f32::EPSILON);
        let sandstone = frozen.provinces.get(crate::ProvinceId(1)).expect("there");
        assert_eq!(sandstone.name, "Redrock");
        assert_eq!(sandstone.in_ranges, None);
        assert_eq!(
            frozen
                .province(crate::ProvinceId(1))
                .expect("resolved")
                .in_ranges,
            crate::ProvinceId(1)
        );
    }

    /// A wrong province file is refused at load, with a message naming the
    /// file and the field, for every kind of wrong: a name nobody
    /// registered, a number out of bounds, a string that is not of its
    /// kind, a field that is missing, misspelt or of the wrong type.
    #[test]
    fn a_wrong_province_names_its_file_and_field() {
        let cases: [(&str, &str, &str); 14] = [
            (
                r#"in_ranges: Some("strata:gneiss"),"#,
                "in_ranges",
                "gneiss",
            ),
            (
                r#"kit: [(species: "strata:oak", weight: 0.5)],"#,
                "kit[0].species",
                "oak",
            ),
            (
                r#"kit: [(species: "strata:meadow", weight: 0.5)],"#,
                "kit[0].species",
                "kind rock",
            ),
            ("frequency: -1.0,", "frequency", "over 0"),
            // 0.1 of 2.1: the field could deal it no cell.
            ("frequency: 0.1,", "frequency", "sixteenth"),
            (
                r#"relief: [(class: "volcano", shape: "fbm")],"#,
                "relief[0].class",
                "volcano",
            ),
            (
                r#"relief: [(class: "hills", shape: "perlin")],"#,
                "relief[0].shape",
                "perlin",
            ),
            ("beds: (strength: 2.0),", "beds.strength", "0..=1"),
            (r#"water: (section: "canyon"),"#, "water.section", "canyon"),
            ("water: (hang: 0.0),", "water.hang", "(0, 1]"),
            ("colour: (1.0, 0.0, 0.0),", "colour", ""),
            ("beds: (strenght: 0.5),", "strenght", ""),
            (
                r#"relief: [(class: "hills", shape: "fbm", damping: "lots")],"#,
                "`relief[0].damping`",
                "string",
            ),
            (r#"frequency: "often","#, "`frequency`", "string"),
        ];
        for (extra, field, detail) in cases {
            let source = with_geology()
                .with(
                    "flora/meadow.flora.ron",
                    r#"(id: "strata:meadow", name: "Meadow", kind: "grass",
                        biomes: [(biome: "strata:plains", weight: 0.9)],
                        density_per_m2: 6.0,
                        tint_dry: (0.5, 0.5, 0.2), tint_lush: (0.2, 0.5, 0.1))"#,
                )
                .with("geology/wrong.geology.ron", province("strata:wrong", extra));
            let err = load_from_source(&source).expect_err(extra);
            let text = err.to_string();
            assert!(
                text.contains("geology/wrong.geology.ron"),
                "{extra}: {text}"
            );
            assert!(text.contains(field), "{extra}: `{field}` not in {text}");
            assert!(text.contains(detail), "{extra}: `{detail}` not in {text}");
        }

        let source = with_geology().with(
            "geology/faceless.geology.ron",
            province("strata:faceless", "").replace("strata:rock", "strata:marble"),
        );
        let err = load_from_source(&source).expect_err("marble is not a material");
        let text = err.to_string();
        assert!(
            text.contains("geology/faceless.geology.ron") && text.contains("rock.face"),
            "{text}"
        );
        assert!(text.contains("marble"), "{text}");

        let source = with_geology().with(
            "geology/cover.geology.ron",
            province("strata:cover", "").replace("cover: 0.5", "cover: true"),
        );
        let err = load_from_source(&source).expect_err("a boolean cover");
        let text = err.to_string();
        assert!(
            text.contains("geology/cover.geology.ron") && text.contains("`rock.cover`"),
            "{text}"
        );

        let source = with_geology().with(
            "geology/bare.geology.ron",
            r#"(id: "strata:bare", name: "Bare", tint: (0.5, 0.5, 0.5))"#,
        );
        let err = load_from_source(&source).expect_err("no rock");
        let text = err.to_string();
        assert!(
            text.contains("geology/bare.geology.ron") && text.contains("rock"),
            "{text}"
        );

        let source =
            with_geology().with("geology/again.geology.ron", province("strata:granite", ""));
        let err = load_from_source(&source).expect_err("two granites");
        assert!(matches!(err, LoadError::Duplicate { .. }), "{err}");
    }

    /// A frequency at a sixteenth of the whole is enough for the field to
    /// deal it a cell of every block, and is taken.
    #[test]
    fn a_sixteenth_of_the_frequencies_is_enough() {
        // 0.2 of 3.2 is a sixteenth exactly.
        let source = with_geology().with(
            "geology/rare.geology.ron",
            province("strata:rare", "frequency: 0.2,"),
        );
        let frozen = load_from_source(&source).expect("a sixteenth loads");
        assert_eq!(frozen.provinces.len(), 3);
    }

    /// A province's face must be one of the layers the terrain draws: its
    /// mesher packs a pair of faces into four bits each, and a face past
    /// the sixteenth layer drew as another rock with no error.
    #[test]
    fn a_face_past_the_drawn_layers_is_refused() {
        let mut materials = String::from("[\n");
        for k in 0..17 {
            writeln!(
                materials,
                r#"(id: "strata:a{k:02}", texture_set: "a{k:02}", tiling_m: 4.0),"#
            )
            .expect("a String takes it");
        }
        materials.push_str(
            "(id: \"strata:grass\", texture_set: \"grass\", tiling_m: 4.0),\n\
             (id: \"strata:rock\", texture_set: \"rock\", tiling_m: 6.0),\n]",
        );
        let source = |face: &str| {
            InMemorySource::new()
                .with("biomes/plains.biome.ron", plains("strata:grass"))
                .with("terrain/materials.ron", materials.clone())
                .with(
                    "geology/deep.geology.ron",
                    province("strata:deep", "").replace("strata:rock", face),
                )
        };
        // Sorted, a00 to a15 are layers 0 to 15 and a16 is layer 16.
        load_from_source(&source("strata:a15")).expect("layer 15 is drawn");
        let err = load_from_source(&source("strata:a16")).expect_err("layer 16 is not");
        let text = err.to_string();
        assert!(
            text.contains("geology/deep.geology.ron")
                && text.contains("rock.face")
                && text.contains("layer 16"),
            "{text}"
        );
    }

    #[test]
    fn at_most_sixteen_provinces() {
        let mut source = with_geology();
        for k in 0..15 {
            source = source.with(
                format!("geology/p{k:02}.geology.ron"),
                province(&format!("strata:p{k:02}"), ""),
            );
        }
        let err = load_from_source(&source).expect_err("seventeen");
        assert!(
            matches!(
                err,
                LoadError::Freeze(FreezeError::TooManyProvinces { count: 17, .. })
            ),
            "{err}"
        );
    }

    fn site_kind(id: &str, rank: u32, wants: &str) -> String {
        format!(
            r#"(
                id: "{id}", name: "A kind", rank: {rank}, per_1000_km2: 10.0,
                footprint_m: 30.0, blend_m: 20.0, max_cut_m: 4.0,
                spacing: (own_m: 2000.0, others_m: 800.0),
                ground: (slope_max: 0.2, height_m: (min: 2.0, max: 500.0),
                         biomes: [(id: "strata:plains", weight: 0.5)]),
                wants: [{wants}],
                facing: "water", road: "track", group: "veyra:test",
                names: (onsets: ["b", "d"], vowels: ["a"], codas: ["", "n"],
                        syllables: [2, 2], patterns: ["{{root}} Ford"]),
            )"#
        )
    }

    /// Site kinds load, patch and freeze: their strings parsed, their
    /// biomes resolved, a kind a term names resolved to its id and placed
    /// first; a set without the directory has none.
    #[test]
    fn site_kinds_load_patch_and_resolve() {
        assert!(
            load_from_source(&minimal())
                .expect("loads")
                .sites
                .is_empty()
        );
        let source = minimal()
            .with("sites/town.site.ron", site_kind("strata:town", 0, ""))
            .with(
                "sites/hamlet.site.ron",
                site_kind(
                    "strata:hamlet",
                    1,
                    r#"(any: [(feature: "kind", kind: "strata:town", within_m: 5000.0),
                              (feature: "view", at_least: 0.2, full: 0.8)], required: true)"#,
                ),
            )
            .with(
                "sites/patches/updates/wider.ron",
                r#"[(target: "strata:town", op: Merge({"footprint_m": 60.0}))]"#,
            );
        let frozen = load_from_source(&source).expect("loads");
        let town = frozen
            .site_kind_id(&Namespaced::parse("strata:town").expect("valid"))
            .expect("registered");
        let hamlet = frozen
            .site_kind_id(&Namespaced::parse("strata:hamlet").expect("valid"))
            .expect("registered");
        assert_eq!((hamlet.0, town.0), (0, 1), "sorted by id");
        assert!((frozen.sites.get(town).expect("there").footprint_m - 60.0).abs() < 1e-12);
        let traits = frozen.site_kind(hamlet).expect("resolved");
        assert_eq!(traits.order, 1, "placed after the town");
        assert_eq!(traits.wants[0].any[0].kind, Some(town));
        assert!(traits.wants[0].any[1].feature.is_measure());
        assert!((traits.biomes[0] - 0.5).abs() < 1e-12);
        assert_eq!(traits.facing, crate::site::Facing::Water);
    }

    /// A wrong site kind is refused at load with its file and field: a
    /// feature nobody knows, a kind placed after it, a measure without its
    /// bounds, a pad too wide for its spacing, a biome nobody registered.
    #[test]
    fn a_wrong_site_kind_names_its_file_and_field() {
        let cases = [
            (
                r#"(any: [(feature: "volcano", within_m: 10.0)])"#,
                "wants[0].any[0].feature",
            ),
            (
                r#"(any: [(feature: "kind", kind: "strata:town", within_m: 10.0)])"#,
                "wants[0].any[0].kind",
            ),
            (r#"(any: [(feature: "view")])"#, "wants[0].any[0].full"),
            (r#"(any: [(feature: "river")])"#, "wants[0].any[0].within_m"),
        ];
        for (wants, field) in cases {
            let source = minimal()
                .with("sites/town.site.ron", site_kind("strata:town", 1, ""))
                .with("sites/wrong.site.ron", site_kind("strata:wrong", 0, wants));
            let text = load_from_source(&source).expect_err(wants).to_string();
            assert!(
                text.contains("sites/wrong.site.ron") && text.contains(field),
                "{wants}: {text}"
            );
        }
        let source = minimal().with(
            "sites/wide.site.ron",
            site_kind("strata:wide", 0, "").replace("own_m: 2000.0", "own_m: 60.0"),
        );
        let text = load_from_source(&source)
            .expect_err("too close")
            .to_string();
        assert!(text.contains("spacing.own_m"), "{text}");
        let source = minimal().with(
            "sites/swamp.site.ron",
            site_kind("strata:swamp", 0, "").replace("strata:plains", "strata:swamp"),
        );
        let text = load_from_source(&source).expect_err("no swamp").to_string();
        assert!(text.contains("ground.biomes[0].id"), "{text}");
        // A band narrower than the developed skirt and its fade.
        let source = minimal().with(
            "sites/thin.site.ron",
            site_kind("strata:thin", 0, "").replace("blend_m: 20.0", "blend_m: 10.0"),
        );
        let text = load_from_source(&source).expect_err("too thin").to_string();
        assert!(text.contains("blend_m"), "{text}");
        // Syllables that can only make a root of two letters, or of over
        // nine: the bake would draw names forever.
        for (parts, syllables) in [
            ((r#"codas: ["", "n"]"#, r#"codas: [""]"#), "[1, 1]"),
            ((r#"vowels: ["a"]"#, r#"vowels: ["aeio"]"#), "[4, 4]"),
        ] {
            let kind = site_kind("strata:mute", 0, "");
            assert!(kind.contains(parts.0), "{}", parts.0);
            let kind = kind
                .replace(parts.0, parts.1)
                .replace("syllables: [2, 2]", &format!("syllables: {syllables}"));
            let source = minimal().with("sites/mute.site.ron", kind);
            let text = load_from_source(&source).expect_err("no name").to_string();
            assert!(
                text.contains("sites/mute.site.ron") && text.contains("names"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_missing_material_file_is_an_empty_list() {
        let source = InMemorySource::new().with("biomes/plains.biome.ron", plains("strata:grass"));
        let err = load_from_source(&source).expect_err("grass is not registered");
        assert!(
            matches!(err, LoadError::Freeze(FreezeError::UnknownMaterial { .. })),
            "{err}"
        );
    }
}
