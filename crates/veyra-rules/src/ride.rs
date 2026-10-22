//! Riding (ticket 0009, step 2; `docs/tickets/0009-the-field-loop.md`,
//! section 3): the household's bike is called with a short channel, the
//! player is mounted on it, and a hit, a crash or deep water throws them.
//!
//! The ride is a state of the player in the sim, [`Riding`], asked for by a
//! `RideRequest` in the `RideInbox` like every other request here, and
//! moved by a second motor in `strata_physics::character` that reads only
//! what [`Riding`] holds: the numbers are copied from the vehicle's file at
//! the mount ([`RideDials`]), so the step needs no registry and a server
//! runs the very same one. [`can_call`] is the one rule, asked by the
//! request; the client's key asks nothing and shows the answer's reason.
//!
//! Local-only like the stock: nothing here is replicated, and a household's
//! bike is the [`Stable`] resource (the second half of the ticket makes it a
//! record).

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use strata_registry::item::{ItemCatalogue, ItemKind, Record};

use strata_registry::progression::SkillGraph;

use strata_registry::vehicle::{CallRule, Lines, VehicleDef, VehicleId, VehicleSet};

use strata_registry::wear::WearRules;

pub use crate::household::{Calling, RideDials, RideEnd, Riding, Stable, vehicle_of};

use crate::items::kind_of;

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::progression::{CharacterSheet, ride_field};

use strata_types::WaterHere;

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

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    use crate::items;

    use crate::progression::CharacterSheet;

    use std::sync::Arc;
    use strata_registry::item::ItemCatalogue;

    use strata_registry::item::Record;
    use strata_registry::load_from_dir;

    use strata_registry::vehicle::VehicleId;

    use strata_types::Registries;
    use strata_types::WaterHere;

    pub const BIKE: &str = "veyra:vehicle.courier_bike";

    pub fn bike() -> VehicleId {
        VehicleId::parse(BIKE).unwrap()
    }

    pub fn sheet(boxes: &[&str]) -> CharacterSheet {
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

    pub fn record(registries: &Registries, speed: u16, handling: u16, condition: u16) -> Record {
        let mut record = store_grade_bike(&registries.0.items, &bike()).unwrap();
        record.lines.insert("speed".to_owned(), speed);
        record.lines.insert("handling".to_owned(), handling);
        record.condition = Some(condition);
        record
    }

    pub fn shipped() -> Registries {
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

    pub fn read_bike(stack: &slotted_model::ItemStack, items: &ItemCatalogue) -> Option<Record> {
        items::read(stack, items).ok()
    }
}
