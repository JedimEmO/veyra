//! The sheet's Loadout tab, by the keys alone: Tab to the tab and Enter,
//! Enter on a place, Enter on an ability, Enter on the place again. What
//! each Enter did is read from the bar the sim holds.

use bevy::prelude::*;
use slotted::ui::InputMode;
use slotted_test::prelude::*;
use strata_client::ui;
use strata_client::ui::sheet::loadout;
use strata_registry::GameRegistries;
use strata_registry::progression::{SkillGraph, XpKind};
use strata_sim::ability::{Bar, Place};
use strata_sim::combat::{Combatant, Weapon};
use strata_sim::progression::CharacterSheet;
use strata_sim::{LocalPlayer, Player, PlayerId, PlayerIntent, Registries, WorldPosition};

use crate::common::*;

fn tab_to(h: &mut UiHarness, id: &str) {
    for _ in 0..80 {
        if h.focused() == h.try_find(&by::test_id(id)) {
            return;
        }
        h.focus_next();
        h.settle();
    }
    panic!("Tab did not reach {id}");
}

/// The sim takes the request on its next tick and the sheet is rebuilt
/// from the bar it left.
fn tick(h: &mut UiHarness) {
    h.advance(std::time::Duration::from_millis(120));
    h.settle();
}

fn ranger(h: &mut UiHarness) -> Entity {
    let mut registries = GameRegistries::default();
    registries.progression = SkillGraph::shipped();
    h.world_mut()
        .insert_resource(Registries(std::sync::Arc::new(registries)));
    let mut sheet = CharacterSheet::default();
    for id in ["blade.novice", "marksman.novice", "marksman.carbine.1"] {
        sheet.learned.insert(id.to_owned());
    }
    sheet.credit(XpKind::Ranged, 150);
    let player = h
        .world_mut()
        .spawn((
            Player { id: PlayerId(4) },
            LocalPlayer,
            Name::new("Test ranger"),
            WorldPosition::new(0.0, 10.0, 0.0),
            PlayerIntent::default(),
            Combatant::default(),
            sheet,
        ))
        .id();
    let mut bar = Bar::default();
    bar.granted.insert("aimed_burst".to_owned());
    bar.granted.insert("butt_strike".to_owned());
    bar.granted.insert("heavy_cut".to_owned());
    h.world_mut().entity_mut(player).insert(bar);
    tick(h);
    player
}

#[test]
fn the_loadout_fills_and_clears_a_place_by_keys_alone() {
    let mut h = harness(temp_settings("loadout"));
    let player = ranger(&mut h);
    let carbine_q = Place::Weapon(Weapon::Carbine, 0);
    let held = |h: &UiHarness| {
        h.world()
            .get::<Bar>(player)
            .unwrap()
            .get(carbine_q)
            .map(str::to_owned)
    };

    h.key(ui::sheet::KEY);
    h.settle();
    h.set_input_mode(InputMode::Keyboard);
    tab_to(&mut h, ui::sheet::LOADOUT_TAB);
    h.key(KeyCode::Enter);
    h.settle();
    assert!(
        h.try_find(&by::test_id(&loadout::place_id(carbine_q)))
            .is_some(),
        "the tab shows the carbine's places"
    );
    let before = held(&h);

    // Enter on a place puts it in focus as the one being filled and the
    // keyboard moves to KNOWN.
    tab_to(&mut h, &loadout::place_id(carbine_q));
    h.key(KeyCode::Enter);
    h.settle();
    let on_known = h
        .try_find(&by::test_id(&loadout::known_id("aimed_burst")))
        .expect("the ability is listed for a carbine place");
    assert!(
        h.try_find(&by::test_id(&loadout::known_id("heavy_cut")))
            .is_some(),
        "a blade ability stays listed with its reason"
    );
    assert!(
        h.world().get::<Children>(on_known).is_some(),
        "the row is built"
    );

    // Enter on the ability asks the sim, which puts it there.
    tab_to(&mut h, &loadout::known_id("aimed_burst"));
    h.key(KeyCode::Enter);
    h.settle();
    tick(&mut h);
    assert_eq!(held(&h).as_deref(), Some("aimed_burst"), "was {before:?}");
    let notice = h.text_of(find(&h, loadout::NOTICE_ID)).unwrap();
    assert_eq!(notice, "Aimed burst is in Q of Carbine.");
    assert_eq!(
        h.focused(),
        Some(find(&h, &loadout::place_id(carbine_q))),
        "the keyboard is back on the place"
    );

    // Enter on the filled place that is the one being filled clears it.
    h.key(KeyCode::Enter);
    h.settle();
    tick(&mut h);
    assert_eq!(held(&h), None);
    let notice = h.text_of(find(&h, loadout::NOTICE_ID)).unwrap();
    assert_eq!(notice, "Q of Carbine is empty.");
}

#[test]
fn a_press_on_a_filled_place_survives_to_its_release() {
    use bevy::picking::pointer::PointerButton;
    use strata_sim::ability::{BarInbox, BarRequest};
    let mut h = harness(temp_settings("loadout-click"));
    let player = ranger(&mut h);
    let carbine_q = Place::Weapon(Weapon::Carbine, 0);
    h.world_mut()
        .resource_mut::<BarInbox>()
        .0
        .push(BarRequest::Set {
            place: carbine_q,
            ability: "aimed_burst".to_owned(),
        });
    tick(&mut h);
    assert_eq!(
        h.world().get::<Bar>(player).unwrap().get(carbine_q),
        Some("aimed_burst")
    );
    h.key(ui::sheet::KEY);
    h.settle();
    tab_to(&mut h, ui::sheet::LOADOUT_TAB);
    h.key(KeyCode::Enter);
    h.settle();
    // Pointing at it puts its card up, which rebuilds the screen; the press
    // that follows is on the card that is there once that has settled.
    let at = h.center_of(find(&h, &loadout::place_id(carbine_q)));
    h.pointer_move_to(at);
    h.advance(std::time::Duration::from_millis(80));
    h.settle();
    let place = find(&h, &loadout::place_id(carbine_q));
    h.pointer_press(PointerButton::Primary);
    // Focus moves with the press; the card must still be there to release on.
    h.advance(std::time::Duration::from_millis(80));
    h.settle();
    assert_eq!(
        find(&h, &loadout::place_id(carbine_q)),
        place,
        "the pressed card was not replaced under the pointer"
    );
    h.pointer_release(PointerButton::Primary);
    h.settle();
    h.advance(std::time::Duration::from_millis(80));
    h.settle();
    assert_eq!(
        h.world().resource::<loadout::LoadoutState>().filling,
        Some(carbine_q),
        "the click made it the place being filled"
    );
    assert!(
        h.try_find(&by::test_id(&loadout::known_id("heavy_cut")))
            .is_some(),
        "KNOWN is listed for the place"
    );
}

#[test]
fn l_opens_the_loadout_and_lists_what_the_trees_still_teach() {
    let mut h = harness(temp_settings("loadout-l"));
    let player = ranger(&mut h);
    h.key(ui::sheet::LOADOUT_KEY);
    h.settle();
    let carbine_q = Place::Weapon(Weapon::Carbine, 0);
    assert!(
        h.try_find(&by::test_id(&loadout::place_id(carbine_q)))
            .is_some(),
        "L opens the sheet on the Loadout tab"
    );
    assert!(
        h.try_find(&by::test_id(ui::sheet::PROFESSIONS_TAB))
            .is_some()
            && h.try_find(&by::test_id(ui::sheet::LOADOUT_TAB)).is_some(),
        "the two tabs are in one strip"
    );
    // Everything a box still grants and the member has not learned is
    // listed, greyed, with where it is taught.
    let graph = SkillGraph::shipped();
    let sheet = h.world().get::<CharacterSheet>(player).unwrap().clone();
    let left = strata_sim::ability::unlearned(&graph, &sheet);
    assert!(!left.is_empty());
    for row in &left {
        assert!(
            h.try_find(&by::test_id(&loadout::locked_id(&row.ability)))
                .is_some(),
            "{} is under NOT LEARNED",
            row.ability
        );
    }
    // L again closes it, as C does.
    h.key(ui::sheet::LOADOUT_KEY);
    h.settle();
    assert!(
        h.try_find(&by::test_id(&loadout::place_id(carbine_q)))
            .is_none()
    );
}

#[test]
fn pointing_at_an_ability_shows_its_card_and_chooses_nothing() {
    let mut h = harness(temp_settings("loadout-point"));
    let player = ranger(&mut h);
    let before = h.world().get::<Bar>(player).unwrap().clone();
    h.key(ui::sheet::LOADOUT_KEY);
    h.settle();
    let heavy = find(&h, &loadout::known_id("heavy_cut"));
    h.hover(heavy);
    h.advance(std::time::Duration::from_millis(80));
    h.settle();
    let state = h.world().resource::<loadout::LoadoutState>();
    assert_eq!(state.focus.as_deref(), Some("heavy_cut"));
    assert_eq!(state.filling, None, "pointing picks no place");
    // An ability not learned shows its card the same way.
    let graph = SkillGraph::shipped();
    let sheet = h.world().get::<CharacterSheet>(player).unwrap().clone();
    let locked = strata_sim::ability::unlearned(&graph, &sheet)
        .first()
        .unwrap()
        .ability
        .clone();
    let row = find(&h, &loadout::locked_id(&locked));
    h.hover(row);
    h.advance(std::time::Duration::from_millis(80));
    h.settle();
    let state = h.world().resource::<loadout::LoadoutState>();
    assert_eq!(state.focus.as_deref(), Some(locked.as_str()));
    tick(&mut h);
    assert_eq!(*h.world().get::<Bar>(player).unwrap(), before, "no request");
}

/// The toast follows the bar's change whichever of the cue and the change
/// the frame sees first.
#[test]
fn a_learned_ability_is_toasted_with_its_place() {
    let mut h = harness(temp_settings("loadout-toast"));
    let player = ranger(&mut h);
    let tables = strata_sim::combat_tables::current();
    let row = tables
        .abilities
        .abilities
        .iter()
        .find(|row| {
            !h.world()
                .get::<Bar>(player)
                .unwrap()
                .known
                .contains(&row.id)
        })
        .expect("an ability the ranger has not got");
    h.world_mut()
        .write_message(ui::progress::ProgressCue::Learned);
    h.settle();
    {
        let mut bar = h.world_mut().get_mut::<Bar>(player).unwrap();
        bar.known.insert(row.id.clone());
        bar.set(Place::Weapon(Weapon::Carbine, 0), &row.id);
    }
    h.advance(std::time::Duration::from_millis(120));
    h.settle();
    let words = format!("Learned {}. On Q.", row.name);
    let said: Vec<String> = h
        .toasts()
        .into_iter()
        .map(|toast| {
            let world = h.world();
            let mut said = String::new();
            let mut below = vec![toast];
            while let Some(at) = below.pop() {
                if let Some(text) = world.get::<Text>(at) {
                    said.push_str(&text.0);
                }
                if let Some(span) = world.get::<bevy::text::TextSpan>(at) {
                    said.push_str(&span.0);
                }
                if let Some(children) = world.get::<Children>(at) {
                    below.extend(children.iter().rev());
                }
            }
            said
        })
        .collect();
    assert!(
        said.iter().any(|text| text.contains(&words)),
        "toasts: {said:?}"
    );
}
