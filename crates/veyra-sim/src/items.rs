//! The adapter half of the `items` rules: its requests, systems and plugin.

pub use veyra_rules::items::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use slotted_model::{ClickAction, Delta, MenuDef, MenuId, MenuSnapshot, MenuState};

use crate::household::{Riding, Stable};

use crate::order::RequestSet;

use crate::progression::{InteractionKind, Reach};

use crate::{AppRequestExt, Inbox, Registries, Request};

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
pub type StockInbox = Inbox<StockRequest>;

/// A click with nobody to click for is refused, for the screen to say so.
impl Request for StockRequest {
    type Answer = StockRefused;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(StockRefused {
            menu: self.menu(),
            slot: None,
            reason: Refusal::NoPlayer,
        });
    }
}

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
    mut players: Query<(&mut Pack, &mut Hands, Has<Riding>)>,
    mut answers: MessageWriter<StockAnswer>,
    mut refusals: MessageWriter<StockRefused>,
    mut commands: Commands,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let items = &registries.0.items;
    let at_locker = *access == StoreAccess::Anywhere
        || matches!(reach.nearest, Some((_, InteractionKind::Locker)));
    // The menu a click of this tick was answered with the truth for. That
    // truth was taken before the clicks after it, so each of those is
    // answered with the truth as well: an acknowledgement would leave the
    // screen at the earlier one.
    let mut resynced = None;
    for (actor, request) in requests {
        let menu = request.menu();
        let Ok((mut pack, mut hands, riding)) = players.get_mut(actor.0) else {
            request.no_actor(&mut commands);
            continue;
        };
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
        app.init_resource::<OpenStock>()
            .init_resource::<StoreAccess>()
            .init_resource::<HouseholdStore>()
            .add_message::<StockAnswer>()
            .add_request::<StockRequest, _>(RequestSet::Stock, consume);
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
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;

    use bevy::app::App;

    use slotted_model::ClickAction;
    use slotted_model::ClickError;
    use slotted_model::Delta;

    use slotted_model::InventoryRef;

    use slotted_model::ItemStack;

    use slotted_model::MenuSnapshot;

    use slotted_model::Namespaced;

    use slotted_model::SlotBehaviour;

    use slotted_model::SlotIx;
    use slotted_model::SlotRange;

    use std::collections::BTreeMap;

    use strata_registry::item::Record;

    use crate::household::Riding;
    use crate::household::Stable;

    use crate::progression::InteractionKind;

    use crate::Registries;

    pub(crate) use veyra_rules::items::test_support::*;

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
        app.world_mut().resource_mut::<StockInbox>().push(request);
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

    fn cursor(app: &mut App) -> Option<(String, u32, u32)> {
        let (pack, _) = carried(app);
        let registries = app.world().resource::<Registries>().clone();
        pack.cursor.as_ref().map(|stack| triple(stack, &registries))
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
        app.world_mut().resource_mut::<StockInbox>().extend([
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
