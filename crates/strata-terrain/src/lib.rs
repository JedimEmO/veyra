//! Terrain: wanted regions in, meshes out.
//!
//! The streamer decides which regions exist and spawns an entity per region
//! (`strata-stream`); this crate answers each [`RegionLoad`] with a mesh.
//! A task per region on the compute pool samples the world on the region's
//! own grid plus a ring for normals and builds the vertex streams
//! ([`mesh::build_region_mesh`], a pure function) and packs them for the
//! GPU ([`store::pack_region`]); the collector writes the result into a slot
//! of the region store under a per-frame budget of bytes, gives the entity
//! the shared region mesh with its slot in `MeshTag`, and reports
//! [`RegionReady`]. Terrain is not a Bevy `Mesh` per region: Bevy's mesh
//! upload interleaved one at about 3.9 ms on the render thread, where a slot
//! is one `write_buffer` (the store module's docs). A task whose
//! region is unloaded is dropped, which cancels it (ADR 0003). Region
//! entities carry a `WorldPosition` and get their `Transform` from the
//! render origin, never the other way round (ADR 0002): the mesh is built
//! relative to the region's corner and this crate writes no transform.
//!
//! Loads queue rather than all going to the pool at once: a fast flight
//! wants regions faster than the upload budget passes them, and a mesh
//! built far ahead of its turn is stale by then (streaming plan, cost 7).
//! At most [`TerrainConfig::max_queued`] meshes are on the pool or waiting
//! for the budget; the rest wait as loads, and only while their region is
//! still wanted. Holes and merges go first, then the nearest refinement; a
//! split's children rank as one unit, so the parent the no-hole rule holds
//! for them is released as soon as it can be (see `Rank`).
//!
//! ```text
//! Update, after StreamSet::Diff, in InWorld
//!   TerrainSet::Dispatch   RegionLoad -> queue; RegionUnload or unwanted -> drop;
//!                          nearest queued loads -> tasks, up to the cap
//!   TerrainSet::Collect    nearest finished tasks -> store slot + shared Mesh3d + MeshTag + material
//!                          + Aabb + TerrainRegion, RegionReady
//! ```
//!
//! Without the store (a headless app that still adds this plugin, and no
//! material plugin) the collector attaches only the [`TerrainRegion`]
//! summary and still reports
//! ready, so the whole pipeline runs under `MinimalPlugins` in a test. The
//! material comes from [`material::TerrainMaterialPlugin`], which needs the
//! renderer and is added by the client plugin group, not here.

// Falls drawn over the river's ramp (ticket 0005, step 8).
pub mod falls;
pub mod material;
pub mod mesh;
pub mod paint;
// Irradiance probes per streamed region (lighting plan phase 3b).
pub mod probes;
pub mod river;
pub mod store;
pub mod water;

use std::cmp::Ordering as Order;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bevy::app::{App, Plugin, Update};
use bevy::asset::Assets;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::light::NotShadowCaster;
use bevy::math::Vec3;
use bevy::mesh::{Mesh, Mesh3d, MeshTag};
use bevy::pbr::MeshMaterial3d;
use bevy::render::renderer::{RenderDevice, RenderQueue};
// Fixed-seed foldhash: the queue hashes a few thousand coords a frame.
use bevy::platform::collections::{HashMap, HashSet};
use bevy::state::condition::in_state;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::time::{Real, Time};
use strata_core::platform::time::Instant;
use strata_core::{RegionCoord, WorldPos};
use strata_sim::{AppState, Observers, Registries, WorldSourceRes};
use strata_stream::{
    ExternalRegionBuilder, LoadedRegions, Region, RegionLoad, RegionReady, RegionUnload,
    StreamConfig, StreamSet, WantedRegions, nearest_distance,
};
use strata_worldgen::Sample;
use tracing::{error, info, warn};

pub use material::{TerrainLook, TerrainMaterial, TerrainMaterialHandle, TerrainMaterialPlugin};
pub use mesh::{
    BiomeLayers, RegionMesh, build_region_mesh, build_region_mesh_painted, build_region_mesh_pinned,
};
pub use river::{RegionWater, RiverConfig, RiverMaterial, RiverMesh, build_river_mesh};
pub use store::{TerrainSlot, TerrainStore};
pub use water::{WaterConfig, WaterMaterial, WaterPlane, WaterPlugin};

/// Marks a `Camera3d` that does not look at the world: a model turning on
/// a screen, a portrait render.
///
/// The terrain dresses and reads cameras by `Camera3d` alone, because the
/// world has one. A second camera drawing something else would otherwise
/// be given the water's depth prepass and the probes' cluster bound, and,
/// drawn before the world's, be the one the probes' fallback and the
/// foliage's levels follow. The crates above the terrain tell the world's
/// camera from another by this too.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OffWorld;

/// The terrain sets, in order, in `Update`, after streaming has decided.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainSet {
    /// Queue loads, drop unloaded and unwanted work, start the nearest.
    Dispatch,
    /// Poll finished tasks, insert meshes and materials, report ready.
    Collect,
}

/// The knobs of the mesher. Insert before [`TerrainPlugin`] to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct TerrainConfig {
    /// Skirt depth as a fraction of the region's side. A coarse neighbour's
    /// straight edge can undercut a fine edge by a good part of the
    /// terrain's relief over one coarse step, and that grows with the node,
    /// so the skirt scales with it.
    pub skirt_fraction: f32,
    /// At most this many finished regions are inserted per frame. Each
    /// region is a slot write, an entity insert and a `RegionReady` the
    /// streamer and the probes answer; the byte budget is the bound in
    /// practice, and this keeps a frame's inserts to what it pays for.
    pub max_collect_per_frame: usize,
    /// At most this many bytes of region records are written to the store
    /// per frame, though one region always goes through: a slot and its
    /// header, 253 532 bytes, so 4 MiB is sixteen regions. A write is a copy
    /// into the staging belt at a few tenths of a millisecond a megabyte,
    /// where Bevy's mesh path cost 6.6 ms a megabyte and held the budget to
    /// two regions a frame (streaming plan, phase 5). A region's rivers and
    /// lakes still go through that path ([`river`]), so their bytes count
    /// [`MESH_PATH_WEIGHT`] times over.
    pub max_upload_bytes_per_frame: usize,
    /// At most this many meshes on the pool or finished and waiting for the
    /// upload budget; further loads wait unbuilt. About a second of what the
    /// walking fixtures upload (11 to 16 a second at the bench's paced
    /// frame) and four times the async pool's four threads, so they never
    /// idle while loads wait. Without a cap the flight held 450 finished
    /// meshes (0.6 MB each) and dropped 61 a second unused, because the
    /// observer had moved on before their turn came; at 32 it still dropped
    /// 6 a second, at 16 about 2, with the same meshes uploaded a second.
    pub max_queued: usize,
    /// Whether a region's task also builds its rivers' and lakes' water
    /// ([`river`]), drawn where there is a renderer. Off for a benchmark of
    /// the ground alone (`--no-rivers`).
    pub water: bool,
    /// Whether a region's task also gathers the rivers and roads its
    /// fragments paint at distance ([`paint`]), where there is a renderer.
    /// Off, the terrain's fragment shader is compiled without the paint
    /// (`TERRAIN_FAR_PAINT`) and the coarse levels keep every road in their
    /// splat: what the paint costs, about 0.15 ms of the opaque pass on the
    /// river flight on an RTX 4090, is then nothing (`--no-far-paint`).
    pub paint: bool,
    /// Whether the falls are drawn over the river's ramp ([`falls`]): a
    /// sheet, foam and spray for each fall within 18 km, where there is a
    /// renderer and [`water`](Self::water) is on (`--no-falls`).
    pub falls: bool,
}

/// What a byte uploaded through Bevy's mesh path costs against one written
/// to the store, for [`TerrainConfig::max_upload_bytes_per_frame`]: 6.6 ms
/// a megabyte against about three tenths (`docs/perf/streaming-2026-10-04.md`),
/// rounded up.
pub const MESH_PATH_WEIGHT: u64 = 24;

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            skirt_fraction: 0.05,
            max_collect_per_frame: 16,
            max_upload_bytes_per_frame: 4 << 20,
            max_queued: 16,
            water: true,
            falls: true,
            paint: true,
        }
    }
}

/// What the mesher knows about a region once it is built. On the region
/// entity next to its `Region`; a HUD, a physics adapter or a test reads it
/// without touching the mesh.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TerrainRegion {
    /// Vertices in the mesh, skirts included.
    pub vertices: u32,
    /// Lowest surface height, metres.
    pub min_height: f32,
    /// Highest surface height, metres.
    pub max_height: f32,
}

/// Counters for the HUD, the bench and the tests.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainStats {
    /// Tasks held: on the compute pool or finished and waiting for the
    /// upload budget. At most [`TerrainConfig::max_queued`].
    pub in_flight: usize,
    /// Of `in_flight`, the meshes finished and waiting for the budget.
    pub waiting: usize,
    /// Loads not yet started: waiting for room under the cap, or for their
    /// region to be wanted again.
    pub queued: usize,
    /// Meshes inserted since the start.
    pub built: u64,
    /// Meshes inserted this frame.
    pub built_this_frame: usize,
    /// Tasks dropped before their mesh was inserted: the region unloaded or
    /// left the wanted set.
    pub cancelled: u64,
    /// Of `cancelled`, the tasks that had already finished: a mesh built
    /// and never used.
    pub dropped: u64,
    /// Compute-pool time spent meshing since the start, in microseconds,
    /// counted when a task's build returns (a cancelled task that never ran
    /// costs nothing and counts nothing).
    pub build_micros: u64,
    /// Bytes written to the region store since the start.
    pub uploaded_bytes: u64,
    /// Store slots held by drawn regions and by freed ones in quarantine.
    pub slots: u32,
    /// Store slots the buffers hold.
    pub slot_capacity: u32,
    /// Regions whose task built water since the start ([`river`]).
    pub water_built: u64,
    /// Vertices of the water those regions built.
    pub water_vertices: u64,
    /// Compute-pool time spent building water since the start, in
    /// microseconds: the share of `build_micros` the rivers add.
    pub water_micros: u64,
    /// Bytes of water uploaded through Bevy's mesh path since the start.
    pub water_bytes: u64,
    /// Compute-pool time spent gathering the far paint since the start, in
    /// microseconds: the share of `build_micros` the paint adds.
    pub paint_micros: u64,
    /// Regions whose task gathered paint since the start.
    pub paint_regions: u64,
    /// Strokes those regions painted.
    pub paint_strokes: u64,
    /// Lines left out of a region's paint because its slot was full.
    pub paint_dropped: u64,
    /// The most strokes any one region's bucket listed, against the
    /// shader's bound ([`paint::BUCKET_MOST`]).
    pub paint_fullest: u64,
    /// The most water bytes any one frame uploaded, and how many regions
    /// carried them: what the mesh path's worst frame pays for the rivers.
    pub water_frame_max: (u64, u32),
}

/// Something that finishes on its own: a pool task, or a stand-in in the
/// tests of the queue.
trait Finish {
    fn finished(&self) -> bool;
}

impl<T> Finish for Task<T> {
    fn finished(&self) -> bool {
        self.is_finished()
    }
}

/// Where a region stands in the queue.
///
/// Regions that only refine ground already on screen go last. A region
/// with nothing loaded over it fills a hole, and a merge parent releases
/// the finer regions the no-hole rule holds until it is ready; when uploads
/// cannot keep up (a flight wants about 87 regions a second and the budget
/// passes about 35), refinement near the observer would otherwise take
/// every slot, the merges behind it would never be built, and the regions
/// they replace would stay resident until the horizon passed them (the
/// first measured flight held 5 500 regions after a minute). A merge costs
/// one mesh for every four a split costs, so this takes about a fifth of
/// the uploads.
///
/// Then nearest first: a split's children share their unit's distance and
/// coordinate, so they sit next to each other and finish together; within
/// a unit the nearer region goes first; the coordinates break every tie, so
/// the order is a function of the inputs alone and two runs with the same
/// history agree.
#[derive(Debug, Clone, Copy)]
struct Rank {
    /// Whether this region only refines loaded ground and releases nothing.
    refines: bool,
    /// The nearest observer's distance to the nearest unbuilt member of
    /// this region's unit.
    unit_distance: f64,
    /// The loaded region this one and its siblings replace, or the region
    /// itself when it replaces nothing.
    unit: RegionCoord,
    /// The nearest observer's distance to this region.
    distance: f64,
    coord: RegionCoord,
}

impl Ord for Rank {
    fn cmp(&self, other: &Self) -> Order {
        self.refines
            .cmp(&other.refines)
            .then(self.unit_distance.total_cmp(&other.unit_distance))
            .then(self.unit.cmp(&other.unit))
            .then(self.distance.total_cmp(&other.distance))
            .then(self.coord.cmp(&other.coord))
    }
}

impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<Order> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Rank {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Order::Equal
    }
}

impl Eq for Rank {}

/// The unit `coord` is built with: its nearest loaded ancestor, which the
/// no-hole rule keeps on screen until every wanted region under it is
/// ready, or `coord` itself when no loaded region covers it.
fn unit_of(coord: RegionCoord, root_lod: u8, loaded: impl Fn(RegionCoord) -> bool) -> RegionCoord {
    (coord.lod.saturating_add(1)..=root_lod)
        .filter_map(|lod| coord.ancestor_at(lod))
        .find(|ancestor| loaded(*ancestor))
        .unwrap_or(coord)
}

/// The wanted regions that would release loaded ones once ready: the
/// nearest wanted ancestor of every loaded region that is not wanted itself
/// (a merge in progress). Into `out`, cleared first.
fn releasing(
    loaded: impl Iterator<Item = RegionCoord>,
    wanted: &HashSet<RegionCoord>,
    root_lod: u8,
    out: &mut HashSet<RegionCoord>,
) {
    out.clear();
    for coord in loaded {
        if wanted.contains(&coord) {
            continue;
        }
        if let Some(ancestor) = (coord.lod.saturating_add(1)..=root_lod)
            .filter_map(|lod| coord.ancestor_at(lod))
            .find(|a| wanted.contains(a))
        {
            out.insert(ancestor);
        }
    }
}

/// Ranks every unbuilt wanted region together, so a unit's distance is
/// that of its nearest member whether that member is queued or on the pool.
fn rank_all(
    coords: &[RegionCoord],
    observers: &[WorldPos],
    root_lod: u8,
    loaded: impl Fn(RegionCoord) -> bool,
    releases: impl Fn(RegionCoord) -> bool,
) -> Vec<Rank> {
    let mut ranks: Vec<Rank> = coords
        .iter()
        .map(|&coord| {
            let distance = nearest_distance(observers, coord);
            let unit = unit_of(coord, root_lod, &loaded);
            Rank {
                refines: unit != coord && !releases(coord),
                unit_distance: distance,
                unit,
                distance,
                coord,
            }
        })
        .collect();
    let mut nearest: HashMap<RegionCoord, f64> = HashMap::new();
    for rank in &ranks {
        nearest
            .entry(rank.unit)
            .and_modify(|d| *d = d.min(rank.distance))
            .or_insert(rank.distance);
    }
    for rank in &mut ranks {
        rank.unit_distance = nearest[&rank.unit];
    }
    ranks
}

/// A load on its way: on the pool, or finished and waiting.
struct Job<T> {
    coord: RegionCoord,
    task: T,
    /// This frame's rank, set in dispatch and read by the collector.
    rank: Rank,
}

/// What [`Queue::drop_unwanted`] and [`Queue::unload`] threw away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Dropped {
    /// Jobs dropped, finished or not.
    cancelled: u64,
    /// Of those, the ones that had finished.
    finished: u64,
}

impl Dropped {
    fn count(&mut self, finished: bool) {
        self.cancelled += 1;
        self.finished += u64::from(finished);
    }
}

/// The terrain queue: loads waiting for a start, and the jobs started.
/// Kept apart from the systems so its rules are tested without an app.
struct Queue<T> {
    /// Region entities whose load has not been started, with their coord.
    pending: HashMap<Entity, RegionCoord>,
    /// Region entities with a job, by entity.
    jobs: HashMap<Entity, Job<T>>,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            jobs: HashMap::new(),
        }
    }
}

impl<T: Finish> Queue<T> {
    fn load(&mut self, coord: RegionCoord, entity: Entity) {
        self.pending.insert(entity, coord);
    }

    /// Forgets `entity`; drops its job, if it had one.
    fn unload(&mut self, entity: Entity, dropped: &mut Dropped) {
        self.pending.remove(&entity);
        if let Some(job) = self.jobs.remove(&entity) {
            dropped.count(job.task.finished());
        }
    }

    /// Drops every job whose region has left the wanted set and queues its
    /// load again. Its entity stays until the streamer unloads it, which it
    /// does once whatever replaces it is ready, and nothing waits on the
    /// readiness of a region that is not wanted; if it is wanted again
    /// first, it is started again.
    fn drop_unwanted(&mut self, wanted: &HashSet<RegionCoord>, dropped: &mut Dropped) {
        let pending = &mut self.pending;
        self.jobs.retain(|entity, job| {
            if wanted.contains(&job.coord) {
                return true;
            }
            dropped.count(job.task.finished());
            pending.insert(*entity, job.coord);
            false
        });
    }

    /// Ranks the jobs and the wanted pending loads together, stores each
    /// job's rank, and returns the loads to start now, best first: as many
    /// as the cap leaves room for.
    fn rank(
        &mut self,
        cap: usize,
        wanted: &HashSet<RegionCoord>,
        rank_all: impl FnOnce(&[RegionCoord]) -> Vec<Rank>,
    ) -> Vec<(Entity, RegionCoord)> {
        let mut entities: Vec<Entity> = Vec::with_capacity(self.jobs.len() + self.pending.len());
        let mut coords: Vec<RegionCoord> = Vec::with_capacity(entities.capacity());
        for (entity, job) in &self.jobs {
            entities.push(*entity);
            coords.push(job.coord);
        }
        let jobs = entities.len();
        for (entity, coord) in &self.pending {
            if wanted.contains(coord) {
                entities.push(*entity);
                coords.push(*coord);
            }
        }
        let ranks = rank_all(&coords);
        for (entity, rank) in entities[..jobs].iter().zip(&ranks) {
            if let Some(job) = self.jobs.get_mut(entity) {
                job.rank = *rank;
            }
        }
        let room = cap.saturating_sub(jobs);
        let mut start: Vec<(Rank, Entity)> = ranks[jobs..]
            .iter()
            .copied()
            .zip(entities[jobs..].iter().copied())
            .collect();
        if room < start.len() {
            if room == 0 {
                return Vec::new();
            }
            start.select_nth_unstable(room - 1);
            start.truncate(room);
        }
        start.sort_unstable();
        start
            .into_iter()
            .map(|(rank, entity)| (entity, rank.coord))
            .collect()
    }

    fn start(&mut self, entity: Entity, coord: RegionCoord, task: T, rank: Rank) {
        self.pending.remove(&entity);
        self.jobs.insert(entity, Job { coord, task, rank });
    }

    /// The finished jobs in upload order, by the rank of this frame.
    fn finished(&self) -> Vec<Entity> {
        let mut finished: Vec<(Rank, Entity)> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.task.finished())
            .map(|(entity, job)| (job.rank, *entity))
            .collect();
        finished.sort_unstable();
        finished.into_iter().map(|(_, entity)| entity).collect()
    }

    fn take(&mut self, entity: Entity) -> Option<Job<T>> {
        self.jobs.remove(&entity)
    }
}

/// What a meshing task hands the collector: the region packed for the
/// store and what the entity's summary and bounds need. The `RegionMesh`
/// itself is dropped on the pool.
struct Built {
    coord: RegionCoord,
    bounds: ([f32; 3], [f32; 3]),
    summary: TerrainRegion,
    packed: store::PackedRegion,
    /// The region's rivers and lakes, if it has any and they were asked
    /// for.
    water: Option<RiverMesh>,
}

impl Built {
    fn new(mesh: &RegionMesh) -> Self {
        Self {
            water: None,
            coord: mesh.coord,
            bounds: mesh.bounds(),
            summary: TerrainRegion {
                // Under u32 by a wide margin; the constant says so.
                #[allow(clippy::cast_possible_truncation)]
                vertices: mesh.positions.len() as u32,
                min_height: mesh.min_height,
                max_height: mesh.max_height,
            },
            packed: store::pack_region(mesh),
        }
    }
}

/// The queue and what every frame of it reuses.
#[derive(Resource, Default)]
struct MeshQueue {
    queue: Queue<Task<Built>>,
    /// The wanted set as a set, refilled each frame.
    wanted: HashSet<RegionCoord>,
    /// The loaded regions as a set, refilled each frame: the streamer's own
    /// map hashes with the standard library's slower hasher, and the unit
    /// walk looks up thousands.
    loaded: HashSet<RegionCoord>,
    /// The wanted regions whose readiness releases loaded ones, refilled
    /// each frame.
    releases: HashSet<RegionCoord>,
    /// Pool time spent meshing, written by the tasks themselves.
    build_micros: Arc<AtomicU64>,
    /// Of it, the time spent building water.
    water_micros: Arc<AtomicU64>,
    /// Of it, the time spent gathering paint, and the regions, strokes and
    /// lines left out it counted.
    paint_counts: Arc<[AtomicU64; 5]>,
    /// Main-thread time in dispatch and collect since the last log line.
    spent: Duration,
}

/// What every task shares: the world and the biome layer table, both
/// read-only behind an `Arc`, and the cache of the networks' lines the
/// paint reads.
#[derive(Resource, Clone)]
struct MeshInputs {
    layers: Arc<BiomeLayers>,
    lines: Arc<paint::PaintCache>,
}

impl MeshInputs {
    /// The inputs from the frozen registries, inserted as a resource.
    fn insert(commands: &mut Commands, registries: &Registries) -> Self {
        let inputs = Self {
            layers: Arc::new(BiomeLayers::from_registries(&registries.0)),
            lines: Arc::new(paint::PaintCache::default()),
        };
        commands.insert_resource(inputs.clone());
        inputs
    }
}

/// Registers the terrain sets after [`StreamSet::Diff`], the mesh tasks and
/// the collector, and tells the streamer an external builder answers.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainConfig>()
            .init_resource::<TerrainStats>()
            .init_resource::<MeshQueue>()
            .init_resource::<ExternalRegionBuilder>()
            .configure_sets(
                Update,
                (TerrainSet::Dispatch, TerrainSet::Collect)
                    .chain()
                    .after(StreamSet::Diff)
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                (
                    dispatch_meshing.in_set(TerrainSet::Dispatch),
                    (collect_meshes, log_queue, log_water)
                        .chain()
                        .in_set(TerrainSet::Collect),
                ),
            );
        // Irradiance probes per streamed region (lighting plan phase 3b).
        probes::register(app);
    }
}

/// [`TerrainSet::Dispatch`]: loads into the queue, unloaded and unwanted
/// work out of it, and the nearest queued loads onto the pool while the cap
/// has room.
///
/// The layer table is built on first use rather than on entering the world
/// so it cannot race the sim's own `OnEnter(InWorld)` insertion of an empty
/// registry set.
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the messages; a scratch resource would only hide the inputs"
)]
fn dispatch_meshing(
    mut commands: Commands,
    cfg: Res<TerrainConfig>,
    stream: (
        Res<StreamConfig>,
        Res<Observers>,
        Res<WantedRegions>,
        Res<LoadedRegions>,
    ),
    source: Option<Res<WorldSourceRes>>,
    registries: Option<Res<Registries>>,
    inputs: Option<Res<MeshInputs>>,
    painted: (
        Option<Res<strata_sim::crossings::Crossings>>,
        Option<Res<TerrainStore>>,
    ),
    zones: Option<Res<strata_stream::DetailZones>>,
    mut queue: ResMut<MeshQueue>,
    mut stats: ResMut<TerrainStats>,
    mut loads: MessageReader<RegionLoad>,
    mut unloads: MessageReader<RegionUnload>,
    mut warned: Local<bool>,
) {
    let began = Instant::now();
    let (stream_cfg, observers, wanted, loaded) = stream;
    let MeshQueue {
        queue,
        wanted: wanted_set,
        loaded: loaded_set,
        releases,
        build_micros,
        water_micros,
        paint_counts,
        spent,
    } = &mut *queue;
    let mut dropped = Dropped::default();
    for RegionUnload(_, entity) in unloads.read() {
        queue.unload(*entity, &mut dropped);
    }
    for RegionLoad(coord, entity) in loads.read() {
        queue.load(*coord, *entity);
    }
    wanted_set.clear();
    wanted_set.extend(wanted.regions().iter().copied());
    queue.drop_unwanted(wanted_set, &mut dropped);
    stats.cancelled += dropped.cancelled;
    stats.dropped += dropped.finished;
    stats.build_micros = build_micros.load(Ordering::Relaxed);
    stats.water_micros = water_micros.load(Ordering::Relaxed);
    stats.paint_micros = paint_counts[0].load(Ordering::Relaxed);
    stats.paint_regions = paint_counts[1].load(Ordering::Relaxed);
    stats.paint_strokes = paint_counts[2].load(Ordering::Relaxed);
    stats.paint_dropped = paint_counts[3].load(Ordering::Relaxed);
    stats.paint_fullest = paint_counts[4].load(Ordering::Relaxed);

    let inputs = match (source.is_some(), inputs) {
        (false, _) => None,
        (true, Some(inputs)) => Some(inputs.clone()),
        (true, None) => registries.map(|registries| MeshInputs::insert(&mut commands, &registries)),
    };
    let (crossings, store) = painted;
    let paint_source = paint_source(&cfg, inputs.as_ref(), crossings.as_deref(), store.is_some());
    let layers = inputs.map(|inputs| inputs.layers);
    if source.is_none() && !*warned && !queue.pending.is_empty() {
        // The loads stay queued, so they are started once the source
        // appears; one that never does is a misconfigured app.
        *warned = true;
        warn!("regions are wanted but there is no WorldSourceRes; add WorldLoadPlugin");
    }
    let root_lod = stream_cfg.root_lod();
    loaded_set.clear();
    loaded_set.extend(loaded.iter().map(|(coord, _)| coord));
    releasing(loaded_set.iter().copied(), wanted_set, root_lod, releases);
    let start = queue.rank(cfg.max_queued, wanted_set, |coords| {
        rank_all(
            coords,
            &observers.positions,
            root_lod,
            |c| loaded_set.contains(&c),
            |c| releases.contains(&c),
        )
    });
    if let (Some(source), Some(layers)) = (source, layers) {
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let skirt = cfg.skirt_fraction;
        let water = cfg.water;
        for (entity, coord) in start {
            let source = source.0.clone();
            let layers = layers.clone();
            let clock = build_micros.clone();
            let water_clock = water_micros.clone();
            let paint_counts = paint_counts.clone();
            let paint_source = paint_source.clone();
            let pinned = zones.as_deref().is_some_and(|z| {
                z.0.iter()
                    .any(|zone| coord.lod <= zone.lod && zone.overlaps(coord))
            });
            let task = pool.spawn(async move {
                let began = Instant::now();
                let built = mesh_region(
                    coord,
                    source.as_ref(),
                    &layers,
                    (skirt, pinned, water),
                    &water_clock,
                    paint_source.as_deref().map(|p| (p, &*paint_counts)),
                );
                let micros = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
                clock.fetch_add(micros, Ordering::Relaxed);
                built
            });
            // Ranked with the others from next frame on; one that finishes
            // before then is collected after every ranked one.
            let rank = Rank {
                refines: true,
                unit_distance: f64::INFINITY,
                unit: coord,
                distance: f64::INFINITY,
                coord,
            };
            queue.start(entity, coord, task, rank);
        }
    }
    stats.in_flight = queue.jobs.len();
    stats.queued = queue.pending.len();
    *spent += began.elapsed();
}

/// What the tasks gather a region's paint from: the procedural world the
/// crossings are read from, the shared cache of its networks' lines and the
/// roads' layers. `None` where the paint is off, or where there is no
/// `store` to write it to (a headless app), or no world to read it from.
fn paint_source(
    cfg: &TerrainConfig,
    inputs: Option<&MeshInputs>,
    crossings: Option<&strata_sim::crossings::Crossings>,
    store: bool,
) -> Option<Arc<paint::PaintSource>> {
    let (Some(inputs), Some(crossings)) = (inputs, crossings) else {
        return None;
    };
    (cfg.paint && store).then(|| {
        Arc::new(paint::PaintSource {
            world: Arc::clone(&crossings.world),
            cache: Arc::clone(&inputs.lines),
            ways: inputs.layers.ways(),
        })
    })
}

/// One region's task: its ground packed for the store and, if `water`, its
/// rivers' and lakes' surface from the same samples, whose time goes on
/// `water_clock` too; with a `paint` source, the rivers and roads its
/// fragments paint at distance, counted on its `[micros, regions, strokes,
/// dropped, fullest bucket]`, and the roads it paints left out of its
/// splat. `(skirt fraction, pinned, water)`.
fn mesh_region(
    coord: RegionCoord,
    source: &dyn strata_worldgen::WorldSource,
    layers: &BiomeLayers,
    (skirt, pinned, water): (f32, bool, bool),
    water_clock: &AtomicU64,
    paint: Option<(&paint::PaintSource, &[AtomicU64; 5])>,
) -> Built {
    // The paint first: the mesher leaves the roads it draws out of the
    // splat on a level too coarse for them.
    let painted = paint.map(|(paint_source, counts)| {
        let began = Instant::now();
        let region = paint::region_paint(coord, paint_source);
        let words = paint::pack_paint(&region, coord);
        let micros = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
        counts[0].fetch_add(micros, Ordering::Relaxed);
        counts[1].fetch_add(1, Ordering::Relaxed);
        counts[2].fetch_add(region.strokes.len() as u64, Ordering::Relaxed);
        counts[3].fetch_add(u64::from(region.dropped), Ordering::Relaxed);
        counts[4].fetch_max(u64::from(region.fullest), Ordering::Relaxed);
        (
            paint::PaintedWays::of(&region, coord.vertex_spacing()),
            words,
        )
    });
    let mut scratch: Vec<Sample> = Vec::new();
    let mesh = build_region_mesh_painted(
        coord,
        source,
        layers,
        (skirt, pinned),
        painted
            .as_ref()
            .map_or(&paint::PaintedWays::default(), |p| &p.0),
        &mut scratch,
    );
    let mut built = Built::new(&mesh);
    drop(mesh);
    if let Some((_, words)) = &painted {
        built.packed.set_paint(words);
    }
    if water {
        // The mesher's samples are still in the scratch.
        let began = Instant::now();
        built.water = build_river_mesh(coord, &scratch, source.sea_level(), pinned);
        let micros = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
        water_clock.fetch_add(micros, Ordering::Relaxed);
    }
    built
}

/// The GPU side the collector writes to, present where there is a renderer.
type Gpu<'w> = (
    Option<ResMut<'w, TerrainStore>>,
    Option<Res<'w, RenderDevice>>,
    Option<Res<'w, RenderQueue>>,
    Option<ResMut<'w, Assets<TerrainMaterial>>>,
);

/// What the collector draws a region's water with, where there is a
/// renderer and the sea has made the rivers' material.
type WaterGpu<'w> = (
    Option<ResMut<'w, Assets<Mesh>>>,
    Option<Res<'w, river::RiverMaterialHandle>>,
);

/// [`TerrainSet::Collect`]: finished tasks onto their entities, nearest
/// first, under the budget. Waits for the material and the store when there
/// is a renderer, so a region is never drawn pink for a frame.
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the tasks; the store and the material are its outputs"
)]
fn collect_meshes(
    mut commands: Commands,
    cfg: Res<TerrainConfig>,
    mut queue: ResMut<MeshQueue>,
    mut stats: ResMut<TerrainStats>,
    gpu: Gpu,
    water_gpu: WaterGpu,
    material: Option<Res<TerrainMaterialHandle>>,
    regions: Query<&Region>,
    mut ready: MessageWriter<RegionReady>,
) {
    let began = Instant::now();
    stats.built_this_frame = 0;
    let (mut store, device, render_queue, mut materials) = gpu;
    let (mut meshes, river_material) = water_gpu;
    if let Some(store) = store.as_mut() {
        store.next_frame();
        (stats.slots, stats.slot_capacity) = store.occupancy();
    }
    let MeshQueue { queue, spent, .. } = &mut *queue;
    if queue.jobs.is_empty() {
        stats.waiting = 0;
        return;
    }
    // With a renderer, regions wait for everything they are drawn with; a
    // headless app takes the summary alone.
    let target = match (&mut store, &device, &render_queue, &material) {
        (None, ..) => None,
        // The material is a page's from here; it is waited for because the
        // store's first page copies it.
        (Some(store), Some(device), Some(render_queue), Some(_)) if store.mesh().is_some() => {
            Some((store, device, render_queue))
        }
        _ => return,
    };
    let mut target = target;
    // Finished only, so a slow task does not block the ones behind it.
    let finished = queue.finished();
    let mut waiting = finished.len();
    let mut uploaded: u64 = 0;
    // What the budget is charged: the store's bytes, and the water's at
    // the mesh path's weight.
    let mut charged: u64 = 0;
    let mut water_frame = (0_u64, 0_u32);
    let budget = cfg.max_upload_bytes_per_frame as u64;
    for entity in finished.into_iter().take(cfg.max_collect_per_frame) {
        // Every region writes the same bytes; the check is honest, so the
        // budget admits what fits and one region whatever its size.
        if charged > 0 && charged + store::SLOT_BYTES + store::HEADER_BYTES > budget {
            break;
        }
        waiting -= 1;
        let Some(mut job) = queue.take(entity) else {
            continue;
        };
        let Some(mut built) = block_on(poll_once(&mut job.task)) else {
            continue;
        };
        // The streamer despawns before this runs and the unload message
        // dropped the task in dispatch; this is the same guard for an entity
        // that went away by another hand.
        let Ok(mut entity_commands) = commands.get_entity(entity) else {
            continue;
        };
        if !regions.contains(entity) {
            continue;
        }
        let (lo, hi) = built.bounds;
        if let Some((store, device, render_queue)) = target.as_mut() {
            let Some(placed) = store.install(
                &built.packed,
                device,
                render_queue,
                materials.as_deref_mut(),
            ) else {
                // Only before the store's first page has its material, which
                // the target above waits for. Not reported ready: the
                // coarser ground the no-hole rule holds over the region is
                // better than a hole.
                error!(coord = ?built.coord, "the terrain store had no page for a region");
                continue;
            };
            uploaded += built.packed.upload_bytes();
            charged += built.packed.upload_bytes();
            let Some(mesh) = store.mesh().cloned() else {
                continue;
            };
            // The bounds come from the samples, morph targets and skirts
            // included; `NoAutoAabb` keeps Bevy from replacing them with the
            // shared mesh's box, which bounds nothing a region reaches.
            entity_commands.insert((
                built.summary,
                Mesh3d(mesh),
                MeshTag(placed.tag),
                placed.slot,
                MeshMaterial3d(placed.material),
                Aabb::from_min_max(Vec3::from_array(lo), Vec3::from_array(hi)),
                NoAutoAabb,
            ));
            // The region's water on a child, so it is culled, rebased and
            // despawned with the region (the river module's docs).
            if let (Some(water), Some(meshes), Some(river_material)) = (
                built.water.take(),
                meshes.as_deref_mut(),
                river_material.as_deref(),
            ) {
                let drawn = draw_water(&mut entity_commands, &water, meshes, river_material);
                stats.water_built += 1;
                stats.water_vertices += u64::from(drawn.vertices);
                uploaded += u64::from(drawn.bytes);
                charged += u64::from(drawn.bytes) * MESH_PATH_WEIGHT;
                water_frame.0 += u64::from(drawn.bytes);
                water_frame.1 += 1;
            }
        } else {
            entity_commands.insert(built.summary);
        }
        ready.write(RegionReady(built.coord, entity));
        stats.built += 1;
        stats.built_this_frame += 1;
    }
    stats.uploaded_bytes += uploaded;
    stats.water_bytes += water_frame.0;
    if water_frame.0 > stats.water_frame_max.0 {
        stats.water_frame_max = water_frame;
    }
    if let Some(store) = store.as_ref() {
        (stats.slots, stats.slot_capacity) = store.occupancy();
    }
    stats.in_flight = queue.jobs.len();
    stats.waiting = waiting;
    *spent += began.elapsed();
}

/// Spawns a region's water as a child of its entity: the mesh, the rivers'
/// material and the bounds of both levels it morphs between.
fn draw_water(
    region: &mut EntityCommands,
    water: &RiverMesh,
    meshes: &mut Assets<Mesh>,
    material: &river::RiverMaterialHandle,
) -> RegionWater {
    let (lo, hi) = water.bounds();
    let drawn = RegionWater {
        vertices: u32::try_from(water.positions.len()).unwrap_or(u32::MAX),
        bytes: u32::try_from(water.upload_bytes()).unwrap_or(u32::MAX),
    };
    region.with_child((
        drawn,
        Mesh3d(meshes.add(water.to_mesh())),
        MeshMaterial3d(material.0.clone()),
        Aabb::from_min_max(Vec3::from_array(lo), Vec3::from_array(hi)),
        NoAutoAabb,
        NotShadowCaster,
    ));
    drawn
}

/// One window of [`log_queue`].
#[derive(Default)]
struct QueueWindow {
    since: f64,
    frames: u32,
    waiting: u64,
    waiting_max: usize,
    held: u64,
    queued: u64,
    last: TerrainStats,
}

/// Every two seconds while Bevy's render diagnostics are on (the client's
/// statistics, so every bench run), as the probes' line is: the queue's
/// depths and what passed through it, the counts the streaming plan's
/// phase 1 is accepted on.
fn log_queue(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Option<Res<Time<Real>>>,
    stats: Res<TerrainStats>,
    mut queue: ResMut<MeshQueue>,
    mut window: Local<QueueWindow>,
) {
    let (Some(diagnostics), Some(time)) = (diagnostics, time) else {
        return;
    };
    window.frames += 1;
    window.waiting += stats.waiting as u64;
    window.waiting_max = window.waiting_max.max(stats.waiting);
    window.held += stats.in_flight as u64;
    window.queued += stats.queued as u64;
    let now = time.elapsed_secs_f64();
    let secs = now - window.since;
    if secs < 2.0 {
        return;
    }
    let shown = diagnostics
        .iter()
        .any(|d| d.path().as_str().ends_with("/elapsed_gpu"));
    if shown {
        let frames = f64::from(window.frames);
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts of a two-second window, far under 2^52"
        )]
        let (mean, rate) = (
            |sum: u64| sum as f64 / frames,
            |now: u64, then: u64| now.saturating_sub(then) as f64 / secs,
        );
        let last = window.last;
        info!(
            "terrain queue t {now:.1} s: waiting {:.1} mean {} max, held {:.1}, queued {:.1}, built {:.1}/s, dropped {:.2}/s, cancelled {:.2}/s, pool {:.2} thread-s/s, main {:.0} us/frame, uploaded {:.2} MB/s, slots {}/{}",
            mean(window.waiting),
            window.waiting_max,
            mean(window.held),
            mean(window.queued),
            rate(stats.built, last.built),
            rate(stats.dropped, last.dropped),
            rate(stats.cancelled, last.cancelled),
            rate(stats.build_micros, last.build_micros) / 1e6,
            queue.spent.as_secs_f64() * 1e6 / frames,
            rate(stats.uploaded_bytes, last.uploaded_bytes) / 1e6,
            stats.slots,
            stats.slot_capacity,
        );
    }
    queue.spent = Duration::ZERO;
    *window = QueueWindow {
        since: now,
        last: *stats,
        ..QueueWindow::default()
    };
}

/// Every two seconds while the queue's line is logged, the rivers' and
/// lakes' water (ticket 0005, step 3): the regions drawing it now and their
/// vertices and bytes, and what the tasks built and spent on it since the
/// start. The line the step's measures are read from.
fn log_water(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Option<Res<Time<Real>>>,
    stats: Res<TerrainStats>,
    drawn: Query<&RegionWater>,
    mut since: Local<f64>,
) {
    let (Some(diagnostics), Some(time)) = (diagnostics, time) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if now - *since < 2.0 {
        return;
    }
    *since = now;
    if !diagnostics
        .iter()
        .any(|d| d.path().as_str().ends_with("/elapsed_gpu"))
    {
        return;
    }
    let (mut regions, mut vertices, mut bytes) = (0_u32, 0_u64, 0_u64);
    for water in &drawn {
        regions += 1;
        vertices += u64::from(water.vertices);
        bytes += u64::from(water.bytes);
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "counts and microseconds far under 2^52"
    )]
    let (per, kib) = (
        |micros: u64, count: u64| micros as f64 / count.max(1) as f64,
        |bytes: u64| bytes as f64 / 1024.0,
    );
    info!(
        "river water t {now:.1} s: drawn {regions} regions, {vertices} vertices, {:.1} KiB; built {} with water of {} regions meshed, {} vertices, {:.1} KiB; worst frame {:.1} KiB in {} regions; water {:.1} us a region meshed, of {:.0} us",
        kib(bytes),
        stats.water_built,
        stats.built,
        stats.water_vertices,
        kib(stats.water_bytes),
        kib(stats.water_frame_max.0),
        stats.water_frame_max.1,
        per(stats.water_micros, stats.built),
        per(stats.build_micros, stats.built),
    );
    if stats.paint_regions > 0 {
        info!(
            "far paint t {now:.1} s: {} regions painted, {:.1} strokes and {:.1} us a region, {} lines left out, fullest bucket {} of {}",
            stats.paint_regions,
            per(stats.paint_strokes, stats.paint_regions),
            per(stats.paint_micros, stats.paint_regions),
            stats.paint_dropped,
            stats.paint_fullest,
            paint::BUCKET_MOST,
        );
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn builds_over_the_streamer() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            TerrainPlugin,
        ));
        app.update();
        assert!(
            app.world()
                .get_resource::<ExternalRegionBuilder>()
                .is_some(),
            "the streamer must wait for this crate's RegionReady"
        );
    }
}

#[cfg(test)]
mod queue {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    /// A job that is finished when the test says so.
    struct Fake(bool);

    impl Finish for Fake {
        fn finished(&self) -> bool {
            self.0
        }
    }

    const ROOT: u8 = 6;

    fn entity(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    fn rank_with(
        observer: WorldPos,
        loaded: &[RegionCoord],
    ) -> impl Fn(&[RegionCoord]) -> Vec<Rank> {
        let loaded: HashSet<RegionCoord> = loaded.iter().copied().collect();
        move |coords| {
            rank_all(
                coords,
                &[observer],
                ROOT,
                |c| loaded.contains(&c),
                |_| false,
            )
        }
    }

    /// A queue with `coords` loaded in order as entities 0, 1, ..., all
    /// wanted.
    fn queue_of(coords: &[RegionCoord]) -> (Queue<Fake>, HashSet<RegionCoord>) {
        let mut queue = Queue::default();
        for (n, coord) in (0u32..).zip(coords) {
            queue.load(*coord, entity(n));
        }
        (queue, coords.iter().copied().collect())
    }

    fn started(queue: &mut Queue<Fake>, start: &[(Entity, RegionCoord)], done: bool) {
        for (entity, coord) in start {
            let rank = Rank {
                refines: false,
                unit_distance: 0.0,
                unit: *coord,
                distance: 0.0,
                coord: *coord,
            };
            queue.start(*entity, *coord, Fake(done), rank);
        }
    }

    #[test]
    fn starts_nearest_first_and_breaks_ties_by_coordinate() {
        // In the middle of region 0 along x: the row east of it and the
        // same row west are at the same distances, so every distance but
        // the first is a tie, which the coordinate breaks (west first).
        let observer = WorldPos::from_xz(32.0, 10.0);
        let coords: Vec<RegionCoord> = [3, -3, 2, -2, 1, -1, 0]
            .into_iter()
            .map(|x| RegionCoord::new(x, 0, 0))
            .collect();
        // An exact tie, bit for bit, or the coordinate never decides.
        assert_eq!(
            nearest_distance(&[observer], coords[0]).to_bits(),
            nearest_distance(&[observer], coords[1]).to_bits()
        );
        let (mut queue, wanted) = queue_of(&coords);
        let start = queue.rank(100, &wanted, rank_with(observer, &[]));
        let order: Vec<i32> = start.iter().map(|(_, c)| c.x).collect();
        assert_eq!(order, [0, -1, 1, -2, 2, -3, 3]);
        // Under a cap that cuts through a tie, the same first three.
        let capped: Vec<i32> = queue
            .rank(3, &wanted, rank_with(observer, &[]))
            .iter()
            .map(|(_, c)| c.x)
            .collect();
        assert_eq!(capped, [0, -1, 1]);
        // The same inputs inserted in another order give the same order.
        let mut reversed: Vec<RegionCoord> = coords.clone();
        reversed.reverse();
        let (mut again, _) = queue_of(&reversed);
        let order_again: Vec<i32> = again
            .rank(100, &wanted, rank_with(observer, &[]))
            .iter()
            .map(|(_, c)| c.x)
            .collect();
        assert_eq!(order, order_again);
    }

    #[test]
    fn the_cap_counts_running_and_waiting_jobs() {
        let observer = WorldPos::from_xz(0.0, 0.0);
        let coords: Vec<RegionCoord> = (0..10).map(|x| RegionCoord::new(x, 0, 0)).collect();
        let (mut queue, wanted) = queue_of(&coords);
        let start = queue.rank(4, &wanted, rank_with(observer, &[]));
        assert_eq!(
            start.iter().map(|(_, c)| c.x).collect::<Vec<_>>(),
            [0, 1, 2, 3],
            "the nearest four"
        );
        // Two still running, two finished and waiting: no room.
        started(&mut queue, &start[..2], false);
        started(&mut queue, &start[2..], true);
        assert!(queue.rank(4, &wanted, rank_with(observer, &[])).is_empty());
        assert_eq!(queue.pending.len(), 6);
        // One collected: one slot, and the next nearest takes it.
        let first = queue.finished()[0];
        assert!(queue.take(first).is_some());
        let next = queue.rank(4, &wanted, rank_with(observer, &[]));
        assert_eq!(next.iter().map(|(_, c)| c.x).collect::<Vec<_>>(), [4]);
    }

    #[test]
    fn finished_jobs_upload_nearest_first() {
        let observer = WorldPos::from_xz(0.0, 0.0);
        let coords: Vec<RegionCoord> = [5, 1, 3, 0, 4, 2]
            .into_iter()
            .map(|x| RegionCoord::new(x, 0, 0))
            .collect();
        let (mut queue, wanted) = queue_of(&coords);
        let start = queue.rank(100, &wanted, rank_with(observer, &[]));
        started(&mut queue, &start, true);
        // Ranks are stored when the queue is ranked, as dispatch does each
        // frame before the collector reads them.
        queue.rank(100, &wanted, rank_with(observer, &[]));
        let order: Vec<i32> = queue
            .finished()
            .iter()
            .map(|e| queue.jobs[e].coord.x)
            .collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5]);
        // The observer flies east: the order follows it.
        queue.rank(100, &wanted, rank_with(WorldPos::from_xz(600.0, 0.0), &[]));
        let order: Vec<i32> = queue
            .finished()
            .iter()
            .map(|e| queue.jobs[e].coord.x)
            .collect();
        assert_eq!(order, [5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn a_job_that_leaves_the_wanted_set_is_dropped_and_queued_again() {
        let observer = WorldPos::from_xz(0.0, 0.0);
        let coords: Vec<RegionCoord> = (0..4).map(|x| RegionCoord::new(x, 0, 0)).collect();
        let (mut queue, mut wanted) = queue_of(&coords);
        let start = queue.rank(100, &wanted, rank_with(observer, &[]));
        started(&mut queue, &start[..2], true);
        started(&mut queue, &start[2..], false);
        // Regions 1 (finished) and 3 (running) stop being wanted.
        wanted.remove(&coords[1]);
        wanted.remove(&coords[3]);
        let mut dropped = Dropped::default();
        queue.drop_unwanted(&wanted, &mut dropped);
        assert_eq!(
            dropped,
            Dropped {
                cancelled: 2,
                finished: 1
            }
        );
        assert_eq!(queue.jobs.len(), 2);
        assert_eq!(queue.finished(), [entity(0)]);
        // Not started again while unwanted, whatever the room.
        assert!(
            queue
                .rank(100, &wanted, rank_with(observer, &[]))
                .is_empty()
        );
        // Wanted again: started again.
        wanted.insert(coords[1]);
        let again = queue.rank(100, &wanted, rank_with(observer, &[]));
        assert_eq!(again, [(entity(1), coords[1])]);
        // An unload forgets the entity wherever it is.
        queue.unload(entity(3), &mut dropped);
        queue.unload(entity(0), &mut dropped);
        assert!(!queue.pending.contains_key(&entity(3)));
        assert!(!queue.jobs.contains_key(&entity(0)));
        assert_eq!(dropped.cancelled, 3);
        assert_eq!(dropped.finished, 2);
    }

    #[test]
    fn a_splits_children_rank_as_one_unit() {
        // A lod-2 parent, loaded and on screen, splits into four lod-1
        // children. The nearest child is 10 m away, the farthest 175 m; a
        // lone lod-0 region west of the parent, 44 m away, refines a loaded
        // lod-1 region of its own.
        let parent = RegionCoord::new(1, 0, 2);
        let children = parent.children().unwrap();
        let observer = WorldPos::from_xz(parent.min_corner().x() - 10.0, 20.0);
        let lone = RegionCoord::new(3, 1, 0);
        let lone_parent = lone.parent().unwrap();
        let lone_distance = nearest_distance(&[observer], lone);
        let near = children
            .iter()
            .map(|c| nearest_distance(&[observer], *c))
            .fold(f64::INFINITY, f64::min);
        let far = children
            .iter()
            .map(|c| nearest_distance(&[observer], *c))
            .fold(0.0, f64::max);
        assert!(
            near < lone_distance && lone_distance < far,
            "{near} {lone_distance} {far}"
        );

        let mut coords = children.to_vec();
        coords.push(lone);
        let (mut queue, wanted) = queue_of(&coords);
        let loaded = [parent, lone_parent];
        let start = queue.rank(100, &wanted, rank_with(observer, &loaded));
        let order: Vec<RegionCoord> = start.iter().map(|(_, c)| *c).collect();
        assert!(
            order[..4].iter().all(|c| children.contains(c)),
            "the lone region split the unit: {order:?}"
        );
        assert_eq!(order[4], lone);
        // Within the unit, nearest first.
        let distances: Vec<f64> = order[..4]
            .iter()
            .map(|c| nearest_distance(&[observer], *c))
            .collect();
        assert!(distances.is_sorted(), "{distances:?}");

        // With nothing loaded every region fills a hole, nothing holds the
        // children together, and the lone region goes by its own distance.
        let (mut queue, wanted) = queue_of(&coords);
        let order: Vec<RegionCoord> = queue
            .rank(100, &wanted, rank_with(observer, &[]))
            .iter()
            .map(|(_, c)| *c)
            .collect();
        let lone_at = order.iter().position(|c| *c == lone).unwrap();
        assert!(0 < lone_at && lone_at < 4, "{order:?}");
    }

    #[test]
    fn merges_and_holes_go_before_refinement() {
        let observer = WorldPos::from_xz(10.0, 10.0);
        // Refinement under the observer: a loaded lod-1 region split.
        let split_parent = RegionCoord::containing(observer, 1);
        let near = split_parent.children().unwrap();
        // Far behind, a lod-3 merge parent whose loaded children it
        // replaces, and far ahead a lod-3 region with nothing over it.
        let merge = RegionCoord::new(-6, 0, 3);
        let merged = merge.children().unwrap();
        let hole = RegionCoord::new(6, 0, 3);
        let mut wanted: HashSet<RegionCoord> = near.iter().copied().collect();
        wanted.extend([merge, hole]);
        let mut loaded: Vec<RegionCoord> = vec![split_parent];
        loaded.extend(merged);
        let mut releases = HashSet::new();
        releasing(loaded.iter().copied(), &wanted, ROOT, &mut releases);
        assert_eq!(releases, [merge].into_iter().collect::<HashSet<_>>());

        let loaded: HashSet<RegionCoord> = loaded.into_iter().collect();
        let coords: Vec<RegionCoord> = wanted.iter().copied().collect();
        let mut ranks = rank_all(
            &coords,
            &[observer],
            ROOT,
            |c| loaded.contains(&c),
            |c| releases.contains(&c),
        );
        ranks.sort_unstable();
        let order: Vec<RegionCoord> = ranks.iter().map(|r| r.coord).collect();
        assert_eq!(order[..2], [merge, hole], "{order:?}");
        assert!(order[2..].iter().all(|c| near.contains(c)), "{order:?}");
    }

    #[test]
    fn a_unit_is_the_nearest_loaded_ancestor() {
        let coord = RegionCoord::new(5, 7, 0);
        let grandparent = coord.ancestor_at(2).unwrap();
        let great = coord.ancestor_at(3).unwrap();
        assert_eq!(unit_of(coord, ROOT, |_| false), coord);
        assert_eq!(unit_of(coord, ROOT, |c| c == great), great);
        assert_eq!(
            unit_of(coord, ROOT, |c| c == great || c == grandparent),
            grandparent
        );
        // Nothing above the root counts.
        assert_eq!(unit_of(coord, 2, |c| c == great), coord);
    }
}
