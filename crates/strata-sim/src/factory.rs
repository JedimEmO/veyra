//! The factory: a fabricator running a production blueprint as a standing
//! job (ticket 0007, step 5; `docs/design/crafting.md`, 5.4; the
//! progression design's "Run the workshop").
//!
//! A run is started with a blueprint from the pack, its bags or the store,
//! a batch size and, for each of the blueprint's slots, the kinds of stack
//! that feed it. Any stock that meets a slot's requirements feeds it
//! ([`meets`]), a slot may be fed from several kinds, and better stock
//! gains nothing. Every input for the whole batch and the blueprint's item
//! are committed at the start or nothing is ([`start`]). The run then makes
//! one unit a stretch of played time, with no roll: every unit has the
//! blueprint's lines exactly, the run's one batch number and the recorder
//! as its maker, and goes to the household's store. When the store has no
//! room the run pauses and says so, and goes on when there is room. Stop
//! leaves what is made, gives the unspent inputs and the blueprint back to
//! the store, and is refused when they will not fit ([`stop`]).
//!
//! Time is counted in ticks and per-mille, so a batch takes the same
//! ticks on every machine ([`advance`]): each tick adds the run's rate to a
//! clock, a thousand with a worker on it and the dial's share with nobody,
//! and a unit is made when the clock reaches a thousand times the unit's
//! ticks. Those ticks are the nominal ones, `SIM_TICK_HZ` a played second,
//! which is what the dials are held to: a sim stepped at another rate (the
//! game's combat play steps at sixty) is counted down to them
//! ([`nominal_ticks`]), so a unit takes its played seconds whatever the
//! rate. The tick only runs while the game does, so a run makes nothing
//! while the game is closed. What is over when a unit is made is carried
//! into the next, so a batch takes the ticks [`seconds`] says whatever the
//! rate. The clock within a unit is not saved: a restart begins the unit
//! under way again. A run standing for want of room is the one case that
//! is kept, since its unit is made and only waiting: a pause read back from
//! a save has its unit still.
//!
//! The worker is a benched member of the household, by name ([`Benched`],
//! which the game writes). The household's side of it, that the character
//! cannot be switched to while they work, is the game's; here the name only
//! sets the rate.
//!
//! Everything a screen shows is worked out here and asked, never written
//! again: [`feeds`] for the stock of a slot with what meets it first and
//! every refusal with its number, [`cheapest`] to fill every slot in a
//! press, [`limit`] for the largest batch and what holds it, [`check_start`]
//! for why Run would be refused, [`seconds`] and [`left`] for the time, and
//! [`can_stop`] for Stop.
//!
//! Start, Assign and Stop are requests consumed in the tick
//! ([`FactoryInbox`], [`consume`]), allowed with a fabricator in reach and
//! answered with what happened or why not ([`FactoryAnswer`]); what the run
//! does on its own is told as it happens ([`FactoryNotice`]). One job a
//! household. Local-only, like the stock: nothing here is replicated.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_core::SIM_TICK_HZ;
use strata_registry::item::{ItemCatalogue, ItemKind, QUALITY_MAX, Record};
use strata_registry::progression::SkillGraph;
use strata_registry::schematic::FactoryRules;
use strata_registry::{GameRegistries, Namespaced};

use crate::blueprint::{Blueprint, Blueprints, SlotNeed};
use crate::crafting::{self, CraftState, Made, Scratch, Spoken, kind_of};
use crate::fitting::WorkbenchAccess;
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};
use crate::progression::{CharacterSheet, Reach, Touch, factory_field, touches};
use crate::{LocalPlayer, Registries, SimClock, SimSet, SimTick};

/// A whole, in the per-mille a run's rate is counted in.
const PER_MILLE: u32 = QUALITY_MAX as u32;

/// The household's fabricator: the run it is on, if any. The game saves it
/// whole on every change, which is once a unit and never once a tick.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FactoryState {
    /// The run under way.
    #[serde(default)]
    pub job: Option<Job>,
}

/// A run under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// The blueprint, by its number.
    pub blueprint: u32,
    /// The blueprint's item, which the fabricator holds while it runs.
    pub item: Record,
    /// The batch number every unit of the run carries.
    pub batch: u32,
    /// How many the run is to make.
    pub size: u32,
    /// How many it has made.
    pub made: u32,
    /// What was committed and is not yet spent, in the order it is used.
    pub inputs: Vec<JobInput>,
    /// The benched member working it, by name; nobody, and it runs at the
    /// unattended rate.
    pub worker: Option<String>,
    /// Why it is standing, when it is.
    pub paused: Option<Pause>,
    /// The pace of the member who started it, kept so a switch of member
    /// does not change a run under way. None reads the dials as they are,
    /// which is also what a run saved before the boxes had effects reads.
    #[serde(default)]
    pub pace: Option<Pace>,
}

/// How fast a run goes, as its starter's sheet made the dials: kept with
/// the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pace {
    /// Sim ticks a unit a point of complexity with a worker on it.
    pub ticks_per_point: u32,
    /// The share of the worked rate with nobody on it, per-mille.
    pub unattended_pm: u16,
}

/// The pace `sheet` makes of the dials, or `None` when none of its boxes
/// moves either figure, so a sheet with none reads the dials untouched. The
/// one place the numbers are worked out, so a screen shows what a run will
/// take.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "seconds and a share, small, rounded and held to one and up before the cast"
)]
pub fn pace_of(rules: &FactoryRules, graph: &SkillGraph, sheet: &CharacterSheet) -> Option<Pace> {
    let seconds = touches(graph, sheet, Touch::Factory, "seconds");
    let unattended = touches(graph, sheet, Touch::Factory, "unattended");
    if !seconds && !unattended {
        return None;
    }
    let ticks_per_point = if seconds {
        let secs = rules.ticks_per_point() as f32 / SIM_TICK_HZ as f32;
        let secs = factory_field(graph, sheet, "seconds", secs);
        (secs * SIM_TICK_HZ as f32).round().max(1.0) as u32
    } else {
        rules.ticks_per_point()
    };
    let unattended_pm = if unattended {
        let share = f32::from(rules.unattended_pm()) / 1000.0;
        let share = factory_field(graph, sheet, "unattended", share);
        (share * 1000.0).round().clamp(1.0, f32::from(QUALITY_MAX)) as u16
    } else {
        rules.unattended_pm()
    };
    Some(Pace {
        ticks_per_point,
        unattended_pm,
    })
}

/// `rules` at `pace`: the dials themselves for none.
#[must_use]
pub fn paced(rules: &FactoryRules, pace: Option<Pace>) -> Cow<'_, FactoryRules> {
    pace.map_or(Cow::Borrowed(rules), |pace| {
        Cow::Owned(rules.paced(pace.ticks_per_point, pace.unattended_pm))
    })
}

/// One kind of stack committed to a slot of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInput {
    /// The slot it feeds, by its id.
    pub slot: String,
    /// The kind: its record with a count of one.
    pub kind: Record,
    /// How much of it is left in the hopper.
    pub left: u32,
}

/// Why a run is standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pause {
    /// The household's store has no room for the next unit, or for the
    /// blueprint when the last is made.
    NoRoom,
}

/// How far the run is toward its next unit, in thousandths of a tick. Not
/// saved: a restart begins the unit under way again, and a run that was
/// standing with a unit made has it still ([`advance`]).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FactoryClock(pub u32);

/// Who is on the household's bench and not out, by name: who may be put on
/// a run. The game writes it; a server and a harness have nobody.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct Benched(pub Vec<String>);

/// What feeds each slot of a run, by the slot's id: the kinds of stack, in
/// the order they are taken.
pub type Feeds = BTreeMap<String, Vec<Record>>;

/// A kind of stack the stock holds for a slot of a blueprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    /// The kind, as a feed names it.
    pub kind: Record,
    /// How much of it the character carries, in the pack and its bags.
    pub carried: u32,
    /// How much the household's store holds.
    pub stored: u32,
    /// Each figure the slot asks of, as this kind's card writes it, in the
    /// order of the slot's requirements.
    pub figures: Vec<(String, u16)>,
    /// How far it clears the requirements: the sum of what each figure has
    /// over its floor or under its ceiling. What the kinds that meet are
    /// ordered by, the least first, so the best is kept for hand work.
    pub margin: u32,
    /// Whether it meets the slot's requirements, or the first it fails,
    /// with its number.
    pub verdict: Result<(), FeedRefusal>,
}

impl Feed {
    /// How much of the kind the stock holds in all.
    #[must_use]
    pub const fn held(&self) -> u32 {
        self.carried.saturating_add(self.stored)
    }
}

/// Why a kind of stack, or what is picked for a slot, will not feed it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeedRefusal {
    /// Nothing is picked for the slot.
    #[error("nothing feeds it")]
    Empty,
    /// It is not something the slot takes.
    #[error("it does not take that")]
    WrongKind,
    /// A figure is under the least the blueprint asks.
    #[error("{stat} {has}, needs {needs}")]
    Under {
        /// The figure, by its key.
        stat: String,
        /// What the stock has.
        has: u16,
        /// The least the slot takes.
        needs: u16,
    },
    /// A figure of which less is better is over the most the blueprint
    /// allows.
    #[error("{stat} {has}, at most {most}")]
    Over {
        /// The figure, by its key.
        stat: String,
        /// What the stock has.
        has: u16,
        /// The most the slot takes.
        most: u16,
    },
    /// What is picked holds too little for the batch.
    #[error("{have} in stock, the batch takes {needs}")]
    Short {
        /// How much the picked kinds hold, less what earlier slots took.
        have: u32,
        /// How much the batch takes.
        needs: u32,
    },
}

/// The largest batch a blueprint can be run for, and what holds it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limit {
    /// The largest batch.
    pub most: u32,
    /// What stops it being larger.
    pub held_by: Held,
}

/// What holds a batch's size down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The blueprint has no more runs.
    Runs {
        /// The runs it has left.
        left: u32,
    },
    /// The stock that meets a slot runs out first.
    Stock {
        /// The slot, by its id.
        slot: String,
        /// How many units the stock feeds.
        feeds: u32,
    },
}

/// Whether `kind` meets every requirement of `need`: each floor at or under
/// its figure, each ceiling at or over it.
///
/// # Errors
/// The first requirement it fails, with the figure it has and the one asked
/// for; or that it has no such figure at all.
pub fn meets(need: &SlotNeed, kind: &Record) -> Result<(), FeedRefusal> {
    for asked in &need.needs {
        let has = *kind.lines.get(&asked.stat).ok_or(FeedRefusal::WrongKind)?;
        if let Some(needs) = asked.at_least
            && has < needs
        {
            return Err(FeedRefusal::Under {
                stat: asked.stat.clone(),
                has,
                needs,
            });
        }
        if let Some(most) = asked.at_most
            && has > most
        {
            return Err(FeedRefusal::Over {
                stat: asked.stat.clone(),
                has,
                most,
            });
        }
    }
    Ok(())
}

/// How far `kind` clears `need`'s requirements, summed. Nought for a figure
/// that fails one.
fn margin(need: &SlotNeed, kind: &Record) -> u32 {
    need.needs
        .iter()
        .map(|asked| {
            let has = kind.lines.get(&asked.stat).copied().unwrap_or(0);
            let over = asked.at_least.map_or(0, |least| has.saturating_sub(least));
            let under = asked.at_most.map_or(0, |most| most.saturating_sub(has));
            u32::from(over) + u32::from(under)
        })
        .sum()
}

/// The kinds of stack that fit the slot `need` in `stock`, each once with
/// its counts: those that meet it first, the one that clears it by least
/// first, then store grade before a batch and a batch before a later one,
/// then by item and by deposit; those that fail it after, in that same
/// order, each with why.
fn feeds_of(items: &ItemCatalogue, stock: &[(Record, bool)], need: &SlotNeed) -> Vec<Feed> {
    let mut found: Vec<Feed> = Vec::new();
    for (record, in_store) in stock {
        if !crafting::fits(items, &need.slot, record) {
            continue;
        }
        let kind = kind_of(record);
        let at = found
            .iter()
            .position(|feed| feed.kind == kind)
            .unwrap_or_else(|| {
                let figures = need
                    .needs
                    .iter()
                    .map(|asked| {
                        let has = kind.lines.get(&asked.stat).copied().unwrap_or(0);
                        (asked.stat.clone(), has)
                    })
                    .collect();
                found.push(Feed {
                    carried: 0,
                    stored: 0,
                    figures,
                    margin: margin(need, &kind),
                    verdict: meets(need, &kind),
                    kind,
                });
                found.len() - 1
            });
        let count = if *in_store {
            &mut found[at].stored
        } else {
            &mut found[at].carried
        };
        *count = count.saturating_add(record.count);
    }
    // Stable, so two that tie on everything keep the order they were met
    // in: the pack's before the store's.
    found.sort_by(|a, b| {
        let deposit = |feed: &Feed| feed.kind.origin.as_ref().map(|o| o.deposit);
        let cleared = |feed: &Feed| if feed.verdict.is_ok() { feed.margin } else { 0 };
        a.verdict
            .is_err()
            .cmp(&b.verdict.is_err())
            .then_with(|| cleared(a).cmp(&cleared(b)))
            .then_with(|| a.kind.batch.cmp(&b.kind.batch))
            .then_with(|| a.kind.item.cmp(&b.kind.item))
            .then_with(|| deposit(a).cmp(&deposit(b)))
    });
    found
}

/// The stock for slot `slot` of `print` in the pack, its bags and, when
/// given, the household's store: every kind that fits it, the ones that
/// meet the blueprint first and the cheapest of those first, every one that
/// does not after them with the figure it fails on and what is asked.
#[must_use]
pub fn feeds(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    print: &Blueprint,
    slot: &str,
) -> Vec<Feed> {
    let items = &registries.items;
    let Some(need) = print.slots.iter().find(|need| need.slot.id == slot) else {
        return Vec::new();
    };
    feeds_of(items, &crafting::reachable(items, pack, store), need)
}

/// A batch's inputs being counted out of the stock, slot by slot, so that
/// one stack is never promised to two slots.
#[derive(Default)]
struct Allot {
    spoken: Spoken,
    inputs: Vec<JobInput>,
}

impl Allot {
    /// Take `batch` units' worth for `need` from `kinds`, each with how
    /// much the stock holds of it, in their order. Short, how much there
    /// was and how much was wanted; what there was is taken all the same.
    fn slot(
        &mut self,
        need: &SlotNeed,
        kinds: &[(Record, u32)],
        batch: u32,
    ) -> Result<(), (u32, u32)> {
        let needs = need.slot.amount().saturating_mul(batch);
        let mut short = needs;
        for (kind, held) in kinds {
            if short == 0 {
                break;
            }
            let taken = held.saturating_sub(self.spoken.of(kind)).min(short);
            if taken == 0 {
                continue;
            }
            self.spoken.add(kind, taken);
            short -= taken;
            self.inputs.push(JobInput {
                slot: need.slot.id.clone(),
                kind: kind.clone(),
                left: taken,
            });
        }
        if short == 0 {
            Ok(())
        } else {
            Err((needs - short, needs))
        }
    }
}

/// For each slot of `print`, the kinds that meet it with how much the
/// stock holds of each, cheapest first.
fn meeting(
    items: &ItemCatalogue,
    stock: &[(Record, bool)],
    print: &Blueprint,
) -> Vec<Vec<(Record, u32)>> {
    print
        .slots
        .iter()
        .map(|need| {
            feeds_of(items, stock, need)
                .into_iter()
                .filter(|feed| feed.verdict.is_ok())
                .map(|feed| {
                    let held = feed.held();
                    (feed.kind, held)
                })
                .collect()
        })
        .collect()
}

/// A batch of `batch` allotted from the cheapest stock that meets each
/// slot, and the first slot it could not cover, if any.
fn allot_cheapest(
    print: &Blueprint,
    meeting: &[Vec<(Record, u32)>],
    batch: u32,
) -> (Allot, Option<String>) {
    let mut allot = Allot::default();
    let mut short = None;
    for (need, kinds) in print.slots.iter().zip(meeting) {
        if allot.slot(need, kinds, batch).is_err() && short.is_none() {
            short = Some(need.slot.id.clone());
        }
    }
    (allot, short)
}

/// The cheapest stock that feeds every slot of `print` for a batch of
/// `batch`: slot by slot, the kinds that meet it, the one that clears the
/// requirements by least first, as many as it takes to cover the batch,
/// with what earlier slots took of a kind counted out. A slot the stock
/// cannot cover holds what there is, and [`check_start`] says it is short.
#[must_use]
pub fn cheapest(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    print: &Blueprint,
    batch: u32,
) -> Feeds {
    let items = &registries.items;
    let stock = crafting::reachable(items, pack, store);
    let (allot, _) = allot_cheapest(print, &meeting(items, &stock, print), batch);
    let mut feeds = Feeds::new();
    for input in allot.inputs {
        feeds.entry(input.slot).or_default().push(input.kind);
    }
    feeds
}

/// The largest batch `print` can be run for from the stock that meets it,
/// no larger than the runs it has left, and which of the two holds it.
#[must_use]
pub fn limit(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    print: &Blueprint,
) -> Limit {
    let items = &registries.items;
    let stock = crafting::reachable(items, pack, store);
    let meeting = meeting(items, &stock, print);
    let covers = |batch: u32| allot_cheapest(print, &meeting, batch).1.is_none();
    // A batch the stock covers, it covers one smaller: the largest is
    // found by halving.
    let (mut most, mut over) = (0, print.runs_left);
    while most < over {
        let middle = most + (over - most).div_ceil(2);
        if covers(middle) {
            most = middle;
        } else {
            over = middle - 1;
        }
    }
    let held_by = match allot_cheapest(print, &meeting, most.saturating_add(1)).1 {
        Some(slot) if most < print.runs_left => Held::Stock { slot, feeds: most },
        _ => Held::Runs {
            left: print.runs_left,
        },
    };
    Limit { most, held_by }
}

/// Why the sim would not do what a factory request asked. The words a
/// player reads are the screen's; this is what is the matter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FactoryRefusal {
    /// There is no local player to work the fabricator.
    #[error("nobody is at the fabricator")]
    NoPlayer,
    /// No fabricator is within reach.
    #[error("a blueprint is run at a fabricator")]
    NoFabricator,
    /// A run is on already.
    #[error("the fabricator is running")]
    Busy,
    /// No such blueprint is in the pack, its bags or the store.
    #[error("no such blueprint is in reach")]
    NoBlueprint,
    /// The blueprint no longer fits the data set: what it makes or what a
    /// slot takes has changed since it was recorded.
    #[error("the blueprint no longer fits what it makes")]
    Stale,
    /// A batch of nought.
    #[error("a batch is one or more")]
    NoBatch,
    /// The batch is more than the blueprint has runs for.
    #[error("the blueprint has {left} runs left")]
    OverRuns {
        /// The runs it has left.
        left: u32,
    },
    /// What is picked for a slot will not feed it.
    #[error("slot `{slot}`: {why}")]
    Slot {
        /// The slot, by its id.
        slot: String,
        /// What is the matter with it.
        why: FeedRefusal,
    },
    /// No run is on.
    #[error("the fabricator is not running")]
    NoJob,
    /// The store has no room for what a stopped run gives back.
    #[error("there is no room")]
    NoRoom,
    /// The worker named is not a benched member of the household.
    #[error("only somebody on the bench works a run")]
    NotOnBench,
}

/// What a start would commit: the blueprint's item as the stock holds it,
/// and every input for the batch.
fn plan(
    registries: &GameRegistries,
    stock: &[(Record, bool)],
    prints: &Blueprints,
    state: &FactoryState,
    id: u32,
    batch: u32,
    feeds: &Feeds,
) -> Result<(Record, Vec<JobInput>), FactoryRefusal> {
    let items = &registries.items;
    if state.job.is_some() {
        return Err(FactoryRefusal::Busy);
    }
    let rules = registries
        .schematics
        .factory()
        .ok_or(FactoryRefusal::NoBlueprint)?;
    let print = prints.0.get(&id).ok_or(FactoryRefusal::NoBlueprint)?;
    let is_blueprint = |record: &Record| {
        matches!(
            items.get(&record.item).map(|def| &def.kind),
            Some(ItemKind::Blueprint)
        )
    };
    let item = stock
        .iter()
        .map(|(record, _)| record)
        .find(|record| record.batch == id && is_blueprint(record))
        .map(kind_of)
        .ok_or(FactoryRefusal::NoBlueprint)?;
    if !print.stands(registries) || rules.unit_ticks(print.complexity) == 0 {
        return Err(FactoryRefusal::Stale);
    }
    if batch == 0 {
        return Err(FactoryRefusal::NoBatch);
    }
    if batch > print.runs_left {
        return Err(FactoryRefusal::OverRuns {
            left: print.runs_left,
        });
    }
    let mut allot = Allot::default();
    for need in &print.slots {
        let refused = |why| FactoryRefusal::Slot {
            slot: need.slot.id.clone(),
            why,
        };
        let picked = feeds.get(&need.slot.id).map_or(&[][..], Vec::as_slice);
        if picked.is_empty() {
            return Err(refused(FeedRefusal::Empty));
        }
        let mut kinds: Vec<(Record, u32)> = Vec::with_capacity(picked.len());
        for chosen in picked {
            if !crafting::fits(items, &need.slot, chosen) {
                return Err(refused(FeedRefusal::WrongKind));
            }
            let kind = kind_of(chosen);
            meets(need, &kind).map_err(refused)?;
            if kinds.iter().any(|(listed, _)| *listed == kind) {
                continue;
            }
            let held = stock
                .iter()
                .filter(|(record, _)| kind_of(record) == kind)
                .map(|(record, _)| record.count)
                .sum();
            kinds.push((kind, held));
        }
        allot
            .slot(need, &kinds, batch)
            .map_err(|(have, needs)| refused(FeedRefusal::Short { have, needs }))?;
    }
    Ok((item, allot.inputs))
}

/// Whether a run of blueprint `id` for `batch` units fed by `feeds` would
/// start from this stock, asked in the order a player would fix it: that
/// the fabricator is free, the blueprint in reach and sound, the batch
/// within its runs, and each slot in the blueprint's order fed by kinds it
/// takes, that meet it, and that together hold the whole batch.
///
/// # Errors
/// The first of those that fails.
#[allow(
    clippy::too_many_arguments,
    reason = "the stock a screen reads, the two ledgers, and the three things a start is asked with"
)]
pub fn check_start(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    prints: &Blueprints,
    state: &FactoryState,
    id: u32,
    batch: u32,
    feeds: &Feeds,
) -> Result<(), FactoryRefusal> {
    let stock = crafting::reachable(&registries.items, pack, store);
    plan(registries, &stock, prints, state, id, batch, feeds).map(|_| ())
}

/// `ticks` of the sim as whole played seconds, a part of one counted whole.
fn seconds_of(ticks: u64) -> u32 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a run's ticks, a few hundred thousand, as seconds on a screen"
    )]
    {
        (ticks as f64 / SIM_TICK_HZ).ceil().min(f64::from(u32::MAX)) as u32
    }
}

/// The played seconds `units` of a blueprint of `complexity` take, with a
/// worker on the run or with nobody.
#[must_use]
pub fn seconds(rules: &FactoryRules, complexity: u8, units: u32, worked: bool) -> u32 {
    let work = u64::from(units) * u64::from(rules.unit_ticks(complexity)) * u64::from(PER_MILLE);
    seconds_of(work.div_ceil(u64::from(rules.rate_pm(worked).max(1))))
}

/// The played seconds `job` has left at the rate it is going, the unit
/// under way counted from where the clock stands. A paused run reads what
/// it would take once it goes on: the unit it stands on is made and only
/// waiting for room, whatever the clock says (one read back from a save has
/// a clock at nought until its first tick).
#[must_use]
pub fn left(rules: &FactoryRules, print: &Blueprint, job: &Job, clock: &FactoryClock) -> u32 {
    let rules = &*paced(rules, job.pace);
    let unit = u64::from(rules.unit_ticks(print.complexity)) * u64::from(PER_MILLE);
    let work = u64::from(job.size.saturating_sub(job.made)) * unit;
    let done = if job.paused.is_some() {
        unit
    } else {
        u64::from(clock.0).min(unit)
    };
    let work = work.saturating_sub(done);
    let rate = u64::from(rules.rate_pm(job.worker.is_some()).max(1));
    seconds_of(work.div_ceil(rate))
}

/// Start a run of blueprint `id` for `batch` units fed by `feeds`, worked
/// by `worker` or by nobody: the blueprint's item and every input for the
/// whole batch leave the pack, its bags and the store, the pack first; the
/// run takes the household's next batch number, and the ledger files what a
/// unit of it is made from. One transaction: a refusal has taken nothing.
/// Gives the batch number and the played seconds the run will take.
///
/// # Errors
/// What [`check_start`] refuses, or a worker who is not on the bench.
#[allow(
    clippy::too_many_arguments,
    reason = "the stock, the three ledgers a start writes or reads, and the four things a start is asked with"
)]
pub fn start(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    prints: &Blueprints,
    craft: &mut CraftState,
    state: &mut FactoryState,
    benched: &Benched,
    (id, batch): (u32, u32),
    feeds: &Feeds,
    (worker, pace): (Option<&str>, Option<Pace>),
) -> Result<(u32, u32), FactoryRefusal> {
    let items = &registries.items;
    let reached = crafting::reachable(items, stock.pack, stock.store.as_deref());
    let (item, inputs) = plan(registries, &reached, prints, state, id, batch, feeds)?;
    if let Some(name) = worker
        && !benched.0.iter().any(|benched| benched == name)
    {
        return Err(FactoryRefusal::NotOnBench);
    }
    // Both were found by `plan`.
    let (Some(rules), Some(print)) = (registries.schematics.factory(), prints.0.get(&id)) else {
        return Err(FactoryRefusal::NoBlueprint);
    };
    let mut scratch = Scratch::of(stock);
    let held = items::write(&item, items).map_err(|_| FactoryRefusal::NoBlueprint)?;
    if !scratch.take(items, &held, 1) {
        return Err(FactoryRefusal::NoBlueprint);
    }
    for input in &inputs {
        let short = || FactoryRefusal::Slot {
            slot: input.slot.clone(),
            why: FeedRefusal::Short {
                have: 0,
                needs: input.left,
            },
        };
        let wanted = items::write(&input.kind, items).map_err(|_| short())?;
        if !scratch.take(items, &wanted, input.left) {
            return Err(short());
        }
    }
    scratch.commit(stock);
    let number = craft.batches.saturating_add(1);
    craft.batches = number;
    // A unit's worth of each kind: what a card says one was made from.
    let from = inputs
        .iter()
        .filter_map(|input| {
            let need = print.slots.iter().find(|need| need.slot.id == input.slot)?;
            let unit = Record {
                count: need.slot.amount(),
                ..input.kind.clone()
            };
            Some((input.slot.clone(), unit))
        })
        .collect();
    craft.made.insert(
        number,
        Made {
            item: print.makes.clone(),
            by: print.by.clone(),
            from,
            schematic: Some(print.schematic.clone()),
            recorded: None,
            blueprint: Some(id),
        },
    );
    state.job = Some(Job {
        blueprint: id,
        item,
        batch: number,
        size: batch,
        made: 0,
        inputs,
        worker: worker.map(str::to_owned),
        paused: None,
        pace,
    });
    Ok((
        number,
        seconds(
            &paced(rules, pace),
            print.complexity,
            batch,
            worker.is_some(),
        ),
    ))
}

/// Put what a run holds back in `store`: every input's unspent amount, and
/// the blueprint's item while it has runs left.
fn give_back(
    items: &ItemCatalogue,
    prints: &Blueprints,
    job: &Job,
    store: &mut HouseholdStore,
) -> Result<(), FactoryRefusal> {
    let mut back = |record: &Record, count: u32| {
        // A kind the data no longer reads has no stack to give.
        let Ok(mut stack) = items::write(record, items) else {
            return Ok(());
        };
        stack.count = count;
        match items::place_in(items, &mut store.0, stack) {
            None => Ok(()),
            Some(_) => Err(FactoryRefusal::NoRoom),
        }
    };
    for input in job.inputs.iter().filter(|input| input.left > 0) {
        back(&input.kind, input.left)?;
    }
    let unspent = prints
        .0
        .get(&job.blueprint)
        .is_some_and(|print| print.runs_left > 0);
    if unspent {
        back(&job.item, 1)?;
    }
    Ok(())
}

/// Whether `job` can be stopped: whether `store` has room for the unspent
/// inputs and the blueprint.
///
/// # Errors
/// No room.
pub fn can_stop(
    items: &ItemCatalogue,
    prints: &Blueprints,
    job: &Job,
    store: &HouseholdStore,
) -> Result<(), FactoryRefusal> {
    give_back(items, prints, job, &mut store.clone())
}

/// Stop the run: what it has made stays in the store, the inputs it has
/// not spent and the blueprint go back to the store, and the blueprint
/// keeps the runs it has left. One transaction: refused, nothing has moved
/// and the run goes on. Gives how many it had made.
///
/// # Errors
/// No run, or no room for what it gives back.
pub fn stop(
    registries: &GameRegistries,
    prints: &Blueprints,
    state: &mut FactoryState,
    clock: &mut FactoryClock,
    store: &mut HouseholdStore,
) -> Result<u32, FactoryRefusal> {
    let job = state.job.as_ref().ok_or(FactoryRefusal::NoJob)?;
    let mut next = store.clone();
    give_back(&registries.items, prints, job, &mut next)?;
    *store = next;
    let made = job.made;
    state.job = None;
    clock.0 = 0;
    Ok(made)
}

/// Put `worker`, or nobody, on the run under way.
///
/// # Errors
/// No run, or a worker who is not on the bench.
pub fn assign(
    state: &mut FactoryState,
    benched: &Benched,
    worker: Option<&str>,
) -> Result<(), FactoryRefusal> {
    let job = state.job.as_mut().ok_or(FactoryRefusal::NoJob)?;
    if let Some(name) = worker
        && !benched.0.iter().any(|benched| benched == name)
    {
        return Err(FactoryRefusal::NotOnBench);
    }
    job.worker = worker.map(str::to_owned);
    Ok(())
}

/// What a run did on its own.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum FactoryNotice {
    /// The batch is made and the fabricator is free.
    Finished {
        /// The item made.
        item: Namespaced,
        /// How many.
        made: u32,
        /// Their batch number.
        batch: u32,
        /// Whether the blueprint's last run went into it, and its item
        /// with it.
        spent: bool,
    },
    /// The run stopped where it was.
    Paused(Pause),
    /// And went on.
    Resumed,
}

/// Take a unit's amount for every slot of `print` out of the run's inputs,
/// across a slot's kinds in their order.
fn spend_unit(print: &Blueprint, inputs: &mut Vec<JobInput>) {
    for need in &print.slots {
        let mut amount = need.slot.amount();
        for input in inputs.iter_mut().filter(|input| input.slot == need.slot.id) {
            let taken = input.left.min(amount);
            input.left -= taken;
            amount -= taken;
        }
    }
    inputs.retain(|input| input.left > 0);
}

/// One tick of the run. The clock gains the run's rate; when it reaches a
/// unit, one is made with the blueprint's lines exactly and put in `store`
/// alone, the blueprint loses a run and the hopper a unit's inputs, and
/// what the clock has over the unit goes toward the next; when the last is
/// made the blueprint's item goes back to the store, if it has a run left,
/// and the fabricator is free. With no room for the unit or for the
/// blueprint the run pauses, says so once, and tries again every tick: a
/// paused run is a made unit waiting for room, so its clock is held at the
/// unit, which is also what gives a pause read back from a save (the clock
/// is not saved) its unit again. Gives what there is to tell, if anything.
pub fn advance(
    registries: &GameRegistries,
    prints: &mut Blueprints,
    state: &mut FactoryState,
    clock: &mut FactoryClock,
    store: &mut HouseholdStore,
) -> Option<FactoryNotice> {
    let items = &registries.items;
    let rules = registries.schematics.factory()?;
    let job = state.job.as_mut()?;
    let rules = &*paced(rules, job.pace);
    // A run whose blueprint is gone or no longer makes anything stands
    // until it is stopped; the save's repair drops it at the next start.
    let print = prints.0.get_mut(&job.blueprint)?;
    let unit = rules.unit_ticks(print.complexity).saturating_mul(PER_MILLE);
    let mut notice = None;
    if job.made < job.size {
        if job.paused.is_some() {
            clock.0 = unit;
        } else {
            let rate = u32::from(rules.rate_pm(job.worker.is_some()));
            clock.0 = clock.0.saturating_add(rate);
        }
        if clock.0 < unit {
            return None;
        }
        let made = items::write(&print.unit(job.batch, items), items).ok()?;
        if items::place_in(items, &mut store.0, made).is_some() {
            clock.0 = unit;
            return job.paused.is_none().then(|| {
                job.paused = Some(Pause::NoRoom);
                FactoryNotice::Paused(Pause::NoRoom)
            });
        }
        if job.paused.take().is_some() {
            notice = Some(FactoryNotice::Resumed);
        }
        job.made += 1;
        print.runs_left = print.runs_left.saturating_sub(1);
        spend_unit(print, &mut job.inputs);
        clock.0 -= unit;
        if job.made < job.size {
            return notice;
        }
    }
    // The batch is made: the blueprint goes back, unless that was its
    // last run.
    let spent = print.runs_left == 0;
    if !spent {
        let back = items::write(&job.item, items).ok()?;
        if items::place_in(items, &mut store.0, back).is_some() {
            return (job.paused.is_none())
                .then(|| {
                    job.paused = Some(Pause::NoRoom);
                    FactoryNotice::Paused(Pause::NoRoom)
                })
                .or(notice);
        }
    }
    let finished = FactoryNotice::Finished {
        item: print.makes.clone(),
        made: job.made,
        batch: job.batch,
        spent,
    };
    state.job = None;
    clock.0 = 0;
    Some(finished)
}

impl FactoryState {
    /// Bring a saved run back inside the data set where an edit to the
    /// data or a broken file has put it outside. A run whose blueprint the
    /// ledger has not got, or whose blueprint no longer fits the data, is
    /// dropped: what still reads of its inputs goes to `store`, and the
    /// blueprint's item with them when the ledger still has it. A run that
    /// stands is held to its batch, and an input that no longer reads is
    /// dropped from its hopper. Said in words for a log, one for each thing
    /// changed; a state nothing was wrong with is left as it was.
    pub fn repair(
        &mut self,
        registries: &GameRegistries,
        prints: &Blueprints,
        store: &mut HouseholdStore,
    ) -> Vec<String> {
        let items = &registries.items;
        let mut said = Vec::new();
        let Some(job) = self.job.as_mut() else {
            return said;
        };
        let number = job.blueprint;
        let print = prints.0.get(&number);
        let sound = registries.schematics.factory().is_some()
            && print.is_some_and(|print| print.stands(registries));
        if !sound {
            let mut lost = 0_u32;
            let mut back = |record: &Record, count: u32| {
                let placed = items::write(record, items).is_ok_and(|mut stack| {
                    stack.count = count;
                    items::place_in(items, &mut store.0, stack).is_none()
                });
                if !placed {
                    lost += 1;
                }
            };
            for input in job.inputs.iter().filter(|input| input.left > 0) {
                back(&input.kind, input.left);
            }
            if print.is_some_and(|print| print.runs_left > 0) {
                back(&job.item, 1);
            }
            let why = if print.is_some() {
                "no longer fits the data"
            } else {
                "is not in the ledger"
            };
            said.push(format!(
                "the run of blueprint {number} is dropped: the blueprint {why}; what it held is in the store{}",
                if lost > 0 {
                    format!(", but for {lost} stacks that no longer read or found no room")
                } else {
                    String::new()
                }
            ));
            self.job = None;
            return said;
        }
        if job.made > job.size {
            said.push(format!(
                "the run of blueprint {number} had made {} of {}; it has made {}",
                job.made, job.size, job.size
            ));
            job.made = job.size;
        }
        job.inputs.retain(|input| {
            let why = items.check_record(&input.kind).err();
            if let Some(why) = &why {
                said.push(format!(
                    "the run of blueprint {number} no longer holds its input for slot `{}` ({why})",
                    input.slot
                ));
            }
            why.is_none()
        });
        said
    }
}

/// What a screen asks of the fabricator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryRequest {
    /// Start a run.
    Start {
        /// The blueprint, by its number.
        blueprint: u32,
        /// How many to make.
        batch: u32,
        /// The kinds that feed each slot.
        feeds: Feeds,
        /// The benched member to work it, by name; nobody, and it runs at
        /// the unattended rate.
        worker: Option<String>,
    },
    /// Put somebody, or nobody, on the run under way.
    Assign {
        /// The benched member, by name.
        worker: Option<String>,
    },
    /// Stop the run under way.
    Stop,
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message, for the reason the stock's inbox is one: a screen
/// writes it in `Update`, the tick may not run that frame or may run
/// twice, and a request is consumed exactly once.
#[derive(Resource, Debug, Default)]
pub struct FactoryInbox(pub Vec<FactoryRequest>);

/// What a request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryDone {
    /// The inputs are committed and the run is on.
    Started {
        /// The batch number its units carry.
        batch: u32,
        /// The played seconds it will take at the rate it starts at.
        seconds: u32,
    },
    /// The worker was changed.
    Assigned,
    /// The run is stopped and what it held is back in the store.
    Stopped {
        /// How many it had made.
        made: u32,
    },
}

/// The sim's answer to a [`FactoryRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct FactoryAnswer {
    /// The request it answers.
    pub request: FactoryRequest,
    /// What happened, or why nothing did.
    pub outcome: Result<FactoryDone, FactoryRefusal>,
}

/// Whether a fabricator may be worked from where the player stands: with
/// one in reach, or from anywhere when the run says so (a still of the
/// screen has none to stand at). The rule a request is held to, and what a
/// screen greys its buttons by.
#[must_use]
pub fn at_fabricator(access: WorkbenchAccess, reach: &Reach) -> bool {
    access == WorkbenchAccess::Anywhere || reach.fabricator
}

/// The truth for an open stores screen whose mirror was not told of a
/// change it did not click.
fn resync(
    open: &OpenStock,
    pack: &Pack,
    hands: &Hands,
    store: &HouseholdStore,
    mirrors: &mut MessageWriter<StockAnswer>,
) {
    let Some(opened) = &open.0 else {
        return;
    };
    // The snapshot only reads; the copies keep the components' change
    // detection out of it.
    let mut truth = (pack.clone(), hands.clone(), store.clone());
    let stock = Stock {
        saddle: None,
        pack: &mut truth.0,
        hands: &mut truth.1,
        store: opened.with_store.then_some(&mut truth.2),
    };
    mirrors.write(StockAnswer::Resync {
        menu: opened.menu,
        snapshot: stock.snapshot(&opened.state, &opened.saddle),
    });
}

/// `SimTick`, `SimSet::Input`, after the bench's requests: every factory
/// request made since the last tick, in order, each on what the one before
/// left.
///
/// The fabricator rule is asked at every request and not when the screen
/// opened, since the world runs under an open screen. With a fabricator in
/// reach the household's store is to hand, as at a workbench, so a
/// blueprint and its inputs come from the pack, its bags or the store. The
/// stores screen, if it is open, is answered with the truth.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock, the bench and the three ledgers in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<FactoryInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    benched: Res<Benched>,
    prints: Res<Blueprints>,
    mut craft: ResMut<CraftState>,
    mut state: ResMut<FactoryState>,
    mut clock: ResMut<FactoryClock>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands, Option<&CharacterSheet>), With<LocalPlayer>>,
    mut answers: MessageWriter<FactoryAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let registries = &registries.0;
    let mut player = players.single_mut().ok();
    let in_reach = at_fabricator(*access, &reach);
    let mut moved = false;
    for request in requests {
        let outcome = match player.as_mut() {
            None => Err(FactoryRefusal::NoPlayer),
            Some(_) if !in_reach => Err(FactoryRefusal::NoFabricator),
            Some((pack, hands, sheet)) => {
                // Worked on copies and written back only where something
                // moved, so a refusal leaves no change for a reader to
                // chase.
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let (mut ledger, mut run, mut ticked) = (craft.clone(), state.clone(), *clock);
                // The run is kept at the pace of whoever starts it.
                let starters = registries
                    .schematics
                    .factory()
                    .zip(*sheet)
                    .and_then(|(rules, sheet)| pace_of(rules, &registries.progression, sheet));
                let done = match &request {
                    FactoryRequest::Start {
                        blueprint,
                        batch,
                        feeds,
                        worker,
                    } => {
                        let mut stock = Stock {
                            saddle: None,
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: Some(&mut next.2),
                        };
                        start(
                            registries,
                            &mut stock,
                            &prints,
                            &mut ledger,
                            &mut run,
                            &benched,
                            (*blueprint, *batch),
                            feeds,
                            (worker.as_deref(), starters),
                        )
                        .map(|(batch, seconds)| {
                            ticked.0 = 0;
                            FactoryDone::Started { batch, seconds }
                        })
                    }
                    FactoryRequest::Assign { worker } => {
                        assign(&mut run, &benched, worker.as_deref())
                            .map(|()| FactoryDone::Assigned)
                    }
                    FactoryRequest::Stop => {
                        stop(registries, &prints, &mut run, &mut ticked, &mut next.2)
                            .map(|made| FactoryDone::Stopped { made })
                    }
                };
                if done.is_ok() {
                    if **pack != next.0 {
                        **pack = next.0;
                        moved = true;
                    }
                    if *store != next.2 {
                        *store = next.2;
                        moved = true;
                    }
                    if *craft != ledger {
                        *craft = ledger;
                    }
                    if *state != run {
                        *state = run;
                    }
                    if *clock != ticked {
                        *clock = ticked;
                    }
                }
                done
            }
        };
        answers.write(FactoryAnswer { request, outcome });
    }
    if moved && let Some((pack, hands, _)) = player.as_ref() {
        resync(&open, pack, hands, &store, &mut mirrors);
    }
}

/// How many nominal ticks, [`SIM_TICK_HZ`] a played second, one step of a
/// sim stepped `stepped_hz` times a second is worth, with `phase` carrying
/// what is left over between steps: one a step at the nominal rate, one
/// every third step at sixty. Whole numbers, so a run takes the same steps
/// on every machine.
pub fn nominal_ticks(phase: &mut u32, stepped_hz: u32) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the sim's rate, twenty"
    )]
    let nominal = SIM_TICK_HZ.round() as u32;
    let stepped = stepped_hz.max(1);
    *phase = phase.saturating_add(nominal);
    let due = *phase / stepped;
    *phase %= stepped;
    due
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: the run's tick
/// ([`advance`]), as many nominal ticks as this step of the sim is worth
/// ([`nominal_ticks`]). What the run changes is marked changed when it
/// changes, which is once a unit: the household saves on a change, and a
/// file a tick would be twenty a second.
#[allow(
    clippy::too_many_arguments,
    reason = "the three ledgers a tick may write, the two clocks, the store, and the two things it tells"
)]
pub fn run(
    registries: Res<Registries>,
    open: Res<OpenStock>,
    sim: Option<Res<SimClock>>,
    mut prints: ResMut<Blueprints>,
    mut state: ResMut<FactoryState>,
    mut clock: ResMut<FactoryClock>,
    mut store: ResMut<HouseholdStore>,
    players: Query<(&Pack, &Hands), With<LocalPlayer>>,
    mut notices: MessageWriter<FactoryNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut phase: Local<u32>,
) {
    let read = |state: &FactoryState| state.job.as_ref().map(|job| (job.made, job.paused));
    let Some(before) = read(&state) else {
        return;
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let mut told = Vec::new();
    for _ in 0..nominal_ticks(&mut phase, stepped) {
        told.extend(advance(
            &registries.0,
            prints.bypass_change_detection(),
            state.bypass_change_detection(),
            clock.bypass_change_detection(),
            store.bypass_change_detection(),
        ));
    }
    let after = read(&state);
    if after != Some(before) {
        state.set_changed();
    }
    // A unit made or the blueprint handed back: the store and the
    // blueprint's runs moved.
    if after.is_none_or(|(made, _)| made != before.0) {
        prints.set_changed();
        store.set_changed();
        if let Ok((pack, hands)) = players.single()
            && open.0.as_ref().is_some_and(|opened| opened.with_store)
        {
            resync(&open, pack, hands, &store, &mut mirrors);
        }
    }
    for notice in told {
        notices.write(notice);
    }
}

/// Registers the fabricator's state and requests, consumes them in the
/// tick and runs the job. Added by `SimPlugin`, so a server and a harness
/// have it too; with no request and no job, which is all either ever has,
/// it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct FactoryPlugin;

impl Plugin for FactoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FactoryInbox>()
            .init_resource::<FactoryState>()
            .init_resource::<FactoryClock>()
            .init_resource::<Benched>()
            .add_message::<FactoryAnswer>()
            .add_message::<FactoryNotice>()
            .add_systems(
                SimTick,
                (consume, run)
                    .chain()
                    .in_set(SimSet::Input)
                    .after(crate::crafting::consume),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::blueprint::{self, Need};
    use crate::crafting::tests::{
        ALLOY, BRINE, CORE, CORE_ITEM, FEED, FEED_ITEM, HARNESS, MAKER, SALT, SEALS, ask, fill,
        first_day, id, lot, part, prop, schematic, shipped, smith, truth, units, work_and_finish,
    };
    use crate::crafting::{CraftDone, CraftRequest, Picks, assemble, finish};
    use crate::progression::{CharacterSheet, Interaction, InteractionKind};
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;

    const BLUEPRINT: &str = "veyra:blueprint.production";
    const WORKER: &str = "Ilse Marrow";

    /// The first day's salt, the prototype's own: potency 632, stability
    /// 225.
    fn tolley(registries: &Registries, units: u32) -> Record {
        lot(registries, SALT, "Tolley", units, &[389, 632, 225])
    }

    /// The first day's brine: purity 534, potency 605, conductivity 495.
    fn oxbow(registries: &Registries, units: u32) -> Record {
        lot(registries, BRINE, "Oxbow", units, &[534, 605, 495])
    }

    /// A better salt than the prototype's.
    fn calder(registries: &Registries, units: u32) -> Record {
        lot(registries, SALT, "Calder", units, &[640, 820, 450])
    }

    /// A salt under the prototype's potency.
    fn marsh(registries: &Registries, units: u32) -> Record {
        lot(registries, SALT, "Marsh", units, &[400, 410, 300])
    }

    /// A better brine than the prototype's on every figure.
    fn deepwell(registries: &Registries, units: u32) -> Record {
        lot(registries, BRINE, "Deepwell", units, &[700, 700, 700])
    }

    /// A household at a fabricator: the first day's pack and store with
    /// `lots` after it, the walk's cell core (513 and 209, batch 3) made of
    /// the first Tolley salt and Oxbow brine among them and lying in the
    /// pack, and blueprint 4 recorded of it at `derate`, its item beside
    /// the core.
    struct Shop {
        registries: Registries,
        pack: Pack,
        hands: Hands,
        store: HouseholdStore,
        craft: CraftState,
        prints: Blueprints,
        state: FactoryState,
        clock: FactoryClock,
        benched: Benched,
    }

    fn shop(lots: impl Fn(&Registries) -> Vec<Record>, derate: u16) -> Shop {
        let registries = shipped();
        let lots = lots(&registries);
        let (mut pack, mut hands, mut store) = crate::crafting::tests::bench(&registries, &lots);
        let picks = Picks::from([
            ("salt".to_owned(), kind_of(&tolley(&registries, 1))),
            ("brine".to_owned(), kind_of(&oxbow(&registries, 1))),
        ]);
        let mut craft = CraftState {
            batches: 2,
            ..CraftState::default()
        };
        let mut prints = Blueprints::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assemble(
            &registries.0,
            &mut stock,
            &mut craft,
            &id(CORE),
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        // The walk's rolls: 0.86 of charge density's 597.
        craft.session.as_mut().unwrap().lines[0].share = 860;
        let core = finish(&registries.0, &mut stock, &mut craft).unwrap();
        assert_eq!(
            (
                core.batch,
                core.lines["charge_density"],
                core.lines["stability"]
            ),
            (3, 513, 209)
        );
        let (number, _) = blueprint::record(
            &registries.0,
            &mut stock,
            &mut craft,
            &mut prints,
            3,
            derate,
            MAKER,
        )
        .unwrap();
        assert_eq!(number, 4);
        Shop {
            registries,
            pack,
            hands,
            store,
            craft,
            prints,
            state: FactoryState::default(),
            clock: FactoryClock::default(),
            benched: Benched(vec![WORKER.to_owned()]),
        }
    }

    impl Shop {
        fn print(&self) -> &Blueprint {
            &self.prints.0[&4]
        }

        fn rules(&self) -> &FactoryRules {
            self.registries.0.schematics.factory().unwrap()
        }

        fn feeds(&self, slot: &str) -> Vec<Feed> {
            feeds(
                &self.registries.0,
                &self.pack,
                Some(&self.store),
                self.print(),
                slot,
            )
        }

        fn cheapest(&self, batch: u32) -> Feeds {
            cheapest(
                &self.registries.0,
                &self.pack,
                Some(&self.store),
                self.print(),
                batch,
            )
        }

        fn limit(&self) -> Limit {
            limit(
                &self.registries.0,
                &self.pack,
                Some(&self.store),
                self.print(),
            )
        }

        fn check(&self, batch: u32, feeds: &Feeds) -> Result<(), FactoryRefusal> {
            check_start(
                &self.registries.0,
                &self.pack,
                Some(&self.store),
                &self.prints,
                &self.state,
                4,
                batch,
                feeds,
            )
        }

        fn start(
            &mut self,
            batch: u32,
            feeds: &Feeds,
            worker: Option<&str>,
        ) -> Result<(u32, u32), FactoryRefusal> {
            self.start_paced(batch, feeds, (worker, None))
        }

        fn start_paced(
            &mut self,
            batch: u32,
            feeds: &Feeds,
            worker: (Option<&str>, Option<Pace>),
        ) -> Result<(u32, u32), FactoryRefusal> {
            let mut stock = Stock {
                saddle: None,
                pack: &mut self.pack,
                hands: &mut self.hands,
                store: Some(&mut self.store),
            };
            let started = start(
                &self.registries.0,
                &mut stock,
                &self.prints,
                &mut self.craft,
                &mut self.state,
                &self.benched,
                (4, batch),
                feeds,
                worker,
            );
            if started.is_ok() {
                self.clock.0 = 0;
            }
            started
        }

        fn tick(&mut self) -> Option<FactoryNotice> {
            advance(
                &self.registries.0,
                &mut self.prints,
                &mut self.state,
                &mut self.clock,
                &mut self.store,
            )
        }

        /// Tick `ticks` times; what was told, each with the tick it was
        /// told on, counted from one.
        fn ticks(&mut self, ticks: u32) -> Vec<(u32, FactoryNotice)> {
            (1..=ticks)
                .filter_map(|tick| Some((tick, self.tick()?)))
                .collect()
        }

        fn stop(&mut self) -> Result<u32, FactoryRefusal> {
            stop(
                &self.registries.0,
                &self.prints,
                &mut self.state,
                &mut self.clock,
                &mut self.store,
            )
        }

        /// How much of `kind` the pack's cells and the store hold.
        fn held(&self, kind: &Record) -> u32 {
            let items = &self.registries.0.items;
            units(&self.pack.cells, kind, items) + units(&self.store.0, kind, items)
        }

        /// The stacks of cell cores the store holds.
        fn cores(&self) -> Vec<Record> {
            let items = &self.registries.0.items;
            self.store
                .0
                .slots()
                .iter()
                .flatten()
                .filter_map(|stack| items::read(stack, items).ok())
                .filter(|record| record.item == id(CORE_ITEM))
                .collect()
        }

        fn job(&self) -> &Job {
            self.state.job.as_ref().unwrap()
        }

        /// Blueprint 4's item, wherever the stock holds it.
        fn blueprint_item(&self) -> Record {
            let items = &self.registries.0.items;
            crafting::reachable(items, &self.pack, Some(&self.store))
                .into_iter()
                .map(|(record, _)| record)
                .find(|record| record.item == id(BLUEPRINT))
                .unwrap()
        }

        fn everything(&self) -> (Pack, HouseholdStore, CraftState, Blueprints, FactoryState) {
            (
                self.pack.clone(),
                self.store.clone(),
                self.craft.clone(),
                self.prints.clone(),
                self.state.clone(),
            )
        }

        /// Every empty cell of the store taken by something that is no
        /// part, so nothing merges onto it; the cells it took.
        fn crowd(&mut self) -> Vec<usize> {
            let free: Vec<usize> = (0..self.store.0.len())
                .filter(|cell| self.store.0.get(*cell).is_none())
                .collect();
            let all = self.store.0.len();
            fill(&mut self.store.0, all, &self.registries.0.items);
            free
        }

        fn free(&mut self, cell: usize) {
            self.store.0.set(cell, None);
            self.store.0.clear_changed();
        }
    }

    /// The first day's two lots as the walk leaves them for a fabricator.
    fn first(registries: &Registries) -> Vec<Record> {
        vec![tolley(registries, 160), oxbow(registries, 160)]
    }

    fn feed(salt: &[&Record], brine: &[&Record]) -> Feeds {
        let kinds = |lots: &[&Record]| lots.iter().map(|lot| kind_of(lot)).collect();
        Feeds::from([
            ("salt".to_owned(), kinds(salt)),
            ("brine".to_owned(), kinds(brine)),
        ])
    }

    /// The step's bar: a hundred cores from one run are one stack of one
    /// batch, every one with the blueprint's lines exactly, the stock that
    /// fed them is spent to the unit, and the blueprint with its last run.
    #[test]
    fn a_hundred_identical_cores() {
        // 2,000 salt and 3,000 brine once the prototype has taken its own.
        let mut shop = shop(
            |registries| {
                let mut lots = first(registries);
                lots.extend((0..9).map(|_| tolley(registries, 200)));
                lots.push(tolley(registries, 60));
                lots.extend((0..14).map(|_| oxbow(registries, 200)));
                lots.push(oxbow(registries, 70));
                lots
            },
            1000,
        );
        let (salt, brine) = (tolley(&shop.registries, 1), oxbow(&shop.registries, 1));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (2000, 3000));
        assert_eq!(
            shop.limit(),
            Limit {
                most: 100,
                held_by: Held::Runs { left: 100 }
            }
        );
        let feeds = shop.cheapest(100);
        assert_eq!(feeds, feed(&[&salt], &[&brine]));
        assert_eq!(shop.check(100, &feeds), Ok(()));
        // Forty minutes with nobody on it.
        assert_eq!(shop.start(100, &feeds, None), Ok((5, 2400)));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (0, 0));
        assert!(shop.cores().is_empty(), "no time has passed");

        // The first unit, as the blueprint says it is.
        assert_eq!(shop.ticks(480), []);
        let expected = Record {
            item: id(CORE_ITEM),
            count: 1,
            batch: 5,
            by: Some(MAKER.to_owned()),
            lines: BTreeMap::from([
                ("charge_density".to_owned(), 513),
                ("stability".to_owned(), 209),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(shop.cores(), std::slice::from_ref(&expected));
        assert_eq!(expected, shop.print().unit(5, &shop.registries.0.items));
        assert_eq!((shop.job().made, shop.print().runs_left), (1, 99));

        // The other ninety-nine: told once, at the end.
        let told = shop.ticks(99 * 480);
        assert_eq!(
            told,
            [(
                99 * 480,
                FactoryNotice::Finished {
                    item: id(CORE_ITEM),
                    made: 100,
                    batch: 5,
                    spent: true,
                }
            )]
        );
        // One stack of a hundred: one item, one batch, one maker, the
        // blueprint's lines, since stacks merge only when all of it is
        // equal.
        assert_eq!(
            shop.cores(),
            [Record {
                count: 100,
                ..expected
            }]
        );
        assert_eq!(shop.cores()[0].lines, shop.print().lines);
        assert_eq!(shop.state.job, None);
        // The blueprint's runs fell by a hundred, and its item went with
        // the last.
        assert_eq!((shop.print().runs, shop.print().runs_left), (100, 0));
        let held = blueprint::held(
            &shop.registries.0,
            &shop.pack,
            Some(&shop.store),
            &shop.prints,
        );
        assert!(held.is_empty(), "{held:?}");
        assert_eq!((shop.held(&salt), shop.held(&brine)), (0, 0));
        // The ledger says what a unit was made from and what it was run
        // off; the prototype is in the pack as it was.
        let filed = &shop.craft.made[&5];
        assert_eq!(
            (filed.item.clone(), filed.by.as_str(), filed.blueprint),
            (id(CORE_ITEM), MAKER, Some(4))
        );
        assert_eq!(filed.schematic, Some(id(CORE)));
        let from: Vec<(&str, u32, &Namespaced)> = filed
            .from
            .iter()
            .map(|(slot, record)| (slot.as_str(), record.count, &record.item))
            .collect();
        assert_eq!(from, [("salt", 20, &salt.item), ("brine", 30, &brine.item)]);
        assert_eq!(shop.craft.batches, 5);
        assert_eq!(shop.craft.check(&shop.registries.0), Ok(()));
        let prototype = Record {
            batch: 3,
            ..shop.print().unit(3, &shop.registries.0.items)
        };
        assert_eq!(
            units(&shop.pack.cells, &prototype, &shop.registries.0.items),
            1
        );
        // A spent blueprint starts nothing more.
        shop.state = FactoryState::default();
        assert_eq!(shop.limit().most, 0);
        assert_eq!(shop.tick(), None);
    }

    /// The other half of the bar: a lot under a minimum is listed after
    /// the stock that meets, with the figure it has and the one asked for,
    /// and a start fed with it is refused with the same and takes nothing;
    /// a Light stat over its ceiling likewise.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "a floor on the core's blueprint, then a ceiling on a magazine's"
    )]
    fn a_lot_under_the_minimum_is_refused_with_its_number() {
        let mut shop = shop(
            |registries| {
                let mut lots = first(registries);
                lots.push(marsh(registries, 120));
                lots
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (salt, poor, brine) = (
            tolley(&registries, 1),
            marsh(&registries, 1),
            oxbow(&registries, 1),
        );
        let under = FeedRefusal::Under {
            stat: "potency".to_owned(),
            has: 410,
            needs: 632,
        };
        assert_eq!(under.to_string(), "potency 410, needs 632");
        let listed = shop.feeds("salt");
        assert_eq!(listed.len(), 2);
        assert_eq!(
            (&listed[0].kind, &listed[0].verdict, listed[0].margin),
            (&salt, &Ok(()), 0)
        );
        assert_eq!((listed[0].carried, listed[0].stored), (0, 140));
        assert_eq!(
            (&listed[1].kind, &listed[1].verdict),
            (&poor, &Err(under.clone()))
        );
        assert_eq!(
            listed[1].figures,
            [("potency".to_owned(), 410), ("stability".to_owned(), 300)]
        );
        assert_eq!(listed[1].held(), 120);
        assert_eq!(meets(&shop.print().slots[0], &poor), Err(under.clone()));
        assert_eq!(meets(&shop.print().slots[0], &salt), Ok(()));

        let before = shop.everything();
        let refused = FactoryRefusal::Slot {
            slot: "salt".to_owned(),
            why: under,
        };
        for fed in [feed(&[&poor], &[&brine]), feed(&[&salt, &poor], &[&brine])] {
            assert_eq!(shop.check(1, &fed), Err(refused.clone()));
            assert_eq!(shop.start(1, &fed, None), Err(refused.clone()));
        }
        assert_eq!(shop.everything(), before, "nothing was taken");
        // Something the slot does not take at all, and nothing.
        assert_eq!(
            shop.start(1, &feed(&[&brine], &[&brine]), None),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::WrongKind
            })
        );
        assert_eq!(
            shop.start(1, &feed(&[], &[&brine]), None),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Empty
            })
        );
        assert_eq!(shop.everything(), before);

        // A magazine's blueprint: its casing was Rake light alloy, density
        // 165, which the schematic reads as Light, so the blueprint asks
        // for a density of at most 165 and a heavier ore is over it.
        let rake = lot(
            &registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        );
        let heavy = lot(
            &registries,
            ALLOY,
            "Lode",
            200,
            &[533, 298, 312, 231, 451, 633, 400, 492],
        );
        let core = part(&registries, CORE_ITEM, 1, Some((1, &[537, 209])));
        let magazine = part(&registries, FEED_ITEM, 1, Some((6, &[527, 372])));
        shop.craft.batches = 6;
        shop.craft.made.insert(
            6,
            Made {
                item: id(FEED_ITEM),
                by: MAKER.to_owned(),
                from: vec![
                    ("core".to_owned(), core.clone()),
                    ("harness".to_owned(), part(&registries, HARNESS, 1, None)),
                    (
                        "casing".to_owned(),
                        Record {
                            count: 40,
                            ..rake.clone()
                        },
                    ),
                    ("seals".to_owned(), part(&registries, SEALS, 1, None)),
                ],
                schematic: Some(id(FEED)),
                recorded: None,
                blueprint: None,
            },
        );
        let print = blueprint::draft(&registries.0, &shop.craft, &magazine, 1000, MAKER).unwrap();
        let casing = print
            .slots
            .iter()
            .find(|need| need.slot.id == "casing")
            .unwrap();
        assert!(casing.needs.contains(&Need {
            stat: "density".to_owned(),
            at_least: None,
            at_most: Some(165),
        }));
        let over = FeedRefusal::Over {
            stat: "density".to_owned(),
            has: 400,
            most: 165,
        };
        assert_eq!(over.to_string(), "density 400, at most 165");
        assert_eq!(meets(casing, &kind_of(&heavy)), Err(over.clone()));
        assert_eq!(meets(casing, &kind_of(&rake)), Ok(()));
        let items = &registries.0.items;
        for ore in [&heavy, &rake] {
            let stack = items::write(ore, items).unwrap();
            assert_eq!(items::place_in(items, &mut shop.store.0, stack), None);
        }
        let listed = feeds(
            &registries.0,
            &shop.pack,
            Some(&shop.store),
            &print,
            "casing",
        );
        let read: Vec<(&Record, &Result<(), FeedRefusal>)> = listed
            .iter()
            .map(|feed| (&feed.kind, &feed.verdict))
            .collect();
        assert_eq!(
            read,
            [
                (&kind_of(&rake), &Ok(())),
                (&kind_of(&heavy), &Err(over.clone()))
            ]
        );
        // Run from it, the start names the casing and the number.
        let number = 7;
        shop.prints.0.insert(number, print);
        // Its item, as recording would have put it away.
        let item = Record {
            batch: number,
            ..shop.blueprint_item()
        };
        let stack = items::write(&item, items).unwrap();
        assert_eq!(items::place_in(items, &mut shop.store.0, stack), None);
        for part in [
            &core,
            &part(&registries, CORE_ITEM, 1, Some((1, &[600, 300]))),
        ] {
            let stack = items::write(part, items).unwrap();
            assert_eq!(items::place_in(items, &mut shop.store.0, stack), None);
        }
        let fed = |casing: &Record| {
            Feeds::from([
                ("core".to_owned(), vec![kind_of(&core)]),
                (
                    "harness".to_owned(),
                    vec![part(&registries, HARNESS, 1, None)],
                ),
                ("casing".to_owned(), vec![kind_of(casing)]),
                ("seals".to_owned(), vec![part(&registries, SEALS, 1, None)]),
            ])
        };
        let check = |shop: &Shop, feeds: &Feeds| {
            check_start(
                &registries.0,
                &shop.pack,
                Some(&shop.store),
                &shop.prints,
                &shop.state,
                number,
                1,
                feeds,
            )
        };
        assert_eq!(
            check(&shop, &fed(&heavy)),
            Err(FactoryRefusal::Slot {
                slot: "casing".to_owned(),
                why: over
            })
        );
        assert_eq!(check(&shop, &fed(&rake)), Ok(()));
        // The cheapest stock for it is the core that meets and no better,
        // the bought parts, and the ore that is light enough.
        let cheap = cheapest(
            &registries.0,
            &shop.pack,
            Some(&shop.store),
            &shop.prints.0[&number],
            1,
        );
        assert_eq!(cheap, fed(&rake));
    }

    /// Stock better than the prototype's is accepted and gains nothing:
    /// the units have the blueprint's lines, not what a bench would have
    /// made of it.
    #[test]
    fn better_stock_gains_nothing() {
        let mut shop = shop(
            |registries| {
                let mut lots = first(registries);
                lots.extend([calder(registries, 100), deepwell(registries, 100)]);
                lots
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (salt, brine) = (calder(&registries, 1), deepwell(&registries, 1));
        // At a bench the same stock has a ceiling of 760.
        let picks = Picks::from([
            ("salt".to_owned(), salt.clone()),
            ("brine".to_owned(), brine.clone()),
        ]);
        assert_eq!(
            crafting::ceiling(schematic(&registries, CORE), 0, &picks),
            Some(760)
        );
        let fed = feed(&[&salt], &[&brine]);
        assert_eq!(shop.start(2, &fed, None), Ok((5, 48)));
        let told = shop.ticks(960);
        assert!(matches!(
            told[..],
            [(
                960,
                FactoryNotice::Finished {
                    made: 2,
                    spent: false,
                    ..
                }
            )]
        ));
        let cores = shop.cores();
        assert_eq!(cores.len(), 1);
        assert_eq!((cores[0].count, cores[0].batch), (2, 5));
        assert_eq!(
            (
                cores[0].lines["charge_density"],
                cores[0].lines["stability"]
            ),
            (513, 209)
        );
        assert_eq!((shop.held(&salt), shop.held(&brine)), (60, 40));
        // The ledger says they were made of the better stock all the same.
        assert_eq!(shop.craft.made[&5].from[0].1.lines["potency"], 820);
    }

    /// Recorded at 85 %, the blueprint asks for 85 % of each figure, so
    /// stock the whole one refuses feeds it, and it makes 85 % of each
    /// line: by the arithmetic, a half rounding up.
    #[test]
    fn a_derated_blueprint_takes_poorer_stock_and_makes_less() {
        let lots = |registries: &Registries| {
            let mut lots = first(registries);
            lots.extend([
                lot(registries, SALT, "Fen", 100, &[400, 560, 200]),
                lot(registries, BRINE, "Shoal", 100, &[460, 520, 430]),
            ]);
            lots
        };
        let whole = shop(lots, 1000);
        let mut derated = shop(lots, 850);
        let registries = derated.registries.clone();
        let fen = kind_of(&lot(&registries, SALT, "Fen", 1, &[400, 560, 200]));
        let shoal = kind_of(&lot(&registries, BRINE, "Shoal", 1, &[460, 520, 430]));
        let fed = feed(&[&fen], &[&shoal]);
        assert_eq!(
            whole.check(1, &fed),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Under {
                    stat: "potency".to_owned(),
                    has: 560,
                    needs: 632
                }
            })
        );
        assert_eq!(
            meets(&whole.print().slots[1], &shoal),
            Err(FeedRefusal::Under {
                stat: "potency".to_owned(),
                has: 520,
                needs: 605
            })
        );
        // Potency 537 and stability 191; potency 514, conductivity 421
        // and purity 454.
        assert_eq!(derated.check(1, &fed), Ok(()));
        let listed = derated.feeds("salt");
        assert_eq!(listed[0].kind, fen, "the one that clears it by least");
        assert_eq!(listed[0].margin, (560 - 537) + (200 - 191));
        assert_eq!(derated.start(2, &fed, None), Ok((5, 48)));
        derated.ticks(960);
        let cores = derated.cores();
        assert_eq!(cores.len(), 1);
        assert_eq!(
            (
                cores[0].count,
                cores[0].lines["charge_density"],
                cores[0].lines["stability"]
            ),
            (2, 436, 178)
        );
        assert_eq!(derated.print().lines, cores[0].lines);
        assert!(cores[0].lines["charge_density"] < whole.print().lines["charge_density"]);
    }

    /// A slot is fed from several stacks when each meets the minimum: they
    /// are spent in the order given, a unit may span two, and one too
    /// small on its own is short with what it has.
    #[test]
    fn a_slot_is_fed_from_several_stacks() {
        let mut shop = shop(
            |registries| {
                vec![
                    tolley(registries, 50),
                    oxbow(registries, 160),
                    calder(registries, 100),
                ]
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (own, better, brine) = (
            tolley(&registries, 1),
            calder(&registries, 1),
            oxbow(&registries, 1),
        );
        assert_eq!(shop.held(&own), 30);
        assert_eq!(
            shop.check(3, &feed(&[&own], &[&brine])),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Short {
                    have: 30,
                    needs: 60
                }
            })
        );
        // Named twice, a kind is still counted once.
        assert!(shop.check(3, &feed(&[&own, &own], &[&brine])).is_err());
        let fed = feed(&[&own, &better], &[&brine]);
        assert_eq!(shop.cheapest(3), fed, "which is also the cheapest");
        assert_eq!(shop.start(3, &fed, None), Ok((5, 72)));
        let hopper = |shop: &Shop| -> Vec<(String, String, u32)> {
            shop.job()
                .inputs
                .iter()
                .map(|input| {
                    let root = input.kind.origin.as_ref().unwrap().root.clone();
                    (input.slot.clone(), root, input.left)
                })
                .collect()
        };
        let row = |slot: &str, root: &str, left: u32| (slot.to_owned(), root.to_owned(), left);
        assert_eq!(
            hopper(&shop),
            [
                row("salt", "Tolley", 30),
                row("salt", "Calder", 30),
                row("brine", "Oxbow", 90)
            ]
        );
        assert_eq!(
            (shop.held(&own), shop.held(&better), shop.held(&brine)),
            (0, 70, 40)
        );
        shop.ticks(480);
        assert_eq!(
            hopper(&shop),
            [
                row("salt", "Tolley", 10),
                row("salt", "Calder", 30),
                row("brine", "Oxbow", 60)
            ]
        );
        // The second unit takes the last ten of one lot and ten of the
        // next.
        shop.ticks(480);
        assert_eq!(
            hopper(&shop),
            [row("salt", "Calder", 20), row("brine", "Oxbow", 30)]
        );
        let told = shop.ticks(480);
        assert!(matches!(
            told[..],
            [(480, FactoryNotice::Finished { made: 3, .. })]
        ));
        assert_eq!(shop.cores()[0].count, 3, "and all three are one stack");
        // The ledger names both salts.
        let from: Vec<(&str, &str, u32)> = shop.craft.made[&5]
            .from
            .iter()
            .map(|(slot, record)| {
                let root = record.origin.as_ref().unwrap().root.as_str();
                (slot.as_str(), root, record.count)
            })
            .collect();
        assert_eq!(
            from,
            [
                ("salt", "Tolley", 20),
                ("salt", "Calder", 20),
                ("brine", "Oxbow", 30)
            ]
        );
        assert_eq!(shop.craft.check(&registries.0), Ok(()));
    }

    /// A start commits the blueprint and every input for the whole batch
    /// at once, or nothing: a batch one slot cannot cover takes none of
    /// the other's stock. And a run that no time has passed for has made
    /// nothing.
    #[test]
    fn a_start_takes_everything_or_nothing() {
        let mut shop = shop(first, 1000);
        let registries = shop.registries.clone();
        let (salt, brine) = (tolley(&registries, 1), oxbow(&registries, 1));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (140, 130));
        let fed = feed(&[&salt], &[&brine]);
        let before = shop.everything();
        // Five take 100 salt, which there is, and 150 brine, which there
        // is not.
        assert_eq!(
            shop.start(5, &fed, None),
            Err(FactoryRefusal::Slot {
                slot: "brine".to_owned(),
                why: FeedRefusal::Short {
                    have: 130,
                    needs: 150
                }
            })
        );
        assert_eq!(shop.everything(), before, "the salt was not taken either");
        assert_eq!(
            blueprint::held(&registries.0, &shop.pack, Some(&shop.store), &shop.prints),
            [(4, false)]
        );

        assert_eq!(shop.start(4, &fed, None), Ok((5, 96)));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (60, 10));
        let held = blueprint::held(&registries.0, &shop.pack, Some(&shop.store), &shop.prints);
        assert!(held.is_empty(), "the fabricator holds the blueprint");
        let job = shop.job().clone();
        assert_eq!((job.blueprint, job.batch, job.size, job.made), (4, 5, 4, 0));
        assert_eq!((job.worker.clone(), job.paused), (None, None));
        assert_eq!(
            (
                job.item.item.clone(),
                job.item.batch,
                job.item.by.as_deref()
            ),
            (id(BLUEPRINT), 4, Some(MAKER))
        );
        let committed: Vec<(&str, u32)> = job
            .inputs
            .iter()
            .map(|input| (input.slot.as_str(), input.left))
            .collect();
        assert_eq!(committed, [("salt", 80), ("brine", 120)]);
        assert_eq!(shop.craft.batches, 5);
        // No time has passed: nothing is made, and no run is spent.
        assert!(shop.cores().is_empty());
        assert_eq!(shop.print().runs_left, 100);
        assert_eq!(shop.clock, FactoryClock(0));

        // The run is in the save's shape and reads back as it was, and a
        // file from before it has none.
        shop.ticks(700);
        assert_eq!(shop.job().made, 1);
        let text =
            ron::ser::to_string_pretty(&shop.state, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<FactoryState>(&text).unwrap(), shop.state);
        assert_eq!(
            ron::from_str::<FactoryState>("()").unwrap(),
            FactoryState::default()
        );
    }

    /// One press fills the slots with the cheapest stock that meets the
    /// blueprint: what clears the minimums by least first, more kinds only
    /// when the first runs out, and never a lot that does not meet.
    #[test]
    fn cheapest_fills_with_the_least_that_meets() {
        let shop = shop(
            |registries| {
                vec![
                    tolley(registries, 60),
                    oxbow(registries, 160),
                    calder(registries, 100),
                    marsh(registries, 100),
                    deepwell(registries, 60),
                ]
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (own, better, poor) = (
            tolley(&registries, 1),
            calder(&registries, 1),
            marsh(&registries, 1),
        );
        let (brine, deep) = (oxbow(&registries, 1), deepwell(&registries, 1));
        let listed = shop.feeds("salt");
        let order: Vec<(&Record, u32, bool)> = listed
            .iter()
            .map(|feed| (&feed.kind, feed.margin, feed.verdict.is_ok()))
            .collect();
        assert_eq!(
            order,
            [
                (&own, 0, true),
                (&better, (820 - 632) + (450 - 225), true),
                (&poor, 75, false)
            ]
        );
        let brines: Vec<Record> = shop
            .feeds("brine")
            .into_iter()
            .map(|feed| feed.kind)
            .collect();
        assert_eq!(brines, [brine.clone(), deep.clone()]);
        assert!(shop.feeds("muzzle").is_empty(), "no such slot");

        // Forty salt is all the prototype's own lot has left.
        assert_eq!(shop.cheapest(2), feed(&[&own], &[&brine]));
        assert_eq!(shop.cheapest(3), feed(&[&own, &better], &[&brine]));
        assert_eq!(shop.cheapest(5), feed(&[&own, &better], &[&brine, &deep]));
        for batch in [1, 2, 3, 5] {
            assert_eq!(shop.check(batch, &shop.cheapest(batch)), Ok(()), "{batch}");
        }
        // Past what the stock feeds, the press fills what it can and the
        // check says which slot is short.
        let most = shop.cheapest(7);
        assert_eq!(most, feed(&[&own, &better], &[&brine, &deep]));
        assert_eq!(
            shop.check(7, &most),
            Err(FactoryRefusal::Slot {
                slot: "brine".to_owned(),
                why: FeedRefusal::Short {
                    have: 190,
                    needs: 210
                }
            })
        );
        // With nothing that meets a slot, the press leaves it empty.
        let mut bare = shop;
        let items = &registries.0.items;
        for cell in 0..bare.store.0.len() {
            let salt = bare.store.0.get(cell).is_some_and(|stack| {
                let record = items::read(stack, items).unwrap();
                [&own, &better].contains(&&kind_of(&record))
            });
            if salt {
                bare.free(cell);
            }
        }
        let fed = bare.cheapest(1);
        assert_eq!(fed.get("salt"), None);
        assert_eq!(
            bare.check(1, &fed),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Empty
            })
        );
    }

    /// The batch cannot exceed what the stock feeds or the runs left, and
    /// the limit says which holds it; a start over either is refused with
    /// the number.
    #[test]
    fn the_batch_is_held_by_runs_then_by_stock() {
        let mut shop = shop(first, 1000);
        let registries = shop.registries.clone();
        let (salt, brine) = (tolley(&registries, 1), oxbow(&registries, 1));
        // 140 salt feed seven and 130 brine four.
        let stock = |slot: &str, feeds: u32| Held::Stock {
            slot: slot.to_owned(),
            feeds,
        };
        assert_eq!(
            shop.limit(),
            Limit {
                most: 4,
                held_by: stock("brine", 4)
            }
        );
        let items = &registries.0.items;
        let more = items::write(&deepwell(&registries, 200), items).unwrap();
        assert_eq!(items::place_in(items, &mut shop.store.0, more), None);
        assert_eq!(
            shop.limit(),
            Limit {
                most: 7,
                held_by: stock("salt", 7)
            }
        );
        shop.prints.0.get_mut(&4).unwrap().runs_left = 3;
        assert_eq!(
            shop.limit(),
            Limit {
                most: 3,
                held_by: Held::Runs { left: 3 }
            }
        );
        let fed = feed(&[&salt], &[&brine]);
        let before = shop.everything();
        assert_eq!(
            shop.start(4, &fed, None),
            Err(FactoryRefusal::OverRuns { left: 3 })
        );
        assert_eq!(shop.start(0, &fed, None), Err(FactoryRefusal::NoBatch));
        assert_eq!(shop.everything(), before);
        // The runs left and the stock at once: the runs are named.
        shop.prints.0.get_mut(&4).unwrap().runs_left = 7;
        assert_eq!(shop.limit().held_by, Held::Runs { left: 7 });
        shop.prints.0.get_mut(&4).unwrap().runs_left = 0;
        assert_eq!(
            shop.limit(),
            Limit {
                most: 0,
                held_by: Held::Runs { left: 0 }
            }
        );
        // A blueprint that is nowhere in reach, and one the ledger has not
        // got.
        assert_eq!(
            check_start(
                &registries.0,
                &shop.pack,
                Some(&shop.store),
                &shop.prints,
                &shop.state,
                9,
                1,
                &fed
            ),
            Err(FactoryRefusal::NoBlueprint)
        );
        let empty = Pack::empty(items.carry());
        assert_eq!(
            check_start(
                &registries.0,
                &empty,
                Some(&shop.store),
                &shop.prints,
                &shop.state,
                4,
                1,
                &fed
            ),
            Err(FactoryRefusal::NoBlueprint),
            "its item is in the pack that is not here"
        );
        // One that no longer makes what the data has is refused as stale.
        let mut stale = shop.prints.clone();
        let print = stale.0.get_mut(&4).unwrap();
        print.runs_left = 5;
        print.lines.remove("stability");
        assert_eq!(
            check_start(
                &registries.0,
                &shop.pack,
                Some(&shop.store),
                &stale,
                &shop.state,
                4,
                1,
                &fed
            ),
            Err(FactoryRefusal::Stale)
        );
    }

    /// The dial: a unit of complexity 1 is twelve played seconds with a
    /// worker and twenty-four with nobody, so three cores take 36 s and
    /// 72 s, twenty four minutes and eight, and the ticks a run takes are
    /// exactly those.
    #[test]
    fn three_take_36_and_72_seconds_and_twenty_four_minutes() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        assert_eq!(
            (seconds(&rules, 1, 3, true), seconds(&rules, 1, 3, false)),
            (36, 72)
        );
        assert_eq!(
            (seconds(&rules, 1, 20, true), seconds(&rules, 1, 20, false)),
            (240, 480)
        );
        assert_eq!(
            (
                seconds(&rules, 1, 100, true),
                seconds(&rules, 1, 100, false)
            ),
            (1200, 2400)
        );
        assert_eq!(
            (seconds(&rules, 2, 3, true), seconds(&rules, 2, 3, false)),
            (72, 144),
            "a magazine is twice a core"
        );
        assert_eq!(seconds(&rules, 1, 0, false), 0);

        let fed = shop.cheapest(3);
        assert_eq!(shop.start(3, &fed, None), Ok((5, 72)));
        let left = |shop: &Shop| left(&rules, shop.print(), shop.job(), &shop.clock);
        assert_eq!(left(&shop), 72);
        // Half a unit in: twelve seconds of it done.
        assert_eq!(shop.ticks(240), []);
        assert_eq!((shop.job().made, left(&shop)), (0, 60));
        assert_eq!(shop.ticks(239), []);
        assert_eq!(
            (shop.job().made, left(&shop)),
            (0, 49),
            "a tick short, rounded up"
        );
        assert_eq!(shop.tick(), None);
        assert_eq!((shop.job().made, left(&shop)), (1, 48));
        let told = shop.ticks(960);
        assert!(
            matches!(told[..], [(960, FactoryNotice::Finished { made: 3, .. })]),
            "1,440 ticks at twenty a second: {told:?}"
        );

        // With somebody on it, half that.
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, Some(WORKER)), Ok((6, 12)));
        assert_eq!(left(&shop), 12);
        let told = shop.ticks(240);
        assert!(matches!(
            told[..],
            [(
                240,
                FactoryNotice::Finished {
                    made: 1,
                    batch: 6,
                    ..
                }
            )]
        ));
    }

    /// A share of the worked rate that does not divide a unit (0.7 of it:
    /// 342.86 ticks a core) loses nothing between units: what the clock
    /// has over a unit goes toward the next, so seven cores take the 2,400
    /// ticks [`seconds`] and [`left`] say, where a clock begun again at
    /// each unit would take 343 a core and 2,401.
    #[test]
    fn a_share_that_does_not_divide_a_unit_loses_nothing() {
        let mut shop = shop(
            |registries| {
                vec![
                    tolley(registries, 200),
                    oxbow(registries, 200),
                    oxbow(registries, 60),
                ]
            },
            1000,
        );
        let assets = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/crafting/factory.ron"
        );
        let text = std::fs::read_to_string(assets).unwrap();
        assert!(text.contains("unattended: 0.5"), "the dial this test turns");
        let dials = FactoryRules::parse(&text.replace("unattended: 0.5", "unattended: 0.7"));
        let mut turned = (*shop.registries.0).clone();
        turned.schematics = turned
            .schematics
            .with_factory(dials.unwrap(), &turned.items)
            .unwrap();
        shop.registries = Registries(std::sync::Arc::new(turned));
        let rules = shop.rules().clone();
        assert_eq!(rules.rate_pm(false), 700);
        // 240,000 a unit at 700 a tick: 342.86 ticks, 17.14 s.
        assert_eq!(seconds(&rules, 1, 1, false), 18);
        assert_eq!(seconds(&rules, 1, 7, false), 120);
        assert_eq!(seconds(&rules, 1, 100, false), 1715);

        let fed = shop.cheapest(7);
        assert_eq!(shop.start(7, &fed, None), Ok((5, 120)));
        assert_eq!(shop.ticks(342), []);
        shop.tick();
        assert_eq!((shop.job().made, shop.clock), (1, FactoryClock(100)));
        assert_eq!(
            left(&rules, shop.print(), shop.job(), &shop.clock),
            103,
            "2,057 ticks: what was over is counted"
        );
        // The second starts a hundred up: 2 x 240,000 is passed at tick
        // 686, and by the seventh what is over has saved a whole tick.
        assert_eq!(shop.ticks(342), []);
        shop.tick();
        assert_eq!((shop.job().made, shop.clock), (2, FactoryClock(200)));
        let told = shop.ticks(2400 - 686);
        assert!(
            matches!(told[..], [(1714, FactoryNotice::Finished { made: 7, .. })]),
            "2,400 ticks in all: {told:?}"
        );
        assert_eq!(shop.clock, FactoryClock(0));
    }

    /// With no room in the store for a unit the run pauses and says so
    /// once, makes nothing and loses nothing, and goes on when there is
    /// room; the blueprint coming back at the end waits for room the same
    /// way.
    #[test]
    fn a_full_store_pauses_and_room_resumes() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        let fed = shop.cheapest(2);
        assert_eq!(shop.start(2, &fed, None), Ok((5, 48)));
        let free = shop.crowd();
        assert!(free.len() > 2);
        let before = (shop.store.clone(), shop.prints.clone());
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.tick(), Some(FactoryNotice::Paused(Pause::NoRoom)));
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (0, Some(Pause::NoRoom))
        );
        // Standing: told once, nothing made, nothing spent, no time lost.
        let hopper = shop.job().inputs.clone();
        assert_eq!(shop.ticks(1000), []);
        assert_eq!((shop.store.clone(), shop.prints.clone()), before);
        assert_eq!(shop.job().inputs, hopper);
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 24);
        assert!(
            can_stop(
                &shop.registries.0.items,
                &shop.prints,
                shop.job(),
                &shop.store
            )
            .is_err()
        );

        // A cell freed: the unit that was waiting is made on the next
        // tick.
        shop.free(free[0]);
        assert_eq!(shop.tick(), Some(FactoryNotice::Resumed));
        assert_eq!((shop.job().made, shop.job().paused), (1, None));
        assert_eq!(shop.cores()[0].count, 1);
        assert_eq!(shop.print().runs_left, 99);
        // The second merges onto the first, and then the blueprint has no
        // cell to come back to.
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.tick(), Some(FactoryNotice::Paused(Pause::NoRoom)));
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (2, Some(Pause::NoRoom))
        );
        assert_eq!(shop.cores()[0].count, 2);
        assert_eq!(shop.ticks(100), []);
        shop.free(free[1]);
        assert_eq!(
            shop.tick(),
            Some(FactoryNotice::Finished {
                item: id(CORE_ITEM),
                made: 2,
                batch: 5,
                spent: false
            })
        );
        assert_eq!(shop.state.job, None);
        assert_eq!(
            blueprint::held(
                &shop.registries.0,
                &shop.pack,
                Some(&shop.store),
                &shop.prints
            ),
            [(4, true)]
        );
        assert_eq!(shop.print().runs_left, 98);
    }

    /// A pause is saved and the clock is not. A run read back standing,
    /// with the clock a start has, still has the unit that was waiting: it
    /// says nothing again while the store is full, makes the unit on the
    /// first tick with room, says it goes on, and takes a whole unit's
    /// time for the next. The blueprint waiting to come back at the end is
    /// handed over the same way.
    #[test]
    fn a_paused_run_read_back_from_a_save_goes_on_when_there_is_room() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        let fed = shop.cheapest(2);
        assert_eq!(shop.start(2, &fed, None), Ok((5, 48)));
        let free = shop.crowd();
        shop.ticks(480);
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (0, Some(Pause::NoRoom))
        );
        let reload = |shop: &mut Shop| {
            let saved = ron::to_string(&shop.state).unwrap();
            shop.state = ron::from_str(&saved).unwrap();
            shop.clock = FactoryClock::default();
        };
        reload(&mut shop);
        assert_eq!(shop.job().paused, Some(Pause::NoRoom));
        assert_eq!(
            left(&rules, shop.print(), shop.job(), &shop.clock),
            24,
            "the unit that waits is not counted again"
        );
        assert_eq!(shop.ticks(100), [], "still full: it stands, told once");
        assert_eq!(shop.job().made, 0);

        shop.free(free[0]);
        assert_eq!(shop.tick(), Some(FactoryNotice::Resumed));
        assert_eq!((shop.job().made, shop.job().paused), (1, None));
        assert_eq!(shop.cores()[0].count, 1);
        assert_eq!(shop.print().runs_left, 99);
        assert_eq!(shop.clock, FactoryClock(0));
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 24);

        // The second merges onto the first and the blueprint has no cell:
        // standing again, read back again, and handed over when there is
        // one.
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.tick(), Some(FactoryNotice::Paused(Pause::NoRoom)));
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (2, Some(Pause::NoRoom))
        );
        reload(&mut shop);
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 0);
        assert_eq!(shop.ticks(10), []);
        shop.free(free[1]);
        assert_eq!(
            shop.tick(),
            Some(FactoryNotice::Finished {
                item: id(CORE_ITEM),
                made: 2,
                batch: 5,
                spent: false
            })
        );
        assert_eq!(shop.cores()[0].count, 2);
        assert_eq!(shop.state.job, None);
    }

    /// Stop leaves what is made, gives back exactly what was not spent and
    /// the blueprint with the runs it has left; with no room for them it
    /// is refused and the run goes on.
    #[test]
    fn stop_returns_the_unspent_or_refuses_for_room() {
        let mut shop = shop(first, 1000);
        let registries = shop.registries.clone();
        let (salt, brine) = (tolley(&registries, 1), oxbow(&registries, 1));
        assert_eq!(shop.stop(), Err(FactoryRefusal::NoJob));
        let fed = feed(&[&salt], &[&brine]);
        assert_eq!(shop.start(4, &fed, None), Ok((5, 96)));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (60, 10));
        shop.ticks(600);
        assert_eq!(shop.job().made, 1);
        assert_eq!(
            can_stop(&registries.0.items, &shop.prints, shop.job(), &shop.store),
            Ok(())
        );
        assert_eq!(shop.stop(), Ok(1));
        // Three units' worth of each came back: 60 and 90.
        assert_eq!((shop.held(&salt), shop.held(&brine)), (120, 100));
        assert_eq!(shop.cores()[0].count, 1, "what was made stays");
        assert_eq!(shop.print().runs_left, 99);
        assert_eq!(
            blueprint::held(&registries.0, &shop.pack, Some(&shop.store), &shop.prints),
            [(4, true)],
            "the blueprint is in the store"
        );
        assert_eq!(
            (shop.state.job.clone(), shop.clock),
            (None, FactoryClock(0))
        );
        assert_eq!(shop.tick(), None, "and nothing more is made");

        // It runs again from what came back, under the next number.
        assert_eq!(shop.start(2, &fed, None), Ok((6, 48)));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (80, 40));
        // With the store full the inputs would merge onto their lots, and
        // the blueprint has no cell: nothing moves, and the run goes on.
        shop.crowd();
        let before = shop.everything();
        assert_eq!(
            can_stop(&registries.0.items, &shop.prints, shop.job(), &shop.store),
            Err(FactoryRefusal::NoRoom)
        );
        assert_eq!(shop.stop(), Err(FactoryRefusal::NoRoom));
        assert_eq!(shop.everything(), before);
        assert_eq!(shop.job().size, 2);
    }

    /// A benched member put on a run works it at the full rate, and can be
    /// taken off and put back while it runs; nobody else can be named.
    #[test]
    fn a_worker_must_be_benched_and_doubles_the_rate() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        let fed = shop.cheapest(3);
        assert_eq!(
            assign(&mut shop.state, &shop.benched, Some(WORKER)),
            Err(FactoryRefusal::NoJob)
        );
        let before = shop.everything();
        assert_eq!(
            shop.start(3, &fed, Some("Frontier ranger")),
            Err(FactoryRefusal::NotOnBench),
            "whoever is out is not on the bench"
        );
        assert_eq!(shop.everything(), before);
        assert_eq!(shop.start(3, &fed, Some(WORKER)), Ok((5, 36)));
        assert_eq!(shop.job().worker.as_deref(), Some(WORKER));
        assert_eq!(shop.ticks(239), []);
        assert_eq!(shop.job().made, 0);
        shop.tick();
        assert_eq!(shop.job().made, 1, "a unit in 240 ticks");
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 24);

        // Taken off, it goes on at half the rate.
        assert_eq!(assign(&mut shop.state, &shop.benched, None), Ok(()));
        assert_eq!(shop.job().worker, None);
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 48);
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.job().made, 1);
        shop.tick();
        assert_eq!(shop.job().made, 2);
        assert_eq!(
            assign(&mut shop.state, &shop.benched, Some("A stranger")),
            Err(FactoryRefusal::NotOnBench)
        );
        assert_eq!(shop.job().worker, None);
        // Put back in the middle of a unit, the unit keeps what it had.
        shop.ticks(240);
        assert_eq!(assign(&mut shop.state, &shop.benched, Some(WORKER)), Ok(()));
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 6);
        let told = shop.ticks(120);
        assert!(matches!(
            told[..],
            [(120, FactoryNotice::Finished { made: 3, .. })]
        ));
    }

    /// A unit takes its played seconds whatever rate the sim is stepped
    /// at: the run counts nominal ticks, twenty a second, so a sim stepped
    /// at sixty ticks it every third step and one stepped at ten twice a
    /// step, with nothing lost between steps.
    #[test]
    fn a_step_is_counted_in_nominal_ticks() {
        for (stepped, seconds) in [(20, 3), (60, 3), (10, 3), (30, 2), (64, 5)] {
            let mut phase = 0;
            let ticked: u32 = (0..stepped * seconds)
                .map(|_| nominal_ticks(&mut phase, stepped))
                .sum();
            assert_eq!(ticked, 20 * seconds, "{seconds} s at {stepped} Hz");
        }
        let mut phase = 0;
        let sixty: Vec<u32> = (0..6).map(|_| nominal_ticks(&mut phase, 60)).collect();
        assert_eq!(sixty, [0, 0, 1, 0, 0, 1]);
        let mut phase = 0;
        assert_eq!(nominal_ticks(&mut phase, 20), 1);
        assert_eq!(nominal_ticks(&mut phase, 10), 2);
        assert_eq!(
            nominal_ticks(&mut phase, 0),
            20,
            "a rate of nought is held to one"
        );
    }

    /// One job a household: a start while a run is on is refused before
    /// anything else is asked.
    #[test]
    fn one_run_at_a_time() {
        let mut shop = shop(first, 1000);
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, None), Ok((5, 24)));
        let before = shop.everything();
        assert_eq!(shop.start(1, &fed, None), Err(FactoryRefusal::Busy));
        assert_eq!(shop.check(0, &Feeds::new()), Err(FactoryRefusal::Busy));
        assert_eq!(shop.everything(), before);
        shop.ticks(480);
        assert_eq!(shop.state.job, None);
        // The blueprint is in the store now, and runs again.
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, None), Ok((6, 24)));
    }

    /// A saved run whose blueprint the ledger has not got is dropped at
    /// the next start with what it held put in the store; one that stands
    /// is left as it is.
    #[test]
    fn a_run_whose_blueprint_is_gone_is_dropped() {
        let mut shop = shop(first, 1000);
        let registries = shop.registries.clone();
        let (salt, brine) = (tolley(&registries, 1), oxbow(&registries, 1));
        let fed = shop.cheapest(4);
        shop.start(4, &fed, None).unwrap();
        shop.ticks(480);
        let sound = shop.state.clone();
        assert_eq!(
            shop.state
                .repair(&registries.0, &shop.prints, &mut shop.store),
            Vec::<String>::new()
        );
        assert_eq!(shop.state, sound);
        assert_eq!((shop.held(&salt), shop.held(&brine)), (60, 10));

        let mut over = sound.clone();
        over.job.as_mut().unwrap().made = 9;
        let said = over.repair(&registries.0, &shop.prints, &mut shop.store);
        assert_eq!(over.job.as_ref().unwrap().made, 4);
        assert!(said[0].contains("had made 9 of 4"), "{said:?}");

        let said = shop
            .state
            .repair(&registries.0, &Blueprints::default(), &mut shop.store);
        assert_eq!(shop.state.job, None);
        assert_eq!(said.len(), 1);
        assert!(
            said[0].contains("blueprint 4") && said[0].contains("is not in the ledger"),
            "{said:?}"
        );
        // Three units' worth is back; the blueprint's item, which is a
        // blueprint of nothing now, is not.
        assert_eq!((shop.held(&salt), shop.held(&brine)), (120, 100));
        let held = blueprint::held(&registries.0, &shop.pack, Some(&shop.store), &shop.prints);
        assert!(held.is_empty());
    }

    fn ask_factory(app: &mut App, request: FactoryRequest) -> Result<FactoryDone, FactoryRefusal> {
        app.world_mut()
            .resource_mut::<FactoryInbox>()
            .0
            .push(request.clone());
        app.update();
        let mut answers: Vec<FactoryAnswer> = app
            .world_mut()
            .resource_mut::<Messages<FactoryAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        let answer = answers.pop().unwrap();
        assert_eq!(answer.request, request);
        answer.outcome
    }

    fn told(app: &mut App) -> Vec<FactoryNotice> {
        app.world_mut()
            .resource_mut::<Messages<FactoryNotice>>()
            .drain()
            .collect()
    }

    /// A blueprint is recorded and run by anyone: the box a schematic needs
    /// is asked when a session opens, and neither a recording nor a run
    /// opens one.
    #[test]
    fn a_blueprint_run_needs_no_box() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        crate::crafting::tests::assemble_best(&mut app, "veyra:schematic.patrol_carbine").unwrap();
        work_and_finish(&mut app);
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = CharacterSheet::default();
        }
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        assert!(
            matches!(recorded, Ok(CraftDone::Recorded { .. })),
            "{recorded:?}"
        );
        app.world_mut().despawn(bench);
        prop(&mut app, InteractionKind::Fabricator);
        let print = app.world().resource::<Blueprints>().0[&2].clone();
        // A weapon's unit is whole, or the catalogue refuses it and the run
        // is Stale for ever.
        assert!(print.stands(&registries.0), "a recorded carbine stands");
        assert_eq!(
            print.unit(1, &registries.0.items).condition,
            Some(registries.0.items.wear().full)
        );
        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 1);
        let started = ask_factory(
            &mut app,
            FactoryRequest::Start {
                blueprint: 2,
                batch: 1,
                feeds: fed,
                worker: None,
            },
        );
        assert!(
            matches!(started, Ok(FactoryDone::Started { .. })),
            "{started:?}"
        );
    }

    /// Through the tick: a core is made and recorded at a workbench, and a
    /// run is started, worked and stopped only with a fabricator in reach;
    /// the run itself goes on wherever the player is, and says when it is
    /// done.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one household, from the bench to the fabricator and away from it"
    )]
    fn away_from_a_fabricator_is_refused() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        assert_eq!(
            crate::crafting::tests::assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let core = work_and_finish(&mut app);
        assert_eq!(core.batch, 1);
        // Recorded by request, with the workbench in reach.
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        let Ok(CraftDone::Recorded {
            blueprint,
            runs,
            item,
        }) = recorded
        else {
            panic!("{recorded:?}");
        };
        assert_eq!((blueprint, runs), (2, 100));
        assert_eq!((item.item.clone(), item.batch), (id(BLUEPRINT), 2));
        assert_eq!(
            ask(
                &mut app,
                CraftRequest::Record {
                    batch: 1,
                    derate: 850
                }
            ),
            Err(crate::crafting::CraftRefusal::Recorded { blueprint: 2 })
        );
        let print = app.world().resource::<Blueprints>().0[&2].clone();
        assert_eq!(print.lines, core.lines);
        assert_eq!(print.by, MAKER);

        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 2);
        let start = |worker: Option<&str>| FactoryRequest::Start {
            blueprint: 2,
            batch: 2,
            feeds: fed.clone(),
            worker: worker.map(str::to_owned),
        };
        let stock = |app: &mut App| {
            let (pack, _, store, _, craft) = truth(app);
            (
                pack,
                store,
                craft,
                app.world().resource::<FactoryState>().clone(),
            )
        };
        // A workbench is no fabricator, and neither is the open.
        let before = stock(&mut app);
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Err(FactoryRefusal::NoFabricator)
        );
        app.world_mut().despawn(bench);
        app.update();
        for request in [
            start(None),
            FactoryRequest::Stop,
            FactoryRequest::Assign { worker: None },
        ] {
            assert_eq!(
                ask_factory(&mut app, request),
                Err(FactoryRefusal::NoFabricator)
            );
        }
        assert_eq!(stock(&mut app), before, "nothing moved");

        // At one, a worker who is not on the bench is refused and then the
        // run starts with nobody.
        let fabricator = prop(&mut app, InteractionKind::Fabricator);
        assert!(app.world().resource::<Reach>().fabricator);
        assert_eq!(
            ask_factory(&mut app, start(Some(WORKER))),
            Err(FactoryRefusal::NotOnBench)
        );
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Ok(FactoryDone::Started {
                batch: 3,
                seconds: 48
            })
        );
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Err(FactoryRefusal::Busy)
        );
        app.insert_resource(Benched(vec![WORKER.to_owned()]));
        assert_eq!(
            ask_factory(
                &mut app,
                FactoryRequest::Assign {
                    worker: Some(WORKER.to_owned())
                }
            ),
            Ok(FactoryDone::Assigned)
        );
        let job = app.world().resource::<FactoryState>().job.clone().unwrap();
        assert_eq!(
            (job.batch, job.size, job.worker.as_deref()),
            (3, 2, Some(WORKER))
        );
        assert_eq!(
            app.world().resource::<CraftState>().made[&3].blueprint,
            Some(2)
        );

        // Walked away from: it cannot be stopped from the open, and it
        // goes on all the same.
        app.world_mut().despawn(fabricator);
        app.update();
        assert_eq!(
            ask_factory(&mut app, FactoryRequest::Stop),
            Err(FactoryRefusal::NoFabricator)
        );
        assert!(told(&mut app).is_empty());
        let mut finished = Vec::new();
        for _ in 0..480 {
            app.update();
            finished.extend(told(&mut app));
            if !finished.is_empty() {
                break;
            }
        }
        assert_eq!(
            finished,
            [FactoryNotice::Finished {
                item: id(CORE_ITEM),
                made: 2,
                batch: 3,
                spent: false
            }]
        );
        let (pack, _, store, ..) = truth(&mut app);
        let items = &registries.0.items;
        let made = Record {
            count: 2,
            ..print.unit(3, items)
        };
        assert_eq!(units(&store.0, &made, items), 2);
        assert_eq!(made.lines, core.lines, "the prototype's lines, exactly");
        let prints = app.world().resource::<Blueprints>().clone();
        assert_eq!(prints.0[&2].runs_left, 98);
        assert_eq!(
            blueprint::held(&registries.0, &pack, Some(&store), &prints),
            [(2, true)]
        );
        assert_eq!(app.world().resource::<FactoryState>().job, None);

        // A run that lifts the rule (a still of the screen) works
        // anywhere, and with nobody to work it, nobody does.
        app.insert_resource(WorkbenchAccess::Anywhere);
        let fed = cheapest(&registries.0, &pack, Some(&store), &prints.0[&2], 1);
        let again = FactoryRequest::Start {
            blueprint: 2,
            batch: 1,
            feeds: fed,
            worker: None,
        };
        assert_eq!(
            ask_factory(&mut app, again.clone()),
            Ok(FactoryDone::Started {
                batch: 4,
                seconds: 24
            })
        );
        assert_eq!(
            ask_factory(&mut app, FactoryRequest::Stop),
            Ok(FactoryDone::Stopped { made: 0 })
        );
        let mut empty = app_in_world();
        empty.insert_resource(shipped());
        assert_eq!(
            ask_factory(&mut empty, again),
            Err(FactoryRefusal::NoPlayer)
        );
        // The props of the test are gone; nothing else was spawned.
        let mut props = app.world_mut().query::<&Interaction>();
        assert_eq!(props.iter(app.world()).count(), 0);
    }

    /// A run is kept at the pace of whoever started it: the sheet's Fixtures
    /// quicken a unit and the unattended share, the figures ride with the job
    /// through a save and a change of who is at the bench, and a job saved
    /// before the boxes had effects reads the dials.
    #[test]
    fn a_run_keeps_its_starters_pace_across_a_switch_and_a_save() {
        let mut shop = shop(first, 1000);
        let full = crate::crafting::tests::whole_tree(&shop.registries, "fabricator");
        let pace = pace_of(shop.rules(), &shop.registries.0.progression, &full);
        assert_eq!(
            pace,
            Some(Pace {
                ticks_per_point: 138,
                unattended_pm: 650
            })
        );
        let fed = shop.cheapest(3);
        // Three units of 6.9 s at 0.65 with nobody on it: 414 / 0.65 ticks.
        assert_eq!(
            shop.start_paced(3, &fed, (None, pace)),
            Ok((5, 32)),
            "636.9 ticks of twenty a second"
        );
        let rules = shop.rules().clone();
        let left = |shop: &Shop| left(&rules, shop.print(), shop.job(), &shop.clock);
        assert_eq!(left(&shop), 32);
        // A save, a bench of another member and a retuned sheet change
        // nothing: the job alone says how fast it goes.
        let saved = ron::to_string(&shop.state).unwrap();
        shop.state = ron::from_str(&saved).unwrap();
        assert_eq!(shop.job().pace, pace);
        assert_eq!(left(&shop), 32);
        assert_eq!(shop.ticks(636), []);
        let told = shop.tick();
        assert!(
            matches!(told, Some(FactoryNotice::Finished { made: 3, .. })),
            "{told:?}"
        );
        // The dials as they are for a run that kept none, which is also what
        // an old save without the key reads.
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, None), Ok((6, 24)));
        let saved = ron::to_string(&shop.state).unwrap();
        let old = saved.replace(",pace:None", "");
        assert_ne!(old, saved, "the key is written");
        let read: FactoryState = ron::from_str(&old).unwrap();
        assert_eq!(read.job.unwrap().pace, None);
    }

    /// Through the tick: a blueprint recorded by a member with Fixtures 2 has
    /// a quarter more runs, a run started by one with Fixtures 1 and 3 is
    /// kept at their pace, and a switch to a sheet with no box afterwards
    /// leaves it.
    #[test]
    fn a_recording_and_a_start_read_the_sheet_of_the_member_at_the_bench() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        let tree = crate::crafting::tests::whole_tree(&registries, "fabricator");
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = tree;
        }
        assert_eq!(
            crate::crafting::tests::assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 6 })
        );
        work_and_finish(&mut app);
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        let Ok(CraftDone::Recorded {
            runs, blueprint, ..
        }) = recorded
        else {
            panic!("{recorded:?}");
        };
        assert_eq!(runs, 125, "a hundred and a quarter");
        assert_eq!(app.world().resource::<Blueprints>().0[&blueprint].runs, 125);
        app.world_mut().despawn(bench);
        prop(&mut app, InteractionKind::Fabricator);
        let print = app.world().resource::<Blueprints>().0[&blueprint].clone();
        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 2);
        let started = ask_factory(
            &mut app,
            FactoryRequest::Start {
                blueprint,
                batch: 2,
                feeds: fed,
                worker: None,
            },
        );
        // Two units of 6.9 s with nobody on it, 0.65 of the rate.
        assert_eq!(
            started,
            Ok(FactoryDone::Started {
                batch: 3,
                seconds: 22
            })
        );
        let kept = Some(Pace {
            ticks_per_point: 138,
            unattended_pm: 650,
        });
        assert_eq!(
            app.world()
                .resource::<FactoryState>()
                .job
                .as_ref()
                .unwrap()
                .pace,
            kept
        );
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = CharacterSheet::default();
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<FactoryState>()
                .job
                .as_ref()
                .unwrap()
                .pace,
            kept
        );
    }
}
