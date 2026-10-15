//! Resources: what the ground holds (ticket 0007, step 3;
//! `docs/design/crafting.md`, section 3), as `resources/` spells it: the
//! class tree in `classes.ron`, one type a `*.resource.ron` file, and the
//! dials of where deposits lie and how a lot is drawn in `deposits.ron`.
//!
//! The tree is family, class, type. A class names the attributes it carries,
//! out of the eleven of [`Attribute`]; a type gives each of them a floor and
//! a cap ([`Span`]), and that is all a type is allowed to range: an attribute
//! a class does not carry is absent from its lots, not nought, and a type
//! that ranged one would be a number nothing reads.
//!
//! Where a type occurs is data here and not in the geology files, which are
//! hashed into the world's version: a weight anywhere, a weight a province
//! and a biome, and a weight by the water record at the point
//! ([`OccursDef`]). The freeze resolves the names to dense ids
//! ([`Occurs`]), so the sweep that places deposits
//! (`strata_worldgen::deposits`) never touches a name.
//!
//! None of it is world state: it joins no world version and no handshake,
//! and a deposit moves no ground. A data set without the directory has no
//! resources, no deposits and no lots, and that is legal.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;
use slotted_registry::icon::IconColor;

use crate::biome::BiomeId;
use crate::geology::ProvinceId;
use crate::item::{IconShape, QUALITY_MAX};
use crate::registries::GameRegistries;

/// The directory of the resource files, relative to the asset root. Absent
/// for a data set with nothing in the ground.
pub const RESOURCES_DIR: &str = "resources";
/// The suffix a resource type file must carry to be read.
pub const RESOURCE_SUFFIX: &str = ".resource.ron";
/// The class tree, relative to the asset root.
pub const CLASSES_FILE: &str = "resources/classes.ron";
/// The deposits' dials, relative to the asset root.
pub const DEPOSITS_FILE: &str = "resources/deposits.ron";
/// The survey dials, the path under `assets/`.
pub const SURVEY_FILE: &str = "resources/survey.ron";
/// The field harvester's dials, the path under `assets/`.
pub const HARVEST_FILE: &str = "resources/harvest.ron";
/// The fewest points a ping's grid side may have.
pub const GRID_MIN: u32 = 3;
/// The most points a ping's grid side may have.
pub const GRID_MAX: u32 = 33;

/// The eleven attributes a resource can carry (crafting.md, 3.2), on the
/// scale of nought to [`QUALITY_MAX`]. A class carries some of them, never
/// all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Attribute {
    /// How little of it is something else.
    Purity,
    /// Load before it deforms.
    Strength,
    /// How it takes shock.
    Toughness,
    /// How it keeps a surface.
    Hardness,
    /// Electrical.
    Conductivity,
    /// Holds its properties hot.
    HeatTolerance,
    /// Resists corrosion and age.
    Stability,
    /// Mass for its volume. Reads both ways: light for handling, heavy for
    /// impact.
    Density,
    /// Chemical or biological energy.
    Potency,
    /// How forgiving it is to work.
    Workability,
    /// How a body takes it.
    Biocompatibility,
}

impl Attribute {
    /// Every attribute, in the design's order.
    pub const ALL: [Self; 11] = [
        Self::Purity,
        Self::Strength,
        Self::Toughness,
        Self::Hardness,
        Self::Conductivity,
        Self::HeatTolerance,
        Self::Stability,
        Self::Density,
        Self::Potency,
        Self::Workability,
        Self::Biocompatibility,
    ];

    /// The attribute's stable key (`heat_tolerance`): a lot's line id in a
    /// record, and what a string table keys the attribute by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Purity => "purity",
            Self::Strength => "strength",
            Self::Toughness => "toughness",
            Self::Hardness => "hardness",
            Self::Conductivity => "conductivity",
            Self::HeatTolerance => "heat_tolerance",
            Self::Stability => "stability",
            Self::Density => "density",
            Self::Potency => "potency",
            Self::Workability => "workability",
            Self::Biocompatibility => "biocompatibility",
        }
    }

    /// The two-letter code (`HT`), for a dense row. A card uses the name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Purity => "PU",
            Self::Strength => "ST",
            Self::Toughness => "TG",
            Self::Hardness => "HD",
            Self::Conductivity => "CN",
            Self::HeatTolerance => "HT",
            Self::Stability => "SB",
            Self::Density => "DN",
            Self::Potency => "PT",
            Self::Workability => "WK",
            Self::Biocompatibility => "BC",
        }
    }

    /// The full name (`Heat tolerance`), as a card writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Purity => "Purity",
            Self::Strength => "Strength",
            Self::Toughness => "Toughness",
            Self::Hardness => "Hardness",
            Self::Conductivity => "Conductivity",
            Self::HeatTolerance => "Heat tolerance",
            Self::Stability => "Stability",
            Self::Density => "Density",
            Self::Potency => "Potency",
            Self::Workability => "Workability",
            Self::Biocompatibility => "Biocompatibility",
        }
    }

    /// The attribute a record's line id names, if it is one.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    /// Its place in [`Self::ALL`]: a stable small number to derive a roll
    /// from.
    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }
}

/// The floor and the cap of one attribute in one type. No deposit of the
/// type reads under the floor or over the cap, which is what makes a type an
/// identity: no light alloy is as strong as good ferrous ore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    /// The least a deposit of the type reads.
    pub floor: u16,
    /// The most.
    pub cap: u16,
}

impl Span {
    /// Whether `value` is one a deposit of the type can read.
    #[must_use]
    pub const fn contains(self, value: u16) -> bool {
        value >= self.floor && value <= self.cap
    }

    /// Where `value` stands in the span, nought at the floor to one at the
    /// cap; clamped, so a commodity lot's flat figure outside a span reads
    /// as its nearer end.
    #[must_use]
    pub fn share(self, value: u16) -> f64 {
        let width = f64::from(self.cap.saturating_sub(self.floor)).max(1.0);
        (f64::from(value.saturating_sub(self.floor)) / width).clamp(0.0, 1.0)
    }
}

/// A family: the top of the tree. It gives a lot's cell its shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyDef {
    /// The namespaced id, `strata:mineral`.
    pub id: Namespaced,
    /// What a card calls it.
    pub name: String,
    /// The shape of a lot's cell.
    pub shape: IconShape,
}

/// A class: what a schematic's slot usually asks for. It gives a lot's cell
/// its colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassDef {
    /// The namespaced id, `strata:reactive`.
    pub id: Namespaced,
    /// What a card calls it.
    pub name: String,
    /// Its family.
    pub family: Namespaced,
    /// The colour of a lot's cell and of its mark on the map, `#rrggbb`.
    pub color: IconColor,
    /// The library symbol (`veyra:icon.material.ore`) a lot of the class
    /// draws, tinted by [`Self::color`]; absent draws the family's shape.
    #[serde(default)]
    pub symbol: Option<String>,
    /// The attributes it carries, in the order a card lists them.
    pub carries: Vec<Attribute>,
}

/// `resources/classes.ron`: the families and the classes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassTree {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The families.
    pub families: Vec<FamilyDef>,
    /// The classes.
    pub classes: Vec<ClassDef>,
}

impl ClassTree {
    /// The file's current shape.
    pub const VERSION: u32 = 1;
}

/// A registry entry and the weight it adds where it is the ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightOf {
    /// The province or the biome.
    pub id: Namespaced,
    /// What it adds, times its share of the point.
    pub weight: f64,
}

/// What the water record at a point adds to a type's weight.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaterWeights {
    /// Where the record is a lake's: its bed's rim and its shore.
    #[serde(default)]
    pub lake: f64,
    /// Where it is a river's: its valley.
    #[serde(default)]
    pub river: f64,
}

/// Where a type occurs, as its file spells it. The weight at a point is
/// `anywhere`, plus each named province's and biome's share there times its
/// weight, plus the water record's.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccursDef {
    /// The weight on any ground at all.
    #[serde(default)]
    pub anywhere: f64,
    /// By province.
    #[serde(default)]
    pub provinces: Vec<WeightOf>,
    /// By biome.
    #[serde(default)]
    pub biomes: Vec<WeightOf>,
    /// By the water record.
    #[serde(default)]
    pub water: WaterWeights,
}

/// One type, as a `.resource.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDef {
    /// The namespaced id, `strata:evaporite_salt`.
    pub id: Namespaced,
    /// What a card calls the type: `Evaporite salt`.
    pub name: String,
    /// The word a lot's name ends in: `salt`, for "Calder salt".
    pub short: String,
    /// Its class.
    pub class: Namespaced,
    /// The words a deposit's place ends in, one chosen a deposit: `flats`,
    /// for "Calder flats".
    pub grounds: Vec<String>,
    /// A floor and a cap for each attribute its class carries, and no
    /// other.
    pub ranges: BTreeMap<Attribute, Span>,
    /// Where it occurs.
    pub occurs: OccursDef,
}

/// A dense handle to a type: its place in id order, so the same data set
/// gives the same handles however the directory lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceTypeId(pub u16);

/// The lattice the candidates stand on.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatticeDef {
    /// A cell's side, metres.
    pub cell_m: f64,
    /// The share of cells that hold a candidate, `0..=1`.
    pub keep: f64,
    /// How much of its cell a candidate may stand off the centre by,
    /// `0..=1`.
    pub jitter: f64,
    /// Points of a cell tried before it is left empty.
    pub tries: u32,
}

/// Where a candidate may not stand.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefuseDef {
    /// The steepest ground, rise over run.
    pub slope_max: f64,
    /// How far over the sea's level the ground must be, metres.
    pub shore_m: f64,
    /// The most of a road's cleared way, `0..=1`.
    pub cleared_max: f64,
    /// The most of a pad's or a settlement's ground, `0..=1`.
    pub developed_max: f64,
}

/// How a deposit's figures are rolled.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeDef {
    /// The share of a figure that is the deposit's one shared roll, `0..=1`.
    pub shared: f64,
    /// The power an attribute's own roll is raised to: higher makes a high
    /// line rarer.
    pub spike: u32,
}

/// The area a deposit covers, as the least and the most a cell's roll may
/// give.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaDef {
    /// The least and the most a radius may be, metres.
    pub radius_m: (f64, f64),
    /// The least and the most the concentration at the centre may be,
    /// per-mille.
    pub peak: (u16, u16),
}

/// How a lot stacks and is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotsDef {
    /// Units a stack holds.
    pub stack: u32,
}

/// How near a deposit the HUD names the place.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceDef {
    /// From how near a deposit's centre the place is named, metres.
    pub place_m: f64,
}

/// How much of a deposit's quality a ping tells.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Reading {
    /// Its type and the concentration only.
    #[default]
    Type,
    /// Each line as a band word.
    Bands,
    /// The figures.
    Exact,
}

/// A ping's dials.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PingDef {
    /// Seconds the player stands still.
    pub channel_s: f64,
    /// The reach to each side of the player, metres, at store grade.
    pub range_m: f64,
    /// Points a side of the grid, odd.
    pub grid: u32,
    /// A reading under this, per-mille, is nothing.
    pub min_pm: u16,
    /// Water this deep over the ground, metres, is swimming: neither a
    /// ping nor a sample is made from it.
    pub swim_m: f32,
    /// The HUD's lead marker goes when the household stands this close to
    /// it, metres.
    pub lead_clear_m: f64,
}

/// A sampling hand's dials.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleDef {
    /// Seconds a cycle.
    pub cycle_s: f64,
    /// Units a cycle at 1000 per-mille.
    pub units: u32,
    /// Moving this far from where sampling began stops it, metres.
    pub stops_at_m: f64,
}

/// What a ping tells before any skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadingDef {
    /// How well a deposit is read with no skill.
    pub untrained: Reading,
}

/// `resources/survey.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurveyRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The ping.
    pub ping: PingDef,
    /// The sample.
    pub sample: SampleDef,
    /// What a ping tells.
    pub reading: ReadingDef,
    /// The upper bounds of poor, fair and good, per-mille of a type's range
    /// for a line; above the last is fine.
    pub bands: (u16, u16, u16),
}

impl SurveyRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// What is wrong with the dials, if anything.
    ///
    /// # Errors
    /// The first thing out of range.
    pub fn check(&self) -> Result<(), ResourceError> {
        let refused = ResourceError::Rules;
        if self.version != Self::VERSION {
            return Err(ResourceError::Version {
                found: self.version,
                expected: Self::VERSION,
            });
        }
        let (ping, sample) = (&self.ping, &self.sample);
        let over_nought = |value: f64| value.is_finite() && value > 0.0;
        if !(ping.channel_s.is_finite() && ping.channel_s >= 0.0 && over_nought(ping.range_m)) {
            return Err(refused(
                "ping.channel_s is not negative and ping.range_m is over nought".to_owned(),
            ));
        }
        if ping.grid < GRID_MIN || ping.grid % 2 == 0 || ping.grid > GRID_MAX {
            return Err(refused(format!(
                "ping.grid is {}; odd, {GRID_MIN} to {GRID_MAX}, so one point is the player's own",
                ping.grid
            )));
        }
        if ping.min_pm == 0 || ping.min_pm > 1000 {
            return Err(refused("ping.min_pm is 1 to 1000".to_owned()));
        }
        if !over_nought(ping.lead_clear_m) {
            return Err(refused("ping.lead_clear_m is over nought".to_owned()));
        }
        if !over_nought(f64::from(ping.swim_m)) {
            return Err(refused("ping.swim_m is over nought".to_owned()));
        }
        if !(over_nought(sample.cycle_s) && over_nought(sample.stops_at_m)) || sample.units == 0 {
            return Err(refused(
                "sample.cycle_s, sample.units and sample.stops_at_m are over nought".to_owned(),
            ));
        }
        let (a, b, c) = self.bands;
        if !(0 < a && a < b && b < c && c < 1000) {
            return Err(refused(
                "bands rise, each over nought and under 1000".to_owned(),
            ));
        }
        Ok(())
    }

    /// The word-band of a share of a range, per-mille: 0 poor, 1 fair, 2
    /// good, 3 fine.
    #[must_use]
    pub fn band(&self, share_pm: u16) -> u8 {
        let (a, b, c) = self.bands;
        u8::from(share_pm >= a) + u8::from(share_pm >= b) + u8::from(share_pm >= c)
    }
}

/// `resources/harvest.ron`: every dial of the field harvester (ticket 0009,
/// step 6).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarvestRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// Units a played hour at a concentration of 1000 per-mille, in
    /// proportion at less.
    pub rate: u32,
    /// Units a harvester holds before it stops.
    pub hopper: u32,
    /// The least concentration, per-mille, it can be planted on.
    pub min_pm: u16,
    /// No two stand nearer than this, metres.
    pub apart_m: f64,
    /// How near a character must stand to collect or pick up, metres.
    pub reach_m: f64,
}

impl HarvestRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// What is wrong with the dials, if anything.
    ///
    /// # Errors
    /// The first thing out of range.
    pub fn check(&self) -> Result<(), ResourceError> {
        if self.version != Self::VERSION {
            return Err(ResourceError::Version {
                found: self.version,
                expected: Self::VERSION,
            });
        }
        let over_nought = |value: f64| value.is_finite() && value > 0.0;
        if self.rate == 0 || self.hopper == 0 {
            return Err(ResourceError::Rules(
                "harvest rate and hopper are over nought".to_owned(),
            ));
        }
        if self.min_pm == 0 || self.min_pm > 1000 {
            return Err(ResourceError::Rules(
                "harvest min_pm is 1 to 1000".to_owned(),
            ));
        }
        if !(over_nought(self.apart_m) && over_nought(self.reach_m)) {
            return Err(ResourceError::Rules(
                "harvest apart_m and reach_m are over nought".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The survey register's reach.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterDef {
    /// How far round the player it lists, metres.
    pub range_m: f64,
    /// The most rows it lists.
    pub rows: u32,
}

/// `resources/deposits.ron`: every dial of where deposits lie and how a lot
/// is drawn.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The lattice.
    pub lattice: LatticeDef,
    /// Where a candidate may not stand.
    pub refuse: RefuseDef,
    /// How figures are rolled.
    pub grade: GradeDef,
    /// The area a deposit covers and how strong it is at its centre.
    pub area: AreaDef,
    /// Lots.
    pub lots: LotsDef,
    /// Naming a deposit's place.
    pub place: PlaceDef,
    /// The register.
    pub register: RegisterDef,
    /// The names deposits take, a square number of them.
    pub roots: Vec<String>,
}

impl DepositRules {
    /// The file's current shape.
    pub const VERSION: u32 = 2;

    /// The side of the square the roots are dealt over: a root repeats
    /// every this many cells and no nearer. Nought with no roots.
    #[must_use]
    pub fn root_block(&self) -> usize {
        let mut side = 0;
        while (side + 1) * (side + 1) <= self.roots.len() {
            side += 1;
        }
        side
    }
}

/// Where a type occurs, resolved: dense by province and by biome id, nought
/// for one its file does not name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Occurs {
    /// The weight on any ground.
    pub anywhere: f64,
    provinces: Vec<f64>,
    biomes: Vec<f64>,
    /// The water record's.
    pub water: WaterWeights,
}

impl Occurs {
    /// What `province` adds, for the whole of a point.
    #[must_use]
    pub fn province(&self, province: ProvinceId) -> f64 {
        self.provinces
            .get(province.0 as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// What `biome` adds, for the whole of a point.
    #[must_use]
    pub fn biome(&self, biome: BiomeId) -> f64 {
        self.biomes.get(biome.0 as usize).copied().unwrap_or(0.0)
    }
}

/// Why a resource file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// A file of another version than this build reads.
    #[error("version {found}, this build reads {expected}")]
    Version {
        /// The version the file states.
        found: u32,
        /// The version this build reads.
        expected: u32,
    },
    /// Two entries share an id.
    #[error("`{0}` is defined twice")]
    Duplicate(Namespaced),
    /// The directory holds types and lacks one of its two other files.
    #[error("the resources need `{0}`")]
    Missing(&'static str),
    /// The class tree breaks a rule.
    #[error("{0}")]
    Tree(String),
    /// A type breaks a rule.
    #[error("`{id}`: {reason}")]
    Type {
        /// The type.
        id: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
    /// The deposits' dials break a rule.
    #[error("{0}")]
    Rules(String),
}

/// Every resource, checked and resolved, with the deposits' dials.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResourceSet {
    /// By [`ResourceTypeId`], which is the types' order by id.
    types: Vec<TypeDef>,
    /// Each type's class, an index into the tree's.
    classes: Vec<usize>,
    /// Each class's family, an index into the tree's.
    families: Vec<usize>,
    occurs: Vec<Occurs>,
    tree: ClassTree,
    rules: DepositRules,
    survey: SurveyRules,
    harvest: HarvestRules,
}

fn finite_weight(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

impl ResourceSet {
    /// Check the tree, the types and the dials against each other and
    /// against `ids` (the provinces, biomes and site kinds a file names),
    /// and resolve the names. Each type comes with the file it was read
    /// from, and an error names the file at fault.
    ///
    /// # Errors
    /// The file and what is wrong in it.
    pub fn build(
        mut types: Vec<(String, TypeDef)>,
        tree: ClassTree,
        rules: DepositRules,
        ids: &GameRegistries,
    ) -> Result<Self, (String, ResourceError)> {
        check_tree(&tree).map_err(|error| (CLASSES_FILE.to_owned(), error))?;
        check_rules(&rules).map_err(|error| (DEPOSITS_FILE.to_owned(), error))?;
        // Sorted so the same data set gives the same handles however the
        // directory lists.
        types.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        if types.len() > usize::from(u16::MAX) {
            return Err((
                RESOURCES_DIR.to_owned(),
                ResourceError::Rules(format!("{} types, more than a handle counts", types.len())),
            ));
        }
        let mut classes = Vec::with_capacity(types.len());
        let mut occurs = Vec::with_capacity(types.len());
        for (index, (path, def)) in types.iter().enumerate() {
            let refused = |error| (path.clone(), error);
            if index > 0 && types[index - 1].1.id == def.id {
                return Err(refused(ResourceError::Duplicate(def.id.clone())));
            }
            let (class, resolved) = check_type(def, &tree, ids).map_err(|reason| {
                refused(ResourceError::Type {
                    id: def.id.clone(),
                    reason,
                })
            })?;
            classes.push(class);
            occurs.push(resolved);
        }
        let families = tree
            .classes
            .iter()
            .map(|class| {
                tree.families
                    .iter()
                    .position(|family| family.id == class.family)
                    .unwrap_or(0)
            })
            .collect();
        Ok(Self {
            types: types.into_iter().map(|(_, def)| def).collect(),
            classes,
            families,
            occurs,
            tree,
            rules,
            survey: SurveyRules::default(),
            harvest: HarvestRules::default(),
        })
    }

    /// The set with the survey dials `survey`, checked.
    ///
    /// # Errors
    /// What is wrong with them.
    pub fn with_survey(mut self, survey: SurveyRules) -> Result<Self, ResourceError> {
        survey.check()?;
        self.survey = survey;
        Ok(self)
    }

    /// The survey dials. Zeroes for a set built with none, which no ping
    /// uses: a data set with deposits has the file.
    #[must_use]
    pub fn survey(&self) -> &SurveyRules {
        &self.survey
    }

    /// The set with the harvester's dials `harvest`, checked.
    ///
    /// # Errors
    /// What is wrong with them.
    pub fn with_harvest(mut self, harvest: HarvestRules) -> Result<Self, ResourceError> {
        harvest.check()?;
        self.harvest = harvest;
        Ok(self)
    }

    /// The harvester's dials. Zeroes for a set built with none.
    #[must_use]
    pub fn harvest(&self) -> &HarvestRules {
        &self.harvest
    }

    /// Every type, in id order: `types()[n]` is [`ResourceTypeId`]`(n)`.
    #[must_use]
    pub fn types(&self) -> &[TypeDef] {
        &self.types
    }

    /// Whether the data set has nothing in the ground.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// The handle of the type named `id`.
    #[must_use]
    pub fn type_id(&self, id: &Namespaced) -> Option<ResourceTypeId> {
        let index = self.types.iter().position(|def| def.id == *id)?;
        u16::try_from(index).ok().map(ResourceTypeId)
    }

    /// A type by its handle.
    #[must_use]
    pub fn get(&self, ty: ResourceTypeId) -> Option<&TypeDef> {
        self.types.get(usize::from(ty.0))
    }

    /// Every handle, in order.
    pub fn type_ids(&self) -> impl ExactSizeIterator<Item = ResourceTypeId> + '_ {
        // The count was held to a `u16` when the set was built.
        (0..self.types.len()).map(|index| ResourceTypeId(u16::try_from(index).unwrap_or(u16::MAX)))
    }

    /// The class of a type.
    ///
    /// # Panics
    /// For a handle from another set.
    #[must_use]
    pub fn class_of(&self, ty: ResourceTypeId) -> &ClassDef {
        &self.tree.classes[self.classes[usize::from(ty.0)]]
    }

    /// The place of a type's class in [`Self::classes`]: a stable small
    /// number to pick a mark by.
    ///
    /// # Panics
    /// For a handle from another set.
    #[must_use]
    pub fn class_index(&self, ty: ResourceTypeId) -> usize {
        self.classes[usize::from(ty.0)]
    }

    /// The family of a type.
    ///
    /// # Panics
    /// For a handle from another set.
    #[must_use]
    pub fn family_of(&self, ty: ResourceTypeId) -> &FamilyDef {
        &self.tree.families[self.families[self.classes[usize::from(ty.0)]]]
    }

    /// Where a type occurs, resolved.
    ///
    /// # Panics
    /// For a handle from another set.
    #[must_use]
    pub fn occurs(&self, ty: ResourceTypeId) -> &Occurs {
        &self.occurs[usize::from(ty.0)]
    }

    /// The classes, in the file's order.
    #[must_use]
    pub fn classes(&self) -> &[ClassDef] {
        &self.tree.classes
    }

    /// The families, in the file's order.
    #[must_use]
    pub fn families(&self) -> &[FamilyDef] {
        &self.tree.families
    }

    /// The deposits' dials.
    #[must_use]
    pub fn rules(&self) -> &DepositRules {
        &self.rules
    }
}

fn check_tree(tree: &ClassTree) -> Result<(), ResourceError> {
    if tree.version != ClassTree::VERSION {
        return Err(ResourceError::Version {
            found: tree.version,
            expected: ClassTree::VERSION,
        });
    }
    let mut seen = BTreeSet::new();
    for family in &tree.families {
        if !seen.insert(&family.id) {
            return Err(ResourceError::Duplicate(family.id.clone()));
        }
    }
    let mut seen = BTreeSet::new();
    for class in &tree.classes {
        if !seen.insert(&class.id) {
            return Err(ResourceError::Duplicate(class.id.clone()));
        }
        if !tree.families.iter().any(|family| family.id == class.family) {
            return Err(ResourceError::Tree(format!(
                "class `{}` is of family `{}`, which is no family",
                class.id, class.family
            )));
        }
        if class.carries.is_empty() {
            return Err(ResourceError::Tree(format!(
                "class `{}` carries nothing",
                class.id
            )));
        }
        let mut carried = BTreeSet::new();
        for attribute in &class.carries {
            if !carried.insert(attribute) {
                return Err(ResourceError::Tree(format!(
                    "class `{}` carries {attribute:?} twice",
                    class.id
                )));
            }
        }
    }
    Ok(())
}

/// What is wrong with a type, if anything; else its class's index and
/// where it occurs, resolved.
fn check_type(
    def: &TypeDef,
    tree: &ClassTree,
    ids: &GameRegistries,
) -> Result<(usize, Occurs), String> {
    let class = tree
        .classes
        .iter()
        .position(|class| class.id == def.class)
        .ok_or_else(|| format!("its class `{}` is no class", def.class))?;
    let carries = &tree.classes[class].carries;
    if let Some(missing) = carries.iter().find(|a| !def.ranges.contains_key(a)) {
        return Err(format!(
            "`ranges` leaves out {missing:?}, which its class carries"
        ));
    }
    if let Some(foreign) = def.ranges.keys().find(|a| !carries.contains(a)) {
        return Err(format!(
            "`ranges` has {foreign:?}, which its class does not carry"
        ));
    }
    for (attribute, span) in &def.ranges {
        if span.floor >= span.cap || span.cap > QUALITY_MAX {
            return Err(format!(
                "{attribute:?} runs {} to {}; a floor is under its cap and a cap at most {QUALITY_MAX}",
                span.floor, span.cap
            ));
        }
    }
    if def.name.trim().is_empty() || def.short.trim().is_empty() {
        return Err("it needs a name and a short name".to_owned());
    }
    if def.grounds.is_empty() || def.grounds.iter().any(|word| word.trim().is_empty()) {
        return Err("`grounds` needs a word a deposit's place can end in".to_owned());
    }
    let occurs = &def.occurs;
    let mut weights = vec![occurs.anywhere, occurs.water.lake, occurs.water.river];
    let mut provinces = vec![0.0; ids.provinces.len()];
    for entry in &occurs.provinces {
        let province = ids
            .province_id(&entry.id)
            .ok_or_else(|| format!("it occurs in province `{}`, which is none", entry.id))?;
        provinces[province.0 as usize] = entry.weight;
        weights.push(entry.weight);
    }
    let mut biomes = vec![0.0; ids.biomes.len()];
    for entry in &occurs.biomes {
        let biome = ids
            .biome_id(&entry.id)
            .ok_or_else(|| format!("it occurs in biome `{}`, which is none", entry.id))?;
        biomes[biome.0 as usize] = entry.weight;
        weights.push(entry.weight);
    }
    if !weights.iter().copied().all(finite_weight) {
        return Err("a weight in `occurs` is negative or no number".to_owned());
    }
    if !weights.iter().any(|weight| *weight > 0.0) {
        return Err("`occurs` gives it no weight anywhere".to_owned());
    }
    Ok((
        class,
        Occurs {
            anywhere: occurs.anywhere,
            provinces,
            biomes,
            water: occurs.water,
        },
    ))
}

/// What is wrong with the dials, if anything.
fn check_rules(rules: &DepositRules) -> Result<(), ResourceError> {
    let refused = ResourceError::Rules;
    if rules.version != DepositRules::VERSION {
        return Err(ResourceError::Version {
            found: rules.version,
            expected: DepositRules::VERSION,
        });
    }
    check_dials(rules)?;
    let side = rules.root_block();
    if side == 0 || side * side != rules.roots.len() {
        return Err(refused(format!(
            "{} roots; a square number of them is dealt, so that a name repeats no nearer than its block",
            rules.roots.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for root in &rules.roots {
        if root.trim().is_empty() || !seen.insert(root) {
            return Err(refused(format!("root `{root}` is empty or there twice")));
        }
    }
    Ok(())
}

/// What is wrong with the lattice, the refusals, the grade, the lots, the
/// place or the register, if anything.
fn check_dials(rules: &DepositRules) -> Result<(), ResourceError> {
    let refused = ResourceError::Rules;
    let unit = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    let lattice = &rules.lattice;
    // A cell under a few metres would put a deposit beside every step.
    if !(lattice.cell_m.is_finite() && lattice.cell_m >= 50.0) {
        return Err(refused(format!(
            "lattice.cell_m is {}, a cell is 50 m or more",
            lattice.cell_m
        )));
    }
    if !(unit(lattice.keep) && lattice.keep > 0.0 && unit(lattice.jitter)) {
        return Err(refused(
            "lattice.keep is over nought and at most one, lattice.jitter nought to one".to_owned(),
        ));
    }
    if !(1..=16).contains(&lattice.tries) {
        return Err(refused(format!(
            "lattice.tries is {}, one to sixteen",
            lattice.tries
        )));
    }
    let refuse = &rules.refuse;
    if !(finite_weight(refuse.slope_max)
        && finite_weight(refuse.shore_m)
        && unit(refuse.cleared_max)
        && unit(refuse.developed_max))
    {
        return Err(refused("a number in `refuse` is out of range".to_owned()));
    }
    if !(unit(rules.grade.shared) && (1..=8).contains(&rules.grade.spike)) {
        return Err(refused(
            "grade.shared is nought to one, grade.spike one to eight".to_owned(),
        ));
    }
    let lots = &rules.lots;
    if lots.stack == 0 {
        return Err(refused("lots.stack is nought".to_owned()));
    }
    let (area, (lo_r, hi_r), (lo_p, hi_p)) = (&rules.area, rules.area.radius_m, rules.area.peak);
    if !(lo_r.is_finite() && hi_r.is_finite() && lo_r > 0.0 && lo_r <= hi_r) {
        return Err(refused(format!(
            "area.radius_m is {lo_r} to {hi_r}; over nought and ordered",
        )));
    }
    if !(lo_p <= hi_p && hi_p <= 1000) || area.peak.0 == 0 {
        return Err(refused(format!(
            "area.peak is {lo_p} to {hi_p}; over nought, ordered and at most 1000",
        )));
    }
    if !(rules.place.place_m.is_finite() && rules.place.place_m > 0.0) {
        return Err(refused("place.place_m is over nought".to_owned()));
    }
    if !(rules.register.range_m.is_finite() && rules.register.range_m > 0.0)
        || rules.register.rows == 0
    {
        return Err(refused(
            "register.range_m and register.rows are over nought".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::load::load_from_dir;

    fn shipped() -> GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).expect("assets/ loads")
    }

    fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).expect("a valid id")
    }

    /// The step's first content: four classes over two families and seven
    /// types, two each in the reactive, brine and structural metal classes,
    /// with the worked example's three among them; every weight resolved to
    /// an id, and the dials the design's.
    #[test]
    fn the_shipped_resources_load() {
        let registries = shipped();
        let set = &registries.resources;
        let names: Vec<String> = set.types().iter().map(|def| def.id.to_string()).collect();
        assert_eq!(
            names,
            [
                "strata:basin_brine",
                "strata:coastal_brine",
                "strata:evaporite_salt",
                "strata:ferrous_ore",
                "strata:light_alloy_ore",
                "strata:red_conductor",
                "strata:sulphur",
            ]
        );
        assert_eq!(set.classes().len(), 4);
        assert_eq!(set.families().len(), 2);
        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        for ty in set.type_ids() {
            let def = set.get(ty).expect("its own handle");
            let class = set.class_of(ty);
            assert_eq!(class.id, def.class);
            assert_eq!(set.type_id(&def.id), Some(ty));
            assert_eq!(set.family_of(ty).id, class.family);
            let ranged: Vec<Attribute> = def.ranges.keys().copied().collect();
            let mut carried = class.carries.clone();
            carried.sort();
            assert_eq!(ranged, carried, "{}", def.id);
            *by_class.entry(class.name.clone()).or_default() += 1;
        }
        assert_eq!(
            by_class,
            BTreeMap::from([
                ("Brine".to_owned(), 2),
                ("Conductor".to_owned(), 1),
                ("Reactive".to_owned(), 2),
                ("Structural metal".to_owned(), 2),
            ])
        );
        let salt = set.type_id(&id("strata:evaporite_salt")).expect("ships");
        assert_eq!(set.class_of(salt).name, "Reactive");
        assert_eq!(set.family_of(salt).name, "Mineral");
        assert_eq!(
            set.get(salt).expect("ships").ranges[&Attribute::Potency],
            Span {
                floor: 300,
                cap: 950
            }
        );
        // The weights are by id: the coast's on the coast and nowhere else.
        let coast = registries.biome_id(&id("strata:coast")).expect("ships");
        let plains = registries.biome_id(&id("strata:plains")).expect("ships");
        let brine = set.type_id(&id("strata:coastal_brine")).expect("ships");
        assert!((set.occurs(brine).biome(coast) - 1.6).abs() < 1e-12);
        assert!(set.occurs(brine).biome(plains).abs() < 1e-12);
        let granite = registries
            .province_id(&id("strata:granite"))
            .expect("ships");
        let alloy = set.type_id(&id("strata:light_alloy_ore")).expect("ships");
        assert!((set.occurs(alloy).province(granite) - 0.8).abs() < 1e-12);
        assert!(set.occurs(alloy).province(ProvinceId::NONE).abs() < 1e-12);

        let rules = set.rules();
        assert_eq!(rules.roots.len(), 256);
        assert_eq!(rules.root_block(), 16);
        assert!((rules.lattice.cell_m - 400.0).abs() < 1e-9);
        assert!((rules.area.radius_m.0 - 120.0).abs() < 1e-9);
        assert!((rules.area.radius_m.1 - 260.0).abs() < 1e-9);
        assert_eq!(rules.area.peak, (350, 1000));

        // The vocabulary: eleven attributes, each with a key, a name and a
        // two-letter code of its own.
        let codes: BTreeSet<&str> = Attribute::ALL.iter().map(|a| a.code()).collect();
        let keys: BTreeSet<&str> = Attribute::ALL.iter().map(|a| a.key()).collect();
        assert_eq!((codes.len(), keys.len()), (11, 11));
        for attribute in Attribute::ALL {
            assert_eq!(Attribute::from_key(attribute.key()), Some(attribute));
            assert_eq!(attribute.code().len(), 2);
        }
        assert_eq!(Attribute::HeatTolerance.name(), "Heat tolerance");
        assert_eq!(Attribute::HeatTolerance.code(), "HT");
    }

    fn tree() -> ClassTree {
        shipped().resources.tree
    }

    fn salt() -> TypeDef {
        let registries = shipped();
        let set = &registries.resources;
        let salt = set.type_id(&id("strata:evaporite_salt")).expect("ships");
        set.get(salt).expect("ships").clone()
    }

    fn refusal(types: Vec<TypeDef>, tree: ClassTree, rules: DepositRules) -> String {
        let registries = shipped();
        let types = types
            .into_iter()
            .map(|def| (format!("resources/{}.resource.ron", def.id.path()), def))
            .collect();
        let (path, error) =
            ResourceSet::build(types, tree, rules, &registries).expect_err("the set is refused");
        format!("{path}: {error}")
    }

    /// A type ranges exactly what its class carries: one short and one with
    /// an attribute of another class are refused by file, as is a span the
    /// wrong way round or past the scale.
    #[test]
    fn a_type_must_range_what_its_class_carries() {
        let rules = shipped().resources.rules.clone();
        let registries = shipped();
        let whole = ResourceSet::build(
            vec![("salt".to_owned(), salt())],
            tree(),
            rules.clone(),
            &registries,
        );
        assert_eq!(whole.expect("salt alone stands").types().len(), 1);

        let mut short = salt();
        short.ranges.remove(&Attribute::Stability);
        let why = refusal(vec![short], tree(), rules.clone());
        assert!(
            why.starts_with("resources/evaporite_salt.resource.ron")
                && why.contains("leaves out Stability"),
            "{why}"
        );
        let mut foreign = salt();
        foreign.ranges.insert(
            Attribute::Strength,
            Span {
                floor: 100,
                cap: 200,
            },
        );
        let why = refusal(vec![foreign], tree(), rules.clone());
        assert!(
            why.contains("Strength, which its class does not carry"),
            "{why}"
        );
        for span in [
            Span {
                floor: 500,
                cap: 500,
            },
            Span {
                floor: 300,
                cap: 1001,
            },
        ] {
            let mut bent = salt();
            bent.ranges.insert(Attribute::Potency, span);
            let why = refusal(vec![bent], tree(), rules.clone());
            assert!(why.contains("Potency runs"), "{why}");
        }
        let mut classless = salt();
        classless.class = id("strata:plasma");
        assert!(refusal(vec![classless], tree(), rules.clone()).contains("is no class"));
        let mut nowhere = salt();
        nowhere.occurs = OccursDef::default();
        assert!(refusal(vec![nowhere], tree(), rules.clone()).contains("no weight"));
        let mut lost = salt();
        lost.occurs.provinces.push(WeightOf {
            id: id("strata:obsidian"),
            weight: 1.0,
        });
        assert!(refusal(vec![lost], tree(), rules.clone()).contains("`strata:obsidian`"));
        assert!(refusal(vec![salt(), salt()], tree(), rules.clone()).contains("twice"));

        // The tree and the dials name their own files.
        let mut twice = tree();
        twice.classes[0].carries.push(Attribute::Purity);
        let why = refusal(vec![salt()], twice, rules.clone());
        assert!(
            why.starts_with(CLASSES_FILE) && why.contains("Purity twice"),
            "{why}"
        );
        let mut backwards = rules.clone();
        backwards.area.radius_m = (260.0, 120.0);
        let why = refusal(vec![salt()], tree(), backwards);
        assert!(
            why.starts_with(DEPOSITS_FILE) && why.contains("area.radius_m"),
            "{why}"
        );
        let mut strong = rules.clone();
        strong.area.peak = (350, 1001);
        assert!(refusal(vec![salt()], tree(), strong).contains("area.peak"));
        let mut odd = rules.clone();
        odd.roots.pop();
        assert!(refusal(vec![salt()], tree(), odd).contains("square number"));
    }

    #[test]
    fn a_data_set_without_resources_is_legal() {
        let set = ResourceSet::default();
        assert!(set.is_empty());
        assert_eq!(set.rules().root_block(), 0);
        assert_eq!(GameRegistries::default().resources, set);
    }

    #[test]
    fn a_span_says_where_a_figure_stands() {
        let span = Span {
            floor: 300,
            cap: 950,
        };
        assert!(span.contains(300) && span.contains(950) && !span.contains(299));
        assert!((span.share(300)).abs() < 1e-12);
        assert!((span.share(950) - 1.0).abs() < 1e-12);
        assert!((span.share(625) - 0.5).abs() < 1e-12);
        assert!(span.share(100).abs() < 1e-12, "clamped under the floor");
        assert!((span.share(1000) - 1.0).abs() < 1e-12, "and over the cap");
    }

    /// The shipped survey dials load, and each kind of nonsense in them is
    /// refused by name.
    #[test]
    fn the_survey_dials_are_checked() {
        let survey = *shipped().resources.survey();
        assert_eq!(survey.version, SurveyRules::VERSION);
        assert!((survey.ping.range_m - 128.0).abs() < 1e-9);
        assert_eq!((survey.ping.grid, survey.ping.min_pm), (5, 20));
        assert_eq!(survey.sample.units, 8);
        assert_eq!(survey.reading.untrained, Reading::Type);
        assert_eq!(survey.check(), Ok(()));
        assert_eq!(
            [0, 249, 250, 499, 500, 749, 750, 1000].map(|pm| survey.band(pm)),
            [0, 0, 1, 1, 2, 2, 3, 3]
        );
        let mutate: [(&str, fn(&mut SurveyRules)); 5] = [
            ("grid", |s| s.ping.grid = 4),
            ("min_pm", |s| s.ping.min_pm = 0),
            ("cycle", |s| s.sample.cycle_s = 0.0),
            ("bands", |s| s.bands = (500, 400, 900)),
            ("range", |s| s.ping.range_m = f64::NAN),
        ];
        for (what, change) in mutate {
            let mut bent = survey;
            change(&mut bent);
            assert!(bent.check().is_err(), "{what}");
        }
        let mut old = survey;
        old.version += 1;
        assert!(matches!(old.check(), Err(ResourceError::Version { .. })));
        assert!(Reading::Type < Reading::Bands && Reading::Bands < Reading::Exact);
    }

    /// The shipped harvester dials load, and nonsense in them is refused.
    #[test]
    fn the_harvest_dials_are_checked() {
        let harvest = *shipped().resources.harvest();
        assert_eq!(harvest.version, HarvestRules::VERSION);
        assert_eq!(
            (harvest.rate, harvest.hopper, harvest.min_pm),
            (1200, 1000, 150)
        );
        assert!((harvest.apart_m - 30.0).abs() < 1e-9 && (harvest.reach_m - 3.0).abs() < 1e-9);
        assert_eq!(harvest.check(), Ok(()));
        let mutate: [(&str, fn(&mut HarvestRules)); 5] = [
            ("rate", |h| h.rate = 0),
            ("hopper", |h| h.hopper = 0),
            ("min_pm", |h| h.min_pm = 1001),
            ("apart", |h| h.apart_m = 0.0),
            ("reach", |h| h.reach_m = f64::NAN),
        ];
        for (what, change) in mutate {
            let mut bent = harvest;
            change(&mut bent);
            assert!(bent.check().is_err(), "{what}");
        }
        let mut old = harvest;
        old.version += 1;
        assert!(matches!(old.check(), Err(ResourceError::Version { .. })));
    }
}
