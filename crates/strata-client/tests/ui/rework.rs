//! Rework on the fitting screen (ticket 0007, step 9): Rework and Buy blank
//! send the sim's requests, and the screen shows what the truth became.

use std::collections::BTreeMap;

use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted_test::prelude::*;
use strata_client::ui::fitting;
use strata_registry::item::Record;
use strata_sim::deposits::Purse;
use strata_sim::items::{self, HouseholdStore};

use crate::common::*;
use crate::fitting::bench_day;

const BLANK: &str = "veyra:component.mount_blank";

impl Day {
    /// The household's store with a blank of `temper` added to its first
    /// free cell, and the purse at 1000.
    fn blank_in_store(&mut self, temper: u16) {
        let registries = self.registries.clone();
        let record = Record {
            item: slotted::model::Namespaced::parse(BLANK).unwrap(),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::from([("temper".to_owned(), temper)]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        let stack = items::write(&record, &registries.items).unwrap();
        let mut store = self.h.world_mut().resource_mut::<HouseholdStore>();
        let cell = (0..store.0.len())
            .find(|cell| store.0.get(*cell).is_none())
            .expect("a free cell");
        store.0.set(cell, Some(stack));
        store.0.clear_changed();
        self.h.world_mut().insert_resource(Purse(1000));
        self.h.step(2);
        self.h.settle();
    }

    /// Tab to the button `id`, as a player on the keys would.
    fn focus_button(&mut self, id: &str) {
        for _ in 0..80 {
            if self.h.focused() == self.h.try_find(&by::test_id(id)) {
                return;
            }
            self.h.focus_next();
            self.h.settle();
        }
        panic!("Tab never reached {id}");
    }
}

/// Rework by the keys: the weapon's moddability rises toward the blank, the
/// blank leaves the store, and the screen reads the new figure.
#[test]
fn rework_raises_the_moddability_and_the_screen_shows_it() {
    let (mut day, bench) = bench_day("rework-keys", 1600.0, 900.0);
    day.blank_in_store(700);
    day.use_workbench(bench);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    assert_eq!(
        day.says(fitting::REWORK_WHY_ID),
        "Mount blank, temper 700. Most often to 650, between 550 and 700."
    );
    assert!(!button_disabled(&day.h, fitting::REWORK));
    let blanks = day.stored(BLANK);
    day.focus_button(fitting::REWORK);
    day.h.action(UiAction::Accept);
    day.tick();
    let now = day.weapon(0).lines["moddability"];
    assert!((550..=700).contains(&now), "moddability {now}");
    let said = day.says(fitting::STATUS_ID);
    assert!(
        said.starts_with(&format!("Reworked: moddability 500 to {now}.")),
        "{said}"
    );
    assert_eq!(
        said.contains("mount is open"),
        now >= 600,
        "a mount that opened is said: {said}"
    );
    assert_eq!(day.stored(BLANK), blanks - 1, "one blank is spent");
    assert_eq!(
        day.says(fitting::REWORK_WHY_ID),
        "No mount blank to hand.",
        "the view is rebuilt from the truth"
    );
    assert!(button_disabled(&day.h, fitting::REWORK));
}

/// Buy blank spends the price and puts a store-grade blank in the pack.
#[test]
fn buy_blank_pays_and_the_blank_arrives() {
    let (mut day, bench) = bench_day("rework-buy", 1600.0, 900.0);
    day.h.world_mut().insert_resource(Purse(1000));
    day.use_workbench(bench);
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    assert_eq!(day.says(fitting::REWORK_WHY_ID), "No mount blank to hand.");
    assert!(!button_disabled(&day.h, fitting::BUY_BLANK));
    day.focus_button(fitting::BUY_BLANK);
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.h.world().resource::<Purse>().0, 850);
    assert_eq!(day.packed(BLANK) + day.stored(BLANK), 1);
    assert_eq!(
        day.says(fitting::REWORK_WHY_ID),
        "This blank is no harder than the mounts: temper 500, moddability 500."
    );
}

/// With nothing to pay with, Buy blank is grey and says why in words.
#[test]
fn a_grey_buy_blank_says_the_reason() {
    let (mut day, bench) = bench_day("rework-poor", 1600.0, 900.0);
    day.h.world_mut().insert_resource(Purse(0));
    day.use_workbench(bench);
    day.h.settle();
    assert!(button_disabled(&day.h, fitting::BUY_BLANK));
    let said = day.says(fitting::REWORK_WHY_ID);
    assert!(said.starts_with("No mount blank to hand."), "{said}");
    assert!(said.contains("150"), "the price is the reason: {said}");
}
