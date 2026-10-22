//! Trees on the client (ADR 0005, 2026-09-26 amendment): the baked props of
//! the tree library, drawn near as entities per mesh level and far as
//! per-cell impostor batches ([`crate::impostor`]).
//!
//! A tree species' variants are `.ashprop` files the content step grew and
//! baked, loaded from the `generated://` source with the material library
//! beside them. Every mesh level of a variant is its bark and its leaves, each
//! drawn with a [`TreeMaterial`]: the ashlar material its key names, and the
//! variant's pivot table, which the wind shader walks from a vertex's own
//! branch out to the trunk.
//!
//! The understory (flowers, ferns, bushes and fallen logs) is still grown
//! here from its registry parameters until Phase 10 bakes it too. It is
//! converted into the same vertex contract with a one-pivot table, so one
//! material and one shader draw both. Only the nearest level casts shadows:
//! the foliage spike measured every level casting at 17 ms and the near one
//! alone at 2 ms.

use std::collections::BTreeMap;

use ashlar_bevy::ashlar::impostor::ImpostorLayout;
use ashlar_bevy::baked::MaterialLibraryAsset;
use ashlar_bevy::prelude::PropAsset;
use ashlar_bevy::prop::prop_level_range;
use bevy::asset::{Asset, AssetServer, Assets, Handle, RenderAssetUsages};
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::color::{Color, LinearRgba};
use bevy::ecs::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::material::AlphaMode;
use bevy::math::{Quat, Vec3};
use bevy::mesh::{
    Indices, Mesh, Mesh3d, MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef,
    PrimitiveTopology,
};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MeshMaterial3d, StandardMaterial,
};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Buffer, BufferInitDescriptor, BufferUsages, RenderPipelineDescriptor,
    SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::renderer::RenderDevice;
use bevy::shader::ShaderRef;
use bevy::transform::components::Transform;
use strata_flora::tree::{Streams, build_understory};
use strata_flora::{FloraTable, Plant, TreeLibrary};
use strata_registry::{FloraId, FloraKind, GameRegistries};
use tracing::warn;

use crate::impostor::{
    FarTree, ImpostorBatch, ImpostorBatchMaterial, ImpostorBatchUniform, base_material,
};

/// The main-pass shader, relative to the asset root.
pub const TREE_SHADER_PATH: &str = "shaders/foliage.wgsl";
/// The prepass and shadow vertex shader.
pub const TREE_PREPASS_SHADER_PATH: &str = "shaders/foliage_prepass.wgsl";
/// The vertex contract and the bend, shared by both.
pub const TREE_COMMON_SHADER_PATH: &str = "shaders/foliage_common.wgsl";
/// The crown grouping's rim, shared by the near and far tree shaders.
pub const CANOPY_SHADER_PATH: &str = "shaders/canopy.wgsl";

/// The asset source the content step's output is mounted as.
pub const GENERATED_SOURCE: &str = "generated";
/// The material library the content step writes, in that source.
pub const MATERIAL_LIBRARY: &str = "materials/library.materials.ron";

/// The share of a level switch's distance its crossfade spans, as
/// `AshlarPlugin` crossfades a building's levels.
pub const CROSSFADE: f32 = 0.1;

/// Floats per pivot in a variant's table: `[x, y, z, parent] [axis, length]
/// [base_radius, frequency_hz, stiffness, 0]`.
pub const PIVOT_FLOATS: usize = 12;

/// `[pivot, level, flutter, phase]`, the generator's `"wind"` vertex data.
#[must_use]
pub fn wind_attribute() -> MeshVertexAttribute {
    ashlar_bevy::attribute("wind", VertexFormat::Float32x4)
}

/// `[occlusion, height, curvature, crown depth]`, the generator's `"shade"`
/// vertex data.
#[must_use]
pub fn shade_attribute() -> MeshVertexAttribute {
    ashlar_bevy::attribute("shade", VertexFormat::Float32x4)
}

/// The extension half of [`TreeMaterial`]: the shared wind and the one
/// pivot atlas, so a material is no longer tied to a variant and every tree
/// of a bark or leaf set batches together.
///
/// Bindless where the adapter has binding arrays: the materials of a slab then
/// share one bind group and Bevy batches draws across them whatever their
/// bark and leaf maps, which is what collapses the batch sets. Without them
/// (the browser) Bevy binds each material plainly and the shader reads the
/// same two buffers at the plain bindings. The slab is held to 128 materials,
/// since each of its binding arrays is that long and a species' bark and
/// leaves come to a few dozen; the numbers start at 50 and the table is at
/// 100, clear of `StandardMaterial`'s own (0 to 30, and its arrays at 1 to 10).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bindless(limit(128), index_table(range(50..52), binding(100)))]
pub struct TreeWind {
    /// The wind, one `WindParams`, shared with the grass.
    #[storage(50, read_only, buffer, binding_array(101))]
    pub wind: Buffer,
    /// Every variant's pivot table, [`PIVOT_FLOATS`] a pivot, after
    /// [`PIVOT_SLOTS`] header rows that say where each table starts; the
    /// mesh tag's third byte names a variant's slot.
    #[storage(51, read_only, buffer, binding_array(102))]
    pub pivots: Buffer,
}

impl MaterialExtension for TreeWind {
    fn vertex_shader() -> ShaderRef {
        TREE_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TREE_SHADER_PATH.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        TREE_PREPASS_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let mut attributes = vec![
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_TANGENT.at_shader_location(4),
            wind_attribute().at_shader_location(8),
            shade_attribute().at_shader_location(9),
        ];
        // Rocks tint their tops with moss and flowers colour their petals
        // through vertex colour. Bevy's mesh pipeline already defines
        // `VERTEX_COLORS` for such a mesh; the attribute only has to be bound.
        if layout.0.contains(Mesh::ATTRIBUTE_COLOR) {
            attributes.push(Mesh::ATTRIBUTE_COLOR.at_shader_location(5));
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&attributes)?];
        Ok(())
    }
}

/// Bark and leaves alike, over the standard material.
pub type TreeMaterial = ExtendedMaterial<StandardMaterial, TreeWind>;

/// One piece of a mesh level: its mesh, its material and the mesh's bounds.
#[derive(Clone, Default)]
pub struct TreePiece {
    /// The mesh.
    pub mesh: Handle<Mesh>,
    /// Bark or leaves.
    pub material: Handle<TreeMaterial>,
    /// The mesh's bounds, measured once when the assets are installed: the
    /// prop meshes keep their vertices in the main world, so Bevy would
    /// otherwise walk every vertex again for each level the streamer makes.
    /// `None` when the mesh was not there to measure; Bevy then does.
    pub aabb: Option<Aabb>,
}

/// One mesh level of one variant.
#[derive(Clone)]
pub struct TreeLevel {
    /// Bark, then leaves.
    pub pieces: Vec<TreePiece>,
    /// The distances the level draws over.
    pub range: VisibilityRange,
    /// Whether it casts shadows.
    pub shadows: bool,
}

/// One variant as the client draws it.
#[derive(Clone)]
pub struct TreeVariantAssets {
    /// Mesh levels, finest first.
    pub levels: Vec<TreeLevel>,
    /// This variant's height over the one the species' impostor was baked
    /// from, so the one picture stands as tall as each variant.
    pub impostor_scale: f32,
    /// Where the variant's pivot table is in the [`PivotAtlas`], the third
    /// byte of its pieces' mesh tag.
    pub pivot_slot: u8,
}

/// A species' far picture: the one material all its batches share.
#[derive(Debug, Clone)]
pub struct SpeciesImpostor {
    /// The atlas, its layout and the fades.
    pub material: Handle<ImpostorBatchMaterial>,
    /// Where its trees start fading in, metres from their roots.
    pub fade_in: f32,
    /// The picture's bounding radius at scale one.
    pub radius: f32,
}

/// One species as the client draws it.
#[derive(Clone)]
pub struct TreeSpecies {
    /// The understory draws one level and no impostor.
    pub understory: bool,
    /// By variant.
    pub variants: Vec<TreeVariantAssets>,
    /// Far trees, for a baked species.
    pub impostor: Option<SpeciesImpostor>,
}

/// Every species with a trunk or a bush, by species id; grass has `None`.
#[derive(Resource, Clone, Default)]
pub struct TreeAssets {
    /// Indexed by `FloraId`.
    pub species: Vec<Option<TreeSpecies>>,
}

impl TreeAssets {
    /// The species, or `None` for grass or an unknown id.
    #[must_use]
    pub fn get(&self, id: FloraId) -> Option<&TreeSpecies> {
        self.species.get(id.0 as usize).and_then(Option::as_ref)
    }

    /// Whether any species draws far trees.
    #[must_use]
    pub fn has_impostors(&self) -> bool {
        self.species.iter().flatten().any(|s| s.impostor.is_some())
    }

    /// Where each mesh level of `plant` is drawn at all, from where it
    /// starts fading in to where it has faded out, in metres from its root:
    /// what Bevy's `VisibilityRange` test reads, and so what the streamer
    /// keeps a level alive for. `None` for a level the plant does not have
    /// or that has nothing to draw, as the understory's empty first slot.
    #[must_use]
    pub fn level_bands(&self, plant: &Plant) -> [Option<(f32, f32)>; MESH_LEVELS] {
        let mut bands = [None; MESH_LEVELS];
        let Some(variant) = self.get(plant.species).and_then(|species| {
            species
                .variants
                .get(usize::from(plant.variant) % species.variants.len().max(1))
        }) else {
            return bands;
        };
        for (band, level) in bands.iter_mut().zip(&variant.levels) {
            if !level.pieces.is_empty() {
                *band = Some((level.range.start_margin.start, level.range.end_margin.end));
            }
        }
        bands
    }

    /// The farthest any plant's mesh level is drawn, metres: past it, with
    /// the streamer's lead, a cell's plants hold no level and need no look.
    #[must_use]
    pub fn mesh_reach(&self) -> f32 {
        self.species
            .iter()
            .flatten()
            .flat_map(|species| &species.variants)
            .flat_map(|variant| variant.levels.iter().take(MESH_LEVELS))
            .filter(|level| !level.pieces.is_empty())
            .map(|level| level.range.end_margin.end)
            .fold(0.0, f32::max)
    }
}

/// The mesh levels a plant can hold; a variant's levels past these are
/// never drawn, as the far trees take over where the last one ends.
pub const MESH_LEVELS: usize = 3;

/// One mesh of one placed tree.
#[derive(Component, Debug, Clone, Copy)]
pub struct TreeInstance {
    /// Stable procedural identity shared by every level of this plant.
    pub id: strata_flora::PlantId,
    /// The species.
    pub species: FloraId,
    /// The level it draws.
    pub lod: usize,
}

/// One cell's far trees of one species.
#[derive(Component, Debug, Clone, Copy)]
pub struct TreeImpostorBatch {
    /// The species.
    pub species: FloraId,
    /// Trees in the batch.
    pub trees: u32,
}

/// The distances the cell streamer prepares tree levels for: where each
/// baked mesh level ends, and where far trees fade out. The props carry
/// their own bands; these say which levels a cell can need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeFades {
    /// Where the nearest level ends.
    pub near: f32,
    /// Where the middle level ends.
    pub mid: f32,
    /// Where the last mesh level ends and impostors have taken over.
    pub mesh: f32,
    /// Far trees fade out over this.
    pub far: (f32, f32),
}

impl Default for TreeFades {
    /// The generator's bands (`veyra_flora`: 60, 150 and 350 m) with their
    /// crossfades, and far trees to 1.6 km.
    fn default() -> Self {
        Self {
            near: 60.0 * (1.0 + CROSSFADE),
            mid: 150.0 * (1.0 + CROSSFADE),
            mesh: 350.0 * (1.0 + CROSSFADE),
            far: (1200.0, 1600.0),
        }
    }
}

/// The level slot the understory's one level is spawned in. The streamer
/// keeps each plant's levels by their own bands, so the slot no longer
/// decides where a bush is made; the first slot is left empty and is never
/// spawned.
pub const UNDERSTORY_LEVEL: usize = 1;

/// Understory levels end here: small plants fade out long before a tree's
/// last mesh level would.
fn understory_range(form: &str) -> VisibilityRange {
    let (start, end) = match form {
        "flower" | "fern" => (90.0, 120.0),
        _ => (150.0, 180.0),
    };
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: start..end,
        use_aabb: false,
    }
}

/// How many pivot tables the atlas can name: the slot is a byte of the mesh
/// tag, the one the shader never read for trees (the spawn time's).
pub const PIVOT_SLOTS: usize = 256;

/// Every variant's pivot table in one buffer, so the pivots are no longer a
/// per-variant material binding (review 2026-10-16, F3). The first
/// [`PIVOT_SLOTS`] rows (a `vec4` each) hold, in their `x` as `u32` bits, the
/// row where that slot's table starts; the tables follow, three rows a pivot.
#[derive(Default)]
pub struct PivotAtlas {
    rows: Vec<f32>,
    slots: Vec<u32>,
    seen: BTreeMap<Vec<u32>, u8>,
    overflowed: bool,
}

impl PivotAtlas {
    /// The slot of `table`, added unless an identical one is already there.
    /// Past [`PIVOT_SLOTS`] distinct tables the last slot is shared and a
    /// warning is logged once.
    pub fn add(&mut self, table: &[f32]) -> u8 {
        let key: Vec<u32> = table.iter().map(|v| v.to_bits()).collect();
        if let Some(&slot) = self.seen.get(&key) {
            return slot;
        }
        let Ok(slot) = u8::try_from(self.slots.len()) else {
            if !self.overflowed {
                warn!("more than {PIVOT_SLOTS} distinct pivot tables; later plants share the last");
                self.overflowed = true;
            }
            return u8::MAX;
        };
        // Row units: header first, then this table; a table is whole rows.
        let start = PIVOT_SLOTS + self.rows.len() / 4;
        self.slots.push(u32::try_from(start).unwrap_or(0));
        self.rows.extend_from_slice(table);
        while !self.rows.len().is_multiple_of(4) {
            self.rows.push(0.0);
        }
        self.seen.insert(key, slot);
        slot
    }

    /// The buffer's bytes: the header, then the tables.
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((PIVOT_SLOTS * 4 + self.rows.len()) * 4);
        for slot in 0..PIVOT_SLOTS {
            // A whole-number float, not the row's bits: a small integer's bits
            // are a denormal, which a driver may flush to zero on load, and a
            // float is exact to 2^24 rows.
            #[allow(clippy::cast_precision_loss, reason = "rows stay far under 2^24")]
            let start = self.slots.get(slot).copied().unwrap_or(0) as f32;
            bytes.extend_from_slice(&start.to_le_bytes());
            bytes.extend_from_slice(&[0; 12]);
        }
        bytes.extend(self.rows.iter().flat_map(|v| v.to_le_bytes()));
        bytes
    }

    /// The buffer the materials bind.
    #[must_use]
    pub fn buffer(&self, device: &RenderDevice) -> Buffer {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("tree pivots"),
            contents: &self.bytes(),
            usage: BufferUsages::STORAGE,
        })
    }
}

/// An understory mesh in the tree contract: the generator's streams with
/// its two old wind attributes turned into a `"wind"` that names the one
/// pivot and a `"shade"` that darkens towards the root.
#[must_use]
pub fn understory_mesh(streams: &Streams) -> Mesh {
    let wind: Vec<[f32; 4]> = streams
        .foliage_b
        .iter()
        .map(|b| [0.0, 1.0, b[2], b[0]])
        .collect();
    let shade: Vec<[f32; 4]> = streams
        .foliage_a
        .iter()
        .map(|a| {
            [
                0.6 + 0.4 * a[2].clamp(0.0, 1.0),
                a[2].clamp(0.0, 1.0),
                0.0,
                0.0,
            ]
        })
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, streams.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, streams.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, streams.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, streams.tangents.clone())
    .with_inserted_attribute(wind_attribute(), wind)
    .with_inserted_attribute(shade_attribute(), shade)
    .with_inserted_indices(Indices::U32(streams.indices.clone()))
}

/// The one pivot an understory plant bends about: its root, up, its
/// height long. Deadwood (the old stream's `w` of -1) is too stiff to move.
#[must_use]
pub fn understory_pivots(streams: &Streams, height: f32) -> Vec<f32> {
    let a = streams.foliage_a.first().copied().unwrap_or_default();
    let b = streams.foliage_b.first().copied().unwrap_or_default();
    let stiffness = if b[3] < -0.5 { 1.0e4 } else { 0.3 + 3.0 * b[1] };
    vec![
        a[0],
        0.0,
        a[1],
        -1.0,
        0.0,
        1.0,
        0.0,
        height.max(0.1),
        0.02,
        1.1,
        stiffness,
        0.0,
    ]
}

/// What the tree assets are made from, gathered while the props load.
#[derive(Resource)]
pub struct TreeLoading {
    library: Handle<MaterialLibraryAsset>,
    /// Baked species: the registry id, its variants' props and heights.
    baked: Vec<(FloraId, Vec<(Handle<PropAsset>, f32)>)>,
    /// Understory species, grown here.
    understory: Vec<(FloraId, UnderstorySpecies)>,
    /// Emission and tints per species, from the registry.
    looks: BTreeMap<u32, Look>,
    warned: bool,
}

struct UnderstorySpecies {
    /// The registry id, which keys the species' materials.
    id: u32,
    form: String,
    bark: String,
    /// Per variant: bark and leaf streams of level 0, and the height.
    variants: Vec<(Streams, Streams, f32)>,
}

#[derive(Clone, Copy)]
struct Look {
    lush: [f32; 3],
    emissive: [f32; 3],
}

impl TreeLoading {
    /// Start loading every baked species the library holds and grow the
    /// understory. A species the library does not hold and that is a tree
    /// is not drawn: its trunk still collides, which a missing content step
    /// makes visible rather than hidden.
    #[must_use]
    pub fn start(
        server: &AssetServer,
        registries: &GameRegistries,
        table: &FloraTable,
        library: Option<&TreeLibrary>,
    ) -> Self {
        let mut baked = Vec::new();
        let mut understory = Vec::new();
        let mut looks = BTreeMap::new();
        for species in table.species() {
            if species.kind == FloraKind::Grass {
                continue;
            }
            let Some(def) = registries.flora.get(species.id) else {
                continue;
            };
            looks.insert(
                species.id.0,
                Look {
                    lush: def.tint_lush.0,
                    emissive: def.emissive.0,
                },
            );
            let entry = library.and_then(|l| l.species.get(def.id.as_ref()));
            if let Some(entry) = entry.filter(|e| !e.variants.is_empty()) {
                let props = entry
                    .variants
                    .iter()
                    .map(|v| {
                        (
                            server.load(format!("{GENERATED_SOURCE}://{}", v.prop)),
                            v.height,
                        )
                    })
                    .collect();
                baked.push((species.id, props));
            } else if !species.tree.is_absent() && species.tree.form != "tree" {
                let variants = (0..species.variants)
                    .filter_map(|variant| {
                        let grown = build_understory(&species.tree, species.species_seed, variant)?;
                        let level = grown.lods.into_iter().next();
                        let (bark, leaves) = level.map(|l| (l.trunk, l.leaves)).unwrap_or_default();
                        Some((bark, leaves, grown.height))
                    })
                    .collect();
                understory.push((
                    species.id,
                    UnderstorySpecies {
                        id: species.id.0,
                        form: species.tree.form.clone(),
                        bark: species.tree.bark.clone(),
                        variants,
                    },
                ));
            }
        }
        Self {
            library: server.load(format!("{GENERATED_SOURCE}://{MATERIAL_LIBRARY}")),
            baked,
            understory,
            looks,
            warned: false,
        }
    }

    /// Every variant's slot in `atlas`, baked species then understory; a
    /// baked species with a prop that failed has `None`.
    fn pivot_slots(
        &self,
        props: &Assets<PropAsset>,
        atlas: &mut PivotAtlas,
    ) -> (Vec<Option<Vec<u8>>>, Vec<Vec<u8>>) {
        let baked = self
            .baked
            .iter()
            .map(|(_, variants)| {
                variants
                    .iter()
                    .map(|(handle, height)| {
                        let asset = props.get(handle)?;
                        Some(atlas.add(&baked_pivots(&asset.prop, *height)))
                    })
                    .collect()
            })
            .collect();
        let understory = self
            .understory
            .iter()
            .map(|(_, species)| {
                species
                    .variants
                    .iter()
                    .map(|(bark, leaves, height)| {
                        let reference = if bark.is_empty() { leaves } else { bark };
                        atlas.add(&understory_pivots(reference, *height))
                    })
                    .collect()
            })
            .collect();
        (baked, understory)
    }

    /// The assets, once every prop and the material library have loaded or
    /// failed; `None` while any is still on its way.
    #[allow(clippy::too_many_arguments, reason = "the render assets it creates")]
    pub fn try_install(
        &mut self,
        species_count: usize,
        wind: &Buffer,
        device: &RenderDevice,
        server: &AssetServer,
        props: &Assets<PropAsset>,
        libraries: &Assets<MaterialLibraryAsset>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<TreeMaterial>,
        batches: &mut Assets<ImpostorBatchMaterial>,
        fades: &TreeFades,
    ) -> Option<TreeAssets> {
        let settled = |id| {
            server.is_loaded_with_dependencies(id)
                || server
                    .get_recursive_dependency_load_state(id)
                    .is_some_and(|s| s.is_failed())
        };
        let waiting = !settled(self.library.id().untyped())
            || self
                .baked
                .iter()
                .flat_map(|(_, v)| v)
                .any(|(prop, _)| !settled(prop.id().untyped()));
        if waiting {
            return None;
        }
        let library = libraries.get(&self.library);
        if library.is_none() && !self.warned {
            warn!("the material library did not load; trees and bushes are drawn plain");
            self.warned = true;
        }
        let mut dressed = Dresser {
            library,
            server,
            cache: BTreeMap::new(),
        };
        let mut installed = TreeAssets {
            species: vec![None; species_count],
        };
        // Every table first: the one buffer has to exist before any material
        // holds it, and a species whose prop failed adds none.
        let mut atlas = PivotAtlas::default();
        let (baked_slots, understory_slots) = self.pivot_slots(props, &mut atlas);
        let pivots = atlas.buffer(device);
        let mut cache = MaterialCache::new();
        for ((id, variants), slots) in self.baked.iter().zip(baked_slots) {
            let look = self.looks.get(&id.0).copied();
            let Some(slots) = slots else {
                warn!(
                    species = id.0,
                    "a baked tree species did not load; it is not drawn"
                );
                continue;
            };
            if let Some(species) = baked_species(
                variants,
                &slots,
                look,
                &Buffers {
                    wind,
                    pivots: &pivots,
                },
                props,
                meshes,
                &mut dressed,
                materials,
                &mut cache,
                batches,
                fades,
            ) {
                installed.species[id.0 as usize] = Some(species);
            } else {
                warn!(
                    species = id.0,
                    "a baked tree species did not load; it is not drawn"
                );
            }
        }
        for ((id, species), slots) in self.understory.iter().zip(&understory_slots) {
            let look = self.looks.get(&id.0).copied();
            installed.species[id.0 as usize] = Some(understory_species(
                species,
                slots,
                look,
                &Buffers {
                    wind,
                    pivots: &pivots,
                },
                meshes,
                &mut dressed,
                materials,
                &mut cache,
            ));
        }
        Some(installed)
    }
}

/// The buffers every tree material binds: the same two for all of them.
struct Buffers<'a> {
    wind: &'a Buffer,
    pivots: &'a Buffer,
}

impl Buffers<'_> {
    fn extension(&self) -> TreeWind {
        TreeWind {
            wind: self.wind.clone(),
            pivots: self.pivots.clone(),
        }
    }
}

/// Materials made so far, by the library key and, for leaves, the species'
/// look: with the pivots in one buffer a material no longer belongs to a
/// variant, so equal ones are one asset and one batch set.
type MaterialCache = BTreeMap<(String, [u32; 6]), Handle<TreeMaterial>>;

fn look_bits(look: Option<Look>) -> [u32; 6] {
    look.map_or([0; 6], |l| {
        [
            l.lush[0].to_bits(),
            l.lush[1].to_bits(),
            l.lush[2].to_bits(),
            l.emissive[0].to_bits(),
            l.emissive[1].to_bits(),
            l.emissive[2].to_bits(),
        ]
    })
}

/// A baked variant's pivot table, or the one pivot a table-less prop bends
/// about.
fn baked_pivots(prop: &ashlar_bevy::ashlar::prop::Prop, height: f32) -> Vec<f32> {
    prop.tables.get("pivots").map_or_else(
        || {
            vec![
                0.0,
                0.0,
                0.0,
                -1.0,
                0.0,
                1.0,
                0.0,
                height.max(1.0),
                0.1,
                0.5,
                1.0e4,
                0.0,
            ]
        },
        Clone::clone,
    )
}

/// Standard materials from the library's keys, each created once.
struct Dresser<'a> {
    library: Option<&'a MaterialLibraryAsset>,
    server: &'a AssetServer,
    cache: BTreeMap<String, StandardMaterial>,
}

impl Dresser<'_> {
    fn get(&mut self, key: &str) -> Option<StandardMaterial> {
        if let Some(material) = self.cache.get(key) {
            return Some(material.clone());
        }
        let definition = self.library?.0.materials.get(key)?;
        let material = ashlar_bevy::create_material(definition, self.server);
        self.cache.insert(key.to_owned(), material.clone());
        Some(material)
    }
}

/// A plain stand-in for a key the library does not have: bark brown, or
/// leaves in the species' own green, cut out and translucent.
fn plain(leaves: bool, look: Option<Look>) -> StandardMaterial {
    if leaves {
        let lush = look.map_or([0.1, 0.25, 0.05], |l| l.lush);
        StandardMaterial {
            base_color: Color::linear_rgb(lush[0], lush[1], lush[2]),
            perceptual_roughness: 0.8,
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Mask(0.5),
            diffuse_transmission: 0.4,
            ..Default::default()
        }
    } else {
        StandardMaterial {
            base_color: Color::linear_rgb(0.13, 0.09, 0.06),
            perceptual_roughness: 0.95,
            ..Default::default()
        }
    }
}

fn with_emissive(mut material: StandardMaterial, look: Option<Look>) -> StandardMaterial {
    if let Some(look) = look
        && look.emissive.iter().any(|&c| c > 0.0)
    {
        material.emissive =
            LinearRgba::new(look.emissive[0], look.emissive[1], look.emissive[2], 1.0);
    }
    material
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one species' render assets: variants, levels, materials and its picture"
)]
fn baked_species(
    variants: &[(Handle<PropAsset>, f32)],
    slots: &[u8],
    look: Option<Look>,
    buffers: &Buffers<'_>,
    props: &Assets<PropAsset>,
    meshes: &Assets<Mesh>,
    dressed: &mut Dresser<'_>,
    materials: &mut Assets<TreeMaterial>,
    cache: &mut MaterialCache,
    batches: &mut Assets<ImpostorBatchMaterial>,
    fades: &TreeFades,
) -> Option<TreeSpecies> {
    let first_height = variants.first()?.1.max(0.01);
    let mut out = Vec::with_capacity(variants.len());
    let mut impostor = None;
    for ((handle, height), &pivot_slot) in variants.iter().zip(slots) {
        let asset = props.get(handle)?;
        let prop = &asset.prop;
        let mut levels = Vec::new();
        for (index, level) in prop.levels.iter().enumerate() {
            if let Some(atlas) = &level.impostor {
                if impostor.is_none() {
                    impostor = Some((atlas.clone(), index));
                }
                continue;
            }
            let range = prop_level_range(prop, index, CROSSFADE).unwrap_or(VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: fades.mesh..fades.mesh,
                use_aabb: false,
            });
            let pieces = level
                .pieces
                .iter()
                .zip(asset.meshes.get(index)?)
                .map(|(piece, mesh)| {
                    let leaves = piece.material.contains("leaves");
                    // Only a leaf set reads the species' look, so bark and
                    // the rest are shared by every species that names them.
                    let key = (
                        piece.material.clone(),
                        if leaves { look_bits(look) } else { [0; 6] },
                    );
                    let material = cache
                        .entry(key)
                        .or_insert_with(|| {
                            let base = dressed
                                .get(&piece.material)
                                .unwrap_or_else(|| plain(leaves, look));
                            // The baked maps are already at the value budget
                            // (tools/content remaps the graphs), so the near
                            // levels and the impostors share one colour with
                            // no multiply here.
                            materials.add(TreeMaterial {
                                base: if leaves {
                                    with_emissive(base, look)
                                } else {
                                    base
                                },
                                extension: buffers.extension(),
                            })
                        })
                        .clone();
                    TreePiece {
                        mesh: mesh.clone(),
                        material,
                        aabb: meshes.get(mesh).and_then(MeshAabb::compute_aabb),
                    }
                })
                .collect();
            levels.push(TreeLevel {
                pieces,
                range,
                shadows: level.cast_shadows,
            });
        }
        out.push(TreeVariantAssets {
            levels,
            impostor_scale: height / first_height,
            pivot_slot,
        });
    }
    // The picture was baked from the first variant; it takes over where
    // that variant's last mesh level fades out.
    let impostor = impostor.and_then(|(atlas, index)| {
        let first = props.get(&variants.first()?.0)?;
        let handover = index
            .checked_sub(1)
            .and_then(|last| prop_level_range(&first.prop, last, CROSSFADE))
            .map_or((fades.mesh, fades.mesh), |range| {
                (range.end_margin.start, range.end_margin.end)
            });
        let layout: &ImpostorLayout = &atlas.layout;
        let (albedo, normal_depth) = ashlar_bevy::impostor::load_impostor_images(
            dressed.server,
            atlas.albedo,
            atlas.normal_depth,
        );
        Some(SpeciesImpostor {
            material: batches.add(ImpostorBatchMaterial {
                base: with_emissive(base_material(0.4), look),
                extension: ImpostorBatch {
                    uniform: ImpostorBatchUniform::new(layout, handover, fades.far),
                    albedo,
                    normal_depth,
                },
            }),
            fade_in: handover.0,
            radius: layout.radius,
        })
    });
    Some(TreeSpecies {
        understory: false,
        variants: out,
        impostor,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "one species' render assets and the shared caches"
)]
fn understory_species(
    species: &UnderstorySpecies,
    slots: &[u8],
    look: Option<Look>,
    buffers: &Buffers<'_>,
    meshes: &mut Assets<Mesh>,
    dressed: &mut Dresser<'_>,
    materials: &mut Assets<TreeMaterial>,
    cache: &mut MaterialCache,
) -> TreeSpecies {
    let small = matches!(species.form.as_str(), "flower" | "fern");
    let lush = look.map_or([0.1, 0.25, 0.05], |l| l.lush);
    // Stems of flowers and ferns are green; a log wears its bark set, and a
    // bush the shrub leaves, from the baked library.
    let bark_base = if small {
        StandardMaterial {
            base_color: Color::linear_rgb(0.08, 0.22, 0.035),
            perceptual_roughness: 0.9,
            cull_mode: None,
            double_sided: true,
            ..Default::default()
        }
    } else {
        dressed
            .get(&format!("flora:bark-{}", species.bark))
            .unwrap_or_else(|| plain(false, look))
    };
    let leaf_base = if small {
        StandardMaterial {
            base_color: Color::linear_rgb(lush[0] * 1.6, lush[1] * 1.6, lush[2] * 1.6),
            perceptual_roughness: 0.8,
            double_sided: true,
            cull_mode: None,
            diffuse_transmission: 0.4,
            ..Default::default()
        }
    } else {
        dressed
            .get("flora:leaves-shrub")
            .unwrap_or_else(|| plain(true, look))
    };
    let leaf_base = with_emissive(leaf_base, look);
    let range = understory_range(&species.form);
    let variants = species
        .variants
        .iter()
        .zip(slots)
        .map(|((bark, leaves, _), &pivot_slot)| {
            let mut pieces = Vec::new();
            for (which, (streams, base)) in [(bark, &bark_base), (leaves, &leaf_base)]
                .into_iter()
                .enumerate()
            {
                if streams.is_empty() {
                    continue;
                }
                // One material a species and half, as the species' variants
                // differ only in mesh and pivots.
                let key = (format!("understory:{}:{which}", species.id), [0; 6]);
                let material = cache
                    .entry(key)
                    .or_insert_with(|| {
                        materials.add(TreeMaterial {
                            base: base.clone(),
                            extension: buffers.extension(),
                        })
                    })
                    .clone();
                let mesh = understory_mesh(streams);
                pieces.push(TreePiece {
                    aabb: mesh.compute_aabb(),
                    mesh: meshes.add(mesh),
                    material,
                });
            }
            let mut levels = vec![
                TreeLevel {
                    pieces: Vec::new(),
                    range: range.clone(),
                    shadows: false,
                };
                UNDERSTORY_LEVEL
            ];
            levels.push(TreeLevel {
                pieces,
                range: range.clone(),
                shadows: true,
            });
            TreeVariantAssets {
                levels,
                impostor_scale: 1.0,
                pivot_slot,
            }
        })
        .collect();
    TreeSpecies {
        understory: true,
        variants,
        impostor: None,
    }
}

/// The tint hash of a plant, from its place: the same for its mesh levels
/// and its impostor.
#[must_use]
pub fn plant_tint(plant: &Plant) -> u32 {
    (plant.x.to_bits() ^ plant.z.to_bits().rotate_left(13)) & 0xffff
}

/// The top byte of a mesh level's tag: what the crowns around the plant
/// take of its sky, `255 - Plant::sky`, which `foliage.wgsl` reads.
#[must_use]
pub fn plant_shade(plant: &Plant) -> u32 {
    u32::from(u8::MAX - plant.sky) << 24
}

/// Spawn mesh level `level` of `plant` under `parent`, at `local` in the
/// parent's frame. Returns the entities, none when the species has no such
/// level.
pub fn spawn_tree_level(
    parent: &mut ChildSpawnerCommands<'_>,
    assets: &TreeAssets,
    level: usize,
    plant: &Plant,
    local: Vec3,
) -> Vec<Entity> {
    let Some(species) = assets.get(plant.species) else {
        return Vec::new();
    };
    let Some(variant) = species
        .variants
        .get(usize::from(plant.variant) % species.variants.len().max(1))
    else {
        return Vec::new();
    };
    let Some(meshes) = variant.levels.get(level) else {
        return Vec::new();
    };
    let transform = Transform::from_translation(local)
        .with_rotation(Quat::from_rotation_y(plant.yaw * core::f32::consts::TAU))
        .with_scale(Vec3::splat(plant.scale));
    // The low half a per-tree hash the shader turns into a tint and a sway
    // phase; the third byte the variant's slot in the pivot atlas (the trees
    // never read a spawn time); the top byte what the crowns around it take
    // of its sky (lighting plan phase 3a), stored so that a tag without it
    // reads as open.
    let tag = plant_tint(plant) | (u32::from(variant.pivot_slot) << 16) | plant_shade(plant);
    let mut entities = Vec::with_capacity(meshes.pieces.len());
    for piece in &meshes.pieces {
        let mut e = parent.spawn((
            TreeInstance {
                id: plant.id,
                species: plant.species,
                lod: level,
            },
            Mesh3d(piece.mesh.clone()),
            MeshMaterial3d(piece.material.clone()),
            MeshTag(tag),
            transform,
            meshes.range.clone(),
        ));
        if let Some(aabb) = piece.aabb {
            // The bounds `compute_aabb` would give, measured once per mesh.
            e.insert((aabb, NoAutoAabb));
        }
        if !meshes.shadows {
            e.insert(NotShadowCaster);
        }
        entities.push(e.id());
    }
    entities
}

/// A cell's far trees, grouped by species, in placement order.
#[must_use]
pub fn far_trees(assets: &TreeAssets, plants: &[Plant]) -> BTreeMap<u32, Vec<FarTree>> {
    let mut batches: BTreeMap<u32, Vec<FarTree>> = BTreeMap::new();
    for plant in plants {
        let Some(species) = assets.get(plant.species) else {
            continue;
        };
        if species.impostor.is_none() {
            continue;
        }
        let scale = species
            .variants
            .get(usize::from(plant.variant) % species.variants.len().max(1))
            .map_or(1.0, |v| v.impostor_scale);
        batches.entry(plant.species.0).or_default().push(FarTree {
            root: Vec3::new(plant.x, plant.y, plant.z),
            scale: plant.scale * scale,
            yaw: plant.yaw * core::f32::consts::TAU,
            tint: plant_tint(plant),
            sky: plant.sky,
        });
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streams() -> Streams {
        Streams {
            positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.1, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![[0.0, 0.0]; 3],
            foliage_a: vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ],
            foliage_b: vec![[0.25, 0.5, 0.7, 0.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn understory_takes_the_tree_contract() {
        let mesh = understory_mesh(&streams());
        let wind = mesh.attribute(wind_attribute()).expect("wind data");
        let shade = mesh.attribute(shade_attribute()).expect("shade data");
        assert_eq!(wind.len(), 3);
        assert_eq!(shade.len(), 3);
    }

    #[test]
    fn the_pivot_atlas_finds_each_table_by_its_slot_and_stores_a_repeat_once() {
        let mut atlas = PivotAtlas::default();
        let first: Vec<f32> = (0..12_u8).map(f32::from).collect();
        let second: Vec<f32> = (0..24_u8).map(|i| 100.0 + f32::from(i)).collect();
        let (a, b) = (atlas.add(&first), atlas.add(&second));
        assert_eq!((a, b), (0, 1));
        assert_eq!(atlas.add(&first), a, "an equal table is the same slot");
        let bytes = atlas.bytes();
        let word = |i: usize| {
            let mut w = [0; 4];
            w.copy_from_slice(&bytes[i * 4..i * 4 + 4]);
            u32::from_le_bytes(w)
        };
        // A header row is four words; the shader reads its x as the row where
        // the table starts, a whole-number float, and a row is four floats.
        for (slot, table) in [(a, &first), (b, &second)] {
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "a whole number"
            )]
            let row = f32::from_bits(word(usize::from(slot) * 4)) as usize;
            assert!(row >= PIVOT_SLOTS, "tables follow the header");
            for (i, v) in table.iter().enumerate() {
                assert_eq!(word(row * 4 + i), v.to_bits(), "slot {slot} float {i}");
            }
        }
        assert_eq!(bytes.len(), (PIVOT_SLOTS * 4 + 3 * PIVOT_FLOATS) * 4);
    }

    #[test]
    fn an_understory_plant_bends_about_one_pivot_at_its_root() {
        let pivots = understory_pivots(&streams(), 1.2);
        assert_eq!(pivots.len(), PIVOT_FLOATS);
        assert!(pivots[3] < 0.0, "the root has no parent");
        assert!(
            (pivots[7] - 1.2).abs() < 1e-6,
            "as long as the plant is tall"
        );

        let mut log = streams();
        log.foliage_b = vec![[0.0, 0.5, 0.0, -1.0]; 3];
        assert!(
            understory_pivots(&log, 1.0)[10] > 1000.0,
            "deadwood does not sway"
        );
    }

    #[test]
    fn default_fades_follow_the_generator_bands() {
        let fades = TreeFades::default();
        assert!(fades.near < fades.mid && fades.mid < fades.mesh && fades.mesh < fades.far.0);
    }

    #[test]
    fn understory_ranges_end_inside_the_slot_they_ride_in() {
        let fades = TreeFades::default();
        let slot_end = [fades.near, fades.mid, fades.mesh][UNDERSTORY_LEVEL];
        for form in ["flower", "fern", "shrub", "log"] {
            let range = understory_range(form);
            assert!(range.end_margin.end <= slot_end + 20.0, "{form}");
        }
    }

    #[test]
    fn a_level_takes_its_measured_bounds_so_bevy_does_not_measure_it_again() {
        use bevy::ecs::system::RunSystemOnce;
        use strata_core::RegionCoord;
        let aabb = Aabb::from_min_max(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 6.0, 1.0));
        let assets = TreeAssets {
            species: vec![Some(TreeSpecies {
                understory: false,
                variants: vec![TreeVariantAssets {
                    levels: vec![TreeLevel {
                        // Bark measured; leaves whose mesh was not there to.
                        pieces: vec![
                            TreePiece {
                                aabb: Some(aabb),
                                ..TreePiece::default()
                            },
                            TreePiece::default(),
                        ],
                        range: VisibilityRange::abrupt(0.0, 66.0),
                        shadows: true,
                    }],
                    impostor_scale: 1.0,
                    pivot_slot: 0,
                }],
                impostor: None,
            })],
        };
        let plant = Plant {
            id: strata_flora::PlantId {
                species_key: 1,
                cell: RegionCoord::new(0, 0, 0),
                slot: [3, 4],
            },
            species: FloraId(0),
            x: 3.0,
            y: 0.0,
            z: 4.0,
            yaw: 0.25,
            scale: 1.2,
            variant: 0,
            sky: u8::MAX,
        };
        let mut world = World::new();
        world
            .run_system_once(move |mut commands: Commands| {
                commands.spawn_empty().with_children(|parent| {
                    spawn_tree_level(parent, &assets, 0, &plant, Vec3::new(3.0, 0.0, 4.0));
                });
            })
            .expect("spawn system");
        let mut pieces: Vec<_> = world
            .query::<(&TreeInstance, Option<&Aabb>, Has<NoAutoAabb>)>()
            .iter(&world)
            .map(|(_, aabb, fixed)| (aabb.copied(), fixed))
            .collect();
        pieces.sort_by_key(|(aabb, _)| aabb.is_none());
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0], (Some(aabb), true));
        assert_eq!(pieces[1], (None, false));
    }
}
