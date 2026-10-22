//! Grass: a shared blade mesh per size bucket and LOD, one material for
//! every cell, and two shared storage buffers that hold every resident
//! cell's blade records as `strata-flora` produced them (ADR 0005) and one
//! small record per cell that says where its blades are.
//!
//! The mesh carries no blade: it is `n` copies of a unit blade whose
//! vertices say only "which vertex of a blade am I" (`uv`: side and height
//! fraction) and "how the mesh maps onto the records" (`uv_b`: the record
//! stride and the vertices per blade). The vertex shader takes its cell
//! record by the entity's `MeshTag`, its blade by `vertex_index /
//! vertices_per_blade * stride` from the cell's first, places the blade on
//! the ground at the record's root with the ground's normal, gives it the
//! species' width and tint, and bends it. A slot past the cell's count
//! collapses to a point. So a cell of blades is one `write_buffer` of 16
//! bytes per blade and one of 16 for its record, and the meshes are uploaded
//! once; the per-vertex upload path the spike measured at 5 ms per megabyte
//! is never taken again.
//!
//! A buffer and a material per cell, as the cells first had, made a batch
//! set per visible cell in every pass it drew in: Bevy builds a bind group
//! and dispatches a bin unpacking compute per set, per view, per frame, and
//! re-prepared a material for every cell that streamed in
//! (`docs/perf/profile-2026-10-03.md`, 4.2 and 4.4). With every cell the same
//! material over the same buffers, the grass is one set a pass.

use std::collections::{BTreeMap, VecDeque};

use bevy::asset::{Asset, Assets, Handle, RenderAssetUsages};
use bevy::math::{Vec2, Vec4};
use bevy::mesh::{Indices, Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    StandardMaterial,
};
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferInitDescriptor, BufferUsages, CommandEncoderDescriptor,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::shader::ShaderRef;
use strata_flora::GrassBlade;

use crate::impostor::QUARANTINE_FRAMES;

/// The main-pass shader, relative to the asset root.
pub const GRASS_SHADER_PATH: &str = "shaders/grass.wgsl";
/// The prepass vertex shader.
pub const GRASS_PREPASS_SHADER_PATH: &str = "shaders/grass_prepass.wgsl";
/// The wind, shared with every foliage shader.
pub const FOLIAGE_WIND_SHADER_PATH: &str = "shaders/foliage_wind.wgsl";
/// The grass vertex placement, shared by the main-pass and prepass shaders.
pub const GRASS_COMMON_SHADER_PATH: &str = "shaders/grass_common.wgsl";

/// The most species one cell's buffer can name; the record's species byte
/// allows 256, the uniform carries the tints of this many.
pub use strata_registry::flora::MAX_GRASS_SPECIES;

/// Blade slots per shared mesh, smallest first. A cell takes the smallest
/// that fits its record count; the rest of the slots collapse.
pub const BUCKETS: [u32; 5] = [2048, 4096, 8192, 16384, 32768];

/// Vertices per blade in the near mesh: three quads and a tip.
pub const NEAR_VERTICES: u32 = 7;
/// Vertices per blade in the mid mesh: one triangle.
pub const MID_VERTICES: u32 = 3;
/// Records the mid mesh skips between blades.
pub const MID_STRIDE: u32 = 4;

/// One LOD of the grass meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrassLod {
    /// Full blades, drawn to the near fade.
    Near,
    /// One triangle per fourth blade, drawn to the far fade.
    Mid,
}

impl GrassLod {
    /// Vertices per blade and record stride.
    #[must_use]
    pub const fn layout(self) -> (u32, u32) {
        match self {
            Self::Near => (NEAR_VERTICES, 1),
            Self::Mid => (MID_VERTICES, MID_STRIDE),
        }
    }

    /// The bucket index a cell of `blades` records takes at this LOD, or
    /// `None` for an empty or oversized cell. Placement bounds record counts
    /// to the largest near bucket; never silently select a truncated mesh.
    #[must_use]
    pub fn bucket_for(self, blades: u32) -> Option<usize> {
        let (_, stride) = self.layout();
        let slots = blades.div_ceil(stride);
        if slots == 0 {
            return None;
        }
        BUCKETS.iter().position(|&b| b >= slots)
    }
}

/// `slots` copies of the unit blade at `lod`.
#[must_use]
pub fn blade_mesh(lod: GrassLod, slots: u32) -> Mesh {
    let (per_blade, stride) = lod.layout();
    let n = slots as usize;
    let vpb = per_blade as usize;
    let mut positions = Vec::with_capacity(n * vpb);
    let mut uvs = Vec::with_capacity(n * vpb);
    let mut uvs_b = Vec::with_capacity(n * vpb);
    let mut indices = Vec::with_capacity(n * (vpb - 2) * 3);
    // Precision is irrelevant: the shader only reads the two small numbers.
    #[allow(clippy::cast_precision_loss)]
    let layout = [stride as f32, per_blade as f32];
    for b in 0..n {
        #[allow(clippy::cast_possible_truncation)]
        let base = (b * vpb) as u32;
        match lod {
            GrassLod::Near => {
                for seg in 0..3u32 {
                    #[allow(clippy::cast_precision_loss)]
                    let v = seg as f32 / 3.0;
                    for side in [0.0f32, 1.0] {
                        positions.push([side - 0.5, v, 0.0]);
                        uvs.push([side, v]);
                        uvs_b.push(layout);
                    }
                }
                positions.push([0.0, 1.0, 0.0]);
                uvs.push([0.5, 1.0]);
                uvs_b.push(layout);
                for seg in 0..2u32 {
                    let l0 = base + seg * 2;
                    indices.extend_from_slice(&[l0, l0 + 1, l0 + 2, l0 + 1, l0 + 3, l0 + 2]);
                }
                indices.extend_from_slice(&[base + 4, base + 5, base + 6]);
            }
            GrassLod::Mid => {
                positions.extend_from_slice(&[[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [0.0, 1.0, 0.0]]);
                uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]);
                uvs_b.extend_from_slice(&[layout; 3]);
                indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvs_b)
    .with_inserted_indices(Indices::U32(indices))
}

/// The wind and the render origin, mirrored field for field by
/// `FoliageWind` in the shader.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct WindParams {
    /// Unit horizontal wind direction, `x, z`.
    pub direction: Vec2,
    /// Metres of tip displacement for a one metre plant at full gust.
    pub strength: f32,
    /// Seconds.
    pub time: f32,
    /// The previous frame's time, for motion vectors.
    pub prev_time: f32,
    /// The render origin's `x` wrapped to [`WIND_PERIOD_M`].
    pub origin_x: f32,
    /// The render origin's `z` wrapped to [`WIND_PERIOD_M`].
    pub origin_z: f32,
    /// Metres per gust noise cell.
    pub gust_scale: f32,
    /// Previous frame's wrapped render origin, so motion-vector wind is
    /// sampled in the previous model's coordinate frame during a rebase.
    pub prev_origin_x: f32,
    /// Previous frame's wrapped render origin z component.
    pub prev_origin_z: f32,
    /// How much the placer's sky term under crowns darkens the sky's fill
    /// on trees and grass, 0 to 1 (lighting plan phase 3a). In the wind's
    /// buffer because it is the one every foliage shader already binds.
    pub sky_visibility: f32,
    /// How much sunlight comes through a blade seen against the sun and
    /// rims its edges, 0 to 2 (lighting plan phase 5b); zero skips the
    /// term and its shadow lookup. Grass alone reads it.
    pub grass_rim: f32,
    /// The shade's colour, a luminance-one multiplier on the indirect
    /// diffuse of trees and grass (visual.md F4), as three floats because a
    /// `vec3` would raise the struct's alignment to sixteen.
    pub shade_tint: [f32; 3],
    /// The crown's core multiplier per colour lane, the core share times its
    /// cool lean (`CanopyLook`, visual.md F5); in the wind's buffer because
    /// every tree shader already binds it.
    pub canopy_core: [f32; 3],
    /// How much a bulging clump lifts and a crease darkens a leaf.
    pub canopy_lift: f32,
    /// The sunlight through the outer leaves seen against the sun, 0 to 2.
    pub canopy_rim: f32,
}

/// The gust lattice repeats every this many metres; the origin offset is
/// wrapped to it so `f32` never sees a large world coordinate.
pub const WIND_PERIOD_M: f64 = 4096.0;

impl Default for WindParams {
    fn default() -> Self {
        Self {
            direction: Vec2::new(0.8, 0.6),
            strength: 0.35,
            time: 0.0,
            prev_time: 0.0,
            origin_x: 0.0,
            origin_z: 0.0,
            gust_scale: 40.0,
            prev_origin_x: 0.0,
            prev_origin_z: 0.0,
            sky_visibility: 1.0,
            grass_rim: 1.0,
            shade_tint: [1.0; 3],
            canopy_core: [1.0; 3],
            canopy_lift: 0.0,
            canopy_rim: 0.0,
        }
    }
}

/// The species table, the same for every cell; what differs between cells
/// is in their [`GrassCellRecord`].
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct GrassParams {
    /// Blade width in metres per species, four to a `Vec4`.
    pub width: [Vec4; MAX_GRASS_SPECIES / 4],
    /// Resting curvature, tip droop, dead blade share and reserved padding.
    pub shape: [Vec4; MAX_GRASS_SPECIES],
    /// Dry tint per species, linear RGB.
    pub tint_dry: [Vec4; MAX_GRASS_SPECIES],
    /// Lush tint per species, linear RGB.
    pub tint_lush: [Vec4; MAX_GRASS_SPECIES],
}

impl Default for GrassParams {
    fn default() -> Self {
        Self {
            width: [Vec4::splat(0.04); MAX_GRASS_SPECIES / 4],
            shape: [Vec4::new(0.2, 0.08, 0.1, 0.0); MAX_GRASS_SPECIES],
            tint_dry: [Vec4::new(0.5, 0.5, 0.2, 1.0); MAX_GRASS_SPECIES],
            tint_lush: [Vec4::new(0.2, 0.5, 0.1, 1.0); MAX_GRASS_SPECIES],
        }
    }
}

/// The extension half of [`GrassMaterial`], the one material every grass
/// cell draws with. Bindings 100 to 103 keep clear of the base's.
///
/// The wind is a buffer the plugin writes once per frame with
/// `write_buffer`, rather than a uniform in the material: changing a
/// material asset re-prepares its bind group, which with one material is
/// cheap but would still be a bind group a frame for nothing.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(from_reflect = false)]
pub struct GrassField {
    /// The species table.
    #[uniform(100)]
    pub params: GrassParams,
    /// Every resident cell's blade records, 16 bytes each; a cell owns the
    /// range its [`GrassCellRecord`] names.
    #[storage(101, read_only, buffer)]
    #[reflect(ignore)]
    pub blades: Buffer,
    /// The wind, one [`WindParams`], shared with the trees.
    #[storage(102, read_only, buffer)]
    #[reflect(ignore)]
    pub wind: Buffer,
    /// One [`GrassCellRecord`] per cell slot; an entity's `MeshTag` is its
    /// cell's slot.
    #[storage(103, read_only, buffer)]
    #[reflect(ignore)]
    pub cells: Buffer,
}

/// What one cell's entities read of their cell, mirrored by the shader's
/// `GrassCellRecord`: 16 bytes at the cell's slot in the cell buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrassCellRecord {
    /// The cell's first blade record in the blade buffer.
    pub start: u32,
    /// Its blade records; slots past them collapse.
    pub count: u32,
    /// The height of the cell's mesh origin, so visibility and the shader's
    /// roots share one elevated frame.
    pub origin_y: f32,
    /// When the cell was spawned, seconds, for the blades' growth from the
    /// ground; a full `f32` of seconds, as the old per-cell uniform carried.
    pub spawn_time: f32,
}

impl GrassCellRecord {
    /// The record as the shader reads it.
    #[must_use]
    pub fn bytes(&self) -> [u8; 16] {
        let mut out = [0; 16];
        out[0..4].copy_from_slice(&self.start.to_le_bytes());
        out[4..8].copy_from_slice(&self.count.to_le_bytes());
        out[8..12].copy_from_slice(&self.origin_y.to_le_bytes());
        out[12..16].copy_from_slice(&self.spawn_time.to_le_bytes());
        out
    }
}

/// The shared wind buffer: [`WindParams`] as the shader reads it.
#[must_use]
pub fn wind_buffer(device: &RenderDevice) -> Buffer {
    device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("foliage wind"),
        contents: &wind_bytes(&WindParams::default()),
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    })
}

/// The size of `FoliageWind` in the shader: twenty floats, a multiple of the
/// struct's alignment of eight.
pub const WIND_BYTES: usize = 80;

/// [`WindParams`] as bytes in the shader's layout.
#[must_use]
pub fn wind_bytes(w: &WindParams) -> Vec<u8> {
    let mut out = Vec::with_capacity(WIND_BYTES);
    for v in [
        w.direction.x,
        w.direction.y,
        w.strength,
        w.time,
        w.prev_time,
        w.origin_x,
        w.origin_z,
        w.gust_scale,
        w.prev_origin_x,
        w.prev_origin_z,
        w.sky_visibility,
        w.grass_rim,
        w.shade_tint[0],
        w.shade_tint[1],
        w.shade_tint[2],
        w.canopy_core[0],
        w.canopy_core[1],
        w.canopy_core[2],
        w.canopy_lift,
        w.canopy_rim,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(out.len(), WIND_BYTES);
    out
}

impl MaterialExtension for GrassField {
    fn vertex_shader() -> ShaderRef {
        GRASS_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        GRASS_SHADER_PATH.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        GRASS_PREPASS_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

/// The grass material as an asset type.
pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassField>;

/// A cell's records as the bytes the shader reads: the struct is
/// `repr(C)` and 16 bytes, and the shader's `Blade` mirrors it.
#[must_use]
pub fn blade_bytes(blades: &[GrassBlade]) -> Vec<u8> {
    let mut out = Vec::with_capacity(blades.len() * 16);
    for b in blades {
        out.extend_from_slice(&b.x.to_le_bytes());
        out.extend_from_slice(&b.z.to_le_bytes());
        out.extend_from_slice(&b.y_bits.to_le_bytes());
        out.extend_from_slice(&[b.height_cm, b.phase, b.stiffness, b.species]);
        out.extend_from_slice(&[
            b.lush,
            b.shade,
            b.nx.to_le_bytes()[0],
            b.nz.to_le_bytes()[0],
        ]);
    }
    out
}

/// Bytes of one blade record.
pub const BLADE_BYTES: u64 = 16;
/// Bytes of one [`GrassCellRecord`].
pub const CELL_BYTES: u64 = 16;

/// Blade records the shared buffer starts with: 16 MiB, about the woodland's
/// 1.2 million resident blades; it doubles when full.
pub const INITIAL_BLADES: u32 = 1 << 20;
/// Cell slots the cell buffer starts with, three times the woodland's
/// resident grass cells; it doubles when full.
pub const INITIAL_CELLS: u32 = 1024;
/// A cell's blade range is its record count rounded up to this, so a range
/// a cell gives back fits a cell of about its size and the free list stays
/// short; the slack is under 4 KiB a cell.
pub const BLADE_GRANULE: u32 = 256;

/// Ranges of a shared buffer, in records: a bump pointer past the highest
/// range in use, the free ranges by start, merged with their neighbours,
/// and the freed ranges still in quarantine. A cell's blades take the
/// smallest free range that holds them (best fit), so the ranges a moving
/// ring of cells gives back are taken by the cells it gains, which are of
/// the same sizes, and the pointer only moves when nothing free fits.
#[derive(Debug, Default)]
pub struct Ranges {
    top: u32,
    free: BTreeMap<u32, u32>,
    quarantine: VecDeque<(u64, u32, u32)>,
}

impl Ranges {
    /// The start of a range of `len` records.
    pub fn allocate(&mut self, len: u32) -> u32 {
        let best = self
            .free
            .iter()
            .filter(|&(_, &have)| have >= len)
            .min_by_key(|&(&start, &have)| (have, start))
            .map(|(&start, &have)| (start, have));
        if let Some((start, have)) = best {
            self.free.remove(&start);
            if have > len {
                self.free.insert(start + len, have - len);
            }
            return start;
        }
        let start = self.top;
        self.top += len;
        start
    }

    /// Gives a range back, to be reused [`QUARANTINE_FRAMES`] after `frame`:
    /// the frame the render world is drawing may still read it, and a write
    /// for its next owner lands at the next queue submission.
    pub fn free(&mut self, start: u32, len: u32, frame: u64) {
        self.quarantine.push_back((frame, start, len));
    }

    /// Moves the ranges whose quarantine is over at `frame` to the free
    /// list.
    pub fn recycle(&mut self, frame: u64) {
        while let Some(&(freed, start, len)) = self.quarantine.front() {
            if freed + QUARANTINE_FRAMES > frame {
                break;
            }
            self.quarantine.pop_front();
            self.release(start, len);
        }
    }

    fn release(&mut self, mut start: u32, mut len: u32) {
        if let Some((&before, &before_len)) = self.free.range(..start).next_back()
            && before + before_len == start
        {
            self.free.remove(&before);
            start = before;
            len += before_len;
        }
        if let Some(after_len) = self.free.remove(&(start + len)) {
            len += after_len;
        }
        if start + len == self.top {
            self.top = start;
        } else {
            self.free.insert(start, len);
        }
    }

    /// Records below the highest range in use: what the buffer must hold.
    #[must_use]
    pub fn top(&self) -> u32 {
        self.top
    }

    /// Records in ranges handed out and not freed.
    #[must_use]
    pub fn live(&self) -> u32 {
        let free: u32 = self.free.values().sum();
        let held: u32 = self.quarantine.iter().map(|&(_, _, len)| len).sum();
        self.top - free - held
    }

    /// Free ranges, for the log: a growing count is fragmentation.
    #[must_use]
    pub fn holes(&self) -> usize {
        self.free.len()
    }
}

/// A cell's ranges and how many of its entities still draw them.
#[derive(Debug, Clone, Copy)]
struct Held {
    blades: u32,
    len: u32,
    refs: u32,
}

/// The shared blade and cell buffers, their ranges and the one material
/// that binds them.
#[derive(Default)]
pub struct GrassStore {
    blades: Option<Buffer>,
    blade_capacity: u32,
    cells: Option<Buffer>,
    cell_capacity: u32,
    blade_ranges: Ranges,
    cell_ranges: Ranges,
    /// By cell slot.
    held: Vec<Option<Held>>,
    frame: u64,
    material: Option<Handle<GrassMaterial>>,
    warned: bool,
}

impl GrassStore {
    /// Makes both buffers, once.
    pub fn build(&mut self, device: &RenderDevice) {
        if self.blades.is_some() {
            return;
        }
        self.blades = Some(create_buffer(
            device,
            "grass blades",
            INITIAL_BLADES,
            BLADE_BYTES,
        ));
        self.blade_capacity = INITIAL_BLADES;
        self.cells = Some(create_buffer(
            device,
            "grass cells",
            INITIAL_CELLS,
            CELL_BYTES,
        ));
        self.cell_capacity = INITIAL_CELLS;
    }

    /// Makes the material every cell draws with over the shared buffers,
    /// replacing any earlier one (a new world's species table). `None`
    /// before [`Self::build`].
    pub fn make_material(
        &mut self,
        base: StandardMaterial,
        params: GrassParams,
        wind: &Buffer,
        materials: &mut Assets<GrassMaterial>,
    ) -> Option<Handle<GrassMaterial>> {
        let handle = materials.add(GrassMaterial {
            base,
            extension: GrassField {
                params,
                blades: self.blades.clone()?,
                wind: wind.clone(),
                cells: self.cells.clone()?,
            },
        });
        self.material = Some(handle.clone());
        Some(handle)
    }

    /// The material every cell draws with, once made.
    #[must_use]
    pub fn material(&self) -> Option<&Handle<GrassMaterial>> {
        self.material.as_ref()
    }

    /// Starts a frame: the ranges whose quarantine is over become free.
    pub fn next_frame(&mut self) {
        self.frame += 1;
        self.blade_ranges.recycle(self.frame);
        self.cell_ranges.recycle(self.frame);
    }

    /// Installs one cell: a blade range and a cell slot, either buffer
    /// grown first when the range is past its end, and one `write_buffer`
    /// each. `refs` is how many entities will draw the cell; the ranges are
    /// given back when the last of them calls [`Self::release`]. Returns the
    /// slot, the entities' `MeshTag`, or `None` when the buffers would pass
    /// the device's largest storage binding.
    #[allow(
        clippy::too_many_arguments,
        reason = "the cell and the GPU resources it is written with"
    )]
    pub fn install(
        &mut self,
        blades: &[GrassBlade],
        origin_y: f32,
        spawn_time: f32,
        refs: u32,
        device: &RenderDevice,
        queue: &RenderQueue,
        materials: &mut Assets<GrassMaterial>,
    ) -> Option<u32> {
        let count = u32::try_from(blades.len()).ok()?;
        let len = count.div_ceil(BLADE_GRANULE).max(1) * BLADE_GRANULE;
        let start = self.blade_ranges.allocate(len);
        if !self.fit(Kind::Blades, start + len, device, queue, materials) {
            self.blade_ranges.free(start, len, self.frame);
            return None;
        }
        let slot = self.cell_ranges.allocate(1);
        if !self.fit(Kind::Cells, slot + 1, device, queue, materials) {
            self.cell_ranges.free(slot, 1, self.frame);
            self.blade_ranges.free(start, len, self.frame);
            return None;
        }
        let record = GrassCellRecord {
            start,
            count,
            origin_y,
            spawn_time,
        };
        queue.write_buffer(
            self.blades.as_ref()?,
            u64::from(start) * BLADE_BYTES,
            &blade_bytes(blades),
        );
        queue.write_buffer(
            self.cells.as_ref()?,
            u64::from(slot) * CELL_BYTES,
            &record.bytes(),
        );
        let index = slot as usize;
        if self.held.len() <= index {
            self.held.resize(index + 1, None);
        }
        self.held[index] = Some(Held {
            blades: start,
            len,
            refs,
        });
        Some(slot)
    }

    /// One of a cell's entities is gone; the last gives the cell's ranges
    /// back.
    pub fn release(&mut self, slot: u32) {
        let Some(entry) = self.held.get_mut(slot as usize) else {
            return;
        };
        let Some(held) = entry.as_mut() else {
            return;
        };
        held.refs = held.refs.saturating_sub(1);
        if held.refs > 0 {
            return;
        }
        let (blades, len) = (held.blades, held.len);
        *entry = None;
        self.blade_ranges.free(blades, len, self.frame);
        self.cell_ranges.free(slot, 1, self.frame);
    }

    /// Bytes of both buffers.
    #[must_use]
    pub fn buffer_bytes(&self) -> u64 {
        u64::from(self.blade_capacity) * BLADE_BYTES + u64::from(self.cell_capacity) * CELL_BYTES
    }

    /// Bytes of the blade ranges live cells own.
    #[must_use]
    pub fn live_bytes(&self) -> u64 {
        u64::from(self.blade_ranges.live()) * BLADE_BYTES
    }

    /// Free blade ranges below the top.
    #[must_use]
    pub fn holes(&self) -> usize {
        self.blade_ranges.holes()
    }

    /// Grows `kind`'s buffer until it holds `records`: doubles it, copies
    /// the records over on the GPU and points the material at the new one.
    /// The copy is submitted at once, after any write still queued for the
    /// old buffer, which the queue runs first; the frame the render world is
    /// drawing keeps the old buffer alive through its bind group, and the
    /// material's changed binding is prepared before the next frame draws.
    fn fit(
        &mut self,
        kind: Kind,
        records: u32,
        device: &RenderDevice,
        queue: &RenderQueue,
        materials: &mut Assets<GrassMaterial>,
    ) -> bool {
        let (buffer, capacity, bytes, label) = match kind {
            Kind::Blades => (
                &mut self.blades,
                &mut self.blade_capacity,
                BLADE_BYTES,
                "grass blades",
            ),
            Kind::Cells => (
                &mut self.cells,
                &mut self.cell_capacity,
                CELL_BYTES,
                "grass cells",
            ),
        };
        if records <= *capacity {
            return true;
        }
        let Some(old) = buffer.as_ref() else {
            return false;
        };
        let mut grown = (*capacity).max(1);
        while grown < records {
            grown = grown.saturating_mul(2);
        }
        let limit = device.limits().max_storage_buffer_binding_size;
        if u64::from(grown) * bytes > limit {
            if !self.warned {
                tracing::warn!(
                    records,
                    limit,
                    label,
                    "a grass buffer is at the device's largest binding; grass past it is not drawn"
                );
                self.warned = true;
            }
            return false;
        }
        let new = create_buffer(device, label, grown, bytes);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grass buffer growth"),
        });
        encoder.copy_buffer_to_buffer(old, 0, &new, 0, u64::from(*capacity) * bytes);
        queue.submit([encoder.finish()]);
        if let Some(handle) = self.material.as_ref()
            && let Some(mut material) = materials.get_mut(handle)
        {
            match kind {
                Kind::Blades => material.extension.blades = new.clone(),
                Kind::Cells => material.extension.cells = new.clone(),
            }
        }
        *buffer = Some(new);
        *capacity = grown;
        true
    }
}

/// Which of the store's buffers.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Blades,
    Cells,
}

fn create_buffer(device: &RenderDevice, label: &'static str, records: u32, bytes: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size: u64::from(records) * bytes,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_buffer_preserves_current_and_previous_origins_in_shader_order() {
        let wind = WindParams {
            direction: Vec2::new(1.0, 2.0),
            strength: 3.0,
            time: 4.0,
            prev_time: 5.0,
            origin_x: 6.0,
            origin_z: 7.0,
            gust_scale: 8.0,
            prev_origin_x: 9.0,
            prev_origin_z: 10.0,
            sky_visibility: 11.0,
            grass_rim: 12.0,
            shade_tint: [13.0, 14.0, 15.0],
            canopy_core: [16.0, 17.0, 18.0],
            canopy_lift: 19.0,
            canopy_rim: 20.0,
        };
        let bytes = wind_bytes(&wind);
        assert_eq!(bytes.len(), WIND_BYTES);
        let actual: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_le_bytes(*chunk))
            .collect();
        let expected: Vec<u32> = (1u8..=20).map(|v| f32::from(v).to_bits()).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn buckets_take_the_smallest_that_fits() {
        assert_eq!(GrassLod::Near.bucket_for(0), None);
        assert_eq!(GrassLod::Near.bucket_for(1), Some(0));
        assert_eq!(GrassLod::Near.bucket_for(2048), Some(0));
        assert_eq!(GrassLod::Near.bucket_for(2049), Some(1));
        assert_eq!(GrassLod::Near.bucket_for(1_000_000), None);
        assert_eq!(GrassLod::Near.bucket_for(32768), Some(4));
        assert_eq!(GrassLod::Near.bucket_for(32769), None);
        assert_eq!(
            BUCKETS[4] as usize,
            strata_registry::flora::MAX_GRASS_BLADES_PER_CELL
        );
        assert_eq!(GrassLod::Mid.bucket_for(131_073), None);
        assert_eq!(GrassLod::Mid.bucket_for(8192), Some(0));
        assert_eq!(GrassLod::Mid.bucket_for(8193), Some(1));
    }

    #[test]
    fn a_record_is_sixteen_bytes_in_field_order() {
        let b = GrassBlade {
            x: 1,
            z: 2,
            y_bits: 3.5f32.to_bits(),
            height_cm: 40,
            phase: 5,
            stiffness: 6,
            species: 7,
            lush: 8,
            shade: 9,
            nx: -1,
            nz: 1,
        };
        let bytes = blade_bytes(&[b]);
        assert_eq!(bytes.len(), 16);
        assert_eq!(&bytes[0..4], &[1, 0, 2, 0]);
        assert_eq!(
            f32::from_le_bytes(bytes[4..8].try_into().expect("four")).to_bits(),
            3.5f32.to_bits()
        );
        assert_eq!(&bytes[8..16], &[40, 5, 6, 7, 8, 9, 255, 1]);
    }

    #[test]
    fn a_cell_record_is_start_count_origin_and_time() {
        let record = GrassCellRecord {
            start: 4096,
            count: 1234,
            origin_y: 12.5,
            spawn_time: 3600.25,
        };
        let bytes = record.bytes();
        assert_eq!(u64::try_from(bytes.len()), Ok(CELL_BYTES));
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().expect("four")),
            4096
        );
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().expect("four")),
            1234
        );
        assert_eq!(
            f32::from_le_bytes(bytes[8..12].try_into().expect("four")).to_bits(),
            12.5f32.to_bits()
        );
        assert_eq!(
            f32::from_le_bytes(bytes[12..16].try_into().expect("four")).to_bits(),
            3600.25f32.to_bits()
        );
    }

    #[test]
    fn a_freed_range_waits_out_its_quarantine_then_the_best_fit_takes_it() {
        let mut ranges = Ranges::default();
        let a = ranges.allocate(512);
        let b = ranges.allocate(256);
        let c = ranges.allocate(1024);
        assert_eq!((a, b, c), (0, 512, 768));
        ranges.free(a, 512, 10);
        ranges.free(c, 1024, 10);
        ranges.recycle(12);
        assert_eq!(
            ranges.allocate(256),
            1792,
            "still in quarantine at frame 12"
        );
        ranges.recycle(13);
        assert_eq!(ranges.top(), 2048, "the freed range below the top stays");
        assert_eq!(
            ranges.allocate(1024),
            768,
            "the smallest free range that holds it"
        );
        assert_eq!(ranges.allocate(256), 0, "split from the front of a range");
        assert_eq!(ranges.allocate(256), 256);
        assert_eq!(ranges.holes(), 0);
        assert_eq!(ranges.live(), 2048);
    }

    #[test]
    fn freed_neighbours_merge_and_a_free_top_lowers_the_pointer() {
        let mut ranges = Ranges::default();
        let starts: Vec<u32> = (0..4).map(|_| ranges.allocate(256)).collect();
        assert_eq!(starts, vec![0, 256, 512, 768]);
        ranges.free(256, 256, 0);
        ranges.free(512, 256, 0);
        ranges.recycle(QUARANTINE_FRAMES);
        assert_eq!(ranges.holes(), 1, "two neighbours are one range");
        assert_eq!(ranges.allocate(512), 256, "the merged range holds both");
        ranges.free(256, 512, 5);
        ranges.free(768, 256, 5);
        ranges.recycle(5 + QUARANTINE_FRAMES);
        assert_eq!(ranges.top(), 256, "everything above the first is free");
        assert_eq!(ranges.holes(), 0);
        assert_eq!(ranges.live(), 256);
        assert_eq!(ranges.allocate(1024), 256);
    }

    #[test]
    fn the_near_mesh_has_seven_vertices_and_five_triangles_per_blade() {
        let mesh = blade_mesh(GrassLod::Near, 3);
        assert_eq!(mesh.count_vertices(), 21);
        assert_eq!(mesh.indices().map(Indices::len), Some(45));
        let mid = blade_mesh(GrassLod::Mid, 3);
        assert_eq!(mid.count_vertices(), 9);
    }
}
