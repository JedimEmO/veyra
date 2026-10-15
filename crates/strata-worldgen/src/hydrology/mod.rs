//! The river network: one bake per land mass, below the render layer
//! (ADR 0010, ticket 0005 step 1), and the carve that
//! [`WorldSource::sample`](crate::WorldSource::sample) ends with
//! (ticket 0005 step 2, the `carve` module).
//!
//! A river is where a whole catchment sends its water, so it depends on
//! ground arbitrarily far away and no function of the point can know it.
//! Every land mass is bounded by the sea, though, so routing water over a
//! whole mass is exact, and on a coarse lattice it is cheap.
//!
//! - **The field.** A lattice of [`LATTICE_M`] over the ground before its
//!   relief, `ProceduralWorld::coarse_ground`: the continent's base and the
//!   range's own metres, and each biome's relief octaves of three lattice
//!   spacings or more (the first always, so the mountains keep their 6 and
//!   3 km octaves and a plain its first) by the blend; then the valleys the
//!   routing assumes ([`RiverParams::valley_range_share`]); the climate's
//!   moisture for the rain; and the dominant province. The relief's finer
//!   octaves are left out because a 512 m lattice aliases them into pits;
//!   the provinces' shapes, the beds and the erosion filter because they
//!   make the form of a slope, not where its water goes, and are most of a
//!   sample's cost. A land node costs a third of a sample (866 against
//!   2 541 ns, measured on one scatter), most of it the rain's march, which
//!   discharge needs anyway. The rain is the moisture (the march with the
//!   share of noise the biomes see) and not the bare march, which credits
//!   a range's rain to its windward foot: every crest and lee read nought
//!   and the ranges shed no rivers at all.
//! - **The mass.** Land nodes, 8-connected (the routing's own
//!   neighbourhood, so water never leaks across a diagonal into another
//!   mass), found by flood fill from any of them and named by the lowest
//!   node coordinate. The field is computed in tiles of [`TILE_NODES`]
//!   squared, on at most [`MAX_THREADS`] threads.
//! - **The cap.** A mass of more than [`MAX_MASS_NODES`] nodes (about
//!   275 000 km2; the largest met on seed 42 within 3 000 km of the origin
//!   is 602 169 nodes, 158 000 km2 and a thousand across, baked whole in
//!   0.77 to 0.81 s on 16 threads) is not baked whole: it is cut into its parts
//!   inside cells of [`CAP_CELL_NODES`] squared (256 km), each a mass of
//!   its own whose cell border is a forced divide. Water near a divide runs
//!   along it to the part's own coast, so a catchment that crosses a cell
//!   border is cut in two; only a part with no coast at all drains out over
//!   its cut edge. Cutting the 92 000 km2 mass under Port Serein into 64 km
//!   cells left 81 % of its river length as it was (within 50 m and 10 % of
//!   discharge), into 128 km cells 88 %. Whether a mass is over the cap is
//!   decided by counting it, which is the same from any of its nodes, so
//!   the parts are the same whoever asks.
//! - **The routing.** A priority flood from the coast, keyed on the level
//!   and then the node coordinate, so two machines agree; a pit deeper than
//!   its province's `keep_pits_m` stays a lake at its spill level, a
//!   shallower one is breached (a second flood lowers the path out of it to
//!   the pit's floor, Lindsay's breaching in priority-flood form). A lake
//!   drains through one outlet, its lowest. Rain, less the province's
//!   `sink`, is summed downstream.
//! - **The rivers.** Chains of nodes with at least
//!   [`RiverParams::river_discharge_m3s`], cut at confluences into reaches
//!   with a Strahler order. A reach's nodes are jittered off the lattice,
//!   smoothed by corner cutting and swayed sideways by a noise, all with
//!   multiplies and adds (the crate's float rule), so a river runs neither
//!   at 0 nor at 45 degrees; the ends of a reach stay where they are so the
//!   network stays joined. Width and depth are downstream hydraulic
//!   geometry, `Q^0.5` and `Q^0.375` by square roots. The water surface is
//!   the breached ground's, which falls monotonically downstream, and it is
//!   set from the mouth upstream once more after smoothing; a step steeper
//!   than [`RiverParams::fall_gradient`] becomes falls along it, as many
//!   equal ones as keep each under [`RiverParams::fall_max_m`], and the
//!   stretch below a fall's lip runs at its foot's level: a gorge for the
//!   carve to cut. A mouth's last step is never a fall: its drop is the
//!   coarse coast's, read across the sea bed's slope, and the carve grades
//!   it to the sample's shore.
//! - **Settlements.** Nodes within [`RiverParams::site_margin_m`] of an
//!   authored settlement's extent are not routed through, carry no water
//!   and leave the jitter and the sway off near it, so no river crosses the
//!   ground a layout is fitted to (ADR 0009).
//! - **The shape** (ticket 0005, step 3b, the `shape` module). Each reach is
//!   settled sideways onto the sample's own lowest ground, its tributaries
//!   bent to join it pointing downstream, and given meanders: an amplitude
//!   and a phase per vertex, which the carve turns into a channel winding
//!   about the line every ten to fourteen of its widths, wider and
//!   shallower between its bends and narrower and deeper in them.
//! - **The surfaces.** Once routed, each reach's surface is set onto the
//!   sample's own ground along it, less an incision, and breached, so a
//!   river never stands over the ground it runs on; each mouth is graded
//!   to the sample's own shore (`rivers::fit_surfaces`,
//!   `rivers::grade_mouths`).
//! - **The sites** (ticket 0005, step 4; ADR 0012, the `place` module).
//!   Last, each mass's procedural sites: its site kinds placed in rank order
//!   on the ground its own network carves, held by the network, folded into
//!   its content hash, and indexed per lattice cell, so a sample reads a
//!   pad from the cell it reads for the carve.
//! - **The roads** (ticket 0005, step 5; ADR 0012, decision 7, the
//!   `roads` module). After the sites, the roads between them: a backbone
//!   of service roads between towns and the settlements' road heads, tracks
//!   and trails joining the rest to it, each a least-cost route over the
//!   carved ground smoothed into a curve, with its crossings; folded into
//!   the content hash and indexed for the nearest road to a point
//!   ([`Network::nearest_road`]).
//! - **The grade** (ticket 0005, step 6, the `grade` module). Last, every
//!   service road's and track's profile, held to its class's gradient, its
//!   pads, its junctions and its crossings' decks and kept over the water
//!   beside it; a sampled cell gathers the roads whose ground reaches it
//!   into buckets, and a sample by one takes its carriageway, its verge and
//!   its cut and fill banks after the carve and before a site's pad.
//! - **The query.** A bucket grid over the reaches' segments answers
//!   [`Network::water_at`] with the [`Water`] record ADR 0010 names;
//!   [`Hydrology::water_at`] reads the masses whose rivers can reach a
//!   point first and bakes any that no one has asked about, on its own
//!   thread. The sampler reads the same grid per lattice cell, a thread
//!   keeping the last cell it read, and blends the segments round the point
//!   for the carve.
//! - **The version.** [`RiverParams`], the constants a bake depends on and
//!   three small canary networks ([`CANARY_MASS`], [`CANARY_CUT`],
//!   [`CANARY_WINDOW`]) join [`world_version`](crate::world_version); a
//!   change here that moves rivers where no canary stands is a
//!   [`GENERATOR_VERSION`](crate::GENERATOR_VERSION) bump by hand.
//!
//! Every step is a pure function of the mass's nodes and the seed: nothing
//! iterates a hash map into an output, the field is per node so the thread
//! count does not matter, and ties go to the lower node coordinate.

pub(crate) mod carve;
pub(crate) mod dual;
mod field;
pub(crate) mod grade;
mod index;
mod mass;
mod place;
mod rivers;
mod roads;
mod route;
mod shape;

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use strata_core::platform::time::Instant;

use serde::{Deserialize, Serialize};
use strata_core::Seed;

use crate::ProceduralWorld;
pub use crate::measure::MAX_THREADS;
pub use carve::Section;
use field::Tile;
use index::{Run, SegmentIndex, Shore};

/// Metres between the network's lattice nodes.
pub const LATTICE_M: f64 = 512.0;

/// Nodes a side of a tile of the coarse field: 64 km, about 10 ms of one
/// core, so a small island does not pay for the ocean round it.
pub const TILE_NODES: i32 = 128;

/// Base metres below sea level under which a node is sea by definition, with
/// no rain or relief computed: no biome's first octave lifts the sea floor
/// this far.
pub const DEEP_SEA_M: f64 = 150.0;

/// The most nodes a land mass is baked whole with: about 275 000 km2, over
/// the largest mass met near seed 42's origin, so the cut is for the tail.
/// Its bake is the bound: about a second and a half on 16 threads.
pub const MAX_MASS_NODES: usize = 1 << 20;

/// Nodes a side of the cells a mass over the cap is cut into: 256 km.
pub const CAP_CELL_NODES: i32 = 512;

/// The shortest relief octave the field keeps, in lattice spacings: a
/// shorter one aliases into pits at [`LATTICE_M`]. A biome's first octave
/// is kept whatever its length.
pub const RELIEF_SPACINGS: f64 = 3.0;

/// Octaves of the valleys' and the sway's fbm.
pub const NOISE_OCTAVES: usize = 2;

/// Each octave's amplitude over the one before, the valleys' and the sway's.
pub const NOISE_GAIN: f64 = 0.5;

/// Each octave's frequency over the one before, the valleys' and the sway's.
pub const NOISE_LACUNARITY: f64 = 2.0;

/// The constants above that change what a bake makes, hashed into the
/// world's version beside [`RiverParams`]: a change to any of them moves
/// rivers somewhere, and the canaries may not stand where it does (the cap
/// bites only on masses far larger than any canary). A new constant that a
/// network depends on belongs here too.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
struct Fixed {
    lattice_m: f64,
    deep_sea_m: f64,
    max_mass_nodes: u64,
    cap_cell_nodes: i32,
    relief_spacings: f64,
    noise_octaves: u64,
    noise_gain: f64,
    noise_lacunarity: f64,
    seconds_per_year: f64,
}

/// What [`Fixed`] holds in this build.
const FIXED: Fixed = Fixed {
    lattice_m: LATTICE_M,
    deep_sea_m: DEEP_SEA_M,
    max_mass_nodes: MAX_MASS_NODES as u64,
    cap_cell_nodes: CAP_CELL_NODES,
    relief_spacings: RELIEF_SPACINGS,
    noise_octaves: NOISE_OCTAVES as u64,
    noise_gain: NOISE_GAIN,
    noise_lacunarity: NOISE_LACUNARITY,
    seconds_per_year: route::SECONDS_PER_YEAR,
};

/// The numbers of the bake. Hashed into the world's version whole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RiverParams {
    /// Runoff in metres a year where the rainfall is one; discharge is the
    /// catchment's rain times this, less what sinks.
    pub runoff_m_per_year: f64,
    /// The least discharge that is a river, m3/s.
    pub river_discharge_m3s: f64,
    /// Width per square root of discharge, `w = a Q^0.5`.
    pub width_coefficient: f64,
    /// Depth per discharge to the 0.375, `d = c Q^0.375`; the speed is
    /// `Q / (w d)`, `Q^0.125 / (a c)`, so the three exponents sum to one.
    pub depth_coefficient: f64,
    /// Multiplies each province's `keep_pits_m`.
    pub lake_depth_scale: f64,
    /// `keep_pits_m` in a world without geology.
    pub keep_pits_m: f64,
    /// A node step whose drop over its run is steeper than this falls.
    pub fall_gradient: f64,
    /// And drops at least this, metres.
    pub fall_drop_m: f64,
    /// The tallest one fall, metres: a step that drops more is a cascade
    /// of equal falls along it. The lattice's steepest steps drop 500 m in
    /// 512, which is a torrent down a gorge, not one sheet.
    pub fall_max_m: f64,
    /// How far a node is jittered off the lattice, a share of the spacing
    /// either way.
    pub jitter: f64,
    /// Corner-cutting passes over each reach.
    pub smoothing: u32,
    /// The sway's greatest sideways offset, metres.
    pub meander_m: f64,
    /// The sway's wavelength along the ground, metres.
    pub meander_wavelength_m: f64,
    /// Over how much of each end of a reach the sway fades in, metres.
    pub meander_taper_m: f64,
    /// Nodes this close to a settlement's extent carry no water, metres.
    pub site_margin_m: f64,
    /// Beyond the margin the jitter and the sway fade in over this.
    pub site_fade_m: f64,
    /// The query's bucket side, metres.
    pub bucket_m: f64,
    /// How far from a centreline the query still answers, metres.
    pub reach_m: f64,
    /// The valleys the routing assumes, metres either way on any land.
    pub valley_m: f64,
    /// And their share of a range's own metres on top: a range's flank is
    /// a smooth bell in the field (its crags are finer than the lattice and
    /// the foothills' relief lies down up it), and water on a smooth ramp
    /// runs down it in parallel threads and gathers into nothing. The carve
    /// cuts the valleys this noise puts there.
    pub valley_range_share: f64,
    /// Their wavelength, metres.
    pub valley_wavelength_m: f64,
    /// How far a river's bank stands over its surface, metres: the ground
    /// beside the channel is never lower, so the water stays in it.
    pub freeboard_m: f64,
    /// Metres of a bank's run per metre it rises from the bed to the
    /// freeboard: 2.5 keeps the steepest of its eased slope near 0.6, which
    /// a person walks down.
    pub bank_run: f64,
    /// A valley's half-width where a river starts, before its section's
    /// share, metres.
    pub valley_base_m: f64,
    /// And what it gains per square root of discharge.
    pub valley_per_root_m: f64,
    /// The widest half-width a valley takes, metres: under
    /// [`reach_m`](Self::reach_m) less [`carve_fade_m`](Self::carve_fade_m),
    /// so the query always finds the river a valley belongs to.
    pub valley_max_m: f64,
    /// Over this last stretch of the query's reach the carve fades to
    /// nothing whatever the valley says, metres.
    pub carve_fade_m: f64,
    /// How far a mouth's surface is graded down to the sample's own shore,
    /// metres along the river.
    pub mouth_grade_m: f64,
    /// How far past a mouth the sample's shore is looked for, metres: the
    /// lattice's coast is up to a node off the real one.
    pub mouth_search_m: f64,
    /// How far a river's surface lies under the sample's ground along it,
    /// metres, before the freeboard: its valley's depth where nothing
    /// rises in its way. A range's valleys are cut deeper by
    /// [`valley_range_share`](Self::valley_range_share) of its own metres,
    /// as deep as the routing assumed them, as the discharge grows from 0.1
    /// to 2 m3/s.
    pub incision_m: f64,
    /// And what that gains per square root of discharge, so a river cuts
    /// deeper downstream.
    pub incision_per_root_m: f64,
    /// Over how much of a reach from its source the incision grows in,
    /// metres.
    pub source_taper_m: f64,
    /// The longest segment a reach keeps once it is shaped, metres: the
    /// corner cutting leaves up to 90 m, and the ground a reach settles
    /// onto turns at a finer grain than that.
    pub resample_m: f64,
    /// How far either way a reach looks for lower ground to settle onto,
    /// metres (ticket 0005, step 3b): the lattice routes a river over the
    /// coarse field, which leaves out the provinces' shapes, and a creek
    /// routed across a spur cut a slot hundreds of metres deep.
    pub settle_search_m: f64,
    /// How much lower ground a metre of settling sideways has to find,
    /// metres a metre: on a flat floor a reach stays where it is.
    pub settle_slack: f64,
    /// A meander's wavelength in channel widths: rivers bend every ten to
    /// fourteen widths.
    pub bend_widths: f64,
    /// A meander's greatest amplitude in channel widths, on a flat, gentle
    /// floor: its bends stay about one and a half widths in radius.
    pub bend_amplitude_widths: f64,
    /// And the most in metres whatever the width.
    pub bend_most_m: f64,
    /// How much wider (and shallower) a river runs where it crosses between
    /// two bends than its mean, and narrower (and deeper) in a bend's pool,
    /// as a share of its width at a full meander.
    pub riffle_share: f64,
    /// A tributary bends into its main stem at no more than the angle of
    /// this tangent off the main stem's upstream line, over this many
    /// metres per metre of the main stem's width and at least 80 m: a
    /// tributary joins pointing downstream, not square on.
    pub confluence_tan: f64,
}

impl RiverParams {
    /// The shipped numbers.
    pub const DEFAULT: Self = Self {
        runoff_m_per_year: 2.0,
        river_discharge_m3s: 0.1,
        width_coefficient: 4.5,
        depth_coefficient: 0.4,
        lake_depth_scale: 1.0,
        keep_pits_m: 10.0,
        fall_gradient: 0.3,
        fall_drop_m: 20.0,
        fall_max_m: 120.0,
        jitter: 0.3,
        smoothing: 3,
        meander_m: 160.0,
        meander_wavelength_m: 1_400.0,
        meander_taper_m: 300.0,
        site_margin_m: 450.0,
        site_fade_m: 700.0,
        bucket_m: 512.0,
        reach_m: 600.0,
        valley_m: 6.0,
        valley_range_share: 0.12,
        valley_wavelength_m: 3_000.0,
        freeboard_m: 0.3,
        bank_run: 2.5,
        valley_base_m: 60.0,
        valley_per_root_m: 110.0,
        valley_max_m: 440.0,
        carve_fade_m: 150.0,
        mouth_grade_m: 400.0,
        mouth_search_m: 600.0,
        incision_m: 3.0,
        incision_per_root_m: 8.0,
        source_taper_m: 500.0,
        resample_m: 100.0,
        settle_search_m: 160.0,
        settle_slack: 0.03,
        bend_widths: 12.0,
        bend_amplitude_widths: 4.2,
        bend_most_m: 30.0,
        riffle_share: 0.3,
        confluence_tan: 0.84,
    };

    /// Channel width for a discharge, metres.
    #[must_use]
    pub fn width(&self, discharge: f64) -> f64 {
        self.width_coefficient * discharge.max(0.0).sqrt()
    }

    /// Centreline depth for a discharge, metres: `Q^0.375` is the eighth
    /// root of `Q^3`, three square roots.
    #[must_use]
    pub fn depth(&self, discharge: f64) -> f64 {
        let q = discharge.max(0.0);
        self.depth_coefficient * (q * q * q).sqrt().sqrt().sqrt()
    }

    /// How far from its centreline a channel `width` metres wide and
    /// `depth` deep tops its banks, metres: the bank rises its depth and
    /// the freeboard over [`bank_run`](Self::bank_run) metres a metre, at
    /// least two, and eases over four tenths of that past the half-width,
    /// both "at least"s softened over a quarter metre so a riffle swinging
    /// them does not crease the bank.
    /// The carve's channel (`carve::Channel::of`, which carries gradients)
    /// and the water a body wades in agree through this, and a test holds
    /// the two to it.
    #[must_use]
    pub fn bank_top_m(&self, width: f64, depth: f64) -> f64 {
        let run = soft(
            (depth + self.freeboard_m) * self.bank_run - 2.0,
            BANK_SOFT_M,
        ) + 2.0;
        soft(width * 0.5 - run * 0.6, BANK_SOFT_M) + run
    }

    /// The greatest meander amplitude a river of this discharge takes,
    /// metres: what [`RiverVertex::meander_m`] is a share of.
    #[must_use]
    pub fn bend_amplitude(&self, discharge: f64) -> f64 {
        (self.bend_amplitude_widths * self.width(discharge)).min(self.bend_most_m)
    }

    /// Mean speed for a discharge, m/s.
    #[must_use]
    pub fn speed(&self, discharge: f64) -> f64 {
        discharge.max(0.0).sqrt().sqrt().sqrt() / (self.width_coefficient * self.depth_coefficient)
    }
}

impl Default for RiverParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The softness of the bank's two larger-ofs ([`RiverParams::bank_top_m`]),
/// metres.
const BANK_SOFT_M: f64 = 0.25;

/// The soft ramp of softness `k` (`dual::soft` without gradients): nought
/// below `-k`, `x` above `k`, a parabola between.
fn soft(x: f64, k: f64) -> f64 {
    if x <= -k {
        0.0
    } else if x >= k {
        x
    } else {
        (x + k) * (x + k) / (4.0 * k)
    }
}

/// What the coarse field says at one lattice node.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CoarseGround {
    /// Metres; land at zero and above.
    pub height: f64,
    /// The range's own metres in it ([`crate::Continent::range_m`]).
    pub range_m: f64,
    /// Rainfall, `0..=1`; zero on the deep sea.
    pub rain: f64,
    /// The dominant province's `(keep_pits_m, sink, along_grain)`, `None`
    /// without geology or on the deep sea.
    pub province: Option<(f64, f64, f64)>,
}

/// What kind of water a point is in or beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaterKind {
    /// A river's channel.
    Channel,
    /// A lake.
    Lake,
    /// A river's fall: the stretch where its surface steps down.
    Fall,
    /// A river's last stretch into the sea.
    Mouth,
}

/// The water at a point (ADR 0010, decision 3): what the carve, the water
/// mesh and the character's water read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Water {
    /// The water's surface, metres.
    pub surface_m: f32,
    /// The bed under the centreline (a lake's coarse ground at the point).
    pub bed_m: f32,
    /// Centreline depth, metres (a lake's depth at the point).
    pub depth_m: f32,
    /// Channel width, metres (a lake's: the side of a square of its area).
    pub width_m: f32,
    /// Metres from the centreline, positive on the side `(-flow[1],
    /// flow[0])` points to; the point is in the channel where its size is
    /// under half the width. For a lake, metres from the shore, negative
    /// inside.
    pub signed_distance_m: f32,
    /// Downstream, a unit `(x, z)`; zero on a lake.
    pub flow_dir: [f32; 2],
    /// Mean speed, m/s; zero on a lake.
    pub speed_mps: f32,
    /// Discharge, m3/s (a lake's: what leaves it).
    pub discharge_m3s: f32,
    /// Strahler order; zero on a lake.
    pub order: u8,
    /// What it is.
    pub kind: WaterKind,
}

/// Where a reach ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RiverEnd {
    /// In the sea: its last vertex where the coarse ground crosses sea
    /// level between its last land node and the sea node beyond, at least a
    /// quarter of the way.
    Sea,
    /// Over a bake window's or a cut mass's edge.
    Edge,
    /// In ground walled in by a settlement's, with nowhere lower to go.
    Sink,
    /// In the lake of this index.
    Lake(u32),
    /// At a confluence, where the reach of this index starts.
    Joins(u32),
}

/// One vertex of a reach.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RiverVertex {
    /// World metres.
    pub x: f64,
    /// World metres.
    pub z: f64,
    /// The water's surface, never higher than the vertex before.
    pub surface_m: f32,
    /// m3/s, never less than the vertex before.
    pub discharge_m3s: f32,
    /// Channel width, metres.
    pub width_m: f32,
    /// Centreline depth, metres.
    pub depth_m: f32,
    /// How far the sample's natural ground at the vertex stands over the
    /// surface, metres: how deep the river's valley is cut there.
    pub cut_m: f32,
    /// Whether the stretch to the next vertex is a fall.
    pub fall: bool,
    /// How far the channel swings either side of this line in its
    /// meanders, metres (ticket 0005, step 3b): the line is the valley's,
    /// settled onto its floor, and the channel winds about it at the
    /// [`phase`](Self::phase), every ten to fourteen of its widths, which
    /// no vertex spacing a whole network can afford would hold. Nought on
    /// falls, steep or confined ground and at a reach's ends.
    pub meander_m: f32,
    /// Where in its meanders the channel is at this vertex, in wavelengths
    /// from an origin the reach's head sets: at whole numbers it crosses
    /// the line towards the side a positive signed distance lies on
    /// ([`Water::signed_distance_m`]), a quarter past them it is at the
    /// apex of its bend that way.
    pub phase: f32,
}

/// A reach: a river from a source or a confluence to the next confluence,
/// a lake or the sea.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct River {
    /// Downstream order.
    pub vertices: Vec<RiverVertex>,
    /// Strahler order.
    pub order: u8,
    /// Where it ends.
    pub end: RiverEnd,
    /// The lake it leaves, for the reach that drains one.
    pub from_lake: Option<u32>,
}

/// The deepest a river runs at its centreline where a walker fords it,
/// metres: thigh deep, under the wading depth past which the character
/// swims (`strata_sim`'s `wade_depth`, 0.85 m), with room for the flow.
/// A road or a trail looks for a ford in [`River::centreline`].
pub const FORD_DEPTH_M: f64 = 0.65;

/// A point on a channel's centreline as it winds about its reach's line
/// ([`River::centreline`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelPoint {
    /// World metres.
    pub x: f64,
    /// World metres.
    pub z: f64,
    /// Metres along the reach's line from its first vertex.
    pub along_m: f64,
    /// The water's surface, metres.
    pub surface_m: f64,
    /// The channel's width here: wider than the mean where it crosses
    /// between two bends, narrower in a bend.
    pub width_m: f64,
    /// The water's depth at the centreline here: shallow on a riffle
    /// between two bends, deep in a bend's pool. What a road looking for a
    /// ford reads.
    pub depth_m: f64,
}

impl River {
    /// The channel's centreline every `step` metres or so along the reach's
    /// line, with its width and depth at each point: the line displaced by
    /// its meanders as the carve displaces it, the frame turned smoothly
    /// through each vertex. Cheap (no sample), for a ford, a crossing, a
    /// map or a test.
    #[must_use]
    #[allow(
        clippy::many_single_char_names,
        reason = "a segment's ends, its index, the step and the share along it"
    )]
    pub fn centreline(&self, params: &RiverParams, step: f64) -> Vec<ChannelPoint> {
        let v = &self.vertices;
        let n = v.len();
        if n < 2 {
            return Vec::new();
        }
        // Each vertex's normal: its segments' averaged, so the frame turns
        // through a vertex rather than jumping at it.
        let unit = |a: &RiverVertex, b: &RiverVertex| {
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let length = (dx * dx + dz * dz).sqrt().max(f64::MIN_POSITIVE);
            [-dz / length, dx / length]
        };
        let normals: Vec<[f64; 2]> = (0..n)
            .map(|k| {
                let before = (k > 0).then(|| unit(&v[k - 1], &v[k]));
                let after = (k + 1 < n).then(|| unit(&v[k], &v[k + 1]));
                match (before, after) {
                    (Some(a), Some(b)) => {
                        let s = [a[0] + b[0], a[1] + b[1]];
                        let length = (s[0] * s[0] + s[1] * s[1]).sqrt();
                        if length > 1e-9 {
                            [s[0] / length, s[1] / length]
                        } else {
                            b
                        }
                    }
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => [0.0, 0.0],
                }
            })
            .collect();
        let mut out = Vec::new();
        let mut along = 0.0;
        for k in 0..n - 1 {
            let (a, b) = (v[k], v[k + 1]);
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let length = (dx * dx + dz * dz).sqrt();
            let steps = (length / step.max(0.01)).ceil().max(1.0);
            let last = k + 2 == n;
            let mut i = 0.0;
            while i < steps || (last && i <= steps) {
                let t = i / steps;
                let lerp = |p: f64, q: f64| p + (q - p) * t;
                let normal = [
                    lerp(normals[k][0], normals[k + 1][0]),
                    lerp(normals[k][1], normals[k + 1][1]),
                ];
                let q = lerp(f64::from(a.discharge_m3s), f64::from(b.discharge_m3s));
                let amplitude = lerp(f64::from(a.meander_m), f64::from(b.meander_m));
                let phase = lerp(f64::from(a.phase), f64::from(b.phase));
                let wave = shape::wave(phase);
                let share = shape::bend_share(params, amplitude, q);
                let riffle = shape::riffle(params, share, &wave);
                let offset = amplitude * wave.offset;
                out.push(ChannelPoint {
                    x: lerp(a.x, b.x) + normal[0] * offset,
                    z: lerp(a.z, b.z) + normal[1] * offset,
                    along_m: along + length * t,
                    surface_m: lerp(f64::from(a.surface_m), f64::from(b.surface_m)),
                    width_m: params.width(q) * riffle,
                    depth_m: params.depth(q) / riffle,
                });
                i += 1.0;
            }
            along += length;
        }
        out
    }

    /// [`centreline`](Self::centreline)'s points alone.
    #[must_use]
    pub fn channel(&self, step: f64) -> Vec<[f64; 2]> {
        self.centreline(&RiverParams::DEFAULT, step)
            .iter()
            .map(|p| [p.x, p.z])
            .collect()
    }
}

/// A lake: a pit deeper than its province keeps, filled to its spill.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lake {
    /// The water's level, metres.
    pub level_m: f32,
    /// Its deepest, over the coarse ground, metres.
    pub depth_m: f32,
    /// The lattice nodes under it, in order.
    pub nodes: Vec<[i32; 2]>,
    /// Where it drains, world `(x, z)`: its outlet node.
    pub outlet: [f64; 2],
    /// What leaves it, m3/s.
    pub discharge_m3s: f32,
    /// The mean of the natural ground's fine part (its height less its
    /// coarse part: the continent and the relief's first octaves) over the
    /// lake's nodes and the ring round them, metres: what the carve takes
    /// off the fine ground before it sets its shape on the coarse basin,
    /// so the shore follows the fine ground's bays about the coarse shore
    /// and not a few tens of metres off it (ticket 0005, step 3b).
    pub detail_m: f32,
}

/// A fall: where a river's surface steps down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fall {
    /// The lip, world `(x, surface, z)`.
    pub top: [f64; 3],
    /// The foot, world `(x, surface, z)`.
    pub bottom: [f64; 3],
    /// The river's width there, metres.
    pub width_m: f32,
    /// Downstream, a unit `(x, z)`.
    pub direction: [f32; 2],
    /// The reach it is on.
    pub river: u32,
}

/// How a bake went: not part of the network or its hash.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakeStats {
    /// Lattice nodes in the mass, settlement ground included.
    pub nodes: usize,
    /// Seconds from asking to the network, field tiles included.
    pub seconds: f64,
    /// Field tiles computed for it.
    pub tiles: usize,
    /// Segments the query holds.
    pub segments: usize,
    /// Segment entries over all its buckets: the index's memory, at four
    /// bytes each over the segments' own.
    pub bucket_entries: usize,
    /// Procedural sites placed on the mass (ADR 0012).
    pub sites: usize,
    /// Seconds of [`seconds`](Self::seconds) the sites' placement took.
    pub sites_seconds: f64,
    /// Roads laid between the sites (ticket 0005, step 5).
    pub roads: usize,
    /// Their length, kilometres.
    pub road_km: f64,
    /// Sites no road reached.
    pub unreached: usize,
    /// Seconds of [`seconds`](Self::seconds) the roads took.
    pub roads_seconds: f64,
    /// Seconds of [`roads_seconds`](Self::roads_seconds) the grade's
    /// profiles took (ticket 0005, step 6).
    pub grade_seconds: f64,
}

/// The network of one land mass.
#[derive(Debug)]
pub struct Network {
    key: [i32; 2],
    capped: bool,
    rivers: Vec<River>,
    lakes: Vec<Lake>,
    falls: Vec<Fall>,
    params: RiverParams,
    index: SegmentIndex,
    shore: Shore,
    hash: u64,
    stats: BakeStats,
    /// The procedural sites on the mass, in the order they were placed
    /// (ADR 0012).
    sites: Vec<crate::sites::ProceduralSite>,
    /// Per lattice cell, the sites whose ground reaches it.
    pads: NodeMap<Vec<u32>>,
    /// The roads between the sites (ticket 0005, step 5).
    roads: Vec<crate::roads::Road>,
    /// Where roads end on others.
    junctions: Vec<crate::roads::Junction>,
    /// The roads' segments in buckets.
    road_index: crate::roads::RoadIndex,
}

impl Network {
    /// The procedural sites on the mass, in the order they were placed:
    /// kinds in rank order, each kind's best first.
    #[must_use]
    pub fn sites(&self) -> &[crate::sites::ProceduralSite] {
        &self.sites
    }

    /// Sets the sites, indexes their pads by lattice cell and folds them
    /// into the content hash. A network with none hashes as it did before
    /// sites existed.
    fn set_sites(&mut self, sites: Vec<crate::sites::ProceduralSite>) {
        let mut pads: NodeMap<Vec<u32>> = NodeMap::default();
        for (k, site) in sites.iter().enumerate() {
            let outer = site.outer_m();
            for j in node_of(site.centre[1] - outer)..=node_of(site.centre[1] + outer) {
                for i in node_of(site.centre[0] - outer)..=node_of(site.centre[0] + outer) {
                    pads.entry([i, j]).or_default().push(local(k));
                }
            }
        }
        if !sites.is_empty() {
            let mut hash = Fnv(self.hash);
            for site in &sites {
                hash.bytes(b"site");
                hash.bytes(&site.id.kind.0.to_le_bytes());
                for v in site.id.mass.iter().chain(&site.id.node) {
                    hash.i32(*v);
                }
                hash.bytes(site.name.as_bytes());
                for v in site
                    .centre
                    .iter()
                    .chain(&site.facing)
                    .chain(&site.arrival)
                    .chain(&[site.level_m, site.radius_m, site.blend_m, site.crown])
                {
                    hash.f64(*v);
                }
            }
            self.hash = hash.0;
        }
        self.stats.sites = sites.len();
        self.sites = sites;
        self.pads = pads;
    }

    /// The roads between the mass's places (ticket 0005, step 5; ADR 0012,
    /// decision 7): service roads first, then tracks, then trails, each in
    /// the order it was laid. What a grade reads: each road's
    /// [`points`](crate::roads::Road::points) (its centreline with the
    /// ground) and [`crossings`](crate::roads::Road::crossings).
    #[must_use]
    pub fn roads(&self) -> &[crate::roads::Road] {
        &self.roads
    }

    /// Where roads end on other roads: a
    /// [`RoadEnd::Junction`](crate::roads::RoadEnd::Junction) indexes this.
    #[must_use]
    pub fn junctions(&self) -> &[crate::roads::Junction] {
        &self.junctions
    }

    /// The nearest point of any of the mass's roads within `reach_m` of
    /// `(x, z)`: its class, distance and bearing there. Reads the bucket
    /// grid, no sample.
    #[must_use]
    pub fn nearest_road(&self, x: f64, z: f64, reach_m: f64) -> Option<crate::roads::RoadHere> {
        self.road_index
            .nearest(&self.roads, self.key, x, z, reach_m)
    }

    /// Sets the roads, indexes them and folds them into the content hash.
    fn set_roads(&mut self, built: roads::Built, seconds: f64) {
        if !built.roads.is_empty() {
            let mut hash = Fnv(self.hash);
            for road in &built.roads {
                hash.bytes(b"road");
                hash.bytes(&[road.class as u8]);
                for end in &road.ends {
                    let (tag, value) = match *end {
                        crate::roads::RoadEnd::Site(id) => (0_u8, id.key()),
                        crate::roads::RoadEnd::Settlement(s) => (1, u64::from(s)),
                        crate::roads::RoadEnd::RoadHead(s) => (2, u64::from(s)),
                        crate::roads::RoadEnd::Junction(j) => (3, u64::from(j)),
                    };
                    hash.bytes(&[tag]);
                    hash.bytes(&value.to_le_bytes());
                }
                for p in &road.points {
                    hash.f64(p.x);
                    hash.f64(p.z);
                    hash.f64(p.along_m);
                    hash.f32(p.ground_m);
                }
                for [level, rise] in &road.profile {
                    hash.f32(*level);
                    hash.f32(*rise);
                }
                for c in &road.crossings {
                    hash.bytes(&[c.kind as u8]);
                    for v in c.at.iter().chain(&c.bearing).chain(&c.banks_m).chain(&[
                        c.along_m,
                        c.span_m,
                        c.deck_m,
                        c.surface_m,
                        c.width_m,
                        c.depth_m,
                        c.abutments_m[0],
                        c.abutments_m[1],
                    ]) {
                        hash.f64(*v);
                    }
                    hash.bytes(&c.river.to_le_bytes());
                }
            }
            for j in &built.junctions {
                hash.bytes(b"junction");
                hash.f64(j.at[0]);
                hash.f64(j.at[1]);
                hash.bytes(&j.road.to_le_bytes());
                hash.f64(j.along_m);
            }
            self.hash = hash.0;
        }
        self.stats.roads = built.roads.len();
        self.stats.road_km = built
            .roads
            .iter()
            .map(crate::roads::Road::length_m)
            .sum::<f64>()
            / 1e3;
        self.stats.unreached = built.unreached;
        self.stats.roads_seconds = seconds;
        self.stats.grade_seconds = built.grade_seconds;
        self.road_index = crate::roads::RoadIndex::new(&built.roads);
        self.roads = built.roads;
        self.junctions = built.junctions;
    }

    /// The mass's name: its lowest lattice node `(i, j)`, the node at
    /// `(i, j) * LATTICE_M`.
    #[must_use]
    pub const fn key(&self) -> [i32; 2] {
        self.key
    }

    /// Whether the mass was over the cap and this is one cell's part.
    #[must_use]
    pub const fn capped(&self) -> bool {
        self.capped
    }

    /// The reaches, in the order of their first node.
    #[must_use]
    pub fn rivers(&self) -> &[River] {
        &self.rivers
    }

    /// The lakes, in the order of their lowest node.
    #[must_use]
    pub fn lakes(&self) -> &[Lake] {
        &self.lakes
    }

    /// The falls, by reach and then downstream.
    #[must_use]
    pub fn falls(&self) -> &[Fall] {
        &self.falls
    }

    /// How the bake went.
    #[must_use]
    pub const fn stats(&self) -> BakeStats {
        self.stats
    }

    /// A hash of everything the network holds, as
    /// `strata_weather::BasinTile::content_hash` is of a tile: FNV-1a over
    /// the bits, so the same on every platform.
    #[must_use]
    pub const fn content_hash(&self) -> u64 {
        self.hash
    }

    /// The water at `(x, z)` from this network alone: the lake over the
    /// point, or the nearest reach within [`RiverParams::reach_m`].
    #[must_use]
    pub fn water_at(&self, x: f64, z: f64) -> Option<Water> {
        if let Some(lake) = self.shore.lake_at(&self.lakes, x, z) {
            return Some(lake);
        }
        self.index.nearest(&self.params, x, z)
    }
}

/// A hasher for lattice coordinates: a multiply and a fold. The maps it
/// keys are only looked up, never iterated into an output, so its order
/// cannot reach the network.
#[derive(Default, Clone, Copy)]
pub(crate) struct NodeHasher(u64);

impl std::hash::Hasher for NodeHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn write_i32(&mut self, i: i32) {
        self.write_u64(u64::from(i.cast_unsigned()));
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

/// A map keyed by lattice coordinates.
pub(crate) type NodeMap<V> = HashMap<[i32; 2], V, BuildHasherDefault<NodeHasher>>;

/// A label no mass has claimed yet.
const UNKNOWN: u32 = 0;
/// A node of a mass known to be over the cap whose part is not baked yet.
const LARGE: u32 = 1;
/// Labels from here on are `networks[label - FIRST]`.
const FIRST: u32 = 2;

/// The river network's cache: the coarse field's tiles, the networks baked
/// so far, which mass each node belongs to, and per lattice cell what the
/// sampler reads there. Shared by every clone of a world; a bake takes one
/// mutex so a mass is baked once.
///
/// A sample reads one cell: the bucket runs of every network
/// whose rivers can reach it and the networks whose lakes stand at its
/// corners. A thread keeps the last cell it read, so a grid inside one cell
/// takes no lock and touches no shared counter (ADR 0003); a cell is made
/// once, under a write lock, the first time anyone reads it.
pub struct Hydrology {
    /// Which cache this is, for the threads' last cell: two worlds in one
    /// process (the tests, the version's canary) never share one.
    id: u64,
    seed: Seed,
    params: RiverParams,
    /// The cap: [`MAX_MASS_NODES`] and [`CAP_CELL_NODES`] but in a test.
    cap: (usize, i32),
    valleys: crate::noise::Fractal,
    threads: AtomicUsize,
    tiles: RwLock<NodeMap<Arc<Tile>>>,
    networks: RwLock<Vec<Arc<Network>>>,
    bake: Mutex<()>,
    /// Held while a field tile is computed, so each is computed once.
    computing: Mutex<()>,
    /// What the sampler reads per lattice cell, made on first read.
    cells: RwLock<NodeMap<Arc<Cell>>>,
    /// The cell with nothing in it, shared by every dry cell.
    empty: Arc<Cell>,
    /// A cache that never bakes: the labels of the networks it was given,
    /// every other node dry. The version's canary samples through one.
    fixed: Option<NodeMap<u32>>,
    /// Whether a sample has had to bake a mass itself yet: said once.
    warned: AtomicBool,
}

impl std::fmt::Debug for Hydrology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hydrology")
            .field("seed", &self.seed)
            .field("params", &self.params)
            .field("networks", &self.networks().len())
            .finish_non_exhaustive()
    }
}

/// What a sample reads in one lattice cell (the cell is the query's bucket,
/// [`RiverParams::bucket_m`] being [`LATTICE_M`]).
#[derive(Debug, Default)]
pub(crate) struct Cell {
    /// Each network whose rivers come within reach of the cell, by key, and
    /// its bucket's run of segments.
    pub runs: Vec<(Arc<Network>, Run)>,
    /// Each network with a lake node at one of the cell's four corners.
    pub lakes: Vec<Arc<Network>>,
    /// The pads of the sites whose ground reaches the cell (ADR 0012).
    pub pads: Vec<crate::sites::Pad>,
    /// The roads whose grade reaches the cell (ticket 0005, step 6), by
    /// bucket.
    pub roads: Option<grade::RoadGrid>,
}

impl Cell {
    fn is_empty(&self) -> bool {
        self.runs.is_empty()
            && self.lakes.is_empty()
            && self.pads.is_empty()
            && self.roads.is_none()
    }

    /// What `network` alone carves at `(x, z)`, with no pad: what the bake
    /// of that network samples its ground through, the cell's run and lakes
    /// read in place.
    pub(crate) fn gather_one(
        network: &Network,
        params: &RiverParams,
        x: f64,
        z: f64,
    ) -> carve::Gathered {
        let key = [node_of(x), node_of(z)];
        let corners = [
            key,
            [key[0] + 1, key[1]],
            [key[0], key[1] + 1],
            [key[0] + 1, key[1] + 1],
        ];
        let run = network.index.run(key).map(|run| [(network, run)]);
        let lake = corners
            .iter()
            .any(|&n| network.shore.has_lake(n))
            .then_some([network]);
        carve::gather(
            run.as_ref().map_or(&[][..], |r| &r[..]),
            lake.as_ref().map_or(&[][..], |l| &l[..]),
            params,
            x,
            z,
        )
    }

    /// The pad whose ground reaches `(x, z)`, if any. Sites of one network
    /// keep their spacing, so no two pads' grounds meet; only across a
    /// capped mass's forced divide could two (`place`'s module doc), and the
    /// first is then taken.
    pub(crate) fn pad_at(&self, x: f64, z: f64) -> Option<crate::sites::Pad> {
        self.pads.iter().copied().find(|pad| {
            let (dx, dz) = (x - pad.centre[0], z - pad.centre[1]);
            let outer = pad.radius + pad.blend;
            dx * dx + dz * dz < outer * outer
        })
    }
}

thread_local! {
    /// The cell this thread read last: `(cache id, cell, the cell)`.
    static LAST_CELL: std::cell::RefCell<Option<(u64, [i32; 2], Arc<Cell>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Every cache gets its own id.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Lattice cells either side of a cell whose nodes' masses can reach it: a
/// river point lies within half a diagonal, the jitter's diagonal and the
/// sway of one of its nodes, or past its mass's last land node on a mouth
/// run on to the sample's shore ([`RiverParams::mouth_search_m`]), so a node
/// over [`RiverParams::reach_m`] plus that from every point of the cell
/// cannot carry a river into it; nor can a reach settled sideways onto its
/// valley's floor, a junction settled with it, or a tributary bent into its
/// main stem (ticket 0005, step 3b). Five cells for the shipped numbers
/// (600 m of reach and 1 779 of slack is 4.65 spacings).
fn cell_margin(params: &RiverParams) -> i32 {
    let slack = LATTICE_M * std::f64::consts::FRAC_1_SQRT_2
        + params.jitter * LATTICE_M * std::f64::consts::SQRT_2
        + params.meander_m
        // A run-on's last vertex is half its width and 8 m past the shore.
        + params.mouth_search_m
        + 32.0
        // The settling sideways, a junction's own settling and a
        // tributary's bend into its main stem
        // (`shape::JUNCTION_SEARCH_M`, `shape::CONFLUENCE_MOST_M`).
        + params.settle_search_m
        + shape::JUNCTION_SEARCH_M
        + shape::CONFLUENCE_MOST_M;
    #[allow(clippy::cast_possible_truncation, reason = "a few lattice spacings")]
    let cells = ((params.reach_m + slack) / LATTICE_M).ceil() as i32;
    cells.max(1)
}

impl Hydrology {
    /// An empty cache for a world of `seed`.
    #[must_use]
    pub fn new(seed: Seed) -> Self {
        Self::with_cap(seed, MAX_MASS_NODES, CAP_CELL_NODES)
    }

    /// An empty cache that cuts any mass over `nodes` into cells of `cell`
    /// nodes a side: the cap made small enough to bite on a test's ground.
    #[must_use]
    pub fn with_cap(seed: Seed, nodes: usize, cell: i32) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            seed,
            params: RiverParams::DEFAULT,
            cap: (nodes, cell.max(1)),
            valleys: crate::noise::Fractal::new(
                seed.derive("rivers/valleys"),
                crate::noise::Shape::Fbm,
                RiverParams::DEFAULT.valley_wavelength_m,
                NOISE_OCTAVES,
                NOISE_GAIN,
                NOISE_LACUNARITY,
            ),
            threads: AtomicUsize::new(strata_core::platform::par::threads()),
            tiles: RwLock::new(NodeMap::default()),
            networks: RwLock::new(Vec::new()),
            bake: Mutex::new(()),
            computing: Mutex::new(()),
            cells: RwLock::new(NodeMap::default()),
            empty: Arc::new(Cell::default()),
            fixed: None,
            warned: AtomicBool::new(false),
        }
    }

    /// An empty cache for the same seed, with the default cap.
    pub(crate) fn empty_like(&self) -> Self {
        Self::new(self.seed)
    }

    /// The cache every world with network key `key` in this process shares
    /// (`version::network_key`): the same inputs make the same networks, so
    /// a second world of them need not bake again. Kept for the process's
    /// life, as a world's own cache is for the world's.
    pub(crate) fn shared(seed: Seed, key: u64) -> Arc<Self> {
        static SHARED: Mutex<Vec<(u64, Arc<Hydrology>)>> = Mutex::new(Vec::new());
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, cache)) = shared.iter().find(|(held, _)| *held == key) {
            return Arc::clone(cache);
        }
        let cache = Arc::new(Self::new(seed));
        shared.push((key, Arc::clone(&cache)));
        cache
    }

    /// A cache holding the window of lattice nodes from `from` to `to`
    /// baked as [`bake_window`](Self::bake_window) bakes it, that never
    /// bakes anything else: every point whose cell no node of the window
    /// reaches is dry. What the world's version samples its canary points
    /// through, so a handshake bakes no mass.
    #[must_use]
    pub(crate) fn fixed_window(world: &ProceduralWorld, from: [i32; 2], to: [i32; 2]) -> Self {
        let mut fixed = Self::new(world.hydrology().seed);
        fixed.set_threads(1);
        let network = fixed.bake_window(world, from, to, 1);
        let mut labels = NodeMap::default();
        for i in from[0]..=to[0] {
            for j in from[1]..=to[1] {
                labels.insert([i, j], FIRST);
            }
        }
        fixed.networks = RwLock::new(vec![Arc::new(network)]);
        fixed.fixed = Some(labels);
        fixed
    }

    /// The bake's numbers.
    #[must_use]
    pub const fn params(&self) -> &RiverParams {
        &self.params
    }

    /// Threads the field is computed on by an explicit bake
    /// ([`bake_area`](Self::bake_area),
    /// [`ProceduralWorld::prepare_water`](crate::ProceduralWorld::prepare_water))
    /// from now on, at least one and at most [`MAX_THREADS`]. A bake a
    /// sample or a query sets off itself runs on its own thread alone. The
    /// networks are the same for any count.
    pub fn set_threads(&self, threads: usize) {
        self.threads
            .store(threads.clamp(1, MAX_THREADS), Ordering::Relaxed);
    }

    /// Every network baked so far, by key.
    #[must_use]
    pub fn networks(&self) -> Vec<Arc<Network>> {
        let mut all = self
            .networks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        all.sort_by_key(|network| network.key);
        all
    }

    /// The water at `(x, z)`: the lake the point is in, or else the nearest
    /// reach within reach, over every mass whose rivers can reach the
    /// point. Bakes a mass no one has asked about first, on this thread
    /// alone.
    pub fn water_at(&self, world: &ProceduralWorld, x: f64, z: f64) -> Option<Water> {
        self.with_cell(world, x, z, |cell| {
            let mut best: Option<Water> = None;
            let mut consider = |water: Water| {
                best = Some(match best {
                    Some(held) if rank(&held) <= rank(&water) => held,
                    _ => water,
                });
            };
            for network in &cell.lakes {
                if let Some(water) = network.shore.lake_at(&network.lakes, x, z) {
                    consider(water);
                }
            }
            for (network, run) in &cell.runs {
                if let Some(water) = network.index.nearest_in(*run, &network.params, x, z) {
                    consider(water);
                }
            }
            best
        })
    }

    /// `f` of the cell `(x, z)` is in, made (and any mass that reaches it
    /// baked, on this thread alone) if no one has read it.
    pub(crate) fn with_cell<R>(
        &self,
        world: &ProceduralWorld,
        x: f64,
        z: f64,
        f: impl FnOnce(&Cell) -> R,
    ) -> R {
        let key = [node_of(x), node_of(z)];
        LAST_CELL.with(|last| {
            let mut last = last.borrow_mut();
            match last.as_ref() {
                Some((id, at, cell)) if *id == self.id && *at == key => f(cell),
                _ => {
                    let cell = self.cell(world, key, 1);
                    let answer = f(&cell);
                    *last = Some((self.id, key, cell));
                    answer
                }
            }
        })
    }

    /// The cell `key`, made if no one has.
    fn cell(&self, world: &ProceduralWorld, key: [i32; 2], threads: usize) -> Arc<Cell> {
        if let Some(cell) = self
            .cells
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Arc::clone(cell);
        }
        let margin = cell_margin(&self.params);
        let mut labels = Vec::new();
        for j in key[1] - margin..=key[1] + 1 + margin {
            for i in key[0] - margin..=key[0] + 1 + margin {
                if let Some(label) = self.label_at(world, [i, j], threads) {
                    labels.push(label);
                }
            }
        }
        labels.sort_unstable();
        labels.dedup();
        let corners = [
            key,
            [key[0] + 1, key[1]],
            [key[0], key[1] + 1],
            [key[0] + 1, key[1] + 1],
        ];
        let mut cell = Cell::default();
        {
            let networks = self.networks.read().unwrap_or_else(PoisonError::into_inner);
            let mut near: Vec<&Arc<Network>> = labels
                .iter()
                .map(|&label| &networks[(label - FIRST) as usize])
                .collect();
            // By key, so every sum over them runs in one order.
            near.sort_by_key(|network| network.key);
            cell.roads = grade::RoadGrid::new(key, &near);
            for network in near {
                if let Some(run) = network.index.run(key) {
                    cell.runs.push((Arc::clone(network), run));
                }
                if corners.iter().any(|&n| network.shore.has_lake(n)) {
                    cell.lakes.push(Arc::clone(network));
                }
                if let Some(sites) = network.pads.get(&key) {
                    cell.pads
                        .extend(sites.iter().map(|&k| network.sites[k as usize].pad()));
                }
            }
        }
        let cell = if cell.is_empty() {
            Arc::clone(&self.empty)
        } else {
            Arc::new(cell)
        };
        let mut cells = self.cells.write().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(cells.entry(key).or_insert(cell))
    }

    /// The label of the mass land node `node` is on, baking it on
    /// `threads` if no one has; `None` on the sea, and off a fixed cache's
    /// window.
    fn label_at(&self, world: &ProceduralWorld, node: [i32; 2], threads: usize) -> Option<u32> {
        if let Some(fixed) = &self.fixed {
            return fixed.get(&node).copied();
        }
        let (key, k) = field::locate(node);
        let tile = self.tile(world, key, threads);
        if tile.height[k] < 0.0 {
            return None;
        }
        let label = tile.label[k].load(Ordering::Acquire);
        Some(if label < FIRST {
            self.bake_at(world, node, threads)
        } else {
            label
        })
    }

    /// The network of the mass the land node nearest `(x, z)` is on, baked
    /// (on this thread alone) if it was not; `None` over the sea.
    pub fn network_at(&self, world: &ProceduralWorld, x: f64, z: f64) -> Option<Arc<Network>> {
        let node = [nearest_node(x), nearest_node(z)];
        let label = self.label_at(world, node, 1)?;
        let networks = self.networks.read().unwrap_or_else(PoisonError::into_inner);
        Some(Arc::clone(&networks[(label - FIRST) as usize]))
    }

    /// Bakes every mass with a land node in the square from `(x0, z0)` to
    /// `(x1, z1)`, or whose rivers can reach it, on the threads
    /// [`set_threads`](Self::set_threads) gave, and returns them by key:
    /// what a map or a startup that knows its reach asks for, so no sample
    /// waits on a bake later.
    pub fn bake_area(
        &self,
        world: &ProceduralWorld,
        from: (f64, f64),
        to: (f64, f64),
    ) -> Vec<Arc<Network>> {
        self.bake_area_on(world, from, to, self.threads.load(Ordering::Relaxed))
    }

    /// [`bake_area`](Self::bake_area) on `threads` (at most
    /// [`MAX_THREADS`]) whatever [`set_threads`](Self::set_threads) said: a
    /// bake in the background while the game runs takes a few.
    pub fn bake_area_on(
        &self,
        world: &ProceduralWorld,
        (x0, z0): (f64, f64),
        (x1, z1): (f64, f64),
        threads: usize,
    ) -> Vec<Arc<Network>> {
        let threads = threads.clamp(1, MAX_THREADS);
        let margin = cell_margin(&self.params);
        let mut labels = Vec::new();
        for j in node_of(z0) - margin..=node_of(z1) + 1 + margin {
            for i in node_of(x0) - margin..=node_of(x1) + 1 + margin {
                if let Some(label) = self.label_at(world, [i, j], threads) {
                    labels.push(label);
                }
            }
        }
        labels.sort_unstable();
        labels.dedup();
        let networks = self.networks.read().unwrap_or_else(PoisonError::into_inner);
        let mut out: Vec<_> = labels
            .into_iter()
            .map(|label| Arc::clone(&networks[(label - FIRST) as usize]))
            .collect();
        out.sort_by_key(|network| network.key);
        out
    }

    /// The network of the land nodes in the window of lattice nodes from
    /// `from` to `to` inclusive, as if the window were a mass whose edge is
    /// a coast, on `threads`: what the world's version bakes as its canary,
    /// and what a test bakes when a whole mass would be too slow. Not
    /// cached.
    #[must_use]
    pub fn bake_window(
        &self,
        world: &ProceduralWorld,
        from: [i32; 2],
        to: [i32; 2],
        threads: usize,
    ) -> Network {
        let started = Instant::now();
        let (domain, tiles) = mass::window(self, world, from, to, threads);
        self.finish(Some(world), &domain, started, tiles, threads)
    }

    /// Bakes the mass of land node `node` on `threads` if no one has, and
    /// returns its label. Drops the rain of every tile whose land is all
    /// baked after it.
    fn bake_at(&self, world: &ProceduralWorld, node: [i32; 2], threads: usize) -> u32 {
        let _one = self.bake.lock().unwrap_or_else(PoisonError::into_inner);
        let (key, k) = field::locate(node);
        let label = self.tile(world, key, threads).label[k].load(Ordering::Acquire);
        if label >= FIRST {
            return label;
        }
        if threads == 1 && !self.warned.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                node = ?node,
                "a sample met a land mass no one had baked and bakes it on its own thread; \
                 prepare the water round a place before sampling it"
            );
        }
        let started = Instant::now();
        let (domain, tiles) = mass::find(self, world, node, label == LARGE, self.cap, threads);
        let network = self.finish(Some(world), &domain, started, tiles, threads);
        let mut networks = self
            .networks
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let label = FIRST + u32::try_from(networks.len()).expect("fewer than 4 billion masses");
        networks.push(Arc::new(network));
        drop(networks);
        let mut cached: NodeMap<Arc<Tile>> = NodeMap::default();
        for &node in &domain.nodes {
            let (key, k) = field::locate(node);
            cached
                .entry(key)
                .or_insert_with(|| self.tile(world, key, threads))
                .label[k]
                .store(label, Ordering::Release);
        }
        let mut keys: Vec<_> = cached.into_keys().collect();
        keys.sort_unstable();
        for key in keys {
            self.tile(world, key, threads).drop_if_baked();
        }
        label
    }

    /// The part round land node `node` of its mass cut as if the cap were
    /// `cap`, `(nodes, cell side)`, uncached; `None` on the sea. The
    /// version's canary, so the cut's code is in the hash. Bake the mass
    /// whole in this cache first: a cut marks the mass's unbaked nodes as
    /// over the cache's own cap, and a later whole bake would read that.
    fn bake_cut(
        &self,
        world: &ProceduralWorld,
        node: [i32; 2],
        cap: (usize, i32),
    ) -> Option<Network> {
        let (key, k) = field::locate(node);
        if self.tile(world, key, 1).height[k] < 0.0 {
            return None;
        }
        let started = Instant::now();
        let (domain, tiles) = mass::find(self, world, node, false, cap, 1);
        Some(self.finish(Some(world), &domain, started, tiles, 1))
    }

    /// Routes a domain round the world's sites into a network, sets its
    /// surfaces onto the sample's ground and its mouths to the sample's
    /// shore, and indexes it.
    /// Without a world (a synthetic domain in a test) there are no sites
    /// and no sample's shore to grade to.
    fn finish(
        &self,
        world: Option<&ProceduralWorld>,
        domain: &mass::Domain,
        started: Instant,
        tiles: usize,
        threads: usize,
    ) -> Network {
        let sites = world.map_or(&[][..], ProceduralWorld::sites);
        // The fold axis the grain runs along: the ranges' direction.
        let axis = world.map_or((1.0, 0.0), ProceduralWorld::range_axis);
        let routed = route::route(domain, &self.params, sites, axis);
        let mut built = rivers::build(domain, &routed, &self.params, self.seed, sites);
        let shore = Shore::new(domain, &routed);
        if let Some(world) = world {
            shape::settle(&mut built, &self.params, world, threads, &shore);
            rivers::fit_surfaces(&mut built, &self.params, world, threads, &shore);
            rivers::grade_mouths(&mut built, &self.params, world, threads);
            shape::meander(&mut built, &self.params, self.seed, sites);
        }
        let index = SegmentIndex::new(&built.rivers, &self.params);
        let hash = content_hash(domain, &built);
        let (segments, bucket_entries) = index.size();
        let mut network = Network {
            key: domain.nodes.first().copied().unwrap_or([0, 0]),
            capped: domain.capped,
            rivers: built.rivers,
            lakes: built.lakes,
            falls: built.falls,
            params: self.params,
            index,
            shore,
            hash,
            stats: BakeStats {
                nodes: domain.nodes.len(),
                seconds: started.elapsed().as_secs_f64(),
                tiles,
                segments,
                bucket_entries,
                sites: 0,
                sites_seconds: 0.0,
                roads: 0,
                road_km: 0.0,
                unreached: 0,
                roads_seconds: 0.0,
                grade_seconds: 0.0,
            },
            sites: Vec::new(),
            pads: NodeMap::default(),
            roads: Vec::new(),
            junctions: Vec::new(),
            road_index: crate::roads::RoadIndex::default(),
        };
        // The sites (ADR 0012), on the ground this network carves.
        if let Some(world) = world
            && !world.site_kinds().is_empty()
        {
            let placing = Instant::now();
            let (sites, grid) = place::place(world, &network, domain, self.seed, threads);
            network.set_sites(sites);
            network.stats.sites_seconds = placing.elapsed().as_secs_f64();
            // The roads between them (ticket 0005, step 5), on the lattice
            // the placement surveyed.
            if let Some(grid) = grid {
                let laying = Instant::now();
                let built = roads::build(world, &network, &grid, self.seed, threads);
                network.set_roads(built, laying.elapsed().as_secs_f64());
            }
        }
        network.stats.seconds = started.elapsed().as_secs_f64();
        network
    }

    /// The field tile `key`, computed on `threads` if no one has.
    fn tile(&self, world: &ProceduralWorld, key: [i32; 2], threads: usize) -> Arc<Tile> {
        if let Some(tile) = self
            .tiles
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Arc::clone(tile);
        }
        // One computation at a time, looked for again under the lock: two
        // sampling threads meeting the same tile would each have computed
        // it, on up to `threads` threads each.
        let _one = self
            .computing
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tile) = self
            .tiles
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Arc::clone(tile);
        }
        let computed = Arc::new(Tile::compute(world, self, key, threads));
        let mut tiles = self.tiles.write().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(tiles.entry(key).or_insert(computed))
    }

    /// Whether tile `key` is computed already: the bake counts what it adds.
    fn has_tile(&self, key: [i32; 2]) -> bool {
        self.tiles
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&key)
    }

    /// Field tiles held, and how many of them still hold their rain: what a
    /// bake keeps.
    #[must_use]
    pub fn tiles_held(&self) -> (usize, usize) {
        let tiles = self.tiles.read().unwrap_or_else(PoisonError::into_inner);
        let holding = tiles.values().filter(|tile| tile.holds_rest()).count();
        (tiles.len(), holding)
    }
}

/// Which of two answers wins at a point: inside a lake, then the nearer
/// centreline.
fn rank(water: &Water) -> (bool, u32) {
    let inside = water.kind == WaterKind::Lake && water.signed_distance_m < 0.0;
    (!inside, water.signed_distance_m.abs().to_bits())
}

/// The lattice index at or below a coordinate.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over the lattice is far inside an i32"
)]
fn node_of(metres: f64) -> i32 {
    (metres / LATTICE_M).floor() as i32
}

/// The nearest lattice index to a coordinate.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over the lattice is far inside an i32"
)]
fn nearest_node(metres: f64) -> i32 {
    (metres / LATTICE_M + 0.5).floor() as i32
}

/// A lattice index in world metres.
fn metres(i: i32) -> f64 {
    f64::from(i) * LATTICE_M
}

/// A metre value at the records' precision.
#[allow(
    clippy::cast_possible_truncation,
    reason = "f32 is the precision every record carries"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

/// A count as a float.
#[allow(
    clippy::cast_precision_loss,
    reason = "counts of nodes and vertices are far under 2^53"
)]
const fn real(count: usize) -> f64 {
    count as f64
}

/// A local index as the arrays store it.
fn local(index: usize) -> u32 {
    u32::try_from(index).expect("a mass under 4 billion nodes")
}

/// FNV-1a over the network: its key, its reaches, lakes and falls, bits and
/// all.
fn content_hash(domain: &mass::Domain, built: &rivers::Built) -> u64 {
    let mut hash = Fnv::default();
    let key = domain.nodes.first().copied().unwrap_or([0, 0]);
    hash.i32(key[0]);
    hash.i32(key[1]);
    hash.bytes(&[u8::from(domain.capped)]);
    hash.bytes(&(domain.nodes.len() as u64).to_le_bytes());
    for river in &built.rivers {
        hash.bytes(b"river");
        hash.bytes(&[river.order]);
        let (tag, value) = match river.end {
            RiverEnd::Sea => (0, 0),
            RiverEnd::Edge => (1, 0),
            RiverEnd::Sink => (2, 0),
            RiverEnd::Lake(lake) => (3, lake),
            RiverEnd::Joins(next) => (4, next),
        };
        hash.bytes(&[tag]);
        hash.bytes(&value.to_le_bytes());
        hash.bytes(&river.from_lake.unwrap_or(u32::MAX).to_le_bytes());
        for v in &river.vertices {
            hash.f64(v.x);
            hash.f64(v.z);
            hash.f32(v.surface_m);
            hash.f32(v.discharge_m3s);
            hash.f32(v.width_m);
            hash.f32(v.depth_m);
            hash.f32(v.cut_m);
            hash.bytes(&[u8::from(v.fall)]);
            hash.f32(v.meander_m);
            hash.f32(v.phase);
        }
    }
    for lake in &built.lakes {
        hash.bytes(b"lake");
        hash.f32(lake.level_m);
        hash.f32(lake.depth_m);
        hash.f32(lake.discharge_m3s);
        hash.f32(lake.detail_m);
        hash.f64(lake.outlet[0]);
        hash.f64(lake.outlet[1]);
        for node in &lake.nodes {
            hash.i32(node[0]);
            hash.i32(node[1]);
        }
    }
    for fall in &built.falls {
        hash.bytes(b"fall");
        for v in fall.top.iter().chain(&fall.bottom) {
            hash.f64(*v);
        }
        hash.f32(fall.width_m);
        hash.f32(fall.direction[0]);
        hash.f32(fall.direction[1]);
        hash.bytes(&fall.river.to_le_bytes());
    }
    hash.0
}

/// FNV-1a, 64 bits.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
}

/// What the world's version folds in for the network (ADR 0010, decision
/// 2). Hashing every mass at a handshake would cost a world of bakes, so it
/// is the bake's numbers ([`RiverParams`] and [`Fixed`]) and three canary
/// networks, as the sample canary stands for the sampler:
///
/// - [`CANARY_MASS`], a whole island of 5 310 nodes with 19 lakes and falls,
///   found by flood fill: the mass, its sea outlets, the lakes kept and
///   breached, the rivers, falls and mouths;
/// - two parts of that island cut as if the cap were [`CANARY_CUT`], one on
///   its coast and one with none, which drains over its cut edge: the cap's
///   cut;
/// - [`CANARY_WINDOW`] round Port Serein, its edge a coast: the ground the
///   network routes round.
///
/// Each costs a tile or less on one thread (about 25 ms in all in a release
/// build): the version is computed at startup beside other work, so it
/// spawns no pool. What none of them reaches is the reason
/// [`GENERATOR_VERSION`](crate::GENERATOR_VERSION) is bumped by hand for a
/// change to this module.
pub(crate) fn version_inputs(world: &ProceduralWorld, window: &Hydrology) -> VersionInputs {
    VersionInputs {
        params: world.hydrology().params,
        fixed: FIXED,
        canaries: canary_hashes(world, window),
    }
}

/// The river network's share of the world's version.
#[derive(Debug, Serialize)]
pub(crate) struct VersionInputs {
    params: RiverParams,
    fixed: Fixed,
    canaries: [u64; 4],
}

/// The canary window's cache on `world`: [`CANARY_WINDOW`] baked on one
/// thread, and nothing else ever.
pub(crate) fn canary_window(world: &ProceduralWorld) -> Hydrology {
    Hydrology::fixed_window(world, CANARY_WINDOW.0, CANARY_WINDOW.1)
}

/// The canaries' content hashes in order (the mass, its two cut parts, the
/// window, read from `window`, a [`canary_window`]), each zero where its
/// ground is sea in a world of other numbers.
pub(crate) fn canary_hashes(world: &ProceduralWorld, window: &Hydrology) -> [u64; 4] {
    let canary = Hydrology::new(world.hydrology().seed);
    canary.set_threads(1);
    let hash = |network: Option<&Network>| network.map_or(0, Network::content_hash);
    let mass = canary.network_at(world, metres(CANARY_MASS[0]), metres(CANARY_MASS[1]));
    // After the whole bake: see `bake_cut`.
    let cut = CANARY_CUT
        .2
        .map(|node| canary.bake_cut(world, node, (CANARY_CUT.0, CANARY_CUT.1)));
    let networks = window.networks();
    [
        hash(mass.as_deref()),
        hash(cut[0].as_ref()),
        hash(cut[1].as_ref()),
        hash(networks.first().map(AsRef::as_ref)),
    ]
}

/// The canary island on seed 42: the land node it is named by, 77 km east
/// of the origin.
pub const CANARY_MASS: [i32; 2] = [150, 54];

/// The cap the canary island is cut by, `(nodes, cell side)`, and the two
/// nodes whose parts are baked: one on the coast, one walled in by cut
/// edges.
pub const CANARY_CUT: (usize, i32, [[i32; 2]; 2]) = (1_000, 16, [[150, 54], [175, 41]]);

/// The canary window of lattice nodes on seed 42, about 11 km round Port
/// Serein: the rivers nearest it and the margin they keep from its extent.
pub const CANARY_WINDOW: ([i32; 2], [i32; 2]) = ([-12, -12], [10, 8]);

#[cfg(test)]
mod tests;
