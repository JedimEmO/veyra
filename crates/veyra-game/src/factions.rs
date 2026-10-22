//! Factions in the game (ticket 0012, step 1): the world's factions handed
//! to the sim.
//!
//! Who they are is `strata::veyra_world::factions`, a pure function of the
//! seed, the placed sites and `assets/factions/`. [`init`] builds it over
//! the loader's world and publishes it as the sim's `Factions`, from which
//! the sim keeps `HeldHere` for the card. Nothing is stood in the world.

use std::sync::Arc;

use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::ProceduralWorldRes;
use strata::sim::factions::{FactionMap, Factions};

/// Publishes the world's factions to the sim, once the world and its
/// registries exist. A data set with no faction kinds has none.
pub fn init(
    mut commands: Commands,
    world: Option<Res<ProceduralWorldRes>>,
    seed: Option<Res<WorldSeed>>,
    registries: Res<strata::sim::Registries>,
) {
    let (Some(world), Some(seed)) = (world, seed) else {
        return;
    };
    if registries.0.factions.is_empty() {
        return;
    }
    let map = FactionMap::new(Arc::clone(&world.0), seed.0, Arc::clone(&registries.0));
    info!(
        "factions: {} kinds, {} authored holders",
        registries.0.factions.kinds().len(),
        registries.0.factions.anchors().len()
    );
    // The camps a board may post, read once here from the baked masses
    // (not every frame): round the seat of the first buyer a faction keeps,
    // which is Pelagic Works' board, the only one that ships.
    if let Some(rules) = registries.0.postings.as_ref()
        && let Some(seat) = registries
            .0
            .buyers
            .iter()
            .find_map(|buyer| strata::sim::deeds::anchor_faction(&map, &registries.0, &buyer.id))
            .map(|faction| faction.seat)
    {
        let camps =
            strata::sim::postings::gather(&world.0, &map, &registries.0, seat, rules.reach_m);
        info!("postings: {} camps within reach of the board", camps.len());
        commands.insert_resource(strata::sim::postings::PostingCandidates(camps));
    }
    commands.insert_resource(Factions(Arc::new(map)));
}
