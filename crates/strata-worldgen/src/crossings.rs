//! What stands where a road crosses a river (ticket 0005, step 7; decided
//! 6): which piece of the content step's crossings kit, where and how
//! turned, as a pure function of the network's crossing record, the kit's
//! layout and the world's ground. The server and every client call it with
//! the same three and stand the same pieces, as they stand a site's group
//! with [`place_group`](crate::sites::place_group).
//!
//! - **A bridge** takes the smallest of its road class's baked span classes
//!   whose gaps hold the gap between its abutments (the grade's level
//!   shelves end there and the ground falls away under the deck), centred
//!   on the gap and turned along the road's chord across it, its deck's top
//!   at the record's `deck_m`. A class is longer than the gap it serves by
//!   several metres: its abutments' faces stand a little out into the gap
//!   and its ends are bedded in the shelves. On a graded road the ground is
//!   read under each end of the deck, across the carriageway: where it does
//!   not meet the deck (a curve carries the road off the deck's axis, or
//!   two crossings close together asked the grade for more than it could
//!   give) the next class up is tried, and if that misses too the crossing
//!   is [`Unplaced::Unmet`] rather than a deck in the air. A long class has piers,
//!   each a piece of its own: spaced evenly, then shifted together the
//!   least that keeps every one off the channel, and each as tall as the
//!   deck stands over the river's bed and a metre more, from the few
//!   heights the kit bakes. A gap longer than the longest class, or a gorge
//!   deeper than the tallest pier, gets nothing yet.
//! - **A culvert** on a road the grade fills is a headwall at each end of
//!   its box, where the fill's slope has come down to the creek again,
//!   found by walking out from the road along the creek's flow on the
//!   world's ground; a trail's creek gets stepping stones beside the trail
//!   where its ground is the creek's.
//! - **A ford** gets a depth post either side of the way at each bank, and
//!   a trail's a row of stepping stones beside it across the water.

use serde::{Deserialize, Serialize};
use strata_core::Rng;

use crate::WorldSource;
use crate::hydrology::Network;
use crate::roads::{Crossing, CrossingKind, Road, RoadClass};

/// Where the content step writes the kit's layout, under its output.
pub const KIT_PATH: &str = "crossings/veyra/kit.ron";

/// A crossing's identity within a world version: the land mass's key, the
/// road's index in its network and the crossing's along the road.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CrossingId {
    /// The land mass's key ([`Network::key`]).
    pub mass: [i32; 2],
    /// The road, an index into [`Network::roads`].
    pub road: u32,
    /// The crossing, an index into its road's crossings.
    pub index: u32,
}

impl CrossingId {
    /// The id as one number, the same on every platform.
    #[must_use]
    pub fn key(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ 0x0c55;
        for v in [
            self.mass[0],
            self.mass[1],
            self.road.cast_signed(),
            self.index.cast_signed(),
        ] {
            for b in v.to_le_bytes() {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

impl std::fmt::Display for CrossingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "crossing {},{}/{}/{}",
            self.mass[0], self.mass[1], self.road, self.index
        )
    }
}

/// A crossing as the world lists it: its record, its road's class, and the
/// road's centreline at each of its abutments (a ford's wetted edges).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProceduralCrossing {
    /// Its id.
    pub id: CrossingId,
    /// Its road's class.
    pub class: RoadClass,
    /// The network's record.
    pub crossing: Crossing,
    /// The road's centreline at `abutments_m[0]` and `[1]`, world `(x, z)`.
    pub edges: [[f64; 2]; 2],
}

/// The point `along` metres down `road`'s centreline, world `(x, z)`.
#[must_use]
pub fn point_along(road: &Road, along: f64) -> [f64; 2] {
    let points = &road.points;
    let Some(first) = points.first() else {
        return [0.0, 0.0];
    };
    let k = points.partition_point(|p| p.along_m <= along);
    if k == 0 {
        return [first.x, first.z];
    }
    if k >= points.len() {
        let last = points[points.len() - 1];
        return [last.x, last.z];
    }
    let (a, b) = (points[k - 1], points[k]);
    let t = if b.along_m > a.along_m {
        (along - a.along_m) / (b.along_m - a.along_m)
    } else {
        0.0
    };
    [a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t]
}

/// Every crossing of `network` whose point `keep` accepts, in id order.
pub fn of_network(network: &Network, keep: impl Fn([f64; 2]) -> bool) -> Vec<ProceduralCrossing> {
    let mut out = Vec::new();
    for (r, road) in network.roads().iter().enumerate() {
        for (i, c) in road.crossings.iter().enumerate() {
            if !keep(c.at) {
                continue;
            }
            out.push(ProceduralCrossing {
                id: CrossingId {
                    mass: network.key(),
                    road: u32::try_from(r).expect("fewer than 4 billion roads"),
                    index: u32::try_from(i).expect("a handful of crossings"),
                },
                class: road.class,
                crossing: *c,
                edges: c.abutments_m.map(|s| point_along(road, s)),
            });
        }
    }
    out
}

/// The crossings' kit as the content step baked it: every piece's file and
/// what the world needs to stand it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossingKit {
    /// Every bridge class.
    pub bridges: Vec<BridgeClass>,
    /// How a road bridge's deck meets the road at each end, which the world
    /// reads the ground against.
    pub deck_end: DeckEnd,
    /// Every pier.
    pub piers: Vec<PierFile>,
    /// A culvert's headwall, one per box height, for every road.
    pub headwalls: Vec<HeadwallFile>,
    /// A ford's depth post.
    pub ford_post: Option<String>,
    /// The stepping stones' shapes.
    pub stones: Vec<String>,
    /// How far a stepping stone's top stands over the water, metres.
    pub stone_proud_m: f64,
}

/// One span class of bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeClass {
    /// The road class it carries.
    pub road: RoadClass,
    /// Its baked file, relative to the content step's output.
    pub file: String,
    /// Metres between its abutments' faces.
    pub clear_m: f64,
    /// The gaps between a crossing's abutments it serves, metres.
    pub gaps_m: [f64; 2],
    /// Its deck's length, metres.
    pub length_m: f64,
    /// How far under the deck's top a pier's cap is set, metres.
    pub underside_m: f64,
    /// How many piers stand under it.
    pub piers: u32,
    /// The longest a girder runs between supports once the piers are
    /// shifted off a channel, metres.
    pub girder_most_m: f64,
}

/// The top of a road bridge's deck near its ends, under the record's
/// `deck_m`: level a little proud of the road, then falling into the shelf
/// over its last metre or so.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckEnd {
    /// How far the level deck's top stands over `deck_m`, metres.
    pub proud_m: f64,
    /// Over its last this many metres the top falls...
    pub ramp_m: f64,
    /// ...by this many metres.
    pub drop_m: f64,
}

impl DeckEnd {
    /// The deck's top over `deck_m` at `from_end` metres in from its end.
    #[must_use]
    pub fn top_m(&self, from_end: f64) -> f64 {
        let into = (self.ramp_m - from_end).clamp(0.0, self.ramp_m);
        self.proud_m - self.drop_m * into / self.ramp_m.max(1e-9)
    }
}

/// A pier of one height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PierFile {
    /// The road class of the bridges it stands under.
    pub road: RoadClass,
    /// Its baked file.
    pub file: String,
    /// From its cap's top to its foot, metres.
    pub height_m: f64,
    /// Half its thickness along the road, cap and all, metres: how much
    /// room the world keeps between it and a channel.
    pub half_along_m: f64,
}

/// A culvert's headwall of one box height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadwallFile {
    /// Its baked file.
    pub file: String,
    /// Its box's height, metres.
    pub box_m: f64,
    /// Its coping's top over the box's floor, metres.
    pub top_m: f64,
}

impl CrossingKit {
    /// Every file the kit names, each once, in order.
    #[must_use]
    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .bridges
            .iter()
            .map(|b| b.file.clone())
            .chain(self.piers.iter().map(|p| p.file.clone()))
            .chain(self.headwalls.iter().map(|h| h.file.clone()))
            .chain(self.ford_post.clone())
            .chain(self.stones.iter().cloned())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    /// The bridge class of `road` that serves a gap of `gap_m`: the
    /// shortest whose gaps hold it.
    #[must_use]
    pub fn bridge_for(&self, road: RoadClass, gap_m: f64) -> Option<&BridgeClass> {
        self.bridges
            .iter()
            .filter(|b| b.road == road && gap_m >= b.gaps_m[0] && gap_m <= b.gaps_m[1])
            .min_by(|a, b| a.clear_m.total_cmp(&b.clear_m))
    }
}

/// One piece of a crossing, stood in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedPiece {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// Its origin, world `(x, y, z)`.
    pub at: [f64; 3],
    /// Its world yaw about `+y`, as `Quat::from_rotation_y` turns.
    pub yaw: f64,
}

/// Why a crossing stands nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unplaced {
    /// Its gap is longer than every class of its road's (or shorter).
    NoClass,
    /// Its piers would have to be taller than the tallest baked.
    TooTall,
    /// The ground under a deck's end does not meet it, with the class that
    /// serves the gap and with the next one up: the road there is not the
    /// level shelf the grade promised (a curve off the deck's axis, or two
    /// crossings too close for one profile). A deck would stand with a lip
    /// at its end or its corners in the air.
    Unmet,
    /// The kit has no piece for it (a layout with no headwall, say).
    NoPiece,
    /// A trail's creek where the trail's ground is not the creek's: in a
    /// gorge, the trail's way across is the ground's.
    NoWater,
}

/// A pier's foot below the river's bed, metres.
pub const PIER_EMBED_M: f64 = 1.0;

/// The longest gap between abutments the kit's bridges serve on a service
/// road or a track, metres: the road search closes a crossing whose gap is
/// longer, rather than lay a road to a crossing that stands nothing
/// ([`Unplaced::NoClass`]). The content step's catalogue is held to it by
/// a test in `veyra-buildings`.
pub const BRIDGE_GAP_MOST_M: f64 = 68.5;

/// The longest gap a bridge with no pier serves, metres: past it a bridge
/// stands on piers, which must reach the river's bed.
pub const PIERLESS_GAP_MOST_M: f64 = 28.5;

/// The tallest pier the kit bakes, metres: a deck further over the bed has
/// none ([`Unplaced::TooTall`]).
pub const PIER_MOST_M: f64 = 48.0;

/// How far a culvert's headwall is looked for out from the road, metres:
/// a track's banks' reach (`hydrology::grade`'s `Way::reach_m`).
const HEADWALL_SEARCH_M: f64 = 30.0;

/// How deep a headwall is, metres, from its face back towards the road:
/// the kit's `HEADWALL_THICK_M`, and the coping's lip over it.
const HEADWALL_BACK_M: f64 = 0.55;

/// How much higher than the crossing's own water a creek may stand where a
/// headwall is looked for, metres, and per metre out: a creek falls along
/// its flow, and water much higher is another channel's.
const SAME_WATER_M: [f64; 2] = [0.5, 0.15];

/// The most a deck's top may stand over the ground under its end, metres:
/// a lip a wheel takes, or a corner over a hollow no one sees under it.
const DECK_OVER_MOST_M: f64 = 0.25;

/// The most the ground may stand over a deck's top near its end, metres:
/// past it the road steps down onto the deck.
const DECK_UNDER_MOST_M: f64 = 0.4;

/// A ford's posts stand this far past the way's edge, metres.
const POST_OUT_M: f64 = 0.7;

/// A row of stepping stones: this far apart, metres (a stride, so the
/// stones read as stones set apart and not as a wall of teeth), and this
/// far beside a trail's centreline.
const STONE_STEP_M: f64 = 0.95;
const STONE_ASIDE_M: f64 = 1.0;

/// What seeds the stepping stones' turns and shapes, beside the id.
const STONE_SALT: u64 = 0x5703_e5a1_7000_0001;

/// The unit `(x, z)` the road runs across the crossing, from its first
/// abutment to its second, or its recorded bearing where they coincide.
fn chord(c: &ProceduralCrossing) -> [f64; 2] {
    let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
    let length = (dx * dx + dz * dz).sqrt();
    if length > 1e-6 {
        [dx / length, dz / length]
    } else {
        c.crossing.bearing
    }
}

/// The yaw that turns a piece's `+z` to the unit `(x, z)` `dir`.
fn yaw_of(dir: [f64; 2]) -> f64 {
    libm::atan2(dir[0], dir[1])
}

/// The creek's downstream direction at the crossing, a unit `(x, z)`, from
/// the world's water record there; square to the road if it has none.
fn flow_at(c: &ProceduralCrossing, ground: &dyn WorldSource, dir: [f64; 2]) -> [f64; 2] {
    let [x, z] = c.crossing.at;
    ground
        .sample(x, z)
        .water
        .map(|w| [f64::from(w.flow[0]), f64::from(w.flow[1])])
        .and_then(|[fx, fz]| {
            let length = (fx * fx + fz * fz).sqrt();
            (length > 1e-6).then(|| [fx / length, fz / length])
        })
        .unwrap_or([dir[1], -dir[0]])
}

/// Stands the kit at `c`, reading `ground` for what the record does not
/// say (a creek's flow, where a fill meets it, the banks under a post).
///
/// # Errors
///
/// [`Unplaced`] when nothing of the kit stands there.
pub fn place_crossing(
    c: &ProceduralCrossing,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
) -> Result<Vec<PlacedPiece>, Unplaced> {
    match c.crossing.kind {
        CrossingKind::Bridge => place_bridge(c, kit, ground),
        CrossingKind::Culvert if c.class == RoadClass::Trail => {
            let x = &c.crossing;
            let height = f64::from(ground.sample(x.at[0], x.at[1]).height);
            if height > x.surface_m + 0.4 {
                return Err(Unplaced::NoWater);
            }
            let half = 0.5 * x.width_m + 0.6;
            stones(c, kit, ground, [x.along_m - half, x.along_m + half])
        }
        CrossingKind::Culvert => place_culvert(c, kit, ground),
        CrossingKind::Ford => {
            let mut pieces = posts(c, kit, ground)?;
            if c.class == RoadClass::Trail {
                let [a0, a1] = c.crossing.abutments_m;
                pieces.extend(stones(c, kit, ground, [a0 - 0.3, a1 + 0.3])?);
            }
            Ok(pieces)
        }
    }
}

/// The deck centred on the gap and its piers: the class that serves the
/// gap, or the next one up where the first's ends miss the ground.
fn place_bridge(
    c: &ProceduralCrossing,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
) -> Result<Vec<PlacedPiece>, Unplaced> {
    let x = &c.crossing;
    let [a0, a1] = x.abutments_m;
    let gap = a1 - a0;
    let first = kit.bridge_for(c.class, gap).ok_or(Unplaced::NoClass)?;
    let dir = chord(c);
    let centre = [
        f64::midpoint(c.edges[0][0], c.edges[1][0]),
        f64::midpoint(c.edges[0][1], c.edges[1][1]),
    ];
    // A trail is not graded: its footbridge's ends stand on the ground as
    // the trail found it, and nothing promised a shelf there to read.
    let class = if c.class == RoadClass::Trail {
        first
    } else {
        let next = kit
            .bridges
            .iter()
            .filter(|b| b.road == c.class && b.clear_m > first.clear_m)
            .min_by(|a, b| a.clear_m.total_cmp(&b.clear_m));
        [Some(first), next]
            .into_iter()
            .flatten()
            .find(|class| ends_meet(c, class, &kit.deck_end, centre, dir, ground))
            .ok_or(Unplaced::Unmet)?
    };
    let yaw = yaw_of(dir);
    let mut pieces = vec![PlacedPiece {
        file: class.file.clone(),
        at: [centre[0], x.deck_m, centre[1]],
        yaw,
    }];
    if class.piers == 0 {
        return Ok(pieces);
    }
    let top = x.deck_m - class.underside_m;
    let need = top - (x.surface_m - x.depth_m) + PIER_EMBED_M;
    let pier = kit
        .piers
        .iter()
        .filter(|p| p.road == c.class && p.height_m >= need)
        .min_by(|a, b| a.height_m.total_cmp(&b.height_m))
        .ok_or(Unplaced::TooTall)?;
    for z in pier_stations(c, class, pier.half_along_m, ground, dir) {
        pieces.push(PlacedPiece {
            file: pier.file.clone(),
            at: [centre[0] + dir[0] * z, top, centre[1] + dir[1] * z],
            yaw,
        });
    }
    Ok(pieces)
}

/// Whether the ground meets a deck of `class` centred at `centre` along
/// `dir`, at both ends: read across the carriageway at the end, at the
/// ramp's top and a metre and a half further in (where that is still half
/// a metre short of the gap, which the grade leaves to the river), it
/// stands within [`DECK_OVER_MOST_M`] under the deck's top and
/// [`DECK_UNDER_MOST_M`] over it everywhere.
fn ends_meet(
    c: &ProceduralCrossing,
    class: &BridgeClass,
    deck: &DeckEnd,
    centre: [f64; 2],
    dir: [f64; 2],
    ground: &dyn WorldSource,
) -> bool {
    let across = [dir[1], -dir[0]];
    let half = crate::sites::way_in_half_m(c.class);
    let end = 0.5 * class.length_m;
    let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
    let gap_half = 0.5 * (dx * dx + dz * dz).sqrt();
    for sign in [-1.0, 1.0] {
        for from_end in [0.05, deck.ramp_m, deck.ramp_m + 1.5] {
            if end - from_end < gap_half + 0.5 {
                continue;
            }
            let top = c.crossing.deck_m + deck.top_m(from_end);
            let z = sign * (end - from_end);
            for off in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let d = off * half;
                let p = [
                    centre[0] + dir[0] * z + across[0] * d,
                    centre[1] + dir[1] * z + across[1] * d,
                ];
                let height = f64::from(ground.sample(p[0], p[1]).height);
                if top - height > DECK_OVER_MOST_M || height - top > DECK_UNDER_MOST_M {
                    return false;
                }
            }
        }
    }
    true
}

/// Where along the deck, from its middle, each pier stands: evenly spaced,
/// then shifted together by the least that keeps every pier clear of the
/// channel's water (its width along the road, as skewed as the creek
/// crosses it) with the girders between them no longer than the class
/// allows and every pier off the abutments. Evenly spaced if no shift
/// does.
#[must_use]
pub fn pier_stations(
    c: &ProceduralCrossing,
    class: &BridgeClass,
    pier_half_m: f64,
    ground: &dyn WorldSource,
    dir: [f64; 2],
) -> Vec<f64> {
    let x = &c.crossing;
    let n = class.piers;
    let span = class.clear_m;
    let spacing = span / f64::from(n + 1);
    let even: Vec<f64> = (1..=n)
        .map(|k| -0.5 * span + spacing * f64::from(k))
        .collect();
    // The channel's centre in the deck's frame, and its half width along
    // the road: wider than the record's where the creek crosses skew.
    let channel = x.along_m - f64::midpoint(x.abutments_m[0], x.abutments_m[1]);
    let flow = flow_at(c, ground, dir);
    let square = (dir[0] * flow[1] - dir[1] * flow[0]).abs().max(0.5);
    let keep = 0.5 * x.width_m / square + 0.5 + pier_half_m;
    let fits = |shift: f64| {
        let stations: Vec<f64> = even.iter().map(|z| z + shift).collect();
        let off_channel = stations.iter().all(|z| (z - channel).abs() >= keep);
        let mut ends = vec![-0.5 * span];
        ends.extend(stations.iter().copied());
        ends.push(0.5 * span);
        let girders = ends
            .windows(2)
            .all(|w| w[1] - w[0] <= class.girder_most_m && w[1] - w[0] >= pier_half_m + 1.0);
        off_channel && girders
    };
    let most = 0.45 * spacing;
    let mut shift = 0.0;
    while shift <= most {
        for s in [shift, -shift] {
            if fits(s) {
                return even.iter().map(|z| z + s).collect();
            }
        }
        shift += 0.25;
    }
    even
}

/// A headwall at each end of a culvert's box: walking out from the road
/// along the creek's flow, from past the verge (a headwall's back never
/// stands in the carriageway or its verge, even where the creek's water is
/// up to the road's edge), where the ground first stands under the creek's
/// own water there (the fill's slope has come down to it), its face there
/// and its floor on that ground. Past the road's banks' reach the creek is
/// not the road's to mouth: a side whose creek stays buried that far (the
/// grade's banks bury a service road's creek for tens of metres where its
/// floodplain is as high as the road) gets none.
///
/// Its box is the tallest of the kit's whose coping stays under the road's
/// level, so a culvert under a low road is a low one; a side where even the
/// lowest would stand over the road (the road is less than a metre over the
/// creek's ground there) gets none either, the creek running through the
/// fill unmarked.
fn place_culvert(
    c: &ProceduralCrossing,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
) -> Result<Vec<PlacedPiece>, Unplaced> {
    let x = &c.crossing;
    if kit.headwalls.is_empty() {
        return Err(Unplaced::NoPiece);
    }
    let dir = chord(c);
    let flow = flow_at(c, ground, dir);
    let way = crate::hydrology::grade::way(c.class);
    let start = way.half_m + way.verge_m + HEADWALL_BACK_M;
    let mut pieces = Vec::new();
    for side in [1.0, -1.0] {
        let out = [flow[0] * side, flow[1] * side];
        let mut d = start;
        while d <= HEADWALL_SEARCH_M {
            let p = [x.at[0] + out[0] * d, x.at[1] + out[1] * d];
            let here = ground.sample(p[0], p[1]);
            let wet = here.water.is_some_and(|w| {
                here.height <= w.surface_m - 0.02
                    && f64::from(w.surface_m) <= x.surface_m + SAME_WATER_M[0] + SAME_WATER_M[1] * d
            });
            if wet {
                let floor = f64::from(here.height) - 0.05;
                let fits = kit
                    .headwalls
                    .iter()
                    .filter(|h| floor + h.top_m <= x.deck_m)
                    .max_by(|a, b| a.box_m.total_cmp(&b.box_m));
                if let Some(headwall) = fits {
                    pieces.push(PlacedPiece {
                        file: headwall.file.clone(),
                        at: [p[0], floor, p[1]],
                        yaw: yaw_of(out),
                    });
                }
                break;
            }
            d += 0.25;
        }
    }
    Ok(pieces)
}

/// A depth post either side of the way at each of a ford's banks, on the
/// ground there.
fn posts(
    c: &ProceduralCrossing,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
) -> Result<Vec<PlacedPiece>, Unplaced> {
    let post = kit.ford_post.as_ref().ok_or(Unplaced::NoPiece)?;
    let dir = chord(c);
    let across = [dir[1], -dir[0]];
    let out = crate::sites::way_in_half_m(c.class) + POST_OUT_M;
    let yaw = yaw_of(dir);
    let mut pieces = Vec::new();
    for edge in c.edges {
        for side in [-1.0, 1.0] {
            let p = [
                edge[0] + across[0] * out * side,
                edge[1] + across[1] * out * side,
            ];
            let height = f64::from(ground.sample(p[0], p[1]).height);
            pieces.push(PlacedPiece {
                file: post.clone(),
                at: [p[0], height - 0.02, p[1]],
                yaw,
            });
        }
    }
    Ok(pieces)
}

/// Stepping stones beside the way from `along[0]` to `along[1]` metres down
/// the road, each its top [`CrossingKit::stone_proud_m`] over the water,
/// shapes and turns seeded by the crossing's id; none where the ground
/// already stands at its top.
fn stones(
    c: &ProceduralCrossing,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
    along: [f64; 2],
) -> Result<Vec<PlacedPiece>, Unplaced> {
    if kit.stones.is_empty() {
        return Err(Unplaced::NoPiece);
    }
    let x = &c.crossing;
    let dir = chord(c);
    let across = [dir[1], -dir[0]];
    let top = x.surface_m + kit.stone_proud_m;
    let mut rng = Rng::new(c.id.key() ^ STONE_SALT);
    let shapes = u32::try_from(kit.stones.len()).expect("a few shapes");
    let mut pieces = Vec::new();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a ford a few metres wide"
    )]
    let count = ((along[1] - along[0]) / STONE_STEP_M).round().max(1.0) as u32;
    for k in 0..=count {
        let s = along[0] + (along[1] - along[0]) * f64::from(k) / f64::from(count);
        let on = [
            x.at[0] + dir[0] * (s - x.along_m),
            x.at[1] + dir[1] * (s - x.along_m),
        ];
        // A hand's width of wander, so the row is laid and not printed.
        let wander = rng.next_f64() * 0.2 - 0.1;
        let p = [
            on[0] + across[0] * (STONE_ASIDE_M + wander),
            on[1] + across[1] * (STONE_ASIDE_M + wander),
        ];
        let shape = kit.stones[rng.below(shapes) as usize].clone();
        let turn = rng.next_f64() * std::f64::consts::TAU;
        if f64::from(ground.sample(p[0], p[1]).height) >= top - 0.02 {
            continue;
        }
        pieces.push(PlacedPiece {
            file: shape,
            at: [p[0], top, p[1]],
            yaw: turn,
        });
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sample, roads::Crossing};
    use strata_data::BiomeId;

    /// Level ground at `height`, a creek's record everywhere flowing `+x`,
    /// and a channel `half` either side of `z = 0` down to `bed`.
    struct Creek {
        height: f32,
        half: f64,
        bed: f32,
    }

    impl WorldSource for Creek {
        fn sample(&self, _x: f64, z: f64) -> Sample {
            Sample {
                height: if z.abs() < self.half {
                    self.bed
                } else {
                    self.height
                },
                biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.5,
                hydration: 0.5,
                cover: 0.5,
                landform: crate::Landform::default(),
                water: Some(crate::SampleWater {
                    surface_m: 10.0,
                    bed_m: self.bed,
                    width_m: 2.0,
                    signed_distance_m: 0.0,
                    flow: [1.0, 0.0],
                    discharge_m3s: 1.0,
                    order: 1,
                    kind: crate::hydrology::WaterKind::Channel,
                    cut_dm: 0,
                }),
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    fn kit() -> CrossingKit {
        let class = |clear_m: f64, piers: u32| BridgeClass {
            road: RoadClass::Service,
            file: format!("bridge-{clear_m}.ashlar"),
            clear_m,
            gaps_m: [clear_m + 0.5, clear_m + 4.5],
            length_m: clear_m + 6.0,
            underside_m: 1.3,
            piers,
            girder_most_m: 27.0,
        };
        CrossingKit {
            bridges: vec![
                class(20.0, 0),
                class(24.0, 0),
                class(44.0, 1),
                class(48.0, 2),
            ],
            deck_end: DeckEnd {
                proud_m: 0.02,
                ramp_m: 1.2,
                drop_m: 0.12,
            },
            piers: [6.0, 12.0]
                .map(|height_m| PierFile {
                    road: RoadClass::Service,
                    file: format!("pier-{height_m}.ashlar"),
                    height_m,
                    half_along_m: 0.9,
                })
                .to_vec(),
            headwalls: [0.45, 0.85]
                .map(|box_m| HeadwallFile {
                    file: format!("headwall-{box_m}.ashlar"),
                    box_m,
                    top_m: box_m + 0.4,
                })
                .to_vec(),
            ford_post: Some("post.ashlar".to_owned()),
            stones: vec!["stone-0.ashlar".to_owned(), "stone-1.ashlar".to_owned()],
            stone_proud_m: 0.12,
        }
    }

    /// A road along `+z` over the creek at the origin, its abutments
    /// `gap` apart, the channel `along` metres from their middle.
    fn crossing(
        kind: CrossingKind,
        class: RoadClass,
        gap: f64,
        channel: f64,
    ) -> ProceduralCrossing {
        let along = 100.0;
        let mid = along - channel;
        ProceduralCrossing {
            id: CrossingId {
                mass: [0, 0],
                road: 3,
                index: 1,
            },
            class,
            crossing: Crossing {
                kind,
                at: [0.0, 0.0],
                along_m: along,
                bearing: [0.0, 1.0],
                span_m: gap,
                deck_m: 12.5,
                surface_m: 10.0,
                banks_m: [10.3, 10.3],
                width_m: 6.0,
                depth_m: 0.5,
                river: 0,
                abutments_m: [mid - 0.5 * gap, mid + 0.5 * gap],
            },
            edges: [[0.0, -channel - 0.5 * gap], [0.0, -channel + 0.5 * gap]],
        }
    }

    fn creek() -> Creek {
        Creek {
            height: 11.0,
            half: 3.0,
            bed: 9.5,
        }
    }

    /// A graded road's ground about a bridge along `z`: the creek's bed in
    /// the gap (`|z|` under `gap_half`), a hollow at 11 m out to `hollow_to`
    /// where the abutment's face has not yet come up to the shelf, then the
    /// level shelf at `level`.
    struct Shelf {
        level: f32,
        gap_half: f64,
        hollow_to: f64,
    }

    impl WorldSource for Shelf {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let mut s = creek().sample(x, z);
            s.height = if z.abs() < self.gap_half {
                9.5
            } else if z.abs() < self.hollow_to {
                11.0
            } else {
                self.level
            };
            s
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    /// The shelf a crossing's grade leaves it: level with its deck from the
    /// gap's edges out.
    fn shelf(c: &ProceduralCrossing) -> Shelf {
        let gap = c.crossing.abutments_m[1] - c.crossing.abutments_m[0];
        #[allow(clippy::cast_possible_truncation, reason = "a test's heights")]
        let level = c.crossing.deck_m as f32;
        Shelf {
            level,
            gap_half: 0.5 * gap,
            hollow_to: 0.5 * gap,
        }
    }

    /// The class a crossing takes covers its gap: its faces stand inside
    /// the gap, and its deck reaches past both edges onto the shelves.
    #[test]
    fn the_class_chosen_covers_the_span() {
        let kit = kit();
        for gap in [20.6, 22.0, 24.5, 26.0, 28.4] {
            let c = crossing(CrossingKind::Bridge, RoadClass::Service, gap, 0.0);
            let class = kit.bridge_for(c.class, gap).expect("a class");
            assert!(
                class.clear_m <= gap && class.length_m >= gap,
                "{gap}: {class:?}"
            );
            let pieces = place_crossing(&c, &kit, &shelf(&c)).expect("placed");
            assert_eq!(pieces[0].file, class.file);
            assert!((pieces[0].at[1] - 12.5).abs() < 1e-12);
            // Centred on the gap, turned along the road.
            assert!(pieces[0].at[0].abs() < 1e-12 && pieces[0].at[2].abs() < 1e-12);
            assert!(pieces[0].yaw.abs() < 1e-12);
        }
        let long = crossing(CrossingKind::Bridge, RoadClass::Service, 80.0, 0.0);
        assert_eq!(
            place_crossing(&long, &kit, &shelf(&long)),
            Err(Unplaced::NoClass)
        );
    }

    /// Where the ground under the deck's end of the class that serves the
    /// gap has not come up to the shelf, the next class up, whose ends
    /// reach further, stands instead; where neither meets the ground the
    /// crossing stands nothing and says so, rather than a deck in the air.
    #[test]
    fn a_deck_whose_ends_miss_the_ground_takes_the_next_class_or_none() {
        let kit = kit();
        // The 20 m class serves a 22 m gap with its ends 2 m past the
        // gap's edges, the 24 m class 4 m past.
        let c = crossing(CrossingKind::Bridge, RoadClass::Service, 22.0, 0.0);
        assert_eq!(
            place_crossing(&c, &kit, &shelf(&c)).expect("placed")[0].file,
            "bridge-20.ashlar"
        );
        let hollow = Shelf {
            hollow_to: 12.2,
            ..shelf(&c)
        };
        assert_eq!(
            place_crossing(&c, &kit, &hollow).expect("placed")[0].file,
            "bridge-24.ashlar"
        );
        assert_eq!(place_crossing(&c, &kit, &creek()), Err(Unplaced::Unmet));
        // A footbridge's trail is not graded and is not read.
        let trail = crossing(CrossingKind::Bridge, RoadClass::Trail, 22.0, 0.0);
        let mut kit = kit;
        for b in &mut kit.bridges {
            b.road = RoadClass::Trail;
        }
        assert_eq!(
            place_crossing(&trail, &kit, &creek()).expect("placed")[0].file,
            "bridge-20.ashlar"
        );
    }

    /// A pier stands off the channel when a shift lets it, and reaches
    /// below the bed; a gorge deeper than the tallest pier gets nothing.
    #[test]
    fn piers_keep_off_the_channel_and_reach_below_the_bed() {
        let kit = kit();
        let c = crossing(CrossingKind::Bridge, RoadClass::Service, 46.0, 0.0);
        let pieces = place_crossing(&c, &kit, &shelf(&c)).expect("placed");
        assert_eq!(pieces.len(), 2, "the deck and its pier");
        let pier = &pieces[1];
        // Off the channel's 3 m half width and the pier's own.
        assert!(pier.at[2].abs() >= 3.0 + 0.9, "{pier:?}");
        let height = kit
            .piers
            .iter()
            .find(|p| p.file == pier.file)
            .expect("a pier")
            .height_m;
        let bed = 10.0 - 0.5;
        assert!(pier.at[1] - height <= bed - PIER_EMBED_M + 1e-9);
        assert!((pier.at[1] - (12.5 - 1.3)).abs() < 1e-12);
        let mut deep = crossing(CrossingKind::Bridge, RoadClass::Service, 46.0, 0.0);
        deep.crossing.deck_m = 40.0;
        assert_eq!(
            place_crossing(&deep, &kit, &shelf(&deep)),
            Err(Unplaced::TooTall)
        );
    }

    /// The fill: 11 m inside 6 m of the road, then the creek's bed.
    struct Fill;
    impl WorldSource for Fill {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let mut s = creek().sample(x, z);
            s.height = if x.abs() < 6.0 { 11.0 } else { 9.5 };
            s
        }
        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    /// A culvert's headwalls stand where the fill comes down to the creek,
    /// either side, facing out along its flow, and never with their backs
    /// in the carriageway or its verge, even where the creek's water is at
    /// the road's edge; each is the tallest box whose coping stays under
    /// the road, and none where even the lowest would stand over it.
    #[test]
    fn a_culverts_headwalls_face_out_where_the_creek_is() {
        let kit = kit();
        let mut c = crossing(CrossingKind::Culvert, RoadClass::Service, 15.0, 0.0);
        // The fill: 11 m inside 6 m of the road, then the creek's bed.
        let pieces = place_crossing(&c, &kit, &Fill).expect("placed");
        assert_eq!(pieces.len(), 2);
        let clear = 4.0 + 2.5 + HEADWALL_BACK_M;
        for p in &pieces {
            assert!((p.at[0].abs() - clear).abs() <= 0.25, "{p:?}");
            assert!((p.at[1] - 9.45).abs() < 1e-6);
            assert_eq!(p.file, "headwall-0.85.ashlar");
            // Facing away from the road.
            let out = [libm::sin(p.yaw), libm::cos(p.yaw)];
            assert!(out[0] * p.at[0] > 0.0);
        }
        // Flooded to the road's edge: still past the verge.
        let flooded = place_crossing(&c, &kit, &creek()).expect("placed");
        assert!(flooded.iter().all(|p| p.at[0].abs() >= clear - 1e-9));
        // A low road: the low box, then none.
        c.crossing.deck_m = 10.5;
        let low = place_crossing(&c, &kit, &Fill).expect("placed");
        assert!(
            low.iter().all(|p| p.file == "headwall-0.45.ashlar"),
            "{low:?}"
        );
        c.crossing.deck_m = 9.8;
        assert!(place_crossing(&c, &kit, &Fill).expect("placed").is_empty());
    }

    /// A ford's posts stand on the ground either side of the way at both
    /// banks; a trail's gets stepping stones over the water beside it, the
    /// same ones every time.
    #[test]
    fn a_ford_is_marked_and_a_trails_has_stones() {
        let kit = kit();
        let c = crossing(CrossingKind::Ford, RoadClass::Track, 6.0, 0.0);
        let pieces = place_crossing(&c, &kit, &creek()).expect("placed");
        assert_eq!(pieces.len(), 4);
        assert!(pieces.iter().all(|p| p.file == "post.ashlar"));
        let trail = crossing(CrossingKind::Ford, RoadClass::Trail, 6.0, 0.0);
        let pieces = place_crossing(&trail, &kit, &creek()).expect("placed");
        let stones: Vec<_> = pieces
            .iter()
            .filter(|p| p.file.starts_with("stone"))
            .collect();
        assert!(stones.len() >= 5, "{stones:?}");
        for s in &stones {
            assert!((s.at[1] - 10.12).abs() < 1e-12);
            assert!(s.at[2].abs() < 3.0, "a stone on the bank");
        }
        // A stride apart, not a wall of teeth.
        for pair in stones.windows(2) {
            assert!((pair[1].at[2] - pair[0].at[2]).abs() >= 0.9, "{pair:?}");
        }
        assert_eq!(
            pieces,
            place_crossing(&trail, &kit, &creek()).expect("placed")
        );
    }
}
