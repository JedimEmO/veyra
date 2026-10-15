//! Far trees as records in one shared buffer, drawn as grass is drawn: a
//! shared mesh of quads that say only which corner of which slot they are,
//! and a vertex shader that reads its tree from a storage buffer.
//!
//! The Phase 8 capacity test drew an impostor as an entity per tree and was
//! bound by the CPU at 700 000 entities at forest range, so every tree of one
//! species in one foliage cell is one entity whatever the density. A buffer
//! per cell would be a binding per cell, a binding a material per cell and a
//! material a draw per cell (twelve thousand of them cost 60 ms of CPU in the
//! woodland fixture), and a mesh per cell, the way the batches first carried
//! their trees, was 150 to 240 new `Mesh` assets a second while the far ring
//! filled, the render thread's largest system on any moving pose
//! (`docs/perf/profile-2026-10-03.md`, 4.4). So all far trees live in one
//! growing storage buffer that every species' one material binds, a batch
//! owns a range of it whose start rides in its `MeshTag`, and its mesh is one
//! of a few shared quad meshes sized in powers of two: installing a cell is a
//! `write_buffer` of 32 bytes a slot and creates no asset. Batches of a
//! species share mesh slab and material and Bevy's GPU preprocessing merges
//! them into a few indirect draws. Each tree still chooses its own frames,
//! and fades in on its own distance with the dither Bevy fades its mesh level
//! out with, so the handover is exact per tree
//! (`assets/shaders/tree_impostor.wgsl`).

use std::collections::VecDeque;

use bevy::asset::{Asset, Assets, Handle, RenderAssetUsages};
use bevy::color::Color;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::prelude::*;
use bevy::ecs::world::DeferredWorld;
use bevy::image::Image;
use bevy::material::{AlphaMode, OpaqueRendererMethod};
use bevy::math::{Vec3, Vec4};
use bevy::mesh::{Indices, Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    StandardMaterial,
};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::shader::ShaderRef;

use ashlar_bevy::ashlar::impostor::ImpostorLayout;

/// Both passes and both stages, relative to the asset root.
pub const TREE_IMPOSTOR_SHADER_PATH: &str = "shaders/tree_impostor.wgsl";

/// The alpha an impostor is cut at: `ashlar-impostor` holds each frame's
/// coverage at a half down its mip chain.
pub const CUTOFF: f32 = 0.5;

/// Bytes of one tree in the shared buffer: the shader's `FarTreeRecord`.
pub const RECORD_BYTES: usize = 32;

/// Slots in the shared quad meshes, smallest first: a batch takes the
/// smallest that holds its trees and the slots past them read empty records
/// and collapse. Powers of two waste at most half a batch's slots, and the
/// last is a whole cell's plant candidates (`MAX_PLANT_CANDIDATES_PER_CELL`
/// in the registry), so no batch is ever split in practice.
pub const CLASSES: [u32; 13] = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Records the shared buffer starts with: about the far ring of a dense
/// forest at the default distance, two megabytes; it doubles when full.
pub const INITIAL_RECORDS: u32 = 1 << 16;

/// Frames a freed range waits before it is handed out again. With pipelined
/// rendering the frame the render world is drawing was extracted before the
/// main world freed the range, and a `write_buffer` lands at the next queue
/// submission, which may be that frame's: reusing the range at once could
/// show a new cell's trees where the old batch stood for a frame.
pub const QUARANTINE_FRAMES: u64 = 3;

/// The class index a batch of `trees` takes, or `None` for an empty batch
/// or one past the largest class.
#[must_use]
pub fn class_for(trees: usize) -> Option<usize> {
    if trees == 0 {
        return None;
    }
    CLASSES.iter().position(|&slots| slots as usize >= trees)
}

/// The uniform block the shader reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct ImpostorBatchUniform {
    /// Frames across, its reciprocal, texels across a frame, and one for a
    /// hemispherical grid.
    pub grid: Vec4,
    /// The bounding sphere's centre in the prop's frame, and its radius.
    pub sphere: Vec4,
    /// The alpha cutoff, the parallax strength, how much the trees' sky
    /// term darkens the sky's fill (`FoliageSky`, lighting plan phase 3a),
    /// and one unused.
    pub shading: Vec4,
    /// Metres from a tree's root: fade in from, fade in to, fade out from,
    /// fade out to.
    pub fade: Vec4,
}

impl ImpostorBatchUniform {
    /// The rows for a species' atlas, fading in over `fade_in` and out over
    /// `fade_out`.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "frame counts and sizes are small whole numbers"
    )]
    pub fn new(layout: &ImpostorLayout, fade_in: (f32, f32), fade_out: (f32, f32)) -> Self {
        let frames = layout.frames.max(2) as f32;
        Self {
            grid: Vec4::new(
                frames,
                frames.recip(),
                layout.frame_px.max(1) as f32,
                if layout.hemispherical { 1.0 } else { 0.0 },
            ),
            sphere: Vec3::from_array(layout.centre).extend(layout.radius),
            shading: Vec4::new(CUTOFF, 1.0, 1.0, 0.0),
            fade: Vec4::new(fade_in.0, fade_in.1, fade_out.0, fade_out.1),
        }
    }
}

/// A species' far picture as the tree assets describe it: its atlas, its
/// layout and its fades. It is never drawn itself: [`FarTrees`] is, made
/// from it with the shared tree buffer once the species is installed, so a
/// species' description does not change when the buffer grows.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct ImpostorBatch {
    /// The layout and the fades.
    #[uniform(100)]
    pub uniform: ImpostorBatchUniform,
    /// The albedo atlas: sRGB colour, straight alpha.
    #[texture(101)]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    /// The normal and depth atlas.
    #[texture(103)]
    pub normal_depth: Handle<Image>,
}

/// A species' far picture, the description [`FarTreeMaterial`] is made from.
pub type ImpostorBatchMaterial = ExtendedMaterial<StandardMaterial, ImpostorBatch>;

/// Never drawn, so no shader of its own; the alias needs the trait.
impl MaterialExtension for ImpostorBatch {}

/// The extension half of [`FarTreeMaterial`]: one species' atlas and the
/// shared buffer of every far tree, shared by all its batches.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct FarTrees {
    /// The layout, the fades and the sky term's strength.
    #[uniform(100)]
    pub uniform: ImpostorBatchUniform,
    /// The albedo atlas: sRGB colour, straight alpha.
    #[texture(101)]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    /// The normal and depth atlas.
    #[texture(103)]
    pub normal_depth: Handle<Image>,
    /// Every far tree of every species, [`RECORD_BYTES`] each; a batch reads
    /// the range its `MeshTag` starts.
    #[storage(104, read_only, buffer)]
    pub trees: Buffer,
}

/// Every far tree of one species.
pub type FarTreeMaterial = ExtendedMaterial<StandardMaterial, FarTrees>;

impl FarTrees {
    /// The drawn half of a species' description, over the shared buffer.
    #[must_use]
    pub fn from_description(description: &ImpostorBatch, trees: Buffer) -> Self {
        Self {
            uniform: description.uniform,
            albedo: description.albedo.clone(),
            normal_depth: description.normal_depth.clone(),
            trees,
        }
    }
}

impl MaterialExtension for FarTrees {
    fn vertex_shader() -> ShaderRef {
        TREE_IMPOSTOR_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TREE_IMPOSTOR_SHADER_PATH.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        TREE_IMPOSTOR_SHADER_PATH.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        TREE_IMPOSTOR_SHADER_PATH.into()
    }

    /// A billboard turned to the sun is another picture than the one the
    /// camera sees, and its shadow swims; far trees cast none, as the
    /// foliage spike settled.
    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_UV_0.at_shader_location(1)])?,
        ];
        Ok(())
    }
}

/// The base material of a species' batches: rough, dielectric, cut out,
/// unculled because the quad's winding is the billboard's, forward always,
/// and translucent as the leaves it pictures.
#[must_use]
pub fn base_material(transmission: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        reflectance: 0.3,
        alpha_mode: AlphaMode::Mask(CUTOFF),
        cull_mode: None,
        diffuse_transmission: transmission,
        opaque_render_method: OpaqueRendererMethod::Forward,
        ..Default::default()
    }
}

/// One far tree: its root in the cell's frame, its scale, its yaw in
/// radians and its tint hash.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarTree {
    /// Metres in the cell's frame.
    pub root: Vec3,
    /// The picture's scale: the plant's times its variant's over the one
    /// the atlas was baked from.
    pub scale: f32,
    /// Radians about the vertical, as the mesh levels turn.
    pub yaw: f32,
    /// The low sixteen bits of the plant's tint hash.
    pub tint: u32,
    /// The plant's share of the sky past its neighbours' crowns, in 255ths
    /// (`strata_flora::Plant::sky`).
    pub sky: u8,
}

/// `slots` records as the shader reads them, `trees` first and empty ones
/// after: `[root, scale]` and `[cos yaw, sin yaw, tint, 0]`, the tint hash
/// in the low sixteen bits of a whole number and what the crowns take of
/// the tree's sky ([`FarTree::sky`]) in the next eight, which a float holds
/// exactly up to 2^24. An empty record has no scale and collapses.
#[must_use]
pub fn record_bytes(trees: &[FarTree], slots: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(slots.max(trees.len()) * RECORD_BYTES);
    for tree in trees {
        let (sin, cos) = tree.yaw.sin_cos();
        // The shade is stored as what the crowns take, so a record without
        // one reads as open.
        let packed = (tree.tint & 0xffff) | (u32::from(u8::MAX - tree.sky) << 16);
        #[allow(clippy::cast_precision_loss, reason = "under 2^24, exact")]
        let record = [
            tree.root.x,
            tree.root.y,
            tree.root.z,
            tree.scale,
            cos,
            sin,
            packed as f32,
            0.0,
        ];
        for value in record {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.resize(slots.max(trees.len()) * RECORD_BYTES, 0);
    out
}

/// A shared quad mesh of `slots` trees: four corners a slot that carry only
/// their corner, the shader finding its slot from the vertex index and its
/// tree in the buffer. Positions are all zero; the entity carries the
/// bounds, because the mesh bounds nothing a tree reaches.
#[must_use]
pub fn quad_mesh(slots: u32) -> Mesh {
    let n = slots as usize;
    let mut uvs = Vec::with_capacity(n * 4);
    let mut indices = Vec::with_capacity(n * 6);
    for slot in 0..slots {
        uvs.extend([[0.0_f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let base = slot * 4;
        indices.extend([base, base + 3, base + 2, base, base + 2, base + 1]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32; 3]; n * 4])
    // Unread; present because Bevy's normal prepass warns without them.
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 1.0, 0.0]; n * 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Ranges of the shared buffer by class: a free list per class, a bump
/// pointer past the highest range handed out, and the freed ranges still
/// in quarantine. Ranges never merge or split: a far ring holds about the
/// same mix of batch sizes wherever it stands, so a freed range of a class
/// is soon wanted again by the same class.
#[derive(Debug, Default)]
pub struct SlotRanges {
    top: u32,
    free: [Vec<u32>; CLASSES.len()],
    quarantine: VecDeque<(u64, usize, u32)>,
}

impl SlotRanges {
    /// The start of a range of class `class`, reused when one is free.
    ///
    /// # Panics
    ///
    /// When `class` is past [`CLASSES`].
    pub fn allocate(&mut self, class: usize) -> u32 {
        if let Some(start) = self.free[class].pop() {
            return start;
        }
        let start = self.top;
        self.top += CLASSES[class];
        start
    }

    /// Gives a range back, to be reused [`QUARANTINE_FRAMES`] after `frame`.
    pub fn free(&mut self, class: usize, start: u32, frame: u64) {
        self.quarantine.push_back((frame, class, start));
    }

    /// Moves the ranges whose quarantine is over at `frame` to the free
    /// lists.
    pub fn recycle(&mut self, frame: u64) {
        while let Some(&(freed, class, start)) = self.quarantine.front() {
            if freed + QUARANTINE_FRAMES > frame {
                break;
            }
            self.quarantine.pop_front();
            self.free[class].push(start);
        }
    }

    /// Records below the highest range ever handed out: what the buffer
    /// must hold.
    #[must_use]
    pub fn top(&self) -> u32 {
        self.top
    }

    /// Slots in ranges handed out and not freed.
    #[must_use]
    pub fn live(&self) -> u32 {
        let free: u32 = self
            .free
            .iter()
            .enumerate()
            .map(|(class, starts)| CLASSES[class] * u32::try_from(starts.len()).unwrap_or(0))
            .sum();
        let held: u32 = self
            .quarantine
            .iter()
            .map(|&(_, class, _)| CLASSES[class])
            .sum();
        self.top - free - held
    }
}

/// The range of the shared buffer a far-tree batch owns, given back when
/// the batch is despawned.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(on_remove = release_slots)]
pub struct FarTreeSlots {
    /// The first record, also the batch's `MeshTag`.
    pub start: u32,
    /// The index into [`CLASSES`].
    pub class: u8,
}

fn release_slots(mut world: DeferredWorld, context: HookContext) {
    let Some(slots) = world.get::<FarTreeSlots>(context.entity).copied() else {
        return;
    };
    if let Some(mut store) = world.get_resource_mut::<FarTreeStore>() {
        let frame = store.frame;
        store
            .ranges
            .free(usize::from(slots.class), slots.start, frame);
    }
}

/// The shared far-tree buffer, its ranges, the quad meshes and every
/// species' drawn material.
#[derive(Resource, Default)]
pub struct FarTreeStore {
    /// The buffer, once a device made it.
    buffer: Option<Buffer>,
    /// Records it holds.
    capacity: u32,
    ranges: SlotRanges,
    frame: u64,
    /// One quad mesh per class.
    meshes: Vec<Handle<Mesh>>,
    /// By species id, for a species with far trees.
    materials: Vec<Option<Handle<FarTreeMaterial>>>,
    warned: bool,
}

impl FarTreeStore {
    /// Makes the buffer and the quad meshes, once.
    pub fn build(&mut self, device: &RenderDevice, meshes: &mut Assets<Mesh>) {
        if self.buffer.is_some() {
            return;
        }
        self.buffer = Some(create_buffer(device, INITIAL_RECORDS));
        self.capacity = INITIAL_RECORDS;
        self.meshes = CLASSES.iter().map(|&n| meshes.add(quad_mesh(n))).collect();
    }

    /// The buffer every species' material binds.
    #[must_use]
    pub fn buffer(&self) -> Option<&Buffer> {
        self.buffer.as_ref()
    }

    /// Starts a frame: the ranges whose quarantine is over become free.
    pub fn next_frame(&mut self) {
        self.frame += 1;
        self.ranges.recycle(self.frame);
    }

    /// Replaces every species' drawn material, `None` for a species with no
    /// far trees.
    pub fn set_materials(&mut self, materials: Vec<Option<Handle<FarTreeMaterial>>>) {
        self.materials = materials;
    }

    /// The species' drawn material, once installed.
    #[must_use]
    pub fn material(&self, species: u32) -> Option<&Handle<FarTreeMaterial>> {
        self.materials.get(species as usize)?.as_ref()
    }

    /// Every species' drawn material.
    pub fn materials(&self) -> impl Iterator<Item = &Handle<FarTreeMaterial>> {
        self.materials.iter().flatten()
    }

    /// Whether batches can be installed: the buffer, the meshes and at
    /// least one species' material exist.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.buffer.is_some() && self.materials.iter().any(Option::is_some)
    }

    /// Bytes of the buffer.
    #[must_use]
    pub fn buffer_bytes(&self) -> u64 {
        u64::from(self.capacity) * RECORD_BYTES as u64
    }

    /// Bytes of the records live batches own.
    #[must_use]
    pub fn live_bytes(&self) -> u64 {
        u64::from(self.ranges.live()) * RECORD_BYTES as u64
    }

    /// Installs one batch's records: a range of the smallest class that
    /// holds them, the buffer grown first when the range is past its end,
    /// and one `write_buffer`. Returns the range and the mesh to draw it
    /// with, or `None` when the batch is empty, larger than a class, or
    /// would pass the device's largest storage binding.
    pub fn install(
        &mut self,
        trees: &[FarTree],
        device: &RenderDevice,
        queue: &RenderQueue,
        materials: &mut Assets<FarTreeMaterial>,
    ) -> Option<(FarTreeSlots, Handle<Mesh>)> {
        let class = class_for(trees.len())?;
        let mesh = self.meshes.get(class)?.clone();
        let start = self.ranges.allocate(class);
        let end = start + CLASSES[class];
        if end > self.capacity && !self.grow(end, device, queue, materials) {
            self.ranges.free(class, start, self.frame);
            return None;
        }
        let buffer = self.buffer.as_ref()?;
        let bytes = record_bytes(trees, CLASSES[class] as usize);
        queue.write_buffer(buffer, u64::from(start) * RECORD_BYTES as u64, &bytes);
        Some((
            FarTreeSlots {
                start,
                class: u8::try_from(class).unwrap_or(u8::MAX),
            },
            mesh,
        ))
    }

    /// Doubles the buffer until it holds `records`, copies the records over
    /// on the GPU and points every species' material at the new one. The
    /// copy is submitted at once, after any write still queued for the old
    /// buffer, which the queue runs first; the frame the render world is
    /// drawing keeps the old buffer alive through its bind groups.
    fn grow(
        &mut self,
        records: u32,
        device: &RenderDevice,
        queue: &RenderQueue,
        materials: &mut Assets<FarTreeMaterial>,
    ) -> bool {
        let Some(old) = self.buffer.as_ref() else {
            return false;
        };
        let mut capacity = self.capacity.max(1);
        while capacity < records {
            capacity = capacity.saturating_mul(2);
        }
        let limit = device.limits().max_storage_buffer_binding_size;
        if u64::from(capacity) * RECORD_BYTES as u64 > limit {
            if !self.warned {
                tracing::warn!(
                    records,
                    limit,
                    "the far-tree buffer is at the device's largest binding; far trees past it are not drawn"
                );
                self.warned = true;
            }
            return false;
        }
        let new = create_buffer(device, capacity);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("far tree buffer growth"),
        });
        encoder.copy_buffer_to_buffer(
            old,
            0,
            &new,
            0,
            u64::from(self.capacity) * RECORD_BYTES as u64,
        );
        queue.submit([encoder.finish()]);
        for handle in self.materials.iter().flatten() {
            if let Some(mut material) = materials.get_mut(handle) {
                material.extension.trees = new.clone();
            }
        }
        self.buffer = Some(new);
        self.capacity = capacity;
        true
    }
}

fn create_buffer(device: &RenderDevice, records: u32) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("far trees"),
        size: u64::from(records) * RECORD_BYTES as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    }

    #[test]
    fn a_record_is_root_scale_then_yaw_and_the_packed_tint() {
        let tree = FarTree {
            root: Vec3::new(1.0, 2.0, 3.0),
            scale: 1.5,
            yaw: 0.5,
            tint: 0x1_2345,
            sky: 200,
        };
        let bytes = record_bytes(&[tree; 3], 4);
        assert_eq!(bytes.len(), 4 * RECORD_BYTES);
        let values = floats(&bytes);
        assert_eq!(&values[0..4], &[1.0, 2.0, 3.0, 1.5]);
        assert!((values[4] - 0.5_f32.cos()).abs() < 1e-6);
        assert!((values[5] - 0.5_f32.sin()).abs() < 1e-6);
        assert!(
            (values[6] - (9029.0 + 55.0 * 65536.0)).abs() < 1e-3,
            "the low sixteen bits, 0x2345, and the 55 the crowns take above them"
        );
        assert!(
            values[24..].iter().all(|&v| v == 0.0),
            "the slot past the trees is empty and collapses"
        );
    }

    #[test]
    fn a_quad_mesh_is_four_corners_and_two_triangles_a_slot() {
        let mesh = quad_mesh(3);
        assert_eq!(mesh.count_vertices(), 12);
        assert_eq!(mesh.indices().map(Indices::len), Some(18));
    }

    #[test]
    fn classes_take_the_smallest_that_fits() {
        assert_eq!(class_for(0), None);
        assert_eq!(class_for(1), Some(0));
        assert_eq!(class_for(3), Some(2));
        assert_eq!(class_for(4), Some(2));
        assert_eq!(class_for(4096), Some(12));
        assert_eq!(class_for(4097), None);
        assert_eq!(
            *CLASSES.last().expect("classes") as usize,
            strata_registry::flora::MAX_PLANT_CANDIDATES_PER_CELL,
            "the largest class holds a whole cell's candidates"
        );
    }

    #[test]
    fn a_freed_range_waits_out_its_quarantine_then_is_reused_by_its_class() {
        let mut ranges = SlotRanges::default();
        let a = ranges.allocate(3);
        let b = ranges.allocate(0);
        assert_eq!((a, b), (0, 8));
        assert_eq!(ranges.top(), 9);
        ranges.free(3, a, 10);
        ranges.recycle(12);
        assert_eq!(ranges.allocate(3), 9, "still in quarantine at frame 12");
        ranges.recycle(13);
        assert_eq!(ranges.allocate(3), 0, "free at frame 13");
        assert_eq!(
            ranges.allocate(2),
            17,
            "a range is only reused by its own class"
        );
        assert_eq!(ranges.live(), 8 + 1 + 8 + 4);
    }
}
