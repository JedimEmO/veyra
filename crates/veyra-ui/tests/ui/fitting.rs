//! The fitting screen (ticket 0007, step 2): fitting and removing modules at
//! a workbench, and the reading away from one.

use crate::ui;
use bevy::asset::AssetApp;
use bevy::math::Rect;
use bevy::prelude::*;
use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted_test::prelude::*;
use strata_registry::item::Property;
use strata_registry::item::Record;
use veyra_sim::WorldPosition;
use veyra_sim::combat::Combatant;
use veyra_sim::fitting::FitInbox;
use veyra_sim::fitting::FitRequest;
use veyra_sim::items::Hands;
use veyra_sim::progression::Interact;
use veyra_sim::progression::Interaction;
use veyra_sim::progression::InteractionKind;
use veyra_ui::card;
use veyra_ui::fitting;
use veyra_ui::stores;
use veyra_ui::stores::StoresState;
use veyra_ui::weapon_diagram as diagram;
use veyra_ui::weapon_diagram::Diagram;

use crate::common::*;
use crate::weapon_diagram::{
    carbine_pictures, drawn, mount_button, odd_pictures, picks, shown, stage,
};

pub(crate) const REFLEX: &str = "veyra:module.vigil_reflex";

/// [`first_day`] with a workbench within reach beside the locker.
pub(crate) fn bench_day(name: &str, width: f32, height: f32) -> (Day, Entity) {
    let mut day = first_day(name, width, height);
    let bench = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(2.0, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Workbench,
                radius: 3.0,
            },
        ))
        .id();
    // A tick, so the sim's reach has found it.
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(
        day.h
            .world()
            .resource::<veyra_sim::progression::Reach>()
            .workbench
    );
    (day, bench)
}

impl Day {
    /// F at the workbench, as the keyboard and the game's script raise it.
    pub(crate) fn use_workbench(&mut self, bench: Entity) {
        self.h.world_mut().write_message(Interact {
            entity: bench,
            kind: InteractionKind::Workbench,
        });
        self.h.step(2);
        self.h.settle();
        assert_eq!(self.h.stack_top(), Some(fitting::kind()));
    }

    /// A tick of the sim, in which it consumes what a screen asked, and the
    /// frames in which the screen hears the answer.
    pub(crate) fn tick(&mut self) {
        self.h.advance(std::time::Duration::from_millis(120));
        self.h.settle();
    }

    pub(crate) fn actor(&self) -> Combatant {
        self.h
            .world()
            .get::<Combatant>(self.player)
            .unwrap()
            .clone()
    }

    /// The weapon in `hand`, as the sim holds it.
    pub(crate) fn weapon(&self, hand: usize) -> Record {
        let hands = self.h.world().get::<Hands>(self.player).unwrap();
        hands.saved(&self.registries.items).unwrap()[hand]
            .clone()
            .expect("a weapon in the hand")
    }

    /// The words of a row that is one button: a mount's, a part's.
    pub(crate) fn row(&self, id: &str) -> Vec<String> {
        texts_under(&self.h, find(&self.h, id))
    }

    /// A cell of the fitting table.
    pub(crate) fn figure(&self, property: Property, cell: &str) -> String {
        self.says(&fitting::cell_id(property, cell))
    }

    /// How many of `item` the household's store holds.
    pub(crate) fn stored(&self, item: &str) -> u32 {
        let id = self.id(item);
        self.store()
            .0
            .slots()
            .iter()
            .flatten()
            .filter(|stack| stack.id == id)
            .map(|stack| stack.count)
            .sum()
    }

    /// How many of `item` the pack holds, its bags included.
    pub(crate) fn packed(&self, item: &str) -> u32 {
        let id = self.id(item);
        let pack = self.pack();
        std::iter::once(&pack.cells)
            .chain(&pack.bags)
            .flat_map(|inventory| inventory.slots().iter().flatten())
            .filter(|stack| stack.id == id)
            .map(|stack| stack.count)
            .sum()
    }
}

/// The ticket's done-when, by the mouse: at a workbench the store's
/// magazine is listed with what it would do, pointing at it previews it in
/// the table, a click picks it and Fit fits it. The `Combatant`, which is
/// what the HUD reads, then holds 29, the table's NOW is the fitted row
/// beside the bare one, and the unit came out of the store. Remove puts
/// the row back exactly and the magazine in the pack.
#[test]
#[allow(clippy::too_many_lines)]
fn fitting_the_feed_at_a_workbench_changes_the_magazine_line_and_the_table() {
    use Property::{Capacity, ReloadTime, RepeatInterval};
    let (mut day, bench) = bench_day("fitting-mouse", 1600.0, 900.0);
    let before = day.truth();
    let bare = day.actor().weapon_spec();
    assert_eq!(bare.magazine, 24);
    let rounds = day.actor().ammo;
    day.use_workbench(bench);

    // The weapon in hand, its mounts and their words.
    assert_eq!(day.says(fitting::TITLE_ID), "Vigil patrol carbine");
    assert_eq!(day.says(fitting::OPEN_ID), "4 of 6 open");
    // The mount in focus and the weapon shown are marked, not only lit.
    assert_eq!(
        day.row(&fitting::mount_id("feed")),
        ["\u{203a} FEED", "Standard", "The part it was built with"]
    );
    assert_eq!(
        day.row(&fitting::mount_id("muzzle")),
        ["MUZZLE", "Shut", "Opens at moddability 600"]
    );
    assert_eq!(
        day.row(&fitting::hand_id(0)),
        ["\u{203a} CARBINE · IN HAND"]
    );
    assert_eq!(day.row(&fitting::hand_id(1)), ["PISTOL · HOLSTERED"]);
    assert_eq!(
        day.says(fitting::FITS_ID),
        "FITS THE FEED, FROM THE PACK AND THE STORE"
    );
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "At the workbench: pick a mount, then a part. Enter picks, Enter again fits."
    );
    // The parts, best first, each in units with its change.
    assert_eq!(
        day.row(&fitting::pick_id(fitting::PICK_STANDARD)),
        ["\u{203a} Standard feed", "Fitted now"]
    );
    assert_eq!(
        day.row(&fitting::pick_id("0")),
        [
            "Patrol extended magazine",
            "Batch 412 · Capacity 610 · 2 in store",
            "Magazine 29 rounds",
            "+5"
        ]
    );
    assert_eq!(
        day.row(&fitting::pick_id("1"))[1],
        "Store grade · Capacity 500 · 4 in store",
        "the two read alike in rounds, and say what orders them"
    );
    // The table: now and bare alike, nothing after, and what is not in the
    // fight yet listed under its own heading.
    for (property, figure) in [
        (Capacity, "24 rounds"),
        (ReloadTime, "1.35 s"),
        (RepeatInterval, "0.462 s"),
    ] {
        assert_eq!(day.figure(property, "now"), figure);
        assert_eq!(day.figure(property, "bare"), figure);
        assert_eq!(day.figure(property, "after"), "");
    }
    let later = texts_under(&day.h, find(&day.h, fitting::LATER_ID));
    assert_eq!(later[0], "NOT IN THE FIGHT YET");
    assert!(!later.contains(&"Aimed spread".to_owned()) && later.contains(&"Recoil".to_owned()));
    assert!(button_disabled(&day.h, fitting::FIT));
    assert!(button_disabled(&day.h, fitting::REMOVE));
    assert_eq!(
        day.says(fitting::WHY_ID),
        "Pick a part to fit · Nothing fitted here"
    );
    assert_eq!(
        day.role_of(fitting::WHY_ID),
        "strata.fitting.why.hint",
        "a word on what to do next is not said as a refusal"
    );
    // With nothing to show in it the AFTER column is not there: no
    // heading, no width, and NOW stands beside BARE.
    let column = |day: &Day, of: &str| {
        let id = format!("{}after:{of}", fitting::COLUMN_TAG);
        day.h.rect_of(find(&day.h, &id)).width()
    };
    let gap = |day: &Day| {
        let now = day
            .h
            .rect_of(find(&day.h, &fitting::cell_id(Capacity, "now")));
        let bare = day
            .h
            .rect_of(find(&day.h, &fitting::cell_id(Capacity, "bare")));
        bare.min.x - now.max.x
    };
    assert_eq!(day.says(fitting::AFTER_HEAD_ID), "");
    assert!(
        column(&day, "capacity") < 3.,
        "{}",
        column(&day, "capacity")
    );
    let shut = gap(&day);
    assert!(shut < 70., "NOW is {shut} from BARE");

    // Pointing at the bought magazine previews it before anything is
    // picked: the AFTER column, with the longer reload marked as worse.
    let bought = find(&day.h, &fitting::pick_id("1"));
    day.h.hover(bought);
    day.h.settle();
    assert_eq!(day.says(fitting::AFTER_HEAD_ID), "AFTER");
    assert!(
        column(&day, "capacity") > 50.,
        "{}",
        column(&day, "capacity")
    );
    assert!(column(&day, "head") > 50.);
    assert!(
        gap(&day) > shut + 100.,
        "the column has opened between them"
    );
    assert_eq!(day.figure(Capacity, "after"), "29 rounds");
    assert_eq!(day.figure(Capacity, "change"), "+5");
    assert_eq!(day.figure(ReloadTime, "after"), "1.49 s");
    assert_eq!(day.figure(ReloadTime, "change"), "+0.14 s worse");
    assert_eq!(
        day.role_of(&fitting::cell_id(Capacity, "change")),
        "strata.fitting.better"
    );
    assert_eq!(
        day.role_of(&fitting::cell_id(ReloadTime, "change")),
        "strata.fitting.worse"
    );
    assert_eq!(day.figure(RepeatInterval, "after"), "");
    assert_eq!(day.figure(Capacity, "now"), "24 rounds", "nothing is spent");
    assert!(
        button_disabled(&day.h, fitting::FIT),
        "looked at, not picked"
    );
    // Off it, the column goes back to what is picked: nothing.
    day.h.hover(find(&day.h, fitting::TITLE_ID));
    day.h.settle();
    assert_eq!(day.figure(Capacity, "after"), "");
    assert_eq!(day.says(fitting::AFTER_HEAD_ID), "");
    assert!(column(&day, "capacity") < 3.);

    // A click picks it, and Fit lights.
    let bought = find(&day.h, &fitting::pick_id("1"));
    day.h.click(bought);
    day.h.settle();
    assert_eq!(
        day.row(&fitting::pick_id("1"))[0],
        "\u{203a} Patrol extended magazine",
        "the pick is marked as well as lit"
    );
    assert!(!button_disabled(&day.h, fitting::FIT));
    assert_eq!(day.says(fitting::WHY_ID), "Nothing fitted here");
    assert_eq!(day.figure(Capacity, "after"), "29 rounds");
    assert_eq!(day.says(fitting::AFTER_HEAD_ID), "AFTER");
    assert!(column(&day, "capacity") > 50., "picked, the column stays");
    assert_eq!(day.actor().magazine(), 24, "picked, not fitted");
    // The part's image is in its item's colour, as its cell in the
    // stores is; with nothing baked it is the item's library symbol.
    assert!(
        day.h
            .try_find(&by::test_id(&format!("{}1", fitting::ICON_TAG)))
            .is_some()
    );

    // Fit pressed twice before the sim's tick is one fit: the second
    // press is not sent to be refused over the first.
    let fit = find(&day.h, fitting::FIT);
    for _ in 0..2 {
        day.h.world_mut().write_message(slotted::menu::MenuChoice {
            id: fitting::FIT.to_owned(),
            screen: fitting::kind(),
            entity: fit,
        });
    }
    day.h.step(1);
    assert_eq!(day.h.world().resource::<FitInbox>().len(), 1);
    day.tick();
    let fitted = day.actor();
    assert_eq!(fitted.magazine(), 29, "what the HUD reads");
    assert!((fitted.weapon_spec().reload - 1.485).abs() < 1e-4);
    assert_eq!(fitted.ammo, rounds, "the rounds are kept, not refilled");
    assert_eq!(day.weapon(0).fitted["feed"].item.to_string(), FEED);
    assert_eq!(
        day.stored(FEED),
        5,
        "one of the store's six is on the carbine"
    );
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "Fitted the patrol extended magazine."
    );
    assert_eq!(
        day.role_of(fitting::STATUS_ID),
        "strata.fitting.status",
        "done, not refused"
    );
    // The sim's word has the footer's line, so the keys are said under it.
    assert_eq!(
        day.says(fitting::KEYS_ID),
        "At the workbench: pick a mount, then a part. Enter picks, Enter again fits."
    );
    assert_eq!(
        day.row(&fitting::mount_id("feed")),
        ["\u{203a} FEED", "Extended magazine", "Store grade"]
    );
    assert_eq!(day.figure(Capacity, "now"), "29 rounds");
    assert_eq!(day.figure(Capacity, "bare"), "24 rounds");
    assert_eq!(day.figure(ReloadTime, "now"), "1.49 s");
    assert_eq!(day.figure(ReloadTime, "bare"), "1.35 s");
    assert_eq!(day.figure(Capacity, "after"), "", "nothing more is picked");
    assert_eq!(
        day.row(&fitting::pick_id(fitting::PICK_FITTED)),
        [
            "\u{203a} Patrol extended magazine",
            "Store grade · Capacity 500 · 3 in store · Fitted now",
            "Magazine 29 rounds",
            "+5"
        ]
    );
    assert_eq!(
        day.row(&fitting::pick_id(fitting::PICK_STANDARD))[1],
        "Remove puts it back"
    );
    assert!(!button_disabled(&day.h, fitting::REMOVE));

    // The HUD's own figure, with the screen away.
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    assert_eq!(shown(&day.h, veyra_view::combat_hud::ids::MAGAZINE), "/ 29");

    // Back at the bench: Remove restores the row exactly, and the magazine
    // goes to the pack before the store.
    day.use_workbench(bench);
    assert_eq!(day.figure(Capacity, "now"), "29 rounds");
    let remove = find(&day.h, fitting::REMOVE);
    day.h.click(remove);
    day.tick();
    let restored = day.actor();
    assert_eq!(restored.weapon_spec(), bare, "the row it had, bit for bit");
    assert!(
        restored.loadout().is_some_and(|loadout| loadout
            .mods
            .values()
            .flatten()
            .all(|factor| factor.property == Property::RepeatInterval)),
        "only the bought carbine's own Action line is left"
    );
    assert!(day.weapon(0).fitted.is_empty());
    assert_eq!(day.packed(FEED), 1);
    assert_eq!(day.stored(FEED), 5);
    assert_eq!(day.truth(), before, "nothing was made or lost");
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "Removed. The feed is standard again."
    );
    assert_eq!(day.figure(Capacity, "now"), "24 rounds");
    assert_eq!(
        day.row(&fitting::mount_id("feed")),
        ["\u{203a} FEED", "Standard", "The part it was built with"]
    );
    assert!(button_disabled(&day.h, fitting::REMOVE));

    // The same fit and its echo in one tick, from wherever they came: the
    // fit is what the screen says, not the refusal of the echo.
    let module = day.weapon(0);
    assert!(module.fitted.is_empty());
    let request = {
        let world = day.h.world();
        let state = world.resource::<fitting::FittingState>();
        let weapon = state.view().and_then(|view| view.weapon.as_ref()).unwrap();
        weapon
            .picks
            .iter()
            .find_map(|row| row.request.clone())
            .expect("a part that can be fitted")
    };
    day.h
        .world_mut()
        .resource_mut::<FitInbox>()
        .extend([request.clone(), request]);
    day.tick();
    assert_eq!(day.weapon(0).fitted.len(), 1);
    assert!(
        day.says(fitting::STATUS_ID).starts_with("Fitted the "),
        "{}",
        day.says(fitting::STATUS_ID)
    );
    assert_eq!(day.role_of(fitting::STATUS_ID), "strata.fitting.status");
    let remove = find(&day.h, fitting::REMOVE);
    day.h.click(remove);
    day.tick();
    assert_eq!(day.actor().weapon_spec(), bare);
    assert_eq!(day.truth(), before, "nothing was made or lost");
    day.h.key(ui::pause::PAUSE_KEY);
    day.h.settle();
    assert_eq!(shown(&day.h, veyra_view::combat_hud::ids::MAGAZINE), "/ 24");
}

/// The same with the keys alone, and every control of the screen in their
/// reach. The screen opens with the focus on the feed's row; Tab goes
/// round both hands' tabs, every mount, every part and Close; the focus on
/// the bought magazine previews it, Accept picks it and Fit is then in
/// Tab's round; Accept on Fit fits it, which puts Remove there, and Accept
/// on Remove restores the row. Accept on the other hand's tab shows the
/// pistol. Nothing here is pointed at or focused by the test's own hand.
#[test]
#[allow(clippy::too_many_lines)]
fn fitting_by_focus_alone_fits_and_removes() {
    use Property::Capacity;
    let (mut day, bench) = bench_day("fitting-keys", 1600.0, 900.0);
    let bare = day.actor().weapon_spec();
    day.use_workbench(bench);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &fitting::mount_id("feed"))),
        "the mount in focus has the keyboard when the screen opens"
    );
    let tab_to = |day: &mut Day, id: &str| {
        for _ in 0..40 {
            if day.h.focused() == day.h.try_find(&by::test_id(id)) {
                return;
            }
            day.h.focus_next();
            day.h.settle();
        }
        panic!(
            "Tab did not reach {id}, and is on {}",
            day.named(day.h.focused())
        );
    };
    // Every control Tab stops on in one round of the screen.
    let round = |day: &mut Day| {
        let from = day.h.focused();
        let mut stops = vec![day.named(from)];
        for _ in 0..60 {
            day.h.focus_next();
            day.h.settle();
            if day.h.focused() == from {
                return stops;
            }
            stops.push(day.named(day.h.focused()));
        }
        panic!("Tab does not come round: {stops:?}");
    };
    let reaches = |stops: &[String], id: &str| stops.iter().any(|stop| stop == id);

    let stops = round(&mut day);
    let mut wanted = vec![
        fitting::hand_id(0),
        fitting::hand_id(1),
        fitting::pick_id(fitting::PICK_STANDARD),
        fitting::pick_id("0"),
        fitting::pick_id("1"),
        fitting::CLOSE_ID.to_owned(),
    ];
    wanted.extend(["sight", "feed", "grip", "stock", "muzzle", "fore_end"].map(fitting::mount_id));
    for id in &wanted {
        assert!(reaches(&stops, id), "Tab misses {id}: {stops:?}");
    }
    // Neither button can act yet. Tab stops on a grey one all the same,
    // where the line beside it says why it is grey.
    assert!(button_disabled(&day.h, fitting::FIT));
    assert!(reaches(&stops, fitting::FIT) && reaches(&stops, fitting::REMOVE));

    tab_to(&mut day, &fitting::pick_id("1"));
    assert_eq!(
        day.figure(Capacity, "after"),
        "29 rounds",
        "the focus previews the part"
    );
    assert_eq!(day.figure(Capacity, "change"), "+5");
    assert_eq!(day.says(fitting::AFTER_HEAD_ID), "AFTER");
    assert!(button_disabled(&day.h, fitting::FIT));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &fitting::pick_id("1"))),
        "the pick keeps the keyboard across the rebuild it caused"
    );
    assert!(!button_disabled(&day.h, fitting::FIT));
    assert_eq!(day.actor().magazine(), 24);

    // Picked, Fit is in the round, and Accept on it fits.
    tab_to(&mut day, fitting::FIT);
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.actor().magazine(), 29);
    assert_eq!(day.figure(Capacity, "now"), "29 rounds");
    assert!(
        day.h.focused().is_some(),
        "the keyboard is not left on a button that went grey"
    );

    // Fitted, Remove is in the round and so is the fitted part's row.
    let stops = round(&mut day);
    for id in [
        fitting::REMOVE.to_owned(),
        fitting::pick_id(fitting::PICK_FITTED),
        fitting::CLOSE_ID.to_owned(),
    ] {
        assert!(reaches(&stops, &id), "Tab misses {id}: {stops:?}");
    }
    tab_to(&mut day, fitting::REMOVE);
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.actor().weapon_spec(), bare);
    assert_eq!(day.figure(Capacity, "now"), "24 rounds");
    assert_eq!(day.packed(FEED), 1);

    // Accept on a picked part fits it too, without a walk to the button.
    tab_to(&mut day, &fitting::pick_id("0"));
    day.h.action(UiAction::Accept);
    day.h.settle();
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.actor().magazine(), 29);
    tab_to(&mut day, fitting::REMOVE);
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.actor().weapon_spec(), bare);

    // The other weapon is a tab's Accept away, and back.
    tab_to(&mut day, &fitting::hand_id(1));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.says(fitting::TITLE_ID), "Kestrel service pistol");
    assert_eq!(
        day.row(&fitting::hand_id(1)),
        ["\u{203a} PISTOL · HOLSTERED"]
    );
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &fitting::hand_id(1))),
        "the tab keeps the keyboard"
    );
    assert_eq!(day.figure(Capacity, "now"), "10 rounds");
    tab_to(&mut day, &fitting::hand_id(0));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.says(fitting::TITLE_ID), "Vigil patrol carbine");

    // The arrows walk it too: down the mounts from the one in focus.
    tab_to(&mut day, &fitting::mount_id("feed"));
    day.h.action(UiAction::Down);
    day.h.settle();
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &fitting::mount_id("grip"))),
        "down the mounts"
    );
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(
        day.says(fitting::FITS_ID),
        "FITS THE GRIP, FROM THE PACK AND THE STORE"
    );
    // Close is reached by Tab and closes; Escape does too, as on every
    // screen.
    tab_to(&mut day, fitting::CLOSE_ID);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    day.use_workbench(bench);
    day.h.action(UiAction::Back);
    day.h.settle();
    assert!(day.h.stack().is_empty());
}

/// Away from a workbench the screen is a reading: everything is shown, the
/// store is out of reach, and Fit and Remove are grey with the reason
/// beside them. A request that reached the sim all the same is refused
/// there, with the same words.
#[test]
fn away_from_a_workbench_fit_says_why() {
    use Property::Capacity;
    let mut day = first_day("fitting-away", 1600.0, 900.0);
    let mut commands = day.h.world_mut().commands();
    fitting::open(&mut commands, Some(1));
    day.h.world_mut().flush();
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(day.says(fitting::TITLE_ID), "Kestrel service pistol");
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "A reading. Fitting is done at a workbench."
    );
    assert_eq!(day.says(fitting::FITS_ID), "FITS THE FEED, FROM THE PACK");
    assert_eq!(day.figure(Capacity, "now"), "10 rounds");
    // The cell in the satchel is listed and previews; it cannot be fitted.
    assert_eq!(
        day.row(&fitting::pick_id("0")),
        [
            "Pistol extended cell",
            "Store grade · Capacity 500 · 1 carried",
            "Magazine 13 rounds",
            "+3"
        ]
    );
    assert!(button_disabled(&day.h, fitting::FIT));
    assert!(button_disabled(&day.h, fitting::REMOVE));
    assert_eq!(day.says(fitting::WHY_ID), "Fitting is done at a workbench");
    assert_eq!(
        day.role_of(fitting::WHY_ID),
        "strata.fitting.why",
        "a reason the buttons cannot act, in the refusal's colour"
    );
    let cell = find(&day.h, &fitting::pick_id("0"));
    day.h.click(cell);
    day.h.settle();
    assert_eq!(day.figure(Capacity, "after"), "13 rounds");
    assert!(
        button_disabled(&day.h, fitting::FIT),
        "picked, and still grey"
    );
    assert_eq!(day.says(fitting::WHY_ID), "Fitting is done at a workbench");
    // Accept again on the picked part asks for nothing.
    let cell = find(&day.h, &fitting::pick_id("0"));
    day.h.click(cell);
    day.tick();
    assert_eq!(
        day.actor()
            .spec_of(veyra_sim::combat::Weapon::Pistol)
            .magazine,
        10
    );
    assert!(day.weapon(1).fitted.is_empty());

    // The sim's own rule, for a request from anywhere.
    let module = day
        .registries
        .items
        .starting()
        .unwrap()
        .packs
        .get("Frontier ranger")
        .and_then(|pack| pack.bags.iter().flatten().flatten().next().cloned());
    let module = module.expect("the cell in the satchel");
    day.h
        .world_mut()
        .resource_mut::<FitInbox>()
        .push(FitRequest::Fit {
            hand: 1,
            mount: "feed".to_owned(),
            module: strata_registry::item::Fitted {
                item: module.item,
                batch: module.batch,
                by: module.by,
                lines: module.lines,
                condition: module.condition,
            },
        });
    day.tick();
    assert!(day.weapon(1).fitted.is_empty());
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "Fitting is done at a workbench"
    );
    assert_eq!(
        day.role_of(fitting::STATUS_ID),
        "strata.fitting.status.refused"
    );
    assert_eq!(day.says(fitting::WHY_ID), "Fitting is done at a workbench");

    // The other weapon is a tab away.
    let carbine = find(&day.h, &fitting::hand_id(0));
    day.h.click(carbine);
    day.h.settle();
    assert_eq!(day.says(fitting::TITLE_ID), "Vigil patrol carbine");
    assert_eq!(
        day.says(fitting::NONE_ID),
        "No feed mod in the pack.",
        "the store's magazines are not in reach"
    );
}

/// A shut mount keeps its standard part, takes nothing and says when it
/// opens at the mount itself; in focus, both buttons are grey for the same
/// reason. The drawing brackets the mount in focus and follows a pick on
/// it, and a fitted mount draws its module's baked piece.
#[test]
fn a_shut_mount_says_when_it_opens_and_a_fitted_one_draws_its_piece() {
    use Property::EquipTime;
    let (mut day, bench) = bench_day("fitting-shut", 1600.0, 900.0);
    day.h.world_mut().insert_resource(carbine_pictures());
    day.use_workbench(bench);
    let root = find(&day.h, diagram::DIAGRAM_ID);
    assert!(day.h.world().get::<Diagram>(root).unwrap().pictured);
    // The hand-made index hides the standard feed, so a marker stands for
    // it, and the bracket goes round where the part is.
    assert_eq!(
        day.h.find_all(&by::test_id(diagram::BRACKET_ID)).len(),
        1,
        "the mount in focus is bracketed as the screen opens"
    );

    // A click on the muzzle's row.
    let muzzle = find(&day.h, &fitting::mount_id("muzzle"));
    day.h.click(muzzle);
    day.h.settle();
    assert_eq!(
        day.row(&fitting::mount_id("muzzle")),
        ["\u{203a} MUZZLE", "Shut", "Opens at moddability 600"]
    );
    assert_eq!(day.says(fitting::WHY_ID), "Opens at moddability 600");
    assert!(button_disabled(&day.h, fitting::FIT));
    assert!(button_disabled(&day.h, fitting::REMOVE));
    assert_eq!(
        day.says(fitting::NONE_ID),
        "No muzzle mod in the pack or the store."
    );
    assert_eq!(
        day.says(fitting::THRESHOLDS_ID),
        "A mount opens at 400, 600 and 800."
    );
    let ticks: Vec<u16> = {
        let world = day.h.world_mut();
        let mut ticks = world.query::<&card::BarTick>();
        let mut all: Vec<u16> = ticks.iter(world).map(|tick| tick.0).collect();
        all.sort_unstable();
        all
    };
    assert_eq!(ticks, [400, 600, 800], "a tick where each mount opens");
    let bracket = day.h.rect_of(find(&day.h, diagram::BRACKET_ID));
    let part = day.h.rect_of(mount_button(&day.h, "muzzle"));
    assert!(
        (bracket.center() - part.center()).abs().max_element() <= 1.,
        "the bracket is round the mount in focus"
    );

    // A pick on the drawing is a mount in focus too.
    let sight = mount_button(&day.h, "sight");
    day.h.click(sight);
    day.h.settle();
    assert_eq!(picks(&mut day.h), ["sight"]);
    assert_eq!(
        day.says(fitting::FITS_ID),
        "FITS THE SIGHT, FROM THE PACK AND THE STORE"
    );
    assert!(drawn(&day.h).iter().any(|stem| stem == "sight.standard"));
    assert_eq!(
        day.row(&fitting::pick_id("0")),
        [
            "Vigil reflex sight",
            "Store grade · Spread 500 · 1 in store",
            "Aimed spread 1.40°",
            "−0.60°"
        ]
    );
    let reflex = find(&day.h, &fitting::pick_id("0"));
    day.h.click(reflex);
    day.h.settle();
    assert_eq!(day.figure(EquipTime, "after"), "0.46 s");
    assert_eq!(day.figure(EquipTime, "change"), "+0.01 s worse");
    let fit = find(&day.h, fitting::FIT);
    day.h.click(fit);
    day.tick();
    assert_eq!(day.weapon(0).fitted["sight"].item.to_string(), REFLEX);
    assert!(
        drawn(&day.h)
            .iter()
            .any(|stem| stem == "sight.vigil_reflex"),
        "the fitted module's own piece: {:?}",
        drawn(&day.h)
    );
    assert!(!drawn(&day.h).iter().any(|stem| stem == "sight.standard"));
    assert_eq!(day.figure(EquipTime, "now"), "0.46 s");
    assert_eq!(day.figure(EquipTime, "bare"), "0.45 s");
    let bracket = day.h.rect_of(find(&day.h, diagram::BRACKET_ID));
    let part = day.h.rect_of(mount_button(&day.h, "sight"));
    assert!((bracket.center() - part.center()).abs().max_element() <= 1.);
    assert!(
        bracket.width() >= diagram::BRACKET_MIN - 0.5
            && bracket.height() >= diagram::BRACKET_MIN - 0.5
    );
}

/// A weapon's cell on the stores screen opens its fitting, by a click and
/// by Accept, for the hand it is in; Back returns to the stores. With no
/// workbench near it is the reading.
#[test]
fn a_hands_cell_opens_fitting() {
    let mut day = first_day("stores-hand", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    let pistol = find(&day.h, &stores::hand_slot_id(1));
    day.h.click(pistol);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(day.says(fitting::TITLE_ID), "Kestrel service pistol");
    assert_eq!(
        day.says(fitting::STATUS_ID),
        "A reading. Fitting is done at a workbench."
    );
    assert_eq!(day.carried(), None, "the weapon was not taken up");
    day.h.action(UiAction::Back);
    day.h.settle();
    assert_eq!(
        day.h.stack_top(),
        Some(stores::kind()),
        "back to the stores"
    );
    assert!(day.h.world().resource::<StoresState>().menu().is_some());

    let carbine = find(&day.h, &stores::hand_slot_id(0));
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.set_focus(Some(carbine));
    day.h.settle();
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(day.says(fitting::TITLE_ID), "Vigil patrol carbine");
    day.h.action(UiAction::Back);
    day.h.settle();
    // A stack in the hand is put down on nothing: a weapon's cell takes no
    // stack, and the click is not the way into its fitting.
    let store0 = day.cell("store", 0);
    day.h.click(store0);
    day.h.settle();
    assert!(day.carried().is_some());
    day.h.click(carbine);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    assert!(day.carried().is_some());
    day.h.click(store0);
    day.h.settle();
    day.close();
    assert_eq!(day.truth(), before);
}

/// T sends the stack the player is on: the focused cell's with the keys,
/// the one under the pointer with the mouse, whatever card the pane has
/// pinned on the way there. The hint line names the key in both forms.
#[test]
fn t_sends_the_focused_stack() {
    let mut day = first_day("stores-t", 1600.0, 900.0);
    let before = day.truth();
    day.use_locker();
    day.h.set_input_mode(InputMode::Keyboard);
    let (feed, cell) = (day.id(FEED), day.id(CELL));
    let (store0, store1, store2) = (
        day.cell("store", 0),
        day.cell("store", 1),
        day.cell("store", 2),
    );
    let pack0 = day.cell("pack", 0);
    assert!(
        day.says(stores::STATUS_ID).starts_with("T or Shift click"),
        "{}",
        day.says(stores::STATUS_ID)
    );
    assert_eq!(day.h.focused(), Some(store0));
    // Two cells along, across the crafted batch: the pin has followed, and
    // the key sends the cells the focus is on, not a stack met on the way.
    day.h.action(UiAction::Right);
    day.h.action(UiAction::Right);
    day.h.settle();
    assert_eq!(day.h.focused(), Some(store2));
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(store2), None);
    assert_eq!(day.shows(store0), Some((feed, 4)), "the first stack stayed");
    assert_eq!(day.shows(store1), Some((feed, 2)), "and the one crossed");
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    assert_eq!(
        day.shows(bag_cell),
        Some((cell, 4)),
        "onto its like, carried"
    );
    assert_eq!(day.pack().bags[0].get(0).map(|stack| stack.count), Some(4));
    // On an empty cell the key does nothing.
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(store0), Some((feed, 4)));

    // The send button says which stack it will send, since the pin moves
    // with the focus on the way to it.
    day.h.set_focus(Some(store1));
    day.h.settle();
    assert_eq!(
        day.says(stores::SENDS_ID),
        "Sends Patrol extended magazine x2"
    );

    // With the pointer in use and on no cell the key sends nothing: the
    // stack the focus was left on is not the one the player means.
    day.h.set_input_mode(InputMode::Pointer);
    day.h.hover(find(&day.h, stores::STATUS_ID));
    day.h.settle();
    assert_eq!(day.h.focused(), Some(store1));
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(store1), Some((feed, 2)), "nothing was pointed at");
    assert_eq!(day.shows(pack0), None);

    // With the pointer, the stack under it, wherever the focus was left.
    day.h.hover(store0);
    day.h.settle();
    assert_eq!(day.h.focused(), Some(store1));
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(store0), None);
    assert_eq!(day.shows(pack0), Some((feed, 4)));
    day.h.hover(pack0);
    day.h.settle();
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(pack0), None);
    assert_eq!(day.shows(store0), Some((feed, 4)), "and back");
    assert_eq!(
        day.h
            .world()
            .resource::<stores::StockAuthority>()
            .in_flight(),
        0
    );
    day.close();
    assert_eq!(day.truth(), before);

    // Without the store the key sends between the pack and a bag, and the
    // hint says so.
    day.h.key(stores::KEY);
    day.h.settle();
    assert!(
        day.says(stores::STATUS_ID)
            .starts_with("T or Shift click sends between"),
        "{}",
        day.says(stores::STATUS_ID)
    );
    let bag_cell = day.cell(&stores::bag_region(0), 0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.set_focus(Some(bag_cell));
    day.h.settle();
    day.h.key(stores::SEND_KEY);
    day.h.settle();
    assert_eq!(day.shows(bag_cell), None);
    assert_eq!(day.shows(day.cell("pack", 0)), Some((cell, 4)));
}

/// The fitting screen fits a small window with its buttons and its Close
/// inside the panel, for a long weapon and for one as tall as it is wide,
/// whose drawing narrows and does not push the parts out of reach.
#[test]
fn the_fitting_screen_fits_small_windows() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0), (1000.0, 560.0)] {
        for hand in [0, 1] {
            let (mut day, _) = bench_day("fitting-small", width, height);
            let mut pictures = carbine_pictures();
            pictures.0.chassis.extend(odd_pictures().0.chassis);
            day.h.world_mut().insert_resource(pictures);
            let mut commands = day.h.world_mut().commands();
            fitting::open(&mut commands, Some(hand));
            day.h.world_mut().flush();
            day.h.settle();
            assert_eq!(day.h.stack_top(), Some(fitting::kind()));
            let window = Rect::new(0., 0., width, height);
            let root = day.h.rect_of(find(&day.h, fitting::ROOT_ID));
            assert!(within(root, window), "{root:?} spills {window:?}");
            for id in [fitting::CLOSE_ID, fitting::STATUS_ID, fitting::TITLE_ID] {
                let rect = day.h.rect_of(find(&day.h, id));
                assert!(within(rect, root), "{id}: {rect:?} outside {root:?}");
            }
            if (width, height) != (1000.0, 560.0) {
                // The columns are whole at the game's sizes; smaller, they
                // scroll inside the panel.
                for id in [
                    fitting::FIT.to_owned(),
                    fitting::REMOVE.to_owned(),
                    diagram::DIAGRAM_ID.to_owned(),
                    fitting::MODDABILITY_ID.to_owned(),
                    fitting::mount_id("feed"),
                    fitting::pick_id(fitting::PICK_STANDARD),
                    fitting::REWORK.to_owned(),
                    fitting::BUY_BLANK.to_owned(),
                    fitting::REWORK_WHY_ID.to_owned(),
                ] {
                    let rect = day.h.rect_of(find(&day.h, &id));
                    assert!(within(rect, root), "{id}: {rect:?} outside {root:?}");
                }
                let well = day.h.rect_of(find(&day.h, diagram::DIAGRAM_ID));
                let stage = day.h.rect_of(stage(&day.h));
                assert!(within(stage, well), "{stage:?} outside {well:?}");
                assert!(
                    well.height() <= 0.3 * height + 1.,
                    "the drawing keeps to its share: {well:?}"
                );
                assert!(
                    well.width() < stage.width() + 8.,
                    "and its well to the weapon: {well:?} round {stage:?}"
                );
                // No row of the table is taller than a line or two.
                let row = day
                    .h
                    .rect_of(find(&day.h, &format!("{}capacity", fitting::ROW_TAG)));
                assert!(row.height() < 40., "{row:?}");
                let mount = day.h.rect_of(find(&day.h, &fitting::mount_id("feed")));
                assert!(mount.height() < 70., "{mount:?}");
            }
        }
    }
}

/// The panel is one size and the buttons stand where they stood whatever
/// the screen shows: the carbine's feed, its muzzle (a short list against a
/// long one) and then the pistol, whose well is a different shape.
#[test]
fn the_fitting_screen_does_not_move_with_the_mount_or_the_weapon() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0)] {
        let (mut day, _) = bench_day("fitting-fixed", width, height);
        let mut pictures = carbine_pictures();
        pictures.0.chassis.extend(odd_pictures().0.chassis);
        day.h.world_mut().insert_resource(pictures);
        let mut commands = day.h.world_mut().commands();
        fitting::open(&mut commands, Some(0));
        day.h.world_mut().flush();
        day.h.settle();
        let rects = |day: &Day| {
            [
                fitting::ROOT_ID,
                fitting::FIT,
                fitting::REMOVE,
                fitting::CLOSE_ID,
                diagram::DIAGRAM_ID,
            ]
            .map(|id| {
                let rect = day.h.rect_of(find(&day.h, id));
                if id == diagram::DIAGRAM_ID {
                    // A taller weapon is a narrower well; its height holds.
                    (rect.min.y, rect.height(), 0.0, 0.0)
                } else {
                    (rect.min.x, rect.min.y, rect.width(), rect.height())
                }
            })
        };
        let feed = rects(&day);
        for key in ["muzzle", "sight", "stock"] {
            let mount = find(&day.h, &fitting::mount_id(key));
            day.h.set_focus(Some(mount));
            day.h.settle();
            day.h.advance(std::time::Duration::from_millis(120));
            day.h.settle();
            assert_eq!(rects(&day), feed, "{width}x{height} mount {key}");
        }
        let mut commands = day.h.world_mut().commands();
        fitting::open(&mut commands, Some(1));
        day.h.world_mut().flush();
        day.h.settle();
        assert_eq!(rects(&day), feed, "{width}x{height} the other hand");
    }
}

/// A hand-made index of the carbine's pieces, so the screen finds a model
/// to draw with no bake and no renderer.
fn carbine_models() -> veyra_ui::weapon_model::WeaponModels {
    use strata_registry::models::{ChassisModels, ModelPiece, ModelSet};
    let piece = |mount: Option<&str>, module: Option<&str>| ModelPiece {
        mount: mount.map(str::to_owned),
        module: module.map(str::to_owned),
        file: format!(
            "weapons/veyra/patrol_carbine/{}.{}.ashprop",
            mount.unwrap_or("chassis"),
            module.unwrap_or("standard")
        ),
        tip: None,
    };
    let mut pieces = vec![piece(None, None)];
    pieces.extend(
        ["sight", "feed", "fore_end", "grip", "muzzle", "stock"]
            .map(|mount| piece(Some(mount), None)),
    );
    pieces.push(piece(Some("sight"), Some("vigil_reflex")));
    let mut set = ModelSet::default();
    set.chassis.insert(
        "patrol_carbine".to_owned(),
        ChassisModels {
            muzzle: [0.0, 0.07, 0.4],
            pieces,
        },
    );
    veyra_ui::weapon_model::WeaponModels(set)
}

/// What the stage is asked to show, by the files of its pieces.
fn on_stage(day: &Day) -> Option<Vec<String>> {
    day.h
        .world()
        .resource::<ui::weapon_stage::WeaponStage>()
        .wanted()
        .map(|wanted| wanted.files.clone())
}

/// With a stage and baked pieces the well is the model widget and the
/// stage is asked for the weapon as it is, or as the part under focus
/// would make it; a switch in the well goes to the diagram and back, by
/// click and by focus; and the stage is let go on every way out. That the
/// camera draws any of it is a still's to show.
#[test]
fn the_well_draws_the_model_on_a_stage_and_the_switch_goes_both_ways() {
    let (mut day, bench) = bench_day("fitting-model", 1600.0, 900.0);
    day.h.world_mut().insert_resource(carbine_models());
    // The harness has an asset server and no ashlar plugin, so the two
    // asset types a piece names are made known for its handles to be made.
    day.h.app.init_asset::<ashlar_bevy::prop::PropAsset>();
    day.h
        .app
        .init_asset::<ashlar_bevy::prelude::MaterialLibraryAsset>();
    day.h
        .world_mut()
        .insert_resource(ui::weapon_stage::WeaponStage::new(Handle::default()));
    day.use_workbench(bench);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    let model = |day: &Day| day.h.try_find(&by::test_id(ui::weapon_stage::MODEL_ID));
    let diagram_up = |day: &Day| day.h.try_find(&by::test_id(diagram::DIAGRAM_ID));
    assert!(model(&day).is_some() && diagram_up(&day).is_none());
    let chassis = "weapons/veyra/patrol_carbine/chassis.standard.ashprop".to_owned();
    let standard_sight = "weapons/veyra/patrol_carbine/sight.standard.ashprop".to_owned();
    let reflex_sight = "weapons/veyra/patrol_carbine/sight.vigil_reflex.ashprop".to_owned();
    let files = on_stage(&day).expect("asked for on open");
    assert_eq!(files[0], chassis);
    assert!(files.contains(&standard_sight) && !files.contains(&reflex_sight));

    // The mount list is the way to a mount; a candidate under focus is on
    // the model before Fit.
    day.h.click(find(&day.h, &fitting::mount_id("sight")));
    day.h.settle();
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    let candidate = find(&day.h, &fitting::pick_id("0"));
    day.h.set_focus(Some(candidate));
    day.h.settle();
    let files = on_stage(&day).expect("still asked for");
    assert!(
        files.contains(&reflex_sight) && !files.contains(&standard_sight),
        "the part under focus is on the model: {files:?}"
    );
    assert_eq!(day.weapon(0).fitted.len(), 0, "and nothing is fitted");
    day.h.set_focus(Some(find(
        &day.h,
        &fitting::pick_id(fitting::PICK_STANDARD),
    )));
    day.h.settle();
    assert!(on_stage(&day).unwrap().contains(&standard_sight));

    // The switch, by click.
    day.h.click(find(&day.h, fitting::WELL_SWITCH));
    day.h.settle();
    assert!(model(&day).is_none() && diagram_up(&day).is_some());
    assert_eq!(on_stage(&day), None, "the diagram needs no stage");
    day.h.click(find(&day.h, fitting::WELL_SWITCH));
    day.h.settle();
    assert!(model(&day).is_some() && diagram_up(&day).is_none());
    assert!(on_stage(&day).is_some());

    // And by focus: Tab to the switch and Accept.
    let switch = find(&day.h, fitting::WELL_SWITCH);
    day.h.set_focus(Some(switch));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert!(model(&day).is_none() && diagram_up(&day).is_some());
    day.h.set_focus(Some(find(&day.h, fitting::WELL_SWITCH)));
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert!(model(&day).is_some() && on_stage(&day).is_some());

    // Let go on Close, and on a tab switch to Crafting.
    day.h.action(UiAction::Back);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    assert_eq!(on_stage(&day), None, "released on close");
    day.use_workbench(bench);
    assert!(on_stage(&day).is_some(), "asked for again on open");
    day.h
        .click(find(&day.h, &ui::crafting::WorkbenchTab::Crafting.tab_id()));
    day.h.settle();
    assert_ne!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(on_stage(&day), None, "released on a tab switch");
}
