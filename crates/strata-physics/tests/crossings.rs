//! Walking the crossings (ticket 0005, step 7): on the real world of seed
//! 42, with the terrain's colliders streamed round the walker and the
//! crossing's pieces baked from the kit and collided as the server does, a
//! character walks and runs across the showcase road's first bridge from one
//! shelf to the other without falling or stopping, is held on the deck by
//! its rails, and wades a track's ford near Wack's Hold.
//!
//! The three on the real world bake a mass's rivers, mesh the kit's pieces
//! and stream terrain colliders, about a minute each in a debug build, so
//! they are ignored in `just test` and run by hand, in release:
//!
//! ```text
//! cargo test --release -p strata-physics --test crossings -- --ignored --test-threads=1
//! ```

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "yaws and heights of a test"
)]

use std::sync::Arc;

use ashlar::LodPolicy;
use bevy::app::App;
use bevy::ecs::prelude::Entity;
use bevy::time::TimeUpdateStrategy;
use strata_app::crossings::{NearCrossing, NearCrossings, ProceduralCrossing};
use strata_app::{
    AppPlugin, GeneratedRoot, LocalPlayer, Player, PlayerIntent, ScriptedIntent, WorldPosition,
    WorldSourceRes,
};
use strata_core::{Seed, WorldPos};
use strata_physics::crossings::CrossingCollider;
use strata_physics::{CharacterController, PhysicsPlugin};
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::crossings::{
    BridgeClass, CrossingKit, DeckEnd, HeadwallFile, PierFile, place_crossing,
};
use strata_worldgen::roads::{CrossingKind, RoadClass};
use strata_worldgen::{ProceduralWorld, WorldSource};
use veyra_buildings::bridge::{self, Piece, Way};

/// The kit's layout as the content step writes it (`content::crossings`),
/// from the same catalogue, and each file's recipe by its path. Repeated
/// here because a physics test cannot depend on the content step, which
/// carries the authoring crates; every number in it is the kit's own.
fn kit() -> (CrossingKit, Vec<(String, ashlar::Building)>) {
    let road = |way: Way| match way {
        Way::Service => RoadClass::Service,
        Way::Track => RoadClass::Track,
        Way::Foot => RoadClass::Trail,
    };
    let mut kit = CrossingKit {
        bridges: Vec::new(),
        deck_end: DeckEnd {
            proud_m: bridge::PROUD_M,
            ramp_m: bridge::RAMP_M,
            drop_m: bridge::RAMP_DROP_M,
        },
        piers: Vec::new(),
        headwalls: Vec::new(),
        ford_post: None,
        stones: Vec::new(),
        stone_proud_m: bridge::STONE_PROUD_M,
    };
    let mut files = Vec::new();
    for piece in bridge::catalogue() {
        let file = format!("crossings/{}", piece.file_name());
        files.push((file.clone(), piece.building().unwrap()));
        match piece {
            Piece::Bridge(b) => kit.bridges.push(BridgeClass {
                road: road(b.way),
                file,
                clear_m: b.clear_m,
                gaps_m: b.gaps_m(),
                length_m: b.length_m(),
                underside_m: b.underside_m(),
                piers: b.piers(),
                girder_most_m: bridge::GIRDER_MOST_M,
            }),
            Piece::Pier(p) => kit.piers.push(PierFile {
                road: road(p.way),
                file,
                height_m: p.height_m,
                half_along_m: p.half_along_m(),
            }),
            Piece::Headwall(h) => kit.headwalls.push(HeadwallFile {
                file,
                box_m: h.box_m,
                top_m: h.top_m(),
            }),
            Piece::FordPost => kit.ford_post = Some(file),
            Piece::Stone(_) => kit.stones.push(file),
        }
    }
    (kit, files)
}

/// Seed 42's world, its rivers baked round `at`.
fn world_round(at: [f64; 2]) -> Arc<ProceduralWorld> {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .unwrap();
    let world = ProceduralWorld::with_params(Seed(42), &registries, strata_app::world_params());
    world.prepare_water_on(at[0], at[1], 3_000.0, 4);
    Arc::new(world)
}

/// The showcase road's first bridge (ticket 0005): where the alignment
/// pass's wall search stood it, 190 m upstream of where the fine search
/// crossed (round 2's (7 579, -5 029)).
const SHOWCASE: [f64; 2] = [7_404.0, -4_940.0];

/// The crossing of `kind` nearest `at`.
fn crossing(world: &ProceduralWorld, at: [f64; 2], kind: CrossingKind) -> ProceduralCrossing {
    world
        .crossings_near(at[0], at[1], 60.0)
        .into_iter()
        .find(|c| c.crossing.kind == kind)
        .unwrap()
}

/// An app over `world` with `crossing`'s pieces listed near and the files
/// they stand baked under a scratch root.
fn app_at(world: &Arc<ProceduralWorld>, crossing: ProceduralCrossing, name: &str) -> App {
    let (kit, files) = kit();
    let placed = place_crossing(&crossing, &kit, world.as_ref());
    let pieces = placed.clone().unwrap();
    assert!(!pieces.is_empty(), "nothing stands at {}", crossing.id);
    let root = std::env::temp_dir().join(format!("strata-crossings-{name}-{}", std::process::id()));
    for piece in &pieces {
        let building = &files.iter().find(|(p, _)| *p == piece.file).unwrap().1;
        let baked = ashlar_manifold::bake(
            building,
            &LodPolicy::ladder(),
            &ashlar_manifold::ManifoldMesher::default(),
        )
        .unwrap();
        let path = root.join(&piece.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            ashlar::collision_file::path_for(path.to_str().unwrap()),
            ashlar::collision_file::CollisionFile::from_baked(&baked)
                .unwrap()
                .write(),
        )
        .unwrap();
        std::fs::write(path, baked.write().unwrap()).unwrap();
    }
    let mut app = App::new();
    app.insert_resource(StreamConfig {
        max_lod: 4,
        view_distance: 400.0,
        max_loads_per_frame: 32,
        max_unloads_per_frame: 32,
        ..StreamConfig::default()
    });
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        AppPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(Arc::clone(world) as Arc<dyn WorldSource>));
    app.insert_resource(GeneratedRoot(Some(strata_registry::files::Files::dir(
        root,
    ))));
    app.insert_resource(NearCrossings {
        crossings: vec![NearCrossing { crossing, placed }],
        around: vec![crossing.crossing.at],
    });
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    app
}

fn spawn(app: &mut App, world: &ProceduralWorld, at: [f64; 2], above: f64) -> Entity {
    let controller = CharacterController::default();
    let ground = f64::from(world.sample(at[0], at[1]).height);
    let centre = ground + above + f64::from(controller.half_height());
    app.world_mut()
        .spawn((
            Player::default(),
            LocalPlayer,
            controller,
            WorldPosition::new(at[0], centre, at[1]),
        ))
        .id()
}

/// The intent's yaw that walks along the unit `(x, z)` `dir`.
fn yaw_along(dir: [f64; 2]) -> f32 {
    (-dir[0]).atan2(-dir[1]) as f32
}

fn walk(app: &mut App, player: Entity, dir: [f64; 2], sprint: bool) {
    app.world_mut()
        .entity_mut(player)
        .insert(ScriptedIntent(PlayerIntent {
            move_dir: bevy::math::Vec2::Y,
            yaw: yaw_along(dir),
            jump: false,
            sprint,
        }));
}

fn position(app: &App, e: Entity) -> WorldPos {
    app.world().get::<WorldPosition>(e).unwrap().0
}

fn feet(app: &App, e: Entity) -> f64 {
    position(app, e).y()
        - f64::from(
            app.world()
                .get::<CharacterController>(e)
                .unwrap()
                .half_height(),
        )
}

/// Updates, with an observer at the crossing, until its pieces collide
/// and the terrain round it is in: a walker spawned before would fall
/// through a deck that is not there yet.
fn wait_for_bodies(app: &mut App, at: [f64; 2]) {
    let observer = app
        .world_mut()
        .spawn((
            strata_app::WorldObserver,
            WorldPosition::new(at[0], 0.0, at[1]),
        ))
        .id();
    let started = std::time::Instant::now();
    loop {
        app.update();
        let bodies = app
            .world_mut()
            .query::<&CrossingCollider>()
            .iter(app.world())
            .count();
        let terrain = app
            .world()
            .resource::<strata_physics::TerrainColliderStats>()
            .loaded;
        if bodies > 0 && terrain > 0 {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "the crossing never collided"
        );
    }
    for _ in 0..30 {
        app.update();
    }
    app.world_mut().despawn(observer);
}

/// Updates until the player stands.
fn settle(app: &mut App, player: Entity) {
    for _ in 0..400 {
        app.update();
        if app
            .world()
            .get::<CharacterController>(player)
            .unwrap()
            .grounded
        {
            for _ in 0..20 {
                app.update();
            }
            return;
        }
    }
    panic!("the player never stood");
}

/// How far along `dir` from `from` the player is, and how far to the side.
fn along_across(app: &App, e: Entity, from: [f64; 2], dir: [f64; 2]) -> (f64, f64) {
    let p = position(app, e);
    let (dx, dz) = (p.x() - from[0], p.z() - from[1]);
    (dx * dir[0] + dz * dir[1], dx * dir[1] - dz * dir[0])
}

/// From one shelf over the deck to the other, walking and then running,
/// never lower than the deck over the gap, never held up at either end.
#[test]
#[ignore = "a river bake and a minute of streaming: by hand, in release"]
fn a_character_walks_and_runs_across_the_showcase_bridge() {
    let world = world_round(SHOWCASE);
    let c = crossing(&world, SHOWCASE, CrossingKind::Bridge);
    assert_eq!(c.class, RoadClass::Service);
    let x = c.crossing;
    let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
    let gap = (dx * dx + dz * dz).sqrt();
    let dir = [dx / gap, dz / gap];
    for sprint in [false, true] {
        let mut app = app_at(&world, c, if sprint { "run" } else { "walk" });
        wait_for_bodies(&mut app, x.at);
        let start = [c.edges[0][0] - dir[0] * 8.0, c.edges[0][1] - dir[1] * 8.0];
        let player = spawn(&mut app, &world, start, 0.1);
        settle(&mut app, player);
        walk(&mut app, player, dir, sprint);
        let goal = gap + 16.0;
        let mut lowest_over_gap = f64::INFINITY;
        let mut last = (0, along_across(&app, player, start, dir).0);
        for tick in 0..600 {
            app.update();
            let (along, across) = along_across(&app, player, start, dir);
            if along >= goal {
                break;
            }
            if (8.5..8.0 + gap - 0.5).contains(&along) {
                lowest_over_gap = lowest_over_gap.min(feet(&app, player));
            }
            assert!(
                across.abs() < 3.0,
                "tick {tick}: drifted {across:.2} m off the road"
            );
            if along > last.1 + 0.5 {
                last = (tick, along);
            }
            assert!(
                tick - last.0 < 20,
                "tick {tick}: held up {along:.2} m along (sprint {sprint})"
            );
            assert!(tick < 599, "never arrived: {along:.2} m of {goal:.1}");
        }
        assert!(
            lowest_over_gap >= x.deck_m - 0.05,
            "fell to {lowest_over_gap:.2} over a deck at {:.2} (sprint {sprint})",
            x.deck_m
        );
        assert!(
            app.world()
                .get::<CharacterController>(player)
                .unwrap()
                .grounded,
            "not standing at the far shelf"
        );
    }
}

/// On the deck, walking square to the road, a character is stopped by the
/// rail and stays on the deck.
#[test]
#[ignore = "a river bake and a minute of streaming: by hand, in release"]
fn the_rails_keep_a_walker_on_the_deck() {
    let world = world_round(SHOWCASE);
    let c = crossing(&world, SHOWCASE, CrossingKind::Bridge);
    let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
    let gap = (dx * dx + dz * dz).sqrt();
    let dir = [dx / gap, dz / gap];
    let mid = [
        f64::midpoint(c.edges[0][0], c.edges[1][0]),
        f64::midpoint(c.edges[0][1], c.edges[1][1]),
    ];
    for side in [1.0, -1.0] {
        let mut app = app_at(&world, c, if side > 0.0 { "right" } else { "left" });
        wait_for_bodies(&mut app, c.crossing.at);
        let controller = CharacterController::default();
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                LocalPlayer,
                controller.clone(),
                WorldPosition::new(
                    mid[0],
                    c.crossing.deck_m + 0.1 + f64::from(controller.half_height()),
                    mid[1],
                ),
            ))
            .id();
        settle(&mut app, player);
        let across = [dir[1] * side, -dir[0] * side];
        walk(&mut app, player, across, true);
        for _ in 0..80 {
            app.update();
        }
        let (_, off) = along_across(&app, player, mid, dir);
        assert!(
            off.abs() < Way::Service.deck_half_m(),
            "walked {off:.2} m off the middle, past the rail"
        );
        assert!(
            (feet(&app, player) - c.crossing.deck_m).abs() < 0.35,
            "not on the deck: feet at {:.2}",
            feet(&app, player)
        );
    }
}

/// A character wades a track's ford near Wack's Hold from one bank to the
/// other, between its posts, without swimming.
#[test]
#[ignore = "a river bake and a minute of streaming: by hand, in release"]
fn a_character_fords_a_ford() {
    let world = world_round([-1_431.0, -4_503.0]);
    let c = crossing(&world, [-1_431.0, -4_503.0], CrossingKind::Ford);
    let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
    let width = (dx * dx + dz * dz).sqrt();
    let dir = [dx / width, dz / width];
    let mut app = app_at(&world, c, "ford");
    wait_for_bodies(&mut app, c.crossing.at);
    let start = [c.edges[0][0] - dir[0] * 6.0, c.edges[0][1] - dir[1] * 6.0];
    let player = spawn(&mut app, &world, start, 0.1);
    settle(&mut app, player);
    walk(&mut app, player, dir, false);
    let goal = width + 12.0;
    for tick in 0..600 {
        app.update();
        let (along, across) = along_across(&app, player, start, dir);
        if along >= goal {
            return;
        }
        assert!(
            !app.world()
                .get::<CharacterController>(player)
                .unwrap()
                .swimming,
            "tick {tick}: swimming {along:.2} m along"
        );
        assert!(
            across.abs() < 2.0,
            "tick {tick}: carried {across:.2} m off the ford"
        );
    }
    panic!("never crossed the ford");
}

/// Level ground at ten metres, no water.
struct Flat;

impl WorldSource for Flat {
    fn sample(&self, _x: f64, _z: f64) -> strata_worldgen::Sample {
        strata_worldgen::Sample {
            height: 10.0,
            biomes: [
                (strata_registry::BiomeId(0), 1.0),
                (strata_registry::BiomeId(0), 0.0),
            ],
            provinces: strata_worldgen::Sample::NO_PROVINCES,
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

/// A piece whose file was never baked stands nothing and is not read
/// again; the crossing's other pieces collide as they would.
#[test]
fn a_missing_file_collides_nothing() {
    use strata_app::crossings::{CrossingId, PlacedPiece};
    use strata_worldgen::roads::Crossing;
    let root =
        std::env::temp_dir().join(format!("strata-crossings-missing-{}", std::process::id()));
    let stone = Piece::Stone(0);
    let baked = ashlar_manifold::bake(
        &stone.building().unwrap(),
        &LodPolicy::ladder(),
        &ashlar_manifold::ManifoldMesher::default(),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("crossings")).unwrap();
    std::fs::write(root.join("crossings/stone.ashlar"), baked.write().unwrap()).unwrap();
    std::fs::write(
        root.join("crossings/stone.collision"),
        ashlar::collision_file::CollisionFile::from_baked(&baked)
            .unwrap()
            .write(),
    )
    .unwrap();
    let piece = |file: &str, x: f64| PlacedPiece {
        file: file.to_owned(),
        at: [x, 10.12, 0.0],
        yaw: 0.0,
    };
    let crossing = ProceduralCrossing {
        id: CrossingId {
            mass: [0, 0],
            road: 0,
            index: 0,
        },
        class: RoadClass::Trail,
        crossing: Crossing {
            kind: CrossingKind::Ford,
            at: [0.0, 0.0],
            along_m: 10.0,
            bearing: [0.0, 1.0],
            span_m: 3.0,
            deck_m: 9.6,
            surface_m: 10.0,
            banks_m: [10.2, 10.2],
            width_m: 3.0,
            depth_m: 0.4,
            river: 0,
            abutments_m: [8.5, 11.5],
        },
        edges: [[0.0, -1.5], [0.0, 1.5]],
    };
    let mut app = App::new();
    app.insert_resource(StreamConfig {
        max_lod: 2,
        view_distance: 300.0,
        ..StreamConfig::default()
    });
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        AppPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(Arc::new(Flat)));
    app.insert_resource(GeneratedRoot(Some(strata_registry::files::Files::dir(
        root.clone(),
    ))));
    app.insert_resource(NearCrossings {
        crossings: vec![NearCrossing {
            crossing,
            placed: Ok(vec![
                piece("crossings/stone.ashlar", 1.0),
                piece("crossings/missing.ashlar", -1.0),
            ]),
        }],
        around: vec![[0.0, 0.0]],
    });
    app.finish();
    app.cleanup();
    app.world_mut().spawn((
        strata_app::WorldObserver,
        strata_app::PrimaryObserver,
        WorldPosition::new(0.0, 12.0, 0.0),
    ));
    let count = |app: &mut App| {
        app.world_mut()
            .query::<&CrossingCollider>()
            .iter(app.world())
            .map(|c| c.file.clone())
            .collect::<Vec<_>>()
    };
    let started = std::time::Instant::now();
    while count(&mut app).is_empty() {
        app.update();
        assert!(started.elapsed() < std::time::Duration::from_secs(60));
    }
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(count(&mut app), vec!["crossings/stone.ashlar".to_owned()]);
    let _ = std::fs::remove_dir_all(&root);
}
