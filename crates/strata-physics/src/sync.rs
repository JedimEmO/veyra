//! `WorldPosition` in `f64` to avian's `Position` in render-space `f32`,
//! and back for what physics owns (ADR 0002).

use avian3d::prelude::{Position, RigidBody};
use bevy::ecs::prelude::*;
use strata_sim::{RenderOrigin, RenderOriginChanged, WorldPosition};

/// `Position = origin.to_render(WorldPosition)` for every body, at the start
/// of the tick. Writes only when the value differs so a static collider does
/// not count as moved every tick.
pub fn world_to_physics(
    origin: Res<RenderOrigin>,
    mut bodies: Query<(&WorldPosition, &mut Position)>,
) {
    for (world, mut position) in &mut bodies {
        let render = origin.to_render(world.0);
        if position.0 != render {
            position.0 = render;
        }
    }
}

/// `WorldPosition = origin.to_world(Position)` for dynamic bodies, whose
/// motion avian decides. Kinematic and static bodies are moved by the sim
/// and only ever read their `WorldPosition`; a character controller writes
/// both itself.
pub fn physics_to_world(
    origin: Res<RenderOrigin>,
    mut bodies: Query<(&RigidBody, &Position, &mut WorldPosition), Changed<Position>>,
) {
    for (body, position, mut world) in &mut bodies {
        if body.is_dynamic() {
            world.0 = origin.to_world(position.0);
        }
    }
}

/// When the render origin moved this frame, every `Position` is rewritten
/// from its `WorldPosition`, the same frame the sim rewrites every
/// `Transform`. Without this the bodies would sit in the old frame until
/// the next tick's [`world_to_physics`], one frame in which a spatial query
/// from `Update` would see the ground a rebase distance away.
pub fn rebase_physics(
    mut changed: MessageReader<RenderOriginChanged>,
    origin: Res<RenderOrigin>,
    mut bodies: Query<(&WorldPosition, &mut Position)>,
) {
    if changed.read().last().is_none() {
        return;
    }
    let mut moved = 0usize;
    for (world, mut position) in &mut bodies {
        position.0 = origin.to_render(world.0);
        moved += 1;
    }
    tracing::debug!(bodies = moved, "physics positions rebased");
}
