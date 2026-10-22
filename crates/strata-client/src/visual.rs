//! The placeholder player: a capsule the size of the character's collider.
//!
//! The mesh is not on the player entity. The player's `Transform` is the
//! sim's, written once per tick from its `WorldPosition`, and a mesh on it
//! would move in 20 Hz steps under a 200 fps camera. So every player gets a
//! separate visual entity whose transform is written every frame from the
//! position interpolated between the last two ticks, in `PostUpdate` after
//! the origin sync so a rebase never shows.

use bevy::asset::Assets;
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::math::Quat;
use bevy::math::primitives::Capsule3d;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::time::{Fixed, Time};
use bevy::transform::components::Transform;
use strata_physics::CharacterController;
use veyra_sim::{Player, PlayerIntent, PreviousWorldPosition, RenderOrigin, WorldPosition};

/// The mesh entity of a player.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerVisual {
    /// The player this draws.
    pub of: Entity,
}

/// A capsule for every new player. Skipped without mesh assets, so the
/// client plugin still builds in a headless test.
pub fn spawn_player_visuals(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    players: Query<
        (Entity, &CharacterController),
        (Added<Player>, Without<veyra_sim::combat::Combatant>),
    >,
) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    for (entity, controller) in &players {
        let capsule = Capsule3d::new(
            controller.radius,
            (controller.height - 2.0 * controller.radius).max(0.0),
        );
        commands.spawn((
            PlayerVisual { of: entity },
            Mesh3d(meshes.add(Mesh::from(capsule))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.55, 0.62, 0.75),
                perceptual_roughness: 0.6,
                ..Default::default()
            })),
            Transform::default(),
        ));
    }
}

/// Puts every visual where its player is drawn this frame, facing the
/// player's yaw, and removes the visuals of players that are gone.
pub fn place_player_visuals(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    players: Query<(&WorldPosition, &PreviousWorldPosition, &PlayerIntent)>,
    mut visuals: Query<(Entity, &PlayerVisual, &mut Transform)>,
) {
    let t = time.overstep_fraction_f64();
    for (entity, visual, mut transform) in &mut visuals {
        let Ok((position, previous, intent)) = players.get(visual.of) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = origin.to_render(previous.interpolate(position.0, t));
        transform.rotation = Quat::from_rotation_y(intent.yaw);
    }
}
