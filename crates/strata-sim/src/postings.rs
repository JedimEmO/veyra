//! A holder's board posts work about the camps near it (ticket 0012, step
//! 4b): clear a named outlaw camp for a stated pay.
//!
//! The postings are not stored. [`offered`] is a pure function of the data's
//! dials, the camps near the board's seat, the played window, the world seed
//! and what is already taken, so every machine and every frame of a window
//! sees the same board. Taking one writes a [`Job`] into [`Jobs`] (which the
//! game saves) and the camp is then heard of; clearing it, which
//! [`Jobs::settle`] reads off [`Cleared`], lets the member collect the pay
//! and a deed at the board. [`Known`] is the camps the household has found,
//! by coming near or by fighting there. Local-only: nothing here is
//! replicated and none of it is in a version.
//!
//! The camps a board may post are [`PostingCandidates`], made once with
//! [`gather`] from the baked world (the game fills it at startup, after the
//! water is prepared, not every frame).

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Plugin, Update};
use bevy::ecs::prelude::*;
use bevy::state::condition::in_state;
use serde::{Deserialize, Serialize};
use strata_core::Seed;
use strata_registry::{Crew, GameRegistries, Namespaced, PostingRules};
use strata_worldgen::ProceduralWorld;
use strata_worldgen::factions::FactionMap;

use crate::combat_tiers::tier_at;
use crate::deeds::{DeedKind, Ledger};
use crate::deposits::Purse;
use crate::encounters::{Cleared, EncounterId, Fights, NearEncounters, mark_cleared};
use crate::fitting::WorkbenchAccess;
use crate::procurement::{self, ProcurementInbox, ProcurementRequest, at_board};
use crate::progression::Reach;
use crate::sites::NearSites;
use crate::{AppState, LocalPlayer, Registries, SimSet, SimTick, WorldPosition, WorldSeed};

/// A camp a board may post, as the world has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The camp's site key ([`strata_worldgen::sites::SiteId::key`]).
    pub key: u64,
    /// The camp's name.
    pub name: String,
    /// The pad's centre, world `(x, z)`.
    pub centre: [f64; 2],
    /// Its tier.
    pub tier: u8,
    /// The band that holds it, by name; empty where none does, which is
    /// every place whose crew is unaligned (a wreck, a derelict machine).
    pub band: String,
    /// Its site kind (`strata:wild_camp`), for the board's verb.
    pub kind: String,
    /// Played hours the camp stays quiet once cleared (its encounter's), so a
    /// camp that has not yet returned is not posted.
    pub returns_h: f32,
}

/// The area a taken job's camp is heard of in: not its point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Heard {
    /// The area's centre, world `(x, z)`, off the camp.
    pub centre: [f64; 2],
    /// Its radius, metres.
    pub radius: f64,
}

/// One posting: a camp to clear and what it pays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Posting {
    /// The camp's site key.
    pub key: u64,
    /// The camp's name.
    pub name: String,
    /// The camp's tier.
    pub tier: u8,
    /// Credits it pays.
    pub pay: u32,
    /// The camp's band, by name; empty for a place with no band.
    pub faction: String,
    /// The place's site kind, which words the posting. Empty in a file saved
    /// before wrecks and machines were posted, which are camps.
    #[serde(default)]
    pub kind: String,
    /// Where the camp is heard of once the posting is taken.
    pub heard: Heard,
}

/// A posting the member took.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// What was taken.
    pub posting: Posting,
    /// The board it was taken at.
    pub buyer: Namespaced,
    /// The played second it was taken.
    pub taken_at: f64,
    /// The camp was cleared after that; stays so until collected, though the
    /// camp returns.
    pub done: bool,
}

/// The jobs held, by camp key. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Jobs {
    /// Jobs by [`Candidate::key`].
    pub taken: BTreeMap<u64, Job>,
    /// The window each camp's job was last collected in: a board does not
    /// post a camp again in the window it was paid for, though the camp
    /// returns long before the window ends.
    #[serde(default)]
    pub collected_in: BTreeMap<u64, u64>,
}

impl Jobs {
    /// Mark done every job whose camp was cleared after it was taken. Whether
    /// anything changed.
    pub fn settle(&mut self, cleared: &Cleared) -> bool {
        let mut changed = false;
        for (key, job) in &mut self.taken {
            if !job.done && cleared.at.get(key).is_some_and(|at| *at >= job.taken_at) {
                job.done = true;
                changed = true;
            }
        }
        changed
    }
}

/// The camps found, by site key. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Known {
    /// Site keys of the camps found.
    pub camps: BTreeSet<u64>,
}

impl Known {
    /// Whether the camp `key` is found.
    #[must_use]
    pub fn is_found(&self, key: u64) -> bool {
        self.camps.contains(&key)
    }
}

/// The camps near the board's seat, made once by [`gather`].
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct PostingCandidates(pub Vec<Candidate>);

/// Every camp of the world within `reach_m` of `seat`, tier and band read
/// once. A camp is a site of a kind that an encounter file stands people at.
/// Reads baked masses only (the game bakes round the spawn before start);
/// this is for startup, not for a frame.
#[must_use]
pub fn gather(
    world: &ProceduralWorld,
    map: &FactionMap,
    registries: &GameRegistries,
    seat: [f64; 2],
    reach_m: f64,
) -> Vec<Candidate> {
    let kinds: Vec<_> = registries
        .encounters
        .iter()
        .filter_map(|def| {
            let kind = registries.site_kind_id(def.at.site()?)?;
            Some((kind, def.at.site()?.to_string(), def.returns_h, def.crew))
        })
        .collect();
    let mut found: Vec<Candidate> = world
        .sites_near(seat[0], seat[1], reach_m)
        .into_iter()
        .filter_map(|site| {
            let (_, name, returns_h, crew) =
                kinds.iter().find(|(kind, ..)| *kind == site.id.kind)?;
            // A corporate or conservation holder of a wreck is not the
            // enemy, so only a band's crew names its holder.
            let band = match crew {
                Crew::Holder => map
                    .holder_of(&site.id)
                    .and_then(|id| map.faction(id))
                    .map(|faction| faction.name)
                    .unwrap_or_default(),
                Crew::Unaligned => String::new(),
            };
            Some(Candidate {
                key: site.id.key(),
                tier: tier_at(site.centre[0], site.centre[1]),
                name: site.name,
                centre: site.centre,
                band,
                kind: name.clone(),
                returns_h: *returns_h,
            })
        })
        .collect();
    found.sort_by_key(|camp| camp.key);
    found
}

fn unit(bits: u64) -> f64 {
    // The top 53 bits as a share of one: exact in an f64.
    #[allow(clippy::cast_precision_loss, reason = "53 bits fit an f64 exactly")]
    let share = (bits >> 11) as f64 / (1_u64 << 53) as f64;
    share
}

/// Where a camp is heard of: a seeded offset from its point, no further than
/// the data's share of the area's radius.
#[must_use]
pub fn heard_of(rules: &PostingRules, buyer: &Namespaced, seed: Seed, camp: &Candidate) -> Heard {
    let base = seed.derive(&format!("heard:{buyer}:{}", camp.key));
    let angle = unit(base) * std::f64::consts::TAU;
    let off = rules.heard_offset
        * rules.heard_m
        * unit(seed.derive(&format!("heard.r:{buyer}:{}", camp.key))).sqrt();
    Heard {
        centre: [
            camp.centre[0] + off * angle.cos(),
            camp.centre[1] + off * angle.sin(),
        ],
        radius: rules.heard_m,
    }
}

/// The postings `buyer`'s board shows at played second `now`: from the
/// candidates within `reach_m` of `seat`, those not quiet in `cleared` and
/// not already taken, `open` of them in the order a hash of the seed, the
/// buyer, the window and the camp gives. The same on every machine and every
/// frame of a window.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the dials, the board, the camps, the two ledgers, the clock and the seed"
)]
pub fn offered(
    rules: &PostingRules,
    buyer: &Namespaced,
    seat: [f64; 2],
    camps: &[Candidate],
    cleared: &Cleared,
    taken: &Jobs,
    now: f64,
    seed: Seed,
) -> Vec<Posting> {
    let window = window_of(rules, now);
    let mut open: Vec<(u64, &Candidate)> = camps
        .iter()
        .filter(|camp| {
            (camp.centre[0] - seat[0]).hypot(camp.centre[1] - seat[1]) <= rules.reach_m
                && !cleared.is_quiet(camp.key, now, camp.returns_h)
                && !taken.taken.contains_key(&camp.key)
                && taken.collected_in.get(&camp.key) != Some(&window)
        })
        .map(|camp| {
            (
                seed.derive(&format!("posting:{buyer}:{window}:{}", camp.key)),
                camp,
            )
        })
        .collect();
    open.sort_by_key(|(hash, camp)| (*hash, camp.key));
    open.into_iter()
        .take(rules.open as usize)
        .map(|(_, camp)| Posting {
            key: camp.key,
            name: camp.name.clone(),
            tier: camp.tier,
            pay: rules.pay_of(camp.tier),
            faction: camp.band.clone(),
            kind: camp.kind.clone(),
            heard: heard_of(rules, buyer, seed, camp),
        })
        .collect()
}

/// The board's window at played second `now`: one set of postings a window.
#[must_use]
pub fn window_of(rules: &PostingRules, now: f64) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window count of played time, small and not negative"
    )]
    let window = (now.max(0.0) / rules.window_s()).floor() as u64;
    window
}

/// Why a job is not taken or not collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobRefusal {
    /// No board of the buyer is in reach.
    NotAtBoard,
    /// The board does not post it now (or the board posts nothing).
    NotOffered,
    /// As many jobs are held as the data allows.
    TooMany,
    /// The camp has not been cleared.
    NotDone,
    /// No such job is held at this board.
    NotTaken,
}

/// What a job request did.
#[derive(Debug, Clone, PartialEq)]
pub enum JobOutcome {
    /// The posting was taken.
    Taken(Posting),
    /// The job was collected: the pay went to the purse and a deed was
    /// written for the board's holder.
    Collected {
        /// The camp's name.
        name: String,
        /// Credits paid.
        pay: u32,
    },
}

/// The sim's answer to a [`ProcurementRequest::TakeJob`] or
/// [`ProcurementRequest::Collect`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct JobAnswer {
    /// What happened, or why nothing did.
    pub result: Result<JobOutcome, JobRefusal>,
}

/// Whether `key` may be taken: the one rule the request and the screen ask.
/// `shown` is [`offered`] now.
///
/// # Errors
/// Why not.
pub fn check_take<'a>(
    rules: &PostingRules,
    at_board: bool,
    shown: &'a [Posting],
    jobs: &Jobs,
    key: u64,
) -> Result<&'a Posting, JobRefusal> {
    if !at_board {
        return Err(JobRefusal::NotAtBoard);
    }
    let posting = shown
        .iter()
        .find(|posting| posting.key == key)
        .ok_or(JobRefusal::NotOffered)?;
    if jobs.taken.len() >= rules.taken_max as usize {
        return Err(JobRefusal::TooMany);
    }
    Ok(posting)
}

/// Whether `key` may be collected at `buyer`'s board.
///
/// # Errors
/// Why not.
pub fn check_collect<'a>(
    at_board: bool,
    jobs: &'a Jobs,
    buyer: &Namespaced,
    key: u64,
) -> Result<&'a Job, JobRefusal> {
    if !at_board {
        return Err(JobRefusal::NotAtBoard);
    }
    let job = jobs
        .taken
        .get(&key)
        .filter(|job| job.buyer == *buyer)
        .ok_or(JobRefusal::NotTaken)?;
    if job.done {
        Ok(job)
    } else {
        Err(JobRefusal::NotDone)
    }
}

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
    if !inbox.0.iter().any(is_job) {
        return;
    }
    let mine: Vec<ProcurementRequest> = inbox.0.extract_if(.., |request| is_job(request)).collect();
    let seed = seed.map_or(Seed(0), |seed| seed.0);
    let now = ledger.now();
    for request in mine {
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

/// Mark as found every camp in `camps` (`(key, centre)`) the member at `at`
/// is within `seen_m` of, and every key in `engaged`. Whether anything new.
pub fn find(
    known: &mut Known,
    camps: impl IntoIterator<Item = (u64, [f64; 2])>,
    at: [f64; 2],
    seen_m: f64,
    engaged: impl IntoIterator<Item = u64>,
) -> bool {
    let near = camps
        .into_iter()
        .filter(|(_, centre)| (centre[0] - at[0]).hypot(centre[1] - at[1]) <= seen_m)
        .map(|(key, _)| key);
    let mut changed = false;
    for key in near.chain(engaged) {
        changed |= known.camps.insert(key);
    }
    changed
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
            .add_systems(
                SimTick,
                consume.in_set(SimSet::Input).before(procurement::consume),
            )
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

    use std::sync::Arc;

    use bevy::app::App;

    use super::*;
    use crate::crafting::tests::shipped;
    use crate::deeds::{Deeds, tests::pelagic};
    use crate::encounters::PlayedClock;
    use crate::factions::Factions;

    const BUYER: &str = "veyra:buyer.pelagic_works";

    fn buyer() -> Namespaced {
        BUYER.parse().unwrap()
    }

    fn rules() -> PostingRules {
        shipped().0.postings.clone().expect("postings.ron ships")
    }

    fn camp(key: u64, x: f64) -> Candidate {
        Candidate {
            key,
            name: format!("Camp {key}"),
            centre: [x, 0.0],
            tier: 1 + u8::try_from(key % 3).unwrap(),
            band: "the Test Gang".to_owned(),
            kind: "strata:wild_camp".to_owned(),
            returns_h: 3.0,
        }
    }

    fn camps() -> Vec<Candidate> {
        (1..=8)
            .map(|key| camp(key, f64::from(u32::try_from(key).unwrap()) * 1000.0))
            .collect()
    }

    fn board(cleared: &Cleared, jobs: &Jobs, now: f64) -> Vec<Posting> {
        offered(
            &rules(),
            &buyer(),
            [0.0, 0.0],
            &camps(),
            cleared,
            jobs,
            now,
            Seed(42),
        )
    }

    /// A window's board is the same every time, shows the data's count, pays
    /// by tier, and the next window draws again.
    #[test]
    fn a_window_shows_the_same_few_postings_and_the_next_draws_again() {
        let none = (Cleared::default(), Jobs::default());
        let first = board(&none.0, &none.1, 10.0);
        assert_eq!(first.len(), rules().open as usize);
        assert_eq!(first, board(&none.0, &none.1, 3600.0 * 23.0));
        for posting in &first {
            assert_eq!(posting.pay, rules().pay_of(posting.tier));
            let camp = camps().into_iter().find(|c| c.key == posting.key).unwrap();
            let off = (posting.heard.centre[0] - camp.centre[0])
                .hypot(posting.heard.centre[1] - camp.centre[1]);
            assert!(off <= rules().heard_offset * rules().heard_m + 1e-6);
        }
        let drawn: Vec<Vec<u64>> = (0..6)
            .map(|w| {
                board(&none.0, &none.1, f64::from(w) * 24.0 * 3600.0 + 1.0)
                    .iter()
                    .map(|p| p.key)
                    .collect()
            })
            .collect();
        assert!(drawn.windows(2).any(|pair| pair[0] != pair[1]));
    }

    /// A camp out of reach, quiet or taken is not posted.
    #[test]
    fn a_far_quiet_or_taken_camp_is_not_posted() {
        let mut cleared = Cleared::default();
        let mut jobs = Jobs::default();
        let all: Vec<u64> = board(&cleared, &jobs, 0.0).iter().map(|p| p.key).collect();
        let (quiet, taken) = (all[0], all[1]);
        cleared.clear(quiet, 0.0);
        let first = board(&cleared, &jobs, 100.0)
            .iter()
            .map(|p| p.key)
            .collect::<Vec<_>>();
        assert!(!first.contains(&quiet));
        let posting = board(&cleared, &jobs, 100.0)
            .into_iter()
            .find(|p| p.key == taken)
            .unwrap();
        jobs.taken.insert(
            taken,
            Job {
                posting,
                buyer: buyer(),
                taken_at: 100.0,
                done: false,
            },
        );
        let next = board(&cleared, &jobs, 100.0);
        assert_eq!(next.len(), rules().open as usize, "the board draws another");
        assert!(next.iter().all(|p| p.key != taken && p.key != quiet));
        // Camps 1 to 8 are 1 to 8 km out; only those within reach count.
        let short = PostingRules {
            reach_m: 2500.0,
            ..rules()
        };
        let near = offered(
            &short,
            &buyer(),
            [0.0, 0.0],
            &camps(),
            &cleared,
            &Jobs::default(),
            0.0,
            Seed(42),
        );
        assert!(!near.is_empty());
        assert!(near.iter().all(|p| p.key <= 2));
        // A quiet camp that has returned is posted again.
        let later = 3.0 * 3600.0 + 1.0;
        assert!(cleared.is_quiet(quiet, 100.0, 3.0) && !cleared.is_quiet(quiet, later, 3.0));
        let wide = PostingRules {
            open: 20,
            ..rules()
        };
        let again = |now: f64| {
            offered(
                &wide,
                &buyer(),
                [0.0, 0.0],
                &camps(),
                &cleared,
                &Jobs::default(),
                now,
                Seed(42),
            )
        };
        assert!(again(100.0).iter().all(|p| p.key != quiet));
        assert!(again(later).iter().any(|p| p.key == quiet));
    }

    /// A clearing after the take sets done, one before does not, and the
    /// rules refuse by reason.
    #[test]
    fn a_job_is_done_by_a_clearing_after_the_take_and_refused_by_reason() {
        let rules = rules();
        let shown = board(&Cleared::default(), &Jobs::default(), 0.0);
        let key = shown[0].key;
        let mut jobs = Jobs::default();
        assert_eq!(
            check_take(&rules, false, &shown, &jobs, key),
            Err(JobRefusal::NotAtBoard)
        );
        assert_eq!(
            check_take(&rules, true, &shown, &jobs, 999),
            Err(JobRefusal::NotOffered)
        );
        assert!(check_take(&rules, true, &shown, &jobs, key).is_ok());
        for k in 100..100 + u64::from(rules.taken_max) {
            jobs.taken.insert(
                k,
                Job {
                    posting: shown[0].clone(),
                    buyer: buyer(),
                    taken_at: 50.0,
                    done: false,
                },
            );
        }
        assert_eq!(
            check_take(&rules, true, &shown, &jobs, key),
            Err(JobRefusal::TooMany)
        );
        assert_eq!(
            check_collect(true, &jobs, &buyer(), 100).err(),
            Some(JobRefusal::NotDone)
        );
        assert_eq!(
            check_collect(true, &jobs, &buyer(), 5).err(),
            Some(JobRefusal::NotTaken)
        );
        assert_eq!(
            check_collect(false, &jobs, &buyer(), 100).err(),
            Some(JobRefusal::NotAtBoard)
        );
        let mut cleared = Cleared::default();
        cleared.clear(100, 10.0);
        assert!(!jobs.settle(&cleared), "cleared before it was taken");
        cleared.clear(100, 60.0);
        assert!(jobs.settle(&cleared));
        assert!(check_collect(true, &jobs, &buyer(), 100).is_ok());
        // The camp returning does not undo it.
        assert!(!jobs.settle(&Cleared::default()));
        assert!(jobs.taken[&100].done);
    }

    /// A camp is found within `seen_m` or when it is fought, once.
    #[test]
    fn a_camp_is_found_by_coming_near_or_fighting() {
        let mut known = Known::default();
        let camps = [(1, [1000.0, 0.0]), (2, [3000.0, 0.0])];
        assert!(!find(&mut known, camps, [0.0, 0.0], 250.0, []));
        assert!(find(&mut known, camps, [800.0, 0.0], 250.0, []));
        assert!(known.is_found(1) && !known.is_found(2));
        assert!(!find(&mut known, camps, [800.0, 0.0], 250.0, []));
        assert!(find(&mut known, camps, [0.0, 0.0], 250.0, [2]));
        assert!(known.is_found(2));
    }

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
            .add_systems(Update, (consume, settle_jobs).chain());
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
                .0
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
        assert!(app.world().resource::<ProcurementInbox>().0.is_empty());

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

    /// Pelagic's first window on seed 42, printed by `--nocapture`, and held
    /// to be three camps of the world within reach, each paying its tier.
    #[test]
    fn pelagics_first_window_on_seed_42() {
        let (map, faction, _) = pelagic();
        let registries = shipped();
        let rules = rules();
        let camps = gather(map.world(), map, &registries.0, faction.seat, rules.reach_m);
        let shown = offered(
            &rules,
            &buyer(),
            faction.seat,
            &camps,
            &Cleared::default(),
            &Jobs::default(),
            0.0,
            Seed(42),
        );
        eprintln!(
            "{} camps within {} m of {:?}",
            camps.len(),
            rules.reach_m,
            faction.seat
        );
        for posting in &shown {
            let camp = camps.iter().find(|c| c.key == posting.key).unwrap();
            let km =
                (camp.centre[0] - faction.seat[0]).hypot(camp.centre[1] - faction.seat[1]) / 1000.0;
            eprintln!(
                "{} {} tier {} {} cr {} {:.1} km heard {:?}",
                posting.kind,
                posting.name,
                posting.tier,
                posting.pay,
                posting.faction,
                km,
                posting.heard
            );
        }
        assert!(
            camps.len() >= 9,
            "the nine camps of step 2a are within 20 km"
        );
        assert_eq!(shown.len(), 3);
        // Only a camp names a band; a wreck or a machine has none.
        let wreck = camps
            .iter()
            .find(|c| c.kind == "strata:wreck")
            .expect("a wreck lies within reach on seed 42");
        assert!(wreck.band.is_empty());
        assert!(
            shown
                .iter()
                .all(|p| p.pay > 0 && p.faction.is_empty() == (p.kind != "strata:wild_camp"))
        );
    }
}
