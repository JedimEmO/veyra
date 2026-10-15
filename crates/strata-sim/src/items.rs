//! Items in the sim: what a character carries, the weapons in their hands
//! and the household's store, as slotted's stacks, and every move of one
//! as a request consumed in the tick (ticket 0007, step 1).
//!
//! A stack is slotted's own `ItemStack`, so one representation runs from
//! the save to the screen. [`read`] and [`write()`] are the typed view over
//! its patch: a [`Record`] by name, which is what the rules and the save
//! speak. There is one writer, so equal records are equal patches and
//! merge, and two batches of one module never do.
//!
//! The truth is here and a screen holds a mirror. A click on the stores
//! screen is predicted on the mirror by slotted and arrives as a
//! [`StockRequest`] in the [`StockInbox`]; [`consume`] replays it on the
//! truth with the same click rules ([`apply`], a pure function over
//! `slotted_model::apply_click_validated`) and answers a [`StockAnswer`]:
//! an acknowledgement when its outcome is the prediction, the whole truth
//! when it is not. A click the sim refuses is answered with the truth and
//! a [`StockRefused`] that says why, since slotted's own refusal says
//! nothing. No rule lives in a screen.
//!
//! The menu both sides click through is [`layout`], derived from the pack:
//! slotted has no inventory that grows, so each bag slot has its full grid
//! of cells declared and a bag enables its own count; a bag slot is locked
//! while its cells hold anything; the hands are locked in this step.
//!
//! [`Hands`] sits on the player alone, beside the `Combatant` and not in
//! it, so no opponent and no harness actor has one. What is fitted to the
//! weapons in them reaches the combat numbers through [`crate::fitting`],
//! which turns the records into a loadout the `Combatant` is equipped
//! with; nothing fitted is no loadout, and the numbers are the tables'.
//! Local-only: nothing here is replicated.

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use slotted_model::{
    Actor, ClickAction, ClickError, Delta, Inventories, Inventory, InventoryRef, ItemId, ItemStack,
    LookupCtx, MenuDef, MenuId, MenuSnapshot, MenuState, Namespaced, Predicate, RoutingTable,
    SlotBehaviour, SlotDef, SlotIx, SlotRange, ToolbarAction, ValidationLevel, Value,
    apply_click_validated,
};
use std::collections::BTreeMap;
use strata_registry::item::{
    BAG_SLOTS, CarryRules, Fitted, HANDS, ItemCatalogue, ItemError, ItemKind, LotOrigin, Record,
    STORE_GRADE, SavedPack,
};

use crate::progression::{InteractionKind, Reach};
use crate::ride::{Riding, Stable};
use crate::{LocalPlayer, Registries, SimSet, SimTick};

/// Why a stack could not be read as a record, or a record written as a
/// stack.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The stack's id is not one of the catalogue's.
    #[error("item {0} is not in the catalogue")]
    UnknownStack(u32),
    /// The patch has a key the writer never writes, or a value of the
    /// wrong shape under one it does.
    #[error("`{item}`: the patch's `{key}` does not read")]
    Patch {
        /// The stack's item.
        item: Namespaced,
        /// The key, or `unknown key` for one that is not a record's.
        key: &'static str,
    },
    /// The record breaks one of the catalogue's rules.
    #[error(transparent)]
    Record(#[from] ItemError),
}

/// A patch value of a shape the writer never writes.
struct Misshapen;

fn quality(value: &Value) -> Result<u16, Misshapen> {
    match value {
        Value::Int(n) => u16::try_from(*n).map_err(|_| Misshapen),
        _ => Err(Misshapen),
    }
}

fn read_lines(value: Option<&Value>) -> Result<BTreeMap<String, u16>, Misshapen> {
    match value {
        None => Ok(BTreeMap::new()),
        Some(Value::Map(map)) => map
            .iter()
            .map(|(line, value)| Ok((line.clone(), quality(value)?)))
            .collect(),
        Some(_) => Err(Misshapen),
    }
}

fn read_batch(value: Option<&Value>) -> Result<u32, Misshapen> {
    match value {
        Some(Value::Int(n)) => u32::try_from(*n).map_err(|_| Misshapen),
        _ => Err(Misshapen),
    }
}

fn read_by(value: Option<&Value>) -> Result<Option<String>, Misshapen> {
    match value {
        None => Ok(None),
        Some(Value::Str(by)) => Ok(Some(by.clone())),
        Some(_) => Err(Misshapen),
    }
}

fn read_fitted(value: &Value) -> Result<Fitted, Misshapen> {
    let Value::Map(map) = value else {
        return Err(Misshapen);
    };
    let known = ["item", "batch", "by", "lines", "condition"];
    if map.keys().any(|key| !known.contains(&key.as_str())) {
        return Err(Misshapen);
    }
    let Some(Value::Str(item)) = map.get("item") else {
        return Err(Misshapen);
    };
    Ok(Fitted {
        item: Namespaced::parse(item).map_err(|_| Misshapen)?,
        batch: read_batch(map.get("batch"))?,
        by: read_by(map.get("by"))?,
        lines: read_lines(map.get("lines"))?,
        condition: map.get("condition").map(quality).transpose()?,
    })
}

fn read_origin(value: &Value) -> Result<LotOrigin, Misshapen> {
    let Value::Map(map) = value else {
        return Err(Misshapen);
    };
    // A lot saved before fees went carries `terms`; it is read past and not
    // written again, so two lots of one deposit differing only by it merge.
    let known = ["deposit", "root", "place"];
    let old = ["terms"];
    if known.iter().any(|key| !map.contains_key(*key))
        || map
            .keys()
            .any(|key| !known.contains(&key.as_str()) && !old.contains(&key.as_str()))
    {
        return Err(Misshapen);
    }
    let text = |key: &str| match map.get(key) {
        Some(Value::Str(text)) => Ok(text.clone()),
        _ => Err(Misshapen),
    };
    let Some(Value::List(cell)) = map.get("deposit") else {
        return Err(Misshapen);
    };
    let [Value::Int(x), Value::Int(z)] = cell[..] else {
        return Err(Misshapen);
    };
    let coordinate = |n: i64| i32::try_from(n).map_err(|_| Misshapen);
    Ok(LotOrigin {
        deposit: [coordinate(x)?, coordinate(z)?],
        root: text("root")?,
        place: text("place")?,
    })
}

/// The record a stack holds: its item by name, its count and its patch,
/// typed and checked against the catalogue.
///
/// # Errors
/// An id from another frozen set, a patch the writer did not write, or a
/// record the catalogue refuses.
pub fn read(stack: &ItemStack, items: &ItemCatalogue) -> Result<Record, RecordError> {
    let def = items
        .def_of(stack.id)
        .ok_or(RecordError::UnknownStack(stack.id.0))?;
    let keys = items.keys();
    let refused = |key| {
        move |_: Misshapen| RecordError::Patch {
            item: def.id.clone(),
            key,
        }
    };
    let known = [
        keys.batch,
        keys.by,
        keys.lines,
        keys.condition,
        keys.fitted,
        keys.origin,
    ];
    if stack.patch.iter().any(|(key, _)| !known.contains(&key)) {
        return Err(refused("unknown key")(Misshapen));
    }
    let patch = &stack.patch;
    let fitted = match patch.get(keys.fitted) {
        None => Ok(BTreeMap::new()),
        Some(Value::Map(map)) => map
            .iter()
            .map(|(slot, value)| Ok((slot.clone(), read_fitted(value)?)))
            .collect(),
        Some(_) => Err(Misshapen),
    };
    let condition = patch.get(keys.condition).map(quality).transpose();
    let origin = patch.get(keys.origin).map(read_origin).transpose();
    let record = Record {
        item: def.id.clone(),
        count: stack.count,
        batch: read_batch(patch.get(keys.batch)).map_err(refused("strata:batch"))?,
        by: read_by(patch.get(keys.by)).map_err(refused("strata:by"))?,
        lines: read_lines(patch.get(keys.lines)).map_err(refused("strata:lines"))?,
        condition: condition.map_err(refused("strata:condition"))?,
        fitted: fitted.map_err(refused("strata:fitted"))?,
        origin: origin.map_err(refused("strata:origin"))?,
    };
    items.check_record(&record)?;
    Ok(record)
}

fn write_lines(lines: &BTreeMap<String, u16>) -> Value {
    Value::Map(
        lines
            .iter()
            .map(|(line, value)| (line.clone(), Value::Int(i64::from(*value))))
            .collect(),
    )
}

/// The stack a record is: the one writer of a patch. A key is left out
/// where the record has nothing to say under it (no maker, no lines,
/// nothing fitted), never written empty, so two stacks are the same kind
/// exactly when their records are equal but for the count.
///
/// # Errors
/// A record the catalogue refuses.
pub fn write(record: &Record, items: &ItemCatalogue) -> Result<ItemStack, RecordError> {
    items.check_record(record)?;
    let id = items
        .id_of(&record.item)
        .ok_or_else(|| ItemError::UnknownItem(record.item.clone()))?;
    let keys = items.keys();
    let mut stack = ItemStack::new(id, record.count);
    stack
        .patch
        .insert(keys.batch, Value::Int(i64::from(record.batch)));
    if let Some(by) = &record.by {
        stack.patch.insert(keys.by, Value::Str(by.clone()));
    }
    if !record.lines.is_empty() {
        stack.patch.insert(keys.lines, write_lines(&record.lines));
    }
    if let Some(condition) = record.condition {
        stack
            .patch
            .insert(keys.condition, Value::Int(i64::from(condition)));
    }
    if !record.fitted.is_empty() {
        let fitted = record
            .fitted
            .iter()
            .map(|(slot, fitted)| {
                let mut map = BTreeMap::from([
                    ("item".to_owned(), Value::Str(fitted.item.to_string())),
                    ("batch".to_owned(), Value::Int(i64::from(fitted.batch))),
                ]);
                if let Some(by) = &fitted.by {
                    map.insert("by".to_owned(), Value::Str(by.clone()));
                }
                if !fitted.lines.is_empty() {
                    map.insert("lines".to_owned(), write_lines(&fitted.lines));
                }
                if let Some(condition) = fitted.condition {
                    map.insert("condition".to_owned(), Value::Int(i64::from(condition)));
                }
                (slot.clone(), Value::Map(map))
            })
            .collect();
        stack.patch.insert(keys.fitted, Value::Map(fitted));
    }
    if let Some(origin) = &record.origin {
        // The deposit is the whole of what tells two lots of one type
        // apart, so it is in the patch whole: its id and the words a card
        // reads, which a later change of the lattice could not give back.
        let [x, z] = origin.deposit;
        let map = BTreeMap::from([
            (
                "deposit".to_owned(),
                Value::List(vec![Value::Int(i64::from(x)), Value::Int(i64::from(z))]),
            ),
            ("root".to_owned(), Value::Str(origin.root.clone())),
            ("place".to_owned(), Value::Str(origin.place.clone())),
        ]);
        stack.patch.insert(keys.origin, Value::Map(map));
    }
    Ok(stack)
}

/// One number for a stack's quality: the mean of its lines, which is what
/// a slot's pip shows so two batches tell apart at a glance. An item with
/// no lines reads store grade.
#[must_use]
pub fn grade(record: &Record) -> u16 {
    let lines = u32::try_from(record.lines.len()).unwrap_or(u32::MAX);
    if lines == 0 {
        return STORE_GRADE;
    }
    let sum: u32 = record.lines.values().map(|line| u32::from(*line)).sum();
    u16::try_from((sum + lines / 2) / lines).unwrap_or(u16::MAX)
}

/// [`grade`] for anything, and for a lot where its figures stand in their
/// type's spans: the mean share of each span, on the scale of a quality
/// line. A lot's raw mean would rank a middling sulphur over the best of
/// salts, since a type's floors are its own; a pip is for telling two lots
/// of one type apart, which this does.
#[must_use]
pub fn grade_of(items: &ItemCatalogue, record: &Record) -> u16 {
    let Some(ItemKind::Lot { ranges, .. }) = items.get(&record.item).map(|def| &def.kind) else {
        return grade(record);
    };
    if ranges.is_empty() || record.origin.is_none() {
        return grade(record);
    }
    let shares: f64 = ranges
        .iter()
        .map(|(attribute, span)| {
            span.share(record.lines.get(attribute.key()).copied().unwrap_or(0))
        })
        .sum();
    // A handful of lines.
    #[allow(clippy::cast_precision_loss)]
    let mean = shares / ranges.len() as f64;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of nought to one on a scale of a thousand"
    )]
    {
        (mean * f64::from(strata_registry::item::QUALITY_MAX)).round() as u16
    }
}

/// Slotted's view of the catalogue for the click rules: how high an item
/// stacks and what it is tagged. Written here because slotted's own is in
/// its Bevy crate, which the sim does not link.
#[derive(Debug, Clone, Copy)]
pub struct CatalogueLookup<'a>(pub &'a ItemCatalogue);

impl LookupCtx for CatalogueLookup<'_> {
    fn max_stack(&self, id: ItemId) -> u32 {
        self.0.def_of(id).map_or(1, |def| def.stack.max(1))
    }

    fn has_tag(&self, id: ItemId, tag: &Namespaced) -> bool {
        self.0.frozen().items_with(tag).contains(&id)
    }
}

/// `saved` in an inventory of `len` slots. The catalogue's own checks have
/// passed by the time this is called, so the length holds.
pub(crate) fn fill(
    len: usize,
    saved: &[Option<Record>],
    items: &ItemCatalogue,
) -> Result<Inventory, RecordError> {
    let mut slots = vec![None; len];
    for (slot, record) in slots.iter_mut().zip(saved) {
        *slot = record
            .as_ref()
            .map(|record| write(record, items))
            .transpose()?;
    }
    Ok(Inventory::from_slots(slots))
}

/// An inventory as a save writes it: slot for slot, without the empty
/// slots at its end, so an empty pack is a short file.
pub(crate) fn unfill(
    inventory: &Inventory,
    items: &ItemCatalogue,
) -> Result<Vec<Option<Record>>, RecordError> {
    let mut saved = inventory
        .slots()
        .iter()
        .map(|slot| slot.as_ref().map(|stack| read(stack, items)).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    while saved.last().is_some_and(Option::is_none) {
        saved.pop();
    }
    Ok(saved)
}

fn stacks(inventory: &Inventory) -> u32 {
    u32::try_from(inventory.slots().iter().flatten().count()).unwrap_or(u32::MAX)
}

/// What a character carries: the pack's own cells, its bag slots, the
/// cells under each bag slot, and a stack left on the cursor.
///
/// A bag's contents live in the pack's bag cells and not inside the bag
/// item, so a cell is one stack whatever opened it and a bag is an
/// ordinary single. Each bag slot keeps its full grid; the bag in it says
/// how many of those cells are open ([`Pack::open_cells`]).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Pack {
    /// The pack's own grid.
    pub cells: Inventory,
    /// The bag slots, each a bag or nothing.
    pub bag_slots: Inventory,
    /// The cells under each bag slot.
    pub bags: [Inventory; BAG_SLOTS],
    /// The stack on the cursor. While the stores screen is open this is
    /// what is being carried; after it closes it is what found no room,
    /// and it is saved, so nothing is lost by closing on a full pack.
    pub cursor: Option<ItemStack>,
}

impl Pack {
    /// A pack of the rules' size with nothing in it.
    #[must_use]
    pub fn empty(carry: &CarryRules) -> Self {
        Self {
            cells: Inventory::new(usize::from(carry.pack.cells())),
            bag_slots: Inventory::new(usize::from(carry.bag_slots)),
            bags: std::array::from_fn(|_| Inventory::new(usize::from(carry.bag_cells))),
            cursor: None,
        }
    }

    /// The pack a save or the first day's file holds.
    ///
    /// # Errors
    /// A saved pack the catalogue refuses.
    pub fn from_saved(saved: &SavedPack, items: &ItemCatalogue) -> Result<Self, RecordError> {
        items.check_pack(saved)?;
        let carry = items.carry();
        let mut pack = Self::empty(carry);
        pack.cells = fill(pack.cells.len(), &saved.cells, items)?;
        pack.bag_slots = fill(pack.bag_slots.len(), &saved.bag_slots, items)?;
        for (bag, cells) in pack.bags.iter_mut().zip(&saved.bags) {
            *bag = fill(bag.len(), cells, items)?;
        }
        pack.cursor = saved
            .cursor
            .as_ref()
            .map(|record| write(record, items))
            .transpose()?;
        Ok(pack)
    }

    /// The pack as a save writes it.
    ///
    /// # Errors
    /// A stack the catalogue cannot read, which no click produces.
    pub fn saved(&self, items: &ItemCatalogue) -> Result<SavedPack, RecordError> {
        let mut bags = self
            .bags
            .iter()
            .map(|bag| unfill(bag, items))
            .collect::<Result<Vec<_>, _>>()?;
        while bags.last().is_some_and(Vec::is_empty) {
            bags.pop();
        }
        Ok(SavedPack {
            cells: unfill(&self.cells, items)?,
            bag_slots: unfill(&self.bag_slots, items)?,
            bags,
            cursor: self
                .cursor
                .as_ref()
                .map(|stack| read(stack, items))
                .transpose()?,
        })
    }

    /// How many cells the bag in slot `bag` opens: its own count, or none
    /// with no bag there.
    #[must_use]
    pub fn open_cells(&self, bag: usize, items: &ItemCatalogue) -> u16 {
        let Some(def) = self
            .bag_slots
            .get(bag)
            .and_then(|stack| items.def_of(stack.id))
        else {
            return 0;
        };
        match def.kind {
            ItemKind::Bag { cells } => cells,
            _ => 0,
        }
    }

    /// How many stacks sit in the cells under bag slot `bag`.
    #[must_use]
    pub fn held_in(&self, bag: usize) -> u32 {
        self.bags.get(bag).map_or(0, stacks)
    }
}

/// The weapons a character carries: two slots, any weapon in either, never
/// two of one family. Which is in hand is the `Combatant`'s `weapon`; a
/// draw moves nothing here, and the `Combatant`'s slots follow these.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(crate::wear::WearCarry)]
pub struct Hands(pub Inventory);

impl Default for Hands {
    /// Both hands empty: a character of a data set without items.
    fn default() -> Self {
        Self(Inventory::new(HANDS))
    }
}

impl Hands {
    /// The hands a save or the first day's file holds.
    ///
    /// # Errors
    /// A record the catalogue refuses, or a weapon in the wrong hand.
    pub fn from_saved(
        saved: &[Option<Record>; HANDS],
        items: &ItemCatalogue,
    ) -> Result<Self, RecordError> {
        items.check_hands(saved)?;
        Ok(Self(fill(HANDS, saved, items)?))
    }

    /// The hands as a save writes them.
    ///
    /// # Errors
    /// A stack the catalogue cannot read, which no click produces.
    pub fn saved(&self, items: &ItemCatalogue) -> Result<[Option<Record>; HANDS], RecordError> {
        let mut saved = std::array::from_fn(|_| None);
        for (index, slot) in saved.iter_mut().enumerate() {
            *slot = self
                .0
                .get(index)
                .map(|stack| read(stack, items))
                .transpose()?;
        }
        Ok(saved)
    }
}

/// The household's store: one for everyone on the bench, reached at a
/// locker. A resource because it belongs to no character; the game fills
/// it from its save.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct HouseholdStore(pub Inventory);

impl Default for HouseholdStore {
    /// No store at all, which is what a server and a harness have.
    fn default() -> Self {
        Self(Inventory::new(0))
    }
}

impl HouseholdStore {
    /// The store a save or the first day's file holds.
    ///
    /// # Errors
    /// A saved store the catalogue refuses.
    pub fn from_saved(
        saved: &[Option<Record>],
        items: &ItemCatalogue,
    ) -> Result<Self, RecordError> {
        items.check_store(saved)?;
        let cells = usize::from(items.carry().store.cells());
        Ok(Self(fill(cells, saved, items)?))
    }

    /// The store as a save writes it.
    ///
    /// # Errors
    /// A stack the catalogue cannot read, which no click produces.
    pub fn saved(&self, items: &ItemCatalogue) -> Result<Vec<Option<Record>>, RecordError> {
        unfill(&self.0, items)
    }
}

/// Whether the household's store is to hand: a locker in reach, or the run
/// says from anywhere. The rule a click on the store is held to.
#[must_use]
pub fn at_locker(access: StoreAccess, reach: &Reach) -> bool {
    access == StoreAccess::Anywhere || matches!(reach.nearest, Some((_, InteractionKind::Locker)))
}

/// The menu a stock is clicked through, and where each part of it is.
///
/// Derived from the pack by [`layout`] on both sides, the sim's for each
/// click and the screen's for its mirror, so the two agree on which slot is
/// which and on what each allows without sending a definition across.
#[derive(Debug, Clone, PartialEq)]
pub struct StockLayout {
    /// The slots and the shift-click routes.
    pub def: MenuDef,
    /// The pack's own cells.
    pub pack: SlotRange,
    /// The bag slots.
    pub bag_slots: SlotRange,
    /// The cells under each bag slot, open or not.
    pub bags: [SlotRange; BAG_SLOTS],
    /// The hands.
    pub hands: SlotRange,
    /// The saddle bags, when the household has a bike.
    pub saddle: Option<SlotRange>,
    /// The household's store, when the menu was opened over it.
    pub store: Option<SlotRange>,
}

/// From where the bike's saddle bags are reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaddleAccess {
    /// Out of reach: the grid is declared and shut.
    Shut,
    /// Mounted: shift-click moves between the pack and the saddle.
    Mounted,
    /// At a locker, with the store open: shift-click moves between the
    /// saddle and the store.
    Locker,
}

impl SaddleAccess {
    /// The access a player has: riding opens it, else a locker with the
    /// store reached.
    #[must_use]
    pub const fn of(riding: bool, store_reached: bool) -> Self {
        if riding {
            Self::Mounted
        } else if store_reached {
            Self::Locker
        } else {
            Self::Shut
        }
    }
}

/// The saddle bags as a menu declares them: the cells and from where they
/// are open. Present exactly when the household has a bike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaddleSlots {
    /// The grid's size in cells.
    pub cells: u16,
    /// From where it is reached.
    pub access: SaddleAccess,
}

impl StockLayout {
    /// The pack's own cells, as the menu's inventories are numbered.
    pub const PACK: InventoryRef = InventoryRef::new(0);
    /// The bag slots.
    pub const BAG_SLOTS: InventoryRef = InventoryRef::new(1);
    /// The first bag's cells; each bag after it is the next.
    pub const BAGS: InventoryRef = InventoryRef::new(2);
    /// The hands, after the last bag's cells.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "two bags, in a constant, where `try_from` cannot be called"
    )]
    pub const HANDS: InventoryRef = InventoryRef::new(2 + BAG_SLOTS as u16);
    /// The bike's saddle bags, after the hands. Always an inventory of the
    /// menu, of no cells when the household has no bike, so the store keeps
    /// its number.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "two bags, in a constant, where `try_from` cannot be called"
    )]
    pub const SADDLE: InventoryRef = InventoryRef::new(3 + BAG_SLOTS as u16);
    /// The household's store, last because a menu may be without it.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "two bags, in a constant, where `try_from` cannot be called"
    )]
    pub const STORE: InventoryRef = InventoryRef::new(4 + BAG_SLOTS as u16);

    /// The inventory of bag `bag`'s cells.
    ///
    /// # Panics
    /// For a bag past [`BAG_SLOTS`].
    #[must_use]
    pub fn bag(bag: usize) -> InventoryRef {
        assert!(bag < BAG_SLOTS, "a pack has {BAG_SLOTS} bags");
        InventoryRef::new(u16::try_from(Self::BAGS.index() + bag).expect("a small index"))
    }
}

/// The menu over a character's stock: the pack's cells, the bag slots, the
/// cells under each, the hands and, `with_store`, the household's store.
///
/// The slot numbers do not move with what is carried, because every grid
/// is declared at its full size: a bag cell its bag does not open is
/// `Disabled`, which no click sees. A bag slot takes one bag and nothing
/// else, and is `Locked` while its cells hold anything, so a bag cannot be
/// taken from under its contents. The hands are `Locked`: taking another
/// weapon in hand is a later step's.
///
/// The saddle, when the household has a bike, is declared at its full size
/// after the hands; its cells are `Normal` mounted or at a locker with the
/// store open, and `Disabled` otherwise.
///
/// Shift-click sends a stack across: from the store to the pack and then
/// the bags, from the pack and the bags to the store, and without the
/// store between the pack and the bags. Mounted, the pack and the bags and
/// the saddle trade instead; at a locker the saddle sends to the store.
#[must_use]
pub fn layout(
    carry: &CarryRules,
    items: &ItemCatalogue,
    pack: &Pack,
    with_store: bool,
    saddle: Option<SaddleSlots>,
) -> StockLayout {
    let mut def = MenuDef::new();
    let cells = def.add_slots(StockLayout::PACK, carry.pack.cells(), SlotBehaviour::Normal);
    let first_bag_slot = def.slots.len();
    for slot in 0..carry.bag_slots {
        let behaviour = if pack.held_in(usize::from(slot)) > 0 {
            SlotBehaviour::Locked
        } else {
            SlotBehaviour::Normal
        };
        def.slots.push(
            SlotDef::new(StockLayout::BAG_SLOTS, slot)
                .max_stack(1)
                .accepts(Predicate::Tag(ItemCatalogue::bag_tag()))
                .behaviour(behaviour),
        );
    }
    let bag_slots = range(first_bag_slot, def.slots.len());
    let bags = std::array::from_fn(|bag| {
        let open = pack.open_cells(bag, items);
        let first = def.slots.len();
        for cell in 0..carry.bag_cells {
            let behaviour = if cell < open {
                SlotBehaviour::Normal
            } else {
                SlotBehaviour::Disabled
            };
            def.slots
                .push(SlotDef::new(StockLayout::bag(bag), cell).behaviour(behaviour));
        }
        range(first, def.slots.len())
    });
    let hands = def.add_slots(
        StockLayout::HANDS,
        u16::try_from(HANDS).expect("two hands"),
        SlotBehaviour::Locked,
    );
    let carried = SlotRange::new(cells.start, hands.start);
    let bag_cells: Vec<InventoryRef> = (0..BAG_SLOTS).map(StockLayout::bag).collect();
    let saddle_range = saddle.map(|saddle| {
        let behaviour = if saddle.access == SaddleAccess::Shut {
            SlotBehaviour::Disabled
        } else {
            SlotBehaviour::Normal
        };
        def.add_slots(StockLayout::SADDLE, saddle.cells, behaviour)
    });
    let store = with_store.then(|| {
        def.add_slots(
            StockLayout::STORE,
            carry.store.cells(),
            SlotBehaviour::Normal,
        )
    });
    let mut across = vec![StockLayout::PACK];
    across.extend(&bag_cells);
    // The first rule that covers a slot wins, so the saddle's come first.
    let mut routes = RoutingTable::new();
    let reaches = saddle
        .zip(saddle_range)
        .map(|(saddle, range)| (saddle.access, range));
    match reaches {
        Some((SaddleAccess::Mounted, range)) => {
            routes = routes
                .route(range, across.clone())
                .route(carried, [StockLayout::SADDLE]);
        }
        Some((SaddleAccess::Locker, range)) if store.is_some() => {
            routes = routes.route(range, [StockLayout::STORE]);
        }
        _ => {}
    }
    def.quick_move = match store {
        Some(store) => routes
            .route(carried, [StockLayout::STORE])
            .route(store, across),
        None => routes
            .route(cells, bag_cells)
            .route(SlotRange::new(cells.end, hands.start), [StockLayout::PACK]),
    };
    StockLayout {
        def,
        pack: cells,
        bag_slots,
        bags,
        hands,
        saddle: saddle_range,
        store,
    }
}

fn range(start: usize, end: usize) -> SlotRange {
    let slot = |n| u16::try_from(n).expect("the carrying rules number fewer slots than a menu can");
    SlotRange::new(slot(start), slot(end))
}

/// Why the sim would not do what a request asked. The words a player
/// reads are the screen's; this is what is the matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// A bag was to leave its slot while its cells hold stacks.
    #[error("the bag holds {stacks} stacks")]
    BagHolds {
        /// How many.
        stacks: u32,
    },
    /// A weapon was to leave its hand, which waits for the workbench.
    #[error("a weapon in hand stays there")]
    HandFixed,
    /// Something that is no bag was put to a bag slot.
    #[error("a bag slot takes a bag")]
    NotABag,
    /// The household's store was reached for away from a locker.
    #[error("the store is reached at a locker")]
    NoLocker,
    /// The saddle bags were reached for neither mounted nor at a locker.
    #[error("the saddle bags are reached mounted or at a locker")]
    SaddleShut,
    /// A stack was to be sent across and the other side has no cell for
    /// any of it.
    #[error("there is no room on the other side")]
    NoRoom,
    /// The click would have put a stack out of the menu, into the world.
    #[error("a stack cannot be dropped")]
    NoDropping,
    /// An action the stores do not offer: sorting, throwing, creating.
    #[error("the stores do not do that")]
    Unsupported,
    /// The request names a menu the sim has not got open.
    #[error("no such menu is open")]
    NotOpen,
    /// There is no local player to carry anything.
    #[error("nobody is carrying")]
    NoPlayer,
    /// The click rules' own refusal: nothing to do, no such slot.
    #[error("{0}")]
    Click(ClickError),
}

/// Why `slot` is locked, for a screen that wants to say so before a click
/// is sent: slotted refuses a click on a locked slot itself and tells
/// nobody. `None` for a slot that is not locked.
#[must_use]
pub fn why_locked(layout: &StockLayout, pack: &Pack, slot: SlotIx) -> Option<Refusal> {
    if layout.hands.contains(slot) {
        return Some(Refusal::HandFixed);
    }
    if layout.bag_slots.contains(slot) {
        let stacks = pack.held_in(usize::from(slot.0 - layout.bag_slots.start));
        return (stacks > 0).then_some(Refusal::BagHolds { stacks });
    }
    None
}

/// The stock one menu is over: the local player's pack and hands and, when
/// the menu was opened over it, the household's store.
#[derive(Debug)]
pub struct Stock<'a> {
    /// What the character carries; its cursor is the carried stack.
    pub pack: &'a mut Pack,
    /// The weapons in their hands.
    pub hands: &'a mut Hands,
    /// The household's store, when the menu reaches it.
    pub store: Option<&'a mut HouseholdStore>,
    /// The bike's saddle bags, when a click may reach them. Left `None` by
    /// everything that moves stock but a click, which never touches them.
    pub saddle: Option<SaddleStock<'a>>,
}

/// The saddle bags in a [`Stock`]: the cells and from where they are
/// reached.
#[derive(Debug)]
pub struct SaddleStock<'a> {
    /// The grid.
    pub cells: &'a mut Inventory,
    /// From where it is reached.
    pub access: SaddleAccess,
}

impl Stock<'_> {
    /// The stock's inventories in the menu's order, copied. A stock with no
    /// saddle has an empty inventory in its place.
    fn inventories(&self) -> Inventories {
        let pack = &*self.pack;
        let mut list = vec![pack.cells.clone(), pack.bag_slots.clone()];
        list.extend(pack.bags.iter().cloned());
        list.push(self.hands.0.clone());
        list.push(
            self.saddle
                .as_ref()
                .map_or_else(|| Inventory::new(0), |saddle| saddle.cells.clone()),
        );
        list.extend(self.store.as_ref().map(|store| store.0.clone()));
        list.into_iter().collect()
    }

    /// The saddle as the layout declares it, if the stock has one.
    fn saddle_slots(&self) -> Option<SaddleSlots> {
        self.saddle.as_ref().map(|saddle| SaddleSlots {
            cells: u16::try_from(saddle.cells.len()).unwrap_or(u16::MAX),
            access: saddle.access,
        })
    }

    /// Take `inventories` as the stock, in the menu's order.
    fn commit(&mut self, inventories: &Inventories) {
        let take = |handle: InventoryRef, into: &mut Inventory| {
            if let Some(inventory) = inventories.get(handle) {
                *into = inventory.clone();
                // The dirty mask is the sync layer's and part of an
                // inventory's equality; nothing here reads it.
                into.clear_changed();
            }
        };
        take(StockLayout::PACK, &mut self.pack.cells);
        take(StockLayout::BAG_SLOTS, &mut self.pack.bag_slots);
        for (bag, cells) in self.pack.bags.iter_mut().enumerate() {
            take(StockLayout::bag(bag), cells);
        }
        take(StockLayout::HANDS, &mut self.hands.0);
        if let Some(saddle) = self.saddle.as_mut() {
            take(StockLayout::SADDLE, saddle.cells);
        }
        if let Some(store) = self.store.as_mut() {
            take(StockLayout::STORE, &mut store.0);
        }
    }

    /// The stack in menu slot `slot` of `def`.
    fn at(&self, def: &MenuDef, slot: SlotIx) -> Option<&ItemStack> {
        let slot = def.slot(slot)?;
        let pack = &*self.pack;
        let inventory = match slot.source {
            StockLayout::PACK => &pack.cells,
            StockLayout::BAG_SLOTS => &pack.bag_slots,
            StockLayout::HANDS => &self.hands.0,
            StockLayout::SADDLE => &*self.saddle.as_ref()?.cells,
            StockLayout::STORE => &self.store.as_ref()?.0,
            bag => pack
                .bags
                .get(bag.index().checked_sub(StockLayout::BAGS.index())?)?,
        };
        inventory.get(usize::from(slot.index))
    }

    /// Everything a screen needs to overwrite its mirror with the truth.
    /// `saddle` is the saddle's truth for a stock that holds none: only a
    /// click changes the saddle, so a system that moves stock otherwise
    /// need not hold it.
    #[must_use]
    pub fn snapshot(&self, state: &MenuState, saddle: &Inventory) -> MenuSnapshot {
        let mut state = state.clone();
        state.carried.clone_from(&self.pack.cursor);
        let mut inventories = self.inventories();
        if self.saddle.is_none() {
            inventories[StockLayout::SADDLE].clone_from(saddle);
            inventories[StockLayout::SADDLE].clear_changed();
        }
        MenuSnapshot { inventories, state }
    }
}

/// The slot an action is at, for a refusal to be shown there.
fn slot_of(action: ClickAction) -> Option<SlotIx> {
    match action {
        ClickAction::Pickup { slot, .. }
        | ClickAction::QuickMove { slot }
        | ClickAction::Swap { slot, .. }
        | ClickAction::Clone { slot }
        | ClickAction::Throw { slot, .. }
        | ClickAction::PickupAll { slot, .. }
        | ClickAction::Toolbar(ToolbarAction::ToggleFavorite { slot }) => Some(slot),
        ClickAction::Drag { slot, .. } => slot,
        ClickAction::Toolbar(_) | ClickAction::Give { .. } => None,
    }
    .filter(|slot| !slot.is_outside())
}

/// One click on a stock, by slotted's own click rules: the same function
/// the screen predicted with, run on the truth. Pure: it reads the
/// catalogue and writes the stock and the menu's state, and on a refusal
/// it has written neither (but for a drag the click rules themselves end).
///
/// `state` is the open menu's: its state id and a drag under way. The
/// carried stack is the pack's cursor, not the state's, so that a swap of
/// who is out, which replaces the pack, cannot leave a second copy of it
/// behind; the state's is overwritten from the cursor going in and equals
/// it coming out.
///
/// # Errors
/// Why the click was refused: by the layout ([`why_locked`]), by the click
/// rules, for want of room across, or because it is not a thing the stores
/// do.
pub fn apply(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    state: &mut MenuState,
    action: ClickAction,
) -> Result<Delta, Refusal> {
    if matches!(
        action,
        ClickAction::Toolbar(_)
            | ClickAction::Throw { .. }
            | ClickAction::Clone { .. }
            | ClickAction::Give { .. }
            | ClickAction::Swap { .. }
    ) {
        return Err(Refusal::Unsupported);
    }
    let layout = layout(
        items.carry(),
        items,
        stock.pack,
        stock.store.is_some(),
        stock.saddle_slots(),
    );
    if let (Some(saddle), Some(slot)) = (layout.saddle, slot_of(action))
        && saddle.contains(slot)
        && stock
            .saddle
            .as_ref()
            .is_some_and(|saddle| saddle.access == SaddleAccess::Shut)
    {
        return Err(Refusal::SaddleShut);
    }
    let mut next = state.clone();
    next.carried.clone_from(&stock.pack.cursor);
    // The click rules end a drag that anything else interrupts and refuse
    // the interruption. The screen's copy of the rules did that to the
    // click before this one and, having refused it, never sent it; this
    // click was predicted with no drag under way.
    if !matches!(action, ClickAction::Drag { .. }) {
        next.drag = None;
    }
    // The check that a click made or lost nothing runs after the click, so
    // the click runs on a copy that is kept only when it passes.
    let mut scratch = stock.inventories();
    let outcome = apply_click_validated(
        &layout.def,
        &mut scratch,
        &mut next,
        action,
        &Actor::SURVIVAL,
        &CatalogueLookup(items),
        ValidationLevel::Always,
    );
    match outcome {
        Ok(delta) if delta.dropped.is_empty() => {
            stock.commit(&scratch);
            stock.pack.cursor.clone_from(&next.carried);
            *state = next;
            Ok(delta)
        }
        Ok(_) => Err(Refusal::NoDropping),
        Err(error) => {
            state.drag = next.drag;
            let slot = slot_of(action);
            Err(match error {
                ClickError::SlotLocked => slot
                    .and_then(|slot| why_locked(&layout, stock.pack, slot))
                    .unwrap_or(Refusal::Click(error)),
                ClickError::NotAllowed
                    if slot.is_some_and(|slot| layout.bag_slots.contains(slot)) =>
                {
                    Refusal::NotABag
                }
                // The click rules have one answer for a send from an empty
                // slot and for a send that found every cell across taken.
                // The second is a refusal a player is owed the reason for.
                ClickError::NothingToDo
                    if matches!(action, ClickAction::QuickMove { .. })
                        && slot.is_some_and(|slot| stock.at(&layout.def, slot).is_some()) =>
                {
                    Refusal::NoRoom
                }
                _ => Refusal::Click(error),
            })
        }
    }
}

/// Put the cursor's stack away ([`place`]). What finds no room stays on
/// the cursor.
pub fn stow(items: &ItemCatalogue, stock: &mut Stock<'_>) {
    let Some(carried) = stock.pack.cursor.take() else {
        return;
    };
    stock.pack.cursor = place(items, stock, carried);
}

/// Put `carried` away: into the pack's cells, then the open cells of its
/// bags, then the store when the stock has one, onto stacks of its kind
/// first and into empty cells after. What found no room comes back.
/// The cursor's stack on a close and a module taken off a weapon are put
/// away by the one rule, so a player learns where things go once.
#[must_use = "what found no room is still somebody's"]
pub fn place(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    mut carried: ItemStack,
) -> Option<ItemStack> {
    let max = CatalogueLookup(items).max_stack(carried.id);
    let open: [u16; BAG_SLOTS] = std::array::from_fn(|bag| stock.pack.open_cells(bag, items));
    let Pack { cells, bags, .. } = &mut *stock.pack;
    let mut targets: Vec<(&mut Inventory, usize)> = Vec::new();
    let all = cells.len();
    targets.push((cells, all));
    for (bag, open) in bags.iter_mut().zip(open) {
        targets.push((bag, usize::from(open)));
    }
    if let Some(store) = stock.store.as_mut() {
        let all = store.0.len();
        targets.push((&mut store.0, all));
    }
    for merge in [true, false] {
        for (inventory, open) in &mut targets {
            place_pass(inventory, *open, &mut carried, max, merge);
        }
    }
    (!carried.is_empty()).then_some(carried)
}

/// One pass of [`place`] over the first `open` cells of one inventory: onto
/// stacks of `carried`'s kind when `merge`, into empty cells when not.
fn place_pass(
    inventory: &mut Inventory,
    open: usize,
    carried: &mut ItemStack,
    max: u32,
    merge: bool,
) {
    for cell in 0..open {
        if carried.is_empty() {
            break;
        }
        match inventory.get(cell) {
            Some(present) if merge && present.same_kind(carried) => {
                let mut merged = present.clone();
                if carried.merge_into(&mut merged, max) > 0 {
                    inventory.set(cell, Some(merged));
                }
            }
            None if !merge => inventory.set(cell, carried.split(max)),
            _ => {}
        }
    }
    // The dirty mask is the sync layer's and part of an inventory's
    // equality; nothing here reads it.
    inventory.clear_changed();
}

/// Put `carried` away in one inventory alone, by [`place`]'s rule: onto
/// stacks of its kind first and into empty cells after. What found no room
/// comes back. A fabricator's output goes to the household's store and
/// nowhere else, since nobody need be standing at it when a unit is made.
#[must_use = "what found no room is still somebody's"]
pub fn place_in(
    items: &ItemCatalogue,
    inventory: &mut Inventory,
    mut carried: ItemStack,
) -> Option<ItemStack> {
    let max = CatalogueLookup(items).max_stack(carried.id);
    let all = inventory.len();
    for merge in [true, false] {
        place_pass(inventory, all, &mut carried, max, merge);
    }
    (!carried.is_empty()).then_some(carried)
}

/// What a screen asks of the stock.
#[derive(Debug, Clone, PartialEq)]
pub enum StockRequest {
    /// A screen opened `menu` over the local player's stock.
    Open {
        /// The screen's id for the menu, which every later request and
        /// every answer names.
        menu: MenuId,
        /// Whether it reaches the household's store.
        with_store: bool,
    },
    /// A click the screen has already predicted on its mirror.
    Click {
        /// The menu.
        menu: MenuId,
        /// The click.
        action: ClickAction,
        /// What the screen's copy of the rules made of it.
        predicted: Delta,
    },
    /// The screen closed.
    Close {
        /// The menu.
        menu: MenuId,
    },
}

impl StockRequest {
    /// The menu the request is about.
    #[must_use]
    pub const fn menu(&self) -> MenuId {
        match self {
            Self::Open { menu, .. } | Self::Click { menu, .. } | Self::Close { menu } => *menu,
        }
    }
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message: a screen writes it in `Update` and the tick reads
/// it in `FixedUpdate`, which may not run for a frame or may run twice,
/// and a request must be consumed exactly once.
#[derive(Resource, Debug, Default)]
pub struct StockInbox(pub Vec<StockRequest>);

/// The sim's answer to a click, in the shape slotted's reconcile takes.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum StockAnswer {
    /// The click did on the truth what the screen predicted.
    Ack {
        /// The menu.
        menu: MenuId,
        /// The menu's state id after the click.
        state_id: u32,
    },
    /// It did not, or was refused: the truth, to overwrite the mirror.
    Resync {
        /// The menu.
        menu: MenuId,
        /// Every inventory and the carried stack.
        snapshot: MenuSnapshot,
    },
}

/// A request the sim refused, and why, for the screen to say at the slot.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockRefused {
    /// The menu.
    pub menu: MenuId,
    /// The slot the click was at; none for a request that is at no slot.
    pub slot: Option<SlotIx>,
    /// What was the matter.
    pub reason: Refusal,
}

/// From where the household's store is reached.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoreAccess {
    /// Standing at a locker, which is the rule.
    #[default]
    AtLocker,
    /// From anywhere: a run that opens the stores for a still has no
    /// locker to stand at.
    Anywhere,
}

/// The menu a screen has open over the stock, as the sim knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    /// The screen's id for it.
    pub menu: MenuId,
    /// Whether it reaches the household's store.
    pub with_store: bool,
    /// Its state id and a drag under way. Its carried stack follows the
    /// pack's cursor, which is the truth (see [`apply`]).
    pub state: MenuState,
    /// The saddle bags as the sim has them, so a resync from a system that
    /// moves stock but not the saddle can say what the screen's mirror of
    /// them should hold.
    pub saddle: Inventory,
}

/// The open menu, if a screen has one open. One at a time: the stock is
/// the local player's and one screen shows it.
#[derive(Resource, Debug, Default)]
pub struct OpenStock(pub Option<Opened>);

/// `SimTick`, `SimSet::Input`: every request made since the last tick, in
/// order, on the local player's stock.
///
/// A click is replayed by [`apply`] and answered: an acknowledgement when
/// its outcome is the screen's prediction, else the truth, and the truth
/// again for every click after it in the tick; a refusal is the truth and a
/// [`StockRefused`]. The store's rule is asked at every click
/// and not once at the open, since the world runs under an open screen and
/// a character can be moved from the locker: a menu opened over the store
/// away from one shows it and moves nothing. A close puts the cursor's
/// stack away ([`stow`]).
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the open menu, the store's rule and the stock in, two answers out"
)]
#[allow(clippy::too_many_lines, reason = "one arm a request")]
pub fn consume(
    mut inbox: ResMut<StockInbox>,
    mut open: ResMut<OpenStock>,
    access: Res<StoreAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    mut store: ResMut<HouseholdStore>,
    mut stable: ResMut<Stable>,
    mut players: Query<(&mut Pack, &mut Hands, Has<Riding>), With<LocalPlayer>>,
    mut answers: MessageWriter<StockAnswer>,
    mut refusals: MessageWriter<StockRefused>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let items = &registries.0.items;
    let Ok((mut pack, mut hands, riding)) = players.single_mut() else {
        for request in requests {
            refusals.write(StockRefused {
                menu: request.menu(),
                slot: None,
                reason: Refusal::NoPlayer,
            });
        }
        return;
    };
    let at_locker = *access == StoreAccess::Anywhere
        || matches!(reach.nearest, Some((_, InteractionKind::Locker)));
    // The menu a click of this tick was answered with the truth for. That
    // truth was taken before the clicks after it, so each of those is
    // answered with the truth as well: an acknowledgement would leave the
    // screen at the earlier one.
    let mut resynced = None;
    for request in requests {
        let menu = request.menu();
        match request {
            StockRequest::Open { with_store, .. } => {
                open.0 = Some(Opened {
                    menu,
                    with_store,
                    state: MenuState::new(&MenuDef::new()),
                    saddle: stable.saddle.clone(),
                });
                if with_store && !at_locker {
                    refusals.write(StockRefused {
                        menu,
                        slot: None,
                        reason: Refusal::NoLocker,
                    });
                }
            }
            StockRequest::Click {
                action, predicted, ..
            } => {
                let opened = open.0.as_mut().filter(|opened| opened.menu == menu);
                let with_store = opened.as_ref().is_some_and(|opened| opened.with_store);
                // The saddle is a copy for the click, kept when it changes.
                let mut saddle = stable.saddle.clone();
                let access = SaddleAccess::of(riding, with_store && at_locker);
                let mut stock = Stock {
                    pack: &mut pack,
                    hands: &mut hands,
                    store: if with_store { Some(&mut *store) } else { None },
                    saddle: stable.bike.is_some().then_some(SaddleStock {
                        cells: &mut saddle,
                        access,
                    }),
                };
                let (outcome, state) = match opened {
                    None => (Err(Refusal::NotOpen), MenuState::new(&MenuDef::new())),
                    Some(opened) => {
                        let outcome = if with_store && !at_locker {
                            Err(Refusal::NoLocker)
                        } else {
                            apply(items, &mut stock, &mut opened.state, action)
                        };
                        (outcome, opened.state.clone())
                    }
                };
                match outcome {
                    Ok(delta) if delta == predicted && resynced != Some(menu) => {
                        answers.write(StockAnswer::Ack {
                            menu,
                            state_id: delta.state_id,
                        });
                    }
                    differs => {
                        resynced = Some(menu);
                        answers.write(StockAnswer::Resync {
                            menu,
                            snapshot: stock.snapshot(&state, &stable.saddle),
                        });
                        if let Err(reason) = differs {
                            refusals.write(StockRefused {
                                menu,
                                slot: slot_of(action),
                                reason,
                            });
                        }
                    }
                }
                if stable.saddle != saddle {
                    stable.saddle = saddle;
                }
                if let Some(opened) = open.0.as_mut().filter(|opened| opened.menu == menu) {
                    opened.saddle.clone_from(&stable.saddle);
                }
            }
            StockRequest::Close { .. } => {
                let Some(opened) = open.0.take_if(|opened| opened.menu == menu) else {
                    continue;
                };
                let reaches = opened.with_store && at_locker;
                stow(
                    items,
                    &mut Stock {
                        pack: &mut pack,
                        hands: &mut hands,
                        store: if reaches { Some(&mut *store) } else { None },
                        saddle: None,
                    },
                );
            }
        }
    }
}

/// Registers the stock's requests and answers and consumes them in the
/// tick. Added by `SimPlugin`, so a server and a harness have it too; with
/// no request, which is all either ever has, it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StockInbox>()
            .init_resource::<OpenStock>()
            .init_resource::<StoreAccess>()
            .init_resource::<HouseholdStore>()
            .add_message::<StockAnswer>()
            .add_message::<StockRefused>()
            .add_systems(
                SimTick,
                consume
                    .in_set(SimSet::Input)
                    .after(crate::player::consume_player_intents),
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
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;
    use slotted_model::Button;
    use std::sync::Arc;

    const MENU: MenuId = MenuId(7);
    const FEED: &str = "veyra:module.patrol_extended_feed";
    const CELL: &str = "veyra:module.pistol_extended_cell";
    const SATCHEL: &str = "veyra:bag.field_satchel";
    const HAUL: &str = "veyra:bag.haul_pack";

    fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    /// The first day in a running sim: the arrival with the pack and the
    /// hands of `starting.ron`, the household's store filled from it, and
    /// nothing within reach. One fixed step an update, so a request sent is
    /// a request consumed by the plugin's own system in its own set.
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
        app.world_mut().spawn((LocalPlayer, pack, hands));
        app.insert_resource(store).insert_resource(registries);
        app
    }

    /// A locker the player stands at. `find_reach` rewrites the reach from
    /// the props in the world at the end of every tick, so the locker is a
    /// prop and a tick passes before it is within reach.
    fn locker(app: &mut App) -> Entity {
        use crate::progression::Interaction;
        let place = app
            .world_mut()
            .spawn((
                crate::WorldPosition::new(0., 0., 0.),
                Interaction {
                    kind: InteractionKind::Locker,
                    radius: 2.5,
                },
            ))
            .id();
        app.update();
        place
    }

    fn send(app: &mut App, request: StockRequest) -> (Vec<StockAnswer>, Vec<StockRefused>) {
        app.world_mut().resource_mut::<StockInbox>().0.push(request);
        app.update();
        let world = app.world_mut();
        (
            world
                .resource_mut::<Messages<StockAnswer>>()
                .drain()
                .collect(),
            world
                .resource_mut::<Messages<StockRefused>>()
                .drain()
                .collect(),
        )
    }

    fn open(app: &mut App, with_store: bool) -> Vec<StockRefused> {
        let menu = MENU;
        let (answers, refused) = send(app, StockRequest::Open { menu, with_store });
        assert!(answers.is_empty(), "an open is not a click");
        refused
    }

    fn carried(app: &mut App) -> (Pack, Hands) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<(&Pack, &Hands), With<LocalPlayer>>();
        let (pack, hands) = players.single(world).unwrap();
        (pack.clone(), hands.clone())
    }

    /// What the screen predicts: the same rules on its mirror, which is a
    /// copy of the truth while the two agree. A click its rules refuse it
    /// never sends, so there is no prediction to make of one.
    fn predict(app: &mut App, action: ClickAction) -> Delta {
        let (mut pack, mut hands) = carried(app);
        let mut store = app.world().resource::<HouseholdStore>().clone();
        let opened = app.world().resource::<OpenStock>().0.clone().unwrap();
        let registries = app.world().resource::<Registries>().clone();
        let mut state = opened.state;
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: opened.with_store.then_some(&mut store),
        };
        apply(&registries.0.items, &mut stock, &mut state, action).expect("the mirror takes it")
    }

    /// A click predicted and sent, which the sim must acknowledge.
    fn click(app: &mut App, action: ClickAction) -> u32 {
        let predicted = predict(app, action);
        let (answers, refused) = send(
            app,
            StockRequest::Click {
                menu: MENU,
                action,
                predicted,
            },
        );
        assert_eq!(refused, [], "{action:?}");
        let [
            StockAnswer::Ack {
                menu: MENU,
                state_id,
            },
        ] = answers[..]
        else {
            panic!("{action:?} was not acknowledged: {answers:?}");
        };
        state_id
    }

    /// A click the screen's own rules would not have sent, sent anyway: the
    /// sim's refusal and the truth it answers with.
    fn refused(app: &mut App, action: ClickAction) -> (StockRefused, MenuSnapshot) {
        let before = (
            carried(app),
            app.world().resource::<HouseholdStore>().clone(),
        );
        let (mut answers, refused) = send(
            app,
            StockRequest::Click {
                menu: MENU,
                action,
                predicted: Delta::default(),
            },
        );
        let after = (
            carried(app),
            app.world().resource::<HouseholdStore>().clone(),
        );
        assert_eq!(before, after, "a refused click moves nothing");
        let [refused] = refused[..] else {
            panic!("{action:?} was not refused: {refused:?}");
        };
        assert_eq!(refused.menu, MENU);
        let Some(StockAnswer::Resync {
            menu: MENU,
            snapshot,
        }) = answers.pop()
        else {
            panic!("a refusal is answered with the truth");
        };
        assert!(answers.is_empty());
        (refused, snapshot)
    }

    fn left(slot: u16) -> ClickAction {
        ClickAction::Pickup {
            slot: SlotIx(slot),
            button: Button::Left,
        }
    }

    fn right(slot: u16) -> ClickAction {
        ClickAction::Pickup {
            slot: SlotIx(slot),
            button: Button::Right,
        }
    }

    fn shift(slot: u16) -> ClickAction {
        ClickAction::QuickMove { slot: SlotIx(slot) }
    }

    fn stock_layout(app: &mut App) -> StockLayout {
        let (pack, _) = carried(app);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        layout(items.carry(), items, &pack, true, None)
    }

    /// What menu slot `slot` holds, as `(item, count, batch)`.
    fn at(app: &mut App, slot: u16) -> Option<(String, u32, u32)> {
        let layout = stock_layout(app);
        let (pack, hands) = carried(app);
        let store = app.world().resource::<HouseholdStore>().clone();
        let registries = app.world().resource::<Registries>().clone();
        let def = layout.def.slot(SlotIx(slot)).unwrap();
        let inventory = match def.source {
            StockLayout::PACK => &pack.cells,
            StockLayout::BAG_SLOTS => &pack.bag_slots,
            StockLayout::HANDS => &hands.0,
            StockLayout::STORE => &store.0,
            bag => &pack.bags[bag.index() - StockLayout::BAGS.index()],
        };
        inventory
            .get(usize::from(def.index))
            .map(|stack| triple(stack, &registries))
    }

    fn triple(stack: &ItemStack, registries: &Registries) -> (String, u32, u32) {
        let record = read(stack, &registries.0.items).unwrap();
        (record.item.to_string(), record.count, record.batch)
    }

    fn cursor(app: &mut App) -> Option<(String, u32, u32)> {
        let (pack, _) = carried(app);
        let registries = app.world().resource::<Registries>().clone();
        pack.cursor.as_ref().map(|stack| triple(stack, &registries))
    }

    fn of(item: &str, count: u32, batch: u32) -> (String, u32, u32) {
        (item.to_owned(), count, batch)
    }

    /// How many of each item and batch exist anywhere: carried, in hand,
    /// stored or on the cursor.
    fn census(app: &mut App) -> BTreeMap<(String, u32), u32> {
        let (pack, hands) = carried(app);
        let store = app.world().resource::<HouseholdStore>().clone();
        let registries = app.world().resource::<Registries>().clone();
        let mut census = BTreeMap::new();
        let inventories = [&pack.cells, &pack.bag_slots, &hands.0, &store.0];
        inventories
            .into_iter()
            .chain(&pack.bags)
            .flat_map(|inventory| inventory.slots().iter().flatten())
            .chain(&pack.cursor)
            .map(|stack| triple(stack, &registries))
            .for_each(|(item, count, batch)| *census.entry((item, batch)).or_insert(0) += count);
        census
    }

    fn behaviours(layout: &StockLayout, slots: SlotRange) -> Vec<SlotBehaviour> {
        slots
            .iter()
            .map(|slot| layout.def.slot(slot).unwrap().behaviour)
            .collect()
    }

    #[test]
    fn a_record_round_trips_through_a_stack() {
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let packed = starting.packs.values().flat_map(|pack| {
            pack.bag_slots
                .iter()
                .chain(pack.bags.iter().flatten())
                .flatten()
        });
        let mut seen = 0;
        for record in starting.hands.iter().chain(&starting.store).chain(packed) {
            let stack = write(record, items).unwrap();
            assert_eq!(read(&stack, items).as_ref(), Ok(record));
            seen += 1;
        }
        assert_eq!(
            seen, 16,
            "two hands, twelve in the store, a bag and its stack"
        );

        // What the patch holds is the design's table: a key is there only
        // when the record has something under it.
        let keys = items.keys();
        let bought = write(&starting.store[0], items).unwrap();
        assert_eq!(bought.patch.len(), 2);
        assert_eq!(bought.patch.get(keys.batch), Some(&Value::Int(0)));
        assert!(
            matches!(bought.patch.get(keys.lines), Some(Value::Map(lines)) if lines.len() == 2)
        );
        let crafted = write(&starting.store[1], items).unwrap();
        assert_eq!(crafted.patch.len(), 3);
        assert_eq!(crafted.patch.get(keys.batch), Some(&Value::Int(412)));
        assert_eq!(
            crafted.patch.get(keys.by),
            Some(&Value::Str("Tovan Reyes".to_owned()))
        );
        let bag = write(&starting.store[5], items).unwrap();
        assert_eq!(bag.patch.len(), 1, "a bag has a batch and nothing else");
        let carbine = write(&starting.hands[0], items).unwrap();
        assert_eq!(carbine.patch.len(), 3);
        assert_eq!(carbine.patch.get(keys.condition), Some(&Value::Int(1000)));
        assert_eq!(carbine.patch.get(keys.fitted), None, "nothing is fitted");

        // A weapon with something on a mount, the shape a later step fits.
        let module = |record: &Record| Fitted {
            item: record.item.clone(),
            batch: record.batch,
            by: record.by.clone(),
            lines: record.lines.clone(),
            condition: None,
        };
        let mut fitted = starting.hands[0].clone();
        fitted
            .fitted
            .insert("feed".to_owned(), module(&starting.store[1]));
        fitted
            .fitted
            .insert("sight".to_owned(), module(&starting.store[3]));
        fitted.condition = Some(780);
        let stack = write(&fitted, items).unwrap();
        assert_eq!(stack.patch.len(), 4);
        assert_eq!(read(&stack, items), Ok(fitted));

        // A patch the writer did not write is not read as something else.
        let mut odd = bought.clone();
        odd.patch
            .insert(keys.condition, Value::Str("fine".to_owned()));
        assert!(matches!(
            read(&odd, items),
            Err(RecordError::Patch {
                key: "strata:condition",
                ..
            })
        ));
        let mut bare = bought.clone();
        bare.patch.remove(keys.batch);
        assert!(matches!(read(&bare, items), Err(RecordError::Patch { .. })));
        assert_eq!(
            read(&ItemStack::new(ItemId(999), 1), items),
            Err(RecordError::UnknownStack(999))
        );
        // And a record the catalogue refuses is never a stack.
        let mut many = starting.store[0].clone();
        many.count = 21;
        assert!(matches!(
            write(&many, items),
            Err(RecordError::Record(ItemError::Count { .. }))
        ));

        assert_eq!(grade(&starting.store[0]), STORE_GRADE);
        assert_eq!(grade(&starting.store[1]), 522, "the mean of 610 and 433");
        assert_eq!(grade(&starting.store[5]), STORE_GRADE, "no lines to mean");
    }

    #[test]
    fn two_batches_of_one_module_never_merge() {
        let mut app = first_day();
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let bought = write(&starting.store[0], items).unwrap();
        let crafted = write(&starting.store[1], items).unwrap();
        assert_eq!(bought.id, crafted.id, "one item");
        assert!(!bought.same_kind(&crafted), "two kinds of stack");
        let mut more = starting.store[0].clone();
        more.count = 9;
        assert!(bought.same_kind(&write(&more, items).unwrap()));

        // Dropped on the other batch, the carried one changes places with
        // it and joins nothing.
        locker(&mut app);
        open(&mut app, true);
        click(&mut app, left(47));
        assert_eq!(cursor(&mut app), Some(of(FEED, 2, 412)));
        click(&mut app, left(46));
        assert_eq!(at(&mut app, 46), Some(of(FEED, 2, 412)));
        assert_eq!(cursor(&mut app), Some(of(FEED, 4, 0)));
        click(&mut app, left(47));
        // Sent across one after the other they take a cell each.
        click(&mut app, shift(46));
        click(&mut app, shift(47));
        assert_eq!(at(&mut app, 0), Some(of(FEED, 2, 412)));
        assert_eq!(at(&mut app, 1), Some(of(FEED, 4, 0)));
        // Two stacks with equal records do join: the three bought cells
        // land on the one in the satchel.
        click(&mut app, shift(48));
        assert_eq!(at(&mut app, 20), Some(of(CELL, 4, 0)));
        assert_eq!(at(&mut app, 48), None);
        let census = census(&mut app);
        assert_eq!(census[&(FEED.to_owned(), 0)], 4);
        assert_eq!(census[&(FEED.to_owned(), 412)], 2);
    }

    #[test]
    fn a_click_carries_a_stack_from_the_store_to_the_pack() {
        let mut app = first_day();
        // The design's numbering, which the screen's grids are laid over.
        let layout = stock_layout(&mut app);
        assert_eq!(layout.pack, SlotRange::new(0, 18));
        assert_eq!(layout.bag_slots, SlotRange::new(18, 20));
        assert_eq!(
            layout.bags,
            [SlotRange::new(20, 32), SlotRange::new(32, 44)]
        );
        assert_eq!(layout.hands, SlotRange::new(44, 46));
        assert_eq!(layout.store, Some(SlotRange::new(46, 100)));
        assert_eq!(layout.def.slots.len(), 100);
        assert_eq!(
            (StockLayout::HANDS, StockLayout::STORE),
            (InventoryRef::new(4), InventoryRef::new(6))
        );
        assert_eq!(layout.def.inventory_sizes(), [18, 2, 12, 12, 2, 0, 54]);

        let before = census(&mut app);
        locker(&mut app);
        assert_eq!(open(&mut app, true), []);
        assert_eq!(at(&mut app, 46), Some(of(FEED, 4, 0)));
        assert_eq!(click(&mut app, left(46)), 1);
        assert_eq!(at(&mut app, 46), None);
        assert_eq!(
            cursor(&mut app),
            Some(of(FEED, 4, 0)),
            "carried, on the pack"
        );
        assert_eq!(census(&mut app), before, "carried is not lost");
        assert_eq!(click(&mut app, left(0)), 2);
        assert_eq!(at(&mut app, 0), Some(of(FEED, 4, 0)));
        assert_eq!(cursor(&mut app), None);
        assert_eq!(census(&mut app), before);
    }

    #[test]
    fn a_right_click_halves_and_a_shift_click_sends_across() {
        let mut app = first_day();
        let before = census(&mut app);
        locker(&mut app);
        open(&mut app, true);
        // Three cells: a right click takes the larger half, a second on an
        // empty cell puts one down.
        click(&mut app, right(48));
        assert_eq!(cursor(&mut app), Some(of(CELL, 2, 0)));
        assert_eq!(at(&mut app, 48), Some(of(CELL, 1, 0)));
        click(&mut app, right(1));
        assert_eq!(at(&mut app, 1), Some(of(CELL, 1, 0)));
        assert_eq!(cursor(&mut app), Some(of(CELL, 1, 0)));
        click(&mut app, left(1));
        assert_eq!(at(&mut app, 1), Some(of(CELL, 2, 0)));
        assert_eq!(cursor(&mut app), None);

        // With the store open a shift click sends a stack to the other
        // side: to the pack's first free cell, and back to the store's.
        click(&mut app, shift(47));
        assert_eq!(at(&mut app, 0), Some(of(FEED, 2, 412)));
        assert_eq!(at(&mut app, 47), None);
        click(&mut app, shift(0));
        assert_eq!(at(&mut app, 0), None);
        assert_eq!(at(&mut app, 47), Some(of(FEED, 2, 412)));
        // From a bag's cell too.
        click(&mut app, shift(20));
        assert_eq!(at(&mut app, 20), None);
        assert_eq!(at(&mut app, 48), Some(of(CELL, 2, 0)), "onto its own kind");

        // Without the store it sends between the pack and its bags.
        send(&mut app, StockRequest::Close { menu: MENU });
        assert_eq!(open(&mut app, false), []);
        click(&mut app, shift(1));
        assert_eq!(at(&mut app, 1), None);
        assert_eq!(at(&mut app, 20), Some(of(CELL, 2, 0)));
        click(&mut app, shift(20));
        assert_eq!(at(&mut app, 0), Some(of(CELL, 2, 0)));
        assert_eq!(census(&mut app), before);
    }

    #[test]
    fn a_bag_opens_its_cells_and_a_full_bag_stays() {
        use SlotBehaviour::{Disabled, Locked, Normal};
        let mut app = first_day();
        let before = census(&mut app);
        open(&mut app, false);

        // The satchel opens six of its slot's twelve cells and, with a
        // stack in one, is held in its slot. The other slot is free and
        // opens nothing.
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bag_slots), [Locked, Normal]);
        let cells = behaviours(&layout, layout.bags[0]);
        assert_eq!(cells[..6], [Normal; 6]);
        assert_eq!(cells[6..], [Disabled; 6]);
        assert_eq!(behaviours(&layout, layout.bags[1]), [Disabled; 12]);
        let full = Refusal::BagHolds { stacks: 1 };
        let (pack, _) = carried(&mut app);
        assert_eq!(why_locked(&layout, &pack, SlotIx(18)), Some(full));
        assert_eq!(why_locked(&layout, &pack, SlotIx(19)), None);
        assert_eq!(why_locked(&layout, &pack, SlotIx(20)), None);

        let (answer, _) = refused(&mut app, left(18));
        assert_eq!(
            answer,
            StockRefused {
                menu: MENU,
                slot: Some(SlotIx(18)),
                reason: full,
            }
        );
        assert_eq!(refused(&mut app, shift(18)).0.reason, full);

        // Emptied, it comes out, and its cells close behind it.
        click(&mut app, left(20));
        click(&mut app, left(0));
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bag_slots), [Normal, Normal]);
        click(&mut app, left(18));
        assert_eq!(cursor(&mut app), Some(of(SATCHEL, 1, 0)));
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bags[0]), [Disabled; 12]);
        assert_eq!(
            refused(&mut app, left(20)).0.reason,
            Refusal::Click(ClickError::NoSuchSlot),
            "a closed cell is no slot"
        );
        // In the other slot it opens that slot's cells.
        click(&mut app, left(19));
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bags[1])[..6], [Normal; 6]);
        assert_eq!(behaviours(&layout, layout.bags[1])[6..], [Disabled; 6]);

        // A bag slot takes a bag and nothing else, and says so.
        click(&mut app, left(0));
        let (answer, _) = refused(&mut app, left(18));
        assert_eq!(
            (answer.slot, answer.reason),
            (Some(SlotIx(18)), Refusal::NotABag)
        );
        click(&mut app, left(32));
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bag_slots), [Normal, Locked]);

        // The haul pack opens all twelve.
        send(&mut app, StockRequest::Close { menu: MENU });
        locker(&mut app);
        open(&mut app, true);
        click(&mut app, left(51));
        assert_eq!(cursor(&mut app), Some(of(HAUL, 1, 0)));
        click(&mut app, left(18));
        let layout = stock_layout(&mut app);
        assert_eq!(behaviours(&layout, layout.bags[0]), [Normal; 12]);
        assert_eq!(census(&mut app), before);
    }

    #[test]
    fn a_weapon_in_hand_is_fixed_in_step_one() {
        let mut app = first_day();
        open(&mut app, false);
        let (_, hands) = carried(&mut app);
        assert_eq!(at(&mut app, 44).unwrap().0, "veyra:weapon.patrol_carbine");
        assert_eq!(at(&mut app, 45).unwrap().0, "veyra:weapon.service_pistol");
        let layout = stock_layout(&mut app);
        assert_eq!(
            behaviours(&layout, layout.hands),
            [SlotBehaviour::Locked; 2]
        );
        let (pack, _) = carried(&mut app);
        assert_eq!(
            why_locked(&layout, &pack, SlotIx(44)),
            Some(Refusal::HandFixed)
        );

        // Taken, sent across or covered with something carried: it stays.
        let (answer, snapshot) = refused(&mut app, left(44));
        assert_eq!(
            (answer.slot, answer.reason),
            (Some(SlotIx(44)), Refusal::HandFixed)
        );
        assert_eq!(
            snapshot.inventories[StockLayout::HANDS].slots(),
            hands.0.slots()
        );
        assert_eq!(refused(&mut app, shift(45)).0.reason, Refusal::HandFixed);
        click(&mut app, left(20));
        assert_eq!(refused(&mut app, left(45)).0.reason, Refusal::HandFixed);
        assert_eq!(carried(&mut app).1, hands);
    }

    #[test]
    fn a_prediction_that_differs_is_resynced() {
        let mut app = first_day();
        locker(&mut app);
        open(&mut app, true);
        // The screen's mirror was wrong about what the click would do. The
        // sim does what the click does on the truth and answers with it.
        let (answers, refusals) = send(
            &mut app,
            StockRequest::Click {
                menu: MENU,
                action: left(46),
                predicted: Delta::default(),
            },
        );
        assert_eq!(refusals, [], "the click itself was fine");
        let [
            StockAnswer::Resync {
                menu: MENU,
                snapshot,
            },
        ] = &answers[..]
        else {
            panic!("{answers:?}");
        };
        assert_eq!(cursor(&mut app), Some(of(FEED, 4, 0)), "the truth moved");
        let (pack, hands) = carried(&mut app);
        let store = app.world().resource::<HouseholdStore>().clone();
        assert_eq!(snapshot.state.carried, pack.cursor);
        assert_eq!(snapshot.state.state_id, 1);
        assert_eq!(
            snapshot.inventories.len(),
            7,
            "the saddle is always an inventory"
        );
        assert_eq!(snapshot.inventories[StockLayout::PACK], pack.cells);
        assert_eq!(snapshot.inventories[StockLayout::BAG_SLOTS], pack.bag_slots);
        assert_eq!(snapshot.inventories[StockLayout::bag(0)], pack.bags[0]);
        assert_eq!(snapshot.inventories[StockLayout::bag(1)], pack.bags[1]);
        assert_eq!(snapshot.inventories[StockLayout::HANDS], hands.0);
        assert_eq!(snapshot.inventories[StockLayout::STORE], store.0);
        assert_eq!(snapshot.inventories[StockLayout::STORE].get(0), None);

        // A prediction a state behind is as wrong as any other.
        let mut stale = predict(&mut app, left(0));
        stale.state_id -= 1;
        let (answers, _) = send(
            &mut app,
            StockRequest::Click {
                menu: MENU,
                action: left(0),
                predicted: stale,
            },
        );
        assert!(matches!(answers[..], [StockAnswer::Resync { .. }]));
        // And one that is right is acknowledged at the sim's state.
        assert_eq!(click(&mut app, left(0)), 3);
    }

    #[test]
    fn closing_stows_the_cursor_and_loses_nothing() {
        let mut app = first_day();
        let before = census(&mut app);
        let place = locker(&mut app);
        let close = StockRequest::Close { menu: MENU };

        // Onto a free cell of the pack.
        open(&mut app, true);
        click(&mut app, left(47));
        assert_eq!(send(&mut app, close.clone()), (vec![], vec![]));
        assert_eq!(cursor(&mut app), None);
        assert_eq!(at(&mut app, 0), Some(of(FEED, 2, 412)));
        assert!(app.world().resource::<OpenStock>().0.is_none());
        // Onto its own kind first, wherever in the pack that is.
        open(&mut app, true);
        click(&mut app, left(48));
        send(&mut app, close.clone());
        assert_eq!(at(&mut app, 20), Some(of(CELL, 4, 0)));
        assert_eq!(at(&mut app, 1), None);
        assert_eq!(census(&mut app), before);

        // A pack with no room: every cell of it and every open cell of its
        // bag holds a stack, and a bag joins no stack.
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let mut one = items.starting().unwrap().store[0].clone();
        one.count = 1;
        let one = write(&one, items).unwrap();
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            let Pack { cells, bags, .. } = &mut *pack;
            for (inventory, open) in [(cells, 18), (&mut bags[0], 6)] {
                for cell in 0..open {
                    if inventory.get(cell).is_none() {
                        inventory.set(cell, Some(one.clone()));
                    }
                }
                inventory.clear_changed();
            }
        }
        let full = census(&mut app);
        open(&mut app, true);
        click(&mut app, left(51));
        assert_eq!(cursor(&mut app), Some(of(HAUL, 1, 0)));
        // Away from the locker the store is out of reach, so the stack has
        // nowhere to go and stays on the cursor, which the save keeps.
        app.world_mut().despawn(place);
        app.update();
        send(&mut app, close.clone());
        assert_eq!(cursor(&mut app), Some(of(HAUL, 1, 0)));
        let (pack, _) = carried(&mut app);
        let saved = pack.saved(items).unwrap();
        assert_eq!(
            saved.cursor.as_ref().map(|record| record.item.to_string()),
            Some(HAUL.to_owned())
        );
        assert_eq!(Pack::from_saved(&saved, items), Ok(pack));
        // Back at one, the next close puts it in the store.
        locker(&mut app);
        open(&mut app, true);
        send(&mut app, close);
        assert_eq!(cursor(&mut app), None);
        assert_eq!(
            at(&mut app, 47),
            Some(of(HAUL, 1, 0)),
            "the first free cell"
        );
        assert_eq!(census(&mut app), full);
    }

    #[test]
    fn the_store_side_needs_a_locker_in_reach() {
        let mut app = first_day();
        let away = StockRefused {
            menu: MENU,
            slot: None,
            reason: Refusal::NoLocker,
        };
        // Opened over the store with nothing in reach: said at once, and
        // every click is refused, in the pack as much as in the store. The
        // answer still shows the store, since the screen drew it.
        assert_eq!(open(&mut app, true), [away]);
        let (answer, snapshot) = refused(&mut app, left(46));
        assert_eq!(
            (answer.slot, answer.reason),
            (Some(SlotIx(46)), Refusal::NoLocker)
        );
        assert_eq!(
            snapshot.inventories.len(),
            7,
            "the saddle is always an inventory"
        );
        assert_eq!(refused(&mut app, left(20)).0.reason, Refusal::NoLocker);

        // A terminal is not a locker.
        let terminal = app
            .world_mut()
            .spawn((
                crate::WorldPosition::new(0., 0., 0.),
                crate::progression::Interaction {
                    kind: InteractionKind::Terminal,
                    radius: 2.5,
                },
            ))
            .id();
        app.update();
        assert_eq!(refused(&mut app, left(46)).0.reason, Refusal::NoLocker);
        app.world_mut().despawn(terminal);

        // At a locker the same menu works.
        let place = locker(&mut app);
        click(&mut app, left(46));
        click(&mut app, left(0));
        // Having walked off, it stops again.
        app.world_mut().despawn(place);
        app.update();
        assert_eq!(refused(&mut app, left(0)).0.reason, Refusal::NoLocker);
        send(&mut app, StockRequest::Close { menu: MENU });

        // What is carried needs no locker, and has no store behind it.
        assert_eq!(open(&mut app, false), []);
        click(&mut app, left(0));
        click(&mut app, left(1));
        let (answer, snapshot) = refused(&mut app, left(46));
        assert_eq!(answer.reason, Refusal::Click(ClickError::NoSuchSlot));
        assert_eq!(snapshot.inventories.len(), 6);
        send(&mut app, StockRequest::Close { menu: MENU });

        // A run that opens the stores for a still has the store anywhere.
        app.insert_resource(StoreAccess::Anywhere);
        assert_eq!(open(&mut app, true), []);
        click(&mut app, left(47));
        click(&mut app, left(2));
        send(&mut app, StockRequest::Close { menu: MENU });

        // A click at a menu that is not open is not a click on anything.
        assert_eq!(refused(&mut app, left(2)).0.reason, Refusal::NotOpen);
    }

    /// Every cell of the pack and every open cell of its bag given a
    /// single of a batch of its own, which joins nothing.
    fn fill_the_pack(app: &mut App) {
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let mut one = items.starting().unwrap().store[1].clone();
        one.count = 1;
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let open = usize::from(pack.open_cells(0, items));
        let Pack { cells, bags, .. } = &mut *pack;
        let all = cells.len();
        for (inventory, open) in [(cells, all), (&mut bags[0], open)] {
            for cell in 0..open {
                if inventory.get(cell).is_none() {
                    one.batch += 1;
                    inventory.set(cell, Some(write(&one, items).unwrap()));
                }
            }
            inventory.clear_changed();
        }
    }

    #[test]
    fn a_send_with_no_room_across_says_so() {
        let mut app = first_day();
        locker(&mut app);
        open(&mut app, true);
        // From a slot with nothing in it there is nothing to send, which
        // is no refusal a player is owed words for.
        assert_eq!(
            refused(&mut app, shift(0)).0.reason,
            Refusal::Click(ClickError::NothingToDo)
        );
        send(&mut app, StockRequest::Close { menu: MENU });

        fill_the_pack(&mut app);
        let full = census(&mut app);
        open(&mut app, true);
        // The store's magazines have no cell to go to: not the pack's, not
        // the satchel's six. The other bag slot opens none.
        let (answer, snapshot) = refused(&mut app, shift(46));
        assert_eq!(
            answer,
            StockRefused {
                menu: MENU,
                slot: Some(SlotIx(46)),
                reason: Refusal::NoRoom,
            }
        );
        assert_eq!(
            snapshot.inventories[StockLayout::STORE]
                .get(0)
                .map(|stack| stack.count),
            Some(4),
            "they stay where they are"
        );
        // The cells already in the satchel take the store's three, since a
        // stack joins its own kind before it wants a cell.
        click(&mut app, shift(48));
        assert_eq!(at(&mut app, 20), Some(of(CELL, 4, 0)));
        // The other way there is room, and the cell it leaves takes the
        // magazines after all.
        click(&mut app, shift(0));
        click(&mut app, shift(46));
        assert_eq!(at(&mut app, 0), Some(of(FEED, 4, 0)));
        send(&mut app, StockRequest::Close { menu: MENU });

        // Without the store a full pack has its bags to send to, and they
        // are full too.
        open(&mut app, false);
        let (answer, _) = refused(&mut app, shift(0));
        assert_eq!(
            (answer.slot, answer.reason),
            (Some(SlotIx(0)), Refusal::NoRoom)
        );
        assert_eq!(refused(&mut app, shift(20)).0.reason, Refusal::NoRoom);
        assert_eq!(census(&mut app), full);
    }

    #[test]
    fn a_drag_spreads_a_stack_and_every_step_is_acknowledged() {
        use slotted_model::{DragKind, DragStage};
        let mut app = first_day();
        let before = census(&mut app);
        locker(&mut app);
        open(&mut app, true);
        click(&mut app, left(46));
        assert_eq!(cursor(&mut app), Some(of(FEED, 4, 0)));
        // A press on the first cell, a sweep over the second, a release:
        // four requests, each predicted with the drag as the last left it.
        let drag = |stage, slot: Option<u16>| ClickAction::Drag {
            stage,
            kind: DragKind::Left,
            slot: slot.map(SlotIx),
        };
        let steps = [
            drag(DragStage::Start, None),
            drag(DragStage::Add, Some(0)),
            drag(DragStage::Add, Some(1)),
            drag(DragStage::End, None),
        ];
        for step in steps {
            click(&mut app, step);
        }
        assert_eq!(at(&mut app, 0), Some(of(FEED, 2, 0)), "spread evenly");
        assert_eq!(at(&mut app, 1), Some(of(FEED, 2, 0)));
        assert_eq!(cursor(&mut app), None);
        assert_eq!(census(&mut app), before);
        let opened = app.world().resource::<OpenStock>().0.clone().unwrap();
        assert_eq!(opened.state.drag, None, "the drag ended on the truth too");
    }

    #[test]
    fn after_the_truth_every_later_click_of_the_tick_gets_the_truth() {
        let mut app = first_day();
        locker(&mut app);
        open(&mut app, true);
        // Two clicks in one tick. The screen was wrong about the first and
        // right about the second, which it predicted on top of the first.
        let second = {
            let (mut pack, mut hands) = carried(&mut app);
            let mut store = app.world().resource::<HouseholdStore>().clone();
            let registries = app.world().resource::<Registries>().clone();
            let mut state = app.world().resource::<OpenStock>().0.clone().unwrap().state;
            let mut stock = Stock {
                saddle: None,
                pack: &mut pack,
                hands: &mut hands,
                store: Some(&mut store),
            };
            apply(&registries.0.items, &mut stock, &mut state, left(46)).unwrap();
            apply(&registries.0.items, &mut stock, &mut state, left(0)).unwrap()
        };
        app.world_mut().resource_mut::<StockInbox>().0.extend([
            StockRequest::Click {
                menu: MENU,
                action: left(46),
                predicted: Delta::default(),
            },
            StockRequest::Click {
                menu: MENU,
                action: left(0),
                predicted: second,
            },
        ]);
        app.update();
        let answers: Vec<StockAnswer> = app
            .world_mut()
            .resource_mut::<Messages<StockAnswer>>()
            .drain()
            .collect();
        // An acknowledgement of the second would leave the screen at the
        // first answer's truth, with the magazines still on the cursor.
        let [
            StockAnswer::Resync { .. },
            StockAnswer::Resync { snapshot, .. },
        ] = &answers[..]
        else {
            panic!("{answers:?}");
        };
        assert_eq!(snapshot.state.carried, None);
        assert_eq!(snapshot.state.state_id, 2);
        let (pack, _) = carried(&mut app);
        assert_eq!(snapshot.inventories[StockLayout::PACK], pack.cells);
        assert_eq!(at(&mut app, 0), Some(of(FEED, 4, 0)));
        // The next tick starts clean: a right prediction is acknowledged.
        assert_eq!(click(&mut app, left(0)), 3);
    }

    /// The household has a store grade bike and a saddle with two feed
    /// modules in its first cell.
    fn with_bike(app: &mut App) -> VehicleBike {
        let registries = app.world().resource::<Registries>().clone();
        let vehicle = Namespaced::parse("veyra:vehicle.courier_bike").unwrap();
        let record = crate::ride::store_grade_bike(&registries.0.items, &vehicle).unwrap();
        let mut stable = Stable::with_bike(record, &registries.0.items, &registries.0.vehicles);
        let feed = crate::crafting::tests::part(&registries, FEED, 2, None);
        crate::crafting::tests::put(&mut stable.saddle, 0, &feed, &registries.0.items);
        *app.world_mut().resource_mut::<Stable>() = stable;
        VehicleBike(registries.0.vehicles.get(&vehicle).unwrap().clone())
    }

    struct VehicleBike(strata_registry::vehicle::VehicleDef);

    fn ride(app: &mut App, bike: &VehicleBike, on: bool) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<Entity, With<LocalPlayer>>();
        let player = players.single(world).unwrap();
        if on {
            world.entity_mut(player).insert(Riding::new(&bike.0, 0.0));
        } else {
            world.entity_mut(player).remove::<Riding>();
        }
    }

    fn saddle_layout(app: &mut App, access: SaddleAccess, with_store: bool) -> StockLayout {
        let (pack, _) = carried(app);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let cells = u16::try_from(app.world().resource::<Stable>().saddle.len()).unwrap();
        layout(
            items.carry(),
            items,
            &pack,
            with_store,
            Some(SaddleSlots { cells, access }),
        )
    }

    /// A click sent as the screen would after a prediction that was wrong:
    /// the sim answers with the truth, and the refusal if it gave one.
    fn forced(app: &mut App, action: ClickAction) -> Option<Refusal> {
        let (answers, refused) = send(
            app,
            StockRequest::Click {
                menu: MENU,
                action,
                predicted: Delta::default(),
            },
        );
        assert!(
            answers
                .iter()
                .all(|answer| matches!(answer, StockAnswer::Resync { .. })),
            "{answers:?}"
        );
        refused.first().map(|refusal| refusal.reason)
    }

    fn saddle_cell(app: &App, cell: usize) -> Option<ItemStack> {
        app.world().resource::<Stable>().saddle.get(cell).cloned()
    }

    /// The saddle is an inventory of every menu, so the store keeps its
    /// number; its cells are declared at full size and are open mounted or
    /// at a locker and shut otherwise.
    #[test]
    fn the_saddle_is_declared_whole_and_open_only_by_its_access() {
        let mut app = first_day();
        let _ = with_bike(&mut app);
        assert_eq!(
            (StockLayout::SADDLE, StockLayout::STORE),
            (InventoryRef::new(5), InventoryRef::new(6))
        );
        for (access, with_store, behaviour) in [
            (SaddleAccess::Shut, false, SlotBehaviour::Disabled),
            (SaddleAccess::Shut, true, SlotBehaviour::Disabled),
            (SaddleAccess::Mounted, false, SlotBehaviour::Normal),
            (SaddleAccess::Locker, true, SlotBehaviour::Normal),
        ] {
            let layout = saddle_layout(&mut app, access, with_store);
            let saddle = layout.saddle.expect("declared with a bike");
            assert_eq!(saddle.end - saddle.start, 18, "the cargo, in full");
            assert_eq!(behaviours(&layout, saddle), vec![behaviour; 18]);
            assert_eq!(saddle.start, layout.hands.end, "after the hands");
            if let Some(store) = layout.store {
                assert_eq!(store.start, saddle.end, "the store stays last");
            }
        }
        assert_eq!(
            SaddleAccess::of(true, true),
            SaddleAccess::Mounted,
            "riding opens it"
        );
        assert_eq!(SaddleAccess::of(false, true), SaddleAccess::Locker);
        assert_eq!(SaddleAccess::of(false, false), SaddleAccess::Shut);
        let (pack, _) = carried(&mut app);
        let registries = app.world().resource::<Registries>().clone();
        let none = layout(
            registries.0.items.carry(),
            &registries.0.items,
            &pack,
            true,
            None,
        );
        assert_eq!(none.saddle, None, "no bike, no grid");
    }

    /// The rule is asked at every click: on foot the saddle is shut, mounted
    /// a stack leaves it, and back on foot the cursor cannot put it back.
    #[test]
    fn the_saddle_is_asked_at_every_click() {
        let mut app = first_day();
        let bike = with_bike(&mut app);
        assert_eq!(open(&mut app, false), []);
        let first = saddle_layout(&mut app, SaddleAccess::Mounted, false)
            .saddle
            .unwrap()
            .start;
        assert_eq!(forced(&mut app, left(first)), Some(Refusal::SaddleShut));
        assert!(saddle_cell(&app, 0).is_some(), "nothing moved");

        ride(&mut app, &bike, true);
        assert_eq!(forced(&mut app, left(first)), None);
        assert!(saddle_cell(&app, 0).is_none(), "the stack left the saddle");
        assert!(carried(&mut app).0.cursor.is_some(), "and is on the cursor");

        ride(&mut app, &bike, false);
        assert_eq!(forced(&mut app, left(first)), Some(Refusal::SaddleShut));
        assert!(saddle_cell(&app, 0).is_none());
        assert!(carried(&mut app).0.cursor.is_some(), "still on the cursor");

        ride(&mut app, &bike, true);
        assert_eq!(forced(&mut app, left(first + 3)), None);
        assert_eq!(
            saddle_cell(&app, 3).map(|stack| stack.count),
            Some(2),
            "put into another cell"
        );
        // The truth the screen is resynced with holds the saddle too.
        let (answers, _) = send(
            &mut app,
            StockRequest::Click {
                menu: MENU,
                action: left(first + 3),
                predicted: Delta::default(),
            },
        );
        let Some(StockAnswer::Resync { snapshot, .. }) = answers.last() else {
            panic!("a resync");
        };
        assert_eq!(
            snapshot.inventories[StockLayout::SADDLE],
            app.world().resource::<Stable>().saddle
        );
    }

    /// Shift-click: mounted, between the pack and the saddle; at a locker,
    /// from the saddle to the store and from the pack to the store as ever.
    #[test]
    fn shift_click_routes_the_saddle_mounted_and_at_a_locker() {
        let mut app = first_day();
        let bike = with_bike(&mut app);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        // One feed module in the pack's last cell, so it is not in the way
        // of what the first day put first.
        let module = crate::crafting::tests::part(&registries, CELL, 1, None);
        let last = {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            let last = pack.cells.len() - 1;
            pack.cells.set(last, None);
            crate::crafting::tests::put(&mut pack.cells, last, &module, items);
            last
        };
        let last = u16::try_from(last).unwrap();

        // Mounted, no store: pack to saddle, saddle to pack.
        ride(&mut app, &bike, true);
        assert_eq!(open(&mut app, false), []);
        let saddle = saddle_layout(&mut app, SaddleAccess::Mounted, false)
            .saddle
            .unwrap();
        assert_eq!(forced(&mut app, shift(last)), None);
        let cell = (0..18).find(|cell| {
            saddle_cell(&app, *cell)
                .and_then(|stack| read(&stack, items).ok())
                .is_some_and(|record| record.item == module.item)
        });
        assert!(cell.is_some(), "the module went to the saddle");
        let (pack, _) = carried(&mut app);
        assert!(pack.cells.get(usize::from(last)).is_none());
        let cell = u16::try_from(cell.unwrap()).unwrap();
        assert_eq!(forced(&mut app, shift(saddle.start + cell)), None);
        assert!(saddle_cell(&app, usize::from(cell)).is_none());
        let (pack, _) = carried(&mut app);
        let in_pack = |pack: &Pack| {
            std::iter::once(&pack.cells)
                .chain(&pack.bags)
                .flat_map(|inventory| inventory.slots().iter().flatten())
                .filter(|stack| read(stack, items).is_ok_and(|record| record.item == module.item))
                .count()
        };
        assert!(in_pack(&pack) >= 1, "and back to the pack");

        // At a locker with the store open, on foot: saddle to store.
        ride(&mut app, &bike, false);
        let _ = locker(&mut app);
        assert_eq!(send(&mut app, StockRequest::Close { menu: MENU }).1, []);
        assert_eq!(open(&mut app, true), []);
        let at = saddle_layout(&mut app, SaddleAccess::Locker, true);
        let (saddle, store) = (at.saddle.unwrap(), at.store.unwrap());
        let before = app.world().resource::<HouseholdStore>().clone();
        assert_eq!(forced(&mut app, shift(saddle.start)), None);
        assert!(saddle_cell(&app, 0).is_none(), "the feed left the saddle");
        let after = app.world().resource::<HouseholdStore>().clone();
        assert_ne!(before, after, "and is in the store");
        let landed = (0..store.end - store.start)
            .find(|cell| before.0.get(usize::from(*cell)) != after.0.get(usize::from(*cell)));
        assert!(landed.is_some());
        // The pack still sends to the store.
        let spare = {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            let cell = (0..pack.cells.len())
                .find(|cell| pack.cells.get(*cell).is_none())
                .expect("a free cell");
            crate::crafting::tests::put(&mut pack.cells, cell, &module, items);
            cell
        };
        assert_eq!(forced(&mut app, shift(u16::try_from(spare).unwrap())), None);
        assert!(carried(&mut app).0.cells.get(spare).is_none());
    }
}
