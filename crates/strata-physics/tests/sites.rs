//! The procedural sites' buildings collide where the site list says they
//! stand, only near an observer, and not at all when their file is missing.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]

use std::sync::Arc;

use ashlar::{Building, Instance, LodPolicy};
use bevy::app::App;
use bevy::time::TimeUpdateStrategy;
use strata_app::sites::{NearSite, NearSites};
use strata_app::{
    AppPlugin, GeneratedRoot, PrimaryObserver, RenderOrigin, WorldObserver, WorldPosition,
    WorldSourceRes,
};
use strata_core::WorldPos;
use strata_physics::PhysicsPlugin;
use strata_physics::avian3d::prelude::{Position, RigidBody, Rotation};
use strata_physics::sites::{COLLIDE_M, SiteCollider};
use strata_registry::{BiomeId, SiteKindId};
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::sites::{PlacedSiteBuilding, ProceduralSite, SiteId, SitePlacement};
use strata_worldgen::{Sample, WorldSource};

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

/// A bollard, the smallest kit that declares collision, baked to
/// `root/sites/bollard.ashlar`.
fn bake_bollard(root: &std::path::Path) {
    use veyra_buildings::port::Fitting;
    let part = Fitting::Bollard.part().unwrap();
    let mut builder = Building::builder("test:bollard");
    for slot in ["metal", "hazard"] {
        builder = builder.material(slot, format!("test:{slot}"));
    }
    let building = builder
        .part(part)
        .instance(Instance::new("one", Fitting::Bollard.key()))
        .build()
        .unwrap();
    let baked = ashlar_manifold::bake(
        &building,
        &LodPolicy::ladder(),
        &ashlar_manifold::ManifoldMesher::default(),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("sites")).unwrap();
    std::fs::write(root.join("sites/bollard.ashlar"), baked.write().unwrap()).unwrap();
    std::fs::write(
        root.join("sites/bollard.collision"),
        ashlar::collision_file::CollisionFile::from_baked(&baked)
            .unwrap()
            .write(),
    )
    .unwrap();
}

/// A site at `(x, z)` standing the bollard and a file that was never baked.
fn site(x: f64, z: f64, node: i32) -> NearSite {
    let building = |file: &str, dx: f64, yaw: f64| PlacedSiteBuilding {
        file: file.to_owned(),
        at: [x + dx, 10.0, z + 5.0],
        yaw,
        footprint: [[x + dx, z + 5.0]; 4],
    };
    NearSite {
        site: ProceduralSite {
            id: SiteId {
                kind: SiteKindId(0),
                mass: [0, 0],
                node: [node, 0],
            },
            name: format!("Test {node}"),
            centre: [x, z],
            level_m: 10.0,
            radius_m: 20.0,
            blend_m: 20.0,
            crown: 0.0,
            facing: [0.0, 1.0],
            arrival: [x, z - 20.0],
            by: None,
            score: 1.0,
            cut_m: 0.0,
        },
        kind: "test:kind".to_owned(),
        kind_name: "Test".to_owned(),
        placement: Some(SitePlacement {
            arrangement: 0,
            way_in_clear: true,
            buildings: vec![
                building("sites/bollard.ashlar", 3.0, 0.5),
                building("sites/missing.ashlar", -3.0, 0.0),
            ],
            sign: None,
        }),
    }
}

fn colliders(app: &mut App) -> Vec<(SiteCollider, Position, Rotation)> {
    let mut query = app
        .world_mut()
        .query_filtered::<(&SiteCollider, &Position, &Rotation), bevy::ecs::query::With<RigidBody>>(
        );
    query
        .iter(app.world())
        .map(|(c, p, r)| (c.clone(), *p, *r))
        .collect()
}

#[test]
fn site_buildings_collide_where_the_list_stands_them_and_only_near() {
    let root = std::env::temp_dir().join(format!("strata-site-colliders-{}", std::process::id()));
    bake_bollard(&root);
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
    // One site near the observer, one far past where anything collides.
    app.insert_resource(NearSites {
        sites: vec![site(40.0, 0.0, 1), site(COLLIDE_M * 3.0, 0.0, 2)],
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
    while colliders(&mut app).is_empty() {
        app.update();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "no site collider was built"
        );
    }
    // A few more frames: nothing else arrives.
    for _ in 0..10 {
        app.update();
    }
    let built = colliders(&mut app);
    assert_eq!(
        built.len(),
        1,
        "only the near site's baked building: {built:?}"
    );
    let (collider, position, rotation) = &built[0];
    assert_eq!(collider.file, "sites/bollard.ashlar");
    assert_eq!(collider.site, "Test 1");
    let origin = *app.world().resource::<RenderOrigin>();
    let at = WorldPos::new(43.0, 10.0, 5.0);
    assert_eq!(position.0, origin.to_render(at));
    assert!(
        rotation
            .0
            .angle_between(bevy::math::Quat::from_rotation_y(0.5))
            < 1e-6
    );
    // The observer leaves: the body goes.
    app.world_mut()
        .entity_mut(observer)
        .insert(WorldPosition::new(-5_000.0, 12.0, 0.0));
    for _ in 0..5 {
        app.update();
    }
    assert!(
        colliders(&mut app).is_empty(),
        "the body outlived its site's range"
    );
    let _ = std::fs::remove_dir_all(&root);
}
