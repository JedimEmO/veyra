//! A screen that declares its inputs compares nothing while they are still
//! (client-app review, F1, step four): a screen left open for a while asks
//! the world no question, and one input moving asks it once.

use bevy::ecs::change_detection::DetectChangesMut;
use strata_registry::Namespaced;
use strata_ui::sim_screen::compared;
use veyra_sim::WorldPosition;
use veyra_sim::deposits::Purse;
use veyra_sim::progression::{Interact, Interaction, InteractionKind};
use veyra_ui::{board, crafting, factory, fitting, map};

use crate::common::*;
use crate::crafting::craft_day;
use crate::factory::factory_day;

/// Frames the open screen is left alone for.
const IDLE: usize = 60;

/// Lets what the opening itself changed be taken in, then leaves the
/// screen alone `IDLE` frames and counts its comparisons.
fn idle_comparisons<S: strata_ui::sim_screen::SimScreen>(day: &mut Day) -> u32 {
    day.h.step(6);
    let before = compared::<S>(day.h.world());
    day.h.step(IDLE);
    compared::<S>(day.h.world()) - before
}

#[test]
fn an_idle_crafting_screen_compares_nothing_and_a_new_purse_compares_once() {
    let (mut day, bench) = craft_day("quiet-crafting", 1600.0, 900.0);
    day.bench_f(bench);
    day.open_crafting("cell_core");
    day.h.settle();
    assert_eq!(idle_comparisons::<crafting::CraftingState>(&mut day), 0);
    let before = compared::<crafting::CraftingState>(day.h.world());
    day.h.world_mut().insert_resource(Purse(12));
    day.h.step(3);
    assert_eq!(
        compared::<crafting::CraftingState>(day.h.world()) - before,
        1
    );
}

#[test]
fn an_idle_fitting_screen_compares_nothing() {
    let (mut day, bench) = crate::fitting::bench_day("quiet-fitting", 1600.0, 900.0);
    day.h.world_mut().write_message(Interact {
        entity: bench,
        kind: InteractionKind::Workbench,
    });
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(fitting::kind()));
    assert_eq!(idle_comparisons::<fitting::FittingState>(&mut day), 0);
}

#[test]
fn an_idle_factory_screen_compares_nothing() {
    let (mut day, _, fabricator) = factory_day("quiet-factory", 1600.0, 900.0);
    day.h.world_mut().write_message(Interact {
        entity: fabricator,
        kind: InteractionKind::Fabricator,
    });
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(factory::kind()));
    assert_eq!(idle_comparisons::<factory::FactoryUi>(&mut day), 0);
}

#[test]
fn an_idle_board_compares_once_a_second_and_a_new_purse_once_more() {
    let mut day = first_day("quiet-board", 1600.0, 900.0);
    let buyer = Namespaced::parse("veyra:buyer.pelagic_works").unwrap();
    let post = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(2.0, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Board {
                    buyer: buyer.clone(),
                },
                radius: 3.0,
            },
        ))
        .id();
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    day.h.world_mut().write_message(Interact {
        entity: post,
        kind: InteractionKind::Board { buyer },
    });
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(board::kind()));
    // The board words a lease's time and a posting's expiry from the played
    // clock, which it reads by the whole second: a second of frames is one
    // comparison, not sixty.
    assert!(idle_comparisons::<board::BoardUi>(&mut day) <= 2);
    let before = compared::<board::BoardUi>(day.h.world());
    day.h.world_mut().insert_resource(Purse(5));
    day.h.step(3);
    assert_eq!(compared::<board::BoardUi>(day.h.world()) - before, 1);
}

#[test]
fn an_idle_map_compares_nothing_and_a_new_find_compares_once() {
    let mut day = crate::survey::survey_day("quiet-map", 1600.0, 900.0).0;
    day.h.key(bevy::prelude::KeyCode::KeyM);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(map::kind()));
    assert_eq!(idle_comparisons::<map::MapState>(&mut day), 0);
    let before = compared::<map::MapState>(day.h.world());
    day.h
        .world_mut()
        .resource_mut::<veyra_sim::deposits::Surveyed>()
        .set_changed();
    day.h.step(3);
    assert_eq!(compared::<map::MapState>(day.h.world()) - before, 1);
}
