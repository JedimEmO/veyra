//! Shared fixtures for the integration tests.

use strata_core::Seed;
use strata_worldgen::ProceduralWorld;
use veyra_data::GameRegistries;

/// The shipped asset set, the one the game boots with.
pub fn registries() -> GameRegistries {
    veyra_data::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("the shipped assets load")
}

/// A world over the shipped assets, its rivers baked over [`PREPARED_M`]
/// round the origin on [`THREADS`] (ticket 0005, step 2): every sample is
/// carved by the network round it, and a sample that met an unbaked land
/// mass would bake it on its own thread, which a debug build takes a minute
/// over. Every world of a seed in a binary shares one network cache, so
/// the bake is paid once a binary (7.5 s in a debug build for seed 42).
#[allow(dead_code, reason = "not every test binary builds a world")]
pub fn world(seed: u64) -> ProceduralWorld {
    world_within(seed, PREPARED_M)
}

/// [`world`] with its rivers baked over `radius` round the origin instead,
/// for a test that samples farther out.
#[allow(dead_code, reason = "not every test binary builds a world")]
pub fn world_within(seed: u64, radius: f64) -> ProceduralWorld {
    static PREPARED: std::sync::Mutex<Vec<(u64, f64)>> = std::sync::Mutex::new(Vec::new());
    let world = ProceduralWorld::new(Seed(seed), &registries());
    let mut prepared = PREPARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Once a seed and radius: the cache is shared, and a property test
    // builds its world for every case.
    if !prepared
        .iter()
        .any(|&(held, reach)| held == seed && reach >= radius)
    {
        world.hydrology().set_threads(THREADS);
        world.prepare_water(0.0, 0.0, radius);
        prepared.push((seed, radius));
    }
    world
}

/// How far round the origin [`world`] bakes the rivers, metres: the drama
/// measures' disc, and every fixed place the tests sample.
#[allow(dead_code, reason = "not every test binary builds a world")]
pub const PREPARED_M: f64 = 100_000.0;

/// Threads of a sweep's pool: not every core. A debug build on the
/// development machine (an i9-13900KF) dies with SIGSEGV under heavy
/// multi-thread load (32 threads in the sampler, or two or three pools at
/// once); that is the CPU, so rerun and do not trace it.
#[allow(dead_code, reason = "not every test binary sweeps")]
pub const THREADS: usize = 16;

/// Held for a sweep's length, so that one pool runs at a time in a binary
/// whatever the harness does.
#[allow(dead_code, reason = "not every test binary sweeps")]
pub fn one_sweep_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A pool of [`THREADS`]. Hold [`one_sweep_at_a_time`] while it is used.
#[allow(dead_code, reason = "not every test binary sweeps")]
pub fn pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(THREADS)
        .build()
        .expect("a thread pool")
}

/// Runs `work` on a [`pool`] holding [`one_sweep_at_a_time`]; `work` must
/// not call this again.
#[allow(dead_code, reason = "not every test binary sweeps")]
pub fn sweep<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let _one = one_sweep_at_a_time();
    pool().install(work)
}
