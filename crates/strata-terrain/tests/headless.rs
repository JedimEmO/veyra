//! The whole pipeline with no renderer: `MinimalPlugins`, the sim, the
//! loader, the streamer and the terrain plugin, driven by an observer on a
//! path. Meshes are built on the compute pool from the real seed-42 world
//! and summarised onto the region entities; without `Assets<Mesh>` nothing
//! is uploaded, which is exactly what a CI runner can do.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bevy::MinimalPlugins;
use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy::state::state::State;
use strata_app::{AppPlugin, AppState, PrimaryObserver, WorldLoadPlugin, WorldPosition, WorldSeed};
use strata_core::{Seed, WorldPos};
use strata_stream::{
    LoadedRegions, Region, RegionState, StreamConfig, StreamPlugin, WantedRegions,
};
use strata_terrain::mesh::VERTEX_COUNT;
use strata_terrain::{TerrainPlugin, TerrainRegion, TerrainStats};

fn assets() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

/// An app in the world with one observer at `at`.
fn app_with_observer(at: WorldPos) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(StreamConfig {
        max_lod: 5,
        view_distance: 2_000.0,
        max_loads_per_frame: 16,
        max_unloads_per_frame: 16,
        ..StreamConfig::default()
    });
    app.insert_resource(WorldSeed(Seed(42)));
    app.add_plugins((
        MinimalPlugins,
        AppPlugin,
        WorldLoadPlugin::new(assets()),
        StreamPlugin,
        TerrainPlugin,
    ));
    // Wall-clock, not frames, as the sim's own load test counts: the load
    // bakes the rivers round the settlement (ticket 0005, step 2), seconds
    // in a debug build, and a frame of these plugins is a tenth of a
    // millisecond.
    let started = std::time::Instant::now();
    while state(&app) != AppState::InWorld {
        app.update();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "never entered the world"
        );
    }
    let observer = app
        .world_mut()
        .spawn((PrimaryObserver, WorldPosition(at)))
        .id();
    (app, observer)
}

fn settled(app: &App) -> bool {
    let loaded = app.world().resource::<LoadedRegions>();
    let wanted: HashSet<_> = app
        .world()
        .resource::<WantedRegions>()
        .regions()
        .iter()
        .copied()
        .collect();
    let loaded_set: HashSet<_> = loaded.iter().map(|(c, _)| c).collect();
    !wanted.is_empty()
        && wanted == loaded_set
        && loaded.pending().count() == 0
        && app.world().resource::<TerrainStats>().in_flight == 0
}

/// Runs until the loaded set equals the wanted set with every mesh in.
///
/// Bounded by wall time, not frames: meshing happens on the pool and a
/// frame with nothing to do takes microseconds, so a debug build would
/// burn through any frame budget before the first mesh landed.
fn settle(app: &mut App, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        app.update();
        if settled(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let stats = *app.world().resource::<TerrainStats>();
    let loaded = app.world().resource::<LoadedRegions>();
    panic!(
        "did not settle in {timeout:?}: {} loaded, {} pending, {stats:?}",
        loaded.len(),
        loaded.pending().count()
    );
}

/// Generous: a debug build meshes a region in tens of milliseconds and the
/// small config wants a few hundred of them.
const SETTLE: Duration = Duration::from_secs(120);

fn assert_every_ready_region_is_meshed(app: &mut App) {
    let source = app.world().resource::<strata_app::WorldSourceRes>().clone();
    let mut query = app.world_mut().query::<(&Region, Option<&TerrainRegion>)>();
    let mut count = 0;
    for (region, terrain) in query.iter(app.world()) {
        if region.state != RegionState::Ready {
            continue;
        }
        let terrain =
            terrain.unwrap_or_else(|| panic!("{:?} is ready without a mesh", region.coord));
        assert_eq!(terrain.vertices as usize, VERTEX_COUNT);
        assert!(terrain.min_height <= terrain.max_height);
        // The centre is a grid vertex, so its height is inside the range.
        let c = region.coord.center();
        let h = source.source().sample(c.x(), c.z()).height;
        assert!(
            (terrain.min_height..=terrain.max_height).contains(&h),
            "{:?}: centre {h} outside {}..{}",
            region.coord,
            terrain.min_height,
            terrain.max_height
        );
        count += 1;
    }
    assert!(count > 0, "nothing was meshed");
}

#[test]
fn regions_are_meshed_and_reported_ready() {
    let (mut app, _) = app_with_observer(WorldPos::from_xz(100.0, 100.0));
    settle(&mut app, SETTLE);
    let stats = *app.world().resource::<TerrainStats>();
    assert!(stats.built > 10, "{stats:?}");
    assert_every_ready_region_is_meshed(&mut app);
}

#[test]
fn a_path_leaks_no_tasks_and_leaves_no_hole() {
    let start = WorldPos::from_xz(20_000.0, -5_000.0);
    let (mut app, observer) = app_with_observer(start);
    settle(&mut app, SETTLE);

    // 1.5 km out and back, fast enough to split, merge and unload with
    // work in flight.
    let steps = 300;
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps) * core::f64::consts::TAU;
        let r = 1_500.0 * (1.0 - t.cos());
        let pos = WorldPos::from_xz(start.x() + r, start.z() + r * 0.4);
        app.world_mut()
            .get_mut::<WorldPosition>(observer)
            .unwrap()
            .0 = pos;
        app.update();
        // Every ready region has its summary, on every frame of the path.
        let mut query = app.world_mut().query::<(&Region, Has<TerrainRegion>)>();
        for (region, meshed) in query.iter(app.world()) {
            if region.state == RegionState::Ready {
                assert!(meshed, "{:?} ready without a mesh", region.coord);
            }
        }
    }
    settle(&mut app, SETTLE);
    let stats = *app.world().resource::<TerrainStats>();
    assert_eq!(stats.in_flight, 0);
    assert!(
        stats.cancelled > 0,
        "the path never cancelled a task; it should have outrun the mesher: {stats:?}"
    );
    assert_every_ready_region_is_meshed(&mut app);
    // No task belongs to an entity that no longer exists: the loaded set
    // and the region entities agree.
    let loaded = app.world().resource::<LoadedRegions>().len();
    let mut query = app.world_mut().query::<&Region>();
    assert_eq!(query.iter(app.world()).count(), loaded);
}
