//! Veyra's physics, over the engine's: what `strata-physics` cannot hold
//! because it is the game's. Bolts and blades resolved against the colliders
//! ([`combat`]) and the hoverbike motor ([`ride`]).
//!
//! `strata-physics` is an engine crate over `strata-app` and links no game
//! rule, so a rule edit (crafting, combat, the ride's dials) never rebuilds
//! it, and it is checked to stay that way by `just server-check`. This crate
//! sits above it and above `veyra-rules`; the game, the dedicated server and
//! the balance harness add [`VeyraPhysicsPlugin`] where they add
//! `PhysicsPlugin`, and the facade's headless group adds both.
//!
//! A rider is a character the walk step leaves alone: [`ride::mark_rider`]
//! puts the engine's `ExternalMotor` on it as `Riding` arrives and
//! [`ride::step_riders`] moves it in the same [`PhysicsSet::Character`] slot,
//! between the walk step and the position sync.

pub mod combat;
pub mod ride;

use bevy::app::{App, Plugin};
use bevy::ecs::schedule::IntoScheduleConfigs;
use strata_app::SimTick;
use strata_physics::PhysicsSet;
use strata_physics::character::{step_characters, sync_characters};
use veyra_rules::combat::CombatSet;

/// The game's half of the physics: the characters' physics waits for the
/// combat decision, bolts and blades resolve after the characters move, and
/// riders take the ridden motor. Add it after `PhysicsPlugin`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VeyraPhysicsPlugin;

impl Plugin for VeyraPhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(ride::mark_rider)
            .add_observer(ride::release_rider)
            .configure_sets(SimTick, PhysicsSet::Sync.after(CombatSet::Decide))
            .add_systems(
                SimTick,
                (
                    ride::step_riders
                        .after(step_characters)
                        .before(sync_characters)
                        .in_set(PhysicsSet::Character),
                    (combat::advance_bolts, combat::resolve_attacks)
                        .chain()
                        .after(PhysicsSet::Character)
                        .before(PhysicsSet::Writeback),
                ),
            );
    }
}
