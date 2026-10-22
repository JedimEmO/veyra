//! The road network's bake (ticket 0005, step 5; ADR 0012, decision 7): run
//! per land mass after its sites, on the ground its own network carves.
//! What it leaves is `crate::roads`'s records; it moves no ground.
//!
//! - **The places.** Every site, by its kind's road class, and the far end
//!   of an authored settlement's own road (Port Serein's 1.2 km road is the
//!   network's first link, kept as it was traced, so its city's fingerprint
//!   cannot move). Each is reached at an approach node outside its pad's
//!   band on the line from its centre through its arrival, so a road
//!   arrives along it.
//! - **Which places are joined.** Service places (towns and the settlement's
//!   road head) by a minimum spanning tree over the coarse travel cost
//!   between each and its nearest few, plus the edges whose detour through
//!   the tree is over [`LOOP_RATIO`] times their own cost: a backbone with a
//!   few loops, not a mesh. Then each track place (hamlet, station,
//!   landing), nearest the network first, joins the network where it is
//!   cheapest to reach, usually a point along a road; then each trail place
//!   (pass shelter, landmark) the same.
//! - **Where a road runs.** A search over the mass's surveyed 512 m lattice
//!   finds the valley or the saddle; a second on 64 m nodes in a corridor
//!   round it ([`Class::corridor_m`], wider where the lattice crosses a
//!   river, widened once if it finds nothing) finds the line. The cost is length, the gradient against the class's
//!   greatest, the climb, the side slope, the cut and fill the grade would
//!   make of what an edge climbs or falls past its grade (to a crossing's
//!   deck too), turning (dearer near a river, so a road swings onto its
//!   bridge), a river's meander belt,
//!   and water: the sea, lakes, standing water and settlements' ground are
//!   closed, and a river is crossed only on an edge that crosses its
//!   channel once, at a cost by kind: a culvert under a creek, a ford on a
//!   riffle for a track or a trail, or a bridge, up to 45 degrees skew and
//!   longer by it, dearer on a bend,
//!   at a mouth or between banks of unequal height. A graded road's
//!   crossing is read off the ground along the edge by the rule the record
//!   uses (`Builder::section`): a creek in a gorge is a bridge at its rim,
//!   the deck stands where the record stands it, and a bridge no class of
//!   the kit spans, or no pier reaches under, closes the edge. Travel on a road laid
//!   earlier costs [`DISCOUNT`] of new ground, so a later road takes an
//!   earlier one where it can and a track ends on a road.
//! - **The walls.** Round a graded road's bridge whose searched line comes
//!   down well past its grade, the line a few hundred metres either side
//!   is laid again on 16 m nodes, its deck slid and turned to where the
//!   line costs least and its approaches carrying their profile, so a road
//!   can slant down a valley's wall the 64 m nodes cannot follow (`walls`).
//! - **The line.** The 64 m nodes are resampled and smoothed into a curve,
//!   crossings, ends and the approach held, tightened where a bend is under
//!   the class's radius (twice it near a bridge); a graded road runs
//!   straight through each bridge, turned toward its way through the valley
//!   within the kit's skew, and is swung onto that straight on a cubic; a
//!   stretch that came to cross water the search did not cross, or into a
//!   river's water, is put back on the searched line. A road ending on
//!   another is eased onto that one's smoothed line.
//! - **What is recorded.** The centreline every
//!   [`POINT_STEP_M`] (or closer in a bend) with the ground there,
//!   the ends, and every crossing of a channel along the final line, with
//!   its banks, deck and span; a bridge's abutments stand back as far as
//!   keeps the shelf behind each out of the channel.
//!
//! Routes are searched in fixed batches on the bake's threads, each a pure
//! function of the network as the batch found it; the roads of one batch
//! keep [`SEPARATION_M`] apart, so one cannot have joined another, and they
//! are merged in a fixed order. So the roads are the same on any thread
//! count.

mod walls;

use walls::{Lines, Plan, Wall};

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use strata_data::RoadClass;

use super::field::parallel_rows;
use super::place::Grid;
use super::{FORD_DEPTH_M, LATTICE_M, Network, NodeMap, RiverEnd, WaterKind};
use crate::ProceduralWorld;
use crate::crossings::{BRIDGE_GAP_MOST_M, PIER_EMBED_M, PIER_MOST_M, PIERLESS_GAP_MOST_M};
use crate::roads::{
    Crossing, CrossingKind, Junction, POINT_LEAST_M, POINT_SAG_M, POINT_STEP_M, Road, RoadEnd,
    RoadPoint,
};
use crate::sites::ProceduralSite;

/// A set of lattice or fine nodes.
type NodeSet =
    std::collections::HashSet<[i32; 2], std::hash::BuildHasherDefault<super::NodeHasher>>;

/// Metres between the fine search's nodes: fine enough to find a valley's
/// floor and a saddle, coarse enough that a corridor of a few hundred
/// metres either side is a few thousand samples a kilometre.
const FINE_M: f64 = 64.0;

/// Fine nodes a lattice spacing.
const PER_LATTICE: i32 = 8;

/// The moves of both searches: the eight neighbours and the eight knight's
/// moves, so a line runs at sixteen bearings and not eight.
const MOVES: [[i32; 2]; 16] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
    [2, 1],
    [1, 2],
    [-1, 2],
    [-2, 1],
    [-2, -1],
    [-1, -2],
    [1, -2],
    [2, -1],
];

/// The fine search's moves: [`MOVES`] and the eight three-by-one moves, so
/// a line crosses a valley's wall at a slant shallow enough to come down it
/// at a road's grade where the wall is narrower than the node spacing: a
/// knight's move falls a wall's height in 143 m, a three-by-one in 202 m.
const FINE_MOVES: [[i32; 2]; 24] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
    [2, 1],
    [1, 2],
    [-1, 2],
    [-2, 1],
    [-2, -1],
    [-1, -2],
    [1, -2],
    [2, -1],
    [3, 1],
    [1, 3],
    [-1, 3],
    [-3, 1],
    [-3, -1],
    [-1, -3],
    [1, -3],
    [3, -1],
];

/// Metres a road keeps from an authored settlement's extent.
const SETTLEMENT_CLEAR_M: f64 = 100.0;

/// Metres a road keeps from a site's band, other than its own ends'.
const PAD_CLEAR_M: f64 = 8.0;

/// Metres past a pad's band its approach node is looked for.
const APPROACH_OUT_M: f64 = 24.0;

/// What travel along a road laid earlier costs, against new ground.
const DISCOUNT: f64 = 0.12;

/// The cost over a class's greatest gradient, per share over it.
const STEEP: f64 = 8.0;

/// Side slope a road takes without cost, rise over run.
const SIDE_FREE: f64 = 0.12;

/// What the meander belt multiplies travel by: a road keeps to a bench
/// above the water and goes down to it to cross.
const BELT: f64 = 4.0;

/// Metres past a river's bank and greatest bend its belt reaches.
const BELT_MARGIN_M: f64 = 6.0;

/// Metres past a channel's half-width and greatest bend within which a
/// node's edges are tested for crossings: over half the longest move.
const WET_REACH_M: f64 = 110.0;

/// The widest creek a culvert takes, metres.
const CULVERT_MAX_M: f64 = 2.0;

/// How far past a river's banks a road's profile is kept over its surface.
const FLOOR_RIVER_M: f64 = 30.0;

/// How far over the water a road beside it runs at least, metres.
const FLOOR_OVER_M: f64 = 0.5;

/// How far a creek's lower bank may stand over its water, metres, before a
/// graded road bridges it rather than culverting or fording it: the gorge
/// a culvert's fill would have to fill.
const GORGE_M: f64 = 4.0;

/// The most a bridge's deck stands over the lower of its banks, metres:
/// half their difference up to this, so its approaches share the climb
/// between a fill and a cutting.
const DECK_LIFT_MOST_M: f64 = 3.0;

/// The skew past which no crossing is made: the cosine of the angle between
/// the road and the channel's normal.
const LEAST_SQUARE: f64 = 0.42;

/// The skew past which no bridge is made, the same cosine: 45 degrees. The
/// kit's deck is square-ended and turned along the road's chord, so a skew
/// bridge is a longer one, its abutments standing back until the shelf
/// behind each keeps out of the channel (`Builder::dry_shelf`); a road
/// that runs along its valley crosses it on a skew line rather than a
/// dog-leg.
const BRIDGE_LEAST_SQUARE: f64 = 0.7;

/// What a right angle's turn costs a service road and a track within
/// [`APPROACH_REACH_M`] of a river a culvert does not take, metres (see
/// `Class::approach`): about what a skew deck's extra span costs, so a road
/// running along its valley takes a skew line across rather than two
/// square turns, and turns it does make there are spread over several
/// nodes, which the smoothing rounds into a curve.
const APPROACH_SERVICE_M: f64 = 400.0;
const APPROACH_TRACK_M: f64 = 200.0;

/// What a square metre of a service road's and a track's earthwork wedge
/// costs, metres of travel (see `Class::earthwork`).
const EARTH_SERVICE: f64 = 1.0;
const EARTH_TRACK: f64 = 0.5;

/// Metres of an edge's cut or fill the search lets pass without its
/// earthwork cost: the grade's own cut and fill over a field's undulation.
const EARTH_FREE_M: f64 = 2.0;

/// Metres past a channel's half-width and greatest bend within which a
/// smoothed stretch is walked for the water, and how far apart it is
/// sampled (see `Builder::in_river`).
const RIVER_NEAR_M: f64 = 10.0;
const RIVER_STEP_M: f64 = 2.0;

/// Metres between the indexed points of a channel.
const CHANNEL_STEP_M: f64 = 16.0;

/// The side of the channel index's buckets, metres.
const CHANNEL_BUCKET_M: f64 = 128.0;

/// Routes searched at a time.
const BATCH: usize = 128;

/// Pending routes a batch looks through for ones that keep apart.
const WINDOW: usize = 8 * BATCH;

/// Metres the boxes of two routes of a batch keep apart.
const SEPARATION_M: f64 = 1_500.0;

/// The most nodes a lattice search expands.
const COARSE_MOST: usize = 400_000;

/// How many times its own cost an edge's detour through the backbone must
/// be for the edge to become a loop.
const LOOP_RATIO: f64 = 2.0;

/// Each service place's nearest others the backbone weighs an edge to: a
/// town on a peninsula has its neighbours across the water first.
const BACKBONE_NEAREST: usize = 6;

/// Metres past which two service places are not weighed for an edge.
const BACKBONE_REACH_M: f64 = 45_000.0;

/// How many times its straight line a lattice route between two places may
/// cost before it is given up: round a bay, not a road.
const DETOUR_MOST: f64 = 6.0;

/// What the search's heuristic weighs the straight line by: over one, so
/// it is not a bound and the search touches a band round its path rather
/// than the corridor (the ground costs two or three times its length), at
/// the price of a path a little dearer than the least.
const PULL: f64 = 1.6;

/// The lattice search's: its nodes are surveyed, so only its expansions
/// cost.
const COARSE_PULL: f64 = 2.0;

/// The wavelength of the texture laid over travel, metres, and how much of
/// the cost it moves either way: fields, copses and soft ground no sample
/// holds, so a road over a plain wanders as one does and is not drawn with
/// a rule.
const TEXTURE_M: f64 = 700.0;
const TEXTURE_SHARE: f64 = 0.3;

/// Metres between the points the smoothing moves.
const SMOOTH_STEP_M: f64 = 16.0;

/// The share of its class's greatest gradient the searched path climbs at
/// past which the smoothing starts to hold it, and the least of the
/// smoothing a point at the limit keeps.
const STIFF_FROM: f64 = 0.5;
const STIFF_LEAST: f64 = 0.08;

/// Metres up or down the ground a smoothed point may move from where the
/// search put it: the line is smoothed in plan, and a hillside would turn a
/// sideways shift into a climb the search never chose.
const HEIGHT_TOLERANCE_M: f64 = 1.0;

/// Metres up or down the ground [`tighten`] may move a point of a bend
/// sharper than its class's radius.
const TIGHTEN_TOLERANCE_M: f64 = 3.0;

/// How a crossing's banks are found: steps of [`RIM_STEP_M`] out from the
/// channel's own bank while the ground rises at [`RIM_SLOPE`] or more, as
/// far as [`RIM_MOST_M`].
const RIM_STEP_M: f64 = 3.0;
const RIM_SLOPE: f64 = 0.35;
const RIM_MOST_M: f64 = 120.0;

/// Metres over which a road ending on another is eased onto its line.
const JOIN_BLEND_M: f64 = 120.0;

/// Metres a bridge's abutment may stand further out than its banks say, so
/// the shelf behind it keeps out of the channel (see `Builder::dry_shelf`),
/// and the step it is moved by: a fine one, as what it moves past becomes
/// the gap under the deck, where the bank is left standing.
const SHELF_PUSH_MOST_M: f64 = 30.0;
const SHELF_STEP_M: f64 = 1.0;

/// Metres past a bridge's span and shelf the line runs straight through it.
const STRAIGHT_PAST_M: f64 = 25.0;

/// Metres over which a wall search's stretch reads the ground's gradient
/// for the smoothing's bound.
const WALL_GRADIENT_M: f64 = 16.0;

/// Metres a bridge's straight stops short of a point already held (another
/// crossing's node, a stub, another bridge's straight): two held points side
/// by side would keep whatever step lay between them, and points free
/// between them let the smoothing close it.
const STRAIGHT_CLEAR_M: f64 = 2.0 * SMOOTH_STEP_M;

/// How much higher or lower than the line's own ground a straight's point
/// off the deck may stand, metres: turned off a valley's wall a straight
/// would leave the ground the search chose for one the grade must cut or
/// fill, which the turn is no reason for.
const STRAIGHT_TOLERANCE_M: f64 = 4.0;

/// How many times its class's corridor a service road's is where its
/// lattice route crosses a river a culvert does not take (see
/// `Builder::route`): the search's longest routes, so not much wider.
const RIVER_WIDEN: f64 = 1.6;

/// Metres either side of a bridge within which a bend is tightened to
/// [`APPROACH_RADIUS`] times its class's least radius: a road swings onto a
/// bridge.
const APPROACH_REACH_M: f64 = 150.0;
const APPROACH_RADIUS: f64 = 2.0;

/// How far a bridge's straight may turn off square to its channel when it
/// follows the road's way through the valley: the cosine of 40 degrees,
/// under [`BRIDGE_LEAST_SQUARE`]'s 45 so the smoothing round it has room. It
/// is held within that 45 of the water's flow too: a meander's flow and its
/// centreline's chord differ by ten degrees and more, and past 45 degrees
/// to the flow the channel reaches under an abutment's shelf
/// (`probe_skew`).
const SKEW_COS: f64 = 0.766;

/// Metres either side of a bridge whose line sets the way its straight
/// follows.
const TURN_REACH_M: f64 = 200.0;

/// Metres over which a road is swung onto the straight through a bridge
/// (see `Builder::blend_onto`), and how much higher or lower than the
/// smoothed line's ground the blend's may be.
const BLEND_M: f64 = 150.0;
const BLEND_TOLERANCE_M: f64 = 4.0;

/// A road's searched line as the shaping starts from it (see
/// `Builder::searched`).
struct Searched {
    points: Vec<[f64; 2]>,
    held: Vec<bool>,
    free: Vec<f64>,
    /// Whether each piece crosses a channel.
    crossing: Vec<bool>,
    /// The road's own sites.
    own: [Option<usize>; 2],
}

/// A bridge on a road's searched line, and the straight the line runs
/// through it (see `Builder::straighten`).
struct Straight {
    /// The searched edge that crosses it: `points[edge]` to `[edge + 1]`.
    edge: usize,
    /// Metres along that edge to the crossing.
    into_m: f64,
    /// Where the edge crosses the channel.
    at: [f64; 2],
    /// The edge's unit direction.
    unit: [f64; 2],
    /// The channel's unit normal there.
    normal: [f64; 2],
    /// Metres the straight runs either side of the crossing.
    half_m: f64,
    /// Metres either side of the crossing to the channel's banks along the
    /// edge: the gap under the deck, where the ground is the river's and
    /// is not compared (see `Builder::straighten`).
    gap_m: f64,
    /// Whether the wall search laid its edge: it weighed the turns onto
    /// the deck on its own nodes, so the straight keeps the edge's way.
    searched: bool,
}

/// A fine node's bits.
const OPEN: u8 = 1;
const IN_BELT: u8 = 2;
const WET: u8 = 4;
/// And a corridor's own.
const SAMPLED: u8 = 8;
const INNER: u8 = 16;
const ON_ROAD: u8 = 32;
const TARGET: u8 = 64;
/// A fine node's: within [`APPROACH_REACH_M`] of a channel wider than a
/// culvert takes, where a turn costs [`Class::approach`].
const NEAR_RIVER: u8 = 128;

/// A channel point's bits.
const ON_FALL: u8 = 1;
const AT_MOUTH: u8 = 2;

/// How each class is routed.
#[derive(Debug, Clone, Copy)]
struct Class {
    /// The greatest gradient, rise over run.
    grade: f64,
    /// Metres of length a metre of climb or descent costs.
    climb: f64,
    /// What side slope over [`SIDE_FREE`] costs, per its square: lightly
    /// on a graded class, whose grade benches a side-long line into the
    /// slope, so a road comes down a valley's wall along it rather than
    /// straight down it in a deep cutting.
    side: f64,
    /// Metres a right angle's turn costs.
    turn: f64,
    /// Metres a reversal costs on top, and its share for a turn sharper
    /// than a right angle: so a climb winds in switchbacks with legs of a
    /// few hundred metres and a tip that turns over two or three moves,
    /// which the smoothing can round, and not in a sawtooth of single
    /// moves doubling back on themselves. Dearer on a graded class than
    /// its earthwork over a short leg, or the search would zigzag down a
    /// slope to keep each edge within its grade.
    hairpin: f64,
    /// Metres either side of the lattice route the fine search looks.
    corridor_m: f64,
    /// Smoothing passes.
    iterations: usize,
    /// The tightest bend, metres.
    min_radius_m: f64,
    /// What a ford costs, where the class fords.
    ford_m: Option<f64>,
    /// What a bridge costs before its span.
    bridge_m: f64,
    /// Metres a right angle's turn costs near a river a culvert does not
    /// take, in place of [`Self::turn`]: a road swings onto a bridge on a
    /// flowing line and does not turn square to it at its abutment.
    approach: f64,
    /// What a square metre of the profile's earthwork costs, metres of
    /// travel (see [`Self::earthwork`]); none on an ungraded class.
    earth: f64,
    /// How far a bridge's deck stands over the water, metres.
    freeboard_m: f64,
    /// How far across the road a bridge's level shelf reaches either side
    /// of its centreline, metres: the carriageway and the verge.
    corner_m: f64,
    /// How deep the level shelf behind a bridge's abutment is, metres.
    shelf_m: f64,
    /// Whether the grade gives it a profile (ticket 0005, step 6).
    graded: bool,
}

impl Class {
    fn of(class: RoadClass) -> Self {
        let spec = crate::roads::spec(class);
        let (climb, side, turn, hairpin, corridor_m, iterations, ford_m, bridge_m) = match class {
            RoadClass::Service => (3.0, 3.2, 20.0, 1_200.0, 384.0, 120, None, 250.0),
            RoadClass::Track => (2.0, 1.6, 10.0, 480.0, 256.0, 60, Some(60.0), 350.0),
            RoadClass::Trail => (1.0, 1.0, 4.0, 20.0, 192.0, 16, Some(25.0), 200.0),
        };
        let (approach, earth) = match class {
            RoadClass::Service => (APPROACH_SERVICE_M, EARTH_SERVICE),
            RoadClass::Track => (APPROACH_TRACK_M, EARTH_TRACK),
            RoadClass::Trail => (0.0, 0.0),
        };
        Self {
            grade: spec.max_grade,
            climb,
            side,
            turn,
            hairpin,
            corridor_m,
            iterations,
            min_radius_m: spec.min_radius_m,
            ford_m,
            bridge_m,
            approach,
            earth,
            freeboard_m: super::grade::way(class).freeboard_m,
            corner_m: super::grade::way(class).half_m + super::grade::way(class).verge_m,
            shelf_m: super::grade::way(class).shelf_m,
            graded: super::grade::way(class).graded,
        }
    }

    /// What `length` metres over a rise of `dh` with a side slope of `side`
    /// cost.
    fn travel(&self, length: f64, dh: f64, side: f64) -> f64 {
        let r = dh.abs() / length / self.grade;
        let grade = if r <= 1.0 {
            0.5 * r * r
        } else {
            0.5 + STEEP * (r - 1.0)
        };
        let over = (side - SIDE_FREE).max(0.0);
        length * (1.0 + grade + self.side * over * over) + self.climb * dh.abs()
    }

    /// What the earthwork of `length` metres of road over a rise (or fall)
    /// of `dh` costs: what the class's grade cannot climb or fall in that
    /// length the profile must cut or fill, and the grade pays it back over
    /// a wedge as long again as its depth over the grade. So a line straight
    /// down a valley's wall, tens of metres off the ground, costs far more
    /// than one that slants across it a little at a time, and a road that
    /// comes down a wall straight onto a bridge's deck (see
    /// `Builder::search`) pays for the cutting the grade gives it there.
    fn earthwork(&self, length: f64, dh: f64) -> f64 {
        let excess = (dh.abs() - self.grade * length).max(0.0);
        let depth = (excess - EARTH_FREE_M).max(0.0);
        self.earth * depth * depth / (2.0 * self.grade)
    }

    /// The least the earthwork of an edge `lengths` long either side of a
    /// crossing, from and to ground `heights` high, can cost with the deck
    /// at `lowest` or higher (see [`Self::earthwork`]): what the search
    /// knows of a crossing before it reads its banks. Each side is free
    /// within its grade's reach of its node; between two such reaches that
    /// do not meet the two wedges cost least shared equally, and a deck held
    /// over both costs least at `lowest`. A shade under, so it never passes
    /// the cost it bounds by a rounding.
    fn earthwork_least(&self, lengths: [f64; 2], heights: [f64; 2], lowest: f64) -> f64 {
        let reach = lengths.map(|l| self.grade * l + EARTH_FREE_M);
        let tops = [heights[0] + reach[0], heights[1] + reach[1]];
        let bottoms = [heights[0] - reach[0], heights[1] - reach[1]];
        // Where the cost is least with no deck to hold: anywhere both
        // reaches hold, or the middle of the gap between them.
        let (best, at) = if bottoms[0].max(bottoms[1]) <= tops[0].min(tops[1]) {
            (0.0, tops[0].min(tops[1]))
        } else {
            let (lo, hi) = if tops[0] < bottoms[1] {
                (tops[0], bottoms[1])
            } else {
                (tops[1], bottoms[0])
            };
            let half = 0.5 * (hi - lo);
            (
                2.0 * self.earth * half * half / (2.0 * self.grade),
                lo + half,
            )
        };
        let least = if at >= lowest {
            best
        } else {
            self.earthwork(lengths[0], lowest - heights[0])
                + self.earthwork(lengths[1], heights[1] - lowest)
        };
        least * 0.999
    }

    /// What a move `length` long over a rise of `dh` costs to travel, on
    /// ground whose slope (rise over run) is `slope` and whose texture
    /// weights at its two ends sum to `textures`, [`BELT`] times as much in
    /// a river's belt. The fine search's moves and the wall search's
    /// (`walls`) alike, so a tuning of one is a tuning of the other.
    fn step(&self, length: f64, dh: f64, slope: f64, textures: f64, belt: bool) -> f64 {
        // The side slope from the ground's slope and the rise along the
        // move: what of the slope the road does not climb.
        let along = dh / length;
        let side = (slope * slope - along * along).max(0.0).sqrt();
        let mut cost = self.travel(length, dh, side) * 0.5 * textures;
        if belt {
            cost *= BELT;
        }
        cost
    }

    /// What a turn onto a move costs, by the cosine of its angle: near a
    /// river a road swings onto its crossing and off it on a flowing line,
    /// so a turn there costs more, and one past a right angle pays toward a
    /// hairpin.
    fn turn_cost(&self, cos: f64, near_river: bool) -> f64 {
        let turn_m = if near_river {
            self.turn.max(self.approach)
        } else {
            self.turn
        };
        turn_m * (1.0 - cos) + self.hairpin * (-cos).max(0.0)
    }

    /// What a crossing of a channel `width` wide and `depth` deep is.
    fn crossing(&self, width: f64, depth: f64, flags: u8) -> CrossingKind {
        if width < CULVERT_MAX_M {
            CrossingKind::Culvert
        } else if self.ford_m.is_some() && depth <= FORD_DEPTH_M && flags & AT_MOUTH == 0 {
            CrossingKind::Ford
        } else {
            CrossingKind::Bridge
        }
    }
}

/// A fine node as sampled.
#[derive(Debug, Clone, Copy, Default)]
struct Fine {
    height: f32,
    /// The ground's slope, rise over run.
    slope: f32,
    flags: u8,
}

/// Sixteen by sixteen fine nodes, each sampled at most once a bake.
struct Block {
    have: [u64; 4],
    cells: [Fine; 256],
}

/// The fine nodes sampled so far. Only read while a batch searches, written
/// between batches: what a node holds is a pure function of the node, so
/// whether a route found it here or sampled it itself changes nothing.
#[derive(Default)]
struct FineCache {
    blocks: NodeMap<Box<Block>>,
}

fn block_of(n: [i32; 2]) -> ([i32; 2], usize) {
    let k = (n[1] & 15) * 16 + (n[0] & 15);
    (
        [n[0] >> 4, n[1] >> 4],
        usize::try_from(k).expect("a slot of 256"),
    )
}

impl FineCache {
    fn get(&self, n: [i32; 2]) -> Option<Fine> {
        let (key, k) = block_of(n);
        let block = self.blocks.get(&key)?;
        (block.have[k / 64] >> (k % 64) & 1 == 1).then(|| block.cells[k])
    }

    fn put(&mut self, n: [i32; 2], fine: Fine) {
        let (key, k) = block_of(n);
        let block = self.blocks.entry(key).or_insert_with(|| {
            Box::new(Block {
                have: [0; 4],
                cells: [Fine::default(); 256],
            })
        });
        block.have[k / 64] |= 1 << (k % 64);
        block.cells[k] = fine;
    }
}

/// A point of a channel's centreline as the crossing test reads it.
#[derive(Debug, Clone, Copy)]
struct ChannelPoint {
    x: f64,
    z: f64,
    surface: f32,
    width: f32,
    depth: f32,
    reach: u32,
    flags: u8,
}

/// A reach's line from one vertex to the next, for the lattice search.
#[derive(Debug, Clone, Copy)]
struct Line {
    a: [f64; 2],
    b: [f64; 2],
    width: f32,
    fall: bool,
}

/// Ids in square buckets.
struct Buckets {
    side: f64,
    cells: NodeMap<Vec<u32>>,
}

impl Buckets {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a world coordinate over a bucket is far inside an i32"
    )]
    fn cell(&self, metres: f64) -> i32 {
        (metres / self.side).floor() as i32
    }

    /// Buckets of `side` holding each id of `boxes` in every bucket its box
    /// `[min_x, min_z, max_x, max_z]` touches, built in `rows` on `threads`.
    fn new(side: f64, boxes: &[(u32, [f64; 4])], threads: usize) -> Self {
        let mut out = Self {
            side,
            cells: NodeMap::default(),
        };
        let chunk = 4_096;
        let groups = parallel_rows(boxes.len().div_ceil(chunk), threads, |row| {
            let mut keyed: Vec<([i32; 2], u32)> = Vec::new();
            for &(id, b) in &boxes[row * chunk..((row + 1) * chunk).min(boxes.len())] {
                for j in out.cell(b[1])..=out.cell(b[3]) {
                    for i in out.cell(b[0])..=out.cell(b[2]) {
                        keyed.push(([i, j], id));
                    }
                }
            }
            keyed.sort_unstable();
            keyed
                .chunk_by(|a, b| a.0 == b.0)
                .map(|g| (g[0].0, g.iter().map(|e| e.1).collect::<Vec<_>>()))
                .collect::<Vec<_>>()
        });
        for (key, ids) in groups.into_iter().flatten() {
            out.cells.entry(key).or_default().extend(ids);
        }
        out
    }

    /// Every id in a bucket the box touches, sorted, once each.
    fn within(&self, b: [f64; 4], out: &mut Vec<u32>) {
        out.clear();
        for j in self.cell(b[1])..=self.cell(b[3]) {
            for i in self.cell(b[0])..=self.cell(b[2]) {
                if let Some(ids) = self.cells.get(&[i, j]) {
                    out.extend_from_slice(ids);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The mass's channels for the crossing tests.
struct Channels {
    points: Vec<ChannelPoint>,
    /// Segments `k` (from point `k` to `k + 1` of one reach).
    segments: Buckets,
    lines: Vec<Line>,
    coarse: Buckets,
}

/// Where a segment crosses a channel.
#[derive(Debug, Clone, Copy)]
struct Hit {
    /// The channel segment: point `seg` to `seg + 1`.
    seg: u32,
    /// Its share along the segment that was tested.
    t: f64,
    /// Its share along the channel segment.
    u: f64,
}

/// Where segments `a`-`b` and `p`-`q` cross: the share along each, the
/// first in `0..=1`, the second in `0..1` so a crossing at a shared point
/// counts once.
#[allow(
    clippy::many_single_char_names,
    reason = "two segments' ends and their shares, as the geometry names them"
)]
fn intersect(a: [f64; 2], b: [f64; 2], p: [f64; 2], q: [f64; 2]) -> Option<(f64, f64)> {
    let d1 = [b[0] - a[0], b[1] - a[1]];
    let d2 = [q[0] - p[0], q[1] - p[1]];
    let denom = d1[0] * d2[1] - d1[1] * d2[0];
    if denom.abs() < 1e-12 {
        return None;
    }
    let w = [p[0] - a[0], p[1] - a[1]];
    let t = (w[0] * d2[1] - w[1] * d2[0]) / denom;
    let u = (w[0] * d1[1] - w[1] * d1[0]) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..1.0).contains(&u)).then_some((t, u))
}

impl Channels {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "metres and m3/s at the records' f32 precision"
    )]
    fn new(network: &Network, threads: usize) -> Self {
        let params = &network.params;
        let rivers = &network.rivers;
        let per = 64;
        let parts = parallel_rows(rivers.len().div_ceil(per), threads, |row| {
            let mut points = Vec::new();
            let mut lines = Vec::new();
            for (r, river) in rivers
                .iter()
                .enumerate()
                .skip(row * per)
                .take(per.min(rivers.len().saturating_sub(row * per)))
            {
                let v = &river.vertices;
                let mut along = vec![0.0; v.len()];
                for k in 1..v.len() {
                    let (dx, dz) = (v[k].x - v[k - 1].x, v[k].z - v[k - 1].z);
                    along[k] = along[k - 1] + (dx * dx + dz * dz).sqrt();
                }
                let total = along.last().copied().unwrap_or(0.0);
                let falls: Vec<(f64, f64)> = (0..v.len().saturating_sub(1))
                    .filter(|&k| v[k].fall)
                    .map(|k| (along[k] - 30.0, along[k + 1] + 30.0))
                    .collect();
                let reach = u32::try_from(r).expect("fewer than 4 billion reaches");
                let line = river.centreline(params, CHANNEL_STEP_M);
                for p in line {
                    let mut flags = 0;
                    if falls.iter().any(|&(a, b)| p.along_m >= a && p.along_m <= b) {
                        flags |= ON_FALL;
                    }
                    if river.end == RiverEnd::Sea && p.along_m > total - 300.0 {
                        flags |= AT_MOUTH;
                    }
                    points.push(ChannelPoint {
                        x: p.x,
                        z: p.z,
                        surface: p.surface_m as f32,
                        width: p.width_m as f32,
                        depth: p.depth_m as f32,
                        reach,
                        flags,
                    });
                }
                for k in 0..v.len().saturating_sub(1) {
                    lines.push(Line {
                        a: [v[k].x, v[k].z],
                        b: [v[k + 1].x, v[k + 1].z],
                        width: v[k].width_m.max(v[k + 1].width_m),
                        fall: v[k].fall,
                    });
                }
            }
            (points, lines)
        });
        let mut points = Vec::new();
        let mut lines = Vec::new();
        for (p, l) in parts {
            points.extend(p);
            lines.extend(l);
        }
        let boxes: Vec<(u32, [f64; 4])> = (0..points.len().saturating_sub(1))
            .filter(|&k| points[k].reach == points[k + 1].reach)
            .map(|k| {
                let (a, b) = (points[k], points[k + 1]);
                (
                    u32::try_from(k).expect("fewer than 4 billion points"),
                    [a.x.min(b.x), a.z.min(b.z), a.x.max(b.x), a.z.max(b.z)],
                )
            })
            .collect();
        let segments = Buckets::new(CHANNEL_BUCKET_M, &boxes, threads);
        let line_boxes: Vec<(u32, [f64; 4])> = lines
            .iter()
            .enumerate()
            .map(|(k, l)| {
                (
                    u32::try_from(k).expect("fewer than 4 billion lines"),
                    [
                        l.a[0].min(l.b[0]),
                        l.a[1].min(l.b[1]),
                        l.a[0].max(l.b[0]),
                        l.a[1].max(l.b[1]),
                    ],
                )
            })
            .collect();
        let coarse = Buckets::new(LATTICE_M, &line_boxes, threads);
        Self {
            points,
            segments,
            lines,
            coarse,
        }
    }

    /// Every crossing of a channel by the segment `a`-`b`, in order along
    /// it.
    fn hits(&self, a: [f64; 2], b: [f64; 2], scratch: &mut Vec<u32>) -> Vec<Hit> {
        self.segments.within(
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ],
            scratch,
        );
        let mut out = Vec::new();
        for &s in scratch.iter() {
            let (p, q) = (self.points[s as usize], self.points[s as usize + 1]);
            if let Some((t, u)) = intersect(a, b, [p.x, p.z], [q.x, q.z]) {
                out.push(Hit { seg: s, t, u });
            }
        }
        out.sort_by(|x, y| x.t.total_cmp(&y.t).then(x.seg.cmp(&y.seg)));
        out
    }

    /// [`hits`](Self::hits) into `out`, for the search's every wet edge:
    /// no allocation, buckets read in place, a segment two buckets hold
    /// counted once.
    fn hits_into(&self, a: [f64; 2], b: [f64; 2], seen: &mut Vec<u32>, out: &mut Vec<Hit>) {
        out.clear();
        seen.clear();
        let bx = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ];
        let buckets = &self.segments;
        for j in buckets.cell(bx[1])..=buckets.cell(bx[3]) {
            for i in buckets.cell(bx[0])..=buckets.cell(bx[2]) {
                let Some(ids) = buckets.cells.get(&[i, j]) else {
                    continue;
                };
                for &s in ids {
                    let (p, q) = (self.points[s as usize], self.points[s as usize + 1]);
                    if let Some((t, u)) = intersect(a, b, [p.x, p.z], [q.x, q.z])
                        && !seen.contains(&s)
                    {
                        seen.push(s);
                        out.push(Hit { seg: s, t, u });
                    }
                }
            }
        }
        if out.len() > 1 {
            out.sort_by(|x, y| x.t.total_cmp(&y.t).then(x.seg.cmp(&y.seg)));
        }
    }

    /// The channel at a hit: its point, the channel's unit direction there
    /// and whether it bends there.
    #[allow(
        clippy::many_single_char_names,
        reason = "a segment's ends, its index and the share along it"
    )]
    fn at(&self, hit: &Hit) -> (ChannelPoint, [f64; 2], bool) {
        let s = hit.seg as usize;
        let (p, q) = (self.points[s], self.points[s + 1]);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a share at the records' f32"
        )]
        let u = hit.u as f32;
        let lerp = |x: f32, y: f32| x + (y - x) * u;
        let here = ChannelPoint {
            x: p.x + (q.x - p.x) * hit.u,
            z: p.z + (q.z - p.z) * hit.u,
            surface: lerp(p.surface, q.surface),
            width: lerp(p.width, q.width),
            depth: lerp(p.depth, q.depth),
            reach: p.reach,
            flags: p.flags | q.flags,
        };
        let unit = |a: ChannelPoint, b: ChannelPoint| {
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let l = (dx * dx + dz * dz).sqrt().max(1e-9);
            [dx / l, dz / l]
        };
        let dir = unit(p, q);
        // Two segments either side, on the same reach: over about 25
        // degrees between them is a bend.
        let same = |k: usize| self.points.get(k).is_some_and(|c| c.reach == p.reach);
        let bent = if s >= 2 && same(s - 2) && same(s + 3) {
            let before = unit(self.points[s - 2], self.points[s - 1]);
            let after = unit(self.points[s + 2], self.points[s + 3]);
            before[0] * after[0] + before[1] * after[1] < 0.9
        } else {
            false
        };
        (here, dir, bent)
    }

    /// The reaches' lines a lattice edge crosses: each one's width and
    /// whether it falls there.
    fn line_hits(&self, a: [f64; 2], b: [f64; 2], seen: &mut Vec<u32>, out: &mut Vec<(f32, bool)>) {
        out.clear();
        seen.clear();
        let bx = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ];
        let buckets = &self.coarse;
        for j in buckets.cell(bx[1])..=buckets.cell(bx[3]) {
            for i in buckets.cell(bx[0])..=buckets.cell(bx[2]) {
                let Some(ids) = buckets.cells.get(&[i, j]) else {
                    continue;
                };
                for &k in ids {
                    let l = self.lines[k as usize];
                    if intersect(a, b, l.a, l.b).is_some() && !seen.contains(&k) {
                        seen.push(k);
                        out.push((l.width, l.fall));
                    }
                }
            }
        }
    }
}

/// A place the network reaches.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// Its site, an index into the network's.
    site: Option<usize>,
    /// Or the authored settlement whose road head it is.
    settlement: Option<u32>,
    class: RoadClass,
    /// The fine node a road reaches it at.
    node: [i32; 2],
    /// Where the road ends: the pad's arrival, or the road head.
    stub: [f64; 2],
}

/// How a route ends.
#[derive(Debug, Clone, Copy)]
enum Goal {
    /// At a place's node.
    Place(usize),
    /// Wherever the network is cheapest to reach.
    Network,
    /// Anywhere along a settlement's own road (this index), whose head is
    /// the place: a link to Port Serein meets its road where that is
    /// cheapest, not only at its far end.
    Road(u32, usize),
}

/// A route to search.
#[derive(Debug, Clone, Copy)]
struct Spec {
    from: usize,
    goal: Goal,
    class: RoadClass,
}

/// What a search found.
struct Found {
    nodes: Vec<[i32; 2]>,
    /// Edges `k` (node `k` to `k + 1`) that cross a channel.
    crossings: Vec<usize>,
    /// Fine nodes it sampled that the cache did not hold.
    sampled: Vec<([i32; 2], Fine)>,
}

/// Where a road found ends, before the records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawEnd {
    Place(usize),
    Road(u32, [i32; 2]),
    Exit(u32),
    Head(u32),
}

/// A road as merged, before its line is shaped.
#[derive(Debug, Clone)]
struct RawRoad {
    class: RoadClass,
    nodes: Vec<[i32; 2]>,
    /// A settlement's own road: its traced points, kept.
    fixed: Option<Vec<[f64; 2]>>,
    ends: [RawEnd; 2],
    /// A straight piece before the first node and after the last: into a
    /// pad's arrival, or from a road head.
    stubs: [Option<[f64; 2]>; 2],
    crossings: Vec<usize>,
}

/// A fine node's position.
fn at(n: [i32; 2]) -> [f64; 2] {
    [f64::from(n[0]) * FINE_M, f64::from(n[1]) * FINE_M]
}

/// The fine node nearest a point.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over 64 m is far inside an i32"
)]
fn fine_node(p: [f64; 2]) -> [i32; 2] {
    [
        (p[0] / FINE_M + 0.5).floor() as i32,
        (p[1] / FINE_M + 0.5).floor() as i32,
    ]
}

/// A lattice node's position.
fn lattice_at(n: [i32; 2]) -> [f64; 2] {
    [super::metres(n[0]), super::metres(n[1])]
}

/// The lattice node nearest a fine node.
fn lattice_of(n: [i32; 2]) -> [i32; 2] {
    [
        (n[0] + PER_LATTICE / 2).div_euclid(PER_LATTICE),
        (n[1] + PER_LATTICE / 2).div_euclid(PER_LATTICE),
    ]
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    (dx * dx + dz * dz).sqrt()
}

/// The nearest point of segment `a`-`b` to `p`, and its share along it.
fn project(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> ([f64; 2], f64) {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dz * dz;
    let u = if l2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ([a[0] + dx * u, a[1] + dz * u], u)
}

/// The fine search's queue entry for a node itself, beside the crossing
/// edges it puts off (see `Builder::search`).
const NOW: u32 = u32::MAX;

/// A queue entry: least first, ties to the lower key.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Queued<K> {
    f: f64,
    key: K,
}

impl<K: Ord + PartialEq> Eq for Queued<K> {}

impl<K: Ord + PartialEq> PartialOrd for Queued<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: Ord + PartialEq> Ord for Queued<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .total_cmp(&self.f)
            .then_with(|| other.key.cmp(&self.key))
    }
}

/// The sites, in buckets, for the pads a road keeps off and the ground a
/// pad lays under a road.
struct SiteGrid {
    buckets: Buckets,
}

impl SiteGrid {
    fn new(sites: &[ProceduralSite]) -> Self {
        let boxes: Vec<(u32, [f64; 4])> = sites
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let r = s.outer_m() + PAD_CLEAR_M;
                (
                    u32::try_from(k).expect("fewer than 4 billion sites"),
                    [
                        s.centre[0] - r,
                        s.centre[1] - r,
                        s.centre[0] + r,
                        s.centre[1] + r,
                    ],
                )
            })
            .collect();
        Self {
            buckets: Buckets::new(2_048.0, &boxes, 1),
        }
    }
}

/// The corridor of one search: a box of fine nodes, those within reach of
/// the lattice route sampled as the search touches them.
struct Local {
    i0: i32,
    j0: i32,
    w: i32,
    h: i32,
    height: Vec<f32>,
    slope: Vec<f32>,
    /// [`texture`] at each node, `1` give or take [`TEXTURE_SHARE`].
    texture: Vec<f32>,
    flags: Vec<u8>,
    /// The pads the route keeps off: centre and radius.
    pads: Vec<([f64; 2], f64)>,
    from: [i32; 2],
    /// A link's goal node; `None` for a join, whose targets are the
    /// network's.
    goal: Option<[i32; 2]>,
    spec: Spec,
}

impl Local {
    fn index(&self, n: [i32; 2]) -> Option<usize> {
        let (i, j) = (n[0] - self.i0, n[1] - self.j0);
        (i >= 0 && j >= 0 && i < self.w && j < self.h)
            .then(|| usize::try_from(j * self.w + i).expect("inside the box"))
    }

    fn node(&self, k: usize) -> [i32; 2] {
        let k = i32::try_from(k).expect("a box under 2 billion nodes");
        [self.i0 + k % self.w, self.j0 + k / self.w]
    }
}

/// Whether two boxes `[min_x, min_z, max_x, max_z]` come within `gap`.
fn boxes_meet(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[0] - gap <= b[2] && b[0] - gap <= a[2] && a[1] - gap <= b[3] && b[1] - gap <= a[3]
}

/// The nodes and places of the network so far, in buckets, for the nearest
/// one to a place.
struct Near {
    cells: NodeMap<Vec<[f64; 2]>>,
    side: f64,
    len: usize,
}

impl Near {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a world coordinate over a bucket is far inside an i32"
    )]
    fn cell(&self, p: [f64; 2]) -> [i32; 2] {
        [
            (p[0] / self.side).floor() as i32,
            (p[1] / self.side).floor() as i32,
        ]
    }

    fn push(&mut self, p: [f64; 2]) {
        let key = self.cell(p);
        self.cells.entry(key).or_default().push(p);
        self.len += 1;
    }

    /// The nearest point held, rings outward until no nearer one can be.
    fn nearest(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        if self.len == 0 {
            return None;
        }
        let c = self.cell(p);
        let mut best: Option<(f64, [f64; 2])> = None;
        for r in 0..2_000_i32 {
            let mut visit = |i: i32, j: i32| {
                if let Some(list) = self.cells.get(&[c[0] + i, c[1] + j]) {
                    for &q in list {
                        let d = distance(p, q);
                        if best.is_none_or(|(b, bq)| {
                            d.total_cmp(&b)
                                .then(q[0].total_cmp(&bq[0]))
                                .then(q[1].total_cmp(&bq[1]))
                                .is_lt()
                        }) {
                            best = Some((d, q));
                        }
                    }
                }
            };
            if r == 0 {
                visit(0, 0);
            } else {
                for i in -r..=r {
                    visit(i, -r);
                    visit(i, r);
                }
                for j in -r + 1..r {
                    visit(-r, j);
                    visit(r, j);
                }
            }
            if best.is_some_and(|(d, _)| d <= f64::from(r) * self.side) {
                break;
            }
        }
        best.map(|(_, q)| q)
    }
}

/// What the bake hands the network.
pub(super) struct Built {
    pub roads: Vec<Road>,
    pub junctions: Vec<Junction>,
    /// Sites no road reached.
    pub unreached: usize,
    /// Seconds the grade's profiles took.
    pub grade_seconds: f64,
}

/// The bake's state.
struct Builder<'a> {
    world: &'a ProceduralWorld,
    network: &'a Network,
    grid: &'a Grid,
    channels: Channels,
    site_grid: SiteGrid,
    cache: FineCache,
    places: Vec<Place>,
    /// Per place, the first road that reached it.
    first_road: Vec<Option<u32>>,
    roads: Vec<RawRoad>,
    on_road: NodeMap<u32>,
    anchors: NodeMap<u32>,
    coarse_on: NodeSet,
    near: Near,
    /// Sites with no open node to reach them at.
    unplaced: usize,
    /// Places the network grew from where it had nothing to join: a lone
    /// town, or the first place of a mass with none.
    roots: Vec<usize>,
    threads: usize,
    /// Each move's length in fine spacings and its unit direction.
    moves: [(f64, [f64; 2]); 24],
    /// The texture's seed.
    seed: u64,
}

/// Builds the roads of the mass `network` holds, over the lattice `grid`
/// its sites were placed by (see the module docs).
pub(super) fn build(
    world: &ProceduralWorld,
    network: &Network,
    grid: &Grid,
    seed: strata_core::Seed,
    threads: usize,
) -> Built {
    let moves = FINE_MOVES.map(|m| {
        let l = (f64::from(m[0] * m[0] + m[1] * m[1])).sqrt();
        (l, [f64::from(m[0]) / l, f64::from(m[1]) / l])
    });
    let mut b = Builder {
        world,
        network,
        grid,
        channels: Channels::new(network, threads),
        site_grid: SiteGrid::new(&network.sites),
        cache: FineCache::default(),
        places: Vec::new(),
        first_road: Vec::new(),
        roads: Vec::new(),
        on_road: NodeMap::default(),
        anchors: NodeMap::default(),
        coarse_on: NodeSet::default(),
        near: Near {
            cells: NodeMap::default(),
            side: 1_024.0,
            len: 0,
        },
        unplaced: 0,
        roots: Vec::new(),
        threads,
        moves,
        seed: seed.derive("roads"),
    };
    b.places();
    b.backbone();
    b.join(RoadClass::Track);
    b.join(RoadClass::Trail);
    b.finish()
}

impl Builder<'_> {
    /// The fine node `n` as the ground has it.
    fn sample(&self, n: [i32; 2]) -> Fine {
        self.sample_at(at(n))
    }

    /// The ground at `p` as a search's node has it: the fine search's at
    /// its nodes, the wall search's at its own (see `walls`).
    #[allow(
        clippy::many_single_char_names,
        reason = "a point and its sample, as the geometry names them"
    )]
    fn sample_at(&self, p: [f64; 2]) -> Fine {
        let [x, z] = p;
        let s = self.world.sample_by(x, z, self.network);
        let clear = self
            .world
            .sites()
            .iter()
            .all(|site| site.extent_distance_at(x, z) >= SETTLEMENT_CLEAR_M);
        let mut flags = 0;
        if s.height >= 0.5 && s.standing_water().is_none() && clear {
            flags |= OPEN;
        }
        if let Some(water) = s.water {
            if water.kind == WaterKind::Lake {
                flags |= WET;
            } else {
                let params = &self.network.params;
                let d = f64::from(water.signed_distance_m.abs());
                let half = 0.5 * f64::from(water.width_m);
                let swing = params.bend_amplitude(f64::from(water.discharge_m3s));
                if d < half + f64::from(water.bank_m()) + swing + BELT_MARGIN_M {
                    flags |= IN_BELT;
                }
                if d < half + swing + WET_REACH_M {
                    flags |= WET;
                }
                if f64::from(water.width_m) >= CULVERT_MAX_M && d < half + swing + APPROACH_REACH_M
                {
                    flags |= NEAR_RIVER;
                }
            }
        }
        Fine {
            height: s.height,
            slope: s.landform.slope,
            flags,
        }
    }

    /// The water's flow at `p` as a unit vector, where it flows.
    fn flow_at(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let w = self.world.sample_by(p[0], p[1], self.network).water?;
        let v = [f64::from(w.flow[0]), f64::from(w.flow[1])];
        let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
        (l > 1e-9).then(|| [v[0] / l, v[1] / l])
    }

    fn fine(&self, n: [i32; 2], sampled: &mut Vec<([i32; 2], Fine)>) -> Fine {
        if let Some(f) = self.cache.get(n) {
            return f;
        }
        let f = self.sample(n);
        sampled.push((n, f));
        f
    }

    /// The places: every site by its kind's class, and the head of every
    /// authored settlement's road that ends on this mass.
    #[allow(
        clippy::too_many_lines,
        reason = "the sites' places, the road heads', and the settlements' own roads, in order"
    )]
    fn places(&mut self) {
        let kinds = self.world.site_kinds();
        let sites = &self.network.sites;
        let wanted: Vec<(
            Option<usize>,
            Option<u32>,
            RoadClass,
            [f64; 2],
            [f64; 2],
            f64,
        )> = sites
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let class = kinds
                    .iter()
                    .find(|kind| kind.id == s.kind())
                    .map_or(RoadClass::Track, |kind| kind.traits.road);
                let (dx, dz) = (s.arrival[0] - s.centre[0], s.arrival[1] - s.centre[1]);
                let l = (dx * dx + dz * dz).sqrt().max(1e-9);
                let out = s.blend_m + APPROACH_OUT_M;
                let p = [s.arrival[0] + dx / l * out, s.arrival[1] + dz / l * out];
                (Some(k), None, class, p, s.arrival, s.outer_m())
            })
            .chain(self.world.sites().iter().enumerate().filter_map(|(k, s)| {
                let last = s.road.points.last()?;
                let head = [last[0], last[1]];
                let n = [super::nearest_node(head[0]), super::nearest_node(head[1])];
                self.grid.open(n).then(|| {
                    (
                        None,
                        Some(u32::try_from(k).expect("a handful of settlements")),
                        RoadClass::Service,
                        head,
                        head,
                        0.0,
                    )
                })
            }))
            .collect();
        let found = parallel_rows(wanted.len().div_ceil(64), self.threads, |row| {
            let mut sampled = Vec::new();
            let nodes: Vec<Option<[i32; 2]>> = wanted[row * 64..((row + 1) * 64).min(wanted.len())]
                .iter()
                .map(|&(site, _, _, p, _, outer)| {
                    let c = fine_node(p);
                    let mut near: Vec<(f64, [i32; 2])> = (-2..=2)
                        .flat_map(|j| (-2..=2).map(move |i| [c[0] + i, c[1] + j]))
                        .map(|n| (distance(at(n), p), n))
                        .collect();
                    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    near.into_iter().map(|(_, n)| n).find(|&n| {
                        let pos = at(n);
                        let own = site.map(|k| &sites[k]);
                        if own.is_some_and(|s| distance(pos, s.centre) < outer + 4.0) {
                            return false;
                        }
                        if self.pad_blocks(pos, site) {
                            return false;
                        }
                        self.fine(n, &mut sampled).flags & OPEN != 0
                    })
                })
                .collect();
            (nodes, sampled)
        });
        let mut k = 0;
        for (nodes, sampled) in found {
            for (n, f) in sampled {
                self.cache.put(n, f);
            }
            for node in nodes {
                let (site, settlement, class, _, stub, _) = wanted[k];
                k += 1;
                let Some(node) = node else {
                    if site.is_some() {
                        self.unplaced += 1;
                    }
                    continue;
                };
                self.places.push(Place {
                    site,
                    settlement,
                    class,
                    node,
                    stub,
                });
                self.first_road.push(None);
            }
        }
        // Each settlement's own road is the network's first link.
        for p in 0..self.places.len() {
            let place = self.places[p];
            let Some(s) = place.settlement else { continue };
            let road = &self.world.sites()[s as usize].road;
            let fixed: Vec<[f64; 2]> = road.points.iter().map(|q| [q[0], q[1]]).collect();
            let mut nodes: Vec<[i32; 2]> = Vec::new();
            for q in &fixed {
                let n = fine_node(*q);
                if nodes.last() != Some(&n) {
                    nodes.push(n);
                }
            }
            let index = self.add(RawRoad {
                class: RoadClass::Service,
                nodes,
                fixed: Some(fixed),
                ends: [RawEnd::Exit(s), RawEnd::Head(s)],
                stubs: [None, None],
                crossings: Vec::new(),
            });
            self.first_road[p] = Some(index);
        }
        for (p, place) in self.places.iter().enumerate() {
            if place.class == RoadClass::Service {
                self.anchors
                    .insert(place.node, u32::try_from(p).expect("a place index"));
                self.coarse_on.insert(lattice_of(place.node));
                self.near.push(at(place.node));
            }
        }
    }

    /// Whether a site other than `own` keeps a road off `p`.
    fn pad_blocks(&self, p: [f64; 2], own: Option<usize>) -> bool {
        let mut ids = Vec::new();
        self.site_grid
            .buckets
            .within([p[0], p[1], p[0], p[1]], &mut ids);
        ids.iter().any(|&k| {
            let s = &self.network.sites[k as usize];
            Some(k as usize) != own && distance(p, s.centre) < s.outer_m() + PAD_CLEAR_M
        })
    }

    /// Whether a shaped line's point `p` stands where the search kept a road
    /// off: on the pad of a site other than its own ends' (`own`), in a
    /// lake's water, or within half the margin a road keeps from an
    /// authored settlement's extent. The search's own margins are wider, so
    /// a point the smoothing slid a little toward one is not refused.
    fn blocked(&self, p: [f64; 2], own: [Option<usize>; 2]) -> bool {
        let mut ids = Vec::new();
        self.site_grid
            .buckets
            .within([p[0], p[1], p[0], p[1]], &mut ids);
        let on_pad = ids.iter().any(|&k| {
            let s = &self.network.sites[k as usize];
            !own.contains(&Some(k as usize)) && distance(p, s.centre) < s.outer_m()
        });
        on_pad
            || self
                .network
                .shore
                .lake_here(&self.network.lakes, p[0], p[1])
                .is_some_and(|here| here.depth() > 0.0)
            || self
                .world
                .sites()
                .iter()
                .any(|site| site.extent_distance_at(p[0], p[1]) < 0.5 * SETTLEMENT_CLEAR_M)
    }

    /// Registers a road and its nodes.
    fn add(&mut self, road: RawRoad) -> u32 {
        let index = u32::try_from(self.roads.len()).expect("fewer than 4 billion roads");
        for &n in &road.nodes {
            if let std::collections::hash_map::Entry::Vacant(free) = self.on_road.entry(n) {
                free.insert(index);
                self.near.push(at(n));
            }
            self.coarse_on.insert(lattice_of(n));
        }
        self.roads.push(road);
        index
    }

    /// The backbone: service places joined by a spanning tree over the
    /// lattice's travel cost and a few loops.
    fn backbone(&mut self) {
        let hubs: Vec<usize> = (0..self.places.len())
            .filter(|&p| self.places[p].class == RoadClass::Service)
            .collect();
        if hubs.len() < 2 {
            self.roots.extend(hubs);
            return;
        }
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &a in &hubs {
            let pa = at(self.places[a].node);
            let mut others: Vec<(f64, usize)> = hubs
                .iter()
                .filter(|&&b| b != a)
                .map(|&b| (distance(pa, at(self.places[b].node)), b))
                .collect();
            others.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            for &(_, b) in others
                .iter()
                .take(BACKBONE_NEAREST)
                .filter(|o| o.0 <= BACKBONE_REACH_M)
            {
                pairs.push((a.min(b), a.max(b)));
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let service = Class::of(RoadClass::Service);
        let costs: Vec<Option<f64>> = parallel_rows(pairs.len(), self.threads, |k| {
            let (a, b) = pairs[k];
            self.coarse(
                at(self.places[a].node),
                Some(at(self.places[b].node)),
                None,
                &service,
            )
            .map(|(_, cost)| cost)
        });
        let mut edges: Vec<(f64, usize, usize)> = pairs
            .iter()
            .zip(&costs)
            .filter_map(|(&(a, b), c)| c.map(|c| (c, a, b)))
            .collect();
        edges.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
        // Kruskal's tree, then the loops a detour would make long.
        let mut root: Vec<usize> = (0..self.places.len()).collect();
        let mut chosen: Vec<(f64, usize, usize)> = Vec::new();
        let mut rest = Vec::new();
        for &(c, a, b) in &edges {
            let (ra, rb) = (find(&mut root, a), find(&mut root, b));
            if ra == rb {
                rest.push((c, a, b));
            } else {
                root[ra] = rb;
                chosen.push((c, a, b));
            }
        }
        let tree = chosen.len();
        for &(c, a, b) in &rest {
            if graph_cost(&chosen, self.places.len(), a, b) > LOOP_RATIO * c {
                chosen.push((c, a, b));
            }
        }
        let specs: VecDeque<Spec> = chosen
            .iter()
            .map(|&(_, a, b)| {
                let head = |p: usize| {
                    self.places[p]
                        .settlement
                        .and_then(|_| self.first_road[p])
                        .map(|r| (r, p))
                };
                let (from, goal) = match (head(a), head(b)) {
                    (Some((r, p)), _) => (b, Goal::Road(r, p)),
                    (_, Some((r, p))) => (a, Goal::Road(r, p)),
                    _ => (a, Goal::Place(b)),
                };
                Spec {
                    from,
                    goal,
                    class: RoadClass::Service,
                }
            })
            .collect();
        tracing::debug!(
            hubs = hubs.len(),
            tree,
            loops = chosen.len() - tree,
            "road backbone"
        );
        self.run(specs);
    }

    /// Every place of `class` not yet on the network joins it, nearest
    /// first.
    fn join(&mut self, class: RoadClass) {
        let mut todo: Vec<usize> = (0..self.places.len())
            .filter(|&p| self.places[p].class == class && self.first_road[p].is_none())
            .collect();
        if todo.is_empty() {
            return;
        }
        if self.near.len == 0 {
            // No network on the mass yet: the first place is its root.
            let root = todo.remove(0);
            self.roots.push(root);
            let node = self.places[root].node;
            self.anchors
                .insert(node, u32::try_from(root).expect("a place index"));
            self.coarse_on.insert(lattice_of(node));
            self.near.push(at(node));
        }
        let mut order: Vec<(f64, usize)> = todo
            .into_iter()
            .map(|p| {
                let pos = at(self.places[p].node);
                let d = self
                    .near
                    .nearest(pos)
                    .map_or(f64::INFINITY, |q| distance(pos, q));
                (d, p)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let specs = order
            .into_iter()
            .map(|(_, p)| Spec {
                from: p,
                goal: Goal::Network,
                class,
            })
            .collect();
        self.run(specs);
    }

    /// The box a route is expected to keep to.
    fn reach_box(&self, spec: &Spec) -> [f64; 4] {
        let a = at(self.places[spec.from].node);
        let b = match spec.goal {
            Goal::Place(p) | Goal::Road(_, p) => at(self.places[p].node),
            Goal::Network => self.near.nearest(a).unwrap_or(a),
        };
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ]
    }

    /// Searches the routes in batches and merges each in order.
    fn run(&mut self, mut pending: VecDeque<Spec>) {
        while !pending.is_empty() {
            let mut batch: Vec<Spec> = Vec::with_capacity(BATCH);
            let mut boxes: Vec<[f64; 4]> = Vec::with_capacity(BATCH);
            let mut k = 0;
            while k < pending.len().min(WINDOW) && batch.len() < BATCH {
                let spec = pending[k];
                let b = self.reach_box(&spec);
                let shared = |s: &Spec| {
                    let mut ends = vec![s.from];
                    if let Goal::Place(p) | Goal::Road(_, p) = s.goal {
                        ends.push(p);
                    }
                    ends
                };
                // Links are searched together, whatever their boxes and ends:
                // they are long and would each hold a batch; where two
                // follow one valley the merge folds the later onto the
                // earlier.
                let link = matches!(spec.goal, Goal::Place(_) | Goal::Road(..));
                let clash = !link
                    && batch.iter().zip(&boxes).any(|(other, ob)| {
                        boxes_meet(b, *ob, SEPARATION_M)
                            || shared(other).iter().any(|e| shared(&spec).contains(e))
                    });
                if clash {
                    k += 1;
                } else {
                    batch.push(spec);
                    boxes.push(b);
                    pending.remove(k);
                }
            }
            // The longest first, so no thread is left with one at the end.
            let mut order: Vec<usize> = (0..batch.len()).collect();
            let size = |b: &[f64; 4]| (b[2] - b[0]) + (b[3] - b[1]);
            order.sort_by(|&x, &y| size(&boxes[y]).total_cmp(&size(&boxes[x])).then(x.cmp(&y)));
            let found = parallel_rows(batch.len(), self.threads, |k| self.route(&batch[order[k]]));
            let mut found: Vec<Option<Option<Found>>> = found.into_iter().map(Some).collect();
            let mut by_spec: Vec<Option<Found>> = (0..batch.len()).map(|_| None).collect();
            for (k, &o) in order.iter().enumerate() {
                by_spec[o] = found[k].take().expect("every route searched once");
            }
            for (spec, found) in batch.iter().zip(by_spec) {
                if let Some(found) = found {
                    self.merge(spec, found);
                }
            }
        }
    }

    /// The route of `spec` on the network as the batch found it.
    fn route(&self, spec: &Spec) -> Option<Found> {
        let class = Class::of(spec.class);
        let from = self.places[spec.from];
        let goal_pos = match spec.goal {
            Goal::Place(p) => Some(at(self.places[p].node)),
            Goal::Network | Goal::Road(..) => None,
        };
        let mut sampled = Vec::new();
        // Already on the network: the stub alone.
        if goal_pos.is_none() && self.is_target(&spec.goal, spec.from, from.node) {
            return Some(Found {
                nodes: vec![from.node],
                crossings: Vec::new(),
                sampled,
            });
        }
        // A settlement's road as the lattice sees it.
        let road_nodes: Option<NodeSet> = match spec.goal {
            Goal::Road(r, p) => Some(
                self.roads[r as usize]
                    .nodes
                    .iter()
                    .chain(std::iter::once(&self.places[p].node))
                    .map(|&n| lattice_of(n))
                    .collect(),
            ),
            _ => None,
        };
        let (path, _) = self.coarse(at(from.node), goal_pos, road_nodes.as_ref(), &class)?;
        let mut poly = vec![at(from.node)];
        poly.extend(path.iter().map(|&n| lattice_at(n)));
        if let Some(g) = goal_pos {
            poly.push(g);
        } else {
            // The lattice node the join reached is the nearest one to a
            // node of the network, up to half a diagonal off it: end the
            // corridor on that node, so it holds its target.
            let last = *path.last().expect("a lattice route has a node");
            let centre = [last[0] * PER_LATTICE, last[1] * PER_LATTICE];
            let half = PER_LATTICE / 2;
            let mut best: Option<(f64, [i32; 2])> = None;
            for j in centre[1] - half..=centre[1] + half {
                for i in centre[0] - half..=centre[0] + half {
                    let n = [i, j];
                    if !self.is_target(&spec.goal, spec.from, n) {
                        continue;
                    }
                    let d = distance(at(n), lattice_at(last));
                    if best.is_none_or(|(b, bn)| d.total_cmp(&b).then(n.cmp(&bn)).is_lt()) {
                        best = Some((d, n));
                    }
                }
            }
            if let Some((_, n)) = best {
                poly.push(at(n));
            }
        }
        let toward = *poly.last().expect("a route has a start");
        // Where the lattice route crosses a river a culvert does not take,
        // a service road's corridor is wider: which bank it comes down, and
        // whether a side valley or a confluence lies across its way down,
        // is decided there, often a few hundred metres off the lattice's
        // line (the showcase's backbone dived into a creek's ravine for
        // want of the rim beside it). A track's is not: its measures did
        // not gain, and its searches are the most.
        let mut seen = Vec::new();
        let mut crossed = Vec::new();
        let wide: Vec<bool> = poly
            .windows(2)
            .map(|s| {
                spec.class == RoadClass::Service && {
                    self.channels.line_hits(s[0], s[1], &mut seen, &mut crossed);
                    crossed.iter().any(|&(w, _)| f64::from(w) >= CULVERT_MAX_M)
                }
            })
            .collect();
        for widen in [1.0, 2.2] {
            let mut local = self.corridor(&poly, &wide, class.corridor_m * widen, spec);
            let found = self.search(&mut local, toward, &class, &mut sampled);
            if let Some((nodes, crossings)) = found {
                return Some(Found {
                    nodes,
                    crossings,
                    sampled,
                });
            }
        }
        None
    }

    /// The lattice route from `from` to `goal`, or to the network's nearest
    /// lattice nodes, and its cost.
    #[allow(
        clippy::too_many_lines,
        reason = "the search, its moves and its water, in one loop"
    )]
    fn coarse(
        &self,
        from: [f64; 2],
        goal: Option<[f64; 2]>,
        nodes: Option<&NodeSet>,
        class: &Class,
    ) -> Option<(Vec<[i32; 2]>, f64)> {
        let nearest_open = |p: [f64; 2]| {
            let c = [super::nearest_node(p[0]), super::nearest_node(p[1])];
            let mut near: Vec<(f64, [i32; 2])> = (-1..=1)
                .flat_map(|j| (-1..=1).map(move |i| [c[0] + i, c[1] + j]))
                .filter(|&n| self.grid.open(n))
                .map(|n| (distance(lattice_at(n), p), n))
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            near.first().map(|x| x.1)
        };
        let start = nearest_open(from)?;
        let target = match goal {
            Some(g) => Some(nearest_open(g)?),
            None => None,
        };
        let set = nodes.unwrap_or(&self.coarse_on);
        let is_goal = |n: [i32; 2]| match target {
            Some(t) => n == t,
            None => set.contains(&n),
        };
        let heuristic = |n: [i32; 2]| match target {
            Some(t) => distance(lattice_at(n), lattice_at(t)) * COARSE_PULL,
            None => 0.0,
        };
        let most = target.map_or(f64::INFINITY, |t| {
            DETOUR_MOST * distance(lattice_at(start), lattice_at(t)) + 5_000.0
        });
        let mut g: NodeMap<f64> = NodeMap::default();
        let mut parent: NodeMap<[i32; 2]> = NodeMap::default();
        let mut closed = NodeSet::default();
        let mut heap = BinaryHeap::new();
        g.insert(start, 0.0);
        heap.push(Queued {
            f: heuristic(start),
            key: start,
        });
        let mut scratch = Vec::new();
        let mut lines = Vec::new();
        let mut expanded = 0;
        while let Some(Queued { key: n, .. }) = heap.pop() {
            if !closed.insert(n) {
                continue;
            }
            let gn = g[&n];
            if gn > most {
                return None;
            }
            if is_goal(n) {
                let mut path = vec![n];
                let mut at_node = n;
                while let Some(&p) = parent.get(&at_node) {
                    path.push(p);
                    at_node = p;
                }
                path.reverse();
                return Some((path, gn));
            }
            expanded += 1;
            if expanded > COARSE_MOST {
                return None;
            }
            let hn = self.grid.node_height(n);
            let pn = lattice_at(n);
            for (k, mv) in MOVES.iter().enumerate() {
                let m = [n[0] + mv[0], n[1] + mv[1]];
                // A target may lie on ground no road passes (a settlement's
                // own road), and is reached all the same.
                if !(self.grid.open(m) || is_goal(m)) || closed.contains(&m) {
                    continue;
                }
                if !straddled(*mv)
                    .iter()
                    .all(|d| d.is_none_or(|d| self.grid.open([n[0] + d[0], n[1] + d[1]])))
                {
                    continue;
                }
                let length = self.moves[k].0 * LATTICE_M;
                let dh = self.grid.node_height(m) - hn;
                let mut cost = class.travel(length, dh, 0.0);
                let pm = lattice_at(m);
                self.channels.line_hits(pn, pm, &mut scratch, &mut lines);
                for &(width, fall) in &lines {
                    let width = f64::from(width);
                    cost += if fall {
                        2_000.0
                    } else if width < CULVERT_MAX_M {
                        30.0
                    } else if let (Some(ford), true) = (class.ford_m, width < 6.0) {
                        ford
                    } else {
                        class.bridge_m + 25.0 * (width + 6.0)
                    };
                }
                let gm = gn + cost;
                if g.get(&m).is_none_or(|&held| gm < held) {
                    g.insert(m, gm);
                    parent.insert(m, n);
                    heap.push(Queued {
                        f: gm + heuristic(m),
                        key: m,
                    });
                }
            }
        }
        None
    }

    /// The corridor within `width` of the polyline, [`RIVER_WIDEN`] times
    /// that along its `wide` segments: a box of fine nodes sampled as the
    /// search first touches them, with the pads the route keeps off.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "fine node indices of a corridor's box"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "a box's indices and a segment's ends, as the geometry names them"
    )]
    fn corridor(&self, poly: &[[f64; 2]], wide: &[bool], width: f64, spec: &Spec) -> Local {
        let (mut x0, mut z0, mut x1, mut z1) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in poly {
            x0 = x0.min(p[0]);
            z0 = z0.min(p[1]);
            x1 = x1.max(p[0]);
            z1 = z1.max(p[1]);
        }
        let widest = if wide.contains(&true) {
            width * RIVER_WIDEN
        } else {
            width
        };
        let reach = widest + 2.0 * FINE_M;
        let i0 = ((x0 - reach) / FINE_M).floor() as i32;
        let j0 = ((z0 - reach) / FINE_M).floor() as i32;
        let i1 = ((x1 + reach) / FINE_M).ceil() as i32;
        let j1 = ((z1 + reach) / FINE_M).ceil() as i32;
        let (w, h) = (i1 - i0 + 1, j1 - j0 + 1);
        let cells = usize::try_from(w * h).expect("a corridor's box");
        let mut flags = vec![0_u8; cells];
        let segments: Vec<([f64; 2], [f64; 2], f64)> = if poly.len() == 1 {
            vec![(poly[0], poly[0], width)]
        } else {
            poly.windows(2)
                .zip(wide)
                .map(|(s, &w)| (s[0], s[1], if w { width * RIVER_WIDEN } else { width }))
                .collect()
        };
        // Which nodes are the corridor's: within `width` of the route.
        for (a, b, width) in segments {
            let si0 = ((a[0].min(b[0]) - width) / FINE_M).floor() as i32;
            let si1 = ((a[0].max(b[0]) + width) / FINE_M).ceil() as i32;
            let sj0 = ((a[1].min(b[1]) - width) / FINE_M).floor() as i32;
            let sj1 = ((a[1].max(b[1]) + width) / FINE_M).ceil() as i32;
            for j in sj0.max(j0)..=sj1.min(j1) {
                for i in si0.max(i0)..=si1.min(i1) {
                    let k = usize::try_from((j - j0) * w + (i - i0)).expect("inside the box");
                    if flags[k] & INNER != 0 {
                        continue;
                    }
                    let p = at([i, j]);
                    let (q, _) = project(p, a, b);
                    if distance(p, q) <= width {
                        flags[k] |= INNER;
                    }
                }
            }
        }
        let mut ids = Vec::new();
        self.site_grid.buckets.within(
            [
                f64::from(i0) * FINE_M,
                f64::from(j0) * FINE_M,
                f64::from(i1) * FINE_M,
                f64::from(j1) * FINE_M,
            ],
            &mut ids,
        );
        let pads = ids
            .iter()
            .map(|&s| {
                let site = &self.network.sites[s as usize];
                (site.centre, site.outer_m() + PAD_CLEAR_M)
            })
            .collect();
        Local {
            i0,
            j0,
            w,
            h,
            height: vec![0.0; cells],
            slope: vec![0.0; cells],
            texture: vec![1.0; cells],
            flags,
            pads,
            from: self.places[spec.from].node,
            goal: match spec.goal {
                Goal::Place(p) => Some(self.places[p].node),
                Goal::Network | Goal::Road(..) => None,
            },
            spec: *spec,
        }
    }

    /// Whether fine node `n` ends a route of `goal` from place `from`.
    fn is_target(&self, goal: &Goal, from: usize, n: [i32; 2]) -> bool {
        match *goal {
            Goal::Place(p) => n == self.places[p].node,
            Goal::Network => {
                n != self.places[from].node
                    && (self.on_road.contains_key(&n)
                        || self.anchors.get(&n).is_some_and(|&q| q as usize != from))
            }
            Goal::Road(r, p) => n == self.places[p].node || self.on_road.get(&n) == Some(&r),
        }
    }

    /// Node `k` of the corridor, sampled (or read from the cache) the first
    /// time it is asked for, its bits set: open, in a river's belt, near
    /// water, on a road, a target.
    fn touch(&self, local: &mut Local, k: usize, sampled: &mut Vec<([i32; 2], Fine)>) -> u8 {
        let flags = local.flags[k];
        if flags & SAMPLED != 0 {
            return flags;
        }
        let n = local.node(k);
        let f = self.fine(n, sampled);
        let mut flags = flags | f.flags | SAMPLED;
        let pos = at(n);
        let own = n == local.from || Some(n) == local.goal;
        if !own && local.pads.iter().any(|&(c, r)| distance(pos, c) < r) {
            flags &= !OPEN;
        }
        if self.on_road.contains_key(&n) {
            flags |= ON_ROAD;
        }
        if self.is_target(&local.spec.goal, local.spec.from, n) {
            flags |= TARGET | OPEN;
        }
        if n == local.from {
            flags |= OPEN;
        }
        local.height[k] = f.height;
        local.slope[k] = f.slope;
        #[allow(clippy::cast_possible_truncation, reason = "a weight at f32")]
        let weight = (1.0 + TEXTURE_SHARE * texture(self.seed, pos[0], pos[1])) as f32;
        local.texture[k] = weight;
        local.flags[k] = flags;
        flags
    }

    /// The least-cost path through the corridor, and its edges that cross
    /// a channel: A* towards the goal, or towards the lattice route's end
    /// for a join (which stops at the first node of the network it meets),
    /// so it touches a band round its path and not the whole corridor.
    #[allow(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "the search, its moves, costs and crossings, in one loop, over A*'s own names"
    )]
    fn search(
        &self,
        local: &mut Local,
        toward: [f64; 2],
        class: &Class,
        sampled: &mut Vec<([i32; 2], Fine)>,
    ) -> Option<(Vec<[i32; 2]>, Vec<usize>)> {
        let start = local.index(local.from)?;
        // Towards a place the straight line is a bound on new ground; a
        // join's lattice end is only near its target, so it pulls less.
        let pull = if local.goal.is_some() {
            PULL
        } else {
            0.75 * PULL
        };
        let heuristic = |local: &Local, k: usize| distance(at(local.node(k)), toward) * pull;
        let n = local.flags.len();
        let mut g = vec![f64::INFINITY; n];
        let mut parent = vec![u32::MAX; n];
        let mut via = vec![u8::MAX; n];
        let mut crossing = vec![false; n];
        let mut closed = vec![false; n];
        let mut heap = BinaryHeap::new();
        self.touch(local, start, sampled);
        g[start] = 0.0;
        // A node's entry is `(node, NOW)`; a crossing edge put off until its
        // least cost comes up is `(its far node, index into put_off)`.
        let mut put_off: Vec<(usize, usize)> = Vec::new();
        heap.push(Queued {
            f: heuristic(local, start),
            key: (start, NOW),
        });
        let mut scratch = Vec::new();
        let mut hits = Vec::new();
        let params = &self.network.params;
        while let Some(Queued {
            key: (popped, put), ..
        }) = heap.pop()
        {
            if closed[popped] {
                continue;
            }
            // A graded road's crossing edge is weighed in full (its banks
            // read off the ground) only once the least it could cost comes
            // up in the queue: most such edges the search makes never come
            // up, the far node reached otherwise first.
            let (k, only) = if put == NOW {
                (popped, None)
            } else {
                let (from, m) = put_off[put as usize];
                (from, Some(m))
            };
            if only.is_none() {
                closed[k] = true;
            }
            if only.is_none() && k != start && local.flags[k] & TARGET != 0 {
                let mut path = vec![k];
                while parent[path[path.len() - 1]] != u32::MAX {
                    path.push(parent[path[path.len() - 1]] as usize);
                }
                path.reverse();
                // Edge `e` runs from node `e` to `e + 1`, which recorded it.
                let crossings = (0..path.len() - 1)
                    .filter(|&e| crossing[path[e + 1]])
                    .collect();
                return Some((path.into_iter().map(|q| local.node(q)).collect(), crossings));
            }
            let node = local.node(k);
            let pa = at(node);
            let ha = f64::from(local.height[k]);
            let sa = f64::from(local.slope[k]);
            for (m, mv) in FINE_MOVES.iter().enumerate() {
                if only.is_some_and(|o| o != m) {
                    continue;
                }
                let next = [node[0] + mv[0], node[1] + mv[1]];
                let Some(j) = local.index(next) else { continue };
                if closed[j] || local.flags[j] & INNER == 0 {
                    continue;
                }
                // A knight's or a three-by-one move only over ground a step
                // has sampled: the steps sample a band a node either side of
                // the path, and the longer moves give the line its 24
                // bearings inside it without sampling a band twice as wide.
                let knight = m >= 8;
                if knight && local.flags[j] & SAMPLED == 0 {
                    continue;
                }
                let fj = self.touch(local, j, sampled);
                if fj & OPEN == 0 {
                    continue;
                }
                let mut through = true;
                for d in straddled(*mv).into_iter().flatten() {
                    let open = local
                        .index([node[0] + d[0], node[1] + d[1]])
                        .is_some_and(|s| {
                            (!knight || local.flags[s] & SAMPLED != 0)
                                && self.touch(local, s, sampled) & OPEN != 0
                        });
                    through &= open;
                }
                if !through {
                    continue;
                }
                let fk = local.flags[k];
                let (len, unit) = self.moves[m];
                let length = len * FINE_M;
                let hb = f64::from(local.height[j]);
                let dh = hb - ha;
                let mut cost = class.step(
                    length,
                    dh,
                    f64::midpoint(sa, f64::from(local.slope[j])),
                    f64::from(local.texture[k] + local.texture[j]),
                    (fk | fj) & IN_BELT != 0,
                );
                // The cosine of the turn onto this edge, if the path into
                // the node had an edge.
                let turn = (via[k] != u8::MAX).then(|| {
                    let before = self.moves[via[k] as usize].1;
                    before[0] * unit[0] + before[1] * unit[1]
                });
                if let Some(cos) = turn {
                    cost += class.turn_cost(cos, fk & NEAR_RIVER != 0);
                }
                if fk & fj & ON_ROAD != 0 {
                    cost *= DISCOUNT;
                }
                let mut earthwork = class.earthwork(length, dh);
                let mut crosses = false;
                if (fk | fj) & WET != 0 {
                    self.channels
                        .hits_into(pa, at(next), &mut scratch, &mut hits);
                    match hits.as_slice() {
                        [] => {}
                        [hit] => {
                            let (c, dir, bent) = self.channels.at(hit);
                            if c.flags & ON_FALL != 0 {
                                continue;
                            }
                            let square = (unit[0] * dir[1] - unit[1] * dir[0]).abs();
                            if square < LEAST_SQUARE {
                                continue;
                            }
                            let (width, depth) = (f64::from(c.width), f64::from(c.depth));
                            let skew = 1.0 / square;
                            let first = class.crossing(width, depth, c.flags);
                            if square < BRIDGE_LEAST_SQUARE && first == CrossingKind::Bridge {
                                continue;
                            }
                            // The least the crossing can add: an edge that
                            // could not better the node's cost even so is
                            // not read off the ground, which is most of
                            // the search's sampling, and changes nothing.
                            let least = match first {
                                CrossingKind::Culvert => 30.0 * skew,
                                CrossingKind::Ford => class.ford_m.unwrap_or(0.0) * skew,
                                // Its gap is the water's width and a metre
                                // either side at the least.
                                CrossingKind::Bridge => {
                                    Self::bridge_cost(class, width + 2.0, dh, bent, c.flags)
                                }
                            };
                            let into = hit.t * length;
                            let surface = f64::from(c.surface);
                            let lowest = match first {
                                CrossingKind::Culvert => surface + 1.0,
                                CrossingKind::Ford => surface - depth,
                                CrossingKind::Bridge => surface + class.freeboard_m,
                            };
                            let mut least_earth =
                                class.earthwork_least([into, length - into], [ha, hb], lowest);
                            if fk & fj & ON_ROAD != 0 {
                                least_earth *= DISCOUNT;
                            }
                            let bound = g[k] + ((cost + least) + least_earth);
                            if bound >= g[j] {
                                continue;
                            }
                            if class.graded && only.is_none() {
                                heap.push(Queued {
                                    f: bound + heuristic(local, j),
                                    key: (
                                        j,
                                        u32::try_from(put_off.len())
                                            .expect("under 4 billion edges"),
                                    ),
                                });
                                put_off.push((k, m));
                                continue;
                            }
                            // The crossing as the record will make it, read
                            // along the edge: a creek in a gorge is a bridge
                            // at its rim, every deck stands where the record
                            // stands it, and a bridge the kit has no class
                            // for, or no pier tall enough, is no way across.
                            // A trail is not graded: it has no deck to hold
                            // and no earthwork, and its footbridge is priced
                            // on the channel's own banks.
                            let (kind, deck, gap) = if class.graded {
                                let p = [pa[0] + unit[0] * into, pa[1] + unit[1] * into];
                                let ground_at = |s: f64| {
                                    f64::from(
                                        self.world
                                            .sample_by(
                                                p[0] + unit[0] * s,
                                                p[1] + unit[1] * s,
                                                self.network,
                                            )
                                            .height,
                                    )
                                };
                                let s = self.section(class, c, dir, unit, &ground_at, false);
                                (s.kind, s.deck, s.gap_m())
                            } else {
                                let kind = class.crossing(width, depth, c.flags);
                                let deck = match kind {
                                    CrossingKind::Bridge => surface + class.freeboard_m,
                                    CrossingKind::Culvert => surface + 1.0,
                                    CrossingKind::Ford => surface - depth,
                                };
                                let span = width + 2.0 * params.bank_top_m(width, depth);
                                (kind, deck, span * skew)
                            };
                            cost += match kind {
                                CrossingKind::Culvert => 30.0 * skew,
                                CrossingKind::Ford => class.ford_m.unwrap_or(0.0) * skew,
                                CrossingKind::Bridge => {
                                    if class.graded && !kit_stands(gap, deck, surface - depth) {
                                        continue;
                                    }
                                    // The gap is read along the edge, so a
                                    // skew deck is longer by its skew; its
                                    // abutments cost what a square one's do.
                                    Self::bridge_cost(class, gap, dh, bent, c.flags)
                                }
                            };
                            crosses = true;
                            // The road is held to the crossing's deck (or a
                            // ford's bed) over the water: what it cannot
                            // fall or climb to it from either node is cut or
                            // filled.
                            earthwork = class.earthwork(into, deck - ha)
                                + class.earthwork(length - into, hb - deck);
                        }
                        _ => continue,
                    }
                }
                // On a road laid earlier the cutting is made already: its
                // earthwork is discounted as its travel is, or a later road
                // would leave it on a slope and lay its own line beside it.
                if fk & fj & ON_ROAD != 0 {
                    earthwork *= DISCOUNT;
                }
                cost += earthwork;
                let gj = g[k] + cost;
                if gj < g[j] {
                    g[j] = gj;
                    parent[j] = u32::try_from(k).expect("a corridor under 4 billion nodes");
                    via[j] = u8::try_from(m).expect("one of 24 moves");
                    crossing[j] = crosses;
                    heap.push(Queued {
                        f: gj + heuristic(local, j),
                        key: (j, NOW),
                    });
                }
            }
        }
        None
    }

    /// What a bridge `gap` long costs the search on an edge rising `dh`:
    /// dearer on a bend and at a mouth.
    fn bridge_cost(class: &Class, gap: f64, dh: f64, bent: bool, flags: u8) -> f64 {
        let mut bridge = class.bridge_m + 25.0 * gap + 12.0 * dh.abs();
        if bent {
            bridge *= 2.0;
        }
        if flags & AT_MOUTH != 0 {
            bridge *= 2.0;
        }
        bridge
    }

    /// Merges a found route into the network: a join is cut where it first
    /// meets the network (a road laid by the same batch, or one it runs
    /// beside); a link is split into the stretches off the network.
    #[allow(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "a join's cut and a link's pieces, by their indices"
    )]
    fn merge(&mut self, spec: &Spec, found: Found) {
        for (n, f) in found.sampled {
            self.cache.put(n, f);
        }
        let nodes = found.nodes;
        match spec.goal {
            Goal::Network => {
                let mut end = nodes.len() - 1;
                let mut extra = None;
                let mut scratch = Vec::new();
                for (m, &n) in nodes.iter().enumerate().skip(1) {
                    if self.on_road.contains_key(&n) || self.anchors.contains_key(&n) {
                        end = m;
                        break;
                    }
                    // A road beside it: join it there.
                    let beside = super::mass::NEIGHBOURS
                        .iter()
                        .map(|d| [n[0] + d[0], n[1] + d[1]])
                        .filter(|r| self.on_road.contains_key(r))
                        .find(|&r| self.channels.hits(at(n), at(r), &mut scratch).is_empty());
                    if let Some(r) = beside {
                        end = m;
                        extra = Some(r);
                        break;
                    }
                }
                let mut path: Vec<[i32; 2]> = nodes[..=end].to_vec();
                if let Some(r) = extra {
                    path.push(r);
                }
                let last = *path.last().expect("a path has a node");
                let tail = if let Some(&q) = self.anchors.get(&last) {
                    RawEnd::Place(q as usize)
                } else if let Some(&r) = self.on_road.get(&last) {
                    RawEnd::Road(r, last)
                } else {
                    return;
                };
                let crossings: Vec<usize> = found
                    .crossings
                    .iter()
                    .copied()
                    .filter(|&e| e < end)
                    .collect();
                self.lay(
                    spec.class,
                    path,
                    [RawEnd::Place(spec.from), tail],
                    crossings,
                );
                let node = self.places[spec.from].node;
                self.anchors
                    .entry(node)
                    .or_insert(u32::try_from(spec.from).expect("a place index"));
            }
            // A link starts at a town that may have roads already, so it is
            // not cut where it first meets one: only its stretches off the
            // network are laid.
            Goal::Place(_) | Goal::Road(..) => {
                let on = |b: &Self, n: [i32; 2]| {
                    b.on_road.contains_key(&n) || b.anchors.contains_key(&n)
                };
                // Links are searched together, so two may lay one valley a
                // node apart: a node beside a road laid earlier is moved
                // onto it, where neither move crosses water, and the
                // stretch is then that road's.
                let (nodes, crossings) = {
                    let held: Vec<bool> = (0..nodes.len())
                        .map(|i| {
                            found.crossings.contains(&i)
                                || (i > 0 && found.crossings.contains(&(i - 1)))
                        })
                        .collect();
                    let mut snapped = nodes.clone();
                    let mut scratch = Vec::new();
                    for i in 1..snapped.len().saturating_sub(1) {
                        let n = snapped[i];
                        if held[i] || on(self, n) {
                            continue;
                        }
                        let (before, after) = (snapped[i - 1], snapped[i + 1]);
                        let beside = super::mass::NEIGHBOURS
                            .iter()
                            .map(|d| [n[0] + d[0], n[1] + d[1]])
                            .filter(|m| self.on_road.contains_key(m))
                            .find(|&m| {
                                self.channels
                                    .hits(at(before), at(m), &mut scratch)
                                    .is_empty()
                                    && self
                                        .channels
                                        .hits(at(m), at(after), &mut scratch)
                                        .is_empty()
                            });
                        if let Some(m) = beside {
                            snapped[i] = m;
                        }
                    }
                    // Two nodes moved onto one are one.
                    let mut out: Vec<[i32; 2]> = Vec::with_capacity(snapped.len());
                    let mut index = Vec::with_capacity(snapped.len());
                    for &n in &snapped {
                        if out.last() != Some(&n) {
                            out.push(n);
                        }
                        index.push(out.len() - 1);
                    }
                    let crossings: Vec<usize> = found.crossings.iter().map(|&c| index[c]).collect();
                    (out, crossings)
                };
                let flags: Vec<bool> = nodes.iter().map(|&n| on(self, n)).collect();
                let mut e = 0;
                while e + 1 < nodes.len() {
                    if flags[e] && flags[e + 1] {
                        e += 1;
                        continue;
                    }
                    let s = e;
                    while e + 1 < nodes.len() && !(flags[e] && flags[e + 1]) {
                        e += 1;
                    }
                    // Edges `s..e`, nodes `s..=e`.
                    let end_of = |b: &Self, n: [i32; 2]| {
                        if let Some(&q) = b.anchors.get(&n) {
                            Some(RawEnd::Place(q as usize))
                        } else {
                            b.on_road.get(&n).map(|&r| RawEnd::Road(r, n))
                        }
                    };
                    let (Some(a), Some(b)) = (end_of(self, nodes[s]), end_of(self, nodes[e]))
                    else {
                        continue;
                    };
                    // A corner of a road cut short reaches nothing.
                    let same_road =
                        matches!((a, b), (RawEnd::Road(x, _), RawEnd::Road(y, _)) if x == y);
                    if a == b || (same_road && e - s < 4) {
                        continue;
                    }
                    let crossings: Vec<usize> = crossings
                        .iter()
                        .filter(|&&c| c >= s && c < e)
                        .map(|&c| c - s)
                        .collect();
                    self.lay(spec.class, nodes[s..=e].to_vec(), [a, b], crossings);
                }
            }
        }
    }

    /// Adds a merged road, giving a place it reaches first its stub and
    /// ending any later road at a place on that place's first road.
    fn lay(
        &mut self,
        class: RoadClass,
        nodes: Vec<[i32; 2]>,
        ends: [RawEnd; 2],
        crossings: Vec<usize>,
    ) {
        let index = u32::try_from(self.roads.len()).expect("fewer than 4 billion roads");
        let mut stubs = [None, None];
        let mut ends = ends;
        for (side, end) in ends.iter_mut().enumerate() {
            let RawEnd::Place(p) = *end else { continue };
            let place = self.places[p];
            if place.settlement.is_some() {
                // A road head: every road from it starts on its point.
                stubs[side] = Some(place.stub);
                continue;
            }
            match self.first_road[p] {
                None => {
                    stubs[side] = Some(place.stub);
                    self.first_road[p] = Some(index);
                }
                Some(r) => *end = RawEnd::Road(r, place.node),
            }
        }
        self.add(RawRoad {
            class,
            nodes,
            fixed: None,
            ends,
            stubs,
            crossings,
        });
    }

    /// Shapes every road's line, eases those ending on another onto it, and
    /// records them.
    #[allow(
        clippy::too_many_lines,
        reason = "the shapes, the junctions, the records, the arms' holds and the profiles, in order"
    )]
    fn finish(self) -> Built {
        // The searched lines, then the wall search of every window on its
        // own (one long road may have several, and holds a thread), then
        // the shapes. A window's ways out run through the nodes other roads
        // end on, and keep off every road's searched line.
        let mut joined: Vec<Vec<[i32; 2]>> = vec![Vec::new(); self.roads.len()];
        for raw in &self.roads {
            for end in raw.ends {
                if let RawEnd::Road(through, node) = end {
                    joined[through as usize].push(node);
                }
            }
        }
        let lines = self.lines();
        let mut searched: Vec<Option<Searched>> = Vec::with_capacity(self.roads.len());
        let mut plans: Vec<(usize, Plan)> = Vec::new();
        for (r, found) in parallel_rows(self.roads.len(), self.threads, |r| {
            self.searched(r, &joined[r], &lines)
        })
        .into_iter()
        .enumerate()
        {
            if let Some((line, windows)) = found {
                plans.extend(windows.into_iter().map(|p| (r, p)));
                searched.push(Some(line));
            } else {
                searched.push(None);
            }
        }
        let laid = parallel_rows(plans.len(), self.threads, |k| self.wall(&plans[k].1));
        let mut walls_of: Vec<Vec<Wall>> = (0..self.roads.len()).map(|_| Vec::new()).collect();
        for ((r, _), wall) in plans.iter().zip(laid) {
            if let Some(wall) = wall {
                walls_of[*r].push(wall);
            }
        }
        let shaped: Vec<Vec<[f64; 2]>> = parallel_rows(self.roads.len(), self.threads, |r| {
            self.shape(&self.roads[r], searched[r].as_ref(), &walls_of[r])
        });
        let mut shaped = shaped;
        // Ends on other roads, onto their lines, in order: a road only ends
        // on one laid before it.
        let mut junctions: Vec<Junction> = Vec::new();
        let mut junction_of: NodeMap<Vec<(u32, u32)>> = NodeMap::default();
        let mut ends: Vec<[RoadEnd; 2]> = Vec::with_capacity(self.roads.len());
        for r in 0..self.roads.len() {
            let raw = &self.roads[r];
            let mut out = [RoadEnd::Settlement(0); 2];
            for (side, slot) in out.iter_mut().enumerate() {
                *slot = match raw.ends[side] {
                    RawEnd::Exit(s) => RoadEnd::Settlement(s),
                    RawEnd::Head(s) => RoadEnd::RoadHead(s),
                    RawEnd::Place(p) => {
                        let place = self.places[p];
                        if place.settlement.is_some() {
                            // The head's road, at its far end.
                            let head = self.first_road[p].expect("a settlement's road");
                            let length = polyline_length(&shaped[head as usize]);
                            let j = junction(
                                &mut junctions,
                                &mut junction_of,
                                head,
                                place.node,
                                place.stub,
                                length,
                            );
                            RoadEnd::Junction(j)
                        } else {
                            RoadEnd::Site(
                                self.network.sites[place.site.expect("a site's place")].id,
                            )
                        }
                    }
                    RawEnd::Road(through, node) => {
                        let end = if side == 0 {
                            shaped[r][0]
                        } else {
                            *shaped[r].last().expect("a shaped road has points")
                        };
                        let (q, along) = nearest_on(&shaped[through as usize], end);
                        self.ease_clear(&mut shaped[r], side, q);
                        let j = junction(&mut junctions, &mut junction_of, through, node, q, along);
                        RoadEnd::Junction(j)
                    }
                };
            }
            ends.push(out);
        }
        let mut roads: Vec<Road> = parallel_rows(self.roads.len(), self.threads, |r| {
            self.record(self.roads[r].class, &shaped[r], ends[r])
        });
        // The junctions' places along their roads' recorded lines.
        for j in &mut junctions {
            let points: Vec<[f64; 2]> = roads[j.road as usize]
                .points
                .iter()
                .map(|p| [p.x, p.z])
                .collect();
            let (q, along) = nearest_on(&points, j.at);
            j.at = q;
            j.along_m = along;
        }
        // A point where each arm's level run off its junction ends (see
        // `grade::junction_hold`), so its profile is level up to it.
        for r in 0..roads.len() {
            for side in 0..2 {
                let RoadEnd::Junction(j) = roads[r].ends[side] else {
                    continue;
                };
                let junction = junctions[j as usize];
                let (hold, _) = super::grade::junction_hold(
                    &roads[r],
                    side,
                    &roads[junction.road as usize],
                    junction.at,
                    junction.along_m,
                );
                let s = if side == 0 {
                    hold
                } else {
                    roads[r].length_m() - hold
                };
                let mut points = std::mem::take(&mut roads[r].points);
                self.insert_point(&mut points, s);
                roads[r].points = points;
            }
        }
        // The grade's profiles (ticket 0005, step 6), each road's after
        // those it ends on.
        let grading = strata_core::platform::time::Instant::now();
        let floor = |x: f64, z: f64| self.water_floor(x, z);
        super::grade::profile_all(
            &mut roads,
            &junctions,
            &self.network.sites,
            &floor,
            self.threads,
        );
        let grade_seconds = grading.elapsed().as_secs_f64();
        // A site no road reached, but a lone root the network grew from.
        let unreached = self.unplaced
            + (0..self.places.len())
                .filter(|&p| {
                    self.places[p].site.is_some()
                        && self.first_road[p].is_none()
                        && !self.roots.contains(&p)
                })
                .count();
        Built {
            roads,
            junctions,
            unreached,
            grade_seconds,
        }
    }

    /// A road's searched line, before the wall search and the shaping:
    /// its nodes and stubs, which of them are held and how freely each
    /// moves, which pieces cross a channel, and the windows round its
    /// bridges the wall search lays again (see `walls`), which stop at the
    /// nodes other roads end on (`joined`) and keep off the network's
    /// `lines`. `None` for a settlement's own road, which is kept as
    /// traced.
    fn searched(
        &self,
        r: usize,
        joined: &[[i32; 2]],
        lines: &Lines,
    ) -> Option<(Searched, Vec<Plan>)> {
        let raw = &self.roads[r];
        if raw.fixed.is_some() {
            return None;
        }
        let class = Class::of(raw.class);
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut held: Vec<bool> = Vec::new();
        // How freely each point moves: where the searched path climbs near
        // its class's limit it holds its line, so a switchback the search
        // laid up a slope is rounded and not ironed into the fall line.
        let mut free: Vec<f64> = Vec::new();
        if let Some(s) = raw.stubs[0] {
            points.push(s);
            held.push(true);
            free.push(1.0);
        }
        let offset = points.len();
        let height = |n: [i32; 2]| self.cache.get(n).map(|f| f64::from(f.height));
        let grade = |a: [i32; 2], b: [i32; 2]| match (height(a), height(b)) {
            (Some(ha), Some(hb)) => (hb - ha).abs() / distance(at(a), at(b)).max(1.0),
            _ => 0.0,
        };
        for (k, &n) in raw.nodes.iter().enumerate() {
            points.push(at(n));
            let crossing = raw.crossings.iter().any(|&e| e == k || e + 1 == k);
            held.push(k == 0 || k + 1 == raw.nodes.len() || crossing);
            let before = if k > 0 {
                grade(raw.nodes[k - 1], n)
            } else {
                0.0
            };
            let after = raw.nodes.get(k + 1).map_or(0.0, |&m| grade(n, m));
            let climb = before.max(after) / class.grade;
            free.push((1.0 - (climb - STIFF_FROM) / (1.0 - STIFF_FROM)).clamp(STIFF_LEAST, 1.0));
        }
        if let Some(s) = raw.stubs[1] {
            points.push(s);
            held.push(true);
            free.push(1.0);
        }
        let own = raw.ends.map(|end| match end {
            RawEnd::Place(p) => self.places[p].site,
            _ => None,
        });
        let crossing: Vec<bool> = (0..points.len().saturating_sub(1))
            .map(|k| k >= offset && raw.crossings.contains(&(k - offset)))
            .collect();
        let plans = if class.graded && !raw.nodes.is_empty() && points.len() >= 2 {
            let heights: Vec<Option<f64>> = (0..points.len())
                .map(|k| {
                    (k >= offset && k < offset + raw.nodes.len())
                        .then(|| height(raw.nodes[k - offset]))
                        .flatten()
                })
                .collect();
            let ends_on: Vec<bool> = (0..points.len())
                .map(|k| {
                    k >= offset
                        && k < offset + raw.nodes.len()
                        && joined.contains(&raw.nodes[k - offset])
                })
                .collect();
            let nodes = [offset, offset + raw.nodes.len() - 1];
            self.plans(
                r, raw.class, &points, &heights, &crossing, &ends_on, nodes, own, lines,
            )
        } else {
            Vec::new()
        };
        Some((
            Searched {
                points,
                held,
                free,
                crossing,
                own,
            },
            plans,
        ))
    }

    /// A road's line: its nodes and stubs resampled and smoothed, ends,
    /// stubs and crossings held, tightened where a bend is too sharp, and
    /// put back on its nodes where the curve came to cross water.
    #[allow(
        clippy::too_many_lines,
        reason = "the points, their freedom, the smoothing and the water check, in order"
    )]
    fn shape(&self, raw: &RawRoad, line: Option<&Searched>, walls: &[Wall]) -> Vec<[f64; 2]> {
        if let Some(fixed) = &raw.fixed {
            return fixed.clone();
        }
        let class = Class::of(raw.class);
        let Some(Searched {
            points,
            held,
            free,
            crossing,
            own,
            ..
        }) = line
        else {
            return Vec::new();
        };
        let (mut points, mut held, mut free, mut crossing, own) = (
            points.clone(),
            held.clone(),
            free.clone(),
            crossing.clone(),
            *own,
        );
        if points.len() < 2 {
            return points;
        }
        // Which points the wall search laid, and which crossings.
        let mut walled = vec![false; points.len()];
        // The straight the wall search held through each of its decks.
        let mut searched: Vec<Option<f64>> = vec![None; points.len() - 1];
        // From the last, so the earlier windows' indices hold.
        for w in walls.iter().rev() {
            let n = w.points.len();
            let rise = |i: usize, j: usize| {
                (w.heights[j] - w.heights[i]).abs() / distance(w.points[i], w.points[j]).max(1.0)
            };
            let laid_free: Vec<f64> = (0..n)
                .map(|i| {
                    if i == 0 {
                        free[w.from]
                    } else if i + 1 == n {
                        free[w.to]
                    } else {
                        let climb = rise(i - 1, i).max(rise(i, i + 1)) / class.grade;
                        (1.0 - (climb - STIFF_FROM) / (1.0 - STIFF_FROM)).clamp(STIFF_LEAST, 1.0)
                    }
                })
                .collect();
            let laid_held: Vec<bool> = (0..n)
                .map(|i| {
                    (i == 0 && held[w.from])
                        || (i + 1 == n && held[w.to])
                        || i == w.crossing
                        || i == w.crossing + 1
                })
                .collect();
            points.splice(w.from..=w.to, w.points.iter().copied());
            held.splice(w.from..=w.to, laid_held);
            free.splice(w.from..=w.to, laid_free);
            walled.splice(w.from..=w.to, vec![true; n]);
            crossing.splice(w.from..w.to, (0..n - 1).map(|i| i == w.crossing));
            searched.splice(
                w.from..w.to,
                (0..n - 1).map(|i| (i == w.crossing).then_some(w.straight_m)),
            );
        }
        let crosses = |k: usize| crossing[k];
        // A graded road's bridges: the line runs straight through each,
        // and the nodes of its edge are not held, so the road swings onto
        // the deck rather than turning at its abutments.
        let bridges = if class.graded {
            let mut bridges = self.bridges_of(raw.class, &points, &crosses);
            // A deck the wall search laid keeps the straight it laid, on
            // the points it laid there: the straightening lays them again
            // where they are.
            for b in &mut bridges {
                if let Some(straight_m) = searched[b.edge] {
                    b.searched = true;
                    b.half_m = straight_m;
                }
            }
            bridges
        } else {
            Vec::new()
        };
        for b in &bridges {
            for k in [b.edge, b.edge + 1] {
                let end = k == 0 || k + 1 == points.len();
                let other = (k > 0 && crosses(k - 1) && !bridges.iter().any(|o| o.edge + 1 == k))
                    || (k + 1 < points.len() && crosses(k) && !bridges.iter().any(|o| o.edge == k));
                held[k] = end || other;
            }
        }
        // Resampled, the held points kept; each piece of a crossing edge
        // may cross its channel once.
        let mut line: Vec<[f64; 2]> = Vec::new();
        let mut fixed: Vec<bool> = Vec::new();
        let mut weight: Vec<f64> = Vec::new();
        let mut allowed: Vec<bool> = Vec::new();
        // Whether each point lies on a stretch the wall search laid.
        let mut fine: Vec<bool> = Vec::new();
        // Where each of `points`' pieces starts in `line`.
        let mut begins: Vec<usize> = Vec::with_capacity(points.len());
        for k in 0..points.len() - 1 {
            begins.push(line.len());
            let (a, b) = (points[k], points[k + 1]);
            let parts = (distance(a, b) / SMOOTH_STEP_M).ceil().max(1.0);
            let mut i = 0.0;
            while i < parts {
                let t = i / parts;
                line.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                fixed.push(i == 0.0 && held[k]);
                weight.push(free[k] + (free[k + 1] - free[k]) * t);
                allowed.push(crosses(k));
                fine.push(walled[k] && walled[k + 1]);
                i += 1.0;
            }
        }
        line.push(*points.last().expect("two points"));
        fixed.push(true);
        weight.push(1.0);
        fine.push(false);
        // The least radius each point's bend is tightened to: wider within
        // reach of a bridge.
        let mut least = vec![class.min_radius_m; line.len()];
        // The held straight through each bridge, by its first and last
        // points.
        let mut straights: Vec<[usize; 2]> = Vec::new();
        if !bridges.is_empty() {
            for b in &bridges {
                // Measured afresh for each: a straight laid moves the points
                // at its ends, and the next is found along the line as it is.
                let mut arc = Vec::with_capacity(line.len());
                let mut total = 0.0;
                for k in 0..line.len() {
                    if k > 0 {
                        total += distance(line[k - 1], line[k]);
                    }
                    arc.push(total);
                }
                let s = arc[begins[b.edge]] + b.into_m;
                let edge = [
                    begins[b.edge],
                    begins.get(b.edge + 1).copied().unwrap_or(line.len() - 1),
                ];
                if let Some(range) = self.straighten(&mut line, &mut fixed, &arc, s, b, own, edge) {
                    straights.push(range);
                }
                for (k, r) in least.iter_mut().enumerate() {
                    if (arc[k] - s).abs() < APPROACH_REACH_M {
                        *r = class.min_radius_m * APPROACH_RADIUS;
                    }
                }
            }
        }
        let raw_line = line.clone();
        // The ground's gradient under each point, from the nodes the search
        // sampled round it: what a move of the point does to its height.
        let under: Vec<[f64; 2]> = raw_line
            .iter()
            .zip(&fine)
            .map(|(&p, &on_wall)| {
                // A wall's gradient changes within a fine node, so the
                // wall search's stretches read it off the ground at hand.
                if on_wall {
                    let h = |dx: f64, dz: f64| {
                        f64::from(
                            self.world
                                .sample_by(p[0] + dx, p[1] + dz, self.network)
                                .height,
                        )
                    };
                    let d = 0.5 * WALL_GRADIENT_M;
                    return [
                        (h(d, 0.0) - h(-d, 0.0)) / WALL_GRADIENT_M,
                        (h(0.0, d) - h(0.0, -d)) / WALL_GRADIENT_M,
                    ];
                }
                let n = fine_node(p);
                let h = |d: [i32; 2]| {
                    self.cache
                        .get([n[0] + d[0], n[1] + d[1]])
                        .map(|f| f64::from(f.height))
                };
                let diff = |lo: Option<f64>, mid: Option<f64>, hi: Option<f64>| match (lo, mid, hi)
                {
                    (Some(a), _, Some(b)) => (b - a) / (2.0 * FINE_M),
                    (Some(a), Some(m), None) => (m - a) / FINE_M,
                    (None, Some(m), Some(b)) => (b - m) / FINE_M,
                    _ => 0.0,
                };
                let mid = h([0, 0]);
                [
                    diff(h([-1, 0]), mid, h([1, 0])),
                    diff(h([0, -1]), mid, h([0, 1])),
                ]
            })
            .collect();
        let bound = Bound {
            raw: &raw_line,
            grads: &under,
            tolerance: HEIGHT_TOLERANCE_M,
        };
        smooth(&mut line, &fixed, &weight, &bound, class.iterations);
        // A bend too sharp may move further: a switchback's tip on a steep
        // slope rounds only by standing off its ground, which the grade's
        // cut and fill then make a platform of.
        let loose = Bound {
            tolerance: TIGHTEN_TOLERANCE_M,
            ..bound
        };
        tighten(&mut line, &fixed, &weight, &loose, &least);
        for &[first, last] in &straights {
            self.blend_onto(&mut line, &fixed, last, true, class.min_radius_m, own);
            self.blend_onto(&mut line, &fixed, first, false, class.min_radius_m, own);
        }
        // Where the curve crosses a channel the nodes did not, or crosses
        // one at a fall or too skew, or comes onto ground the search kept
        // it off (another site's pad, a lake, a settlement's margin), back
        // onto the nodes round it, wider each round, then whole.
        let was_blocked: Vec<bool> = raw_line.iter().map(|&p| self.blocked(p, own)).collect();
        let mut scratch = Vec::new();
        for round in 0..5 {
            let bad: Vec<usize> = (0..line.len() - 1)
                .filter(|&k| {
                    if !was_blocked[k]
                        && moved(line[k], raw_line[k])
                        && (self.blocked(line[k], own) || self.in_river(line[k], line[k + 1]))
                    {
                        return true;
                    }
                    let hits = self.channels.hits(line[k], line[k + 1], &mut scratch);
                    match hits.as_slice() {
                        [] => false,
                        [hit] if allowed[k] => {
                            let (c, dir, _) = self.channels.at(hit);
                            let d = [line[k + 1][0] - line[k][0], line[k + 1][1] - line[k][1]];
                            let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-9);
                            let square = ((d[0] * dir[1] - d[1] * dir[0]) / l).abs();
                            let least =
                                if class.crossing(f64::from(c.width), f64::from(c.depth), c.flags)
                                    == CrossingKind::Bridge
                                {
                                    BRIDGE_LEAST_SQUARE
                                } else {
                                    LEAST_SQUARE
                                };
                            c.flags & ON_FALL != 0 || square < least
                        }
                        _ => true,
                    }
                })
                .collect();
            if bad.is_empty() {
                break;
            }
            if round == 4 {
                return raw_line;
            }
            for k in bad {
                let lo = k.saturating_sub(8 + 4 * round);
                let hi = (k + 9 + 4 * round).min(line.len() - 1);
                line[lo..=hi].copy_from_slice(&raw_line[lo..=hi]);
            }
        }
        line
    }

    /// Whether the line from `a` to `b` runs under a river's water: the
    /// smoothing may slide a line into the water round a creek's head, past
    /// the end of the line the crossings are found on, where no crossing is
    /// recorded. Walked every [`RIVER_STEP_M`], and only near a channel.
    #[allow(
        clippy::many_single_char_names,
        reason = "a stretch's ends, a step and its share, as the geometry names them"
    )]
    fn in_river(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let params = &self.network.params;
        let length = distance(a, b);
        let mid = [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1])];
        let near = self
            .network
            .index
            .nearest(params, mid[0], mid[1])
            .is_some_and(|water| {
                let reach = 0.5 * f64::from(water.width_m)
                    + params.bend_amplitude(f64::from(water.discharge_m3s))
                    + RIVER_NEAR_M
                    + 0.5 * length;
                water.kind != WaterKind::Lake && f64::from(water.signed_distance_m).abs() < reach
            });
        if !near {
            return false;
        }
        let steps = (length / RIVER_STEP_M).ceil().max(1.0);
        let mut i = 0.0;
        while i <= steps {
            let t = i / steps;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            if self
                .world
                .sample_by(p[0], p[1], self.network)
                .standing_water()
                .is_some()
            {
                return true;
            }
            i += 1.0;
        }
        false
    }

    /// The bridges a road's searched edges cross: each crossing edge of
    /// `points` whose channel the class bridges.
    fn bridges_of(
        &self,
        class: RoadClass,
        points: &[[f64; 2]],
        crosses: &dyn Fn(usize) -> bool,
    ) -> Vec<Straight> {
        let way = super::grade::way(class);
        let class = Class::of(class);
        let mut scratch = Vec::new();
        (0..points.len() - 1)
            .filter(|&k| crosses(k))
            .filter_map(|k| {
                let (a, b) = (points[k], points[k + 1]);
                let hits = self.channels.hits(a, b, &mut scratch);
                let [hit] = hits.as_slice() else {
                    return None;
                };
                let (c, dir, _) = self.channels.at(hit);
                let (width, depth) = (f64::from(c.width), f64::from(c.depth));
                if class.crossing(width, depth, c.flags) != CrossingKind::Bridge {
                    return None;
                }
                let l = distance(a, b).max(1e-9);
                let unit = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
                let square = (unit[0] * dir[1] - unit[1] * dir[0])
                    .abs()
                    .max(BRIDGE_LEAST_SQUARE);
                let span = width + 2.0 * self.network.params.bank_top_m(width, depth);
                Some(Straight {
                    edge: k,
                    into_m: hit.t * l,
                    at: [a[0] + (b[0] - a[0]) * hit.t, a[1] + (b[1] - a[1]) * hit.t],
                    unit,
                    normal: [dir[1], -dir[0]],
                    half_m: 0.5 * span / SKEW_COS.min(square) + way.shelf_m + STRAIGHT_PAST_M,
                    gap_m: 0.5 * span / square,
                    searched: false,
                })
            })
            .collect()
    }

    /// Lays `line`'s points within the bridge `b`'s straight half-length of
    /// its crossing (`s` metres along the line, `arc` each point's) on a
    /// straight line through the crossing, and holds them. The straight runs
    /// along the line's own way through the valley (between where it comes
    /// from and where it goes [`TURN_REACH_M`] either side), turned no
    /// further off square to the channel than [`SKEW_COS`] allows, so a
    /// road running along its valley crosses on a skew deck rather than a
    /// dog-leg; else along the searched edge. Shorter, to half of it, where
    /// the straight would cross another channel (a confluence), come onto
    /// ground the search kept off, or stand a point off the deck more than
    /// [`STRAIGHT_TOLERANCE_M`] off its ground; it never reaches within
    /// [`STRAIGHT_CLEAR_M`] of a point already held, and the step from each
    /// of its ends back onto the line crosses no channel and no water, so
    /// the line with its straights is one the shape may fall back on whole.
    /// Where none fits, the points of the searched edge itself (`edge`, the
    /// first and last of them) are held.
    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "the line, its holds and lengths, the crossing's place, the bridge, the road's own sites and its edge; the way, the candidates and their checks, in order"
    )]
    fn straighten(
        &self,
        line: &mut [[f64; 2]],
        fixed: &mut [bool],
        arc: &[f64],
        s: f64,
        b: &Straight,
        own: [Option<usize>; 2],
        edge: [usize; 2],
    ) -> Option<[usize; 2]> {
        let n = line.len();
        let total = arc[n - 1];
        let point_at = |a: f64| {
            let a = a.clamp(0.0, total);
            let k = arc.partition_point(|&x| x <= a).clamp(1, n - 1);
            let (p, q) = (line[k - 1], line[k]);
            let l = (arc[k] - arc[k - 1]).max(1e-9);
            let t = ((a - arc[k - 1]) / l).clamp(0.0, 1.0);
            [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
        };
        let unit = |v: [f64; 2]| {
            let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
            (l > 1e-9).then(|| [v[0] / l, v[1] / l])
        };
        // The way the line goes through the valley, turned toward square.
        let from = point_at(s - TURN_REACH_M);
        let to = point_at(s + TURN_REACH_M);
        let way = match (
            unit([b.at[0] - from[0], b.at[1] - from[1]]),
            unit([to[0] - b.at[0], to[1] - b.at[1]]),
        ) {
            (Some(i), Some(o)) => unit([i[0] + o[0], i[1] + o[1]]),
            _ => None,
        };
        // The water's own flow there, which the kit's piers and the channel's
        // banks follow through a meander's bend more closely than the
        // indexed centreline's chord.
        let flow = self.flow_at(b.at);
        let turned = way.map(|w| {
            let flip = if b.normal[0] * w[0] + b.normal[1] * w[1] < 0.0 {
                -1.0
            } else {
                1.0
            };
            let nrm = [b.normal[0] * flip, b.normal[1] * flip];
            let cos = nrm[0] * w[0] + nrm[1] * w[1];
            if cos >= SKEW_COS {
                w
            } else {
                // As far toward the way as the skew allows.
                let side = [w[0] - nrm[0] * cos, w[1] - nrm[1] * cos];
                let side = unit(side).unwrap_or([nrm[1], -nrm[0]]);
                let sin = (1.0 - SKEW_COS * SKEW_COS).sqrt();
                [
                    nrm[0] * SKEW_COS + side[0] * sin,
                    nrm[1] * SKEW_COS + side[1] * sin,
                ]
            }
        });
        // How far the straight may run each way before it comes within
        // reach of a point already held; the line's ends are held.
        let held_back = (0..n)
            .filter(|&k| fixed[k] && arc[k] <= s)
            .map(|k| arc[k])
            .fold(f64::NEG_INFINITY, f64::max);
        let held_on = (0..n)
            .filter(|&k| fixed[k] && arc[k] > s)
            .map(|k| arc[k])
            .fold(f64::INFINITY, f64::min);
        let room = [
            s - held_back - STRAIGHT_CLEAR_M,
            held_on - s - STRAIGHT_CLEAR_M,
        ];
        // The ground under the line as it is, off the deck, sampled once.
        let height = |p: [f64; 2]| f64::from(self.world.sample_by(p[0], p[1], self.network).height);
        let was: Vec<Option<f64>> = (0..n)
            .map(|k| {
                let d = (arc[k] - s).abs();
                (k > 0 && k + 1 < n && d <= b.half_m && d > b.gap_m).then(|| height(line[k]))
            })
            .collect();
        let mut scratch = Vec::new();
        let turned = turned.filter(|&d| !b.searched && square_to_flow(flow, d));
        for dir in turned.into_iter().chain(std::iter::once(b.unit)) {
            // Along the road's own direction of travel.
            let dir = if dir[0] * b.unit[0] + dir[1] * b.unit[1] < 0.0 {
                [-dir[0], -dir[1]]
            } else {
                dir
            };
            // The last lengths tried: the shares only shrink them, so one
            // no shorter is the same straight again.
            let mut tried: Option<[f64; 2]> = None;
            for share in [1.0, 0.75, 0.5] {
                let back = (b.half_m * share).min(room[0]);
                let on = (b.half_m * share).min(room[1]);
                if back < 0.5 * b.half_m
                    || on < 0.5 * b.half_m
                    || tried.is_some_and(|t| back >= t[0] && on >= t[1])
                {
                    continue;
                }
                tried = Some([back, on]);
                let inside: Vec<usize> = (1..n - 1)
                    .filter(|&k| arc[k] - s >= -back && arc[k] - s <= on)
                    .collect();
                let laid: Vec<[f64; 2]> = inside
                    .iter()
                    .map(|&k| {
                        let d = arc[k] - s;
                        [b.at[0] + dir[0] * d, b.at[1] + dir[1] * d]
                    })
                    .collect();
                let (Some(&lo), Some(&hi)) = (inside.first(), inside.last()) else {
                    continue;
                };
                let (first, last) = (laid[0], laid[laid.len() - 1]);
                // The steps back onto the line at either end: the shape's
                // checks take the line with its straights as their own, so
                // a step that crossed a channel or ran in water would stand.
                let steps = [(line[lo - 1], first), (last, line[hi + 1])];
                let clear = self.channels.hits(first, last, &mut scratch).len() == 1
                    && inside
                        .iter()
                        .zip(&laid)
                        .all(|(&k, &p)| !moved(p, line[k]) || !self.blocked(p, own))
                    && steps.iter().all(|&(p, q)| {
                        self.channels.hits(p, q, &mut scratch).is_empty() && !self.in_river(p, q)
                    })
                    && inside.iter().zip(&laid).all(|(&k, &p)| {
                        was[k].is_none_or(|h| {
                            !moved(p, line[k]) || (height(p) - h).abs() <= STRAIGHT_TOLERANCE_M
                        })
                    });
                if clear {
                    for (&k, &p) in inside.iter().zip(&laid) {
                        line[k] = p;
                        fixed[k] = true;
                    }
                    return Some([lo, hi]);
                }
            }
        }
        let [a, z] = edge;
        for f in &mut fixed[a..=z] {
            *f = true;
        }
        (a > 0 && z + 1 < n).then_some([a, z])
    }

    /// Swings `line` onto the straight it runs through a bridge on, from
    /// the straight's end at `from` (an index of `line`) away in `step`'s
    /// direction: the points within the longest of [`BLEND_M`], two thirds
    /// and two fifths of it are laid on a cubic tangent to the straight at
    /// its end and to the line where the blend ends, so the road comes onto
    /// the deck on a curve of a few times its class's radius rather than
    /// turning off the searched line at the straight's end. A blend is laid
    /// only where none of its points is held, its curve is nowhere tighter
    /// than the class's radius, crosses no channel, lays no point in a
    /// river's water or on ground the search kept off, and moves no point to
    /// ground more than [`BLEND_TOLERANCE_M`] higher or lower than its own.
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "the line, its holds, where the blend starts and which way, the class and the road's own sites; the blend's lengths, its curve and its checks, in order"
    )]
    fn blend_onto(
        &self,
        line: &mut [[f64; 2]],
        fixed: &[bool],
        from: usize,
        forward: bool,
        least_m: f64,
        own: [Option<usize>; 2],
    ) {
        let n = line.len();
        let next = |k: usize| if forward { k + 1 } else { k.wrapping_sub(1) };
        let inside = |k: usize| k < n;
        let back = if forward {
            from.wrapping_sub(1)
        } else {
            from + 1
        };
        if !inside(back) || !inside(next(from)) {
            return;
        }
        // The straight's direction away from the deck.
        let t0 = {
            let (a, b) = (line[back], line[from]);
            let l = distance(a, b).max(1e-9);
            [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
        };
        let p0 = line[from];
        let mut scratch = Vec::new();
        for share in [5.0 / 3.0, 4.0 / 3.0, 1.0, 2.0 / 3.0, 0.4] {
            let blend = BLEND_M * share;
            // The points the blend lays, and where it ends.
            let mut ks = Vec::new();
            let mut along = vec![0.0];
            let mut k = from;
            let mut total = 0.0;
            let mut end = None;
            while inside(next(k)) {
                let j = next(k);
                total += distance(line[k], line[j]);
                if total >= blend {
                    end = Some(j);
                    break;
                }
                ks.push(j);
                along.push(total);
                k = j;
            }
            let Some(end) = end else { continue };
            if ks.is_empty() || ks.iter().any(|&k| fixed[k]) || !inside(next(end)) {
                continue;
            }
            let p1 = line[end];
            let t1 = {
                let (a, b) = (line[ks[ks.len() - 1]], line[next(end)]);
                let l = distance(a, b).max(1e-9);
                [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
            };
            let m = distance(p0, p1);
            let laid: Vec<[f64; 2]> = along[1..]
                .iter()
                .map(|&a| {
                    let t = a / total;
                    let (t2, t3) = (t * t, t * t * t);
                    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                    let h10 = t3 - 2.0 * t2 + t;
                    let h01 = -2.0 * t3 + 3.0 * t2;
                    let h11 = t3 - t2;
                    [
                        h00 * p0[0] + h10 * m * t0[0] + h01 * p1[0] + h11 * m * t1[0],
                        h00 * p0[1] + h10 * m * t0[1] + h01 * p1[1] + h11 * m * t1[1],
                    ]
                })
                .collect();
            let mut path = vec![p0];
            path.extend(laid.iter().copied());
            path.push(p1);
            let before: Vec<[f64; 2]> = std::iter::once(p0)
                .chain(ks.iter().map(|&k| line[k]))
                .chain(std::iter::once(p1))
                .collect();
            let before_n = before.len();
            let crossings_before: usize = (0..before_n - 1)
                .map(|i| {
                    self.channels
                        .hits(before[i], before[i + 1], &mut scratch)
                        .len()
                })
                .sum();
            let crossings_after: usize = (0..path.len() - 1)
                .map(|i| self.channels.hits(path[i], path[i + 1], &mut scratch).len())
                .sum();
            let tight =
                (1..path.len() - 1).any(|i| radius(path[i - 1], path[i], path[i + 1]) < least_m);
            if crossings_before != 0 || crossings_after != 0 {
                continue;
            }
            if tight {
                continue;
            }
            // The ground under a point, and whether it is under a river's
            // water: a blend may slant along a creek without crossing its
            // line, and must not run in it.
            let ground = |p: [f64; 2]| {
                let s = self.world.sample_by(p[0], p[1], self.network);
                (f64::from(s.height), s.standing_water().is_some())
            };
            let clear = ks.iter().zip(&laid).all(|(&k, &p)| {
                let ((now, wet), (was, _)) = (ground(p), ground(line[k]));
                !wet && !self.blocked(p, own) && (now - was).abs() <= BLEND_TOLERANCE_M
            });
            if clear {
                for (&k, &p) in ks.iter().zip(&laid) {
                    line[k] = p;
                }
                return;
            }
        }
    }

    /// A road's record from its line: the line every
    /// [`POINT_STEP_M`] with the ground there, and every crossing of a
    /// channel along it.
    fn record(&self, class: RoadClass, line: &[[f64; 2]], ends: [RoadEnd; 2]) -> Road {
        // The step at each vertex: a chord of the bend there that stands
        // [`POINT_SAG_M`] off the curve, at most [`POINT_STEP_M`].
        let steps: Vec<f64> = (0..line.len())
            .map(|k| {
                let r = if k == 0 || k + 1 == line.len() {
                    f64::INFINITY
                } else {
                    radius(line[k - 1], line[k], line[k + 1])
                };
                (8.0 * r * POINT_SAG_M)
                    .sqrt()
                    .clamp(POINT_LEAST_M, POINT_STEP_M)
            })
            .collect();
        let mut points = Vec::new();
        let mut along = 0.0;
        let mut next = 0.0;
        for k in 0..line.len().saturating_sub(1) {
            let (a, b) = (line[k], line[k + 1]);
            let l = distance(a, b);
            let step = steps[k].min(steps[k + 1]);
            while next <= along + l {
                let t = if l > 0.0 { (next - along) / l } else { 0.0 };
                points.push(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], next));
                next += step;
            }
            along += l;
        }
        if let Some(&last) = line.last()
            && points.last().is_none_or(|p| along - p.1 > 1e-6)
        {
            points.push((last, along));
        }
        let points: Vec<RoadPoint> = points
            .into_iter()
            .map(|(p, a)| RoadPoint {
                x: p[0],
                z: p[1],
                along_m: a,
                ground_m: self.ground(p),
            })
            .collect();
        let class_of = Class::of(class);
        let mut cumulative = Vec::with_capacity(line.len());
        let mut total = 0.0;
        for k in 0..line.len() {
            if k > 0 {
                total += distance(line[k - 1], line[k]);
            }
            cumulative.push(total);
        }
        let line_at = |s: f64| point_along(line, &cumulative, s);
        let mut crossings = Vec::new();
        let mut scratch = Vec::new();
        let mut along = 0.0;
        for k in 0..line.len().saturating_sub(1) {
            let (a, b) = (line[k], line[k + 1]);
            let l = distance(a, b);
            for hit in self.channels.hits(a, b, &mut scratch) {
                let (c, dir, _) = self.channels.at(&hit);
                let bearing = if l > 0.0 {
                    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
                } else {
                    [1.0, 0.0]
                };
                crossings.push(self.crossing(
                    &class_of,
                    c,
                    dir,
                    bearing,
                    along + l * hit.t,
                    &line_at,
                ));
            }
            along += l;
        }
        // A point at each end of every stretch the grade holds level at a
        // crossing's deck (ticket 0005, step 6), so the profile between two
        // is level and not a cubic towards the next point 20 m on.
        let way = super::grade::way(class);
        let mut points = points;
        for c in &crossings {
            for s in super::grade::held(&way, c) {
                self.insert_point(&mut points, s);
            }
        }
        Road {
            class,
            points,
            ends,
            crossings,
            profile: Vec::new(),
        }
    }

    /// Eases the end `side` of `line` onto `q` (see [`ease`]) over the
    /// longest of [`JOIN_BLEND_M`], a half and a quarter of it whose eased
    /// stretch meets no channel the line did not already, crosses none at a
    /// fall, and comes onto no ground [`Self::blocked`] refuses; over the
    /// shortest if none is clear, the shift being least there.
    fn ease_clear(&self, line: &mut [[f64; 2]], side: usize, q: [f64; 2]) {
        let before = line.to_vec();
        let n = line.len();
        let mut scratch = Vec::new();
        let hits = |a: [f64; 2], b: [f64; 2], scratch: &mut Vec<u32>| {
            self.channels
                .hits(a, b, scratch)
                .iter()
                .map(|hit| self.channels.at(hit).0.flags)
                .collect::<Vec<u8>>()
        };
        for share in [1.0, 0.5, 0.25] {
            line.copy_from_slice(&before);
            let blend = JOIN_BLEND_M * share;
            ease(line, side, q, blend);
            let clear = (0..n.saturating_sub(1)).all(|k| {
                if !moved(line[k], before[k]) && !moved(line[k + 1], before[k + 1]) {
                    return true;
                }
                let was = hits(before[k], before[k + 1], &mut scratch);
                let now = hits(line[k], line[k + 1], &mut scratch);
                now.len() <= was.len()
                    && now.iter().all(|f| f & ON_FALL == 0)
                    && (self.blocked(before[k], [None; 2]) || !self.blocked(line[k], [None; 2]))
            });
            if clear {
                return;
            }
        }
    }

    /// A point `s` metres along `points` on the chord between its
    /// neighbours, with the ground there, unless one stands within half a
    /// metre: where the grade holds a stretch level, so the cubic between
    /// points is level up to its end.
    fn insert_point(&self, points: &mut Vec<RoadPoint>, s: f64) {
        let at = points.partition_point(|p| p.along_m < s);
        if at == 0 || at >= points.len() {
            return;
        }
        let (before, after) = (points[at - 1], points[at]);
        if s - before.along_m < 0.5 || after.along_m - s < 0.5 {
            return;
        }
        let share = (s - before.along_m) / (after.along_m - before.along_m);
        let here = [
            before.x + (after.x - before.x) * share,
            before.z + (after.z - before.z) * share,
        ];
        points.insert(
            at,
            RoadPoint {
                x: here[0],
                z: here[1],
                along_m: s,
                ground_m: self.ground(here),
            },
        );
    }

    /// The lowest a road's profile may run at a point (ticket 0005, step
    /// 6): a little over the water of a lake whose record reaches the point
    /// or of a river within [`FLOOR_RIVER_M`] of its banks, so a road
    /// beside them never runs, nor cuts its banks, under their level. A
    /// crossing's own deck or bed is held apart from this.
    fn water_floor(&self, x: f64, z: f64) -> Option<f64> {
        // Anywhere a lake's bed reaches, as the carve reads it, its water
        // stands on ground under its level.
        let lake = self
            .network
            .shore
            .lake_here(&self.network.lakes, x, z)
            .map(|here| here.level + FLOOR_OVER_M);
        let river = self
            .network
            .index
            .nearest(&self.network.params, x, z)
            .filter(|water| water.kind != WaterKind::Lake)
            .and_then(|water| {
                let bank = self
                    .network
                    .params
                    .bank_top_m(f64::from(water.width_m), f64::from(water.depth_m));
                (f64::from(water.signed_distance_m).abs() < bank + FLOOR_RIVER_M)
                    .then_some(f64::from(water.surface_m) + FLOOR_OVER_M)
            });
        match (lake, river) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// The ground the world samples at a point: carved, the pad laid.
    fn ground(&self, p: [f64; 2]) -> f32 {
        let s = self.world.sample_by(p[0], p[1], self.network);
        let mut h = f64::from(s.height);
        let mut ids = Vec::new();
        self.site_grid
            .buckets
            .within([p[0], p[1], p[0], p[1]], &mut ids);
        if let Some(shaped) = ids.iter().find_map(|&k| {
            self.network.sites[k as usize]
                .pad()
                .shape(p[0], p[1], h, (0.0, 0.0))
        }) {
            h = shaped.height;
        }
        #[allow(clippy::cast_possible_truncation, reason = "a height at f32")]
        let h = h as f32;
        h
    }

    /// How far from the crossing `along` metres along a road, on its `side`
    /// (-1 back, 1 on), a bridge's abutment stands, from `from` metres out:
    /// further, a step at a time and at most [`SHELF_PUSH_MOST_M`], while
    /// any corner of the level shelf behind it (the class's carriageway and
    /// verge across, its shelf deep, square to the road's `bearing`) stands
    /// in a river's channel. The shelf is square to the road and the
    /// channel's bank is not: skew, a corner nearer the water than the
    /// centreline is, and round a meander the creek may curl back under one
    /// (`probe_skew`).
    fn dry_shelf(
        &self,
        class: &Class,
        along: f64,
        side: f64,
        from: f64,
        bearing: [f64; 2],
        line_at: &dyn Fn(f64) -> [f64; 2],
    ) -> f64 {
        let across = [-bearing[1], bearing[0]];
        let wet = |p: [f64; 2]| {
            self.world
                .sample_by(p[0], p[1], self.network)
                .water
                .is_some_and(|w| {
                    w.kind != WaterKind::Lake && w.signed_distance_m.abs() < 0.5 * w.width_m
                })
        };
        let dry = |off: f64| {
            [0.0, 0.5, 1.0].iter().all(|&f| {
                let c = line_at(along + side * (off + f * class.shelf_m));
                [-class.corner_m, class.corner_m]
                    .iter()
                    .all(|&l| !wet([c[0] + across[0] * l, c[1] + across[1] * l]))
            })
        };
        let mut off = from;
        while off < from + SHELF_PUSH_MOST_M && !dry(off) {
            off += SHELF_STEP_M;
        }
        off
    }

    /// The record of a crossing of the channel at `c`, whose indexed line
    /// runs along `dir` there, on a road running along `bearing`, `along`
    /// metres from its start.
    #[allow(
        clippy::too_many_lines,
        reason = "the banks, the kind, and the deck and abutments by kind, in order"
    )]
    fn crossing(
        &self,
        class: &Class,
        c: ChannelPoint,
        dir: [f64; 2],
        bearing: [f64; 2],
        along: f64,
        line_at: &dyn Fn(f64) -> [f64; 2],
    ) -> Crossing {
        let (width, depth, surface) =
            (f64::from(c.width), f64::from(c.depth), f64::from(c.surface));
        // The ground `s` metres along the road from the crossing, on the
        // road's own line: a straight line out along the crossing's bearing
        // leaves a road that bends off it, and read a gorge's wall where the
        // road came along a terrace below it (a deck 30 m over the road).
        let ground_at = |s: f64| {
            let p = line_at(along + s);
            f64::from(self.world.sample_by(p[0], p[1], self.network).height)
        };
        let s = self.section(class, c, dir, bearing, &ground_at, true);
        let (deck, span, abutments) = match s.kind {
            CrossingKind::Ford => (s.deck, width, [along - 0.5 * width, along + 0.5 * width]),
            CrossingKind::Culvert | CrossingKind::Bridge => {
                let [lo, hi] = s.out_m;
                let least = s.least_m;
                let (lo_at, hi_at) = if s.kind == CrossingKind::Bridge {
                    (
                        self.dry_shelf(class, along, -1.0, lo.max(least), bearing, line_at),
                        self.dry_shelf(class, along, 1.0, hi.max(least), bearing, line_at),
                    )
                } else {
                    (lo.max(least), hi.max(least))
                };
                (
                    s.deck,
                    (lo + hi).max(2.0 * least),
                    [along - lo_at, along + hi_at],
                )
            }
        };
        Crossing {
            kind: s.kind,
            at: [c.x, c.z],
            along_m: along,
            bearing,
            span_m: span,
            deck_m: deck,
            surface_m: surface,
            banks_m: s.banks,
            width_m: width,
            depth_m: depth,
            river: c.reach,
            abutments_m: abutments,
        }
    }

    /// What a crossing of the channel at `c` (its indexed line running
    /// along `dir`) by a road running along `bearing` is, the ground
    /// `ground_at(s)` metres along the road from it: its kind, banks, deck
    /// and how far out each way the ground stays under the deck. One rule
    /// for the record and the search, so the search never prices a dive to
    /// a culvert where the record makes a bridge at the rim, nor a crossing
    /// the kit has nothing to stand at. The walks out from the channel are
    /// left out of a culvert unless `culvert_out`: the search needs only
    /// its deck.
    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_lines,
        reason = "the channel point, the side and the steps, as the walks name them; the kind, the banks, the deck and the walks, in order"
    )]
    fn section(
        &self,
        class: &Class,
        c: ChannelPoint,
        dir: [f64; 2],
        bearing: [f64; 2],
        ground_at: &dyn Fn(f64) -> f64,
        culvert_out: bool,
    ) -> Section {
        let params = &self.network.params;
        let (width, depth, surface) =
            (f64::from(c.width), f64::from(c.depth), f64::from(c.surface));
        let first = class.crossing(width, depth, c.flags);
        // The walks out to the rims and out to the deck's level read the
        // same points (see `out` below), each once: the search reads a
        // section for every edge it weighs across a channel.
        let read: std::cell::RefCell<Vec<(i64, f64)>> = std::cell::RefCell::new(Vec::new());
        let ground_at = |s: f64| {
            #[allow(clippy::cast_possible_truncation, reason = "a key of metres over 64")]
            let key = (s * 64.0).round() as i64;
            if let Some(&(_, h)) = read.borrow().iter().find(|(k, _)| *k == key) {
                return h;
            }
            let h = ground_at(s);
            read.borrow_mut().push((key, h));
            h
        };
        // How square the road crosses the channel, to its indexed line and
        // to the water's own flow there, the skewer of the two (a meander's
        // flow turns off its line's chord by ten degrees and more): along a
        // skew road the channel and its banks are wider by its inverse. Only
        // a bridge's walks and abutments read it.
        let square = || {
            let across = |d: [f64; 2]| (bearing[0] * d[1] - bearing[1] * d[0]).abs();
            let flow = self
                .world
                .sample_by(c.x, c.z, self.network)
                .water
                .and_then(|w| {
                    let f = [f64::from(w.flow[0]), f64::from(w.flow[1])];
                    let l = (f[0] * f[0] + f[1] * f[1]).sqrt();
                    (l > 1e-9).then(|| [f[0] / l, f[1] / l])
                });
            flow.map_or(across(dir), |f| across(dir).min(across(f)))
                .clamp(LEAST_SQUARE, 1.0)
        };
        let mut square_at = (first == CrossingKind::Bridge).then(square);
        // A bridge's banks are read out past the channel's own along the
        // road, as skew as it crosses; a culvert's and a ford's as before.
        let bank_off =
            (0.5 * width + params.bank_top_m(width, depth)) / square_at.unwrap_or(1.0) + 3.0;
        // The top of each bank: out from the channel's own bank while the
        // ground still rises as steeply as a wall, so a creek in a gorge has
        // its banks at the gorge's rim.
        let rim = |side: f64| {
            let mut s = bank_off;
            let mut h = ground_at(side * s);
            while s < RIM_MOST_M {
                let next = ground_at(side * (s + RIM_STEP_M));
                if next - h < RIM_STEP_M * RIM_SLOPE {
                    break;
                }
                s += RIM_STEP_M;
                h = next;
            }
            (h, s)
        };
        let rims = [rim(-1.0), rim(1.0)];
        let banks = [rims[0].0, rims[1].0];
        let low = banks[0].min(banks[1]);
        // A creek in a gorge has its banks at the rim: a culvert's fill or a
        // ford's ramps there would be the gorge's whole depth, so a graded
        // road bridges it at the rim (ticket 0005, step 6); a trail, which
        // follows the ground, still fords it.
        let kind = if first != CrossingKind::Bridge && class.graded && low - surface > GORGE_M {
            CrossingKind::Bridge
        } else {
            first
        };
        if kind == CrossingKind::Ford {
            return Section {
                kind,
                banks,
                deck: surface - depth,
                out_m: [0.0; 2],
                least_m: 0.0,
            };
        }
        // A bridge's deck stands its class's freeboard over the water (the
        // grade holds the road level at it); a culvert's fill a metre over,
        // its pipe under.
        let bridge = kind == CrossingKind::Bridge;
        let clear = if bridge { class.freeboard_m } else { 1.0 };
        // Between unequal banks a bridge's deck stands part of the way up
        // from the lower: its approach there comes up on a fill, and the
        // higher's comes down in a shallower cutting than to the lower rim.
        // A culvert's fill sits on the lower.
        let lift = if bridge {
            (0.5 * (banks[0] - banks[1]).abs()).min(DECK_LIFT_MOST_M)
        } else {
            0.0
        };
        let deck = (low + lift).max(surface + clear);
        // Each abutment at least a metre past the water's edge along the
        // road, as skew as it crosses.
        let least_m = if bridge {
            0.5 * width / *square_at.get_or_insert_with(square) + 1.0
        } else {
            0.5 * width + 1.0
        };
        if !bridge && !culvert_out {
            return Section {
                kind,
                banks,
                deck,
                out_m: [0.0; 2],
                least_m,
            };
        }
        // Out from the channel both ways while the ground is under the
        // deck, but no further than that side's rim: past the lower rim the
        // road is on its fill. In steps on the rims' own, so the points the
        // rims were read at are read once.
        let reach = 0.5 * width + RIM_MOST_M;
        let phase = bank_off - RIM_STEP_M * (bank_off / RIM_STEP_M).floor();
        // Within the water, a metre short of its edge, the ground is the
        // channel's bed, under any deck: not read.
        let wet = ((0.5 * width - 1.0 - phase) / RIM_STEP_M).floor().max(0.0);
        let out = |side: f64, rim: f64| {
            let mut s = phase + wet * RIM_STEP_M;
            let mut found = if wet > 0.0 { s } else { 0.0 };
            while s < reach {
                s += RIM_STEP_M;
                if ground_at(side * s) >= deck - 0.3 {
                    break;
                }
                found = s;
            }
            if lift > 0.0 { found.min(rim) } else { found }
        };
        Section {
            kind,
            banks,
            deck,
            out_m: [out(-1.0, rims[0].1), out(1.0, rims[1].1)],
            least_m,
        }
    }
}

/// A crossing as [`Builder::section`] reads it from the ground along the
/// road.
struct Section {
    kind: CrossingKind,
    /// The ground at the top of each bank, back along the road and on.
    banks: [f64; 2],
    /// The deck's level, a culvert's fill's or a ford's bed.
    deck: f64,
    /// Metres out from the channel, back and on, the ground stays under the
    /// deck: none for a ford, nor for a culvert the search prices.
    out_m: [f64; 2],
    /// The least an abutment stands from the channel's line, metres.
    least_m: f64,
}

impl Section {
    /// The gap between its abutments before a shelf is kept dry (see
    /// `Builder::dry_shelf`), metres.
    fn gap_m(&self) -> f64 {
        self.out_m[0].max(self.least_m) + self.out_m[1].max(self.least_m)
    }
}

/// Value noise in `-1..1` at `TEXTURE_M` and half that, smoothstepped
/// between hashed lattice values: adds, multiplies and `floor`, the same
/// bits on every platform.
#[allow(
    clippy::cast_possible_truncation,
    reason = "lattice indices of a world coordinate over hundreds of metres"
)]
fn texture(seed: u64, x: f64, z: f64) -> f64 {
    let octave = |seed: u64, x: f64, z: f64| {
        let (fx, fz) = (x.floor(), z.floor());
        let (ix, iz) = (fx as i64, fz as i64);
        let (tx, tz) = (x - fx, z - fz);
        let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
        let at = |i: i64, k: i64| {
            let h = strata_core::mix64(
                seed ^ (i.cast_unsigned()).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ (k.cast_unsigned()).wrapping_mul(0xc2b2_ae3d_27d4_eb4f),
            );
            #[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
            let unit = (h >> 11) as f64 / (1_u64 << 53) as f64;
            unit * 2.0 - 1.0
        };
        let a = at(ix, iz) + (at(ix + 1, iz) - at(ix, iz)) * sx;
        let b = at(ix, iz + 1) + (at(ix + 1, iz + 1) - at(ix, iz + 1)) * sx;
        a + (b - a) * sz
    };
    (2.0 * octave(seed, x / TEXTURE_M, z / TEXTURE_M)
        + octave(seed ^ 1, 2.0 * x / TEXTURE_M, 2.0 * z / TEXTURE_M))
        / 3.0
}

/// Whether the kit stands a bridge `gap` long whose deck is at `deck`
/// over a bed at `bed`: no longer than its longest class, and no pier
/// taller than its tallest where the gap needs piers.
fn kit_stands(gap: f64, deck: f64, bed: f64) -> bool {
    !(gap > BRIDGE_GAP_MOST_M
        || (gap > PIERLESS_GAP_MOST_M && deck - bed + PIER_EMBED_M > PIER_MOST_M))
}

/// Whether a deck along `d` (a unit vector) is as square to the water's
/// `flow` (a unit vector) as the record holds a bridge: the kit's piers
/// and the channel's banks follow the flow through a meander's bend more
/// closely than the indexed centreline's chord.
fn square_to_flow(flow: Option<[f64; 2]>, d: [f64; 2]) -> bool {
    flow.is_none_or(|f| (d[0] * f[1] - d[1] * f[0]).abs() >= BRIDGE_LEAST_SQUARE)
}

/// The root of `x`'s set, halving the path to it.
fn find(root: &mut [usize], mut x: usize) -> usize {
    while root[x] != x {
        root[x] = root[root[x]];
        x = root[x];
    }
    x
}

/// The cells a move passes between, which must be open: none for a step,
/// the two beside a diagonal, the two a knight's move runs between.
fn straddled(mv: [i32; 2]) -> [Option<[i32; 2]>; 2] {
    let (dx, dz) = (mv[0], mv[1]);
    match (dx.abs(), dz.abs()) {
        (1, 1) => [Some([dx, 0]), Some([0, dz])],
        (2, 1) => [Some([dx.signum(), 0]), Some([dx.signum(), dz])],
        (1, 2) => [Some([0, dz.signum()]), Some([dx, dz.signum()])],
        (3, 1) => [Some([dx.signum(), 0]), Some([2 * dx.signum(), dz])],
        (1, 3) => [Some([0, dz.signum()]), Some([dx, 2 * dz.signum()])],
        _ => [None, None],
    }
}

/// The cheapest way from `a` to `b` over `edges` (cost, one end, the other),
/// Dijkstra's over a graph of `n` nodes.
fn graph_cost(edges: &[(f64, usize, usize)], n: usize, a: usize, b: usize) -> f64 {
    let mut adjacent: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for &(c, x, y) in edges {
        adjacent[x].push((y, c));
        adjacent[y].push((x, c));
    }
    let mut best = vec![f64::INFINITY; n];
    let mut heap = BinaryHeap::new();
    best[a] = 0.0;
    heap.push(Queued { f: 0.0, key: a });
    while let Some(Queued { f, key }) = heap.pop() {
        if key == b {
            return f;
        }
        if f > best[key] {
            continue;
        }
        for &(m, c) in &adjacent[key] {
            if f + c < best[m] {
                best[m] = f + c;
                heap.push(Queued { f: f + c, key: m });
            }
        }
    }
    f64::INFINITY
}

/// The junction of `road` at fine node `node`, made the first time a road
/// ends there.
fn junction(
    junctions: &mut Vec<Junction>,
    of: &mut NodeMap<Vec<(u32, u32)>>,
    road: u32,
    node: [i32; 2],
    point: [f64; 2],
    along: f64,
) -> u32 {
    if let Some(&(_, j)) = of
        .get(&node)
        .and_then(|v| v.iter().find(|(r, _)| *r == road))
    {
        return j;
    }
    let j = u32::try_from(junctions.len()).expect("fewer than 4 billion junctions");
    junctions.push(Junction {
        at: point,
        road,
        along_m: along,
    });
    of.entry(node).or_default().push((road, j));
    j
}

/// The point `s` metres along `line`, whose points stand `cumulative`
/// metres along it, carried on straight past its ends.
#[allow(
    clippy::many_single_char_names,
    reason = "a segment's ends, its length and the share along it"
)]
fn point_along(line: &[[f64; 2]], cumulative: &[f64], s: f64) -> [f64; 2] {
    if line.len() < 2 {
        return line.first().copied().unwrap_or([0.0, 0.0]);
    }
    let k = cumulative
        .partition_point(|&c| c <= s)
        .clamp(1, line.len() - 1);
    let (a, b) = (line[k - 1], line[k]);
    let l = cumulative[k] - cumulative[k - 1];
    let t = if l > 0.0 {
        (s - cumulative[k - 1]) / l
    } else {
        0.0
    };
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Whether a point moved at all: the same bits are the same point.
fn moved(a: [f64; 2], b: [f64; 2]) -> bool {
    a[0].to_bits() != b[0].to_bits() || a[1].to_bits() != b[1].to_bits()
}

/// A polyline's length.
fn polyline_length(line: &[[f64; 2]]) -> f64 {
    line.windows(2).map(|s| distance(s[0], s[1])).sum()
}

/// The nearest point of a polyline to `p`, and how far along it lies.
#[allow(
    clippy::many_single_char_names,
    reason = "a point, a segment and the share along it"
)]
fn nearest_on(line: &[[f64; 2]], p: [f64; 2]) -> ([f64; 2], f64) {
    if line.len() < 2 {
        return (line.first().copied().unwrap_or(p), 0.0);
    }
    let mut best = (f64::INFINITY, line[0], 0.0);
    let mut along = 0.0;
    for s in line.windows(2) {
        let l = distance(s[0], s[1]);
        let (q, u) = project(p, s[0], s[1]);
        let d = distance(p, q);
        if d < best.0 {
            best = (d, q, along + l * u);
        }
        along += l;
    }
    (best.1, best.2)
}

/// Moves the end `side` of `line` onto `q`, easing the shift out over
/// `blend` metres.
#[allow(
    clippy::many_single_char_names,
    reason = "a line's points, their count and the distance along it"
)]
fn ease(line: &mut [[f64; 2]], side: usize, q: [f64; 2], blend: f64) {
    let n = line.len();
    if n == 0 {
        return;
    }
    let end = if side == 0 { 0 } else { n - 1 };
    let shift = [q[0] - line[end][0], q[1] - line[end][1]];
    let mut s = 0.0;
    for step in 0..n {
        let k = if side == 0 { step } else { n - 1 - step };
        if step > 0 {
            let prev = if side == 0 { k - 1 } else { k + 1 };
            s += distance(line[k], line[prev]);
        }
        if s >= blend {
            break;
        }
        let w = 1.0 - s / blend;
        line[k][0] += shift[0] * w;
        line[k][1] += shift[1] * w;
    }
}

/// `iterations` passes of Laplacian smoothing, the fixed points held.
fn smooth(
    line: &mut [[f64; 2]],
    fixed: &[bool],
    weight: &[f64],
    bound: &Bound<'_>,
    iterations: usize,
) {
    let n = line.len();
    if n < 3 {
        return;
    }
    let mut prev = line.to_vec();
    for _ in 0..iterations {
        prev.copy_from_slice(line);
        for k in 1..n - 1 {
            if fixed[k] {
                continue;
            }
            let a = 0.25 * weight[k];
            line[k] = bound.hold(
                k,
                [
                    prev[k][0] + a * (prev[k - 1][0] + prev[k + 1][0] - 2.0 * prev[k][0]),
                    prev[k][1] + a * (prev[k - 1][1] + prev[k + 1][1] - 2.0 * prev[k][1]),
                ],
            );
        }
    }
}

/// Where the smoothing may move each point: within
/// [`HEIGHT_TOLERANCE_M`] of its searched height, by the ground's gradient
/// there.
#[derive(Clone, Copy)]
struct Bound<'a> {
    raw: &'a [[f64; 2]],
    grads: &'a [[f64; 2]],
    /// Metres up or down the ground a point may move.
    tolerance: f64,
}

impl Bound<'_> {
    /// `p` for point `k`, its move from the searched line cut back to the
    /// band where the ground's height changes by at most the tolerance.
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, its searched place, the gradient and the move"
    )]
    fn hold(&self, k: usize, p: [f64; 2]) -> [f64; 2] {
        let (r, g) = (self.raw[k], self.grads[k]);
        let d = [p[0] - r[0], p[1] - r[1]];
        let rise = g[0] * d[0] + g[1] * d[1];
        let g2 = g[0] * g[0] + g[1] * g[1];
        if rise.abs() <= self.tolerance || g2 < 1e-12 {
            return p;
        }
        let over = (rise - rise.clamp(-self.tolerance, self.tolerance)) / g2;
        [p[0] - g[0] * over, p[1] - g[1] * over]
    }
}

/// The radius of the circle through three points.
fn radius(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    let (ab, bc, ca) = (distance(a, b), distance(b, c), distance(c, a));
    let cross = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
    if cross < 1e-9 {
        f64::INFINITY
    } else {
        ab * bc * ca / (2.0 * cross)
    }
}

/// The least share of a pass [`tighten`] moves a point in a bend too
/// sharp by, however stiffly the climb holds it: the bound already keeps
/// each point within its height tolerance, so a switchback's tip may round
/// without the line being ironed into the fall line.
const TIGHTEN_LEAST: f64 = 0.5;

/// More smoothing only where a bend is under its point's `least` metres, a
/// few dozen passes at most.
fn tighten(
    line: &mut [[f64; 2]],
    fixed: &[bool],
    weight: &[f64],
    bound: &Bound<'_>,
    least: &[f64],
) {
    let n = line.len();
    if n < 5 {
        return;
    }
    for _ in 0..60 {
        let mut soft = vec![false; n];
        let mut any = false;
        for k in 2..n - 2 {
            if radius(line[k - 2], line[k], line[k + 2]) < least[k] {
                for s in &mut soft[k - 1..=k + 1] {
                    *s = true;
                }
                any = true;
            }
        }
        if !any {
            return;
        }
        let prev = line.to_vec();
        for k in 1..n - 1 {
            if soft[k] && !fixed[k] {
                let a = 0.25 * weight[k].max(TIGHTEN_LEAST);
                line[k] = bound.hold(
                    k,
                    [
                        prev[k][0] + a * (prev[k - 1][0] + prev[k + 1][0] - 2.0 * prev[k][0]),
                        prev[k][1] + a * (prev[k - 1][1] + prev[k + 1][1] - 2.0 * prev[k][1]),
                    ],
                );
            }
        }
    }
}
