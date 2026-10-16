//! Riding (ticket 0009, step 2; `docs/tickets/0009-the-field-loop.md`,
//! section 3): the household's bike is called with a short channel, the
//! player is mounted on it, and a hit, a crash or deep water throws them.
//!
//! The ride is a state of the player in the sim, [`Riding`], asked for by a
//! [`RideRequest`] in the [`RideInbox`] like every other request here, and
//! moved by a second motor in `strata_physics::character` that reads only
//! what [`Riding`] holds: the numbers are copied from the vehicle's file at
//! the mount ([`RideDials`]), so the step needs no registry and a server
//! runs the very same one. [`can_call`] is the one rule, asked by the
//! request; the client's key asks nothing and shows the answer's reason.
//!
//! Local-only like the stock: nothing here is replicated, and a household's
//! bike is the [`Stable`] resource (the second half of the ticket makes it a
//! record).

use bevy::app::{App, Plugin};
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use serde::{Deserialize, Serialize};
use slotted_model::Inventory;
use strata_registry::item::{ItemCatalogue, ItemKind, Record};
use strata_registry::progression::SkillGraph;
use strata_registry::vehicle::{
    CallRule, Crash, Lines, RoadRule, Slope, Speeds, Turn, VehicleDef, VehicleId, VehicleSet, Water,
};
use strata_registry::wear::WearRules;

use crate::combat::{Action, CombatImpact, CombatIntent, CombatSet, Combatant};
use crate::crafting::kind_of;
use crate::deposits::{Pinging, Sampling};
use crate::encounters::enemies;
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer, StoreAccess};
use crate::progression::{CharacterSheet, InteractionKind, Reach, ride_field};
use crate::{
    LocalPlayer, PlayerCommand, Registries, SimSet, SimTick, WaterHere, WorldPosition,
    WorldSourceRes, water_here,
};

/// The household's ride: the bike it has taken as its own, a record of a
/// vehicle item with its lines and its condition, and the saddle bags that
/// ride with it. The saddle is sized by the vehicle's cargo and is empty
/// without a bike.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Stable {
    /// The bike, as the item's record.
    pub bike: Option<Record>,
    /// The saddle bags.
    pub saddle: Inventory,
}

impl Default for Stable {
    fn default() -> Self {
        Self {
            bike: None,
            saddle: Inventory::new(0),
        }
    }
}

impl Stable {
    /// A stable holding `bike`, with an empty saddle of the vehicle's size.
    #[must_use]
    pub fn with_bike(bike: Record, items: &ItemCatalogue, vehicles: &VehicleSet) -> Self {
        let cells = vehicle_of(items, vehicles, &bike).map_or(0, |def| def.cargo.cells());
        Self {
            bike: Some(bike),
            saddle: Inventory::new(usize::from(cells)),
        }
    }

    /// The bike's vehicle file and record, if there is a bike and its file
    /// is known.
    #[must_use]
    pub fn vehicle<'a>(
        &'a self,
        items: &ItemCatalogue,
        vehicles: &'a VehicleSet,
    ) -> Option<(&'a VehicleDef, &'a Record)> {
        let bike = self.bike.as_ref()?;
        Some((vehicle_of(items, vehicles, bike)?, bike))
    }

    /// Whether the saddle holds anything.
    #[must_use]
    pub fn loaded(&self) -> bool {
        self.saddle.slots().iter().any(Option::is_some)
    }
}

/// The stable as a save writes it: the bike's record and the saddle slot
/// for slot without the empty slots at its end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedStable {
    /// The bike, if the household has one.
    #[serde(default)]
    pub bike: Option<Record>,
    /// What the saddle holds.
    #[serde(default)]
    pub saddle: Vec<Option<Record>>,
}

impl SavedStable {
    /// Whether the catalogue and the vehicles accept it: the bike is a
    /// record of a vehicle item whose file exists, every stack in the
    /// saddle is a record the catalogue accepts, and none is past the
    /// cargo's cells.
    ///
    /// # Errors
    /// The first rule broken, in words.
    pub fn check(&self, items: &ItemCatalogue, vehicles: &VehicleSet) -> Result<(), String> {
        let Some(bike) = &self.bike else {
            return if self.saddle.iter().any(Option::is_some) {
                Err("a saddle with no bike".to_owned())
            } else {
                Ok(())
            };
        };
        items.check_record(bike).map_err(|e| e.to_string())?;
        let def = vehicle_of(items, vehicles, bike)
            .ok_or_else(|| format!("`{}` is no bike", bike.item))?;
        if self.saddle.len() > usize::from(def.cargo.cells()) {
            return Err(format!(
                "the saddle holds {} cells, the bike {}",
                self.saddle.len(),
                def.cargo.cells()
            ));
        }
        self.saddle
            .iter()
            .flatten()
            .try_for_each(|record| items.check_record(record).map_err(|e| e.to_string()))
    }
}

impl Stable {
    /// The stable a save holds.
    ///
    /// # Errors
    /// A stack the catalogue cannot write, which [`SavedStable::check`]
    /// refuses first.
    pub fn from_saved(
        saved: &SavedStable,
        items: &ItemCatalogue,
        vehicles: &VehicleSet,
    ) -> Result<Self, items::RecordError> {
        let Some(bike) = saved.bike.clone() else {
            return Ok(Self::default());
        };
        let mut stable = Self::with_bike(bike, items, vehicles);
        stable.saddle = items::fill(stable.saddle.len(), &saved.saddle, items)?;
        Ok(stable)
    }

    /// The stable as a save writes it.
    ///
    /// # Errors
    /// A stack the catalogue cannot read, which no click produces.
    pub fn saved(&self, items: &ItemCatalogue) -> Result<SavedStable, items::RecordError> {
        Ok(SavedStable {
            bike: self.bike.clone(),
            saddle: items::unfill(&self.saddle, items)?,
        })
    }
}

/// A store grade, whole bike of `vehicle`: every line 500, full condition,
/// as the board sells it. `None` when no item is a bike of that vehicle.
#[must_use]
pub fn store_grade_bike(items: &ItemCatalogue, vehicle: &VehicleId) -> Option<Record> {
    items
        .iter()
        .find(|def| matches!(&def.kind, ItemKind::Vehicle { vehicle: of } if of == vehicle))
        .map(crate::crafting::bought)
}

/// The vehicle file a vehicle item's record is a bike of.
#[must_use]
pub fn vehicle_of<'a>(
    items: &ItemCatalogue,
    vehicles: &'a VehicleSet,
    record: &Record,
) -> Option<&'a VehicleDef> {
    match &items.get(&record.item)?.kind {
        ItemKind::Vehicle { vehicle } => vehicles.get(vehicle),
        _ => None,
    }
}

/// The numbers the motor needs, copied from the vehicle's file at the
/// mount so the physics reads no registry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideDials {
    /// Top speeds.
    pub speed: Speeds,
    /// Gathering speed, metres a second squared.
    pub accel: f32,
    /// Braking against the throttle, metres a second squared.
    pub brake: f32,
    /// The slowing with no throttle, metres a second squared.
    pub drag: f32,
    /// The turn.
    pub turn: Turn,
    /// The slope it climbs.
    pub slope: Slope,
    /// The water it crosses.
    pub water: Water,
    /// What reads as road.
    pub road: RoadRule,
    /// What a crash is.
    pub crash: Crash,
}

impl RideDials {
    /// The dials of `def`.
    #[must_use]
    pub const fn of(def: &VehicleDef) -> Self {
        Self {
            speed: def.speed,
            accel: def.accel,
            brake: def.brake,
            drag: def.drag,
            turn: def.turn,
            slope: def.slope,
            water: def.water,
            road: def.road,
            crash: def.crash,
        }
    }
}

/// The share of both top speeds a bike worn to `condition` keeps: all of
/// it from the wear rules' `slide_below` up, sliding straight down to the
/// vehicle's `worst_speed` at nothing. A bike at nothing still rides.
#[must_use]
pub fn wear_speed(def: &VehicleDef, rules: &WearRules, condition: u16) -> f32 {
    if condition >= rules.slide_below {
        return 1.0;
    }
    let t = f32::from(condition) / f32::from(rules.slide_below);
    def.wear.worst_speed * (1.0 - t) + t
}

/// The dials of `def` for the bike `bike` under the rider whose sheet is
/// `sheet`. The one place a bike's lines, a rider's skill and wear become
/// numbers, in this order: the vehicle's file, the record's lines (speed
/// scales both top speeds, handling the turn and its grip, exactly one at
/// store grade), the sheet's learned `Ride` boxes (Set, Add, Mul per field,
/// see [`strata_registry::progression::Effect::Ride`]), then the wear
/// ([`wear_speed`]).
#[must_use]
pub fn dials(
    def: &VehicleDef,
    bike: &Record,
    wear: &WearRules,
    graph: &SkillGraph,
    sheet: &CharacterSheet,
) -> RideDials {
    let mut dials = RideDials::of(def);
    let line = |name: &str| {
        bike.lines
            .get(name)
            .copied()
            .unwrap_or(strata_registry::item::STORE_GRADE)
    };
    let speed = Lines::factor(def.lines.speed, line("speed"));
    let handling = Lines::factor(def.lines.handling, line("handling"));
    dials.speed.road *= speed;
    dials.speed.ground *= speed;
    dials.turn.slow *= handling;
    dials.turn.fast *= handling;
    dials.turn.grip *= handling;
    let field = |name: &str, base: f32| ride_field(graph, sheet, name, base);
    dials.speed.road = field("speed_road", dials.speed.road);
    dials.speed.ground = field("speed_ground", dials.speed.ground);
    dials.slope.stop_deg = field("stop_deg", dials.slope.stop_deg);
    dials.turn.grip = field("grip", dials.turn.grip);
    let worn = wear_speed(def, wear, crate::wear::condition_of(wear, bike.condition));
    dials.speed.road *= worn;
    dials.speed.ground *= worn;
    dials
}

/// What a crash at `speed` costs the bike, in points of condition: the
/// vehicle's `wear.crash` at its `crash.speed_min`, more in proportion to
/// the speed and at most `speed_cap` times that, scaled by the durability
/// line by the wear rules' own [`crate::wear::cost`], in whole steps of
/// the rules' `step`.
#[must_use]
pub fn crash_cost(def: &VehicleDef, rules: &WearRules, bike: &Record, speed: f32) -> u16 {
    let share = (speed / def.crash.speed_min).clamp(0.0, def.wear.speed_cap);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a thousand times a few points, a share of three at most"
    )]
    let base = (f32::from(def.wear.crash) * 1000.0 * share).round() as u32;
    let thousandths = crate::wear::cost(rules, base, bike);
    let step = u32::from(rules.step) * 1000;
    let steps = (thousandths + step / 2) / step;
    u16::try_from(steps * u32::from(rules.step)).unwrap_or(rules.full)
}

/// Seconds the rider's call takes: the vehicle's channel after the sheet's
/// `call_s` boxes.
#[must_use]
pub fn call_s(def: &VehicleDef, graph: &SkillGraph, sheet: &CharacterSheet) -> f32 {
    ride_field(graph, sheet, "call_s", def.call.channel_s)
}

/// Why the physics step ended a ride.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RideEnd {
    /// Against something not walkable, at this speed, metres a second.
    Crash(f32),
    /// Into water deeper than the bike stalls in.
    Water,
}

/// A player mounted on a vehicle. The physics step turns [`heading`](Self::heading)
/// and moves [`speed`](Self::speed); the sim removes it when the ride ends.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Riding {
    /// Which vehicle.
    pub vehicle: VehicleId,
    /// Its numbers.
    pub dials: RideDials,
    /// Where the bike points, radians about `y`, the intent's yaw
    /// convention.
    pub heading: f32,
    /// Along the heading, signed, metres a second.
    pub speed: f32,
    /// The bike's own horizontal velocity, which swings to the heading at
    /// the grip. It lives here and not on the controller so that ending the
    /// ride drops it with the component.
    pub velocity: Vec3,
    /// Set by the step when it ended the ride; the sim throws the rider.
    pub ended: Option<RideEnd>,
}

impl Riding {
    /// A rider just mounted on `def`, at rest, pointing `heading`.
    #[must_use]
    pub fn new(def: &VehicleDef, heading: f32) -> Self {
        Self {
            vehicle: def.id.clone(),
            dials: RideDials::of(def),
            heading,
            speed: 0.0,
            velocity: Vec3::ZERO,
            ended: None,
        }
    }

    /// A rider just mounted on `def` with `dials` (see [`dials`]).
    #[must_use]
    pub fn with_dials(def: &VehicleDef, dials: RideDials, heading: f32) -> Self {
        Self {
            dials,
            ..Self::new(def, heading)
        }
    }
}

/// A bike being called: the channel, in seconds left. The player stands
/// still for it, and moving cancels it.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Calling {
    /// Seconds left.
    pub left: f32,
}

/// Why the bike was not called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RideRefusal {
    /// The household has no bike.
    #[error("there is no bike to call")]
    NoBike,
    /// In water too deep to ride or to call a bike into.
    #[error("not from the water")]
    InWater,
    /// A living hostile is too near.
    #[error("not with a hostile this near")]
    Hostile,
    /// Mid action, or down.
    #[error("not in the middle of an action")]
    Busy,
    /// Already on it, or already calling it.
    #[error("the bike is already here")]
    AlreadyRiding,
    /// What was named to be taken as the ride is no bike.
    #[error("that is not a bike")]
    NotABike,
    /// The bike named is not in the pack, its bags or an open store.
    #[error("that bike is not to hand")]
    NotHeld,
    /// The saddle bags hold something, which a swap or a put-away would
    /// leave behind.
    #[error("empty the saddle bags first")]
    SaddleLoaded,
    /// Neither the pack nor an open store has a cell for the bike.
    #[error("there is no room for it")]
    NoRoom,
}

/// What the rule needs to know of the player.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RiderState {
    /// Mounted already, or calling.
    pub riding: bool,
    /// The water over the player's feet, metres; zero or less when dry.
    /// From the feet, not the ground below them: on a bridge deck over deep
    /// water the surface is under the feet.
    pub water_m: f32,
    /// Mid action or downed.
    pub busy: bool,
    /// The distance to the nearest living hostile combatant, if any.
    pub nearest_hostile_m: Option<f64>,
}

/// Whether the household's bike can be called now, and which it is.
///
/// # Errors
/// The first of: no bike, already riding, busy, in water, a hostile near.
pub fn can_call<'a>(
    stable: &'a Stable,
    items: &ItemCatalogue,
    vehicles: &'a VehicleSet,
    state: &RiderState,
) -> Result<(&'a VehicleDef, &'a Record), RideRefusal> {
    let (def, bike) = stable.vehicle(items, vehicles).ok_or(RideRefusal::NoBike)?;
    if state.riding {
        return Err(RideRefusal::AlreadyRiding);
    }
    if state.busy {
        return Err(RideRefusal::Busy);
    }
    if state.water_m > def.water.stall_m {
        return Err(RideRefusal::InWater);
    }
    let CallRule { clear_m, .. } = def.call;
    if state
        .nearest_hostile_m
        .is_some_and(|near| near < f64::from(clear_m))
    {
        return Err(RideRefusal::Hostile);
    }
    Ok((def, bike))
}

/// The water over the feet at a player whose point is at height `y`, feet
/// `feet_m` below it.
#[must_use]
pub fn water_over_feet(here: &WaterHere, y: f64, feet_m: f32) -> f32 {
    #[allow(clippy::cast_possible_truncation, reason = "a depth in metres")]
    let feet = (y - f64::from(feet_m)) as f32;
    here.surface_m - feet
}

/// Whether the bike `kind` can be taken as the household's ride, from what
/// `pack` carries and, when a locker's menu is open over it, `store`.
/// `riding` is mounted or calling.
///
/// # Errors
/// The first of: not a bike, already riding, a stack on the cursor, not to
/// hand, the saddle loaded under the bike it would swap with.
pub fn can_stable(
    items: &ItemCatalogue,
    stable: &Stable,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    kind: &Record,
    riding: bool,
) -> Result<(), RideRefusal> {
    if !matches!(
        items.get(&kind.item).map(|def| &def.kind),
        Some(ItemKind::Vehicle { .. })
    ) {
        return Err(RideRefusal::NotABike);
    }
    if riding {
        return Err(RideRefusal::AlreadyRiding);
    }
    if pack.cursor.is_some() {
        return Err(RideRefusal::Busy);
    }
    crate::equip::find(items, pack, store, &kind_of(kind)).ok_or(RideRefusal::NotHeld)?;
    if stable.bike.is_some() && stable.loaded() {
        return Err(RideRefusal::SaddleLoaded);
    }
    Ok(())
}

/// Whether the stabled bike can go back to the pack, or else the open
/// store.
///
/// # Errors
/// The first of: no bike, riding, the saddle loaded, no room.
pub fn can_put_away(
    items: &ItemCatalogue,
    stable: &Stable,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    riding: bool,
) -> Result<(), RideRefusal> {
    put_away_into(items, stable, pack, store, riding).map(|_| ())
}

/// The stack of the stabled bike, and a pack and store it has been put
/// into, when it fits.
fn put_away_into(
    items: &ItemCatalogue,
    stable: &Stable,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    riding: bool,
) -> Result<(Pack, Option<HouseholdStore>), RideRefusal> {
    let bike = stable.bike.as_ref().ok_or(RideRefusal::NoBike)?;
    if riding {
        return Err(RideRefusal::AlreadyRiding);
    }
    if stable.loaded() {
        return Err(RideRefusal::SaddleLoaded);
    }
    let record = items::write(bike, items).map_err(|_| RideRefusal::NotABike)?;
    let mut next = (pack.clone(), Hands::default(), store.cloned());
    let mut stock = Stock {
        pack: &mut next.0,
        hands: &mut next.1,
        store: next.2.as_mut(),
        saddle: None,
    };
    match items::place(items, &mut stock, record) {
        None => Ok((next.0, next.2)),
        Some(_) => Err(RideRefusal::NoRoom),
    }
}

/// Take the bike `kind` as the household's ride, swapping it with the one
/// stabled, which goes into the cell the new one came out of. Changes
/// `stock` and `stable` only on success.
///
/// # Errors
/// What [`can_stable`] refuses.
pub fn stable_bike(
    items: &ItemCatalogue,
    vehicles: &VehicleSet,
    stock: &mut Stock<'_>,
    stable: &mut Stable,
    kind: &Record,
    riding: bool,
) -> Result<(), RideRefusal> {
    can_stable(
        items,
        stable,
        stock.pack,
        stock.store.as_deref(),
        kind,
        riding,
    )?;
    let kind = kind_of(kind);
    let cell = crate::equip::find(items, stock.pack, stock.store.as_deref(), &kind)
        .ok_or(RideRefusal::NotHeld)?;
    let leaving = stable
        .bike
        .as_ref()
        .and_then(|bike| items::write(bike, items).ok());
    let inventory = crate::equip::inventory_of(cell, stock).ok_or(RideRefusal::NotHeld)?;
    let index = cell.index();
    let arriving = inventory.take(index);
    inventory.set(index, leaving);
    inventory.clear_changed();
    let bike = arriving
        .and_then(|stack| items::read(&stack, items).ok())
        .ok_or(RideRefusal::NotHeld)?;
    *stable = Stable::with_bike(bike, items, vehicles);
    Ok(())
}

/// Put the stabled bike back in the pack, its bags or the store, by the
/// stock's put-away rule. Changes `stock` and `stable` only on success.
///
/// # Errors
/// What [`can_put_away`] refuses.
pub fn put_bike_away(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    stable: &mut Stable,
    riding: bool,
) -> Result<(), RideRefusal> {
    let (pack, store) = put_away_into(items, stable, stock.pack, stock.store.as_deref(), riding)?;
    *stock.pack = pack;
    if let (Some(into), Some(store)) = (stock.store.as_mut(), store) {
        **into = store;
    }
    *stable = Stable::default();
    Ok(())
}

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
#[derive(Resource, Debug, Default)]
pub struct RideInbox(pub Vec<RideRequest>);

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

/// Why a rider was thrown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrownWhy {
    /// A blow landed on the rider.
    Hit,
    /// The bike hit something at speed.
    Crash,
    /// The bike stalled in deep water.
    Water,
}

/// Why a call ended before the rider was on the bike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelWhy {
    /// The rider moved.
    Moved,
    /// The rule no longer held when the channel ended.
    Refused(RideRefusal),
}

/// What happened to the ride, for the client and for wear.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum RideNotice {
    /// The call ended with no rider mounted.
    Cancelled {
        /// Why.
        why: CancelWhy,
    },
    /// The rider is on the bike.
    Mounted,
    /// The rider got off.
    Dismounted,
    /// The rider was thrown, at this speed (metres a second).
    Thrown {
        /// Why.
        why: ThrownWhy,
        /// The bike's speed when it happened.
        speed: f32,
        /// The bike's condition after a crash, which alone costs it.
        condition: Option<u16>,
    },
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
    reason = "the inbox, the stable, the stock and its menu, the world, the player and the hostiles in, two messages out"
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
    mut players: Query<
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
            Option<&mut Pack>,
            Option<&mut Hands>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<RideAnswer>,
    mut notices: MessageWriter<RideNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let Ok((
        player,
        command,
        at,
        combatant,
        riding,
        calling,
        pinging,
        sampling,
        sheet,
        mut pack,
        mut hands,
    )) = players.single_mut()
    else {
        inbox.0.clear();
        return;
    };
    let items = &registries.0.items;
    let vehicles = &registries.0.vehicles;
    let graph = &registries.0.progression;
    let sheet = sheet.cloned().unwrap_or_default();
    let state = |stable: &Stable| RiderState {
        riding: riding.is_some() || calling.is_some(),
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
    let mut calling = calling.cloned();
    let mut mounted = riding.is_some();
    let mut stocked = false;
    for request in std::mem::take(&mut inbox.0) {
        match request {
            RideRequest::Call => {
                let result = can_call(&stable, items, vehicles, &state(&stable)).map(|(def, _)| {
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
                            stocked = true;
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
    }
    if stocked
        && let Some(opened) = open.0.as_mut()
        && let (Some(pack), Some(hands)) = (pack.as_deref(), hands.as_deref())
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
    let mut still = state(&stable);
    still.riding = false;
    match can_call(&stable, items, vehicles, &still) {
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
        app.init_resource::<RideInbox>()
            .init_resource::<Stable>()
            .add_message::<RideAnswer>()
            .add_message::<RideNotice>()
            .add_systems(
                SimTick,
                (
                    consume
                        .in_set(SimSet::Input)
                        .after(crate::rework::consume)
                        .after(crate::deposits::consume)
                        .after(crate::items::consume)
                        .after(crate::wear::consume)
                        .after(crate::player::consume_player_intents),
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
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;
    use std::sync::Arc;
    use strata_registry::load_from_dir;

    const BIKE: &str = "veyra:vehicle.courier_bike";

    fn bike() -> VehicleId {
        VehicleId::parse(BIKE).unwrap()
    }

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
        app.world_mut().resource_mut::<RideInbox>().0.push(request);
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

    fn sheet(boxes: &[&str]) -> CharacterSheet {
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    /// The ride's dials with no Routes box are the file's, and each box moves
    /// the one number it names and no other.
    #[test]
    fn each_routes_box_moves_its_own_dial() {
        let registries =
            load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap();
        let graph = &registries.progression;
        let def = registries.vehicles.get(&bike()).unwrap();
        let record = store_grade_bike(&registries.items, &bike()).unwrap();
        let dials =
            |sheet: &CharacterSheet| dials(def, &record, registries.items.wear(), graph, sheet);
        let none = dials(&CharacterSheet::default());
        assert_eq!(none, RideDials::of(def), "no box, the file");
        assert!((call_s(def, graph, &CharacterSheet::default()) - def.call.channel_s).abs() < 1e-6);
        // Other trees' boxes change nothing here.
        assert_eq!(dials(&sheet(&["blade.novice", "surveyor.novice"])), none);
        let one = |id: &str| dials(&sheet(&[id]));
        let mut want = none;
        want.speed.ground *= 1.1;
        assert_eq!(one("surveyor.routes.2"), want);
        let mut want = none;
        want.slope.stop_deg += 5.0;
        assert_eq!(one("surveyor.routes.3"), want);
        let mut want = none;
        want.speed.road *= 1.1;
        assert_eq!(one("surveyor.routes.4"), want);
        assert_eq!(one("surveyor.routes.1"), none, "its box is the call's");
        let quick = call_s(def, graph, &sheet(&["surveyor.routes.1"]));
        assert!((quick - def.call.channel_s * 0.6).abs() < 1e-6);
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

    fn record(registries: &Registries, speed: u16, handling: u16, condition: u16) -> Record {
        let mut record = store_grade_bike(&registries.0.items, &bike()).unwrap();
        record.lines.insert("speed".to_owned(), speed);
        record.lines.insert("handling".to_owned(), handling);
        record.condition = Some(condition);
        record
    }

    fn shipped() -> Registries {
        Registries(Arc::new(
            load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap(),
        ))
    }

    /// Store grade and whole reads exactly the file; the lines scale the
    /// top speeds and the turn straight between their ends; wear below
    /// `slide_below` slides both top speeds to the vehicle's worst.
    #[test]
    fn the_lines_and_the_wear_become_the_dials_in_one_place() {
        let registries = shipped();
        let def = registries.0.vehicles.get(&bike()).unwrap();
        let rules = registries.0.items.wear();
        let graph = &registries.0.progression;
        let sheet = CharacterSheet::default();
        let read = |speed, handling, condition| {
            dials(
                def,
                &record(&registries, speed, handling, condition),
                rules,
                graph,
                &sheet,
            )
        };
        let file = RideDials::of(def);
        assert_eq!(
            read(500, 500, rules.full),
            file,
            "a bought bike is the file"
        );
        let near = |a: f32, b: f32| assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        let worst = read(0, 0, rules.full);
        near(worst.speed.road, 60.0 * 0.90);
        near(worst.speed.ground, 36.0 * 0.90);
        near(worst.turn.slow, 150.0 * 0.85);
        near(worst.turn.fast, 85.0 * 0.85);
        near(worst.turn.grip, 8.0 * 0.85);
        near(worst.speed.reverse, 6.0);
        let best = read(1000, 1000, rules.full);
        near(best.speed.road, 60.0 * 1.10);
        near(best.speed.ground, 36.0 * 1.10);
        near(best.turn.slow, 150.0 * 1.15);
        near(best.turn.grip, 8.0 * 1.15);
        let half = read(750, 250, rules.full);
        near(half.speed.road, 60.0 * 1.05);
        near(half.turn.fast, 85.0 * 0.925);
        // Wear: nothing at slide_below, straight down to worst_speed.
        assert_eq!(read(500, 500, rules.slide_below), file);
        let half_worn = read(500, 500, rules.slide_below / 2);
        near(half_worn.speed.road, 60.0 * 0.8);
        near(half_worn.speed.ground, 36.0 * 0.8);
        near(half_worn.turn.slow, 150.0);
        let broken = read(500, 500, 0);
        near(broken.speed.road, 60.0 * 0.6);
        near(broken.speed.ground, 36.0 * 0.6);
        assert!(broken.speed.road > 0.0, "a bike at nothing still rides");
        // The order: the lines, then the wear, multiply.
        let both = read(0, 500, 0);
        near(both.speed.road, 60.0 * 0.9 * 0.6);
    }

    /// A crash costs the vehicle's points at its minimum speed, more with
    /// speed up to the cap, scaled by the durability line, in the wear
    /// rules' steps.
    #[test]
    fn a_crash_costs_by_speed_and_durability_in_steps() {
        let registries = shipped();
        let def = registries.0.vehicles.get(&bike()).unwrap();
        let rules = registries.0.items.wear();
        assert_eq!((def.wear.crash, def.crash.speed_min), (120, 12.0));
        assert_eq!(rules.step, 25);
        let with = |durability: u16| {
            let mut bike = record(&registries, 500, 500, rules.full);
            bike.lines.insert("durability".to_owned(), durability);
            bike
        };
        let cost = |durability, speed| crash_cost(def, rules, &with(durability), speed);
        // Store grade durability reads the dial: 120 points, in steps of 25.
        assert_eq!(cost(500, 12.0), 125);
        assert_eq!(cost(500, 24.0), 250);
        // Three times the minimum is the cap, however fast.
        assert_eq!(cost(500, 36.0), 350);
        assert_eq!(cost(500, 90.0), 350);
        // A well-made bike pays less, a rough one more.
        assert_eq!(cost(1000, 24.0), 125);
        assert_eq!(cost(0, 12.0), 175);
        assert!(cost(1000, 24.0) < cost(500, 24.0));
        assert!(cost(0, 24.0) > cost(500, 24.0));
        assert_eq!(cost(500, 3.0), 25, "a slow knock still costs a step");
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

    /// The water a call is refused in is over the feet: a deck over a deep
    /// river is dry to a rider on it, the bed itself is not.
    #[test]
    fn a_call_from_a_bridge_deck_over_deep_water_is_allowed() {
        let registries = shipped();
        let items = &registries.0.items;
        let vehicles = &registries.0.vehicles;
        let def = vehicles.get(&bike()).unwrap();
        let record = store_grade_bike(items, &bike()).unwrap();
        let stable = Stable::with_bike(record, items, vehicles);
        let river = WaterHere {
            surface_m: 2.0,
            ground_m: 0.0,
            flow: [0.0, 0.0],
            fall: false,
        };
        assert!(river.depth_m() > def.water.stall_m, "deep water");
        let call = |y: f64| {
            let state = RiderState {
                water_m: water_over_feet(&river, y, def.call.feet_m),
                ..RiderState::default()
            };
            can_call(&stable, items, vehicles, &state).map(|_| ())
        };
        // On a deck six metres up, the point a feet-height above it.
        assert_eq!(call(8.0 + f64::from(def.call.feet_m)), Ok(()));
        // On the river bed itself.
        assert_eq!(call(f64::from(def.call.feet_m)), Err(RideRefusal::InWater));
        // Wading to the knee is still a call.
        assert_eq!(call(1.7 + f64::from(def.call.feet_m)), Ok(()));
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

    fn read_bike(stack: &slotted_model::ItemStack, items: &ItemCatalogue) -> Option<Record> {
        items::read(stack, items).ok()
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
