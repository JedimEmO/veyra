//! The map (ticket 0011, step 3): M, the marks, the panel, the scales.

use bevy::prelude::*;
use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted_test::prelude::*;
use veyra_sim::deposits::{Surveyed, lot};
use veyra_sim::harvest::{Harvester, Harvesters};
use veyra_ui::map::{self, MapState, MarkKey, Scale};
use veyra_view::combat::CombatCamera;

use crate::common::*;
use crate::survey::survey_day;

/// A day with four deposits found, then the last, Sorrel, forgotten: it is
/// near enough to list and nowhere in what the household found.
fn mapped_day(name: &str) -> (Day, crate::survey::Ground) {
    let (mut day, _, ground) = survey_day(name, 1600.0, 900.0);
    day.h
        .world_mut()
        .resource_mut::<Surveyed>()
        .found
        .remove(&ground.sorrel.id);
    (day, ground)
}

fn mark_of(day: &Day, key: &MarkKey) -> Option<Entity> {
    day.h.try_find(&by::test_id(&key.id()))
}

/// M opens the map over the world and M closes it from the top.
#[test]
fn m_opens_the_map_and_m_closes_it() {
    let (mut day, _) = mapped_day("map-m");
    assert_eq!(day.h.stack_top(), None);
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(map::kind()));
    assert!(day.h.try_find(&by::test_id(map::FRAME_ID)).is_some());
    assert!(
        day.h.try_find(&by::test_id(map::CHARTING_ID)).is_some(),
        "no picture has landed, so the screen says it is charting"
    );
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    assert_eq!(day.h.stack_top(), None);
}

/// The ground is right-handed: seen from above with north up, `+x` lies to
/// the west, so a deposit at `+x` is left of the member; and a camera
/// facing `-x` (east) turns the arrow clockwise, to the right. A planted
/// harvester has its mark.
#[test]
fn east_is_right_the_arrow_turns_with_the_member_and_a_harvester_is_marked() {
    let (mut day, ground) = mapped_day("map-east");
    day.h.world_mut().spawn((
        CombatCamera::default(),
        Transform::default().looking_to(Vec3::NEG_X, Vec3::Y),
    ));
    let stack = lot(&ground.harrow, &day.registries.resources, 1).unwrap();
    let mut harvesters = Harvesters::default();
    harvesters.list.push(Harvester {
        id: 7,
        deposit: ground.harrow.id,
        at: [800.0, 800.0],
        pm: 500,
        item: stack.clone(),
        by: String::new(),
        rate: 1,
        hopper: 10,
        lot: stack,
        held: 10,
        carry: 0,
        full: false,
    });
    day.h.world_mut().insert_resource(harvesters);
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    let view = day.h.world().resource::<MapState>().view().unwrap();
    let harrow = view
        .marks
        .iter()
        .find(|mark| mark.key == MarkKey::Deposit(ground.harrow.id))
        .unwrap();
    assert!(
        harrow.at[0] < view.member[0],
        "+x is west, the picture's left"
    );
    assert!(harrow.at[1] < view.member[1], "north is up");
    assert!(
        (view.heading - std::f32::consts::FRAC_PI_2).abs() < 0.1,
        "facing east turns the arrow right: {}",
        view.heading
    );
    assert!(
        view.marks
            .iter()
            .any(|mark| mark.key == MarkKey::Harvester(7))
    );
    assert!(mark_of(&day, &MarkKey::Harvester(7)).is_some());
    // A machine at work moves its carry every tick, which no mark shows:
    // the screen is not rebuilt for it.
    let views = day.h.world().resource::<MapState>().views();
    for step in 1..=3 {
        day.h.world_mut().resource_mut::<Harvesters>().list[0].carry = step;
        day.tick();
        day.h.settle();
    }
    assert_eq!(day.h.world().resource::<MapState>().views(), views);
}

/// A found deposit has a mark and an unfound one has none, however near.
#[test]
fn a_found_deposit_has_a_mark_and_an_unfound_one_has_none() {
    let (mut day, ground) = mapped_day("map-marks");
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    for found in [&ground.calder, &ground.rake, &ground.harrow] {
        assert!(
            mark_of(&day, &MarkKey::Deposit(found.id)).is_some(),
            "{} is found",
            found.place
        );
    }
    assert!(
        mark_of(&day, &MarkKey::Deposit(ground.sorrel.id)).is_none(),
        "Sorrel is not found"
    );
    let view = day.h.world().resource::<MapState>().view().unwrap();
    assert!(
        view.marks
            .iter()
            .all(|mark| mark.key != MarkKey::Deposit(ground.sorrel.id))
    );
}

/// Focusing a deposit fills the panel, and Enter tracks it.
#[test]
fn focusing_a_deposit_fills_the_panel_and_enter_tracks_it() {
    let (mut day, ground) = mapped_day("map-panel");
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.set_focus(None);
    day.h.settle();
    assert_eq!(
        day.says("map.panel.title"),
        "Point at a mark, or move the focus onto one."
    );
    let harrow = mark_of(&day, &MarkKey::Deposit(ground.harrow.id)).unwrap();
    day.h.set_focus(Some(harrow));
    day.h.settle();
    assert_eq!(day.says("map.panel.title"), ground.harrow.place);
    assert!(day.says("map.panel.kind").contains("Evaporite salt"));
    assert!(day.says("map.panel.lies").contains("north-west"));
    assert_eq!(day.says("map.panel.action"), "Enter tracks it");
    assert_eq!(day.h.world().resource::<Surveyed>().tracked, None);
    day.h.action(UiAction::Accept);
    day.tick();
    day.h.settle();
    let tracked = day.h.world().resource::<Surveyed>().tracked.clone();
    assert_eq!(tracked.map(|deposit| deposit.id), Some(ground.harrow.id));
    assert_eq!(day.says("map.panel.action"), "Tracked. Enter lets it go");
    let view = day.h.world().resource::<MapState>().view().unwrap();
    assert!(
        view.marks
            .iter()
            .any(|mark| mark.key == MarkKey::Deposit(ground.harrow.id) && mark.tracked)
    );
    assert!(
        day.h.focused().is_some(),
        "the keyboard stays on the map after the rebuild"
    );
}

/// The scale chips change the span and ask for a picture of it.
#[test]
fn the_scale_chips_change_the_span_and_ask_for_a_picture() {
    let (mut day, _) = mapped_day("map-scale");
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    assert_eq!(day.h.world().resource::<MapState>().scale(), Scale::Region);
    assert_eq!(
        day.h
            .world()
            .resource::<MapState>()
            .asked(Scale::Region)
            .map(|chart| chart.span_m),
        Some(map::REGION_SPAN_M)
    );
    for (scale, span) in [
        (Scale::Far, map::FAR_SPAN_M),
        (Scale::Near, map::NEAR_SPAN_M),
    ] {
        let chip = find(&day.h, &format!("{}{}", map::SCALE_TAG, scale.key()));
        day.h.click(chip);
        day.h.settle();
        let state = day.h.world().resource::<MapState>();
        assert_eq!(state.scale(), scale);
        assert_eq!(state.asked(scale).map(|chart| chart.span_m), Some(span));
        assert_eq!(state.view().map(|view| view.chart.span_m), Some(span));
        assert_eq!(day.h.stack_top(), Some(map::kind()));
    }
}

/// The Holders chip is off to begin with, turns the layer on with a mark in
/// the chip's word, and a second click turns it off again.
#[test]
fn the_holders_chip_toggles_the_layer() {
    let (mut day, _) = mapped_day("map-holders");
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    assert!(!day.h.world().resource::<MapState>().holders());
    assert_eq!(day.family_of(map::HOLDERS_ID), "strata.stores.chip");
    let chip = find(&day.h, map::HOLDERS_ID);
    day.h.click(chip);
    day.h.settle();
    assert!(day.h.world().resource::<MapState>().holders());
    assert!(day.h.world().resource::<MapState>().view().unwrap().holders);
    assert_eq!(day.family_of(map::HOLDERS_ID), "strata.stores.chip.on");
    // Nobody's ground is in view in a world with no factions, and the
    // legend says so instead of listing none.
    assert_eq!(day.h.stack_top(), Some(map::kind()));
    let chip = find(&day.h, map::HOLDERS_ID);
    day.h.click(chip);
    day.h.settle();
    assert!(!day.h.world().resource::<MapState>().holders());
}

/// The frame is the same size with the panel empty and with it full.
#[test]
fn the_frame_is_the_same_size_with_the_panel_empty_and_full() {
    let (mut day, ground) = mapped_day("map-frame");
    day.h.key(KeyCode::KeyM);
    day.h.settle();
    day.h.set_input_mode(InputMode::Keyboard);
    let sizes = |day: &Day| {
        [map::ROOT_ID, map::PANEL_ID, map::FRAME_ID]
            .map(|id| day.h.rect_of(find(&day.h, id)).size())
    };
    day.h.set_focus(None);
    day.h.settle();
    assert_eq!(
        day.says("map.panel.title"),
        "Point at a mark, or move the focus onto one."
    );
    let empty = sizes(&day);
    let rake = mark_of(&day, &MarkKey::Deposit(ground.rake.id)).unwrap();
    day.h.set_focus(Some(rake));
    day.h.settle();
    assert_eq!(day.says("map.panel.title"), ground.rake.place);
    assert_eq!(sizes(&day), empty);
}
