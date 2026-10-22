//! The adapter half of the `equip` rules: its requests, systems and plugin.

pub use veyra_rules::equip::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_registry::item::Record;

use crate::combat::{Action, Combatant};

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer, StoreAccess};

use crate::order::RequestSet;

use crate::progression::{InteractionKind, Reach};

use crate::{AppRequestExt, Inbox, Registries, Request};

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
pub type EquipInbox = Inbox<EquipRequest>;

/// With nobody to take it, the weapon is as good as not held.
impl Request for EquipRequest {
    type Answer = EquipAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(EquipAnswer {
            result: Err(EquipRefusal::NotHeld),
        });
    }
}

/// The sim's answer to an [`EquipRequest`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct EquipAnswer {
    /// What happened, or why nothing did.
    pub result: Result<Equipped, EquipRefusal>,
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
    mut players: Query<(&mut Pack, &mut Hands, Option<&Combatant>)>,
    mut answers: MessageWriter<EquipAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let items = &registries.0.items;
    let at_locker = *access == StoreAccess::Anywhere
        || matches!(reach.nearest, Some((_, InteractionKind::Locker)));
    let with_store = open
        .0
        .as_ref()
        .is_some_and(|opened| opened.with_store && at_locker);
    let mut changed = false;
    let mut last = None;
    for (actor, request) in requests {
        let EquipRequest::Take { kind, hand } = request;
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
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
        && let Some(actor) = last
        && let Ok((pack, hands, _)) = players.get(actor)
    {
        let mut truth = ((*pack).clone(), (*hands).clone(), store.clone());
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
        app.add_request::<EquipRequest, _>(RequestSet::Equip, consume);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::LocalPlayer;
    use crate::crafting::tests::first_day;
    use crate::crafting::tests::id;
    use crate::crafting::tests::part;
    use crate::crafting::tests::prop;
    use crate::crafting::tests::put;
    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use std::collections::BTreeMap;
    use strata_registry::item::Fitted;
    use strata_registry::item::QUALITY_MAX;

    use crate::combat::Action;
    use crate::combat::Combatant;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;
    use crate::items::OpenStock;
    use crate::items::Pack;
    use crate::items::kind_of;
    use strata_registry::item::Record;

    use crate::items::StoreAccess;

    use crate::progression::InteractionKind;

    use crate::Registries;

    pub(crate) use veyra_rules::equip::test_support::*;

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
