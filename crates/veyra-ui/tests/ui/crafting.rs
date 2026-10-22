//! The crafting session at the workbench (ticket 0007, step 4).

use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use slotted::ui::ButtonState;
use slotted::ui::InputMode;
use slotted::ui::SemanticLabel;
use slotted::ui::UiAction;
use slotted::ui::def::Tags;
use slotted_test::prelude::*;
use strata_registry::GameRegistries;
use strata_registry::item::Record;
use strata_registry::schematic::Hand;
use veyra_sim::crafting::CraftState;
use veyra_sim::deposits::Purse;
use veyra_sim::fitting::FitInbox;
use veyra_sim::fitting::FitRequest;
use veyra_sim::items::HouseholdStore;
use veyra_sim::items::Pack;
use veyra_sim::progression::Interact;
use veyra_sim::progression::InteractionKind;
use veyra_ui::card::BarFill;
use veyra_ui::card::QualityBar;
use veyra_ui::crafting;
use veyra_ui::fitting;

use crate::common::*;
use crate::fitting::bench_day;

pub(crate) const CORE_ITEM: &str = "veyra:component.cell_core";
pub(crate) const HARNESS: &str = "veyra:component.harness";
pub(crate) const SEALS: &str = "veyra:commodity.seal_set";
pub(crate) const SALT: &str = "strata:evaporite_salt";
pub(crate) const BRINE: &str = "strata:coastal_brine";
pub(crate) const ALLOY: &str = "strata:light_alloy_ore";

/// A lot of `resource` drawn at `root`, with `figures` in its class's
/// order.
pub(crate) fn lot_of(
    registries: &GameRegistries,
    resource: &str,
    root: &str,
    units: u32,
    figures: &[u16],
) -> Record {
    let resource = strata_registry::Namespaced::parse(resource).unwrap();
    let def = registries.items.lot_of(&resource).expect("a lot item");
    assert_eq!(def.lines.len(), figures.len());
    let cell = i32::try_from(root.bytes().map(u32::from).sum::<u32>()).unwrap();
    Record {
        item: def.id.clone(),
        count: units,
        batch: 0,
        by: None,
        lines: def
            .lines
            .iter()
            .zip(figures)
            .map(|(line, figure)| (line.id.clone(), *figure))
            .collect(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: Some(strata_registry::item::LotOrigin {
            deposit: [cell, 0],
            root: root.to_owned(),
            place: format!("{root} flats"),
        }),
    }
}

/// The first day's three lots at the default seed, as the game draws them.
pub(crate) fn first_lots(registries: &GameRegistries) -> [Record; 3] {
    [
        lot_of(registries, SALT, "Tolley", 160, &[389, 632, 225]),
        lot_of(registries, BRINE, "Oxbow", 160, &[534, 605, 495]),
        lot_of(
            registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        ),
    ]
}

/// [`bench_day`] as a fresh household has it: the first lots in the store
/// beside the bought parts, and the first day's purse.
pub(crate) fn craft_day(name: &str, width: f32, height: f32) -> (Day, Entity) {
    let (mut day, bench) = bench_day(name, width, height);
    for lot in first_lots(&day.registries.clone()) {
        day.stock(&lot);
    }
    let purse = day.registries.items.starting().unwrap().purse;
    day.h.world_mut().insert_resource(Purse(purse));
    day.h.settle();
    (day, bench)
}

impl Day {
    /// Puts `record` in the first empty cell of the household's store.
    pub(crate) fn stock(&mut self, record: &Record) {
        let stack = veyra_sim::items::write(record, &self.registries.items).unwrap();
        let mut store = self.h.world_mut().resource_mut::<HouseholdStore>();
        let cell = store
            .0
            .slots()
            .iter()
            .position(Option::is_none)
            .expect("room in the store");
        store.0.set(cell, Some(stack));
        store.0.clear_changed();
    }

    /// Puts `record` in the first empty cell of the pack.
    pub(crate) fn carry(&mut self, record: &Record) {
        let stack = veyra_sim::items::write(record, &self.registries.items).unwrap();
        let mut pack = self.h.world_mut().get_mut::<Pack>(self.player).unwrap();
        let cell = pack
            .cells
            .slots()
            .iter()
            .position(Option::is_none)
            .expect("room in the pack");
        pack.cells.set(cell, Some(stack));
        pack.cells.clear_changed();
    }

    /// F at the workbench, whichever of its tabs that opens.
    pub(crate) fn bench_f(&mut self, bench: Entity) {
        self.h.world_mut().write_message(Interact {
            entity: bench,
            kind: InteractionKind::Workbench,
        });
        self.h.step(2);
        self.h.settle();
    }

    /// The crafting tab on `schematic`, as the tab strip would open it.
    pub(crate) fn open_crafting(&mut self, schematic: &str) {
        let id = crafting::schematic_named(&self.registries, schematic).expect("a schematic");
        let mut commands = self.h.world_mut().commands();
        crafting::open(&mut commands, Some(id));
        self.h.world_mut().flush();
        self.h.settle();
        assert_eq!(self.h.stack_top(), Some(crafting::kind()));
    }

    /// One press of `action`, counted, and the tick in which the sim
    /// answers what it asked.
    pub(crate) fn press(&mut self, presses: &mut usize, action: UiAction) {
        *presses += 1;
        self.h.action(action);
        self.tick();
    }

    pub(crate) fn craft(&self) -> CraftState {
        self.h.world().resource::<CraftState>().clone()
    }

    pub(crate) fn focus_id(&self) -> String {
        self.named(self.h.focused())
    }

    /// Whether the control `id` is drawn inert.
    pub(crate) fn inert(&self, id: &str) -> bool {
        let entity = find(&self.h, id);
        self.h.world().get::<ButtonState>(entity).map_or_else(
            || {
                self.h
                    .world()
                    .get::<bevy::ui::InteractionDisabled>(entity)
                    .is_some()
            },
            |button| button.disabled,
        )
    }

    /// Every stack of `item` in the pack and its bags, read.
    pub(crate) fn carried_of(&self, item: &str) -> Vec<Record> {
        let pack = self.pack();
        std::iter::once(&pack.cells)
            .chain(&pack.bags)
            .flat_map(|inventory| inventory.slots().iter().flatten())
            .filter_map(|stack| veyra_sim::items::read(stack, &self.registries.items).ok())
            .filter(|record| record.item.to_string() == item)
            .collect()
    }

    /// How many units of the lot drawn at `root` the store holds.
    pub(crate) fn units_of(&self, root: &str) -> u32 {
        self.store()
            .0
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| veyra_sim::items::read(stack, &self.registries.items).ok())
            .filter(|record| {
                record
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.root == root)
            })
            .map(|record| record.count)
            .sum()
    }

    /// The width of the fill of the line `line`'s bar, in percent of its
    /// track.
    pub(crate) fn fill_of(&mut self, line: &str) -> f32 {
        let bar = find(&self.h, &format!("{}{line}", crafting::BAR_TAG));
        let world = self.h.world_mut();
        let mut fills = world.query_filtered::<(&Node, &ChildOf), With<BarFill>>();
        let widths: Vec<(Val, Entity)> = fills
            .iter(world)
            .map(|(node, track)| (node.width, track.parent()))
            .collect();
        widths
            .into_iter()
            .find(|(_, track)| world.get::<ChildOf>(*track).map(ChildOf::parent) == Some(bar))
            .map(|(width, _)| match width {
                Val::Percent(percent) => percent,
                other => panic!("the fill is {other:?} wide"),
            })
            .expect("the bar has a fill")
    }

    /// The cell core from the workbench's F by the keys alone, as a player
    /// with a fresh household would make it; gives the presses it took.
    pub(crate) fn core_by_keys(&mut self, bench: Entity) -> usize {
        let mut presses = 1;
        self.bench_f(bench);
        assert_eq!(self.h.stack_top(), Some(fitting::kind()));
        self.press(&mut presses, UiAction::TabNext);
        assert_eq!(self.h.stack_top(), Some(crafting::kind()));
        assert_eq!(
            self.focus_id(),
            "crafting.schem:schematic.cell_core",
            "the tab opens with the keyboard on the schematic shown"
        );
        self.press(&mut presses, UiAction::Accept);
        assert_eq!(
            self.focus_id(),
            crafting::BEST,
            "a schematic leads to Best stock"
        );
        self.press(&mut presses, UiAction::Accept);
        assert_eq!(self.focus_id(), crafting::ASSEMBLE, "and that to Assemble");
        assert!(self.craft().session.is_none(), "nothing is spent yet");
        self.press(&mut presses, UiAction::Accept);
        let session = self.craft().session.expect("assembled");
        assert_eq!(session.points, 3);
        assert_eq!(
            self.focus_id(),
            crafting::roll_id("charge_density", Hand::Steady),
            "and that to the first hand"
        );
        for left in [2, 1, 0] {
            self.press(&mut presses, UiAction::Accept);
            assert_eq!(self.craft().session.expect("still open").points, left);
        }
        assert_eq!(self.focus_id(), crafting::FINISH, "no points: on to Finish");
        self.press(&mut presses, UiAction::Accept);
        assert!(self.craft().session.is_none(), "finished");
        presses
    }
}

/// The ticket's done-when for the first half of the worked example: from
/// the workbench's F a fresh household makes a cell core by the keys alone
/// in fifteen presses or fewer, counted. The tab, the schematic, Best
/// stock, Assemble, a point a press and Finish: nine. The core is in the
/// pack with its batch, its maker and the lines the sim left it at; the
/// lots are down by what it took and by nothing else.
#[test]
fn the_core_is_crafted_by_keys_alone_in_fifteen_presses() {
    let (mut day, bench) = craft_day("craft-core-keys", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    let presses = day.core_by_keys(bench);
    assert!(presses <= 15, "{presses} presses");
    assert_eq!(presses, 9, "the path the design counts");
    let made = day.carried_of(CORE_ITEM);
    assert_eq!(made.len(), 1);
    let core = &made[0];
    assert_eq!((core.count, core.batch), (1, 1));
    assert_eq!(core.by.as_deref(), Some("Frontier ranger"));
    let state = day.craft();
    assert_eq!((state.rolls, state.batches), (3, 1));
    let density = core.lines["charge_density"];
    assert!((358..=597).contains(&density), "{density}");
    assert_eq!(core.lines["stability"], 209, "no point went on it");
    assert_eq!((day.units_of("Tolley"), day.units_of("Oxbow")), (140, 130));
    assert_eq!(day.units_of("Rake"), 200);
    // The finished card, with what went in and the way to another.
    let card = day.card_at(crafting::CARD_ID);
    assert_eq!(card.name, "Cell core");
    assert_eq!(card.kind, "Part");
    assert_eq!(card.from, "Crafted by Frontier ranger · batch 1");
    assert_eq!(card.lines[0].value, density);
    assert_eq!(
        card.lines[0].note,
        "Goes into: Capacity of a patrol extended magazine"
    );
    assert_eq!(
        card.notes.last().map(String::as_str),
        Some("Made from Tolley salt · Oxbow brine.")
    );
    assert_eq!(
        day.says(crafting::STATUS_ID),
        "Cell core batch 1 is in your pack."
    );
    assert!(
        day.toasted()
            .iter()
            .any(|toast| toast.contains("Cell core batch 1 is in your pack.")),
        "{:?}",
        day.toasted()
    );
    assert_eq!(
        day.focus_id(),
        "crafting.schem:schematic.cell_core",
        "Finish leads back to the row of what was made"
    );
    assert!(!day.inert(crafting::AGAIN));
    // The magazine can be made now, which its mark says.
    let mark = find(&day.h, "crafting.schem.mark:schematic.patrol_extended_feed");
    assert_eq!(
        day.h.world().get::<Tags>(mark).unwrap().get("state"),
        Some("Can be made now")
    );
}

/// The second half: with the core made, the patrol magazine from the
/// workbench's F by the keys alone, counted. F comes back on the crafting
/// tab, Up walks to the magazine's row (the press count adds the walk),
/// then the same Accepts with five points. The magazine holds the core, a harness, forty units of the
/// alloy and a seal set, its card says so, and fitted it gives the
/// `Combatant` the magazine the bench promised.
#[test]
#[allow(clippy::too_many_lines)]
fn the_magazine_is_crafted_by_keys_alone_and_counted() {
    let (mut day, bench) = craft_day("craft-magazine-keys", 1600.0, 900.0);
    day.h.set_input_mode(InputMode::Keyboard);
    day.core_by_keys(bench);
    let core = day.carried_of(CORE_ITEM)[0].clone();
    day.h.action(UiAction::Back);
    day.h.settle();
    assert!(day.h.stack().is_empty());

    let mut presses = 1;
    day.bench_f(bench);
    assert_eq!(
        day.h.stack_top(),
        Some(crafting::kind()),
        "F opens the tab last shown"
    );
    assert_eq!(day.focus_id(), "crafting.schem:schematic.cell_core");
    // The list leads with the weapons and the parts of a gun and of a blade,
    // so the magazine is above the core now: Up reaches it through the blade
    // parts and the gun parts, each a press.
    let feed = "crafting.schem:schematic.patrol_extended_feed";
    let mut walked = 0;
    while day.focus_id() != feed {
        assert!(walked < 20, "Up never reached the magazine");
        day.press(&mut presses, UiAction::Up);
        walked += 1;
    }
    day.press(&mut presses, UiAction::Accept);
    assert_eq!(day.focus_id(), crafting::BEST);
    // What it would be, before anything is spent, in the carbine's units.
    let promised = day.says(&format!("{}capacity", crafting::MEANS_TAG));
    assert!(promised.ends_with(" rounds"), "{promised}");
    assert_eq!(
        day.says(crafting::USES_ID),
        "Uses 1 cell core, 1 harness, 40 units of Rake light alloy, 1 seal set."
    );
    day.press(&mut presses, UiAction::Accept);
    day.press(&mut presses, UiAction::Accept);
    assert_eq!(day.craft().session.expect("assembled").points, 5);
    assert!(day.carried_of(CORE_ITEM).is_empty(), "the core is spent");
    for _ in 0..5 {
        day.press(&mut presses, UiAction::Accept);
    }
    assert_eq!(day.craft().session.expect("still open").points, 0);
    assert_eq!(day.focus_id(), crafting::FINISH);
    let stood = day.says(&format!("{}capacity", crafting::MEANS_TAG));
    day.press(&mut presses, UiAction::Accept);
    // Fifteen before the melee parts stood in the list, seven more after,
    // and one more for the rifle's field rest.
    assert_eq!(presses, 10 + walked);

    let made: Vec<Record> = day
        .carried_of(FEED)
        .into_iter()
        .filter(|record| record.batch == 2)
        .collect();
    assert_eq!(made.len(), 1);
    let magazine = &made[0];
    assert_eq!(magazine.by.as_deref(), Some("Frontier ranger"));
    let state = day.craft();
    assert_eq!((state.rolls, state.batches), (8, 2));
    let from: Vec<(&str, String, u32)> = state.made[&2]
        .from
        .iter()
        .map(|(slot, input)| (slot.as_str(), input.item.to_string(), input.count))
        .collect();
    assert_eq!(from[0], ("core", CORE_ITEM.to_owned(), 1));
    assert_eq!(from[1], ("harness", HARNESS.to_owned(), 1));
    assert_eq!(from[2].2, 40);
    assert_eq!(from[3], ("seals", SEALS.to_owned(), 1));
    assert_eq!(state.made[&2].from[0].1.lines, core.lines);
    assert_eq!((day.stored(HARNESS), day.stored(SEALS)), (2, 5));
    assert_eq!(day.units_of("Rake"), 160);
    let card = day.card_at(crafting::CARD_ID);
    assert_eq!(card.name, "Patrol extended magazine");
    assert_eq!(
        card.notes.last().map(String::as_str),
        Some("Made from cell core batch 1 · harness, store grade · Rake light alloy · seal set.")
    );
    // Fitted, the `Combatant` holds what the bench hand last said.
    day.h
        .world_mut()
        .resource_mut::<FitInbox>()
        .push(FitRequest::Fit {
            hand: 0,
            mount: "feed".to_owned(),
            module: strata_registry::item::Fitted {
                item: magazine.item.clone(),
                batch: magazine.batch,
                by: magazine.by.clone(),
                lines: magazine.lines.clone(),
                condition: None,
            },
        });
    day.tick();
    assert_eq!(format!("{} rounds", day.actor().magazine()), stood);
}

/// Every pick moves what comes out before anything is spent: the bar's
/// floor and ceiling, the sentence that says them, the rounds downstream
/// and what Assemble would use. The list is best first, a click picks,
/// and Best stock puts the best back.
#[test]
#[allow(clippy::too_many_lines)]
fn a_pick_changes_what_comes_out_before_assemble() {
    let (mut day, _) = craft_day("craft-pick", 1600.0, 900.0);
    let calder = lot_of(
        &day.registries.clone(),
        SALT,
        "Calder",
        80,
        &[640, 820, 450],
    );
    day.stock(&calder);
    let before = day.truth();
    day.open_crafting("cell_core");
    let line = |day: &Day, tag: &str| day.says(&format!("{tag}charge_density"));
    let floor = |day: &Day| {
        let bar = find(&day.h, &format!("{}charge_density", crafting::BAR_TAG));
        day.h.world().get::<QualityBar>(bar).unwrap().0
    };
    // The best stock is in the slots as the screen opens.
    assert_eq!(
        day.row(&crafting::open_id("salt")),
        [
            "Calder salt",
            "Potency",
            "820",
            "\u{b7}",
            "Stability",
            "450",
            "2 fit"
        ]
    );
    assert_eq!(
        line(&day, crafting::RANGE_TAG),
        "Assembles at 415, can be worked up to 691."
    );
    assert_eq!(floor(&day), 415);
    let better = line(&day, crafting::DOWNSTREAM_TAG);
    assert!(
        better.starts_with("In a patrol extended magazine: ") && better.ends_with(" rounds"),
        "{better}"
    );
    // The list, and the other salt.
    let open = find(&day.h, &crafting::open_id("salt"));
    day.h.click(open);
    day.h.settle();
    assert_eq!(day.row(&crafting::open_id("salt")).last().unwrap(), "Close");
    assert_eq!(
        day.row(&crafting::pick_id("salt", 0)),
        [
            "\u{203a} Calder salt",
            "BEST",
            "Potency",
            "820",
            "\u{b7}",
            "Stability",
            "450",
            "80 u"
        ],
        "the one picked is marked, and the first is the best"
    );
    assert_eq!(
        day.row(&crafting::pick_id("salt", 1)),
        [
            "Tolley salt",
            "Potency",
            "632",
            "\u{b7}",
            "Stability",
            "225",
            "160 u"
        ]
    );
    let tolley = find(&day.h, &crafting::pick_id("salt", 1));
    day.h.click(tolley);
    day.h.settle();
    assert_eq!(
        line(&day, crafting::RANGE_TAG),
        "Assembles at 358, can be worked up to 597."
    );
    assert_eq!(floor(&day), 358);
    assert_eq!(
        line(&day, crafting::DOWNSTREAM_TAG),
        "In a patrol extended magazine: 27 to 29 rounds"
    );
    assert_eq!(
        day.says(&format!("{}stability", crafting::DOWNSTREAM_TAG)),
        "Goes into nothing yet"
    );
    assert_eq!(
        day.says(crafting::USES_ID),
        "Uses 20 units of Tolley salt, 30 units of Oxbow brine."
    );
    assert_eq!(day.row(&crafting::open_id("salt"))[0], "Tolley salt");
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::pick_id("salt", 0)))
            .is_none()
    );
    assert!(!day.inert(crafting::ASSEMBLE));
    // Best stock puts the best back.
    let best = find(&day.h, crafting::BEST);
    day.h.click(best);
    day.h.settle();
    assert_eq!(day.row(&crafting::open_id("salt"))[0], "Calder salt");
    assert_eq!(floor(&day), 415);
    day.tick();
    assert!(day.craft().session.is_none());
    assert_eq!(day.truth(), before, "nothing was spent by looking");
}

/// A stack too small for its slot is listed where its figures put it,
/// dimmed, with how much there is and how much is wanted, and cannot be
/// picked.
#[test]
fn a_short_stack_is_dimmed_with_its_reason() {
    let (mut day, _) = craft_day("craft-short", 1600.0, 900.0);
    let calder = lot_of(
        &day.registries.clone(),
        SALT,
        "Calder",
        12,
        &[640, 820, 450],
    );
    day.stock(&calder);
    day.open_crafting("cell_core");
    assert_eq!(
        day.row(&crafting::open_id("salt"))[0],
        "Tolley salt",
        "the best there is enough of"
    );
    let open = find(&day.h, &crafting::open_id("salt"));
    day.h.click(open);
    day.h.settle();
    let short = crafting::pick_id("salt", 0);
    assert_eq!(
        day.row(&short),
        ["Calder salt", "BEST", "12 units, needs 20", "12 u"]
    );
    assert!(day.inert(&short));
    // The row is labelled by its name too; the words are the second.
    let name = day.h.find(&by::text("Calder salt").index(1));
    assert_eq!(
        day.h
            .world()
            .get::<slotted::theme::Themed>(name)
            .unwrap()
            .0
            .as_str(),
        "strata.crafting.pick.short"
    );
    let row = find(&day.h, &short);
    day.h.click(row);
    day.h.settle();
    assert_eq!(day.row(&crafting::open_id("salt"))[0], "Tolley salt");
    assert!(!day.inert(&crafting::pick_id("salt", 1)));
}

/// A slot that takes a bought part with none in the stock says so and
/// offers one at its price. The purse short, Buy is grey with what it
/// needs; pressed, the price leaves the purse in the tick, the part is in
/// the pack and in the slot.
#[test]
fn an_empty_slot_offers_to_buy_and_says_why_not() {
    let (mut day, _) = craft_day("craft-buy", 1600.0, 900.0);
    // The seal sets are gone.
    {
        let seals = day.id(SEALS);
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        let cell = store
            .0
            .slots()
            .iter()
            .position(|stack| stack.as_ref().is_some_and(|stack| stack.id == seals))
            .unwrap();
        store.0.set(cell, None);
        store.0.clear_changed();
    }
    day.h.world_mut().insert_resource(Purse(8));
    day.open_crafting("patrol_extended_feed");
    let none = format!("{}seals", crafting::NONE_TAG);
    let why = format!("{}seals", crafting::BUY_WHY_TAG);
    let buy = crafting::buy_id("seals");
    assert_eq!(day.says(&none), "Nothing in the pack or the store fits");
    assert_eq!(
        day.h
            .world()
            .get::<SemanticLabel>(find(&day.h, &buy))
            .unwrap()
            .0,
        "Buy 1 · 20 credits"
    );
    assert!(day.inert(&buy));
    assert_eq!(day.says(&why), "Needs 20 credits, you have 8");
    // A harness is in stock, so its slot offers none.
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::buy_id("harness")))
            .is_none()
    );
    // With the price in the purse.
    day.h.world_mut().insert_resource(Purse(2500));
    day.h.settle();
    assert!(!day.inert(&buy));
    assert_eq!(day.says(&why), "");
    let button = find(&day.h, &buy);
    day.h.click(button);
    day.tick();
    assert_eq!(day.purse(), 2480);
    assert_eq!(day.packed(SEALS), 1);
    assert_eq!(
        day.says(crafting::STATUS_ID),
        "Bought a seal set. 2,480 credits left."
    );
    assert!(
        day.h.try_find(&by::test_id(&buy)).is_none(),
        "nothing more to buy"
    );
    assert_eq!(
        day.row(&crafting::open_id("seals")),
        ["Seal set", "Bought in, nothing to weigh"],
        "the one there is, in its slot: no count to open"
    );
    assert_eq!(day.says(crafting::PURSE_ID), "2,480");
}

/// The odds of a hand are drawn as the five bars of the roll table, each
/// as tall as its chance, and nowhere written as a figure: nothing on the
/// bench hand is a percentage. Nothing says what a roll would come to
/// before it is pressed.
#[test]
fn the_odds_are_five_bars_and_never_a_percentage() {
    let (mut day, _) = craft_day("craft-odds", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    assert_eq!(day.says(crafting::POINTS_ID), "3 points left");
    for (hand, odds) in [
        (Hand::Steady, [5_u8, 55, 35, 5, 0]),
        (Hand::Bold, [25, 25, 25, 15, 10]),
    ] {
        for (bar, chance) in odds.into_iter().enumerate() {
            let entity = find(&day.h, &crafting::odds_id("charge_density", hand, bar));
            let rect = day.h.rect_of(entity);
            let tall = (f32::from(chance) * 0.4).max(1.);
            assert!(
                (rect.height() - tall).abs() < 0.6 && (rect.width() - 9.).abs() < 0.6,
                "{hand:?} bar {bar}: {rect:?}, wants {tall} tall"
            );
        }
        // Left to right, best first, standing on one line.
        let feet: Vec<f32> = (0..5)
            .map(|bar| {
                let id = crafting::odds_id("charge_density", hand, bar);
                day.h.rect_of(find(&day.h, &id)).max.y
            })
            .collect();
        assert!(
            feet.iter().all(|foot| (foot - feet[0]).abs() < 0.6),
            "{feet:?}"
        );
    }
    assert_eq!(
        day.says(&format!("{}charge_density:steady", crafting::OFFER_TAG)),
        "most often +60"
    );
    assert_eq!(
        day.says(&format!("{}charge_density:bold", crafting::OFFER_TAG)),
        "up to +96, can slip 24"
    );
    let said = texts_under(&day.h, find(&day.h, crafting::ROOT_ID));
    assert!(said.iter().all(|text| !text.contains('%')), "{said:?}");
    assert!(
        said.contains(
            &"Bars read left to right: exceptional, good, fair, poor, setback.".to_owned()
        )
    );
    for word in ["EXCEPTIONAL", "GOOD", "FAIR", "POOR", "SETBACK"] {
        assert!(
            said.iter().all(|text| !text.starts_with(word)),
            "no outcome is shown before a press: {said:?}"
        );
    }
    assert!(
        day.h
            .try_find(&by::test_id(&format!(
                "{}charge_density",
                crafting::WORD_TAG
            )))
            .is_none()
    );
}

/// A roll spends a point and moves its line, and says what it came to only
/// once the sim has answered: the word, the figure of the ceiling, the
/// points left. Its bar fills from the figure before to the figure after
/// over a third of a second.
#[test]
fn a_roll_moves_the_line_and_never_shows_its_outcome_before_the_press() {
    let (mut day, _) = craft_day("craft-roll", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    let stands = format!("{}charge_density", crafting::STANDS_TAG);
    let word = format!("{}charge_density", crafting::WORD_TAG);
    assert_eq!(day.says(&stands), "358 of 597");
    assert!((day.fill_of("charge_density") - 35.8).abs() < 0.01);
    let bold = find(&day.h, &crafting::roll_id("charge_density", Hand::Bold));
    day.h.click(bold);
    // The press alone shows nothing: the sim has not rolled.
    assert_eq!(day.craft().rolls, 0);
    assert!(day.h.try_find(&by::test_id(&word)).is_none());
    assert_eq!(day.says(crafting::POINTS_ID), "3 points left");
    for _ in 0..30 {
        if day.craft().rolls == 1 {
            break;
        }
        day.h.step(1);
    }
    let line = day.craft().session.expect("open").lines[0].clone();
    let (outcome, moved) = line.last.expect("rolled");
    assert_ne!(moved, 0);
    // A few frames in, the fill is on its way and the word is there.
    day.h.step(4);
    let said = day.says(&word);
    assert!(
        said.starts_with(&outcome.key().to_uppercase()),
        "{said} for {outcome:?}"
    );
    let (from, to) = (35.8, f32::from(line.figure()) / 10.);
    let partway = day.fill_of("charge_density");
    assert!(
        (partway - from).abs() > 0.01 && (partway - to).abs() > 0.01,
        "the fill is at {partway} between {from} and {to}"
    );
    assert!(partway > from.min(to) && partway < from.max(to));
    day.h.advance(std::time::Duration::from_millis(400));
    day.h.settle();
    assert!((day.fill_of("charge_density") - to).abs() < 0.01);
    assert_eq!(day.says(&stands), format!("{} of 597", line.figure()));
    assert_eq!(day.says(crafting::POINTS_ID), "2 points left");
    assert_eq!(
        day.role_of(&word),
        match outcome {
            strata_registry::schematic::Outcome::Exceptional
            | strata_registry::schematic::Outcome::Good => "strata.crafting.word.good",
            strata_registry::schematic::Outcome::Fair => "strata.crafting.word.fair",
            strata_registry::schematic::Outcome::Poor => "strata.crafting.word.poor",
            strata_registry::schematic::Outcome::Setback => "strata.crafting.word.setback",
        }
    );
    // The other line has said nothing and not moved.
    assert!(
        day.h
            .try_find(&by::test_id(&format!("{}stability", crafting::WORD_TAG)))
            .is_none()
    );
    assert_eq!(
        day.says(&format!("{}stability", crafting::STANDS_TAG)),
        "209 of 349"
    );
}

/// Holding a hand rolls again as each roll lands, until the points are
/// gone, and stops there: Finish is not pressed by a held key. A key held
/// from before the hand had the focus is no hold: the press comes first.
#[test]
fn holding_a_hand_repeats_the_roll() {
    let (mut day, _) = craft_day("craft-hold", 1600.0, 900.0);
    day.open_crafting("cell_core");
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    day.tab_to(crafting::ASSEMBLE);
    // Accept held through Assemble: assembled, and nothing rolled.
    day.h.hold(KeyCode::Enter);
    day.h.advance(std::time::Duration::from_millis(1500));
    day.h.settle();
    let state = day.craft();
    assert_eq!(state.session.as_ref().expect("assembled").points, 3);
    assert_eq!(
        state.rolls, 0,
        "a key held from before the press rolls nothing"
    );
    day.h.release(KeyCode::Enter);
    day.h.settle();
    assert_eq!(
        day.focus_id(),
        crafting::roll_id("charge_density", Hand::Steady)
    );
    // One press, held.
    day.h.hold(KeyCode::Enter);
    day.h.advance(std::time::Duration::from_millis(700));
    let partway = day.craft().rolls;
    assert!((1..3).contains(&partway), "{partway} rolls in 0.7 s");
    day.h.advance(std::time::Duration::from_millis(2500));
    day.h.settle();
    let state = day.craft();
    assert_eq!(state.rolls, 3, "a point a roll, one after the other");
    let session = state.session.expect("held, not finished");
    assert_eq!(session.points, 0);
    assert_eq!(day.focus_id(), crafting::FINISH);
    day.h.release(KeyCode::Enter);
    day.h.settle();
    assert!(day.craft().session.is_some());
    assert_eq!(day.says(crafting::POINTS_ID), "0 points left");
    assert_eq!(day.says(crafting::WHY_ID), "No points left.");
    assert_eq!(day.role_of(crafting::WHY_ID), "strata.fitting.why.hint");
}

/// Holding the pointer down on a hand rolls at once and again as each roll
/// lands, until the points are gone, and stops there with the cursor still
/// over the hand. A pointer's click is only made when it is let go, which
/// is no start for a hold, so the press itself is the roll and the click
/// that follows it rolls nothing more.
#[test]
fn holding_a_hand_with_the_pointer_repeats_the_roll() {
    let (mut day, _) = craft_day("craft-hold-pointer", 1600.0, 900.0);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    let hand = crafting::roll_id("charge_density", Hand::Steady);
    // A click is one roll: the press makes it and the release adds none.
    let steady = find(&day.h, &hand);
    day.h.click(steady);
    day.h.advance(std::time::Duration::from_millis(1500));
    day.h.settle();
    assert_eq!(day.craft().rolls, 1, "a click is one roll");
    assert_eq!(day.craft().session.expect("open").points, 2);

    // Down and held, the cursor where the hand is.
    let steady = find(&day.h, &hand);
    let at = day.h.center_of(steady);
    day.h.pointer_move_to(at);
    day.h.pointer_press(PointerButton::Primary);
    day.h.advance(std::time::Duration::from_millis(200));
    assert_eq!(
        day.craft().rolls,
        2,
        "the press is one roll, made before the pointer is let go"
    );
    day.h.advance(std::time::Duration::from_millis(2500));
    day.h.settle();
    let state = day.craft();
    assert_eq!(state.rolls, 3, "a point a roll, one after the other");
    assert_eq!(
        state.session.as_ref().expect("held, not finished").points,
        0
    );
    // The hand was rebuilt under the cursor by every roll and is still
    // there, inert now; the pointer has not moved.
    let rebuilt = find(&day.h, &hand);
    assert_ne!(rebuilt, steady, "the node is a new one");
    assert!(day.h.rect_of(rebuilt).contains(day.h.pointer_pos()));
    assert!(day.inert(&hand));
    day.h.advance(std::time::Duration::from_millis(800));
    assert_eq!(day.craft().rolls, 3, "and no more");
    day.h.pointer_release(PointerButton::Primary);
    day.h.settle();
    day.tick();
    let state = day.craft();
    assert_eq!(state.rolls, 3);
    assert!(state.session.is_some(), "Finish is not pressed by a hold");
    assert_eq!(day.says(crafting::POINTS_ID), "0 points left");
}

/// The carbine's row is listed for a crafter the Weaponsmith has not
/// taught, with its button grey and the box named in words; taught, the
/// same button is live.
#[test]
fn the_carbine_is_greyed_with_the_box_it_needs() {
    let (mut day, bench) = craft_day("craft-gate", 1600.0, 900.0);
    day.bench_f(bench);
    let player = day.player;
    let box_id = "weaponsmith.frames.1";
    let mut sheet = day
        .h
        .world_mut()
        .get_mut::<veyra_sim::progression::CharacterSheet>(player)
        .unwrap();
    assert!(sheet.learned.remove(box_id), "the day's sheet teaches it");
    // The list opens on what is taught, so All is chosen to list the rest;
    // the carbine is then shown by its row's own press.
    day.open_crafting("patrol_carbine");
    let id = crafting::schematic_named(&day.registries, "patrol_carbine").unwrap();
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::schem_id(&id)))
            .is_none(),
        "an untaught carbine is not on the Known list"
    );
    day.h
        .world_mut()
        .resource_mut::<crafting::CraftingState>()
        .filter = crafting::ListFilter::All;
    day.open_crafting("patrol_carbine");
    assert!(
        day.h
            .try_find(&by::test_id(&crafting::schem_id(&id)))
            .is_some(),
        "the carbine is listed on All"
    );
    assert!(day.inert(crafting::ASSEMBLE));
    assert_eq!(day.says(crafting::WHY_ID), "Needs Weaponsmith: Frames 1.");

    let mut sheet = day
        .h
        .world_mut()
        .get_mut::<veyra_sim::progression::CharacterSheet>(player)
        .unwrap();
    sheet.learned.insert(box_id.to_owned());
    day.h.step(2);
    day.h.settle();
    assert!(!day.inert(crafting::ASSEMBLE), "taught, it is live");
    assert_eq!(day.says(crafting::WHY_ID), "");
}

/// The list opens on Known, with a chip each for Known, Ready and All; a
/// press on a chip changes what is listed and the chip lit, and each group
/// is headed with its count.
#[test]
fn the_chips_change_what_the_list_shows() {
    let (mut day, bench) = craft_day("craft-chips", 1600.0, 900.0);
    day.bench_f(bench);
    let player = day.player;
    day.h
        .world_mut()
        .get_mut::<veyra_sim::progression::CharacterSheet>(player)
        .unwrap()
        .learned
        .remove("weaponsmith.frames.1");
    day.open_crafting("cell_core");
    let carbine = crafting::schematic_named(&day.registries, "patrol_carbine").unwrap();
    let row = by::test_id(&crafting::schem_id(&carbine));
    for filter in crafting::ListFilter::ALL {
        assert!(
            day.h.try_find(&by::test_id(&filter.chip_id())).is_some(),
            "a chip for {filter:?}"
        );
    }
    assert!(day.h.try_find(&row).is_none(), "untaught, so not on Known");
    day.h
        .click(find(&day.h, &crafting::ListFilter::All.chip_id()));
    day.h.settle();
    assert!(day.h.try_find(&row).is_some(), "All lists it");
    assert_eq!(
        day.h.world().resource::<crafting::CraftingState>().filter,
        crafting::ListFilter::All
    );
    let heads = texts_under(&day.h, find(&day.h, crafting::LIST_ID));
    assert!(
        heads.iter().any(|text| text.starts_with("WEAPONS  ·  ")),
        "a group is headed with its count: {heads:?}"
    );
    day.h
        .click(find(&day.h, &crafting::ListFilter::Ready.chip_id()));
    day.h.settle();
    assert!(
        day.h.try_find(&row).is_none(),
        "Ready drops what cannot be made"
    );
}

/// What a quiet frame costs the open crafting screen: one view made and
/// compared (what the screen did every frame before it declared its
/// inputs) against the dirty check (what it does now). Run with
/// `--ignored --nocapture` in release for figures worth reading.
#[test]
#[ignore = "a probe: it prints timings and asserts only that a quiet frame is quiet"]
fn probe_a_quiet_frame_of_the_crafting_screen() {
    use std::time::Instant;
    use strata_ui::sim_screen;

    const RUNS: u32 = 300;
    const CHECKS: u32 = 20_000;

    let (mut day, bench) = craft_day("craft-probe", 1600.0, 900.0);
    day.bench_f(bench);
    day.open_crafting("cell_core");
    day.h.settle();
    for _ in 0..30 {
        day.h.step(1);
    }
    let world = day.h.world_mut();
    let started = Instant::now();
    let mut equal = 0;
    for _ in 0..RUNS {
        let view = sim_screen::probe_view::<crafting::CraftingState>(world).expect("a view");
        if world.resource::<crafting::CraftingState>().view() == Some(&view) {
            equal += 1;
        }
    }
    let view_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(RUNS);
    assert_eq!(equal, RUNS, "the view of a quiet bench is the one on show");

    let mut check = sim_screen::probe_check::<crafting::CraftingState>();
    check.initialize(world);
    // Two runs to take in whatever the setup changed, then a quiet one.
    check.run((), world).unwrap();
    check.run((), world).unwrap();
    let started = Instant::now();
    for _ in 0..CHECKS {
        check.run((), world).unwrap();
    }
    let check_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(CHECKS);

    // The probe's own reads are let go by.
    day.h.step(5);
    let before = sim_screen::compared::<crafting::CraftingState>(day.h.world());
    let started = Instant::now();
    for _ in 0..100 {
        day.h.step(1);
    }
    let frame_us = started.elapsed().as_secs_f64() * 1e6 / 100.0;
    let compares = sim_screen::compared::<crafting::CraftingState>(day.h.world()) - before;
    println!(
        "crafting, quiet: view and compare {view_us:.1} us, dirty check {check_us:.2} us, whole harness frame {frame_us:.0} us, {compares} comparisons in 100 frames"
    );
    assert_eq!(compares, 0, "a quiet bench compares nothing");
    assert!(
        check_us * 20.0 < view_us,
        "the check should be far cheaper than the view"
    );
}

/// What one crafting answer costs the screen: the two definitions either
/// side of a roll, put on show in turn by `replace_screen` (every node
/// despawned and spawned) and by `patch_screen` (a keyed diff), each with the
/// frame that follows it. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a probe: it prints timings and asserts only that a patch is cheaper than a replace"]
fn probe_one_crafting_answer_replaced_and_patched() {
    use std::sync::Arc;
    use std::time::Instant;
    use strata_ui::sim_screen;

    const FLIPS: u32 = 200;

    let (mut day, bench) = craft_day("craft-answer-probe", 1600.0, 900.0);
    day.bench_f(bench);
    day.open_crafting("cell_core");
    let assemble = find(&day.h, crafting::ASSEMBLE);
    day.h.click(assemble);
    day.tick();
    day.h.settle();
    let first = sim_screen::build::<crafting::CraftingState>(day.h.world_mut()).expect("a def");
    let steady = find(&day.h, &crafting::roll_id("charge_density", Hand::Steady));
    day.h.click(steady);
    for _ in 0..30 {
        if day.craft().rolls == 1 {
            break;
        }
        day.h.step(1);
    }
    day.h.advance(std::time::Duration::from_millis(600));
    day.h.settle();
    let second = sim_screen::build::<crafting::CraftingState>(day.h.world_mut()).expect("a def");
    assert!(first != second, "a roll changes the screen");
    let nodes = day
        .h
        .world_mut()
        .query::<&Node>()
        .iter(day.h.world())
        .count();

    let mut time = |patch: bool| -> (f64, f64) {
        let mut total = 0.0;
        let mut apply_only = 0.0;
        for i in 0..FLIPS {
            let def = if i % 2 == 0 { &first } else { &second };
            let started = Instant::now();
            let world = day.h.world_mut();
            if patch {
                sim_screen::patch_top(world, def.clone(), true);
            } else {
                slotted::ui::replace_screen(&mut world.commands(), Arc::new(def.clone()), None);
            }
            world.flush();
            apply_only += started.elapsed().as_secs_f64();
            day.h.step(1);
            total += started.elapsed().as_secs_f64();
        }
        (
            total * 1e6 / f64::from(FLIPS),
            apply_only * 1e6 / f64::from(FLIPS),
        )
    };
    // One of each to warm, then the figures.
    time(false);
    time(true);
    let (replace_total, replace_apply) = time(false);
    let (patch_total, patch_apply) = time(true);
    println!(
        "one crafting answer over {nodes} nodes: replace {replace_total:.0} us (command {replace_apply:.0}), patch {patch_total:.0} us (command {patch_apply:.0})"
    );
    assert!(patch_total < replace_total);
}
