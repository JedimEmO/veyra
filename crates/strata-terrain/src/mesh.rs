//! The heightfield mesh builder: one region coordinate in, one mesh out.
//!
//! A pure function of the world source and the coordinate, with no ECS and
//! no GPU, so it runs on the compute pool and in a headless test alike
//! (ADR 0003). Every LOD builds the same [`GRID_N`] by [`GRID_N`] grid; a
//! coarse node is simply sampled at a wider step.
//!
//! # Layout
//!
//! Positions are metres relative to the region's minimum corner, so the
//! region entity's `WorldPosition` plus the render-origin sync place the
//! mesh (ADR 0002) and a mesh never carries a large `f32`. The grid is
//! sampled with two extra rings of points around it: one so every grid
//! vertex has a central-difference normal from the same samples its
//! neighbours use, which keeps normals continuous across region borders,
//! and a second so the same is true of the parent LOD's normal, whose
//! difference spans two steps.
//!
//! Vertex `j * GRID_N + i` is the grid point `(i, j)` at `(i * step, j *
//! step)`. After the grid come the skirts: for each of the four edges, a
//! copy of every edge vertex pushed straight down by [`RegionMesh::skirt_depth`].
//! A skirt wall hides the crack where a coarse neighbour's straight edge
//! cuts under this node's finer edge. Skirt walls face outward and are
//! never seen from inside the node, so back-face culling keeps them free.
//!
//! # LOD morph
//!
//! The grid of a node's parent is the node's own grid with every other
//! point: nodes are aligned to their size, so a parent's vertex spacing of
//! two steps lands on the child's even `(i, j)`. Every vertex therefore
//! knows where it sits on the parent's mesh: an even vertex is on it, an
//! odd vertex on an edge lies halfway between its even neighbours, and a
//! vertex odd on both axes lies on the parent cell's diagonal, which the
//! index buffer runs from `(i + 1, j - 1)` to `(i - 1, j + 1)`. The builder
//! writes that parent height and normal per vertex ([`RegionMesh::morph`]),
//! and the vertex shader lerps towards them with distance, so a node's far
//! edge meets a coarse neighbour exactly and a LOD swap changes nothing on
//! screen (the CDLOD morph; the shader side is in `material.rs`).
//!
//! # Layer weights
//!
//! Every vertex carries the share of each layer, one byte per layer summing
//! to 255 ([`RegionMesh::weights`]), and the shader interpolates all of
//! them and draws the strongest. Nothing about the ground is flat per
//! triangle, so a border between two layers is where their shares cross,
//! wherever it runs through the grid; a pair of layers read from one vertex
//! per triangle drew every diagonal border as a sawtooth a grid step deep
//! and every meeting of three layers as a fan of teeth. The shares are
//! tent-filtered over the sampled grid, so a border that the samples
//! resolve as a staircase is drawn along its smoothed line. The layer of a
//! biome is its flat-ground material; steep ground turns to the dominant
//! biome's slope material in the shader from the geometric normal, which is
//! per pixel and independent of LOD, so the sample's own slope-chosen
//! material is not used. The tint attribute is the biomes' ground colours
//! (`BiomeDef::ground_colour`'s hue at the luminance of the biome's surface
//! layer, `strata_registry::far_colour`) blended by
//! weight, for the far field and the near ground's hue.
//!
//! # Rock
//!
//! Steep wild ground bares the rock of its geological province (ticket
//! 0004, step 3): each vertex carries its sample's two provinces as the
//! pair of face layers they name and the share of the higher layer
//! ([`RegionMesh::slope`]). A pair is two indices, and an index cannot be
//! interpolated; read flat, a triangle takes one vertex's pair and its
//! neighbour another's wherever the pairs differ (the edge of a border's
//! blend, where a province alone meets a pair, and everywhere three
//! provinces meet), and the border is drawn along the triangles.
//!
//! So every vertex also carries its corner of the triangle it is in,
//! [`corner_of`]: `(i - j) mod 3` on the grid, which gives every triangle
//! one vertex of each corner. The shader writes each vertex's pair and
//! share into the slot of its corner and a one in that slot of a third
//! vector, all interpolated: in a triangle the slot `k` of the third
//! vector is the barycentric weight of the vertex at corner `k`, and slot
//! `k` of the pair vector over it is that vertex's pair again. The fragment
//! so reads all three vertices' pairs and shares and blends them by where
//! it lies, which along an edge is a blend of that edge's two vertices
//! alone, the same from both triangles. [`RockCorners`] is the
//! fragment's decode, mirrored here for the tests.
//!
//! # Sky
//!
//! Every vertex carries how much of the sky it sees and the mean direction
//! it sees it in, a bent normal ([`RegionMesh::sky`]; the lighting plan's
//! phase 3a), so the sky's fill is darker in a gully than on a ridge at no
//! cost per frame. Eight azimuths, the axes and the diagonals, each read
//! the ground at three distances, and the steepest rise over the vertex is
//! that azimuth's horizon. A horizon at elevation `e` leaves `cos^2 e` of a
//! sector's cosine-weighted sky, which is `1 / (1 + t^2)` of the rise `t`
//! and needs no transcendental. The vertex's own tangent plane is divided
//! out, so a uniform slope reads as open: the shader's environment light is
//! already looked up along the normal, and only the ground that rises above
//! the plane, a gully's far wall, is occlusion.
//!
//! The ground the rays read is two world-aligned lattices of heights, one
//! at four ray units for the near distances and one at sixteen for the far
//! one, a ray unit being the vertex step but never under 2 m. Being world
//! aligned and a pure function of the level, they give two regions of a
//! level the same value on their shared edge, and a node's parent level is
//! the same function at twice the unit: every even vertex carries the
//! parent's value at its point and every odd one the parent's interpolation,
//! as the heights do, so the morph meets a coarse neighbour exactly. The
//! lattice nodes the grid already sampled are reused; the rest are the
//! bake's cost, a few hundred samples a region.
//!
//! # Water's edge
//!
//! Ground by a river or a lake (ticket 0005, step 3) is the water's: within
//! a metre or so over its surface, by a river's channel or round a lake, a
//! vertex takes the bed's material (a channel's gravel, a lake's or a
//! mouth's sand) in place of its biomes' and bares no rock, so a creek's
//! small steep banks are gravel and not a dashed line of the province's
//! rock bands; and it carries how wet it is ([`bank_of`]), which the shader
//! darkens within a metre of the water line, in the sky attribute's spare
//! byte. A valley's wall well over its river, where the carve lowered the
//! ground, carries how much of a cut wall it is, in the parent sky's spare
//! byte, and the shader breaks a banded rock's beds up there, so a gorge's
//! wall does not read as ruled lines.
//!
//! # Made ground
//!
//! A bank a road cut or filled and a wall a river's valley cut (ticket
//! 0005, step 6) are soil that takes the land's own ground and cover, not
//! rock: a vertex whose ground was moved ([`made`], the most of it and its
//! neighbours, so a bank's rim is soil too) bares rock only where the cut
//! is both deep and steep, and there in outcrops tens of metres across
//! ([`made_rock`]); a road's fill bares none.
//!
//! # Roads at distance
//!
//! A level whose spacing is a road's carriageway width or more leaves the
//! carriageway out of its shares, its rock and its tint, as wild ground
//! ([`crate::paint::way_dropped`]): the fragment shader paints the road
//! there at its own width from the network's line ([`crate::paint`]). Which
//! samples are that road's is the painted line's to say
//! ([`crate::paint::PaintedWays`], [`build_region_mesh_painted`]), not
//! their material's, and a region built without its paint keeps every road
//! in its splat.

// Grid indices are at most GRID_N squared plus skirts (a few thousand) and
// region sizes are powers of two under 32 km: every cast between usize,
// u32, f32 and f64 in this module is exact.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "grid indices and region sizes are small and exact in every width used"
)]
// The geometry uses the conventional single letters for vertices and
// vectors; longer names would hide the formulas.
#![allow(clippy::many_single_char_names, reason = "vertex and vector maths")]

use strata_core::{GRID_N, RegionCoord};
use strata_registry::{BiomeId, FloraKind, GameRegistries, ProvinceId};
use strata_worldgen::hydrology::WaterKind;
use strata_worldgen::{Sample, SampleWater, WorldSource};

use crate::material::{MAX_LAYERS, UV_PERIOD_M};

/// The material a river's bed is laid in, as the carve lays it
/// (`strata_worldgen::procedural`'s road material).
const BED_GRAVEL: &str = "strata:gravel";

/// The material a lake's and a mouth's bed is laid in.
const BED_SAND: &str = "strata:sand";

/// Over this band of height over the water's surface, metres, a bank's bed
/// material gives way to its biomes': whole up to the first, none from the
/// second. A creek's bank rises its depth and a freeboard of 0.3 m from its
/// bed, so its steep part is inside the band; a level river's flat bank
/// takes a strip of gravel a few metres wide and not a road's width.
const WET_BAND_M: (f32, f32) = (SampleWater::WETTED_BED_M, 1.0);

/// How far over the water the bank is darkened, metres, fading from the
/// water line: the wet line of a river's stones.
const DAMP_M: f32 = 0.8;

/// Over this band of height over the water, metres, the wet bank (ticket
/// 0005, step 8) gives way to dry ground: the soil a lake's or a river's
/// water keeps damp beyond its wet line, darker and smoother, fading out by
/// the second.
const WET_BANK_M: (f32, f32) = (0.5, 1.9);

/// Past a river's banks, how far out its wet bank reaches over its edge,
/// metres: a few strides of damp soil, past the bed's.
const WET_BANK_REACH_M: f32 = 4.0;

/// How wet the wet bank is at its wettest, as a share of the water line's
/// darkening: soil, not stone in the splash.
const WET_BANK_SHARE: f32 = 0.45;

/// Past a river's banks, how far out the water's edge reaches, metres
/// (the world's, which placement keeps off too).
const EDGE_REACH_M: f32 = SampleWater::EDGE_REACH_M;

/// Over this band of height over a river's surface, metres, a valley's
/// ground becomes a cut wall whose rock bands the shader breaks up.
const WALL_BAND_M: (f32, f32) = (2.0, 8.0);

/// Over this band of how far the carve lowered the ground, metres, the
/// ground is the carve's wall and not a slope the valley left standing: a
/// natural cliff in the valley keeps its ruled beds, and the treatment
/// fades in with the cut, which is continuous, so it has no edge where the
/// valley's record ends or the nearest river changes.
const WALL_CUT_M: (f32, f32) = (0.5, 6.0);

/// Over this band of the ground's rise over run, a cut is a wall: a
/// valley's floor is cut by metres too, but it is gentle and its beds lie
/// flat; with the treatment on it, a shallow valley's whole flank read as
/// wavy bands against the ruled benches of the hillside above, edged where
/// the cut ended (the review of step 3).
const WALL_SLOPE: (f32, f32) = (0.7, 1.2);

/// Rings of samples around the grid: one for this LOD's normals, one more
/// for the parent LOD's.
pub const RING: usize = 2;

/// Grid points sampled per side: the grid plus the rings.
pub const SAMPLE_N: usize = GRID_N + 2 * RING;

/// Vertices in the grid alone.
pub const GRID_VERTICES: usize = GRID_N * GRID_N;

/// Skirt vertices: one per edge vertex, on four edges. Corners are
/// duplicated per edge, which is simpler than sharing them and costs eight
/// vertices.
pub const SKIRT_VERTICES: usize = 4 * GRID_N;

/// Total vertices of a region mesh.
pub const VERTEX_COUNT: usize = GRID_VERTICES + SKIRT_VERTICES;

/// Indices of the grid's triangles.
pub const GRID_INDICES: usize = (GRID_N - 1) * (GRID_N - 1) * 6;

/// Indices of the skirt walls.
pub const SKIRT_INDICES: usize = 4 * (GRID_N - 1) * 6;

/// Total indices of a region mesh.
pub const INDEX_COUNT: usize = GRID_INDICES + SKIRT_INDICES;

/// What the mesh builder needs to know about each biome: its texture
/// layers and its tint, by biome id.
///
/// Built once from the frozen registries and shared by every meshing task,
/// so a task does a slice lookup per vertex and never touches the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeLayers {
    layers: Vec<u8>,
    slope_layers: Vec<u8>,
    tints: Vec<[f32; 3]>,
    /// Per biome: the canopy colour of its trees and bushes (linear RGB)
    /// and the fraction of the ground they cover, from the flora registry.
    covers: Vec<[f32; 4]>,
    /// `1 - normal.y` at which the biome's slope material takes over,
    /// from its rise-over-run threshold.
    slope_thresholds: Vec<f32>,
    /// The biome's rise-over-run threshold itself, which a province's
    /// `bare_slope` multiplies before it becomes a `1 - normal.y`.
    slope_rises: Vec<f32>,
    /// Per province: the rock its bare faces show, by province id.
    provinces: Vec<ProvinceLook>,
    /// The layers a river's bed (gravel) and a lake's (sand) are laid in,
    /// for the ground at the water's edge ([`bank_of`]); `None` in a
    /// registry set without them, where the bank keeps its biomes.
    beds: [Option<u8>; 2],
    /// The layers each class of road's carriageway is laid in, `[service,
    /// track, trail]` (`strata_worldgen::procedural::WAY_MATERIALS`): the
    /// paint draws a road in its class's layer where a level is too coarse
    /// for the splat to ([`crate::paint`]).
    ways: [Option<u8>; 3],
}

/// What the mesh builder needs of one geological province: the layer of
/// its bare faces, the factor on the slope at which they bare, and the
/// factor on the far tint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProvinceLook {
    /// The texture array layer of its faces.
    pub face: u8,
    /// Multiplies the biome's rise-over-run slope threshold.
    pub bare_slope: f32,
    /// Multiplies the biome's far tint, linear RGB.
    pub far_tint: [f32; 3],
}

impl BiomeLayers {
    /// The flat-ground and slope material layers and the tint of every
    /// biome in `registries`.
    #[must_use]
    pub fn from_registries(registries: &GameRegistries) -> Self {
        let mut layers = Vec::new();
        let mut slope_layers = Vec::new();
        let mut tints = Vec::new();
        let mut slope_thresholds = Vec::new();
        let mut slope_rises = Vec::new();
        let mut covers = Vec::new();
        for id in registries.biome_ids() {
            covers.push(canopy_of(registries, id));
            let surface = registries.surface(id);
            layers.push(surface.map_or(0, |s| s.material.layer()));
            slope_layers.push(surface.map_or(0, |s| s.slope_material.layer()));
            let rise = surface.map_or(0.7, |s| s.slope_threshold);
            slope_rises.push(rise);
            slope_thresholds.push(slope_of_rise(rise));
            // The far colour (value budget, review step six): the ground's
            // palette hue at the luminance its surface layer is drawn at near,
            // so the plain at a kilometre is as bright as the ground it
            // blends from and not several times over it. A biome whose
            // layer the budget leaves as baked keeps its colour as written.
            tints.push(registries.biomes.get(id).map_or([1.0, 1.0, 1.0], |def| {
                let ground = def.ground_colour().0;
                surface
                    .and_then(|s| registries.materials.get(s.material))
                    .and_then(|m| registries.values.terrain_target(&m.texture_set))
                    .map_or(ground, |target| strata_registry::far_colour(ground, target))
            }));
        }
        let provinces = registries
            .province_ids()
            .map(|id| {
                registries
                    .province(id)
                    .map_or(ProvinceLook::NEUTRAL, |traits| {
                        let rock = &traits.rock;
                        ProvinceLook {
                            face: rock.face.layer(),
                            bare_slope: rock.bare_slope,
                            far_tint: rock.far_tint.0,
                        }
                    })
            })
            .collect();
        let layer_of = |name: &str| {
            strata_registry::Namespaced::parse(name)
                .ok()
                .and_then(|name| registries.material_id(&name))
                .map(strata_registry::TerrainMaterialId::layer)
        };
        let ways = strata_worldgen::procedural::WAY_MATERIALS.map(layer_of);
        let beds = [BED_GRAVEL, BED_SAND].map(layer_of);
        Self {
            layers,
            slope_layers,
            tints,
            covers,
            slope_thresholds,
            slope_rises,
            provinces,
            beds,
            ways,
        }
    }

    /// The same table with `beds` as the layers of a river's bed and a
    /// lake's, for tests and tools.
    #[must_use]
    pub fn with_beds(mut self, beds: [Option<u8>; 2]) -> Self {
        self.beds = beds;
        self
    }

    /// The layers of the roads' carriageways, `[service, track, trail]`.
    #[must_use]
    pub fn ways(&self) -> [Option<u8>; 3] {
        self.ways
    }

    /// The layer the bed of water of `kind` is laid in: a channel's and a
    /// fall's gravel, a lake's and a mouth's sand, as the carve lays them.
    #[must_use]
    pub fn bed(&self, kind: WaterKind) -> Option<u8> {
        match kind {
            WaterKind::Channel | WaterKind::Fall => self.beds[0],
            WaterKind::Lake | WaterKind::Mouth => self.beds[1],
        }
    }

    /// The same table with `provinces` as its geology, by province id, for
    /// tests and tools.
    #[must_use]
    pub fn with_provinces(mut self, provinces: Vec<ProvinceLook>) -> Self {
        self.provinces = provinces;
        self
    }

    /// What `province` looks like, or `None` for [`ProvinceId::NONE`] and
    /// an id this table does not know, where the biome's own slope layer
    /// stands in.
    #[must_use]
    pub fn province(&self, province: ProvinceId) -> Option<&ProvinceLook> {
        if province.is_none() {
            return None;
        }
        self.provinces.get(province.0 as usize)
    }

    /// The canopy colour and cover of `biome`, or no cover for an id from
    /// another registry set.
    #[must_use]
    pub fn cover(&self, biome: BiomeId) -> [f32; 4] {
        self.covers
            .get(biome.0 as usize)
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, 0.0])
    }

    /// A table straight from layer indices, for tests and tools: slopes
    /// paint the same layer and every tint is white.
    #[must_use]
    pub fn from_layers(layers: Vec<u8>) -> Self {
        let n = layers.len();
        Self {
            slope_layers: layers.clone(),
            layers,
            tints: vec![[1.0, 1.0, 1.0]; n],
            covers: vec![[0.0, 0.0, 0.0, 0.0]; n],
            slope_thresholds: vec![slope_of_rise(0.7); n],
            slope_rises: vec![0.7; n],
            provinces: Vec::new(),
            beds: [None, None],
            ways: [None, None, None],
        }
    }

    /// The layer of `biome`; layer 0 for an id this table does not know,
    /// which only a registry set other than the one it was built from can
    /// produce.
    #[must_use]
    pub fn layer(&self, biome: BiomeId) -> u8 {
        self.layers.get(biome.0 as usize).copied().unwrap_or(0)
    }

    /// The layer steep ground of `biome` turns into.
    #[must_use]
    pub fn slope_layer(&self, biome: BiomeId) -> u8 {
        self.slope_layers
            .get(biome.0 as usize)
            .copied()
            .unwrap_or(0)
    }

    /// The tint of `biome`, white when unknown.
    #[must_use]
    pub fn tint(&self, biome: BiomeId) -> [f32; 3] {
        self.tints
            .get(biome.0 as usize)
            .copied()
            .unwrap_or([1.0, 1.0, 1.0])
    }

    /// The slope, as `1 - normal.y`, past which `biome` shows its slope
    /// material.
    #[must_use]
    pub fn slope_threshold(&self, biome: BiomeId) -> f32 {
        self.slope_thresholds
            .get(biome.0 as usize)
            .copied()
            .unwrap_or_else(|| slope_of_rise(0.7))
    }

    /// The rise-over-run slope past which `biome` shows its slope material.
    #[must_use]
    pub fn slope_rise(&self, biome: BiomeId) -> f32 {
        self.slope_rises
            .get(biome.0 as usize)
            .copied()
            .unwrap_or(0.7)
    }
}

impl ProvinceLook {
    /// A province that changes nothing: a biome's own slope layer, its own
    /// threshold, its own tint. Stands in for an id the registry does not
    /// resolve, which a frozen set never produces.
    pub const NEUTRAL: Self = Self {
        face: 0,
        bare_slope: 1.0,
        far_tint: [1.0, 1.0, 1.0],
    };
}

/// A rise-over-run slope as the `1 - normal.y` the shader compares
/// against: the normal of a slope with tangent `rise` has `y = 1 /
/// sqrt(1 + rise^2)`.
#[must_use]
pub fn slope_of_rise(rise: f32) -> f32 {
    1.0 - 1.0 / (1.0 + rise * rise).sqrt()
}

/// A built region: the vertex streams and what the collector needs to know
/// to place and cull it. Plain vectors rather than a GPU layout so the
/// builder and its tests never touch one; [`crate::store::pack_region`] is
/// the one conversion, and the store's tests hold the shader's unpacking of
/// it to these streams.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionMesh {
    /// Which region this is.
    pub coord: RegionCoord,
    /// Metres from the region's minimum corner.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals from the sampled heights.
    pub normals: Vec<[f32; 3]>,
    /// World `(x, z)` in metres wrapped to [`UV_PERIOD_M`], the texture
    /// anchor. The wrap is one offset per region ([`uv_wrap_base`]), so the
    /// UVs inside a region are continuous.
    pub uvs: Vec<[f32; 2]>,
    /// The share of every layer per vertex, in 255ths summing to 255.
    pub weights: Vec<[u8; MAX_LAYERS]>,
    /// `[pair, wild_share, hi_share, corner]` per vertex: the two rock
    /// layers steep ground bares, packed by [`pack_rock_pair`] lower layer
    /// first; how much of the rock steep ground may take (none on a
    /// settlement's laid ground); the share of the higher layer of the
    /// two in 255ths; and the vertex's corner, [`corner_of`]. See the
    /// module docs, "Rock".
    pub slope: Vec<[u8; 4]>,
    /// `[parent_height, parent_normal_x, parent_normal_z, node_size]` per
    /// vertex: the LOD morph target.
    pub morph: Vec<[f32; 4]>,
    /// Blended biome tint per vertex, linear RGB, times the provinces' far
    /// tint on wild ground, and in `w` the slope as `1 - normal.y` at which
    /// rock bares: the dominant biome's threshold times the provinces'
    /// `bare_slope`.
    pub tints: Vec<[f32; 4]>,
    /// The factor of [`tints`](Self::tints) that is the provinces' far tint
    /// on wild ground, per vertex, linear RGB: the tint over it is the
    /// biomes' own ground colour, which the near ground's palette is pulled
    /// to (ticket 0006), while the far ground takes the whole tint, so a
    /// sandstone country is red from afar and its grass still green at
    /// the feet.
    pub far_factors: Vec<[f32; 3]>,
    /// Blended canopy colour per vertex, linear RGB, and in `w` the cover.
    pub covers: Vec<[f32; 4]>,
    /// `[visibility, bent_x, bent_z, wet]` per vertex in 255ths, the bent
    /// normal's components mapped from `-1..1`: the share of the sky the
    /// vertex sees and the mean direction it sees it in (the module docs,
    /// "Sky"); and how wet the water's edge is ([`Bank::wet`]), in a byte
    /// the sky's three left spare.
    pub sky: Vec<[u8; 4]>,
    /// The same at the vertex's point on the parent level's mesh, the
    /// morph's target for [`sky`](Self::sky), but in its fourth byte how
    /// much of a cut wall the vertex is ([`Bank::wall`]), which does not
    /// morph.
    pub sky_parent: Vec<[u8; 4]>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Lowest surface height in the grid, metres, morph targets included.
    pub min_height: f32,
    /// Highest surface height in the grid, metres, morph targets included.
    pub max_height: f32,
    /// How far below the edge the skirt reaches, metres.
    pub skirt_depth: f32,
}

impl RegionMesh {
    /// The corners of a box that contains every vertex, skirts included, in
    /// the mesh's own frame. Heights vary, so this comes from the samples
    /// and not from the region's footprint.
    #[must_use]
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let size = self.coord.size() as f32;
        (
            [0.0, self.min_height - self.skirt_depth, 0.0],
            [size, self.max_height, size],
        )
    }
}

/// The sampled grid with its rings, addressed in grid coordinates that may
/// run `-RING` past either side.
struct Grid<'a> {
    samples: &'a [Sample],
}

impl Grid<'_> {
    fn at(&self, i: isize, j: isize) -> &Sample {
        let n = SAMPLE_N as isize;
        let (ii, jj) = (i + RING as isize, j + RING as isize);
        debug_assert!((0..n).contains(&ii) && (0..n).contains(&jj));
        &self.samples[(jj * n + ii) as usize]
    }

    fn height(&self, i: isize, j: isize) -> f32 {
        self.at(i, j).height
    }

    /// The central-difference normal at `(i, j)` over `span` steps of
    /// `step` metres each.
    fn normal(&self, i: isize, j: isize, span: isize, step: f32) -> [f32; 3] {
        let inv = 1.0 / (2.0 * span as f32 * step);
        let dx = (self.height(i + span, j) - self.height(i - span, j)) * inv;
        let dz = (self.height(i, j + span) - self.height(i, j - span)) * inv;
        unit([-dx, 1.0, -dz])
    }

    /// Where `(i, j)` sits on the parent LOD's mesh: its height and normal.
    ///
    /// An even vertex is a parent vertex. An odd vertex on one axis is the
    /// midpoint of the parent edge between its even neighbours. A vertex
    /// odd on both lies on the parent cell's diagonal, which the index
    /// buffer runs from `(i + 1, j - 1)` to `(i - 1, j + 1)`.
    fn parent(&self, i: isize, j: isize, step: f32) -> (f32, [f32; 3]) {
        let (pi, pj) = (i.rem_euclid(2) == 1, j.rem_euclid(2) == 1);
        let pair = |a: (isize, isize), b: (isize, isize)| {
            let h = f32::midpoint(self.height(a.0, a.1), self.height(b.0, b.1));
            let na = self.normal(a.0, a.1, 2, step);
            let nb = self.normal(b.0, b.1, 2, step);
            (h, unit([na[0] + nb[0], na[1] + nb[1], na[2] + nb[2]]))
        };
        match (pi, pj) {
            (false, false) => (self.height(i, j), self.normal(i, j, 2, step)),
            (true, false) => pair((i - 1, j), (i + 1, j)),
            (false, true) => pair((i, j - 1), (i, j + 1)),
            (true, true) => pair((i + 1, j - 1), (i - 1, j + 1)),
        }
    }
}

/// Builds the mesh of `coord` from `source`.
///
/// `skirt_fraction` is the skirt depth as a fraction of the region's side,
/// so a coarse node, whose neighbours can undercut it by more, gets a
/// deeper skirt. `scratch` receives the samples and is reused across calls
/// on the same task so a busy pool does not reallocate per region.
#[must_use]
pub fn build_region_mesh(
    coord: RegionCoord,
    source: &dyn WorldSource,
    layers: &BiomeLayers,
    skirt_fraction: f32,
    scratch: &mut Vec<Sample>,
) -> RegionMesh {
    build_region_mesh_pinned(coord, source, layers, skirt_fraction, false, scratch)
}

/// [`build_region_mesh`], with its LOD morph `pinned` off: every vertex's
/// morph target is its own height and normal, so it never moves towards the
/// parent grid. For a region a detail zone keeps finer than its distance
/// would (`strata_stream::DetailZone`): morphed, it would draw the coarse
/// grid the zone exists to avoid. Its skirts hide the seam with an unpinned
/// neighbour.
#[must_use]
pub fn build_region_mesh_pinned(
    coord: RegionCoord,
    source: &dyn WorldSource,
    layers: &BiomeLayers,
    skirt_fraction: f32,
    pinned: bool,
    scratch: &mut Vec<Sample>,
) -> RegionMesh {
    build_region_mesh_painted(
        coord,
        source,
        layers,
        (skirt_fraction, pinned),
        &crate::paint::PaintedWays::default(),
        scratch,
    )
}

/// [`build_region_mesh_pinned`] for a region whose fragments paint the
/// roads in `ways` (`(skirt fraction, pinned)` as there): their ground is
/// wild in the layer shares, the rock and the tint, so the splat does not
/// draw them a second time as a blur (the module docs, "Roads at
/// distance").
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one pass over the grid, then the skirts and the indices"
)]
pub fn build_region_mesh_painted(
    coord: RegionCoord,
    source: &dyn WorldSource,
    layers: &BiomeLayers,
    (skirt_fraction, pinned): (f32, bool),
    ways: &crate::paint::PaintedWays,
    scratch: &mut Vec<Sample>,
) -> RegionMesh {
    let step = coord.vertex_spacing();
    let min = coord.min_corner();
    source.sample_grid(
        min.x() - RING as f64 * step,
        min.z() - RING as f64 * step,
        step,
        SAMPLE_N,
        scratch,
    );
    let samples: &[Sample] = scratch;
    debug_assert_eq!(samples.len(), SAMPLE_N * SAMPLE_N);
    let grid = Grid { samples };

    let size = coord.size() as f32;
    let skirt_depth = (size * skirt_fraction).max(1.0);
    let step32 = step as f32;
    let (uv_base_x, uv_base_z) = uv_wrap_base(coord);

    let mut positions = Vec::with_capacity(VERTEX_COUNT);
    let mut normals = Vec::with_capacity(VERTEX_COUNT);
    let mut uvs = Vec::with_capacity(VERTEX_COUNT);
    let mut weights = Vec::with_capacity(VERTEX_COUNT);
    let mut slope = Vec::with_capacity(VERTEX_COUNT);
    // How much of each sample is a road the paint draws instead.
    let painted: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let form = &s.landform;
            if ways.is_empty() || form.cleared <= 0.0 || form.developed <= 0.0 {
                return 0.0;
            }
            let (i, j) = (k % SAMPLE_N, k / SAMPLE_N);
            let at = |n: usize| (n as f32 - RING as f32) * step as f32;
            form.cleared.clamp(0.0, 1.0) * ways.share([at(i), at(j)])
        })
        .collect();
    let shares: Vec<[f32; MAX_LAYERS]> = samples
        .iter()
        .zip(&painted)
        .map(|(s, &way)| shares_of(s, layers, way))
        .collect();
    let made_all: Vec<f32> = samples.iter().map(made).collect();
    let mut morph = Vec::with_capacity(VERTEX_COUNT);
    let mut tints = Vec::with_capacity(VERTEX_COUNT);
    let mut far_factors = Vec::with_capacity(VERTEX_COUNT);
    let mut covers = Vec::with_capacity(VERTEX_COUNT);
    let mut min_height = f32::INFINITY;
    let mut max_height = f32::NEG_INFINITY;

    for j in 0..GRID_N {
        for i in 0..GRID_N {
            let (ii, jj) = (i as isize, j as isize);
            let s = grid.at(ii, jj);
            let way = painted[(j + RING) * SAMPLE_N + i + RING];
            let height = s.height;
            let (parent_height, parent_normal) = if pinned {
                (height, grid.normal(ii, jj, 1, step32))
            } else {
                grid.parent(ii, jj, step32)
            };
            min_height = min_height.min(height).min(parent_height);
            max_height = max_height.max(height).max(parent_height);
            // Grid offsets are small integers times a power-of-two step.
            let (x, z) = (i as f32 * step32, j as f32 * step32);
            positions.push([x, height, z]);
            normals.push(grid.normal(ii, jj, 1, step32));
            // Offsets from the wrap base are a few thousand metres at most,
            // so the cast keeps sub-millimetre precision anywhere in the
            // world (see UV_PERIOD_M in material.rs).
            uvs.push([
                (min.x() - uv_base_x + i as f64 * step) as f32,
                (min.z() - uv_base_z + j as f64 * step) as f32,
            ]);
            weights.push(filtered_weights(&shares, ii, jj));
            slope.push(slope_of(
                s,
                layers,
                corner_of(i, j),
                (min.x() + i as f64 * step, min.z() + j as f64 * step),
                made_near(&made_all, ii, jj),
                way,
            ));
            morph.push([parent_height, parent_normal[0], parent_normal[2], size]);
            let (tint, far_factor) = tint_of(s, layers, way);
            tints.push(tint);
            far_factors.push(far_factor);
            covers.push(cover_of(s, layers));
        }
    }

    // Skirts: each edge in order, every edge vertex copied and dropped,
    // its morph target dropped with it so the wall follows the edge.
    let (mut sky, mut sky_parent) = bake_sky(coord, source, &grid, pinned);
    // The water's edge in the sky's spare bytes: how wet, and how much of a
    // cut wall (the module docs, "Water's edge").
    for j in 0..GRID_N {
        for i in 0..GRID_N {
            let bank = bank_of(grid.at(i as isize, j as isize), layers);
            let v = j * GRID_N + i;
            sky[v][3] = unit_byte(bank.wet);
            sky_parent[v][3] = unit_byte(bank.wall);
        }
    }

    let edges = edge_indices();
    for (e, edge) in edges.iter().enumerate() {
        for &v in edge {
            let v = v as usize;
            sky.push(sky[v]);
            sky_parent.push(sky_parent[v]);
            let p = positions[v];
            positions.push([p[0], p[1] - skirt_depth, p[2]]);
            normals.push(normals[v]);
            uvs.push(uvs[v]);
            weights.push(weights[v]);
            let mut rock = slope[v];
            rock[3] = skirt_corner(rock[3], e);
            slope.push(rock);
            let m = morph[v];
            morph.push([m[0] - skirt_depth, m[1], m[2], m[3]]);
            tints.push(tints[v]);
            far_factors.push(far_factors[v]);
            covers.push(covers[v]);
        }
    }

    let mut indices = Vec::with_capacity(INDEX_COUNT);
    let n = GRID_N as u32;
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = j * n + i;
            let b = a + 1;
            let c = a + n;
            let d = c + 1;
            // Counter-clockwise seen from above (+y), Bevy's front face.
            // The diagonal runs b to c; `Grid::parent` relies on that.
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    for (e, edge) in edges.iter().enumerate() {
        let skirt_base = (GRID_VERTICES + e * GRID_N) as u32;
        let outward = EDGE_OUTWARD[e];
        for k in 0..GRID_N - 1 {
            let e0 = edge[k];
            let e1 = edge[k + 1];
            let s0 = skirt_base + k as u32;
            let s1 = s0 + 1;
            push_outward_quad(&mut indices, &positions, [e0, e1, s1, s0], outward);
        }
    }
    debug_assert_eq!(positions.len(), VERTEX_COUNT);
    debug_assert_eq!(indices.len(), INDEX_COUNT);

    RegionMesh {
        coord,
        positions,
        normals,
        uvs,
        weights,
        slope,
        morph,
        tints,
        far_factors,
        covers,
        sky,
        sky_parent,
        indices,
        min_height,
        max_height,
        skirt_depth,
    }
}

/// The shortest ray unit, metres. A finer one would cost the finest level
/// more lattice samples than its grid for relief its normal maps already
/// draw, and it makes the two finest levels the same function, so their
/// morph changes nothing.
const SKY_MIN_UNIT_M: f64 = 2.0;

/// The eight azimuths, as whole steps along each axis: the axes and the
/// diagonals, so every point a ray reads is a whole grid step and the
/// lattice reads are exact.
const SKY_AZIMUTHS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// Where an axis ray reads the ground, in ray units, near to far. The far
/// lattice's samples are most of the bake's cost; out to 32 units they
/// were 280 a region against 496 out to 36 with a near reach of 12, for
/// an image that differs in the gullies' last few metres.
const SKY_AXIS_REACH: [i32; 3] = [4, 8, 32];

/// Where a diagonal ray reads it, in ray units along each axis: 4.2, 8.5
/// and 32.5 units out, as near the axes' reach as whole steps come.
const SKY_DIAGONAL_REACH: [i32; 3] = [3, 6, 23];

/// The near lattice's spacing in ray units: the first two reaches read it.
const SKY_NEAR_SPACING: i32 = 4;

/// The far lattice's spacing in ray units: the last reach reads it.
const SKY_FAR_SPACING: i32 = 16;

/// A square of ground heights at a whole number of grid steps apart,
/// aligned to the world, in the region's grid coordinates.
struct Lattice {
    /// The first node on both axes, steps from the region's corner; a
    /// multiple of `spacing`.
    origin: i32,
    /// Steps between nodes, a power of two.
    spacing: i32,
    /// Its base-two logarithm: the reads shift rather than divide.
    shift: u32,
    /// Nodes per side.
    n: i32,
    /// Row-major, `x` fastest.
    heights: Vec<f32>,
}

impl Lattice {
    /// Nodes `spacing` steps apart covering the grid and `reach` steps
    /// around it, unsampled.
    fn covering(spacing: i32, reach: i32) -> Self {
        let lo = (-reach).div_euclid(spacing) * spacing;
        let hi = -(-(GRID_N as i32 - 1 + reach)).div_euclid(spacing) * spacing;
        debug_assert!(spacing > 0 && spacing.count_ones() == 1);
        Self {
            origin: lo,
            spacing,
            shift: spacing.trailing_zeros(),
            n: (hi - lo) / spacing + 1,
            heights: Vec::new(),
        }
    }

    /// The height of the node at `(x, z)` steps, if this lattice has one
    /// there.
    fn node(&self, x: i32, z: i32) -> Option<f32> {
        let (dx, dz) = (x - self.origin, z - self.origin);
        if dx < 0 || dz < 0 || dx % self.spacing != 0 || dz % self.spacing != 0 {
            return None;
        }
        let (a, b) = (dx / self.spacing, dz / self.spacing);
        (a < self.n && b < self.n).then(|| self.heights[(b * self.n + a) as usize])
    }

    /// Fills the heights: from the grid's samples where it has them, from
    /// `others` where they have the node, and from the world elsewhere.
    fn fill(
        &mut self,
        coord: RegionCoord,
        source: &dyn WorldSource,
        grid: &Grid<'_>,
        others: &[&Lattice],
    ) {
        let step = coord.vertex_spacing();
        let min = coord.min_corner();
        let sampled = -(RING as i32)..=GRID_N as i32 - 1 + RING as i32;
        self.heights.clear();
        self.heights.reserve((self.n * self.n) as usize);
        for b in 0..self.n {
            for a in 0..self.n {
                let (x, z) = (
                    self.origin + a * self.spacing,
                    self.origin + b * self.spacing,
                );
                let height = if sampled.contains(&x) && sampled.contains(&z) {
                    grid.height(x as isize, z as isize)
                } else if let Some(h) = others.iter().find_map(|l| l.node(x, z)) {
                    h
                } else {
                    // The same expression as the grid's own positions, so a
                    // node is the same sample whichever region asks.
                    source
                        .sample(min.x() + f64::from(x) * step, min.z() + f64::from(z) * step)
                        .height
                };
                self.heights.push(height);
            }
        }
    }

    /// The height at whole steps `(x, z)`, bilinear between the nodes.
    /// Whole steps over a power-of-two spacing make the weights exact, so
    /// two regions reading one world point get the same bits.
    fn height(&self, x: i32, z: i32) -> f32 {
        let (dx, dz) = (x - self.origin, z - self.origin);
        // Floor division and its remainder by a power of two.
        let (a, b) = (dx >> self.shift, dz >> self.shift);
        let mask = self.spacing - 1;
        let s = self.spacing as f32;
        let u = (dx & mask) as f32 / s;
        let v = (dz & mask) as f32 / s;
        let last = self.n - 1;
        let (a0, a1) = (a.clamp(0, last), (a + 1).clamp(0, last));
        let (b0, b1) = (b.clamp(0, last), (b + 1).clamp(0, last));
        let h = |a: i32, b: i32| self.heights[(b * self.n + a) as usize];
        (h(a0, b0) * (1.0 - u) + h(a1, b0) * u) * (1.0 - v)
            + (h(a0, b1) * (1.0 - u) + h(a1, b1) * u) * v
    }
}

/// One azimuth of a level's rays, worked out once per region.
#[derive(Clone, Copy)]
struct Azimuth {
    /// Whole steps per reach along `x` and `z`: the direction.
    step: (i32, i32),
    /// The unit direction.
    dir: (f32, f32),
    /// Each reach's offset, in whole steps along each axis.
    offsets: [i32; 3],
    /// One over each reach's distance, metres.
    inv_metres: [f32; 3],
}

/// One level's rays: its ray unit and the two lattices they read.
struct SkyRays {
    /// Grid steps per ray unit.
    unit: i32,
    /// Metres per grid step.
    step_m: f32,
    near: Lattice,
    far: Lattice,
    azimuths: [Azimuth; 8],
}

impl SkyRays {
    /// The lattices for a ray unit of `unit` steps, unsampled.
    fn new(unit: i32, step_m: f32) -> Self {
        let reach = |r: [i32; 3]| r.iter().copied().max().unwrap_or(0);
        let near_reach = reach([SKY_AXIS_REACH[1], SKY_DIAGONAL_REACH[1], 0]);
        let far_reach = reach([SKY_AXIS_REACH[2], SKY_DIAGONAL_REACH[2], 0]);
        let azimuths = SKY_AZIMUTHS.map(|(ax, az)| {
            let diagonal = ax != 0 && az != 0;
            let (reach, along) = if diagonal {
                (SKY_DIAGONAL_REACH, core::f32::consts::SQRT_2)
            } else {
                (SKY_AXIS_REACH, 1.0)
            };
            let offsets = reach.map(|n| n * unit);
            Azimuth {
                step: (ax, az),
                dir: (ax as f32 / along, az as f32 / along),
                offsets,
                // The same product whichever level works it out: a whole
                // number of steps times a power-of-two step is exact.
                inv_metres: offsets.map(|off| 1.0 / (off as f32 * step_m * along)),
            }
        });
        Self {
            unit,
            step_m,
            near: Lattice::covering(SKY_NEAR_SPACING * unit, near_reach * unit),
            far: Lattice::covering(SKY_FAR_SPACING * unit, far_reach * unit),
            azimuths,
        }
    }

    /// The sky at grid point `(i, j)` of height `h0` and unit `normal`, as
    /// the attribute's bytes. See the module docs, "Sky".
    fn at(&self, i: i32, j: i32, h0: f32, normal: [f32; 3]) -> [u8; 4] {
        // The cosine-weighted sky left above a rise of `t`: cos^2 of its
        // elevation.
        let open = |t: f32| {
            let t = t.max(0.0);
            1.0 / (1.0 + t * t)
        };
        let inv_ny = 1.0 / normal[1].max(1e-3);
        let mut seen = 0.0_f32;
        let mut bent = [0.0_f32; 3];
        for a in &self.azimuths {
            let (dx, dz) = a.dir;
            // The tangent plane's own rise along the azimuth.
            let plane = -(normal[0] * dx + normal[2] * dz) * inv_ny;
            let mut rise = plane;
            for k in 0..3 {
                let off = a.offsets[k];
                let (x, z) = (i + a.step.0 * off, j + a.step.1 * off);
                let lattice = if k < 2 { &self.near } else { &self.far };
                rise = rise.max((lattice.height(x, z) - h0) * a.inv_metres[k]);
            }
            let share = (open(rise) / open(plane)).min(1.0);
            seen += share;
            // The sector's sky seen in the middle of what is open of it: the
            // bisector of its horizon and the zenith, whose length is
            // sqrt(2 + 2 sin e).
            let t = rise.max(0.0);
            let c = 1.0 / (1.0 + t * t).sqrt();
            let w = share / (2.0 + 2.0 * t * c).sqrt();
            bent[0] += w * dx * c;
            bent[1] += w * (t * c + 1.0);
            bent[2] += w * dz * c;
        }
        let visibility = seen / self.azimuths.len() as f32;
        let bent = unit(bent);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [
            byte(visibility),
            byte(bent[0] * 0.5 + 0.5),
            byte(bent[2] * 0.5 + 0.5),
            0,
        ]
    }
}

impl SkyRays {
    /// The sky of every grid vertex as the level `levels` above this one
    /// draws it: rays from the vertices that level has (every `2^levels`th
    /// of this grid's, the even ones of the level drawn, which may be this
    /// one's parent), each with its plane over one ray unit, then the
    /// vertices between them filled down level by level along the edges and
    /// the diagonals of each level's triangles, as the rasteriser and
    /// [`Grid::parent`] place values between them.
    fn evaluate(&self, grid: &Grid<'_>, levels: u32) -> Vec<[u8; 4]> {
        let n = GRID_N as i32;
        let span = self.unit as isize;
        let mut sky = vec![[0_u8; 4]; GRID_VERTICES];
        let top = 1_i32 << levels;
        for j in (0..n).step_by(top as usize) {
            for i in (0..n).step_by(top as usize) {
                let (ii, jj) = (i as isize, j as isize);
                let normal = grid.normal(ii, jj, span, self.step_m);
                sky[(j * n + i) as usize] = self.at(i, j, grid.height(ii, jj), normal);
            }
        }
        let mid = |a: [u8; 4], b: [u8; 4]| -> [u8; 4] {
            std::array::from_fn(|k| u16::midpoint(u16::from(a[k]), u16::from(b[k])) as u8)
        };
        let mut s = top / 2;
        while s >= 1 {
            for j in (0..n).step_by(s as usize) {
                for i in (0..n).step_by(s as usize) {
                    let (odd_i, odd_j) = ((i / s) % 2 == 1, (j / s) % 2 == 1);
                    let at = |i: i32, j: i32| sky[(j * n + i) as usize];
                    let value = match (odd_i, odd_j) {
                        (false, false) => continue,
                        (true, false) => mid(at(i - s, j), at(i + s, j)),
                        (false, true) => mid(at(i, j - s), at(i, j + s)),
                        (true, true) => mid(at(i + s, j - s), at(i - s, j + s)),
                    };
                    sky[(j * n + i) as usize] = value;
                }
            }
            s /= 2;
        }
        sky
    }
}

/// Grid steps per ray unit at a vertex spacing of `step` metres.
fn sky_unit(step: f64) -> i32 {
    ((SKY_MIN_UNIT_M / step).ceil() as i32).max(1)
}

/// The sky of every grid vertex and its morph target.
///
/// A level evaluates its even vertices alone and interpolates the odd ones
/// along the edge or the diagonal their triangles share, as [`Grid::parent`]
/// places the height: the nearest ray reads four units out, so the term
/// changes too slowly for the odd vertices to show it, and it halves the
/// bake's time. The target is the parent level's term as the parent draws
/// it, so from the parent's own even vertices, every fourth of this grid's. The tangent plane is measured over one
/// ray unit, so the parent level's plane at an even vertex is its own and
/// the two finest levels, whose unit is the same 2 m, are one function
/// over the same lattices. Pinned, the target is the vertex's own.
fn bake_sky(
    coord: RegionCoord,
    source: &dyn WorldSource,
    grid: &Grid<'_>,
    pinned: bool,
) -> (Vec<[u8; 4]>, Vec<[u8; 4]>) {
    let step = coord.vertex_spacing();
    let step32 = step as f32;
    let mut own = SkyRays::new(sky_unit(step), step32);
    own.near.fill(coord, source, grid, &[]);
    own.far.fill(coord, source, grid, &[&own.near]);
    let sky = own.evaluate(grid, 1);
    if pinned {
        return (sky.clone(), sky);
    }
    // The parent's unit in this level's steps. At the finest levels it is
    // this one's, and so are its lattices.
    let parent_unit = 2 * sky_unit(2.0 * step);
    if parent_unit == own.unit {
        let targets = own.evaluate(grid, 2);
        return (sky, targets);
    }
    let mut parent = SkyRays::new(parent_unit, step32);
    parent
        .near
        .fill(coord, source, grid, &[&own.near, &own.far]);
    parent
        .far
        .fill(coord, source, grid, &[&own.far, &parent.near, &own.near]);
    let targets = parent.evaluate(grid, 2);
    (sky, targets)
}

/// The world `(x, z)` that a region's UVs are measured from: the region's
/// corner rounded down to a multiple of [`UV_PERIOD_M`]. Every vertex of the
/// region shares it, so UVs are continuous inside the region, and it is a
/// whole number of periods, so neighbouring regions with different bases
/// still sample the same texels.
#[must_use]
pub fn uv_wrap_base(coord: RegionCoord) -> (f64, f64) {
    let period = f64::from(UV_PERIOD_M);
    let min = coord.min_corner();
    (
        (min.x() / period).floor() * period,
        (min.z() / period).floor() * period,
    )
}

/// The corner of grid vertex `(i, j)`: 0, 1 or 2, different at the three
/// vertices of every grid triangle.
///
/// The triangles are `(i, j), (i, j + 1), (i + 1, j)` and `(i + 1, j),
/// (i, j + 1), (i + 1, j + 1)` (the diagonal runs from `b` to `c`), whose
/// values of `i - j` are `x, x - 1, x + 1` and `x + 1, x - 1, x`: three
/// residues mod 3 each.
#[must_use]
pub fn corner_of(i: usize, j: usize) -> u8 {
    ((i as isize - j as isize).rem_euclid(3)) as u8
}

/// The corner of the skirt vertex hung below an edge vertex of corner
/// `corner` on edge `edge` ([`edge_indices`] order).
///
/// A skirt quad `e0, e1, s1, s0` is the triangles `e0, e1, s1` and `e0, s1,
/// s0`. Along a row the edge's corners step by one (`i` rises), along a
/// column by minus one (`j` rises); with that step `d`, a skirt vertex at
/// its edge vertex's corner plus `d` makes both triangles `x, x + d, x +
/// 2d`, three residues. Skirts face outward and are rarely seen, but a
/// triangle with two vertices of one corner would decode its rock wrongly.
fn skirt_corner(corner: u8, edge: usize) -> u8 {
    let step = if edge < 2 { 1 } else { 2 };
    (corner + step) % 3
}

/// Horizontal outward direction of each edge in [`edge_indices`] order:
/// south (`-z`), north (`+z`), west (`-x`), east (`+x`).
const EDGE_OUTWARD: [[f32; 3]; 4] = [
    [0.0, 0.0, -1.0],
    [0.0, 0.0, 1.0],
    [-1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
];

/// The grid vertex indices along each edge: south row, north row, west
/// column, east column, each in increasing `i` or `j`.
fn edge_indices() -> [[u32; GRID_N]; 4] {
    let n = GRID_N as u32;
    let mut edges = [[0u32; GRID_N]; 4];
    for (k, cols) in (0..n).zip(0..GRID_N) {
        edges[0][cols] = k;
        edges[1][cols] = (n - 1) * n + k;
        edges[2][cols] = k * n;
        edges[3][cols] = k * n + (n - 1);
    }
    edges
}

/// Two triangles for the quad `q` (a loop of four vertices), wound so the
/// face normal points along `outward`. Computing the winding from the
/// geometry rather than per edge by hand means the four edges share one
/// rule and a test can check the rule instead of four cases.
fn push_outward_quad(
    indices: &mut Vec<u32>,
    positions: &[[f32; 3]],
    q: [u32; 4],
    outward: [f32; 3],
) {
    let [a, b, c, d] = q;
    let normal = triangle_normal(positions, a, b, c);
    let facing = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];
    if facing >= 0.0 {
        indices.extend_from_slice(&[a, b, c, a, c, d]);
    } else {
        indices.extend_from_slice(&[a, c, b, a, d, c]);
    }
}

/// The (unnormalised) normal of triangle `(a, b, c)`, counter-clockwise.
fn triangle_normal(positions: &[[f32; 3]], a: u32, b: u32, c: u32) -> [f32; 3] {
    let p = positions[a as usize];
    let q = positions[b as usize];
    let r = positions[c as usize];
    let u = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let v = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// The share of each layer at one sample, summing to one.
///
/// Wild ground is its biomes' layers by the biomes' weights. Developed
/// ground (a settlement's, a road's) carries its own material, which no
/// biome names, and takes it over the wild layers as it becomes developed,
/// so a city's paving fades into the hills across its falloff.
fn shares_of(sample: &Sample, layers: &BiomeLayers, way: f32) -> [f32; MAX_LAYERS] {
    let [(biome_a, weight_a), (biome_b, weight_b)] = sample.biomes;
    let total = (weight_a + weight_b).max(1e-6);
    let wild = wildness(sample, way);
    let bank = bank_of(sample, layers);
    let mut shares = [0.0; MAX_LAYERS];
    let biomes = wild * (1.0 - bank.share);
    shares[layer_slot(layers.layer(biome_a))] += biomes * weight_a / total;
    shares[layer_slot(layers.layer(biome_b))] += biomes * weight_b / total;
    if let Some(bed) = bank.bed {
        shares[layer_slot(bed)] += wild * bank.share;
    }
    shares[layer_slot(sample.material)] += 1.0 - wild;
    shares
}

/// What the water makes of the ground at a sample (the module docs,
/// "Water's edge"): the share of the bed's material, the layer, how wet the
/// ground is and how much of a cut wall, each from zero to one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bank {
    /// The share of the bed's material, which the rock gives way to as well.
    pub share: f32,
    /// The bed's layer; `None` where no water is near or the registry has
    /// no such material.
    pub bed: Option<u8>,
    /// One under water and at the water line, nothing a metre over it but
    /// the wet bank's damp soil (under half the line's), which fades
    /// by two metres over.
    pub wet: f32,
    /// One on a river valley's wall the carve cut, well over its river.
    pub wall: f32,
}

/// The water's edge at a sample: by a river's channel or anywhere round a
/// lake, the band of ground just over its surface.
#[must_use]
pub fn bank_of(sample: &Sample, layers: &BiomeLayers) -> Bank {
    let Some(water) = sample.water else {
        return Bank::default();
    };
    let above = sample.height - water.surface_m;
    let (near, damp_near, wall) = if water.kind == WaterKind::Lake {
        (1.0, 1.0, 0.0)
    } else {
        let bank = water.bank_m();
        let out = water.signed_distance_m.abs();
        (
            1.0 - smoothstep(bank, bank + EDGE_REACH_M, out),
            1.0 - smoothstep(
                bank + EDGE_REACH_M,
                bank + EDGE_REACH_M + WET_BANK_REACH_M,
                out,
            ),
            smoothstep(WALL_BAND_M.0, WALL_BAND_M.1, above)
                * smoothstep(WALL_CUT_M.0, WALL_CUT_M.1, water.cut_m())
                * smoothstep(WALL_SLOPE.0, WALL_SLOPE.1, sample.landform.slope),
        )
    };
    // The damp soil past the wet line, by the water's own ground only: a
    // road's way across a ford or over a culvert's fill is the road's.
    let damp = WET_BANK_SHARE
        * damp_near
        * (1.0 - smoothstep(WET_BANK_M.0, WET_BANK_M.1, above))
        * (1.0 - sample.landform.cleared);
    let bed = layers.bed(water.kind);
    Bank {
        share: if bed.is_some() {
            near * (1.0 - smoothstep(WET_BAND_M.0, WET_BAND_M.1, above))
        } else {
            0.0
        },
        bed,
        wet: (near * (1.0 - smoothstep(0.0, DAMP_M, above))).max(damp),
        wall,
    }
}

/// Over this band of how far the ground was cut or filled, metres, a slope
/// is made ground, a road's bank or a river valley's wall, and not one the
/// land made (ticket 0005, step 6): low, so a bank's shoulder, cut by
/// centimetres where it rounds into the hillside but as steep as the bank,
/// is soil too and not a rock rim along every cut.
const MADE_M: (f32, f32) = (0.05, 0.6);

/// Over this band of a cut's depth, metres, its rock may show: a shallow
/// cut is a soil bank, whatever its slope. The band wanders by
/// [`MADE_ROCK_CUT_WANDER_M`] along a wall, so a channel's trench, cut to
/// one depth for kilometres, does not bare its rock at one height all along
/// it.
const MADE_ROCK_CUT_M: (f32, f32) = (5.0, 14.0);

/// How far the depth band wanders either way, metres.
const MADE_ROCK_CUT_WANDER_M: f32 = 5.0;

/// Over this band of a cut's rise over run its rock may show: a gentle bank
/// holds its soil and the land's own cover, and a road's bank at its
/// rock's slope (1.8, wandering a quarter) only where it is steepest, so an
/// outcrop on one is a face and not a disc whose beds ring a bowl.
const MADE_ROCK_SLOPE: (f32, f32) = (1.5, 2.2);

/// How long the patches of a made wall's rock are, metres: it breaks
/// through its soil in outcrops tens of metres across, not in a ruled band.
const MADE_ROCK_PATCH_M: f64 = 23.0;

/// How long the stretches of a made wall that bare any rock are, metres:
/// the outcrops come in clusters with long soil walls between, so along a
/// river they do not repeat like a row of slabs.
const MADE_ROCK_CLUSTER_M: f64 = 170.0;

/// How much of a sample is made ground, `0..=1` (ticket 0005, step 6): a
/// bank a road cut or filled, or a wall a river's valley cut. Fades in with
/// the cut, which is continuous, so it has no edge.
#[must_use]
pub fn made(sample: &Sample) -> f32 {
    let carved = sample
        .water
        .filter(|water| water.kind != WaterKind::Lake)
        .map_or(0.0, |water| water.cut_m());
    let moved = carved.max(sample.landform.cut_m.abs());
    smoothstep(MADE_M.0, MADE_M.1, moved)
}

/// How much rock a made slope may show where its ground is made by `made`
/// (its own [`made`], or more, a neighbour's), `0..=1`: all of it on
/// ground the land made; on a bank a road cut or a wall a river's valley
/// cut, only where the cut is both deep and steep, and there in patches, so
/// the rest is soil that takes the land's own ground and cover; none on a
/// road's fill, which is grassed.
#[must_use]
pub fn made_rock(sample: &Sample, made: f32, x: f64, z: f64) -> f32 {
    if made <= 0.0 {
        return 1.0;
    }
    let carved = sample
        .water
        .filter(|water| water.kind != WaterKind::Lake)
        .map_or(0.0, |water| water.cut_m());
    let cut = carved.max(sample.landform.cut_m);
    let cluster = patch(x, z, MADE_ROCK_CLUSTER_M, 0x726f_636b_0003);
    let shift = (cluster - 0.5) * 2.0 * MADE_ROCK_CUT_WANDER_M;
    let deep = smoothstep(MADE_ROCK_CUT_M.0 - shift, MADE_ROCK_CUT_M.1 - shift, cut);
    let steep = smoothstep(MADE_ROCK_SLOPE.0, MADE_ROCK_SLOPE.1, sample.landform.slope);
    // The deeper and steeper, the more of the wall the outcrops take, and
    // only within a cluster.
    let open = deep * steep * smoothstep(0.45, 0.7, cluster);
    // A wide edge, so an outcrop's rim is a fade over metres and not a
    // vertex-wide saw-tooth.
    let patches = smoothstep(
        0.6 - 0.25 * open,
        0.9 - 0.25 * open,
        patch(x, z, MADE_ROCK_PATCH_M, 0x726f_636b_0001),
    );
    1.0 - made * (1.0 - open * patches)
}

/// Value noise in `0..1`, `wavelength` and a third of it, for a made wall's
/// outcrops and their clusters. The terrain's own look, so any float is
/// fine.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "lattice indices of a world coordinate, and 24 bits of a hash"
)]
fn patch(x: f64, z: f64, wavelength: f64, salt: u64) -> f32 {
    let octave = |x: f64, z: f64, salt: u64| -> f64 {
        let (fx, fz) = (x.floor(), z.floor());
        let (tx, tz) = (x - fx, z - fz);
        let (ix, iz) = (fx as i64, fz as i64);
        let at = |a: i64, b: i64| -> f64 {
            let mut k = salt
                ^ (a as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (b as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
            k ^= k >> 29;
            k = k.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            k ^= k >> 32;
            (k >> 40) as f64 / f64::from(1_u32 << 24)
        };
        let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
        let top = at(ix, iz) + (at(ix + 1, iz) - at(ix, iz)) * sx;
        let bottom = at(ix, iz + 1) + (at(ix + 1, iz + 1) - at(ix, iz + 1)) * sx;
        top + (bottom - top) * sz
    };
    let (u, v) = (x / wavelength, z / wavelength);
    (0.7 * octave(u, v, salt) + 0.3 * octave(3.0 * u, 3.0 * v, salt ^ 0x100)) as f32
}

/// A share from zero to one as a byte.
fn unit_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// How much of a sample is still wild ground: one outside any settlement
/// or road, none where it is wholly laid. `way` of it is a road the paint
/// draws on this level (the module docs, "Roads at distance"), which is
/// wild ground here: drawn by the vertices that happen to land on it, it
/// was a dotted blur of its material a spacing or two across, and the
/// paint draws it at its own width instead.
fn wildness(sample: &Sample, way: f32) -> f32 {
    let developed = sample.landform.developed;
    if developed <= 0.0 {
        return 1.0;
    }
    let t = ((developed - 0.25) / 0.5).clamp(0.0, 1.0);
    let wild = 1.0 - t * t * (3.0 - 2.0 * t);
    1.0 - (1.0 - wild) * (1.0 - way.clamp(0.0, 1.0))
}

fn layer_slot(layer: u8) -> usize {
    usize::from(layer).min(MAX_LAYERS - 1)
}

/// The two layers of a rock pair, lower first, in one byte: a layer is
/// under [`MAX_LAYERS`], sixteen, so four bits each.
#[must_use]
pub fn pack_rock_pair(lo: u8, hi: u8) -> u8 {
    debug_assert!(usize::from(lo.max(hi)) < MAX_LAYERS);
    (lo & 0x0f) | (hi << 4)
}

/// The two layers of a byte from [`pack_rock_pair`], lower first.
#[must_use]
pub fn unpack_rock_pair(pair: u8) -> (u8, u8) {
    (pair & 0x0f, pair >> 4)
}

/// Below this barycentric weight a corner's pair is not trusted: the
/// fragment recovers it by dividing by that weight. Its share fades out
/// between this and [`DECODE_FULL`], so dropping it never steps.
pub const DECODE_FROM: f32 = 0.004;

/// The barycentric weight from which a corner counts in full.
pub const DECODE_FULL: f32 = 0.02;

/// What the fragment shader reads of a triangle's rock at one point, and
/// how it decodes it: `terrain.wgsl`'s `rock_mix` and `rock_pick`, kept
/// here so the tests can hold the scheme of the module docs ("Rock").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RockCorners {
    /// The barycentric weight of the vertex at each corner.
    pub corner: [f32; 3],
    /// Each corner's packed pair times its weight.
    pub pair: [f32; 3],
    /// Each corner's higher-layer share times its weight.
    pub share: [f32; 3],
    /// The wild share, interpolated as it always was.
    pub wild: f32,
}

impl RockCorners {
    /// The interpolated vectors at barycentric weights `bary` of a triangle
    /// whose three vertices carry the slope attributes `slopes`, as the
    /// vertex shader writes them and the rasteriser blends them.
    #[must_use]
    pub fn interpolate(slopes: [[u8; 4]; 3], bary: [f32; 3]) -> Self {
        let mut out = Self {
            corner: [0.0; 3],
            pair: [0.0; 3],
            share: [0.0; 3],
            wild: 0.0,
        };
        for (slope, b) in slopes.iter().zip(bary) {
            let k = usize::from(slope[3]).min(2);
            out.corner[k] += b;
            out.pair[k] += b * f32::from(slope[0]);
            out.share[k] += b * f32::from(slope[2]) / 255.0;
            out.wild += b * f32::from(slope[1]) / 255.0;
        }
        out
    }

    /// The weight of every layer in the rock at this point, summing to one:
    /// each corner's pair recovered from its slot, its two layers weighted
    /// by its share and the corner's barycentric weight.
    #[must_use]
    pub fn weights(&self) -> [f32; MAX_LAYERS] {
        let mut weights = [0.0f32; MAX_LAYERS];
        let mut total = 0.0;
        for k in 0..3 {
            let b = self.corner[k];
            let trust = smoothstep(DECODE_FROM, DECODE_FULL, b);
            if trust <= 0.0 {
                continue;
            }
            let (lo, hi) = unpack_rock_pair((self.pair[k] / b).round().clamp(0.0, 255.0) as u8);
            let hi_part = self.share[k].clamp(0.0, b.max(0.0));
            weights[usize::from(lo)] += trust * (b - hi_part);
            weights[usize::from(hi)] += trust * hi_part;
            total += trust * b;
        }
        if total > 0.0 {
            for w in &mut weights {
                *w /= total;
            }
        }
        weights
    }

    /// The layer a fragment draws at contact noise `contact` in `[0, 1)`:
    /// the layers stacked in index order, the one whose band of the
    /// cumulative weight the noise falls in. Where two layers weigh, that is
    /// the higher where its share exceeds `1 - contact`; the border between
    /// them is where the noise crosses the blend, ragged and moving with
    /// the weights.
    #[must_use]
    pub fn pick(&self, contact: f32) -> u8 {
        let weights = self.weights();
        let mut cumulative = 0.0;
        let mut last = 0;
        for (layer, w) in weights.iter().enumerate() {
            if *w <= 0.0 {
                continue;
            }
            cumulative += w;
            last = layer as u8;
            if contact < cumulative {
                return last;
            }
        }
        last
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The rock of one sample: the face layers of its two provinces, lower
/// layer first, and the share of the higher one. Ordered by layer rather
/// than by weight so the pair does not swap where the two weigh the same,
/// and two provinces of one face are one layer. Without geology, or for a
/// province this table does not know, the dominant biome's slope layer
/// stands in, which is what steep ground showed before provinces.
fn rock_of(sample: &Sample, layers: &BiomeLayers) -> (u8, u8, f32) {
    let [(biome_a, _), _] = sample.biomes;
    let fallback = layers.slope_layer(biome_a);
    let [(first, w1), (second, w2)] = sample.provinces;
    let a = layers.province(first).map_or(fallback, |p| p.face);
    let b = layers.province(second).map_or(a, |p| p.face);
    let (a, b) = (layer_slot(a) as u8, layer_slot(b) as u8);
    if a == b {
        return (a, a, 0.0);
    }
    let total = (w1 + w2).max(1e-6);
    if a < b {
        (a, b, w2 / total)
    } else {
        (b, a, w1 / total)
    }
}

/// The slope attribute of one sample, `[pair, wild_share, hi_share,
/// corner]`: its rock pair ([`rock_of`]), how much of it steep ground may
/// take, the share of the pair's higher layer, and the vertex's corner.
/// No rock where the ground is laid: a settlement's laid material already
/// follows the slope (its banks are classified by it), and rock breaking
/// through a ramp's gravel reads as damage. It fades back in with the wild
/// ground, so the city's banks become its hills' cliffs.
fn slope_of(
    sample: &Sample,
    layers: &BiomeLayers,
    corner: u8,
    at: (f64, f64),
    made: f32,
    way: f32,
) -> [u8; 4] {
    let (lo, hi, share) = rock_of(sample, layers);
    // The water's edge bares no rock: its bed's material takes the slope.
    let bank = bank_of(sample, layers);
    let made = made_rock(sample, made, at.0, at.1);
    [
        pack_rock_pair(lo, hi),
        (wildness(sample, way) * (1.0 - bank.share) * made * 255.0).round() as u8,
        (share.clamp(0.0, 1.0) * 255.0).round() as u8,
        corner,
    ]
}

/// How much of the ground round grid point `(i, j)` is made: the most of it
/// and its eight neighbours. A made bank's top rounds into the hillside
/// within a metre or two, where it is cut by centimetres but still steep;
/// the triangles between its last made vertex and the first natural one
/// would bare the rock along every cut's rim.
fn made_near(made: &[f32], i: isize, j: isize) -> f32 {
    let n = SAMPLE_N as isize;
    let mut most = 0.0f32;
    for dj in -1..=1isize {
        for di in -1..=1isize {
            let (ii, jj) = (i + di + RING as isize, j + dj + RING as isize);
            most = most.max(made[(jj * n + ii) as usize]);
        }
    }
    most
}

/// The layer shares at grid point `(i, j)`, tent-filtered over its eight
/// neighbours and quantised to 255ths that sum to 255.
///
/// The samples resolve a diagonal border between two laid materials as a
/// staircase of whole grid steps; filtered, the shares are a function of
/// the distance across it and their crossing runs along the border itself.
/// The neighbours come from the sampled ring, so both sides of a region
/// border filter the same samples and agree.
fn filtered_weights(shares: &[[f32; MAX_LAYERS]], i: isize, j: isize) -> [u8; MAX_LAYERS] {
    let n = SAMPLE_N as isize;
    let mut sum = [0.0f32; MAX_LAYERS];
    for dj in -1..=1isize {
        for di in -1..=1isize {
            let k = 4.0 / (1 << (di.abs() + dj.abs())) as f32;
            let (ii, jj) = (i + di + RING as isize, j + dj + RING as isize);
            let s = &shares[(jj * n + ii) as usize];
            for (acc, share) in sum.iter_mut().zip(s) {
                *acc += k * share;
            }
        }
    }
    quantise(sum.map(|w| w / 16.0))
}

/// Shares summing to one as 255ths summing to exactly 255: every share is
/// rounded and the largest takes up the rounding, so a vertex never shows
/// more or less than all of its ground.
fn quantise(shares: [f32; MAX_LAYERS]) -> [u8; MAX_LAYERS] {
    let mut out = shares.map(|w| (w * 255.0).round().clamp(0.0, 255.0) as u8);
    let total: i32 = out.iter().map(|&w| i32::from(w)).sum();
    let largest = (0..MAX_LAYERS).max_by_key(|&k| out[k]).unwrap_or(0);
    out[largest] = (i32::from(out[largest]) + 255 - total).clamp(0, 255) as u8;
    out
}

/// The two biomes' tints blended by weight, in linear RGB, with the
/// dominant biome's slope threshold in `w`.
///
/// Tints are authored as display colours against the world map, so they
/// are sRGB; the shader mixes them into a linear albedo.
/// The canopy colour and cover at a sample: the biome pair's, by share,
/// with the amount of canopy scaled by the ground's own cover so the
/// clearings placement leaves in a forest read from afar too (ADR 0006).
fn cover_of(sample: &Sample, layers: &BiomeLayers) -> [f32; 4] {
    let [(biome_a, weight_a), (biome_b, weight_b)] = sample.biomes;
    let a = layers.cover(biome_a);
    let b = layers.cover(biome_b);
    let total = (weight_a + weight_b).max(1e-6);
    let (wa, wb) = (weight_a / total, weight_b / total);
    [
        a[0] * wa + b[0] * wb,
        a[1] * wa + b[1] * wb,
        a[2] * wa + b[2] * wb,
        (a[3] * wa + b[3] * wb) * sample.cover.clamp(0.0, 1.0),
    ]
}

/// What the trees and bushes of `biome` look like from far enough that
/// they are a colour on the ground: their lush tints weighted by how much
/// ground each species covers there (its fit, its spacing, the crown of
/// its mean height), and that cover, capped at all of it.
fn canopy_of(registries: &GameRegistries, biome: BiomeId) -> [f32; 4] {
    let mut colour = [0.0f32; 3];
    let mut cover = 0.0f32;
    for id in registries.flora_ids() {
        let (Some(def), Some(fit)) = (registries.flora.get(id), registries.flora_fit(id)) else {
            continue;
        };
        if def.kind == FloraKind::Grass || def.tree.is_absent() {
            continue;
        }
        let weight = fit.weight(biome);
        if weight <= 0.0 {
            continue;
        }
        // Three quarters of the slots survive the hash threshold at weight
        // one; a crown is about a third of the height across.
        let per_m2 = weight * 0.75 / (def.spacing_m * def.spacing_m).max(1.0);
        let height = f32::midpoint(def.tree.height_m.min, def.tree.height_m.max)
            * f32::midpoint(def.size.min, def.size.max);
        let crown = core::f32::consts::PI * (height * 0.35).powi(2);
        let share = per_m2 * crown;
        let lush = def.tint_lush.0.map(srgb_to_linear);
        for c in 0..3 {
            colour[c] += lush[c] * share;
        }
        cover += share;
    }
    if cover <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    // Seen from afar a canopy is its lit leaves, which the client draws
    // brighter than the authored tint; and crowns overlap less than the
    // circles add up to, so the cover is pushed towards full where the
    // sum says the ground is mostly under trees.
    let lit = 1.4;
    [
        colour[0] / cover * lit,
        colour[1] / cover * lit,
        colour[2] / cover * lit,
        (cover * 1.6).min(1.0),
    ]
}

/// How far a parched sample leans towards its dry colour, and what that
/// colour is as a factor of the biome's own tint: more red, a little more
/// green, much less blue, which is what grass losing its chlorophyll does.
/// Leaning the biome's tint rather than mixing a fixed sand keeps every
/// biome recognisable while a valley reads greener than the ridge above it
/// (ADR 0006).
const DRY_LEAN: f32 = 0.5;
const DRY_FACTOR: [f32; 3] = [1.15, 1.05, 0.7];

/// How much lighter an eroded ridge reads than the ground around it, and a
/// gully floor darker, from afar. Past a few hundred metres the carving is
/// below a pixel as geometry, and this is what keeps a range reading as
/// eroded from across the valley: the drainage drawn in value, as a
/// painter would.
const RIDGE_VALUE: f32 = 0.14;

/// How much darker a hollow's damp ground reads from afar.
const HOLLOW_VALUE: f32 = 0.08;

/// The biomes' tint at a sample and the slope at which its rock bares, and
/// the factor of that tint which is the provinces' far tint.
///
/// The provinces' `far_tint` multiplies the colour and their `bare_slope`
/// the dominant biome's rise-over-run threshold, both blended by the
/// provinces' shares so neither steps at a border, and both on wild ground
/// only, so a settlement's own ground keeps what it had (ADR 0009).
fn tint_of(sample: &Sample, layers: &BiomeLayers, way: f32) -> ([f32; 4], [f32; 3]) {
    let [(biome_a, weight_a), (biome_b, weight_b)] = sample.biomes;
    let a = layers.tint(biome_a).map(srgb_to_linear);
    let b = layers.tint(biome_b).map(srgb_to_linear);
    let total = (weight_a + weight_b).max(1e-6);
    let (wa, wb) = (weight_a / total, weight_b / total);
    let dry = (1.0 - sample.hydration.clamp(0.0, 1.0)) * DRY_LEAN;
    let form = &sample.landform;
    let value = 1.0 + RIDGE_VALUE * form.ridge.clamp(-1.0, 1.0)
        - HOLLOW_VALUE * form.hollow.clamp(0.0, 1.0);
    let (far_tint, bare_slope) = province_factors(sample, layers);
    let wild = wildness(sample, way);
    let bare_slope = 1.0 + wild * (bare_slope - 1.0);
    let mut out = [0.0f32; 4];
    let mut factors = [1.0f32; 3];
    for c in 0..3 {
        let wet = a[c] * wa + b[c] * wb;
        let factor = 1.0 + wild * (far_tint[c] - 1.0);
        out[c] = (wet * (1.0 - dry) + wet * DRY_FACTOR[c] * dry) * value * factor;
        factors[c] = factor;
    }
    // Without geology the factor is one and this is the biome's own
    // threshold to the bit.
    out[3] = slope_of_rise(layers.slope_rise(biome_a) * bare_slope);
    (out, factors)
}

/// The provinces' far tint and `bare_slope` at a sample, blended by their
/// shares; white and one without geology.
fn province_factors(sample: &Sample, layers: &BiomeLayers) -> ([f32; 3], f32) {
    let [(first, w1), (second, w2)] = sample.provinces;
    let (Some(p), q) = (layers.province(first), layers.province(second)) else {
        return ([1.0; 3], 1.0);
    };
    let q = q.unwrap_or(p);
    let total = (w1 + w2).max(1e-6);
    let (s1, s2) = (w1 / total, w2 / total);
    if s2 <= 0.0 || p == q {
        return (p.far_tint, p.bare_slope);
    }
    (
        [0, 1, 2].map(|c| p.far_tint[c] * s1 + q.far_tint[c] * s2),
        p.bare_slope * s1 + q.bare_slope * s2,
    )
}

/// The sRGB transfer function, inverted.
fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_core::WorldPos;

    /// A tilted plane: height = `ax * x + az * z`, biome by side of `x = 0`.
    struct Plane {
        ax: f64,
        az: f64,
        /// Uniform over the plane, so a test can ask for a wet or a parched
        /// ground without a second source.
        hydration: f32,
    }

    impl WorldSource for Plane {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let height = (self.ax * x + self.az * z) as f32;
            // A 200 m blend band across x = 0 between biomes 0 and 1.
            let t = ((x + 100.0) / 200.0).clamp(0.0, 1.0) as f32;
            let biomes = if t <= 0.0 {
                [(BiomeId(0), 1.0), (BiomeId(0), 0.0)]
            } else if t >= 1.0 {
                [(BiomeId(1), 1.0), (BiomeId(1), 0.0)]
            } else if t < 0.5 {
                [(BiomeId(0), 1.0 - t), (BiomeId(1), t)]
            } else {
                [(BiomeId(1), t), (BiomeId(0), 1.0 - t)]
            };
            Sample {
                height,
                biomes,
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.5,
                hydration: self.hydration,
                cover: 0.5,
                landform: strata_worldgen::Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    /// Rough ground: a product of sines, so heights differ between LODs.
    struct Bumps;

    impl WorldSource for Bumps {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let height = (10.0 * (x * 0.37).sin() * (z * 0.23).cos() + 0.01 * x) as f32;
            Sample {
                height,
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

    /// A settlement's ground: wholly developed, laid in layer 0 on one side
    /// of a diagonal `x + z = 32` and layer 3 on the other.
    struct Laid;

    impl WorldSource for Laid {
        fn sample(&self, x: f64, z: f64) -> Sample {
            Sample {
                height: 5.0,
                biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: if x + z < 32.0 { 0 } else { 3 },
                moisture: 0.5,
                hydration: 0.5,
                cover: 0.0,
                landform: strata_worldgen::Landform {
                    developed: 1.0,
                    ..strata_worldgen::Landform::default()
                },
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    #[test]
    fn two_laid_materials_meet_along_their_border() {
        let mesh = build(RegionCoord::new(0, 0, 0), &Laid);
        // The samples lay the border as a staircase; the shares must be a
        // function of the distance across the diagonal alone, so their
        // crossing, interpolated over triangles whose diagonals run along
        // it, is the straight line itself and not a step per vertex.
        let mut across = std::collections::BTreeMap::new();
        for (v, w) in mesh.weights[..GRID_VERTICES].iter().enumerate() {
            assert_eq!(w.iter().map(|&x| u32::from(x)).sum::<u32>(), 255);
            assert_eq!(
                u32::from(w[0]) + u32::from(w[3]),
                255,
                "only the two laid layers"
            );
            let d = v % GRID_N + v / GRID_N;
            let seen = *across.entry(d).or_insert(w[3]);
            assert_eq!(seen, w[3], "layer 3's share varies along x + z = {d}");
        }
        let shares: Vec<u8> = across.values().copied().collect();
        assert!(shares.windows(2).all(|p| p[0] <= p[1]), "{shares:?}");
        assert_eq!(across[&29], 0);
        assert_eq!(across[&35], 255);
        assert!(
            across[&31] > 0 && across[&33] < 255,
            "the border is blended"
        );
    }

    #[test]
    fn three_layers_blend_where_they_meet() {
        // Layer 0 west of x = 32, layer 3 east of it and south of z = 32,
        // layer 5 east and north: a T junction, where a pair per vertex
        // showed a fan of teeth.
        struct Junction;
        impl WorldSource for Junction {
            fn sample(&self, x: f64, z: f64) -> Sample {
                Sample {
                    material: if x < 32.0 {
                        0
                    } else if z < 32.0 {
                        3
                    } else {
                        5
                    },
                    ..Laid.sample(x, z)
                }
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        let mesh = build(RegionCoord::new(0, 0, 0), &Junction);
        let at = |i: usize, j: usize| mesh.weights[j * GRID_N + i];
        let middle = at(32, 32);
        assert!(
            middle[0] > 0 && middle[3] > 0 && middle[5] > 0,
            "{middle:?}"
        );
        assert_eq!(at(10, 10)[0], 255);
        assert_eq!(at(50, 10)[3], 255);
        assert_eq!(at(50, 50)[5], 255);
    }

    /// A service road's carriageway is the splat's on a level fine enough
    /// to draw it and the paint's on one whose spacing is its width or more,
    /// where it is wild ground; a road nothing paints keeps its splat, and
    /// ground the paint does not draw stays laid at every level, even where
    /// it is in the road's own material under its cleared verge (a site's
    /// gravel yard where a track meets it).
    #[test]
    fn a_coarse_level_leaves_a_carriageway_to_the_paint() {
        use crate::paint::{KIND_ROAD, PaintedWays, RegionPaint, Stroke};
        /// A carriageway 8 m wide along `z = 4` in layer 5 up to `x =
        /// 300`, and past it a yard laid in layer 6, the cleared verge
        /// running on over it.
        struct Road;
        impl WorldSource for Road {
            fn sample(&self, x: f64, z: f64) -> Sample {
                let on = (z - 4.0).abs() <= 4.0;
                let laid = x > 300.0;
                Sample {
                    material: if laid { 6 } else { 5 },
                    landform: strata_worldgen::Landform {
                        developed: if on || laid { 1.0 } else { 0.0 },
                        cleared: if on { 1.0 } else { 0.0 },
                        ..strata_worldgen::Landform::default()
                    },
                    ..Laid.sample(x, z)
                }
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        // The road's line as its region's paint holds it.
        let paint = RegionPaint {
            strokes: vec![Stroke {
                a: [-100.0, 4.0],
                b: [300.0, 4.0],
                half: [4.0, 4.0],
                flags: KIND_ROAD | (5 << 8) | (255 << 16) | (8 << 24),
            }],
            ..RegionPaint::default()
        };
        let layers = layers();
        let mut scratch = Vec::new();
        let mut build = |coord: RegionCoord, painted: bool| {
            let ways = if painted {
                PaintedWays::of(&paint, coord.vertex_spacing())
            } else {
                PaintedWays::default()
            };
            build_region_mesh_painted(coord, &Road, &layers, (0.05, false), &ways, &mut scratch)
        };
        // Level 1 (2 m): the carriageway's vertices are the road's layer.
        let fine = build(RegionCoord::new(0, 0, 1), true);
        assert_eq!(fine.weights[2 * GRID_N + 10][5], 255);
        // Level 3 (8 m) and up: the vertex on the road is its biome's.
        for lod in [3, 5] {
            let coarse = build(RegionCoord::new(0, 0, lod), true);
            assert_eq!(coarse.weights[5][5], 0, "lod {lod}");
            assert_eq!(coarse.weights[5][4], 255, "lod {lod}");
            // The yard is laid, on the verge as off it.
            assert_eq!(coarse.weights[60][6], 255, "lod {lod}");
            assert_eq!(coarse.weights[3 * GRID_N + 60][6], 255, "lod {lod}");
        }
        // Unpainted, the coarse level keeps the road: the vertex on it is
        // half its layer, the filter taking the rows either side.
        let unpainted = build(RegionCoord::new(0, 0, 5), false);
        assert!(unpainted.weights[5][5] > 100, "{:?}", unpainted.weights[5]);
    }

    fn layers() -> BiomeLayers {
        // Biome 0 paints layer 4, biome 1 paints layer 1: the pair must be
        // reordered.
        BiomeLayers::from_layers(vec![4, 1])
    }

    fn build(coord: RegionCoord, source: &dyn WorldSource) -> RegionMesh {
        let mut scratch = Vec::new();
        build_region_mesh(coord, source, &layers(), 0.05, &mut scratch)
    }

    fn flat() -> Plane {
        Plane {
            ax: 0.0,
            az: 0.0,
            hydration: 1.0,
        }
    }

    #[test]
    fn counts_match_the_layout() {
        let mesh = build(RegionCoord::new(0, 0, 0), &flat());
        assert_eq!(mesh.positions.len(), VERTEX_COUNT);
        assert_eq!(mesh.normals.len(), VERTEX_COUNT);
        assert_eq!(mesh.uvs.len(), VERTEX_COUNT);
        assert_eq!(mesh.weights.len(), VERTEX_COUNT);
        assert_eq!(mesh.slope.len(), VERTEX_COUNT);
        assert_eq!(mesh.morph.len(), VERTEX_COUNT);
        assert_eq!(mesh.tints.len(), VERTEX_COUNT);
        assert_eq!(mesh.indices.len(), INDEX_COUNT);
        assert_eq!(VERTEX_COUNT, 65 * 65 + 4 * 65);
        assert!(mesh.indices.iter().all(|&i| (i as usize) < VERTEX_COUNT));
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "the wrap base is an exact multiple")]
    fn uvs_stay_precise_far_from_the_world_origin() {
        // A lod 0 region a hundred kilometres out: every UV is under one
        // period plus the region, and the f32 spacing there is well under a
        // texel of the finest shipped tiling (4 m over 2048 texels).
        let coord = RegionCoord::containing(WorldPos::new(100_000.0, 0.0, 100_000.0), 0);
        let mesh = build(coord, &flat());
        let (x0, z0) = uv_wrap_base(coord);
        assert_eq!(x0 % f64::from(UV_PERIOD_M), 0.0);
        assert_eq!(z0 % f64::from(UV_PERIOD_M), 0.0);
        let texel = 4.0 / 2048.0;
        for (v, uv) in mesh.uvs.iter().enumerate() {
            assert!(uv[0] >= 0.0 && uv[0] < UV_PERIOD_M + 64.0, "{v}: {uv:?}");
            assert!(uv[1] >= 0.0 && uv[1] < UV_PERIOD_M + 64.0, "{v}: {uv:?}");
            let spacing = f32::EPSILON * uv[0].abs().max(uv[1].abs());
            assert!(spacing < texel / 2.0, "{v}: {uv:?} steps by {spacing}");
        }
        // The wrap is exact: the UV plus the base is the world position.
        let world_x = f64::from(mesh.uvs[0][0]) + x0;
        assert!((world_x - coord.min_corner().x()).abs() < 1e-3);
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "grid offsets are exact")]
    fn grid_positions_span_the_region_from_its_corner() {
        let coord = RegionCoord::new(3, -2, 2);
        let mesh = build(coord, &flat());
        let size = coord.size() as f32;
        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[GRID_N - 1], [size, 0.0, 0.0]);
        assert_eq!(mesh.positions[GRID_VERTICES - 1], [size, 0.0, size]);
        let step = coord.vertex_spacing() as f32;
        assert_eq!(mesh.positions[GRID_N + 1], [step, 0.0, step]);
        // UVs are world metres from the region's wrap base.
        let min = coord.min_corner();
        let (x0, z0) = uv_wrap_base(coord);
        assert_eq!(mesh.uvs[0], [(min.x() - x0) as f32, (min.z() - z0) as f32]);
        assert_eq!(
            mesh.uvs[GRID_VERTICES - 1],
            [
                (min.x() - x0 + coord.size()) as f32,
                (min.z() - z0 + coord.size()) as f32
            ]
        );
        // Every vertex knows its node's size.
        assert!(mesh.morph.iter().all(|m| m[3] == size));
    }

    #[test]
    fn heights_and_bounds_follow_the_samples() {
        let coord = RegionCoord::new(1, 1, 3);
        let plane = Plane {
            ax: 0.1,
            az: 0.0,
            hydration: 1.0,
        };
        let mesh = build(coord, &plane);
        let min = coord.min_corner();
        let expect_min = (0.1 * min.x()) as f32;
        let expect_max = (0.1 * (min.x() + coord.size())) as f32;
        assert!((mesh.min_height - expect_min).abs() < 1e-3);
        assert!((mesh.max_height - expect_max).abs() < 1e-3);
        let (lo, hi) = mesh.bounds();
        for p in &mesh.positions {
            for k in 0..3 {
                assert!(
                    p[k] >= lo[k] - 1e-3 && p[k] <= hi[k] + 1e-3,
                    "{p:?} outside {lo:?}..{hi:?}"
                );
            }
        }
        assert!((lo[1] - (mesh.min_height - mesh.skirt_depth)).abs() < 1e-6);
    }

    #[test]
    fn normals_are_unit_and_match_the_plane() {
        let plane = Plane {
            ax: 0.5,
            az: -0.25,
            hydration: 1.0,
        };
        let mesh = build(RegionCoord::new(-4, 7, 1), &plane);
        // The analytic normal of height = 0.5 x - 0.25 z.
        let expected = unit([-0.5, 1.0, 0.25]);
        for (n, m) in mesh.normals.iter().zip(&mesh.morph) {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "{n:?} has length {len}");
            for k in 0..3 {
                assert!((n[k] - expected[k]).abs() < 1e-4, "{n:?} vs {expected:?}");
            }
            // On a plane the parent normal is the same normal.
            assert!((m[1] - expected[0]).abs() < 1e-4 && (m[2] - expected[2]).abs() < 1e-4);
        }
    }

    /// The morph target of a child vertex is where the parent mesh passes
    /// over that point: build the parent for real and interpolate its
    /// triangles.
    #[test]
    fn morph_targets_lie_on_the_parent_mesh() {
        let child = RegionCoord::new(3, 1, 2);
        let parent = child.parent().expect("lod 2 has a parent");
        let child_mesh = build(child, &Bumps);
        let parent_mesh = build(parent, &Bumps);
        let pstep = parent.vertex_spacing() as f32;
        // Offset of the child's corner inside the parent, in parent steps.
        let off = child.min_corner() - parent.min_corner();
        let (ox, oz) = (
            (off.x / f64::from(pstep)) as usize,
            (off.z / f64::from(pstep)) as usize,
        );
        let pv = |i: usize, j: usize| parent_mesh.positions[j * GRID_N + i];
        let pn = |i: usize, j: usize| parent_mesh.normals[j * GRID_N + i];
        let mut max_err = 0.0f32;
        for j in 0..GRID_N {
            for i in 0..GRID_N {
                let m = child_mesh.morph[j * GRID_N + i];
                // Parent-grid coordinates of this child vertex, halves for
                // odd ones.
                let (pi, pj) = (ox * 2 + i, oz * 2 + j);
                let (ci, cj) = (pi / 2, pj / 2);
                let expected = match (pi % 2, pj % 2) {
                    (0, 0) => (pv(ci, cj)[1], pn(ci, cj)),
                    (1, 0) => (
                        f32::midpoint(pv(ci, cj)[1], pv(ci + 1, cj)[1]),
                        unit(add(pn(ci, cj), pn(ci + 1, cj))),
                    ),
                    (0, 1) => (
                        f32::midpoint(pv(ci, cj)[1], pv(ci, cj + 1)[1]),
                        unit(add(pn(ci, cj), pn(ci, cj + 1))),
                    ),
                    // The cell's diagonal: b = (ci + 1, cj) to c = (ci, cj + 1).
                    _ => (
                        f32::midpoint(pv(ci + 1, cj)[1], pv(ci, cj + 1)[1]),
                        unit(add(pn(ci + 1, cj), pn(ci, cj + 1))),
                    ),
                };
                max_err = max_err.max((m[0] - expected.0).abs());
                assert!((m[0] - expected.0).abs() < 1e-3, "height at ({i}, {j})");
                assert!(
                    (m[1] - expected.1[0]).abs() < 1e-4 && (m[2] - expected.1[2]).abs() < 1e-4,
                    "normal at ({i}, {j}): {m:?} vs {:?}",
                    expected.1
                );
            }
        }
        // And the targets genuinely differ from the child's own heights on
        // rough ground, or the test proves nothing.
        let moved = child_mesh
            .positions
            .iter()
            .zip(&child_mesh.morph)
            .filter(|(p, m)| (p[1] - m[0]).abs() > 0.5)
            .count();
        assert!(moved > 100, "only {moved} vertices move, max err {max_err}");
    }

    /// A valley along `z` at `x = 32` with walls rising one in two; with
    /// `ridge`, the same ground upside down; with `bank`, only its west
    /// wall, flat ground east of the floor.
    struct Valley {
        ridge: bool,
        bank: bool,
    }

    impl WorldSource for Valley {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let across = if self.bank {
                (32.0 - x).max(0.0)
            } else {
                (x - 32.0).abs()
            };
            let rise = (across * 0.5) as f32;
            Sample {
                height: if self.ridge { 40.0 - rise } else { rise },
                ..flat().sample(x, z)
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    fn visibility(mesh: &RegionMesh, i: usize, j: usize) -> u8 {
        mesh.sky[j * GRID_N + i][0]
    }

    #[test]
    fn open_ground_and_an_even_slope_see_the_whole_sky() {
        for source in [
            flat(),
            Plane {
                ax: 0.3,
                az: -0.2,
                hydration: 1.0,
            },
        ] {
            for lod in [0, 3] {
                let mesh = build(RegionCoord::new(1, -2, lod), &source);
                for (v, sky) in mesh.sky.iter().enumerate() {
                    assert!(sky[0] >= 254, "vertex {v} at lod {lod} sees {}", sky[0]);
                }
                if source.ax == 0.0 {
                    // The bent normal is straight up: x and z at the middle.
                    assert!(
                        mesh.sky
                            .iter()
                            .all(|s| s[1].abs_diff(128) <= 1 && s[2].abs_diff(128) <= 1)
                    );
                }
            }
        }
    }

    #[test]
    fn a_valley_floor_sees_less_sky_than_a_ridge_and_bends_up_the_valley() {
        let shape = |ridge, bank| build(RegionCoord::new(0, 0, 0), &Valley { ridge, bank });
        let (valley, ridge, bank) = (shape(false, false), shape(true, false), shape(false, true));
        let floor = visibility(&valley, 32, 32);
        assert!(floor < 230, "the floor sees {floor}");
        assert!(visibility(&ridge, 32, 32) >= 254, "a ridge is open");
        // Up a wall the far wall sinks and the sky opens.
        assert!(visibility(&valley, 50, 32) > floor);
        // On the floor the open sky is along the valley: no sideways lean.
        let bent = valley.sky[32 * GRID_N + 32];
        assert!(bent[1].abs_diff(128) <= 1, "{bent:?}");
        // Beside a bank the open sky leans away from it, east.
        let beside = bank.sky[32 * GRID_N + 40];
        assert!(beside[0] < 250 && beside[1] > 130, "{beside:?}");
    }

    #[test]
    fn a_node_carries_its_parents_sky_and_neighbours_agree_on_their_edge() {
        for (child, neighbour) in [
            (RegionCoord::new(3, 1, 2), RegionCoord::new(4, 1, 2)),
            (RegionCoord::new(-3, 5, 0), RegionCoord::new(-2, 5, 0)),
        ] {
            let parent = child.parent().expect("a parent");
            let child_mesh = build(child, &Bumps);
            let parent_mesh = build(parent, &Bumps);
            let pstep = parent.vertex_spacing();
            let off = child.min_corner() - parent.min_corner();
            let (ox, oz) = ((off.x / pstep) as usize, (off.z / pstep) as usize);
            for j in (0..GRID_N).step_by(2) {
                for i in (0..GRID_N).step_by(2) {
                    assert_eq!(
                        child_mesh.sky_parent[j * GRID_N + i],
                        parent_mesh.sky[(oz + j / 2) * GRID_N + ox + i / 2],
                        "{child:?} even vertex ({i}, {j})"
                    );
                }
            }
            // The morph target of an odd vertex is between its parents'.
            let m = child_mesh.sky_parent[GRID_N + 1][0];
            let (a, b) = (
                child_mesh.sky_parent[2][0],
                child_mesh.sky_parent[2 * GRID_N][0],
            );
            assert!((a.min(b)..=a.max(b)).contains(&m));
            // The east edge of one is the west edge of the next.
            let east = build(neighbour, &Bumps);
            for j in 0..GRID_N {
                assert_eq!(
                    child_mesh.sky[j * GRID_N + GRID_N - 1],
                    east.sky[j * GRID_N],
                    "{child:?} edge row {j}"
                );
            }
        }
        // Rough ground is not all open, or the comparisons prove little.
        let mesh = build(RegionCoord::new(3, 1, 2), &Bumps);
        assert!(mesh.sky.iter().any(|s| s[0] < 200));
    }

    #[test]
    fn a_pinned_node_morphs_its_sky_nowhere() {
        let mut scratch = Vec::new();
        let mesh = build_region_mesh_pinned(
            RegionCoord::new(3, 1, 2),
            &Bumps,
            &layers(),
            0.05,
            true,
            &mut scratch,
        );
        assert_eq!(mesh.sky, mesh.sky_parent);
    }

    fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "skirt vertices are exact copies")]
    fn skirts_hang_below_their_edge_vertex_and_face_outward() {
        let coord = RegionCoord::new(0, 0, 4);
        let mesh = build(
            coord,
            &Plane {
                ax: 0.02,
                az: 0.03,
                hydration: 1.0,
            },
        );
        let edges = edge_indices();
        for (e, edge) in edges.iter().enumerate() {
            for (k, &v) in edge.iter().enumerate() {
                let s = GRID_VERTICES + e * GRID_N + k;
                let p = mesh.positions[v as usize];
                let q = mesh.positions[s];
                assert_eq!(q[0], p[0]);
                assert_eq!(q[2], p[2]);
                assert!((p[1] - q[1] - mesh.skirt_depth).abs() < 1e-5);
                assert_eq!(mesh.normals[s], mesh.normals[v as usize]);
                assert_eq!(mesh.weights[s], mesh.weights[v as usize]);
                assert_eq!(mesh.slope[s][..3], mesh.slope[v as usize][..3]);
                assert_eq!(mesh.slope[s][3], skirt_corner(mesh.slope[v as usize][3], e));
                assert_eq!(mesh.tints[s], mesh.tints[v as usize]);
                let (m, ms) = (mesh.morph[v as usize], mesh.morph[s]);
                assert!((m[0] - ms[0] - mesh.skirt_depth).abs() < 1e-5);
                assert_eq!(&m[1..], &ms[1..]);
            }
        }
        assert!((mesh.skirt_depth - 0.05 * coord.size() as f32).abs() < 1e-3);
        // Every skirt triangle's normal points out of the region.
        let center = [coord.size() as f32 * 0.5, 0.0, coord.size() as f32 * 0.5];
        for tri in mesh.indices[GRID_INDICES..].chunks(3) {
            let n = triangle_normal(&mesh.positions, tri[0], tri[1], tri[2]);
            let p = mesh.positions[tri[0] as usize];
            let out = [p[0] - center[0], 0.0, p[2] - center[2]];
            let facing = n[0] * out[0] + n[2] * out[2];
            assert!(facing > 0.0, "skirt triangle {tri:?} faces inward");
            assert!(n[1].abs() < 1e-3, "skirt triangle {tri:?} is not vertical");
        }
        // And every grid triangle faces up.
        for tri in mesh.indices[..GRID_INDICES].chunks(3) {
            let n = triangle_normal(&mesh.positions, tri[0], tri[1], tri[2]);
            assert!(n[1] > 0.0, "grid triangle {tri:?} faces down");
        }
    }

    #[test]
    fn weights_follow_the_biome_blend() {
        // Region 0..64 at lod 0 straddles no border; use lod 2 (256 m)
        // starting at -256 so x = 0 is inside.
        let coord = RegionCoord::new(-1, 0, 2);
        let mesh = build(coord, &flat());
        let mut saw_blend = false;
        let step = coord.vertex_spacing() as f32;
        for (k, w) in mesh.weights.iter().enumerate() {
            let x = mesh.positions[k][0] + coord.min_corner().x() as f32;
            let slope = mesh.slope[k];
            assert_eq!(slope[1], 255, "wild ground takes its slope layer");
            // Without geology the rock is the dominant biome's slope layer,
            // alone.
            assert_eq!(slope[2], 0);
            if x <= -100.0 - step {
                assert_eq!(w[4], 255, "pure biome 0 at {x}");
                assert_eq!(slope[0], pack_rock_pair(4, 4));
            } else if x >= 100.0 + step {
                assert_eq!(w[1], 255, "pure biome 1 at {x}");
                assert_eq!(slope[0], pack_rock_pair(1, 1));
            } else if x.abs() < 100.0 - step {
                saw_blend = true;
                assert_eq!(u32::from(w[1]) + u32::from(w[4]), 255);
                // The share of layer 1 (biome 1) rises with x; the tent
                // filter keeps a linear ramp as it is.
                let t = (x + 100.0) / 200.0;
                let share = f32::from(w[1]) / 255.0;
                assert!(
                    (share - t).abs() < 2.0 / 255.0,
                    "share {share} at {x}, expected {t}"
                );
                // The slope layer follows the dominant biome.
                let dominant = if t < 0.5 { 4 } else { 1 };
                assert_eq!(
                    slope[0],
                    pack_rock_pair(dominant, dominant),
                    "slope layer at {x}"
                );
            }
        }
        assert!(saw_blend);
    }

    #[test]
    fn tints_blend_between_the_biomes() {
        let mut table = BiomeLayers::from_layers(vec![0, 1]);
        table.tints = vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let coord = RegionCoord::new(-1, 0, 2);
        let mut scratch = Vec::new();
        let mesh = build_region_mesh(coord, &flat(), &table, 0.05, &mut scratch);
        for (k, t) in mesh.tints.iter().enumerate() {
            let x = mesh.positions[k][0] + coord.min_corner().x() as f32;
            let blue = ((x + 100.0) / 200.0).clamp(0.0, 1.0);
            assert!((t[0] - (1.0 - blue)).abs() < 1e-4 && (t[2] - blue).abs() < 1e-4);
            assert!(t[1].abs() < 1e-6);
            assert!((t[3] - slope_of_rise(0.7)).abs() < 1e-6);
        }
        // A 45 degree slope has normal.y = 1 / sqrt 2.
        assert!((slope_of_rise(1.0) - (1.0 - core::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-6);
    }

    #[test]
    fn a_dry_ground_yellows_its_tint_and_thins_its_canopy() {
        let mut table = BiomeLayers::from_layers(vec![0, 1]);
        table.tints = vec![[0.2, 0.5, 0.2], [0.2, 0.5, 0.2]];
        table.covers = vec![[0.1, 0.3, 0.1, 0.8], [0.1, 0.3, 0.1, 0.8]];
        let coord = RegionCoord::new(-1, 0, 2);
        let mut scratch = Vec::new();
        let wet = build_region_mesh(
            coord,
            &Plane {
                ax: 0.0,
                az: 0.0,
                hydration: 1.0,
            },
            &table,
            0.05,
            &mut scratch,
        );
        let dry = build_region_mesh(
            coord,
            &Plane {
                ax: 0.0,
                az: 0.0,
                hydration: 0.0,
            },
            &table,
            0.05,
            &mut scratch,
        );
        // Parched ground keeps more red and loses blue and some green.
        assert!(dry.tints[0][0] > wet.tints[0][0]);
        assert!(dry.tints[0][2] < wet.tints[0][2]);
        assert!(dry.tints[0][1] > wet.tints[0][1]);
        // The sample's cover of 0.5 halves the biome's canopy either way.
        assert!((wet.covers[0][3] - 0.4).abs() < 1e-5, "{:?}", wet.covers[0]);
    }

    #[test]
    fn deterministic() {
        let plane = Plane {
            ax: 0.3,
            az: 0.7,
            hydration: 1.0,
        };
        let a = build(RegionCoord::new(5, 5, 5), &plane);
        let b = build(RegionCoord::new(5, 5, 5), &plane);
        assert_eq!(a, b);
    }

    /// Faces of each province, by id: 0 shows layer 9, 1 layer 2, 2 layer
    /// 11; with `bare_slope` and far tints of their own.
    fn geology() -> BiomeLayers {
        BiomeLayers::from_layers(vec![4, 1]).with_provinces(vec![
            ProvinceLook {
                face: 9,
                bare_slope: 0.5,
                far_tint: [1.25, 1.0, 0.75],
            },
            ProvinceLook {
                face: 2,
                bare_slope: 1.0,
                far_tint: [0.75, 0.75, 0.75],
            },
            ProvinceLook {
                face: 11,
                bare_slope: 2.0,
                far_tint: [1.0, 1.0, 1.0],
            },
        ])
    }

    /// Where the provinces of [`Provinces`] meet: the middle of the lod 2
    /// region `(-1, -1)`.
    const MEET: f64 = -128.0;

    /// Province 0 west of a 200 m blend across `x = MEET` and province 1
    /// east of it, over [`Plane`]'s biomes; and, north of `z = MEET`,
    /// province 2 in place of province 1, blending with both across a 200 m
    /// band of its own, so around `(MEET, MEET)` three provinces meet and
    /// the samples' pairs disagree.
    struct Provinces {
        three: bool,
    }

    impl Provinces {
        fn weights(&self, x: f64, z: f64) -> [f32; 3] {
            let east = ((x - MEET + 100.0) / 200.0).clamp(0.0, 1.0) as f32;
            let north = if self.three {
                ((z - MEET + 100.0) / 200.0).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            [1.0 - east, east * (1.0 - north), east * north]
        }
    }

    impl WorldSource for Provinces {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let w = self.weights(x, z);
            // The top two, heaviest first, renormalised as the field's pair is.
            let mut order = [0usize, 1, 2];
            order.sort_by(|&a, &b| w[b].total_cmp(&w[a]).then(a.cmp(&b)));
            let (a, b) = (order[0], order[1]);
            let total = w[a] + w[b];
            let provinces = if w[b] <= 0.0 {
                [(ProvinceId(a as u32), 1.0), (ProvinceId(a as u32), 0.0)]
            } else {
                [
                    (ProvinceId(a as u32), w[a] / total),
                    (ProvinceId(b as u32), w[b] / total),
                ]
            };
            Sample {
                provinces,
                ..flat().sample(x, z)
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    fn build_with(
        coord: RegionCoord,
        source: &dyn WorldSource,
        layers: &BiomeLayers,
    ) -> RegionMesh {
        let mut scratch = Vec::new();
        build_region_mesh(coord, source, layers, 0.05, &mut scratch)
    }

    #[test]
    fn rock_pairs_pack_and_unpack() {
        for lo in 0..MAX_LAYERS as u8 {
            for hi in lo..MAX_LAYERS as u8 {
                assert_eq!(unpack_rock_pair(pack_rock_pair(lo, hi)), (lo, hi));
            }
        }
        // And every byte is a pair: the shader decodes whatever it reads.
        for byte in 0..=u8::MAX {
            let (lo, hi) = unpack_rock_pair(byte);
            assert_eq!(pack_rock_pair(lo, hi), byte);
        }
    }

    /// The decode rests on this: a triangle has one vertex at each corner,
    /// skirts included.
    #[test]
    fn every_triangle_has_one_vertex_of_each_corner() {
        let mesh = build(RegionCoord::new(0, 0, 0), &flat());
        for tri in mesh.indices.chunks(3) {
            let mut corners: Vec<u8> = tri.iter().map(|&v| mesh.slope[v as usize][3]).collect();
            corners.sort_unstable();
            assert_eq!(corners, [0, 1, 2], "triangle {tri:?}");
        }
    }

    #[test]
    fn a_vertex_in_one_province_carries_its_face_alone() {
        let layers = geology();
        // Lod 0 regions well to either side of the blend.
        let west = build_with(
            RegionCoord::new(-5, 0, 0),
            &Provinces { three: false },
            &layers,
        );
        for (k, slope) in west.slope[..GRID_VERTICES].iter().enumerate() {
            assert_eq!(slope[0], pack_rock_pair(9, 9), "vertex {k}");
            assert_eq!(slope[2], 0);
            assert_eq!(slope[1], 255);
            // Its threshold is the biome's rise times the province's 0.5,
            // and its far colour the biome's times its far tint.
            let t = west.tints[k];
            assert!((t[3] - slope_of_rise(0.35)).abs() < 1e-6, "{t:?}");
        }
        let plain = build_with(RegionCoord::new(-5, 0, 0), &flat(), &layers);
        for (a, b) in west.tints.iter().zip(&plain.tints) {
            assert!((a[0] - b[0] * 1.25).abs() < 1e-5 && (a[2] - b[2] * 0.75).abs() < 1e-5);
        }
        // The factor is carried apart, so the near ground can take the
        // biomes' colour without it (ticket 0006), and none without geology.
        for (k, f) in west.far_factors[..GRID_VERTICES].iter().enumerate() {
            assert!(
                (f[0] - 1.25).abs() < 1e-6 && (f[2] - 0.75).abs() < 1e-6,
                "{k}: {f:?}"
            );
            let (t, p) = (west.tints[k], plain.tints[k]);
            for c in 0..3 {
                assert!((t[c] / f[c] - p[c]).abs() < 1e-5, "{k}: {t:?} {f:?} {p:?}");
            }
        }
        let one = 1.0_f32.to_bits();
        assert!(
            plain
                .far_factors
                .iter()
                .all(|f| f.iter().all(|c| c.to_bits() == one))
        );
        assert_eq!(west.far_factors.len(), VERTEX_COUNT);
        let east = build_with(
            RegionCoord::new(3, 0, 0),
            &Provinces { three: false },
            &layers,
        );
        for slope in &east.slope[..GRID_VERTICES] {
            assert_eq!(slope[0], pack_rock_pair(2, 2));
            assert_eq!(slope[2], 0);
        }
    }

    #[test]
    fn a_vertex_on_a_border_carries_both_faces_and_the_higher_ones_share() {
        let layers = geology();
        let coord = RegionCoord::new(-1, 0, 2);
        let mesh = build_with(coord, &Provinces { three: false }, &layers);
        let mut saw = 0;
        for (k, slope) in mesh.slope[..GRID_VERTICES].iter().enumerate() {
            let x = f64::from(mesh.positions[k][0]) + coord.min_corner().x() - MEET;
            if x.abs() >= 99.0 {
                continue;
            }
            saw += 1;
            // Province 0 (layer 9) and province 1 (layer 2), lower layer
            // first whichever weighs more, so the pair never swaps; the
            // share is layer 9's, which is province 0's, falling with x.
            assert_eq!(slope[0], pack_rock_pair(2, 9), "at {x}");
            let expected = (1.0 - (x + 100.0) / 200.0) as f32;
            assert!(
                (f32::from(slope[2]) / 255.0 - expected).abs() <= 0.5 / 255.0 + 1e-6,
                "share {} at {x}, expected {expected}",
                slope[2]
            );
            // bare_slope and far tint blend by the same shares.
            let bare = 0.5 * expected + (1.0 - expected);
            assert!((mesh.tints[k][3] - slope_of_rise(0.7 * bare)).abs() < 1e-5);
        }
        assert!(saw > 20);
    }

    /// Where three provinces meet, neighbouring vertices name different
    /// pairs. Decoded as the fragment decodes them, the rock is the same
    /// along every edge from both of its triangles, sums to one, and at a
    /// vertex is that vertex's own.
    #[test]
    fn three_provinces_meet_without_a_seam() {
        let layers = geology();
        let coord = RegionCoord::new(-1, -1, 2);
        let mesh = build_with(coord, &Provinces { three: true }, &layers);
        let pairs: std::collections::BTreeSet<u8> =
            mesh.slope[..GRID_VERTICES].iter().map(|s| s[0]).collect();
        assert!(pairs.len() >= 3, "the pairs disagree somewhere: {pairs:?}");

        // Every grid triangle, keyed by its edges.
        let mut by_edge: std::collections::HashMap<(u32, u32), Vec<[u32; 3]>> =
            std::collections::HashMap::new();
        for tri in mesh.indices[..GRID_INDICES].chunks(3) {
            let tri = [tri[0], tri[1], tri[2]];
            for e in 0..3 {
                let (a, b) = (tri[e], tri[(e + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(tri);
            }
        }
        let decode = |tri: [u32; 3], at: &[(u32, f32)]| {
            let bary = tri.map(|v| at.iter().find(|p| p.0 == v).map_or(0.0, |p| p.1));
            RockCorners::interpolate(tri.map(|v| mesh.slope[v as usize]), bary)
        };
        let mut edges_with_disagreement = 0;
        let mut worst = 0.0f32;
        for (&(a, b), tris) in &by_edge {
            if tris.len() != 2 {
                continue;
            }
            let differ = tris
                .iter()
                .flatten()
                .any(|&v| mesh.slope[v as usize][0] != mesh.slope[a as usize][0]);
            edges_with_disagreement += usize::from(differ);
            for f in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
                let at = [(a, 1.0 - f), (b, f)];
                let one = decode(tris[0], &at);
                let two = decode(tris[1], &at);
                let (w1, w2) = (one.weights(), two.weights());
                let sum: f32 = w1.iter().sum();
                assert!((sum - 1.0).abs() < 1e-5, "weights sum to {sum}");
                for l in 0..MAX_LAYERS {
                    worst = worst.max((w1[l] - w2[l]).abs());
                }
                for contact in [0.05, 0.3, 0.5, 0.7, 0.95] {
                    assert_eq!(one.pick(contact), two.pick(contact), "edge {a}-{b} at {f}");
                }
            }
            // At a vertex the rock is its own pair by its own share.
            let own = decode(tris[0], &[(a, 1.0)]).weights();
            let (lo, hi) = unpack_rock_pair(mesh.slope[a as usize][0]);
            let share = f32::from(mesh.slope[a as usize][2]) / 255.0;
            let mut expected = [0.0f32; MAX_LAYERS];
            expected[usize::from(lo)] += 1.0 - share;
            expected[usize::from(hi)] += share;
            for l in 0..MAX_LAYERS {
                assert!((own[l] - expected[l]).abs() < 1e-5, "vertex {a}: {own:?}");
            }
        }
        assert!(worst < 1e-5, "an edge's two triangles differ by {worst}");
        assert!(edges_with_disagreement > 10, "{edges_with_disagreement}");
        // And inside a triangle whose three vertices name three different
        // pairs, all three provinces' rocks weigh.
        let mut three_way = false;
        for tri in mesh.indices[..GRID_INDICES].chunks(3) {
            let tri = [tri[0], tri[1], tri[2]];
            let names: std::collections::BTreeSet<u8> =
                tri.iter().map(|&v| mesh.slope[v as usize][0]).collect();
            if names.len() == 3 {
                let w = decode(
                    tri,
                    &[
                        (tri[0], 1.0 / 3.0),
                        (tri[1], 1.0 / 3.0),
                        (tri[2], 1.0 / 3.0),
                    ],
                )
                .weights();
                three_way |= w[9] > 0.0 && w[2] > 0.0 && w[11] > 0.0;
            }
        }
        assert!(three_way, "no triangle blends all three rocks");
    }

    /// A corner too near the far edge to be divided by is dropped smoothly:
    /// the decode never steps as the weight crosses the threshold.
    #[test]
    fn the_decode_fades_a_corner_out_without_a_step() {
        let slopes = [
            [pack_rock_pair(2, 9), 255, 128, 0],
            [pack_rock_pair(9, 11), 255, 40, 1],
            [pack_rock_pair(0, 2), 255, 200, 2],
        ];
        let mut last: Option<[f32; MAX_LAYERS]> = None;
        for k in 0..=400 {
            let c = 0.03 * k as f32 / 400.0;
            let w = RockCorners::interpolate(slopes, [0.6 - c / 2.0, 0.4 - c / 2.0, c]).weights();
            if let Some(prev) = last {
                let step: f32 = w.iter().zip(&prev).map(|(a, b)| (a - b).abs()).sum();
                assert!(step < 0.01, "a step of {step} at {c}");
            }
            last = Some(w);
        }
    }

    #[test]
    fn a_settlements_ground_takes_no_rock_and_keeps_its_colour() {
        struct LaidOnGeology;
        impl WorldSource for LaidOnGeology {
            fn sample(&self, x: f64, z: f64) -> Sample {
                Sample {
                    provinces: Provinces { three: true }.sample(x, z).provinces,
                    ..Laid.sample(x, z)
                }
            }
            fn sea_level(&self) -> f32 {
                0.0
            }
        }
        let layers = geology();
        let coord = RegionCoord::new(-1, -1, 2);
        let with = build_with(coord, &LaidOnGeology, &layers);
        let without = build_with(coord, &Laid, &layers);
        assert_eq!(with.weights, without.weights);
        assert_eq!(with.tints, without.tints, "the far colour of laid ground");
        assert_eq!(with.covers, without.covers);
        assert!(
            with.slope.iter().all(|s| s[1] == 0),
            "no rock on laid ground"
        );
    }

    #[test]
    fn without_geology_the_tint_and_threshold_are_the_biomes() {
        let mut table = BiomeLayers::from_layers(vec![0, 1]);
        table.slope_rises = vec![0.45, 1.2];
        table.slope_thresholds = vec![slope_of_rise(0.45), slope_of_rise(1.2)];
        let coord = RegionCoord::new(-1, 0, 2);
        let mesh = build_with(coord, &flat(), &table);
        for (k, t) in mesh.tints.iter().enumerate() {
            let x = mesh.positions[k][0] + coord.min_corner().x() as f32;
            let dominant = usize::from(x >= 0.0);
            assert_eq!(
                t[3].to_bits(),
                table.slope_thresholds[dominant].to_bits(),
                "at {x}"
            );
        }
    }

    /// The rock the fragment decodes at world `(x, z)` on `mesh`: the grid
    /// triangle holding the point and its barycentric weights there.
    fn rock_at(mesh: &RegionMesh, x: f64, z: f64) -> RockCorners {
        let step = mesh.coord.vertex_spacing();
        let min = mesh.coord.min_corner();
        let (u, v) = ((x - min.x()) / step, (z - min.z()) / step);
        let (i, j) = (u.floor() as usize, v.floor() as usize);
        let (i, j) = (i.min(GRID_N - 2), j.min(GRID_N - 2));
        let (fx, fz) = ((u - i as f64) as f32, (v - j as f64) as f32);
        let at = |i: usize, j: usize| mesh.slope[j * GRID_N + i];
        if fx + fz < 1.0 {
            RockCorners::interpolate(
                [at(i, j), at(i + 1, j), at(i, j + 1)],
                [1.0 - fx - fz, fx, fz],
            )
        } else {
            RockCorners::interpolate(
                [at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)],
                [1.0 - fz, 1.0 - fx, fx + fz - 1.0],
            )
        }
    }

    /// A node and its parent draw the same rock: the pair and share are the
    /// samples' own at every vertex, so a coarser grid only interpolates
    /// the same field over longer edges. Across a two-province blend the
    /// weights differ by under a hundredth (0.0097 here, at the corners of
    /// this source's linear ramp, which the field's smooth blend does not
    /// have); where three meet, the samples' pair changes which two it
    /// names, and that step is spread over one triangle of whichever grid
    /// is drawn, so a LOD change moves it within a triangle, by at most the
    /// weight the third province had there (0.24 here, a third at worst).
    #[test]
    fn a_node_and_its_parent_draw_the_same_rock() {
        let layers = geology();
        for (three, bound) in [(false, 0.015), (true, 0.34)] {
            let source = Provinces { three };
            // The lod 2 node round the meeting point, which a node's
            // border never crosses at this size, and its lod 3 parent.
            let child = RegionCoord::new(-1, -1, 2);
            let parent = child.parent().expect("lod 2 has a parent");
            let fine = build_with(child, &source, &layers);
            let coarse = build_with(parent, &source, &layers);
            let size = child.size();
            let min = child.min_corner();
            let mut worst = 0.0f32;
            for a in 0..40 {
                for b in 0..40 {
                    let x = min.x() + size * (f64::from(a) + 0.37) / 40.0;
                    let z = min.z() + size * (f64::from(b) + 0.61) / 40.0;
                    let (w1, w2) = (
                        rock_at(&fine, x, z).weights(),
                        rock_at(&coarse, x, z).weights(),
                    );
                    for l in 0..MAX_LAYERS {
                        worst = worst.max((w1[l] - w2[l]).abs());
                    }
                }
            }
            assert!(
                worst < bound,
                "three provinces {three}: the LODs differ by {worst}"
            );
        }
    }

    /// The cut-wall look is the carve's walls' alone: steep ground the
    /// carve lowered, well over its river. A natural cliff in the same
    /// valley and a valley floor the carve lowered gently keep their beds.
    #[test]
    fn only_a_steep_cut_is_a_wall() {
        let layers = BiomeLayers::from_layers(vec![0]);
        let at = |slope: f32, cut_dm: u16| Sample {
            height: 60.0,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: strata_worldgen::Landform {
                slope,
                ..strata_worldgen::Landform::default()
            },
            water: Some(strata_worldgen::SampleWater {
                surface_m: 20.0,
                bed_m: 19.5,
                width_m: 6.0,
                signed_distance_m: 50.0,
                flow: [0.5, 0.0],
                discharge_m3s: 1.0,
                order: 2,
                kind: WaterKind::Channel,
                cut_dm,
            }),
        };
        assert!(bank_of(&at(1.5, 300), &layers).wall > 0.99);
        assert!(bank_of(&at(1.5, 0), &layers).wall < 1e-6, "a natural cliff");
        assert!(
            bank_of(&at(0.3, 300), &layers).wall < 1e-6,
            "a valley floor"
        );
    }

    /// The wet bank (ticket 0005, step 8): past the wet line the water's
    /// own ground stays damp, darker by a share of the line's, out to a few
    /// metres past a river's edge and up to two over its surface; a road's
    /// way across it is the road's and stays dry.
    #[test]
    fn the_wet_bank_is_damp_past_the_line_and_dry_on_a_road() {
        let layers = BiomeLayers::from_layers(vec![0]);
        let at = |above: f32, out: f32, cleared: f32, kind: WaterKind| Sample {
            height: 20.0 + above,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: strata_worldgen::Landform {
                cleared,
                ..strata_worldgen::Landform::default()
            },
            water: Some(strata_worldgen::SampleWater {
                surface_m: 20.0,
                bed_m: 19.0,
                width_m: 10.0,
                signed_distance_m: out,
                flow: [0.5, 0.0],
                discharge_m3s: 5.0,
                order: 2,
                kind,
                cut_dm: 0,
            }),
        };
        let wet = |above, out, cleared, kind| bank_of(&at(above, out, cleared, kind), &layers).wet;
        let bank = at(0.0, 0.0, 0.0, WaterKind::Channel)
            .water
            .expect("water")
            .bank_m();
        // The line itself, then the damp soil a metre over it.
        assert!(wet(0.0, bank, 0.0, WaterKind::Channel) > 0.99);
        let damp = wet(1.0, bank, 0.0, WaterKind::Channel);
        assert!(damp > 0.2 && damp <= WET_BANK_SHARE + 1e-6, "{damp}");
        // Past the river's edge by the bed's reach and a stride, still
        // damp; by its full reach, dry.
        assert!(wet(1.0, bank + EDGE_REACH_M + 1.0, 0.0, WaterKind::Channel) > 0.1);
        let far = bank + EDGE_REACH_M + WET_BANK_REACH_M;
        assert!(wet(0.5, far, 0.0, WaterKind::Channel) < 1e-6);
        // Two metres over the water, dry; round a lake, as by a river.
        assert!(wet(2.0, bank, 0.0, WaterKind::Channel) < 1e-6);
        assert!(wet(1.0, 30.0, 0.0, WaterKind::Lake) > 0.2);
        // A road's way across the bank: the line stays, the damp does not.
        assert!(wet(1.0, bank, 1.0, WaterKind::Channel) < 1e-6);
        assert!(wet(0.0, bank, 1.0, WaterKind::Channel) > 0.99);
    }
}
