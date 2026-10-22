//! The adapter half of the `deposits::hand` rules: its requests, systems and plugin.

#[allow(
    clippy::wildcard_imports,
    reason = "the rest of deposits, whose names this half needs all of"
)]
use super::*;

use bevy::ecs::message::{MessageReader, MessageWriter};

use bevy::ecs::prelude::*;

use strata_registry::progression::XpKind;

use strata_registry::Reading;

use super::{Found, NearDeposits, Survey, SurveyUnkept, Surveyed, lot, resync, with_lot};

use crate::combat::CombatImpact;

use crate::deeds::Ledger;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, StockAnswer};

use crate::progression::{CharacterSheet, XpGained};

use crate::{LocalPlayer, PlayerCommand, Registries, WorldPosition, WorldSourceRes, water_here};

/// Puts `amount` of Survey XP on `sheet` and tells the HUD; nothing for
/// nothing.
pub(crate) fn credit_survey(
    sheet: &mut CharacterSheet,
    entity: Entity,
    amount: u32,
    gained: &mut MessageWriter<XpGained>,
) {
    if amount == 0 {
        return;
    }
    sheet.credit(XpKind::Survey, amount);
    gained.write(XpGained {
        entity,
        kind: XpKind::Survey,
        amount,
        kill: false,
    });
}

/// `SimTick`, `SimSet::Observe`: the channel of a ping under way. Moving
/// cancels it; at the end the ground is read, the household learns what it
/// met and the answer carries the result.
#[allow(
    clippy::too_many_arguments,
    reason = "the channel, the world and the household in, an answer and a notice out"
)]
pub fn advance_ping(
    mut commands: Commands,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    unkept: Option<Res<SurveyUnkept>>,
    mut surveyed: ResMut<Surveyed>,
    mut last: ResMut<LastPing>,
    mut players: Query<
        (
            Entity,
            &mut Pinging,
            Option<&PlayerCommand>,
            &WorldPosition,
            &Pack,
            Option<&mut CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<PingAnswer>,
    mut notices: MessageWriter<SurveyNotice>,
    mut gained: MessageWriter<XpGained>,
) {
    for (entity, mut pinging, command, at, pack, mut sheet) in &mut players {
        if command.is_some_and(|command| command.0.move_dir.length_squared() > 0.0) {
            commands.entity(entity).remove::<Pinging>();
            notices.write(SurveyNotice::PingMoved);
            continue;
        }
        pinging.left -= time.delta_secs();
        if pinging.left > 0.0 {
            continue;
        }
        commands.entity(entity).remove::<Pinging>();
        let registries = &registries.0;
        let Some(tool) = best_tool(&registries.items, pack) else {
            notices.write(SurveyNotice::PingNoTool);
            continue;
        };
        let skill = skill(
            &registries.resources,
            &registries.items,
            &registries.progression,
            &tool,
            sheet.as_deref().unwrap_or(&CharacterSheet::default()),
        );
        let here = [at.0.x(), at.0.z()];
        let result = match survey.as_deref() {
            Some(survey) => ping(&survey.0, &skill, here, &pinging.class),
            None => ping_with(
                &|_, _| Vec::new(),
                &registries.resources,
                &skill,
                here,
                &pinging.class,
            ),
        };
        let learned = learn(&mut surveyed, &result);
        let rates = &registries.progression.rates.survey;
        let earned = survey_xp(
            rates,
            SurveyEvent::Ping {
                found: learned.new > 0,
                better: learned.raised_pm >= rates.better_pm,
            },
        );
        if let Some(sheet) = sheet.as_deref_mut().filter(|_| unkept.is_none()) {
            credit_survey(sheet, entity, earned, &mut gained);
        }
        last.0 = Some(result.clone());
        answers.write(PingAnswer {
            class: pinging.class.clone(),
            outcome: Ok(PingDone::Met(result)),
        });
    }
}

/// `SimTick`, `SimSet::Observe`: sampling under way. A blow, swimming or
/// moving from where it began ends it, as does a pack with no room for the
/// cycle's lot; each cycle puts a lot in the pack, then a bag, and reads the
/// deposit exactly from then on.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    reason = "the channel, the world, the stock and the household in, two answers and a mirror out; a dial's seconds as a clock's f32"
)]
pub fn advance_sample(
    mut commands: Commands,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
    mut impacts: MessageReader<CombatImpact>,
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    source: Option<Res<WorldSourceRes>>,
    near: Res<NearDeposits>,
    open: Res<OpenStock>,
    store: Res<HouseholdStore>,
    unkept: Option<Res<SurveyUnkept>>,
    mut surveyed: ResMut<Surveyed>,
    mut players: Query<
        (
            Entity,
            &mut Sampling,
            &WorldPosition,
            &mut Pack,
            &Hands,
            Option<&mut CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<SampleAnswer>,
    mut notices: MessageWriter<SurveyNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
    (mut gained, mut ledger): (MessageWriter<XpGained>, Ledger),
) {
    let hit: Vec<Entity> = impacts.read().filter_map(|impact| impact.victim).collect();
    let registries = &registries.0;
    let (set, items) = (&registries.resources, &registries.items);
    for (entity, mut sampling, at, mut pack, hands, mut sheet) in &mut players {
        let here = [at.0.x(), at.0.z()];
        let stray = {
            let (dx, dz) = (here[0] - sampling.from[0], here[1] - sampling.from[1]);
            dx.hypot(dz) > set.survey().sample.stops_at_m
        };
        let swimming = source.as_ref().is_some_and(|source| {
            water_here(source.source(), here[0], here[1]).depth_m() > set.survey().ping.swim_m
        });
        let ended = if hit.contains(&entity) {
            Some(SampleEnd::Hit)
        } else if swimming {
            Some(SampleEnd::Swimming)
        } else if stray {
            Some(SampleEnd::Moved)
        } else {
            None
        };
        let end = |why: SampleEnd, commands: &mut Commands, notices: &mut MessageWriter<_>| {
            commands.entity(entity).remove::<Sampling>();
            notices.write(SurveyNotice::SampleEnded(why));
        };
        if let Some(why) = ended {
            end(why, &mut commands, &mut notices);
            continue;
        }
        sampling.left -= time.delta_secs();
        if sampling.left > 0.0 {
            continue;
        }
        let Some(tool) = best_tool(items, &pack) else {
            end(SampleEnd::NoTool, &mut commands, &mut notices);
            continue;
        };
        let skill = skill(
            set,
            items,
            &registries.progression,
            &tool,
            sheet.as_deref().unwrap_or(&CharacterSheet::default()),
        );
        sampling.left += skill.cycle_s;
        let deposit = survey
            .as_ref()
            .and_then(|survey| survey.0.get(sampling.deposit))
            .or_else(|| near.get(sampling.deposit).cloned());
        let Some(deposit) = deposit else {
            end(SampleEnd::Stopped, &mut commands, &mut notices);
            continue;
        };
        let pm = deposit.concentration(sampling.from[0], sampling.from[1]);
        let units = cycle_units(skill.units, pm);
        let taken = lot(&deposit, set, units).and_then(|record| with_lot(items, &pack, &record));
        let Some(next) = taken.filter(|_| units > 0) else {
            end(SampleEnd::NoRoom, &mut commands, &mut notices);
            continue;
        };
        *pack = next;
        sampling.got += units;
        ledger.worked(registries, &deposit, units);
        let first = surveyed.found.get(&deposit.id).is_none_or(|f| !f.sampled);
        if let Some(sheet) = sheet.as_deref_mut().filter(|_| unkept.is_none()) {
            let earned = survey_xp(
                &registries.progression.rates.survey,
                SurveyEvent::Cycle { first, pm },
            );
            credit_survey(sheet, entity, earned, &mut gained);
        }
        let known = surveyed.found.entry(deposit.id).or_insert(Found {
            reading: Reading::Exact,
            sampled: true,
            best_pm: deposit.concentration(sampling.from[0], sampling.from[1]),
            best_at: sampling.from,
        });
        known.sampled = true;
        known.reading = Reading::Exact;
        answers.write(SampleAnswer {
            outcome: Ok(SampleDone::Cycle {
                deposit: deposit.id,
                units,
                total: sampling.got,
            }),
        });
        resync(&open, &store, &pack, hands, &mut mirrors);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "the lattice's points are exact sums of the spacing"
    )]

    use std::sync::Arc;

    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;

    use super::*;
    use crate::PlayerIntent;
    use crate::WorldPosition;
    use crate::combat::Action;
    use crate::combat::Combatant;
    use crate::deposits::SurveyInbox;
    use crate::deposits::SurveyRequest;
    use crate::items::Pack;
    use crate::tests::app_in_world;
    use strata_registry::progression::XpKind;

    use strata_registry::Namespaced;
    use strata_registry::Reading;

    use super::super::Deposit;

    use super::super::Survey;
    use super::super::SurveyUnkept;
    use super::super::Surveyed;

    use crate::combat::CombatImpact;

    use crate::items::Hands;

    use crate::progression::CharacterSheet;

    use crate::LocalPlayer;

    use crate::Registries;
    use crate::WorldSourceRes;
    use crate::water_here;

    pub(crate) use veyra_rules::deposits::hand::test_support::*;

    /// A player with the arrival's pack standing `off` metres east of the
    /// nearest deposit's centre, in the world.
    fn playing(off: f64) -> (App, Entity, Deposit) {
        let (field, deposit) = world();
        let mut app = app_in_world();
        let registries = registries();
        let pack = arrival_pack(&registries);
        app.insert_resource(registries)
            .insert_resource(Survey(Arc::clone(field)));
        let player = app
            .world_mut()
            .spawn((
                crate::Player::default(),
                LocalPlayer,
                pack,
                Hands::default(),
                WorldPosition::new(deposit.at[0] + off, 0.0, deposit.at[1]),
                Combatant::default(),
            ))
            .id();
        app.update();
        (app, player, deposit.clone())
    }

    fn tick(app: &mut App, heard: &mut Heard) {
        app.update();
        let world = app.world_mut();
        heard
            .pings
            .extend(world.resource_mut::<Messages<PingAnswer>>().drain());
        heard
            .samples
            .extend(world.resource_mut::<Messages<SampleAnswer>>().drain());
        heard
            .notices
            .extend(world.resource_mut::<Messages<SurveyNotice>>().drain());
    }

    fn send(app: &mut App, request: SurveyRequest) -> Heard {
        app.world_mut().resource_mut::<SurveyInbox>().push(request);
        let mut heard = Heard::default();
        tick(app, &mut heard);
        heard
    }

    fn run(app: &mut App, ticks: usize) -> Heard {
        let mut heard = Heard::default();
        for _ in 0..ticks {
            tick(app, &mut heard);
        }
        heard
    }

    fn class_id(app: &App, deposit: &Deposit) -> Namespaced {
        class_of(&app.world().resource::<Registries>().0.resources, deposit)
    }

    /// A ping channels for the dials' seconds, then answers with what the
    /// ground read, and the household has found the deposit; sampling
    /// needs that first.
    #[test]
    fn a_ping_channels_then_finds_and_a_sample_needs_the_find() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert_eq!(
            heard.samples[0].outcome,
            Err(SurveyRefusal::Nothing),
            "not pinged, not sampled"
        );
        let started = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        )
        .pings;
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].outcome, Ok(PingDone::Started));
        assert!(app.world().get::<Pinging>(player).is_some());
        // A second ping while one is under way is busy.
        let again = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        )
        .pings;
        assert_eq!(again[0].outcome, Err(SurveyRefusal::Busy));
        // 2.5 s is 50 ticks at 20 Hz.
        let done = run(&mut app, 60).pings;
        assert_eq!(done.len(), 1);
        let Ok(PingDone::Met(result)) = &done[0].outcome else {
            panic!("{done:?}");
        };
        assert_eq!(result.found[0].deposit.id, deposit.id);
        assert_eq!(result.found[0].pm, deposit.peak);
        assert!(app.world().get::<Pinging>(player).is_none());
        let surveyed = app.world().resource::<Surveyed>();
        assert!(surveyed.found.contains_key(&deposit.id));
        assert_eq!(surveyed.lead.unwrap().deposit, deposit.id);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert!(matches!(
            heard.samples[0].outcome,
            Ok(SampleDone::Started { .. })
        ));
    }

    /// Moving cancels a ping with no reading; stopping does too.
    #[test]
    fn moving_cancels_a_ping_with_no_reading() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        run(&mut app, 10);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::Y;
        let heard = run(&mut app, 2);
        assert!(app.world().get::<Pinging>(player).is_none());
        assert_eq!(heard.notices, [SurveyNotice::PingMoved]);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::ZERO;
        let heard = run(&mut app, 60);
        assert!(heard.pings.is_empty(), "no reading");
        assert!(app.world().resource::<Surveyed>().found.is_empty());

        send(&mut app, SurveyRequest::Ping { class });
        let heard = send(&mut app, SurveyRequest::Stop);
        assert_eq!(heard.notices, [SurveyNotice::PingStopped]);
        assert!(app.world().get::<Pinging>(player).is_none());
    }

    /// The pack's survey tool, a rider and the swimmer: refused at the
    /// request, in the sim's own state.
    #[test]
    fn a_player_in_the_saddle_or_without_a_tool_cannot_ping() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        let def = app
            .world()
            .resource::<Registries>()
            .0
            .vehicles
            .iter()
            .next()
            .unwrap()
            .clone();
        app.world_mut()
            .entity_mut(player)
            .insert(crate::ride::Riding::new(&def, 0.0));
        let heard = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::Mounted));
        app.world_mut()
            .entity_mut(player)
            .remove::<crate::ride::Riding>();
        let registries = app.world().resource::<Registries>().clone();
        *app.world_mut().get_mut::<Pack>(player).unwrap() = bare_pack(&registries);
        let heard = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::NoTool));
        // Mid action is busy.
        *app.world_mut().get_mut::<Pack>(player).unwrap() = arrival_pack(&registries);
        app.world_mut().get_mut::<Combatant>(player).unwrap().action = Action::Heavy;
        let heard = send(&mut app, SurveyRequest::Ping { class });
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::Busy));
    }

    /// Start sampling at the deposit's centre: found by a ping first.
    fn sampling_at(off: f64) -> (App, Entity, Deposit) {
        let (mut app, player, deposit) = playing(off);
        let class = class_id(&app, &deposit);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert!(matches!(
            heard.samples[0].outcome,
            Ok(SampleDone::Started { .. })
        ));
        (app, player, deposit)
    }

    fn carried(app: &App, player: Entity, deposit: &Deposit) -> u32 {
        let items = &app.world().resource::<Registries>().0.items;
        let pack = app.world().get::<Pack>(player).unwrap();
        crate::crafting::reachable(items, pack, None)
            .into_iter()
            .filter(|(record, _)| {
                record
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.deposit == deposit.id.0)
            })
            .map(|(record, _)| record.count)
            .sum()
    }

    /// Each cycle puts a lot of the deposit in the pack, the lots of one
    /// deposit stack across cycles, the units follow the concentration and
    /// the deposit is sampled and exact from the first.
    #[test]
    fn sampling_stacks_cycles_by_concentration() {
        let (mut app, player, deposit) = sampling_at(0.0);
        let per = cycle_units(8, deposit.peak);
        // 4 s a cycle is 80 ticks; three cycles.
        let got: Vec<SampleDone> = run(&mut app, 80 * 3 + 2)
            .samples
            .into_iter()
            .map(|answer| answer.outcome.unwrap())
            .collect();
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[2],
            SampleDone::Cycle {
                deposit: deposit.id,
                units: per,
                total: 3 * per
            }
        );
        assert_eq!(carried(&app, player, &deposit), 3 * per);
        // One stack, because the lots are of one deposit.
        let items = &app.world().resource::<Registries>().0.items;
        let pack = app.world().get::<Pack>(player).unwrap();
        let lots = crate::crafting::reachable(items, pack, None)
            .into_iter()
            .filter(|(record, _)| record.origin.is_some())
            .count();
        assert_eq!(lots, 1);
        let found = app.world().resource::<Surveyed>().found[&deposit.id];
        assert!(found.sampled);
        assert_eq!(found.reading, Reading::Exact);
        assert_eq!(app.world().get::<Sampling>(player).unwrap().got, 3 * per);

        // Half way to the edge the cycle gives less, in proportion.
        let off = f64::from(deposit.radius_m) * 0.5;
        let (mut app, player, deposit) = sampling_at(off);
        let pm = deposit.concentration(deposit.at[0] + off, deposit.at[1]);
        assert!(pm < deposit.peak && pm >= 20);
        run(&mut app, 82);
        assert_eq!(carried(&app, player, &deposit), cycle_units(8, pm));
        assert!(carried(&app, player, &deposit) < per || per == 1);
    }

    /// Each way sampling ends says why.
    #[test]
    fn sampling_ends_by_stopping_moving_a_blow_or_a_full_pack() {
        // Stopping.
        let (mut app, player, _) = sampling_at(0.0);
        let heard = send(&mut app, SurveyRequest::Stop);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::Stopped)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());

        // Moving the dial's distance from where it began.
        let (mut app, player, _) = sampling_at(0.0);
        run(&mut app, 5);
        app.world_mut()
            .get_mut::<WorldPosition>(player)
            .unwrap()
            .0
            .0
            .x += 1.0;
        run(&mut app, 2);
        assert!(
            app.world().get::<Sampling>(player).is_some(),
            "1 m is within 1.5"
        );
        app.world_mut()
            .get_mut::<WorldPosition>(player)
            .unwrap()
            .0
            .0
            .x += 1.0;
        let heard = run(&mut app, 2);
        assert!(app.world().get::<Sampling>(player).is_none());
        assert_eq!(heard.notices, [SurveyNotice::SampleEnded(SampleEnd::Moved)]);

        // A blow on the player.
        let (mut app, player, _) = sampling_at(0.0);
        app.world_mut().write_message(CombatImpact {
            attacker: Entity::PLACEHOLDER,
            victim: Some(player),
            from: strata_core::WorldPos::ZERO,
            point: strata_core::WorldPos::ZERO,
            melee: false,
            dealt: 1.0,
            lethal: false,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        let heard = run(&mut app, 2);
        assert_eq!(heard.notices, [SurveyNotice::SampleEnded(SampleEnd::Hit)]);
        assert!(app.world().get::<Sampling>(player).is_none());

        // A pack with no room for the next cycle's lot.
        let (mut app, player, _) = sampling_at(0.0);
        {
            let items = app.world().resource::<Registries>().0.clone();
            let mut pack = app.world_mut().get_mut::<Pack>(player).unwrap();
            crate::crafting::tests::fill(&mut pack.cells, 18, &items.items);
            for bag in 0..2 {
                let open = usize::from(pack.open_cells(bag, &items.items));
                crate::crafting::tests::fill(&mut pack.bags[bag], open, &items.items);
            }
        }
        let heard = run(&mut app, 85);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::NoRoom)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());
    }

    /// The bike is called with `fire` held, and what the sim said of the call
    /// and whether the shot survived.
    fn call_the_bike(
        app: &mut App,
        player: Entity,
    ) -> (
        Result<crate::ride::RideDone, crate::ride::RideRefusal>,
        bool,
    ) {
        use crate::combat::CombatIntent;
        use crate::ride::{RideInbox, RideRequest};
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .fire = true;
        app.world_mut()
            .resource_mut::<RideInbox>()
            .push(RideRequest::Call);
        app.update();
        let answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<crate::ride::RideAnswer>>()
            .drain()
            .collect();
        (
            answers[0].result,
            app.world().get::<CombatIntent>(player).unwrap().fire,
        )
    }

    fn give_the_bike(app: &mut App) {
        let registries = app.world().resource::<Registries>().0.clone();
        let vehicle = registries.vehicles.iter().next().unwrap().id.clone();
        let bike = crate::ride::store_grade_bike(&registries.items, &vehicle).unwrap();
        *app.world_mut().resource_mut::<crate::ride::Stable>() =
            crate::ride::Stable::with_bike(bike, &registries.items, &registries.vehicles);
    }

    /// While pinging or sampling the combatant fires nothing and the bike
    /// cannot be called: both channels, each on its own.
    #[test]
    fn pinging_and_sampling_hold_fire_and_busy_the_bike() {
        use crate::ride::RideRefusal;
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        give_the_bike(&mut app);
        send(&mut app, SurveyRequest::Ping { class });
        assert!(app.world().get::<Pinging>(player).is_some());
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Err(RideRefusal::Busy));
        assert!(!fire, "no shot while pinging");

        // Sampling, once the ping has found the deposit.
        let (mut app, player, _) = sampling_at(0.0);
        give_the_bike(&mut app);
        assert!(app.world().get::<Sampling>(player).is_some());
        assert!(app.world().get::<Pinging>(player).is_none());
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Err(RideRefusal::Busy));
        assert!(!fire, "no shot while sampling");

        // Neither: the call is granted, the shot is the combatant's.
        let (mut app, player, _) = playing(0.0);
        give_the_bike(&mut app);
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Ok(crate::ride::RideDone::Calling));
        assert!(fire, "a held shot survives a tick with no channel");
    }

    /// The first point in the world's sea, as far out as it takes.
    fn deep_water(app: &App) -> [f64; 2] {
        let source = app.world().resource::<WorldSourceRes>().source();
        let swim = app
            .world()
            .resource::<Registries>()
            .0
            .resources
            .survey()
            .ping
            .swim_m;
        for step in 1..80 {
            let r = f64::from(step) * 250.0;
            for k in 0..16 {
                let a = f64::from(k) * std::f64::consts::TAU / 16.0;
                let (x, z) = (r * a.cos(), r * a.sin());
                if water_here(source, x, z).depth_m() > swim + 1.0 {
                    return [x, z];
                }
            }
        }
        panic!("no deep water within 20 km");
    }

    /// Swimming ends a sampling and says so, even before the move away from
    /// where it began would have.
    #[test]
    fn a_sampling_over_deep_water_ends_as_swimming() {
        let (mut app, player, _) = sampling_at(0.0);
        let source = Arc::clone(world().0.world());
        app.insert_resource(WorldSourceRes(source));
        let [x, z] = deep_water(&app);
        {
            let mut at = app.world_mut().get_mut::<WorldPosition>(player).unwrap();
            at.0.0.x = x;
            at.0.0.z = z;
        }
        let heard = run(&mut app, 2);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::Swimming)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());
        // And the same water refuses a new ping.
        let class = {
            let deposit = world().1.clone();
            class_id(&app, &deposit)
        };
        let heard = send(&mut app, SurveyRequest::Ping { class });
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::InWater));
    }

    /// In the sim: a ping that finds earns Survey XP once, a second ping
    /// there earns none, and sampling pays a first bonus once.
    #[test]
    fn surveying_earns_survey_xp_in_the_tick() {
        let (mut app, player, deposit) = playing(0.0);
        app.world_mut()
            .entity_mut(player)
            .insert(CharacterSheet::default());
        let rates = app
            .world()
            .resource::<Registries>()
            .0
            .progression
            .rates
            .survey
            .clone();
        let class = class_id(&app, &deposit);
        send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        run(&mut app, 60);
        let xp = |app: &App| {
            app.world()
                .get::<CharacterSheet>(player)
                .unwrap()
                .xp(XpKind::Survey)
        };
        assert_eq!(xp(&app), rates.find);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        assert_eq!(
            xp(&app),
            rates.find,
            "pinging the same ground earns nothing"
        );

        send(&mut app, SurveyRequest::Sample);
        run(&mut app, 82);
        let each = survey_xp(
            &rates,
            SurveyEvent::Cycle {
                first: false,
                pm: deposit.peak,
            },
        );
        assert_eq!(xp(&app), rates.find + each + rates.first_sample);
        run(&mut app, 80);
        assert_eq!(xp(&app), rates.find + 2 * each + rates.first_sample);
    }

    /// A household whose record is not kept this run earns no Survey XP, so a
    /// restart cannot pay the same finds again.
    #[test]
    fn an_unkept_record_earns_no_survey_xp() {
        let (mut app, player, deposit) = playing(0.0);
        app.world_mut()
            .entity_mut(player)
            .insert(CharacterSheet::default());
        app.insert_resource(SurveyUnkept);
        let class = class_id(&app, &deposit);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        send(&mut app, SurveyRequest::Sample);
        run(&mut app, 82);
        let sheet = app.world().get::<CharacterSheet>(player).unwrap();
        assert_eq!(sheet.xp(XpKind::Survey), 0);
    }
}
