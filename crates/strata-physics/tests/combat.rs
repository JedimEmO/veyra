//! Combat against actual Avian colliders with no renderer or procedural terrain.
#![allow(
    clippy::unwrap_used,
    reason = "a missing scenario component fails the test"
)]

use bevy::app::App;
use bevy::ecs::prelude::Entity;
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use bevy::transform::components::Transform;
use std::sync::Arc;
use strata_core::WorldPos;
use strata_physics::avian3d::prelude::{Collider, RigidBody};
use strata_physics::{CharacterController, PhysicsPlugin};
use strata_registry::BiomeId;
use strata_sim::combat::{Action, CombatIntent, Combatant};
use strata_sim::combat_curves::MeleeCurves;
use strata_sim::{LocalPlayer, Player, SimPlugin, WorldPosition, WorldSourceRes};
use strata_stream::StreamPlugin;
use strata_worldgen::{Sample, WorldSource};

struct Flat;
impl WorldSource for Flat {
    fn sample(&self, _: f64, _: f64) -> Sample {
        Sample {
            height: 2.,
            biomes: [(BiomeId(0), 1.), (BiomeId(0), 0.)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.,
            landform: strata_worldgen::Landform::default(),
            water: None,
        }
    }
    fn sea_level(&self) -> f32 {
        0.
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        SimPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(Time::<Fixed>::from_hz(60.));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(Arc::new(Flat)));
    app.insert_resource(
        MeleeCurves::parse(include_str!("../../../assets/characters/combat/melee.ron")).unwrap(),
    );
    app.finish();
    app.cleanup();
    advance(&mut app, 3);
    // A ready floor makes the scenario independent of asynchronous stream tasks.
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(100., 1., 100.),
        WorldPosition::new(0., 1.5, 0.),
        Transform::default(),
    ));
    app
}

fn actor(app: &mut App, x: f64, z: f64, team: u8) -> Entity {
    app.world_mut()
        .spawn((
            Player::default(),
            CharacterController {
                height: 1.8,
                responsive: true,
                ..Default::default()
            },
            WorldPosition::new(x, 2.9, z),
            Combatant::on_team(team, 100.),
        ))
        .id()
}

/// An actor with `weapon` in hand and the pistol holstered.
fn armed(app: &mut App, x: f64, z: f64, team: u8, weapon: strata_sim::combat::Weapon) -> Entity {
    let id = actor(app, x, z, team);
    let mut combatant = app.world_mut().get_mut::<Combatant>(id).unwrap();
    *combatant = combatant
        .clone()
        .carrying([Some(weapon), Some(strata_sim::combat::Weapon::Pistol)], 0);
    id
}

fn advance(app: &mut App, ticks: u16) {
    for _ in 0..ticks {
        app.update();
    }
}
fn wall(app: &mut App, z: f64) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Static,
            Collider::cuboid(6., 4., 0.3),
            WorldPosition::new(0., 4., z),
            Transform::default(),
        ))
        .id()
}

#[test]
fn muzzle_cover_blocks_damage_then_an_open_ray_hits() {
    let mut app = app();
    let player = actor(&mut app, 0., 0., 0);
    app.world_mut().entity_mut(player).insert(LocalPlayer);
    let target = actor(&mut app, 0., -6., 1);
    let cover = wall(&mut app, -3.);
    advance(&mut app, 20);
    *app.world_mut().get_mut::<CombatIntent>(player).unwrap() = CombatIntent {
        fire: true,
        aim_point: WorldPos::new(0., 3.2, -6.),
        ..Default::default()
    };
    advance(&mut app, 15);
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 0);
    app.world_mut().despawn(cover);
    advance(&mut app, 15);
    assert!(app.world().get::<Combatant>(target).unwrap().hurts >= 1);
}

#[test]
fn a_dodge_hits_the_wall_instead_of_teleporting_through_it() {
    let mut app = app();
    let player = actor(&mut app, 0., 0., 0);
    app.world_mut().entity_mut(player).insert(LocalPlayer);
    wall(&mut app, -2.);
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .dodge = true;
    advance(&mut app, 40);
    let z = app.world().get::<WorldPosition>(player).unwrap().0.z();
    assert!(z < -1. && z > -1.8, "wall constrained dodge to z={z}");
}

#[test]
fn a_barrel_protruding_through_thin_cover_still_cannot_fire_through_it() {
    let mut app = app();
    let player = actor(&mut app, 0., 0., 0);
    app.world_mut().entity_mut(player).insert(LocalPlayer);
    let target = actor(&mut app, 0., -6., 1);
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(6., 4., 0.02),
        WorldPosition::new(0., 4., -0.4),
        Transform::default(),
    ));
    advance(&mut app, 20);
    *app.world_mut().get_mut::<CombatIntent>(player).unwrap() = CombatIntent {
        fire: true,
        aim_point: WorldPos::new(0., 3.2, -6.),
        ..Default::default()
    };
    advance(&mut app, 15);
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 0);
    assert!(app.world().get::<Combatant>(player).unwrap().shots > 0);
}

#[test]
fn the_exported_blade_hits_once_and_cannot_cut_through_cover() {
    for action in [Action::Light, Action::Followup, Action::Heavy] {
        for obstructed in [false, true] {
            let mut app = app();
            let player = actor(&mut app, 0., 0., 0);
            app.world_mut().entity_mut(player).insert(LocalPlayer);
            let target = actor(&mut app, 0., -1.5, 1);
            if obstructed {
                wall(&mut app, -0.75);
            }
            advance(&mut app, 20);
            // Enter each action directly here: this test isolates its geometry.
            // The sim tests separately exercise buffered action transitions.
            {
                let mut attacker = app.world_mut().get_mut::<Combatant>(player).unwrap();
                attacker.action = action;
                attacker.age = 0.;
            }
            advance(&mut app, 65);
            let hurts = app.world().get::<Combatant>(target).unwrap().hurts;
            assert_eq!(
                hurts,
                u64::from(!obstructed),
                "{action:?}, cover={obstructed}"
            );
        }
    }
}

#[test]
fn a_responsive_motor_reaches_jog_and_stops_within_the_authored_windows() {
    let mut app = app();
    let player = actor(&mut app, 0., 0., 0);
    app.world_mut().entity_mut(player).insert(LocalPlayer);
    app.world_mut()
        .get_mut::<CharacterController>(player)
        .unwrap()
        .walk_speed = 5.5;
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<strata_sim::PlayerIntent>(player)
        .unwrap()
        .move_dir = bevy::math::Vec2::Y;
    advance(&mut app, 9);
    assert!(
        (app.world()
            .get::<CharacterController>(player)
            .unwrap()
            .horizontal_velocity
            .length()
            - 5.5)
            .abs()
            < 0.01
    );
    app.world_mut()
        .get_mut::<strata_sim::PlayerIntent>(player)
        .unwrap()
        .move_dir = bevy::math::Vec2::ZERO;
    let before = app.world().get::<WorldPosition>(player).unwrap().0;
    advance(&mut app, 7);
    assert!(
        app.world()
            .get::<CharacterController>(player)
            .unwrap()
            .horizontal_velocity
            .length()
            < 0.01
    );
    let after = app.world().get::<WorldPosition>(player).unwrap().0;
    assert!(
        before.distance(after) < 0.32,
        "stopping distance {}",
        before.distance(after)
    );
}

#[test]
fn bolts_take_time_to_arrive_and_do_not_track_a_strafing_target() {
    for moving in [false, true] {
        let mut app = app();
        let shooter = actor(&mut app, 0., 0., 1);
        app.world_mut()
            .get_mut::<Combatant>(shooter)
            .unwrap()
            .bolt_speed = 24.;
        // One bolt a press, whatever the carbine's burst does.
        app.world_mut()
            .get_mut::<Combatant>(shooter)
            .unwrap()
            .mode_override = Some(strata_sim::combat::FireMode::Single);
        // The table's aimed cone (2.0 degrees, 0.63 m at 18 m) can miss a
        // standing target; this test is about the bolt's travel, so a sight
        // fitted to the shooter closes the cone to a hundredth of it.
        app.world_mut()
            .get_mut::<Combatant>(shooter)
            .unwrap()
            .equip(&strata_sim::fitting::Loadout {
                mods: [(
                    strata_sim::combat::Weapon::Carbine,
                    vec![strata_sim::fitting::Factor {
                        property: strata_registry::item::Property::AimSpread,
                        factor: 0.01,
                    }],
                )]
                .into(),
                ..Default::default()
            });
        let target = actor(&mut app, 0., -18., 0);
        app.world_mut().entity_mut(target).insert(LocalPlayer);
        advance(&mut app, 20);
        // Aimed, so the shot is in the narrowed cone above; the test is about
        // the bolt's travel, not the spread.
        *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
            fire: true,
            aim: true,
            aim_point: WorldPos::new(0., 3.2, -18.),
            ..Default::default()
        };
        advance(&mut app, 1);
        app.world_mut()
            .get_mut::<CombatIntent>(shooter)
            .unwrap()
            .fire = false;
        advance(&mut app, 15);
        assert_eq!(
            app.world().get::<Combatant>(target).unwrap().hurts,
            0,
            "no hitscan damage before arrival"
        );
        if moving {
            app.world_mut()
                .get_mut::<strata_sim::PlayerIntent>(target)
                .unwrap()
                .move_dir
                .x = 1.;
        }
        advance(&mut app, 50);
        assert_eq!(
            app.world().get::<Combatant>(target).unwrap().hurts,
            u64::from(!moving)
        );
        advance(&mut app, 300);
        assert_eq!(
            app.world_mut()
                .query::<&strata_sim::combat::BlasterBolt>()
                .iter(app.world())
                .count(),
            0,
            "missed bolts expire"
        );
    }
}

#[test]
fn an_aiming_shooters_bolts_leave_closer_to_one_line_than_a_hip_shooters() {
    use strata_sim::combat::BlasterBolt;
    // The mean spread of fifty bolts at a far target, read off their
    // velocities against their own mean direction.
    let mean_angle = |aim: bool| -> (f32, f32) {
        let mut app = app();
        let shooter = actor(&mut app, 0., 0., 1);
        let target = actor(&mut app, 0., -40., 0);
        app.world_mut().entity_mut(target).insert(LocalPlayer);
        app.world_mut()
            .get_mut::<Combatant>(shooter)
            .unwrap()
            .mode_override = Some(strata_sim::combat::FireMode::Single);
        advance(&mut app, 20);
        let mut seen = Vec::new();
        let mut directions = Vec::new();
        for _ in 0..50 {
            let mut actor = app.world_mut().get_mut::<Combatant>(shooter).unwrap();
            actor.ammo = actor.weapon_spec().magazine;
            *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
                fire: true,
                aim,
                aim_point: WorldPos::new(0., 3.2, -40.),
                ..Default::default()
            };
            advance(&mut app, 1);
            app.world_mut()
                .get_mut::<CombatIntent>(shooter)
                .unwrap()
                .fire = false;
            let mut bolts = app.world_mut().query::<(Entity, &BlasterBolt)>();
            for (entity, bolt) in bolts.iter(app.world()) {
                if !seen.contains(&entity) {
                    seen.push(entity);
                    directions.push(bolt.velocity.normalize());
                }
            }
            // Past the carbine's interval, so every press fires.
            advance(&mut app, 30);
        }
        assert_eq!(directions.len(), 50, "one bolt a shot");
        let mean = directions
            .iter()
            .copied()
            .sum::<bevy::math::Vec3>()
            .normalize();
        let angles: Vec<f32> = directions
            .iter()
            .map(|d| mean.dot(*d).clamp(-1., 1.).acos().to_degrees())
            .collect();
        (
            angles.iter().sum::<f32>() / 50.,
            angles.iter().copied().fold(0., f32::max),
        )
    };
    let row = strata_sim::combat::Weapon::Carbine.spec();
    let (hip, hip_widest) = mean_angle(false);
    let (aimed, aimed_widest) = mean_angle(true);
    assert!(aimed < hip * 0.6, "aimed {aimed} against hip {hip} degrees");
    // Two cones' width at most: the mean direction is itself a little off.
    assert!(aimed_widest <= 2. * row.aim_spread + 0.05, "{aimed_widest}");
    assert!(hip_widest <= 2. * row.spread + 0.05, "{hip_widest}");
}

/// A bolt of the carbine's row fired at a target `distance` metres off,
/// as if it had already flown `flown`; the pools the target lost.
fn lost_to_a_bolt(flown: f32, remaining: f32, distance: f64) -> f32 {
    use strata_sim::combat::{BlasterBolt, Weapon};
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let target = actor(&mut app, 0., -distance, 0);
    app.world_mut().entity_mut(target).insert(LocalPlayer);
    advance(&mut app, 20);
    let pools = |app: &App| {
        let target = app.world().get::<Combatant>(target).unwrap();
        target.health + target.shield
    };
    let before = pools(&app);
    let row = Weapon::Carbine.spec();
    app.world_mut().spawn(BlasterBolt {
        owner: shooter,
        team: 1,
        previous: WorldPos::new(0., 3.35, 0.),
        position: WorldPos::new(0., 3.35, 0.),
        velocity: bevy::math::Vec3::NEG_Z * row.bolt_speed,
        damage: row.damage,
        posture: row.posture,
        remaining,
        flown,
        falloff: row.falloff,
        weapon: Weapon::Carbine,
        effects: strata_sim::ability::ShotEffects::default(),
    });
    advance(&mut app, 60);
    before - pools(&app)
}

/// The pools an actor `distance` metres off loses to a rifle bolt that
/// starts `offset` metres past the muzzle, so the bolt meets the body at a
/// different point of a tick every time.
fn lost_to_a_rifle_bolt(distance: f64, offset: f64) -> f32 {
    use strata_sim::combat::{BlasterBolt, Weapon};
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let target = actor(&mut app, 0., -distance, 0);
    app.world_mut().entity_mut(target).insert(LocalPlayer);
    advance(&mut app, 20);
    let pools = |app: &App| {
        let target = app.world().get::<Combatant>(target).unwrap();
        target.health + target.shield
    };
    let before = pools(&app);
    let row = Weapon::Rifle.spec();
    app.world_mut().spawn(BlasterBolt {
        owner: shooter,
        team: 1,
        previous: WorldPos::new(0., 3.35, -offset),
        position: WorldPos::new(0., 3.35, -offset),
        velocity: bevy::math::Vec3::NEG_Z * row.bolt_speed,
        damage: row.damage,
        posture: row.posture,
        remaining: row.range,
        flown: 0.,
        falloff: row.falloff,
        weapon: Weapon::Rifle,
        effects: strata_sim::ability::ShotEffects::default(),
    });
    advance(&mut app, 240);
    before - pools(&app)
}

/// The rifle's bolt covers more than two metres a tick, several times an
/// actor's width side on: it is swept, so none passes through a body
/// between two ticks wherever in a tick it meets it.
#[test]
fn a_rifle_bolt_never_passes_through_a_body_between_two_ticks() {
    use strata_sim::combat::Weapon;
    let row = Weapon::Rifle.spec();
    let per_tick = f64::from(row.bolt_speed) / 60.;
    assert!(per_tick > 2., "{per_tick} m a tick");
    let mut hit = 0;
    for distance in [12., 40.5, 60., 120.25, 200.] {
        for phase in [0., 0.25, 0.5, 0.75, 0.999] {
            let offset = phase * per_tick;
            let lost = lost_to_a_rifle_bolt(distance, offset);
            #[allow(clippy::cast_possible_truncation, reason = "metres, a few hundred")]
            let share = row.share_at((distance - offset) as f32);
            assert!(
                lost > row.damage * share * 0.5,
                "{distance} m, {offset:.2} m into a tick: lost {lost}"
            );
            hit += 1;
        }
    }
    assert_eq!(hit, 25);
}

#[test]
fn a_bolts_damage_falls_with_the_distance_it_has_flown() {
    let row = strata_sim::combat::Weapon::Carbine.spec();
    let near = lost_to_a_bolt(0., 70., 6.);
    assert!(
        (near - row.damage).abs() < 0.01,
        "inside full range: {near}"
    );
    let far = lost_to_a_bolt(row.falloff.far, 70., 6.);
    assert!(
        (far - row.damage * row.falloff.share).abs() < 0.01,
        "{} m flown: {far}",
        row.falloff.far
    );
}

#[test]
fn a_bolt_dies_at_its_weapons_range() {
    use strata_sim::combat::{BlasterBolt, Weapon};
    let lost = lost_to_a_bolt(0., 3., 6.);
    assert!(
        lost.abs() < f32::EPSILON,
        "out of range deals nothing: {lost}"
    );
    // A shot leaves with the row's range and nothing flown.
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    app.world_mut()
        .get_mut::<Combatant>(shooter)
        .unwrap()
        .mode_override = Some(strata_sim::combat::FireMode::Single);
    advance(&mut app, 20);
    *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
        fire: true,
        aim_point: WorldPos::new(0., 3.2, -40.),
        ..Default::default()
    };
    advance(&mut app, 1);
    let mut bolts = app.world_mut().query::<&BlasterBolt>();
    let bolt = bolts.iter(app.world()).next().expect("a bolt left");
    let row = Weapon::Carbine.spec();
    assert!((bolt.flown + bolt.remaining - row.range).abs() < 1e-3);
    assert_eq!(bolt.falloff, row.falloff);
}

#[test]
fn projectile_damage_respects_dodge_immunity_at_arrival() {
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let target = actor(&mut app, 0., -6., 0);
    app.world_mut().entity_mut(target).insert(LocalPlayer);
    advance(&mut app, 20);
    *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
        fire: true,
        aim_point: WorldPos::new(0., 3.2, -6.),
        ..Default::default()
    };
    advance(&mut app, 1);
    app.world_mut()
        .get_mut::<CombatIntent>(shooter)
        .unwrap()
        .fire = false;
    // Freeze the motor so only arrival-time immunity is under test.
    app.world_mut()
        .entity_mut(target)
        .remove::<strata_sim::PlayerCommand>();
    {
        let mut actor = app.world_mut().get_mut::<Combatant>(target).unwrap();
        actor.action = Action::Dodge;
        actor.age = 0.12;
    }
    advance(&mut app, 30);
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 0);
}

#[test]
fn a_contact_buffer_chains_after_physics_on_the_same_tick() {
    let mut app = app();
    let player = armed(&mut app, 0., 0., 0, strata_sim::combat::Weapon::Blade);
    app.world_mut().entity_mut(player).insert(LocalPlayer);
    let target = actor(&mut app, 0., -1.5, 1);
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .light = true;
    advance(&mut app, 8);
    assert_eq!(
        app.world().get::<Combatant>(player).unwrap().action,
        Action::Light
    );
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .light = true;
    advance(&mut app, 5);
    assert_eq!(
        app.world().get::<Combatant>(player).unwrap().action,
        Action::Light
    );
    advance(&mut app, 1);
    let attacker = app.world().get::<Combatant>(player).unwrap();
    assert_eq!(attacker.action, Action::Followup);
    assert!(
        attacker.age.abs() < f32::EPSILON,
        "the view sees the chain immediately"
    );
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 1);
    advance(&mut app, 35);
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 2);
    assert_eq!(app.world().get::<Combatant>(player).unwrap().sequence, 2);
}

/// The ledger over the real sweep: a fitted blade novice's cut lands, the
/// impact carries what it took, and the sheet earns that much blade XP.
/// The same cut from an actor whose boxes do not open the heavy is a
/// heavy that never starts.
#[test]
fn a_landed_cut_credits_blade_xp_and_an_unopened_heavy_never_swings() {
    use strata_registry::GameRegistries;
    use strata_registry::progression::XpKind;
    use strata_sim::progression::CharacterSheet;
    let mut app = app();
    let mut registries = GameRegistries::default();
    registries.progression = strata_registry::SkillGraph::shipped();
    let graph = registries.progression.clone();
    app.insert_resource(strata_sim::Registries(Arc::new(registries)));
    let player = armed(&mut app, 0., 0., 0, strata_sim::combat::Weapon::Blade);
    let target = actor(&mut app, 0., -1.5, 1);
    let mut sheet = CharacterSheet::default();
    sheet.learned.insert("blade.novice".into());
    app.world_mut()
        .get_mut::<Combatant>(player)
        .unwrap()
        .fit(&graph, &CharacterSheet::default());
    app.world_mut().entity_mut(player).insert(sheet.clone());
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .heavy = true;
    advance(&mut app, 65);
    assert_eq!(app.world().get::<Combatant>(target).unwrap().hurts, 0);
    assert_eq!(
        app.world().get::<Combatant>(player).unwrap().sequence,
        0,
        "no heavy without the Blade novice"
    );
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .heavy = false;
    app.world_mut()
        .get_mut::<Combatant>(player)
        .unwrap()
        .fit(&graph, &sheet);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .light = true;
    advance(&mut app, 65);
    let victim = app.world().get::<Combatant>(target).unwrap();
    assert_eq!(victim.hurts, 1);
    let taken = 100. + 40. - (victim.health + victim.shield);
    assert!(
        (taken - 28.).abs() < 1e-3,
        "the light cut's damage: {taken}"
    );
    let sheet = app.world().get::<CharacterSheet>(player).unwrap();
    assert_eq!(sheet.xp(XpKind::Blade), 28);
    assert_eq!(sheet.xp(XpKind::Ranged), 0);
}

/// How many times a cut of `weapon` from the origin hurts a dummy `metres`
/// out, in the same stance on the same ground.
fn cut_hurts(weapon: strata_sim::combat::Weapon, metres: f64) -> u64 {
    let mut app = app();
    let player = armed(&mut app, 0., 0., 0, weapon);
    let target = actor(&mut app, 0., -metres, 1);
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .light = true;
    advance(&mut app, 90);
    app.world().get::<Combatant>(target).unwrap().hurts
}

#[test]
fn a_glaive_reaches_a_target_a_blade_does_not() {
    use strata_sim::combat::Weapon;
    let metres = 3.0;
    assert_eq!(cut_hurts(Weapon::Blade, metres), 0, "the blade falls short");
    assert_eq!(cut_hurts(Weapon::Glaive, metres), 1, "the glaive lands");
    assert_eq!(cut_hurts(Weapon::Blade, 1.5), 1, "both land up close");
}

/// Update once and note how the guard met any hit written this tick.
fn advance_noting(app: &mut App, met: &mut Vec<strata_sim::combat::GuardOutcome>) {
    use strata_sim::combat::CombatImpact;
    app.update();
    let messages = app
        .world()
        .resource::<bevy::ecs::message::Messages<CombatImpact>>();
    met.extend(
        messages
            .iter_current_update_messages()
            .filter(|impact| impact.victim.is_some())
            .map(|impact| {
                if impact.guard == strata_sim::combat::GuardOutcome::Parried {
                    assert!(
                        impact.dealt.abs() < f32::EPSILON && !impact.lethal,
                        "a parried impact deals nothing"
                    );
                }
                impact.guard
            }),
    );
}

/// A shooter 6 m ahead of a blade holder who is facing it; the blade
/// holder raises the guard once the bolt is within `raise_within` metres
/// (`None`: held from the start). Returns the world, the holder and how
/// the guard met each bolt that hit.
fn bolt_at_a_guard(
    raise_within: Option<f64>,
) -> (App, Entity, Vec<strata_sim::combat::GuardOutcome>) {
    let (app, holder, _, met) = bolt_at_a_guard_with(raise_within, false);
    (app, holder, met)
}

/// [`bolt_at_a_guard`], the holder bearing Reflecting when `reflecting`.
/// Returns the shooter as well.
fn bolt_at_a_guard_with(
    raise_within: Option<f64>,
    reflecting: bool,
) -> (App, Entity, Entity, Vec<strata_sim::combat::GuardOutcome>) {
    use strata_sim::combat::{BlasterBolt, FireMode, Weapon};
    let mut met = Vec::new();
    let mut app = app();
    let holder = armed(&mut app, 0., 0., 0, Weapon::Blade);
    let shooter = actor(&mut app, 0., -6., 1);
    app.world_mut()
        .get_mut::<Combatant>(shooter)
        .unwrap()
        .mode_override = Some(FireMode::Single);
    app.world_mut().get_mut::<Combatant>(holder).unwrap().shield = 0.;
    if reflecting {
        app.world_mut()
            .get_mut::<Combatant>(holder)
            .unwrap()
            .give_status("reflecting", 30.);
    }
    app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = raise_within.is_none();
    advance(&mut app, 30);
    *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
        fire: true,
        aim: true,
        aim_point: WorldPos::new(0., 3.2, 0.),
        ..Default::default()
    };
    advance_noting(&mut app, &mut met);
    app.world_mut()
        .get_mut::<CombatIntent>(shooter)
        .unwrap()
        .fire = false;
    if let Some(within) = raise_within {
        let mut ticks = 0;
        loop {
            let near = {
                let mut bolts = app.world_mut().query::<&BlasterBolt>();
                bolts
                    .iter(app.world())
                    .next()
                    .is_some_and(|bolt| bolt.position.0.z > -within)
            };
            if near || ticks > 120 {
                break;
            }
            advance_noting(&mut app, &mut met);
            ticks += 1;
        }
        app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = true;
    }
    for _ in 0..40 {
        advance_noting(&mut app, &mut met);
    }
    (app, holder, shooter, met)
}

#[test]
fn a_parried_bolt_deals_nothing_and_is_gone_a_blocked_one_costs_stamina() {
    use strata_sim::combat::{BlasterBolt, GuardOutcome};
    let (mut app, holder, met) = bolt_at_a_guard(Some(2.));
    assert_eq!(met, [GuardOutcome::Parried]);
    let actor = app.world().get::<Combatant>(holder).unwrap().clone();
    assert_eq!(actor.health.to_bits(), 100_f32.to_bits(), "taken nothing");
    assert_eq!(actor.hurts, 0, "a parry is no hurt");
    assert!(actor.stamina >= Combatant::default().stamina - 1e-3);
    assert_eq!(
        app.world_mut()
            .query::<&BlasterBolt>()
            .iter(app.world())
            .count(),
        0,
        "the bolt is despawned"
    );

    let (mut app, holder, met) = bolt_at_a_guard(None);
    assert_eq!(met, [GuardOutcome::Blocked]);
    let actor = app.world().get::<Combatant>(holder).unwrap().clone();
    assert_eq!(actor.hurts, 1, "a block lands what is left");
    assert!(
        actor.health < 100. && actor.health > 80.,
        "{}",
        actor.health
    );
    assert_eq!(
        app.world_mut()
            .query::<&BlasterBolt>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn a_parried_cut_staggers_a_swinger_whose_posture_the_riposte_empties() {
    use strata_sim::combat::Weapon;
    let mut app = app();
    let holder = armed(&mut app, 0., -1.5, 0, Weapon::Blade);
    let swinger = armed(&mut app, 0., 0., 1, Weapon::Blade);
    app.world_mut()
        .get_mut::<strata_sim::PlayerIntent>(holder)
        .unwrap()
        .yaw = std::f32::consts::PI;
    advance(&mut app, 20);
    let riposte = strata_sim::combat_tables::with_melee(|melee| melee.class(Weapon::Blade))
        .unwrap()
        .guard
        .riposte;
    app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = true;
    advance(&mut app, 1);
    // A swinger the riposte can empty, and one it only dents.
    app.world_mut()
        .get_mut::<Combatant>(swinger)
        .unwrap()
        .posture = riposte - 30.;
    app.world_mut()
        .get_mut::<CombatIntent>(swinger)
        .unwrap()
        .light = true;
    let mut staggered = false;
    let mut met = Vec::new();
    for _ in 0..40 {
        advance_noting(&mut app, &mut met);
        staggered |= app.world().get::<Combatant>(swinger).unwrap().action == Action::Stagger;
    }
    let holder = app.world().get::<Combatant>(holder).unwrap();
    assert_eq!(holder.health.to_bits(), 100_f32.to_bits());
    assert_eq!(met, [strata_sim::combat::GuardOutcome::Parried]);
    assert_eq!(holder.hurts, 0, "parried");
    assert!(staggered, "the riposte staggered the swinger");
}

/// A carbine holder with a butt strike on the bar, who presses it at once.
fn striker(app: &mut App) -> Entity {
    use strata_sim::ability::{Bar, Place};
    use strata_sim::combat::Weapon;
    let id = armed(app, 0., 0., 0, Weapon::Carbine);
    let mut bar = Bar::default();
    bar.set(Place::Weapon(Weapon::Carbine, 0), "butt_strike");
    app.world_mut()
        .get_mut::<Combatant>(id)
        .unwrap()
        .set_bar(&bar);
    id
}

#[test]
fn a_butt_strike_hits_what_is_in_reach_and_arc_once_and_nothing_behind() {
    let mut app = app();
    let player = striker(&mut app);
    let near = actor(&mut app, 0., -1.2, 1);
    let far = actor(&mut app, 0., -3., 1);
    let behind = actor(&mut app, 0., 1.2, 1);
    let aside = actor(&mut app, 1.5, 0., 1);
    let friend = actor(&mut app, 0.3, -1., 0);
    app.world_mut().get_mut::<Combatant>(near).unwrap().shield = 0.;
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(player)
        .unwrap()
        .ability = Some(0);
    advance(&mut app, 60);
    let hurts = |app: &App, who| app.world().get::<Combatant>(who).unwrap().hurts;
    assert_eq!(hurts(&app, near), 1, "in reach and in front, once");
    assert_eq!(hurts(&app, far), 0, "out of reach");
    assert_eq!(hurts(&app, behind), 0, "behind");
    assert_eq!(hurts(&app, aside), 0, "out of the arc");
    assert_eq!(hurts(&app, friend), 0, "and no friend");
    let hit = app.world().get::<Combatant>(near).unwrap();
    // Eight damage, less the health the ticks since have given back.
    assert!(hit.health > 92. && hit.health < 98., "{}", hit.health);
}

#[test]
fn a_butt_strike_can_be_parried_like_a_cut() {
    use strata_sim::combat::{GuardOutcome, Weapon};
    let guarded = |guard: bool| {
        let mut app = app();
        let player = striker(&mut app);
        let holder = armed(&mut app, 0., -1.2, 1, Weapon::Blade);
        app.world_mut()
            .get_mut::<strata_sim::PlayerIntent>(holder)
            .unwrap()
            .yaw = std::f32::consts::PI;
        app.world_mut().get_mut::<Combatant>(holder).unwrap().shield = 0.;
        advance(&mut app, 20);
        let posture = app.world().get::<Combatant>(player).unwrap().posture;
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .ability = Some(0);
        app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = guard;
        let mut met = Vec::new();
        for _ in 0..60 {
            advance_noting(&mut app, &mut met);
        }
        let holder = app.world().get::<Combatant>(holder).unwrap().clone();
        let striker = app.world().get::<Combatant>(player).unwrap().clone();
        (met, holder, posture, striker.posture)
    };
    let (met, holder, _, _) = guarded(false);
    assert_eq!(met, [GuardOutcome::Open]);
    assert_eq!(holder.hurts, 1);
    let (met, holder, before, after) = guarded(true);
    assert_eq!(met, [GuardOutcome::Parried]);
    assert_eq!(holder.hurts, 0, "a parry is no hurt");
    assert_eq!(holder.health.to_bits(), 100_f32.to_bits());
    assert!(after < before, "the riposte cost the striker posture");
}

#[test]
fn a_precision_shots_bolt_carries_its_factors_and_flies_exact() {
    use strata_sim::ability::{Bar, Place};
    use strata_sim::combat::{BlasterBolt, Weapon};
    let mut app = app();
    let player = armed(&mut app, 0., 0., 0, Weapon::Pistol);
    let mut bar = Bar::default();
    bar.set(Place::Weapon(Weapon::Pistol, 0), "precision_shot");
    app.world_mut()
        .get_mut::<Combatant>(player)
        .unwrap()
        .set_bar(&bar);
    advance(&mut app, 20);
    let row = Weapon::Pistol.spec();
    *app.world_mut().get_mut::<CombatIntent>(player).unwrap() = CombatIntent {
        ability: Some(0),
        aim_point: WorldPos::new(0.2, 3.35, -30.),
        ..Default::default()
    };
    let mut seen = Vec::new();
    for _ in 0..40 {
        app.update();
        let mut bolts = app.world_mut().query::<&BlasterBolt>();
        seen.extend(
            bolts
                .iter(app.world())
                .map(|bolt| (bolt.damage, bolt.posture, bolt.velocity)),
        );
        if !seen.is_empty() {
            break;
        }
    }
    let (damage, posture, velocity) = seen[0];
    assert_eq!(damage.to_bits(), (row.damage * 2.2).to_bits());
    assert_eq!(posture.to_bits(), (row.posture * 1.5).to_bits());
    // Exact: the muzzle is 0.2 m to the right, so a point 0.2 m right of the
    // actor, dead ahead of it, is a line with no sideways turn at all.
    assert!(velocity.x.abs() < 1e-3, "{velocity}");
    assert!(velocity.z < -row.bolt_speed * 0.99, "{velocity}");
}

/// An exact bolt flies down the middle of a standing capsule, the line a
/// sphere cast can miss on a tick; it must still hurt once for the factor
/// times the row's damage, wherever in a tick it meets the body.
#[test]
fn an_exact_pistol_bolt_hurts_a_standing_body_dead_ahead_once() {
    use strata_sim::combat::{BlasterBolt, Weapon};
    let row = Weapon::Pistol.spec();
    let per_tick = f64::from(row.bolt_speed) / 60.;
    for distance in [8., 30., 30.37, 55.] {
        for phase in [0., 0.3, 0.6, 0.9] {
            let offset = phase * per_tick;
            let mut app = app();
            let shooter = actor(&mut app, 0., 0., 1);
            let target = actor(&mut app, 0., -distance, 0);
            app.world_mut().entity_mut(target).insert(LocalPlayer);
            advance(&mut app, 20);
            let pools = |app: &App| {
                let target = app.world().get::<Combatant>(target).unwrap();
                target.health + target.shield
            };
            let before = pools(&app);
            app.world_mut().spawn(BlasterBolt {
                owner: shooter,
                team: 1,
                previous: WorldPos::new(0., 3.35, -offset),
                position: WorldPos::new(0., 3.35, -offset),
                velocity: bevy::math::Vec3::NEG_Z * row.bolt_speed,
                damage: row.damage * 2.2,
                posture: row.posture * 1.5,
                remaining: row.range,
                flown: 0.,
                falloff: row.falloff,
                weapon: Weapon::Pistol,
                effects: strata_sim::ability::ShotEffects::default(),
            });
            advance(&mut app, 240);
            let lost = before - pools(&app);
            #[allow(clippy::cast_possible_truncation, reason = "metres, a few dozen")]
            let share = row.share_at((distance - offset) as f32);
            let want = row.damage * 2.2 * share;
            assert!(
                lost > want * 0.5 && lost <= want * 1.01,
                "{distance} m, {offset:.2} m in: lost {lost}, wanted at most {want} (a pool takes part of a blow, two blows would be twice)"
            );
        }
    }
}

/// A bolt of the carbine's row from `from` along `direction`, owned by
/// `owner` of `team`, carrying `effects`.
fn bolt(
    app: &mut App,
    owner: Entity,
    team: u8,
    from: bevy::math::Vec3,
    toward: bevy::math::Vec3,
    effects: strata_sim::ability::ShotEffects,
) -> Entity {
    use strata_sim::combat::{BlasterBolt, Weapon};
    let row = Weapon::Carbine.spec();
    let at = WorldPos::new(f64::from(from.x), f64::from(from.y), f64::from(from.z));
    app.world_mut()
        .spawn(BlasterBolt {
            owner,
            team,
            previous: at,
            position: at,
            velocity: toward * row.bolt_speed,
            damage: 10.,
            posture: 4.,
            remaining: row.range,
            flown: 0.,
            falloff: strata_sim::combat::Falloff {
                full: 1e6,
                far: 2e6,
                share: 1.,
            },
            weapon: Weapon::Carbine,
            effects,
        })
        .id()
}

fn hurts(app: &App, who: Entity) -> u64 {
    app.world().get::<Combatant>(who).unwrap().hurts
}

fn pools(app: &App, who: Entity) -> f32 {
    let who = app.world().get::<Combatant>(who).unwrap();
    who.health + who.shield
}

fn effects_of(
    on_hit: Option<(&str, f32)>,
    jump: Option<(f32, f32)>,
    blast: Option<(f32, f32, f32)>,
    breach: bool,
) -> strata_sim::ability::ShotEffects {
    use strata_sim::ability::{Blast, Jump, OnHit, ShotEffects};
    ShotEffects {
        on_hit: on_hit.map(|(status, seconds)| OnHit {
            status: status.to_owned(),
            seconds,
        }),
        jump: jump.map(|(radius, share)| Jump { radius, share }),
        blast: blast.map(|(radius, damage, posture)| Blast {
            radius,
            damage,
            posture,
        }),
        breach,
        reflected: false,
    }
}

const ABOVE: f32 = 3.35;

#[test]
fn a_status_on_a_bolt_lands_on_an_open_target_and_on_no_guarded_one() {
    use strata_sim::combat::Weapon;
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let open = actor(&mut app, 0., -10., 0);
    let guard = armed(&mut app, 6., -10., 0, Weapon::Blade);
    // Facing the shooter, who is behind the line the bolts fly.
    app.world_mut()
        .get_mut::<strata_sim::PlayerIntent>(guard)
        .unwrap()
        .yaw = std::f32::consts::PI;
    app.world_mut().get_mut::<Combatant>(guard).unwrap().shield = 0.;
    app.world_mut().get_mut::<CombatIntent>(guard).unwrap().aim = true;
    advance(&mut app, 80);
    let carrying = effects_of(Some(("disarmed", 2.5)), None, None, false);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        carrying.clone(),
    );
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(6., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        carrying,
    );
    advance(&mut app, 60);
    let open = app.world().get::<Combatant>(open).unwrap();
    assert_eq!(open.statuses().len(), 1, "an open hit lands it");
    assert_eq!(open.statuses()[0].0, "disarmed");
    assert!(open.disarmed());
    let guard = app.world().get::<Combatant>(guard).unwrap();
    assert_eq!(guard.hurts, 1, "the block still lands what is left");
    assert!(guard.statuses().is_empty(), "and no status with it");
}

#[test]
fn a_breaching_bolt_empties_a_shield_and_a_plain_one_does_not() {
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let a = actor(&mut app, 0., -10., 0);
    let b = actor(&mut app, 6., -10., 0);
    for who in [a, b] {
        app.world_mut().get_mut::<Combatant>(who).unwrap().shield = 30.;
    }
    advance(&mut app, 20);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        effects_of(None, None, None, true),
    );
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(6., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        effects_of(None, None, None, false),
    );
    advance(&mut app, 40);
    let (a, b) = (
        app.world().get::<Combatant>(a).unwrap(),
        app.world().get::<Combatant>(b).unwrap(),
    );
    assert!(a.shield < 1., "{}", a.shield);
    assert!(
        a.health < 100. && b.health >= 100. - 1e-3,
        "{} {}",
        a.health,
        b.health
    );
    assert!(
        b.shield > 10.,
        "the shield took the plain bolt: {}",
        b.shield
    );
}

#[test]
fn a_jump_hurts_the_nearest_other_hostile_once_and_nobody_when_alone() {
    let setup = |with_second: bool| {
        let mut app = app();
        let shooter = actor(&mut app, 0., 0., 1);
        let first = actor(&mut app, 0., -10., 0);
        let second = actor(&mut app, 4., -10., 0);
        let far = actor(&mut app, -7.5, -10., 0);
        let friend = actor(&mut app, -1.5, -10., 1);
        if !with_second {
            app.world_mut().despawn(second);
            app.world_mut().despawn(far);
        }
        advance(&mut app, 20);
        let jump = effects_of(None, Some((8., 0.8)), None, false);
        bolt(
            &mut app,
            shooter,
            1,
            bevy::math::Vec3::new(0., ABOVE, -1.),
            bevy::math::Vec3::NEG_Z,
            jump,
        );
        advance(&mut app, 60);
        (app, first, second, far, friend)
    };
    let (app, first, second, far, friend) = setup(true);
    assert_eq!(hurts(&app, first), 1, "never struck back a second time");
    assert_eq!(hurts(&app, second), 1, "the nearest other");
    assert_eq!(hurts(&app, far), 0, "and only that one");
    assert_eq!(hurts(&app, friend), 0, "never the shooter's side");
    let (damage, share) = (10., 0.8);
    let first_lost =
        100. + app.world().get::<Combatant>(first).unwrap().shield_capacity - pools(&app, first);
    let second_lost = 100.
        + app
            .world()
            .get::<Combatant>(second)
            .unwrap()
            .shield_capacity
        - pools(&app, second);
    assert!(
        (second_lost / first_lost - share).abs() < 0.05,
        "{second_lost} against {first_lost} for {damage}"
    );
    let (app, first, ..) = setup(false);
    assert_eq!(hurts(&app, first), 1, "alone, nothing more happens");
}

#[test]
fn a_blast_strikes_each_hostile_in_the_radius_once_at_a_body_and_at_a_wall() {
    let blast = || effects_of(None, None, Some((4., 12., 60.)), false);
    let mut app = app();
    let shooter = actor(&mut app, 0., 0., 1);
    let struck = actor(&mut app, 0., -10., 0);
    let beside = actor(&mut app, 3., -10., 0);
    let outside = actor(&mut app, 6., -10., 0);
    let friend = actor(&mut app, -2., -10., 1);
    advance(&mut app, 20);
    let before = pools(&app, beside);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        blast(),
    );
    advance(&mut app, 60);
    assert_eq!(hurts(&app, struck), 2, "the bolt and the blast, once each");
    assert_eq!(hurts(&app, beside), 1);
    assert_eq!(hurts(&app, outside), 0);
    assert_eq!(hurts(&app, friend), 0, "never the user's side");
    assert_eq!(hurts(&app, shooter), 0);
    assert!(before - pools(&app, beside) > 0.);

    // At a wall: nobody is struck directly, the radius alone.
    let mut app = self::app();
    let shooter = actor(&mut app, 0., 0., 1);
    let near = actor(&mut app, 2., -8., 0);
    let away = actor(&mut app, 2., -3., 0);
    wall(&mut app, -10.);
    advance(&mut app, 20);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -1.),
        bevy::math::Vec3::NEG_Z,
        blast(),
    );
    advance(&mut app, 60);
    assert_eq!(hurts(&app, near), 1, "within the radius of the contact");
    assert_eq!(hurts(&app, away), 0);
}

#[test]
fn a_blast_beside_a_raised_guard_lands_whole_while_the_bolt_itself_is_met() {
    use strata_sim::combat::{GuardOutcome, Weapon};
    let mut app = app();
    let shooter = actor(&mut app, 0., -20., 1);
    let holder = armed(&mut app, 0., 0., 0, Weapon::Blade);
    app.world_mut().get_mut::<Combatant>(holder).unwrap().shield = 0.;
    app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = true;
    advance(&mut app, 30);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -19.),
        bevy::math::Vec3::Z,
        effects_of(None, None, Some((4., 12., 60.)), false),
    );
    let mut met = Vec::new();
    let mut open_dealt = 0.;
    for _ in 0..60 {
        advance_noting(&mut app, &mut met);
        open_dealt += app
            .world()
            .resource::<bevy::ecs::message::Messages<strata_sim::combat::CombatImpact>>()
            .iter_current_update_messages()
            .filter(|impact| impact.victim.is_some() && impact.guard == GuardOutcome::Open)
            .map(|impact| impact.dealt)
            .sum::<f32>();
    }
    assert_eq!(
        met,
        [GuardOutcome::Blocked, GuardOutcome::Open],
        "the bolt is met, the blast beside it is not"
    );
    assert!(
        (open_dealt - 12.).abs() < 1e-3,
        "the blast's whole damage: {open_dealt}"
    );
}

/// A blade holder at the origin, shield spent, guard up and bearing
/// Reflecting, with a gun holder 6 m behind the line of fire.
fn reflecting_holder(app: &mut App) -> (Entity, Entity) {
    use strata_sim::combat::Weapon;
    let holder = armed(app, 0., 0., 0, Weapon::Blade);
    let shooter = actor(app, 0., -6., 1);
    let mut combatant = app.world_mut().get_mut::<Combatant>(holder).unwrap();
    combatant.shield = 0.;
    combatant.give_status("reflecting", 30.);
    app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = true;
    advance(app, 40);
    (holder, shooter)
}

#[test]
fn a_guard_that_blocks_or_parries_a_bolt_sends_it_back() {
    use strata_sim::combat::GuardOutcome;
    for (raise, want) in [
        (None, GuardOutcome::Blocked),
        (Some(2.), GuardOutcome::Parried),
    ] {
        let (app, _, shooter, met) = bolt_at_a_guard_with(raise, true);
        assert_eq!(
            met,
            [want, GuardOutcome::Open],
            "{want:?}: the bolt and its return"
        );
        let full = 100.
            + app
                .world()
                .get::<Combatant>(shooter)
                .unwrap()
                .shield_capacity;
        assert!(full - pools(&app, shooter) > 1., "the shooter took it");
        // The same without Reflecting sends nothing.
        let (app, _, shooter, met) = bolt_at_a_guard_with(raise, false);
        assert_eq!(met, [want]);
        assert!((app.world().get::<Combatant>(shooter).unwrap().health - 100.).abs() < 1e-3);
    }
    // The share is the status row's (twice): a bolt of ten comes back as twenty.
    let mut app = app();
    let (holder, shooter) = reflecting_holder(&mut app);
    let full = 100.
        + app
            .world()
            .get::<Combatant>(shooter)
            .unwrap()
            .shield_capacity;
    let posture = app.world().get::<Combatant>(shooter).unwrap().posture;
    let mut met = Vec::new();
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -5.),
        bevy::math::Vec3::Z,
        effects_of(None, None, None, false),
    );
    for _ in 0..40 {
        advance_noting(&mut app, &mut met);
    }
    assert_eq!(met, [GuardOutcome::Blocked, GuardOutcome::Open]);
    let lost = full - pools(&app, shooter);
    assert!((lost - 20.).abs() < 0.5, "{lost}");
    let shot_at = app.world().get::<Combatant>(shooter).unwrap();
    assert!(shot_at.posture < posture, "and its posture too");
    assert_eq!(app.world().get::<Combatant>(holder).unwrap().hurts, 1);
}

#[test]
fn a_reflected_bolt_is_never_reflected_again() {
    use strata_sim::combat::{BlasterBolt, GuardOutcome};
    let mut app = app();
    let (_, shooter) = reflecting_holder(&mut app);
    // The shooter bears Reflecting and a guard too, so a bolt sent back at
    // it would be sent back again were it not marked.
    let mut sent = effects_of(None, None, None, false);
    sent.reflected = true;
    let mut met = Vec::new();
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -5.),
        bevy::math::Vec3::Z,
        sent,
    );
    for _ in 0..40 {
        advance_noting(&mut app, &mut met);
    }
    assert_eq!(met, [GuardOutcome::Blocked], "blocked, and that is all");
    assert_eq!(
        app.world_mut()
            .query::<&BlasterBolt>()
            .iter(app.world())
            .count(),
        0
    );
    let full = 100.
        + app
            .world()
            .get::<Combatant>(shooter)
            .unwrap()
            .shield_capacity;
    assert!((pools(&app, shooter) - full).abs() < 1e-3);
}

/// How far along -z the actor has gone from `z0`, in metres.
fn gone(app: &App, who: Entity, z0: f64) -> f64 {
    z0 - app.world().get::<WorldPosition>(who).unwrap().0.z()
}

fn thruster(app: &mut App, x: f64, z: f64) -> Entity {
    use strata_sim::ability::{Bar, Place};
    use strata_sim::combat::Weapon;
    let id = armed(app, x, z, 0, Weapon::Blade);
    let mut bar = Bar::default();
    bar.set(Place::Weapon(Weapon::Blade, 0), "thrust");
    app.world_mut()
        .get_mut::<Combatant>(id)
        .unwrap()
        .set_bar(&bar);
    id
}

#[test]
fn a_thrust_carries_its_distance_on_open_ground_and_a_wall_or_a_body_stops_it() {
    let press = |app: &mut App, who: Entity| {
        app.world_mut()
            .get_mut::<CombatIntent>(who)
            .unwrap()
            .ability = Some(0);
        advance(app, 70);
    };
    // Open ground.
    let mut app = app();
    let who = thruster(&mut app, 0., 0.);
    advance(&mut app, 20);
    let start = app.world().get::<WorldPosition>(who).unwrap().0.z();
    press(&mut app, who);
    let open = gone(&app, who, start);
    assert!((open - 6.).abs() < 0.5, "{open}");
    // A wall at 2.5 m.
    let mut app = self::app();
    let who = thruster(&mut app, 0., 0.);
    wall(&mut app, -2.5);
    advance(&mut app, 20);
    let start = app.world().get::<WorldPosition>(who).unwrap().0.z();
    press(&mut app, who);
    let walled = gone(&app, who, start);
    assert!(walled > 0.5 && walled < 2.4, "{walled}");
    // A body at 2.5 m: stopped by it, and the strike lands on it once.
    let mut app = self::app();
    let who = thruster(&mut app, 0., 0.);
    let body = actor(&mut app, 0., -2.5, 1);
    app.world_mut().get_mut::<Combatant>(body).unwrap().shield = 0.;
    advance(&mut app, 20);
    let start = app.world().get::<WorldPosition>(who).unwrap().0.z();
    press(&mut app, who);
    let blocked = gone(&app, who, start);
    assert!(blocked > 0.3 && blocked < 2.2, "{blocked}");
    assert_eq!(hurts(&app, body), 1, "the strike lands where it stopped");
    // A body the thrust ends short of is out of the strike's reach.
    let mut app = self::app();
    let who = thruster(&mut app, 0., 0.);
    let far = actor(&mut app, 0., -9., 1);
    advance(&mut app, 20);
    press(&mut app, who);
    assert_eq!(hurts(&app, far), 0);
}

#[test]
fn slowed_walkers_cover_a_share_of_the_ground_whoever_moves_them() {
    let mut app = app();
    let plain = actor(&mut app, -3., 0., 0);
    let slowed = actor(&mut app, 3., 0., 1);
    app.world_mut()
        .get_mut::<Combatant>(slowed)
        .unwrap()
        .give_status("slowed", 30.);
    advance(&mut app, 20);
    let (a0, b0) = (
        app.world().get::<WorldPosition>(plain).unwrap().0.z(),
        app.world().get::<WorldPosition>(slowed).unwrap().0.z(),
    );
    for who in [plain, slowed] {
        app.world_mut()
            .get_mut::<strata_sim::PlayerIntent>(who)
            .unwrap()
            .move_dir = bevy::math::Vec2::new(0., 1.);
    }
    advance(&mut app, 60);
    let (a, b) = (gone(&app, plain, a0), gone(&app, slowed, b0));
    assert!(a > 2., "{a}");
    assert!((b / a - 0.6).abs() < 0.08, "{b} against {a}");
}

#[test]
fn slowed_takes_the_sprint_with_it_as_any_pace_under_one_does() {
    let mut app = app();
    let plain = actor(&mut app, -3., 0., 0);
    let slowed = actor(&mut app, 3., 0., 1);
    app.world_mut()
        .get_mut::<Combatant>(slowed)
        .unwrap()
        .give_status("slowed", 30.);
    advance(&mut app, 20);
    let (a0, b0) = (
        app.world().get::<WorldPosition>(plain).unwrap().0.z(),
        app.world().get::<WorldPosition>(slowed).unwrap().0.z(),
    );
    for who in [plain, slowed] {
        let mut intent = app
            .world_mut()
            .get_mut::<strata_sim::PlayerIntent>(who)
            .unwrap();
        intent.move_dir = bevy::math::Vec2::new(0., 1.);
        intent.sprint = true;
    }
    advance(&mut app, 60);
    let (a, b) = (gone(&app, plain, a0), gone(&app, slowed, b0));
    // The status's share is of the walking speed: a sprinting bearer drops
    // to it, under the 0.6 of the sprint a plain runner keeps.
    assert!(b / a < 0.5, "{b} against {a}");
}

#[test]
fn a_reflected_bolt_belongs_to_the_one_who_sent_it_back() {
    use strata_sim::combat::BlasterBolt;
    let mut app = app();
    let (holder, shooter) = reflecting_holder(&mut app);
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, -5.),
        bevy::math::Vec3::Z,
        effects_of(None, None, None, false),
    );
    let team = app.world().get::<Combatant>(holder).unwrap().team;
    let weapon = app.world().get::<Combatant>(holder).unwrap().weapon;
    let mut seen = None;
    for _ in 0..40 {
        advance(&mut app, 1);
        let mut bolts = app.world_mut().query::<&BlasterBolt>();
        if let Some(sent) = bolts.iter(app.world()).find(|b| b.effects.reflected) {
            seen = Some((sent.owner, sent.team, sent.weapon));
            break;
        }
    }
    assert_eq!(seen, Some((holder, team, weapon)));
}

#[test]
fn a_blast_carries_the_shooters_scale_once_where_the_bolt_is_made() {
    use strata_sim::ability::{Bar, Place};
    use strata_sim::combat::{BlasterBolt, Weapon};
    let mut app = app();
    let shooter = armed(&mut app, 0., 0., 0, Weapon::Carbine);
    let mut bar = Bar::default();
    bar.set(Place::Weapon(Weapon::Carbine, 0), "explosive_shot");
    {
        let mut actor = app.world_mut().get_mut::<Combatant>(shooter).unwrap();
        actor.set_bar(&bar);
        actor.damage_scale = 0.5;
    }
    advance(&mut app, 20);
    *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
        ability: Some(0),
        aim_point: WorldPos::new(0., 3.35, -30.),
        ..Default::default()
    };
    let mut blast = None;
    for _ in 0..60 {
        app.update();
        let mut bolts = app.world_mut().query::<&BlasterBolt>();
        blast = bolts.iter(app.world()).find_map(|b| b.effects.blast);
        if blast.is_some() {
            break;
        }
    }
    let blast = blast.expect("the bolt flew");
    assert!((blast.damage - 24.).abs() < 1e-4 && (blast.posture - 45.).abs() < 1e-4);
}

#[test]
fn a_bolt_blocked_at_point_blank_by_a_reflecting_guard_comes_back_at_once() {
    use strata_sim::combat::Weapon;
    let mut app = app();
    // The guard faces the shooter 0.6 m off, inside the barrel's
    // reach, so no bolt is made.
    let holder = armed(&mut app, 0., 0., 0, Weapon::Blade);
    let shooter = armed(&mut app, 0., -0.6, 1, Weapon::Carbine);
    {
        let mut guard = app.world_mut().get_mut::<Combatant>(holder).unwrap();
        guard.shield = 0.;
        guard.give_status("reflecting", 30.);
    }
    app.world_mut().get_mut::<CombatIntent>(holder).unwrap().aim = true;
    advance(&mut app, 40);
    let before = pools(&app, shooter);
    *app.world_mut().get_mut::<CombatIntent>(shooter).unwrap() = CombatIntent {
        fire: true,
        aim_point: WorldPos::new(0., 3.35, 30.),
        ..Default::default()
    };
    let mut met = Vec::new();
    for _ in 0..30 {
        advance_noting(&mut app, &mut met);
    }
    assert!(
        met.contains(&strata_sim::combat::GuardOutcome::Blocked)
            || met.contains(&strata_sim::combat::GuardOutcome::Parried),
        "{met:?}"
    );
    assert!(before - pools(&app, shooter) > 1., "the shooter took it");
}

/// A victim of `weapon` 1.5 m in front of a blade, how much posture the
/// blade's `swing` took off it, or `None` if it was staggered (its posture
/// refilled).
fn posture_taken(weapon: strata_sim::combat::Weapon, swing: impl Fn(&mut CombatIntent)) -> f32 {
    use strata_sim::combat::Weapon;
    let mut app = app();
    let player = armed(&mut app, 0., 0., 0, Weapon::Blade);
    let target = armed(&mut app, 0., -1.5, 1, weapon);
    app.world_mut().get_mut::<Combatant>(target).unwrap().shield = 0.;
    advance(&mut app, 20);
    swing(&mut app.world_mut().get_mut::<CombatIntent>(player).unwrap());
    let mut lowest = 100_f32;
    for _ in 0..70 {
        advance(&mut app, 1);
        let victim = app.world().get::<Combatant>(target).unwrap();
        lowest = lowest.min(if victim.action == Action::Stagger {
            0.
        } else {
            victim.posture
        });
    }
    100. - lowest
}

#[test]
fn a_blades_cut_breaks_a_gun_bearers_posture_sooner_and_a_butt_strike_does_not() {
    use strata_sim::combat::Weapon;
    let caught = strata_sim::combat_tables::with_melee(|melee| melee.caught);
    let light = |intent: &mut CombatIntent| intent.light = true;
    let against_blade = posture_taken(Weapon::Blade, light);
    let against_gun = posture_taken(Weapon::Carbine, light);
    assert!(against_blade > 20., "{against_blade}");
    assert!(
        (against_gun / against_blade - caught.posture).abs() < 0.05,
        "{against_gun} against {against_blade}"
    );
    // The heavy: 80 of posture is a dent in a blade bearer and, caught, a
    // stagger in a gun bearer.
    let heavy = |intent: &mut CombatIntent| intent.heavy = true;
    assert!(posture_taken(Weapon::Blade, heavy) < 100.);
    assert!((posture_taken(Weapon::Carbine, heavy) - 100.).abs() < 1e-3);
    // A gun's butt strike is a cut that is not caught: the same blow whatever
    // the bearer holds. Since step 9 it breaks a full posture, so what is
    // compared is the stagger it begins and the health it takes.
    let butt = |who: Weapon| {
        let mut app = app();
        let player = striker(&mut app);
        let target = armed(&mut app, 0., -1.2, 1, who);
        app.world_mut().get_mut::<Combatant>(target).unwrap().shield = 0.;
        advance(&mut app, 20);
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .ability = Some(0);
        advance(&mut app, 12);
        let struck = app.world().get::<Combatant>(target).unwrap();
        (struck.action, struck.health)
    };
    let (on_gun, on_blade) = (butt(Weapon::Carbine), butt(Weapon::Blade));
    assert_eq!(on_gun.0, Action::Stagger);
    assert_eq!(on_blade.0, Action::Stagger);
    assert!(
        (on_gun.1 - on_blade.1).abs() < 1e-3,
        "{} and {}",
        on_gun.1,
        on_blade.1
    );
}

#[test]
fn the_rushers_heavy_is_caught_on_a_gun_bearer_without_a_stagger() {
    use strata_sim::combat::Weapon;
    let taken = |who: Weapon| {
        let mut app = app();
        let rusher = armed(&mut app, 0., 0., 0, Weapon::Blade);
        app.world_mut()
            .get_mut::<Combatant>(rusher)
            .unwrap()
            .damage_scale = 0.5;
        let target = armed(&mut app, 0., -1.5, 1, who);
        app.world_mut().get_mut::<Combatant>(target).unwrap().shield = 0.;
        advance(&mut app, 20);
        app.world_mut()
            .get_mut::<CombatIntent>(rusher)
            .unwrap()
            .heavy = true;
        let mut lowest = 100_f32;
        for _ in 0..70 {
            advance(&mut app, 1);
            let victim = app.world().get::<Combatant>(target).unwrap();
            assert_ne!(victim.action, Action::Stagger, "one heavy does not stagger");
            lowest = lowest.min(victim.posture);
        }
        100. - lowest
    };
    let caught = strata_sim::combat_tables::with_melee(|melee| melee.caught);
    let (gun, blade) = (taken(Weapon::Carbine), taken(Weapon::Blade));
    assert!((blade - 40.).abs() < 1., "{blade}");
    assert!((gun - 40. * caught.posture).abs() < 1., "{gun}");
}

#[test]
fn a_thrusts_strike_is_caught_on_a_gun_bearer() {
    use strata_sim::combat::Weapon;
    let taken = |who: Weapon| {
        let mut app = app();
        let player = thruster(&mut app, 0., 0.);
        let target = armed(&mut app, 0., -2.5, 1, who);
        app.world_mut().get_mut::<Combatant>(target).unwrap().shield = 0.;
        advance(&mut app, 20);
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .ability = Some(0);
        advance(&mut app, 50);
        100. - app.world().get::<Combatant>(target).unwrap().posture
    };
    let caught = strata_sim::combat_tables::with_melee(|melee| melee.caught);
    let (gun, blade) = (taken(Weapon::Carbine), taken(Weapon::Blade));
    assert!(blade > 20., "{blade}");
    assert!(
        (gun / blade - caught.posture).abs() < 0.06,
        "{gun} and {blade}"
    );
}

#[test]
#[allow(clippy::cast_possible_truncation, reason = "a few metres")]
fn a_thrusts_cover_sends_back_a_bolt_from_ahead_to_a_reflecting_user() {
    use strata_sim::combat::GuardOutcome;
    let mut app = app();
    let who = thruster(&mut app, 0., 0.);
    let shooter = actor(&mut app, 0., -12., 1);
    app.world_mut().get_mut::<Combatant>(who).unwrap().shield = 0.;
    app.world_mut()
        .get_mut::<Combatant>(who)
        .unwrap()
        .give_status("reflecting", 30.);
    let full = 100.
        + app
            .world()
            .get::<Combatant>(shooter)
            .unwrap()
            .shield_capacity;
    advance(&mut app, 20);
    app.world_mut()
        .get_mut::<CombatIntent>(who)
        .unwrap()
        .ability = Some(0);
    advance(&mut app, 16);
    let z = app.world().get::<WorldPosition>(who).unwrap().0.z();
    let mut met = Vec::new();
    bolt(
        &mut app,
        shooter,
        1,
        bevy::math::Vec3::new(0., ABOVE, (z - 1.5) as f32),
        bevy::math::Vec3::Z,
        effects_of(None, None, None, false),
    );
    for _ in 0..30 {
        advance_noting(&mut app, &mut met);
    }
    assert_eq!(met, [GuardOutcome::Blocked, GuardOutcome::Open]);
    assert!(full - pools(&app, shooter) > 5., "it went back");
}
