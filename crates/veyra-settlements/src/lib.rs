//! Veyra's settlements: the plans `tools/content` bakes into buildings and the
//! world clears ground for.
//!
//! A settlement is a set of ashlar buildings in one frame, the settlement's
//! own, with the few points the rest of the game needs to find its way in:
//! where a walker can start, and where the road out leaves. The settlement
//! brings its ground (`strata_worldgen::Site`), and the plan is fitted to
//! it: [`blockout::port_serein`] lays Port Serein out on the terraces, the
//! platforms and the quay that ground has.
use ashlar::Building;

pub mod blockout;
pub mod landmarks;
mod materials;
pub mod sites;
pub mod skyline;
pub mod types;

pub use materials::{graphs, materials};

/// One settlement, laid out and ready to bake.
#[derive(Clone, Debug)]
pub struct SettlementPlan {
    /// The settlement's stable identity, which a registry entry and the
    /// world's site name.
    pub id: &'static str,
    /// The buildings: the landmarks, the kits by district, and the
    /// structure's masses.
    pub buildings: Vec<PlannedBuilding>,
    /// A street-level point in the city, clear of buildings, where a walker
    /// can start.
    pub spawn: [f64; 3],
    /// Where a walker is headed from [`Self::spawn`]: a point on open
    /// ground a few steps on, the layout's lobby.
    pub lobby: [f64; 3],
    /// The point on the settlement's edge where the one road into the
    /// wilderness leaves.
    pub road_exit: [f64; 2],
    /// The lamps and floodlights that light the streets at night.
    pub lights: Vec<PlannedLight>,
    /// The trees and bushes the plan planted, of the wilderness's own
    /// species, which the game draws and collides as it does the forest's.
    pub plants: Vec<PlannedPlant>,
}

/// A plant of a plan: a flora registry species planted where the plan put
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedPlant {
    /// The flora registry id, `strata:oak`.
    pub species: &'static str,
    /// The root in the settlement frame; `y` is the world height.
    pub position: [f64; 3],
    /// Rotation about the vertical, in turns.
    pub yaw: f32,
    /// Size multiplier; the game clamps it into the species' range.
    pub scale: f32,
    /// Which of its variants.
    pub variant: u8,
}

/// A light of a plan: where a lamp's or a floodlight's light comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedLight {
    /// In the settlement frame; `y` is the world height.
    pub position: [f64; 3],
    /// Linear RGB.
    pub color: [f32; 3],
    /// Luminous power, lumens.
    pub lumens: f32,
    /// How far it reaches, metres.
    pub range: f32,
}

/// One building of a settlement and where it stands.
#[derive(Clone, Debug)]
pub struct PlannedBuilding {
    /// A file stem, unique within the plan.
    pub name: String,
    /// The validated recipe.
    pub building: Building,
    /// The building's origin in the settlement frame; y = 0 is the platform.
    pub position: [f64; 3],
    /// Radians about +Y.
    pub yaw: f64,
    /// `[min_x, min_z, max_x, max_z]` in the settlement frame after the
    /// pose: the ground the building claims, which vegetation keeps clear of.
    pub footprint: [f64; 4],
}

/// Port Serein's registry id.
pub const PORT_SEREIN: &str = "strata:port_serein";
