//! Studio rendering, orbit controls, persistence and reproducible captures.
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::Path,
};

use anyhow::{Context, Result};
use ashlar::{PieceOrigin, TriangleMesh};
use ashlar_manifold::{ManifoldMesher, mesh_building};
use bevy::{
    camera::{RenderTarget, Viewport},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::PrimaryWindow,
};
use veyra_weapons::{Assembly, Finish, Weapon};

use crate::{Action, Captures, Options, StudioActions, ViewMode, Workbench, materials, ui};

#[derive(Component)]
struct StudioCamera;

#[derive(Component)]
struct WeaponPiece {
    base: Vec3,
    separation: Vec3,
}

#[derive(Resource)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
    home_distance: f32,
    home_pitch: f32,
    exploded_distance: f32,
    view: ViewMode,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: -1.05,
            pitch: 0.24,
            distance: 1.35,
            home_distance: 1.35,
            home_pitch: 0.24,
            exploded_distance: 1.8,
            view: ViewMode::Studio,
        }
    }
}

impl Orbit {
    fn inspect(&mut self, view: ViewMode) {
        self.view = view;
        self.yaw = if view == ViewMode::Studio {
            -1.05
        } else {
            -std::f32::consts::FRAC_PI_2
        };
        self.pitch = match view {
            ViewMode::Studio => self.home_pitch,
            ViewMode::Side => 0.0,
            ViewMode::Top => 1.50,
        };
        self.distance = self.home_distance;
    }
}

#[derive(Resource, Default)]
struct MeshBank(BTreeMap<u64, Handle<Mesh>>);

#[derive(Resource, Default)]
struct Rendered(String);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Orbit>()
        .init_resource::<MeshBank>()
        .init_resource::<Rendered>()
        .insert_resource(ClearColor(Color::srgb(0.065, 0.085, 0.10)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.72, 0.81, 0.93),
            brightness: 220.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (orbit_input, animate_parts, grid))
        .add_systems(PostUpdate, (apply_actions, rebuild).chain())
        .add_systems(PreUpdate, capture.after(bevy::ui::UiSystems::Focus));
}

fn setup(world: &mut World) {
    let options = world.resource::<Options>().clone();
    world.resource_mut::<Orbit>().inspect(options.view);
    let target = if world.resource::<Captures>().cases.is_empty() {
        RenderTarget::default()
    } else {
        let image = world
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                options.width,
                options.height,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
        world.resource_mut::<Captures>().target = Some(image.clone());
        RenderTarget::Image(image.into())
    };
    world.spawn((
        Camera3d::default(),
        StudioCamera,
        bevy::camera::ShadowLodOrigin,
        Camera {
            viewport: Some(viewport(options.width, options.height, 1.0)),
            ..default()
        },
        target.clone(),
        Msaa::Sample4,
        Projection::Perspective(PerspectiveProjection {
            fov: 0.63,
            near: 0.01,
            far: 30.0,
            ..default()
        }),
        Transform::from_xyz(-1.1, 0.35, 0.6).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    world.spawn((
        Camera2d,
        IsDefaultUiCamera,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        target,
    ));
    world.spawn((
        DirectionalLight {
            illuminance: 8500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-2.0, 4.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    for (position, color, intensity) in [
        (
            Vec3::new(-1.3, 1.6, -1.0),
            Color::srgb(0.73, 0.86, 1.0),
            22000.0,
        ),
        (
            Vec3::new(1.5, 0.6, 1.0),
            Color::srgb(1.0, 0.75, 0.48),
            15000.0,
        ),
    ] {
        world.spawn((
            PointLight {
                color,
                intensity,
                range: 8.0,
                radius: 0.6,
                ..default()
            },
            Transform::from_translation(position),
        ));
    }
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(8.0, 8.0));
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.095, 0.12, 0.13),
            perceptual_roughness: 0.72,
            metallic: 0.15,
            ..default()
        });
    world.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, -0.32, 0.0),
    ));
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "window dimensions are bounded positive pixel coordinates"
)]
fn viewport(width: u32, height: u32, scale: f32) -> Viewport {
    let left = (ui::LEFT_WIDTH * scale) as u32;
    let right = (ui::RIGHT_WIDTH * scale) as u32;
    let top = (ui::HEADER_HEIGHT * scale) as u32;
    let bottom = (ui::FOOTER_HEIGHT * scale) as u32;
    Viewport {
        physical_position: UVec2::new(left, top),
        physical_size: UVec2::new(
            width.saturating_sub(left + right).max(1),
            height.saturating_sub(top + bottom).max(1),
        ),
        ..default()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "independent Bevy system resources and input queries"
)]
fn orbit_input(
    time: Res<Time>,
    state: Res<Workbench>,
    mut orbit: ResMut<Orbit>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: Query<(&mut Transform, &mut Camera), With<StudioCamera>>,
    mut actions: ResMut<StudioActions>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        actions.0.push(Action::ResetView);
    }
    if keys.just_pressed(KeyCode::KeyE) {
        actions.0.push(Action::ToggleExploded);
    }
    if keys.just_pressed(KeyCode::Space) {
        actions.0.push(Action::ToggleRotate);
    }
    if state.auto_rotate {
        orbit.view = ViewMode::Studio;
        orbit.yaw += time.delta_secs() * 0.24;
    }
    if let Ok(window) = windows.single() {
        if let Some(cursor) = window.cursor_position() {
            let over_stage = cursor.x > ui::LEFT_WIDTH
                && cursor.x < window.width() - ui::RIGHT_WIDTH
                && cursor.y > ui::HEADER_HEIGHT
                && cursor.y < window.height() - ui::FOOTER_HEIGHT;
            if over_stage {
                if mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right) {
                    if motion.delta != Vec2::ZERO {
                        orbit.view = ViewMode::Studio;
                    }
                    orbit.yaw -= motion.delta.x * 0.007;
                    orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-0.6, 1.25);
                }
                orbit.distance = (orbit.distance * (-scroll.delta.y * 0.09).exp()).clamp(0.35, 4.0);
            }
        }
        if let Ok((_, mut camera)) = camera.single_mut() {
            camera.viewport = Some(viewport(
                window.physical_width(),
                window.physical_height(),
                window.scale_factor(),
            ));
        }
    }
    if let Ok((mut transform, camera)) = camera.single_mut() {
        // The vertical field of view stays fixed. Narrow stages need more
        // distance and a little horizontal margin between the side panels.
        let narrow_stage = camera.viewport.as_ref().map_or(1.0, |viewport| {
            let size = viewport.physical_size.as_vec2();
            (1.1 * size.y / size.x).max(1.0)
        });
        let distance = orbit.distance
            * narrow_stage
            * if state.exploded {
                orbit.exploded_distance / orbit.home_distance
            } else {
                1.0
            };
        let eye = Vec3::new(
            orbit.yaw.sin() * orbit.pitch.cos(),
            orbit.pitch.sin(),
            orbit.yaw.cos() * orbit.pitch.cos(),
        ) * distance;
        *transform = Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Y);
    }
}

fn animate_parts(state: Res<Workbench>, mut pieces: Query<(&WeaponPiece, &mut Transform)>) {
    for (piece, mut transform) in &mut pieces {
        let target = piece.base
            + if state.exploded {
                piece.separation
            } else {
                Vec3::ZERO
            };
        transform.translation = transform.translation.lerp(target, 0.22);
    }
}

fn grid(mut gizmos: Gizmos) {
    for n in -10_i16..=10 {
        let p = f32::from(n) * 0.1;
        let color = Color::srgba(0.25, 0.35, 0.37, if n % 5 == 0 { 0.30 } else { 0.12 });
        gizmos.line(Vec3::new(p, -0.319, -1.0), Vec3::new(p, -0.319, 1.0), color);
        gizmos.line(Vec3::new(-1.0, -0.319, p), Vec3::new(1.0, -0.319, p), color);
    }
}

fn signature(mesh: &TriangleMesh) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for value in mesh
        .positions
        .iter()
        .chain(&mesh.normals)
        .flat_map(ashlar::glam::DVec3::to_array)
    {
        value.to_bits().hash(&mut hash);
    }
    for uv in &mesh.uvs {
        for value in uv {
            value.to_bits().hash(&mut hash);
        }
    }
    mesh.indices.hash(&mut hash);
    hash.finish()
}

fn separation(instance: &str) -> Vec3 {
    match instance {
        "sight" => Vec3::Y * 0.16,
        "feed" => Vec3::NEG_Y * 0.19,
        "fore_end" | "muzzle" | "head" | "edge" => Vec3::Z * 0.22,
        "stock" => Vec3::NEG_Z * 0.18,
        "grip" | "support" => Vec3::NEG_Y * 0.14,
        "cooling" => Vec3::NEG_X * 0.16,
        "power" => Vec3::X * 0.15,
        _ => Vec3::ZERO,
    }
}

fn rebuild(world: &mut World) {
    let state = world.resource::<Workbench>();
    let key = format!(
        "{:?}:{}:{}",
        state.assembly, state.clay, state.compare_stock
    );
    if world.resource::<Rendered>().0 == key {
        return;
    }
    let mut assembly = state.assembly.clone();
    if state.compare_stock {
        assembly.reset_modules();
    }
    let clay = state.clay;
    match prepare(world, &assembly, clay) {
        Ok(()) => world.resource_mut::<Rendered>().0 = key,
        Err(error) => {
            world.resource_mut::<Workbench>().status = format!("Assembly failed: {error:#}");
            world.resource_mut::<Rendered>().0 = key;
            error!("assembly build: {error:#}");
            if !world.resource::<Captures>().cases.is_empty() {
                world.write_message(AppExit::error());
            }
        }
    }
}

fn prepare(world: &mut World, assembly: &Assembly, clay: bool) -> Result<()> {
    assembly.validate()?;
    let building = veyra_weapons::build(assembly)?;
    let meshed = mesh_building(&building, &ManifoldMesher::default())?;
    let palette = materials::palette(world, assembly.weapon.maker(), assembly.finish, clay)?;
    let mut min = ashlar::glam::DVec3::splat(f64::INFINITY);
    let mut max = ashlar::glam::DVec3::splat(f64::NEG_INFINITY);
    let mut exploded_min = min;
    let mut exploded_max = max;
    for piece in meshed.pieces() {
        let offset = match piece.origin {
            PieceOrigin::Element { instance, .. } => separation(instance).as_dvec3(),
            _ => ashlar::glam::DVec3::ZERO,
        };
        for point in &piece.mesh.positions {
            let point = piece.pose.transform_point(*point);
            min = min.min(point);
            max = max.max(point);
            exploded_min = exploded_min.min(point + offset);
            exploded_max = exploded_max.max(point + offset);
        }
    }
    anyhow::ensure!(
        min.is_finite() && max.is_finite(),
        "empty procedural assembly"
    );
    let center = ((min + max) * 0.5).as_vec3();
    let exploded_center = ((exploded_min + exploded_max) * 0.5).as_vec3() - center;
    let mut ready = Vec::new();
    let mut triangles = 0;
    for piece in meshed.pieces() {
        let hash = signature(piece.mesh);
        let cached = world.resource::<MeshBank>().0.get(&hash).cloned();
        let mesh = if let Some(mesh) = cached {
            mesh
        } else {
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(ashlar_bevy::mesh(piece.mesh)?);
            world
                .resource_mut::<MeshBank>()
                .0
                .insert(hash, mesh.clone());
            mesh
        };
        let material = palette
            .get(&piece.binding.material)
            .with_context(|| format!("missing material {}", piece.binding.material))?
            .clone();
        let instance = match piece.origin {
            PieceOrigin::Element { instance, .. } => instance,
            _ => "chassis",
        };
        let base = piece.pose.translation.as_vec3() - center;
        let transform = Transform {
            translation: base,
            rotation: piece.pose.rotation.as_quat(),
            ..default()
        };
        triangles += piece.mesh.indices.len() / 3;
        ready.push((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            WeaponPiece {
                base,
                separation: separation(instance) - exploded_center,
            },
        ));
    }
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<WeaponPiece>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    let count = ready.len();
    for entity in ready {
        world.spawn(entity);
    }
    let distance = ((max - min).as_vec3().length() * 1.6).clamp(0.65, 2.3);
    let mut orbit = world.resource_mut::<Orbit>();
    if (orbit.home_distance - distance).abs() > 0.15 {
        orbit.distance = distance;
    }
    orbit.home_distance = distance;
    orbit.exploded_distance = ((exploded_max - exploded_min).as_vec3().length() * 1.8).max(0.9);
    let pitch = if matches!(assembly.weapon, Weapon::FieldBlade | Weapon::HeavyCutter) {
        0.75
    } else {
        0.24
    };
    if orbit.view == ViewMode::Studio && (orbit.home_pitch - pitch).abs() > f32::EPSILON {
        orbit.pitch = pitch;
    }
    orbit.home_pitch = pitch;
    let mut state = world.resource_mut::<Workbench>();
    state.mesh_count = count;
    state.triangle_count = triangles;
    Ok(())
}

fn save_path(world: &World) -> std::path::PathBuf {
    let options = world.resource::<Options>();
    options
        .config
        .clone()
        .unwrap_or_else(|| options.output.join("assembly.ron"))
}

fn write_file(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let pending = path.with_extension("tmp");
    std::fs::write(&pending, text)?;
    std::fs::rename(pending, path)?;
    Ok(())
}

fn apply_actions(world: &mut World) {
    let actions = std::mem::take(&mut world.resource_mut::<StudioActions>().0);
    for action in actions {
        let result = apply(world, action);
        world.resource_mut::<Workbench>().revision += 1;
        if let Err(error) = result {
            world.resource_mut::<Workbench>().status = format!("{error:#}");
            if world.resource::<Options>().exercise {
                error!("UI exercise action failed: {error:#}");
                world.write_message(AppExit::error());
            }
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one explicit branch per editor action keeps mutations auditable"
)]
fn apply(world: &mut World, action: Action) -> Result<()> {
    match action {
        Action::Maker(maker) => {
            world.resource_mut::<Workbench>().maker_filter = maker;
        }
        Action::Preset(id) => {
            let preset = veyra_weapons::presets::PRESETS
                .iter()
                .find(|preset| preset.id == id)
                .with_context(|| format!("unknown preset {id}"))?;
            let finish = world.resource::<Workbench>().assembly.finish;
            let assembly = preset.assembly(finish)?;
            let mut state = world.resource_mut::<Workbench>();
            state.assembly = assembly;
            state.compare_stock = false;
            state.status = format!("{}: {}", preset.name, preset.description);
        }
        Action::ToggleCompare => {
            let mut state = world.resource_mut::<Workbench>();
            state.compare_stock = !state.compare_stock;
            state.status = if state.compare_stock {
                "Stock reference; fitted selections retained."
            } else {
                "Fitted assembly restored."
            }
            .into();
        }
        Action::ViewSide | Action::ViewTop => {
            let mut orbit = world.resource_mut::<Orbit>();
            orbit.view = if matches!(action, Action::ViewTop) {
                ViewMode::Top
            } else {
                ViewMode::Side
            };
            orbit.yaw = -std::f32::consts::FRAC_PI_2;
            orbit.pitch = if matches!(action, Action::ViewTop) {
                1.50
            } else {
                0.0
            };
            orbit.distance = orbit.home_distance;
            world.resource_mut::<Workbench>().auto_rotate = false;
        }
        Action::Weapon(weapon) => {
            let mut state = world.resource_mut::<Workbench>();
            state.assembly = Assembly {
                weapon,
                finish: state.assembly.finish,
                ..Assembly::default()
            };
            state.compare_stock = false;
            state.status = format!("{} / stock assembly", weapon.label());
        }
        Action::Module(slot, id) => {
            let mut state = world.resource_mut::<Workbench>();
            state.assembly.select(slot, id.as_deref())?;
            state.compare_stock = false;
            state.status = format!(
                "{}: {}",
                slot.label(),
                state.assembly.selected(slot).map_or("Stock", |m| m.name)
            );
        }
        Action::Finish(finish) => {
            let mut state = world.resource_mut::<Workbench>();
            state.assembly.finish = finish;
            state.status = format!("{} finish", finish.label());
        }
        Action::ToggleExploded => {
            let mut state = world.resource_mut::<Workbench>();
            state.exploded = !state.exploded;
        }
        Action::ToggleRotate => {
            let mut state = world.resource_mut::<Workbench>();
            state.auto_rotate = !state.auto_rotate;
        }
        Action::ToggleClay => {
            let mut state = world.resource_mut::<Workbench>();
            state.clay = !state.clay;
        }
        Action::ResetView => {
            let distance = world.resource::<Orbit>().home_distance;
            let pitch = world.resource::<Orbit>().home_pitch;
            let exploded_distance = world.resource::<Orbit>().exploded_distance;
            *world.resource_mut::<Orbit>() = Orbit {
                distance,
                home_distance: distance,
                pitch,
                home_pitch: pitch,
                exploded_distance,
                ..Orbit::default()
            };
        }
        Action::ResetBuild => {
            let mut state = world.resource_mut::<Workbench>();
            state.assembly.reset_modules();
            state.compare_stock = false;
            state.assembly.finish = Finish::Issued;
            state.status = "Stock components restored.".into();
        }
        Action::Save => {
            let path = save_path(world);
            let text = ron::ser::to_string_pretty(
                &world.resource::<Workbench>().assembly,
                ron::ser::PrettyConfig::default(),
            )?;
            write_file(&path, &text)?;
            world.resource_mut::<Workbench>().status = format!("Saved {}", path.display());
        }
        Action::Load => {
            let path = save_path(world);
            let assembly = crate::read_assembly(&path)?;
            let mut state = world.resource_mut::<Workbench>();
            state.maker_filter = Some(assembly.weapon.maker());
            state.compare_stock = false;
            state.assembly = assembly;
            state.status = format!("Loaded {}", path.display());
        }
        Action::Export => {
            let directory = world.resource::<Options>().output.clone();
            let assembly = world.resource::<Workbench>().assembly.clone();
            crate::export::export(world, &assembly, &directory)?;
            world.resource_mut::<Workbench>().status =
                format!("Exported OBJ and ashlar recipe to {}", directory.display());
        }
    }
    Ok(())
}

fn capture(world: &mut World) {
    if world.resource::<Captures>().cases.is_empty() || world.resource::<Captures>().pending {
        return;
    }
    world.resource_mut::<Captures>().frames += 1;
    if world.resource::<Captures>().frames < 24 {
        return;
    }
    if world.resource::<Options>().exercise && !exercise(world) {
        return;
    }
    let target = world
        .resource::<Captures>()
        .target
        .clone()
        .expect("capture has an image target");
    let path = world
        .resource::<Captures>()
        .cases
        .front()
        .expect("checked case")
        .path
        .clone();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        error!("capture directory: {error}");
        world.write_message(AppExit::error());
        return;
    }
    world.resource_mut::<Captures>().pending = true;
    world.spawn(Screenshot::image(target)).observe(
        move |event: On<ScreenshotCaptured>,
              mut captures: ResMut<Captures>,
              mut state: ResMut<Workbench>,
              mut exit: MessageWriter<AppExit>| {
            let result = event
                .image
                .clone()
                .try_into_dynamic()
                .map_err(anyhow::Error::from)
                .and_then(|image| image.save(&path).map_err(anyhow::Error::from));
            if let Err(error) = result {
                error!("capture failed: {error}");
                exit.write(AppExit::error());
                return;
            }
            println!("saved {}", path.display());
            captures.cases.pop_front();
            captures.pending = false;
            captures.frames = 0;
            if let Some(next) = captures.cases.front() {
                state.assembly.clone_from(&next.assembly);
                state.maker_filter = Some(next.assembly.weapon.maker());
                state.compare_stock = false;
                state.exploded = next.exploded;
                state.status = format!("{} / procedural assembly", state.assembly.weapon.label());
                state.revision += 1;
            } else {
                exit.write(AppExit::Success);
            }
        },
    );
}

fn exercise(world: &mut World) -> bool {
    let sequence = [
        Action::Maker(Some(veyra_weapons::Maker::Vigil)),
        Action::Weapon(Weapon::PatrolCarbine),
        Action::Preset("vigil_recon".into()),
        Action::Finish(Finish::Field),
        Action::ToggleCompare,
        Action::ToggleCompare,
        Action::ViewSide,
        Action::ToggleExploded,
        Action::Save,
        Action::ResetBuild,
        Action::Load,
        Action::Export,
    ];
    let step = world.resource::<Captures>().exercise_step;
    if let Some(expected) = sequence.get(step) {
        let button = world
            .query::<(Entity, &ui::ButtonAction)>()
            .iter(world)
            .find_map(|(entity, button)| (&button.0 == expected).then_some(entity));
        if let Some(button) = button {
            world.entity_mut(button).insert(Interaction::Pressed);
            let mut captures = world.resource_mut::<Captures>();
            captures.exercise_step += 1;
            captures.frames = 0;
            println!(
                "UI exercise {} / {}: {expected:?}",
                step + 1,
                sequence.len()
            );
        } else {
            error!("UI exercise could not find {expected:?}");
            world.write_message(AppExit::error());
        }
        return false;
    }
    let expected = veyra_weapons::presets::PRESETS
        .iter()
        .find(|preset| preset.id == "vigil_recon")
        .expect("catalogue includes recon preset")
        .assembly(Finish::Field)
        .expect("authored recon fits");
    let state = world.resource::<Workbench>();
    if state.assembly != expected || !state.exploded || state.compare_stock {
        error!("UI exercise did not preserve the saved fitted assembly");
        world.write_message(AppExit::error());
        return false;
    }
    true
}
