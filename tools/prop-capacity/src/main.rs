//! The Phase 8 capacity test: can the runtime carry baked 15k-triangle trees
//! at forest density?
//!
//! ```text
//! just content --capacity
//! cargo run --release -p prop-capacity -- --frames 2000
//! ```
//!
//! It loads every synthetic tree `just content --capacity` wrote through the
//! real `.ashprop` loader, then plants them on a flat ground out to the
//! farthest mesh level at the woodland fixture's density, the way the forest
//! will draw them: shared meshes, one entity per piece per level, each level
//! in its `VisibilityRange` band, and only the nearest level casting shadows.
//! It logs how long loading took, how many bytes of mesh it holds, the entity
//! count, and the frame and GPU time from then on.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "planting-grid indices, hash bits and frame counts, all small and exact"
)]
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ashlar_bevy::prelude::{
    AshlarPlugin, ImpostorMaterial, MaterialLibraryAsset, PropAsset, impostor_material,
    impostor_quad, load_impostor_images,
};
use ashlar_bevy::prop::prop_level_range;
use bevy::asset::io::AssetSourceBuilder;
use bevy::diagnostic::{DiagnosticsStore, FrameCount};
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PresentMode, WindowResolution};
use bevy::winit::WinitSettings;
use clap::Parser;
use veyra_flora::capacity::{SPECIES, VARIANTS, path};

const WORKSPACE_GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");

#[derive(Parser, Resource, Debug, Clone)]
struct Cli {
    /// Where `just content --capacity` wrote the fixture.
    #[arg(long, default_value = WORKSPACE_GENERATED)]
    generated: PathBuf,
    /// Plant trees out to this many metres from the camera: past the last
    /// mesh level (350 m) they are impostors.
    #[arg(long, default_value_t = 1600.0)]
    radius: f32,
    /// Metres between trees on the planting grid, jittered: 9 is the oak's
    /// spacing, which the woodland fixture is made of.
    #[arg(long, default_value_t = 9.0)]
    spacing: f32,
    /// Exit after this many frames past loading.
    #[arg(long)]
    frames: Option<u32>,
    /// Save the last frame here.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size, `WxH`.
    #[arg(long, default_value = "2560x1440")]
    window: String,
}

#[derive(Resource)]
struct Fixture {
    props: Vec<Handle<PropAsset>>,
    library: Handle<MaterialLibraryAsset>,
    started: Instant,
    planted: Option<u32>,
    frames: Vec<f64>,
}

fn main() -> AppExit {
    let cli = Cli::parse();
    let (width, height) = cli
        .window
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((2560, 1440));
    let mut app = App::new();
    app.register_asset_source(
        "generated",
        AssetSourceBuilder::platform_default(&cli.generated.to_string_lossy(), None),
    );
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "prop capacity".into(),
                resolution: WindowResolution::new(width, height),
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }),
        AshlarPlugin::default(),
        RenderDiagnosticsPlugin,
    ))
    .insert_resource(WinitSettings::continuous())
    .insert_resource(cli)
    .add_systems(Startup, setup)
    .add_systems(Update, (plant, measure).chain());
    app.run()
}

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let props = (0..SPECIES)
        .flat_map(|species| (0..VARIANTS).map(move |variant| (species, variant)))
        .map(|(species, variant)| server.load(format!("generated://{}", path(species, variant))))
        .collect();
    commands.insert_resource(Fixture {
        props,
        library: server.load("generated://materials/library.materials.ron"),
        started: Instant::now(),
        planted: None,
        frames: Vec::new(),
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.30, 0.27, 0.18))),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(40.0, 60.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 120.0,
            maximum_distance: 2000.0,
            ..default()
        }
        .build(),
    ));
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_xyz(0.0, 1.8, 0.0).looking_at(Vec3::new(10.0, 1.6, -30.0), Vec3::Y),
    ));
}

/// Once every prop and the library are in, plant the forest.
#[allow(
    clippy::too_many_arguments,
    reason = "one system reads every asset it plants from"
)]
fn plant(
    mut commands: Commands,
    mut fixture: ResMut<Fixture>,
    props: Res<Assets<PropAsset>>,
    libraries: Res<Assets<MaterialLibraryAsset>>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impostors: ResMut<Assets<ImpostorMaterial>>,
    cli: Res<Cli>,
) {
    if fixture.planted.is_some() {
        return;
    }
    let Some(library) = libraries.get(&fixture.library) else {
        return;
    };
    let loaded: Option<Vec<&PropAsset>> = fixture.props.iter().map(|h| props.get(h)).collect();
    let Some(loaded) = loaded else {
        return;
    };
    let load_time = fixture.started.elapsed();
    let bytes = mesh_bytes(&loaded, &meshes);
    let mut dressed = std::collections::BTreeMap::new();
    for key in [veyra_flora::capacity::BARK, veyra_flora::capacity::LEAF] {
        let definition = &library.0.materials[key];
        dressed.insert(
            key,
            materials.add(ashlar_bevy::create_material(definition, &server)),
        );
    }
    // One quad and one material per impostor atlas, shared by every tree
    // that wears it.
    let mut drawn_impostors = std::collections::BTreeMap::new();
    let mut trees = 0_u32;
    let mut entities = 0_u32;
    let cells = (cli.radius / cli.spacing).ceil() as i32;
    for i in -cells..=cells {
        for j in -cells..=cells {
            let h = hash(i, j);
            let x = (i as f32 + unit(h) - 0.5) * cli.spacing;
            let z = (j as f32 + unit(h >> 16) - 0.5) * cli.spacing;
            if x * x + z * z > cli.radius * cli.radius || x * x + z * z < 4.0 {
                continue;
            }
            let asset = loaded[(h >> 32) as usize % loaded.len()];
            let transform = Transform::from_xyz(x, 0.0, z)
                .with_rotation(Quat::from_rotation_y(
                    unit(h >> 40) * core::f32::consts::TAU,
                ))
                .with_scale(Vec3::splat(0.85 + 0.3 * unit(h >> 48)));
            let root = commands.spawn((transform, Visibility::default())).id();
            for (index, level) in asset.prop.levels.iter().enumerate() {
                if let Some(impostor) = &level.impostor {
                    let (quad, material) = drawn_impostors
                        .entry(impostor.albedo.clone())
                        .or_insert_with(|| {
                            // The loader has already qualified the atlas keys
                            // with the source the prop came from.
                            let (albedo, normal_depth) = load_impostor_images(
                                &server,
                                impostor.albedo.clone(),
                                impostor.normal_depth.clone(),
                            );
                            (
                                meshes.add(impostor_quad(&impostor.layout)),
                                impostors.add(impostor_material(
                                    &impostor.layout,
                                    albedo,
                                    normal_depth,
                                )),
                            )
                        })
                        .clone();
                    let mut entity = commands.spawn((
                        Mesh3d(quad),
                        MeshMaterial3d(material),
                        Transform::default(),
                        NotShadowCaster,
                        ChildOf(root),
                    ));
                    if let Some(range) = prop_level_range(&asset.prop, index, 0.1) {
                        entity.insert(range);
                    }
                    entities += 1;
                    continue;
                }
                for (piece, mesh) in level.pieces.iter().zip(&asset.meshes[index]) {
                    let mut entity = commands.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(dressed[piece.material.as_str()].clone()),
                        Transform::default(),
                        ChildOf(root),
                    ));
                    if let Some(range) = prop_level_range(&asset.prop, index, 0.1) {
                        entity.insert(range);
                    }
                    if !level.cast_shadows {
                        entity.insert(NotShadowCaster);
                    }
                    entities += 1;
                }
            }
            trees += 1;
        }
    }
    info!(
        "capacity: loaded {} props in {load_time:.2?}, {:.1} MB of mesh; planted {trees} trees as {entities} entities",
        loaded.len(),
        bytes as f64 / 1e6
    );
    fixture.planted = Some(0);
}

/// Frame and GPU time after planting; a summary on exit.
#[allow(
    clippy::too_many_arguments,
    reason = "one system measures, captures and exits"
)]
fn measure(
    mut commands: Commands,
    mut fixture: ResMut<Fixture>,
    time: Res<Time<Real>>,
    diagnostics: Res<DiagnosticsStore>,
    cli: Res<Cli>,
    frame: Res<FrameCount>,
    mut exit: MessageWriter<AppExit>,
    mut gpu: Local<Vec<f64>>,
) {
    let Some(frames) = fixture.planted else {
        return;
    };
    // Two seconds for uploads and pipelines before anything counts.
    if frames > 0 || time.delta() < Duration::from_secs(1) {
        fixture.frames.push(time.delta_secs_f64() * 1000.0);
        // Every top-level render pass's GPU time, summed: without the
        // atmosphere plugin there is no single camera-frame span. Sub-spans
        // are nested under their parent and would count twice.
        let ms: f64 = diagnostics
            .iter()
            .filter(|d| {
                let path = d.path().as_str();
                path.starts_with("render/")
                    && path.ends_with("/elapsed_gpu")
                    && path.matches('/').count() == 2
            })
            .filter_map(bevy::diagnostic::Diagnostic::smoothed)
            .sum();
        if ms > 0.0 {
            gpu.push(ms);
        }
    }
    fixture.planted = Some(frames + 1);
    let done = cli.frames.is_some_and(|limit| frames + 1 >= limit);
    if frames + 1 == cli.frames.unwrap_or(u32::MAX).saturating_sub(60)
        && let Some(path) = &cli.screenshot
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
    }
    if done {
        let skip = fixture.frames.len().min(240);
        let mut sorted = fixture.frames[skip..].to_vec();
        sorted.sort_by(f64::total_cmp);
        let rank = |p: usize| sorted[(sorted.len() * p).div_ceil(100).max(1) - 1];
        let gpu_mean = if gpu.is_empty() {
            0.0
        } else {
            gpu.iter().sum::<f64>() / gpu.len() as f64
        };
        info!(
            "capacity: {} frames at frame {}: p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, gpu {gpu_mean:.2} ms",
            sorted.len(),
            frame.0,
            rank(50),
            rank(95),
            rank(99),
        );
        exit.write(AppExit::Success);
    }
}

/// Vertex and index bytes of every mesh the props hold: what the GPU keeps.
fn mesh_bytes(loaded: &[&PropAsset], meshes: &Assets<Mesh>) -> usize {
    loaded
        .iter()
        .flat_map(|asset| asset.meshes.iter().flatten())
        .filter_map(|handle| meshes.get(handle))
        .map(|mesh| {
            mesh.get_vertex_buffer_size() + mesh.get_index_buffer_bytes().map_or(0, <[u8]>::len)
        })
        .sum()
}

fn hash(i: i32, j: i32) -> u64 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ (h >> 29)
}

fn unit(h: u64) -> f32 {
    (h & 0xFFFF) as f32 / 65_536.0
}
