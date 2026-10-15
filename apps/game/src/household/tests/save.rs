//! Tests of the save: round trips, the upgrades from each older shape and
//! what is kept beside the file.

use super::*;

/// The first day's store holds two batches of the patrol magazine in
/// two cells. They go through the sim, where one is carried off to the
/// pack, and through the file and back as two stacks, never one.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_store_with_two_unmerged_batches_round_trips_the_save() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let items = &registries.items;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");

    let household = Household::starting(&registries, &Grants::default());
    assert_eq!(household.version, 5);
    assert_eq!(household.check(&registries), Ok(()));
    assert_eq!(
        stack(household.store[0].as_ref()),
        Some((FEED.to_owned(), 4, 0))
    );
    assert_eq!(
        stack(household.store[1].as_ref()),
        Some((FEED.to_owned(), 2, 412))
    );
    assert_eq!(household.store.len(), 12, "no empty cell is written");
    // Everyone has both weapons; only the arrival carries anything.
    for character in household.characters.iter().chain(&household.recruits) {
        assert!(
            character.hands.iter().all(Option::is_some),
            "{}",
            character.name
        );
    }
    assert!(household.characters[0].pack.bag_slots[0].is_some());
    // The survey tool is in the last cell of every pack and nothing else.
    for character in household.characters.iter().chain(&household.recruits) {
        let carried: Vec<_> = character.pack.cells.iter().flatten().collect();
        assert_eq!(carried.len(), 1, "{}", character.name);
        assert_eq!(carried[0].item.to_string(), "veyra:tool.survey");
    }
    assert_eq!(household.recruits[0].pack.bag_slots, []);

    // Into the sim as it is at a spawn, and a stack moved there.
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    let store = HouseholdStore::from_saved(&household.store, items).unwrap();
    assert!(
        !store.0.get(0).unwrap().same_kind(store.0.get(1).unwrap()),
        "two batches are two kinds of stack"
    );
    world.insert_resource(store);
    let (.., pack, hands) = household.stats(&registries);
    let player = world.spawn((LocalPlayer, pack, hands)).id();
    world.insert_resource(household.clone());
    world.run_system_once(track_stock).unwrap();
    assert_eq!(
        *world.resource::<Household>(),
        household,
        "the sim holds what the save held"
    );
    let crafted = world.resource_mut::<HouseholdStore>().0.take(1);
    world.get_mut::<Pack>(player).unwrap().cells.set(0, crafted);
    world.run_system_once(track_stock).unwrap();
    let moved = world.resource::<Household>().clone();
    assert_eq!(
        stack(moved.store[0].as_ref()),
        Some((FEED.to_owned(), 4, 0))
    );
    assert_eq!(moved.store[1], None, "its cell is empty, not closed up");
    assert_eq!(
        stack(moved.characters[0].pack.cells[0].as_ref()),
        Some((FEED.to_owned(), 2, 412))
    );
    assert_eq!(moved.characters[0].hands, household.characters[0].hands);

    // Through the file and back.
    moved.save(&path).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, moved);
    assert!(!beside(&path, ".v1.bak").exists() && !beside(&path, ".refused.bak").exists());
    // And into the sim again, as the next run's spawn does.
    let again = HouseholdStore::from_saved(&read.household.store, items).unwrap();
    assert_eq!(again.saved(items).unwrap(), moved.store);
    let (.., pack, hands) = read.household.stats(&registries);
    assert_eq!(pack.saved(items).unwrap(), moved.characters[0].pack);
    assert_eq!(hands.saved(items).unwrap(), moved.characters[0].hands);
}

/// A file of this shape written before the engine's rename, its data ids
/// under the old namespace, reads as the household it was and is kept
/// beside as `.ids.bak`; written again it reads with no upgrade.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_file_under_the_old_namespace_is_renamed_and_kept_beside() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let grants = Grants {
        lots: vec![lot_of(&registries, "strata:evaporite_salt", [3, -2], 160)],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let household = Household::starting(&registries, &grants);
    household.save(&path).unwrap();
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("strata:", OLD_DATA_NAMESPACE);
    assert!(text.contains(OLD_DATA_NAMESPACE), "the file names data ids");
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, household);
    let kept = beside(&path, ".ids.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);

    read.household.save(&path).unwrap();
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains(OLD_DATA_NAMESPACE)
    );
    let again = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!again.first_day && !again.hold);
    assert_eq!(again.household, household);
    assert!(!beside(&path, ".ids.2.bak").exists());
}

/// An earlier copy that is not text is different bytes like any other: the
/// next number is used and the keeping does not fail.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn keeping_goes_past_an_earlier_copy_that_is_not_text() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    std::fs::write(&path, "(now)").unwrap();
    std::fs::write(beside(&path, ".x.bak"), [0xff_u8, 0xfe]).unwrap();
    let kept = super::super::save::keep(&path, "x").unwrap();
    assert_eq!(kept, beside(&path, ".x.2.bak"));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "(now)");
}

/// A household saved before items existed, as that build wrote it.
mod before_items {
    use crate::household::{CharacterSheet, Weapon, WorldPos};
    use serde::Serialize;

    #[derive(Serialize)]
    pub(super) struct Character {
        pub name: String,
        pub sheet: CharacterSheet,
        pub weapon: Weapon,
        pub ammo: u16,
        pub holstered_ammo: u16,
        pub tint: u8,
        pub left_at: Option<WorldPos>,
    }

    #[derive(Serialize)]
    pub(super) struct Household {
        pub version: u32,
        pub characters: Vec<Character>,
        pub active: usize,
        pub recruits: Vec<Character>,
    }
}

/// A played household of the shape before items: the arrival has earned
/// and learned, two recruits have joined, the pistol marksman is out.
fn played_before_items(registries: &GameRegistries) -> (String, Household) {
    let mut played = Household::starting(registries, &Grants::default());
    played.characters[0]
        .sheet
        .credit(strata::registry::progression::XpKind::Ranged, 826);
    played.characters[0].ammo = 7;
    played.characters[0].left_at = Some(WorldPos::new(12.5, 40.25, -3.));
    played.characters.push(played.recruits.remove(0));
    played.characters.push(played.recruits.remove(0));
    played.active = 2;
    let old = |c: &Character| before_items::Character {
        name: c.name.clone(),
        sheet: c.sheet.clone(),
        weapon: c.weapon,
        ammo: c.ammo,
        holstered_ammo: c.holstered_ammo,
        tint: c.tint,
        left_at: c.left_at,
    };
    let file = before_items::Household {
        version: 1,
        characters: played.characters.iter().map(old).collect(),
        active: played.active,
        recruits: played.recruits.iter().map(old).collect(),
    };
    let text =
        ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).expect("serialises");
    (text, played)
}

#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_version_one_file_is_upgraded_and_kept_beside() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let (text, played) = played_before_items(&registries);
    assert!(text.contains("version: 1") && !text.contains("pack"));
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day, "a played household is not started over");
    assert!(!read.hold);
    let upgraded = read.household;
    assert_eq!(upgraded.version, Household::VERSION);
    // Every character is who they were: `played` was built from the
    // first day, so it already carries the first day's stock, which is
    // exactly what an upgrade gives.
    assert_eq!(upgraded, played);
    assert_eq!(upgraded.active().name, "Ilse Marrow");
    assert_eq!(upgraded.characters[0].ammo, 7);
    assert!(upgraded.characters[0].left_at.is_some());
    assert_eq!(upgraded.store, starting_store(&registries.items));
    assert_eq!(upgraded.store.len(), 12);
    for character in upgraded.characters.iter().chain(&upgraded.recruits) {
        assert!(
            character.hands.iter().all(Option::is_some),
            "{}",
            character.name
        );
    }
    assert!(upgraded.characters[0].pack.bag_slots[0].is_some());

    // The old file is beside the new one, to the byte, before anything
    // is written over it.
    let kept = beside(&path, ".v1.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    // Written, it is a file of this build's shape, read without another
    // upgrade and without another copy.
    upgraded.save(&path).unwrap();
    let again = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!again.first_day && !again.hold);
    assert_eq!(again.household, upgraded);
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert!(!beside(&path, ".v1.2.bak").exists());
    assert!(!beside(&path, ".refused.bak").exists());

    // An old file the graph refuses is refused as any file is, and kept.
    let broken = text.replace("blade.novice", "blade.gone");
    assert_ne!(broken, text);
    std::fs::write(&path, &broken).unwrap();
    let refused = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(refused.first_day && !refused.hold);
    assert_eq!(
        std::fs::read_to_string(beside(&path, ".refused.bak")).unwrap(),
        broken
    );
}

/// A household saved before the purse and the survey, as that build
/// wrote it.
#[derive(Serialize)]
struct BeforeThePurse {
    version: u32,
    characters: Vec<Character>,
    active: usize,
    recruits: Vec<Character>,
    store: Vec<Option<Record>>,
}

/// A file of the shape before the purse is read whole, given the first
/// lots in the store's first empty cells and the purse, and after them
/// the bought parts a bench takes, and kept beside as `.v2.bak`;
/// written and read again it is given nothing more. A file of the
/// shape before items goes through both upgrades.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_version_two_file_is_given_the_lots_and_the_purse_once() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let grants = Grants {
        lots: vec![
            lot_of(&registries, "strata:evaporite_salt", [3, -2], 160),
            lot_of(&registries, "strata:light_alloy_ore", [4, 1], 200),
        ],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    for lot in &grants.lots {
        assert_eq!(registries.items.check_record(lot), Ok(()));
    }
    // A played household of the old shape: a recruit joined, the
    // bought magazines carried off, which left the store's first cell
    // empty.
    let mut played = Household::starting(&registries, &Grants::default());
    // A store of that shape had none of the parts a bench buys.
    let mut parts = played.store.split_off(6);
    assert_eq!(
        parts.len(),
        6,
        "the harnesses, the seals, the kits, the vented muzzle and the two blades"
    );
    // A store of that shape had no kits, muzzle or blades either, and an upgrade
    // gives the bench's parts alone.
    parts.truncate(2);
    played.characters.push(played.recruits.remove(0));
    played.characters[0].pack.cells = vec![played.store[0].take()];
    played.characters[0].ammo = 7;
    let old = BeforeThePurse {
        version: 2,
        characters: played.characters.clone(),
        active: played.active,
        recruits: played.recruits.clone(),
        store: played.store.clone(),
    };
    let text = ron::ser::to_string_pretty(&old, ron::ser::PrettyConfig::default()).unwrap();
    assert!(text.contains("version: 2") && !text.contains("purse"));
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &|| grants.clone());
    assert!(!read.first_day && !read.hold);
    let upgraded = read.household;
    assert_eq!(upgraded.version, Household::VERSION);
    assert_eq!(upgraded.purse, 2500);
    assert!(upgraded.visited.is_empty() && upgraded.tracked.is_none());
    // Everyone is who they were and carries what they carried.
    let owed = owed_the_tool(&played, &registries);
    assert_eq!(upgraded.characters, owed.characters);
    assert_eq!(upgraded.recruits, owed.recruits);
    assert_eq!(upgraded.active, played.active);
    // The store is the store it was, with the salt in the cell that
    // was empty and the alloy after the last stack, and after that the
    // harnesses and the seal sets of the first day.
    assert_eq!(upgraded.store.len(), played.store.len() + 3);
    assert_eq!(upgraded.store[0].as_ref(), Some(&grants.lots[0]));
    assert_eq!(upgraded.store[1..6], played.store[1..6]);
    assert_eq!(upgraded.store[6].as_ref(), Some(&grants.lots[1]));
    assert_eq!(upgraded.store[7..], parts[..]);
    assert_eq!(upgraded.crafting, CraftState::default());
    assert_eq!(upgraded.check(&registries), Ok(()));

    // The old file is beside the new one, to the byte, before anything
    // is written over it.
    let kept = beside(&path, ".v2.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(!beside(&path, ".v1.bak").exists());

    // Played on: a lot drawn, a lot used up, a post stood at. Written,
    // it is a file of this build's shape, and reading it gives nothing
    // again: no lots, no purse, no second copy.
    let mut later = upgraded.clone();
    later.purse = 2380;
    later.store[0] = None;
    later.visited.insert(DepositId([3, -2]));
    later.tracked = Some(DepositId([4, 1]));
    later.save(&path).unwrap();
    // A file of this build's shape never asks for the grants, which
    // would read the world's deposits for nothing.
    let asked = std::cell::Cell::new(0);
    let counted = || {
        asked.set(asked.get() + 1);
        grants.clone()
    };
    let again = Household::load_or_start(&path, false, &registries, &counted);
    assert_eq!(asked.get(), 0, "no upgrade, no grants");
    assert!(!again.first_day && !again.hold);
    assert_eq!(again.household, later);
    assert_eq!(again.household.purse, 2380);
    assert_eq!(
        again.household.store[0], None,
        "the salt is not given twice"
    );
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert!(!beside(&path, ".v2.2.bak").exists());
    assert!(!beside(&path, ".refused.bak").exists());

    // The first day has the same lots after the items' own store.
    let first = Household::starting(&registries, &grants);
    assert_eq!(first.purse, 2500);
    assert_eq!(first.store.len(), 14);
    assert_eq!(first.store[12].as_ref(), Some(&grants.lots[0]));
    assert_eq!(first.store[13].as_ref(), Some(&grants.lots[1]));
    assert_eq!(first.check(&registries), Ok(()));

    // A file from before items is given both: the first day's stock,
    // then the lots and the purse, and is kept under its own shape.
    let v1 = dir.path().join("older.ron");
    let (old_text, _) = played_before_items(&registries);
    std::fs::write(&v1, &old_text).unwrap();
    let read = Household::load_or_start(&v1, false, &registries, &|| grants.clone());
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household.purse, 2500);
    assert_eq!(read.household.store, first.store);
    assert_eq!(
        std::fs::read_to_string(beside(&v1, ".v1.bak")).unwrap(),
        old_text
    );
    assert!(!beside(&v1, ".v2.bak").exists());
}

/// A household saved before the bench made anything, as that build
/// wrote it.
#[derive(Serialize)]
struct BeforeTheBench {
    version: u32,
    characters: Vec<Character>,
    active: usize,
    recruits: Vec<Character>,
    store: Vec<Option<Record>>,
    purse: u32,
    visited: BTreeSet<DepositId>,
    tracked: Option<DepositId>,
    surveyed_in: Option<SurveyedIn>,
}

/// A file of the shape before the bench made anything is read whole,
/// its purse, its lots and its survey record as they were, given the
/// first day's harnesses and seal sets in the store's first empty
/// cells and an empty bench, and kept beside as `.v3.bak`; written and
/// read again it is given nothing more. No deposit is asked for.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
#[allow(
    clippy::too_many_lines,
    reason = "one file, read, played on and read again"
)]
fn a_version_three_file_is_given_the_parts_once() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let asked = std::cell::Cell::new(0);
    let grants = || {
        asked.set(asked.get() + 1);
        Grants::default()
    };
    // A played household of that shape: the first day's six stacks and
    // two lots, the vigil sight carried off (which left the store's
    // fourth cell empty), a lot drawn, a post stood at, a deposit
    // tracked.
    let mut played = Household::starting(&registries, &Grants::default());
    let mut parts = played.store.split_off(6);
    assert_eq!(
        parts.len(),
        6,
        "the harnesses, the seals, the kits, the vented muzzle and the two blades"
    );
    // A store of that shape had no kits, muzzle or blades either, and an upgrade
    // gives the bench's parts alone.
    parts.truncate(2);
    played.store.extend([
        Some(lot_of(&registries, "strata:evaporite_salt", [3, -2], 160)),
        Some(lot_of(&registries, "strata:light_alloy_ore", [4, 1], 200)),
    ]);
    played.characters[0].pack.cells = vec![played.store[3].take()];
    played.purse = 2380;
    played.visited.insert(DepositId([3, -2]));
    played.tracked = Some(DepositId([4, 1]));
    played.surveyed_in = Some(SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    });
    let old = BeforeTheBench {
        version: 3,
        characters: played.characters.clone(),
        active: played.active,
        recruits: played.recruits.clone(),
        store: played.store.clone(),
        purse: played.purse,
        visited: played.visited.clone(),
        tracked: played.tracked,
        surveyed_in: played.surveyed_in,
    };
    let text = ron::ser::to_string_pretty(&old, ron::ser::PrettyConfig::default()).unwrap();
    assert!(text.contains("version: 3") && !text.contains("crafting"));
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &grants);
    assert!(!read.first_day && !read.hold);
    assert_eq!(
        asked.get(),
        0,
        "the parts are the starting file's, not the world's"
    );
    let upgraded = read.household;
    assert_eq!(upgraded.version, Household::VERSION);
    assert_eq!(upgraded.version, 5);
    // Everything it had, it has.
    let owed = owed_the_tool(&played, &registries);
    assert_eq!(upgraded.characters, owed.characters);
    assert_eq!(upgraded.recruits, owed.recruits);
    assert_eq!(
        (upgraded.purse, &upgraded.visited, upgraded.tracked),
        (2380, &played.visited, played.tracked)
    );
    assert_eq!(upgraded.surveyed_in, played.surveyed_in);
    // The harnesses are in the cell that was empty and the seal sets
    // after the last stack; nothing else moved.
    assert_eq!(upgraded.store.len(), played.store.len() + 1);
    assert_eq!(upgraded.store[3], parts[0]);
    assert_eq!(
        stack(upgraded.store[3].as_ref()),
        Some(("veyra:component.harness".to_owned(), 3, 0))
    );
    assert_eq!(upgraded.store[..3], played.store[..3]);
    assert_eq!(upgraded.store[4..8], played.store[4..8]);
    assert_eq!(upgraded.store[8], parts[1]);
    assert_eq!(
        stack(upgraded.store[8].as_ref()),
        Some(("veyra:commodity.seal_set".to_owned(), 6, 0))
    );
    assert_eq!(upgraded.crafting, CraftState::default());
    assert_eq!(upgraded.check(&registries), Ok(()));

    // The old file is beside the new one, to the byte, before anything
    // is written over it.
    let kept = beside(&path, ".v3.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(!beside(&path, ".v2.bak").exists());

    // Played on: the harnesses used up. Written, it is a file of this
    // build's shape, and reading it gives nothing again.
    let mut later = upgraded.clone();
    later.store[3] = None;
    later.crafting.rolls = 3;
    later.crafting.batches = 1;
    later.save(&path).unwrap();
    let again = Household::load_or_start(&path, false, &registries, &grants);
    assert!(!again.first_day && !again.hold);
    assert_eq!(again.household, later);
    assert_eq!(
        again.household.store[3], None,
        "the harnesses are not given twice"
    );
    assert_eq!(asked.get(), 0);
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert!(!beside(&path, ".v3.2.bak").exists());
    assert!(!beside(&path, ".refused.bak").exists());

    // A full store leaves both stacks out, says so and is written all
    // the same: the store is the player's own to make room in.
    let full = BeforeTheBench {
        store: vec![played.store[1].clone(); usize::from(registries.items.carry().store.cells())],
        ..old
    };
    let crowded = dir.path().join("crowded.ron");
    let text = ron::ser::to_string_pretty(&full, ron::ser::PrettyConfig::default()).unwrap();
    std::fs::write(&crowded, &text).unwrap();
    let (read, upgrade, _) = Household::read(&text, &registries, &grants).unwrap();
    assert_eq!(read.store, full.store);
    assert_eq!(
        upgrade,
        Some(Upgrade {
            shape: "v3",
            short: 0,
            left_out: 2
        })
    );
    let read = Household::load_or_start(&crowded, false, &registries, &grants);
    assert!(!read.first_day && !read.hold);
    assert!(beside(&crowded, ".v3.bak").exists(), "the file as it was");
}

/// A session left on the bench is in the file and on the bench at the
/// next start: the materials were committed at assembly, so they are
/// in the session and not in the store, the points and the roll that
/// was made stand, and finishing it then takes nothing more. A session
/// of a schematic the data no longer has is a file this build refuses,
/// and keeps.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
#[allow(
    clippy::too_many_lines,
    reason = "one session, from the bench to the file and back to the bench"
)]
fn an_open_session_round_trips_the_save() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let items = &registries.items;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let salt = lot_of(&registries, "strata:evaporite_salt", [3, -2], 160);
    let brine = lot_of(&registries, "strata:coastal_brine", [5, 5], 160);
    let grants = Grants {
        lots: vec![salt.clone(), brine.clone()],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let household = Household::starting(&registries, &grants);
    let units = |household: &Household, lot: &Record| -> u32 {
        household
            .store
            .iter()
            .flatten()
            .filter(|record| record.origin == lot.origin)
            .map(|record| record.count)
            .sum()
    };
    assert_eq!(
        (units(&household, &salt), units(&household, &brine)),
        (160, 160)
    );

    // At the bench: a core assembled and one point spent, by the sim's
    // own functions on the stock a spawn hands it.
    let core = "veyra:schematic.cell_core".parse().unwrap();
    let schematic = registries.schematics.get(&core).unwrap();
    let (.., mut pack, mut hands) = household.stats(&registries);
    let mut store = HouseholdStore::from_saved(&household.store, items).unwrap();
    let mut bench = household.crafting.clone();
    let picks = crafting::best_stock(&registries, &pack, Some(&store), schematic);
    let mut stock = Stock {
        saddle: None,
        pack: &mut pack,
        hands: &mut hands,
        store: Some(&mut store),
    };
    let seed = strata::core::Seed(42);
    assert_eq!(
        crafting::assemble(
            &registries,
            &mut stock,
            &mut bench,
            &core,
            &picks,
            "Frontier ranger",
            &CharacterSheet::default()
        ),
        Ok(3)
    );
    let rolled = crafting::roll_line(
        &registries,
        seed,
        &mut bench,
        "charge_density",
        Hand::Steady,
    )
    .unwrap();
    assert_eq!(rolled.points, 2);

    // The sim's state is copied to the household, which the file holds.
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(store);
    world.insert_resource(bench.clone());
    world.spawn((LocalPlayer, pack, hands));
    world.insert_resource(household.clone());
    world.run_system_once(track_stock).unwrap();
    let open = world.resource::<Household>().clone();
    assert_eq!(open.crafting, bench);
    assert_eq!((units(&open, &salt), units(&open, &brine)), (140, 130));
    // Nothing moved, nothing copied: a second pass leaves it as it is.
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), open);
    open.save(&path).unwrap();

    // The next start reads it back as it was.
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, open);
    let session = read.household.crafting.session.as_ref().unwrap();
    assert_eq!(
        (session.points, session.by.as_str()),
        (2, "Frontier ranger")
    );
    assert_eq!(session.lines[0].figure(), rolled.to);
    let committed: Vec<(&str, u32)> = session
        .inputs
        .iter()
        .map(|(slot, record)| (slot.as_str(), record.count))
        .collect();
    assert_eq!(committed, [("salt", 20), ("brine", 30)]);
    assert_eq!(read.household.crafting.rolls, 1);

    // And it is finished from there, with the stock as it was read:
    // the core is made and no more salt or brine leaves the store.
    let (.., mut pack, mut hands) = read.household.stats(&registries);
    let mut store = HouseholdStore::from_saved(&read.household.store, items).unwrap();
    let mut bench = read.household.crafting.clone();
    let stored = store.saved(items).unwrap();
    let mut stock = Stock {
        saddle: None,
        pack: &mut pack,
        hands: &mut hands,
        store: Some(&mut store),
    };
    let product = crafting::finish(&registries, &mut stock, &mut bench).unwrap();
    assert_eq!(product.item.to_string(), "veyra:component.cell_core");
    assert_eq!(
        (product.batch, product.lines["charge_density"]),
        (1, rolled.to)
    );
    assert_eq!(
        store.saved(items).unwrap(),
        stored,
        "the store is as it was"
    );
    // The product in the first cell and the survey tool in the last.
    let saved = pack.saved(items).unwrap().cells;
    assert_eq!(saved[0], Some(product));
    assert_eq!(
        saved
            .iter()
            .flatten()
            .map(|r| r.item.to_string())
            .collect::<Vec<_>>(),
        ["veyra:component.cell_core", "veyra:tool.survey"]
    );
    assert_eq!(bench.session, None);
    assert_eq!(bench.made[&1].from.len(), 2);

    // What an edit to the data would put out of line does not cost the
    // household its file: a session with more points than the dial now
    // gives is held to the dial, a ledger entry of an item that is gone
    // is dropped, and everything else is as it was written. The file as
    // it was is kept aside, since an entry was dropped.
    let mut edited = open.clone();
    edited.crafting.session.as_mut().unwrap().points = 11;
    edited.crafting.batches = 1;
    edited.crafting.made.insert(
        1,
        crafting::Made {
            item: "veyra:component.gone".parse().unwrap(),
            by: "Frontier ranger".to_owned(),
            from: Vec::new(),
            schematic: None,
            recorded: None,
            blueprint: None,
        },
    );
    let off = dir.path().join("retuned.ron");
    edited.save(&off).unwrap();
    let written = std::fs::read_to_string(&off).unwrap();
    let mended = Household::load_or_start(&off, false, &registries, &Grants::default);
    assert!(
        !mended.first_day && !mended.hold,
        "the household is loaded, not started over"
    );
    let mut expected = open.clone();
    expected.crafting.batches = 1;
    expected.crafting.session.as_mut().unwrap().points = 10;
    assert_eq!(mended.household, expected);
    assert_eq!(mended.household.characters, open.characters);
    assert_eq!(mended.household.store, open.store);
    assert_eq!(
        std::fs::read_to_string(beside(&off, ".mended.bak")).unwrap(),
        written
    );
    assert!(!beside(&off, ".refused.bak").exists());
    let (_, _, said) = Household::read(&written, &registries, &Grants::default).unwrap();
    assert_eq!(said.len(), 2, "{said:?}");

    // A session of a schematic this build has not got is refused with
    // the rest of the file, which is kept aside.
    let text = std::fs::read_to_string(&path).unwrap();
    let stale = text.replace("veyra:schematic.cell_core", "veyra:schematic.gone");
    assert_ne!(stale, text);
    std::fs::write(&path, &stale).unwrap();
    let refused = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(refused.first_day && !refused.hold);
    assert_eq!(
        std::fs::read_to_string(beside(&path, ".refused.bak")).unwrap(),
        stale
    );
}

/// A full store has no cell for a first lot or a bought part: each is
/// left out, and said with the name of the copy of the file as it was,
/// and the purse is given all the same. The upgrade is written, since the store is the
/// player's own to make room in.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_full_store_leaves_the_first_lots_out_of_an_upgrade() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let grants = Grants {
        lots: vec![
            lot_of(&registries, "strata:evaporite_salt", [3, -2], 160),
            lot_of(&registries, "strata:light_alloy_ore", [4, 1], 200),
        ],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let played = Household::starting(&registries, &Grants::default());
    let filler = played.store[1].clone();
    let full = BeforeThePurse {
        version: 2,
        characters: played.characters.clone(),
        active: played.active,
        recruits: played.recruits.clone(),
        store: vec![filler; usize::from(registries.items.carry().store.cells())],
    };
    let crowded = dir.path().join("crowded.ron");
    let text = ron::ser::to_string_pretty(&full, ron::ser::PrettyConfig::default());
    std::fs::write(&crowded, text.unwrap()).unwrap();
    let read = Household::load_or_start(&crowded, false, &registries, &|| grants.clone());
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household.purse, 2500);
    assert_eq!(read.household.store, full.store);
    assert!(beside(&crowded, ".v2.bak").exists(), "the file as it was");
    let text = std::fs::read_to_string(&crowded).unwrap();
    let (_, upgrade, _) = Household::read(&text, &registries, &|| grants.clone()).unwrap();
    assert_eq!(
        upgrade,
        Some(Upgrade {
            shape: "v2",
            short: 0,
            left_out: 4
        }),
        "two lots and two stacks of parts"
    );
}

/// An upgrade that cannot give every first lot (a world with no such
/// deposit in range) is not written: the file stays in its older
/// shape, with no copy made, and a later start that finds them all
/// finishes it once.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn an_upgrade_short_of_a_lot_waits_for_a_later_start() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let salt = lot_of(&registries, "strata:evaporite_salt", [3, -2], 160);
    let alloy = lot_of(&registries, "strata:light_alloy_ore", [4, 1], 200);
    let short = Grants {
        lots: vec![salt.clone()],
        purse: 2500,
        short: 1,
        deposits: Vec::new(),
    };
    let played = Household::starting(&registries, &Grants::default());
    let old = BeforeThePurse {
        version: 2,
        characters: played.characters.clone(),
        active: played.active,
        recruits: played.recruits.clone(),
        store: played.store.clone(),
    };
    let text = ron::ser::to_string_pretty(&old, ron::ser::PrettyConfig::default()).unwrap();
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &|| short.clone());
    assert!(!read.first_day);
    assert!(read.hold, "the file is not to be written this run");
    assert_eq!(
        read.household.purse, 2500,
        "the run is played with what there is"
    );
    assert!(!beside(&path, ".v2.bak").exists());
    // Holding, a change of the household writes nothing over it.
    let mut world = World::new();
    world.insert_resource(read.household);
    world.insert_resource(HouseholdFile {
        path: path.clone(),
        fresh: false,
        fitted: Vec::new(),
        in_hand: Vec::new(),
        learn: Vec::new(),
        grant: Vec::new(),
        condition: None,
        member: None,
        crafted_carbine: None,
        blanks: None,
        give: Vec::new(),
        bike: false,
        bike_condition: None,
        purse: None,
        saddle_stock: false,
        harvester: false,
        harvest_hours: None,
        open_harvester: false,
    });
    world.insert_resource(SaveHeld);
    world.run_system_once(save_on_change).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    // The next start finds both and finishes the upgrade.
    let whole = Grants {
        lots: vec![salt.clone(), alloy.clone()],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let read = Household::load_or_start(&path, false, &registries, &|| whole.clone());
    assert!(!read.first_day && !read.hold);
    let lots: Vec<&Record> = read
        .household
        .store
        .iter()
        .flatten()
        .filter(|record| record.origin.is_some())
        .collect();
    assert_eq!(lots, [&salt, &alloy]);
    assert_eq!(
        std::fs::read_to_string(beside(&path, ".v2.bak")).unwrap(),
        text
    );
}

/// `played` as an upgrade leaves it: given the survey tool its pack lacks,
/// which every file written before the tool is owed once.
fn owed_the_tool(played: &Household, registries: &GameRegistries) -> Household {
    let mut owed = played.clone();
    owed.give_survey_tool(&registries.items);
    owed
}

/// What the household knows of a deposit pinged at `at`: sampled and exact.
fn found_at(at: &[f64; 2]) -> strata::sim::deposits::Found {
    strata::sim::deposits::Found {
        reading: strata::registry::Reading::Exact,
        sampled: true,
        best_pm: 800,
        best_at: *at,
    }
}

/// A file written when posts stood has a `visited` list and no `found`:
/// each visited deposit reads as found and sampled, at its centre and its
/// peak, and the sim is handed that; the file written afterwards has the
/// found and no visited. One the world cannot find is left out.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn an_old_visited_list_reads_as_found_and_sampled() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let here = SurveyedIn {
        seed: 42,
        cell_m: 400.0,
    };
    let harrow = Deposit {
        id: DepositId([4, 1]),
        ty: strata::registry::ResourceTypeId(0),
        root: "Harrow".to_owned(),
        place: "Harrow edge".to_owned(),
        at: [3_500.0, 1_100.0],
        height_m: 20.0,
        figures: Vec::new(),
        radius_m: 200.0,
        peak: 800,
        grade_pm: 500,
    };
    let mut household = Household::starting(&registries, &Grants::default());
    household.surveyed_in = Some(here);
    // As the old writer wrote it: no `found`, no `survey_tool_given`.
    let text = ron::ser::to_string_pretty(&household, ron::ser::PrettyConfig::default()).unwrap();
    assert!(
        !text.contains("visited"),
        "a household with none writes none"
    );
    let old = text
        .replace("found: {},", "")
        .replace("survey_tool_given: true,", "")
        .replacen("tracked:", "visited: [(4, 1), (90, 90)],\n    tracked:", 1);
    assert!(old.contains("visited: [(4, 1)"), "{old}");
    let (read, upgraded, _) = Household::read(&old, &registries, &Grants::default).unwrap();
    assert_eq!(read.visited.len(), 2, "read as the file had it");
    assert!(read.found.is_empty());
    // The tool is owed too: this file never had one.
    assert_eq!(upgraded.map(|up| up.shape), Some("v5"));

    let find = |id: DepositId| (id == harrow.id).then(|| harrow.clone());
    let (surveyed, live) = survey_record(&read, Some(here), find);
    assert_eq!(
        surveyed.found,
        BTreeMap::from([(
            harrow.id,
            strata::sim::deposits::Found {
                reading: strata::registry::Reading::Exact,
                sampled: true,
                best_pm: 800,
                best_at: harrow.at,
            }
        )]),
        "found and sampled; the deposit the world cannot give is left out"
    );
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&read.store, &registries.items).unwrap());
    world.insert_resource(surveyed);
    world.insert_resource(live.unwrap());
    world.insert_resource(read);
    world.run_system_once(track_stock).unwrap();
    let saved = world.resource::<Household>();
    assert!(saved.visited.is_empty(), "the old list goes");
    assert_eq!(saved.found.len(), 1);
    let written = ron::ser::to_string_pretty(saved, ron::ser::PrettyConfig::default()).unwrap();
    assert!(!written.contains("visited"), "{written}");
    let (again, upgraded, _) = Household::read(&written, &registries, &Grants::default).unwrap();
    assert_eq!(&again, saved);
    assert_eq!(upgraded, None, "the tool was given once");
}

/// A household saved before the survey tool is given one at load, once:
/// into the first empty cell of each pack that carries none, the file kept
/// beside it as `.v5.bak`; what it is then written as is not given another,
/// even when the player has dropped the tool.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_household_saved_before_the_tool_is_given_one_once() {
    let registries = shipped();
    let items = &registries.items;
    let household = Household::starting(&registries, &Grants::default());
    assert!(household.survey_tool_given);
    let tool_in = |character: &Character| {
        character
            .pack
            .cells
            .iter()
            .chain(character.pack.bags.iter().flatten())
            .flatten()
            .filter(|record| record.item.to_string() == "veyra:tool.survey")
            .count()
    };
    assert_eq!(tool_in(&household.characters[0]), 1, "the first day's pack");
    // As written before the tool: every pack without one, no flag.
    let mut before = household.clone();
    for character in before.characters.iter_mut().chain(&mut before.recruits) {
        for cell in &mut character.pack.cells {
            if cell
                .as_ref()
                .is_some_and(|record| record.item.to_string() == "veyra:tool.survey")
            {
                *cell = None;
            }
        }
        // The player's first cell holds something, so the tool goes to the
        // next empty one.
        character.pack.cells[0] = Some(items.starting().unwrap().store[0].clone());
    }
    before.survey_tool_given = false;
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    let (read, upgraded, _) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(upgraded.map(|up| up.shape), Some("v5"));
    assert!(read.survey_tool_given);
    for character in read.characters.iter().chain(&read.recruits) {
        assert_eq!(tool_in(character), 1, "{}", character.name);
        assert!(
            character.pack.cells[0].is_some(),
            "what was in the first cell stays there"
        );
    }
    // Written again, it is given no second.
    let written = ron::ser::to_string_pretty(&read, ron::ser::PrettyConfig::default()).unwrap();
    let (again, upgraded, _) = Household::read(&written, &registries, &Grants::default).unwrap();
    assert_eq!(upgraded, None);
    assert_eq!(again, read);
    // The player drops it; it does not come back.
    let mut dropped = read.clone();
    for cell in &mut dropped.characters[0].pack.cells {
        if cell
            .as_ref()
            .is_some_and(|record| record.item.to_string() == "veyra:tool.survey")
        {
            *cell = None;
        }
    }
    let written = ron::ser::to_string_pretty(&dropped, ron::ser::PrettyConfig::default()).unwrap();
    let (back, upgraded, _) = Household::read(&written, &registries, &Grants::default).unwrap();
    assert_eq!((upgraded, tool_in(&back.characters[0])), (None, 0));
}

/// The purse and the survey record are the sim's while the game runs:
/// a lot sampled, a deposit found and a deposit tracked are copied to the
/// household, and nothing is written when nothing moved.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_purse_and_the_survey_are_copied_from_the_sim() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    household.purse = 2500;
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(Purse(2500));
    let here = SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    };
    let (surveyed, live) = survey_record(&household, Some(here), |_| None);
    assert_eq!(surveyed, Surveyed::default());
    world.insert_resource(surveyed);
    world.insert_resource(live.expect("a household with no record yet is this world's"));
    world.insert_resource(household.clone());
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), household);
    assert_eq!(household.surveyed_in, None, "nothing recorded, no world");

    world.resource_mut::<Purse>().0 = 2470;
    let harrow = Deposit {
        id: DepositId([4, 1]),
        ty: strata::registry::ResourceTypeId(0),
        root: "Harrow".to_owned(),
        place: "Harrow edge".to_owned(),
        at: [3_500.0, 1_100.0],
        height_m: 20.0,
        figures: Vec::new(),
        radius_m: 200.0,
        peak: 800,
        grade_pm: 500,
    };
    let mut surveyed = world.resource_mut::<Surveyed>();
    surveyed
        .found
        .insert(DepositId([3, -2]), found_at(&[3_000.0, -1_000.0]));
    surveyed.tracked = Some(harrow);
    world.run_system_once(track_stock).unwrap();
    let saved = world.resource::<Household>();
    assert_eq!(saved.purse, 2470);
    assert_eq!(
        saved.found,
        BTreeMap::from([(DepositId([3, -2]), found_at(&[3_000.0, -1_000.0]))])
    );
    assert_eq!(saved.tracked, Some(DepositId([4, 1])));
    assert_eq!(saved.surveyed_in, Some(here), "the record says whose it is");
    // The file holds them and reads back.
    let text = ron::ser::to_string_pretty(saved, ron::ser::PrettyConfig::default());
    let (read, upgraded, mended) =
        Household::read(&text.unwrap(), &registries, &Grants::default).unwrap();
    assert_eq!(&read, saved);
    assert_eq!(upgraded, None);
    assert_eq!(
        mended,
        Vec::<String>::new(),
        "nothing of the bench's to mend"
    );
}

/// A survey record belongs to the world it was made in. In that world
/// it is handed to the sim whole, an id the world no longer finds
/// included, and a tracked id it cannot find is kept in the file until
/// the player tracks something else. In another world (another seed,
/// another lattice) nothing of it is handed over, nothing the run
/// records is copied back, and the file keeps it as it was.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
#[allow(
    clippy::too_many_lines,
    reason = "one record, every world it is read in"
)]
fn a_survey_record_of_another_world_is_left_alone() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let here = SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    };
    let harrow = Deposit {
        id: DepositId([4, 1]),
        ty: strata::registry::ResourceTypeId(0),
        root: "Harrow".to_owned(),
        place: "Harrow edge".to_owned(),
        at: [3_500.0, 1_100.0],
        height_m: 20.0,
        figures: Vec::new(),
        radius_m: 200.0,
        peak: 800,
        grade_pm: 500,
    };
    let mut household = Household::starting(&registries, &Grants::default());
    household.found = BTreeMap::from([
        (DepositId([3, -2]), found_at(&[3_000.0, -1_000.0])),
        (DepositId([90, 90]), found_at(&[1.0, 1.0])),
    ]);
    household.tracked = Some(harrow.id);
    household.surveyed_in = Some(here);
    let find = |id: DepositId| (id == harrow.id).then(|| harrow.clone());

    // Its own world: everything, and the tracked deposit whole.
    let (surveyed, live) = survey_record(&household, Some(here), find);
    assert_eq!(surveyed.found, household.found, "none is dropped");
    assert_eq!(surveyed.tracked.as_ref(), Some(&harrow));
    let live = live.unwrap();
    assert_eq!((live.world, live.tracked), (here, Some(harrow.id)));

    // Another seed, another lattice, and no survey at all.
    for other in [Some(SurveyedIn { seed: 7, ..here }), None] {
        let (surveyed, live) = survey_record(&household, other, find);
        assert_eq!((surveyed, live), (Surveyed::default(), None), "{other:?}");
    }
    // The same seed on another lattice is dropped, so the run is live and
    // restamps.
    let mut older = household.clone();
    older
        .harvesters
        .list
        .push(super::harvest::machine(0, harrow.id));
    let today = SurveyedIn {
        cell_m: 400.0,
        ..here
    };
    assert!(drop_old_lattice(&mut older, Some(today)));
    assert!(older.found.is_empty() && older.tracked.is_none());
    assert_eq!(
        older.harvesters.list.len(),
        1,
        "machines carry their own lot and stay"
    );
    let (_, live) = survey_record(&older, Some(today), find);
    assert!(live.is_some(), "live after the drop");
    let mut other_seed = household.clone();
    assert!(!drop_old_lattice(
        &mut other_seed,
        Some(SurveyedIn { seed: 7, ..here })
    ));
    assert_eq!(other_seed, household, "another seed is kept");
    let run = |household: &Household, surveyed: Surveyed, live: Option<SurveyLive>| {
        let mut world = World::new();
        world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
            registries.clone(),
        )));
        world.insert_resource(
            HouseholdStore::from_saved(&household.store, &registries.items).unwrap(),
        );
        world.insert_resource(surveyed);
        if let Some(live) = live {
            world.insert_resource(live);
        }
        world.insert_resource(household.clone());
        world.run_system_once(track_stock).unwrap();
        // What the run then records: a deposit found, one let go.
        let mut surveyed = world.resource_mut::<Surveyed>();
        surveyed
            .found
            .insert(DepositId([1, 1]), found_at(&[1.0, 1.0]));
        surveyed.tracked = None;
        world.run_system_once(track_stock).unwrap();
        world.resource::<Household>().clone()
    };
    let elsewhere = run(&household, Surveyed::default(), None);
    assert_eq!(
        elsewhere, household,
        "another world's run changes none of it"
    );

    // In its own world, with the tracked id no longer found: handed
    // over as none, and still the file's until the player lets go.
    let (surveyed, live) = survey_record(&household, Some(here), |_| None);
    assert_eq!(surveyed.tracked, None);
    assert_eq!(live.unwrap().tracked, None);
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(surveyed);
    world.insert_resource(live.unwrap());
    world.insert_resource(household.clone());
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), household, "not a release");
    world.resource_mut::<Surveyed>().tracked = Some(Deposit {
        id: DepositId([5, 5]),
        ..harrow.clone()
    });
    world.run_system_once(track_stock).unwrap();
    assert_eq!(
        world.resource::<Household>().tracked,
        Some(DepositId([5, 5]))
    );

    // Home again the sim's changes are the household's.
    let (surveyed, live) = survey_record(&household, Some(here), find);
    let home = run(&household, surveyed, live);
    assert!(home.found.contains_key(&DepositId([1, 1])) && home.found.len() == 3);
    assert_eq!(home.tracked, None);
    assert_eq!(home.surveyed_in, Some(here));
}

#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_refused_file_is_copied_before_the_first_day() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let first = Household::starting(&registries, &Grants::default());

    // A file that is not a household at all.
    let garbage = "not a household (";
    std::fs::write(&path, garbage).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(read.first_day && !read.hold);
    assert_eq!(read.household, first);
    let kept = beside(&path, ".refused.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), garbage);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        garbage,
        "reading writes nothing over it"
    );
    // Read again unchanged, the copy is already there.
    Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!beside(&path, ".refused.2.bak").exists());

    // A file of a later build, a stack over its item's size and an item
    // that no longer exists are each refused, and each kept under a name
    // of its own: an earlier copy is never written over.
    let good = ron::ser::to_string_pretty(&first, ron::ser::PrettyConfig::default()).unwrap();
    let later = good.replacen("version: 5", "version: 6", 1);
    let heavy = good.replacen("count: 4", "count: 400", 1);
    let gone = good.replace("veyra:bag.haul_pack", "veyra:bag.sea_chest");
    let mut copies = vec![garbage.to_owned()];
    for refused in [later, heavy, gone] {
        assert_ne!(refused, good);
        std::fs::write(&path, &refused).unwrap();
        let read = Household::load_or_start(&path, false, &registries, &Grants::default);
        assert!(read.first_day && !read.hold);
        copies.push(refused);
    }
    for (index, copy) in copies.iter().enumerate() {
        let suffix = match index {
            0 => ".refused.bak".to_owned(),
            n => format!(".refused.{}.bak", n + 1),
        };
        assert_eq!(
            &std::fs::read_to_string(beside(&path, &suffix)).unwrap(),
            copy
        );
    }

    // `--fresh` is the player's own word to start over: no copy.
    let fresh_dir = tempfile::TempDir::new().unwrap();
    let fresh_path = fresh_dir.path().join("household.ron");
    std::fs::write(&fresh_path, garbage).unwrap();
    let read = Household::load_or_start(&fresh_path, true, &registries, &Grants::default);
    assert!(read.first_day && !read.hold);
    assert!(!beside(&fresh_path, ".refused.bak").exists());

    // What cannot be read cannot be copied either, so the run holds its
    // hand, and holding, writes nothing.
    let unreadable = dir.path().join("a_directory.ron");
    std::fs::create_dir(&unreadable).unwrap();
    let read = Household::load_or_start(&unreadable, false, &registries, &Grants::default);
    assert!(read.first_day && read.hold);
    let held = dir.path().join("held.ron");
    let mut world = World::new();
    world.insert_resource(first.clone());
    world.insert_resource(HouseholdFile {
        path: held.clone(),
        fresh: false,
        fitted: Vec::new(),
        in_hand: Vec::new(),
        learn: Vec::new(),
        grant: Vec::new(),
        condition: None,
        member: None,
        crafted_carbine: None,
        blanks: None,
        give: Vec::new(),
        bike: false,
        bike_condition: None,
        purse: None,
        saddle_stock: false,
        harvester: false,
        harvest_hours: None,
        open_harvester: false,
    });
    world.insert_resource(SaveHeld);
    world.run_system_once(save_on_change).unwrap();
    assert!(!held.exists());
    world.remove_resource::<SaveHeld>();
    world.resource_mut::<Household>().set_changed();
    world.run_system_once(save_on_change).unwrap();
    assert_eq!(
        ron::from_str::<Household>(&std::fs::read_to_string(&held).unwrap()).unwrap(),
        first
    );
}

/// A household saved before blueprints and the fabricator, as that
/// build wrote it.
#[derive(Serialize)]
struct BeforeBlueprints {
    version: u32,
    characters: Vec<Character>,
    active: usize,
    recruits: Vec<Character>,
    store: Vec<Option<Record>>,
    purse: u32,
    visited: BTreeSet<DepositId>,
    tracked: Option<DepositId>,
    surveyed_in: Option<SurveyedIn>,
    crafting: CraftState,
}

/// A file of the shape before blueprints is read whole, its stock, its
/// purse and its bench's work as they were, with no blueprint and no
/// run, and kept beside as `.v4.bak`; it is owed nothing, so nothing is
/// asked of the world and nothing is given. Written and read again it
/// is this build's.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
#[allow(
    clippy::too_many_lines,
    reason = "one file, read, recorded from, written and read again"
)]
fn a_version_four_file_loads_with_no_blueprints_and_no_job() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let asked = std::cell::Cell::new(0);
    let grants = || {
        asked.set(asked.get() + 1);
        Grants::default()
    };
    // A played household of that shape: a lot in the store, a fee
    // paid, and a core made at the bench, in the pack with its ledger.
    let mut played = Household::starting(&registries, &Grants::default());
    played.store.push(Some(lot_of(
        &registries,
        "strata:evaporite_salt",
        [3, -2],
        140,
    )));
    played.purse = 2380;
    let core = Record {
        item: "veyra:component.cell_core".parse().unwrap(),
        count: 1,
        batch: 1,
        by: Some("Frontier ranger".to_owned()),
        lines: BTreeMap::from([
            ("charge_density".to_owned(), 513),
            ("stability".to_owned(), 209),
        ]),
        condition: None,
        fitted: BTreeMap::new(),
        origin: None,
    };
    played.characters[0].pack.cells = vec![Some(core.clone())];
    played.crafting.rolls = 3;
    played.crafting.batches = 1;
    played.crafting.made.insert(
        1,
        crafting::Made {
            item: core.item.clone(),
            by: "Frontier ranger".to_owned(),
            from: vec![(
                "salt".to_owned(),
                lot_of(&registries, "strata:evaporite_salt", [3, -2], 20),
            )],
            schematic: None,
            recorded: None,
            blueprint: None,
        },
    );
    let old = BeforeBlueprints {
        version: 4,
        characters: played.characters.clone(),
        active: played.active,
        recruits: played.recruits.clone(),
        store: played.store.clone(),
        purse: played.purse,
        visited: played.visited.clone(),
        tracked: played.tracked,
        surveyed_in: played.surveyed_in,
        crafting: played.crafting.clone(),
    };
    let text = ron::ser::to_string_pretty(&old, ron::ser::PrettyConfig::default()).unwrap();
    assert!(text.contains("version: 4"));
    for later in ["blueprints", "factory", "schematic", "recorded"] {
        assert!(!text.contains(later), "{later}");
    }
    std::fs::write(&path, &text).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &grants);
    assert!(!read.first_day && !read.hold);
    assert_eq!(asked.get(), 0, "that shape is owed nothing of the world");
    let upgraded = read.household;
    assert_eq!(upgraded.version, Household::VERSION);
    assert_eq!(upgraded.version, 5);
    // Everything it had, it has, and it has no blueprint and no run.
    assert_eq!(upgraded, owed_the_tool(&played, &registries));
    assert_eq!(upgraded.blueprints, Blueprints::default());
    assert_eq!(upgraded.factory, FactoryState::default());
    assert_eq!(upgraded.crafting, played.crafting);
    assert_eq!(upgraded.check(&registries), Ok(()));
    let (_, upgrade, mended) = Household::read(&text, &registries, &grants).unwrap();
    assert_eq!(
        upgrade,
        Some(Upgrade {
            shape: "v4",
            short: 0,
            left_out: 0
        })
    );
    assert_eq!(mended, Vec::<String>::new());

    // The old file is beside the new one, to the byte, before anything
    // is written over it.
    let kept = beside(&path, ".v4.bak");
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(!beside(&path, ".v3.bak").exists());
    assert!(!beside(&path, ".mended.bak").exists());

    // The core it made before can be recorded now: its ledger entry
    // names no schematic, and one schematic makes a core.
    let (.., mut pack, mut hands) = upgraded.stats(&registries);
    let mut store = HouseholdStore::from_saved(&upgraded.store, &registries.items).unwrap();
    let (mut bench, mut prints) = (upgraded.crafting.clone(), upgraded.blueprints.clone());
    // A whole entry, as that build wrote it for a core.
    bench.made.get_mut(&1).unwrap().from.push((
        "brine".to_owned(),
        lot_of(&registries, "strata:coastal_brine", [5, 5], 30),
    ));
    let mut stock = Stock {
        saddle: None,
        pack: &mut pack,
        hands: &mut hands,
        store: Some(&mut store),
    };
    let (number, _) = blueprint::record(
        &registries,
        &mut stock,
        &mut bench,
        &mut prints,
        1,
        1000,
        "Frontier ranger",
    )
    .unwrap();
    assert_eq!(number, 2);
    assert_eq!(prints.0[&2].schematic.to_string(), SCRIPT_CORE);

    // Written, it is a file of this build's shape, and reading it
    // keeps no second copy.
    upgraded.save(&path).unwrap();
    let again = Household::load_or_start(&path, false, &registries, &grants);
    assert!(!again.first_day && !again.hold);
    assert_eq!(again.household, upgraded);
    assert_eq!(asked.get(), 0);
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), text);
    assert!(!beside(&path, ".v4.2.bak").exists());
    assert!(!beside(&path, ".refused.bak").exists());
}

/// Whether an actor's loadout is the bought carbine's action line and
/// nothing fitted.
fn only_the_action(actor: &strata::sim::combat::Combatant) -> bool {
    actor.loadout().is_some_and(|loadout| {
        loadout
            .mods
            .values()
            .flatten()
            .all(|factor| factor.property == strata::registry::item::Property::RepeatInterval)
    })
}

/// `--fitted` is the workbench's own transaction run before the player
/// spawns: the unit leaves the store, the weapon's record holds it, the
/// actor walks in equipped, and a module nothing in hand takes is
/// refused with the household as it was.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its household")]
fn a_module_fitted_at_the_start_comes_out_of_the_store() {
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    let bare = household
        .active()
        .actor(&registries.progression, &registries.items);
    // The bought carbine carries only its own action line.
    assert!(only_the_action(&bare));
    household.fit_at_start(SCRIPT_MODULE, &registries).unwrap();
    assert_eq!(
        stack(household.store[0].as_ref()),
        Some((FEED.to_owned(), 3, 0))
    );
    let carbine = household.active().hands[0].as_ref().unwrap();
    assert_eq!(carbine.fitted["feed"].item.to_string(), FEED);
    assert_eq!(household.check(&registries), Ok(()));
    let actor = household
        .active()
        .actor(&registries.progression, &registries.items);
    assert_eq!((bare.magazine(), actor.magazine()), (24, 29));
    assert_eq!(
        actor.ammo, 24,
        "the rounds they had, not a full fitted magazine"
    );
    // The file holds it, and the actor read back is equipped again.
    let text = ron::ser::to_string_pretty(&household, ron::ser::PrettyConfig::default());
    let (read, ..) = Household::read(&text.unwrap(), &registries, &Grants::default).unwrap();
    assert_eq!(read, household);

    let before = household.clone();
    assert_eq!(
        household.fit_at_start(SCRIPT_MODULE, &registries),
        Err("that part is fitted already".to_owned())
    );
    assert!(
        household
            .fit_at_start("veyra:bag.haul_pack", &registries)
            .is_err()
    );
    assert!(
        household
            .fit_at_start("veyra:module.none", &registries)
            .is_err()
    );
    assert_eq!(household, before);
}

/// The rounds a character left with are counted at a bench switch and
/// a module can come off afterwards. The actor made from the save (an
/// encounter reset, the next run) holds no more than its magazines do
/// now, in the hand and in the holster, and is never topped up.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its household")]
fn the_rounds_remembered_are_clamped_to_the_magazine_as_it_is() {
    let registries = shipped();
    let (graph, items) = (&registries.progression, &registries.items);
    let mut household = Household::starting(&registries, &Grants::default());
    household.fit_at_start(SCRIPT_MODULE, &registries).unwrap();
    let active = household.active;
    let fitted = household.active().actor(graph, items);
    assert_eq!(fitted.weapon, Weapon::Carbine);
    let (long, pistol) = (fitted.magazine(), fitted.spec_of(Weapon::Pistol).magazine);
    // Reloaded to the fitted magazine, then out to the bench and back.
    household.characters[active].ammo = long;
    let reloaded = household.active().actor(graph, items);
    assert_eq!((long, reloaded.ammo), (29, 29));

    // The module comes off while the save still says 29.
    let carbine = household.characters[active].hands[0].as_mut().unwrap();
    *carbine = fitting::bare_of(carbine);
    assert_eq!(household.check(&registries), Ok(()));
    let stripped = household.active().actor(graph, items);
    assert!(only_the_action(&stripped));
    assert_eq!((stripped.magazine(), stripped.ammo), (24, 24));
    assert_eq!(stripped.holstered_ammo, pistol);

    // The same with the carbine holstered, and a count under the
    // magazine is kept as it is.
    household.characters[active].weapon = Weapon::Pistol;
    household.characters[active].ammo = 3;
    household.characters[active].holstered_ammo = long;
    let holstered = household.active().actor(graph, items);
    assert_eq!((holstered.ammo, holstered.holstered_ammo), (3, 24));
}

/// Wear added fields to a record and none to the file's shape: a version 5
/// file written before it, with no repair kit in the store, is read as it
/// was, written over nothing and kept beside nothing.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_version_five_file_from_before_wear_loads_unchanged() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let mut before = Household::starting(&registries, &Grants::default());
    assert_eq!(before.version, 5);
    // A module fitted and never worn: the one record that gained a field.
    before.fit_at_start(SCRIPT_MODULE, &registries).unwrap();
    assert_eq!(
        before.active().hands[0].as_ref().unwrap().fitted["feed"].condition,
        None
    );
    before.store.retain(|cell| {
        cell.as_ref()
            .is_none_or(|record| record.item.to_string() != "veyra:kit.field_repair")
    });
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    std::fs::write(&path, &text).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no copy");
}

/// A file from before the weapon slots were free holds a carbine and a
/// pistol in hand: that satisfies the new rule, loads as it was and is
/// written back the same, with no version change. The same file with two
/// carbines is a household the rule refuses, kept beside and started over.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_carbine_and_a_pistol_in_hand_load_unchanged_and_two_of_a_family_are_refused() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let before = Household::starting(&registries, &Grants::default());
    assert_eq!(before.version, 5, "no version change");
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    std::fs::write(&path, &text).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, before);
    // Spawned and saved again, the file is the same.
    let hands = read.household.active().carried(&registries.items).1;
    let saved = hands.saved(&registries.items).unwrap();
    assert_eq!(saved.each_ref().map(Option::is_some), [true, true]);
    assert_eq!(registries.items.check_hands(&saved), Ok(()));
    assert_eq!(
        ron::ser::to_string_pretty(&read.household, ron::ser::PrettyConfig::default()).unwrap(),
        text
    );
    // Two carbines are refused on the way in.
    let twice = text.replace("veyra:weapon.service_pistol", "veyra:weapon.patrol_carbine");
    assert_ne!(twice, text);
    std::fs::write(&path, &twice).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(read.first_day && !read.hold, "started over");
    assert_eq!(
        std::fs::read_to_string(beside(&path, ".refused.bak")).unwrap(),
        twice,
        "and the refused file is kept"
    );
}

/// A version 5 file written before the buyers' boards has no ledger, and
/// loads with an empty one: the field has a serde default and the version
/// did not move.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real data")]
fn a_version_five_file_without_a_ledger_loads_with_none() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let before = Household::starting(&registries, &Grants::default());
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    let mut cut = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.starts_with("    procurement:") {
            skipping = true;
        }
        if !skipping {
            cut.push(line);
        } else if line == "    )," {
            skipping = false;
        }
    }
    let old = cut.join("\n");
    assert!(!old.contains("procurement"));
    std::fs::write(&path, &old).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household.procurement, ProcurementState::default());
    assert_eq!(read.household, before);
}

/// A sale survives a save and a reload: what is taken of a line and the
/// ledger with its batch and maker.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real data")]
fn a_sale_survives_a_save_and_a_reload() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let buyer = strata::registry::Namespaced::parse("veyra:buyer.pelagic_works").unwrap();
    let mut household = Household::starting(&registries, &Grants::default());
    household
        .procurement
        .taken
        .insert(strata::sim::procurement::key(&buyer, "cell_cores"), 40);
    household
        .procurement
        .sales
        .push(strata::sim::procurement::Sale {
            buyer,
            line: "cell_cores".to_owned(),
            item: strata::registry::Namespaced::parse("veyra:component.cell_core").unwrap(),
            batch: 412,
            by: Some("Tovan Reyes".to_owned()),
            origin: None,
            count: 40,
            paid: 5_875,
        });
    household.save(&path).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day);
    assert_eq!(read.household.procurement, household.procurement);
    assert_eq!(read.household, household);
}

/// A version 5 file written while a lot's origin carried the terms it was
/// drawn on (the writer before ticket 0009 put `terms` after `place`) loads
/// with the lot in the store whole, though nothing looks its deposit up: the figures, the root and the place are the lot's own.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_lot_saved_with_its_terms_loads_whole() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let cell = [31_415, -27_182];
    let lot = lot_of(&registries, "strata:evaporite_salt", cell, 160);
    let mut before = Household::starting(&registries, &Grants::default());
    assert_eq!(before.version, 5);
    before.store.push(Some(lot.clone()));
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    // The old writer's origin, as the game wrote it: the words, then the
    // terms.
    let place = "                place: \"Calder flats\",\n";
    assert_eq!(text.matches(place).count(), 1, "{text}");
    let old = text.replace(
        place,
        &format!("{place}                terms: \"Pelagic concession\",\n"),
    );
    assert!(old.contains("terms: \"Pelagic concession\""));
    std::fs::write(&path, &old).unwrap();

    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    let kept = read
        .household
        .store
        .iter()
        .flatten()
        .find(|record| record.origin.as_ref().is_some_and(|o| o.deposit == cell))
        .expect("the lot is in the store");
    assert_eq!(kept, &lot);
    assert!(!kept.lines.is_empty());
    let origin = kept.origin.as_ref().unwrap();
    assert_eq!(
        (origin.root.as_str(), origin.place.as_str()),
        ("Calder", "Calder flats")
    );
    assert_eq!(read.household, before);
    assert_eq!(read.household.check(&registries), Ok(()));
}

/// A bike, a worn one and a loaded saddle go through the sim's stable, the
/// household, the file and back whole.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real data")]
fn a_worn_bike_and_a_loaded_saddle_round_trip_the_save() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let items = &registries.items;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let vehicle = strata::registry::Namespaced::parse("veyra:vehicle.courier_bike").unwrap();
    let mut household = Household::starting(&registries, &Grants::default());
    assert_eq!(household.stable, strata::sim::ride::SavedStable::default());
    household.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("bike: Some"), "no bike, none written");

    let mut worn = strata::sim::ride::store_grade_bike(items, &vehicle).unwrap();
    worn.condition = Some(325);
    worn.lines.insert("speed".to_owned(), 720);
    let lot = lot_of(&registries, "strata:evaporite_salt", [31_415, -27_182], 160);
    household.stable.bike = Some(worn.clone());
    household.stable.saddle = vec![None, Some(lot.clone()), None, Some(lot.clone())];
    assert_eq!(household.check(&registries), Ok(()));
    household.save(&path).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, household);

    // Into the sim: the stable stands as saved, and the sim's own change
    // (a crash wearing the bike) is what the household then holds.
    let stable =
        strata::sim::ride::Stable::from_saved(&read.household.stable, items, &registries.vehicles)
            .unwrap();
    assert_eq!(stable.bike.as_ref(), Some(&worn));
    assert_eq!(stable.saddle.len(), 18, "sized by the cargo");
    assert!(stable.saddle.get(0).is_none() && stable.saddle.get(1).is_some());
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&read.household.store, items).unwrap());
    let (.., pack, hands) = read.household.stats(&registries);
    world.spawn((LocalPlayer, pack, hands));
    world.insert_resource(read.household.clone());
    world.insert_resource(stable);
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), read.household);
    world
        .resource_mut::<strata::sim::ride::Stable>()
        .bike
        .as_mut()
        .unwrap()
        .condition = Some(75);
    world.run_system_once(track_stock).unwrap();
    assert_eq!(
        world
            .resource::<Household>()
            .stable
            .bike
            .as_ref()
            .unwrap()
            .condition,
        Some(75)
    );
    assert_eq!(
        world.resource::<Household>().stable.saddle,
        household.stable.saddle,
        "the saddle is as it was"
    );
    // A saddle with no bike, and a bike of no vehicle, are refused.
    let mut broken = household.clone();
    broken.stable.bike = None;
    assert!(broken.check(&registries).is_err());
    let mut unknown = household.clone();
    unknown.stable.bike.as_mut().unwrap().item =
        strata::registry::Namespaced::parse("veyra:kit.field_repair").unwrap();
    assert!(unknown.check(&registries).is_err());
}

/// A file saved when the household owned a bike by vehicle id (the first
/// half of step 2) loads with a store grade bike of that vehicle, stabled,
/// and the old key is not written again.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real data")]
fn the_old_bike_key_loads_as_a_store_grade_bike() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let mut before = Household::starting(&registries, &Grants::default());
    before.bike = Some(strata::registry::Namespaced::parse("veyra:vehicle.courier_bike").unwrap());
    // The old writer's file: the id in `bike`, nothing in `stable`.
    let text = ron::ser::to_string_pretty(&before, ron::ser::PrettyConfig::default()).unwrap();
    assert!(
        text.contains("bike: Some(\"veyra:vehicle.courier_bike\")"),
        "{text}"
    );
    std::fs::write(&path, &text).unwrap();
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    let bike = read.household.stable.bike.as_ref().expect("a bike stabled");
    assert_eq!(bike.item.to_string(), "veyra:vehicle.courier_bike");
    assert_eq!(bike.condition, Some(1000));
    assert!(bike.lines.values().all(|line| *line == 500));
    assert_eq!(read.household.bike, None);
    read.household.save(&path).unwrap();
    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(
        !rewritten.contains("bike: Some(\"veyra:vehicle"),
        "{rewritten}"
    );
    let again = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert_eq!(again.household, read.household);
}

/// `--learn` puts boxes on the sheet past their prerequisites, so a still
/// can hold a greatblade nobody starts certified for; an unknown box
/// changes nothing.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real data")]
fn learn_at_start_certifies_the_member_for_a_greatblade() {
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    let greatblade = ["veyra:weapon.yard_greatblade".to_owned()];
    assert!(
        household
            .in_hand_at_start(&greatblade.clone().map(|id| id), &registries)
            .is_err(),
        "nobody starts certified on the greatblade"
    );
    let before = household.clone();
    assert!(
        household
            .learn_at_start(&["no.such.box".to_owned()], &registries)
            .is_err()
    );
    assert_eq!(household, before, "nothing changed");
    household
        .learn_at_start(&["blade.breaking.1".to_owned()], &registries)
        .unwrap();
    household
        .in_hand_at_start(&["-".to_owned(), greatblade[0].clone()], &registries)
        .unwrap();
    let held = household.active().hands[1].as_ref().unwrap();
    assert_eq!(held.item.to_string(), greatblade[0]);
}

/// The played clock and the camps cleared are copied from the sim, written,
/// read back and stood again in their own world only; in another world the
/// record stays in the file and is not stood.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_played_clock_and_the_cleared_camps_round_trip_the_save() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let here = SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    };
    let household = Household::starting(&registries, &Grants::default());
    assert_eq!(household.cleared, Cleared::default());
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(Surveyed::default());
    world.insert_resource(SurveyLive {
        world: here,
        tracked: None,
    });
    world.insert_resource(PlayedClock { seconds: 5_000.0 });
    world.insert_resource(Cleared::default());
    world.insert_resource(household);
    world.run_system_once(track_stock).unwrap();
    // With nothing changed the household is not touched: a write of the file
    // every frame is what a `DerefMut` on it would cost.
    let track = world.register_system(track_stock);
    let probe = world.register_system(|household: Res<Household>| household.is_changed());
    world.run_system(track).unwrap();
    world.run_system(probe).unwrap();
    world.run_system(track).unwrap();
    assert!(
        !world.run_system(probe).unwrap(),
        "track_stock changed the household with nothing to copy"
    );
    // A camp cleared: copied with the clock it was cleared on, and stamped.
    let mut cleared = Cleared::default();
    cleared.clear(77, 5_000.0);
    world.insert_resource(cleared);
    world.run_system_once(track_stock).unwrap();
    let saved = world.resource::<Household>().clone();
    assert_eq!(saved.cleared.at.get(&77), Some(&5_000.0));
    assert_eq!(saved.played, PlayedClock { seconds: 5_000.0 });
    assert_eq!(saved.surveyed_in, Some(here));
    // Through the file and back.
    let text = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()).unwrap();
    let (read, _, _) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(read.cleared, saved.cleared);
    assert_eq!(read.played, saved.played);
    assert_eq!(stood_cleared(&read, true), saved.cleared);
    assert_eq!(
        stood_cleared(&read, false),
        Cleared::default(),
        "another world's record is kept and not read"
    );
}

/// The deeds, the commons' tallies and the leases are copied from the sim
/// only when they change, written, read back and stood again in their own
/// world only; a run of `track_stock` with nothing changed leaves the
/// household alone.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_deeds_and_the_leases_round_trip_the_save() {
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::deeds::{DeedKind, Rules};
    use strata::worldgen::deposits::DepositId;
    use strata::worldgen::factions::FactionId;
    let registries = shipped();
    let here = SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    };
    let household = Household::starting(&registries, &Grants::default());
    assert_eq!(household.deeds, Deeds::default());
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(Surveyed::default());
    world.insert_resource(SurveyLive {
        world: here,
        tracked: None,
    });
    world.insert_resource(Deeds::default());
    world.insert_resource(CommonsTaken::default());
    world.insert_resource(Leases::default());
    world.insert_resource(household);
    world.run_system_once(track_stock).unwrap();
    // Nothing changed: the household is not touched.
    let track = world.register_system(track_stock);
    let probe = world.register_system(|household: Res<Household>| household.is_changed());
    world.run_system(track).unwrap();
    world.run_system(probe).unwrap();
    world.run_system(track).unwrap();
    assert!(
        !world.run_system(probe).unwrap(),
        "track_stock changed the household with nothing to copy"
    );
    // A deed, a tally and a lease: copied and stamped.
    let faction = FactionId(7);
    let rules = Rules::of(&registries).unwrap();
    world
        .resource_mut::<Deeds>()
        .write(faction, DeedKind::WorkedReserve, 3, 100.0, rules.deeds);
    world
        .resource_mut::<CommonsTaken>()
        .by
        .insert(DepositId([3, -2]), (0, 12));
    world
        .resource_mut::<Leases>()
        .until
        .insert(faction.0, 21_700.0);
    world.run_system_once(track_stock).unwrap();
    let saved = world.resource::<Household>().clone();
    assert_eq!(saved.deeds.by.get(&faction.0).map(Vec::len), Some(1));
    assert_eq!(saved.commons.by.len(), 1);
    assert_eq!(saved.leases.until.get(&faction.0), Some(&21_700.0));
    assert_eq!(saved.surveyed_in, Some(here));
    // Through the file and back, in this world and in another.
    let text = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()).unwrap();
    let (read, _, _) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(read.deeds, saved.deeds);
    assert_eq!(read.leases, saved.leases);
    let (deeds, commons, leases) = stood_deeds(&read, true);
    assert_eq!(
        (deeds, commons, leases),
        (
            saved.deeds.clone(),
            saved.commons.clone(),
            saved.leases.clone()
        )
    );
    assert_eq!(
        stood_deeds(&read, false),
        Default::default(),
        "another world's record is kept and not read"
    );
}
