//! Taking a weapon in hand (ticket 0007, step 9; `docs/design/crafting.md`,
//! section 6).
//!
//! The hands' slots stay `Locked` for a click ([`crate::items::Refusal::HandFixed`]),
//! so a weapon reaches a hand by one request that swaps it with the one
//! there: the weapon that leaves the hand goes into the cell the other came
//! out of, which is why there is always room, and it keeps its condition
//! and everything fitted. [`can_take`] is the one rule, asked by the
//! request and by the screen alike.
//!
//! Local-only like the stock it moves: nothing here is replicated.

use bevy_ecs::prelude::*;

use slotted_model::Inventory;

use strata_registry::item::{BAG_SLOTS, HANDS, ItemCatalogue, Record};

use crate::items::kind_of;

use crate::items::{self, HouseholdStore, Pack, Stock};

/// Why a weapon would not be taken in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EquipRefusal {
    /// The item is not a weapon.
    #[error("that is not a weapon")]
    NotAWeapon,
    /// The other hand holds a weapon of that family already.
    #[error("the other hand holds one of those")]
    SameFamily,
    /// A menu's cursor holds a stack, which a swap would leave behind.
    #[error("put down what is on the cursor first")]
    Busy,
    /// No such weapon is in the pack, its bags or an open store.
    #[error("that weapon is not to hand")]
    NotHeld,
    /// The member is in the middle of a swing, a reload or an ability.
    #[error("finish what you are doing first")]
    MidAction,
}

/// Where a weapon lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// A cell of the pack's own grid.
    Pack(usize),
    /// A cell under a bag slot.
    Bag(usize, usize),
    /// A cell of the household's store.
    Store(usize),
}

/// A weapon taken in hand.
#[derive(Debug, Clone, PartialEq)]
pub struct Equipped {
    /// The hand it is in now.
    pub hand: usize,
    /// The weapon, as it was read where it lay.
    pub item: Record,
}

impl Cell {
    /// The cell's index in its inventory.
    pub const fn index(self) -> usize {
        match self {
            Self::Pack(cell) | Self::Bag(_, cell) | Self::Store(cell) => cell,
        }
    }
}

/// The inventory `cell` is in, of `stock`.
pub(crate) fn inventory_of<'a>(cell: Cell, stock: &'a mut Stock<'_>) -> Option<&'a mut Inventory> {
    Some(match cell {
        Cell::Pack(_) => &mut stock.pack.cells,
        Cell::Bag(bag, _) => &mut stock.pack.bags[bag],
        Cell::Store(_) => &mut stock.store.as_mut()?.0,
    })
}

/// Where the record `kind` lies among the pack's cells, its bags' open
/// cells and, given, the store: a weapon, or a bike.
pub fn find(
    items: &ItemCatalogue,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
) -> Option<Cell> {
    let is = |inventory: &Inventory, cell: usize| {
        inventory
            .get(cell)
            .and_then(|stack| items::read(stack, items).ok())
            .is_some_and(|record| kind_of(&record) == *kind)
    };
    if let Some(cell) = (0..pack.cells.len()).find(|cell| is(&pack.cells, *cell)) {
        return Some(Cell::Pack(cell));
    }
    for (bag, cells) in pack.bags.iter().enumerate().take(BAG_SLOTS) {
        let open = usize::from(pack.open_cells(bag, items));
        if let Some(cell) = (0..open.min(cells.len())).find(|cell| is(cells, *cell)) {
            return Some(Cell::Bag(bag, cell));
        }
    }
    let store = store?;
    (0..store.0.len())
        .find(|cell| is(&store.0, *cell))
        .map(Cell::Store)
}

/// The hand a weapon of `kind` goes to when the player does not choose:
/// the hand holding that family if one does (the weapon replaces its own
/// kind), else the hand not in use (`in_use`, the one in hand).
#[must_use]
pub fn hand_for(
    items: &ItemCatalogue,
    hands: &[Option<Record>; HANDS],
    kind: &Record,
    in_use: usize,
) -> usize {
    let family = items.family_of(kind);
    hands
        .iter()
        .position(|held| {
            held.as_ref()
                .is_some_and(|held| family.is_some() && items.family_of(held) == family)
        })
        .unwrap_or(1 - in_use.min(1))
}

/// Whether the weapon of `kind` can be taken in `hand` from what `pack`
/// carries and, when a locker's menu is open over it, `store`. `hands` is
/// what the hands hold now: the weapon that leaves `hand` does not count
/// against the family rule, the one in the other hand does.
///
/// # Errors
/// The first of: not a weapon, the other hand holds one of its family, the
/// member is mid action (`ready` false), a stack on the cursor that is not
/// the weapon asked for, not to hand. A weapon carried on the cursor is
/// taken from there.
pub fn can_take(
    items: &ItemCatalogue,
    hands: &[Option<Record>; HANDS],
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
    hand: usize,
    ready: bool,
) -> Result<(), EquipRefusal> {
    let Some(family) = items.family_of(kind) else {
        return Err(EquipRefusal::NotAWeapon);
    };
    if hand >= HANDS {
        return Err(EquipRefusal::NotHeld);
    }
    let other = hands[1 - hand].as_ref();
    if other.is_some_and(|other| items.family_of(other) == Some(family)) {
        return Err(EquipRefusal::SameFamily);
    }
    if !ready {
        return Err(EquipRefusal::MidAction);
    }
    find_for_take(items, pack, store, &kind_of(kind)).map(|_| ())
}

/// Where a taken weapon lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The menu's cursor, where a weapon is carried.
    Cursor,
    /// A cell of the stock.
    At(Cell),
}

/// Where the weapon `kind` is taken from: the cursor if it carries that
/// weapon, else [`find`], and a cursor holding anything else is `Busy`.
fn find_for_take(
    items: &ItemCatalogue,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
) -> Result<Source, EquipRefusal> {
    if let Some(carried) = &pack.cursor {
        return if items::read(carried, items).is_ok_and(|record| kind_of(&record) == *kind) {
            Ok(Source::Cursor)
        } else {
            Err(EquipRefusal::Busy)
        };
    }
    find(items, pack, store, kind)
        .map(Source::At)
        .ok_or(EquipRefusal::NotHeld)
}

/// Swap the weapon of `kind` with the one in `hand`, by [`can_take`]'s
/// rule. The stock is changed only on success.
///
/// # Errors
/// What [`can_take`] says.
pub fn take(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    kind: &Record,
    hand: usize,
    ready: bool,
) -> Result<Equipped, EquipRefusal> {
    let saved = stock
        .hands
        .saved(items)
        .map_err(|_| EquipRefusal::NotHeld)?;
    can_take(
        items,
        &saved,
        stock.pack,
        stock.store.as_deref(),
        kind,
        hand,
        ready,
    )?;
    let kind = kind_of(kind);
    let cell = find_for_take(items, stock.pack, stock.store.as_deref(), &kind)?;
    let leaving = stock.hands.0.take(hand);
    let arriving = if let Source::At(cell) = cell {
        let index = cell.index();
        let inventory = inventory_of(cell, stock).ok_or(EquipRefusal::NotHeld)?;
        let arriving = inventory.take(index);
        inventory.set(index, leaving);
        inventory.clear_changed();
        arriving
    } else {
        // The player sets the old weapon down where they like.
        std::mem::replace(&mut stock.pack.cursor, leaving)
    };
    stock.hands.0.set(hand, arriving.clone());
    stock.hands.0.clear_changed();
    let item = arriving
        .and_then(|stack| items::read(&stack, items).ok())
        .ok_or(EquipRefusal::NotHeld)?;
    Ok(Equipped { hand, item })
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

    pub const CARBINE: &str = "veyra:weapon.patrol_carbine";

    pub const PISTOL: &str = "veyra:weapon.service_pistol";

    pub const GRIP: &str = "veyra:module.vigil_angled_grip";

    pub const GLAIVE: &str = "veyra:weapon.pier_glaive";

    pub const BLADE: &str = "veyra:weapon.service_blade";
}
