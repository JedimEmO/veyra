//! The item card: the pinned pane following focus, no hover card over it, and
//! batches told apart by their pips.

use bevy::prelude::*;
use slotted::ui::SemanticLabel;
use slotted::ui::TooltipTier;
use slotted::ui::def::UiNodeDef;
use slotted::ui::def::WidgetKind;
use strata_client::ui::card;
use strata_client::ui::card::CardContext;
use strata_client::ui::card::CardParams;
use strata_client::ui::card::CardPart;
use strata_client::ui::card::ItemCard;
use strata_client::ui::card::QualityBar;
use strata_client::ui::stores;
use strata_client::ui::stores::SlotDressing;
use strata_client::ui::stores::StoresState;

use crate::common::*;

/// Every number a string holds, as written.
pub(crate) fn numbers(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|word| word.trim_matches('.'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The card pinned in the pane is the focused slot's, the long one, with a
/// bar a quality line; it follows the pointer too. It reads in the game's
/// units against the weapons carried: a module's lines are what the
/// weapon would have with it and the change from what its mount holds, a
/// weapon's are its magazine, reload and interval.
#[test]
#[allow(clippy::too_many_lines)]
fn the_pinned_card_follows_focus_and_reads_in_the_games_units() {
    let mut day = first_day("stores-card", 1600.0, 900.0);
    day.use_locker();
    let card = find(&day.h, stores::CARD_ID);
    let pinned = |day: &Day| day.h.world().get::<ItemCard>(card).unwrap().0.clone();
    let bars = |day: &mut Day| {
        let world = day.h.world_mut();
        let mut bars = world.query::<(Entity, &QualityBar)>();
        let found: Vec<(Entity, u16)> = bars.iter(world).map(|(e, bar)| (e, bar.0)).collect();
        let mut parents = world.query::<&ChildOf>();
        found
            .into_iter()
            .filter(|(entity, _)| {
                let mut at = *entity;
                loop {
                    if at == card {
                        return true;
                    }
                    match parents.get(world, at) {
                        Ok(parent) => at = parent.parent(),
                        Err(_) => return false,
                    }
                }
            })
            .map(|(_, value)| value)
            .collect::<Vec<u16>>()
    };
    let effects = |shown: &CardParams| -> Vec<String> {
        shown
            .effects
            .iter()
            .map(|effect| format!("{} {} {}", effect.name, effect.value, effect.change))
            .collect()
    };

    // The focus starts on the bought magazines: what one would do to the
    // carbine in hand, in rounds and seconds, the longer reload marked.
    let shown = pinned(&day);
    assert!(shown.framed, "in the pane the card has its own frame");
    assert_eq!(shown.name, "Patrol extended magazine");
    assert_eq!(shown.from, "Store grade · Vigil · 4 in the stack");
    assert_eq!(
        effects(&shown),
        ["Magazine 29 rounds +5", "Reload 1.49 s +0.14 s worse"]
    );
    assert_eq!(
        shown
            .effects
            .iter()
            .map(|effect| effect.better)
            .collect::<Vec<_>>(),
        [Some(true), Some(false)]
    );
    assert_eq!(bars(&mut day), vec![500, 500]);
    assert!(
        shown
            .notes
            .contains(&"Against the standard feed on the patrol carbine in hand.".to_owned()),
        "{:?}",
        shown.notes
    );
    assert!(
        shown
            .notes
            .iter()
            .any(|note| note.contains("is 500, the tick on each bar")),
        "the pinned card is the long one: {:?}",
        shown.notes
    );
    // Painted: every line of the card is a text under it, and no factor.
    let painted = texts_under(&day.h, card).join("\n");
    for words in ["Magazine", "29 rounds", "+5", "+0.14 s worse", "Capacity"] {
        assert!(
            painted.contains(words),
            "{words} is not on the card: {painted}"
        );
    }
    assert!(!numbers(&painted).contains("1.2"), "{painted}");

    // The crafted batch beside them: the ticket's card.
    let store1 = day.cell("store", 1);
    day.h.set_focus(Some(store1));
    day.h.settle();
    let shown = pinned(&day);
    assert_eq!(shown.kind, "Mod · Feed");
    assert_eq!(
        shown.from,
        "Crafted by Tovan Reyes · batch 412 · 2 in the stack"
    );
    assert_eq!(bars(&mut day), vec![610, 433]);
    assert!(
        shown
            .notes
            .contains(&"Fits the patrol carbine's feed".to_owned())
    );

    // The carbine in hand: a weapon's card, with the row it fights by.
    let hand = find(&day.h, &stores::hand_slot_id(0));
    day.h.set_focus(Some(hand));
    day.h.settle();
    let shown = pinned(&day);
    assert_eq!(shown.name, "Vigil patrol carbine");
    assert_eq!(shown.kind, "Weapon");
    assert_eq!(shown.condition, Some(1000));
    assert_eq!(bars(&mut day), vec![500, 500, 500]);
    assert_eq!(
        effects(&shown),
        [
            "Magazine 24 rounds ",
            "Reload 1.35 s ",
            "Pause between bursts 0.462 s ",
            "Fire Burst of 3, Hold for full auto ",
            "Falloff Full to 25 m, 55% at 50 m ",
            "Reach 70 m "
        ]
    );
    let spec = strata_sim::combat::Weapon::Carbine.spec();
    assert!(
        numbers(&shown.text()).contains(&spec.magazine.to_string()),
        "the tables' own magazine: {}",
        shown.text()
    );
    assert_eq!(
        day.says(stores::NOTE_ID),
        "Click a hand, or Enter on it, opens its fitting."
    );
    assert!(
        day.h.is_visible(find(&day.h, stores::NOTE_ID)),
        "a weapon in hand shows the way to its fitting where the button was"
    );

    // An empty cell takes no pin: the way from a stack to the send button
    // lies across cells, and the card stays with the stack on the way.
    let empty = day.cell("store", 30);
    day.h.set_focus(Some(empty));
    day.h.settle();
    assert_eq!(day.pinned(), Some(hand));
    assert_eq!(pinned(&day).name, "Vigil patrol carbine");

    // The pointer pins as well. A part the fight has no number for yet is
    // worded and tagged, beside the one line of it that is live.
    let store3 = day.cell("store", 3);
    day.h.hover(store3);
    day.h.advance(std::time::Duration::from_millis(200));
    day.h.settle();
    let shown = pinned(&day);
    assert_eq!(shown.name, "Vigil reflex sight");
    assert_eq!(
        effects(&shown),
        ["Aimed spread 1.40° −0.60°", "Draw 0.46 s +0.01 s worse"]
    );
    assert_eq!(
        shown
            .effects
            .iter()
            .map(|effect| effect.later)
            .collect::<Vec<_>>(),
        [false, false]
    );
    assert!(
        !texts_under(&day.h, card)
            .iter()
            .any(|words| words == "NOT IN THE FIGHT YET")
    );
}

/// With the card pinned in the pane, pointing at a stack raises no second
/// card beside it: the pane already shows that stack's, and a raised one
/// would lie over the cells next to it. Away from such a screen the same
/// part is slotted's tooltip, the short card on hover and the long one
/// with Shift.
#[test]
fn no_card_is_raised_over_the_pinned_pane() {
    for with_store in [true, false] {
        let mut day = first_day("stores-quiet", 1600.0, 900.0);
        let cell = if with_store {
            day.use_locker();
            day.cell("store", 0)
        } else {
            day.h.key(stores::KEY);
            day.h.settle();
            day.cell(&stores::bag_region(0), 0)
        };
        assert!(day.shows(cell).is_some(), "a stack to point at");
        day.h.hover(cell);
        // Past the tooltip's delay, with and without Shift.
        day.h.advance(std::time::Duration::from_millis(900));
        day.h.settle();
        assert_eq!(day.pinned(), Some(cell), "the pane has the stack's card");
        assert!(day.h.tooltip().is_none(), "a card raised over the pane");
        day.h.hold(KeyCode::ShiftLeft);
        day.h.step(2);
        day.h.advance(std::time::Duration::from_millis(900));
        day.h.settle();
        assert!(
            day.h.tooltip().is_none(),
            "the long card raised over the pane"
        );
        day.h.release(KeyCode::ShiftLeft);
        assert!(day.h.world().resource::<CardContext>().quiet());

        // Closed, the card is free to be raised again by whatever screen
        // next shows a stack without pinning its card.
        day.close();
        day.h.settle();
        assert!(!day.h.world().resource::<CardContext>().quiet());
    }

    // The part itself: quiet it adds nothing, and a tooltip with nothing
    // in it is not raised; otherwise it is the card alone, short or long.
    let day = first_day("stores-raised", 1600.0, 900.0);
    let items = &day.registries.items;
    let stack = day.store().0.get(0).cloned().expect("the magazines");
    let context = day.h.world().resource::<CardContext>().clone();
    let part = CardPart(day.registries.clone(), context.clone());
    let build = |tier| {
        let mut out = Vec::new();
        slotted::ui::TooltipPart::build(
            &part,
            &slotted::ui::TooltipCtx {
                stack: Some(&stack),
                registries: items.frozen(),
                tier,
            },
            &mut out,
        );
        out
    };
    let raised = |tier| {
        let out = build(tier);
        let [UiNodeDef::Custom { kind, params, .. }] = &out[..] else {
            panic!("the tooltip is the card alone: {out:?}");
        };
        assert_eq!(*kind, WidgetKind::new(card::WIDGET));
        CardParams::from_value(params).unwrap()
    };
    let short = raised(TooltipTier::Compact);
    assert!(!short.framed, "raised, it is drawn in the tooltip's frame");
    assert_eq!(short.name, "Patrol extended magazine");
    assert!(
        short
            .notes
            .contains(&"Hold Shift for the long card".to_owned())
    );
    assert_eq!(
        short.effects[0].value, "29 rounds",
        "read against the hands"
    );
    let long = raised(TooltipTier::Expanded);
    assert!(
        !long
            .notes
            .contains(&"Hold Shift for the long card".to_owned())
    );
    context.set_quiet(true);
    assert!(build(TooltipTier::Compact).is_empty());
    assert!(build(TooltipTier::Expanded).is_empty());
}

/// Two batches of one module sit side by side and are told apart in the
/// grid: each cell's pip is filled to its batch's quality, beside its own
/// count, and the crafted batch's pip has a cap across its head, which is
/// what an eye tells them by where the two fills stand a pixel apart. A
/// weapon as new has no condition strip. A filter chip dims what it leaves
/// out and nothing is rebuilt.
#[test]
#[allow(clippy::too_many_lines)]
fn two_batches_tell_apart_by_their_pips() {
    let mut day = first_day("stores-pips", 1600.0, 900.0);
    day.use_locker();
    day.h.step(3);
    day.h.settle();
    let dressing = |day: &Day, slot: Entity| {
        day.h
            .world()
            .get::<SlotDressing>(slot)
            .expect("a stores cell is dressed")
            .clone()
    };
    // The pip's fill is the one mark in a cell that is as tall as a
    // percentage of its track.
    let fill = |day: &mut Day, slot: Entity| {
        let world = day.h.world_mut();
        let mut found = Vec::new();
        let mut below: Vec<Entity> = world.get::<Children>(slot).unwrap().to_vec();
        while let Some(entity) = below.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                below.extend(children.iter());
            }
            if world.get::<stores::SlotMark>(entity).is_some()
                && let Some(Val::Percent(height)) = world.get::<Node>(entity).map(|n| n.height)
            {
                found.push(height);
            }
        }
        assert_eq!(found.len(), 1, "one pip fill a cell");
        found[0]
    };
    let (store0, store1) = (day.cell("store", 0), day.cell("store", 1));
    assert_eq!(dressing(&day, store0).grade, Some(500), "store grade");
    assert_eq!(
        dressing(&day, store1).grade,
        Some(522),
        "the mean of 610 and 433"
    );
    assert_eq!(fill(&mut day, store0), 50.0);
    assert!((fill(&mut day, store1) - 52.2).abs() < 0.01);
    // Those two fills are under a pixel apart on a pip this tall, so the
    // mark that tells the batches apart is the cap: drawn on the crafted
    // one, wider than the pip it sits on, and not drawn on the bought one.
    let (bought, crafted) = (dressing(&day, store0), dressing(&day, store1));
    assert!(!bought.crafted && crafted.crafted);
    assert!(!day.h.is_visible(bought.cap()), "store grade has no cap");
    assert!(day.h.is_visible(crafted.cap()));
    let cap = day.h.rect_of(crafted.cap());
    let cell = day.h.rect_of(store1);
    assert!(
        cap.width() >= 6.0 && cap.height() >= 2.0,
        "a mark an eye can find: {cap:?}"
    );
    assert!(within(cap, cell), "{cap:?} inside its cell {cell:?}");
    assert!(
        cap.max.y < cell.center().y,
        "at the pip's head, clear of its fill"
    );
    assert_eq!(
        day.h
            .world()
            .get::<slotted::theme::Themed>(crafted.cap())
            .unwrap()
            .0
            .as_str(),
        "strata.slot.pip.cap"
    );
    // A bought stack of another item has none either.
    assert!(!dressing(&day, day.cell("store", 2)).crafted);
    assert_eq!(
        day.h
            .world()
            .get::<SemanticLabel>(store0)
            .unwrap()
            .0
            .as_str(),
        "veyra:module.patrol_extended_feed x4",
        "slotted's own count beside the pip"
    );
    assert_eq!(
        day.h.displayed_stack(store1).map(|stack| stack.count),
        Some(2)
    );
    // Empty: no pip. A weapon as new: a pip and no strip.
    let empty = day.cell("store", 30);
    assert_eq!(dressing(&day, empty).grade, None);
    let hand = find(&day.h, &stores::hand_slot_id(0));
    assert_eq!(dressing(&day, hand).grade, Some(500));
    assert_eq!(dressing(&day, hand).worn, None, "condition 1000 of 1000");

    // The cap goes with the stack: carried to another cell, it is that
    // cell's.
    let moved_to = day.cell("store", 10);
    day.h.click(store1);
    day.h.settle();
    day.h.click(moved_to);
    day.h.step(3);
    day.h.settle();
    assert!(!day.h.is_visible(dressing(&day, store1).cap()));
    assert!(day.h.is_visible(dressing(&day, moved_to).cap()));

    // Mods: the haul pack in the sixth cell is dimmed, the magazines not.
    let root = find(&day.h, stores::ROOT_ID);
    let store5 = day.cell("store", 5);
    assert!(!dressing(&day, store5).dimmed);
    let mods = find(&day.h, &stores::Filter::Mods.chip_id());
    day.h.click(mods);
    day.h.step(3);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().filter,
        stores::Filter::Mods
    );
    assert!(dressing(&day, store5).dimmed);
    assert!(!dressing(&day, store0).dimmed);
    assert!(dressing(&day, hand).dimmed, "a weapon is gear");
    let strength = |day: &mut Day, slot: Entity| {
        let world = day.h.world_mut();
        let children: Vec<Entity> = world.get::<Children>(slot).unwrap().to_vec();
        children
            .into_iter()
            .find(|child| world.get::<slotted::ui::ItemIcon>(*child).is_some())
            .and_then(|icon| world.get::<bevy::ui::widget::ImageNode>(icon))
            .map(|image| image.color.alpha())
            .expect("a cell has an icon")
    };
    assert!((strength(&mut day, store5) - stores::DIMMED).abs() < 0.001);
    assert_eq!(strength(&mut day, store0), 1.0);
    assert_eq!(
        find(&day.h, stores::ROOT_ID),
        root,
        "the screen was not rebuilt"
    );
    let all = find(&day.h, &stores::Filter::All.chip_id());
    day.h.click(all);
    day.h.step(3);
    day.h.settle();
    assert!(!dressing(&day, store5).dimmed);
    assert_eq!(strength(&mut day, store5), 1.0);
}
