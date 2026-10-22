//! Wear and repair (ticket 0007, step 6; `docs/design/crafting.md`, 4.6).
//!
//! A weapon in use loses condition, a little for a shot or a cut and a
//! knock to everything carried when its bearer goes down. Above
//! `slide_below` an item performs as made; below it its lines slide, which
//! [`crate::fitting::factors`] reads through [`slide`], the one place wear
//! reaches a number the fight uses. A repair kit mends the weapon and
//! every part fitted to it ([`can_repair`], `WearRequest::Repair`).
//!
//! Every figure is whole numbers and every dial is `assets/crafting/wear.ron`
//! (`WearRules`). Local-only like the stock it changes: nothing here is
//! replicated, and records change only through [`items::read`] and
//! [`items::write`].

use bevy_ecs::prelude::*;

use slotted_model::{Inventory, ItemStack};

use strata_registry::item::{HANDS, ItemCatalogue, ItemKind, Record, STORE_GRADE};

use strata_registry::progression::SkillGraph;

use strata_registry::wear::{DURABILITY, RESTORE, WearRules};

use crate::combat::Combatant;

use crate::fitting::{self};

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::progression::{CharacterSheet, Touch, touches, wear_field};

/// How far an item's lines have slid, in thousandths: nought at or above
/// `slide_below`, a thousand at nothing.
#[must_use]
pub fn slide(rules: &WearRules, condition: u16) -> u16 {
    if condition >= rules.slide_below {
        return 0;
    }
    let below = u32::from(rules.slide_below - condition);
    u16::try_from(below * 1000 / u32::from(rules.slide_below)).unwrap_or(1000)
}

/// How worn an item reads, for a bar and a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// At or above `show_below`: nothing to say.
    Sound,
    /// Below `show_below` and still performing as made.
    Tired,
    /// Below `slide_below`: the lines are sliding.
    Worn,
}

/// Which [`Band`] `condition` is in.
#[must_use]
pub fn band(rules: &WearRules, condition: u16) -> Band {
    if condition >= rules.show_below {
        Band::Sound
    } else if condition >= rules.slide_below {
        Band::Tired
    } else {
        Band::Worn
    }
}

/// The condition of a record that may have none to say: a part never worn
/// reads full.
#[must_use]
pub fn condition_of(rules: &WearRules, condition: Option<u16>) -> u16 {
    condition.unwrap_or(rules.full)
}

/// Whose boxes a wear rule reads: the sheet of the member who bears the
/// weapon or does the repair, with the graph that gives its boxes their
/// effects. None reads the dials as they are, which is what an opponent and
/// a balance bout read.
#[derive(Debug, Clone, Copy)]
pub struct Bearer<'a> {
    /// The graph the sheet's boxes are looked up in.
    pub graph: &'a SkillGraph,
    /// The member's sheet.
    pub sheet: &'a CharacterSheet,
}

impl Bearer<'_> {
    /// `base` after the sheet's `Wear` boxes on `field`, rounded to a whole
    /// number and never under `least`; `base` itself when none touches it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "thousandths of a point and points, small, rounded and held to nought before the cast"
    )]
    fn whole(self, field: &str, base: u32) -> u32 {
        if !touches(self.graph, self.sheet, Touch::Wear, field) {
            return base;
        }
        wear_field(self.graph, self.sheet, field, base as f32)
            .round()
            .max(0.0) as u32
    }

    /// Whether a repair at a workbench spends a kit: yes, unless a box says
    /// it uses none.
    fn bench_kit(self) -> bool {
        wear_field(self.graph, self.sheet, "bench_kit", 1.0) >= 0.5
    }
}

/// [`cost`] for a shot or a cut (`base`) borne by `bearer`: the sheet's
/// boxes scale what the action costs before the weapon's own durability
/// does.
#[must_use]
pub fn cost_for(rules: &WearRules, base: u32, record: &Record, bearer: Option<Bearer<'_>>) -> u32 {
    cost(
        rules,
        bearer.map_or(base, |bearer| bearer.whole("cost", base)),
        record,
    )
}

/// `base` thousandths of a point, scaled by the item's durability line:
/// more for a rough item, less for a well-made one.
#[must_use]
pub fn cost(rules: &WearRules, base: u32, record: &Record) -> u32 {
    let line = record.lines.get(DURABILITY).copied().unwrap_or(STORE_GRADE);
    base * rules.durability_at_zero.saturating_sub(u32::from(line)) / 1000
}

/// What a fitted or loose part reads once worn down to `value`: full is
/// not written, so a part never worn is the record it always was.
fn part_condition(rules: &WearRules, value: u16) -> Option<u16> {
    (value < rules.full).then_some(value)
}

/// Take `points` off `record` and every part fitted to it, never below
/// nought. Whether anything moved.
pub fn lose(rules: &WearRules, record: &mut Record, points: u16) -> bool {
    let mut moved = false;
    if let Some(condition) = &mut record.condition {
        let next = condition.saturating_sub(points);
        moved |= next != *condition;
        *condition = next;
    }
    for part in record.fitted.values_mut() {
        let was = condition_of(rules, part.condition);
        let next = was.saturating_sub(points);
        moved |= next != was;
        part.condition = part_condition(rules, next);
    }
    moved
}

/// Add `cost` thousandths to `carry` and take whole steps off `record`
/// for what it comes to; the rest stays in `carry`. Whether the record
/// changed.
pub fn accrue(rules: &WearRules, record: &mut Record, carry: &mut u32, cost: u32) -> bool {
    *carry += cost;
    let step = u32::from(rules.step) * 1000;
    let steps = *carry / step;
    *carry %= step;
    let points = u16::try_from(steps * u32::from(rules.step)).unwrap_or(u16::MAX);
    let moved = steps > 0 && lose(rules, record, points);
    if record.condition == Some(0) {
        *carry = 0;
    }
    moved
}

/// The thousandths of a point a hand has earned and not yet paid, which a
/// save does not keep: under one step is lost on exit. It belongs to the
/// hand, not the weapon in it, so under one step of a weapon's wear can land
/// on the one swapped in; that is accepted.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct WearCarry {
    /// Per hand, in the order of [`Hands`].
    pub hands: [u32; HANDS],
    /// The last cut counted, so a swing's several contact ticks cost once.
    pub last_cut: Option<u64>,
    /// Whether the bearer is down, so a fall knocks once.
    pub down: bool,
}

/// Wear the client says out loud.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearNotice {
    /// Going down took this many points off everything carried.
    Knocked {
        /// How many.
        points: u16,
    },
}

/// Charge the weapon in hand `base` thousandths of a point.
pub fn spend(
    items: &ItemCatalogue,
    actor: &Combatant,
    hands: &mut Mut<Hands>,
    carry: &mut WearCarry,
    (base, bearer): (u32, Option<Bearer<'_>>),
) {
    let hand = actor.hand();
    let Some(mut record) = hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
    else {
        return;
    };
    let rules = items.wear();
    let price = cost_for(rules, base, &record, bearer);
    if accrue(rules, &mut record, &mut carry.hands[hand], price)
        && let Ok(stack) = items::write(&record, items)
    {
        hands.0.set(hand, Some(stack));
        hands.0.clear_changed();
    }
}

/// Take `points` off one stack if it is a weapon or a module, or holds
/// parts. A module never worn reads full, so it is knocked like the rest.
/// Whether the stack changed.
pub fn knock_stack(items: &ItemCatalogue, stack: &mut ItemStack, points: u16) -> bool {
    let Ok(mut record) = items::read(stack, items) else {
        return false;
    };
    let module = items
        .get(&record.item)
        .is_some_and(|def| matches!(def.kind, ItemKind::Module { .. }));
    if module && record.condition.is_none() {
        record.condition = Some(items.wear().full);
    }
    if record.condition.is_none() && record.fitted.is_empty() {
        return false;
    }
    if !lose(items.wear(), &mut record, points) {
        return false;
    }
    match items::write(&record, items) {
        Ok(written) => {
            *stack = written;
            true
        }
        Err(_) => false,
    }
}

/// Take `points` off every stack in `inventory`'s first `open` cells that
/// has a condition.
pub fn knock(items: &ItemCatalogue, inventory: &mut Inventory, open: usize, points: u16) {
    for cell in 0..open.min(inventory.len()) {
        let Some(mut stack) = inventory.get(cell).cloned() else {
            continue;
        };
        if knock_stack(items, &mut stack, points) {
            inventory.set(cell, Some(stack));
            inventory.clear_changed();
        }
    }
}

/// Public for the adapter half in `veyra-sim`, which calls it.
pub fn bearing<'a>(graph: &'a SkillGraph, sheet: Option<&'a CharacterSheet>) -> Option<Bearer<'a>> {
    sheet.map(|sheet| Bearer { graph, sheet })
}

/// Why a repair is not done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairRefusal {
    /// No weapon in that hand.
    EmptyHand,
    /// The weapon and every part fitted to it are full.
    NothingToRepair,
    /// No kit to hand: carried, or at a workbench in the store.
    NoKit,
}

/// What a repair would do and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairPlan {
    /// The kit spent, as a record of one; none when the member's boxes say
    /// a repair at a workbench uses no kit.
    pub kit: Option<Record>,
    /// Whether it is a workbench's repair, which restores everything.
    pub at_bench: bool,
    /// The most any of the weapon and its parts gains.
    pub gain: u16,
    /// What the weapon's condition becomes.
    pub to: u16,
}

/// What a repair did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repaired {
    /// See [`RepairPlan::gain`].
    pub gain: u16,
    /// See [`RepairPlan::to`].
    pub to: u16,
    /// Whether it was at a workbench.
    pub at_bench: bool,
}

/// What one kit of restore line `line` mends away from a workbench, in
/// points.
#[must_use]
pub fn field_amount(rules: &WearRules, line: u16) -> u16 {
    let points = rules.field.at_zero + rules.field.per_quality * u32::from(line) / 1000;
    u16::try_from(points.min(u32::from(rules.full))).unwrap_or(rules.full)
}

/// [`field_amount`] as done by `bearer`: the sheet's boxes scale what the
/// kit restores, never past a whole item.
#[must_use]
pub fn field_amount_for(rules: &WearRules, line: u16, bearer: Option<Bearer<'_>>) -> u16 {
    let dial = u32::from(field_amount(rules, line));
    let points = bearer.map_or(dial, |bearer| bearer.whole("field", dial));
    u16::try_from(points.min(u32::from(rules.full))).unwrap_or(rules.full)
}

/// Public for the adapter half in `veyra-sim`, which calls it.
pub fn restore_line(kit: &Record) -> u16 {
    kit.lines.get(RESTORE).copied().unwrap_or(0)
}

/// Whether the weapon in `hand` can be repaired, and with what: the pure
/// rule the request and a screen both ask. At a workbench the poorest kit
/// among the pack, its bags and the store mends everything; away from one
/// the best kit carried mends [`field_amount`] of each.
///
/// # Errors
/// An empty hand, nothing worn, or no kit.
pub fn can_repair(
    items: &ItemCatalogue,
    hands: &Hands,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    hand: usize,
    (at_bench, bearer): (bool, Option<Bearer<'_>>),
) -> Result<RepairPlan, RepairRefusal> {
    let weapon = hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
        .filter(|record| record.condition.is_some())
        .ok_or(RepairRefusal::EmptyHand)?;
    plan_for(items, &weapon, pack, store, (at_bench, bearer))
}

/// Whether the stabled bike can be repaired, by [`can_repair`]'s rule: the
/// same kits, the same field amount, the same workbench. A household with
/// no bike has nothing in its hand to mend, which says `EmptyHand`.
///
/// # Errors
/// No bike, nothing worn, or no kit.
pub fn can_repair_bike(
    items: &ItemCatalogue,
    bike: Option<&Record>,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    (at_bench, bearer): (bool, Option<Bearer<'_>>),
) -> Result<RepairPlan, RepairRefusal> {
    let bike = bike.ok_or(RepairRefusal::EmptyHand)?;
    plan_for(items, bike, pack, store, (at_bench, bearer))
}

fn plan_for(
    items: &ItemCatalogue,
    weapon: &Record,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    (at_bench, bearer): (bool, Option<Bearer<'_>>),
) -> Result<RepairPlan, RepairRefusal> {
    let rules = items.wear();
    let was = condition_of(rules, weapon.condition);
    let parts = || {
        weapon
            .fitted
            .values()
            .map(|part| condition_of(rules, part.condition))
    };
    if was >= rules.full && parts().all(|each| each >= rules.full) {
        return Err(RepairRefusal::NothingToRepair);
    }
    let kits = crate::items::reachable(items, pack, if at_bench { store } else { None })
        .into_iter()
        .map(|(record, _)| record)
        .filter(|record| {
            matches!(
                items.get(&record.item).map(|def| &def.kind),
                Some(ItemKind::Kit)
            )
        });
    let mut chosen: Option<Record> = None;
    for kit in kits {
        let better = chosen.as_ref().is_none_or(|held| {
            if at_bench {
                restore_line(&kit) < restore_line(held)
            } else {
                restore_line(&kit) > restore_line(held)
            }
        });
        if better {
            chosen = Some(kit);
        }
    }
    // A workbench repair a member's boxes make kitless needs none, and so
    // refuses none for want of one.
    let kitless = at_bench && bearer.is_some_and(|bearer| !bearer.bench_kit());
    let kit = match chosen {
        Some(mut kit) => {
            kit.count = 1;
            (!kitless).then_some(kit)
        }
        None if kitless => None,
        None => return Err(RepairRefusal::NoKit),
    };
    let amount = field_amount_for(rules, kit.as_ref().map_or(0, restore_line), bearer);
    let to = |from: u16| {
        if at_bench {
            rules.full
        } else {
            rules.full.min(from.saturating_add(amount))
        }
    };
    let gain = std::iter::once(was)
        .chain(parts())
        .map(|from| to(from) - from)
        .max()
        .unwrap_or(0);
    Ok(RepairPlan {
        kit,
        at_bench,
        gain,
        to: to(was),
    })
}

/// Mend the weapon in `hand` by the plan [`can_repair`] makes and spend
/// its kit, on `stock`. One transaction on the caller's copies.
///
/// # Errors
/// What [`can_repair`] refuses.
pub fn repair(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    hand: usize,
    (at_bench, bearer): (bool, Option<Bearer<'_>>),
) -> Result<Repaired, RepairRefusal> {
    let plan = can_repair(
        items,
        stock.hands,
        stock.pack,
        stock.store.as_deref().filter(|_| at_bench),
        hand,
        (at_bench, bearer),
    )?;
    let wanted = plan
        .kit
        .as_ref()
        .map(|kit| items::write(kit, items).map_err(|_| RepairRefusal::NoKit))
        .transpose()?;
    let mut weapon = stock
        .hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
        .ok_or(RepairRefusal::EmptyHand)?;
    let rules = items.wear();
    let amount = field_amount_for(rules, plan.kit.as_ref().map_or(0, restore_line), bearer);
    let mend = |from: u16| {
        if at_bench {
            rules.full
        } else {
            rules.full.min(from.saturating_add(amount))
        }
    };
    weapon.condition = Some(mend(condition_of(rules, weapon.condition)));
    for part in weapon.fitted.values_mut() {
        part.condition = part_condition(rules, mend(condition_of(rules, part.condition)));
    }
    let written = items::write(&weapon, items).map_err(|_| RepairRefusal::NoKit)?;
    if let Some(wanted) = &wanted
        && !fitting::take_one(items, stock, wanted)
    {
        return Err(RepairRefusal::NoKit);
    }
    stock.hands.0.set(hand, Some(written));
    stock.hands.0.clear_changed();
    Ok(Repaired {
        gain: plan.gain,
        to: plan.to,
        at_bench,
    })
}

/// Mend the stabled bike by the plan [`can_repair_bike`] makes and spend
/// its kit, on `stock`. One transaction on the caller's copies.
///
/// # Errors
/// What [`can_repair_bike`] refuses.
pub fn repair_bike(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    bike: &mut Option<Record>,
    (at_bench, bearer): (bool, Option<Bearer<'_>>),
) -> Result<Repaired, RepairRefusal> {
    let plan = can_repair_bike(
        items,
        bike.as_ref(),
        stock.pack,
        stock.store.as_deref().filter(|_| at_bench),
        (at_bench, bearer),
    )?;
    if let Some(kit) = &plan.kit {
        let wanted = items::write(kit, items).map_err(|_| RepairRefusal::NoKit)?;
        if !fitting::take_one(items, stock, &wanted) {
            return Err(RepairRefusal::NoKit);
        }
    }
    if let Some(record) = bike {
        record.condition = Some(plan.to);
    }
    Ok(Repaired {
        gain: plan.gain,
        to: plan.to,
        at_bench,
    })
}

/// The sim's answer to a `WearRequest`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Repaired, RepairRefusal>,
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
    #![allow(
        clippy::float_cmp,
        reason = "a worn row is the table's number times a factor exactly; a bit off is a wrong merge"
    )]

    use super::*;

    use crate::combat::Combatant;

    use crate::combat::Weapon;
    use crate::fitting;

    use crate::fitting::loadout;

    use slotted_model::Namespaced;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use strata_registry::item::Fitted;

    use strata_registry::item::ItemCatalogue;

    use strata_registry::item::Property;
    use strata_registry::item::Record;

    use strata_registry::wear::DURABILITY;
    use strata_registry::wear::RESTORE;

    use strata_types::Registries;

    pub const FEED: &str = "veyra:module.patrol_extended_feed";

    pub const KIT: &str = "veyra:kit.field_repair";

    pub fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).unwrap()
    }

    pub fn feed() -> Fitted {
        Fitted {
            item: id(FEED),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("capacity".to_owned(), 500),
                ("reload_time".to_owned(), 500),
            ]),
            condition: None,
        }
    }

    pub fn kit(line: u16, count: u32) -> Record {
        let batch = u32::from(line != 500);
        Record {
            item: id(KIT),
            count,
            batch,
            by: (batch != 0).then(|| "Tester".to_owned()),
            lines: BTreeMap::from([(RESTORE.to_owned(), line)]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    pub fn carbine(items: &ItemCatalogue) -> Record {
        items.starting().unwrap().hands[0].clone()
    }

    #[test]
    fn slide_and_band_follow_the_thresholds() {
        let rules = shipped().0.items.wear().clone();
        let at = |condition| (slide(&rules, condition), band(&rules, condition));
        assert_eq!(at(1000), (0, Band::Sound));
        assert_eq!(at(500), (0, Band::Sound));
        assert_eq!(at(499), (0, Band::Tired));
        assert_eq!(at(250), (0, Band::Tired));
        assert_eq!(at(249), (4, Band::Worn));
        assert_eq!(at(0), (1000, Band::Worn));
    }

    #[test]
    fn a_shot_and_a_cut_cost_what_the_dials_say() {
        let registries = shipped();
        let items = &registries.0.items;
        let rules = items.wear();
        let mut weapon = carbine(items);
        let mut costs = Vec::new();
        for line in [0, 500, 1000] {
            weapon.lines.insert(DURABILITY.to_owned(), line);
            costs.push((
                cost(rules, rules.shot, &weapon),
                cost(rules, rules.cut, &weapon),
            ));
        }
        assert_eq!(costs, [(3000, 4500), (2000, 3000), (1000, 1500)]);
        // No durability line reads store grade.
        weapon.lines.remove(DURABILITY);
        assert_eq!(cost(rules, rules.shot, &weapon), 2000);
    }

    #[test]
    fn sound_items_read_exactly_as_made_and_the_worst_is_the_dials() {
        let registries = shipped();
        let items = &registries.0.items;
        let fitted = fitting::with(items, &carbine(items), "feed", Some(&feed())).unwrap();
        let pistol = Some(items.starting().unwrap().hands[1].clone());
        let never = loadout(items, &[Some(fitted.clone()), pistol.clone()]);
        for condition in [1000, 700, 500, 250] {
            let mut worn = fitted.clone();
            worn.condition = Some(condition);
            worn.fitted.get_mut("feed").unwrap().condition = Some(condition);
            assert_eq!(
                loadout(items, &[Some(worn), pistol.clone()]),
                never,
                "{condition}"
            );
        }
        // Nothing worn nor fitted: only the bought carbine's own action line.
        let bare = loadout(items, &items.starting().unwrap().hands.clone().map(Some));
        assert_eq!(bare.mods.len(), 1);
        assert_eq!(bare.mods[&Weapon::Carbine].len(), 1);

        let mut worst = carbine(items);
        worst.condition = Some(0);
        let mut actor = Combatant::default();
        let table = actor.weapon_spec();
        actor.equip(&loadout(items, &[Some(worst), pistol]));
        let now = actor.weapon_spec();
        // The bought carbine's own 1.10 of the interval, then the wear.
        assert!((now.interval - table.interval * 1.1 * 1.25).abs() < 1e-6);
        assert_eq!(now.reload, table.reload * 1.25);
        // A worn gun's bolts keep their damage (HitDamage is a blade's),
        // and the magazine is the table's.
        assert_eq!(now.damage, table.damage);
        assert_eq!(now.magazine, table.magazine);
        // Halfway down the slide is halfway to the worst.
        let mut half = carbine(items);
        half.condition = Some(125);
        let factors = fitting::factors(items, &half);
        let interval = factors
            .iter()
            .rfind(|factor| factor.property == Property::RepeatInterval)
            .unwrap();
        assert_eq!(interval.factor, 1.125);
    }

    pub fn carbine_pistol(items: &Arc<strata_registry::GameRegistries>) -> Record {
        items.items.starting().unwrap().hands[1].clone()
    }
}
