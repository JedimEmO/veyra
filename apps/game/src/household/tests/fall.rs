//! Tests of a fall in the world (ticket 0012, step 4a): the member is carried
//! home hurt, the next able member goes out, nothing carried is lost, the
//! injury is saved, and the staging fight keeps F5.

use strata::sim::encounters::{Fights, NearEncounters};
use strata::sim::loot::{Cache, Caches};

use super::super::bench::Out;
use super::super::cache::changes;
use super::super::fall::{
    carry_home, engaged_in_the_world, in_the_world, injured_minutes, injured_refusal,
};
use super::*;

const NOW: f64 = 10_000.0;

fn home() -> WorldPos {
    WorldPos::new(40., 5., -60.)
}

/// The household with its bench full, the player's parts for whoever is out,
/// and the fall's dials as shipped.
fn bench(registries: &GameRegistries) -> Household {
    let mut household = Household::starting(registries, &Grants::default());
    join_all(&mut household);
    assert!(household.characters.len() > 2, "a bench of several");
    household
}

macro_rules! fall {
    ($household:expr, $registries:expr, $worker:expr) => {{
        let (mut actor, mut sheet, mut name, mut tint, mut pack, mut hands) =
            $household.stats($registries);
        let mut bar = $household.bar();
        let mut input = CombatIntent::default();
        let carried = pack.saved(&$registries.items).unwrap();
        let rules = $registries.fall.clone().expect("fall.ron ships");
        let report = carry_home(
            &mut $household,
            &rules,
            NOW,
            $worker,
            $registries,
            Out {
                position: WorldPos::new(1., 2., 3.),
                actor: &mut actor,
                sheet: &mut sheet,
                input: &mut input,
                name: &mut name,
                tint: &mut tint,
                pack: &mut pack,
                hands: &mut hands,
                bar: &mut bar,
            },
            home(),
        );
        (report, carried, actor)
    }};
}

/// A fall hurts the member for the dial's hours, leaves them at the bench
/// place with everything they carried, and puts the next able member out
/// with full pools.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_fall_brings_the_member_home_hurt_and_the_next_able_one_out() {
    let registries = shipped();
    let mut household = bench(&registries);
    let hours = registries.fall.as_ref().unwrap().out_s();
    let (fallen, next) = (household.active, 1);
    let (report, carried, actor) = fall!(household, &registries, None);
    let before = &household.characters[fallen];
    assert_eq!(before.injured_until, Some(NOW + hours));
    assert_eq!(before.left_at, Some(home()), "left at the bench place");
    assert_eq!(before.pack, carried, "nothing carried is lost");
    assert_eq!(household.active, next);
    assert_eq!(household.characters[next].injured_until, None);
    assert!(actor.health > 0., "the one who is out starts whole");
    assert_eq!(report.fallen, before.name);
    assert!(matches!(
        report.cover,
        strata::client::ui::loot::FallCover::Able(ref name) if *name == household.characters[next].name
    ));
    assert_eq!(injured_minutes(before, NOW), Some(120));
    assert!(injured_refusal(before, NOW).is_some_and(|said| said.contains(&before.name)));
    assert_eq!(injured_refusal(before, NOW + hours), None, "healed");
}

/// With nobody able the soonest to heal is out early; alone, the fallen
/// member is back whole.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn with_nobody_able_the_soonest_gets_up_early() {
    use strata::client::ui::loot::FallCover;
    let registries = shipped();
    let mut household = bench(&registries);
    let others = household.characters.len() - 1;
    for (index, character) in household.characters.iter_mut().enumerate().skip(1) {
        character.injured_until =
            Some(NOW + 5_000.0 + 1_000.0 * f64::from(u8::try_from(index).expect("a small bench")));
    }
    let (report, _, _) = fall!(household, &registries, None);
    assert_eq!(household.active, 1, "the soonest to heal");
    assert!(matches!(report.cover, FallCover::Early(_)));
    assert_eq!(
        injured_minutes(&household.characters[1], NOW),
        None,
        "stood up, at the dial's minutes"
    );
    assert!(others > 1);

    let mut alone = Household::starting(&registries, &Grants::default());
    assert_eq!(alone.characters.len(), 1);
    let (report, _, _) = fall!(alone, &registries, None);
    assert_eq!(report.cover, FallCover::Alone);
    assert_eq!(alone.characters[0].injured_until, None);
}

/// The fabricator's worker is not sent out in the fallen member's place.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_fabricators_worker_is_passed_over() {
    let registries = shipped();
    let mut household = bench(&registries);
    let worker = household.characters[1].name.clone();
    fall!(household, &registries, Some(worker.as_str()));
    assert_eq!(household.active, 2);
}

/// The injury is in the file and reads back.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_injury_is_saved_and_read_back() {
    let registries = shipped();
    let mut household = bench(&registries);
    fall!(household, &registries, None);
    let text = ron::ser::to_string(&household).unwrap();
    let (read, ..) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(
        read.characters[0].injured_until,
        household.characters[0].injured_until
    );
    assert!(read.characters[0].injured_until.is_some());
    assert_eq!(read.active, household.active);
}

/// A fight at the staging ground is not a fall in the world: F5 stays as it
/// was. Only an engaged camp or post is.
#[test]
fn the_staging_fight_is_not_a_fall_in_the_world() {
    let mut fights = Fights::default();
    let near = NearEncounters::default();
    assert!(!in_the_world(&fights, &near));
    let staging = strata::sim::encounters::EncounterId::Staging;
    fights.observe(
        [strata::sim::encounters::Present {
            id: staging,
            hurt: true,
            alive: true,
            passive: false,
            distance: 3.,
        }],
        30.,
        false,
    );
    assert!(fights.engaged(staging));
    assert!(!in_the_world(&fights, &near), "staging alone keeps F5");
}

/// The chest stands for a cache held and goes when it is emptied.
#[test]
fn the_cache_prop_follows_the_caches() {
    let mut caches = Caches::default();
    assert_eq!(changes(&caches, &[]), (vec![], vec![]));
    caches.by.insert(
        7,
        Cache {
            encounter: "strata:outlaw_camp".parse().expect("a valid id"),
            credits: 90,
            records: Vec::new(),
        },
    );
    assert_eq!(
        changes(&caches, &[]),
        (vec![], vec![7]),
        "left by a clearing"
    );
    assert_eq!(changes(&caches, &[7]), (vec![], vec![]), "standing");
    caches.by.clear();
    assert_eq!(changes(&caches, &[7]), (vec![7], vec![]), "emptied");
}

/// A camp engaged is a fall in the world; a staging fight engaged beside it
/// is not, whatever the camp's wounded person is doing.
#[test]
fn an_engaged_camp_is_a_fall_in_the_world_unless_staging_is_engaged() {
    use strata::sim::encounters::{EncounterId, Present};
    let camp = EncounterId::Camp(5);
    let present = |id| Present {
        id,
        hurt: true,
        alive: true,
        passive: false,
        distance: 3.,
    };
    let mut fights = Fights::default();
    fights.observe([present(camp)], 30., false);
    assert!(engaged_in_the_world(&fights, [camp].into_iter()));
    fights.observe([present(camp), present(EncounterId::Staging)], 30., false);
    assert!(!engaged_in_the_world(&fights, [camp].into_iter()));
}

/// The caches are kept in the file and handed to the sim only in the world
/// they belong to.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_caches_are_saved_and_handed_over_only_when_live() {
    use super::super::cache::stood_caches;
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    household.caches.by.insert(
        9,
        Cache {
            encounter: "strata:outlaw_camp".parse().expect("a valid id"),
            credits: 120,
            records: Vec::new(),
        },
    );
    let text = ron::ser::to_string(&household).unwrap();
    let (read, ..) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(read.caches, household.caches);
    assert_eq!(stood_caches(&read, true), household.caches);
    assert!(stood_caches(&read, false).by.is_empty());
    assert_eq!(read.caches.by.len(), 1, "kept in the file");
}

/// A taken job and a found camp are kept in the file and handed to the sim
/// only in the world they belong to.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn jobs_and_found_camps_round_trip_the_household_file() {
    use super::super::cache::stood_jobs;
    use strata::sim::postings::{Heard, Job, Posting};
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    household.jobs.taken.insert(
        9,
        Job {
            posting: Posting {
                key: 9,
                name: "Snogirk Cut".to_owned(),
                tier: 1,
                pay: 400,
                faction: "the Skan Gang".to_owned(),
                kind: "strata:wild_camp".to_owned(),
                heard: Heard {
                    centre: [10.0, -20.0],
                    radius: 900.0,
                },
            },
            buyer: "veyra:buyer.pelagic_works".parse().unwrap(),
            taken_at: 12.0,
            done: true,
        },
    );
    household.known.camps.insert(4);
    let text = ron::ser::to_string(&household).unwrap();
    let (read, ..) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(read.jobs, household.jobs);
    assert_eq!(read.known, household.known);
    let (jobs, known) = stood_jobs(&read, true);
    assert_eq!((&jobs, &known), (&household.jobs, &household.known));
    let (jobs, known) = stood_jobs(&read, false);
    assert!(jobs.taken.is_empty() && known.camps.is_empty());
    assert_eq!(read.jobs.taken.len(), 1, "kept in the file");
}
