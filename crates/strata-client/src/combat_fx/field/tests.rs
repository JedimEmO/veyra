#![allow(
    clippy::unwrap_used,
    reason = "a missing scenario piece fails the test"
)]
use super::*;
use bevy::ecs::system::RunSystemOnce;
use std::sync::Arc;
use std::time::Duration;
use strata_sim::deposits::DepositId;

fn shipped() -> strata_registry::GameRegistries {
    strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("assets/ loads")
}

fn world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.init_resource::<Time>();
    world.init_resource::<FieldTints>();
    world.run_system_once(super::super::load).unwrap();
    world
}

fn pulses(world: &mut World) -> usize {
    world.query::<&Pulse>().iter(world).count()
}

/// A schedule of one system, which keeps its `Local`s and change ticks
/// between runs as the app's does.
fn once<M>(system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>) -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(system);
    schedule
}

fn tick(world: &mut World, seconds: f32) {
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(seconds));
}

#[test]
fn a_ping_draws_a_standing_band_out_to_its_range_over_its_channel_and_only_with_a_ping() {
    let mut world = world();
    let registries = Arc::new(shipped());
    let pack = {
        let saved = registries
            .items
            .starting()
            .and_then(|start| start.packs.values().next())
            .expect("a starting pack");
        Pack::from_saved(saved, &registries.items).expect("a pack")
    };
    let sheet = CharacterSheet::default();
    let skill = skill_of(&registries, &pack, &sheet).expect("the starting pack carries the tool");
    world.insert_resource(Registries(registries));
    let member = world
        .spawn((WorldPosition::new(0., 10., 0.), pack, sheet))
        .id();
    let mut rings = once(ping_rings);
    rings.run(&mut world);
    assert_eq!(pulses(&mut world), 0, "no ping, no ring");
    world.entity_mut(member).insert(Pinging {
        class: "strata:structural_metal".parse().unwrap(),
        left: skill.channel_s,
    });
    rings.run(&mut world);
    assert_eq!(pulses(&mut world), 1);
    let ring = world.query::<&Pulse>().single(&world).unwrap();
    assert!(matches!(ring.shape, Shape::Ring));
    assert!(
        ring.axis.is_none(),
        "a flat ring is not turned to the camera"
    );
    assert!((ring.lifetime - skill.channel_s).abs() < 1e-4);
    // Its wall at the end is the ping's range.
    let end = ring.size.x * (1. + ring.growth);
    assert!((f64::from(end) - skill.range_m).abs() < 1e-2, "{end}");
    // Standing up, a band of its own, so a low camera sees it.
    assert!(ring.mesh.is_some());
    assert_eq!(ring.rotation, Quat::IDENTITY);
    // The same ping is not drawn twice.
    rings.run(&mut world);
    assert_eq!(pulses(&mut world), 1);
}

#[test]
fn a_sampling_member_throws_sparks_every_third_of_a_second_and_a_stopped_one_none() {
    let mut world = world();
    let member = world
        .spawn((
            WorldPosition::new(0., 1., 0.),
            Combatant::default(),
            Sampling {
                deposit: DepositId([0, 0]),
                from: [0., 0.],
                left: 3.,
                got: 0,
            },
        ))
        .id();
    let mut sparks = once(sample_sparks);
    let mut run = |world: &mut World| sparks.run(world);
    run(&mut world);
    // The puff and four sparks, at once.
    assert_eq!(pulses(&mut world), 5);
    tick(&mut world, 0.1);
    run(&mut world);
    assert_eq!(pulses(&mut world), 5);
    tick(&mut world, 0.25);
    run(&mut world);
    assert_eq!(pulses(&mut world), 10);
    world.entity_mut(member).remove::<Sampling>();
    tick(&mut world, 1.);
    run(&mut world);
    assert_eq!(pulses(&mut world), 10);
}

#[test]
fn dust_rises_with_speed_and_is_none_standing_off_the_ground_or_over_water() {
    assert!(dust_per_second(0., true).abs() < f32::EPSILON);
    assert!(dust_per_second(STANDING_SHARE * 0.5, true).abs() < f32::EPSILON);
    assert!(dust_per_second(1., false).abs() < f32::EPSILON);
    let slow = dust_per_second(0.3, true);
    let fast = dust_per_second(0.9, true);
    assert!(slow > 0. && fast > slow);
    assert!((dust_per_second(3., true) - DUST_RATE).abs() < 1e-5);
}

#[test]
fn a_ridden_bike_throws_dust_by_its_speed_and_stood_still_throws_none() {
    let mut world = world();
    let registries = shipped();
    let def = registries
        .vehicles
        .get(&"veyra:vehicle.courier_bike".parse().unwrap())
        .expect("the shipped bike");
    let mut riding = Riding::new(def, 0.);
    let top = riding.dials.speed.road.max(riding.dials.speed.ground);
    world.spawn((WorldPosition::new(0., 1., 0.), riding.clone()));
    let mut dust = once(bike_dust);
    tick(&mut world, 0.5);
    dust.run(&mut world);
    assert_eq!(pulses(&mut world), 0, "standing");
    let rider = world
        .query_filtered::<Entity, With<Riding>>()
        .single(&world)
        .unwrap();
    riding.speed = top * 0.5;
    world.entity_mut(rider).insert(riding.clone());
    dust.run(&mut world);
    let slow = pulses(&mut world);
    assert!(slow > 0);
    riding.speed = top;
    world.entity_mut(rider).insert(riding);
    tick(&mut world, 0.2);
    dust.run(&mut world);
    assert!(pulses(&mut world) > slow);
}

#[test]
fn a_ridden_bike_has_one_glow_which_goes_with_the_ride() {
    let mut world = world();
    let registries = shipped();
    let def = registries
        .vehicles
        .get(&"veyra:vehicle.courier_bike".parse().unwrap())
        .expect("the shipped bike");
    world.init_resource::<Time<Fixed>>();
    let rider = world
        .spawn((
            WorldPosition::new(0., 1., 0.),
            PreviousWorldPosition::default(),
            Riding::new(def, 0.),
        ))
        .id();
    let mut glow = once(bike_glow);
    glow.run(&mut world);
    glow.run(&mut world);
    assert_eq!(world.query::<&BikeGlow>().iter(&world).count(), 1);
    world.entity_mut(rider).remove::<Riding>();
    glow.run(&mut world);
    assert_eq!(world.query::<&BikeGlow>().iter(&world).count(), 0);
}

#[test]
fn a_dust_puff_asked_for_draws_one_pulse_and_none_unasked() {
    let mut world = world();
    world.init_resource::<Messages<DustPuff>>();
    let mut puffs = once(prop_puffs);
    puffs.run(&mut world);
    assert_eq!(pulses(&mut world), 0);
    world.write_message(DustPuff(WorldPos::new(1., 0., 1.)));
    puffs.run(&mut world);
    assert_eq!(pulses(&mut world), 1);
    puffs.run(&mut world);
    assert_eq!(pulses(&mut world), 1);
}

#[test]
fn the_kneel_eases_in_with_sampling_and_out_after_it() {
    assert!((eased(0., 1., 0.1) - KNEEL_RATE * 0.1).abs() < 1e-6);
    assert!((eased(0.95, 1., 1.) - 1.).abs() < f32::EPSILON);
    assert!(eased(0.5, 0., 0.01) < 0.5);
    assert!(eased(0.01, 0., 1.).abs() < f32::EPSILON);
    let mut world = world();
    let member = world
        .spawn(Sampling {
            deposit: DepositId([0, 0]),
            from: [0., 0.],
            left: 3.,
            got: 0,
        })
        .id();
    let mut kneel = once(ease_kneel);
    for _ in 0..4 {
        tick(&mut world, 0.1);
        kneel.run(&mut world);
    }
    let depth = world.get::<Kneel>(member).expect("kneeling").0;
    assert!(depth > 0.3 && depth <= 1.);
    world.entity_mut(member).remove::<Sampling>();
    for _ in 0..12 {
        tick(&mut world, 0.1);
        kneel.run(&mut world);
    }
    assert!(world.get::<Kneel>(member).is_none());
}
