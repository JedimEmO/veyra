//! Persistent 64 m foliage cells, independent of terrain mesh LOD ownership.
//! Placement is deterministic in `strata-flora`; this crate schedules bounded
//! cell jobs and incrementally installs baked tree levels, far-tree batches
//! and compact grass buffers.

// Air under dense crowns (lighting plan phase 2c).
pub mod fog_volumes;
pub mod grass;
pub mod impostor;
mod streaming;
pub mod tree;

use bevy::app::{App, Plugin, PostUpdate, Startup, Update};
use bevy::asset::{AssetApp, AssetServer, Assets, Handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::color::Color;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::ecs::world::DeferredWorld;
use bevy::light::NotShadowCaster;
use bevy::math::{Vec2, Vec3, Vec4};
use bevy::mesh::{Mesh, Mesh3d, MeshTag};
use bevy::pbr::{MaterialPlugin, MeshMaterial3d, StandardMaterial};
use bevy::render::render_resource::Buffer;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::shader::Shader;
use bevy::state::condition::in_state;
use bevy::state::state::OnExit;
use bevy::time::Time;
use bevy::transform::components::Transform;
use std::sync::{Arc, Mutex, PoisonError};
use strata_app::{AppState, OriginSet, Registries, RenderOrigin, Wind};
use strata_core::RegionCoord;
use strata_flora::{CellFlora, FloraTable};
use strata_terrain::TerrainSet;
use tracing::info;

use grass::{
    BUCKETS, FOLIAGE_WIND_SHADER_PATH, GRASS_COMMON_SHADER_PATH, GrassStore, WIND_PERIOD_M,
};
pub use grass::{GrassCellRecord, GrassField, GrassLod, GrassMaterial, GrassParams, WindParams};
pub use impostor::{FarTreeMaterial, FarTreeStore, FarTrees, ImpostorBatch, ImpostorBatchMaterial};
use strata_registry::{CANOPY_FILE, CanopyLook};
use tree::{CANOPY_SHADER_PATH, TREE_COMMON_SHADER_PATH};
pub use tree::{
    TreeAssets, TreeFades, TreeImpostorBatch, TreeInstance, TreeLoading, TreeMaterial, TreePiece,
    TreeWind,
};

/// Foliage work runs after terrain collection, without inheriting its lifetime.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoliageSet {
    /// Select cells and start a bounded number of jobs.
    Dispatch,
    /// Resume entity installation and retirement within frame budgets.
    Collect,
    /// Update the shared wind buffer and statistics.
    Wind,
}

/// Streaming and rendering budgets. Insert before the plugin to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FoliageConfig {
    /// Whether cells are wanted.
    pub enabled: bool,
    /// Near to mid grass crossfade in metres.
    pub near_fade: (f32, f32),
    /// Mid grass to terrain crossfade in metres.
    pub far_fade: (f32, f32),
    /// Tree mesh crossfade distances.
    pub tree_fades: TreeFades,
    /// Completed placement results accepted in one frame.
    pub collect_per_frame: usize,
    /// Hard entity installation budget, including cell roots. Applying a
    /// spawned entity costs 2 to 5 us after the collector, so the default
    /// keeps a burst's apply near half a millisecond.
    pub spawn_budget_per_frame: u64,
    /// Hard entity retirement budget, including cell roots.
    pub despawn_budget_per_frame: usize,
    /// Soft CPU budget for queuing installation and retirement, milliseconds,
    /// checked per plant and per cell. The previous frame's apply counts
    /// against it, up to half of it. A single bounded tree or grass upload
    /// is atomic.
    pub collect_budget_ms: f32,
    /// Grass bytes uploaded per frame. One cell larger than this waits for
    /// a frame of its own; the shared mesh capacity also bounds each upload.
    pub upload_budget_bytes: usize,
    /// Maximum placement jobs, including completed results awaiting collection.
    pub max_in_flight: usize,
    /// New placement jobs per frame.
    pub dispatch_per_frame: usize,
    /// Extra distance to prepare cells, their far trees and their grass
    /// before their fade bands; a plant's mesh levels follow `level_lead_m`.
    pub prefetch_m: f32,
    /// Retain resident cells this far beyond the preparation radius.
    pub retention_m: f32,
    /// How far before a plant's mesh level starts fading in it is made,
    /// metres, at a standstill: the camera moves after the streamer has
    /// looked at it, and a level made late would show as a hole.
    pub level_lead_m: f32,
    /// And how many seconds of the camera's speed are added to that, so a
    /// flight's levels are made as far ahead in time as a walk's.
    pub level_lead_s: f32,
    /// How far past its band a level is kept before it is dropped, metres,
    /// so a camera walking a band's edge does not make and drop it each step.
    pub level_margin_m: f32,
    /// The least time a level lives once made, seconds, for the same reason
    /// when the camera turns or bobs about the edge.
    pub level_dwell_s: f32,
    /// CPU placement cache after cells leave the resident ring.
    pub cache_bytes: usize,
    /// Maximum cached cells, also bounds entries for empty ground.
    pub cache_cells: usize,
    /// Diffuse transmission of blades.
    pub transmission: f32,
}

impl Default for FoliageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            near_fade: (110.0, 130.0),
            far_fade: (400.0, 480.0),
            tree_fades: TreeFades::default(),
            collect_per_frame: 4,
            spawn_budget_per_frame: 150,
            despawn_budget_per_frame: 150,
            collect_budget_ms: 1.5,
            upload_budget_bytes: 1024 * 1024,
            max_in_flight: 8,
            dispatch_per_frame: 4,
            prefetch_m: 128.0,
            retention_m: 128.0,
            level_lead_m: 8.0,
            level_lead_s: 0.25,
            level_margin_m: 8.0,
            level_dwell_s: 1.0,
            cache_bytes: 32 * 1024 * 1024,
            cache_cells: 256,
            transmission: 0.4,
        }
    }
}

/// How strongly the placer's sky term under crowns darkens the sky's fill
/// on trees, bushes, rocks and grass, 0 to 1 (lighting plan phase 3a); 0 is
/// foliage lit by the whole sky. The client sets it from the look's
/// `sky_visibility`; the wind's buffer carries it to the near meshes and the
/// grass, the impostor materials' uniform to the far trees.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FoliageSky {
    /// The strength.
    pub strength: f32,
    /// How much sunlight comes through a blade of grass seen against the
    /// sun and rims its edges, 0 to 2 (lighting plan phase 5b): the look's
    /// `grass_rim`, which the client sets. Here because the wind's buffer
    /// that carries this resource to the grass is the one the grass binds.
    pub grass_rim: f32,
    /// The shade's colour from the look's `LookUniform`, which the client
    /// sets; the wind's buffer carries it to trees and grass, the far-tree
    /// uniform to the impostors.
    pub shade_tint: Vec3,
    /// The crown's grouping (`assets/look/canopy.ron`, visual.md F5), read at
    /// start-up over the compiled-in copy: the near shader applies its core,
    /// lift and rim, the impostor shader its rim (its core and lift are in
    /// the baked atlas).
    pub canopy: CanopyLook,
}

impl Default for FoliageSky {
    fn default() -> Self {
        Self {
            strength: 1.0,
            grass_rim: 1.0,
            shade_tint: Vec3::ONE,
            canopy: CanopyLook::default(),
        }
    }
}

/// Reads the crown's grouping from the asset root, when the app has one, over
/// the compiled-in copy; a broken file is reported and the shipped look kept.
fn load_canopy(root: Option<Res<strata_app::WorldAssetRoot>>, mut sky: ResMut<FoliageSky>) {
    let Some(root) = root else {
        return;
    };
    match root.0.read_string(CANOPY_FILE) {
        Ok(text) => match CanopyLook::parse(&text) {
            Ok(read) => sky.canopy = read,
            Err(error) => tracing::warn!("{CANOPY_FILE}: {error}; the shipped look stays"),
        },
        Err(error) => tracing::debug!("{CANOPY_FILE}: {error}; the shipped look stays"),
    }
}

/// Live resident counts and last-frame streaming costs for the HUD and log.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct FoliageStats {
    /// Resident grass cells (visibility may cull them).
    pub cells: usize,
    /// Resident blade records.
    pub blades: u64,
    /// Resident placed plants, including bushes and decorative understory.
    pub trees: u64,
    /// Resident far trees, drawn in impostor batches.
    pub far_trees: u64,
    /// Resident render entities, including cell roots.
    pub entities: u64,
    /// Of those, the mesh level entities of plants: one level a plant, two
    /// while it crosses from one to the next.
    pub tree_meshes: u64,
    /// Outstanding placement jobs.
    pub in_flight: usize,
    /// Completed cell jobs since world entry, including grass upgrades.
    pub placed: u64,
    /// Most recently collected placement time, milliseconds.
    pub last_place_ms: f32,
    /// Installation and retirement command generation, milliseconds.
    pub collect_ms: f32,
    /// Deferred entity insertion/removal, milliseconds.
    pub apply_ms: f32,
    /// Grass buffer creation, included in collection time, milliseconds.
    pub upload_ms: f32,
    /// Bytes uploaded this frame.
    pub uploaded_bytes: usize,
    /// Entities queued for insertion this frame.
    pub spawned: u64,
    /// Entities queued for removal this frame.
    pub despawned: usize,
    /// Placement results reused from the bounded cache.
    pub cache_hits: u64,
    /// Cells whose plants' levels were looked at this frame: those the
    /// camera has moved past the slack of, or with work left over.
    pub looked: u64,
    /// Bytes of CPU plant and blade records in resident cells, excluding cache/jobs.
    pub resident_cpu_bytes: usize,
    /// Bytes in resident grass storage buffers, excluding shared meshes/materials.
    pub resident_grass_bytes: usize,
    /// Grass buffer bytes awaiting budgeted entity retirement.
    pub retiring_grass_bytes: usize,
}

/// One grass LOD, owned by a persistent foliage cell.
#[derive(Component, Debug, Clone, Copy)]
pub struct GrassPatch {
    /// World cell coordinate.
    pub cell: RegionCoord,
    /// Geometry level.
    pub lod: GrassLod,
    /// Records in the cell buffer.
    pub blades: u32,
}

/// The cell slot a grass entity draws, also its `MeshTag`; the cell's
/// ranges of the shared buffers go back when the last of its entities is
/// despawned, whoever despawns it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(on_remove = release_grass)]
pub struct GrassSlot(pub u32);

fn release_grass(world: DeferredWorld, context: HookContext) {
    let Some(slot) = world.get::<GrassSlot>(context.entity).copied() else {
        return;
    };
    if let Some(assets) = world.get_resource::<GrassAssets>() {
        assets
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .release(slot.0);
    }
}

#[derive(Resource, Clone)]
struct PlaceInputs {
    table: Arc<FloraTable>,
}

#[derive(Resource)]
struct GrassAssets {
    meshes: [Vec<Handle<Mesh>>; 2],
    params: GrassParams,
    wind: Buffer,
    queue: RenderQueue,
    /// Behind a lock because cells are installed through the shared
    /// reference the collector holds and given back from a component hook;
    /// both run on the main world's thread, so it is never contended.
    store: Mutex<GrassStore>,
}

#[derive(Resource)]
struct FoliageShaderLibrary(
    #[allow(dead_code, reason = "keep imported modules loaded")] Vec<Handle<Shader>>,
);

/// Registers the foliage materials and the persistent cell scheduler.
#[derive(Debug, Clone, Copy, Default)]
pub struct FoliagePlugin;

impl Plugin for FoliagePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<GrassMaterial>::default(),
            MaterialPlugin::<TreeMaterial>::default(),
            MaterialPlugin::<FarTreeMaterial>::default(),
        ))
        // A species' far picture as the tree assets describe it; what is
        // drawn is its `FarTreeMaterial`.
        .init_asset::<ImpostorBatchMaterial>()
        .init_resource::<FarTreeStore>()
        .init_resource::<FoliageConfig>()
        // Sky under crowns (lighting plan phase 3a).
        .init_resource::<FoliageSky>()
        .init_resource::<FoliageStats>()
        .init_resource::<streaming::CellStream>()
        .add_systems(
            Startup,
            (load_shader_library, build_grass_assets, load_canopy),
        )
        .configure_sets(
            Update,
            (FoliageSet::Dispatch, FoliageSet::Collect, FoliageSet::Wind)
                .chain()
                .after(TerrainSet::Collect)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(
            Update,
            (
                prepare_inputs,
                install_tree_assets,
                install_far_materials,
                next_grass_frame,
                streaming::dispatch,
            )
                .chain()
                .in_set(FoliageSet::Dispatch),
        )
        .add_systems(
            Update,
            (streaming::collect, ApplyDeferred, streaming::record_apply)
                .chain()
                .in_set(FoliageSet::Collect),
        )
        .add_systems(
            Update,
            (
                log_stats,
                sync_far_fade.run_if(resource_changed::<FoliageConfig>),
                // Sky under crowns (lighting plan phase 3a).
                sync_impostor_sky,
            )
                .in_set(FoliageSet::Wind),
        )
        .add_systems(
            PostUpdate,
            update_wind
                .after(OriginSet::Sync)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(OnExit(AppState::InWorld), streaming::clear);
        // Air under dense crowns (lighting plan phase 2c).
        fog_volumes::register(app);
    }
}

fn load_shader_library(
    mut commands: Commands,
    server: Res<AssetServer>,
    shaders: Option<Res<Assets<Shader>>>,
) {
    if shaders.is_some() {
        commands.insert_resource(FoliageShaderLibrary(vec![
            server.load(FOLIAGE_WIND_SHADER_PATH),
            server.load(GRASS_COMMON_SHADER_PATH),
            server.load(TREE_COMMON_SHADER_PATH),
            server.load(CANOPY_SHADER_PATH),
        ]));
    }
}

fn build_grass_assets(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    device: Option<Res<RenderDevice>>,
    queue: Option<Res<RenderQueue>>,
    mut store: ResMut<FarTreeStore>,
) {
    let (Some(mut meshes), Some(device), Some(queue)) = (meshes, device, queue) else {
        return;
    };
    store.build(&device, &mut meshes);
    let mut grass = GrassStore::default();
    grass.build(&device);
    let build = |lod, meshes: &mut Assets<Mesh>| {
        BUCKETS
            .iter()
            .map(|&slots| meshes.add(grass::blade_mesh(lod, slots)))
            .collect::<Vec<_>>()
    };
    let near = build(GrassLod::Near, &mut meshes);
    let mid = build(GrassLod::Mid, &mut meshes);
    commands.insert_resource(GrassAssets {
        meshes: [near, mid],
        params: GrassParams::default(),
        wind: grass::wind_buffer(&device),
        queue: queue.clone(),
        store: Mutex::new(grass),
    });
}

/// Starts the grass store's frame: freed ranges whose quarantine is over
/// can be handed out again.
fn next_grass_frame(assets: Option<ResMut<GrassAssets>>) {
    if let Some(mut assets) = assets {
        assets
            .store
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .next_frame();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the inputs placement and trees start from"
)]
fn prepare_inputs(
    mut commands: Commands,
    cfg: Res<FoliageConfig>,
    server: Res<AssetServer>,
    inputs: Option<Res<PlaceInputs>>,
    registries: Option<Res<Registries>>,
    library: Option<Res<strata_app::TreeLibraryRes>>,
    assets: Option<ResMut<GrassAssets>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    if inputs.is_some() || !cfg.enabled {
        return;
    }
    let (Some(registries), Some(mut assets)) = (registries, assets) else {
        return;
    };
    // The same table physics places trunks from: the library's variants.
    let table = Arc::new(FloraTable::from_registries_and_library(
        &registries.0,
        library.as_deref().and_then(|library| library.0.as_deref()),
    ));
    assets.params = GrassParams::default();
    for species in table.species() {
        let Some(palette) = registries.0.grass_palette_index(species.id) else {
            continue;
        };
        let i = usize::from(palette);
        let Some(def) = registries.0.flora.get(species.id) else {
            continue;
        };
        let dry = def.tint_dry.0;
        let lush = def.tint_lush.0;
        assets.params.tint_dry[i] = Vec4::new(dry[0], dry[1], dry[2], 1.0);
        assets.params.tint_lush[i] = Vec4::new(lush[0], lush[1], lush[2], 1.0);
        assets.params.width[i / 4][i % 4] = def.grass_shape[0];
        assets.params.shape[i] = Vec4::new(
            def.grass_shape[1],
            def.grass_shape[2],
            def.grass_shape[3],
            0.0,
        );
    }
    // Every cell draws with this one material, made once the species table
    // is known; it is what the cells each made for themselves before, less
    // the cell's own origin and time, which are in its record.
    let assets = &mut *assets;
    assets
        .store
        .get_mut()
        .unwrap_or_else(PoisonError::into_inner)
        .make_material(
            StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.85,
                double_sided: true,
                cull_mode: None,
                diffuse_transmission: cfg.transmission,
                ..Default::default()
            },
            assets.params,
            &assets.wind,
            &mut materials,
        );
    commands.insert_resource(TreeLoading::start(
        &server,
        &registries.0,
        &table,
        library.as_deref().and_then(|library| library.0.as_deref()),
    ));
    commands.insert_resource(PlaceInputs { table });
}

// Props load through the asset server; once every one has settled the
// materials and pivot tables are made in one frame, which is a few dozen
// small assets: the meshes were uploaded by the loader.
#[allow(clippy::too_many_arguments, reason = "the render assets it creates")]
fn install_tree_assets(
    mut commands: Commands,
    cfg: Res<FoliageConfig>,
    loading: Option<ResMut<TreeLoading>>,
    inputs: Option<Res<PlaceInputs>>,
    assets: Option<Res<GrassAssets>>,
    device: Option<Res<RenderDevice>>,
    server: Res<AssetServer>,
    props: Option<Res<Assets<ashlar_bevy::prelude::PropAsset>>>,
    libraries: Option<Res<Assets<ashlar_bevy::baked::MaterialLibraryAsset>>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TreeMaterial>>,
    mut batches: ResMut<Assets<ImpostorBatchMaterial>>,
) {
    let (Some(mut loading), Some(inputs), Some(assets), Some(device), Some(props), Some(libraries)) =
        (loading, inputs, assets, device, props, libraries)
    else {
        return;
    };
    if let Some(trees) = loading.try_install(
        inputs.table.species().len(),
        &assets.wind,
        &device,
        &server,
        &props,
        &libraries,
        &mut meshes,
        &mut materials,
        &mut batches,
        &cfg.tree_fades,
    ) {
        commands.insert_resource(trees);
        commands.remove_resource::<TreeLoading>();
    }
}

fn grass_lods(cfg: &FoliageConfig) -> [(GrassLod, VisibilityRange); 2] {
    [
        (
            GrassLod::Near,
            VisibilityRange {
                start_margin: 0.0..0.0,
                end_margin: cfg.near_fade.0..cfg.near_fade.1,
                use_aabb: false,
            },
        ),
        (
            GrassLod::Mid,
            VisibilityRange {
                start_margin: cfg.near_fade.0..cfg.near_fade.1,
                end_margin: cfg.far_fade.0..cfg.far_fade.1,
                use_aabb: false,
            },
        ),
    ]
}

/// A cell's grass: one entity per LOD over the shared blade meshes, all
/// tagged with one cell slot whose record says where the cell's blades are
/// in the shared buffer, and all drawn with the one grass material, so
/// every grass cell is the same batch set.
fn spawn_grass(
    p: &mut ChildSpawnerCommands<'_>,
    cfg: &FoliageConfig,
    assets: &GrassAssets,
    device: &RenderDevice,
    materials: &mut Assets<GrassMaterial>,
    flora: &CellFlora,
    spawn_time: f32,
) -> Vec<Entity> {
    let count = u32::try_from(flora.grass.len()).unwrap_or(u32::MAX);
    let lods: Vec<_> = grass_lods(cfg)
        .into_iter()
        .filter_map(|(lod, range)| {
            let bucket = lod.bucket_for(count)?;
            let mesh = assets.meshes[usize::from(lod == GrassLod::Mid)][bucket].clone();
            Some((lod, range, mesh))
        })
        .collect();
    if lods.is_empty() {
        return Vec::new();
    }
    let (min_y, max_y) = flora
        .grass
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), b| {
            let y = f32::from_bits(b.y_bits);
            (lo.min(y), hi.max(y))
        });
    // CPU culling, GPU preprocessing and both shader passes must use the
    // same origin at ground height. A custom AABB alone fixes only CPU LOD.
    let centre_y = f32::midpoint(min_y, max_y);
    let (slot, material) = {
        let mut store = assets.store.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(material) = store.material().cloned() else {
            return Vec::new();
        };
        let Some(slot) = store.install(
            &flora.grass,
            centre_y,
            spawn_time,
            u32::try_from(lods.len()).unwrap_or(u32::MAX),
            device,
            &assets.queue,
            materials,
        ) else {
            return Vec::new();
        };
        (slot, material)
    };
    let aabb = Aabb::from_min_max(
        Vec3::new(-35.0, min_y - centre_y - 0.5, -35.0),
        Vec3::new(35.0, max_y - centre_y + 3.0, 35.0),
    );
    lods.into_iter()
        .map(|(lod, range, mesh)| {
            p.spawn((
                GrassPatch {
                    cell: flora.coord,
                    lod,
                    blades: count,
                },
                GrassSlot(slot),
                MeshTag(slot),
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(32.0, centre_y, 32.0),
                aabb,
                NoAutoAabb,
                NotShadowCaster,
                range,
            ))
            .id()
        })
        .collect()
}

fn update_wind(
    time: Res<Time>,
    origin: Res<RenderOrigin>,
    wind: Res<Wind>,
    sky: Res<FoliageSky>,
    assets: Option<Res<GrassAssets>>,
    queue: Option<Res<RenderQueue>>,
    mut prev: Local<Option<(f32, f32, f32)>>,
) {
    let (Some(assets), Some(queue)) = (assets, queue) else {
        return;
    };
    let now = time.elapsed_secs();
    #[allow(clippy::cast_possible_truncation)]
    let wrap = |v: f64| v.rem_euclid(WIND_PERIOD_M) as f32;
    let origin_x = wrap(origin.0.x());
    let origin_z = wrap(origin.0.z());
    let previous = prev.unwrap_or((now, origin_x, origin_z));
    let params = WindParams {
        direction: Vec2::from(wind.direction).normalize_or(Vec2::X),
        strength: wind.strength,
        time: now,
        prev_time: previous.0,
        origin_x,
        origin_z,
        gust_scale: wind.gust_scale_m.max(1.0),
        prev_origin_x: previous.1,
        prev_origin_z: previous.2,
        sky_visibility: sky.strength,
        grass_rim: sky.grass_rim,
        shade_tint: sky.shade_tint.to_array(),
        canopy_core: sky.canopy.cool.map(|lane| lane * sky.canopy.core),
        canopy_lift: sky.canopy.lift,
        canopy_rim: sky.canopy.rim,
    };
    queue.write_buffer(&assets.wind, 0, &grass::wind_bytes(&params));
    *prev = Some((now, origin_x, origin_z));
}

/// Makes every species' drawn far-tree material from its description once
/// the tree assets are installed: the description's atlas and layout over
/// the shared buffer, with the distance setting's fade and the sky term
/// already applied, so a batch never draws with a stale one.
fn install_far_materials(
    cfg: Res<FoliageConfig>,
    sky: Res<FoliageSky>,
    trees: Option<Res<TreeAssets>>,
    descriptions: Res<Assets<ImpostorBatchMaterial>>,
    mut materials: ResMut<Assets<FarTreeMaterial>>,
    mut store: ResMut<FarTreeStore>,
) {
    let Some(trees) = trees else { return };
    if !trees.is_changed() {
        return;
    }
    let Some(buffer) = store.buffer().cloned() else {
        return;
    };
    let drawn = trees
        .species
        .iter()
        .map(|species| {
            let far = species.as_ref()?.impostor.as_ref()?;
            let description = descriptions.get(&far.material)?;
            let mut extension = FarTrees::from_description(&description.extension, buffer.clone());
            extension.uniform.fade.z = cfg.tree_fades.far.0;
            extension.uniform.fade.w = cfg.tree_fades.far.1;
            extension.uniform.shading.z = sky.strength;
            extension.uniform.shading.w = sky.canopy.rim;
            extension.uniform.shade = sky.shade_tint.extend(1.0);
            Some(materials.add(FarTreeMaterial {
                base: description.base.clone(),
                extension,
            }))
        })
        .collect();
    store.set_materials(drawn);
}

/// The distance setting moves where far trees fade out; every batch of a
/// species shares its material, so the change is one write per species.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_far_fade(
    cfg: Res<FoliageConfig>,
    store: Res<FarTreeStore>,
    mut materials: ResMut<Assets<FarTreeMaterial>>,
) {
    let fade = cfg.tree_fades.far;
    for handle in store.materials() {
        if materials
            .get(handle)
            .is_some_and(|m| (m.extension.uniform.fade.z, m.extension.uniform.fade.w) != fade)
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.uniform.fade.z = fade.0;
            material.extension.uniform.fade.w = fade.1;
        }
    }
}

/// Carries [`FoliageSky`] to every species' far-tree material, whose
/// uniform's `shading.z` and `shade` the impostor shader reads it from.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_impostor_sky(
    sky: Res<FoliageSky>,
    store: Res<FarTreeStore>,
    mut materials: ResMut<Assets<FarTreeMaterial>>,
) {
    for handle in store.materials() {
        if materials.get(handle).is_some_and(|m| {
            m.extension.uniform.shading.z != sky.strength
                || m.extension.uniform.shading.w != sky.canopy.rim
                || m.extension.uniform.shade.truncate() != sky.shade_tint
        }) && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.uniform.shading.z = sky.strength;
            material.extension.uniform.shading.w = sky.canopy.rim;
            material.extension.uniform.shade = sky.shade_tint.extend(1.0);
        }
    }
}

fn log_stats(
    time: Res<Time>,
    stats: Res<FoliageStats>,
    store: Res<FarTreeStore>,
    grass: Option<Res<GrassAssets>>,
    grass_materials: Res<Assets<GrassMaterial>>,
    mut next: Local<f32>,
) {
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 2.0;
    let (grass_buffer_bytes, grass_live_bytes, grass_holes) = grass.map_or((0, 0, 0), |grass| {
        let store = grass.store.lock().unwrap_or_else(PoisonError::into_inner);
        (store.buffer_bytes(), store.live_bytes(), store.holes())
    });
    info!(
        cells = stats.cells,
        blades = stats.blades,
        plants = stats.trees,
        entities = stats.entities,
        tree_meshes = stats.tree_meshes,
        in_flight = stats.in_flight,
        placed = stats.placed,
        last_place_ms = stats.last_place_ms,
        collect_ms = stats.collect_ms,
        apply_ms = stats.apply_ms,
        upload_ms = stats.upload_ms,
        uploaded_bytes = stats.uploaded_bytes,
        spawned = stats.spawned,
        despawned = stats.despawned,
        cache_hits = stats.cache_hits,
        looked = stats.looked,
        resident_cpu_bytes = stats.resident_cpu_bytes,
        resident_grass_bytes = stats.resident_grass_bytes,
        retiring_grass_bytes = stats.retiring_grass_bytes,
        far_trees = stats.far_trees,
        far_buffer_bytes = store.buffer_bytes(),
        far_live_bytes = store.live_bytes(),
        grass_buffer_bytes,
        grass_live_bytes,
        grass_holes,
        grass_materials = grass_materials.len(),
        "foliage"
    );
}
