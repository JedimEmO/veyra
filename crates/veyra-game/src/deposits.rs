//! Deposits in the game (ticket 0007, step 3; ticket 0009, step 3): the
//! world's deposits handed to the sim.
//!
//! Where deposits are is `strata::veyra_world::deposits`, a pure function of
//! the seed, the resource registry and the ground. [`init`] builds it over
//! the loader's world and publishes it as the sim's `Survey`, from which
//! the sim keeps `NearDeposits` and a ping reads. Nothing is stood in the
//! world: a deposit is found with the survey tool.

use std::sync::Arc;

use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::ProceduralWorldRes;
use strata::sim::deposits::{DepositField, Survey};

/// Publishes the world's deposits to the sim, once the world and its
/// registries exist. A data set with no resources has none, and neither
/// has a run with no household to draw from them unless it starts at one:
/// the sim lists the deposits round every observer it is given a survey
/// for, which a flight or a benchmark has no use for.
pub fn init(
    mut commands: Commands,
    launch: Res<crate::Launch>,
    world: Option<Res<ProceduralWorldRes>>,
    seed: Option<Res<WorldSeed>>,
    registries: Res<strata::sim::Registries>,
) {
    let (Some(world), Some(seed)) = (world, seed) else {
        return;
    };
    if registries.0.resources.is_empty() || !launch.deposits {
        return;
    }
    let field = DepositField::new(Arc::clone(&world.0), seed.0, Arc::clone(&registries.0));
    info!(
        "deposits: {} types on a {:.0} m lattice",
        registries.0.resources.types().len(),
        registries.0.resources.rules().lattice.cell_m
    );
    commands.insert_resource(Survey(Arc::new(field)));
}
