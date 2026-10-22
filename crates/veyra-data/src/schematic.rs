//! Schematics: what a bench can make, from what, and how what went in
//! becomes the quality of what comes out (ticket 0007, step 4;
//! `docs/design/crafting.md`, 5.1 to 5.3), as `schematics/*.schematic.ron`
//! spells them, one a file, with the session's dials
//! (`crafting/session.ron`) beside them.
//!
//! A schematic names the item it makes, its slots and, for every quality
//! line of that item, the weighted inputs: a slot and one of the stats of
//! what is in it ([`Stat`]), read straight or as `Light`, which is the top
//! of the scale less the figure, for a line that wants less of something.
//! A slot takes a resource by family, class or type with a number of units,
//! or a component or a commodity item with a count ([`Takes`]). A component
//! exposes its lines on the same scale as a lot's attributes, so a
//! component in a slot is read exactly as a resource is.
//!
//! Everything a weight could get wrong is refused here, at load, with the
//! file's name: a line may weigh only an attribute every class its slot
//! takes carries, or a line its slot's component defines; nothing weighs a
//! commodity; every line of the product is given once and nothing else is;
//! a line's weights sum to one. The sim then works in whole per-mille
//! ([`Schematic::weights`]), since 0.3 of 605 is 181.5 and a float sum
//! rounds either way.
//!
//! The session's dials ([`SessionRules`]) are the share of its ceiling a
//! line is assembled at, the experimentation points by complexity, what
//! each result of a roll adds and the odds of each by hand. What a roll is
//! and what a session does with it is the sim's
//! (`veyra_sim::crafting`).
//!
//! The factory's dials ([`FactoryRules`], `crafting/factory.ron`; step 5,
//! crafting.md 5.4) are how far under a prototype a blueprint may be
//! recorded, the runs it starts with by complexity, and how long a
//! fabricator takes a unit, worked and not. What a blueprint is and what a
//! run does is the sim's again (`veyra_sim::blueprint`, `::factory`). A
//! data set without the file records no blueprints.
//!
//! The box that teaches a schematic, the licence it needs and the fixture
//! it is made at are in the format and read by nothing yet: until the
//! Weaponsmith tree exists every character knows every schematic.
//!
//! None of it is world state: it joins no world version and no handshake.
//! A data set without the directory has no schematics, and that is legal.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::item::{ItemCatalogue, ItemDefinition, ItemKind, QUALITY_MAX, Record};
use crate::progression::SkillGraph;
use crate::resource::{Attribute, ClassDef, ResourceSet};

/// The directory of `.schematic.ron` files, relative to the asset root.
/// Absent for a data set in which nothing is made.
pub const SCHEMATICS_DIR: &str = "schematics";
/// The suffix a schematic file must carry to be read.
pub const SCHEMATIC_SUFFIX: &str = ".schematic.ron";
/// The session's dials, relative to the asset root. Absent, a session has
/// no points for any complexity, so the data set may hold no schematic.
pub const SESSION_FILE: &str = "crafting/session.ron";
/// The factory's dials, relative to the asset root. Absent, nothing is
/// recorded as a blueprint and no fabricator runs.
pub const FACTORY_FILE: &str = "crafting/factory.ron";

/// The rows of odds a hand has: the novice's, the journeyman's and the
/// master's. Not a tunable: the screen draws one row and the rules move
/// between neighbours, so the file gives exactly these.
pub const ODDS_ROWS: usize = 3;

/// A whole, in the per-mille every weight and share is worked in: the top
/// of the quality scale, so a weight of one of a figure is the figure.
const PER_MILLE: u32 = QUALITY_MAX as u32;

/// A whole of a row of odds, percent.
const PERCENT: u32 = 100;

/// What a slot takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Takes {
    /// A lot of any type of a family, `strata:mineral`.
    Family(Namespaced),
    /// A lot of any type of a class, `strata:reactive`.
    Class(Namespaced),
    /// A lot of one type, `strata:light_alloy_ore`.
    Type(Namespaced),
    /// A component item, whose lines the schematic may weigh.
    Component(Namespaced),
    /// A commodity item, which is flat and weighs nothing.
    Commodity(Namespaced),
}

impl Takes {
    /// Whether the slot takes a lot, counted in units, and not an item,
    /// counted in pieces.
    #[must_use]
    pub const fn is_resource(&self) -> bool {
        matches!(self, Self::Family(_) | Self::Class(_) | Self::Type(_))
    }
}

/// One slot of a schematic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    /// The slot's stable key (`casing`), which a weight names.
    pub id: String,
    /// What the screen calls it.
    pub name: String,
    /// What goes in it.
    pub takes: Takes,
    /// How many units of a lot it takes. Nought on an item's slot.
    #[serde(default)]
    pub units: u32,
    /// How many of an item it takes. Nought on a lot's slot.
    #[serde(default)]
    pub count: u32,
}

impl SlotDef {
    /// How much of what it takes the slot uses up: units of a lot, or
    /// pieces of an item. Either way it is the count of a stack.
    #[must_use]
    pub const fn amount(&self) -> u32 {
        if self.takes.is_resource() {
            self.units
        } else {
            self.count
        }
    }
}

/// One figure of what is in a slot, as a weight reads it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Stat {
    /// A lot's attribute, read straight.
    Attribute(Attribute),
    /// A lot's attribute read the other way, the top of the scale less the
    /// figure: for a line that is better the less of it there is, as a
    /// casing's density is to handling.
    Light(Attribute),
    /// A component's quality line, by its id.
    Line(String),
}

impl Stat {
    /// The line of a record the stat is read from: an attribute's key or
    /// the component's line id.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Attribute(attribute) | Self::Light(attribute) => attribute.key(),
            Self::Line(line) => line,
        }
    }

    /// Whether less of the figure is better for the line that weighs it.
    #[must_use]
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light(_))
    }

    /// The figure of `record` the stat names, as its card writes it;
    /// `None` when the record has no such line.
    #[must_use]
    pub fn figure(&self, record: &Record) -> Option<u16> {
        record.lines.get(self.key()).copied()
    }

    /// `figure` as a weight counts it: itself, or the top of the scale less
    /// it for [`Stat::Light`].
    #[must_use]
    pub const fn weighed(&self, figure: u16) -> u16 {
        if self.is_light() {
            QUALITY_MAX.saturating_sub(figure)
        } else {
            figure
        }
    }

    /// What a weight on this stat reads of `record`: [`Self::figure`]
    /// through [`Self::weighed`].
    #[must_use]
    pub fn read(&self, record: &Record) -> Option<u16> {
        self.figure(record).map(|figure| self.weighed(figure))
    }
}

/// One weighted input of a line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightDef {
    /// The slot, by its id.
    pub slot: String,
    /// Which figure of what is in it.
    pub stat: Stat,
    /// Its share of the line, a whole number of thousandths; a line's
    /// shares sum to one.
    pub weight: f32,
}

/// How one quality line of the product comes of what is in the slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineRecipe {
    /// The product's line, by its id.
    pub line: String,
    /// The weighted inputs.
    pub from: Vec<WeightDef>,
}

/// One schematic, as a `.schematic.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicDef {
    /// The namespaced id, `veyra:schematic.cell_core`.
    pub id: Namespaced,
    /// The item it makes: a component, a module, a weapon or a kit.
    pub makes: Namespaced,
    /// How involved it is, from 1: what sets a session's points.
    pub complexity: u8,
    /// The skill box that teaches it: only a character whose sheet has the
    /// box opens a session of it. Checked against the skill graph at load.
    #[serde(default)]
    pub profession: Option<String>,
    /// The maker whose licence it needs. Read by nothing yet.
    #[serde(default)]
    pub licence: Option<Namespaced>,
    /// The workshop fixture it is made at. Read by nothing yet.
    #[serde(default)]
    pub fixture: Option<String>,
    /// What it takes, in the order a screen lists the slots.
    pub slots: Vec<SlotDef>,
    /// A recipe for each line of the product, in the product's order.
    pub lines: Vec<LineRecipe>,
}

/// A schematic that passed every check, with its weights resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Schematic {
    /// The schematic as its file spells it.
    pub def: SchematicDef,
    /// For each line of the product, in the product's order: the slot by
    /// its place in `def.slots`, the stat, and the weight in per-mille. A
    /// line's weights sum to a thousand.
    pub weights: Vec<Vec<(usize, Stat, u16)>>,
    /// Whether the product is a weapon or a module: a part's line goes next
    /// into the first of these, so a tool or a bike made of the same part
    /// does not take the place of what the part went into before.
    pub weapon_side: bool,
}

impl Schematic {
    /// The slot `id` and its place among the slots.
    #[must_use]
    pub fn slot(&self, id: &str) -> Option<(usize, &SlotDef)> {
        self.def
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.id == id)
    }

    /// Every weight any line puts on the slot at `slot`, as the stat and
    /// its per-mille: what a candidate for the slot is scored by.
    pub fn weights_on(&self, slot: usize) -> impl Iterator<Item = (&Stat, u16)> {
        self.weights
            .iter()
            .flatten()
            .filter(move |(at, ..)| *at == slot)
            .map(|(_, stat, weight)| (stat, *weight))
    }

    /// The stats the slot at `slot` is weighed by, each once, in the order
    /// the lines first name them: the ones a screen lights.
    #[must_use]
    pub fn stats_on(&self, slot: usize) -> Vec<&Stat> {
        let mut stats: Vec<&Stat> = Vec::new();
        for (stat, _) in self.weights_on(slot) {
            if !stats.contains(&stat) {
                stats.push(stat);
            }
        }
        stats
    }
}

/// What a roll came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Outcome {
    /// The best a roll gives.
    Exceptional,
    /// What a steady hand most often gets.
    Good,
    /// A middling lift.
    Fair,
    /// Barely a lift.
    Poor,
    /// The line goes backwards.
    Setback,
}

impl Outcome {
    /// Every outcome, best first: the order of a row of odds.
    pub const ALL: [Self; 5] = [
        Self::Exceptional,
        Self::Good,
        Self::Fair,
        Self::Poor,
        Self::Setback,
    ];

    /// Its place in [`Self::ALL`] and in a row of odds.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The outcome's stable key (`exceptional`), which a string table keys
    /// its word by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Exceptional => "exceptional",
            Self::Good => "good",
            Self::Fair => "fair",
            Self::Poor => "poor",
            Self::Setback => "setback",
        }
    }
}

/// How a point is spent: the crafter's own choice of risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Hand {
    /// For getting there: seldom the best, never backwards.
    Steady,
    /// For reaching a ceiling in fewer points, at the risk of a setback.
    Bold,
}

impl Hand {
    /// The hand's stable key (`steady`).
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Steady => "steady",
            Self::Bold => "bold",
        }
    }
}

/// The odds of each hand, as the file spells them: rows as lists, since a
/// fixed array is a tuple to RON and would be written in round brackets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OddsDef {
    steady: Vec<Vec<u8>>,
    bold: Vec<Vec<u8>>,
}

/// What workable stock does, as the file spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkabilityDef {
    row_up_from: u16,
}

/// `crafting/session.ron`, as the file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDef {
    version: u32,
    assembly: f32,
    points: Vec<u8>,
    results: Vec<(Outcome, f32)>,
    odds: OddsDef,
    base_row: usize,
    workability: WorkabilityDef,
}

/// `value` as a whole number of thousandths, or `None` when it is not one:
/// the sim sums weights and shares in integers, and a share it could not
/// hold exactly would round a figure one way here and another there.
fn per_mille(value: f32) -> Option<i32> {
    let scaled = f64::from(value) * f64::from(PER_MILLE);
    let whole = scaled.round();
    // An `f32` is good to seven digits, so 0.15 is a thousandth of 150 to
    // well inside this.
    if !scaled.is_finite() || (scaled - whole).abs() > 1e-3 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a share of a whole, a few thousand at most; the callers hold it to their range"
    )]
    let whole = whole.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Some(whole)
}

/// The session's dials, checked and in the per-mille the sim works in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRules {
    assembly_pm: u16,
    /// By complexity, from 1.
    points: Vec<u8>,
    /// By [`Outcome::index`].
    lifts_pm: [i16; 5],
    steady: [[u8; 5]; ODDS_ROWS],
    bold: [[u8; 5]; ODDS_ROWS],
    base_row: usize,
    row_up_from: u16,
}

impl Default for SessionRules {
    /// A data set without the file: a line is assembled at its ceiling and
    /// no complexity has a point, which is why such a data set may hold no
    /// schematic. A roll, were one made, is a Fair that lifts nothing.
    fn default() -> Self {
        let flat = [[0, 0, 100, 0, 0]; ODDS_ROWS];
        Self {
            assembly_pm: QUALITY_MAX,
            points: Vec::new(),
            lifts_pm: [0; 5],
            steady: flat,
            bold: flat,
            base_row: 0,
            row_up_from: QUALITY_MAX,
        }
    }
}

impl SessionRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse and check the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, a share that is no share or
    /// not a whole number of thousandths, results out of their order, a
    /// row of odds that does not sum to a hundred, or a row that is not
    /// there.
    pub fn parse(text: &str) -> Result<Self, SchematicError> {
        let def: SessionDef =
            ron::from_str(text).map_err(|error| SchematicError::Ron(error.to_string()))?;
        if def.version != Self::VERSION {
            return Err(SchematicError::Version {
                found: def.version,
                expected: Self::VERSION,
            });
        }
        let refused = |reason: String| SchematicError::Session(reason);
        let assembly = per_mille(def.assembly)
            .and_then(|share| u16::try_from(share).ok())
            .filter(|share| (1..=QUALITY_MAX).contains(share))
            .ok_or_else(|| {
                refused(format!(
                    "assembly is {}, a share over nought and up to one, in thousandths",
                    def.assembly
                ))
            })?;
        if def.points.is_empty() {
            return Err(refused(
                "points is empty: it gives the points of complexity 1, then 2, and on".to_owned(),
            ));
        }
        let named: Vec<Outcome> = def.results.iter().map(|(outcome, _)| *outcome).collect();
        if named != Outcome::ALL {
            return Err(refused(format!(
                "results are {named:?}, not the five in their order, {:?}",
                Outcome::ALL
            )));
        }
        let mut lifts_pm = [0_i16; 5];
        for (outcome, lift) in &def.results {
            lifts_pm[outcome.index()] = per_mille(*lift)
                .and_then(|share| i16::try_from(share).ok())
                .filter(|share| share.unsigned_abs() <= QUALITY_MAX)
                .ok_or_else(|| {
                    refused(format!(
                        "{outcome:?} lifts by {lift}, a share of the ceiling between minus one and one, in thousandths"
                    ))
                })?;
        }
        let rows =
            |hand: Hand, given: &[Vec<u8>]| -> Result<[[u8; 5]; ODDS_ROWS], SchematicError> {
                if given.len() != ODDS_ROWS {
                    return Err(refused(format!(
                        "{} has {} rows of odds, not {ODDS_ROWS}",
                        hand.key(),
                        given.len()
                    )));
                }
                let mut rows = [[0_u8; 5]; ODDS_ROWS];
                for (index, (row, given)) in rows.iter_mut().zip(given).enumerate() {
                    *row = given.as_slice().try_into().map_err(|_| {
                        refused(format!(
                            "row {index} of {} has {} odds, not one for each of the five results",
                            hand.key(),
                            given.len()
                        ))
                    })?;
                    let sum: u32 = row.iter().map(|odds| u32::from(*odds)).sum();
                    if sum != PERCENT {
                        return Err(refused(format!(
                            "row {index} of {} sums to {sum}, not {PERCENT}",
                            hand.key()
                        )));
                    }
                }
                Ok(rows)
            };
        let steady = rows(Hand::Steady, &def.odds.steady)?;
        let bold = rows(Hand::Bold, &def.odds.bold)?;
        if def.base_row >= ODDS_ROWS {
            return Err(refused(format!(
                "base_row is {}, and a hand has rows 0 to {}",
                def.base_row,
                ODDS_ROWS - 1
            )));
        }
        if def.workability.row_up_from > QUALITY_MAX {
            return Err(refused(format!(
                "workability.row_up_from is {}, over {QUALITY_MAX}",
                def.workability.row_up_from
            )));
        }
        Ok(Self {
            assembly_pm: assembly,
            points: def.points,
            lifts_pm,
            steady,
            bold,
            base_row: def.base_row,
            row_up_from: def.workability.row_up_from,
        })
    }

    /// The share of its ceiling a line is assembled at, per-mille.
    #[must_use]
    pub const fn assembly_pm(&self) -> u16 {
        self.assembly_pm
    }

    /// The points a session of a schematic of `complexity` has. Nought for
    /// a complexity the dials do not reach, which no loaded schematic has.
    #[must_use]
    pub fn points(&self, complexity: u8) -> u8 {
        usize::from(complexity)
            .checked_sub(1)
            .and_then(|index| self.points.get(index))
            .copied()
            .unwrap_or(0)
    }

    /// The most involved a schematic may be: the last complexity the dials
    /// give points for.
    #[must_use]
    pub fn max_complexity(&self) -> usize {
        self.points.len()
    }

    /// What `outcome` adds to a line's share of its ceiling, per-mille;
    /// under nought for a setback.
    #[must_use]
    pub const fn lift_pm(&self, outcome: Outcome) -> i16 {
        self.lifts_pm[outcome.index()]
    }

    /// The odds of each outcome for `hand` on `row`, percent, in
    /// [`Outcome::ALL`]'s order. A row past the last reads the last.
    #[must_use]
    pub fn odds(&self, hand: Hand, row: usize) -> [u8; 5] {
        let rows = match hand {
            Hand::Steady => &self.steady,
            Hand::Bold => &self.bold,
        };
        rows[row.min(ODDS_ROWS - 1)]
    }

    /// The row everyone rolls on until a crafter's boxes say otherwise.
    #[must_use]
    pub const fn base_row(&self) -> usize {
        self.base_row
    }

    /// The average Workability at which the stock moves the odds a row
    /// upward.
    #[must_use]
    pub const fn row_up_from(&self) -> u16 {
        self.row_up_from
    }

    /// The row a session rolls on when the stock in its slots averages
    /// `workability`: the base row, and the next one up for workable stock.
    /// `None` is stock none of which carries the attribute, which moves
    /// nothing.
    #[must_use]
    pub fn row(&self, workability: Option<u16>) -> usize {
        let up = usize::from(workability.is_some_and(|mean| mean >= self.row_up_from));
        (self.base_row + up).min(ODDS_ROWS - 1)
    }
}

/// How far under its prototype a blueprint may be recorded, as the file
/// spells it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DerateDef {
    from: f32,
    to: f32,
    step: f32,
}

/// `crafting/factory.ron`, as the file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FactoryDef {
    version: u32,
    blueprint: Namespaced,
    derate: DerateDef,
    runs: Vec<u32>,
    seconds_per_point: f32,
    unattended: f32,
}

/// The factory's dials, checked, in the per-mille and the ticks the sim
/// works in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryRules {
    blueprint: Namespaced,
    /// The least and the most a blueprint is recorded at and the step
    /// between, per-mille of the prototype.
    derate: (u16, u16, u16),
    /// The runs a blueprint starts with, by complexity, from 1.
    runs: Vec<u32>,
    /// Sim ticks a unit, a point of complexity, with a worker on it.
    ticks_per_point: u32,
    /// The share of the worked rate with nobody on it, per-mille.
    unattended_pm: u16,
}

impl FactoryRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse and check the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, a derating that is no share
    /// of the prototype or does not land on its own steps, a complexity
    /// with no runs, a unit that is no whole number of ticks, or a rate
    /// that is no share of the worked one.
    pub fn parse(text: &str) -> Result<Self, SchematicError> {
        let def: FactoryDef =
            ron::from_str(text).map_err(|error| SchematicError::Ron(error.to_string()))?;
        if def.version != Self::VERSION {
            return Err(SchematicError::Version {
                found: def.version,
                expected: Self::VERSION,
            });
        }
        let refused = |reason: String| SchematicError::Factory(reason);
        let share = |value: f32| {
            per_mille(value)
                .and_then(|share| u16::try_from(share).ok())
                .filter(|share| (1..=QUALITY_MAX).contains(share))
        };
        let derate = def.derate;
        let (Some(from), Some(to), Some(step)) =
            (share(derate.from), share(derate.to), share(derate.step))
        else {
            return Err(refused(format!(
                "derate is from {} to {} by {}: each a share over nought and up to one, in thousandths",
                derate.from, derate.to, derate.step
            )));
        };
        // The slider stops on the steps from the least, so the most must be
        // one of them, or the prototype's own figures could not be asked
        // for.
        if from >= to || !(to - from).is_multiple_of(step) {
            return Err(refused(format!(
                "derate is from {} to {} by {}: the least is under the most, a whole number of steps apart",
                derate.from, derate.to, derate.step
            )));
        }
        if def.runs.is_empty() {
            return Err(refused(
                "runs is empty: it gives the runs of complexity 1, then 2, and on".to_owned(),
            ));
        }
        if let Some(index) = def.runs.iter().position(|runs| *runs == 0) {
            return Err(refused(format!(
                "a blueprint of complexity {} starts with no runs",
                index + 1
            )));
        }
        // A unit is counted in ticks, and half a tick would make a batch
        // of twenty a tick longer or shorter than twenty units.
        let ticks = f64::from(def.seconds_per_point) * strata_core::SIM_TICK_HZ;
        let whole = ticks.round();
        if !ticks.is_finite() || whole < 1. || (ticks - whole).abs() > 1e-3 || whole > 1e9 {
            return Err(refused(format!(
                "seconds_per_point is {}, which is no whole number of the sim's {} ticks a second, one or more",
                def.seconds_per_point,
                strata_core::SIM_TICK_HZ
            )));
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "held to one tick and up, and under a thousand million, above"
        )]
        let ticks_per_point = whole as u32;
        let unattended_pm = share(def.unattended).ok_or_else(|| {
            refused(format!(
                "unattended is {}, a share of the worked rate over nought and up to one, in thousandths",
                def.unattended
            ))
        })?;
        Ok(Self {
            blueprint: def.blueprint,
            derate: (from, to, step),
            runs: def.runs,
            ticks_per_point,
            unattended_pm,
        })
    }

    /// The item a recorded blueprint is.
    #[must_use]
    pub const fn blueprint(&self) -> &Namespaced {
        &self.blueprint
    }

    /// The least and the most a blueprint is recorded at and the step
    /// between them, per-mille of the prototype: a slider's range.
    #[must_use]
    pub const fn derate(&self) -> (u16, u16, u16) {
        self.derate
    }

    /// Whether a blueprint may be recorded at `derate_pm` of its
    /// prototype: within the range and on one of its steps.
    #[must_use]
    pub const fn allows(&self, derate_pm: u16) -> bool {
        let (from, to, step) = self.derate;
        derate_pm >= from && derate_pm <= to && (derate_pm - from).is_multiple_of(step)
    }

    /// Sim ticks a unit takes a point of complexity, with a worker on the
    /// run.
    #[must_use]
    pub const fn ticks_per_point(&self) -> u32 {
        self.ticks_per_point
    }

    /// The share of the worked rate with nobody on the run, per-mille.
    #[must_use]
    pub const fn unattended_pm(&self) -> u16 {
        self.unattended_pm
    }

    /// These dials at another pace: a unit a point of complexity of
    /// `ticks_per_point` ticks, one tick at the least, and `unattended_pm`
    /// of the worked rate with nobody on the run, held to the scale. What a
    /// sheet's `Factory` boxes make of the dials for one run.
    #[must_use]
    pub fn paced(&self, ticks_per_point: u32, unattended_pm: u16) -> Self {
        Self {
            ticks_per_point: ticks_per_point.max(1),
            unattended_pm: unattended_pm.clamp(1, QUALITY_MAX),
            ..self.clone()
        }
    }

    /// The runs a blueprint of a schematic of `complexity` starts with.
    /// Nought for a complexity the dials do not reach, which no loaded
    /// schematic has.
    #[must_use]
    pub fn runs(&self, complexity: u8) -> u32 {
        usize::from(complexity)
            .checked_sub(1)
            .and_then(|index| self.runs.get(index))
            .copied()
            .unwrap_or(0)
    }

    /// The sim ticks a unit of a schematic of `complexity` takes with a
    /// worker on the run.
    #[must_use]
    pub fn unit_ticks(&self, complexity: u8) -> u32 {
        self.ticks_per_point
            .saturating_mul(u32::from(complexity.max(1)))
    }

    /// The rate a run goes at, per-mille of the worked one: the whole of
    /// it with somebody on it, the dial's share with nobody.
    #[must_use]
    pub const fn rate_pm(&self, worked: bool) -> u16 {
        if worked {
            QUALITY_MAX
        } else {
            self.unattended_pm
        }
    }
}

/// Why a schematic or the session's dials were refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchematicError {
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
    /// The session's dials break a rule.
    #[error("{0}")]
    Session(String),
    /// The factory's dials break a rule.
    #[error("{0}")]
    Factory(String),
    /// Two files define one schematic.
    #[error("`{0}` is defined twice")]
    Duplicate(Namespaced),
    /// A schematic breaks a rule.
    #[error("`{id}`: {reason}")]
    Schematic {
        /// The schematic.
        id: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
}

/// Every schematic, checked, with the session's dials and, when the data
/// set has them, the factory's.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchematicSet {
    /// In id order.
    schematics: Vec<Schematic>,
    rules: SessionRules,
    factory: Option<FactoryRules>,
}

impl SchematicSet {
    /// Check the schematics against the items, the resources and the
    /// dials, and resolve their weights. Each comes with the file it was
    /// read from, and an error names the file at fault.
    ///
    /// # Errors
    /// The file and what is wrong in it.
    pub fn build(
        mut defs: Vec<(String, SchematicDef)>,
        rules: SessionRules,
        items: &ItemCatalogue,
        resources: &ResourceSet,
    ) -> Result<Self, (String, SchematicError)> {
        // Sorted so the same data set lists the same way however the
        // directory does, and so "the first schematic that uses a part" is
        // one schematic.
        defs.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        let mut schematics = Vec::with_capacity(defs.len());
        for (index, (path, def)) in defs.iter().enumerate() {
            if index > 0 && defs[index - 1].1.id == def.id {
                return Err((path.clone(), SchematicError::Duplicate(def.id.clone())));
            }
            let weights = check_schematic(def, &rules, items, resources).map_err(|reason| {
                let error = SchematicError::Schematic {
                    id: def.id.clone(),
                    reason,
                };
                (path.clone(), error)
            })?;
            schematics.push(weights);
        }
        let schematics = defs
            .into_iter()
            .zip(schematics)
            .map(|((_, def), weights)| {
                let weapon_side = items.get(&def.makes).is_some_and(|product| {
                    matches!(
                        product.kind,
                        ItemKind::Module { .. } | ItemKind::Weapon { .. }
                    )
                });
                Schematic {
                    def,
                    weights,
                    weapon_side,
                }
            })
            .collect();
        Ok(Self {
            schematics,
            rules,
            factory: None,
        })
    }

    /// Check that every profession a schematic names is a box of the graph,
    /// so a typo cannot leave a schematic nobody can ever learn.
    ///
    /// # Errors
    /// The first schematic whose profession is no box.
    pub fn check_professions(&self, graph: &SkillGraph) -> Result<(), SchematicError> {
        for schematic in &self.schematics {
            if let Some(box_id) = &schematic.def.profession
                && graph.find(box_id).is_none()
            {
                return Err(SchematicError::Schematic {
                    id: schematic.def.id.clone(),
                    reason: format!("profession `{box_id}` is no box of the skill graph"),
                });
            }
        }
        Ok(())
    }

    /// The set with the factory's dials, checked against it and the items:
    /// the blueprint they name is an item of that kind, and every
    /// complexity a schematic may have starts with runs.
    ///
    /// # Errors
    /// The first of those that fails.
    pub fn with_factory(
        mut self,
        factory: FactoryRules,
        items: &ItemCatalogue,
    ) -> Result<Self, SchematicError> {
        let refused = |reason: String| SchematicError::Factory(reason);
        let id = factory.blueprint();
        match items.get(id).map(|def| &def.kind) {
            Some(ItemKind::Blueprint) => {}
            Some(kind) => {
                return Err(refused(format!(
                    "blueprint is `{id}`, a {}: a recording is an item of kind Blueprint",
                    kind.key()
                )));
            }
            None => return Err(refused(format!("blueprint is `{id}`, which is no item"))),
        }
        // A schematic may be as involved as the session gives points for,
        // and one of a complexity with no runs could be recorded and never
        // run.
        let most = self.rules.max_complexity();
        if factory.runs.len() < most {
            return Err(refused(format!(
                "runs gives {} complexities, and `{SESSION_FILE}` gives points for 1 to {most}",
                factory.runs.len()
            )));
        }
        self.factory = Some(factory);
        Ok(self)
    }

    /// A schematic by its id.
    #[must_use]
    pub fn get(&self, id: &Namespaced) -> Option<&Schematic> {
        self.schematics
            .iter()
            .find(|schematic| schematic.def.id == *id)
    }

    /// Every schematic, in id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Schematic> {
        self.schematics.iter()
    }

    /// How many schematics there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.schematics.len()
    }

    /// Whether the data set makes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schematics.is_empty()
    }

    /// The session's dials.
    #[must_use]
    pub const fn rules(&self) -> &SessionRules {
        &self.rules
    }

    /// The factory's dials; `None` for a data set without the file, in
    /// which nothing is recorded and nothing run.
    #[must_use]
    pub const fn factory(&self) -> Option<&FactoryRules> {
        self.factory.as_ref()
    }

    /// The first schematic, by id, that weighs `line` of the component
    /// `component`, with the place of the line of its product that does:
    /// where a part's line goes next. A schematic of a weapon or a module
    /// comes before any other. `None` for a line nothing reads yet.
    #[must_use]
    pub fn first_using(&self, component: &Namespaced, line: &str) -> Option<(&Schematic, usize)> {
        let first = |weapon_side: bool| {
            self.schematics
                .iter()
                .filter(|s| s.weapon_side == weapon_side)
                .find_map(|schematic| {
                    let uses = |(slot, stat, _): &(usize, Stat, u16)| {
                        matches!(stat, Stat::Line(named) if named == line)
                            && matches!(
                                &schematic.def.slots[*slot].takes,
                                Takes::Component(item) if item == component
                            )
                    };
                    let index = schematic
                        .weights
                        .iter()
                        .position(|weights| weights.iter().any(uses))?;
                    Some((schematic, index))
                })
        };
        first(true).or_else(|| first(false))
    }
}

/// The classes a resource slot may hold a lot of, or why the slot names
/// nothing the ground holds.
fn classes_of<'a>(takes: &Takes, resources: &'a ResourceSet) -> Result<Vec<&'a ClassDef>, String> {
    match takes {
        Takes::Family(id) => {
            if !resources.families().iter().any(|family| family.id == *id) {
                return Err(format!("`{id}` is no family of resource"));
            }
            let classes: Vec<&ClassDef> = resources
                .classes()
                .iter()
                .filter(|class| class.family == *id)
                .collect();
            if classes.is_empty() {
                return Err(format!("the family `{id}` has no class"));
            }
            Ok(classes)
        }
        Takes::Class(id) => resources
            .classes()
            .iter()
            .find(|class| class.id == *id)
            .map(|class| vec![class])
            .ok_or_else(|| format!("`{id}` is no class of resource")),
        Takes::Type(id) => resources
            .type_id(id)
            .map(|ty| vec![resources.class_of(ty)])
            .ok_or_else(|| format!("`{id}` is no type of resource")),
        Takes::Component(_) | Takes::Commodity(_) => Ok(Vec::new()),
    }
}

/// What a slot holds, as the checks of its weights need it.
enum Held<'a> {
    /// A lot of one of these classes.
    Lot(Vec<&'a ClassDef>),
    /// A component of this definition.
    Component(&'a ItemDefinition),
    /// A commodity.
    Commodity,
}

/// What is wrong with a slot, if anything; else what it holds.
fn check_slot<'a>(
    slot: &SlotDef,
    product: &ItemDefinition,
    items: &'a ItemCatalogue,
    resources: &'a ResourceSet,
) -> Result<Held<'a>, String> {
    let id = &slot.id;
    if slot.takes.is_resource() {
        let classes =
            classes_of(&slot.takes, resources).map_err(|why| format!("slot `{id}`: {why}"))?;
        let stack = resources.rules().lots.stack;
        if slot.count != 0 {
            return Err(format!(
                "slot `{id}` takes a lot, which is counted in units, and gives a count"
            ));
        }
        if slot.units == 0 || slot.units > stack {
            return Err(format!(
                "slot `{id}` takes {} units, a lot holds 1 to {stack}",
                slot.units
            ));
        }
        return Ok(Held::Lot(classes));
    }
    let (Takes::Component(item) | Takes::Commodity(item)) = &slot.takes else {
        unreachable!("a slot takes a lot or an item");
    };
    let def = items
        .get(item)
        .ok_or_else(|| format!("slot `{id}` takes `{item}`, which is no item"))?;
    let held = match (&slot.takes, &def.kind) {
        (Takes::Component(_), ItemKind::Component) => Held::Component(def),
        (Takes::Commodity(_), ItemKind::Commodity) => Held::Commodity,
        (Takes::Component(_), kind) => {
            return Err(format!(
                "slot `{id}` takes `{item}` as a component, and it is a {}",
                kind.key()
            ));
        }
        (_, kind) => {
            return Err(format!(
                "slot `{id}` takes `{item}` as a commodity, and it is a {}",
                kind.key()
            ));
        }
    };
    // Depth is capped at two: resource, component, product. A component
    // made of components would need a factory for a single rifle.
    if matches!(held, Held::Component(_)) && matches!(product.kind, ItemKind::Component) {
        return Err(format!(
            "slot `{id}` takes a component, and `{}` is itself a component: a component is made of resources and bought parts",
            product.id
        ));
    }
    if slot.units != 0 {
        return Err(format!(
            "slot `{id}` takes an item, which is counted, and gives units"
        ));
    }
    if slot.count == 0 || slot.count > def.stack {
        return Err(format!(
            "slot `{id}` takes {} of `{item}`, which stacks 1 to {}",
            slot.count, def.stack
        ));
    }
    Ok(held)
}

/// What is wrong with one schematic, if anything; else its weights, a list
/// a line of the product, each the slot's place, the stat and the weight in
/// per-mille.
fn check_schematic(
    def: &SchematicDef,
    rules: &SessionRules,
    items: &ItemCatalogue,
    resources: &ResourceSet,
) -> Result<Vec<Vec<(usize, Stat, u16)>>, String> {
    let product = items
        .get(&def.makes)
        .ok_or_else(|| format!("makes `{}`, which is no item", def.makes))?;
    if !matches!(
        product.kind,
        ItemKind::Component
            | ItemKind::Module { .. }
            | ItemKind::Kit
            | ItemKind::Weapon { .. }
            | ItemKind::Tool { .. }
            | ItemKind::Vehicle { .. }
            | ItemKind::Harvester { .. }
    ) {
        return Err(format!(
            "makes `{}`, a {}: a schematic makes a component, a module, a weapon, a tool, a vehicle, a harvester (or a kit)",
            def.makes,
            product.kind.key()
        ));
    }
    if def.complexity == 0 || usize::from(def.complexity) > rules.max_complexity() {
        return Err(format!(
            "complexity {}, and `{SESSION_FILE}` gives points for 1 to {}",
            def.complexity,
            rules.max_complexity()
        ));
    }
    if def.slots.is_empty() {
        return Err("takes nothing: a schematic has a slot".to_owned());
    }
    let mut seen = BTreeSet::new();
    let mut held = Vec::with_capacity(def.slots.len());
    for slot in &def.slots {
        if !seen.insert(&slot.id) {
            return Err(format!("slot `{}` is declared twice", slot.id));
        }
        held.push(check_slot(slot, product, items, resources)?);
    }

    // Every line of the product, once, in the product's order, and nothing
    // else: a line left out would have no ceiling, and one the product
    // lacks would be a number no record holds.
    let expected: Vec<&str> = product.lines.iter().map(|line| line.id.as_str()).collect();
    let given: Vec<&str> = def.lines.iter().map(|line| line.line.as_str()).collect();
    if given != expected {
        return Err(format!(
            "gives the lines {given:?}, and `{}` has {expected:?}: each once, in that order",
            product.id
        ));
    }

    let mut weights = Vec::with_capacity(def.lines.len());
    for recipe in &def.lines {
        let line = &recipe.line;
        let mut resolved: Vec<(usize, Stat, u16)> = Vec::with_capacity(recipe.from.len());
        for weight in &recipe.from {
            let slot = def
                .slots
                .iter()
                .position(|slot| slot.id == weight.slot)
                .ok_or_else(|| {
                    format!("line `{line}` weighs slot `{}`, which is none", weight.slot)
                })?;
            check_stat(line, &def.slots[slot], &held[slot], &weight.stat)?;
            if resolved
                .iter()
                .any(|(at, stat, _)| *at == slot && *stat == weight.stat)
            {
                return Err(format!(
                    "line `{line}` weighs {:?} of slot `{}` twice",
                    weight.stat, weight.slot
                ));
            }
            let share = per_mille(weight.weight)
                .and_then(|share| u16::try_from(share).ok())
                .filter(|share| (1..=QUALITY_MAX).contains(share))
                .ok_or_else(|| {
                    format!(
                        "line `{line}` gives {:?} of slot `{}` the weight {}, a share over nought and up to one, in thousandths",
                        weight.stat, weight.slot, weight.weight
                    )
                })?;
            resolved.push((slot, weight.stat.clone(), share));
        }
        let sum: u32 = resolved.iter().map(|(.., share)| u32::from(*share)).sum();
        if sum != PER_MILLE {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a handful of thousandths, shown as the share the file's author wrote"
            )]
            let wrote = f64::from(sum) / f64::from(PER_MILLE);
            return Err(format!(
                "the weights of line `{line}` sum to {wrote}, not to one"
            ));
        }
        weights.push(resolved);
    }
    Ok(weights)
}

/// What is wrong with a weight's stat on its slot, if anything: a line may
/// weigh only what its slot is sure to carry.
fn check_stat(line: &str, slot: &SlotDef, held: &Held<'_>, stat: &Stat) -> Result<(), String> {
    let id = &slot.id;
    match (held, stat) {
        (Held::Lot(classes), Stat::Attribute(attribute) | Stat::Light(attribute)) => {
            // Any class the slot may hold must carry it: an attribute a
            // class does not carry is absent from its lots, not nought.
            match classes
                .iter()
                .find(|class| !class.carries.contains(attribute))
            {
                Some(class) => Err(format!(
                    "line `{line}` weighs {} of slot `{id}`, which the class `{}` does not carry",
                    attribute.name(),
                    class.id
                )),
                None => Ok(()),
            }
        }
        (Held::Lot(_), Stat::Line(named)) => Err(format!(
            "line `{line}` weighs the line `{named}` of slot `{id}`, which takes a lot: a lot has attributes"
        )),
        (Held::Component(def), Stat::Line(named)) => {
            if def.lines.iter().any(|own| own.id == *named) {
                Ok(())
            } else {
                Err(format!(
                    "line `{line}` weighs the line `{named}` of slot `{id}`, which `{}` does not define",
                    def.id
                ))
            }
        }
        (Held::Component(def), Stat::Attribute(attribute) | Stat::Light(attribute)) => {
            Err(format!(
                "line `{line}` weighs {} of slot `{id}`, which takes `{}`: a component has lines",
                attribute.name(),
                def.id
            ))
        }
        (Held::Commodity, _) => Err(format!(
            "line `{line}` weighs slot `{id}`, which takes a commodity: a commodity is flat"
        )),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::load::{LoadError, load_from_dir};
    use crate::{GameRegistries, InMemorySource};

    fn shipped() -> GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).expect("assets/ loads")
    }

    fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).expect("a valid id")
    }

    const SESSION: &str = "(version: 1, assembly: 0.6, points: [3, 5, 8],
        results: [(Exceptional, 0.16), (Good, 0.10), (Fair, 0.06), (Poor, 0.02), (Setback, -0.04)],
        odds: (steady: [[0, 35, 50, 15, 0], [5, 55, 35, 5, 0], [15, 65, 20, 0, 0]],
               bold: [[15, 20, 25, 22, 18], [25, 25, 25, 15, 10], [38, 27, 20, 10, 5]]),
        base_row: 1, workability: (row_up_from: 700))";

    const CORE: &str = r#"(id: "test:schematic.core", makes: "veyra:component.cell_core", complexity: 1,
        slots: [
            (id: "salt", name: "Reactive", takes: Class("strata:reactive"), units: 20),
            (id: "brine", name: "Brine", takes: Class("strata:brine"), units: 30),
        ],
        lines: [
            (line: "charge_density", from: [
                (slot: "salt", stat: Attribute(Potency), weight: 0.5),
                (slot: "brine", stat: Attribute(Potency), weight: 0.3),
                (slot: "brine", stat: Attribute(Conductivity), weight: 0.2),
            ]),
            (line: "stability", from: [
                (slot: "salt", stat: Attribute(Stability), weight: 0.6),
                (slot: "brine", stat: Attribute(Purity), weight: 0.4),
            ]),
        ])"#;

    const FEED: &str = r#"(id: "test:schematic.feed", makes: "veyra:module.patrol_extended_feed", complexity: 2,
        slots: [
            (id: "core", name: "Cell core", takes: Component("veyra:component.cell_core"), count: 1),
            (id: "harness", name: "Harness", takes: Component("veyra:component.harness"), count: 1),
            (id: "casing", name: "Casing", takes: Type("strata:light_alloy_ore"), units: 40),
            (id: "seals", name: "Seals", takes: Commodity("veyra:commodity.seal_set"), count: 1),
        ],
        lines: [
            (line: "capacity", from: [
                (slot: "core", stat: Line("charge_density"), weight: 0.60),
                (slot: "harness", stat: Line("conductance"), weight: 0.25),
                (slot: "casing", stat: Attribute(Purity), weight: 0.15),
            ]),
            (line: "reload_time", from: [
                (slot: "casing", stat: Light(Density), weight: 0.60),
                (slot: "casing", stat: Attribute(Strength), weight: 0.40),
            ]),
        ])"#;

    /// The schematic `text` spells over the shipped items and resources,
    /// under the shipped dials; refused, why.
    fn built(text: &str) -> Result<SchematicSet, String> {
        let registries = shipped();
        let def: SchematicDef = ron::from_str(text).expect("the test's schematic parses");
        let rules = SessionRules::parse(SESSION).expect("the dials stand");
        SchematicSet::build(
            vec![("schematics/test.schematic.ron".to_owned(), def)],
            rules,
            &registries.items,
            &registries.resources,
        )
        .map_err(|(path, error)| {
            assert_eq!(path, "schematics/test.schematic.ron");
            error.to_string()
        })
    }

    /// `text`, with `from` replaced by `to`, is refused with `reason` in
    /// what is said.
    #[track_caller]
    fn refused(text: &str, from: &str, to: &str, reason: &str) {
        let changed = text.replace(from, to);
        assert_ne!(changed, text, "`{from}` is in the schematic");
        let error = built(&changed).expect_err("the schematic is refused");
        assert!(error.contains(reason), "{error}");
    }

    /// The chain of the worked example ships: the cell core, the patrol
    /// magazine over it and the pistol's cell over the same core, with the
    /// weights of crafting.md 5.1 in per-mille and the mockup's dials.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "three schematics and the dials, a table of expected literals each"
    )]
    fn the_shipped_schematics_load() {
        use Attribute::{Conductivity, Density, Potency, Purity, Stability, Strength};
        let registries = shipped();
        let set = &registries.schematics;
        let ids: Vec<String> = set.iter().map(|s| s.def.id.to_string()).collect();
        assert_eq!(
            ids,
            [
                "veyra:schematic.blade_balanced_grip",
                "veyra:schematic.blade_hooked_edge",
                "veyra:schematic.cell_core",
                "veyra:schematic.courier_bike",
                "veyra:schematic.cutter_compact_power",
                "veyra:schematic.cutter_reinforced_head",
                "veyra:schematic.cutter_wrapped_grip",
                "veyra:schematic.drive_coil",
                "veyra:schematic.field_harvester",
                "veyra:schematic.field_repair_kit",
                "veyra:schematic.glaive_broad_edge",
                "veyra:schematic.glaive_counterweight",
                "veyra:schematic.harness",
                "veyra:schematic.hull_plate",
                "veyra:schematic.marksman_field_rest",
                "veyra:schematic.mount_blank",
                "veyra:schematic.patrol_carbine",
                "veyra:schematic.patrol_extended_feed",
                "veyra:schematic.pier_glaive",
                "veyra:schematic.pistol_extended_cell",
                "veyra:schematic.service_blade",
                "veyra:schematic.service_pistol",
                "veyra:schematic.survey_tool",
                "veyra:schematic.vigil_angled_grip",
                "veyra:schematic.vigil_compact_feed",
                "veyra:schematic.vigil_compensator",
                "veyra:schematic.vigil_long_optic",
                "veyra:schematic.vigil_marksman_rifle",
                "veyra:schematic.vigil_precision_stock",
                "veyra:schematic.vigil_reflex",
                "veyra:schematic.yard_greatblade",
            ]
        );
        assert_eq!(set.len(), 31);

        let core = set.get(&id("veyra:schematic.cell_core")).expect("ships");
        assert_eq!(core.def.makes, id("veyra:component.cell_core"));
        assert_eq!(core.def.complexity, 1);
        let amounts: Vec<(&str, u32)> = core
            .def
            .slots
            .iter()
            .map(|slot| (slot.id.as_str(), slot.amount()))
            .collect();
        assert_eq!(amounts, [("salt", 20), ("brine", 30)]);
        assert_eq!(
            core.weights,
            vec![
                vec![
                    (0, Stat::Attribute(Potency), 500),
                    (1, Stat::Attribute(Potency), 300),
                    (1, Stat::Attribute(Conductivity), 200),
                ],
                vec![
                    (0, Stat::Attribute(Stability), 600),
                    (1, Stat::Attribute(Purity), 400),
                ],
            ]
        );
        assert_eq!(
            core.stats_on(1),
            [
                &Stat::Attribute(Potency),
                &Stat::Attribute(Conductivity),
                &Stat::Attribute(Purity)
            ]
        );

        let feed = set
            .get(&id("veyra:schematic.patrol_extended_feed"))
            .expect("ships");
        assert_eq!(feed.def.makes, id("veyra:module.patrol_extended_feed"));
        assert_eq!(feed.def.complexity, 2);
        assert_eq!(feed.def.licence, Some(id("veyra:maker.vigil")));
        let amounts: Vec<(&str, u32)> = feed
            .def
            .slots
            .iter()
            .map(|slot| (slot.id.as_str(), slot.amount()))
            .collect();
        assert_eq!(
            amounts,
            [("core", 1), ("harness", 1), ("casing", 40), ("seals", 1)]
        );
        assert_eq!(
            feed.weights,
            vec![
                vec![
                    (0, Stat::Line("charge_density".to_owned()), 600),
                    (1, Stat::Line("conductance".to_owned()), 250),
                    (2, Stat::Attribute(Purity), 150),
                ],
                vec![
                    (2, Stat::Light(Density), 600),
                    (2, Stat::Attribute(Strength), 400),
                ],
            ]
        );
        // Nothing weighs the seals.
        assert_eq!(feed.weights_on(3).count(), 0);

        let cell = set
            .get(&id("veyra:schematic.pistol_extended_cell"))
            .expect("ships");
        assert_eq!(cell.def.makes, id("veyra:module.pistol_extended_cell"));
        assert_eq!(cell.slot("casing").map(|(_, slot)| slot.units), Some(25));
        assert_eq!(
            cell.weights, feed.weights,
            "the same recipe, a smaller casing"
        );

        // Where a part's line goes next: the first schematic by id.
        let core_item = id("veyra:component.cell_core");
        let (next, line) = set
            .first_using(&core_item, "charge_density")
            .expect("the magazine reads it");
        assert_eq!((next.def.id.clone(), line), (feed.def.id.clone(), 0));
        assert!(set.first_using(&core_item, "stability").is_none());
        let (next, line) = set
            .first_using(&id("veyra:component.harness"), "conductance")
            .expect("the carbine reads it");
        // The carbine's schematic sorts before the magazine's, and the
        // rifle's (`vigil_`) after both.
        assert_eq!(
            (next.def.id.clone(), line),
            (id("veyra:schematic.patrol_carbine"), 0)
        );

        // A tool or a bike made of the same part does not take its place, and
        // a part only they read goes into the first of them.
        let (coil, _) = set
            .first_using(&id("veyra:component.drive_coil"), "flux")
            .expect("the bike reads it");
        assert_eq!(coil.def.id, id("veyra:schematic.courier_bike"));
        let (plate, line) = set
            .first_using(&id("veyra:component.hull_plate"), "rigidity")
            .expect("the bike reads it");
        assert_eq!((plate.def.id.clone(), line), (coil.def.id.clone(), 1));

        // The dials are the mockup's.
        let rules = set.rules();
        assert_eq!(rules.assembly_pm(), 600);
        assert_eq!(
            [
                rules.points(1),
                rules.points(2),
                rules.points(3),
                rules.points(4)
            ],
            [3, 5, 8, 0]
        );
        assert_eq!(
            Outcome::ALL.map(|outcome| rules.lift_pm(outcome)),
            [160, 100, 60, 20, -40]
        );
        assert_eq!(rules.base_row(), 1);
        assert_eq!(rules.odds(Hand::Steady, 1), [5, 55, 35, 5, 0]);
        assert_eq!(rules.odds(Hand::Bold, 1), [25, 25, 25, 15, 10]);
        assert_eq!(rules.odds(Hand::Bold, 2), [38, 27, 20, 10, 5]);
        assert_eq!(
            [rules.row(None), rules.row(Some(699)), rules.row(Some(700))],
            [1, 1, 2]
        );
        assert_eq!(
            *rules,
            SessionRules::parse(SESSION).expect("the same dials")
        );
    }

    #[test]
    fn a_line_weighs_only_what_its_slot_carries() {
        assert!(built(CORE).is_ok() && built(FEED).is_ok());
        // A reactive carries no conductivity.
        refused(
            CORE,
            r#"(slot: "salt", stat: Attribute(Potency), weight: 0.5)"#,
            r#"(slot: "salt", stat: Attribute(Conductivity), weight: 0.5)"#,
            "Conductivity of slot `salt`, which the class `strata:reactive` does not carry",
        );
        // Read as Light it is carried no more than read straight.
        refused(
            CORE,
            r#"(slot: "salt", stat: Attribute(Potency), weight: 0.5)"#,
            r#"(slot: "salt", stat: Light(Density), weight: 0.5)"#,
            "Density of slot `salt`",
        );
        // A family's slot may weigh only what every class of it carries:
        // structural metal has no potency.
        refused(
            CORE,
            r#"takes: Class("strata:reactive")"#,
            r#"takes: Family("strata:mineral")"#,
            "which the class `strata:structural_metal` does not carry",
        );
        let any_mineral = CORE
            .replace(
                r#"takes: Class("strata:reactive")"#,
                r#"takes: Family("strata:mineral")"#,
            )
            .replace(
                "Attribute(Potency), weight: 0.5",
                "Attribute(Purity), weight: 0.5",
            )
            .replace(
                r#"(slot: "salt", stat: Attribute(Stability)"#,
                r#"(slot: "salt", stat: Light(Stability)"#,
            );
        assert_eq!(
            built(&any_mineral).map(|_| ()),
            Ok(()),
            "every mineral has both"
        );
        // A lot has attributes and a component has lines, never the other.
        refused(
            CORE,
            "Attribute(Potency), weight: 0.5",
            r#"Line("charge_density"), weight: 0.5"#,
            "takes a lot: a lot has attributes",
        );
        refused(
            FEED,
            r#"Line("charge_density")"#,
            "Attribute(Potency)",
            "a component has lines",
        );
        refused(
            FEED,
            r#"Line("charge_density")"#,
            r#"Line("conductance")"#,
            "which `veyra:component.cell_core` does not define",
        );
        // Nothing weighs a commodity, and a slot is a slot of the schematic.
        refused(
            FEED,
            r#"(slot: "casing", stat: Attribute(Purity), weight: 0.15)"#,
            r#"(slot: "seals", stat: Attribute(Purity), weight: 0.15)"#,
            "a commodity is flat",
        );
        refused(
            FEED,
            r#"(slot: "casing", stat: Attribute(Purity), weight: 0.15)"#,
            r#"(slot: "shell", stat: Attribute(Purity), weight: 0.15)"#,
            "weighs slot `shell`, which is none",
        );
        // What a slot takes exists, and is of the kind the slot says.
        refused(
            CORE,
            "strata:reactive",
            "strata:unobtainium",
            "`strata:unobtainium` is no class of resource",
        );
        refused(
            FEED,
            "strata:light_alloy_ore",
            "strata:reactive",
            "`strata:reactive` is no type of resource",
        );
        refused(
            FEED,
            r#"Commodity("veyra:commodity.seal_set")"#,
            r#"Commodity("veyra:component.harness")"#,
            "as a commodity, and it is a component",
        );
        refused(
            FEED,
            r#"Component("veyra:component.harness")"#,
            r#"Component("veyra:module.vigil_reflex")"#,
            "as a component, and it is a module",
        );
        refused(
            FEED,
            r#"Component("veyra:component.harness")"#,
            r#"Component("veyra:component.none")"#,
            "which is no item",
        );
    }

    #[test]
    fn every_line_of_the_product_is_given_once() {
        // One left out.
        let short = CORE.split(r#"(line: "stability""#).next().expect("a head");
        let short = format!("{short}])");
        let error = built(&short).expect_err("a line is missing");
        assert!(
            error.contains(r#"gives the lines ["charge_density"]"#)
                && error.contains(r#"["charge_density", "stability"]"#),
            "{error}"
        );
        // One the product has not got.
        refused(
            CORE,
            r#"(line: "stability""#,
            r#"(line: "lustre""#,
            r#""lustre"]"#,
        );
        // One given twice.
        refused(
            CORE,
            r#"(line: "stability""#,
            r#"(line: "charge_density""#,
            "each once, in that order",
        );
        // And out of the product's order.
        let (head, tail) = FEED.split_once(r#"(line: "capacity""#).expect("two lines");
        let (capacity, reload) = tail
            .split_once(r#"(line: "reload_time""#)
            .expect("two lines");
        let reload = reload.trim_end().trim_end_matches("])").trim_end();
        let swapped = format!(
            r#"{head}(line: "reload_time"{reload} (line: "capacity"{} ])"#,
            capacity.trim_end()
        );
        let error = built(&swapped).expect_err("the lines are out of order");
        assert!(error.contains("in that order"), "{error}");
        // What it makes is an item, and a component or a module.
        refused(
            CORE,
            "veyra:component.cell_core",
            "veyra:component.none",
            "which is no item",
        );
        refused(
            CORE,
            r#"makes: "veyra:component.cell_core""#,
            r#"makes: "veyra:commodity.seal_set""#,
            "a schematic makes a component, a module, a weapon, a tool, a vehicle, a harvester (or a kit)",
        );
        refused(
            CORE,
            r#"makes: "veyra:component.cell_core""#,
            r#"makes: "veyra:bag.field_satchel""#,
            "a schematic makes a component, a module, a weapon, a tool, a vehicle, a harvester (or a kit)",
        );
        refused(CORE, "complexity: 1", "complexity: 0", "complexity 0");
        refused(
            CORE,
            "complexity: 1",
            "complexity: 4",
            "gives points for 1 to 3",
        );
    }

    /// The carbine is a weapon a schematic makes, and a profession that is
    /// no box of the graph is refused when the set loads.
    #[test]
    fn a_weapon_is_made_and_a_profession_must_be_a_box() {
        let registries = shipped();
        let set = &registries.schematics;
        let carbine = set
            .get(&id("veyra:schematic.patrol_carbine"))
            .expect("ships");
        assert_eq!(carbine.def.makes, id("veyra:weapon.patrol_carbine"));
        assert_eq!(
            carbine.def.profession.as_deref(),
            Some("weaponsmith.frames.1")
        );
        assert_eq!(carbine.def.lines.len(), 3);
        assert_eq!(set.check_professions(&registries.progression), Ok(()));
        let mut stray = set.clone();
        stray.schematics[0].def.profession = Some("weaponsmith.nothing.9".to_owned());
        let error = stray
            .check_professions(&registries.progression)
            .expect_err("no such box");
        assert!(error.to_string().contains("is no box"), "{error}");
    }

    /// Each melee schematic is taught by the Edges box the tree's words
    /// name, and its slots are the ones its lines read.
    #[test]
    fn the_melee_schematics_are_taught_by_their_edges_boxes() {
        let registries = shipped();
        let boxes = [
            ("service_blade", 1),
            ("blade_hooked_edge", 1),
            ("blade_balanced_grip", 2),
            ("pier_glaive", 2),
            ("yard_greatblade", 3),
            ("cutter_reinforced_head", 3),
            ("glaive_broad_edge", 3),
            ("glaive_counterweight", 3),
            ("cutter_wrapped_grip", 4),
            ("cutter_compact_power", 4),
        ];
        for (name, tier) in boxes {
            let schematic = registries
                .schematics
                .get(&id(&format!("veyra:schematic.{name}")))
                .expect("ships");
            assert_eq!(
                schematic.def.profession.as_deref(),
                Some(format!("weaponsmith.edges.{tier}").as_str()),
                "{name}"
            );
            for slot in &schematic.def.slots {
                assert!(!slot.id.starts_with("weaponsmith"), "{name}: {}", slot.id);
            }
        }
    }

    /// The rifle and its four parts are made by the boxes the tree's words
    /// name, and each makes an item that exists with the lines it reads.
    #[test]
    fn the_rifle_schematics_are_taught_by_the_boxes_the_tree_names() {
        let registries = shipped();
        let boxes = [
            ("vigil_marksman_rifle", "weaponsmith.frames.3"),
            ("vigil_compact_feed", "weaponsmith.fittings.2"),
            ("vigil_precision_stock", "weaponsmith.fittings.3"),
            ("vigil_long_optic", "weaponsmith.fittings.4"),
            ("marksman_field_rest", "weaponsmith.fittings.4"),
        ];
        for (name, taught_by) in boxes {
            let schematic = registries
                .schematics
                .get(&id(&format!("veyra:schematic.{name}")))
                .expect("ships");
            assert_eq!(
                schematic.def.profession.as_deref(),
                Some(taught_by),
                "{name}"
            );
            let made = registries.items.get(&schematic.def.makes).expect("an item");
            let lines: Vec<&str> = made.lines.iter().map(|l| l.id.as_str()).collect();
            let read: Vec<&str> = schematic
                .def
                .lines
                .iter()
                .map(|l| l.line.as_str())
                .collect();
            assert_eq!(read, lines[..read.len()], "{name}");
            let taught = registries.progression.find(taught_by).expect("a box");
            assert!(
                taught.description.contains(&made.name),
                "{taught_by} does not say it teaches {}",
                made.name
            );
        }
        let rifle = registries
            .schematics
            .get(&id("veyra:schematic.vigil_marksman_rifle"))
            .expect("ships");
        assert_eq!(rifle.def.slots[0].units, 110);
        assert_eq!(
            rifle.def.licence.as_ref().map(ToString::to_string),
            Some("veyra:maker.vigil".into())
        );
    }

    #[test]
    fn a_lines_weights_sum_to_one() {
        refused(
            CORE,
            "Attribute(Conductivity), weight: 0.2",
            "Attribute(Conductivity), weight: 0.3",
            "the weights of line `charge_density` sum to 1.1, not to one",
        );
        refused(
            CORE,
            "Attribute(Purity), weight: 0.4",
            "Attribute(Purity), weight: 0.3",
            "the weights of line `stability` sum to 0.9",
        );
        // A weight is a whole number of thousandths, over nought.
        refused(
            CORE,
            "Attribute(Conductivity), weight: 0.2",
            "Attribute(Conductivity), weight: 0.2005",
            "in thousandths",
        );
        refused(
            CORE,
            "Attribute(Conductivity), weight: 0.2",
            "Attribute(Conductivity), weight: 0.0",
            "a share over nought",
        );
        refused(
            CORE,
            "Attribute(Conductivity), weight: 0.2",
            "Attribute(Conductivity), weight: -0.2",
            "a share over nought",
        );
        // One stat of one slot once a line: twice would be one weight
        // written as two.
        refused(
            CORE,
            r#"(slot: "brine", stat: Attribute(Conductivity), weight: 0.2)"#,
            r#"(slot: "brine", stat: Attribute(Potency), weight: 0.2)"#,
            "twice",
        );
        // A thousandth is a whole share: 0.333 + 0.333 + 0.334.
        let thirds = CORE
            .replace("weight: 0.2", "weight: 0.334")
            .replace(
                r#"(slot: "brine", stat: Attribute(Potency), weight: 0.3)"#,
                r#"(slot: "brine", stat: Attribute(Potency), weight: 0.333)"#,
            )
            .replace("weight: 0.5", "weight: 0.333");
        let set = built(&thirds).expect("a thousand thousandths");
        let core = set.get(&id("test:schematic.core")).expect("built");
        let shares: Vec<u16> = core.weights[0].iter().map(|(.., share)| *share).collect();
        assert_eq!(shares, [333, 333, 334]);
    }

    #[test]
    fn a_component_is_not_made_of_components() {
        // The harness in a cell core's slot: depth three.
        refused(
            CORE,
            r#"(id: "brine", name: "Brine", takes: Class("strata:brine"), units: 30)"#,
            r#"(id: "brine", name: "Brine", takes: Component("veyra:component.harness"), count: 1)"#,
            "is itself a component",
        );
        // A commodity in one is no depth at all.
        let sealed = CORE.replace(
            r#"(id: "brine", name: "Brine", takes: Class("strata:brine"), units: 30),"#,
            r#"(id: "brine", name: "Brine", takes: Class("strata:brine"), units: 30),
               (id: "seals", name: "Seals", takes: Commodity("veyra:commodity.seal_set"), count: 2),"#,
        );
        assert_ne!(sealed, CORE);
        assert_eq!(built(&sealed).map(|_| ()), Ok(()));
        // A module is made of them, which is what they are for.
        assert_eq!(built(FEED).map(|_| ()), Ok(()));
    }

    #[test]
    fn a_slot_gives_units_or_a_count() {
        refused(
            CORE,
            r#"takes: Class("strata:reactive"), units: 20"#,
            r#"takes: Class("strata:reactive"), count: 20"#,
            "slot `salt` takes a lot, which is counted in units, and gives a count",
        );
        refused(
            CORE,
            r#"takes: Class("strata:reactive"), units: 20"#,
            r#"takes: Class("strata:reactive")"#,
            "slot `salt` takes 0 units, a lot holds 1 to 200",
        );
        refused(
            CORE,
            "units: 30",
            "units: 201",
            "slot `brine` takes 201 units, a lot holds 1 to 200",
        );
        refused(
            FEED,
            r#"Component("veyra:component.harness"), count: 1"#,
            r#"Component("veyra:component.harness"), units: 1"#,
            "slot `harness` takes an item, which is counted, and gives units",
        );
        refused(
            FEED,
            r#"Component("veyra:component.harness"), count: 1"#,
            r#"Component("veyra:component.harness"), count: 21"#,
            "slot `harness` takes 21 of `veyra:component.harness`, which stacks 1 to 20",
        );
        refused(
            FEED,
            r#"Commodity("veyra:commodity.seal_set"), count: 1"#,
            r#"Commodity("veyra:commodity.seal_set")"#,
            "slot `seals` takes 0 of",
        );
        refused(
            FEED,
            r#"(id: "harness", name: "Harness""#,
            r#"(id: "core", name: "Harness""#,
            "slot `core` is declared twice",
        );
        let nothing = CORE.split("slots:").next().expect("a head");
        let nothing = format!("{nothing}slots: [], lines: [])");
        let error = built(&nothing).expect_err("no slot");
        assert!(error.contains("takes nothing"), "{error}");
    }

    #[test]
    fn the_session_dials_are_checked() {
        let rules = SessionRules::parse(SESSION).expect("the dials stand");
        assert_eq!(rules.assembly_pm(), 600);
        let refusal = |from: &str, to: &str| {
            let changed = SESSION.replace(from, to);
            assert_ne!(changed, SESSION, "`{from}` is in the dials");
            SessionRules::parse(&changed)
                .expect_err("the dials are refused")
                .to_string()
        };
        assert!(refusal("assembly: 0.6", "assembly: 0.0").contains("assembly is 0"));
        assert!(refusal("assembly: 0.6", "assembly: 1.2").contains("assembly is 1.2"));
        assert!(refusal("assembly: 0.6", "assembly: 0.6004").contains("in thousandths"));
        assert_eq!(
            SessionRules::parse(&SESSION.replace("assembly: 0.6", "assembly: 1.0"))
                .map(|rules| rules.assembly_pm()),
            Ok(1000)
        );
        assert!(refusal("points: [3, 5, 8]", "points: []").contains("points is empty"));
        // The five results, in their order.
        let said = refusal("(Good, 0.10), (Fair, 0.06)", "(Fair, 0.06), (Good, 0.10)");
        assert!(said.contains("not the five in their order"), "{said}");
        let said = refusal(", (Setback, -0.04)", "");
        assert!(said.contains("not the five in their order"), "{said}");
        assert!(refusal("(Good, 0.10)", "(Good, 1.5)").contains("Good lifts by 1.5"));
        // Three rows a hand, each a hundred.
        let said = refusal("[5, 55, 35, 5, 0]", "[5, 55, 35, 6, 0]");
        assert!(
            said.contains("row 1 of steady sums to 101, not 100"),
            "{said}"
        );
        let said = refusal(", [38, 27, 20, 10, 5]", "");
        assert!(said.contains("bold has 2 rows of odds, not 3"), "{said}");
        let said = refusal("[15, 65, 20, 0, 0]", "[15, 65, 20, 0]");
        assert!(said.contains("row 2 of steady has 4 odds"), "{said}");
        assert!(refusal("base_row: 1", "base_row: 3").contains("base_row is 3"));
        assert!(refusal("row_up_from: 700", "row_up_from: 1001").contains("over 1000"));
        assert!(matches!(
            SessionRules::parse(&SESSION.replace("version: 1", "version: 2")),
            Err(SchematicError::Version {
                found: 2,
                expected: 1
            })
        ));
        assert!(matches!(
            SessionRules::parse("(version: 1)"),
            Err(SchematicError::Ron(_))
        ));

        // A data set without the file has no points, so no schematic: the
        // file is named by the refusal.
        let registries = shipped();
        let def: SchematicDef = ron::from_str(CORE).expect("parses");
        let (_, error) = SchematicSet::build(
            vec![("schematics/core.schematic.ron".to_owned(), def)],
            SessionRules::default(),
            &registries.items,
            &registries.resources,
        )
        .expect_err("no dials, no schematics");
        assert!(error.to_string().contains(SESSION_FILE), "{error}");
        assert_eq!(SessionRules::default().points(1), 0);
    }

    const FACTORY: &str = r#"(version: 1, blueprint: "veyra:blueprint.production",
        derate: (from: 0.70, to: 1.00, step: 0.05), runs: [100, 50, 25],
        seconds_per_point: 12.0, unattended: 0.5)"#;

    /// The factory's dials, with `from` replaced by `to`, are refused by
    /// their own parse; what is said.
    #[track_caller]
    fn unruled(from: &str, to: &str) -> String {
        let changed = FACTORY.replace(from, to);
        assert_ne!(changed, FACTORY, "`{from}` is in the dials");
        FactoryRules::parse(&changed)
            .expect_err("the dials are refused")
            .to_string()
    }

    /// The factory's dials ship as the design's: a blueprint is recorded at
    /// 70 to 100 % of its prototype in steps of five, starts with a hundred
    /// runs at complexity 1, and a unit is twelve seconds a point with a
    /// worker and twice that with nobody.
    #[test]
    fn the_shipped_factory_dials_load() {
        let registries = shipped();
        let rules = registries.schematics.factory().expect("ships");
        assert_eq!(rules, &FactoryRules::parse(FACTORY).expect("the dials"));
        assert_eq!(rules.blueprint(), &id("veyra:blueprint.production"));
        assert_eq!(rules.derate(), (700, 1000, 50));
        assert_eq!((rules.runs(1), rules.runs(2), rules.runs(3)), (100, 50, 25));
        assert_eq!((rules.runs(0), rules.runs(4)), (0, 0));
        assert_eq!((rules.unit_ticks(1), rules.unit_ticks(2)), (240, 480));
        assert_eq!((rules.rate_pm(true), rules.rate_pm(false)), (1000, 500));
        // Every shipped schematic can be recorded and run.
        for schematic in registries.schematics.iter() {
            assert!(
                rules.runs(schematic.def.complexity) > 0,
                "{}",
                schematic.def.id
            );
        }
        assert!(matches!(
            FactoryRules::parse(&FACTORY.replace("version: 1", "version: 2")),
            Err(SchematicError::Version {
                found: 2,
                expected: 1
            })
        ));
        assert!(matches!(
            FactoryRules::parse("(version: 1)"),
            Err(SchematicError::Ron(_))
        ));
        let said = unruled("unattended: 0.5", "unattended: 0.0");
        assert!(said.contains("unattended is 0"), "{said}");
        let said = unruled("unattended: 0.5", "unattended: 1.5");
        assert!(said.contains("unattended is 1.5"), "{said}");

        // A data set without the file has schematics and records nothing;
        // one whose file is wrong is refused by that file's name.
        let load = |source: &InMemorySource| {
            crate::load::schematics(source, &registries.items, &registries.resources)
        };
        let source = InMemorySource::new()
            .with(SESSION_FILE, SESSION)
            .with("schematics/core.schematic.ron", CORE);
        assert_eq!(load(&source).expect("loads").factory(), None);
        let with = |factory: &str| load(&source.clone().with(FACTORY_FILE, factory));
        assert!(with(FACTORY).expect("loads").factory().is_some());
        let error = with(&FACTORY.replace("step: 0.05", "step: 0.04")).expect_err("off its steps");
        assert!(
            matches!(&error, LoadError::Schematics { path, source }
                if path == FACTORY_FILE && matches!(**source, SchematicError::Factory(_))),
            "{error}"
        );
        // The blueprint is an item, and of that kind.
        let said = with(&FACTORY.replace("veyra:blueprint.production", "veyra:blueprint.gone"))
            .expect_err("no such item")
            .to_string();
        assert!(said.contains("which is no item"), "{said}");
        let said =
            with(&FACTORY.replace("veyra:blueprint.production", "veyra:component.cell_core"))
                .expect_err("a component")
                .to_string();
        assert!(said.contains("a component"), "{said}");
    }

    #[test]
    fn a_derate_off_its_steps_is_refused() {
        let rules = FactoryRules::parse(FACTORY).expect("the dials");
        for on in [700, 750, 850, 950, 1000] {
            assert!(rules.allows(on), "{on}");
        }
        for off in [0, 650, 699, 701, 849, 975, 1001, 1050] {
            assert!(!rules.allows(off), "{off}");
        }
        // The most is a step from the least, or the prototype's own figures
        // could not be asked for.
        let said = unruled("step: 0.05", "step: 0.04");
        assert!(said.contains("a whole number of steps apart"), "{said}");
        let said = unruled("from: 0.70", "from: 1.00");
        assert!(said.contains("the least is under the most"), "{said}");
        for (from, to) in [
            ("from: 0.70", "from: 0.0"),
            ("to: 1.00", "to: 1.20"),
            ("step: 0.05", "step: 0.0"),
            ("step: 0.05", "step: 0.0504"),
        ] {
            let said = unruled(from, to);
            assert!(said.contains("in thousandths"), "{said}");
        }
        // A single step is a slider of two stops.
        let two =
            FactoryRules::parse(&FACTORY.replace("step: 0.05", "step: 0.30")).expect("one step");
        assert!(two.allows(700) && two.allows(1000) && !two.allows(850));
    }

    #[test]
    fn runs_cover_every_complexity() {
        let registries = shipped();
        let set = |factory: &str| {
            let def: SchematicDef = ron::from_str(CORE).expect("parses");
            SchematicSet::build(
                vec![("schematics/core.schematic.ron".to_owned(), def)],
                SessionRules::parse(SESSION).expect("the dials stand"),
                &registries.items,
                &registries.resources,
            )
            .expect("the schematic stands")
            .with_factory(
                FactoryRules::parse(factory).expect("the dials parse"),
                &registries.items,
            )
        };
        assert!(set(FACTORY).is_ok());
        // The session gives points for three complexities, so three have
        // runs: a schematic of the third could else be recorded and never
        // run.
        let said = set(&FACTORY.replace("[100, 50, 25]", "[100, 50]"))
            .expect_err("two of three")
            .to_string();
        assert!(
            said.contains("runs gives 2 complexities") && said.contains(SESSION_FILE),
            "{said}"
        );
        assert!(set(&FACTORY.replace("[100, 50, 25]", "[100, 50, 25, 10]")).is_ok());
        let said = unruled("[100, 50, 25]", "[100, 0, 25]");
        assert!(said.contains("complexity 2 starts with no runs"), "{said}");
        let said = unruled("[100, 50, 25]", "[]");
        assert!(said.contains("runs is empty"), "{said}");
    }

    #[test]
    fn a_unit_is_whole_ticks() {
        let ticks = |seconds: &str| {
            FactoryRules::parse(&FACTORY.replace("seconds_per_point: 12.0", seconds))
                .map(|rules| rules.unit_ticks(1))
        };
        assert_eq!(ticks("seconds_per_point: 12.0"), Ok(240));
        assert_eq!(ticks("seconds_per_point: 0.05"), Ok(1), "one tick");
        assert_eq!(ticks("seconds_per_point: 2.5"), Ok(50));
        for off in ["0.0", "0.01", "0.12", "12.02", "-1.0"] {
            let said = unruled(
                "seconds_per_point: 12.0",
                &format!("seconds_per_point: {off}"),
            );
            assert!(said.contains("no whole number"), "{off}: {said}");
        }
        // A unit is its complexity's points of them, and a complexity of
        // nought, which no schematic has, is read as one.
        let rules = FactoryRules::parse(FACTORY).expect("the dials");
        assert_eq!(rules.unit_ticks(3), 720);
        assert_eq!(rules.unit_ticks(0), 240);
    }

    /// The loader names the file at fault, for a schematic and for the
    /// dials, and two files of one id are one too many.
    #[test]
    fn a_refusal_names_its_file() {
        let registries = shipped();
        let load = |source: &InMemorySource| {
            crate::load::schematics(source, &registries.items, &registries.resources)
        };
        let good = InMemorySource::new()
            .with(SESSION_FILE, SESSION)
            .with("schematics/core.schematic.ron", CORE)
            .with("schematics/feed.schematic.ron", FEED)
            .with("schematics/notes.txt", "not a schematic");
        assert_eq!(load(&good).expect("loads").len(), 2);
        assert!(
            load(&InMemorySource::new()).expect("nothing").is_empty(),
            "no directory is no schematics"
        );

        let source = InMemorySource::new()
            .with(SESSION_FILE, SESSION)
            .with("schematics/core.schematic.ron", CORE)
            .with(
                "schematics/feed.schematic.ron",
                FEED.replace("weight: 0.60", "weight: 0.50"),
            );
        let error = load(&source).expect_err("a line sums short");
        let LoadError::Schematics { path, source } = &error else {
            panic!("{error}");
        };
        assert_eq!(path, "schematics/feed.schematic.ron");
        assert!(matches!(**source, SchematicError::Schematic { .. }));
        assert!(
            error.to_string().contains("schematics/feed.schematic.ron")
                && error.to_string().contains("test:schematic.feed"),
            "{error}"
        );

        let source = InMemorySource::new()
            .with(
                SESSION_FILE,
                SESSION.replace("assembly: 0.6", "assembly: 2.0"),
            )
            .with("schematics/core.schematic.ron", CORE);
        let error = load(&source).expect_err("the dials are wrong");
        assert!(
            matches!(&error, LoadError::Schematics { path, .. } if path == SESSION_FILE),
            "{error}"
        );

        let source = InMemorySource::new()
            .with(SESSION_FILE, SESSION)
            .with("schematics/core.schematic.ron", CORE)
            .with("schematics/core_again.schematic.ron", CORE);
        let error = load(&source).expect_err("one id, two files");
        let LoadError::Schematics { source, .. } = &error else {
            panic!("{error}");
        };
        assert_eq!(
            **source,
            SchematicError::Duplicate(id("test:schematic.core"))
        );

        let source = InMemorySource::new()
            .with(SESSION_FILE, SESSION)
            .with("schematics/core.schematic.ron", "(id: 3)");
        let error = load(&source).expect_err("not a schematic");
        assert!(
            matches!(&error, LoadError::Schematics { path, source }
                if path == "schematics/core.schematic.ron" && matches!(**source, SchematicError::Ron(_))),
            "{error}"
        );
    }
}
