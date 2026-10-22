//! `--take-job` and `--found` (ticket 0012, step 4b): a still's postings. The
//! factions themselves are handed to the sim by `veyra_game::factions::init`.

use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::factions::Factions;

/// `--take-job` and `--found`, once, for a still: the posting taken as the
/// board's button would take it (without being at the board), the camp
/// counted cleared after it for `--job-done`, and a camp found. Runs after the
/// camps are read from the baked world; a run with neither flag does nothing.
#[allow(
    clippy::too_many_arguments,
    reason = "the flags, the candidates, the ledgers a still writes and the clock"
)]
pub fn still_jobs(
    cli: Res<crate::DevArgs>,
    candidates: Res<strata::sim::postings::PostingCandidates>,
    registries: Res<strata::sim::Registries>,
    (factions, seed, clock): (
        Option<Res<Factions>>,
        Option<Res<WorldSeed>>,
        Option<Res<strata::sim::encounters::PlayedClock>>,
    ),
    (mut jobs, mut known, mut cleared): (
        ResMut<strata::sim::postings::Jobs>,
        ResMut<strata::sim::postings::Known>,
        ResMut<strata::sim::encounters::Cleared>,
    ),
    player: Query<&strata::sim::WorldPosition, With<strata::sim::LocalPlayer>>,
    mut done: Local<bool>,
) {
    if *done || (cli.take_job.is_none() && cli.found.is_none()) {
        return;
    }
    let (Some(factions), Some(seed), Some(clock), Ok(position)) =
        (factions, seed, clock, player.single())
    else {
        return;
    };
    if candidates.0.is_empty() {
        return;
    }
    *done = true;
    let at = [position.0.x(), position.0.z()];
    if let (Some(n), Some(rules)) = (cli.take_job, registries.0.postings.as_ref())
        && let Some((buyer, seat)) = registries.0.buyers.iter().find_map(|buyer| {
            let faction =
                strata::sim::deeds::anchor_faction(&factions.0, &registries.0, &buyer.id)?;
            Some((buyer.id.clone(), faction.seat))
        })
    {
        let shown = strata::sim::postings::offered(
            rules,
            &buyer,
            seat,
            &candidates.0,
            &cleared,
            &jobs,
            clock.seconds,
            seed.0,
        );
        if let Some(posting) = n.checked_sub(1).and_then(|at| shown.get(at)) {
            info!("postings: took {} for {} cr", posting.name, posting.pay);
            if cli.job_done {
                cleared.clear(posting.key, clock.seconds);
            }
            jobs.taken.insert(
                posting.key,
                strata::sim::postings::Job {
                    posting: posting.clone(),
                    buyer,
                    taken_at: clock.seconds,
                    done: cli.job_done,
                },
            );
        }
    }
    if let Some(which) = &cli.found {
        let camp = if which == "nearest" {
            candidates.0.iter().min_by(|a, b| {
                let d = |c: &&strata::sim::postings::Candidate| {
                    (c.centre[0] - at[0]).hypot(c.centre[1] - at[1])
                };
                d(a).total_cmp(&d(b))
            })
        } else {
            candidates
                .0
                .iter()
                .find(|camp| camp.name.eq_ignore_ascii_case(which))
        };
        if let Some(camp) = camp {
            info!("postings: found {}", camp.name);
            known.camps.insert(camp.key);
        }
    }
}
