//! Keyboard to [`PlayerIntent`]: WASD relative to the camera's yaw, space
//! to jump, shift to sprint.
//!
//! The intent is written every frame and consumed once per sim tick, so
//! the jump is set and never cleared here: the tick clears it once it has
//! seen it, which is what makes a press between two ticks count exactly
//! once. A player with a [`ScriptedIntent`] is left alone; the script owns
//! its intent.

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use veyra_sim::{LocalPlayer, PlayerIntent, ScriptedIntent};

use crate::orbit::OrbitCamera;

/// Writes the local player's intent from the keys and the camera.
pub fn write_player_intent(
    keys: Res<ButtonInput<KeyCode>>,
    cameras: Query<&OrbitCamera>,
    mut players: Query<&mut PlayerIntent, (With<LocalPlayer>, Without<ScriptedIntent>)>,
) {
    let mut wish = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish.x -= 1.0;
    }
    let sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let jump = keys.just_pressed(KeyCode::Space);
    let yaw = cameras.iter().next().map(|cam| cam.yaw);
    for mut intent in &mut players {
        intent.move_dir = wish.normalize_or_zero();
        intent.sprint = sprint;
        intent.jump |= jump;
        if let Some(yaw) = yaw {
            intent.yaw = yaw;
        }
    }
}
