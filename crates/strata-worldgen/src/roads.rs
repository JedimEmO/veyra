//! The road network (ticket 0005, step 5; ADR 0012, decision 7): what a land
//! mass's bake leaves of the roads between its places, and the query for the
//! nearest one.
//!
//! The bake (`hydrology::roads`) joins a mass's places after it has placed
//! them: service roads between towns and Port Serein along a sparse
//! backbone, gravel tracks from each hamlet, station and landing to the
//! network where it is cheapest to reach, foot trails to pass shelters and
//! landmarks. Each road is a least-cost route over the carved ground, then
//! smoothed into a curve; what it leaves is per road a centreline every
//! [`POINT_STEP_M`] metres with the ground under it, its two ends, its
//! crossings of rivers and its profile ([`Road::profile`], step 6), which a
//! sample by the road cuts and fills the ground to.
//!
//! The records are part of the network a mass bakes to, so they are the same
//! on any machine and thread count and are folded into its content hash, so
//! into the world's version through the canaries.

pub use strata_registry::RoadClass;

use crate::hydrology::NodeMap;
use crate::sites::SiteId;

/// The most metres between the points of a road's recorded centreline, on
/// a straight: fine enough for a grade to fit a profile to.
pub const POINT_STEP_M: f64 = 20.0;

/// How far a chord between two points of a centreline stands off the curve
/// at most, metres: in a bend the points close up, down to
/// [`POINT_LEAST_M`], so the line keeps the curve the smoothing made.
pub const POINT_SAG_M: f64 = 0.25;

/// The least metres between two points of a centreline.
pub const POINT_LEAST_M: f64 = 4.0;

/// What a class of road is held to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassSpec {
    /// The steepest the road climbs, rise over run, where any route allows.
    pub max_grade: f64,
    /// The tightest bend its centreline takes, metres.
    pub min_radius_m: f64,
    /// Half its carriageway, metres: the way a site keeps clear for it.
    pub half_width_m: f64,
}

/// The numbers a class is routed and checked by.
#[must_use]
pub const fn spec(class: RoadClass) -> ClassSpec {
    match class {
        RoadClass::Service => ClassSpec {
            max_grade: 0.08,
            min_radius_m: 50.0,
            half_width_m: crate::sites::way_in_half_m(class),
        },
        RoadClass::Track => ClassSpec {
            max_grade: 0.12,
            min_radius_m: 25.0,
            half_width_m: crate::sites::way_in_half_m(class),
        },
        RoadClass::Trail => ClassSpec {
            max_grade: 0.22,
            min_radius_m: 10.0,
            half_width_m: crate::sites::way_in_half_m(class),
        },
    }
}

/// A point of a road's centreline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadPoint {
    /// World metres.
    pub x: f64,
    /// World metres.
    pub z: f64,
    /// Metres along the road from its first end.
    pub along_m: f64,
    /// The ground's height here as the world samples it (carved, pads
    /// laid), metres: what the grade fits its profile over.
    pub ground_m: f32,
}

/// How a road gets over a river.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossingKind {
    /// A deck on piers and abutments over a straight reach with firm banks.
    Bridge,
    /// Through the water on a riffle no deeper than
    /// [`FORD_DEPTH_M`](crate::hydrology::FORD_DEPTH_M): tracks and trails
    /// over small rivers.
    Ford,
    /// A pipe under the road: a creek under about two metres wide.
    Culvert,
}

/// Where a road crosses a river, as the crossing's kit (step 7) needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// What kind.
    pub kind: CrossingKind,
    /// Where the road's centreline meets the channel's, world `(x, z)`.
    pub at: [f64; 2],
    /// Metres along the road.
    pub along_m: f64,
    /// The road's direction there, a unit `(x, z)` from its first end on.
    pub bearing: [f64; 2],
    /// Bank to bank at the deck's height, metres: a bridge's or a
    /// culvert's span; a ford's wetted width.
    pub span_m: f64,
    /// The carriageway's height over the water: a bridge's or a culvert's
    /// deck, a ford's bed, metres.
    pub deck_m: f64,
    /// The water's surface, metres.
    pub surface_m: f64,
    /// The ground at the top of each bank, before the crossing along the
    /// road and after it, metres.
    pub banks_m: [f64; 2],
    /// The channel's width and centreline depth here, metres.
    pub width_m: f64,
    /// The water's depth at the channel's centre, metres.
    pub depth_m: f64,
    /// The reach crossed, an index into
    /// [`Network::rivers`](crate::hydrology::Network::rivers).
    pub river: u32,
    /// Metres along the road where the crossing meets each bank: a bridge's
    /// or a culvert's abutments, where the ground first stands within a
    /// handspan of the deck, and a ford's wetted edges. A bridge's stand
    /// further out where the level shelf behind one (square to the road,
    /// across its carriageway and verge) would reach into the channel, as
    /// on a skew deck or a meander that curls back. The grade (ticket
    /// 0005, step 6) holds the road level at the deck between them and, at
    /// a bridge, leaves the ground under the deck alone.
    pub abutments_m: [f64; 2],
}

/// Where a road ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoadEnd {
    /// At a procedural site's [`arrival`](crate::sites::ProceduralSite::arrival).
    Site(SiteId),
    /// At an authored settlement's exit: its own road's first end, an index
    /// into [`ProceduralWorld::sites`](crate::ProceduralWorld::sites).
    Settlement(u32),
    /// The far end of an authored settlement's own road, where the network
    /// starts.
    RoadHead(u32),
    /// On another road, at the junction of this index in
    /// [`Network::junctions`](crate::hydrology::Network::junctions).
    Junction(u32),
}

/// A point where a road ends on another one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Junction {
    /// World `(x, z)`, on the through road's centreline.
    pub at: [f64; 2],
    /// The road it lies on.
    pub road: u32,
    /// Metres along that road.
    pub along_m: f64,
}

/// One road of the network.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    /// Its class.
    pub class: RoadClass,
    /// Its centreline from its first end to its second: a point every
    /// [`POINT_STEP_M`] on a straight, closer in a bend (so a chord stands
    /// at most [`POINT_SAG_M`] off the curve), with the ground at each.
    pub points: Vec<RoadPoint>,
    /// Its first and second end.
    pub ends: [RoadEnd; 2],
    /// Where it crosses water, in order along it.
    pub crossings: Vec<Crossing>,
    /// Its grade (ticket 0005, step 6), point for point: the carriageway's
    /// level at the centreline, metres, and its rise per metre along the
    /// road. Empty for a trail, which follows the ground, and for an
    /// authored settlement's own road, which the settlement lays.
    pub profile: Vec<[f32; 2]>,
}

impl Road {
    /// Its length, metres.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p.along_m)
    }

    /// Whether the grade sets its ground to a profile of its own.
    #[must_use]
    pub fn graded(&self) -> bool {
        !self.profile.is_empty()
    }

    /// The profile's level at `along` metres from the first end, and its
    /// rise per metre there: a cubic between two points' levels and rises,
    /// so level and rise are continuous along the road. `None` for a road
    /// without a profile.
    #[must_use]
    pub fn level_at(&self, along: f64) -> Option<(f64, f64)> {
        if self.profile.len() != self.points.len() || self.points.is_empty() {
            return None;
        }
        let k = self
            .points
            .partition_point(|p| p.along_m <= along)
            .clamp(1, self.points.len().max(2) - 1);
        if self.points.len() == 1 {
            let [level, rise] = self.profile[0];
            return Some((f64::from(level), f64::from(rise)));
        }
        Some(hermite(
            [self.points[k - 1].along_m, self.points[k].along_m],
            [self.profile[k - 1], self.profile[k]],
            along,
        ))
    }

    /// Whether the road is an authored settlement's own (ADR 0009,
    /// decision 3), which the settlement lays and the grade leaves alone.
    #[must_use]
    pub fn is_settlements(&self) -> bool {
        self.ends
            .iter()
            .any(|end| matches!(end, RoadEnd::Settlement(_)))
    }
}

/// The cubic through two points of a profile at `along`, clamped to their
/// span: `(level, rise)`. Adds and multiplies only.
#[must_use]
pub(crate) fn hermite(at: [f64; 2], profile: [[f32; 2]; 2], along: f64) -> (f64, f64) {
    let length = at[1] - at[0];
    let [p0, m0] = profile[0].map(f64::from);
    let [p1, m1] = profile[1].map(f64::from);
    if length <= 0.0 {
        return (p0, m0);
    }
    let t = ((along - at[0]) / length).clamp(0.0, 1.0);
    let (t2, t3) = (t * t, t * t * t);
    let level = (2.0 * t3 - 3.0 * t2 + 1.0) * p0
        + (t3 - 2.0 * t2 + t) * length * m0
        + (-2.0 * t3 + 3.0 * t2) * p1
        + (t3 - t2) * length * m1;
    let rise = ((6.0 * t2 - 6.0 * t) * p0 + (-6.0 * t2 + 6.0 * t) * p1) / length
        + (3.0 * t2 - 4.0 * t + 1.0) * m0
        + (3.0 * t2 - 2.0 * t) * m1;
    (level, rise)
}

/// The nearest road to a point ([`Network::nearest_road`](crate::hydrology::Network::nearest_road)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadHere {
    /// Its index in its network's [`roads`](crate::hydrology::Network::roads).
    pub road: u32,
    /// The land mass's key.
    pub mass: [i32; 2],
    /// Its class.
    pub class: RoadClass,
    /// Metres from the centreline.
    pub distance_m: f64,
    /// The road's direction at its nearest point, a unit `(x, z)` from its
    /// first end on.
    pub bearing: [f64; 2],
    /// Metres along it.
    pub along_m: f64,
    /// The nearest point of the centreline, world `(x, z)`.
    pub at: [f64; 2],
    /// The ground under that point as the bake recorded it, metres.
    pub ground_m: f64,
}

/// Buckets of road segments, for the nearest road to a point.
#[derive(Debug, Default)]
pub(crate) struct RoadIndex {
    cells: NodeMap<Vec<(u32, u32)>>,
}

/// The side of a [`RoadIndex`] bucket, metres.
const BUCKET_M: f64 = 128.0;

#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over a bucket is far inside an i32"
)]
fn bucket(metres: f64) -> i32 {
    (metres / BUCKET_M).floor() as i32
}

impl RoadIndex {
    /// Every segment of every road, in each bucket its box touches.
    pub(crate) fn new(roads: &[Road]) -> Self {
        let mut keyed: Vec<([i32; 2], (u32, u32))> = Vec::new();
        for (r, road) in roads.iter().enumerate() {
            let r = u32::try_from(r).expect("fewer than 4 billion roads");
            for (k, pair) in road.points.windows(2).enumerate() {
                let k = u32::try_from(k).expect("fewer than 4 billion points");
                let (a, b) = (pair[0], pair[1]);
                for j in bucket(a.z.min(b.z))..=bucket(a.z.max(b.z)) {
                    for i in bucket(a.x.min(b.x))..=bucket(a.x.max(b.x)) {
                        keyed.push(([i, j], (r, k)));
                    }
                }
            }
        }
        keyed.sort_unstable();
        let mut cells: NodeMap<Vec<(u32, u32)>> = NodeMap::default();
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            cells.insert(group[0].0, group.iter().map(|g| g.1).collect());
        }
        Self { cells }
    }

    /// Every `(road, segment)` whose bucket a box `[min_x, min_z, max_x,
    /// max_z]` touches, sorted and each once: what a sampled cell gathers
    /// the grade from.
    pub(crate) fn within(&self, b: [f64; 4], out: &mut Vec<(u32, u32)>) {
        out.clear();
        if self.cells.is_empty() {
            return;
        }
        for j in bucket(b[1])..=bucket(b[3]) {
            for i in bucket(b[0])..=bucket(b[2]) {
                if let Some(list) = self.cells.get(&[i, j]) {
                    out.extend_from_slice(list);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// The nearest point of any road within `reach` of `(x, z)`: buckets
    /// are read in rings outward until no farther ring can hold a nearer
    /// segment; ties go to the lower road and segment.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a reach over a bucket is a few hundred at most"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, a segment's ends and the share along it"
    )]
    pub(crate) fn nearest(
        &self,
        roads: &[Road],
        mass: [i32; 2],
        x: f64,
        z: f64,
        reach: f64,
    ) -> Option<RoadHere> {
        if self.cells.is_empty() || reach.is_nan() {
            return None;
        }
        let centre = [bucket(x), bucket(z)];
        let rings = (reach.min(1e5) / BUCKET_M).ceil() as i32 + 1;
        // (distance squared, road, segment, share along it)
        let mut best: Option<(f64, u32, u32, f64)> = None;
        for r in 0..=rings {
            let mut visit = |i: i32, j: i32| {
                let Some(list) = self.cells.get(&[centre[0] + i, centre[1] + j]) else {
                    return;
                };
                for &(road, k) in list {
                    let points = &roads[road as usize].points;
                    let (a, b) = (points[k as usize], points[k as usize + 1]);
                    let (dx, dz) = (b.x - a.x, b.z - a.z);
                    let length2 = dx * dx + dz * dz;
                    let u = if length2 > 0.0 {
                        (((x - a.x) * dx + (z - a.z) * dz) / length2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let (px, pz) = (a.x + dx * u - x, a.z + dz * u - z);
                    let d2 = px * px + pz * pz;
                    if best.is_none_or(|(held, hr, hk, _)| {
                        d2.total_cmp(&held)
                            .then(road.cmp(&hr))
                            .then(k.cmp(&hk))
                            .is_lt()
                    }) {
                        best = Some((d2, road, k, u));
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
            let floor = f64::from(r) * BUCKET_M;
            if best.is_some_and(|(d2, ..)| d2 <= floor * floor) {
                break;
            }
        }
        let (d2, road, k, u) = best?;
        let distance = d2.sqrt();
        if distance > reach {
            return None;
        }
        let points = &roads[road as usize].points;
        let (a, b) = (points[k as usize], points[k as usize + 1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let length = (dx * dx + dz * dz).sqrt();
        let bearing = if length > 0.0 {
            [dx / length, dz / length]
        } else {
            [0.0, 0.0]
        };
        Some(RoadHere {
            road,
            mass,
            class: roads[road as usize].class,
            distance_m: distance,
            bearing,
            along_m: a.along_m + (b.along_m - a.along_m) * u,
            at: [a.x + dx * u, a.z + dz * u],
            ground_m: f64::from(a.ground_m) + (f64::from(b.ground_m) - f64::from(a.ground_m)) * u,
        })
    }
}
