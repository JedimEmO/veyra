//! Geological provinces: what the ground is made of, as `.geology.ron`
//! files spell it (ticket 0004, step 3).
//!
//! A biome is the climate's: what grows. A province is the rock's: which
//! faces show where the ground is bare, how the relief a biome names takes
//! shape on it, how its beds lie and how water cuts it, and which rock kit
//! dresses it. The two cross, so a wet forest on limestone is not a wet
//! forest on granite. The world generator chooses a province per point by a
//! field of its own (`strata-worldgen`'s `geology`), which has nothing to
//! do with the climate.
//!
//! Every section is read and validated at load even where nothing uses it
//! yet (the relief rules are step 4's, the beds step 5's, the water steps
//! 6 and 7's, the kit the placement's): a wrong file fails today, not when
//! the step that reads it lands.
//!
//! Geology is optional. A registry set with no provinces gives a world
//! whose province field is off, with every sample exactly as it was before
//! provinces existed; the small sets the tests build rely on that.
//!
//! The `Value`-safe rule of the other defs holds: patch lists rewrite the
//! raw RON before it is typed, and RON's `Value` forgets enum variant
//! names, so every enum-like field (a relief class, a shape, a water
//! section) is a lowercase string, parsed and checked at freeze, where the
//! error can name the field.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::biome::{ReliefKind, Tint};
use crate::flora::{FloraId, MetreRange};
use crate::material::TerrainMaterialId;

/// A dense handle to a frozen province, assigned by the freeze in sorted-id
/// order so the same asset set gives the same ids however the directory
/// lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProvinceId(pub u32);

slotted_registry::impl_interned_id!(ProvinceId);

impl ProvinceId {
    /// No province: what a sample carries in a world without geology, so
    /// a consumer can tell "no rock data" from the first province.
    pub const NONE: Self = Self(u32::MAX);

    /// Whether this is [`NONE`](Self::NONE).
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == u32::MAX
    }
}

/// The most provinces one frozen set may hold. The field deals provinces
/// into blocks of at most sixteen cells, so every province appears in
/// every block; a seventeenth could not.
pub const MAX_PROVINCES: usize = 16;

/// One province, as a `.geology.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvinceDef {
    /// The namespaced id, `strata:granite`, and the file's key in the data
    /// stage.
    pub id: Namespaced,
    /// What a map legend or a HUD calls it.
    pub name: String,
    /// The colour `just worldmap`'s `-geology` layer paints it, `0..=1`
    /// sRGB. Chosen to tell provinces apart, not as the rock's colour.
    pub tint: Tint,
    /// Its share of the field's cells against the other provinces'. The
    /// field deals whole cells a block (of 6, 8, 9, 12 or 16, the one whose
    /// slots come nearest every frequency), largest remainders first, so
    /// shares are exact only to a slot of the block: 0.4 among five at 1
    /// takes one slot of sixteen, 6.25 % of the cells against the 7.4 %
    /// asked. At least a sixteenth of all the provinces' frequencies, or the
    /// freeze refuses it: less might be dealt no cell and be nowhere.
    #[serde(default = "one")]
    pub frequency: f32,
    /// Where it stands in the order the field deals provinces into a
    /// block's slots before each block's shuffle, lowest first, ties by
    /// id. For a seed, which cell holds which province follows from this
    /// order, so changing it moves every province; it is data rather than
    /// the id's order so that a province added or renamed does not reshuffle
    /// the others. The shipped six keep the order of the design's prototype
    /// (ticket 0004's table: granite, sandstone, karst, basalt, folded,
    /// badlands), whose maps and fixtures were judged.
    #[serde(default)]
    pub deal_order: u32,
    /// The province that takes a point over where a mountain range rises
    /// through this one (the range field from the world's
    /// `province_range_lo` to `province_range_hi`, 0.35 to 0.6), or `None` to
    /// stay itself: sandstone country whose ranges are granite, as the
    /// laccoliths of the Colorado Plateau are. One step: the province named
    /// is not asked in turn.
    #[serde(default)]
    pub in_ranges: Option<Namespaced>,
    /// What its bare ground is.
    pub rock: Rock,
    /// How the relief a biome names takes shape on it, per relief class
    /// (ticket 0004, step 4). A class it does not list keeps the biome's
    /// own shape, amplitude and wavelength.
    #[serde(default)]
    pub relief: Vec<ShapeRule>,
    /// Its beds (step 5). The default is none.
    #[serde(default)]
    pub beds: Beds,
    /// How water cuts it (steps 6 and 7).
    #[serde(default)]
    pub water: Water,
    /// The rock species that dress it, by weight; placement reads it.
    #[serde(default)]
    pub kit: Vec<KitEntry>,
}

/// What a province's bare ground is. The part of a province step 3 reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rock {
    /// The terrain material of its bare faces. On wild ground it wins over
    /// the biome's `slope_material`, which stays as the face of a world
    /// without geology.
    pub face: Namespaced,
    /// Multiplies the biome's `slope_threshold`, `(0, 4]`: hard rock bares
    /// at a lower slope than soft.
    #[serde(default = "one")]
    pub bare_slope: f32,
    /// Multiplies the biome's far tint, each channel `0..=2`, so a red
    /// country reads red from a ridge when its faces are too far to see.
    #[serde(default = "white")]
    pub far_tint: Tint,
    /// Multiplies the ground's cover, `0..=1`: badlands shed soil too fast
    /// for anything to hold.
    #[serde(default = "one")]
    pub cover: f32,
}

/// How one relief class takes shape on a province (step 4).
///
/// Across a province's border the amplitude, damping, offset and erosion
/// blend by the provinces' weights; the shape, wavelength and stretch do
/// not (a wavelength that changed across a border would move the noise's
/// phase by tens of cycles inside the blend 50 km out), so two provinces
/// with different ones are two noises whose heights blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeRule {
    /// The biome relief class it applies to: plains, hills, mountains,
    /// dunes or ridges.
    pub class: String,
    /// The shape the class takes here: fbm, ridged, billow, damped, hybrid
    /// or cells.
    pub shape: String,
    /// Multiplies the biome's `amplitude_m`, at least 0. The shape's value
    /// is laid onto the class's range about the middle the biome's own
    /// shape would have at this amplitude: its variation about its own mean
    /// by half the amplitude for a signed shape (fbm, damped, hybrid) and the
    /// whole of it for an unsigned one (ridged, billow, cells). A fractal
    /// normalised by its octaves rarely leaves the middle fifth of its
    /// range, so a rule that should stand out takes several times the
    /// biome's amplitude (step 4's damped rules take 10 to 18).
    #[serde(default = "one")]
    pub amplitude: f32,
    /// Multiplies the biome's `wavelength_m`, over 0. Does not blend across
    /// a border (see the type's docs).
    #[serde(default = "one")]
    pub wavelength: f32,
    /// Multiplies the biome's `erosion`, `0..=4`; the product is clamped to
    /// 4.
    #[serde(default = "one")]
    pub erosion: f32,
    /// For `damped`: `k` in `1 / (1 + k |D|^2)`, at least 0, which
    /// flattens the fine octaves where the coarse ones are steep. `D` is
    /// the gradient of the octaves so far in noise space, times the first
    /// wavelength (Quilez's sum of derivatives), whose size runs to 3 and
    /// more: 1.5 to 2 makes benches; the design's 2 to 3 laid the ground
    /// flat but for creases (ticket 0004, step 4).
    #[serde(default)]
    pub damping: f32,
    /// For `hybrid`: the multifractal's offset, `0..=2`.
    #[serde(default = "default_offset")]
    pub offset: f32,
    /// How much longer than wide its features are along the seed's range
    /// axis, the fold grain, `1..=16`. Does not blend across a border.
    #[serde(default = "one")]
    pub stretch: f32,
}

/// A province's beds (step 5): hard and soft layers, and how they lie.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Beds {
    /// How thick one bed is, metres, over 0.
    #[serde(default = "default_thickness")]
    pub thickness_m: MetreRange,
    /// The share of beds that are hard, `0..=1`.
    #[serde(default = "half")]
    pub hard_share: f32,
    /// How far a hard riser steepens the ground, `0..=1`; 0 is no beds.
    #[serde(default)]
    pub strength: f32,
    /// How steeply the beds dip, degrees, `0..=90`.
    #[serde(default = "flat")]
    pub dip_deg: MetreRange,
    /// Over how far the dip turns, metres, over 0.
    #[serde(default = "default_dip_wavelength")]
    pub dip_wavelength_m: f32,
    /// Rise over run over which beds fade in by the slope already there,
    /// `min < max`, both at least 0: gentle ground keeps its shape.
    #[serde(default = "default_slope_gate")]
    pub slope_gate: MetreRange,
}

impl Default for Beds {
    fn default() -> Self {
        Self {
            thickness_m: default_thickness(),
            hard_share: half(),
            strength: 0.0,
            dip_deg: flat(),
            dip_wavelength_m: default_dip_wavelength(),
            slope_gate: default_slope_gate(),
        }
    }
}

/// How water cuts a province (steps 6 and 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Water {
    /// A valley's cross-section: v, trough, stepped, slot or gully.
    #[serde(default = "default_section")]
    pub section: String,
    /// Pits deeper than this stay lakes, shallower ones are breached,
    /// metres, at least 0.
    #[serde(default = "default_keep_pits")]
    pub keep_pits_m: f32,
    /// The share of the flow that goes underground, `0..=1`.
    #[serde(default)]
    pub sink: f32,
    /// A tributary's cut as a share of its main stem's, `(0, 1]`; under 1
    /// leaves hanging valleys.
    #[serde(default = "one")]
    pub hang: f32,
    /// How much the routing prefers the fold axis, `0..=1`: trellis
    /// drainage.
    #[serde(default)]
    pub along_grain: f32,
}

impl Default for Water {
    fn default() -> Self {
        Self {
            section: default_section(),
            keep_pits_m: default_keep_pits(),
            sink: 0.0,
            hang: 1.0,
            along_grain: 0.0,
        }
    }
}

/// One rock species a province is dressed with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitEntry {
    /// A flora species of kind `rock`.
    pub species: Namespaced,
    /// `0..=1`: how much of the species' fit the province keeps.
    pub weight: f32,
}

const fn one() -> f32 {
    1.0
}

const fn half() -> f32 {
    0.5
}

const fn white() -> Tint {
    Tint([1.0; 3])
}

const fn default_offset() -> f32 {
    0.7
}

const fn default_thickness() -> MetreRange {
    MetreRange::new(5.0, 20.0)
}

const fn flat() -> MetreRange {
    MetreRange::new(0.0, 0.0)
}

const fn default_dip_wavelength() -> f32 {
    30_000.0
}

const fn default_slope_gate() -> MetreRange {
    MetreRange::new(0.15, 0.5)
}

fn default_section() -> String {
    "v".to_owned()
}

const fn default_keep_pits() -> f32 {
    10.0
}

/// The shape a relief class takes on a province.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProvinceShape {
    /// Plain fbm.
    Fbm,
    /// Ridged multifractal.
    Ridged,
    /// Billow: rounded cushions and domes.
    Billow,
    /// fbm whose fine octaves are damped by the slope of the coarse ones:
    /// flat in places, steep in others.
    Damped,
    /// A hybrid multifractal: smooth valleys, rough heights.
    Hybrid,
    /// Cellular hollows: closed depressions, the karst's.
    Cells,
}

impl ProvinceShape {
    /// Every shape, for iteration and error messages.
    pub const ALL: [Self; 6] = [
        Self::Fbm,
        Self::Ridged,
        Self::Billow,
        Self::Damped,
        Self::Hybrid,
        Self::Cells,
    ];

    /// The lowercase name used in RON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fbm => "fbm",
            Self::Ridged => "ridged",
            Self::Billow => "billow",
            Self::Damped => "damped",
            Self::Hybrid => "hybrid",
            Self::Cells => "cells",
        }
    }
}

impl fmt::Display for ProvinceShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProvinceShape {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|v| v.as_str() == s).ok_or(())
    }
}

/// A valley's cross-section where water cuts a province.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaterSection {
    /// A V, the usual.
    V,
    /// A broad U-shaped trough.
    Trough,
    /// A canyon in steps, benches on the soft beds.
    Stepped,
    /// A slot: narrow, deep, sheer.
    Slot,
    /// A maze of small gullies.
    Gully,
}

impl WaterSection {
    /// Every section, for iteration and error messages.
    pub const ALL: [Self; 5] = [
        Self::V,
        Self::Trough,
        Self::Stepped,
        Self::Slot,
        Self::Gully,
    ];

    /// The lowercase name used in RON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V => "v",
            Self::Trough => "trough",
            Self::Stepped => "stepped",
            Self::Slot => "slot",
            Self::Gully => "gully",
        }
    }
}

impl fmt::Display for WaterSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for WaterSection {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|v| v.as_str() == s).ok_or(())
    }
}

/// A province's rock with its face resolved: what the terrain and the
/// sampler read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProvinceRock {
    /// The texture array layer of its bare faces.
    pub face: TerrainMaterialId,
    /// Multiplies a biome's `slope_threshold`.
    pub bare_slope: f32,
    /// Multiplies a biome's far tint.
    pub far_tint: Tint,
    /// Multiplies the ground's cover.
    pub cover: f32,
}

/// One relief rule with its class and shape parsed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProvinceShapeRule {
    /// The biome relief class it applies to.
    pub class: ReliefKind,
    /// The shape the class takes.
    pub shape: ProvinceShape,
    /// Multiplies the biome's amplitude.
    pub amplitude: f32,
    /// Multiplies the biome's wavelength.
    pub wavelength: f32,
    /// Multiplies the biome's erosion (the product clamped to 4).
    pub erosion: f32,
    /// The damping of a `damped` shape.
    pub damping: f32,
    /// The offset of a `hybrid` shape.
    pub offset: f32,
    /// The stretch along the fold grain.
    pub stretch: f32,
}

/// A frozen province with every name resolved and every string parsed, so
/// nothing downstream touches a name or a string again.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvinceTraits {
    /// Its rock.
    pub rock: ProvinceRock,
    /// The province that takes over in the ranges; itself when the file
    /// names none.
    pub in_ranges: ProvinceId,
    /// Its relief rules, at most one per class.
    pub relief: Vec<ProvinceShapeRule>,
    /// Its water's section, the rest of the water section being numbers.
    pub section: WaterSection,
    /// Its kit, resolved to species ids.
    pub kit: Vec<(FloraId, f32)>,
}

impl ProvinceTraits {
    /// The rule for a relief class, or `None` for the identity: the
    /// biome's own shape, amplitude and wavelength.
    #[must_use]
    pub fn rule(&self, class: ReliefKind) -> Option<&ProvinceShapeRule> {
        self.relief.iter().find(|rule| rule.class == class)
    }
}

/// What is wrong with one field of a province, before its names are
/// resolved: the field's path in the file and why.
pub(crate) type FieldError = (String, String);

fn unit(v: f32) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

impl ProvinceDef {
    /// The first field whose number or string is unusable, with its path in
    /// the file (`rock.bare_slope`, `relief[1].shape`) and the reason.
    /// Names are resolved by the freeze, which knows the other registries.
    #[must_use]
    pub fn invalid_field(&self) -> Option<(String, String)> {
        self.check().err()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every bound of the schema in one flat list, in the file's order"
    )]
    fn check(&self) -> Result<(), FieldError> {
        let fail = |field: &str, reason: &str| Err((field.to_owned(), reason.to_owned()));
        if self.name.trim().is_empty() {
            return fail("name", "must not be empty");
        }
        if !self.tint.0.iter().all(|&c| unit(c)) {
            return fail("tint", "each channel must be in 0..=1");
        }
        if !(self.frequency.is_finite() && self.frequency > 0.0) {
            return fail("frequency", "must be over 0");
        }
        let rock = &self.rock;
        if !(rock.bare_slope.is_finite() && rock.bare_slope > 0.0 && rock.bare_slope <= 4.0) {
            return fail("rock.bare_slope", "must be in (0, 4]");
        }
        if !rock
            .far_tint
            .0
            .iter()
            .all(|&c| c.is_finite() && (0.0..=2.0).contains(&c))
        {
            return fail("rock.far_tint", "each channel must be in 0..=2");
        }
        if !unit(rock.cover) {
            return fail("rock.cover", "must be in 0..=1");
        }
        let mut classes = Vec::with_capacity(self.relief.len());
        for (i, rule) in self.relief.iter().enumerate() {
            let at = |field: &str| format!("relief[{i}].{field}");
            let Ok(class) = rule.class.parse::<ReliefKind>() else {
                return Err((
                    at("class"),
                    format!(
                        "`{}` is not a relief class; expected plains, hills, mountains, dunes \
                         or ridges",
                        rule.class
                    ),
                ));
            };
            if classes.contains(&class) {
                return Err((at("class"), format!("`{class}` is listed twice")));
            }
            classes.push(class);
            if rule.shape.parse::<ProvinceShape>().is_err() {
                return Err((
                    at("shape"),
                    format!(
                        "`{}` is not a shape; expected fbm, ridged, billow, damped, hybrid or \
                         cells",
                        rule.shape
                    ),
                ));
            }
            let checks = [
                (
                    "amplitude",
                    rule.amplitude.is_finite() && rule.amplitude >= 0.0,
                    "must be at least 0",
                ),
                (
                    "wavelength",
                    rule.wavelength.is_finite() && rule.wavelength > 0.0,
                    "must be over 0",
                ),
                (
                    "erosion",
                    rule.erosion.is_finite() && (0.0..=4.0).contains(&rule.erosion),
                    "must be in 0..=4",
                ),
                (
                    "damping",
                    rule.damping.is_finite() && rule.damping >= 0.0,
                    "must be at least 0",
                ),
                (
                    "offset",
                    rule.offset.is_finite() && (0.0..=2.0).contains(&rule.offset),
                    "must be in 0..=2",
                ),
                (
                    "stretch",
                    rule.stretch.is_finite() && (1.0..=16.0).contains(&rule.stretch),
                    "must be in 1..=16",
                ),
            ];
            if let Some((field, _, reason)) = checks.iter().find(|c| !c.1) {
                return Err((at(field), (*reason).to_owned()));
            }
        }
        let beds = &self.beds;
        let checks = [
            (
                "beds.thickness_m",
                beds.thickness_m.is_valid() && beds.thickness_m.min > 0.0,
                "must be ordered, with min over 0",
            ),
            ("beds.hard_share", unit(beds.hard_share), "must be in 0..=1"),
            ("beds.strength", unit(beds.strength), "must be in 0..=1"),
            (
                "beds.dip_deg",
                beds.dip_deg.is_valid() && beds.dip_deg.min >= 0.0 && beds.dip_deg.max <= 90.0,
                "must be ordered and in 0..=90 degrees",
            ),
            (
                "beds.dip_wavelength_m",
                beds.dip_wavelength_m.is_finite() && beds.dip_wavelength_m > 0.0,
                "must be over 0",
            ),
            (
                "beds.slope_gate",
                beds.slope_gate.is_valid()
                    && beds.slope_gate.min >= 0.0
                    && beds.slope_gate.min < beds.slope_gate.max,
                "must have 0 <= min < max",
            ),
        ];
        if let Some((field, _, reason)) = checks.iter().find(|c| !c.1) {
            return fail(field, reason);
        }
        let water = &self.water;
        if water.section.parse::<WaterSection>().is_err() {
            return Err((
                "water.section".to_owned(),
                format!(
                    "`{}` is not a section; expected v, trough, stepped, slot or gully",
                    water.section
                ),
            ));
        }
        let checks = [
            (
                "water.keep_pits_m",
                water.keep_pits_m.is_finite() && water.keep_pits_m >= 0.0,
                "must be at least 0",
            ),
            ("water.sink", unit(water.sink), "must be in 0..=1"),
            (
                "water.hang",
                water.hang.is_finite() && water.hang > 0.0 && water.hang <= 1.0,
                "must be in (0, 1]",
            ),
            (
                "water.along_grain",
                unit(water.along_grain),
                "must be in 0..=1",
            ),
        ];
        if let Some((field, _, reason)) = checks.iter().find(|c| !c.1) {
            return fail(field, reason);
        }
        for (i, entry) in self.kit.iter().enumerate() {
            if !unit(entry.weight) {
                return Err((format!("kit[{i}].weight"), "must be in 0..=1".to_owned()));
            }
            if self.kit[..i].iter().any(|e| e.species == entry.species) {
                return Err((
                    format!("kit[{i}].species"),
                    format!("`{}` is listed twice", entry.species),
                ));
            }
        }
        Ok(())
    }

    /// The relief rules parsed, for a def [`invalid_field`] passed.
    ///
    /// [`invalid_field`]: Self::invalid_field
    pub(crate) fn parsed_relief(&self) -> Vec<ProvinceShapeRule> {
        self.relief
            .iter()
            .filter_map(|rule| {
                Some(ProvinceShapeRule {
                    class: rule.class.parse().ok()?,
                    shape: rule.shape.parse().ok()?,
                    amplitude: rule.amplitude,
                    wavelength: rule.wavelength,
                    erosion: rule.erosion,
                    damping: rule.damping,
                    offset: rule.offset,
                    stretch: rule.stretch,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const GRANITE: &str = r#"(
        id: "strata:granite",
        name: "Granite massif",
        tint: (0.93, 0.66, 0.70),
        in_ranges: Some("strata:folded"),
        rock: (face: "strata:rock", bare_slope: 0.8, far_tint: (1.04, 1.02, 1.0), cover: 0.9),
        relief: [
            (class: "plains", shape: "billow", amplitude: 1.6, wavelength: 1.3, erosion: 0.5),
            (class: "mountains", shape: "ridged"),
        ],
        beds: (strength: 0.0),
        water: (section: "trough", keep_pits_m: 6.0, hang: 0.5),
        kit: [(species: "strata:granite_stone", weight: 1.0)],
    )"#;

    #[test]
    fn a_province_survives_the_value_round_trip() {
        let value: ron::Value = ron::from_str(GRANITE).expect("parses as a value");
        let def: ProvinceDef = value.into_rust().expect("typed from the value");
        assert_eq!(
            def.in_ranges,
            Some(Namespaced::parse("strata:folded").expect("valid"))
        );
        assert_eq!(def.invalid_field(), None);
        let text = ron::to_string(&def).expect("serialises");
        let value: ron::Value = ron::from_str(&text).expect("parses as a value");
        let back: ProvinceDef = value.into_rust().expect("typed from the value");
        assert_eq!(back, def);
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "defaults are exact literals")]
    fn defaults_fill_what_a_file_leaves_out() {
        let text = r#"(id: "strata:plain", name: "Plain", tint: (0.5, 0.5, 0.5),
            rock: (face: "strata:rock"))"#;
        let value: ron::Value = ron::from_str(text).expect("parses");
        let def: ProvinceDef = value.into_rust().expect("typed");
        assert_eq!(def.frequency, 1.0);
        assert_eq!(def.deal_order, 0);
        assert_eq!(def.in_ranges, None);
        assert_eq!(def.rock.bare_slope, 1.0);
        assert_eq!(def.rock.far_tint, Tint([1.0; 3]));
        assert_eq!(def.rock.cover, 1.0);
        assert!(def.relief.is_empty() && def.kit.is_empty());
        assert_eq!(def.beds, Beds::default());
        assert_eq!(def.beds.strength, 0.0, "no beds");
        assert_eq!(def.water, Water::default());
        assert_eq!(def.water.section, "v");
        assert_eq!(def.invalid_field(), None);
    }

    #[test]
    fn strings_parse_and_print_alike() {
        for shape in ProvinceShape::ALL {
            assert_eq!(shape.as_str().parse::<ProvinceShape>(), Ok(shape));
            assert_eq!(shape.to_string(), shape.as_str());
        }
        for section in WaterSection::ALL {
            assert_eq!(section.as_str().parse::<WaterSection>(), Ok(section));
            assert_eq!(section.to_string(), section.as_str());
        }
        assert!("canyon".parse::<WaterSection>().is_err());
        assert!("perlin".parse::<ProvinceShape>().is_err());
        assert!(ProvinceId::NONE.is_none() && !ProvinceId(0).is_none());
    }

    /// Every number the schema bounds, pushed out of bounds one at a time,
    /// is refused, and the refusal names that field.
    #[test]
    fn every_bound_names_its_field() {
        let good: ProvinceDef = ron::from_str::<ron::Value>(GRANITE)
            .expect("parses")
            .into_rust()
            .expect("typed");
        let cases: Vec<(&str, Box<dyn Fn(&mut ProvinceDef)>)> = vec![
            ("name", Box::new(|d| d.name = " ".into())),
            ("tint", Box::new(|d| d.tint.0[1] = 1.5)),
            ("frequency", Box::new(|d| d.frequency = 0.0)),
            ("frequency", Box::new(|d| d.frequency = f32::NAN)),
            ("rock.bare_slope", Box::new(|d| d.rock.bare_slope = 0.0)),
            ("rock.bare_slope", Box::new(|d| d.rock.bare_slope = 4.5)),
            ("rock.far_tint", Box::new(|d| d.rock.far_tint.0[2] = 2.5)),
            ("rock.cover", Box::new(|d| d.rock.cover = 1.2)),
            (
                "relief[0].class",
                Box::new(|d| d.relief[0].class = "volcano".into()),
            ),
            (
                "relief[1].class",
                Box::new(|d| d.relief[1].class = "plains".into()),
            ),
            (
                "relief[1].shape",
                Box::new(|d| d.relief[1].shape = "perlin".into()),
            ),
            (
                "relief[0].amplitude",
                Box::new(|d| d.relief[0].amplitude = -1.0),
            ),
            (
                "relief[0].wavelength",
                Box::new(|d| d.relief[0].wavelength = 0.0),
            ),
            ("relief[0].erosion", Box::new(|d| d.relief[0].erosion = 5.0)),
            (
                "relief[0].damping",
                Box::new(|d| d.relief[0].damping = -0.1),
            ),
            ("relief[0].offset", Box::new(|d| d.relief[0].offset = 3.0)),
            ("relief[0].stretch", Box::new(|d| d.relief[0].stretch = 0.5)),
            (
                "beds.thickness_m",
                Box::new(|d| d.beds.thickness_m = MetreRange::new(0.0, 5.0)),
            ),
            (
                "beds.thickness_m",
                Box::new(|d| d.beds.thickness_m = MetreRange::new(9.0, 5.0)),
            ),
            ("beds.hard_share", Box::new(|d| d.beds.hard_share = -0.1)),
            ("beds.strength", Box::new(|d| d.beds.strength = 1.1)),
            (
                "beds.dip_deg",
                Box::new(|d| d.beds.dip_deg = MetreRange::new(10.0, 95.0)),
            ),
            (
                "beds.dip_wavelength_m",
                Box::new(|d| d.beds.dip_wavelength_m = 0.0),
            ),
            (
                "beds.slope_gate",
                Box::new(|d| d.beds.slope_gate = MetreRange::new(0.3, 0.3)),
            ),
            (
                "water.section",
                Box::new(|d| d.water.section = "canyon".into()),
            ),
            (
                "water.keep_pits_m",
                Box::new(|d| d.water.keep_pits_m = -1.0),
            ),
            ("water.sink", Box::new(|d| d.water.sink = 2.0)),
            ("water.hang", Box::new(|d| d.water.hang = 0.0)),
            (
                "water.along_grain",
                Box::new(|d| d.water.along_grain = -0.5),
            ),
            ("kit[0].weight", Box::new(|d| d.kit[0].weight = 1.5)),
            ("kit[1].species", Box::new(|d| d.kit.push(d.kit[0].clone()))),
        ];
        for (field, break_it) in cases {
            let mut def = good.clone();
            break_it(&mut def);
            let (named, reason) = def.invalid_field().unwrap_or_else(|| {
                panic!("breaking {field} was not refused");
            });
            assert_eq!(named, field, "{reason}");
        }
    }
}
