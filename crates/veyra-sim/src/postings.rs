//! The adapter half of the `postings` rules: its requests, systems and plugin.

pub use veyra_rules::postings::*;

use bevy::app::{App, Plugin, Update};

use bevy::ecs::prelude::*;

use bevy::state::condition::in_state;

use strata_core::Seed;

use crate::deeds::{DeedKind, Ledger};

use crate::deposits::Purse;

use crate::encounters::{Cleared, EncounterId, Fights, NearEncounters, mark_cleared};

use crate::fitting::WorkbenchAccess;

use crate::order::RequestSet;

use crate::procurement::{ProcurementInbox, ProcurementRequest, at_board};

use crate::progression::Reach;

use crate::sites::NearSites;

use crate::{AppRequestExt, AppState, LocalPlayer, Registries, WorldPosition, WorldSeed};

fn is_job(request: &ProcurementRequest) -> bool {
    matches!(
        request,
        ProcurementRequest::TakeJob { .. } | ProcurementRequest::Collect { .. }
    )
}

/// `SimSet::Input`, before the board's own requests: the job requests leave
/// the inbox here and are answered with a [`JobAnswer`].
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the board, the data, the world, the books and the answer"
)]
pub fn consume(
    mut inbox: ResMut<ProcurementInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    seed: Option<Res<WorldSeed>>,
    candidates: Res<PostingCandidates>,
    cleared: Res<Cleared>,
    mut jobs: ResMut<Jobs>,
    mut purse: ResMut<Purse>,
    mut answers: MessageWriter<JobAnswer>,
    mut ledger: Ledger,
) {
    let mine = inbox.take_if(is_job);
    if mine.is_empty() {
        return;
    }
    let seed = seed.map_or(Seed(0), |seed| seed.0);
    let now = ledger.now();
    for (_, request) in mine {
        let result = match request {
            ProcurementRequest::TakeJob { buyer, key } => {
                let board = at_board(*access, &reach, &buyer);
                let faction = ledger.faction_of_buyer(&registries.0, &buyer);
                match (registries.0.postings.as_ref(), faction) {
                    (Some(rules), Some(faction)) => {
                        let shown = offered(
                            rules,
                            &buyer,
                            faction.seat,
                            &candidates.0,
                            &cleared,
                            &jobs,
                            now,
                            seed,
                        );
                        check_take(rules, board, &shown, &jobs, key).map(|posting| {
                            let posting = posting.clone();
                            jobs.taken.insert(
                                key,
                                Job {
                                    posting: posting.clone(),
                                    buyer: buyer.clone(),
                                    taken_at: now,
                                    done: false,
                                },
                            );
                            JobOutcome::Taken(posting)
                        })
                    }
                    _ if !board => Err(JobRefusal::NotAtBoard),
                    _ => Err(JobRefusal::NotOffered),
                }
            }
            ProcurementRequest::Collect { buyer, key } => {
                let board = at_board(*access, &reach, &buyer);
                check_collect(board, &jobs, &buyer, key)
                    .map(|job| (job.posting.name.clone(), job.posting.pay))
                    .map(|(name, pay)| {
                        jobs.taken.remove(&key);
                        if let Some(rules) = registries.0.postings.as_ref() {
                            let window = window_of(rules, now);
                            jobs.collected_in.retain(|_, was| *was >= window);
                            jobs.collected_in.insert(key, window);
                        }
                        purse.0 = purse.0.saturating_add(pay);
                        if let Some(faction) = ledger.faction_of_buyer(&registries.0, &buyer) {
                            ledger.deed(&registries.0, faction.id, DeedKind::JobDone);
                        }
                        JobOutcome::Collected { name, pay }
                    })
            }
            _ => continue,
        };
        answers.write(JobAnswer { result });
    }
}

/// `Update`: a job whose camp was cleared since it was taken is done.
pub fn settle_jobs(cleared: Option<Res<Cleared>>, mut jobs: ResMut<Jobs>) {
    let Some(cleared) = cleared else {
        return;
    };
    if jobs.taken.values().all(|job| job.done) {
        return;
    }
    let due = jobs
        .taken
        .iter()
        .any(|(key, job)| !job.done && cleared.at.get(key).is_some_and(|at| *at >= job.taken_at));
    if due {
        jobs.settle(&cleared);
    }
}

/// `Update`: the camps the member comes near, or fights, are found.
pub fn find_camps(
    registries: Option<Res<Registries>>,
    (sites, near, fights): (
        Option<Res<NearSites>>,
        Option<Res<NearEncounters>>,
        Option<Res<Fights>>,
    ),
    players: Query<&WorldPosition, With<LocalPlayer>>,
    mut known: ResMut<Known>,
) {
    let (Some(registries), Some(sites), Some(near), Some(fights)) =
        (registries, sites, near, fights)
    else {
        return;
    };
    let (Some(rules), Ok(position)) = (registries.0.postings.as_ref(), players.single()) else {
        return;
    };
    let at = [position.0.x(), position.0.z()];
    // Distance first: almost no site is near, so the kind is rarely asked.
    let camps = sites
        .sites
        .iter()
        .filter(|site| {
            let centre = site.site.centre;
            (centre[0] - at[0]).hypot(centre[1] - at[1]) <= rules.seen_m
        })
        .filter(|site| {
            registries.0.encounters.iter().any(|def| {
                def.at
                    .site()
                    .is_some_and(|kind| kind.to_string() == site.kind)
            })
        })
        .map(|site| (site.site.id.key(), site.site.centre));
    let engaged = near
        .camps
        .iter()
        .filter(|camp| fights.engaged(camp.id))
        .filter_map(|camp| match camp.id {
            EncounterId::Camp(key) => Some(key),
            _ => None,
        });
    let fresh: Vec<u64> = camps
        .map(|(key, _)| key)
        .chain(engaged)
        .filter(|key| !known.is_found(*key))
        .collect();
    if !fresh.is_empty() {
        known.camps.extend(fresh);
    }
}

/// Registers the jobs, the found camps and what keeps them. Added by
/// `SimPlugin`; with no request and nothing taken it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct PostingsPlugin;

impl Plugin for PostingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Jobs>()
            .init_resource::<Known>()
            .init_resource::<PostingCandidates>()
            .add_message::<JobAnswer>()
            .add_request::<ProcurementRequest, _>(RequestSet::Postings, consume)
            .add_systems(
                Update,
                (settle_jobs, find_camps)
                    .after(mark_cleared)
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::crafting::tests::shipped;
    use crate::deeds::Deeds;
    use crate::deeds::tests::pelagic;
    use crate::encounters::PlayedClock;
    use crate::factions::Factions;
    use bevy::app::App;
    use std::sync::Arc;

    use bevy::app::Update;

    use strata_core::Seed;

    use crate::deeds::DeedKind;

    use crate::deposits::Purse;
    use crate::encounters::Cleared;

    use crate::fitting::WorkbenchAccess;

    use crate::procurement::ProcurementInbox;
    use crate::procurement::ProcurementRequest;

    use crate::progression::Reach;

    use crate::WorldSeed;

    pub(crate) use veyra_rules::postings::test_support::*;

    fn world_app() -> (App, Vec<Candidate>, [f64; 2]) {
        let (map, faction, _) = pelagic();
        let registries = shipped();
        let rules = rules();
        let camps = gather(map.world(), map, &registries.0, faction.seat, rules.reach_m);
        let mut app = App::new();
        app.insert_resource(registries)
            .insert_resource(Factions(Arc::clone(map)))
            .insert_resource(PlayedClock { seconds: 10.0 })
            .insert_resource(WorldSeed(Seed(42)))
            .insert_resource(PostingCandidates(camps.clone()))
            .insert_resource(Reach {
                board: Some(buyer()),
                ..Default::default()
            })
            .insert_resource(WorkbenchAccess::AtWorkbench)
            .init_resource::<ProcurementInbox>()
            .init_resource::<Cleared>()
            .init_resource::<Jobs>()
            .init_resource::<Purse>()
            .init_resource::<Deeds>()
            .add_message::<JobAnswer>()
            .add_systems(
                Update,
                (
                    strata_app::request::address::<ProcurementRequest>,
                    consume,
                    settle_jobs,
                )
                    .chain(),
            );
        (app, camps, faction.seat)
    }

    fn answers(app: &mut App) -> Vec<Result<JobOutcome, JobRefusal>> {
        app.world_mut()
            .resource_mut::<bevy::ecs::message::Messages<JobAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect()
    }

    /// Take, clear, collect: the pay reaches the purse, a deed is written and
    /// the job is gone; a second collect is refused.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one job's whole life, take to the next window"
    )]
    fn taking_clearing_and_collecting_pays_and_writes_a_deed() {
        let (mut app, camps, seat) = world_app();
        let now = 10.0;
        let shown = offered(
            &rules(),
            &buyer(),
            seat,
            &camps,
            &Cleared::default(),
            &Jobs::default(),
            now,
            Seed(42),
        );
        let posting = shown[0].clone();
        let key = posting.key;
        let mut got = Vec::new();
        let mut push = |app: &mut App, request| {
            app.world_mut()
                .resource_mut::<ProcurementInbox>()
                .push(request);
            app.update();
            got.extend(answers(app));
        };
        push(
            &mut app,
            ProcurementRequest::TakeJob {
                buyer: buyer(),
                key: 12345,
            },
        );
        push(
            &mut app,
            ProcurementRequest::TakeJob {
                buyer: buyer(),
                key,
            },
        );
        push(
            &mut app,
            ProcurementRequest::Collect {
                buyer: buyer(),
                key,
            },
        );
        app.world_mut()
            .resource_mut::<Cleared>()
            .clear(key, now + 5.0);
        app.update();
        push(
            &mut app,
            ProcurementRequest::Collect {
                buyer: buyer(),
                key,
            },
        );
        push(
            &mut app,
            ProcurementRequest::Collect {
                buyer: buyer(),
                key,
            },
        );
        assert_eq!(
            got,
            vec![
                Err(JobRefusal::NotOffered),
                Ok(JobOutcome::Taken(posting.clone())),
                Err(JobRefusal::NotDone),
                Ok(JobOutcome::Collected {
                    name: posting.name,
                    pay: posting.pay
                }),
                Err(JobRefusal::NotTaken),
            ]
        );
        assert_eq!(app.world().resource::<Purse>().0, posting.pay);
        assert!(app.world().resource::<Jobs>().taken.is_empty());
        let deeds = app.world().resource::<Deeds>();
        let pelagic_id = pelagic().1.id.0;
        assert_eq!(
            deeds.by.keys().copied().collect::<Vec<_>>(),
            vec![pelagic_id]
        );
        let kinds: Vec<DeedKind> = deeds.by[&pelagic_id].iter().map(|d| d.kind).collect();
        assert_eq!(kinds, vec![DeedKind::JobDone]);
        assert!(app.world().resource::<ProcurementInbox>().is_empty());

        // The camp returns within hours, but the board does not post it again
        // in the window it was paid for; the next window may.
        let jobs = app.world().resource::<Jobs>();
        let returned = now + 4.0 * 3600.0;
        assert!(camps.iter().all(|camp| !Cleared::default().is_quiet(
            camp.key,
            returned,
            camp.returns_h
        )));
        let show = |jobs: &Jobs, at: f64| {
            let cleared = Cleared::default();
            offered(
                &rules(),
                &buyer(),
                seat,
                &camps,
                &cleared,
                jobs,
                at,
                Seed(42),
            )
        };
        assert!(
            show(jobs, returned).iter().all(|shown| shown.key != key),
            "posted again in the window it was paid"
        );
        let next = rules().window_s() * 1.5;
        assert_eq!(
            show(jobs, next),
            show(&Jobs::default(), next),
            "a later window draws as if nothing was collected"
        );
    }
}
