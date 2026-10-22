//! The procurement board (ticket 0007, step 7): selling by the keys and
//! what a row says when it cannot.

use bevy::prelude::*;
use slotted::ui::{InputMode, UiAction};
use strata_registry::Namespaced;
use strata_registry::item::Record;
use veyra_sim::WorldPosition;
use veyra_sim::deposits::Purse;
use veyra_sim::progression::{Interact, Interaction, InteractionKind};
use veyra_ui::board;

use crate::common::*;
use crate::crafting::{ALLOY, CORE_ITEM, lot_of};

const BUYER: &str = "veyra:buyer.pelagic_works";

fn buyer() -> Namespaced {
    Namespaced::parse(BUYER).unwrap()
}

/// 100 cell cores of batch 412 at charge density 724.
fn cores() -> Record {
    Record {
        item: Namespaced::parse(CORE_ITEM).unwrap(),
        count: 100,
        batch: 412,
        by: Some("Tovan Reyes".to_owned()),
        lines: [("charge_density", 724), ("stability", 600)]
            .into_iter()
            .map(|(line, figure)| (line.to_owned(), figure))
            .collect(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: None,
    }
}

/// The first day with a board of Pelagic Works within reach and 100 cores
/// in the store, and the purse at 1,000.
fn board_day(name: &str, stocked: bool) -> (Day, Entity) {
    let mut day = first_day(name, 1600.0, 900.0);
    let post = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(2.0, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Board { buyer: buyer() },
                radius: 3.0,
            },
        ))
        .id();
    day.h.world_mut().insert_resource(Purse(1000));
    if stocked {
        day.stock(&cores());
    }
    // A tick, so the sim's reach has found the board.
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    (day, post)
}

impl Day {
    /// F at the board, as the keyboard raises it.
    fn read_board(&mut self, post: Entity) {
        self.h.world_mut().write_message(Interact {
            entity: post,
            kind: InteractionKind::Board { buyer: buyer() },
        });
        self.h.step(2);
        self.h.settle();
        assert_eq!(self.h.stack_top(), Some(board::kind()));
    }
}

#[test]
fn a_sale_by_the_keys_fills_the_line_and_fills_the_purse() {
    let (mut day, post) = board_day("board-keys", true);
    day.h.set_input_mode(InputMode::Keyboard);
    day.read_board(post);
    assert_eq!(day.says(board::PURSE_ID), "1,000");
    assert!(day.says("board.wants:cell_cores").contains("40"));
    let sell = find(&day.h, &board::sell_id("cell_cores"));
    // Focus starts on the first row that can sell, and travels down and up.
    assert_eq!(day.h.focused(), Some(sell));
    day.h.action(UiAction::Down);
    day.h.settle();
    let next = find(&day.h, &board::sell_id("patrol_magazines"));
    assert_eq!(
        day.h.focused(),
        Some(next),
        "{}",
        day.named(day.h.focused())
    );
    day.h.action(UiAction::Up);
    day.h.settle();
    assert_eq!(day.h.focused(), Some(sell));
    assert!(day.says(&board::sell_id("cell_cores")).contains("5,875"));
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.says(board::PURSE_ID), "6,875");
    assert_eq!(
        day.says("board.state:cell_cores"),
        "filled \u{b7} refills with time"
    );
    assert!(button_disabled(&day.h, &board::sell_id("cell_cores")));
    assert_eq!(day.says("board.why:cell_cores"), "filled");
    // After it, SOLD HERE names the count, the item and the batch.
    let sold = day.says("board.sold:0");
    assert!(
        sold.contains("40") && sold.contains("Cell cores") && sold.contains("0412"),
        "{sold}"
    );
    let left: u32 = day
        .store()
        .0
        .slots()
        .iter()
        .flatten()
        .filter(|stack| stack.id == day.id(CORE_ITEM))
        .map(|stack| stack.count)
        .sum();
    assert_eq!(left, 60);
}

#[test]
fn a_kind_under_the_minimum_is_grey_and_says_its_figure() {
    let (mut day, post) = board_day("board-short", true);
    let registries = day.registries.clone();
    let mut weak = lot_of(
        &registries,
        ALLOY,
        "Rake",
        200,
        &[533, 298, 312, 231, 451, 633, 165, 492],
    );
    let strength = weak
        .lines
        .keys()
        .find(|line| line.as_str() == "strength")
        .cloned()
        .expect("a light alloy lot has a strength line");
    weak.lines.insert(strength, 300);
    day.stock(&weak);
    day.read_board(post);
    let sell = board::sell_id("structural_metal");
    assert!(button_disabled(&day.h, &sell));
    let why = day.says("board.why:structural_metal");
    assert!(why.contains("not met") && why.contains("300"), "{why}");
    let yours = day.says("board.yours:structural_metal");
    assert!(
        yours.contains("not met: 300") && yours.contains("450"),
        "{yours}"
    );
    // A second lot of the class at 540 is offered by name and units.
    let mut strong = lot_of(
        &registries,
        ALLOY,
        "Harrow",
        200,
        &[533, 298, 312, 231, 451, 633, 165, 492],
    );
    strong.lines.insert("strength".to_owned(), 540);
    day.stock(&strong);
    day.tick();
    let yours = day.says("board.yours:structural_metal");
    assert!(
        yours.contains("540") && yours.contains("200 units"),
        "{yours}"
    );
    assert!(!button_disabled(&day.h, &sell));
}

#[test]
fn a_row_with_nothing_held_says_so_and_is_grey() {
    let (mut day, post) = board_day("board-none", false);
    day.read_board(post);
    let sell = board::sell_id("cell_cores");
    assert!(button_disabled(&day.h, &sell));
    assert_eq!(day.says("board.yours:cell_cores"), "none held");
    assert_eq!(day.says("board.why:cell_cores"), "none held");
}

const BIKE: &str = "veyra:vehicle.courier_bike";

fn buy_bike() -> String {
    board::buy_id(&Namespaced::parse(BIKE).unwrap())
}

#[test]
fn for_sale_lists_the_bike_with_its_price_and_the_purse_and_buys_it() {
    let (mut day, post) = board_day("board-buy", false);
    day.read_board(post);
    let buy = buy_bike();
    assert_eq!(day.says(&format!("board.price:{BIKE}")), "450 cr");
    assert_eq!(day.says(&format!("board.have:{BIKE}")), "purse 1,000 cr");
    assert!(
        !button_disabled(&day.h, &buy),
        "Buy is live with the credits"
    );
    let sold = day
        .registries
        .buyers
        .get(&buyer())
        .unwrap()
        .sells
        .iter()
        .map(|sale| sale.item.to_string())
        .collect::<Vec<_>>();
    assert!(sold.iter().any(|item| item == BIKE), "{sold:?}");
    let button = find(&day.h, &buy);
    day.h.set_focus(Some(button));
    day.h.settle();
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.says(board::PURSE_ID), "550");
    let id = day.id(BIKE);
    let held = day
        .pack()
        .cells
        .slots()
        .iter()
        .flatten()
        .any(|stack| stack.id == id);
    assert!(held, "the bike is in the pack");
    assert_eq!(day.says(&format!("board.have:{BIKE}")), "purse 550 cr");
}

#[test]
fn a_buy_that_cannot_be_made_is_grey_with_its_reason() {
    // Short of the price.
    let (mut day, post) = board_day("board-buy-short", false);
    day.h.world_mut().insert_resource(Purse(100));
    day.read_board(post);
    let buy = buy_bike();
    assert!(button_disabled(&day.h, &buy));
    assert_eq!(
        day.says(&format!("board.buywhy:{BIKE}")),
        "Short by 350 cr \u{b7} costs 450 cr"
    );
    // No room in the pack or the store.
    let (mut day, post) = board_day("board-buy-room", false);
    day.fill_the_pack();
    let items = &day.registries.items;
    let mut one = items.starting().unwrap().store[1].clone();
    one.count = 1;
    for cell in 0..day.store().0.len() {
        if day.store().0.get(cell).is_none() {
            one.batch += 1;
            let single = veyra_sim::items::write(&one, items).unwrap();
            let mut store = day
                .h
                .world_mut()
                .resource_mut::<veyra_sim::items::HouseholdStore>();
            store.0.set(cell, Some(single));
            store.0.clear_changed();
        }
    }
    day.read_board(post);
    assert!(button_disabled(&day.h, &buy_bike()));
    assert_eq!(
        day.says(&format!("board.buywhy:{BIKE}")),
        "No room in the pack or the store"
    );
}

/// Stepping away from the board with its screen up greys every row in
/// place and says why in the sim's words: the sale of the cores held and the
/// buy of the bike alike.
#[test]
fn a_board_row_away_from_the_board_is_grey_with_its_reason() {
    let (mut day, post) = board_day("board-away", true);
    day.read_board(post);
    let sell = board::sell_id("cell_cores");
    assert!(!button_disabled(&day.h, &sell), "live at the board");
    assert!(!button_disabled(&day.h, &buy_bike()));
    day.h
        .world_mut()
        .entity_mut(post)
        .insert(WorldPosition::new(300.0, 10.0, 0.0));
    day.tick();
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(board::kind()), "the screen stays");
    assert!(button_disabled(&day.h, &sell));
    assert_eq!(
        day.says("board.why:cell_cores"),
        "Out of reach of the board"
    );
    assert!(button_disabled(&day.h, &buy_bike()));
    assert_eq!(
        day.says(&format!("board.buywhy:{BIKE}")),
        "Out of reach of the board"
    );
}

/// Every order line and every sale row draws the image the pack draws for
/// what it names, never a bare name.
#[test]
fn every_board_row_carries_the_packs_image() {
    use veyra_ui::card::ItemImages;
    let (mut day, post) = board_day("board-images", true);
    day.read_board(post);
    let images = day.h.world().resource::<ItemImages>().0.clone();
    let view = day
        .h
        .world()
        .resource::<board::BoardUi>()
        .view()
        .cloned()
        .expect("the open board's view");
    assert!(!view.sells.is_empty() && !view.orders.is_empty());
    for sale in &view.sells {
        let swatch = sale.swatch.as_ref().expect("a sale row's swatch");
        let art = &images[&sale.item.to_string()];
        assert_eq!(
            swatch.image.as_deref(),
            Some(art.path.as_str()),
            "{}",
            sale.item
        );
    }
    for line in view.orders.iter().flat_map(|order| &order.lines) {
        let swatch = line.swatch.as_ref().expect("an order line's swatch");
        assert!(swatch.image.is_some(), "{} draws a stand-in", line.id);
    }
}
