//! A free camera: WASD to move, mouse look while the right button is held,
//! shift to go fast, scroll to change the base speed.
//!
//! The camera moves in world space: [`fly`] writes the entity's
//! `WorldPosition` in `f64` and the sim's origin sync derives the
//! `Transform` from it (ADR 0002). Only the rotation is written here. A
//! flycam entity therefore carries a `WorldPosition`, and as the slice's
//! one observer it is also the `PrimaryObserver` the streamer and the
//! render origin follow.

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseScrollUnit,
};
use bevy::math::{DVec3, EulerRot, Quat, Vec3};
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use strata_sim::{PrimaryObserver, WorldPosition};

/// The free camera's state and tuning. Attach to the camera entity.
///
/// Yaw and pitch are stored here rather than read back from the transform
/// because a quaternion round trip drifts, and because clamping pitch is
/// simpler on the angle than on the rotation. The systems write the
/// rotation every frame from these, so the spawn transform's rotation does
/// not matter. Requires [`PrimaryObserver`] (and through it a
/// `WorldPosition`): the flycam is where the world is looked at from.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[require(PrimaryObserver)]
pub struct FlyCam {
    /// Base speed in metres per second.
    pub speed: f32,
    /// Multiplier while shift is held.
    pub fast_multiplier: f32,
    /// Radians of rotation per pixel of mouse motion.
    pub sensitivity: f32,
    /// Rotation about `y`, radians. Zero looks down `-z`.
    pub yaw: f32,
    /// Rotation about the local `x`, radians, clamped short of the poles.
    pub pitch: f32,
}

impl Default for FlyCam {
    fn default() -> Self {
        Self {
            speed: 50.0,
            fast_multiplier: 8.0,
            sensitivity: 0.002,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

impl FlyCam {
    /// Slowest the scroll wheel can go, metres per second.
    pub const MIN_SPEED: f32 = 1.0;
    /// Fastest the scroll wheel can go, metres per second. Well above the
    /// 200 m/s Phase 2 tests streaming at.
    pub const MAX_SPEED: f32 = 5_000.0;
    /// Just short of straight up or down, so `forward` never degenerates.
    const PITCH_LIMIT: f32 = core::f32::consts::FRAC_PI_2 - 0.01;

    /// A camera facing `yaw`, `pitch` at the default speed.
    #[must_use]
    pub fn looking(yaw: f32, pitch: f32) -> Self {
        Self {
            yaw,
            pitch: pitch.clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT),
            ..Self::default()
        }
    }

    /// The rotation the yaw and pitch describe.
    #[must_use]
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

/// Whether the camera is being steered: the right button is held.
fn steering(mouse: &ButtonInput<MouseButton>) -> bool {
    mouse.pressed(MouseButton::Right)
}

/// Locks and hides the cursor while the right button is held, so a long
/// drag does not hit the window edge, and restores it on release.
pub fn grab_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    if mouse.just_pressed(MouseButton::Right) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    } else if mouse.just_released(MouseButton::Right) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Mouse look while steering. Always writes the rotation, so a camera
/// spawned with any transform faces its `FlyCam` angles from the first frame.
pub fn look(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut cameras: Query<(&mut FlyCam, &mut Transform)>,
) {
    for (mut cam, mut transform) in &mut cameras {
        if steering(&mouse) && motion.delta != bevy::math::Vec2::ZERO {
            cam.yaw -= motion.delta.x * cam.sensitivity;
            cam.pitch = (cam.pitch - motion.delta.y * cam.sensitivity)
                .clamp(-FlyCam::PITCH_LIMIT, FlyCam::PITCH_LIMIT);
        }
        transform.rotation = cam.rotation();
    }
}

/// Scroll changes the base speed geometrically, so one notch is the same
/// relative change at 5 m/s and at 500 m/s.
pub fn adjust_speed(scroll: Res<AccumulatedMouseScroll>, mut cameras: Query<&mut FlyCam>) {
    if scroll.delta.y == 0.0 {
        return;
    }
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        // Trackpads report pixels; roughly a notch per 50 px.
        MouseScrollUnit::Pixel => scroll.delta.y / 50.0,
    };
    for mut cam in &mut cameras {
        cam.speed = (cam.speed * 1.25f32.powf(notches)).clamp(FlyCam::MIN_SPEED, FlyCam::MAX_SPEED);
    }
}

/// WASD in the camera's frame, Q/E and Space/Ctrl for down/up, shift for
/// the fast multiplier. Moves the `WorldPosition`; the transform follows.
pub fn fly(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cameras: Query<(&FlyCam, &Transform, &mut WorldPosition)>,
) {
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
        wish.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::ControlLeft) {
        wish.y -= 1.0;
    }
    if wish == Vec3::ZERO {
        return;
    }
    let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (cam, transform, mut position) in &mut cameras {
        // Forward and right follow the full look direction (flying, not
        // walking), up stays world up so vertical motion is predictable.
        let forward = transform.forward().as_vec3();
        let right = transform.right().as_vec3();
        let dir = (forward * -wish.z + right * wish.x + Vec3::Y * wish.y).normalize_or_zero();
        let speed = if fast {
            cam.speed * cam.fast_multiplier
        } else {
            cam.speed
        };
        let step = dir * speed * time.delta_secs();
        position.0 += DVec3::new(f64::from(step.x), f64::from(step.y), f64::from(step.z));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_zero_looks_down_negative_z() {
        let cam = FlyCam::looking(0.0, 0.0);
        let forward = cam.rotation() * Vec3::NEG_Z;
        assert!((forward - Vec3::NEG_Z).length() < 1e-6);
    }

    #[test]
    fn pitch_is_clamped() {
        let cam = FlyCam::looking(0.0, 10.0);
        assert!(cam.pitch < core::f32::consts::FRAC_PI_2);
        let cam = FlyCam::looking(0.0, -10.0);
        assert!(cam.pitch > -core::f32::consts::FRAC_PI_2);
    }
}
