//! The character on synthetic ground, with no renderer and no GPU: the
//! streamer loads regions around the player, the colliders are built from
//! the synthetic world, and the character walks on them.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test heights and distances are small"
)]

use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::prelude::{Entity, With};
use bevy::math::Vec3;
use bevy::state::state::State;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::components::Transform;
use strata_core::WorldPos;
use strata_physics::avian3d::prelude::{Collider, Position, RigidBody};
use strata_physics::{CharacterController, PhysicsPlugin, TerrainCollider, TerrainColliderStats};
use strata_registry::BiomeId;
use strata_sim::{
    AppState, LocalPlayer, Player, PlayerIntent, RenderOrigin, ScriptedIntent, SimPlugin,
    WorldPosition, WorldSourceRes,
};
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::{Sample, WorldSource};

/// A world from a height function.
struct Synthetic<F: Fn(f64, f64) -> f32 + Send + Sync>(F);

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
            landform: strata_worldgen::Landform::default(),
            water: None,
        }
    }

    fn sea_level(&self) -> f32 {
        0.0
    }
}

/// Gentle hills: eight metres of relief over forty-metre waves, slopes
/// under twelve degrees.
fn hills() -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(|x: f64, z: f64| {
        (8.0 * (x / 40.0).sin() * (z / 40.0).cos() + 2.0) as f32
    }))
}

/// A flat floor with a thirty-metre wall at `x = 20`.
fn cliff() -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(
        |x: f64, _z: f64| if x < 20.0 { 0.0 } else { 30.0 },
    ))
}

/// A bowl: a metre high at the origin, rising gently all around, so a
/// ball dropped near the middle rolls to it and stops.
fn bowl() -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(|x: f64, z: f64| {
        ((x * x + z * z) / 400.0 + 1.0) as f32
    }))
}

/// A shore: three metres of dry land west of `x = 0`, five metres of water
/// east of it.
fn shore() -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(
        |x: f64, _z: f64| if x < 0.0 { 3.0 } else { -5.0 },
    ))
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

fn walking(yaw: f32) -> ScriptedIntent {
    ScriptedIntent(PlayerIntent {
        move_dir: bevy::math::Vec2::Y,
        yaw,
        jump: false,
        sprint: false,
    })
}

fn position(app: &App, e: Entity) -> WorldPos {
    app.world().get::<WorldPosition>(e).unwrap().0
}

fn controller(app: &App, e: Entity) -> CharacterController {
    app.world().get::<CharacterController>(e).unwrap().clone()
}

fn feet_height(app: &App, e: Entity) -> f64 {
    position(app, e).y() - f64::from(controller(app, e).half_height())
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
    }
}

#[test]
fn a_dropped_player_lands_on_the_sampled_height() {
    let source = hills();
    let mut app = app_over(source.clone(), stream_config());
    let (x, z) = (37.0, 51.0);
    let player = spawn_player(&mut app, source.as_ref(), x, z, 100.0);
    run(&mut app, 400);
    let expected = f64::from(source.sample(x, z).height);
    let feet = feet_height(&app, player);
    assert!(
        (feet - expected).abs() < 0.2,
        "feet at {feet}, ground at {expected}"
    );
    assert!(controller(&app, player).grounded);
    let at = position(&app, player);
    assert!(
        (at.x() - x).abs() < 0.01 && (at.z() - z).abs() < 0.01,
        "{at:?}"
    );
    assert!(app.world().resource::<TerrainColliderStats>().loaded > 0);
}

#[test]
fn a_walking_player_stays_on_the_ground() {
    let source = hills();
    let mut app = app_over(source.clone(), stream_config());
    let (x, z) = (10.0, 10.0);
    let player = spawn_player(&mut app, source.as_ref(), x, z, 0.5);
    app.world_mut().entity_mut(player).insert(walking(0.0));
    run(&mut app, 40);
    let start = position(&app, player);
    for tick in 0..200 {
        app.update();
        let at = position(&app, player);
        let ground = f64::from(source.sample(at.x(), at.z()).height);
        let feet = feet_height(&app, player);
        assert!(
            (feet - ground).abs() < 0.35,
            "tick {tick}: feet at {feet}, ground at {ground}, at {at:?}"
        );
        assert!(controller(&app, player).grounded, "tick {tick}: airborne");
    }
    let end = position(&app, player);
    // Ten seconds at five metres per second, north (`-z`), less the slope.
    let walked = start.z() - end.z();
    assert!((38.0..=52.0).contains(&walked), "walked {walked} m");
    assert!((end.x() - start.x()).abs() < 1.0);
}

#[test]
fn a_wall_steeper_than_the_slope_limit_stops_the_player() {
    let source = cliff();
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 10.0, 5.0, 0.5);
    // Yaw such that forward is `+x`.
    app.world_mut()
        .entity_mut(player)
        .insert(walking(-core::f32::consts::FRAC_PI_2));
    run(&mut app, 120);
    let at = position(&app, player);
    assert!(at.x() < 19.9, "walked into the wall: {at:?}");
    assert!(at.x() > 16.0, "never reached the wall: {at:?}");
    let feet = feet_height(&app, player);
    assert!(feet < 0.6, "climbed the wall: feet at {feet}");
    assert!(controller(&app, player).grounded);
}

#[test]
fn a_jump_leaves_the_ground_and_comes_back() {
    let source = hills();
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 5.0, 5.0, 0.2);
    run(&mut app, 40);
    let resting = feet_height(&app, player);
    app.world_mut()
        .get_mut::<PlayerIntent>(player)
        .unwrap()
        .jump = true;
    let mut peak = resting;
    let mut left_ground = false;
    for _ in 0..40 {
        app.update();
        peak = peak.max(feet_height(&app, player));
        left_ground |= !controller(&app, player).grounded;
    }
    assert!(left_ground);
    assert!(peak - resting > 0.4, "peak {peak} from {resting}");
    run(&mut app, 40);
    assert!(controller(&app, player).grounded);
    assert!((feet_height(&app, player) - resting).abs() < 0.2);
}

#[test]
fn a_player_walking_into_the_sea_floats() {
    let source = shore();
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), -10.0, 0.0, 0.2);
    app.world_mut()
        .entity_mut(player)
        .insert(walking(-core::f32::consts::FRAC_PI_2));
    run(&mut app, 200);
    let at = position(&app, player);
    let ctl = controller(&app, player);
    assert!(at.x() > 5.0, "still on land: {at:?}");
    assert!(ctl.swimming, "not swimming at {at:?}");
    assert!(!ctl.grounded);
    // The centre floats a metre under the surface, well above the floor.
    assert!((at.y() + f64::from(ctl.float_depth)).abs() < 0.15, "{at:?}");
}

#[test]
fn a_rebase_moves_every_body_by_the_origin_delta() {
    let source = hills();
    let mut app = app_over(
        source.clone(),
        StreamConfig {
            max_unloads_per_frame: 4,
            ..stream_config()
        },
    );
    let player = spawn_player(&mut app, source.as_ref(), 5_000.0, 0.0, 0.5);
    app.update();
    let origin = app.world().resource::<RenderOrigin>().0;
    assert!((origin.x() - 5_000.0).abs() < 1e-9, "no rebase: {origin:?}");
    // Rewritten the same frame the origin moved, not by the next tick.
    let physics = app.world().get::<Position>(player).unwrap().0;
    assert_eq!(physics, Vec3::ZERO);

    run(&mut app, 30);
    let colliders_before: Vec<(Entity, Vec3)> = {
        let mut q = app
            .world_mut()
            .query_filtered::<(Entity, &Position), With<TerrainCollider>>();
        q.iter(app.world()).map(|(e, p)| (e, p.0)).collect()
    };
    assert!(colliders_before.len() > 4, "{}", colliders_before.len());
    let from = app.world().resource::<RenderOrigin>().0;

    // Teleport past the threshold, onto a hilltop (the spawn was in the
    // sea, where the character floats rather than stands), half a metre up.
    let x = 5_000.0 + 3_105.3;
    let hilltop = f64::from(source.sample(x, 0.0).height);
    assert!(hilltop > 9.0, "not a hilltop: {hilltop}");
    let far = WorldPos::new(
        x,
        hilltop + 0.5 + f64::from(controller(&app, player).half_height()),
        0.0,
    );
    app.world_mut().get_mut::<WorldPosition>(player).unwrap().0 = far;
    app.update();
    let to = app.world().resource::<RenderOrigin>().0;
    let delta = to - from;
    assert!(delta.length() > 2_900.0, "no rebase: {delta:?}");
    let origin = *app.world().resource::<RenderOrigin>();
    let mut checked = 0;
    for (entity, before) in colliders_before {
        let Some(after) = app.world().get::<Position>(entity) else {
            continue; // unloaded this frame
        };
        let world = app.world().get::<WorldPosition>(entity).unwrap().0;
        assert_eq!(after.0, origin.to_render(world));
        let moved = before - after.0;
        assert!(
            (moved - delta.as_vec3()).length() < 0.01,
            "{entity}: moved {moved:?}, origin moved {delta:?}"
        );
        checked += 1;
    }
    assert!(checked > 0, "every collider was unloaded before the check");
    assert_eq!(app.world().get::<Position>(player).unwrap().0, Vec3::ZERO);

    // The character steps on in the new frame: the ground streams in
    // around the teleport, it lands on the sampled height and walks off
    // grounded, so the rebased colliders are the ones the sweeps see.
    run(&mut app, 200);
    let at = position(&app, player);
    let ground = f64::from(source.sample(at.x(), at.z()).height);
    let feet = feet_height(&app, player);
    assert!(
        (feet - ground).abs() < 0.2,
        "after the rebase: feet at {feet}, ground at {ground}, at {at:?}"
    );
    assert!(
        controller(&app, player).grounded,
        "airborne after the rebase"
    );
    app.world_mut().entity_mut(player).insert(walking(0.0));
    let start = at;
    for tick in 0..40 {
        app.update();
        let at = position(&app, player);
        let ground = f64::from(source.sample(at.x(), at.z()).height);
        let feet = feet_height(&app, player);
        assert!(
            (feet - ground).abs() < 0.35,
            "tick {tick} after the rebase: feet at {feet}, ground at {ground}"
        );
        assert!(controller(&app, player).grounded, "tick {tick}: airborne");
    }
    let walked = start.z() - position(&app, player).z();
    assert!(
        (7.0..=11.0).contains(&walked),
        "walked {walked} m after the rebase"
    );
}

#[test]
fn a_dynamic_body_writes_its_world_position_back() {
    let source = bowl();
    let mut app = app_over(source.clone(), stream_config());
    // Somebody has to observe for the ground to stream in; the ball is not
    // an observer, the player standing seven metres off is.
    spawn_player(&mut app, source.as_ref(), 5.0, 5.0, 0.2);
    run(&mut app, 40);
    let ball = app
        .world_mut()
        .spawn((
            RigidBody::Dynamic,
            Collider::sphere(0.5),
            WorldPosition::new(2.0, 11.0, 0.0),
            Transform::default(),
        ))
        .id();
    run(&mut app, 10);
    let origin = *app.world().resource::<RenderOrigin>();
    let falling = position(&app, ball);
    assert!(falling.y() < 10.9, "the ball is falling: {falling:?}");
    let physics = app.world().get::<Position>(ball).unwrap().0;
    assert!(
        falling.distance(origin.to_world(physics)) < 1e-3,
        "world {falling:?} follows physics {physics:?}"
    );
    run(&mut app, 300);
    let at = position(&app, ball);
    let physics = app.world().get::<Position>(ball).unwrap().0;
    assert!(
        at.distance(origin.to_world(physics)) < 1e-3,
        "world {at:?} follows physics {physics:?}"
    );
    // At rest near the bottom of the bowl, its radius over the ground.
    let ground = f64::from(source.sample(at.x(), at.z()).height);
    assert!(
        (at.y() - ground - 0.5).abs() < 0.3,
        "{at:?}, ground {ground}"
    );
    assert!(at.x().hypot(at.z()) < 3.0, "rolled off: {at:?}");
}

#[test]
fn colliders_follow_the_streamer_in_and_out() {
    let source = hills();
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 0.0, 0.0, 0.5);
    // Collider tasks run on the async pool, which a busy test run shares:
    // wait for them by condition, with a bound, rather than a frame count.
    for _ in 0..40 {
        run(&mut app, 5);
        let stats = *app.world().resource::<TerrainColliderStats>();
        if stats.loaded > 0 && stats.built > 0 {
            break;
        }
    }
    let stats = *app.world().resource::<TerrainColliderStats>();
    assert!(stats.loaded > 0 && stats.built > 0, "{stats:?}");
    app.world_mut().get_mut::<WorldPosition>(player).unwrap().0 =
        WorldPos::new(100_000.0, 10.0, 0.0);
    run(&mut app, 60);
    let stats = *app.world().resource::<TerrainColliderStats>();
    let remaining = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<TerrainCollider>>();
        q.iter(app.world()).count()
    };
    assert_eq!(stats.loaded, remaining);
    // The old ground is gone; the new ground is loaded around the player.
    let mut q = app.world_mut().query::<&TerrainCollider>();
    assert!(
        q.iter(app.world())
            .all(|c| c.coord.min_corner().x() > 90_000.0),
        "old colliders linger"
    );
}

/// A species table with one tree, ten metres apart, on any ground.
fn one_tree_species() -> strata_flora::FloraTable {
    use strata_flora::Species;
    use strata_registry::{FloraFit, FloraId, FloraKind, MetreRange, TreeParams};
    strata_flora::FloraTable::from_species(vec![Species {
        id: FloraId(0),
        kind: FloraKind::Tree,
        fit: FloraFit::from_weights(vec![1.0]),
        spacing_m: 10.0,
        density_per_m2: 0.0,
        slope_max: 1.0,
        altitude_m: (-100.0, 10_000.0),
        sea_clearance_m: 0.5,
        moisture: (0.0, 1.0),
        // A full floor, so the synthetic world's cover does not thin the
        // grid these tests count colliders on.
        cover_floor: 1.0,
        size: (1.0, 1.0),
        exclusion_m: 3.0,
        variants: 2,
        tree: TreeParams {
            height_m: MetreRange::new(8.0, 8.0),
            trunk_ratio: 0.05,
            ..TreeParams::default()
        },
        species_seed: 7,
        // Baked trunks, as the tree library gives them: the collider comes
        // from the variant, not from the parameters above.
        colliders: vec![
            strata_flora::Collider::Cylinder(strata_flora::Trunk {
                radius: 0.45,
                height: 4.8,
            }),
            strata_flora::Collider::Cylinder(strata_flora::Trunk {
                radius: 0.5,
                height: 5.2,
            }),
        ],
        ground: strata_registry::GroundPrefs::default(),
        cluster: strata_registry::Clustering::default(),
        children: Vec::new(),
        dressing: false,
    }])
}

/// A flat floor two metres up.
fn flat() -> Arc<dyn WorldSource> {
    Arc::new(Synthetic(|_x: f64, _z: f64| 2.0))
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one scenario: the colliders, then the walker, over one app"
)]
fn trees_get_trunk_colliders_where_placement_puts_them_and_stop_a_walker() {
    use strata_core::{RegionCoord, Seed};
    use strata_physics::{FloraPlacement, TreeCollider};

    let source = flat();
    let table = one_tree_species();
    let mut app = App::new();
    app.insert_resource(stream_config());
    app.insert_resource(FloraPlacement(Arc::new(table.clone())));
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        SimPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(source.clone()));
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    let seed = app
        .world()
        .get_resource::<strata_sim::WorldSeed>()
        .map_or(Seed(0), |s| s.0);

    // The cell the player will stand in, placed the way the server does.
    let cell = RegionCoord::new(0, 0, 0);
    let expected =
        strata_flora::place_cell_with(cell, source.as_ref(), &table, seed, &mut Vec::new(), false);
    assert!(
        expected.plants.len() > 10,
        "{} trees in the cell",
        expected.plants.len()
    );

    let player = spawn_player(&mut app, source.as_ref(), 32.0, 32.0, 0.2);
    run(&mut app, 60);
    let colliders: Vec<(TreeCollider, WorldPos)> = app
        .world_mut()
        .query::<(&TreeCollider, &WorldPosition)>()
        .iter(app.world())
        .filter(|(c, _)| c.cell == cell)
        .map(|(c, p)| (*c, p.0))
        .collect();
    assert_eq!(
        colliders.len(),
        expected.plants.len(),
        "one trunk per placed tree"
    );
    let corner = cell.min_corner();
    for plant in &expected.plants {
        let base = WorldPos::new(
            corner.x() + f64::from(plant.x),
            f64::from(plant.y),
            corner.z() + f64::from(plant.z),
        );
        assert!(
            colliders.iter().any(|(collider, at)| {
                collider.id == plant.id
                    && (at.x() - base.x()).abs() < 1e-3
                    && (at.z() - base.z()).abs() < 1e-3
                    && at.y() > base.y()
            }),
            "no trunk at {base:?}"
        );
    }
    assert!(app.world().resource::<TerrainColliderStats>().trees >= expected.plants.len());

    // Walk north (-z) from directly south of the tree nearest the middle
    // of the cell; the trunk stops the player short of its axis.
    let target = expected
        .plants
        .iter()
        .min_by(|a, b| {
            let da = (a.x - 32.0).powi(2) + (a.z - 32.0).powi(2);
            let db = (b.x - 32.0).powi(2) + (b.z - 32.0).powi(2);
            da.total_cmp(&db)
        })
        .unwrap();
    let (tx, tz) = (
        corner.x() + f64::from(target.x),
        corner.z() + f64::from(target.z),
    );
    // The trunk is a body: a ray from above lands on its top.
    {
        use bevy::ecs::system::RunSystemOnce;
        use strata_physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
        let origin = app.world().resource::<RenderOrigin>().0;
        let rel = WorldPos::new(tx, 20.0, tz).relative_to(origin);
        let start = Vec3::new(rel.x as f32, rel.y as f32, rel.z as f32);
        let hit = app
            .world_mut()
            .run_system_once(move |sq: SpatialQuery| {
                sq.cast_ray(
                    start,
                    bevy::math::Dir3::NEG_Y,
                    100.0,
                    true,
                    &SpatialQueryFilter::default(),
                )
                .map(|h| h.distance)
            })
            .unwrap()
            .unwrap();
        let (radius, height) = table.species()[0].trunk(target).unwrap();
        assert!(
            (20.0 - hit - (target.y + height)).abs() < 0.05,
            "the ray landed {hit} m down, the trunk top is at {}",
            target.y + height
        );
        assert!(radius > 0.3, "{radius}");
    }
    // Walk north (-z) at the trunk: a round trunk deflects the capsule and
    // it slides around, never inside the trunk's footprint.
    // A glancing trajectory exercises sliding. An exactly centered capsule
    // can validly stop against the cylinder without choosing either side.
    let walker = spawn_player(&mut app, source.as_ref(), tx + 0.2, tz + 6.0, 0.2);
    app.world_mut().entity_mut(walker).insert(walking(0.0));
    let radius = f64::from(table.species()[0].trunk(target).unwrap().0);
    let mut closest = f64::MAX;
    let mut deflected: f64 = 0.0;
    for _ in 0..100 {
        app.update();
        let p = position(&app, walker);
        closest = closest.min(((p.x() - tx).powi(2) + (p.z() - tz).powi(2)).sqrt());
        deflected = deflected.max((p.x() - tx).abs());
        assert!(
            controller(&app, walker).grounded,
            "left the ground at {p:?}"
        );
    }
    let at = position(&app, walker);
    assert!(
        closest >= radius,
        "the capsule's centre came within {closest} m of the axis, radius {radius}"
    );
    assert!(
        deflected > 0.5,
        "walked straight through: deflected {deflected} m, ended at {at:?}"
    );
    assert!(
        at.z() < tz,
        "never got past the trunk: {at:?} vs tree at z {tz}"
    );
    let _ = player;
}

/// The bodies of every lod 0 cell that has any: whether its ground exists,
/// and its trunks by plant.
fn built_cells(
    app: &mut App,
) -> std::collections::HashMap<strata_core::RegionCoord, (bool, usize)> {
    use strata_physics::TreeCollider;
    let mut cells = std::collections::HashMap::new();
    let world = app.world_mut();
    for ground in world.query::<&TerrainCollider>().iter(world) {
        cells.entry(ground.coord).or_insert((false, 0)).0 = true;
    }
    for trunk in world.query::<&TreeCollider>().iter(world) {
        cells.entry(trunk.cell).or_insert((false, 0)).1 += 1;
    }
    cells
}

/// The lod 0 cells whose square is within `margin` metres of `at`.
fn cells_around(at: WorldPos, margin: f64) -> Vec<strata_core::RegionCoord> {
    use strata_core::RegionCoord;
    let size = RegionCoord::size_at(0);
    let lo = |v: f64| ((v - margin) / size).floor() as i32;
    let hi = |v: f64| ((v + margin) / size).floor() as i32;
    let mut cells = Vec::new();
    for x in lo(at.x())..=hi(at.x()) {
        for z in lo(at.z())..=hi(at.z()) {
            cells.push(RegionCoord::new(x, z, 0));
        }
    }
    cells
}

/// The collectors' rule that a character never stands where its bodies are
/// pending: with no time budget at all, so every region out of a
/// character's reach waits behind one body a frame, a sprinter crosses a
/// forest of regions, and the ground and every trunk of each cell it
/// touches are there on every tick it touches it. At least one of those
/// cells must have been seen pending before the sprinter got to it, or the
/// test proved nothing.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one scenario: the fill, then the sprint checked on every tick"
)]
fn a_character_never_reaches_a_region_whose_bodies_are_pending() {
    use strata_core::Seed;
    use strata_physics::FloraPlacement;
    use strata_physics::terrain::ColliderBudget;

    let source = flat();
    // Trees sixteen metres apart: a forest a sprinter can cross.
    let mut species = one_tree_species().species().to_vec();
    species[0].spacing_m = 16.0;
    let table = strata_flora::FloraTable::from_species(species);
    let mut app = App::new();
    app.insert_resource(stream_config());
    app.insert_resource(FloraPlacement(Arc::new(table.clone())));
    app.insert_resource(ColliderBudget {
        time: core::time::Duration::ZERO,
        ..ColliderBudget::default()
    });
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        SimPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(source.clone()));
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    let seed = app
        .world()
        .get_resource::<strata_sim::WorldSeed>()
        .map_or(Seed(0), |s| s.0);
    let mut expected = std::collections::HashMap::new();
    let mut trunks_in = |cell: strata_core::RegionCoord| {
        *expected.entry(cell).or_insert_with(|| {
            strata_flora::place_cell_with(
                cell,
                source.as_ref(),
                &table,
                seed,
                &mut Vec::new(),
                false,
            )
            .plants
            .len()
        })
    };

    let player = spawn_player(&mut app, source.as_ref(), 20.0, 24.0, 0.2);
    // Its own cell is built the frame its task finishes; tasks run on the
    // shared pool, so wait by condition.
    for _ in 0..200 {
        app.update();
        if built_cells(&mut app)
            .get(&strata_core::RegionCoord::new(0, 0, 0))
            .is_some_and(|c| c.0)
        {
            break;
        }
    }
    // East, sprinting.
    app.world_mut()
        .entity_mut(player)
        .insert(ScriptedIntent(PlayerIntent {
            move_dir: bevy::math::Vec2::Y,
            yaw: -core::f32::consts::FRAC_PI_2,
            jump: false,
            sprint: true,
        }));
    let mut seen_pending = std::collections::HashSet::new();
    let mut entered_pending = 0;
    let mut touched = std::collections::HashSet::new();
    let mut waited = false;
    for tick in 0..420 {
        app.update();
        let at = position(&app, player);
        assert!(
            controller(&app, player).grounded,
            "tick {tick}: airborne at {at:?}"
        );
        let cells = built_cells(&mut app);
        waited |= app.world().resource::<TerrainColliderStats>().waiting > 0;
        for cell in cells_around(at, 1.0) {
            let (ground, trunks) = cells.get(&cell).copied().unwrap_or((false, 0));
            let want = trunks_in(cell);
            assert!(
                ground && trunks == want,
                "tick {tick}: at {at:?}, cell {cell:?} has ground {ground} and {trunks} of {want} trunks"
            );
            if touched.insert(cell) && seen_pending.contains(&cell) {
                entered_pending += 1;
            }
        }
        // Cells ahead, all lod 0 at this distance, without their ground or
        // some of their trunks: pending on the pool or behind the budget.
        for cell in cells_around(at, 120.0) {
            let (ground, trunks) = cells.get(&cell).copied().unwrap_or((false, 0));
            if !ground || trunks < trunks_in(cell) {
                seen_pending.insert(cell);
            }
        }
    }
    let at = position(&app, player);
    assert!(at.x() > 150.0, "the sprinter got stuck at {at:?}");
    assert!(waited, "the budget never held a region back");
    assert!(
        entered_pending > 0,
        "no cell the sprinter reached was ever seen half built; touched {touched:?}"
    );
}

#[test]
fn fitted_building_ramps_reach_threshold_and_survive_rebase() {
    for corporate in [false, true] {
        fitted_ramp(corporate);
    }
}

/// The spec, the policy and the collision proxies the elements declare, derived
/// from the same evaluated solids the game meshes.
fn fitted_content(
    corporate: bool,
) -> (
    ashlar::GroundingSpec,
    ashlar::GroundingPolicy,
    Vec<ashlar::ConvexSolid>,
) {
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    let (spec, policy, building) = if corporate {
        (
            veyra_buildings::corporate::grounding_spec(),
            veyra_buildings::corporate::grounding_policy(),
            veyra_buildings::corporate::building(4).unwrap(),
        )
    } else {
        (
            veyra_buildings::grounding_spec(),
            veyra_buildings::grounding_policy(),
            veyra_buildings::building(veyra_buildings::Scene::Outpost).unwrap(),
        )
    };
    let solids = mesh_building(&building, &ManifoldMesher::default())
        .unwrap()
        .colliders()
        .into_iter()
        .map(|placed| placed.solid)
        .collect();
    (spec, policy, solids)
}

fn fitted_ramp(corporate: bool) {
    use ashlar::{ConvexSolid, fit_ground};
    use strata_physics::avian3d::prelude::Rotation;
    use strata_physics::grounding::{collider, sample_patch};
    let source: Arc<dyn WorldSource> = Arc::new(Synthetic(|_, z: f64| (10.0 + z * 0.05) as f32));
    let (spec, policy, mut solids) = fitted_content(corporate);
    let threshold = if corporate { 0.13 } else { 0.1 };
    let fit = fit_ground(
        &spec,
        policy,
        &sample_patch(source.as_ref(), [0.0, 0.0]).unwrap(),
        [0.0, 0.0],
        0.65,
    )
    .unwrap();
    solids.extend(fit.solids.iter().map(ConvexSolid::from));
    let mut app = app_over(source.clone(), stream_config());
    let base = WorldPos::new(
        fit.pose.translation.x,
        fit.pose.translation.y,
        fit.pose.translation.z,
    );
    let body = app
        .world_mut()
        .spawn((
            RigidBody::Static,
            collider(&solids).unwrap(),
            WorldPosition(base),
            Position(fit.pose.translation.as_vec3()),
            Rotation(fit.pose.rotation.as_quat()),
            Transform::default(),
        ))
        .id();
    let visual = app
        .world_mut()
        .spawn((
            WorldPosition(base),
            Transform::from_rotation(fit.pose.rotation.as_quat()),
        ))
        .id();
    let start = fit.toe + fit.pose.rotation * bevy::math::DVec3::new(0.0, 0.0, -1.0);
    let player = spawn_player(&mut app, source.as_ref(), start.x, start.z, 0.1);
    app.world_mut()
        .entity_mut(player)
        .insert(walking(0.65 + std::f32::consts::PI));
    run(&mut app, 80);
    let at = position(&app, player);
    let local = fit.pose.rotation.inverse()
        * (bevy::math::DVec3::new(at.x(), at.y(), at.z()) - fit.pose.translation);
    // The doorway is an opening cut inside the entrance panel, and a derived
    // proxy is convex, so collision seals it (ashlar ADR 0002). The walker
    // climbs the ramp and stands on the threshold against the closed face,
    // half its own width outside the plane of the wall.
    assert!(
        local.z > -0.6 && local.z < 0.0,
        "did not reach closed door threshold: {local:?}"
    );
    assert!(
        (feet_height(&app, player) - (base.y() + threshold)).abs() < 0.12,
        "did not stand on threshold"
    );
    assert!(controller(&app, player).grounded);
    // The far observer is the one primary: a player is one too (its
    // required component), and with two the origin followed whichever the
    // query met last, which a new resource's archetype reordered.
    app.world_mut()
        .entity_mut(player)
        .remove::<(ScriptedIntent, strata_sim::PrimaryObserver)>();
    let observer = app
        .world_mut()
        .spawn((
            strata_sim::WorldObserver,
            strata_sim::PrimaryObserver,
            WorldPosition::new(5000.0, 20.0, 0.0),
        ))
        .id();
    app.update();
    let origin = *app.world().resource::<RenderOrigin>();
    assert!(origin.0.x() > 4000.0);
    assert_eq!(
        app.world().get::<Position>(body).unwrap().0,
        origin.to_render(base)
    );
    assert_eq!(
        app.world().get::<Transform>(visual).unwrap().translation,
        origin.to_render(base)
    );
    app.world_mut().despawn(observer);
    run(&mut app, 10);
    assert!((feet_height(&app, player) - (base.y() + threshold)).abs() < 0.12);
}

/// A river along `x`: a channel `half` metres either side of `z = 0`, its
/// bed `depth` under a surface ten metres up at `x = 0` that falls `drop`
/// metres a metre downstream, flowing `+x` at `speed`; the ground beyond
/// the channel stands `beyond` metres over the surface (under it, a
/// floodplain). Every sample carries the river's record, as one in its
/// valley does. The sea is far below, out of the way.
struct River {
    depth: f32,
    half: f32,
    speed: f32,
    kind: strata_worldgen::hydrology::WaterKind,
    drop: f32,
    beyond: f32,
}

impl River {
    fn channel(depth: f32, speed: f32) -> Self {
        Self {
            depth,
            half: 15.0,
            speed,
            kind: strata_worldgen::hydrology::WaterKind::Channel,
            drop: 0.0,
            beyond: 0.5,
        }
    }
}

impl WorldSource for River {
    fn sample(&self, x: f64, z: f64) -> Sample {
        let across = z as f32;
        let surface = 10.0 - self.drop * x as f32;
        let height = if across.abs() < self.half {
            surface - self.depth
        } else {
            surface + self.beyond
        };
        Sample {
            height,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: strata_worldgen::Landform::default(),
            water: Some(strata_worldgen::SampleWater {
                surface_m: surface,
                bed_m: surface - self.depth,
                width_m: 2.0 * self.half,
                signed_distance_m: across,
                flow: [self.speed, 0.0],
                discharge_m3s: 2.0,
                order: 2,
                kind: self.kind,
                cut_dm: 0,
            }),
        }
    }

    fn sea_level(&self) -> f32 {
        -1_000.0
    }
}

fn river(depth: f32, speed: f32) -> Arc<dyn WorldSource> {
    Arc::new(River::channel(depth, speed))
}

/// Metres walked north (`-z`) in a second from the point, after a second
/// to settle, and where the walker ended.
fn walk_a_second(source: &Arc<dyn WorldSource>, z: f64) -> (f64, WorldPos) {
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 0.0, z, 0.05);
    run(&mut app, 20);
    let start = position(&app, player);
    app.world_mut().entity_mut(player).insert(walking(0.0));
    run(&mut app, 20);
    let end = position(&app, player);
    (start.z() - end.z(), end)
}

/// Heading upstream (`-x`), sprinting or not.
fn upstream(sprint: bool) -> ScriptedIntent {
    ScriptedIntent(PlayerIntent {
        move_dir: bevy::math::Vec2::Y,
        yaw: core::f32::consts::FRAC_PI_2,
        jump: false,
        sprint,
    })
}

#[test]
fn a_walker_in_a_shallow_channel_is_slowed_by_its_depth() {
    let (dry, _) = walk_a_second(&river(0.5, 0.0), 30.0);
    let (shallow, at) = walk_a_second(&river(0.5, 0.0), 5.0);
    let (deeper, _) = walk_a_second(&river(0.75, 0.0), 5.0);
    assert!((4.5..=5.3).contains(&dry), "dry ground walked {dry} m");
    // Half a metre over the feet is six tenths of the way to the swim's
    // half speed at the waist (0.85 m): 0.71 of the walk.
    assert!(
        (3.3..=3.8).contains(&shallow) && shallow < dry - 0.4,
        "waded {shallow} m against {dry} on the bank"
    );
    assert!(
        deeper < shallow - 0.4,
        "{deeper} m deeper, {shallow} shallower"
    );
    let afloat = controller_at(&river(0.5, 0.0), at);
    assert!(!afloat, "half a metre of water floated the walker");
}

/// Whether a character dropped at `at` over `source` is swimming after a
/// settle.
fn controller_at(source: &Arc<dyn WorldSource>, at: WorldPos) -> bool {
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), at.x(), at.z(), 0.05);
    run(&mut app, 20);
    controller(&app, player).swimming
}

#[test]
fn a_swimmer_in_flowing_water_drifts_downstream() {
    let source = river(3.0, 0.7);
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 0.0, 0.0, 0.2);
    run(&mut app, 40);
    let start = position(&app, player);
    assert!(controller(&app, player).swimming, "not afloat: {start:?}");
    run(&mut app, 100);
    let end = position(&app, player);
    let drift = end.x() - start.x();
    // Five seconds at eight tenths of 0.7 m/s.
    assert!((2.4..=3.2).contains(&drift), "drifted {drift} m");
    assert!((end.z() - start.z()).abs() < 0.05, "{start:?} to {end:?}");
}

/// The review of step 3 found the current counted as the body's own
/// motion held back by a wall, so a swimmer's stroke was cut to what it
/// made good against the flow and it could not swim up any river.
#[test]
fn a_swimmer_makes_way_upstream() {
    let source = river(3.0, 0.7);
    let mut app = app_over(source.clone(), stream_config());
    let player = spawn_player(&mut app, source.as_ref(), 0.0, 0.0, 0.2);
    run(&mut app, 40);
    let start = position(&app, player);
    app.world_mut().entity_mut(player).insert(upstream(false));
    run(&mut app, 60);
    let end = position(&app, player);
    let made = start.x() - end.x();
    // Three seconds at the swim's 2.5 m/s less the current's 0.56, and a
    // quarter of a second getting up to speed.
    assert!((4.6..=6.4).contains(&made), "made {made} m upstream");
    assert!(controller(&app, player).swimming);
}

/// A fall carries what is in it down and holds nothing up: a sprinting
/// swimmer cannot climb a fall's face, and on a cascade gentle enough to
/// stand on, waist-deep water carries faster than the sprint's swim.
#[test]
fn nobody_climbs_a_fall() {
    for (drop, steep) in [(1.5_f32, true), (0.5, false)] {
        let source: Arc<dyn WorldSource> = Arc::new(River {
            depth: 1.2,
            kind: strata_worldgen::hydrology::WaterKind::Fall,
            drop,
            ..River::channel(1.2, 0.7)
        });
        let mut app = app_over(source.clone(), stream_config());
        let player = spawn_player(&mut app, source.as_ref(), 0.0, 0.0, 0.05);
        run(&mut app, 4);
        let start = position(&app, player);
        app.world_mut().entity_mut(player).insert(upstream(true));
        run(&mut app, 40);
        let end = position(&app, player);
        let carried = end.x() - start.x();
        assert!(!controller(&app, player).swimming, "afloat on a fall");
        if steep {
            assert!(
                carried > 2.0 && end.y() < start.y() - 2.0,
                "{start:?} to {end:?} on the face"
            );
        } else {
            assert!(
                (0.0..=3.0).contains(&carried),
                "carried {carried} m down the cascade"
            );
        }
    }
}

/// On the bank over the water, and on a floodplain lower than the river's
/// level but past its banks, the river does nothing: no drift, a full
/// walk.
#[test]
fn past_its_banks_the_river_does_nothing() {
    for beyond in [0.5_f32, -0.3] {
        let source: Arc<dyn WorldSource> = Arc::new(River {
            beyond,
            ..River::channel(0.5, 0.7)
        });
        let mut app = app_over(source.clone(), stream_config());
        let player = spawn_player(&mut app, source.as_ref(), 0.0, 40.0, 0.05);
        run(&mut app, 20);
        let start = position(&app, player);
        run(&mut app, 60);
        let end = position(&app, player);
        assert!(
            (end.x() - start.x()).abs() < 1e-3 && (end.z() - start.z()).abs() < 1e-3,
            "{beyond}: {start:?} to {end:?}"
        );
        assert!(controller(&app, player).grounded);
        let (walked, _) = walk_a_second(&source, 40.0);
        assert!(
            (4.5..=5.3).contains(&walked),
            "{beyond}: walked {walked} m past the banks"
        );
    }
}
