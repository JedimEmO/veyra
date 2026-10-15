//! The ground: a static heightfield body per lod 0 region.
//!
//! The streamer spawns a region entity per wanted node and says so with
//! [`RegionLoad`]; this module answers the lod 0 ones with a collider, the
//! way the terrain crate answers with a mesh, and neither knows the other
//! exists. The heights are the world's own samples on the region's grid, so
//! the collider is the same surface the mesh draws: the mesh builder samples
//! the grid at `min_corner + (i, j) * step` (through a ring offset that is
//! exact at lod 0, where every coordinate is an integer), and
//! [`Heightfield::sample`] asks for exactly those points. Parry splits every
//! cell on the same diagonal the mesh does, from `(i + 1, j)` to
//! `(i, j + 1)`. A test in the facade crate compares the two bit for bit.
//!
//! Sampling runs on the compute pool like meshing does (ADR 0003), and so
//! does every shape: the heightfield, the trunk cylinders and log capsules
//! and the hull compounds of rocks and cliffs, the hulls through the
//! [`ShapeCache`]. A task is dropped when its region unloads before it
//! finishes.
//!
//! # What the main thread pays, and in what order
//!
//! Spawning a body runs avian's insert hooks (its mass properties, its
//! required components), about 0.06 ms a heightfield and 0.025 a trunk
//! (`docs/perf/streaming-2026-10-04.md`, cost 3). Counted in bodies, a
//! flight that finished nine regions and a hundred trees in one frame paid
//! 6 to 9 ms for them, and an unload of thirty regions 6 ms of despawns. So
//! the collector spends a [`ColliderBudget::time`] a frame, measured with
//! the hooks inside it because it spawns and despawns in the world
//! directly. The bodies of unloaded regions wait in a queue that it drains
//! first, for at most [`ColliderBudget::despawn_time`] of that and
//! [`ColliderBudget::despawns`] bodies a frame: a retired body far from everything does no harm for a
//! few frames. A despawn runs avian's remove hooks and costs 0.02 to 0.04
//! ms, so a count alone let 64 of them take 2.4 ms in a traced flight.
//!
//! Finished regions within [`ColliderBudget::reach`] of a character go
//! first, nearest it first, and the rest nearest an observer, ties by
//! coordinate, so the order is the same for the same history. The first
//! group is also above the budget: such a region gets its ground and every
//! trunk the frame its task finishes, so a character never walks into a region
//! whose bodies are pending or half built. A character over a region whose
//! task has not finished yet holds still, which is the character module's
//! rule and unchanged. The server runs this module as it is: its characters
//! are its players.
//!
//! The collider entity is separate from the region entity: parry centres a
//! heightfield on its origin while a region's `WorldPosition` is its
//! corner, and the streamer despawns the region entity on unload while this
//! module wants to despawn the collider itself, keyed by the region. It
//! carries a `Transform` because avian's collider hook reads the entity's
//! `GlobalTransform` scale on insert and scales the shape to zero without
//! one; the transform's translation is the sim's to write, as everywhere.

use core::time::Duration;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use strata_core::platform::time::Instant;

use avian3d::prelude::{Collider, Position, RigidBody};
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemState;
use bevy::math::Vec3;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::components::Transform;
use strata_core::{GRID_N, RegionCoord, WorldPos};
use strata_sim::{Observers, Registries, RenderOrigin, WorldPosition, WorldSeed, WorldSourceRes};
use strata_stream::{RegionLoad, RegionUnload};
use strata_worldgen::{Sample, WorldSource};
use tracing::warn;

use crate::character::CharacterController;
use crate::flora::{FloraPlacement, ShapeCache, TrunkBody, build_trunk, place_trunks, spawn_trunk};

/// The LOD whose regions get a collider: the finest, one vertex per metre.
pub const COLLIDER_LOD: u8 = 0;

/// How much of each frame the colliders may take on the main thread, and
/// how close to a character the budget stops applying. A test or a tool
/// inserts its own; without one the defaults hold.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ColliderBudget {
    /// Main-thread time a frame for the collector, despawns and spawns
    /// together, avian's hooks included. At least one body is spawned a
    /// frame whatever it says, so a zero budget still makes progress.
    pub time: Duration,
    /// The share of [`Self::time`] the despawns of unloaded regions may
    /// take, first, so spawning always keeps the rest. At least one a
    /// frame. A flight retires about 300 bodies a second, 16 a frame at the
    /// Xvfb frame rate, which half a millisecond covers untraced.
    pub despawn_time: Duration,
    /// The most bodies despawned a frame, whatever the clock says.
    pub despawns: usize,
    /// Metres from a character to a region's square within which the region
    /// is built in full the frame its task finishes, budget or not. Far more
    /// than a character covers between two frames, so no trunk near one is
    /// ever still pending.
    pub reach: f64,
}

impl Default for ColliderBudget {
    fn default() -> Self {
        Self {
            time: Duration::from_micros(1_250),
            despawn_time: Duration::from_micros(500),
            despawns: 64,
            reach: 48.0,
        }
    }
}

/// The heights of one region, ready to become a collider.
#[derive(Debug, Clone, PartialEq)]
pub struct Heightfield {
    /// Which region.
    pub coord: RegionCoord,
    /// `heights[ix][iz]` is the height at grid point `(ix, iz)`: the outer
    /// index runs along `x`, the inner along `z`, which is the order
    /// [`Collider::heightfield`] reads rows and columns in.
    pub heights: Vec<Vec<f32>>,
}

impl Heightfield {
    /// Samples `coord`'s grid from `source`. `scratch` is reused across
    /// calls on the same task.
    #[must_use]
    pub fn sample(coord: RegionCoord, source: &dyn WorldSource, scratch: &mut Vec<Sample>) -> Self {
        let min = coord.min_corner();
        let step = coord.vertex_spacing();
        source.sample_grid(min.x(), min.z(), step, GRID_N, scratch);
        debug_assert_eq!(scratch.len(), GRID_N * GRID_N);
        let heights = (0..GRID_N)
            .map(|ix| {
                (0..GRID_N)
                    .map(|iz| scratch[iz * GRID_N + ix].height)
                    .collect()
            })
            .collect();
        Self { coord, heights }
    }

    /// The height at grid point `(ix, iz)`.
    #[must_use]
    pub fn height_at(&self, ix: usize, iz: usize) -> f32 {
        self.heights[ix][iz]
    }

    /// The world position of the collider: the region's horizontal centre
    /// at height zero, since the heights are absolute.
    #[must_use]
    pub fn center(&self) -> WorldPos {
        self.coord.center()
    }

    /// The avian collider, spanning the region's square with the heights in
    /// metres.
    #[must_use]
    pub fn collider(&self) -> Collider {
        self.clone().into_collider()
    }

    /// [`Self::collider`] without copying the heights, for the task that
    /// sampled them.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "region sizes are powers of two under 32 km, exact in f32"
    )]
    pub fn into_collider(self) -> Collider {
        let size = self.coord.size() as f32;
        Collider::heightfield(self.heights, Vec3::new(size, 1.0, size))
    }
}

/// A terrain collider entity: the static body under one lod 0 region.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainCollider {
    /// Which region.
    pub coord: RegionCoord,
    /// The streamer's region entity, which is gone by the time this one is.
    pub region: Entity,
}

/// Counters for the HUD and the tests.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainColliderStats {
    /// Sampling tasks plus finished regions with bodies still to spawn.
    pub in_flight: usize,
    /// Colliders that exist right now.
    pub loaded: usize,
    /// Colliders built since the start.
    pub built: u64,
    /// Tasks dropped by an unload before they finished.
    pub cancelled: u64,
    /// Tree trunks that exist right now.
    pub trees: usize,
    /// Finished regions with bodies the budget has not spawned yet.
    pub waiting: usize,
    /// Bodies of unloaded regions still waiting for their despawn.
    pub retiring: usize,
}

/// What a region's task produces: the ground and the trunks on it, every
/// shape already built.
pub(crate) struct RegionBodies {
    coord: RegionCoord,
    ground: Collider,
    trees: Vec<TrunkBody>,
}

/// A finished region whose bodies are being spawned: the ground until it
/// is, then the trunks in placement order.
pub(crate) struct Ready {
    coord: RegionCoord,
    ground: Option<Collider>,
    trees: std::vec::IntoIter<TrunkBody>,
}

impl Ready {
    fn is_done(&self) -> bool {
        self.ground.is_none() && self.trees.len() == 0
    }
}

/// The colliders' work in progress. Dropping a task cancels it.
#[derive(Resource, Default)]
pub(crate) struct ColliderTasks {
    /// Sampling tasks in flight, by region entity.
    running: HashMap<Entity, Task<RegionBodies>>,
    /// Finished regions with bodies left to spawn, by region entity.
    ready: HashMap<Entity, Ready>,
    /// Bodies of unloaded regions, oldest first, waiting for a despawn.
    retiring: VecDeque<Entity>,
    /// The hull triangulations of the current species table.
    shapes: Option<Arc<ShapeCache>>,
}

impl ColliderTasks {
    /// Moves every finished task's output to [`Self::ready`]. Only tasks
    /// that say they are finished are polled, so this never waits.
    fn take_finished(&mut self) {
        let finished: Vec<Entity> = self
            .running
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(region, _)| *region)
            .collect();
        for region in finished {
            let Some(mut task) = self.running.remove(&region) else {
                continue;
            };
            let Some(RegionBodies {
                coord,
                ground,
                trees,
            }) = block_on(poll_once(&mut task))
            else {
                continue;
            };
            self.ready.insert(
                region,
                Ready {
                    coord,
                    ground: Some(ground),
                    trees: trees.into_iter(),
                },
            );
        }
    }
}

/// The collider entities of every region that has them, by region entity:
/// the ground first, then the trunks.
#[derive(Resource, Default)]
pub(crate) struct ColliderEntities(HashMap<Entity, Vec<Entity>>);

/// [`TerrainColliderSet::Dispatch`](crate::TerrainColliderSet::Dispatch):
/// a task per lod 0 load; per unload a drop and the bodies queued for the
/// collector to despawn.
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the messages, as in the terrain crate"
)]
pub(crate) fn dispatch_colliders(
    mut commands: Commands,
    source: Option<Res<WorldSourceRes>>,
    registries: Option<Res<Registries>>,
    library: Option<Res<strata_sim::TreeLibraryRes>>,
    flora: Option<Res<FloraPlacement>>,
    exclusions: Option<Res<strata_sim::footprints::VegetationExclusions>>,
    planted: Option<Res<strata_sim::footprints::PlantedFlora>>,
    seed: Option<Res<WorldSeed>>,
    mut tasks: ResMut<ColliderTasks>,
    mut colliders: ResMut<ColliderEntities>,
    mut stats: ResMut<TerrainColliderStats>,
    mut loads: MessageReader<RegionLoad>,
    mut unloads: MessageReader<RegionUnload>,
    mut warned: Local<bool>,
) {
    let tasks = &mut *tasks;
    for RegionUnload(_, region) in unloads.read() {
        let unbuilt = tasks
            .ready
            .remove(region)
            .is_some_and(|ready| ready.ground.is_some());
        if tasks.running.remove(region).is_some() || unbuilt {
            stats.cancelled += 1;
        }
        if let Some(bodies) = colliders.0.remove(region) {
            tasks.retiring.extend(bodies);
        }
    }
    stats.retiring = tasks.retiring.len();
    stats.loaded = colliders.0.len();
    let Some(source) = source else {
        // Not read, so the loads wait for the source within Bevy's message
        // window, the same as the terrain crate.
        if !*warned && !loads.is_empty() {
            *warned = true;
            warn!("regions are wanted but there is no WorldSourceRes; no terrain colliders");
        }
        return;
    };
    // The species table on first use, from the frozen registries; a table
    // inserted by hand (a test) is kept.
    let flora = match (flora, registries) {
        (Some(flora), _) => Some(flora.0.clone()),
        (None, Some(registries)) => {
            let placement = FloraPlacement::from_registries(&registries, library.as_deref());
            let table = placement.0.clone();
            commands.insert_resource(placement);
            Some(table)
        }
        (None, None) => None,
    };
    // One hull cache per table, kept across frames so every region's
    // plants share it.
    let shapes = flora.as_ref().map(|table| match &tasks.shapes {
        Some(shapes) if shapes.is_for(table) => shapes.clone(),
        _ => {
            let shapes = Arc::new(ShapeCache::new(table.clone()));
            tasks.shapes = Some(shapes.clone());
            shapes
        }
    });
    let seed = seed.map_or(strata_core::Seed(0), |s| s.0);
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
    for RegionLoad(coord, region) in loads.read() {
        if coord.lod != COLLIDER_LOD {
            continue;
        }
        let source = source.0.clone();
        let flora = flora.clone();
        let shapes = shapes.clone();
        let exclusions = exclusions.as_deref().cloned().unwrap_or_default();
        let planted = planted
            .as_deref()
            .map(strata_sim::footprints::PlantedFlora::plants);
        let coord = *coord;
        let task = pool.spawn(async move {
            let mut scratch = Vec::new();
            let field = Heightfield::sample(coord, source.as_ref(), &mut scratch);
            let mut trees = flora.as_ref().map_or_else(Vec::new, |flora| {
                place_trunks(coord, &scratch, source.as_ref(), flora, seed)
            });
            trees.retain(|tree| !exclusions.contains(tree.base.x(), tree.base.z()));
            // A settlement's own trees collide as the foliage draws them.
            if let (Some(flora), Some(planted)) = (&flora, &planted) {
                let plants = strata_flora::planted_in(coord, flora, planted);
                trees.extend(crate::flora::trunks_of(coord, &plants, flora));
            }
            let trees = shapes.map_or_else(Vec::new, |shapes| {
                trees
                    .iter()
                    .filter_map(|tree| build_trunk(coord, tree, &shapes))
                    .collect()
            });
            RegionBodies {
                coord,
                ground: field.into_collider(),
                trees,
            }
        });
        // A region loaded again before its old task finished starts over.
        tasks.ready.remove(region);
        tasks.running.insert(*region, task);
    }
    stats.in_flight = tasks.running.len() + tasks.ready.len();
}

/// Horizontal metres from the nearest of `points` to `coord`'s square, zero
/// inside it, infinite with no points.
fn distance_to(coord: RegionCoord, points: &[WorldPos]) -> f64 {
    let min = coord.min_corner();
    let size = coord.size();
    points
        .iter()
        .map(|p| {
            let dx = (min.x() - p.x()).max(p.x() - (min.x() + size)).max(0.0);
            let dz = (min.z() - p.z()).max(p.z() - (min.z() + size)).max(0.0);
            (dx * dx + dz * dz).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

/// [`TerrainColliderSet::Collect`](crate::TerrainColliderSet::Collect):
/// retired bodies despawned within their budget, then finished tasks become
/// static bodies, those within reach of a character first and whatever the
/// budget, then nearest an observer within it.
///
/// Exclusive, so that its clocks include avian's insert and remove hooks:
/// through commands they would run after it, unmeasured, in the next sync
/// point.
pub(crate) fn collect_colliders(
    world: &mut World,
    characters: &mut SystemState<Query<&WorldPosition, With<CharacterController>>>,
) {
    let budget = world
        .get_resource::<ColliderBudget>()
        .copied()
        .unwrap_or_default();
    // A query never fails validation, so an error means no characters.
    let characters: Vec<WorldPos> = characters
        .get(world)
        .map(|query| query.iter().map(|p| p.0).collect())
        .unwrap_or_default();
    world.resource_scope(|world, mut tasks: Mut<ColliderTasks>| {
        world.resource_scope(|world, mut colliders: Mut<ColliderEntities>| {
            collect(world, &budget, &characters, &mut tasks, &mut colliders.0);
        });
    });
}

/// The body of [`collect_colliders`], with the two resources taken out of
/// the world so it can spawn into it.
fn collect(
    world: &mut World,
    budget: &ColliderBudget,
    characters: &[WorldPos],
    tasks: &mut ColliderTasks,
    colliders: &mut HashMap<Entity, Vec<Entity>>,
) {
    // One clock for both, so a frame that retires a region and builds
    // another still stays near the budget.
    let started = Instant::now();
    for despawned in 0..budget.despawns {
        if despawned > 0 && started.elapsed() >= budget.despawn_time {
            break;
        }
        let Some(body) = tasks.retiring.pop_front() else {
            break;
        };
        // Only this module despawns these, but a test or a tool may clear
        // the world under it.
        if let Ok(entity) = world.get_entity_mut(body) {
            entity.despawn();
        }
    }

    tasks.take_finished();

    let mut built = 0;
    if !tasks.ready.is_empty() {
        let observers = world
            .get_resource::<Observers>()
            .map(|o| o.positions.clone())
            .unwrap_or_default();
        // Within reach of a character first, nearest it first; then the
        // rest nearest an observer, which is what the camera sees next.
        let mut order: Vec<(bool, f64, f64, RegionCoord, Entity)> = tasks
            .ready
            .iter()
            .map(|(region, ready)| {
                let to_character = distance_to(ready.coord, characters);
                let to_observer = distance_to(ready.coord, &observers);
                if to_character <= budget.reach {
                    (true, to_character, to_observer, ready.coord, *region)
                } else {
                    (false, to_observer, to_character, ready.coord, *region)
                }
            })
            .collect();
        order.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.total_cmp(&b.2))
                .then(a.3.cmp(&b.3))
                .then(a.4.cmp(&b.4))
        });
        let origin = *world.resource::<RenderOrigin>();
        let mut spawned = false;
        let mut done = Vec::new();
        'regions: for (within_reach, _, _, _, region) in order {
            let Some(ready) = tasks.ready.get_mut(&region) else {
                continue;
            };
            loop {
                if ready.is_done() {
                    done.push(region);
                    break;
                }
                // Sorted with the regions within reach first, so once one
                // is out of reach every later one is too.
                if !within_reach && spawned && started.elapsed() >= budget.time {
                    break 'regions;
                }
                if let Some(ground) = ready.ground.take() {
                    let center = ready.coord.center();
                    let body = world
                        .spawn((
                            TerrainCollider {
                                coord: ready.coord,
                                region,
                            },
                            RigidBody::Static,
                            ground,
                            WorldPosition(center),
                            Position(origin.to_render(center)),
                            Transform::default(),
                        ))
                        .id();
                    if let Some(stale) = colliders.insert(region, vec![body]) {
                        tasks.retiring.extend(stale);
                    }
                    built += 1;
                } else if let Some(tree) = ready.trees.next() {
                    let body = spawn_trunk(world, &origin, tree);
                    colliders.entry(region).or_default().push(body);
                }
                spawned = true;
            }
        }
        for region in done {
            tasks.ready.remove(&region);
        }
    }

    let mut stats = world.resource_mut::<TerrainColliderStats>();
    stats.built += built;
    stats.in_flight = tasks.running.len() + tasks.ready.len();
    stats.waiting = tasks.ready.len();
    stats.retiring = tasks.retiring.len();
    stats.loaded = colliders.len();
    stats.trees = colliders.values().map(|b| b.len().saturating_sub(1)).sum();
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::BiomeId;

    /// A ramp along `x`: height is a tenth of `x`, whatever `z` is.
    struct Ramp;

    impl WorldSource for Ramp {
        #[allow(clippy::cast_possible_truncation, reason = "test heights are small")]
        fn sample(&self, x: f64, _z: f64) -> Sample {
            Sample {
                height: (x * 0.1) as f32,
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

    /// The collector alone, with nothing in flight, under `budget`.
    fn collector(budget: ColliderBudget) -> bevy::app::App {
        let mut app = bevy::app::App::new();
        app.init_resource::<ColliderTasks>()
            .init_resource::<ColliderEntities>()
            .init_resource::<TerrainColliderStats>()
            .init_resource::<RenderOrigin>()
            .init_resource::<Observers>()
            .insert_resource(budget)
            .add_message::<RegionLoad>()
            .add_message::<RegionUnload>()
            .add_systems(
                bevy::app::Update,
                (dispatch_colliders, collect_colliders).chain(),
            );
        app
    }

    /// A finished region at `coord` with `trees` trunks, as a task leaves
    /// it, under a fresh region entity.
    fn finished(app: &mut bevy::app::App, coord: RegionCoord, trees: usize) -> Entity {
        let region = app.world_mut().spawn_empty().id();
        let corner = coord.min_corner();
        let trunk = TrunkBody {
            marker: crate::flora::TreeCollider {
                id: strata_flora::PlantId {
                    species_key: 1,
                    cell: coord,
                    slot: [0, 0],
                },
                cell: coord,
                species: strata_registry::FloraId(0),
            },
            collider: Collider::cylinder(0.5, 8.0),
            center: corner + strata_core::glam::DVec3::new(10.0, 4.0, 10.0),
            rotation: bevy::math::Quat::IDENTITY,
        };
        let field = Heightfield {
            coord,
            heights: vec![vec![0.0; GRID_N]; GRID_N],
        };
        app.world_mut()
            .resource_mut::<ColliderTasks>()
            .ready
            .insert(
                region,
                Ready {
                    coord,
                    ground: Some(field.into_collider()),
                    trees: vec![trunk; trees].into_iter(),
                },
            );
        region
    }

    fn bodies(app: &bevy::app::App, region: Entity) -> usize {
        app.world()
            .resource::<ColliderEntities>()
            .0
            .get(&region)
            .map_or(0, Vec::len)
    }

    fn character_at(app: &mut bevy::app::App, x: f64, z: f64) {
        app.world_mut().spawn((
            CharacterController::default(),
            WorldPosition(WorldPos::new(x, 0.0, z)),
        ));
    }

    #[test]
    fn a_spent_budget_still_spawns_one_body_a_frame() {
        let mut app = collector(ColliderBudget {
            time: Duration::ZERO,
            ..ColliderBudget::default()
        });
        let region = finished(&mut app, RegionCoord::new(10, 10, 0), 3);
        for frame in 1..=4 {
            app.update();
            assert_eq!(bodies(&app, region), frame, "frame {frame}");
        }
        app.update();
        let stats = *app.world().resource::<TerrainColliderStats>();
        assert_eq!((stats.loaded, stats.trees, stats.built), (1, 3, 1));
        assert_eq!((stats.in_flight, stats.waiting), (0, 0));
    }

    #[test]
    fn regions_near_a_character_go_first_and_whole() {
        let mut app = collector(ColliderBudget {
            time: Duration::ZERO,
            ..ColliderBudget::default()
        });
        // The observer is over the far corner; the character, which wins,
        // is over the near one.
        app.world_mut().resource_mut::<Observers>().positions =
            vec![WorldPos::new(5_000.0, 0.0, 5_000.0)];
        let watched = finished(&mut app, RegionCoord::new(78, 78, 0), 40);
        let near = finished(&mut app, RegionCoord::new(1, 0, 0), 300);
        let beside = finished(&mut app, RegionCoord::new(0, 0, 0), 300);
        let far = finished(&mut app, RegionCoord::new(-40, 3, 0), 40);
        // In the region at (0, 0), 20 m from the one east of it.
        character_at(&mut app, 44.0, 30.0);
        app.update();
        // Both regions within reach in full, whatever the budget; nothing
        // else, since the budget is spent.
        assert_eq!(bodies(&app, beside), 301);
        assert_eq!(bodies(&app, near), 301);
        assert_eq!(bodies(&app, watched), 0);
        assert_eq!(bodies(&app, far), 0);
        assert_eq!(app.world().resource::<TerrainColliderStats>().waiting, 2);
        // Then the observer's region before the one nearer nothing.
        app.update();
        assert_eq!((bodies(&app, watched), bodies(&app, far)), (1, 0));
    }

    #[test]
    fn a_character_reaching_a_half_built_region_finishes_it() {
        let mut app = collector(ColliderBudget {
            time: Duration::ZERO,
            ..ColliderBudget::default()
        });
        let region = finished(&mut app, RegionCoord::new(3, 0, 0), 200);
        app.update();
        app.update();
        assert_eq!(bodies(&app, region), 2, "ground and one trunk");
        // Within reach of the region's west edge at x = 192.
        character_at(&mut app, 150.0, 20.0);
        app.update();
        assert_eq!(bodies(&app, region), 201);
        assert_eq!(app.world().resource::<TerrainColliderStats>().waiting, 0);
    }

    /// Every collider entity, ground and trunks.
    fn count(app: &mut bevy::app::App) -> usize {
        app.world_mut()
            .query::<&crate::flora::TreeCollider>()
            .iter(app.world())
            .count()
            + app
                .world_mut()
                .query::<&TerrainCollider>()
                .iter(app.world())
                .count()
    }

    /// A region of 150 bodies, built in one frame by a character beside
    /// it, then unloaded.
    fn built_and_unloaded(budget: ColliderBudget) -> bevy::app::App {
        let mut app = collector(budget);
        let region = finished(&mut app, RegionCoord::new(0, 0, 0), 149);
        character_at(&mut app, 30.0, 30.0);
        app.update();
        assert_eq!(bodies(&app, region), 150);
        assert_eq!(count(&mut app), 150);
        app.world_mut()
            .write_message(RegionUnload(RegionCoord::new(0, 0, 0), region));
        app
    }

    #[test]
    fn unloads_despawn_at_most_the_count_a_frame() {
        let mut app = built_and_unloaded(ColliderBudget {
            despawn_time: Duration::from_secs(1),
            ..ColliderBudget::default()
        });
        app.update();
        assert_eq!(count(&mut app), 150 - 64);
        let stats = *app.world().resource::<TerrainColliderStats>();
        assert_eq!((stats.loaded, stats.retiring), (0, 150 - 64));
        app.update();
        app.update();
        assert_eq!(count(&mut app), 0);
        assert_eq!(app.world().resource::<TerrainColliderStats>().retiring, 0);
    }

    #[test]
    fn a_spent_despawn_budget_still_despawns_one_body_a_frame() {
        let mut app = built_and_unloaded(ColliderBudget {
            despawn_time: Duration::ZERO,
            ..ColliderBudget::default()
        });
        for frame in 1..=3 {
            app.update();
            assert_eq!(count(&mut app), 150 - frame, "frame {frame}");
        }
    }

    #[test]
    fn an_unload_drops_a_region_still_waiting_for_the_budget() {
        let mut app = collector(ColliderBudget {
            time: Duration::ZERO,
            ..ColliderBudget::default()
        });
        let first = finished(&mut app, RegionCoord::new(0, 0, 0), 0);
        let second = finished(&mut app, RegionCoord::new(5, 0, 0), 10);
        app.update();
        assert_eq!((bodies(&app, first), bodies(&app, second)), (1, 0));
        app.world_mut()
            .write_message(RegionUnload(RegionCoord::new(5, 0, 0), second));
        app.update();
        app.update();
        assert_eq!(bodies(&app, second), 0);
        let stats = *app.world().resource::<TerrainColliderStats>();
        assert_eq!((stats.in_flight, stats.cancelled), (0, 1));
    }

    #[test]
    fn heights_are_indexed_x_then_z_and_match_the_samples() {
        let coord = RegionCoord::new(2, -1, 0);
        let field = Heightfield::sample(coord, &Ramp, &mut Vec::new());
        assert_eq!(field.heights.len(), GRID_N);
        let min = coord.min_corner();
        for ix in [0, 1, 17, 64] {
            for iz in [0, 30, 64] {
                #[allow(clippy::cast_precision_loss, reason = "grid indices")]
                let expected = Ramp.sample(min.x() + ix as f64, min.z() + iz as f64).height;
                assert_eq!(field.height_at(ix, iz).to_bits(), expected.to_bits());
            }
        }
        assert!(field.height_at(64, 0) > field.height_at(0, 0));
        assert!((field.height_at(0, 64) - field.height_at(0, 0)).abs() < 1e-6);
    }
}
