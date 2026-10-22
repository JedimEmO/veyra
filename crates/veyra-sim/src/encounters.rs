//! The adapter half of the `encounters` rules: its requests, systems and plugin.

pub use veyra_rules::encounters::*;

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Plugin, Update};

use bevy::ecs::prelude::*;

use bevy::state::condition::in_state;

use strata_core::Seed;

use strata_registry::ClaimKind;

use strata_worldgen::sites::SiteId;

use veyra_world::deposits::{Deposit, DepositId};

use veyra_world::factions::Claim;

use crate::combat::{Action, CombatImpact, Combatant};

pub use crate::combat::{NEUTRAL_TEAM, enemies};

use crate::combat_tables::current as combat_tables;

use crate::deeds::{DeedKind, Deeds, Leases, Ledger, Stance, stance_of};

use crate::deposits::{NearDeposits, Survey};

use crate::factions::{FactionId, Factions};

use crate::factory::nominal_ticks;

use crate::loot::Caches;

use crate::progression::{CharacterSheet, Interaction, InteractionKind};

use crate::sites::NearSites;

pub use crate::standing::{EncounterId, Fights, PlayedClock, Present, Warning, post_key};

use crate::{
    AppState, LocalPlayer, Registries, SimClock, SimSet, SimTick, WorldPosition, WorldSeed,
};

use strata_core::SIM_TICK_HZ;

/// `SimTick`: advance the played clock by nominal time.
pub fn advance_clock(
    sim: Option<Res<SimClock>>,
    mut clock: ResMut<PlayedClock>,
    mut phase: Local<u32>,
) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let ticks = nominal_ticks(&mut phase, stepped);
    clock.seconds += f64::from(ticks) / SIM_TICK_HZ;
}

/// `Update`: a standing camp whose people are all down is cleared at the
/// played second now. The staging fight is never recorded.
#[allow(
    clippy::too_many_arguments,
    reason = "the people, the clock, the ledgers, the world for a cache's draw and the deeds"
)]
pub fn mark_cleared(
    people: Query<(&EncounterId, &Combatant)>,
    clock: Res<PlayedClock>,
    mut near: ResMut<NearEncounters>,
    mut cleared: ResMut<Cleared>,
    registries: Option<Res<Registries>>,
    seed: Option<Res<WorldSeed>>,
    (survey, deposits): (Option<Res<Survey>>, Option<Res<NearDeposits>>),
    mut caches: ResMut<Caches>,
    mut ledger: Ledger,
) {
    if near.camps.iter().all(|camp| camp.cleared) && near.posts.iter().all(|post| post.cleared) {
        return;
    }
    let mut left: BTreeMap<EncounterId, (usize, usize)> = BTreeMap::new();
    for (id, actor) in &people {
        let entry = left.entry(*id).or_default();
        entry.0 += 1;
        entry.1 += usize::from(actor.action != Action::Dead);
    }
    for camp in near.camps.iter_mut().filter(|camp| !camp.cleared) {
        let Some((total, alive)) = left.get(&camp.id).copied() else {
            continue;
        };
        camp.seen_alive |= alive > 0;
        if camp.seen_alive && alive == 0 && total >= camp.slots.len() {
            camp.cleared = true;
            cleared.clear(camp.site.key(), clock.seconds);
            if let Some(registries) = &registries {
                ledger.cleared(&registries.0, camp.centre);
                leave_cache(
                    &registries.0,
                    camp.site.kind,
                    (camp.site.key(), camp.centre, camp.tier),
                    seed.as_deref().map_or(Seed(0), |seed| seed.0),
                    (survey.as_deref(), deposits.as_deref()),
                    clock.seconds,
                    &mut caches,
                );
            }
        }
    }
    // A post is quiet for its hours like a camp, but it was never an
    // outlaw camp: no deed for the factions about it.
    for post in near.posts.iter_mut().filter(|post| !post.cleared) {
        let Some((total, alive)) = left.get(&post.id).copied() else {
            continue;
        };
        post.seen_alive |= alive > 0;
        if post.seen_alive && alive == 0 && total >= post.slots.len() {
            post.cleared = true;
            cleared.clear(post_key(post.deposit), clock.seconds);
        }
    }
}

/// `Update`: [`refresh`] and [`refresh_posts`] for the local player. A
/// camp's candidates come from [`NearSites`], which is made off the frame;
/// what is decided here is a distance per camp. A post's are the deposits of
/// [`NearDeposits`] within reach of the member, whose claim is asked once
/// and kept.
#[allow(
    clippy::too_many_arguments,
    reason = "the readings and ledgers refresh takes"
)]
pub fn update_near_encounters(
    registries: Option<Res<Registries>>,
    sites: Option<Res<NearSites>>,
    deposits: Option<Res<NearDeposits>>,
    factions: Option<Res<Factions>>,
    clock: Res<PlayedClock>,
    mut fights: ResMut<Fights>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    mut near: ResMut<NearEncounters>,
    mut cleared: ResMut<Cleared>,
    mut caches: ResMut<Caches>,
    mut changes: MessageWriter<EncounterChanged>,
    mut claims: Local<BTreeMap<DepositId, Option<Claim>>>,
) {
    let (Some(registries), Some(sites), Ok(position)) = (registries, sites, player.single()) else {
        return;
    };
    let band = |id: &SiteId| {
        factions
            .as_ref()
            .and_then(|factions| factions.0.holder_of(id))
    };
    let here = [position.0.x(), position.0.z()];
    // Quiet is forgotten every pass but changes rarely: the ledger is marked
    // only when a record went, so what watches it is not woken every frame.
    let before = cleared.at.len();
    let mut made = refresh(
        &mut near,
        &sites.sites,
        &registries.0.encounters,
        here,
        clock.seconds,
        cleared.bypass_change_detection(),
        &fights,
        band,
    );
    if cleared.at.len() != before {
        cleared.set_changed();
    }
    // A camp that returned takes what it left with it.
    let held = caches.by.len();
    caches
        .bypass_change_detection()
        .prune(&cleared, clock.seconds, &registries.0.encounters);
    if caches.by.len() != held {
        caches.set_changed();
    }
    if let (Some(deposits), Some(factions)) = (deposits, factions.as_ref()) {
        let reach = registries
            .0
            .encounters
            .iter()
            .filter(|def| def.at.claim().is_some())
            .map(|def| f64::from(def.stand_m))
            .fold(0.0, f64::max);
        let claimed: Vec<(Deposit, Claim)> = deposits
            .deposits
            .iter()
            .filter(|deposit| distance(deposit.at, here) <= reach)
            .filter_map(|deposit| {
                let claim = *claims
                    .entry(deposit.id)
                    .or_insert_with(|| factions.0.claim_of(deposit));
                Some((deposit.clone(), claim?))
            })
            .collect();
        let posts = refresh_posts(
            &mut near,
            &claimed,
            &registries.0.encounters,
            here,
            clock.seconds,
            &cleared,
            &fights,
        );
        apply_post_changes(&mut fights, &posts);
        made.stood.extend(posts.stood);
        made.folded.extend(posts.folded);
    }
    if !made.stood.is_empty() || !made.folded.is_empty() {
        changes.write(EncounterChanged(made));
    }
}

/// `Update`: [`watch`] for the local player.
#[allow(
    clippy::too_many_arguments,
    reason = "the readings the pass takes and the message it writes"
)]
pub fn watch_posts(
    registries: Option<Res<Registries>>,
    factions: Option<Res<Factions>>,
    deeds: Option<Res<Deeds>>,
    leases: Option<Res<Leases>>,
    clock: Res<PlayedClock>,
    near: Res<NearEncounters>,
    mut fights: ResMut<Fights>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    people: Query<(&EncounterId, &WorldPosition)>,
    mut turned: MessageWriter<PostTurned>,
) {
    let (Some(registries), Some(factions), Ok(position)) = (registries, factions, player.single())
    else {
        return;
    };
    if near.posts.is_empty() {
        return;
    }
    let standing: Vec<(EncounterId, [f64; 2])> = people
        .iter()
        .filter(|(id, _)| matches!(id, EncounterId::Post(_)))
        .map(|(id, at)| (*id, [at.0.x(), at.0.z()]))
        .collect();
    let none = Leases::default();
    let leases = leases.as_deref().unwrap_or(&none);
    let books = deeds.as_deref().cloned().unwrap_or_default();
    let now = clock.seconds;
    let world = Watching {
        posts: &near.posts,
        player: [position.0.x(), position.0.z()],
        now,
        engage_m: f64::from(combat_tables().opponents.engage_distance),
        people: &standing,
    };
    // Reading the guards' stance costs a faction lookup, so only a post that
    // is neutral asks it.
    let hostile = |id: FactionId| {
        factions.0.faction(id).is_some_and(|faction| {
            stance_of(&registries.0, &books, &faction, now) == Stance::Hostile
        })
    };
    let made = watch(&mut fights, &world, |id| leases.live(id, now), hostile);
    for (post, faction, by) in made {
        turned.write(PostTurned { post, faction, by });
    }
}

/// `Update`: every uncrossed guard of a concession carries an
/// [`Interaction`] that sells its faction's lease within `guard_reach_m`;
/// one that turns, or a warden, carries none. The game stands the bodies;
/// the sim says what they offer.
pub fn tend_guards(
    registries: Option<Res<Registries>>,
    near: Res<NearEncounters>,
    fights: Res<Fights>,
    people: Query<(Entity, &EncounterId, &Allegiance, Has<Interaction>)>,
    mut commands: Commands,
) {
    let Some(reach) = registries
        .as_ref()
        .and_then(|registries| registries.0.factions.claims())
        .map(|claims| claims.guard_reach_m)
    else {
        return;
    };
    for (entity, id, allegiance, has) in &people {
        let Allegiance::Faction(faction) = allegiance else {
            continue;
        };
        let sells = near.posts.iter().any(|post| {
            post.id == *id && post.kind == ClaimKind::Concession && !fights.crossed(post.id)
        });
        if sells && !has {
            commands.entity(entity).insert(Interaction {
                kind: InteractionKind::Guard { faction: *faction },
                radius: reach,
            });
        } else if !sells && has {
            commands.entity(entity).remove::<Interaction>();
        }
    }
}

/// `SimTick`, `SimSet::Observe`: a household member's blow on a person of a
/// faction's post turns the post at once and writes a hurt deed once per
/// person and a killed deed on a lethal blow. Outlaws and the staging fight
/// write none: they are nobody's people.
pub fn strike_posts(
    registries: Option<Res<Registries>>,
    mut impacts: MessageReader<CombatImpact>,
    people: Query<(&EncounterId, &Allegiance, Has<Struck>)>,
    members: Query<(), (With<CharacterSheet>, Without<Allegiance>)>,
    mut commands: Commands,
    mut ledger: Ledger,
    mut turned: MessageWriter<PostTurned>,
) {
    let Some(registries) = registries else {
        return;
    };
    let mut hurt_now: BTreeSet<Entity> = BTreeSet::new();
    for impact in impacts.read() {
        let Some(victim) = impact.victim else {
            continue;
        };
        if impact.dealt <= 0. || !members.contains(impact.attacker) {
            continue;
        }
        let Ok((id, Allegiance::Faction(faction), struck)) = people.get(victim) else {
            continue;
        };
        if !struck && hurt_now.insert(victim) {
            ledger.deed(&registries.0, *faction, DeedKind::Hurt);
            commands.entity(victim).insert(Struck);
        }
        if impact.lethal {
            ledger.deed(&registries.0, *faction, DeedKind::Killed);
        }
        if ledger.cross(*id) {
            turned.write(PostTurned {
                post: *id,
                faction: *faction,
                by: TurnedBy::Struck,
            });
        }
    }
}

/// Registers the encounters' state and its systems. Added by `SimPlugin`; a
/// server and a harness have no encounter data worth a camp and touch
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct EncountersPlugin;

impl Plugin for EncountersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayedClock>()
            .init_resource::<Cleared>()
            .init_resource::<NearEncounters>()
            .init_resource::<Fights>()
            .add_message::<EncounterChanged>()
            .add_message::<PostTurned>()
            .add_systems(SimTick, advance_clock.in_set(SimSet::Input))
            .add_systems(SimTick, strike_posts.in_set(SimSet::Observe))
            .add_systems(
                Update,
                (
                    update_near_encounters,
                    mark_cleared,
                    watch_posts,
                    tend_guards,
                )
                    .chain()
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

    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use std::sync::Arc;

    use super::*;
    use crate::deeds::CommonsTaken;

    use bevy::app::App;

    use bevy::app::Update;

    use strata_registry::ClaimKind;

    use crate::combat::Action;
    use crate::combat::CombatImpact;
    use crate::combat::Combatant;

    use crate::deeds::DeedKind;
    use crate::deeds::Deeds;
    use crate::deeds::Leases;
    use crate::deeds::Ledger;

    use crate::factions::FactionId;
    use crate::factions::Factions;

    use crate::loot::Caches;

    use crate::progression::CharacterSheet;
    use crate::progression::Interaction;
    use crate::progression::InteractionKind;

    use crate::Registries;

    pub(crate) use veyra_rules::encounters::test_support::*;

    #[test]
    fn the_clock_counts_nominal_seconds() {
        let mut world = World::new();
        world.init_resource::<PlayedClock>();
        for _ in 0..40 {
            world
                .run_system_once(advance_clock)
                .expect("the clock runs");
        }
        assert!((world.resource::<PlayedClock>().seconds - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_camp_is_cleared_only_once_its_own_people_have_stood_and_fallen() {
        let s = site(1, FAR);
        let camp = camp_with_slots(&s, 1);
        let id = camp.id;
        let mut world = World::new();
        world.insert_resource(PlayedClock { seconds: 50.0 });
        world.insert_resource(Cleared::default());
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            camps: vec![camp],
            forced_tier: None,
            ..Default::default()
        });
        // Corpses of an earlier standing, with the new camp not yet seen
        // alive: nothing is cleared.
        let corpses: Vec<Entity> = (0..2).map(|_| world.spawn(body(id, true)).id()).collect();
        world.run_system_once(mark_cleared).expect("runs");
        assert!(world.resource::<Cleared>().at.is_empty());
        assert!(!world.resource::<NearEncounters>().camps[0].cleared);
        // The old bodies go and the new people stand, one not yet there:
        // killing the one that has stood clears nothing.
        for corpse in corpses {
            world.despawn(corpse);
        }
        let first = world.spawn(body(id, false)).id();
        world.run_system_once(mark_cleared).expect("runs");
        world.get_mut::<Combatant>(first).expect("body").action = Action::Dead;
        world.run_system_once(mark_cleared).expect("runs");
        assert!(
            world.resource::<Cleared>().at.is_empty(),
            "a person is still to stand"
        );
        // The last stands and falls: the camp is cleared at the clock.
        let second = world.spawn(body(id, false)).id();
        world.run_system_once(mark_cleared).expect("runs");
        world.get_mut::<Combatant>(second).expect("body").action = Action::Dead;
        world.run_system_once(mark_cleared).expect("runs");
        assert_eq!(world.resource::<Cleared>().at.get(&s.id.key()), Some(&50.0));
        assert!(world.resource::<NearEncounters>().camps[0].cleared);
    }

    /// A cleared camp leaves a cache of its table at its tier, kept while
    /// the camp is quiet and gone when it returns.
    #[test]
    fn a_cleared_camp_leaves_a_cache_that_goes_when_it_returns() {
        let registries = crate::deeds::tests::shipped_registries();
        let kind = registries
            .0
            .site_kind_id(&"strata:wild_camp".parse().unwrap())
            .expect("the camp kind ships");
        let mut s = site(1, FAR);
        s.id.kind = kind;
        let key = s.id.key();
        let camp = camp_with_slots(&s, 2);
        let id = camp.id;
        let slots = camp.slots.len();
        let mut world = World::new();
        world.insert_resource(registries.clone());
        world.insert_resource(PlayedClock { seconds: 50.0 });
        world.init_resource::<Cleared>();
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            camps: vec![camp],
            ..Default::default()
        });
        let people: Vec<Entity> = (0..slots)
            .map(|_| world.spawn(body(id, false)).id())
            .collect();
        world.run_system_once(mark_cleared).unwrap();
        assert!(world.resource::<Caches>().by.is_empty(), "still fighting");
        for person in people {
            world.get_mut::<Combatant>(person).unwrap().action = Action::Dead;
        }
        world.run_system_once(mark_cleared).unwrap();
        let caches = world.resource::<Caches>().clone();
        let cache = caches.by.get(&key).expect("a cache at the camp");
        let table = registries.0.loot.of(&cache.encounter).unwrap();
        let credits = table.tier(2).unwrap().credits;
        assert!((credits.0..=credits.1).contains(&cache.credits));
        // Quiet keeps it; the camp's return takes it.
        let encounters = &registries.0.encounters;
        let cleared = world.resource::<Cleared>().clone();
        let mut kept = caches.clone();
        kept.prune(&cleared, 50.0 + 3600.0, encounters);
        assert_eq!(kept, caches);
        kept.prune(&cleared, 50.0 + 4.0 * 3600.0, encounters);
        assert!(kept.by.is_empty());
    }

    #[test]
    fn a_camp_that_returns_while_the_member_is_by_is_not_cleared_again_by_its_old_bodies() {
        let s = site(1, FAR);
        let key = s.id.key();
        let id = EncounterId::Camp(key);
        let mut world = World::new();
        world.insert_resource(PlayedClock { seconds: 100.0 });
        let mut cleared = Cleared::default();
        cleared.clear(key, 100.0);
        world.insert_resource(cleared);
        world.init_resource::<Caches>();
        let mut standing = camp_with_slots(&s, 1);
        standing.cleared = true;
        standing.seen_alive = true;
        world.insert_resource(NearEncounters {
            camps: vec![standing],
            forced_tier: None,
            ..Default::default()
        });
        for _ in 0..2 {
            world.spawn(body(id, true));
        }
        // Three played hours on, the member still by: the camp folds and
        // stands again in one pass, and its old dead are still there.
        let hours = 3.0 * 3600.0 + 1.0;
        world.insert_resource(PlayedClock {
            seconds: 100.0 + hours,
        });
        let sites = [near(&s)];
        let mut near_camps = world.remove_resource::<NearEncounters>().expect("near");
        let mut cleared = world.remove_resource::<Cleared>().expect("cleared");
        let made = run(
            &mut near_camps,
            &sites,
            [FAR[0] + 20.0, FAR[1]],
            100.0 + hours,
            &mut cleared,
            &Fights::default(),
        );
        assert!(made.folded.contains(&id) && made.stood.contains(&id));
        world.insert_resource(near_camps);
        world.insert_resource(cleared);
        world.run_system_once(mark_cleared).expect("runs");
        assert!(!world.resource::<NearEncounters>().camps[0].cleared);
        // The record stays for the longest quiet of any encounter; the camp
        // reads it as run out, so it is not cleared again by old bodies.
        assert!(
            !world
                .resource::<Cleared>()
                .is_quiet(key, 100.0 + hours, 3.0)
        );
    }

    fn striking_world() -> App {
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .insert_resource(PlayedClock { seconds: 100.0 })
            .init_resource::<Deeds>()
            .init_resource::<Fights>()
            .add_message::<CombatImpact>()
            .add_message::<PostTurned>()
            .add_systems(Update, strike_posts);
        app
    }

    fn blow(app: &mut App, attacker: Entity, victim: Entity, lethal: bool) {
        let point = strata_core::WorldPos::ZERO;
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(victim),
            from: point,
            point,
            melee: false,
            dealt: 5.0,
            lethal,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        app.update();
    }

    #[test]
    fn a_strike_crosses_the_post_at_once_and_writes_one_hurt_a_person_and_a_killed_on_a_kill() {
        let mut app = striking_world();
        let faction = FactionId(77);
        let id = EncounterId::Post(8);
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        let member = app
            .world_mut()
            .spawn((Combatant::default(), CharacterSheet::default()))
            .id();
        let person = |app: &mut App, id: EncounterId| {
            app.world_mut()
                .spawn((
                    id,
                    Allegiance::Faction(faction),
                    Combatant::on_team(NEUTRAL_TEAM, 30.),
                ))
                .id()
        };
        let (a, b) = (person(&mut app, id), person(&mut app, id));
        // Somebody who is not a household member crosses nothing.
        let stranger = app.world_mut().spawn(Combatant::on_team(1, 10.)).id();
        blow(&mut app, stranger, a, false);
        assert!(app.world().resource::<Fights>().neutral(id));
        assert!(app.world().resource::<Deeds>().by.is_empty());
        // The member's first blow turns the post and is a hurt deed.
        blow(&mut app, member, a, false);
        assert!(app.world().resource::<Fights>().crossed(id));
        assert!(app.world().resource::<Fights>().engaged(id));
        let turned: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PostTurned>>()
            .drain()
            .collect();
        assert_eq!(
            turned,
            [PostTurned {
                post: id,
                faction,
                by: TurnedBy::Struck
            }]
        );
        // The same person again is not a second hurt; another person is, and
        // a kill is its own deed.
        blow(&mut app, member, a, false);
        let amount = |app: &App, kind: DeedKind| -> u32 {
            app.world().resource::<Deeds>().by[&faction.0]
                .iter()
                .filter(|d| d.kind == kind)
                .map(|d| d.amount)
                .sum()
        };
        assert_eq!(amount(&app, DeedKind::Hurt), 1);
        blow(&mut app, member, b, true);
        assert_eq!(amount(&app, DeedKind::Hurt), 2);
        assert_eq!(amount(&app, DeedKind::Killed), 1);
        // Outlaws are nobody's people: no deed.
        let before = app.world().resource::<Deeds>().clone();
        let outlaw = app
            .world_mut()
            .spawn((
                EncounterId::Camp(1),
                Allegiance::Outlaw(Some(faction)),
                Combatant::on_team(1, 30.),
            ))
            .id();
        blow(&mut app, member, outlaw, true);
        assert_eq!(*app.world().resource::<Deeds>(), before);
    }

    #[test]
    fn working_a_concession_without_a_lease_starts_a_warning_that_lapses_or_turns_the_post() {
        let (rich, claim, _) = claims_near_the_city();
        let id = EncounterId::Post(post_key(rich.id));
        let post = post_of(&rich, claim);
        let warn_s = crate::deeds::tests::shipped_registries()
            .0
            .factions
            .claims()
            .unwrap()
            .warn_s;
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .insert_resource(Factions(Arc::clone(&crate::deeds::tests::pelagic().0)))
            .insert_resource(PlayedClock { seconds: 1_000.0 })
            .init_resource::<Deeds>()
            .init_resource::<Leases>()
            .init_resource::<CommonsTaken>()
            .init_resource::<Fights>();
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        let trespass = |app: &mut App| {
            let deposit = rich.clone();
            app.world_mut()
                .run_system_once(move |mut ledger: Ledger, registries: Res<Registries>| {
                    ledger.trespass(&registries.0, &deposit);
                })
                .unwrap();
        };
        // A lease in force: working is lawful and nothing warns.
        app.world_mut()
            .resource_mut::<Leases>()
            .until
            .insert(claim.holder.0, 5_000.0);
        trespass(&mut app);
        assert_eq!(app.world().resource::<Fights>().warning(), None);
        app.world_mut().resource_mut::<Leases>().until.clear();
        trespass(&mut app);
        let warning = app
            .world()
            .resource::<Fights>()
            .warning()
            .expect("a warning runs");
        assert_eq!((warning.post, warning.faction), (id, claim.holder));
        assert!((warning.until - (1_000.0 + warn_s)).abs() < 1e-9);
        // It runs on one at a time.
        let mut fights = app.world().resource::<Fights>().clone();
        assert!(!fights.begin_warning(id, claim.holder, 9.0));
        let world = |player: [f64; 2], now: f64| Watching {
            posts: std::slice::from_ref(&post),
            player,
            now,
            engage_m: 11.0,
            people: &[],
        };
        let calm = |_: FactionId| false;
        // Leaving the claim lapses it.
        let mut left = fights.clone();
        let away = [rich.at[0] + f64::from(rich.radius_m) + 50.0, rich.at[1]];
        assert!(watch(&mut left, &world(away, 1_001.0), calm, calm).is_empty());
        assert_eq!(left.warning(), None);
        assert!(left.neutral(id));
        // A lease taken lapses it.
        let mut leased = fights.clone();
        assert!(watch(&mut leased, &world(rich.at, 1_001.0), |_| true, calm).is_empty());
        assert_eq!(leased.warning(), None);
        // Still working before it runs out: nothing; at the second, it turns.
        assert!(
            watch(
                &mut fights,
                &world(rich.at, 1_000.0 + warn_s - 1.0),
                calm,
                calm
            )
            .is_empty()
        );
        assert!(fights.warning().is_some() && fights.neutral(id));
        let turned = watch(&mut fights, &world(rich.at, 1_000.0 + warn_s), calm, calm);
        assert_eq!(turned, [(id, claim.holder, TurnedBy::Warning)]);
        assert!(fights.crossed(id) && fights.engaged(id) && fights.warning().is_none());
    }

    #[test]
    fn a_cleared_post_is_quiet_and_writes_no_camp_deed() {
        let (rich, claim, _) = claims_near_the_city();
        let mut post = post_of(&rich, claim);
        post.cleared = false;
        let id = post.id;
        let slots = post.slots.len();
        let mut world = World::new();
        world.insert_resource(crate::deeds::tests::shipped_registries());
        world.insert_resource(Factions(Arc::clone(&crate::deeds::tests::pelagic().0)));
        world.insert_resource(PlayedClock { seconds: 60.0 });
        world.init_resource::<Deeds>();
        world.init_resource::<Cleared>();
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            posts: vec![post],
            ..Default::default()
        });
        let people: Vec<Entity> = (0..slots)
            .map(|_| world.spawn(body(id, false)).id())
            .collect();
        world.run_system_once(mark_cleared).unwrap();
        for person in people {
            world.get_mut::<Combatant>(person).unwrap().action = Action::Dead;
        }
        world.run_system_once(mark_cleared).unwrap();
        assert!(world.resource::<NearEncounters>().posts[0].cleared);
        assert_eq!(
            world.resource::<Cleared>().at.get(&post_key(rich.id)),
            Some(&60.0)
        );
        assert!(
            world.resource::<Caches>().by.is_empty(),
            "a post leaves no cache"
        );
        assert!(
            world.resource::<Deeds>().by.is_empty(),
            "no ClearedCamp deed"
        );
        // It stays quiet for its hours.
        let hours = encounters()
            .at_claim(ClaimKind::Concession)
            .unwrap()
            .returns_h;
        assert!(
            world
                .resource::<Cleared>()
                .is_quiet(post_key(rich.id), 61.0, hours)
        );
        assert!(!is_cleared(
            &encounters(),
            world.resource::<Cleared>(),
            61.0,
            "strata:wild_camp",
            1
        ));
    }

    #[test]
    fn an_uncrossed_concession_guard_sells_a_lease_and_a_warden_or_a_turned_guard_does_not() {
        let (rich, claim, _) = claims_near_the_city();
        let id = EncounterId::Post(post_key(rich.id));
        let mut guards = post_of(&rich, claim);
        let mut wardens = guards.clone();
        wardens.id = EncounterId::Post(1);
        wardens.kind = ClaimKind::Reserve;
        guards.kind = ClaimKind::Concession;
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .init_resource::<Fights>()
            .insert_resource(NearEncounters {
                posts: vec![guards, wardens],
                ..Default::default()
            })
            .add_systems(Update, tend_guards);
        let faction = claim.holder;
        let person = |app: &mut App, id: EncounterId| {
            app.world_mut()
                .spawn((
                    id,
                    Allegiance::Faction(faction),
                    Combatant::on_team(NEUTRAL_TEAM, 30.),
                ))
                .id()
        };
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        app.world_mut()
            .resource_mut::<Fights>()
            .stand_post(EncounterId::Post(1));
        let (guard, warden) = (person(&mut app, id), person(&mut app, EncounterId::Post(1)));
        app.update();
        assert_eq!(
            app.world()
                .get::<Interaction>(guard)
                .map(|i| i.kind.clone()),
            Some(InteractionKind::Guard { faction })
        );
        assert!(app.world().get::<Interaction>(warden).is_none());
        app.world_mut().resource_mut::<Fights>().cross(id);
        app.update();
        assert!(app.world().get::<Interaction>(guard).is_none());
    }
}
