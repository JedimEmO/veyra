//! A settlement's buildings as colliders, on the server as on the client.
//!
//! The world places a settlement from the seed and the content step bakes
//! its buildings; each baked `.ashlar` file carries its collision proxies as
//! convex solids, readable with plain `ashlar` and no renderer. When the world
//! is published, a task reads every building of every settlement's layout
//! and the collector spawns one static body per building: a compound of its
//! nearest level's proxies at its pose. A city is a few dozen bodies, not a
//! stream, so they are built once and kept for the world's life.

use ashlar::{BakedBuilding, ConvexSolid};
use avian3d::prelude::{Position, RigidBody, Rotation};
use bevy::ecs::prelude::*;
use bevy::math::Quat;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::components::Transform;
use strata_core::WorldPos;
use strata_registry::AssetSource;
use strata_sim::{GeneratedRoot, RenderOrigin, Settlements, WorldPosition};
use tracing::{info, warn};

use crate::grounding::collider;

/// A building's static body.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct BuildingCollider {
    /// The settlement's id.
    pub settlement: String,
    /// The baked file, relative to the content step's output.
    pub file: String,
}

/// One building's collision, read and placed, waiting to be spawned.
pub(crate) struct BuildingBody {
    settlement: String,
    file: String,
    at: WorldPos,
    yaw: f64,
    solids: Vec<ConvexSolid>,
}

/// The read in flight, or done.
#[derive(Resource, Default)]
pub(crate) enum SettlementColliders {
    /// Not started.
    #[default]
    Idle,
    /// Reading the baked buildings.
    Reading(Task<Vec<BuildingBody>>),
    /// Spawned.
    Built,
}

/// The nearest level's collision proxies of a baked building.
pub(crate) fn read_collision(
    generated: &dyn AssetSource,
    path: &str,
) -> Result<Vec<ConvexSolid>, String> {
    let bytes = generated
        .read(path)
        .map_err(|e| format!("reading {path}: {e}"))?;
    let baked = BakedBuilding::read(&bytes).map_err(|e| format!("reading {path}: {e}"))?;
    let level = baked
        .level(0)
        .ok_or_else(|| format!("{path} has no levels"))?;
    Ok(level
        .colliders()
        .into_iter()
        .map(|placed| placed.solid)
        .collect())
}

/// Starts reading every settlement's buildings once the world is published.
pub(crate) fn start_settlement_colliders(
    settlements: Option<Res<Settlements>>,
    generated: Option<Res<GeneratedRoot>>,
    mut state: ResMut<SettlementColliders>,
) {
    if !matches!(*state, SettlementColliders::Idle) {
        return;
    }
    let (Some(settlements), Some(generated)) = (settlements, generated) else {
        return;
    };
    let Some(root) = generated.0.clone() else {
        *state = SettlementColliders::Built;
        return;
    };
    let placed = settlements.0.clone();
    let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
        let mut bodies = Vec::new();
        for settlement in &placed {
            let Some(layout) = &settlement.layout else {
                continue;
            };

            for building in &layout.buildings {
                match read_collision(&root, &building.file) {
                    Ok(solids) if !solids.is_empty() => bodies.push(BuildingBody {
                        settlement: layout.id.clone(),
                        file: building.file.clone(),
                        at: settlement.to_world(building.position),
                        yaw: settlement.world_yaw(building.yaw),
                        solids,
                    }),
                    Ok(_) => {}
                    Err(error) => warn!("{error}; the building will not collide"),
                }
            }
        }
        bodies
    });
    *state = SettlementColliders::Reading(task);
}

/// Spawns the buildings' bodies once they are read.
pub(crate) fn collect_settlement_colliders(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut state: ResMut<SettlementColliders>,
) {
    let SettlementColliders::Reading(task) = &mut *state else {
        return;
    };
    let Some(bodies) = block_on(poll_once(task)) else {
        return;
    };
    let mut spawned = 0;
    for body in bodies {
        let collider = match collider(&body.solids) {
            Ok(collider) => collider,
            Err(error) => {
                warn!(file = %body.file, "{error}; the building will not collide");
                continue;
            }
        };
        #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
        let rotation = Quat::from_rotation_y(body.yaw as f32);
        commands.spawn((
            BuildingCollider {
                settlement: body.settlement,
                file: body.file,
            },
            RigidBody::Static,
            collider,
            WorldPosition(body.at),
            Position(origin.to_render(body.at)),
            Rotation(rotation),
            Transform::default(),
        ));
        spawned += 1;
    }
    info!(buildings = spawned, "settlement colliders built");
    *state = SettlementColliders::Built;
}
