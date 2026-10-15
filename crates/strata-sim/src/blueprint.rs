//! Production blueprints: the recording of one finished prototype, which a
//! fabricator copies (ticket 0007, step 5; `docs/design/crafting.md`, 5.4).
//!
//! A blueprint copies exactly and asks for a minimum. Recorded at a
//! **derating** (a share of the prototype, one of the dial's steps), its
//! lines are the prototype's times the derating, and each slot records what
//! the schematic's slot takes and, for every stat the schematic weighs
//! there, a requirement read off what the prototype was made from: the
//! input's figure times the derating as a floor, or, where the schematic
//! reads the stat as `Light`, a ceiling eased the other way, so that the
//! requirement is always the easier one. Better stock is accepted and gains
//! nothing. All of it is whole per-mille, as the bench's own arithmetic is:
//!
//! ```text
//! line or floor = (figure * derate_pm + 500) / 1000
//! ceiling       = (1_000_000 - (1000 - figure) * derate_pm + 500) / 1000
//! ```
//!
//! A blueprint is an item, a stack of one of the item the factory's dials
//! name, which is stored and carried like any other. Its body is here, in
//! the household's ledger ([`Blueprints`]) under the stack's batch number,
//! which is the blueprint's number: the patch gains no key, so
//! `crate::items::{read, write}` stay the only reader and writer of one, and
//! the game saves the ledger whole beside the bench's work. A blueprint is
//! finite: it starts with the runs the dials give its schematic's
//! complexity, and a prototype is recorded once.
//!
//! [`draft`] is what a screen shows before Record is pressed and what
//! [`record`] writes, so the two cannot differ, and [`check_record`] is why
//! a recording would be refused, asked by the screen and by the request
//! alike. Recording is a request
//! consumed in the tick (`crate::crafting::CraftRequest::Record`), allowed
//! with a workbench in reach, and does not consume the prototype. What a
//! fabricator then does with a blueprint is [`crate::factory`]'s.

use std::collections::BTreeMap;

use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_registry::item::{ItemCatalogue, ItemKind, QUALITY_MAX, Record};
use strata_registry::progression::SkillGraph;
use strata_registry::schematic::{FactoryRules, Schematic, SlotDef, Stat, Takes};
use strata_registry::{GameRegistries, Namespaced};

use crate::crafting::{self, CraftRefusal, CraftState, Made, Scratch};
use crate::items::{self, HouseholdStore, Pack, Stock};
use crate::progression::{CharacterSheet, Touch, factory_field, touches};

/// A whole, in the per-mille a derating is worked in.
const PER_MILLE: u32 = QUALITY_MAX as u32;

/// Every blueprint the household has recorded, by its number, which is the
/// batch number of its item. A spent one stays, so a thing run off it can
/// still say what it was run off; its item is gone. The game saves it whole
/// on every change.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Blueprints(pub BTreeMap<u32, Blueprint>);

/// One production blueprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blueprint {
    /// The schematic the prototype was made by.
    pub schematic: Namespaced,
    /// The item every run makes.
    pub makes: Namespaced,
    /// The schematic's complexity when it was recorded: what a unit's time
    /// is counted by.
    pub complexity: u8,
    /// The lines every unit has, exactly: the prototype's times the
    /// derating.
    pub lines: BTreeMap<String, u16>,
    /// What each slot takes and asks for, in the schematic's order.
    pub slots: Vec<SlotNeed>,
    /// The runs it started with.
    pub runs: u32,
    /// The runs it has left.
    pub runs_left: u32,
    /// Who recorded it, and so whose name every unit carries.
    pub by: String,
    /// What it was recorded at, per-mille of the prototype.
    pub derate: u16,
    /// The prototype's batch number.
    pub prototype: u32,
}

/// One slot of a blueprint: the schematic's slot, copied, and what stock
/// must have to feed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotNeed {
    /// The schematic's slot as it was when the blueprint was recorded: what
    /// it takes and how much a unit.
    pub slot: SlotDef,
    /// A requirement for each stat the schematic weighs on the slot, in the
    /// order its lines first name them. None for a commodity's slot.
    pub needs: Vec<Need>,
}

/// What a slot asks of one figure of its stock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    /// The figure, by the line of a record it is read from: an attribute's
    /// key or a component's line id.
    pub stat: String,
    /// The least it may be, where more of it is better.
    pub at_least: Option<u16>,
    /// The most it may be, where the schematic reads it as `Light` and less
    /// of it is better.
    pub at_most: Option<u16>,
}

/// `figure` at `pm` of itself, to the nearest whole: a blueprint's line, and
/// the floor it asks of a figure of which more is better.
#[must_use]
pub fn derated(figure: u16, pm: u16) -> u16 {
    crafting::rounded(u32::from(figure) * u32::from(pm.min(QUALITY_MAX)))
}

/// The ceiling a blueprint recorded at `pm` asks of a figure of which less
/// is better, the prototype's input having had `figure`: what the figure is
/// short of the top of the scale is what is derated, so a lower derating
/// lets a heavier stock in, as it lets a weaker one in elsewhere.
#[must_use]
pub fn eased_cap(figure: u16, pm: u16) -> u16 {
    let short = PER_MILLE.saturating_sub(u32::from(figure));
    crafting::rounded(PER_MILLE * PER_MILLE - short * u32::from(pm.min(QUALITY_MAX)))
}

/// The schematic a ledger entry was made by: the one it names, or for an
/// entry from before it named one, the only schematic that makes its item
/// from its slots.
fn schematic_of<'a>(registries: &'a GameRegistries, made: &Made) -> Option<&'a Schematic> {
    if let Some(id) = &made.schematic {
        return registries.schematics.get(id);
    }
    let mut fitting = registries.schematics.iter().filter(|schematic| {
        schematic.def.makes == made.item
            && schematic
                .def
                .slots
                .iter()
                .map(|slot| &slot.id)
                .eq(made.from.iter().map(|(slot, _)| slot))
    });
    let first = fitting.next()?;
    fitting.next().is_none().then_some(first)
}

/// The runs a blueprint of a schematic of `complexity` starts with when
/// `sheet` records it: the dial's and the sheet's `Factory` boxes on it. The
/// one place the number is worked out, so a screen shows what a recording
/// will write; a sheet with no such box reads the dial untouched.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a run count, a few hundred, rounded and held to one and up before the cast"
)]
pub fn runs_for(
    rules: &FactoryRules,
    graph: &SkillGraph,
    sheet: &CharacterSheet,
    complexity: u8,
) -> u32 {
    let dial = rules.runs(complexity);
    if !touches(graph, sheet, Touch::Factory, "runs") {
        return dial;
    }
    (factory_field(graph, sheet, "runs", dial as f32)
        .round()
        .max(1.0) as u32)
        .max(1)
}

/// The blueprint `prototype` would be recorded as at `derate` per-mille by
/// `by`, with the runs it would start with: what a screen shows as the
/// derating moves, and what [`record`] writes. Nothing is changed.
///
/// # Errors
/// No factory dials, a derating off the dial's steps, a thing the ledger
/// does not say the making of, one run off a blueprint itself, or one
/// recorded already.
pub fn draft(
    registries: &GameRegistries,
    craft: &CraftState,
    prototype: &Record,
    derate: u16,
    by: &str,
) -> Result<Blueprint, CraftRefusal> {
    draft_for(
        registries,
        craft,
        prototype,
        derate,
        (by, &CharacterSheet::default()),
    )
}

/// [`draft`] as the member `by`, whose `sheet` sets the runs it starts with.
///
/// # Errors
/// What [`draft`] refuses.
pub fn draft_for(
    registries: &GameRegistries,
    craft: &CraftState,
    prototype: &Record,
    derate: u16,
    (by, sheet): (&str, &CharacterSheet),
) -> Result<Blueprint, CraftRefusal> {
    let rules = registries
        .schematics
        .factory()
        .ok_or(CraftRefusal::NoBlueprints)?;
    if !rules.allows(derate) {
        return Err(CraftRefusal::Derate { asked: derate });
    }
    // A bought thing has no entry, and a batch number another household's
    // maker gave (the first day's stock has one) names another item here.
    let made = craft
        .made
        .get(&prototype.batch)
        .filter(|made| prototype.batch != 0 && made.item == prototype.item)
        .ok_or(CraftRefusal::NoLedger)?;
    if let Some(blueprint) = made.blueprint {
        return Err(CraftRefusal::RunOff { blueprint });
    }
    if let Some(blueprint) = made.recorded {
        return Err(CraftRefusal::Recorded { blueprint });
    }
    let schematic = schematic_of(registries, made).ok_or(CraftRefusal::NoLedger)?;
    let mut lines = BTreeMap::new();
    for recipe in &schematic.def.lines {
        let figure = prototype
            .lines
            .get(&recipe.line)
            .ok_or(CraftRefusal::NoLedger)?;
        lines.insert(recipe.line.clone(), derated(*figure, derate));
    }
    let mut slots = Vec::with_capacity(schematic.def.slots.len());
    for (index, slot) in schematic.def.slots.iter().enumerate() {
        let input = made
            .from
            .iter()
            .find(|(id, _)| *id == slot.id)
            .map(|(_, input)| input)
            .ok_or(CraftRefusal::NoLedger)?;
        let mut needs: Vec<Need> = Vec::new();
        for stat in schematic.stats_on(index) {
            let figure = stat.figure(input).ok_or(CraftRefusal::NoLedger)?;
            let key = stat.key();
            let at = needs
                .iter()
                .position(|need| need.stat == key)
                .unwrap_or_else(|| {
                    needs.push(Need {
                        stat: key.to_owned(),
                        at_least: None,
                        at_most: None,
                    });
                    needs.len() - 1
                });
            match stat {
                Stat::Attribute(_) | Stat::Line(_) => {
                    needs[at].at_least = Some(derated(figure, derate));
                }
                Stat::Light(_) => needs[at].at_most = Some(eased_cap(figure, derate)),
            }
        }
        slots.push(SlotNeed {
            slot: slot.clone(),
            needs,
        });
    }
    let runs = runs_for(
        rules,
        &registries.progression,
        sheet,
        schematic.def.complexity,
    );
    Ok(Blueprint {
        schematic: schematic.def.id.clone(),
        makes: schematic.def.makes.clone(),
        complexity: schematic.def.complexity,
        lines,
        slots,
        runs,
        runs_left: runs,
        by: by.to_owned(),
        derate,
        prototype: prototype.batch,
    })
}

/// The things in the pack, its bags and, when given, the household's store
/// that were made by `schematic` at a bench and whose making the ledger
/// still knows, each kind once, the earliest batch first: what the crafting
/// tab lists as made earlier. One recorded already is listed, and [`draft`]
/// says so; a fabricator's copy is not, since it is no prototype.
#[must_use]
pub fn prototypes(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    craft: &CraftState,
    schematic: &Schematic,
) -> Vec<Record> {
    let mut found: Vec<Record> = Vec::new();
    for (record, _) in crafting::reachable(&registries.items, pack, store) {
        let kind = crafting::kind_of(&record);
        let known = craft.made.get(&kind.batch).is_some_and(|made| {
            kind.batch != 0
                && made.item == kind.item
                && made.blueprint.is_none()
                && schematic_of(registries, made).is_some_and(|by| by.def.id == schematic.def.id)
        });
        if known && !found.contains(&kind) {
            found.push(kind);
        }
    }
    found.sort_by_key(|kind| kind.batch);
    found
}

/// Whether the thing of batch `batch` in the pack, its bags and, when
/// given, the household's store can be recorded as a blueprint at `derate`
/// per-mille by `by`, and as what: the prototype found and the blueprint
/// [`record`] would write of it. Every refusal a request meets, in the
/// order it meets them, but the two that are not this function's to know:
/// the workbench, which is the request's own gate
/// (`crate::crafting::at_workbench`), and room for the blueprint's item,
/// which only the putting away finds. What a screen greys Record as
/// blueprint by, so the screen and the request cannot give two reasons.
/// Nothing is changed.
///
/// # Errors
/// No factory dials, a derating off the dial's steps, nothing of that batch
/// in reach, or what [`draft`] refuses.
pub fn check_record(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    craft: &CraftState,
    batch: u32,
    derate: u16,
    by: &str,
) -> Result<(Record, Blueprint), CraftRefusal> {
    check_record_for(
        registries,
        pack,
        store,
        craft,
        batch,
        derate,
        (by, &CharacterSheet::default()),
    )
}

/// [`check_record`] as the member `by`, whose `sheet` sets the runs.
///
/// # Errors
/// What [`check_record`] refuses.
pub fn check_record_for(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    craft: &CraftState,
    batch: u32,
    derate: u16,
    by: (&str, &CharacterSheet),
) -> Result<(Record, Blueprint), CraftRefusal> {
    let rules = registries
        .schematics
        .factory()
        .ok_or(CraftRefusal::NoBlueprints)?;
    if !rules.allows(derate) {
        return Err(CraftRefusal::Derate { asked: derate });
    }
    // The one the ledger knows, where a number is shared with something
    // made elsewhere.
    let held: Vec<Record> = crafting::reachable(&registries.items, pack, store)
        .into_iter()
        .map(|(record, _)| record)
        .filter(|record| batch != 0 && record.batch == batch)
        .collect();
    let known = |record: &&Record| {
        craft
            .made
            .get(&batch)
            .is_some_and(|made| made.item == record.item)
    };
    let prototype = held
        .iter()
        .find(known)
        .or(held.first())
        .ok_or(CraftRefusal::NoPrototype)?;
    let blueprint = draft_for(registries, craft, prototype, derate, by)?;
    Ok((prototype.clone(), blueprint))
}

/// Record the thing of batch `batch` in reach as a blueprint at `derate`
/// per-mille, by `by`: the blueprint takes the household's next number, its
/// item is put away as a finished thing is (the pack, then a bag, then the
/// store), its body is filed under the number and the prototype's ledger
/// entry says it is recorded. The prototype is not consumed. One
/// transaction: a refusal has changed nothing. Gives the blueprint's number
/// and its item.
///
/// # Errors
/// What [`check_record`] refuses, or no room for the item.
pub fn record(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    craft: &mut CraftState,
    prints: &mut Blueprints,
    batch: u32,
    derate: u16,
    by: &str,
) -> Result<(u32, Record), CraftRefusal> {
    record_for(
        registries,
        stock,
        craft,
        prints,
        (batch, derate),
        (by, &CharacterSheet::default()),
    )
}

/// [`record`] as the member `by`, whose `sheet` sets the runs.
///
/// # Errors
/// What [`record`] refuses.
pub fn record_for(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    craft: &mut CraftState,
    prints: &mut Blueprints,
    (batch, derate): (u32, u16),
    (by, sheet): (&str, &CharacterSheet),
) -> Result<(u32, Record), CraftRefusal> {
    let items = &registries.items;
    let (_, blueprint) = check_record_for(
        registries,
        stock.pack,
        stock.store.as_deref(),
        craft,
        batch,
        derate,
        (by, sheet),
    )?;
    // Found by `check_record`.
    let rules = registries
        .schematics
        .factory()
        .ok_or(CraftRefusal::NoBlueprints)?;
    let number = craft.batches.saturating_add(1);
    let item = Record {
        item: rules.blueprint().clone(),
        count: 1,
        batch: number,
        by: Some(by.to_owned()),
        lines: BTreeMap::new(),
        condition: None,
        fitted: BTreeMap::new(),
        origin: None,
    };
    let written = items::write(&item, items).map_err(|_| CraftRefusal::NoBlueprints)?;
    let mut scratch = Scratch::of(stock);
    scratch.put_away(items, written)?;
    scratch.commit(stock);
    prints.0.insert(number, blueprint);
    craft.batches = number;
    if let Some(made) = craft.made.get_mut(&batch) {
        made.recorded = Some(number);
    }
    Ok((number, item))
}

/// The blueprints whose items are in the pack, its bags and, when given,
/// the household's store, each with whether its item is in the store, in
/// the order the stock is reached: the carried first. An item whose body
/// the ledger has not got is no blueprint.
#[must_use]
pub fn held(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    prints: &Blueprints,
) -> Vec<(u32, bool)> {
    let items = &registries.items;
    let mut found: Vec<(u32, bool)> = Vec::new();
    for (record, stored) in crafting::reachable(items, pack, store) {
        let blueprint = matches!(
            items.get(&record.item).map(|def| &def.kind),
            Some(ItemKind::Blueprint)
        );
        if blueprint
            && prints.0.contains_key(&record.batch)
            && !found.iter().any(|(id, _)| *id == record.batch)
        {
            found.push((record.batch, stored));
        }
    }
    found
}

impl Blueprint {
    /// One unit as a run makes it: the blueprint's item and lines exactly,
    /// of batch `batch`, by whoever recorded the blueprint.
    #[must_use]
    pub fn unit(&self, batch: u32, items: &ItemCatalogue) -> Record {
        Record {
            item: self.makes.clone(),
            count: 1,
            batch,
            by: Some(self.by.clone()),
            lines: self.lines.clone(),
            condition: crate::crafting::made_condition(items, &self.makes),
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    /// Whether the blueprint still fits the data set: what it makes is an
    /// item with exactly its lines, and every slot takes something that
    /// exists. One that does not is kept, since the data may come back,
    /// and is refused at a fabricator.
    #[must_use]
    pub fn stands(&self, registries: &GameRegistries) -> bool {
        let items = &registries.items;
        if items.check_record(&self.unit(1, items)).is_err() {
            return false;
        }
        self.slots.iter().all(|need| {
            let slot = &need.slot;
            let takes_what_is = items.iter().any(|def| match (&slot.takes, &def.kind) {
                (Takes::Family(wanted), ItemKind::Lot { family, .. }) => family == wanted,
                (Takes::Class(wanted), ItemKind::Lot { class, .. }) => class == wanted,
                (Takes::Type(wanted), ItemKind::Lot { resource, .. }) => resource == wanted,
                (Takes::Component(wanted), ItemKind::Component)
                | (Takes::Commodity(wanted), ItemKind::Commodity) => def.id == *wanted,
                _ => false,
            });
            takes_what_is && slot.amount() > 0
        })
    }
}

impl Blueprints {
    /// Bring a saved ledger back inside what a blueprint can be where a
    /// file has put it outside: one with more runs left than it started
    /// with is held to what it started with. Said in words for a log, one
    /// for each thing changed. A blueprint the data no longer fits is not
    /// this function's to mend: it is kept as it is and refused at a
    /// fabricator ([`Blueprint::stands`]).
    pub fn repair(&mut self, _registries: &GameRegistries) -> Vec<String> {
        let mut said = Vec::new();
        for (number, blueprint) in &mut self.0 {
            if blueprint.runs_left > blueprint.runs {
                said.push(format!(
                    "blueprint {number} had {} runs left of the {} it started with; it has {}",
                    blueprint.runs_left, blueprint.runs, blueprint.runs
                ));
                blueprint.runs_left = blueprint.runs;
            }
        }
        said
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::Registries;
    use crate::crafting::tests::{
        CORE, CORE_ITEM, FEED, HARNESS, MAKER, SEALS, bench, fill, first_lots, id, part, schematic,
        shipped, smith, units,
    };
    use crate::crafting::{Picks, assemble, best_stock, finish, kind_of};
    use crate::items::Hands;

    const BLUEPRINT: &str = "veyra:blueprint.production";

    /// The walk's second core: charge density 513, stability 209, from the
    /// first day's Tolley salt and Oxbow brine, as batch 3 of a household
    /// whose first two batches were something else.
    fn walked(registries: &Registries) -> (Pack, Hands, HouseholdStore, CraftState, Record) {
        let [salt, brine, _] = first_lots(registries);
        let (mut pack, mut hands, mut store) = bench(registries, &[salt, brine]);
        let core = schematic(registries, CORE);
        let picks = best_stock(&registries.0, &pack, Some(&store), core);
        let mut state = CraftState {
            batches: 2,
            ..CraftState::default()
        };
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
        // The walk's rolls came to 0.86 of the ceiling of 597; stability
        // stood at its floor.
        let session = state.session.as_mut().unwrap();
        assert_eq!(
            (session.lines[0].ceiling, session.lines[1].ceiling),
            (597, 349)
        );
        session.lines[0].share = 860;
        let made = finish(&registries.0, &mut stock, &mut state).unwrap();
        assert_eq!(made.batch, 3);
        assert_eq!(
            (made.lines["charge_density"], made.lines["stability"]),
            (513, 209)
        );
        (pack, hands, store, state, made)
    }

    fn need(stat: &str, at_least: u16) -> Need {
        Need {
            stat: stat.to_owned(),
            at_least: Some(at_least),
            at_most: None,
        }
    }

    /// The design's table: the walk's core recorded at the whole of itself
    /// asks for exactly what went into it, and at 85 % for 85 % of each
    /// figure and makes 85 % of each line, a half rounding up.
    #[test]
    fn the_walks_core_records_at_full_and_at_85() {
        let registries = shipped();
        let (.., state, core) = walked(&registries);
        let full = draft(&registries.0, &state, &core, 1000, MAKER).unwrap();
        assert_eq!(
            (full.schematic.clone(), full.makes.clone(), full.complexity),
            (id(CORE), id(CORE_ITEM), 1)
        );
        assert_eq!(
            full.lines,
            BTreeMap::from([
                ("charge_density".to_owned(), 513),
                ("stability".to_owned(), 209),
            ])
        );
        let slots: Vec<(&str, &str, u32)> = full
            .slots
            .iter()
            .map(|need| {
                (
                    need.slot.id.as_str(),
                    need.slot.name.as_str(),
                    need.slot.amount(),
                )
            })
            .collect();
        assert_eq!(slots, [("salt", "Reactive", 20), ("brine", "Brine", 30)]);
        // The slot is the schematic's own, so any reactive that meets the
        // minimum feeds it, not only the prototype's salt.
        assert_eq!(
            full.slots[0].slot,
            schematic(&registries, CORE).def.slots[0]
        );
        assert_eq!(
            full.slots[0].needs,
            [need("potency", 632), need("stability", 225)]
        );
        assert_eq!(
            full.slots[1].needs,
            [
                need("potency", 605),
                need("conductivity", 495),
                need("purity", 534)
            ]
        );
        assert_eq!((full.runs, full.runs_left), (100, 100));
        assert_eq!(
            (full.by.as_str(), full.derate, full.prototype),
            (MAKER, 1000, 3)
        );

        let derated = draft(&registries.0, &state, &core, 850, MAKER).unwrap();
        assert_eq!(
            derated.lines,
            BTreeMap::from([
                ("charge_density".to_owned(), 436),
                ("stability".to_owned(), 178),
            ])
        );
        assert_eq!(
            derated.slots[0].needs,
            [need("potency", 537), need("stability", 191)]
        );
        assert_eq!(
            derated.slots[1].needs,
            [
                need("potency", 514),
                need("conductivity", 421),
                need("purity", 454)
            ]
        );
        assert_eq!((derated.derate, derated.runs), (850, 100));
        // A derated blueprint asks for less and makes less, line by line.
        for (line, figure) in &derated.lines {
            assert!(figure < &full.lines[line], "{line}");
        }
        // The arithmetic, in whole numbers.
        assert_eq!(
            (super::derated(513, 850), super::derated(209, 850)),
            (436, 178)
        );
        assert_eq!(super::derated(495, 850), 421, "420.75");
        assert_eq!(super::derated(605, 700), 424, "423.5 rounds up");
        assert_eq!(super::derated(1000, 1000), 1000);
        // Off the dial's steps nothing is drafted.
        for off in [0, 650, 849, 1001] {
            assert_eq!(
                draft(&registries.0, &state, &core, off, MAKER),
                Err(CraftRefusal::Derate { asked: off })
            );
        }
        // The draft changed nothing, and is the same asked twice.
        assert_eq!(draft(&registries.0, &state, &core, 1000, MAKER), Ok(full));
    }

    /// Where the schematic reads a stat as Light, less of it is better, so
    /// the blueprint asks for a ceiling and a derating raises it: the
    /// magazine's casing, density 165, is at most 165, then 290, then 416.
    #[test]
    fn a_light_stat_is_a_cap_eased_the_other_way() {
        let registries = shipped();
        let [.., alloy] = first_lots(&registries);
        let core = part(&registries, CORE_ITEM, 1, Some((1, &[537, 209])));
        let (mut pack, mut hands, mut store) = bench(&registries, &[alloy.clone(), core.clone()]);
        let picks = Picks::from([
            ("core".to_owned(), kind_of(&core)),
            ("harness".to_owned(), part(&registries, HARNESS, 1, None)),
            ("casing".to_owned(), kind_of(&alloy)),
            ("seals".to_owned(), part(&registries, SEALS, 1, None)),
        ]);
        let mut state = CraftState {
            batches: 1,
            ..CraftState::default()
        };
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
            &id(FEED),
            &picks,
            MAKER,
            &smith(),
        )
        .unwrap();
        let magazine = finish(&registries.0, &mut stock, &mut state).unwrap();
        assert_eq!(alloy.lines["density"], 165);

        let casing = |derate: u16| {
            let print = draft(&registries.0, &state, &magazine, derate, MAKER).unwrap();
            assert_eq!(print.complexity, 2);
            assert_eq!(print.runs, 50, "a magazine's blueprint has fifty runs");
            let slot = print.slots.iter().find(|need| need.slot.id == "casing");
            slot.unwrap().needs.clone()
        };
        let cap = |stat: &str, at_most: u16| Need {
            stat: stat.to_owned(),
            at_least: None,
            at_most: Some(at_most),
        };
        // In the order the lines first name them: purity for capacity, then
        // density and strength for the reload.
        assert_eq!(
            casing(1000),
            [
                need("purity", 533),
                cap("density", 165),
                need("strength", 298)
            ]
        );
        assert_eq!(
            casing(850),
            [
                need("purity", 453),
                cap("density", 290),
                need("strength", 253)
            ]
        );
        assert_eq!(casing(700)[1], cap("density", 416));
        assert_eq!(
            (
                eased_cap(165, 1000),
                eased_cap(165, 850),
                eased_cap(165, 700)
            ),
            (165, 290, 416)
        );
        // The requirement is always the easier one: never under the
        // prototype's own figure, and at the top of the scale at nought.
        assert_eq!(eased_cap(1000, 700), 1000);
        assert_eq!(eased_cap(0, 700), 300);
        // The components' slots ask for their lines, and a commodity's for
        // nothing.
        let print = draft(&registries.0, &state, &magazine, 1000, MAKER).unwrap();
        let needs: Vec<(&str, &[Need])> = print
            .slots
            .iter()
            .map(|need| (need.slot.id.as_str(), need.needs.as_slice()))
            .collect();
        assert_eq!(needs[0], ("core", &[need("charge_density", 537)][..]));
        assert_eq!(needs[1], ("harness", &[need("conductance", 500)][..]));
        assert_eq!(needs[3], ("seals", &[][..]));
        assert!(print.stands(&registries.0));
    }

    /// Recording takes the household's next number, puts the blueprint's
    /// item with what the character carries, files the body and leaves the
    /// prototype where it was.
    #[test]
    fn recording_keeps_the_prototype_and_takes_a_number() {
        let registries = shipped();
        let items = &registries.0.items;
        let (mut pack, mut hands, mut store, mut state, core) = walked(&registries);
        let mut prints = Blueprints::default();
        let before = (pack.clone(), store.clone());
        let drafted = draft(&registries.0, &state, &core, 850, MAKER).unwrap();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        let (number, item) = record(
            &registries.0,
            &mut stock,
            &mut state,
            &mut prints,
            3,
            850,
            MAKER,
        )
        .unwrap();
        assert_eq!(number, 4, "the next number after the core's 3");
        assert_eq!(
            state.batches, 4,
            "one counter numbers batches and blueprints"
        );
        assert_eq!(
            (
                item.item.clone(),
                item.count,
                item.batch,
                item.by.as_deref()
            ),
            (id(BLUEPRINT), 1, 4, Some(MAKER))
        );
        assert!(item.lines.is_empty());
        assert_eq!(items.check_record(&item), Ok(()));
        // What was shown is what was written.
        assert_eq!(prints.0.keys().copied().collect::<Vec<_>>(), [4]);
        assert_eq!(prints.0[&4], drafted);
        assert_eq!(state.made[&3].recorded, Some(4));
        assert_eq!(state.made[&3].schematic, Some(id(CORE)));
        // The prototype is where it was, and the blueprint beside it.
        assert_eq!(units(&pack.cells, &core, items), 1);
        assert_eq!(
            items::read(pack.cells.get(0).unwrap(), items).unwrap(),
            core
        );
        assert_eq!(
            items::read(pack.cells.get(1).unwrap(), items).unwrap(),
            item
        );
        assert_eq!(store, before.1, "nothing left the store");
        assert_eq!(
            held(&registries.0, &pack, Some(&store), &prints),
            [(4, false)]
        );
        assert!(held(&registries.0, &pack, None, &Blueprints::default()).is_empty());
        // Listed as made earlier under its own schematic, and under no
        // other.
        assert_eq!(
            prototypes(
                &registries.0,
                &pack,
                Some(&store),
                &state,
                schematic(&registries, CORE)
            ),
            std::slice::from_ref(&core)
        );
        assert!(
            prototypes(
                &registries.0,
                &pack,
                Some(&store),
                &state,
                schematic(&registries, FEED)
            )
            .is_empty()
        );
        // The file holds it and reads it back.
        let text = ron::ser::to_string_pretty(&prints, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<Blueprints>(&text).unwrap(), prints);
        assert_eq!(
            ron::from_str::<Blueprints>("({})").unwrap(),
            Blueprints::default()
        );
    }

    /// A prototype is recorded once, so a blueprint's runs stay finite: the
    /// second asking is refused with the number it has, at any derating,
    /// and has changed nothing.
    #[test]
    fn a_prototype_is_recorded_once() {
        let registries = shipped();
        let (mut pack, mut hands, mut store, mut state, core) = walked(&registries);
        let mut prints = Blueprints::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        record(
            &registries.0,
            &mut stock,
            &mut state,
            &mut prints,
            3,
            1000,
            MAKER,
        )
        .unwrap();
        let before = (state.clone(), prints.clone());
        for derate in [1000, 700] {
            assert_eq!(
                record(
                    &registries.0,
                    &mut stock,
                    &mut state,
                    &mut prints,
                    3,
                    derate,
                    MAKER
                ),
                Err(CraftRefusal::Recorded { blueprint: 4 })
            );
        }
        assert_eq!(
            draft(&registries.0, &state, &core, 850, MAKER),
            Err(CraftRefusal::Recorded { blueprint: 4 })
        );
        assert_eq!((state, prints), before);
        // The blueprint's own item is no prototype either: nothing went
        // into it.
        let (mut state, mut prints) = before;
        assert_eq!(
            record(
                &registries.0,
                &mut stock,
                &mut state,
                &mut prints,
                4,
                1000,
                MAKER
            ),
            Err(CraftRefusal::NoLedger)
        );
    }

    /// A unit a fabricator ran off a blueprint is a copy: it is refused as
    /// a prototype with the blueprint it came off, and is not listed among
    /// the things made earlier.
    #[test]
    fn a_factory_unit_is_no_prototype() {
        let registries = shipped();
        let (mut pack, mut hands, mut store, mut state, core) = walked(&registries);
        let mut prints = Blueprints::default();
        // Batch 5, as a run off blueprint 4 files it.
        let copy = Record {
            batch: 5,
            ..core.clone()
        };
        state.batches = 5;
        state.made.insert(
            5,
            Made {
                blueprint: Some(4),
                ..state.made[&3].clone()
            },
        );
        let items = &registries.0.items;
        let copied = items::write(&copy, items).unwrap();
        assert_eq!(items::place_in(items, &mut store.0, copied), None);
        assert_eq!(
            draft(&registries.0, &state, &copy, 1000, MAKER),
            Err(CraftRefusal::RunOff { blueprint: 4 })
        );
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            record(
                &registries.0,
                &mut stock,
                &mut state,
                &mut prints,
                5,
                1000,
                MAKER
            ),
            Err(CraftRefusal::RunOff { blueprint: 4 })
        );
        assert!(prints.0.is_empty());
        assert_eq!(state.batches, 5);
        assert_eq!(
            prototypes(
                &registries.0,
                &pack,
                Some(&store),
                &state,
                schematic(&registries, CORE)
            ),
            [core]
        );
    }

    /// What the ledger no longer says the making of cannot be recorded: a
    /// bought part, a batch made elsewhere, an entry that is gone, and one
    /// whose inputs no longer say what the schematic weighs. Nothing in
    /// reach of that batch is another refusal, and an entry from before a
    /// schematic was named is read by the one schematic that fits it.
    #[test]
    fn a_thing_the_ledger_forgot_is_refused() {
        let registries = shipped();
        let (mut pack, mut hands, mut store, state, core) = walked(&registries);
        let mut prints = Blueprints::default();
        let harness = part(&registries, HARNESS, 1, None);
        assert_eq!(
            draft(&registries.0, &state, &harness, 1000, MAKER),
            Err(CraftRefusal::NoLedger)
        );
        // Tovan Reyes' magazines are batch 412 of somebody else's bench.
        let theirs = registries.0.items.starting().unwrap().store[1].clone();
        assert_eq!(theirs.batch, 412);
        assert_eq!(
            draft(&registries.0, &state, &theirs, 1000, MAKER),
            Err(CraftRefusal::NoLedger)
        );
        let mut record_as = |state: &mut CraftState, batch: u32| {
            let mut stock = Stock {
                saddle: None,
                pack: &mut pack,
                hands: &mut hands,
                store: Some(&mut store),
            };
            record(
                &registries.0,
                &mut stock,
                state,
                &mut prints,
                batch,
                1000,
                MAKER,
            )
        };
        let mut shared = state.clone();
        shared.made.insert(412, state.made[&3].clone());
        assert_eq!(
            record_as(&mut shared, 412),
            Err(CraftRefusal::NoLedger),
            "the number is shared with a core of this bench, which is not in reach"
        );
        let mut forgotten = state.clone();
        forgotten.made.clear();
        assert_eq!(record_as(&mut forgotten, 3), Err(CraftRefusal::NoLedger));
        let mut thin = state.clone();
        thin.made.get_mut(&3).unwrap().from.pop();
        assert_eq!(record_as(&mut thin, 3), Err(CraftRefusal::NoLedger));
        let mut blind = state.clone();
        blind.made.get_mut(&3).unwrap().from[0]
            .1
            .lines
            .remove("potency");
        assert_eq!(record_as(&mut blind, 3), Err(CraftRefusal::NoLedger));
        let mut gone = state.clone();
        gone.made.get_mut(&3).unwrap().schematic = Some(id("veyra:schematic.gone"));
        assert_eq!(record_as(&mut gone, 3), Err(CraftRefusal::NoLedger));
        // Nothing of the batch in reach, and no batch at all.
        let mut whole = state.clone();
        assert_eq!(record_as(&mut whole, 9), Err(CraftRefusal::NoPrototype));
        assert_eq!(record_as(&mut whole, 0), Err(CraftRefusal::NoPrototype));
        assert_eq!(whole, state, "nothing was changed by a refusal");

        // An entry written before blueprints names no schematic: one
        // schematic makes a cell core from a salt and a brine, so it is
        // that one.
        let mut older = state.clone();
        older.made.get_mut(&3).unwrap().schematic = None;
        let read = draft(&registries.0, &older, &core, 1000, MAKER).unwrap();
        assert_eq!(read.schematic, id(CORE));
        assert_eq!(
            read,
            draft(&registries.0, &state, &core, 1000, MAKER).unwrap()
        );
        // With no factory dials nothing is recorded at all.
        let mut bare = (*registries.0).clone();
        bare.schematics = strata_registry::schematic::SchematicSet::default();
        assert_eq!(
            draft(&bare, &state, &core, 1000, MAKER),
            Err(CraftRefusal::NoBlueprints)
        );
    }

    /// What a screen asks before Record is what the request then meets:
    /// [`check_record`] gives the prototype and the draft a recording
    /// writes, and for each thing that cannot be recorded the refusal
    /// [`record`] gives, in the request's order (the dials and the
    /// derating before what is in reach, that before what the ledger says).
    /// Asking changes nothing.
    #[test]
    fn the_check_and_the_request_give_one_answer() {
        let registries = shipped();
        let (mut pack, mut hands, mut store, state, core) = walked(&registries);
        let check = |pack: &Pack, store: &HouseholdStore, state: &CraftState, batch, derate| {
            check_record(
                &registries.0,
                pack,
                Some(store),
                state,
                batch,
                derate,
                MAKER,
            )
        };
        let (prototype, drafted) = check(&pack, &store, &state, 3, 850).unwrap();
        assert_eq!(prototype, core);
        assert_eq!(
            drafted,
            draft(&registries.0, &state, &core, 850, MAKER).unwrap()
        );
        // Out of reach of the store it is where it lies that says.
        let carried = crate::crafting::reachable(&registries.0.items, &pack, None)
            .iter()
            .any(|(record, _)| record.batch == 3);
        assert_eq!(
            check_record(&registries.0, &pack, None, &state, 3, 850, MAKER).is_ok(),
            carried
        );

        let mut copied = state.clone();
        copied.made.get_mut(&3).unwrap().blueprint = Some(2);
        let mut forgotten = state.clone();
        forgotten.made.clear();
        // Each case: the ledger, the batch, the derating, and the refusal.
        let cases = [
            (&state, 3, 840, CraftRefusal::Derate { asked: 840 }),
            // Off the steps and not in reach: the derating is said first.
            (&state, 9, 840, CraftRefusal::Derate { asked: 840 }),
            (&state, 9, 850, CraftRefusal::NoPrototype),
            (&state, 0, 850, CraftRefusal::NoPrototype),
            (&copied, 3, 850, CraftRefusal::RunOff { blueprint: 2 }),
            (&forgotten, 3, 850, CraftRefusal::NoLedger),
        ];
        for (ledger, batch, derate, refusal) in cases {
            assert_eq!(
                check(&pack, &store, ledger, batch, derate).map(|_| ()),
                Err(refusal.clone()),
                "batch {batch} at {derate}"
            );
            let (mut asked, mut prints) = (ledger.clone(), Blueprints::default());
            let mut stock = Stock {
                saddle: None,
                pack: &mut pack,
                hands: &mut hands,
                store: Some(&mut store),
            };
            assert_eq!(
                record(
                    &registries.0,
                    &mut stock,
                    &mut asked,
                    &mut prints,
                    batch,
                    derate,
                    MAKER
                ),
                Err(refusal),
                "batch {batch} at {derate}"
            );
        }

        // Recorded, the check says so with the number, as the request does.
        let (mut recorded, mut prints) = (state.clone(), Blueprints::default());
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        let (number, _) = record(
            &registries.0,
            &mut stock,
            &mut recorded,
            &mut prints,
            3,
            850,
            MAKER,
        )
        .unwrap();
        assert_eq!(prints.0[&number], drafted);
        assert_eq!(
            check(&pack, &store, &recorded, 3, 1000).map(|_| ()),
            Err(CraftRefusal::Recorded { blueprint: number })
        );
        // With no factory dials nothing is, whatever is asked.
        let mut bare = (*registries.0).clone();
        bare.schematics = strata_registry::schematic::SchematicSet::default();
        assert_eq!(
            check_record(&bare, &pack, Some(&store), &state, 9, 840, MAKER).map(|_| ()),
            Err(CraftRefusal::NoBlueprints)
        );
    }

    /// With no cell for the blueprint's item nothing is recorded: no number
    /// is spent, no body filed, and the prototype can be recorded once
    /// there is room.
    #[test]
    fn no_room_records_nothing() {
        let registries = shipped();
        let items = &registries.0.items;
        let (mut pack, mut hands, mut store, mut state, _) = walked(&registries);
        let mut prints = Blueprints::default();
        let all = pack.cells.len();
        fill(&mut pack.cells, all, items);
        let open = usize::from(pack.open_cells(0, items));
        fill(&mut pack.bags[0], open, items);
        let stored = store.0.len();
        fill(&mut store.0, stored, items);
        let before = (pack.clone(), store.clone(), state.clone());
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        assert_eq!(
            record(
                &registries.0,
                &mut stock,
                &mut state,
                &mut prints,
                3,
                1000,
                MAKER
            ),
            Err(CraftRefusal::NoRoom)
        );
        assert_eq!((pack.clone(), store.clone(), state.clone()), before);
        assert!(prints.0.is_empty());
        // A cell freed in the store, it goes there.
        store.0.set(stored - 1, None);
        store.0.clear_changed();
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        let (number, item) = record(
            &registries.0,
            &mut stock,
            &mut state,
            &mut prints,
            3,
            1000,
            MAKER,
        )
        .unwrap();
        assert_eq!(number, 4);
        assert_eq!(
            items::read(store.0.get(stored - 1).unwrap(), items).unwrap(),
            item
        );
        assert_eq!(
            held(&registries.0, &pack, Some(&store), &prints),
            [(4, true)]
        );
        assert!(held(&registries.0, &pack, None, &prints).is_empty());
        // More runs left than it started with is put back and said.
        prints.0.get_mut(&4).unwrap().runs_left = 250;
        let said = prints.repair(&registries.0);
        assert_eq!(prints.0[&4].runs_left, 100);
        assert_eq!(said.len(), 1);
        assert!(
            said[0].contains("blueprint 4") && said[0].contains("250"),
            "{said:?}"
        );
        assert_eq!(prints.repair(&registries.0), Vec::<String>::new());
    }
}
