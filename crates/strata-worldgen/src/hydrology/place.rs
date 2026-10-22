//! Where a land mass's procedural sites go (ADR 0012, decision 3; ticket
//! 0005, step 4): run in the bake after the rivers, on the ground the
//! mass's own network carves.
//!
//! - **The survey.** Every node of the mass is sampled once on the real
//!   ground (carved, no pad): its height, slope, biomes, provinces and
//!   whether it is dry, undeveloped ground. The heights make a lattice the
//!   measures of the ground read by bilinear interpolation: how far a place
//!   stands over the ground round it (`summit`), how much of the horizon is
//!   open and below it (`view`), how deep a saddle it sits in (`pass`) and
//!   how much of the sea's horizon land closes (`cove`).
//! - **The features.** The sea's shore (found on the natural ground between
//!   each coast node and the sea beyond it), the rivers' channels, fords,
//!   mouths, confluences, falls and gorges from the network, and the lakes'
//!   shores, each in a grid of points for the nearest one within a reach.
//! - **The kinds**, in rank order. Every node is scored against the kind's
//!   wants (each the best of its terms, the score their weighted mean) times
//!   its ground's biome and province weights; the nodes are taken best
//!   first, ties by a hash of the node and then the node's coordinate. Each
//!   is refined to the real ground: nine places round the node are scored
//!   on their own sample, and the best is tried as a pad (its footprint,
//!   band and the ring past it sampled): no water, no channel, no carved
//!   wall, no settlement's ground, nothing below the sea, no cut or fill
//!   over the kind's and no slope over its limit. The first refined place
//!   clear of every site already placed, by the spacing of both kinds and
//!   of authored settlements, is taken, until the kind's count for the
//!   mass's area is met or a bound on refinements is spent.
//!
//! Refinements run in fixed batches on the bake's threads and are pure
//! functions of the node, so the sites are the same on any thread count; a
//! node already ruled out by the spacing is not refined at all. "Best
//! first" holds between batches, not strictly within one: a node deferred
//! because a better one of the batch lies within its spacing can be ruled
//! out by a weaker node of the same batch when the better one fails its
//! refinement. That costs a little quality, never determinism, since the
//! batch's size is fixed.
//!
//! A mass over the bake's cap is cut into parts at forced divides, and each
//! part places its sites alone: spacing is not kept across a divide, and
//! its neighbour's ground reads as sea to `cove`, `view` and `summit`. Two
//! pads could then meet across it (the sample takes the first). No mass of
//! seed 42 within 3 000 km of the origin reaches the cap.

use strata_core::Rng;
use strata_data::{BiomeId, Facing, ProvinceId, SiteFeature, SiteTerm};

use super::field::parallel_rows;
use super::mass::{Domain, NEIGHBOURS};
use super::{FORD_DEPTH_M, LATTICE_M, Network, NodeMap, RiverEnd, WaterKind, metres};
use crate::sites::{ProceduralSite, SiteId, SiteKind};
use crate::{ProceduralWorld, Sample};

/// Sixteen unit directions, a sixteenth of a turn apart from `+x`, written
/// out so no platform's `cos` decides a site.
const DIRS: [[f64; 2]; 16] = [
    [1.0, 0.0],
    [0.923_879_532_511_286_7, 0.382_683_432_365_089_8],
    [
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ],
    [0.382_683_432_365_089_8, 0.923_879_532_511_286_7],
    [0.0, 1.0],
    [-0.382_683_432_365_089_8, 0.923_879_532_511_286_7],
    [
        -std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ],
    [-0.923_879_532_511_286_7, 0.382_683_432_365_089_8],
    [-1.0, 0.0],
    [-0.923_879_532_511_286_7, -0.382_683_432_365_089_8],
    [
        -std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ],
    [-0.382_683_432_365_089_8, -0.923_879_532_511_286_7],
    [0.0, -1.0],
    [0.382_683_432_365_089_8, -0.923_879_532_511_286_7],
    [
        std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ],
    [0.923_879_532_511_286_7, -0.382_683_432_365_089_8],
];

/// The places round a node a refinement tries, metres: a third of the
/// lattice's spacing apart, so together they cover the node's own square.
const OFFSET_M: f64 = 170.0;

/// The places round the best of those a refinement tries next, metres: a
/// third of [`OFFSET_M`], so they cover its square.
const FINE_OFFSET_M: f64 = OFFSET_M / 3.0;

/// How far a refined place can lie from its node, metres: what the spacing
/// test that rules a node out before refining it allows for.
const SHIFT_M: f64 = (OFFSET_M + FINE_OFFSET_M) * std::f64::consts::SQRT_2 + 1.0;

/// Candidates refined at a time. Fixed, so the work a batch wastes on nodes
/// a site in the same batch rules out is the same on any thread count (the
/// sites are the same either way).
const BATCH: usize = 256;

/// Nodes a survey row holds.
const CHUNK: usize = 1024;

/// What a node or a place below which no place is worth refining scores.
const LEAST_SCORE: f64 = 0.05;

/// Metres clear of a river's bank a pad's ground keeps.
const CHANNEL_CLEAR_M: f32 = 8.0;

/// Metres over the water beside it a pad stands at least.
const FLOOD_M: f64 = 1.0;

/// Height the lattice gives the sea and ground off the mass, metres.
const SEA_H: f64 = -5.0;

/// Metres past a pad's band the approaches are judged at.
const APPROACH_M: f64 = 40.0;

/// How many refinements a kind may spend per site it is after, and a few
/// more: the placement's bound on a mass of poor ground.
const REFINES_PER_SITE: usize = 4;

/// And what every kind may spend on top: a rare kind with strict wants (a
/// town by a confluence) fails most of its first tries.
const REFINES_SPARE: usize = 768;

/// One node's survey.
#[derive(Debug, Clone, Copy)]
struct Spot {
    height: f64,
    slope: f64,
    dry: bool,
    /// Whether a road may pass: over the sea, out of standing water and off
    /// a settlement's ground (the road bake's lattice, `roads`).
    open: bool,
    biomes: [(BiomeId, f32); 2],
    provinces: [(ProvinceId, f32); 2],
}

impl Spot {
    fn of(sample: &Sample) -> Self {
        Self {
            height: f64::from(sample.height),
            slope: f64::from(sample.landform.slope),
            dry: sound(sample) && sample.height >= 0.5,
            open: sample.height >= 0.5
                && sample.standing_water().is_none()
                && sample.landform.developed <= 0.0,
            biomes: sample.biomes,
            provinces: sample.provinces,
        }
    }
}

/// Whether a sample's ground can take a pad: no standing water, no channel
/// within its bank and a few metres, no carved wall, no settlement's ground.
pub(super) fn sound(sample: &Sample) -> bool {
    let channel = sample.water.is_some_and(|water| {
        water.kind != WaterKind::Lake
            && water.signed_distance_m.abs() <= water.bank_m() + CHANNEL_CLEAR_M
    });
    let wall = sample
        .water
        .is_some_and(|water| water.cut_m() > 1.0 && sample.landform.slope > 0.5);
    sample.standing_water().is_none() && !channel && !wall && sample.landform.developed <= 0.0
}

/// Where place `(di, dj)` of a refinement's nine, each `-1..=1`, is kept.
fn slot(di: i32, dj: i32) -> usize {
    usize::try_from((dj + 1) * 3 + di + 1).expect("one of nine")
}

/// The lattice of the mass's surveyed heights, which the road bake routes
/// its first, coarse search over.
pub(super) struct Grid {
    i0: i32,
    j0: i32,
    w: i32,
    h: i32,
    heights: Vec<f64>,
    /// Per slot, whether the node is the mass's and a road may pass it.
    open: Vec<bool>,
}

impl Grid {
    fn new(domain: &Domain) -> Self {
        let (mut i0, mut j0, mut i1, mut j1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for n in &domain.nodes {
            i0 = i0.min(n[0]);
            j0 = j0.min(n[1]);
            i1 = i1.max(n[0]);
            j1 = j1.max(n[1]);
        }
        let (w, h) = (i1 - i0 + 1, j1 - j0 + 1);
        let cells = usize::try_from(w).unwrap_or(0) * usize::try_from(h).unwrap_or(0);
        Self {
            i0,
            j0,
            w,
            h,
            heights: vec![SEA_H; cells],
            open: vec![false; cells],
        }
    }

    /// Whether node `n` is the mass's and open to a road.
    pub(super) fn open(&self, n: [i32; 2]) -> bool {
        self.slot(n).is_some_and(|k| self.open[k])
    }

    /// The surveyed height of node `n`.
    pub(super) fn node_height(&self, n: [i32; 2]) -> f64 {
        self.node(n[0], n[1])
    }

    fn slot(&self, n: [i32; 2]) -> Option<usize> {
        let (i, j) = (n[0] - self.i0, n[1] - self.j0);
        (i >= 0 && j >= 0 && i < self.w && j < self.h)
            .then(|| usize::try_from(j * self.w + i).expect("inside the grid"))
    }

    fn node(&self, i: i32, j: i32) -> f64 {
        self.slot([i, j]).map_or(SEA_H, |k| self.heights[k])
    }

    /// The surveyed height at `(x, z)`, bilinear between nodes.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a world coordinate over the lattice is far inside an i32"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, its lattice cell and the shares across it"
    )]
    fn height(&self, x: f64, z: f64) -> f64 {
        let (u, v) = (x / LATTICE_M, z / LATTICE_M);
        let (fu, fv) = (u.floor(), v.floor());
        let (i, j) = (fu as i32, fv as i32);
        let (s, t) = (u - fu, v - fv);
        let a = self.node(i, j) + (self.node(i + 1, j) - self.node(i, j)) * s;
        let b = self.node(i, j + 1) + (self.node(i + 1, j + 1) - self.node(i, j + 1)) * s;
        a + (b - a) * t
    }

    /// Metres the place stands over the mean of the land 1.5 km round it:
    /// the sea is left out, so a coastal plateau is no summit for standing
    /// over the water beside it, and a headland's hill still is one.
    fn summit(&self, x: f64, z: f64, h0: f64) -> f64 {
        let (mut sum, mut land) = (0.0, 0.0);
        for d in DIRS {
            let h = self.height(x + d[0] * 1_500.0, z + d[1] * 1_500.0);
            if h > 0.0 {
                sum += h;
                land += 1.0;
            }
        }
        if land > 0.0 { h0 - sum / land } else { 0.0 }
    }

    /// The steepest the ground within 4 km rises (or the least it falls)
    /// over the place's eye, rise over run, along direction `k`.
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, a direction and a step along it"
    )]
    fn horizon(&self, x: f64, z: f64, h0: f64, k: usize) -> f64 {
        let d = DIRS[k];
        let mut horizon = f64::NEG_INFINITY;
        for step in 1..=12 {
            let run = 333.0 * f64::from(step);
            let h = self.height(x + d[0] * run, z + d[1] * run).max(0.0);
            horizon = horizon.max((h - h0 - 2.0) / run);
        }
        horizon
    }

    /// The share of the horizon the place looks down on: directions where
    /// nothing within 4 km comes up to its eye.
    fn view(&self, x: f64, z: f64, h0: f64) -> f64 {
        let open = (0..16)
            .filter(|&k| self.horizon(x, z, h0, k) < -0.01)
            .count();
        // A count of sixteen at most.
        #[allow(clippy::cast_precision_loss)]
        let open = open as f64;
        open / 16.0
    }

    /// The most open direction: the lowest horizon, ties to the first.
    fn open_direction(&self, x: f64, z: f64, h0: f64) -> [f64; 2] {
        let mut best = (f64::INFINITY, 0);
        for k in 0..16 {
            let horizon = self.horizon(x, z, h0, k);
            if horizon < best.0 {
                best = (horizon, k);
            }
        }
        DIRS[best.1]
    }

    /// How deep a saddle the place sits in, metres: the ground rising both
    /// ways along one line and falling both ways across it, the lesser of
    /// the two, at 0.7 and 1.2 km, the best line.
    fn pass(&self, x: f64, z: f64, h0: f64) -> f64 {
        let mut best = 0.0_f64;
        for reach in [700.0, 1_200.0] {
            for k in 0..4 {
                let (a, b) = (DIRS[k * 2], DIRS[k * 2 + 4]);
                let at = |d: [f64; 2], s: f64| self.height(x + d[0] * s, z + d[1] * s);
                let rise = at(a, reach).min(at(a, -reach)) - h0;
                let fall = h0 - at(b, reach).max(at(b, -reach));
                best = best.max(rise.min(fall));
            }
        }
        best
    }

    /// The share of the sea's directions that land closes: of the
    /// directions with sea within a kilometre, those that meet land again
    /// within 6 km. A bay or a sound reads high, a straight shore or a
    /// headland nought.
    fn cove(&self, x: f64, z: f64) -> f64 {
        let (mut seaward, mut closed) = (0_u32, 0_u32);
        for d in DIRS {
            let mut wet = None;
            for step in 1..=4 {
                let run = 250.0 * f64::from(step);
                if self.height(x + d[0] * run, z + d[1] * run) < 0.0 {
                    wet = Some(step);
                    break;
                }
            }
            let Some(first) = wet else { continue };
            seaward += 1;
            for step in first + 1..=24 {
                let run = 250.0 * f64::from(step);
                if self.height(x + d[0] * run, z + d[1] * run) > 0.0 {
                    closed += 1;
                    break;
                }
            }
        }
        if seaward < 2 {
            0.0
        } else {
            f64::from(closed) / f64::from(seaward)
        }
    }
}

/// Points in a grid of square buckets, for the nearest within a reach.
struct Points {
    side: f64,
    cells: NodeMap<Vec<u32>>,
    at: Vec<[f64; 2]>,
    size: Vec<f32>,
    tag: Vec<u32>,
}

impl Points {
    fn new(side: f64) -> Self {
        Self {
            side,
            cells: NodeMap::default(),
            at: Vec::new(),
            size: Vec::new(),
            tag: Vec::new(),
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "a world coordinate over a bucket is far inside an i32"
    )]
    fn bucket(&self, x: f64, z: f64) -> [i32; 2] {
        [
            (x / self.side).floor() as i32,
            (z / self.side).floor() as i32,
        ]
    }

    fn push(&mut self, x: f64, z: f64, size: f64, tag: u32) {
        let k = super::local(self.at.len());
        let key = self.bucket(x, z);
        self.cells.entry(key).or_default().push(k);
        self.at.push([x, z]);
        self.size.push(super::single(size));
        self.tag.push(tag);
    }

    /// Many points at once, of no tag: a bucket's entries pushed together,
    /// which is what a few hundred thousand of a network's points cost.
    fn extend(&mut self, items: &[(f64, f64, f64)]) {
        let start = self.at.len();
        for &(x, z, size) in items {
            self.at.push([x, z]);
            self.size.push(super::single(size));
            self.tag.push(0);
        }
        let mut keyed: Vec<([i32; 2], u32)> = (start..self.at.len())
            .map(|k| (self.bucket(self.at[k][0], self.at[k][1]), super::local(k)))
            .collect();
        keyed.sort_unstable();
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            self.cells
                .entry(group[0].0)
                .or_default()
                .extend(group.iter().map(|g| g.1));
        }
    }

    /// The nearest point within `reach` of at least `size` (and of `tag`,
    /// where one is asked for): its distance and index. Buckets are read
    /// in rings outward until no farther ring can hold a nearer point;
    /// ties go to the lower index.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a reach over a bucket is a few hundred at most"
    )]
    fn nearest(
        &self,
        x: f64,
        z: f64,
        reach: f64,
        size: f64,
        tag: Option<u32>,
    ) -> Option<(f64, usize)> {
        if self.at.is_empty() || reach.is_nan() {
            return None;
        }
        let centre = self.bucket(x, z);
        let rings = (reach.min(1e6) / self.side).ceil() as i32 + 1;
        let mut best: Option<(f64, u32)> = None;
        let consider = |k: u32, best: &mut Option<(f64, u32)>| {
            let i = k as usize;
            if f64::from(self.size[i]) < size || tag.is_some_and(|t| self.tag[i] != t) {
                return;
            }
            let (dx, dz) = (self.at[i][0] - x, self.at[i][1] - z);
            let d2 = dx * dx + dz * dz;
            if best.is_none_or(|(b, bk)| d2.total_cmp(&b).then(k.cmp(&bk)).is_lt()) {
                *best = Some((d2, k));
            }
        };
        for r in 0..=rings {
            let mut visit = |i: i32, j: i32| {
                if let Some(list) = self.cells.get(&[centre[0] + i, centre[1] + j]) {
                    for &k in list {
                        consider(k, &mut best);
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
            let ring_floor = f64::from(r) * self.side;
            if best.is_some_and(|(d2, _)| d2 <= ring_floor * ring_floor) {
                break;
            }
        }
        best.map(|(d2, k)| (d2.sqrt(), k as usize))
            .filter(|&(d, _)| d <= reach)
    }

    /// Every point within `reach`, by index.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a reach over a bucket is a few hundred at most"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "a point and a bucket's offsets"
    )]
    fn within(&self, x: f64, z: f64, reach: f64, mut keep: impl FnMut(usize, f64) -> bool) -> bool {
        let centre = self.bucket(x, z);
        let r = (reach / self.side).ceil() as i32;
        for j in -r..=r {
            for i in -r..=r {
                if let Some(list) = self.cells.get(&[centre[0] + i, centre[1] + j]) {
                    for &k in list {
                        let p = self.at[k as usize];
                        let (dx, dz) = (p[0] - x, p[1] - z);
                        let d = (dx * dx + dz * dz).sqrt();
                        if d <= reach && !keep(k as usize, d) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// The sites placed so far: all of them, tagged by kind, for the spacing;
/// and each kind's, in buckets of its own spacing, for a term's distance.
struct Placed {
    all: Points,
    by_kind: Vec<Points>,
}

/// The features a term measures a distance to.
struct Features {
    sea: Points,
    river: Points,
    lake: Points,
    confluence: Points,
    mouth: Points,
    ford: Points,
    fall: Points,
    gorge: Points,
}

impl Features {
    fn get(&self, feature: SiteFeature) -> Option<&Points> {
        Some(match feature {
            SiteFeature::Sea => &self.sea,
            SiteFeature::River => &self.river,
            SiteFeature::Lake => &self.lake,
            SiteFeature::Confluence => &self.confluence,
            SiteFeature::Mouth => &self.mouth,
            SiteFeature::Ford => &self.ford,
            SiteFeature::Fall => &self.fall,
            SiteFeature::Gorge => &self.gorge,
            _ => return None,
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "each feature from the network, in turn"
    )]
    fn new(
        world: &ProceduralWorld,
        network: &Network,
        domain: &Domain,
        grid: &Grid,
        threads: usize,
    ) -> Self {
        let params = &network.params;
        let mut river = Points::new(512.0);
        let mut ford = Points::new(512.0);
        let mut mouth = Points::new(1_024.0);
        let mut confluence = Points::new(1_024.0);
        let mut fall = Points::new(1_024.0);
        let mut gorge = Points::new(512.0);
        let mut arms: Vec<(u32, f64)> = Vec::new();
        // Each reach's channel and fords on the bake's threads, in order.
        let reaches = network.rivers.len().div_ceil(64);
        let lines = parallel_rows(reaches, threads, |row| {
            network.rivers[row * 64..((row + 1) * 64).min(network.rivers.len())]
                .iter()
                .map(|r| {
                    let v = &r.vertices;
                    // Where each vertex lies along the line, for the falls'
                    // stretches, which hold no ford.
                    let mut along = vec![0.0; v.len()];
                    for k in 1..v.len() {
                        let (dx, dz) = (v[k].x - v[k - 1].x, v[k].z - v[k - 1].z);
                        along[k] = along[k - 1] + (dx * dx + dz * dz).sqrt();
                    }
                    let total = along.last().copied().unwrap_or(0.0);
                    let falls: Vec<(f64, f64)> = (0..v.len().saturating_sub(1))
                        .filter(|&k| v[k].fall)
                        .map(|k| (along[k] - 50.0, along[k + 1] + 50.0))
                        .collect();
                    // A point every hundred metres or so: the centreline
                    // puts one at every vertex too, which doubles them.
                    let mut kept = f64::NEG_INFINITY;
                    r.centreline(params, 100.0)
                        .into_iter()
                        .filter(|p| {
                            let keep = p.along_m - kept >= 90.0;
                            if keep {
                                kept = p.along_m;
                            }
                            keep
                        })
                        .map(|p| {
                            let on_fall =
                                falls.iter().any(|&(a, b)| p.along_m >= a && p.along_m <= b);
                            // A mouth's last stretch is the sea's.
                            let at_sea = r.end == RiverEnd::Sea && p.along_m > total - 300.0;
                            let fords = p.depth_m <= FORD_DEPTH_M
                                && p.width_m >= 1.0
                                && !on_fall
                                && !at_sea;
                            (p.x, p.z, p.width_m, fords)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });
        let (mut channel, mut shallows) = (Vec::new(), Vec::new());
        for (r, line) in network.rivers.iter().zip(lines.into_iter().flatten()) {
            let v = &r.vertices;
            for (x, z, width, fords) in line {
                channel.push((x, z, width));
                if fords {
                    shallows.push((x, z, width));
                }
            }
            for vertex in v {
                if vertex.cut_m >= 20.0 {
                    gorge.push(vertex.x, vertex.z, f64::from(vertex.cut_m), 0);
                }
            }
            match (r.end, v.last()) {
                (RiverEnd::Sea, Some(last)) => {
                    mouth.push(last.x, last.z, f64::from(last.width_m), 0);
                }
                (RiverEnd::Joins(next), Some(last)) => arms.push((next, f64::from(last.width_m))),
                _ => {}
            }
        }
        river.extend(&channel);
        ford.extend(&shallows);
        arms.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut k = 0;
        while k < arms.len() {
            let next = arms[k].0;
            // The smaller arm's width, the list sorted by it within a joint.
            let smaller = arms[k].1;
            while k < arms.len() && arms[k].0 == next {
                k += 1;
            }
            if let Some(first) = network
                .rivers
                .get(next as usize)
                .and_then(|r| r.vertices.first())
            {
                confluence.push(first.x, first.z, smaller, 0);
            }
        }
        for f in &network.falls {
            fall.push(f.top[0], f.top[2], f.top[1] - f.bottom[1], 0);
        }
        let mut lake = Points::new(512.0);
        for l in &network.lakes {
            let side = super::real(l.nodes.len()).sqrt() * LATTICE_M;
            let mut nodes = l.nodes.clone();
            nodes.sort_unstable();
            for n in &l.nodes {
                for d in &NEIGHBOURS[..4] {
                    let m = [n[0] + d[0], n[1] + d[1]];
                    if nodes.binary_search(&m).is_err() {
                        lake.push(
                            f64::midpoint(metres(n[0]), metres(m[0])),
                            f64::midpoint(metres(n[1]), metres(m[1])),
                            side,
                            0,
                        );
                    }
                }
            }
        }
        // The shore on the natural ground, between each node of the mass
        // and each sea node beside it on a side: halved three times, so to
        // 64 m, the coarse lattice's coast being up to a node off the real.
        let pairs: Vec<([i32; 2], [i32; 2])> = domain
            .nodes
            .iter()
            .flat_map(|&n| {
                NEIGHBOURS[..4]
                    .iter()
                    .map(move |d| (n, [n[0] + d[0], n[1] + d[1]]))
            })
            .filter(|&(_, m)| grid.slot(m).is_none_or(|k| grid.heights[k] <= SEA_H))
            .collect();
        let rows = pairs.len().div_ceil(CHUNK);
        let shores = parallel_rows(rows, threads, |row| {
            let mut held: Option<([i32; 2], f64)> = None;
            pairs[row * CHUNK..((row + 1) * CHUNK).min(pairs.len())]
                .iter()
                .filter_map(|&(n, m)| {
                    let a = [metres(n[0]), metres(n[1])];
                    let b = [metres(m[0]), metres(m[1])];
                    // A node's pairs are together: its own height once.
                    let ha = match held {
                        Some((at, h)) if at == n => h,
                        _ => {
                            let h = world.natural_height(a[0], a[1]);
                            held = Some((n, h));
                            h
                        }
                    };
                    if ha < 0.0 || world.natural_height(b[0], b[1]) >= 0.0 {
                        return None;
                    }
                    let (mut lo, mut hi) = (0.0, 1.0);
                    for _ in 0..3 {
                        let mid = f64::midpoint(lo, hi);
                        let p = [a[0] + (b[0] - a[0]) * mid, a[1] + (b[1] - a[1]) * mid];
                        if world.natural_height(p[0], p[1]) >= 0.0 {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let t = f64::midpoint(lo, hi);
                    Some([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
                })
                .collect::<Vec<_>>()
        });
        let mut sea = Points::new(512.0);
        for p in shores.into_iter().flatten() {
            sea.push(p[0], p[1], 0.0, 0);
        }
        Self {
            sea,
            river,
            lake,
            confluence,
            mouth,
            ford,
            fall,
            gorge,
        }
    }
}

/// A refined place: a pad that passed.
#[derive(Debug, Clone, Copy)]
struct Refined {
    centre: [f64; 2],
    level: f64,
    facing: [f64; 2],
    arrival: [f64; 2],
    by: Option<SiteFeature>,
    score: f64,
    cut: f64,
}

/// What a placement reads.
struct Placer<'a> {
    world: &'a ProceduralWorld,
    network: &'a Network,
    domain: &'a Domain,
    grid: Grid,
    survey: Vec<Spot>,
    features: Features,
    kinds: &'a [SiteKind],
    /// Per kind, its wants in the order they are scored: the required ones
    /// first, cheapest first, so a place one rules out costs little.
    order: Vec<Vec<usize>>,
    seed: u64,
}

/// What scoring a term costs, roughly: a view marches the horizon, a cove
/// the sea's, the rest read a point or two.
fn cost(term: &SiteTerm) -> u32 {
    match term.feature {
        SiteFeature::View => 3,
        SiteFeature::Cove => 2,
        _ => 1,
    }
}

/// Places the sites of the mass `network` was baked from (see the module
/// docs), and hands back the surveyed lattice for the roads. Nothing for a
/// world with no site kinds.
pub(crate) fn place(
    world: &ProceduralWorld,
    network: &Network,
    domain: &Domain,
    seed: strata_core::Seed,
    threads: usize,
) -> (Vec<ProceduralSite>, Option<Grid>) {
    let kinds = world.site_kinds();
    if kinds.is_empty() || domain.nodes.is_empty() {
        return (Vec::new(), None);
    }
    let rows = domain.nodes.len().div_ceil(CHUNK);
    let survey: Vec<Spot> = parallel_rows(rows, threads, |row| {
        domain.nodes[row * CHUNK..((row + 1) * CHUNK).min(domain.nodes.len())]
            .iter()
            .map(|n| Spot::of(&world.sample_by(metres(n[0]), metres(n[1]), network)))
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    let mut grid = Grid::new(domain);
    for (n, spot) in domain.nodes.iter().zip(&survey) {
        let k = grid.slot(*n).expect("a node of the mass is in its grid");
        grid.heights[k] = spot.height;
        grid.open[k] = spot.open
            && world
                .sites()
                .iter()
                .all(|site| site.extent_distance_at(metres(n[0]), metres(n[1])) >= 0.0);
    }
    let features = Features::new(world, network, domain, &grid, threads);
    let placer = Placer {
        world,
        network,
        domain,
        grid,
        survey,
        features,
        kinds,
        order: kinds
            .iter()
            .map(|kind| {
                let wants = &kind.traits.wants;
                let mut order: Vec<usize> = (0..wants.len()).collect();
                order.sort_by_key(|&w| {
                    let most = wants[w].any.iter().map(cost).max().unwrap_or(0);
                    (!wants[w].required, most)
                });
                order
            })
            .collect(),
        seed: seed.derive("sites"),
    };
    let sites = placer.run(threads);
    (sites, Some(placer.grid))
}

impl Placer<'_> {
    fn run(&self, threads: usize) -> Vec<ProceduralSite> {
        let area_km2 = super::real(self.domain.nodes.len()) * LATTICE_M * LATTICE_M / 1e6;
        let key = self.network.key;
        let mut placed: Vec<(usize, Refined, [i32; 2])> = Vec::new();
        let mut at = Placed {
            all: Points::new(4_096.0),
            by_kind: self
                .kinds
                .iter()
                .map(|kind| Points::new(kind.def.spacing.own_m.clamp(2_000.0, 10_000.0)))
                .collect(),
        };
        for (index, kind) in self.kinds.iter().enumerate() {
            let def = &kind.def;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a count of sites, rounded from a small positive number"
            )]
            let target = (def.per_1000_km2 * area_km2 / 1_000.0).round().max(0.0) as usize;
            if target == 0 {
                continue;
            }
            let rows = self.domain.nodes.len().div_ceil(CHUNK);
            let mut candidates: Vec<(f64, [i32; 2])> = parallel_rows(rows, threads, |row| {
                let from = row * CHUNK;
                (from..((row + 1) * CHUNK).min(self.domain.nodes.len()))
                    .filter_map(|k| {
                        self.node_score(kind, k, &at)
                            .filter(|&s| s >= LEAST_SCORE)
                            .map(|s| (s, self.domain.nodes[k]))
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .flatten()
            .collect();
            candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let budget = target * REFINES_PER_SITE + REFINES_SPARE;
            let (mut taken, mut refined, mut next) = (0, 0, 0);
            let mut alive = vec![true; candidates.len()];
            let at_node = |k: usize| {
                let n = candidates[k].1;
                [metres(n[0]), metres(n[1])]
            };
            // Two nodes of a batch closer than this cannot both be taken,
            // however far their refinements move them.
            let reserve = def.spacing.own_m - 2.0 * SHIFT_M;
            while taken < target && refined < budget {
                while next < candidates.len() && !alive[next] {
                    next += 1;
                }
                if next >= candidates.len() {
                    break;
                }
                // A batch of nodes the sites so far leave room for, none
                // ruling out another: a node too near one already in the
                // batch waits for a later one.
                let mut batch: Vec<usize> = Vec::with_capacity(BATCH);
                let window = (next + BATCH * 16).min(candidates.len());
                let mut k = next;
                while batch.len() < BATCH && k < window && refined + batch.len() < budget {
                    if alive[k] {
                        let p = at_node(k);
                        if !self.clear(index, p, &at, SHIFT_M) {
                            alive[k] = false;
                        } else if !batch.iter().any(|&b| {
                            let q = at_node(b);
                            (q[0] - p[0]) * (q[0] - p[0]) + (q[1] - p[1]) * (q[1] - p[1])
                                < reserve * reserve
                        }) {
                            batch.push(k);
                            alive[k] = false;
                        }
                    }
                    k += 1;
                }
                refined += batch.len();
                let results = parallel_rows(batch.len(), threads, |b| {
                    self.refine(kind, candidates[batch[b]].1, &at)
                });
                for (&b, result) in batch.iter().zip(results) {
                    if taken >= target {
                        break;
                    }
                    let Some(place) = result else { continue };
                    if !self.clear(index, place.centre, &at, 0.0) {
                        continue;
                    }
                    at.all.push(
                        place.centre[0],
                        place.centre[1],
                        def.footprint_m + def.blend_m,
                        super::local(index),
                    );
                    at.by_kind[index].push(place.centre[0], place.centre[1], 0.0, 0);
                    placed.push((index, place, candidates[b].1));
                    taken += 1;
                }
            }
        }
        self.name(placed, key)
    }

    /// Whether a site of kind `index` at `p` keeps its spacing from every
    /// site placed so far and from the authored settlements, `slack` metres
    /// granted to a place a refinement may yet move.
    fn clear(&self, index: usize, p: [f64; 2], at: &Placed, slack: f64) -> bool {
        let def = &self.kinds[index].def;
        let settled =
            self.world.sites().iter().all(|site| {
                site.extent_distance_at(p[0], p[1]) + slack >= def.spacing.settlements_m
            });
        if !settled {
            return false;
        }
        let outer = def.footprint_m + def.blend_m;
        let most = self
            .kinds
            .iter()
            .map(|k| k.def.spacing.own_m.max(k.def.spacing.others_m))
            .fold(def.spacing.own_m, f64::max);
        at.all.within(p[0], p[1], most, |k, d| {
            let other = at.all.tag[k] as usize;
            let other_def = &self.kinds[other].def;
            let keep = if other == index {
                def.spacing.own_m
            } else {
                def.spacing.others_m.max(other_def.spacing.others_m)
            }
            .max(outer + f64::from(at.all.size[k]) + 20.0);
            d + slack >= keep
        })
    }

    /// A node's score for a kind, or `None` where its ground rules it out.
    #[allow(
        clippy::many_single_char_names,
        reason = "a node, its point and the ground's fall across it"
    )]
    fn node_score(&self, kind: &SiteKind, k: usize, at: &Placed) -> Option<f64> {
        let spot = &self.survey[k];
        let ground = &kind.def.ground;
        if !spot.dry
            || spot.height < ground.height_m.min - 10.0
            || spot.height > ground.height_m.max + 10.0
            || spot.slope > ground.slope_max * 1.5
        {
            return None;
        }
        let weight = ground_weight(kind, spot);
        if weight <= 0.0 {
            return None;
        }
        let n = self.domain.nodes[k];
        // A hillside at the lattice's scale: the ground across the node
        // falls too steeply both ways for a pad anywhere in its square.
        let g = &self.grid;
        let (gx, gz) = (
            (g.node(n[0] + 1, n[1]) - g.node(n[0] - 1, n[1])) / (2.0 * LATTICE_M),
            (g.node(n[0], n[1] + 1) - g.node(n[0], n[1] - 1)) / (2.0 * LATTICE_M),
        );
        if gx * gx + gz * gz > 4.0 * ground.slope_max * ground.slope_max {
            return None;
        }
        let (x, z) = (metres(n[0]), metres(n[1]));
        if self
            .world
            .sites()
            .iter()
            .any(|site| site.extent_distance_at(x, z) + SHIFT_M < kind.def.spacing.settlements_m)
        {
            return None;
        }
        let (wants, _) = self.wants(kind, x, z, spot.height, at)?;
        let level = 1.0 - 0.6 * (spot.slope / ground.slope_max).min(1.0);
        Some(wants * weight * level * self.jitter(kind, n))
    }

    /// Up to 3 % off a score by a hash of the kind and the node: equal
    /// scores on a plain are common, and a sweep from the lowest coordinate
    /// would lay them out in rows.
    fn jitter(&self, kind: &SiteKind, n: [i32; 2]) -> f64 {
        let mut state = self.seed ^ u64::from(kind.id.0).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        state ^= u64::from(n[0].cast_unsigned()).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        state ^= u64::from(n[1].cast_unsigned()).wrapping_mul(0x94d0_49bb_1331_11eb);
        let bits = strata_core::mix64(state) >> 11;
        // 53 bits, exact in an f64.
        #[allow(clippy::cast_precision_loss)]
        let unit = bits as f64 / (1_u64 << 53) as f64;
        0.97 + 0.03 * unit
    }

    /// The kind's wants at a place: the weighted mean of each want's best
    /// term, and the feature that decided most (what the site stands by);
    /// `None` where a required want scores nought.
    fn wants(
        &self,
        kind: &SiteKind,
        x: f64,
        z: f64,
        h0: f64,
        at: &Placed,
    ) -> Option<(f64, Option<SiteFeature>)> {
        let (mut sum, mut total) = (0.0, 0.0);
        let mut by: (f64, Option<SiteFeature>) = (0.0, None);
        let index = self
            .kinds
            .iter()
            .position(|k| k.id == kind.id)
            .expect("a kind of this placement");
        for &w in &self.order[index] {
            let want = &kind.traits.wants[w];
            let mut best = (0.0_f64, None);
            // Cheapest first, and none more once one scores in full.
            for cost_of in 1..=3 {
                for term in want.any.iter().filter(|term| cost(term) == cost_of) {
                    if best.0 >= 1.0 {
                        break;
                    }
                    let score = self.term(term, x, z, h0, at);
                    if score > best.0 {
                        best = (score, Some(term.feature));
                    }
                }
            }
            if want.required && best.0 <= 0.0 {
                return None;
            }
            sum += want.weight * best.0;
            total += want.weight;
            let names = !matches!(
                best.1,
                Some(SiteFeature::Kind | SiteFeature::Settlement | SiteFeature::View) | None
            );
            if names && want.weight * best.0 > by.0 {
                by = (want.weight * best.0, best.1);
            }
        }
        let score = if total > 0.0 { sum / total } else { 1.0 };
        Some((score, by.1))
    }

    fn term(&self, term: &SiteTerm, x: f64, z: f64, h0: f64, at: &Placed) -> f64 {
        let reach = if term.within_m.is_finite() {
            term.reach_m()
        } else {
            term.beyond_m
        };
        let distance = |found: Option<(f64, usize)>| found.map_or(f64::INFINITY, |(d, _)| d);
        match term.feature {
            SiteFeature::Summit => term.of_measure(self.grid.summit(x, z, h0)),
            SiteFeature::View => term.of_measure(self.grid.view(x, z, h0)),
            SiteFeature::Pass => term.of_measure(self.grid.pass(x, z, h0)),
            // A cove is the sea's: inland it is nought, and its march is
            // the dearest measure.
            SiteFeature::Cove => {
                if self
                    .features
                    .sea
                    .nearest(x, z, 1_000.0, 0.0, None)
                    .is_none()
                {
                    0.0
                } else {
                    term.of_measure(self.grid.cove(x, z))
                }
            }
            SiteFeature::Settlement => {
                let d = self
                    .world
                    .sites()
                    .iter()
                    .map(|site| site.extent_distance_at(x, z).max(0.0))
                    .fold(f64::INFINITY, f64::min);
                term.of_distance(d)
            }
            SiteFeature::Kind => {
                let placed = term
                    .kind
                    .and_then(|id| self.kinds.iter().position(|k| k.id == id))
                    .map(|index| &at.by_kind[index]);
                term.of_distance(distance(
                    placed.and_then(|points| points.nearest(x, z, reach, 0.0, None)),
                ))
            }
            feature => {
                let points = self
                    .features
                    .get(feature)
                    .expect("every other feature has points");
                term.of_distance(distance(points.nearest(x, z, reach, term.size_m, None)))
            }
        }
    }

    /// The nine places `step` apart round `c` that the kind's ground limits
    /// allow, each with its score, sample and the feature it stands by.
    /// Each place's fall to its neighbours is the ground's slope at a pad's
    /// scale, which rules out most hillsides before a footprint is sampled.
    fn score_nine(
        &self,
        kind: &SiteKind,
        c: [f64; 2],
        step: f64,
        at: &Placed,
    ) -> Vec<(f64, [f64; 2], Sample, Option<SiteFeature>)> {
        let ground = &kind.def.ground;
        let place = |di: i32, dj: i32| [c[0] + f64::from(di) * step, c[1] + f64::from(dj) * step];
        let mut nine = [None; 9];
        for dj in -1..=1_i32 {
            for di in -1..=1_i32 {
                let p = place(di, dj);
                nine[slot(di, dj)] = Some(self.sample(p[0], p[1]));
            }
        }
        let nine = nine.map(|s| s.expect("all nine sampled"));
        let mut places = Vec::with_capacity(9);
        for dj in -1..=1_i32 {
            for di in -1..=1_i32 {
                let [x, z] = place(di, dj);
                let sample = nine[slot(di, dj)];
                let broad = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .into_iter()
                    .filter(|&(a, b)| (-1..=1).contains(&(di + a)) && (-1..=1).contains(&(dj + b)))
                    .map(|(a, b)| (nine[slot(di + a, dj + b)].height - sample.height).abs())
                    .fold(0.0_f32, f32::max);
                if f64::from(broad) > 1.6 * ground.slope_max * step {
                    continue;
                }
                let h = f64::from(sample.height);
                let slope = f64::from(sample.landform.slope);
                if !sound(&sample)
                    || h < ground.height_m.min.max(0.5)
                    || h > ground.height_m.max
                    || slope > ground.slope_max
                {
                    continue;
                }
                let weight = ground_weight(kind, &Spot::of(&sample));
                if weight <= 0.0 {
                    continue;
                }
                let Some((wants, by)) = self.wants(kind, x, z, h, at) else {
                    continue;
                };
                let score = wants * weight * (1.0 - 0.5 * (slope / ground.slope_max).min(1.0));
                if score >= LEAST_SCORE {
                    places.push((score, [x, z], sample, by));
                }
            }
        }
        places
    }

    /// The node `n` refined to the real ground for `kind`: the best of the
    /// places round it ([`score_nine`](Self::score_nine), coarse then fine)
    /// whose pad passes, or `None`.
    #[allow(
        clippy::too_many_lines,
        reason = "the places, the pad's tests and its facing, in order"
    )]
    fn refine(&self, kind: &SiteKind, n: [i32; 2], at: &Placed) -> Option<Refined> {
        let def = &kind.def;
        let ground = &def.ground;
        // Nine places round the node a third of the lattice apart, then
        // nine round the best of them a third of that apart: the coarse
        // step finds the right ground, the fine one brings a site up to the
        // water or into the saddle its wants are about, which a 170 m step
        // misses by half of it.
        let mut places = self.score_nine(kind, [metres(n[0]), metres(n[1])], OFFSET_M, at);
        places.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let Some(best) = places.first().map(|p| p.1) {
            let mut fine = self.score_nine(kind, best, FINE_OFFSET_M, at);
            // The best coarse place is the fine grid's centre again.
            fine.retain(|p| {
                p.1[0].to_bits() != best[0].to_bits() || p.1[1].to_bits() != best[1].to_bits()
            });
            places.extend(fine);
        }
        // Best first; the order of the offsets breaks ties.
        places.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (r, blend) = (def.footprint_m, def.blend_m);
        for (score, c, centre, by) in places.into_iter().take(3) {
            // No channel within the band: the line's nearest reach, its
            // bank, the most a meander of a river its size swings off the
            // line (`bend_most_m` is the largest river's: a creek's bends
            // are a few metres, and holding a ford's hamlet 30 m further off
            // kept it out of sight of its water) and the margin.
            if let Some(water) = self.network.index.nearest(&self.network.params, c[0], c[1])
                && water.kind != WaterKind::Lake
            {
                let params = &self.network.params;
                let bank = params.bank_top_m(f64::from(water.width_m), f64::from(water.depth_m));
                let swing = params.bend_amplitude(f64::from(water.discharge_m3s));
                let clear = r + blend + bank + swing + f64::from(CHANNEL_CLEAR_M);
                if f64::from(water.signed_distance_m.abs()) < clear {
                    continue;
                }
            }
            // The footprint: its edge first (where most places fail, on
            // its slope), then a ring at half the radius; the band halfway
            // out last.
            let mut level_floor = ground.height_m.min.max(0.5);
            let mut look = |s: &Sample, in_pad: bool| -> bool {
                if let Some(water) = s.water {
                    level_floor = level_floor.max(f64::from(water.surface_m) + FLOOD_M);
                }
                sound(s) && (!in_pad || s.height >= 0.5)
            };
            if !look(&centre, true) {
                continue;
            }
            let mut edge = [0.0; 16];
            let mut ok = true;
            // Two diameters across first: most pads that fail, fail on
            // them.
            for (n, k) in [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15]
                .into_iter()
                .enumerate()
            {
                let d = DIRS[k];
                let s = self.sample(c[0] + d[0] * r, c[1] + d[1] * r);
                edge[k] = f64::from(s.height);
                if !look(&s, true) {
                    ok = false;
                    break;
                }
                if n % 2 == 1 && (edge[k] - edge[k - 8]).abs() > 2.0 * r * ground.slope_max {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let slope = (0..8)
                .map(|k| (edge[k] - edge[k + 8]).abs() / (2.0 * r))
                .fold(0.0, f64::max);
            if slope > ground.slope_max {
                continue;
            }
            let mut pad: Vec<(f64, f64)> = vec![(0.0, f64::from(centre.height))];
            pad.extend(edge.iter().map(|&h| (r, h)));
            for k in (0..16).step_by(4) {
                let d = DIRS[k + 2];
                let s = self.sample(c[0] + d[0] * r * 0.55, c[1] + d[1] * r * 0.55);
                ok &= look(&s, true);
                pad.push((r * 0.55, f64::from(s.height)));
            }
            // The band at three fifths of its width in eight directions
            // and near its end in the eight between them: a lake's shore or
            // a creek's bank in the band would be moved by the blend. The
            // sea may reach the band of a kind that faces the water (the
            // blend makes a little made ground into the shallows there) but
            // not one that does not.
            let out = r + 0.6 * blend;
            let end = r + 0.95 * blend;
            let shore_ok = matches!(kind.traits.facing, Facing::Water);
            let mut band = [0.0; 8];
            for (k, slot) in band.iter_mut().enumerate() {
                for (i, (d, dist)) in [(DIRS[k * 2], out), (DIRS[k * 2 + 1], end)]
                    .into_iter()
                    .enumerate()
                {
                    if !ok {
                        break;
                    }
                    let s = self.sample(c[0] + d[0] * dist, c[1] + d[1] * dist);
                    ok &= look(&s, false) && (shore_ok || s.height >= 0.0);
                    if i == 0 {
                        *slot = f64::from(s.height);
                    }
                }
            }
            if !ok {
                continue;
            }
            let mean = pad.iter().map(|p| p.1).sum::<f64>() / super::real(pad.len());
            let level = mean.max(level_floor);
            if level > ground.height_m.max {
                continue;
            }
            let crown = def.crown;
            let target = |d: f64| {
                if d <= r {
                    level - 0.5 * crown * d * d / r
                } else {
                    level - crown * (0.5 * r + (d - r))
                }
            };
            let cut = pad
                .iter()
                .map(|&(d, h)| (h - target(d)).abs())
                .fold(0.0, f64::max);
            let band_cut = band
                .iter()
                .map(|&h| (h - target(out)).abs())
                .fold(0.0, f64::max);
            if cut > def.max_cut_m || band_cut > 2.0 * def.max_cut_m {
                continue;
            }
            // The approaches: the ring past the band, the gentlest way up
            // to the pad that crosses no water, ties to the first.
            let reach = r + blend + APPROACH_M;
            let mut best = (f64::INFINITY, 0);
            let mut seaward = [0.0, 0.0];
            for (k, d) in DIRS.iter().enumerate().step_by(2) {
                let s = self.sample(c[0] + d[0] * reach, c[1] + d[1] * reach);
                let mut cost = (f64::from(s.height) - level).abs() / (blend + APPROACH_M);
                if s.height < 0.0 {
                    seaward = [seaward[0] + d[0], seaward[1] + d[1]];
                    cost += 10.0;
                } else if !sound(&s) {
                    cost += 10.0;
                }
                if cost < best.0 {
                    best = (cost, k);
                }
            }
            let arrival = [c[0] + DIRS[best.1][0] * r, c[1] + DIRS[best.1][1] * r];
            let facing = self.facing(kind, c, level, &edge, seaward);
            let quality =
                (1.0 - 0.5 * cut / def.max_cut_m) * (1.0 - 0.5 * slope / ground.slope_max);
            return Some(Refined {
                centre: c,
                level,
                facing,
                arrival,
                by,
                score: score * quality,
                cut,
            });
        }
        None
    }

    /// Which way the site looks, by its kind's rule: to the nearest water
    /// (the sea it sees first), to the most open horizon, or down the fall
    /// of its footprint; whichever is not there gives way to the next.
    fn facing(
        &self,
        kind: &SiteKind,
        c: [f64; 2],
        level: f64,
        edge: &[f64; 16],
        seaward: [f64; 2],
    ) -> [f64; 2] {
        let unit = |v: [f64; 2]| {
            let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
            (length > 1e-9).then(|| [v[0] / length, v[1] / length])
        };
        let downhill = || {
            let (mut gx, mut gz) = (0.0, 0.0);
            for (k, d) in DIRS.iter().enumerate() {
                gx += edge[k] * d[0];
                gz += edge[k] * d[1];
            }
            // A fall of a centimetre across the pad is no direction.
            if gx * gx + gz * gz < 1e-4 {
                None
            } else {
                unit([-gx, -gz])
            }
        };
        let view = || self.grid.open_direction(c[0], c[1], level);
        match kind.traits.facing {
            Facing::Water => unit(seaward)
                .or_else(|| {
                    let mut best: Option<(f64, [f64; 2])> = None;
                    for (points, size) in [
                        (&self.features.sea, 0.0),
                        (&self.features.lake, 0.0),
                        (&self.features.river, 1.5),
                    ] {
                        if let Some((d, k)) = points.nearest(c[0], c[1], 4_000.0, size, None)
                            && best.is_none_or(|(b, _)| d < b)
                        {
                            best = Some((d, points.at[k]));
                        }
                    }
                    best.and_then(|(_, p)| unit([p[0] - c[0], p[1] - c[1]]))
                })
                .or_else(downhill)
                .unwrap_or_else(view),
            Facing::View => view(),
            Facing::Downhill => downhill().unwrap_or_else(view),
        }
    }

    fn sample(&self, x: f64, z: f64) -> Sample {
        self.world.sample_by(x, z, self.network)
    }

    /// The placed sites with their names: in the order placed, each from
    /// its kind's rules, a hash of the seed and its id, drawn again until
    /// no site of the mass has it.
    fn name(&self, placed: Vec<(usize, Refined, [i32; 2])>, key: [i32; 2]) -> Vec<ProceduralSite> {
        // The authored settlements' names are taken before any site's: no
        // generated "Port Serein" beside the real one.
        let mut taken: Vec<String> = self.world.settlement_names().to_vec();
        taken.sort();
        let mut out = Vec::with_capacity(placed.len());
        for (index, place, node) in placed {
            let kind = &self.kinds[index];
            let id = SiteId {
                kind: kind.id,
                mass: key,
                node,
            };
            let patterns = place
                .by
                .and_then(|feature| {
                    kind.traits
                        .by_feature
                        .iter()
                        .find(|(f, _)| *f == feature)
                        .map(|(_, p)| p.as_slice())
                })
                .unwrap_or(&kind.def.names.patterns);
            let mut rng = Rng::new(self.seed ^ id.key());
            let mut name = crate::sites::name(&kind.def.names, patterns, &mut rng);
            let mut tries = 0;
            while taken.binary_search(&name).is_ok() {
                tries += 1;
                name = if tries < 64 {
                    crate::sites::name(&kind.def.names, patterns, &mut rng)
                } else {
                    format!("{name} {tries}")
                };
            }
            let (Ok(at) | Err(at)) = taken.binary_search(&name);
            taken.insert(at, name.clone());
            out.push(ProceduralSite {
                id,
                name,
                centre: place.centre,
                level_m: place.level,
                radius_m: kind.def.footprint_m,
                blend_m: kind.def.blend_m,
                crown: kind.def.crown,
                facing: place.facing,
                arrival: place.arrival,
                by: place.by,
                score: place.score,
                cut_m: place.cut,
            });
        }
        out
    }
}

/// The ground's weight for a kind: its biomes' and provinces' weights by
/// their shares.
fn ground_weight(kind: &SiteKind, spot: &Spot) -> f64 {
    let biomes: f64 = spot
        .biomes
        .iter()
        .map(|&(id, share)| {
            f64::from(share)
                * kind
                    .traits
                    .biomes
                    .get(id.0 as usize)
                    .copied()
                    .unwrap_or(1.0)
        })
        .sum();
    let provinces: f64 = if kind.traits.provinces.is_empty() || spot.provinces[0].0.is_none() {
        1.0
    } else {
        spot.provinces
            .iter()
            .map(|&(id, share)| {
                f64::from(share)
                    * kind
                        .traits
                        .provinces
                        .get(id.0 as usize)
                        .copied()
                        .unwrap_or(1.0)
            })
            .sum()
    };
    biomes * provinces
}
