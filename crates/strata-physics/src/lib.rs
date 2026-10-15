//! Physics: `avian3d` stepped inside the simulation tick, the ground as a
//! heightfield collider per region, and the character that walks on it.
//!
//! This crate sits above the sim and the streamer and below the render
//! crates: a dedicated server runs it, so the collider is the authoritative
//! ground and the terrain mesh is only a picture of it (PLAN Phase 4a).
//!
//! # One clock
//!
//! Avian's [`PhysicsSchedule`] runs inside [`SimTick`], in
//! [`SimSet::Step`], so physics advances exactly once per 20 Hz sim tick
//! and never on its own clock. Avian reads its step from the generic `Time`,
//! which is the fixed clock while `FixedUpdate` runs the tick.
//!
//! ```text
//! SimTick / SimSet::Step
//!   PhysicsSystems::First
//!     PhysicsSet::Sync         colliders given to new characters; Position = origin.to_render(WorldPosition)
//!   PhysicsSystems::Prepare .. Writeback     avian: colliders, broad phase, contacts, solver
//!   PhysicsSystems::Last
//!     PhysicsSet::Character    move and slide every CharacterController from its PlayerCommand,
//!                              then Position from WorldPosition again for the characters
//!     PhysicsSet::Writeback    WorldPosition = origin.to_world(Position) for dynamic bodies
//!
//! Update, after StreamSet::Diff
//!   TerrainColliderSet::Dispatch   RegionLoad at lod 0 -> a sampling task; RegionUnload -> drop it, despawn the bodies
//!   TerrainColliderSet::Collect    finished tasks -> a static heightfield at the region's centre and a
//!                                  static cylinder per trunk the flora placement put on it; a static
//!                                  compound per procedural site building near an observer
//!
//! PostUpdate, after OriginSet::Rebase
//!   rebase_physics           on RenderOriginChanged, every Position rewritten from its WorldPosition
//! ```
//!
//! # Coordinates
//!
//! Avian is `f32` and runs in render space, relative to the
//! [`RenderOrigin`] (ADR 0002). Its `Position` is derived from the sim's
//! `f64` [`WorldPosition`] at the start of every tick and never the other
//! way for anything the sim owns: the character step writes both, a dynamic
//! body writes its `WorldPosition` back after the step. Avian's own
//! `Transform` sync is switched off, because the sim's origin sync owns
//! every `Transform` and the two would fight. When the origin moves,
//! [`sync::rebase_physics`] rewrites every body's `Position` the same frame
//! the transforms are rewritten, so nothing observes a mixed frame; the
//! next tick's sync would have done it too, one frame late.
//!
//! [`PhysicsSchedule`]: avian3d::schedule::PhysicsSchedule
//! [`SimTick`]: strata_sim::SimTick
//! [`SimSet::Step`]: strata_sim::SimSet::Step
//! [`RenderOrigin`]: strata_sim::RenderOrigin
//! [`WorldPosition`]: strata_sim::WorldPosition

pub mod character;
pub mod combat;
pub mod crossings;
pub mod flora;
pub mod grounding;
pub mod roadside;
pub mod settlement;
pub mod sites;
pub mod sync;
pub mod terrain;

pub use avian3d;

use avian3d::collider_tree::ColliderTreeOptimization;
use avian3d::interpolation::PhysicsInterpolationPlugin;
use avian3d::physics_transform::PhysicsTransformConfig;
use avian3d::prelude::{PhysicsPlugins, PhysicsSystems};
use bevy::app::{App, Plugin, PluginGroup, PostUpdate, Update};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::state::condition::in_state;
use strata_sim::{AppState, OriginSet, SimSet, SimTick};
use strata_stream::StreamSet;

pub use character::{CharacterController, WaterAt};
pub use flora::{FloraPlacement, TreeBody, TreeCollider};
pub use terrain::{Heightfield, TerrainCollider, TerrainColliderStats};

/// This crate's sets inside [`SimTick`], placed
/// around avian's [`PhysicsSystems`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicsSet {
    /// Before avian: new characters get their collider, every `Position` is
    /// derived from its `WorldPosition`.
    Sync,
    /// After avian: the characters move and slide.
    Character,
    /// After the characters: dynamic bodies write their `WorldPosition`.
    Writeback,
}

/// The terrain collider sets, in `Update` after the streamer has decided.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainColliderSet {
    /// A sampling task per lod 0 load, a drop and a despawn per unload.
    Dispatch,
    /// Finished tasks become static heightfield bodies.
    Collect,
}

/// Registers avian in the sim tick with its transform sync off, the
/// position sync and rebase, the terrain colliders and the character
/// controller.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        // Inserted before avian's plugins build, because the transform
        // plugin reads it at build time to decide whether `Position`
        // requires a `Transform`. All three off: the sim owns transforms.
        app.insert_resource(PhysicsTransformConfig {
            propagate_before_physics: false,
            transform_to_position: false,
            position_to_transform: false,
            // Colliders here are never scaled through a `Transform`.
            transform_to_collider_scale: false,
        });
        // Interpolation writes `Transform` between ticks for bodies that
        // opt in; nothing here opts in, and the client interpolates
        // `WorldPosition` itself, so the plugin is left out rather than
        // idle.
        app.add_plugins(
            PhysicsPlugins::new(SimTick)
                .build()
                .disable::<PhysicsInterpolationPlugin>(),
        );
        // Avian optimises its collider trees on the async compute pool and
        // then blocks the tick until that task is done. The pool is also where
        // terrain meshing, collider building and foliage placement queue their
        // jobs, so on a fast flight the optimisation waited behind them for up
        // to 80 ms. The trees it optimises hold the few moving bodies; doing it
        // in place costs microseconds.
        app.insert_resource(ColliderTreeOptimization {
            use_async_tasks: false,
            ..ColliderTreeOptimization::default()
        });
        app.init_resource::<TerrainColliderStats>()
            .init_resource::<terrain::ColliderTasks>()
            .init_resource::<terrain::ColliderEntities>()
            .configure_sets(
                SimTick,
                (
                    PhysicsSystems::First,
                    PhysicsSystems::Prepare,
                    PhysicsSystems::StepSimulation,
                    PhysicsSystems::Writeback,
                    PhysicsSystems::Last,
                )
                    .chain()
                    .in_set(SimSet::Step),
            )
            .configure_sets(
                SimTick,
                (
                    PhysicsSet::Sync
                        .in_set(PhysicsSystems::First)
                        .after(strata_sim::combat::CombatSet::Decide),
                    (PhysicsSet::Character, PhysicsSet::Writeback)
                        .chain()
                        .in_set(PhysicsSystems::Last),
                ),
            )
            .add_systems(
                SimTick,
                (
                    (character::init_characters, sync::world_to_physics)
                        .chain()
                        .in_set(PhysicsSet::Sync),
                    // The characters synced again after their step, so a
                    // character's `Position` is current for the rest of
                    // the frame; the dynamic bodies are the writeback's.
                    (character::step_characters, character::sync_characters)
                        .chain()
                        .in_set(PhysicsSet::Character),
                    (combat::advance_bolts, combat::resolve_attacks)
                        .chain()
                        .after(PhysicsSet::Character)
                        .before(PhysicsSet::Writeback),
                    sync::physics_to_world.in_set(PhysicsSet::Writeback),
                ),
            )
            .add_systems(
                PostUpdate,
                sync::rebase_physics
                    .after(OriginSet::Rebase)
                    .run_if(in_state(AppState::InWorld)),
            )
            .configure_sets(
                Update,
                (TerrainColliderSet::Dispatch, TerrainColliderSet::Collect)
                    .chain()
                    .after(StreamSet::Diff)
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                (
                    terrain::dispatch_colliders.in_set(TerrainColliderSet::Dispatch),
                    terrain::collect_colliders.in_set(TerrainColliderSet::Collect),
                    (
                        settlement::start_settlement_colliders,
                        settlement::collect_settlement_colliders,
                    )
                        .chain()
                        .in_set(TerrainColliderSet::Collect),
                    sites::stream_site_colliders.in_set(TerrainColliderSet::Collect),
                    crossings::stream_crossing_colliders.in_set(TerrainColliderSet::Collect),
                    roadside::stream_roadside_colliders.in_set(TerrainColliderSet::Collect),
                ),
            )
            .init_resource::<settlement::SettlementColliders>()
            .init_resource::<sites::SiteBodies>()
            .init_resource::<crossings::CrossingBodies>()
            .init_resource::<roadside::RoadsideBodies>();
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn builds_over_the_streamer() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            PhysicsPlugin,
        ));
        app.finish();
        app.cleanup();
        app.update();
        app.update();
        app.update();
        app.update();
        let config = app.world().resource::<PhysicsTransformConfig>();
        assert!(!config.position_to_transform, "the sim owns Transform");
    }
}
