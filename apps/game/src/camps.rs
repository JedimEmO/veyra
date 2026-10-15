//! `--at camp`: a still's start on the trail into the outlaw camp nearest Port
//! Serein, facing it.

use bevy::prelude::*;
use strata::physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata::prelude::*;
use strata::registry::faction::ClaimKind;
use strata::sim::ProceduralWorldRes;
use strata::sim::combat::{BlasterBolt, Combatant};
use strata::sim::deposits::Survey;
use strata::sim::encounters::{
    Allegiance, Cleared, EncounterChanged, EncounterId, Fights, NearEncounters, PlayedClock, Slot,
    hostility,
};
use strata::sim::factions::Factions;
use strata::sim::sites::NearSites;
use strata::worldgen::factions::FactionId;

use super::Cli;
use super::combat::{Birth, spawn_hostile};

/// The outlaw camp's site kind.
const CAMP_KIND: &str = "strata:wild_camp";

/// The derelict machine's site kind, whose security wears grey metal.
const MACHINE_KIND: &str = "strata:wreck_works";

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
    let stand_off = if cli.cache {
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

/// The plate's words for a post's person: `Pelagic Works guard`.
fn plate_name(kind: ClaimKind, faction: &str) -> String {
    let key = if kind == ClaimKind::Reserve {
        "strata.post.warden"
    } else {
        "strata.post.guard"
    };
    let mut name = faction.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    strata::client::ui::strings::english(key)
        .unwrap_or(key)
        .replace("{faction}", &name)
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

/// The camps whose people have not all been given ground yet, each as its
/// camp and the index of a slot in its plan. A camp stands up 260 m or so
/// out, where the ground may not have streamed, so a slot waits here until
/// a ray finds it, as the staging fight's places do.
#[derive(Resource, Default)]
pub(super) struct Waiting(Vec<(EncounterId, usize)>);

/// What a standing encounter asks of the game: where its people stand, at
/// what tier, under which allegiance and in whose colour.
struct Stand<'a> {
    slots: &'a [Slot],
    tier: u8,
    allegiance: Allegiance,
    holder: Option<FactionId>,
    /// The words a plate says of a post's person, `None` for a camp's.
    post: Option<ClaimKind>,
    /// A post's deposit centre, which is never in water: a slot of its ring
    /// that is wet is drawn in towards it. `None` for a camp, whose pad is dry.
    centre: Option<[f64; 2]>,
}

/// The camp or post `id` names, as it now stands.
fn stand_of(near: &NearEncounters, id: EncounterId) -> Option<Stand<'_>> {
    if let Some(camp) = near.camps.iter().find(|camp| camp.id == id) {
        return Some(Stand {
            slots: &camp.slots,
            tier: camp.tier,
            allegiance: Allegiance::Outlaw(camp.band),
            holder: camp.band,
            post: None,
            centre: None,
        });
    }
    let post = near.posts.iter().find(|post| post.id == id)?;
    Some(Stand {
        slots: &post.slots,
        tier: post.tier,
        allegiance: Allegiance::Faction(post.faction),
        holder: Some(post.faction),
        post: Some(post.kind),
        centre: Some(post.centre),
    })
}

/// A post's person is the team its post's state says: neutral until the post
/// is crossed, then the hostile team, so a hit lands the same either way and
/// the enemy rule leaves an uncrossed guard out of every "hostile near" test.
pub(super) fn sync_post_teams(
    fights: Res<Fights>,
    mut people: Query<(&EncounterId, &Allegiance, &mut Combatant)>,
) {
    for (id, allegiance, mut actor) in &mut people {
        if !matches!(id, EncounterId::Post(_)) {
            continue;
        }
        let team = hostility(*allegiance, fights.crossed(*id));
        if actor.team != team {
            actor.team = team;
        }
    }
}

/// Turns what the sim stood up and folded into people and back. A folded
/// camp's or post's people and their bolts go; a standing camp's slots get their
/// ground as it streams, and a person is made with its tier's scaled stats,
/// its kind's brain, its holder's allegiance and colour, and no
/// `ResetPosition`, so F5 leaves a camp as it is. A post's people are named
/// for their faction and their post, and stand neutral.
#[allow(
    clippy::too_many_arguments,
    reason = "the sim's changes, the world's ground, and what is made"
)]
pub(super) fn stage_camps(
    mut commands: Commands,
    mut changes: MessageReader<EncounterChanged>,
    near: Res<NearEncounters>,
    mut waiting: ResMut<Waiting>,
    people: Query<(Entity, &EncounterId)>,
    bolts: Query<(Entity, &BlasterBolt)>,
    actors: Query<Entity, With<Combatant>>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    registries: Res<strata::sim::Registries>,
    factions: Option<Res<Factions>>,
) {
    for change in changes.read() {
        for id in &change.0.folded {
            waiting.0.retain(|(waiting, _)| waiting != id);
            let gone: Vec<Entity> = people
                .iter()
                .filter(|(_, person)| *person == id)
                .map(|(entity, _)| entity)
                .collect();
            for (bolt, shot) in &bolts {
                if gone.contains(&shot.owner) {
                    commands.entity(bolt).despawn();
                }
            }
            for entity in gone {
                commands.entity(entity).despawn();
            }
        }
        for id in &change.0.stood {
            if let Some(stand) = stand_of(&near, *id) {
                waiting
                    .0
                    .extend((0..stand.slots.len()).map(|index| (*id, index)));
            }
        }
    }
    if waiting.0.is_empty() {
        return;
    }
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    let mut pending = Vec::new();
    for (id, index) in waiting.0.drain(..) {
        let Some(stand) = stand_of(&near, id) else {
            continue;
        };
        let Slot { kind, at: [x, z] } = stand.slots[index];
        let (x, z) = stand
            .centre
            .map_or((x, z), |centre| dry_towards(&source, [x, z], centre));
        let Some(ground) = super::household::ground_at(x, z, &source, &origin, &spatial, &filter)
        else {
            pending.push((id, index));
            continue;
        };
        let birth = Birth {
            passive: false,
            kind,
            tier: stand.tier,
            id,
            allegiance: stand.allegiance,
        };
        let at = WorldPos::new(x, ground + super::PLAYER_DROP + 0.9, z);
        info!(
            "camp: {} stands at ({x:.0}, {z:.0}), tier {}",
            kind.name(),
            stand.tier
        );
        let mut person = spawn_hostile(&mut commands, at, birth, &registries.0);
        let holder = stand
            .holder
            .zip(factions.as_deref())
            .and_then(|(holder, factions)| factions.0.faction(holder));
        if let Some(holder) = holder {
            person.insert(strata::client::combat_art::BodyColour(holder.colour));
            if let Some(kind) = stand.post {
                person.insert(Name::new(plate_name(kind, &holder.name)));
            }
        } else {
            // An unbanded outlaw keeps the second household look, as before.
            person.insert(strata::client::combat_art::BodyTint(1));
            // Scavengers and security units are of no faction: the place's
            // kind words their plate, and a machine's people wear grey.
            let place = near
                .camps
                .iter()
                .find(|camp| camp.id == id)
                .and_then(|camp| registries.0.sites.get(camp.site.kind))
                .map(|def| def.id.to_string());
            if let Some(place) = place.as_deref() {
                if let Some(name) = crew_name(place) {
                    person.insert(Name::new(name));
                }
                if place == MACHINE_KIND {
                    person.insert(strata::client::combat_art::BodyColour(
                        strata::client::combat_art::MACHINE_STEEL,
                    ));
                }
            }
        }
    }
    waiting.0 = pending;
}

/// What a plate says of the unaligned crew of a place of `kind`, if the
/// kind has one (a camp's people keep their kind's name).
pub(super) fn crew_name(kind: &str) -> Option<&'static str> {
    strata::client::ui::strings::english(&format!("strata.crew.{kind}"))
}

/// The first dry point from `slot` in towards `centre`, in quarters of the
/// way; the centre itself when every step is wet.
fn dry_towards(source: &WorldSourceRes, slot: [f64; 2], centre: [f64; 2]) -> (f64, f64) {
    let world = source.source();
    [1.0, 0.75, 0.5, 0.25]
        .into_iter()
        .map(|share| {
            (
                centre[0] + (slot[0] - centre[0]) * share,
                centre[1] + (slot[1] - centre[1]) * share,
            )
        })
        .find(|&(x, z)| !world.sample(x, z).in_water())
        .unwrap_or((centre[0], centre[1]))
}

/// `--cleared`: the camp nearest the member is quiet from the first frame,
/// once, so a still shows it with nobody standing. `--cache` clears it the
/// same way and leaves its cache. Runs before the sim reads the ledger.
#[allow(
    clippy::too_many_arguments,
    reason = "the sites, the ledgers a clearing writes and the world a cache is drawn from"
)]
pub(super) fn clear_nearest(
    cli: Res<Cli>,
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "the scenario owns its world")]

    use super::*;
    use clap::Parser;

    /// The unaligned crews are worded by their place; a camp's are not.
    #[test]
    fn the_crew_of_a_wreck_and_a_machine_have_plates_of_their_own() {
        assert_eq!(crew_name("strata:wreck"), Some("Scavenger"));
        assert_eq!(crew_name("strata:wreck_works"), Some("Security unit"));
        assert_eq!(crew_name(CAMP_KIND), None);
    }

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
