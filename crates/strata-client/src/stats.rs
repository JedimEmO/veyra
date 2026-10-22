//! Frame statistics: the frame rate, the worst frame, and what is loaded.
//!
//! One resource feeds both the HUD and the log. The window is short so the
//! HUD reacts, and the log line every few windows carries the worst frame
//! seen across them, because a hitch is one long frame and an average
//! hides it. A scripted flight is judged by that line.
//!
//! A second line at the same moment says where the frame went
//! ([`FrameCost`]): the GPU time of each timed pass, the sun's cascades
//! among them, the batch sets each phase drew, the meshes uploaded and the
//! foliage resident. These were a scratch module for the 2026-10-03
//! profile (`docs/perf/profile-2026-10-03.md`); kept here so every later
//! change to the frame's cost shows as a diff in the ledger, and a
//! benchmark keeps their medians over its measured stretch.
//!
//! The same line carries what the streamers did ([`StreamFrame`]): terrain
//! regions waiting for a mesh, meshes uploaded and cancelled, colliders
//! built and despawned, and the worst main and render frame of the interval.
//! The 2026-10-04 streaming profile (`docs/perf/streaming-2026-10-04.md`)
//! found the moving fixtures' cost in those counts, and a benchmark of a
//! moving fixture keeps them over its whole measured stretch.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strata_core::platform::time::Instant;

use bevy::asset::AssetEvent;
use bevy::camera::visibility::VisibleEntities;
use bevy::camera::{Camera, Camera3d};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::time::{Real, Time};
use strata_atmosphere::diagnostics::{BatchSets, RenderCounts, SUN_SHADOWS_SPAN};
use strata_foliage::FoliageStats;
use strata_physics::{TerrainCollider, TerrainColliderStats, TreeCollider};
use strata_stream::LoadedRegions;
use strata_terrain::TerrainStats;
use tracing::info;
use veyra_sim::{Observers, RenderOrigin};

use crate::flight::MotionHold;

/// How long one statistics window lasts.
pub const WINDOW: Duration = Duration::from_millis(500);

/// A log line is written every this many windows: two seconds.
pub const LOG_EVERY_WINDOWS: u32 = 4;

/// A frame longer than this is counted as a hitch. The Phase 2 target is
/// none during a flight.
pub const HITCH_MS: f64 = 50.0;

// Bounds both memory and sorting cost even during uncapped/headless runs.
const MAX_SAMPLES: usize = 4096;

#[derive(Debug, Default)]
struct FrameWindow {
    frames: u32,
    elapsed: f64,
    worst_ms: f64,
    samples: VecDeque<f64>,
}

impl FrameWindow {
    fn push(&mut self, dt: f64) {
        self.frames += 1;
        self.elapsed += dt;
        self.worst_ms = self.worst_ms.max(dt * 1000.0);
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(dt * 1000.0);
    }

    fn percentiles(&self) -> [f64; 3] {
        let mut sorted: Vec<_> = self.samples.iter().copied().collect();
        sorted.sort_unstable_by(f64::total_cmp);
        if sorted.is_empty() {
            return [0.0; 3];
        }
        [50, 95, 99].map(|p| sorted[(sorted.len() * p).div_ceil(100) - 1])
    }

    fn clear(&mut self) {
        self.frames = 0;
        self.elapsed = 0.0;
        self.worst_ms = 0.0;
        self.samples.clear();
    }
}

/// The running window and the last completed one.
#[derive(Resource, Debug, Default)]
pub struct FrameStats {
    window: FrameWindow,
    log_window: FrameWindow,
    windows: u32,
    /// Frames per second over the last completed window.
    pub fps: f64,
    /// Mean frame time over the last completed window, milliseconds.
    pub avg_ms: f64,
    /// The longest frame in the last completed window, milliseconds.
    pub last_worst_ms: f64,
    /// Median frame time in the last window, milliseconds.
    pub p50_ms: f64,
    /// 95th percentile frame time in the last window, milliseconds.
    pub p95_ms: f64,
    /// 99th percentile frame time in the last window, milliseconds.
    /// Percentiles use nearest ranks over at most the latest 4096 frames;
    /// averages, worst frames and hitch counts always include every frame.
    pub p99_ms: f64,
    /// Frames over [`HITCH_MS`] since the start, for a test or a log grep.
    pub hitches: u32,
    /// Elapsed GPU time for camera rendering, milliseconds (timestamp queries;
    /// zero where unsupported). Falls back to summed Bevy spans when the
    /// atmosphere plugin is absent. Independent of the vsync wall-clock wait.
    pub gpu_ms: f64,
    /// Where the frame went, as of the last log line.
    pub cost: FrameCost,
    /// What the streamers did this frame.
    pub stream: StreamFrame,
    /// The same, summed since the last log line.
    stream_interval: StreamTotals,
    /// Meshes added since the last log line, and the real time it was at.
    meshes_added: u64,
    cost_since: f64,
    /// Whether the first frame has been skipped: the frame that builds the
    /// pipelines is not a hitch anyone can fix here.
    warmed: bool,
}

impl FrameStats {
    fn record_frame(&mut self, time: &Time<Real>) -> bool {
        if !self.warmed {
            self.warmed = true;
            return false;
        }
        let dt = time.delta_secs_f64();
        self.window.push(dt);
        self.log_window.push(dt);
        if dt * 1000.0 > HITCH_MS {
            self.hitches = self.hitches.saturating_add(1);
        }
        if self.window.elapsed < WINDOW.as_secs_f64() {
            return false;
        }
        self.fps = f64::from(self.window.frames) / self.window.elapsed;
        self.avg_ms = self.window.elapsed * 1000.0 / f64::from(self.window.frames);
        self.last_worst_ms = self.window.worst_ms;
        [self.p50_ms, self.p95_ms, self.p99_ms] = self.window.percentiles();
        self.window.clear();
        self.windows += 1;
        true
    }
}

/// What the streamers did in one frame, read from their public counters.
///
/// The terrain crate counts tasks, not their state, so a mesh that has
/// finished and waits for the upload budget is one of
/// [`Self::terrain_tasks`] along with the ones still running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamFrame {
    /// Regions requested and not yet built (`LoadedRegions::pending`).
    pub regions_pending: usize,
    /// Terrain meshing tasks held, running or finished and waiting.
    pub terrain_tasks: usize,
    /// Terrain meshes inserted this frame.
    pub terrain_uploads: u64,
    /// Terrain tasks dropped by an unload this frame.
    pub terrain_cancelled: u64,
    /// Ground (heightfield) colliders built this frame.
    pub heightfields: u64,
    /// Collider entities, ground and trunks, spawned this frame.
    pub colliders_added: u64,
    /// Collider entities, ground and trunks, despawned this frame.
    pub colliders_removed: u64,
}

/// [`StreamFrame`]s summed over a stretch of frames.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct StreamTotals {
    frames: u64,
    regions_pending: u64,
    regions_pending_max: usize,
    terrain_tasks: u64,
    terrain_tasks_max: usize,
    terrain_uploads: u64,
    terrain_uploads_max: u64,
    terrain_cancelled: u64,
    heightfields: u64,
    colliders_added: u64,
    colliders_removed: u64,
}

impl StreamTotals {
    fn add(&mut self, frame: &StreamFrame) {
        self.frames += 1;
        self.regions_pending += frame.regions_pending as u64;
        self.regions_pending_max = self.regions_pending_max.max(frame.regions_pending);
        self.terrain_tasks += frame.terrain_tasks as u64;
        self.terrain_tasks_max = self.terrain_tasks_max.max(frame.terrain_tasks);
        self.terrain_uploads += frame.terrain_uploads;
        self.terrain_uploads_max = self.terrain_uploads_max.max(frame.terrain_uploads);
        self.terrain_cancelled += frame.terrain_cancelled;
        self.heightfields += frame.heightfields;
        self.colliders_added += frame.colliders_added;
        self.colliders_removed += frame.colliders_removed;
    }

    /// The totals as rates over `seconds` of real time.
    #[allow(
        clippy::cast_precision_loss,
        reason = "frame and entity counts far below 2^52 are exact in f64"
    )]
    fn rates(&self, seconds: f64) -> StreamRates {
        let frames = self.frames.max(1) as f64;
        let per_s = |count: u64| {
            if seconds > 0.0 {
                count as f64 / seconds
            } else {
                0.0
            }
        };
        StreamRates {
            regions_pending_mean: self.regions_pending as f64 / frames,
            regions_pending_max: self.regions_pending_max,
            terrain_tasks_mean: self.terrain_tasks as f64 / frames,
            terrain_tasks_max: self.terrain_tasks_max,
            uploads_per_frame: self.terrain_uploads as f64 / frames,
            uploads_max_frame: self.terrain_uploads_max,
            uploads_per_s: per_s(self.terrain_uploads),
            cancelled_per_s: per_s(self.terrain_cancelled),
            heightfields_per_s: per_s(self.heightfields),
            colliders_added_per_s: per_s(self.colliders_added),
            colliders_removed_per_s: per_s(self.colliders_removed),
        }
    }
}

/// The streamers over a stretch of frames: means and maxima of what is
/// queued, rates of what happened.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StreamRates {
    /// Regions waiting for their mesh, mean over the frames.
    pub regions_pending_mean: f64,
    /// Regions waiting for their mesh, the most in one frame.
    pub regions_pending_max: usize,
    /// Terrain tasks held, mean over the frames.
    pub terrain_tasks_mean: f64,
    /// Terrain tasks held, the most in one frame.
    pub terrain_tasks_max: usize,
    /// Terrain meshes uploaded per frame, mean.
    pub uploads_per_frame: f64,
    /// Terrain meshes uploaded in the busiest frame.
    pub uploads_max_frame: u64,
    /// Terrain meshes uploaded per second.
    pub uploads_per_s: f64,
    /// Terrain tasks cancelled per second.
    pub cancelled_per_s: f64,
    /// Ground colliders built per second.
    pub heightfields_per_s: f64,
    /// Collider entities spawned per second.
    pub colliders_added_per_s: f64,
    /// Collider entities despawned per second.
    pub colliders_removed_per_s: f64,
}

/// What [`StreamFrame`] reads. The terrain and collider counters are
/// totals since the start, so a frame's share is the difference from the
/// last frame's; the collider entities are counted by their components,
/// since the physics crate counts regions, not the trunks on them.
#[derive(SystemParam)]
pub struct StreamInputs<'w, 's> {
    loaded: Option<Res<'w, LoadedRegions>>,
    terrain: Option<Res<'w, TerrainStats>>,
    colliders: Option<Res<'w, TerrainColliderStats>>,
    added: Query<'w, 's, (), Or<(Added<TerrainCollider>, Added<TreeCollider>)>>,
    removed_ground: RemovedComponents<'w, 's, TerrainCollider>,
    removed_trees: RemovedComponents<'w, 's, TreeCollider>,
    /// Terrain built, terrain cancelled and heightfields built, as of the
    /// last frame.
    last: Local<'s, Option<[u64; 3]>>,
}

impl StreamInputs<'_, '_> {
    /// This frame's streaming. Must run every frame: the removals are
    /// messages Bevy keeps for two frames, and the additions are counted
    /// against this system's last run.
    fn sample(&mut self) -> StreamFrame {
        let regions_pending = self.loaded.as_deref().map_or(0, |l| l.pending().count());
        let (terrain_tasks, built, cancelled) = self
            .terrain
            .as_deref()
            .map_or((0, 0, 0), |t| (t.in_flight, t.built, t.cancelled));
        let heightfields = self.colliders.as_deref().map_or(0, |c| c.built);
        let now = [built, cancelled, heightfields];
        let before = self.last.replace(now).unwrap_or(now);
        let delta = |i: usize| now[i].saturating_sub(before[i]);
        StreamFrame {
            regions_pending,
            terrain_tasks,
            terrain_uploads: delta(0),
            terrain_cancelled: delta(1),
            heightfields: delta(2),
            colliders_added: self.added.iter().count() as u64,
            colliders_removed: (self.removed_ground.read().count()
                + self.removed_trees.read().count()) as u64,
        }
    }
}

/// The GPU passes the cost line names, each with the spans Bevy or this
/// workspace times it under. A pass drawn in two spans (the prepass early
/// and late, the light rays' march and composite) is their sum.
pub const PASSES: [(&str, &[&str]); 10] = [
    ("opaque", &["main_opaque_pass_3d"]),
    ("prepass", &["early prepass", "late prepass"]),
    ("shadows", &[SUN_SHADOWS_SPAN]),
    ("ssao", &["ssao"]),
    ("valley_fog", &["ground_fog"]),
    ("local_fog", &["local_fog"]),
    ("bloom", &["bloom"]),
    ("light_rays", &["light_rays_march", "light_rays_composite"]),
    ("auto_exposure", &["auto_exposure"]),
    ("tonemapping", &["tonemapping"]),
];

/// The names of a [`FrameCost::reading`]'s values, in order: what the
/// benchmark summary writes them as.
pub const COST_KEYS: [&str; 26] = [
    "gpu_opaque_ms",
    "gpu_prepass_ms",
    "gpu_shadows_ms",
    "gpu_ssao_ms",
    "gpu_valley_fog_ms",
    "gpu_local_fog_ms",
    "gpu_bloom_ms",
    "gpu_light_rays_ms",
    "gpu_auto_exposure_ms",
    "gpu_tonemapping_ms",
    "sets_total",
    "sets_opaque",
    "sets_alpha_mask",
    "sets_prepass",
    "sets_prepass_alpha_mask",
    "sets_shadow",
    "shadow_views",
    "meshes_added_per_s",
    "mesh_entities",
    "mesh_visible",
    "foliage_cells",
    "foliage_plants",
    "foliage_far_trees",
    "foliage_entities",
    "foliage_in_flight",
    "foliage_placed",
];

/// Where the frame went over the last log interval.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameCost {
    /// Smoothed GPU milliseconds per entry of [`PASSES`]; `None` for a pass
    /// that has never been timed in this run (not registered, or never
    /// drawn, like the light rays with the sun behind the camera).
    pub passes: [Option<f64>; PASSES.len()],
    /// The render world's batch sets in its last prepared frame.
    pub batch_sets: BatchSets,
    /// New `Mesh` assets per second: the uploads that keep the render
    /// thread's `allocate_and_free_meshes` busy.
    pub meshes_added_per_s: f64,
    /// Entities with a `Mesh3d`, which visibility walks every frame.
    pub mesh_entities: usize,
    /// `Mesh3d` entities the main camera found visible.
    pub mesh_visible: usize,
    /// The foliage streamer's own counts.
    pub foliage: FoliageStats,
    /// The terrain and collider streamers over the interval.
    pub stream: StreamRates,
    /// The longest main-world and render-world work of the interval,
    /// milliseconds: a hitch is one frame, which the spot readings miss.
    pub worst_work_ms: (f64, f64),
}

impl FrameCost {
    /// The cost as plain numbers in [`COST_KEYS`] order, absent passes as 0.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "counts far below 2^52 are exact in f64"
    )]
    pub fn reading(&self) -> [f64; COST_KEYS.len()] {
        let mut out = [0.0; COST_KEYS.len()];
        for (slot, pass) in out.iter_mut().zip(self.passes) {
            *slot = pass.unwrap_or(0.0);
        }
        let sets = self.batch_sets;
        let foliage = &self.foliage;
        let rest = [
            sets.total() as f64,
            sets.opaque as f64,
            sets.alpha_mask as f64,
            sets.prepass as f64,
            sets.prepass_alpha_mask as f64,
            sets.shadow as f64,
            sets.shadow_views as f64,
            self.meshes_added_per_s,
            self.mesh_entities as f64,
            self.mesh_visible as f64,
            foliage.cells as f64,
            foliage.trees as f64,
            foliage.far_trees as f64,
            foliage.entities as f64,
            foliage.in_flight as f64,
            foliage.placed as f64,
        ];
        out[PASSES.len()..].copy_from_slice(&rest);
        out
    }

    /// The cost line's text.
    fn line(&self) -> String {
        use std::fmt::Write as _;
        let mut text = String::from("cost | gpu ms:");
        for ((name, _), ms) in PASSES.iter().zip(self.passes) {
            let name = name.replace('_', " ");
            match ms {
                Some(ms) => {
                    let _ = write!(text, " {name} {ms:.2},");
                }
                None => {
                    let _ = write!(text, " {name} -,");
                }
            }
        }
        text.pop();
        let s = self.batch_sets;
        let f = &self.foliage;
        let _ = write!(
            text,
            " | sets {} (opaque {}, mask {}, prepass {}, prepass mask {}, shadow {} over {} views) | meshes +{:.0}/s, {} entities, {} visible | foliage {} cells, {} plants, {} far, {} entities, {} in flight, {} placed",
            s.total(),
            s.opaque,
            s.alpha_mask,
            s.prepass,
            s.prepass_alpha_mask,
            s.shadow,
            s.shadow_views,
            self.meshes_added_per_s,
            self.mesh_entities,
            self.mesh_visible,
            f.cells,
            f.trees,
            f.far_trees,
            f.entities,
            f.in_flight,
            f.placed,
        );
        let t = &self.stream;
        let _ = write!(
            text,
            " | terrain {:.0} pending (max {}), {:.0} tasks held (max {}), {:.2} uploads/frame (max {}), {:.1} cancelled/s | colliders {:.1} ground/s, +{:.0}/s -{:.0}/s entities | worst main {:.1} ms, render {:.1} ms",
            t.regions_pending_mean,
            t.regions_pending_max,
            t.terrain_tasks_mean,
            t.terrain_tasks_max,
            t.uploads_per_frame,
            t.uploads_max_frame,
            t.cancelled_per_s,
            t.heightfields_per_s,
            t.colliders_added_per_s,
            t.colliders_removed_per_s,
            self.worst_work_ms.0,
            self.worst_work_ms.1,
        );
        text
    }
}

/// What the cost line reads besides the diagnostics. The mesh events are
/// read through a cursor of their own, every frame, because Bevy keeps a
/// message for two frames and the line is written every two seconds.
#[derive(SystemParam)]
pub struct CostInputs<'w, 's> {
    mesh_events: Option<Res<'w, Messages<AssetEvent<Mesh>>>>,
    cursor: Local<'s, MessageCursor<AssetEvent<Mesh>>>,
    counts: Option<Res<'w, RenderCounts>>,
    foliage: Option<Res<'w, FoliageStats>>,
    mesh_entities: Query<'w, 's, (), With<Mesh3d>>,
    cameras: Query<
        'w,
        's,
        (&'static Camera, &'static VisibleEntities),
        (With<Camera3d>, Without<strata_terrain::OffWorld>),
    >,
    bench: Option<ResMut<'w, BenchRecorder>>,
}

impl CostInputs<'_, '_> {
    /// Counts this frame's new meshes.
    fn count_meshes(&mut self, stats: &mut FrameStats) {
        if let Some(events) = &self.mesh_events {
            let added = self
                .cursor
                .read(events)
                .filter(|event| matches!(event, AssetEvent::Added { .. }))
                .count();
            stats.meshes_added += added as u64;
        }
    }

    /// Closes the interval at `now`: the cost since the last call.
    fn close(
        &mut self,
        stats: &mut FrameStats,
        diagnostics: Option<&DiagnosticsStore>,
        clock: Option<&WorkClock>,
        now: f64,
    ) {
        let span = now - stats.cost_since;
        let stream = std::mem::take(&mut stats.stream_interval).rates(span);
        #[allow(
            clippy::cast_precision_loss,
            reason = "mesh counts per interval are far below 2^52"
        )]
        let per_s = if span > 0.0 {
            stats.meshes_added as f64 / span
        } else {
            0.0
        };
        stats.meshes_added = 0;
        stats.cost_since = now;
        let mut passes = [None; PASSES.len()];
        if let Some(diagnostics) = diagnostics {
            for (slot, (_, spans)) in passes.iter_mut().zip(PASSES) {
                *slot = pass_ms(diagnostics, spans);
            }
        }
        let mesh_visible = self
            .cameras
            .iter()
            .filter(|(camera, _)| camera.is_active)
            .map(|(_, visible)| visible.get(std::any::TypeId::of::<Mesh3d>()).len())
            .max()
            .unwrap_or(0);
        stats.cost = FrameCost {
            passes,
            batch_sets: self
                .counts
                .as_deref()
                .map_or_else(BatchSets::default, RenderCounts::batch_sets),
            meshes_added_per_s: per_s,
            mesh_entities: self.mesh_entities.iter().len(),
            mesh_visible,
            foliage: self.foliage.as_deref().copied().unwrap_or_default(),
            stream,
            worst_work_ms: clock.map_or((0.0, 0.0), WorkClock::take_worst),
        };
        if let Some(bench) = self.bench.as_deref_mut() {
            bench.push_cost(stats.cost.reading());
        }
    }
}

/// The smoothed GPU milliseconds of every span named in `spans`, summed,
/// wherever it nests; `None` when none of them has been timed.
fn pass_ms(diagnostics: &DiagnosticsStore, spans: &[&str]) -> Option<f64> {
    let mut total = None;
    for diagnostic in diagnostics.iter() {
        let path = diagnostic.path().as_str();
        let Some(rest) = path.strip_suffix("/elapsed_gpu") else {
            continue;
        };
        let name = rest.rsplit('/').next().unwrap_or(rest);
        if spans.contains(&name)
            && let Some(ms) = diagnostic.smoothed()
        {
            *total.get_or_insert(0.0) += ms;
        }
    }
    total
}

/// Accumulates, closes a window every [`WINDOW`], and logs every
/// [`LOG_EVERY_WINDOWS`] windows: the frame line, then the cost line.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
pub fn log_frame_stats(
    time: Res<Time<Real>>,
    mut stats: ResMut<FrameStats>,
    loaded: Res<LoadedRegions>,
    terrain: Option<Res<TerrainStats>>,
    origin: Res<RenderOrigin>,
    observers: Res<Observers>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    clock: Option<Res<WorkClock>>,
    mut cost: CostInputs,
    mut stream: StreamInputs,
) {
    cost.count_meshes(&mut stats);
    stats.stream = stream.sample();
    let frame = stats.stream;
    stats.stream_interval.add(&frame);
    // Virtual time is clamped (normally 250 ms), paused and scaled. Only
    // real time preserves the cost of a streaming stall in a flight log.
    if !stats.record_frame(&time) {
        return;
    }
    stats.gpu_ms = diagnostics.as_deref().map_or(0.0, gpu_ms);
    if stats.windows < LOG_EVERY_WINDOWS {
        return;
    }
    let (in_flight, built) = terrain.map_or((0, 0), |t| (t.in_flight, t.built));
    let cam = observers.primary.unwrap_or_default();
    let [p50, p95, p99] = stats.log_window.percentiles();
    // The last frame's work: a spot reading, which the benchmark's summary
    // turns into percentiles.
    let (main_ms, render_ms) = clock.as_deref().map_or((0.0, 0.0), WorkClock::last);
    info!(
        "{:.0} fps, avg {:.1} ms, p50 {p50:.1} ms, p95 {p95:.1} ms, p99 {p99:.1} ms, worst {:.1} ms, gpu {:.1} ms, main {main_ms:.1} ms, render {render_ms:.1} ms, hitches {} | regions {} ({} pending, {in_flight} meshing, {built} built) | cam ({:.0}, {:.0}, {:.0}) origin ({:.0}, {:.0}, {:.0})",
        f64::from(stats.log_window.frames) / stats.log_window.elapsed,
        stats.log_window.elapsed * 1000.0 / f64::from(stats.log_window.frames),
        stats.log_window.worst_ms,
        stats.gpu_ms,
        stats.hitches,
        loaded.len(),
        loaded.pending().count(),
        cam.x(),
        cam.y(),
        cam.z(),
        origin.0.x(),
        origin.0.y(),
        origin.0.z(),
    );
    cost.close(
        &mut stats,
        diagnostics.as_deref(),
        clock.as_deref(),
        time.elapsed_secs_f64(),
    );
    info!("{}", stats.cost.line());
    stats.windows = 0;
    stats.log_window.clear();
}

/// The CPU work of each frame, main world and render world apart, in
/// milliseconds: what a frame costs whatever paces it.
///
/// Wall-clock frame time on this project's development machine measures the
/// compositor as often as the game (a 60 Hz monitor, an unfocused window, a
/// virtual display for screenshots). The work does not wait on any of them:
/// the main world's is the span from `First` to `Last`, the render world's the
/// span of its `Render` schedule up to the end of its render graph (so before
/// presenting, which blocks on a slow display) less the time
/// `prepare_windows` spends waiting for a swapchain image. With pipelined rendering the two run side by
/// side, so a frame can take no less than the larger of them and the GPU time.
#[derive(Resource, Clone, Default)]
pub struct WorkClock {
    shared: Arc<Mutex<WorkSamples>>,
}

#[derive(Default)]
struct WorkSamples {
    main_start: Option<Instant>,
    render_start: Option<Instant>,
    windows_start: Option<Instant>,
    windows_ms: f64,
    last_main_ms: f64,
    last_render_ms: f64,
    /// The longest work of each world since [`WorkClock::take_worst`].
    worst_main_ms: f64,
    worst_render_ms: f64,
    recording: bool,
    main_ms: Vec<f64>,
    render_ms: Vec<f64>,
}

impl WorkClock {
    fn with<R>(&self, f: impl FnOnce(&mut WorkSamples) -> R) -> R {
        // A poisoned lock only means another frame's timing panicked; the
        // samples are still plain numbers.
        let mut samples = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut samples)
    }

    /// The last frame's main-world and render-world work, milliseconds.
    #[must_use]
    pub fn last(&self) -> (f64, f64) {
        self.with(|s| (s.last_main_ms, s.last_render_ms))
    }

    /// The longest main-world and render-world work since the last call,
    /// milliseconds; the next interval starts from nothing.
    #[must_use]
    pub fn take_worst(&self) -> (f64, f64) {
        self.with(|s| {
            let worst = (s.worst_main_ms, s.worst_render_ms);
            s.worst_main_ms = 0.0;
            s.worst_render_ms = 0.0;
            worst
        })
    }

    /// Adds the timing systems to the main world and, if it has one, the
    /// render world.
    pub fn register(app: &mut bevy::app::App) {
        use bevy::app::{First, Last};
        let clock = Self::default();
        app.insert_resource(clock.clone())
            .add_systems(First, main_begin)
            .add_systems(Last, main_end);
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            use bevy::render::renderer::{RenderGraph, RenderGraphSystems};
            use bevy::render::{Render, RenderSystems, view::window::prepare_windows};
            render.add_systems(RenderGraph, graph_end.after(RenderGraphSystems::Finish));
            render.insert_resource(clock).add_systems(
                Render,
                (
                    render_begin.before(RenderSystems::ExtractCommands),
                    windows_begin
                        .in_set(RenderSystems::PrepareViews)
                        .before(prepare_windows),
                    windows_end
                        .in_set(RenderSystems::PrepareViews)
                        .after(prepare_windows),
                ),
            );
        }
    }
}

fn elapsed_ms(since: Option<Instant>) -> f64 {
    since.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
}

fn main_begin(clock: Res<WorkClock>) {
    clock.with(|s| s.main_start = Some(Instant::now()));
}

fn main_end(clock: Res<WorkClock>) {
    clock.with(|s| {
        s.last_main_ms = elapsed_ms(s.main_start);
        s.worst_main_ms = s.worst_main_ms.max(s.last_main_ms);
        if s.recording {
            let ms = s.last_main_ms;
            s.main_ms.push(ms);
        }
    });
}

fn render_begin(clock: Res<WorkClock>) {
    clock.with(|s| {
        s.render_start = Some(Instant::now());
        s.windows_ms = 0.0;
    });
}

fn windows_begin(clock: Res<WorkClock>) {
    clock.with(|s| s.windows_start = Some(Instant::now()));
}

fn windows_end(clock: Res<WorkClock>) {
    clock.with(|s| s.windows_ms = elapsed_ms(s.windows_start));
}

fn graph_end(clock: Res<WorkClock>) {
    clock.with(|s| {
        s.last_render_ms = (elapsed_ms(s.render_start) - s.windows_ms).max(0.0);
        s.worst_render_ms = s.worst_render_ms.max(s.last_render_ms);
        if s.recording {
            let ms = s.last_render_ms;
            s.render_ms.push(ms);
        }
    });
}

/// Nearest-rank percentiles `p50`, `p95`, `p99`, the mean and the largest
/// of `values`.
fn spread(values: &[f64]) -> [f64; 5] {
    if values.is_empty() {
        return [0.0; 5];
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    let rank = |p: usize| sorted[(sorted.len() * p).div_ceil(100) - 1];
    #[allow(clippy::cast_precision_loss, reason = "frame counts far below 2^52")]
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    [rank(50), rank(95), rank(99), mean, sorted[sorted.len() - 1]]
}

/// How long the streamer must have placed nothing, with no job in flight,
/// before a settling warm-up ends: two log lines' worth.
pub const SETTLE_QUIET: Duration = Duration::from_secs(2);

/// The span a moving observer's progress is checked over: a walker that
/// got less far than a few metres in any span this long stopped in it, and
/// the run measured a static view.
pub const STALL_SPAN: Duration = Duration::from_secs(5);

/// When a benchmark's warm-up ends.
#[derive(Debug, Clone, Copy, PartialEq)]
enum WarmUp {
    /// After this many seconds of real time.
    Fixed(f64),
    /// Once the streamers are idle (the foliage streamer with no job in
    /// flight and its placed count unchanged, and with `terrain` also no
    /// region waiting for a mesh and no meshing task held, all for
    /// [`SETTLE_QUIET`]), but not before `floor` seconds and not after
    /// `ceiling`.
    Settle {
        floor: f64,
        ceiling: f64,
        quiet: f64,
        placed: Option<u64>,
        terrain: bool,
    },
}

/// How a benchmark's warm-up ended, for the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmUpEnd {
    /// The fixed warm-up ran out.
    Fixed,
    /// The streamers went idle.
    Settled,
    /// The ceiling came first: the measured frames include streaming.
    Ceiling,
}

impl WarmUpEnd {
    /// The word the summary writes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Settled => "settled",
            Self::Ceiling => "ceiling",
        }
    }
}

/// What one benchmark frame reads besides its times.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BenchFrame {
    /// The foliage streamer's placed count and jobs in flight.
    pub foliage: (u64, usize),
    /// What the terrain and collider streamers did.
    pub stream: StreamFrame,
    /// The primary observer's horizontal position, world `(x, z)`.
    pub observer: Option<(f64, f64)>,
}

/// Every frame of a benchmark run after its warm-up, for one summary at the end.
///
/// The log's windows are for watching a run; a benchmark needs percentiles
/// over the whole measured stretch, which a percentile of window percentiles is
/// not. Inserting this resource turns recording on; nothing else pays for it.
///
/// A fixed warm-up measured the static fixtures while the far foliage ring
/// was still filling (about 50 s around a new camera, with 150 to 240 new
/// meshes a second; `docs/perf/profile-2026-10-03.md`), so their p95 was
/// mostly streaming. [`BenchRecorder::until_settled`] waits for the
/// streamer instead and measures the settled frame.
///
/// A moving fixture with a fixed warm-up measured mostly its first terrain
/// fill (31 to 37 s of two uploads a frame around the new camera;
/// `docs/perf/streaming-2026-10-04.md`), and its walker could stop against
/// a wall unnoticed. [`BenchRecorder::holding`] keeps the motion at its
/// start (a [`MotionHold`]) until terrain and foliage are idle, lets it go,
/// and measures from a lead after that; every measured frame also records
/// where the observer is, so a summary says how far it went.
#[derive(Resource, Debug)]
pub struct BenchRecorder {
    warm_up: WarmUp,
    /// Seconds a held run moves before it is measured; `None` when the run
    /// holds nothing.
    lead: Option<f64>,
    elapsed: f64,
    /// Real time the held motion was let go at, and how the wait ended.
    released: Option<(f64, WarmUpEnd)>,
    /// Real time the warm-up took and how it ended, once measuring began.
    started: Option<(f64, WarmUpEnd)>,
    frames_ms: Vec<f64>,
    gpu_ms: Vec<f64>,
    cost: Vec<[f64; COST_KEYS.len()]>,
    stream: StreamTotals,
    /// The observer over the measured frames: real time, world `x`, `z`.
    track: Vec<(f64, f64, f64)>,
}

/// How far the observer went over the measured stretch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Travel {
    /// Length of the path, metres, horizontal.
    pub metres: f64,
    /// Mean horizontal speed, metres per second.
    pub speed_mps: f64,
    /// The least horizontal distance covered in any [`STALL_SPAN`] of the
    /// stretch, metres; `None` when the stretch is shorter than that.
    pub slowest_span_m: Option<f64>,
}

impl Travel {
    /// The travel along `track` (real time, `x`, `z`, in time order);
    /// `None` without two points apart in time.
    #[must_use]
    fn along(track: &[(f64, f64, f64)]) -> Option<Self> {
        let (first, last) = (track.first()?, track.last()?);
        let seconds = last.0 - first.0;
        if seconds <= 0.0 {
            return None;
        }
        let gap = |a: &(f64, f64, f64), b: &(f64, f64, f64)| (b.1 - a.1).hypot(b.2 - a.2);
        let metres: f64 = track.windows(2).map(|w| gap(&w[0], &w[1])).sum();
        // Two pointers: for each start, the first point a span later.
        let span = STALL_SPAN.as_secs_f64();
        let mut slowest: Option<f64> = None;
        let mut j = 0;
        for (i, from) in track.iter().enumerate() {
            j = j.max(i);
            while j < track.len() && track[j].0 - from.0 < span {
                j += 1;
            }
            let Some(to) = track.get(j) else {
                break;
            };
            let covered = gap(from, to);
            slowest = Some(slowest.map_or(covered, |s| s.min(covered)));
        }
        Some(Self {
            metres,
            speed_mps: metres / seconds,
            slowest_span_m: slowest,
        })
    }
}

/// What a benchmark run measured after its warm-up. Times in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchSummary {
    /// Frames measured.
    pub frames: usize,
    /// Mean frame time.
    pub mean_ms: f64,
    /// Median frame time.
    pub p50_ms: f64,
    /// 95th percentile frame time.
    pub p95_ms: f64,
    /// 99th percentile frame time.
    pub p99_ms: f64,
    /// The longest frame.
    pub worst_ms: f64,
    /// Frames longer than 33 ms: a streaming hitch at any refresh rate.
    pub over_33ms: usize,
    /// Mean of the smoothed GPU camera time, sampled every measured frame.
    pub gpu_mean_ms: f64,
    /// Main-world work per frame: `p50`, `p95`, `p99`, mean, the longest.
    pub main_ms: [f64; 5],
    /// Render-world CPU work per frame, less the swapchain wait: `p50`,
    /// `p95`, `p99`, mean, the longest.
    pub render_ms: [f64; 5],
    /// Seconds of real time the warm-up took.
    pub warmup_s: f64,
    /// How the warm-up ended.
    pub warm_up: WarmUpEnd,
    /// Seconds of real time the motion was held at its start, for a run
    /// that held it.
    pub held_s: Option<f64>,
    /// Medians of the cost line's readings over the measured stretch, in
    /// [`COST_KEYS`] order; `None` when no line was written after the
    /// warm-up.
    pub cost: Option<[f64; COST_KEYS.len()]>,
    /// The terrain and collider streamers over the measured stretch.
    pub stream: StreamRates,
    /// How far the primary observer went; `None` without one.
    pub travel: Option<Travel>,
}

impl BenchRecorder {
    /// Record every frame after `warmup` of real time.
    #[must_use]
    pub fn new(warmup: Duration) -> Self {
        Self::with(WarmUp::Fixed(warmup.as_secs_f64()), None)
    }

    /// Record every frame once the foliage streamer is idle, after at least
    /// `floor` and at most `ceiling` of real time.
    #[must_use]
    pub fn until_settled(floor: Duration, ceiling: Duration) -> Self {
        Self::with(Self::settle(floor, ceiling, false), None)
    }

    /// Hold the scripted motion until terrain and foliage are idle (after
    /// at least `floor` and at most `ceiling` of real time), let it go, and
    /// record every frame from `lead` after that.
    #[must_use]
    pub fn holding(floor: Duration, ceiling: Duration, lead: Duration) -> Self {
        Self::with(Self::settle(floor, ceiling, true), Some(lead.as_secs_f64()))
    }

    fn settle(floor: Duration, ceiling: Duration, terrain: bool) -> WarmUp {
        WarmUp::Settle {
            floor: floor.as_secs_f64(),
            ceiling: ceiling.as_secs_f64().max(floor.as_secs_f64()),
            quiet: 0.0,
            placed: None,
            terrain,
        }
    }

    fn with(warm_up: WarmUp, lead: Option<f64>) -> Self {
        Self {
            warm_up,
            lead,
            elapsed: 0.0,
            released: None,
            started: None,
            frames_ms: Vec::new(),
            gpu_ms: Vec::new(),
            cost: Vec::new(),
            stream: StreamTotals::default(),
            track: Vec::new(),
        }
    }

    /// Frames measured so far.
    #[must_use]
    pub fn measured_frames(&self) -> usize {
        self.frames_ms.len()
    }

    /// Seconds of real time measured so far.
    #[must_use]
    pub fn measured_secs(&self) -> f64 {
        self.frames_ms.iter().sum::<f64>() / 1000.0
    }

    /// Whether a held run's wait has ended, so its motion may start.
    #[must_use]
    pub fn released(&self) -> bool {
        self.released.is_some()
    }

    /// Whether the warm-up has ended, given this frame's streamers, with
    /// `dt` seconds since the last.
    fn warm_up_over(&mut self, dt: f64, frame: &BenchFrame) -> Option<WarmUpEnd> {
        let elapsed = self.elapsed;
        match &mut self.warm_up {
            WarmUp::Fixed(warmup) => (elapsed > *warmup).then_some(WarmUpEnd::Fixed),
            WarmUp::Settle {
                floor,
                ceiling,
                quiet,
                placed,
                terrain,
            } => {
                let (now, in_flight) = frame.foliage;
                let ground_idle = !*terrain
                    || (frame.stream.regions_pending == 0 && frame.stream.terrain_tasks == 0);
                if *placed == Some(now) && in_flight == 0 && ground_idle {
                    *quiet += dt;
                } else {
                    *quiet = 0.0;
                    *placed = Some(now);
                }
                if elapsed >= *floor && *quiet >= SETTLE_QUIET.as_secs_f64() {
                    Some(WarmUpEnd::Settled)
                } else {
                    (elapsed >= *ceiling).then_some(WarmUpEnd::Ceiling)
                }
            }
        }
    }

    fn push(&mut self, dt: f64, gpu_ms: f64, clock: Option<&WorkClock>, frame: &BenchFrame) {
        self.elapsed += dt;
        let elapsed = self.elapsed;
        if self.started.is_none() {
            self.started = match self.lead {
                None => self.warm_up_over(dt, frame).map(|end| (elapsed, end)),
                Some(lead) => {
                    if self.released.is_none() {
                        self.released = self.warm_up_over(dt, frame).map(|end| (elapsed, end));
                    }
                    self.released
                        .filter(|(at, _)| elapsed >= at + lead)
                        .map(|(_, end)| (elapsed, end))
                }
            };
        }
        let on = self.started.is_some();
        if let Some(clock) = clock {
            clock.with(|s| s.recording = on);
        }
        if on {
            self.frames_ms.push(dt * 1000.0);
            self.gpu_ms.push(gpu_ms);
            self.stream.add(&frame.stream);
            if let Some((x, z)) = frame.observer {
                self.track.push((elapsed, x, z));
            }
        }
    }

    /// Keeps one cost line's reading if the warm-up is over.
    fn push_cost(&mut self, reading: [f64; COST_KEYS.len()]) {
        if self.started.is_some() {
            self.cost.push(reading);
        }
    }

    /// The summary so far; `None` before the first measured frame.
    #[must_use]
    pub fn summary(&self) -> Option<BenchSummary> {
        self.summary_with(None)
    }

    /// The summary so far, with the work `clock` recorded; `None` before the
    /// first measured frame.
    #[must_use]
    pub fn summary_with(&self, clock: Option<&WorkClock>) -> Option<BenchSummary> {
        let (warmup_s, warm_up) = self.started?;
        if self.frames_ms.is_empty() {
            return None;
        }
        let mut sorted = self.frames_ms.clone();
        sorted.sort_unstable_by(f64::total_cmp);
        let rank = |p: usize| sorted[(sorted.len() * p).div_ceil(100) - 1];
        // Frame counts stay far below 2^52; the casts are exact.
        #[allow(clippy::cast_precision_loss)]
        let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
        let cost = (!self.cost.is_empty()).then(|| {
            std::array::from_fn(|key| {
                let column: Vec<f64> = self.cost.iter().map(|reading| reading[key]).collect();
                spread(&column)[0]
            })
        });
        Some(BenchSummary {
            frames: sorted.len(),
            mean_ms: mean(&self.frames_ms),
            p50_ms: rank(50),
            p95_ms: rank(95),
            p99_ms: rank(99),
            worst_ms: sorted[sorted.len() - 1],
            over_33ms: sorted.iter().filter(|ms| **ms > 33.0).count(),
            gpu_mean_ms: mean(&self.gpu_ms),
            main_ms: clock.map_or([0.0; 5], |c| c.with(|s| spread(&s.main_ms))),
            render_ms: clock.map_or([0.0; 5], |c| c.with(|s| spread(&s.render_ms))),
            warmup_s,
            warm_up,
            held_s: self.released.map(|(at, _)| at),
            cost,
            stream: self.stream.rates(self.measured_secs()),
            travel: Travel::along(&self.track),
        })
    }
}

/// Feed the benchmark recorder, when there is one, after the statistics,
/// and let a held run's motion go once its wait is over.
pub fn record_bench(
    time: Res<Time<Real>>,
    stats: Res<FrameStats>,
    bench: Option<ResMut<BenchRecorder>>,
    clock: Option<Res<WorkClock>>,
    foliage: Option<Res<FoliageStats>>,
    observers: Res<Observers>,
    hold: Option<ResMut<MotionHold>>,
) {
    let Some(mut bench) = bench else {
        return;
    };
    if !stats.warmed {
        return;
    }
    let frame = BenchFrame {
        // With no foliage the streamer is idle from the start, and a
        // settling warm-up ends at its floor.
        foliage: foliage.map_or((0, 0), |f| (f.placed, f.in_flight)),
        stream: stats.stream,
        observer: observers.primary.map(|at| (at.x(), at.z())),
    };
    bench.push(
        time.delta_secs_f64(),
        stats.gpu_ms,
        clock.as_deref(),
        &frame,
    );
    if let Some(mut hold) = hold
        && hold.held()
        && bench.released()
    {
        hold.release();
        info!(
            "streamers idle after {:.1} s; scripted motion starts",
            bench.elapsed
        );
    }
}

/// The smoothed whole-camera GPU interval, or summed spans without that scope.
///
/// Bevy names a span's GPU time `render/<span>/elapsed_gpu` and nests
/// sub-spans under their parent, so only the three-component paths are
/// added or a pass would be counted twice.
fn gpu_ms(diagnostics: &DiagnosticsStore) -> f64 {
    // The atmosphere plugin brackets the camera driver. Prefer that whole
    // interval: Bevy's fog and DOF do not carry their own timing scopes.
    if let Some(value) = diagnostics
        .iter()
        .find(|d| d.path().as_str() == "render/camera_frame/elapsed_gpu")
        .and_then(bevy::diagnostic::Diagnostic::smoothed)
    {
        return value;
    }
    diagnostics
        .iter()
        .filter(|d| {
            let path = d.path().as_str();
            path.starts_with("render/")
                && path.ends_with("/elapsed_gpu")
                && path.matches('/').count() == 2
        })
        .filter_map(bevy::diagnostic::Diagnostic::smoothed)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame where only the foliage streamer reports.
    fn foliage(placed: u64, in_flight: usize) -> BenchFrame {
        BenchFrame {
            foliage: (placed, in_flight),
            ..BenchFrame::default()
        }
    }

    fn assert_percentiles(actual: [f64; 3], expected: [f64; 3]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn a_benchmark_measures_only_after_its_warm_up() {
        let mut bench = BenchRecorder::new(Duration::from_secs(1));
        assert_eq!(bench.summary(), None);
        for _ in 0..99 {
            bench.push(0.01, 2.0, None, &foliage(0, 0));
        }
        assert_eq!(bench.summary(), None, "still inside the warm-up");
        for milliseconds in (1..=100).rev() {
            bench.push(f64::from(milliseconds) / 1000.0, 3.0, None, &foliage(0, 0));
        }
        bench.push(0.040, 3.0, None, &foliage(0, 0));
        let summary = bench.summary().expect("measured frames");
        assert_eq!(summary.frames, 101);
        // 1..=100 ms and one more 40 ms frame: the 51st of 101 is 50 ms.
        assert!((summary.p50_ms - 50.0).abs() < 1e-9, "{summary:?}");
        assert!((summary.worst_ms - 100.0).abs() < 1e-9);
        assert_eq!(summary.over_33ms, 68);
        assert!((summary.gpu_mean_ms - 3.0).abs() < 1e-9);
        assert_eq!(summary.warm_up, WarmUpEnd::Fixed);
    }

    #[test]
    fn a_settling_warm_up_waits_for_the_streamer_between_its_bounds() {
        let floor = Duration::from_secs(10);
        let ceiling = Duration::from_secs(90);
        // Quiet from the start: measuring begins at the floor.
        let mut idle = BenchRecorder::until_settled(floor, ceiling);
        for _ in 0..1100 {
            idle.push(0.01, 1.0, None, &foliage(5, 0));
        }
        let summary = idle.summary().expect("measured after the floor");
        assert_eq!(summary.warm_up, WarmUpEnd::Settled);
        assert!((summary.warmup_s - 10.0).abs() < 0.02, "{summary:?}");

        // Placing for 30 s, then a job in flight with nothing new placed for
        // 5 s, then quiet: measuring begins two seconds into the quiet.
        let mut busy = BenchRecorder::until_settled(floor, ceiling);
        let mut placed = 0;
        for frame in 0..6000_u32 {
            let t = f64::from(frame) * 0.01;
            if t < 30.0 {
                placed += 1;
            }
            let in_flight = usize::from((30.0..35.0).contains(&t));
            busy.push(0.01, 1.0, None, &foliage(placed, in_flight));
        }
        let summary = busy.summary().expect("measured after settling");
        assert_eq!(summary.warm_up, WarmUpEnd::Settled);
        assert!((summary.warmup_s - 37.0).abs() < 0.05, "{summary:?}");

        // Never quiet: the ceiling ends it.
        let mut endless = BenchRecorder::until_settled(floor, ceiling);
        for frame in 0..10_000_u64 {
            endless.push(0.01, 1.0, None, &foliage(frame, 1));
        }
        let summary = endless.summary().expect("measured after the ceiling");
        assert_eq!(summary.warm_up, WarmUpEnd::Ceiling);
        assert!((summary.warmup_s - 90.0).abs() < 0.02, "{summary:?}");
        assert_eq!(summary.cost, None, "no cost line was written");
    }

    #[test]
    fn a_held_run_waits_for_the_terrain_too_then_measures_after_its_lead() {
        let floor = Duration::from_secs(10);
        let ceiling = Duration::from_secs(90);
        let lead = Duration::from_secs(5);
        let mut held = BenchRecorder::holding(floor, ceiling, lead);
        // Foliage idle from the start, terrain busy for 30 s: the wait
        // ends two seconds into the terrain's quiet, at 32 s, and measuring
        // five seconds later. A walker standing until then, then at 2 m/s.
        let mut released_at = None;
        for frame in 0..6000_u32 {
            let t = f64::from(frame + 1) * 0.01;
            let busy = t < 30.0;
            let x = released_at.map_or(0.0, |at: f64| (t - at) * 2.0);
            let input = BenchFrame {
                foliage: (7, 0),
                stream: StreamFrame {
                    regions_pending: usize::from(busy) * 3,
                    terrain_tasks: usize::from(busy) * 4,
                    terrain_uploads: 1,
                    ..StreamFrame::default()
                },
                observer: Some((x, 0.0)),
            };
            held.push(0.01, 1.0, None, &input);
            if released_at.is_none() && held.released() {
                released_at = Some(t);
            }
        }
        let released_at = released_at.expect("released");
        assert!((released_at - 32.0).abs() < 0.02, "{released_at}");
        let summary = held.summary().expect("measured after the lead");
        assert_eq!(summary.warm_up, WarmUpEnd::Settled);
        assert!((summary.warmup_s - 37.0).abs() < 0.02, "{summary:?}");
        assert!((summary.held_s.expect("held") - 32.0).abs() < 0.02);
        let travel = summary.travel.expect("an observer");
        assert!((travel.speed_mps - 2.0).abs() < 1e-6, "{travel:?}");
        assert!((travel.slowest_span_m.expect("long enough") - 10.0).abs() < 0.03);
        assert!((summary.stream.uploads_per_frame - 1.0).abs() < 1e-9);
        assert!((summary.stream.uploads_per_s - 100.0).abs() < 0.1);
        assert_eq!(summary.stream.terrain_tasks_max, 0);

        // A settling warm-up of a still fixture ignores the terrain.
        let mut still = BenchRecorder::until_settled(floor, ceiling);
        for _ in 0..1100 {
            let input = BenchFrame {
                foliage: (5, 0),
                stream: StreamFrame {
                    terrain_tasks: 9,
                    ..StreamFrame::default()
                },
                observer: None,
            };
            still.push(0.01, 1.0, None, &input);
        }
        let summary = still.summary().expect("settled at the floor");
        assert!((summary.warmup_s - 10.0).abs() < 0.02, "{summary:?}");
        assert_eq!(summary.held_s, None);
        assert_eq!(summary.travel, None);
    }

    #[test]
    fn a_walker_that_stops_shows_in_its_slowest_span() {
        // 20 s at 5 m/s, 10 s standing, 20 s at 5 m/s, sampled at 10 Hz.
        let track: Vec<(f64, f64, f64)> = (0..=500)
            .map(|i| {
                let t = f64::from(i) * 0.1;
                let x = 5.0 * t.min(20.0) + 5.0 * (t - 30.0).max(0.0);
                (t, x, 0.0)
            })
            .collect();
        let travel = Travel::along(&track).expect("moved");
        assert!((travel.metres - 200.0).abs() < 1e-6, "{travel:?}");
        assert!((travel.speed_mps - 4.0).abs() < 1e-6);
        assert!(travel.slowest_span_m.expect("long enough") < 1e-9);
        assert_eq!(
            Travel::along(&track[..10]).map(|t| t.slowest_span_m),
            Some(None)
        );
        assert_eq!(Travel::along(&track[..1]), None);
    }

    #[test]
    fn cost_readings_are_kept_only_after_the_warm_up_as_medians() {
        let mut bench = BenchRecorder::new(Duration::from_secs(1));
        let mut reading = [0.0; COST_KEYS.len()];
        reading[0] = 100.0;
        bench.push_cost(reading);
        bench.push(2.0, 1.0, None, &foliage(0, 0));
        for value in [1.0, 3.0, 2.0] {
            reading[0] = value;
            bench.push_cost(reading);
        }
        let cost = bench.summary().and_then(|s| s.cost).expect("readings");
        assert!((cost[0] - 2.0).abs() < f64::EPSILON, "{cost:?}");
    }

    #[test]
    fn a_pass_sums_its_spans_wherever_they_nest() {
        use bevy::diagnostic::{Diagnostic, DiagnosticMeasurement, DiagnosticPath};
        let mut store = DiagnosticsStore::default();
        for (path, ms) in [
            ("render/camera_frame/early prepass/elapsed_gpu", 0.5),
            ("render/camera_frame/late prepass/elapsed_gpu", 0.25),
            ("render/camera_frame/early prepass/elapsed_cpu", 9.0),
            ("render/camera_frame/sun_shadows/elapsed_gpu", 1.5),
        ] {
            let path = DiagnosticPath::new(path);
            let mut diagnostic = Diagnostic::new(path);
            diagnostic.add_measurement(DiagnosticMeasurement {
                time: Instant::now(),
                value: ms,
            });
            store.add(diagnostic);
        }
        let prepass = pass_ms(&store, PASSES[1].1).expect("timed");
        assert!((prepass - 0.75).abs() < 1e-9);
        let shadows = pass_ms(&store, PASSES[2].1).expect("timed");
        assert!((shadows - 1.5).abs() < 1e-9);
        assert_eq!(pass_ms(&store, PASSES[3].1), None);
    }

    #[test]
    fn real_time_preserves_a_stall_longer_than_the_virtual_clamp() {
        let mut stats = FrameStats::default();
        let mut time = Time::<Real>::default();
        assert!(!stats.record_frame(&time));
        time.advance_by(Duration::from_millis(750));
        assert!(stats.record_frame(&time));
        assert!((stats.last_worst_ms - 750.0).abs() < f64::EPSILON);
        assert!((stats.avg_ms - 750.0).abs() < f64::EPSILON);
        assert!((stats.p99_ms - 750.0).abs() < f64::EPSILON);
        assert_eq!(stats.hitches, 1);
        assert!((stats.fps - 1.0 / 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn nearest_rank_percentiles_capture_the_tail() {
        let mut window = FrameWindow::default();
        assert_percentiles(window.percentiles(), [0.0; 3]);
        for milliseconds in (1..=100).rev() {
            window.push(f64::from(milliseconds) / 1000.0);
        }
        assert_percentiles(window.percentiles(), [50.0, 95.0, 99.0]);
    }

    #[test]
    fn log_window_retains_hitches_from_previous_hud_windows() {
        let mut stats = FrameStats::default();
        let mut time = Time::<Real>::default();
        stats.record_frame(&time);
        time.advance_by(Duration::from_millis(750));
        assert!(stats.record_frame(&time));
        for _ in 0..11 {
            time.advance_by(Duration::from_millis(50));
            stats.record_frame(&time);
        }
        assert!((stats.last_worst_ms - 50.0).abs() < f64::EPSILON);
        assert!((stats.log_window.worst_ms - 750.0).abs() < f64::EPSILON);
        assert_percentiles(stats.log_window.percentiles(), [50.0, 750.0, 750.0]);
        assert_eq!(stats.log_window.frames, 12);
    }

    #[test]
    fn bounded_percentile_samples_keep_all_frame_totals() {
        let mut window = FrameWindow::default();
        window.push(1.0);
        for _ in 0..MAX_SAMPLES {
            window.push(0.01);
        }
        assert_eq!(window.samples.len(), MAX_SAMPLES);
        assert_eq!(window.frames, 4097);
        assert!((window.worst_ms - 1000.0).abs() < f64::EPSILON);
        assert_percentiles(window.percentiles(), [10.0; 3]);
        window.clear();
        assert_eq!(window.frames, 0);
        assert!(window.samples.is_empty());
    }
}
