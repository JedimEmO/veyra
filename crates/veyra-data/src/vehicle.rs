//! Vehicles: what a household can ride (ticket 0009, step 2;
//! `docs/tickets/0009-the-field-loop.md`, section 3), as
//! `vehicles/*.vehicle.ron` spells them, one a file.
//!
//! Every dial of the ride is data: the top speeds by ground, the turn, the
//! slope it takes, the water it crosses, what a crash is, the call and how
//! it is drawn. The sim copies the numbers it needs onto the rider at the
//! mount, so the physics step reads no registry. None of it is world state:
//! it joins no world version and no handshake. A data set without the
//! directory has no vehicles, and that is legal.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

/// The directory of `.vehicle.ron` files, relative to the asset root.
pub const VEHICLES_DIR: &str = "vehicles";
/// The suffix a vehicle file must carry to be read.
pub const VEHICLE_SUFFIX: &str = ".vehicle.ron";
/// The only version of the file this build reads.
pub const VEHICLE_VERSION: u32 = 1;

/// A vehicle's id, `veyra:vehicle.courier_bike`.
pub type VehicleId = Namespaced;

/// Where the content step writes the index of the baked vehicle models,
/// under its output root; the props go beside it.
pub const MODELS_PATH: &str = "vehicles/veyra/models.ron";

/// Which baked prop draws which vehicle, written by the content step and
/// read by the client. Art only: in no version and no handshake, and a
/// client without it stands a plain box.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleModels {
    /// The material library the props' keys are defined in, relative to the
    /// generated root.
    pub materials: String,
    /// The prop file of each vehicle, relative to the generated root.
    pub files: std::collections::BTreeMap<String, String>,
    /// The square icon of each vehicle, relative to the generated root; an
    /// older index has none.
    #[serde(default)]
    pub icons: std::collections::BTreeMap<String, String>,
}

impl VehicleModels {
    /// Read an index from the text the content step wrote.
    ///
    /// # Errors
    /// Text that is not an index.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// The index as the content step writes it.
    ///
    /// # Panics
    /// Never: every field serialises.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a model index serialises")
    }

    /// The prop that draws `id`, if baked.
    #[must_use]
    pub fn file(&self, id: &VehicleId) -> Option<&str> {
        self.files.get(&id.to_string()).map(String::as_str)
    }

    /// The square icon that draws `id` in a cell, if baked.
    #[must_use]
    pub fn icon(&self, id: &VehicleId) -> Option<&str> {
        self.icons.get(&id.to_string()).map(String::as_str)
    }
}

/// Top speeds, metres a second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speeds {
    /// On a road.
    pub road: f32,
    /// Off one.
    pub ground: f32,
    /// Backwards.
    pub reverse: f32,
}

/// The turn, degrees a second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    /// At a crawl.
    pub slow: f32,
    /// At top speed.
    pub fast: f32,
    /// How fast the velocity swings round to the heading, per second.
    pub grip: f32,
    /// Half the angle, in degrees, between the camera and the bike's
    /// heading inside which forward is the throttle and back the brake.
    pub throttle_arc_deg: f32,
    /// The stick's size under which it asks for nothing.
    pub dead_zone: f32,
}

/// The slope it climbs, degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slope {
    /// Up to here no speed is lost.
    pub full_deg: f32,
    /// Here and steeper it will not climb.
    pub stop_deg: f32,
}

/// The water it crosses, metres over the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Water {
    /// This deep or less is ridden over at full speed.
    pub ride_m: f32,
    /// Deeper than this throws the rider.
    pub stall_m: f32,
    /// The top speed at `stall_m`, metres a second.
    pub crawl: f32,
}

/// What reads as road.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadRule {
    /// A sample's `cleared` at or above this is road.
    pub cleared_min: f32,
    /// A settlement's ground (`developed`) at or above this is road too.
    pub developed_min: f32,
}

/// What a crash is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crash {
    /// Slower than this nothing is a crash, metres a second.
    pub speed_min: f32,
    /// The share of the speed one tick must lose against something not
    /// walkable.
    pub lost_share: f32,
}

/// Calling it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRule {
    /// The channel, seconds.
    pub channel_s: f32,
    /// No living hostile nearer than this, metres.
    pub clear_m: f32,
    /// How far below the player's point their feet are, metres: the water
    /// a call is refused in is measured over the feet, so a bridge deck
    /// over a deep river is not the river's bed. The character's half
    /// height.
    pub feet_m: f32,
}

/// How it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// How high it hovers, metres.
    pub hover_m: f32,
    /// The camera's boom while mounted, metres.
    pub camera_distance: f32,
    /// The camera's eye height while mounted, metres.
    pub camera_eye: f32,
    /// Where the rider sits in the model's frame.
    pub seat: [f32; 3],
    /// How far the bike leans into a turn at the slow-turn rate, degrees.
    pub lean_deg: f32,
    /// How far the nose pitches with the slope at most, radians.
    pub pitch_max: f32,
    /// The bob over the hover, metres.
    pub bob_m: f32,
    /// The bob's period, seconds.
    pub bob_period_s: f32,
    /// How long the camera takes to pull onto the bike and back, seconds.
    pub ease_s: f32,
    /// How fast the lean and the pitch follow what they are for, per second.
    pub follow: f32,
}

/// The factor a quality line gives, at line 0 and at line 1000, straight
/// between, so a line of 500 (store grade) is exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lines {
    /// Top speeds, road and ground: the factor at line 0 and at line 1000.
    pub speed: [f32; 2],
    /// The turn and its grip: the factor at line 0 and at line 1000.
    pub handling: [f32; 2],
}

impl Lines {
    /// The factor `span` gives at `line`: its first at 0, one at 500, its
    /// second at 1000, straight between each pair.
    #[must_use]
    pub fn factor(span: [f32; 2], line: u16) -> f32 {
        let (grade, top) = (
            f32::from(crate::item::STORE_GRADE),
            f32::from(crate::item::QUALITY_MAX),
        );
        // Exactly one at a store grade line, whatever the span's start.
        if line == crate::item::STORE_GRADE {
            return 1.0;
        }
        let line = f32::from(line).min(top);
        if line <= grade {
            span[0] + (1.0 - span[0]) * line / grade
        } else {
            1.0 + (span[1] - 1.0) * (line - grade) / (top - grade)
        }
    }
}

/// The saddle bags, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cargo {
    /// Cells across.
    pub columns: u16,
    /// Cells down.
    pub rows: u16,
}

impl Cargo {
    /// How many cells the saddle holds.
    #[must_use]
    pub const fn cells(&self) -> u16 {
        self.columns * self.rows
    }
}

/// What a crash and a worn bike cost.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleWear {
    /// Condition points one crash at `crash.speed_min` costs, before the
    /// durability line.
    pub crash: u16,
    /// A crash costs at most this many times `crash`, however fast.
    pub speed_cap: f32,
    /// The share of both top speeds left at condition 0, reached straight
    /// from wear.ron's `slide_below`.
    pub worst_speed: f32,
}

/// A vehicle, as the file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleDef {
    /// The version of the format.
    pub version: u32,
    /// The namespaced id.
    pub id: VehicleId,
    /// What the screens call it.
    pub name: String,
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
    /// The slope.
    pub slope: Slope,
    /// The water.
    pub water: Water,
    /// What is road.
    pub road: RoadRule,
    /// What is a crash.
    pub crash: Crash,
    /// Calling it.
    pub call: CallRule,
    /// Drawing it.
    pub view: View,
    /// What its quality lines do.
    pub lines: Lines,
    /// The saddle bags.
    pub cargo: Cargo,
    /// What a crash costs it.
    pub wear: VehicleWear,
}

/// Why a vehicle was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VehicleError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// A file of another version than this build reads.
    #[error("version {found}, this build reads {expected}")]
    Version {
        /// The version the file states.
        found: u32,
        /// The version this build reads.
        expected: u32,
    },
    /// Two files define one vehicle.
    #[error("`{0}` is defined twice")]
    Duplicate(VehicleId),
    /// A dial breaks a rule.
    #[error("`{vehicle}`: {reason}")]
    Dial {
        /// The vehicle.
        vehicle: VehicleId,
        /// What is wrong.
        reason: String,
    },
}

impl VehicleDef {
    /// Check every dial: all positive, the turn slower at speed, the slope's
    /// two angles in order, the water's two depths in order, and the road
    /// faster than the ground.
    ///
    /// # Errors
    /// The first rule broken.
    pub fn check(&self) -> Result<(), String> {
        let positive = [
            ("speed.road", self.speed.road),
            ("speed.ground", self.speed.ground),
            ("speed.reverse", self.speed.reverse),
            ("accel", self.accel),
            ("brake", self.brake),
            ("drag", self.drag),
            ("turn.slow", self.turn.slow),
            ("turn.fast", self.turn.fast),
            ("turn.grip", self.turn.grip),
            ("slope.full_deg", self.slope.full_deg),
            ("slope.stop_deg", self.slope.stop_deg),
            ("water.ride_m", self.water.ride_m),
            ("water.stall_m", self.water.stall_m),
            ("water.crawl", self.water.crawl),
            ("road.cleared_min", self.road.cleared_min),
            ("road.developed_min", self.road.developed_min),
            ("crash.speed_min", self.crash.speed_min),
            ("crash.lost_share", self.crash.lost_share),
            ("call.channel_s", self.call.channel_s),
            ("call.clear_m", self.call.clear_m),
            ("call.feet_m", self.call.feet_m),
            ("view.hover_m", self.view.hover_m),
            ("view.camera_distance", self.view.camera_distance),
            ("view.camera_eye", self.view.camera_eye),
            ("view.lean_deg", self.view.lean_deg),
            ("view.pitch_max", self.view.pitch_max),
            ("view.bob_m", self.view.bob_m),
            ("view.bob_period_s", self.view.bob_period_s),
            ("view.ease_s", self.view.ease_s),
            ("view.follow", self.view.follow),
            ("turn.throttle_arc_deg", self.turn.throttle_arc_deg),
            ("turn.dead_zone", self.turn.dead_zone),
            ("wear.crash", f32::from(self.wear.crash)),
            ("wear.speed_cap", self.wear.speed_cap),
            ("wear.worst_speed", self.wear.worst_speed),
            ("lines.speed", self.lines.speed[0]),
            ("lines.speed", self.lines.speed[1]),
            ("lines.handling", self.lines.handling[0]),
            ("lines.handling", self.lines.handling[1]),
        ];
        for (name, value) in positive {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!("`{name}` must be a positive number"));
            }
        }
        if self.turn.slow <= self.turn.fast {
            return Err("`turn.slow` must be above `turn.fast`".to_owned());
        }
        if self.slope.full_deg >= self.slope.stop_deg || self.slope.stop_deg >= 90.0 {
            return Err("`slope.full_deg` must be under `slope.stop_deg`, under 90".to_owned());
        }
        if self.water.ride_m >= self.water.stall_m {
            return Err("`water.ride_m` must be under `water.stall_m`".to_owned());
        }
        if self.speed.road <= self.speed.ground {
            return Err("`speed.road` must be above `speed.ground`".to_owned());
        }
        if self.crash.lost_share >= 1.0 {
            return Err("`crash.lost_share` must be under 1".to_owned());
        }
        if self.turn.throttle_arc_deg >= 180.0 || self.turn.dead_zone >= 1.0 {
            return Err(
                "`turn.throttle_arc_deg` is under 180 and `turn.dead_zone` under 1".to_owned(),
            );
        }
        for (name, span) in [
            ("lines.speed", self.lines.speed),
            ("lines.handling", self.lines.handling),
        ] {
            if span[0] > 1.0 || span[1] < 1.0 {
                return Err(format!("`{name}` must run from at most 1 to at least 1"));
            }
        }
        if self.cargo.columns == 0
            || self.cargo.rows == 0
            || self.cargo.columns > 12
            || self.cargo.rows > 12
        {
            return Err("`cargo` must be 1 to 12 columns and rows".to_owned());
        }
        if self.wear.speed_cap < 1.0 || self.wear.worst_speed > 1.0 {
            return Err(
                "`wear.speed_cap` must be at least 1 and `wear.worst_speed` at most 1".to_owned(),
            );
        }
        if self.view.seat.iter().any(|v| !v.is_finite()) {
            return Err("`view.seat` must be finite".to_owned());
        }
        Ok(())
    }
}

/// Every vehicle, checked, in id order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VehicleSet {
    vehicles: Vec<VehicleDef>,
}

impl VehicleSet {
    /// Check `defs`.
    ///
    /// # Errors
    /// The path of the first file that breaks a rule, and which.
    pub fn build(mut defs: Vec<(String, VehicleDef)>) -> Result<Self, (String, VehicleError)> {
        defs.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
        for pair in defs.windows(2) {
            if pair[0].1.id == pair[1].1.id {
                return Err((
                    pair[1].0.clone(),
                    VehicleError::Duplicate(pair[1].1.id.clone()),
                ));
            }
        }
        for (path, def) in &defs {
            if def.version != VEHICLE_VERSION {
                return Err((
                    path.clone(),
                    VehicleError::Version {
                        found: def.version,
                        expected: VEHICLE_VERSION,
                    },
                ));
            }
            def.check().map_err(|reason| {
                (
                    path.clone(),
                    VehicleError::Dial {
                        vehicle: def.id.clone(),
                        reason,
                    },
                )
            })?;
        }
        Ok(Self {
            vehicles: defs.into_iter().map(|(_, def)| def).collect(),
        })
    }

    /// One vehicle by id.
    #[must_use]
    pub fn get(&self, id: &VehicleId) -> Option<&VehicleDef> {
        self.vehicles.iter().find(|vehicle| vehicle.id == *id)
    }

    /// Every vehicle, in id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &VehicleDef> {
        self.vehicles.iter()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vehicles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::load_from_dir;

    const FILE: &str = include_str!("../../../assets/vehicles/courier_bike.vehicle.ron");

    fn built(text: &str) -> Result<VehicleSet, String> {
        let def: VehicleDef = ron::from_str(text).map_err(|e| e.to_string())?;
        VehicleSet::build(vec![("vehicles/test.vehicle.ron".to_owned(), def)])
            .map_err(|(_, error)| error.to_string())
    }

    #[track_caller]
    fn refused(from: &str, to: &str, reason: &str) {
        let changed = FILE.replace(from, to);
        assert_ne!(changed, FILE, "`{from}` is in the file");
        let error = built(&changed).expect_err("the edit is refused");
        assert!(error.contains(reason), "`{error}` should say `{reason}`");
    }

    #[test]
    fn the_shipped_bike_loads() {
        let registries =
            load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap();
        let id = Namespaced::parse("veyra:vehicle.courier_bike").unwrap();
        let bike = registries.vehicles.get(&id).expect("the bike is there");
        assert_eq!(bike.name, "Courier bike");
        assert!((bike.speed.road - 60.0).abs() < 1e-6);
        assert_eq!(registries.vehicles.iter().len(), 1);
    }

    #[test]
    fn a_dial_out_of_order_is_refused() {
        refused(
            "slow: 150.0, fast: 85.0",
            "slow: 50.0, fast: 85.0",
            "turn.slow",
        );
        refused(
            "full_deg: 14.0, stop_deg: 36.0",
            "full_deg: 40.0, stop_deg: 36.0",
            "slope",
        );
        refused(
            "ride_m: 0.5, stall_m: 1.1",
            "ride_m: 1.5, stall_m: 1.1",
            "water.ride_m",
        );
        refused(
            "road: 60.0, ground: 36.0",
            "road: 10.0, ground: 36.0",
            "speed.road",
        );
        refused("accel: 16.0", "accel: 0.0", "accel");
        refused("version: 1", "version: 2", "version 2");
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "store grade is exactly the file's number, not near it"
    )]
    fn the_lines_run_straight_and_store_grade_is_exactly_one() {
        let bike = built(FILE).unwrap();
        let def = bike.iter().next().unwrap();
        for span in [def.lines.speed, def.lines.handling] {
            assert!((Lines::factor(span, 0) - span[0]).abs() < 1e-6);
            assert_eq!(Lines::factor(span, 500), 1.0);
            assert!((Lines::factor(span, 1000) - span[1]).abs() < 1e-6);
            let quarter = Lines::factor(span, 250);
            assert!((quarter - f32::midpoint(span[0], 1.0)).abs() < 1e-6);
            assert_eq!(Lines::factor(span, 2000), Lines::factor(span, 1000));
        }
        assert_eq!(def.cargo.cells(), 18);
    }

    #[test]
    fn the_lines_the_cargo_and_the_wear_are_held_to_their_rules() {
        refused("speed: (0.90, 1.10)", "speed: (1.10, 1.10)", "lines.speed");
        refused("speed: (0.90, 1.10)", "speed: (0.90, 0.95)", "lines.speed");
        refused(
            "handling: (0.85, 1.15)",
            "handling: (0.0, 1.15)",
            "lines.handling",
        );
        refused("columns: 6", "columns: 0", "cargo");
        refused("rows: 3", "rows: 13", "cargo");
        refused("crash: 120", "crash: 0", "wear.crash");
        refused("speed_cap: 3.0", "speed_cap: 0.5", "wear.speed_cap");
        refused("worst_speed: 0.6", "worst_speed: 1.5", "wear.worst_speed");
        refused("dead_zone: 0.1", "dead_zone: 0.0", "turn.dead_zone");
        refused(
            "throttle_arc_deg: 100.0",
            "throttle_arc_deg: 190.0",
            "throttle_arc_deg",
        );
        refused("feet_m: 0.85", "feet_m: 0.0", "call.feet_m");
    }
}
