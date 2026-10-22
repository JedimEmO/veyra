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
//! `StockRequest` in the `StockInbox`; `consume` replays it on the
//! truth with the same click rules ([`apply`], a pure function over
//! `slotted_model::apply_click_validated`) and answers a `StockAnswer`:
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

use bevy_ecs::prelude::*;

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
pub fn slot_of(action: ClickAction) -> Option<SlotIx> {
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

/// The kind of stack `record` is: itself, with a count of one.
#[must_use]
pub fn kind_of(record: &Record) -> Record {
    Record {
        count: 1,
        ..record.clone()
    }
}

/// Every stack a bench can reach, read: the pack's cells, its bags' open
/// cells and, when given, the household's store, each with whether it is
/// the store's. A stack the catalogue cannot read is no stock.
pub fn reachable(
    items: &ItemCatalogue,
    pack: &Pack,
    store: Option<&HouseholdStore>,
) -> Vec<(Record, bool)> {
    let open: [usize; BAG_SLOTS] =
        std::array::from_fn(|bag| usize::from(pack.open_cells(bag, items)));
    let carried = pack.cells.slots().iter().flatten().chain(
        pack.bags
            .iter()
            .zip(open)
            .flat_map(|(bag, open)| bag.slots().iter().take(open).flatten()),
    );
    let stored = store
        .into_iter()
        .flat_map(|store| store.0.slots().iter().flatten());
    carried
        .map(|stack| (stack, false))
        .chain(stored.map(|stack| (stack, true)))
        .filter_map(|(stack, stored)| Some((read(stack, items).ok()?, stored)))
        .collect()
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

    use slotted_model::Button;
    use slotted_model::ClickAction;

    use slotted_model::ItemId;
    use slotted_model::ItemStack;

    use slotted_model::MenuId;

    use slotted_model::SlotBehaviour;

    use slotted_model::SlotIx;
    use slotted_model::SlotRange;

    use slotted_model::Value;

    use std::sync::Arc;

    use strata_registry::item::Fitted;

    use strata_registry::item::ItemError;

    use strata_registry::item::Record;
    use strata_registry::item::STORE_GRADE;

    use strata_types::Registries;

    pub const MENU: MenuId = MenuId(7);

    pub const FEED: &str = "veyra:module.patrol_extended_feed";

    pub const CELL: &str = "veyra:module.pistol_extended_cell";

    pub const SATCHEL: &str = "veyra:bag.field_satchel";

    pub const HAUL: &str = "veyra:bag.haul_pack";

    pub fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn left(slot: u16) -> ClickAction {
        ClickAction::Pickup {
            slot: SlotIx(slot),
            button: Button::Left,
        }
    }

    pub fn right(slot: u16) -> ClickAction {
        ClickAction::Pickup {
            slot: SlotIx(slot),
            button: Button::Right,
        }
    }

    pub fn shift(slot: u16) -> ClickAction {
        ClickAction::QuickMove { slot: SlotIx(slot) }
    }

    pub fn triple(stack: &ItemStack, registries: &Registries) -> (String, u32, u32) {
        let record = read(stack, &registries.0.items).unwrap();
        (record.item.to_string(), record.count, record.batch)
    }

    pub fn of(item: &str, count: u32, batch: u32) -> (String, u32, u32) {
        (item.to_owned(), count, batch)
    }

    pub fn behaviours(layout: &StockLayout, slots: SlotRange) -> Vec<SlotBehaviour> {
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

    pub struct VehicleBike(pub strata_registry::vehicle::VehicleDef);
}
