//! What Bevy 0.19.1's volumetric fog needs around the local fog volumes
//! (lighting plan phase 2c), and what it costs.
//!
//! The volumes themselves are the world's: the city spawns one over each
//! covered run of its market and the foliage one under each dense cell of
//! crowns, as plain `FogVolume`s. [`look`](crate::look) puts `VolumetricFog`
//! on the camera and `VolumetricLight` on the sun while
//! [`RenderLook::local_fog`] is on. This module covers four things Bevy's
//! pass gets wrong or does not report:
//!
//! - **The moon.** `prepare_lights` sorts directional lights by
//!   `(volumetric, shadows, entity)`, so a light that is not volumetric
//!   sorts first, and the fog shader stops at the first light that is not
//!   volumetric: with the moon in the list and not volumetric, the sun never
//!   scatters and the fog only darkens (ticket 0003's spike found it). The
//!   moon is made volumetric whenever the sun is, and hidden whenever it is
//!   dark (all day), so by day the pass marches the sun alone. A shadowless
//!   light scatters nothing in Bevy's pass (a step outside every shadow
//!   cascade counts as unlit), so at dusk the moon costs a march and adds
//!   nothing; at night the volumes only absorb.
//! - **The study's mist at zero density.** The study scene spawns its 320 m
//!   box whatever the look; it was only marched while the look gave it a
//!   density. With the camera's `VolumetricFog` on for the local volumes it
//!   would be marched at density zero, full screen, for nothing. Its
//!   `FogVolume` is parked while the density is zero and given back, with
//!   the look's values, when it is not.
//! - **The near plane** (Bevy issue 25333, fixed upstream by PR 25336, which
//!   0.19.1 does not carry): Bevy draws a volume's front faces when the
//!   camera is outside it and a full-screen quad when it is inside, so with
//!   the camera outside but nearer a face than the near plane the face is
//!   clipped and the volume vanishes, or is cut along a diagonal. A camera
//!   walking into a market aisle crosses that shell. A volume whose box the
//!   camera is within half a metre of, outside, is drawn grown on that side
//!   to take the camera a quarter metre inside, so Bevy takes its interior
//!   path; a box's density fades to nothing over its outer fifth, so the
//!   growth does not show.
//! - **GPU time.** Bevy's pass writes no timestamps; a span from just after
//!   the ground fog pass (which runs in the main pass's set, after the
//!   transparent pass, and has its own span) to just before TAA brackets
//!   it. The wireframe pass shares that gap, and draws only under F4.
//!
//! [`RenderLook::local_fog`]: crate::look::RenderLook::local_fog

use crate::look::RenderLook;
use crate::study::StudyFog;
use crate::{Moon, Sun};
use bevy::anti_alias::taa::temporal_anti_alias;
use bevy::app::{App, PostUpdate, Update};
use bevy::camera::primitives::{Aabb, Frustum};
use bevy::camera::visibility::Visibility;
use bevy::camera::{Camera3d, Projection};
use bevy::core_pipeline::core_3d::main_transparent_pass_3d;
use bevy::core_pipeline::{Core3dSystems, schedule::Core3d};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::ecs::system::NonSendMarker;
use bevy::light::{DirectionalLight, FogVolume, VolumetricFog, VolumetricLight};
use bevy::math::{Vec3, Vec3A};
use bevy::render::{RenderApp, diagnostic::RecordDiagnostics, renderer::RenderContext};
use bevy::transform::TransformSystems;
use bevy::transform::components::{GlobalTransform, Transform};

/// A camera outside a volume nearer its box than this, metres, is taken
/// inside it ([`swallowing`]). Bevy's default near plane is 0.1 m and the
/// corners of its rectangle at 2560x1440 and 45 degrees lie within 0.13 m
/// of the eye; a camera this far out is clear of both.
const SHELL_M: f32 = 0.5;

/// How far inside the grown box the camera ends up, metres: over the near
/// plane, so the interior path's quad at the near plane starts in the box.
const INSIDE_M: f32 = 0.25;

/// The span's name in Bevy's render diagnostics.
const SPAN: &str = "local_fog";

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        (
            keep_the_moon_behind_the_sun.after(crate::move_sun),
            park_the_study_mist.after(crate::study::update_volumes),
            log_gpu_time,
        ),
    )
    .add_systems(
        PostUpdate,
        swallow_the_camera.after(TransformSystems::Propagate),
    );
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            Core3d,
            (
                // After the ground fog pass when it is registered, so the
                // span holds Bevy's volumetric pass alone.
                begin
                    .in_set(Core3dSystems::MainPass)
                    .after(main_transparent_pass_3d)
                    .after(crate::fog_pass::GroundFogPass),
                end.in_set(Core3dSystems::EarlyPostProcess)
                    .before(temporal_anti_alias),
            ),
        );
    }
}

/// See the module's first point.
fn keep_the_moon_behind_the_sun(
    mut commands: Commands,
    suns: Query<Has<VolumetricLight>, With<Sun>>,
    mut moons: Query<
        (
            Entity,
            &DirectionalLight,
            &mut Visibility,
            Has<VolumetricLight>,
        ),
        (With<Moon>, Without<Sun>),
    >,
) {
    let scattering = suns.iter().any(|volumetric| volumetric);
    for (entity, light, mut visibility, volumetric) in &mut moons {
        visibility.set_if_neq(if light.illuminance > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if scattering && !volumetric {
            commands.entity(entity).insert(VolumetricLight);
        } else if !scattering && volumetric {
            commands.entity(entity).remove::<VolumetricLight>();
        }
    }
}

/// The study mist's volume while its density is zero. The study scene is
/// chosen on the command line and never changes in a run, so a parked
/// mist is never one `study::update_volumes` would have to find again.
#[derive(Component)]
struct ParkedMist(FogVolume);

/// See the module's second point.
fn park_the_study_mist(
    mut commands: Commands,
    look: Res<RenderLook>,
    drawn: Query<(Entity, &FogVolume), With<StudyFog>>,
    parked: Query<(Entity, &ParkedMist), With<StudyFog>>,
) {
    if look.fog_density > 0.0 {
        for (entity, ParkedMist(fog)) in &parked {
            let mut fog = fog.clone();
            fog.density_factor = look.fog_density;
            fog.scattering = look.fog_scattering;
            fog.light_intensity = look.fog_light_intensity;
            fog.scattering_asymmetry = look.fog_asymmetry;
            commands.entity(entity).remove::<ParkedMist>().insert(fog);
        }
    } else {
        for (entity, fog) in &drawn {
            commands
                .entity(entity)
                .insert(ParkedMist(fog.clone()))
                .remove::<FogVolume>();
        }
    }
}

/// The box a volume is drawn with for a camera at `eye`: its own, unless the
/// camera is outside it but within [`SHELL_M`] of it, when the sides the
/// camera is beyond move out to take it [`INSIDE_M`] inside. Only those
/// sides move, so the far side of the fog stays where it was.
#[must_use]
pub(crate) fn swallowing(volume: Transform, eye: Vec3, shell: f32) -> Transform {
    let half = volume.scale.abs() * 0.5;
    let local = volume.rotation.inverse() * (eye - volume.translation);
    let gap = (local.abs() - half).max(Vec3::ZERO).length();
    if gap == 0.0 || gap > shell {
        return volume;
    }
    let (mut low, mut high) = (-half, half);
    for axis in 0..3 {
        if local[axis] > half[axis] {
            high[axis] = local[axis] + INSIDE_M;
        } else if local[axis] < -half[axis] {
            low[axis] = local[axis] - INSIDE_M;
        }
    }
    Transform {
        translation: volume.translation + volume.rotation * ((low + high) * 0.5),
        rotation: volume.rotation,
        scale: high - low,
    }
}

/// See the module's third point. Writes the drawn box into the volume's
/// `GlobalTransform`, which is what Bevy extracts, after propagation, and
/// puts its own back once the camera leaves the shell; the `Transform` is
/// the world's and the render origin's, untouched. Root volumes only: every
/// volume the world spawns is one, and a child's global box is its
/// parent's business.
fn swallow_the_camera(
    cameras: Query<(&GlobalTransform, Option<&Projection>), (With<Camera3d>, With<VolumetricFog>)>,
    mut volumes: Query<
        (&Transform, &mut GlobalTransform),
        (With<FogVolume>, Without<ChildOf>, Without<Camera3d>),
    >,
) {
    let Some((eye, projection)) = cameras.iter().next() else {
        return;
    };
    let near = match projection {
        Some(Projection::Perspective(p)) => p.near,
        _ => 0.1,
    };
    let shell = SHELL_M.max(near * 4.0);
    let eye = eye.translation();
    for (transform, mut global) in &mut volumes {
        let drawn = GlobalTransform::from(swallowing(*transform, eye, shell));
        if *global != drawn {
            *global = drawn;
        }
    }
}

// Not exclusive, unlike `diagnostics`: `Core3d` applies its systems'
// command buffers at its end, in system order, while an exclusive system's
// nested run applies at once, so exclusive ends would both be queued ahead
// of the pass they bracket. Bevy's recorder pairs a span's two ends by
// thread, so both are kept on the thread running the schedule.
fn begin(mut ctx: RenderContext, _thread: NonSendMarker) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.begin_time_span(ctx.command_encoder(), SPAN.into());
    }
}

fn end(mut ctx: RenderContext, _thread: NonSendMarker) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.end_time_span(ctx.command_encoder());
    }
}

/// Every two seconds, with Bevy's render diagnostics on (`--stats`), the
/// GPU time of Bevy's volumetric pass, with how many volumes are alive and
/// how many the camera's frustum touches: the phase's 0.4 ms ceiling is
/// for a volume in view.
fn log_gpu_time(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Res<bevy::time::Time<bevy::time::Real>>,
    cameras: Query<(&GlobalTransform, &Frustum), (With<Camera3d>, With<VolumetricFog>)>,
    volumes: Query<(&GlobalTransform, &Transform), With<FogVolume>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let ms = |span: &str| {
        let suffix = format!("/{span}/elapsed_gpu");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&suffix))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    };
    let Some(span) = ms(SPAN) else {
        return;
    };
    let unit = Aabb {
        center: Vec3A::ZERO,
        half_extents: Vec3A::splat(0.5),
    };
    let (alive, mut in_view, mut around) = (volumes.iter().len(), 0, 0);
    // The camera's signed distance to the nearest volume's own box, metres,
    // negative inside: what the near-plane check is posed by.
    let mut nearest = f32::INFINITY;
    if let Some((eye, frustum)) = cameras.iter().next() {
        for (drawn, own) in &volumes {
            let affine = drawn.affine();
            if frustum.intersects_obb(&unit, &affine, true, true) {
                in_view += 1;
            }
            let local = affine.inverse().transform_point3(eye.translation());
            if local.abs().max_element() <= 0.5 {
                around += 1;
            }
            let local = own.rotation.inverse() * (eye.translation() - own.translation);
            let outside = local.abs() - own.scale.abs() * 0.5;
            let gap = if outside.max_element() > 0.0 {
                outside.max(Vec3::ZERO).length()
            } else {
                outside.max_element()
            };
            if gap.abs() < nearest.abs() {
                nearest = gap;
            }
        }
    }
    tracing::info!(
        "local fog pass, ms of GPU time: {span:.3} ({alive} volumes, {in_view} in view, {around} around the camera, nearest box {nearest:.3} m)",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Quat;

    fn boxed() -> Transform {
        Transform::from_xyz(10.0, 2.0, -4.0)
            .with_rotation(Quat::from_rotation_y(0.7))
            .with_scale(Vec3::new(20.0, 3.0, 8.0))
    }

    fn inside(volume: Transform, eye: Vec3) -> bool {
        let local = volume.rotation.inverse() * (eye - volume.translation);
        (local.abs() - volume.scale * 0.5).max_element() <= 0.0
    }

    #[test]
    fn a_camera_in_the_near_plane_shell_is_taken_inside() {
        let volume = boxed();
        // Five centimetres over the top, inside the near plane of a camera
        // looking down: the case Bevy draws nothing for.
        let eye = volume.translation + volume.rotation * Vec3::new(3.0, 1.55, 1.0);
        assert!(!inside(volume, eye));
        let drawn = swallowing(volume, eye, SHELL_M);
        assert!(inside(drawn, eye));
        // Only the top moved, and by the gap and the margin.
        assert!((drawn.scale.y - (3.0 + 0.05 + INSIDE_M)).abs() < 1e-4);
        assert!((drawn.scale.x - 20.0).abs() < 1e-4 && (drawn.scale.z - 8.0).abs() < 1e-4);
        let bottom = |t: Transform| (t.rotation.inverse() * t.translation).y - t.scale.y * 0.5;
        assert!((bottom(drawn) - bottom(volume)).abs() < 1e-4);
    }

    #[test]
    fn a_camera_inside_or_clear_of_the_shell_leaves_the_box_alone() {
        let volume = boxed();
        for local in [
            Vec3::ZERO,
            Vec3::new(9.9, 1.4, 3.9),
            Vec3::new(10.6, 0.0, 0.0),
            Vec3::new(0.0, 30.0, 0.0),
        ] {
            let eye = volume.translation + volume.rotation * local;
            assert_eq!(swallowing(volume, eye, SHELL_M), volume, "{local}");
        }
    }

    #[test]
    fn a_camera_off_a_corner_grows_every_side_it_is_beyond() {
        let volume = boxed();
        let eye = volume.translation + volume.rotation * Vec3::new(-10.2, -1.7, 4.1);
        let drawn = swallowing(volume, eye, SHELL_M);
        assert!(inside(drawn, eye));
        assert!(drawn.scale.x > 20.0 && drawn.scale.y > 3.0 && drawn.scale.z > 8.0);
    }

    #[test]
    fn the_moon_scatters_with_the_sun_and_hides_while_it_is_dark() {
        let mut app = App::new();
        app.add_systems(Update, keep_the_moon_behind_the_sun);
        let sun = app
            .world_mut()
            .spawn((Sun, DirectionalLight::default(), VolumetricLight))
            .id();
        let moon = app
            .world_mut()
            .spawn((
                Moon,
                DirectionalLight {
                    illuminance: 0.0,
                    ..Default::default()
                },
            ))
            .id();
        app.update();
        let world = app.world();
        assert!(world.get::<VolumetricLight>(moon).is_some());
        assert_eq!(world.get::<Visibility>(moon), Some(&Visibility::Hidden));
        app.world_mut()
            .get_mut::<DirectionalLight>(moon)
            .expect("moon")
            .illuminance = 4.0;
        app.world_mut().entity_mut(sun).remove::<VolumetricLight>();
        app.update();
        let world = app.world();
        assert!(world.get::<VolumetricLight>(moon).is_none());
        assert_eq!(world.get::<Visibility>(moon), Some(&Visibility::Inherited));
    }
}
