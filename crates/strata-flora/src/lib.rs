//! Vegetation placement: the plants of one 64 m cell as a pure function of
//! the world, the species registry and the seed (ADR 0005).
//!
//! The server calls [`place_cell`] to put a collider under every tree and
//! the client calls it to draw the tree, and the two agree bit for bit
//! because nothing in here depends on the caller: randomness is an integer
//! hash of the seed, the cell and the slot, the ground is the world's own
//! 65 x 65 height sample of the cell (the grid the collider samples), and
//! the arithmetic is `f32` adds, multiplies and square roots, which are
//! correctly rounded everywhere. No platform transcendental is called.
//!
//! Big things first: trees, then bushes, each species on its own jittered
//! grid at its spacing, a slot kept when its hash falls under the species'
//! fit at that point (biome weights by the sample's biome shares, moisture,
//! slope, altitude, the sea, and the ground's cover) and no candidate
//! competing at a higher stable priority is within the larger exclusion
//! radius. A bounded neighboring-cell halo makes the decisions independent
//! of streaming order. Grass last, per
//! species, at its density thinned by the ground's hydration, kept clear of
//! trunks, as a compact record the client uploads as it is.

pub mod library;
pub mod tree;

pub use library::{Collider, TreeLibrary, TreeVariant, Trunk};

use serde::{Deserialize, Serialize};
use strata_core::{RegionCoord, Seed, mix64};
use strata_data::flora::{
    MAX_EXCLUSION_M, MAX_GRASS_BLADES_PER_CELL, MAX_GRASS_CANDIDATES_PER_CELL,
    MAX_PLANT_CANDIDATES_PER_CELL,
};
use strata_data::{Clustering, FloraFit, FloraId, FloraKind, GroundPrefs, TreeParams, WorldDefs};
use strata_worldgen::hydrology::WaterKind;
use strata_worldgen::{Sample, SampleWater, WorldSource};

/// Samples per side of a cell's height grid: one per metre plus the far
/// edge. The same grid the heightfield collider samples.
pub const GRID: usize = 65;

/// The cell size placement is defined on. Placement is per lod 0 region.
pub const CELL_M: f32 = strata_data::flora::FOLIAGE_CELL_M;

/// Stable identity of a procedural candidate, independent of dense registry IDs
/// and render entity lifetime. Persist together with the world's seed/version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlantId {
    /// Hash of the namespaced species identity.
    pub species_key: u64,
    /// Owning lod-zero cell.
    pub cell: RegionCoord,
    /// Candidate lattice coordinate within the cell, or for a child of
    /// another plant a hash of its parent with the top bit set, and its
    /// index among its siblings.
    pub slot: [u32; 2],
}

impl PlantId {
    /// Whether this plant was brought by a parent rather than placed on its
    /// species' grid.
    #[must_use]
    pub fn is_child(&self) -> bool {
        self.slot[0] & 0x8000_0000 != 0
    }
}

/// One placed bush or tree, cell local.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plant {
    /// Identity for persistence, interaction and replication.
    pub id: PlantId,
    /// The species.
    pub species: FloraId,
    /// Metres east of the cell's corner, `0..64`.
    pub x: f32,
    /// Ground height at the root, metres.
    pub y: f32,
    /// Metres north of the cell's corner, `0..64`.
    pub z: f32,
    /// Rotation about the vertical, in turns `[0, 1)`; the client makes an
    /// angle of it, the collider is round and does not care.
    pub yaw: f32,
    /// Size multiplier, within the species' range.
    pub scale: f32,
    /// Which of the species' generated variants.
    pub variant: u8,
    /// How much of the sky the plant sees past its neighbours' crowns, in
    /// 255ths: 255 in the open (lighting plan phase 3a, [`sky_terms`]). The
    /// client scales the sky's fill on the plant by it; nothing else reads
    /// it, and a record without it loads as open.
    #[serde(default = "open_sky")]
    pub sky: u8,
}

/// A plant under no crown but its own sees the whole sky.
fn open_sky() -> u8 {
    u8::MAX
}

/// One grass blade, cell local, 16 bytes, laid out for a GPU buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(C)]
pub struct GrassBlade {
    /// Metres east of the cell's corner as a fraction of the cell in
    /// `u16`: a millimetre.
    pub x: u16,
    /// Metres north, likewise.
    pub z: u16,
    /// Ground height at the root, metres, as `f32` bits (the struct stays
    /// plain bytes; the client reinterprets).
    pub y_bits: u32,
    /// Blade height in centimetres.
    pub height_cm: u8,
    /// Wind phase, `0..256` for one turn.
    pub phase: u8,
    /// How much wind the blade resists, `0..256` for `0..1`.
    pub stiffness: u8,
    /// Grass-only material palette index, independent of dense flora IDs,
    /// in the low five bits ([`palette`](Self::palette)); in the high three
    /// how deep under crowns the blade stands ([`crown_sky`](Self::crown_sky),
    /// lighting plan phase 3a), 0 in the open, so a blade under no tree
    /// keeps the byte it always had.
    pub species: u8,
    /// Where between the species' dry and lush tints, `0..256`.
    pub lush: u8,
    /// Per-blade brightness jitter, `0..256` for `0.75..1.25`.
    pub shade: u8,
    /// Ground normal `x` component, `-127..=127` for `-1..1`.
    pub nx: i8,
    /// Ground normal `z` component, likewise; `y` is rebuilt as positive.
    pub nz: i8,
}

impl GrassBlade {
    /// Bits of [`species`](Self::species) the palette index takes; the
    /// palette holds [`MAX_GRASS_SPECIES`](strata_data::flora::MAX_GRASS_SPECIES).
    pub const PALETTE_BITS: u32 = 5;

    /// Crown shade levels above the open: the high three bits.
    pub const CROWN_LEVELS: u8 = 7;

    /// The grass-only palette index.
    #[must_use]
    pub fn palette(&self) -> u8 {
        self.species & ((1 << Self::PALETTE_BITS) - 1)
    }

    /// The share of the sky the blade sees past the crowns over it, from
    /// one in the open down to [`SKY_FLOOR`].
    #[must_use]
    pub fn crown_sky(&self) -> f32 {
        let level = self.species >> Self::PALETTE_BITS;
        1.0 - f32::from(level) / f32::from(Self::CROWN_LEVELS) * (1.0 - SKY_FLOOR)
    }

    /// The root, metres from the cell's corner.
    #[must_use]
    pub fn root(&self) -> [f32; 3] {
        [
            f32::from(self.x) * (CELL_M / 65535.0),
            f32::from_bits(self.y_bits),
            f32::from(self.z) * (CELL_M / 65535.0),
        ]
    }
}

/// The plants of one cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellFlora {
    /// The cell.
    pub coord: RegionCoord,
    /// Bushes and trees, in placement order (trees first).
    pub plants: Vec<Plant>,
    /// Grass blades, in placement order.
    pub grass: Vec<GrassBlade>,
}

impl CellFlora {
    /// The trees alone.
    pub fn trees<'a>(&'a self, flora: &'a FloraTable) -> impl Iterator<Item = &'a Plant> + 'a {
        self.plants
            .iter()
            .filter(move |p| flora.kind(p.species) == Some(FloraKind::Tree))
    }
}

/// One species as placement reads it: the frozen def's numbers and its fit,
/// copied out of the registry so a task holds no registry borrow.
#[derive(Debug, Clone, PartialEq)]
pub struct Species {
    /// The id.
    pub id: FloraId,
    /// The kind.
    pub kind: FloraKind,
    /// Where it grows.
    pub fit: FloraFit,
    /// Metres between candidate slots (bushes and trees).
    pub spacing_m: f32,
    /// Blades per square metre (grass).
    pub density_per_m2: f32,
    /// Rise over run limit.
    pub slope_max: f32,
    /// Altitude window, metres.
    pub altitude_m: (f32, f32),
    /// Metres above the sea.
    pub sea_clearance_m: f32,
    /// Moisture window.
    pub moisture: (f32, f32),
    /// The share of its chance the species keeps where `cover` is zero.
    pub cover_floor: f32,
    /// Size range.
    pub size: (f32, f32),
    /// Exclusion radius, metres.
    pub exclusion_m: f32,
    /// Generated variants (bushes and trees).
    pub variants: u8,
    /// The generator's parameters (bushes and trees), absent for grass.
    pub tree: TreeParams,
    /// The seed the species' variants are generated from: a hash of its
    /// namespaced identity, so adding unrelated species preserves its shapes.
    pub species_seed: u64,
    /// The baked variants' colliders at scale one, from the library, in
    /// variant order. Empty for a species the library does not hold, whose
    /// trunk is then derived from its parameters as it was before trees were
    /// baked (tests with hand-built tables, a world with no content step).
    pub colliders: Vec<Collider>,
    /// Where on the ground's shape it prefers to stand.
    pub ground: GroundPrefs,
    /// How it gathers into clumps and leaves clearings.
    pub cluster: Clustering,
    /// What a placed one brings with it.
    pub children: Vec<Child>,
    /// The client's alone (`FloraDef::dressing`): placed after and below
    /// every other species, so it never moves one, and left out entirely
    /// by a table [`FloraTable::without_dressing`].
    pub dressing: bool,
}

/// A child rule with its species resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Child {
    /// The child's species.
    pub species: FloraId,
    /// Fewest and most.
    pub count: [u8; 2],
    /// Nearest and furthest from the parent's root, metres.
    pub radius_m: [f32; 2],
}

impl Species {
    /// The collider of a placed plant of this species at its scale, in its
    /// own frame before its yaw, or `None` for one walked through. A baked
    /// species takes the library's; a tree the library does not hold gets a
    /// trunk from its parameters.
    #[must_use]
    pub fn collider(&self, plant: &Plant) -> Option<Collider> {
        if !self.colliders.is_empty() {
            let collider = &self.colliders[usize::from(plant.variant) % self.colliders.len()];
            return (*collider != Collider::None).then(|| collider.scaled(plant.scale));
        }
        if self.kind != FloraKind::Tree || self.tree.is_absent() {
            return None;
        }
        let height = tree::variant_height(&self.tree, self.species_seed, plant.variant);
        let (radius, height) = tree::trunk_collider(&self.tree, height, plant.scale);
        Some(Collider::Cylinder(Trunk { radius, height }))
    }

    /// The trunk collider of a placed tree, `(radius, height)` in metres, or
    /// `None` for anything that is not an upright cylinder.
    #[must_use]
    pub fn trunk(&self, plant: &Plant) -> Option<(f32, f32)> {
        self.collider(plant)?
            .trunk()
            .map(|trunk| (trunk.radius, trunk.height))
    }
}

/// Every species, in id order, ready for placement. Built once from the
/// frozen registries and shared by every task.
#[derive(Debug, Clone, Default)]
pub struct FloraTable {
    species: Vec<Species>,
    /// Places no dressing species: a server's table, or a client's
    /// physics', whose plants are the same as a full table's but for them.
    skip_dressing: bool,
    /// Which table this is, for the [`CandidateCache`]: a clone keeps it, a
    /// table built or changed afterwards takes a new one, so a memo of one
    /// table's candidates is never read for another's.
    serial: u64,
}

impl PartialEq for FloraTable {
    fn eq(&self, other: &Self) -> bool {
        self.species == other.species && self.skip_dressing == other.skip_dressing
    }
}

/// The next table identity; zero stays the default table's.
fn next_serial() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl FloraTable {
    /// The table for a frozen registry set, with no baked trees: every trunk
    /// derived from its species' parameters.
    #[must_use]
    pub fn from_registries(registries: &WorldDefs) -> Self {
        Self::from_registries_and_library(registries, None)
    }

    /// The table for a frozen registry set and the baked tree library: a
    /// species the library holds takes its variant count and trunks from it,
    /// so placement picks only variants that were baked.
    #[must_use]
    pub fn from_registries_and_library(
        registries: &WorldDefs,
        library: Option<&TreeLibrary>,
    ) -> Self {
        let species = registries
            .flora_ids()
            .filter_map(|id| {
                let def = registries.flora.get(id)?;
                let fit = registries.flora_fit(id)?.clone();
                Some(Species {
                    id,
                    kind: def.kind,
                    fit,
                    spacing_m: def.spacing_m,
                    density_per_m2: def.density_per_m2,
                    slope_max: def.slope_max,
                    altitude_m: (def.altitude_m.min, def.altitude_m.max),
                    sea_clearance_m: def.sea_clearance_m,
                    moisture: (def.moisture.min, def.moisture.max),
                    cover_floor: def.cover_floor,
                    size: (def.size.min, def.size.max),
                    exclusion_m: def.exclusion_m,
                    variants: if def.tree.is_absent() {
                        1
                    } else {
                        def.tree.variants
                    },
                    tree: def.tree.clone(),
                    species_seed: Seed(0).derive(def.id.as_ref()),
                    colliders: Vec::new(),
                    ground: def.ground,
                    cluster: def.cluster,
                    children: def
                        .children
                        .iter()
                        .filter_map(|child| {
                            Some(Child {
                                species: registries.flora.id_of(&child.species)?,
                                count: child.count,
                                radius_m: child.radius_m,
                            })
                        })
                        .collect(),
                    dressing: def.dressing,
                })
                .map(|mut species| {
                    let baked = library.and_then(|library| {
                        library.species.get(registries.flora.get(id)?.id.as_ref())
                    });
                    if let Some(entry) = baked.filter(|entry| !entry.variants.is_empty()) {
                        species.colliders =
                            entry.variants.iter().map(|v| v.collider.clone()).collect();
                        // Variant counts are bounded by the registry's 1..=16.
                        #[allow(clippy::cast_possible_truncation)]
                        {
                            species.variants = entry.variants.len().min(16) as u8;
                        }
                    }
                    species
                })
            })
            .collect();
        Self {
            species,
            skip_dressing: false,
            serial: next_serial(),
        }
    }

    /// A table from species built by hand, for tests.
    #[must_use]
    pub fn from_species(species: Vec<Species>) -> Self {
        Self {
            species,
            skip_dressing: false,
            serial: next_serial(),
        }
    }

    /// The same table placing no dressing species ([`Species::dressing`]):
    /// what a server collides with is every other plant, and those stand
    /// where a full table puts them, since a dressing species takes its
    /// slots last, thins below everything and crowds out no child. Species
    /// ids are unchanged, so a plant's species still indexes the table.
    #[must_use]
    pub fn without_dressing(mut self) -> Self {
        self.skip_dressing = true;
        self.serial = next_serial();
        self
    }

    /// Whether the table places `species`: every one but a dressing one in
    /// a table [`without_dressing`](Self::without_dressing).
    fn places(&self, species: &Species) -> bool {
        !(self.skip_dressing && species.dressing)
    }

    /// Whether a far cell's candidates include `species`: everything placed
    /// but grass and dressing. A bush outranks nothing a tree is thinned by,
    /// but the impostor-bearing scrub is a bush and is thinned by every other
    /// bush, so leaving any out would move it. Dressing ranks below everything
    /// and takes the budget last, so leaving it out moves nothing else.
    fn is_far_species(species: &Species) -> bool {
        !species.dressing && species.kind != FloraKind::Grass
    }

    /// Whether [`place_cell_trees`] gives exactly a full placement's plants
    /// but for the children, the dressing and the grass. It does while no
    /// dressing species is a tree (it would shade its neighbours and be
    /// missing) and no species raises a tree as a child (the same, placed
    /// only by the full path). The shipped data meets both.
    fn trees_only_is_exact(&self) -> bool {
        self.species.iter().all(|s| {
            (!s.dressing || s.kind != FloraKind::Tree)
                && s.children.iter().all(|c| {
                    self.species
                        .get(c.species.0 as usize)
                        .is_none_or(|child| child.kind != FloraKind::Tree)
                })
        })
    }

    /// Whether the species of `id` is dressing.
    fn is_dressing(&self, id: FloraId) -> bool {
        self.species.get(id.0 as usize).is_some_and(|s| s.dressing)
    }

    /// The reach of exclusion between plants, metres: the widest exclusion
    /// of any placed species but grass and dressing, so the halo and the
    /// thinning's bins are the same with dressing or without. A dressing
    /// plant's exclusion is capped at it (unless only dressing grows), so
    /// it stays as local as everything else.
    fn exclusion_reach(&self) -> f32 {
        let widest = |dressing: bool| {
            self.species
                .iter()
                .filter(|s| s.kind != FloraKind::Grass && s.dressing == dressing)
                .map(|s| s.exclusion_m.clamp(0.0, MAX_EXCLUSION_M))
                .fold(0.0_f32, f32::max)
        };
        let reach = widest(false);
        if reach > 0.0 { reach } else { widest(true) }
    }

    /// Every species, in id order.
    #[must_use]
    pub fn species(&self) -> &[Species] {
        &self.species
    }

    /// The kind of a species, or `None` for an id from another set.
    #[must_use]
    pub fn kind(&self, id: FloraId) -> Option<FloraKind> {
        self.species.get(id.0 as usize).map(|s| s.kind)
    }

    /// Whether nothing grows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.species.is_empty()
    }
}

/// A unit float in `[0, 1)` from the seed, a cell and a slot. 24 bits, so
/// the result is exact in `f32` and the same on every platform.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "24 bits fit an f32 mantissa exactly"
)]
pub fn hash_unit(cell_seed: u64, i: i64, j: i64, salt: u64) -> f32 {
    // Reinterpreting the slot indices as bit patterns is the intent.
    #[allow(clippy::cast_sign_loss)]
    let h = mix64(
        cell_seed
            ^ mix64((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
            ^ mix64((j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
            ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9),
    );
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The seed of one cell.
#[must_use]
#[allow(
    clippy::cast_sign_loss,
    reason = "the coordinates are hashed as bit patterns"
)]
pub fn cell_seed(seed: Seed, coord: RegionCoord) -> u64 {
    mix64(seed.derive("flora") ^ mix64(coord.x as u64) ^ mix64((coord.z as u64) << 20))
}

/// The world's 65 x 65 height sample of a cell, interpolated on the same
/// `(i + 1, j)` to `(i, j + 1)` diagonal as terrain and collision.
pub struct HeightGrid<'a> {
    backing: Backing<'a>,
}

/// Where a grid's nodes come from.
enum Backing<'a> {
    /// All of them, sampled up front.
    Samples(&'a [Sample]),
    /// Each when first read.
    Lazy(LazyNodes<'a>),
}

/// A grid that samples a node the first time anything reads it. A far
/// cell's trees probe a third of the 4,225 nodes, so the rest are never
/// paid for.
struct LazyNodes<'a> {
    source: &'a dyn WorldSource,
    origin: [f64; 2],
    nodes: Vec<std::cell::OnceCell<Sample>>,
}

impl<'a> HeightGrid<'a> {
    /// Over `samples` as [`WorldSource::sample_grid`] fills them for
    /// [`GRID`] points per side from the cell's corner at one metre.
    ///
    /// # Panics
    ///
    /// If there are not exactly `GRID * GRID` samples.
    #[must_use]
    pub fn new(samples: &'a [Sample]) -> Self {
        assert_eq!(samples.len(), GRID * GRID, "a cell is GRID x GRID samples");
        Self {
            backing: Backing::Samples(samples),
        }
    }

    /// The grid of `coord` that samples `source` on first read. A node is
    /// the same bits [`sample`](Self::sample) fills it with, since a source
    /// must return from `sample_grid` exactly what `sample` would.
    #[must_use]
    pub fn lazy(coord: RegionCoord, source: &'a dyn WorldSource) -> Self {
        let min = coord.min_corner();
        let mut nodes = Vec::new();
        nodes.resize_with(GRID * GRID, std::cell::OnceCell::new);
        Self {
            backing: Backing::Lazy(LazyNodes {
                source,
                origin: [min.x(), min.z()],
                nodes,
            }),
        }
    }

    /// Fills `scratch` with the cell's samples and wraps it.
    pub fn sample(
        coord: RegionCoord,
        source: &dyn WorldSource,
        scratch: &'a mut Vec<Sample>,
    ) -> Self {
        let min = coord.min_corner();
        source.sample_grid(min.x(), min.z(), 1.0, GRID, scratch);
        Self::new(scratch)
    }

    #[allow(clippy::cast_precision_loss, reason = "grid indices are at most 64")]
    fn at(&self, i: usize, j: usize) -> &Sample {
        let (i, j) = (i.min(GRID - 1), j.min(GRID - 1));
        match &self.backing {
            Backing::Samples(samples) => &samples[j * GRID + i],
            Backing::Lazy(lazy) => lazy.nodes[j * GRID + i].get_or_init(|| {
                lazy.source
                    .sample(lazy.origin[0] + i as f64, lazy.origin[1] + j as f64)
            }),
        }
    }

    /// Height, normal and the nearest sample at cell-local `x, z`.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "cell-local metres are small and clamped before the cast"
    )]
    pub fn probe(&self, x: f32, z: f32) -> Probe<'_> {
        let fi = x.clamp(0.0, CELL_M - 1e-3);
        let fj = z.clamp(0.0, CELL_M - 1e-3);
        let (i, j) = (fi.floor() as usize, fj.floor() as usize);
        let (fx, fz) = (fi - i as f32, fj - j as f32);
        let h00 = self.at(i, j).height;
        let h10 = self.at(i + 1, j).height;
        let h01 = self.at(i, j + 1).height;
        let h11 = self.at(i + 1, j + 1).height;
        let (height, dx, dz) = if fx + fz <= 1.0 {
            (
                h00 + (h10 - h00) * fx + (h01 - h00) * fz,
                h10 - h00,
                h01 - h00,
            )
        } else {
            (
                h11 + (h01 - h11) * (1.0 - fx) + (h10 - h11) * (1.0 - fz),
                h11 - h01,
                h11 - h10,
            )
        };
        let len = (dx * dx + dz * dz + 1.0).sqrt();
        let nearest = self.at(
            if fx < 0.5 { i } else { i + 1 },
            if fz < 0.5 { j } else { j + 1 },
        );
        Probe {
            height,
            normal: [-dx / len, 1.0 / len, -dz / len],
            slope: (dx * dx + dz * dz).sqrt(),
            sample: nearest,
        }
    }
}

/// What the ground is at one point of a cell.
#[derive(Debug, Clone, Copy)]
pub struct Probe<'a> {
    /// Metres.
    pub height: f32,
    /// Unit, `y` up.
    pub normal: [f32; 3],
    /// Rise over run.
    pub slope: f32,
    /// The nearest world sample, for biomes and moisture.
    pub sample: &'a Sample,
}

/// How much of its chance a species keeps at a point's `cover`, the
/// world's fertility with its patchiness (ADR 0006).
///
/// ADR 0006 asks for `cover.powf(k)` with `k` 1.5 for trees, 1 for bushes
/// and 0.4 for grass, so a clearing empties of trees before it empties of
/// grass. `powf` is a platform transcendental and this crate calls none,
/// so the three curves are built from `sqrt`, which is correctly rounded
/// everywhere: `c * sqrt(c)` is exactly `c^1.5`, `c` is itself, and the
/// three-term fit below tracks `c^0.4` to seven thousandths over the whole
/// unit interval, monotonically and from zero to one, which is finer than
/// a hash threshold can tell.
///
/// The species' `cover_floor` is the share it keeps where the ground
/// carries nothing, mixed in rather than taken as a maximum so the curve
/// stays monotonic and still reaches one at full cover.
#[must_use]
pub fn cover_gain(kind: FloraKind, cover: f32, cover_floor: f32) -> f32 {
    let c = cover.clamp(0.0, 1.0);
    let root = c.sqrt();
    let curve = match kind {
        // A rock is where the ground put it, fertile or not.
        FloraKind::Rock => 1.0,
        FloraKind::Tree => c * root,
        FloraKind::Bush => c,
        FloraKind::Grass => {
            let quarter = root.sqrt();
            0.98 * root + 0.22 * quarter - 0.2 * root * quarter
        }
    };
    let floor = cover_floor.clamp(0.0, 1.0);
    floor + (1.0 - floor) * curve
}

/// A metre lattice caches world-continuous grass fields. Candidate blades
/// only interpolate four values instead of hashing noise per blade.
struct GrassField {
    values: Vec<[f32; 3]>,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "world lattice indices use f64; local grid indices are bounded to 64"
)]
impl GrassField {
    fn new(coord: RegionCoord, seed: u64) -> Self {
        let min = coord.min_corner();
        let mut values = Vec::with_capacity(GRID * GRID);
        for z in 0..GRID {
            for x in 0..GRID {
                let wx = min.x() + x as f64;
                let wz = min.z() + z as f64;
                values.push([
                    0.65 * grass_noise(seed, wx / 24.0, wz / 24.0, 1)
                        + 0.35 * grass_noise(seed, wx / 7.0, wz / 7.0, 2),
                    grass_noise(seed, wx / 2.0, wz / 2.0, 3),
                    grass_noise(seed, wx / 2.0, wz / 2.0, 4),
                ]);
            }
        }
        Self { values }
    }

    fn probe(&self, x: f32, z: f32) -> [f32; 3] {
        let x = x.clamp(0.0, CELL_M - 1e-3);
        let z = z.clamp(0.0, CELL_M - 1e-3);
        let i = x.floor() as usize;
        let j = z.floor() as usize;
        let fx = x - i as f32;
        let fz = z - j as f32;
        std::array::from_fn(|k| {
            let v00 = self.values[j * GRID + i][k];
            let v10 = self.values[j * GRID + i + 1][k];
            let v01 = self.values[(j + 1) * GRID + i][k];
            let v11 = self.values[(j + 1) * GRID + i + 1][k];
            (v00 + (v10 - v00) * fx) * (1.0 - fz) + (v01 + (v11 - v01) * fx) * fz
        })
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "world coordinates are f64; fractional noise is intentionally f32"
)]
fn grass_noise(seed: u64, x: f64, z: f64, salt: u64) -> f32 {
    let i = x.floor() as i64;
    let j = z.floor() as i64;
    let smooth = |v: f32| v * v * (3.0 - 2.0 * v);
    let fx = smooth((x - x.floor()) as f32);
    let fz = smooth((z - z.floor()) as f32);
    let v00 = hash_unit(seed, i, j, salt);
    let v10 = hash_unit(seed, i + 1, j, salt);
    let v01 = hash_unit(seed, i, j + 1, salt);
    let v11 = hash_unit(seed, i + 1, j + 1, salt);
    (v00 + (v10 - v00) * fx) * (1.0 - fz) + (v01 + (v11 - v01) * fx) * fz
}

/// Each exclusion disk is entered in all overlapping 4 m bins, so grass
/// candidates inspect one small bin even for unusually large modded trees.
struct GrassExclusions {
    bins: Vec<Vec<[f32; 3]>>,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bin indices are clamped to the 16 by 16 cell lattice"
)]
impl GrassExclusions {
    fn bin(v: f32) -> usize {
        (v / 4.0).floor().clamp(0.0, 15.0) as usize
    }

    fn new(plants: &[Plant], flora: &FloraTable) -> Self {
        let mut bins = vec![Vec::new(); 16 * 16];
        for p in plants {
            let radius = flora.species()[p.species.0 as usize].exclusion_m * 0.5;
            for z in Self::bin(p.z - radius)..=Self::bin(p.z + radius) {
                for x in Self::bin(p.x - radius)..=Self::bin(p.x + radius) {
                    bins[z * 16 + x].push([p.x, p.z, radius * radius]);
                }
            }
        }
        Self { bins }
    }

    fn contains(&self, x: f32, z: f32) -> bool {
        self.bins[Self::bin(z) * 16 + Self::bin(x)].iter().any(|p| {
            let dx = x - p[0];
            let dz = z - p[1];
            dx * dx + dz * dz < p[2]
        })
    }
}

/// The species' chance at a point: its biome weights by the sample's biome
/// shares, for a rock some province's kit names by [`province_gain`] too,
/// scaled by the ground's cover and, for grass, its hydration, and zero
/// outside its moisture, altitude, slope and sea limits.
///
/// Grass takes its density from hydration here, on the threshold and not
/// on the grid, so the candidate slots of a cell stay the same wherever
/// the ground is wet: only how many survive changes.
#[must_use]
pub fn fit_at(species: &Species, probe: &Probe<'_>, sea_level: f32) -> f32 {
    // A settlement's platform and road hold nothing that grows or lies
    // there by nature, on the server as on the client.
    if probe.sample.landform.developed > 0.5 {
        return 0.0;
    }
    // A road's way, its carriageway and its verge, holds grass and nothing
    // taller (ticket 0005, step 6): the verge is kept clear either side.
    if probe.sample.landform.cleared > 0.5 && species.kind != FloraKind::Grass {
        return 0.0;
    }
    // Nothing stands in a river or a lake, and what keeps clear of the sea
    // keeps as clear of their surface (ticket 0005, step 2), on the server
    // as on the client: both read it from the sample.
    if probe.sample.in_water()
        || probe
            .sample
            .water
            .is_some_and(|water| probe.height < water.surface_m + species.sea_clearance_m)
    {
        return 0.0;
    }
    // A plant of the wet bank stands by slow water's edge and nowhere else;
    // everything else that grows keeps off the wetted bed the terrain lays
    // in gravel or sand there (ticket 0005, step 8). A stone may lie on it,
    // as stones do.
    let mut bank_gain = 1.0;
    if species.ground.bank_m > 0.0 {
        let Some(water) = probe
            .sample
            .water
            .filter(|water| on_bank(water, probe.height, &species.ground))
        else {
            return 0.0;
        };
        // Thickest at the water's edge, thinning up the bank.
        bank_gain = 1.0 - BANK_THINNING * (probe.height - water.surface_m) / species.ground.bank_m;
    } else if species.kind != FloraKind::Rock
        && probe.sample.water.is_some_and(|water| {
            water.edge_reaches() && probe.height < water.surface_m + SampleWater::WETTED_BED_M
        })
    {
        return 0.0;
    }
    // A wall a river's valley or a road cut into the hillside, or a road's
    // fill, is made ground: smooth, with none of the ledges a cliff piece or
    // a root holds to, so a cliff kit's piece on it reads as stuck to the
    // face. Where the carve or a road moved the ground, the slope, the
    // sample's own and the triangle's, is held to the walkable limit
    // whatever the species allows; a natural cliff the cut left standing
    // keeps its kit. Every plant keeps its own slope limit there, all under
    // this one, so a cut bare of plants is one too steep to hold them, and a
    // gentle one is planted as the land round it is (ticket 0005, step 6).
    let carved = probe
        .sample
        .water
        .filter(|water| water.kind != WaterKind::Lake)
        .map_or(0.0, |water| water.cut_m());
    if carved.max(probe.sample.landform.cut_m.abs()) > CARVED_WALL_CUT_M
        && probe.slope.max(probe.sample.landform.slope) > CARVED_WALL_SLOPE
    {
        return 0.0;
    }
    if probe.slope > species.slope_max
        || probe.height < sea_level + species.sea_clearance_m
        || probe.height < species.altitude_m.0
        || probe.height > species.altitude_m.1
        || probe.sample.moisture < species.moisture.0
        || probe.sample.moisture > species.moisture.1
    {
        return 0.0;
    }
    let mut base: f32 = probe
        .sample
        .biomes
        .iter()
        .map(|(biome, share)| species.fit.weight(*biome) * share)
        .sum();
    // The biome says how rocky the climate is; the province says which rock.
    // Only a species some kit names is scaled, so every tree and grass keeps
    // its chance to the bit (ticket 0004, step 3e).
    if species.fit.in_a_kit() {
        base *= province_gain(&species.fit, probe.sample);
    }
    let mut chance = base * cover_gain(species.kind, probe.sample.cover, species.cover_floor);
    if species.kind == FloraKind::Grass {
        chance *= 0.4 + 0.6 * probe.sample.hydration.clamp(0.0, 1.0);
    }
    chance * ground_gain(&species.ground, probe) * bank_gain
}

/// How much of its chance a kit species keeps at a sample: its kits'
/// weights by the sample's two province shares, so a boulder field thins
/// across a border into rock whose kit does not list it.
///
/// A share that names no province (a source with no province field, whose
/// samples carry [`Sample::NO_PROVINCES`]) leaves the choice to the biome
/// alone and counts as weight one, as placement was before provinces: a
/// hand-built world keeps its rocks rather than losing every one of them.
#[must_use]
pub fn province_gain(fit: &strata_data::FloraFit, sample: &Sample) -> f32 {
    sample
        .provinces
        .iter()
        .map(|&(province, share)| {
            let weight = if province.is_none() {
                1.0
            } else {
                fit.province_weight(province)
            };
            weight * share
        })
        .sum()
}

/// How much a species' chance is scaled by the ground's shape: zero below
/// its slope, more in the hollows or on the ridges it prefers.
#[must_use]
pub fn ground_gain(ground: &GroundPrefs, probe: &Probe<'_>) -> f32 {
    if probe.slope < ground.slope_min {
        return 0.0;
    }
    let landform = &probe.sample.landform;
    (1.0 + ground.hollow * landform.hollow + ground.ridge * landform.ridge).clamp(0.0, 2.0)
}

/// Rank of a kind in placement and thinning: rocks are where the ground put
/// them before anything grew, then trees, then what grows under them.
fn kind_rank(kind: FloraKind) -> u8 {
    match kind {
        FloraKind::Rock => 0,
        FloraKind::Tree => 1,
        FloraKind::Bush => 2,
        FloraKind::Grass => 3,
    }
}

/// How much of its chance a plant of the wet bank loses from the water's
/// edge to the top of its band: a reed bed is thickest in the wet.
const BANK_THINNING: f32 = 0.6;

/// Whether ground at `height` is the wet bank `ground` asks for by this
/// water: within its `bank_m` over the surface where the edge reaches, by a
/// lake or a river at least its `bank_width_min_m` wide, never a fall.
#[must_use]
pub fn on_bank(water: &SampleWater, height: f32, ground: &GroundPrefs) -> bool {
    let slow = match water.kind {
        WaterKind::Lake => true,
        WaterKind::Fall => false,
        WaterKind::Channel | WaterKind::Mouth => water.width_m >= ground.bank_width_min_m,
    };
    slow && water.edge_reaches() && height <= water.surface_m + ground.bank_m
}

/// The steepest ground a river's valley cut anything is placed on, rise
/// over run: 1.2 is the character's walkable 50 degrees. A natural cliff
/// keeps its kit's own limit; a wall the carve cut steeper is bare.
pub const CARVED_WALL_SLOPE: f32 = 1.2;

/// How far the carve has to have lowered the ground for it to be the
/// carve's wall, metres: past the half metre its ramp softens the cut's
/// shoulder over, so a natural slope the valley's edge only grazed is not.
pub const CARVED_WALL_CUT_M: f32 = 0.5;

/// Clump centres are hashed on a lattice this many metres across, in world
/// coordinates, so a clump spans cells without either knowing.
const CLUMP_LATTICE_M: f64 = 32.0;

/// How much a species' chance is scaled at a world point by its clumps and
/// clearings: a Thomas-like process without a stream, then a patch field.
///
/// Clump centres are hashed per lattice square; each lends a smooth bump
/// `(1 - d^2/r^2)^2` inside its radius. The sum is divided by its mean
/// over the plane, so the species keeps its density on average and moves
/// it into groves. The patch field then clears what falls under its cover.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "lattice indices of world metres, and a few clump slots"
)]
pub fn cluster_gain(species: &Species, seed: Seed, x: f64, z: f64) -> f32 {
    let rules = &species.cluster;
    let mut gain = 1.0_f32;
    if rules.radius_m > 0.0 && rules.clumps_per_ha > 0.0 {
        let key = mix64(seed.0 ^ species.species_seed.rotate_left(17));
        let radius = f64::from(rules.radius_m);
        let per_square =
            rules.clumps_per_ha * (CLUMP_LATTICE_M * CLUMP_LATTICE_M / 10_000.0) as f32;
        let reach = (radius / CLUMP_LATTICE_M).ceil() as i64;
        let ci = (x / CLUMP_LATTICE_M).floor() as i64;
        let cj = (z / CLUMP_LATTICE_M).floor() as i64;
        let mut density = 0.0_f32;
        for j in cj - reach..=cj + reach {
            for i in ci - reach..=ci + reach {
                for k in 0..3_u64 {
                    // Up to three centres a square: the k-th exists while the
                    // hash stays under the expected count less k.
                    if hash_unit(key, i, j, 11 + k * 7) >= per_square - k as f32 {
                        break;
                    }
                    let px =
                        (i as f64 + f64::from(hash_unit(key, i, j, 12 + k * 7))) * CLUMP_LATTICE_M;
                    let pz =
                        (j as f64 + f64::from(hash_unit(key, i, j, 13 + k * 7))) * CLUMP_LATTICE_M;
                    let d2 = ((x - px) * (x - px) + (z - pz) * (z - pz)) / (radius * radius);
                    if d2 < 1.0 {
                        let bump = (1.0 - d2) as f32;
                        density += bump * bump;
                    }
                }
            }
        }
        // The bump integrates to pi r^2 / 3; centres fall per square metre at
        // clumps_per_ha / 10 000.
        let mean = rules.clumps_per_ha / 10_000.0
            * core::f32::consts::PI
            * rules.radius_m
            * rules.radius_m
            / 3.0;
        gain = (density / mean.max(1e-6)).min(3.0);
    }
    if rules.patch_m > 0.0 {
        let noise = grass_noise(
            species.species_seed ^ seed.0,
            x / f64::from(rules.patch_m),
            z / f64::from(rules.patch_m),
            173,
        );
        let edge = 1.0 - rules.patch_cover;
        let keep = ((noise - edge + 0.08) / 0.16).clamp(0.0, 1.0);
        gain *= keep * keep * (3.0 - 2.0 * keep);
    }
    gain
}

/// A plant a settlement placed by hand (a street's tree, a garden's bush),
/// in world metres, as its layout says.
///
/// A city's trees are the wilderness's species from the same baked library,
/// drawn and collided by the same code: the plan chooses where they stand,
/// placement merges them into their cell with [`planted_in`], and the
/// foliage and the physics never know the difference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Planted {
    /// The species.
    pub species: FloraId,
    /// The root, world metres.
    pub at: [f64; 3],
    /// Rotation about the vertical, in turns.
    pub yaw: f32,
    /// Size multiplier; placement clamps it into the species' range.
    pub scale: f32,
    /// Which of the species' variants, wrapped into the baked count.
    pub variant: u8,
    /// Its index among all planted plants, for a stable identity.
    pub index: u32,
}

/// How far round a crossing a plant of the wet bank keeps clear (ticket
/// 0005, step 8): a crossing's ground is the road's, and the sample does
/// not say where a deck stands over the river, so this is read from the
/// crossing's record. `(ends, aside)`: how far past the road's ends of the
/// crossing, metres, and how far either side of its centreline past the
/// carriageway's half.
fn crossing_clear_m(kind: strata_worldgen::roads::CrossingKind) -> (f64, f64) {
    use strata_worldgen::roads::CrossingKind;
    match kind {
        // From under the soffit to past the deck's ends, which reach a
        // class's insets and bedding (under five metres) beyond the gap,
        // and past its kerbs, rails and pilasters.
        CrossingKind::Bridge => (6.0, 3.0),
        // The way, and the depth posts a stride past its edges.
        CrossingKind::Ford => (3.0, 2.0),
        // A culvert's headwalls stand out along the creek up to its road's
        // banks' reach (`strata_worldgen::crossings`), anywhere round it.
        CrossingKind::Culvert => (32.0, 32.0),
    }
}

/// Takes from `flora` every plant of the wet bank ([`GroundPrefs::bank_m`])
/// whose root stands on a crossing's ground: under a bridge's deck and
/// past its ends, on a ford's way and by its posts, on a culvert's apron.
/// Pure in its inputs: `crossings` are those near the cell, as the world
/// lists them (`ProceduralWorld::crossings_near`). Every other plant stays,
/// so a cell whose crossings are not known yet differs only in its reeds.
pub fn clear_bank_plants_off_crossings(
    flora: &mut CellFlora,
    table: &FloraTable,
    crossings: &[strata_worldgen::crossings::ProceduralCrossing],
) {
    if crossings.is_empty() {
        return;
    }
    let corner = flora.coord.min_corner();
    let on_crossing = |x: f64, z: f64| {
        crossings.iter().any(|c| {
            let (ends, aside) = crossing_clear_m(c.crossing.kind);
            let half = strata_worldgen::sites::way_in_half_m(c.class) + aside;
            let [a, b] = c.edges;
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let length = (dx * dx + dz * dz).sqrt();
            let dir = if length > 1e-6 {
                [dx / length, dz / length]
            } else {
                c.crossing.bearing
            };
            let (px, pz) = (x - a[0], z - a[1]);
            let along = px * dir[0] + pz * dir[1];
            let across = (px * dir[1] - pz * dir[0]).abs();
            along >= -ends && along <= length + ends && across <= half
        })
    };
    flora.plants.retain(|p| {
        let bank = table
            .species()
            .get(p.species.0 as usize)
            .is_some_and(|s| s.ground.bank_m > 0.0);
        !bank || !on_crossing(corner.x() + f64::from(p.x), corner.z() + f64::from(p.z))
    });
}

/// The mark in a [`PlantId`]'s slot of a planted plant: neither a grid slot
/// (which is small) nor a child (the top bit).
pub const PLANTED_SLOT: u32 = 0x4000_0000;

/// The planted plants whose roots fall in lod 0 cell `coord`, as the cell's
/// plants.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "cell-local metres are 0..64"
)]
pub fn planted_in(coord: RegionCoord, flora: &FloraTable, planted: &[Planted]) -> Vec<Plant> {
    let corner = coord.min_corner();
    let size = coord.size();
    planted
        .iter()
        .filter_map(|p| {
            let (x, z) = (p.at[0] - corner.x(), p.at[2] - corner.z());
            if !(0.0..size).contains(&x) || !(0.0..size).contains(&z) {
                return None;
            }
            let species = flora.species().get(p.species.0 as usize)?;
            Some(Plant {
                id: PlantId {
                    species_key: species.species_seed,
                    cell: coord,
                    slot: [PLANTED_SLOT | p.index, 0],
                },
                species: p.species,
                x: x as f32,
                y: p.at[1] as f32,
                z: z as f32,
                yaw: p.yaw.rem_euclid(1.0),
                scale: p.scale.clamp(species.size.0, species.size.1),
                variant: p.variant % species.variants.max(1),
                sky: u8::MAX,
            })
        })
        .collect()
}

/// The plants of `coord`, grass included.
///
/// `scratch` receives the height samples and is reused across calls on the
/// same task so a busy pool does not reallocate per cell.
#[must_use]
pub fn place_cell(
    coord: RegionCoord,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    scratch: &mut Vec<Sample>,
) -> CellFlora {
    place_cell_with(coord, source, flora, seed, scratch, true)
}

/// [`place_cell`], with the grass left out when `grass` is false: a
/// coarse region or a server wants the trees alone.
#[must_use]
pub fn place_cell_with(
    coord: RegionCoord,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    scratch: &mut Vec<Sample>,
    grass: bool,
) -> CellFlora {
    let grid = HeightGrid::sample(coord, source, scratch);
    place_on_grid_with_source(coord, &grid, source, flora, seed, grass)
}

// Sky under crowns (lighting plan phase 3a). A plant's and a blade's share
// of the sky past the crowns around it, from the crowns alone and with no
// rays: a crown is a sphere, and one a point sees at distance `d` and at an
// elevation whose sine is `s` takes `(r / d)^2 s` of its cosine-weighted
// sky, all of it from inside. Only crowns over the point and within two of
// their radii of it count. The library records a tree's height, not its
// crown, so the crown is the species' shape at the variant's height:
// the share of the height its middle stands at and its radius over the
// height, narrower for needles. The terms are adds, multiplies, divisions
// and square roots, so the server and every client get the same bytes.

// The palette must fit under the crown shade's bits.
const _: () = assert!(
    strata_data::flora::MAX_GRASS_SPECIES <= 1 << GrassBlade::PALETTE_BITS,
    "the grass palette outgrew its bits"
);

/// The least share of the sky the client lets the term leave, so nothing
/// under the densest canopy goes black and shade keeps the sky's colour.
pub const SKY_FLOOR: f32 = 0.25;

/// Where a crown's middle stands, as a share of its tree's height.
const CROWN_CENTRE: f32 = 0.65;

/// A broadleaf crown's radius over its tree's height: the third of the
/// height across the terrain's canopy colour assumes, a little under.
const BROAD_CROWN: f32 = 0.3;

/// A conifer's: narrower for its height.
const NEEDLE_CROWN: f32 = 0.2;

/// Where a tree's own term is taken, as a share of its height: its crown's
/// underside, which its neighbours shade most; the client lightens it
/// towards the crown's top, which sees past them.
const CROWN_UNDERSIDE: f32 = 0.45;

/// The 2 m raster the grass reads its crown shade from, per side.
const GRASS_SHADE_N: usize = 33;

/// A tree's crown, cell local.
#[derive(Debug, Clone, Copy)]
struct Crown {
    id: PlantId,
    centre: [f32; 3],
    radius: f32,
}

/// The height of a plant at its scale from its species' parameters, or
/// `None` for one without them.
fn plant_height(species: &Species, plant: &Plant) -> Option<f32> {
    (!species.tree.is_absent()).then(|| {
        tree::variant_height(&species.tree, species.species_seed, plant.variant) * plant.scale
    })
}

/// The crowns among `plants`: their trees'.
fn crowns(plants: &[Plant], flora: &FloraTable) -> Vec<Crown> {
    plants
        .iter()
        .filter_map(|p| {
            let species = flora.species().get(p.species.0 as usize)?;
            if species.kind != FloraKind::Tree {
                return None;
            }
            let height = plant_height(species, p)?;
            let shape = if species.tree.leaves == "needle" {
                NEEDLE_CROWN
            } else {
                BROAD_CROWN
            };
            Some(Crown {
                id: p.id,
                centre: [p.x, p.y + height * CROWN_CENTRE, p.z],
                radius: height * shape,
            })
        })
        .collect()
}

/// The share of the cosine-weighted sky `crowns` take from `point`,
/// leaving out the crown of `own`.
fn crown_shade(point: [f32; 3], crowns: &[Crown], own: Option<PlantId>) -> f32 {
    let mut lost = 0.0_f32;
    for crown in crowns {
        if Some(crown.id) == own {
            continue;
        }
        let dy = crown.centre[1] - point[1];
        if dy <= 0.0 {
            continue;
        }
        let (dx, dz) = (crown.centre[0] - point[0], crown.centre[2] - point[2]);
        let across = dx * dx + dz * dz;
        let reach = 2.0 * crown.radius;
        if across >= reach * reach {
            continue;
        }
        let d2 = across + dy * dy;
        let r2 = crown.radius * crown.radius;
        lost += r2 / d2.max(r2) * (dy / d2.sqrt());
    }
    lost
}

/// The sky left by a loss, floored, in 255ths.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 first"
)]
fn sky_byte(lost: f32) -> u8 {
    ((1.0 - lost).clamp(SKY_FLOOR, 1.0) * 255.0 + 0.5) as u8
}

/// Gives each of `plants` its [`Plant::sky`] from the crowns of
/// `neighbours` (the cell's plants and the halo's, in the cell's frame):
/// a tree's at its crown's underside, past every crown but its own, whose
/// inside the baked mesh already darkens; a bush's or a rock's halfway up.
pub fn sky_terms(plants: &mut [Plant], neighbours: &[Plant], flora: &FloraTable) {
    let crowns = crowns(neighbours, flora);
    if crowns.is_empty() {
        return;
    }
    for plant in plants {
        let Some(species) = flora.species().get(plant.species.0 as usize) else {
            continue;
        };
        let height = plant_height(species, plant).unwrap_or(1.0);
        let lift = if species.kind == FloraKind::Tree {
            CROWN_UNDERSIDE
        } else {
            0.5
        };
        let point = [plant.x, plant.y + height * lift, plant.z];
        plant.sky = sky_byte(crown_shade(point, &crowns, Some(plant.id)));
    }
}

/// The crown shade over the cell's ground every 2 m, as the level of the
/// grass record's high bits, from the crowns of `plants`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "raster indices are under 33 and the level is clamped to 0..=7"
)]
fn grass_shade(grid: &HeightGrid<'_>, plants: &[Plant], flora: &FloraTable) -> Option<Vec<u8>> {
    let crowns = crowns(plants, flora);
    if crowns.is_empty() {
        return None;
    }
    let levels = f32::from(GrassBlade::CROWN_LEVELS);
    let mut shade = Vec::with_capacity(GRASS_SHADE_N * GRASS_SHADE_N);
    for b in 0..GRASS_SHADE_N {
        for a in 0..GRASS_SHADE_N {
            let ground = grid.at(2 * a, 2 * b).height;
            let point = [2.0 * a as f32, ground, 2.0 * b as f32];
            let lost = crown_shade(point, &crowns, None).clamp(0.0, 1.0 - SKY_FLOOR);
            shade.push((lost / (1.0 - SKY_FLOOR) * levels + 0.5) as u8);
        }
    }
    Some(shade)
}

/// Placement over an isolated grid already sampled. This helper has no
/// neighbor context; production callers use [`place_on_grid_with_source`].
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "slot counts and cell-local metres are small; the u8 and u16 quantisations are clamped"
)]
pub fn place_on_grid(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    sea_level: f32,
    flora: &FloraTable,
    seed: Seed,
) -> CellFlora {
    place_on_grid_with(coord, grid, sea_level, flora, seed, true)
}

/// Isolated-grid placement, with optional grass. For streamed cells use
/// [`place_on_grid_with_source`] so exclusion sees neighboring cells.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "slot counts and cell-local metres are small; the u8 and u16 quantisations are clamped"
)]
pub fn place_on_grid_with(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    sea_level: f32,
    flora: &FloraTable,
    seed: Seed,
    with_grass: bool,
) -> CellFlora {
    let candidates = plant_candidates(
        coord,
        grid,
        sea_level,
        flora,
        seed,
        [0.0, CELL_M, 0.0, CELL_M],
        false,
    );
    let mut plants = thin_candidates(&candidates, flora);
    let children = place_children(coord, grid, sea_level, flora, &plants);
    plants.extend(children);
    let neighbours = plants.clone();
    sky_terms(&mut plants, &neighbours, flora);
    let grass = if with_grass {
        place_grass(coord, grid, sea_level, flora, seed, &plants)
    } else {
        Vec::new()
    };
    CellFlora {
        coord,
        plants,
        grass,
    }
}

/// Placement with neighbor context, shared by rendering and physics. Reuses the
/// caller's center height grid. A finite halo plus local-priority thinning makes
/// exclusion independent of cell loading order, without recursive greedy chains.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "halo offsets at most one; grid indices bounded to 64"
)]
pub fn place_on_grid_with_source(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    with_grass: bool,
) -> CellFlora {
    place_halo(coord, grid, source, flora, seed, with_grass, None)
}

/// [`place_cell_with`] reading the halo's candidates from `cache` instead of
/// sampling the neighbours' strips, and leaving this cell's own there for
/// the cells round it. The same bits.
#[must_use]
pub fn place_cell_cached(
    coord: RegionCoord,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    scratch: &mut Vec<Sample>,
    grass: bool,
    cache: &CandidateCache,
) -> CellFlora {
    let grid = HeightGrid::sample(coord, source, scratch);
    place_halo(coord, &grid, source, flora, seed, grass, Some(cache))
}

/// The halo of `coord` as a list of candidates in the cell's frame: the
/// neighbours' candidates inside `2 * radius` of the shared edge, in the
/// order the strip walk gives them (row by row, then each cell's own order).
fn halo_candidates(
    coord: RegionCoord,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    trees_only: bool,
    cache: &CandidateCache,
    out: &mut Vec<Plant>,
) {
    let halo = flora.exclusion_reach() * 2.0;
    for dz in -1_i32..=1 {
        for dx in -1_i32..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let (Some(x), Some(z)) = (coord.x.checked_add(dx), coord.z.checked_add(dz)) else {
                continue;
            };
            let axis_bounds = |offset: i32| match offset {
                -1 => [CELL_M - halo, CELL_M],
                1 => [0.0, halo],
                _ => [0.0, CELL_M],
            };
            let [xmin, xmax] = axis_bounds(dx);
            let [zmin, zmax] = axis_bounds(dz);
            let list = cache.candidates(
                RegionCoord::new(x, z, 0),
                trees_only,
                flora,
                seed,
                source,
                None,
            );
            // The test `plant_candidates` applies to a slot, so the strip is
            // the same set.
            out.extend(
                list.iter()
                    .filter(|p| !(p.x < xmin || p.x > xmax || p.z < zmin || p.z > zmax)),
            );
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "halo offsets at most one; grid indices bounded to 64"
)]
fn place_halo(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    with_grass: bool,
    cache: Option<&CandidateCache>,
) -> CellFlora {
    let mut candidates = match cache {
        Some(cache) => cache
            .candidates(coord, false, flora, seed, source, Some(grid))
            .to_vec(),
        None => plant_candidates(
            coord,
            grid,
            source.sea_level(),
            flora,
            seed,
            [0.0, CELL_M, 0.0, CELL_M],
            false,
        ),
    };
    let radius = flora.exclusion_reach();
    // Grass needs accepted neighboring plants, whose own competitors can be
    // another radius away. 2r fits within one neighboring cell (r <= 16m).
    if radius > 0.0
        && let Some(cache) = cache
    {
        halo_candidates(coord, source, flora, seed, false, cache, &mut candidates);
    } else if radius > 0.0 {
        let mut samples = Vec::new();
        for dz in -1_i32..=1 {
            for dx in -1_i32..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let (Some(x), Some(z)) = (coord.x.checked_add(dx), coord.z.checked_add(dz)) else {
                    continue;
                };
                let neighbor = RegionCoord::new(x, z, 0);
                let halo = radius * 2.0;
                let axis_bounds = |offset: i32| match offset {
                    -1 => [CELL_M - halo, CELL_M],
                    1 => [0.0, halo],
                    _ => [0.0, CELL_M],
                };
                let [xmin, xmax] = axis_bounds(dx);
                let [zmin, zmax] = axis_bounds(dz);
                let bounds = [xmin, xmax, zmin, zmax];
                // Only sample the edge strip used by candidates, plus the
                // horizontal deadwood footprint. Other grid entries are never
                // probed. This avoids eight full terrain grids per request.
                let support = flora
                    .species()
                    .iter()
                    .filter(|s| s.tree.form == "log")
                    .map(|s| (s.tree.height_m.max * s.size.1 * 0.5).clamp(0.0, CELL_M))
                    .fold(1.0_f32, f32::max);
                samples.resize(GRID * GRID, *grid.at(0, 0));
                let corner = neighbor.min_corner();
                let lo = |v: f32| (v - support).floor().max(0.0) as usize;
                let hi = |v: f32| (v + support).ceil().min(CELL_M) as usize;
                for z in lo(zmin)..=hi(zmax) {
                    for x in lo(xmin)..=hi(xmax) {
                        samples[z * GRID + x] =
                            source.sample(corner.x() + x as f64, corner.z() + z as f64);
                    }
                }
                let neighbor_grid = HeightGrid::new(&samples);
                candidates.extend(plant_candidates(
                    neighbor,
                    &neighbor_grid,
                    source.sea_level(),
                    flora,
                    seed,
                    bounds,
                    false,
                ));
            }
        }
    }
    let retained = thin_candidates(&candidates, flora);
    // Halo roots in this cell's frame: children of a neighbor's plant may
    // land here, and grass keeps clear of both.
    let mut local: Vec<_> = retained
        .iter()
        .copied()
        .map(|mut p| {
            p.x += (f64::from(p.id.cell.x) - f64::from(coord.x)) as f32 * CELL_M;
            p.z += (f64::from(p.id.cell.z) - f64::from(coord.z)) as f32 * CELL_M;
            p
        })
        .collect();
    let children = place_children(coord, grid, source.sea_level(), flora, &local);
    let mut plants: Vec<Plant> = retained
        .iter()
        .copied()
        .filter(|p| p.id.cell == coord)
        .collect();
    plants.extend(children.iter().copied());
    local.extend(children);
    sky_terms(&mut plants, &local, flora);
    let grass = if with_grass {
        place_grass(coord, grid, source.sea_level(), flora, seed, &local)
    } else {
        Vec::new()
    };
    CellFlora {
        coord,
        plants,
        grass,
    }
}

/// The plants of `coord` a far cell draws, as a full placement gives them
/// but for the children, the dressing and the grass: the rocks, trees and
/// bushes that survive thinning, with their sky. A far cell exists only for
/// its impostors (the trees and the scrub), so the children, the reeds and
/// the grass are never worked out, and the ground is read only where a
/// candidate probes it (the grid samples a node on first read) instead of
/// all 4,225 nodes and the 5,184 of the halo, which the cache replaces.
///
/// Exact, not approximate: the plants come out in the same order with the
/// same bytes as the full placement's (a test holds it over a few thousand
/// cells). Dressing ranks below everything in thinning and takes the
/// candidate budget last, a child is placed after thinning from the retained
/// plants and (when none is a tree) shades no crown, so none of them moves
/// a plant that is placed. A bush that is deadwood still runs its footprint
/// check on the lazy grid. A table where a dressing tree or a tree child
/// exists takes the full path and drops the grass.
#[must_use]
pub fn place_cell_trees(
    coord: RegionCoord,
    source: &dyn WorldSource,
    flora: &FloraTable,
    seed: Seed,
    cache: &CandidateCache,
) -> CellFlora {
    if !flora.trees_only_is_exact() {
        return place_cell_cached(coord, source, flora, seed, &mut Vec::new(), false, cache);
    }
    let own = cache.candidates(coord, true, flora, seed, source, None);
    let mut candidates = own.to_vec();
    if flora.exclusion_reach() > 0.0 {
        halo_candidates(coord, source, flora, seed, true, cache, &mut candidates);
    }
    let retained = thin_candidates(&candidates, flora);
    let local: Vec<Plant> = retained
        .iter()
        .copied()
        .map(|mut p| {
            #[allow(clippy::cast_possible_truncation, reason = "a whole number of cells")]
            {
                p.x += (f64::from(p.id.cell.x) - f64::from(coord.x)) as f32 * CELL_M;
                p.z += (f64::from(p.id.cell.z) - f64::from(coord.z)) as f32 * CELL_M;
            }
            p
        })
        .collect();
    let mut plants: Vec<Plant> = retained
        .iter()
        .copied()
        .filter(|p| p.id.cell == coord)
        .collect();
    sky_terms(&mut plants, &local, flora);
    CellFlora {
        coord,
        plants,
        grass: Vec::new(),
    }
}

/// Which candidate lists a [`CandidateCache`] holds: a cell's, for a table
/// and a seed, either all of them or a far cell's (no dressing).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CandidateKey {
    table: u64,
    seed: u64,
    coord: RegionCoord,
    trees_only: bool,
}

#[derive(Default)]
struct CandidateStore {
    entries: std::collections::HashMap<CandidateKey, (u64, std::sync::Arc<[Plant]>)>,
    order: std::collections::VecDeque<(u64, CandidateKey)>,
    next: u64,
    bytes: usize,
}

impl CandidateStore {
    fn take(&mut self, key: &CandidateKey) {
        if let Some((_, list)) = self.entries.remove(key) {
            self.bytes -= list.len() * size_of::<Plant>();
        }
    }
}

/// A bounded memo of each cell's candidates before thinning. The halo of a
/// cell is its neighbours' candidates inside a strip, and a neighbour has
/// just been placed or is about to be, so each cell's list is computed once
/// from its own grid and every cell round it filters it by bounds instead
/// of sampling the strip and rolling the hashes again. The oldest entries
/// go first.
pub struct CandidateCache {
    store: std::sync::Mutex<CandidateStore>,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for CandidateCache {
    /// Room for a streamed ring of foliage: a few thousand lists.
    fn default() -> Self {
        Self::new(8192, 96 << 20)
    }
}

impl CandidateCache {
    /// A cache holding at most `max_entries` lists and `max_bytes` of plants.
    #[must_use]
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            store: std::sync::Mutex::new(CandidateStore::default()),
            max_entries,
            max_bytes,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CandidateStore> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The lists held now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops every list.
    pub fn clear(&self) {
        *self.lock() = CandidateStore::default();
    }

    fn get(&self, key: &CandidateKey) -> Option<std::sync::Arc<[Plant]>> {
        self.lock().entries.get(key).map(|(_, list)| list.clone())
    }

    fn put(&self, key: CandidateKey, list: std::sync::Arc<[Plant]>) {
        let mut store = self.lock();
        store.take(&key);
        store.bytes += list.len() * size_of::<Plant>();
        store.next += 1;
        let stamp = store.next;
        store.entries.insert(key, (stamp, list));
        store.order.push_back((stamp, key));
        while store.bytes > self.max_bytes || store.entries.len() > self.max_entries {
            let Some((stamp, old)) = store.order.pop_front() else {
                break;
            };
            if store.entries.get(&old).is_some_and(|(s, _)| *s == stamp) {
                store.take(&old);
            }
        }
        // Keys replaced since they were queued would pile up.
        if store.order.len() > 4 * self.max_entries.max(1) {
            let store = &mut *store;
            let entries = &store.entries;
            store
                .order
                .retain(|(stamp, key)| entries.get(key).is_some_and(|(s, _)| s == stamp));
        }
    }

    /// The candidates of `coord` (`trees_only`: all but the dressing, which
    /// is all a far cell reads), from the memo, or worked out from `grid`
    /// when the caller has the cell's own, else from a grid that samples
    /// what it reads. The list holds every slot that passed its chance in
    /// placement order, whole-cell, so a halo filters it by bounds.
    #[must_use]
    pub fn candidates(
        &self,
        coord: RegionCoord,
        trees_only: bool,
        flora: &FloraTable,
        seed: Seed,
        source: &dyn WorldSource,
        grid: Option<&HeightGrid<'_>>,
    ) -> std::sync::Arc<[Plant]> {
        let key = CandidateKey {
            table: flora.serial,
            seed: seed.0,
            coord,
            trees_only,
        };
        if let Some(hit) = self.get(&key) {
            return hit;
        }
        // The rocks and trees of a whole list are the far list, in order.
        if trees_only
            && let Some(all) = self.get(&CandidateKey {
                trees_only: false,
                ..key
            })
        {
            let far: std::sync::Arc<[Plant]> = all
                .iter()
                .copied()
                .filter(|p| {
                    flora
                        .species()
                        .get(p.species.0 as usize)
                        .is_some_and(FloraTable::is_far_species)
                })
                .collect();
            self.put(key, far.clone());
            return far;
        }
        let lazy;
        let grid = if let Some(grid) = grid {
            grid
        } else {
            lazy = HeightGrid::lazy(coord, source);
            &lazy
        };
        let list: std::sync::Arc<[Plant]> = plant_candidates(
            coord,
            grid,
            source.sea_level(),
            flora,
            seed,
            [0.0, CELL_M, 0.0, CELL_M],
            trees_only,
        )
        .into();
        self.put(key, list.clone());
        list
    }
}

// Matérn type-II thinning: a candidate survives only if no higher-priority
// candidate overlaps it, even if that competitor is itself suppressed. This
// conservative rule gives finite influence and identical boundary decisions.
#[allow(
    clippy::cast_possible_truncation,
    reason = "world coordinates fit in i64 bins"
)]
fn thin_candidates(candidates: &[Plant], flora: &FloraTable) -> Vec<Plant> {
    use std::collections::HashMap;
    let radius = flora.exclusion_reach();
    if radius == 0.0 {
        return candidates.to_vec();
    }
    let position = |p: &Plant| {
        [
            f64::from(p.id.cell.x) * 64.0 + f64::from(p.x),
            f64::from(p.id.cell.z) * 64.0 + f64::from(p.z),
        ]
    };
    let bin = |p: &Plant| position(p).map(|v| (v / f64::from(radius)).floor() as i64);
    // Dressing ranks below everything else, so it never thins out a plant
    // a server collides with.
    let priority = |p: &Plant| {
        (
            flora.is_dressing(p.species),
            flora.kind(p.species).map_or(u8::MAX, kind_rank),
            mix64(
                p.id.species_key
                    ^ cell_seed(Seed(0), p.id.cell)
                    ^ (u64::from(p.id.slot[0]) << 32)
                    ^ u64::from(p.id.slot[1]),
            ),
            p.id.species_key,
            p.id.cell.x,
            p.id.cell.z,
            p.id.slot,
        )
    };
    let mut bins: HashMap<[i64; 2], Vec<usize>> = HashMap::new();
    for (i, p) in candidates.iter().enumerate() {
        bins.entry(bin(p)).or_default().push(i);
    }
    candidates
        .iter()
        .copied()
        .filter(|p| {
            let [bx, bz] = bin(p);
            let [px, pz] = position(p);
            for z in bz - 1..=bz + 1 {
                for x in bx - 1..=bx + 1 {
                    if let Some(indices) = bins.get(&[x, z]) {
                        for &i in indices {
                            let q = &candidates[i];
                            if priority(q) >= priority(p) {
                                continue;
                            }
                            let [qx, qz] = position(q);
                            let r = f64::from(
                                flora.species()[p.species.0 as usize]
                                    .exclusion_m
                                    .max(flora.species()[q.species.0 as usize].exclusion_m)
                                    .clamp(0.0, radius),
                            );
                            if (px - qx) * (px - qx) + (pz - qz) * (pz - qz) < r * r {
                                return false;
                            }
                        }
                    }
                }
            }
            true
        })
        .collect()
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "candidate slots bounded to 4096; compact IDs and variants are bounded"
)]
fn plant_candidates(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    sea_level: f32,
    flora: &FloraTable,
    seed: Seed,
    bounds: [f32; 4],
    trees_only: bool,
) -> Vec<Plant> {
    let cell = cell_seed(seed, coord);
    let mut plants: Vec<Plant> = Vec::new();
    // Trees before bushes, and within a kind the id order, so the same set
    // always places in the same order. Dressing last of all, so its slots
    // never take the budget another species needs.
    let mut placed: Vec<&Species> = flora
        .species()
        .iter()
        .filter(|s| s.kind != FloraKind::Grass && flora.places(s))
        .filter(|s| !trees_only || FloraTable::is_far_species(s))
        .collect();
    placed.sort_by_key(|s| (s.dressing, kind_rank(s.kind), s.id));
    let mut remaining = MAX_PLANT_CANDIDATES_PER_CELL;
    for species in placed {
        let salt = species.species_seed;
        let n = (CELL_M / species.spacing_m).floor().clamp(1.0, 64.0) as i64;
        let count = (n * n) as usize;
        if count > remaining {
            continue;
        }
        remaining -= count;
        let step = CELL_M / n as f32;
        for j in 0..n {
            for i in 0..n {
                let x = (i as f32 + hash_unit(cell, i, j, salt.wrapping_add(1))) * step;
                let z = (j as f32 + hash_unit(cell, i, j, salt.wrapping_add(2))) * step;
                if x < bounds[0] || x > bounds[1] || z < bounds[2] || z > bounds[3] {
                    continue;
                }
                let probe = grid.probe(x, z);
                let mut chance = fit_at(species, &probe, sea_level);
                if chance > 0.0 {
                    let corner = coord.min_corner();
                    chance *= cluster_gain(
                        species,
                        seed,
                        corner.x() + f64::from(x),
                        corner.z() + f64::from(z),
                    );
                }
                if species.kind == FloraKind::Bush && species.tree.form != "tree" {
                    let corner = coord.min_corner();
                    let patch = grass_noise(
                        species.species_seed,
                        (corner.x() + f64::from(x)) / 14.0,
                        (corner.z() + f64::from(z)) / 14.0,
                        91,
                    );
                    chance *= ((patch - 0.2) * 1.8).clamp(0.08, 1.0);
                }
                if chance <= 0.0 || hash_unit(cell, i, j, salt.wrapping_add(3)) >= chance {
                    continue;
                }
                let t = hash_unit(cell, i, j, salt.wrapping_add(5));
                // Size follows fit for what grows: a grove's edge and a poor
                // slope hold smaller trees. A rock's size is its own.
                let t = if species.kind == FloraKind::Rock {
                    t
                } else {
                    t * (0.35 + 0.65 * chance.min(1.0))
                };
                let scale = species.size.0 + (species.size.1 - species.size.0) * t;
                if species.tree.form == "log"
                    && !deadwood_ground_is_flat(
                        grid,
                        x,
                        z,
                        probe.height,
                        species.tree.height_m.max * scale * 0.5,
                    )
                {
                    continue;
                }
                plants.push(Plant {
                    id: PlantId {
                        species_key: species.species_seed,
                        cell: coord,
                        slot: [i as u32, j as u32],
                    },
                    species: species.id,
                    x,
                    y: probe.height,
                    z,
                    yaw: facing(species, &probe, hash_unit(cell, i, j, salt.wrapping_add(4))),
                    scale,
                    variant: (hash_unit(cell, i, j, salt.wrapping_add(6))
                        * f32::from(species.variants)) as u8
                        % species.variants.max(1),
                    sky: u8::MAX,
                });
            }
        }
    }

    plants
}

/// Children of `parents` (roots in `coord`'s frame, a neighbor's outside
/// `0..64`) that land inside `coord` on ground their species tolerates.
///
/// Everything about a child is hashed from its parent's identity: how many,
/// the bearing and distance, the size and the variant. A child is placed by
/// whichever cell it lands in, from parents that cell's halo already holds,
/// so it is where it is whatever streamed first. It skips the thinning
/// against its own parent, since a fern under its tree is the point, but
/// keeps clear of every other plant by the larger of the two exclusions, so
/// no stone lands in a trunk. Children do not keep clear of each other
/// across a cell edge: they are small, and have no halo of their own.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "child counts and variants are small; hashes are unit floats"
)]
fn place_children(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    sea_level: f32,
    flora: &FloraTable,
    parents: &[Plant],
) -> Vec<Plant> {
    let mut children = Vec::new();
    for parent in parents {
        let Some(species) = flora.species().get(parent.species.0 as usize) else {
            continue;
        };
        for (rule_index, rule) in species.children.iter().enumerate() {
            let Some(child) = flora
                .species()
                .get(rule.species.0 as usize)
                .filter(|child| flora.places(child))
            else {
                continue;
            };
            let key = mix64(
                parent.id.species_key
                    ^ cell_seed(Seed(0), parent.id.cell)
                    ^ (u64::from(parent.id.slot[0]) << 32)
                    ^ u64::from(parent.id.slot[1])
                    ^ ((rule_index as u64) << 56),
            );
            let span = u32::from(rule.count[1] - rule.count[0]) + 1;
            let count = u32::from(rule.count[0])
                + ((hash_unit(key, 0, 0, 1) * span as f32) as u32).min(span - 1);
            for k in 0..count {
                let h = |salt: u64| hash_unit(key, i64::from(k), 1, salt);
                let (sin, cos) = libm::sincosf(h(2) * core::f32::consts::TAU);
                let reach = rule.radius_m[0] + (rule.radius_m[1] - rule.radius_m[0]) * h(3).sqrt();
                let x = parent.x + cos * reach;
                let z = parent.z + sin * reach;
                if !(0.0..CELL_M).contains(&x) || !(0.0..CELL_M).contains(&z) {
                    continue;
                }
                // Dressing crowds out no child: the child would be missing
                // from a server's world, which has no dressing.
                let crowded = parents.iter().any(|other| {
                    if std::ptr::eq(other, parent) || flora.is_dressing(other.species) {
                        return false;
                    }
                    let r = flora
                        .species()
                        .get(other.species.0 as usize)
                        .map_or(0.0, |s| s.exclusion_m)
                        .max(child.exclusion_m);
                    (other.x - x) * (other.x - x) + (other.z - z) * (other.z - z) < r * r
                });
                if crowded {
                    continue;
                }
                let probe = grid.probe(x, z);
                if fit_at(child, &probe, sea_level) <= 0.0 {
                    continue;
                }
                children.push(Plant {
                    id: PlantId {
                        species_key: child.species_seed,
                        cell: coord,
                        // Grid slots are below 64; a child's first word has
                        // the top bit set, so the two never meet.
                        slot: [0x8000_0000 | (key >> 33) as u32, k],
                    },
                    species: child.id,
                    x,
                    y: probe.height,
                    z,
                    yaw: facing(child, &probe, h(4)),
                    scale: child.size.0 + (child.size.1 - child.size.0) * h(5),
                    variant: (h(6) * f32::from(child.variants)) as u8 % child.variants.max(1),
                    sky: u8::MAX,
                });
            }
        }
    }
    children
}

/// A plant's yaw in turns: the hash's, or for a species that faces downhill
/// the slope's own bearing with a little of the hash, so a cliff piece's
/// face (`+z` in its frame) looks down the slope.
fn facing(species: &Species, probe: &Probe<'_>, hash: f32) -> f32 {
    let [ax, az] = probe.sample.landform.aspect;
    if !species.ground.face_downhill || ax * ax + az * az < 1e-6 {
        return hash;
    }
    // Rotation about y by theta takes +z to (sin theta, 0, cos theta).
    let turns = libm::atan2f(ax, az) / core::f32::consts::TAU + (hash - 0.5) * 0.08;
    turns - turns.floor()
}

/// Deadwood remains horizontal, so only accept a fully sampled flat footprint.
/// Reject cell-edge candidates rather than clamping an unseen endpoint.
fn deadwood_ground_is_flat(
    grid: &HeightGrid<'_>,
    x: f32,
    z: f32,
    height: f32,
    radius: f32,
) -> bool {
    if x < radius || z < radius || x + radius >= CELL_M || z + radius >= CELL_M {
        return false;
    }
    (-2_i16..=2).all(|dz| {
        (-2_i16..=2).all(|dx| {
            let probe = grid.probe(
                x + f32::from(dx) * radius * 0.5,
                z + f32::from(dz) * radius * 0.5,
            );
            (probe.height - height).abs() <= 0.06
        })
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "cell-local candidate counts are small; compact record quantisations are bounded"
)]
fn place_grass(
    coord: RegionCoord,
    grid: &HeightGrid<'_>,
    sea_level: f32,
    flora: &FloraTable,
    seed: Seed,
    plants: &[Plant],
) -> Vec<GrassBlade> {
    let cell = cell_seed(seed, coord);
    let mut grass = Vec::new();
    let mut grasses: Vec<&Species> = flora
        .species()
        .iter()
        .filter(|s| s.kind == FloraKind::Grass)
        .collect();
    grasses.sort_by_key(|s| s.id);
    let exclusions = GrassExclusions::new(plants, flora);
    // Grass under a crown takes the crown's shade, from the nearest node
    // of a 2 m raster: a blade is a picture, and the raster costs a cell
    // a thousand crown sums rather than one per blade.
    let crown_shade = grass_shade(grid, plants, flora);
    let mut remaining = MAX_GRASS_CANDIDATES_PER_CELL;
    for (palette, species) in grasses.iter().enumerate() {
        let salt = species.species_seed;
        let per_side = (CELL_M * species.density_per_m2.sqrt())
            .round()
            .clamp(1.0, 256.0) as i64;
        let count = (per_side * per_side) as usize;
        if count > remaining || palette >= strata_data::flora::MAX_GRASS_SPECIES {
            continue;
        }
        remaining -= count;
        let field = GrassField::new(coord, seed.derive("grass-patches") ^ species.species_seed);
        let tufts = grass_tufts(coord, seed.derive("grass-tufts") ^ species.species_seed);
        let step = CELL_M / per_side as f32;
        for j in 0..per_side {
            for i in 0..per_side {
                let mut x = (i as f32 + hash_unit(cell, i, j, salt.wrapping_add(1))) * step;
                let mut z = (j as f32 + hash_unit(cell, i, j, salt.wrapping_add(2))) * step;
                let tx = (x * 0.5).floor().min(31.0) as usize;
                let tz = (z * 0.5).floor().min(31.0) as usize;
                let tuft_shape = tufts[tz * 32 + tx];
                let clustered = hash_unit(cell, i, j, salt.wrapping_add(8)) < 0.85;
                if clustered {
                    // Compress existing slots into a disk around a jittered
                    // tuft centre. No new candidates or blade records.
                    x = tuft_shape[0] + (x - (tx as f32 * 2.0 + 1.0)) * tuft_shape[4];
                    z = tuft_shape[1] + (z - (tz as f32 * 2.0 + 1.0)) * tuft_shape[4];
                }
                // Quantise before probing: the GPU root and its ground height
                // must describe the same point, especially on steep ground.
                let qx = (x / CELL_M * 65535.0) as u16;
                let qz = (z / CELL_M * 65535.0) as u16;
                let x = f32::from(qx) * (CELL_M / 65535.0);
                let z = f32::from(qz) * (CELL_M / 65535.0);
                let probe = grid.probe(x, z);
                let [patch, tuft, mut lean] = field.probe(x, z);
                let coverage =
                    ((patch - 0.15) * 1.8).clamp(0.04, 1.0) * ((tuft - 0.2) * 2.3).clamp(0.06, 1.0);
                let chance = fit_at(species, &probe, sea_level) * coverage;
                if chance <= 0.0 || hash_unit(cell, i, j, salt.wrapping_add(3)) >= chance {
                    continue;
                }
                if exclusions.contains(x, z) {
                    continue;
                }
                let profile = if clustered {
                    lean = tuft_shape[3];
                    tuft_shape[2]
                } else {
                    tuft
                };
                let t = (0.75 * profile + 0.25 * hash_unit(cell, i, j, salt.wrapping_add(4)))
                    .clamp(0.0, 1.0);
                let height_m = species.size.0 + (species.size.1 - species.size.0) * t;
                let quant = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
                // A tuft reads as one clump: its blades share most of their
                // shade, and their normals round over it like a dome, so it
                // is lit as a mass rather than as a carpet.
                let own_shade = hash_unit(cell, i, j, salt.wrapping_add(7));
                let (shade, normal) = if clustered {
                    let dx = x - tuft_shape[0];
                    let dz = z - tuft_shape[1];
                    let from_centre = (dx * dx + dz * dz).sqrt().max(1e-3);
                    let reach = (from_centre / (tuft_shape[4] * 1.4)).min(1.0) * 0.35;
                    let n = [
                        probe.normal[0] + dx / from_centre * reach,
                        probe.normal[1],
                        probe.normal[2] + dz / from_centre * reach,
                    ];
                    let norm = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    (
                        0.7 * tuft_shape[3] + 0.3 * own_shade,
                        [n[0] / norm, n[2] / norm],
                    )
                } else {
                    (own_shade, [probe.normal[0], probe.normal[2]])
                };
                grass.push(GrassBlade {
                    x: qx,
                    z: qz,
                    y_bits: probe.height.to_bits(),
                    height_cm: (height_m * 100.0).clamp(1.0, 255.0) as u8,
                    phase: quant(
                        (lean + 0.08 * hash_unit(cell, i, j, salt.wrapping_add(5))).fract(),
                    ),
                    stiffness: quant(hash_unit(cell, i, j, salt.wrapping_add(6)) * 0.4),
                    species: palette as u8
                        | crown_shade.as_ref().map_or(0, |shade| {
                            let column = ((x * 0.5 + 0.5) as usize).min(GRASS_SHADE_N - 1);
                            let row = ((z * 0.5 + 0.5) as usize).min(GRASS_SHADE_N - 1);
                            shade[row * GRASS_SHADE_N + column] << GrassBlade::PALETTE_BITS
                        }),
                    lush: quant(probe.sample.hydration),
                    shade: quant(shade),
                    nx: (normal[0].clamp(-1.0, 1.0) * 127.0) as i8,
                    nz: (normal[1].clamp(-1.0, 1.0) * 127.0) as i8,
                });
            }
        }
    }

    // Evenly retain the whole species-ordered stream, avoiding truncation's
    // bias toward early species. Candidate allocation is itself bounded.
    if grass.len() > MAX_GRASS_BLADES_PER_CELL {
        let total = grass.len();
        grass = (0..MAX_GRASS_BLADES_PER_CELL)
            .map(|i| grass[i * total / MAX_GRASS_BLADES_PER_CELL])
            .collect();
    }
    grass
}

// Two metres divides a placement cell exactly, so each jittered tuft and
// its whole 0.25..0.45 m radius belongs to one cell even at negative x/z.
#[allow(
    clippy::cast_precision_loss,
    reason = "local tuft indices are at most31"
)]
fn grass_tufts(coord: RegionCoord, seed: u64) -> Vec<[f32; 5]> {
    let mut tufts = Vec::with_capacity(32 * 32);
    for z in 0..32i64 {
        for x in 0..32i64 {
            let wx = i64::from(coord.x) * 32 + x;
            let wz = i64::from(coord.z) * 32 + z;
            tufts.push([
                x as f32 * 2.0 + 0.45 + 1.1 * hash_unit(seed, wx, wz, 1),
                z as f32 * 2.0 + 0.45 + 1.1 * hash_unit(seed, wx, wz, 2),
                hash_unit(seed, wx, wz, 3),
                hash_unit(seed, wx, wz, 4),
                (0.25 + 0.2 * hash_unit(seed, wx, wz, 5)) * std::f32::consts::FRAC_1_SQRT_2,
            ]);
        }
    }
    tufts
}

#[cfg(test)]
mod grass_field_tests {
    use super::*;

    #[test]
    fn deadwood_rejects_unseen_edges_and_uneven_support() {
        let biome = strata_data::BiomeId(0);
        let mut samples = vec![
            Sample {
                height: 10.0,
                biomes: [(biome, 1.0), (biome, 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.7,
                hydration: 1.0,
                cover: 1.0,
                landform: strata_worldgen::Landform::default(),
                water: None,
            };
            GRID * GRID
        ];
        assert!(deadwood_ground_is_flat(
            &HeightGrid::new(&samples),
            32.0,
            32.0,
            10.0,
            2.0
        ));
        assert!(!deadwood_ground_is_flat(
            &HeightGrid::new(&samples),
            0.5,
            32.0,
            10.0,
            2.0
        ));
        samples[32 * GRID + 33].height += 0.2;
        assert!(!deadwood_ground_is_flat(
            &HeightGrid::new(&samples),
            32.0,
            32.0,
            10.0,
            2.0
        ));
    }

    #[test]
    fn jittered_tufts_keep_their_full_radius_inside_the_owning_cell() {
        for x in [-2, -1, 0, 1] {
            let tufts = grass_tufts(RegionCoord::new(x, -1, 0), 42);
            for tuft in tufts {
                let radius = tuft[4] * std::f32::consts::SQRT_2;
                assert!(tuft[0] - radius >= 0.0 && tuft[0] + radius < CELL_M);
                assert!(tuft[1] - radius >= 0.0 && tuft[1] + radius < CELL_M);
            }
        }
        // The negative cell's last tuft uses world lattice index -1,
        // continuing directly into index0 in the next cell.
        let left = grass_tufts(RegionCoord::new(-1, 0, 0), 42);
        let right = grass_tufts(RegionCoord::new(0, 0, 0), 42);
        assert_eq!(left[31][3].to_bits(), hash_unit(42, -1, 0, 4).to_bits());
        assert_eq!(right[0][3].to_bits(), hash_unit(42, 0, 0, 4).to_bits());
    }

    #[test]
    fn patches_share_boundary_samples_across_negative_and_positive_cells() {
        for x in [-2, -1, 0, 1] {
            let left = GrassField::new(RegionCoord::new(x, -1, 0), 42);
            let right = GrassField::new(RegionCoord::new(x + 1, -1, 0), 42);
            let north = GrassField::new(RegionCoord::new(x, 0, 0), 42);
            for i in 0..GRID {
                assert_eq!(
                    left.values[i * GRID + GRID - 1].map(f32::to_bits),
                    right.values[i * GRID].map(f32::to_bits)
                );
                assert_eq!(
                    left.values[(GRID - 1) * GRID + i].map(f32::to_bits),
                    north.values[i].map(f32::to_bits)
                );
            }
        }
    }
}
