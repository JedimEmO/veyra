//! A windowed check of `TerrainMaterial`: a rolling heightfield with a
//! ridge, the layer pair painted as a gradient across it.
//!
//! ```text
//! cargo run -p strata-terrain --example material_plane -- --frames 90 --screenshot shot.png
//! ```
//!
//! Every layer appears in order left to right, blended at the band edges,
//! and the ridge in the middle turns to rock where it steepens. Lighting is
//! a plain directional light with shadows, no atmosphere, so what is seen
//! is the material alone.

// Grid indices and band numbers are tiny; the casts between them and f32
// are exact, and this is an example.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::diagnostic::FrameCount;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use clap::Parser;
use strata_core::{GRID_N, RegionCoord};
use strata_sim::{Registries, SimPlugin, WorldLoadPlugin};
use strata_terrain::material::{
    MAX_LAYERS, TerrainMaterial, TerrainMaterialHandle, TerrainMaterialPlugin, dev_asset_root,
};
use strata_terrain::mesh::RegionMesh;
use strata_terrain::store::{TerrainStore, pack_region};

#[derive(Parser, Resource, Debug, Clone)]
struct Cli {
    /// Capture on this frame (and exit if a screenshot was asked for).
    #[arg(long, default_value_t = 90)]
    frames: u32,
    /// Save the frame here and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Stand on the ground instead of the overview, to see the maps at
    /// their real scale.
    #[arg(long)]
    close: bool,
}

fn main() -> AppExit {
    let cli = Cli::parse();
    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin {
                file_path: dev_asset_root().to_string_lossy().into_owned(),
                ..default()
            }),
            // The material's layers come from the terrain material
            // registry, so the real registries are loaded.
            SimPlugin,
            WorldLoadPlugin::new(dev_asset_root()),
            TerrainMaterialPlugin,
        ))
        .insert_resource(cli)
        .init_resource::<ScreenshotDone>()
        .add_systems(Startup, setup)
        .add_systems(Update, (spawn_terrain, screenshot_then_exit))
        .run()
}

/// The patch is one region of this level: 256 m a side at 4 m a step.
const LOD: u8 = 2;
/// Half-size of the patch in metres.
const HALF: f32 = 128.0;
/// Vertices per side: a region's grid.
const N: usize = GRID_N;

fn height(x: f32, z: f32) -> f32 {
    let u = (x - 30.0) / 18.0;
    let ridge = 30.0 * (-u * u).exp() * (1.0 + 0.15 * (z / 9.0).sin());
    ridge + 3.0 * (x / 11.0).sin() * (z / 13.0).cos()
}

fn normal(x: f32, z: f32) -> Vec3 {
    let e = 0.25;
    let dx = (height(x + e, z) - height(x - e, z)) / (2.0 * e);
    let dz = (height(x, z + e) - height(x, z - e)) / (2.0 * e);
    Vec3::new(-dx, 1.0, -dz).normalize()
}

/// Layer shares by band along z: this band's layer fades into the next's
/// from the band's start to its end.
fn splat(z: f32, layers: usize) -> [u8; MAX_LAYERS] {
    let t = ((z + HALF) / (2.0 * HALF)).clamp(0.0, 0.999) * layers as f32;
    let band = t.floor();
    let a = band as usize;
    let b = (a + 1).min(layers - 1);
    let mut shares = [0u8; MAX_LAYERS];
    let into_b = ((t - band) * 255.0).round() as u8;
    shares[a] = 255 - into_b;
    shares[b] += into_b;
    shares
}

/// The patch as a region the store packs: the grid alone, as the store
/// reads no more (its skirts are hung by the shader, and its indices are
/// the shared mesh's).
fn build_region(layers: usize, rock: usize) -> RegionMesh {
    let step = 2.0 * HALF / (N as f32 - 1.0);
    let vertices = N * N;
    let mut mesh = RegionMesh {
        coord: RegionCoord::new(0, 0, LOD),
        positions: Vec::with_capacity(vertices),
        normals: Vec::with_capacity(vertices),
        uvs: Vec::with_capacity(vertices),
        weights: Vec::with_capacity(vertices),
        slope: Vec::with_capacity(vertices),
        morph: Vec::with_capacity(vertices),
        tints: Vec::with_capacity(vertices),
        far_factors: vec![[1.0; 3]; vertices],
        covers: vec![[0.0; 4]; vertices],
        // The whole sky, seen straight up.
        sky: vec![[255, 128, 128, 0]; vertices],
        sky_parent: vec![[255, 128, 128, 0]; vertices],
        indices: Vec::new(),
        min_height: f32::INFINITY,
        max_height: f32::NEG_INFINITY,
        skirt_depth: 1.0,
    };
    for j in 0..N {
        for i in 0..N {
            let x = -HALF + i as f32 * step;
            let z = -HALF + j as f32 * step;
            let h = height(x, z);
            let n = normal(x, z);
            mesh.min_height = mesh.min_height.min(h);
            mesh.max_height = mesh.max_height.max(h);
            mesh.positions.push([i as f32 * step, h, j as f32 * step]);
            mesh.normals.push(n.to_array());
            mesh.uvs.push([i as f32 * step, j as f32 * step]);
            mesh.weights.push(splat(z, layers));
            // The rock alone (a pair of one layer), and the vertex's corner
            // of its triangles, which this grid shares with the mesher's.
            mesh.slope.push([
                strata_terrain::mesh::pack_rock_pair(rock as u8, rock as u8),
                255,
                0,
                strata_terrain::mesh::corner_of(i, j),
            ]);
            // The vertex is its own parent: the patch never morphs.
            mesh.morph.push([h, n.x, n.z, 2.0 * HALF]);
            // White, and a 35 degree slope threshold.
            mesh.tints.push([1.0, 1.0, 1.0, 0.18]);
        }
    }
    mesh
}

fn setup(mut commands: Commands, cli: Res<Cli>) {
    let camera = if cli.close {
        // Eye height on the snow band, looking along the ridge so the
        // gravel, snow and rock bands and the slope blend are all in frame.
        Transform::from_xyz(-20.0, height(-20.0, 55.0) + 2.0, 55.0)
            .looking_at(Vec3::new(30.0, 6.0, 5.0), Vec3::Y)
    } else {
        Transform::from_xyz(-30.0, 28.0, 125.0).looking_at(Vec3::new(15.0, 6.0, 0.0), Vec3::Y)
    };
    commands.spawn((Camera3d::default(), camera));
    commands.spawn((
        DirectionalLight {
            // Against the default exposure this reads as a bright day.
            illuminance: 2_500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.3, -0.9, 0.0)),
    ));
}

/// Spawns the patch once the material exists: one region written to the
/// store and drawn through the shared mesh, as the game draws its terrain.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn spawn_terrain(
    mut commands: Commands,
    material: Option<Res<TerrainMaterialHandle>>,
    registries: Option<Res<Registries>>,
    mut store: ResMut<TerrainStore>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut spawned: Local<bool>,
) {
    // The material is the store's first page's from here; it is waited for
    // because the store copies it.
    let (Some(_), Some(registries)) = (material, registries) else {
        return;
    };
    if *spawned {
        return;
    }
    *spawned = true;
    let layers = registries.0.materials.len();
    let rock = registries
        .0
        .material_id(&strata_registry::Namespaced::parse("strata:rock").expect("valid"))
        .map_or(0, |id| id.layer() as usize);
    let region = build_region(layers, rock);
    let (lo, hi) = region.bounds();
    let packed = pack_region(&region);
    let (Some(placed), Some(mesh)) = (
        store.install(&packed, &device, &queue, Some(&mut materials)),
        store.mesh().cloned(),
    ) else {
        error!("the patch did not fit the terrain store");
        return;
    };
    commands.spawn((
        Mesh3d(mesh),
        MeshTag(placed.tag),
        placed.slot,
        MeshMaterial3d(placed.material),
        Aabb::from_min_max(Vec3::from_array(lo), Vec3::from_array(hi)),
        NoAutoAabb,
        Transform::from_xyz(-HALF, 0.0, -HALF),
    ));
}

#[derive(Resource, Default)]
struct ScreenshotDone(bool);

/// Frames between the material appearing and the capture, so the mesh has
/// been uploaded and drawn.
const SETTLE_FRAMES: u32 = 30;

/// Same shape as apps/game: capture on the requested frame, but not before
/// the material has been up for a while, and exit when the file is on disk.
// A Bevy system's parameters are its dependencies, not an API to tidy.
#[allow(clippy::too_many_arguments)]
fn screenshot_then_exit(
    mut commands: Commands,
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    done: Res<ScreenshotDone>,
    material: Option<Res<TerrainMaterialHandle>>,
    mut ready_at: Local<Option<u32>>,
    mut requested: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if material.is_some() && ready_at.is_none() {
        *ready_at = Some(frames.0);
    }
    let Some(path) = cli.screenshot.clone() else {
        if frames.0 >= cli.frames {
            exit.write(AppExit::Success);
        }
        return;
    };
    if done.0 {
        exit.write(AppExit::Success);
        return;
    }
    let Some(ready) = *ready_at else {
        return;
    };
    if *requested || frames.0 < cli.frames || frames.0 < ready + SETTLE_FRAMES {
        return;
    }
    *requested = true;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    info!("capturing {}", path.display());
    let mut save = save_to_disk(path);
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut done: ResMut<ScreenshotDone>| {
            save(captured);
            done.0 = true;
        },
    );
}
