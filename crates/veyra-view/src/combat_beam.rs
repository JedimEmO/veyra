//! The marksman's aim made readable at range: a thin beam from its muzzle
//! to the point it aims at while it charges, faint while the aim follows
//! the target and bright and still for the lock, so a player who watches
//! it can step out of the line and one who stands in it was warned.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use veyra_sim::combat::{CombatIntent, Combatant};
use veyra_sim::{RenderOrigin, WorldPosition};

use strata_client::ClientSet;

pub use veyra_sim::view::AimBeam;

/// How far the beam's origin sits above the actor's centre and in front of
/// it, the charge glow's own muzzle.
const MUZZLE_UP: f32 = 0.45;
const MUZZLE_FORWARD: f32 = 0.6;
/// The beam's width on the screen, in pixels, at every point along it: a
/// faint one while the aim follows and a thicker one for the lock.
const FAINT_PIXELS: f32 = 1.5;
const LOCK_PIXELS: f32 = 2.5;
/// What a camera that cannot say sees: the combat view's field of view and a
/// 1080 line screen.
const DEFAULT_FOV: f32 = std::f32::consts::FRAC_PI_4;
const DEFAULT_HEIGHT_PX: f32 = 1080.;

#[derive(Resource)]
struct BeamAssets {
    faint: Handle<StandardMaterial>,
    lock: Handle<StandardMaterial>,
}

/// One beam: the actor it belongs to and its own mesh, rebuilt every frame
/// because its width follows the distance to the camera at each end.
#[derive(Component)]
struct BeamView {
    owner: Entity,
    mesh: Handle<Mesh>,
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(Startup, load_beam)
        .add_systems(Update, draw_beam.in_set(ClientSet::Hud));
}

fn load_beam(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let hostile = Color::srgb(1., 0.65, 0.2);
    commands.insert_resource(BeamAssets {
        faint: materials.add(StandardMaterial {
            base_color: hostile.with_alpha(0.35),
            emissive: hostile.to_linear() * 1.5,
            unlit: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        lock: materials.add(StandardMaterial {
            base_color: hostile,
            emissive: hostile.to_linear() * 6.,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
    });
}

/// Whether the charge has reached the lock: the aim no longer follows.
fn locked(charge: f32) -> bool {
    veyra_sim::combat_tables::current()
        .opponents
        .marksman
        .locked(charge)
}

/// A four sided prism along `-Z` from the muzzle (z = 0) to the aim point
/// (z = -length), `near` and `far` wide at each end. A cuboid could not
/// hold a pixel's width from a metre to sixty.
fn prism(near: f32, far: f32, length: f32) -> Mesh {
    let corners = |half: f32, z: f32| {
        [
            [-half, -half, z],
            [half, -half, z],
            [half, half, z],
            [-half, half, z],
        ]
    };
    let mut positions: Vec<[f32; 3]> = corners(near / 2., 0.).to_vec();
    positions.extend(corners(far / 2., -length));
    let mut indices = Vec::new();
    for side in 0..4_u32 {
        let next = (side + 1) % 4;
        indices.extend([side, next, 4 + next, side, 4 + next, 4 + side]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(indices))
}

#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "one query each for the shooters, the beams and the camera"
)]
fn draw_beam(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    assets: Option<Res<BeamAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    added: Query<Entity, Added<AimBeam>>,
    shooters: Query<(&WorldPosition, &Combatant, &CombatIntent), With<AimBeam>>,
    cameras: Query<
        (&GlobalTransform, Option<&Projection>, Option<&Camera>),
        (With<Camera3d>, Without<strata_terrain::OffWorld>),
    >,
    mut beams: Query<(
        Entity,
        &BeamView,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(assets) = assets else { return };
    for shooter in &added {
        let mesh = meshes.add(prism(0., 0., 0.));
        commands.spawn((
            BeamView {
                owner: shooter,
                mesh: mesh.clone(),
            },
            Mesh3d(mesh),
            MeshMaterial3d(assets.faint.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            NoFrustumCulling,
        ));
    }
    let camera = cameras.iter().next();
    let eye = camera.map(|(transform, ..)| transform.translation());
    // Metres across one pixel at one metre from the camera.
    let pixel = camera.map_or(
        2. * (DEFAULT_FOV / 2.).tan() / DEFAULT_HEIGHT_PX,
        |(_, projection, camera)| {
            let fov = match projection {
                Some(Projection::Perspective(perspective)) => perspective.fov,
                _ => DEFAULT_FOV,
            };
            let height = camera
                .and_then(Camera::logical_viewport_size)
                .map_or(DEFAULT_HEIGHT_PX, |size| size.y);
            2. * (fov / 2.).tan() / height
        },
    );
    for (entity, view, mut transform, mut visibility, mut material) in &mut beams {
        let Ok((position, actor, input)) = shooters.get(view.owner) else {
            commands.entity(entity).despawn();
            meshes.remove(&view.mesh);
            continue;
        };
        let from = origin.to_render(position.0)
            + Vec3::Y * MUZZLE_UP
            + veyra_sim::PlayerIntent::forward(actor.facing) * MUZZLE_FORWARD;
        let to = origin.to_render(input.aim_point);
        let line = to - from;
        let length = line.length();
        if actor.charge <= 0. || length < 0.1 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;
        let lock = locked(actor.charge);
        material.0 = if lock {
            assets.lock.clone()
        } else {
            assets.faint.clone()
        };
        let pixels = if lock { LOCK_PIXELS } else { FAINT_PIXELS };
        let width = |at: Vec3| eye.map_or(length, |eye| at.distance(eye)) * pixel * pixels;
        if let Some(mut mesh) = meshes.get_mut(&view.mesh) {
            *mesh = prism(width(from), width(to), length);
        }
        *transform = Transform::from_translation(from).looking_to(line, Vec3::Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_core::WorldPos;

    fn world_with(charge: f32, aim: WorldPos) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(RenderOrigin::default());
        app.init_resource::<Assets<Mesh>>();
        app.insert_resource(BeamAssets {
            faint: Handle::default(),
            lock: Handle::default(),
        });
        app.add_systems(Update, draw_beam);
        let mut actor = Combatant::on_team(1, 85.);
        actor.charge = charge;
        let shooter = app
            .world_mut()
            .spawn((
                AimBeam,
                WorldPosition(WorldPos::new(0., 0., 0.)),
                actor,
                CombatIntent {
                    aim_point: aim,
                    ..default()
                },
            ))
            .id();
        (app, shooter)
    }

    /// The first update spawns the beam, the second draws it.
    fn beam(app: &mut App) -> (Visibility, Transform, f32) {
        app.update();
        app.update();
        let world = app.world_mut();
        let (view, visibility, transform) = world
            .query::<(&BeamView, &Visibility, &Transform)>()
            .single(world)
            .map(|(a, b, c)| (a.mesh.clone(), *b, *c))
            .expect("one beam");
        let meshes = world.resource::<Assets<Mesh>>();
        let positions = meshes
            .get(&view)
            .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
            .and_then(|values| values.as_float3())
            .expect("positions");
        let length = positions.iter().map(|p| -p[2]).fold(0., f32::max);
        (visibility, transform, length)
    }

    #[test]
    fn a_beam_is_there_only_while_the_charge_runs_and_reaches_the_aim() {
        let aim = WorldPos::new(0., 0., 60.);
        let (mut app, shooter) = world_with(0.5, aim);
        let (visibility, transform, length) = beam(&mut app);
        assert_eq!(visibility, Visibility::Visible);
        let end = transform.translation + transform.rotation * Vec3::NEG_Z * length;
        let target = RenderOrigin::default().to_render(aim);
        assert!(end.distance(target) < 0.01, "{end} against {target}");
        app.world_mut()
            .get_mut::<Combatant>(shooter)
            .expect("actor")
            .charge = 0.;
        assert_eq!(beam(&mut app).0, Visibility::Hidden);
    }

    #[test]
    fn a_shooter_without_the_marker_has_no_beam() {
        let (mut app, shooter) = world_with(0.5, WorldPos::new(0., 0., 60.));
        app.world_mut().entity_mut(shooter).remove::<AimBeam>();
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&BeamView>().iter(world).count(), 0);
    }

    #[test]
    fn the_lock_is_the_last_of_the_charge() {
        let row = veyra_sim::combat_tables::current()
            .opponents
            .marksman
            .clone();
        assert!(!locked(0.5));
        assert!(locked(1. - row.lock / row.charge + 0.001));
        assert!(locked(1.));
    }
}
