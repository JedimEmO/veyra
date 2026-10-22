//! The stills that set a ledger before the sim reads it: a camp cleared
//! or left with its cache, a stance and a lease, a posting taken or a place
//! found. Each is installed only when its flag is set.

use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::deposits::Survey;
use strata::sim::encounters::{Cleared, NearEncounters, PlayedClock};
use strata::sim::sites::NearSites;
use veyra_game::camps::CAMP_KIND;

use crate::DevArgs;

/// `--cleared`: the camp nearest the member is quiet from the first frame,
/// once, so a still shows it with nobody standing. `--cache` clears it the
/// same way and leaves its cache. Runs before the sim reads the ledger.
#[allow(
    clippy::too_many_arguments,
    reason = "the sites, the ledgers a clearing writes and the world a cache is drawn from"
)]
pub fn clear_nearest(
    cli: Res<DevArgs>,
    sites: Option<Res<NearSites>>,
    clock: Res<PlayedClock>,
    mut cleared: ResMut<Cleared>,
    mut caches: ResMut<strata::sim::loot::Caches>,
    (registries, seed, survey): (
        Option<Res<strata::sim::Registries>>,
        Option<Res<strata::sim::WorldSeed>>,
        Option<Res<Survey>>,
    ),
    encounters: Option<Res<NearEncounters>>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    mut done: Local<bool>,
) {
    if !(cli.cleared || cli.cache) || *done {
        return;
    }
    let (Some(sites), Ok(position)) = (sites, player.single()) else {
        return;
    };
    let (x, z) = (position.0.x(), position.0.z());
    // Any place an encounter file stands people at: a camp, a wreck or a machine.
    let places: Vec<String> = registries.as_ref().map_or_else(
        || vec![CAMP_KIND.to_owned()],
        |registries| {
            registries
                .0
                .encounters
                .iter()
                .filter_map(|def| def.at.site().map(ToString::to_string))
                .collect()
        },
    );
    let nearest = sites
        .sites
        .iter()
        .filter(|near| places.contains(&near.kind))
        .min_by(|a, b| {
            let d = |s: &&strata::sim::sites::NearSite| {
                (s.site.centre[0] - x).hypot(s.site.centre[1] - z)
            };
            d(a).total_cmp(&d(b))
        });
    if let Some(near) = nearest {
        cleared.clear(near.site.id.key(), clock.seconds);
        if cli.cache
            && let Some(registries) = registries
        {
            let tier = encounters
                .and_then(|near| near.forced_tier)
                .unwrap_or_else(|| {
                    strata::sim::combat_tiers::tier_at(near.site.centre[0], near.site.centre[1])
                });
            strata::sim::encounters::leave_cache(
                &registries.0,
                near.site.kind(),
                (near.site.id.key(), near.site.centre, tier),
                seed.map_or(strata::core::Seed(0), |seed| seed.0),
                (survey.as_deref(), None),
                clock.seconds,
                &mut caches,
            );
        }
        *done = true;
    }
}

/// The stills' set-ups run only for a run that asked for them, so no other
/// session carries a system that reads the flags to return.
pub fn install(app: &mut App, cli: &DevArgs) {
    if cli.cleared || cli.cache {
        app.add_systems(
            Update,
            clear_nearest
                .before(strata::sim::encounters::update_near_encounters)
                .run_if(in_state(AppState::InWorld)),
        );
    }
    if cli.take_job.is_some() || cli.found.is_some() {
        app.add_systems(
            Update,
            crate::factions::still_jobs.run_if(in_state(AppState::InWorld)),
        );
    }
    if !cli.stance.is_empty() || cli.lease {
        app.add_systems(
            Update,
            crate::standing::set_stance_and_lease.run_if(in_state(AppState::InWorld)),
        );
    }
}
