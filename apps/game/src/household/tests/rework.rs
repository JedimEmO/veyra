//! The rework end to end, as the household keeps it: a made carbine under
//! the stock mount's threshold and two blanks start the smith's day (the
//! setup of `--crafted-carbine` and `--blanks`), the request goes through the
//! inbox at a workbench, and the file holds what the sim did.

use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use strata::sim::rework::{self, ReworkAnswer, ReworkInbox, ReworkRequest, Reworked};

use super::*;

/// The made carbine's moddability: under the stock's 400, over nothing the
/// store-grade blank could not lift.
const MADE: u16 = 350;

/// A sim in the world with the smith's day on it, and the household that
/// day was started from.
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn smiths_day(registries: &GameRegistries) -> (App, Household) {
    let grants = Grants {
        lots: Vec::new(),
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let mut household = Household::starting(registries, &grants);
    join_all(&mut household);
    household.active = household.index_with("weaponsmith.frames.2").unwrap();
    household
        .crafted_carbine_at_start(MADE, true, registries)
        .unwrap();
    household.blanks_at_start(2, registries).unwrap();

    let mut app = App::new();
    app.add_plugins((bevy::time::TimePlugin, strata::sim::SimPlugin));
    app.insert_resource(bevy::time::TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(strata::sim::WorldSeed(strata::core::Seed(42)));
    for _ in 0..3 {
        app.update();
    }
    let (combatant, sheet, name, _, pack, hands) = household.stats(registries);
    app.world_mut().spawn((
        LocalPlayer,
        name,
        strata::sim::WorldPosition::new(0., 0., 0.),
        pack,
        hands,
        combatant,
        sheet,
    ));
    let store = HouseholdStore::from_saved(&household.store, &registries.items).unwrap();
    app.insert_resource(store)
        .insert_resource(strata::sim::Registries(std::sync::Arc::new(
            registries.clone(),
        )))
        .insert_resource(Purse(household.purse));
    app.world_mut().spawn((
        strata::sim::WorldPosition::new(0., 0., 0.),
        Interaction {
            kind: InteractionKind::Workbench,
            radius: 2.5,
        },
    ));
    // A tick for the prop to be in reach.
    app.update();
    (app, household)
}

#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn carried(app: &mut App, registries: &GameRegistries) -> (Record, u32) {
    let items = &registries.items;
    let world = app.world_mut();
    let mut players = world.query_filtered::<(&Pack, &Hands), With<LocalPlayer>>();
    let (pack, hands) = players.single(world).unwrap();
    let carbine = items::read(hands.0.get(0).unwrap(), items).unwrap();
    let blank = registries.rework.as_ref().unwrap().blank.clone();
    let blanks = pack
        .cells
        .slots()
        .iter()
        .flatten()
        .filter_map(|stack| items::read(stack, items).ok())
        .filter(|record| record.item == blank)
        .map(|record| record.count)
        .sum();
    (carbine, blanks)
}

/// The smith reworks the carbine at the workbench: one roll, one blank
/// spent, the line moved to where the outcome puts it, the mount it opens
/// open, and the household file reads back as what the sim left.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the shipped data")]
fn a_rework_at_the_bench_lifts_the_line_spends_a_blank_and_is_saved() {
    let registries = shipped();
    let (mut app, household) = smiths_day(&registries);
    assert_eq!(household.active().name, "Tovan Reyes");
    let (before, blanks) = carried(&mut app, &registries);
    assert_eq!(before.lines["moddability"], MADE);
    assert_eq!(blanks, 2);
    assert_eq!(
        registries.items.mount_opens(&before, "stock"),
        Some(Err(400)),
        "the stock mount is shut on it"
    );

    app.world_mut()
        .resource_mut::<ReworkInbox>()
        .0
        .push(ReworkRequest::Rework { hand: 0 });
    app.update();
    let mut answers: Vec<ReworkAnswer> = app
        .world_mut()
        .resource_mut::<Messages<ReworkAnswer>>()
        .drain()
        .collect();
    assert_eq!(answers.len(), 1, "one request, one answer");
    let done: Reworked = answers.pop().unwrap().result.unwrap();

    // The line is where the outcome closes its share of the gap to the
    // blank's temper, from where it was.
    let rules = registries.rework.as_ref().unwrap();
    assert_eq!((done.hand, done.from), (0, MADE));
    assert_eq!(done.to, rework::result_of(rules, done.outcome, MADE, 500));
    assert!(done.to > MADE, "the roll lifted the line: {done:?}");
    let (after, blanks) = carried(&mut app, &registries);
    assert_eq!(after.lines["moddability"], done.to);
    assert_eq!(blanks, 1, "one blank was spent");
    assert_eq!(
        app.world().resource::<CraftState>().rolls,
        1,
        "one roll was made"
    );
    let stock_open = registries.items.mount_opens(&after, "stock") == Some(Ok(()));
    assert_eq!(stock_open, done.opened.contains(&"stock".to_owned()));
    assert_eq!(stock_open, done.to >= 400);
    // Only the moddability moved.
    let mut expected = before.clone();
    expected.lines.insert("moddability".to_owned(), done.to);
    assert_eq!(after, expected);

    // The household takes it, and the file reads back what was written.
    app.insert_resource(household);
    app.world_mut().run_system_once(track_stock).unwrap();
    let saved = app.world().resource::<Household>().clone();
    let smith = saved.active();
    let held = smith.hands[0].clone().unwrap();
    assert_eq!(held.lines["moddability"], done.to);
    assert_eq!(saved.crafting.rolls, 1);
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    saved.save(&path).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, saved, "the file round-trips");
    let reread = read.household.active();
    assert_eq!(
        reread.hands[0].as_ref().unwrap().lines["moddability"],
        done.to,
        "the smith's hand holds the lifted line"
    );
    let (.., pack, _) = read.household.stats(&registries);
    let blank = registries.rework.as_ref().unwrap().blank.clone();
    let left: u32 = pack
        .cells
        .slots()
        .iter()
        .flatten()
        .filter_map(|stack| items::read(stack, &registries.items).ok())
        .filter(|record| record.item == blank)
        .map(|record| record.count)
        .sum();
    assert_eq!(left, 1, "the saved pack holds the one blank left");
}
