//! The engine's data layer: the files port, the data bundle, the loader
//! frame and the world's schemas (biomes, terrain materials, flora, geological
//! provinces, site kinds, settlements).
//!
//! Built on `slotted-registry` rather than a second registry system (PLAN
//! principle 5): namespaced ids, a data stage that loads and patches RON in
//! three rounds, and a freeze that hands back immutable registries with
//! dense handles. Everything that samples the world holds a frozen
//! [`WorldDefs`] and never a mutable one, so lookups on the compute pool are
//! reads of fixed data.
//!
//! This is the half the world crates (`strata-worldgen`, `strata-flora`,
//! `strata-weather`) link, so an edit to an item, a schematic or a quest
//! (`veyra-data`) leaves them, and their optimised build, alone.
//!
//! The shape of a load, from the app's point of view:
//!
//! ```no_run
//! let world = strata_data::load_world_from_dir("assets")?;
//! let plains = world
//!     .biome_id(&strata_data::Namespaced::parse("strata:plains")?)
//!     .expect("shipped in assets/biomes");
//! let surface = world.surface(plains).expect("resolved at freeze");
//! println!("plains paint texture layer {}", surface.material.layer());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! No Bevy: the same function runs on the server, in the map tool and in
//! tests.

pub mod biome;
pub mod building_look;
pub mod bundle;
pub mod canopy_look;
pub mod cloud_look;
pub mod defs;
pub mod files;
pub mod flora;
pub mod geology;
pub mod load;
pub mod material;
pub mod settlement;
pub mod site;
pub mod terrain_arrays;
pub mod values;

pub use biome::{
    BiomeDef, BiomeId, ClimateWindow, GRADE_WHITE_SHIFT, Grade, Relief, ReliefKind, Surface, Tint,
    UnitRange, UnknownReliefKind,
};
pub use building_look::{BUILDINGS_FILE, BuildingLook};
pub use canopy_look::{CANOPY_FILE, CanopyLook};
pub use cloud_look::{CLOUDS_FILE, CloudLook, DensityLook, LightLook, ShapeLook};
pub use defs::{BiomeSurface, FloraFit, FreezeError, OpenRegistries, WorldDefs};
pub use flora::{
    BiomeWeight, ChildRule, Clustering, FloraDef, FloraId, FloraKind, GroundPrefs, MetreRange,
    TreeParams,
};
pub use geology::{
    Beds, KitEntry, MAX_PROVINCES, ProvinceDef, ProvinceId, ProvinceRock, ProvinceShape,
    ProvinceShapeRule, ProvinceTraits, Rock, ShapeRule, Water, WaterSection,
};
pub use load::{
    BIOME_SUFFIX, BIOMES_DIR, GEOLOGY_DIR, GEOLOGY_SUFFIX, LoadError, MATERIALS_FILE, SITE_SUFFIX,
    SITES_DIR, load_world_from_dir, load_world_from_source,
};
pub use material::{
    Laid, MAX_DRAWN_LAYERS, MAX_TERRAIN_MATERIALS, TerrainMaterialDef, TerrainMaterialId,
};
pub use settlement::{
    ExtentDef, GroundDef, LandformDef, PlatformDef, RoadDef, SettlementDef, SpurDef, TerracesDef,
};
pub use site::{
    Facing, FeatureNamesDef, GroundLimitsDef, MetresDef, NamesDef, PAD_SKIRT_FADE_M, PAD_SKIRT_M,
    RoadClass, SiteFeature, SiteKindDef, SiteKindId, SiteKindTraits, SiteTerm, SiteWant,
    SpacingDef, TermDef, WantDef, WeightDef,
};
pub use slotted_model::Namespaced;
pub use slotted_registry::loader::{AssetSource, DirSource, InMemorySource, SourceError};
pub use slotted_registry::registry::Registry;
pub use values::{
    CharacterLook, VALUES_FILE, ValueBudget, ValueFamily, far_colour, linear_to_srgb, luminance,
    srgb_to_linear,
};

/// The frozen biome registry, the type of [`WorldDefs::biomes`]. Named
/// so the facade can re-export it alongside [`BiomeId`].
pub type BiomeStore = Registry<BiomeDef, BiomeId>;
/// The frozen terrain material registry, the type of
/// [`WorldDefs::materials`].
pub type TerrainMaterialStore = Registry<TerrainMaterialDef, TerrainMaterialId>;
/// The frozen province registry, the type of [`WorldDefs::provinces`].
pub type ProvinceStore = Registry<ProvinceDef, ProvinceId>;
/// The frozen site kind registry, the type of [`WorldDefs::sites`].
pub type SiteKindStore = Registry<SiteKindDef, SiteKindId>;
