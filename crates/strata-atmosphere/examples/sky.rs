//! A windowed check of the sky: a plane, a few blocks for shadows, the
//! camera at eye height, at a chosen time of day.
//!
//! ```text
//! cargo run -p strata-atmosphere --example sky -- --time 0.5 --frames 90 --screenshot noon.png
//! cargo run -p strata-atmosphere --example sky -- --time 0.77 --screenshot dusk.png
//! ```

use std::path::PathBuf;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use clap::Parser;
use strata_app::DayStart;
use strata_atmosphere::AtmospherePlugin;

#[derive(Parser, Resource, Debug, Clone)]
struct Cli {
    /// Time of day in [0, 1): 0 midnight, 0.25 sunrise, 0.5 noon, 0.75 sunset.
    #[arg(long, default_value_t = 0.5)]
    time: f64,
    /// Exercise live preset and prepass transitions every 90 frames.
    #[arg(long)]
    cycle_looks: bool,
    /// Capture on this frame (and exit if a screenshot was asked for).
    #[arg(long, default_value_t = 90)]
    frames: u32,
    /// Save the frame here and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
}

fn main() -> AppExit {
    let cli = Cli::parse();
    App::new()
        .add_plugins((DefaultPlugins, strata_app::AppPlugin, AtmospherePlugin))
        .insert_resource(DayStart(cli.time))
        .insert_resource(cli)
        .init_resource::<ScreenshotDone>()
        .add_systems(Startup, setup)
        .add_systems(Update, (cycle_looks, screenshot_then_exit))
        .run()
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 60_000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 3.0, 40.0).looking_at(Vec3::new(0.0, 6.0, -60.0), Vec3::Y),
    ));
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.4, 0.25),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40_000.0, 40_000.0))),
        MeshMaterial3d(ground),
        Transform::IDENTITY,
    ));
    let block = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.55, 0.5),
        perceptual_roughness: 0.7,
        ..default()
    });
    let cube = meshes.add(Cuboid::new(6.0, 12.0, 6.0));
    for (x, z) in [(-20.0, -30.0), (0.0, -60.0), (25.0, -40.0), (-40.0, -120.0)] {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(block.clone()),
            Transform::from_xyz(x, 6.0, z),
        ));
    }
    // A distant ridge of tall blocks to see aerial perspective work.
    let far = meshes.add(Cuboid::new(600.0, 400.0, 200.0));
    for i in 0u8..12 {
        let x = -6_000.0 + f32::from(i) * 1_100.0;
        commands.spawn((
            Mesh3d(far.clone()),
            MeshMaterial3d(block.clone()),
            Transform::from_xyz(x, 200.0, -9_000.0 - f32::from(i % 3) * 3_000.0),
        ));
    }
}

#[derive(Resource, Default)]
struct ScreenshotDone(bool);

fn screenshot_then_exit(
    mut commands: Commands,
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    done: Res<ScreenshotDone>,
    mut requested: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
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
    if *requested || frames.0 < cli.frames {
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

// Run with --cycle-looks --frames 900 to cover removal and re-enabling.
fn cycle_looks(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    mut sky: ResMut<strata_atmosphere::SkySettings>,
    mut look: ResMut<strata_atmosphere::look::RenderLook>,
) {
    if !cli.cycle_looks || frames.0 == 0 || !frames.0.is_multiple_of(90) {
        return;
    }
    let stage = (frames.0 / 90) % 8;
    let name = match stage {
        1 | 7 => "cinematic-woodland",
        3..=6 => "clear-contrast",
        _ => "baseline",
    };
    let mut file = strata_atmosphere::look::LookFile::preset(name).expect("known preset");
    match stage {
        4 => file.look.ssao = false,
        5 => {
            file.look.taa = false;
            file.look.motion_blur = 0.25;
        }
        6 => file.look.taa = false,
        _ => {}
    }
    info!(stage, name, "live render transition");
    *sky = file.sky;
    *look = file.look;
}
