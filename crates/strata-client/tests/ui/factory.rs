//! Blueprints, the fabricator and the record state (ticket 0007, step 5).

use bevy::math::Rect;
use bevy::prelude::*;
use slotted::model::ItemId;
use slotted::ui::InputMode;
use slotted::ui::SliderState;
use slotted::ui::UiAction;
use slotted::ui::def::UiNodeDef;
use slotted_test::prelude::*;
use strata_client::ui;
use strata_client::ui::card::CardContext;
use strata_client::ui::card::CardParams;
use strata_client::ui::crafting;
use strata_client::ui::factory;
use strata_client::ui::stores;
use strata_registry::item::Record;
use strata_sim::WorldPosition;
use strata_sim::blueprint::Blueprints;
use strata_sim::crafting::CraftInbox;
use strata_sim::crafting::CraftRequest;
use strata_sim::factory::Benched;
use strata_sim::factory::FactoryInbox;
use strata_sim::factory::FactoryRequest;
use strata_sim::factory::FactoryState;
use strata_sim::factory::Job;
use strata_sim::items::HouseholdStore;
use strata_sim::progression::CharacterSheet;
use strata_sim::progression::Interact;
use strata_sim::progression::Interaction;
use strata_sim::progression::InteractionKind;

use crate::common::*;
use crate::crafting::{BRINE, CORE_ITEM, SALT, craft_day, lot_of};

pub(crate) const BLUEPRINT: &str = "veyra:blueprint.production";

/// [`craft_day`] with a fabricator within reach past the workbench.
pub(crate) fn factory_day(name: &str, width: f32, height: f32) -> (Day, Entity, Entity) {
    let (mut day, bench) = craft_day(name, width, height);
    let fabricator = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(2.5, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Fabricator,
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
            .resource::<strata_sim::progression::Reach>()
            .fabricator
    );
    (day, bench, fabricator)
}

impl Day {
    /// A cell core made at the bench by the keys and recorded as a
    /// blueprint at `derate` per-mille by the request the Record box
    /// sends, with the screen shut after: the core is batch 1 and the
    /// blueprint number 2, both in the pack.
    pub(crate) fn recorded_core(&mut self, bench: Entity, derate: u16) -> Record {
        self.h.set_input_mode(InputMode::Keyboard);
        self.core_by_keys(bench);
        self.h
            .world_mut()
            .resource_mut::<CraftInbox>()
            .0
            .push(CraftRequest::Record { batch: 1, derate });
        self.tick();
        assert_eq!(self.prints().0.keys().copied().collect::<Vec<_>>(), [2]);
        self.h.action(UiAction::Back);
        self.h.settle();
        assert!(self.h.stack().is_empty());
        self.carried_of(CORE_ITEM)[0].clone()
    }

    /// F at the fabricator, as the keyboard and the game's script raise it.
    pub(crate) fn use_fabricator(&mut self, fabricator: Entity) {
        self.h.world_mut().write_message(Interact {
            entity: fabricator,
            kind: InteractionKind::Fabricator,
        });
        self.h.step(2);
        self.h.settle();
        assert_eq!(self.h.stack_top(), Some(factory::kind()));
    }

    pub(crate) fn prints(&self) -> Blueprints {
        self.h.world().resource::<Blueprints>().clone()
    }

    pub(crate) fn job(&self) -> Option<Job> {
        self.h.world().resource::<FactoryState>().job.clone()
    }

    /// What the factory screen holds for the slot `slot`, by the roots of
    /// the lots picked to feed it.
    pub(crate) fn feeding(&self, slot: &str) -> Vec<String> {
        self.h
            .world()
            .resource::<factory::FactoryUi>()
            .feeds
            .get(slot)
            .into_iter()
            .flatten()
            .map(|kind| {
                kind.origin
                    .as_ref()
                    .map_or_else(|| kind.item.to_string(), |origin| origin.root.clone())
            })
            .collect()
    }

    /// Starts a run of blueprint 2 for `batch` from the cheapest stock by
    /// the request the screen sends, worked by `worker`.
    pub(crate) fn start_run(&mut self, batch: u32, worker: Option<&str>) {
        let registries = self.registries.clone();
        let (pack, store, prints) = (self.pack(), self.store(), self.prints());
        let feeds =
            strata_sim::factory::cheapest(&registries, &pack, Some(&store), &prints.0[&2], batch);
        self.h
            .world_mut()
            .resource_mut::<FactoryInbox>()
            .0
            .push(FactoryRequest::Start {
                blueprint: 2,
                batch,
                feeds,
                worker: worker.map(str::to_owned),
            });
        self.tick();
        assert_eq!(self.job().map(|job| job.size), Some(batch), "the run is on");
    }

    /// Played time, a second a step, until the run has made `made` or is
    /// over, and no longer than `most` seconds.
    pub(crate) fn run_until(&mut self, made: u32, most: u32) {
        for _ in 0..most {
            if self.job().is_none_or(|job| job.made >= made) {
                return;
            }
            self.h.advance(std::time::Duration::from_secs(1));
        }
        panic!("{most} s did not make {made}: {:?}", self.job());
    }

    /// Every stack of `item` in the household's store, read.
    pub(crate) fn stored_of(&self, item: &str) -> Vec<Record> {
        self.store()
            .0
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| strata_sim::items::read(stack, &self.registries.items).ok())
            .filter(|record| record.item.to_string() == item)
            .collect()
    }

    /// The card `record` would show, long, read against what the client
    /// last read of the world: the weapons carried, the ledger of what was
    /// made and the blueprints.
    pub(crate) fn card_of(&self, record: &Record) -> CardParams {
        let against = self.h.world().resource::<CardContext>().against();
        CardParams::of(record, &self.registries, true, against.as_ref()).expect("a card")
    }
}

/// The Record box shows what the blueprint would be before Record is
/// pressed, and the slider moves it: the lines it would make and the
/// minimum each slot would ask are the prototype's at a hundred and fall
/// with the derating, the words written in place so the drag that moves
/// them is not cut. Nothing is recorded by looking.
#[test]
fn the_slider_moves_the_lines_and_the_minimums() {
    let (mut day, bench) = craft_day("record-slider", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.core_by_keys(bench);
    let core = day.carried_of(CORE_ITEM)[0].clone();
    let density = core.lines["charge_density"];
    assert!(!day.inert(crafting::RECORD_OPEN));
    assert_eq!(day.says(crafting::RECORD_WHY_ID), "");
    assert!(
        day.h.try_find(&by::test_id(crafting::RECORD_ID)).is_none(),
        "the box is shut until asked for"
    );
    let open = find(&day.h, crafting::RECORD_OPEN);
    day.h.click(open);
    day.h.settle();
    assert!(
        day.h
            .try_find(&by::test_id(crafting::RECORD_OPEN))
            .is_none(),
        "the box's own Record is the one there is"
    );
    assert_eq!(day.says(crafting::RECORD_AT_ID), "100% of this one");
    assert_eq!(
        day.says(crafting::RECORD_MAKES_ID),
        format!("Charge density {density} · Stability 209")
    );
    assert_eq!(
        day.says(&crafting::record_need_id("salt")),
        "Potency at least 632 · Stability at least 225"
    );
    assert_eq!(
        day.says(&crafting::record_need_id("brine")),
        "Potency at least 605 · Conductivity at least 495 · Purity at least 534"
    );
    assert_eq!(day.says(crafting::RECORD_RUNS_ID), "100");
    let slider = find(&day.h, crafting::DERATE_ID);
    let state = day.h.world().get::<SliderState>(slider).unwrap().clone();
    assert_eq!((state.min, state.max, state.value), (0.7, 1.0, 1.0));
    assert!((state.step - 0.05).abs() < 1e-9, "the dial's step");
    // Half way along the track is 85 %.
    day.h.drag_slider(slider, 0.5);
    day.h.settle();
    assert_eq!(day.says(crafting::RECORD_AT_ID), "85% of this one");
    let derated = (u32::from(density) * 850 + 500) / 1000;
    assert_eq!(
        day.says(crafting::RECORD_MAKES_ID),
        format!("Charge density {derated} · Stability 178")
    );
    assert_eq!(
        day.says(&crafting::record_need_id("salt")),
        "Potency at least 537 · Stability at least 191"
    );
    assert_eq!(
        day.says(&crafting::record_need_id("brine")),
        "Potency at least 514 · Conductivity at least 421 · Purity at least 454"
    );
    assert_eq!(
        day.h.try_find(&by::test_id(crafting::DERATE_ID)),
        Some(slider),
        "the slider is the one that was dragged: nothing was rebuilt under it"
    );
    // All the way down is the dial's least.
    day.h.drag_slider(slider, 0.0);
    day.h.settle();
    assert_eq!(day.says(crafting::RECORD_AT_ID), "70% of this one");
    assert_eq!(
        day.says(&crafting::record_need_id("salt")),
        "Potency at least 442 · Stability at least 158"
    );
    assert!(day.prints().0.is_empty(), "nothing is recorded by looking");
    assert_eq!(day.craft().batches, 1);
    // Make another shuts the box with the card.
    let again = find(&day.h, crafting::AGAIN);
    day.h.click(again);
    day.h.settle();
    assert!(day.h.try_find(&by::test_id(crafting::RECORD_ID)).is_none());
}

/// The ticket's done-when for recording, by the keys alone: from the
/// finished card Tab reaches Record as blueprint, Accept opens the box on
/// its slider, Left takes the derating down a step a press, Tab reaches
/// Record and Accept records. The blueprint is an item in the pack with
/// its body in the household's ledger at what was shown, the prototype is
/// still there, a toast says so, and the card then says it is recorded.
#[test]
fn a_blueprint_is_recorded_by_keys_alone() {
    let (mut day, bench) = craft_day("record-keys", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.core_by_keys(bench);
    let core = day.carried_of(CORE_ITEM)[0].clone();
    day.tab_to(crafting::RECORD_OPEN);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(
        day.focus_id(),
        crafting::DERATE_ID,
        "Record as blueprint leads to its slider"
    );
    for _ in 0..3 {
        day.h.action(UiAction::Left);
        day.h.settle();
    }
    assert_eq!(day.says(crafting::RECORD_AT_ID), "85% of this one");
    assert_eq!(
        day.focus_id(),
        crafting::DERATE_ID,
        "the slider keeps the keys"
    );
    let shown = day.says(&crafting::record_need_id("salt"));
    assert_eq!(shown, "Potency at least 537 · Stability at least 191");
    day.tab_to(crafting::RECORD_GO);
    assert!(day.prints().0.is_empty(), "nothing yet");
    day.h.action(UiAction::Accept);
    day.tick();

    let prints = day.prints();
    let print = &prints.0[&2];
    assert_eq!((print.derate, print.prototype), (850, 1));
    assert_eq!((print.runs, print.runs_left), (100, 100));
    assert_eq!(print.by, "Frontier ranger");
    assert_eq!(
        print.lines["charge_density"],
        u16::try_from((u32::from(core.lines["charge_density"]) * 850 + 500) / 1000).unwrap()
    );
    let salt = &print.slots[0];
    assert_eq!(
        salt.needs
            .iter()
            .map(|need| (need.stat.as_str(), need.at_least))
            .collect::<Vec<_>>(),
        [("potency", Some(537)), ("stability", Some(191))],
        "what the box showed is what was written"
    );
    assert_eq!(day.carried_of(CORE_ITEM), [core], "the prototype is kept");
    let item = day.carried_of(BLUEPRINT);
    assert_eq!(item.len(), 1);
    assert_eq!((item[0].count, item[0].batch), (1, 2));
    assert_eq!(day.craft().batches, 2, "one counter numbers both");
    assert!(
        day.toasted()
            .iter()
            .any(|toast| toast.contains("Blueprint recorded at 85%.")),
        "{:?}",
        day.toasted()
    );
    assert_eq!(day.says(crafting::STATUS_ID), "Recorded as blueprint 2.");
    // The box is shut; a prototype is recorded once, and the button says
    // why it is grey.
    assert!(day.h.try_find(&by::test_id(crafting::RECORD_ID)).is_none());
    assert!(day.inert(crafting::RECORD_OPEN));
    assert_eq!(
        day.says(crafting::RECORD_WHY_ID),
        "Recorded already as blueprint 2"
    );
    assert_eq!(day.focus_id(), crafting::AGAIN);
}

/// A thing made earlier and still in the stock is listed under the
/// schematics while the household's ledger knows what went into it: its
/// row shows its finished card again, and it is recorded from there as one
/// just made is.
#[test]
fn an_earlier_core_is_recorded_from_the_tab() {
    let (mut day, bench) = craft_day("record-earlier", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.core_by_keys(bench);
    let core = day.carried_of(CORE_ITEM)[0].clone();
    day.h.action(UiAction::Back);
    day.h.settle();
    assert!(day.h.stack().is_empty());
    day.bench_f(bench);
    assert_eq!(day.h.stack_top(), Some(crafting::kind()));
    assert!(
        day.h.try_find(&by::test_id(crafting::CARD_ID)).is_none(),
        "the slots, not the card of the thing made before"
    );
    let row = crafting::proto_id(1);
    assert_eq!(
        day.row(&row),
        [format!(
            "Cell core · batch 1 · {}",
            core.lines["charge_density"]
        )]
    );
    // By the keys: its row, then Record as blueprint, the slider, Record.
    day.tab_to(&row);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.card_at(crafting::CARD_ID).name, "Cell core");
    assert_eq!(
        day.card_at(crafting::CARD_ID).from,
        "Crafted by Frontier ranger · batch 1"
    );
    assert_eq!(day.focus_id(), crafting::RECORD_OPEN);
    assert!(
        day.row(&row)[0].starts_with('\u{203a}'),
        "the one shown is marked"
    );
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.focus_id(), crafting::DERATE_ID);
    assert_eq!(day.says(crafting::RECORD_AT_ID), "100% of this one");
    day.tab_to(crafting::RECORD_GO);
    day.h.action(UiAction::Accept);
    day.tick();
    let prints = day.prints();
    assert_eq!((prints.0[&2].derate, prints.0[&2].prototype), (1000, 1));
    assert_eq!(
        prints.0[&2].lines, core.lines,
        "at a hundred, the prototype's own"
    );
    assert_eq!(day.craft().made[&1].recorded, Some(2));
    // A second time it is refused where it is asked.
    assert!(day.inert(crafting::RECORD_OPEN));
    assert_eq!(
        day.says(crafting::RECORD_WHY_ID),
        "Recorded already as blueprint 2"
    );
    // Away from a workbench nothing is recorded, and the button says so.
    day.h.world_mut().despawn(bench);
    day.tick();
    assert_eq!(day.says(crafting::RECORD_WHY_ID), "Step up to a workbench");
}

/// The ticket's rule 4 at the factory: a lot that does not meet a
/// blueprint's slot sits in the slot's list where the others are, after
/// the ones that meet it, dimmed, with the figure it fails on and what is
/// asked, and cannot be picked. The slot says what it needs in words.
#[test]
fn a_lot_under_the_minimum_is_dimmed_in_place_with_its_number() {
    let (mut day, bench, fabricator) = factory_day("factory-under", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    let marsh = lot_of(&day.registries.clone(), SALT, "Marsh", 60, &[380, 410, 300]);
    day.stock(&marsh);
    day.use_fabricator(fabricator);
    assert_eq!(day.says(factory::HEAD_ID), "CELL CORE · BLUEPRINT 2");
    assert_eq!(
        day.says(&format!("{}salt", factory::NEED_TAG)),
        "needs Potency 632 · Stability 225"
    );
    assert_eq!(
        day.says(&format!("{}brine", factory::NEED_TAG)),
        "needs Potency 605 · Conductivity 495 · Purity 534"
    );
    // The one that meets it first, marked and already feeding.
    let meets = factory::feed_id("salt", 0);
    assert_eq!(
        day.row(&meets),
        [
            "\u{203a} Tolley salt",
            "FEEDS",
            "Potency",
            "632",
            "·",
            "Stability",
            "225",
            "140 in store"
        ]
    );
    assert!(!day.inert(&meets));
    // The one that does not, in place, with its number and the need.
    let under = factory::feed_id("salt", 1);
    assert_eq!(
        day.row(&under),
        ["Marsh salt", "Potency 410, needs 632", "60 in store"]
    );
    assert!(day.inert(&under), "it cannot be picked");
    let name = day.h.find(&by::text("Marsh salt").index(1));
    assert_eq!(
        day.h
            .world()
            .get::<slotted::theme::Themed>(name)
            .unwrap()
            .0
            .as_str(),
        "strata.factory.feed.fail",
        "dimmed"
    );
    assert!(
        day.h
            .try_find(&by::test_id(&factory::mark_id("salt", 1)))
            .is_none(),
        "and not marked as meeting"
    );
    let row = find(&day.h, &under);
    day.h.click(row);
    day.h.settle();
    assert_eq!(
        day.feeding("salt"),
        ["Tolley"],
        "a press on it picks nothing"
    );
    // It takes the focus all the same, so its reason is read by the keys.
    day.h.set_input_mode(InputMode::Keyboard);
    day.tab_to(&under);
    // A lot at 85 % of the prototype would have been taken: the same lot
    // meets a blueprint recorded lower, which is the sim's to say.
    let registries = day.registries.clone();
    let mut eased = day.prints().0[&2].clone();
    eased.slots[0].needs[0].at_least = Some(400);
    assert!(strata_sim::factory::meets(&eased.slots[0], &marsh).is_ok());
    assert!(strata_sim::crafting::fits(
        &registries.items,
        &eased.slots[0].slot,
        &marsh
    ));
}

/// One press fills every slot with the cheapest stock that meets the
/// blueprint: the lot that clears the minimums by least, so the best is
/// kept for hand work. A row is a toggle, a slot may be fed from two lots,
/// and Cheapest stock puts the sim's own choice back and leads to Run.
#[test]
fn cheapest_stock_fills_every_slot() {
    let (mut day, bench, fabricator) = factory_day("factory-cheapest", 1600.0, 900.0);
    day.recorded_core(bench, 850);
    let calder = lot_of(
        &day.registries.clone(),
        SALT,
        "Calder",
        80,
        &[640, 820, 450],
    );
    day.stock(&calder);
    day.use_fabricator(fabricator);
    // The screen opens with the cheapest in the slots: Tolley clears 537
    // and 191 by less than Calder does, and is listed first.
    assert_eq!(day.feeding("salt"), ["Tolley"]);
    assert_eq!(day.feeding("brine"), ["Oxbow"]);
    assert_eq!(
        day.row(&factory::feed_id("salt", 0))[0],
        "\u{203a} Tolley salt"
    );
    assert_eq!(
        day.row(&factory::feed_id("salt", 1))[..2],
        ["Calder salt", "MEETS"]
    );
    assert_eq!(day.says(&factory::mark_id("salt", 0)), "FEEDS");
    // Calder in as well, then Tolley out: the better lot alone.
    let better = find(&day.h, &factory::feed_id("salt", 1));
    day.h.click(better);
    day.h.settle();
    assert_eq!(
        day.feeding("salt"),
        ["Tolley", "Calder"],
        "fed from two lots"
    );
    assert_eq!(day.says(&factory::mark_id("salt", 1)), "FEEDS");
    let cheaper = find(&day.h, &factory::feed_id("salt", 0));
    day.h.click(cheaper);
    day.h.settle();
    assert_eq!(day.feeding("salt"), ["Calder"]);
    assert_eq!(day.says(&factory::mark_id("salt", 0)), "MEETS");
    assert!(!day.inert(factory::RUN), "four from Calder's eighty");
    // With nothing feeding a slot Run says which.
    let only = find(&day.h, &factory::feed_id("salt", 1));
    day.h.click(only);
    day.h.settle();
    assert!(day.feeding("salt").is_empty());
    assert!(day.inert(factory::RUN));
    assert_eq!(day.says(factory::WHY_ID), "Reactive: nothing feeds it");
    // The press: the sim's choice again, and on to Run.
    day.h.set_input_mode(InputMode::Keyboard);
    day.tab_to(factory::CHEAPEST);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.feeding("salt"), ["Tolley"]);
    assert_eq!(day.feeding("brine"), ["Oxbow"]);
    assert_eq!(day.says(factory::WHY_ID), "");
    assert_eq!(day.focus_id(), factory::RUN, "Cheapest stock leads to Run");
    assert!(day.job().is_none(), "nothing is spent by filling");
}

/// The batch cannot pass what the stock feeds or the runs the blueprint
/// has left, and the line under it says which holds it and at what. A
/// press that would change nothing is grey.
#[test]
fn the_batch_says_what_holds_it() {
    let (mut day, bench, fabricator) = factory_day("factory-batch", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    day.use_fabricator(fabricator);
    // 140 salt feeds seven and 130 brine four: the brine holds it.
    assert_eq!(day.says(factory::BATCH_ID), "4");
    assert_eq!(
        day.says(factory::BATCH_WHY_ID),
        "Up to 4 · the Brine in stock feeds 4"
    );
    assert_eq!(day.role_of(factory::BATCH_WHY_ID), "strata.fitting.why");
    for step in ["more", "more10", "most"] {
        assert!(day.inert(&factory::batch_id(step)), "{step} at the limit");
    }
    assert_eq!(
        day.says(factory::MAKES_ID),
        format!(
            "Makes 4 cell cores at {} · ready in 2 min",
            day.prints().0[&2].lines["charge_density"]
        ),
        "four at 24 s with nobody on it: 96 s"
    );
    let less = find(&day.h, &factory::batch_id("less"));
    day.h.click(less);
    day.h.settle();
    assert_eq!(day.says(factory::BATCH_ID), "3");
    assert!(!day.inert(&factory::batch_id("more")));
    assert_eq!(
        day.role_of(factory::BATCH_WHY_ID),
        "strata.fitting.why.hint",
        "under the limit it is a note"
    );
    assert!(
        day.says(factory::MAKES_ID)
            .starts_with("Makes 3 cell cores")
    );
    assert!(day.says(factory::MAKES_ID).ends_with("ready in 2 min"));
    let least = find(&day.h, &factory::batch_id("less10"));
    day.h.click(least);
    day.h.settle();
    assert_eq!(day.says(factory::BATCH_ID), "1");
    assert!(
        day.inert(&factory::batch_id("less")),
        "a batch is one or more"
    );
    assert!(
        day.says(factory::MAKES_ID)
            .starts_with("Makes 1 cell core at")
    );
    assert!(day.says(factory::MAKES_ID).ends_with("ready in 24 s"));
    let most = find(&day.h, &factory::batch_id("most"));
    day.h.click(most);
    day.h.settle();
    assert_eq!(day.says(factory::BATCH_ID), "4");
    // More brine than the runs: the blueprint holds it.
    let oxbow = lot_of(
        &day.registries.clone(),
        BRINE,
        "Oxbow",
        160,
        &[534, 605, 495],
    );
    day.stock(&oxbow);
    day.h
        .world_mut()
        .resource_mut::<Blueprints>()
        .0
        .get_mut(&2)
        .unwrap()
        .runs_left = 3;
    day.h.settle();
    assert_eq!(day.says(factory::BATCH_ID), "3");
    assert_eq!(day.says(factory::BATCH_WHY_ID), "Up to 3 · 3 runs left");
    assert_eq!(
        day.row(&factory::print_id(2)),
        [
            "\u{203a} Cell core".to_owned(),
            day.prints().0[&2].lines["charge_density"].to_string(),
            "3 runs left".to_owned()
        ]
    );
    // And with no brine at all, nothing feeds it.
    day.h.action(UiAction::Back);
    day.h.settle();
    let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
    for cell in 0..store.0.len() {
        store.0.set(cell, None);
    }
    day.tick();
    day.use_fabricator(fabricator);
    assert_eq!(day.says(factory::BATCH_ID), "1");
    assert_eq!(
        day.says(factory::BATCH_WHY_ID),
        "The Reactive in stock does not feed one"
    );
    assert_eq!(
        day.says(&format!("{}salt", factory::NOTHING_TAG)),
        "Nothing in stock fits"
    );
    assert!(day.inert(factory::RUN));
    assert_eq!(day.says(factory::WHY_ID), "Reactive: nothing feeds it");
}

/// The ticket's done-when at the fabricator, by the keys alone: F, Accept
/// on the blueprint, Accept on Run. Every input for the batch and the
/// blueprint are committed, the screen shows the run, and Stop gives back
/// what was not used and the blueprint with the runs it has left.
#[test]
fn a_run_is_started_and_stopped_by_keys_alone() {
    let (mut day, bench, fabricator) = factory_day("factory-keys", 1600.0, 900.0);
    let core = day.recorded_core(bench, 1000);
    let mut presses = 1;
    day.use_fabricator(fabricator);
    assert_eq!(
        day.focus_id(),
        factory::print_id(2),
        "the screen opens with the keyboard on the blueprint shown"
    );
    assert_eq!(
        day.says(factory::WORKER_ID),
        "WORKED BY NOBODY · HALF SPEED"
    );
    assert_eq!(
        day.says(factory::STATUS_ID),
        "Feed a blueprint's slots and run it. Every unit comes out the same, and better stock gains nothing."
    );
    day.press(&mut presses, UiAction::Accept);
    assert_eq!(day.focus_id(), factory::RUN, "a blueprint leads to Run");
    assert!(day.job().is_none(), "nothing is spent yet");
    day.press(&mut presses, UiAction::Accept);
    assert_eq!(presses, 3, "F, the blueprint, Run");

    let job = day.job().expect("the run is on");
    assert_eq!((job.blueprint, job.batch, job.size, job.made), (2, 3, 4, 0));
    assert_eq!(job.worker, None);
    assert_eq!(
        (day.units_of("Tolley"), day.units_of("Oxbow")),
        (60, 10),
        "eighty and a hundred and twenty are in the hopper"
    );
    assert!(
        day.carried_of(BLUEPRINT).is_empty(),
        "the fabricator holds it"
    );
    assert_eq!(
        day.says(factory::HEAD_ID),
        "CELL CORE · BLUEPRINT 2 · BATCH 3"
    );
    assert_eq!(day.says(factory::MADE_ID), "0 of 4");
    assert_eq!(day.says(factory::LEFT_ID), "Ready in 2 min");
    assert_eq!(
        day.says(factory::STATUS_ID),
        "Batch 3 is running · ready in 2 min."
    );
    assert_eq!(
        day.focus_id(),
        factory::print_id(2),
        "Run leads back to the blueprint: a second Accept does not stop it"
    );
    let hopper = find(&day.h, factory::HOPPER_ID);
    let listed = texts_under(
        &day.h,
        day.h.world().get::<ChildOf>(hopper).unwrap().parent(),
    );
    assert_eq!(
        listed,
        [
            "IN THE HOPPER",
            "Tolley salt",
            "80 units",
            "Oxbow brine",
            "120 units"
        ]
    );
    // Stop, by the keys.
    assert!(!day.inert(factory::STOP));
    day.tab_to(factory::STOP);
    day.h.action(UiAction::Accept);
    day.tick();
    assert!(day.job().is_none());
    assert_eq!((day.units_of("Tolley"), day.units_of("Oxbow")), (140, 130));
    let back = day.stored_of(BLUEPRINT);
    assert_eq!(back.len(), 1, "the blueprint is handed to the store");
    assert_eq!(day.prints().0[&2].runs_left, 100, "none was made");
    assert_eq!(
        day.says(factory::STATUS_ID),
        "Stopped at 0. What it had not used is back in the store."
    );
    assert_eq!(day.says(factory::HEAD_ID), "CELL CORE · BLUEPRINT 2");
    assert!(!day.inert(factory::RUN), "ready to run again");
    assert_eq!(day.carried_of(CORE_ITEM), [core]);
    day.tab_to(factory::CLOSE_ID);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert!(day.h.stack().is_empty());
}

/// The time a unit takes is the time this sheet's boxes would start the run
/// at: the same blueprint reads a shorter time to a member with Fixtures
/// and the master box, and the same to one without.
#[test]
fn the_time_a_run_takes_follows_the_members_sheet() {
    let (mut day, bench, fabricator) = factory_day("factory-pace", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    day.use_fabricator(fabricator);
    let plain = day.says(factory::MAKES_ID);
    assert!(plain.ends_with("ready in 2 min"), "{plain}");
    day.h.action(UiAction::Back);
    day.h.settle();
    let world = day.h.world_mut();
    let mut players = world.query_filtered::<&mut CharacterSheet, With<strata_sim::LocalPlayer>>();
    let mut sheet = players.single_mut(world).expect("the player");
    for id in [
        "fabricator.fixtures.1",
        "fabricator.fixtures.4",
        "fabricator.master",
    ] {
        sheet.learned.insert(id.to_owned());
    }
    day.use_fabricator(fabricator);
    let quick = day.says(factory::MAKES_ID);
    assert!(
        quick.ends_with("ready in 56 s"),
        "four units at 24 s with nobody on it, at 0.9, 0.8 and 0.8 of the time: {quick}"
    );
}

/// With a run on, the same screen shows it: how many of the batch are
/// made, the time left, who works it, and when the store has no room that
/// it stands and why. The worker is changed from the screen, and a run
/// that stands goes on when there is room.
#[test]
#[allow(clippy::too_many_lines)]
fn a_run_shows_made_time_worker_and_pause() {
    let (mut day, bench, fabricator) = factory_day("factory-run", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    day.h
        .world_mut()
        .insert_resource(Benched(vec!["Ilse Marrow".to_owned()]));
    day.start_run(3, Some("Ilse Marrow"));
    day.use_fabricator(fabricator);
    assert_eq!(day.says(factory::WORKER_ID), "WORKED BY ILSE MARROW");
    assert_eq!(
        day.says(factory::HEAD_ID),
        "CELL CORE · BLUEPRINT 2 · BATCH 3"
    );
    assert_eq!(day.says(factory::MADE_ID), "0 of 3");
    assert_eq!(day.says(factory::LEFT_ID), "Ready in 36 s");
    assert_eq!(
        day.row(&factory::worker_id(1)),
        ["\u{203a} Ilse Marrow · full speed"]
    );
    assert_eq!(day.row(&factory::worker_id(0)), ["Nobody · half speed"]);
    assert_eq!(
        day.says(factory::STATUS_ID),
        "The run goes on while you are away. Stop keeps what is made and gives back the rest."
    );
    // The time is written where it stands as it runs down.
    let left = find(&day.h, factory::LEFT_ID);
    day.h.advance(std::time::Duration::from_secs(3));
    assert_eq!(day.h.try_find(&by::test_id(factory::LEFT_ID)), Some(left));
    let said = day.says(factory::LEFT_ID);
    let seconds: u32 = said
        .strip_prefix("Ready in ")
        .and_then(|rest| rest.strip_suffix(" s"))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{said}"));
    assert!((30..=34).contains(&seconds), "{said}");
    // A unit made: the count, the bar and the blueprint's runs.
    day.run_until(1, 14);
    day.h.settle();
    assert_eq!(day.says(factory::MADE_ID), "1 of 3");
    assert_eq!(day.prints().0[&2].runs_left, 99);
    assert_eq!(
        day.row(&factory::print_id(2))[2],
        "99 runs left",
        "the fabricator holds the blueprint, and its row says what it has left"
    );
    let made = day.stored_of(CORE_ITEM);
    assert_eq!(made.len(), 1);
    assert_eq!((made[0].count, made[0].batch), (1, 3));
    // Nobody on it: asked of the sim, and the time left doubles.
    let nobody = find(&day.h, &factory::worker_id(0));
    day.h.click(nobody);
    day.tick();
    assert_eq!(day.job().unwrap().worker, None);
    assert_eq!(
        day.says(factory::WORKER_ID),
        "WORKED BY NOBODY · HALF SPEED"
    );
    assert_eq!(day.says(factory::STATUS_ID), "Nobody is working the run.");
    assert_eq!(
        day.row(&factory::worker_id(0)),
        ["\u{203a} Nobody · half speed"]
    );
    let somebody = find(&day.h, &factory::worker_id(1));
    day.h.click(somebody);
    day.tick();
    assert_eq!(day.job().unwrap().worker.as_deref(), Some("Ilse Marrow"));
    assert_eq!(
        day.says(factory::STATUS_ID),
        "Ilse Marrow is working the run."
    );
    // No room in the store: the run stands, says so here and in a toast,
    // and Stop is refused for the same want of room.
    {
        let registries = day.registries.clone();
        let items = &registries.items;
        let mut one = items.starting().unwrap().store[1].clone();
        one.count = 1;
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        // The stack of cores too: a full stack of something else stands
        // in its cell, so the next unit has nowhere to go.
        for cell in 0..store.0.len() {
            let core = store.0.get(cell).is_some_and(|stack| {
                strata_sim::items::read(stack, items)
                    .is_ok_and(|record| record.item.to_string() == CORE_ITEM)
            });
            if store.0.get(cell).is_none() || core {
                one.batch += 1;
                let single = strata_sim::items::write(&one, items).unwrap();
                store.0.set(cell, Some(single));
            }
        }
    }
    day.h.advance(std::time::Duration::from_secs(13));
    day.h.settle();
    let job = day.job().expect("still on");
    assert_eq!(job.made, 1, "the unit has nowhere to go");
    assert!(job.paused.is_some());
    assert_eq!(
        day.says(factory::PAUSED_ID),
        "PAUSED · no room in the store"
    );
    assert_eq!(day.role_of(factory::PAUSED_ID), "strata.factory.paused");
    assert!(day.h.try_find(&by::test_id(factory::LEFT_ID)).is_none());
    assert!(
        day.toasted()
            .iter()
            .any(|toast| toast.contains("The fabricator has stopped: the store is full")),
        "{:?}",
        day.toasted()
    );
    assert!(day.inert(factory::STOP));
    assert_eq!(
        day.says(factory::STOP_WHY_ID),
        "No room in the store for what the run holds"
    );
    // Room: it goes on with the next tick.
    {
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        let last = store.0.len() - 1;
        store.0.set(last, None);
        store.0.set(last - 1, None);
        store.0.set(last - 2, None);
    }
    day.tick();
    let job = day.job().expect("still on");
    assert_eq!((job.made, job.paused), (2, None));
    assert_eq!(day.says(factory::MADE_ID), "2 of 3");
    assert!(day.h.try_find(&by::test_id(factory::PAUSED_ID)).is_none());
    assert!(!day.inert(factory::STOP));
}

/// A finished run is announced wherever the player is: a toast with how
/// many of what are in the store and under which batch, with no screen
/// open. A screen opened after shows the fabricator free again.
#[test]
fn a_finished_run_is_toasted_with_no_screen_open() {
    let (mut day, bench, fabricator) = factory_day("factory-toast", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    day.h
        .world_mut()
        .insert_resource(Benched(vec!["Ilse Marrow".to_owned()]));
    day.start_run(2, Some("Ilse Marrow"));
    assert!(day.h.stack().is_empty(), "no screen");
    day.run_until(2, 30);
    day.h.step(3);
    assert!(day.job().is_none(), "finished");
    assert!(
        day.toasted()
            .iter()
            .any(|toast| toast.contains("2 cell cores are in the store · batch 3")),
        "{:?}",
        day.toasted()
    );
    assert_eq!(day.stored_of(BLUEPRINT).len(), 1, "the blueprint is back");
    assert_eq!(day.prints().0[&2].runs_left, 98);
    day.use_fabricator(fabricator);
    assert_eq!(day.says(factory::HEAD_ID), "CELL CORE · BLUEPRINT 2");
    assert_eq!(day.row(&factory::print_id(2))[2], "98 runs left");
    // A blueprint's last run says the blueprint is spent, and one unit is
    // counted as one.
    let registries = day.registries.clone();
    assert_eq!(
        factory::notice_words(
            &registries,
            &strata_sim::factory::FactoryNotice::Finished {
                item: strata_registry::Namespaced::parse(CORE_ITEM).unwrap(),
                made: 1,
                batch: 9,
                spent: true,
            }
        ),
        "1 cell core is in the store · batch 9 · the blueprint is spent"
    );
}

/// Away from a fabricator the screen is a reading of what the character
/// carries: the store is out of reach, Run is grey and says where a
/// blueprint is run.
#[test]
fn away_from_a_fabricator_run_is_grey_with_its_reason() {
    let (mut day, bench, fabricator) = factory_day("factory-away", 1600.0, 900.0);
    day.recorded_core(bench, 1000);
    day.h.world_mut().despawn(fabricator);
    day.tick();
    let mut commands = day.h.world_mut().commands();
    factory::open(&mut commands);
    day.h.world_mut().flush();
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(factory::kind()));
    // The blueprint is carried, so it is listed; the lots are not.
    assert_eq!(day.says(factory::HEAD_ID), "CELL CORE · BLUEPRINT 2");
    assert_eq!(
        day.says(&format!("{}salt", factory::NOTHING_TAG)),
        "Nothing in your pack fits"
    );
    assert!(day.inert(factory::RUN));
    assert_eq!(day.says(factory::WHY_ID), "Step up to a fabricator");
    assert_eq!(
        day.says(factory::STATUS_ID),
        "A reading. A blueprint is run at a fabricator."
    );
    let run = find(&day.h, factory::RUN);
    day.h.click(run);
    day.tick();
    assert!(day.job().is_none());
    // A request from there is refused by the sim all the same.
    day.h
        .world_mut()
        .resource_mut::<FactoryInbox>()
        .0
        .push(FactoryRequest::Stop);
    day.tick();
    assert_eq!(day.says(factory::STATUS_ID), "Step up to a fabricator");
    assert_eq!(
        day.role_of(factory::STATUS_ID),
        "strata.fitting.status.refused"
    );
}

/// The bench says where a working member is: their card reads that they
/// are at the fabricator with the count so far, Switch is grey for them
/// alone, and the line over it says why. A press starts no switch.
#[test]
fn a_working_member_says_so_and_is_not_switched_to() {
    let mut h = harness(temp_settings("bench-working"));
    spawn_local_player(&mut h, 7);
    let card = |name: &str, work| ui::bench::BenchCard {
        name: name.to_owned(),
        tint: 0,
        weapon: strata_sim::combat::Weapon::Carbine,
        sheet: CharacterSheet::default(),
        work,
        injured_min: None,
    };
    h.world_mut().insert_resource(ui::bench::BenchView {
        characters: vec![
            card("Frontier ranger", None),
            card(
                "Ilse Marrow",
                Some(ui::bench::BenchWork { made: 7, size: 20 }),
            ),
            card("Tovan Reyes", None),
        ],
        active: 0,
        recruits: Vec::new(),
        capacity: 4,
    });
    h.world_mut().write_message(Interact {
        entity: Entity::PLACEHOLDER,
        kind: InteractionKind::Terminal,
    });
    h.step(2);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::bench::kind()));
    let says = |h: &UiHarness, id: &str| h.text_of(find(h, id)).unwrap_or_default();
    assert_eq!(
        says(&h, &format!("{}1", ui::bench::WORK_TAG)),
        "AT THE FABRICATOR · 7 OF 20"
    );
    assert_eq!(
        says(&h, &format!("{}1", ui::bench::WHY_TAG)),
        "Working until the run ends or is stopped."
    );
    assert!(button_disabled(&h, &format!("{}1", ui::bench::SWITCH_TAG)));
    assert!(
        !button_disabled(&h, &format!("{}2", ui::bench::SWITCH_TAG)),
        "somebody idle is switched to as before"
    );
    for other in [0, 2] {
        assert!(
            h.try_find(&by::test_id(&format!("{}{other}", ui::bench::WORK_TAG)))
                .is_none()
        );
        assert!(
            h.try_find(&by::test_id(&format!("{}{other}", ui::bench::WHY_TAG)))
                .is_none()
        );
    }
    let grey = find(&h, &format!("{}1", ui::bench::SWITCH_TAG));
    h.click(grey);
    h.settle();
    assert_eq!(
        h.world().resource::<ui::bench::SwitchFade>().target,
        None,
        "no switch was begun"
    );
    // The run over, the card is as it was and the button live.
    h.world_mut()
        .resource_mut::<ui::bench::BenchView>()
        .characters[1]
        .work = None;
    h.step(2);
    h.settle();
    assert!(!button_disabled(&h, &format!("{}1", ui::bench::SWITCH_TAG)));
    assert!(
        h.try_find(&by::test_id(&format!("{}1", ui::bench::WORK_TAG)))
            .is_none()
    );
}

/// A blueprint is an item with a card: what it makes, the lines every run
/// has, the runs left, who recorded it, what each slot needs and what it
/// was recorded from. The stores list it under its own chip, and its card
/// is the one pinned there.
#[test]
fn a_blueprints_card_says_what_it_needs_and_has_left() {
    let (mut day, bench, _) = factory_day("blueprint-card", 1600.0, 900.0);
    let core = day.recorded_core(bench, 850);
    let density = (u32::from(core.lines["charge_density"]) * 850 + 500) / 1000;
    day.use_locker();
    assert_eq!(
        stores::Filter::ALL[4],
        stores::Filter::Blueprints,
        "the chip after Mods"
    );
    let chip = find(&day.h, &stores::Filter::Blueprints.chip_id());
    assert_eq!(texts_under(&day.h, chip), ["BLUEPRINTS"]);
    let mut commands = day.h.world_mut().commands();
    stores::show_only(&mut commands, stores::Filter::Blueprints);
    day.h.world_mut().flush();
    day.h.step(3);
    day.h.settle();
    let card = day.card();
    assert_eq!(card.name, "Cell core blueprint");
    assert_eq!(card.kind, "Blueprint");
    assert_eq!(card.from, "Recorded by Frontier ranger · blueprint 2");
    // The line under Send names it as its card does, not by the item every
    // blueprint is.
    assert_eq!(
        day.says(stores::SENDS_ID),
        "Sends Cell core blueprint x1",
        "one thing has one name on the screen"
    );
    assert_eq!(
        card.lines
            .iter()
            .map(|line| (line.name.as_str(), u32::from(line.value)))
            .collect::<Vec<_>>(),
        [("Charge density", density), ("Stability", 178)],
        "what every run makes"
    );
    assert_eq!(
        card.lines[0].note,
        "Goes into: Capacity of a patrol extended magazine"
    );
    assert_eq!(
        card.notes,
        [
            "100 of 100 runs left",
            "Makes cell cores, every one the same.",
            "Reactive, 20 units: Potency at least 537 · Stability at least 191",
            "Brine, 30 units: Potency at least 514 · Conductivity at least 421 · Purity at least 454",
            "Recorded at 85% of batch 1."
        ]
    );
    for def in day.registries.items.iter() {
        assert_eq!(
            stores::Filter::Blueprints.shows(&def.kind),
            def.id.to_string() == BLUEPRINT,
            "{}",
            def.id
        );
    }
    // The short card keeps the lines and the runs and leaves the slots to
    // the long one.
    let item = day.carried_of(BLUEPRINT)[0].clone();
    let against = day.h.world().resource::<CardContext>().against();
    let short = CardParams::of(&item, &day.registries, false, against.as_ref()).unwrap();
    assert_eq!(short.lines.len(), 2);
    assert_eq!(
        short.notes,
        [
            "100 of 100 runs left",
            "Makes cell cores, every one the same.",
            "Hold Shift for the long card"
        ]
    );
}

/// What a fabricator makes is one batch in one stack: every unit has the
/// blueprint's lines exactly, the run's one batch number and the recorder
/// as its maker, and its card says the blueprint it was run off and how
/// many are in the stack.
#[test]
fn a_factory_stack_is_one_batch_with_a_count() {
    let (mut day, bench, _) = factory_day("factory-stack", 1600.0, 900.0);
    let core = day.recorded_core(bench, 1000);
    day.h
        .world_mut()
        .insert_resource(Benched(vec!["Ilse Marrow".to_owned()]));
    day.start_run(3, Some("Ilse Marrow"));
    day.run_until(3, 45);
    day.h.step(3);
    assert!(day.job().is_none());
    let made = day.stored_of(CORE_ITEM);
    assert_eq!(made.len(), 1, "one stack");
    let stack = &made[0];
    assert_eq!((stack.count, stack.batch), (3, 3));
    assert_eq!(stack.by.as_deref(), Some("Frontier ranger"));
    assert_eq!(stack.lines, core.lines, "recorded at a hundred: the same");
    assert_eq!(stack.lines, day.prints().0[&2].lines);
    let card = day.card_of(stack);
    assert_eq!(card.name, "Cell core");
    assert_eq!(card.from, "Run off blueprint 2 · batch 3 · 3 in the stack");
    assert_eq!(
        card.notes.last().map(String::as_str),
        Some("Made from Tolley salt · Oxbow brine.")
    );
    // The prototype, made by hand, still says who made it.
    assert_eq!(
        day.card_of(&core).from,
        "Crafted by Frontier ranger · batch 1"
    );
    // The stores show it as one cell with its count.
    day.use_locker();
    let id = day.id(CORE_ITEM);
    let cells: Vec<(ItemId, u32)> = (0..54)
        .filter_map(|cell| day.shows(day.cell("store", cell)))
        .filter(|(item, _)| *item == id)
        .collect();
    assert_eq!(cells, [(id, 3)]);
    // A copy is no prototype: it is not offered at the workbench.
    day.close();
    day.bench_f(bench);
    assert_eq!(day.h.stack_top(), Some(crafting::kind()));
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::proto_id(1)))
            .is_some()
    );
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::proto_id(3)))
            .is_none()
    );
}

/// The factory fits a small window in both its states with its buttons
/// and its Close inside the panel, and at the game's sizes Run and Stop
/// are not scrolled out of sight.
#[test]
fn the_factory_fits_small_windows() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0), (1000.0, 560.0)] {
        let (mut day, bench, fabricator) = factory_day("factory-small", width, height);
        let window = Rect::new(0., 0., width, height);
        day.recorded_core(bench, 850);
        let marsh = lot_of(&day.registries.clone(), SALT, "Marsh", 60, &[380, 410, 300]);
        day.stock(&marsh);
        day.use_fabricator(fabricator);
        let root = day.h.rect_of(find(&day.h, factory::ROOT_ID));
        assert!(within(root, window), "{root:?} at {width}x{height}");
        for id in [factory::CLOSE_ID, factory::TITLE_ID, factory::STATUS_ID] {
            let rect = day.h.rect_of(find(&day.h, id));
            assert!(within(rect, root), "{id} {rect:?} in {root:?}");
        }
        if height >= 720. {
            for id in [factory::RUN, factory::CHEAPEST, factory::BATCH_ID] {
                let rect = day.h.rect_of(find(&day.h, id));
                assert!(
                    within(rect, root) && day.laid_out(find(&day.h, id)),
                    "{id} {rect:?} in {root:?} at {width}x{height}"
                );
            }
        }
        // On the smallest the column scrolls, and Run is reached by the
        // keys as any control out of sight is.
        day.h.set_input_mode(InputMode::Keyboard);
        day.tab_to(factory::RUN);
        day.h.action(UiAction::Accept);
        day.tick();
        assert!(day.job().is_some(), "at {width}x{height}");
        let root = day.h.rect_of(find(&day.h, factory::ROOT_ID));
        assert!(within(root, window));
        for id in [factory::CLOSE_ID, factory::STOP, factory::MADE_ID] {
            let rect = day.h.rect_of(find(&day.h, id));
            if height >= 720. || id == factory::CLOSE_ID {
                assert!(within(rect, root), "{id} {rect:?} in {root:?}");
            }
        }
    }
}

/// Every role and every key the factory's two states and the Record box
/// name is in the theme and has English.
#[test]
#[allow(clippy::too_many_lines)]
fn the_factory_and_the_record_box_name_roles_and_keys_that_exist() {
    let (mut day, bench, fabricator) = factory_day("factory-roles", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.core_by_keys(bench);
    let open = find(&day.h, crafting::RECORD_OPEN);
    day.h.click(open);
    day.h.settle();
    let record = crafting::screen_def(
        day.h
            .world()
            .resource::<crafting::CraftingState>()
            .view()
            .expect("the crafting screen is up"),
    );
    let go = find(&day.h, crafting::RECORD_GO);
    day.h.click(go);
    day.tick();
    day.h.action(UiAction::Back);
    day.h.settle();
    let marsh = lot_of(&day.registries.clone(), SALT, "Marsh", 60, &[380, 410, 300]);
    day.stock(&marsh);
    day.use_fabricator(fabricator);
    let view = |day: &Day| {
        factory::screen_def(
            day.h
                .world()
                .resource::<factory::FactoryUi>()
                .view()
                .expect("the factory is up"),
        )
    };
    let idle = view(&day);
    let run = find(&day.h, factory::RUN);
    day.h.click(run);
    day.tick();
    let running = view(&day);
    let mut roles: std::collections::BTreeSet<String> =
        factory::ROLES.into_iter().map(str::to_owned).collect();
    roles.insert("strata.bench.work".to_owned());
    let mut keys = Vec::new();
    for def in [&record, &idle, &running] {
        def.root.walk(&mut |node| match node {
            UiNodeDef::Panel { role, .. } => {
                roles.insert(role.as_str().to_owned());
            }
            UiNodeDef::Text { key, opts, .. } => {
                keys.push(key.0.clone());
                if let Some(role) = &opts.role {
                    roles.insert(role.as_str().to_owned());
                }
            }
            UiNodeDef::Card { role, .. } => {
                for state in ["", ".hover", ".focus", ".pressed"] {
                    roles.insert(format!("{}{state}", role.as_str()));
                }
            }
            UiNodeDef::Button { opts, .. } => {
                keys.extend(opts.label.iter().map(|label| label.0.clone()));
                roles.extend(opts.role.iter().map(|role| role.as_str().to_owned()));
            }
            _ => {}
        });
    }
    let world = day.h.world();
    let active = world.resource::<slotted::theme::ActiveTheme>().0.clone();
    let theme = world
        .resource::<Assets<slotted::theme::Theme>>()
        .get(&active)
        .expect("the theme");
    for role in &roles {
        assert!(
            theme
                .roles
                .contains_key(&slotted::theme::Role::new(role.clone())),
            "{role} is not in the theme"
        );
    }
    assert!(roles.contains("strata.factory.feed.fail"), "{roles:?}");
    assert!(roles.contains("strata.factory.progress.fill"), "{roles:?}");
    for key in &keys {
        if key.starts_with("strata.") {
            assert!(ui::strings::english(key).is_some(), "{key} has no English");
        }
    }
    for key in [
        "strata.prompt.fabricator",
        "strata.stores.filter.blueprints",
        "strata.card.kind.blueprint",
        "strata.card.from.run",
        "strata.bench.work",
        "strata.bench.work.why",
        "strata.factory.toast.finished",
        "strata.factory.toast.paused",
        "strata.crafting.record.done",
    ] {
        assert!(ui::strings::english(key).is_some(), "{key}");
    }
    assert!(keys.iter().any(|key| key == "Marsh salt"), "{keys:?}");
    assert!(
        keys.iter().any(|key| key == "Potency 410, needs 632"),
        "{keys:?}"
    );
    assert!(
        keys.iter()
            .any(|key| key == "IN THE HOPPER" || key == "strata.factory.hopper")
    );
}
