//! Cell identity and work budgets are independent of terrain quadtree nodes.
//!
//! Nothing here walks every cell every frame. The ring of wanted cells is
//! chosen again only when the eye has moved [`RESELECT_M`], crossed a cell
//! edge or turned; the collector looks only at cells with work: those
//! that just arrived or changed, those with far trees or grass still to
//! install, and those whose plants' slack the camera has used up, which a
//! queue ordered on the camera's odometer hands back when it has. The
//! statistics are running totals kept as cells change.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use strata_core::platform::time::Instant;

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, Visibility, VisibilityRange};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Mesh3d, MeshTag};
use bevy::pbr::MeshMaterial3d;

use crate::impostor::{FarTreeMaterial, FarTreeStore, RECORD_BYTES};
use crate::tree::{MESH_LEVELS, TreeImpostorBatch};
use bevy::camera::{Camera, Camera3d};
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::components::GlobalTransform;
use strata_app::RenderOrigin;
use strata_app::{Observers, WorldPosition, WorldSeed, WorldSourceRes};
use strata_core::{Seed, WorldPos};
use strata_flora::{CandidateCache, place_cell_cached, place_cell_trees};
use strata_stream::{LoadedRegions, distance_to_region};

use super::{
    FoliageConfig, FoliageStats, GrassAssets, GrassMaterial, PlaceInputs, TreeAssets, impostor,
    spawn_grass, tree,
};
use bevy::asset::Assets;
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::time::Time;
use bevy::transform::components::Transform;
use strata_core::RegionCoord;
use strata_flora::CellFlora;

/// How far the eye moves before the ring is chosen again, metres. The ring
/// reaches the preparation radius and is kept to the retention band past
/// it, so a ring this stale wants a cell at most this late, against 128 m
/// of prefetch.
const RESELECT_M: f64 = 8.0;

/// The cosine of the turn in the eye's heading that chooses the ring again:
/// the ring is ranked with a bias ahead, which a turn of 30 degrees moves.
const RESELECT_TURN: f64 = 0.866;

/// The share of the time budget the previous frame's apply may take from
/// this one. The measured apply also holds the wait for the exclusive sync
/// point and the other systems' commands applied there, so all of it would
/// starve the collector on a frame that was busy for other reasons.
const APPLY_CARRY: f32 = 0.5;

/// Plants a cell's walk evaluates between looks at the clock: a dense cell
/// holds hundreds, and one look costs as much as a few plants.
const PLANTS_PER_CLOCK: usize = 8;

#[derive(Clone)]
struct Placement {
    flora: Arc<CellFlora>,
    with_grass: bool,
    ms: f32,
}

impl Placement {
    fn bytes(&self) -> usize {
        self.flora.grass.capacity() * size_of::<strata_flora::GrassBlade>()
            + self.flora.plants.capacity() * size_of::<strata_flora::Plant>()
    }
}

/// The retiring mask's bits: a cell's plant levels, its far-tree batches
/// and its grass pair. Only the grass retires on its own; an evicted cell
/// retires everything.
const MESH_BIT: u8 = 1;
const FAR_BIT: u8 = 1 << 1;
const GRASS_BIT: u8 = 1 << 2;

/// One mesh level a plant holds: its bark and leaf entities, and when they
/// were made, for the least dwell.
struct HeldLevel {
    entities: Vec<Entity>,
    since: f32,
}

/// One plant's mesh levels. Each is made when the camera nears its band
/// and dropped once it has left it, so a plant holds the one level it
/// draws and, through a crossfade, the next one too, instead of all three
/// with `VisibilityRange` choosing: Bevy pays for every entity, seen or not.
#[derive(Default)]
struct PlantLevels {
    /// Where each level is drawn at all, from the tree assets, kept so the
    /// per-frame choice does not look the species up again.
    bands: [Option<(f32, f32)>; MESH_LEVELS],
    held: [Option<HeldLevel>; MESH_LEVELS],
}

impl PlantLevels {
    fn mask(&self) -> u8 {
        bits(self.held.iter().map(Option::is_some))
    }

    /// The held levels past their least dwell, which may be dropped.
    fn settled(&self, now: f32, dwell: f32) -> u8 {
        bits(
            self.held
                .iter()
                .map(|held| held.as_ref().is_some_and(|h| now - h.since >= dwell)),
        )
    }
}

fn bits(flags: impl Iterator<Item = bool>) -> u8 {
    flags
        .enumerate()
        .fold(0, |mask, (level, set)| mask | (u8::from(set) << level))
}

struct Resident {
    root: Entity,
    placement: Placement,
    // By placement index; empty until the camera first comes near enough
    // for any of the cell's plants to want a level.
    plants: Vec<PlantLevels>,
    // The mesh level entities `plants` holds.
    meshes: u64,
    // The far batches go in at once, all or nothing, and stay while the
    // cell does, below every plant's mesh levels.
    far_installed: bool,
    far_entities: Vec<Entity>,
    grass_spawned: bool,
    grass_entities: Vec<Entity>,
    far_trees: u64,
    // Set when every plant holds what it wants, or, for a cell beyond any
    // level's reach, when it holds none: until the camera has moved the
    // slack, looking at the plants again would change nothing.
    checked: Option<Checked>,
    // The stamp of the cell's latest entry in `CellStream::expiries`; an
    // entry with another is stale.
    stamp: u64,
}

struct Retiring {
    root: Option<Entity>,
    children: Vec<Entity>,
    next: usize,
    grass_bytes: usize,
    cell: RegionCoord,
    // `MESH_BIT`, `FAR_BIT` and `GRASS_BIT`, all of them for an evicted cell.
    details: u8,
}

/// Placements of cells that left the resident ring, oldest evicted first.
/// Looked up for every wanted cell the scheduler may dispatch, so by key; the
/// order queue may hold keys already taken, which eviction skips.
#[derive(Default)]
struct Cache {
    entries: HashMap<RegionCoord, (u64, Placement)>,
    order: VecDeque<(u64, RegionCoord)>,
    next: u64,
    bytes: usize,
}

impl Cache {
    fn take(&mut self, coord: RegionCoord) -> Option<Placement> {
        let (_, result) = self.entries.remove(&coord)?;
        self.bytes -= result.bytes();
        Some(result)
    }

    fn insert(&mut self, coord: RegionCoord, value: Placement, cfg: &FoliageConfig) {
        self.take(coord);
        self.bytes += value.bytes();
        self.next += 1;
        self.entries.insert(coord, (self.next, value));
        self.order.push_back((self.next, coord));
        while self.bytes > cfg.cache_bytes || self.entries.len() > cfg.cache_cells {
            let Some((stamp, old)) = self.order.pop_front() else {
                break;
            };
            if self.entries.get(&old).is_some_and(|(s, _)| *s == stamp) {
                self.take(old);
            }
        }
        // Keys taken since they were queued would otherwise pile up.
        if self.order.len() > 4 * cfg.cache_cells.max(1) {
            let entries = &self.entries;
            self.order
                .retain(|(stamp, c)| entries.get(c).is_some_and(|(s, _)| s == stamp));
        }
    }
}

#[derive(Clone, Copy)]
struct Wanted {
    coord: RegionCoord,
    grass: bool,
    priority: f64,
}

/// Where the ring was last chosen from, and its radii. Whether a cell is
/// wanted, whether it wants grass and its rank follow from these alone, so
/// a new ring, which a flight chooses every frame, builds no index.
#[derive(Clone, Copy, Debug)]
struct Selection {
    eye: WorldPos,
    centre: RegionCoord,
    heading: (f64, f64),
    /// Cells within this are wanted; below zero when foliage is off.
    radius: f64,
    /// And within this, with their grass.
    grass_radius: f64,
    /// How far the ranking leans ahead.
    prefetch: f64,
}

impl Selection {
    fn new(eye: WorldPos, heading: (f64, f64), cfg: &FoliageConfig) -> Self {
        Self {
            eye,
            centre: RegionCoord::containing(eye, 0),
            heading,
            radius: if cfg.enabled {
                preparation_radius(cfg)
            } else {
                -1.0
            },
            grass_radius: f64::from(cfg.far_fade.1 + cfg.prefetch_m),
            prefetch: f64::from(cfg.prefetch_m),
        }
    }

    /// The cell as this ring wants it, `None` when it does not. A lower
    /// priority comes first: nearer, and ahead of the heading.
    fn want(&self, coord: RegionCoord) -> Option<Wanted> {
        let distance = distance_to_region(self.eye, coord);
        if distance > self.radius {
            return None;
        }
        let c = coord.center();
        let ahead = ((c.x() - self.eye.x()) * self.heading.0
            + (c.z() - self.eye.z()) * self.heading.1)
            / (distance + 64.0);
        Some(Wanted {
            coord,
            grass: distance <= self.grass_radius,
            priority: distance - ahead * self.prefetch * 0.5,
        })
    }
}

/// When a cell's plants may next want other levels: once the camera's
/// odometer has reached `at`.
#[derive(Clone, Copy, Debug)]
struct Expiry {
    at: f64,
    coord: RegionCoord,
    stamp: u64,
}

impl PartialEq for Expiry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Expiry {}

impl PartialOrd for Expiry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Expiry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.at
            .total_cmp(&other.at)
            .then(self.coord.cmp(&other.coord))
            .then(self.stamp.cmp(&other.stamp))
    }
}

/// What one resident cell adds to the totals [`FoliageStats`] reports. Every
/// change to a resident is bracketed by its tally before and after, so the
/// totals are kept without summing over every cell each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    cells: usize,
    blades: u64,
    trees: u64,
    far_trees: u64,
    entities: u64,
    tree_meshes: u64,
    cpu_bytes: usize,
    grass_bytes: usize,
}

impl Tally {
    fn of(resident: &Resident) -> Self {
        let grass = resident.grass_spawned;
        Self {
            cells: usize::from(grass),
            blades: if grass {
                resident.placement.flora.grass.len() as u64
            } else {
                0
            },
            trees: resident.placement.flora.plants.len() as u64,
            far_trees: resident.far_trees,
            entities: resident.entities(),
            tree_meshes: resident.meshes,
            cpu_bytes: resident.placement.bytes(),
            grass_bytes: resident.grass_bytes(),
        }
    }

    /// Takes `before` out and puts `after` in.
    fn change(&mut self, before: Self, after: Self) {
        self.cells = self.cells - before.cells + after.cells;
        self.blades = self.blades - before.blades + after.blades;
        self.trees = self.trees - before.trees + after.trees;
        self.far_trees = self.far_trees - before.far_trees + after.far_trees;
        self.entities = self.entities - before.entities + after.entities;
        self.tree_meshes = self.tree_meshes - before.tree_meshes + after.tree_meshes;
        self.cpu_bytes = self.cpu_bytes - before.cpu_bytes + after.cpu_bytes;
        self.grass_bytes = self.grass_bytes - before.grass_bytes + after.grass_bytes;
    }

    /// The totals summed afresh, which the running ones must equal.
    fn sum<'a>(residents: impl Iterator<Item = &'a Resident>) -> Self {
        let mut total = Self::default();
        for resident in residents {
            total.change(Self::default(), Self::of(resident));
        }
        total
    }
}

/// Runtime state for one world/registry epoch. Evicted CPU records are bounded
/// by both count and bytes; GPU allocations live only with resident entities.
#[derive(Resource, Default)]
pub(super) struct CellStream {
    residents: HashMap<RegionCoord, Resident>,
    tasks: HashMap<RegionCoord, Task<Placement>>,
    ready: HashMap<RegionCoord, Placement>,
    // The wanted cells not yet resident as wanted, nearest first, which
    // dispatch walks.
    unsatisfied: Vec<Wanted>,
    selection: Option<Selection>,
    // Bumped by every new ring; the collector sweeps for retirement when it
    // has moved since the last sweep.
    generation: u64,
    swept: u64,
    retiring: VecDeque<Retiring>,
    // The grass bytes `retiring` holds.
    retiring_grass: usize,
    cache: Cache,
    // Every placed cell's candidates before thinning, which the halo of the
    // cells round it reads instead of sampling the strip again.
    candidates: Arc<CandidateCache>,
    previous_eye: Option<WorldPos>,
    // The camera at the last collection, for its speed.
    previous_view: Option<WorldPos>,
    epoch: Option<Seed>,
    reset: bool,
    apply_started: Option<Instant>,
    // The last apply, milliseconds, counted against the next collection.
    last_apply_ms: f32,
    // Residents whose plants are looked at the next time the levels are
    // walked: new or changed cells, those left short by a budget, and those
    // whose expiry has come.
    dirty: HashSet<RegionCoord>,
    // Wanted residents with far trees or grass still to install.
    installs: HashSet<RegionCoord>,
    expiries: BinaryHeap<Reverse<Expiry>>,
    // The camera's path plus how far the lead has moved, metres. It never
    // grows less than `Sight::moved_since` measures from any check, so a
    // cell handed back when it reaches the check's slack is never late.
    odometer: f64,
    last_sight: Option<(WorldPos, f32)>,
    next_stamp: u64,
    totals: Tally,
}

impl CellStream {
    fn retiring_details(&self) -> HashMap<RegionCoord, u8> {
        let mut pending = HashMap::new();
        for retiring in &self.retiring {
            *pending.entry(retiring.cell).or_insert(0) |= retiring.details;
        }
        pending
    }

    /// Takes `wanted`, nearest first, as the ring chosen from `selection`.
    /// Dispatch drops the cells already resident as it walks them: a flight
    /// chooses a ring every frame and starts its few jobs near the front.
    fn select(&mut self, wanted: Vec<Wanted>, selection: Selection) {
        self.unsatisfied = wanted;
        self.selection = Some(selection);
        self.generation += 1;
    }

    /// No ring: nothing is wanted until the next selection.
    fn forget_selection(&mut self) {
        self.unsatisfied.clear();
        self.selection = None;
        self.generation += 1;
    }

    /// The cell as the current ring wants it, if it does.
    fn want(&self, coord: RegionCoord) -> Option<Wanted> {
        self.selection.and_then(|selection| selection.want(coord))
    }

    /// A cell's place in the ring, past every wanted cell when unwanted.
    fn rank(&self, coord: RegionCoord) -> f64 {
        self.want(coord).map_or(f64::INFINITY, |want| want.priority)
    }

    /// A finished placement: in place of a resident's, whose plants are then
    /// looked at again, or into the cache.
    fn settle(&mut self, coord: RegionCoord, placement: Placement, cfg: &FoliageConfig) {
        let want = self.want(coord);
        let Some(resident) = self.residents.get_mut(&coord) else {
            self.cache.insert(coord, placement, cfg);
            return;
        };
        let before = Tally::of(resident);
        resident.placement = placement;
        self.totals.change(before, Tally::of(resident));
        self.dirty.insert(coord);
        if let Some(want) = want {
            if needs_install(resident, want.grass) {
                self.installs.insert(coord);
            }
            // A placement without grass that a grass cell must replace again.
            if want.grass && !resident.placement.with_grass {
                self.unsatisfied.push(Wanted {
                    coord,
                    grass: true,
                    priority: 0.0,
                });
            }
        }
    }

    /// A new resident cell for `placement`.
    fn admit(&mut self, commands: &mut Commands, coord: RegionCoord, placement: Placement) {
        let root = commands
            .spawn((
                WorldPosition(coord.min_corner()),
                Transform::default(),
                Visibility::default(),
                Name::new("foliage cell"),
            ))
            .id();
        let resident = Resident {
            root,
            placement,
            plants: Vec::new(),
            meshes: 0,
            far_installed: false,
            far_entities: Vec::new(),
            grass_spawned: false,
            grass_entities: Vec::new(),
            far_trees: 0,
            checked: None,
            stamp: 0,
        };
        self.totals.change(Tally::default(), Tally::of(&resident));
        self.dirty.insert(coord);
        if self
            .want(coord)
            .is_some_and(|want| needs_install(&resident, want.grass))
        {
            self.installs.insert(coord);
        }
        self.residents.insert(coord, resident);
    }

    /// The wanted residents with anything left to install, after the ring
    /// changed which want grass.
    fn rebuild_installs(&mut self) {
        self.installs.clear();
        let Some(selection) = self.selection else {
            return;
        };
        for (coord, resident) in &self.residents {
            if selection
                .want(*coord)
                .is_some_and(|want| needs_install(resident, want.grass))
            {
                self.installs.insert(*coord);
            }
        }
    }

    /// Moves the odometer with the camera and marks the cells it has come
    /// to. With more than one camera no slack holds, and every cell is
    /// looked at, as the levels are chosen for the nearest of them.
    fn advance(&mut self, sight: &Sight) {
        let [view] = sight.views.as_slice() else {
            self.last_sight = None;
            self.dirty.extend(self.residents.keys().copied());
            return;
        };
        if let Some((last, ahead)) = self.last_sight {
            self.odometer += (*view - last).length() + f64::from((sight.lead.ahead - ahead).abs());
        }
        self.last_sight = Some((*view, sight.lead.ahead));
        while let Some(Reverse(next)) = self.expiries.peek()
            && next.at <= self.odometer
        {
            let Some(Reverse(due)) = self.expiries.pop() else {
                break;
            };
            if self
                .residents
                .get(&due.coord)
                .is_some_and(|resident| resident.stamp == due.stamp)
            {
                self.dirty.insert(due.coord);
            }
        }
        // Entries of evicted or re-checked cells wait for their time; past a
        // few per cell they are dropped at once.
        if self.expiries.len() > 4 * self.residents.len() + 64 {
            let residents = &self.residents;
            self.expiries.retain(|Reverse(entry)| {
                residents
                    .get(&entry.coord)
                    .is_some_and(|resident| resident.stamp == entry.stamp)
            });
        }
    }

    /// After a cell's plants were looked at: one that holds what it wants
    /// is handed back when the camera has used up its slack.
    fn reschedule(&mut self, coord: RegionCoord, sight: &Sight) {
        let Some(resident) = self.residents.get_mut(&coord) else {
            self.dirty.remove(&coord);
            return;
        };
        let Some(check) = resident.checked else {
            return;
        };
        self.dirty.remove(&coord);
        let left = check.slack - sight.moved_since(&check);
        if !left.is_finite() {
            // No plant has a band: nothing but a new placement changes it.
            return;
        }
        self.next_stamp += 1;
        resident.stamp = self.next_stamp;
        self.expiries.push(Reverse(Expiry {
            at: self.odometer + f64::from(left.max(0.0)),
            coord,
            stamp: resident.stamp,
        }));
    }

    /// Looks at one dirty cell's plants and makes or drops their levels,
    /// then hands the cell back to the queue if it holds what it wants. An
    /// unwanted cell, or one whose old levels are still being retired, only
    /// drops.
    #[allow(
        clippy::too_many_arguments,
        reason = "one cell's levels against the frame's shared budgets"
    )]
    fn look(
        &mut self,
        commands: &mut Commands,
        coord: RegionCoord,
        trees: &TreeAssets,
        sight: &Sight,
        cfg: &FoliageConfig,
        budget: &mut Budget,
        removals: &mut usize,
        retiring_mask: u8,
    ) {
        let wanted = self.want(coord).is_some();
        let Some(resident) = self.residents.get_mut(&coord) else {
            self.dirty.remove(&coord);
            return;
        };
        let before = Tally::of(resident);
        update_plant_levels(
            commands,
            resident,
            coord,
            trees,
            sight,
            cfg,
            budget,
            removals,
            wanted && retiring_mask & MESH_BIT == 0,
        );
        self.totals.change(before, Tally::of(resident));
        self.reschedule(coord, sight);
    }

    /// Installs what one wanted cell still lacks of its far trees and grass.
    fn install(
        &mut self,
        commands: &mut Commands,
        coord: RegionCoord,
        install: &mut Install<'_>,
        retiring_mask: u8,
        budget: &mut Budget,
    ) {
        let grass = self.want(coord).is_some_and(|want| want.grass);
        let Some(resident) = self.residents.get_mut(&coord) else {
            self.installs.remove(&coord);
            return;
        };
        let before = Tally::of(resident);
        install.cell(commands, resident, grass, retiring_mask, budget);
        self.totals.change(before, Tally::of(resident));
        if !needs_install(resident, grass) {
            self.installs.remove(&coord);
        }
    }
}

/// Whether a wanted cell is resident as it is wanted.
fn satisfied(residents: &HashMap<RegionCoord, Resident>, want: &Wanted) -> bool {
    residents
        .get(&want.coord)
        .is_some_and(|resident| !want.grass || resident.placement.with_grass)
}

/// Whether the cell's grass is wanted, placed and not yet installed.
fn grass_pending(resident: &Resident, grass: bool) -> bool {
    grass
        && !resident.grass_spawned
        && resident.placement.with_grass
        && !resident.placement.flora.grass.is_empty()
}

fn needs_install(resident: &Resident, grass: bool) -> bool {
    !resident.far_installed || grass_pending(resident, grass)
}

fn preparation_radius(cfg: &FoliageConfig) -> f64 {
    f64::from(cfg.tree_fades.far.1.max(cfg.far_fade.1) + cfg.prefetch_m)
}

fn retained(coord: RegionCoord, eye: WorldPos, cfg: &FoliageConfig) -> bool {
    cfg.enabled
        && distance_to_region(eye, coord) <= preparation_radius(cfg) + f64::from(cfg.retention_m)
}

/// Which way the eye moved over the last frame, a unit vector in x and z,
/// or zero when it stood.
fn heading(eye: WorldPos, previous: Option<WorldPos>) -> (f64, f64) {
    let (dx, dz) = previous.map_or((0.0, 0.0), |p| (eye.x() - p.x(), eye.z() - p.z()));
    let length = dx.hypot(dz);
    if length > 0.01 {
        (dx / length, dz / length)
    } else {
        (0.0, 0.0)
    }
}

/// Whether the ring chosen from `selection` is stale for an eye at `eye`
/// heading `heading`. Stopping keeps the last heading's ranking.
fn reselect(selection: Option<&Selection>, eye: WorldPos, heading: (f64, f64)) -> bool {
    let Some(selection) = selection else {
        return true;
    };
    if RegionCoord::containing(eye, 0) != selection.centre {
        return true;
    }
    let (dx, dz) = (eye.x() - selection.eye.x(), eye.z() - selection.eye.z());
    if dx.hypot(dz) >= RESELECT_M {
        return true;
    }
    heading != (0.0, 0.0)
        && heading.0 * selection.heading.0 + heading.1 * selection.heading.1 < RESELECT_TURN
}

/// Horizontal selection keeps high terrain resident too. Actual render LODs
/// use the tree roots and the grass bounds, both at their real world height.
#[allow(
    clippy::cast_possible_truncation,
    reason = "bounded cell radius and world grid coordinates"
)]
fn select_cells(eye: WorldPos, direction: (f64, f64), cfg: &FoliageConfig) -> Vec<Wanted> {
    if !cfg.enabled {
        return Vec::new();
    }
    let selection = Selection::new(eye, direction, cfg);
    let n = (selection.radius / 64.0).ceil() as i32 + 1;
    let centre = selection.centre;
    let mut wanted = Vec::new();
    for z in centre.z.saturating_sub(n)..=centre.z.saturating_add(n) {
        for x in centre.x.saturating_sub(n)..=centre.x.saturating_add(n) {
            wanted.extend(selection.want(RegionCoord::new(x, z, 0)));
        }
    }
    wanted.sort_unstable_by(|a, b| {
        a.priority
            .total_cmp(&b.priority)
            .then(a.coord.cmp(&b.coord))
    });
    wanted
}

impl Resident {
    fn entities(&self) -> u64 {
        1 + self.meshes + self.far_entities.len() as u64 + self.grass_entities.len() as u64
    }

    /// Every entity the cell's plants hold, emptying them.
    fn take_meshes(&mut self) -> Vec<Entity> {
        self.meshes = 0;
        self.plants
            .drain(..)
            .flat_map(|plant| plant.held.into_iter().flatten())
            .flat_map(|held| held.entities)
            .collect()
    }

    fn grass_bytes(&self) -> usize {
        if self.grass_spawned {
            self.placement.flora.grass.len() * size_of::<strata_flora::GrassBlade>()
        } else {
            0
        }
    }
}

// Dropping grass does not discard the stable cell root or far trees. Its
// range already hides it; destruction shares the cell budget. If the eye
// returns before deletion finishes, installation waits for the old pair to
// drain, so its still-live entities can never overlap a replacement. Plants'
// mesh levels are not retired here: they come and go one plant at a time.
fn retire_detail(stream: &mut CellStream, cfg: &FoliageConfig, eye: WorldPos) {
    let reach = f64::from(cfg.far_fade.1 + cfg.prefetch_m + cfg.retention_m);
    for (coord, resident) in &mut stream.residents {
        if distance_to_region(eye, *coord) <= reach {
            continue;
        }
        if resident.grass_entities.is_empty()
            && !resident.grass_spawned
            && !resident.placement.with_grass
        {
            continue;
        }
        let before = Tally::of(resident);
        let grass_bytes = resident.grass_bytes();
        let details = if resident.grass_entities.is_empty() {
            0
        } else {
            GRASS_BIT
        };
        let children = std::mem::take(&mut resident.grass_entities);
        resident.grass_spawned = false;
        // Avoid a copy of the large blade allocation while a task result
        // is shared; only the plant records survive the detail downgrade.
        if resident.placement.with_grass {
            if let Some(flora) = Arc::get_mut(&mut resident.placement.flora) {
                flora.grass = Vec::new();
            } else {
                resident.placement.flora = Arc::new(CellFlora {
                    coord: *coord,
                    plants: resident.placement.flora.plants.clone(),
                    grass: Vec::new(),
                });
            }
            resident.placement.with_grass = false;
        }
        stream.totals.change(before, Tally::of(resident));
        if !children.is_empty() {
            stream.retiring_grass += grass_bytes;
            stream.retiring.push_back(Retiring {
                root: None,
                children,
                next: 0,
                grass_bytes,
                cell: *coord,
                details,
            });
        }
    }
}

fn ground_ready(mut coord: RegionCoord, loaded: &LoadedRegions) -> bool {
    loop {
        if loaded.is_ready(coord) {
            return true;
        }
        let Some(parent) = coord.parent() else {
            return false;
        };
        coord = parent;
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "read-only placement inputs and scheduler state, and the walk in one piece"
)]
pub(super) fn dispatch(
    cfg: Res<FoliageConfig>,
    source: Option<Res<WorldSourceRes>>,
    procedural: Option<Res<strata_app::ProceduralWorldRes>>,
    inputs: Option<Res<PlaceInputs>>,
    exclusions: Option<Res<strata_app::footprints::VegetationExclusions>>,
    planted: Option<Res<strata_app::footprints::PlantedFlora>>,
    seed: Res<WorldSeed>,
    observers: Res<Observers>,
    loaded: Res<LoadedRegions>,
    mut stream: ResMut<CellStream>,
    mut stats: ResMut<FoliageStats>,
) {
    let Some(eye) = observers.primary else {
        if stream.selection.is_some() {
            stream.forget_selection();
        }
        return;
    };
    if stream.epoch != Some(seed.0) {
        stream.reset = stream.epoch.is_some();
        stream.epoch = Some(seed.0);
        stream.tasks.clear();
        stream.ready.clear();
        stream.cache = Cache::default();
        stream.candidates.clear();
    }
    if stream.reset {
        return;
    }
    let stream = &mut *stream;
    stream.tasks.retain(|coord, _| retained(*coord, eye, &cfg));
    stream.ready.retain(|coord, _| retained(*coord, eye, &cfg));
    let heading = heading(eye, stream.previous_eye);
    stream.previous_eye = Some(eye);
    if cfg.is_changed() || reselect(stream.selection.as_ref(), eye, heading) {
        let wanted = select_cells(eye, heading, &cfg);
        stream.select(wanted, Selection::new(eye, heading, &cfg));
    }
    let (Some(source), Some(inputs)) = (source, inputs) else {
        return;
    };
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
    let CellStream {
        residents,
        tasks,
        ready,
        cache,
        unsatisfied,
        candidates,
        ..
    } = stream;
    // The cells not yet resident as wanted, nearest first; the walk stops
    // as soon as no more can start a job. One that walked the whole list
    // drops what is resident by now, so a settled ring walks nothing.
    let mut dispatched = 0;
    let mut walked_all = true;
    let mut resident = 0;
    for want in unsatisfied.iter() {
        if tasks.len() + ready.len() >= cfg.max_in_flight || dispatched >= cfg.dispatch_per_frame {
            walked_all = false;
            break;
        }
        if satisfied(residents, want) {
            resident += 1;
            continue;
        }
        if tasks.contains_key(&want.coord) || ready.contains_key(&want.coord) {
            continue;
        }
        if !residents.contains_key(&want.coord)
            && let Some(cached) = cache.take(want.coord)
        {
            ready.insert(want.coord, cached);
            stats.cache_hits += 1;
            continue;
        }
        if !ground_ready(want.coord, &loaded) {
            continue;
        }
        let source = source.0.clone();
        let procedural = procedural.as_deref().map(|world| Arc::clone(&world.0));
        let table = inputs.table.clone();
        let seed = seed.0;
        let coord = want.coord;
        let with_grass = want.grass;
        let candidates = Arc::clone(candidates);
        let exclusions = exclusions.as_deref().cloned().unwrap_or_default();
        let planted = planted
            .as_deref()
            .map(strata_app::footprints::PlantedFlora::plants);
        let task = pool.spawn(async move {
            let started = Instant::now();
            // A cell wants grass out to the far fade, well past where any
            // understory, rock or bush mesh is drawn, so the whole placement is
            // for those and a cell beyond is placed for its impostors alone: it
            // exists only for its impostors, and is upgraded when it comes
            // in (`satisfied` sees no grass).
            let mut flora = if with_grass {
                place_cell_cached(
                    coord,
                    source.as_ref(),
                    &table,
                    seed,
                    &mut Vec::new(),
                    true,
                    &candidates,
                )
            } else {
                place_cell_trees(coord, source.as_ref(), &table, seed, &candidates)
            };
            exclude_occupied(&mut flora, &exclusions);
            if let Some(world) = &procedural {
                clear_crossings(&mut flora, &table, world);
            }
            // A settlement's own trees, after the exclusions: they stand
            // where the plan put them, among its buildings.
            if let Some(planted) = &planted {
                flora
                    .plants
                    .extend(strata_flora::planted_in(coord, &table, planted));
            }
            Placement {
                flora: Arc::new(flora),
                with_grass,
                ms: started.elapsed().as_secs_f32() * 1000.0,
            }
        });
        tasks.insert(coord, task);
        dispatched += 1;
    }
    if walked_all && resident > 0 {
        unsatisfied.retain(|want| !satisfied(residents, want));
    }
    stats.in_flight = tasks.len() + ready.len();
}

/// How far ahead of a band a plant's level is made, and how much farther
/// past it the level is kept, metres.
#[derive(Clone, Copy, Debug)]
struct Lead {
    ahead: f32,
    margin: f32,
}

/// The mesh levels a plant `distance` metres from the camera holds next, as
/// a mask over its `bands` (where each level is drawn at all). A level is
/// wanted while its band, widened by `lead.ahead`, holds the distance, so it
/// exists before it starts fading in. One already `held` stays until the
/// distance is `lead.margin` farther out of that band still and the level
/// is `settled`, past its least dwell: a camera on a band's edge neither
/// makes nor drops it every frame. Outside its band a level draws nothing,
/// so dropping it there is never seen; inside, it is always wanted.
fn plant_levels(
    distance: f32,
    bands: &[Option<(f32, f32)>; MESH_LEVELS],
    held: u8,
    settled: u8,
    lead: Lead,
) -> u8 {
    let mut mask = 0;
    for (level, band) in bands.iter().enumerate() {
        let Some((start, end)) = *band else {
            continue;
        };
        let bit = 1 << level;
        let within = |widen: f32| distance >= start - widen && distance < end + widen;
        let kept = held & bit != 0 && (settled & bit == 0 || within(lead.ahead + lead.margin));
        if within(lead.ahead) || kept {
            mask |= bit;
        }
    }
    mask
}

/// How far `distance` is from the nearest edge where [`plant_levels`]
/// would answer otherwise for a plant holding `held`: the camera can move
/// that far before the plant needs looking at again. Zero for a level kept
/// only by its dwell, which time alone will drop.
fn plant_slack(
    distance: f32,
    bands: &[Option<(f32, f32)>; MESH_LEVELS],
    held: u8,
    lead: Lead,
) -> f32 {
    let mut slack = f32::INFINITY;
    for (level, band) in bands.iter().enumerate() {
        let Some((start, end)) = *band else {
            continue;
        };
        let mut edges = |widen: f32| {
            slack = slack
                .min((distance - (start - widen)).abs())
                .min((distance - (end + widen)).abs());
        };
        edges(lead.ahead);
        if held & (1 << level) != 0 {
            let keep = lead.ahead + lead.margin;
            if distance < start - keep || distance >= end + keep {
                return 0.0;
            }
            edges(keep);
        }
    }
    slack
}

/// A cell whose plants all hold what they want, and how far the camera
/// may move from where it was before any of them can want otherwise.
#[derive(Clone, Copy, Debug)]
struct Checked {
    view: WorldPos,
    ahead: f32,
    slack: f32,
}

/// Where the cameras are for choosing plants' levels, and how far ahead.
struct Sight {
    views: Vec<WorldPos>,
    lead: Lead,
    now: f32,
    dwell: f32,
    /// A band is held to here at most: a level that never ends would
    /// otherwise live as long as its cell.
    cap: f32,
    /// The farthest any level is drawn, within `cap`.
    farthest: f32,
}

impl Sight {
    /// From the cameras, `previous` (the first camera a frame ago) for
    /// their speed, and the configuration's lead, margin and dwell.
    fn new(
        views: Vec<WorldPos>,
        previous: Option<WorldPos>,
        delta: f32,
        now: f32,
        trees: &TreeAssets,
        cfg: &FoliageConfig,
    ) -> Self {
        // A teleport is not a speed: past a fast flight's the lead stops
        // growing, and the levels it makes are dropped after their dwell.
        const FASTEST: f32 = 400.0;
        let speed = match (views.first(), previous) {
            (Some(view), Some(previous)) if delta > 0.0 => {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a frame's movement, metres"
                )]
                let moved = (*view - previous).length() as f32;
                (moved / delta).min(FASTEST)
            }
            _ => 0.0,
        };
        let cap = cfg.tree_fades.mesh + cfg.prefetch_m;
        Self {
            views,
            lead: Lead {
                ahead: cfg.level_lead_m + speed * cfg.level_lead_s,
                margin: cfg.level_margin_m,
            },
            now,
            dwell: cfg.level_dwell_s,
            cap,
            farthest: trees.mesh_reach().min(cap),
        }
    }

    /// No plant level is wanted or kept farther than this from the camera.
    fn reach(&self) -> f32 {
        self.farthest + self.lead.ahead + self.lead.margin
    }

    /// How far the camera has come since `check`, plus how far the lead
    /// has moved the bands' edges: what a plant's distance to an edge can
    /// have lost since then. Unbounded with more than one camera.
    fn moved_since(&self, check: &Checked) -> f32 {
        match self.views.as_slice() {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a camera's movement, metres"
            )]
            [view] => (*view - check.view).length() as f32 + (self.lead.ahead - check.ahead).abs(),
            _ => f32::INFINITY,
        }
    }

    /// The nearest camera's distance to `point`, as Bevy measures a
    /// visibility range: from the camera to the entity's translation.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "distances within the foliage ring"
    )]
    fn distance(&self, point: WorldPos) -> f32 {
        self.views
            .iter()
            .map(|view| (*view - point).length())
            .fold(f64::INFINITY, f64::min) as f32
    }

    fn cell_distance(&self, coord: RegionCoord) -> f64 {
        self.views
            .iter()
            .map(|view| distance_to_region(*view, coord))
            .fold(f64::INFINITY, f64::min)
    }
}

/// Makes and drops the mesh levels of one cell's plants for where the
/// camera is now: drops come out of `removals`, the frame's despawn budget,
/// and makes, when `spawn` allows them, out of the spawn and time budgets,
/// one plant's level (its two or so entities) at a time. The clock is read
/// every few plants, and a cell it stops is looked at again next frame, as
/// is one whose level could not be made or dropped this frame: the choice
/// is made from scratch each time.
#[allow(
    clippy::too_many_arguments,
    reason = "one cell's levels against the frame's shared budgets"
)]
fn update_plant_levels(
    commands: &mut Commands,
    resident: &mut Resident,
    coord: RegionCoord,
    trees: &TreeAssets,
    sight: &Sight,
    cfg: &FoliageConfig,
    budget: &mut Budget,
    removals: &mut usize,
    spawn: bool,
) {
    let cell_distance = sight.cell_distance(coord);
    let reach = f64::from(sight.reach());
    let near = cell_distance <= reach;
    if !near && resident.meshes == 0 {
        // Nothing to hold until the camera has come within reach; how far it
        // still has to come is the cell's slack.
        resident.checked = match sight.views.as_slice() {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "distances within the foliage ring"
            )]
            [view] => Some(Checked {
                view: *view,
                ahead: sight.lead.ahead,
                slack: (cell_distance - reach) as f32,
            }),
            _ => None,
        };
        return;
    }
    let flora = resident.placement.flora.clone();
    if resident.plants.len() == flora.plants.len()
        && resident
            .checked
            .is_some_and(|check| sight.moved_since(&check) < check.slack)
    {
        return;
    }
    resident.checked = None;
    if resident.plants.len() != flora.plants.len() {
        // A placement replaced by one with other plants: whatever the old
        // one held goes now, outside the budget, as it no longer matches.
        for entity in resident.take_meshes() {
            commands.entity(entity).despawn();
        }
        resident.plants = flora
            .plants
            .iter()
            .map(|plant| PlantLevels {
                bands: trees
                    .level_bands(plant)
                    .map(|band| band.map(|(start, end)| (start, end.min(sight.cap)))),
                held: Default::default(),
            })
            .collect();
    }
    let corner = coord.min_corner();
    let mut changes: Vec<(usize, u8)> = Vec::new();
    let mut slack = f32::INFINITY;
    let mut cut = false;
    for (index, (plant, state)) in flora.plants.iter().zip(&resident.plants).enumerate() {
        if index % PLANTS_PER_CLOCK == PLANTS_PER_CLOCK - 1 && !budget.time_left(cfg) {
            cut = true;
            break;
        }
        let held = state.mask();
        if !near && held == 0 {
            continue;
        }
        let distance = if near {
            sight.distance(WorldPos::new(
                corner.x() + f64::from(plant.x),
                f64::from(plant.y),
                corner.z() + f64::from(plant.z),
            ))
        } else {
            f32::INFINITY
        };
        let mask = plant_levels(
            distance,
            &state.bands,
            held,
            state.settled(sight.now, sight.dwell),
            sight.lead,
        );
        slack = slack.min(plant_slack(distance, &state.bands, mask, sight.lead));
        if mask != held {
            changes.push((index, mask));
        }
    }
    drop_levels(commands, resident, &changes, removals);
    let wants_new = changes
        .iter()
        .any(|&(index, mask)| mask & !resident.plants[index].mask() != 0);
    if spawn && wants_new {
        spawn_levels(
            commands, resident, &flora, &changes, trees, sight, cfg, budget,
        );
    }
    // Only a cell that got everything it wanted may be skipped: one left
    // short by a budget is looked at again next frame.
    let done = !cut
        && changes
            .iter()
            .all(|&(index, mask)| resident.plants[index].mask() == mask);
    if near
        && done
        && let [view] = sight.views.as_slice()
    {
        resident.checked = Some(Checked {
            view: *view,
            ahead: sight.lead.ahead,
            slack,
        });
    }
}

/// Makes the levels `changes` want, while the spawn and time budgets last.
#[allow(
    clippy::too_many_arguments,
    reason = "one cell's new levels against the frame's shared budgets"
)]
fn spawn_levels(
    commands: &mut Commands,
    resident: &mut Resident,
    flora: &CellFlora,
    changes: &[(usize, u8)],
    trees: &TreeAssets,
    sight: &Sight,
    cfg: &FoliageConfig,
    budget: &mut Budget,
) {
    let plants = &mut resident.plants;
    let meshes = &mut resident.meshes;
    commands.entity(resident.root).with_children(|parent| {
        'plants: for &(index, mask) in changes {
            let plant = &flora.plants[index];
            let state = &mut plants[index];
            for level in 0..MESH_LEVELS {
                if mask & (1 << level) == 0 || state.held[level].is_some() {
                    continue;
                }
                if !budget.entities_fit(2, cfg) || !budget.time_left(cfg) {
                    break 'plants;
                }
                let entities = tree::spawn_tree_level(
                    parent,
                    trees,
                    level,
                    plant,
                    Vec3::new(plant.x, plant.y, plant.z),
                );
                budget.spawned += entities.len() as u64;
                *meshes += entities.len() as u64;
                state.held[level] = Some(HeldLevel {
                    entities,
                    since: sight.now,
                });
            }
        }
    });
}

/// Despawns the levels `changes` no longer want, while `removals` lasts.
fn drop_levels(
    commands: &mut Commands,
    resident: &mut Resident,
    changes: &[(usize, u8)],
    removals: &mut usize,
) {
    for &(index, mask) in changes {
        let state = &mut resident.plants[index];
        for level in 0..MESH_LEVELS {
            if mask & (1 << level) != 0 {
                continue;
            }
            let Some(held) = &state.held[level] else {
                continue;
            };
            if held.entities.len() > *removals {
                continue;
            }
            let held = state.held[level].take().expect("checked held level");
            *removals -= held.entities.len();
            resident.meshes -= held.entities.len() as u64;
            for entity in held.entities {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Atomic mesh work is at most one tree LOD (two entities) or one cell buffer.
struct Budget {
    started: Instant,
    // The previous frame's apply, counted as spent before this one starts.
    carried_ms: f32,
    spawned: u64,
    uploaded: usize,
}

impl Budget {
    fn carrying(carried_ms: f32) -> Self {
        Self {
            started: Instant::now(),
            carried_ms,
            spawned: 0,
            uploaded: 0,
        }
    }
    fn time_left(&self, cfg: &FoliageConfig) -> bool {
        self.carried_ms + self.started.elapsed().as_secs_f32() * 1000.0 < cfg.collect_budget_ms
    }
    fn entities_fit(&self, count: u64, cfg: &FoliageConfig) -> bool {
        self.spawned + count <= cfg.spawn_budget_per_frame
    }
    fn upload_fits(&self, bytes: usize, cfg: &FoliageConfig) -> bool {
        cfg.upload_budget_bytes > 0
            && (self.uploaded + bytes <= cfg.upload_budget_bytes || self.uploaded == 0)
    }
}

fn retire_cells(
    commands: &mut Commands,
    stream: &mut CellStream,
    cfg: &FoliageConfig,
    eye: Option<WorldPos>,
) {
    let mut evicted: Vec<_> = stream
        .residents
        .keys()
        .copied()
        .filter(|coord| stream.reset || eye.is_none_or(|eye| !retained(*coord, eye, cfg)))
        .collect();
    evicted.sort_unstable();
    for coord in evicted {
        let Some(mut resident) = stream.residents.remove(&coord) else {
            continue;
        };
        stream.totals.change(Tally::of(&resident), Tally::default());
        stream.dirty.remove(&coord);
        stream.installs.remove(&coord);
        commands.entity(resident.root).insert(Visibility::Hidden);
        let grass_bytes = resident.grass_bytes();
        let mut owned = resident.take_meshes();
        owned.append(&mut resident.grass_entities);
        owned.append(&mut resident.far_entities);
        stream.retiring_grass += grass_bytes;
        stream.retiring.push_back(Retiring {
            root: Some(resident.root),
            children: owned,
            next: 0,
            grass_bytes,
            cell: coord,
            details: u8::MAX,
        });
        if !stream.reset {
            stream.cache.insert(coord, resident.placement, cfg);
        }
    }
    if stream.reset {
        // A new world: the ring and every cell's schedule start again.
        stream.dirty.clear();
        stream.installs.clear();
        stream.expiries.clear();
        stream.forget_selection();
    }
    stream.reset = false;
}

fn drain_retiring(
    commands: &mut Commands,
    stream: &mut CellStream,
    cfg: &FoliageConfig,
    budget: &Budget,
) -> usize {
    let mut despawned = 0;
    while despawned < cfg.despawn_budget_per_frame && budget.time_left(cfg) {
        let Some(retiring) = stream.retiring.front_mut() else {
            break;
        };
        if let Some(&child) = retiring.children.get(retiring.next) {
            commands.entity(child).despawn();
            retiring.next += 1;
        } else {
            if let Some(root) = retiring.root {
                commands.entity(root).despawn();
                despawned += 1;
            }
            stream.retiring_grass -= retiring.grass_bytes;
            stream.retiring.pop_front();
            continue;
        }
        despawned += 1;
    }
    despawned
}

/// What installing a cell's far trees and grass draws on.
struct Install<'a> {
    cfg: &'a FoliageConfig,
    assets: &'a GrassAssets,
    trees: Option<&'a TreeAssets>,
    device: &'a RenderDevice,
    queue: Option<&'a RenderQueue>,
    store: &'a mut FarTreeStore,
    far_materials: &'a mut Assets<FarTreeMaterial>,
    materials: &'a mut Assets<GrassMaterial>,
    now: f32,
    upload_ms: f32,
}

impl Install<'_> {
    /// Whether far trees can be installed at all this frame.
    fn far_ready(&self) -> bool {
        self.trees.is_some() && self.queue.is_some() && self.store.ready()
    }

    /// Installs what the cell still lacks of its far trees and, when
    /// `grass` is wanted, its grass, while the budgets last.
    fn cell(
        &mut self,
        commands: &mut Commands,
        resident: &mut Resident,
        grass: bool,
        retiring_mask: u8,
        budget: &mut Budget,
    ) {
        let cfg = self.cfg;
        let grass = grass_pending(resident, grass);
        let flora = &resident.placement.flora;
        commands.entity(resident.root).with_children(|parent| {
            // A cell's far batches are records written into the shared
            // buffer, budgeted as entities and as upload bytes; they wait
            // for the species' materials.
            if let Some(trees) = self.trees
                && !resident.far_installed
                && retiring_mask & FAR_BIT == 0
                && self.store.ready()
                && budget.time_left(cfg)
                && let Some(queue) = self.queue
            {
                let records = tree::far_trees(trees, &flora.plants);
                let (count, bytes) = far_cost(&records);
                if budget.entities_fit(count, cfg)
                    && (budget.uploaded == 0 || budget.uploaded + bytes <= cfg.upload_budget_bytes)
                {
                    let entities = spawn_far_trees(
                        parent,
                        trees,
                        &mut FarInstall {
                            store: &mut *self.store,
                            device: self.device,
                            queue,
                            materials: &mut *self.far_materials,
                        },
                        cfg,
                        &records,
                    );
                    resident.far_trees = records.values().map(|r| r.len() as u64).sum();
                    resident.far_installed = true;
                    budget.spawned += entities.len() as u64;
                    budget.uploaded += bytes;
                    resident.far_entities.extend(entities);
                }
            }
            let bytes = flora.grass.len() * size_of::<strata_flora::GrassBlade>();
            if retiring_mask & GRASS_BIT == 0
                && grass
                && budget.entities_fit(2, cfg)
                && budget.upload_fits(bytes, cfg)
                && budget.time_left(cfg)
            {
                let upload = Instant::now();
                let entities = spawn_grass(
                    parent,
                    cfg,
                    self.assets,
                    self.device,
                    &mut *self.materials,
                    flora,
                    self.now,
                );
                self.upload_ms += upload.elapsed().as_secs_f32() * 1000.0;
                budget.spawned += entities.len() as u64;
                budget.uploaded += bytes;
                resident.grass_entities = entities;
                resident.grass_spawned = true;
            }
        });
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one budget spans result acceptance, retirement, and incremental installation"
)]
pub(super) fn collect(
    mut commands: Commands,
    cfg: Res<FoliageConfig>,
    assets: Option<Res<GrassAssets>>,
    trees: Option<Res<TreeAssets>>,
    device: Res<RenderDevice>,
    queue: Option<Res<RenderQueue>>,
    time: Res<Time>,
    observers: Res<Observers>,
    origin: Option<Res<RenderOrigin>>,
    cameras: Query<
        (&Camera, &GlobalTransform),
        (With<Camera3d>, Without<strata_terrain::OffWorld>),
    >,
    mut materials: ResMut<Assets<GrassMaterial>>,
    mut far_materials: ResMut<Assets<FarTreeMaterial>>,
    mut store: ResMut<FarTreeStore>,
    mut stream: ResMut<CellStream>,
    mut stats: ResMut<FoliageStats>,
) {
    let stream = &mut *stream;
    let mut budget = Budget::carrying(
        stream
            .last_apply_ms
            .min(cfg.collect_budget_ms * APPLY_CARRY),
    );
    store.next_frame();
    stats.upload_ms = 0.0;
    stats.uploaded_bytes = 0;
    stats.spawned = 0;
    stats.despawned = 0;
    // Cells leave only when the eye has moved, which is when the ring is
    // chosen again: until then nothing can have left the retention band.
    let eye = observers.primary;
    if stream.reset || eye.is_none() || stream.swept != stream.generation {
        retire_cells(&mut commands, stream, &cfg, eye);
        if let Some(eye) = eye {
            retire_detail(stream, &cfg, eye);
        }
        stream.rebuild_installs();
        stream.swept = stream.generation;
    }
    stats.despawned = drain_retiring(&mut commands, stream, &cfg, &budget);
    let mut removals = cfg.despawn_budget_per_frame.saturating_sub(stats.despawned);
    // Plants' levels follow the cameras Bevy measures visibility ranges
    // from (a frame old here, which the lead covers), not the observer the
    // orbit camera trails by up to 20 m.
    let mut views: Vec<WorldPos> = origin.as_deref().map_or_else(Vec::new, |origin| {
        cameras
            .iter()
            .filter(|(camera, _)| camera.is_active)
            .map(|(_, transform)| origin.to_world(transform.translation()))
            .collect()
    });
    if views.is_empty() {
        views.extend(observers.primary);
    }
    let previous_view = std::mem::replace(&mut stream.previous_view, views.first().copied());
    let sight = trees.as_deref().filter(|_| !views.is_empty()).map(|trees| {
        Sight::new(
            views,
            previous_view,
            time.delta_secs(),
            time.elapsed_secs(),
            trees,
            &cfg,
        )
    });
    let stale_ready: Vec<_> = stream
        .ready
        .keys()
        .copied()
        .filter(|coord| stream.want(*coord).is_none())
        .collect();
    for coord in stale_ready {
        if let Some(placement) = stream.ready.remove(&coord) {
            stream.settle(coord, placement, &cfg);
        }
    }
    // A completed job may now be in the retention ring behind the camera.
    // Drain it too: otherwise it holds an in-flight slot until eviction.
    let finished: Vec<_> = stream
        .tasks
        .iter()
        .filter(|(_, task)| task.is_finished())
        .map(|(coord, _)| *coord)
        .collect();
    for coord in finished {
        if let Some(mut task) = stream.tasks.remove(&coord)
            && let Some(placement) = block_on(poll_once(&mut task))
        {
            stats.last_place_ms = placement.ms;
            stats.placed += 1;
            if stream.want(coord).is_some() {
                stream.ready.insert(coord, placement);
            } else {
                stream.settle(coord, placement, &cfg);
            }
        }
    }
    // Finished placements go in nearest first.
    let mut arrivals: Vec<(f64, RegionCoord)> = stream
        .ready
        .keys()
        .map(|coord| (stream.rank(*coord), *coord))
        .collect();
    arrivals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut accepted = 0;
    for (_, coord) in arrivals {
        if accepted >= cfg.collect_per_frame || !budget.time_left(&cfg) {
            break;
        }
        let resident = stream.residents.contains_key(&coord);
        if !resident && !budget.entities_fit(1, &cfg) {
            continue;
        }
        let placement = stream.ready.remove(&coord).expect("ready placement");
        if resident {
            stream.settle(coord, placement, &cfg);
        } else {
            stream.admit(&mut commands, coord, placement);
            budget.spawned += 1;
        }
        accepted += 1;
    }
    // The cells with work, nearest first: those whose plants are to be
    // looked at, and those with far trees or grass to install.
    let levels = trees.as_deref().zip(sight.as_ref());
    if let Some((_, sight)) = levels {
        stream.advance(sight);
    }
    let mut install = assets.as_deref().map(|assets| Install {
        cfg: &cfg,
        assets,
        trees: trees.as_deref(),
        device: &device,
        queue: queue.as_deref(),
        store: &mut store,
        far_materials: &mut far_materials,
        materials: &mut materials,
        now: time.elapsed_secs(),
        upload_ms: 0.0,
    });
    let far_ready = install.as_ref().is_some_and(Install::far_ready);
    let mut work: Vec<(f64, RegionCoord)> = Vec::new();
    if install.is_some() {
        // Far trees that cannot go in yet leave only the grass to try.
        work.extend(
            stream
                .installs
                .iter()
                .filter(|coord| {
                    far_ready
                        || stream.residents.get(coord).is_some_and(|resident| {
                            grass_pending(
                                resident,
                                stream.want(**coord).is_some_and(|want| want.grass),
                            )
                        })
                })
                .map(|coord| (stream.rank(*coord), *coord)),
        );
    }
    if levels.is_some() {
        work.extend(
            stream
                .dirty
                .iter()
                .map(|coord| (stream.rank(*coord), *coord)),
        );
    }
    work.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    work.dedup_by_key(|(_, coord)| *coord);
    let retiring = if work.is_empty() {
        HashMap::new()
    } else {
        stream.retiring_details()
    };
    let mut looked = 0;
    for (_, coord) in work {
        if !budget.time_left(&cfg) {
            break;
        }
        let retiring_mask = retiring.get(&coord).copied().unwrap_or(0);
        if let Some((trees, sight)) = levels
            && stream.dirty.contains(&coord)
        {
            stream.look(
                &mut commands,
                coord,
                trees,
                sight,
                &cfg,
                &mut budget,
                &mut removals,
                retiring_mask,
            );
            looked += 1;
        }
        if let Some(install) = install.as_mut()
            && stream.installs.contains(&coord)
        {
            stream.install(&mut commands, coord, install, retiring_mask, &mut budget);
        }
    }
    stats.upload_ms = install.map_or(0.0, |install| install.upload_ms);
    debug_assert_eq!(stream.totals, Tally::sum(stream.residents.values()));
    debug_assert_eq!(
        stream.retiring_grass,
        stream.retiring.iter().map(|r| r.grass_bytes).sum::<usize>()
    );
    let totals = stream.totals;
    stats.looked = looked;
    stats.despawned = cfg.despawn_budget_per_frame.saturating_sub(removals);
    stats.tree_meshes = totals.tree_meshes;
    stats.in_flight = stream.tasks.len() + stream.ready.len();
    stats.cells = totals.cells;
    stats.blades = totals.blades;
    stats.trees = totals.trees;
    stats.far_trees = totals.far_trees;
    stats.entities = totals.entities;
    stats.resident_cpu_bytes = totals.cpu_bytes;
    stats.resident_grass_bytes = totals.grass_bytes;
    stats.retiring_grass_bytes = stream.retiring_grass;
    stats.spawned = budget.spawned;
    stats.uploaded_bytes = budget.uploaded;
    stats.collect_ms = budget.started.elapsed().as_secs_f32() * 1000.0;
    stream.apply_started = Some(Instant::now());
}

pub(super) fn record_apply(mut stream: ResMut<CellStream>, mut stats: ResMut<FoliageStats>) {
    stats.apply_ms = stream
        .apply_started
        .take()
        .map_or(0.0, |t| t.elapsed().as_secs_f32() * 1000.0);
    stream.last_apply_ms = stats.apply_ms;
}

pub(super) fn clear(
    mut commands: Commands,
    mut stream: ResMut<CellStream>,
    mut stats: ResMut<FoliageStats>,
    mut store: ResMut<FarTreeStore>,
) {
    // The batches give their ranges back as they are despawned; the buffer
    // and the meshes stay for the next world, the species' materials do not.
    store.set_materials(Vec::new());
    for resident in stream.residents.values() {
        commands.entity(resident.root).despawn();
    }
    for retiring in &stream.retiring {
        if let Some(root) = retiring.root {
            commands.entity(root).despawn();
        }
    }
    *stream = CellStream::default();
    *stats = FoliageStats::default();
    commands.remove_resource::<PlaceInputs>();
    commands.remove_resource::<TreeAssets>();
    commands.remove_resource::<super::TreeLoading>();
}

/// The largest batch one entity draws: the largest shared quad mesh.
const FAR_BATCH: usize = impostor::CLASSES[impostor::CLASSES.len() - 1] as usize;

/// Entities and buffer bytes a cell's far trees take: one batch per species,
/// split past the largest mesh, each a range of its class's slots.
fn far_cost(records: &std::collections::BTreeMap<u32, Vec<impostor::FarTree>>) -> (u64, usize) {
    let mut entities = 0;
    let mut bytes = 0;
    for chunk in records.values().flat_map(|trees| trees.chunks(FAR_BATCH)) {
        if let Some(class) = impostor::class_for(chunk.len()) {
            entities += 1;
            bytes += impostor::CLASSES[class] as usize * RECORD_BYTES;
        }
    }
    (entities, bytes)
}

/// What installing far trees writes to.
struct FarInstall<'a> {
    store: &'a mut FarTreeStore,
    device: &'a RenderDevice,
    queue: &'a RenderQueue,
    materials: &'a mut Assets<FarTreeMaterial>,
}

/// One impostor batch per species with far trees in the cell: a range of
/// the shared buffer holding its trees, drawn by the shared quad mesh of
/// its size under the species' shared material, the range's start in its
/// tag. Its visibility range is the cell's: from where its nearest tree
/// could start fading in to where its farthest has faded out, measured from
/// the cell's corner, which is where the entity stands. Each tree then fades
/// on its own distance in the shader.
fn spawn_far_trees(
    parent: &mut ChildSpawnerCommands<'_>,
    trees: &TreeAssets,
    install: &mut FarInstall<'_>,
    cfg: &FoliageConfig,
    records: &std::collections::BTreeMap<u32, Vec<impostor::FarTree>>,
) -> Vec<Entity> {
    // The farthest a tree in the cell sits from its corner.
    const DIAGONAL: f32 = 90.6;
    let mut spawned = Vec::with_capacity(records.len());
    for (&species, species_trees) in records {
        let Some(far) = trees
            .get(strata_registry::FloraId(species))
            .and_then(|s| s.impostor.as_ref())
        else {
            continue;
        };
        let Some(material) = install.store.material(species).cloned() else {
            continue;
        };
        for cell_trees in species_trees.chunks(FAR_BATCH) {
            let Some((slots, mesh)) =
                install
                    .store
                    .install(cell_trees, install.device, install.queue, install.materials)
            else {
                continue;
            };
            let (low, high, scale) = cell_trees
                .iter()
                .fold((f32::MAX, f32::MIN, 0.0_f32), |(lo, hi, scale), t| {
                    (lo.min(t.root.y), hi.max(t.root.y), scale.max(t.scale))
                });
            let radius = far.radius * scale;
            let aabb = Aabb::from_min_max(
                Vec3::new(-radius, low - radius, -radius),
                Vec3::new(64.0 + radius, high + 3.0 * radius, 64.0 + radius),
            );
            let start = (far.fade_in - DIAGONAL).max(0.0);
            let end = cfg.tree_fades.far.1 + DIAGONAL;
            let entity = parent
                .spawn((
                    TreeImpostorBatch {
                        species: strata_registry::FloraId(species),
                        trees: u32::try_from(cell_trees.len()).unwrap_or(u32::MAX),
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    MeshTag(slots.start),
                    slots,
                    Transform::IDENTITY,
                    aabb,
                    NoAutoAabb,
                    NotShadowCaster,
                    VisibilityRange::abrupt(start, end),
                ))
                .id();
            spawned.push(entity);
        }
    }
    spawned
}

/// Takes the wet bank's plants off the crossings near the cell (ticket
/// 0005, step 8): the sample does not say where a deck stands, the world's
/// crossings do. Asked only of a cell that has such a plant, which only a
/// shore's does, so the rest of the world pays nothing; the cell's ground
/// is loaded, so its land mass is baked and the list is a lookup.
fn clear_crossings(
    flora: &mut CellFlora,
    table: &strata_flora::FloraTable,
    world: &strata_worldgen::ProceduralWorld,
) {
    let bank = |p: &strata_flora::Plant| {
        table
            .species()
            .get(p.species.0 as usize)
            .is_some_and(|s| s.ground.bank_m > 0.0)
    };
    if !flora.plants.iter().any(bank) {
        return;
    }
    let size = flora.coord.size();
    let corner = flora.coord.min_corner();
    // The cell's corners from its centre, and the widest keep-out past them.
    let reach = size * std::f64::consts::FRAC_1_SQRT_2 + 80.0;
    let crossings = world.crossings_near(corner.x() + 0.5 * size, corner.z() + 0.5 * size, reach);
    strata_flora::clear_bank_plants_off_crossings(flora, table, &crossings);
}

// Plants store metres; grass stores a packed fraction of the cell. Decode using
// the same root accessor as rendering before testing world-space occupancy.
fn exclude_occupied(
    flora: &mut CellFlora,
    exclusions: &strata_app::footprints::VegetationExclusions,
) {
    let corner = flora.coord.min_corner();
    flora
        .plants
        .retain(|p| !exclusions.contains(corner.x() + f64::from(p.x), corner.z() + f64::from(p.z)));
    flora.grass.retain(|p| {
        let [x, _, z] = p.root();
        !exclusions.contains(corner.x() + f64::from(x), corner.z() + f64::from(z))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn occupancy_decodes_packed_grass_roots_and_preserves_nearby_blades() {
        use strata_app::footprints::{Footprint, VegetationExclusions};
        let coord = RegionCoord::new(-24, -188, 0);
        let corner = coord.min_corner();
        let inside = strata_flora::GrassBlade {
            x: 32768,
            z: 32768,
            y_bits: 0,
            height_cm: 30,
            phase: 0,
            stiffness: 0,
            species: 0,
            lush: 0,
            shade: 0,
            nx: 0,
            nz: 0,
        };
        let outside = strata_flora::GrassBlade {
            x: 1024,
            z: 1024,
            ..inside
        };
        let mut flora = CellFlora {
            coord,
            plants: vec![],
            grass: vec![inside, outside],
        };
        let exclusions = VegetationExclusions(vec![Footprint {
            min: [corner.x() + 31.0, corner.z() + 31.0],
            max: [corner.x() + 33.0, corner.z() + 33.0],
        }]);
        exclude_occupied(&mut flora, &exclusions);
        assert_eq!(flora.grass, vec![outside]);
    }

    fn fixture_resident(world: &mut World, coord: RegionCoord) -> Resident {
        Resident {
            root: world
                .spawn((Transform::default(), Visibility::default()))
                .id(),
            placement: Placement {
                flora: Arc::new(CellFlora {
                    coord,
                    plants: vec![],
                    grass: vec![],
                }),
                with_grass: true,
                ms: 0.0,
            },
            plants: Vec::new(),
            meshes: 0,
            far_installed: true,
            far_entities: Vec::new(),
            grass_spawned: false,
            grass_entities: Vec::new(),
            far_trees: 0,
            checked: None,
            stamp: 0,
        }
    }

    /// Makes `resident` resident, counted in the running totals.
    fn insert(stream: &mut CellStream, coord: RegionCoord, resident: Resident) {
        stream.totals.change(Tally::default(), Tally::of(&resident));
        stream.residents.insert(coord, resident);
    }

    fn retire_test_system(
        mut commands: Commands,
        mut stream: ResMut<CellStream>,
        cfg: Res<FoliageConfig>,
        observers: Res<Observers>,
    ) {
        retire_cells(&mut commands, &mut stream, &cfg, observers.primary);
    }

    #[test]
    fn terrain_parent_replacement_preserves_resident_entity() {
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let mut resident = fixture_resident(&mut world, coord);
        let root = resident.root;
        let child = world.spawn((Transform::default(), ChildOf(root))).id();
        resident.far_entities.push(child);
        let mut stream = CellStream::default();
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(FoliageConfig::default());
        world.insert_resource(Observers {
            primary: Some(coord.center()),
            positions: vec![coord.center()],
        });
        // Terrain replaces a coarse parent with fine nodes independently of
        // the foliage root, as happens when the viewer approaches a cell.
        let parent = world
            .spawn(strata_stream::Region {
                coord: coord.parent().expect("parent"),
                state: strata_stream::RegionState::Ready,
            })
            .id();
        world.despawn(parent);
        for fine in coord
            .parent()
            .expect("parent")
            .children()
            .expect("children")
        {
            world.spawn(strata_stream::Region {
                coord: fine,
                state: strata_stream::RegionState::Ready,
            });
        }
        world
            .run_system_once(retire_test_system)
            .expect("retire system");
        let stream = world.resource::<CellStream>();
        assert_eq!(stream.residents[&coord].root, root);
        assert!(stream.residents[&coord].far_installed);
        assert!(stream.retiring.is_empty());
        assert!(world.get_entity(child).is_ok());
        // Leaving the retention band retires the same root and caches its
        // placement, while actual entity deletion remains separately budgeted.
        world.resource_mut::<Observers>().primary = Some(WorldPos::new(5000.0, 0.0, 0.0));
        world
            .run_system_once(retire_test_system)
            .expect("retire system");
        let stream = world.resource::<CellStream>();
        assert!(stream.residents.is_empty());
        assert_eq!(stream.retiring[0].root, Some(root));
        assert_eq!(stream.retiring[0].children, vec![child]);
        assert_eq!(stream.cache.entries.len(), 1);
        assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden));
    }

    fn detail_test_system(
        mut stream: ResMut<CellStream>,
        cfg: Res<FoliageConfig>,
        observers: Res<Observers>,
    ) {
        retire_detail(&mut stream, &cfg, observers.primary.expect("eye"));
    }

    fn drain_test_system(
        mut commands: Commands,
        mut stream: ResMut<CellStream>,
        cfg: Res<FoliageConfig>,
        mut stats: ResMut<FoliageStats>,
    ) {
        stats.despawned = drain_retiring(&mut commands, &mut stream, &cfg, &Budget::carrying(0.0));
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one scenario, frame by frame")]
    fn detail_retires_independently_and_cell_eviction_does_not_bypass_removal_budget() {
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let mut resident = fixture_resident(&mut world, coord);
        let root = resident.root;
        let entities: Vec<_> = (0..5)
            .map(|_| {
                world
                    .spawn((Transform::default(), Visibility::default(), ChildOf(root)))
                    .id()
            })
            .collect();
        // One plant through a crossfade, holding two levels.
        resident.plants = vec![PlantLevels {
            bands: [Some((0.0, 66.0)), Some((60.0, 165.0)), None],
            held: [
                Some(HeldLevel {
                    entities: vec![entities[0]],
                    since: 0.0,
                }),
                Some(HeldLevel {
                    entities: vec![entities[1]],
                    since: 0.0,
                }),
                None,
            ],
        }];
        resident.meshes = 2;
        resident.far_entities = vec![entities[2]];
        resident.grass_entities = vec![entities[3], entities[4]];
        resident.grass_spawned = true;
        Arc::make_mut(&mut resident.placement.flora)
            .grass
            .push(strata_flora::GrassBlade {
                x: 0,
                z: 0,
                y_bits: 10.0f32.to_bits(),
                height_cm: 50,
                phase: 0,
                stiffness: 0,
                species: 0,
                lush: 0,
                shade: 0,
                nx: 0,
                nz: 0,
            });
        let mut stream = CellStream::default();
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(FoliageConfig {
            despawn_budget_per_frame: 1,
            collect_budget_ms: 10_000.0,
            ..Default::default()
        });
        world.insert_resource(FoliageStats::default());
        // Inside near-detail hysteresis, nothing is retired.
        world.insert_resource(Observers {
            primary: Some(WorldPos::new(350.0, 0.0, 32.0)),
            positions: Vec::new(),
        });
        world
            .run_system_once(detail_test_system)
            .expect("detail retirement");
        assert!(world.resource::<CellStream>().retiring.is_empty());
        // Past the grass retention range, the far tree, the plant's levels
        // (which leave one plant at a time, elsewhere) and the cell survive.
        world.resource_mut::<Observers>().primary = Some(WorldPos::new(900.0, 0.0, 32.0));
        world
            .run_system_once(detail_test_system)
            .expect("detail retirement");
        let stream = world.resource::<CellStream>();
        let resident = &stream.residents[&coord];
        assert_eq!(resident.root, root);
        assert!(resident.far_installed);
        assert_eq!(resident.entities(), 4);
        assert!(!resident.placement.with_grass);
        assert!(resident.placement.flora.grass.is_empty());
        assert_eq!(stream.retiring[0].grass_bytes, 16);
        assert_eq!(
            stream.retiring_details()[&coord],
            GRASS_BIT,
            "replacement grass waits for retirement"
        );
        assert_eq!(
            world.get::<Visibility>(entities[0]),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            world.get::<Visibility>(entities[2]),
            Some(&Visibility::Inherited)
        );
        // Evict before queued detail deletions finish. No duplicated ownership
        // and no root cascade may delete children above the per-frame cap.
        world.resource_mut::<Observers>().primary = Some(WorldPos::new(5000.0, 0.0, 32.0));
        world
            .run_system_once(retire_test_system)
            .expect("cell retirement");
        for remaining in (0..6).rev() {
            world
                .run_system_once(drain_test_system)
                .expect("budgeted removal");
            assert_eq!(world.resource::<FoliageStats>().despawned, 1);
            let count = entities
                .iter()
                .chain(std::iter::once(&root))
                .filter(|&&entity| world.get_entity(entity).is_ok())
                .count();
            assert_eq!(count, remaining);
        }
        world
            .run_system_once(drain_test_system)
            .expect("finish queue");
        assert!(world.resource::<CellStream>().retiring.is_empty());
    }

    #[test]
    fn dispatch_leaves_a_resident_cell_and_its_far_trees_alone() {
        struct UnusedSource;
        impl strata_worldgen::WorldSource for UnusedSource {
            fn sample(&self, _: f64, _: f64) -> strata_worldgen::Sample {
                panic!("dispatch is disabled in this scheduling test")
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let resident = fixture_resident(&mut world, coord);
        let root = resident.root;
        let seed = Seed(42);
        let mut stream = CellStream {
            epoch: Some(seed),
            ..Default::default()
        };
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(FoliageConfig {
            dispatch_per_frame: 0,
            ..Default::default()
        });
        world.insert_resource(WorldSeed(seed));
        world.insert_resource(WorldSourceRes(Arc::new(UnusedSource)));
        world.insert_resource(PlaceInputs {
            table: Arc::new(strata_flora::FloraTable::default()),
        });
        world.insert_resource(LoadedRegions::default());
        world.insert_resource(FoliageStats::default());
        world.insert_resource(Observers {
            primary: Some(coord.center()),
            positions: vec![coord.center()],
        });
        world.run_system_once(dispatch).expect("dispatch system");
        let stream = world.resource::<CellStream>();
        let resident = &stream.residents[&coord];
        assert!(resident.far_installed);
        assert!(resident.plants.is_empty());
        assert_eq!(resident.root, root);
        assert!(stream.tasks.is_empty());
        assert!(stream.ready.is_empty());
    }

    /// A baked tree's three bands as the props give them: 60, 150 and
    /// 350 m with a tenth's crossfade.
    const TREE_BANDS: [Option<(f32, f32)>; MESH_LEVELS] =
        [Some((0.0, 66.0)), Some((60.0, 165.0)), Some((150.0, 385.0))];
    const WALK: Lead = Lead {
        ahead: 8.0,
        margin: 8.0,
    };

    fn drawn_at(distance: f32) -> u8 {
        bits(
            TREE_BANDS
                .iter()
                .map(|b| b.is_some_and(|(s, e)| distance >= s && distance < e)),
        )
    }

    #[test]
    fn a_plant_holds_the_level_it_draws_and_the_next_only_through_a_crossfade() {
        // Out from the camera and back, settled levels: never a level
        // missing where Bevy would draw it, never more than two, and one
        // inside a band's own stretch.
        let mut held = 0;
        let steps: Vec<f32> = (0..=900_u16)
            .chain((0..=900).rev())
            .map(|i| f32::from(i) * 0.5)
            .collect();
        for distance in steps {
            held = plant_levels(distance, &TREE_BANDS, held, held, WALK);
            let drawn = drawn_at(distance);
            assert_eq!(held & drawn, drawn, "a hole at {distance} m");
            assert!(held.count_ones() <= 2, "{held:03b} at {distance} m");
        }
        assert_eq!(plant_levels(30.0, &TREE_BANDS, 0, 0, WALK), 0b001);
        assert_eq!(plant_levels(110.0, &TREE_BANDS, 0, 0, WALK), 0b010);
        assert_eq!(plant_levels(250.0, &TREE_BANDS, 0, 0, WALK), 0b100);
        assert_eq!(plant_levels(420.0, &TREE_BANDS, 0, 0, WALK), 0);
        // The next level is made ahead of its fade-in.
        assert_eq!(plant_levels(53.0, &TREE_BANDS, 0b001, 0b001, WALK), 0b011);
    }

    #[test]
    fn a_camera_on_a_band_edge_neither_makes_nor_drops_a_level_each_step() {
        // Level 0 ends at 66 m, is wanted to 74 and kept to 82.
        let mut held = plant_levels(60.0, &TREE_BANDS, 0, 0, WALK);
        assert_eq!(held, 0b011);
        let mut changes = 0;
        for step in 0..200 {
            let distance = if step % 2 == 0 { 70.0 } else { 80.0 };
            let next = plant_levels(distance, &TREE_BANDS, held, held, WALK);
            changes += u32::from(next != held);
            held = next;
        }
        assert_eq!(changes, 0);
        assert_eq!(held, 0b011);
        // Past the margin a settled level goes; one younger than its dwell
        // stays until it is settled.
        assert_eq!(plant_levels(83.0, &TREE_BANDS, 0b011, 0b011, WALK), 0b010);
        assert_eq!(plant_levels(83.0, &TREE_BANDS, 0b011, 0b010, WALK), 0b011);
        // A level the plant does not have is never wanted.
        let understory = [None, Some((0.0, 180.0)), None];
        assert_eq!(plant_levels(10.0, &understory, 0, 0, WALK), 0b010);
    }

    #[test]
    fn within_its_slack_a_plant_wants_what_it_holds() {
        // From every settled state on a walk out and back, any move short of
        // the slack leaves the choice as it is, which is what lets a cell
        // skip its plants until the camera has come that far.
        let mut held = 0;
        let steps: Vec<f32> = (0..=900_u16)
            .chain((0..=900).rev())
            .map(|i| f32::from(i) * 0.5)
            .collect();
        for distance in steps {
            held = plant_levels(distance, &TREE_BANDS, held, held, WALK);
            if plant_levels(distance, &TREE_BANDS, held, held, WALK) != held {
                continue;
            }
            let slack = plant_slack(distance, &TREE_BANDS, held, WALK).min(40.0);
            for moved in [-0.99, -0.5, 0.5, 0.99].map(|share| share * slack) {
                assert_eq!(
                    plant_levels(distance + moved, &TREE_BANDS, held, held, WALK),
                    held,
                    "{held:03b} at {distance} m moved {moved} m"
                );
            }
        }
        // A level kept only by its dwell has no slack.
        assert!(plant_slack(90.0, &TREE_BANDS, 0b011, WALK) <= 0.0);
    }

    #[derive(Resource)]
    struct Now(f32);

    fn levels_test_system(
        mut commands: Commands,
        mut stream: ResMut<CellStream>,
        trees: Res<TreeAssets>,
        cfg: Res<FoliageConfig>,
        observers: Res<Observers>,
        now: Res<Now>,
    ) {
        let sight = Sight::new(
            observers.primary.into_iter().collect(),
            None,
            0.0,
            now.0,
            &trees,
            &cfg,
        );
        let mut budget = Budget::carrying(0.0);
        let mut removals = cfg.despawn_budget_per_frame;
        for (coord, resident) in &mut stream.residents {
            update_plant_levels(
                &mut commands,
                resident,
                *coord,
                &trees,
                &sight,
                &cfg,
                &mut budget,
                &mut removals,
                true,
            );
        }
    }

    fn tree_fixture() -> TreeAssets {
        use crate::tree::{TreeLevel, TreeSpecies, TreeVariantAssets};
        let level = |(start, end): (f32, f32)| TreeLevel {
            pieces: vec![crate::TreePiece::default(), crate::TreePiece::default()],
            range: VisibilityRange {
                start_margin: start..start,
                end_margin: end..end,
                use_aabb: false,
            },
            shadows: false,
        };
        TreeAssets {
            species: vec![Some(TreeSpecies {
                understory: false,
                variants: vec![TreeVariantAssets {
                    levels: TREE_BANDS.iter().flatten().copied().map(level).collect(),
                    impostor_scale: 1.0,
                    pivot_slot: 0,
                }],
                impostor: None,
            })],
        }
    }

    fn plant_at(coord: RegionCoord, x: f32, z: f32) -> strata_flora::Plant {
        strata_flora::Plant {
            id: strata_flora::PlantId {
                species_key: 1,
                cell: coord,
                // Only identity; the fixture's plants sit on a whole-metre grid.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "fixture coordinates in 0..64"
                )]
                slot: [x as u32, z as u32],
            },
            species: strata_registry::FloraId(0),
            x,
            y: 0.0,
            z,
            yaw: 0.0,
            scale: 1.0,
            variant: 0,
            sky: u8::MAX,
        }
    }

    /// The tree entities alive, by level.
    fn levels_alive(world: &mut World) -> [usize; MESH_LEVELS] {
        let mut count = [0; MESH_LEVELS];
        for tree in world.query::<&crate::TreeInstance>().iter(world) {
            count[tree.lod] += 1;
        }
        count
    }

    #[test]
    fn a_cell_moving_across_the_bands_ends_with_one_levels_entities() {
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let mut resident = fixture_resident(&mut world, coord);
        let plants: Vec<_> = (0..4_u8)
            .flat_map(|i| {
                (0..4_u8).map(move |j| (8.0 + 16.0 * f32::from(i), 8.0 + 16.0 * f32::from(j)))
            })
            .map(|(x, z)| plant_at(coord, x, z))
            .collect();
        let count = plants.len();
        resident.placement.flora = Arc::new(CellFlora {
            coord,
            plants,
            grass: vec![],
        });
        let mut stream = CellStream::default();
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(tree_fixture());
        world.insert_resource(FoliageConfig::default());
        world.insert_resource(Now(0.0));
        world.insert_resource(Observers {
            primary: Some(coord.center()),
            positions: Vec::new(),
        });
        world
            .run_system_once(levels_test_system)
            .expect("level update");
        assert_eq!(levels_alive(&mut world), [2 * count, 0, 0]);
        let check = world.resource::<CellStream>().residents[&coord]
            .checked
            .expect("a settled cell is skipped until the camera moves");
        assert!(check.slack > 1.0, "{check:?}");

        // The camera backs away west at walking pace, a metre a frame at
        // sixty frames a second, until the cell is 250 m off.
        let mut most = 0;
        for step in 1..=260_u16 {
            let x = 32.0 - f32::from(step);
            world.resource_mut::<Observers>().primary =
                Some(WorldPos::new(f64::from(x), 0.0, 32.0));
            world.resource_mut::<Now>().0 = f32::from(step) / 60.0;
            world
                .run_system_once(levels_test_system)
                .expect("level update");
            let stream = world.resource::<CellStream>();
            let resident = &stream.residents[&coord];
            for (plant, state) in resident.placement.flora.plants.iter().zip(&resident.plants) {
                let distance = (plant.x - x).hypot(plant.z - 32.0);
                let drawn = drawn_at(distance);
                assert_eq!(state.mask() & drawn, drawn, "a hole at {distance} m");
                most = most.max(state.mask().count_ones());
            }
        }
        assert_eq!(most, 2, "the crossfades hold two levels, no more");
        // Once the last level made has dwelt, only the level the cell's
        // band draws is left.
        world.resource_mut::<Now>().0 += 2.0;
        world
            .run_system_once(levels_test_system)
            .expect("level update");
        assert_eq!(levels_alive(&mut world), [0, 0, 2 * count]);
        let stream = world.resource::<CellStream>();
        assert_eq!(stream.residents[&coord].meshes, 2 * count as u64);
        assert_eq!(stream.residents[&coord].entities(), 2 * count as u64 + 1);
    }

    #[test]
    fn levels_far_past_every_band_are_dropped_and_never_made() {
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let mut resident = fixture_resident(&mut world, coord);
        resident.placement.flora = Arc::new(CellFlora {
            coord,
            plants: vec![plant_at(coord, 32.0, 32.0)],
            grass: vec![],
        });
        let mut stream = CellStream::default();
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(tree_fixture());
        world.insert_resource(FoliageConfig::default());
        world.insert_resource(Now(0.0));
        world.insert_resource(Observers {
            primary: Some(coord.center()),
            positions: Vec::new(),
        });
        world
            .run_system_once(levels_test_system)
            .expect("level update");
        assert_eq!(levels_alive(&mut world), [2, 0, 0]);
        // A teleport a kilometre away: the near level goes once it has
        // dwelt, and nothing replaces it.
        world.resource_mut::<Observers>().primary = Some(WorldPos::new(1032.0, 0.0, 32.0));
        world.resource_mut::<Now>().0 = 0.5;
        world
            .run_system_once(levels_test_system)
            .expect("level update");
        assert_eq!(levels_alive(&mut world), [2, 0, 0], "inside its dwell");
        world.resource_mut::<Now>().0 = 2.0;
        world
            .run_system_once(levels_test_system)
            .expect("level update");
        assert_eq!(levels_alive(&mut world), [0, 0, 0]);
        assert_eq!(world.resource::<CellStream>().residents[&coord].meshes, 0);
    }

    #[test]
    fn selection_is_independent_of_terrain_lod_and_world_height() {
        let cfg = FoliageConfig::default();
        let low = WorldPos::new(-2528.0, 0.0, -37088.0);
        let high = WorldPos::new(-2528.0, 2000.0, -37088.0);
        let a = select_cells(low, (0.0, 0.0), &cfg);
        let b = select_cells(high, (0.0, 0.0), &cfg);
        assert_eq!(
            a.iter().map(|w| w.coord).collect::<Vec<_>>(),
            b.iter().map(|w| w.coord).collect::<Vec<_>>()
        );
        assert!(a[0].grass);
        assert!(a.len() < 3000, "cell count bounded by render range");
    }

    #[test]
    fn hysteresis_keeps_a_cell_after_it_leaves_preparation_range() {
        let cfg = FoliageConfig::default();
        let coord = RegionCoord::new(0, 0, 0);
        let eye = WorldPos::new(preparation_radius(&cfg) + 65.0, 0.0, 32.0);
        assert!(
            !select_cells(eye, (0.0, 0.0), &cfg)
                .iter()
                .any(|w| w.coord == coord)
        );
        assert!(retained(coord, eye, &cfg));
        assert!(!retained(coord, WorldPos::new(2000.0, 0.0, 32.0), &cfg));
    }

    #[test]
    fn frame_budget_never_overshoots_entities_or_accumulates_oversize_uploads() {
        let cfg = FoliageConfig::default();
        let mut budget = Budget::carrying(0.0);
        budget.spawned = cfg.spawn_budget_per_frame - 1;
        assert!(budget.entities_fit(1, &cfg));
        assert!(!budget.entities_fit(2, &cfg));
        assert!(budget.upload_fits(cfg.upload_budget_bytes + 1, &cfg));
        budget.uploaded = cfg.upload_budget_bytes + 1;
        assert!(!budget.upload_fits(1, &cfg));
    }

    #[test]
    fn cache_evicts_old_cells_and_preserves_placement_identity() {
        let cfg = FoliageConfig {
            cache_cells: 1,
            ..Default::default()
        };
        let a = RegionCoord::new(0, 0, 0);
        let b = RegionCoord::new(1, 0, 0);
        let make = |coord| Placement {
            flora: Arc::new(CellFlora {
                coord,
                plants: vec![],
                grass: vec![],
            }),
            with_grass: true,
            ms: 1.0,
        };
        let mut cache = Cache::default();
        cache.insert(a, make(a), &cfg);
        let original = make(b);
        cache.insert(b, original.clone(), &cfg);
        assert!(cache.take(a).is_none());
        let result = cache.take(b).expect("cached cell");
        assert!(Arc::ptr_eq(&original.flora, &result.flora));
        assert!(result.with_grass);
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn the_ring_is_chosen_again_only_when_the_eye_has_moved_or_turned() {
        let eye = WorldPos::new(20.0, 0.0, 20.0);
        let east = (1.0, 0.0);
        let at = |eye: WorldPos, heading| Selection::new(eye, heading, &FoliageConfig::default());
        let turned = |degrees: f64| {
            let r = degrees.to_radians();
            (r.cos(), r.sin())
        };
        let selection = at(eye, east);
        assert!(reselect(None, eye, east));
        assert!(!reselect(Some(&selection), eye, east));
        assert!(
            !reselect(Some(&selection), eye, (0.0, 0.0)),
            "stopping keeps the ring"
        );
        assert!(!reselect(
            Some(&selection),
            WorldPos::new(26.0, 0.0, 20.0),
            east
        ));
        assert!(reselect(
            Some(&selection),
            WorldPos::new(20.0, 0.0, 28.5),
            east
        ));
        assert!(!reselect(Some(&selection), eye, turned(20.0)));
        assert!(reselect(Some(&selection), eye, turned(45.0)));
        // A cell edge a metre and a half away.
        let edge = at(WorldPos::new(63.0, 0.0, 20.0), east);
        assert!(reselect(Some(&edge), WorldPos::new(64.5, 0.0, 20.0), east));
    }

    #[test]
    fn a_still_eye_keeps_its_ring_and_dispatch_walks_only_the_missing_cells() {
        struct UnusedSource;
        impl strata_worldgen::WorldSource for UnusedSource {
            fn sample(&self, _: f64, _: f64) -> strata_worldgen::Sample {
                panic!("dispatch is disabled in this scheduling test")
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        let mut world = World::new();
        let coord = RegionCoord::new(0, 0, 0);
        let resident = fixture_resident(&mut world, coord);
        let seed = Seed(42);
        let mut stream = CellStream {
            epoch: Some(seed),
            ..Default::default()
        };
        insert(&mut stream, coord, resident);
        world.insert_resource(stream);
        world.insert_resource(FoliageConfig {
            // No region is loaded, so no cell can start a job and
            // every walk reaches the end of the list.
            dispatch_per_frame: 4,
            ..Default::default()
        });
        world.insert_resource(WorldSeed(seed));
        world.insert_resource(WorldSourceRes(Arc::new(UnusedSource)));
        world.insert_resource(PlaceInputs {
            table: Arc::new(strata_flora::FloraTable::default()),
        });
        world.insert_resource(LoadedRegions::default());
        world.insert_resource(FoliageStats::default());
        let eye = coord.center();
        world.insert_resource(Observers {
            primary: Some(eye),
            positions: vec![eye],
        });
        let system = world.register_system(dispatch);
        let run = |world: &mut World, eye: WorldPos| {
            world.resource_mut::<Observers>().primary = Some(eye);
            world.run_system(system).expect("dispatch system");
            let stream = world.resource::<CellStream>();
            (stream.generation, stream.unsatisfied.len())
        };
        let ranks = select_cells(eye, (0.0, 0.0), &FoliageConfig::default()).len();
        let (generation, unsatisfied) = run(&mut world, eye);
        assert_eq!(generation, 1);
        assert!(ranks > 2000, "{ranks}");
        assert_eq!(unsatisfied, ranks - 1, "the resident cell is not walked");
        for _ in 0..3 {
            assert_eq!(run(&mut world, eye).0, 1, "a still eye keeps its ring");
        }
        let step = |dx: f64| WorldPos::new(eye.x() + dx, eye.y(), eye.z());
        assert_eq!(
            run(&mut world, step(3.0)).0,
            2,
            "starting to move ranks ahead"
        );
        assert_eq!(
            run(&mut world, step(6.0)).0,
            2,
            "3 m on in the same heading"
        );
        assert_eq!(run(&mut world, step(12.0)).0, 3, "8 m from the last ring");
    }

    /// One frame of the collector's level walk with its odometer queue,
    /// or, with `full`, the walk before it: every resident each frame.
    #[derive(Resource, Default)]
    struct Walk {
        now: f32,
        delta: f32,
        previous: Option<WorldPos>,
        full: bool,
        looked: u64,
    }

    fn walk_test_system(
        mut commands: Commands,
        mut stream: ResMut<CellStream>,
        trees: Res<TreeAssets>,
        cfg: Res<FoliageConfig>,
        observers: Res<Observers>,
        mut walk: ResMut<Walk>,
    ) {
        let views: Vec<_> = observers.primary.into_iter().collect();
        let sight = Sight::new(views, walk.previous, walk.delta, walk.now, &trees, &cfg);
        walk.previous = observers.primary;
        let stream = &mut *stream;
        if walk.full {
            stream.dirty.extend(stream.residents.keys().copied());
        }
        stream.advance(&sight);
        let mut order: Vec<_> = stream
            .dirty
            .iter()
            .map(|coord| (stream.rank(*coord), *coord))
            .collect();
        order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut budget = Budget::carrying(0.0);
        let mut removals = cfg.despawn_budget_per_frame;
        let retiring = stream.retiring_details();
        walk.looked = 0;
        for (_, coord) in order {
            if !budget.time_left(&cfg) {
                break;
            }
            let mask = retiring.get(&coord).copied().unwrap_or(0);
            stream.look(
                &mut commands,
                coord,
                &trees,
                &sight,
                &cfg,
                &mut budget,
                &mut removals,
                mask,
            );
            walk.looked += 1;
        }
    }

    /// A strip of wanted cells, sixteen plants each, every cell dirty.
    fn strip(full: bool) -> World {
        let mut world = World::new();
        // A ring from the strip's middle wants all of it.
        let mut stream = CellStream {
            selection: Some(Selection::new(
                WorldPos::new(320.0, 0.0, 32.0),
                (0.0, 0.0),
                &FoliageConfig::default(),
            )),
            ..Default::default()
        };
        let cells = (-2..=2).flat_map(|z| (-3..=12).map(move |x| RegionCoord::new(x, z, 0)));
        for coord in cells {
            let mut resident = fixture_resident(&mut world, coord);
            let plants = (0..4_u8)
                .flat_map(|i| {
                    (0..4_u8).map(move |j| (5.0 + 16.0 * f32::from(i), 11.0 + 16.0 * f32::from(j)))
                })
                .map(|(x, z)| plant_at(coord, x, z))
                .collect();
            resident.placement.flora = Arc::new(CellFlora {
                coord,
                plants,
                grass: vec![],
            });
            insert(&mut stream, coord, resident);
            stream.dirty.insert(coord);
        }
        world.insert_resource(stream);
        world.insert_resource(tree_fixture());
        // Budgets that never bind: what is compared is which cells are
        // looked at, not the order a budget would cut them in.
        world.insert_resource(FoliageConfig {
            collect_budget_ms: 10_000.0,
            spawn_budget_per_frame: 100_000,
            despawn_budget_per_frame: 100_000,
            ..Default::default()
        });
        world.insert_resource(Walk {
            delta: 1.0 / 18.0,
            full,
            ..Default::default()
        });
        world.insert_resource(Observers {
            primary: None,
            positions: Vec::new(),
        });
        world
    }

    /// Every plant's held levels, by cell.
    fn masks(world: &World) -> Vec<(RegionCoord, Vec<u8>)> {
        let stream = world.resource::<CellStream>();
        let mut masks: Vec<_> = stream
            .residents
            .iter()
            .map(|(coord, r)| (*coord, r.plants.iter().map(PlantLevels::mask).collect()))
            .collect();
        masks.sort_unstable_by_key(|(coord, _)| *coord);
        masks
    }

    /// Plants a level of which Bevy would draw from `eye` and that do not
    /// hold it.
    fn holes(world: &World, eye: WorldPos) -> usize {
        let stream = world.resource::<CellStream>();
        let mut holes = 0;
        for (coord, resident) in &stream.residents {
            let corner = coord.min_corner();
            for (index, plant) in resident.placement.flora.plants.iter().enumerate() {
                let root = WorldPos::new(
                    corner.x() + f64::from(plant.x),
                    f64::from(plant.y),
                    corner.z() + f64::from(plant.z),
                );
                #[allow(clippy::cast_possible_truncation, reason = "metres in the strip")]
                let drawn = drawn_at((eye - root).length() as f32);
                let held = resident.plants.get(index).map_or(0, PlantLevels::mask);
                holes += usize::from(held & drawn != drawn);
            }
        }
        holes
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one route, frame by frame")]
    fn the_odometer_queue_makes_what_the_full_walk_makes_at_any_speed() {
        // A camera 2 m up stands, walks east, flies at 270 m/s at eighteen
        // frames a second, stops past the dwell and comes back west; each
        // phase is its frames and metres a frame.
        let route: [(u16, f64); 5] = [(40, 0.0), (100, 1.4), (60, 15.0), (40, 0.0), (80, -5.0)];
        let mut queued = strip(false);
        let mut full = strip(true);
        let queued_walk = queued.register_system(walk_test_system);
        let full_walk = full.register_system(walk_test_system);
        let mut x = -100.0;
        let mut frame = 0_u16;
        let mut most_looked = 0;
        for (phase, (frames, step)) in route.into_iter().enumerate() {
            for i in 0..frames {
                x += step;
                frame += 1;
                let eye = WorldPos::new(x, 2.0, 32.0);
                for (world, system) in [(&mut queued, queued_walk), (&mut full, full_walk)] {
                    world.resource_mut::<Observers>().primary = Some(eye);
                    world.resource_mut::<Walk>().now = f32::from(frame) / 18.0;
                    world.run_system(system).expect("level walk");
                }
                assert_eq!(masks(&queued), masks(&full), "frame {frame} at x {x}");
                assert_eq!(holes(&queued, eye), 0, "a hole on frame {frame} at x {x}");
                let stream = queued.resource::<CellStream>();
                assert_eq!(stream.totals, Tally::sum(stream.residents.values()));
                let looked = queued.resource::<Walk>().looked;
                if step == 0.0 && i + 10 >= frames {
                    assert_eq!(looked, 0, "a still camera, phase {phase} frame {i}");
                }
                if step != 0.0 {
                    most_looked = most_looked.max(looked);
                }
            }
        }
        // Moving, the queue looks at the cells whose slack ran out, not at
        // all eighty.
        assert!(most_looked < 80, "{most_looked}");
        let full_looked = full.resource::<Walk>().looked;
        assert_eq!(full_looked, 80);
    }

    fn look_once_system(
        mut commands: Commands,
        mut stream: ResMut<CellStream>,
        trees: Res<TreeAssets>,
        cfg: Res<FoliageConfig>,
        observers: Res<Observers>,
    ) {
        let sight = Sight::new(
            observers.primary.into_iter().collect(),
            None,
            0.0,
            0.0,
            &trees,
            &cfg,
        );
        let coord = RegionCoord::new(0, 0, 0);
        let mut budget = Budget::carrying(0.0);
        let mut removals = cfg.despawn_budget_per_frame;
        stream.look(
            &mut commands,
            coord,
            &trees,
            &sight,
            &cfg,
            &mut budget,
            &mut removals,
            0,
        );
    }

    #[test]
    fn a_cell_the_clock_stops_is_looked_at_again_and_finished() {
        let mut world = strip(false);
        world.resource_mut::<FoliageConfig>().collect_budget_ms = 0.0;
        world.resource_mut::<Observers>().primary = Some(WorldPos::new(32.0, 2.0, 32.0));
        world.run_system_once(look_once_system).expect("look");
        let coord = RegionCoord::new(0, 0, 0);
        {
            let stream = world.resource::<CellStream>();
            assert!(stream.residents[&coord].checked.is_none());
            assert!(stream.dirty.contains(&coord));
            assert_eq!(stream.residents[&coord].meshes, 0, "no time, nothing made");
        }
        world.resource_mut::<FoliageConfig>().collect_budget_ms = 10_000.0;
        world.run_system_once(look_once_system).expect("look");
        let stream = world.resource::<CellStream>();
        assert!(stream.residents[&coord].checked.is_some());
        assert!(!stream.dirty.contains(&coord));
        assert_eq!(stream.residents[&coord].meshes, 32);
        assert_eq!(stream.totals, Tally::sum(stream.residents.values()));
    }

    #[test]
    fn the_last_apply_counts_against_the_budget_up_to_half() {
        let cfg = FoliageConfig::default();
        assert!(Budget::carrying(cfg.collect_budget_ms * APPLY_CARRY).time_left(&cfg));
        assert!(!Budget::carrying(cfg.collect_budget_ms).time_left(&cfg));
    }
}
