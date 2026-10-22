//! Household state the rules below the field share: the purse, the stable
//! and the rider's components.
//!
//! These types sat in `deposits` and `ride`, which made the stock, the wear
//! rules and the bench read upward to name a credit or a bike. Here they
//! have no rule of their own, so every layer may name them; the rules that
//! move them stay where they were and the old paths re-export.

use bevy_ecs::prelude::*;

use bevy_math::Vec3;

use slotted_model::Inventory;

use strata_registry::item::{ItemCatalogue, ItemKind, Record};

use strata_registry::vehicle::{
    Crash, RoadRule, Slope, Speeds, Turn, VehicleDef, VehicleId, VehicleSet, Water,
};

/// The household's credits, which buying spends and selling earns. The
/// game saves it with the household.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Purse(pub u32);

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

    /// A rider just mounted on `def` with `dials` (see [`crate::ride::dials`]).
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
