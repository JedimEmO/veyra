//! Rivers and lakes drawn near (ticket 0005, step 3): a water surface per
//! terrain region, built on the region's meshing task from the mesher's own
//! grid of samples.
//!
//! # What is built
//!
//! Every grid vertex whose sample carries water ([`Sample::water`]) takes a
//! surface: the record's own, which the carve keeps continuous across
//! confluences and down to a mouth's shore at sea level. A cell is drawn
//! when one of its corners has water standing over its ground
//! ([`Sample::standing_water`]: within a river's banks, under a lake's
//! level), at the vertex's own level of detail or at its parent's, and its
//! two triangles are the terrain's, on the same diagonal. So the water and
//! the ground are two linear surfaces over the same triangles, and the
//! shore is where they cross, per pixel, wherever it runs through the grid:
//! at a bank vertex the surface lies under the ground, which hides the edge.
//! A vertex on a floodplain lower than its river's level (past the banks,
//! where the water does not stand), or lower than the water standing beside
//! it (a lake whose rim its outlet's valley cut), takes its own ground less
//! [`DRY_DROP_M`], so the sheet never stands over dry ground and runs down
//! over a breached rim rather than dropping in a wall to the river below. Most regions
//! have no water and build nothing; a river's region builds a strip.
//!
//! # Levels of detail
//!
//! The water morphs as the ground does: every vertex carries where it lies
//! on the parent level's surface (an even vertex its own, an odd one the
//! midpoint of the edge or the diagonal its parent triangle has, as
//! `mesh::Grid::parent` places the ground), and the vertex shader lerps to
//! it by the same distance band the terrain's morph uses. A region's edge
//! therefore meets a coarse neighbour's water where its ground meets that
//! neighbour's ground, and a detail zone's pinned region pins its water.
//! A cell is drawn at either level's wetness, so the morph never uncovers a
//! hole. A river narrower than a level resolves ([`RESOLVED_CELLS`]) is not
//! drawn at that level: its channel would only dip under the surface where
//! a vertex happened to land in it, a zigzag of cells with a film of water
//! over the banks between them. One the parent level leaves out fades out
//! over the morph's band instead of popping ([`FADES`]). So a creek is
//! drawn near, where the ground resolves its channel, and not at all beyond
//! (painting rivers at distance is ticket 0005's step 8).
//!
//! # Drawing
//!
//! A built surface is a small Bevy `Mesh` on a child of its region's entity,
//! so it is culled, rebased (ADR 0002) and despawned with the region. A
//! river's region builds a few hundred vertices of 56 bytes, where a
//! terrain region was 575 KiB through Bevy's mesh path (the store module's
//! docs): the measures are in ticket 0005's step 3 record. The material is
//! the sea's water (`water_common.wgsl`), with a vertex stage of its own
//! that morphs, ripples scrolled along the flow at its speed, and foam
//! where the surface is steep and at the bank.

// Grid indices are small and region sizes powers of two under 32 km: every
// cast between usize, u32, u16 and f32 here is exact.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "grid indices and region sizes are small and exact in every width used"
)]
// The cell's corners are the terrain's a, b, c and d.
#![allow(clippy::many_single_char_names, reason = "a cell's four corners")]

use bevy::asset::{Asset, Handle, RenderAssetUsages};
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::math::Vec4;
use bevy::mesh::{
    Indices, Mesh, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexFormat,
};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    StandardMaterial,
};
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    AsBindGroup, Buffer, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use strata_core::{GRID_N, RegionCoord};
use strata_worldgen::Sample;
use strata_worldgen::hydrology::WaterKind;

use crate::mesh::{GRID_VERTICES, RING, SAMPLE_N};

/// The rivers' shader, vertex and fragment, relative to the asset root.
pub const RIVER_SHADER_PATH: &str = "shaders/river_water.wgsl";

/// How far under its own ground a vertex on a floodplain lower than its
/// river's level puts the water, metres: deep enough that the sheet does
/// not flicker against the ground where it is hidden.
pub const DRY_DROP_M: f32 = 0.25;

/// Added to a river mesh's view depth in the transparent pass's sort, so
/// every river and lake draws before every other blended thing. The water
/// writes its depth like an opaque surface; drawn after a fall's foam or
/// sheet over it (which sort by their own centres, kilometres from a
/// region's), it would fail the depth test under them and leave the bed
/// showing through. Larger than any two centres' difference in view depth
/// within the far ring. The pipeline's own depth bias is kept at zero
/// ([`RiverSurface`]'s `specialize`), so this moves the order alone.
pub const RIVER_SORT_BIAS: f32 = -1.0e5;

/// Water whose surface is no more than this over the sea's level is the
/// sea's to draw, metres: a mouth's last stretch comes down to it.
pub const SEA_GAP_M: f32 = 0.005;

/// `[parent_surface, parent_depth, node_size, kind]`: where the vertex lies
/// on the parent level's water and how deep that water stands over the
/// parent level's ground; the region's side, for the morph's band; the
/// water's kind ([`kind_code`]), plus [`FADES`] where it fades out over the
/// morph.
pub const ATTRIBUTE_WATER_MORPH: MeshVertexAttribute =
    MeshVertexAttribute::new("WaterMorph", 0x5c2a_71e0_0000_0001, VertexFormat::Float32x4);

/// `[flow_x, flow_z, depth, steepness]`: downstream times the mean speed,
/// m/s; the surface over the vertex's ground, metres, negative under a
/// bank; the surface's rise over run.
pub const ATTRIBUTE_WATER_FLOW: MeshVertexAttribute =
    MeshVertexAttribute::new("WaterFlow", 0x5c2a_71e0_0000_0002, VertexFormat::Float32x4);

/// The water's kind as the shader reads it: channel 0, lake 1, fall 2,
/// mouth 3.
#[must_use]
pub fn kind_code(kind: WaterKind) -> f32 {
    match kind {
        WaterKind::Channel => 0.0,
        WaterKind::Lake => 1.0,
        WaterKind::Fall => 2.0,
        WaterKind::Mouth => 3.0,
    }
}

/// A region's water surface as its meshing task builds it: plain vectors,
/// like [`crate::RegionMesh`], so the builder and its tests never touch a
/// GPU layout. Metres from the region's minimum corner.
#[derive(Debug, Clone, PartialEq)]
pub struct RiverMesh {
    /// Which region this is.
    pub coord: RegionCoord,
    /// `[x, surface, z]`.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals of the surface, from the neighbouring vertices' levels.
    pub normals: Vec<[f32; 3]>,
    /// [`ATTRIBUTE_WATER_MORPH`] per vertex.
    pub morph: Vec<[f32; 4]>,
    /// [`ATTRIBUTE_WATER_FLOW`] per vertex.
    pub flow: Vec<[f32; 4]>,
    /// Triangle list, the terrain's winding and diagonal.
    pub indices: Vec<u16>,
    /// Lowest surface, metres, morph targets included.
    pub min_height: f32,
    /// Highest surface, metres, morph targets included.
    pub max_height: f32,
}

impl RiverMesh {
    /// The corners of a box that holds every vertex at either level.
    #[must_use]
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let size = self.coord.size() as f32;
        ([0.0, self.min_height, 0.0], [size, self.max_height, size])
    }

    /// The Bevy mesh, for the render world only: its bounds are
    /// [`bounds`](Self::bounds), so the main world keeps no copy.
    #[must_use]
    pub fn to_mesh(&self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone())
        .with_inserted_attribute(ATTRIBUTE_WATER_MORPH, self.morph.clone())
        .with_inserted_attribute(ATTRIBUTE_WATER_FLOW, self.flow.clone())
        .with_inserted_indices(Indices::U16(self.indices.clone()))
    }

    /// Bytes the mesh uploads: 56 a vertex and 2 an index.
    #[must_use]
    pub fn upload_bytes(&self) -> usize {
        self.positions.len() * 56 + self.indices.len() * 2
    }
}

/// The level, the ground and the record of one sampled point, as the
/// builder reads it.
#[derive(Debug, Clone, Copy)]
struct Point {
    /// Where the drawn water lies, metres; `None` with no water near.
    y: Option<f32>,
    /// The water's own level there, the record's or its neighbours'.
    level: Option<f32>,
    ground: f32,
    /// Water stands over the ground here, over the sea's level.
    wet: bool,
    flow: [f32; 2],
    kind: f32,
    /// A river's channel width, metres; infinite for a lake and for a
    /// point with no water: what the level's spacing has to resolve.
    width: f32,
}

/// The water of the region `coord` from its mesher's samples: the grid
/// and its rings, row-major, `SAMPLE_N` a side
/// ([`crate::mesh::build_region_mesh_pinned`]'s `scratch` once it has
/// run). `None` where no water stands in the region. `pinned` as the
/// terrain's: every vertex's morph target is its own.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "the points, their parents, the cells and the vertices, in one pass each"
)]
pub fn build_river_mesh(
    coord: RegionCoord,
    samples: &[Sample],
    sea_level: f32,
    pinned: bool,
) -> Option<RiverMesh> {
    debug_assert_eq!(samples.len(), SAMPLE_N * SAMPLE_N);
    let floor = sea_level + SEA_GAP_M;
    // The quick way out, which nearly every region takes.
    let ring = RING;
    let grid_wet = |s: &Sample| s.standing_water().is_some_and(|w| w.surface_m > floor);
    let any = (0..GRID_N).any(|j| {
        let row = (j + ring) * SAMPLE_N + ring;
        samples[row..row + GRID_N].iter().any(grid_wet)
    });
    if !any {
        return None;
    }
    // Every sampled point's level: the record's surface, or for a point
    // with none beside one that has, the mean of its neighbours' (a lake's
    // record ends a lattice cell out; a vertex just past it still closes
    // the cells round the lake's last wet vertex).
    let n = SAMPLE_N;
    let surface: Vec<Option<f32>> = samples
        .iter()
        .map(|s| s.water.map(|w| w.surface_m))
        .collect();
    // The level of the water standing at each point, if any.
    let standing: Vec<Option<f32>> = samples
        .iter()
        .map(|s| s.standing_water().map(|w| w.surface_m))
        .collect();
    let mut points = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            let s = &samples[k];
            // The highest water standing beside the point: where its ground
            // is under that, the water beside it stands over lower ground
            // (a lake whose rim its outlet's valley has cut), and the sheet
            // runs down over the ground to it instead of dropping to this
            // point's own, far lower river.
            let mut beside: Option<f32> = None;
            for dj in -1_isize..=1 {
                for di in -1_isize..=1 {
                    let (ii, jj) = (i as isize + di, j as isize + dj);
                    if (0..n as isize).contains(&ii)
                        && (0..n as isize).contains(&jj)
                        && let Some(level) = standing[jj as usize * n + ii as usize]
                    {
                        beside = Some(beside.map_or(level, |held: f32| held.max(level)));
                    }
                }
            }
            let level = surface[k].or_else(|| {
                let (mut sum, mut count) = (0.0, 0);
                for dj in -1_isize..=1 {
                    for di in -1_isize..=1 {
                        let (ii, jj) = (i as isize + di, j as isize + dj);
                        if (0..n as isize).contains(&ii)
                            && (0..n as isize).contains(&jj)
                            && let Some(level) = surface[jj as usize * n + ii as usize]
                        {
                            sum += level;
                            count += 1;
                        }
                    }
                }
                (count > 0).then(|| sum / count as f32)
            });
            let standing = s.standing_water();
            let y = level.map(|level| {
                if standing.is_some() {
                    return level;
                }
                if s.height < beside.map_or(level, |beside| beside.max(level)) {
                    s.height - DRY_DROP_M
                } else {
                    level
                }
            });
            points.push(Point {
                y,
                level,
                ground: s.height,
                wet: standing.is_some_and(|w| w.surface_m > floor),
                flow: s.water.map_or([0.0, 0.0], |w| w.flow),
                kind: s.water.map_or(0.0, |w| kind_code(w.kind)),
                width: s
                    .water
                    .filter(|w| w.kind != WaterKind::Lake)
                    .map_or(f32::INFINITY, |w| w.width_m),
            });
        }
    }
    let at = |i: isize, j: isize| {
        &points[(j + ring as isize) as usize * n + (i + ring as isize) as usize]
    };

    let step = coord.vertex_spacing() as f32;
    let size = coord.size() as f32;
    // The narrowest river this level draws, metres ([`RESOLVED_CELLS`]).
    let resolves = resolved_width(coord.lod, step);
    // Where each grid vertex lies on the parent level: its level and its
    // ground, as `mesh::Grid::parent` pairs the even vertices.
    let parent = |i: isize, j: isize| -> (Option<f32>, f32) {
        let own = at(i, j);
        if pinned {
            return (own.y, own.ground);
        }
        let pair = |a: (isize, isize), b: (isize, isize)| {
            let (pa, pb) = (at(a.0, a.1), at(b.0, b.1));
            let y = match (pa.y, pb.y) {
                (Some(ya), Some(yb)) => Some(f32::midpoint(ya, yb)),
                _ => own.y,
            };
            (y, f32::midpoint(pa.ground, pb.ground))
        };
        match (i.rem_euclid(2) == 1, j.rem_euclid(2) == 1) {
            (false, false) => (own.y, own.ground),
            (true, false) => pair((i - 1, j), (i + 1, j)),
            (false, true) => pair((i, j - 1), (i, j + 1)),
            (true, true) => pair((i + 1, j - 1), (i - 1, j + 1)),
        }
    };
    let mut parents = Vec::with_capacity(GRID_VERTICES);
    for j in 0..GRID_N as isize {
        for i in 0..GRID_N as isize {
            parents.push(parent(i, j));
        }
    }
    let vertex_wet = |i: usize, j: usize| {
        let point = at(i as isize, j as isize);
        let (py, pg) = parents[j * GRID_N + i];
        point.wet || py.is_some_and(|py| py > pg && py > floor)
    };
    let usable = |i: usize, j: usize| at(i as isize, j as isize).y.is_some();
    // A grid vertex where the parent level puts it, for the test of two
    // waters at that level too: a cell the morph carries towards a coarse
    // cell holding two waters would draw that cell's wall on its way.
    let parent_point = |i: usize, j: usize| {
        let own = *at(i as isize, j as isize);
        let (py, pg) = parents[j * GRID_N + i];
        Point {
            y: py,
            ground: pg,
            wet: py.is_some_and(|py| py > pg && py > floor),
            ..own
        }
    };

    let mut index_of = vec![u16::MAX; GRID_VERTICES];
    let mut cells = Vec::new();
    for j in 0..GRID_N - 1 {
        for i in 0..GRID_N - 1 {
            let corners = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)];
            // A creek narrower than this level resolves is left to the
            // finer levels: drawn, its channel only dips under the surface
            // where a vertex happens to land in it, a zigzag of cells and
            // a film on the banks between them.
            if corners.iter().all(|&(i, j)| usable(i, j))
                && corners
                    .iter()
                    .any(|&(i, j)| vertex_wet(i, j) && at(i as isize, j as isize).width >= resolves)
                && !two_waters(corners.map(|(i, j)| *at(i as isize, j as isize)), step)
                && (pinned || !two_waters(corners.map(|(i, j)| parent_point(i, j)), 2.0 * step))
            {
                cells.push((i, j));
            }
        }
    }
    if cells.is_empty() {
        return None;
    }

    let mut mesh = RiverMesh {
        coord,
        positions: Vec::new(),
        normals: Vec::new(),
        morph: Vec::new(),
        flow: Vec::new(),
        indices: Vec::with_capacity(cells.len() * 6),
        min_height: f32::INFINITY,
        max_height: f32::NEG_INFINITY,
    };
    let mut vertex = |i: usize, j: usize, mesh: &mut RiverMesh| -> u16 {
        let slot = &mut index_of[j * GRID_N + i];
        if *slot != u16::MAX {
            return *slot;
        }
        let (ii, jj) = (i as isize, j as isize);
        let point = at(ii, jj);
        let y = point.y.unwrap_or(point.ground);
        let (py, pg) = parents[j * GRID_N + i];
        let py = py.unwrap_or(y);
        // The surface's slope from the neighbouring levels, the ring's
        // across the region's edge, so two regions agree on an edge. The
        // water's own levels and not the drawn ones: a floodplain's
        // vertex dropped under its ground is no slope of the river's.
        // A lake is level; and a neighbour's level further off than any
        // water runs is another water's (a lake's vertex beside the river
        // its outlet's valley cut far below it), no slope of this one's:
        // read as one, it foamed level lake water white in wedges.
        let own = point.level.unwrap_or(y);
        let lake = (point.kind - kind_code(WaterKind::Lake)).abs() < 0.5;
        let level = |di: isize, dj: isize| {
            at(ii + di, jj + dj)
                .level
                .filter(|level| !lake && (level - own).abs() <= STEEPEST_WATER * step)
                .unwrap_or(own)
        };
        let gx = (level(1, 0) - level(-1, 0)) / (2.0 * step);
        let gz = (level(0, 1) - level(0, -1)) / (2.0 * step);
        let len = (gx * gx + gz * gz + 1.0).sqrt();
        mesh.min_height = mesh.min_height.min(y).min(py);
        mesh.max_height = mesh.max_height.max(y).max(py);
        mesh.positions.push([i as f32 * step, y, j as f32 * step]);
        mesh.normals.push([-gx / len, 1.0 / len, -gz / len]);
        // A creek the parent level will not draw fades out as the vertex
        // morphs towards that level (the kind's code plus [`FADES`]).
        let fades = !pinned && point.width < resolved_width(coord.lod + 1, 2.0 * step);
        mesh.morph.push([
            py,
            py - pg,
            size,
            point.kind + if fades { FADES } else { 0.0 },
        ]);
        mesh.flow.push([
            point.flow[0],
            point.flow[1],
            y - point.ground,
            (gx * gx + gz * gz).sqrt(),
        ]);
        let index = (mesh.positions.len() - 1) as u16;
        *slot = index;
        index
    };
    // A lake's open water is one level over ground well under it, so its
    // cells are merged into rectangles: a lake's region drew 4 225 vertices
    // a region and the lake's own view 26 MiB of them, for a plane.
    let open = |i: usize, j: usize| -> Option<u32> {
        let corners = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)];
        let first = at(i as isize, j as isize).y?;
        corners
            .iter()
            .all(|&(ci, cj)| {
                let point = at(ci as isize, cj as isize);
                let (py, pg) = parents[cj * GRID_N + ci];
                point.wet
                    && (point.kind - kind_code(WaterKind::Lake)).abs() < 0.5
                    && point.y.is_some_and(|y| y.to_bits() == first.to_bits())
                    && py.is_some_and(|py| py.to_bits() == first.to_bits())
                    && first - point.ground > OPEN_DEPTH_M
                    && first - pg > OPEN_DEPTH_M
            })
            .then_some(first.to_bits())
    };
    let mut rectangles: Vec<[usize; 4]> = Vec::new();
    let mut growing: Vec<([usize; 4], u32)> = Vec::new();
    let mut is_cell = vec![false; (GRID_N - 1) * (GRID_N - 1)];
    for &(i, j) in &cells {
        is_cell[j * (GRID_N - 1) + i] = true;
    }
    let mut merged = vec![false; (GRID_N - 1) * (GRID_N - 1)];
    for j in 0..GRID_N - 1 {
        let mut runs: Vec<([usize; 2], u32)> = Vec::new();
        let mut i = 0;
        while i < GRID_N - 1 {
            let Some(level) = is_cell[j * (GRID_N - 1) + i].then(|| open(i, j)).flatten() else {
                i += 1;
                continue;
            };
            let start = i;
            while i < GRID_N - 1 && is_cell[j * (GRID_N - 1) + i] && open(i, j) == Some(level) {
                merged[j * (GRID_N - 1) + i] = true;
                i += 1;
            }
            runs.push(([start, i - 1], level));
        }
        // A run under one of the same span and level a row up extends it.
        let mut next = Vec::new();
        for (run, level) in runs {
            if let Some(at) = growing
                .iter()
                .position(|(r, l)| *l == level && r[0] == run[0] && r[1] == run[1])
            {
                let (mut r, l) = growing.swap_remove(at);
                r[3] = j;
                next.push((r, l));
            } else {
                next.push(([run[0], run[1], j, j], level));
            }
        }
        rectangles.extend(growing.drain(..).map(|(r, _)| r));
        growing = next;
    }
    rectangles.extend(growing.into_iter().map(|(r, _)| r));
    for (i, j) in cells {
        if merged[j * (GRID_N - 1) + i] {
            continue;
        }
        let a = vertex(i, j, &mut mesh);
        let b = vertex(i + 1, j, &mut mesh);
        let c = vertex(i, j + 1, &mut mesh);
        let d = vertex(i + 1, j + 1, &mut mesh);
        // Counter-clockwise from above, the diagonal from b to c: the
        // terrain's own triangles (`mesh::build_region_mesh_pinned`).
        mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
    }
    for [i0, i1, j0, j1] in rectangles {
        let a = vertex(i0, j0, &mut mesh);
        let b = vertex(i1 + 1, j0, &mut mesh);
        let c = vertex(i0, j1 + 1, &mut mesh);
        let d = vertex(i1 + 1, j1 + 1, &mut mesh);
        mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
    }
    Some(mesh)
}

/// Added to a vertex's kind code ([`kind_code`]) in
/// [`ATTRIBUTE_WATER_MORPH`] where its river is narrower than the parent
/// level resolves: the parent does not draw it, so the shader fades it out
/// over the morph's band and the creek does not pop at the level's edge.
pub const FADES: f32 = 4.0;

/// How many vertex spacings wide a river has to be for a level to draw it:
/// at one, a creek about as wide as the spacing still drew as a zigzag of
/// cells with films of water on its banks, seen from a few hundred metres.
/// The finest level draws anything a spacing wide, so the narrowest creek
/// (1.4 m) is drawn near.
pub const RESOLVED_CELLS: f32 = 2.0;

/// The narrowest river a level of `lod` with vertices `step` metres apart
/// draws, metres.
fn resolved_width(lod: u8, step: f32) -> f32 {
    if lod == 0 {
        step
    } else {
        RESOLVED_CELLS * step
    }
}

/// How deep a lake stands over a cell's every corner, at its own level and
/// its parent's, for the cell to be open water merged with its neighbours,
/// metres: past the bank's foam and the shore, where the ground under it
/// never crosses its level.
pub const OPEN_DEPTH_M: f32 = 0.5;

/// How far apart two waters standing in one cell's corners may lie and
/// still be one surface, metres: a lake beside the river below it (its
/// outlet's valley cut beside the lake) is two, and one sheet between them
/// stood as a wall of water over the valley.
pub const LAKE_STEP_M: f32 = 1.0;

/// The steepest a river's surface runs, rise over run, past which two of a
/// cell's wet corners are two waters whatever their kinds: steeper than any
/// fall the network grades (ADR 0010), with a metre to spare.
pub const STEEPEST_WATER: f32 = 1.5;

/// Whether a cell's corners hold two waters at different levels, which no
/// one sheet over the cell can draw: a lake's corner and a river's more
/// than [`LAKE_STEP_M`] apart, or any two wet corners further apart than
/// the steepest water runs; or water perched over a dry corner's ground
/// far under it. Such a cell is left undrawn and each water ends at its own
/// corner.
fn two_waters(corners: [Point; 4], step: f32) -> bool {
    let mut lake = (f32::INFINITY, f32::NEG_INFINITY);
    let mut river = (f32::INFINITY, f32::NEG_INFINITY);
    for corner in corners {
        let (true, Some(y)) = (corner.wet, corner.y) else {
            continue;
        };
        let range = if (corner.kind - kind_code(WaterKind::Lake)).abs() < 0.5 {
            &mut lake
        } else {
            &mut river
        };
        *range = (range.0.min(y), range.1.max(y));
    }
    let steepest = 1.0 + STEEPEST_WATER * step * core::f32::consts::SQRT_2;
    let spread = lake.1.max(river.1) - lake.0.min(river.0);
    let both = lake.0.is_finite() && river.0.is_finite();
    // A lake is level: its corners at two heights are two lakes, or one
    // and the sheet a morph draws towards a wall.
    let lakes = lake.0.is_finite() && lake.1 - lake.0 > LAKE_STEP_M;
    // A dry corner whose ground lies well under a lake's level beside it:
    // the lake is perched over lower ground there (its rim cut away), and
    // a sheet from its level down to that corner stood as a wall of water.
    let perched = corners.iter().any(|corner| {
        !corner.wet && (corner.ground < lake.1 - LAKE_STEP_M || corner.ground < river.1 - steepest)
    });
    (both && spread > LAKE_STEP_M) || lakes || spread > steepest || perched
}

/// The rivers' own numbers over the sea's water, mirrored by `RiverParams`
/// in `river_water.wgsl`.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct RiverParams {
    /// The terrain's morph band in node sizes in `xy` (start, end), the
    /// sea's level in `z`; `w` unused.
    pub morph: Vec4,
    /// Absorption per metre in `x`, the flow's ripple slope in `y`, the
    /// still water's ripple slope in `z`, the bank's foam band in metres of
    /// depth in `w`.
    pub look: Vec4,
    /// The surface's rise over run where steep water starts to foam in
    /// `x` and where it is all foam in `y`; the flow cycle's length,
    /// seconds, in `z`; `w` unused.
    pub foam: Vec4,
}

/// The `MaterialExtension` half of [`RiverMaterial`]: the sea's parameter
/// buffer and wave image (bound, not copied: the render world rewrites the
/// one buffer for both) and the rivers' own numbers.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(from_reflect = false)]
pub struct RiverSurface {
    /// The sea's [`crate::water::WaterParams`] buffer.
    #[storage(100, read_only, buffer)]
    #[reflect(ignore)]
    pub params: Buffer,
    /// The sea's wave image.
    #[texture(101)]
    #[sampler(102)]
    pub waves: Handle<Image>,
    /// The morph band and the rivers' look.
    #[uniform(103)]
    pub river: RiverParams,
}

impl MaterialExtension for RiverSurface {
    fn vertex_shader() -> ShaderRef {
        RIVER_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        RIVER_SHADER_PATH.into()
    }

    /// The surface's position and normal and its two water attributes.
    /// Transparent water draws in no prepass and casts no shadow, so this
    /// lays out the main pass alone.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            ATTRIBUTE_WATER_MORPH.at_shader_location(2),
            ATTRIBUTE_WATER_FLOW.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        // The water writes its depth, so the sea plane, drawn after it and
        // at sea level everywhere, does not draw its shallows' foam in a
        // river's channel whose bed lies under the sea's level near a
        // mouth. Where the river comes down to that level the shader
        // discards it and the sea draws the water.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(true);
            // The material's depth bias is the sort's (`RIVER_SORT_BIAS`);
            // as a depth offset it would sink the water through the ground.
            depth.bias.constant = 0;
        }
        Ok(())
    }
}

/// The rivers' and lakes' material as an asset type.
pub type RiverMaterial = ExtendedMaterial<StandardMaterial, RiverSurface>;

/// The one river material, made with the sea once the world exists.
#[derive(Resource, Debug, Clone)]
pub struct RiverMaterialHandle(pub Handle<RiverMaterial>);

/// A region's drawn water, on the child entity that draws it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionWater {
    /// Vertices in its mesh.
    pub vertices: u32,
    /// Bytes its mesh uploaded.
    pub bytes: u32,
}

/// How the rivers and lakes look over the sea's water. Insert before
/// [`crate::WaterPlugin`] to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct RiverConfig {
    /// The water's colour, linear RGB: a river carries silt and a lake
    /// grows weed, so they are a little greener than the sea; a greener
    /// grey still, with its gravel showing through, read from above as a
    /// road's asphalt (the review of step 3).
    pub color: [f32; 3],
    /// Absorption per metre of view path. Far over the sea's: a river is a
    /// metre deep, and at the sea's rate its body would be a film over the
    /// gravel, read from above as wet stones and not as water.
    pub absorption: f32,
    /// The flow's ripples: their tile side, metres, and their steepest
    /// slope.
    pub ripple: [f32; 2],
    /// The steepest slope still water's wind ripples reach, the sea's chop
    /// scaled down: a lake is calm next to the sea.
    pub still_slope: f32,
    /// Seconds of one cycle of the flow map: the ripples slide this long
    /// along the flow before the other phase takes over.
    pub cycle_s: f32,
    /// Water shallower than this at the bank foams, metres.
    pub bank_foam_depth: f32,
    /// The surface's rise over run where foam starts, and where the water
    /// is all foam: rapids and falls.
    pub steep_foam: [f32; 2],
}

impl Default for RiverConfig {
    fn default() -> Self {
        Self {
            color: [0.025, 0.08, 0.12],
            absorption: 1.6,
            ripple: [4.2, 0.16],
            still_slope: 0.35,
            cycle_s: 3.0,
            bank_foam_depth: 0.07,
            steep_foam: [0.06, 0.32],
        }
    }
}

impl RiverConfig {
    /// The shader's numbers for the terrain's morph band `(start, end)`
    /// and the sea's level.
    #[must_use]
    pub fn params(&self, morph: (f32, f32), sea_level: f32) -> RiverParams {
        RiverParams {
            morph: Vec4::new(morph.0, morph.1, sea_level, 0.0),
            look: Vec4::new(
                self.absorption,
                self.ripple[1],
                self.still_slope,
                self.bank_foam_depth,
            ),
            foam: Vec4::new(self.steep_foam[0], self.steep_foam[1], self.cycle_s, 0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::BiomeId;
    use strata_worldgen::{Landform, SampleWater};

    /// A river along `x` through the region's middle, `half` metres either
    /// side of `z = mid`, bed half a metre under a surface at ten metres,
    /// banks a metre over it.
    fn samples(coord: RegionCoord, half: f32, surface: impl Fn(f64) -> f32) -> Vec<Sample> {
        let step = coord.vertex_spacing();
        let min = coord.min_corner();
        let mid = min.z() + coord.size() * 0.5;
        let mut out = Vec::new();
        for j in 0..SAMPLE_N {
            for i in 0..SAMPLE_N {
                let x = min.x() + (i as f64 - RING as f64) * step;
                let z = min.z() + (j as f64 - RING as f64) * step;
                let across = (z - mid) as f32;
                let level = surface(x);
                let height = if across.abs() < half {
                    level - 0.5
                } else {
                    level + 1.0
                };
                out.push(Sample {
                    height,
                    biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                    provinces: Sample::NO_PROVINCES,
                    material: 0,
                    moisture: 0.5,
                    hydration: 0.5,
                    cover: 0.5,
                    landform: Landform::default(),
                    water: (across.abs() < 60.0).then_some(SampleWater {
                        surface_m: level,
                        bed_m: level - 0.5,
                        width_m: 2.0 * half,
                        signed_distance_m: across,
                        flow: [0.6, 0.0],
                        discharge_m3s: 2.0,
                        order: 2,
                        kind: WaterKind::Channel,
                        cut_dm: 0,
                    }),
                });
            }
        }
        out
    }

    /// Dry ground builds nothing; a river builds a strip over its channel
    /// whose every triangle has a wet corner, and whose surface stands at
    /// the river's level over the channel and under the ground at the bank.
    #[test]
    fn a_river_builds_a_strip_and_dry_ground_nothing() {
        let coord = RegionCoord::new(0, 0, 1);
        let dry: Vec<Sample> = samples(coord, 3.0, |_| 10.0)
            .into_iter()
            .map(|s| Sample { water: None, ..s })
            .collect();
        assert!(build_river_mesh(coord, &dry, 0.0, false).is_none());
        let wet = samples(coord, 3.0, |_| 10.0);
        let mesh = build_river_mesh(coord, &wet, 0.0, false).expect("a river");
        assert!(
            mesh.positions.len() < GRID_VERTICES / 4,
            "{}",
            mesh.positions.len()
        );
        for triangle in mesh.indices.chunks(3) {
            assert!(
                triangle.iter().any(|&v| mesh.flow[usize::from(v)][2] > 0.0),
                "a triangle with no water over its ground"
            );
        }
        for (p, flow) in mesh.positions.iter().zip(&mesh.flow) {
            assert!((p[1] - 10.0).abs() < 1e-6, "{p:?}");
            // In the channel the water is half a metre deep; on the bank it
            // lies a metre under the ground.
            assert!((flow[2] - 0.5).abs() < 1e-6 || (flow[2] + 1.0).abs() < 1e-6);
        }
    }

    /// The parent level's surface at an odd vertex is the midpoint of its
    /// even neighbours', so a region morphed all the way lies on its
    /// coarse neighbour's water; pinned, every target is the vertex's own.
    #[test]
    fn the_morph_targets_lie_on_the_parent_surface() {
        let coord = RegionCoord::new(0, 0, 0);
        let slope = |x: f64| 10.0 - (x * 0.05) as f32;
        let wet = samples(coord, 6.0, slope);
        let mesh = build_river_mesh(coord, &wet, 0.0, false).expect("a river");
        let step = coord.vertex_spacing() as f32;
        for (p, morph) in mesh.positions.iter().zip(&mesh.morph) {
            let i = (p[0] / step).round() as i32;
            if i % 2 == 0 {
                assert!((morph[0] - p[1]).abs() < 1e-5);
            } else {
                // A straight slope: the midpoint is the vertex's own level.
                assert!((morph[0] - p[1]).abs() < 1e-4, "{p:?} {morph:?}");
            }
        }
        let pinned = build_river_mesh(coord, &wet, 0.0, true).expect("a river");
        for (p, morph) in pinned.positions.iter().zip(&pinned.morph) {
            assert!((morph[0] - p[1]).abs() < 1e-6);
        }
    }

    /// Where a river's surface is down at the sea's level, the sea draws
    /// it: a mouth's last stretch builds nothing.
    #[test]
    fn a_river_at_sea_level_is_left_to_the_sea() {
        let coord = RegionCoord::new(0, 0, 1);
        let at_sea = samples(coord, 3.0, |_| 0.0);
        assert!(build_river_mesh(coord, &at_sea, 0.0, false).is_none());
    }

    /// A lake whose rim is cut away beside the valley of a river far below
    /// it ends at its last corner: no sheet runs from its level down to the
    /// valley, where it stood as a wall of water.
    #[test]
    fn a_perched_lake_stands_no_wall_over_the_valley() {
        let coord = RegionCoord::new(0, 0, 1);
        let step = coord.vertex_spacing();
        let min = coord.min_corner();
        let mid = min.x() + coord.size() * 0.5;
        let mut out = Vec::new();
        for _ in 0..SAMPLE_N {
            for i in 0..SAMPLE_N {
                let x = min.x() + (i as f64 - RING as f64) * step;
                let lake = x < mid;
                let water = if lake {
                    SampleWater {
                        surface_m: 50.0,
                        bed_m: 45.0,
                        width_m: 400.0,
                        signed_distance_m: -50.0,
                        flow: [0.0, 0.0],
                        discharge_m3s: 1.0,
                        order: 0,
                        kind: WaterKind::Lake,
                        cut_dm: 0,
                    }
                } else {
                    SampleWater {
                        surface_m: 30.0,
                        bed_m: 29.5,
                        width_m: 4.0,
                        signed_distance_m: 40.0,
                        flow: [0.5, 0.0],
                        discharge_m3s: 1.0,
                        order: 1,
                        kind: WaterKind::Channel,
                        cut_dm: 0,
                    }
                };
                out.push(Sample {
                    height: if lake { 47.0 } else { 36.0 },
                    biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                    provinces: Sample::NO_PROVINCES,
                    material: 0,
                    moisture: 0.5,
                    hydration: 0.5,
                    cover: 0.5,
                    landform: Landform::default(),
                    water: Some(water),
                });
            }
        }
        let mesh = build_river_mesh(coord, &out, 0.0, true).expect("the lake");
        for triangle in mesh.indices.chunks(3) {
            let ys: Vec<f32> = triangle
                .iter()
                .map(|&v| mesh.positions[usize::from(v)][1])
                .collect();
            let spread = ys.iter().copied().fold(f32::MIN, f32::max)
                - ys.iter().copied().fold(f32::MAX, f32::min);
            assert!(spread < 1e-3, "a sheet over {spread} m: {ys:?}");
        }
        // Level water is level to the shader too: the river's level beside
        // the lake's foamed its edge white as steep water.
        for (morph, flow) in mesh.morph.iter().zip(&mesh.flow) {
            if (morph[3] - kind_code(WaterKind::Lake)).abs() < 0.5 {
                assert!(flow[3] < 1e-6, "lake water {} steep", flow[3]);
            }
        }
    }

    /// A region wholly under a lake's open water is one rectangle.
    #[test]
    fn open_lake_water_is_merged() {
        let coord = RegionCoord::new(0, 0, 2);
        let lake = |s: Sample| Sample {
            height: 7.0,
            water: Some(SampleWater {
                surface_m: 10.0,
                bed_m: 7.0,
                width_m: 500.0,
                signed_distance_m: -100.0,
                flow: [0.0, 0.0],
                discharge_m3s: 1.0,
                order: 0,
                kind: WaterKind::Lake,
                cut_dm: 0,
            }),
            ..s
        };
        let all: Vec<Sample> = samples(coord, 1e6, |_| 10.0)
            .into_iter()
            .map(lake)
            .collect();
        let mesh = build_river_mesh(coord, &all, 0.0, false).expect("a lake");
        assert_eq!((mesh.positions.len(), mesh.indices.len()), (4, 6));
        let size = coord.size() as f32;
        assert!(
            mesh.positions
                .iter()
                .any(|p| (p[0] - size).abs() < 1e-3 && (p[2] - size).abs() < 1e-3)
        );
    }

    /// A creek six metres wide is drawn at the level two metres apart and
    /// fades out over its morph, as the level four apart leaves it out; a
    /// pinned region never morphs, so never fades.
    #[test]
    fn a_creek_is_drawn_where_its_level_resolves_it() {
        let (fine, coarse) = (RegionCoord::new(0, 0, 1), RegionCoord::new(0, 0, 2));
        assert!(build_river_mesh(coarse, &samples(coarse, 3.0, |_| 10.0), 0.0, false).is_none());
        let wide = build_river_mesh(coarse, &samples(coarse, 8.0, |_| 10.0), 0.0, false);
        assert!(wide.is_some(), "sixteen metres at four apart");
        let near = build_river_mesh(fine, &samples(fine, 3.0, |_| 10.0), 0.0, false)
            .expect("six metres at two apart");
        assert!(near.morph.iter().all(|m| m[3] > FADES - 0.5));
        let pinned =
            build_river_mesh(fine, &samples(fine, 3.0, |_| 10.0), 0.0, true).expect("pinned");
        assert!(pinned.morph.iter().all(|m| m[3] < FADES - 0.5));
    }

    /// Two regions side by side put the same water on their shared edge.
    #[test]
    fn neighbours_agree_on_their_edge() {
        let level = |x: f64| 10.0 - (x * 0.01) as f32;
        let (a, b) = (RegionCoord::new(0, 0, 1), RegionCoord::new(1, 0, 1));
        let ma = build_river_mesh(a, &samples(a, 3.0, level), 0.0, false).expect("a");
        let mb = build_river_mesh(b, &samples(b, 3.0, level), 0.0, false).expect("b");
        let size = a.size() as f32;
        let edge = |mesh: &RiverMesh, x: f32| {
            let mut out: Vec<(i32, [u32; 2])> = mesh
                .positions
                .iter()
                .zip(&mesh.normals)
                .filter(|(p, _)| (p[0] - x).abs() < 1e-3)
                .map(|(p, n)| (p[2] as i32, [p[1].to_bits(), n[1].to_bits()]))
                .collect();
            out.sort_unstable();
            out
        };
        let (east, west) = (edge(&ma, size), edge(&mb, 0.0));
        assert!(!east.is_empty());
        assert_eq!(east, west);
    }
}
