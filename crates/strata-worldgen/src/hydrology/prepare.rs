//! Baking the rivers ahead of a start, and the one file a bake is kept in.
//!
//! This is Bevy-free so a tool (`xtask bundle`) can bake the browser's file
//! without linking the sim; `veyra-sim` wraps [`PrepareArea`] in the
//! resource the loader reads and re-exports the rest.

use strata_core::Seed;
use strata_data::LoadError;
use strata_data::files::Files;

use super::MAX_THREADS;
use super::persist::{MassEntry, WaterIndex};
use crate::{ProceduralWorld, WorldParams};

/// How far round a place the network is baked by default, metres: past the
/// farthest terrain ring, so the first view of the world samples no unbaked
/// mass (captures with 12 and 72 km each met one further out, at 59 and
/// 87 km). Seed 42's 83 masses within it bake in 0.9 s on 16 threads.
pub const PREPARE_WATER_RADIUS_M: f64 = 128_000.0;

/// How far round a place a start bakes (or reads) the network when the
/// masses come over the network: the browser's. The first view samples out
/// to about 87 km (a capture with a bake of 72 km met an unbaked mass there,
/// see [`PREPARE_WATER_RADIUS_M`]), so 96 km leaves no sample to bake a mass
/// on its own thread, which in a browser is the only thread; the 128 km square
/// beyond arrives through the fetch ahead of the observer, whose first round
/// asks for it as soon as the world has loaded and whose later rounds start a
/// quarter of 128 km from every square asked before. `xtask bundle` carries the masses inside it: seed 42's bundle holds
/// 21 of its 42 masses and 56 of the 63 MB they weigh gzipped, because the
/// home mass alone is 35 MB, and the other 21 are 6 MB fetched.
pub const WEB_PREPARE_RADIUS_M: f64 = 96_000.0;

/// Where the world will be sampled first, `(x, z)`, and how far round: the
/// river network is baked there, and round every settlement as far.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepareArea {
    /// The places, `(x, z)`.
    pub places: Vec<(f64, f64)>,
    /// How far round each, metres.
    pub radius_m: f64,
}

impl Default for PrepareArea {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            radius_m: PREPARE_WATER_RADIUS_M,
        }
    }
}

/// Every place a [`PrepareArea`] bakes round: each settlement's centre, then
/// its own places.
#[must_use]
pub fn water_places(world: &ProceduralWorld, prepare: &PrepareArea) -> Vec<(f64, f64)> {
    world
        .sites()
        .iter()
        .map(|site| {
            let [x, z] = site.to_world_xz([0.0, 0.0]);
            (x, z)
        })
        .chain(prepare.places.iter().copied())
        .collect()
}

/// Bakes the rivers round every place of `prepare` on `threads` threads
/// (at most `MAX_THREADS`), and returns how many masses that is.
#[must_use]
pub fn prepare_round(world: &ProceduralWorld, prepare: &PrepareArea, threads: usize) -> usize {
    water_places(world, prepare)
        .iter()
        .map(|&(x, z)| world.prepare_water_on(x, z, prepare.radius_m, threads))
        .sum()
}

/// The baked rivers as the files of [`WATER_INDEX`](super::WATER_INDEX) and
/// [`MASS_DIR`](super::MASS_DIR): the
/// bake a start would run, one file a mass, with the stamp they carry. A tool
/// runs it natively on every thread and decides which masses ship where; the
/// browser reads what it is given and fetches the rest. `near` is baked
/// first and its masses are marked ([`WaterMasses::near`]); `far` is baked
/// after it, in the same cache, and its masses are the rest.
///
/// # Errors
/// The registries did not load.
pub fn bake_water_masses(
    files: &Files,
    seed: Seed,
    params: WorldParams,
    near: &PrepareArea,
    far: &PrepareArea,
) -> Result<(WaterMasses, usize), LoadError> {
    let registries = strata_data::load_world_from_source(&files.under("assets"))?;
    let world = ProceduralWorld::with_params(seed, &registries, params);
    // Only which masses the near bake met is wanted of it, below.
    let _ = prepare_round(&world, near, MAX_THREADS);
    let stamp = crate::version::water_stamp(seed, &params, &registries);
    let near: Vec<[i32; 2]> = world
        .hydrology()
        .save_masses(stamp, |_| true)
        .into_iter()
        .map(|(entry, _)| entry.key)
        .collect();
    let masses = prepare_round(&world, far, MAX_THREADS);
    Ok((
        WaterMasses {
            seed,
            stamp,
            near,
            files: world.hydrology().save_masses(stamp, |_| true),
        },
        masses,
    ))
}

/// A bake's per-mass files, sorted by key, with what names them.
#[derive(Debug)]
pub struct WaterMasses {
    /// The world's seed.
    pub seed: Seed,
    /// The stamp every file carries.
    pub stamp: u64,
    /// The keys of the masses the near bake met.
    pub near: Vec<[i32; 2]>,
    /// Each mass's entry and file.
    pub files: Vec<(MassEntry, Vec<u8>)>,
}

impl WaterMasses {
    /// The index of every file, wherever it ships, so a reader knows what to
    /// look for.
    #[must_use]
    pub fn index(&self) -> WaterIndex {
        WaterIndex::new(
            self.seed,
            self.stamp,
            self.files.iter().map(|(entry, _)| entry.clone()).collect(),
        )
    }

    /// Whether `entry`'s mass is one the near bake met.
    #[must_use]
    pub fn is_near(&self, entry: &MassEntry) -> bool {
        self.near.contains(&entry.key)
    }
}
