//! Crafting: the session at a bench, from the stock picked for a
//! schematic's slots to the finished part (ticket 0007, step 4;
//! `docs/design/crafting.md`, 5.1 to 5.3 and 5.5).
//!
//! What a schematic is, and every dial of the session, is data
//! (`strata_registry::schematic`). This module is what the data means.
//! A line's **ceiling** is the weighted sum of the figures of what is in
//! the slots ([`ceiling`]): what the materials allow, which no skill or
//! luck exceeds. **Assembly** lands every line at a share of its ceiling
//! with no roll. **Experimentation** spends a budget of points, each on one
//! line with a hand, steady or bold, rolled against the table and lifting
//! the line by a share of its ceiling and never past it ([`roll`],
//! [`lift`]). All of it is whole per-mille:
//!
//! ```text
//! ceiling = (sum(weight_pm * v) + 500) / 1000     v: the figure, or 1000 less it for Light
//! figure  = (ceiling * share_pm + 500) / 1000     share_pm in 0..=1000; the floor is the assembly share
//! ```
//!
//! A roll is drawn from the world's seed and a counter kept in
//! [`CraftState`], which the game saves on every change: a result stands
//! once it is made, the same counter gives the same result, and two crafts
//! from the same lots differ because the counter moved. Nothing here shows
//! an outcome before the press; [`hand_odds`] is the odds.
//!
//! The session is state in the sim and in the save ([`Session`]): opened by
//! [`assemble`], which takes every input from the pack, its bags and the
//! store at once or takes nothing and names the slot that failed; advanced
//! by [`roll_line`]; closed by [`finish`], which makes the product and puts
//! it in the pack, then a bag, then the store, or refuses for want of room
//! and leaves the session as it was. The materials are committed at
//! assembly and never lost: a session left open is there when the bench is
//! next opened. What a finished thing was made from is kept by its batch
//! ([`Made`]), for its card.
//!
//! Everything a screen shows is worked out here and asked, never written
//! again: [`candidates`] for a slot, best first by what each would do for
//! the lines that weigh it; [`best_stock`] to fill every slot in a press;
//! [`preview`] for each line's floor and ceiling and what they are worth
//! in the game's units, before anything is spent; [`now`] for the same of a
//! session under way; [`can_roll`] for whether a hand would be refused and
//! [`lifts_odds`] for whether the stock moves the odds. A module's line is read through the fitting step's
//! own arithmetic on the weapon it fits (`crate::fitting`), so the rounds a
//! screen promises are the rounds the `Combatant` gets.
//!
//! Assemble, Roll, Finish, Buy and Record are requests consumed in the
//! tick (`CraftInbox`, `consume`), allowed with a workbench in reach
//! and answered with what happened or why not (`CraftAnswer`). Each is
//! one transaction: a refusal has changed nothing. Local-only, like the
//! stock: nothing here is replicated.
//!
//! Record (step 5) makes a production blueprint of a finished thing, which
//! is [`crate::blueprint`]'s: this module carries the request and keeps, in
//! the ledger, the schematic a thing was made by and whether it has been
//! recorded or was itself run off a blueprint.

use std::collections::BTreeMap;

use std::collections::btree_map::Entry;

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use slotted_model::{Inventory, ItemStack};

use strata_core::{Seed, mix64};

use strata_registry::item::{
    BAG_SLOTS, Fitted, HANDS, ItemCatalogue, ItemDefinition, ItemKind, MODDABILITY, Property,
    QUALITY_MAX, Record, STORE_GRADE,
};

use strata_registry::progression::{CraftRates, Effect, SkillGraph};

use strata_registry::schematic::{Hand, Outcome, Schematic, SessionRules, SlotDef, Stat, Takes};

use strata_registry::wear::DURABILITY;

use strata_registry::{Attribute, GameRegistries, Namespaced};

use crate::combat::{Combatant, FireMode, Weapon};

use crate::fitting::{self, Amount, Row};

use crate::household::Purse;

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

pub use crate::items::{kind_of, reachable};

use crate::progression::CharacterSheet;

use crate::wear;

/// A whole, in the per-mille every weight and share is worked in.
const PER_MILLE: u32 = QUALITY_MAX as u32;

/// The label the roll's stream is derived from the world's seed by.
const ROLL_STREAM: &str = "crafting.roll";

/// Who made a thing when the bench was worked by somebody with no name (a
/// harness actor, which has none): the empty name. A crafted batch names
/// its maker (`ItemCatalogue::check_record`), and what a card calls a maker
/// with no name is the card's to word.
pub const UNNAMED: &str = "";

/// What is picked for each slot of a schematic, by the slot's id: the kind
/// of stack, which is its record with a count of one. A pick names what a
/// thing is and not where it lies, so the stock may have moved under the
/// screen since it was listed.
pub type Picks = BTreeMap<String, Record>;

/// The crafting a household has done and has under way. The game saves it
/// whole on every change.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CraftState {
    /// How many rolls have been made: the counter the next is drawn from.
    #[serde(default)]
    pub rolls: u64,
    /// How many things have been finished: the last batch number given.
    #[serde(default)]
    pub batches: u32,
    /// The session on the bench, if one is open.
    #[serde(default)]
    pub session: Option<Session>,
    /// What each finished batch was made from, by its batch number.
    #[serde(default)]
    pub made: BTreeMap<u32, Made>,
    /// The best figure the household has made of each line of each
    /// schematic, keyed `<schematic id>#<line>`: what a later batch has to
    /// beat to earn the best-line XP.
    #[serde(default)]
    pub bests: BTreeMap<String, u16>,
}

/// A session under way: the materials are spent and the lines stand where
/// assembly and the rolls so far have put them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// The schematic being made.
    pub schematic: Namespaced,
    /// Who assembled it, and so whose name the product carries.
    pub by: String,
    /// What went in, by slot, each with the amount taken as its count.
    pub inputs: Vec<(String, Record)>,
    /// The product's lines, in its order.
    pub lines: Vec<SessionLine>,
    /// The experimentation points left.
    pub points: u8,
    /// The average Workability of the lots that carry it; none when none
    /// does. It moves the odds ([`SessionRules::row`]).
    pub workability: Option<u16>,
}

impl Session {
    /// Each line as it stands, by its id: what [`finish`] would write.
    #[must_use]
    pub fn figures(&self) -> BTreeMap<String, u16> {
        self.lines
            .iter()
            .map(|line| (line.line.clone(), line.figure()))
            .collect()
    }
}

/// One line of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLine {
    /// The product's line, by its id.
    pub line: String,
    /// What the materials allow.
    pub ceiling: u16,
    /// The share of the ceiling the line stands at, per-mille.
    pub share: u16,
    /// The last roll spent on it: what it came to, and what it moved the
    /// figure by.
    pub last: Option<(Outcome, i16)>,
}

impl SessionLine {
    /// The line's figure as it stands.
    #[must_use]
    pub fn figure(&self) -> u16 {
        figure(self.ceiling, self.share)
    }

    /// Whether a roll could still lift it.
    #[must_use]
    pub const fn at_ceiling(&self) -> bool {
        self.share >= QUALITY_MAX
    }
}

/// What a finished batch was made from, for its long card and for
/// recording it as a blueprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    /// The item made.
    pub item: Namespaced,
    /// Who made it.
    pub by: String,
    /// What went in, by slot, each with the amount taken as its count. For
    /// a batch run off a blueprint, a unit's worth of each kind that fed
    /// the slot.
    pub from: Vec<(String, Record)>,
    /// The schematic it was made by. None in an entry written before
    /// blueprints, which is read as the one schematic that makes the item
    /// from those slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic: Option<Namespaced>,
    /// The blueprint it was recorded as, once it has been: a prototype is
    /// recorded once, so a blueprint's runs stay finite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<u32>,
    /// The blueprint the batch was run off, for a fabricator's batch. Such
    /// a unit is a copy and no prototype.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint: Option<u32>,
}

/// `sum` of per-mille weights times figures, as a figure.
pub(crate) fn rounded(sum: u32) -> u16 {
    u16::try_from((sum + PER_MILLE / 2) / PER_MILLE).unwrap_or(QUALITY_MAX)
}

/// The figure a line stands at: `share_pm` of its `ceiling`, to the
/// nearest whole. The one place a share becomes a figure.
#[must_use]
pub fn figure(ceiling: u16, share_pm: u16) -> u16 {
    rounded(u32::from(ceiling) * u32::from(share_pm.min(QUALITY_MAX)))
}

/// Whether `record` is something `slot` takes: a lot of its family, class
/// or type, or the component or commodity it names.
#[must_use]
pub fn fits(items: &ItemCatalogue, slot: &SlotDef, record: &Record) -> bool {
    let Some(def) = items.get(&record.item) else {
        return false;
    };
    match (&slot.takes, &def.kind) {
        (Takes::Family(wanted), ItemKind::Lot { family, .. }) => family == wanted,
        (Takes::Class(wanted), ItemKind::Lot { class, .. }) => class == wanted,
        (Takes::Type(wanted), ItemKind::Lot { resource, .. }) => resource == wanted,
        (Takes::Component(wanted), ItemKind::Component)
        | (Takes::Commodity(wanted), ItemKind::Commodity) => record.item == *wanted,
        _ => false,
    }
}

/// The ceiling of the product's line at `line` with `picks` in the slots:
/// the weighted sum of the figures the line reads. `None` while a slot it
/// weighs is empty.
#[must_use]
pub fn ceiling(schematic: &Schematic, line: usize, picks: &Picks) -> Option<u16> {
    let mut sum = 0_u32;
    for (slot, stat, weight) in schematic.weights.get(line)? {
        let pick = picks.get(&schematic.def.slots[*slot].id)?;
        sum += u32::from(*weight) * u32::from(stat.read(pick)?);
    }
    Some(rounded(sum))
}

/// The Workability `record` carries, when it is a lot of a class that has
/// the attribute.
fn workability_of(record: &Record) -> Option<u16> {
    record.lines.get(Attribute::Workability.key()).copied()
}

/// The average Workability of what is picked, over the slots whose lot
/// carries it; `None` when none does. A slot counts once, whatever it
/// takes of its lot.
#[must_use]
pub fn workability(schematic: &Schematic, picks: &Picks) -> Option<u16> {
    let carried: Vec<u32> = schematic
        .def
        .slots
        .iter()
        .filter_map(|slot| workability_of(picks.get(&slot.id)?))
        .map(u32::from)
        .collect();
    let slots = u32::try_from(carried.len())
        .ok()
        .filter(|slots| *slots > 0)?;
    u16::try_from((carried.iter().sum::<u32>() + slots / 2) / slots).ok()
}

/// Whether stock that averages `workability` moves the odds a row upward:
/// what a screen says of the picks before they are spent ([`workability`])
/// and of a session while it is worked ([`Session::workability`]).
#[must_use]
pub fn lifts_odds(rules: &SessionRules, workability: Option<u16>) -> bool {
    rules.row(workability) > rules.base_row()
}

/// A kind of stack the stock holds for a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The kind, as a pick names it.
    pub kind: Record,
    /// How much of it the character carries, in the pack and its bags.
    pub carried: u32,
    /// How much the household's store holds.
    pub stored: u32,
    /// What it would do for the lines that weigh the slot: the sum of each
    /// weight in per-mille times the figure it reads. What the list is
    /// ordered by; nought for a slot nothing weighs.
    pub score: u32,
    /// The stats the slot is weighed by, each once, with this kind's
    /// figure as its card writes it: for [`Stat::Light`] the figure
    /// itself, of which less is better.
    pub stats: Vec<(Stat, u16)>,
    /// Its Workability, when its lot carries it. No line weighs it and the
    /// order of the list ignores it: it moves the odds of the whole
    /// session ([`workability`]).
    pub workability: Option<u16>,
    /// When there is too little of it for the slot: how much there is, and
    /// how much the slot takes.
    pub short: Option<(u32, u32)>,
}

impl Candidate {
    /// How much of the kind the stock holds in all.
    #[must_use]
    pub const fn held(&self) -> u32 {
        self.carried.saturating_add(self.stored)
    }
}

/// The kinds of stack that fit `slot` of `schematic` in the pack, its bags
/// and, when given, the household's store, each once with its counts, best
/// first: by what it would do for the slot's weighted lines, then store
/// grade before a batch and a batch before a later one, then by item and
/// by deposit. One too small for the slot is listed where its figures put
/// it, with how much there is and how much is wanted.
#[must_use]
pub fn candidates(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    schematic: &Schematic,
    slot: &str,
) -> Vec<Candidate> {
    let Some((index, def)) = schematic.slot(slot) else {
        return Vec::new();
    };
    let items = &registries.items;
    let mut found: Vec<Candidate> = Vec::new();
    for (record, in_store) in reachable(items, pack, store) {
        if !fits(items, def, &record) {
            continue;
        }
        let kind = kind_of(&record);
        let at = found
            .iter()
            .position(|candidate| candidate.kind == kind)
            .unwrap_or_else(|| {
                let weighed = schematic
                    .weights_on(index)
                    .map(|(stat, weight)| {
                        u32::from(weight) * u32::from(stat.read(&kind).unwrap_or(0))
                    })
                    .sum();
                let stats = schematic
                    .stats_on(index)
                    .into_iter()
                    .map(|stat| (stat.clone(), stat.figure(&kind).unwrap_or(0)))
                    .collect();
                found.push(Candidate {
                    workability: workability_of(&kind),
                    kind,
                    carried: 0,
                    stored: 0,
                    score: weighed,
                    stats,
                    short: None,
                });
                found.len() - 1
            });
        let count = if in_store {
            &mut found[at].stored
        } else {
            &mut found[at].carried
        };
        *count = count.saturating_add(record.count);
    }
    let needs = def.amount();
    for candidate in &mut found {
        let have = candidate.held();
        candidate.short = (have < needs).then_some((have, needs));
    }
    // Stable, so two that tie on everything keep the order they were met
    // in: the pack's before the store's.
    found.sort_by(|a, b| {
        let deposit = |candidate: &Candidate| candidate.kind.origin.as_ref().map(|o| o.deposit);
        b.score
            .cmp(&a.score)
            .then_with(|| a.kind.batch.cmp(&b.kind.batch))
            .then_with(|| a.kind.item.cmp(&b.kind.item))
            .then_with(|| deposit(a).cmp(&deposit(b)))
    });
    found
}

/// How much of each kind the slots so far have spoken for.
#[derive(Default)]
pub struct Spoken(Vec<(Record, u32)>);

impl Spoken {
    pub(crate) fn of(&self, kind: &Record) -> u32 {
        self.0
            .iter()
            .find(|(spoken, _)| spoken == kind)
            .map_or(0, |(_, amount)| *amount)
    }

    pub(crate) fn add(&mut self, kind: &Record, amount: u32) {
        match self.0.iter_mut().find(|(spoken, _)| spoken == kind) {
            Some((_, spoken)) => *spoken = spoken.saturating_add(amount),
            None => self.0.push((kind.clone(), amount)),
        }
    }
}

/// The best stock for every slot of `schematic`: slot by slot, the first
/// candidate there is enough of once what earlier slots took of its kind
/// is counted out, so one stack is never promised twice. A slot nothing is
/// enough for has no pick.
#[must_use]
pub fn best_stock(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    schematic: &Schematic,
) -> Picks {
    let mut spoken = Spoken::default();
    let mut picks = Picks::new();
    for slot in &schematic.def.slots {
        let needs = slot.amount();
        let best = candidates(registries, pack, store, schematic, &slot.id)
            .into_iter()
            .find(|candidate| candidate.held() >= spoken.of(&candidate.kind).saturating_add(needs));
        if let Some(best) = best {
            spoken.add(&best.kind, needs);
            picks.insert(slot.id.clone(), best.kind);
        }
    }
    picks
}

/// Whether `sheet` has the box that teaches `schematic`, if it names one.
/// Asked when a session opens and not after: a session already on the
/// bench is finished by whoever stands there.
///
/// # Errors
/// [`CraftRefusal::NotTaught`] with the box it needs.
pub fn taught(schematic: &Schematic, sheet: &CharacterSheet) -> Result<(), CraftRefusal> {
    match &schematic.def.profession {
        Some(needs) if !sheet.learned.contains(needs) => Err(CraftRefusal::NotTaught {
            needs: needs.clone(),
        }),
        _ => Ok(()),
    }
}

/// The points a session of `complexity` opens with for the holder of
/// `sheet`: the dials' for the complexity and one more for each point the
/// sheet's boxes add. The one place the number is worked out, so a screen
/// shows what the bench will give.
#[must_use]
pub fn points(
    rules: &SessionRules,
    graph: &SkillGraph,
    sheet: &CharacterSheet,
    complexity: u8,
) -> u8 {
    let added: u8 = crate::progression::learned_effects(graph, sheet)
        .map(|effect| match effect {
            Effect::CraftPoints { add } => *add,
            _ => 0,
        })
        .fold(0, u8::saturating_add);
    rules.points(complexity).saturating_add(added)
}

/// The assembly share, per-mille, a session opens with for the holder of
/// `sheet`: the dial and the sheet's `Craft` boxes on it. The one place the
/// number is worked out, so a screen shows what the bench will give; a sheet
/// with no such box reads the dial untouched.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a share in thousandths, rounded and held to the scale just below"
)]
pub fn assembly_pm(rules: &SessionRules, graph: &SkillGraph, sheet: &CharacterSheet) -> u16 {
    let dial = rules.assembly_pm();
    if !crate::progression::touches(graph, sheet, crate::progression::Touch::Craft, "assembly") {
        return dial;
    }
    let share = crate::progression::craft_field(graph, sheet, "assembly", f32::from(dial) / 1000.0);
    ((share * 1000.0).round().max(1.0) as u16).min(QUALITY_MAX)
}

/// The most points any sheet could open a session of `complexity` with:
/// the bound a saved session is held to, since the file does not say whose
/// sheet it was opened on.
fn most_points(registries: &GameRegistries, complexity: u8) -> u8 {
    let graph = &registries.progression;
    let every = CharacterSheet {
        learned: graph.boxes().map(|b| b.id.clone()).collect(),
        ..CharacterSheet::default()
    };
    points(registries.schematics.rules(), graph, &every, complexity)
}

/// Whether the stock has enough to fill every slot of `schematic` and
/// `sheet` is taught it: the mark a list puts on what can be made now.
#[must_use]
pub fn can_make(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    schematic: &Schematic,
    sheet: &CharacterSheet,
) -> bool {
    taught(schematic, sheet).is_ok()
        && best_stock(registries, pack, store, schematic).len() == schematic.def.slots.len()
}

/// Why one slot's pick will not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SlotRefusal {
    /// Nothing is picked for it.
    #[error("nothing is picked")]
    Empty,
    /// What is picked is not something it takes.
    #[error("it does not take that")]
    WrongKind,
    /// The stock has too little of what is picked.
    #[error("{have} in stock, it takes {needs}")]
    Short {
        /// How much the stock holds, less what earlier slots took of it.
        have: u32,
        /// How much the slot takes.
        needs: u32,
    },
}

/// Why the sim would not do what a crafting request asked. The words a
/// player reads are the screen's; this is what is the matter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CraftRefusal {
    /// There is no local player to work the bench.
    #[error("nobody is at the bench")]
    NoPlayer,
    /// No workbench is within reach.
    #[error("a thing is made at a workbench")]
    NoWorkbench,
    /// The data set has no such schematic.
    #[error("no such schematic")]
    UnknownSchematic,
    /// A session is on the bench already.
    #[error("finish what is on the bench first")]
    SessionOpen,
    /// A slot's pick will not do.
    #[error("slot `{slot}`: {why}")]
    Slot {
        /// The slot, by its id.
        slot: String,
        /// What is the matter with it.
        why: SlotRefusal,
    },
    /// Nothing is on the bench.
    #[error("nothing is on the bench")]
    NoSession,
    /// The product has no such line.
    #[error("the product has no such line")]
    UnknownLine,
    /// The crafter has not learned the box that teaches the schematic.
    #[error("needs the box `{needs}`")]
    NotTaught {
        /// The box, by its id.
        needs: String,
    },
    /// Every point is spent.
    #[error("no points are left")]
    NoPoints,
    /// The line is at what its materials allow.
    #[error("the line is at its ceiling")]
    AtCeiling,
    /// The pack, its bags and the store have no room for what was made or
    /// bought.
    #[error("there is no room")]
    NoRoom,
    /// The item is not one a bench buys.
    #[error("that is not for sale")]
    NotForSale,
    /// The purse does not cover the price.
    #[error("needs {needs} credits, the purse holds {have}")]
    Short {
        /// What it costs.
        needs: u32,
        /// What the purse holds.
        have: u32,
    },
    /// The data set has no factory dials, so nothing is recorded.
    #[error("nothing is recorded as a blueprint here")]
    NoBlueprints,
    /// Nothing of that batch is in the pack, its bags or the store.
    #[error("no such thing is in reach to record")]
    NoPrototype,
    /// The household's ledger no longer says what went into it, so there is
    /// nothing for a blueprint to ask for.
    #[error("what went into it is forgotten")]
    NoLedger,
    /// It was run off a blueprint itself: a copy is no prototype.
    #[error("it was run off blueprint {blueprint}")]
    RunOff {
        /// The blueprint it was run off.
        blueprint: u32,
    },
    /// It has been recorded already.
    #[error("it is recorded already, as blueprint {blueprint}")]
    Recorded {
        /// The blueprint it was recorded as.
        blueprint: u32,
    },
    /// The derating asked for is not one the dial stops at.
    #[error("a blueprint is not recorded at {asked} per-mille")]
    Derate {
        /// What was asked, per-mille of the prototype.
        asked: u16,
    },
}

/// Whether `picks` fill every slot of `schematic` from the stock, asked
/// slot by slot in the schematic's order: something picked, something the
/// slot takes, and enough of it once what earlier slots took of the same
/// kind is counted out.
///
/// # Errors
/// The schematic's box not learned, or the first slot that fails, and why.
pub fn check_picks(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    schematic: &Schematic,
    picks: &Picks,
    sheet: &CharacterSheet,
) -> Result<(), CraftRefusal> {
    taught(schematic, sheet)?;
    let items = &registries.items;
    let stock = reachable(items, pack, store);
    let mut spoken = Spoken::default();
    for slot in &schematic.def.slots {
        let refused = |why| CraftRefusal::Slot {
            slot: slot.id.clone(),
            why,
        };
        let chosen = picks
            .get(&slot.id)
            .ok_or_else(|| refused(SlotRefusal::Empty))?;
        if !fits(items, slot, chosen) {
            return Err(refused(SlotRefusal::WrongKind));
        }
        let kind = kind_of(chosen);
        let held: u32 = stock
            .iter()
            .filter(|(record, _)| kind_of(record) == kind)
            .map(|(record, _)| record.count)
            .sum();
        let have = held.saturating_sub(spoken.of(&kind));
        let needs = slot.amount();
        if have < needs {
            return Err(refused(SlotRefusal::Short { have, needs }));
        }
        spoken.add(&kind, needs);
    }
    Ok(())
}

/// Who is at the bench, as a figure in the game's units needs them: the
/// weapon in each hand with its holder's own row for it, and which is
/// drawn. A module's line is read on the weapon it fits, in the hand it is
/// in.
#[derive(Debug, Clone, PartialEq)]
pub struct Holder {
    /// The weapon in each hand, the carbine's then the pistol's, with the
    /// holder's row for it with nothing fitted.
    pub hands: [Option<(Record, Row)>; HANDS],
    /// The hand whose weapon is drawn.
    pub drawn: usize,
}

impl Default for Holder {
    /// Nobody: every figure is then read on a bought weapon and the
    /// tables' own row.
    fn default() -> Self {
        Self {
            hands: std::array::from_fn(|_| None),
            drawn: 0,
        }
    }
}

impl Holder {
    /// The holder of `hands`, fighting as `actor`; with no actor, by the
    /// installed tables' rows.
    #[must_use]
    pub fn of(items: &ItemCatalogue, hands: &Hands, actor: Option<&Combatant>) -> Self {
        let Ok(saved) = hands.saved(items) else {
            return Self::default();
        };
        let mut holder = Self::default();
        for (hand, record) in saved.into_iter().enumerate() {
            let Some(record) = record else {
                continue;
            };
            let Some(weapon) = fitting::weapon_of(items, &record) else {
                continue;
            };
            let row = actor.map_or_else(
                || Combatant::default().bare(weapon),
                |actor| actor.bare(weapon),
            );
            holder.hands[hand] = Some((record, row));
        }
        holder.drawn = actor.map_or(0, Combatant::hand);
        holder
    }
}

/// What a line's figures are worth in the game's units: the property the
/// line drives, at the low figure and at the high.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Means {
    /// What the line moves.
    pub property: Property,
    /// Whether the fight has a number for it; else the amounts are shares
    /// of the weapon's standard, and belong under "not in the fight yet".
    pub live: bool,
    /// What the low figure gives.
    pub low: Amount,
    /// What the high figure gives.
    pub high: Amount,
}

/// One line of what a schematic would make from what is picked, or of what
/// is on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePreview {
    /// The product's line, by its id.
    pub line: String,
    /// What a card calls it.
    pub name: String,
    /// What the materials allow.
    pub ceiling: u16,
    /// What assembly alone gives; for a session, the figure as it stands.
    pub floor: u16,
    /// For a module: what the floor and the ceiling do on the weapon it
    /// fits. None for a component, and for a line that drives nothing.
    pub means: Option<Means>,
    /// For a component: the product of the first schematic that weighs
    /// this line, and what the line would give there with that schematic's
    /// other inputs at store grade. None for a line nothing reads yet.
    pub downstream: Option<(Namespaced, Means)>,
    /// For a weapon: what the floor and the ceiling are worth to it. None
    /// for anything else.
    pub weapon: Option<WeaponMeans>,
}

/// What a weapon's line is worth, which no [`Property`] says: moddability
/// is how many mounts open and durability how many shots it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponMeans {
    /// Moddability: the mounts open at the low figure and at the high, of
    /// all the chassis has.
    Mounts {
        /// Open at the low figure.
        low: u32,
        /// Open at the high figure.
        high: u32,
        /// All the chassis has.
        of: u32,
    },
    /// Durability: the shots from full condition to where the lines start
    /// to slide, at the low figure and at the high.
    Shots {
        /// At the low figure.
        low: u32,
        /// At the high figure.
        high: u32,
    },
    /// Durability of a melee weapon, which wears by the cut: the cuts from
    /// full condition to where the lines start to slide.
    Cuts {
        /// At the low figure.
        low: u32,
        /// At the high figure.
        high: u32,
    },
    /// The weapon's signature line: the number of its own row, at the low
    /// figure and at the high, in the fitting screen's units.
    Row {
        /// The number of the row the line moves.
        property: Property,
        /// At the low figure.
        low: Amount,
        /// At the high figure.
        high: Amount,
        /// The row is a burst weapon's, whose interval is the pause between
        /// bursts rather than between shots.
        burst: bool,
    },
    /// A line the fight has no number for yet, the way a module's unlive
    /// property reads.
    NotInTheFight,
}

/// What the line `line` of the weapon `def` is worth at `low` and at
/// `high`: from the mounts' thresholds and from wear's cost of a shot.
pub fn weapon_means(
    items: &ItemCatalogue,
    def: &ItemDefinition,
    line: &str,
    (low, high): (u16, u16),
) -> WeaponMeans {
    let ItemKind::Weapon {
        mounts,
        family,
        signature,
        ..
    } = &def.kind
    else {
        return WeaponMeans::NotInTheFight;
    };
    if let (Some(signature), Some(weapon)) = (signature, Weapon::from_name(family))
        && signature.line == line
        && fitting::live_on(signature.property, weapon.is_melee())
    {
        let bare = Combatant::default().bare(weapon);
        let at = |figure: u16| {
            let mut record = bought(def);
            record.lines.insert(line.to_owned(), figure);
            fitting::lines(items, &bare, &record, &record, &[signature.property])
                .first()
                .map(|line| line.value)
        };
        if let (Some(low), Some(high)) = (at(low), at(high)) {
            return WeaponMeans::Row {
                property: signature.property,
                low,
                high,
                burst: matches!(bare.spec.mode, FireMode::Burst { .. }),
            };
        }
    }
    match line {
        MODDABILITY => {
            let open = |figure: u16| {
                let mut record = bought(def);
                record.lines.insert(MODDABILITY.to_owned(), figure);
                let count = mounts
                    .iter()
                    .filter(|mount| items.mount_opens(&record, &mount.slot) == Some(Ok(())))
                    .count();
                u32::try_from(count).unwrap_or(u32::MAX)
            };
            WeaponMeans::Mounts {
                low: open(low),
                high: open(high),
                of: u32::try_from(mounts.len()).unwrap_or(u32::MAX),
            }
        }
        DURABILITY => {
            let rules = items.wear();
            let usable = u32::from(rules.full.saturating_sub(rules.slide_below)) * 1000;
            let melee = Weapon::from_name(family).is_some_and(Weapon::is_melee);
            let per = if melee { rules.cut } else { rules.shot };
            let lasts = |figure: u16| {
                let mut record = bought(def);
                record.lines.insert(DURABILITY.to_owned(), figure);
                let cost = wear::cost(rules, per, &record);
                usable.checked_div(cost).unwrap_or(0)
            };
            let (low, high) = (lasts(low), lasts(high));
            if melee {
                WeaponMeans::Cuts { low, high }
            } else {
                WeaponMeans::Shots { low, high }
            }
        }
        _ => WeaponMeans::NotInTheFight,
    }
}

/// A bought example of `def`: every line at store grade, and for a weapon
/// in full condition with nothing fitted.
pub fn bought(def: &ItemDefinition) -> Record {
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
        condition: matches!(def.kind, ItemKind::Weapon { .. } | ItemKind::Vehicle { .. })
            .then_some(QUALITY_MAX),
        fitted: BTreeMap::new(),
        origin: None,
    }
}

/// The weapon a module made for `fits` is read on, bare, with its holder's
/// row: the first the holder carries, the drawn one first; else a bought
/// example of the first it is made for, on the tables' own row.
fn host(items: &ItemCatalogue, holder: &Holder, fits: &[Namespaced]) -> Option<(Record, Row)> {
    let carried = (0..HANDS)
        .map(|turn| (holder.drawn + turn) % HANDS)
        .filter_map(|hand| holder.hands[hand].as_ref())
        .find(|(weapon, _)| fits.contains(&weapon.item));
    if let Some((weapon, row)) = carried {
        return Some((fitting::bare_of(weapon), *row));
    }
    fits.iter().find_map(|item| {
        let def = items.get(item)?;
        let ItemKind::Weapon { family, .. } = &def.kind else {
            return None;
        };
        let weapon = Weapon::from_name(family)?;
        Some((bought(def), Combatant::default().bare(weapon)))
    })
}

/// What the line `line` of the module `module` is worth at `low` and at
/// `high`: the effect it drives, by the fitting step's own arithmetic on
/// the weapon the module fits, the module's other lines at store grade.
fn module_means(
    items: &ItemCatalogue,
    holder: &Holder,
    module: &ItemDefinition,
    line: &str,
    (low, high): (u16, u16),
) -> Option<Means> {
    let ItemKind::Module {
        slot: mount,
        fits,
        effects,
        ..
    } = &module.kind
    else {
        return None;
    };
    let effect = effects.iter().find(|effect| effect.line == line)?;
    let (weapon, row) = host(items, holder, fits)?;
    let at = |figure: u16| {
        let mut fitted = Fitted {
            item: module.id.clone(),
            batch: 0,
            by: None,
            lines: bought(module).lines,
            condition: None,
        };
        fitted.lines.insert(line.to_owned(), figure);
        let then = fitting::preview(items, &weapon, mount, Some(&fitted));
        fitting::lines(items, &row, &weapon, &then, &[effect.property])
            .first()
            .map(|line| line.value)
    };
    Some(Means {
        property: effect.property,
        live: fitting::live_for(items, &weapon, effect.property),
        low: at(low)?,
        high: at(high)?,
    })
}

/// Where the line `line` of the component `component` goes next and what
/// it is worth there: the first schematic that weighs it, that schematic's
/// other inputs at store grade. The low end is the line at `low` in a
/// product then left at its own floor, the high end the line at `high` in
/// one worked to its ceiling.
fn downstream_means(
    registries: &GameRegistries,
    holder: &Holder,
    component: &Namespaced,
    line: &str,
    (low, high): (u16, u16),
) -> Option<(Namespaced, Means)> {
    let (next, index) = registries.schematics.first_using(component, line)?;
    let ceiling_with = |figure: u16| {
        let sum: u32 = next.weights[index]
            .iter()
            .map(|(slot, stat, weight)| {
                let own = matches!(stat, Stat::Line(named) if named == line)
                    && matches!(&next.def.slots[*slot].takes, Takes::Component(item) if item == component);
                let read = if own { figure } else { stat.weighed(STORE_GRADE) };
                u32::from(*weight) * u32::from(read)
            })
            .sum();
        rounded(sum)
    };
    let rules = registries.schematics.rules();
    let range = (
        self::figure(ceiling_with(low), rules.assembly_pm()),
        ceiling_with(high),
    );
    let product = registries.items.get(&next.def.makes)?;
    let means = module_means(
        &registries.items,
        holder,
        product,
        &next.def.lines[index].line,
        range,
    )?;
    Some((next.def.makes.clone(), means))
}

/// One line of `schematic`'s product, standing between `floor` and
/// `ceiling`, with what those are worth.
fn line_preview(
    registries: &GameRegistries,
    holder: &Holder,
    schematic: &Schematic,
    index: usize,
    (floor, ceiling): (u16, u16),
) -> Option<LinePreview> {
    let product = registries.items.get(&schematic.def.makes)?;
    let line = product.lines.get(index)?;
    let (means, downstream) = match product.kind {
        ItemKind::Module { .. } => (
            module_means(
                &registries.items,
                holder,
                product,
                &line.id,
                (floor, ceiling),
            ),
            None,
        ),
        ItemKind::Component => (
            None,
            downstream_means(registries, holder, &product.id, &line.id, (floor, ceiling)),
        ),
        _ => (None, None),
    };
    let weapon = matches!(product.kind, ItemKind::Weapon { .. })
        .then(|| weapon_means(&registries.items, product, &line.id, (floor, ceiling)));
    Some(LinePreview {
        line: line.id.clone(),
        name: line.name.clone(),
        ceiling,
        floor,
        means,
        downstream,
        weapon,
    })
}

/// What `schematic` would make from `picks`, a line of its product each,
/// before anything is spent: the ceiling, the floor assembly alone gives,
/// and what both are worth in the game's units. `None` for a line while a
/// slot it weighs is empty.
#[must_use]
pub fn preview(
    registries: &GameRegistries,
    holder: &Holder,
    schematic: &Schematic,
    picks: &Picks,
) -> Vec<Option<LinePreview>> {
    let assembly = registries.schematics.rules().assembly_pm();
    preview_at(registries, holder, schematic, picks, assembly)
}

/// [`preview`] for the member at the bench: the floor is what assembly
/// gives on their sheet, since a session opens with that share.
#[must_use]
pub fn preview_for(
    registries: &GameRegistries,
    holder: &Holder,
    schematic: &Schematic,
    picks: &Picks,
    sheet: &CharacterSheet,
) -> Vec<Option<LinePreview>> {
    let assembly = assembly_pm(
        registries.schematics.rules(),
        &registries.progression,
        sheet,
    );
    preview_at(registries, holder, schematic, picks, assembly)
}

fn preview_at(
    registries: &GameRegistries,
    holder: &Holder,
    schematic: &Schematic,
    picks: &Picks,
    assembly: u16,
) -> Vec<Option<LinePreview>> {
    (0..schematic.weights.len())
        .map(|index| {
            let ceiling = ceiling(schematic, index, picks)?;
            let floor = figure(ceiling, assembly);
            line_preview(registries, holder, schematic, index, (floor, ceiling))
        })
        .collect()
}

/// What is on the bench, a line each: its ceiling, its figure as it stands
/// (in [`LinePreview::floor`]) and what that figure is worth. A module's
/// line reads one amount, low and high the same; a component's reads what
/// the part as it stands would give downstream, from that product's floor
/// to its ceiling. Empty for a session whose schematic the data set no
/// longer has.
#[must_use]
pub fn now(registries: &GameRegistries, holder: &Holder, session: &Session) -> Vec<LinePreview> {
    let Some(schematic) = registries.schematics.get(&session.schematic) else {
        return Vec::new();
    };
    session
        .lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let stands = line.figure();
            let mut shown = line_preview(registries, holder, schematic, index, (stands, stands))?;
            shown.ceiling = line.ceiling;
            Some(shown)
        })
        .collect()
}

/// `lift_pm` of `ceiling`, to the nearest whole, keeping its sign.
fn share_of(ceiling: u16, lift_pm: i16) -> i16 {
    let size = rounded(u32::from(ceiling) * u32::from(lift_pm.unsigned_abs()));
    let size = i16::try_from(size).unwrap_or(i16::MAX);
    if lift_pm < 0 { -size } else { size }
}

/// What a hand offers on a line, to be shown before the press: the odds
/// and what they would move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandOdds {
    /// The chance of each result, percent, in [`Outcome::ALL`]'s order:
    /// the five bars.
    pub odds: [u8; 5],
    /// What the likeliest result would add to the figure.
    pub most_often: i16,
    /// What the best result the hand can give would add.
    pub best: i16,
    /// What the worst would take off, for a hand that can go backwards.
    pub slip: Option<u16>,
}

/// The odds of `hand` on `line` for stock that averages `workability`, and
/// what its results would move the line's figure by. The amounts are
/// shares of the ceiling, so a line near its ceiling may gain less than
/// they say: it never passes it.
#[must_use]
pub fn hand_odds(
    rules: &SessionRules,
    line: &SessionLine,
    hand: Hand,
    workability: Option<u16>,
) -> HandOdds {
    let odds = rules.odds(hand, rules.row(workability));
    let lift = |outcome: Outcome| share_of(line.ceiling, rules.lift_pm(outcome));
    // The likeliest, and of two as likely the better: the first.
    let most_often = Outcome::ALL
        .into_iter()
        .rev()
        .max_by_key(|outcome| odds[outcome.index()])
        .map_or(0, lift);
    let possible = || {
        Outcome::ALL
            .into_iter()
            .filter(|outcome| odds[outcome.index()] > 0)
            .map(lift)
    };
    HandOdds {
        odds,
        most_often,
        best: possible().max().unwrap_or(0),
        slip: possible()
            .min()
            .filter(|worst| *worst < 0)
            .map(i16::unsigned_abs),
    }
}

/// The result of the roll numbered `counter` in the world of `seed`,
/// against a row of `odds` in percent: a point in a hundred, walked down
/// the row from the best result. The same counter in the same world is the
/// same point, whatever the row.
#[must_use]
pub fn roll(seed: Seed, counter: u64, odds: &[u8; 5]) -> Outcome {
    let drawn = mix64(seed.derive(ROLL_STREAM) ^ mix64(counter));
    // The high half scaled to a hundred: nought to ninety-nine.
    let mut point = ((drawn >> 32) * 100) >> 32;
    let mut index = 0;
    while index < Outcome::ALL.len() - 1 && point >= u64::from(odds[index]) {
        point -= u64::from(odds[index]);
        index += 1;
    }
    Outcome::ALL[index]
}

/// Lift `line` by what `outcome` is worth: its share moves by the result's
/// share of the ceiling and is held between nought and the whole of it, so
/// no result takes a line past what its materials allow. Gives the figure
/// before and after.
pub fn lift(rules: &SessionRules, line: &mut SessionLine, outcome: Outcome) -> (u16, u16) {
    let from = line.figure();
    let share = i32::from(line.share) + i32::from(rules.lift_pm(outcome));
    line.share = u16::try_from(share.clamp(0, i32::from(QUALITY_MAX))).unwrap_or(QUALITY_MAX);
    let to = line.figure();
    let moved = i16::try_from(i32::from(to) - i32::from(from)).unwrap_or(0);
    line.last = Some((outcome, moved));
    (from, to)
}

/// The stock, copied, for a transaction that must change all of it or
/// none.
pub struct Scratch {
    /// Public for the adapter half in `veyra-sim`, which calls it.
    pub pack: Pack,
    /// Public for the adapter half in `veyra-sim`, which calls it.
    pub hands: Hands,
    /// Public for the adapter half in `veyra-sim`, which calls it.
    pub store: Option<HouseholdStore>,
}

impl Scratch {
    /// Public for the adapter half in `veyra-sim`, which calls it.
    pub fn of(stock: &Stock<'_>) -> Self {
        Self {
            pack: stock.pack.clone(),
            hands: stock.hands.clone(),
            store: stock.store.as_deref().cloned(),
        }
    }

    fn stock(&mut self) -> Stock<'_> {
        Stock {
            saddle: None,
            pack: &mut self.pack,
            hands: &mut self.hands,
            store: self.store.as_mut(),
        }
    }

    /// Take `amount` of `wanted`'s kind out of the stock: from the pack's
    /// cells, then its bags' open cells, then the store, a stack at a time.
    /// Whether all of it was there.
    pub fn take(&mut self, items: &ItemCatalogue, wanted: &ItemStack, mut amount: u32) -> bool {
        let open: [u16; BAG_SLOTS] = std::array::from_fn(|bag| self.pack.open_cells(bag, items));
        let Pack { cells, bags, .. } = &mut self.pack;
        let all = cells.len();
        let mut sources: Vec<(&mut Inventory, usize)> = vec![(cells, all)];
        for (bag, open) in bags.iter_mut().zip(open) {
            sources.push((bag, usize::from(open)));
        }
        if let Some(store) = self.store.as_mut() {
            let all = store.0.len();
            sources.push((&mut store.0, all));
        }
        for (inventory, open) in sources {
            for cell in 0..open {
                if amount == 0 {
                    break;
                }
                let Some(present) = inventory.get(cell) else {
                    continue;
                };
                if !present.same_kind(wanted) {
                    continue;
                }
                let mut rest = present.clone();
                let taken = rest.count.min(amount);
                rest.count -= taken;
                amount -= taken;
                inventory.set(cell, (!rest.is_empty()).then_some(rest));
            }
            // The dirty mask is the sync layer's and part of an
            // inventory's equality; nothing here reads it.
            inventory.clear_changed();
        }
        amount == 0
    }

    /// Put `stack` away: with what the character carries when there is
    /// room, and only then in the store, as a module taken off a weapon is
    /// (`fitting::remove`): what was just made at the bench is in the hand
    /// that made it.
    pub fn put_away(
        &mut self,
        items: &ItemCatalogue,
        stack: ItemStack,
    ) -> Result<(), CraftRefusal> {
        let mut carried = Stock {
            saddle: None,
            pack: &mut self.pack,
            hands: &mut self.hands,
            store: None,
        };
        let Some(rest) = items::place(items, &mut carried, stack) else {
            return Ok(());
        };
        match items::place(items, &mut self.stock(), rest) {
            None => Ok(()),
            Some(_) => Err(CraftRefusal::NoRoom),
        }
    }

    /// Public for the adapter half in `veyra-sim`, which calls it.
    pub fn commit(self, stock: &mut Stock<'_>) {
        *stock.pack = self.pack;
        *stock.hands = self.hands;
        if let (Some(into), Some(store)) = (stock.store.as_mut(), self.store) {
            **into = store;
        }
    }
}

/// Assemble `schematic` from `picks`: every slot's amount leaves the pack,
/// its bags and the store, and a session opens with every line at the
/// assembly share of its ceiling and the points its complexity gives. One
/// transaction: a refusal has taken nothing and opened nothing. `by` is
/// who is at the bench and `sheet` theirs. Gives the points the session has.
///
/// # Errors
/// A session open already, no such schematic, a schematic `sheet` is not
/// taught, or the first slot whose pick will not do ([`check_picks`]).
pub fn assemble(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    state: &mut CraftState,
    schematic: &Namespaced,
    picks: &Picks,
    by: &str,
    sheet: &CharacterSheet,
) -> Result<u8, CraftRefusal> {
    if state.session.is_some() {
        return Err(CraftRefusal::SessionOpen);
    }
    let schematic = registries
        .schematics
        .get(schematic)
        .ok_or(CraftRefusal::UnknownSchematic)?;
    check_picks(
        registries,
        stock.pack,
        stock.store.as_deref(),
        schematic,
        picks,
        sheet,
    )?;
    let items = &registries.items;
    let rules = registries.schematics.rules();
    let mut scratch = Scratch::of(stock);
    let mut inputs = Vec::with_capacity(schematic.def.slots.len());
    for slot in &schematic.def.slots {
        let refused = |why| CraftRefusal::Slot {
            slot: slot.id.clone(),
            why,
        };
        let kind = picks
            .get(&slot.id)
            .map(kind_of)
            .ok_or_else(|| refused(SlotRefusal::Empty))?;
        let wanted = items::write(&kind, items).map_err(|_| refused(SlotRefusal::WrongKind))?;
        let needs = slot.amount();
        if !scratch.take(items, &wanted, needs) {
            return Err(refused(SlotRefusal::Short { have: 0, needs }));
        }
        inputs.push((
            slot.id.clone(),
            Record {
                count: needs,
                ..kind
            },
        ));
    }
    let share = assembly_pm(rules, &registries.progression, sheet);
    let mut lines = Vec::with_capacity(schematic.def.lines.len());
    for (index, recipe) in schematic.def.lines.iter().enumerate() {
        // Every slot is filled with something it takes, and a weight is on
        // what its slot is sure to carry, so there is a ceiling.
        let ceiling = ceiling(schematic, index, picks).ok_or(CraftRefusal::UnknownLine)?;
        lines.push(SessionLine {
            line: recipe.line.clone(),
            ceiling,
            share,
            last: None,
        });
    }
    let points = points(
        rules,
        &registries.progression,
        sheet,
        schematic.def.complexity,
    );
    scratch.commit(stock);
    state.session = Some(Session {
        schematic: schematic.def.id.clone(),
        by: by.to_owned(),
        inputs,
        lines,
        points,
        workability: workability(schematic, picks),
    });
    Ok(points)
}

/// What a roll did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rolled {
    /// The line the point was spent on.
    pub line: String,
    /// The hand it was spent with.
    pub hand: Hand,
    /// What the roll came to.
    pub outcome: Outcome,
    /// The line's figure before.
    pub from: u16,
    /// And after.
    pub to: u16,
    /// The points left.
    pub points: u8,
}

/// Whether a point of `session` can be spent on `line`: the reasons a roll
/// is refused for, in the order [`roll_line`] gives them, for a screen to
/// grey a hand by.
///
/// # Errors
/// No such line, no points left, or a line at its ceiling.
pub fn can_roll(session: &Session, line: &str) -> Result<(), CraftRefusal> {
    let target = session
        .lines
        .iter()
        .find(|own| own.line == line)
        .ok_or(CraftRefusal::UnknownLine)?;
    if session.points == 0 {
        return Err(CraftRefusal::NoPoints);
    }
    if target.at_ceiling() {
        return Err(CraftRefusal::AtCeiling);
    }
    Ok(())
}

/// Spend a point of the open session on `line` with `hand`: the roll
/// numbered by the household's counter is drawn against the hand's row,
/// the line is lifted by what it came to, and the counter moves on.
/// Refused, nothing is spent and the counter stands.
///
/// # Errors
/// No session, no such line, no points left, or a line at its ceiling.
pub fn roll_line(
    registries: &GameRegistries,
    seed: Seed,
    state: &mut CraftState,
    line: &str,
    hand: Hand,
) -> Result<Rolled, CraftRefusal> {
    let rules = registries.schematics.rules();
    let counter = state.rolls;
    let session = state.session.as_mut().ok_or(CraftRefusal::NoSession)?;
    can_roll(session, line)?;
    let workability = session.workability;
    let target = session
        .lines
        .iter_mut()
        .find(|own| own.line == line)
        .ok_or(CraftRefusal::UnknownLine)?;
    let odds = rules.odds(hand, rules.row(workability));
    let outcome = roll(seed, counter, &odds);
    let (from, to) = lift(rules, target, outcome);
    session.points -= 1;
    let points = session.points;
    state.rolls = counter.wrapping_add(1);
    Ok(Rolled {
        line: line.to_owned(),
        hand,
        outcome,
        from,
        to,
        points,
    })
}

/// The condition a made thing leaves the bench or the fabricator with: a
/// weapon or a vehicle whole, a part none until it wears. One rule for both, so a
/// recorded weapon is a record the catalogue accepts.
#[must_use]
pub fn made_condition(items: &ItemCatalogue, item: &Namespaced) -> Option<u16> {
    matches!(
        items.get(item).map(|def| &def.kind),
        Some(ItemKind::Weapon { .. } | ItemKind::Vehicle { .. })
    )
    .then_some(items.wear().full)
}

/// Finish the open session: the product is made, one of it, with the next
/// batch number, the name of who assembled it and its lines as they stand;
/// it goes to the pack, then a bag, then the store; what it was made from
/// is filed under its batch and the bench is clear. One transaction: with
/// no room nothing is made, no batch is spent and the session stays.
///
/// # Errors
/// No session, a schematic the data set no longer has, or no room.
pub fn finish(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    state: &mut CraftState,
) -> Result<Record, CraftRefusal> {
    let session = state.session.as_ref().ok_or(CraftRefusal::NoSession)?;
    let schematic = registries
        .schematics
        .get(&session.schematic)
        .ok_or(CraftRefusal::UnknownSchematic)?;
    let items = &registries.items;
    let batch = state.batches.saturating_add(1);
    let product = Record {
        item: schematic.def.makes.clone(),
        count: 1,
        batch,
        by: Some(session.by.clone()),
        lines: session.figures(),
        // A made weapon leaves the bench whole with nothing fitted; a part
        // has no condition until it wears.
        condition: made_condition(items, &schematic.def.makes),
        fitted: BTreeMap::new(),
        origin: None,
    };
    let made = items::write(&product, items).map_err(|_| CraftRefusal::UnknownSchematic)?;
    let mut scratch = Scratch::of(stock);
    scratch.put_away(items, made)?;
    scratch.commit(stock);
    state.batches = batch;
    if let Some(session) = state.session.take() {
        state.made.insert(
            batch,
            Made {
                item: product.item.clone(),
                by: session.by,
                from: session.inputs,
                schematic: Some(session.schematic),
                recorded: None,
                blueprint: None,
            },
        );
    }
    Ok(product)
}

/// The stack `count` of `item` bought at store grade make, and what they
/// cost.
fn sale(
    items: &ItemCatalogue,
    item: &Namespaced,
    count: u32,
) -> Result<(ItemStack, u32), CraftRefusal> {
    let def = items.get(item).ok_or(CraftRefusal::NotForSale)?;
    let price = def.price.ok_or(CraftRefusal::NotForSale)?;
    // A bike, a harvester and a weapon are sold at a board, not at a bench.
    if count == 0
        || matches!(
            def.kind,
            ItemKind::Vehicle { .. } | ItemKind::Harvester { .. } | ItemKind::Weapon { .. }
        )
    {
        return Err(CraftRefusal::NotForSale);
    }
    let mut stack = items::write(&bought(def), items).map_err(|_| CraftRefusal::NotForSale)?;
    // More than a stack holds is put away as several.
    stack.count = count;
    Ok((stack, price.saturating_mul(count)))
}

/// Whether `count` of `item` can be bought into this stock from `purse`,
/// asked in the order a player would fix them: that it is for sale, the
/// purse, the room. Gives what it would cost.
///
/// # Errors
/// The first of those that fails.
pub fn can_buy(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    purse: &Purse,
    item: &Namespaced,
    count: u32,
) -> Result<u32, CraftRefusal> {
    let mut scratch = Scratch {
        pack: pack.clone(),
        hands: Hands::default(),
        store: store.cloned(),
    };
    let mut paid = *purse;
    buy_into(registries, &mut scratch, &mut paid, item, count)
}

fn buy_into(
    registries: &GameRegistries,
    scratch: &mut Scratch,
    purse: &mut Purse,
    item: &Namespaced,
    count: u32,
) -> Result<u32, CraftRefusal> {
    let items = &registries.items;
    let (stack, cost) = sale(items, item, count)?;
    if cost > purse.0 {
        return Err(CraftRefusal::Short {
            needs: cost,
            have: purse.0,
        });
    }
    scratch.put_away(items, stack)?;
    purse.0 -= cost;
    Ok(cost)
}

/// Buy `count` of `item` at store grade: the price leaves the purse and
/// the stack goes to the pack, then a bag, then the store. The vendor of
/// the design's chain, made as small as it can be. One transaction: a
/// refusal has changed neither. Gives what was paid.
///
/// # Errors
/// Why not ([`can_buy`]).
pub fn buy(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    purse: &mut Purse,
    item: &Namespaced,
    count: u32,
) -> Result<u32, CraftRefusal> {
    let mut scratch = Scratch::of(stock);
    let mut paid = *purse;
    let cost = buy_into(registries, &mut scratch, &mut paid, item, count)?;
    scratch.commit(stock);
    *purse = paid;
    Ok(cost)
}

/// The Fabrication XP a finished session earns: `rates.made` for the
/// schematic's `complexity` (from 1; past the last it earns the last), the
/// `first` rate when the household had made none of it before, and the
/// `best` rate for each of `bests_beaten` lines.
#[must_use]
pub fn craft_xp(rates: &CraftRates, complexity: u8, first: bool, bests_beaten: u32) -> u32 {
    let at = usize::from(complexity.max(1)) - 1;
    let made = rates
        .made
        .get(at)
        .or_else(|| rates.made.last())
        .copied()
        .unwrap_or(0);
    let first = if first { rates.first } else { 0 };
    made.saturating_add(first)
        .saturating_add(rates.best.saturating_mul(bests_beaten))
}

/// The key a line's best is kept under.
#[must_use]
pub fn best_key(schematic: &Namespaced, line: &str) -> String {
    format!("{schematic}#{line}")
}

impl CraftState {
    /// Whether the household has made any of `schematic` before: an entry
    /// of the ledger that names it, or, in an entry written before
    /// blueprints, the item it makes.
    #[must_use]
    pub fn has_made(&self, registries: &GameRegistries, schematic: &Namespaced) -> bool {
        let makes = registries
            .schematics
            .get(schematic)
            .map(|found| &found.def.makes);
        self.made.values().any(|made| match &made.schematic {
            Some(own) => own == schematic,
            None => Some(&made.item) == makes,
        })
    }

    /// Record the finished `figures` of `schematic` against the household's
    /// bests and say how many lines beat one. A line with no best yet has
    /// one set and earns nothing, so a schematic's first batch sets them all.
    pub fn note_bests(&mut self, schematic: &Namespaced, figures: &BTreeMap<String, u16>) -> u32 {
        let mut beaten = 0;
        for (line, figure) in figures {
            match self.bests.entry(best_key(schematic, line)) {
                Entry::Vacant(slot) => {
                    slot.insert(*figure);
                }
                Entry::Occupied(mut slot) => {
                    if *figure > *slot.get() {
                        slot.insert(*figure);
                        beaten += 1;
                    }
                }
            }
        }
        beaten
    }

    /// Bring a saved state back inside the data set where an edit to the
    /// data, and not a broken file, has put it outside: a session with more
    /// points than the dial now gives is held to the dial, and a record of
    /// what went into something (the session's inputs, a ledger entry) that
    /// no longer reads is dropped, since it only words a card and the stock
    /// it names is spent. Said in words for a log, one for each thing
    /// changed; a state nothing was wrong with is left as it was. What is
    /// left for [`CraftState::check`] to refuse is what no function here
    /// can work on.
    pub fn repair(&mut self, registries: &GameRegistries) -> Vec<String> {
        let items = &registries.items;
        let mut said = Vec::new();
        if let Some(session) = self.session.as_mut()
            && let Some(schematic) = registries.schematics.get(&session.schematic)
        {
            let id = &session.schematic;
            let most = most_points(registries, schematic.def.complexity);
            if session.points > most {
                said.push(format!(
                    "the session of `{id}` had {} points, more than its complexity gives; it has {most}",
                    session.points
                ));
                session.points = most;
            }
            session.inputs.retain(|(slot, record)| {
                let why = if schematic.slot(slot).is_none() {
                    Some("the schematic has no such slot".to_owned())
                } else {
                    items.check_record(record).err().map(|e| e.to_string())
                };
                if let Some(why) = &why {
                    said.push(format!(
                        "the session of `{id}` no longer says what went into slot `{slot}` ({why})"
                    ));
                }
                why.is_none()
            });
        }
        self.made.retain(|batch, made| {
            let why = if items.get(&made.item).is_none() {
                Some(format!("`{}` is no item", made.item))
            } else {
                made.from.iter().find_map(|(slot, record)| {
                    let error = items.check_record(record).err()?;
                    Some(format!("slot `{slot}`: {error}"))
                })
            };
            if let Some(why) = &why {
                said.push(format!(
                    "batch {batch} no longer says what it was made from ({why})"
                ));
            }
            why.is_none()
        });
        self.bests.retain(|key, _| {
            let known = key.split_once('#').is_some_and(|(schematic, line)| {
                Namespaced::parse(schematic)
                    .ok()
                    .and_then(|id| registries.schematics.get(&id))
                    .is_some_and(|found| found.def.lines.iter().any(|own| own.line == line))
            });
            if !known {
                said.push(format!(
                    "the best `{key}` is of no line of any schematic, and is dropped"
                ));
            }
            known
        });
        said
    }

    /// What is wrong with a saved state against the data set, if anything,
    /// once [`CraftState::repair`] has had it: a session of a schematic the
    /// data no longer has, lines that are not the product's, or a figure out
    /// of range. A state that passes is one every function here can work
    /// on.
    ///
    /// # Errors
    /// The first rule broken, in words for a log.
    pub fn check(&self, registries: &GameRegistries) -> Result<(), String> {
        let Some(session) = &self.session else {
            return Ok(());
        };
        let id = &session.schematic;
        let schematic = registries
            .schematics
            .get(id)
            .ok_or_else(|| format!("the session is of `{id}`, which is no schematic"))?;
        let given: Vec<&str> = session
            .lines
            .iter()
            .map(|line| line.line.as_str())
            .collect();
        let expected: Vec<&str> = schematic
            .def
            .lines
            .iter()
            .map(|line| line.line.as_str())
            .collect();
        if given != expected {
            return Err(format!(
                "the session of `{id}` has the lines {given:?}, its product has {expected:?}"
            ));
        }
        if let Some(line) = session
            .lines
            .iter()
            .find(|line| line.ceiling > QUALITY_MAX || line.share > QUALITY_MAX)
        {
            return Err(format!(
                "the session's line `{}` stands at {} per-mille of {}, over {QUALITY_MAX}",
                line.line, line.share, line.ceiling
            ));
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    use crate::combat::Combatant;

    use crate::fitting::Unit;

    use crate::household::Purse;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;

    use crate::items::Pack;
    use crate::items::Stock;
    use crate::progression::CharacterSheet;

    use slotted_model::Inventory;

    use std::collections::BTreeMap;

    use std::sync::Arc;
    use strata_core::Seed;

    use strata_registry::Attribute;

    use strata_registry::Namespaced;

    use strata_registry::item::ItemCatalogue;

    use strata_registry::item::LotOrigin;

    use strata_registry::item::Property;

    use strata_registry::item::Record;
    use strata_registry::item::STORE_GRADE;

    use strata_registry::schematic::Hand;
    use strata_registry::schematic::Outcome;
    use strata_registry::schematic::Schematic;
    use strata_registry::schematic::SchematicDef;
    use strata_registry::schematic::SchematicSet;

    use strata_registry::schematic::Stat;

    use strata_types::Registries;

    pub const CORE: &str = "veyra:schematic.cell_core";

    pub const FEED: &str = "veyra:schematic.patrol_extended_feed";

    pub const CORE_ITEM: &str = "veyra:component.cell_core";

    pub const HARNESS: &str = "veyra:component.harness";

    pub const SEALS: &str = "veyra:commodity.seal_set";

    pub const FEED_ITEM: &str = "veyra:module.patrol_extended_feed";

    pub const HAUL: &str = "veyra:bag.haul_pack";

    pub const SALT: &str = "strata:evaporite_salt";

    pub const SULPHUR: &str = "strata:sulphur";

    pub const BRINE: &str = "strata:coastal_brine";

    pub const ALLOY: &str = "strata:light_alloy_ore";

    pub const MAKER: &str = "Frontier ranger";

    /// A sheet that has learned every box of the shipped graph that teaches
    /// a schematic, and none that adds points: whoever stands at the bench
    /// in a test that is not about who may or how many points.
    pub fn smith() -> CharacterSheet {
        CharacterSheet {
            learned: shipped()
                .0
                .progression
                .boxes()
                .filter(|b| b.effects.is_empty())
                .map(|b| b.id.clone())
                .collect(),
            ..CharacterSheet::default()
        }
    }

    pub fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).unwrap()
    }

    /// A lot of `resource` drawn from a deposit named `root`, with
    /// `figures` in its class's order.
    pub fn lot(
        registries: &Registries,
        resource: &str,
        root: &str,
        units: u32,
        figures: &[u16],
    ) -> Record {
        let def = registries.0.items.lot_of(&id(resource)).unwrap();
        assert_eq!(def.lines.len(), figures.len(), "{resource}");
        // A deposit of its own a name, so two lots of one type never merge.
        let cell = i32::try_from(root.bytes().map(u32::from).sum::<u32>()).unwrap();
        let record = Record {
            item: def.id.clone(),
            count: units,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .zip(figures)
                .map(|(line, figure)| (line.id.clone(), *figure))
                .collect(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: Some(LotOrigin {
                deposit: [cell, 0],
                root: root.to_owned(),
                place: format!("{root} flats"),
            }),
        };
        assert_eq!(registries.0.items.check_record(&record), Ok(()));
        record
    }

    /// The worked example's stock (crafting.md, 5.5): Calder salt, Serein
    /// brine and Harrow light alloy.
    pub fn worked(registries: &Registries) -> [Record; 3] {
        [
            lot(registries, SALT, "Calder", 160, &[640, 820, 450]),
            lot(registries, BRINE, "Serein", 160, &[600, 560, 730]),
            lot(
                registries,
                ALLOY,
                "Harrow",
                200,
                &[610, 540, 560, 410, 520, 700, 320, 760],
            ),
        ]
    }

    /// The first day's lots at the default seed, as the game draws them
    /// from the deposits nearest the spawn: Tolley salt, Oxbow brine and
    /// Rake light alloy.
    pub fn first_lots(registries: &Registries) -> [Record; 3] {
        [
            lot(registries, SALT, "Tolley", 160, &[389, 632, 225]),
            lot(registries, BRINE, "Oxbow", 160, &[534, 605, 495]),
            lot(
                registries,
                ALLOY,
                "Rake",
                200,
                &[533, 298, 312, 231, 451, 633, 165, 492],
            ),
        ]
    }

    /// A stack of `count` of `item` as the dealer sells it, or with `lines`
    /// as a batch somebody made.
    pub fn part(
        registries: &Registries,
        item: &str,
        count: u32,
        made: Option<(u32, &[u16])>,
    ) -> Record {
        let def = registries.0.items.get(&id(item)).unwrap();
        let mut record = bought(def);
        record.count = count;
        if let Some((batch, lines)) = made {
            record.batch = batch;
            record.by = Some(MAKER.to_owned());
            for (line, figure) in def.lines.iter().zip(lines) {
                record.lines.insert(line.id.clone(), *figure);
            }
        }
        assert_eq!(registries.0.items.check_record(&record), Ok(()));
        record
    }

    /// The arrival's first-day pack and weapons, and the first day's store
    /// with `more` after it.
    pub fn bench(registries: &Registries, more: &[Record]) -> (Pack, Hands, HouseholdStore) {
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let arrival = &registries.0.sheets.household.as_ref().unwrap().arrival;
        let pack = Pack::from_saved(&starting.packs[&arrival.name], items).unwrap();
        let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).unwrap();
        let store: Vec<Option<Record>> = starting
            .store
            .iter()
            .chain(more)
            .cloned()
            .map(Some)
            .collect();
        let store = HouseholdStore::from_saved(&store, items).unwrap();
        (pack, hands, store)
    }

    pub fn schematic<'a>(registries: &'a Registries, name: &str) -> &'a Schematic {
        registries.0.schematics.get(&id(name)).unwrap()
    }

    /// How much of `kind` an inventory holds.
    pub fn units(inventory: &Inventory, kind: &Record, items: &ItemCatalogue) -> u32 {
        inventory
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, items).ok())
            .filter(|record| kind_of(record) == kind_of(kind))
            .map(|record| record.count)
            .sum()
    }

    pub fn put(inventory: &mut Inventory, cell: usize, record: &Record, items: &ItemCatalogue) {
        inventory.set(cell, Some(items::write(record, items).unwrap()));
        inventory.clear_changed();
    }

    /// Every empty cell of `inventory` up to `open` taken by something
    /// that is no part, so nothing merges onto it.
    pub fn fill(inventory: &mut Inventory, open: usize, items: &ItemCatalogue) {
        let haul = items::write(&bought(items.get(&id(HAUL)).unwrap()), items).unwrap();
        for cell in 0..open {
            if inventory.get(cell).is_none() {
                inventory.set(cell, Some(haul.clone()));
            }
        }
        inventory.clear_changed();
    }

    /// A line's ceiling and floor, and the digits of what they are worth.
    pub fn read(line: Option<&LinePreview>) -> (u16, u16, Option<(String, String)>) {
        let line = line.unwrap();
        let worth = line
            .means
            .or(line.downstream.as_ref().map(|(_, means)| *means))
            .map(|means| (means.low.text(), means.high.text()));
        (line.ceiling, line.floor, worth)
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "set against what a line is worth, which may be nothing"
    )]
    pub fn worth(low: &str, high: &str) -> Option<(String, String)> {
        Some((low.to_owned(), high.to_owned()))
    }

    /// crafting.md 5.5, line by line: the cell core's charge density from
    /// Calder salt and Serein brine has a ceiling of 724 and is assembled
    /// at 0.6 of it; stability, weighed in the same spirit, 510 and 306.
    #[test]
    fn the_worked_example_reads_724_and_434() {
        let registries = shipped();
        let [salt, brine, _] = worked(&registries);
        let core = schematic(&registries, CORE);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&salt)),
            ("brine".to_owned(), kind_of(&brine)),
        ]);
        // 0.5 * 820 + 0.3 * 560 + 0.2 * 730.
        assert_eq!(ceiling(core, 0, &picks), Some(724));
        assert_eq!(figure(724, 600), 434);
        // 0.6 * 450 + 0.4 * 600.
        assert_eq!(ceiling(core, 1, &picks), Some(510));
        assert_eq!(figure(510, 600), 306);
        let lines = preview(&registries.0, &Holder::default(), core, &picks);
        assert_eq!(lines.len(), 2);
        let charge = lines[0].as_ref().unwrap();
        assert_eq!(
            (charge.line.as_str(), charge.name.as_str()),
            ("charge_density", "Charge density")
        );
        assert_eq!((charge.ceiling, charge.floor), (724, 434));
        assert_eq!(read(lines[1].as_ref()), (510, 306, None));
        // While a slot a line weighs is empty the line has no ceiling.
        let mut short = picks.clone();
        short.remove("brine");
        assert_eq!(ceiling(core, 0, &short), None);
        let lines = preview(&registries.0, &Holder::default(), core, &short);
        assert_eq!(lines, [None, None]);
        // A half rounds up, in whole numbers: 0.3 of 605 is 181.5.
        let [salt, brine, _] = first_lots(&registries);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&salt)),
            ("brine".to_owned(), kind_of(&brine)),
        ]);
        assert_eq!(ceiling(core, 0, &picks), Some(597), "596.5");
    }

    /// The rest of 5.5: three points, each a Good, bring the core to 652;
    /// the magazine made of it, a bought harness and Harrow light alloy
    /// has ceilings of 608 and 624; four Goods and one finish it at 608
    /// and 437, which on the carbine is 29 rounds and a reload of 1.49 s.
    #[test]
    fn three_goods_give_652_then_608_and_437() {
        let registries = shipped();
        let rules = registries.0.schematics.rules();
        let [.., alloy] = worked(&registries);
        let mut charge = SessionLine {
            line: "charge_density".to_owned(),
            ceiling: 724,
            share: rules.assembly_pm(),
            last: None,
        };
        assert_eq!(charge.figure(), 434);
        for _ in 0..3 {
            lift(rules, &mut charge, Outcome::Good);
        }
        assert_eq!((charge.share, charge.figure()), (900, 652));
        // The last Good took it from 0.8 to 0.9 of 724: 579 to 652.
        assert_eq!(charge.last, Some((Outcome::Good, 73)));

        let feed = schematic(&registries, FEED);
        let picks = Picks::from([
            (
                "core".to_owned(),
                part(&registries, CORE_ITEM, 1, Some((1, &[652, 306]))),
            ),
            ("harness".to_owned(), part(&registries, HARNESS, 1, None)),
            ("casing".to_owned(), kind_of(&alloy)),
            ("seals".to_owned(), part(&registries, SEALS, 1, None)),
        ]);
        // 0.60 * 652 + 0.25 * 500 + 0.15 * 610, and
        // 0.60 * (1000 - 320) + 0.40 * 540.
        let lines = preview(&registries.0, &Holder::default(), feed, &picks);
        assert_eq!(read(lines[0].as_ref()), (608, 365, worth("28", "29")));
        assert_eq!(read(lines[1].as_ref()), (624, 374, worth("1.50", "1.47")));
        let capacity = lines[0].as_ref().unwrap().means.unwrap();
        assert_eq!(
            (capacity.property, capacity.live, capacity.low.unit),
            (Property::Capacity, true, Unit::Rounds)
        );
        let reload = lines[1].as_ref().unwrap().means.unwrap();
        assert_eq!(
            (reload.property, reload.live, reload.high.unit),
            (Property::ReloadTime, true, Unit::Seconds)
        );

        let mut session = Session {
            schematic: feed.def.id.clone(),
            by: MAKER.to_owned(),
            inputs: Vec::new(),
            lines: vec![
                SessionLine {
                    line: "capacity".to_owned(),
                    ceiling: 608,
                    share: rules.assembly_pm(),
                    last: None,
                },
                SessionLine {
                    line: "reload_time".to_owned(),
                    ceiling: 624,
                    share: rules.assembly_pm(),
                    last: None,
                },
            ],
            points: rules.points(feed.def.complexity),
            workability: workability(feed, &picks),
        };
        assert_eq!(session.points, 5);
        for _ in 0..4 {
            lift(rules, &mut session.lines[0], Outcome::Good);
        }
        lift(rules, &mut session.lines[1], Outcome::Good);
        assert_eq!(
            session.figures(),
            BTreeMap::from([
                ("capacity".to_owned(), 608),
                ("reload_time".to_owned(), 437)
            ])
        );
        assert!(session.lines[0].at_ceiling() && !session.lines[1].at_ceiling());
        let stands = now(&registries.0, &Holder::default(), &session);
        assert_eq!(read(stands.first()), (608, 608, worth("29", "29")));
        assert_eq!(read(stands.get(1)), (624, 437, worth("1.49", "1.49")));
    }

    /// The first day's real lots at the default seed: a core of 597 and
    /// 358, a magazine of 563 and 338 from the best of cores and 420 and
    /// 252 from one nobody worked, and what the screen says of each.
    #[test]
    fn the_first_days_lots_read_597_and_563() {
        let registries = shipped();
        let [salt, brine, alloy] = first_lots(&registries);
        let holder = Holder::default();
        let core = schematic(&registries, CORE);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&salt)),
            ("brine".to_owned(), kind_of(&brine)),
        ]);
        let lines = preview(&registries.0, &holder, core, &picks);
        // In a patrol extended magazine: 27 to 29 rounds.
        assert_eq!(read(lines[0].as_ref()), (597, 358, worth("27", "29")));
        let next = lines[0].as_ref().unwrap().downstream.as_ref().unwrap();
        assert_eq!(next.0, id(FEED_ITEM));
        // Goes into nothing yet.
        assert_eq!(read(lines[1].as_ref()), (349, 209, None));

        let feed = schematic(&registries, FEED);
        let with_core = |charge: u16| {
            let picks = Picks::from([
                (
                    "core".to_owned(),
                    part(&registries, CORE_ITEM, 1, Some((1, &[charge, 209]))),
                ),
                ("harness".to_owned(), part(&registries, HARNESS, 1, None)),
                ("casing".to_owned(), kind_of(&alloy)),
                ("seals".to_owned(), part(&registries, SEALS, 1, None)),
            ]);
            preview(&registries.0, &holder, feed, &picks)
        };
        let best = with_core(597);
        assert_eq!(read(best[0].as_ref()), (563, 338, worth("28", "29")));
        assert_eq!(read(best[1].as_ref()), (620, 372, worth("1.50", "1.47")));
        let unworked = with_core(358);
        assert_eq!(read(unworked[0].as_ref()), (420, 252, worth("27", "28")));
        assert_eq!(
            read(unworked[1].as_ref()),
            (620, 372, worth("1.50", "1.47"))
        );
        // Three Goods on the core are 537, and its magazine 527 and 316.
        assert_eq!(figure(597, 900), 537);
        let good = with_core(537);
        assert_eq!(read(good[0].as_ref()), (527, 316, worth("28", "29")));

        // On the core's charge density: steady most often adds 60; bold
        // can add 96 and can slip 24. Nothing in its slots is workable, so
        // these are the journeyman's rows.
        let rules = registries.0.schematics.rules();
        assert_eq!(workability(core, &picks), None);
        let line = SessionLine {
            line: "charge_density".to_owned(),
            ceiling: 597,
            share: 600,
            last: None,
        };
        let steady = hand_odds(rules, &line, Hand::Steady, None);
        assert_eq!(steady.odds, [5, 55, 35, 5, 0]);
        assert_eq!(
            (steady.most_often, steady.best, steady.slip),
            (60, 96, None)
        );
        let bold = hand_odds(rules, &line, Hand::Bold, None);
        assert_eq!(bold.odds, [25, 25, 25, 15, 10]);
        assert_eq!((bold.most_often, bold.best, bold.slip), (96, 96, Some(24)));
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one stock, read slot by slot")]
    fn candidates_are_best_first_and_a_short_stack_says_why() {
        let registries = shipped();
        let items = &registries.0.items;
        let [calder, serein, harrow] = worked(&registries);
        let [mut tolley, ..] = first_lots(&registries);
        tolley.count = 12;
        let sulphur = lot(&registries, SULPHUR, "Brimmer", 40, &[400, 900, 300]);
        let good_core = part(&registries, CORE_ITEM, 1, Some((2, &[652, 306])));
        let poor_core = part(&registries, CORE_ITEM, 2, Some((1, &[400, 510])));
        let (mut pack, _, store) = bench(
            &registries,
            &[
                tolley.clone(),
                sulphur.clone(),
                calder.clone(),
                serein.clone(),
                harrow.clone(),
                poor_core.clone(),
                good_core.clone(),
            ],
        );
        // Five more units of the Calder salt are carried.
        let mut carried = calder.clone();
        carried.count = 5;
        put(&mut pack.cells, 3, &carried, items);

        let core = schematic(&registries, CORE);
        let found = candidates(&registries.0, &pack, Some(&store), core, "salt");
        let names: Vec<&str> = found
            .iter()
            .map(|c| c.kind.origin.as_ref().unwrap().root.as_str())
            .collect();
        // By 0.5 of potency and 0.6 of stability: 680, 630 and 451.
        assert_eq!(
            names,
            ["Calder", "Brimmer", "Tolley"],
            "sulphur is a reactive too"
        );
        assert_eq!(
            found.iter().map(|c| c.score).collect::<Vec<_>>(),
            [680_000, 630_000, 451_000]
        );
        assert_eq!(
            (found[0].carried, found[0].stored, found[0].held()),
            (5, 160, 165)
        );
        assert_eq!(
            found[0].kind,
            kind_of(&calder),
            "a pick is a kind, its count one"
        );
        assert_eq!(
            found[0].stats,
            [
                (Stat::Attribute(Attribute::Potency), 820),
                (Stat::Attribute(Attribute::Stability), 450)
            ]
        );
        assert_eq!(found[0].short, None);
        assert_eq!(found[1].short, None, "forty units of sulphur will do");
        // Twelve units, needs 20: listed where its figures put it, and
        // said.
        assert_eq!(found[2].short, Some((12, 20)));
        // Without the store only what is carried is there, and it is short.
        let carried_only = candidates(&registries.0, &pack, None, core, "salt");
        assert_eq!(carried_only.len(), 1);
        assert_eq!(carried_only[0].short, Some((5, 20)));
        assert!(!can_make(&registries.0, &pack, None, core, &smith()));
        assert!(can_make(&registries.0, &pack, Some(&store), core, &smith()));

        // A component is read by its lines as a lot by its attributes, and
        // two batches of one part are two candidates.
        let feed = schematic(&registries, FEED);
        let cores = candidates(&registries.0, &pack, Some(&store), feed, "core");
        assert_eq!(
            cores
                .iter()
                .map(|c| (c.kind.batch, c.held()))
                .collect::<Vec<_>>(),
            [(2, 1), (1, 2)]
        );
        assert_eq!(
            cores[0].stats,
            [(Stat::Line("charge_density".to_owned()), 652)]
        );
        // A casing's density is shown as it is, and less is better.
        let casings = candidates(&registries.0, &pack, Some(&store), feed, "casing");
        assert_eq!(casings.len(), 1);
        assert_eq!(
            casings[0].stats,
            [
                (Stat::Attribute(Attribute::Purity), 610),
                (Stat::Light(Attribute::Density), 320),
                (Stat::Attribute(Attribute::Strength), 540),
            ]
        );
        // 0.15 * 610 + 0.60 * 680 + 0.40 * 540, in per-mille.
        assert_eq!(casings[0].score, 91_500 + 408_000 + 216_000);
        // Nothing weighs the seals: a candidate all the same, with no
        // figure to show.
        let seals = candidates(&registries.0, &pack, Some(&store), feed, "seals");
        assert_eq!(seals.len(), 1);
        assert_eq!((seals[0].score, seals[0].held()), (0, 6));
        assert!(seals[0].stats.is_empty());
        assert!(candidates(&registries.0, &pack, Some(&store), feed, "muzzle").is_empty());

        // Best stock is the first of each list that will do.
        let best = best_stock(&registries.0, &pack, Some(&store), core);
        assert_eq!(best["salt"], kind_of(&calder));
        assert_eq!(best["brine"], kind_of(&serein));
        let best = best_stock(&registries.0, &pack, Some(&store), feed);
        assert_eq!(best["core"], kind_of(&good_core));
        assert_eq!(best["casing"], kind_of(&harrow));
        assert_eq!(best.len(), 4);
        // With only the short salt in stock the slot has no pick.
        let (bare, _, lean) = bench(&registries, &[tolley, serein]);
        let best = best_stock(&registries.0, &bare, Some(&lean), core);
        assert_eq!(best.keys().collect::<Vec<_>>(), ["brine"]);
        assert!(!can_make(&registries.0, &bare, Some(&lean), core, &smith()));
    }

    /// The shipped data with one schematic in place of its own: two slots
    /// that take the same type, each `units` of it.
    pub fn twin(units: u32) -> Registries {
        let mut registries = (*shipped().0).clone();
        let text = format!(
            r#"(id: "test:schematic.twin", makes: "veyra:component.harness", complexity: 1,
                slots: [
                    (id: "a", name: "A", takes: Type("strata:light_alloy_ore"), units: {units}),
                    (id: "b", name: "B", takes: Type("strata:light_alloy_ore"), units: {units}),
                ],
                lines: [(line: "conductance", from: [
                    (slot: "a", stat: Attribute(Purity), weight: 0.5),
                    (slot: "b", stat: Attribute(Purity), weight: 0.5),
                ])])"#
        );
        let def: SchematicDef = ron::from_str(&text).unwrap();
        let rules = registries.schematics.rules().clone();
        registries.schematics = SchematicSet::build(
            vec![("schematics/twin.schematic.ron".to_owned(), def)],
            rules,
            &registries.items,
            &registries.resources,
        )
        .unwrap();
        Registries(Arc::new(registries))
    }

    #[test]
    fn best_stock_never_promises_a_stack_twice() {
        let registries = twin(150);
        let twin_id = id("test:schematic.twin");
        let schematic = registries.0.schematics.get(&twin_id).unwrap();
        let [.., harrow] = worked(&registries);
        let [.., rake] = first_lots(&registries);

        // One lot of 200 fills one slot of 150 and not two.
        let (mut pack, mut hands, mut store) = bench(&registries, std::slice::from_ref(&harrow));
        let best = best_stock(&registries.0, &pack, Some(&store), schematic);
        assert_eq!(best.keys().collect::<Vec<_>>(), ["a"]);
        assert!(!can_make(
            &registries.0,
            &pack,
            Some(&store),
            schematic,
            &smith()
        ));
        let both = Picks::from([
            ("a".to_owned(), kind_of(&harrow)),
            ("b".to_owned(), kind_of(&harrow)),
        ]);
        let short = CraftRefusal::Slot {
            slot: "b".to_owned(),
            why: SlotRefusal::Short {
                have: 50,
                needs: 150,
            },
        };
        assert_eq!(
            check_picks(
                &registries.0,
                &pack,
                Some(&store),
                schematic,
                &both,
                &smith()
            ),
            Err(short.clone())
        );
        let before = (pack.clone(), store.clone());
        let mut state = CraftState::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &twin_id,
                &both,
                MAKER,
                &smith()
            ),
            Err(short)
        );
        assert_eq!((pack, store), before, "nothing was taken for slot a either");
        assert_eq!(state, CraftState::default());

        // With a second lot the second slot takes the next best, and the
        // odds are moved by the mean of the two lots' workability.
        let (pack, _, store) = bench(&registries, &[rake.clone(), harrow.clone()]);
        let best = best_stock(&registries.0, &pack, Some(&store), schematic);
        assert_eq!(best["a"], kind_of(&harrow), "purity 610 before 533");
        assert_eq!(best["b"], kind_of(&rake));
        assert_eq!(workability(schematic, &best), Some(626), "of 760 and 492");
        assert_eq!(
            ceiling(schematic, 0, &best),
            Some(572),
            "of 610 and 533, 571.5"
        );

        // Two slots of 100 are one lot of 200 exactly.
        let registries = twin(100);
        let schematic = registries.0.schematics.get(&twin_id).unwrap();
        let (mut pack, mut hands, mut store) = bench(&registries, std::slice::from_ref(&harrow));
        let best = best_stock(&registries.0, &pack, Some(&store), schematic);
        assert_eq!(best, both);
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &twin_id,
                &best,
                MAKER,
                &smith()
            ),
            Ok(3)
        );
        assert_eq!(units(&store.0, &harrow, &registries.0.items), 0);
        let session = state.session.unwrap();
        assert_eq!(session.inputs[0].1.count + session.inputs[1].1.count, 200);
        assert_eq!(session.workability, Some(760));
    }

    /// A better core is more rounds, and the screen says so while the
    /// stock is all still where it was: every figure here is asked of pure
    /// functions that are handed the stock to read.
    #[test]
    fn a_pick_moves_the_rounds_before_anything_is_spent() {
        let registries = shipped();
        let [.., harrow] = worked(&registries);
        let [.., rake] = first_lots(&registries);
        let fine = part(&registries, CORE_ITEM, 1, Some((2, &[950, 400])));
        let plain = part(&registries, CORE_ITEM, 1, Some((1, &[358, 209])));
        let (pack, hands, store) = bench(
            &registries,
            &[harrow.clone(), rake.clone(), plain.clone(), fine.clone()],
        );
        let before = (pack.clone(), store.clone());
        let holder = Holder::of(&registries.0.items, &hands, Some(&Combatant::default()));
        let feed = schematic(&registries, FEED);
        let mut picks = best_stock(&registries.0, &pack, Some(&store), feed);
        assert_eq!(picks["core"], fine);
        assert_eq!(picks["casing"], kind_of(&harrow));
        let lines = preview(&registries.0, &holder, feed, &picks);
        // 0.60 * 950 + 0.25 * 500 + 0.15 * 610.
        assert_eq!(read(lines[0].as_ref()), (787, 472, worth("29", "30")));
        assert_eq!(read(lines[1].as_ref()), (624, 374, worth("1.50", "1.47")));

        // 0.60 * 358 + 0.25 * 500 + 0.15 * 610: a round fewer at each end.
        picks.insert("core".to_owned(), plain);
        let lines = preview(&registries.0, &holder, feed, &picks);
        assert_eq!(read(lines[0].as_ref()), (431, 259, worth("27", "28")));
        assert_eq!(read(lines[1].as_ref()), (624, 374, worth("1.50", "1.47")));

        // The casing moves the handling, and a little of the capacity.
        picks.insert("casing".to_owned(), kind_of(&rake));
        let lines = preview(&registries.0, &holder, feed, &picks);
        assert_eq!(read(lines[0].as_ref()), (420, 252, worth("27", "28")));
        assert_eq!(read(lines[1].as_ref()), (620, 372, worth("1.50", "1.47")));
        assert_eq!((pack, store), before);

        // The pistol's cell is read on the pistol, wherever it is held:
        // 10 rounds bare and 1.25 of that bought, so 12.4 at the floor and
        // 13.1 at the ceiling.
        let cell = schematic(&registries, "veyra:schematic.pistol_extended_cell");
        let picks = best_stock(&registries.0, &before.0, Some(&before.1), cell);
        assert_eq!(picks.len(), 4);
        let lines = preview(&registries.0, &holder, cell, &picks);
        assert_eq!(read(lines[0].as_ref()), (787, 472, worth("12", "13")));
        let capacity = lines[0].as_ref().unwrap().means.unwrap();
        assert_eq!(capacity.low.unit, Unit::Rounds);
        // With nobody at the bench the figures are a bought weapon's on
        // the tables' row, which for the arrival is the same row.
        let nobody = preview(&registries.0, &Holder::default(), cell, &picks);
        assert_eq!(nobody, lines);
    }

    #[test]
    fn a_component_says_what_it_gives_downstream() {
        let registries = shipped();
        let [salt, brine, _] = worked(&registries);
        let core = schematic(&registries, CORE);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&salt)),
            ("brine".to_owned(), kind_of(&brine)),
        ]);
        let holder = Holder::default();
        let lines = preview(&registries.0, &holder, core, &picks);
        let charge = lines[0].as_ref().unwrap();
        assert_eq!(charge.means, None, "a component does nothing by itself");
        let (product, means) = charge.downstream.as_ref().unwrap();
        // The first schematic by id that weighs the line: the patrol
        // magazine, its harness and casing at store grade. At the core's
        // floor of 434 the magazine's capacity has a ceiling of 460 and is
        // assembled at 276; at the core's ceiling of 724 it can be worked
        // to 634.
        assert_eq!(*product, id(FEED_ITEM));
        assert_eq!((means.property, means.live), (Property::Capacity, true));
        assert_eq!(
            (means.low.text(), means.high.text()),
            ("28".to_owned(), "30".to_owned())
        );
        // Stability goes into nothing yet.
        let stability = lines[1].as_ref().unwrap();
        assert_eq!((stability.means, &stability.downstream), (None, &None));

        // On the bench the part as it stands is read the same way: from
        // the magazine's floor to its ceiling with this core in it.
        let rules = registries.0.schematics.rules();
        let session = Session {
            schematic: core.def.id.clone(),
            by: MAKER.to_owned(),
            inputs: Vec::new(),
            lines: vec![
                SessionLine {
                    line: "charge_density".to_owned(),
                    ceiling: 724,
                    share: 1000,
                    last: None,
                },
                SessionLine {
                    line: "stability".to_owned(),
                    ceiling: 510,
                    share: rules.assembly_pm(),
                    last: None,
                },
            ],
            points: 0,
            workability: None,
        };
        let stands = now(&registries.0, &holder, &session);
        assert_eq!(stands.len(), 2);
        assert_eq!((stands[0].ceiling, stands[0].floor), (724, 724));
        // A core of 724: the magazine is 634 at its best and 380 as
        // assembled.
        let (_, means) = stands[0].downstream.as_ref().unwrap();
        assert_eq!(
            (means.low.text(), means.high.text()),
            ("28".to_owned(), "30".to_owned())
        );
        assert_eq!((stands[1].ceiling, stands[1].floor), (510, 306));
        assert_eq!(stands[1].downstream, None);
        // A session of a schematic the data no longer has reads nothing.
        let gone = Session {
            schematic: id("veyra:schematic.gone"),
            ..session
        };
        assert!(now(&registries.0, &holder, &gone).is_empty());
    }

    /// A roll is a function of the world's seed and the counter: asked
    /// again it is the same, the next counter is another draw, and over
    /// many counters the results fall as the row says.
    #[test]
    fn the_same_counter_rolls_the_same() {
        const DRAWS: u32 = 20_000;
        let registries = shipped();
        let rules = registries.0.schematics.rules();
        let seed = Seed(42);
        let steady = rules.odds(Hand::Steady, rules.base_row());
        let bold = rules.odds(Hand::Bold, rules.base_row());
        let first: Vec<Outcome> = (0..40).map(|counter| roll(seed, counter, &bold)).collect();
        let again: Vec<Outcome> = (0..40).map(|counter| roll(seed, counter, &bold)).collect();
        assert_eq!(first, again);
        let mut results = first.clone();
        results.sort();
        results.dedup();
        assert_eq!(results, Outcome::ALL, "forty bold rolls see every result");
        let other: Vec<Outcome> = (0..40)
            .map(|counter| roll(Seed(43), counter, &bold))
            .collect();
        assert_ne!(first, other, "another world rolls otherwise");

        for (hand, odds) in [(Hand::Steady, steady), (Hand::Bold, bold)] {
            let mut counts = [0_u32; 5];
            for counter in 0..u64::from(DRAWS) {
                counts[roll(seed, counter, &odds).index()] += 1;
            }
            for outcome in Outcome::ALL {
                let expected = DRAWS * u32::from(odds[outcome.index()]) / 100;
                let got = counts[outcome.index()];
                // Three standard deviations of the likeliest share is 210.
                assert!(
                    got.abs_diff(expected) <= 250,
                    "{hand:?} {outcome:?}: {got} of {DRAWS}, the row says {expected}"
                );
            }
            if hand == Hand::Steady {
                assert_eq!(
                    counts[Outcome::Setback.index()],
                    0,
                    "a steady hand never slips"
                );
            }
        }
        // A row that is all one result is that result.
        assert!(
            (0..200).all(|counter| roll(seed, counter, &[0, 0, 0, 0, 100]) == Outcome::Setback)
        );
        assert!(
            (0..200).all(|counter| roll(seed, counter, &[100, 0, 0, 0, 0]) == Outcome::Exceptional)
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "two lots, each assembled and rolled in turn"
    )]
    fn workable_stock_moves_the_odds_a_row() {
        let registries = shipped();
        let rules = registries.0.schematics.rules();
        let [.., harrow] = worked(&registries);
        let [.., rake] = first_lots(&registries);
        let feed = schematic(&registries, FEED);
        let with_casing = |casing: &Record| {
            Picks::from([
                (
                    "core".to_owned(),
                    part(&registries, CORE_ITEM, 1, Some((1, &[652, 306]))),
                ),
                ("harness".to_owned(), part(&registries, HARNESS, 1, None)),
                ("casing".to_owned(), kind_of(casing)),
                ("seals".to_owned(), part(&registries, SEALS, 1, None)),
            ])
        };
        // Only the casing carries workability, so its figure is the mean.
        assert_eq!(workability(feed, &with_casing(&harrow)), Some(760));
        assert_eq!(workability(feed, &with_casing(&rake)), Some(492));
        assert_eq!(rules.row_up_from(), 700);
        // Which a screen can say of the picks before anything is spent.
        assert!(lifts_odds(rules, workability(feed, &with_casing(&harrow))));
        assert!(!lifts_odds(rules, workability(feed, &with_casing(&rake))));
        assert!(lifts_odds(rules, Some(700)) && !lifts_odds(rules, Some(699)));
        assert!(!lifts_odds(rules, None), "stock that carries none of it");
        let line = SessionLine {
            line: "capacity".to_owned(),
            ceiling: 608,
            share: 600,
            last: None,
        };
        let plain = hand_odds(rules, &line, Hand::Steady, Some(492));
        assert_eq!(plain.odds, rules.odds(Hand::Steady, 1));
        let workable = hand_odds(rules, &line, Hand::Steady, Some(760));
        assert_eq!(workable.odds, [15, 65, 20, 0, 0]);
        assert_eq!(workable.odds, rules.odds(Hand::Steady, 2));
        assert_eq!(
            hand_odds(rules, &line, Hand::Steady, Some(700)).odds,
            workable.odds
        );
        assert_eq!(
            hand_odds(rules, &line, Hand::Steady, Some(699)).odds,
            plain.odds
        );
        let bold = hand_odds(rules, &line, Hand::Bold, Some(760));
        assert_eq!(bold.odds, [38, 27, 20, 10, 5]);
        // 0.16 and 0.04 of 608.
        assert_eq!((bold.best, bold.slip), (97, Some(24)));

        // The session keeps what its stock averaged, and rolls on that row.
        let (mut pack, mut hands, mut store) = bench(
            &registries,
            &[
                harrow.clone(),
                rake.clone(),
                part(&registries, CORE_ITEM, 1, Some((1, &[652, 306]))),
            ],
        );
        // A candidate says its workability, which no line weighs and the
        // order ignores: the casing's two alloys, and nothing on a part.
        let listed = |slot: &str| -> Vec<Option<u16>> {
            candidates(&registries.0, &pack, Some(&store), feed, slot)
                .iter()
                .map(|candidate| candidate.workability)
                .collect()
        };
        let mut casings = listed("casing");
        casings.sort_unstable();
        assert_eq!(casings, [Some(492), Some(760)]);
        assert!(listed("core").iter().all(Option::is_none));
        assert!(listed("seals").iter().all(Option::is_none));
        let mut state = CraftState::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        let picks = with_casing(&harrow);
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &id(FEED),
                &picks,
                MAKER,
                &smith()
            ),
            Ok(5)
        );
        assert_eq!(state.session.as_ref().unwrap().workability, Some(760));
        // On the master's steady row nothing is poor and nothing slips.
        for _ in 0..5 {
            let line = ["capacity", "reload_time"]
                .into_iter()
                .find(|line| {
                    let session = state.session.as_ref().unwrap();
                    !session
                        .lines
                        .iter()
                        .find(|own| own.line == *line)
                        .unwrap()
                        .at_ceiling()
                })
                .unwrap();
            let rolled =
                roll_line(&registries.0, Seed(42), &mut state, line, Hand::Steady).unwrap();
            assert!(rolled.outcome <= Outcome::Fair, "{rolled:?}");
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "each refusal, then the one assembly, on one stock"
    )]
    fn assemble_takes_everything_or_nothing_and_names_the_slot() {
        let registries = shipped();
        let items = &registries.0.items;
        let [salt, brine, _] = first_lots(&registries);
        let mut short_salt = lot(&registries, SALT, "Calder", 12, &[640, 820, 450]);
        let (mut pack, mut hands, mut store) = bench(
            &registries,
            &[salt.clone(), brine.clone(), short_salt.clone()],
        );
        // The Tolley salt lies in three places: five units in the pack,
        // eight in the satchel and the rest in the store.
        let mut some = salt.clone();
        some.count = 5;
        put(&mut pack.cells, 2, &some, items);
        some.count = 8;
        put(&mut pack.bags[0], 3, &some, items);
        let core_id = id(CORE);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&salt)),
            ("brine".to_owned(), kind_of(&brine)),
        ]);
        let before = (pack.clone(), hands.clone(), store.clone());
        let mut state = CraftState::default();
        let slot = |slot: &str, why| CraftRefusal::Slot {
            slot: slot.to_owned(),
            why,
        };
        short_salt.count = 1;

        // Each refusal names its slot and takes nothing.
        let mut refusals: Vec<(Picks, Namespaced, CraftRefusal)> = Vec::new();
        let mut empty = picks.clone();
        empty.remove("brine");
        refusals.push((empty, core_id.clone(), slot("brine", SlotRefusal::Empty)));
        let mut wrong = picks.clone();
        wrong.insert("brine".to_owned(), kind_of(&salt));
        refusals.push((
            wrong,
            core_id.clone(),
            slot("brine", SlotRefusal::WrongKind),
        ));
        let mut short = picks.clone();
        short.insert("salt".to_owned(), short_salt);
        refusals.push((
            short,
            core_id.clone(),
            slot(
                "salt",
                SlotRefusal::Short {
                    have: 12,
                    needs: 20,
                },
            ),
        ));
        refusals.push((
            picks.clone(),
            id("veyra:schematic.gone"),
            CraftRefusal::UnknownSchematic,
        ));
        for (picks, schematic, refusal) in refusals {
            let mut stock = Stock {
                saddle: None,
                pack: &mut pack,
                hands: &mut hands,
                store: Some(&mut store),
            };
            assert_eq!(
                assemble(
                    &registries.0,
                    &mut stock,
                    &mut state,
                    &schematic,
                    &picks,
                    MAKER,
                    &smith()
                ),
                Err(refusal)
            );
            assert_eq!((&pack, &hands, &store), (&before.0, &before.1, &before.2));
            assert_eq!(state, CraftState::default());
        }
        // Away from the store the same picks are short of brine.
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: None,
        };
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &core_id,
                &picks,
                MAKER,
                &smith()
            ),
            Err(slot(
                "salt",
                SlotRefusal::Short {
                    have: 13,
                    needs: 20
                }
            ))
        );
        assert_eq!((&pack, &store), (&before.0, &before.2));

        // Assembled: twenty of salt and thirty of brine, once, from the
        // pack first, then the bag, then the store.
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &core_id,
                &picks,
                MAKER,
                &smith()
            ),
            Ok(3)
        );
        assert_eq!(pack.cells.get(2), None, "the pack's five went first");
        assert_eq!(pack.bags[0].get(3), None, "then the satchel's eight");
        assert_eq!(units(&store.0, &salt, items), 160 - 7);
        assert_eq!(units(&store.0, &brine, items), 160 - 30);
        assert_eq!(hands, before.1);
        let session = state.session.clone().unwrap();
        assert_eq!(
            (session.schematic.clone(), session.by.as_str()),
            (core_id.clone(), MAKER)
        );
        let mut salt_in = kind_of(&salt);
        salt_in.count = 20;
        let mut brine_in = kind_of(&brine);
        brine_in.count = 30;
        assert_eq!(
            session.inputs,
            [("salt".to_owned(), salt_in), ("brine".to_owned(), brine_in)]
        );
        assert_eq!(
            session.lines,
            [
                SessionLine {
                    line: "charge_density".to_owned(),
                    ceiling: 597,
                    share: 600,
                    last: None
                },
                SessionLine {
                    line: "stability".to_owned(),
                    ceiling: 349,
                    share: 600,
                    last: None
                },
            ]
        );
        assert_eq!((session.points, session.workability), (3, None));
        assert_eq!(
            (state.rolls, state.batches),
            (0, 0),
            "assembly rolls nothing"
        );
        assert_eq!(state.check(&registries.0), Ok(()));

        // A second assembly waits for the first to be finished, and takes
        // nothing.
        let after = (pack.clone(), store.clone(), state.clone());
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            assemble(
                &registries.0,
                &mut stock,
                &mut state,
                &core_id,
                &picks,
                MAKER,
                &smith()
            ),
            Err(CraftRefusal::SessionOpen)
        );
        assert_eq!((pack, store, state), after);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "each refusal, a roll, and every result from every share"
    )]
    fn a_roll_spends_a_point_and_never_passes_the_ceiling() {
        let registries = shipped();
        let rules = registries.0.schematics.rules();
        let [salt, brine, _] = first_lots(&registries);
        let (mut pack, mut hands, mut store) = bench(&registries, &[salt.clone(), brine.clone()]);
        let picks = best_stock(
            &registries.0,
            &pack,
            Some(&store),
            schematic(&registries, CORE),
        );
        let seed = Seed(42);
        let mut state = CraftState::default();
        assert_eq!(
            roll_line(
                &registries.0,
                seed,
                &mut state,
                "charge_density",
                Hand::Steady
            ),
            Err(CraftRefusal::NoSession)
        );
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assemble(
            &registries.0,
            &mut stock,
            &mut state,
            &id(CORE),
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        let before = state.clone();
        assert_eq!(
            roll_line(&registries.0, seed, &mut state, "lustre", Hand::Steady),
            Err(CraftRefusal::UnknownLine)
        );
        assert_eq!(state, before, "a refusal spends nothing and rolls nothing");
        // What a screen greys a hand by is the same question.
        let open = state.session.as_ref().unwrap();
        assert_eq!(can_roll(open, "lustre"), Err(CraftRefusal::UnknownLine));
        assert_eq!(can_roll(open, "charge_density"), Ok(()));

        // The first roll is the counter's: what `roll` says of nought.
        let expected = roll(seed, 0, &rules.odds(Hand::Steady, 1));
        let rolled = roll_line(
            &registries.0,
            seed,
            &mut state,
            "charge_density",
            Hand::Steady,
        )
        .unwrap();
        assert_eq!(rolled.outcome, expected);
        assert_eq!(
            (rolled.from, rolled.points, rolled.hand),
            (358, 2, Hand::Steady)
        );
        let share = 600 + rules.lift_pm(expected);
        assert_eq!(rolled.to, figure(597, u16::try_from(share).unwrap()));
        let session = state.session.as_ref().unwrap();
        assert_eq!(session.lines[0].figure(), rolled.to);
        assert_eq!(
            session.lines[0].last,
            Some((expected, i16::try_from(rolled.to - rolled.from).unwrap()))
        );
        assert_eq!(session.lines[1].share, 600, "the other line is as it was");
        assert_eq!((state.rolls, session.points), (1, 2));
        // The same state rolls the same again, and the other hand reads
        // the same draw against its own row.
        let mut replay = before.clone();
        assert_eq!(
            roll_line(
                &registries.0,
                seed,
                &mut replay,
                "charge_density",
                Hand::Steady
            ),
            Ok(rolled)
        );
        assert_eq!(replay, state);

        roll_line(&registries.0, seed, &mut state, "stability", Hand::Bold).unwrap();
        roll_line(&registries.0, seed, &mut state, "stability", Hand::Bold).unwrap();
        assert_eq!(
            (state.rolls, state.session.as_ref().unwrap().points),
            (3, 0)
        );
        let spent = state.clone();
        assert_eq!(
            roll_line(&registries.0, seed, &mut state, "stability", Hand::Steady),
            Err(CraftRefusal::NoPoints)
        );
        assert_eq!(state, spent);
        assert_eq!(
            can_roll(state.session.as_ref().unwrap(), "stability"),
            Err(CraftRefusal::NoPoints)
        );

        // A line at its ceiling takes no more, with points to spare.
        let session = state.session.as_mut().unwrap();
        session.points = 2;
        session.lines[0].share = 1000;
        let full = state.clone();
        assert_eq!(
            roll_line(
                &registries.0,
                seed,
                &mut state,
                "charge_density",
                Hand::Bold
            ),
            Err(CraftRefusal::AtCeiling)
        );
        assert_eq!(state, full);
        let open = state.session.as_ref().unwrap();
        assert_eq!(
            can_roll(open, "charge_density"),
            Err(CraftRefusal::AtCeiling)
        );
        assert_eq!(can_roll(open, "stability"), Ok(()));

        // Whatever is rolled, from wherever a line stands, it ends between
        // nought and its ceiling: the best result at the top and a setback
        // at the bottom are held there.
        for ceiling in [0, 1, 349, 597, 724, 1000] {
            for share in [0, 20, 39, 40, 600, 840, 841, 960, 999, 1000] {
                for outcome in Outcome::ALL {
                    let mut line = SessionLine {
                        line: "charge_density".to_owned(),
                        ceiling,
                        share,
                        last: None,
                    };
                    let (from, to) = lift(rules, &mut line, outcome);
                    assert!(line.share <= 1000 && to <= ceiling, "{line:?}");
                    assert_eq!(to, line.figure());
                    assert_eq!(
                        line.last,
                        Some((
                            outcome,
                            i16::try_from(i32::from(to) - i32::from(from)).unwrap()
                        ))
                    );
                    let wanted = i32::from(share) + i32::from(rules.lift_pm(outcome));
                    assert_eq!(i32::from(line.share), wanted.clamp(0, 1000));
                }
            }
        }
        // And through the request, over a great many counters and both
        // hands.
        for counter in 0..2_000_u64 {
            let mut state = full.clone();
            state.rolls = counter;
            let session = state.session.as_mut().unwrap();
            session.lines[0].share = u16::try_from(900 + counter % 100).unwrap();
            let hand = if counter % 2 == 0 {
                Hand::Bold
            } else {
                Hand::Steady
            };
            let rolled =
                roll_line(&registries.0, seed, &mut state, "charge_density", hand).unwrap();
            assert!(rolled.to <= 597, "{rolled:?}");
            assert_eq!(state.rolls, counter + 1);
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "four finishes as the stock fills, in order"
    )]
    fn finish_goes_to_the_pack_then_a_bag_then_the_store_or_refuses() {
        let registries = shipped();
        let items = &registries.0.items;
        let [salt, brine, _] = first_lots(&registries);
        let (mut pack, mut hands, mut store) = bench(&registries, &[salt.clone(), brine.clone()]);
        let picks = best_stock(
            &registries.0,
            &pack,
            Some(&store),
            schematic(&registries, CORE),
        );
        let core_id = id(CORE);
        let mut state = CraftState::default();
        macro_rules! stock {
            () => {
                Stock {
                    saddle: None,
                    pack: &mut pack,
                    hands: &mut hands,
                    store: Some(&mut store),
                }
            };
        }
        assert_eq!(
            finish(&registries.0, &mut stock!(), &mut state),
            Err(CraftRefusal::NoSession)
        );

        // The first, into the pack's first cell: one of it, batch 1, the
        // assembler's name, the lines as they stand.
        assemble(
            &registries.0,
            &mut stock!(),
            &mut state,
            &core_id,
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        roll_line(
            &registries.0,
            Seed(42),
            &mut state,
            "charge_density",
            Hand::Steady,
        )
        .unwrap();
        let stood = state.session.as_ref().unwrap().figures();
        let inputs = state.session.as_ref().unwrap().inputs.clone();
        let made = finish(&registries.0, &mut stock!(), &mut state).unwrap();
        assert_eq!(made.item, id(CORE_ITEM));
        assert_eq!(
            (made.count, made.batch, made.by.as_deref()),
            (1, 1, Some(MAKER))
        );
        assert_eq!(made.lines, stood);
        assert_eq!(made.lines["stability"], 209);
        assert_eq!(items.check_record(&made), Ok(()));
        assert_eq!(
            items::read(pack.cells.get(0).unwrap(), items).unwrap(),
            made
        );
        assert_eq!((state.batches, state.rolls), (1, 1));
        assert_eq!(state.session, None, "the bench is clear");
        assert_eq!(
            state.made[&1],
            Made {
                item: id(CORE_ITEM),
                by: MAKER.to_owned(),
                from: inputs,
                schematic: Some(core_id.clone()),
                recorded: None,
                blueprint: None,
            }
        );
        assert_eq!(state.check(&registries.0), Ok(()));

        // With the pack full the next goes to the satchel.
        let all = pack.cells.len();
        fill(&mut pack.cells, all, items);
        assemble(
            &registries.0,
            &mut stock!(),
            &mut state,
            &core_id,
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        let second = finish(&registries.0, &mut stock!(), &mut state).unwrap();
        assert_eq!(second.batch, 2);
        assert_eq!(
            items::read(pack.bags[0].get(1).unwrap(), items).unwrap(),
            second
        );

        // With the satchel full too, to the store.
        let open = usize::from(pack.open_cells(0, items));
        fill(&mut pack.bags[0], open, items);
        assemble(
            &registries.0,
            &mut stock!(),
            &mut state,
            &core_id,
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        let third = finish(&registries.0, &mut stock!(), &mut state).unwrap();
        assert_eq!(third.batch, 3);
        assert_eq!(units(&store.0, &third, items), 1);
        assert_eq!(
            units(&pack.cells, &third, items) + units(&pack.bags[0], &third, items),
            0
        );

        // With no room anywhere it is refused: nothing is made, no batch
        // is spent and the session stays as it was, materials and all.
        let all = store.0.len();
        fill(&mut store.0, all, items);
        // The store is full but for the lots, so the fourth is assembled.
        assemble(
            &registries.0,
            &mut stock!(),
            &mut state,
            &core_id,
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        roll_line(
            &registries.0,
            Seed(42),
            &mut state,
            "charge_density",
            Hand::Bold,
        )
        .unwrap();
        let before = (pack.clone(), store.clone(), state.clone());
        assert_eq!(
            finish(&registries.0, &mut stock!(), &mut state),
            Err(CraftRefusal::NoRoom)
        );
        assert_eq!((&pack, &store, &state), (&before.0, &before.1, &before.2));
        assert_eq!(state.batches, 3);
        assert_eq!(
            state.session.as_ref().unwrap().inputs.len(),
            2,
            "nothing is lost"
        );
        // Room made, the same session finishes.
        pack.cells.set(4, None);
        pack.cells.clear_changed();
        let fourth = finish(&registries.0, &mut stock!(), &mut state).unwrap();
        assert_eq!(fourth.batch, 4);
        assert_eq!(
            items::read(pack.cells.get(4).unwrap(), items).unwrap(),
            fourth
        );
        assert_eq!(state.made.keys().copied().collect::<Vec<_>>(), [1, 2, 3, 4]);
        // Four cores took four times the stock.
        assert_eq!(units(&store.0, &salt, items), 160 - 80);
        assert_eq!(units(&store.0, &brine, items), 160 - 120);
    }

    /// A state as a save writes it reads back as it was, an open session
    /// and its committed materials with it, and is held to the data: a
    /// session of a schematic the data no longer has, lines that are not
    /// its product's and a figure out of range are each refused, and what
    /// a later edit to the data can put out of line is repaired.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one saved state, each way it can be out of line, refused or mended"
    )]
    fn a_saved_state_is_held_to_the_data() {
        let registries = shipped();
        let [salt, brine, _] = first_lots(&registries);
        let (mut pack, mut hands, mut store) = bench(&registries, &[salt, brine]);
        let picks = best_stock(
            &registries.0,
            &pack,
            Some(&store),
            schematic(&registries, CORE),
        );
        let mut state = CraftState::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assemble(
            &registries.0,
            &mut stock,
            &mut state,
            &id(CORE),
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        finish(&registries.0, &mut stock, &mut state).unwrap();
        assemble(
            &registries.0,
            &mut stock,
            &mut state,
            &id(CORE),
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        roll_line(
            &registries.0,
            Seed(42),
            &mut state,
            "charge_density",
            Hand::Bold,
        )
        .unwrap();
        let text = ron::ser::to_string_pretty(&state, ron::ser::PrettyConfig::default()).unwrap();
        let read: CraftState = ron::from_str(&text).unwrap();
        assert_eq!(read, state);
        assert_eq!(read.check(&registries.0), Ok(()));
        assert_eq!((read.rolls, read.batches), (1, 1));
        let session = read.session.as_ref().unwrap();
        assert_eq!((session.points, session.inputs.len()), (2, 2));
        assert!(session.lines[0].last.is_some());
        // An older file has none of it.
        assert_eq!(
            ron::from_str::<CraftState>("()").unwrap(),
            CraftState::default()
        );
        assert_eq!(CraftState::default().check(&registries.0), Ok(()));

        let said = |change: &dyn Fn(&mut CraftState)| {
            let mut changed = state.clone();
            change(&mut changed);
            changed.check(&registries.0).unwrap_err()
        };
        let gone = said(&|state| {
            state.session.as_mut().unwrap().schematic = id("veyra:schematic.gone");
        });
        assert!(gone.contains("which is no schematic"), "{gone}");
        let lines = said(&|state| {
            state.session.as_mut().unwrap().lines.pop();
        });
        assert!(lines.contains("its product has"), "{lines}");
        let over = said(&|state| state.session.as_mut().unwrap().lines[0].share = 1001);
        assert!(over.contains("over 1000"), "{over}");

        // What an edit to the data can put out of line is put back and
        // said, not refused: the points a retuned dial no longer gives, and
        // a record of what went in that no longer reads. A repaired state
        // passes, and one with nothing wrong is left as it was.
        let mut sound = state.clone();
        assert_eq!(sound.repair(&registries.0), Vec::<String>::new());
        assert_eq!(sound, state);
        let mended = |change: &dyn Fn(&mut CraftState)| {
            let mut changed = state.clone();
            change(&mut changed);
            assert_eq!(changed.check(&registries.0), Ok(()), "nothing is refused");
            let said = changed.repair(&registries.0);
            assert_eq!(changed.check(&registries.0), Ok(()));
            assert_eq!(changed.repair(&registries.0), Vec::<String>::new());
            (changed, said)
        };
        let (points, said) = mended(&|state| state.session.as_mut().unwrap().points = 11);
        // The dial's three, the four tuning boxes' one each, the
        // Fabricator's two Batches and its master: the most any sheet opens
        // a session of this complexity with.
        assert_eq!(points.session.as_ref().unwrap().points, 10, "the most");
        assert_eq!(said.len(), 1);
        assert!(
            said[0].contains("had 11 points") && said[0].contains("it has 10"),
            "{said:?}"
        );
        // Everything else of the session stands.
        let mut same = points.clone();
        same.session.as_mut().unwrap().points = 2;
        assert_eq!(same, state);

        let (slot, said) = mended(&|state| {
            state.session.as_mut().unwrap().inputs[0].0 = "muzzle".to_owned();
        });
        let left: Vec<&str> = slot
            .session
            .as_ref()
            .unwrap()
            .inputs
            .iter()
            .map(|(slot, _)| slot.as_str())
            .collect();
        assert_eq!(left, ["brine"]);
        assert!(said[0].contains("slot `muzzle`"), "{said:?}");
        let (input, said) =
            mended(&|state| state.session.as_mut().unwrap().inputs[0].1.count = 201);
        assert_eq!(input.session.as_ref().unwrap().inputs.len(), 1);
        assert!(said[0].contains("slot `salt`"), "{said:?}");
        // The lines, the points and the counters are none of it.
        assert_eq!(
            input.session.as_ref().unwrap().lines,
            state.session.as_ref().unwrap().lines
        );
        assert_eq!((input.rolls, input.batches), (state.rolls, state.batches));

        let (made, said) =
            mended(&|state| state.made.get_mut(&1).unwrap().item = id("veyra:component.gone"));
        assert!(made.made.is_empty(), "the entry is dropped");
        assert!(
            said[0].contains("batch 1") && said[0].contains("is no item"),
            "{said:?}"
        );
        assert_eq!(made.session, state.session);
        let (from, said) = mended(&|state| state.made.get_mut(&1).unwrap().from[1].1.count = 0);
        assert!(from.made.is_empty());
        assert!(
            said[0].contains("batch 1") && said[0].contains("slot `brine`"),
            "{said:?}"
        );
        // A session of a schematic that is gone is not the repair's to
        // mend: it is left for the check to refuse.
        let mut gone = state.clone();
        gone.session.as_mut().unwrap().schematic = id("veyra:schematic.gone");
        gone.session.as_mut().unwrap().points = 9;
        assert_eq!(gone.repair(&registries.0), Vec::<String>::new());
        assert!(gone.check(&registries.0).is_err());
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "each purchase and each refusal on one stock"
    )]
    fn buying_pays_from_the_purse_or_says_why() {
        let registries = shipped();
        let items = &registries.0.items;
        let (mut pack, mut hands, mut store) = bench(&registries, &[]);
        let mut purse = Purse(2500);
        let (harness, seals) = (id(HARNESS), id(SEALS));
        macro_rules! stock {
            () => {
                Stock {
                    saddle: None,
                    pack: &mut pack,
                    hands: &mut hands,
                    store: Some(&mut store),
                }
            };
        }
        // One harness: its price leaves the purse and one is in the pack,
        // at store grade.
        assert_eq!(
            can_buy(&registries.0, &pack, Some(&store), &purse, &harness, 1),
            Ok(80)
        );
        assert_eq!(
            buy(&registries.0, &mut stock!(), &mut purse, &harness, 1),
            Ok(80)
        );
        assert_eq!(purse, Purse(2420));
        let got = items::read(pack.cells.get(0).unwrap(), items).unwrap();
        assert_eq!(got, part(&registries, HARNESS, 1, None));
        assert_eq!(got.lines["conductance"], STORE_GRADE);
        // Three seal sets are three times the price, on one stack.
        assert_eq!(
            buy(&registries.0, &mut stock!(), &mut purse, &seals, 3),
            Ok(60)
        );
        assert_eq!(purse, Purse(2360));
        assert_eq!(
            items::read(pack.cells.get(1).unwrap(), items).unwrap(),
            part(&registries, SEALS, 3, None)
        );
        // The next harness joins the first.
        buy(&registries.0, &mut stock!(), &mut purse, &harness, 1).unwrap();
        assert_eq!(pack.cells.get(0).unwrap().count, 2);
        assert_eq!(
            units(&store.0, &got, items),
            3,
            "the store's are as they were"
        );

        // Each refusal says why and changes nothing.
        let before = (pack.clone(), store.clone(), purse);
        let refusals = [
            (id(CORE_ITEM), 1, CraftRefusal::NotForSale),
            (id(FEED_ITEM), 1, CraftRefusal::NotForSale),
            (id("veyra:component.gone"), 1, CraftRefusal::NotForSale),
            (harness.clone(), 0, CraftRefusal::NotForSale),
            (
                harness.clone(),
                29,
                CraftRefusal::Short {
                    needs: 2320,
                    have: 2280,
                },
            ),
        ];
        for (item, count, refusal) in refusals {
            assert_eq!(
                can_buy(&registries.0, &pack, Some(&store), &purse, &item, count),
                Err(refusal.clone())
            );
            assert_eq!(
                buy(&registries.0, &mut stock!(), &mut purse, &item, count),
                Err(refusal)
            );
            assert_eq!((&pack, &store, &purse), (&before.0, &before.1, &before.2));
        }
        let mut poor = Purse(40);
        assert_eq!(
            buy(&registries.0, &mut stock!(), &mut poor, &harness, 1),
            Err(CraftRefusal::Short {
                needs: 80,
                have: 40
            })
        );
        assert_eq!(poor, Purse(40));

        // With every cell taken there is nowhere to put one, and nothing
        // is paid; a seal set still joins the stack that is there.
        let all = pack.cells.len();
        fill(&mut pack.cells, all, items);
        let open = usize::from(pack.open_cells(0, items));
        fill(&mut pack.bags[0], open, items);
        let all = store.0.len();
        fill(&mut store.0, all, items);
        let cell_core = part(&registries, CORE_ITEM, 20, Some((1, &[500, 500])));
        put(&mut pack.cells, 0, &cell_core, items);
        let harnesses = items::write(&part(&registries, HARNESS, 20, None), items).unwrap();
        let held: Vec<usize> = (0..store.0.len())
            .filter(|cell| {
                store
                    .0
                    .get(*cell)
                    .is_some_and(|stack| stack.same_kind(&harnesses))
            })
            .collect();
        for cell in held {
            store.0.set(cell, Some(harnesses.clone()));
        }
        store.0.clear_changed();
        let full = (pack.clone(), store.clone(), purse);
        assert_eq!(
            can_buy(&registries.0, &pack, Some(&store), &purse, &harness, 1),
            Err(CraftRefusal::NoRoom)
        );
        assert_eq!(
            buy(&registries.0, &mut stock!(), &mut purse, &harness, 1),
            Err(CraftRefusal::NoRoom)
        );
        assert_eq!((&pack, &store, &purse), (&full.0, &full.1, &full.2));
        assert_eq!(
            buy(&registries.0, &mut stock!(), &mut purse, &seals, 1),
            Ok(20)
        );
        assert_eq!(pack.cells.get(1).unwrap().count, 4);
    }

    pub const CARBINE: &str = "veyra:schematic.patrol_carbine";

    pub const CARBINE_ITEM: &str = "veyra:weapon.patrol_carbine";

    pub const FRAMES_1: &str = "weaponsmith.frames.1";

    /// A sheet that has learned exactly `boxes`.
    pub fn knowing(boxes: &[&str]) -> CharacterSheet {
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    /// The harness is half of a carbine's Action line, so a made one is how
    /// a smith beats a bought one (which cannot pass 500 on its half): 0.8 of
    /// the conductor's Conductivity and 0.2 of its Purity, and then half of
    /// that with half of the frame's Stability.
    #[test]
    fn a_made_harness_lifts_a_carbines_action() {
        let registries = shipped();
        let conductor = lot(
            &registries,
            "strata:red_conductor",
            "Vane",
            100,
            // Purity, Conductivity, HeatTolerance, Stability, Density,
            // Workability, in the class's order.
            &[800, 900, 600, 500, 600, 700],
        );
        let maker = schematic(&registries, "veyra:schematic.harness");
        let picks = Picks::from([("conductor".to_owned(), kind_of(&conductor))]);
        // 0.8 * 900 + 0.2 * 800.
        assert_eq!(ceiling(maker, 0, &picks), Some(880));

        let alloy = lot(
            &registries,
            ALLOY,
            "Harrow",
            200,
            &[610, 540, 560, 410, 520, 800, 320, 760],
        );
        let carbine = schematic(&registries, CARBINE);
        let picks = Picks::from([
            ("frame".to_owned(), kind_of(&alloy)),
            (
                "action".to_owned(),
                part(&registries, HARNESS, 1, Some((1, &[880]))),
            ),
            ("seals".to_owned(), part(&registries, SEALS, 1, None)),
        ]);
        // 0.5 * 880 + 0.5 * 800.
        assert_eq!(ceiling(carbine, 0, &picks), Some(840));
    }

    /// The first day's light alloy lot cannot reach a 600 moddability.
    #[test]
    fn the_first_days_lot_cannot_open_the_muzzle() {
        let registries = shipped();
        let [.., alloy] = first_lots(&registries);
        let carbine = schematic(&registries, CARBINE);
        let picks = Picks::from([
            ("frame".to_owned(), kind_of(&alloy)),
            ("action".to_owned(), part(&registries, HARNESS, 1, None)),
            ("seals".to_owned(), part(&registries, SEALS, 1, None)),
        ]);
        // 0.6 * 231 + 0.4 * 298.
        assert_eq!(ceiling(carbine, 2, &picks), Some(258));
    }

    /// A sheet that has learned every box of the tree `tree`.
    pub fn whole_tree(registries: &Registries, tree: &str) -> CharacterSheet {
        CharacterSheet {
            learned: registries
                .0
                .progression
                .tree(tree)
                .unwrap()
                .boxes()
                .map(|b| b.id.clone())
                .collect(),
            ..CharacterSheet::default()
        }
    }

    /// The household's smith as the authored sheets have him.
    pub fn tovan(registries: &Registries) -> CharacterSheet {
        let member = registries
            .0
            .sheets
            .household
            .as_ref()
            .unwrap()
            .recruits
            .iter()
            .find(|member| member.name == "Tovan Reyes")
            .unwrap();
        CharacterSheet {
            learned: member.sheet.learned.iter().cloned().collect(),
            xp: member.sheet.xp.clone(),
        }
    }

    /// The Fabricator's boxes on the bench, the fabricator and repair: the
    /// novice alone, the household's smith and the whole tree each read
    /// their figures through the one fold, and a sheet with none reads the
    /// dials as they are.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "a table of eight figures for four sheets"
    )]
    fn the_fabricator_boxes_change_the_bench_the_run_and_the_repair() {
        use crate::factory::{Pace, pace_of};
        use crate::wear::{Bearer, cost_for, field_amount_for};
        let registries = shipped();
        let graph = &registries.0.progression;
        let rules = registries.0.schematics.rules();
        let factory = registries.0.schematics.factory().unwrap();
        let wear = registries.0.items.wear();
        let mut weapon = part(&registries, HARNESS, 1, None);
        weapon.lines.clear();
        let novice = knowing(&["fabricator.novice"]);
        let smith_tovan = tovan(&registries);
        let full = whole_tree(&registries, "fabricator");
        // (share, points of complexity 1, runs of complexity 1, pace, field
        // amount of a kit at 500 and at 0, whether a bench repair takes a
        // kit, the cost of a shot)
        let read = |sheet: &CharacterSheet| {
            let bearer = Some(Bearer { graph, sheet });
            (
                assembly_pm(rules, graph, sheet),
                points(rules, graph, sheet, 1),
                crate::blueprint::runs_for(factory, graph, sheet, 1),
                pace_of(factory, graph, sheet),
                (
                    field_amount_for(wear, 500, bearer),
                    field_amount_for(wear, 0, bearer),
                ),
                crate::progression::wear_field(graph, sheet, "bench_kit", 1.0) >= 0.5,
                cost_for(wear, wear.shot, &weapon, bearer),
            )
        };
        // No box at all: the dials as they are, and no pace to keep.
        assert_eq!(
            read(&CharacterSheet::default()),
            (600, 3, 100, None, (700, 400), true, 2000)
        );
        assert_eq!(
            factory.ticks_per_point(),
            240,
            "twelve seconds at twenty ticks a second"
        );
        assert_eq!(factory.unattended_pm(), 500);
        // The novice: two points of assembly, nothing else.
        assert_eq!(read(&novice), (620, 3, 100, None, (700, 400), true, 2000));
        // Tovan: the novice, Fixtures 1 to 3 and his one tuning box. A unit
        // in 10.8 s, a quarter more runs and 0.65 unattended.
        assert_eq!(
            read(&smith_tovan),
            (
                620,
                4,
                125,
                Some(Pace {
                    ticks_per_point: 216,
                    unattended_pm: 650
                }),
                (700, 400),
                true,
                2000
            )
        );
        // The whole tree: 0.69 assembly, six points, a unit in 6.9 s, a kit
        // that mends a whole item from store grade, no kit at a bench and a
        // fifth less wear.
        assert_eq!(
            read(&full),
            (
                690,
                6,
                125,
                Some(Pace {
                    ticks_per_point: 138,
                    unattended_pm: 650
                }),
                (1000, 624),
                false,
                1600
            )
        );
        // And he knows Materials 1, so the drive coil and the survey tool are
        // his to make from the first day.
        for name in ["veyra:schematic.drive_coil", "veyra:schematic.survey_tool"] {
            assert_eq!(taught(schematic(&registries, name), &smith_tovan), Ok(()));
        }
        // A full sheet leaves the lines something to roll for.
        assert!(assembly_pm(rules, graph, &full) < 900);
        // The runs of the other complexities, rounded.
        assert_eq!(
            [1, 2, 3].map(|c| crate::blueprint::runs_for(factory, graph, &full, c)),
            [125, 63, 31]
        );
        // The rules of the schematics, the boxes teach: the Materials boxes
        // name the schematics they teach.
        for (needs, makes) in [
            ("fabricator.materials.1", "veyra:schematic.drive_coil"),
            ("fabricator.materials.1", "veyra:schematic.survey_tool"),
            ("fabricator.materials.2", "veyra:schematic.hull_plate"),
            ("fabricator.materials.3", "veyra:schematic.courier_bike"),
            ("weaponsmith.fittings.3", "veyra:schematic.vigil_reflex"),
            (
                "weaponsmith.fittings.3",
                "veyra:schematic.vigil_angled_grip",
            ),
            (
                "weaponsmith.fittings.4",
                "veyra:schematic.vigil_compensator",
            ),
            ("weaponsmith.frames.1", "veyra:schematic.service_pistol"),
        ] {
            let made = schematic(&registries, makes);
            assert_eq!(made.def.profession.as_deref(), Some(needs), "{makes}");
            assert!(taught(made, &CharacterSheet::default()).is_err());
            assert_eq!(taught(made, &knowing(&[needs])), Ok(()));
        }
    }
}
