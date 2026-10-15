//! Terrain regions on the GPU: one record a region in one storage buffer,
//! drawn through one shared grid-and-skirt mesh (streaming plan, phase 5).
//!
//! A region used to be a Bevy `Mesh` of 13 vertex attributes, 575 KiB with
//! its indices, and Bevy's `allocate_and_free_meshes` packed it into the
//! vertex slab on the render thread one attribute of one vertex at a time:
//! about 3.9 ms a region, two regions a frame on every frame that streamed,
//! and the same task was often stolen onto the main thread by a system
//! waiting on its own parallel work (`docs/perf/streaming-2026-10-04.md`,
//! cost 1). Grass and far trees had already left that path for the same
//! reason; terrain now follows them.
//!
//! # Layout
//!
//! The meshing task packs the region into [`VERTEX_WORDS`] words a grid
//! vertex ([`pack_region`]), and the collector writes them with one
//! `write_buffer` into an equal-size slot of the store's vertex buffer, a
//! memory copy into the staging belt. Every region is the same size, so the
//! slots never fragment. A grid vertex's record, word by word:
//!
//! | Word | What | Read as |
//! | --- | --- | --- |
//! | 0 | height, metres | `f32` |
//! | 1 to 4 | the sixteen layer shares | `unpack4x8unorm` each |
//! | 5 | rock: `[pair, wild_share, hi_share, corner]` | `unpack4x8unorm` |
//! | 6 | sky: `[visibility, bent_x, bent_z, wet]` | `unpack4x8unorm` |
//! | 7 | the parent level's sky, and how much of a cut wall | `unpack4x8unorm` |
//! | 8 | normal, octahedral | `unpack2x16snorm` |
//! | 9 | parent height, metres | `f32` |
//! | 10 | parent normal, octahedral | `unpack2x16snorm` |
//! | 11, 12 | tint, linear RGB, and the bare slope | `unpack2x16float` each |
//! | 13, 14 | canopy colour and cover | `unpack2x16float` each |
//! | 15 | the tint's far factor, a byte each over [`FAR_FACTOR_RANGE`], and a spare byte | `unpack_far_factor` |
//!
//! What is the same for every vertex of a region, or follows from where the
//! vertex sits on the grid, is not stored per vertex: x and z are the grid
//! index times the region's step, the UV is the region's wrapped origin plus
//! x and z, and a skirt vertex is its edge vertex's record dropped by the
//! skirt depth with its rock corner turned (`mesh::skirt_corner`). Those
//! come from the region's [`SlotHeader`] in a second, small buffer, read by
//! the same slot. Heights stay `f32`, so the surface, the morph and the
//! skirts are where the CPU mesh puts them to the bit; normals lose about a
//! ten-thousandth to the octahedral pair and the tint and cover a part in
//! two thousand to `f16`, both under anything a frame can show.
//!
//! # Drawing
//!
//! Every region entity draws the one shared mesh ([`shared_mesh`]): the grid
//! and its skirts, wound as the mesher winds them, whose only read
//! attribute is the vertex's place, `(i, skirt, j)` in `POSITION`. The
//! entity's `MeshTag` is its slot in its page, and the vertex shader
//! (`terrain_common.wgsl`, `terrain_attributes`) rebuilds the old vertex
//! contract from the slot's record and header, so the morph, the sky terms
//! and the fragment stage are the same code as before. The entity keeps its
//! `Aabb`, `NoAutoAabb` and the transform the render origin gives it (ADR
//! 0002), so culling and the rebase are unchanged. With one mesh and one
//! material, Bevy batches every region into a few indirect draws.
//!
//! # Paint
//!
//! A third buffer holds each slot's paint at the same slot index: the
//! rivers' and roads' lines the fragment shader draws at distance
//! ([`crate::paint`], whose module docs have the layout), [`PAINT_BYTES`] a
//! slot. A region writes only the words it uses, one where it has nothing
//! to paint, so a reused slot never shows its last owner's lines: the
//! fragment reads no further than the first word says.
//!
//! A page holds what one storage binding can. A desktop card binds
//! gigabytes and the store is one page; an adapter at wgpu's default limits
//! binds 529 slots, about half what a moving view keeps, and the store opens
//! further pages, each with a copy of the material bound to its own buffers
//! ([`TerrainStore`]). A region is then drawn or waiting, never reported
//! ready with nothing drawn, so the no-hole rule holds on any adapter.
//!
//! A freed slot waits [`QUARANTINE_FRAMES`] before it is handed out again,
//! as the far trees' and the grass's ranges do: with pipelined rendering the
//! frame the render world is drawing was extracted before the region went,
//! and a write for the slot's next owner lands at the next queue
//! submission, which may be that frame's.

// Grid indices, word offsets and slot counts are small; every cast between
// usize, u32 and f32 in this module is exact.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "grid indices, word offsets and slot counts are small and exact in every width used"
)]
// The grid and vector maths use the mesher's single letters.
#![allow(clippy::many_single_char_names, reason = "vertex and vector maths")]

use std::collections::VecDeque;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::prelude::*;
use bevy::ecs::world::DeferredWorld;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use strata_core::GRID_N;
use tracing::info;

use crate::material::{MAX_LAYERS, TerrainMaterial};
use crate::mesh::{GRID_VERTICES, INDEX_COUNT, RegionMesh, VERTEX_COUNT};
use crate::paint::PAINT_BYTES;

/// Words of one grid vertex's record (the module docs, "Layout").
pub const VERTEX_WORDS: usize = 16;

/// Bytes of one grid vertex's record: 64.
pub const VERTEX_BYTES: usize = VERTEX_WORDS * 4;

/// Words of one region's slot: its grid vertices' records. Skirts are not
/// stored; the shader hangs them from the edge vertices.
pub const SLOT_WORDS: usize = GRID_VERTICES * VERTEX_WORDS;

/// Bytes of one region's slot, 270 400.
pub const SLOT_BYTES: u64 = (SLOT_WORDS * 4) as u64;

/// Bytes of one [`SlotHeader`] as the shader's `TerrainSlot` reads it.
pub const HEADER_BYTES: u64 = 32;

/// Slots the store starts with: about one and a half times the resident
/// regions the moving fixtures hold (979 to 1 087), so a merge the no-hole
/// rule holds and the quarantine fit without growing. 396 MiB, which is
/// less than the terrain vertices the mesh slab held before.
pub const INITIAL_SLOTS: u32 = 1536;

/// Frames a freed slot waits before it is handed out again (the module
/// docs, "Drawing").
pub const QUARANTINE_FRAMES: u64 = 3;

/// The `POSITION.y` of a shared-mesh vertex: 0 on the grid, and on a skirt
/// the step its rock corner turns by, 1 under the south and north rows and
/// 2 under the west and east columns (`mesh::skirt_corner`).
const SKIRT_ROW: f32 = 1.0;
const SKIRT_COLUMN: f32 = 2.0;

/// What every vertex of a region shares, at the region's slot in the header
/// buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotHeader {
    /// World `(x, z)` of the region's minimum corner less the UV wrap base
    /// (`mesh::uv_wrap_base`): the UV of grid vertex `(0, 0)`.
    pub uv_origin: [f32; 2],
    /// Metres between grid vertices.
    pub step: f32,
    /// The region's side, metres, which scales the morph band.
    pub size: f32,
    /// How far below its edge vertex a skirt vertex hangs, metres.
    pub skirt_depth: f32,
    /// The region's level, for a debug view; the shader needs only the step
    /// and the size.
    pub lod: u32,
}

impl SlotHeader {
    /// The header as the shader's `TerrainSlot` reads it.
    #[must_use]
    pub fn bytes(&self) -> [u8; HEADER_BYTES as usize] {
        let mut out = [0; HEADER_BYTES as usize];
        let words = [
            self.uv_origin[0].to_bits(),
            self.uv_origin[1].to_bits(),
            self.step.to_bits(),
            self.size.to_bits(),
            self.skirt_depth.to_bits(),
            self.lod,
            0,
            0,
        ];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// A region as the store takes it: the header and the grid's records.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedRegion {
    /// What every vertex shares.
    pub header: SlotHeader,
    /// [`SLOT_BYTES`] bytes, [`VERTEX_WORDS`] little-endian words a grid
    /// vertex in grid order: packed on the meshing task, so the main thread
    /// only hands them to `write_buffer`.
    pub bytes: Vec<u8>,
    /// The region's paint as little-endian words, as long as it needs and
    /// at most [`PAINT_BYTES`] (`paint::pack_paint`); one zero word where it
    /// paints nothing.
    pub paint: Vec<u8>,
}

impl PackedRegion {
    /// Word `index` of the records.
    #[must_use]
    pub fn word(&self, index: usize) -> u32 {
        let bytes = &self.bytes[index * 4..index * 4 + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Bytes this region writes to the store: its slot, its header and its
    /// paint.
    #[must_use]
    pub fn upload_bytes(&self) -> u64 {
        SLOT_BYTES + HEADER_BYTES + self.paint.len() as u64
    }

    /// Sets the region's paint from its words.
    pub fn set_paint(&mut self, words: &[u32]) {
        debug_assert!(words.len() as u64 * 4 <= PAINT_BYTES);
        self.paint = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    }
}

/// Packs a built region for the store: the module docs' layout, from the
/// grid vertices of `mesh` (skirts are rebuilt by the shader).
#[must_use]
pub fn pack_region(mesh: &RegionMesh) -> PackedRegion {
    let mut bytes = Vec::with_capacity(SLOT_WORDS * 4);
    let mut push = |word: u32| bytes.extend_from_slice(&word.to_le_bytes());
    for v in 0..GRID_VERTICES {
        let weights = &mesh.weights[v];
        let morph = mesh.morph[v];
        let tint = mesh.tints[v];
        let cover = mesh.covers[v];
        let parent_normal = rebuild_up(morph[1], morph[2]);
        push(mesh.positions[v][1].to_bits());
        for k in 0..MAX_LAYERS / 4 {
            push(u32::from_le_bytes([
                weights[4 * k],
                weights[4 * k + 1],
                weights[4 * k + 2],
                weights[4 * k + 3],
            ]));
        }
        push(u32::from_le_bytes(mesh.slope[v]));
        push(u32::from_le_bytes(mesh.sky[v]));
        push(u32::from_le_bytes(mesh.sky_parent[v]));
        push(pack_snorm2(oct_encode(mesh.normals[v])));
        push(morph[0].to_bits());
        push(pack_snorm2(oct_encode(parent_normal)));
        push(pack_half2(tint[0], tint[1]));
        push(pack_half2(tint[2], tint[3]));
        push(pack_half2(cover[0], cover[1]));
        push(pack_half2(cover[2], cover[3]));
        push(pack_far_factor(mesh.far_factors[v]));
    }
    debug_assert_eq!(bytes.len(), SLOT_WORDS * 4);
    let coord = mesh.coord;
    PackedRegion {
        header: SlotHeader {
            uv_origin: mesh.uvs[0],
            step: coord.vertex_spacing() as f32,
            size: coord.size() as f32,
            skirt_depth: mesh.skirt_depth,
            lod: u32::from(coord.lod),
        },
        bytes,
        paint: vec![0; 4],
    }
}

/// The parent normal from its `x` and `z`, as the shader rebuilt it from
/// the morph attribute: a heightfield normal points up.
fn rebuild_up(x: f32, z: f32) -> [f32; 3] {
    [x, (1.0 - x * x - z * z).max(0.0).sqrt(), z]
}

/// A unit vector as the octahedral pair in `x` and `z` (the up axis is the
/// one folded), so a heightfield normal, which points up, never folds.
#[must_use]
pub fn oct_encode(n: [f32; 3]) -> [f32; 2] {
    let l1 = n[0].abs() + n[1].abs() + n[2].abs();
    let (x, z) = (n[0] / l1, n[2] / l1);
    if n[1] >= 0.0 {
        [x, z]
    } else {
        [(1.0 - z.abs()) * sign(x), (1.0 - x.abs()) * sign(z)]
    }
}

/// [`oct_encode`]'s inverse, as `terrain_common.wgsl`'s `oct_decode`.
#[must_use]
pub fn oct_decode(p: [f32; 2]) -> [f32; 3] {
    let mut x = p[0];
    let mut z = p[1];
    let y = 1.0 - x.abs() - z.abs();
    let t = (-y).max(0.0);
    x += if x >= 0.0 { -t } else { t };
    z += if z >= 0.0 { -t } else { t };
    let len = (x * x + y * y + z * z).sqrt();
    [x / len, y / len, z / len]
}

/// One with the sign of `v`, and one at zero, so the pole folds to a
/// corner rather than to the centre.
fn sign(v: f32) -> f32 {
    if v >= 0.0 { 1.0 } else { -1.0 }
}

/// WGSL's `pack2x16snorm`.
fn pack_snorm2(v: [f32; 2]) -> u32 {
    let one = |x: f32| u32::from((x.clamp(-1.0, 1.0) * 32767.0).round() as i16 as u16);
    one(v[0]) | (one(v[1]) << 16)
}

/// WGSL's `unpack2x16snorm`.
#[must_use]
pub fn unpack_snorm2(word: u32) -> [f32; 2] {
    let one = |bits: u32| (f32::from(bits as u16 as i16) / 32767.0).max(-1.0);
    [one(word & 0xffff), one(word >> 16)]
}

/// The largest far factor a record holds: the provinces' far tints are a
/// quarter either side of one, and a byte over about twice that keeps a
/// part in 128. It is 255 / 128 so a factor of one, all the ground outside
/// a province's far tint, is a byte of 128 exactly and the near ground's
/// colour there is the tint to the bit (`terrain_common.wgsl`'s
/// `TERRAIN_FAR_FACTOR_RANGE`).
pub const FAR_FACTOR_RANGE: f32 = 255.0 / 128.0;

/// The far factor as WGSL's `pack4x8unorm` over [`FAR_FACTOR_RANGE`], the
/// fourth byte zero.
fn pack_far_factor(f: [f32; 3]) -> u32 {
    let byte = |x: f32| u32::from((x / FAR_FACTOR_RANGE).clamp(0.0, 1.0).mul_add(255.0, 0.5) as u8);
    byte(f[0]) | (byte(f[1]) << 8) | (byte(f[2]) << 16)
}

/// The far factor of a record's word 15.
#[must_use]
pub fn unpack_far_factor(word: u32) -> [f32; 3] {
    // Over 255 times the range is over 128, exact in binary.
    [0, 8, 16].map(|shift| f32::from((word >> shift) as u8) * (FAR_FACTOR_RANGE / 255.0))
}

/// WGSL's `pack2x16float`, rounding to nearest even.
pub(crate) fn pack_half2(a: f32, b: f32) -> u32 {
    u32::from(half_bits(a)) | (u32::from(half_bits(b)) << 16)
}

/// WGSL's `unpack2x16float`.
#[must_use]
pub fn unpack_half2(word: u32) -> [f32; 2] {
    [half_value(word as u16), half_value((word >> 16) as u16)]
}

/// The IEEE half nearest `x`, ties to even; overflow is infinity.
fn half_bits(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man == 0 { 0 } else { 0x200 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    let round = |value: u32, shift: u32| {
        let kept = value >> shift;
        let rest = value & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        kept + u32::from(rest > half || (rest == half && kept & 1 == 1))
    };
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        // A subnormal half: the hidden bit joins the mantissa.
        return sign | round(man | 0x0080_0000, (14 - e) as u32) as u16;
    }
    // A carry out of the mantissa rounds into the exponent, as it should.
    sign | round(((e as u32) << 23) | man, 13) as u16
}

/// The value of half `bits`.
fn half_value(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let man = f32::from(bits & 0x3ff);
    match exp {
        0 => sign * man * (2.0_f32).powi(-24),
        0x1f if man == 0.0 => sign * f32::INFINITY,
        0x1f => f32::NAN,
        _ => sign * (1.0 + man / 1024.0) * (2.0_f32).powi(exp - 15),
    }
}

/// One vertex as the shader's `terrain_attributes` rebuilds it, in the
/// layout of the old vertex contract (the material module's docs): what the
/// tests compare against the CPU mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unpacked {
    /// Metres from the region's minimum corner.
    pub position: [f32; 3],
    /// Unit normal.
    pub normal: [f32; 3],
    /// Wrapped world `(x, z)`.
    pub uv: [f32; 2],
    /// The layer shares in 255ths.
    pub weights: [u8; MAX_LAYERS],
    /// `[pair, wild_share, hi_share, corner]`.
    pub slope: [u8; 4],
    /// `[parent_height, parent_normal_x, parent_normal_z, node_size]`.
    pub morph: [f32; 4],
    /// Tint and bare slope.
    pub tint: [f32; 4],
    /// Canopy colour and cover.
    pub cover: [f32; 4],
    /// The tint's far factor.
    pub far_factor: [f32; 3],
    /// The sky in 255ths.
    pub sky: [u8; 4],
    /// The parent level's sky in 255ths.
    pub sky_parent: [u8; 4],
}

/// The CPU mirror of `terrain_common.wgsl`'s `terrain_attributes`: shared
/// mesh vertex `place` (its `POSITION`) of the region packed as `packed`.
#[must_use]
pub fn unpack_vertex(packed: &PackedRegion, place: [f32; 3]) -> Unpacked {
    let (i, skirt, j) = (place[0] as usize, place[1] as u32, place[2] as usize);
    let base = (j * GRID_N + i) * VERTEX_WORDS;
    let w: Vec<u32> = (base..base + VERTEX_WORDS)
        .map(|k| packed.word(k))
        .collect();
    let header = packed.header;
    let mut weights = [0; MAX_LAYERS];
    for k in 0..MAX_LAYERS / 4 {
        weights[4 * k..4 * k + 4].copy_from_slice(&w[1 + k].to_le_bytes());
    }
    let mut slope = w[5].to_le_bytes();
    let mut height = f32::from_bits(w[0]);
    let mut parent_height = f32::from_bits(w[9]);
    if skirt != 0 {
        height -= header.skirt_depth;
        parent_height -= header.skirt_depth;
        slope[3] = ((u32::from(slope[3]) + skirt) % 3) as u8;
    }
    let (x, z) = (i as f32 * header.step, j as f32 * header.step);
    let parent_normal = oct_decode(unpack_snorm2(w[10]));
    let [t0, t1] = unpack_half2(w[11]);
    let [t2, t3] = unpack_half2(w[12]);
    let [c0, c1] = unpack_half2(w[13]);
    let [c2, c3] = unpack_half2(w[14]);
    Unpacked {
        position: [x, height, z],
        normal: oct_decode(unpack_snorm2(w[8])),
        uv: [header.uv_origin[0] + x, header.uv_origin[1] + z],
        weights,
        slope,
        morph: [
            parent_height,
            parent_normal[0],
            parent_normal[2],
            header.size,
        ],
        tint: [t0, t1, t2, t3],
        cover: [c0, c1, c2, c3],
        far_factor: unpack_far_factor(w[15]),
        sky: w[6].to_le_bytes(),
        sky_parent: w[7].to_le_bytes(),
    }
}

/// The CPU mirror of `terrain_common.wgsl`'s `tint_parent`: the tint and
/// far factor at shared mesh vertex `place`'s point on the parent level's
/// mesh, which the vertex stage morphs them to with the height, so a level
/// swap does not change the colour the near ground is pulled to. As
/// `mesh::Grid::parent`: an even vertex is its own, an odd one the
/// midpoint of the two parent vertices it lies between.
#[must_use]
pub fn tint_parent(packed: &PackedRegion, place: [f32; 3]) -> ([f32; 4], [f32; 3]) {
    let (i, j) = (place[0] as usize, place[2] as usize);
    let at = |i: usize, j: usize| {
        let u = unpack_vertex(packed, [i as f32, 0.0, j as f32]);
        (u.tint, u.far_factor)
    };
    let (a, b) = match (i % 2 == 1, j % 2 == 1) {
        (false, false) => return at(i, j),
        (true, false) => (at(i - 1, j), at(i + 1, j)),
        (false, true) => (at(i, j - 1), at(i, j + 1)),
        (true, true) => (at(i + 1, j - 1), at(i - 1, j + 1)),
    };
    (
        std::array::from_fn(|k| f32::midpoint(a.0[k], b.0[k])),
        std::array::from_fn(|k| f32::midpoint(a.1[k], b.1[k])),
    )
}

/// The shared mesh's places, `(i, skirt, j)` per vertex in the mesher's
/// vertex order: the grid row by row, then each edge's skirt (south and
/// north rows, west and east columns, `mesh::edge_indices`).
#[must_use]
pub fn shared_places() -> Vec<[f32; 3]> {
    let mut places = Vec::with_capacity(VERTEX_COUNT);
    for j in 0..GRID_N {
        for i in 0..GRID_N {
            places.push([i as f32, 0.0, j as f32]);
        }
    }
    let last = (GRID_N - 1) as f32;
    for k in 0..GRID_N {
        places.push([k as f32, SKIRT_ROW, 0.0]);
    }
    for k in 0..GRID_N {
        places.push([k as f32, SKIRT_ROW, last]);
    }
    for k in 0..GRID_N {
        places.push([0.0, SKIRT_COLUMN, k as f32]);
    }
    for k in 0..GRID_N {
        places.push([last, SKIRT_COLUMN, k as f32]);
    }
    places
}

/// The shared triangle list: the mesher's grid and skirt walls, which every
/// region winds alike. A skirt wall's winding is chosen by its face normal,
/// which is the edge's direction crossed with straight down whatever the
/// heights, so it is the same for every region; the tests hold the two
/// lists equal on built regions.
#[must_use]
pub fn shared_indices() -> Vec<u16> {
    let n = GRID_N as u16;
    let mut indices = Vec::with_capacity(INDEX_COUNT);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = j * n + i;
            let b = a + 1;
            let c = a + n;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    // Per edge (south, north, west, east), the quad `e0, e1, s1, s0` wound
    // to face outward: south and east as `a, b, c, a, c, d`, north and west
    // the other way (`mesh::push_outward_quad`).
    let outward_first = [true, false, false, true];
    for (e, &first) in outward_first.iter().enumerate() {
        let skirt_base = GRID_VERTICES as u16 + e as u16 * n;
        for k in 0..n - 1 {
            let edge = |k: u16| match e {
                0 => k,
                1 => (n - 1) * n + k,
                2 => k * n,
                _ => k * n + (n - 1),
            };
            let (e0, e1) = (edge(k), edge(k + 1));
            let (s0, s1) = (skirt_base + k, skirt_base + k + 1);
            if first {
                indices.extend_from_slice(&[e0, e1, s1, e0, s1, s0]);
            } else {
                indices.extend_from_slice(&[e0, s1, e1, e0, s0, s1]);
            }
        }
    }
    indices
}

/// The one mesh every region draws: [`shared_places`] in `POSITION` and the
/// [`shared_indices`]. The normal and UV are never read; they are there
/// because Bevy derives the pipelines' shader defs from the mesh's
/// attributes, and the terrain shaders are written for a mesh with both
/// (the prepass writes a normal and the fragment rebuilds Bevy's vertex
/// output with its UV).
#[must_use]
pub fn shared_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, shared_places())
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0_f32, 1.0, 0.0]; VERTEX_COUNT],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0_f32, 0.0]; VERTEX_COUNT])
    .with_inserted_indices(Indices::U16(shared_indices()))
}

/// The store's slots: a free list, a bump pointer past the highest slot
/// handed out, and the freed slots in quarantine. Every slot is the same
/// size, so there is nothing to merge; the lowest free slot goes first, so
/// the order is a function of the history.
#[derive(Debug, Default)]
pub struct Slots {
    top: u32,
    free: std::collections::BTreeSet<u32>,
    quarantine: VecDeque<(u64, u32)>,
}

impl Slots {
    /// A slot, the lowest free one, or a new one past the top.
    pub fn allocate(&mut self) -> u32 {
        if let Some(slot) = self.free.pop_first() {
            return slot;
        }
        self.top += 1;
        self.top - 1
    }

    /// Gives `slot` back, to be reused [`QUARANTINE_FRAMES`] after `frame`.
    pub fn free(&mut self, slot: u32, frame: u64) {
        self.quarantine.push_back((frame, slot));
    }

    /// Moves the slots whose quarantine is over at `frame` to the free list.
    pub fn recycle(&mut self, frame: u64) {
        while let Some(&(freed, slot)) = self.quarantine.front() {
            if freed + QUARANTINE_FRAMES > frame {
                break;
            }
            self.quarantine.pop_front();
            self.free.insert(slot);
        }
    }

    /// Slots below the highest one ever handed out: what the buffer must
    /// hold.
    #[must_use]
    pub fn top(&self) -> u32 {
        self.top
    }

    /// Slots handed out and not freed.
    #[must_use]
    pub fn live(&self) -> u32 {
        self.top - self.free.len() as u32 - self.quarantine.len() as u32
    }
}

/// The store slot a region entity draws, given back when the entity goes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(on_remove = release_slot)]
pub struct TerrainSlot(pub u32);

fn release_slot(mut world: DeferredWorld, context: HookContext) {
    let Some(TerrainSlot(slot)) = world.get::<TerrainSlot>(context.entity).copied() else {
        return;
    };
    if let Some(mut store) = world.get_resource_mut::<TerrainStore>() {
        let frame = store.frame;
        store.slots.free(slot, frame);
    }
}

/// One binding's worth of slots: a vertex, a header and a paint buffer and
/// the material that binds them.
struct Page {
    vertices: Buffer,
    headers: Buffer,
    paints: Buffer,
    /// Slots the buffers hold, up to [`TerrainStore`]'s page size.
    capacity: u32,
    /// The material whose bind group holds these buffers; a region in this
    /// page draws with it.
    material: Option<Handle<TerrainMaterial>>,
}

/// Where [`TerrainStore::install`] put a region: what its entity draws with.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The store slot, given back when the entity goes.
    pub slot: TerrainSlot,
    /// The slot within its page: the entity's `MeshTag`, which the shader
    /// indexes the page's buffers by.
    pub tag: u32,
    /// The page's material.
    pub material: Handle<TerrainMaterial>,
}

/// The page of global slot `slot` and the slot within it, for pages of
/// `page_slots`.
#[must_use]
pub fn page_of(slot: u32, page_slots: u32) -> (usize, u32) {
    ((slot / page_slots) as usize, slot % page_slots)
}

/// The GPU side of every region: its pages of vertex and header buffers,
/// their slots, the shared mesh and the materials that bind them.
///
/// A page holds at most what the device's largest storage binding does, so
/// on an adapter with small limits (wgpu's defaults bind 128 MiB, 529 slots,
/// where the moving fixtures keep about 1 000 regions) the store opens a
/// second page with a material of its own instead of leaving regions
/// undrawn. On a desktop card the first page grows to thousands of slots and
/// there is one page and one material, so every region batches together.
///
/// Present only where the terrain material is (a renderer); a headless app
/// meshes regions and never uploads them.
#[derive(Resource, Default)]
pub struct TerrainStore {
    pages: Vec<Page>,
    /// The most slots one page holds: the device's largest storage binding.
    page_slots: u32,
    slots: Slots,
    frame: u64,
    mesh: Option<Handle<Mesh>>,
}

impl TerrainStore {
    /// Makes the first page and the shared mesh, once, at [`INITIAL_SLOTS`]
    /// or what the device's largest storage binding holds, if less.
    pub fn build(&mut self, device: &RenderDevice, meshes: &mut Assets<Mesh>) {
        let bound = Self::largest_binding(device);
        self.build_paged(device, meshes, bound);
    }

    /// [`Self::build`] with pages of at most `bound` bytes, for a run that
    /// wants to see the paging on a card that would not need it.
    pub fn build_paged(&mut self, device: &RenderDevice, meshes: &mut Assets<Mesh>, bound: u64) {
        if !self.pages.is_empty() {
            return;
        }
        let bound = bound.min(Self::largest_binding(device));
        self.page_slots = u32::try_from(bound / SLOT_BYTES).unwrap_or(u32::MAX).max(1);
        let capacity = INITIAL_SLOTS.min(self.page_slots);
        self.pages.push(Page {
            vertices: vertex_buffer(device, capacity),
            headers: header_buffer(device, capacity),
            paints: paint_buffer(device, capacity),
            capacity,
            material: None,
        });
        self.mesh = Some(meshes.add(shared_mesh()));
        info!(
            "terrain store: {capacity} slots of {SLOT_BYTES} bytes, {:.0} MiB; a page binds up to {} slots ({:.0} MiB)",
            mib(u64::from(capacity) * SLOT_BYTES),
            self.page_slots,
            mib(u64::from(self.page_slots) * SLOT_BYTES),
        );
        if self.page_slots < INITIAL_SLOTS {
            info!(
                page_slots = self.page_slots,
                "the device's largest storage binding holds fewer terrain slots than a moving view keeps; the store pages, a material a page"
            );
        }
    }

    fn largest_binding(device: &RenderDevice) -> u64 {
        let limits = device.limits();
        limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size)
    }

    /// The first page's vertex buffer, which the first material binds.
    #[must_use]
    pub fn vertices(&self) -> Option<&Buffer> {
        self.pages.first().map(|page| &page.vertices)
    }

    /// The first page's header buffer, which the first material binds.
    #[must_use]
    pub fn headers(&self) -> Option<&Buffer> {
        self.pages.first().map(|page| &page.headers)
    }

    /// The first page's paint buffer, which the first material binds.
    #[must_use]
    pub fn paints(&self) -> Option<&Buffer> {
        self.pages.first().map(|page| &page.paints)
    }

    /// The shared mesh every region draws, once built.
    #[must_use]
    pub fn mesh(&self) -> Option<&Handle<Mesh>> {
        self.mesh.as_ref()
    }

    /// Remembers the material that binds the first page's buffers: a growth
    /// points it at the new ones, and a new page's material is a copy of it.
    pub fn set_material(&mut self, material: Handle<TerrainMaterial>) {
        if let Some(page) = self.pages.first_mut() {
            page.material = Some(material);
        }
    }

    /// Every page's material, so a change to the look reaches all of them.
    pub fn materials(&self) -> impl Iterator<Item = &Handle<TerrainMaterial>> {
        self.pages.iter().filter_map(|page| page.material.as_ref())
    }

    /// Pages open.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// Starts a frame: the slots whose quarantine is over become free.
    pub fn next_frame(&mut self) {
        self.frame += 1;
        self.slots.recycle(self.frame);
    }

    /// Slots held by drawn or quarantined regions, and the buffers'
    /// capacity over every page, for the log.
    #[must_use]
    pub fn occupancy(&self) -> (u32, u32) {
        (
            self.slots.live(),
            self.pages.iter().map(|page| page.capacity).sum(),
        )
    }

    /// Writes one region into a slot: a page opened or grown first when the
    /// slot is past what the pages hold, then one `write_buffer` for the
    /// records, one for the header and one for the paint. `None` only before
    /// the first page and its material exist, or when a new page has no
    /// material to copy.
    pub fn install(
        &mut self,
        packed: &PackedRegion,
        device: &RenderDevice,
        queue: &RenderQueue,
        mut materials: Option<&mut Assets<TerrainMaterial>>,
    ) -> Option<Placed> {
        if self.pages.is_empty() {
            return None;
        }
        let slot = self.slots.allocate();
        let (page, tag) = page_of(slot, self.page_slots);
        // The lowest free slot goes first, so a slot past every page is the
        // first of the next one.
        if page == self.pages.len() && !self.open_page(device, materials.as_deref_mut()) {
            self.slots.free(slot, self.frame);
            return None;
        }
        if tag >= self.pages[page].capacity {
            self.grow(page, tag + 1, device, queue, materials);
        }
        let page = &self.pages[page];
        let material = page.material.clone()?;
        queue.write_buffer(&page.vertices, u64::from(tag) * SLOT_BYTES, &packed.bytes);
        queue.write_buffer(
            &page.headers,
            u64::from(tag) * HEADER_BYTES,
            &packed.header.bytes(),
        );
        queue.write_buffer(&page.paints, u64::from(tag) * PAINT_BYTES, &packed.paint);
        Some(Placed {
            slot: TerrainSlot(slot),
            tag,
            material,
        })
    }

    /// Opens a full page with a copy of the first page's material bound to
    /// its buffers.
    fn open_page(
        &mut self,
        device: &RenderDevice,
        materials: Option<&mut Assets<TerrainMaterial>>,
    ) -> bool {
        let (Some(materials), Some(first)) = (
            materials,
            self.pages.first().and_then(|page| page.material.as_ref()),
        ) else {
            return false;
        };
        let Some(mut material) = materials.get(first).cloned() else {
            return false;
        };
        let capacity = self.page_slots;
        let vertices = vertex_buffer(device, capacity);
        let headers = header_buffer(device, capacity);
        let paints = paint_buffer(device, capacity);
        material.extension.vertices = vertices.clone();
        material.extension.slots = headers.clone();
        material.extension.paint = paints.clone();
        let handle = materials.add(material);
        info!(
            "terrain store opened page {} of {capacity} slots ({:.0} MiB)",
            self.pages.len(),
            mib(u64::from(capacity) * SLOT_BYTES)
        );
        self.pages.push(Page {
            vertices,
            headers,
            paints,
            capacity,
            material: Some(handle),
        });
        true
    }

    /// Grows page `index`'s buffers by half until they hold `slots`, never
    /// past the page size, copies the slots over on the GPU and points the
    /// page's material at the new buffers. The copy is submitted at once,
    /// after any write still queued for the old buffers, which the queue
    /// runs first; the frame the render world is drawing keeps the old
    /// buffers alive through its bind group.
    fn grow(
        &mut self,
        index: usize,
        slots: u32,
        device: &RenderDevice,
        queue: &RenderQueue,
        materials: Option<&mut Assets<TerrainMaterial>>,
    ) {
        let page_slots = self.page_slots;
        let page = &mut self.pages[index];
        let mut capacity = page.capacity.max(1);
        while capacity < slots {
            capacity = capacity.saturating_add(capacity.div_ceil(2));
        }
        let capacity = capacity.min(page_slots);
        let vertices = vertex_buffer(device, capacity);
        let headers = header_buffer(device, capacity);
        let paints = paint_buffer(device, capacity);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("terrain store growth"),
        });
        let old = u64::from(page.capacity);
        encoder.copy_buffer_to_buffer(&page.vertices, 0, &vertices, 0, old * SLOT_BYTES);
        encoder.copy_buffer_to_buffer(&page.headers, 0, &headers, 0, old * HEADER_BYTES);
        encoder.copy_buffer_to_buffer(&page.paints, 0, &paints, 0, old * PAINT_BYTES);
        queue.submit([encoder.finish()]);
        if let (Some(handle), Some(materials)) = (page.material.as_ref(), materials)
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.vertices = vertices.clone();
            material.extension.slots = headers.clone();
            material.extension.paint = paints.clone();
        }
        info!(
            "terrain store page {index} grew from {} to {capacity} slots ({:.0} MiB)",
            page.capacity,
            mib(u64::from(capacity) * SLOT_BYTES)
        );
        page.vertices = vertices;
        page.headers = headers;
        page.paints = paints;
        page.capacity = capacity;
    }
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn vertex_buffer(device: &RenderDevice, slots: u32) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("terrain vertices"),
        size: u64::from(slots) * SLOT_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn paint_buffer(device: &RenderDevice, slots: u32) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("terrain paint"),
        size: u64::from(slots) * PAINT_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn header_buffer(device: &RenderDevice, slots: u32) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("terrain slots"),
        size: u64::from(slots) * HEADER_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "the store keeps heights and positions to the bit; the tests say so"
    )]

    use strata_core::{RegionCoord, Seed};
    use strata_worldgen::ProceduralWorld;

    use super::*;
    use crate::material::{UV_PERIOD_M, dev_asset_root};
    use crate::mesh::{BiomeLayers, build_region_mesh, build_region_mesh_pinned};

    /// The seed-42 world and its layer table.
    fn world() -> (ProceduralWorld, BiomeLayers) {
        let registries =
            strata_registry::load_from_dir(dev_asset_root()).expect("the shipped assets load");
        (
            ProceduralWorld::new(Seed(42), &registries),
            BiomeLayers::from_registries(&registries),
        )
    }

    fn build(world: &ProceduralWorld, layers: &BiomeLayers, coord: RegionCoord) -> RegionMesh {
        build_region_mesh(coord, world, layers, 0.05, &mut Vec::new())
    }

    /// Regions of every kind the store takes: the finest level by the
    /// origin, coarse ones, far out where the UV wraps from a large base,
    /// at negative coordinates, and one pinned by a detail zone.
    fn sample_regions(world: &ProceduralWorld, layers: &BiomeLayers) -> Vec<RegionMesh> {
        let mut regions: Vec<RegionMesh> = [
            RegionCoord::new(0, 0, 0),
            RegionCoord::new(-3, 7, 0),
            RegionCoord::new(234, -197, 0),
            RegionCoord::new(5, -2, 3),
            RegionCoord::new(-1, 1, 6),
            RegionCoord::new(0, -1, 8),
        ]
        .into_iter()
        .map(|coord| build(world, layers, coord))
        .collect();
        regions.push(build_region_mesh_pinned(
            RegionCoord::new(2, 2, 1),
            world,
            layers,
            0.05,
            true,
            &mut Vec::new(),
        ));
        regions
    }

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    /// The shader's unpacking, through its CPU mirror, gives back every
    /// vertex of the mesher's own streams, skirts included: heights, the
    /// morph, positions and UVs to the bit, bytes as they were, normals to a
    /// ten-thousandth and the half-precision colours to a part in a
    /// thousand.
    #[test]
    fn the_unpacked_store_is_the_meshers_vertex() {
        let (world, layers) = world();
        let places = shared_places();
        assert_eq!(places.len(), VERTEX_COUNT);
        for mesh in sample_regions(&world, &layers) {
            let packed = pack_region(&mesh);
            assert_eq!(packed.bytes.len() as u64, SLOT_BYTES);
            for (v, place) in places.iter().enumerate() {
                let u = unpack_vertex(&packed, *place);
                let at = format!("{:?} vertex {v}", mesh.coord);
                assert_eq!(
                    u.position.map(f32::to_bits),
                    mesh.positions[v].map(f32::to_bits),
                    "{at}"
                );
                assert_eq!(
                    u.uv.map(f32::to_bits),
                    mesh.uvs[v].map(f32::to_bits),
                    "{at}"
                );
                assert_eq!(u.weights, mesh.weights[v], "{at}");
                assert_eq!(u.slope, mesh.slope[v], "{at}");
                assert_eq!(u.sky, mesh.sky[v], "{at}");
                assert_eq!(u.sky_parent, mesh.sky_parent[v], "{at}");
                let morph = mesh.morph[v];
                assert_eq!(u.morph[0].to_bits(), morph[0].to_bits(), "{at}");
                assert_eq!(u.morph[3], morph[3], "{at}");
                assert!(close(u.morph[1], morph[1], 1e-4), "{at}: {u:?}");
                assert!(close(u.morph[2], morph[2], 1e-4), "{at}: {u:?}");
                for k in 0..3 {
                    assert!(close(u.normal[k], mesh.normals[v][k], 1e-4), "{at}: {u:?}");
                }
                for k in 0..4 {
                    let tint = mesh.tints[v][k];
                    let cover = mesh.covers[v][k];
                    assert!(
                        close(u.tint[k], tint, tint.abs() * 1e-3 + 1e-6),
                        "{at}: {u:?}"
                    );
                    assert!(
                        close(u.cover[k], cover, cover.abs() * 1e-3 + 1e-6),
                        "{at}: {u:?}"
                    );
                }
                for k in 0..3 {
                    assert!(
                        close(u.far_factor[k], mesh.far_factors[v][k], 0.5 / 128.0 + 1e-6),
                        "{at}: {u:?}"
                    );
                    if mesh.far_factors[v][k] == 1.0 {
                        assert_eq!(u.far_factor[k], 1.0, "{at}");
                    }
                }
            }
        }
    }

    /// Every region winds its triangles as the shared mesh does, so one
    /// index list draws them all; it covers the grid and the four skirt
    /// walls and touches every vertex.
    #[test]
    fn the_shared_mesh_is_every_regions_grid_and_skirts() {
        let (world, layers) = world();
        let indices = shared_indices();
        assert_eq!(indices.len(), INDEX_COUNT);
        let mut used = vec![false; VERTEX_COUNT];
        for &i in &indices {
            used[usize::from(i)] = true;
        }
        assert!(used.iter().all(|&u| u), "a vertex no triangle draws");
        let wide: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();
        for mesh in sample_regions(&world, &layers) {
            assert_eq!(wide, mesh.indices, "{:?}", mesh.coord);
        }
        let mesh = shared_mesh();
        assert_eq!(mesh.count_vertices(), VERTEX_COUNT);
        assert_eq!(mesh.indices().map(Indices::len), Some(INDEX_COUNT));
    }

    /// Two neighbours of one level read the same ground on their shared
    /// edge from the store, height, normal and morph alike, and the same
    /// texels: their UVs differ by whole wrap periods.
    #[test]
    fn neighbours_agree_on_their_shared_edge() {
        let (world, layers) = world();
        let places = shared_places();
        let last = (GRID_N - 1) as f32;
        for (west, east) in [
            (RegionCoord::new(0, 0, 0), RegionCoord::new(1, 0, 0)),
            (RegionCoord::new(-1, 3, 2), RegionCoord::new(0, 3, 2)),
            // Across a wrap line: x = 5040 m lies inside the second.
            (RegionCoord::new(77, -4, 0), RegionCoord::new(78, -4, 0)),
        ] {
            let a = pack_region(&build(&world, &layers, west));
            let b = pack_region(&build(&world, &layers, east));
            for j in 0..GRID_N {
                let on_a = unpack_vertex(&a, [last, 0.0, j as f32]);
                let on_b = unpack_vertex(&b, [0.0, 0.0, j as f32]);
                let at = format!("{west:?} | {east:?} row {j}");
                assert_eq!(on_a.position[1], on_b.position[1], "{at}");
                assert_eq!(on_a.morph[0], on_b.morph[0], "{at}");
                assert_eq!(on_a.normal, on_b.normal, "{at}");
                assert_eq!(on_a.sky, on_b.sky, "{at}");
                let wraps = (on_a.uv[0] - on_b.uv[0]) / UV_PERIOD_M;
                assert_eq!(wraps, wraps.round(), "{at}: {:?} {:?}", on_a.uv, on_b.uv);
                assert_eq!(on_a.uv[1], on_b.uv[1], "{at}");
            }
            // The places are the shared mesh's own.
            assert!(places.contains(&[last, 0.0, 0.0]));
        }
    }

    /// A fine region's edge, fully morphed, lies on its coarse neighbour's
    /// edge: every vertex's parent height is the coarse vertex at its place,
    /// or the midpoint of the two it lies between.
    #[test]
    fn a_fine_edge_morphs_onto_its_coarse_neighbour() {
        let (world, layers) = world();
        let fine = RegionCoord::new(3, 2, 0);
        // The level-1 region east of the fine one's parent.
        let coarse = RegionCoord::new(2, 1, 1);
        assert_eq!(
            fine.min_corner().x() + fine.size(),
            coarse.min_corner().x(),
            "the coarse region is the fine one's east neighbour"
        );
        let f = pack_region(&build(&world, &layers, fine));
        let c = pack_region(&build(&world, &layers, coarse));
        let last = (GRID_N - 1) as f32;
        // The fine region covers the coarse one's southern half, rows 0 to
        // 32 of the coarse grid.
        let offset = ((fine.min_corner().z() - coarse.min_corner().z()) / coarse.vertex_spacing())
            .round() as usize;
        for j in 0..GRID_N {
            let edge = unpack_vertex(&f, [last, 0.0, j as f32]);
            let row = offset + j / 2;
            let coarse_height = |row: usize| unpack_vertex(&c, [0.0, 0.0, row as f32]).position[1];
            let expected = if j % 2 == 0 {
                coarse_height(row)
            } else {
                f32::midpoint(coarse_height(row), coarse_height(row + 1))
            };
            assert!(
                close(edge.morph[0], expected, 1e-3),
                "row {j}: {} against {expected}",
                edge.morph[0]
            );
        }
    }

    /// The tint the vertex stage morphs to lies on the coarse neighbour's
    /// edge too (ticket 0006: the near ground's palette is pulled to it, and
    /// an unmorphed tint changed under the eye at every level swap): even
    /// rows are the coarse vertex's own, odd ones the midpoint of the two
    /// they lie between, to the half precision of the record.
    #[test]
    fn a_fine_edge_tint_morphs_onto_its_coarse_neighbour() {
        let (world, layers) = world();
        let fine = RegionCoord::new(3, 2, 0);
        let coarse = RegionCoord::new(2, 1, 1);
        let f = pack_region(&build(&world, &layers, fine));
        let c = pack_region(&build(&world, &layers, coarse));
        let last = (GRID_N - 1) as f32;
        let offset = ((fine.min_corner().z() - coarse.min_corner().z()) / coarse.vertex_spacing())
            .round() as usize;
        let coarse_at = |row: usize| unpack_vertex(&c, [0.0, 0.0, row as f32]);
        for j in 0..GRID_N {
            let (tint, factor) = tint_parent(&f, [last, 0.0, j as f32]);
            let row = offset + j / 2;
            let (want_tint, want_factor): ([f32; 4], [f32; 3]) = if j % 2 == 0 {
                let u = coarse_at(row);
                (u.tint, u.far_factor)
            } else {
                let (a, b) = (coarse_at(row), coarse_at(row + 1));
                (
                    std::array::from_fn(|k| f32::midpoint(a.tint[k], b.tint[k])),
                    std::array::from_fn(|k| f32::midpoint(a.far_factor[k], b.far_factor[k])),
                )
            };
            for k in 0..4 {
                assert!(
                    close(tint[k], want_tint[k], want_tint[k].abs() * 2e-3 + 1e-6),
                    "row {j}: {tint:?} against {want_tint:?}"
                );
            }
            for k in 0..3 {
                assert!(
                    close(factor[k], want_factor[k], 1e-6),
                    "row {j}: {factor:?} against {want_factor:?}"
                );
            }
        }
    }

    #[test]
    fn a_freed_slot_waits_out_its_quarantine() {
        let mut slots = Slots::default();
        assert_eq!(
            (slots.allocate(), slots.allocate(), slots.allocate()),
            (0, 1, 2)
        );
        slots.free(1, 10);
        slots.free(0, 11);
        slots.recycle(12);
        assert_eq!(
            slots.allocate(),
            3,
            "slot 1 is in quarantine until frame 13"
        );
        slots.recycle(13);
        assert_eq!(slots.allocate(), 1, "slot 1 is free at frame 13");
        assert_eq!(slots.allocate(), 4, "slot 0 is not, until frame 14");
        slots.recycle(14);
        assert_eq!(slots.allocate(), 0);
        assert_eq!(slots.top(), 5);
        assert_eq!(slots.live(), 5);
        slots.free(4, 20);
        assert_eq!(slots.live(), 4, "a slot in quarantine is not live");
    }

    /// Slots fill the first page before the second, and a slot's tag is
    /// its place in its page, so a page's buffers are indexed from zero.
    #[test]
    fn slots_fill_pages_in_order() {
        let mut slots = Slots::default();
        let placed: Vec<(usize, u32)> = (0..7).map(|_| page_of(slots.allocate(), 3)).collect();
        assert_eq!(
            placed,
            [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (2, 0)]
        );
        slots.free(1, 0);
        slots.recycle(QUARANTINE_FRAMES);
        assert_eq!(
            page_of(slots.allocate(), 3),
            (0, 1),
            "a freed slot in the first page goes first"
        );
        assert_eq!(page_of(529, 529), (1, 0));
        assert_eq!(page_of(1535, u32::MAX), (0, 1535));
    }

    #[test]
    fn halves_round_to_nearest_even_and_back() {
        for value in [0.0_f32, 1.0, 0.5, 0.333_333, 1.0e-5, 2.75, 65_504.0, -0.25] {
            let back = unpack_half2(pack_half2(value, -value));
            assert!(
                close(back[0], value, value.abs() / 2048.0 + 6.0e-8),
                "{value}: {back:?}"
            );
            assert_eq!(back[1], -back[0]);
        }
        // 1 + 2^-11 is halfway between two halves; the even one is 1.
        assert_eq!(unpack_half2(pack_half2(1.0 + 1.0 / 2048.0, 0.0))[0], 1.0);
        assert_eq!(unpack_half2(pack_half2(1.0e6, 0.0))[0], f32::INFINITY);
    }

    #[test]
    fn octahedral_normals_round_trip_both_hemispheres() {
        for n in [
            [0.0_f32, 1.0, 0.0],
            [0.6, 0.8, 0.0],
            [-0.48, 0.6, -0.64],
            [0.0, -1.0, 0.0],
            [0.36, -0.48, 0.8],
        ] {
            let back = oct_decode(unpack_snorm2(pack_snorm2(oct_encode(n))));
            for k in 0..3 {
                assert!(close(back[k], n[k], 1e-4), "{n:?}: {back:?}");
            }
        }
    }
}
