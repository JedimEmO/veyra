//! The crafting screen at its edges: late refusals, the action row, a session
//! left open, finishing, reading away from a bench, tab switching and reach,
//! small windows and the Parts chip.

use crate::ui;
use bevy::math::Rect;
use bevy::prelude::*;
use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted_test::prelude::*;
use strata_registry::item::Record;
use strata_registry::schematic::Hand;
use veyra_sim::crafting::CraftInbox;
use veyra_sim::crafting::CraftRequest;
use veyra_sim::deposits::Purse;
use veyra_sim::items::HouseholdStore;
use veyra_sim::items::Pack;
use veyra_ui::crafting;
use veyra_ui::crafting::WorkbenchTab;
use veyra_ui::fitting;
use veyra_ui::stores;
use veyra_ui::stores::StoresState;

use crate::common::*;
use crate::crafting::*;

/// Workable stock moves the odds of the whole session, so it is seen
/// before anything is spent: a casing's row shows its Workability and what
/// that does, and the sentence under what Assemble would use comes and
/// goes with the pick. Assembled with the workable alloy, the bench hand
/// says the same and its bars are the better row.
#[test]
#[allow(clippy::too_many_lines)]
fn workable_stock_is_said_before_assemble() {
    let (mut day, _) = craft_day("craft-workable", 1600.0, 900.0);
    let registries = day.registries.clone();
    let harrow = lot_of(
        &registries,
        ALLOY,
        "Harrow",
        200,
        &[610, 540, 560, 410, 520, 700, 320, 760],
    );
    day.stock(&harrow);
    let core = Record {
        item: strata_registry::Namespaced::parse(CORE_ITEM).unwrap(),
        count: 1,
        batch: 1,
        by: Some("Frontier ranger".to_owned()),
        lines: [("charge_density", 466), ("stability", 209)]
            .map(|(line, figure)| (line.to_owned(), figure))
            .into(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: None,
    };
    day.carry(&core);
    let before = day.truth();
    day.open_crafting("patrol_extended_feed");
    let workable = |day: &Day| {
        day.h
            .try_find(&by::test_id(crafting::WORKABLE_ID))
            .map(|entity| day.h.text_of(entity).unwrap_or_default())
    };
    // The list: both alloys say their workability and that it is the odds
    // it moves.
    let open = find(&day.h, &crafting::open_id("casing"));
    day.h.click(open);
    day.h.settle();
    let rows: Vec<Vec<String>> = (0..2)
        .map(|at| day.row(&crafting::pick_id("casing", at)))
        .collect();
    for (name, figure) in [("Rake light alloy", "492"), ("Harrow light alloy", "760")] {
        let row = rows
            .iter()
            .find(|row| row.iter().any(|text| text.ends_with(name)))
            .unwrap_or_else(|| panic!("{name} is listed: {rows:?}"));
        let at = row.iter().position(|text| text == "Workability").unwrap();
        assert_eq!(
            row[at + 1..at + 3],
            [figure.to_owned(), "moves the odds".to_owned()],
            "{row:?}"
        );
    }
    let at = |name: &str| {
        rows.iter()
            .position(|row| row.iter().any(|text| text.ends_with(name)))
            .unwrap()
    };
    let (rake, harrow) = (at("Rake light alloy"), at("Harrow light alloy"));
    // The plain alloy: nothing is said. The workable one: the sentence.
    let pick = |day: &mut Day, row: usize| {
        if day
            .h
            .try_find(&by::test_id(&crafting::pick_id("casing", row)))
            .is_none()
        {
            let open = find(&day.h, &crafting::open_id("casing"));
            day.h.click(open);
            day.h.settle();
        }
        let row = find(&day.h, &crafting::pick_id("casing", row));
        day.h.click(row);
        day.h.settle();
    };
    pick(&mut day, rake);
    assert_eq!(day.row(&crafting::open_id("casing"))[0], "Rake light alloy");
    assert_eq!(workable(&day), None);
    pick(&mut day, harrow);
    assert_eq!(
        day.row(&crafting::open_id("casing"))[0],
        "Harrow light alloy"
    );
    assert_eq!(
        workable(&day).as_deref(),
        Some("Workable stock. The odds are a row better.")
    );
    pick(&mut day, rake);
    assert_eq!(workable(&day), None, "and it goes with the pick");
    day.tick();
    assert!(day.craft().session.is_none());
    assert_eq!(day.truth(), before, "nothing was spent by looking");
    // Assembled with the workable alloy, the hand says the same.
    pick(&mut day, harrow);
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    assert_eq!(
        day.craft().session.expect("assembled").workability,
        Some(760)
    );
    assert_eq!(
        workable(&day).as_deref(),
        Some("Workable stock. The odds are a row better.")
    );
    for (bar, chance) in [15_u8, 65, 20, 0, 0].into_iter().enumerate() {
        let entity = find(&day.h, &crafting::odds_id("capacity", Hand::Steady, bar));
        assert_eq!(
            day.h.world().get::<crafting::OddsBar>(entity).unwrap().0,
            chance
        );
    }
}

/// A refusal the sim gives after the fact names the slot as its box does:
/// the stock moved between the press and the tick, and the footer says
/// which slot by its name and not by its key.
#[test]
fn a_late_refusal_names_the_slot_as_its_box_does() {
    let (mut day, _) = craft_day("craft-late-refusal", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    assert!(day.craft().session.is_none(), "the tick has not run");
    // The salt leaves the store under the request.
    {
        let items = day.registries.items.clone();
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        let cell = store
            .0
            .slots()
            .iter()
            .position(|stack| {
                stack
                    .as_ref()
                    .and_then(|stack| veyra_sim::items::read(stack, &items).ok())
                    .and_then(|record| record.origin)
                    .is_some_and(|origin| origin.root == "Tolley")
            })
            .unwrap();
        store.0.set(cell, None);
        store.0.clear_changed();
    }
    day.tick();
    assert!(day.craft().session.is_none(), "nothing was taken");
    assert_eq!(day.units_of("Oxbow"), 160);
    assert_eq!(
        day.says(crafting::STATUS_ID),
        "Reactive: 0 in stock, needs 20."
    );
    assert_eq!(
        day.role_of(crafting::STATUS_ID),
        "strata.fitting.status.refused"
    );
}

/// The buttons that act stand at the foot of the right column in all three
/// states, so the primary one does not move as a craft goes from the slots
/// to the hand to the card, nor when the reason over it comes and goes.
#[test]
fn the_action_row_stands_at_the_columns_foot() {
    let (mut day, _) = craft_day("craft-foot", 1600.0, 900.0);
    let foot = |day: &Day, id: &str| day.h.rect_of(find(&day.h, id)).max.y;
    // The magazine with no core to pick: Assemble grey, with its reason.
    day.open_crafting("patrol_extended_feed");
    assert!(day.inert(crafting::ASSEMBLE));
    assert_ne!(day.says(crafting::WHY_ID), "");
    let grey = foot(&day, crafting::ASSEMBLE);
    let why = day.h.rect_of(find(&day.h, crafting::WHY_ID));
    assert!(why.max.y <= day.h.rect_of(find(&day.h, crafting::ASSEMBLE)).min.y + 0.5);
    // The core, ready: no reason, the same place.
    day.open_crafting("cell_core");
    assert!(!day.inert(crafting::ASSEMBLE));
    let ready = foot(&day, crafting::ASSEMBLE);
    assert!((ready - grey).abs() < 1., "{ready} against {grey}");
    assert!((foot(&day, crafting::BEST) - ready).abs() < 1.);
    // At the foot: nothing of the columns is under the row.
    let root = day.h.rect_of(find(&day.h, crafting::ROOT_ID));
    let status = day.h.rect_of(find(&day.h, crafting::STATUS_ID));
    assert!(ready <= status.min.y + 0.5, "over the footer");
    assert!(
        status.min.y - ready < 40.,
        "{ready} is the last thing over the footer at {}",
        status.min.y
    );
    assert!(root.height() > 600., "the frame the three states share");
    // The bench hand: Finish where Assemble was.
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    let finish = foot(&day, crafting::FINISH);
    assert!((finish - ready).abs() < 1., "{finish} against {ready}");
    // The finished card: Make another in the same place.
    let button = find(&day.h, crafting::FINISH);
    day.h.click(button);
    day.tick();
    let again = foot(&day, crafting::AGAIN);
    assert!((again - ready).abs() < 1., "{again} against {ready}");
}

/// A session left open is there when the workbench is used again: the
/// materials were committed at assembly and are not lost with the screen.
/// F opens on it whatever tab was last shown, the other schematics wait
/// for the bench, and the session finishes as if it had never been left.
#[test]
fn a_session_left_open_is_there_again() {
    let (mut day, bench) = craft_day("craft-left-open", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    let steady = find(&day.h, &crafting::roll_id("charge_density", Hand::Steady));
    day.h.click(steady);
    day.tick();
    let left = day.craft();
    assert_eq!(left.session.as_ref().expect("open").points, 2);
    assert_eq!((day.units_of("Tolley"), day.units_of("Oxbow")), (140, 130));
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    day.tick();
    assert_eq!(day.craft(), left, "closing the screen changes nothing");

    // Even with the fitting tab the one last shown.
    *day.h.world_mut().resource_mut::<WorkbenchTab>() = WorkbenchTab::Fitting;
    day.bench_f(bench);
    assert_eq!(day.h.stack_top(), Some(crafting::kind()));
    assert_eq!(day.says(crafting::POINTS_ID), "2 points left");
    let figure = left.session.as_ref().unwrap().lines[0].figure();
    assert_eq!(
        day.says(&format!("{}charge_density", crafting::STANDS_TAG)),
        format!("{figure} of 597")
    );
    assert_eq!(
        texts_under(&day.h, find(&day.h, crafting::ROOT_ID))
            .iter()
            .filter(
                |text| ["Tolley salt", "20 units", "Oxbow brine", "30 units"]
                    .contains(&text.as_str())
            )
            .count(),
        4,
        "what went in is still said"
    );
    let other = "crafting.schem:schematic.patrol_extended_feed";
    assert!(day.inert(other));
    // A row is one line, the name and the mount, with no reason under it:
    // the shown schematic is the one on the bench, which says its own.
    assert_eq!(
        day.row(other),
        ["Patrol extended magazine".to_owned(), "FEED".to_owned()]
    );
    // The other tab and back: still there.
    day.h.action(UiAction::TabPrev);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    day.h.action(UiAction::TabNext);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(crafting::kind()));
    assert_eq!(day.says(crafting::POINTS_ID), "2 points left");
    let finish = find(&day.h, crafting::FINISH);
    day.h.click(finish);
    day.tick();
    assert!(day.craft().session.is_none());
    let core = &day.carried_of(CORE_ITEM)[0];
    assert_eq!(core.lines["charge_density"], figure);
}

/// With the pack, its bags and the store all full, Finish is refused by
/// the sim and says so at the button; the session stays on the bench with
/// nothing lost, and finishes once there is room.
#[test]
fn finishing_with_no_room_says_so() {
    let (mut day, _) = craft_day("craft-no-room", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    day.fill_the_pack();
    {
        let items = &day.registries.items;
        let mut one = items.starting().unwrap().store[1].clone();
        one.count = 1;
        one.batch = 5000;
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        for cell in 0..store.0.len() {
            if store.0.get(cell).is_none() {
                one.batch += 1;
                let single = veyra_sim::items::write(&one, items).unwrap();
                store.0.set(cell, Some(single));
            }
        }
        store.0.clear_changed();
    }
    day.h.settle();
    let before = day.craft();
    let finish = find(&day.h, crafting::FINISH);
    day.h.click(finish);
    day.tick();
    assert_eq!(day.craft(), before, "nothing made, no batch spent");
    assert_eq!(day.says(crafting::WHY_ID), "No room");
    assert_eq!(day.role_of(crafting::WHY_ID), "strata.fitting.why");
    assert_eq!(day.says(crafting::STATUS_ID), "No room");
    assert_eq!(
        day.role_of(crafting::STATUS_ID),
        "strata.fitting.status.refused"
    );
    assert!(day.carried_of(CORE_ITEM).is_empty());
    // A cell freed, it finishes.
    {
        let mut pack = day.h.world_mut().get_mut::<Pack>(day.player).unwrap();
        let last = pack.cells.len() - 1;
        pack.cells.set(last, None);
        pack.cells.clear_changed();
    }
    day.h.settle();
    let finish = find(&day.h, crafting::FINISH);
    day.h.click(finish);
    day.tick();
    assert!(day.craft().session.is_none());
    assert_eq!(day.carried_of(CORE_ITEM).len(), 1);
}

/// Away from a workbench the tab is a reading: the slots, the picks and
/// what would come out all work on what the character carries, the store
/// is out of reach, and Assemble is grey with the reason beside it. A
/// request that reached the sim all the same is refused there in the same
/// words.
#[test]
fn away_from_a_workbench_crafting_is_a_reading() {
    let mut day = first_day("craft-away", 1600.0, 900.0);
    let [salt, brine, alloy] = first_lots(&day.registries.clone());
    day.stock(&alloy);
    day.carry(&salt);
    day.h.world_mut().insert_resource(Purse(2500));
    day.open_crafting("cell_core");
    assert_eq!(
        day.says(crafting::STATUS_ID),
        "A reading. Things are made at a workbench."
    );
    // The salt carried is a candidate; no brine is in reach.
    assert_eq!(day.row(&crafting::open_id("salt"))[0], "Tolley salt");
    assert_eq!(
        day.says(&format!("{}brine", crafting::NONE_TAG)),
        "Nothing in the pack fits"
    );
    assert_eq!(
        day.says(&format!("{}charge_density", crafting::RANGE_TAG)),
        "Fill the slots to see this."
    );
    assert!(day.inert(crafting::ASSEMBLE));
    assert_eq!(day.says(crafting::WHY_ID), "Step up to a workbench");
    assert_eq!(day.role_of(crafting::WHY_ID), "strata.fitting.why");
    // With the brine carried too the outcome is all there, and Assemble
    // is still grey for the same reason.
    day.carry(&brine);
    let best = find(&day.h, crafting::BEST);
    day.h.click(best);
    day.h.settle();
    assert_eq!(
        day.says(&format!("{}charge_density", crafting::RANGE_TAG)),
        "Assembles at 358, can be worked up to 597."
    );
    assert_eq!(
        day.says(&format!("{}charge_density", crafting::DOWNSTREAM_TAG)),
        "In a patrol extended magazine: 27 to 29 rounds"
    );
    assert!(day.inert(crafting::ASSEMBLE));
    assert_eq!(day.says(crafting::WHY_ID), "Step up to a workbench");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    assert!(day.craft().session.is_none());
    // The sim's own rule, for a request from anywhere.
    let picks = [("salt", &salt), ("brine", &brine)]
        .into_iter()
        .map(|(slot, lot)| (slot.to_owned(), veyra_sim::crafting::kind_of(lot)))
        .collect();
    day.h
        .world_mut()
        .resource_mut::<CraftInbox>()
        .push(CraftRequest::Assemble {
            schematic: crafting::schematic_named(&day.registries, "cell_core").unwrap(),
            picks,
        });
    day.tick();
    assert!(day.craft().session.is_none());
    assert_eq!(day.says(crafting::STATUS_ID), "Step up to a workbench");
    assert_eq!(
        day.role_of(crafting::STATUS_ID),
        "strata.fitting.status.refused"
    );
}

/// The workbench is one screen of two tabs: F opens the one last shown,
/// the tab keys and a click on the strip change it in place, and Back
/// leaves the workbench from either.
#[test]
fn q_and_e_switch_the_workbench_tabs() {
    let (mut day, bench) = craft_day("craft-tabs", 1600.0, 900.0);
    day.bench_f(bench);
    assert_eq!(day.h.stack(), [fitting::kind()]);
    let tab = |tab: WorkbenchTab| tab.tab_id();
    assert_eq!(day.row(&tab(WorkbenchTab::Fitting)), ["\u{203a} FITTING"]);
    assert_eq!(day.row(&tab(WorkbenchTab::Crafting)), ["CRAFTING"]);
    day.h.action(UiAction::TabNext);
    day.h.settle();
    assert_eq!(day.h.stack(), [crafting::kind()], "in place, not on top");
    assert_eq!(day.row(&tab(WorkbenchTab::Fitting)), ["FITTING"]);
    assert_eq!(day.row(&tab(WorkbenchTab::Crafting)), ["\u{203a} CRAFTING"]);
    assert_eq!(day.says(crafting::TITLE_ID), "Workbench");
    assert_eq!(day.says(crafting::WORKER_ID), "WORKED BY FRONTIER RANGER");
    assert_eq!(day.says(crafting::PURSE_ID), "2,500");
    day.h.action(UiAction::TabPrev);
    day.h.settle();
    assert_eq!(day.h.stack(), [fitting::kind()]);
    assert_eq!(day.says(fitting::TITLE_ID), "Vigil patrol carbine");
    // A click on the strip, and on the tab already shown.
    let crafting_tab = find(&day.h, &tab(WorkbenchTab::Crafting));
    day.h.click(crafting_tab);
    day.h.settle();
    assert_eq!(day.h.stack(), [crafting::kind()]);
    let again = find(&day.h, &tab(WorkbenchTab::Crafting));
    day.h.click(again);
    day.h.settle();
    assert_eq!(day.h.stack(), [crafting::kind()]);
    // Back leaves the workbench, and F comes back to this tab.
    day.h.action(UiAction::Back);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    day.bench_f(bench);
    assert_eq!(day.h.stack(), [crafting::kind()]);
    let fitting_tab = find(&day.h, &tab(WorkbenchTab::Fitting));
    day.h.click(fitting_tab);
    day.h.settle();
    assert_eq!(day.h.stack(), [fitting::kind()]);
    day.h.action(UiAction::Back);
    day.h.settle();
    day.bench_f(bench);
    assert_eq!(day.h.stack(), [fitting::kind()]);
}

/// Every control of the tab is in Tab's round in each of its three states,
/// the grey ones too, where the line beside them says why.
#[test]
#[allow(clippy::too_many_lines)]
fn every_crafting_control_is_reached_by_tab() {
    let (mut day, _) = craft_day("craft-tab-round", 1600.0, 900.0);
    day.open_crafting("cell_core");
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    let misses = |stops: &[String], wanted: &[String]| -> Vec<String> {
        wanted
            .iter()
            .filter(|id| !stops.contains(id))
            .cloned()
            .collect()
    };
    let always = [
        WorkbenchTab::Fitting.tab_id(),
        WorkbenchTab::Crafting.tab_id(),
        "crafting.schem:schematic.cell_core".to_owned(),
        "crafting.schem:schematic.patrol_extended_feed".to_owned(),
        "crafting.schem:schematic.pistol_extended_cell".to_owned(),
        crafting::CLOSE_ID.to_owned(),
    ];
    // Picking, with a list open.
    day.tab_to(&crafting::open_id("brine"));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.focus_id(), crafting::open_id("brine"));
    let stops = day.round();
    let mut wanted = always.to_vec();
    wanted.extend([
        crafting::open_id("salt"),
        crafting::open_id("brine"),
        crafting::pick_id("brine", 0),
        crafting::BEST.to_owned(),
        crafting::ASSEMBLE.to_owned(),
    ]);
    assert_eq!(misses(&stops, &wanted), Vec::<String>::new(), "{stops:?}");
    // A candidate's Accept picks it, closes the list and hands the focus
    // back to the slot.
    day.tab_to(&crafting::pick_id("brine", 0));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.focus_id(), crafting::open_id("brine"));
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::pick_id("brine", 0)))
            .is_none()
    );
    // The magazine with no core: Buy has nothing to offer for a made
    // part, and the grey Assemble is still a stop.
    day.tab_to("crafting.schem:schematic.patrol_extended_feed");
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.focus_id(), crafting::BEST);
    assert!(day.inert(crafting::ASSEMBLE));
    assert_eq!(day.says(crafting::WHY_ID), "Cell core: nothing is picked.");
    let stops = day.round();
    let mut wanted = always.to_vec();
    wanted.extend([
        crafting::open_id("harness"),
        crafting::open_id("casing"),
        crafting::open_id("seals"),
        crafting::BEST.to_owned(),
        crafting::ASSEMBLE.to_owned(),
    ]);
    assert_eq!(misses(&stops, &wanted), Vec::<String>::new(), "{stops:?}");
    // The bench hand.
    day.tab_to("crafting.schem:schematic.cell_core");
    day.h.action(UiAction::Accept);
    day.h.settle();
    day.h.action(UiAction::Accept);
    day.h.settle();
    day.h.action(UiAction::Accept);
    day.tick();
    assert!(day.craft().session.is_some());
    let stops = day.round();
    let mut wanted = always.to_vec();
    for line in ["charge_density", "stability"] {
        for hand in [Hand::Steady, Hand::Bold] {
            wanted.push(crafting::roll_id(line, hand));
        }
    }
    wanted.push(crafting::FINISH.to_owned());
    assert_eq!(misses(&stops, &wanted), Vec::<String>::new(), "{stops:?}");
    // The arrows walk the hands: down from the first Steady is its Bold.
    day.tab_to(&crafting::roll_id("charge_density", Hand::Steady));
    day.h.action(UiAction::Down);
    day.h.settle();
    assert_eq!(
        day.focus_id(),
        crafting::roll_id("charge_density", Hand::Bold)
    );
    // Finished.
    day.tab_to(crafting::FINISH);
    day.h.action(UiAction::Accept);
    day.tick();
    let stops = day.round();
    let mut wanted = always.to_vec();
    wanted.push(crafting::AGAIN.to_owned());
    assert_eq!(misses(&stops, &wanted), Vec::<String>::new(), "{stops:?}");
    day.tab_to(crafting::AGAIN);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(
        day.focus_id(),
        crafting::ASSEMBLE,
        "another, ready to assemble"
    );
    day.tab_to(crafting::CLOSE_ID);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert!(day.h.stack().is_empty());
}

/// The tab fits a small window in each of its states with its buttons and
/// its Close inside the panel, and at the game's sizes nothing a craft
/// needs is scrolled out of sight.
#[test]
#[allow(clippy::too_many_lines)]
fn the_crafting_screen_fits_small_windows() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0), (1000.0, 560.0)] {
        let (mut day, _) = craft_day("craft-small", width, height);
        let window = Rect::new(0., 0., width, height);
        let whole = (width, height) != (1000.0, 560.0);
        let held = |day: &Day, ids: &[String]| {
            let root = day.h.rect_of(find(&day.h, crafting::ROOT_ID));
            assert!(within(root, window), "{root:?} spills {window:?}");
            for id in [crafting::CLOSE_ID, crafting::STATUS_ID, crafting::TITLE_ID] {
                let rect = day.h.rect_of(find(&day.h, id));
                assert!(within(rect, root), "{id}: {rect:?} outside {root:?}");
            }
            if whole {
                for id in ids {
                    let rect = day.h.rect_of(find(&day.h, id));
                    assert!(
                        within(rect, root),
                        "{id} at {width}x{height}: {rect:?} outside {root:?}"
                    );
                }
            }
        };
        // The magazine's four slots, the longest of the picking states.
        let core = Record {
            item: strata_registry::Namespaced::parse(CORE_ITEM).unwrap(),
            count: 1,
            batch: 1,
            by: Some("Frontier ranger".to_owned()),
            lines: [("charge_density", 466), ("stability", 209)]
                .map(|(line, figure)| (line.to_owned(), figure))
                .into(),
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        };
        day.carry(&core);
        day.open_crafting("patrol_extended_feed");
        let mut ids = vec![
            crafting::BEST.to_owned(),
            crafting::ASSEMBLE.to_owned(),
            crafting::USES_ID.to_owned(),
            // The row of the schematic being worked, not its neighbour: the
            // list is five rows longer since the rifle (ticket 0010, step 4)
            // and a neighbour scrolls out of a 720 p panel.
            "crafting.schem:schematic.patrol_extended_feed".to_owned(),
        ];
        for slot in ["core", "harness", "casing", "seals"] {
            ids.push(format!("{}{slot}", crafting::SLOT_TAG));
        }
        for line in ["capacity", "reload_time"] {
            ids.push(format!("{}{line}", crafting::LINE_TAG));
        }
        held(&day, &ids);
        // No row of the list is taller than its two lines, and a slot's
        // button is one row of the middle column.
        if whole {
            let row = day.h.rect_of(find(
                &day.h,
                "crafting.schem:schematic.patrol_extended_feed",
            ));
            assert!(row.height() < 64., "{row:?}");
            let open = day.h.rect_of(find(&day.h, &crafting::open_id("casing")));
            assert!(open.height() < 64., "{open:?}");
        }
        // The bench hand of five points and two lines.
        let assemble = find(&day.h, crafting::ASSEMBLE);
        day.h.click(assemble);
        day.tick();
        assert!(day.craft().session.is_some());
        let mut ids = vec![crafting::FINISH.to_owned(), crafting::POINTS_ID.to_owned()];
        for line in ["capacity", "reload_time"] {
            for hand in [Hand::Steady, Hand::Bold] {
                ids.push(crafting::roll_id(line, hand));
                ids.push(format!("{}{line}:{}", crafting::OFFER_TAG, hand.key()));
            }
        }
        held(&day, &ids);
        // A hand's words end inside the column: nothing is clipped.
        if whole {
            let root = day.h.rect_of(find(&day.h, crafting::ROOT_ID));
            let offer = day.h.rect_of(find(
                &day.h,
                &format!("{}capacity:bold", crafting::OFFER_TAG),
            ));
            assert!(offer.max.x <= root.max.x - 8., "{offer:?} in {root:?}");
        }
        // The finished card.
        let finish = find(&day.h, crafting::FINISH);
        day.h.click(finish);
        day.tick();
        assert!(day.craft().session.is_none());
        held(
            &day,
            &[crafting::AGAIN.to_owned(), crafting::CARD_ID.to_owned()],
        );
    }
}

/// The stores' Parts chip stands after Lots and shows what is made of lots
/// or bought in and goes into something else; pinned, a part's long card
/// says where its line goes.
#[test]
fn the_parts_chip_shows_parts() {
    use veyra_ui::stores::Filter;
    let mut day = first_day("stores-parts", 1600.0, 900.0);
    day.use_locker();
    assert_eq!(Filter::ALL[2], Filter::Parts, "the chip after Lots");
    let chip = find(&day.h, &Filter::Parts.chip_id());
    assert_eq!(texts_under(&day.h, chip), ["PARTS"]);
    let mut commands = day.h.world_mut().commands();
    stores::show_only(&mut commands, Filter::Parts);
    day.h.world_mut().flush();
    day.h.step(3);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().filter,
        Filter::Parts
    );
    let chip = find(&day.h, &Filter::Parts.chip_id());
    assert_eq!(texts_under(&day.h, chip), ["\u{203a} PARTS"]);
    let card = day.card();
    assert_eq!(card.name, "Harness");
    assert_eq!(card.kind, "Part");
    assert_eq!(card.from, "Store grade · the dealer · 3 in the stack");
    // The carbine's schematic sorts before the magazine's.
    assert_eq!(card.lines[0].note, "Goes into: Action of a patrol carbine");
    let items = &day.registries.items;
    for def in items.iter() {
        assert_eq!(
            Filter::Parts.shows(&def.kind),
            [
                "veyra:component.cell_core",
                "veyra:component.drive_coil",
                "veyra:component.hull_plate",
                "veyra:component.mount_blank",
                HARNESS,
                SEALS,
                // The questline's slate (ticket 0014): a component for the
                // one line the board grades it on.
                "veyra:item.station_record",
            ]
            .contains(&def.id.to_string().as_str()),
            "{}",
            def.id
        );
    }
}

/// A list of fifteen schematics on a 720p window scrolls alone: focusing the
/// last row (the field repair kit) brings it inside the list's own rectangle.
#[test]
fn the_schematic_list_scrolls_the_last_row_into_view() {
    let (mut day, _) = craft_day("craft-list-scroll", 1280.0, 720.0);
    day.open_crafting("cell_core");
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    let last = "crafting.schem:schematic.field_repair_kit";
    day.tab_to(last);
    for _ in 0..4 {
        day.h.settle();
    }
    let scroll = day.h.rect_of(find(&day.h, crafting::LIST_ID));
    let row = day.h.rect_of(find(&day.h, last));
    assert!(
        row.min.y >= scroll.min.y - 2. && row.max.y <= scroll.max.y + 2.,
        "{row:?} outside the list {scroll:?}"
    );
}
