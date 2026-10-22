//! Settlements: where the world makes room for a city, as
//! `settlements/*.settlement.ron` files spell it.
//!
//! A settlement is data so the next one is a file, not code. The author puts
//! it somewhere and says which way it faces; the settlement brings its own
//! landform (the hillside, the bay, the spurs), which the world blends in
//! over whatever its noise made there, and then terraces. Inside the city the
//! ground is therefore a function of this file alone, so the world's
//! generator can change without moving a building. The content step's baked
//! layout (which buildings stand where) is named here too, so a game and a
//! server find the same one.
//!
//! Lengths inside the city are in the **settlement frame**: its origin at the
//! chosen site, `+z` pointing downhill towards the water, `+x` along the
//! shore, `y` up.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

/// One settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementDef {
    /// The namespaced id, `strata:port_serein`.
    pub id: Namespaced,
    /// What a map calls it.
    pub name: String,
    /// The frame's origin, world metres `(x, z)`.
    pub at: [f64; 2],
    /// The world bearing of the frame's `+z`, downhill towards the water,
    /// degrees from the world's `+z` towards its `+x`.
    pub downhill_deg: f64,
    /// The city's extent in its frame, where its ground is shaped.
    pub extent: ExtentDef,
    /// The ground the city stands on, before terracing.
    pub landform: LandformDef,
    /// The hillside's terraces.
    pub terraces: TerracesDef,
    /// Level platforms, cut and filled out of the terraces.
    pub platforms: Vec<PlatformDef>,
    /// The road out into the wilderness.
    pub road: RoadDef,
    /// The content step's layout of its buildings, relative to `generated/`.
    pub layout: String,
    /// The terrain materials the city's ground is laid in; without it, the
    /// wild gravel of a road.
    #[serde(default)]
    pub ground: Option<GroundDef>,
}

/// What a city lays its ground in, by kind of ground: terrain material ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundDef {
    /// The terraces' level ground: the lanes and the lots.
    pub lanes: Namespaced,
    /// The quay and the platforms: the city's aprons.
    pub aprons: Namespaced,
    /// Slopes: the banks between levels and the verges.
    pub banks: Namespaced,
}

/// The rounded rectangle, in the settlement frame, whose ground the city
/// shapes. Beyond it the ground blends back to nature over `falloff_m`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtentDef {
    /// Metres either side of the origin along the shore.
    pub half_width_m: f64,
    /// Metres uphill of the origin (towards `-z`).
    pub uphill_m: f64,
    /// Metres downhill of the origin (towards `+z`), to past the waterline.
    pub downhill_m: f64,
    /// The blend back to natural ground.
    pub falloff_m: f64,
}

/// A settlement's own landform, in its frame: a hillside profile down the
/// slope, bent into a bay, with spurs and gullies and seeded relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandformDef {
    /// `(z, height)` knots down the slope, `z` increasing: the ridge first,
    /// the seabed last. Heights between knots are interpolated linearly;
    /// past the ends they hold.
    pub profile: Vec<[f64; 2]>,
    /// How much higher the ground stands at the extent's sides than on its
    /// centre line, metres: the bay the waterfront curves round.
    pub bay_m: f64,
    /// Ridges and hollows running down the slope, added along the frame's
    /// `x`.
    pub spurs: Vec<SpurDef>,
    /// The seeded relief's amplitude, metres.
    pub relief_m: f64,
    /// The relief's wavelength across the slope, metres; down the slope it is
    /// stretched, the way water carves a hillside.
    pub relief_wavelength_m: f64,
    /// The relief's seed: the settlement's, not the world's.
    pub seed: u64,
}

/// A spur (a rise) or a gully (a hollow) running down the whole slope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpurDef {
    /// Its centre line's `x` in the frame.
    pub x_m: f64,
    /// Half its width, metres.
    pub half_width_m: f64,
    /// How much it raises the ground on its centre line, metres; negative
    /// for a gully.
    pub rise_m: f64,
}

/// Terraces: the natural height quantised into level steps, each level
/// running along its own contour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerracesDef {
    /// The quay's height, the lowest level, metres above the sea. Ground
    /// between the waterline and the first terrace lies at it.
    pub quay_m: f64,
    /// The mean rise from one level to the next, metres.
    pub step_m: f64,
    /// How far each rise may differ from `step_m`, as a share of it, `0..1`:
    /// uneven steps keep the hillside from reading as a staircase.
    pub step_jitter: f64,
    /// The share of each step's natural rise taken by its wall, `0..1`:
    /// the smaller, the steeper and narrower the walls.
    pub wall: f64,
}

/// A level platform in the settlement frame: a rounded rectangle at its own
/// `level_m`, or at the mean height of the landform under its footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformDef {
    /// What stands on it, `hub` or `processor`.
    pub id: String,
    /// Its centre in the settlement frame, `(x, z)`.
    pub at: [f64; 2],
    /// Half its size along the frame's `x` and `z`, metres.
    pub half_size: [f64; 2],
    /// The rounding of its corners, metres.
    pub corner_m: f64,
    /// The blend from its edge back to the ground around it.
    pub falloff_m: f64,
    /// Its level, metres, where the design fixes it.
    #[serde(default)]
    pub level_m: Option<f64>,
}

/// A settlement's road out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadDef {
    /// Where it leaves the city, in the settlement frame, metres.
    pub exit: [f64; 2],
    /// How far it runs, metres.
    pub length_m: f64,
    /// Its carriageway's width, metres.
    pub width_m: f64,
}

impl SettlementDef {
    /// Why the numbers are unusable, or `None` when they are fine.
    #[must_use]
    pub fn invalid_reason(&self) -> Option<&'static str> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let e = &self.extent;
        if !(self.at.iter().all(|v| v.is_finite())
            && self.downhill_deg.is_finite()
            && positive(e.half_width_m)
            && positive(e.uphill_m)
            && positive(e.downhill_m)
            && positive(e.falloff_m))
        {
            return Some(
                "near, search_m, centre_height_m and the extent must be finite, sizes positive",
            );
        }
        let t = &self.terraces;
        if !(t.quay_m.is_finite()
            && positive(t.step_m)
            && (0.0..1.0).contains(&t.step_jitter)
            && t.wall > 0.0
            && t.wall < 1.0)
        {
            return Some(
                "terraces need a finite quay, a positive step, a jitter in 0..1 and a wall in 0..1",
            );
        }
        if self.platforms.iter().any(|p| {
            !(p.at.iter().all(|v| v.is_finite())
                && p.half_size.iter().all(|&v| positive(v))
                && p.corner_m.is_finite()
                && p.corner_m >= 0.0
                && p.corner_m <= p.half_size[0].min(p.half_size[1])
                && positive(p.falloff_m)
                && p.level_m.is_none_or(f64::is_finite))
        }) {
            return Some(
                "platforms need a finite centre, positive half sizes, a corner within them and a falloff",
            );
        }
        if !(self.road.exit.iter().all(|v| v.is_finite())
            && positive(self.road.length_m)
            && positive(self.road.width_m))
        {
            return Some("the road needs a finite exit and a positive length and width");
        }
        None
    }
}
