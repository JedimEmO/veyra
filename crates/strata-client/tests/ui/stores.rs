//! The stores and inventory screens and the item card (ticket 0007, step 1).

use bevy::math::Rect;
use bevy::math::Vec2;
use bevy::prelude::*;
use slotted::menu::kinds;
use slotted::model::ItemStack;
use slotted::ui::InputMode;
use slotted::ui::SemanticLabel;
use slotted::ui::UiAction;
use slotted::ui::def::Tags;
use slotted_test::prelude::*;
use strata_client::ui;
use strata_client::ui::card::CardParams;
use strata_client::ui::fitting;
use strata_client::ui::stores;
use strata_client::ui::stores::SlotDressing;
use strata_client::ui::stores::StoresState;
use strata_sim::items::HouseholdStore;
use strata_sim::items::OpenStock;
use strata_sim::items::Pack;
use strata_sim::progression::Interact;
use strata_sim::progression::InteractionKind;

use crate::common::*;

/// A stack is carried from the household's store to the pack by the mouse:
/// a click takes it up, a click puts it down, and the sim's own pack and
/// store are what moved. A right click takes half and puts one down, two
/// batches of one module swap and do not merge, and when the screen
/// closes nothing was made or lost.
#[test]
fn a_stack_crosses_by_mouse() {
    let mut day = first_day("stores-mouse", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    assert_eq!(day.says(stores::TITLE_ID), "Stores");
    assert_eq!(
        day.h.world().resource::<StoresState>().with_store(),
        Some(true)
    );
    let feed = day.id(FEED);
    let (store0, store1) = (day.cell("store", 0), day.cell("store", 1));
    let (pack0, pack1) = (day.cell("pack", 0), day.cell("pack", 1));
    assert_eq!(day.shows(store0), Some((feed, 4)));
    assert_eq!(day.shows(store1), Some((feed, 2)), "the crafted batch");
    assert_eq!(day.shows(pack0), None);

    day.h.click(store0);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 4)));
    assert_eq!(day.shows(store0), None);
    assert_eq!(
        day.says(stores::STATUS_ID),
        "Carrying: Patrol extended magazine x4"
    );
    assert_eq!(
        day.pack().cursor.map(|stack| stack.count),
        Some(4),
        "the sim carries it too"
    );
    day.h.click(pack0);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(day.shows(pack0), Some((feed, 4)));
    assert_eq!(
        day.pack().cells.get(0).map(|stack| (stack.id, stack.count)),
        Some((feed, 4)),
        "the truth moved, not only the mirror"
    );
    assert!(day.store().0.get(0).is_none());
    assert!(!day.says(stores::STATUS_ID).starts_with("Carrying"));

    // A right click takes half, and with a stack carried puts one down.
    day.h.right_click(pack0);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 2)));
    assert_eq!(day.shows(pack0), Some((feed, 2)));
    day.h.right_click(pack1);
    day.h.settle();
    assert_eq!(day.shows(pack1), Some((feed, 1)));
    assert_eq!(day.carried(), Some((feed, 1)));
    day.h.click(pack1);
    day.h.settle();
    assert_eq!(day.shows(pack1), Some((feed, 2)), "like stacks merge");
    assert_eq!(day.carried(), None);

    // The crafted batch onto the bought one: a swap, never a merge.
    day.h.click(store1);
    day.h.settle();
    day.h.click(pack0);
    day.h.settle();
    assert_eq!(day.shows(pack0), Some((feed, 2)));
    assert_eq!(day.carried(), Some((feed, 2)), "the bought two came up");
    let batch = |stack: Option<ItemStack>| {
        stack
            .and_then(|stack| strata_sim::items::read(&stack, &day.registries.items).ok())
            .map(|record| record.batch)
    };
    assert_eq!(batch(day.h.stack_at(pack0)), Some(412));
    assert_eq!(batch(day.h.carried(day.menu())), Some(0));
    assert_eq!(
        day.h
            .world()
            .resource::<stores::StockAuthority>()
            .in_flight(),
        0,
        "every click was answered"
    );

    // Closed with a stack on the cursor: the sim puts it away.
    day.close();
    assert!(day.h.stack().is_empty());
    assert_eq!(day.h.world().resource::<StoresState>().menu(), None);
    assert!(day.h.world().resource::<OpenStock>().0.is_none());
    assert_eq!(day.pack().cursor, None, "stowed, not left on the cursor");
    assert_eq!(day.truth(), before, "nothing was made or lost");
    assert!(
        day.h
            .world_mut()
            .query::<&stores::StockMirror>()
            .iter(day.h.world())
            .next()
            .is_none(),
        "the mirror went with the screen"
    );
}

/// The same crossing with the keys alone: the screen opens with the focus
/// on the store's first cell, Accept takes the stack up, the arrows walk
/// out of the store into the pack, Accept puts it down. The secondary
/// action takes half, Shift and Accept send a stack back across, and the
/// footer says what is carried, since the carried icon follows the mouse.
#[test]
fn a_stack_crosses_by_focus_and_keys_alone() {
    let mut day = first_day("stores-keys", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    day.h.set_input_mode(InputMode::Keyboard);
    let feed = day.id(FEED);
    let store0 = day.cell("store", 0);
    let (pack0, pack1) = (day.cell("pack", 0), day.cell("pack", 1));
    assert_eq!(day.h.focused(), Some(store0), "the first store cell");

    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 4)));
    assert_eq!(
        day.says(stores::STATUS_ID),
        "Carrying: Patrol extended magazine x4"
    );
    // Along the store's top row and out of its last column into the pack.
    let cols = day.registries.items.carry().store.cols;
    for _ in 0..cols {
        day.h.action(UiAction::Right);
    }
    day.h.settle();
    assert_eq!(day.h.focused(), Some(pack0), "the arrows cross the regions");
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(
        day.pack().cells.get(0).map(|stack| (stack.id, stack.count)),
        Some((feed, 4))
    );

    // The secondary action is a right click: half up, and down next door.
    day.h.action(UiAction::Secondary);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 2)));
    day.h.action(UiAction::Right);
    day.h.settle();
    assert_eq!(day.h.focused(), Some(pack1));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.shows(pack1), Some((feed, 2)));
    assert_eq!(day.carried(), None);

    // Shift and Accept send the focused stack across.
    day.h.hold(KeyCode::ShiftLeft);
    day.h.action(UiAction::Accept);
    day.h.release(KeyCode::ShiftLeft);
    day.h.settle();
    assert_eq!(day.shows(pack1), None);
    assert_eq!(day.shows(store0), Some((feed, 2)), "back in the store");

    // Down from the pack are the bags: the slot, and the cells it opens.
    for _ in 0..day.registries.items.carry().pack.rows {
        day.h.action(UiAction::Down);
    }
    day.h.settle();
    let below = day.h.focused().expect("the focus is somewhere");
    let region = day
        .h
        .world()
        .get::<Tags>(below)
        .and_then(|tags| tags.get("region").map(str::to_owned));
    assert!(
        matches!(region.as_deref(), Some("bag0" | "bag_slot")),
        "below the pack is a bag, not {region:?}"
    );

    day.close();
    assert_eq!(day.truth(), before);
}

/// Shift and a click send a stack across, onto a stack of its kind first;
/// the send button does the same for the pinned stack and says which way.
#[test]
fn shift_click_and_the_send_button_send_a_stack_across() {
    let mut day = first_day("stores-send", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    let (feed, cell) = (day.id(FEED), day.id(CELL));
    let (store0, store2) = (day.cell("store", 0), day.cell("store", 2));
    let pack0 = day.cell("pack", 0);
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    assert_eq!(day.shows(store2), Some((cell, 3)));
    assert_eq!(day.shows(bag_cell), Some((cell, 1)), "in the satchel");

    // The cells go to the one already carried, in the bag, not to the
    // pack's first empty cell.
    day.h.shift_click(store2);
    day.h.settle();
    assert_eq!(day.shows(store2), None);
    assert_eq!(day.shows(bag_cell), Some((cell, 4)));
    assert_eq!(
        day.pack().bags[0].get(0).map(|stack| stack.count),
        Some(4),
        "on the truth"
    );
    // And back: from a bag to the store.
    day.h.shift_click(bag_cell);
    day.h.settle();
    assert_eq!(day.shows(bag_cell), None);
    assert_eq!(day.shows(store2), Some((cell, 4)));

    // The button, for the stack pointed at.
    let send = find(&day.h, stores::SEND);
    let shown = |day: &Day| day.h.is_visible(send);
    let label = |day: &Day| day.h.world().get::<SemanticLabel>(send).unwrap().0.clone();
    day.h.hover(store0);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().pinned(),
        Some(store0)
    );
    assert!(shown(&day));
    assert_eq!(label(&day), "Send to carried");
    day.h.click(send);
    day.h.settle();
    assert_eq!(day.shows(store0), None);
    assert_eq!(day.shows(pack0), Some((feed, 4)));
    assert!(!shown(&day), "the pinned slot is empty: nothing to send");
    day.h.hover(pack0);
    day.h.settle();
    assert_eq!(label(&day), "Send to the store");
    day.h.click(send);
    day.h.settle();
    assert_eq!(day.shows(pack0), None);
    assert_eq!(day.shows(store0), Some((feed, 4)));

    day.close();
    assert_eq!(day.truth(), before);
}

/// A bag that holds something will not come out of its slot, and says why
/// at the bag: in its caption beside a lock while it stands, and in the
/// warning colour when it is clicked. Emptied it comes out; a bag put in
/// a slot opens its cells; a bag slot takes nothing but a bag; and a
/// weapon in hand is not carried off, by any click.
#[test]
#[allow(clippy::too_many_lines)]
fn a_full_bag_says_why_at_the_bag() {
    let mut day = first_day("stores-bag", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    let (satchel, haul, cell) = (day.id(SATCHEL), day.id(HAUL), day.id(CELL));
    let slot0 = find(&day.h, &stores::bag_slot_id(0));
    let slot1 = find(&day.h, &stores::bag_slot_id(1));
    let caption0 = stores::bag_caption_id(0);
    let caption1 = stores::bag_caption_id(1);
    let role = |day: &Day, id: &str| {
        day.h
            .world()
            .get::<slotted::theme::Themed>(find(&day.h, id))
            .unwrap()
            .0
            .as_str()
            .to_owned()
    };
    let displayed = |day: &Day, bag: usize| {
        day.h
            .find_all(&by::role(SemanticRole::Slot).tag("region", &stores::bag_region(bag)))
            .into_iter()
            .filter(|cell| day.h.world().get::<Node>(*cell).unwrap().display != Display::None)
            .count()
    };
    assert_eq!(day.shows(slot0), Some((satchel, 1)));
    assert_eq!(
        day.says(&caption0),
        "Field satchel · holds 1 · empty it to take it out"
    );
    assert_eq!(day.says(&caption1), "Empty bag slot");
    assert_eq!(displayed(&day, 0), 6, "the satchel's six cells");
    assert_eq!(displayed(&day, 1), 0, "no bag, no cells");

    // The click that would take it out: refused, and said at the bag.
    day.h.click(slot0);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(day.shows(slot0), Some((satchel, 1)));
    assert_eq!(day.says(&caption0), "Holds 1: empty the bag to take it out");
    assert_eq!(role(&day, &caption0), "strata.stores.refusal");
    day.h.advance(std::time::Duration::from_secs(3));
    day.h.settle();
    assert_eq!(
        day.says(&caption0),
        "Field satchel · holds 1 · empty it to take it out",
        "the standing caption comes back"
    );
    assert_eq!(role(&day, &caption0), "strata.stores.caption");

    // Emptied, it opens and comes out.
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    let (pack0, pack1) = (day.cell("pack", 0), day.cell("pack", 1));
    day.h.click(bag_cell);
    day.h.settle();
    day.h.click(pack0);
    day.h.settle();
    assert_eq!(day.shows(pack0), Some((cell, 1)));
    assert_eq!(day.says(&caption0), "Field satchel · 6 slots");
    day.h.click(slot0);
    day.h.settle();
    assert_eq!(day.carried(), Some((satchel, 1)));
    assert_eq!(day.says(&caption0), "Empty bag slot");
    assert_eq!(displayed(&day, 0), 0, "its cells went with it");
    day.h.click(pack1);
    day.h.settle();
    assert_eq!(day.shows(pack1), Some((satchel, 1)));

    // The haul pack from the store into the second slot: twelve cells.
    let store5 = day.cell("store", 5);
    assert_eq!(day.shows(store5), Some((haul, 1)));
    day.h.click(store5);
    day.h.settle();
    day.h.click(slot1);
    day.h.settle();
    assert_eq!(day.shows(slot1), Some((haul, 1)));
    assert_eq!(day.says(&caption1), "Haul pack · 12 slots");
    assert_eq!(displayed(&day, 1), 12);
    assert_eq!(
        day.pack().open_cells(1, &day.registries.items),
        12,
        "on the truth"
    );

    // A magazine is no bag.
    day.h.click(pack0);
    day.h.settle();
    assert_eq!(day.carried(), Some((cell, 1)));
    day.h.click(slot0);
    day.h.settle();
    assert_eq!(day.shows(slot0), None);
    assert_eq!(day.carried(), Some((cell, 1)), "still carried");
    assert_eq!(day.says(&caption0), "A bag slot takes a bag");
    // Into the haul pack's cells it goes, and now that bag is the one
    // that stays.
    let haul_cell = day.cell(&stores::bag_region(1), 11);
    day.h.click(haul_cell);
    day.h.settle();
    assert_eq!(day.shows(haul_cell), Some((cell, 1)));
    assert_eq!(
        day.says(&caption1),
        "Haul pack · holds 1 · empty it to take it out"
    );

    // A weapon in hand does not move, and a try at half of it says whose
    // business that is. (A plain click opens its fitting: see
    // `a_hands_cell_opens_fitting`.)
    let hand = find(&day.h, &stores::hand_slot_id(0));
    let fitted = stores::hand_fitted_id(0);
    assert_eq!(day.says(&fitted), "Nothing fitted");
    day.h.right_click(hand);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    assert_eq!(
        day.says(&fitted),
        "A hand takes a weapon. Carry one here, or press T on it."
    );

    day.close();
    assert_eq!(day.truth(), before);
}

/// Key I opens the pack anywhere, without the household's store: the title
/// says Carried, no store cell exists, the focus starts in the pack, and
/// Shift and a click send between a bag and the pack. I closes it again,
/// and does nothing under another screen.
#[test]
fn key_i_opens_the_pack_without_the_store() {
    let mut day = first_day("stores-key-i", 1600.0, 900.0);
    let before_store = day.store();
    day.h.key(stores::KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![stores::kind()]);
    assert_eq!(
        day.h.world().resource::<StoresState>().with_store(),
        Some(false)
    );
    assert_eq!(day.says(stores::TITLE_ID), "Carried");
    assert!(
        day.h
            .find_all(&by::role(SemanticRole::Slot).tag("region", "store"))
            .is_empty(),
        "the store is a locker's"
    );
    let pack0 = day.cell("pack", 0);
    assert_eq!(day.h.focused(), Some(pack0));
    assert_eq!(day.pinned(), None, "the focused cell holds nothing");
    assert_eq!(day.card(), CardParams::empty().framed());
    let cell = day.id(CELL);
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    // The pointer comes first: a module pinned shows Take in hand, grey, and
    // the taller column moves the cells a little in a window this short.
    day.h.hover(bag_cell);
    day.h.settle();
    day.h.shift_click(bag_cell);
    day.h.settle();
    assert_eq!(day.shows(bag_cell), None);
    assert_eq!(day.shows(pack0), Some((cell, 1)));
    assert_eq!(
        day.pack().cells.get(0).map(|stack| stack.id),
        Some(cell),
        "on the truth"
    );
    // The send button belongs to the store's side.
    day.h.hover(pack0);
    day.h.settle();
    let send = find(&day.h, stores::SEND);
    assert!(!day.h.is_visible(send), "blank, and still laid out");
    assert!(
        !day.h
            .world()
            .get::<Node>(send)
            .unwrap()
            .display
            .eq(&Display::None),
        "its room is kept"
    );

    day.h.key(stores::KEY);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(day.h.stack().is_empty(), "I closes what I opened");
    assert_eq!(day.store(), before_store, "the store was never touched");

    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    day.h.key(stores::KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![kinds::pause()], "not over the pause");
}

/// A stack that found no room when the screen last closed is still on the
/// sim's cursor, and is carried again when the screen next opens: the
/// mirror and the truth start from the same cursor, so the click that
/// puts it down is acknowledged and not overwritten.
#[test]
fn a_stack_left_on_the_cursor_is_carried_again() {
    let mut day = first_day("stores-cursor", 1600.0, 900.0);
    let before = day.truth();
    let feed = day.id(FEED);
    // Take the bought magazines out of the store and leave them on the
    // cursor, as a close on a full pack would.
    let stack = {
        let world = day.h.world_mut();
        let stack = world.resource_mut::<HouseholdStore>().0.take(0).unwrap();
        world.get_mut::<Pack>(day.player).unwrap().cursor = Some(stack.clone());
        stack
    };
    assert_eq!(day.truth(), before);
    day.h.key(stores::KEY);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, stack.count)));
    assert_eq!(
        day.says(stores::STATUS_ID),
        "Carrying: Patrol extended magazine x4"
    );
    let pack3 = day.cell("pack", 3);
    day.h.click(pack3);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(day.shows(pack3), Some((feed, 4)));
    let pack = day.pack();
    assert_eq!(pack.cursor, None);
    assert_eq!(
        pack.cells.get(3),
        Some(&stack),
        "the very stack, patch and all"
    );
    day.close();
    assert_eq!(day.truth(), before);
}

/// Away from a locker the household's store is shown and moves nothing:
/// the sim refuses the open and every click, the footer says where the
/// store is reached, and the click the screen predicted is taken back.
#[test]
fn the_store_shows_and_moves_nothing_away_from_a_locker() {
    let mut day = first_day("stores-no-locker", 1600.0, 900.0);
    let before = day.truth();
    let feed = day.id(FEED);
    // The locker goes, and the tick's reach with it; the screen is asked
    // for all the same, as a stale F would ask.
    let locker = day.locker;
    day.h.world_mut().despawn(locker);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    day.h.world_mut().write_message(Interact {
        entity: locker,
        kind: InteractionKind::Locker,
    });
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    assert_eq!(
        day.says(stores::STATUS_ID),
        "The household's store is reached at a locker"
    );
    let (store0, pack0) = (day.cell("store", 0), day.cell("pack", 0));
    assert_eq!(day.shows(store0), Some((feed, 4)), "shown");
    day.h.click(store0);
    day.h.settle();
    assert_eq!(day.carried(), None, "the prediction was taken back");
    assert_eq!(day.shows(store0), Some((feed, 4)));
    assert_eq!(day.shows(pack0), None);
    assert_eq!(
        day.store().0.get(0).map(|stack| stack.count),
        Some(4),
        "and the truth never moved"
    );
    assert_eq!(
        day.says(stores::STATUS_ID),
        "The household's store is reached at a locker"
    );
    day.close();
    assert_eq!(day.truth(), before);
}

/// The pause menu's Inventory button opens the pack without the store, as
/// a page over the pause; a locker's chip says what F does there, and F
/// opens the stores with the household's store beside the pack.
#[test]
fn the_pause_and_the_locker_reach_the_screen() {
    let mut day = first_day("stores-reach", 1600.0, 900.0);
    assert_eq!(
        day.h.text_of(find(&day.h, ui::prompt::TEXT_ID)).unwrap(),
        "Open the locker"
    );
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    let inventory = day
        .h
        .find(&by::role(SemanticRole::Button).tag("menu", "inventory"));
    day.h.click(inventory);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![kinds::pause(), stores::kind()]);
    assert_eq!(
        day.h.world().resource::<StoresState>().with_store(),
        Some(false)
    );
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![kinds::pause()]);
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(day.h.stack().is_empty());

    day.use_locker();
    assert_eq!(
        day.h.world().resource::<StoresState>().with_store(),
        Some(true)
    );
    assert_eq!(
        day.h
            .find_all(&by::role(SemanticRole::Slot).tag("region", "store"))
            .len(),
        usize::from(day.registries.items.carry().store.cells())
    );
    assert_eq!(day.h.focused(), Some(day.cell("store", 0)));
    // Close by its button.
    let close = find(&day.h, stores::CLOSE_ID);
    day.h.click(close);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(day.h.stack().is_empty());
    assert!(day.h.world().resource::<OpenStock>().0.is_none());
}
/// Both forms fit a 1280 by 720 window: the panel inside the window, and
/// inside the panel the Close button, the card with a weapon on it, the
/// last cell of every grid and the hands.
#[test]
fn the_stores_screen_fits_small_windows() {
    let mut day = first_day("stores-small", 1280.0, 720.0);
    let window = Rect::new(0., 0., 1280., 720.);
    // The haul pack in the second slot, so both bags' cells are open:
    // the tallest the middle column gets.
    let haul = day.id(HAUL);
    let registries = day.registries.clone();
    let record = strata_registry::item::Record {
        item: registries.items.def_of(haul).unwrap().id.clone(),
        count: 1,
        batch: 0,
        by: None,
        lines: std::collections::BTreeMap::new(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: None,
    };
    let stack = strata_sim::items::write(&record, &registries.items).unwrap();
    day.h
        .world_mut()
        .get_mut::<Pack>(day.player)
        .unwrap()
        .bag_slots
        .set(1, Some(stack));
    for with_store in [true, false] {
        if with_store {
            day.use_locker();
        } else {
            day.h.key(stores::KEY);
            day.h.settle();
        }
        // The tallest card: a weapon's.
        let hand = find(&day.h, &stores::hand_slot_id(0));
        day.h.set_focus(Some(hand));
        day.h.step(3);
        day.h.settle();
        let root = day.h.rect_of(find(&day.h, stores::ROOT_ID));
        assert!(
            within(root, window),
            "the stores {root:?} spill the window (store: {with_store})"
        );
        let mut inside = vec![
            ("Close", find(&day.h, stores::CLOSE_ID)),
            ("the card", find(&day.h, stores::CARD_ID)),
            ("the footer's line", find(&day.h, stores::STATUS_ID)),
            ("the second hand", find(&day.h, &stores::hand_slot_id(1))),
            ("the pack's last cell", day.cell("pack", 17)),
            (
                "the first bag's last cell",
                day.cell(&stores::bag_region(0), 5),
            ),
            (
                "the second bag's last cell",
                day.cell(&stores::bag_region(1), 11),
            ),
        ];
        if with_store {
            inside.push(("the store's last cell", day.cell("store", 53)));
        }
        for (what, entity) in inside {
            let rect = day.h.rect_of(entity);
            assert!(rect.width() > 0., "{what} is not laid out");
            assert!(
                within(rect, root),
                "{what} {rect:?} outside the stores {root:?} (store: {with_store})"
            );
        }
        day.close();
        assert!(day.h.stack().is_empty());
    }
}

/// Every control the screen has is reached and worked by the keys alone:
/// a filter chip, the send button for a stack in the store, a bag slot and
/// the second one under it, a hand, and Close. The send button acts on the
/// pinned stack, so the walk to it goes by empty cells, which leave the
/// pin where it is.
#[test]
#[allow(clippy::too_many_lines)]
fn every_control_is_reached_by_the_keys_alone() {
    use UiAction::{Accept, Down, Left, Right, Up};
    let mut day = first_day("stores-controls", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    day.h.set_input_mode(InputMode::Keyboard);
    let feed = day.id(FEED);
    let store0 = day.cell("store", 0);
    let pack0 = day.cell("pack", 0);
    assert_eq!(day.h.focused(), Some(store0));
    assert_eq!(day.pinned(), Some(store0));

    // Up to the chips, along to Mods, and Accept sets the filter.
    let mods = find(&day.h, &stores::Filter::Mods.chip_id());
    day.h.action(Up);
    day.h.settle();
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &stores::Filter::All.chip_id()))
    );
    day.walk_to(Right, mods, 3);
    day.h.action(Accept);
    day.h.step(3);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().filter,
        stores::Filter::Mods
    );
    let dimmed = |day: &Day, slot: Entity| day.h.world().get::<SlotDressing>(slot).unwrap().dimmed;
    assert!(
        dimmed(&day, day.cell("store", 5)),
        "the haul pack is no mod"
    );
    assert!(!dimmed(&day, store0));
    day.walk_to(Left, find(&day.h, &stores::Filter::All.chip_id()), 3);
    day.h.action(Accept);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().filter,
        stores::Filter::All
    );
    day.walk_to(Down, store0, 2);
    assert_eq!(day.pinned(), Some(store0), "a chip takes no pin");

    // To the send button by the store's second row and the pack's, which
    // hold nothing: the bought magazines stay pinned all the way.
    let send = find(&day.h, stores::SEND);
    day.h.action(Down);
    day.h.settle();
    let cols = usize::from(
        day.registries.items.carry().store.cols + day.registries.items.carry().pack.cols,
    );
    day.walk_to(Right, send, cols + 1);
    assert_eq!(day.pinned(), Some(store0));
    assert_eq!(
        day.h.world().get::<SemanticLabel>(send).unwrap().0,
        "Send to carried"
    );
    day.h.action(Accept);
    day.h.settle();
    assert_eq!(day.shows(store0), None);
    assert_eq!(day.shows(pack0), Some((feed, 4)));
    assert_eq!(
        day.pack().cells.get(0).map(|stack| (stack.id, stack.count)),
        Some((feed, 4)),
        "on the truth"
    );
    assert!(day.store().0.get(0).is_none());
    assert!(!day.laid_out(send), "nothing pinned to send");
    assert_eq!(
        day.h.focused(),
        Some(store0),
        "the focus is not left on a button that went"
    );

    // Down the pack's side to the bags: the slot whose bag holds something
    // says why it stays, and under its cells is the second slot.
    let slot0 = find(&day.h, &stores::bag_slot_id(0));
    let slot1 = find(&day.h, &stores::bag_slot_id(1));
    day.h.set_focus(Some(pack0));
    day.h.settle();
    assert_eq!(day.pinned(), Some(pack0), "the stack that was sent");
    let rows = usize::from(day.registries.items.carry().pack.rows);
    day.walk_to(Down, slot0, rows + 1);
    day.h.action(Accept);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(
        day.says(&stores::bag_caption_id(0)),
        "Holds 1: empty the bag to take it out"
    );
    day.walk_to(Down, slot1, 3);
    assert_eq!(day.pinned(), Some(day.cell(&stores::bag_region(0), 0)));

    // Across to the store and down to a hand, where Accept opens the
    // weapon's fitting; back from it, and on to Close.
    let hand1 = find(&day.h, &stores::hand_slot_id(1));
    let close = find(&day.h, stores::CLOSE_ID);
    // The hands stand under the store's first column.
    let carry = day.registries.items.carry();
    let (cols, rows) = (usize::from(carry.store.cols), usize::from(carry.store.rows));
    let corner = day.cell("store", cols * (rows - 1));
    day.walk_to(Left, corner, cols + 1);
    day.walk_to(Down, hand1, 2);
    day.h.action(Accept);
    day.h.settle();
    assert_eq!(day.carried(), None);
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(day.says(fitting::TITLE_ID), "Kestrel service pistol");
    day.h.action(UiAction::Back);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    day.h.set_focus(Some(hand1));
    day.h.settle();
    day.walk_to(Down, close, 2);
    day.h.action(Accept);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(day.h.stack().is_empty(), "Accept on Close closes");
    assert!(day.h.world().resource::<OpenStock>().0.is_none());
    assert_eq!(day.truth(), before);
}

/// Tab walks only what is laid out, in both forms, and gets from the pack
/// to Close: the cells a bag does not open and the send button while it is
/// away are no stops. A bag put in a slot gives its cells their stops back.
#[test]
fn tab_stops_only_where_something_is_laid_out() {
    use bevy::input_focus::tab_navigation::TabIndex;
    for with_store in [true, false] {
        let mut day = first_day("stores-tab", 1600.0, 900.0);
        if with_store {
            day.use_locker();
        } else {
            day.h.key(stores::KEY);
            day.h.settle();
        }
        let close = find(&day.h, stores::CLOSE_ID);
        let send = find(&day.h, stores::SEND);
        let last = usize::from(day.registries.items.carry().pack.cells()) - 1;
        day.h.set_focus(Some(day.cell("pack", last)));
        day.h.settle();
        assert_eq!(
            day.laid_out(send),
            with_store,
            "the send button is the store's"
        );
        let mut stops = Vec::new();
        // Every stop the screen has, and a few over.
        for _ in 0..150 {
            day.h.focus_next();
            day.h.settle();
            let at = day.h.focused().expect("Tab leaves the focus somewhere");
            assert!(
                day.laid_out(at),
                "stop {} is on nothing a player can see (store: {with_store}): {:?}",
                stops.len(),
                day.h.world().get::<slotted::ui::TestId>(at)
            );
            stops.push(at);
            if at == close {
                break;
            }
        }
        assert_eq!(
            stops.last(),
            Some(&close),
            "Tab reaches Close (store: {with_store})"
        );
        assert_eq!(stops.contains(&send), with_store);
        let stop_of = |day: &Day, entity: Entity| day.h.world().get::<TabIndex>(entity).unwrap().0;
        assert_eq!(stop_of(&day, send), if with_store { 0 } else { -1 });
        // The satchel opens six cells of twelve; the other slot opens none.
        let cells = |day: &Day, bag: usize| {
            day.h
                .find_all(&by::role(SemanticRole::Slot).tag("region", &stores::bag_region(bag)))
        };
        let open_stops = |day: &Day, bag: usize| {
            cells(day, bag)
                .into_iter()
                .map(|cell| stop_of(day, cell))
                .filter(|stop| *stop >= 0)
                .collect::<Vec<i32>>()
        };
        assert_eq!(open_stops(&day, 0), (0..6).collect::<Vec<i32>>());
        assert_eq!(open_stops(&day, 1), Vec::<i32>::new());
        for cell in cells(&day, 1) {
            assert!(!stops.contains(&cell) && !day.laid_out(cell));
        }
        if with_store {
            // The haul pack into the second slot: twelve cells, twelve stops.
            let slot1 = find(&day.h, &stores::bag_slot_id(1));
            day.h.click(day.cell("store", 5));
            day.h.settle();
            day.h.click(slot1);
            day.h.settle();
            assert_eq!(open_stops(&day, 1), (0..12).collect::<Vec<i32>>());
            assert!(cells(&day, 1).into_iter().all(|cell| day.laid_out(cell)));
        }
        day.close();
    }
}

/// A stack sent across that finds no room says so: by the lit send button
/// and by Shift and a click alike, in the footer's line and the warning
/// colour, naming the side that is full. Nothing moves, and the line goes
/// back to its hint.
#[test]
fn a_send_with_no_room_says_why() {
    let mut day = first_day("stores-no-room", 1600.0, 900.0);
    day.fill_the_pack();
    let before = day.truth();
    day.use_locker();
    let feed = day.id(FEED);
    let store0 = day.cell("store", 0);
    let hint = day.says(stores::STATUS_ID);
    assert!(hint.starts_with("T or Shift click sends"), "{hint}");

    let send = find(&day.h, stores::SEND);
    day.h.hover(store0);
    day.h.settle();
    assert_eq!(day.pinned(), Some(store0));
    assert!(day.laid_out(send), "the button is lit: there is a stack");
    day.h.click(send);
    day.h.settle();
    assert_eq!(day.shows(store0), Some((feed, 4)), "nothing moved");
    assert_eq!(day.says(stores::STATUS_ID), "No room in the pack");
    assert_eq!(day.role_of(stores::STATUS_ID), "strata.stores.refusal");
    day.h.advance(std::time::Duration::from_secs(3));
    day.h.settle();
    assert_eq!(day.says(stores::STATUS_ID), hint, "the hint comes back");
    assert_eq!(day.role_of(stores::STATUS_ID), "strata.stores.status");

    day.h.shift_click(store0);
    day.h.settle();
    assert_eq!(day.shows(store0), Some((feed, 4)));
    assert_eq!(day.says(stores::STATUS_ID), "No room in the pack");
    // By the keys the same: Shift and Accept on the focused stack.
    day.h.advance(std::time::Duration::from_secs(3));
    day.h.set_focus(Some(store0));
    day.h.hold(KeyCode::ShiftLeft);
    day.h.action(UiAction::Accept);
    day.h.release(KeyCode::ShiftLeft);
    day.h.settle();
    assert_eq!(day.says(stores::STATUS_ID), "No room in the pack");
    // A send from an empty cell is no refusal: there was nothing to send.
    day.h.advance(std::time::Duration::from_secs(3));
    day.h.shift_click(day.cell("store", 30));
    day.h.settle();
    assert_eq!(day.says(stores::STATUS_ID), hint);
    assert_eq!(
        day.h
            .world()
            .resource::<stores::StockAuthority>()
            .in_flight(),
        0,
        "a click the screen's own rules refuse is never sent"
    );
    day.close();
    assert_eq!(day.truth(), before);

    // Without the store a full pack sends to its bags, which are full too.
    day.h.key(stores::KEY);
    day.h.settle();
    let pack0 = day.cell("pack", 0);
    let held = day.shows(pack0);
    day.h.shift_click(pack0);
    day.h.settle();
    assert_eq!(day.shows(pack0), held);
    assert_eq!(day.says(stores::STATUS_ID), "No room in a bag");
    day.h.advance(std::time::Duration::from_secs(3));
    day.h.shift_click(day.cell(&stores::bag_region(0), 0));
    day.h.settle();
    assert_eq!(day.says(stores::STATUS_ID), "No room in the pack");
    day.close();
    assert_eq!(day.truth(), before);
}

/// The pointer pins a card only once it has moved since the screen opened
/// and only while it is the device in use: a screen that comes up over a
/// parked pointer shows the focused stack's card, not that of the cell
/// under the pointer. An empty cell crossed keeps the pin, so the way from
/// a stack to the send button does not lose the stack.
#[test]
fn a_parked_pointer_pins_nothing_and_an_empty_cell_keeps_the_pin() {
    let mut day = first_day("stores-parked", 1600.0, 900.0);
    // Where the satchel's first cell, which holds a stack, comes up.
    day.use_locker();
    let at = day.h.center_of(day.cell(&stores::bag_region(0), 0));
    day.close();
    assert!(day.h.stack().is_empty());
    day.h.pointer_move_to(at);
    day.h.settle();

    // Opened with the pointer the device in use and resting there.
    day.use_locker();
    assert_eq!(day.h.input_mode(), InputMode::Pointer);
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    let store0 = day.cell("store", 0);
    assert!(
        day.h
            .world()
            .get::<bevy::picking::hover::Hovered>(bag_cell)
            .unwrap()
            .get(),
        "the cell came up under the pointer"
    );
    assert_eq!(day.h.focused(), Some(store0));
    assert_eq!(
        day.pinned(),
        Some(store0),
        "a parked pointer points at nothing"
    );
    assert_eq!(day.card().name, "Patrol extended magazine");

    // A pointer jogged while the keys are in use takes nothing from them.
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.pointer_move_to(at + Vec2::new(1.0, 0.0));
    day.h.settle();
    assert_eq!(day.h.input_mode(), InputMode::Keyboard);
    assert_eq!(day.pinned(), Some(store0));

    // Moved in earnest, it pins the stack it is over.
    day.h.pointer_move_to(at + Vec2::new(6.0, 0.0));
    day.h.settle();
    assert_eq!(day.h.input_mode(), InputMode::Pointer);
    assert_eq!(day.pinned(), Some(bag_cell));
    assert_eq!(day.card().name, "Pistol extended cell");
    // Across empty cells the pin stays with the stack, and the button
    // still sends it.
    for cell in [12, 6, 5] {
        day.h.hover(day.cell("pack", cell));
        day.h.settle();
        assert_eq!(day.pinned(), Some(bag_cell));
    }
    assert_eq!(day.card().name, "Pistol extended cell");
    let send = find(&day.h, stores::SEND);
    assert_eq!(
        day.h.world().get::<SemanticLabel>(send).unwrap().0,
        "Send to the store"
    );
    // Onto another stack, it moves.
    let slot0 = find(&day.h, &stores::bag_slot_id(0));
    day.h.hover(slot0);
    day.h.settle();
    assert_eq!(day.pinned(), Some(slot0));
    assert_eq!(day.card().name, "Field satchel");
    day.close();
}

/// A screen asked for while the sim has yet to hear that the last one
/// closed waits for the tick: the close puts the cursor's stack away on the
/// truth, and a mirror copied before that would show it as still carried.
#[test]
fn an_open_waits_for_the_last_close_to_reach_the_sim() {
    use strata_sim::items::{StockInbox, StockRequest};
    let mut day = first_day("stores-reopen", 1600.0, 900.0);
    let before = day.truth();
    let feed = day.id(FEED);
    day.use_locker();
    let store0 = day.cell("store", 0);
    day.h.click(store0);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 4)));

    // Closed and asked for again with no frame between, as a key pressed
    // twice inside one tick would.
    {
        let world = day.h.world_mut();
        let mut commands = world.commands();
        slotted::ui::pop_screen(&mut commands);
        world.flush();
    }
    for _ in 0..8 {
        if day.h.world().resource::<StoresState>().menu().is_none() {
            break;
        }
        day.h.step(1);
    }
    assert_eq!(day.h.world().resource::<StoresState>().menu(), None);
    // The frames the screen took to go may have held a tick. Whether or
    // not, a close the sim has not heard of is in its inbox now.
    let world = day.h.world_mut();
    let waiting = |world: &World| {
        world
            .resource::<StockInbox>()
            .0
            .iter()
            .any(|request| matches!(request, StockRequest::Close { .. }))
    };
    if !waiting(world) {
        world
            .resource_mut::<StockInbox>()
            .0
            .push(StockRequest::Close {
                menu: slotted::model::MenuId(u32::MAX),
            });
    }
    let mut commands = world.commands();
    stores::open(&mut commands, true);
    world.flush();
    assert_eq!(
        world.resource::<StoresState>().menu(),
        None,
        "not over a close the sim has yet to consume"
    );
    assert!(waiting(world));

    // The tick takes the close, and the screen opens on what it left.
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    assert_eq!(day.carried(), None, "the mirror starts from the truth");
    assert_eq!(day.pack().cursor, None);
    let pack0 = day.cell("pack", 0);
    assert_eq!(day.shows(pack0), Some((feed, 4)), "stowed in the pack");
    // And the two agree: the next click is acknowledged as predicted.
    day.h.click(pack0);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 4)));
    assert_eq!(day.pack().cursor.map(|stack| stack.count), Some(4));
    day.close();
    assert_eq!(day.truth(), before);
}

/// A carried stack dragged across cells is spread over them, on the truth
/// as on the mirror: every step of the drag is a click the sim replays.
#[test]
fn a_drag_spreads_a_carried_stack_on_the_truth() {
    let mut day = first_day("stores-drag", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    let feed = day.id(FEED);
    let store0 = day.cell("store", 0);
    let (pack0, pack1) = (day.cell("pack", 0), day.cell("pack", 1));
    day.h.click(store0);
    day.h.settle();
    assert_eq!(day.carried(), Some((feed, 4)));
    day.h.drag_paint(&[pack0, pack1]);
    day.h.settle();
    assert_eq!(day.shows(pack0), Some((feed, 2)));
    assert_eq!(day.shows(pack1), Some((feed, 2)));
    assert_eq!(day.carried(), None);
    let pack = day.pack();
    assert_eq!(
        [0, 1].map(|cell| pack.cells.get(cell).map(|stack| stack.count)),
        [Some(2), Some(2)],
        "the truth was spread the same"
    );
    assert_eq!(pack.cursor, None);
    assert_eq!(
        day.h
            .world()
            .resource::<stores::StockAuthority>()
            .in_flight(),
        0
    );
    day.close();
    assert_eq!(day.truth(), before);
}

/// The cells a bag opens continue the pack's columns: the first of them
/// stands under the pack's first cell to the unit, and the bag's own slot,
/// at their left, is a whole cell and not squeezed by its caption.
#[test]
fn a_bags_cells_line_up_with_the_packs() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0)] {
        for with_store in [true, false] {
            let mut day = first_day("stores-line-up", width, height);
            if with_store {
                day.use_locker();
            } else {
                day.h.key(stores::KEY);
                day.h.settle();
            }
            let pack0 = day.h.rect_of(day.cell("pack", 0));
            let pack_last = day.h.rect_of(day.cell(
                "pack",
                usize::from(day.registries.items.carry().pack.cols) - 1,
            ));
            let slot0 = day.h.rect_of(find(&day.h, &stores::bag_slot_id(0)));
            let slot1 = day.h.rect_of(find(&day.h, &stores::bag_slot_id(1)));
            let bag0 = day.h.rect_of(day.cell(&stores::bag_region(0), 0));
            let bag_last = day.h.rect_of(day.cell(&stores::bag_region(0), 5));
            assert_eq!(slot0.size(), pack0.size(), "the bag's slot is a whole cell");
            assert_eq!(slot1.size(), pack0.size());
            assert_eq!(bag0.size(), pack0.size());
            assert_eq!(bag0.min.x, pack0.min.x, "the first column");
            assert_eq!(bag_last.min.x, pack_last.min.x, "the last column");
            assert!(
                slot0.max.x < bag0.min.x,
                "the slot stands left of its cells"
            );
            // The satchel opens one row: the second slot is under it, and
            // not a row's worth of nothing further down.
            assert!(
                slot1.min.y - bag0.max.y < pack0.height(),
                "{slot1:?} far under {bag0:?}"
            );
        }
    }
}

impl Day {
    /// A carbine made by the household's smith in the pack's first cell:
    /// batch 7, every line 650, whole.
    fn carry_made_carbine(&mut self) -> strata_registry::item::Record {
        let mut record = self.weapon(0);
        record.batch = 7;
        record.by = Some("Tovan Reyes".to_owned());
        record.condition = Some(1000);
        for figure in record.lines.values_mut() {
            *figure = 650;
        }
        self.carry(&record);
        record
    }

    /// The pack opened by key I, with the keyboard in use.
    fn open_pack_by_key(&mut self) {
        self.h.key(stores::KEY);
        self.h.settle();
        self.h.set_input_mode(InputMode::Keyboard);
        self.h.settle();
    }
}

/// Take in hand, by the keys alone: the carbine pinned in the pack, focus
/// reaches the button, Accept swaps it, and the hand holds the made one
/// while the bought one lies in its cell.
#[test]
fn take_in_hand_by_keys_swaps_the_made_carbine() {
    let mut day = first_day("stores-take-keys", 1600.0, 900.0);
    let made = day.carry_made_carbine();
    let bought = day.weapon(0);
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.set_focus(Some(pack0));
    day.h.settle();
    assert_eq!(day.pinned(), Some(pack0));
    let take = find(&day.h, stores::TAKE);
    assert_eq!(
        day.h.world().get::<SemanticLabel>(take).unwrap().0,
        "Swap out Vigil patrol carbine"
    );
    assert!(!button_disabled(&day.h, stores::TAKE));
    let cols = usize::from(day.registries.items.carry().pack.cols);
    day.walk_to(UiAction::Right, take, cols + 6);
    day.h.action(UiAction::Accept);
    day.tick();
    day.h.step(6);
    day.h.settle();
    assert_eq!(day.weapon(0).batch, made.batch, "the made one is in hand");
    assert_eq!(day.weapon(0).by, made.by);
    let items = &day.registries.items;
    let lying = strata_sim::items::read(day.pack().cells.get(0).unwrap(), items).unwrap();
    assert_eq!(lying.batch, bought.batch, "the bought one lies in its cell");
    // The screen follows the truth: its IN HAND row holds the made one.
    let hand = find(&day.h, &stores::hand_slot_id(0));
    let shown = day.h.stack_at(hand).expect("a weapon in the hand row");
    assert_eq!(strata_sim::items::read(&shown, items).unwrap().batch, 7);
}

/// The panel is one size and the way across stands where it stood whatever
/// is pinned: a lot, which has a short card, then a weapon, which has the
/// tallest, then nothing at all, on a window of each of two sizes.
#[test]
fn the_stores_screen_does_not_move_with_the_pinned_card() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0)] {
        for with_store in [true, false] {
            let mut day = first_day("stores-fixed", width, height);
            day.carry_made_carbine();
            // Something short to pin that is no weapon, in the pack where
            // the pack alone has nothing else.
            let plain = day
                .registries
                .items
                .starting()
                .unwrap()
                .store
                .iter()
                .find(|record| day.registries.items.family_of(record).is_none())
                .cloned()
                .expect("a plain stack in the first day's store");
            day.carry(&plain);
            if with_store {
                day.use_locker();
            } else {
                day.open_pack_by_key();
            }
            let parts = [
                stores::ROOT_ID,
                stores::CARD_REGION_ID,
                stores::SEND,
                stores::TAKE,
                stores::NOTE_ID,
            ];
            // The note is the foot's last line, so its words may take more
            // or less room below its top; where it begins is what holds.
            let rects = |day: &Day| {
                parts.map(|id| {
                    let rect = day.h.rect_of(find(&day.h, id));
                    if id == stores::NOTE_ID {
                        bevy::math::Rect::from_corners(rect.min, rect.min)
                    } else {
                        rect
                    }
                })
            };
            let weapon = day.cell("pack", 0);
            let lot = if with_store {
                day.cell("store", 0)
            } else {
                day.cell("pack", 1)
            };
            day.h.hover(lot);
            day.h.settle();
            assert_eq!(day.pinned(), Some(lot), "the short thing is pinned");
            let short = rects(&day);
            day.h.hover(weapon);
            day.h.settle();
            assert_eq!(day.pinned(), Some(weapon));
            // The card in its scrolling region keeps room for its labels: a
            // fact row whose name collapsed would show numbers alone.
            let world = day.h.world();
            let mut below = vec![find(&day.h, stores::CARD_ID)];
            let mut seen = 0;
            while let Some(at) = below.pop() {
                if let (Some(text), Some(node)) = (
                    world.get::<bevy::ui::widget::Text>(at),
                    world.get::<bevy::ui::ComputedNode>(at),
                ) && !text.0.is_empty()
                {
                    seen += 1;
                    assert!(node.size().x > 0., "text {:?} has no width", text.0);
                }
                if let Some(children) = world.get::<Children>(at) {
                    below.extend(children.iter());
                }
            }
            assert!(seen > 8, "the weapon card draws its rows: {seen}");
            let tall = rects(&day);
            assert_eq!(short, tall, "{width}x{height} store {with_store}");
            // Nothing pinned: a hover that leaves every cell.
            day.h.hover(find(&day.h, stores::CLOSE_ID));
            day.h.settle();
            assert_eq!(rects(&day), short);
        }
    }
}

/// T on a weapon in the pack, on the pack-only screen, takes it in hand.
#[test]
fn t_on_a_pack_weapon_takes_it_in_hand() {
    let mut day = first_day("stores-take-t", 1600.0, 900.0);
    let made = day.carry_made_carbine();
    let bought = day.weapon(0);
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.set_focus(Some(pack0));
    day.h.settle();
    day.h.key(stores::SEND_KEY);
    day.tick();
    day.h.step(6);
    day.h.settle();
    assert_eq!(day.weapon(0).batch, made.batch, "the made one is in hand");
    let items = &day.registries.items;
    let lying = strata_sim::items::read(day.pack().cells.get(0).unwrap(), items).unwrap();
    assert_eq!(lying.batch, bought.batch, "the old one lies in the cell");
}

/// A weapon carried onto a hand is taken in it, the old one kept.
#[test]
fn a_weapon_carried_onto_a_hand_is_taken() {
    let mut day = first_day("stores-take-carried", 1600.0, 900.0);
    let made = day.carry_made_carbine();
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.click(pack0);
    day.tick();
    assert!(day.carried().is_some(), "the made carbine is carried");
    let old = day.weapon(0);
    let hand = find(&day.h, &stores::hand_slot_id(0));
    day.h.hover(hand);
    day.h.settle();
    day.h.click(hand);
    day.tick();
    day.h.step(6);
    day.h.settle();
    assert_eq!(day.weapon(0).batch, made.batch, "the made one is in hand");
    // The weapon it replaced left the hand for the cursor, and the screen
    // is made again after a take, which puts what is carried away: it lies
    // in the pack and none is lost.
    let items = &day.registries.items;
    let pack = day.pack();
    let lying = (0..pack.cells.len())
        .filter_map(|cell| pack.cells.get(cell))
        .filter_map(|stack| strata_sim::items::read(stack, items).ok())
        .any(|record| record.batch == old.batch);
    assert!(lying || pack.cursor.is_some(), "the old weapon is kept");
    assert_ne!(day.weapon(0).batch, old.batch);
}

/// Anything but a weapon carried onto a hand is refused in the new words.
#[test]
fn a_lot_carried_onto_a_hand_is_refused_in_the_new_words() {
    let mut day = first_day("stores-take-lot", 1600.0, 900.0);
    let module = day
        .registries
        .items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| {
            matches!(
                day.registries.items.get(&record.item).map(|def| &def.kind),
                Some(strata_registry::item::ItemKind::Module { .. })
            )
        })
        .cloned()
        .expect("a module in the first day's store");
    day.carry(&module);
    day.open_pack_by_key();
    let at = day.cell("pack", 0);
    day.h.click(at);
    day.h.settle();
    assert!(day.carried().is_some());
    let hand = find(&day.h, &stores::hand_slot_id(0));
    day.h.hover(hand);
    day.h.settle();
    day.h.click(hand);
    day.h.settle();
    assert!(day.carried().is_some(), "still carried");
    assert_eq!(
        day.says(&stores::hand_fitted_id(0)),
        "A hand takes a weapon. Carry one here, or press T on it."
    );
}

/// A blade pinned in the pack offers the hand it would go to: the hand not in
/// use, since neither hand holds a blade, and the button is live.
#[test]
fn take_in_hand_offers_the_hand_a_blade_would_go_to() {
    let mut day = first_day("stores-take-blade", 1600.0, 900.0);
    let blade = day
        .registries
        .items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| record.item.path() == "weapon.service_blade")
        .cloned()
        .expect("the first day stocks service blades");
    day.carry(&blade);
    day.open_pack_by_key();
    let cell = day.cell("pack", 0);
    day.h.set_focus(Some(cell));
    day.h.settle();
    assert_eq!(day.pinned(), Some(cell));
    let take = find(&day.h, stores::TAKE);
    assert_eq!(
        day.h.world().get::<SemanticLabel>(take).unwrap().0,
        "Swap out Kestrel service pistol",
        "the pistol's hand is not in use, so the blade goes there"
    );
    assert!(!button_disabled(&day.h, stores::TAKE));
}

/// A module is no weapon: the button is there, grey, with the sim's reason
/// in words under it.
#[test]
fn take_in_hand_is_grey_with_its_reason_for_a_module() {
    let mut day = first_day("stores-take-module", 1600.0, 900.0);
    let module = day
        .registries
        .items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| {
            matches!(
                day.registries.items.get(&record.item).map(|def| &def.kind),
                Some(strata_registry::item::ItemKind::Module { .. })
            )
        })
        .cloned()
        .expect("a module in the first day's store");
    day.carry(&module);
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.set_focus(Some(pack0));
    day.h.settle();
    assert_eq!(day.pinned(), Some(pack0));
    assert!(button_disabled(&day.h, stores::TAKE));
    assert_eq!(day.says(stores::NOTE_ID), "That is not a weapon.");
}

// ---- the bike (ticket 0009, step 2, second half) ----

const BIKE: &str = "veyra:vehicle.courier_bike";

impl Day {
    /// A store grade bike, as the board sells it.
    fn bike(&self) -> strata_registry::item::Record {
        strata_sim::ride::store_grade_bike(&self.registries.items, &BIKE.parse().unwrap())
            .expect("a bike item")
    }

    /// The household's ride, with an empty saddle.
    fn stable_the_bike(&mut self, condition: Option<u16>) {
        let mut bike = self.bike();
        bike.condition = condition;
        let registries = self.registries.clone();
        self.h
            .world_mut()
            .insert_resource(strata_sim::ride::Stable::with_bike(
                bike,
                &registries.items,
                &registries.vehicles,
            ));
    }

    fn mount(&mut self) {
        let id = BIKE.parse().unwrap();
        let def = self.registries.vehicles.get(&id).unwrap();
        let riding = strata_sim::ride::Riding::new(def, 0.0);
        let player = self.player;
        self.h.world_mut().entity_mut(player).insert(riding);
        self.h.step(2);
        self.h.settle();
    }

    /// The pinned bike's button reads and says.
    fn take_button(&self) -> (String, bool, String) {
        let take = find(&self.h, stores::TAKE);
        (
            self.h.world().get::<SemanticLabel>(take).unwrap().0.clone(),
            button_disabled(&self.h, stores::TAKE),
            self.says(stores::NOTE_ID),
        )
    }
}

/// A bike in the pack is pinned with Take as ride, which makes it the
/// household's ride, and the RIDE pane then names it with Put away live.
#[test]
fn take_as_ride_stables_the_bike_and_the_ride_pane_follows() {
    let mut day = first_day("stores-bike-take", 1600.0, 900.0);
    day.carry(&day.bike());
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.set_focus(Some(pack0));
    day.h.settle();
    assert_eq!(day.pinned(), Some(pack0));
    let (label, grey, _) = day.take_button();
    assert_eq!(label, "Take as ride");
    assert!(!grey);
    // No bike stabled yet, so no RIDE pane.
    assert!(day.h.try_find(&by::test_id(stores::RIDE_PUT)).is_none());
    let take = find(&day.h, stores::TAKE);
    let cols = usize::from(day.registries.items.carry().pack.cols);
    day.walk_to(UiAction::Right, take, cols + 6);
    day.h.action(UiAction::Accept);
    day.tick();
    day.h.step(6);
    day.h.settle();
    let stable = day.h.world().resource::<strata_sim::ride::Stable>();
    assert!(stable.bike.is_some(), "the bike is the household's ride");
    let id = day.id(BIKE);
    assert!(
        !day.pack()
            .cells
            .slots()
            .iter()
            .flatten()
            .any(|s| s.id == id),
        "it left the pack"
    );
    assert_eq!(day.says(stores::RIDE_NAME_ID), "Courier bike");
    assert!(
        !button_disabled(&day.h, stores::RIDE_PUT),
        "Put away is live"
    );
    assert_eq!(day.says(stores::RIDE_PUT_WHY), "");
}

/// Mounted, a bike pinned cannot be taken and Put away is grey: both say
/// to get off first, in the sim's own refusal's words.
#[test]
fn take_as_ride_and_put_away_are_grey_while_mounted_with_their_reason() {
    let mut day = first_day("stores-bike-mounted", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    day.carry(&day.bike());
    day.mount();
    day.open_pack_by_key();
    let pack0 = day.cell("pack", 0);
    day.h.set_focus(Some(pack0));
    day.h.settle();
    let (label, grey, note) = day.take_button();
    assert_eq!(label, "Take as ride");
    assert!(grey);
    assert_eq!(note, "Get off the bike first");
    assert!(button_disabled(&day.h, stores::RIDE_PUT));
    assert_eq!(day.says(stores::RIDE_PUT_WHY), "Get off the bike first");
}

/// The RIDE pane's saddle is drawn shut with its one line on foot away from
/// a locker, and open, with the line gone, once mounted.
#[test]
fn the_saddle_is_shut_on_foot_and_open_mounted() {
    let mut day = first_day("stores-saddle", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    day.open_pack_by_key();
    let shut = find(&day.h, stores::RIDE_SHUT);
    assert_eq!(day.says(stores::RIDE_SHUT), "Mount up or stand at a locker");
    assert!(day.laid_out(shut), "the line is said on foot");
    assert_eq!(day.says(stores::RIDE_NAME_ID), "Courier bike");
    // Nothing is worn, so Repair is grey with its reason.
    assert!(button_disabled(&day.h, stores::RIDE_REPAIR));
    assert_eq!(day.says(stores::RIDE_REPAIR_WHY), "Nothing to repair.");
    day.close();
    day.mount();
    day.open_pack_by_key();
    let shut = find(&day.h, stores::RIDE_SHUT);
    assert!(!day.laid_out(shut), "mounted, the saddle needs no line");
    // The grid is there to click in: 6 by 3 cells.
    let first = day.cell("saddle", 0);
    let last = day.cell("saddle", 17);
    assert!(day.laid_out(first) && day.laid_out(last));
}

/// T on a weapon in the saddle bags sends it across as it always did: the
/// take-in-hand key is for the pack alone, and the sim does not look in the
/// saddle.
#[test]
fn t_on_a_saddle_weapon_still_quick_moves_it() {
    let mut day = first_day("stores-t-saddle", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    day.mount();
    let blade = day
        .registries
        .items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| record.item.path() == "weapon.service_blade")
        .cloned()
        .expect("the first day stocks service blades");
    let stack = strata_sim::items::write(&blade, &day.registries.items).unwrap();
    day.h
        .world_mut()
        .resource_mut::<strata_sim::ride::Stable>()
        .saddle
        .set(0, Some(stack));
    day.open_pack_by_key();
    let cell = day.cell("saddle", 0);
    day.h.set_focus(Some(cell));
    day.h.settle();
    day.h.key(stores::SEND_KEY);
    day.tick();
    day.h.settle();
    let hands = day
        .h
        .world()
        .get::<strata_sim::items::Hands>(day.player)
        .unwrap();
    let held: Vec<_> = hands
        .saved(&day.registries.items)
        .unwrap()
        .into_iter()
        .flatten()
        .map(|record| record.item.path().to_owned())
        .collect();
    assert!(
        !held.iter().any(|path| path == "weapon.service_blade"),
        "the blade was quick moved, not taken in hand: {held:?}"
    );
    assert!(
        day.h
            .world()
            .resource::<strata_sim::ride::Stable>()
            .saddle
            .get(0)
            .is_none(),
        "the quick move took it out of the saddle"
    );
}

/// At a locker with a bike, at 1280 by 720 with the arrival's bag open,
/// every cell of the saddle and the Repair and Put away buttons lie inside
/// the body's visible area with no scrolling: the panel's rect would pass
/// a pane the body clips.
#[test]
fn the_ride_pane_is_in_view_beside_the_store() {
    let mut day = first_day("stores-ride-small", 1280.0, 720.0);
    day.stable_the_bike(Some(1000));
    day.use_locker();
    let body = day.h.rect_of(find(&day.h, stores::BODY_ID));
    let mut parts = vec![
        ("Put away", find(&day.h, stores::RIDE_PUT)),
        ("Repair", find(&day.h, stores::RIDE_REPAIR)),
        (
            "the first bag's last cell",
            day.cell(&stores::bag_region(0), 5),
        ),
    ];
    let cells: Vec<_> = (0..18).map(|cell| day.cell("saddle", cell)).collect();
    parts.extend(cells.into_iter().map(|cell| ("a saddle cell", cell)));
    for (what, entity) in parts {
        let rect = day.h.rect_of(entity);
        assert!(rect.width() > 0., "{what} is not laid out");
        assert!(
            within(rect, body),
            "{what} {rect:?} outside the body's visible {body:?}"
        );
    }
    let shut = find(&day.h, stores::RIDE_SHUT);
    assert!(!day.laid_out(shut), "at a locker the saddle needs no line");
}

/// A repair answer is the fitting screen's as well: a weapon mended there
/// must leave the fitting screen up, with the stores below it untouched.
#[test]
fn a_weapon_repair_does_not_close_the_fitting_screen() {
    let mut day = first_day("stores-repair-keeps", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    day.h.key(stores::KEY);
    day.h.settle();
    let carbine = find(&day.h, &stores::hand_slot_id(0));
    day.h.click(carbine);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    day.h
        .world_mut()
        .write_message(strata_sim::wear::RepairAnswer {
            result: Ok(strata_sim::wear::Repaired {
                gain: 100,
                to: 1000,
                at_bench: false,
            }),
        });
    day.h.step(4);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
}

/// A call that finishes while the pack is open opens the saddle in the
/// screen's copy too: a stack clicked into a saddle cell lands in the sim's.
#[test]
fn mounting_with_the_pack_open_opens_the_saddle_cells() {
    let mut day = first_day("stores-mount-open", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    let feed = day.id(FEED);
    day.h
        .world_mut()
        .get_mut::<Pack>(day.player)
        .unwrap()
        .cells
        .set(0, Some(ItemStack::new(feed, 3)));
    day.open_pack_by_key();
    day.mount();
    let (pack0, saddle0) = (day.cell("pack", 0), day.cell("saddle", 0));
    day.h.click(pack0);
    day.h.settle();
    day.h.click(saddle0);
    day.h.settle();
    day.tick();
    let stable = day.h.world().resource::<strata_sim::ride::Stable>();
    assert_eq!(stable.saddle.get(0).map(|stack| stack.count), Some(3));
}

/// Shift-click from the pack to the saddle while mounted: the stack leaves
/// the pack for the saddle's first cell, in the screen and in the sim's.
#[test]
fn shift_click_from_the_pack_fills_the_saddle_while_mounted() {
    let mut day = first_day("stores-saddle-shift-in", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    let feed = day.id(FEED);
    day.h
        .world_mut()
        .get_mut::<Pack>(day.player)
        .unwrap()
        .cells
        .set(0, Some(ItemStack::new(feed, 3)));
    day.mount();
    day.open_pack_by_key();
    let (pack0, saddle0) = (day.cell("pack", 0), day.cell("saddle", 0));
    assert_eq!(day.shows(pack0), Some((feed, 3)));
    day.h.shift_click(pack0);
    day.h.settle();
    day.tick();
    assert_eq!(day.shows(pack0), None, "it left the pack");
    assert_eq!(day.shows(saddle0), Some((feed, 3)), "and is in the saddle");
    let stable = day.h.world().resource::<strata_sim::ride::Stable>();
    assert_eq!(stable.saddle.get(0).map(|stack| stack.count), Some(3));
    assert!(day.pack().cells.get(0).is_none(), "the truth moved too");
}

/// At a locker the saddle is open on foot, and Shift-click from it sends
/// the stack to the household's store.
#[test]
fn shift_click_from_the_saddle_goes_to_the_store_at_a_locker() {
    let mut day = first_day("stores-saddle-shift-out", 1600.0, 900.0);
    day.stable_the_bike(Some(1000));
    let rations = day.id(CELL);
    let held = |slots: &[Option<ItemStack>]| {
        slots
            .iter()
            .flatten()
            .filter(|stack| stack.id == rations)
            .map(|stack| stack.count)
            .sum::<u32>()
    };
    let (store_before, pack_before) = (held(day.store().0.slots()), held(day.pack().cells.slots()));
    day.h
        .world_mut()
        .resource_mut::<strata_sim::ride::Stable>()
        .saddle
        .set(0, Some(ItemStack::new(rations, 2)));
    let before = day.truth();
    day.use_locker();
    let saddle0 = day.cell("saddle", 0);
    assert_eq!(day.shows(saddle0), Some((rations, 2)), "open at a locker");
    day.h.shift_click(saddle0);
    day.h.settle();
    day.tick();
    assert_eq!(day.shows(saddle0), None, "the saddle is empty");
    let stable = day.h.world().resource::<strata_sim::ride::Stable>();
    assert!(stable.saddle.get(0).is_none(), "in the sim's too");
    let after = day.truth();
    assert_eq!(
        after[&rations],
        before[&rations] + 2,
        "the pack and the store hold what it held"
    );
    assert_eq!(
        held(day.store().0.slots()),
        store_before + 2,
        "it went to the store"
    );
    assert_eq!(
        held(day.pack().cells.slots()),
        pack_before,
        "and not to the pack"
    );
}

/// Repair on the RIDE pane is live with a worn bike and a kit in reach: a
/// click spends the kit, raises the bike's condition and greys the button
/// with its reason.
#[test]
fn repair_on_the_ride_pane_mends_the_bike_with_a_kit() {
    let mut day = first_day("stores-ride-repair", 1600.0, 900.0);
    day.stable_the_bike(Some(300));
    let kit = day.id("veyra:kit.field_repair");
    // Away from a bench a kit has to be carried: the store's do not count.
    let mut record = day
        .registries
        .items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| record.item.to_string() == "veyra:kit.field_repair")
        .cloned()
        .expect("the first day holds a kit");
    record.count = 1;
    day.carry(&record);
    let kits = |day: &Day| {
        day.pack()
            .cells
            .slots()
            .iter()
            .flatten()
            .filter(|stack| stack.id == kit)
            .map(|stack| stack.count)
            .sum::<u32>()
    };
    let before = kits(&day);
    assert_eq!(before, 1);
    day.open_pack_by_key();
    let why = day.says(stores::RIDE_REPAIR_WHY);
    assert!(
        !button_disabled(&day.h, stores::RIDE_REPAIR),
        "worn and a kit carried, so live: {why}"
    );
    let repair = find(&day.h, stores::RIDE_REPAIR);
    day.h.click(repair);
    day.h.settle();
    day.tick();
    day.h.step(6);
    day.h.settle();
    let condition = day
        .h
        .world()
        .resource::<strata_sim::ride::Stable>()
        .bike
        .as_ref()
        .and_then(|bike| bike.condition)
        .unwrap();
    assert!(condition > 300, "the bike is mended: {condition}");
    assert_eq!(kits(&day), 0, "the carried kit is spent");
}

/// A pack can hold any item the catalogue has, and every one of them is
/// drawn by an image in its cell, never by slotted's stand-in shape, with
/// nothing baked: a lot in its class's colour, a painted square (the
/// repair kit) in its own, two kinds apart by their image.
#[test]
fn every_item_has_an_image_and_none_a_stand_in() {
    use slotted::registry::IconDef;
    use strata_client::ui::card::ItemImages;

    let mut day = first_day("stores-images", 1600.0, 900.0);
    day.h.settle();
    let frozen = day
        .h
        .world()
        .resource::<slotted::ecs::Registries>()
        .0
        .clone();
    let tints = day.h.world().resource::<stores::PicturedIcons>().0.clone();
    let images = day.h.world().resource::<ItemImages>().0.clone();
    let items = &day.registries.items;
    let mut paths = std::collections::BTreeSet::new();
    for def in items.iter() {
        let id = items.id_of(&def.id).expect("an id");
        let drawn = frozen
            .items
            .get(id)
            .and_then(|slotted| slotted.icon.clone())
            .unwrap_or_else(|| panic!("{} has no icon", def.id));
        let IconDef::Image(path) = drawn else {
            panic!("{} is drawn by a stand-in shape", def.id);
        };
        assert_eq!(images[&def.id.to_string()].path, path, "{}", def.id);
        paths.insert(path);
        let painted = day
            .registries
            .icons
            .is_square(def.icon.symbol.as_deref().unwrap_or_default());
        assert_eq!(
            tints.contains_key(&id),
            !painted,
            "{}: a painted square is never tinted",
            def.id
        );
        if matches!(def.kind, strata_registry::item::ItemKind::Lot { .. }) {
            let [red, green, blue, _] = def.icon.color.0;
            assert_eq!(tints[&id], Color::srgb(red, green, blue), "{}", def.id);
        }
    }
    assert!(paths.len() >= 12, "only {} different images", paths.len());
    let kit = items
        .id_of(&strata_registry::Namespaced::parse("veyra:kit.field_repair").unwrap())
        .expect("the repair kit");
    assert!(
        !tints.contains_key(&kit),
        "the repair kit's square is its own"
    );
}
