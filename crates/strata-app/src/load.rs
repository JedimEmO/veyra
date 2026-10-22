//! Loading the game data and building the world source, in
//! [`AppState::LoadingRegistries`].
//!
//! The registries are plain files and the world is a pure function of the
//! seed and those registries (PLAN principle 1), so this is the one place
//! that reads the asset root for game data. It runs the same on a dedicated
//! server and on the client: the render side never builds a world source,
//! it only samples the one the sim publishes as [`WorldSourceRes`].
//!
//! The load runs on the compute pool so a window that is already open keeps
//! pumping events, and the [`LoadingHold`] keeps the app out of the world
//! until the source exists. A failed load is fatal: nothing above this can
//! run without biomes, so the app exits with an error rather than entering
//! an empty world.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::app::{App, AppExit, Plugin, Update};
use bevy::ecs::prelude::*;
use bevy::state::condition::in_state;
use bevy::state::state::OnEnter;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use strata_core::Seed;
use strata_flora::TreeLibrary;
use strata_registry::files::Files;
use strata_registry::{AssetSource, GameRegistries, LoadError};
use strata_worldgen::hydrology::{MassEntry, PrepareArea, WATER_INDEX, WaterIndex};
use strata_worldgen::{ProceduralWorld, SettlementLayout, Site, WorldParams, WorldSource};
use tracing::{error, info, warn};

use crate::{AppState, LoadingHold, Registries, WorldSeed};

/// The world every consumer samples, shared read-only across threads.
///
/// A trait object so a persisted-edits source can replace the procedural
/// one later without any consumer noticing (PLAN principle 4). Present from
/// [`AppState::InWorld`] on when [`WorldLoadPlugin`] is added; absent in a
/// bare test app, which is why terrain takes it as an `Option`.
#[derive(Resource, Clone)]
pub struct WorldSourceRes(pub Arc<dyn WorldSource>);

impl WorldSourceRes {
    /// The source.
    #[must_use]
    pub fn source(&self) -> &dyn WorldSource {
        self.0.as_ref()
    }
}

/// The same world as [`WorldSourceRes`], as the procedural world it is:
/// what draws the network itself (the falls) reads its baked rivers from
/// it, which the trait object does not expose. Present with
/// [`WorldSourceRes`], from [`AppState::InWorld`] on.
#[derive(Resource, Clone)]
pub struct ProceduralWorldRes(pub Arc<ProceduralWorld>);

/// The baked tree library, when the app was given the content step's output:
/// placement and trunk colliders read their variants and trunks from it
/// (ADR 0005, 2026-09-26 amendment). `None` places trees with trunks derived
/// from their parameters, as a test or a tool without baked content does.
#[derive(Resource, Debug, Clone, Default)]
pub struct TreeLibraryRes(pub Option<Arc<TreeLibrary>>);

/// The parameters [`WorldLoadPlugin`] builds every world with: the
/// defaults. One function so [`world_version`] hashes the set the loader
/// uses and not a copy of it.
#[must_use]
pub fn world_params() -> WorldParams {
    WorldParams::default()
}

/// The version of the world [`WorldLoadPlugin`] builds from `asset_root`
/// (`strata_worldgen::world_version` of [`world_params`] and the
/// registries there), for the replication handshake: a process reads it
/// before the loader runs, from the same directory, as it reads the tree
/// library's.
///
/// # Errors
///
/// If the registries under `asset_root` do not load; the loader would
/// fail on them the same way.
pub fn world_version(asset_root: impl AsRef<std::path::Path>) -> Result<u64, LoadError> {
    let registries = strata_registry::load_from_dir(asset_root)?;
    Ok(strata_worldgen::world_version(&world_params(), &registries))
}

/// [`world_version`] over a source with the logical root `assets/`, the
/// installed one in the game.
///
/// # Errors
///
/// If the registries do not load.
pub fn world_version_from(files: &Files) -> Result<u64, LoadError> {
    let registries = strata_registry::load_from_source(&files.under("assets"))?;
    Ok(strata_worldgen::world_version(&world_params(), &registries))
}

/// Where `biomes/` and `terrain/` live, as a source whose paths are relative
/// to the asset root.
#[derive(Resource, Debug, Clone)]
pub struct WorldAssetRoot(pub Files);

/// Where the world will be sampled first, `(x, z)` (the spawn), and how far
/// round: the river network is baked there, and round every settlement as
/// far, before the world is published (ADR 0010). A game or a server bakes
/// it before it builds its app ([`prepare_water_before_start`]), so the
/// loader's world finds it done; with this resource the loader then bakes
/// ahead of the observers as they move, in the background on a few
/// threads. Without one (a test's app, which a debug build would keep for
/// seconds baking the whole view's) the loader bakes only the settlements'
/// country, `TEST_PREPARE_RADIUS_M` round each, and nothing ahead. A sample
/// that meets a land mass no one has baked bakes it itself, on its own
/// thread, which on the streamer's task pool would stall that region's
/// meshing for a second or more.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct PrepareWater {
    /// The places, `(x, z)`.
    pub places: Vec<(f64, f64)>,
    /// How far round each, metres.
    pub radius_m: f64,
    /// How far the fetch and bake ahead of an observer reach, when that is
    /// farther than `radius_m`: a start that prepares only part of the view
    /// (the browser's) still sets its rounds off by the whole view's reach,
    /// and asks for the rest of it at once. `None` is `radius_m`.
    pub ahead_radius_m: Option<f64>,
}

impl Default for PrepareWater {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            radius_m: strata_worldgen::hydrology::PREPARE_WATER_RADIUS_M,
            ahead_radius_m: None,
        }
    }
}

impl PrepareWater {
    /// The same places as the plain, Bevy-free record the bake reads.
    fn area(&self) -> PrepareArea {
        PrepareArea {
            places: self.places.clone(),
            radius_m: self.radius_m,
        }
    }
}

/// How far round each settlement an app with no [`PrepareWater`] bakes the
/// network, metres.
const TEST_PREPARE_RADIUS_M: f64 = 12_000.0;

/// Bakes the rivers round every place of `prepare` on `threads` threads
/// (at most `MAX_THREADS`), and returns how many masses that is.
fn prepare_round(world: &ProceduralWorld, prepare: &PrepareWater, threads: usize) -> usize {
    strata_worldgen::hydrology::prepare_round(world, &prepare.area(), threads)
}

/// The threads the loader's own bake uses. It runs inside a task pool, and
/// in a test binary beside other tests' apps doing the same, on a machine
/// that fails under heavy multi-thread load; a game or a server has baked
/// already, before its pools started, and finds the work done.
const LOADER_BAKE_THREADS: usize = 4;

/// Bakes the rivers a game or a server will sample first, before it builds
/// its app: no task pool is running yet, so the bake's threads (at most
/// `MAX_THREADS`) have the machine to themselves, and the world the loader
/// then builds from the same seed, registries and [`world_params`] shares
/// the bake (ADR 0010) and waits on nothing. Returns how many masses and how
/// long it took, in seconds.
///
/// With a `cache` directory the bake is kept there as a
/// [`WATER_INDEX`] and a file a mass
/// under `mass/`: read first when the index is there and its stamp
/// ([`water_stamp`](strata_worldgen::version::water_stamp)) matches this
/// world, and the masses the bake met that no file held are written back. The
/// bake is 9 s of every thread on a start, the read about 3, and the stamp
/// covers everything that moves a river, so a stale file is refused and
/// rebaked, never used.
///
/// # Errors
/// The registries at `asset_root` did not load.
pub fn prepare_water_before_start(
    asset_root: impl AsRef<std::path::Path>,
    seed: Seed,
    prepare: &PrepareWater,
    cache: Option<&std::path::Path>,
) -> Result<(usize, f64), LoadError> {
    let started = strata_core::platform::time::Instant::now();
    let registries = strata_registry::load_from_dir(asset_root.as_ref())?;
    let params = world_params();
    let world = ProceduralWorld::with_params(seed, &registries, params);
    let stamp = strata_worldgen::version::water_stamp(seed, &params, &registries);
    let known = cache.map_or_else(Vec::new, |dir| {
        read_water(&world, seed, stamp, started, |path| {
            std::fs::read(dir.join(path.trim_start_matches("generated/hydrology/"))).ok()
        })
    });
    let masses = prepare_round(&world, prepare, strata_worldgen::hydrology::MAX_THREADS);
    if let Some(dir) = cache {
        write_water_cache(&world, seed, stamp, &known, dir);
    }
    Ok((masses, started.elapsed().as_secs_f64()))
}

/// Reads the index through `read` (a logical path to its bytes) and merges
/// into the world's cache every mass it names whose file `read` has, and
/// returns the entries it merged (none for an index of another world, which
/// is logged and left). A mass whose file is missing or refused is left for
/// the bake, as a world with no files at all is.
fn read_water(
    world: &ProceduralWorld,
    seed: Seed,
    stamp: u64,
    started: strata_core::platform::time::Instant,
    read: impl Fn(&str) -> Option<Vec<u8>>,
) -> Vec<MassEntry> {
    let Some(bytes) = read(WATER_INDEX) else {
        return Vec::new();
    };
    let index = match WaterIndex::read(&bytes, seed, stamp) {
        Ok(index) => index,
        Err(error) => {
            warn!(%error, "the baked rivers index is not used, baking");
            return Vec::new();
        }
    };
    let threads = strata_worldgen::hydrology::MAX_THREADS;
    let mut merged = Vec::new();
    for entry in index.masses {
        let Some(bytes) = read(&entry.path()) else {
            continue;
        };
        match world.hydrology().load_mass(world, &bytes, stamp, threads) {
            Ok(_) => merged.push(entry),
            Err(error) => warn!(%error, key = ?entry.key, "a baked mass is not used, baking"),
        }
    }
    info!(
        masses = merged.len(),
        seconds = started.elapsed().as_secs_f64(),
        "the rivers read from the baked files"
    );
    merged
}

/// Writes the masses the cache holds that `known` (what the files gave)
/// does not, a file each, and the index of both, into `dir`, each through a
/// temporary name so a second process starting beside this one never reads
/// half a file. A failure is logged and costs only the next start its bake.
fn write_water_cache(
    world: &ProceduralWorld,
    seed: Seed,
    stamp: u64,
    known: &[MassEntry],
    dir: &std::path::Path,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let fresh = world
            .hydrology()
            .save_masses(stamp, |e| !known.iter().any(|k| k.key == e.key));
        if fresh.is_empty() {
            return;
        }
        let keep = |path: &std::path::Path, bytes: &[u8]| -> std::io::Result<()> {
            let temporary = path.with_extension(format!("tmp{}", std::process::id()));
            path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
            std::fs::write(&temporary, bytes)?;
            std::fs::rename(&temporary, path)
        };
        let mut total = 0;
        let mut entries = known.to_vec();
        for (entry, bytes) in fresh {
            let path = dir.join(entry.path().trim_start_matches("generated/hydrology/"));
            if let Err(error) = keep(&path, &bytes) {
                warn!(%error, path = %path.display(), "a baked mass was not kept");
                continue;
            }
            total += bytes.len();
            entries.push(entry);
        }
        let index = WaterIndex::new(seed, stamp, entries).to_bytes();
        let path = dir.join("water.index");
        match keep(&path, &index) {
            Ok(()) => info!(dir = %dir.display(), bytes = total, "the baked rivers kept"),
            Err(error) => warn!(%error, path = %path.display(), "the baked rivers were not kept"),
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (world, seed, stamp, known, dir);
}

/// [`prepare_water_before_start`] over a source with the logical root
/// `assets/`, the installed one in the game. A source that holds the index
/// ([`WATER_INDEX`]) for this world has the masses it holds read first,
/// which is what makes the browser's start-up seconds and not a minute: the
/// bake then only finds what the files did not hold (a spawn elsewhere, or a
/// mass the bundle leaves to the fetch). Files for another world are refused
/// and the bake runs as it always did. A native start passes the `cache`
/// directory those files are kept in (see [`prepare_water_before_start`]).
///
/// # Errors
/// The registries did not load.
pub fn prepare_water_before_start_from(
    files: &Files,
    seed: Seed,
    prepare: &PrepareWater,
    cache: Option<&std::path::Path>,
) -> Result<(usize, f64), LoadError> {
    let started = strata_core::platform::time::Instant::now();
    let registries = strata_registry::load_from_source(&files.under("assets"))?;
    let params = world_params();
    let world = ProceduralWorld::with_params(seed, &registries, params);
    let stamp = strata_worldgen::version::water_stamp(seed, &params, &registries);
    let known = read_water(&world, seed, stamp, started, |path| files.read(path).ok());
    let masses = prepare_round(&world, prepare, strata_worldgen::hydrology::MAX_THREADS);
    if let Some(dir) = cache {
        write_water_cache(&world, seed, stamp, &known, dir);
    }
    Ok((masses, started.elapsed().as_secs_f64()))
}

/// The rivers baked ahead of the observers as they move: the load baked
/// them round its places; an observer that comes within a quarter of the
/// radius of the edge of every square baked so far sets off a bake round
/// itself, in the background on [`AHEAD_THREADS`] threads, so the streamer
/// meets no unbaked mass on its own threads. One bake at a time.
#[derive(Resource)]
struct WaterAhead {
    world: Arc<ProceduralWorld>,
    centres: Vec<(f64, f64)>,
    radius_m: f64,
    /// The whole view's square has not been asked for yet: the first
    /// observer's round is set off at once, not after a quarter of the
    /// radius of travel, because the load prepared less than the view.
    kick: bool,
    task: Option<Task<(f64, f64)>>,
    /// The browser's bake in slices (ticket 0015, step 3): the square being
    /// baked, its centre, and the cells not yet baked, the nearest to the
    /// observer last (popped from the back). Empty on a native build, which
    /// bakes the round whole on a task.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "only the browser's bake reads it")
    )]
    slices: Option<((f64, f64), Vec<(f64, f64)>)>,
    /// The baked masses' index and stamp: with it and a [`WaterFetcher`] the
    /// masses a round meets are fetched before they would be baked.
    water: Option<(Arc<WaterIndex>, u64)>,
    /// The fetch in flight, for the round an observer set off.
    fetching: Option<Fetching>,
    /// Masses whose file was asked for and was not there (or was refused),
    /// left to the bake.
    missing: Vec<[i32; 2]>,
}

/// What a [`MassFetcher`] answers for one path.
#[derive(Debug)]
pub enum MassFetch {
    /// Asked for, not here yet; ask again next frame.
    Pending,
    /// The file.
    Ready(Vec<u8>),
    /// There is no such file (or it could not be read): bake the mass.
    Missing,
}

/// Where a mass's file comes from when the source has not got it: the game
/// installs one over the asset server, so a browser's fetch is the engine's
/// own HTTP. A call asks for the file the first time and reports it after,
/// so it is cheap to ask every frame.
pub trait MassFetcher: Send + Sync + 'static {
    /// The state of the file at `path` (a [`MassEntry::path`]).
    fn fetch(&self, path: &str) -> MassFetch;
}

/// The installed [`MassFetcher`]. Without one the fetch ahead does nothing
/// and every mass is baked, as it was before the files were split.
#[derive(Resource, Clone)]
pub struct WaterFetcher(pub Arc<dyn MassFetcher>);

/// The masses one round waits for.
struct Fetching {
    wanted: Vec<MassEntry>,
    since: strata_core::platform::time::Instant,
}

/// How long a round waits for a file before it bakes the mass, seconds.
const FETCH_GIVE_UP_S: f64 = 30.0;

/// Threads a merged mass's tiles are computed on: the browser has one.
#[cfg(target_arch = "wasm32")]
const FETCH_THREADS: usize = 1;
#[cfg(not(target_arch = "wasm32"))]
const FETCH_THREADS: usize = 2;

/// One frame of the fetch: asks for each wanted mass and merges the first
/// that has arrived (a merge computes tiles, so one a frame), dropping those
/// with no file. True while some are still wanted.
fn fetch_step(ahead: &mut WaterAhead, fetcher: &dyn MassFetcher) -> bool {
    let Some((_, stamp)) = ahead.water.as_ref() else {
        ahead.fetching = None;
        return false;
    };
    let stamp = *stamp;
    let Some(fetching) = ahead.fetching.as_mut() else {
        return false;
    };
    let gave_up = fetching.since.elapsed().as_secs_f64() > FETCH_GIVE_UP_S;
    let mut merged = false;
    let mut keep = Vec::new();
    for entry in std::mem::take(&mut fetching.wanted) {
        if merged {
            keep.push(entry);
            continue;
        }
        match fetcher.fetch(&entry.path()) {
            MassFetch::Pending if !gave_up => keep.push(entry),
            MassFetch::Ready(bytes) => {
                merged = true;
                if let Err(error) =
                    ahead
                        .world
                        .hydrology()
                        .load_mass(&ahead.world, &bytes, stamp, FETCH_THREADS)
                {
                    warn!(%error, key = ?entry.key, "a fetched mass is not used, baking");
                    ahead.missing.push(entry.key);
                }
            }
            MassFetch::Pending | MassFetch::Missing => {
                warn!(key = ?entry.key, "a mass has no file to fetch, baking");
                ahead.missing.push(entry.key);
            }
        }
    }
    fetching.wanted = keep;
    if fetching.wanted.is_empty() {
        ahead.fetching = None;
        return false;
    }
    true
}

/// Starts the fetch for the square of half-side `ahead.radius_m` round
/// `(x, z)`: the masses the index names there that the cache does not hold
/// and no earlier fetch found missing. True when there are some.
fn begin_fetch(ahead: &mut WaterAhead, x: f64, z: f64) -> bool {
    let Some((index, _)) = ahead.water.clone() else {
        return false;
    };
    let r = ahead.radius_m;
    let wanted: Vec<MassEntry> = ahead
        .world
        .hydrology()
        .masses_wanted(&index, (x - r, z - r), (x + r, z + r))
        .into_iter()
        .filter(|m| !ahead.missing.contains(&m.key))
        .cloned()
        .collect();
    if wanted.is_empty() {
        return false;
    }
    info!(
        x,
        z,
        masses = wanted.len(),
        "fetching the rivers ahead of an observer"
    );
    ahead.fetching = Some(Fetching {
        wanted,
        since: strata_core::platform::time::Instant::now(),
    });
    true
}

/// Cells a round's side is cut into when the browser bakes it, one cell a
/// frame.
#[cfg(any(target_arch = "wasm32", test))]
const AHEAD_CELLS: u32 = 8;

/// The cells of the square of half-side `radius_m` round `(x, z)` that no
/// square of `baked` covers, as their centres, farthest from `(x, z)` first.
/// The browser has one thread, so a bake that runs in one poll would stall
/// a frame for as long as every unbaked mass of the round takes; a cell is
/// the smallest area one frame's bake is asked for.
#[cfg(any(target_arch = "wasm32", test))]
fn ahead_cells(x: f64, z: f64, radius_m: f64, baked: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let side = 2.0 * radius_m / f64::from(AHEAD_CELLS);
    let mut cells = Vec::new();
    for i in 0..AHEAD_CELLS {
        for j in 0..AHEAD_CELLS {
            let cx = x - radius_m + side * (f64::from(i) + 0.5);
            let cz = z - radius_m + side * (f64::from(j) + 0.5);
            let covered = baked.iter().any(|&(bx, bz)| {
                (cx - bx).abs() + side / 2.0 <= radius_m && (cz - bz).abs() + side / 2.0 <= radius_m
            });
            if !covered {
                cells.push((cx, cz));
            }
        }
    }
    cells.sort_by(|a, b| {
        let far = |c: &(f64, f64)| (c.0 - x).abs().max((c.1 - z).abs());
        far(b).total_cmp(&far(a))
    });
    cells
}

/// Threads a bake ahead of an observer runs on, beside the game.
#[cfg(not(target_arch = "wasm32"))]
const AHEAD_THREADS: usize = 2;

/// How far from every centre baked round an observer may go before the
/// rivers round it are baked, as a share of the radius: the far terrain
/// ring sees about two thirds of the radius out, so a bake set off at a
/// quarter of it has the rest to finish in.
const AHEAD_SHARE: f64 = 0.25;

/// Sets off a bake round the first observer that has moved far enough from
/// every square baked, and records one that finished.
fn prepare_water_ahead(
    mut ahead: ResMut<WaterAhead>,
    observers: Res<crate::Observers>,
    fetcher: Option<Res<WaterFetcher>>,
) {
    if let Some(fetcher) = &fetcher
        && ahead.fetching.is_some()
    {
        fetch_step(&mut ahead, fetcher.0.as_ref());
        return;
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(((x, z), cells)) = ahead.slices.as_mut() {
        // One cell a frame, on the one thread there is.
        if let Some((cx, cz)) = cells.pop() {
            let (world, side) = (
                Arc::clone(&ahead.world),
                ahead.radius_m / f64::from(AHEAD_CELLS),
            );
            world.prepare_water_on(cx, cz, side, 1);
        } else {
            let centre = (*x, *z);
            ahead.centres.push(centre);
            ahead.slices = None;
        }
        return;
    }
    if let Some(task) = ahead.task.as_mut() {
        let Some(centre) = block_on(poll_once(task)) else {
            return;
        };
        ahead.centres.push(centre);
        ahead.task = None;
    }
    if ahead.kick && fetcher.is_some() {
        let Some(at) = observers.positions.first() else {
            return;
        };
        ahead.kick = false;
        let (x, z) = (at.x(), at.z());
        if begin_fetch(&mut ahead, x, z) {
            return;
        }
    }
    let reach = ahead.radius_m * AHEAD_SHARE;
    let Some(at) = observers.positions.iter().find(|pos| {
        ahead.centres.iter().all(|&(x, z)| {
            // The squares baked are squares: an observer is inside one by
            // the larger of its offsets.
            (pos.x() - x).abs().max((pos.z() - z).abs()) > reach
        })
    }) else {
        return;
    };
    let (x, z) = (at.x(), at.z());
    // The files first, a round later the bake of what no file held: the
    // observer is still outside every square baked, so it comes back here.
    if fetcher.is_some() && begin_fetch(&mut ahead, x, z) {
        return;
    }
    info!(x, z, "baking the rivers ahead of an observer");
    #[cfg(target_arch = "wasm32")]
    {
        let cells = ahead_cells(x, z, ahead.radius_m, &ahead.centres);
        ahead.slices = Some(((x, z), cells));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let world = Arc::clone(&ahead.world);
        let radius = ahead.radius_m;
        ahead.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            world.prepare_water_on(x, z, radius, AHEAD_THREADS);
            (x, z)
        }));
    }
}

/// Loads the registries from [`WorldAssetRoot`], builds a
/// [`ProceduralWorld`] from [`WorldSeed`], and publishes [`Registries`] and
/// [`WorldSourceRes`] before releasing its [`LoadingHold`].
///
/// Takes the root as a field rather than a resource the app must remember
/// to insert, because a plugin with no asset root loads nothing and the
/// failure would show up three states later as an empty world.
#[derive(Debug, Clone)]
pub struct WorldLoadPlugin {
    /// The asset root holding `biomes/` and `terrain/`.
    pub asset_root: PathBuf,
    /// The content step's output root (`generated/`), holding the baked tree
    /// library. `None` loads no library.
    pub generated: Option<PathBuf>,
    /// The installed source, when the app reads through one: it serves
    /// `assets/...` and `generated/...` and the two paths above only say
    /// whether a library is wanted. `None` reads those directories.
    pub files: Option<Files>,
}

impl WorldLoadPlugin {
    /// A loader over `asset_root`.
    pub fn new(asset_root: impl Into<PathBuf>) -> Self {
        Self {
            asset_root: asset_root.into(),
            generated: None,
            files: None,
        }
    }

    /// Reads through `files` (logical roots `assets/` and `generated/`)
    /// instead of the directories, which is how the game reads on every
    /// target.
    #[must_use]
    pub fn with_files(mut self, files: Files) -> Self {
        self.files = Some(files);
        self
    }

    /// Also load the baked tree library from `generated`, the content step's
    /// output root. A missing or stale library is a load error, because a
    /// world placed without it would not be the world a server with it
    /// places.
    #[must_use]
    pub fn with_generated(mut self, generated: impl Into<PathBuf>) -> Self {
        self.generated = Some(generated.into());
        self
    }
}

/// The content step's output, as a source whose paths are relative to it,
/// that [`WorldLoadPlugin`] reads the tree library and the settlement
/// layouts from, if any; the physics reads baked buildings from it too.
#[derive(Resource, Debug, Clone)]
pub struct GeneratedRoot(pub Option<Files>);

impl Plugin for WorldLoadPlugin {
    fn build(&self, app: &mut App) {
        // Take the hold at build time: the state machine only checks it in
        // `LoadingRegistries`, and a hold taken by a system in `Boot` would
        // race the same frame's `leave_boot`.
        app.world_mut()
            .get_resource_or_insert_with(LoadingHold::default)
            .0 += 1;
        let (assets, generated) = match &self.files {
            Some(files) => (
                files.under("assets"),
                self.generated.as_ref().map(|_| files.under("generated")),
            ),
            None => (
                Files::dir(&self.asset_root),
                self.generated.as_ref().map(Files::dir),
            ),
        };
        app.insert_resource(WorldAssetRoot(assets))
            .insert_resource(GeneratedRoot(generated))
            .init_resource::<TreeLibraryRes>()
            .init_resource::<Settlements>()
            .init_resource::<crate::sites::NearSites>()
            .init_resource::<crate::crossings::NearCrossings>()
            .init_resource::<crate::roadside::NearRoadside>()
            .add_systems(OnEnter(AppState::LoadingRegistries), start_world_load)
            .add_systems(
                Update,
                (
                    finish_world_load.run_if(in_state(AppState::LoadingRegistries)),
                    crate::sites::refresh_near_sites.run_if(in_state(AppState::InWorld)),
                    crate::crossings::refresh_near_crossings.run_if(in_state(AppState::InWorld)),
                    crate::roadside::refresh_near_roadside.run_if(in_state(AppState::InWorld)),
                    prepare_water_ahead
                        .run_if(in_state(AppState::InWorld))
                        .run_if(resource_exists::<WaterAhead>),
                ),
            );
    }
}

/// What the load task hands back.
struct LoadedWorld {
    registries: Arc<GameRegistries>,
    source: Arc<dyn WorldSource>,
    /// The same world, for the rivers' bake ahead of the observers.
    procedural: Arc<ProceduralWorld>,
    /// Where the load baked them, when the app asked for a bake ahead of
    /// its observers (a game or a server; not a test's app).
    prepare: Option<PrepareWater>,
    /// The index of the baked masses and the stamp their files carry, when
    /// the source holds an index for this world: what the fetch ahead asks.
    water: Option<(Arc<WaterIndex>, u64)>,
    library: Option<Arc<TreeLibrary>>,
    settlements: Vec<PlacedSettlement>,
    /// The site kinds' building groups (ticket 0005, step 4).
    site_groups: crate::sites::SiteGroups,
    /// The crossings' kit (ticket 0005, step 7).
    crossings: crate::crossings::Crossings,
    /// The roadside kit (ticket 0005, step 8).
    roadside: crate::roadside::Roadside,
}

/// One settlement as the world placed it: its site, from the seed, and its
/// buildings, from the content step's layout.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSettlement {
    /// Where the world put it, its platform and its road.
    pub site: Site,
    /// Which buildings stand where in its frame, or `None` when the content
    /// step has not written its layout: the ground is flattened either way.
    pub layout: Option<SettlementLayout>,
}

impl PlacedSettlement {
    /// A point of the settlement's frame in the world: `x` and `z` in the
    /// frame, `y` a world height.
    #[must_use]
    pub fn to_world(&self, local: [f64; 3]) -> strata_core::WorldPos {
        let [x, y, z] = self.site.to_world(local);
        strata_core::WorldPos::new(x, y, z)
    }

    /// A yaw given in the settlement's frame, as a world yaw.
    #[must_use]
    pub fn world_yaw(&self, local: f64) -> f64 {
        self.site.world_yaw(local)
    }
}

/// Every settlement of the world, in the registries' order. Published with
/// the world, so the server's colliders and the client's buildings come
/// from the same list.
#[derive(Resource, Debug, Clone, Default)]
pub struct Settlements(pub Vec<PlacedSettlement>);

impl Settlements {
    /// A named place of the first settlement, world `(x, z)`: `city` is its
    /// layout's street spawn, `road` the far end of its road in the
    /// wilderness, where the slice's route starts.
    #[must_use]
    pub fn place(&self, name: &str) -> Option<(f64, f64)> {
        let settlement = self.0.first()?;
        match name {
            "city" => {
                let layout = settlement.layout.as_ref()?;
                let at = settlement.to_world(layout.spawn);
                Some((at.x(), at.z()))
            }
            "road" => settlement.site.road.points.last().map(|p| (p[0], p[1])),
            _ => None,
        }
    }
}

/// Why the world could not be loaded.
#[derive(Debug, thiserror::Error)]
enum WorldLoadError {
    #[error(transparent)]
    Registries(#[from] LoadError),
    #[error(transparent)]
    Library(#[from] strata_flora::library::LibraryError),
}

/// The load in flight.
#[derive(Resource)]
struct WorldLoading(Task<Result<LoadedWorld, WorldLoadError>>);

/// Reads the registries and builds the world, off the main thread.
fn start_world_load(
    mut commands: Commands,
    root: Res<WorldAssetRoot>,
    generated: Res<GeneratedRoot>,
    seed: Option<Res<WorldSeed>>,
    prepare: Option<Res<PrepareWater>>,
) {
    let seed = if let Some(seed) = seed {
        seed.0
    } else {
        warn!("no WorldSeed resource; building the world from seed 0");
        Seed(0)
    };
    let root = root.0.clone();
    let generated: Option<Files> = generated.0.clone();
    // Without a `PrepareWater` (a test's app) only the settlements' own
    // country, whose mass every walk from them meets first.
    let ahead = prepare.is_some();
    let prepare = prepare.map_or(
        PrepareWater {
            places: Vec::new(),
            radius_m: TEST_PREPARE_RADIUS_M,
            ahead_radius_m: None,
        },
        |prepare| prepare.clone(),
    );
    info!(seed = seed.0, "loading the world");
    // `get_or_init` so a test app without `TaskPoolPlugin` still loads;
    // under Bevy's plugins the pool already exists and the closure is
    // never called.
    let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
        let registries = strata_registry::load_from_source(&root)?;
        let source = ProceduralWorld::with_params(seed, &registries, world_params());
        // The rivers before the world is shown, once, while the loading
        // hold keeps every system that samples waiting. A game or a server
        // has baked them already, before its task pools started
        // ([`prepare_water_before_start`]), and every world of the same
        // inputs shares that bake, so this finds them done; a test's app
        // bakes its settlements' country here.
        let started = strata_core::platform::time::Instant::now();
        let masses = prepare_round(&source, &prepare, LOADER_BAKE_THREADS);
        info!(
            masses,
            seconds = started.elapsed().as_secs_f64(),
            "prepared the rivers"
        );
        let source = Arc::new(source);
        let water = generated.as_ref().and_then(|generated| {
            let bytes = generated.read("hydrology/water.index").ok()?;
            let stamp = strata_worldgen::version::water_stamp(seed, &world_params(), &registries);
            WaterIndex::read(&bytes, seed, stamp)
                .ok()
                .map(|index| (Arc::new(index), stamp))
        });
        let library = generated
            .as_ref()
            .map(|generated| TreeLibrary::read_from(generated).map(Arc::new))
            .transpose()?;
        let settlements = registries
            .settlements
            .iter()
            .zip(source.sites())
            .map(|(def, site)| {
                let layout = generated
                    .as_ref()
                    .and_then(|generated| read_layout(generated, &def.layout))
                    .filter(|layout| {
                        let fits = layout.ground == site.fingerprint();
                        if !fits {
                            warn!(
                                "the {} layout was fitted to other ground; its buildings are left out \
                                 until `just content` fits it again",
                                def.id
                            );
                        }
                        fits
                    });
                PlacedSettlement {
                    site: site.clone(),
                    layout,
                }
            })
            .collect();
        let site_groups = crate::sites::SiteGroups::read(
            Arc::clone(&source),
            generated.as_ref().map(|g| g as &dyn AssetSource),
        );
        let crossings =
            crate::crossings::Crossings::read(
                Arc::clone(&source),
                generated.as_ref().map(|g| g as &dyn AssetSource),
            );
        let roadside = crate::roadside::Roadside::read(
            Arc::clone(&source),
            generated.as_ref().map(|g| g as &dyn AssetSource),
            crossings.kit.clone(),
        );
        Ok(LoadedWorld {
            site_groups,
            crossings,
            roadside,
            registries: Arc::new(registries),
            source: Arc::clone(&source) as Arc<dyn WorldSource>,
            procedural: source,
            prepare: ahead.then_some(prepare),
            water,
            library,
            settlements,
        })
    });
    commands.insert_resource(WorldLoading(task));
}

/// Every settlement layout's plants, resolved: a species the registry does not
/// know is left out with a warning.
fn planted(
    registries: &strata_registry::GameRegistries,
    settlements: &[PlacedSettlement],
) -> Vec<strata_flora::Planted> {
    let mut out = Vec::new();
    for settlement in settlements {
        let Some(layout) = &settlement.layout else {
            continue;
        };
        for plant in &layout.plants {
            let species = strata_registry::Namespaced::parse(&plant.species)
                .ok()
                .and_then(|name| registries.flora_id(&name));
            let Some(species) = species else {
                warn!("{} plants an unknown species {}", layout.id, plant.species);
                continue;
            };
            let at = settlement.to_world(plant.position);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "plant counts are thousands"
            )]
            let index = out.len() as u32;
            out.push(strata_flora::Planted {
                species,
                at: [at.x(), at.y(), at.z()],
                yaw: plant.yaw,
                scale: plant.scale,
                variant: plant.variant,
                index,
            });
        }
    }
    out
}

/// A settlement's layout, or `None` with a warning when it is missing or
/// unreadable: the world still flattens its ground, and the city is simply
/// not built, which a missing content step makes plain.
fn read_layout(generated: &dyn AssetSource, path: &str) -> Option<SettlementLayout> {
    let text = crate::sites::read_text(generated, path)
        .map_err(|error| {
            warn!("no settlement layout at {path}: {error}; run `just content`");
        })
        .ok()?;
    ron::from_str(&text)
        .map_err(|error| warn!("settlement layout {path}: {error}"))
        .ok()
}

/// Publishes the world once the task is done and releases the hold; on
/// failure logs the error and asks the app to exit.
fn finish_world_load(
    mut commands: Commands,
    loading: Option<ResMut<WorldLoading>>,
    mut hold: ResMut<LoadingHold>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut loading) = loading else {
        return;
    };
    let Some(result) = block_on(poll_once(&mut loading.0)) else {
        return;
    };
    commands.remove_resource::<WorldLoading>();
    match result {
        Ok(world) => {
            info!(
                biomes = world.registries.biomes.len(),
                materials = world.registries.materials.len(),
                "world ready"
            );
            commands.insert_resource(crate::footprints::PlantedFlora(Arc::new(planted(
                &world.registries,
                &world.settlements,
            ))));
            commands.insert_resource(Registries(world.registries));
            commands.insert_resource(WorldSourceRes(world.source));
            commands.insert_resource(ProceduralWorldRes(Arc::clone(&world.procedural)));
            if let Some(prepare) = &world.prepare {
                commands.insert_resource(WaterAhead {
                    centres: strata_worldgen::hydrology::water_places(
                        &world.procedural,
                        &prepare.area(),
                    ),
                    radius_m: prepare.ahead_radius_m.unwrap_or(prepare.radius_m),
                    kick: prepare.ahead_radius_m.is_some_and(|r| r > prepare.radius_m),
                    world: Arc::clone(&world.procedural),
                    task: None,
                    slices: None,
                    water: world.water.clone(),
                    fetching: None,
                    missing: Vec::new(),
                });
            }
            if let Some(library) = &world.library {
                info!(
                    species = library.species.len(),
                    version = library.version,
                    "tree library ready"
                );
            }
            commands.insert_resource(TreeLibraryRes(world.library));
            for settlement in &world.settlements {
                info!(
                    id = %settlement.site.id,
                    x = settlement.site.centre[0],
                    z = settlement.site.centre[1],
                    height = settlement.site.height,
                    buildings = settlement.layout.as_ref().map_or(0, |l| l.buildings.len()),
                    "settlement placed"
                );
            }
            commands.insert_resource(Settlements(world.settlements));
            commands.insert_resource(world.site_groups);
            commands.insert_resource(world.crossings);
            commands.insert_resource(world.roadside);
            hold.0 = hold.0.saturating_sub(1);
        }
        Err(err) => {
            error!("the world could not be loaded: {err}");
            exit.write(AppExit::error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_bakes_only_cells_nothing_has_covered() {
        let r = 128_000.0;
        // Nothing baked: every cell, the near ones last.
        let all = ahead_cells(0.0, 0.0, r, &[]);
        assert_eq!(all.len(), (AHEAD_CELLS * AHEAD_CELLS) as usize);
        let far = |c: &(f64, f64)| c.0.abs().max(c.1.abs());
        assert!(far(&all[0]) >= far(all.last().expect("cells")));
        // A round 32 km east of a baked one: the strip beyond the old edge.
        let strip = ahead_cells(32_000.0, 0.0, r, &[(0.0, 0.0)]);
        assert!(!strip.is_empty() && strip.len() < all.len());
        assert!(strip.iter().all(|c| c.0 + r / f64::from(AHEAD_CELLS) > r));
        // Inside a baked square: nothing.
        assert!(ahead_cells(0.0, 0.0, r, &[(0.0, 0.0)]).is_empty());
    }

    use crate::AppPlugin;
    use bevy::state::state::State;

    fn assets() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    fn state(app: &App) -> AppState {
        *app.world().resource::<State<AppState>>().get()
    }

    #[test]
    fn the_world_is_published_before_entering_it() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            AppPlugin,
            WorldLoadPlugin::new(assets()),
        ));
        app.insert_resource(WorldSeed(Seed(42)));
        // Wall-clock, not frames: the load runs on a pool a busy test run
        // shares, and a debug world chooses its settlement sites as it loads.
        let started = strata_core::platform::time::Instant::now();
        while state(&app) != AppState::InWorld {
            app.update();
            assert!(
                started.elapsed() < std::time::Duration::from_secs(120),
                "never entered the world"
            );
        }
        let source = app.world().resource::<WorldSourceRes>();
        let registries = app.world().resource::<Registries>();
        assert!(registries.0.biomes.len() >= 7);
        // The seed-42 world pinned by the worldgen snapshots.
        let sample = source.source().sample(0.0, 0.0);
        assert!(sample.height < 0.0, "{sample:?}");
    }

    #[test]
    fn a_missing_asset_root_exits_with_an_error() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            AppPlugin,
            WorldLoadPlugin::new("/nonexistent/strata-assets"),
        ));
        let mut exited = false;
        for _ in 0..200 {
            app.update();
            if app
                .world()
                .resource::<bevy::ecs::message::Messages<AppExit>>()
                .iter_current_update_messages()
                .any(AppExit::is_error)
            {
                exited = true;
                break;
            }
        }
        assert!(exited, "no AppExit::Error was written");
        assert_ne!(state(&app), AppState::InWorld);
    }

    /// A fetcher whose answers the test writes.
    struct Script(std::sync::Mutex<Vec<MassFetch>>);

    impl MassFetcher for Script {
        fn fetch(&self, _path: &str) -> MassFetch {
            self.0
                .lock()
                .expect("the test's own lock")
                .pop()
                .unwrap_or(MassFetch::Pending)
        }
    }

    #[test]
    fn a_mass_with_no_usable_file_is_left_to_the_bake() {
        let registries = strata_registry::load_from_dir(assets()).expect("assets");
        let world = Arc::new(ProceduralWorld::with_params(
            Seed(7),
            &registries,
            world_params(),
        ));
        let entry = |key: [i32; 2]| MassEntry {
            key,
            capped: false,
            nodes: 1,
            bytes: 1,
            bounds: [key, key],
            tiles: vec![[key[0].div_euclid(128), key[1].div_euclid(128)]],
        };
        let index = WaterIndex::new(Seed(7), 1, vec![entry([0, 0]), entry([1, 1])]);
        let mut ahead = WaterAhead {
            world,
            centres: Vec::new(),
            radius_m: 10_000.0,
            kick: false,
            task: None,
            slices: None,
            water: Some((Arc::new(index), 1)),
            fetching: None,
            missing: Vec::new(),
        };
        assert!(begin_fetch(&mut ahead, 0.0, 0.0));
        assert_eq!(ahead.fetching.as_ref().map(|f| f.wanted.len()), Some(2));
        // The first is still on its way, the second has no file: the second is
        // dropped for good and the round goes on waiting for the first.
        let fetcher = Script(std::sync::Mutex::new(vec![
            MassFetch::Missing,
            MassFetch::Pending,
        ]));
        assert!(fetch_step(&mut ahead, &fetcher));
        assert_eq!(ahead.missing, vec![[1, 1]]);
        // A file that is not a mass is refused and left to the bake as well.
        let fetcher = Script(std::sync::Mutex::new(vec![MassFetch::Ready(vec![1, 2, 3])]));
        assert!(!fetch_step(&mut ahead, &fetcher));
        assert!(ahead.fetching.is_none());
        assert_eq!(ahead.missing.len(), 2);
        // Nothing is asked twice: both are known to be missing now.
        assert!(!begin_fetch(&mut ahead, 0.0, 0.0));
    }
}
