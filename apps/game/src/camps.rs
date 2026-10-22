//! `--at camp`: a still's start on the trail into the outlaw camp nearest Port
//! Serein, facing it, and the stills that clear or empty a camp before the sim
//! reads the ledger. The camps' people are `veyra_game::camps`.

use bevy::prelude::*;
use strata::registry::faction::ClaimKind;
use strata::sim::ProceduralWorldRes;
use strata::sim::deposits::Survey;
use strata::sim::factions::Factions;
use veyra_game::Launch;
use veyra_game::camps::{CAMP_KIND, MACHINE_KIND};

use super::Cli;

/// The kind of site `--at camp`, `--at wreck` and `--at machine` look for.
fn site_kind_of(at: &str) -> Option<&'static str> {
    match at {
        "camp" => Some(CAMP_KIND),
        "wreck" => Some("strata:wreck"),
        "machine" => Some(MACHINE_KIND),
        _ => None,
    }
}

/// How far from the site's centre the member stands, on the side its trail
/// arrives from: outside the yard, with the camp whole in view.
const STAND_OFF_M: f64 = 120.0;

/// How far from the centre `--at camp --cache` stands: within reach of the
/// chest, so its F chip is up.
const CACHE_STAND_OFF_M: f64 = 2.0;

/// How far `--at camp --cache` turns from the chest, radians, so the chest
/// stands beside the character and not behind them.
const CACHE_YAW_OFFSET: f32 = 0.6;

/// How far from Port Serein the search for a site of the kind reaches.
const SEARCH_M: f64 = 40_000.0;

/// The place and heading `--at camp` (or `wreck`, `machine`) gives:
/// [`STAND_OFF_M`] from the site of that kind nearest Port Serein, out along
/// its own trail, looking back at the centre. Where the trail is missing or short it stands toward the site's
/// arrival point instead. `None` when the world has no such site in reach.
fn view(
    world: &strata::worldgen::ProceduralWorld,
    kind_name: &str,
    stand_off: f64,
) -> Option<((f64, f64), f32)> {
    use strata::worldgen::roads::RoadEnd;
    let kind = world
        .site_kinds()
        .iter()
        .find(|kind| kind.name.to_string() == kind_name)?
        .id;
    let [cx, cz] = world.sites().first()?.centre;
    let dist = |site: &strata::worldgen::sites::ProceduralSite| {
        (site.centre[0] - cx).hypot(site.centre[1] - cz)
    };
    let site = world
        .sites_near(cx, cz, SEARCH_M)
        .into_iter()
        .filter(|site| site.kind() == kind)
        .min_by(|a, b| dist(a).total_cmp(&dist(b)))?;
    // Close in, the trail's own points are too far apart to stand on.
    let on_trail = (stand_off >= STAND_OFF_M)
        .then_some(world)
        .and_then(|world| {
            world
                .hydrology()
                .network_at(world, site.centre[0], site.centre[1])
                .and_then(|network| {
                    let road = network
                        .roads()
                        .iter()
                        .find(|road| road.ends.contains(&RoadEnd::Site(site.id)))?;
                    let mut points: Vec<(f64, f64)> =
                        road.points.iter().map(|p| (p.x, p.z)).collect();
                    if road.ends[1] == RoadEnd::Site(site.id) {
                        points.reverse();
                    }
                    points
                        .into_iter()
                        .find(|p| (p.0 - site.centre[0]).hypot(p.1 - site.centre[1]) >= stand_off)
                })
        });
    let at = on_trail.unwrap_or_else(|| {
        let (dx, dz) = (
            site.arrival[0] - site.centre[0],
            site.arrival[1] - site.centre[1],
        );
        let len = dx.hypot(dz).max(f64::EPSILON);
        (
            site.centre[0] + dx / len * stand_off,
            site.centre[1] + dz / len * stand_off,
        )
    });
    // Facing is `(-sin yaw, -cos yaw)`, toward the centre.
    let (ux, uz) = (at.0 - site.centre[0], at.1 - site.centre[1]);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "heading is render-space f32"
    )]
    let yaw = ux.atan2(uz) as f32;
    let yaw = if stand_off < STAND_OFF_M {
        yaw + CACHE_YAW_OFFSET
    } else {
        yaw
    };
    Some((at, yaw))
}

/// Turns `--at camp` into a `--spawn` and a `--look` once the world exists,
/// so the spawn systems need no knowledge of it.
pub(super) fn resolve_at(
    world: Option<Res<ProceduralWorldRes>>,
    survey: Option<Res<Survey>>,
    factions: Option<Res<Factions>>,
    mut cli: ResMut<Cli>,
) {
    if let Some(wanted) = cli.at.as_deref().and_then(claim_spec) {
        let wanted = wanted.map(str::to_owned);
        cli.at = None;
        let found = world
            .as_deref()
            .zip(survey.as_deref())
            .zip(factions.as_deref());
        if let Some(((world, survey), factions)) = found
            && let Some((at, yaw)) = claim_view(&world.0, survey, factions, wanted.as_deref())
        {
            info!("claim: standing at ({:.0}, {:.0})", at.0, at.1);
            cli.spawn = Some(at);
            cli.look = Some(yaw);
        } else {
            warn!("no claim to stand before; spawning by --spawn");
        }
        return;
    }
    let Some(at_word) = cli.at.clone() else {
        return;
    };
    let Some(kind_name) = site_kind_of(&at_word) else {
        return;
    };
    cli.at = None;
    let Some(world) = world else {
        return;
    };
    let stand_off = if cli.cache() {
        CACHE_STAND_OFF_M
    } else {
        STAND_OFF_M
    };
    if let Some((at, yaw)) = view(&world.0, kind_name, stand_off) {
        info!("{at_word}: standing at ({:.0}, {:.0})", at.0, at.1);
        cli.spawn = Some(at);
        cli.look = Some(yaw);
    } else {
        warn!("no {at_word} site within {SEARCH_M} m; spawning by --spawn");
    }
}

/// What [`resolve_at`] settled of `--at`, `--spawn` and `--look` is copied to
/// the headless side's [`Launch`], which the people's start reads, so the two
/// agree on where the run begins. Right after `resolve_at`, before the
/// household's start.
pub(super) fn sync_launch(cli: Res<Cli>, mut launch: ResMut<Launch>) {
    launch.at.clone_from(&cli.at);
    launch.spawn = cli.spawn;
}

/// Whether `--at` asks for a claim, and the faction it names: `claim` is the
/// nearest concession, `claim:<name>` the nearest of a faction's.
#[allow(
    clippy::option_option,
    reason = "outer: asks for a claim at all; inner: names a faction"
)]
pub(super) fn claim_spec(at: &str) -> Option<Option<&str>> {
    match at.strip_prefix("claim") {
        Some("") => Some(None),
        Some(rest) => rest.strip_prefix(':').map(Some),
        None => None,
    }
}

/// How far from the deposit's centre `--at claim` stands: outside the
/// post's ring, with its people in view.
const CLAIM_STAND_OFF_M: f64 = 40.0;

/// The place and heading `--at claim` gives: [`CLAIM_STAND_OFF_M`] from the
/// nearest concession to Port Serein (of the faction `wanted` names, when
/// one is), on the city's side where that is dry, looking at its centre.
fn claim_view(
    world: &strata::worldgen::ProceduralWorld,
    survey: &Survey,
    factions: &Factions,
    wanted: Option<&str>,
) -> Option<((f64, f64), f32)> {
    use strata::prelude::WorldSource;
    let [cx, cz] = world.sites().first()?.centre;
    let held_by = |deposit: &strata::sim::deposits::Deposit| {
        let claim = factions.0.claim_of(deposit)?;
        let faction = factions.0.faction(claim.holder)?;
        (claim.kind == ClaimKind::Concession
            && wanted.is_none_or(|name| faction.name.eq_ignore_ascii_case(name)))
        .then_some(())
    };
    let range = survey.0.set().rules().register.range_m;
    let deposit = survey
        .0
        .nearest(cx, cz, range, |deposit| held_by(deposit).is_some())?;
    let sea = f64::from(world.sea_level());
    let base = (cz - deposit.at[1]).atan2(cx - deposit.at[0]);
    let at = (0..8)
        .map(|step| base + f64::from(step) * std::f64::consts::FRAC_PI_4)
        .map(|angle| {
            (
                deposit.at[0] + angle.cos() * CLAIM_STAND_OFF_M,
                deposit.at[1] + angle.sin() * CLAIM_STAND_OFF_M,
            )
        })
        .find(|&(x, z)| {
            let sample = world.sample(x, z);
            !sample.in_water() && f64::from(sample.height) > sea
        })?;
    let (ux, uz) = (at.0 - deposit.at[0], at.1 - deposit.at[1]);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "heading is render-space f32"
    )]
    let yaw = ux.atan2(uz) as f32;
    info!(
        "claim: {} at ({:.0}, {:.0})",
        deposit.place, deposit.at[0], deposit.at[1]
    );
    Some((at, yaw))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "the scenario owns its world")]

    use super::*;
    use clap::Parser;
    use strata::prelude::*;
    use strata::sim::combat::Combatant;
    use strata::sim::encounters::NearEncounters;
    use strata::sim::encounters::{Allegiance, EncounterId};

    /// A member who stands at a camp's door has its tier's people stood up
    /// with their allegiance and ids, and when the member is moved far off
    /// with nobody fighting they are folded away.
    #[allow(
        clippy::too_many_lines,
        reason = "one scenario in one world, which takes long to stand up"
    )]
    #[test]
    fn a_camp_stands_its_tiers_people_and_folds_them_when_the_member_leaves() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("household.ron");
        let mut cli = Cli::parse_from([
            "game",
            "--headless",
            "--no-hostiles",
            "--fresh",
            "--household",
            path.to_str().unwrap(),
            "--spawn",
            "4943,-3111",
        ]);
        cli.resolve_play_mode();
        crate::install_combat_tables(&cli).unwrap();
        let mut app = crate::build(cli, None).unwrap();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 60.),
        ));
        app.finish();
        app.cleanup();
        let count = |app: &mut App| {
            app.world_mut()
                .query::<&EncounterId>()
                .iter(app.world())
                .filter(|id| matches!(id, EncounterId::Camp(_)))
                .count()
        };
        let started = std::time::Instant::now();
        let wanted = loop {
            app.update();
            let near = app.world().resource::<NearEncounters>();
            if let Some(camp) = near.camps.first() {
                let registries = &app.world().resource::<strata::sim::Registries>().0;
                let def = registries.encounters.iter().next().unwrap();
                let wanted: usize = def
                    .roster(camp.tier)
                    .iter()
                    .map(|person| usize::from(person.count))
                    .sum();
                if count(&mut app) == wanted {
                    break wanted;
                }
            }
            assert!(
                started.elapsed().as_secs() < 120,
                "the camp's people never stood"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert!(wanted > 0);
        let allegiances = app
            .world_mut()
            .query::<(&EncounterId, &Allegiance)>()
            .iter(app.world())
            .filter(|(id, _)| matches!(id, EncounterId::Camp(_)))
            .all(|(_, allegiance)| matches!(allegiance, Allegiance::Outlaw(_)));
        assert!(allegiances, "a camp's people are outlaws");
        // One of them is hurt: the camp engages, and its rusher comes for the
        // member, the group's own fight and not the staging one.
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        let from_member = |app: &mut App| {
            let me = app.world().get::<WorldPosition>(player).unwrap().0;
            app.world_mut()
                .query::<(&EncounterId, &Combatant, &WorldPosition)>()
                .iter(app.world())
                .filter(|(id, _, _)| matches!(id, EncounterId::Camp(_)))
                .map(|(_, _, at)| at.0.distance(me))
                .fold(f64::MAX, f64::min)
        };
        let before = from_member(&mut app);
        for (id, mut actor) in app
            .world_mut()
            .query::<(&EncounterId, &mut Combatant)>()
            .iter_mut(app.world_mut())
        {
            if matches!(id, EncounterId::Camp(_)) {
                actor.hurts = 1;
                break;
            }
        }
        for _ in 0..600 {
            app.update();
        }
        let fights = app.world().resource::<strata::sim::encounters::Fights>();
        let camp = app.world().resource::<NearEncounters>().camps[0].id;
        assert!(fights.engaged(camp), "a hurt person engages its camp");
        assert!(!fights.engaged(EncounterId::Staging));
        let after = from_member(&mut app);
        assert!(
            after < before - 1.0,
            "a camp's people close on the member: {before:.1} m then {after:.1} m"
        );
        // Engaged, a camp holds while the member runs; after a fall and an
        // F5 the fight is let go and the camp folds once the member is far.
        app.world_mut()
            .resource_mut::<strata::sim::encounters::Fights>()
            .abandon_camps();
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<WorldPosition>(player).unwrap().0 =
            WorldPos::new(4_943.0, 100.0, -3_111.0 - 2_000.0);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(count(&mut app), 0, "folded away");
        assert!(app.world().resource::<NearEncounters>().camps.is_empty());
    }

    /// A member who stands before a concession has its guards stood up on the
    /// neutral team, named for their faction; a blow on one turns the post
    /// to the hostile team at once, and the guards fold when the member is
    /// moved far off with nobody fighting.
    #[allow(
        clippy::too_many_lines,
        reason = "one scenario in one world, which takes long to stand up"
    )]
    #[test]
    fn a_post_stands_neutral_guards_at_a_claim_and_a_blow_turns_them() {
        use strata::sim::combat::CombatImpact;
        use strata::sim::encounters::NEUTRAL_TEAM;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("household.ron");
        let mut cli = Cli::parse_from([
            "game",
            "--headless",
            "--no-hostiles",
            "--fresh",
            "--household",
            path.to_str().unwrap(),
            "--at",
            "claim",
        ]);
        cli.resolve_play_mode();
        crate::install_combat_tables(&cli).unwrap();
        let mut app = crate::build(cli, None).unwrap();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 60.),
        ));
        app.finish();
        app.cleanup();
        let count = |app: &mut App| {
            app.world_mut()
                .query::<&EncounterId>()
                .iter(app.world())
                .filter(|id| matches!(id, EncounterId::Post(_)))
                .count()
        };
        let started = std::time::Instant::now();
        let wanted = loop {
            app.update();
            let near = app.world().resource::<NearEncounters>();
            if let Some(post) = near.posts.first() {
                let registries = &app.world().resource::<strata::sim::Registries>().0;
                let def = registries.encounters.at_claim(post.kind).unwrap();
                let wanted: usize = def
                    .roster(post.tier)
                    .iter()
                    .map(|person| usize::from(person.count))
                    .sum();
                if count(&mut app) == wanted {
                    break wanted;
                }
            }
            assert!(
                started.elapsed().as_secs() < 120,
                "the post's people never stood"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert!(wanted > 0);
        let post = app.world().resource::<NearEncounters>().posts[0].id;
        let mut people = app
            .world_mut()
            .query::<(Entity, &EncounterId, &Allegiance, &Combatant, &Name)>();
        let stood: Vec<_> = people
            .iter(app.world())
            .filter(|(_, id, ..)| **id == post)
            .map(|(entity, _, allegiance, actor, name)| {
                (
                    entity,
                    matches!(allegiance, Allegiance::Faction(_)),
                    actor.team,
                    name.to_string(),
                )
            })
            .collect();
        assert_eq!(stood.len(), wanted);
        assert!(
            stood.iter().all(|(_, faction, team, name)| *faction
                && *team == NEUTRAL_TEAM
                && name.ends_with("guard")),
            "guards of a faction, neutral, named for their post: {stood:?}"
        );
        // A member's blow on one of them crosses the post.
        let member = app
            .world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        let point = WorldPos::new(0., 0., 0.);
        app.world_mut().write_message(CombatImpact {
            attacker: member,
            victim: Some(stood[0].0),
            from: point,
            point,
            melee: false,
            dealt: 5.0,
            lethal: false,
            guard: strata::sim::combat::GuardOutcome::Open,
            shot: None,
        });
        for _ in 0..10 {
            app.update();
        }
        assert!(
            app.world()
                .resource::<strata::sim::encounters::Fights>()
                .crossed(post)
        );
        let teams: Vec<u8> = people
            .iter(app.world())
            .filter(|(_, id, ..)| **id == post)
            .map(|(_, _, _, actor, _)| actor.team)
            .collect();
        assert!(
            teams.iter().all(|team| *team == 1),
            "a crossed post's people are the hostile team: {teams:?}"
        );
        // Far off with nobody fighting once it is let go, the post folds.
        app.world_mut()
            .resource_mut::<strata::sim::encounters::Fights>()
            .abandon_camps();
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<WorldPosition>(player).unwrap().0 =
            WorldPos::new(-1_153.0, 100.0, 90.0 + 2_000.0);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(count(&mut app), 0, "folded away");
    }
}
