//! Tests of the progression script, the scripted hurt and the whole
//! scripted path with its crafting report.

use super::*;

/// An opponent fitted with its authored sheet, as the game spawns it.
fn opponent(name: Option<&str>, health: f32) -> Combatant {
    let graph = SkillGraph::shipped();
    let sheets = AuthoredSheets::shipped();
    let sheet = name
        .and_then(|name| sheets.opponents.get(name))
        .map(CharacterSheet::authored)
        .unwrap_or_default();
    let tables = strata::sim::combat_tables::current();
    let mut actor = Combatant::on_team(1, health);
    actor.damage_scale = tables.opponents.damage_scale;
    actor.fit(&graph, &sheet);
    actor
}

/// The household's smith in the saved file: the one who has the box that
/// teaches the carbine, who made everything at the bench.
fn smith(saved: &Household) -> &Character {
    &saved.characters[saved
        .index_with(SCRIPT_SMITH_BOX)
        .expect("the smith joined")]
}

/// `--script-hurt 2`: from four seconds into the fight the
/// nearest hostile that can strike lands a blow a second at its
/// table's value, twice; the training target, nearer still, cannot
/// strike and is passed over.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_scripted_hurt_lands_its_blows_at_the_tables_value() {
    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::combat::CombatImpact;
    let mut world = World::new();
    world.init_resource::<Messages<CombatImpact>>();
    world.insert_resource(ProgressionScript {
        hurts: 2,
        since: HURT_AT_SECS - 0.5,
        ..default()
    });
    let player = world
        .spawn((
            LocalPlayer,
            WorldPosition::new(0., 0., 0.),
            Combatant::default(),
        ))
        .id();
    let tables = strata::sim::combat_tables::current();
    world.spawn((
        WorldPosition::new(0., 0., 3.),
        opponent(None, tables.opponents.passive_health),
    ));
    let sentry = opponent(Some("sentry"), tables.opponents.health);
    let (damage, _, melee) = strike(&sentry).unwrap();
    assert!(!melee, "the sentry fires its carbine");
    let shot = sentry.weapon_spec().damage * tables.opponents.damage_scale;
    assert!((damage - shot).abs() < 1e-4, "{damage} vs {shot}");
    let sentry = world.spawn((WorldPosition::new(0., 0., 12.), sentry)).id();
    world.spawn((
        WorldPosition::new(0., 0., 30.),
        opponent(Some("rusher"), tables.opponents.health),
    ));
    let full = world.get::<Combatant>(player).unwrap().clone();

    world.run_system_once(script_hurt).unwrap();
    assert!(
        world.resource::<Messages<CombatImpact>>().is_empty(),
        "not before {HURT_AT_SECS} s"
    );
    for at in [0.0, 0.5, 1.0, 2.0, 3.0] {
        world.resource_mut::<ProgressionScript>().since = HURT_AT_SECS + at;
        world.run_system_once(script_hurt).unwrap();
    }

    let impacts: Vec<CombatImpact> = world
        .resource_mut::<Messages<CombatImpact>>()
        .drain()
        .collect();
    assert_eq!(impacts.len(), 2, "two blows, a second apart, then none");
    for impact in &impacts {
        assert_eq!(impact.attacker, sentry, "the nearest that can strike");
        assert_eq!(impact.victim, Some(player));
        assert!((impact.dealt - damage).abs() < 1e-4);
    }
    let hurt = world.get::<Combatant>(player).unwrap();
    let before = full.health + full.shield;
    assert!(
        (before - (hurt.health + hurt.shield) - 2. * damage).abs() < 1e-3,
        "the pools fell by the blows"
    );
    assert_eq!(hurt.hurts, full.hurts + 2, "the sim's own hit");
}

/// `--hold-guard` with `--script-hurt`: the player's blade guard is up and
/// facing the sentry, and the scripted blow comes from where the hostile
/// stands, so it is blocked and takes the guard's share off.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_scripted_hurt_meets_a_held_guard_and_is_blocked() {
    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::combat::{CombatImpact, CombatIntent, CombatMotion, GuardOutcome};
    let mut world = World::new();
    world.init_resource::<Messages<CombatImpact>>();
    world.insert_resource(ProgressionScript {
        hurts: 1,
        since: HURT_AT_SECS + 1.,
        ..default()
    });
    let mut blade = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
    let (mut input, mut motion) = (
        CombatIntent {
            aim: true,
            ..default()
        },
        CombatMotion::default(),
    );
    // Facing +z, where the sentry stands, with the button held for a second.
    let toward = strata::sim::PlayerIntent {
        yaw: std::f32::consts::PI,
        ..default()
    };
    for _ in 0..60 {
        blade.step(&mut input, &toward, 1. / 60., &mut motion);
    }
    assert!(blade.guard_up);
    let player = world
        .spawn((LocalPlayer, WorldPosition::new(0., 0., 0.), blade))
        .id();
    let tables = strata::sim::combat_tables::current();
    let sentry = opponent(Some("sentry"), tables.opponents.health);
    let (damage, _, _) = strike(&sentry).unwrap();
    world.spawn((WorldPosition::new(0., 0., 12.), sentry));
    world.run_system_once(script_hurt).unwrap();
    let impacts: Vec<CombatImpact> = world
        .resource_mut::<Messages<CombatImpact>>()
        .drain()
        .collect();
    assert_eq!(impacts.len(), 1);
    assert_eq!(impacts[0].guard, GuardOutcome::Blocked);
    let share = strata::sim::combat_tables::with_melee(|melee| melee.class(Weapon::Blade))
        .unwrap()
        .guard
        .share;
    assert!((impacts[0].dealt - damage * (1. - share)).abs() < 1e-3);
    assert!(world.get::<Combatant>(player).unwrap().guard_up);
}

/// What `--script-crafting` leaves behind: the report of the workbench
/// stop and the stock in the saved household.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn crafted(report: &Path, saved: &Household) {
    // The workbench stop, between the lesson and the terminal: the
    // store's magazine on the carbine's feed and off again, read off
    // the `Combatant` the HUD reads.
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let arrival = smith(saved);
    let (bare, fitted) = (report.bare.unwrap(), report.fitted.unwrap());
    // The smith holds the second box of the carbine, which is a quicker
    // reload and nothing else of the row.
    let standard = Weapon::Carbine.spec();
    // And the bought carbine's own action line, 1.10 of the table's interval.
    assert!((bare.interval - standard.interval * 1.1).abs() < 1e-6);
    assert_eq!(
        WeaponSpec {
            reload: standard.reload,
            interval: standard.interval,
            ..bare
        },
        standard,
        "the smith's boxes move the carbine's reload and nothing else"
    );
    assert!((bare.reload - standard.reload * 0.85).abs() < 1e-4);
    assert_eq!(fitted.magazine, 29);
    assert!(
        (fitted.reload - bare.reload * 1.1).abs() < 1e-4,
        "{} against {}",
        fitted.reload,
        bare.reload
    );
    assert_eq!(
        WeaponSpec {
            magazine: bare.magazine,
            reload: bare.reload,
            ..fitted
        },
        bare,
        "the magazine moved the magazine and the reload and nothing else"
    );
    // Three fired from a full 24: fitting keeps the 21 and refills
    // nothing; the reload fills 29; two fired, and the removal leaves
    // what the bare magazine holds.
    assert_eq!(report.ammo_at_fit, (21, 21));
    assert_eq!(report.reloaded_to, 29);
    assert_eq!(report.ammo_at_remove, (27, 24));
    assert_eq!(report.restored, Some(bare), "the row is restored exactly");
    // In the file the store's magazine is fitted to nothing any more:
    // it is in the smith's pack and the store is one short. What is
    // on the carbine's feed is the magazine made at the bench
    // afterwards, batch 2 ([`made`]), and nobody else has anything
    // fitted.
    for character in &saved.characters {
        for (index, hand) in character.hands.iter().enumerate() {
            let fitted: Vec<(&str, u32)> = hand
                .iter()
                .flat_map(|hand| &hand.fitted)
                .map(|(mount, module)| (mount.as_str(), module.batch))
                .collect();
            let expected: &[(&str, u32)] = if character.name == arrival.name && index == 0 {
                &[("feed", 2)]
            } else {
                &[]
            };
            assert_eq!(fitted, expected, "{}: hand {index}", character.name);
        }
    }
    assert_eq!(
        stack(arrival.pack.cells[0].as_ref()),
        Some((FEED.to_owned(), 1, 0))
    );
    assert_eq!(
        stack(arrival.pack.cells[1].as_ref()),
        Some(("veyra:component.cell_core".to_owned(), 1, 3)),
        "the second core"
    );
    let others: Vec<String> = arrival.pack.cells[2..]
        .iter()
        .flatten()
        .map(|record| record.item.to_string())
        .collect();
    assert_eq!(
        others,
        ["veyra:tool.survey"],
        "and nothing else in the pack but the survey tool"
    );
    assert_eq!(
        stack(saved.store[0].as_ref()),
        Some((FEED.to_owned(), 3, 0))
    );
    assert_eq!(
        stack(saved.store[1].as_ref()),
        Some((FEED.to_owned(), 2, 412))
    );
}

/// Whether `record` is a lot that is `of` a kind.
fn lot_is(registries: &GameRegistries, record: &Record, of: &LotOf) -> bool {
    let Some(ItemKind::Lot {
        resource, class, ..
    }) = registries.items.get(&record.item).map(|def| &def.kind)
    else {
        return false;
    };
    match of {
        LotOf::Type(ty) => resource == ty,
        LotOf::Class(wanted) => class == wanted,
    }
}

/// The units of lots that are `of` a kind which the bench's ledger says
/// went into something made by hand there.
fn spent(saved: &Household, registries: &GameRegistries, of: &LotOf) -> u32 {
    saved
        .crafting
        .made
        .values()
        .filter(|made| made.blueprint.is_none())
        .flat_map(|made| &made.from)
        .filter(|(_, record)| lot_is(registries, record, of))
        .map(|(_, record)| record.count)
        .sum()
}

/// The units of lots that are `of` a kind which went into the
/// fabricator's batches: a unit's worth by the ledger, for every unit
/// of the batch the store holds.
fn run_off(saved: &Household, registries: &GameRegistries, of: &LotOf) -> u32 {
    saved
        .crafting
        .made
        .iter()
        .filter(|(_, made)| made.blueprint.is_some())
        .map(|(batch, made)| {
            // What the run made is in the store or, once sold, the ledger.
            let units: u32 = saved
                .store
                .iter()
                .flatten()
                .filter(|record| record.batch == *batch && record.item == made.item)
                .map(|record| record.count)
                .sum::<u32>()
                + saved
                    .procurement
                    .sales
                    .iter()
                    .filter(|sale| sale.batch == *batch && sale.item == made.item)
                    .map(|sale| sale.count)
                    .sum::<u32>();
            let a_unit: u32 = made
                .from
                .iter()
                .filter(|(_, record)| lot_is(registries, record, of))
                .map(|(_, record)| record.count)
                .sum();
            units * a_unit
        })
        .sum()
}

/// Every unit of lots that are `of` a kind that went into anything, by
/// hand or at the fabricator.
fn used(saved: &Household, registries: &GameRegistries, of: &LotOf) -> u32 {
    spent(saved, registries, of) + run_off(saved, registries, of)
}

/// What `--script-crafting` leaves behind of the bench's crafting: two
/// cell cores from the same lots that differ, the magazine made of the
/// first, on the carbine and holding what the bench said it would, the
/// second core in the pack, the lots down by what the three took and
/// the ledger saying what each was made from.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
#[allow(
    clippy::too_many_lines,
    reason = "everything the three crafts left, in the order they were made"
)]
fn made(report: &Path, saved: &Household) {
    let registries = shipped();
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let made = &report.made;
    let arrival = smith(saved);
    let within = |lines: &BTreeMap<String, u16>, range: &[(String, u16, u16)]| {
        assert_eq!(lines.len(), range.len(), "{lines:?} of {range:?}");
        for (line, floor, ceiling) in range {
            assert!(
                (*floor..=*ceiling).contains(&lines[line]),
                "{line}: {} of {floor} to {ceiling}",
                lines[line]
            );
        }
    };
    // Both cores stand between the floor and the ceiling the bench
    // showed before the first was assembled, and they differ: the same
    // lots, the same hand, the same line, other rolls.
    let lines: Vec<&str> = made
        .core_range
        .iter()
        .map(|(line, ..)| line.as_str())
        .collect();
    assert_eq!(lines, ["charge_density", "stability"]);
    within(&made.core, &made.core_range);
    within(&made.second, &made.core_range);
    assert_ne!(made.core, made.second, "two crafts from the same lots");
    // Every point went on charge density, so stability stands at its
    // floor in both.
    let floor = made.core_range[1].1;
    assert_eq!(
        (made.core["stability"], made.second["stability"]),
        (floor, floor)
    );
    assert!(made.core["charge_density"] > made.core_range[0].1);
    // The magazine was made with that core, inside what the bench
    // showed, and its capacity was worked on.
    let lines: Vec<&str> = made
        .magazine_range
        .iter()
        .map(|(line, ..)| line.as_str())
        .collect();
    assert_eq!(lines, ["capacity", "reload_time"]);
    within(&made.magazine, &made.magazine_range);
    assert!(made.magazine["capacity"] > made.magazine_range[0].1);
    // Four points, six and four (the smith's tuning box is one more in
    // each), a roll each; three batches. The household's counter went on
    // to five: the blueprint took the fourth number and the
    // fabricator's batch the fifth ([`ran`]); the carbine made last took
    // the sixth, with five rolls: the soft alloy's ceiling on moddability
    // came before the nine points ran out ([`carbine`]); the smith's rework
    // of it drew one more for each blank spent.
    assert_eq!(made.rolled.len(), 14, "{:?}", made.rolled);
    assert_eq!((made.rolls, made.batches), (14, 3));
    let reworks = report.carbine.reworks.len() as u64;
    assert_eq!(
        (saved.crafting.rolls, saved.crafting.batches),
        (19 + reworks, 6)
    );
    assert_eq!(saved.crafting.session, None, "nothing is left on the bench");

    // Fitted, the `Combatant` holds the rounds and the reload the bench
    // showed before Finish, by the fitting step's own arithmetic.
    let (magazine, reload) = made.fitted.expect("the made magazine was fitted");
    assert_eq!(i32::from(magazine), made.previewed.0);
    // The figure is the reload rounded to the hundredth it is read at.
    assert!((f64::from(reload) * 100. - f64::from(made.previewed.1)).abs() <= 0.5);
    assert!((27..=31).contains(&magazine), "{magazine}");
    let fitted = &arrival.hands[0].as_ref().unwrap().fitted["feed"];
    assert_eq!(fitted.item.to_string(), FEED);
    assert_eq!(
        (fitted.batch, fitted.by.as_deref()),
        (2, Some("Tovan Reyes"))
    );
    assert_eq!(fitted.lines, made.magazine);
    let actor = arrival.actor(&registries.progression, &registries.items);
    assert_eq!(actor.spec_of(Weapon::Carbine).magazine, magazine);

    // The second core is in the pack; the first went into the magazine.
    let second = arrival.pack.cells[1].as_ref().unwrap();
    assert_eq!(
        (second.batch, second.by.as_deref()),
        (3, Some("Tovan Reyes"))
    );
    assert_eq!(second.lines, made.second);
    // The ledger says what each was made from: the cores from the same
    // two lots, the magazine from the first core, a bought harness, the
    // alloy and a seal set. The fifth entry is the fabricator's batch.
    let ledger = &saved.crafting.made;
    assert_eq!(ledger.keys().copied().collect::<Vec<_>>(), [1, 2, 3, 5, 6]);
    assert_eq!(ledger[&1].from, ledger[&3].from);
    let slots: Vec<(&str, u32)> = ledger[&1]
        .from
        .iter()
        .map(|(slot, record)| (slot.as_str(), record.count))
        .collect();
    assert_eq!(slots, [("salt", 20), ("brine", 30)]);
    let slots: Vec<(&str, u32)> = ledger[&2]
        .from
        .iter()
        .map(|(slot, record)| (slot.as_str(), record.count))
        .collect();
    assert_eq!(
        slots,
        [("core", 1), ("harness", 1), ("casing", 40), ("seals", 1)]
    );
    let core = &ledger[&2].from[0].1;
    assert_eq!((core.batch, &core.lines), (1, &made.core));
    assert_eq!(ledger[&2].item.to_string(), FEED);
    for entry in ledger.values() {
        assert_eq!(entry.by, "Tovan Reyes");
    }
    // The lots are down 40, 60 and 120 (the carbine's frame took 80 of
    // the alloy), and the store two harnesses and two seal sets.
    let starting = registries.items.starting().unwrap();
    let down: Vec<u32> = starting
        .lots
        .iter()
        .map(|lot| spent(saved, &registries, &lot.of))
        .collect();
    assert_eq!(down, [40, 60, 120]);
    let held = |item: &str| -> u32 {
        saved
            .store
            .iter()
            .flatten()
            .filter(|record| record.item.to_string() == item)
            .map(|record| record.count)
            .sum()
    };
    assert_eq!(held("veyra:component.harness"), 1);
    assert_eq!(held("veyra:commodity.seal_set"), 4);
    // No core made by hand is in the store, and the fabricator's three
    // were sold at the board ([`sold`]).
    assert_eq!(held("veyra:component.cell_core"), 0);
}

/// What `--script-crafting` leaves behind of its stop at a survey post:
/// the lot read in the pack with its deposit's own figures, the purse
/// as it was, the lot in the household's store and the pack as it
/// was; and the first day's lots, drawn from real deposits.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
#[allow(
    clippy::too_many_lines,
    reason = "everything the stop at the post left, in the order it happened"
)]
fn drawn(report: &Path, saved: &Household) {
    let registries = shipped();
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let draw = report.draw.expect("a lot was drawn at a survey post");
    // The lot the pack held read the deposit's figures, line for line,
    // and no line the deposit has no figure for.
    let mut figures: Vec<(String, u16)> = draw
        .figures
        .iter()
        .map(|(attribute, value)| (attribute.key().to_owned(), *value))
        .collect();
    figures.sort();
    let mut lines = draw.lot_lines.clone();
    lines.sort();
    assert!(figures.len() >= 3, "{figures:?}");
    assert_eq!(lines, figures, "the lot's lines are the deposit's figures");
    // Sampled in cycles of a few units, until the script had the ten it
    // stops at: at least that, and the lot the pack held is what was got.
    let offered = draw.units.0;
    assert!(offered >= 10, "{offered} units sampled");
    assert_eq!(draw.units, (offered, offered));
    // Sampling is free: the first day's purse is what it was; the file
    // holds what the run then bought and sold.
    let purse = registries.items.starting().unwrap().purse;
    assert_eq!(purse, 2500);
    assert_eq!(draw.purse_before, purse);
    assert_eq!(draw.purse_after, purse);
    // The sale of the run's cores came after the draw and paid into it.
    let sales: u32 = saved.procurement.sales.iter().map(|sale| sale.paid).sum();
    // and the smith bought a mount blank for each rework ([`carbine`]).
    let blank = registries.rework.as_ref().unwrap().blank.clone();
    let price = registries.items.get(&blank).unwrap().price.unwrap();
    let blanks = u32::try_from(report.carbine.reworks.len()).unwrap();
    assert_eq!(saved.purse, draw.purse_after + sales - blanks * price);
    // Carried to the locker and sent across: the store is up by the
    // units and the pack holds what it held before the draw, which is
    // the magazine that came off and the second core.
    assert_eq!(draw.store_after, draw.store_before + offered);
    assert_eq!(
        (draw.pack_before, draw.pack_after),
        (3, 3),
        "the magazine, the second core and the survey tool"
    );
    let from = |record: &&Record| {
        record
            .origin
            .as_ref()
            .is_some_and(|origin| origin.deposit == draw.deposit.0)
    };
    let held: u32 = saved
        .store
        .iter()
        .flatten()
        .filter(from)
        .map(|r| r.count)
        .sum();
    // The smith's carbine was cut from the best metal in reach, which is
    // this deposit's, so the store is down by what its frame took.
    let framed: u32 = saved
        .crafting
        .made
        .get(&6)
        .into_iter()
        .flat_map(|made| &made.from)
        .map(|(_, record)| record)
        .filter(from)
        .map(|record| record.count)
        .sum();
    assert_eq!(held + framed, draw.store_after);
    let lot = saved.store.iter().flatten().find(from).unwrap();
    assert_eq!(lot.origin.as_ref().unwrap().place, draw.place);
    for (key, value) in &figures {
        assert_eq!(lot.lines[key], *value, "{key}");
    }
    // The ping found the deposit and the sampling read it exactly.
    let known = saved
        .found
        .get(&draw.deposit)
        .expect("the deposit is found");
    assert!(known.sampled && known.reading == strata::registry::Reading::Exact);
    assert_eq!(saved.tracked, None);

    // The first day's lots are in the store behind the items' own
    // eight stacks, each from a deposit of its own with figures inside
    // its type's spans, in the amounts the starting file names less
    // what the bench took of them ([`spent`]) and what the fabricator
    // did ([`run_off`]).
    let starting = registries.items.starting().unwrap();
    let lots: Vec<&Record> = saved
        .store
        .iter()
        .flatten()
        .filter(|record| record.origin.is_some())
        .collect();
    for lot in &lots {
        assert_eq!(registries.items.check_record(lot), Ok(()));
    }
    let units = |wanted: &LotOf| -> u32 {
        lots.iter()
            .filter(|lot| {
                let Some(ItemKind::Lot {
                    resource, class, ..
                }) = registries.items.get(&lot.item).map(|def| &def.kind)
                else {
                    return false;
                };
                match wanted {
                    LotOf::Type(ty) => resource == ty,
                    LotOf::Class(of) => class == of,
                }
            })
            .map(|lot| lot.count)
            .sum()
    };
    assert_eq!(starting.lots.len(), 3);
    let mut granted = 0;
    for wanted in &starting.lots {
        let held = units(&wanted.of) + used(saved, &registries, &wanted.of);
        assert!(
            held == wanted.units || held == wanted.units + offered,
            "{}: {held} units in the store and spent",
            wanted.of
        );
        granted += wanted.units - used(saved, &registries, &wanted.of);
    }
    let all: u32 = lots.iter().map(|lot| lot.count).sum();
    assert_eq!(all, granted + offered, "the three lots and the one drawn");
    for cell in &saved.store[..8] {
        assert!(cell.as_ref().is_some_and(|record| record.origin.is_none()));
    }
}

/// What `--script-crafting` leaves behind of the fabricator: the second
/// core recorded as blueprint 4 at the whole of itself, asking for what
/// went into it; three cores run off it from the first lots in the
/// played time the dial gives, in the store as one stack of one batch
/// with the blueprint's lines exactly; the blueprint back in the store
/// with three runs fewer, and the lots down by what three take.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
#[allow(
    clippy::too_many_lines,
    reason = "the blueprint, the run and what both left, in the order they happened"
)]
fn ran(report: &Path, saved: &Household) {
    let registries = shipped();
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let run = &report.factory;
    let arrival = smith(saved);
    let second = arrival.pack.cells[1].as_ref().unwrap();
    assert_eq!((run.blueprint, run.prototype), (Some(4), 3));

    // The blueprint is the second core's lines, exactly, and each slot
    // asks for the figures of the lot that went into it.
    assert_eq!(saved.blueprints.0.keys().copied().collect::<Vec<_>>(), [4]);
    let print = &saved.blueprints.0[&4];
    assert_eq!(print.lines, second.lines);
    assert_eq!(
        (
            print.schematic.to_string().as_str(),
            print.makes.to_string().as_str()
        ),
        (SCRIPT_CORE, "veyra:component.cell_core")
    );
    assert_eq!(
        (
            print.by.as_str(),
            print.derate,
            print.prototype,
            print.complexity
        ),
        ("Tovan Reyes", 1000, 3, 1)
    );
    let from = &saved.crafting.made[&3].from;
    assert_eq!(saved.crafting.made[&3].recorded, Some(4));
    let slots: Vec<(&str, u32)> = print
        .slots
        .iter()
        .map(|need| (need.slot.id.as_str(), need.slot.amount()))
        .collect();
    assert_eq!(slots, [("salt", 20), ("brine", 30)]);
    for (need, (slot, went_in)) in print.slots.iter().zip(from) {
        assert_eq!(&need.slot.id, slot);
        assert!(!need.needs.is_empty());
        for asked in &need.needs {
            assert_eq!(
                (asked.at_least, asked.at_most),
                (Some(went_in.lines[&asked.stat]), None),
                "{slot}: {}",
                asked.stat
            );
        }
    }
    let asked: Vec<&str> = print
        .slots
        .iter()
        .flat_map(|need| need.needs.iter().map(|asked| asked.stat.as_str()))
        .collect();
    assert_eq!(
        asked,
        ["potency", "stability", "potency", "conductivity", "purity"]
    );

    // What the bench drafted before Record: at the whole of the core
    // what was recorded, and at 85 % that much of every line and every
    // minimum, a half rounding up. The first lots meet both, and feed
    // three: a hundred brine is what two cores and a magazine left.
    let deratings: Vec<u16> = run.drafts.iter().map(|draft| draft.derate).collect();
    assert_eq!(deratings, [1000, 850]);
    let (whole, less) = (&run.drafts[0], &run.drafts[1]);
    assert_eq!(whole.lines, print.lines);
    for (line, figure) in &print.lines {
        assert_eq!(less.lines[line], blueprint::derated(*figure, 850), "{line}");
        assert!(less.lines[line] < *figure);
    }
    assert_eq!(whole.needs.len(), 5);
    for (full, derated) in whole.needs.iter().zip(&less.needs) {
        assert_eq!((&full.0, &full.1), (&derated.0, &derated.1));
        assert_eq!(
            derated.2,
            Some(blueprint::derated(full.2.unwrap(), 850)),
            "{} {}",
            full.0,
            full.1
        );
    }
    for draft in &run.drafts {
        let fed: Vec<(&str, u32, bool)> = draft
            .stock
            .iter()
            .map(|(slot, _, units, why)| (slot.as_str(), *units, why.is_none()))
            .collect();
        assert_eq!(fed, [("salt", 120, true), ("brine", 100, true)]);
        assert_eq!(draft.most, 3);
    }

    // The run: three asked for, which is the most the stock fed. Tovan's
    // Fixtures make a unit 10.8 s and the unattended rate 0.65: 33 s with a
    // worker and 50 s with nobody, and with nobody on it 49.9 s of play it
    // was.
    assert_eq!((run.most, run.size, run.made), (3, SCRIPT_BATCH, 3));
    assert_eq!(run.batch, 5);
    assert_eq!(run.promised_secs, (33, 50));
    assert!(
        (49.8..=49.95).contains(&run.played_secs),
        "{} s of play",
        run.played_secs
    );
    assert_eq!(run.runs, (125, 122));
    assert_eq!((print.runs, print.runs_left), (125, 122));

    // The three cores were sold at the board as the script's last act
    // ([`sold`]), so none is in the store, and the batch they carried is in
    // the ledger with the blueprint's lines.
    assert!(
        saved
            .store
            .iter()
            .flatten()
            .all(|record| record.item.to_string() != "veyra:component.cell_core"),
        "the run's cores were sold"
    );
    // The run copied every line of the blueprint onto its cores.
    let sale = report.sale.as_ref().unwrap();
    assert_eq!(sale.grade, print.lines["charge_density"]);
    assert_eq!(sale.lines, print.lines);
    // The ledger says they were run off the blueprint, a unit of the
    // same two lots as the prototype, and the lots are down 60 and 90
    // by it.
    let filed = &saved.crafting.made[&5];
    assert_eq!((filed.blueprint, filed.recorded), (Some(4), None));
    assert_eq!(filed.by, "Tovan Reyes");
    assert_eq!(&filed.from, from);
    let starting = registries.items.starting().unwrap();
    let down: Vec<u32> = starting
        .lots
        .iter()
        .map(|lot| run_off(saved, &registries, &lot.of))
        .collect();
    assert_eq!(down, [60, 90, 0]);

    // The fabricator is free and the blueprint is back in the store:
    // one item, numbered as its body is, carried by nobody.
    assert_eq!(saved.factory, FactoryState::default());
    let is_blueprint = |record: &&Record| {
        matches!(
            registries.items.get(&record.item).map(|def| &def.kind),
            Some(ItemKind::Blueprint)
        )
    };
    let stored: Vec<&Record> = saved.store.iter().flatten().filter(is_blueprint).collect();
    let [item] = stored[..] else {
        panic!("one blueprint in the store: {stored:?}");
    };
    assert_eq!(
        (item.count, item.batch, item.by.as_deref()),
        (1, 4, Some("Tovan Reyes"))
    );
    for character in &saved.characters {
        let carried = character
            .pack
            .cells
            .iter()
            .chain(character.pack.bags.iter().flatten())
            .flatten()
            .filter(is_blueprint)
            .count();
        assert_eq!(carried, 0, "{}", character.name);
    }
}

/// The last act: the run's cores sold on the cell core line of Pelagic
/// Works. The sale's count and pay are the report's, the purse rose by the
/// pay, and the ledger names the run's batch, its maker and the line.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn sold(report: &Path, saved: &Household) {
    let registries = shipped();
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let sale = report.sale.expect("the cores were sold");
    assert_eq!((sale.count, sale.batch), (3, report.factory.batch));
    let buyer = registries
        .buyers
        .get(&strata::registry::Namespaced::parse("veyra:buyer.pelagic_works").unwrap())
        .unwrap();
    let (_, line) = buyer.line("cell_cores").unwrap();
    assert!(sale.grade >= line.min, "the cores meet the minimum");
    let unit = strata::sim::procurement::pays(buyer, line, sale.grade);
    // Pelagic employs the household, so its board pays the welcoming factor.
    let welcome = strata::sim::deeds::Terms::of(
        strata::sim::deeds::Stance::Welcoming,
        registries.factions.deeds().unwrap(),
    );
    assert_eq!(
        sale.paid,
        strata::sim::procurement::pay_for(
            strata::sim::deeds::Terms::scale(unit, welcome.pays_pm),
            3
        )
    );
    assert_eq!(sale.purse.1, sale.purse.0 + sale.paid);
    let [entry] = &saved.procurement.sales[..] else {
        panic!("one sale in the ledger: {:?}", saved.procurement.sales);
    };
    assert_eq!(
        (
            entry.buyer.to_string().as_str(),
            entry.line.as_str(),
            entry.item.to_string().as_str(),
            entry.batch,
            entry.by.as_deref(),
            entry.count,
            entry.paid
        ),
        (
            "veyra:buyer.pelagic_works",
            "cell_cores",
            "veyra:component.cell_core",
            report.factory.batch,
            Some("Tovan Reyes"),
            3,
            sale.paid
        )
    );
    assert_eq!(saved.procurement.taken.values().copied().sum::<u32>(), 3);
}

/// What `--script-crafting` ends with: the smith's carbine, made from the
/// first day's soft alloy with every point on moddability, on the
/// arrival's hand, and the arrival's bought one lying in the store.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn carbine(report: &Path, saved: &Household) {
    let registries = shipped();
    let items = &registries.items;
    let report: CraftingReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let made = &report.carbine;
    assert_eq!((made.batch, made.by.as_deref()), (6, Some("Tovan Reyes")));
    // Eight points for the complexity and one for the smith's tuning box.
    assert_eq!(made.points, 9);
    assert_eq!(made.in_hand_of, "Frontier ranger");
    let arrival = &saved.characters[0];
    let held = arrival.hands[0].as_ref().unwrap();
    assert_eq!(held.item.to_string(), "veyra:weapon.patrol_carbine");
    assert_eq!((held.batch, held.by.as_deref()), (6, Some("Tovan Reyes")));
    assert_eq!(held.condition, Some(1000), "made whole");
    assert!(held.fitted.is_empty());
    // The first day's alloy, drawn from the nearest deposit on the finer
    // lattice (Mardle light alloy, 862 m off), is not as soft as the one the
    // old lattice gave: its Hardness and Strength put the moddability
    // ceiling at 420, over the 400 that opens the stock mount. The smith
    // spends every point, the stock is open as made, and the path buys no
    // blank and reworks nothing (the rework itself is held by the sim's
    // tests).
    assert!(made.reworks.is_empty(), "{:?}", made.reworks);
    assert_eq!(
        held.lines.clone().into_iter().collect::<Vec<_>>(),
        made.lines.clone().into_iter().collect::<Vec<_>>()
    );
    assert_eq!(made.ceiling, 420);
    assert_eq!(made.lines["moddability"], made.ceiling, "every point spent");
    assert_eq!(made.open, (4, 6));
    assert_eq!(made.bought_open, 4);
    assert_eq!(items.mount_opens(held, "stock"), Some(Ok(())));
    assert_eq!(open_count(items, held), 4);
    // The credits, in the order they were earned: the core, the magazine,
    // the second core (one line beaten), the carbine, then 60 for each
    // rework that raised the line.
    let rates = &registries.progression.rates.craft;
    let xp = |schematic: &str, first: bool, beaten: u32| {
        let found = registries
            .schematics
            .get(&strata::registry::Namespaced::parse(schematic).unwrap())
            .unwrap();
        strata::sim::crafting::craft_xp(rates, found.def.complexity, first, beaten)
    };
    let second_beats = u32::try_from(
        report
            .made
            .second
            .iter()
            .filter(|(line, figure)| **figure > report.made.core[*line])
            .count(),
    )
    .unwrap();
    let lifts = made
        .reworks
        .iter()
        .filter(|(_, from, to)| to > from)
        .count();
    let mut expected = vec![
        xp("veyra:schematic.cell_core", true, 0),
        xp("veyra:schematic.patrol_extended_feed", true, 0),
        xp("veyra:schematic.cell_core", false, second_beats),
        xp(SCRIPT_CARBINE, true, 0),
    ];
    expected.extend(std::iter::repeat_n(rates.rework, lifts));
    assert_eq!(made.credits, expected);
    let tovan = smith(saved);
    assert_eq!(
        tovan.sheet.xp(XpKind::Fabrication),
        900 + expected.iter().sum::<u32>(),
        "the smith's bench work is in the saved file"
    );
    // The made one is the smith's by the ledger, and the smith's own hand
    // still holds the bought one with the made magazine on it.
    assert_eq!(saved.crafting.made[&6].by, "Tovan Reyes");
    let tovan = smith(saved).hands[0].as_ref().unwrap();
    assert_eq!((tovan.batch, tovan.by.as_deref()), (0, None));
    // The arrival's bought carbine lies in the store where the made one
    // came out of.
    let bought = saved
        .store
        .iter()
        .flatten()
        .find(|record| record.item == held.item)
        .unwrap();
    // It is worn: the arrival fired it in the fight.
    assert_eq!(bought.batch, 0);
    assert!(bought.condition.is_some_and(|condition| condition < 1000));
}

/// How many mounts of `weapon` are open.
fn open_count(items: &ItemCatalogue, weapon: &Record) -> u32 {
    let Some(ItemKind::Weapon { mounts, .. }) = items.get(&weapon.item).map(|def| &def.kind) else {
        return 0;
    };
    let open = mounts
        .iter()
        .filter(|mount| items.mount_opens(weapon, &mount.slot) == Some(Ok(())))
        .count();
    u32::try_from(open).expect("a few mounts")
}

/// The headless slice, run to its end in process: the scripted fight
/// earns XP, the trainer teaches Cuts 1, the store's magazine is fitted
/// and taken off at the workbench ([`crafted`]), a cell core, the
/// magazine of it and a second core are made there ([`made`]), the
/// second core is recorded as a blueprint and three run off it at the
/// fabricator ([`ran`]), a lot is sampled at the middle of a deposit and carried to
/// the locker's store ([`drawn`]), the
/// recruits join at the terminal and the pistol marksman comes out; the
/// file says so and reads back as what was written. One run, so the
/// world streams once.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_scripted_path_earns_learns_swaps_and_saves() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let report = dir.path().join("crafting.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--script-progression",
        "--script-crafting",
        "--script-report",
        report.to_str().unwrap(),
        "--fresh",
        "--household",
        path.to_str().unwrap(),
    ]);
    cli.resolve_play_mode();
    assert!(cli.combat && cli.combat_script);
    crate::install_combat_tables(&cli).unwrap();
    let exit = crate::build(cli, None).unwrap().run();
    assert_eq!(exit, AppExit::Success);

    let text = std::fs::read_to_string(&path).unwrap();
    let saved: Household = ron::from_str(&text).unwrap();
    let graph = SkillGraph::shipped();
    let authored = AuthoredSheets::shipped();
    let start = &authored.household.unwrap().arrival.sheet;
    let arrival = &saved.characters[0];
    assert_eq!(arrival.name, "Frontier ranger");
    assert!(
        arrival.sheet.xp(XpKind::Blade) > start.xp.get(&XpKind::Blade).copied().unwrap_or(0),
        "the fight earned blade XP: {:?}",
        arrival.sheet.xp
    );
    assert!(
        arrival.sheet.xp(XpKind::Ranged) > start.xp.get(&XpKind::Ranged).copied().unwrap_or(0),
        "the fight earned ranged XP: {:?}",
        arrival.sheet.xp
    );
    assert!(
        arrival.sheet.has(SCRIPT_BOX),
        "the trainer taught {SCRIPT_BOX}: {:?}",
        arrival.sheet.learned
    );
    assert_eq!(
        arrival.sheet.points_spent(&graph),
        16,
        "the arrival's 14 points and the 2 of Cuts 1"
    );
    assert_eq!(saved.characters.len(), BENCH, "everyone joined");
    assert!(saved.recruits.is_empty());
    let pistol = saved.index_with("marksman.pistol.1").unwrap();
    assert_ne!(pistol, 0);
    assert!(
        saved.characters[pistol].left_at.is_some(),
        "the pistol marksman was out, and was left where the smith came back"
    );
    assert_eq!(
        saved.active, 0,
        "the arrival is out again, with the carbine"
    );
    assert!(
        smith(&saved).left_at.is_some(),
        "the smith was left where the arrival took over"
    );
    let again: Household = ron::from_str(
        &ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(again, saved, "the file round-trips");

    crafted(&report, &saved);
    made(&report, &saved);
    ran(&report, &saved);
    sold(&report, &saved);
    drawn(&report, &saved);
    carbine(&report, &saved);
}

static RESYNCS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
fn count(mut heard: MessageReader<strata::sim::items::StockAnswer>) {
    for answer in heard.read() {
        if matches!(answer, strata::sim::items::StockAnswer::Resync { .. }) {
            RESYNCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// `--script-crafting` with a bike in the stable: the saddle's cells come
/// before the store's in the stock's menu, and the script predicts its
/// clicks on that layout (the carbine sent from the pack to the store, the
/// lot stowed) as the sim answers them, so the same report holds as without a
/// bike and the bike is still in the stable after it.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_scripted_crafting_path_clicks_true_with_a_bike_in_the_stable() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let report = dir.path().join("crafting.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--script-progression",
        "--script-crafting",
        "--bike",
        "--script-report",
        report.to_str().unwrap(),
        "--fresh",
        "--household",
        path.to_str().unwrap(),
    ]);
    cli.resolve_play_mode();
    crate::install_combat_tables(&cli).unwrap();
    // A click the sim did not do as the script predicted is answered with a
    // resync, which is counted.
    let mut app = crate::build(cli, None).unwrap();
    app.add_systems(Update, count);
    let exit = app.run();
    assert_eq!(exit, AppExit::Success);
    // One resync belongs to the script with or without a bike; the saddle
    // predicted away, the two clicks added two more.
    assert_eq!(
        RESYNCS.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "a bike must add no resync to the run's own one"
    );
    let saved: Household = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved.stable.bike.is_some(), "the bike is stabled still");
    drawn(&report, &saved);
    carbine(&report, &saved);
}

/// `--script-ride`, headless: the bike is called at the far end of Port
/// Serein's road and ridden in along it for forty played seconds. The ride holds
/// close to road speed, is never thrown and never stalls for a second, on
/// the real world streaming round it. It runs on the 60 Hz wall clock so the
/// streamer keeps up; on a loaded machine a stall may be the clock's and not
/// the ride's.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_scripted_ride_covers_the_road_at_close_to_road_speed() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let report = dir.path().join("ride.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--bike",
        "--script-ride",
        "--no-hostiles",
        "--at",
        "road",
        "--script-report",
        report.to_str().unwrap(),
        "--fresh",
        "--household",
        path.to_str().unwrap(),
    ]);
    cli.resolve_play_mode();
    crate::install_combat_tables(&cli).unwrap();
    let exit = crate::build(cli, None).unwrap().run();
    assert_eq!(exit, AppExit::Success);

    let measured: crate::ride::RideReport =
        ron::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    // The road speed is the vehicle file's, so the test follows a retune.
    let road = ron::from_str::<strata::registry::vehicle::VehicleDef>(include_str!(
        "../../../../../assets/vehicles/courier_bike.vehicle.ron"
    ))
    .unwrap()
    .speed
    .road;
    let road = f64::from(road);
    assert!(
        measured.top_mps >= 0.95 * road,
        "top {:.1} m/s against road speed {road}: {measured:?}",
        measured.top_mps
    );
    assert!(measured.mounted, "{measured:?}");
    assert_eq!(measured.crashes, 0, "{measured:?}");
    assert_eq!(measured.thrown, 0, "{measured:?}");
    assert!(
        measured.seconds >= 39.0 || measured.reached_end,
        "the ride ended early: {measured:?}"
    );
    assert!(
        measured.mean_mps >= 0.6 * road,
        "mean {:.1} m/s against road speed {road}: {measured:?}",
        measured.mean_mps
    );
    assert!(measured.longest_stall_s <= 1.0, "{measured:?}");
    let saved: Household = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        saved.stable.bike.is_some(),
        "the bike is in the household's file"
    );
}

/// Runs `--script-survey` headless as `member` (the arrival when `None`) on
/// a fresh household in `dir`, and returns what it reported and where the
/// household was saved.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn run_survey(
    dir: &tempfile::TempDir,
    member: Option<&str>,
) -> (crate::survey_script::SurveyReport, std::path::PathBuf) {
    let path = dir.path().join("household.ron");
    let report = dir.path().join("survey.ron");
    let mut args = vec![
        "game",
        "--headless",
        "--script-survey",
        "--script-report",
        report.to_str().unwrap(),
        "--fresh",
        "--household",
        path.to_str().unwrap(),
    ];
    if let Some(member) = member {
        args.extend(["--as", member]);
    }
    let mut cli = crate::Cli::parse_from(args);
    cli.resolve_play_mode();
    assert!(cli.combat && cli.no_hostiles);
    crate::install_combat_tables(&cli).unwrap();
    let exit = crate::build(cli, None).unwrap().run();
    assert_eq!(exit, AppExit::Success);
    let measured = ron::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    (measured, path)
}

/// `--script-survey`, headless: a member starts on dry ground 400 m from the
/// nearest conductor deposit, looking its way, and finds it by pings alone
/// without knowing where it is, homes in on the strongest point of each,
/// and samples fifteen units. The deposit is found and sampled, the
/// sampling was at 0.6 of its peak or better, the pack holds the units, and
/// it all took under 240 played seconds. The run is the manual clock's, so
/// the seconds are the sim's and not the wall's.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_scripted_survey_finds_a_deposit_by_pings_and_samples_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let (measured, path) = run_survey(&dir, None);
    eprintln!("{measured:?}");
    assert!(measured.ok, "{measured:?}");
    assert!(
        measured.survey_xp > 0,
        "surveying earns Survey XP: {measured:?}"
    );
    let deposit = crate::survey_script::deposit_of(&measured).expect("a deposit was met");
    assert!(measured.units >= 15, "{measured:?}");
    assert!(measured.seconds < 240.0, "{measured:?}");
    assert!(measured.pings >= 2, "a search and a homing: {measured:?}");
    assert!(
        f64::from(measured.sampled_pm) >= 0.6 * f64::from(measured.peak_pm),
        "sampled at {} of a peak of {}",
        measured.sampled_pm,
        measured.peak_pm
    );

    // The household has found it and sampled it, and carries the lot.
    let saved: Household = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let known = saved.found.get(&deposit).expect("the deposit is found");
    assert!(known.sampled && known.reading == strata::registry::Reading::Exact);
    let carried: u32 = saved.characters[0]
        .pack
        .cells
        .iter()
        .flatten()
        .filter(|record| {
            record
                .origin
                .as_ref()
                .is_some_and(|origin| origin.deposit == deposit.0)
        })
        .map(|record| record.count)
        .sum();
    assert!(carried >= 15, "{carried} units in the pack");
}

/// The same script as Ilse Marrow, the household's surveyor, reads more
/// ground in a ping and takes more units a cycle than the arrival, and earns
/// Survey XP too.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_surveyor_reads_more_ground_and_takes_more_than_the_arrival() {
    let dir = tempfile::TempDir::new().unwrap();
    let (arrival, _) = run_survey(&dir, None);
    let dir = tempfile::TempDir::new().unwrap();
    let (ilse, _) = run_survey(&dir, Some("Ilse Marrow"));
    eprintln!("arrival {arrival:?}\nilse {ilse:?}");
    assert!(ilse.ok && ilse.survey_xp > 0, "{ilse:?}");
    assert!(
        ilse.reach_m > arrival.reach_m,
        "{ilse:?} against {arrival:?}"
    );
    assert!(
        ilse.units_cycle > arrival.units_cycle,
        "{ilse:?} against {arrival:?}"
    );
}
