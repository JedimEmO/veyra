//! Wear and repair (ticket 0007, step 6): Repair on the fitting screen, the
//! condition on the HUD, and the kit's card.

use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted_test::prelude::*;
use strata_client::combat_hud::ids;
use strata_client::ui::card::CardParams;
use strata_client::ui::fitting;
use strata_sim::items::Hands;

use crate::common::*;
use crate::fitting::bench_day;

const KIT: &str = "veyra:kit.field_repair";

impl Day {
    /// The weapon in `hand` worn down to `condition`.
    fn wear_to(&mut self, hand: usize, condition: u16) {
        let registries = self.registries.clone();
        let mut record = self.weapon(hand);
        record.condition = Some(condition);
        let stack = strata_sim::items::write(&record, &registries.items).unwrap();
        let mut hands = self.h.world_mut().get_mut::<Hands>(self.player).unwrap();
        hands.0.set(hand, Some(stack));
        hands.0.clear_changed();
        self.h.step(2);
        self.h.settle();
    }
}

/// At a workbench, by the keys alone: Tab reaches Repair, Accept sends it,
/// and the condition reads full with one kit gone from the store.
#[test]
fn repair_by_keys_at_a_workbench_restores_the_weapon() {
    let (mut day, bench) = bench_day("wear-repair-keys", 1600.0, 900.0);
    day.wear_to(0, 180);
    let kits = day.stored(KIT);
    day.use_workbench(bench);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    assert_eq!(day.says(fitting::CONDITION_ID), "180 of 1000");
    assert_eq!(day.says(fitting::TIRED_ID), "WORN");
    assert_eq!(
        day.says(fitting::WORN_ID),
        "Worn. Its lines are sliding until it is repaired."
    );
    assert!(!button_disabled(&day.h, fitting::REPAIR));
    for _ in 0..60 {
        if day.h.focused() == day.h.try_find(&by::test_id(fitting::REPAIR)) {
            break;
        }
        day.h.focus_next();
        day.h.settle();
    }
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, fitting::REPAIR)),
        "Tab reaches Repair"
    );
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.weapon(0).condition, Some(1000));
    assert_eq!(day.says(fitting::CONDITION_ID), "1000 of 1000");
    assert_eq!(day.stored(KIT), kits - 1, "one kit is spent");
    assert!(day.h.try_find(&by::test_id(fitting::WORN_ID)).is_none());
    assert!(
        button_disabled(&day.h, fitting::REPAIR),
        "nothing left to mend"
    );
    assert_eq!(day.says(fitting::REPAIR_WHY_ID), "Nothing to repair.");
}

/// With no kit anywhere Repair is grey and the line beside it says why.
#[test]
fn repair_with_no_kit_is_grey_and_says_why() {
    let (mut day, bench) = bench_day("wear-no-kit", 1600.0, 900.0);
    day.wear_to(0, 400);
    let kit = day.id(KIT);
    {
        let mut store = day
            .h
            .world_mut()
            .resource_mut::<strata_sim::items::HouseholdStore>();
        for cell in 0..store.0.len() {
            if store.0.get(cell).is_some_and(|stack| stack.id == kit) {
                store.0.set(cell, None);
            }
        }
        store.0.clear_changed();
    }
    day.use_workbench(bench);
    assert!(button_disabled(&day.h, fitting::REPAIR));
    assert_eq!(
        day.says(fitting::REPAIR_WHY_ID),
        "No repair kit carried or in the store."
    );
}

/// The vitals cluster: no bar while the weapon is sound, a bar and a chip
/// saying TIRED once tired, and WORN once its lines slide.
#[test]
fn the_hud_shows_the_bar_when_tired_and_a_chip_when_worn() {
    let mut day = first_day("wear-hud", 1600.0, 900.0);
    let shown = |day: &Day, id: &str| day.laid_out(find(&day.h, id));
    day.wear_to(0, 600);
    assert!(!shown(&day, ids::WEAR_TRACK), "no bar at 600");
    day.wear_to(0, 400);
    assert!(shown(&day, ids::WEAR_TRACK), "a bar at 400");
    assert!(shown(&day, ids::WEAR_CHIP), "named, so the bar is not bare");
    assert_eq!(day.says(ids::WEAR_WORD), "CARBINE TIRED");
    day.wear_to(0, 200);
    assert!(shown(&day, ids::WEAR_CHIP), "a chip at 200");
    assert_eq!(day.says(ids::WEAR_WORD), "CARBINE WORN");
}

/// A kit's card says what it restores in the field and at a bench.
#[test]
fn a_kits_card_reads_its_field_amount() {
    let day = first_day("wear-kit-card", 1600.0, 900.0);
    let items = &day.registries.items;
    let kit = items
        .starting()
        .unwrap()
        .store
        .iter()
        .find(|record| record.item.to_string() == KIT)
        .expect("a kit in the store")
        .clone();
    let card = CardParams::of(&kit, &day.registries, true, None).expect("a card");
    assert!(
        card.notes
            .iter()
            .any(|note| note == "Restores 700 points in the field, everything at a workbench."),
        "{:?}",
        card.notes
    );
}
