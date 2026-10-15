//! The search on a valley's walls round a graded road's bridge (ticket
//! 0005, alignment pass, round 3).
//!
//! The fine search's nodes are [`FINE_M`] apart. Where a river runs in a
//! trench whose walls are one or two of them wide, every line it can lay
//! comes down a wall in one or two edges, and the profile then cuts what
//! they fall past the grade: the showcase's first bridge was reached down
//! a 44 % edge through a 25 m cutting. So round a bridge whose searched
//! line falls well past its grade near the crossing ([`STEEP_EARTH`]), the
//! line between two of its nodes [`REACH_M`] or more along it either side
//! is laid again: a deck across the river near where the searched line
//! crosses it ([`SLIDES`]), at the bearing of [`TURNS`] that costs least,
//! held straight past its shelves as the shaping holds it, and from the end
//! of each straight a search on nodes [`WALL_M`] apart out to the window's
//! end on that side (`Builder::side`), where a line can slant down a wall
//! at its grade and come onto the deck at its level.
//!
//! The costs are the fine search's (one move's travel, side slope, texture
//! and belt by `Class::step`, its turns by `Class::turn_cost`, the deck as
//! `Builder::section` reads it and the kit stands it) but for the
//! earthwork, which the ways out carry from the deck as a profile. Priced
//! edge by edge, as the fine search prices it, a fall cut into short edges
//! costs less than the same fall in one, so a finer search comes straight
//! down a wall's foot where the profile then cuts deepest (the first
//! attempt here did, with the wedge scaled to the edge). The searched line
//! through the window is priced the same way, and a window keeps it unless
//! a deck and its ways out cost less.
//!
//! A window keeps to the road's own nodes between its other crossings and
//! its ends, and its ways out run through every node another road ends on,
//! so each junction stands where the fine search laid it (the shaping then
//! moves it as it moves any node, a few metres). The ways out cross no
//! channel and no other road's line, and keep a little off them; they bend
//! no tighter than [`BEND`] of the class's radius (twice that near the
//! deck) and pay the fine search's hairpin for a reversal within one of its
//! moves; so the rest of the line is unchanged and the whole is shaped as
//! before.

use std::collections::BinaryHeap;

use strata_registry::RoadClass;

use crate::roads::CrossingKind;

use super::{
    APPROACH_RADIUS, APPROACH_REACH_M, BRIDGE_LEAST_SQUARE, Buckets, Builder, Class, EARTH_FREE_M,
    FINE_M, FINE_MOVES, Hit, IN_BELT, INNER, NEAR_RIVER, ON_FALL, OPEN, PAD_CLEAR_M, Queued,
    RawRoad, SAMPLED, STRAIGHT_PAST_M, TEXTURE_SHARE, WET, distance, intersect, kit_stands,
    project, radius, square_to_flow, straddled, texture,
};

/// Metres between the wall search's nodes: a quarter of the fine
/// search's, so a wall two of those wide is eight of these, enough for a
/// line to slant across it.
const WALL_M: f64 = 16.0;

/// Metres along the searched line either side of a crossing whose nodes
/// the wall search replaces: a service road comes down a 50 m wall at its
/// grade in over 600 m, which a line winding along the wall finds between
/// two ends this far out.
const REACH_M: f64 = 512.0;

/// The margin the search box keeps past the window's ends, so a way out
/// that overshoots its end is still inside the box it is searched in.
const ROOM_M: f64 = 96.0;

/// The widest water the wall search crosses, metres: its longest move (a
/// three-by-one, 51 m) must span it with a dry node either side, and a
/// river that wide runs on a plain rather than in a trench.
const WIDTH_MOST_M: f64 = 30.0;

/// The earthwork of its searched line, square metres, past which a
/// window is searched again: a cut about 4 m past what passes free on one
/// edge. 40 % of the home mass's bridges, and every one with a deep cut.
const STEEP_EARTH: f64 = 100.0;

/// How many times the searched line's own length from its crossing to the
/// window's end on a side a way out may run on that side, from its deck's
/// middle: a little winding down a wall, not a loop round a plateau to
/// come back to where the searched line went.
const LONGEST: f64 = 1.75;

/// The share of its class's least radius the wall search turns at, step
/// to step. Its moves are 16 m, so a switchback's tip turns over two or
/// three of them at about this radius, and the smoothing (which bends no
/// tighter than the class's radius) must still round it: at 0.6 a tip of
/// 30 m came out of the shaping at (12 478, -6 963) as a U 20 m across
/// beside a deck. At 1.0 the showcase's west approach could not wind onto
/// its junction and cut 20 m.
const BEND: f64 = 0.8;

/// The bearings a deck is tried at, as the cosine and sine of its turn
/// off square to its channel: square, and 20 and 40 degrees either way
/// (40 is `SKEW_COS`'s, the most a straight through a bridge turns).
const TURNS: [[f64; 2]; 5] = [
    [1.0, 0.0],
    [0.939_692_620_785_908_4, 0.342_020_143_325_668_7],
    [0.939_692_620_785_908_4, -0.342_020_143_325_668_7],
    [0.766_044_443_118_978, 0.642_787_609_686_539_3],
    [0.766_044_443_118_978, -0.642_787_609_686_539_3],
];

/// Metres up and down its river from the searched line's crossing a deck
/// is tried at: a road running along its valley crosses where it has come
/// down the wall, which the fine search, coming down in one edge, may not
/// have found.
const SLIDES: [f64; 5] = [0.0, -96.0, 96.0, -192.0, 192.0];

/// Metres either side of the river a line is drawn to find it at a slid
/// deck's place.
const SLIDE_ACROSS_M: f64 = 120.0;

/// Metres a way out keeps off another road's searched line (or its own
/// beyond the window), but within as much of the window's ends, where
/// roads meet the line: the shaping moves a line a few tens of metres at
/// most, so two lines this far apart do not come to cross.
const ROAD_CLEAR_M: f64 = 24.0;

/// Metres within which of another road's line a node's moves are tested
/// for crossing it: over half the longest move, so a move that crosses a
/// line has an end this near it.
const ROAD_TEST_M: f64 = 32.0;

/// Wide enough that a move's test against the roads' lines (`ROAD_TEST_M`)
/// reads a bucket or two, not a line per node.
const LINE_BUCKET_M: f64 = 256.0;

/// A stretch of a road's line the wall search laid.
pub(super) struct Wall {
    /// The first and last of the line's points it replaces; both are kept
    /// as its own first and last.
    pub from: usize,
    pub to: usize,
    pub points: Vec<[f64; 2]>,
    /// The ground at each point, as the search sampled it.
    pub heights: Vec<f64>,
    /// The piece of `points` (point `crossing` to `crossing + 1`) that
    /// crosses the river.
    pub crossing: usize,
    /// Metres the search held the line straight either side of the deck's
    /// middle, the shorter side's: the shaping's straight through the
    /// bridge runs as far and no further, on the points the search laid.
    pub straight_m: f64,
}

/// The window round one crossing, as a road's searched line has it: what
/// one wall search needs, apart from the road.
pub(super) struct Plan {
    /// The first and last of the line's points it replaces.
    from: usize,
    to: usize,
    class: RoadClass,
    /// Its first and last points.
    ends: [[f64; 2]; 2],
    /// The line's direction into the first and out of the last.
    before: Option<[f64; 2]>,
    after: Option<[f64; 2]>,
    /// Where the searched line crosses, the channel there, the river's
    /// reach and the searched edge's direction.
    at: [f64; 2],
    hit: Hit,
    reach: u32,
    unit: [f64; 2],
    /// The searched line through it, which bounds the search's corridor,
    /// and its piece that crosses the river.
    line: Vec<[f64; 2]>,
    edge: usize,
    /// Metres along the searched line from the crossing back to its first
    /// point and on to its last.
    arcs: [f64; 2],
    /// The road's own sites, whose pads it may stand on.
    own: [Option<usize>; 2],
    /// The nodes other roads end on, back from the crossing and on from
    /// it, each from the crossing out: the ways out run through them, so
    /// every junction stands where the fine search laid it.
    holds: [Vec<[f64; 2]>; 2],
    /// The pieces of the other roads' searched lines round it, and of the
    /// road's own beyond it, which the ways out keep off.
    others: Vec<[[f64; 2]; 2]>,
}

/// Every road's searched line in buckets, for the windows' ways out to
/// keep off.
pub(super) struct Lines {
    /// Each piece: its road, its index along the road's searched points,
    /// and its ends.
    pieces: Vec<(u32, usize, [[f64; 2]; 2])>,
    buckets: Buckets,
}

/// One window's nodes at `WALL_M` spacing, read once each by the search.
struct Grid {
    i0: i32,
    j0: i32,
    w: i32,
    h: i32,
    height: Vec<f32>,
    slope: Vec<f32>,
    texture: Vec<f32>,
    flags: Vec<u8>,
    /// Whether a node lies within [`ROAD_TEST_M`] of another road's line,
    /// so that its moves are tested against them.
    near_road: Vec<bool>,
}

impl Grid {
    fn index(&self, n: [i32; 2]) -> Option<usize> {
        let (i, j) = (n[0] - self.i0, n[1] - self.j0);
        (i >= 0 && j >= 0 && i < self.w && j < self.h)
            .then(|| usize::try_from(j * self.w + i).expect("inside the box"))
    }

    fn node(&self, k: usize) -> [i32; 2] {
        let k = i32::try_from(k).expect("a box under 2 billion nodes");
        [self.i0 + k % self.w, self.j0 + k / self.w]
    }

    fn ground(&self, k: usize) -> Ground {
        Ground {
            height: f64::from(self.height[k]),
            slope: f64::from(self.slope[k]),
            weight: self.texture[k],
            flags: self.flags[k],
        }
    }
}

/// What a window's searches share: the nodes it keeps (its ends and the
/// nodes other roads end on) and where they are, the road's own sites and
/// the lines its ways keep off.
struct Window<'a> {
    kept: Vec<usize>,
    kept_at: Vec<[f64; 2]>,
    own: [Option<usize>; 2],
    others: &'a [[[f64; 2]; 2]],
}

impl Window<'_> {
    /// Whether the move from `a` to `b` crosses another road's line away
    /// from the nodes the window keeps, where roads meet its line.
    fn crosses_road(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        self.others.iter().any(|s| {
            intersect(a, b, s[0], s[1]).is_some_and(|(t, _)| {
                let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                self.kept_at.iter().all(|&e| distance(p, e) > ROAD_CLEAR_M)
            })
        })
    }
}

/// The ground at one point of a way out, as its cost reads it.
#[derive(Debug, Clone, Copy)]
struct Ground {
    height: f64,
    slope: f64,
    /// The texture's weight on travel there.
    weight: f32,
    flags: u8,
}

/// The profile a way out carries from its deck: its level at a point,
/// and how far that is left off the ground past what passes free.
#[derive(Debug, Clone, Copy)]
struct Carried {
    level: f64,
    left: f64,
}

/// One search's labels, kept for every search of a window and made fresh
/// for each by a stamp rather than cleared: a window's box is some ten
/// thousand nodes, searched up to fifty times.
struct Labels {
    stamp: Vec<u32>,
    now: u32,
    g: Vec<f64>,
    along: Vec<f64>,
    carried: Vec<Carried>,
    heading: Vec<[f64; 2]>,
    /// The way's last four points before the node, the latest first.
    trail: Vec<[[f64; 2]; 4]>,
    parent: Vec<u32>,
    via: Vec<u8>,
    closed: Vec<bool>,
    heap: BinaryHeap<Queued<usize>>,
}

impl Labels {
    fn new(cells: usize) -> Self {
        Self {
            stamp: vec![0; cells],
            now: 0,
            g: vec![f64::INFINITY; cells],
            along: vec![0.0; cells],
            carried: vec![
                Carried {
                    level: 0.0,
                    left: 0.0,
                };
                cells
            ],
            heading: vec![[0.0; 2]; cells],
            trail: vec![[[0.0; 2]; 4]; cells],
            parent: vec![u32::MAX; cells],
            via: vec![u8::MAX; cells],
            closed: vec![false; cells],
            heap: BinaryHeap::new(),
        }
    }

    /// A new search: every label stale.
    fn begin(&mut self) {
        self.now += 1;
        self.heap.clear();
    }

    /// Node `k`'s labels, made fresh if a former search left them.
    fn fresh(&mut self, k: usize) {
        if self.stamp[k] != self.now {
            self.stamp[k] = self.now;
            self.g[k] = f64::INFINITY;
            self.parent[k] = u32::MAX;
            self.via[k] = u8::MAX;
            self.closed[k] = false;
        }
    }
}

fn at(n: [i32; 2]) -> [f64; 2] {
    [f64::from(n[0]) * WALL_M, f64::from(n[1]) * WALL_M]
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over 16 m is far inside an i32"
)]
fn node_of(p: [f64; 2]) -> [i32; 2] {
    [
        (p[0] / WALL_M).round() as i32,
        (p[1] / WALL_M).round() as i32,
    ]
}

fn unit(p: [f64; 2], q: [f64; 2]) -> [f64; 2] {
    let l = distance(p, q).max(1e-9);
    [(q[0] - p[0]) / l, (q[1] - p[1]) / l]
}

/// The texture's weight on travel at `p`, as the fine search's nodes hold
/// it.
#[allow(clippy::cast_possible_truncation, reason = "a weight at f32")]
fn weight(seed: u64, p: [f64; 2]) -> f32 {
    (1.0 + TEXTURE_SHARE * texture(seed, p[0], p[1])) as f32
}

/// What a piece of a way out `length` long from `a` to `b` costs with the
/// profile `carried` into `a`, and the profile carried on to `b`: the
/// fine search's travel (`Class::step`), and for the earthwork the depth
/// the profile is left off the ground over the piece. The profile comes
/// as near the ground at `b` as the class's grade lets it from `a`.
fn piece(class: &Class, a: Ground, b: Ground, length: f64, carried: Carried) -> (f64, Carried) {
    let reach = class.grade * length;
    let level = b.height.clamp(carried.level - reach, carried.level + reach);
    let left = ((b.height - level).abs() - EARTH_FREE_M).max(0.0);
    let cost = class.step(
        length,
        b.height - a.height,
        f64::midpoint(a.slope, b.slope),
        f64::from(a.weight + b.weight),
        (a.flags | b.flags) & IN_BELT != 0,
    ) + class.earth * 0.5 * (carried.left + left) * length;
    (cost, Carried { level, left })
}

/// What a turn from `a` onto `b` (unit vectors) costs between moves
/// `mean_m` long on average: the fine search's charge for a turn between
/// two of its moves, times [`FINE_M`] over the moves' length. A bend of a
/// given radius split into moves a quarter as long turns a quarter as far
/// at each, which costs a sixteenth as much, at four times as many moves:
/// a quarter as much in all, so scaled, a bend costs about the same per
/// metre on either search's nodes. A turn where the window meets the searched line, or at one of
/// the searched line's own nodes, is one of the fine search's, at
/// `mean_m` [`FINE_M`].
fn turn(class: &Class, a: [f64; 2], b: [f64; 2], near_river: bool, mean_m: f64) -> f64 {
    let cos = a[0] * b[0] + a[1] * b[1];
    class.turn_cost(cos, near_river) * (FINE_M / mean_m)
}

/// The searched points of a road, as `Builder::searched` lays them: its
/// stubs and nodes, or a settlement's own road's traced line.
fn searched_points(raw: &RawRoad) -> Vec<[f64; 2]> {
    if let Some(fixed) = &raw.fixed {
        return fixed.clone();
    }
    raw.stubs[0]
        .into_iter()
        .chain(raw.nodes.iter().map(|&n| super::at(n)))
        .chain(raw.stubs[1])
        .collect()
}

impl Builder<'_> {
    /// Every road's searched line, in buckets.
    pub(super) fn lines(&self) -> Lines {
        let mut pieces = Vec::new();
        for (r, raw) in self.roads.iter().enumerate() {
            let r = u32::try_from(r).expect("under 4 billion roads");
            let points = searched_points(raw);
            for (k, w) in points.windows(2).enumerate() {
                pieces.push((r, k, [w[0], w[1]]));
            }
        }
        let boxes: Vec<(u32, [f64; 4])> = pieces
            .iter()
            .enumerate()
            .map(|(i, &(_, _, [a, b]))| {
                (
                    u32::try_from(i).expect("under 4 billion pieces"),
                    [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                    ],
                )
            })
            .collect();
        Lines {
            buckets: Buckets::new(LINE_BUCKET_M, &boxes, self.threads),
            pieces,
        }
    }

    /// The windows of the graded road `road`'s line `points` (with the
    /// ground `heights` at its searched nodes) the wall search lays afresh:
    /// one round each bridge, on the river's `crosses` piece, whose
    /// searched line within [`REACH_M`] of it falls well past its grade
    /// (its edges' earthwork, the crossing priced to a deck at its
    /// freeboard, over [`STEEP_EARTH`]). A window keeps within `nodes` (the
    /// first and last of the searched nodes in `points`, inside the
    /// stubs), short of the next crossing and of the last window, and its
    /// ways out run through the points other roads end on (`joined`). In
    /// order along the line, each a pure function of the line and the
    /// network's searched lines (`lines`), so they are searched apart.
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "the road, its class, the line, its ground, crossings and junctions, its nodes' range, its sites and the network's lines; an edge's ends, its channel and lengths, as the geometry names them"
    )]
    pub(super) fn plans(
        &self,
        road: usize,
        class: RoadClass,
        points: &[[f64; 2]],
        heights: &[Option<f64>],
        crosses: &[bool],
        joined: &[bool],
        nodes: [usize; 2],
        own: [Option<usize>; 2],
        lines: &Lines,
    ) -> Vec<Plan> {
        let kind = class;
        let class = Class::of(kind);
        let mut out = Vec::new();
        let mut arc = Vec::with_capacity(points.len());
        let mut total = 0.0;
        for k in 0..points.len() {
            if k > 0 {
                total += distance(points[k - 1], points[k]);
            }
            arc.push(total);
        }
        let mut floor = nodes[0];
        let mut scratch = Vec::new();
        let mut ids = Vec::new();
        for e in 0..points.len() - 1 {
            if !crosses[e] {
                continue;
            }
            let ceiling = (e + 1..points.len() - 1)
                .find(|&k| crosses[k])
                .unwrap_or(nodes[1])
                .min(nodes[1]);
            let lowest = floor;
            floor = floor.max(e + 1);
            if e < lowest || e + 1 > ceiling {
                continue;
            }
            let (a, b) = (points[e], points[e + 1]);
            let hits = self.channels.hits(a, b, &mut scratch);
            let [hit] = hits.as_slice() else {
                continue;
            };
            let (c, _, _) = self.channels.at(hit);
            let (width, depth) = (f64::from(c.width), f64::from(c.depth));
            if class.crossing(width, depth, c.flags) != CrossingKind::Bridge || width > WIDTH_MOST_M
            {
                continue;
            }
            let l = distance(a, b);
            let s = arc[e] + hit.t * l;
            // Out to the first node at least the reach away each way: the
            // searched line's moves are up to 200 m long.
            let mut ka = e;
            while ka > lowest && s - arc[ka] < REACH_M {
                ka -= 1;
            }
            let mut kb = e + 1;
            while kb < ceiling && arc[kb] - s < REACH_M {
                kb += 1;
            }
            if ka == e && kb == e + 1 {
                continue;
            }
            // Only where the searched line falls well past its grade:
            // elsewhere its nodes are fine enough, and the search's samples
            // cost.
            let deck = f64::from(c.surface) + class.freeboard_m;
            let earth: f64 = (ka..kb)
                .map(|k| {
                    let (Some(ha), Some(hb)) = (heights[k], heights[k + 1]) else {
                        return 0.0;
                    };
                    let l = distance(points[k], points[k + 1]);
                    if k == e {
                        let into = hit.t * l;
                        class.earthwork(into, deck - ha) + class.earthwork(l - into, hb - deck)
                    } else {
                        class.earthwork(l, hb - ha)
                    }
                })
                .sum();
            if earth < STEEP_EARTH * class.earth {
                continue;
            }
            // The next window starts where this one ends, whether or not
            // its search finds a line: so each is planned without the
            // others' searches.
            floor = kb;
            let x = [a[0] + (b[0] - a[0]) * hit.t, a[1] + (b[1] - a[1]) * hit.t];
            let reach_m = distance(x, points[ka]).max(distance(x, points[kb])) + ROOM_M;
            lines.buckets.within(
                [
                    x[0] - reach_m,
                    x[1] - reach_m,
                    x[0] + reach_m,
                    x[1] + reach_m,
                ],
                &mut ids,
            );
            // A road that ends on this one ends at one of its nodes, which
            // the ways out run through, from the deck out.
            let holds = [
                (ka + 1..=e)
                    .rev()
                    .filter(|&k| joined[k])
                    .map(|k| points[k])
                    .collect(),
                (e + 1..kb)
                    .filter(|&k| joined[k])
                    .map(|k| points[k])
                    .collect(),
            ];
            let others = ids
                .iter()
                .map(|&i| lines.pieces[i as usize])
                .filter(|&(r, k, _)| r as usize != road || k < ka || k >= kb)
                .map(|(_, _, piece)| piece)
                .collect();
            out.push(Plan {
                from: ka,
                to: kb,
                class: kind,
                ends: [points[ka], points[kb]],
                before: (ka > 0).then(|| unit(points[ka - 1], points[ka])),
                after: (kb + 1 < points.len()).then(|| unit(points[kb], points[kb + 1])),
                at: x,
                hit: *hit,
                reach: c.reach,
                unit: unit(a, b),
                line: points[ka..=kb].to_vec(),
                edge: e - ka,
                arcs: [s - arc[ka], arc[kb] - s],
                own,
                holds,
                others,
            });
        }
        out
    }

    /// The wall search of one window (see [`Self::wall_search`]).
    pub(super) fn wall(&self, plan: &Plan) -> Option<Wall> {
        self.wall_search(&Class::of(plan.class), plan)
    }

    /// Node `k` of the window's grid, sampled the first time it is asked
    /// for: open where the fine search's node would be, off other sites'
    /// pads and other roads' lines, and always at the nodes the window
    /// keeps.
    fn touch_wall(&self, grid: &mut Grid, k: usize, window: &Window<'_>) -> u8 {
        let flags = grid.flags[k];
        if flags & SAMPLED != 0 {
            return flags;
        }
        let p = at(grid.node(k));
        let f = self.sample_at(p);
        let mut flags = flags | f.flags | SAMPLED;
        let road = window
            .others
            .iter()
            .map(|s| distance(p, project(p, s[0], s[1]).0))
            .fold(f64::INFINITY, f64::min);
        grid.near_road[k] = road < ROAD_TEST_M;
        if window.kept.contains(&k) {
            flags |= OPEN;
        } else if flags & OPEN != 0 {
            let mut ids = Vec::new();
            self.site_grid
                .buckets
                .within([p[0], p[1], p[0], p[1]], &mut ids);
            let on_pad = ids.iter().any(|&s| {
                let site = &self.network.sites[s as usize];
                !window.own.contains(&Some(s as usize))
                    && distance(p, site.centre) < site.outer_m() + PAD_CLEAR_M
            });
            let on_road = road < ROAD_CLEAR_M
                && window
                    .kept_at
                    .iter()
                    .all(|&e| distance(p, e) > ROAD_CLEAR_M);
            if on_pad || on_road {
                flags &= !OPEN;
            }
        }
        grid.height[k] = f.height;
        grid.slope[k] = f.slope;
        grid.texture[k] = weight(self.seed, p);
        grid.flags[k] = flags;
        flags
    }

    /// The ground at `p` as a way out reads it, off the grid.
    fn ground_at(&self, p: [f64; 2]) -> Ground {
        let f = self.sample_at(p);
        Ground {
            height: f64::from(f.height),
            slope: f64::from(f.slope),
            weight: weight(self.seed, p),
            flags: f.flags,
        }
    }

    /// What the polyline `way` costs as a way out from a deck at `deck`,
    /// leaving it along `heading` and meeting the line beyond along
    /// `after`: walked in pieces no longer than [`WALL_M`] with the profile
    /// carried as [`Builder::side`] carries it, and turning at its points
    /// as the fine search turns at its nodes.
    #[allow(
        clippy::many_single_char_names,
        reason = "a piece's ends, length, direction and steps, as the geometry names them"
    )]
    fn walk(
        &self,
        class: &Class,
        way: &[[f64; 2]],
        deck: f64,
        heading: [f64; 2],
        after: Option<[f64; 2]>,
    ) -> f64 {
        let mut a = self.ground_at(way[0]);
        let mut carried = Carried {
            level: deck,
            left: ((a.height - deck).abs() - EARTH_FREE_M).max(0.0),
        };
        let mut cost = 0.0;
        let mut heading = heading;
        for w in way.windows(2) {
            let l = distance(w[0], w[1]);
            if l < 1e-6 {
                continue;
            }
            let u = unit(w[0], w[1]);
            cost += turn(class, heading, u, a.flags & NEAR_RIVER != 0, FINE_M);
            heading = u;
            let parts = (l / WALL_M).ceil();
            let mut i = 1.0;
            while i <= parts {
                let t = i / parts;
                let b = self.ground_at([
                    w[0][0] + (w[1][0] - w[0][0]) * t,
                    w[0][1] + (w[1][1] - w[0][1]) * t,
                ]);
                let (c, next) = piece(class, a, b, l / parts, carried);
                cost += c;
                carried = next;
                a = b;
                i += 1.0;
            }
        }
        if let Some(after) = after {
            cost += turn(class, heading, after, a.flags & NEAR_RIVER != 0, FINE_M);
        }
        cost
    }

    /// What the searched line through the window costs as the wall search
    /// prices its own: its deck as `Builder::section` reads it along the
    /// searched edge, and from each end of its shelves the searched line
    /// out to the window's end on that side (see [`Self::walk`]).
    /// Infinite where the kit stands no bridge there.
    fn searched_cost(&self, class: &Class, window: &Plan) -> f64 {
        let (c, chdir, on_bend) = self.channels.at(&window.hit);
        let (x, dir) = (window.at, window.unit);
        let ground_at = |s: f64| {
            f64::from(
                self.world
                    .sample_by(x[0] + dir[0] * s, x[1] + dir[1] * s, self.network)
                    .height,
            )
        };
        let s = self.section(class, c, chdir, dir, &ground_at, false);
        let gap = s.gap_m();
        let bed = f64::from(c.surface) - f64::from(c.depth);
        if s.kind != CrossingKind::Bridge || !kit_stands(gap, s.deck, bed) {
            return f64::INFINITY;
        }
        let back = s.out_m[0].max(s.least_m) + class.shelf_m;
        let on = s.out_m[1].max(s.least_m) + class.shelf_m;
        // Each way from its shelf's end along the searched line, past the
        // points the shelf reaches over, to the window's end.
        let along = |p: [f64; 2]| (p[0] - x[0]) * dir[0] + (p[1] - x[1]) * dir[1];
        let mut far = vec![[x[0] + dir[0] * on, x[1] + dir[1] * on]];
        far.extend(
            window.line[window.edge + 1..]
                .iter()
                .copied()
                .skip_while(|&p| along(p) <= on),
        );
        if far.len() == 1 {
            far.push(window.ends[1]);
        }
        let mut near = vec![[x[0] - dir[0] * back, x[1] - dir[1] * back]];
        near.extend(
            window.line[..=window.edge]
                .iter()
                .rev()
                .copied()
                .skip_while(|&p| along(p) >= -back),
        );
        if near.len() == 1 {
            near.push(window.ends[0]);
        }
        Self::bridge_cost(class, gap, 0.0, on_bend, c.flags)
            + self.walk(class, &far, s.deck, dir, window.after)
            + self.walk(
                class,
                &near,
                s.deck,
                [-dir[0], -dir[1]],
                window.before.map(|b| [-b[0], -b[1]]),
            )
    }

    /// The line through the window: a deck across its river at the place
    /// of [`SLIDES`] and the bearing of [`TURNS`] whose line costs least,
    /// as `Builder::section` reads it and the kit stands it, and from each
    /// end of its shelves the least-cost way out to the window's end on
    /// that side (see [`Self::side`]); `None` where no deck stands, no way
    /// out is found, or none costs less than the searched line
    /// ([`Self::searched_cost`]), which then stands. The decks share one
    /// grid, sampled as the ways out first touch it.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::many_single_char_names,
        reason = "the grid, the decks and their two ways out, in order; a box of node indices; points, a box and a channel, as the geometry names them"
    )]
    fn wall_search(&self, class: &Class, plan: &Plan) -> Option<Wall> {
        let (hit, line_unit) = (&plan.hit, plan.unit);
        let [from, to] = plan.ends;
        let x = plan.at;
        let reach_m = distance(x, from).max(distance(x, to)) + ROOM_M;
        let i0 = ((x[0] - reach_m) / WALL_M).floor() as i32;
        let j0 = ((x[1] - reach_m) / WALL_M).floor() as i32;
        let i1 = ((x[0] + reach_m) / WALL_M).ceil() as i32;
        let j1 = ((x[1] + reach_m) / WALL_M).ceil() as i32;
        let (w, h) = (i1 - i0 + 1, j1 - j0 + 1);
        let cells = usize::try_from(w * h).expect("a window's box");
        let mut grid = Grid {
            i0,
            j0,
            w,
            h,
            height: vec![0.0; cells],
            slope: vec![0.0; cells],
            texture: vec![1.0; cells],
            flags: vec![0; cells],
            near_road: vec![false; cells],
        };
        let near_line = |p: [f64; 2]| {
            plan.line
                .windows(2)
                .any(|s| distance(p, project(p, s[0], s[1]).0) <= class.corridor_m)
        };
        for k in 0..cells {
            let p = at(grid.node(k));
            if distance(p, x) <= reach_m && near_line(p) {
                grid.flags[k] |= INNER;
            }
        }
        let ends = [grid.index(node_of(from))?, grid.index(node_of(to))?];
        let holds: [Vec<usize>; 2] = [
            plan.holds[0]
                .iter()
                .map(|&p| grid.index(node_of(p)))
                .collect::<Option<_>>()?,
            plan.holds[1]
                .iter()
                .map(|&p| grid.index(node_of(p)))
                .collect::<Option<_>>()?,
        ];
        let kept: Vec<usize> = ends
            .iter()
            .chain(&holds[0])
            .chain(&holds[1])
            .copied()
            .collect();
        for &k in &kept {
            grid.flags[k] |= INNER;
        }
        let window = Window {
            kept_at: kept.iter().map(|&k| at(grid.node(k))).collect(),
            kept,
            own: plan.own,
            others: &plan.others,
        };
        let mut labels = Labels::new(cells);
        let mut scratch = Vec::new();
        // The searched line is the one to beat.
        let mut held = self.searched_cost(class, plan);
        let mut best: Option<Wall> = None;
        let mut bends = [[f64::INFINITY; 24]; 24];
        for (a, row) in bends.iter_mut().enumerate() {
            for (b, bend) in row.iter_mut().enumerate() {
                let (p, q) = (FINE_MOVES[a], FINE_MOVES[b]);
                *bend = radius(
                    [-f64::from(p[0]) * WALL_M, -f64::from(p[1]) * WALL_M],
                    [0.0, 0.0],
                    [f64::from(q[0]) * WALL_M, f64::from(q[1]) * WALL_M],
                );
            }
        }
        // The decks tried: where the searched line crosses and at each of
        // [`SLIDES`] up and down the river from it, each at every bearing of
        // [`TURNS`].
        let (_, along_river, _) = self.channels.at(hit);
        let mut decks: Vec<(Hit, [f64; 2])> = Vec::new();
        for slide in SLIDES {
            let base = [x[0] + along_river[0] * slide, x[1] + along_river[1] * slide];
            if slide == 0.0 {
                decks.push((*hit, x));
                continue;
            }
            // The river where a line square to it there crosses it.
            let across = [
                along_river[1] * SLIDE_ACROSS_M,
                -along_river[0] * SLIDE_ACROSS_M,
            ];
            let (a, b) = (
                [base[0] - across[0], base[1] - across[1]],
                [base[0] + across[0], base[1] + across[1]],
            );
            let found = self
                .channels
                .hits(a, b, &mut scratch)
                .into_iter()
                .filter(|h| self.channels.at(h).0.reach == plan.reach)
                .min_by(|p, q| (p.t - 0.5).abs().total_cmp(&(q.t - 0.5).abs()));
            if let Some(h) = found {
                let p = [a[0] + (b[0] - a[0]) * h.t, a[1] + (b[1] - a[1]) * h.t];
                if grid
                    .index(node_of(p))
                    .is_some_and(|k| grid.flags[k] & INNER != 0)
                {
                    decks.push((h, p));
                }
            }
        }
        for ((hit, x), [cos, sin]) in decks
            .iter()
            .flat_map(|&d| TURNS.iter().map(move |&t| (d, t)))
        {
            let (c, chdir, on_bend) = self.channels.at(&hit);
            if c.flags & ON_FALL != 0 {
                continue;
            }
            let (depth, surface) = (f64::from(c.depth), f64::from(c.surface));
            // Square to the channel, along the road's way across it.
            let mut normal = [chdir[1], -chdir[0]];
            if normal[0] * line_unit[0] + normal[1] * line_unit[1] < 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            let dir = [
                normal[0] * cos - normal[1] * sin,
                normal[0] * sin + normal[1] * cos,
            ];
            // As square to the channel's line as the search crosses it, and
            // to the water's flow as the record holds a deck.
            if (dir[0] * chdir[1] - dir[1] * chdir[0]).abs() < BRIDGE_LEAST_SQUARE
                || !square_to_flow(self.flow_at(x), dir)
            {
                continue;
            }
            let ground_at = |s: f64| {
                f64::from(
                    self.world
                        .sample_by(x[0] + dir[0] * s, x[1] + dir[1] * s, self.network)
                        .height,
                )
            };
            let s = self.section(class, c, chdir, dir, &ground_at, false);
            let gap = s.gap_m();
            if s.kind != CrossingKind::Bridge || !kit_stands(gap, s.deck, surface - depth) {
                continue;
            }
            // Each way out starts where the shelf behind its abutment ends,
            // and runs straight on past it.
            let back = s.out_m[0].max(s.least_m) + class.shelf_m;
            let on = s.out_m[1].max(s.least_m) + class.shelf_m;
            let near = [x[0] - dir[0] * back, x[1] - dir[1] * back];
            let far = [x[0] + dir[0] * on, x[1] + dir[1] * on];
            let (straight_back, straight_on) = (back + STRAIGHT_PAST_M, on + STRAIGHT_PAST_M);
            let crossed = self.channels.hits(near, far, &mut scratch);
            let dry = |p: [f64; 2]| {
                let sample = self.world.sample_by(p[0], p[1], self.network);
                sample.standing_water().is_none() && !self.blocked(p, plan.own)
            };
            if crossed.len() != 1
                || self.channels.at(&crossed[0]).0.reach != plan.reach
                || !dry(near)
                || !dry(far)
                || window.crosses_road(
                    [x[0] - dir[0] * straight_back, x[1] - dir[1] * straight_back],
                    [x[0] + dir[0] * straight_on, x[1] + dir[1] * straight_on],
                )
            {
                continue;
            }
            let bridge = Self::bridge_cost(class, gap, 0.0, on_bend, c.flags);
            let Some((on_cost, on_straight, on_h, on_path)) = self.side(
                class,
                &mut grid,
                &mut labels,
                &window,
                Side {
                    from: far,
                    dir,
                    deck: s.deck,
                    holds: &holds[1],
                    goal: ends[1],
                    after: plan.after,
                    longest: LONGEST * plan.arcs[1] - on,
                    most: held - bridge,
                    bends: &bends,
                },
            ) else {
                continue;
            };
            let Some((back_cost, back_straight, back_h, back_path)) = self.side(
                class,
                &mut grid,
                &mut labels,
                &window,
                Side {
                    from: near,
                    dir: [-dir[0], -dir[1]],
                    deck: s.deck,
                    holds: &holds[0],
                    goal: ends[0],
                    after: plan.before.map(|b| [-b[0], -b[1]]),
                    longest: LONGEST * plan.arcs[0] - back,
                    most: held - bridge - on_cost,
                    bends: &bends,
                },
            ) else {
                continue;
            };
            let total = on_cost + back_cost + bridge;
            if total >= held {
                continue;
            }
            let mut points: Vec<[f64; 2]> = Vec::new();
            let mut heights: Vec<f64> = Vec::new();
            for &k in back_path.iter().rev() {
                points.push(at(grid.node(k)));
                heights.push(f64::from(grid.height[k]));
            }
            points.push(back_straight);
            heights.push(back_h);
            let crossing = points.len();
            points.extend([near, far, on_straight]);
            heights.extend([s.deck, s.deck, on_h]);
            for &k in &on_path {
                points.push(at(grid.node(k)));
                heights.push(f64::from(grid.height[k]));
            }
            let n = points.len();
            points[0] = from;
            points[n - 1] = to;
            held = total;
            best = Some(Wall {
                from: plan.from,
                to: plan.to,
                points,
                heights,
                crossing,
                straight_m: straight_back.min(straight_on),
            });
        }
        best
    }

    /// The least-cost way from the end of a deck's shelf out to the
    /// window's end on its side: on along the deck for [`STRAIGHT_PAST_M`],
    /// then on the grid's nodes, through each node another road ends on
    /// (`holds`) in turn, a leg to each ([`Self::leg`]). Its cost, the
    /// straight's end and ground, and its nodes from the first past the
    /// straight to the window's end; `None` where there is none under
    /// `most`.
    ///
    /// Costs as the fine search's but for the earthwork, which is carried
    /// along the way as a profile (see [`piece`]): it leaves the deck at
    /// its level and at each node comes as near the ground as the class's
    /// grade lets it from the last, and the way pays for the depth it is
    /// left off the ground (past what passes free) over its length. So a
    /// line straight down a wall onto the deck pays for the cutting all the
    /// way back to where the grade meets the ground, one that dips into a
    /// side valley and climbs out pays for both, and one slanting down the
    /// wall at the grade pays nothing.
    ///
    /// The search is a heuristic, not a least-cost one: the profile, the
    /// heading (and so the bend allowed next) and the length run all
    /// depend on how a node was reached, but each node keeps only its
    /// cheapest arrival, so a dearer arrival at a better level or heading
    /// is lost, and with it the continuations only it allows. The profile
    /// is carried forward only, so it starts down late before a steep
    /// pitch. Keeping a node's arrivals by heading (24 of them) would make
    /// each search some twenty times the work, and there are up to fifty
    /// a window. The heuristic itself is admissible: no move costs less
    /// than its length times the least texture weight.
    #[allow(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "the search, its moves and costs, in one loop, over A*'s own names"
    )]
    fn side(
        &self,
        class: &Class,
        grid: &mut Grid,
        labels: &mut Labels,
        window: &Window<'_>,
        side: Side<'_>,
    ) -> Option<(f64, [f64; 2], f64, Vec<usize>)> {
        let Side {
            from,
            dir,
            deck,
            holds,
            goal,
            after,
            longest,
            most,
            bends,
        } = side;
        // The way runs through the nodes other roads end on, in order,
        // then to the window's end: a leg to each. The heuristic is the
        // straight line through what is left of them.
        let targets: Vec<usize> = holds.iter().copied().chain([goal]).collect();
        let places: Vec<[f64; 2]> = targets.iter().map(|&t| at(grid.node(t))).collect();
        let mut rest = vec![0.0; targets.len()];
        for i in (0..targets.len() - 1).rev() {
            rest[i] = rest[i + 1] + distance(places[i], places[i + 1]);
        }
        let least_weight = (1.0 - TEXTURE_SHARE) * 0.999;
        let heuristic = |grid: &Grid, k: usize, leg: usize| {
            least_weight * (distance(at(grid.node(k)), places[leg]) + rest[leg])
        };
        let near_radius = class.min_radius_m * BEND * APPROACH_RADIUS;
        let mut scratch = Vec::new();
        labels.begin();
        // The deck's straight runs on past the shelf, as the shaping lays
        // it (`Builder::straighten`), and the way out leaves its end.
        let straight = [
            from[0] + dir[0] * STRAIGHT_PAST_M,
            from[1] + dir[1] * STRAIGHT_PAST_M,
        ];
        if !self.channels.hits(from, straight, &mut scratch).is_empty() {
            return None;
        }
        let g0 = self.ground_at(from);
        let g1 = self.ground_at(straight);
        if g1.flags & OPEN == 0 {
            return None;
        }
        let (first, c1) = piece(
            class,
            g0,
            g1,
            STRAIGHT_PAST_M,
            Carried {
                level: deck,
                left: ((g0.height - deck).abs() - EARTH_FREE_M).max(0.0),
            },
        );
        // Onto the grid: the nodes round a step on, as round a bend as
        // near the deck a road may take.
        let ahead = node_of([straight[0] + dir[0] * WALL_M, straight[1] + dir[1] * WALL_M]);
        for dj in -1..=1 {
            for di in -1..=1 {
                let q = at([ahead[0] + di, ahead[1] + dj]);
                let Some(k) = grid.index(node_of(q)) else {
                    continue;
                };
                let length = distance(straight, q);
                if grid.flags[k] & INNER == 0 || length < 0.5 * WALL_M {
                    continue;
                }
                let u = unit(straight, q);
                if u[0] * dir[0] + u[1] * dir[1] <= 0.0 || radius(from, straight, q) < near_radius {
                    continue;
                }
                let fk = self.touch_wall(grid, k, window);
                if fk & OPEN == 0
                    || !self.channels.hits(straight, q, &mut scratch).is_empty()
                    || window.crosses_road(straight, q)
                {
                    continue;
                }
                let (step, carried) = piece(class, g1, grid.ground(k), length, c1);
                let cost = first
                    + step
                    + turn(
                        class,
                        dir,
                        u,
                        fk & NEAR_RIVER != 0,
                        f64::midpoint(STRAIGHT_PAST_M, length),
                    );
                labels.fresh(k);
                if cost < labels.g[k] {
                    labels.g[k] = cost;
                    labels.along[k] = STRAIGHT_PAST_M + length;
                    labels.carried[k] = carried;
                    labels.heading[k] = u;
                    labels.trail[k] = [straight, from, from, from];
                    labels.heap.push(Queued {
                        f: cost + heuristic(grid, k, 0),
                        key: k,
                    });
                }
            }
        }
        let mut way: Vec<usize> = Vec::new();
        for (leg, &target) in targets.iter().enumerate() {
            let last = leg + 1 == targets.len();
            if !self.leg(
                class,
                grid,
                labels,
                window,
                Leg {
                    target,
                    index: leg,
                    after: if last { after } else { None },
                    straight,
                    longest,
                    most,
                    bends,
                },
                &heuristic,
            ) {
                return None;
            }
            let mut path = vec![target];
            while labels.parent[path[path.len() - 1]] != u32::MAX {
                path.push(labels.parent[path[path.len() - 1]] as usize);
            }
            path.reverse();
            // A leg after the first starts where the last one ended.
            way.extend(path.into_iter().skip(usize::from(leg > 0)));
            if last {
                return Some((labels.g[target], straight, g1.height, way));
            }
            // The next leg leaves the hold as this one reached it.
            let (g, along, carried, heading, trail, via) = (
                labels.g[target],
                labels.along[target],
                labels.carried[target],
                labels.heading[target],
                labels.trail[target],
                labels.via[target],
            );
            labels.begin();
            labels.fresh(target);
            labels.g[target] = g;
            labels.along[target] = along;
            labels.carried[target] = carried;
            labels.heading[target] = heading;
            labels.trail[target] = trail;
            labels.via[target] = via;
            labels.heap.push(Queued {
                f: g + heuristic(grid, target, leg + 1),
                key: target,
            });
        }
        None
    }

    /// One leg of a way out (see [`Self::side`]): the search from the
    /// labels queued to `target`, whose labels then hold the way there.
    /// Whether it was reached under `most`.
    #[allow(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "the search, its moves and costs, in one loop, over A*'s own names"
    )]
    fn leg(
        &self,
        class: &Class,
        grid: &mut Grid,
        labels: &mut Labels,
        window: &Window<'_>,
        leg: Leg<'_>,
        heuristic: &dyn Fn(&Grid, usize, usize) -> f64,
    ) -> bool {
        let Leg {
            target,
            index: leg,
            after,
            straight,
            longest,
            most,
            bends,
        } = leg;
        let least_radius = class.min_radius_m * BEND;
        let near_radius = least_radius * APPROACH_RADIUS;
        let mut scratch = Vec::new();
        let mut hits = Vec::new();
        while let Some(Queued { key: k, f }) = labels.heap.pop() {
            if labels.closed[k] {
                continue;
            }
            if f >= most {
                return false;
            }
            labels.closed[k] = true;
            if k == target {
                return true;
            }
            let node = grid.node(k);
            let pa = at(node);
            let ga = grid.ground(k);
            let behind = if labels.parent[k] == u32::MAX {
                straight
            } else {
                at(grid.node(labels.parent[k] as usize))
            };
            // The length of the move into the node, for its turn's weight.
            let into = if labels.via[k] == u8::MAX {
                distance(straight, pa)
            } else {
                self.moves[usize::from(labels.via[k])].0 * WALL_M
            };
            // Round a bend no tighter than the class can take, and near the
            // deck no tighter than the shaping makes it there.
            let least = if labels.along[k] < APPROACH_REACH_M {
                near_radius
            } else {
                least_radius
            };
            for (m, mv) in FINE_MOVES.iter().enumerate() {
                let next = [node[0] + mv[0], node[1] + mv[1]];
                let Some(j) = grid.index(next) else { continue };
                labels.fresh(j);
                if labels.closed[j] || grid.flags[j] & INNER == 0 {
                    continue;
                }
                let (len, u) = self.moves[m];
                let length = len * WALL_M;
                let out = labels.along[k] + length;
                if out > longest {
                    continue;
                }
                let pb = at(next);
                // No sharper than a right angle in a step: three points in
                // a line doubling back have no bend to measure.
                let heading = labels.heading[k];
                let bend = if heading[0] * u[0] + heading[1] * u[1] <= 0.0 {
                    0.0
                } else if labels.via[k] == u8::MAX {
                    radius(behind, pa, pb)
                } else {
                    bends[usize::from(labels.via[k])][m]
                };
                if bend < least {
                    continue;
                }
                let fj = self.touch_wall(grid, j, window);
                if fj & OPEN == 0 {
                    continue;
                }
                let through = straddled(*mv).into_iter().flatten().all(|d| {
                    grid.index([node[0] + d[0], node[1] + d[1]])
                        .is_some_and(|s| self.touch_wall(grid, s, window) & OPEN != 0)
                });
                if !through {
                    continue;
                }
                // No channel is crossed on the way out (the window ends
                // short of the line's other crossings), and no road.
                if (ga.flags | fj) & WET != 0 {
                    self.channels.hits_into(pa, pb, &mut scratch, &mut hits);
                    if !hits.is_empty() {
                        continue;
                    }
                }
                if (grid.near_road[k] || grid.near_road[j]) && window.crosses_road(pa, pb) {
                    continue;
                }
                let (step, carried) = piece(class, ga, grid.ground(j), length, labels.carried[k]);
                let mut cost = step
                    + turn(
                        class,
                        heading,
                        u,
                        ga.flags & NEAR_RIVER != 0,
                        f64::midpoint(into, length),
                    );
                // A reversal within one of the fine search's moves pays its
                // hairpin, as one there does: the way's line from a point
                // [`FINE_M`] or more back (or as far as it goes) turned past
                // a right angle onto this move. Its steps never turn so far
                // one by one, and a switchback over a few of them is a tip
                // the smoothing closes up.
                let trail = labels.trail[k];
                let mut back = trail[3];
                let mut run = 0.0;
                let mut was = pa;
                for &p in &trail {
                    run += distance(was, p);
                    was = p;
                    if run >= FINE_M {
                        back = p;
                        break;
                    }
                }
                let chord = unit(back, pa);
                cost += class.hairpin * (-(chord[0] * u[0] + chord[1] * u[1])).max(0.0);
                // Onto the searched line where the window ends: a turn at
                // one of its nodes.
                if j == target
                    && let Some(after) = after
                {
                    cost += turn(class, u, after, fj & NEAR_RIVER != 0, FINE_M);
                }
                let gj = labels.g[k] + cost;
                if gj < labels.g[j] {
                    labels.g[j] = gj;
                    labels.along[j] = out;
                    labels.carried[j] = carried;
                    labels.heading[j] = u;
                    labels.trail[j] = [pa, trail[0], trail[1], trail[2]];
                    labels.via[j] = u8::try_from(m).expect("one of 24 moves");
                    labels.parent[j] = u32::try_from(k).expect("a window under 4 billion nodes");
                    labels.heap.push(Queued {
                        f: gj + heuristic(grid, j, leg),
                        key: j,
                    });
                }
            }
        }
        false
    }
}

/// One leg of a way out for [`Builder::leg`].
struct Leg<'a> {
    /// The node it ends at, and which of the way's legs it is.
    target: usize,
    index: usize,
    /// The searched line's bearing on past the target, where it is the
    /// window's end.
    after: Option<[f64; 2]>,
    /// The end of the deck's straight, where the way left it.
    straight: [f64; 2],
    longest: f64,
    most: f64,
    bends: &'a [[f64; 24]; 24],
}

/// One way out of a deck for [`Builder::side`].
struct Side<'a> {
    /// The end of the shelf behind the abutment, and the deck's bearing
    /// away from the river there.
    from: [f64; 2],
    dir: [f64; 2],
    /// The deck's level.
    deck: f64,
    /// The nodes other roads end on that the way runs through, from the
    /// deck out, and the window's end on this side, nodes of the grid.
    holds: &'a [usize],
    goal: usize,
    /// The searched line's bearing on past it.
    after: Option<[f64; 2]>,
    /// Metres the way may run from the shelf's end (see [`LONGEST`]).
    longest: f64,
    /// What a way out must cost less than to better the best line so far.
    most: f64,
    /// The radius of a bend from one move into another, metres.
    bends: &'a [[f64; 24]; 24],
}
