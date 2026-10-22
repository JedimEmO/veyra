//! The adapter half of the `ride` rules: its requests, systems and plugin.

pub use veyra_rules::ride::*;

use bevy::app::{App, Plugin};

use bevy::ecs::message::{MessageReader, MessageWriter};

use bevy::ecs::prelude::*;

use strata_registry::item::Record;

use crate::combat::{Action, CombatImpact, CombatIntent, CombatSet, Combatant, enemies};

use crate::deposits::{Pinging, Sampling};

pub use crate::household::{Calling, RideDials, RideEnd, Riding, Stable, vehicle_of};

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer, StoreAccess};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, InteractionKind, Reach};

use crate::{
    AppRequestExt, Inbox, LocalPlayer, PlayerCommand, Registries, Request, SimSet, SimTick,
    WorldPosition, WorldSourceRes, water_here,
};

/// What a request asks of the ride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RideRequest {
    /// Start calling the bike.
    Call,
    /// Get off, or stop calling.
    Dismount,
    /// Take the bike of this kind, from the pack, its bags or the open
    /// store, as the household's ride.
    Stable {
        /// The bike, as a stack of its kind is read.
        kind: Record,
    },
    /// Put the stabled bike back in the pack, else the open store.
    PutAway,
}

/// The requests not yet consumed.
pub type RideInbox = Inbox<RideRequest>;

/// With nobody to ride, a request is dropped unanswered, as it always was.
impl Request for RideRequest {
    type Answer = RideAnswer;

    fn no_actor(&self, _commands: &mut Commands) {}
}

/// What a granted request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RideDone {
    /// The channel started.
    Calling,
    /// The channel ended and the rider is on the bike.
    Mounted,
    /// The rider is on foot, standing where they were.
    Dismounted,
    /// A bike is the household's ride.
    Stabled,
    /// The bike is back in the pack or the store.
    PutAway,
}

/// The sim's answer to a [`RideRequest`], or to a channel that ended.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RideAnswer {
    /// What happened, or why nothing did.
    pub result: Result<RideDone, RideRefusal>,
}

/// The distance to the nearest living combatant of another team.
fn nearest_hostile(
    team: u8,
    at: &WorldPosition,
    others: &Query<(&Combatant, &WorldPosition), Without<LocalPlayer>>,
) -> Option<f64> {
    others
        .iter()
        .filter(|(other, _)| {
            enemies(other.team, team) && other.health > 0.0 && other.action != Action::Dead
        })
        .map(|(_, position)| position.0.0.distance(at.0.0))
        .min_by(f64::total_cmp)
}

/// `SimTick`, `SimSet::Input`, after the last inbox: the requests made
/// since the last tick, then the channel of a call under way. A bike taken
/// or put away reaches the store by the same rule a click on it has: a menu
/// open over it and a locker in reach.
#[allow(
    clippy::too_many_arguments,
    reason = "the inbox, the stable, the stock and its menu, the world, the riders and the hostiles in, two messages out"
)]
#[allow(clippy::too_many_lines, reason = "one arm a request, then the channel")]
pub fn consume(
    mut commands: Commands,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
    mut inbox: ResMut<RideInbox>,
    mut stable: ResMut<Stable>,
    registries: Res<Registries>,
    source: Option<Res<WorldSourceRes>>,
    access: Res<StoreAccess>,
    reach: Res<Reach>,
    mut open: ResMut<OpenStock>,
    mut store: ResMut<HouseholdStore>,
    others: Query<(&Combatant, &WorldPosition), Without<LocalPlayer>>,
    mut riders: Query<(
        Entity,
        &WorldPosition,
        Option<&Combatant>,
        Option<&Riding>,
        Option<&Calling>,
        Has<Pinging>,
        Has<Sampling>,
        Option<&CharacterSheet>,
        Option<&mut Pack>,
        Option<&mut Hands>,
    )>,
    local: Query<
        (
            Entity,
            &PlayerCommand,
            &WorldPosition,
            Option<&Combatant>,
            Option<&Riding>,
            Option<&Calling>,
            Has<Pinging>,
            Has<Sampling>,
            Option<&CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<RideAnswer>,
    mut notices: MessageWriter<RideNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let items = &registries.0.items;
    let vehicles = &registries.0.vehicles;
    let graph = &registries.0.progression;
    let state = |stable: &Stable,
                 at: &WorldPosition,
                 combatant: Option<&Combatant>,
                 riding: bool,
                 pinging: bool,
                 sampling: bool| RiderState {
        riding,
        water_m: source.as_ref().map_or(0.0, |source| {
            let feet_m = stable
                .vehicle(items, vehicles)
                .map_or(0.0, |(def, _)| def.call.feet_m);
            let here = water_here(source.source(), at.0.x(), at.0.z());
            water_over_feet(&here, at.0.y(), feet_m)
        }),
        busy: combatant.is_some_and(|c| c.action != Action::Ready || c.health <= 0.0)
            || pinging
            || sampling,
        nearest_hostile_m: nearest_hostile(combatant.map_or(0, |c| c.team), at, &others),
    };
    let at_locker = *access == StoreAccess::Anywhere
        || matches!(reach.nearest, Some((_, InteractionKind::Locker)));
    let with_store = open
        .0
        .as_ref()
        .is_some_and(|opened| opened.with_store && at_locker);
    // What this tick's requests left of each actor's mount and call, since
    // the commands that write them land after the system.
    let mut left: Vec<(Entity, bool, Option<Calling>)> = Vec::new();
    let mut stocked: Option<Entity> = None;
    for (actor, request) in inbox.take() {
        let Ok((
            player,
            at,
            combatant,
            riding,
            calling,
            pinging,
            sampling,
            sheet,
            mut pack,
            mut hands,
        )) = riders.get_mut(actor.0)
        else {
            continue;
        };
        let sheet = sheet.cloned().unwrap_or_default();
        let slot = left.iter().position(|(who, _, _)| *who == player);
        let (mut mounted, mut calling) = slot.map_or_else(
            || (riding.is_some(), calling.cloned()),
            |at| (left[at].1, left[at].2.clone()),
        );
        let seen = riding.is_some() || calling.is_some();
        match request {
            RideRequest::Call => {
                let result = can_call(
                    &stable,
                    items,
                    vehicles,
                    &state(&stable, at, combatant, seen, pinging, sampling),
                )
                .map(|(def, _)| {
                    let left = call_s(def, graph, &sheet);
                    calling = Some(Calling { left });
                    commands.entity(player).insert(Calling { left });
                    RideDone::Calling
                });
                answers.write(RideAnswer { result });
            }
            RideRequest::Dismount => {
                if mounted || calling.is_some() {
                    commands.entity(player).remove::<(Riding, Calling)>();
                    if mounted {
                        notices.write(RideNotice::Dismounted);
                    }
                    mounted = false;
                    calling = None;
                    answers.write(RideAnswer {
                        result: Ok(RideDone::Dismounted),
                    });
                }
            }
            RideRequest::Stable { .. } | RideRequest::PutAway => {
                let busy = mounted || calling.is_some();
                let result = match (pack.as_deref_mut(), hands.as_deref_mut()) {
                    (Some(pack), Some(hands)) => {
                        let mut next = (pack.clone(), hands.clone(), store.clone());
                        let mut moved = stable.clone();
                        let mut stock = Stock {
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: with_store.then_some(&mut next.2),
                            saddle: None,
                        };
                        let (done, kind) = match &request {
                            RideRequest::Stable { kind } => (
                                stable_bike(items, vehicles, &mut stock, &mut moved, kind, busy),
                                RideDone::Stabled,
                            ),
                            _ => (
                                put_bike_away(items, &mut stock, &mut moved, busy),
                                RideDone::PutAway,
                            ),
                        };
                        done.map(|()| {
                            stocked = Some(player);
                            if *pack != next.0 {
                                *pack = next.0;
                            }
                            *hands = next.1;
                            if *store != next.2 {
                                *store = next.2;
                            }
                            *stable = moved;
                            kind
                        })
                    }
                    _ => Err(RideRefusal::NotHeld),
                };
                answers.write(RideAnswer { result });
            }
        }
        match slot {
            Some(at) => left[at] = (player, mounted, calling),
            None => left.push((player, mounted, calling)),
        }
    }
    if let Some(who) = stocked
        && let Some(opened) = open.0.as_mut()
        && let Ok((.., pack, hands)) = riders.get(who)
        && let (Some(pack), Some(hands)) = (pack, hands)
    {
        opened.saddle.clone_from(&stable.saddle);
        let mut truth = (pack.clone(), hands.clone(), store.clone());
        let stock = Stock {
            pack: &mut truth.0,
            hands: &mut truth.1,
            store: opened.with_store.then_some(&mut truth.2),
            saddle: None,
        };
        mirrors.write(StockAnswer::Resync {
            menu: opened.menu,
            snapshot: stock.snapshot(&opened.state, &opened.saddle),
        });
    }
    // The call channel is the local rider's own, a per-tick clock and not a
    // request, so it stays on the one local player.
    let Ok((player, command, at, combatant, _, calling, pinging, sampling, sheet)) = local.single()
    else {
        return;
    };
    let sheet = sheet.cloned().unwrap_or_default();
    let calling = left
        .iter()
        .find(|(who, _, _)| *who == player)
        .map_or_else(|| calling.cloned(), |(_, _, channel)| channel.clone());
    let Some(mut channel) = calling else {
        return;
    };
    if command.0.move_dir.length_squared() > 0.0 {
        commands.entity(player).remove::<Calling>();
        notices.write(RideNotice::Cancelled {
            why: CancelWhy::Moved,
        });
        return;
    }
    channel.left -= time.delta_secs();
    if channel.left > 0.0 {
        commands.entity(player).insert(channel);
        return;
    }
    commands.entity(player).remove::<Calling>();
    // The bike is called again against the rule at the end: a hostile that
    // came in while it was on its way stops it.
    match can_call(
        &stable,
        items,
        vehicles,
        &state(&stable, at, combatant, false, pinging, sampling),
    ) {
        Ok((def, bike)) => {
            commands.entity(player).insert(Riding::with_dials(
                def,
                dials(def, bike, items.wear(), graph, &sheet),
                command.0.yaw,
            ));
            notices.write(RideNotice::Mounted);
            answers.write(RideAnswer {
                result: Ok(RideDone::Mounted),
            });
        }
        Err(refusal) => {
            notices.write(RideNotice::Cancelled {
                why: CancelWhy::Refused(refusal),
            });
            answers.write(RideAnswer {
                result: Err(refusal),
            });
        }
    }
}

/// `SimTick`, `SimSet::Step`, before the combatant decides: a rider, or a
/// player pinging or sampling, fires, cuts, dodges and reloads nothing. The
/// weapon in hand stays as it is.
pub fn hold_fire(
    mut riders: Query<&mut CombatIntent, Or<(With<Riding>, With<Pinging>, With<Sampling>)>>,
) {
    for mut intent in &mut riders {
        intent.fire = false;
        intent.light = false;
        intent.heavy = false;
        intent.dodge = false;
        intent.reload = false;
    }
}

/// `SimTick`, `SimSet::Observe`: a blow on the rider or an end the step
/// reported throws them, on foot where they are. A crash wears the bike by
/// [`crash_cost`]; a hit or deep water costs it nothing.
pub fn throw_riders(
    mut commands: Commands,
    mut impacts: MessageReader<CombatImpact>,
    registries: Res<Registries>,
    mut stable: ResMut<Stable>,
    riders: Query<(Entity, &Riding)>,
    mut notices: MessageWriter<RideNotice>,
) {
    let hit: Vec<Entity> = impacts.read().filter_map(|i| i.victim).collect();
    for (entity, riding) in &riders {
        let (why, speed) = match riding.ended {
            Some(RideEnd::Crash(speed)) => (ThrownWhy::Crash, speed),
            Some(RideEnd::Water) => (ThrownWhy::Water, riding.speed),
            None if hit.contains(&entity) => (ThrownWhy::Hit, riding.speed),
            None => continue,
        };
        let items = &registries.0.items;
        let rules = items.wear();
        let mut condition = None;
        if why == ThrownWhy::Crash
            && let Some(def) = registries.0.vehicles.get(&riding.vehicle)
            && let Some(bike) = stable.bike.as_mut()
        {
            let was = crate::wear::condition_of(rules, bike.condition);
            let now = was.saturating_sub(crash_cost(def, rules, bike, speed));
            bike.condition = Some(now);
            condition = Some(now);
        }
        commands.entity(entity).remove::<Riding>();
        notices.write(RideNotice::Thrown {
            why,
            speed,
            condition,
        });
    }
}

/// Registers the request and its consumers. Added by `SimPlugin`, so a
/// server and a harness have it too; with no bike it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct RidePlugin;

impl Plugin for RidePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stable>()
            .add_message::<RideNotice>()
            .add_request::<RideRequest, _>(RequestSet::Ride, consume)
            .add_systems(
                SimTick,
                (
                    hold_fire.in_set(SimSet::Step).before(CombatSet::Decide),
                    throw_riders
                        .in_set(SimSet::Observe)
                        .after(CombatSet::Finish),
                ),
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
    use crate::combat::Combatant;
    use crate::tests::app_in_world;
    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;
    use slotted_model::Inventory;
    use std::sync::Arc;
    use strata_registry::load_from_dir;

    use strata_registry::item::Record;

    use crate::combat::Action;
    use crate::combat::CombatImpact;
    use crate::combat::CombatIntent;

    use crate::items;

    use crate::items::HouseholdStore;
    use crate::items::OpenStock;
    use crate::items::Pack;

    use crate::progression::InteractionKind;

    use crate::LocalPlayer;

    use crate::Registries;

    use crate::WorldPosition;

    pub(crate) use veyra_rules::ride::test_support::*;

    fn world(owns: bool) -> (App, Entity) {
        let mut app = app_in_world();
        let registries =
            load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap();
        app.insert_resource(Registries(Arc::new(registries)));
        if owns {
            let registries = app.world().resource::<Registries>().0.clone();
            let record = store_grade_bike(&registries.items, &bike()).unwrap();
            *app.world_mut().resource_mut::<Stable>() =
                Stable::with_bike(record, &registries.items, &registries.vehicles);
        }
        let player = app
            .world_mut()
            .spawn((
                crate::Player::default(),
                LocalPlayer,
                WorldPosition::new(0.0, 0.0, 0.0),
                Combatant::default(),
            ))
            .id();
        app.update();
        (app, player)
    }

    fn answers(app: &mut App) -> Vec<RideAnswer> {
        app.world_mut()
            .resource_mut::<Messages<RideAnswer>>()
            .drain()
            .collect()
    }

    fn ask(app: &mut App, request: RideRequest) -> Result<RideDone, RideRefusal> {
        app.world_mut().resource_mut::<RideInbox>().push(request);
        app.update();
        let mut got = answers(app);
        got.pop().unwrap().result
    }

    fn mount(app: &mut App) {
        assert_eq!(ask(app, RideRequest::Call), Ok(RideDone::Calling));
        for _ in 0..40 {
            app.update();
        }
    }

    #[test]
    fn a_call_with_no_bike_is_refused() {
        let (mut app, _) = world(false);
        assert_eq!(ask(&mut app, RideRequest::Call), Err(RideRefusal::NoBike));
    }

    #[test]
    fn a_call_channels_then_mounts_and_dismounting_leaves_the_rider_standing() {
        let (mut app, player) = world(true);
        assert_eq!(ask(&mut app, RideRequest::Call), Ok(RideDone::Calling));
        assert!(app.world().get::<Calling>(player).is_some());
        assert!(app.world().get::<Riding>(player).is_none());
        // 1.5 s is 30 ticks at 20 Hz.
        let mut told = Vec::new();
        for _ in 0..40 {
            app.update();
            told.extend(answers(&mut app));
        }
        assert!(app.world().get::<Calling>(player).is_none());
        let riding = app.world().get::<Riding>(player).unwrap();
        assert_eq!(riding.vehicle, bike());
        assert!((riding.dials.speed.road - 60.0).abs() < 1e-6);
        assert_eq!(told.last().unwrap().result, Ok(RideDone::Mounted));
        assert_eq!(
            ask(&mut app, RideRequest::Call),
            Err(RideRefusal::AlreadyRiding)
        );
        assert_eq!(
            ask(&mut app, RideRequest::Dismount),
            Ok(RideDone::Dismounted)
        );
        assert!(app.world().get::<Riding>(player).is_none());
    }

    #[test]
    fn moving_cancels_the_channel() {
        let (mut app, player) = world(true);
        assert_eq!(ask(&mut app, RideRequest::Call), Ok(RideDone::Calling));
        app.world_mut()
            .get_mut::<crate::PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::Y;
        for _ in 0..40 {
            app.update();
        }
        assert!(app.world().get::<Calling>(player).is_none());
        assert!(app.world().get::<Riding>(player).is_none());
    }

    #[test]
    fn a_hostile_near_or_a_busy_rider_stops_the_call() {
        let (mut app, player) = world(true);
        app.world_mut().spawn((
            Combatant::on_team(1, 100.0),
            WorldPosition::new(10.0, 0.0, 0.0),
        ));
        assert_eq!(ask(&mut app, RideRequest::Call), Err(RideRefusal::Hostile));
        let (mut app, player_b) = world(true);
        app.world_mut().spawn((
            Combatant::on_team(1, 100.0),
            WorldPosition::new(60.0, 0.0, 0.0),
        ));
        app.world_mut()
            .get_mut::<Combatant>(player_b)
            .unwrap()
            .action = Action::Heavy;
        assert_eq!(ask(&mut app, RideRequest::Call), Err(RideRefusal::Busy));
        let _ = player;
    }

    #[test]
    fn a_rider_fires_nothing_and_a_blow_throws_them() {
        let (mut app, player) = world(true);
        mount(&mut app);
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .light = true;
        app.update();
        assert_eq!(
            app.world().get::<Combatant>(player).unwrap().action,
            Action::Ready
        );
        app.world_mut()
            .resource_mut::<Messages<CombatImpact>>()
            .write(CombatImpact {
                attacker: player,
                victim: Some(player),
                from: strata_core::WorldPos::ZERO,
                point: strata_core::WorldPos::ZERO,
                melee: false,
                dealt: 1.0,
                lethal: false,
                guard: crate::combat::GuardOutcome::Open,
                shot: None,
            });
        app.update();
        assert!(app.world().get::<Riding>(player).is_none());
        let notices: Vec<RideNotice> = app
            .world_mut()
            .resource_mut::<Messages<RideNotice>>()
            .drain()
            .collect();
        assert!(matches!(
            notices.last(),
            Some(RideNotice::Thrown {
                why: ThrownWhy::Hit,
                ..
            })
        ));
    }

    #[test]
    fn a_crash_the_step_reported_throws_the_rider() {
        let (mut app, player) = world(true);
        mount(&mut app);
        app.world_mut().get_mut::<Riding>(player).unwrap().ended = Some(RideEnd::Crash(21.0));
        app.update();
        assert!(app.world().get::<Riding>(player).is_none());
    }

    /// A rider's call takes the sheet's time and the mount carries the
    /// sheet's dials.
    #[test]
    fn a_trained_rider_calls_faster_and_mounts_with_the_dials() {
        let (mut app, player) = world(true);
        app.world_mut()
            .entity_mut(player)
            .insert(sheet(&["surveyor.routes.1", "surveyor.routes.4"]));
        assert_eq!(ask(&mut app, RideRequest::Call), Ok(RideDone::Calling));
        let left = app.world().get::<Calling>(player).unwrap().left;
        let registries = app.world().resource::<Registries>().0.clone();
        let def = registries.vehicles.get(&bike()).unwrap();
        let full = def.call.channel_s * 0.6;
        assert!(left <= full && left > full - 0.2, "{left} of {full}");
        for _ in 0..80 {
            app.update();
        }
        let riding = app.world().get::<Riding>(player).unwrap();
        assert!((riding.dials.speed.road - def.speed.road * 1.1).abs() < 1e-4);
    }

    /// A crash the step reported wears the stabled bike and the notice says
    /// how much is left; a hit and deep water cost it nothing.
    #[test]
    fn only_a_crash_wears_the_bike() {
        for (end, worn) in [
            (Some(RideEnd::Crash(24.0)), true),
            (Some(RideEnd::Water), false),
            (None, false),
        ] {
            let (mut app, player) = world(true);
            mount(&mut app);
            app.world_mut().get_mut::<Riding>(player).unwrap().ended = end;
            if end.is_none() {
                app.world_mut()
                    .resource_mut::<Messages<CombatImpact>>()
                    .write(CombatImpact {
                        attacker: player,
                        victim: Some(player),
                        from: strata_core::WorldPos::ZERO,
                        point: strata_core::WorldPos::ZERO,
                        melee: false,
                        dealt: 1.0,
                        lethal: false,
                        guard: crate::combat::GuardOutcome::Open,
                        shot: None,
                    });
            }
            app.update();
            let condition = app
                .world()
                .resource::<Stable>()
                .bike
                .as_ref()
                .unwrap()
                .condition;
            assert_eq!(condition, Some(if worn { 750 } else { 1000 }), "{end:?}");
            let notices: Vec<RideNotice> = app
                .world_mut()
                .resource_mut::<Messages<RideNotice>>()
                .drain()
                .collect();
            let Some(RideNotice::Thrown { condition, .. }) = notices.last() else {
                panic!("a throw is told: {notices:?}");
            };
            assert_eq!(*condition, worn.then_some(750), "{end:?}");
        }
    }

    /// Moving cancels a call and says so; a hostile that came near while
    /// it was on its way ends it at the end of the channel and says why.
    #[test]
    fn a_cancelled_call_is_said() {
        let told = |app: &mut App| -> Vec<RideNotice> {
            app.world_mut()
                .resource_mut::<Messages<RideNotice>>()
                .drain()
                .collect()
        };
        let (mut app, player) = world(true);
        assert_eq!(ask(&mut app, RideRequest::Call), Ok(RideDone::Calling));
        app.world_mut()
            .get_mut::<crate::PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::Y;
        let mut said = Vec::new();
        for _ in 0..3 {
            app.update();
            said.extend(told(&mut app));
        }
        assert_eq!(
            said,
            [RideNotice::Cancelled {
                why: CancelWhy::Moved
            }]
        );
        let (mut app, _) = world(true);
        assert_eq!(ask(&mut app, RideRequest::Call), Ok(RideDone::Calling));
        app.world_mut().spawn((
            Combatant::on_team(1, 100.0),
            WorldPosition::new(10.0, 0.0, 0.0),
        ));
        let mut said = Vec::new();
        for _ in 0..40 {
            app.update();
            said.extend(told(&mut app));
        }
        assert_eq!(
            said,
            [RideNotice::Cancelled {
                why: CancelWhy::Refused(RideRefusal::Hostile)
            }]
        );
    }

    /// The first day in a running sim with the ride's components on the
    /// player and a bike of `condition` in the pack.
    fn stocked(owns: bool) -> (App, Entity, Record) {
        let mut app = crate::crafting::tests::first_day();
        let registries = app.world().resource::<Registries>().clone();
        let player = {
            let world = app.world_mut();
            let mut players = world.query_filtered::<Entity, With<LocalPlayer>>();
            let player = players.single(world).unwrap();
            world.entity_mut(player).insert(crate::Player::default());
            player
        };
        let bought = record(&registries, 700, 400, 900);
        let items = &registries.0.items;
        if owns {
            let own = record(&registries, 500, 500, 1000);
            *app.world_mut().resource_mut::<Stable>() =
                Stable::with_bike(own, items, &registries.0.vehicles);
        }
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = packs.single_mut(world).unwrap();
        let cell = (0..pack.cells.len())
            .find(|cell| pack.cells.get(*cell).is_none())
            .unwrap();
        crate::crafting::tests::put(&mut pack.cells, cell, &bought, items);
        (app, player, bought)
    }

    fn pack_of(app: &mut App) -> Pack {
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&Pack, With<LocalPlayer>>();
        packs.single(world).unwrap().clone()
    }

    /// A bike in the pack becomes the ride; with one already stabled the
    /// two swap places and the old one goes to the cell the new came from.
    #[test]
    fn a_bike_is_stabled_swapped_and_put_away() {
        let (mut app, _, bought) = stocked(true);
        let own = app.world().resource::<Stable>().bike.clone().unwrap();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let before = pack_of(&mut app);
        let cell = (0..before.cells.len())
            .find(|cell| {
                before.cells.get(*cell).is_some_and(|stack| {
                    read_bike(stack, items).is_some_and(|record| record == bought)
                })
            })
            .unwrap();
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Ok(RideDone::Stabled)
        );
        let stable = app.world().resource::<Stable>().clone();
        assert_eq!(stable.bike, Some(bought.clone()));
        assert_eq!(stable.saddle.len(), 18, "sized by the cargo");
        let after = pack_of(&mut app);
        assert_eq!(
            after
                .cells
                .get(cell)
                .and_then(|stack| read_bike(stack, items)),
            Some(own.clone()),
            "the old bike is where the new one lay"
        );
        // Put away: the stabled bike goes to the pack and the stable is
        // empty, with no saddle.
        assert_eq!(ask(&mut app, RideRequest::PutAway), Ok(RideDone::PutAway));
        let stable = app.world().resource::<Stable>();
        assert_eq!((stable.bike.clone(), stable.saddle.len()), (None, 0));
        let pack = pack_of(&mut app);
        let held = |record: &Record| {
            pack.cells
                .slots()
                .iter()
                .flatten()
                .any(|stack| read_bike(stack, items).as_ref() == Some(record))
        };
        assert!(held(&bought) && held(&own), "both bikes are in the pack");
    }

    /// Every refusal of taking a bike and of putting it away.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one scenario, a refusal after a refusal"
    )]
    fn stabling_and_putting_away_are_refused_for_each_reason() {
        let (mut app, player, bought) = stocked(false);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        // Not a bike.
        let feed =
            crate::crafting::tests::part(&registries, "veyra:module.patrol_extended_feed", 1, None);
        assert_eq!(
            ask(&mut app, RideRequest::Stable { kind: feed }),
            Err(RideRefusal::NotABike)
        );
        // Not to hand.
        let absent = record(&registries, 123, 456, 789);
        assert_eq!(
            ask(&mut app, RideRequest::Stable { kind: absent }),
            Err(RideRefusal::NotHeld)
        );
        // A stack on the cursor.
        app.world_mut().get_mut::<Pack>(player).unwrap().cursor = Some(
            items::write(
                &crate::crafting::tests::part(
                    &registries,
                    "veyra:module.patrol_extended_feed",
                    1,
                    None,
                ),
                items,
            )
            .unwrap(),
        );
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Err(RideRefusal::Busy)
        );
        app.world_mut().get_mut::<Pack>(player).unwrap().cursor = None;
        // No bike to put away.
        assert_eq!(
            ask(&mut app, RideRequest::PutAway),
            Err(RideRefusal::NoBike)
        );
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Ok(RideDone::Stabled)
        );
        // A loaded saddle blocks a swap and a put-away.
        let other = record(&registries, 100, 100, 100);
        {
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let cell = (0..pack.cells.len())
                .find(|cell| pack.cells.get(*cell).is_none())
                .unwrap();
            crate::crafting::tests::put(&mut pack.cells, cell, &other, items);
        }
        let load = items::write(
            &crate::crafting::tests::part(
                &registries,
                "veyra:module.patrol_extended_feed",
                3,
                None,
            ),
            items,
        )
        .unwrap();
        app.world_mut()
            .resource_mut::<Stable>()
            .saddle
            .set(4, Some(load));
        assert_eq!(
            ask(&mut app, RideRequest::Stable { kind: other }),
            Err(RideRefusal::SaddleLoaded)
        );
        assert_eq!(
            ask(&mut app, RideRequest::PutAway),
            Err(RideRefusal::SaddleLoaded)
        );
        app.world_mut().resource_mut::<Stable>().saddle.set(4, None);
        // No room: every cell of the pack, its bags and nothing behind.
        {
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
                .map(|bag| usize::from(pack.open_cells(bag, items)))
                .collect();
            let all = pack.cells.len();
            crate::crafting::tests::fill(&mut pack.cells, all, items);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                crate::crafting::tests::fill(bag, open, items);
            }
        }
        assert_eq!(
            ask(&mut app, RideRequest::PutAway),
            Err(RideRefusal::NoRoom)
        );
        // Riding: neither.
        let hold = app.world().resource::<Stable>().bike.clone();
        let def = registries.0.vehicles.get(&bike()).unwrap();
        app.world_mut()
            .entity_mut(player)
            .insert(Riding::new(def, 0.0));
        assert_eq!(
            ask(&mut app, RideRequest::PutAway),
            Err(RideRefusal::AlreadyRiding)
        );
        assert_eq!(
            ask(&mut app, RideRequest::Stable { kind: bought }),
            Err(RideRefusal::AlreadyRiding)
        );
        assert_eq!(app.world().resource::<Stable>().bike, hold, "nothing moved");
    }

    /// The store is reached for a put-away with a locker's menu open and
    /// not otherwise.
    #[test]
    fn a_full_pack_puts_the_bike_in_the_open_store_at_a_locker() {
        let (mut app, _, bought) = stocked(false);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        assert_eq!(
            ask(&mut app, RideRequest::Stable { kind: bought }),
            Ok(RideDone::Stabled)
        );
        {
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
                .map(|bag| usize::from(pack.open_cells(bag, items)))
                .collect();
            let all = pack.cells.len();
            crate::crafting::tests::fill(&mut pack.cells, all, items);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                crate::crafting::tests::fill(bag, open, items);
            }
        }
        assert_eq!(
            ask(&mut app, RideRequest::PutAway),
            Err(RideRefusal::NoRoom)
        );
        crate::crafting::tests::prop(&mut app, InteractionKind::Locker);
        app.world_mut().resource_mut::<OpenStock>().0 = Some(items::Opened {
            menu: slotted_model::MenuId(2),
            with_store: true,
            state: slotted_model::MenuState::new(&slotted_model::MenuDef::new()),
            saddle: Inventory::new(0),
        });
        let store = app.world().resource::<HouseholdStore>().clone();
        let free = store.0.slots().iter().filter(|slot| slot.is_none()).count();
        assert!(free > 0, "the first day's store has room");
        assert_eq!(ask(&mut app, RideRequest::PutAway), Ok(RideDone::PutAway));
        let now = app.world().resource::<HouseholdStore>().0.clone();
        assert_eq!(
            now.slots().iter().filter(|slot| slot.is_none()).count(),
            free - 1
        );
        assert!(app.world().resource::<Stable>().bike.is_none());
    }

    /// A bike lying in the store is stabled from the open store at a locker,
    /// and from nowhere else: with the store shut, or away from the locker,
    /// the household does not hold it.
    #[test]
    fn a_bike_is_stabled_out_of_the_open_store_at_a_locker() {
        let (mut app, _, bought) = stocked(false);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        // Out of the pack and into the store's first empty cell.
        {
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let cell = (0..pack.cells.len())
                .find(|cell| {
                    pack.cells
                        .get(*cell)
                        .is_some_and(|s| read_bike(s, items).as_ref() == Some(&bought))
                })
                .unwrap();
            pack.cells.set(cell, None);
            pack.cells.clear_changed();
        }
        let in_store = {
            let mut store = app.world_mut().resource_mut::<HouseholdStore>();
            let cell = (0..store.0.len())
                .find(|c| store.0.get(*c).is_none())
                .unwrap();
            crate::crafting::tests::put(&mut store.0, cell, &bought, items);
            cell
        };
        // Not at a locker with the store open: not held.
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Err(RideRefusal::NotHeld)
        );
        crate::crafting::tests::prop(&mut app, InteractionKind::Locker);
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Err(RideRefusal::NotHeld),
            "a locker with the store not opened"
        );
        app.world_mut().resource_mut::<OpenStock>().0 = Some(items::Opened {
            menu: slotted_model::MenuId(2),
            with_store: true,
            state: slotted_model::MenuState::new(&slotted_model::MenuDef::new()),
            saddle: Inventory::new(0),
        });
        assert_eq!(
            ask(
                &mut app,
                RideRequest::Stable {
                    kind: bought.clone()
                }
            ),
            Ok(RideDone::Stabled)
        );
        let stable = app.world().resource::<Stable>();
        assert_eq!(stable.bike, Some(bought));
        assert_eq!(stable.saddle.len(), 18, "sized by the cargo");
        let store = app.world().resource::<HouseholdStore>();
        assert!(store.0.get(in_store).is_none(), "the bike left the store");
    }

    #[test]
    fn a_neutral_person_does_not_block_calling_the_bike() {
        use crate::encounters::NEUTRAL_TEAM;
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let nearest = |app: &mut App| {
            app.world_mut()
                .run_system_once(
                    |others: Query<(&Combatant, &WorldPosition), Without<LocalPlayer>>| {
                        nearest_hostile(0, &WorldPosition::new(0., 0., 0.), &others)
                    },
                )
                .unwrap()
        };
        app.world_mut().spawn((
            Combatant::on_team(NEUTRAL_TEAM, 30.),
            WorldPosition::new(5., 0., 0.),
        ));
        assert_eq!(nearest(&mut app), None, "a guard that is not crossed");
        app.world_mut()
            .spawn((Combatant::on_team(1, 30.), WorldPosition::new(9., 0., 0.)));
        assert_eq!(nearest(&mut app), Some(9.0));
    }
}
