//! Tests of where the props stand: the locker, the workbench and the
//! fabricator clear of the script's stops.

use super::*;

/// The scripted path stops at the terminal and at the blade trainer and
/// waits until that very prop is the one within reach. The sim offers
/// only the nearest, so the locker must never be it there.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_locker_stands_clear_of_the_scripts_stops() {
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::progression::find_reach;
    // Both bench places are `bench_place` at this spacing.
    let staging = sites(Some(((100., -40.), 0.6)), None).0;
    assert_eq!(staging.len(), 9);
    for (x, z, yaw) in [(0., 0., 0.), (100., -40., 0.6), (-7.5, 3., 2.4)] {
        let place = bench_place(x, z, yaw, 3.5);
        let lockers: Vec<&PropSite> = place
            .iter()
            .filter(|site| site.kind == InteractionKind::Locker)
            .collect();
        let [locker] = lockers[..] else {
            panic!("one locker a bench place");
        };
        assert_eq!(locker.name, "Locker");
        let mut world = World::new();
        world.init_resource::<Reach>();
        for site in &place {
            world.spawn((
                WorldPosition::new(site.x, 0., site.z),
                Interaction {
                    kind: site.kind.clone(),
                    radius: REACH_M,
                },
            ));
        }
        let player = world
            .spawn((LocalPlayer, WorldPosition::new(x, 0., z)))
            .id();
        let mut reach_at = |px: f64, pz: f64| {
            world.get_mut::<WorldPosition>(player).unwrap().0 = WorldPos::new(px, 0., pz);
            world.run_system_once(find_reach).unwrap();
            world
                .resource::<Reach>()
                .nearest
                .clone()
                .map(|(_, kind)| kind)
        };
        let is_blade = |kind: &InteractionKind| matches!(kind, InteractionKind::Trainer { trees } if trees.iter().any(|t| t == "blade"));
        let stops = place
            .iter()
            .filter(|site| site.kind == InteractionKind::Terminal || is_blade(&site.kind));
        let mut stood = 0;
        for stop in stops {
            let apart = (locker.x - stop.x).hypot(locker.z - stop.z);
            assert!(
                apart > 2. * f64::from(REACH_M),
                "{}: the locker is {apart:.1} m off, within two reaches",
                stop.name
            );
            // Everywhere the stop is within reach, on a quarter metre
            // grid: the stop or a nearer neighbour, never the locker.
            for i in -10..=10 {
                for j in -10..=10 {
                    let (dx, dz) = (f64::from(i) * 0.25, f64::from(j) * 0.25);
                    if dx.hypot(dz) > f64::from(REACH_M) {
                        continue;
                    }
                    let offered = reach_at(stop.x + dx, stop.z + dz);
                    assert_ne!(offered, Some(InteractionKind::Locker), "{}", stop.name);
                    assert!(offered.is_some());
                    stood += 1;
                }
            }
            // And where the script stops walking, it is the stop itself.
            assert_eq!(reach_at(stop.x, stop.z), Some(stop.kind.clone()));
        }
        assert!(stood > 500, "both stops were stood at");
        // The locker is a place of its own all the same.
        assert_eq!(reach_at(locker.x, locker.z), Some(InteractionKind::Locker));
    }
}

/// The workbench stands one spacing past the locker: its own prompt
/// where it stands, out of reach at both of the script's stops, and
/// within reach under the locker's prompt between the two, where a
/// part comes out of the store and onto the weapon.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_workbench_stands_beside_the_locker_and_clear_of_the_stops() {
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::progression::find_reach;
    for (x, z, yaw) in [(0., 0., 0.), (100., -40., 0.6), (-7.5, 3., 2.4)] {
        let place = bench_place(x, z, yaw, 3.5);
        let only = |kind: InteractionKind| -> &PropSite {
            let found: Vec<&PropSite> = place.iter().filter(|site| site.kind == kind).collect();
            let [site] = found[..] else {
                panic!("one {kind:?} a bench place");
            };
            site
        };
        let (locker, bench) = (
            only(InteractionKind::Locker),
            only(InteractionKind::Workbench),
        );
        assert_eq!(bench.name, "Workbench");
        let mut world = World::new();
        world.init_resource::<Reach>();
        for site in &place {
            world.spawn((
                WorldPosition::new(site.x, 0., site.z),
                Interaction {
                    kind: site.kind.clone(),
                    radius: REACH_M,
                },
            ));
        }
        let player = world
            .spawn((LocalPlayer, WorldPosition::new(x, 0., z)))
            .id();
        let mut reach_at = |px: f64, pz: f64| {
            world.get_mut::<WorldPosition>(player).unwrap().0 = WorldPos::new(px, 0., pz);
            world.run_system_once(find_reach).unwrap();
            world.resource::<Reach>().clone()
        };
        let at_bench = reach_at(bench.x, bench.z);
        assert_eq!(
            at_bench.nearest.map(|(_, kind)| kind),
            Some(InteractionKind::Workbench)
        );
        assert!(at_bench.workbench);
        let is_blade = |kind: &InteractionKind| matches!(kind, InteractionKind::Trainer { trees } if trees.iter().any(|t| t == "blade"));
        let stops = place
            .iter()
            .filter(|site| site.kind == InteractionKind::Terminal || is_blade(&site.kind));
        for stop in stops {
            assert!(!reach_at(stop.x, stop.z).workbench, "{}", stop.name);
        }
        // A step nearer the locker than the bench.
        let between = |a: f64, b: f64| a * 0.6 + b * 0.4;
        let midway = reach_at(between(locker.x, bench.x), between(locker.z, bench.z));
        assert_eq!(
            midway.nearest.map(|(_, kind)| kind),
            Some(InteractionKind::Locker)
        );
        assert!(midway.workbench, "both are within reach between them");
        assert!(
            !reach_at(locker.x, locker.z).workbench,
            "at the locker itself the bench is a step further than a reach"
        );
    }
}

/// The fabricator stands one spacing past the workbench, with only the
/// works foreman beyond it: its own prompt where it stands, out of reach
/// at the script's stops, at the locker and at the workbench, and never
/// the prop offered where the script stands to use the workbench. The
/// foreman stands one spacing past it, teaches Fabricator and
/// Weaponsmith, and is the nearest prop at his own post.
#[test]
#[allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    reason = "the scenario owns its world, and walks the whole row of props"
)]
fn the_fabricator_stands_past_the_workbench_and_clear_of_the_stops() {
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::progression::find_reach;
    for (x, z, yaw) in [(0., 0., 0.), (100., -40., 0.6), (-7.5, 3., 2.4)] {
        let place = bench_place(x, z, yaw, 3.5);
        let only = |kind: InteractionKind| -> &PropSite {
            let found: Vec<&PropSite> = place.iter().filter(|site| site.kind == kind).collect();
            let [site] = found[..] else {
                panic!("one {kind:?} a bench place");
            };
            site
        };
        let (terminal, locker, bench, fabricator) = (
            only(InteractionKind::Terminal),
            only(InteractionKind::Locker),
            only(InteractionKind::Workbench),
            only(InteractionKind::Fabricator),
        );
        assert_eq!(fabricator.name, "Fabricator");
        let apart = |a: &PropSite, b: &PropSite| (a.x - b.x).hypot(a.z - b.z);
        assert!((apart(fabricator, bench) - 3.5).abs() < 1e-6);
        assert!((apart(fabricator, locker) - 7.).abs() < 1e-6);
        assert!((apart(fabricator, terminal) - 14.).abs() < 1e-6);
        for site in place.iter().filter(|site| site.name != "Works foreman") {
            assert!(
                apart(site, terminal) <= apart(fabricator, terminal),
                "{} is past the fabricator",
                site.name
            );
        }
        let works: Vec<&PropSite> = place
            .iter()
            .filter(|site| site.name == "Works foreman")
            .collect();
        let [foreman] = works[..] else {
            panic!("one works foreman a bench place");
        };
        assert_eq!(
            foreman.kind,
            InteractionKind::Trainer {
                trees: vec!["fabricator".to_owned(), "weaponsmith".to_owned()]
            }
        );
        assert!((apart(foreman, fabricator) - 3.5).abs() < 1e-6);
        assert!((apart(foreman, terminal) - 17.5).abs() < 1e-6);
        let mut world = World::new();
        world.init_resource::<Reach>();
        for site in &place {
            world.spawn((
                WorldPosition::new(site.x, 0., site.z),
                Interaction {
                    kind: site.kind.clone(),
                    radius: REACH_M,
                },
            ));
        }
        let player = world
            .spawn((LocalPlayer, WorldPosition::new(x, 0., z)))
            .id();
        let mut reach_at = |px: f64, pz: f64| {
            world.get_mut::<WorldPosition>(player).unwrap().0 = WorldPos::new(px, 0., pz);
            world.run_system_once(find_reach).unwrap();
            world.resource::<Reach>().clone()
        };
        let at = reach_at(fabricator.x, fabricator.z);
        assert_eq!(
            at.nearest.map(|(_, kind)| kind),
            Some(InteractionKind::Fabricator)
        );
        assert!(at.fabricator && !at.workbench);
        for stop in place
            .iter()
            .filter(|site| site.kind != InteractionKind::Fabricator)
        {
            let there = reach_at(stop.x, stop.z);
            assert!(!there.fabricator, "{}", stop.name);
            assert_eq!(there.nearest.map(|(_, kind)| kind), Some(stop.kind.clone()));
        }
        // Where the script stops walking to the workbench, within a
        // metre and a half of it, the workbench is what is offered.
        let mut stood = 0;
        for i in -6..=6 {
            for j in -6..=6 {
                let (dx, dz) = (f64::from(i) * 0.25, f64::from(j) * 0.25);
                if dx.hypot(dz) > f64::from(REACH_M) - 1. {
                    continue;
                }
                let offered = reach_at(bench.x + dx, bench.z + dz);
                assert_eq!(
                    offered.nearest.map(|(_, kind)| kind),
                    Some(InteractionKind::Workbench)
                );
                assert!(offered.workbench);
                stood += 1;
            }
        }
        assert!(stood > 100);
        // Midway between the two both are within reach, as the locker
        // and the workbench are: a thing recorded at one is run at the
        // other without a walk.
        let between = |a: f64, b: f64| a * 0.5 + b * 0.5;
        let midway = reach_at(
            between(bench.x, fabricator.x),
            between(bench.z, fabricator.z),
        );
        assert!(midway.workbench && midway.fabricator);
    }
}

/// A survey trainer stands three spacings out, past the board, and teaches
/// the Surveyor tree and no other: its post is the nearest prop where it
/// stands, a sheet learns Surveyor there and not at the blade trainer, and
/// the board stays the nearest at its own post.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_survey_trainer_stands_past_the_board_and_teaches_surveyor_and_ranger() {
    use bevy::ecs::system::RunSystemOnce;
    use strata::sim::progression::{CharacterSheet, find_reach};
    let graph = strata::registry::SkillGraph::shipped();
    for (x, z, yaw) in [(0., 0., 0.), (100., -40., 0.6), (-7.5, 3., 2.4)] {
        let place = bench_place(x, z, yaw, 3.5);
        let trainer = place
            .iter()
            .find(|site| site.name == "Survey trainer")
            .expect("one survey trainer a bench place");
        assert_eq!(
            trainer.kind,
            InteractionKind::Trainer {
                trees: vec!["surveyor".to_owned(), "ranger".to_owned()]
            }
        );
        let board = place
            .iter()
            .find(|site| matches!(site.kind, InteractionKind::Board { .. }))
            .unwrap();
        let blade = place
            .iter()
            .find(|site| site.name == "Blade trainer")
            .unwrap();
        let apart = |a: &PropSite, b: &PropSite| (a.x - b.x).hypot(a.z - b.z);
        assert!((apart(trainer, board) - 3.5).abs() < 1e-6);
        assert!((apart(trainer, blade) - 7.).abs() < 1e-6);
        let mut world = World::new();
        world.init_resource::<Reach>();
        for site in &place {
            world.spawn((
                WorldPosition::new(site.x, 0., site.z),
                Interaction {
                    kind: site.kind.clone(),
                    radius: REACH_M,
                },
            ));
        }
        let player = world
            .spawn((LocalPlayer, WorldPosition::new(x, 0., z)))
            .id();
        let mut reach_at = |px: f64, pz: f64| {
            world.get_mut::<WorldPosition>(player).unwrap().0 = WorldPos::new(px, 0., pz);
            world.run_system_once(find_reach).unwrap();
            world.resource::<Reach>().clone()
        };
        let sheet = CharacterSheet::default();
        let there = reach_at(trainer.x, trainer.z);
        assert_eq!(
            there.nearest.clone().map(|(_, kind)| kind),
            Some(trainer.kind.clone())
        );
        assert!(there.teaches(&graph, "surveyor.novice"));
        assert!(
            sheet
                .can_learn(
                    &graph,
                    "surveyor.novice",
                    there.teaches(&graph, "surveyor.novice")
                )
                .is_ok()
        );
        assert!(there.teaches(&graph, "ranger.long_shots.1"));
        assert!(!there.teaches(&graph, "blade.novice"));
        assert!(!there.teaches(&graph, "marksman.novice"));
        // Nowhere else in the row.
        for site in place.iter().filter(|site| site.name != "Survey trainer") {
            let elsewhere = reach_at(site.x, site.z);
            assert!(
                !elsewhere.teaches(&graph, "surveyor.novice")
                    && !elsewhere.teaches(&graph, "ranger.novice"),
                "{} teaches Surveyor or Ranger",
                site.name
            );
        }
        let at_board = reach_at(board.x, board.z);
        assert_eq!(
            at_board.nearest.map(|(_, kind)| kind),
            Some(board.kind.clone())
        );
    }
}
