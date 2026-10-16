//! The first questline (ticket 0014), walked to each of its three endings by
//! `--script-quest`, with each of Pell's three branches met once across them.

use super::*;
use crate::household::script_quest::QuestReport;

/// Run the script for `ending` and `pell` on a fresh household in `dir`, and
/// give back what it wrote and what was saved.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn walk(dir: &Path, ending: &str, pell: &str) -> (QuestReport, Household) {
    let path = dir.join(format!("{ending}.ron"));
    let report = dir.join(format!("{ending}.report.ron"));
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--script-quest",
        ending,
        "--script-pell",
        pell,
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
    let made: QuestReport = ron::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    let saved: Household = ron::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    (made, saved)
}

/// The courier bike was bought at the board and is the household's ride.
fn bought_and_stabled(saved: &Household) {
    let bike = saved.stable.bike.as_ref().expect("a stabled bike");
    assert_eq!(bike.item.to_string(), "veyra:vehicle.courier_bike");
    assert!(saved.quests.flag("newcomer_credit"));
}

/// The deeds' score by role at the end.
fn score(report: &QuestReport, role: &str) -> i64 {
    report
        .deeds
        .iter()
        .find(|(name, _)| name == role)
        .map_or_else(
            || panic!("no score for {role}: {:?}", report.deeds),
            |(_, score)| *score,
        )
}

/// Each ending is reached, the log says so and is saved, the deeds lean the
/// way the ending's replies said, the bike was bought at the board and
/// stabled, and the reward is in the household. Pell is talked round in the
/// keepers' run, fought in the assembly's and paid in the company's; Wim's
/// pump is shut, left, and he is fought.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn the_questline_is_walked_to_each_ending() {
    let dir = tempfile::TempDir::new().unwrap();

    let (keepers, saved_keepers) = walk(dir.path(), "keepers", "talk");
    assert!(keepers.ok, "{keepers:?}");
    assert_eq!(keepers.reached.as_deref(), Some("keepers"));
    let log = &saved_keepers.quests;
    assert!(log.is_done("arrival"));
    assert_eq!(log.ending.as_deref(), Some("keepers"));
    assert!(log.flag("ending_keepers") && log.flag("array") && log.flag("pump_shut"));
    assert!(log.flag("station_restored") && log.flag("core"));
    bought_and_stabled(&saved_keepers);
    // Teague's tool reads further than the store's, and the record went
    // with nothing sold for it.
    let carried = saved_keepers
        .characters
        .iter()
        .flat_map(|member| member.pack.cells.iter())
        .chain(saved_keepers.store.iter())
        .flatten();
    assert!(
        carried.into_iter().any(|record| {
            record.item.to_string() == "veyra:tool.survey"
                && record.lines.get("range") == Some(&700)
        }),
        "Teague's tool, read at 700, is in the household"
    );
    assert!(
        saved_keepers.procurement.sales.is_empty(),
        "nothing sold on the keepers' road"
    );
    // The keepers gain, the company that took the array and the borough whose
    // grower's pump was shut lose.
    assert!(score(&keepers, "keepers") > 0, "{:?}", keepers.deeds);
    assert!(score(&keepers, "holder") < 0, "{:?}", keepers.deeds);
    assert!(score(&keepers, "assembly") < 0, "{:?}", keepers.deeds);

    let (assembly, saved_assembly) = walk(dir.path(), "assembly", "fight");
    assert!(assembly.ok, "{assembly:?}");
    let log = &saved_assembly.quests;
    assert_eq!(log.ending.as_deref(), Some("assembly"));
    assert!(log.flag("ending_assembly") && log.flag("pell_fight") && log.flag("pump_left"));
    assert!(
        log.hostile.contains("pell"),
        "the crossing is kept in the log"
    );
    bought_and_stabled(&saved_assembly);
    assert!(!log.flag("array") && !log.flag("pump_shut"));
    assert!(score(&assembly, "assembly") > 0, "{:?}", assembly.deeds);
    assert!(score(&assembly, "keepers") < 0, "{:?}", assembly.deeds);
    let text = ron::ser::to_string(&saved_assembly).unwrap();
    assert!(
        text.contains("veyra:station.field_harvester"),
        "the borough's harvester is in the household"
    );

    let (holder, saved_holder) = walk(dir.path(), "holder", "pay");
    assert!(holder.ok, "{holder:?}");
    let log = &saved_holder.quests;
    assert_eq!(log.ending.as_deref(), Some("holder"));
    assert!(log.flag("ending_holder") && log.flag("core") && log.flag("wim_threatened"));
    assert!(log.flag("wim_fallen") && log.hostile.contains("wim_larrow"));
    assert!(!log.flag("pell_fight") && !log.flag("array"));
    bought_and_stabled(&saved_holder);
    assert!(score(&holder, "holder") > 0, "{:?}", holder.deeds);
    assert!(score(&holder, "keepers") < 0, "{:?}", holder.deeds);
    assert!(score(&holder, "assembly") < 0, "{:?}", holder.deeds);
    // The company paid at the board, once, for the station's record.
    let sold: Vec<_> = saved_holder
        .procurement
        .sales
        .iter()
        .filter(|sale| sale.item.to_string() == "veyra:item.station_record")
        .collect();
    assert_eq!(sold.len(), 1, "{:?}", saved_holder.procurement.sales);
    assert_eq!(sold[0].count, 1);
    assert!(sold[0].paid > 0);

    // Each ending's log is kept, and the three differ only in how it ended.
    for saved in [&saved_keepers, &saved_assembly, &saved_holder] {
        let again: Household = ron::from_str(
            &ron::ser::to_string_pretty(saved, ron::ser::PrettyConfig::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(&again.quests, &saved.quests, "the log round-trips");
    }
}

/// The terminal's two buttons raise the questline's events from the game's
/// own answer (ticket 0014): a join is `Recruited` with the name joined, a
/// switch `Switched` with the name now out.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the shipped data")]
fn the_terminal_raises_recruited_and_switched() {
    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use strata::client::ui::bench::{BenchOutcome, BenchRequest};
    use strata::sim::combat::CombatIntent;
    use strata::sim::quests::QuestEvent;
    let registries = shipped();
    let household = Household::starting(&registries, &Grants::default());
    let recruit = household.recruits.first().unwrap().name.clone();
    let (actor, sheet, name, tint, pack, hands) = household.stats(&registries);
    let mut world = World::new();
    world.init_resource::<Messages<BenchRequest>>();
    world.init_resource::<Messages<BenchOutcome>>();
    world.init_resource::<Messages<QuestEvent>>();
    let terminal = world.spawn_empty().id();
    world.insert_resource(Reach {
        nearest: Some((terminal, InteractionKind::Terminal)),
        ..Reach::default()
    });
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.spawn((
        LocalPlayer,
        WorldPosition::new(0., 0., 0.),
        actor,
        sheet,
        CombatIntent::default(),
        name,
        tint,
        pack,
        hands,
        household.bar(),
    ));
    world.insert_resource(household);
    let ask = |world: &mut World, request: BenchRequest| -> Vec<QuestEvent> {
        world
            .resource_mut::<Messages<BenchRequest>>()
            .write(request);
        world
            .run_system_once(super::super::bench::answer_bench)
            .unwrap();
        world.resource_mut::<Messages<BenchRequest>>().clear();
        world
            .resource_mut::<Messages<QuestEvent>>()
            .drain()
            .collect()
    };
    assert_eq!(
        ask(&mut world, BenchRequest::Join(0)),
        [QuestEvent::Recruited(recruit.clone())]
    );
    let index = world
        .resource::<Household>()
        .characters
        .iter()
        .position(|member| member.name == recruit)
        .unwrap();
    assert_eq!(
        ask(&mut world, BenchRequest::Switch(index)),
        [QuestEvent::Switched(recruit)]
    );
}

/// The arrival on seed 42 (ticket 0014): the household and Maren on the
/// quay's dry paving, clear of every building's collision proxies and every
/// planted tree, the household looking out to sea and Maren ahead of it and
/// to its right, facing it, so the camera behind the household sees her face
/// past its shoulder.
#[test]
#[allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    reason = "a scenario over the real world, read in one place"
)]
fn the_arrival_is_open_quay_paving() {
    use strata::worldgen::{ProceduralWorld, SettlementLayout, WorldSource};
    let registries = shipped();
    let rules = registries.quests.rules().unwrap().clone();
    let world = ProceduralWorld::with_params(
        strata::core::Seed(42),
        &registries,
        strata::sim::world_params(),
    )
    .with_own_water();
    world.hydrology().set_threads(4);
    let site = world.sites()[0].clone();
    world.prepare_water(site.centre[0], site.centre[1], 8_000.0);
    let path = format!(
        "{}/../../generated/{}",
        env!("CARGO_MANIFEST_DIR"),
        registries.settlements[0].layout
    );
    let layout: SettlementLayout = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(layout.ground, site.fingerprint(), "run `just content`");
    let settlements = strata::sim::Settlements(vec![strata::sim::PlacedSettlement {
        site: site.clone(),
        layout: Some(layout.clone()),
    }]);
    let quay = super::super::people::quay_point(&settlements, &world, &rules).unwrap();
    let arrival = super::super::people::arrival(&settlements, &world, &rules).unwrap();
    assert_eq!(
        arrival.at, quay.0,
        "on the quay, not the Exchange's fallback"
    );
    for (who, at) in [("the household", arrival.at), ("Maren", arrival.greeter)] {
        let sample = world.sample(at.0, at.1);
        let over = f64::from(sample.height) - f64::from(world.sea_level());
        assert!(
            over > rules.shore_m && !sample.in_water(),
            "{who} is dry: {over:.2} m"
        );
        assert!(sample.landform.developed > 0.5, "{who} stands on paving");
        let [x, z] = site.to_local_xz([at.0, at.1]);
        // Every proxy solid's box in the world: a body standing on the
        // highest one under it (the terrain, or a deck a step above it)
        // meets none between its knees and its head.
        let mut boxes = Vec::new();
        for building in &layout.buildings {
            let file = format!(
                "{}/../../generated/{}",
                env!("CARGO_MANIFEST_DIR"),
                building.file
            );
            let baked = ashlar::BakedBuilding::read(&std::fs::read(&file).unwrap()).unwrap();
            let origin = settlements.0[0].to_world(building.position);
            #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
            let turn = Quat::from_rotation_y(settlements.0[0].world_yaw(building.yaw) as f32);
            for placed in baked.level(0).unwrap().colliders() {
                let mut low = [f64::MAX; 3];
                let mut high = [f64::MIN; 3];
                for vertex in &placed.solid.vertices {
                    #[allow(clippy::cast_possible_truncation, reason = "metres near the origin")]
                    let turned =
                        turn * Vec3::new(vertex.x as f32, vertex.y as f32, vertex.z as f32);
                    let world = [
                        origin.x() + f64::from(turned.x),
                        origin.y() + f64::from(turned.y),
                        origin.z() + f64::from(turned.z),
                    ];
                    for axis in 0..3 {
                        low[axis] = low[axis].min(world[axis]);
                        high[axis] = high[axis].max(world[axis]);
                    }
                }
                boxes.push((building.file.clone(), low, high));
            }
        }
        let over = |margin: f64| {
            move |(_, low, high): &&(String, [f64; 3], [f64; 3])| {
                at.0 > low[0] - margin
                    && at.0 < high[0] + margin
                    && at.1 > low[2] - margin
                    && at.1 < high[2] + margin
            }
        };
        assert!(
            boxes.iter().filter(over(20.0)).count() > 3,
            "the quay's cargo round {who} is read"
        );
        let terrain = f64::from(sample.height);
        let floor = boxes
            .iter()
            .filter(over(0.0))
            .map(|(_, _, high)| high[1])
            .filter(|top| *top <= terrain + 1.0)
            .fold(terrain, f64::max);
        // Open paving: nothing within two metres, so neither the body nor
        // the camera behind it is pressed against a crate.
        for (file, low, high) in boxes.iter().filter(over(2.0)) {
            assert!(
                // A kerb a step high is walked over.
                high[1] <= floor + 0.5 || low[1] >= floor + 1.9,
                "{who} at ({x:.1}, {z:.1}) stands in a proxy of {file}: {low:?} to {high:?}, floor {floor:.2}"
            );
        }
        for plant in &layout.plants {
            let apart = (plant.position[0] - x).hypot(plant.position[2] - z);
            assert!(
                apart > 1.5,
                "{who} stands {apart:.1} m from a {}",
                plant.species
            );
        }
    }
    // Maren is ahead of the household, to its right, and faces it.
    let (forward, right) = (
        PlayerIntent::forward(arrival.yaw),
        PlayerIntent::right(arrival.yaw),
    );
    let (dx, dz) = (
        arrival.greeter.0 - arrival.at.0,
        arrival.greeter.1 - arrival.at.1,
    );
    let ahead = dx * f64::from(forward.x) + dz * f64::from(forward.z);
    let aside = dx * f64::from(right.x) + dz * f64::from(right.z);
    assert!((ahead - rules.greeter_ahead_m).abs() < 1e-6, "{ahead}");
    assert!((aside - rules.greeter_aside_m).abs() < 1e-6, "{aside}");
    let back = PlayerIntent::forward(arrival.greeter_yaw);
    let apart = dx.hypot(dz);
    assert!(
        (f64::from(back.x) + dx / apart).abs() < 1e-3
            && (f64::from(back.z) + dz / apart).abs() < 1e-3,
        "Maren faces the household"
    );
    // And the household looks out to sea: the ground falls away ahead.
    let seaward = (
        arrival.at.0 + f64::from(forward.x) * 40.0,
        arrival.at.1 + f64::from(forward.z) * 40.0,
    );
    assert!(
        world.sample(seaward.0, seaward.1).height < world.sample(arrival.at.0, arrival.at.1).height
    );
}
