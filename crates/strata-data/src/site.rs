//! Site kinds: the places the world's bake finds for itself (ADR 0012), as
//! `sites/*.site.ron` files spell them, one kind a file.
//!
//! An authored settlement (ADR 0009) stands where its file puts it. A
//! procedural site is searched for: the bake scores the ground of a land
//! mass against a kind's wants and takes the best places subject to its
//! spacing. A kind is data so that tuning where hamlets go is an edit to a
//! file and a look at the map, not a change to the generator.
//!
//! A kind says what ground it wants, as hard limits (`ground`) and as
//! weighted preferences (`wants`, each the best of a few `terms`); how big
//! its pad is and how level; the spacing it keeps; how many a thousand
//! square kilometres of land holds; its rank, which orders the placement;
//! which way it faces; the class of road that will reach it and the group
//! of buildings that will stand on it (both data only until later steps);
//! and how its names are made.
//!
//! A set with no kinds places nothing, and the world is the one it was
//! before sites existed, to the bit.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

/// A dense handle to a frozen site kind, assigned by the freeze in sorted-id
/// order so the same asset set gives the same ids however the directory
/// lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SiteKindId(pub u32);

slotted_registry::impl_interned_id!(SiteKindId);

/// One kind of site, as a `.site.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteKindDef {
    /// The namespaced id, `strata:hamlet`, and the file's key.
    pub id: Namespaced,
    /// What a map legend calls the kind.
    pub name: String,
    /// Kinds are placed in rank order, lowest first, ties by id: a town
    /// before the hamlets that want to be near one.
    pub rank: u32,
    /// How many a thousand square kilometres of land holds at most; the
    /// spacing and the ground decide how many it gets.
    pub per_1000_km2: f64,
    /// The pad's radius, metres: the level ground the buildings stand on.
    pub footprint_m: f64,
    /// The band outside the pad over which the ground eases back to
    /// nature, metres.
    pub blend_m: f64,
    /// The pad's fall at its edge, rise over run: a crown that sheds water,
    /// nought for a dead-level pad.
    #[serde(default = "default_crown")]
    pub crown: f64,
    /// The deepest cut or fill the pad may make anywhere over its
    /// footprint, metres. A place that needs more was a bad place.
    pub max_cut_m: f64,
    /// The distances it keeps.
    pub spacing: SpacingDef,
    /// Hard limits on the ground.
    pub ground: GroundLimitsDef,
    /// What it would like: each want scores the best of its terms, and a
    /// place's score is the weighted mean of its wants.
    #[serde(default)]
    pub wants: Vec<WantDef>,
    /// Which way its front looks: `water`, `view` or `downhill`.
    pub facing: String,
    /// The class of road that reaches it: `service`, `track` or `trail`.
    /// Data only until the road network (ticket 0005, step 5).
    pub road: String,
    /// The group of buildings that stands on its pad. Data only until the
    /// buildings are baked onto pads.
    pub group: String,
    /// How its names are made.
    pub names: NamesDef,
}

fn default_crown() -> f64 {
    0.02
}

/// The distances a kind keeps, metres between centres (from an authored
/// settlement, from the edge of its extent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacingDef {
    /// From another site of its own kind.
    pub own_m: f64,
    /// From a site of any other kind; the larger of the two kinds' numbers
    /// holds between them.
    pub others_m: f64,
    /// From an authored settlement's extent: a kilometre unless a kind says
    /// otherwise, so nothing crowds a city's fitted ground.
    #[serde(default = "default_settlements_m")]
    pub settlements_m: f64,
}

fn default_settlements_m() -> f64 {
    1_000.0
}

/// Hard limits on a site's ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundLimitsDef {
    /// The steepest the natural ground may fall across the footprint, rise
    /// over run.
    pub slope_max: f64,
    /// The pad's height, metres above the sea.
    pub height_m: MetresDef,
    /// Weights by biome, `0..=1`; a biome not listed takes `biome_default`.
    /// Nought refuses the biome.
    #[serde(default)]
    pub biomes: Vec<WeightDef>,
    /// The weight of a biome the list does not name.
    #[serde(default = "one")]
    pub biome_default: f64,
    /// Weights by province, as the biomes'.
    #[serde(default)]
    pub provinces: Vec<WeightDef>,
    /// The weight of a province the list does not name.
    #[serde(default = "one")]
    pub province_default: f64,
}

fn one() -> f64 {
    1.0
}

/// A range of metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetresDef {
    /// The least.
    pub min: f64,
    /// The most.
    pub max: f64,
}

/// A registry entry and its weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightDef {
    /// The biome or province.
    pub id: Namespaced,
    /// Its weight, `0..=1`.
    pub weight: f64,
}

/// One preference: the best of its terms, weighted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WantDef {
    /// The terms; the want scores the best of them, so "by a mouth, a
    /// confluence or a bay" is one want of three terms.
    pub any: Vec<TermDef>,
    /// Its weight in the mean.
    #[serde(default = "one")]
    pub weight: f64,
    /// Whether a place that scores nought on it is refused outright: a hard
    /// limit on the distance to water, a view or a pass.
    #[serde(default)]
    pub required: bool,
}

/// One term of a want: a distance to a feature, or a measure of the ground.
///
/// A distance scores one between `beyond_m` and `within_m` (either may be
/// left out) and falls to nought over `fade_m` outside them. A measure
/// scores nought at `at_least` and one at `full`, linearly between.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermDef {
    /// What it measures: a distance to `sea`, `river`, `lake`,
    /// `confluence`, `mouth`, `ford`, `fall`, `gorge`, `kind` (sites of
    /// another kind) or `settlement` (an authored one's extent); or a
    /// measure, `summit` (metres over the ground round it), `view` (the
    /// share of the horizon open), `pass` (metres of a saddle) or `cove`
    /// (the share of the sea's horizon closed by land).
    pub feature: String,
    /// The least size of the feature that counts: a river's, a ford's, a
    /// mouth's or a confluence's smaller arm's width, a fall's drop, a
    /// gorge's depth, a lake's side, metres.
    #[serde(default)]
    pub size_m: f64,
    /// For `kind`: which kind, a namespaced id. It must be placed before
    /// this one. Empty for every other feature.
    #[serde(default)]
    pub kind: String,
    /// A distance scores one within this; unbounded when left out.
    #[serde(default = "unbounded")]
    pub within_m: f64,
    /// And beyond this; nought when left out.
    #[serde(default)]
    pub beyond_m: f64,
    /// Over how far outside them it falls to nought, metres.
    #[serde(default = "default_fade")]
    pub fade_m: f64,
    /// A measure scores nought at this.
    #[serde(default)]
    pub at_least: f64,
    /// And one at this; a measure needs it over `at_least`.
    #[serde(default)]
    pub full: f64,
}

fn unbounded() -> f64 {
    f64::INFINITY
}

fn default_fade() -> f64 {
    500.0
}

/// How far past a pad's edge its ground is laid and kept clear, metres:
/// fully developed to here, then fading out over [`PAD_SKIRT_FADE_M`]. Here
/// so a kind's band can be held wider than the two.
pub const PAD_SKIRT_M: f64 = 6.0;
/// The fade of a pad's developed skirt, metres.
pub const PAD_SKIRT_FADE_M: f64 = 8.0;

/// How a kind's names are made: a root of syllables, set in a pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamesDef {
    /// A syllable's first consonants.
    pub onsets: Vec<String>,
    /// Its vowels.
    pub vowels: Vec<String>,
    /// What may close the root's last syllable; an empty string leaves it
    /// open.
    pub codas: Vec<String>,
    /// How many syllables a root has, `[least, most]`.
    pub syllables: [u32; 2],
    /// Where the root goes: `{root}` is replaced by it, capitalised.
    pub patterns: Vec<String>,
    /// Patterns for a site by a feature it stands by, which take the place
    /// of `patterns` there: a landmark beside a fall is "{root} Falls".
    #[serde(default)]
    pub by_feature: Vec<FeatureNamesDef>,
}

impl NamesDef {
    /// The first field of the rules that could not make a name, with its path
    /// from the file's `names` and the reason. Shared by every file that names
    /// things with the site namer.
    pub fn check(&self) -> Result<(), (String, String)> {
        let n = self;
        let fail = |field: &str, reason: &str| Err((field.to_owned(), reason.to_owned()));
        let clean = |parts: &[String], empty_ok: bool| {
            !parts.is_empty()
                && parts.iter().all(|p| {
                    (empty_ok || !p.is_empty())
                        && p.len() <= 4
                        && p.chars().all(|c| c.is_ascii_lowercase())
                })
        };
        if !clean(&n.onsets, false) {
            return fail(
                "names.onsets",
                "needs at least one, each one to four lowercase letters",
            );
        }
        if !clean(&n.vowels, false) {
            return fail(
                "names.vowels",
                "needs at least one, each one to four lowercase letters",
            );
        }
        if !clean(&n.codas, true) {
            return fail(
                "names.codas",
                "needs at least one, each up to four lowercase letters",
            );
        }
        if !(1..=4).contains(&n.syllables[0])
            || n.syllables[1] < n.syllables[0]
            || n.syllables[1] > 4
        {
            return fail("names.syllables", "must be [least, most] within 1..=4");
        }
        // A root is kept at four to nine letters; rules whose every root is
        // shorter or longer would have the bake draw forever.
        let len = |parts: &[String], most: bool| {
            let lens = parts.iter().map(String::len);
            u32::try_from(if most { lens.max() } else { lens.min() }.unwrap_or(0))
                .unwrap_or(u32::MAX)
        };
        let shortest =
            n.syllables[0] * (len(&n.onsets, false) + len(&n.vowels, false)) + len(&n.codas, false);
        let longest =
            n.syllables[1] * (len(&n.onsets, true) + len(&n.vowels, true)) + len(&n.codas, true);
        if shortest > 9 || longest < 4 {
            return fail(
                "names",
                "can make no root of four to nine letters from its syllables",
            );
        }
        let patterns_ok = |patterns: &[String]| {
            !patterns.is_empty()
                && patterns
                    .iter()
                    .all(|p| p.matches("{root}").count() == 1 && p.len() <= 32)
        };
        if !patterns_ok(&n.patterns) {
            return fail(
                "names.patterns",
                "needs at least one, each holding `{root}` once in 32 characters",
            );
        }
        for (i, entry) in n.by_feature.iter().enumerate() {
            if SiteFeature::parse(&entry.feature).is_none() {
                return fail(
                    &format!("names.by_feature[{i}].feature"),
                    "is not a feature a term names",
                );
            }
            if !patterns_ok(&entry.patterns) {
                return fail(
                    &format!("names.by_feature[{i}].patterns"),
                    "needs at least one, each holding `{root}` once in 32 characters",
                );
            }
        }
        Ok(())
    }
}

/// The patterns for a site by one feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureNamesDef {
    /// The feature, as a term names it.
    pub feature: String,
    /// Its patterns.
    pub patterns: Vec<String>,
}

/// What a term measures, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SiteFeature {
    /// The coast.
    Sea,
    /// A river's channel.
    River,
    /// A lake's shore.
    Lake,
    /// Where two rivers meet.
    Confluence,
    /// Where a river meets the sea.
    Mouth,
    /// A shallow riffle a walker crosses.
    Ford,
    /// A fall's lip.
    Fall,
    /// A river cut deep below its ground.
    Gorge,
    /// Sites of another kind.
    Kind,
    /// An authored settlement's extent.
    Settlement,
    /// Metres over the ground round the place.
    Summit,
    /// The share of the horizon open from it.
    View,
    /// Metres of a saddle: ground rising along one line and falling across.
    Pass,
    /// The share of the sea's horizon land closes.
    Cove,
}

impl SiteFeature {
    /// Every feature, in order.
    pub const ALL: [Self; 14] = [
        Self::Sea,
        Self::River,
        Self::Lake,
        Self::Confluence,
        Self::Mouth,
        Self::Ford,
        Self::Fall,
        Self::Gorge,
        Self::Kind,
        Self::Settlement,
        Self::Summit,
        Self::View,
        Self::Pass,
        Self::Cove,
    ];

    /// Its name in a file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sea => "sea",
            Self::River => "river",
            Self::Lake => "lake",
            Self::Confluence => "confluence",
            Self::Mouth => "mouth",
            Self::Ford => "ford",
            Self::Fall => "fall",
            Self::Gorge => "gorge",
            Self::Kind => "kind",
            Self::Settlement => "settlement",
            Self::Summit => "summit",
            Self::View => "view",
            Self::Pass => "pass",
            Self::Cove => "cove",
        }
    }

    /// Whether it is a measure of the ground rather than a distance.
    #[must_use]
    pub const fn is_measure(self) -> bool {
        matches!(self, Self::Summit | Self::View | Self::Pass | Self::Cove)
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == text)
    }
}

/// Which way a site's front looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    /// Towards the nearest water: the sea, a river or a lake.
    Water,
    /// Towards the most open part of the horizon.
    View,
    /// Down the ground's fall over the footprint.
    Downhill,
}

/// The class of road that reaches a site (ticket 0005, decided 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoadClass {
    /// A paved service road: wide, gentle, bridges and cuttings.
    Service,
    /// A gravel track.
    Track,
    /// A foot trail that follows the ground and fords streams.
    Trail,
}

/// A term with its strings parsed and its kind resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteTerm {
    /// What it measures.
    pub feature: SiteFeature,
    /// The least size that counts.
    pub size_m: f64,
    /// For [`SiteFeature::Kind`], which.
    pub kind: Option<SiteKindId>,
    /// Scores one within this distance.
    pub within_m: f64,
    /// And beyond this one.
    pub beyond_m: f64,
    /// Falls to nought over this outside them.
    pub fade_m: f64,
    /// A measure's nought.
    pub at_least: f64,
    /// A measure's one.
    pub full: f64,
}

impl SiteTerm {
    /// The term's score for a distance, `0..=1`; an absent feature is an
    /// infinite one.
    #[must_use]
    pub fn of_distance(&self, d: f64) -> f64 {
        if d < self.beyond_m {
            (1.0 - (self.beyond_m - d) / self.fade_m).max(0.0)
        } else if d > self.within_m {
            (1.0 - (d - self.within_m) / self.fade_m).max(0.0)
        } else {
            1.0
        }
    }

    /// The term's score for a measure, `0..=1`.
    #[must_use]
    pub fn of_measure(&self, m: f64) -> f64 {
        ((m - self.at_least) / (self.full - self.at_least)).clamp(0.0, 1.0)
    }

    /// How far out a distance still scores: past this the feature counts as
    /// absent. Infinite for a term that only wants to be beyond.
    #[must_use]
    pub fn reach_m(&self) -> f64 {
        self.within_m + self.fade_m
    }
}

/// A want, parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteWant {
    /// Its terms.
    pub any: Vec<SiteTerm>,
    /// Its weight.
    pub weight: f64,
    /// Whether nought refuses the place.
    pub required: bool,
}

/// A frozen kind with its strings parsed and its names resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteKindTraits {
    /// Where it comes in the placement: kinds by rank, ties by id.
    pub order: u32,
    /// Its wants.
    pub wants: Vec<SiteWant>,
    /// Weights by biome id, dense.
    pub biomes: Vec<f64>,
    /// Weights by province id, dense; empty in a world without geology.
    pub provinces: Vec<f64>,
    /// Which way it faces.
    pub facing: Facing,
    /// The road that reaches it.
    pub road: RoadClass,
    /// Patterns for a site by a feature, parsed.
    pub by_feature: Vec<(SiteFeature, Vec<String>)>,
}

impl SiteKindDef {
    /// The first field whose number or string is unusable, with its path in
    /// the file and the reason. Names are resolved by the freeze.
    #[must_use]
    pub fn invalid_field(&self) -> Option<(String, String)> {
        self.check().err()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every bound of the schema in one flat list, in the file's order"
    )]
    fn check(&self) -> Result<(), (String, String)> {
        let fail = |field: &str, reason: &str| Err((field.to_owned(), reason.to_owned()));
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let non_negative = |v: f64| v.is_finite() && v >= 0.0;
        if self.name.trim().is_empty() {
            return fail("name", "must not be empty");
        }
        if !non_negative(self.per_1000_km2) || self.per_1000_km2 > 1_000.0 {
            return fail("per_1000_km2", "must be in 0..=1000");
        }
        if !positive(self.footprint_m) || self.footprint_m > 250.0 {
            return fail("footprint_m", "must be in (0, 250]");
        }
        if !positive(self.blend_m) || self.blend_m > 250.0 {
            return fail("blend_m", "must be in (0, 250]");
        }
        // The developed skirt fades out inside the band; a narrower band
        // would end the gravel and the bare ground in a hard line.
        if self.blend_m < PAD_SKIRT_M + PAD_SKIRT_FADE_M {
            return fail(
                "blend_m",
                "must be at least the developed skirt and its fade (14 m)",
            );
        }
        if !non_negative(self.crown) || self.crown > 0.1 {
            return fail("crown", "must be in 0..=0.1");
        }
        if !positive(self.max_cut_m) || self.max_cut_m > 30.0 {
            return fail("max_cut_m", "must be in (0, 30]");
        }
        let s = &self.spacing;
        let outer = self.footprint_m + self.blend_m;
        if !(positive(s.own_m) && s.own_m >= 2.0 * outer) {
            return fail(
                "spacing.own_m",
                "must be at least twice the footprint and the blend",
            );
        }
        if !(positive(s.others_m) && s.others_m >= 2.0 * outer) {
            return fail(
                "spacing.others_m",
                "must be at least twice the footprint and the blend",
            );
        }
        if !non_negative(s.settlements_m) {
            return fail("spacing.settlements_m", "must be finite and not negative");
        }
        let g = &self.ground;
        if !(positive(g.slope_max) && g.slope_max <= 1.0) {
            return fail("ground.slope_max", "must be in (0, 1]");
        }
        if !(g.height_m.min.is_finite()
            && g.height_m.max.is_finite()
            && g.height_m.min >= 0.0
            && g.height_m.min < g.height_m.max)
        {
            return fail(
                "ground.height_m",
                "needs 0 <= min < max, both finite: a pad below the sea is no pad",
            );
        }
        let weight = |w: f64| w.is_finite() && (0.0..=1.0).contains(&w);
        if !weight(g.biome_default) {
            return fail("ground.biome_default", "must be in 0..=1");
        }
        if !weight(g.province_default) {
            return fail("ground.province_default", "must be in 0..=1");
        }
        for (i, entry) in g.biomes.iter().enumerate() {
            if !weight(entry.weight) {
                return fail(&format!("ground.biomes[{i}].weight"), "must be in 0..=1");
            }
        }
        for (i, entry) in g.provinces.iter().enumerate() {
            if !weight(entry.weight) {
                return fail(&format!("ground.provinces[{i}].weight"), "must be in 0..=1");
            }
        }
        for (i, want) in self.wants.iter().enumerate() {
            if !(positive(want.weight) && want.weight <= 100.0) {
                return fail(&format!("wants[{i}].weight"), "must be in (0, 100]");
            }
            if want.any.is_empty() {
                return fail(&format!("wants[{i}].any"), "needs at least one term");
            }
            for (k, term) in want.any.iter().enumerate() {
                let at = |field: &str| format!("wants[{i}].any[{k}].{field}");
                let Some(feature) = SiteFeature::parse(&term.feature) else {
                    return Err((
                        at("feature"),
                        format!(
                            "`{}` is not one of {}",
                            term.feature,
                            SiteFeature::ALL.map(SiteFeature::name).join(", ")
                        ),
                    ));
                };
                if !non_negative(term.size_m) {
                    return Err((at("size_m"), "must be finite and not negative".to_owned()));
                }
                if (feature == SiteFeature::Kind) == term.kind.is_empty() {
                    return Err((
                        at("kind"),
                        "names a kind exactly when the feature is `kind`".to_owned(),
                    ));
                }
                let distances = term.within_m.is_finite() || term.beyond_m > 0.0;
                if feature.is_measure() {
                    if !(term.at_least.is_finite()
                        && term.full.is_finite()
                        && term.full > term.at_least)
                    {
                        return Err((
                            at("full"),
                            "a measure needs `at_least` and a finite `full` over it".to_owned(),
                        ));
                    }
                    if distances {
                        return Err((
                            at("within_m"),
                            "a measure takes `at_least` and `full`, not distances".to_owned(),
                        ));
                    }
                } else {
                    if !distances {
                        return Err((
                            at("within_m"),
                            "a distance needs `within_m`, `beyond_m` or both".to_owned(),
                        ));
                    }
                    let bounded = |v: f64| !v.is_nan() && (0.0..=50_000.0).contains(&v);
                    if !(bounded(term.beyond_m)
                        && (term.within_m.is_infinite() || bounded(term.within_m)))
                    {
                        return Err((
                            at("within_m"),
                            "distances must be in 0..=50000 metres".to_owned(),
                        ));
                    }
                    if term.beyond_m > term.within_m {
                        return Err((at("beyond_m"), "must not be over `within_m`".to_owned()));
                    }
                    if !(positive(term.fade_m) && term.fade_m <= 20_000.0) {
                        return Err((at("fade_m"), "must be in (0, 20000]".to_owned()));
                    }
                    if term.at_least != 0.0 || term.full != 0.0 {
                        return Err((
                            at("at_least"),
                            "a distance takes `within_m` and `beyond_m`, not a measure's bounds"
                                .to_owned(),
                        ));
                    }
                }
            }
        }
        if Facing::parse(&self.facing).is_none() {
            return fail("facing", "must be `water`, `view` or `downhill`");
        }
        if RoadClass::parse(&self.road).is_none() {
            return fail("road", "must be `service`, `track` or `trail`");
        }
        if self.group.trim().is_empty() {
            return fail("group", "must name a building group");
        }
        self.names.check()?;
        Ok(())
    }

    /// The term's parsed form; `kind` resolved by the caller.
    pub(crate) fn term(term: &TermDef, kind: Option<SiteKindId>) -> SiteTerm {
        let feature = SiteFeature::parse(&term.feature).expect("checked by invalid_field");
        SiteTerm {
            feature,
            size_m: term.size_m,
            kind,
            within_m: term.within_m,
            beyond_m: term.beyond_m,
            fade_m: term.fade_m,
            at_least: term.at_least,
            full: term.full,
        }
    }

    /// The parsed facing.
    #[must_use]
    pub fn parsed_facing(&self) -> Facing {
        Facing::parse(&self.facing).expect("checked by invalid_field")
    }

    /// The parsed road class.
    #[must_use]
    pub fn parsed_road(&self) -> RoadClass {
        RoadClass::parse(&self.road).expect("checked by invalid_field")
    }

    /// The parsed feature patterns.
    pub(crate) fn parsed_by_feature(&self) -> Vec<(SiteFeature, Vec<String>)> {
        self.names
            .by_feature
            .iter()
            .map(|entry| {
                (
                    SiteFeature::parse(&entry.feature).expect("checked by invalid_field"),
                    entry.patterns.clone(),
                )
            })
            .collect()
    }
}

impl Facing {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "water" => Some(Self::Water),
            "view" => Some(Self::View),
            "downhill" => Some(Self::Downhill),
            _ => None,
        }
    }
}

impl RoadClass {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "service" => Some(Self::Service),
            "track" => Some(Self::Track),
            "trail" => Some(Self::Trail),
            _ => None,
        }
    }
}
