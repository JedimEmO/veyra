#![allow(
    clippy::unwrap_used,
    reason = "a missing scenario piece fails the test"
)]
use super::*;
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use veyra_sim::combat::Weapon;

fn world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.init_resource::<Time>();
    world.init_resource::<FieldTints>();
    world.init_resource::<ArcMeshes>();
    world.init_resource::<Messages<CombatImpact>>();
    world.init_resource::<Messages<BlastWent>>();
    world.run_system_once(super::super::load).unwrap();
    world
}

fn pulses(world: &mut World) -> usize {
    world.query::<&Pulse>().iter(world).count()
}

fn once<M>(system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>) -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems(system);
    schedule
}

/// A melee bearer part way into the contact window of `action`.
fn cutter(world: &mut World, weapon: Weapon, action: Action, team: u8) -> Entity {
    let mut actor = Combatant::on_team(team, 100.);
    actor.weapon = weapon;
    actor.action = action;
    actor.sequence = 1;
    actor.age = actor.spec(action).contact_start + 0.01;
    world.spawn((actor, WorldPosition::new(0., 1., 0.))).id()
}

fn impact(guard: GuardOutcome, victim: Entity, lethal: bool) -> CombatImpact {
    CombatImpact {
        attacker: Entity::PLACEHOLDER,
        victim: Some(victim),
        from: WorldPos::new(0., 1., 5.),
        point: WorldPos::new(0., 1., 0.),
        melee: true,
        dealt: 0.,
        lethal,
        guard,
        shot: None,
    }
}

#[test]
fn a_cut_draws_its_arc_once_when_it_reaches_contact_and_not_before() {
    let mut world = world();
    let mut arcs = once(cut_arcs);
    arcs.run(&mut world);
    assert_eq!(pulses(&mut world), 0, "nobody is cutting");
    let cutter_id = cutter(&mut world, Weapon::Blade, Action::Light, 0);
    world.get_mut::<Combatant>(cutter_id).unwrap().age = 0.;
    arcs.run(&mut world);
    assert_eq!(pulses(&mut world), 0, "still in the windup");
    let start = Action::Light.spec().contact_start;
    world.get_mut::<Combatant>(cutter_id).unwrap().age = start + 0.01;
    arcs.run(&mut world);
    arcs.run(&mut world);
    assert_eq!(pulses(&mut world), 1, "once for the cut, not each frame");
    let arc = world.query::<&Pulse>().single(&world).unwrap();
    assert!(matches!(arc.shape, Shape::Arc) && arc.mesh.is_some());
    assert_eq!(arc.tint, 0, "the player's blade is the cool colour");
}

#[test]
fn a_heavy_arc_is_wider_and_longer_lived_and_the_reach_is_the_weapons() {
    let mut world = world();
    let light = cutter(&mut world, Weapon::Blade, Action::Light, 0);
    let heavy = cutter(&mut world, Weapon::Blade, Action::Heavy, 1);
    once(cut_arcs).run(&mut world);
    let mut found: Vec<(f32, f32, usize)> = world
        .query::<&Pulse>()
        .iter(&world)
        .map(|arc| (arc.lifetime, arc.size.x, arc.tint))
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(found.len(), 2);
    assert!(found[0].0 < found[1].0 && found[1].0 <= 0.2 + f32::EPSILON);
    assert_eq!(found[1].2, 1, "a hostile's blade is warm");
    // The tip is the contact's own: the hilt and the class's reach.
    let blade = world.get::<Combatant>(light).unwrap().reach_factor();
    assert!((found[0].1 - (0.45 + 1.2 * blade)).abs() < 1e-5);
    // A glaive reaches further than a blade and its arc is drawn so.
    world.despawn(light);
    world.despawn(heavy);
    cutter(&mut world, Weapon::Glaive, Action::Light, 0);
    world.query::<&Pulse>().iter(&world).count();
    let mut again = once(cut_arcs);
    again.run(&mut world);
    let glaive = world
        .query::<&Pulse>()
        .iter(&world)
        .map(|arc| arc.size.x)
        .fold(0., f32::max);
    assert!(glaive > found[0].1, "{glaive} against {}", found[0].1);
}

#[test]
fn the_arc_spans_the_weapons_own_sweep() {
    let light = Action::Light.spec();
    let [a, b] = [light.contact_start, light.contact_end]
        .map(|time| sweep_angle(blade_point(Action::Light, &light, time, 1.), false));
    assert!((a.to_degrees() + 70.).abs() < 1e-3 && (b.to_degrees() - 70.).abs() < 1e-3);
    let heavy = Action::Heavy.spec();
    let [a, b] = [heavy.contact_start, heavy.contact_end]
        .map(|time| sweep_angle(blade_point(Action::Heavy, &heavy, time, 1.), true));
    assert!((a.to_degrees() + 55.).abs() < 1e-3 && (b.to_degrees() - 50.).abs() < 1e-3);
}

#[test]
fn a_guard_met_draws_by_its_outcome_and_an_open_hit_draws_none() {
    let mut world = world();
    let victim = world.spawn(WorldPosition::new(0., 1., 0.)).id();
    let mut guard = once(guard_effects);
    let mut count = |world: &mut World, outcome| {
        world.resource_mut::<Messages<CombatImpact>>().clear();
        world.write_message(impact(outcome, victim, false));
        let before = pulses(world);
        guard.run(world);
        pulses(world) - before
    };
    assert_eq!(count(&mut world, GuardOutcome::Open), 0);
    let blocked = count(&mut world, GuardOutcome::Blocked);
    let parried = count(&mut world, GuardOutcome::Parried);
    let broken = count(&mut world, GuardOutcome::Broken);
    assert!(blocked >= 2, "sparks and a ring: {blocked}");
    assert!(parried >= 2, "a flash and sparks: {parried}");
    assert!(broken > blocked, "a burst is more than a block: {broken}");
}

#[test]
fn a_blast_draws_a_ring_to_its_radius_and_none_without_one() {
    let mut world = world();
    let mut rings = once(blast_rings);
    rings.run(&mut world);
    assert_eq!(pulses(&mut world), 0);
    world.write_message(BlastWent {
        point: WorldPos::new(0., 1., 0.),
        radius: 4.,
    });
    rings.run(&mut world);
    let ring = world
        .query::<&Pulse>()
        .iter(&world)
        .find(|effect| matches!(effect.shape, Shape::Ring))
        .unwrap();
    let end = ring.size.x * (1. + ring.growth) * 0.75;
    assert!(
        (end - 4.).abs() < 1e-4,
        "its crest ends on the radius: {end}"
    );
    assert!(pulses(&mut world) > 2, "with a glow and sparks");
}

#[test]
fn a_death_scatters_and_a_wound_does_not() {
    let mut world = world();
    let victim = world.spawn(WorldPosition::new(0., 1., 0.)).id();
    let mut deaths = once(deaths);
    world.write_message(impact(GuardOutcome::Open, victim, false));
    deaths.run(&mut world);
    assert_eq!(pulses(&mut world), 0);
    world.resource_mut::<Messages<CombatImpact>>().clear();
    world.write_message(impact(GuardOutcome::Open, victim, true));
    deaths.run(&mut world);
    assert!(pulses(&mut world) >= 10, "a scatter");
}

#[test]
fn a_status_glows_from_the_moment_it_is_borne_to_the_moment_it_ends_on_any_bearer() {
    let mut world = world();
    let player = world
        .spawn((Combatant::on_team(0, 100.), WorldPosition::new(0., 1., 0.)))
        .id();
    let hostile = world
        .spawn((Combatant::on_team(1, 100.), WorldPosition::new(5., 1., 0.)))
        .id();
    let mut glows = once(status_glows);
    glows.run(&mut world);
    assert_eq!(pulses(&mut world), 0, "no status, no glow");
    world
        .get_mut::<Combatant>(player)
        .unwrap()
        .give_status("focused", 5.);
    world
        .get_mut::<Combatant>(hostile)
        .unwrap()
        .give_status("slowed", 5.);
    world
        .get_mut::<Combatant>(hostile)
        .unwrap()
        .give_status("a_status_with_no_colour", 5.);
    glows.run(&mut world);
    glows.run(&mut world);
    assert_eq!(pulses(&mut world), 2, "one a bearer and status, kept");
    // The glow breathes without ever being retired.
    let mark = world.query::<&Pulse>().iter(&world).next().unwrap();
    assert!(mark.age < mark.lifetime);
    // It ends with the bearer's hold of it: a bearer that falls loses it.
    world.get_mut::<Combatant>(player).unwrap().action = Action::Dead;
    glows.run(&mut world);
    world.flush();
    assert_eq!(world.query::<&StatusMark>().iter(&world).count(), 1);
    let left = world.query::<&StatusMark>().single(&world).unwrap();
    assert_eq!(left.bearer, hostile);
}

#[test]
fn the_cap_holds_and_drops_the_oldest_first_and_spares_what_is_kept() {
    let mut world = world();
    let kept = world
        .spawn(Pulse::glow(
            WorldPos::new(0., 0., 0.),
            0,
            Vec3::ONE,
            f32::MAX,
        ))
        .id();
    for i in 0..PULSE_CAP + 20 {
        let mut effect = Pulse::glow(WorldPos::new(0., 0., 0.), 0, Vec3::ONE, 1.);
        #[allow(clippy::cast_precision_loss, reason = "a small test index")]
        {
            effect.age = i as f32 * 0.001;
        }
        world.spawn(effect);
    }
    once(cap_pulses).run(&mut world);
    world.flush();
    assert_eq!(
        pulses(&mut world),
        PULSE_CAP + 1,
        "the cap and the kept one"
    );
    assert!(world.get::<Pulse>(kept).is_some());
    let youngest = world
        .query::<&Pulse>()
        .iter(&world)
        .filter(|effect| effect.lifetime < KEPT)
        .map(|effect| effect.age)
        .fold(f32::MAX, f32::min);
    assert!(youngest < 0.0001 + f32::EPSILON, "the youngest all stayed");
    let oldest = world
        .query::<&Pulse>()
        .iter(&world)
        .filter(|effect| effect.lifetime < KEPT)
        .map(|effect| effect.age)
        .fold(0., f32::max);
    #[allow(clippy::cast_precision_loss, reason = "a small test count")]
    let cap = PULSE_CAP as f32;
    assert!(
        oldest < (cap - 1.) * 0.001 + 1e-4,
        "the oldest twenty went: {oldest}"
    );
}

#[test]
fn every_status_the_table_names_has_a_glow_colour() {
    let table = veyra_sim::combat_tables::current();
    assert!(!table.abilities.statuses.is_empty());
    for status in &table.abilities.statuses {
        assert!(
            status_colour(&status.id).is_some(),
            "status {} has no glow colour",
            status.id
        );
    }
}
