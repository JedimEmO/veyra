//! Procurement: what an institution buys of the household, and what it
//! pays (ticket 0007, step 7; `docs/design/crafting.md`, 7).
//!
//! A buyer's board lists lines (`strata_registry::procurement`): the item
//! or class of resource wanted, the grade it is paid by, the minimum, the
//! quantity still wanted and the price. A sale pays in full up to what is
//! wanted and the line then ends; an upkeep line refills with played time
//! ([`advance`]) and a project never does. The ledger ([`Sale`]) keeps who
//! bought what from which batch, because a later step may put that batch in
//! the buyer's wardens' hands.
//!
//! Every figure is whole numbers: a unit's pay is in thousandths of a
//! credit ([`pays`]) and a sale rounds down once, over the whole count.
//! [`check_sale`] and the pure functions beside it are what the request and
//! a screen both ask, so a button greys for the reason the sim would give.
//! Local-only like the stock it changes: nothing here is replicated.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use strata_core::SIM_TICK_HZ;

use strata_registry::item::{ItemCatalogue, ItemKind, LotOrigin, Record};

use strata_registry::procurement::{BuyerDef, LineSpec, OrderKind, Wants};

use strata_registry::{GameRegistries, Namespaced};

use crate::crafting::{self, Scratch, kind_of};

use crate::deeds::{LeaseRefusal, Leased, Terms};

use crate::deposits::Purse;

use crate::fitting::WorkbenchAccess;

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::progression::Reach;

/// The key a line is under in the state: the buyer's id, a slash, the
/// line's.
#[must_use]
pub fn key(buyer: &Namespaced, line: &str) -> String {
    format!("{buyer}/{line}")
}

/// One entry of the ledger: what was sold to one buyer's line of one kind
/// of stack. Batch, maker and origin are the record's own, so the buyer's
/// side can name the batch it was sold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sale {
    /// Who bought it.
    pub buyer: Namespaced,
    /// The line it was sold to.
    pub line: String,
    /// The item.
    pub item: Namespaced,
    /// The batch number; nought for a bought item or a lot.
    #[serde(default)]
    pub batch: u32,
    /// Who made it.
    #[serde(default)]
    pub by: Option<String>,
    /// The deposit of a lot.
    #[serde(default)]
    pub origin: Option<LotOrigin>,
    /// How many, in all sales of the kind.
    pub count: u32,
    /// What they paid, in credits.
    pub paid: u32,
}

impl Sale {
    /// Whether `sale` is of the same buyer, line and kind of stack.
    fn same_kind(&self, sale: &Self) -> bool {
        (
            &self.buyer,
            &self.line,
            &self.item,
            self.batch,
            &self.by,
            &self.origin,
        ) == (
            &sale.buyer,
            &sale.line,
            &sale.item,
            sale.batch,
            &sale.by,
            &sale.origin,
        )
    }
}

/// What the household has sold: how much of each line is taken, and the
/// ledger. The game saves it with the household.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcurementState {
    /// Units taken of each line, by [`key`]. Absent is nought.
    pub taken: BTreeMap<String, u32>,
    /// What was sold, a kind a line an entry, so the ledger grows by batch
    /// and not by click.
    pub sales: Vec<Sale>,
}

impl ProcurementState {
    /// Hold the state to the data as it now is: an entry for a buyer or a
    /// line the data no longer has is dropped and a count taken is held to
    /// its line's quantity. Said in words for a log, one for each thing
    /// changed.
    pub fn repair(&mut self, registries: &GameRegistries) -> Vec<String> {
        let mut said = Vec::new();
        let line_of = |buyer: &Namespaced, line: &str| {
            registries
                .buyers
                .get(buyer)
                .and_then(|buyer| buyer.line(line))
                .map(|(_, line)| line)
        };
        let known: BTreeMap<String, u32> = registries
            .buyers
            .iter()
            .flat_map(|buyer| {
                buyer
                    .lines()
                    .map(|(_, line)| (key(&buyer.id, &line.id), line.quantity))
            })
            .collect();
        self.taken.retain(|each, taken| {
            let Some(quantity) = known.get(each) else {
                said.push(format!(
                    "procurement: `{each}` is no line now; its count is dropped"
                ));
                return false;
            };
            if *taken > *quantity {
                said.push(format!(
                    "procurement: `{each}` is held to its quantity {quantity}"
                ));
                *taken = *quantity;
            }
            true
        });
        self.sales.retain(|sale| {
            let keep = line_of(&sale.buyer, &sale.line).is_some();
            if !keep {
                said.push(format!(
                    "procurement: a sale on `{}` is dropped; the data has no such line",
                    key(&sale.buyer, &sale.line)
                ));
            }
            keep
        });
        said
    }
}

/// How far each line is toward its next refill, in played time. Not saved:
/// under one refill interval is lost on exit, which is accepted, so the
/// file is not written every second.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcurementClock {
    /// Nominal ticks toward the next second.
    pub ticks: u32,
    /// Played seconds toward the next refill, by [`key`], for a line with
    /// something taken.
    pub seconds: BTreeMap<String, u32>,
}

/// How much of a line is still wanted.
#[must_use]
pub fn wanted(buyer: &BuyerDef, line: &LineSpec, state: &ProcurementState) -> u32 {
    let taken = state.taken.get(&key(&buyer.id, &line.id)).copied();
    line.quantity.saturating_sub(taken.unwrap_or(0))
}

/// How much of a line's quantity is taken, in thousandths, for a bar.
#[must_use]
pub fn taken_pm(buyer: &BuyerDef, line: &LineSpec, state: &ProcurementState) -> u16 {
    let taken = u64::from(line.quantity - wanted(buyer, line, state).min(line.quantity));
    let share = taken * 1000 / u64::from(line.quantity.max(1));
    u16::try_from(share).unwrap_or(1000)
}

/// The figure of `record` a line grades on; `None` when the record is not
/// what the line wants: another item, or a lot whose class is another.
#[must_use]
pub fn grade_of(items: &ItemCatalogue, line: &LineSpec, record: &Record) -> Option<u16> {
    let wants = match &line.wants {
        Wants::Item(item) => record.item == *item,
        Wants::Class(class) => matches!(
            items.get(&record.item).map(|def| &def.kind),
            Some(ItemKind::Lot { class: of, .. }) if of == class
        ),
    };
    // A worn thing's lines have slid, so it is not sold at its made grade.
    let worn = record
        .condition
        .is_some_and(|condition| condition < items.wear().slide_below);
    if !wants || worn {
        return None;
    }
    record.lines.get(line.grade.key()).copied()
}

/// Thousandths of a credit a unit of `grade` is paid: the price, and above
/// the minimum each point adds the buyer's premium of it up to the cap.
#[must_use]
pub fn pays(buyer: &BuyerDef, line: &LineSpec, grade: u16) -> u32 {
    let points = u64::from(grade.saturating_sub(line.min));
    let premium = (points * u64::from(buyer.premium.per_point)).min(u64::from(buyer.premium.cap));
    let price = u64::from(line.price);
    u32::try_from(price * 1000 + price * premium).unwrap_or(u32::MAX)
}

/// What `count` units at `unit` thousandths are paid, in credits: rounded
/// down once over the whole count, and never under one.
#[must_use]
pub fn pay_for(unit: u32, count: u32) -> u32 {
    let paid = u64::from(count) * u64::from(unit) / 1000;
    u32::try_from(paid)
        .unwrap_or(u32::MAX)
        .max(u32::from(count > 0))
}

/// A kind of stack in reach that a line would buy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The kind, as a record of one: what a sale names.
    pub kind: Record,
    /// How many of it the household holds in reach.
    pub held: u32,
    /// Its figure on the line's grade.
    pub grade: u16,
    /// What a unit pays, in thousandths of a credit ([`pays`]).
    pub unit: u32,
}

/// Every kind in reach the line wants, with its grade and the units held,
/// not yet held to the minimum.
fn kinds(
    items: &ItemCatalogue,
    line: &LineSpec,
    pack: &Pack,
    store: Option<&HouseholdStore>,
) -> Vec<(Record, u32, u16)> {
    let mut found: Vec<(Record, u32, u16)> = Vec::new();
    for (record, _) in crafting::reachable(items, pack, store) {
        let Some(grade) = grade_of(items, line, &record) else {
            continue;
        };
        let kind = kind_of(&record);
        match found.iter_mut().find(|(each, ..)| *each == kind) {
            Some((_, held, _)) => *held = held.saturating_add(record.count),
            None => found.push((kind, record.count, grade)),
        }
    }
    found
}

/// Every kind in reach the line buys, at or above its minimum, the best
/// pay first.
#[must_use]
pub fn offers(
    registries: &GameRegistries,
    buyer: &BuyerDef,
    line: &LineSpec,
    pack: &Pack,
    store: Option<&HouseholdStore>,
) -> Vec<Offer> {
    let mut offers: Vec<Offer> = kinds(&registries.items, line, pack, store)
        .into_iter()
        .filter(|(_, _, grade)| *grade >= line.min)
        .map(|(kind, held, grade)| Offer {
            kind,
            held,
            grade,
            unit: pays(buyer, line, grade),
        })
        .collect();
    offers.sort_by(|a, b| b.unit.cmp(&a.unit).then(b.held.cmp(&a.held)));
    offers
}

/// The best grade held among kinds of the right thing that are under the
/// minimum: "not met: 610".
#[must_use]
pub fn best_short(
    registries: &GameRegistries,
    line: &LineSpec,
    pack: &Pack,
    store: Option<&HouseholdStore>,
) -> Option<u16> {
    kinds(&registries.items, line, pack, store)
        .into_iter()
        .map(|(_, _, grade)| grade)
        .filter(|grade| *grade < line.min)
        .max()
}

/// Whether a board of `buyer` is within reach, or the run says from
/// anywhere: the rule a sale is held to, and what a screen greys by.
#[must_use]
pub fn at_board(access: WorkbenchAccess, reach: &Reach, buyer: &Namespaced) -> bool {
    access == WorkbenchAccess::Anywhere || reach.board.as_ref() == Some(buyer)
}

/// Why a sale is not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaleRefusal {
    /// No board of that buyer is in reach.
    NoBoard,
    /// The buyer or the line is not in the data.
    UnknownLine,
    /// Nothing more is wanted.
    Filled,
    /// The kind is not what the line wants.
    NotWanted,
    /// The kind grades under the minimum.
    BelowMinimum {
        /// Its grade.
        has: u16,
        /// The line's minimum.
        needs: u16,
    },
    /// No stack of the kind is in reach.
    NoneHeld,
    /// The board's holder wants the household gone and will not trade.
    Hostile,
}

/// What a sale would be: how many go and what they pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    /// The units sold: the least of the count asked, the units held and
    /// what the line still wants.
    pub count: u32,
    /// Credits paid for them.
    pub paid: u32,
}

/// Whether `count` of `kind` can be sold to a line, and for how much: the
/// pure rule the request and a screen both ask, at the data's own prices.
///
/// # Errors
/// A [`SaleRefusal`], the first that applies in the order it is declared.
#[allow(
    clippy::too_many_arguments,
    reason = "the board, the line, what was taken, the stock and the sale: each its own thing"
)]
pub fn check_sale(
    registries: &GameRegistries,
    at_board: bool,
    buyer: &Namespaced,
    line: &str,
    state: &ProcurementState,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
    count: u32,
) -> Result<Quote, SaleRefusal> {
    check_sale_on(
        registries,
        at_board,
        Terms::NEUTRAL,
        buyer,
        line,
        state,
        pack,
        store,
        kind,
        count,
    )
}

/// [`check_sale`] at a board's `terms`: what its holder's stance makes of the
/// pay, or a refusal where it will not trade.
///
/// # Errors
/// A [`SaleRefusal`], the first that applies in the order it is declared.
#[allow(
    clippy::too_many_arguments,
    reason = "the board, its terms, the line, what was taken, the stock and the sale: each its own thing"
)]
pub fn check_sale_on(
    registries: &GameRegistries,
    at_board: bool,
    terms: Terms,
    buyer: &Namespaced,
    line: &str,
    state: &ProcurementState,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
    count: u32,
) -> Result<Quote, SaleRefusal> {
    if !at_board {
        return Err(SaleRefusal::NoBoard);
    }
    if terms.refuses {
        return Err(SaleRefusal::Hostile);
    }
    let (buyer, line) = registries
        .buyers
        .get(buyer)
        .and_then(|buyer| Some((buyer, buyer.line(line)?.1)))
        .ok_or(SaleRefusal::UnknownLine)?;
    let left = wanted(buyer, line, state);
    if left == 0 {
        return Err(SaleRefusal::Filled);
    }
    let grade = grade_of(&registries.items, line, kind).ok_or(SaleRefusal::NotWanted)?;
    let kind = kind_of(kind);
    let held = kinds(&registries.items, line, pack, store)
        .into_iter()
        .find(|(each, ..)| *each == kind)
        .map_or(0, |(_, held, _)| held);
    if held == 0 {
        return Err(SaleRefusal::NoneHeld);
    }
    if grade < line.min {
        return Err(SaleRefusal::BelowMinimum {
            has: grade,
            needs: line.min,
        });
    }
    let count = count.min(held).min(left);
    if count == 0 {
        return Err(SaleRefusal::NoneHeld);
    }
    Ok(Quote {
        count,
        paid: pay_for(Terms::scale(pays(buyer, line, grade), terms.pays_pm), count),
    })
}

/// What a sale did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sold {
    /// The buyer.
    pub buyer: Namespaced,
    /// The line.
    pub line: String,
    /// The item.
    pub item: Namespaced,
    /// How many went.
    pub count: u32,
    /// What they paid, in credits.
    pub paid: u32,
}

/// Why a purchase at the board is not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseRefusal {
    /// No board of that buyer is in reach.
    NotAtBoard,
    /// The buyer does not sell that.
    NotSold,
    /// The purse is short of the price.
    Short {
        /// The price, in credits.
        needs: u32,
        /// The purse.
        have: u32,
    },
    /// Neither the pack nor the store has a cell for it.
    NoRoom,
    /// The board's holder wants the household gone and will not trade.
    Hostile,
}

/// What a purchase did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bought {
    /// The buyer.
    pub buyer: Namespaced,
    /// The item.
    pub item: Namespaced,
    /// What it cost, in credits.
    pub paid: u32,
}

/// The stack one `item` bought of `buyer` is, and its price: store grade,
/// every line 500 and, for a bike or a weapon, whole. Gives `NotSold`
/// unless the buyer's `sells` lists it.
fn wares(
    registries: &GameRegistries,
    terms: Terms,
    buyer: &Namespaced,
    item: &Namespaced,
) -> Result<(slotted_model::ItemStack, u32), PurchaseRefusal> {
    let sale = registries
        .buyers
        .get(buyer)
        .and_then(|buyer| buyer.sells.iter().find(|sale| sale.item == *item))
        .ok_or(PurchaseRefusal::NotSold)?;
    let def = registries.items.get(item).ok_or(PurchaseRefusal::NotSold)?;
    let stack = items::write(&crafting::bought(def), &registries.items)
        .map_err(|_| PurchaseRefusal::NotSold)?;
    Ok((stack, Terms::scale(sale.price, terms.sells_pm)))
}

/// Whether one `item` can be bought of `buyer` now, and for how much: the
/// pure rule the request and a screen both ask, at the data's own prices.
/// The store is the one the board reaches, as it is for a sale.
///
/// # Errors
/// A [`PurchaseRefusal`], the first that applies in the order it is
/// declared.
pub fn check_purchase(
    registries: &GameRegistries,
    at_board: bool,
    purse: &Purse,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    buyer: &Namespaced,
    item: &Namespaced,
) -> Result<u32, PurchaseRefusal> {
    check_purchase_on(
        registries,
        at_board,
        Terms::NEUTRAL,
        purse,
        pack,
        store,
        buyer,
        item,
    )
}

/// [`check_purchase`] at a board's `terms`.
///
/// # Errors
/// A [`PurchaseRefusal`], the first that applies in the order it is
/// declared.
#[allow(
    clippy::too_many_arguments,
    reason = "the board and its terms, the purse, the stock and the ware: each its own thing"
)]
pub fn check_purchase_on(
    registries: &GameRegistries,
    at_board: bool,
    terms: Terms,
    purse: &Purse,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    buyer: &Namespaced,
    item: &Namespaced,
) -> Result<u32, PurchaseRefusal> {
    if !at_board {
        return Err(PurchaseRefusal::NotAtBoard);
    }
    if terms.refuses {
        return Err(PurchaseRefusal::Hostile);
    }
    let (stack, price) = wares(registries, terms, buyer, item)?;
    if price > purse.0 {
        return Err(PurchaseRefusal::Short {
            needs: price,
            have: purse.0,
        });
    }
    let mut scratch = Scratch {
        pack: pack.clone(),
        hands: Hands::default(),
        store: store.cloned(),
    };
    scratch
        .put_away(&registries.items, stack)
        .map_err(|_| PurchaseRefusal::NoRoom)?;
    Ok(price)
}

/// Buy one `item` of `buyer`: the price leaves the purse and the record
/// goes to the pack, its bags, then the store, by the bench's own put-away
/// rule. One transaction: a refusal has changed nothing.
///
/// # Errors
/// What [`check_purchase`] refuses.
pub fn buy(
    registries: &GameRegistries,
    at_board: bool,
    stock: &mut Stock<'_>,
    purse: &mut Purse,
    buyer: &Namespaced,
    item: &Namespaced,
) -> Result<Bought, PurchaseRefusal> {
    buy_on(
        registries,
        at_board,
        Terms::NEUTRAL,
        stock,
        purse,
        buyer,
        item,
    )
}

/// [`buy`] at a board's `terms`.
///
/// # Errors
/// What [`check_purchase_on`] refuses.
#[allow(
    clippy::too_many_arguments,
    reason = "the board and its terms, the stock, the purse and the ware"
)]
pub fn buy_on(
    registries: &GameRegistries,
    at_board: bool,
    terms: Terms,
    stock: &mut Stock<'_>,
    purse: &mut Purse,
    buyer: &Namespaced,
    item: &Namespaced,
) -> Result<Bought, PurchaseRefusal> {
    let paid = check_purchase_on(
        registries,
        at_board,
        terms,
        purse,
        stock.pack,
        stock.store.as_deref(),
        buyer,
        item,
    )?;
    let (bought, _) = wares(registries, terms, buyer, item)?;
    let mut scratch = Scratch::of(stock);
    scratch
        .put_away(&registries.items, bought)
        .map_err(|_| PurchaseRefusal::NoRoom)?;
    scratch.commit(stock);
    purse.0 -= paid;
    Ok(Bought {
        buyer: buyer.clone(),
        item: item.clone(),
        paid,
    })
}

/// The sim's answer to a `ProcurementRequest`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct SaleAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Sold, SaleRefusal>,
}

/// The sim's answer to a `ProcurementRequest::Buy`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct PurchaseAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Bought, PurchaseRefusal>,
}

/// The sim's answer to a `ProcurementRequest::Lease`.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct LeaseAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Leased, LeaseRefusal>,
}

/// Take a sale off the stock and put it on the books. One transaction on
/// the caller's copies, in the order the stock gives: the pack first, then
/// its bags, then the store.
///
/// # Errors
/// What [`check_sale`] refuses.
#[allow(
    clippy::too_many_arguments,
    reason = "the stock, the books, the purse and the sale"
)]
pub fn sell(
    registries: &GameRegistries,
    at_board: bool,
    stock: &mut Stock<'_>,
    state: &mut ProcurementState,
    purse: &mut Purse,
    buyer: &Namespaced,
    line: &str,
    kind: &Record,
    count: u32,
) -> Result<Sold, SaleRefusal> {
    sell_on(
        registries,
        at_board,
        Terms::NEUTRAL,
        stock,
        state,
        purse,
        buyer,
        line,
        kind,
        count,
    )
}

/// [`sell`] at a board's `terms`.
///
/// # Errors
/// What [`check_sale_on`] refuses.
#[allow(
    clippy::too_many_arguments,
    reason = "the board and its terms, the stock, the books, the purse and the sale"
)]
pub fn sell_on(
    registries: &GameRegistries,
    at_board: bool,
    terms: Terms,
    stock: &mut Stock<'_>,
    state: &mut ProcurementState,
    purse: &mut Purse,
    buyer: &Namespaced,
    line: &str,
    kind: &Record,
    count: u32,
) -> Result<Sold, SaleRefusal> {
    let items = &registries.items;
    let quote = check_sale_on(
        registries,
        at_board,
        terms,
        buyer,
        line,
        state,
        stock.pack,
        stock.store.as_deref(),
        kind,
        count,
    )?;
    let one = kind_of(kind);
    let wanted = items::write(&one, items).map_err(|_| SaleRefusal::NotWanted)?;
    let mut scratch = Scratch::of(stock);
    if !scratch.take(items, &wanted, quote.count) {
        return Err(SaleRefusal::NoneHeld);
    }
    scratch.commit(stock);
    purse.0 = purse.0.saturating_add(quote.paid);
    let taken = state.taken.entry(key(buyer, line)).or_insert(0);
    *taken = taken.saturating_add(quote.count);
    let sale = Sale {
        buyer: buyer.clone(),
        line: line.to_owned(),
        item: one.item.clone(),
        batch: one.batch,
        by: one.by.clone(),
        origin: one.origin.clone(),
        count: quote.count,
        paid: quote.paid,
    };
    // A repeat moves its entry to the end, so the ledger's order is the order
    // of the latest sale and the screen's newest first is true.
    let mut sale = sale;
    if let Some(at) = state.sales.iter().position(|each| each.same_kind(&sale)) {
        let earlier = state.sales.remove(at);
        sale.count = sale.count.saturating_add(earlier.count);
        sale.paid = sale.paid.saturating_add(earlier.paid);
    }
    state.sales.push(sale);
    Ok(Sold {
        buyer: buyer.clone(),
        line: line.to_owned(),
        item: one.item,
        count: quote.count,
        paid: quote.paid,
    })
}

/// Let `ticks` nominal ticks of played time pass: each upkeep line with
/// something taken gets its refill back every interval, never past nought
/// taken, and a line with nothing taken carries no seconds. A project never
/// refills: filled is filled. Whether anything taken changed.
pub fn advance(
    registries: &GameRegistries,
    state: &mut ProcurementState,
    clock: &mut ProcurementClock,
    ticks: u32,
) -> bool {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the sim's rate, twenty"
    )]
    let nominal = SIM_TICK_HZ.round().max(1.) as u32;
    clock.ticks = clock.ticks.saturating_add(ticks);
    let seconds = clock.ticks / nominal;
    clock.ticks %= nominal;
    let mut moved = false;
    for buyer in registries.buyers.iter() {
        for (order, line) in buyer.lines() {
            let line_key = key(&buyer.id, &line.id);
            let (OrderKind::Upkeep, Some(refill)) = (order.kind, line.refill) else {
                continue;
            };
            let Some(taken) = state.taken.get_mut(&line_key).filter(|taken| **taken > 0) else {
                clock.seconds.remove(&line_key);
                continue;
            };
            let carried = clock.seconds.entry(line_key.clone()).or_insert(0);
            *carried = carried.saturating_add(seconds);
            let intervals = *carried / refill.every_s;
            *carried %= refill.every_s;
            if intervals > 0 {
                *taken = taken.saturating_sub(refill.units.saturating_mul(intervals));
                moved = true;
            }
            if *taken == 0 {
                clock.seconds.remove(&line_key);
            }
        }
    }
    state.taken.retain(|_, taken| *taken > 0);
    moved
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

    use crate::crafting::test_support::CORE_ITEM;
    use crate::crafting::test_support::id;

    use crate::crafting::test_support::part;
    use crate::crafting::test_support::shipped;

    use strata_registry::Namespaced;

    use strata_registry::item::Record;

    use veyra_world::factions::FactionId;

    pub const BUYER: &str = "veyra:buyer.pelagic_works";

    pub const NOW: &str = "Pelagic Works";

    pub fn buyer() -> Namespaced {
        id(BUYER)
    }

    pub fn cores(count: u32, density: u16) -> Record {
        part(&shipped(), CORE_ITEM, count, Some((412, &[density, 600])))
    }

    #[test]
    fn pay_is_the_price_a_premium_and_a_cap() {
        let registries = shipped();
        let set = &registries.0.buyers;
        let works = set.get(&buyer()).unwrap();
        assert_eq!(works.name, NOW);
        let (_, line) = works.line("cell_cores").unwrap();
        assert_eq!(pays(works, line, 500), 120_000);
        assert_eq!(pays(works, line, 724), 146_880);
        assert_eq!(pay_for(pays(works, line, 724), 40), 5_875);
        // Past the cap: 300 thousandths of the price on top, and no more.
        assert_eq!(pays(works, line, 800), 156_000);
        assert_eq!(pays(works, line, 1000), 156_000);
        // A single unit of the cheapest line still pays a credit.
        assert_eq!(pay_for(500, 1), 1);
    }

    pub const BIKE: &str = "veyra:vehicle.courier_bike";

    pub fn pelagic() -> FactionId {
        crate::deeds::test_support::pelagic().1.id
    }
}
