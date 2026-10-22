//! The skill graph of `docs/design/progression.md`, as
//! `assets/progression/trees.ron` spells it, and the authored character
//! sheets beside it (`opponents.ron`, `player.ron`, `household.ron`).
//!
//! The graph is content: trees, branches, boxes, costs, XP gates and each
//! box's effects over the combat tables. Nothing here knows what a field
//! does; it knows the vocabulary ([`ACTION_FIELDS`], [`WEAPON_FIELDS`],
//! [`ACTOR_FIELDS`], [`ACTIONS`], [`WEAPONS`]) so a box that names a field
//! the sim has no such thing as is refused at load, not discovered when
//! the box is learned. `veyra-sim` holds the sheet component, the rules
//! of learning and the merge of effects into an actor's numbers; a test
//! there holds this vocabulary to the sim's real structs.
//!
//! The shape is fixed by the design and checked here: a novice box, four
//! branches of four, a master, costs 6 / 2 4 6 8 / 12, 98 points a tree.
//! An elite tree's novice names the boxes of two basic trees it needs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::{GRID_MAX, GRID_MIN, Reading};

/// The kinds of XP, one per activity. A box needs the kind its tree is
/// about, earned by doing the thing, so a fighter who crafts on the side
/// is a fighter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum XpKind {
    /// Bolts that land.
    Ranged,
    /// Cuts that land.
    Blade,
    /// Deposits found, stations read.
    Survey,
    /// Deliveries, deals.
    Trade,
    /// Batches made.
    Fabrication,
    /// Machines kept running.
    Machine,
    /// Patients treated.
    Medical,
    /// The basin lived off.
    Wilderness,
}

impl XpKind {
    /// Every kind, for a ledger to be checked against.
    pub const ALL: [Self; 8] = [
        Self::Ranged,
        Self::Blade,
        Self::Survey,
        Self::Trade,
        Self::Fabrication,
        Self::Machine,
        Self::Medical,
        Self::Wilderness,
    ];

    /// The kind's name, lower case, for a HUD line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ranged => "ranged",
            Self::Blade => "blade",
            Self::Survey => "survey",
            Self::Trade => "trade",
            Self::Fabrication => "fabrication",
            Self::Machine => "machine",
            Self::Medical => "medical",
            Self::Wilderness => "wilderness",
        }
    }
}

impl std::fmt::Display for XpKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// How an effect changes a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// Replace it.
    Set,
    /// Add to it (negative to subtract).
    Add,
    /// Scale it.
    Mul,
}

impl Op {
    /// `current` after this op with `value`.
    #[must_use]
    pub fn apply(self, current: f32, value: f32) -> f32 {
        match self {
            Self::Set => value,
            Self::Add => current + value,
            Self::Mul => current * value,
        }
    }
}

/// What a learned box does to an actor's numbers. A small typed
/// vocabulary: the sim merges these, nothing else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// Change a field of an action's row.
    Action {
        /// The action, by its table name (`Light`, `Heavy`, ...).
        action: String,
        /// The field, one of [`ACTION_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a field of a weapon's row.
    Weapon {
        /// The weapon, by its table name (`Carbine`, `Pistol`).
        weapon: String,
        /// The field, one of [`WEAPON_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a field of the actor table.
    Actor {
        /// The field, one of [`ACTOR_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// An action the actor cannot begin until a box unlocks it. An action
    /// no box in the graph unlocks is open to everyone.
    Unlock {
        /// The action.
        action: String,
    },
    /// A weapon the actor cannot fire or draw until a box certifies it. A
    /// weapon no box certifies is open to everyone.
    Certify {
        /// The weapon.
        weapon: String,
    },
    /// More experimentation points in every crafting session the holder
    /// opens, on top of the dials' for the schematic's complexity. Allowed
    /// in any tree.
    CraftPoints {
        /// How many more.
        add: u8,
    },
    /// Change a figure of the survey tool's dials (`survey.ron`). Allowed in
    /// any tree. Effects of one field apply in the order Set, then Add, then
    /// Mul, whatever order the boxes were learned in, so a result never
    /// depends on that order.
    Survey {
        /// The field, one of [`SURVEY_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// A ping reads a deposit at least this well; the best of the boxes
    /// learned and the dials' untrained reading is what a ping tells.
    Read {
        /// How well.
        reading: Reading,
    },
    /// A ping also tells each met deposit's centre, so the lead goes to the
    /// middle of the richest ground and the row says how far and which way.
    Peak,
    /// Change a figure of the bike's dials (`*.vehicle.ron`) for the rider.
    /// Allowed in any tree; ordered like [`Effect::Survey`].
    Ride {
        /// The field, one of [`RIDE_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a figure of the bench's dials (`session.ron`) for the member at
    /// the bench. Allowed in any tree; ordered like [`Effect::Survey`].
    Craft {
        /// The field, one of [`CRAFT_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a figure of the fabricator's dials (`factory.ron`) for the
    /// member who records or starts a run. Allowed in any tree; ordered like
    /// [`Effect::Survey`].
    Factory {
        /// The field, one of [`FACTORY_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a figure of the wear dials (`wear.ron`) or of how a repair is
    /// done. Allowed in any tree; ordered like [`Effect::Survey`].
    Wear {
        /// The field, one of [`WEAR_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a figure of the field harvester for the member who plants one
    /// (how many the household may have, its rate, its hopper). Allowed in
    /// any tree; ordered like [`Effect::Survey`].
    Station {
        /// The field, one of [`STATION_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// The holder knows this active skill (a row of `abilities.ron`) and
    /// may put it on a bar. That the row exists is checked where the graph
    /// meets the combat tables, not here.
    Grant {
        /// The ability's id.
        ability: String,
    },
    /// Change a figure of a known ability's row for its bearer. Allowed in
    /// any tree; ordered like [`Effect::Survey`].
    Ability {
        /// The ability's id.
        id: String,
        /// The field, one of [`ABILITY_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
    /// Change a figure of the guard row of the melee class in hand
    /// (`melee.ron`). Allowed in any tree; ordered like [`Effect::Survey`].
    Guard {
        /// The field, one of [`GUARD_FIELDS`].
        field: String,
        /// How.
        op: Op,
        /// By what.
        value: f32,
    },
}

/// One box: a step of a branch, the novice or the master.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxDef {
    /// `tree.branch.n`, `tree.novice` or `tree.master`; unique in the graph.
    pub id: String,
    /// What the UI calls it.
    pub name: String,
    /// One line, plain language, what it does.
    pub description: String,
    /// Skill points.
    pub cost: u16,
    /// XP of the tree's kind needed to learn it.
    pub xp: u32,
    /// What it does to the numbers.
    #[serde(default)]
    pub effects: Vec<Effect>,
}

/// Four boxes up one line of a tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchDef {
    /// `tree.branch`.
    pub id: String,
    /// What the UI calls it.
    pub name: String,
    /// Bottom to top; each needs the one before it.
    pub boxes: Vec<BoxDef>,
}

/// A profession tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeDef {
    /// `blade`, `marksman`, `warden`, ...
    pub id: String,
    /// What the UI calls it.
    pub name: String,
    /// The XP every box of the tree needs.
    pub xp: XpKind,
    /// No box carries an effect yet and nothing earns its XP kind.
    #[serde(default)]
    pub placeholder: bool,
    /// Why it is a placeholder, for the UI to say so.
    #[serde(default)]
    pub note: Option<String>,
    /// Taught at any workbench, not by a trainer prop.
    #[serde(default)]
    pub bench: bool,
    /// Boxes of other trees the novice needs: empty for a basic tree, the
    /// named boxes of two basic trees for an elite.
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// The first box.
    pub novice: BoxDef,
    /// Four of them.
    pub branches: Vec<BranchDef>,
    /// The last box, needing the top of every branch.
    pub master: BoxDef,
}

impl TreeDef {
    /// An elite tree: one with prerequisites in other trees.
    #[must_use]
    pub fn is_elite(&self) -> bool {
        !self.prerequisites.is_empty()
    }

    /// Whether the sheet may offer the tree's boxes: a placeholder is not,
    /// unless a workbench teaches it, since that is the one way to spend
    /// the Fabrication XP that is earned.
    #[must_use]
    pub fn in_game(&self) -> bool {
        !self.placeholder || self.bench
    }

    /// Every box of the tree in graph order: novice, branches bottom to
    /// top, master.
    pub fn boxes(&self) -> impl Iterator<Item = &BoxDef> {
        std::iter::once(&self.novice)
            .chain(self.branches.iter().flat_map(|branch| branch.boxes.iter()))
            .chain(std::iter::once(&self.master))
    }

    /// The sum of the tree's costs.
    #[must_use]
    pub fn total_cost(&self) -> u32 {
        self.boxes().map(|b| u32::from(b.cost)).sum()
    }
}

/// What the XP ledger credits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpRates {
    /// XP per point of health and shield a bolt or a cut takes off.
    pub damage: f32,
    /// XP for the hit that kills.
    pub kill: u32,
    /// Fabrication XP for making things at a bench.
    #[serde(default)]
    pub craft: CraftRates,
    /// Survey XP for surveying by hand.
    #[serde(default)]
    pub survey: SurveyRates,
    /// Wilderness XP for shooting a rifle at range.
    #[serde(default)]
    pub wilderness: WildernessRates,
}

/// What a rifle's shooting earns in Wilderness XP, on top of the combat XP
/// the same hit pays. Only a rifle's bolt counts: the pistol's reach past
/// the distance below pays nothing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildernessRates {
    /// XP per point of health and shield a rifle's bolt takes off.
    pub rifle: f32,
    /// XP on top for a rifle hit that flew `long_m` metres or more.
    pub long: u32,
    /// Metres a hit must have flown to earn `long`.
    pub long_m: f32,
}

/// What surveying by hand earns in Survey XP. An empty ping earns nothing,
/// so standing and pinging is not a way to grind.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurveyRates {
    /// A ping that meets a deposit the household had not found.
    pub find: u32,
    /// A ping that raises a found deposit's best reading by at least
    /// [`SurveyRates::better_pm`].
    pub better: u32,
    /// The first sampling cycle of a deposit.
    pub first_sample: u32,
    /// Each sampling cycle, times its concentration over 1000, at least 1.
    pub cycle: u32,
    /// How many per-mille a reading must rise to count as better.
    pub better_pm: u16,
    /// Every ten units collected from a field harvester, to whoever
    /// collects.
    pub harvest: u32,
}

/// What making things at a bench earns in Fabrication XP.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CraftRates {
    /// A finished session, by the schematic's complexity from 1.
    pub made: Vec<u32>,
    /// The household's first batch of a schematic, on top.
    pub first: u32,
    /// Each line of a batch that beats the household's best for it.
    pub best: u32,
    /// A rework that raised a weapon's moddability.
    pub rework: u32,
}

impl Default for XpRates {
    /// Nothing: a world with no graph credits nothing.
    fn default() -> Self {
        Self {
            damage: 0.,
            kill: 0,
            craft: CraftRates::default(),
            survey: SurveyRates::default(),
            wilderness: WildernessRates::default(),
        }
    }
}

/// `assets/progression/trees.ron`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillGraph {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// Skill points a character has.
    pub budget: u32,
    /// What the ledger credits.
    #[serde(default)]
    pub rates: XpRates,
    /// The trees, in the file's order.
    pub trees: Vec<TreeDef>,
}

/// An authored sheet: boxes already learned and XP already earned, for
/// an opponent or the arrival. Checked against the graph but not against
/// the XP gates: an authored character learned these elsewhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetDef {
    /// Box ids.
    #[serde(default)]
    pub learned: Vec<String>,
    /// XP by kind.
    #[serde(default)]
    pub xp: BTreeMap<XpKind, u32>,
}

/// `assets/progression/opponents.ron`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpponentSheets {
    /// See [`SkillGraph::version`].
    pub version: u32,
    /// By the encounter's name for the opponent: `rusher`, `sentry`.
    pub sheets: BTreeMap<String, SheetDef>,
}

/// `assets/progression/player.ron`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSheet {
    /// See [`SkillGraph::version`].
    pub version: u32,
    /// The arrival's starting sheet.
    pub sheet: SheetDef,
}

/// One character of the starting household as `household.ron` authors
/// it: a name, a body tint the view picks a colour by, and a sheet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberDef {
    /// Shown on the HUD and the bench.
    pub name: String,
    /// An index into the view's tint palette, so two characters of the
    /// household never wear the same colour. Not a colour: the view owns
    /// those.
    #[serde(default)]
    pub tint: u8,
    /// What they know.
    pub sheet: SheetDef,
}

/// `assets/progression/household.ron`: the arrival and the characters
/// that can join the bench in this slice. Acquisition by play (the
/// rescue, the partner, the hire of the design's section 4) is future
/// work; here a recruit joins free at a bench place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HouseholdDef {
    /// See [`SkillGraph::version`].
    pub version: u32,
    /// The household's voice, the character the player starts as.
    pub arrival: MemberDef,
    /// Who can still join, in the order they are offered.
    #[serde(default)]
    pub recruits: Vec<MemberDef>,
}

/// The authored sheets, together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthoredSheets {
    /// The encounter's opponents, by name.
    pub opponents: BTreeMap<String, SheetDef>,
    /// The arrival's; `None` for a data set without one, which starts the
    /// player with nothing learned. The game prefers the household's
    /// arrival when there is one.
    pub player: Option<SheetDef>,
    /// The starting household; `None` for a data set without one, which
    /// has a bench of one and nobody to recruit.
    pub household: Option<HouseholdDef>,
}

/// The actions a box may name: `Action::ALL` of the sim, by name.
pub const ACTIONS: &[&str] = &[
    "Ready", "Light", "Followup", "Heavy", "Dodge", "Reload", "Stagger", "Swap", "Dead",
];
/// The weapons a box may name: `Weapon::ALL` of the sim, by name.
pub const WEAPONS: &[&str] = &[
    "Carbine",
    "Pistol",
    "Blade",
    "Greatblade",
    "Glaive",
    "Rifle",
];
/// The fields of an action's row.
pub const ACTION_FIELDS: &[&str] = &[
    "contact_start",
    "contact_end",
    "duration",
    "dodge_cancel",
    "stamina",
    "damage",
    "posture",
    "travel",
];
/// The fields of a weapon's row.
pub const WEAPON_FIELDS: &[&str] = &[
    "interval",
    "bolt_speed",
    "damage",
    "posture",
    "magazine",
    "reload",
    "spread",
    "aim_spread",
    "move_spread",
    "range",
    "aim_time",
];
/// The fields of the actor table a box may change. `version` is not one.
pub const ACTOR_FIELDS: &[&str] = &[
    "health",
    "shield",
    "stamina",
    "posture",
    "stamina_regen",
    "shield_regen",
    "posture_regen",
    "recovery_wait",
    "hurt_wait",
    "buffer",
    "combo_window",
    "aim_speed_scale",
    "walk_speed",
    "sprint_speed",
    "jump_speed",
    "gravity",
];

/// The survey dials a box may change.
pub const SURVEY_FIELDS: &[&str] = &["range_m", "grid", "channel_s", "units", "cycle_s"];
/// The bike's dials a box may change.
pub const RIDE_FIELDS: &[&str] = &["call_s", "speed_road", "speed_ground", "stop_deg", "grip"];

/// The bench's dials a box may change.
pub const CRAFT_FIELDS: &[&str] = &["assembly"];
/// The fabricator's dials a box may change.
pub const FACTORY_FIELDS: &[&str] = &["seconds", "runs", "unattended"];
/// The wear dials a box may change: what a field repair restores, what a
/// shot and a cut cost the bearer's weapon, and whether a workbench repair
/// needs a kit (a Set to nought says it needs none).
pub const WEAR_FIELDS: &[&str] = &["field", "cost", "bench_kit"];

/// The field harvester's figures a box may change: how many the planter
/// may have planted, and the rate and hopper of one planted.
pub const STATION_FIELDS: &[&str] = &["count", "rate", "hopper"];

/// The figures of an ability a box may change; one that the ability's effect
/// does not have is left as it is.
pub const ABILITY_FIELDS: &[&str] = &[
    "stamina", "cooldown", "windup", "length", "damage", "posture", "seconds",
];

/// The figures of a class's guard row a `Guard` effect may change.
pub const GUARD_FIELDS: &[&str] = &["share", "arc", "stamina", "posture", "parry", "riposte"];

/// The costs the design fixes: novice, the four steps of a branch, master.
pub const NOVICE_COST: u16 = 6;
/// See [`NOVICE_COST`].
pub const BRANCH_COSTS: [u16; 4] = [2, 4, 6, 8];
/// See [`NOVICE_COST`].
pub const MASTER_COST: u16 = 12;
/// What a whole tree costs: 6 + 4 * 20 + 12.
pub const TREE_COST: u32 = 98;
/// Branches a tree has, and boxes a branch has.
pub const BRANCHES: usize = 4;

/// Why a graph or a sheet was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProgressionError {
    /// Two boxes, branches or trees share an id.
    #[error("`{0}` is declared twice")]
    DuplicateId(String),
    /// A tree has not four branches of four.
    #[error("tree `{tree}`: {reason}")]
    Shape {
        /// The tree.
        tree: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A box costs other than the design says.
    #[error("box `{id}` costs {cost}, the design says {expected}")]
    Cost {
        /// The box.
        id: String,
        /// Its cost.
        cost: u16,
        /// What it should cost.
        expected: u16,
    },
    /// A tree's costs do not sum to [`TREE_COST`].
    #[error("tree `{tree}` costs {total} points, a tree is {TREE_COST}")]
    TreeTotal {
        /// The tree.
        tree: String,
        /// Its total.
        total: u32,
    },
    /// A prerequisite names no box.
    #[error("tree `{tree}` requires `{prerequisite}`, which is no box")]
    UnknownPrerequisite {
        /// The tree.
        tree: String,
        /// The missing id.
        prerequisite: String,
    },
    /// An elite names a box of its own tree.
    #[error("tree `{tree}` requires its own box `{prerequisite}`")]
    SelfPrerequisite {
        /// The tree.
        tree: String,
        /// The box.
        prerequisite: String,
    },
    /// An elite's prerequisites do not span two basic trees: the design
    /// makes an elite a meeting of two professions, not a deeper one.
    #[error("tree `{tree}` requires boxes of {found}; an elite needs boxes of two basic trees")]
    ElitePrerequisites {
        /// The tree.
        tree: String,
        /// What the prerequisites covered, for the message.
        found: String,
    },
    /// A box moves the survey grid other than by a Set to an odd number the
    /// ping can draw.
    #[error("box `{id}` moves the survey grid other than by a Set to an odd number, 3 to 33")]
    EvenGrid {
        /// The box.
        id: String,
    },
    /// A survey or ride effect with a value no dial can take.
    #[error("box `{id}` has a value that is not finite, or a Set or Mul that is not over nought")]
    BadValue {
        /// The box.
        id: String,
    },
    /// A rate that would pay for nothing.
    #[error("rates.survey.better_pm is nought while `better` pays")]
    NoBetterStep,
    /// An effect names an action, weapon or field the sim has not got.
    #[error("box `{id}`: {what} `{name}` is not one the sim has")]
    UnknownName {
        /// The box.
        id: String,
        /// `action`, `weapon` or `field`.
        what: &'static str,
        /// The name.
        name: String,
    },
    /// The budget is below one tree, which the design's arithmetic needs.
    #[error("the budget is {budget} points, below one tree of {TREE_COST}")]
    Budget {
        /// The budget.
        budget: u32,
    },
    /// A sheet learned a box the graph has not got.
    #[error("sheet `{sheet}` learned `{id}`, which is no box")]
    SheetUnknownBox {
        /// The sheet.
        sheet: String,
        /// The id.
        id: String,
    },
    /// A sheet learned a box without its prerequisite.
    #[error("sheet `{sheet}` learned `{id}` without `{prerequisite}`")]
    SheetMissingPrerequisite {
        /// The sheet.
        sheet: String,
        /// The box.
        id: String,
        /// What it needs.
        prerequisite: String,
    },
    /// A sheet learned a box without the XP its gate asks. Forgetting is
    /// free and XP is kept, so a box held under its gate could never be
    /// learned back: a silent permanent loss, which the design has none of.
    #[error("sheet `{sheet}` learned `{id}`, which needs {need} {kind} XP; the sheet has {have}")]
    SheetXp {
        /// The sheet.
        sheet: String,
        /// The box.
        id: String,
        /// The kind.
        kind: XpKind,
        /// What the sheet has.
        have: u32,
        /// What the box needs.
        need: u32,
    },
    /// A sheet spends more than the budget.
    #[error("sheet `{sheet}` spends {spent} points of {budget}")]
    SheetOverBudget {
        /// The sheet.
        sheet: String,
        /// Its total.
        spent: u32,
        /// The graph's budget.
        budget: u32,
    },
}

#[cfg(any(test, feature = "shipped-data"))]
const TREES: &str = include_str!("../../../assets/progression/trees.ron");
#[cfg(any(test, feature = "shipped-data"))]
const OPPONENTS: &str = include_str!("../../../assets/progression/opponents.ron");
#[cfg(any(test, feature = "shipped-data"))]
const PLAYER: &str = include_str!("../../../assets/progression/player.ron");
#[cfg(any(test, feature = "shipped-data"))]
const HOUSEHOLD: &str = include_str!("../../../assets/progression/household.ron");

impl SkillGraph {
    /// The file under the asset root.
    pub const FILE: &'static str = "progression/trees.ron";

    /// The graph as shipped in `assets/progression/trees.ron`, compiled
    /// in, so a test or a tool with no assets directory has it.
    ///
    /// # Panics
    /// If the shipped file does not parse or validate, which this crate's
    /// tests catch before a build ships.
    #[cfg(any(test, feature = "shipped-data"))]
    #[must_use]
    pub fn shipped() -> Self {
        Self::parse(TREES).expect("the shipped skill graph parses and validates")
    }

    /// Parse and validate the file's text.
    ///
    /// # Errors
    /// RON that does not fit, as its message; or what [`SkillGraph::validate`] refuses.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let graph: Self = ron::from_str(text).map_err(|e| ParseError::Ron(e.to_string()))?;
        graph.validate()?;
        Ok(graph)
    }

    /// Check the graph against the design: unique ids, the shape, the
    /// costs, prerequisites that exist, effects that name real things.
    ///
    /// # Errors
    /// The first thing wrong.
    #[allow(clippy::too_many_lines, reason = "one pass over every rule")]
    pub fn validate(&self) -> Result<(), ProgressionError> {
        if self.budget < TREE_COST {
            return Err(ProgressionError::Budget {
                budget: self.budget,
            });
        }
        if self.rates.survey.better > 0 && self.rates.survey.better_pm == 0 {
            return Err(ProgressionError::NoBetterStep);
        }
        let mut ids = BTreeSet::new();
        let mut claim = |id: &str| {
            if ids.insert(id.to_owned()) {
                Ok(())
            } else {
                Err(ProgressionError::DuplicateId(id.to_owned()))
            }
        };
        for tree in &self.trees {
            claim(&tree.id)?;
            // The total before the per-box costs: a tree that is the right
            // sum of wrong costs is refused by the next rule.
            let total = tree.total_cost();
            if total != TREE_COST {
                return Err(ProgressionError::TreeTotal {
                    tree: tree.id.clone(),
                    total,
                });
            }
            if tree.branches.len() != BRANCHES {
                return Err(ProgressionError::Shape {
                    tree: tree.id.clone(),
                    reason: format!("{} branches, a tree has {BRANCHES}", tree.branches.len()),
                });
            }
            for branch in &tree.branches {
                claim(&branch.id)?;
                if branch.boxes.len() != BRANCHES {
                    return Err(ProgressionError::Shape {
                        tree: tree.id.clone(),
                        reason: format!(
                            "branch `{}` has {} boxes, a branch has {BRANCHES}",
                            branch.id,
                            branch.boxes.len()
                        ),
                    });
                }
                for (step, b) in branch.boxes.iter().enumerate() {
                    claim(&b.id)?;
                    expect_cost(b, BRANCH_COSTS[step])?;
                }
            }
            claim(&tree.novice.id)?;
            claim(&tree.master.id)?;
            expect_cost(&tree.novice, NOVICE_COST)?;
            expect_cost(&tree.master, MASTER_COST)?;
        }
        for tree in &self.trees {
            let mut owners: BTreeSet<&str> = BTreeSet::new();
            for prerequisite in &tree.prerequisites {
                let Some(owner) = self.tree_of(prerequisite) else {
                    return Err(ProgressionError::UnknownPrerequisite {
                        tree: tree.id.clone(),
                        prerequisite: prerequisite.clone(),
                    });
                };
                if owner.id == tree.id {
                    return Err(ProgressionError::SelfPrerequisite {
                        tree: tree.id.clone(),
                        prerequisite: prerequisite.clone(),
                    });
                }
                owners.insert(owner.id.as_str());
            }
            let elite_owner = |id: &str| self.tree(id).is_some_and(TreeDef::is_elite);
            if tree.is_elite() && (owners.len() != 2 || owners.iter().any(|o| elite_owner(o))) {
                return Err(ProgressionError::ElitePrerequisites {
                    tree: tree.id.clone(),
                    found: owners
                        .iter()
                        .map(|owner| {
                            if elite_owner(owner) {
                                format!("the elite `{owner}`")
                            } else {
                                format!("`{owner}`")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" and "),
                });
            }
            for b in tree.boxes() {
                for effect in &b.effects {
                    check_effect(&b.id, effect)?;
                }
            }
        }
        Ok(())
    }

    /// Every box in graph order: tree by tree, novice, branches bottom to
    /// top, master.
    pub fn boxes(&self) -> impl Iterator<Item = &BoxDef> {
        self.trees.iter().flat_map(TreeDef::boxes)
    }

    /// The box called `id`.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&BoxDef> {
        self.boxes().find(|b| b.id == id)
    }

    /// The tree called `id`.
    #[must_use]
    pub fn tree(&self, id: &str) -> Option<&TreeDef> {
        self.trees.iter().find(|t| t.id == id)
    }

    /// The tree the box `id` belongs to.
    #[must_use]
    pub fn tree_of(&self, id: &str) -> Option<&TreeDef> {
        self.trees.iter().find(|t| t.boxes().any(|b| b.id == id))
    }

    /// The box called `id` with the tree it belongs to, so a rule that
    /// needs the tree's XP kind has it without a fallback kind.
    #[must_use]
    pub fn find_with_tree(&self, id: &str) -> Option<(&TreeDef, &BoxDef)> {
        self.trees
            .iter()
            .find_map(|t| t.boxes().find(|b| b.id == id).map(|b| (t, b)))
    }

    /// What learning `id` needs learned first: the tree's prerequisites
    /// for a novice, the novice for a branch's first box, the step below
    /// for the rest, the top of every branch for a master. Empty for an
    /// unknown id.
    #[must_use]
    pub fn prerequisites(&self, id: &str) -> Vec<&str> {
        let Some(tree) = self.tree_of(id) else {
            return Vec::new();
        };
        if tree.novice.id == id {
            return tree.prerequisites.iter().map(String::as_str).collect();
        }
        if tree.master.id == id {
            return tree
                .branches
                .iter()
                .filter_map(|branch| branch.boxes.last())
                .map(|b| b.id.as_str())
                .collect();
        }
        for branch in &tree.branches {
            if let Some(step) = branch.boxes.iter().position(|b| b.id == id) {
                return vec![if step == 0 {
                    tree.novice.id.as_str()
                } else {
                    branch.boxes[step - 1].id.as_str()
                }];
            }
        }
        Vec::new()
    }

    /// The boxes that need `id` learned: what forgetting it would break.
    #[must_use]
    pub fn dependents(&self, id: &str) -> Vec<&str> {
        self.boxes()
            .filter(|b| self.prerequisites(&b.id).contains(&id))
            .map(|b| b.id.as_str())
            .collect()
    }

    /// The actions some box unlocks: closed until learned. Any other
    /// action is open to everyone.
    #[must_use]
    pub fn gated_actions(&self) -> BTreeSet<&str> {
        self.boxes()
            .flat_map(|b| b.effects.iter())
            .filter_map(|effect| match effect {
                Effect::Unlock { action } => Some(action.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The weapons some box certifies: closed until learned.
    #[must_use]
    pub fn gated_weapons(&self) -> BTreeSet<&str> {
        self.boxes()
            .flat_map(|b| b.effects.iter())
            .filter_map(|effect| match effect {
                Effect::Certify { weapon } => Some(weapon.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Check an authored sheet: every box exists, every box's
    /// prerequisites are in the sheet too, the sheet's XP of the tree's
    /// kind meets every box's gate, and the costs fit the budget. The XP
    /// gate is checked because forgetting is free and XP is kept: a box
    /// held under its gate could be forgotten and never learned back,
    /// and the design has no permanent loss.
    ///
    /// # Errors
    /// The first thing wrong, naming `name`.
    pub fn check_sheet(&self, name: &str, sheet: &SheetDef) -> Result<(), ProgressionError> {
        let learned: BTreeSet<&str> = sheet.learned.iter().map(String::as_str).collect();
        let mut spent = 0;
        for id in &learned {
            let Some((tree, b)) = self.find_with_tree(id) else {
                return Err(ProgressionError::SheetUnknownBox {
                    sheet: name.to_owned(),
                    id: (*id).to_owned(),
                });
            };
            let have = sheet.xp.get(&tree.xp).copied().unwrap_or(0);
            if have < b.xp {
                return Err(ProgressionError::SheetXp {
                    sheet: name.to_owned(),
                    id: (*id).to_owned(),
                    kind: tree.xp,
                    have,
                    need: b.xp,
                });
            }
            spent += u32::from(b.cost);
            for prerequisite in self.prerequisites(id) {
                if !learned.contains(prerequisite) {
                    return Err(ProgressionError::SheetMissingPrerequisite {
                        sheet: name.to_owned(),
                        id: (*id).to_owned(),
                        prerequisite: prerequisite.to_owned(),
                    });
                }
            }
        }
        if spent > self.budget {
            return Err(ProgressionError::SheetOverBudget {
                sheet: name.to_owned(),
                spent,
                budget: self.budget,
            });
        }
        Ok(())
    }
}

impl AuthoredSheets {
    /// The opponents' file under the asset root.
    pub const OPPONENTS_FILE: &'static str = "progression/opponents.ron";
    /// The player's file under the asset root.
    pub const PLAYER_FILE: &'static str = "progression/player.ron";
    /// The household's file under the asset root.
    pub const HOUSEHOLD_FILE: &'static str = "progression/household.ron";

    /// The sheets as shipped, compiled in and checked against the shipped
    /// graph.
    ///
    /// # Panics
    /// If a shipped file does not parse or check, which this crate's tests
    /// catch before a build ships.
    #[cfg(any(test, feature = "shipped-data"))]
    #[must_use]
    pub fn shipped() -> Self {
        Self::parse(
            &SkillGraph::shipped(),
            Some(OPPONENTS),
            Some(PLAYER),
            Some(HOUSEHOLD),
        )
        .expect("the shipped sheets parse and check against the shipped graph")
    }

    /// Parse the three files' text, any absent, and check every sheet
    /// against `graph`. A loader with the files' paths parses each with
    /// [`AuthoredSheets::parse_opponents`] and friends instead, so an
    /// error names the file it is in.
    ///
    /// # Errors
    /// RON that does not fit, or a sheet the graph refuses.
    pub fn parse(
        graph: &SkillGraph,
        opponents: Option<&str>,
        player: Option<&str>,
        household: Option<&str>,
    ) -> Result<Self, ParseError> {
        Ok(Self {
            opponents: opponents.map_or_else(
                || Ok(BTreeMap::new()),
                |text| Self::parse_opponents(graph, text),
            )?,
            player: player
                .map(|text| Self::parse_player(graph, text))
                .transpose()?,
            household: household
                .map(|text| Self::parse_household(graph, text))
                .transpose()?,
        })
    }

    /// `opponents.ron`: the sheets by name, each checked.
    ///
    /// # Errors
    /// RON that does not fit, or a sheet the graph refuses.
    pub fn parse_opponents(
        graph: &SkillGraph,
        text: &str,
    ) -> Result<BTreeMap<String, SheetDef>, ParseError> {
        let file: OpponentSheets =
            ron::from_str(text).map_err(|e| ParseError::Ron(e.to_string()))?;
        for (name, sheet) in &file.sheets {
            graph.check_sheet(name, sheet)?;
        }
        Ok(file.sheets)
    }

    /// `player.ron`: the arrival's sheet, checked.
    ///
    /// # Errors
    /// RON that does not fit, or a sheet the graph refuses.
    pub fn parse_player(graph: &SkillGraph, text: &str) -> Result<SheetDef, ParseError> {
        let file: PlayerSheet = ron::from_str(text).map_err(|e| ParseError::Ron(e.to_string()))?;
        graph.check_sheet("player", &file.sheet)?;
        Ok(file.sheet)
    }

    /// `household.ron`: the arrival and the recruits, each checked.
    ///
    /// # Errors
    /// RON that does not fit, or a sheet the graph refuses.
    pub fn parse_household(graph: &SkillGraph, text: &str) -> Result<HouseholdDef, ParseError> {
        let file: HouseholdDef = ron::from_str(text).map_err(|e| ParseError::Ron(e.to_string()))?;
        for member in std::iter::once(&file.arrival).chain(&file.recruits) {
            graph.check_sheet(&member.name, &member.sheet)?;
        }
        Ok(file)
    }
}

/// Why a progression file could not be read into its type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// Valid RON, refused by the rules.
    #[error("{0}")]
    Invalid(#[from] ProgressionError),
}

fn expect_cost(b: &BoxDef, expected: u16) -> Result<(), ProgressionError> {
    if b.cost == expected {
        Ok(())
    } else {
        Err(ProgressionError::Cost {
            id: b.id.clone(),
            cost: b.cost,
            expected,
        })
    }
}

/// A dial value that cannot be zero, negative or not a number: a Set or Mul
/// to nought would stop a channel or a motor, and a NaN poisons a fold.
fn check_value(id: &str, op: Op, value: f32) -> Result<(), ProgressionError> {
    let ok = value.is_finite() && (op == Op::Add || value > 0.0);
    if ok {
        Ok(())
    } else {
        Err(ProgressionError::BadValue { id: id.to_owned() })
    }
}

/// The shape of a `Grant` or an `Ability` effect: a snake case id, and for
/// the second a known field and a value that can be applied. That the id
/// names a row of `abilities.ron` is checked where the two files meet.
fn check_ability_effect(id: &str, effect: &Effect) -> Result<(), ProgressionError> {
    let unknown = |what: &'static str, name: &str| ProgressionError::UnknownName {
        id: id.to_owned(),
        what,
        name: name.to_owned(),
    };
    let snake = |ability: &str| {
        !ability.is_empty()
            && ability
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    match effect {
        Effect::Grant { ability } if !snake(ability) => Err(unknown("ability", ability)),
        Effect::Ability {
            id: ability,
            field,
            op,
            value,
        } => {
            if ability.is_empty() {
                return Err(unknown("ability", ability));
            }
            if !ABILITY_FIELDS.contains(&field.as_str()) {
                return Err(unknown("field", field));
            }
            check_value(id, *op, *value)
        }
        _ => Ok(()),
    }
}

fn check_effect(id: &str, effect: &Effect) -> Result<(), ProgressionError> {
    let known = |what: &'static str, name: &str, among: &[&str]| {
        if among.contains(&name) {
            Ok(())
        } else {
            Err(ProgressionError::UnknownName {
                id: id.to_owned(),
                what,
                name: name.to_owned(),
            })
        }
    };
    match effect {
        Effect::Action { action, field, .. } => {
            known("action", action, ACTIONS)?;
            known("field", field, ACTION_FIELDS)
        }
        Effect::Weapon { weapon, field, .. } => {
            known("weapon", weapon, WEAPONS)?;
            known("field", field, WEAPON_FIELDS)
        }
        Effect::Actor { field, .. } => known("field", field, ACTOR_FIELDS),
        Effect::Unlock { action } => known("action", action, ACTIONS),
        Effect::Certify { weapon } => known("weapon", weapon, WEAPONS),
        Effect::CraftPoints { .. } | Effect::Read { .. } | Effect::Peak => Ok(()),
        Effect::Survey { field, op, value } => {
            known("field", field, SURVEY_FIELDS)?;
            // The grid is odd so a point is the player's own, and a side the
            // ping can draw: only a Set to such a number keeps both.
            if field == "grid" {
                let side = f64::from(*value);
                let whole = side.fract() == 0.0 && side >= 0.0 && side <= f64::from(u32::MAX);
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked whole and in range just above"
                )]
                let side = side as u32;
                if *op != Op::Set
                    || !whole
                    || side.is_multiple_of(2)
                    || !(GRID_MIN..=GRID_MAX).contains(&side)
                {
                    return Err(ProgressionError::EvenGrid { id: id.to_owned() });
                }
            }
            check_value(id, *op, *value)
        }
        Effect::Ride { field, op, value } => {
            known("field", field, RIDE_FIELDS)?;
            check_value(id, *op, *value)
        }
        Effect::Craft { field, op, value } => {
            known("field", field, CRAFT_FIELDS)?;
            check_value(id, *op, *value)
        }
        Effect::Factory { field, op, value } => {
            known("field", field, FACTORY_FIELDS)?;
            check_value(id, *op, *value)
        }
        Effect::Station { field, op, value } => {
            known("field", field, STATION_FIELDS)?;
            check_value(id, *op, *value)
        }
        Effect::Grant { .. } | Effect::Ability { .. } => check_ability_effect(id, effect),
        Effect::Guard { field, op, value } => {
            known("field", field, GUARD_FIELDS)?;
            check_value(id, *op, *value)
        }
        Effect::Wear { field, op, value } => {
            known("field", field, WEAR_FIELDS)?;
            // A bench kit is a yes or no: only a Set to nought is a meaning.
            if field == "bench_kit" && (*op != Op::Set || *value != 0.0) {
                return Err(ProgressionError::BadValue { id: id.to_owned() });
            }
            if field == "bench_kit" {
                Ok(())
            } else {
                check_value(id, *op, *value)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_graph_is_the_design() {
        let graph = SkillGraph::shipped();
        assert_eq!(graph.budget, 250);
        assert_eq!(graph.trees.len(), 14);
        let basics = [
            "marksman",
            "blade",
            "surveyor",
            "runner",
            "fabricator",
            "rigger",
            "medic",
            "broker",
        ];
        let elites = [
            "warden",
            "ranger",
            "station_engineer",
            "ecologist",
            "assembly_agent",
        ];
        for id in basics {
            let tree = graph.tree(id).expect(id);
            assert!(!tree.is_elite());
            assert_eq!(tree.total_cost(), TREE_COST);
        }
        for id in elites {
            let tree = graph.tree(id).expect(id);
            // The Ranger is real in its Long shots branch alone.
            assert!(
                tree.is_elite() && (tree.placeholder || id == "ranger"),
                "{id}"
            );
            assert_eq!(tree.prerequisites.len(), 5, "{id}: three boxes and two");
            let owners: BTreeSet<&str> = tree
                .prerequisites
                .iter()
                .map(|p| graph.tree_of(p).expect(p).id.as_str())
                .collect();
            assert_eq!(owners.len(), 2, "{id}: boxes of two basic trees");
            assert!(
                owners.iter().all(|o| basics.contains(o)),
                "{id}: {owners:?}"
            );
            // Elite gates are double the basics'.
            assert_eq!(tree.branches[0].boxes[0].xp, 300);
            assert_eq!(tree.placeholder, id != "ranger", "{id}");
            assert_eq!(tree.master.xp, 6000);
        }
        for tree in &graph.trees {
            if tree.placeholder {
                assert!(tree.boxes().all(|b| b.effects.is_empty()), "{}", tree.id);
                assert!(tree.note.is_some(), "{}", tree.id);
            } else if ["weaponsmith", "surveyor", "fabricator", "ranger"]
                .contains(&tree.id.as_str())
            {
                // Real, with the effects they have: the tuning boxes' points,
                // the surveyor's all but the Stations branch and the
                // fabricator's Materials boxes, which teach schematics (the
                // schematic names the box) or wait for the harvesters.
                assert!(tree.boxes().any(|b| !b.effects.is_empty()));
            } else {
                assert!(tree.boxes().all(|b| !b.effects.is_empty()), "{}", tree.id);
            }
            assert_eq!(tree.novice.xp, 0);
            let mult = if tree.is_elite() { 2 } else { 1 };
            for branch in &tree.branches {
                let gates: Vec<u32> = branch.boxes.iter().map(|b| b.xp).collect();
                let mut want = [150 * mult, 400 * mult, 900 * mult, 1600 * mult];
                // The rifle's certificate is the way into Wilderness XP, so
                // it cannot ask for any.
                if branch.id == "ranger.long_shots" {
                    want[0] = 0;
                }
                assert_eq!(gates, want);
            }
            assert_eq!(tree.master.xp, 3000 * mult);
        }
        assert_eq!(graph.boxes().count(), 14 * 18);
    }

    #[test]
    fn the_weaponsmith_is_a_real_elite_of_98_points() {
        let graph = SkillGraph::shipped();
        let tree = graph.tree("weaponsmith").expect("ships");
        assert!(tree.is_elite() && !tree.placeholder);
        assert_eq!(tree.total_cost(), 98);
        assert_eq!(tree.xp, XpKind::Fabrication);
        for n in 1..=4 {
            let tuning = graph
                .find(&format!("weaponsmith.tuning.{n}"))
                .expect("ships");
            assert_eq!(tuning.effects, [Effect::CraftPoints { add: 1 }]);
        }
        let points: Vec<&str> = graph
            .boxes()
            .filter(|b| {
                b.effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::CraftPoints { .. }))
            })
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(
            points,
            [
                "fabricator.batches.1",
                "fabricator.batches.3",
                "fabricator.master",
                "weaponsmith.tuning.1",
                "weaponsmith.tuning.2",
                "weaponsmith.tuning.3",
                "weaponsmith.tuning.4",
            ],
            "the tuning boxes and the fabricator's batches and master"
        );
    }

    #[test]
    fn prerequisites_follow_the_shape() {
        let graph = SkillGraph::shipped();
        assert!(graph.prerequisites("blade.novice").is_empty());
        assert_eq!(graph.prerequisites("blade.cuts.1"), ["blade.novice"]);
        assert_eq!(graph.prerequisites("blade.cuts.3"), ["blade.cuts.2"]);
        assert_eq!(
            graph.prerequisites("blade.master"),
            [
                "blade.cuts.4",
                "blade.guard.4",
                "blade.footwork.4",
                "blade.breaking.4"
            ]
        );
        assert_eq!(graph.prerequisites("warden.novice").len(), 5);
        let mut dependents = graph.dependents("blade.novice");
        dependents.sort_unstable();
        assert_eq!(
            dependents,
            [
                "blade.breaking.1",
                "blade.cuts.1",
                "blade.footwork.1",
                "blade.guard.1"
            ]
        );
        assert_eq!(graph.dependents("blade.cuts.4"), ["blade.master"]);
        assert!(
            graph
                .dependents("marksman.carbine.1")
                .contains(&"warden.novice")
        );
        assert!(graph.prerequisites("nothing").is_empty());
    }

    #[test]
    fn only_what_a_box_opens_is_gated() {
        let graph = SkillGraph::shipped();
        assert_eq!(
            graph.gated_actions().into_iter().collect::<Vec<_>>(),
            ["Followup", "Heavy", "Light"]
        );
        assert_eq!(
            graph.gated_weapons().into_iter().collect::<Vec<_>>(),
            [
                "Blade",
                "Carbine",
                "Glaive",
                "Greatblade",
                "Pistol",
                "Rifle"
            ]
        );
    }

    #[test]
    fn a_broken_prerequisite_is_refused() {
        // The Warden's list, not the box itself, or the rename would be
        // consistent and the graph still whole.
        let text = TREES.replace(
            "\"broker.novice\", \"broker.introductions.1\"]",
            "\"broker.novice\", \"broker.introductions.9\"]",
        );
        assert_ne!(text, TREES);
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::UnknownPrerequisite {
                tree: "warden".into(),
                prerequisite: "broker.introductions.9".into()
            })
        );
    }

    #[test]
    fn an_elite_needs_boxes_of_two_basic_trees() {
        let mut graph = SkillGraph::shipped();
        let index = graph
            .trees
            .iter()
            .position(|t| t.id == "warden")
            .expect("warden");
        // The Warden's five from one tree.
        graph.trees[index].prerequisites = vec![
            "marksman.novice".into(),
            "marksman.carbine.1".into(),
            "marksman.steadiness.1".into(),
            "marksman.pistol.1".into(),
            "marksman.field_craft.1".into(),
        ];
        let error = graph.validate().expect_err("one basic tree");
        assert!(
            matches!(&error, ProgressionError::ElitePrerequisites { tree, .. } if tree == "warden"),
            "{error}"
        );
        // Two trees, one of them an elite.
        graph.trees[index].prerequisites = vec![
            "marksman.novice".into(),
            "marksman.carbine.1".into(),
            "ranger.novice".into(),
        ];
        let error = graph.validate().expect_err("an elite among them");
        assert!(
            matches!(&error, ProgressionError::ElitePrerequisites { found, .. } if found.contains("the elite `ranger`")),
            "{error}"
        );
    }

    #[test]
    fn a_wrong_total_and_a_wrong_cost_are_refused() {
        // The first master, 12 to 13: the tree totals 99.
        let text = TREES.replacen("cost: 12,", "cost: 13,", 1);
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::TreeTotal {
                tree: "marksman".into(),
                total: 99
            })
        );
        // The right total of the wrong costs: Carbine 1 and 2 swapped.
        let mut graph = SkillGraph::shipped();
        let boxes = &mut graph.trees[0].branches[0].boxes;
        boxes[0].cost = 4;
        boxes[1].cost = 2;
        let error = graph.validate().expect_err("refused");
        assert_eq!(
            error,
            ProgressionError::Cost {
                id: "marksman.carbine.1".into(),
                cost: 4,
                expected: 2
            }
        );
        // A fifth box of no cost keeps the total and breaks the shape.
        let mut graph = SkillGraph::shipped();
        graph.trees[0].branches[0].boxes.push(BoxDef {
            id: "marksman.carbine.5".into(),
            name: "Carbine 5".into(),
            description: String::new(),
            cost: 0,
            xp: 0,
            effects: Vec::new(),
        });
        let error = graph.validate().expect_err("five boxes");
        assert!(matches!(error, ProgressionError::Shape { .. }), "{error}");
    }

    #[test]
    fn a_duplicate_id_and_an_unknown_field_are_refused() {
        let text = TREES.replacen("\"blade.cuts.2\"", "\"blade.cuts.1\"", 1);
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::DuplicateId("blade.cuts.1".into()))
        );
        let text = TREES.replacen("field: \"reload\"", "field: \"reloads\"", 1);
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::UnknownName {
                id: "marksman.carbine.2".into(),
                what: "field",
                name: "reloads".into()
            })
        );
    }

    #[test]
    fn the_surveyor_effects_are_checked_and_the_tree_names_them() {
        let text = TREES.replacen("field: \"units\"", "field: \"unit\"", 1);
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::UnknownName {
                id: "surveyor.novice".into(),
                what: "field",
                name: "unit".into()
            })
        );
        let text = TREES.replacen("field: \"call_s\"", "field: \"call\"", 1);
        assert!(SkillGraph::parse(&text).is_err());
        let text = TREES.replacen(
            "field: \"grid\", op: Set, value: 7.0",
            "field: \"grid\", op: Set, value: 6.0",
            1,
        );
        let error = SkillGraph::parse(&text).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::EvenGrid {
                id: "surveyor.reading.3".into()
            })
        );
        let bad = |from: &str, to: &str| {
            let text = TREES.replacen(from, to, 1);
            SkillGraph::parse(&text).expect_err("refused")
        };
        // Grid: only a Set, odd, 3 to 33 and whole.
        let grid = "field: \"grid\", op: Set, value: 7.0";
        for value in ["1.0", "35.0", "7.5"] {
            assert!(
                matches!(
                    bad(grid, &format!("field: \"grid\", op: Set, value: {value}")),
                    ParseError::Invalid(ProgressionError::EvenGrid { .. })
                ),
                "{value}"
            );
        }
        assert!(matches!(
            bad(grid, "field: \"grid\", op: Add, value: 2.0"),
            ParseError::Invalid(ProgressionError::EvenGrid { .. })
        ));
        // A Mul to nought stops a channel; a Set to nought a dial.
        for (from, to) in [
            (
                "field: \"channel_s\", op: Mul, value: 0.8",
                "field: \"channel_s\", op: Mul, value: 0.0",
            ),
            (
                "field: \"call_s\", op: Mul, value: 0.6",
                "field: \"call_s\", op: Set, value: 0.0",
            ),
        ] {
            assert!(matches!(
                bad(from, to),
                ParseError::Invalid(ProgressionError::BadValue { .. })
            ));
        }
        // A better step of nought would pay for every ping.
        assert!(matches!(
            bad("better_pm: 50", "better_pm: 0"),
            ParseError::Invalid(ProgressionError::NoBetterStep)
        ));
        let graph = SkillGraph::shipped();
        let tree = graph.tree("surveyor").expect("ships");
        assert!(!tree.placeholder && tree.note.is_none());
        let named = |pick: fn(&Effect) -> bool| {
            graph
                .boxes()
                .filter(|b| b.id.starts_with("surveyor."))
                .flat_map(|b| &b.effects)
                .filter(|e| pick(e))
                .count()
        };
        assert_eq!(named(|e| matches!(e, Effect::Survey { .. })), 10);
        assert_eq!(named(|e| matches!(e, Effect::Read { .. })), 2);
        assert_eq!(named(|e| matches!(e, Effect::Peak)), 1);
        assert_eq!(named(|e| matches!(e, Effect::Ride { .. })), 4);
        // Stations 1 to 4 count, speed and hold (ticket 0009, step 6).
        let stations: Vec<usize> = (1..=4)
            .map(|n| {
                graph
                    .find(&format!("surveyor.stations.{n}"))
                    .expect("ships")
                    .effects
                    .iter()
                    .filter(|e| matches!(e, Effect::Station { .. }))
                    .count()
            })
            .collect();
        assert_eq!(stations, [1, 1, 1, 2]);
        assert_eq!(named(|e| matches!(e, Effect::Station { .. })), 5);
        for (from, to) in [
            ("field: \"hopper\"", "field: \"capacity\""),
            (
                "field: \"rate\", op: Mul, value: 1.25",
                "field: \"rate\", op: Mul, value: 0.0",
            ),
        ] {
            let text = TREES.replacen(from, to, 1);
            assert_ne!(text, TREES, "`{from}` is in the trees");
            SkillGraph::parse(&text).expect_err("refused");
        }
    }

    #[test]
    fn the_fabricator_effects_are_checked_and_the_tree_names_them() {
        let bad = |from: &str, to: &str| {
            let text = TREES.replacen(from, to, 1);
            assert_ne!(text, TREES, "`{from}` is in the trees");
            SkillGraph::parse(&text).expect_err("refused")
        };
        // An unknown field of each kind.
        for (from, to) in [
            ("field: \"assembly\"", "field: \"assemble\""),
            ("field: \"unattended\"", "field: \"attended\""),
            ("field: \"bench_kit\"", "field: \"kit\""),
        ] {
            assert!(
                matches!(
                    bad(from, to),
                    ParseError::Invalid(ProgressionError::UnknownName { .. })
                ),
                "{from}"
            );
        }
        // A Mul to nought stops a unit; a bench kit is only ever Set to nought.
        for (from, to) in [
            (
                "field: \"seconds\", op: Mul, value: 0.9",
                "field: \"seconds\", op: Mul, value: 0.0",
            ),
            (
                "field: \"cost\", op: Mul, value: 0.8",
                "field: \"cost\", op: Set, value: 0.0",
            ),
            (
                "field: \"bench_kit\", op: Set, value: 0.0",
                "field: \"bench_kit\", op: Set, value: 1.0",
            ),
            (
                "field: \"bench_kit\", op: Set, value: 0.0",
                "field: \"bench_kit\", op: Mul, value: 0.0",
            ),
            (
                "field: \"assembly\", op: Add, value: 0.02",
                "field: \"assembly\", op: Add, value: inf",
            ),
        ] {
            assert!(
                matches!(
                    bad(from, to),
                    ParseError::Invalid(ProgressionError::BadValue { .. })
                ),
                "{from}"
            );
        }
        let graph = SkillGraph::shipped();
        let tree = graph.tree("fabricator").expect("ships");
        assert!(!tree.placeholder && tree.note.is_none() && tree.bench);
        let empty: Vec<&str> = tree
            .boxes()
            .filter(|b| b.effects.is_empty())
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(
            empty,
            [
                "fabricator.materials.1",
                "fabricator.materials.2",
                "fabricator.materials.3",
                "fabricator.materials.4"
            ],
            "the Materials boxes teach schematics, which name them"
        );
    }

    #[test]
    fn the_shipped_sheets_check_and_a_broken_one_does_not() {
        let graph = SkillGraph::shipped();
        let sheets = AuthoredSheets::shipped();
        // The household's surveyor learns her boxes under the file's checks.
        let household = sheets.household.as_ref().expect("ships");
        let ilse = household
            .recruits
            .iter()
            .find(|member| member.name == "Ilse Marrow")
            .expect("ships");
        assert!(
            ilse.sheet
                .learned
                .contains(&"surveyor.reading.1".to_owned())
        );
        assert_eq!(ilse.sheet.xp[&XpKind::Survey], 150);
        graph.check_sheet(&ilse.name, &ilse.sheet).expect("checks");
        // The rusher's Heavy is the novice's; One hand 3 would raise its
        // posture to 92 (ticket 0010, step 8), so its sheet stops at 2.
        let rusher = &sheets.opponents["rusher"].learned;
        assert!(rusher.contains(&"blade.cuts.2".to_owned()));
        assert!(!rusher.contains(&"blade.cuts.3".to_owned()));
        assert!(
            sheets.opponents["sentry"]
                .learned
                .contains(&"marksman.carbine.2".to_owned())
        );
        assert!(sheets.player.is_some());
        let household = sheets.household.as_ref().expect("a shipped household");
        assert_eq!(household.arrival.sheet.learned.len(), 3);
        assert_eq!(household.recruits.len(), 3);
        let tints: BTreeSet<u8> = std::iter::once(&household.arrival)
            .chain(&household.recruits)
            .map(|m| m.tint)
            .collect();
        assert_eq!(
            tints.len(),
            4,
            "every member of the household has its own tint"
        );
        // The rusher's is the first in the file; the duelist's keeps its own.
        let without_novice = OPPONENTS.replacen("\"blade.novice\", ", "", 1);
        let error =
            AuthoredSheets::parse(&graph, Some(&without_novice), None, None).expect_err("refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::SheetMissingPrerequisite {
                sheet: "rusher".into(),
                id: "blade.cuts.1".into(),
                prerequisite: "blade.novice".into()
            })
        );
        // A box held under its gate: Tovan's Cuts 2 needs 400 blade XP.
        let under_gate = HOUSEHOLD.replace("xp: {Blade: 400,", "xp: {Blade: 399,");
        assert_ne!(under_gate, HOUSEHOLD);
        let error = AuthoredSheets::parse_household(&graph, &under_gate)
            .expect_err("a sheet under a gate is refused");
        assert_eq!(
            error,
            ParseError::Invalid(ProgressionError::SheetXp {
                sheet: "Tovan Reyes".into(),
                id: "blade.cuts.2".into(),
                kind: XpKind::Blade,
                have: 399,
                need: 400
            })
        );
        let empty = AuthoredSheets::parse(&graph, None, None, None).expect("nothing is fine");
        assert_eq!(empty, AuthoredSheets::default());
        let broken_recruit = HOUSEHOLD.replace(
            "\"marksman.novice\", \"marksman.pistol.1\"",
            "\"marksman.pistol.1\"",
        );
        assert_ne!(
            broken_recruit, HOUSEHOLD,
            "the pistol recruit's chain is in the file"
        );
        let error = AuthoredSheets::parse(&graph, None, None, Some(&broken_recruit))
            .expect_err("a recruit without the novice is refused");
        assert!(matches!(
            error,
            ParseError::Invalid(ProgressionError::SheetMissingPrerequisite { .. })
        ));
    }

    #[test]
    fn a_grant_and_an_ability_effect_are_checked_for_their_shape() {
        let with = |effect: Effect| {
            let mut graph = SkillGraph::shipped();
            graph.trees[0].novice.effects.push(effect);
            graph.validate()
        };
        let ability = |id: &str, field: &str, op, value| Effect::Ability {
            id: id.to_owned(),
            field: field.to_owned(),
            op,
            value,
        };
        assert_eq!(
            with(Effect::Grant {
                ability: "heavy_cut".to_owned()
            }),
            Ok(())
        );
        assert!(
            matches!(
                with(Effect::Grant {
                    ability: "Heavy Cut".to_owned()
                }),
                Err(ProgressionError::UnknownName {
                    what: "ability",
                    ..
                })
            ),
            "a grant names a snake case id"
        );
        assert_eq!(with(ability("heavy_cut", "cooldown", Op::Mul, 0.5)), Ok(()));
        assert!(matches!(
            with(ability("heavy_cut", "luck", Op::Mul, 0.5)),
            Err(ProgressionError::UnknownName { what: "field", .. })
        ));
        assert!(matches!(
            with(ability("heavy_cut", "cooldown", Op::Mul, 0.)),
            Err(ProgressionError::BadValue { .. })
        ));
        assert!(matches!(
            with(ability("", "cooldown", Op::Mul, 1.)),
            Err(ProgressionError::UnknownName {
                what: "ability",
                ..
            })
        ));
    }

    #[test]
    fn a_guard_effect_names_a_field_of_the_guard_row() {
        let with = |field: &str, op, value| {
            let mut graph = SkillGraph::shipped();
            graph.trees[0].novice.effects.push(Effect::Guard {
                field: field.to_owned(),
                op,
                value,
            });
            graph.validate()
        };
        for field in GUARD_FIELDS {
            assert_eq!(with(field, Op::Mul, 1.1), Ok(()), "{field}");
        }
        assert_eq!(with("parry", Op::Add, 0.03), Ok(()));
        assert!(matches!(
            with("luck", Op::Mul, 1.1),
            Err(ProgressionError::UnknownName { what: "field", .. })
        ));
        assert!(matches!(
            with("share", Op::Mul, 0.),
            Err(ProgressionError::BadValue { .. })
        ));
    }

    #[test]
    fn the_shipped_graph_grants_the_heavy_cut_with_the_box_that_opens_it() {
        let graph = SkillGraph::shipped();
        let opener = graph
            .boxes()
            .find(|b| {
                b.effects
                    .iter()
                    .any(|e| matches!(e, Effect::Unlock { action } if action == "Heavy"))
            })
            .expect("a box opens the heavy cut");
        assert!(
            opener
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Grant { ability } if ability == "heavy_cut"))
        );
    }
}
