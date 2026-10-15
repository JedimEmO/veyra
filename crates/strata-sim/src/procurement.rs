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

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_core::SIM_TICK_HZ;
use strata_registry::item::{ItemCatalogue, ItemKind, LotOrigin, Record};
use strata_registry::procurement::{BuyerDef, LineSpec, OrderKind, Wants};
use strata_registry::{GameRegistries, Namespaced};
use strata_worldgen::factions::FactionId;

use crate::crafting::{self, Scratch, kind_of};
use crate::deeds::{LeaseRefusal, Leased, Ledger, Terms, lease_buyer};
use crate::deposits::Purse;
use crate::fitting::WorkbenchAccess;
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};
use crate::progression::Reach;
use crate::{LocalPlayer, Registries, SimClock, SimSet, SimTick};

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

/// What a screen asks of procurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcurementRequest {
    /// Buy one of what the buyer sells.
    Buy {
        /// The buyer.
        buyer: Namespaced,
        /// The item.
        item: Namespaced,
    },
    /// Sell up to `count` of `kind` to a line.
    Sell {
        /// The buyer.
        buyer: Namespaced,
        /// The line.
        line: String,
        /// The kind of stack, as an offer names it.
        kind: Record,
        /// How many at most.
        count: u32,
    },
    /// Buy a lease of a faction's concessions at its board.
    Lease {
        /// The faction whose concessions it opens.
        faction: FactionId,
    },
    /// Take a posting the buyer's board shows, to clear a camp for its pay.
    /// Answered by [`crate::postings::consume`], which runs first.
    TakeJob {
        /// The buyer.
        buyer: Namespaced,
        /// The camp's site key.
        key: u64,
    },
    /// Collect the pay of a job whose camp is cleared. Answered by
    /// [`crate::postings::consume`].
    Collect {
        /// The buyer.
        buyer: Namespaced,
        /// The camp's site key.
        key: u64,
    },
}

/// The requests not yet consumed, a resource for the reason the stock's
/// inbox is one.
#[derive(Resource, Debug, Default)]
pub struct ProcurementInbox(pub Vec<ProcurementRequest>);

/// The sim's answer to a [`ProcurementRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct SaleAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Sold, SaleRefusal>,
}

/// The sim's answer to a [`ProcurementRequest::Buy`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct PurchaseAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Bought, PurchaseRefusal>,
}

/// The sim's answer to a [`ProcurementRequest::Lease`].
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

/// `SimTick`, `SimSet::Input`, after the wear requests: every sale asked
/// for since the last tick, in order, each on the stock and the books the
/// one before left, so two in one tick cannot take a line past its
/// quantity. The store is to hand with a board in reach, as it is at a
/// bench.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock, the books and the purse in, two answers out"
)]
#[allow(clippy::too_many_lines, reason = "one arm a request, then the resync")]
pub fn consume(
    mut inbox: ResMut<ProcurementInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    mut state: ResMut<ProcurementState>,
    mut purse: ResMut<Purse>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands), With<LocalPlayer>>,
    mut answers: MessageWriter<SaleAnswer>,
    mut purchases: MessageWriter<PurchaseAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut leases: MessageWriter<LeaseAnswer>,
    mut ledger: Ledger,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let mut player = players.single_mut().ok();
    let mut changed = false;
    for request in requests {
        let (buyer, line, kind, count) = match request {
            ProcurementRequest::Sell {
                buyer,
                line,
                kind,
                count,
            } => (buyer, line, kind, count),
            // The postings' own system took these out of the inbox first.
            ProcurementRequest::TakeJob { .. } | ProcurementRequest::Collect { .. } => continue,
            ProcurementRequest::Lease { faction } => {
                let result = match ledger.map().and_then(|map| map.faction(faction)) {
                    None => Err(LeaseRefusal::NoLease),
                    Some(faction) => {
                        let board = lease_buyer(&registries.0, &faction)
                            .is_some_and(|buyer| at_board(*access, &reach, &buyer));
                        // A guard of the faction's concession sells it too.
                        let guard = reach.guards.contains(&faction.id);
                        let mut paid = *purse;
                        let done = ledger.lease(&registries.0, board || guard, &mut paid, &faction);
                        if done.is_ok() {
                            *purse = paid;
                        }
                        done
                    }
                };
                leases.write(LeaseAnswer { result });
                continue;
            }
            ProcurementRequest::Buy { buyer, item } => {
                let terms = ledger.terms(&registries.0, &buyer);
                let board = at_board(*access, &reach, &buyer);
                let result = match player.as_mut() {
                    None => Err(PurchaseRefusal::NoRoom),
                    Some((pack, hands)) => {
                        let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                        let mut paid = *purse;
                        let mut stock = Stock {
                            saddle: None,
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: Some(&mut next.2),
                        };
                        let done = buy_on(
                            &registries.0,
                            board,
                            terms,
                            &mut stock,
                            &mut paid,
                            &buyer,
                            &item,
                        );
                        if done.is_ok() {
                            changed = true;
                            if **pack != next.0 {
                                **pack = next.0;
                            }
                            **hands = next.1;
                            if *store != next.2 {
                                *store = next.2;
                            }
                            *purse = paid;
                        }
                        done
                    }
                };
                purchases.write(PurchaseAnswer { result });
                continue;
            }
        };
        let terms = ledger.terms(&registries.0, &buyer);
        let board = at_board(*access, &reach, &buyer);
        let result = match player.as_mut() {
            None => Err(SaleRefusal::NoneHeld),
            Some((pack, hands)) => {
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut books = (state.clone(), *purse);
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = sell_on(
                    &registries.0,
                    board,
                    terms,
                    &mut stock,
                    &mut books.0,
                    &mut books.1,
                    &buyer,
                    &line,
                    &kind,
                    count,
                );
                if let Ok(sold) = &done {
                    changed = true;
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                    *state = books.0;
                    *purse = books.1;
                    ledger.sold(&registries.0, &buyer, sold.paid);
                }
                done
            }
        };
        answers.write(SaleAnswer { result });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some((pack, hands)) = player.as_mut()
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

/// `SimTick`, `SimSet::Input`, after [`consume`]: played time refills the
/// upkeep lines, as many nominal ticks as this step of the sim is worth
/// ([`crate::factory::nominal_ticks`]). The state is marked changed only
/// when a count moves, which is once an interval: the household saves on a
/// change.
pub fn refill(
    registries: Res<Registries>,
    sim: Option<Res<SimClock>>,
    mut state: ResMut<ProcurementState>,
    mut clock: ResMut<ProcurementClock>,
    mut phase: Local<u32>,
) {
    if state.taken.is_empty() {
        return;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let ticks = crate::factory::nominal_ticks(&mut phase, stepped);
    if advance(
        &registries.0,
        state.bypass_change_detection(),
        &mut clock,
        ticks,
    ) {
        state.set_changed();
    }
}

/// Registers the books, the clock and the request. Added by `SimPlugin`,
/// so a server and a harness have it too; with no request and nothing
/// taken it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct ProcurementPlugin;

impl Plugin for ProcurementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProcurementInbox>()
            .init_resource::<ProcurementState>()
            .init_resource::<ProcurementClock>()
            .add_message::<SaleAnswer>()
            .add_message::<PurchaseAnswer>()
            .add_message::<LeaseAnswer>()
            .add_systems(
                SimTick,
                (consume, refill)
                    .chain()
                    .in_set(SimSet::Input)
                    .after(crate::wear::consume),
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
    use crate::crafting::tests::{
        ALLOY, BRINE, CORE_ITEM, first_day, id, lot, part, prop, shipped,
    };
    use crate::deeds::{Deeds, Leases};
    use crate::progression::InteractionKind;
    use bevy::ecs::message::Messages;
    use bevy::prelude::App;

    const BUYER: &str = "veyra:buyer.pelagic_works";
    const NOW: &str = "Pelagic Works";

    fn buyer() -> Namespaced {
        id(BUYER)
    }

    fn cores(count: u32, density: u16) -> Record {
        part(&shipped(), CORE_ITEM, count, Some((412, &[density, 600])))
    }

    fn board(app: &mut App) {
        prop(app, InteractionKind::Board { buyer: buyer() });
    }

    fn store_in(app: &mut App, record: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let stack = items::write(record, &registries.0.items).unwrap();
        let mut store = app.world_mut().resource_mut::<HouseholdStore>();
        assert!(items::place_in(&registries.0.items, &mut store.0, stack).is_none());
    }

    fn pack_in(app: &mut App, record: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let stack = items::write(record, &registries.0.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        pack.cells.set(cell, Some(stack));
    }

    /// One tick with every `(line, kind, count)` asked, in order.
    fn sell_all(app: &mut App, asks: &[(&str, &Record, u32)]) -> Vec<Result<Sold, SaleRefusal>> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .0
            .extend(
                asks.iter()
                    .map(|(line, kind, count)| ProcurementRequest::Sell {
                        buyer: buyer(),
                        line: (*line).to_owned(),
                        kind: (*kind).clone(),
                        count: *count,
                    }),
            );
        app.update();
        app.world_mut()
            .resource_mut::<Messages<SaleAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect()
    }

    fn held(app: &mut App, kind: &Record) -> (u32, u32) {
        let items = app.world().resource::<Registries>().0.items.clone();
        let (pack, _, store, ..) = crate::crafting::tests::truth(app);
        (
            crate::crafting::tests::units(&pack.cells, kind, &items),
            crate::crafting::tests::units(&store.0, kind, &items),
        )
    }

    fn purse(app: &App) -> u32 {
        app.world().resource::<Purse>().0
    }

    fn state(app: &App) -> ProcurementState {
        app.world().resource::<ProcurementState>().clone()
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

    #[test]
    fn a_sale_ends_at_the_quantity_and_leaves_the_rest() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 100)]);
        let sold = done[0].clone().unwrap();
        assert_eq!((sold.count, sold.paid), (40, 5_875));
        assert_eq!(purse(&app), before + 5_875);
        assert_eq!(held(&mut app, &kind), (0, 60));
        let again = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(again, vec![Err(SaleRefusal::Filled)]);
        assert_eq!(held(&mut app, &kind), (0, 60));
    }

    #[test]
    fn two_sales_in_one_tick_cannot_oversell_a_line() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let done = sell_all(
            &mut app,
            &[("cell_cores", &kind, 30), ("cell_cores", &kind, 30)],
        );
        let counts: Vec<_> = done
            .iter()
            .map(|each| each.clone().unwrap().count)
            .collect();
        assert_eq!(counts, vec![30, 10]);
        assert_eq!(state(&app).taken.values().sum::<u32>(), 40);
    }

    #[test]
    fn a_kind_under_the_minimum_is_refused_with_its_figure() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(10, 450);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(
            done,
            vec![Err(SaleRefusal::BelowMinimum {
                has: 450,
                needs: 500
            })]
        );
        assert_eq!(purse(&app), before);
        let registries = shipped();
        let works = registries.0.buyers.get(&buyer()).unwrap();
        let (_, line) = works.line("cell_cores").unwrap();
        let (pack, _, store, ..) = crate::crafting::tests::truth(&mut app);
        assert!(offers(&registries.0, works, line, &pack, Some(&store)).is_empty());
        assert_eq!(
            best_short(&registries.0, line, &pack, Some(&store)),
            Some(450)
        );
        // A magazine worn under the slide is not graded at its made figure.
        let (_, feed) = works.line("patrol_magazines").unwrap();
        let mut mag = part(
            &registries,
            "veyra:module.patrol_extended_feed",
            1,
            Some((1, &[610, 500])),
        );
        assert_eq!(grade_of(&registries.0.items, feed, &mag), Some(610));
        mag.condition = Some(0);
        assert_eq!(grade_of(&registries.0.items, feed, &mag), None);
    }

    #[test]
    fn a_lot_sells_by_units_and_only_of_its_class() {
        let mut app = first_day();
        board(&mut app);
        let registries = shipped();
        let mut ore = lot(
            &registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        );
        ore.lines.insert("strength".to_owned(), 550);
        let brine = lot(&registries, BRINE, "Oxbow", 160, &[534, 605, 495]);
        store_in(&mut app, &ore);
        let before = purse(&app);
        let done = sell_all(
            &mut app,
            &[
                ("structural_metal", &brine, 50),
                ("structural_metal", &ore, 1000),
            ],
        );
        assert_eq!(done[0], Err(SaleRefusal::NotWanted));
        // 200 are in hand, 600 wanted; 2 a unit and 100 points over the minimum.
        let sold = done[1].clone().unwrap();
        assert_eq!((sold.count, sold.paid), (200, 200 * 2 * 1100 / 1000));
        assert_eq!(purse(&app), before + sold.paid);
    }

    #[test]
    fn stock_leaves_the_pack_before_the_store() {
        let mut app = first_day();
        board(&mut app);
        pack_in(&mut app, &cores(30, 724));
        store_in(&mut app, &cores(100, 724));
        let kind = cores(1, 724);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 40)]);
        assert_eq!(done[0].clone().unwrap().count, 40);
        assert_eq!(held(&mut app, &kind), (0, 90));
    }

    #[test]
    fn the_ledger_names_the_batch_and_adds_a_second_sale() {
        let mut app = first_day();
        board(&mut app);
        store_in(&mut app, &cores(100, 724));
        let kind = cores(1, 724);
        sell_all(&mut app, &[("cell_cores", &kind, 15)]);
        sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        let registries = shipped();
        let mut ore = lot(
            &registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        );
        ore.lines.insert("strength".to_owned(), 550);
        pack_in(&mut app, &ore);
        sell_all(&mut app, &[("structural_metal", &ore, 20)]);
        let sales = state(&app).sales;
        assert_eq!(sales.len(), 2);
        let core = &sales[0];
        assert_eq!(
            (
                core.buyer.clone(),
                core.line.as_str(),
                core.item.clone(),
                core.batch
            ),
            (buyer(), "cell_cores", id(CORE_ITEM), 412)
        );
        assert_eq!(
            (core.by.as_deref(), core.count),
            (Some("Frontier ranger"), 25)
        );
        assert_eq!(core.paid, pay_for(146_880, 15) + pay_for(146_880, 10));
        assert_eq!(sales[1].origin, ore.origin);
        assert_eq!((sales[1].batch, sales[1].count), (0, 20));
        // A repeat moves to the end, so the vector is oldest first.
        sell_all(&mut app, &[("cell_cores", &kind, 5)]);
        let sales = state(&app).sales;
        assert_eq!((sales[1].batch, sales[1].count), (412, 30));
    }

    #[test]
    fn upkeep_refills_with_played_time_and_a_project_never_does() {
        let registries = shipped();
        let mut state = ProcurementState::default();
        let mut clock = ProcurementClock::default();
        let cores = key(&buyer(), "cell_cores");
        let intake = key(&buyer(), "intake_metal");
        state.taken.insert(cores.clone(), 10);
        state.taken.insert(intake.clone(), 2000);
        let second = 20;
        assert!(!advance(
            &registries.0,
            &mut state,
            &mut clock,
            599 * second
        ));
        assert_eq!(state.taken[&cores], 10);
        assert!(advance(&registries.0, &mut state, &mut clock, second));
        assert_eq!(state.taken[&cores], 6);
        advance(&registries.0, &mut state, &mut clock, 600 * second);
        advance(&registries.0, &mut state, &mut clock, 600 * second);
        // Never below nought, and a line with nothing taken carries no time.
        assert!(!state.taken.contains_key(&cores));
        assert!(!clock.seconds.contains_key(&cores));
        advance(&registries.0, &mut state, &mut clock, 100_000 * second);
        assert_eq!(state.taken[&intake], 2000);
    }

    #[test]
    fn no_board_in_reach_is_no_sale() {
        let mut app = first_day();
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(done, vec![Err(SaleRefusal::NoBoard)]);
        assert_eq!(held(&mut app, &kind), (0, 100));
        assert!(state(&app).sales.is_empty());
    }

    const BIKE: &str = "veyra:vehicle.courier_bike";

    fn buy_one(app: &mut App, item: &str) -> Result<Bought, PurchaseRefusal> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .0
            .push(ProcurementRequest::Buy {
                buyer: buyer(),
                item: id(item),
            });
        app.update();
        let mut answers: Vec<PurchaseAnswer> = app
            .world_mut()
            .resource_mut::<Messages<PurchaseAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        answers.pop().unwrap().result
    }

    fn snapshot(app: &mut App) -> (Pack, HouseholdStore, Purse) {
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&Pack, With<LocalPlayer>>();
        let pack = packs.single(world).unwrap().clone();
        (
            pack,
            world.resource::<HouseholdStore>().clone(),
            *world.resource::<Purse>(),
        )
    }

    /// Every cell of the pack, its open bags and the store taken.
    fn fill_everything(app: &mut App) {
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = packs.single_mut(world).unwrap();
        let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
            .map(|bag| usize::from(pack.open_cells(bag, items)))
            .collect();
        let all = pack.cells.len();
        crate::crafting::tests::fill(&mut pack.cells, all, items);
        for (bag, open) in pack.bags.iter_mut().zip(open) {
            crate::crafting::tests::fill(bag, open, items);
        }
        let mut store = world.resource_mut::<HouseholdStore>();
        let all = store.0.len();
        crate::crafting::tests::fill(&mut store.0, all, items);
    }

    /// The board sells a store grade bike for its price: the purse and the
    /// stock change by exactly that, and every refusal changes nothing.
    #[test]
    fn a_bike_is_bought_at_the_board_and_each_refusal_changes_nothing() {
        let mut app = first_day();
        app.insert_resource(Purse(600));
        let registries = app.world().resource::<Registries>().clone();
        let before = snapshot(&mut app);
        // No board in reach.
        assert_eq!(buy_one(&mut app, BIKE), Err(PurchaseRefusal::NotAtBoard));
        assert_eq!(snapshot(&mut app), before);
        board(&mut app);
        // Not a thing this buyer sells.
        assert_eq!(buy_one(&mut app, CORE_ITEM), Err(PurchaseRefusal::NotSold));
        assert_eq!(
            buy_one(&mut app, "veyra:vehicle.nope"),
            Err(PurchaseRefusal::NotSold)
        );
        assert_eq!(snapshot(&mut app), before);
        // The purse is short.
        app.insert_resource(Purse(449));
        assert_eq!(
            buy_one(&mut app, BIKE),
            Err(PurchaseRefusal::Short {
                needs: 450,
                have: 449
            })
        );
        let short = snapshot(&mut app);
        assert_eq!(short.2, Purse(449));
        assert_eq!((&short.0, &short.1), (&before.0, &before.1));
        // No room anywhere.
        app.insert_resource(Purse(600));
        let kept = snapshot(&mut app);
        fill_everything(&mut app);
        let full = snapshot(&mut app);
        assert_eq!(buy_one(&mut app, BIKE), Err(PurchaseRefusal::NoRoom));
        assert_eq!(snapshot(&mut app), full);
        // Room, and the money.
        let mut app = first_day();
        app.insert_resource(Purse(600));
        board(&mut app);
        let before = snapshot(&mut app);
        let done = buy_one(&mut app, BIKE).unwrap();
        assert_eq!((done.paid, done.item), (450, id(BIKE)));
        let (pack, store, purse) = snapshot(&mut app);
        assert_eq!(purse, Purse(150));
        assert_eq!(store, before.1, "the pack had room, the store is as it was");
        let bikes: Vec<Record> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &registries.0.items).ok())
            .filter(|record| record.item == id(BIKE))
            .collect();
        assert_eq!(bikes.len(), 1);
        let bike = &bikes[0];
        assert_eq!(
            (bike.batch, bike.by.as_ref(), bike.condition),
            (0, None, Some(1000))
        );
        assert!(bike.lines.values().all(|line| *line == 500));
        assert_eq!(bike.lines.len(), 3);
        // Exactly one stack more than before, and nothing else moved.
        let count = |pack: &Pack| pack.cells.slots().iter().flatten().count();
        assert_eq!(count(&pack), count(&before.0) + 1);
        let _ = kept;
        // The pure rule says the same as the request.
        let (pack, store, purse) = snapshot(&mut app);
        assert_eq!(
            check_purchase(
                &registries.0,
                true,
                &Purse(449),
                &pack,
                Some(&store),
                &buyer(),
                &id(BIKE)
            ),
            Err(PurchaseRefusal::Short {
                needs: 450,
                have: 449
            })
        );
        assert_eq!(
            check_purchase(
                &registries.0,
                true,
                &purse,
                &pack,
                Some(&store),
                &buyer(),
                &id("veyra:tool.survey")
            ),
            Ok(120)
        );
    }

    /// The board sells the three melee classes at store grade, a whole
    /// weapon with its condition, for the prices of the buyer's file.
    #[test]
    fn the_board_sells_a_blade_a_greatblade_and_a_glaive() {
        let mut app = first_day();
        app.insert_resource(Purse(1000));
        board(&mut app);
        let registries = app.world().resource::<Registries>().clone();
        for (item, price) in [
            ("veyra:weapon.service_blade", 180),
            ("veyra:weapon.yard_greatblade", 260),
            ("veyra:weapon.pier_glaive", 240),
        ] {
            let (pack, store, purse) = snapshot(&mut app);
            assert_eq!(
                check_purchase(
                    &registries.0,
                    true,
                    &purse,
                    &pack,
                    Some(&store),
                    &buyer(),
                    &id(item)
                ),
                Ok(price)
            );
            let done = buy_one(&mut app, item).unwrap();
            assert_eq!((done.paid, done.item), (price, id(item)));
            let (pack, _, after) = snapshot(&mut app);
            assert_eq!(after.0, purse.0 - price);
            let bought: Vec<Record> = pack
                .cells
                .slots()
                .iter()
                .flatten()
                .filter_map(|stack| items::read(stack, &registries.0.items).ok())
                .filter(|record| record.item == id(item))
                .collect();
            assert_eq!(bought.len(), 1);
            assert_eq!((bought[0].batch, bought[0].condition), (0, Some(1000)));
            assert!(bought[0].lines.values().all(|line| *line == 500));
        }
    }

    /// With the pack full the bike goes to the store the board reaches.
    #[test]
    fn a_full_pack_buys_into_the_store() {
        let mut app = first_day();
        app.insert_resource(Purse(500));
        board(&mut app);
        {
            let registries = app.world().resource::<Registries>().clone();
            let items = &registries.0.items;
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
                .map(|bag| usize::from(pack.open_cells(bag, items)))
                .collect();
            let all = pack.cells.len();
            crate::crafting::tests::fill(&mut pack.cells, all, items);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                crate::crafting::tests::fill(bag, open, items);
            }
        }
        let before = snapshot(&mut app);
        assert!(buy_one(&mut app, BIKE).is_ok());
        let after = snapshot(&mut app);
        assert_eq!(after.0, before.0, "the pack was full");
        assert_eq!(
            after.1.0.slots().iter().flatten().count(),
            before.1.0.slots().iter().flatten().count() + 1
        );
        assert_eq!(after.2, Purse(50));
    }

    /// The first day with Pelagic Works' world, so its board has a holder.
    fn with_holder() -> App {
        let mut app = first_day();
        app.insert_resource(crate::factions::Factions(std::sync::Arc::clone(
            &crate::deeds::tests::pelagic().0,
        )));
        board(&mut app);
        app
    }

    fn pelagic() -> FactionId {
        crate::deeds::tests::pelagic().1.id
    }

    #[test]
    fn a_sale_at_pelagics_board_is_a_deed_for_it_and_pays_the_welcoming_price() {
        let mut app = with_holder();
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let sold = sell_all(&mut app, &[("cell_cores", &kind, 100)])[0]
            .clone()
            .unwrap();
        // Pelagic starts welcoming: five per cent over the data's 5_875.
        assert_eq!(sold.paid, 6_168);
        assert_eq!(purse(&app), before + 6_168);
        let deeds = app.world().resource::<Deeds>();
        let list = &deeds.by[&pelagic().0];
        assert_eq!(
            (list[0].kind, list[0].amount),
            (crate::deeds::DeedKind::Sold, 6_168)
        );
    }

    #[test]
    fn a_hostile_board_refuses_to_trade() {
        let mut app = with_holder();
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        let kind = cores(10, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        assert_eq!(
            sell_all(&mut app, &[("cell_cores", &kind, 10)]),
            vec![Err(SaleRefusal::Hostile)]
        );
        assert_eq!(purse(&app), before);
        let item = shipped()
            .0
            .buyers
            .get(&buyer())
            .unwrap()
            .sells
            .first()
            .unwrap()
            .item
            .clone();
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .0
            .push(ProcurementRequest::Buy {
                buyer: buyer(),
                item,
            });
        app.update();
        let bought: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PurchaseAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(bought, vec![Err(PurchaseRefusal::Hostile)]);
    }

    /// The first day beside one of Pelagic's guards and not its board.
    fn beside_a_guard(faction: FactionId) -> App {
        let mut app = first_day();
        app.insert_resource(crate::factions::Factions(std::sync::Arc::clone(
            &crate::deeds::tests::pelagic().0,
        )));
        prop(&mut app, InteractionKind::Guard { faction });
        app
    }

    #[test]
    fn a_guard_sells_a_lease_unless_it_is_anothers_or_hostile_or_short() {
        let claims = shipped().0.factions.claims().unwrap().clone();
        // A guard of some other faction is no seller of Pelagic's lease.
        let mut app = beside_a_guard(FactionId(1));
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::NotAtBoard));
        // Pelagic's own guard sells it at the stance's price.
        let mut app = beside_a_guard(pelagic());
        app.world_mut().insert_resource(Purse(10_000));
        let price = crate::deeds::Terms::scale(claims.lease.price, 950);
        assert_eq!(lease_once(&mut app).unwrap().paid, price);
        assert!(app.world().resource::<Leases>().live(pelagic(), 0.0));
        app.world_mut().insert_resource(Purse(price - 1));
        assert!(matches!(
            lease_once(&mut app),
            Err(LeaseRefusal::Short { .. })
        ));
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::Hostile));
        assert_eq!(purse(&app), 10_000);
    }

    fn lease_once(app: &mut App) -> Result<Leased, LeaseRefusal> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .0
            .push(ProcurementRequest::Lease { faction: pelagic() });
        app.update();
        let mut answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<LeaseAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(answers.len(), 1);
        answers.remove(0)
    }

    #[test]
    fn a_lease_costs_credits_and_runs_for_played_hours_unless_hostile_or_short() {
        let mut app = with_holder();
        let claims = shipped().0.factions.claims().unwrap().clone();
        app.world_mut().insert_resource(Purse(10_000));
        let leased = lease_once(&mut app).unwrap();
        // Welcoming: the lease price at the sell factor.
        let price = crate::deeds::Terms::scale(claims.lease.price, 950);
        assert_eq!(leased.paid, price);
        assert_eq!(purse(&app), 10_000 - price);
        assert!(app.world().resource::<Leases>().live(pelagic(), 0.0));
        // From the played second now, a few ticks in.
        let run = leased.until - claims.lease.hours * 3600.0;
        assert!((0.0..60.0).contains(&run), "{run}");
        // Short of credits: nothing changes.
        app.world_mut().insert_resource(Purse(price - 1));
        assert!(matches!(
            lease_once(&mut app),
            Err(LeaseRefusal::Short { .. })
        ));
        assert_eq!(purse(&app), price - 1);
        // Hostile: refused before the purse is asked.
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::Hostile));
        assert_eq!(purse(&app), 10_000);
    }
}
