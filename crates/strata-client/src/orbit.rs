//! A third-person orbit camera over the local player.
//!
//! The camera is a plain render-space entity: it has no `WorldPosition`
//! and is not an observer. The player is the observer, so the streamer and
//! the render origin follow the player, and the camera is placed every
//! frame from where the player is drawn. That placement happens in
//! `PostUpdate` after the origin sync, so a rebase and the camera never
//! disagree for a frame (ADR 0002).
//!
//! The player moves at 20 Hz and the camera at the frame rate, so the
//! target is the player's position interpolated between the last two ticks
//! by the fixed clock's overstep, the same interpolation the player's own
//! mesh uses. Terrain is kept out of the shot by sampling the world height
//! under the camera and along the boom, which is cheap and does not need
//! the collider.

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton, MouseScrollUnit,
};
use bevy::math::{EulerRot, Quat, Vec2, Vec3};
use bevy::time::{Fixed, Time};
use bevy::transform::components::Transform;
use veyra_sim::{LocalPlayer, PreviousWorldPosition, RenderOrigin, WorldPosition, WorldSourceRes};

/// The orbit camera's angles and distance. Attach to the camera entity;
/// it follows whichever entity is the [`LocalPlayer`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// Rotation about `y`, radians. Zero looks down `-z`; positive turns
    /// towards `-x`. The player's intent is relative to this.
    pub yaw: f32,
    /// Rotation about the local `x`, radians; negative looks down at the
    /// player.
    pub pitch: f32,
    /// Boom length, metres, between [`Self::MIN_DISTANCE`] and
    /// [`Self::MAX_DISTANCE`].
    pub distance: f32,
    /// Radians per pixel of mouse motion.
    pub sensitivity: f32,
    /// Where the boom is anchored, metres above the capsule's centre.
    pub eye_height: f32,
    /// How far the camera is kept above the ground and the sea.
    pub clearance: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.3,
            distance: 6.0,
            sensitivity: 0.002,
            eye_height: 0.5,
            clearance: 0.5,
        }
    }
}

impl OrbitCamera {
    /// The shortest boom.
    pub const MIN_DISTANCE: f32 = 2.0;
    /// The longest boom.
    pub const MAX_DISTANCE: f32 = 20.0;
    /// Steepest look down: nearly straight above the player.
    pub const MIN_PITCH: f32 = -1.45;
    /// Highest look up: a little below the player, which the terrain
    /// clearance usually overrides anyway.
    pub const MAX_PITCH: f32 = 0.6;

    /// A camera behind a player facing `yaw`.
    #[must_use]
    pub fn looking(yaw: f32) -> Self {
        Self {
            yaw,
            ..Self::default()
        }
    }

    /// The rotation the angles describe; the camera looks along its `-z`.
    #[must_use]
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// Where the camera sits for a target at `target`, before terrain
    /// clearance.
    #[must_use]
    pub fn position_for(&self, target: Vec3) -> Vec3 {
        target + self.rotation() * Vec3::Z * self.distance
    }
}

/// Mouse look while the right button is held (the flycam's cursor grab
/// covers this camera too).
pub fn look(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut cameras: Query<&mut OrbitCamera, Without<crate::view::CombatCamera>>,
) {
    if !mouse.pressed(MouseButton::Right) || motion.delta == Vec2::ZERO {
        return;
    }
    for mut cam in &mut cameras {
        cam.yaw -= motion.delta.x * cam.sensitivity;
        cam.pitch = (cam.pitch - motion.delta.y * cam.sensitivity)
            .clamp(OrbitCamera::MIN_PITCH, OrbitCamera::MAX_PITCH);
    }
}

/// Scroll changes the boom length geometrically.
pub fn zoom(
    scroll: Res<AccumulatedMouseScroll>,
    mut cameras: Query<&mut OrbitCamera, Without<crate::view::CombatCamera>>,
) {
    if scroll.delta.y == 0.0 {
        return;
    }
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 50.0,
    };
    for mut cam in &mut cameras {
        cam.distance = (cam.distance * 0.85f32.powf(notches))
            .clamp(OrbitCamera::MIN_DISTANCE, OrbitCamera::MAX_DISTANCE);
    }
}

/// Places the camera behind the interpolated player, above the ground.
/// Runs in `PostUpdate` after the origin sync.
pub fn follow(
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    source: Option<Res<WorldSourceRes>>,
    view: Option<Res<crate::view::RideView>>,
    players: Query<
        (
            &WorldPosition,
            &PreviousWorldPosition,
            Option<&strata_physics::CharacterController>,
        ),
        With<LocalPlayer>,
    >,
    mut cameras: Query<(&OrbitCamera, &mut Transform), Without<crate::view::CombatCamera>>,
) {
    let Ok((position, previous, controller)) = players.single() else {
        return;
    };
    let world = previous.interpolate(position.0, time.overstep_fraction_f64());
    let target = origin.to_render(world);
    let view = view.as_deref().filter(|_| controller.is_some());
    for (cam, mut transform) in &mut cameras {
        // Pulled back and up onto the bike, eased both ways.
        let (height, boom) = match (view, controller) {
            (Some(view), Some(controller)) => (
                view.mix(cam.eye_height, view.eye - controller.half_height()),
                view.mix(cam.distance, view.distance),
            ),
            _ => (cam.eye_height, cam.distance),
        };
        let anchor = target + Vec3::Y * height;
        let mut eye = anchor + cam.rotation() * Vec3::Z * boom;
        if let Some(source) = &source {
            // The ground under the camera and under the middle of the
            // boom: the second catches a ridge between the two.
            let floor = |p: Vec3| {
                let w = origin.to_world(p);
                let s = source.source().sample(w.x(), w.z());
                let ground = f64::from(s.height.max(source.source().sea_level()));
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "render space is f32 by definition"
                )]
                let y = (ground - origin.0.y()) as f32;
                y + cam.clearance
            };
            let min_y = floor(eye).max(floor((eye + anchor) * 0.5));
            if eye.y < min_y {
                eye.y = min_y;
            }
        }
        transform.translation = eye;
        transform.look_at(anchor, Vec3::Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_sits_behind_and_above() {
        let cam = OrbitCamera::looking(0.0);
        let eye = cam.position_for(Vec3::ZERO);
        assert!(eye.z > 0.0, "behind a player facing -z: {eye}");
        assert!(eye.y > 0.0, "above with a downward pitch: {eye}");
        assert!((eye.length() - cam.distance).abs() < 1e-5);
    }
}
