//! Trees, logs and rocks as colliders: a static body per placed plant with
//! a collider on every lod 0 region, from the same placement the client
//! draws (ADR 0005). A trunk is an upright cylinder, a log a capsule, a rock
//! its baked convex hulls turned by the plant's yaw.
//!
//! The heightfield task already samples the region's grid; the same
//! samples go through [`place_on_grid_with_source`] with the grass left out, and
//! every tree trunk becomes a body next to the ground. Neighbor terrain
//! supplies the same exclusion halo as rendering.
//! Nothing about a tree crosses the wire: a client places the same tree
//! from the same seed.
//!
//! Every shape is built on the region's task ([`build_trunk`]), so the main
//! thread only spawns the body. A rock's or a cliff's hulls are the costly
//! part, and their triangulation depends only on the species and the
//! variant, so [`ShapeCache`] keeps it at scale one and each placed plant
//! scales the cached vertices. Its own scale makes every plant's shape
//! distinct, so it is the triangulation that is shared, not the parry shape:
//! avian's insert hook resets a collider's scale to its `GlobalTransform`'s,
//! which would undo a scale carried on a shared shape.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use avian3d::parry::shape::SharedShape;
use avian3d::prelude::{Collider, Position, RigidBody, Rotation};
use bevy::ecs::prelude::*;
use bevy::math::{Quat, Vec3};
use bevy::transform::components::Transform;
use strata_core::{RegionCoord, Seed, WorldPos};
use strata_flora::{FloraTable, HeightGrid, PlantId, place_on_grid_with_source};
use strata_registry::FloraId;
use strata_sim::{Registries, RenderOrigin, TreeLibraryRes, WorldPosition};
use strata_worldgen::{Sample, WorldSource};

/// The species table placement reads, shared by every task. Built from the
/// frozen registries on first use; a test inserts its own.
#[derive(Resource, Clone)]
pub struct FloraPlacement(pub Arc<FloraTable>);

impl FloraPlacement {
    /// The table for the frozen registries and, when the world has one, the
    /// baked tree library: its variants and trunks are what the client's
    /// meshes were grown with. Dressing is left out: it has no collider,
    /// and every other plant stands where it would with it.
    #[must_use]
    pub fn from_registries(registries: &Registries, library: Option<&TreeLibraryRes>) -> Self {
        Self(Arc::new(
            FloraTable::from_registries_and_library(
                &registries.0,
                library.and_then(|library| library.0.as_deref()),
            )
            .without_dressing(),
        ))
    }
}

/// One placed plant's collider, as the task hands it to the collector.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeBody {
    /// Stable procedural identity.
    pub id: PlantId,
    /// The species.
    pub species: FloraId,
    /// The plant's root, world.
    pub base: WorldPos,
    /// Its collider at its scale, in its own frame before its yaw.
    pub shape: strata_flora::Collider,
    /// Radians about the vertical.
    pub yaw: f32,
    /// Which of the species' variants, so a baked hull is looked up in the
    /// [`ShapeCache`] instead of being wrapped again.
    pub variant: u8,
    /// The plant's scale, which [`Self::shape`] already carries; the cache
    /// holds scale one and applies it.
    pub scale: f32,
}

impl TreeBody {
    /// The trunk's `(radius, height)`, for a body that is one.
    #[must_use]
    pub fn trunk(&self) -> Option<(f32, f32)> {
        self.shape.trunk().map(|t| (t.radius, t.height))
    }
}

/// A tree collider entity: the static body of one trunk.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TreeCollider {
    /// Stable procedural identity.
    pub id: PlantId,
    /// The cell the tree stands in.
    pub cell: RegionCoord,
    /// The species.
    pub species: FloraId,
}

/// The trunks of a lod 0 region, from its height samples. `samples` is the
/// region's grid as the heightfield task filled it.
#[must_use]
pub fn place_trunks(
    coord: RegionCoord,
    samples: &[Sample],
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
) -> Vec<TreeBody> {
    if flora.is_empty() || coord.lod != 0 {
        return Vec::new();
    }
    let grid = HeightGrid::new(samples);
    let cell = place_on_grid_with_source(coord, &grid, source, flora, seed, false);
    trunks_of(coord, &cell.plants, flora)
}

/// The bodies of `plants`, placed in cell `coord`: a placed or a planted
/// plant alike.
#[must_use]
pub fn trunks_of(
    coord: RegionCoord,
    plants: &[strata_flora::Plant],
    flora: &FloraTable,
) -> Vec<TreeBody> {
    let corner = coord.min_corner();
    plants
        .iter()
        .filter_map(|plant| {
            let species = flora.species().get(plant.species.0 as usize)?;
            let shape = species.collider(plant)?;
            Some(TreeBody {
                id: plant.id,
                species: plant.species,
                base: WorldPos::new(
                    corner.x() + f64::from(plant.x),
                    f64::from(plant.y),
                    corner.z() + f64::from(plant.z),
                ),
                shape,
                yaw: plant.yaw * core::f32::consts::TAU,
                variant: plant.variant,
                scale: plant.scale,
            })
        })
        .collect()
}

/// One convex hull as parry's quickhull leaves it: the hull's own vertices,
/// at scale one, and its triangles.
///
/// Quickhull returns the input points it keeps as they are, and a uniform
/// scale does not change which points those are, so these vertices times a
/// plant's scale are what wrapping the scaled points gives, bit for bit.
/// Rounding could in principle tip a near-degenerate cloud the other way;
/// `cached_hulls_are_the_hulls_of_the_scaled_points` checks every baked
/// hull at a spread of scales.
/// [`SharedShape::convex_mesh`] then builds the same polyhedron from them
/// without the hull search.
#[derive(Debug)]
struct HullMesh {
    vertices: Vec<Vec3>,
    indices: Vec<[u32; 3]>,
}

/// The hulls of one (species, variant) at scale one, `None` where a part is
/// too degenerate to wrap, as [`Collider::convex_hull`] would say.
type VariantHulls = Arc<[Option<HullMesh>]>;

/// The baked hulls of each (species, variant), triangulated once on first
/// use and shared by every task and every plant of it.
///
/// Filled lazily on the pool, under a lock held only to look up or insert:
/// two tasks that miss the same key both triangulate and the second insert
/// is dropped, which costs one hull and keeps the lock off the slow path.
#[derive(Debug)]
pub struct ShapeCache {
    table: Arc<FloraTable>,
    hulls: Mutex<HashMap<(FloraId, usize), VariantHulls>>,
}

impl ShapeCache {
    /// An empty cache over `table`, whose species' baked colliders it reads.
    #[must_use]
    pub fn new(table: Arc<FloraTable>) -> Self {
        Self {
            table,
            hulls: Mutex::new(HashMap::new()),
        }
    }

    /// Whether this cache was built over `table`, so a replaced table gets
    /// a fresh one.
    #[must_use]
    pub fn is_for(&self, table: &Arc<FloraTable>) -> bool {
        Arc::ptr_eq(&self.table, table)
    }

    /// The scale-one hulls of `tree`'s species and variant, or `None` when
    /// its species has no baked hulls for that variant.
    fn hulls(&self, tree: &TreeBody) -> Option<VariantHulls> {
        let species = self.table.species().get(tree.species.0 as usize)?;
        if species.colliders.is_empty() {
            return None;
        }
        let slot = usize::from(tree.variant) % species.colliders.len();
        let strata_flora::Collider::Hulls(parts) = &species.colliders[slot] else {
            return None;
        };
        let key = (tree.species, slot);
        if let Some(hulls) = self
            .hulls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Some(hulls.clone());
        }
        let built: VariantHulls = parts
            .iter()
            .map(|part| {
                let points: Vec<Vec3> = part.iter().map(|p| Vec3::from_array(*p)).collect();
                avian3d::parry::transformation::try_convex_hull(&points)
                    .ok()
                    .map(|(vertices, indices)| HullMesh { vertices, indices })
            })
            .collect();
        self.hulls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert_with(|| built.clone());
        Some(built)
    }
}

/// A plant's static body, built on the task and spawned by the collector.
#[derive(Debug, Clone)]
pub struct TrunkBody {
    /// Its marker.
    pub marker: TreeCollider,
    /// Its shape, ready to insert.
    pub collider: Collider,
    /// Where the body stands, world.
    pub center: WorldPos,
    /// Its yaw.
    pub rotation: Quat,
}

/// The body of one placed plant, or `None` for a shape that makes no
/// collider (a hull too degenerate to wrap). Runs on the region's task.
///
/// A cylinder is centred on its own middle, so its body stands half its
/// height above the root; every other shape is in the plant's frame, so its
/// body stands at the root, turned by the yaw.
#[must_use]
pub fn build_trunk(cell: RegionCoord, tree: &TreeBody, shapes: &ShapeCache) -> Option<TrunkBody> {
    let v = |p: &[f32; 3]| Vec3::from_array(*p);
    let (collider, lift) = match &tree.shape {
        strata_flora::Collider::None => return None,
        strata_flora::Collider::Cylinder(trunk) => (
            Collider::cylinder(trunk.radius, trunk.height),
            f64::from(trunk.height) * 0.5,
        ),
        strata_flora::Collider::Capsule { a, b, radius } => {
            (Collider::capsule_endpoints(*radius, v(a), v(b)), 0.0)
        }
        strata_flora::Collider::Hulls(hulls) => {
            let parts: Vec<_> = if let Some(cached) = shapes.hulls(tree) {
                cached
                    .iter()
                    .filter_map(|hull| {
                        let hull = hull.as_ref()?;
                        let scaled = hull.vertices.iter().map(|p| *p * tree.scale).collect();
                        SharedShape::convex_mesh(scaled, &hull.indices).map(Collider::from)
                    })
                    .map(|hull| (Vec3::ZERO, Quat::IDENTITY, hull))
                    .collect()
            } else {
                hulls
                    .iter()
                    .filter_map(|hull| Collider::convex_hull(hull.iter().map(v).collect()))
                    .map(|hull| (Vec3::ZERO, Quat::IDENTITY, hull))
                    .collect()
            };
            if parts.is_empty() {
                return None;
            }
            (Collider::compound(parts), 0.0)
        }
    };
    Some(TrunkBody {
        marker: TreeCollider {
            id: tree.id,
            cell,
            species: tree.species,
        },
        collider,
        center: tree.base + strata_core::glam::DVec3::new(0.0, lift, 0.0),
        rotation: Quat::from_rotation_y(tree.yaw),
    })
}

/// Spawns a built plant body into `world`. Spawned directly rather than
/// through commands so the collector's clock sees avian's insert hooks,
/// which are most of a body's cost.
pub fn spawn_trunk(world: &mut World, origin: &RenderOrigin, body: TrunkBody) -> Entity {
    world
        .spawn((
            body.marker,
            RigidBody::Static,
            body.collider,
            WorldPosition(body.center),
            Position(origin.to_render(body.center)),
            Rotation(body.rotation),
            Transform::default(),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The polyhedron's vertices and face normals, bit for bit.
    fn signature(collider: &Collider) -> Vec<Vec<(u32, u32, u32)>> {
        let bits = |v: Vec3| (v.x.to_bits(), v.y.to_bits(), v.z.to_bits());
        let compound = collider.shape().as_compound().expect("a compound");
        compound
            .shapes()
            .iter()
            .map(|(_, part)| {
                let hull = part.as_convex_polyhedron().expect("a hull");
                hull.points()
                    .iter()
                    .copied()
                    .chain(hull.faces().iter().map(|f| f.normal))
                    .map(bits)
                    .collect()
            })
            .collect()
    }

    /// The direct path: every part wrapped from its scaled points, as the
    /// collector did before the cache.
    fn direct(tree: &TreeBody) -> Option<Collider> {
        let strata_flora::Collider::Hulls(hulls) = &tree.shape else {
            return None;
        };
        let parts: Vec<_> = hulls
            .iter()
            .filter_map(|hull| {
                Collider::convex_hull(hull.iter().map(|p| Vec3::from_array(*p)).collect())
            })
            .map(|hull| (Vec3::ZERO, Quat::IDENTITY, hull))
            .collect();
        (!parts.is_empty()).then(|| Collider::compound(parts))
    }

    fn table_with(colliders: Vec<strata_flora::Collider>) -> FloraTable {
        use strata_flora::Species;
        use strata_registry::{FloraFit, FloraKind};
        FloraTable::from_species(vec![Species {
            id: FloraId(0),
            kind: FloraKind::Rock,
            fit: FloraFit::from_weights(vec![1.0]),
            spacing_m: 10.0,
            density_per_m2: 0.0,
            slope_max: 1.0,
            altitude_m: (-100.0, 10_000.0),
            sea_clearance_m: 0.5,
            moisture: (0.0, 1.0),
            cover_floor: 1.0,
            size: (0.5, 3.0),
            exclusion_m: 3.0,
            #[allow(clippy::cast_possible_truncation, reason = "a handful of variants")]
            variants: colliders.len() as u8,
            tree: strata_registry::TreeParams::default(),
            species_seed: 7,
            colliders,
            ground: strata_registry::GroundPrefs::default(),
            cluster: strata_registry::Clustering::default(),
            children: Vec::new(),
            dressing: false,
        }])
    }

    /// Lumpy point clouds, a few dozen points each, from a fixed hash.
    fn rocks() -> Vec<strata_flora::Collider> {
        let mut state = 0x9e37_79b9_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            #[allow(clippy::cast_precision_loss, reason = "a unit float from a hash")]
            let unit = (state >> 8) as f32 / (1u32 << 24) as f32;
            unit
        };
        (0..6)
            .map(|_| {
                strata_flora::Collider::Hulls(
                    (0..3)
                        .map(|_| {
                            (0..40)
                                .map(|_| [next() * 2.0 - 1.0, next() * 1.5, next() * 2.0 - 1.0])
                                .collect()
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn tree_of(table: &FloraTable, variant: u8, scale: f32) -> TreeBody {
        let species = &table.species()[0];
        let shape = species.colliders[usize::from(variant) % species.colliders.len()].scaled(scale);
        TreeBody {
            id: PlantId {
                species_key: 0,
                cell: RegionCoord::new(0, 0, 0),
                slot: [0, 0],
            },
            species: FloraId(0),
            base: WorldPos::new(1.0, 2.0, 3.0),
            shape,
            yaw: 0.5,
            variant,
            scale,
        }
    }

    /// The baked library's hulls when this checkout has run `just content`,
    /// else none: the synthetic rocks below always run.
    fn library_hulls() -> Vec<strata_flora::Collider> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generated");
        let Ok(library) = strata_flora::TreeLibrary::read(&root) else {
            return Vec::new();
        };
        library
            .species
            .values()
            .flat_map(|species| species.variants.iter().map(|v| v.collider.clone()))
            .filter(|c| matches!(c, strata_flora::Collider::Hulls(_)))
            .collect()
    }

    #[test]
    fn cached_hulls_are_the_hulls_of_the_scaled_points() {
        let mut sets = vec![rocks()];
        let baked = library_hulls();
        if !baked.is_empty() {
            sets.push(baked);
        }
        let mut compared = 0;
        for colliders in sets {
            let variants = colliders.len();
            let table = Arc::new(table_with(colliders));
            let shapes = ShapeCache::new(table.clone());
            for variant in 0..variants {
                #[allow(clippy::cast_possible_truncation, reason = "a handful of variants")]
                let variant = variant as u8;
                for scale in [0.5, 0.731, 1.0, 1.37, 2.9] {
                    let tree = tree_of(&table, variant, scale);
                    let cached = build_trunk(RegionCoord::new(0, 0, 0), &tree, &shapes)
                        .map(|body| signature(&body.collider));
                    let direct = direct(&tree).map(|c| signature(&c));
                    assert_eq!(cached, direct, "variant {variant} at scale {scale}");
                    compared += 1;
                }
            }
        }
        assert!(compared >= 30, "{compared}");
    }

    #[test]
    fn cylinders_and_capsules_are_built_as_before() {
        let table = Arc::new(table_with(vec![
            strata_flora::Collider::Cylinder(strata_flora::Trunk {
                radius: 0.45,
                height: 4.8,
            }),
            strata_flora::Collider::Capsule {
                a: [0.0, 0.2, -1.0],
                b: [0.0, 0.2, 1.0],
                radius: 0.3,
            },
        ]));
        let shapes = ShapeCache::new(table.clone());
        let cylinder = build_trunk(RegionCoord::new(0, 0, 0), &tree_of(&table, 0, 1.3), &shapes)
            .expect("a trunk");
        let c = cylinder.collider.shape().as_cylinder().expect("a cylinder");
        assert_eq!(c.radius.to_bits(), (0.45f32 * 1.3).to_bits());
        assert_eq!(c.half_height.to_bits(), ((4.8f32 * 1.3) * 0.5).to_bits());
        assert!((cylinder.center.y() - (2.0 + f64::from(4.8f32 * 1.3) * 0.5)).abs() < 1e-9);
        let capsule = build_trunk(RegionCoord::new(0, 0, 0), &tree_of(&table, 1, 2.0), &shapes)
            .expect("a log");
        assert!(capsule.collider.shape().as_capsule().is_some());
        assert_eq!(capsule.center, WorldPos::new(1.0, 2.0, 3.0));
    }
}
