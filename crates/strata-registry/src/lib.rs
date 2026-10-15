//! The game's registries: biomes, terrain materials, flora, geological
//! provinces, settlements, site kinds, items, resources, schematics, and
//! later quests.
//!
//! Built on `slotted-registry` rather than a second registry system (PLAN
//! principle 5): namespaced ids, a data stage that loads and patches RON in
//! three rounds, and a freeze that hands back immutable registries with
//! dense handles. Everything that samples the world holds a frozen
//! [`GameRegistries`] and never a mutable one, so lookups on the compute
//! pool are reads of fixed data.
//!
//! The shape of a load, from the app's point of view:
//!
//! ```no_run
//! let registries = strata_registry::load_from_dir("assets")?;
//! let plains = registries
//!     .biome_id(&strata_registry::Namespaced::parse("strata:plains")?)
//!     .expect("shipped in assets/biomes");
//! let surface = registries.surface(plains).expect("resolved at freeze");
//! println!("plains paint texture layer {}", surface.material.layer());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! No Bevy: the same function runs on the server, in the map tool and in
//! tests. The Bevy side of loading (doing it in the `LoadingRegistries`
//! state and handing the result to the world source) lives in `strata-sim`.

pub mod biome;
pub mod bundle;
pub mod encounter;
pub mod faction;
pub mod fall;
pub mod files;
pub mod flora;
pub mod geology;
pub mod icons;
pub mod item;
pub mod load;
pub mod loot;
pub mod material;
pub mod models;
pub mod pictures;
pub mod posting;
pub mod procurement;
pub mod progression;
pub mod registries;
pub mod resource;
pub mod rework;
pub mod schematic;
pub mod settlement;
pub mod site;
pub mod vehicle;
pub mod wear;

pub use biome::{
    BiomeDef, BiomeId, ClimateWindow, GRADE_WHITE_SHIFT, Grade, Relief, ReliefKind, Surface, Tint,
    UnitRange, UnknownReliefKind,
};
pub use encounter::{
    Crew, ENCOUNTER_SUFFIX, ENCOUNTER_VERSION, ENCOUNTERS_DIR, EncounterAt, EncounterDef,
    EncounterDefId, EncounterError, EncounterSet, FoeKind, Person,
};
pub use faction::{
    ANCHORS_FILE, AnchorDef, AnchorsFile, CLAIM_RULES_FILE, ClaimKind, ClaimRules, DEED_RULES_FILE,
    DeedRules, DeedWeights, FACTION_RULES_FILE, FACTION_SUFFIX, FACTIONS_DIR, FactionError,
    FactionKindDef, FactionRules, FactionSet, HoldDef, LeaseDef, Nature, PerMassDef, PriceFactor,
    StanceBands, parse_colour,
};
pub use fall::{FALL_FILE, FallRules};
pub use flora::{
    BiomeWeight, ChildRule, Clustering, FloraDef, FloraId, FloraKind, GroundPrefs, MetreRange,
    TreeParams,
};
pub use geology::{
    Beds, KitEntry, MAX_PROVINCES, ProvinceDef, ProvinceId, ProvinceRock, ProvinceShape,
    ProvinceShapeRule, ProvinceTraits, Rock, ShapeRule, Water, WaterSection,
};
pub use icons::{
    CATALOGUE_FILE, CATALOGUE_VERSION, Family, Icon, IconCatalogue, IconError, ImageSource,
    ItemImage, item_image,
};
pub use item::{
    BAG_SLOTS, Band, Better, CarryRules, CountRule, EffectDef, Fitted, FittingRules, GridDef,
    HANDS, IconShape, IconSpec, ItemCatalogue, ItemDefinition, ItemError, ItemKind, LineDef, LotOf,
    LotOrigin, MODDABILITY, MountDef, PatchKeys, Property, QUALITY_MAX, Record, STORE_GRADE,
    SavedPack, StartingLot, StartingStock, ToolReads, lot_item,
};
pub use load::{
    BIOME_SUFFIX, BIOMES_DIR, CARRY_FILE, FITTING_FILE, GEOLOGY_DIR, GEOLOGY_SUFFIX, ITEM_SUFFIX,
    ITEMS_DIR, LoadError, MATERIALS_FILE, SITE_SUFFIX, SITES_DIR, STARTING_FILE, WEAR_FILE,
    load_from_dir, load_from_source,
};
pub use loot::{
    LOOT_DIR, LOOT_SUFFIX, LOOT_VERSION, LootDef, LootError, LootSet, LotPick, Pick, SALVAGE_MAKER,
    TierLoot,
};
pub use material::{
    MAX_DRAWN_LAYERS, MAX_TERRAIN_MATERIALS, TerrainMaterialDef, TerrainMaterialId,
};
pub use posting::{POSTINGS_FILE, PostingRules};
pub use procurement::{
    BUYER_SUFFIX, BUYER_VERSION, BUYERS_DIR, BuyerDef, BuyerError, BuyerSet, GradeOn, LineSpec,
    OrderDef, OrderKind, Premium, Refill, Wants,
};
pub use progression::{
    AuthoredSheets, BoxDef, BranchDef, Effect, Op, ProgressionError, SheetDef, SkillGraph, TreeDef,
    XpKind, XpRates,
};
pub use registries::{BiomeSurface, FloraFit, FreezeError, GameRegistries, OpenRegistries};
pub use resource::{
    AreaDef, Attribute, CLASSES_FILE, ClassDef, ClassTree, DEPOSITS_FILE, DepositRules, FamilyDef,
    HARVEST_FILE, HarvestRules, Occurs, PingDef, RESOURCE_SUFFIX, RESOURCES_DIR, Reading,
    ResourceError, ResourceSet, ResourceTypeId, SURVEY_FILE, SampleDef, Span, SurveyRules, TypeDef,
};
pub use rework::{Closes, REWORK_FILE, ReworkRules};
pub use schematic::{
    FACTORY_FILE, FactoryRules, Hand, ODDS_ROWS, Outcome, SCHEMATIC_SUFFIX, SCHEMATICS_DIR,
    SESSION_FILE, Schematic, SchematicDef, SchematicError, SchematicSet, SessionRules, Stat, Takes,
};
pub use settlement::{
    ExtentDef, GroundDef, LandformDef, PlatformDef, RoadDef, SettlementDef, SpurDef, TerracesDef,
};
pub use site::{
    Facing, FeatureNamesDef, GroundLimitsDef, MetresDef, NamesDef, PAD_SKIRT_FADE_M, PAD_SKIRT_M,
    RoadClass, SiteFeature, SiteKindDef, SiteKindId, SiteKindTraits, SiteTerm, SiteWant,
    SpacingDef, TermDef, WantDef, WeightDef,
};
pub use slotted_registry::registry::Registry;
pub use vehicle::{VehicleDef, VehicleId, VehicleSet};
pub use wear::{DURABILITY, FieldRepair, RESTORE, WearRules};

/// The frozen biome registry, the type of [`GameRegistries::biomes`]. Named
/// so the facade can re-export it alongside [`BiomeId`].
pub type BiomeStore = Registry<BiomeDef, BiomeId>;
/// The frozen terrain material registry, the type of
/// [`GameRegistries::materials`].
pub type TerrainMaterialStore = Registry<TerrainMaterialDef, TerrainMaterialId>;
/// The frozen province registry, the type of [`GameRegistries::provinces`].
pub type ProvinceStore = Registry<ProvinceDef, ProvinceId>;
/// The frozen site kind registry, the type of [`GameRegistries::sites`].
pub type SiteKindStore = Registry<SiteKindDef, SiteKindId>;
pub use slotted_model::Namespaced;
pub use slotted_registry::loader::{AssetSource, DirSource, InMemorySource, SourceError};

#[cfg(test)]
mod smoke {
    use super::*;

    /// The shipped data set is the one the game boots with; if it stops
    /// loading, nothing above this crate works.
    #[test]
    fn the_shipped_assets_load() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        let registries = load_from_dir(root).expect("assets/ loads and freezes");
        assert!(registries.biomes.len() >= 7, "at least seven biomes ship");
        assert!(
            registries.icons.len() > 200,
            "the icon catalogue loads with the rest"
        );
        for name in [
            "ocean",
            "coast",
            "plains",
            "forest",
            "desert",
            "mountains",
            "tundra",
        ] {
            let id = Namespaced::parse(&format!("strata:{name}")).expect("valid");
            assert!(registries.biome_id(&id).is_some(), "{id} ships");
        }
        for name in ["grass", "forest_floor", "sand", "rock", "snow", "gravel"] {
            let id = Namespaced::parse(&format!("strata:{name}")).expect("valid");
            assert!(registries.material_id(&id).is_some(), "{id} ships");
        }
        // The six provinces of ticket 0004's step 3, each on its own rock
        // but karst, whose limestone is the shared rock (its step 3c).
        assert_eq!(registries.provinces.len(), 6, "six provinces ship");
        for (name, face) in [
            ("granite", "granite"),
            ("sandstone", "sandstone"),
            ("karst", "rock"),
            ("basalt", "basalt"),
            ("folded", "slate"),
            ("badlands", "clay"),
        ] {
            let id = Namespaced::parse(&format!("strata:{name}")).expect("valid");
            let province = registries.province_id(&id).expect("ships");
            let traits = registries.province(province).expect("resolved");
            let face = registries
                .material_id(&Namespaced::parse(&format!("strata:{face}")).expect("valid"))
                .expect("the face ships");
            assert_eq!(traits.rock.face, face, "{id}'s face");
        }
        // Four of the five new faces have a grain that keeps its direction.
        let oriented: Vec<String> = registries
            .materials
            .iter()
            .filter(|(_, _, def)| def.oriented)
            .map(|(_, name, _)| name.to_string())
            .collect();
        assert_eq!(
            oriented,
            [
                "strata:basalt",
                "strata:clay",
                "strata:sandstone",
                "strata:slate"
            ]
        );
    }
}
