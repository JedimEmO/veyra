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

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use slotted_model::Inventory;
use strata_registry::item::{BAG_SLOTS, HANDS, ItemCatalogue, Record};

use crate::combat::{Action, Combatant};
use crate::crafting::kind_of;
use crate::fitting;
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer, StoreAccess};
use crate::progression::{InteractionKind, Reach};
use crate::{LocalPlayer, Registries, SimSet, SimTick};

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
pub(crate) enum Cell {
    /// A cell of the pack's own grid.
    Pack(usize),
    /// A cell under a bag slot.
    Bag(usize, usize),
    /// A cell of the household's store.
    Store(usize),
}

/// What a request or a screen asks of the hands.
#[derive(Debug, Clone, PartialEq)]
pub enum EquipRequest {
    /// Swap the weapon of this kind, from the pack or the open store, with
    /// the weapon in `hand`.
    Take {
        /// The weapon, as a stack of its kind is read.
        kind: Record,
        /// The hand, 0 or 1.
        hand: usize,
    },
}

/// The requests not yet consumed, a resource for the reason the stock's
/// inbox is one.
#[derive(Resource, Debug, Default)]
pub struct EquipInbox(pub Vec<EquipRequest>);

/// A weapon taken in hand.
#[derive(Debug, Clone, PartialEq)]
pub struct Equipped {
    /// The hand it is in now.
    pub hand: usize,
    /// The weapon, as it was read where it lay.
    pub item: Record,
}

/// The sim's answer to an [`EquipRequest`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct EquipAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Equipped, EquipRefusal>,
}

impl Cell {
    /// The cell's index in its inventory.
    pub(crate) const fn index(self) -> usize {
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
pub(crate) fn find(
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

/// `SimTick`, `SimSet::Input`, after the stock's own requests and before
/// the loadout follows the hands: every request made since the last tick,
/// in order, each on the stock the one before left. The store is reached
/// by the same rule a click on it has: a menu open over it and a locker in
/// reach.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the open menu, the store's rule and the stock in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<EquipInbox>,
    open: Res<OpenStock>,
    access: Res<StoreAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands, Option<&Combatant>), With<LocalPlayer>>,
    mut answers: MessageWriter<EquipAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let items = &registries.0.items;
    let at_locker = *access == StoreAccess::Anywhere
        || matches!(reach.nearest, Some((_, InteractionKind::Locker)));
    let with_store = open
        .0
        .as_ref()
        .is_some_and(|opened| opened.with_store && at_locker);
    let mut player = players.single_mut().ok();
    let mut changed = false;
    for request in requests {
        let EquipRequest::Take { kind, hand } = request;
        let result = match player.as_mut() {
            None => Err(EquipRefusal::NotHeld),
            Some((pack, hands, actor)) => {
                let ready = actor.is_none_or(|actor| actor.action == Action::Ready);
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: with_store.then_some(&mut next.2),
                };
                let done = take(items, &mut stock, &kind, hand, ready);
                if done.is_ok() {
                    changed = true;
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                }
                done
            }
        };
        answers.write(EquipAnswer { result });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some((pack, hands, _)) = player.as_mut()
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

/// Registers the request and consumes it in the tick. Added by `SimPlugin`,
/// so a server and a harness have it too; with no request it touches
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct EquipPlugin;

impl Plugin for EquipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EquipInbox>()
            .add_message::<EquipAnswer>()
            .add_systems(
                SimTick,
                consume
                    .in_set(SimSet::Input)
                    .after(crate::wear::consume)
                    .before(fitting::sync_loadout),
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
    use crate::crafting::tests::{first_day, id, part, prop, put};
    use bevy::ecs::message::Messages;
    use std::collections::BTreeMap;
    use strata_registry::item::{Fitted, QUALITY_MAX};

    const CARBINE: &str = "veyra:weapon.patrol_carbine";
    const PISTOL: &str = "veyra:weapon.service_pistol";
    const GRIP: &str = "veyra:module.vigil_angled_grip";

    /// A carbine a smith made: batch 7, every line 650, whole.
    fn made(app: &App) -> Record {
        let registries = app.world().resource::<Registries>().clone();
        Record {
            batch: 7,
            by: Some("Tovan Reyes".to_owned()),
            lines: ["interval", "durability", "moddability"]
                .into_iter()
                .map(|line| (line.to_owned(), 650))
                .collect(),
            condition: Some(QUALITY_MAX),
            ..part(&registries, CARBINE, 1, None)
        }
    }

    /// The pack, the hands and the store as they stand.
    fn stock(app: &mut App) -> (Pack, Hands, HouseholdStore) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<(&Pack, &Hands), With<LocalPlayer>>();
        let (pack, hands) = players.single(world).unwrap();
        let (pack, hands) = (pack.clone(), hands.clone());
        (pack, hands, world.resource::<HouseholdStore>().clone())
    }

    fn ask(app: &mut App, kind: &Record, hand: usize) -> Result<Equipped, EquipRefusal> {
        app.world_mut()
            .resource_mut::<EquipInbox>()
            .0
            .push(EquipRequest::Take {
                kind: kind.clone(),
                hand,
            });
        app.update();
        let mut answers: Vec<EquipAnswer> = app
            .world_mut()
            .resource_mut::<Messages<EquipAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        answers.pop().unwrap().result
    }

    /// The carbine in hand 0 gets a grip fitted and a worn condition, and
    /// `carbine` lies in the first empty cell of the pack, or the store.
    fn set_up(app: &mut App, in_store: bool) -> (Record, Record) {
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let grip = Fitted {
            item: id(GRIP),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("equip_time".to_owned(), 500),
                ("recoil_recovery".to_owned(), 500),
            ]),
            condition: Some(700),
        };
        let carbine = made(app);
        let world = app.world_mut();
        let mut players = world.query_filtered::<(&mut Pack, &mut Hands), With<LocalPlayer>>();
        let (mut pack, mut hands) = players.single_mut(world).unwrap();
        let old = items::read(hands.0.get(0).unwrap(), items).unwrap();
        let mut old = crate::fitting::with(items, &old, "grip", Some(&grip)).unwrap();
        old.condition = Some(640);
        put(&mut hands.0, 0, &old, items);
        if in_store {
            let mut store = world.resource_mut::<HouseholdStore>();
            let cell = store.0.slots().iter().position(Option::is_none).unwrap();
            put(&mut store.0, cell, &carbine, items);
        } else {
            let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
            put(&mut pack.cells, cell, &carbine, items);
        }
        (old, carbine)
    }

    #[test]
    fn a_carbine_in_the_pack_is_taken_and_the_old_one_lies_where_it_was() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let (old, carbine) = set_up(&mut app, false);
        app.update();
        {
            let world = app.world_mut();
            let mut actors = world.query_filtered::<&Combatant, With<LocalPlayer>>();
            let before = actors.single(world).unwrap().loadout().cloned();
            assert!(
                before.is_some_and(|equipped| !equipped.is_empty()),
                "the grip"
            );
        }
        let (pack, ..) = stock(&mut app);
        let cell = pack
            .cells
            .slots()
            .iter()
            .position(|stack| {
                stack
                    .as_ref()
                    .and_then(|stack| items::read(stack, items).ok())
                    .is_some_and(|record| record == carbine)
            })
            .unwrap();
        assert_eq!(
            ask(&mut app, &carbine, 0),
            Ok(Equipped {
                hand: 0,
                item: carbine.clone()
            })
        );
        let (pack, hands, _) = stock(&mut app);
        let saved = hands.saved(items).unwrap();
        assert_eq!(saved[0].as_ref(), Some(&carbine));
        assert_eq!(saved[1].as_ref().unwrap().item, id(PISTOL));
        let lies = items::read(pack.cells.get(cell).unwrap(), items).unwrap();
        assert_eq!(lies, old, "it keeps its condition and what is fitted");
        assert_eq!(lies.condition, Some(640));
        assert!(lies.fitted.contains_key("grip"));
        // The loadout follows the hands in the same tick.
        let world = app.world_mut();
        let mut actors = world.query_filtered::<&Combatant, With<LocalPlayer>>();
        let actor = actors.single(world).unwrap();
        let expected = crate::fitting::loadout(items, &saved);
        // Nothing is fitted to it, so all it carries is its own action line.
        assert!(
            expected.mods.values().all(|mods| mods.len() == 1
                && mods[0].property == strata_registry::item::Property::RepeatInterval),
            "the made carbine has nothing fitted"
        );
        assert!(
            actor
                .loadout()
                .is_some_and(|equipped| *equipped == expected),
            "the grip on the old carbine is no longer equipped"
        );
    }

    #[test]
    fn the_four_refusals() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let (_, carbine) = set_up(&mut app, false);
        let before = stock(&mut app);
        let grip = part(&registries, GRIP, 1, None);
        assert_eq!(ask(&mut app, &grip, 0), Err(EquipRefusal::NotAWeapon));
        assert_eq!(ask(&mut app, &carbine, 1), Err(EquipRefusal::SameFamily));
        let mut absent = carbine.clone();
        absent.batch = 99;
        assert_eq!(ask(&mut app, &absent, 0), Err(EquipRefusal::NotHeld));
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            pack.cursor =
                Some(items::write(&part(&registries, GRIP, 1, None), &registries.0.items).unwrap());
        }
        assert_eq!(ask(&mut app, &carbine, 0), Err(EquipRefusal::Busy));
        let (mut pack, hands, store) = stock(&mut app);
        pack.cursor = None;
        assert_eq!((pack, hands, store), before, "nothing moved");
        // Not a hand at all.
        assert_eq!(ask(&mut app, &carbine, 2), Err(EquipRefusal::NotHeld));
    }

    #[test]
    fn the_stores_carbine_is_reached_only_with_the_store_open() {
        let mut app = first_day();
        app.insert_resource(StoreAccess::Anywhere);
        let (old, carbine) = set_up(&mut app, true);
        // No menu open over the store: not to hand.
        assert_eq!(ask(&mut app, &carbine, 0), Err(EquipRefusal::NotHeld));
        app.world_mut().resource_mut::<OpenStock>().0 = Some(items::Opened {
            menu: slotted_model::MenuId(3),
            with_store: false,
            state: slotted_model::MenuState::new(&slotted_model::MenuDef::new()),
            saddle: slotted_model::Inventory::new(0),
        });
        assert_eq!(ask(&mut app, &carbine, 0), Err(EquipRefusal::NotHeld));
        app.world_mut()
            .resource_mut::<OpenStock>()
            .0
            .as_mut()
            .unwrap()
            .with_store = true;
        // The store menu is open but no locker is in reach.
        app.insert_resource(StoreAccess::AtLocker);
        assert_eq!(ask(&mut app, &carbine, 0), Err(EquipRefusal::NotHeld));
        prop(&mut app, InteractionKind::Locker);
        assert!(ask(&mut app, &carbine, 0).is_ok());
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let (_, hands, store) = stock(&mut app);
        assert_eq!(hands.saved(items).unwrap()[0].as_ref(), Some(&carbine));
        let back = store
            .0
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, items).ok())
            .find(|record| record.item == old.item)
            .unwrap();
        assert_eq!(back, old, "the old one lies in the store's cell");
        // A weapon that lies nowhere is not to hand.
        let mut again = carbine.clone();
        again.batch = 8;
        assert_eq!(ask(&mut app, &again, 0), Err(EquipRefusal::NotHeld));
    }

    /// `carbine` lifted from its cell onto the cursor.
    fn carry(app: &mut App, carbine: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = find(items, &pack, None, &kind_of(carbine)).unwrap();
        pack.cursor = pack.cells.take(cell.index());
    }

    #[test]
    fn a_weapon_on_the_cursor_is_taken_and_the_old_one_takes_its_place() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let (old, carbine) = set_up(&mut app, false);
        carry(&mut app, &carbine);
        let (before, ..) = stock(&mut app);
        assert_eq!(
            ask(&mut app, &carbine, 0),
            Ok(Equipped {
                hand: 0,
                item: carbine.clone()
            })
        );
        let (pack, hands, _) = stock(&mut app);
        assert_eq!(hands.saved(items).unwrap()[0].as_ref(), Some(&carbine));
        let cursor = items::read(pack.cursor.as_ref().unwrap(), items).unwrap();
        assert_eq!(cursor, old, "the old one is carried now");
        assert_eq!(pack.cells, before.cells, "no cell was touched");
    }

    #[test]
    fn the_same_family_is_refused_from_the_cursor_and_mid_action_changes_nothing() {
        let mut app = first_day();
        let (_, carbine) = set_up(&mut app, false);
        carry(&mut app, &carbine);
        let before = stock(&mut app);
        assert_eq!(ask(&mut app, &carbine, 1), Err(EquipRefusal::SameFamily));
        assert_eq!(stock(&mut app), before);
        {
            let world = app.world_mut();
            let mut actors = world.query_filtered::<&mut Combatant, With<LocalPlayer>>();
            actors.single_mut(world).unwrap().action = Action::Reload;
        }
        assert_eq!(ask(&mut app, &carbine, 0), Err(EquipRefusal::MidAction));
        assert_eq!(stock(&mut app), before);
    }

    const GLAIVE: &str = "veyra:weapon.pier_glaive";
    const BLADE: &str = "veyra:weapon.service_blade";

    /// A bought `item` lying in the first empty cell of the pack.
    fn lies_in_the_pack(app: &mut App, item: &str) -> Record {
        let registries = app.world().resource::<Registries>().clone();
        let record = part(&registries, item, 1, None);
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        put(&mut pack.cells, cell, &record, &registries.0.items);
        record
    }

    /// The Combatant's slots and the weapon it holds.
    fn slots(app: &mut App) -> ([Option<crate::combat::Weapon>; 2], crate::combat::Weapon) {
        let world = app.world_mut();
        let mut actors = world.query_filtered::<&Combatant, With<LocalPlayer>>();
        let actor = actors.single(world).unwrap();
        (actor.slots(), actor.weapon)
    }

    #[test]
    fn either_hand_takes_any_weapon_and_the_combatants_slots_follow() {
        use crate::combat::Weapon;
        let mut app = first_day();
        let glaive = lies_in_the_pack(&mut app, GLAIVE);
        let blade = lies_in_the_pack(&mut app, BLADE);
        assert_eq!(
            slots(&mut app).0,
            [Some(Weapon::Carbine), Some(Weapon::Pistol)]
        );
        // The glaive goes to the pistol's hand; the carbine stays in hand.
        assert_eq!(
            ask(&mut app, &glaive, 1),
            Ok(Equipped {
                hand: 1,
                item: glaive.clone()
            })
        );
        assert_eq!(
            slots(&mut app),
            (
                [Some(Weapon::Carbine), Some(Weapon::Glaive)],
                Weapon::Carbine
            )
        );
        // The blade goes to the hand in use: it is in hand at once.
        assert_eq!(
            ask(&mut app, &blade, 0),
            Ok(Equipped {
                hand: 0,
                item: blade.clone()
            })
        );
        assert_eq!(
            slots(&mut app),
            ([Some(Weapon::Blade), Some(Weapon::Glaive)], Weapon::Blade)
        );
        // And never two of a family: a second glaive is refused for the
        // other hand, and nothing moves.
        let second = lies_in_the_pack(&mut app, GLAIVE);
        let before = stock(&mut app);
        assert_eq!(ask(&mut app, &second, 0), Err(EquipRefusal::SameFamily));
        assert_eq!(stock(&mut app), before);
        // The glaive may take its own hand again.
        assert_eq!(
            ask(&mut app, &second, 1),
            Ok(Equipped {
                hand: 1,
                item: second.clone()
            })
        );
        assert_eq!(
            slots(&mut app).0,
            [Some(Weapon::Blade), Some(Weapon::Glaive)]
        );
        let registries = app.world().resource::<Registries>().clone();
        let (_, hands, _) = stock(&mut app);
        assert_eq!(
            registries
                .0
                .items
                .check_hands(&hands.saved(&registries.0.items).unwrap()),
            Ok(())
        );
        assert_eq!(
            hand_for(
                &registries.0.items,
                &hands.saved(&registries.0.items).unwrap(),
                &glaive,
                0
            ),
            1,
            "a weapon goes to the hand holding its family"
        );
        let bag = lies_in_the_pack(&mut app, "veyra:weapon.yard_greatblade");
        assert_eq!(
            hand_for(
                &registries.0.items,
                &hands.saved(&registries.0.items).unwrap(),
                &bag,
                0
            ),
            1,
            "else the hand not in use"
        );
    }
}
