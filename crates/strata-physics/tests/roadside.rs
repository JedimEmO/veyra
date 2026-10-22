//! The roadside's markers and lamps (ticket 0005, step 8) collide where the
//! roadside list stands them, each only near an observer by its own
//! distance, from the kit's real baked pieces.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]

use std::sync::Arc;

use ashlar::LodPolicy;
use bevy::app::App;
use bevy::time::TimeUpdateStrategy;
use strata_app::roadside::{NearPiece, NearRoadside};
use strata_app::{
    AppPlugin, GeneratedRoot, PrimaryObserver, RenderOrigin, WorldObserver, WorldPosition,
    WorldSourceRes,
};
use strata_physics::PhysicsPlugin;
use strata_physics::avian3d::prelude::{Position, RigidBody};
use strata_physics::roadside::{COLLIDE_M, RoadsideCollider};
use strata_registry::BiomeId;
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::crossings::PlacedPiece;
use strata_worldgen::{Sample, WorldSource};
use veyra_buildings::roadside::Piece;

/// Level ground at ten metres.
struct Flat;

impl WorldSource for Flat {
    fn sample(&self, _x: f64, _z: f64) -> Sample {
        Sample {
            height: 10.0,
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

/// Both pieces baked under `root`, as the content step names them.
fn bake(root: &std::path::Path) {
    for piece in Piece::ALL {
        let baked = ashlar_manifold::bake(
            &piece.building().unwrap(),
            &LodPolicy::ladder(),
            &ashlar_manifold::ManifoldMesher::default(),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("roadside")).unwrap();
        let file = root.join("roadside").join(piece.file_name());
        std::fs::write(
            ashlar::collision_file::path_for(file.to_str().unwrap()),
            ashlar::collision_file::CollisionFile::from_baked(&baked)
                .unwrap()
                .write(),
        )
        .unwrap();
        std::fs::write(file, baked.write().unwrap()).unwrap();
    }
}

fn colliders(app: &mut App) -> Vec<(RoadsideCollider, Position)> {
    let mut query = app
        .world_mut()
        .query_filtered::<(&RoadsideCollider, &Position), bevy::ecs::query::With<RigidBody>>();
    query
        .iter(app.world())
        .map(|(c, p)| (c.clone(), *p))
        .collect()
}

#[test]
fn roadside_pieces_collide_where_the_list_stands_them_and_only_near() {
    let root = std::env::temp_dir().join(format!("strata-roadside-{}", std::process::id()));
    bake(&root);
    let piece = |file: &str, x: f64, i: usize| NearPiece {
        key: (11, i),
        piece: PlacedPiece {
            file: format!("roadside/{file}"),
            at: [x, 10.0, 6.0],
            yaw: 0.3,
        },
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
    // A marker and a lamp near the observer, a marker past where anything
    // collides, on the same road: each piece is measured on its own.
    app.insert_resource(NearRoadside {
        pieces: vec![
            piece(Piece::Marker.file_name(), 20.0, 0),
            piece(Piece::Lamp.file_name(), 40.0, 1),
            piece(Piece::Marker.file_name(), COLLIDE_M * 2.0, 2),
        ],
        around: vec![[0.0, 0.0]],
    });
    app.finish();
    app.cleanup();
    let observer = app
        .world_mut()
        .spawn((
            WorldObserver,
            PrimaryObserver,
            WorldPosition::new(0.0, 12.0, 0.0),
        ))
        .id();
    let started = std::time::Instant::now();
    while colliders(&mut app).len() < 2 {
        app.update();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "no roadside collider was built"
        );
    }
    for _ in 0..10 {
        app.update();
    }
    let mut built = colliders(&mut app);
    built.sort_by_key(|(c, _)| c.key);
    assert_eq!(built.len(), 2, "the two near pieces: {built:?}");
    assert_eq!(built[0].0.key, (11, 0));
    assert_eq!(built[1].0.key, (11, 1));
    let origin = *app.world().resource::<RenderOrigin>();
    let at = strata_core::WorldPos::new(40.0, 10.0, 6.0);
    assert_eq!(built[1].1.0, origin.to_render(at));
    app.world_mut()
        .entity_mut(observer)
        .insert(WorldPosition::new(-5_000.0, 12.0, 0.0));
    for _ in 0..5 {
        app.update();
    }
    assert!(
        colliders(&mut app).is_empty(),
        "the bodies outlived their range"
    );
    let _ = std::fs::remove_dir_all(&root);
}
