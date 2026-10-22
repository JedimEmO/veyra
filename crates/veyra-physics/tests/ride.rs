//! The ridden motor on synthetic ground, with no renderer: the same capsule
//! and move and slide as the walk, a different motor (ticket 0009, step 2).

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test heights and distances are small"
)]
#![allow(
    clippy::unreadable_literal,
    clippy::float_cmp,
    reason = "positions captured before the change, compared bit for bit"
)]

use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::prelude::Entity;
use bevy::math::{Vec2, Vec3};
use bevy::state::state::State;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::components::Transform;
use strata_core::WorldPos;
use strata_physics::avian3d::prelude::{Collider, RigidBody};
use strata_physics::{CharacterController, PhysicsPlugin};
use strata_registry::BiomeId;
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::{Sample, WorldSource};
use veyra_sim::ride::{RideDials, Riding};
use veyra_sim::{
    AppState, LocalPlayer, Player, PlayerIntent, ScriptedIntent, SimPlugin, WorldPosition,
    WorldSourceRes,
};

/// A world from a height function and a constant `cleared` (the road flag).
struct Synthetic<F: Fn(f64, f64) -> f32 + Send + Sync>(F, f32);

impl<F: Fn(f64, f64) -> f32 + Send + Sync> WorldSource for Synthetic<F> {
    fn sample(&self, x: f64, z: f64) -> Sample {
        Sample {
            height: (self.0)(x, z),
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: strata_worldgen::Landform {
                cleared: self.1,
                ..strata_worldgen::Landform::default()
            },
            water: None,
        }
    }

    fn sea_level(&self) -> f32 {
        0.0
    }
}

fn world(
    height: impl Fn(f64, f64) -> f32 + Send + Sync + 'static,
    cleared: f32,
) -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(height, cleared))
}

/// Gentle hills: eight metres of relief over forty-metre waves.
fn hills() -> Arc<dyn WorldSource> {
    world(
        |x, z| (8.0 * (x / 40.0).sin() * (z / 40.0).cos() + 2.0) as f32,
        0.0,
    )
}

fn stream_config() -> StreamConfig {
    StreamConfig {
        max_lod: 4,
        view_distance: 600.0,
        max_loads_per_frame: 32,
        max_unloads_per_frame: 32,
        ..StreamConfig::default()
    }
}

/// An app in the world over `source`, one sim tick per update.
fn app_over(source: Arc<dyn WorldSource>, cfg: StreamConfig) -> App {
    let mut app = App::new();
    app.insert_resource(cfg);
    app.add_plugins((
        // Avian's broad phase runs on the compute pool.
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        SimPlugin,
        StreamPlugin,
        PhysicsPlugin,
        veyra_physics::VeyraPhysicsPlugin,
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(source));
    // Avian registers its diagnostics in `Plugin::finish`, which `run`
    // calls and `update` alone does not.
    app.finish();
    app.cleanup();
    app.update();
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::InWorld
    );
    app
}

/// A local player standing (or falling) with its feet `above` metres over
/// the sampled ground at `(x, z)`.
fn spawn_player(app: &mut App, source: &dyn WorldSource, x: f64, z: f64, above: f64) -> Entity {
    let controller = CharacterController::default();
    let ground = f64::from(source.sample(x, z).height);
    let center = ground + above + f64::from(controller.half_height());
    app.world_mut()
        .spawn((
            Player::default(),
            LocalPlayer,
            controller,
            WorldPosition::new(x, center, z),
        ))
        .id()
}

fn position(app: &App, e: Entity) -> WorldPos {
    app.world().get::<WorldPosition>(e).unwrap().0
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
    }
}

/// A fixed script of intents over 200 ticks: walk, turn and sprint, jump,
/// stand, strafe.
fn foot_script(app: &mut App, player: Entity) -> Vec<WorldPos> {
    let mut seen = Vec::new();
    for tick in 0..200_usize {
        let (move_dir, yaw, jump, sprint) = match tick / 40 {
            0 => (Vec2::Y, 0.3, false, false),
            1 => (Vec2::new(0.6, 0.8), 1.1, false, true),
            2 => (Vec2::Y, 2.0, tick % 7 == 0, false),
            3 => (Vec2::ZERO, 2.0, false, false),
            _ => (Vec2::new(-1.0, 0.0), -0.7, false, true),
        };
        let intent = PlayerIntent {
            move_dir,
            yaw,
            jump,
            sprint,
        };
        app.world_mut()
            .entity_mut(player)
            .insert(ScriptedIntent(intent));
        app.update();
        if tick % 50 == 49 {
            seen.push(position(app, player));
        }
    }
    seen
}

/// Captured on the walk before the ridden motor existed: a character
/// without `Riding` steps exactly as it did.
#[test]
fn a_walker_steps_to_the_same_place_as_before_the_bike() {
    let source = hills();
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 3.0, 4.0, 0.05);
    run(&mut app, 20);
    let seen = foot_script(&mut app, player);
    let captured = [
        (-1.7648621797561646, 2.5239310264587402, -9.623244285583496),
        (-12.089582443237305, 1.5707215070724487, -19.697372436523438),
        (
            -16.014617919921875,
            0.05405843257904053,
            -17.894351959228516,
        ),
        (-22.82375717163086, -0.7162078619003296, -23.764589309692383),
    ];
    assert_eq!(seen.len(), captured.len());
    for (got, want) in seen.iter().zip(captured) {
        assert_eq!(
            (got.x(), got.y(), got.z()),
            want,
            "bit for bit is the claim"
        );
    }
}

fn dials() -> RideDials {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .unwrap();
    let id = strata_registry::Namespaced::parse("veyra:vehicle.courier_bike").unwrap();
    RideDials::of(registries.vehicles.get(&id).unwrap())
}

/// A rider on the bike over `source` at `(x, z)`, heading `heading`,
/// settled for a second with no intent, then given `dials` and the speed.
fn rider(
    source: &Arc<dyn WorldSource>,
    x: f64,
    z: f64,
    heading: f32,
    edit: impl FnOnce(&mut Riding),
) -> (App, Entity) {
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), x, z, 0.05);
    run(&mut app, 20);
    let mut riding = Riding {
        vehicle: strata_registry::Namespaced::parse("veyra:vehicle.courier_bike").unwrap(),
        dials: dials(),
        heading,
        speed: 0.0,
        velocity: Vec3::ZERO,
        ended: None,
    };
    edit(&mut riding);
    riding.velocity = PlayerIntent::forward(heading) * riding.speed;
    app.world_mut().entity_mut(player).insert(riding);
    (app, player)
}

fn throttle(player: Entity, app: &mut App, yaw: f32) {
    app.world_mut()
        .entity_mut(player)
        .insert(ScriptedIntent(PlayerIntent {
            move_dir: Vec2::Y,
            yaw,
            ..Default::default()
        }));
}

fn speed_of(app: &App, e: Entity) -> f32 {
    app.world().get::<Riding>(e).unwrap().speed
}

#[test]
fn on_open_ground_the_bike_reaches_its_ground_speed_and_holds_it() {
    let ground = dials().speed.ground;
    let source = world(|_, _| 0.0, 0.0);
    let (mut app, player) = rider(&source, 0.0, 0.0, 0.0, |_| {});
    throttle(player, &mut app, 0.0);
    run(&mut app, 100);
    assert!(
        (speed_of(&app, player) - ground).abs() < 0.01,
        "{}",
        speed_of(&app, player)
    );
    let before = position(&app, player);
    run(&mut app, 40);
    let after = position(&app, player);
    assert!((speed_of(&app, player) - ground).abs() < 0.01);
    let covered = before.z() - after.z();
    assert!(
        (covered - 2.0 * f64::from(ground)).abs() < 1.0,
        "{covered} m in two seconds"
    );
}

#[test]
fn on_a_road_the_bike_reaches_the_road_speed() {
    let road = dials().speed.road;
    let source = world(|_, _| 0.0, 1.0);
    let (mut app, player) = rider(&source, 0.0, 0.0, 0.0, |_| {});
    throttle(player, &mut app, 0.0);
    run(&mut app, 120);
    assert!(
        (speed_of(&app, player) - road).abs() < 0.01,
        "{}",
        speed_of(&app, player)
    );
}

#[test]
fn at_rest_the_bike_turns_at_the_slow_rate() {
    let source = world(|_, _| 0.0, 0.0);
    let (mut app, player) = rider(&source, 0.0, 0.0, 0.0, |_| {});
    // The wish is ninety degrees to the left, +x of the camera's frame
    // turned: yaw pi/2 faces -x.
    throttle(player, &mut app, core::f32::consts::FRAC_PI_2);
    run(&mut app, 8);
    let heading = app
        .world()
        .get::<Riding>(player)
        .unwrap()
        .heading
        .to_degrees();
    // 150 deg/s falling to ~139 as the speed gathers, over 0.4 s.
    assert!((52.0..60.0).contains(&heading), "turned {heading} degrees");
}

fn ramp(deg: f64) -> Arc<dyn WorldSource> {
    let rise = deg.to_radians().tan();
    world(move |x, _| (x * rise) as f32, 0.0)
}

#[test]
fn a_slope_at_the_full_angle_costs_nothing_and_at_the_stop_angle_stops_it() {
    // Up the ramp is +x: yaw -pi/2.
    let up = -core::f32::consts::FRAC_PI_2;
    let source = ramp(14.0);
    let (mut app, player) = rider(&source, 0.0, 0.0, up, |_| {});
    throttle(player, &mut app, up);
    run(&mut app, 100);
    assert!(
        (speed_of(&app, player) - dials().speed.ground).abs() < 0.05,
        "{}",
        speed_of(&app, player)
    );

    // The bike may slide back down and put its rider off (who may then walk
    // up, a walker's slope being steeper): the claim is only that no tick
    // ridden gains ground uphill.
    let source = ramp(f64::from(dials().slope.stop_deg));
    let (mut app, player) = rider(&source, 0.0, 0.0, up, |_| {});
    let start = position(&app, player);
    throttle(player, &mut app, up);
    let mut climbed: f64 = 0.0;
    for _ in 0..100 {
        run(&mut app, 1);
        if app.world().get::<Riding>(player).is_none() {
            break;
        }
        climbed = climbed.max(position(&app, player).x() - start.x());
    }
    assert!(climbed < 0.5, "rode {climbed} m up a wall of a slope");
}

#[test]
fn a_wall_at_twenty_metres_a_second_is_a_crash_and_at_eight_is_not() {
    let source = world(|x, _| if x < 20.0 { 0.0 } else { 30.0 }, 0.0);
    let east = -core::f32::consts::FRAC_PI_2;
    let (mut app, player) = rider(&source, 0.0, 0.0, east, |r| {
        r.dials.speed.ground = 20.0;
        r.speed = 20.0;
    });
    throttle(player, &mut app, east);
    run(&mut app, 40);
    assert!(
        app.world().get::<Riding>(player).is_none(),
        "the rider was thrown"
    );
    assert!(position(&app, player).x() < 20.0);

    let (mut app, player) = rider(&source, 0.0, 0.0, east, |r| {
        r.dials.speed.ground = 8.0;
        r.speed = 8.0;
    });
    throttle(player, &mut app, east);
    run(&mut app, 80);
    let riding = app.world().get::<Riding>(player).expect("still mounted");
    assert!(riding.ended.is_none());
    assert!(
        riding.speed < 1.0,
        "pressed against the wall at {}",
        riding.speed
    );
}

#[test]
fn shallow_water_is_ridden_deeper_slows_and_the_deep_throws() {
    // The sea stands at 0: the ground at -0.3, -0.8 and -3 puts that much
    // water over the feet.
    // At -0.8 the water stands halfway from ride_m to stall_m, so the top
    // speed is halfway from the ground speed to the crawl.
    let dials = dials();
    let half = f32::midpoint(dials.speed.ground, dials.water.crawl);
    for (bed, want) in [
        (-0.3, Some(dials.speed.ground)),
        (-0.8, Some(half)),
        (-3.0, None),
    ] {
        let source = world(move |_, _| bed, 0.0);
        let (mut app, player) = rider(&source, 0.0, 0.0, 0.0, |_| {});
        throttle(player, &mut app, 0.0);
        run(&mut app, 100);
        match want {
            Some(top) => {
                let speed = speed_of(&app, player);
                // The feet hover a skin above the bed, so the wading is a
                // hair under: allow 2.5 cm of depth, in metres a second.
                let per_m = (dials.speed.ground - dials.water.crawl)
                    / (dials.water.stall_m - dials.water.ride_m);
                assert!((speed - top).abs() < 0.025 * per_m, "bed {bed}: {speed}");
            }
            None => assert!(
                app.world().get::<Riding>(player).is_none(),
                "bed {bed}: still mounted"
            ),
        }
    }
}

#[test]
fn a_hit_at_the_ground_speed_throws_at_every_phase_of_the_tick() {
    // The wall stands within one tick's travel (0.3 m at 18 m/s) of the
    // path's end; a loss judged on displacement would miss some phases.
    let east = -core::f32::consts::FRAC_PI_2;
    for step in 0..12_u32 {
        let wall_x = 20.0 + f64::from(step) * 0.025;
        let source = world(move |x, _| if x < wall_x { 0.0 } else { 30.0 }, 0.0);
        let (mut app, player) = rider(&source, 0.0, 0.0, east, |r| r.speed = 18.0);
        throttle(player, &mut app, east);
        run(&mut app, 60);
        assert!(
            app.world().get::<Riding>(player).is_none(),
            "wall at {wall_x}: the rider was not thrown"
        );
    }
}

#[test]
fn a_trunk_head_on_at_speed_throws_the_rider() {
    let east = -core::f32::consts::FRAC_PI_2;
    for step in 0..6_u32 {
        let source = world(|_, _| 0.0, 0.0);
        let (mut app, player) = rider(&source, 0.0, 0.0, east, |r| r.speed = 18.0);
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::cylinder(0.45, 8.0),
            WorldPosition::new(20.0 + f64::from(step) * 0.05, 4.0, 0.0),
            Transform::default(),
        ));
        run(&mut app, 2);
        throttle(player, &mut app, east);
        run(&mut app, 60);
        assert!(
            app.world().get::<Riding>(player).is_none(),
            "trunk {step}: the rider was not thrown"
        );
        assert!(position(&app, player).x() < 20.0);
    }
}

#[test]
fn a_dismount_drops_the_bikes_velocity() {
    let source = world(|_, _| 0.0, 0.0);
    let (mut app, player) = rider(&source, 0.0, 0.0, 0.0, |r| r.speed = 20.0);
    throttle(player, &mut app, 0.0);
    run(&mut app, 10);
    app.world_mut().entity_mut(player).remove::<Riding>();
    app.world_mut()
        .entity_mut(player)
        .insert(ScriptedIntent(PlayerIntent::default()));
    run(&mut app, 2);
    let before = position(&app, player);
    run(&mut app, 20);
    let slid = (position(&app, player).z() - before.z()).abs();
    assert!(slid < 0.5, "the walker slid {slid} m on the bike's speed");
}

#[test]
fn ground_steeper_than_the_stop_angle_is_a_wall_even_diagonally() {
    let rise = 40.0_f64.to_radians().tan();
    let source = world(move |x, _| ((x - 10.0).max(0.0) * rise) as f32, 0.0);
    // Heading 70 degrees off the fall line, up the ramp's +x side.
    let heading = -20.0_f32.to_radians();
    let (mut app, player) = rider(&source, 0.0, 0.0, heading, |r| r.speed = 18.0);
    let start = position(&app, player);
    throttle(player, &mut app, heading);
    run(&mut app, 60);
    let climbed = position(&app, player).y() - start.y();
    assert!(climbed < 0.6, "climbed {climbed} m of a 40 degree face");
}
