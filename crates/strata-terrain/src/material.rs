//! The terrain material: a texture-array splat over Bevy's `StandardMaterial`.
//!
//! Extending `StandardMaterial` instead of writing a `Material` from scratch
//! is what makes shadows, distance fog and the atmosphere's aerial
//! perspective and sun transmittance apply to terrain without any work
//! here: the shader builds a `PbrInput` from the splat and hands it to the
//! standard lighting.
//!
//! The layers come from the frozen terrain material registry: layer `i` of
//! the arrays is the material with dense id `i`, so a `TerrainMaterialId`
//! from a world sample is an array slice without a lookup, and the mesh
//! builder and this material can never disagree about the order. Each
//! material names a terrain set `just content` bakes from `veyra-materials`
//! into the `generated://` source. Once [`strata_sim::Registries`] exists,
//! [`TerrainMaterialPlugin`] loads the four baked maps of every material
//! (base colour, normal, ORM, height), packs them into two texture arrays
//! (colour with roughness in alpha, normal with height in alpha) with a full
//! mip chain on the compute pool, and publishes the one shared material as
//! [`TerrainMaterialHandle`].
//! Every terrain region uses that one handle, so the whole terrain is one
//! material bind group.
//!
//! # Vertex contract
//!
//! Terrain regions are not `Mesh` assets. Every region draws one shared
//! grid-and-skirt mesh whose only read attribute is the vertex's place on
//! the grid, `(i, skirt, j)` in `Mesh::ATTRIBUTE_POSITION`, and its entity's
//! `MeshTag` names its slot in its page of [`crate::store::TerrainStore`]: a
//! record per grid vertex in one storage buffer ([`TerrainSplat::vertices`])
//! and a header per region in another ([`TerrainSplat::slots`]). The layout is
//! the store module's. The vertex shader (`terrain_common.wgsl`,
//! `terrain_attributes`) rebuilds from them, per vertex:
//!
//! - The position: render-space metres from the region's minimum corner,
//!   which the entity's transform places (ADR 0002); x and z from the grid
//!   index and the region's step, the height as built.
//! - The normal: the world normal from sampling, not from the mesh (ADR
//!   0003). The shader reads slope from it.
//! - The UV: world `(x, z)` in metres, wrapped to [`UV_PERIOD_M`] per
//!   region, the region's wrapped origin plus x and z. The shader divides by
//!   the per-layer tiling in [`TerrainParams::tiling`], so texture detail is
//!   anchored to the world and does not swim across LOD changes or region
//!   borders. Absolute world metres in `f32` would step by 4 mm at 50 km
//!   from the world origin, a texel or two of the close-up sets; the wrap
//!   keeps every UV under a few thousand metres, and because every tiling
//!   and every noise lattice in the shader divides the period, two regions
//!   on different sides of a wrap line sample the same texels.
//! - The layer shares: the share of each of the [`MAX_LAYERS`] layers, one
//!   normalised byte each. Every share interpolates, and the shader draws
//!   the three strongest, so a border between layers runs where their
//!   shares cross and never along a triangle's edge. The shares of a vertex
//!   sum to one.
//! - The rock: `[pair, wild_share, hi_share, corner]` as normalised bytes.
//!   The two rock layers steep ground bares, packed four bits each
//!   (`mesh::pack_rock_pair`); how much of the rock steep ground may take;
//!   the share of the pair's higher layer; and the vertex's corner of its
//!   triangles, 0, 1 or 2 (`mesh::corner_of`). An index cannot be
//!   interpolated and a flat one is a single vertex's, so the vertex shader
//!   spreads pair and share into the slot of their corner and the fragment
//!   reads all three vertices' (the mesh module's docs, "Rock").
//! - The morph: the parent height and normal, where this vertex sits on the
//!   next coarser LOD's grid, and the region's side. The vertex shader lerps
//!   height and normal towards them as the vertex approaches the distance at
//!   which the coarser LOD takes over, so a fine node meets its coarse
//!   neighbour exactly and LOD changes do not pop (the CDLOD morph, ADR
//!   0003).
//! - The canopy: the canopy colour of the biome's trees and bushes, linear
//!   RGB, and how much of the ground they cover. Blended in from
//!   `cover_near` to `cover_far`, past the drawn trees.
//! - The tint: the biome tint colour, linear RGB, times the provinces' far
//!   tint on wild ground, and the slope (as `1 - normal.y`) past which the
//!   rock takes over: the biome's threshold times the provinces'
//!   `bare_slope`. At range the shader blends the ground towards the tint,
//!   which both hides texture repetition and keeps biomes distinct on the
//!   horizon.
//! - The sky, at the vertex and at its point on the parent level's mesh:
//!   `[visibility, bent_x, bent_z, wet]` as normalised bytes, the bent
//!   normal's components mapped from `-1..1` (the mesh module's docs,
//!   "Sky"); the parent's fourth byte is how much of a cut wall the vertex
//!   is, and neither fourth byte morphs (the mesh module's docs, "Water's
//!   edge"). The vertex shader morphs between them as it morphs the height,
//!   and the fragment scales the sky's fill by the visibility and turns its
//!   lookup towards the bent normal, by [`TerrainParams::sky_visibility`].
//!
//! The fragment reads its region's header and paint by the same tag
//! ([`TerrainSplat::paint`], [`crate::paint`]): the rivers and roads drawn
//! at distance, over the splat and the rock, where the near drawing stops.
//!
//! Steep faces blend towards the rock, in the shader, from the geometric
//! normal and that threshold, so the mesh builder never has to paint cliffs
//! and a sandstone country bares sandstone sooner than a granite one does.
//! Without geology the rock is the dominant biome's slope layer, and a dune
//! biome can stay sand where a mountain biome shows rock.

use std::path::PathBuf;

use bevy::app::{App, Plugin, Startup, Update};
use bevy::asset::{Asset, AssetServer, Assets, Handle, LoadState, RenderAssetUsages};
use bevy::ecs::prelude::*;
use bevy::image::{
    Image, ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
    ImageSamplerDescriptor,
};
use bevy::material::OpaqueRendererMethod;
use bevy::math::Vec4;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MaterialPlugin, StandardMaterial,
};
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    AsBindGroup, Buffer, Extent3d, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError, TextureDataOrder, TextureDimension, TextureFormat,
};
use bevy::render::renderer::RenderDevice;
use bevy::shader::{Shader, ShaderRef};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use strata_core::platform::time::Instant;
use strata_registry::{Registry, TerrainMaterialDef, TerrainMaterialId};
use strata_sim::Registries;
use strata_stream::StreamConfig;
use tracing::{error, info, warn};

use crate::store::TerrainStore;

/// The splat shader, relative to the asset root.
pub const TERRAIN_SHADER_PATH: &str = "shaders/terrain.wgsl";

/// The vertex shader of the shadow and depth passes, relative to the asset
/// root. It applies the same LOD morph as the main pass so a shadow falls
/// from the surface that is drawn, not from an unmorphed ghost of it.
pub const TERRAIN_PREPASS_SHADER_PATH: &str = "shaders/terrain_prepass.wgsl";

/// The functions both vertex shaders share, relative to the asset root.
/// Loaded by the plugin so its `#define_import_path` is known to the shader
/// pipeline when the other two compile.
pub const TERRAIN_COMMON_SHADER_PATH: &str = "shaders/terrain_common.wgsl";

/// The workspace `assets/` directory, resolved at compile time.
///
/// Bevy's default asset root is the running package's manifest directory,
/// which for `apps/game` or a crate example is not where the textures and
/// shaders live. Development binaries set `AssetPlugin::file_path` to this;
/// a shipped build copies `assets/` next to the executable and does not.
pub const DEV_ASSET_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// The most layers the arrays hold. The uniform carries a tiling per layer
/// in four `Vec4`s, and sixteen ground types is more than the slice paints.
/// The registry's own bound, so its freeze can refuse a province face past
/// it (the mesher packs a pair of faces into four bits each).
pub const MAX_LAYERS: usize = strata_registry::MAX_DRAWN_LAYERS;

/// The period, in metres, that the mesh builder wraps texture UVs to, and
/// that every world-space lattice in the shader repeats at. 5040 is the
/// least common multiple of 1 to 10 and of the shipped tilings, so a
/// registry tiling that is an integer number of metres up to 10, or one of
/// the usual multiples beyond it, divides it exactly; a tiling that does
/// not shows a seam in the stochastic pattern where two regions wrap
/// differently. [`TerrainMaterialPlugin`] warns about such a tiling.
pub const UV_PERIOD_M: f32 = 5040.0;

/// The terrain material as an asset type: `StandardMaterial` for lighting,
/// [`TerrainSplat`] for the surface.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainSplat>;

/// Uniform parameters of the splat, mirrored by `TerrainParams` in the
/// shader. Field order is the WGSL struct's order.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct TerrainParams {
    /// Metres per repeat per layer, four to a `Vec4` because a uniform array
    /// of scalars pads each element to 16 bytes. Slots beyond the loaded
    /// layers are unused.
    pub tiling: [Vec4; MAX_LAYERS / 4],
    /// Where the slope blend starts, as a multiple of the biome's slope
    /// threshold.
    pub slope_start: f32,
    /// Where the face is all slope layer, as a multiple of the biome's
    /// slope threshold.
    pub slope_end: f32,
    /// Scale on the tangent-space xy of the normal maps; 1.0 is as authored.
    pub normal_strength: f32,
    /// Distance, in node sizes, at which a vertex starts morphing towards
    /// the coarser LOD's grid.
    pub morph_start: f32,
    /// Distance, in node sizes, at which the morph is complete. Must not
    /// exceed twice the streamer's split factor, or a fine node's edge stops
    /// short of its coarse neighbour and a crack opens.
    pub morph_end: f32,
    /// Distance at which the detail texture starts giving way to the
    /// layer's mean colour, metres.
    pub detail_near: f32,
    /// Distance at which no detail texture remains, metres. From here out
    /// the ground is the layer's mean colour under the macro variation and
    /// the biome tint, so no repeat can show however far the eye reaches.
    pub detail_far: f32,
    /// Distance at which the normal map has faded out, metres. Sooner than
    /// the albedo: a normal map seen at a grazing angle from far away
    /// shimmers and streaks where the albedo still reads as ground.
    pub normal_far: f32,
    /// Distance at which the biome tint starts to take over, metres.
    pub tint_near: f32,
    /// Distance at which the tint is at full strength, metres.
    pub tint_far: f32,
    /// How much of the ground colour the tint replaces at `tint_far`.
    pub tint_strength: f32,
    /// Amplitude of the low-frequency albedo variation over the ground, as
    /// a fraction of the albedo: 0.15 lightens and darkens the ground by a
    /// seventh over a few hundred metres, which is what keeps a plain from
    /// reading as one flat colour once the detail has faded.
    pub macro_strength: f32,
    /// What far slopes are tinted with, as a multiple of the biome tint.
    /// Below one, so a range's rock faces read darker than its snow from
    /// ten kilometres away, where the rock texture's own colour has faded.
    pub slope_tint: f32,
    /// The UV wrap period, metres; [`UV_PERIOD_M`].
    pub uv_period: f32,
    /// Distance at which the canopy colour starts to take over the ground,
    /// metres. Inside the trees' own range, so the ground under the last
    /// drawn trees is already turning.
    pub cover_near: f32,
    /// Distance at which the ground is fully the canopy colour where the
    /// biome is covered, metres. Past the trees' range.
    pub cover_far: f32,
    /// A bit per layer: set where the layer is a laid pattern read plainly
    /// rather than through the stochastic cells
    /// (`strata_registry::TerrainMaterialDef::patterned`).
    pub patterned: u32,
    /// A bit per layer: set where the layer has a grain whose direction the
    /// stochastic cells must keep, shifting it along `u` only and never
    /// turning it (`strata_registry::TerrainMaterialDef::oriented`).
    pub oriented: u32,
    /// How much the baked sky visibility darkens the sky's fill and bends
    /// its lookup, 0 to 1 (the lighting plan's phase 3a); 0 is the ground
    /// lit by the whole sky as before the bake. The look's
    /// `sky_visibility` sets it.
    pub sky_visibility: f32,
    /// A bit per layer: set where the layer is living cover that the
    /// ground's palette paints, the colour shapes and the near tint
    /// (`strata_registry::TerrainMaterialDef::palette`).
    pub palette: u32,
    // The ground painted (ticket 0006, pass 1). The ground before the pass
    // is every one of these at zero but `grain` at one and `cavity`, which
    // is baked; with `normal_strength` at one and `normal_far` at 400 m,
    // which this pass also lowered. A `cavity` of zero needs a restart.
    /// How far the lush and dry patches shift the living cover's hue, 0 to
    /// 2: greener in hollows, along the water and in patches of tens and
    /// hundreds of metres, straw on the crests between. The value is kept,
    /// so the patches are colour, not light and dark. It scales the shape
    /// field (a spread of one) before a soft knee (`tanh`) takes it to the
    /// shift, so the strongest patches round off rather than flatten.
    pub colour_lush: f32,
    /// How far the warm and cool patches shift the living cover's hue, 0 to
    /// 2, on a field of their own, through the same knee: gold against
    /// blue-green, the second colour a painter lays over a field.
    pub colour_warm: f32,
    /// The share of the way the near living ground's hue is pulled to its
    /// biomes' ground colour, 0 to 1: the vertex tint without the
    /// provinces' far tint (`mesh::RegionMesh::far_factors`), morphed with
    /// the height so a level swap does not move it. The layers' texture
    /// keeps its value and its detail; only the colour moves, so a meadow's
    /// grass and a wood's litter read in their biome's palette before the
    /// far tint takes the ground over.
    pub near_tint: f32,
    /// The share of [`near_tint`](Self::near_tint)'s pull that moves the
    /// near cover's value to its biome's ground colour's too, 0 to 1: a
    /// wood's dark litter lightens towards the moss it is painted in.
    pub near_value: f32,
    /// The detail's contrast about the layer's mean colour, 1 as baked.
    /// Under one the grain recedes and the shapes (tufts, leaves, ripples)
    /// stay nameable, as a painted ground is broad colour with few marks.
    pub grain: f32,
    /// How far the ground's roughness goes towards fully matte, 0 to 1.
    /// The wet edge, the far water and laid patterns (the city's setts
    /// and flags, `patterned`) keep their own, as laid patterns keep their
    /// full `grain`.
    pub matte: f32,
    /// How dark the drawn edge where two ground layers meet is, 0 to 1: a
    /// narrow band along the height blend's crossing, so grass meets soil
    /// and sand with a line rather than a smear.
    pub seam_shade: f32,
    /// How much of the baked cavity occlusion (the ORM's red) is multiplied
    /// into the albedo, 0 to 1: painted shade in the cracks, which no light
    /// takes away. Read when the arrays are packed, not by the shader, so a
    /// change shows on the next start.
    pub cavity: f32,
    /// Distance at which the far ground starts to take the air's colour,
    /// metres.
    pub air_near: f32,
    /// Distance at which it has all of [`air_lift`](Self::air_lift) and
    /// [`air_desaturate`](Self::air_desaturate), metres.
    pub air_far: f32,
    /// The share of the way the far ground is lifted to [`air_colour`](Self::air_colour).
    pub air_lift: f32,
    /// The share of its saturation the far ground loses.
    pub air_desaturate: f32,
    /// The air's colour as an albedo, linear RGB in `xyz` (`w` unused): a
    /// pale blue grey, lit by the sun and the sky as the ground is, so it
    /// warms at dusk with them.
    pub air_colour: Vec4,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            tiling: [Vec4::ONE; MAX_LAYERS / 4],
            slope_start: 0.75,
            slope_end: 1.35,
            // Under one (ticket 0006): a painted ground's relief is soft,
            // its light painted into the albedo.
            normal_strength: 0.6,
            morph_start: 4.25,
            morph_end: 4.95,
            detail_near: 40.0,
            detail_far: 1_200.0,
            normal_far: 250.0,
            tint_near: 400.0,
            tint_far: 6_000.0,
            tint_strength: 1.0,
            macro_strength: 0.15,
            slope_tint: 0.55,
            uv_period: UV_PERIOD_M,
            cover_near: 250.0,
            cover_far: 800.0,
            patterned: 0,
            oriented: 0,
            sky_visibility: 1.0,
            palette: 0,
            // Chosen by eye on the pass's four software-rendered stills
            // (2026-10-10), and owed a look on the GPU. The colours are the
            // mean shift those stills had (0.62 and 0.41) through the knee
            // and the shape field's measured spread.
            colour_lush: 1.4,
            colour_warm: 0.75,
            near_tint: 0.6,
            near_value: 0.5,
            grain: 0.7,
            matte: 0.55,
            seam_shade: 0.35,
            cavity: 0.8,
            air_near: 2_000.0,
            air_far: 12_000.0,
            air_lift: 0.12,
            air_desaturate: 0.25,
            air_colour: Vec4::new(0.30, 0.36, 0.45, 1.0),
        }
    }
}

impl TerrainParams {
    /// Sets the tiling of layer `layer`. Out-of-range layers are ignored.
    pub fn set_tiling(&mut self, layer: usize, metres: f32) {
        if layer >= MAX_LAYERS {
            return;
        }
        self.tiling[layer / 4][layer % 4] = metres;
    }

    /// The tiling of layer `layer`, or 1 for a layer beyond the table.
    #[must_use]
    pub fn tiling_of(&self, layer: usize) -> f32 {
        if layer >= MAX_LAYERS {
            return 1.0;
        }
        self.tiling[layer / 4][layer % 4]
    }

    /// These parameters with everything [`TerrainLook`] sets taken from
    /// `look`: all but the tiling, the morph band, the read bits and the
    /// period, which the registry and the streamer own.
    #[must_use]
    pub fn with_look(self, look: &Self) -> Self {
        Self {
            tiling: self.tiling,
            morph_start: self.morph_start,
            morph_end: self.morph_end,
            uv_period: self.uv_period,
            patterned: self.patterned,
            oriented: self.oriented,
            palette: self.palette,
            ..*look
        }
    }

    /// Morph factors that keep a fine node's edge on its coarse neighbour
    /// for the streamer's split factor `k`.
    ///
    /// A coarse node is shown while its nearest point is at least `2k`
    /// sizes away, so a fine node's shared-edge vertices, which lie on the
    /// coarse node's boundary, are at least `2k` fine sizes away and must be
    /// fully morphed by then. The coarse node's own edge vertices must not
    /// have started morphing: they can be as close as `2k` coarse sizes
    /// minus the fine node's reach: with the streamer's 10 percent
    /// hysteresis a fine node's far vertex is under `2.2k + 2 sqrt 2` fine
    /// sizes away, which is `1.1k + 1.42` coarse sizes. The band between is
    /// where the transition is spread; it needs `k` above about 1.7.
    #[must_use]
    pub fn morph_for_split_factor(k: f32) -> (f32, f32) {
        (1.1 * k + 1.5, 1.98 * k)
    }
}

/// The `MaterialExtension` half of [`TerrainMaterial`]: two texture arrays,
/// the splat parameters and the region store's three buffers.
///
/// Bindings start at 100 so they never collide with `StandardMaterial`'s.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(from_reflect = false)]
#[bind_group_data(TerrainSplatKey)]
pub struct TerrainSplat {
    /// Albedo, sRGB, one slice per layer, with the perceptual roughness in
    /// alpha (an sRGB format's alpha is linear). One read fetches both, and
    /// the stochastic tiling reads every layer three times per plane.
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub diffuse: Handle<Image>,
    /// Tangent-space normals, OpenGL convention, linear, with the layer's
    /// height in alpha for the height blend.
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub normal: Handle<Image>,
    /// Tiling, slope, morph and distance settings.
    #[uniform(106)]
    pub params: TerrainParams,
    /// Every resident region's grid records, a slot each
    /// ([`crate::store`]); a region entity's `MeshTag` is its slot. Read by
    /// the vertex stages alone.
    #[storage(107, read_only, buffer, visibility(vertex))]
    #[reflect(ignore)]
    pub vertices: Buffer,
    /// Every slot's header: the region's step, side, skirt depth and UV
    /// origin ([`crate::store::SlotHeader`]). The fragment reads it too, for
    /// the paint's level.
    #[storage(108, read_only, buffer, visibility(vertex, fragment))]
    #[reflect(ignore)]
    pub slots: Buffer,
    /// Every slot's paint: the rivers' and roads' lines the fragment draws
    /// at distance ([`crate::paint`]).
    #[storage(109, read_only, buffer, visibility(fragment))]
    #[reflect(ignore)]
    pub paint: Buffer,
    /// Whether the fragment shader carries the paint at all
    /// ([`crate::TerrainConfig::paint`]): with it off, the shader is
    /// compiled without it (`TERRAIN_FAR_PAINT`) and pays nothing, where a
    /// paint with nothing to draw still costs its code and a read a
    /// fragment.
    pub far_paint: bool,
}

/// What a [`TerrainSplat`]'s pipeline is specialised on: whether its
/// fragment shader is compiled with the far paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerrainSplatKey {
    /// [`TerrainSplat::far_paint`].
    pub far_paint: bool,
}

impl From<&TerrainSplat> for TerrainSplatKey {
    fn from(splat: &TerrainSplat) -> Self {
        Self {
            far_paint: splat.far_paint,
        }
    }
}

impl MaterialExtension for TerrainSplat {
    fn vertex_shader() -> ShaderRef {
        TERRAIN_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER_PATH.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        TERRAIN_PREPASS_SHADER_PATH.into()
    }

    /// Binds the vertex's place on the shared grid, the one attribute the
    /// vertex shader reads; everything else comes from the store (the module
    /// docs, "Vertex contract"). Also runs for the prepass and shadow
    /// pipelines, whose vertex shader is [`TERRAIN_PREPASS_SHADER_PATH`] and
    /// reads the same.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout
            .0
            .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        if key.bind_group_data.far_paint
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            fragment.shader_defs.push("TERRAIN_FAR_PAINT".into());
        }
        Ok(())
    }
}

/// The one terrain material, present once the texture arrays are built.
///
/// Absent while loading, so consumers take `Option<Res<..>>` and wait; a
/// region meshed before this exists simply gets its material a frame later.
#[derive(Resource, Debug, Clone)]
pub struct TerrainMaterialHandle(pub Handle<TerrainMaterial>);

/// The look of the terrain, as a resource so an app can set it before the
/// material is built and a HUD can change it after. Tiling per layer and
/// the morph band come from the registry and the streamer and are not
/// here; everything else in [`TerrainParams`] is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct TerrainLook {
    /// Applied to every field of [`TerrainParams`] except tiling and morph.
    pub params: TerrainParams,
}

/// Registers [`TerrainMaterial`] and builds [`TerrainMaterialHandle`] from
/// the registry's texture sets once the registries are frozen.
///
/// Needs Bevy's asset and render plugins present; the client plugin group
/// adds this, the headless group never does.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .init_resource::<TerrainLook>()
            .init_resource::<TerrainStore>()
            .add_systems(Startup, load_shader_library)
            .add_systems(Update, (assemble_terrain_arrays, apply_look));
    }
}

/// Carries a change of [`TerrainLook`] to the built material, so the look
/// panel's sliders are live: every field the look sets
/// ([`TerrainParams::with_look`]). The cavity shade alone is baked into the
/// arrays when they are packed and waits for the next start.
fn apply_look(
    look: Res<TerrainLook>,
    handle: Option<Res<TerrainMaterialHandle>>,
    store: Option<Res<TerrainStore>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Some(handle) = handle else { return };
    // Every page of the store draws with its own copy of the material.
    let pages = store.iter().flat_map(|store| store.materials());
    for handle in std::iter::once(&handle.0).chain(pages) {
        let Some(params) = materials.get(handle).map(|m| m.extension.params) else {
            continue;
        };
        let wanted = params.with_look(&look.params);
        if wanted != params
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.params = wanted;
        }
    }
}

/// Keeps the shared shader loaded for as long as the app runs; an unloaded
/// import path is a pipeline compile error.
#[derive(Resource, Debug, Clone)]
pub struct TerrainShaderLibrary(pub Handle<Shader>);

/// Loads the shared shader so its import path is known before the first
/// pipeline compiles. Skipped where there is no `Shader` asset type, which
/// is any app without a renderer.
fn load_shader_library(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    shaders: Option<Res<Assets<Shader>>>,
) {
    if shaders.is_none() {
        return;
    }
    commands.insert_resource(TerrainShaderLibrary(
        asset_server.load::<Shader>(TERRAIN_COMMON_SHADER_PATH),
    ));
}

/// The three maps of one layer, as loaded.
#[derive(Debug, Clone)]
struct LayerHandles {
    base: Handle<Image>,
    normal: Handle<Image>,
    orm: Handle<Image>,
    height: Handle<Image>,
}

/// The arrays as built on the compute pool.
struct TerrainArrays {
    diffuse: Image,
    normal: Image,
    /// How long the task took to pack and stack them, for the log line a
    /// memory or start-up review reads.
    stacked_in: std::time::Duration,
}

impl TerrainArrays {
    /// What the two arrays upload, colour and normal, in MiB: every level of
    /// every layer, from their bytes, as the GPU holds them.
    fn mib(&self) -> (f64, f64) {
        let mib = |image: &Image| {
            #[allow(clippy::cast_precision_loss, reason = "a byte count far under 2^52")]
            let bytes = image.data.as_ref().map_or(0, Vec::len) as f64;
            bytes / (1024.0 * 1024.0)
        };
        (mib(&self.diffuse), mib(&self.normal))
    }

    /// The line a memory or start-up review reads: how many layers, what
    /// they upload, and how long they took to become drawable.
    fn log_ready(&self, layers: usize, requested: Instant) {
        let (colour, normal) = self.mib();
        info!(
            "terrain material ready: {layers} layers, arrays {:.2} MiB (colour {colour:.2}, normal {normal:.2}), stacked in {:.1} ms, {:.1} ms after the maps were requested",
            colour + normal,
            self.stacked_in.as_secs_f64() * 1000.0,
            requested.elapsed().as_secs_f64() * 1000.0,
        );
    }
}

/// Loading state, removed once the material exists or loading has failed.
#[derive(Resource)]
struct TerrainTextureLoading {
    layers: Vec<LayerHandles>,
    tiling: Vec<f32>,
    reads: LayerReads,
    build: Option<Task<Result<TerrainArrays, StackError>>>,
    /// When the maps were first asked for, so the log can say how long the
    /// terrain took to become drawable.
    requested: Instant,
}

/// Marks that loading was started (or refused) so it happens once.
#[derive(Resource)]
struct TerrainTexturesRequested;

/// How the shader reads and paints each drawn layer, a bit per layer as
/// [`TerrainParams`] carries them, from the registry's flags of the same
/// names. A layer both patterned and oriented is read as patterned, as the
/// shader tests that bit first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerReads {
    /// [`TerrainParams::patterned`].
    pub patterned: u32,
    /// [`TerrainParams::oriented`].
    pub oriented: u32,
    /// [`TerrainParams::palette`].
    pub palette: u32,
}

/// The [`LayerReads`] of the registered materials.
#[must_use]
pub fn layer_reads(materials: &Registry<TerrainMaterialDef, TerrainMaterialId>) -> LayerReads {
    let mut reads = LayerReads::default();
    for (layer, (_, _, def)) in materials.iter().take(MAX_LAYERS).enumerate() {
        let bit = 1 << layer;
        if def.patterned {
            reads.patterned |= bit;
        }
        if def.oriented {
            reads.oriented |= bit;
        }
        if def.palette {
            reads.palette |= bit;
        }
    }
    reads
}

/// Loads the source maps of every registered material, main-world only:
/// they are stacked on the CPU and never uploaded individually.
fn start_loading_textures(
    commands: &mut Commands,
    asset_server: &AssetServer,
    registries: &Registries,
) {
    let load = |path: String, is_srgb: bool| {
        asset_server
            .load_builder()
            .with_settings(move |settings: &mut ImageLoaderSettings| {
                settings.is_srgb = is_srgb;
                settings.asset_usage = RenderAssetUsages::MAIN_WORLD;
            })
            .load(path)
    };
    let materials = &registries.0.materials;
    if materials.is_empty() {
        warn!("no terrain materials are registered; terrain will not be drawn");
        return;
    }
    if materials.len() > MAX_LAYERS {
        error!(
            "{} terrain materials are registered but the material holds {MAX_LAYERS}; the rest are dropped",
            materials.len()
        );
    }
    let mut layers = Vec::with_capacity(materials.len());
    let mut tiling = Vec::with_capacity(materials.len());
    let reads = layer_reads(materials);
    for (_, name, def) in materials.iter().take(MAX_LAYERS) {
        info!(
            "terrain layer {}: {name} ({})",
            layers.len(),
            def.texture_set
        );
        let cells = UV_PERIOD_M / def.tiling_m;
        if (cells - cells.round()).abs() > 1e-3 {
            warn!(
                "terrain material {name} tiles every {} m, which does not divide the UV period of {UV_PERIOD_M} m; expect a seam every {UV_PERIOD_M} m",
                def.tiling_m
            );
        }
        layers.push(LayerHandles {
            base: load(def.base_path(), true),
            normal: load(def.normal_path(), false),
            orm: load(def.orm_path(), false),
            height: load(def.height_path(), false),
        });
        tiling.push(def.tiling_m);
    }
    commands.insert_resource(TerrainTextureLoading {
        layers,
        tiling,
        reads,
        build: None,
        requested: Instant::now(),
    });
}

/// Waits for the registries, then the sources, stacks them off the main
/// thread, then publishes the material. A failed load is logged once and
/// loading stops, so a missing texture directory costs one error line
/// rather than a panic.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one state machine over the whole load; splitting it would only scatter the states"
)]
fn assemble_terrain_arrays(
    mut commands: Commands,
    requested: Option<Res<TerrainTexturesRequested>>,
    registries: Option<Res<Registries>>,
    loading: Option<ResMut<TerrainTextureLoading>>,
    look: Res<TerrainLook>,
    stream: Option<Res<StreamConfig>>,
    config: Option<Res<crate::TerrainConfig>>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    gpu: (
        Option<Res<RenderDevice>>,
        Option<ResMut<TerrainStore>>,
        Option<ResMut<Assets<Mesh>>>,
    ),
) {
    if requested.is_none() {
        let Some(registries) = registries else {
            return;
        };
        start_loading_textures(&mut commands, &asset_server, &registries);
        commands.insert_resource(TerrainTexturesRequested);
        return;
    }
    let Some(mut loading) = loading else {
        return;
    };

    if let Some(task) = loading.build.as_mut() {
        let Some(result) = block_on(poll_once(task)) else {
            return;
        };
        match result {
            Ok(arrays) => {
                let (Some(device), Some(mut store), Some(mut meshes)) = gpu else {
                    error!("the terrain material needs a render device and its region store");
                    commands.remove_resource::<TerrainTextureLoading>();
                    return;
                };
                store.build(&device, &mut meshes);
                let (Some(vertices), Some(slots), Some(paint)) = (
                    store.vertices().cloned(),
                    store.headers().cloned(),
                    store.paints().cloned(),
                ) else {
                    return;
                };
                let mut params = look.params;
                for (layer, metres) in loading.tiling.iter().enumerate() {
                    params.set_tiling(layer, *metres);
                }
                params.patterned = loading.reads.patterned;
                params.oriented = loading.reads.oriented;
                params.palette = loading.reads.palette;
                if let Some(stream) = stream {
                    // The split factor is an f64 for the streamer's sake;
                    // the shader is f32.
                    #[allow(clippy::cast_possible_truncation)]
                    let (start, end) =
                        TerrainParams::morph_for_split_factor(stream.split_factor as f32);
                    params.morph_start = start;
                    params.morph_end = end;
                }
                arrays.log_ready(loading.tiling.len(), loading.requested);
                let splat = TerrainSplat {
                    diffuse: images.add(arrays.diffuse),
                    normal: images.add(arrays.normal),
                    params,
                    vertices,
                    slots,
                    paint,
                    far_paint: config.as_deref().is_none_or(|config| config.paint),
                };
                let handle = materials.add(TerrainMaterial {
                    base: StandardMaterial {
                        perceptual_roughness: 1.0,
                        // Only a forward fragment shader is provided.
                        opaque_render_method: OpaqueRendererMethod::Forward,
                        ..Default::default()
                    },
                    extension: splat,
                });
                store.set_material(handle.clone());
                commands.insert_resource(TerrainMaterialHandle(handle));
            }
            Err(err) => error!("terrain textures could not be stacked: {err}"),
        }
        commands.remove_resource::<TerrainTextureLoading>();
        return;
    }

    for handle in loading
        .layers
        .iter()
        .flat_map(|l| [&l.base, &l.normal, &l.orm, &l.height])
    {
        match asset_server.load_state(handle.id()) {
            LoadState::Loaded => {}
            LoadState::Failed(err) => {
                error!("terrain texture failed to load (run `just content`): {err}");
                commands.remove_resource::<TerrainTextureLoading>();
                return;
            }
            LoadState::NotLoaded | LoadState::Loading => return,
        }
    }

    // Everything is in. Take the images out of `Assets` (they are not
    // needed again) and stack them off the main thread.
    let mut take = |h: &Handle<Image>| images.remove(h.id());
    let mut baked = Vec::with_capacity(loading.layers.len());
    for layer in &loading.layers {
        let (Some(base), Some(normal), Some(orm), Some(height)) = (
            take(&layer.base),
            take(&layer.normal),
            take(&layer.orm),
            take(&layer.height),
        ) else {
            error!("a terrain texture was loaded but not present in Assets<Image>");
            commands.remove_resource::<TerrainTextureLoading>();
            return;
        };
        baked.push(BakedLayer {
            base,
            normal,
            orm,
            height,
        });
    }
    let cavity = look.params.cavity;
    loading.build =
        Some(AsyncComputeTaskPool::get().spawn(async move { stack_arrays(&baked, cavity) }));
}

/// Packs every layer's maps into its two planes and stacks each plane into
/// an array with its mip chain: the work of the compute-pool task.
fn stack_arrays(baked: &[BakedLayer], cavity: f32) -> Result<TerrainArrays, StackError> {
    let started = Instant::now();
    let shade = CavityShade::new(cavity);
    let mut diffuse = Vec::with_capacity(baked.len());
    let mut normal = Vec::with_capacity(baked.len());
    for (index, layer) in baked.iter().enumerate() {
        let [d, n] = layer.planes(index, &shade)?;
        diffuse.push(d);
        normal.push(n);
    }
    Ok(TerrainArrays {
        diffuse: stack_layers(&diffuse, TextureFormat::Rgba8UnormSrgb, "terrain diffuse")?,
        normal: stack_layers(&normal, TextureFormat::Rgba8Unorm, "terrain normal")?,
        stacked_in: started.elapsed(),
    })
}

/// One layer's four baked maps, as the content step wrote them.
struct BakedLayer {
    base: Image,
    normal: Image,
    orm: Image,
    height: Image,
}

impl BakedLayer {
    /// The two planes the arrays hold, each one level at full size: the
    /// base colour, darkened by the ORM's baked occlusion (its red) through
    /// `shade`, with the ORM's roughness (its green) in alpha; and the
    /// normal with its green turned to point up the texture (ashlar bakes
    /// image-space normals) and the height in its alpha, for the shader's
    /// height blend.
    ///
    /// The occlusion is folded into the colour here rather than kept as a
    /// plane of its own (ticket 0006, "To decide" 3): the stochastic read
    /// already takes two array reads per lattice corner, three corners per
    /// layer, up to three layers and two rock planes a fragment, and a third
    /// array would add a read to each of them. Folded, it costs nothing per
    /// frame, and the cavities are dark under every light, which is what a
    /// painted ground's shade is.
    fn planes(&self, layer: usize, shade: &CavityShade) -> Result<[Image; 2], StackError> {
        let (width, height) = (self.base.width(), self.base.height());
        let texels = (width as usize) * (height as usize);
        let base = level0(&self.base, layer, 4, texels)?;
        let normal = level0(&self.normal, layer, 4, texels)?;
        let orm = level0(&self.orm, layer, 4, texels)?;
        let relief = level0(&self.height, layer, 2, texels)?;
        let mut packed = Vec::with_capacity(texels * 4);
        for (texel, height) in normal
            .as_chunks::<4>()
            .0
            .iter()
            .zip(relief.as_chunks::<2>().0)
        {
            // The 16-bit height is little-endian; its high byte is plenty
            // for a blend weight.
            packed.extend_from_slice(&[texel[0], 255 - texel[1], texel[2], height[1]]);
        }
        let mut colour = Vec::with_capacity(texels * 4);
        for (texel, orm) in base.as_chunks::<4>().0.iter().zip(orm.as_chunks::<4>().0) {
            let ao = orm[0];
            colour.extend_from_slice(&[
                shade.apply(texel[0], ao),
                shade.apply(texel[1], ao),
                shade.apply(texel[2], ao),
                orm[1],
            ]);
        }
        let plane = |data: Vec<u8>, format| {
            Image::new(
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                data,
                format,
                RenderAssetUsages::MAIN_WORLD,
            )
        };
        Ok([
            plane(colour, TextureFormat::Rgba8UnormSrgb),
            plane(packed, TextureFormat::Rgba8Unorm),
        ])
    }
}

/// The baked occlusion multiplied into an sRGB colour byte, as a table of
/// every occlusion byte by every colour byte: the darkening is in linear
/// light, and the table makes the fourteen layers' sixteen million texels
/// three lookups each.
struct CavityShade {
    table: Vec<u8>,
}

impl CavityShade {
    /// The table for a `strength` from 0 (the colour as baked) to 1 (the
    /// colour times the whole occlusion).
    fn new(strength: f32) -> Self {
        let mut table = Vec::with_capacity(256 * 256);
        for ao in 0..=255_u8 {
            for byte in 0..=255_u8 {
                table.push(cavity_shade(byte, ao, strength));
            }
        }
        Self { table }
    }

    fn apply(&self, byte: u8, ao: u8) -> u8 {
        self.table[usize::from(ao) * 256 + usize::from(byte)]
    }
}

/// An sRGB colour byte darkened by a linear occlusion byte, by `strength`
/// of the way: `colour * (1 - strength * (1 - ao))` in linear light.
#[must_use]
pub fn cavity_shade(byte: u8, ao: u8, strength: f32) -> u8 {
    let linear = srgb_decode(f32::from(byte) / 255.0);
    let occlusion = f32::from(ao) / 255.0;
    let factor = 1.0 - strength.clamp(0.0, 1.0) * (1.0 - occlusion);
    // In 0..=255 after the clamp; the rounding is the conversion.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let out = (srgb_encode(linear * factor).clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

/// The sRGB transfer function, inverted.
fn srgb_decode(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB transfer function.
fn srgb_encode(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The first mip level's bytes of a baked map. A KTX2 loads with its whole
/// chain, mip-major, so level 0 is the leading `texels * bytes` of it.
fn level0(image: &Image, layer: usize, bytes: usize, texels: usize) -> Result<&[u8], StackError> {
    let data = image.data.as_deref().ok_or(StackError::NoData { layer })?;
    data.get(..texels * bytes)
        .ok_or(StackError::UnsupportedFormat {
            layer,
            format: image.texture_descriptor.format,
        })
}

/// Why a set of images could not become one array.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackError {
    /// No layers were given.
    #[error("no layers to stack")]
    Empty,
    /// A layer's size differed from the first layer's.
    #[error("layer {layer} is {width}x{height}, expected {expected_width}x{expected_height}")]
    SizeMismatch {
        /// Which layer.
        layer: usize,
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
        /// The first layer's width.
        expected_width: u32,
        /// The first layer's height.
        expected_height: u32,
    },
    /// A layer had a pixel format this stacker does not repack.
    #[error("layer {layer} has unsupported format {format:?}")]
    UnsupportedFormat {
        /// Which layer.
        layer: usize,
        /// Its format.
        format: TextureFormat,
    },
    /// A layer had no CPU-side data (loaded render-world only).
    #[error("layer {layer} has no pixel data")]
    NoData {
        /// Which layer.
        layer: usize,
    },
}

/// Bytes per pixel of the formats the stacker accepts as input or output.
fn channels_of(format: TextureFormat) -> Option<usize> {
    match format {
        TextureFormat::R8Unorm => Some(1),
        TextureFormat::Rg8Unorm => Some(2),
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => Some(4),
        _ => None,
    }
}

/// Stacks equally sized 2D images into one `2d_array` image in `format`,
/// with a full box-filtered mip chain, layer-major as wgpu uploads it.
///
/// Channel counts are converted on the way: a greyscale roughness map
/// becomes one channel, an RGB normal map keeps four. Alpha is set opaque
/// where the source has none. Pure CPU work with no GPU dependency, so the
/// tests below cover it.
pub fn stack_layers(
    layers: &[Image],
    format: TextureFormat,
    label: &'static str,
) -> Result<Image, StackError> {
    let first = layers.first().ok_or(StackError::Empty)?;
    let (width, height) = (first.width(), first.height());
    let out_channels =
        channels_of(format).ok_or(StackError::UnsupportedFormat { layer: 0, format })?;
    let mip_count = mip_count_for(width, height);

    let mut data = Vec::with_capacity(mip_chain_bytes(width, height, out_channels) * layers.len());
    for (index, layer) in layers.iter().enumerate() {
        if layer.width() != width || layer.height() != height {
            return Err(StackError::SizeMismatch {
                layer: index,
                width: layer.width(),
                height: layer.height(),
                expected_width: width,
                expected_height: height,
            });
        }
        let in_format = layer.texture_descriptor.format;
        let in_channels = channels_of(in_format).ok_or(StackError::UnsupportedFormat {
            layer: index,
            format: in_format,
        })?;
        let src = layer
            .data
            .as_deref()
            .ok_or(StackError::NoData { layer: index })?;
        let mut level = repack(src, in_channels, out_channels);
        let (mut w, mut h) = (width, height);
        data.extend_from_slice(&level);
        for _ in 1..mip_count {
            let (next, nw, nh) = downsample(&level, w, h, out_channels);
            data.extend_from_slice(&next);
            level = next;
            w = nw;
            h = nh;
        }
    }

    // `Image::new` asserts data.len() == one mip level, so build by hand.
    // Truncation: layer counts are single digits.
    #[allow(clippy::cast_possible_truncation)]
    let depth = layers.len() as u32;
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: depth,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.label = Some(label);
    image.texture_descriptor.mip_level_count = mip_count;
    image.data_order = TextureDataOrder::LayerMajor;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some(label.to_owned()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // Ground seen at a grazing angle is the whole terrain; without
        // anisotropy the mid field turns to mush one mip early.
        anisotropy_clamp: 16,
        ..Default::default()
    });
    Ok(image)
}

/// Levels down to 1x1.
fn mip_count_for(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Bytes of one layer's full chain.
fn mip_chain_bytes(width: u32, height: u32, channels: usize) -> usize {
    let (mut w, mut h) = (width as usize, height as usize);
    let mut total = 0;
    loop {
        total += w * h * channels;
        if w == 1 && h == 1 {
            return total;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
}

/// Changes the channel count of tightly packed 8-bit pixels. Missing colour
/// channels replicate red (greyscale to RGB); a missing alpha is 255.
fn repack(src: &[u8], from: usize, to: usize) -> Vec<u8> {
    if from == to {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(src.len() / from * to);
    for px in src.chunks_exact(from) {
        for c in 0..to {
            out.push(match (c, from) {
                (3, _) => 255,
                (c, from) if c < from => px[c],
                (_, 1) => px[0],
                _ => 0,
            });
        }
    }
    out
}

/// One box-filter step. Odd sizes drop the last row or column, as the
/// standard mip definition does. A normal map's xy shrinks slightly at each
/// level as opposing bumps cancel, which is the accepted approximation.
fn downsample(src: &[u8], width: u32, height: u32, channels: usize) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width as usize, height as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; nw * nh * channels];
    let sample = |x: usize, y: usize| &src[(y.min(h - 1) * w + x.min(w - 1)) * channels..];
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = (x * 2, y * 2);
            let quad = [
                sample(x0, y0),
                sample(x0 + 1, y0),
                sample(x0, y0 + 1),
                sample(x0 + 1, y0 + 1),
            ];
            let dst = &mut out[(y * nw + x) * channels..][..channels];
            for (c, d) in dst.iter_mut().enumerate() {
                let sum: u32 = quad.iter().map(|p| u32::from(p[c])).sum();
                // Rounded average of four bytes fits a byte.
                #[allow(clippy::cast_possible_truncation)]
                {
                    *d = ((sum + 2) / 4) as u8;
                }
            }
        }
    }
    // Sizes were halved from u32 inputs.
    #[allow(clippy::cast_possible_truncation)]
    (out, nw as u32, nh as u32)
}

/// The asset root for development binaries, as a path.
pub fn dev_asset_root() -> PathBuf {
    PathBuf::from(DEV_ASSET_ROOT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, format: TextureFormat, fill: &[u8]) -> Image {
        let channels = channels_of(format).expect("test format");
        let mut data = Vec::with_capacity((width * height) as usize * channels);
        for _ in 0..width * height {
            data.extend_from_slice(&fill[..channels]);
        }
        Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            format,
            RenderAssetUsages::MAIN_WORLD,
        )
    }

    /// The shader's per-layer read bits come from the shipped registry's
    /// flags, at each material's layer: the laid patterns read plainly, the
    /// rocks with a grain never turned, the rest stochastic and turned.
    #[test]
    fn layer_reads_follow_the_registry_flags() {
        let registries =
            strata_registry::load_from_dir(dev_asset_root()).expect("the shipped assets load");
        let LayerReads {
            patterned,
            oriented,
            palette,
        } = layer_reads(&registries.materials);
        let bit = |name: &str| {
            let id = registries
                .material_id(&strata_registry::Namespaced::parse(name).expect("valid"))
                .expect("ships");
            1_u32 << id.0
        };
        let expect = |names: &[&str]| names.iter().fold(0, |bits, name| bits | bit(name));
        assert_eq!(patterned, expect(&["strata:setts", "strata:flagstones"]));
        assert_eq!(
            oriented,
            expect(&[
                "strata:sandstone",
                "strata:basalt",
                "strata:slate",
                "strata:clay"
            ])
        );
        assert_eq!(oriented & bit("strata:granite"), 0);
        assert_eq!(palette, expect(&["strata:grass", "strata:forest_floor"]));
    }

    #[test]
    // Exact copies of constants, not arithmetic.
    #[allow(clippy::float_cmp)]
    fn tiling_table_round_trips() {
        let mut params = TerrainParams::default();
        params.set_tiling(0, 4.0);
        params.set_tiling(5, 6.0);
        params.set_tiling(MAX_LAYERS, 99.0);
        assert_eq!(params.tiling_of(0), 4.0);
        assert_eq!(params.tiling_of(5), 6.0);
        assert_eq!(params.tiling[1].y, 6.0);
        assert_eq!(params.tiling_of(MAX_LAYERS), 1.0);
    }

    #[test]
    fn morph_band_sits_inside_the_split_band() {
        let (start, end) = TerrainParams::morph_for_split_factor(2.5);
        // A fine node's shared edge is at least 2k fine sizes away: the
        // morph must be complete there.
        assert!(end <= 2.0 * 2.5, "{end}");
        // A coarse node's edge can be as close as 2.2k + 2 sqrt(2) fine
        // sizes, which is (2.2k + 2.83) / 2 coarse sizes; morphing must not
        // have started there.
        assert!(start >= f32::midpoint(2.2 * 2.5, 2.83), "{start}");
        assert!(start < end);
    }

    #[test]
    fn stacks_with_a_full_mip_chain() {
        let a = image(4, 4, TextureFormat::Rgba8UnormSrgb, &[10, 20, 30, 255]);
        let b = image(4, 4, TextureFormat::Rgba8UnormSrgb, &[40, 50, 60, 255]);
        let stacked = stack_layers(&[a, b], TextureFormat::Rgba8UnormSrgb, "t").expect("stack");
        assert_eq!(stacked.texture_descriptor.size.depth_or_array_layers, 2);
        assert_eq!(stacked.texture_descriptor.mip_level_count, 3);
        assert_eq!(stacked.data_order, TextureDataOrder::LayerMajor);
        let data = stacked.data.as_ref().expect("data");
        let per_layer = (16 + 4 + 1) * 4;
        assert_eq!(data.len(), per_layer * 2);
        // Layer-major: layer 0's chain, then layer 1's. A flat fill survives
        // every mip unchanged.
        assert_eq!(&data[0..4], &[10, 20, 30, 255]);
        assert_eq!(&data[per_layer - 4..per_layer], &[10, 20, 30, 255]);
        assert_eq!(&data[per_layer..per_layer + 4], &[40, 50, 60, 255]);
    }

    #[test]
    fn repacks_greyscale_to_rgba_and_back() {
        let grey = image(2, 2, TextureFormat::R8Unorm, &[7]);
        let rgba = stack_layers(std::slice::from_ref(&grey), TextureFormat::Rgba8Unorm, "t")
            .expect("stack");
        assert_eq!(&rgba.data.as_ref().expect("data")[0..4], &[7, 7, 7, 255]);
        let colour = image(2, 2, TextureFormat::Rgba8Unorm, &[9, 1, 2, 3]);
        let r8 = stack_layers(&[colour], TextureFormat::R8Unorm, "t").expect("stack");
        assert_eq!(&r8.data.as_ref().expect("data")[0..1], &[9]);
        assert_eq!(r8.texture_descriptor.mip_level_count, 2);
    }

    #[test]
    fn rejects_mismatched_sizes() {
        let a = image(4, 4, TextureFormat::R8Unorm, &[1]);
        let b = image(2, 4, TextureFormat::R8Unorm, &[1]);
        let err = stack_layers(&[a, b], TextureFormat::R8Unorm, "t").expect_err("mismatch");
        assert!(matches!(err, StackError::SizeMismatch { layer: 1, .. }));
        assert!(matches!(
            stack_layers(&[], TextureFormat::R8Unorm, "t"),
            Err(StackError::Empty)
        ));
    }

    /// The cavity shade is the colour in linear light times the share of
    /// the occlusion asked for: none at no strength or an occlusion of
    /// one, and an sRGB byte darkened by half its light at strength one
    /// and an occlusion of a half.
    #[test]
    fn the_cavity_shade_darkens_in_linear_light() {
        for byte in [0_u8, 1, 17, 128, 200, 255] {
            assert_eq!(cavity_shade(byte, 255, 1.0), byte);
            assert_eq!(cavity_shade(byte, 0, 0.0), byte);
        }
        assert_eq!(cavity_shade(200, 0, 1.0), 0);
        // 188 is linear 0.5029; times 128 / 255 it is 0.2524, 138 in sRGB.
        assert_eq!(cavity_shade(188, 128, 1.0), 138);
        // Monotone in the occlusion.
        let mut last = 0;
        for ao in 0..=255_u8 {
            let out = cavity_shade(160, ao, 0.8);
            assert!(out >= last, "{ao}: {out} after {last}");
            last = out;
        }
        let shade = CavityShade::new(0.8);
        assert_eq!(shade.apply(160, 77), cavity_shade(160, 77, 0.8));
    }

    /// The look sets everything but what the registry and the streamer own.
    #[test]
    #[allow(clippy::float_cmp, reason = "copies, not arithmetic")]
    fn the_look_leaves_the_registry_s_fields_alone() {
        let mut built = TerrainParams::default();
        built.set_tiling(3, 6.0);
        built.morph_start = 3.0;
        built.patterned = 0b101;
        built.palette = 0b11;
        let look = TerrainParams {
            near_tint: 0.9,
            grain: 0.5,
            morph_start: 99.0,
            patterned: 0,
            ..TerrainParams::default()
        };
        let applied = built.with_look(&look);
        assert_eq!(applied.near_tint, 0.9);
        assert_eq!(applied.grain, 0.5);
        assert_eq!(applied.tiling_of(3), 6.0);
        assert_eq!(applied.morph_start, 3.0);
        assert_eq!(applied.patterned, 0b101);
        assert_eq!(applied.palette, 0b11);
    }

    #[test]
    fn downsample_averages_with_rounding() {
        let src = [
            0u8, 0, 0, 2, 100, 100, 100, 100, 200, 200, 200, 200, 0, 0, 0, 0,
        ];
        let (out, w, h) = downsample(&src, 2, 2, 4);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![75, 75, 75, 76]);
        assert_eq!(mip_count_for(2048, 2048), 12);
        assert_eq!(mip_count_for(1, 1), 1);
        assert_eq!(mip_chain_bytes(4, 4, 1), 16 + 4 + 1);
    }
}
