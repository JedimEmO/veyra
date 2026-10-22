//! Shared fixtures: the shipped assets and a world over them.

use strata_core::Seed;
use strata_data::WorldDefs;
use strata_flora::FloraTable;
use strata_worldgen::ProceduralWorld;

pub fn registries() -> WorldDefs {
    strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("the shipped assets load")
}

/// A world over the shipped assets, its rivers baked over 100 km round the
/// origin on [`THREADS`] once a seed (ticket 0005, step 2: a sample that met
/// an unbaked land mass would bake it on its own thread, slowly in a debug
/// build), and the flora table.
pub fn world_and_table(seed: u64) -> (ProceduralWorld, FloraTable) {
    static PREPARED: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
    let registries = registries();
    let world = ProceduralWorld::new(Seed(seed), &registries);
    let mut prepared = PREPARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !prepared.contains(&seed) {
        world.hydrology().set_threads(THREADS);
        world.prepare_water(0.0, 0.0, 100_000.0);
        prepared.push(seed);
    }
    (world, FloraTable::from_registries(&registries))
}

/// Threads of a parallel test's pool: not every core. A debug build on the
/// development machine (an i9-13900KF) dies with SIGSEGV under heavy
/// multi-thread load; that is the CPU, so rerun and do not trace it. The
/// tests of a binary run one at a time already (`serial` in
/// `placement.rs`), so one pool runs at a time.
pub const THREADS: usize = 16;

/// A pool of [`THREADS`] for a test to `install` its parallel work on.
#[allow(dead_code, reason = "not every test binary runs a pool")]
pub fn pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(THREADS)
        .build()
        .expect("a thread pool")
}
