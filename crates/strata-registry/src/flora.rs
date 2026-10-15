//! Flora species: what grows where, as `.flora.ron` files spell it.
//!
//! A species names the biomes it grows in with a weight, so a species (or a
//! mod's) is added without editing a biome. The rest is what placement
//! needs to decide whether a slot of a cell holds one: how dense, how
//! steep, how high, how far from the sea, how wet; and what the client
//! needs to draw one: a size range, a tint pair, and for a tree the
//! generator's parameters. Nothing here is a mesh or a texture.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use slotted_model::Namespaced;

use crate::biome::{Tint, UnitRange};

/// A dense handle to a frozen species. Assigned by the freeze in sorted-id
/// order, so the same asset set gives the same ids however the directory
/// lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FloraId(pub u32);

slotted_registry::impl_interned_id!(FloraId);

/// What kind of plant a species is, which decides how it is placed and
/// drawn: grass is a blade record in a per-cell buffer, a bush or a tree
/// is a placed instance with a collider when it is a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloraKind {
    /// Blades, placed by density, drawn from a per-cell buffer.
    Grass,
    /// A placed instance without a collider.
    Bush,
    /// A placed instance with a trunk collider on the server.
    Tree,
    /// A placed stone, boulder or cliff piece: placed before trees, by the
    /// ground's shape as much as its biome, with the baked hulls as its
    /// collider on the server.
    Rock,
}

impl FloraKind {
    /// Every kind, for tests and tools.
    pub const ALL: [Self; 4] = [Self::Grass, Self::Bush, Self::Tree, Self::Rock];

    /// The lowercase name used in RON. A string rather than a bare variant
    /// for the same reason as `ReliefKind`: the loader goes through RON's
    /// `Value`, which has no enum.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Bush => "bush",
            Self::Tree => "tree",
            Self::Rock => "rock",
        }
    }
}

impl fmt::Display for FloraKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A flora kind name nobody defined.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown flora kind `{0}`; expected grass, bush, tree or rock")]
pub struct UnknownFloraKind(pub String);

impl FromStr for FloraKind {
    type Err = UnknownFloraKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| UnknownFloraKind(s.to_owned()))
    }
}

impl Serialize for FloraKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FloraKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// How much a species likes one biome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeWeight {
    /// The biome, `strata:forest`.
    pub biome: Namespaced,
    /// `0..=1`; the chance a slot in pure ground of that biome holds the
    /// species, before the other limits.
    pub weight: f32,
}

/// A closed interval in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetreRange {
    /// Lower bound, inclusive.
    pub min: f32,
    /// Upper bound, inclusive.
    pub max: f32,
}

impl MetreRange {
    /// A range from `min` to `max`.
    #[must_use]
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// Whether the range is ordered and finite.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min <= self.max
    }

    /// Whether `v` is inside.
    #[must_use]
    pub fn contains(self, v: f32) -> bool {
        v >= self.min && v <= self.max
    }
}

/// The tree generator's parameters, Weber and Penn reduced to what a file
/// names. Grass leaves it out, which is the default with no height: RON's
/// `Value`, which the loader goes through, has no `Option`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeParams {
    /// Procedural silhouette: tree, shrub, fern, flower, reed or log. Logs use
    /// `height_m` as their length; all living forms use plant height.
    #[serde(default = "default_form")]
    pub form: String,
    /// Height of the whole plant at scale one, metres. Zero to zero means
    /// "no tree", the default.
    #[serde(default = "default_no_height")]
    pub height_m: MetreRange,
    /// Branch levels below the trunk, 1 to 5.
    #[serde(default = "default_levels")]
    pub levels: u8,
    /// Children per branch, 1 to 4.
    #[serde(default = "default_branch_factor")]
    pub branch_factor: u8,
    /// Child length over parent length, `(0, 1)`.
    #[serde(default = "default_taper")]
    pub taper: f32,
    /// Angle between a parent and its children, degrees.
    #[serde(default = "default_split_angle")]
    pub split_angle_deg: f32,
    /// Trunk radius over plant height.
    #[serde(default = "default_trunk_ratio")]
    pub trunk_ratio: f32,
    /// Leaf card size over plant height.
    #[serde(default = "default_card_ratio")]
    pub card_ratio: f32,
    /// Distinct meshes generated for the species.
    #[serde(default = "default_variants")]
    pub variants: u8,
    /// The bark set: the content step bakes it as the material
    /// `flora:bark-<bark>`, which a baked tree binds and the understory's
    /// deadwood wears. Empty for a flat colour.
    #[serde(default)]
    pub bark: String,
    /// The leaf card look: `broad` (a cluster of leaves) or `needle` (a
    /// spray of needles).
    #[serde(default = "default_leaves")]
    pub leaves: String,
}

fn default_leaves() -> String {
    "broad".to_owned()
}

fn default_form() -> String {
    "tree".to_owned()
}

const fn default_no_height() -> MetreRange {
    MetreRange::new(0.0, 0.0)
}
const fn default_levels() -> u8 {
    3
}
const fn default_branch_factor() -> u8 {
    3
}
const fn default_taper() -> f32 {
    0.7
}
const fn default_split_angle() -> f32 {
    35.0
}
const fn default_trunk_ratio() -> f32 {
    0.03
}
const fn default_card_ratio() -> f32 {
    0.12
}
const fn default_variants() -> u8 {
    4
}

impl Default for TreeParams {
    fn default() -> Self {
        Self {
            form: default_form(),
            height_m: default_no_height(),
            levels: default_levels(),
            branch_factor: default_branch_factor(),
            taper: default_taper(),
            split_angle_deg: default_split_angle(),
            trunk_ratio: default_trunk_ratio(),
            card_ratio: default_card_ratio(),
            variants: default_variants(),
            bark: String::new(),
            leaves: default_leaves(),
        }
    }
}

impl TreeParams {
    /// Whether these are the "no tree" parameters.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.height_m.max <= 0.0
    }

    /// Whether the numbers describe a tree the generator can build.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.height_m.is_valid()
            && matches!(
                self.form.as_str(),
                "tree" | "shrub" | "fern" | "flower" | "log" | "reed"
            )
            && self.height_m.min > 0.0
            && (1..=5).contains(&self.levels)
            && (1..=4).contains(&self.branch_factor)
            && self.taper > 0.0
            && self.taper < 1.0
            && self.split_angle_deg.is_finite()
            && self.split_angle_deg > 0.0
            && self.split_angle_deg < 90.0
            && self.trunk_ratio.is_finite()
            && self.trunk_ratio > 0.0
            && self.card_ratio.is_finite()
            && self.card_ratio > 0.0
            && (1..=16).contains(&self.variants)
            && !self.bark.contains(['/', '\\', '.'])
            && (self.leaves == "broad" || self.leaves == "needle")
    }
}

/// One species, as a `.flora.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloraDef {
    /// The namespaced id, `strata:oak`.
    pub id: Namespaced,
    /// What a HUD calls it.
    pub name: String,
    /// See [`FloraKind`].
    pub kind: FloraKind,
    /// Where it grows. A biome not listed has weight zero.
    pub biomes: Vec<BiomeWeight>,
    /// Bushes and trees: metres between candidate slots. Grass: ignored.
    #[serde(default = "default_spacing")]
    pub spacing_m: f32,
    /// Grass: blades per square metre in pure ground at weight one.
    #[serde(default)]
    pub density_per_m2: f32,
    /// Grass shape: width in metres, resting curvature, tip droop and
    /// proportion of dead blades. The last three values are in `0..=1`.
    #[serde(default = "default_grass_shape")]
    pub grass_shape: [f32; 4],
    /// Rise over run past which the species does not grow.
    #[serde(default = "default_slope_max")]
    pub slope_max: f32,
    /// Height above sea level it grows at, metres.
    #[serde(default = "default_altitude")]
    pub altitude_m: MetreRange,
    /// Metres above the sea a slot must be.
    #[serde(default = "default_sea_clearance")]
    pub sea_clearance_m: f32,
    /// The moisture it tolerates, in the climate field's unit.
    #[serde(default = "default_moisture")]
    pub moisture: UnitRange,
    /// The share of its chance the species keeps where the ground carries
    /// nothing, `0..=1`. Placement scales a chance by the sample's cover,
    /// which would empty a bare patch completely; a species that stands
    /// alone in poor ground (a plains oak) raises this so the patch thins
    /// instead of clearing.
    #[serde(default)]
    pub cover_floor: f32,
    /// Size multiplier range of an instance; grass reads it as blade height
    /// in metres.
    #[serde(default = "default_size")]
    pub size: MetreRange,
    /// Radius other plants keep clear of one of these, metres.
    #[serde(default = "default_exclusion")]
    pub exclusion_m: f32,
    /// Colour in dry ground.
    pub tint_dry: Tint,
    /// Colour in wet ground.
    pub tint_lush: Tint,
    /// Emissive colour, black for a plant that does not glow.
    #[serde(default = "default_emissive")]
    pub emissive: Tint,
    /// The generator's parameters; required for a bush or a tree, left out
    /// for grass and rocks.
    #[serde(default)]
    pub tree: TreeParams,
    /// Where on the ground's shape it prefers to stand.
    #[serde(default)]
    pub ground: GroundPrefs,
    /// How it gathers into groves and leaves clearings.
    #[serde(default)]
    pub cluster: Clustering,
    /// What a placed one brings with it: ferns under a tree, stones around
    /// a boulder.
    #[serde(default)]
    pub children: Vec<ChildRule>,
    /// Dressing: a small plant only a client draws, with no collider and
    /// no children, that never takes another plant's place. Placement
    /// takes its candidate slots after every other species', ranks it
    /// below every other plant when thinning and lets it crowd out no
    /// other's child, so every plant that is not dressing stands where it
    /// would with no dressing species at all, and a server places none
    /// (`strata_flora::FloraTable::without_dressing`). That is why the
    /// world's version leaves it out: a client and a server that differ
    /// in one stand on the same ground among the same trunks and rocks.
    /// The baked tree library's version is what keeps two ends' drawn
    /// plants alike. Written only when set, so a def without it reads as
    /// it always did.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dressing: bool,
}

/// Whether a flag is unset, for a field written only when set.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_false(flag: &bool) -> bool {
    !*flag
}

/// Whether a number is nought, for a field written only when set.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

/// Where on the ground's shape a species prefers to stand, read from the
/// terrain's landform (ADR 0006's 2026-09-26 amendment). The defaults are
/// indifferent, so a species that names none places as it did before.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GroundPrefs {
    /// Rise over run below which it does not stand: a cliff wants a slope.
    pub slope_min: f32,
    /// How much it prefers hollows where water gathers (positive) or knolls
    /// (negative), `-1..=1`.
    pub hollow: f32,
    /// How much it prefers the ridges between erosion gullies (positive) or
    /// the drainage lines in them (negative), `-1..=1`.
    pub ridge: f32,
    /// Face down the slope rather than a random way: a cliff piece's face.
    pub face_downhill: bool,
    /// Stands only on the wet bank of slow water (ticket 0005, step 8):
    /// within this many metres over the surface of a lake, or of a river at
    /// least [`bank_width_min_m`](Self::bank_width_min_m) wide (a creek is
    /// steep and quick, and a fall never slow), where its banks reach; and
    /// there it may stand on the wetted bed, which nothing else does.
    /// Nought stands anywhere, as before. This and the next are written
    /// only when set, so a species without them reads, and hashes into the
    /// world's version, as it always did.
    #[serde(skip_serializing_if = "is_zero")]
    pub bank_m: f32,
    /// The narrowest river whose bank it lines, metres; a lake's always.
    #[serde(skip_serializing_if = "is_zero")]
    pub bank_width_min_m: f32,
}

/// How a species gathers. Candidates keep their grid slots; clustering
/// only changes which survive, so adding it moves no plant that stays.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Clustering {
    /// Radius of one clump, metres; zero places with no clumps.
    pub radius_m: f32,
    /// Clump centres per hectare.
    pub clumps_per_ha: f32,
    /// The scale of the patch field that opens clearings, metres; zero
    /// leaves no clearings.
    pub patch_m: f32,
    /// The share of the ground the patch field keeps, `0..=1`.
    pub patch_cover: f32,
}

/// Plants a placed one of this species brings with it, each hashed from its
/// parent's identity, so a child is where its parent is on every machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildRule {
    /// The child's species, `strata:woodland_fern`.
    pub species: Namespaced,
    /// How many, fewest and most.
    pub count: [u8; 2],
    /// Metres from the parent's root, nearest and furthest.
    pub radius_m: [f32; 2],
}

const fn default_spacing() -> f32 {
    10.0
}
const fn default_grass_shape() -> [f32; 4] {
    [0.04, 0.2, 0.08, 0.1]
}
const fn default_slope_max() -> f32 {
    0.6
}
const fn default_altitude() -> MetreRange {
    MetreRange::new(-100.0, 10_000.0)
}
const fn default_sea_clearance() -> f32 {
    0.5
}
const fn default_moisture() -> UnitRange {
    UnitRange::FULL
}
const fn default_size() -> MetreRange {
    MetreRange::new(0.8, 1.2)
}
const fn default_exclusion() -> f32 {
    1.0
}
const fn default_emissive() -> Tint {
    Tint([0.0; 3])
}

/// Placement cell size shared by validation and the placement engine.
pub const FOLIAGE_CELL_M: f32 = 64.0;
/// Grass-only palette capacity; unrelated flora does not consume slots.
pub const MAX_GRASS_SPECIES: usize = 32;
/// Maximum records uploaded for one cell at either grass LOD.
pub const MAX_GRASS_BLADES_PER_CELL: usize = 32_768;
/// Aggregate grass candidate budget for a cell, before environmental thinning.
pub const MAX_GRASS_CANDIDATES_PER_CELL: usize = 65_536;
/// Aggregate tree and bush candidate budget for a cell.
pub const MAX_PLANT_CANDIDATES_PER_CELL: usize = 4_096;
/// Bounds neighboring-cell exclusion searches.
pub const MAX_EXCLUSION_M: f32 = 16.0;

impl FloraDef {
    /// Candidate slots evaluated per cell before habitat and exclusion tests.
    /// Saturates for invalid inputs, so validation cannot overflow.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn candidate_count(&self) -> usize {
        let side = match self.kind {
            FloraKind::Grass => (FOLIAGE_CELL_M * self.density_per_m2.sqrt()).round(),
            _ => (FOLIAGE_CELL_M / self.spacing_m).floor(),
        }
        .max(1.0) as usize;
        side.saturating_mul(side)
    }

    /// Why the numbers are unusable, or `None` when they are fine.
    #[must_use]
    pub fn invalid_reason(&self) -> Option<&'static str> {
        if self.biomes.is_empty() {
            return Some("a species needs at least one biome");
        }
        if self
            .biomes
            .iter()
            .any(|b| !(b.weight.is_finite() && (0.0..=1.0).contains(&b.weight)))
        {
            return Some("every biome weight must be in 0..=1");
        }
        if !(self.spacing_m.is_finite() && self.spacing_m > 0.0) {
            return Some("spacing_m must be positive");
        }
        if !(self.density_per_m2.is_finite() && self.density_per_m2 >= 0.0) {
            return Some("density_per_m2 must be non-negative");
        }
        if self.kind == FloraKind::Grass && self.density_per_m2 <= 0.0 {
            return Some("grass needs a positive density_per_m2");
        }
        let candidate_limit = if self.kind == FloraKind::Grass {
            MAX_GRASS_CANDIDATES_PER_CELL
        } else {
            MAX_PLANT_CANDIDATES_PER_CELL
        };
        if self.candidate_count() > candidate_limit {
            return Some("spacing_m or density_per_m2 exceeds the per-cell candidate budget");
        }
        if !(self.grass_shape[0].is_finite() && self.grass_shape[0] > 0.0)
            || self.grass_shape[1..]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Some(
                "grass_shape needs positive width and curvature, droop, dead share in 0..=1",
            );
        }
        if !(self.slope_max.is_finite() && self.slope_max > 0.0) {
            return Some("slope_max must be positive");
        }
        if !self.altitude_m.is_valid() || !self.size.is_valid() || self.size.min <= 0.0 {
            return Some("altitude_m and size must be ordered, size positive");
        }
        if !self.sea_clearance_m.is_finite() {
            return Some("sea_clearance_m must be finite");
        }
        if !self.moisture.is_valid() {
            return Some("moisture must be a range inside 0..=1");
        }
        if !(self.cover_floor.is_finite() && (0.0..=1.0).contains(&self.cover_floor)) {
            return Some("cover_floor must be in 0..=1");
        }
        if !(self.exclusion_m.is_finite() && (0.0..=MAX_EXCLUSION_M).contains(&self.exclusion_m)) {
            return Some("exclusion_m must be between 0 and 16 metres");
        }
        let within = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        let g = &self.ground;
        if !(within(g.slope_min, 0.0, f32::MAX)
            && within(g.hollow, -1.0, 1.0)
            && within(g.ridge, -1.0, 1.0))
        {
            return Some("ground needs slope_min >= 0 and hollow, ridge in -1..=1");
        }
        if !(within(g.bank_m, 0.0, 10.0) && within(g.bank_width_min_m, 0.0, f32::MAX)) {
            return Some("ground needs bank_m in 0..=10 and a non-negative bank_width_min_m");
        }
        // The forms the generators grow with no collider: a shrub's and a
        // log's capsule would be a body a server never places.
        if self.dressing
            && (self.kind != FloraKind::Bush
                || !self.children.is_empty()
                || !matches!(self.tree.form.as_str(), "fern" | "flower" | "reed"))
        {
            return Some("dressing is a fern, flower or reed bush that brings no children");
        }
        let c = &self.cluster;
        if !(within(c.radius_m, 0.0, MAX_EXCLUSION_M * 2.0)
            && within(c.clumps_per_ha, 0.0, f32::MAX)
            && within(c.patch_m, 0.0, f32::MAX)
            && within(c.patch_cover, 0.0, 1.0))
        {
            return Some(
                "cluster needs radius_m in 0..=32, non-negative clumps and patch_m, patch_cover in 0..=1",
            );
        }
        if self.children.iter().any(|child| {
            child.count[0] > child.count[1]
                || child.count[1] > 16
                || !(child.radius_m[0].is_finite()
                    && child.radius_m[0] >= 0.0
                    && child.radius_m[0] <= child.radius_m[1]
                    && child.radius_m[1] <= MAX_EXCLUSION_M)
        }) {
            return Some("children need ordered counts up to 16 and radii in 0..=16 m");
        }
        match (self.kind, self.tree.is_absent()) {
            (FloraKind::Grass, false) => Some("grass has no tree parameters"),
            (FloraKind::Grass | FloraKind::Rock, true) => None,
            (_, true) => Some("a bush or a tree needs tree parameters with a height"),
            (_, false) => (!self.tree.is_valid()).then_some("tree parameters out of range"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_def_survives_the_value_round_trip_with_defaults() {
        let text = r#"(
            id: "strata:oak",
            name: "Oak",
            kind: "tree",
            biomes: [(biome: "strata:forest", weight: 0.8)],
            spacing_m: 9.0,
            tint_dry: (0.3, 0.4, 0.1),
            tint_lush: (0.2, 0.5, 0.1),
            tree: (height_m: (min: 7.0, max: 11.0)),
        )"#;
        let value: ron::Value = ron::from_str(text).expect("parses");
        let def: FloraDef = value.into_rust().expect("typed");
        assert_eq!(def.kind, FloraKind::Tree);
        assert_eq!(def.tree.levels, 3);
        assert_eq!(def.tree.form, "tree");
        assert!(def.cover_floor.abs() < f32::EPSILON, "the default is bare");
        assert_eq!(def.invalid_reason(), None);
        let back = ron::to_string(&def).expect("serialises");
        let again: FloraDef = ron::from_str(&back).expect("parses");
        assert_eq!(again, def);
    }

    #[test]
    fn tree_ratios_reject_non_finite_values() {
        let valid = TreeParams {
            height_m: MetreRange::new(1.0, 2.0),
            ..TreeParams::default()
        };
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                !TreeParams {
                    trunk_ratio: value,
                    ..valid.clone()
                }
                .is_valid()
            );
            assert!(
                !TreeParams {
                    card_ratio: value,
                    ..valid.clone()
                }
                .is_valid()
            );
        }
    }

    #[test]
    fn procedural_forms_are_explicit_and_validated() {
        let mut params = TreeParams {
            height_m: MetreRange::new(0.3, 1.0),
            ..TreeParams::default()
        };
        for form in ["tree", "shrub", "fern", "flower", "log", "reed"] {
            params.form = form.into();
            assert!(params.is_valid());
        }
        params.form = "unknown".into();
        assert!(!params.is_valid());
    }

    #[test]
    fn grass_and_trees_are_checked_for_their_kind() {
        let mut grass = FloraDef {
            id: Namespaced::parse("strata:grass").expect("valid"),
            name: "Grass".into(),
            kind: FloraKind::Grass,
            biomes: vec![BiomeWeight {
                biome: Namespaced::parse("strata:plains").expect("valid"),
                weight: 1.0,
            }],
            spacing_m: 1.0,
            density_per_m2: 0.0,
            grass_shape: default_grass_shape(),
            slope_max: 0.6,
            altitude_m: default_altitude(),
            sea_clearance_m: 0.5,
            moisture: UnitRange::FULL,
            cover_floor: 0.0,
            size: MetreRange::new(0.3, 0.6),
            exclusion_m: 0.0,
            tint_dry: Tint([0.5; 3]),
            tint_lush: Tint([0.5; 3]),
            emissive: Tint([0.0; 3]),
            tree: TreeParams::default(),
            ground: GroundPrefs::default(),
            cluster: Clustering::default(),
            children: Vec::new(),
            dressing: false,
        };
        assert!(grass.invalid_reason().is_some(), "no density");
        grass.density_per_m2 = 4.0;
        assert_eq!(grass.invalid_reason(), None);
        grass.kind = FloraKind::Tree;
        assert!(grass.invalid_reason().is_some(), "a tree needs parameters");
    }

    #[test]
    fn only_a_bush_with_no_collider_or_children_is_dressing() {
        let mut reed = FloraDef {
            id: Namespaced::parse("strata:reed").expect("valid"),
            name: "Reeds".into(),
            kind: FloraKind::Bush,
            biomes: vec![BiomeWeight {
                biome: Namespaced::parse("strata:plains").expect("valid"),
                weight: 1.0,
            }],
            spacing_m: 1.5,
            density_per_m2: 0.0,
            grass_shape: default_grass_shape(),
            slope_max: 0.35,
            altitude_m: default_altitude(),
            sea_clearance_m: 0.03,
            moisture: UnitRange::FULL,
            cover_floor: 1.0,
            size: MetreRange::new(0.8, 1.15),
            exclusion_m: 0.55,
            tint_dry: Tint([0.5; 3]),
            tint_lush: Tint([0.5; 3]),
            emissive: Tint([0.0; 3]),
            tree: TreeParams {
                form: "reed".into(),
                height_m: MetreRange::new(1.4, 2.1),
                ..TreeParams::default()
            },
            ground: GroundPrefs::default(),
            cluster: Clustering::default(),
            children: Vec::new(),
            dressing: true,
        };
        assert_eq!(reed.invalid_reason(), None);
        reed.tree.form = "log".into();
        assert!(reed.invalid_reason().is_some(), "a log collides");
        reed.tree.form = "shrub".into();
        assert!(reed.invalid_reason().is_some(), "a shrub collides");
        reed.tree.form = "reed".into();
        reed.kind = FloraKind::Tree;
        assert!(reed.invalid_reason().is_some(), "a tree is the world's");
    }
}
