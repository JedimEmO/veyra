//! Wear and repair (ticket 0007, step 6; `docs/design/crafting.md`, 4.6).
//!
//! A weapon in use loses condition, a little for a shot or a cut and a
//! knock to everything carried when its bearer goes down. Above
//! `slide_below` an item performs as made; below it its lines slide, which
//! [`crate::fitting::factors`] reads through [`slide`], the one place wear
//! reaches a number the fight uses. A repair kit mends the weapon and
//! every part fitted to it ([`can_repair`], [`WearRequest::Repair`]).
//!
//! Every figure is whole numbers and every dial is `assets/crafting/wear.ron`
//! (`WearRules`). Local-only like the stock it changes: nothing here is
//! replicated, and records change only through [`items::read`] and
//! [`items::write`].

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use slotted_model::{Inventory, ItemStack};
use strata_registry::item::{HANDS, ItemCatalogue, ItemKind, Record, STORE_GRADE};
use strata_registry::progression::SkillGraph;
use strata_registry::wear::{DURABILITY, RESTORE, WearRules};

use crate::combat::{Action, AttackKind, AttackRequest, Combatant, ShotFired};
use crate::fitting::{self, WorkbenchAccess};
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};
use crate::progression::{CharacterSheet, Reach, Touch, touches, wear_field};
use crate::ride::Stable;
use crate::{LocalPlayer, Registries, SimSet, SimTick};

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
fn lose(rules: &WearRules, record: &mut Record, points: u16) -> bool {
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
fn accrue(rules: &WearRules, record: &mut Record, carry: &mut u32, cost: u32) -> bool {
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
    last_cut: Option<u64>,
    /// Whether the bearer is down, so a fall knocks once.
    down: bool,
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
fn spend(
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
fn knock_stack(items: &ItemCatalogue, stack: &mut ItemStack, points: u16) -> bool {
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
fn knock(items: &ItemCatalogue, inventory: &mut Inventory, open: usize, points: u16) {
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

fn bearing<'a>(graph: &'a SkillGraph, sheet: Option<&'a CharacterSheet>) -> Option<Bearer<'a>> {
    sheet.map(|sheet| Bearer { graph, sheet })
}

/// `SimTick`, `SimSet::Observe`: the shots and cuts of the tick wear the
/// weapon in hand, and a fall knocks everything carried, once.
pub fn tick(
    registries: Res<Registries>,
    mut shots: MessageReader<ShotFired>,
    mut attacks: MessageReader<AttackRequest>,
    mut notices: MessageWriter<WearNotice>,
    mut actors: Query<(
        &Combatant,
        &mut Hands,
        &mut WearCarry,
        Option<&mut Pack>,
        Option<&CharacterSheet>,
    )>,
) {
    let items = &registries.0.items;
    let rules = items.wear();
    let graph = &registries.0.progression;
    for shot in shots.read() {
        if let Ok((actor, mut hands, mut carry, _, sheet)) = actors.get_mut(shot.attacker) {
            spend(
                items,
                actor,
                &mut hands,
                &mut carry,
                (rules.shot, bearing(graph, sheet)),
            );
        }
    }
    for attack in attacks.read() {
        if !matches!(
            attack.kind,
            AttackKind::Melee { .. } | AttackKind::Strike { .. }
        ) {
            continue;
        }
        if let Ok((actor, mut hands, mut carry, _, sheet)) = actors.get_mut(attack.attacker)
            && carry.last_cut != Some(attack.sequence)
        {
            carry.last_cut = Some(attack.sequence);
            spend(
                items,
                actor,
                &mut hands,
                &mut carry,
                (rules.cut, bearing(graph, sheet)),
            );
        }
    }
    for (actor, mut hands, mut carry, pack, _) in &mut actors {
        let dead = actor.action == Action::Dead;
        if dead == carry.down {
            continue;
        }
        carry.down = dead;
        if !dead || rules.down == 0 {
            continue;
        }
        for hand in 0..HANDS {
            let Some(mut record) = hands
                .0
                .get(hand)
                .and_then(|stack| items::read(stack, items).ok())
            else {
                continue;
            };
            if lose(rules, &mut record, rules.down)
                && let Ok(stack) = items::write(&record, items)
            {
                hands.0.set(hand, Some(stack));
                hands.0.clear_changed();
            }
            carry.hands[hand] = 0;
        }
        if let Some(mut pack) = pack {
            let open: [usize; strata_registry::item::BAG_SLOTS] =
                std::array::from_fn(|bag| usize::from(pack.open_cells(bag, items)));
            let cells = pack.cells.len();
            knock(items, &mut pack.cells, cells, rules.down);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                knock(items, bag, open, rules.down);
            }
            if let Some(stack) = &mut pack.cursor {
                knock_stack(items, stack, rules.down);
            }
        }
        notices.write(WearNotice::Knocked { points: rules.down });
    }
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

fn restore_line(kit: &Record) -> u16 {
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
    let kits = crate::crafting::reachable(items, pack, if at_bench { store } else { None })
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

/// What a screen asks of wear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearRequest {
    /// Mend the stabled bike with a kit.
    RepairBike,
    /// Mend the weapon in a hand with a kit.
    Repair {
        /// The hand the weapon is in: the carbine's, then the pistol's.
        hand: usize,
    },
}

/// The requests not yet consumed, a resource for the reason the fitting
/// inbox is one.
#[derive(Resource, Debug, Default)]
pub struct WearInbox(pub Vec<WearRequest>);

/// The sim's answer to a [`WearRequest`].
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Repaired, RepairRefusal>,
}

/// `SimTick`, `SimSet::Input`, after the factory's requests: every repair
/// asked for since the last tick, in order, each on the stock the one
/// before left. With a workbench in reach the store's kits are to hand and
/// everything is mended; away from one only a carried kit is, and part.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock and its open menu in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<WearInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    mut store: ResMut<HouseholdStore>,
    mut stable: ResMut<Stable>,
    mut players: Query<
        (
            &mut Pack,
            &mut Hands,
            &mut WearCarry,
            Option<&CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<RepairAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let items = &registries.0.items;
    let at_bench = fitting::at_workbench(*access, &reach);
    let mut player = players.single_mut().ok();
    let mut changed = false;
    for request in requests {
        let result = match player.as_mut() {
            None => Err(RepairRefusal::EmptyHand),
            Some((pack, hands, carry, sheet)) => {
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut bike = stable.bike.clone();
                let bearer = sheet.map(|sheet| Bearer {
                    graph: &registries.0.progression,
                    sheet,
                });
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = match request {
                    WearRequest::Repair { hand } => {
                        repair(items, &mut stock, hand, (at_bench, bearer))
                    }
                    WearRequest::RepairBike => {
                        repair_bike(items, &mut stock, &mut bike, (at_bench, bearer))
                    }
                };
                if done.is_ok() {
                    changed = true;
                    if stable.bike != bike {
                        stable.bike = bike;
                    }
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                    if let WearRequest::Repair { hand } = request {
                        carry.hands[hand] = 0;
                    }
                }
                done
            }
        };
        answers.write(RepairAnswer { result });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some((pack, hands, _, _)) = player.as_mut()
    {
        let mut truth = ((**pack).clone(), (**hands).clone(), store.clone());
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
}

/// Registers wear and its request. Added by `SimPlugin`, so a server and a
/// harness have it too; with no hands and no request it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct WearPlugin;

impl Plugin for WearPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WearInbox>()
            .add_message::<RepairAnswer>()
            .add_message::<WearNotice>()
            .add_systems(
                SimTick,
                consume
                    .in_set(SimSet::Input)
                    .after(crate::factory::consume)
                    .before(fitting::sync_loadout),
            )
            .add_systems(SimTick, tick.in_set(SimSet::Observe));
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "a worn row is the table's number times a factor exactly; a bit off is a wrong merge"
    )]

    use super::*;
    use crate::combat::Weapon;
    use crate::fitting::{self, FitInbox, FitRequest, loadout};
    use crate::progression::{Interaction, InteractionKind};
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;
    use bevy::math::Vec3;
    use slotted_model::Namespaced;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use strata_core::WorldPos;
    use strata_registry::item::{Fitted, Property};

    const FEED: &str = "veyra:module.patrol_extended_feed";
    const KIT: &str = "veyra:kit.field_repair";

    fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).unwrap()
    }

    fn feed() -> Fitted {
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

    fn kit(line: u16, count: u32) -> Record {
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

    fn carbine(items: &ItemCatalogue) -> Record {
        items.starting().unwrap().hands[0].clone()
    }

    /// The first day: the player holding the starting weapons, the store
    /// as `starting.ron` has it (two store kits among it), nothing in reach.
    fn first_day() -> App {
        let mut app = app_in_world();
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let arrival = &registries.0.sheets.household.as_ref().unwrap().arrival;
        let pack = Pack::from_saved(&starting.packs[&arrival.name], items).unwrap();
        let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).unwrap();
        let store: Vec<Option<Record>> = starting.store.iter().cloned().map(Some).collect();
        let store = HouseholdStore::from_saved(&store, items).unwrap();
        app.world_mut().spawn((
            LocalPlayer,
            crate::WorldPosition::new(0., 0., 0.),
            pack,
            hands,
            Combatant::default(),
        ));
        app.insert_resource(store).insert_resource(registries);
        app.update();
        app
    }

    fn bench(app: &mut App) {
        app.world_mut().spawn((
            crate::WorldPosition::new(0., 0., 0.),
            Interaction {
                kind: InteractionKind::Workbench,
                radius: 2.5,
            },
        ));
        app.update();
    }

    fn player(app: &mut App) -> (Entity, Pack, Hands, Combatant) {
        let world = app.world_mut();
        let mut players =
            world.query_filtered::<(Entity, &Pack, &Hands, &Combatant), With<LocalPlayer>>();
        let (entity, pack, hands, actor) = players.single(world).unwrap();
        (entity, pack.clone(), hands.clone(), actor.clone())
    }

    fn hand(app: &mut App, index: usize) -> Record {
        let items = app.world().resource::<Registries>().0.clone();
        let (_, _, hands, _) = player(app);
        items::read(hands.0.get(index).unwrap(), &items.items).unwrap()
    }

    fn put_hand(app: &mut App, index: usize, record: &Record) {
        let items = app.world().resource::<Registries>().0.clone();
        let stack = items::write(record, &items.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Hands, With<LocalPlayer>>();
        players.single_mut(world).unwrap().0.set(index, Some(stack));
    }

    fn shoot(app: &mut App, shots: usize) {
        let (entity, ..) = player(app);
        for _ in 0..shots {
            app.world_mut()
                .resource_mut::<Messages<ShotFired>>()
                .write(ShotFired {
                    attacker: entity,
                    from: WorldPos::ZERO,
                    direction: Vec3::X,
                });
        }
        app.update();
    }

    fn repair_all(app: &mut App, hands: &[usize]) -> Vec<Result<Repaired, RepairRefusal>> {
        app.world_mut()
            .resource_mut::<WearInbox>()
            .0
            .extend(hands.iter().map(|hand| WearRequest::Repair { hand: *hand }));
        app.update();
        app.world_mut()
            .resource_mut::<Messages<RepairAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect()
    }

    fn kits_in(app: &mut App) -> Vec<u16> {
        let items = app.world().resource::<Registries>().0.clone();
        let (_, pack, _, _) = player(app);
        let store = app.world().resource::<HouseholdStore>().clone();
        let mut lines = Vec::new();
        for (record, _) in crate::crafting::reachable(&items.items, &pack, Some(&store)) {
            if record.item == id(KIT) {
                lines.extend(std::iter::repeat_n(
                    restore_line(&record),
                    record.count as usize,
                ));
            }
        }
        lines.sort_unstable();
        lines
    }

    fn put_in_pack(app: &mut App, record: &Record) {
        let items = app.world().resource::<Registries>().0.clone();
        let stack = items::write(record, &items.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        pack.cells.set(cell, Some(stack));
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
    fn a_swing_costs_once_however_many_ticks_it_touches() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        let swing = |app: &mut App, sequence: u64| {
            app.world_mut()
                .resource_mut::<Messages<AttackRequest>>()
                .write(AttackRequest {
                    attacker: entity,
                    sequence,
                    kind: AttackKind::Melee {
                        action: Action::Dead,
                        from: 0.0,
                        to: 0.1,
                    },
                });
            app.update();
        };
        let carried = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&WearCarry, With<LocalPlayer>>()
                .single(app.world())
                .unwrap()
                .hands[0]
        };
        for _ in 0..3 {
            swing(&mut app, 7);
        }
        // Durability 500: a cut is the dial itself.
        let cut = app.world().resource::<Registries>().0.items.wear().cut;
        assert_eq!(carried(&mut app), cut);
        swing(&mut app, 8);
        assert_eq!(carried(&mut app), 2 * cut);
    }

    #[test]
    fn a_butt_strike_wears_the_weapon_once_like_a_cut() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<Messages<AttackRequest>>()
                .write(AttackRequest {
                    attacker: entity,
                    sequence: 4,
                    kind: AttackKind::Strike {
                        reach: 1.7,
                        arc: 50.,
                        damage: 8.,
                        posture: 70.,
                        on_hit: None,
                    },
                });
            app.update();
        }
        let cut = app.world().resource::<Registries>().0.items.wear().cut;
        let carried = app
            .world_mut()
            .query_filtered::<&WearCarry, With<LocalPlayer>>()
            .single(app.world())
            .unwrap()
            .hands[0];
        assert_eq!(carried, cut);
    }

    #[test]
    fn a_cut_wears_the_blade_that_made_it_and_a_worn_blade_cuts_for_less() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let blade = crate::crafting::bought(
            items
                .items
                .get(&strata_registry::Namespaced::parse("veyra:weapon.service_blade").unwrap())
                .unwrap(),
        );
        // The blade takes the pistol's hand and is drawn.
        put_hand(&mut app, 1, &blade);
        app.update();
        app.world_mut().get_mut::<Combatant>(entity).unwrap().weapon = Weapon::Blade;
        app.world_mut()
            .resource_mut::<Messages<AttackRequest>>()
            .write(AttackRequest {
                attacker: entity,
                sequence: 3,
                kind: AttackKind::Melee {
                    action: Action::Light,
                    from: 0.0,
                    to: 0.1,
                },
            });
        app.update();
        let carried = app
            .world_mut()
            .query_filtered::<&WearCarry, With<LocalPlayer>>()
            .single(app.world())
            .unwrap()
            .hands;
        assert_eq!(carried[0], 0, "the carbine did not make the cut");
        assert_eq!(carried[1], items.items.wear().cut, "the blade did");
        // Sound, it cuts at its edge's figure; worn to nothing, for less.
        let damage = |app: &mut App| {
            app.world()
                .get::<Combatant>(entity)
                .unwrap()
                .spec(Action::Light)
                .damage
        };
        app.update();
        let sound = damage(&mut app);
        assert!((sound - 28. * 0.95).abs() < 1e-4, "{sound}");
        let mut worn = hand(&mut app, 1);
        worn.condition = Some(0);
        put_hand(&mut app, 1, &worn);
        app.update();
        let worn = damage(&mut app);
        assert!((worn - 28. * 0.95 * 0.8).abs() < 1e-3, "{worn}");
        assert!(worn < sound);
    }

    #[test]
    fn condition_moves_in_steps_and_never_under_nought() {
        let mut app = first_day();
        // At durability 500 a shot is 2 thousandths of a point: a step of
        // 25 points is 12.5 shots.
        shoot(&mut app, 12);
        assert_eq!(hand(&mut app, 0).condition, Some(1000));
        shoot(&mut app, 1);
        assert_eq!(hand(&mut app, 0).condition, Some(975));
        let mut weapon = hand(&mut app, 0);
        weapon.condition = Some(10);
        put_hand(&mut app, 0, &weapon);
        shoot(&mut app, 40);
        assert_eq!(hand(&mut app, 0).condition, Some(0));
        shoot(&mut app, 40);
        assert_eq!(hand(&mut app, 0).condition, Some(0));
        // The pistol in the other hand was never touched.
        assert_eq!(hand(&mut app, 1).condition, Some(1000));
    }

    #[test]
    fn a_fall_knocks_everything_carried_once() {
        let mut app = first_day();
        let items = app.world().resource::<Registries>().0.clone();
        let mut worn = feed();
        worn.condition = Some(600);
        let mut spare = Record {
            item: worn.item.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: worn.lines.clone(),
            condition: worn.condition,
            fitted: BTreeMap::new(),
            origin: None,
        };
        put_in_pack(&mut app, &spare);
        spare.condition = None;
        put_in_pack(&mut app, &spare);
        let mut host = hand(&mut app, 0);
        host.fitted.insert("feed".to_owned(), feed());
        put_hand(&mut app, 0, &host);
        app.world_mut()
            .query_filtered::<&mut Combatant, With<LocalPlayer>>()
            .single_mut(app.world_mut())
            .unwrap()
            .action = Action::Dead;
        app.update();
        assert_eq!(app.world().resource::<Messages<WearNotice>>().len(), 1);
        for _ in 0..4 {
            app.update();
        }
        let weapon = hand(&mut app, 0);
        assert_eq!(weapon.condition, Some(850));
        assert_eq!(weapon.fitted["feed"].condition, Some(850));
        assert_eq!(hand(&mut app, 1).condition, Some(850));
        let (_, pack, _, _) = player(&mut app);
        let conditions: Vec<Option<u16>> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &items.items).ok())
            .filter(|record| record.item == id(FEED))
            .map(|record| record.condition)
            .collect();
        // The worn spare lost 150 and the unworn one, which reads full,
        // is knocked from there.
        assert!(conditions.contains(&Some(450)), "{conditions:?}");
        assert!(conditions.contains(&Some(850)), "{conditions:?}");
    }

    #[test]
    fn a_part_wears_with_its_host_and_keeps_it_when_moved() {
        let registries = shipped();
        let items = &registries.0.items;
        let rules = items.wear();
        let mut host = fitting::with(items, &carbine(items), "feed", Some(&feed())).unwrap();
        let mut carry = 0;
        // 750 thousandths short of a step, then past it.
        assert!(!accrue(rules, &mut host, &mut carry, 24_000));
        assert!(accrue(rules, &mut host, &mut carry, 1_000));
        assert_eq!(host.condition, Some(975));
        assert_eq!(host.fitted["feed"].condition, Some(975));
        let (bare, part) = fitting::without(items, &host, "feed").unwrap();
        assert_eq!(bare.condition, Some(975));
        assert_eq!(part.condition, Some(975));
        // Another carbine takes it as worn as it came off.
        let other = fitting::with(items, &carbine(items), "feed", Some(&part)).unwrap();
        assert_eq!(other.condition, Some(1000));
        assert_eq!(other.fitted["feed"].condition, Some(975));
        // And the loose stack it is put away as is a worn module.
        let stack = items::write(&fitting::loose_for_tests(&part), items).unwrap();
        assert_eq!(items::read(&stack, items).unwrap().condition, Some(975));
        through_the_stock();
    }

    /// A melee part wears with its blade, a worn one reads for less, and a
    /// bench repair mends it, by the rules a gun's part is under.
    #[test]
    fn a_fitted_melee_part_wears_with_its_blade_and_a_bench_repair_mends_it() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let blade =
            crate::crafting::bought(items.items.get(&id("veyra:weapon.service_blade")).unwrap());
        let hooked = Fitted {
            item: id("veyra:module.blade_hooked_edge"),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("hit_posture".to_owned(), 500),
                ("recovery_time".to_owned(), 500),
            ]),
            condition: None,
        };
        let mut host = fitting::with(&items.items, &blade, "edge", Some(&hooked)).unwrap();
        let rules = items.items.wear();
        let mut carry = 0;
        assert!(accrue(rules, &mut host, &mut carry, 25_000));
        assert_eq!(host.condition, Some(975));
        assert_eq!(host.fitted["edge"].condition, Some(975));
        // Sound, the part reads as made; worn to nothing, its posture is
        // less than made and still a benefit, its tail less than made too.
        let posture = |host: &Record| {
            fitting::factors(&items.items, host)
                .iter()
                .find(|factor| factor.property == Property::HitPosture)
                .unwrap()
                .factor
        };
        let sound = fitting::with(&items.items, &blade, "edge", Some(&hooked)).unwrap();
        assert_eq!(posture(&sound), 1.08);
        let mut worn = sound.clone();
        worn.fitted.get_mut("edge").unwrap().condition = Some(0);
        assert!(
            posture(&worn) < 1.08 && posture(&worn) > 1.,
            "{}",
            posture(&worn)
        );
        // At the bench a kit mends the blade and its part.
        put_in_pack(&mut app, &kit(900, 1));
        worn.condition = Some(120);
        worn.fitted.get_mut("edge").unwrap().condition = Some(80);
        put_hand(&mut app, 1, &worn);
        let done = repair_all(&mut app, &[1]);
        assert!(
            matches!(done.as_slice(), [Ok(Repaired { to: 1000, .. })]),
            "{done:?}"
        );
        let mended = hand(&mut app, 1);
        assert_eq!(mended.condition, Some(1000));
        assert_eq!(mended.fitted["edge"].condition, None);
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

    #[test]
    fn a_bench_repair_takes_the_poorest_kit_and_mends_everything() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        let mut worn =
            fitting::with(&items.items, &carbine(&items.items), "feed", Some(&feed())).unwrap();
        worn.condition = Some(120);
        worn.fitted.get_mut("feed").unwrap().condition = Some(80);
        put_hand(&mut app, 0, &worn);
        assert_eq!(kits_in(&mut app), [100, 500, 500, 900]);
        app.update();
        let worn_loadout = player(&mut app).3.loadout().cloned();
        assert!(
            worn_loadout.as_ref().is_some_and(|loadout| loadout
                .mods
                .values()
                .flatten()
                .any(|factor| factor.property == Property::RepeatInterval)),
            "{worn_loadout:?}"
        );
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 920,
                to: 1000,
                at_bench: true
            })]
        );
        assert_eq!(kits_in(&mut app), [500, 500, 900]);
        let weapon = hand(&mut app, 0);
        assert_eq!(weapon.condition, Some(1000));
        assert_eq!(weapon.fitted["feed"].condition, None);
        app.update();
        let never =
            fitting::with(&items.items, &carbine(&items.items), "feed", Some(&feed())).unwrap();
        let pistol = Some(carbine_pistol(&items));
        let expected = loadout(&items.items, &[Some(never), pistol]);
        assert_eq!(player(&mut app).3.loadout(), Some(&expected));
        assert_ne!(worn_loadout.as_ref(), Some(&expected));
    }

    fn carbine_pistol(items: &Arc<strata_registry::GameRegistries>) -> Record {
        items.items.starting().unwrap().hands[1].clone()
    }

    #[test]
    fn a_field_repair_takes_the_best_carried_kit_and_gains_what_the_dials_say() {
        let mut app = first_day();
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        // The store's kits are out of reach away from a workbench.
        let done = repair_all(&mut app, &[0]);
        // 400 + 600 * 900 / 1000 is 940, over what a weapon at 100 lacks.
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 900,
                to: 1000,
                at_bench: false
            })]
        );
        assert_eq!(kits_in(&mut app), [100, 500, 500]);
        // The next is the best left carried: line 100 is 460 points, and
        // the store kits stay put.
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 460,
                to: 560,
                at_bench: false
            })]
        );
        assert_eq!(kits_in(&mut app), [500, 500]);
        assert_eq!(hand(&mut app, 0).condition, Some(560));
    }

    #[test]
    fn a_repair_is_refused_for_an_empty_hand_a_sound_weapon_and_no_kit() {
        let mut app = first_day();
        bench(&mut app);
        assert_eq!(
            repair_all(&mut app, &[0]),
            [Err(RepairRefusal::NothingToRepair)]
        );
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Hands, With<LocalPlayer>>();
            players.single_mut(world).unwrap().0.set(1, None);
        }
        assert_eq!(repair_all(&mut app, &[1]), [Err(RepairRefusal::EmptyHand)]);
        // Both store kits spent.
        let done = repair_all(&mut app, &[0]);
        assert!(done[0].is_ok());
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert!(repair_all(&mut app, &[0])[0].is_ok());
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(repair_all(&mut app, &[0]), [Err(RepairRefusal::NoKit)]);
    }

    #[test]
    fn two_repairs_in_a_tick_take_two_kits_and_the_second_has_nothing_to_mend() {
        let mut app = first_day();
        bench(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(kits_in(&mut app), [500, 500]);
        let done = repair_all(&mut app, &[0, 0]);
        assert!(done[0].is_ok());
        assert_eq!(done[1], Err(RepairRefusal::NothingToRepair));
        // One kit went, and a second request on a still-worn weapon would
        // take the second.
        assert_eq!(kits_in(&mut app), [500]);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        let mut pistol = hand(&mut app, 1);
        pistol.condition = Some(300);
        put_hand(&mut app, 1, &pistol);
        put_in_pack(&mut app, &kit(500, 1));
        let done = repair_all(&mut app, &[0, 1]);
        assert!(done.iter().all(Result::is_ok));
        assert_eq!(kits_in(&mut app), Vec::<u16>::new());
    }

    /// The fit path carries a worn module's condition through the stock.
    fn through_the_stock() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let mut worn = feed();
        worn.condition = Some(300);
        let loose = fitting::loose_for_tests(&worn);
        put_in_pack(&mut app, &loose);
        app.world_mut()
            .resource_mut::<FitInbox>()
            .0
            .push(FitRequest::Fit {
                hand: 0,
                mount: "feed".to_owned(),
                module: worn.clone(),
            });
        app.update();
        assert_eq!(hand(&mut app, 0).fitted["feed"].condition, Some(300));
        app.world_mut()
            .resource_mut::<FitInbox>()
            .0
            .push(FitRequest::Remove {
                hand: 0,
                mount: "feed".to_owned(),
            });
        app.update();
        let (_, pack, _, _) = player(&mut app);
        let back = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &items.items).ok())
            .any(|record| record.item == id(FEED) && record.condition == Some(300));
        assert!(back, "the module came back as worn as it went");
    }

    /// A store grade bike of condition `condition` as the household's ride.
    fn stable_bike(app: &mut App, condition: u16) {
        let registries = app.world().resource::<Registries>().0.clone();
        let vehicle = id("veyra:vehicle.courier_bike");
        let mut bike = crate::ride::store_grade_bike(&registries.items, &vehicle).unwrap();
        bike.condition = Some(condition);
        *app.world_mut().resource_mut::<Stable>() =
            Stable::with_bike(bike, &registries.items, &registries.vehicles);
    }

    fn repair_bike_now(app: &mut App) -> Result<Repaired, RepairRefusal> {
        app.world_mut()
            .resource_mut::<WearInbox>()
            .0
            .push(WearRequest::RepairBike);
        app.update();
        let mut answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<RepairAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(answers.len(), 1);
        answers.pop().unwrap()
    }

    fn bike_condition(app: &App) -> Option<u16> {
        app.world()
            .resource::<Stable>()
            .bike
            .as_ref()
            .unwrap()
            .condition
    }

    /// The bike is mended by the same kits, the same field amount and the
    /// same bench rule as a weapon.
    #[test]
    fn a_bike_is_repaired_in_the_field_and_at_a_bench_like_a_weapon() {
        let mut app = first_day();
        stable_bike(&mut app, 100);
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        // Away from a bench: the best carried kit, 400 + 600 * 900 / 1000.
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 900,
                to: 1000,
                at_bench: false
            })
        );
        assert_eq!(bike_condition(&app), Some(1000));
        assert_eq!(kits_in(&mut app), [100, 500, 500]);
        // A sound bike has nothing to mend.
        assert_eq!(
            repair_bike_now(&mut app),
            Err(RepairRefusal::NothingToRepair)
        );
        // The next best is line 100: 460 points.
        stable_bike(&mut app, 100);
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 460,
                to: 560,
                at_bench: false
            })
        );
        assert_eq!(kits_in(&mut app), [500, 500]);
        // At a bench the poorest kit mends all of it.
        bench(&mut app);
        stable_bike(&mut app, 300);
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 700,
                to: 1000,
                at_bench: true
            })
        );
        assert_eq!(kits_in(&mut app), [500]);
        assert_eq!(bike_condition(&app), Some(1000));
    }

    /// No bike and no kit are refused, and the weapon in hand is not the
    /// bike.
    #[test]
    fn a_bike_repair_is_refused_without_a_bike_or_a_kit() {
        let mut app = first_day();
        assert_eq!(repair_bike_now(&mut app), Err(RepairRefusal::EmptyHand));
        stable_bike(&mut app, 100);
        // The store's kits are out of reach away from a bench; empty the pack.
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            for cell in 0..pack.cells.len() {
                pack.cells.set(cell, None);
            }
            pack.cells.clear_changed();
        }
        assert_eq!(repair_bike_now(&mut app), Err(RepairRefusal::NoKit));
        assert_eq!(bike_condition(&app), Some(100), "nothing moved");
    }

    /// The player's sheet: the boxes of `ids` and nothing else.
    fn learn(app: &mut App, ids: &[&str]) {
        let (entity, ..) = player(app);
        app.world_mut().entity_mut(entity).insert(CharacterSheet {
            learned: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..CharacterSheet::default()
        });
    }

    /// Repair 4 makes a shot cost a fifth less of the weapon in the bearer's
    /// hand; anyone without it, and an actor with no sheet, pay the dials.
    #[test]
    fn the_bearers_boxes_scale_what_a_shot_costs() {
        let spent = |boxes: Option<&[&str]>| {
            let mut app = first_day();
            if let Some(boxes) = boxes {
                learn(&mut app, boxes);
            }
            shoot(&mut app, 100);
            1000 - hand(&mut app, 0).condition.unwrap_or(1000)
        };
        let plain = spent(None);
        assert_eq!(spent(Some(&[])), plain, "a sheet with none reads the dials");
        assert_eq!(spent(Some(&["fabricator.novice"])), plain);
        let sparing = spent(Some(&["fabricator.repair.4"]));
        // A hundred shots of two points at the store grade, a fifth less,
        // in steps of the record's own.
        assert_eq!(plain, 200);
        assert_eq!(sparing, 150, "1.6 points a shot is 160, stepped by 25");
    }

    /// A field repair mends what the bearer's kit boxes say, and a workbench
    /// repair with the Repair 2 box takes no kit and is not refused for want
    /// of one.
    #[test]
    fn repair_boxes_scale_the_field_amount_and_a_bench_needs_no_kit() {
        // Repair 1 and 3: 400 + 600 * 500 / 1000 is 700, and a fifth and a
        // third more is 1092, held to a whole item.
        let mut app = first_day();
        learn(&mut app, &["fabricator.repair.1"]);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        put_in_pack(&mut app, &kit(500, 1));
        assert_eq!(
            repair_all(&mut app, &[0]),
            [Ok(Repaired {
                gain: 840,
                to: 940,
                at_bench: false
            })],
            "700 and a fifth"
        );
        // No kit anywhere, a workbench and the box: mended, nothing spent.
        let mut app = first_day();
        bench(&mut app);
        let before = kits_in(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        learn(&mut app, &["fabricator.repair.2"]);
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 700,
                to: 1000,
                at_bench: true
            })]
        );
        assert_eq!(kits_in(&mut app), before, "no kit was used");
        // The same member away from a bench still needs one.
        let mut app = first_day();
        learn(&mut app, &["fabricator.repair.2"]);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(repair_all(&mut app, &[0]), [Err(RepairRefusal::NoKit)]);
        // And without the box the bench still takes the poorest kit.
        let mut app = first_day();
        bench(&mut app);
        learn(&mut app, &["fabricator.novice"]);
        let before = kits_in(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert!(repair_all(&mut app, &[0])[0].is_ok());
        assert_eq!(kits_in(&mut app).len() + 1, before.len());
    }
}
