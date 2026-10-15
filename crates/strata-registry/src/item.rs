//! Items: what a character carries and the household stores (ticket 0007,
//! step 1), as `items/*.item.ron` spells them, one definition a file, with
//! the carrying rules (`items/carry.ron`) and the first day's stock
//! (`items/starting.ron`) beside them.
//!
//! One representation runs from the save to the screen: slotted's
//! `ItemStack`, an item id, a count and a patch. This module is the shape
//! of that patch and its checks. A [`Record`] is a stack by name, which is
//! what a file holds, since slotted's dense ids are only good for the
//! process that froze them; the sim turns a record into a stack and back
//! (`strata_sim::items`). The patch keys are interned into the frozen set
//! here ([`PatchKeys`]), so two stacks with equal records have equal
//! patches and merge, and two batches of one module never do.
//!
//! The catalogue also projects each definition to a slotted `ItemDef` and
//! freezes them ([`ItemCatalogue::frozen`]), because the client's slot
//! widgets resolve an item id against slotted's own frozen set and a second
//! list of items would drift from this one.
//!
//! Fitting (ticket 0007, step 2) is data too. A weapon declares its mounts
//! and the moddability at which each extra one opens ([`MountDef`]); a
//! module declares what it changes ([`EffectDef`]: the property, the
//! factor a bought one has and the quality line that moves it); and
//! `crafting/fitting.ron` ([`FittingRules`]) holds the band a line moves a
//! factor in and which way each property is better. What a factor does in
//! a fight is the sim's (`strata_sim::fitting`).
//!
//! A lot (ticket 0007, step 3) is an item too, but no file declares it:
//! the catalogue makes one a resource type from the resource registry
//! ([`ItemKind::Lot`]), with a quality line an attribute its class carries
//! and tags for its class and family. A lot's record names the deposit it
//! was drawn from ([`LotOrigin`]), which is part of its patch, so two lots
//! merge only when they are of one deposit.
//!
//! A component and a commodity (ticket 0007, step 4) are what a schematic
//! takes beside lots ([`ItemKind::Component`], [`ItemKind::Commodity`]): a
//! component has quality lines a schematic's slot reads as it reads a lot's
//! attributes, and a commodity is flat, the same every time. Either may
//! have a price ([`ItemDefinition::price`]), which is what a bench buys one
//! for; what a schematic is lives in [`crate::schematic`].
//!
//! A blueprint (ticket 0007, step 5) is an item as well
//! ([`ItemKind::Blueprint`]): a single with no lines, whose batch number is
//! the blueprint's own and whose maker is who recorded it. What it makes
//! and what it asks for is the household's record of it
//! (`strata_sim::blueprint`), so its patch has no key a made part lacks.
//!
//! None of it is world state: it joins no world version and no handshake.
//! A data set without the directory has no items, and that is legal.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use slotted_model::{ComponentId, ItemId, Namespaced};
use slotted_registry::defs::ItemDef;
use slotted_registry::icon::{IconColor, IconDef, ShapeIcon, ShapeKind};
use slotted_registry::registry::{FrozenRegistries, Registries};

use crate::progression::WEAPONS;
use crate::resource::{Attribute, ResourceSet, Span};
use crate::wear::{RESTORE, WearRules};

/// The top of a quality line and of a weapon's condition.
pub const QUALITY_MAX: u16 = 1000;

/// What every line of a bought item reads: the tick on a quality bar, and
/// the grade of an item with no lines at all.
pub const STORE_GRADE: u16 = 500;

/// The bag slots a pack has room for. Not a tunable: the sim's pack holds
/// this many bags' cells and the stores screen lays out this many bag
/// grids, so `carry.ron` may enable fewer and never more.
pub const BAG_SLOTS: usize = 2;

/// The weapons a character carries: two slots, any weapon in either, never
/// two of one family.
pub const HANDS: usize = 2;

/// The quality line a weapon's extra mounts open by. Named here because
/// the rule reads it off a record; a weapon with a threshold must carry it.
pub const MODDABILITY: &str = "moddability";

/// What a module can change of the weapon it is fitted to: the thirteen
/// properties of the workbench catalogue (`crates/veyra-weapons`), which
/// spells them as labels (`reload time`). Which of them the fight has a
/// number for is the sim's to say, not the data's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Property {
    /// Rounds in the magazine.
    Capacity,
    /// Seconds a reload takes.
    ReloadTime,
    /// Seconds between shots.
    RepeatInterval,
    /// Seconds to draw the weapon.
    EquipTime,
    /// Pace while aiming.
    AimMoveSpeed,
    /// Health damage a hit does.
    HitDamage,
    /// Posture damage a hit does.
    HitPosture,
    /// Stamina an attack costs.
    AttackStamina,
    /// How far shots scatter.
    AimSpread,
    /// How long the aim takes to settle after a shot.
    RecoilRecovery,
    /// How fast heat leaves.
    CoolingRate,
    /// Seconds to brace.
    BraceTime,
    /// Seconds an attack takes to recover from.
    RecoveryTime,
}

impl Property {
    /// Whether the fight has a number for the property. One that has none
    /// keeps its factor in the data and never reaches a row.
    #[must_use]
    pub const fn live(self) -> bool {
        matches!(
            self,
            Self::Capacity
                | Self::ReloadTime
                | Self::RepeatInterval
                | Self::EquipTime
                | Self::AimMoveSpeed
                | Self::HitDamage
                | Self::HitPosture
                | Self::AttackStamina
                | Self::AimSpread
        )
    }

    /// Every property, for the rules to be checked against.
    pub const ALL: [Self; 13] = [
        Self::Capacity,
        Self::ReloadTime,
        Self::RepeatInterval,
        Self::EquipTime,
        Self::AimMoveSpeed,
        Self::HitDamage,
        Self::HitPosture,
        Self::AttackStamina,
        Self::AimSpread,
        Self::RecoilRecovery,
        Self::CoolingRate,
        Self::BraceTime,
        Self::RecoveryTime,
    ];

    /// The property's stable key (`reload_time`): the catalogue's label
    /// with its space as an underscore, and what a string table keys the
    /// property's name by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::ReloadTime => "reload_time",
            Self::RepeatInterval => "repeat_interval",
            Self::EquipTime => "equip_time",
            Self::AimMoveSpeed => "aim_move_speed",
            Self::HitDamage => "hit_damage",
            Self::HitPosture => "hit_posture",
            Self::AttackStamina => "attack_stamina",
            Self::AimSpread => "aim_spread",
            Self::RecoilRecovery => "recoil_recovery",
            Self::CoolingRate => "cooling_rate",
            Self::BraceTime => "brace_time",
            Self::RecoveryTime => "recovery_time",
        }
    }
}

/// Which way a property is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Better {
    /// More of it is better: capacity, damage.
    Higher,
    /// Less of it is better: every time and cost.
    Lower,
}

/// How a count that a factor has moved becomes whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CountRule {
    /// To the nearest whole, as a progression box rounds a magazine.
    #[default]
    Nearest,
    /// Down: a round is there or it is not.
    Floor,
}

/// How far under a whole number a count may fall and still be that number
/// when it is floored: a count is a product of `f32` factors, and 50 by
/// 1.06 is 52.999996 there. Wider than the error of any magazine by any
/// factor, far narrower than a round.
const FLOOR_SLACK: f64 = 1e-4;

impl CountRule {
    /// `count` as whole units, never below nought. Floored, a count that
    /// is whole but for the error of its `f32` product keeps its last unit.
    #[must_use]
    pub fn whole(self, count: f32) -> u16 {
        let whole = match self {
            Self::Nearest => f64::from(count.round()),
            Self::Floor => (f64::from(count) + FLOOR_SLACK).floor(),
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the type's range first; a magazine is a few dozen rounds"
        )]
        {
            whole.clamp(0., f64::from(u16::MAX)) as u16
        }
    }
}

/// One thing a module changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDef {
    /// What it changes.
    pub property: Property,
    /// The factor a bought module multiplies the weapon's own number by:
    /// the workbench catalogue's, and what every line at store grade gives.
    pub factor: f32,
    /// The module's quality line that moves the factor inside its band.
    pub line: String,
}

/// A factor is a finite number in every definition the catalogue passed,
/// so equality is total there, and an item's kind stays comparable as it
/// was before a module carried numbers.
impl Eq for EffectDef {}

/// A weapon's own line that moves one number of its own row, whatever is
/// fitted: the chassis's craft, where a module's effects are the parts'.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// The weapon's own quality line that drives it.
    pub line: String,
    /// The number of the row it multiplies.
    pub property: Property,
    /// The factor at the line's floor, nought.
    pub worst: f32,
    /// The factor at the line's ceiling, a thousand.
    pub best: f32,
}

/// Both ends are checked finite and positive at load, so equality is
/// total, as for an effect.
impl Eq for Signature {}

impl Signature {
    /// The factor the line gives at `line`, running straight from `worst`
    /// at nought to `best` at a thousand. Worked in `f64` and narrowed.
    #[must_use]
    pub fn factor(&self, line: u16) -> f32 {
        let q = f64::from(line.min(QUALITY_MAX)) / f64::from(QUALITY_MAX);
        let (worst, best) = (f64::from(self.worst), f64::from(self.best));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a factor near one, narrowed to the precision the sim multiplies in"
        )]
        {
            (worst + (best - worst) * q) as f32
        }
    }
}

/// One mount of a weapon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountDef {
    /// The mount's stable key (`feed`), which a module's `slot` names.
    pub slot: String,
    /// The moddability at which the mount opens. `None` for a base mount,
    /// open on every example.
    #[serde(default)]
    pub opens_at: Option<u16>,
}

/// The line a quality moves a factor along: `at_zero + per_quality * q` of
/// the authored change, `q` from nought to one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// The share of the authored change the poorest module has.
    pub at_zero: f32,
    /// What a whole scale of quality adds to that share.
    pub per_quality: f32,
}

impl Band {
    /// The share of the authored change at quality `q`, nought to one.
    fn share(self, q: f64) -> f64 {
        f64::from(self.at_zero) + f64::from(self.per_quality) * q
    }
}

/// `crafting/fitting.ron`: how a module's quality becomes a factor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FittingRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The band of an effect that moves its property the better way.
    pub benefit: Band,
    /// The band of one that moves it the worse way.
    pub drawback: Band,
    /// How a magazine a factor has moved becomes whole rounds.
    pub count: CountRule,
    /// Which way each property is better.
    pub better: BTreeMap<Property, Better>,
}

impl Default for FittingRules {
    /// A data set without the file: quality moves nothing, so a factor is
    /// its authored value, and no property has a direction, which is why
    /// such a data set may hold no module with an effect.
    fn default() -> Self {
        let flat = Band {
            at_zero: 1.,
            per_quality: 0.,
        };
        Self {
            version: Self::VERSION,
            benefit: flat,
            drawback: flat,
            count: CountRule::default(),
            better: BTreeMap::new(),
        }
    }
}

impl FittingRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse and check the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, a band that does not land
    /// store grade on the authored factor, or a property with no direction.
    pub fn parse(text: &str) -> Result<Self, ItemError> {
        let rules: Self = ron::from_str(text).map_err(|e| ItemError::Ron(e.to_string()))?;
        if rules.version != Self::VERSION {
            return Err(ItemError::Version {
                found: rules.version,
                expected: Self::VERSION,
            });
        }
        // A bought module is the catalogue's baseline, so the band may
        // tilt about store grade and never move it.
        let store = f64::from(STORE_GRADE) / f64::from(QUALITY_MAX);
        for (name, band) in [("benefit", rules.benefit), ("drawback", rules.drawback)] {
            let share = band.share(store);
            if share.is_nan() || (share - 1.).abs() > 1e-6 {
                return Err(ItemError::Fitting(format!(
                    "the {name} band gives {share} of the authored change at store grade, not 1"
                )));
            }
        }
        if let Some(missing) = Property::ALL
            .iter()
            .find(|property| !rules.better.contains_key(property))
        {
            return Err(ItemError::Fitting(format!(
                "`better` does not say which way {missing:?} is better"
            )));
        }
        Ok(rules)
    }

    /// Whether `effect` moves its property the better way. A property the
    /// rules give no direction reads as better when higher; the catalogue
    /// refuses an effect on one, so no loaded module reaches that.
    #[must_use]
    pub fn is_benefit(&self, effect: &EffectDef) -> bool {
        let higher = self.better.get(&effect.property) != Some(&Better::Lower);
        (effect.factor > 1.) == higher
    }

    /// The factor `effect` has on a module whose driving line reads `line`:
    /// the authored factor with its change scaled by the band, a benefit's
    /// growing with quality and a drawback's shrinking. Worked in `f64` and
    /// then narrowed, so store grade is the authored `f32` to the bit.
    #[must_use]
    pub fn factor(&self, effect: &EffectDef, line: u16) -> f32 {
        let band = if self.is_benefit(effect) {
            self.benefit
        } else {
            self.drawback
        };
        let q = f64::from(line.min(QUALITY_MAX)) / f64::from(QUALITY_MAX);
        let change = f64::from(effect.factor) - 1.;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a factor near one, narrowed to the precision the sim multiplies in"
        )]
        {
            (1. + change * band.share(q)) as f32
        }
    }
}

/// One item, as an `.item.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    /// The namespaced id, `veyra:weapon.patrol_carbine`, and what a save
    /// names the item by.
    pub id: Namespaced,
    /// What the card calls it.
    pub name: String,
    /// Who makes the bought one: the card's "Store grade · Vigil".
    pub maker: String,
    /// What it is, with what only that kind carries.
    pub kind: ItemKind,
    /// How many share a slot. One for a weapon and a bag, whose stacks
    /// are singles because each has its own condition or its own cells.
    pub stack: u32,
    /// The quality lines a stack of it carries, in the card's order. A
    /// record holds exactly these.
    #[serde(default)]
    pub lines: Vec<LineDef>,
    /// What one costs bought at store grade, in credits. `None` for
    /// anything that is not for sale; only a component or a commodity may
    /// have one.
    #[serde(default)]
    pub price: Option<u32>,
    /// The article a sentence names one of it with (`an` extended cell),
    /// where the language's usual one is wrong. Beside the name, which is
    /// as much the language's, so no screen guesses it from a letter.
    #[serde(default)]
    pub article: Option<String>,
    /// How a slot and the card draw it.
    pub icon: IconSpec,
}

/// What an item is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    /// A weapon a character holds in the hand of its family.
    Weapon {
        /// The sim's weapon family, by the name the combat tables key
        /// their rows with (`Carbine`). The definition is which carbine;
        /// the family is which row it fights by.
        family: String,
        /// The chassis's key in the workbench catalogue and in the baked
        /// picture index (`patrol_carbine`). Empty for a weapon with no
        /// pictures.
        #[serde(default)]
        chassis: String,
        /// Its mounts, in the order a screen lists them.
        #[serde(default)]
        mounts: Vec<MountDef>,
        /// The weapon's own line that moves its own row, if it has one.
        #[serde(default)]
        signature: Option<Signature>,
    },
    /// A part fitted to a weapon's mount.
    Module {
        /// The mount it fills, by its stable key (`feed`).
        slot: String,
        /// The weapons whose mount takes it.
        fits: Vec<Namespaced>,
        /// The module's key in the workbench catalogue and in the baked
        /// picture index (`patrol_extended_feed`). Empty for one with no
        /// picture.
        #[serde(default)]
        part: String,
        /// What it changes on the weapon it is fitted to.
        #[serde(default)]
        effects: Vec<EffectDef>,
    },
    /// A bag: in a pack's bag slot it opens this many cells.
    Bag {
        /// The cells it opens.
        cells: u16,
    },
    /// A lot of a resource type, its count the units. Made by the
    /// catalogue from the resource registry, never by an item file.
    Lot {
        /// The resource type, `strata:evaporite_salt`.
        resource: Namespaced,
        /// Its class.
        class: Namespaced,
        /// Its family.
        family: Namespaced,
        /// The type's floor and cap for each attribute its class carries,
        /// in the class's order, which is the order of the item's lines.
        ranges: Vec<(Attribute, Span)>,
    },
    /// A part a schematic takes and reads: its quality lines are to a
    /// schematic's slot what a lot's attributes are (crafting.md, 5.2).
    Component,
    /// A part a schematic takes and does not read: bought in, with no
    /// quality of its own, every one the same.
    Commodity,
    /// A production blueprint: the recording of one finished prototype,
    /// which a fabricator copies (crafting.md, 5.4). Every blueprint is a
    /// stack of this one item; which it is, is its batch number.
    Blueprint,
    /// A repair kit: consumed to restore a weapon's condition, by the
    /// amount of its `restore` line (crafting.md, 4.6).
    Kit,
    /// A carried tool: it works from the pack or a bag, never held. Its one
    /// quality line scales what it does (ticket 0009, step 3).
    Tool {
        /// What it reads.
        reads: ToolReads,
        /// The factor its line gives at 0, per-mille.
        worst: u16,
        /// The factor its line gives at [`QUALITY_MAX`], per-mille; store
        /// grade gives 1000.
        best: u16,
    },
    /// A ride the household owns (ticket 0009, step 2): a single with a
    /// condition and the lines of [`VEHICLE_LINES`], whose dials are the
    /// vehicle file's named here.
    Vehicle {
        /// The vehicle file this is a bike of, `veyra:vehicle.courier_bike`.
        vehicle: crate::vehicle::VehicleId,
    },
    /// A field harvester (ticket 0009, step 6): planted on found ground, it
    /// fills over played time. Its lines scale the dials of
    /// `resources/harvest.ron` once, at the planting; each runs from `worst`
    /// at nought through one at store grade to `best` at the top, as a tool's.
    Harvester {
        /// The factor the `rate` line gives at 0 and at [`QUALITY_MAX`],
        /// per-mille.
        rate: (u16, u16),
        /// The same for the `hopper` line.
        hopper: (u16, u16),
    },
}

/// The line a harvester's fill rate is read from.
pub const HARVEST_RATE: &str = "rate";
/// The line a harvester's hopper is read from.
pub const HARVEST_HOPPER: &str = "hopper";

/// The factor a quality `line` gives between `worst` (per-mille at nought)
/// and `best` (per-mille at the top), one at store grade, straight between.
fn grade_factor(worst: u16, best: u16, line: u16) -> f64 {
    let (worst, best) = (f64::from(worst) / 1000.0, f64::from(best) / 1000.0);
    let line = f64::from(line.min(QUALITY_MAX));
    let store = f64::from(STORE_GRADE);
    if line <= store {
        worst + (1.0 - worst) * line / store
    } else {
        1.0 + (best - 1.0) * (line - store) / (f64::from(QUALITY_MAX) - store)
    }
}

/// The lines a vehicle item carries: the speed and the handling scale the
/// vehicle file's dials, the durability scales a crash's cost.
pub const VEHICLE_LINES: [&str; 3] = ["speed", "handling", "durability"];

/// What a tool reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolReads {
    /// The ground's deposits, by a ping.
    Survey,
}

/// The line a survey tool's reach is read from.
pub const RANGE: &str = "range";

impl ItemKind {
    /// The factor a tool's `line` gives: `worst` at nought, one at store
    /// grade and `best` at the top, straight between each. One for a
    /// kind that is no tool.
    #[must_use]
    pub fn tool_factor(&self, line: u16) -> f64 {
        let Self::Tool { worst, best, .. } = self else {
            return 1.0;
        };
        grade_factor(*worst, *best, line)
    }

    /// The factors a harvester's `rate` and `hopper` lines give, in that
    /// order. One and one for a kind that is no harvester.
    #[must_use]
    pub fn harvester_factors(&self, rate_line: u16, hopper_line: u16) -> (f64, f64) {
        let Self::Harvester { rate, hopper } = self else {
            return (1.0, 1.0);
        };
        (
            grade_factor(rate.0, rate.1, rate_line),
            grade_factor(hopper.0, hopper.1, hopper_line),
        )
    }

    /// The kind's word in a tag and in a filter: `weapon`, `module`, `bag`,
    /// `lot`, `component`, `commodity`, `blueprint`.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Weapon { .. } => "weapon",
            Self::Module { .. } => "module",
            Self::Bag { .. } => "bag",
            Self::Lot { .. } => "lot",
            Self::Component => "component",
            Self::Commodity => "commodity",
            Self::Blueprint => "blueprint",
            Self::Kit => "kit",
            Self::Tool { .. } => "tool",
            Self::Vehicle { .. } => "vehicle",
            Self::Harvester { .. } => "harvester",
        }
    }
}

/// One quality line of an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineDef {
    /// The line's key in a record (`capacity`).
    pub id: String,
    /// What the card calls it.
    pub name: String,
}

/// How an item is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconSpec {
    /// The shape a slot shows: slotted bakes it, so a cell has an icon
    /// with no file on disk.
    pub shape: IconShape,
    /// The shape's colour, `#rrggbb`.
    pub color: IconColor,
    /// The baked picture the card shows when the content step has run,
    /// relative to the directory of [`crate::pictures::PICTURES_PATH`]. A
    /// side-on weapon is far wider than a cell, so it is the card's and
    /// not the slot's.
    #[serde(default)]
    pub picture: Option<String>,
    /// The library symbol (`veyra:icon.crafting.component`) the item draws
    /// when no picture is baked: a tintable mark takes [`Self::color`], a
    /// painted square is drawn as it is. [`ItemCatalogue::check_symbols`]
    /// refuses an id the catalogue does not hold.
    #[serde(default)]
    pub symbol: Option<String>,
}

/// The shapes a slot icon can be: slotted's primitives, spelt as variants
/// here because an item file is never patched and so never passes through
/// a raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IconShape {
    /// A full block.
    Cube,
    /// A half-height block.
    Slab,
    /// A flat, tapered bar.
    Ingot,
    /// An octahedron.
    Gem,
    /// A thin shaft.
    Rod,
    /// A ball.
    Sphere,
}

impl From<IconShape> for ShapeKind {
    fn from(shape: IconShape) -> Self {
        match shape {
            IconShape::Cube => Self::Cube,
            IconShape::Slab => Self::Slab,
            IconShape::Ingot => Self::Ingot,
            IconShape::Gem => Self::Gem,
            IconShape::Rod => Self::Rod,
            IconShape::Sphere => Self::Sphere,
        }
    }
}

fn one() -> u32 {
    1
}

/// A stack by name: the saved form of a slot, and the typed view the rules
/// read. Everything but the item and the count is the stack's patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// The definition's id.
    pub item: Namespaced,
    /// How many.
    #[serde(default = "one")]
    pub count: u32,
    /// The batch it was made in; nought is store grade.
    #[serde(default)]
    pub batch: u32,
    /// Who made a crafted batch. None for store grade, whose maker is the
    /// definition's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Each of the definition's lines, `0..=`[`QUALITY_MAX`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, u16>,
    /// A weapon's condition, `0..=`[`QUALITY_MAX`]; a module may have one
    /// too, and none reads as full; anything else has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<u16>,
    /// What is fitted to a weapon, by mount.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fitted: BTreeMap<String, Fitted>,
    /// The deposit a lot was drawn from. None for anything that is no lot,
    /// and for commodity stock, which is from nowhere and reads
    /// [`STORE_GRADE`] on every line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<LotOrigin>,
}

/// Where a lot was drawn: the deposit by its id and by its names. Part of
/// the lot's patch, so two lots of one type from
/// two deposits are two kinds of stack and never mix silently.
///
/// Not `deny_unknown_fields`: a household saved while a lot named the
/// terms it was drawn on still carries them, and they are read past.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LotOrigin {
    /// The deposit's id: its cell of the deposits' lattice.
    pub deposit: [i32; 2],
    /// The deposit's name, `Calder`: a lot of it is "Calder salt".
    pub root: String,
    /// The place it lies, `Calder flats`.
    pub place: String,
}

/// The lot item of a resource type: `strata:lot.evaporite_salt` for
/// `strata:evaporite_salt`. `None` only for an id too long to prefix.
#[must_use]
pub fn lot_item(resource: &Namespaced) -> Option<Namespaced> {
    Namespaced::new(resource.namespace(), &format!("lot.{}", resource.path())).ok()
}

/// Who a lot's card names as its maker when nothing else is known.
const LOT_MAKER: &str = "the survey office";

/// A module on a weapon's mount: a record of one, without what only a
/// loose stack has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fitted {
    /// The module's id.
    pub item: Namespaced,
    /// See [`Record::batch`].
    #[serde(default)]
    pub batch: u32,
    /// See [`Record::by`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// See [`Record::lines`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, u16>,
    /// The module's own wear, kept when it is removed (crafting.md, 4.6).
    /// None reads as full, so a module never worn is written as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<u16>,
}

/// A grid's size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridDef {
    /// Cells across.
    pub cols: u16,
    /// Cells down.
    pub rows: u16,
}

impl GridDef {
    /// How many cells the grid has.
    #[must_use]
    pub const fn cells(self) -> u16 {
        self.cols.saturating_mul(self.rows)
    }
}

/// `items/carry.ron`: how much a character carries and the household
/// stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The pack's own grid.
    pub pack: GridDef,
    /// How many bag slots a pack has, at most [`BAG_SLOTS`].
    pub bag_slots: u16,
    /// The most cells one bag opens: the size of the grid kept under each
    /// bag slot, of which a bag enables its own count.
    pub bag_cells: u16,
    /// The household's store.
    pub store: GridDef,
}

impl Default for CarryRules {
    /// A data set without the file: nothing is carried and nothing stored.
    fn default() -> Self {
        Self {
            version: Self::VERSION,
            pack: GridDef::default(),
            bag_slots: 0,
            bag_cells: 0,
            store: GridDef::default(),
        }
    }
}

impl CarryRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse and check the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, more bag slots than a pack
    /// has room for, or more slots in all than a menu can number.
    pub fn parse(text: &str) -> Result<Self, ItemError> {
        let rules: Self = ron::from_str(text).map_err(|e| ItemError::Ron(e.to_string()))?;
        if rules.version != Self::VERSION {
            return Err(ItemError::Version {
                found: rules.version,
                expected: Self::VERSION,
            });
        }
        if usize::from(rules.bag_slots) > BAG_SLOTS {
            return Err(ItemError::Carry(format!(
                "{} bag slots, a pack has room for {BAG_SLOTS}",
                rules.bag_slots
            )));
        }
        // A menu numbers its slots in a `u16` and keeps the top value for
        // "outside every slot".
        let slots = [rules.pack, rules.store]
            .iter()
            .map(|grid| usize::from(grid.cols) * usize::from(grid.rows))
            .sum::<usize>()
            + usize::from(rules.bag_slots)
            + usize::from(rules.bag_cells) * BAG_SLOTS
            + HANDS;
        if slots >= usize::from(u16::MAX) {
            return Err(ItemError::Carry(format!(
                "{slots} slots in all, a menu numbers fewer than {}",
                u16::MAX
            )));
        }
        Ok(rules)
    }
}

/// What a pack holds, slot for slot, as a save and the first day's file
/// write it. A list shorter than its grid leaves the rest empty.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPack {
    /// The pack's own cells.
    #[serde(default)]
    pub cells: Vec<Option<Record>>,
    /// The bag slots.
    #[serde(default)]
    pub bag_slots: Vec<Option<Record>>,
    /// The cells under each bag slot. A bag's contents live here and not
    /// inside the bag, so a cell is one stack whatever holds it.
    #[serde(default)]
    pub bags: Vec<Vec<Option<Record>>>,
    /// A stack left on the cursor when the screen closed on a full pack.
    #[serde(default)]
    pub cursor: Option<Record>,
}

/// What a first lot is of: one type, or any type of a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LotOf {
    /// A resource type, `strata:evaporite_salt`.
    Type(Namespaced),
    /// Any type of a class, `strata:brine`.
    Class(Namespaced),
}

impl std::fmt::Display for LotOf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Type(id) => write!(f, "the type `{id}`"),
            Self::Class(id) => write!(f, "the class `{id}`"),
        }
    }
}

/// One lot of the first day's store: the game draws it from the nearest
/// deposit that is `of` this, with that deposit's own figures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartingLot {
    /// What it is of.
    pub of: LotOf,
    /// How many units.
    pub units: u32,
}

/// `items/starting.ron`: the first day's stock, and what an older save
/// that knew no items is given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartingStock {
    /// See [`CarryRules::version`].
    pub version: u32,
    /// The weapon in each hand, for everyone: any two of different
    /// families. A fixed pair, which RON writes in round brackets.
    pub hands: [Record; HANDS],
    /// The household's store, filled from its first cell.
    #[serde(default)]
    pub store: Vec<Record>,
    /// What a member carries on arrival, by their name in the household
    /// file. Anyone not named carries nothing.
    #[serde(default)]
    pub packs: BTreeMap<String, SavedPack>,
    /// The household's credits on the first day.
    #[serde(default)]
    pub purse: u32,
    /// The lots in the store on the first day, each from the nearest
    /// deposit of its kind to where the household starts.
    #[serde(default)]
    pub lots: Vec<StartingLot>,
    /// How far from the start a first lot's deposit is looked for, metres.
    #[serde(default)]
    pub lots_within_m: f64,
}

impl StartingStock {
    /// The file's current shape: 2 since the purse and the first lots.
    pub const VERSION: u32 = 2;
}

/// The patch keys of a stack, interned in the catalogue's frozen set.
///
/// | Key | Value | Present |
/// | --- | --- | --- |
/// | `strata:batch` | an integer, nought for store grade | always |
/// | `strata:by` | a string | crafted only |
/// | `strata:lines` | a map of line id to `0..=1000` | when the item has lines |
/// | `strata:condition` | `0..=1000` | weapons, always |
/// | `strata:fitted` | a map of mount to `{item, batch, by, lines}` | weapons, when anything is fitted |
/// | `strata:origin` | `{deposit: [x, z], root, place}` | lots drawn from a deposit |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchKeys {
    /// `strata:batch`.
    pub batch: ComponentId,
    /// `strata:by`.
    pub by: ComponentId,
    /// `strata:lines`.
    pub lines: ComponentId,
    /// `strata:condition`.
    pub condition: ComponentId,
    /// `strata:fitted`.
    pub fitted: ComponentId,
    /// `strata:origin`.
    pub origin: ComponentId,
}

/// Why an item file, a record or a stock was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ItemError {
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
    /// The carrying rules do not make a pack.
    #[error("{0}")]
    Carry(String),
    /// The fitting rules do not make a band.
    #[error("{0}")]
    Fitting(String),
    /// The wear rules do not hold together.
    #[error("{0}")]
    Wear(String),
    /// The rework rules do not hold together.
    #[error("{0}")]
    Rework(String),
    /// Two files define one item.
    #[error("`{0}` is defined twice")]
    Duplicate(Namespaced),
    /// A weapon names a family the sim has no row for.
    #[error("`{item}`: family `{family}` is not one the sim has")]
    UnknownFamily {
        /// The weapon.
        item: Namespaced,
        /// The family it names.
        family: String,
    },
    /// A definition breaks a rule of its kind.
    #[error("`{item}`: {reason}")]
    Definition {
        /// The item.
        item: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
    /// A record names no item.
    #[error("`{0}` is no item")]
    UnknownItem(Namespaced),
    /// A record's count is nought or over the item's stack.
    #[error("`{item}`: a stack of {count}, the item stacks to {stack}")]
    Count {
        /// The item.
        item: Namespaced,
        /// The record's count.
        count: u32,
        /// The definition's stack size.
        stack: u32,
    },
    /// A record's lines are not exactly its item's.
    #[error("`{item}`: lines {found:?}, the item has {expected:?}")]
    Lines {
        /// The item.
        item: Namespaced,
        /// The line ids the record carries.
        found: Vec<String>,
        /// The line ids the definition has.
        expected: Vec<String>,
    },
    /// A line or a condition is over [`QUALITY_MAX`].
    #[error("`{item}`: {what} is {value}, over {QUALITY_MAX}")]
    Quality {
        /// The item.
        item: Namespaced,
        /// The line's id, or `condition`.
        what: String,
        /// The value.
        value: u16,
    },
    /// A weapon without a condition, or anything else with one.
    #[error("`{item}`: {reason}")]
    Condition {
        /// The item.
        item: Namespaced,
        /// Which way round it is wrong.
        reason: &'static str,
    },
    /// A store grade record that names a maker, or a crafted one that
    /// names none.
    #[error("`{item}`: {reason}")]
    Provenance {
        /// The item.
        item: Namespaced,
        /// Which way round it is wrong.
        reason: &'static str,
    },
    /// A deposit named on something that is no lot.
    #[error("`{item}`: {reason}")]
    Origin {
        /// The item.
        item: Namespaced,
        /// What is wrong.
        reason: &'static str,
    },
    /// A lot's figure outside what its type or its origin allows.
    #[error("`{item}`: {line} is {value}, {allowed}")]
    Span {
        /// The lot's item.
        item: Namespaced,
        /// The line's id.
        line: String,
        /// The value.
        value: u16,
        /// What it may be.
        allowed: String,
    },
    /// Something fitted that the weapon's mount does not take.
    #[error("`{item}`: `{fitted}` on `{slot}`: {reason}")]
    Fitted {
        /// The weapon, or whatever claimed to have a mount.
        item: Namespaced,
        /// The mount.
        slot: String,
        /// What is on it.
        fitted: Namespaced,
        /// Why it does not fit.
        reason: &'static str,
    },
    /// A stock does not fit the carrying rules.
    #[error("{place}: {reason}")]
    Stock {
        /// Where in the stock: `store`, `pack`, `bag 1`.
        place: String,
        /// What is wrong there.
        reason: String,
    },
    /// The first day's stock names a pack for nobody in the household.
    #[error("a pack for `{0}`, who is not in the household file")]
    UnknownMember(String),
    /// The projected set would not freeze.
    #[error("{0}")]
    Freeze(String),
}

/// Every item, frozen, with the carrying rules and the first day's stock.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemCatalogue {
    /// By [`ItemId`], which is the definitions' order by id.
    defs: Vec<ItemDefinition>,
    /// The same items as slotted knows them.
    frozen: Arc<FrozenRegistries>,
    keys: PatchKeys,
    carry: CarryRules,
    fitting: FittingRules,
    wear: WearRules,
    starting: Option<StartingStock>,
}

impl Default for ItemCatalogue {
    /// No items: what a data set without the directory holds. Built by
    /// freezing an empty set, since slotted's frozen set has no default
    /// and the patch keys must be interned in it either way.
    fn default() -> Self {
        Self::build(
            Vec::new(),
            CarryRules::default(),
            FittingRules::default(),
            &ResourceSet::default(),
        )
        .map_err(|(_, error)| error)
        .expect("an empty item set freezes")
    }
}

fn tag(group: &str, name: &str) -> Option<Namespaced> {
    Namespaced::new("strata", &format!("{group}/{}", name.to_lowercase())).ok()
}

impl ItemCatalogue {
    /// The tag every bag carries, which is what a bag slot accepts.
    ///
    /// # Panics
    /// Never: the name is a valid id.
    #[must_use]
    pub fn bag_tag() -> Namespaced {
        tag("kind", "bag").expect("a valid tag")
    }

    /// Check the definitions against each other and the rules, add a lot
    /// item for every type of `resources`, project them all to slotted's
    /// and freeze. Each definition comes with the file it was read from,
    /// and an error names the file at fault.
    ///
    /// # Errors
    /// The file and what is wrong in it.
    ///
    /// # Panics
    /// Never: the patch keys are valid ids.
    pub fn build(
        mut defs: Vec<(String, ItemDefinition)>,
        carry: CarryRules,
        fitting: FittingRules,
        resources: &ResourceSet,
    ) -> Result<Self, (String, Box<ItemError>)> {
        // A lot is the resource registry's to make: one authored in an item
        // file would be a lot of nothing the ground holds.
        if let Some((path, def)) = defs
            .iter()
            .find(|(_, def)| matches!(def.kind, ItemKind::Lot { .. }))
        {
            let error = ItemError::Definition {
                item: def.id.clone(),
                reason: "a lot is made from a resource type, not declared in an item file"
                    .to_owned(),
            };
            return Err((path.clone(), Box::new(error)));
        }
        defs.extend(lot_definitions(resources));
        // Sorted so the same data set gives the same ids however the
        // directory lists.
        defs.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        let (frozen, keys) = project(
            defs.iter().map(|(path, def)| (path.as_str(), def)),
            &carry,
            &fitting,
            &BTreeMap::new(),
        )?;
        Ok(Self {
            defs: defs.into_iter().map(|(_, def)| def).collect(),
            frozen: Arc::new(frozen),
            keys,
            carry,
            fitting,
            wear: WearRules::default(),
            starting: None,
        })
    }

    /// The catalogue with its wear rules.
    #[must_use]
    pub fn with_wear(mut self, wear: WearRules) -> Self {
        self.wear = wear;
        self
    }

    /// How things wear and what a kit restores.
    #[must_use]
    pub fn wear(&self) -> &WearRules {
        &self.wear
    }

    /// The catalogue with the first day's stock, checked against it.
    ///
    /// # Errors
    /// Another version, a record the catalogue refuses, a hand of the
    /// wrong family, or a stock that does not fit the carrying rules.
    pub fn with_starting(mut self, starting: StartingStock) -> Result<Self, ItemError> {
        if starting.version != StartingStock::VERSION {
            return Err(ItemError::Version {
                found: starting.version,
                expected: StartingStock::VERSION,
            });
        }
        self.check_hands(&starting.hands.clone().map(Some))?;
        let store: Vec<Option<Record>> = starting.store.iter().cloned().map(Some).collect();
        self.check_store(&store)?;
        for pack in starting.packs.values() {
            self.check_pack(pack)?;
        }
        self.check_lots(&starting)?;
        self.starting = Some(starting);
        Ok(self)
    }

    /// Whether every vehicle item names a vehicle that exists. Run after
    /// both are loaded, since the vehicles do not depend on the items.
    ///
    /// # Errors
    /// The first vehicle item whose vehicle is not in `vehicles`.
    pub fn check_vehicles(&self, vehicles: &crate::vehicle::VehicleSet) -> Result<(), ItemError> {
        for def in &self.defs {
            if let ItemKind::Vehicle { vehicle } = &def.kind
                && vehicles.get(vehicle).is_none()
            {
                return Err(ItemError::Definition {
                    item: def.id.clone(),
                    reason: format!("is a bike of `{vehicle}`, which is no vehicle"),
                });
            }
        }
        Ok(())
    }

    /// Whether every symbol an item names is in the icon library. A data
    /// set without a library (an empty catalogue) is not checked.
    ///
    /// # Errors
    /// The first item whose symbol the catalogue does not hold.
    pub fn check_symbols(&self, icons: &crate::icons::IconCatalogue) -> Result<(), ItemError> {
        if icons.is_empty() {
            return Ok(());
        }
        for def in &self.defs {
            if let Some(symbol) = &def.icon.symbol
                && icons.get(symbol).is_none()
            {
                return Err(ItemError::Definition {
                    item: def.id.clone(),
                    reason: format!("names the icon `{symbol}`, which the library does not hold"),
                });
            }
        }
        Ok(())
    }

    /// A definition by name.
    #[must_use]
    pub fn get(&self, name: &Namespaced) -> Option<&ItemDefinition> {
        self.def_of(self.id_of(name)?)
    }

    /// A definition by a stack's id, or `None` for an id from another
    /// frozen set.
    #[must_use]
    pub fn def_of(&self, id: ItemId) -> Option<&ItemDefinition> {
        self.defs.get(id.0 as usize)
    }

    /// The id a stack of `name` carries.
    #[must_use]
    pub fn id_of(&self, name: &Namespaced) -> Option<ItemId> {
        self.frozen.item_id(name)
    }

    /// Every definition, in id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ItemDefinition> {
        self.defs.iter()
    }

    /// How many items there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Whether the data set has no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The items as slotted knows them: what the client hands its slot
    /// widgets, and what a stack's ids are valid against.
    #[must_use]
    pub fn frozen(&self) -> &Arc<FrozenRegistries> {
        &self.frozen
    }

    /// The same items with a picture for an icon where `icons` has one,
    /// each an asset path by the item's id, and the stand-in shape for the
    /// rest: what a client that found the baked pictures hands its slot
    /// widgets in place of [`Self::frozen`].
    ///
    /// The ids and the patch keys are the catalogue's own, since the items
    /// are projected again in the same order, so a stack means the same
    /// item against either set. Only what a cell draws differs, which is
    /// why the sim, the save and a server never see this one.
    #[must_use]
    pub fn frozen_with(&self, icons: &BTreeMap<Namespaced, String>) -> Arc<FrozenRegistries> {
        if icons.is_empty() {
            return self.frozen.clone();
        }
        project(
            self.defs.iter().map(|def| ("", def)),
            &self.carry,
            &self.fitting,
            icons,
        )
        // The same definitions froze once already; one that would not
        // freeze again keeps its shapes.
        .map_or_else(|_| self.frozen.clone(), |(frozen, _)| Arc::new(frozen))
    }

    /// The patch keys.
    #[must_use]
    pub fn keys(&self) -> &PatchKeys {
        &self.keys
    }

    /// The carrying rules.
    #[must_use]
    pub fn carry(&self) -> &CarryRules {
        &self.carry
    }

    /// How a module's quality becomes a factor.
    #[must_use]
    pub fn fitting(&self) -> &FittingRules {
        &self.fitting
    }

    /// Whether `weapon`'s mount `slot` takes a module: `Ok` when it is
    /// open, `Err` with the moddability it opens at when the record's line
    /// is short of it, and `None` when `weapon` is no weapon or has no such
    /// mount.
    #[must_use]
    pub fn mount_opens(&self, weapon: &Record, slot: &str) -> Option<Result<(), u16>> {
        let ItemKind::Weapon { mounts, .. } = &self.get(&weapon.item)?.kind else {
            return None;
        };
        let mount = mounts.iter().find(|mount| mount.slot == slot)?;
        let moddability = weapon.lines.get(MODDABILITY).copied().unwrap_or(0);
        Some(match mount.opens_at {
            Some(threshold) if moddability < threshold => Err(threshold),
            _ => Ok(()),
        })
    }

    /// The first day's stock; `None` for a data set without the file,
    /// whose characters start with empty hands.
    #[must_use]
    pub fn starting(&self) -> Option<&StartingStock> {
        self.starting.as_ref()
    }

    /// Whether `record` is one a stack of its item can hold: the count
    /// within the stack size, exactly the item's lines, a condition on a
    /// weapon, allowed on a module and refused on anything else, a maker on a crafted batch and on
    /// nothing else, only modules of its own open mounts fitted, a
    /// deposit named on a lot alone, whose figures are then inside its
    /// type's spans, and a batch number on every blueprint.
    ///
    /// # Errors
    /// The first rule broken.
    pub fn check_record(&self, record: &Record) -> Result<(), ItemError> {
        let item = &record.item;
        let def = self
            .get(item)
            .ok_or_else(|| ItemError::UnknownItem(item.clone()))?;
        if record.count == 0 || record.count > def.stack {
            return Err(ItemError::Count {
                item: item.clone(),
                count: record.count,
                stack: def.stack,
            });
        }
        check_made(def, record.batch, record.by.as_deref(), &record.lines)?;
        check_origin(def, record)?;
        // A blueprint's batch is the number its body is kept under: one at
        // store grade would be a blueprint of nothing.
        if matches!(def.kind, ItemKind::Blueprint) && record.batch == 0 {
            return Err(ItemError::Provenance {
                item: item.clone(),
                reason: "a blueprint is recorded, never bought",
            });
        }
        let weapon = matches!(def.kind, ItemKind::Weapon { .. });
        let module = matches!(def.kind, ItemKind::Module { .. });
        let vehicle = matches!(def.kind, ItemKind::Vehicle { .. });
        match record.condition {
            Some(_) if !weapon && !module && !vehicle => {
                return Err(ItemError::Condition {
                    item: item.clone(),
                    reason: "only a weapon, a module or a vehicle has a condition",
                });
            }
            None if weapon => {
                return Err(ItemError::Condition {
                    item: item.clone(),
                    reason: "a weapon has a condition",
                });
            }
            Some(value) if value > QUALITY_MAX => {
                return Err(ItemError::Quality {
                    item: item.clone(),
                    what: "condition".to_owned(),
                    value,
                });
            }
            _ => {}
        }
        for (slot, fitted) in &record.fitted {
            let refused = |reason| ItemError::Fitted {
                item: item.clone(),
                slot: slot.clone(),
                fitted: fitted.item.clone(),
                reason,
            };
            if !weapon {
                return Err(refused("only a weapon has mounts"));
            }
            let module = self
                .get(&fitted.item)
                .ok_or_else(|| ItemError::UnknownItem(fitted.item.clone()))?;
            let ItemKind::Module {
                slot: mount, fits, ..
            } = &module.kind
            else {
                return Err(refused("is not a module"));
            };
            if mount != slot {
                return Err(refused("is made for another mount"));
            }
            if !fits.contains(item) {
                return Err(refused("does not fit this weapon"));
            }
            match self.mount_opens(record, slot) {
                None => return Err(refused("the weapon has no such mount")),
                Some(Err(_)) => return Err(refused("the mount is shut")),
                Some(Ok(())) => {}
            }
            check_made(module, fitted.batch, fitted.by.as_deref(), &fitted.lines)?;
            if let Some(value) = fitted.condition.filter(|value| *value > QUALITY_MAX) {
                return Err(ItemError::Quality {
                    item: fitted.item.clone(),
                    what: "condition".to_owned(),
                    value,
                });
            }
        }
        Ok(())
    }

    /// The family of the weapon `record` is, if it is one.
    #[must_use]
    pub fn family_of(&self, record: &Record) -> Option<&str> {
        match &self.get(&record.item)?.kind {
            ItemKind::Weapon { family, .. } => Some(family),
            _ => None,
        }
    }

    /// Whether `hands` can be a character's: each a single weapon, or
    /// nothing, and no two of one family (the fitted rows and the combat
    /// tables are keyed by it).
    ///
    /// # Errors
    /// The first hand that is wrong.
    pub fn check_hands(&self, hands: &[Option<Record>; HANDS]) -> Result<(), ItemError> {
        let mut seen: Vec<&str> = Vec::new();
        for (index, record) in hands.iter().enumerate() {
            let Some(record) = record else {
                continue;
            };
            self.check_record(record)?;
            let Some(family) = self.family_of(record) else {
                return Err(ItemError::Stock {
                    place: format!("hand {index}"),
                    reason: format!("`{}` is no weapon", record.item),
                });
            };
            if seen.contains(&family) {
                return Err(ItemError::Stock {
                    place: format!("hand {index}"),
                    reason: format!("a second {family}"),
                });
            }
            seen.push(family);
        }
        Ok(())
    }

    /// Whether `store` fits the household's store.
    ///
    /// # Errors
    /// More cells than the store has, or a record the catalogue refuses.
    pub fn check_store(&self, store: &[Option<Record>]) -> Result<(), ItemError> {
        self.check_cells("store", store, self.carry.store.cells())
    }

    /// Whether `pack` fits the carrying rules: every list within its
    /// grid, a bag and nothing else in a bag slot, and nothing in a bag
    /// cell that no bag opens.
    ///
    /// # Errors
    /// The first place that is wrong.
    pub fn check_pack(&self, pack: &SavedPack) -> Result<(), ItemError> {
        self.check_cells("pack", &pack.cells, self.carry.pack.cells())?;
        self.check_cells("bag slots", &pack.bag_slots, self.carry.bag_slots)?;
        if pack.bags.len() > usize::from(self.carry.bag_slots) {
            return Err(ItemError::Stock {
                place: "bags".to_owned(),
                reason: format!(
                    "{} bags' cells, a pack has {} bag slots",
                    pack.bags.len(),
                    self.carry.bag_slots
                ),
            });
        }
        let mut open = Vec::new();
        for (index, slot) in pack.bag_slots.iter().enumerate() {
            let cells = match slot {
                None => 0,
                Some(record) => self.bag_cells(record).ok_or_else(|| ItemError::Stock {
                    place: format!("bag slot {index}"),
                    reason: format!("`{}` is no bag", record.item),
                })?,
            };
            open.push(cells);
        }
        for (index, cells) in pack.bags.iter().enumerate() {
            let place = format!("bag {index}");
            self.check_cells(&place, cells, self.carry.bag_cells)?;
            let open = open.get(index).copied().unwrap_or(0);
            if cells.iter().skip(usize::from(open)).any(Option::is_some) {
                return Err(ItemError::Stock {
                    place,
                    reason: format!("a stack past the {open} cells its bag opens"),
                });
            }
        }
        if let Some(cursor) = &pack.cursor {
            self.check_record(cursor)?;
        }
        Ok(())
    }

    /// The lot item of the resource type `resource`, if the data set has
    /// the type.
    #[must_use]
    pub fn lot_of(&self, resource: &Namespaced) -> Option<&ItemDefinition> {
        self.get(&lot_item(resource)?)
            .filter(|def| matches!(def.kind, ItemKind::Lot { .. }))
    }

    /// Whether the first day's lots can be drawn: each of a type or a class
    /// the data set has, in an amount a stack holds, with a distance to
    /// look within, and all of them with room in the store beside what is
    /// already there.
    fn check_lots(&self, starting: &StartingStock) -> Result<(), ItemError> {
        let refused = |reason: String| ItemError::Stock {
            place: "lots".to_owned(),
            reason,
        };
        if starting.lots.is_empty() {
            return Ok(());
        }
        if !(starting.lots_within_m.is_finite() && starting.lots_within_m > 0.0) {
            return Err(refused(format!(
                "lots_within_m is {}, a distance to look for their deposits within",
                starting.lots_within_m
            )));
        }
        let lots = || {
            self.defs.iter().filter_map(|def| match &def.kind {
                ItemKind::Lot {
                    resource, class, ..
                } => Some((resource, class, def.stack)),
                _ => None,
            })
        };
        for lot in &starting.lots {
            let stack = match &lot.of {
                LotOf::Type(wanted) => lots().find(|(resource, ..)| *resource == wanted),
                LotOf::Class(wanted) => lots().find(|(_, class, _)| *class == wanted),
            }
            .map(|(.., stack)| stack)
            .ok_or_else(|| refused(format!("{} is nothing the ground holds", lot.of)))?;
            if lot.units == 0 || lot.units > stack {
                return Err(refused(format!(
                    "{} units of {}, a lot stacks to {stack}",
                    lot.units, lot.of
                )));
            }
        }
        let room = usize::from(self.carry.store.cells());
        if starting.store.len() + starting.lots.len() > room {
            return Err(refused(format!(
                "{} lots after {} stacks, the store has {room} cells",
                starting.lots.len(),
                starting.store.len()
            )));
        }
        Ok(())
    }

    /// The cells `record` opens in a bag slot; `None` when it is no bag.
    #[must_use]
    pub fn bag_cells(&self, record: &Record) -> Option<u16> {
        match self.get(&record.item)?.kind {
            ItemKind::Bag { cells } => Some(cells),
            _ => None,
        }
    }

    fn check_cells(
        &self,
        place: &str,
        cells: &[Option<Record>],
        room: u16,
    ) -> Result<(), ItemError> {
        if cells.len() > usize::from(room) {
            return Err(ItemError::Stock {
                place: place.to_owned(),
                reason: format!("{} cells, there is room for {room}", cells.len()),
            });
        }
        cells
            .iter()
            .flatten()
            .try_for_each(|record| self.check_record(record))
    }
}

/// The lot item of every resource type, each labelled as the resource
/// registry's where a file's path would be: the type's name, a line an
/// attribute its class carries in the class's order, the family's shape in
/// the class's colour.
fn lot_definitions(resources: &ResourceSet) -> Vec<(String, ItemDefinition)> {
    resources
        .type_ids()
        .filter_map(|ty| {
            let def = resources.get(ty)?;
            let (class, family) = (resources.class_of(ty), resources.family_of(ty));
            let ranges: Vec<(Attribute, Span)> = class
                .carries
                .iter()
                .filter_map(|attribute| Some((*attribute, *def.ranges.get(attribute)?)))
                .collect();
            let item = ItemDefinition {
                id: lot_item(&def.id)?,
                name: def.name.clone(),
                maker: LOT_MAKER.to_owned(),
                stack: resources.rules().lots.stack,
                lines: ranges
                    .iter()
                    .map(|(attribute, _)| LineDef {
                        id: attribute.key().to_owned(),
                        name: attribute.name().to_owned(),
                    })
                    .collect(),
                kind: ItemKind::Lot {
                    resource: def.id.clone(),
                    class: class.id.clone(),
                    family: family.id.clone(),
                    ranges,
                },
                price: None,
                article: None,
                icon: IconSpec {
                    shape: family.shape,
                    color: class.color,
                    picture: None,
                    symbol: class.symbol.clone(),
                },
            };
            Some((format!("the resource `{}`", def.id), item))
        })
        .collect()
}

/// The definitions as slotted knows them, frozen, with the patch keys
/// interned after them: each checked against the others and the rules,
/// drawn by the picture `icons` names for it or else by its shape. In the
/// order given, which is the order of the ids.
fn project<'a>(
    defs: impl Iterator<Item = (&'a str, &'a ItemDefinition)> + Clone,
    carry: &CarryRules,
    fitting: &FittingRules,
    icons: &BTreeMap<Namespaced, String>,
) -> Result<(FrozenRegistries, PatchKeys), (String, Box<ItemError>)> {
    let weapons: BTreeMap<&Namespaced, &[MountDef]> = defs
        .clone()
        .filter_map(|(_, def)| match &def.kind {
            ItemKind::Weapon { mounts, .. } => Some((&def.id, mounts.as_slice())),
            _ => None,
        })
        .collect();
    let mut registries = Registries::new();
    for (path, def) in defs {
        let refused = |error| (path.to_owned(), Box::new(error));
        let tags = check_definition(def, &weapons, carry, fitting).map_err(refused)?;
        let mut projected = ItemDef::new(def.id.clone());
        projected.display_name = Some(def.name.clone());
        projected.max_stack_size = def.stack;
        projected.tags = tags;
        projected.icon = Some(icons.get(&def.id).map_or_else(
            || IconDef::Shape(ShapeIcon::new(def.icon.shape.into(), def.icon.color)),
            |path| IconDef::Image(path.clone()),
        ));
        registries
            .add_item(projected)
            .map_err(|_| refused(ItemError::Duplicate(def.id.clone())))?;
    }
    let (mut frozen, _) = registries.freeze().map_err(|error| {
        let error = ItemError::Freeze(error.to_string());
        (String::new(), Box::new(error))
    })?;
    // No definition declares these as components, since their values
    // are a stack's and never a default, so the freeze has not seen
    // them.
    let mut key = |name: &str| {
        let name = Namespaced::new("strata", name).expect("a valid patch key");
        frozen.components.intern(&name)
    };
    let keys = PatchKeys {
        batch: key("batch"),
        by: key("by"),
        lines: key("lines"),
        condition: key("condition"),
        fitted: key("fitted"),
        origin: key("origin"),
    };
    Ok((frozen, keys))
}

/// What is wrong with one definition, if anything; else the tags its
/// slotted projection carries: its kind, and its family or its mount.
#[allow(clippy::too_many_lines, reason = "one arm a kind of item")]
fn check_definition(
    def: &ItemDefinition,
    weapons: &BTreeMap<&Namespaced, &[MountDef]>,
    carry: &CarryRules,
    fitting: &FittingRules,
) -> Result<Vec<Namespaced>, ItemError> {
    let refused = |reason: String| ItemError::Definition {
        item: def.id.clone(),
        reason,
    };
    if def.stack == 0 {
        return Err(refused("a stack of nought".to_owned()));
    }
    let mut lines = BTreeSet::new();
    for line in &def.lines {
        if !lines.insert(&line.id) {
            return Err(refused(format!("line `{}` is declared twice", line.id)));
        }
    }
    let single = || {
        if def.stack == 1 {
            Ok(())
        } else {
            Err(refused(format!(
                "a {} stacks to 1, not {}",
                def.kind.key(),
                def.stack
            )))
        }
    };
    check_part(def).map_err(refused)?;
    let mut tags = vec![tag("kind", def.kind.key()).expect("a valid tag")];
    match &def.kind {
        ItemKind::Weapon {
            family,
            mounts,
            signature,
            ..
        } => {
            if !WEAPONS.contains(&family.as_str()) {
                return Err(ItemError::UnknownFamily {
                    item: def.id.clone(),
                    family: family.clone(),
                });
            }
            single()?;
            check_mounts(def, mounts).map_err(refused)?;
            if let Some(signature) = signature {
                check_signature(def, signature, fitting).map_err(refused)?;
            }
            tags.extend(tag("family", family));
        }
        ItemKind::Module {
            slot,
            fits,
            effects,
            ..
        } => {
            let mount = tag("slot", slot)
                .ok_or_else(|| refused(format!("`{slot}` cannot name a mount")))?;
            for weapon in fits {
                let Some(mounts) = weapons.get(weapon) else {
                    return Err(refused(format!("fits `{weapon}`, which is no weapon")));
                };
                if !mounts.iter().any(|mount| mount.slot == *slot) {
                    return Err(refused(format!(
                        "fits `{weapon}`, which has no `{slot}` mount"
                    )));
                }
            }
            check_effects(def, effects, fitting).map_err(refused)?;
            tags.push(mount);
        }
        ItemKind::Bag { cells } => {
            single()?;
            if *cells == 0 || *cells > carry.bag_cells {
                return Err(refused(format!(
                    "opens {cells} cells, a bag opens 1 to {}",
                    carry.bag_cells
                )));
            }
        }
        ItemKind::Lot {
            resource,
            class,
            family,
            ..
        } => {
            // The tree as tags, so a slot can ask for a type, a class or a
            // family: `strata:class/reactive`.
            for (group, id) in [("type", resource), ("class", class), ("resource", family)] {
                tags.push(
                    tag(group, id.path())
                        .ok_or_else(|| refused(format!("`{id}` cannot name a tag")))?,
                );
            }
        }
        // Held to their rules by `check_part`; they carry their kind's tag
        // and no other.
        ItemKind::Component | ItemKind::Commodity => {}
        ItemKind::Kit => {
            // The line is how much one restores; a kit without it would
            // restore nothing and no schematic could weigh it.
            if !def.lines.iter().any(|line| line.id == RESTORE) {
                return Err(refused(format!("a kit has a `{RESTORE}` line")));
            }
        }
        ItemKind::Tool { worst, best, .. } => {
            single()?;
            // The factor is read off the range line, one at store grade.
            if !def.lines.iter().any(|line| line.id == RANGE) {
                return Err(refused(format!("a tool has a `{RANGE}` line")));
            }
            if !(*worst > 0 && *worst <= 1000 && *best >= 1000) {
                return Err(refused(format!(
                    "a tool's factor runs from {worst} (1 to 1000) to {best} (1000 or more), per-mille"
                )));
            }
        }
        ItemKind::Vehicle { .. } => {
            single()?;
            for line in VEHICLE_LINES {
                if !def.lines.iter().any(|each| each.id == line) {
                    return Err(refused(format!("a vehicle has a `{line}` line")));
                }
            }
        }
        ItemKind::Harvester { rate, hopper } => {
            single()?;
            for line in [HARVEST_RATE, HARVEST_HOPPER] {
                if !def.lines.iter().any(|each| each.id == line) {
                    return Err(refused(format!("a harvester has a `{line}` line")));
                }
            }
            for (worst, best) in [rate, hopper] {
                if !(*worst > 0 && *worst <= 1000 && *best >= 1000) {
                    return Err(refused(format!(
                        "a harvester's factor runs from {worst} (1 to 1000) to {best} (1000 or more), per-mille"
                    )));
                }
            }
        }
        ItemKind::Blueprint => {
            single()?;
            // What a blueprint makes and how well is the household's record
            // of it; a line here would be a second answer to that.
            if !def.lines.is_empty() {
                return Err(refused(
                    "a blueprint has no quality lines: what it makes is its recording's".to_owned(),
                ));
            }
        }
    }
    Ok(tags)
}

/// What is wrong with a definition as a bought part, if anything: a price
/// on something a bench does not buy, a price of nought, or a commodity
/// that is not flat or has no price.
fn check_part(def: &ItemDefinition) -> Result<(), String> {
    // A price is what a bench buys a part for; on anything else it would
    // be a number nothing reads.
    match (def.price, &def.kind) {
        (Some(0), _) => return Err("a price of nought".to_owned()),
        (
            Some(_),
            ItemKind::Component
            | ItemKind::Commodity
            | ItemKind::Tool { .. }
            | ItemKind::Vehicle { .. }
            | ItemKind::Harvester { .. }
            | ItemKind::Weapon { .. },
        )
        | (None, _) => {}
        (Some(_), kind) => {
            return Err(format!(
                "a {} has no price: only a component, a commodity, a tool, a weapon, a vehicle or a harvester is bought",
                kind.key()
            ));
        }
    }
    if !matches!(def.kind, ItemKind::Commodity) {
        return Ok(());
    }
    // Flat is the whole of what a commodity is: with a line it would be a
    // component, and without a price nothing could ever put one in a store.
    if !def.lines.is_empty() {
        return Err("a commodity has no quality lines; one with lines is a component".to_owned());
    }
    if def.price.is_none() {
        return Err("a commodity has a price: it is bought, never made".to_owned());
    }
    Ok(())
}

/// What is wrong with a weapon's mounts, if anything: one declared twice,
/// or a threshold nothing could ever meet.
fn check_mounts(def: &ItemDefinition, mounts: &[MountDef]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for mount in mounts {
        let slot = &mount.slot;
        if !seen.insert(slot) {
            return Err(format!("mount `{slot}` is declared twice"));
        }
        let Some(threshold) = mount.opens_at else {
            continue;
        };
        // A threshold is read off the record's line, so a weapon without
        // the line could never open the mount.
        if !def.lines.iter().any(|line| line.id == MODDABILITY) {
            return Err(format!(
                "mount `{slot}` opens at {threshold}, and the weapon has no `{MODDABILITY}` line"
            ));
        }
        if threshold > QUALITY_MAX {
            return Err(format!(
                "mount `{slot}` opens at {threshold}, over {QUALITY_MAX}"
            ));
        }
    }
    Ok(())
}

/// What is wrong with a module's effects, if anything: a property changed
/// twice, a factor that is none, a line the module has not got, or a
/// property the fitting rules give no direction.
fn check_effects(
    def: &ItemDefinition,
    effects: &[EffectDef],
    fitting: &FittingRules,
) -> Result<(), String> {
    let mut changed = BTreeSet::new();
    for effect in effects {
        let property = effect.property;
        if !changed.insert(property) {
            return Err(format!("{property:?} is changed twice"));
        }
        if !(effect.factor.is_finite() && effect.factor > 0.) {
            return Err(format!(
                "{property:?} has the factor {}, which is no factor",
                effect.factor
            ));
        }
        // The line is the module's own quality: one it lacks would leave
        // the factor with nothing to move it.
        if !def.lines.iter().any(|line| line.id == effect.line) {
            return Err(format!(
                "{property:?} is driven by line `{}`, which the module lacks",
                effect.line
            ));
        }
        if !fitting.better.contains_key(&property) {
            return Err(format!(
                "{property:?} has no direction in the fitting rules"
            ));
        }
    }
    Ok(())
}

/// A signature's line is one of the weapon's own, both ends are factors,
/// and the better end is the one the fitting rules say is better.
fn check_signature(
    def: &ItemDefinition,
    signature: &Signature,
    fitting: &FittingRules,
) -> Result<(), String> {
    if !def.lines.iter().any(|line| line.id == signature.line) {
        return Err(format!(
            "its signature is driven by line `{}`, which the weapon lacks",
            signature.line
        ));
    }
    for end in [signature.worst, signature.best] {
        if !(end.is_finite() && end > 0.) {
            return Err(format!(
                "its signature has the factor {end}, which is no factor"
            ));
        }
    }
    let Some(better) = fitting.better.get(&signature.property) else {
        return Err(format!(
            "{:?} has no direction in the fitting rules",
            signature.property
        ));
    };
    let right_way = match better {
        Better::Lower => signature.worst > signature.best,
        Better::Higher => signature.worst < signature.best,
    };
    if right_way {
        Ok(())
    } else {
        Err(format!(
            "its signature runs the wrong way: {:?} is better {better:?}, and worst {} best {}",
            signature.property, signature.worst, signature.best
        ))
    }
}

/// The rules a loose stack and a fitted module share: who made it, and
/// that its lines are exactly its definition's and in range.
fn check_made(
    def: &ItemDefinition,
    batch: u32,
    by: Option<&str>,
    lines: &BTreeMap<String, u16>,
) -> Result<(), ItemError> {
    match (batch, by) {
        (0, Some(_)) => {
            return Err(ItemError::Provenance {
                item: def.id.clone(),
                reason: "store grade names no maker of its own",
            });
        }
        (1.., None) => {
            return Err(ItemError::Provenance {
                item: def.id.clone(),
                reason: "a crafted batch names who made it",
            });
        }
        _ => {}
    }
    let expected: BTreeSet<&String> = def.lines.iter().map(|line| &line.id).collect();
    if !lines.keys().eq(expected.iter().copied()) {
        return Err(ItemError::Lines {
            item: def.id.clone(),
            found: lines.keys().cloned().collect(),
            expected: expected.into_iter().cloned().collect(),
        });
    }
    if let Some((line, value)) = lines.iter().find(|(_, value)| **value > QUALITY_MAX) {
        return Err(ItemError::Quality {
            item: def.id.clone(),
            what: line.clone(),
            value: *value,
        });
    }
    Ok(())
}

/// The rules of a lot's record: a deposit is named on a lot alone, a lot is
/// drawn and never made, and its figures are inside its type's spans when
/// it has a deposit and flat at store grade when it has none.
fn check_origin(def: &ItemDefinition, record: &Record) -> Result<(), ItemError> {
    let ItemKind::Lot { ranges, .. } = &def.kind else {
        return match record.origin {
            Some(_) => Err(ItemError::Origin {
                item: def.id.clone(),
                reason: "only a lot is drawn from a deposit",
            }),
            None => Ok(()),
        };
    };
    if record.batch != 0 || record.by.is_some() {
        return Err(ItemError::Provenance {
            item: def.id.clone(),
            reason: "a lot is drawn, not made: it has no batch and no maker",
        });
    }
    for (attribute, span) in ranges {
        let line = attribute.key();
        // The lines were held to the definition's by `check_made`.
        let value = record.lines.get(line).copied().unwrap_or(0);
        let allowed = match record.origin {
            Some(_) if span.contains(value) => continue,
            Some(_) => format!("its type runs {} to {}", span.floor, span.cap),
            None if value == STORE_GRADE => continue,
            None => format!("commodity stock is {STORE_GRADE} on every line"),
        };
        return Err(ItemError::Span {
            item: def.id.clone(),
            line: line.to_owned(),
            value,
            allowed,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::load::{CARRY_FILE, FITTING_FILE, LoadError, STARTING_FILE, load_from_dir};
    use crate::progression::AuthoredSheets;
    use crate::{GameRegistries, InMemorySource};

    /// The item stage alone over `source`: a world needs a biome before it
    /// freezes, and nothing here is about the world.
    fn load(source: &InMemorySource) -> Result<ItemCatalogue, LoadError> {
        crate::load::items(source, &AuthoredSheets::default(), &ResourceSet::default())
    }

    fn shipped() -> GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).expect("assets/ loads")
    }

    fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).expect("a valid id")
    }

    const CARRY: &str = "(version: 1, pack: (cols: 2, rows: 1), bag_slots: 1, bag_cells: 4, store: (cols: 3, rows: 1))";

    fn weapon(family: &str) -> String {
        format!(
            r##"(id: "test:weapon.carbine", name: "Carbine", maker: "Nobody",
                kind: Weapon(family: "{family}"), stack: 1,
                lines: [(id: "interval", name: "Action")],
                icon: (shape: Rod, color: "#C6D0DD"))"##
        )
    }

    /// The step's items are all there, each weapon names a family the
    /// combat tables have a row for, each module fits a weapon that
    /// ships, and the first day's stock is the design's.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one walk over every shipped item kind"
    )]
    fn the_shipped_items_load_and_name_live_families() {
        let registries = shipped();
        let items = &registries.items;
        let mut families = Vec::new();
        for def in items.iter() {
            match &def.kind {
                ItemKind::Weapon { family, .. } => {
                    assert!(WEAPONS.contains(&family.as_str()), "{}: {family}", def.id);
                    families.push(family.as_str());
                }
                ItemKind::Module { fits, .. } => {
                    assert!(!fits.is_empty(), "{} fits something", def.id);
                    for weapon in fits {
                        let fitted = items.get(weapon).expect("a module fits a shipped item");
                        assert!(matches!(fitted.kind, ItemKind::Weapon { .. }), "{weapon}");
                    }
                }
                ItemKind::Bag { cells } => assert!(*cells <= items.carry().bag_cells),
                // A lot is the resource registry's, held to it by
                // `a_lot_item_is_made_for_every_type`; the parts are held
                // to their own rules by
                // `a_commodity_has_a_price_and_no_lines`.
                // A blueprint is held to its rules by
                // `a_blueprint_is_single_flat_and_never_bought`.
                ItemKind::Lot { .. }
                | ItemKind::Component
                | ItemKind::Commodity
                | ItemKind::Kit
                | ItemKind::Tool { .. }
                | ItemKind::Blueprint
                | ItemKind::Vehicle { .. }
                | ItemKind::Harvester { .. } => {}
            }
        }
        families.sort_unstable();
        let mut expected = WEAPONS.to_vec();
        expected.sort_unstable();
        assert_eq!(families, expected, "one weapon a family");
        for name in [
            "veyra:weapon.patrol_carbine",
            "veyra:weapon.service_pistol",
            "veyra:module.patrol_extended_feed",
            "veyra:module.pistol_extended_cell",
            "veyra:module.vigil_reflex",
            "veyra:module.vigil_angled_grip",
            "veyra:bag.field_satchel",
            "veyra:bag.haul_pack",
        ] {
            let item = items.id_of(&id(name)).expect("ships");
            let def = items.def_of(item).expect("by id as by name");
            assert_eq!(def.id, id(name));
            let projected = items.frozen().items.get(item).expect("projected");
            assert_eq!(projected.max_stack_size, def.stack, "{name}");
            let kind = tag("kind", def.kind.key()).expect("a valid tag");
            assert!(items.frozen().tag_index.has_tag(item, &kind), "{name}");
        }
        let carbine = items
            .id_of(&id("veyra:weapon.patrol_carbine"))
            .expect("ships");
        let frozen = items.frozen();
        assert!(
            frozen
                .tag_index
                .has_tag(carbine, &id("strata:family/carbine"))
        );
        let feed = items
            .id_of(&id("veyra:module.patrol_extended_feed"))
            .expect("ships");
        assert!(frozen.tag_index.has_tag(feed, &id("strata:slot/feed")));
        let satchel = items.id_of(&id("veyra:bag.field_satchel")).expect("ships");
        assert!(frozen.tag_index.has_tag(satchel, &ItemCatalogue::bag_tag()));
        assert!(!frozen.tag_index.has_tag(feed, &ItemCatalogue::bag_tag()));
        for key in ["batch", "by", "lines", "condition", "fitted", "origin"] {
            let name = Namespaced::new("strata", key).expect("valid");
            assert!(frozen.components.contains(&name), "{key} is interned");
        }

        let carry = items.carry();
        assert_eq!((carry.pack.cells(), carry.store.cells()), (18, 54));
        assert_eq!((carry.bag_slots, carry.bag_cells), (2, 12));

        let starting = items.starting().expect("the first day's stock ships");
        for (hand, family) in starting.hands.iter().zip(["Carbine", "Pistol"]) {
            let def = items.get(&hand.item).expect("ships");
            assert!(
                matches!(&def.kind, ItemKind::Weapon { family: own, .. } if own == family),
                "{}: {:?}",
                def.id,
                def.kind
            );
            assert_eq!((hand.batch, hand.condition), (0, Some(QUALITY_MAX)));
            assert!(hand.lines.values().all(|line| *line == STORE_GRADE));
        }
        // The two feed batches of the ticket's card, apart in the store.
        let feeds: Vec<&Record> = starting
            .store
            .iter()
            .filter(|record| record.item == id("veyra:module.patrol_extended_feed"))
            .collect();
        assert_eq!(feeds.len(), 2);
        assert_eq!((feeds[0].count, feeds[0].batch), (4, 0));
        assert_eq!((feeds[1].count, feeds[1].batch), (2, 412));
        assert_eq!(feeds[1].by.as_deref(), Some("Tovan Reyes"));
        assert_eq!(
            feeds[1].lines,
            BTreeMap::from([
                ("capacity".to_owned(), 610),
                ("reload_time".to_owned(), 433)
            ])
        );
        // The arrival's satchel is in a bag slot with a stack in its cells,
        // so the bag rule is live on the first day.
        let household = registries.sheets.household.as_ref().expect("ships");
        let pack = &starting.packs[&household.arrival.name];
        let bag = pack.bag_slots[0].as_ref().expect("a bag in the first slot");
        assert_eq!(items.bag_cells(bag), Some(6));
        assert!(pack.bags[0][0].is_some());
    }

    /// The field harvester is a single with a rate and a hopper line, each a
    /// factor of one at store grade running between the file's two ends.
    #[test]
    fn the_field_harvester_scales_its_two_lines_through_store_grade() {
        let registries = shipped();
        let def = registries
            .items
            .get(&id("veyra:station.field_harvester"))
            .expect("ships");
        assert_eq!((def.stack, def.price), (1, Some(300)));
        assert_eq!(def.kind.key(), "harvester");
        let lines: Vec<&str> = def.lines.iter().map(|line| line.id.as_str()).collect();
        assert_eq!(lines, [HARVEST_RATE, HARVEST_HOPPER]);
        let at = |rate, hopper| def.kind.harvester_factors(rate, hopper);
        assert_eq!(at(0, 0), (0.8, 0.8));
        assert_eq!(at(STORE_GRADE, STORE_GRADE), (1.0, 1.0));
        assert_eq!(at(QUALITY_MAX, QUALITY_MAX), (1.25, 1.5));
        assert_eq!(ItemKind::Kit.harvester_factors(0, 0), (1.0, 1.0));
    }

    /// The survey tool is a single with a range line, one at store grade,
    /// its two ends the file's.
    #[test]
    fn the_survey_tool_scales_its_factor_through_store_grade() {
        let registries = shipped();
        let def = registries
            .items
            .get(&id("veyra:tool.survey"))
            .expect("ships");
        assert_eq!(def.stack, 1);
        assert_eq!(def.lines.len(), 1);
        assert_eq!(def.lines[0].id, RANGE);
        assert_eq!(def.kind.key(), "tool");
        let factor = |line| def.kind.tool_factor(line);
        assert!((factor(0) - 0.75).abs() < 1e-9);
        assert!((factor(STORE_GRADE) - 1.0).abs() < 1e-9);
        assert!((factor(QUALITY_MAX) - 1.5).abs() < 1e-9);
        assert!((factor(250) - 0.875).abs() < 1e-9);
        assert!((factor(u16::MAX) - 1.5).abs() < 1e-9, "clamped");
        assert!((ItemKind::Kit.tool_factor(0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_family_outside_the_vocabulary_is_refused_by_file() {
        let source = InMemorySource::new()
            .with(CARRY_FILE, CARRY)
            .with("items/carbine.item.ron", weapon("Carbine"))
            .with(
                "items/lance.item.ron",
                weapon("Lance").replace("weapon.carbine", "weapon.lance"),
            );
        let error = load(&source).expect_err("the sim has no lance");
        let LoadError::Items { path, source } = &error else {
            panic!("{error}");
        };
        assert_eq!(path, "items/lance.item.ron");
        assert_eq!(
            **source,
            ItemError::UnknownFamily {
                item: id("test:weapon.lance"),
                family: "Lance".to_owned(),
            }
        );
        assert!(
            error.to_string().contains("items/lance.item.ron"),
            "{error}"
        );

        // The other files name themselves the same way.
        let source = InMemorySource::new()
            .with(CARRY_FILE, CARRY.replace("bag_slots: 1", "bag_slots: 3"))
            .with("items/carbine.item.ron", weapon("Carbine"));
        let error = load(&source).expect_err("a pack has two bag slots");
        assert!(
            matches!(&error, LoadError::Items { path, .. } if path == CARRY_FILE),
            "{error}"
        );
        let source = InMemorySource::new()
            .with(CARRY_FILE, CARRY)
            .with("items/carbine.item.ron", weapon("Carbine"))
            .with(
                STARTING_FILE,
                r#"(version: 2, hands: (
                    (item: "test:weapon.carbine", lines: {"interval": 500}, condition: Some(1000)),
                    (item: "test:weapon.carbine", lines: {"interval": 500}, condition: Some(1000)),
                ))"#,
            );
        let error = load(&source).expect_err("a carbine in the pistol's hand");
        assert!(
            matches!(&error, LoadError::Items { path, .. } if path == STARTING_FILE),
            "{error}"
        );
    }

    #[test]
    fn a_record_with_a_line_its_item_lacks_is_refused() {
        let registries = shipped();
        let items = &registries.items;
        let feed = Record {
            item: id("veyra:module.patrol_extended_feed"),
            count: 2,
            batch: 412,
            by: Some("Tovan Reyes".to_owned()),
            lines: BTreeMap::from([
                ("capacity".to_owned(), 610),
                ("reload_time".to_owned(), 433),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(items.check_record(&feed), Ok(()));

        let mut extra = feed.clone();
        extra.lines.insert("interval".to_owned(), 500);
        assert!(
            matches!(items.check_record(&extra), Err(ItemError::Lines { .. })),
            "a line the feed has not got"
        );
        let mut short = feed.clone();
        short.lines.remove("capacity");
        assert!(
            matches!(items.check_record(&short), Err(ItemError::Lines { .. })),
            "a line the feed has, missing"
        );
        let mut over = feed.clone();
        over.lines.insert("capacity".to_owned(), 1001);
        assert!(matches!(
            items.check_record(&over),
            Err(ItemError::Quality { .. })
        ));
        let mut many = feed.clone();
        many.count = 21;
        assert!(matches!(
            items.check_record(&many),
            Err(ItemError::Count { .. })
        ));
        let mut anonymous = feed.clone();
        anonymous.by = None;
        assert!(matches!(
            items.check_record(&anonymous),
            Err(ItemError::Provenance { .. })
        ));
        let mut worn = feed.clone();
        worn.condition = Some(900);
        assert_eq!(items.check_record(&worn), Ok(()), "a module may be worn");
        let mut seals = Record {
            item: id("veyra:commodity.seal_set"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: Some(900),
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert!(
            matches!(items.check_record(&seals), Err(ItemError::Condition { .. })),
            "a seal set has no condition"
        );
        seals.condition = None;
        assert_eq!(items.check_record(&seals), Ok(()));

        // A weapon takes the module on the mount it was made for, and on
        // no other; a pistol takes none of the carbine's.
        let starting = items.starting().expect("ships");
        let fitted = Fitted {
            item: feed.item.clone(),
            batch: feed.batch,
            by: feed.by.clone(),
            lines: feed.lines.clone(),
            condition: None,
        };
        let mut carbine = starting.hands[0].clone();
        carbine.fitted.insert("feed".to_owned(), fitted.clone());
        assert_eq!(items.check_record(&carbine), Ok(()));
        let mut wrong_mount = starting.hands[0].clone();
        wrong_mount
            .fitted
            .insert("sight".to_owned(), fitted.clone());
        assert!(matches!(
            items.check_record(&wrong_mount),
            Err(ItemError::Fitted { .. })
        ));
        let mut pistol = starting.hands[1].clone();
        pistol.fitted.insert("feed".to_owned(), fitted);
        assert!(matches!(
            items.check_record(&pistol),
            Err(ItemError::Fitted { .. })
        ));
        let mut bare = starting.hands[0].clone();
        bare.condition = None;
        assert!(matches!(
            items.check_record(&bare),
            Err(ItemError::Condition { .. })
        ));
    }

    const FITTING: &str = "(version: 1, benefit: (at_zero: 0.4, per_quality: 1.2),
        drawback: (at_zero: 1.5, per_quality: -1.0), count: Nearest,
        better: {Capacity: Higher, ReloadTime: Lower, RepeatInterval: Lower, EquipTime: Lower,
            AimMoveSpeed: Higher, HitDamage: Higher, HitPosture: Higher, AttackStamina: Lower,
            AimSpread: Lower, RecoilRecovery: Lower, CoolingRate: Higher, BraceTime: Lower,
            RecoveryTime: Lower})";

    /// A carbine with a base feed and a stock that opens at 400, and a
    /// feed for it whose effects are `effects`, under the shipped bands.
    fn fitted_set(effects: &str) -> InMemorySource {
        unruled_set(effects).with(FITTING_FILE, FITTING)
    }

    /// [`fitted_set`] without the fitting rules.
    fn unruled_set(effects: &str) -> InMemorySource {
        InMemorySource::new()
            .with(CARRY_FILE, CARRY)
            .with(
                "items/carbine.item.ron",
                r##"(id: "test:weapon.carbine", name: "Carbine", maker: "Nobody",
                    kind: Weapon(family: "Carbine", mounts: [
                        (slot: "feed"), (slot: "stock", opens_at: Some(400))]),
                    stack: 1, lines: [(id: "moddability", name: "Moddability")],
                    icon: (shape: Rod, color: "#C6D0DD"))"##,
            )
            .with(
                "items/feed.item.ron",
                format!(
                    r##"(id: "test:module.feed", name: "Feed", maker: "Nobody",
                        kind: Module(slot: "feed", fits: ["test:weapon.carbine"],
                            effects: {effects}),
                        stack: 20, lines: [(id: "capacity", name: "Capacity")],
                        icon: (shape: Slab, color: "#7FD1FF"))"##
                ),
            )
    }

    /// A weapon whose action line is the signature's `line`, `worst` to
    /// `best` on the repeat interval, under the shipped bands.
    fn signed_set(line: &str, worst: f32, best: f32) -> InMemorySource {
        unruled_set("[]").with(FITTING_FILE, FITTING).with(
            "items/carbine.item.ron",
            format!(
                r##"(id: "test:weapon.carbine", name: "Carbine", maker: "Nobody",
                        kind: Weapon(family: "Carbine", mounts: [(slot: "feed")],
                            signature: Some((line: "{line}", property: RepeatInterval,
                                worst: {worst}, best: {best}))),
                        stack: 1, lines: [(id: "interval", name: "Action")],
                        icon: (shape: Rod, color: "#C6D0DD"))"##
            ),
        )
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "the line's ends are the factors exactly")]
    fn a_signature_is_one_of_the_weapons_own_lines_and_runs_the_right_way() {
        assert!(load(&signed_set("interval", 1.3, 0.9)).is_ok());
        assert!(
            refusal(
                &signed_set("moddability", 1.3, 0.9),
                "items/carbine.item.ron"
            )
            .contains("lacks")
        );
        // Less interval is better, so the worst end is the larger.
        assert!(
            refusal(&signed_set("interval", 0.9, 1.3), "items/carbine.item.ron")
                .contains("wrong way")
        );
        assert!(
            refusal(&signed_set("interval", 1.3, 0.0), "items/carbine.item.ron")
                .contains("no factor")
        );
        let signature = Signature {
            line: "interval".to_owned(),
            property: Property::RepeatInterval,
            worst: 1.3,
            best: 0.9,
        };
        assert_eq!(signature.factor(0), 1.3);
        assert_eq!(signature.factor(1000), 0.9);
        assert!((signature.factor(500) - 1.1).abs() < 1e-6);
    }

    /// Why the item at `file` was refused, as its file says it.
    fn refusal(source: &InMemorySource, file: &str) -> String {
        let error = load(source).expect_err("the set is refused");
        let LoadError::Items { path, source } = &error else {
            panic!("{error}");
        };
        assert_eq!(path, file, "{error}");
        source.to_string()
    }

    /// A bought module, every line 500, has the factor its file gives to
    /// the bit; the best and the poorest are the design's 60 percent more
    /// and 60 percent less of a benefit, and a drawback sharpened by 50 or halved
    /// (crafting.md, 4.3, whose table is the compact optic's 0.90 and 1.06).
    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "store grade is the authored factor exactly, which is the rule under test"
    )]
    fn the_band_lands_store_grade_on_the_authored_factor() {
        let registries = shipped();
        let items = &registries.items;
        let rules = items.fitting();
        assert_eq!(rules.count, CountRule::Nearest);
        let mut effects = 0;
        for def in items.iter() {
            let ItemKind::Module { effects: own, .. } = &def.kind else {
                continue;
            };
            assert!(!own.is_empty(), "{} changes something", def.id);
            for effect in own {
                assert_eq!(
                    rules.factor(effect, STORE_GRADE),
                    effect.factor,
                    "{}: {:?}",
                    def.id,
                    effect.property
                );
                // Quality never turns a benefit into a drawback or back.
                for line in [0, QUALITY_MAX] {
                    let factor = rules.factor(effect, line);
                    assert_eq!(factor > 1., effect.factor > 1., "{}: {line}", def.id);
                }
                effects += 1;
            }
        }
        assert_eq!(effects, 32, "two effects a module, sixteen modules");

        let effect = |property, factor| EffectDef {
            property,
            factor,
            line: "line".to_owned(),
        };
        let near = |factor: f32, expected: f32| (factor - expected).abs() < 1e-6;
        let spread = effect(Property::AimSpread, 0.90);
        assert!(rules.is_benefit(&spread), "less spread is better");
        // Four tenths of its good at nought, 1.6 of it at a thousand.
        assert!(near(rules.factor(&spread, 0), 0.96));
        assert!(near(rules.factor(&spread, QUALITY_MAX), 0.84));
        assert_eq!(rules.factor(&spread, STORE_GRADE), 0.90, "exact to the bit");
        let equip = effect(Property::EquipTime, 1.06);
        assert!(!rules.is_benefit(&equip), "a slower draw is worse");
        // Half again its harm at nought, half of it at a thousand.
        assert!(near(rules.factor(&equip, 0), 1.09));
        assert!(near(rules.factor(&equip, QUALITY_MAX), 1.03));
        assert_eq!(rules.factor(&equip, STORE_GRADE), 1.06, "exact to the bit");
        // The same property is a benefit the other way round.
        assert!(rules.is_benefit(&effect(Property::EquipTime, 0.95)));
        assert!(!rules.is_benefit(&effect(Property::Capacity, 0.9)));
        // The patrol magazine: the 30th round needs capacity 622.
        let capacity = effect(Property::Capacity, 1.2);
        let rounds = |line| rules.count.whole(24. * rules.factor(&capacity, line));
        assert_eq!(
            [0, 500, 610, 621, 622, 1000].map(rounds),
            [26, 29, 29, 29, 30, 32]
        );
        assert_eq!(CountRule::Floor.whole(24. * 1.2), 28);
        // A product that is whole but for its `f32` error keeps its last
        // round when floored, and one that is not whole loses nothing more.
        let short = std::hint::black_box(50_f32) * 1.06;
        assert!(short < 53., "the product falls short in f32: {short}");
        assert_eq!(
            [
                (50., 1.06),
                (50., 1.18),
                (60., 1.05),
                (50., 1.07),
                (24., 1.25)
            ]
            .map(|(magazine, factor): (f32, f32)| CountRule::Floor.whole(magazine * factor)),
            [53, 59, 63, 53, 30]
        );
        assert_eq!(CountRule::Floor.whole(-1.), 0);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "without the rules quality moves nothing: the factor is the authored one exactly"
    )]
    fn a_band_that_misses_store_grade_is_refused() {
        let tilted = FITTING.replace("at_zero: 0.4", "at_zero: 0.5");
        let error = FittingRules::parse(&tilted).expect_err("0.5 + 0.6 is not 1");
        assert!(matches!(&error, ItemError::Fitting(why) if why.contains("benefit")));
        let source = fitted_set("[]").with(FITTING_FILE, tilted);
        assert!(refusal(&source, FITTING_FILE).contains("store grade"));

        let short = FITTING.replace("RecoveryTime: Lower", "");
        let error = FittingRules::parse(&short).expect_err("twelve of thirteen");
        assert!(
            matches!(&error, ItemError::Fitting(why) if why.contains("RecoveryTime")),
            "{error}"
        );
        assert!(matches!(
            FittingRules::parse(&FITTING.replace("version: 1", "version: 2")),
            Err(ItemError::Version { found: 2, .. })
        ));
        assert_eq!(
            FittingRules::parse(FITTING).as_ref(),
            Ok(shipped().items.fitting())
        );
        // Without the file quality moves nothing, and a module may change
        // nothing either, since no property has a direction.
        let flat = FittingRules::default();
        let effect = EffectDef {
            property: Property::Capacity,
            factor: 1.2,
            line: "capacity".to_owned(),
        };
        assert_eq!(
            [0, 500, 1000].map(|line| flat.factor(&effect, line)),
            [1.2; 3]
        );
        assert!(load(&unruled_set("[]")).is_ok(), "no effects need no rules");
        let effects = r#"[(property: Capacity, factor: 1.2, line: "capacity")]"#;
        assert!(load(&fitted_set(effects)).is_ok());
        let why = refusal(&unruled_set(effects), "items/feed.item.ron");
        assert!(why.contains("no direction"), "{why}");
    }

    #[test]
    fn an_effect_on_a_line_the_module_lacks_is_refused() {
        let good = r#"[(property: Capacity, factor: 1.2, line: "capacity")]"#;
        let items = load(&fitted_set(good)).expect("the line is the module's own");
        let feed = items.get(&id("test:module.feed")).expect("loaded");
        assert!(matches!(&feed.kind, ItemKind::Module { effects, .. } if effects.len() == 1));

        let file = "items/feed.item.ron";
        let stray = r#"[(property: Capacity, factor: 1.2, line: "reload_time")]"#;
        let why = refusal(&fitted_set(stray), file);
        assert!(why.contains("`reload_time`"), "{why}");
        let twice = r#"[(property: Capacity, factor: 1.2, line: "capacity"),
            (property: Capacity, factor: 1.1, line: "capacity")]"#;
        assert!(refusal(&fitted_set(twice), file).contains("twice"));
        let nought = r#"[(property: Capacity, factor: 0.0, line: "capacity")]"#;
        assert!(refusal(&fitted_set(nought), file).contains("no factor"));
        // A module made for a mount its weapon has not got.
        let source = fitted_set(good);
        let muzzle = r##"(id: "test:module.brake", name: "Brake", maker: "Nobody",
            kind: Module(slot: "muzzle", fits: ["test:weapon.carbine"]), stack: 20,
            icon: (shape: Rod, color: "#7FD1FF"))"##;
        let why = refusal(
            &source.with("items/brake.item.ron", muzzle),
            "items/brake.item.ron",
        );
        assert!(why.contains("no `muzzle` mount"), "{why}");
        // A threshold is read off the moddability line.
        let lineless = InMemorySource::new().with(CARRY_FILE, CARRY).with(
            "items/carbine.item.ron",
            r##"(id: "test:weapon.carbine", name: "Carbine", maker: "Nobody",
                kind: Weapon(family: "Carbine", mounts: [(slot: "stock", opens_at: Some(400))]),
                stack: 1, icon: (shape: Rod, color: "#C6D0DD"))"##,
        );
        let why = refusal(&lineless, "items/carbine.item.ron");
        assert!(why.contains("`moddability`"), "{why}");
    }

    /// The carbine of the first day with `moddability` on its line.
    fn carbine_at(items: &ItemCatalogue, moddability: u16) -> Record {
        let mut carbine = items.starting().expect("ships").hands[0].clone();
        carbine.lines.insert(MODDABILITY.to_owned(), moddability);
        carbine
    }

    #[test]
    fn a_module_on_a_shut_mount_is_refused() {
        let registries = shipped();
        let items = &registries.items;
        // No shipped module is made for a mount that opens, so the stock
        // of the test set stands in: a part for the stock, which opens at
        // 400.
        let source = fitted_set("[]").with(
            "items/stock.item.ron",
            r##"(id: "test:module.stock", name: "Stock", maker: "Nobody",
                kind: Module(slot: "stock", fits: ["test:weapon.carbine"]), stack: 20,
                icon: (shape: Rod, color: "#7FD1FF"))"##,
        );
        let set = load(&source).expect("the set loads");
        let weapon = |moddability: u16| Record {
            item: id("test:weapon.carbine"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::from([(MODDABILITY.to_owned(), moddability)]),
            condition: Some(QUALITY_MAX),
            fitted: BTreeMap::from([(
                "stock".to_owned(),
                Fitted {
                    item: id("test:module.stock"),
                    batch: 0,
                    by: None,
                    lines: BTreeMap::new(),
                    condition: None,
                },
            )]),
            origin: None,
        };
        assert_eq!(set.check_record(&weapon(400)), Ok(()));
        assert_eq!(
            set.check_record(&weapon(399)),
            Err(ItemError::Fitted {
                item: id("test:weapon.carbine"),
                slot: "stock".to_owned(),
                fitted: id("test:module.stock"),
                reason: "the mount is shut",
            })
        );
        assert_eq!(set.mount_opens(&weapon(399), "stock"), Some(Err(400)));
        assert_eq!(set.mount_opens(&weapon(399), "feed"), Some(Ok(())));
        assert_eq!(set.mount_opens(&weapon(399), "muzzle"), None);

        // The shipped carbine, rough: its base mounts take their modules
        // whatever the line reads.
        let mut rough = carbine_at(items, 0);
        rough.fitted.insert(
            "feed".to_owned(),
            Fitted {
                item: id("veyra:module.patrol_extended_feed"),
                batch: 0,
                by: None,
                lines: BTreeMap::from([
                    ("capacity".to_owned(), STORE_GRADE),
                    ("reload_time".to_owned(), STORE_GRADE),
                ]),
                condition: None,
            },
        );
        assert_eq!(items.check_record(&rough), Ok(()));
        rough.fitted.get_mut("feed").expect("fitted").condition = Some(QUALITY_MAX + 1);
        assert!(matches!(
            items.check_record(&rough),
            Err(ItemError::Quality { value, .. }) if value == QUALITY_MAX + 1
        ));
        let feed = Record {
            item: id("veyra:module.patrol_extended_feed"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(
            items.mount_opens(&feed, "feed"),
            None,
            "a module has no mounts"
        );
    }

    /// The three melee items stand on chassis with mounts (ticket 0010, step
    /// 3): a bought greatblade, at 500, has its head and grip and not its
    /// power mount, and a part for a mount a weapon lacks is refused.
    #[test]
    fn a_bought_greatblade_opens_two_mounts_of_three_and_a_part_needs_its_mount() {
        let registries = shipped();
        let items = &registries.items;
        let record = |item: &str, moddability: u16| Record {
            item: id(item),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::from([(MODDABILITY.to_owned(), moddability)]),
            condition: Some(QUALITY_MAX),
            fitted: BTreeMap::new(),
            origin: None,
        };
        let greatblade = record("veyra:weapon.yard_greatblade", 500);
        assert_eq!(items.mount_opens(&greatblade, "head"), Some(Ok(())));
        assert_eq!(items.mount_opens(&greatblade, "grip"), Some(Ok(())));
        assert_eq!(items.mount_opens(&greatblade, "power"), Some(Err(600)));
        let made = record("veyra:weapon.yard_greatblade", 600);
        assert_eq!(items.mount_opens(&made, "power"), Some(Ok(())));
        let blade = record("veyra:weapon.service_blade", 500);
        assert_eq!(items.mount_opens(&blade, "edge"), Some(Ok(())));
        assert_eq!(items.mount_opens(&blade, "grip"), Some(Ok(())));
        assert_eq!(
            items.mount_opens(&blade, "head"),
            None,
            "a blade has no head"
        );
        assert_eq!(
            items.mount_opens(&record("veyra:weapon.pier_glaive", 399), "grip"),
            Some(Err(400))
        );

        let source = fitted_set("[]").with(
            "items/head.item.ron",
            r##"(id: "test:module.head", name: "Head", maker: "Nobody",
                kind: Module(slot: "head", fits: ["test:weapon.carbine"]), stack: 20,
                icon: (shape: Rod, color: "#7FD1FF"))"##,
        );
        let error = load(&source).expect_err("the carbine has no head mount");
        assert!(error.to_string().contains("no `head` mount"), "{error}");
    }

    /// The mounts are the workbench catalogue's (`Weapon::slots`), and the
    /// thresholds crafting.md's table for the carbine.
    #[test]
    fn a_bought_carbine_opens_four_mounts_and_a_bought_pistol_three() {
        let registries = shipped();
        let items = &registries.items;
        let starting = items.starting().expect("ships");
        let open = |weapon: &Record| -> Vec<String> {
            let ItemKind::Weapon { mounts, .. } = &items.get(&weapon.item).expect("ships").kind
            else {
                panic!("{} is a weapon", weapon.item);
            };
            mounts
                .iter()
                .filter(|mount| items.mount_opens(weapon, &mount.slot) == Some(Ok(())))
                .map(|mount| mount.slot.clone())
                .collect()
        };
        assert_eq!(open(&starting.hands[0]), ["sight", "feed", "grip", "stock"]);
        assert_eq!(open(&starting.hands[1]), ["sight", "feed", "grip"]);

        let carbine = |moddability| open(&carbine_at(items, moddability)).len();
        assert_eq!(
            [0, 399, 400, 599, 600, 799, 800, 1000].map(carbine),
            [3, 3, 4, 4, 5, 5, 6, 6]
        );
        let bought = &starting.hands[0];
        assert_eq!(items.mount_opens(bought, "muzzle"), Some(Err(600)));
        assert_eq!(items.mount_opens(bought, "fore_end"), Some(Err(800)));
        assert_eq!(items.mount_opens(bought, "cooling"), None);
        let mut pistol = starting.hands[1].clone();
        pistol.lines.insert(MODDABILITY.to_owned(), 399);
        assert_eq!(open(&pistol), ["feed", "grip"]);
        assert_eq!(items.mount_opens(&pistol, "sight"), Some(Err(400)));

        for (weapon, chassis) in [(0, "patrol_carbine"), (1, "service_pistol")] {
            let def = items.get(&starting.hands[weapon].item).expect("ships");
            assert!(
                matches!(&def.kind, ItemKind::Weapon { chassis: own, .. } if own == chassis),
                "{:?}",
                def.kind
            );
        }
    }

    #[test]
    fn a_data_set_without_items_is_legal() {
        let items = &load(&InMemorySource::new()).expect("nothing loads");
        assert!(items.is_empty());
        assert_eq!(*items, ItemCatalogue::default());
        assert_eq!(GameRegistries::default().items, *items);
        assert_eq!(*items.carry(), CarryRules::default());
        assert!(items.starting().is_none());
        assert_eq!(items.check_pack(&SavedPack::default()), Ok(()));
        assert_eq!(items.check_hands(&[None, None]), Ok(()));
        assert!(matches!(
            items.check_record(&Record {
                item: id("veyra:weapon.patrol_carbine"),
                count: 1,
                batch: 0,
                by: None,
                lines: BTreeMap::new(),
                condition: Some(1000),
                fitted: BTreeMap::new(),
                origin: None,
            }),
            Err(ItemError::UnknownItem(_))
        ));
        // The keys are there all the same, so a client can be handed the
        // frozen set whatever the data set holds.
        assert_eq!(items.frozen().components.len(), 6);
    }

    /// A catalogue projected with pictures draws the items named by their
    /// picture and the rest by their shape, and is otherwise the catalogue
    /// it was: the same ids, so a stack made against one reads against the
    /// other.
    #[test]
    fn a_picture_changes_an_items_icon_and_no_id() {
        let registries = shipped();
        let items = &registries.items;
        let carbine = id("veyra:weapon.patrol_carbine");
        let icons = BTreeMap::from([(
            carbine.clone(),
            "generated://weapons/veyra/patrol_carbine/icon.whole.png".to_owned(),
        )]);
        let pictured = items.frozen_with(&icons);
        let plain = items.frozen();
        assert_eq!(pictured.items.len(), plain.items.len());
        for ((id, name, def), (plain_id, plain_name, plain_def)) in
            pictured.items.iter().zip(plain.items.iter())
        {
            assert_eq!((id, name), (plain_id, plain_name));
            if *name == carbine {
                assert_eq!(def.icon, Some(IconDef::Image(icons[&carbine].clone())));
            } else {
                assert_eq!(def.icon, plain_def.icon, "{name}");
                assert!(matches!(def.icon, Some(IconDef::Shape(_))), "{name}");
            }
            assert_eq!(def.tags, plain_def.tags, "{name}");
        }
        // With no pictures it is the catalogue's own set, not a copy.
        assert!(Arc::ptr_eq(&items.frozen_with(&BTreeMap::new()), plain));
    }

    const SALT: &str = "strata:lot.evaporite_salt";

    /// A lot of salt as a draw makes it: `figures` on its three lines and,
    /// with `origin`, the deposit it is from.
    fn salt_lot(figures: [u16; 3], origin: bool) -> Record {
        let [purity, potency, stability] = figures;
        Record {
            item: id(SALT),
            count: 80,
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("purity".to_owned(), purity),
                ("potency".to_owned(), potency),
                ("stability".to_owned(), stability),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: origin.then(|| LotOrigin {
                deposit: [3, -2],
                root: "Calder".to_owned(),
                place: "Calder flats".to_owned(),
            }),
        }
    }

    /// Every resource type has a lot item the catalogue made: its id under
    /// `lot.`, the type's name, a line an attribute its class carries in the
    /// class's order, the dials' stack, the family's shape in the class's
    /// colour, and the tree as tags.
    #[test]
    fn a_lot_item_is_made_for_every_type() {
        let registries = shipped();
        let (items, resources) = (&registries.items, &registries.resources);
        let lots: Vec<&ItemDefinition> = items
            .iter()
            .filter(|def| matches!(def.kind, ItemKind::Lot { .. }))
            .collect();
        assert_eq!(lots.len(), 7);
        assert_eq!(lots.len(), resources.types().len());
        for ty in resources.type_ids() {
            let resource = resources.get(ty).expect("its own handle");
            let (class, family) = (resources.class_of(ty), resources.family_of(ty));
            let def = items.lot_of(&resource.id).expect("a lot a type");
            assert_eq!(def.id, lot_item(&resource.id).expect("a valid id"));
            assert_eq!(def.id.path(), format!("lot.{}", resource.id.path()));
            assert_eq!(def.name, resource.name);
            assert_eq!(def.stack, 200);
            assert_eq!(def.kind.key(), "lot");
            let lines: Vec<(&str, &str)> = def
                .lines
                .iter()
                .map(|line| (line.id.as_str(), line.name.as_str()))
                .collect();
            let carried: Vec<(&str, &str)> =
                class.carries.iter().map(|a| (a.key(), a.name())).collect();
            assert_eq!(lines, carried, "{}", def.id);
            let ItemKind::Lot {
                resource: named,
                class: of,
                family: under,
                ranges,
            } = &def.kind
            else {
                unreachable!("filtered above");
            };
            assert_eq!((named, of, under), (&resource.id, &class.id, &family.id));
            for (attribute, span) in ranges {
                assert_eq!(resource.ranges[attribute], *span);
            }
            assert_eq!(ranges.len(), class.carries.len());
            assert_eq!(
                (def.icon.shape, def.icon.color, &def.icon.picture),
                (family.shape, class.color, &None)
            );
            let item = items.id_of(&def.id).expect("frozen");
            let projected = items.frozen().items.get(item).expect("projected");
            assert_eq!(projected.max_stack_size, 200);
            assert!(matches!(projected.icon, Some(IconDef::Shape(_))));
            for tag in [
                "strata:kind/lot".to_owned(),
                format!("strata:class/{}", class.id.path()),
                format!("strata:resource/{}", family.id.path()),
                format!("strata:type/{}", resource.id.path()),
            ] {
                assert!(
                    items.frozen().tag_index.has_tag(item, &id(&tag)),
                    "{} lacks {tag}",
                    def.id
                );
            }
        }
        let salt = items.get(&id(SALT)).expect("ships");
        assert_eq!(salt.name, "Evaporite salt");
        let frozen = items.frozen();
        let salt_id = items.id_of(&id(SALT)).expect("frozen");
        assert!(
            frozen
                .tag_index
                .has_tag(salt_id, &id("strata:class/reactive"))
        );
        assert!(
            frozen
                .tag_index
                .has_tag(salt_id, &id("strata:resource/mineral"))
        );
        assert!(!frozen.tag_index.has_tag(salt_id, &id("strata:class/brine")));
        assert!(items.lot_of(&id("strata:plasma")).is_none());
    }

    /// The first day's purse and lots are the design's, a lot of something
    /// the ground does not hold or over a stack is refused in the starting
    /// file, and an item file may not declare a lot.
    #[test]
    fn the_first_days_lots_are_held_to_the_resources() {
        let registries = shipped();
        let items = &registries.items;
        let starting = items.starting().expect("ships");
        assert_eq!(starting.version, 2);
        assert_eq!(starting.purse, 2500);
        assert_eq!(
            starting.lots,
            [
                StartingLot {
                    of: LotOf::Type(id("strata:evaporite_salt")),
                    units: 160
                },
                StartingLot {
                    of: LotOf::Class(id("strata:brine")),
                    units: 160
                },
                StartingLot {
                    of: LotOf::Type(id("strata:light_alloy_ore")),
                    units: 200
                },
            ]
        );
        assert!(starting.lots_within_m > 0.0);

        // An item file may not declare a lot, and a first lot of something
        // the ground does not hold is refused in the starting file.
        let authored = InMemorySource::new().with(CARRY_FILE, CARRY).with(
            "items/salt.item.ron",
            r##"(id: "test:lot.salt", name: "Salt", maker: "Nobody",
                kind: Lot(resource: "test:salt", class: "test:reactive", family: "test:mineral",
                    ranges: []),
                stack: 200, icon: (shape: Ingot, color: "#F4F6F8"))"##,
        );
        assert!(refusal(&authored, "items/salt.item.ron").contains("not declared in an item file"));
        let mut stock = starting.clone();
        stock.lots[0].of = LotOf::Type(id("strata:plasma"));
        assert!(matches!(
            items.clone().with_starting(stock),
            Err(ItemError::Stock { place, .. }) if place == "lots"
        ));
        let mut stock = starting.clone();
        stock.lots[2].units = 201;
        assert!(matches!(
            items.clone().with_starting(stock),
            Err(ItemError::Stock { place, .. }) if place == "lots"
        ));
        let mut stock = starting.clone();
        stock.lots_within_m = 0.0;
        assert!(items.clone().with_starting(stock).is_err());
        let mut stock = starting.clone();
        stock.version = 1;
        assert!(matches!(
            items.clone().with_starting(stock),
            Err(ItemError::Version { found: 1, .. })
        ));
    }

    /// A lot from a deposit holds figures inside its type's spans; commodity
    /// stock, from nowhere, is 500 on every line. A deposit is named on a
    /// lot alone, and a lot is drawn, never made.
    #[test]
    fn a_lots_figures_stay_inside_its_spans() {
        let registries = shipped();
        let items = &registries.items;
        // Purity 200 to 900, potency 300 to 950, stability 100 to 700.
        assert_eq!(items.check_record(&salt_lot([610, 820, 240], true)), Ok(()));
        assert_eq!(items.check_record(&salt_lot([200, 950, 700], true)), Ok(()));
        for (figures, line) in [
            ([199, 820, 240], "purity"),
            ([610, 951, 240], "potency"),
            ([610, 820, 701], "stability"),
            ([610, 820, 99], "stability"),
        ] {
            let error = items
                .check_record(&salt_lot(figures, true))
                .expect_err("outside its span");
            assert!(
                matches!(&error, ItemError::Span { line: at, .. } if at == line),
                "{error}"
            );
        }
        assert_eq!(items.check_record(&salt_lot([500; 3], false)), Ok(()));
        let error = items
            .check_record(&salt_lot([610, 820, 240], false))
            .expect_err("no deposit, no figures of its own");
        assert!(
            error.to_string().contains("commodity stock is 500"),
            "{error}"
        );
        // Sulphur's potency starts at 550: its commodity lot is 500 all the
        // same, since that number is the anchor and not a deposit's.
        let mut sulphur = salt_lot([500; 3], false);
        sulphur.item = id("strata:lot.sulphur");
        assert_eq!(items.check_record(&sulphur), Ok(()));

        let mut short = salt_lot([610, 820, 240], true);
        short.lines.remove("stability");
        assert!(matches!(
            items.check_record(&short),
            Err(ItemError::Lines { .. })
        ));
        let mut foreign = salt_lot([610, 820, 240], true);
        foreign.lines.insert("strength".to_owned(), 400);
        assert!(
            matches!(items.check_record(&foreign), Err(ItemError::Lines { .. })),
            "an attribute its class does not carry is absent, not nought"
        );
        let mut made = salt_lot([610, 820, 240], true);
        made.batch = 412;
        made.by = Some("Tovan Reyes".to_owned());
        assert!(matches!(
            items.check_record(&made),
            Err(ItemError::Provenance { .. })
        ));
        let mut heavy = salt_lot([610, 820, 240], true);
        heavy.count = 201;
        assert!(matches!(
            items.check_record(&heavy),
            Err(ItemError::Count { stack: 200, .. })
        ));
        // A module is from no deposit.
        let mut feed = Record {
            item: id("veyra:module.patrol_extended_feed"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("capacity".to_owned(), 500),
                ("reload_time".to_owned(), 500),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(items.check_record(&feed), Ok(()));
        feed.origin = salt_lot([500; 3], true).origin;
        assert!(matches!(
            items.check_record(&feed),
            Err(ItemError::Origin { .. })
        ));
        // A saved lot reads back with its deposit, and one without writes
        // no origin at all.
        let lot = salt_lot([610, 820, 240], true);
        let text = ron::to_string(&lot).expect("serialises");
        assert_eq!(ron::from_str::<Record>(&text).as_ref(), Ok(&lot));
        let plain = ron::to_string(&salt_lot([500; 3], false)).expect("serialises");
        assert!(!plain.contains("origin"), "{plain}");
    }

    /// A part of `kind` with `rest` after its stack, in a data set of its
    /// own.
    fn part(kind: &str, rest: &str) -> InMemorySource {
        InMemorySource::new().with(CARRY_FILE, CARRY).with(
            "items/part.item.ron",
            format!(
                r##"(id: "test:part", name: "Part", maker: "Nobody", kind: {kind}, stack: 20,
                    {rest} icon: (shape: Cube, color: "#8A97AD"))"##
            ),
        )
    }

    /// The parts of the first schematics ship: the cell core and the
    /// harness are components with lines, the seal set a commodity with a
    /// price and none, and the first day's store has the two that are
    /// bought. A commodity with a line or without a price is refused by
    /// its file, as a price on anything that is not a part is.
    #[test]
    fn the_compensator_is_a_muzzle_module_and_in_the_first_store() {
        let registries = shipped();
        let items = &registries.items;
        let def = items
            .get(&id("veyra:module.vigil_compensator"))
            .expect("ships");
        let ItemKind::Module { slot, fits, .. } = &def.kind else {
            panic!("a module");
        };
        assert_eq!(slot, "muzzle");
        assert_eq!(
            fits,
            &[
                id("veyra:weapon.patrol_carbine"),
                id("veyra:weapon.marksman_rifle")
            ]
        );
        let starting = items.starting().expect("ships");
        assert!(
            starting
                .store
                .iter()
                .any(|record| record.item == def.id && record.count == 1)
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the three parts, then each rule of their two kinds"
    )]
    fn a_commodity_has_a_price_and_no_lines() {
        let registries = shipped();
        let items = &registries.items;
        let core = items.get(&id("veyra:component.cell_core")).expect("ships");
        assert_eq!((&core.kind, core.price), (&ItemKind::Component, None));
        let lines: Vec<&str> = core.lines.iter().map(|line| line.id.as_str()).collect();
        assert_eq!(lines, ["charge_density", "stability"]);
        let harness = items.get(&id("veyra:component.harness")).expect("ships");
        assert_eq!(
            (&harness.kind, harness.price),
            (&ItemKind::Component, Some(80))
        );
        assert_eq!(harness.lines.len(), 1);
        let seals = items.get(&id("veyra:commodity.seal_set")).expect("ships");
        assert_eq!((&seals.kind, seals.price), (&ItemKind::Commodity, Some(20)));
        assert!(seals.lines.is_empty());
        for (def, tag) in [
            (core, "strata:kind/component"),
            (harness, "strata:kind/component"),
            (seals, "strata:kind/commodity"),
        ] {
            let item = items.id_of(&def.id).expect("frozen");
            assert!(
                items.frozen().tag_index.has_tag(item, &id(tag)),
                "{}",
                def.id
            );
            assert_eq!(def.kind.key(), tag.rsplit('/').next().expect("a tag"));
        }
        // Nothing else has a price but the mount blank (crafting.md, 4.2),
        // the survey tool (ticket 0009, step 3) and the courier bike (step 2).
        let blank = id("veyra:component.mount_blank");
        let tool = id("veyra:tool.survey");
        let bike = id("veyra:vehicle.courier_bike");
        // And the drive coil and the hull plate (step 5), bought at a bench
        // when a slot wants one and the stock has none.
        let coil = id("veyra:component.drive_coil");
        let plate = id("veyra:component.hull_plate");
        // And the field harvester (step 6), sold at a board.
        let harvester = id("veyra:station.field_harvester");
        // And the three melee weapons (ticket 0010, step 2), sold at a board.
        let blade = id("veyra:weapon.service_blade");
        let greatblade = id("veyra:weapon.yard_greatblade");
        let glaive = id("veyra:weapon.pier_glaive");
        // And the marksman rifle (ticket 0010, step 4).
        let rifle = id("veyra:weapon.marksman_rifle");
        for def in items.iter() {
            assert_eq!(
                def.price.is_some(),
                [
                    &harness.id,
                    &seals.id,
                    &blank,
                    &tool,
                    &bike,
                    &coil,
                    &plate,
                    &harvester,
                    &blade,
                    &greatblade,
                    &glaive,
                    &rifle
                ]
                .contains(&&def.id),
                "{}",
                def.id
            );
        }

        // The first day's store has three harnesses and six seal sets,
        // after the stock it had before them.
        let starting = items.starting().expect("ships");
        let bought: Vec<(String, u32, u32)> = starting
            .store
            .iter()
            .skip(6)
            .map(|record| (record.item.to_string(), record.count, record.batch))
            .collect();
        assert_eq!(
            bought,
            [
                ("veyra:component.harness".to_owned(), 3, 0),
                ("veyra:commodity.seal_set".to_owned(), 6, 0),
                ("veyra:kit.field_repair".to_owned(), 2, 0),
                ("veyra:module.vigil_compensator".to_owned(), 1, 0),
                ("veyra:weapon.service_blade".to_owned(), 1, 0),
                ("veyra:weapon.service_blade".to_owned(), 1, 0),
            ]
        );
        assert_eq!(
            starting.store[6].lines,
            BTreeMap::from([("conductance".to_owned(), STORE_GRADE)])
        );
        assert!(starting.store[7].lines.is_empty());

        // A made component names its batch and its maker as a module does,
        // and a commodity has nothing to say but its count.
        let mut made = Record {
            item: core.id.clone(),
            count: 1,
            batch: 1,
            by: Some("Frontier ranger".to_owned()),
            lines: BTreeMap::from([
                ("charge_density".to_owned(), 537),
                ("stability".to_owned(), 209),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(items.check_record(&made), Ok(()));
        made.lines.remove("stability");
        assert!(matches!(
            items.check_record(&made),
            Err(ItemError::Lines { .. })
        ));
        let mut flat = Record {
            item: seals.id.clone(),
            count: 50,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(items.check_record(&flat), Ok(()));
        flat.lines.insert("grade".to_owned(), 500);
        assert!(matches!(
            items.check_record(&flat),
            Err(ItemError::Lines { .. })
        ));

        // The article a sentence names one with is the file's; no shipped
        // item needs one.
        assert!(items.iter().all(|def| def.article.is_none()));
        let an = load(&part("Component", r#"article: Some("an"),"#)).expect("loads");
        assert_eq!(
            an.iter().next().and_then(|def| def.article.as_deref()),
            Some("an")
        );

        // The rules of the two kinds, by file.
        let stands = |source: &InMemorySource| load(source).map(|items| items.len()).ok();
        assert_eq!(stands(&part("Commodity", "price: Some(20),")), Some(1));
        assert_eq!(
            stands(&part("Component", "")),
            Some(1),
            "a made part has no price"
        );
        assert_eq!(
            stands(&part(
                "Component",
                r#"price: Some(80), lines: [(id: "conductance", name: "Conductance")],"#
            )),
            Some(1)
        );
        let said = refusal(
            &part(
                "Commodity",
                r#"price: Some(20), lines: [(id: "grade", name: "Grade")],"#,
            ),
            "items/part.item.ron",
        );
        assert!(said.contains("a commodity has no quality lines"), "{said}");
        let said = refusal(&part("Commodity", ""), "items/part.item.ron");
        assert!(said.contains("a commodity has a price"), "{said}");
        let said = refusal(&part("Component", "price: Some(0),"), "items/part.item.ron");
        assert!(said.contains("a price of nought"), "{said}");
        let said = refusal(
            &part("Bag(cells: 4)", "price: Some(5),").with(
                "items/part.item.ron",
                r##"(id: "test:part", name: "Part", maker: "Nobody", kind: Bag(cells: 4), stack: 1,
                    price: Some(5), icon: (shape: Cube, color: "#8A97AD"))"##,
            ),
            "items/part.item.ron",
        );
        assert!(said.contains("a bag has no price"), "{said}");
    }

    /// The blueprint item ships as a single with no lines and no price,
    /// tagged as its kind, and each of those is its file's to get right. A
    /// record of one names the number it was recorded under and who
    /// recorded it: one at store grade is a blueprint of nothing.
    #[test]
    fn a_blueprint_is_single_flat_and_never_bought() {
        let registries = shipped();
        let items = &registries.items;
        let def = items.get(&id("veyra:blueprint.production")).expect("ships");
        assert_eq!(
            (&def.kind, def.stack, def.price),
            (&ItemKind::Blueprint, 1, None)
        );
        assert!(def.lines.is_empty());
        assert_eq!(def.kind.key(), "blueprint");
        let item = items.id_of(&def.id).expect("frozen");
        assert!(
            items
                .frozen()
                .tag_index
                .has_tag(item, &id("strata:kind/blueprint"))
        );
        let blueprints = items
            .iter()
            .filter(|def| matches!(def.kind, ItemKind::Blueprint))
            .count();
        assert_eq!(blueprints, 1, "one item for every blueprint");

        let mut recorded = Record {
            item: def.id.clone(),
            count: 1,
            batch: 4,
            by: Some("Frontier ranger".to_owned()),
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(items.check_record(&recorded), Ok(()));
        recorded.count = 2;
        assert!(matches!(
            items.check_record(&recorded),
            Err(ItemError::Count { stack: 1, .. })
        ));
        recorded.count = 1;
        recorded.lines.insert("charge_density".to_owned(), 513);
        assert!(matches!(
            items.check_record(&recorded),
            Err(ItemError::Lines { .. })
        ));
        recorded.lines.clear();
        recorded.by = None;
        assert!(matches!(
            items.check_record(&recorded),
            Err(ItemError::Provenance { .. })
        ));
        recorded.batch = 0;
        assert_eq!(
            items.check_record(&recorded),
            Err(ItemError::Provenance {
                item: def.id.clone(),
                reason: "a blueprint is recorded, never bought",
            })
        );

        // The rules of the kind, by file.
        let stands = |source: &InMemorySource| load(source).map(|items| items.len()).ok();
        let blueprint = |stack: u32, rest: &str| {
            InMemorySource::new().with(CARRY_FILE, CARRY).with(
                "items/part.item.ron",
                format!(
                    r##"(id: "test:part", name: "Part", maker: "Nobody", kind: Blueprint,
                        stack: {stack}, {rest} icon: (shape: Slab, color: "#7FD1FF"))"##
                ),
            )
        };
        assert_eq!(stands(&blueprint(1, "")), Some(1));
        let said = refusal(&blueprint(20, ""), "items/part.item.ron");
        assert!(said.contains("a blueprint stacks to 1, not 20"), "{said}");
        let said = refusal(
            &blueprint(1, r#"lines: [(id: "grade", name: "Grade")],"#),
            "items/part.item.ron",
        );
        assert!(said.contains("a blueprint has no quality lines"), "{said}");
        let said = refusal(&blueprint(1, "price: Some(500),"), "items/part.item.ron");
        assert!(said.contains("a blueprint has no price"), "{said}");
    }

    /// The wear rules ship and are held together; a `worst` on a property
    /// the fight has no number for is refused, as is a kit without its
    /// restore line.
    #[test]
    fn wear_rules_load_and_a_kit_needs_its_restore_line() {
        let registries = shipped();
        let wear = registries.items.wear();
        assert_eq!(
            (wear.full, wear.slide_below, wear.show_below, wear.step),
            (QUALITY_MAX, 250, 500, 25)
        );
        assert_eq!(
            wear.worst.get(&Property::RepeatInterval).copied(),
            Some(1.25)
        );
        let kit = registries
            .items
            .get(&id("veyra:kit.field_repair"))
            .expect("ships");
        assert_eq!(kit.kind, ItemKind::Kit);
        assert_eq!(kit.lines[0].id, RESTORE);

        let shipped_text = include_str!("../../../assets/crafting/wear.ron");
        let tilted = shipped_text.replace("RepeatInterval: 1.25", "RecoilRecovery: 1.25");
        let error = WearRules::parse(&tilted).expect_err("recoil recovery has no number");
        assert!(matches!(&error, ItemError::Wear(why) if why.contains("RecoilRecovery")));

        let said = refusal(&part("Kit", ""), "items/part.item.ron");
        assert!(said.contains(RESTORE), "{said}");
        let line = r#"lines: [(id: "restore", name: "Restoration")],"#;
        assert!(load(&part("Kit", line)).is_ok());
    }

    /// The bike is an item of the vehicle of its name, a single with three
    /// lines and a price; a bike of no vehicle is refused once the vehicles
    /// are known, and a vehicle may carry a condition like a weapon.
    #[test]
    fn a_bike_is_an_item_of_a_vehicle_that_exists() {
        let registries = shipped();
        let bike = registries
            .items
            .get(&id("veyra:vehicle.courier_bike"))
            .expect("ships");
        assert!(matches!(
            &bike.kind,
            ItemKind::Vehicle { vehicle } if *vehicle == id("veyra:vehicle.courier_bike")
        ));
        assert_eq!((bike.stack, bike.price), (1, Some(450)));
        let lines: Vec<&str> = bike.lines.iter().map(|line| line.id.as_str()).collect();
        assert_eq!(lines, VEHICLE_LINES);
        registries
            .items
            .check_vehicles(&registries.vehicles)
            .expect("the shipped bike has its vehicle");
        let none = crate::vehicle::VehicleSet::default();
        let said = registries
            .items
            .check_vehicles(&none)
            .expect_err("no vehicle, no bike")
            .to_string();
        assert!(said.contains("is no vehicle"), "{said}");
        let record = Record {
            item: bike.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: VEHICLE_LINES
                .iter()
                .map(|line| ((*line).to_owned(), STORE_GRADE))
                .collect(),
            condition: Some(QUALITY_MAX),
            fitted: BTreeMap::new(),
            origin: None,
        };
        registries.items.check_record(&record).expect("whole bike");
        let worn = Record {
            condition: Some(300),
            ..record.clone()
        };
        registries.items.check_record(&worn).expect("a worn bike");
        let without = Record {
            lines: BTreeMap::new(),
            ..record
        };
        assert!(registries.items.check_record(&without).is_err());
    }

    /// A vehicle item needs its three lines and stacks to one.
    #[test]
    fn a_bike_without_its_lines_is_refused() {
        let kind = r#"Vehicle(vehicle: "veyra:vehicle.courier_bike")"#;
        let said = refusal(&part(kind, ""), "items/part.item.ron");
        assert!(said.contains("a vehicle stacks to 1, not 20"), "{said}");
        let single = |rest: &str| {
            InMemorySource::new().with(CARRY_FILE, CARRY).with(
                "items/part.item.ron",
                format!(
                    r##"(id: "test:part", name: "Part", maker: "Nobody", kind: {kind}, stack: 1,
                        {rest} icon: (shape: Cube, color: "#8A97AD"))"##
                ),
            )
        };
        let said = refusal(&single(""), "items/part.item.ron");
        assert!(said.contains("a vehicle has a `speed` line"), "{said}");
        let all = r#"lines: [(id: "speed", name: "S"), (id: "handling", name: "H"), (id: "durability", name: "D")],"#;
        assert!(load(&single(all)).is_ok());
    }

    /// A store grade example of the weapon `name`, as a bench or a board
    /// hands it over.
    fn example(items: &ItemCatalogue, name: &str) -> Record {
        let def = items.get(&id(name)).expect("ships");
        Record {
            item: def.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .map(|line| (line.id.clone(), STORE_GRADE))
                .collect(),
            condition: Some(QUALITY_MAX),
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    /// Either hand takes any weapon; two of one family are refused whichever
    /// hands they are in (ticket 0010, step 2).
    #[test]
    fn any_weapon_may_be_in_either_hand_and_never_two_of_a_family() {
        let registries = shipped();
        let items = &registries.items;
        let carbine = example(items, "veyra:weapon.patrol_carbine");
        let pistol = example(items, "veyra:weapon.service_pistol");
        let blade = example(items, "veyra:weapon.service_blade");
        let glaive = example(items, "veyra:weapon.pier_glaive");
        let greatblade = example(items, "veyra:weapon.yard_greatblade");
        let pair = |a: &Record, b: &Record| [Some(a.clone()), Some(b.clone())];
        for (a, b) in [
            (&carbine, &pistol),
            (&pistol, &carbine),
            (&blade, &carbine),
            (&carbine, &blade),
            (&glaive, &greatblade),
            (&blade, &pistol),
        ] {
            assert_eq!(items.check_hands(&pair(a, b)), Ok(()), "{a:?} {b:?}");
        }
        assert_eq!(items.check_hands(&[Some(glaive.clone()), None]), Ok(()));
        assert_eq!(items.check_hands(&[None, Some(blade.clone())]), Ok(()));
        for (a, b) in [(&carbine, &carbine), (&blade, &blade), (&glaive, &glaive)] {
            assert!(
                matches!(items.check_hands(&pair(a, b)), Err(ItemError::Stock { .. })),
                "{a:?}"
            );
        }
        // Another example of a family is the same family.
        let mut made = blade.clone();
        made.batch = 9;
        made.by = Some("Tovan Reyes".to_owned());
        assert!(items.check_hands(&pair(&blade, &made)).is_err());
        // A module is no weapon.
        let feed = Record {
            item: id("veyra:module.patrol_extended_feed"),
            lines: BTreeMap::from([
                ("capacity".to_owned(), STORE_GRADE),
                ("reload_time".to_owned(), STORE_GRADE),
            ]),
            condition: None,
            ..carbine
        };
        assert!(items.check_hands(&[Some(feed), None]).is_err());
    }
}
