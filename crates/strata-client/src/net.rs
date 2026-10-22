//! What a connected client draws that a single process never has: the
//! other players.
//!
//! A replicated player that is not ours arrives with no
//! `CharacterController` (the server keeps that; the client never steps
//! it), so [`crate::visual::spawn_player_visuals`], which reads the
//! controller for the capsule's size, skips it. This plugin gives such a
//! player the default capsule instead, drawn and interpolated by the same
//! [`PlayerVisual`] system as the local one. It knows nothing about the
//! network: "a player that is neither ours nor stepped here" is the whole
//! definition of a remote player, so this crate needs no adapter to name
//! it, and `ClientPlugin` runs the system in every build; in a standalone
//! game the query is simply empty.

use bevy::asset::Assets;
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::math::primitives::Capsule3d;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::transform::components::Transform;
use strata_physics::CharacterController;
use veyra_sim::{LocalPlayer, Player};

use crate::visual::PlayerVisual;

/// A capsule for every new player that is neither ours nor stepped here.
/// Skipped without mesh assets, like the local one. Runs after
/// [`crate::visual::spawn_player_visuals`] so the two never race over one
/// player.
pub fn spawn_remote_player_visuals(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    players: Query<
        Entity,
        (
            Added<Player>,
            Without<LocalPlayer>,
            Without<CharacterController>,
        ),
    >,
) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    let controller = CharacterController::default();
    for entity in &players {
        let capsule = Capsule3d::new(
            controller.radius,
            (controller.height - 2.0 * controller.radius).max(0.0),
        );
        commands.spawn((
            PlayerVisual { of: entity },
            Mesh3d(meshes.add(Mesh::from(capsule))),
            MeshMaterial3d(materials.add(StandardMaterial {
                // A warmer tint than ours, so two capsules on one beach
                // are told apart.
                base_color: Color::srgb(0.80, 0.55, 0.40),
                perceptual_roughness: 0.6,
                ..Default::default()
            })),
            Transform::default(),
        ));
    }
}
