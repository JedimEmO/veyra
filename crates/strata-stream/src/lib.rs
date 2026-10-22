//! Streaming: which regions of the world should exist right now, and what
//! changed since last frame.
//!
//! The selection is a pure function of observer positions and a
//! [`StreamConfig`] (ADR 0003, [`select()`]), so it is tested headless, and
//! the diff against the loaded set is emitted under a per-frame budget.
//! This crate decides *that* a region is wanted and never learns what a
//! `Mesh` is; `strata-terrain` turns wanted regions into meshes and never
//! learns why they were wanted.
//!
//! # The contract with the terrain crate
//!
//! Every frame in [`StreamSet::Diff`] (in `Update`, only in
//! [`AppState::InWorld`]) this crate:
//!
//! 1. Reads [`RegionReady`] messages and marks those regions
//!    [`RegionState::Ready`], both in [`LoadedRegions`] and in the entity's
//!    [`Region`] component.
//! 2. Spawns an entity `(Region { coord, state: Requested },
//!    WorldPosition(coord.min_corner()))` for each newly wanted region, at
//!    most [`StreamConfig::max_loads_per_frame`] per frame, nearest first,
//!    and writes [`RegionLoad`]`(coord, entity)` for each.
//! 3. Despawns regions that are no longer wanted, at most
//!    [`StreamConfig::max_unloads_per_frame`] per frame, farthest first, and
//!    writes [`RegionUnload`]`(coord, entity)` for each. The entity is
//!    despawned (with its children) when the diff system's commands apply,
//!    which is before anything ordered after `StreamSet::Diff` runs: a
//!    `RegionUnload` names an entity that is already gone. Key in-flight
//!    work by the `Entity` or the coord and drop it on unload.
//!
//! The terrain crate, ordered after [`StreamSet::Diff`]:
//!
//! - Reads [`RegionLoad`], starts sampling and meshing for the coord, and
//!   when the mesh is on the entity writes [`RegionReady`]`(coord, entity)`.
//!   A ready message for an entity that was unloaded meanwhile is ignored.
//! - Inserts [`ExternalRegionBuilder`] once (in its plugin) to say it will
//!   answer. Without that resource the streamer marks regions ready itself
//!   the frame after they load, which is what a headless server wants.
//! - Never despawns a region entity and never writes its `Transform`: the
//!   position is a `WorldPosition` and `strata-app` derives the transform
//!   from it (ADR 0002).
//!
//! Both systems do work only on a change. The walk reruns when an observer
//! has moved [`RESELECT_DISTANCE_M`] since the last one, when observers
//! come or go, or when [`DetailZones`] or the [`StreamConfig`] change; and
//! [`WantedRegions`] is flagged changed only when its set changed, not its
//! order. The diff keeps what it derives from the wanted set between frames
//! and runs its load pass while wanted regions are missing and its unload
//! pass when the wanted set changed or a region became ready. At rest both
//! cost a few comparisons a frame.
//!
//! No holes: when a node splits (its four children become wanted) or
//! children merge (their parent becomes wanted), the old entities stay
//! loaded until every replacement has reported ready; only then are they
//! unloaded. The loaded set can therefore transiently contain overlapping
//! nodes, and the terrain crate should expect two meshes for the same
//! ground for a few frames. Regions that leave the wanted set with nothing
//! replacing them (behind the horizon) unload at once.

pub mod select;

use std::collections::{HashMap, HashSet};

use bevy::app::{App, Plugin, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::state::condition::in_state;
use strata_app::{AppState, Observers, WorldPosition};
use strata_core::{RegionCoord, WorldPos};

pub use select::{
    DetailZone, DetailZones, Selector, StreamConfig, distance_to_region, nearest_distance,
    overlaps, select,
};

/// The streaming sets, in order, in `Update`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamSet {
    /// Quadtree walk from each observer: the wanted set of regions.
    Select,
    /// Wanted against loaded: load and unload requests, budgeted.
    Diff,
}

/// A region entity: one node of the quadtree that currently exists.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// Which square of the world, at which LOD.
    pub coord: RegionCoord,
    /// Whether the region's content has arrived.
    pub state: RegionState,
}

/// Where a region is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RegionState {
    /// A `RegionLoad` was written; nothing has answered yet.
    #[default]
    Requested,
    /// A `RegionReady` arrived; the region shows on screen (or, headless,
    /// counts as present).
    Ready,
}

/// A region became wanted; its entity exists with a `Region` and a
/// `WorldPosition` at the region's minimum corner.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionLoad(pub RegionCoord, pub Entity);

/// A region stopped being wanted; its entity is despawned by the time this
/// is readable.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionUnload(pub RegionCoord, pub Entity);

/// Sent by whoever builds regions once this one is complete. Ignored when
/// the entity is not the current one for the coord.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionReady(pub RegionCoord, pub Entity);

/// Present when something outside this crate answers `RegionLoad` with
/// `RegionReady`. Absent, the streamer marks every requested region ready
/// on the next frame, so a headless app still cycles regions.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ExternalRegionBuilder;

/// How far, horizontally, an observer moves before the quadtree is walked
/// again. A split distance is `split_factor` times a node's size, 160 m at
/// the finest LOD, so a metre moves no threshold by more than a metre and
/// the wanted set lags the observer by at most that; the hysteresis band
/// beyond it is tens of metres.
pub const RESELECT_DISTANCE_M: f64 = 1.0;

/// What the selection wants to exist, refreshed in [`StreamSet::Select`].
///
/// Change detection fires when the set changed, not when it was walked
/// again with the same result or only reordered.
#[derive(Resource, Debug, Clone, Default)]
pub struct WantedRegions {
    selector: Selector,
    /// The observers the last walk was made from, in order.
    anchors: Vec<WorldPos>,
    /// Whether [`DetailZones`] existed at the last walk, so its removal is
    /// a change too.
    zoned: bool,
}

impl WantedRegions {
    /// The wanted set, disjoint, nearest first (as of the last walk).
    #[must_use]
    pub fn regions(&self) -> &[RegionCoord] {
        self.selector.regions()
    }

    /// Whether `coord` is wanted, without a scan.
    #[must_use]
    pub fn contains(&self, coord: RegionCoord) -> bool {
        self.selector.contains(coord)
    }

    /// Whether the observers are where the last walk saw them, to within
    /// [`RESELECT_DISTANCE_M`]. Height is ignored as the selection ignores
    /// it.
    fn observers_still(&self, observers: &[WorldPos]) -> bool {
        observers.len() == self.anchors.len()
            && observers.iter().zip(&self.anchors).all(|(now, then)| {
                (now.xz() - then.xz()).length_squared() < RESELECT_DISTANCE_M * RESELECT_DISTANCE_M
            })
    }
}

/// One entry of [`LoadedRegions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadedRegion {
    /// The region entity.
    pub entity: Entity,
    /// Whether a `RegionReady` has arrived for it.
    pub state: RegionState,
}

/// Every region entity that exists, by coord. The streamer's view of the
/// world; the `Region` components mirror it for systems that query.
#[derive(Resource, Debug, Clone, Default)]
pub struct LoadedRegions {
    regions: HashMap<RegionCoord, LoadedRegion>,
}

impl LoadedRegions {
    /// The entry for `coord`, if it exists.
    #[must_use]
    pub fn get(&self, coord: RegionCoord) -> Option<LoadedRegion> {
        self.regions.get(&coord).copied()
    }

    /// The entity for `coord`, if it exists.
    #[must_use]
    pub fn entity(&self, coord: RegionCoord) -> Option<Entity> {
        self.get(coord).map(|r| r.entity)
    }

    /// Whether `coord` exists and is ready.
    #[must_use]
    pub fn is_ready(&self, coord: RegionCoord) -> bool {
        self.get(coord)
            .is_some_and(|r| r.state == RegionState::Ready)
    }

    /// Every loaded region.
    pub fn iter(&self) -> impl Iterator<Item = (RegionCoord, LoadedRegion)> + '_ {
        self.regions.iter().map(|(c, r)| (*c, *r))
    }

    /// The regions still waiting for a `RegionReady`.
    pub fn pending(&self) -> impl Iterator<Item = RegionCoord> + '_ {
        self.regions
            .iter()
            .filter(|(_, r)| r.state == RegionState::Requested)
            .map(|(c, _)| *c)
    }

    /// How many are loaded, ready or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// Whether nothing is loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The ready regions covering `pos`. More than one only during a
    /// split or merge.
    pub fn ready_containing(&self, pos: WorldPos) -> impl Iterator<Item = RegionCoord> + '_ {
        self.regions
            .iter()
            .filter(move |(c, r)| r.state == RegionState::Ready && c.contains(pos))
            .map(|(c, _)| *c)
    }
}

/// Registers the streaming sets, the resources and the messages; the
/// systems run only in [`AppState::InWorld`]. Insert a [`StreamConfig`]
/// before adding the plugin to override the defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamPlugin;

impl Plugin for StreamPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreamConfig>()
            .init_resource::<WantedRegions>()
            .init_resource::<LoadedRegions>()
            .add_message::<RegionLoad>()
            .add_message::<RegionUnload>()
            .add_message::<RegionReady>()
            .configure_sets(
                Update,
                (StreamSet::Select, StreamSet::Diff)
                    .chain()
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                (
                    zone_settlements.before(StreamSet::Select),
                    select_regions.in_set(StreamSet::Select),
                    diff_regions.in_set(StreamSet::Diff),
                ),
            );
    }
}

/// A settlement's ground is kept at the finest LOD (1 m, the grid its walls
/// are traced on and physics collides with) while an observer is near
/// enough to see a metre's error, and one level coarser while its buildings
/// are drawn at all; see [`DetailZone`].
const SETTLEMENT_FINE_REACH_M: f64 = 900.0;
const SETTLEMENT_REACH_M: f64 = 2_600.0;

/// Keeps [`DetailZones`] in step with the placed settlements.
fn zone_settlements(
    mut commands: Commands,
    settlements: Option<Res<strata_app::Settlements>>,
    zones: Option<Res<DetailZones>>,
) {
    let Some(settlements) = settlements else {
        return;
    };
    if !settlements.is_changed() && zones.is_some() {
        return;
    }
    let mut out = Vec::new();
    for placed in &settlements.0 {
        let bounds = placed.site.owned_bounds();
        out.push(DetailZone {
            bounds,
            lod: 0,
            reach: SETTLEMENT_FINE_REACH_M,
        });
        out.push(DetailZone {
            bounds,
            lod: 1,
            reach: SETTLEMENT_REACH_M,
        });
    }
    commands.insert_resource(DetailZones(out));
}

/// [`StreamSet::Select`]: the quadtree walk from the observers, skipped
/// while nothing it reads has changed (see the crate docs).
pub fn select_regions(
    observers: Res<Observers>,
    cfg: Res<StreamConfig>,
    zones: Option<Res<DetailZones>>,
    mut wanted: ResMut<WantedRegions>,
) {
    let zones_changed = zones
        .as_ref()
        .map_or(wanted.zoned, |z| z.is_changed() || !wanted.zoned);
    if !zones_changed && !cfg.is_changed() && wanted.observers_still(&observers.positions) {
        return;
    }
    // Written around change detection: a walk that gives the same set must
    // not tell the diff (or anyone else) that the wanted set changed.
    let inner = wanted.bypass_change_detection();
    let slice = zones.as_deref().map_or(&[][..], |z| &z.0[..]);
    let changed = inner.selector.reselect(&observers.positions, &cfg, slice);
    inner.anchors.clone_from(&observers.positions);
    inner.zoned = zones.is_some();
    if changed {
        wanted.set_changed();
    }
}

/// What [`diff_regions`] derives from the wanted and loaded sets, kept
/// between frames because at rest neither changes.
#[derive(Debug, Default)]
pub struct DiffMemory {
    /// Per ancestor of a wanted region: how many wanted descendants it has
    /// and how many of those are ready.
    cover: HashMap<RegionCoord, (u32, u32)>,
    /// Loaded regions that are not wanted: the only unload candidates.
    extra: HashSet<RegionCoord>,
    /// A wanted region may still be missing from the loaded set.
    loads_pending: bool,
    /// Something an unload depends on changed, or the last pass left
    /// candidates behind its budget.
    unloads_pending: bool,
    /// A loaded region may still be `Requested`; without an external
    /// builder those turn ready next frame.
    requested: bool,
}

impl DiffMemory {
    fn rebuild(&mut self, wanted: &WantedRegions, loaded: &LoadedRegions, root_lod: u8) {
        self.cover.clear();
        for coord in wanted.regions() {
            let ready = loaded.is_ready(*coord);
            for ancestor in cover_ancestors(*coord, root_lod) {
                let entry = self.cover.entry(ancestor).or_default();
                entry.0 += 1;
                if ready {
                    entry.1 += 1;
                }
            }
        }
        self.extra.clear();
        self.extra
            .extend(loaded.regions.keys().filter(|c| !wanted.contains(**c)));
        self.loads_pending = true;
        self.unloads_pending = true;
        self.requested = true;
    }
}

/// The ancestors a wanted region counts towards in the diff's cover map:
/// up to and including the first at or above the root LOD.
fn cover_ancestors(coord: RegionCoord, root_lod: u8) -> impl Iterator<Item = RegionCoord> {
    std::iter::successors(coord.parent(), move |p| {
        if p.lod >= root_lod { None } else { p.parent() }
    })
}

/// [`StreamSet::Diff`]: ready messages in, loads and unloads out, under the
/// budget and the no-hole rule (see the crate docs).
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass; splitting it would need a scratch resource"
)]
pub fn diff_regions(
    mut commands: Commands,
    cfg: Res<StreamConfig>,
    observers: Res<Observers>,
    wanted: Res<WantedRegions>,
    external: Option<Res<ExternalRegionBuilder>>,
    mut loaded: ResMut<LoadedRegions>,
    mut ready_in: MessageReader<RegionReady>,
    mut load_out: MessageWriter<RegionLoad>,
    mut unload_out: MessageWriter<RegionUnload>,
    mut regions: Query<&mut Region>,
    mut memory: Local<DiffMemory>,
) {
    let root_lod = cfg.root_lod();
    // Only this system writes `LoadedRegions`, and its own writes do not
    // show as changes to itself, so a change here is the resource replaced.
    // On the first run every resource reads as changed.
    if wanted.is_changed() || loaded.is_changed() || cfg.is_changed() {
        memory.rebuild(&wanted, &loaded, root_lod);
    }

    // 1. Readiness. Without an external builder every requested region is
    //    ready now; loads written below stay requested until next frame,
    //    so a headless app still sees the Requested state for one frame.
    let mut newly_ready: Vec<(RegionCoord, Entity)> = Vec::new();
    if external.is_none() && memory.requested {
        for (coord, r) in &mut loaded.regions {
            if r.state == RegionState::Requested {
                r.state = RegionState::Ready;
                newly_ready.push((*coord, r.entity));
            }
        }
        memory.requested = false;
    }
    for RegionReady(coord, entity) in ready_in.read() {
        if let Some(r) = loaded.regions.get_mut(coord)
            && r.entity == *entity
            && r.state == RegionState::Requested
        {
            r.state = RegionState::Ready;
            newly_ready.push((*coord, *entity));
        }
    }
    for (coord, entity) in newly_ready {
        if let Ok(mut region) = regions.get_mut(entity) {
            region.state = RegionState::Ready;
        }
        if wanted.contains(coord) {
            for ancestor in cover_ancestors(coord, root_lod) {
                if let Some(entry) = memory.cover.get_mut(&ancestor) {
                    entry.1 += 1;
                }
            }
        }
        memory.unloads_pending = true;
    }

    // 2. Loads, nearest first. Nothing unloads a wanted region, so once a
    //    pass finds every one loaded none goes missing until the set
    //    changes.
    if memory.loads_pending {
        memory.loads_pending = load_wanted(
            &mut commands,
            &cfg,
            &wanted,
            &mut loaded,
            &mut load_out,
            &mut memory,
        );
    }

    // 3. Unloads. A loaded region that is not wanted goes when whatever
    //    replaces it is ready: its wanted ancestor (a merge), or all of its
    //    wanted descendants (a split). No replacement at all is the horizon
    //    moving on, and it goes at once. Which regions qualify depends only
    //    on the wanted set and on readiness, so the pass runs when either
    //    changed or the last one hit its budget.
    if !memory.unloads_pending {
        return;
    }
    let mut candidates: Vec<(f64, RegionCoord)> = Vec::new();
    for coord in &memory.extra {
        let wanted_ancestor = (coord.lod + 1..=root_lod.max(coord.lod))
            .filter_map(|lod| coord.ancestor_at(lod))
            .find(|a| wanted.contains(*a));
        let replaced = match wanted_ancestor {
            Some(ancestor) => loaded.is_ready(ancestor),
            None => memory
                .cover
                .get(coord)
                .is_none_or(|(total, ready)| total == ready),
        };
        if replaced {
            candidates.push((nearest_distance(&observers.positions, *coord), *coord));
        }
    }
    memory.unloads_pending = candidates.len() > cfg.max_unloads_per_frame;
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, coord) in candidates.into_iter().take(cfg.max_unloads_per_frame) {
        memory.extra.remove(&coord);
        let Some(region) = loaded.regions.remove(&coord) else {
            continue;
        };
        commands.entity(region.entity).despawn();
        unload_out.write(RegionUnload(coord, region.entity));
    }
}

/// The load pass of [`diff_regions`]: spawns missing wanted regions, nearest
/// first, up to the budget. Returns whether any is still missing.
fn load_wanted(
    commands: &mut Commands,
    cfg: &StreamConfig,
    wanted: &WantedRegions,
    loaded: &mut LoadedRegions,
    load_out: &mut MessageWriter<RegionLoad>,
    memory: &mut DiffMemory,
) -> bool {
    let mut loads = 0;
    for coord in wanted.regions() {
        if loaded.regions.contains_key(coord) {
            continue;
        }
        if loads >= cfg.max_loads_per_frame {
            return true;
        }
        let entity = commands
            .spawn((
                Region {
                    coord: *coord,
                    state: RegionState::Requested,
                },
                WorldPosition(coord.min_corner()),
            ))
            .id();
        loaded.regions.insert(
            *coord,
            LoadedRegion {
                entity,
                state: RegionState::Requested,
            },
        );
        load_out.write(RegionLoad(*coord, entity));
        memory.requested = true;
        loads += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use bevy::ecs::message::Messages;
    use bevy::state::state::State;
    use bevy::time::TimeUpdateStrategy;
    use std::collections::{HashSet, VecDeque};
    use strata_app::{AppPlugin, PrimaryObserver};

    fn small() -> StreamConfig {
        StreamConfig {
            max_lod: 5,
            view_distance: 3_000.0,
            max_loads_per_frame: 6,
            max_unloads_per_frame: 6,
            ..StreamConfig::default()
        }
    }

    /// An app in the world with one primary observer at `at`, spawned once
    /// the world exists so the first update after this is the first frame
    /// that streams: the sim tick collects the observer, then `Update`
    /// selects and loads.
    fn app_with_observer(cfg: StreamConfig, at: WorldPos) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(cfg);
        app.add_plugins((bevy::time::TimePlugin, AppPlugin, StreamPlugin));
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.update();
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::InWorld
        );
        assert!(loaded(&app).is_empty());
        let observer = app
            .world_mut()
            .spawn((PrimaryObserver, WorldPosition(at)))
            .id();
        (app, observer)
    }

    fn move_observer(app: &mut App, observer: Entity, to: WorldPos) {
        app.world_mut()
            .get_mut::<WorldPosition>(observer)
            .unwrap()
            .0 = to;
    }

    fn loaded(app: &App) -> &LoadedRegions {
        app.world().resource::<LoadedRegions>()
    }

    fn wanted(app: &App) -> Vec<RegionCoord> {
        app.world().resource::<WantedRegions>().regions().to_vec()
    }

    fn drain<M: Message + Clone>(app: &mut App) -> Vec<M> {
        app.world_mut()
            .resource_mut::<Messages<M>>()
            .drain()
            .collect()
    }

    #[test]
    fn builds_over_the_sim() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, AppPlugin, StreamPlugin));
        app.update();
    }

    #[test]
    fn loads_respect_the_budget_and_come_nearest_first() {
        let cfg = small();
        let (mut app, _) = app_with_observer(cfg, WorldPos::from_xz(100.0, 100.0));
        app.update();
        let loads = drain::<RegionLoad>(&mut app);
        assert_eq!(loads.len(), cfg.max_loads_per_frame);
        assert_eq!(loaded(&app).len(), cfg.max_loads_per_frame);
        let observer = WorldPos::from_xz(100.0, 100.0);
        let mut last = 0.0;
        for RegionLoad(coord, entity) in &loads {
            let d = distance_to_region(observer, *coord);
            assert!(d >= last);
            last = d;
            let region = app.world().get::<Region>(*entity).unwrap();
            assert_eq!(region.coord, *coord);
            assert_eq!(region.state, RegionState::Requested);
            let pos = app.world().get::<WorldPosition>(*entity).unwrap();
            assert_eq!(pos.0, coord.min_corner());
        }
        assert_eq!(loads[0].0, RegionCoord::containing(observer, 0));
    }

    #[test]
    fn headless_regions_become_ready_by_themselves_and_settle_to_the_wanted_set() {
        let cfg = small();
        let (mut app, _) = app_with_observer(cfg, WorldPos::from_xz(100.0, 100.0));
        for _ in 0..200 {
            app.update();
        }
        let wanted: HashSet<_> = wanted(&app).into_iter().collect();
        let loaded_set: HashSet<_> = loaded(&app).iter().map(|(c, _)| c).collect();
        assert_eq!(wanted, loaded_set);
        assert_eq!(loaded(&app).pending().count(), 0);
        for (_, r) in loaded(&app).iter() {
            assert_eq!(
                app.world().get::<Region>(r.entity).unwrap().state,
                RegionState::Ready
            );
        }
    }

    #[test]
    fn unloads_respect_the_budget_and_despawn() {
        let cfg = small();
        let (mut app, observer) = app_with_observer(cfg, WorldPos::from_xz(0.0, 0.0));
        for _ in 0..200 {
            app.update();
        }
        let before = loaded(&app).len();
        assert!(before > 2 * cfg.max_unloads_per_frame);
        // Leave: everything is behind the horizon, nothing replaces it.
        move_observer(&mut app, observer, WorldPos::from_xz(200_000.0, 0.0));
        app.update(); // observers collected in PostUpdate
        drain::<RegionUnload>(&mut app);
        app.update();
        let unloads = drain::<RegionUnload>(&mut app);
        assert_eq!(unloads.len(), cfg.max_unloads_per_frame);
        for RegionUnload(coord, entity) in &unloads {
            assert!(
                app.world().get_entity(*entity).is_err(),
                "{coord:?} still exists"
            );
            assert!(loaded(&app).get(*coord).is_none());
        }
    }

    /// Stands in for the terrain crate: answers every load `delay` frames
    /// later, in order.
    #[derive(Resource, Default)]
    struct FakeBuilder {
        queue: VecDeque<(u32, RegionLoad)>,
        frame: u32,
        delay: u32,
    }

    fn fake_builder(
        mut builder: ResMut<FakeBuilder>,
        mut loads: MessageReader<RegionLoad>,
        mut unloads: MessageReader<RegionUnload>,
        mut ready: MessageWriter<RegionReady>,
    ) {
        builder.frame += 1;
        let frame = builder.frame;
        let delay = builder.delay;
        for load in loads.read() {
            builder.queue.push_back((frame + delay, *load));
        }
        let gone: HashSet<Entity> = unloads.read().map(|u| u.1).collect();
        while let Some((due, RegionLoad(coord, entity))) = builder.queue.front().copied() {
            if due > frame {
                break;
            }
            builder.queue.pop_front();
            if !gone.contains(&entity) {
                ready.write(RegionReady(coord, entity));
            }
        }
    }

    /// Whether some region in `set` contains `p`: one lookup per LOD
    /// rather than a scan, so the path test stays quick in debug builds.
    fn covered_by(set: &HashSet<RegionCoord>, p: WorldPos, root_lod: u8) -> bool {
        (0..=root_lod).any(|lod| set.contains(&RegionCoord::containing(p, lod)))
    }

    fn ready_set(app: &App) -> HashSet<RegionCoord> {
        loaded(app)
            .iter()
            .filter(|(_, r)| r.state == RegionState::Ready)
            .map(|(c, _)| c)
            .collect()
    }

    /// The centres of the wanted regions that a ready region covers. A
    /// point covered last frame that is still inside the wanted area must
    /// be covered this frame: that is the no-hole rule, stated on points so
    /// it survives the wanted node under a point changing in a split or a
    /// merge.
    fn covered_points(app: &App, root_lod: u8) -> Vec<WorldPos> {
        let ready = ready_set(app);
        wanted(app)
            .into_iter()
            .map(RegionCoord::center)
            .filter(|p| covered_by(&ready, *p, root_lod))
            .collect()
    }

    #[test]
    fn no_hole_while_splitting_and_merging_along_a_path() {
        let cfg = StreamConfig {
            max_loads_per_frame: 12,
            max_unloads_per_frame: 12,
            ..small()
        };
        let root_lod = cfg.root_lod();
        let start = WorldPos::from_xz(37.0, 41.0);
        let (mut app, observer) = app_with_observer(cfg, start);
        app.insert_resource(ExternalRegionBuilder);
        app.insert_resource(FakeBuilder {
            delay: 3,
            ..Default::default()
        });
        app.add_systems(Update, fake_builder.after(StreamSet::Diff));

        // Let the initial set settle.
        for _ in 0..300 {
            app.update();
        }
        assert_eq!(loaded(&app).pending().count(), 0);
        let mut prev = covered_points(&app, root_lod);
        assert_eq!(
            prev.len(),
            wanted(&app).len(),
            "settled set is not fully covered"
        );

        // Out 6 km and back, fast enough that nodes split and merge every
        // few frames and loads queue up behind the budget.
        let mut max_overlap = 0usize;
        let frames = 600;
        for i in 1..=frames {
            let t = f64::from(i) / f64::from(frames) * core::f64::consts::TAU;
            let r = 3_000.0 * (1.0 - t.cos());
            let pos = WorldPos::from_xz(start.x() + r, start.z() + r * 0.37);
            move_observer(&mut app, observer, pos);
            app.update();
            let wanted_now: HashSet<_> = wanted(&app).into_iter().collect();
            let ready = ready_set(&app);
            let all_loaded: HashSet<_> = loaded(&app).iter().map(|(c, _)| c).collect();
            for q in &prev {
                if covered_by(&wanted_now, *q, root_lod) {
                    assert!(
                        covered_by(&ready, *q, root_lod),
                        "hole at {q:?} on frame {i}"
                    );
                }
            }
            // The rule costs overlap: a parent and its children loaded at
            // once. The path must actually exercise that.
            for w in &wanted_now {
                let c = w.center();
                let overlap = (0..=root_lod)
                    .filter(|lod| all_loaded.contains(&RegionCoord::containing(c, *lod)))
                    .count();
                max_overlap = max_overlap.max(overlap);
            }
            prev = covered_points(&app, root_lod);
        }
        assert!(
            max_overlap >= 2,
            "the path never produced a split or merge in flight"
        );

        // Settle again: exactly the wanted set, all ready.
        for _ in 0..300 {
            app.update();
        }
        let wanted: HashSet<_> = wanted(&app).into_iter().collect();
        let loaded_set: HashSet<_> = loaded(&app).iter().map(|(c, _)| c).collect();
        assert_eq!(wanted, loaded_set);
        assert_eq!(loaded(&app).pending().count(), 0);
    }

    /// A point on the x axis where a walk half a metre further east, made
    /// with the history of a walk at the point, selects a different set,
    /// and so does one a metre further: what a skipped walk would have
    /// changed.
    fn a_threshold(cfg: &StreamConfig) -> f64 {
        let at = |x: f64| WorldPos::from_xz(x, 33.0);
        let set = |s: &Selector| s.regions().iter().copied().collect::<HashSet<_>>();
        (0..8_000)
            .map(|i| f64::from(i) * 0.5)
            .find(|&x| {
                let mut base = Selector::default();
                base.select(&[at(x)], cfg);
                let first = set(&base);
                let mut half = base.clone();
                half.select(&[at(x + 0.5)], cfg);
                let mut metre = base;
                metre.select(&[at(x + 1.0)], cfg);
                set(&half) != first && set(&metre) != first
            })
            .expect("a split threshold along 4 km")
    }

    #[test]
    fn under_a_metre_of_movement_the_wanted_set_is_left_as_it_is() {
        let cfg = small();
        let x = a_threshold(&cfg);
        let (mut app, observer) = app_with_observer(cfg, WorldPos::from_xz(x, 33.0));
        for _ in 0..5 {
            app.update();
        }
        let before = wanted(&app);
        let history = app.world().resource::<WantedRegions>().selector.clone();
        let changed_at = app.world().resource_ref::<WantedRegions>().last_changed();
        assert!(!before.is_empty());

        // Half a metre: a walk would change the set, and none is made.
        move_observer(&mut app, observer, WorldPos::from_xz(x + 0.5, 33.0));
        app.update(); // observers collected in PostUpdate
        app.update();
        assert_eq!(
            app.world().resource_ref::<WantedRegions>().last_changed(),
            changed_at,
            "the skipped frame flagged a change"
        );
        assert_eq!(wanted(&app), before, "the skipped frame changed the set");
        // Height is not movement either.
        move_observer(&mut app, observer, WorldPos::new(x + 0.5, 400.0, 33.0));
        app.update();
        app.update();
        assert_eq!(wanted(&app), before);

        // A metre from where the last walk stood: walked again, from the
        // same history, so exactly what the selector gives there.
        move_observer(&mut app, observer, WorldPos::from_xz(x + 1.0, 33.0));
        app.update();
        app.update();
        let after = wanted(&app);
        let mut expected = history;
        expected.select(&[WorldPos::from_xz(x + 1.0, 33.0)], &cfg);
        assert_eq!(after, expected.regions());
        assert_ne!(
            after.iter().collect::<HashSet<_>>(),
            before.iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn new_detail_zones_are_walked_without_movement() {
        let cfg = small();
        let at = WorldPos::from_xz(1_500.0, 150.0);
        let (mut app, _) = app_with_observer(cfg, at);
        for _ in 0..5 {
            app.update();
        }
        let zone = DetailZone {
            bounds: [0.0, 0.0, 300.0, 300.0],
            lod: 0,
            reach: 2_000.0,
        };
        assert!(wanted(&app).iter().any(|r| zone.overlaps(*r) && r.lod > 0));
        app.insert_resource(DetailZones(vec![zone]));
        app.update();
        assert!(
            wanted(&app)
                .iter()
                .filter(|r| zone.overlaps(**r))
                .all(|r| r.lod == 0)
        );
        // And their removal.
        app.world_mut().remove_resource::<DetailZones>();
        app.update();
        assert!(wanted(&app).iter().any(|r| zone.overlaps(*r) && r.lod > 0));
    }

    #[test]
    fn a_stale_ready_for_an_unloaded_entity_is_ignored() {
        let cfg = small();
        let (mut app, observer) = app_with_observer(cfg, WorldPos::from_xz(0.0, 0.0));
        app.insert_resource(ExternalRegionBuilder);
        app.update();
        let loads = drain::<RegionLoad>(&mut app);
        let RegionLoad(coord, entity) = loads[0];
        move_observer(&mut app, observer, WorldPos::from_xz(200_000.0, 0.0));
        app.update();
        app.update();
        assert!(loaded(&app).get(coord).is_none());
        app.world_mut()
            .resource_mut::<Messages<RegionReady>>()
            .write(RegionReady(coord, entity));
        app.update();
        assert!(loaded(&app).get(coord).is_none());
    }

    #[test]
    fn a_split_keeps_the_parent_until_all_children_are_ready() {
        let cfg = StreamConfig {
            max_loads_per_frame: 1_000,
            max_unloads_per_frame: 1_000,
            ..small()
        };
        // Sit far enough east of a lod-2 node that it is selected whole.
        let node = RegionCoord::new(8, 0, 2);
        let split_at = cfg.split_factor * node.size();
        let east = node.min_corner().x() + node.size();
        let (mut app, observer) =
            app_with_observer(cfg, WorldPos::from_xz(east + split_at + 50.0, 100.0));
        app.insert_resource(ExternalRegionBuilder);
        app.insert_resource(FakeBuilder {
            delay: 1,
            ..Default::default()
        });
        app.add_systems(Update, fake_builder.after(StreamSet::Diff));
        for _ in 0..20 {
            app.update();
        }
        assert!(wanted(&app).contains(&node), "{:?}", wanted(&app));
        assert!(loaded(&app).is_ready(node));

        // Step inside the split distance: the children become wanted, the
        // parent stays until they are all ready.
        move_observer(
            &mut app,
            observer,
            WorldPos::from_xz(east + split_at - 50.0, 100.0),
        );
        app.update();
        app.update();
        let children = node.children().unwrap();
        assert!(children.iter().all(|c| wanted(&app).contains(c)));
        assert!(!wanted(&app).contains(&node));
        assert!(
            loaded(&app).is_ready(node),
            "parent unloaded before the children were ready"
        );
        assert!(children.iter().all(|c| loaded(&app).get(*c).is_some()));
        for _ in 0..5 {
            app.update();
        }
        assert!(children.iter().all(|c| loaded(&app).is_ready(*c)));
        assert!(
            loaded(&app).get(node).is_none(),
            "parent lingered after the split completed"
        );

        // And back out: the parent is re-requested, the children stay until
        // it is ready.
        move_observer(
            &mut app,
            observer,
            WorldPos::from_xz(east + split_at * 2.0, 100.0),
        );
        app.update();
        app.update();
        assert!(wanted(&app).contains(&node));
        assert!(children.iter().all(|c| loaded(&app).is_ready(*c)));
        for _ in 0..5 {
            app.update();
        }
        assert!(loaded(&app).is_ready(node));
        assert!(children.iter().all(|c| loaded(&app).get(*c).is_none()));
    }
}
