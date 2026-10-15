//! The rivers' shape (ticket 0005, step 3b): each reach settled onto the
//! sample's own valley floor, its tributaries joined at an acute angle, and
//! its channel's meanders.
//!
//! The lattice routes water over the coarse field, 512 m apart, and corner
//! cutting leaves long, nearly straight runs between its nodes: seen from
//! above a river read as a canal of even width, and where the provinces'
//! shapes raise a spur over the coarse field a creek was cut through it.
//!
//! - **Settling.** Every reach is cut into segments of at most
//!   [`RiverParams::resample_m`], and each vertex moved sideways onto the
//!   lowest natural ground within [`RiverParams::settle_search_m`] that is
//!   worth the move ([`RiverParams::settle_slack`]), searched coarse and
//!   then fine, the offsets smoothed along the reach. A junction is moved
//!   once, in two dimensions, for every reach that meets there. A vertex
//!   moves at most two fifths of the way to the nearest other reach, so two
//!   reaches never settle into one line, and not at all near a settlement,
//!   into the sea or into another lake.
//! - **Confluences.** A tributary that meets its main stem square on is
//!   bent over its last stretch to come in at the angle
//!   [`RiverParams::confluence_tan`] off the main stem's upstream line,
//!   pointing downstream.
//! - **Meanders.** The channel winds about the settled line by a periodic
//!   wave ([`wave`]) along the reach, every [`RiverParams::bend_widths`]
//!   channel widths, its wavelength and size varied by noises keyed by
//!   world position and the seed. Its amplitude is a share of
//!   [`RiverParams::bend_amplitude`]: whole on a flat floor at a gentle
//!   gradient, nothing on a steep or confined one, at a fall, at a reach's
//!   ends (so the channel passes through every junction on the line) or
//!   near a settlement, and never more than the line's own curvature or the
//!   room to the next reach allows, so a channel crosses neither itself nor
//!   another. The phase runs on from the main stem's into the reach below a
//!   confluence. The vertices hold the amplitude and the phase; the carve
//!   evaluates the wave between them, so meanders a few tens of metres long
//!   cost no more segments than the line has.
//!
//! Every operation is `+ - * /`, `sqrt` and `floor` (the crate's float
//! rule), and every reach is shaped from the bake's own data alone, so the
//! result is the same on any thread count.

use strata_core::{Seed, mix64};

use super::index::Shore;
use super::rivers::Built;
use super::route::site_distance;
use super::{NodeMap, River, RiverEnd, RiverParams, RiverVertex, local, single};
use crate::ProceduralWorld;
use crate::Site;
use crate::noise::{Fractal, Shape, smoothstep};

/// How far a junction looks for lower ground, metres, on a square of
/// [`JUNCTION_STEPS`] a side.
pub(crate) const JUNCTION_SEARCH_M: f64 = 48.0;

/// Candidates a side of a junction's search, centre included.
const JUNCTION_STEPS: i32 = 2;

/// The farthest back along a tributary its bend into the main stem reaches,
/// metres; it moves no vertex further than that.
pub(crate) const CONFLUENCE_MOST_M: f64 = 200.0;

/// Over how much of a reach next to a fixed end (a junction, a lake, an
/// edge) its settling fades in, metres.
const END_TAPER_M: f64 = 120.0;

/// Over how much of a reach's last stretch into the sea the settling fades
/// out, metres: the mouth is graded along its last segment's line.
const MOUTH_KEEP_M: f64 = 300.0;

/// The share of the way to the nearest other reach a vertex may settle.
const CLEARANCE_SHARE: f64 = 0.4;

/// Candidates either side of the line in the coarse search, the line's own
/// point aside, and in the fine one, which steps a third as far.
const COARSE_STEPS: i32 = 4;
const FINE_STEPS: i32 = 2;

/// Natural ground lower than this is the sea's, metres: a reach is never
/// settled into it unless it was there.
const SEA_GUARD_M: f64 = 0.5;

/// How far under the lake's level the ground must stand for a point to be
/// another lake's water, which a reach is not settled into, metres.
const LAKE_GUARD_M: f64 = 2.0;

/// The rise of the floor either side of a settled vertex, metres, over
/// which its meanders shrink from whole to nothing: a flat floor lets a
/// river wander, a V holds it to its line.
const FLAT_RISE_M: (f64, f64) = (1.5, 10.0);

/// The surface's gradient, rise over run, over which a meander shrinks
/// from whole to nothing: a lowland river loops, a torrent runs straight.
const MEANDER_SLOPE: (f64, f64) = (0.008, 0.05);

/// The cut over which a valley confines its river's meanders, metres, and
/// how much of them it takes at the deepest: the natural ground a gorge is
/// cut into may be a flat plateau, which says nothing of its floor.
const CONFINED_CUT_M: (f64, f64) = (35.0, 90.0);
const CONFINED_LEAST: f64 = 0.85;

/// The share of its meanders a steep river keeps: a torrent runs nearly
/// straight, not ruled, or a creek down a hillside reads as a road.
const STEEP_LEAST: f64 = 0.2;

/// The meanders' size and length vary over this many metres of ground.
const BEND_NOISE_M: (f64, f64) = (700.0, 500.0);

/// The share of a reach's last stretch into the sea over which its meanders
/// die out, metres.
const MOUTH_STRAIGHT_M: f64 = 200.0;

/// The most a meander may swing, as a share of the radius its line turns
/// at: so the wave never folds the channel over on a bend's inside.
const RADIUS_SHARE: f64 = 0.45;

/// The most a meander may swing, as a share of the room to the nearest
/// other reach less the channel's width.
const ROOM_SHARE: f64 = 0.3;

/// A periodic polynomial close to a sine at `phase` (in periods), its
/// derivative by the phase and its second: `1 - 1.2 x^2 + 0.2 x^4` over
/// each half period (`x` from -1 to 1, the apex at 0) and its negative
/// over the other, so that it, its slope and its curvature are continuous
/// (a parabola's curvature flipped at every crossing). Only `floor` and
/// polynomials: the float rule.
pub(crate) fn sine(phase: f64) -> (f64, f64, f64) {
    let u = phase - phase.floor();
    let (v, sign) = if u < 0.5 { (u, 1.0) } else { (u - 0.5, -1.0) };
    let x = 4.0 * v - 1.0;
    let x2 = x * x;
    let p = 1.0 - 1.2 * x2 + 0.2 * x2 * x2;
    let dp = -2.4 * x + 0.8 * x2 * x;
    let ddp = -2.4 + 2.4 * x2;
    (sign * p, sign * 4.0 * dp, sign * 16.0 * ddp)
}

/// The mean of [`sine`]'s square over a period: the mean of `p^2` over `x`
/// in `-1..1`, term by term.
pub(crate) const SINE_MEAN_SQUARE: f64 = 1.0 - 2.4 / 3.0 + 1.84 / 5.0 - 0.48 / 7.0 + 0.04 / 9.0;

/// The share of a meander's amplitude its own bends take; the rest is a
/// slower wander the bends ride on, [`WANDER_PERIODS`] of them long. One
/// wave alone, however its size varied, read from above as a regular
/// squiggle.
const BEND_SHARE: f64 = 0.68;
const WANDER_PERIODS: f64 = 4.7;

/// How much each bend's own size varies, as the least share of the
/// largest: a bend's size is a hash of its index along the phase, eased
/// from one bend's apex to the next.
const BEND_LEAST: f64 = 0.5;

/// The channel's swing about its line, per metre of its amplitude, at
/// `phase` (in bends): what the carve, a centreline and the bake read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wave {
    /// The offset over the amplitude, `-1..=1`, and its first and second
    /// derivatives by the phase.
    pub offset: f64,
    pub d_offset: f64,
    pub dd_offset: f64,
    /// How far the channel's width runs over its mean, per unit of
    /// [`RiverParams::riffle_share`] at a full meander, and its derivative:
    /// positive where it crosses between two bends, negative in a bend, a
    /// mean of nought over each bend.
    pub riffle: f64,
    pub d_riffle: f64,
    /// The bend's own swing, signed, `-1..=1`: which way the channel bends
    /// and how hard, for the bar on its inside.
    pub bend: f64,
}

/// A bend's size, from its index: `BEND_LEAST..1`.
#[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
fn bend_size(index: f64) -> f64 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a bend's index is far inside an i64"
    )]
    let k = index as i64;
    let h = mix64(k.cast_unsigned() ^ 0x6d65_616e_6465_7273);
    BEND_LEAST + (1.0 - BEND_LEAST) * ((h >> 11) as f64 / (1_u64 << 53) as f64)
}

/// The meanders at `phase` (see [`Wave`]): each bend a [`sine`] lobe sized
/// by [`bend_size`], eased between apexes, over a slower wander.
#[allow(
    clippy::many_single_char_names,
    reason = "the wave's own letters: its value, a bend's size and their derivatives"
)]
pub(crate) fn wave(phase: f64) -> Wave {
    let (p, dp, ddp) = sine(phase);
    // Each half period is a bend; its size eases from one apex to the next.
    let lobes = 2.0 * phase - 0.5;
    let k = lobes.floor();
    let t = lobes - k;
    let (r0, r1) = (bend_size(k), bend_size(k + 1.0));
    let ease = t * t * (3.0 - 2.0 * t);
    let ease_rate = 6.0 * t * (1.0 - t) * 2.0;
    let ease_curve = (6.0 - 12.0 * t) * 4.0;
    let m = r0 + (r1 - r0) * ease;
    let dm = (r1 - r0) * ease_rate;
    let ddm = (r1 - r0) * ease_curve;
    let (q, dq, ddq) = sine(phase / WANDER_PERIODS + 0.31);
    let (dq, ddq) = (dq / WANDER_PERIODS, ddq / (WANDER_PERIODS * WANDER_PERIODS));
    let wander = 1.0 - BEND_SHARE;
    let spread = SINE_MEAN_SQUARE - p * p;
    Wave {
        offset: BEND_SHARE * m * p + wander * q,
        d_offset: BEND_SHARE * (dm * p + m * dp) + wander * dq,
        dd_offset: BEND_SHARE * (ddm * p + 2.0 * dm * dp + m * ddp) + wander * ddq,
        riffle: m * m * spread / SINE_MEAN_SQUARE,
        d_riffle: (2.0 * m * dm * spread - 2.0 * m * m * p * dp) / SINE_MEAN_SQUARE,
        bend: m * p,
    }
}

/// How much of a full meander a channel of `discharge` swinging by
/// `amplitude` metres has, `0..=1`: what its riffles and pools scale by.
pub(crate) fn bend_share(params: &RiverParams, amplitude: f64, discharge: f64) -> f64 {
    let most = params.bend_amplitude(discharge);
    if most > 0.0 {
        (amplitude / most).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The channel's width over its mean where the meanders read `wave` and
/// are `share` of a full one: wider where it crosses between two bends (a
/// riffle), narrower at a bend's apex (a pool), the mean over a bend one.
/// Its depth is the mean's over this, so the channel's section keeps its
/// area.
pub(crate) fn riffle(params: &RiverParams, share: f64, wave: &Wave) -> f64 {
    1.0 + params.riffle_share * riffle_share(share) * wave.riffle
}

/// How much of its riffles and pools a channel with `share` of a full
/// meander has: a straight one keeps [`RIFFLE_LEAST`] of them, as a
/// torrent's steps and pools, so even a reach that runs straight is
/// fordable at its riffles.
pub(crate) fn riffle_share(share: f64) -> f64 {
    RIFFLE_LEAST + (1.0 - RIFFLE_LEAST) * share
}

/// The share of its riffles a straight channel keeps.
pub(crate) const RIFFLE_LEAST: f64 = 0.35;

/// 53 bits of a hash as a float in `0..1`.
#[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1_u64 << 53) as f64
}

/// The unit vector along `(dx, dz)`, or nought.
fn normalise(dx: f64, dz: f64) -> [f64; 2] {
    let length = (dx * dx + dz * dz).sqrt();
    if length > 1e-12 {
        [dx / length, dz / length]
    } else {
        [0.0, 0.0]
    }
}

/// The distance from `(x, z)` to the segment from `a` to `b`.
#[allow(
    clippy::many_single_char_names,
    reason = "a point, a segment's ends and the projection"
)]
fn segment_distance(x: f64, z: f64, a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dz * dz;
    let t = if length2 > 0.0 {
        (((x - a[0]) * dx + (z - a[1]) * dz) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (px, pz) = (a[0] + dx * t - x, a[1] + dz * t - z);
    (px * px + pz * pz).sqrt()
}

/// Whether segments `a`-`b` and `c`-`d` cross, ends excluded.
fn crosses(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The reaches' segments in square cells, for the room between reaches.
struct LineGrid {
    cells: NodeMap<Vec<(u32, u32)>>,
}

/// The side of a [`LineGrid`] cell, metres.
const GRID_M: f64 = 256.0;

#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over a cell is far inside an i32"
)]
fn grid_cell(metres: f64) -> i32 {
    (metres / GRID_M).floor() as i32
}

impl LineGrid {
    fn new(rivers: &[River]) -> Self {
        let mut cells: NodeMap<Vec<(u32, u32)>> = NodeMap::default();
        for (r, river) in rivers.iter().enumerate() {
            for (s, pair) in river.vertices.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                for j in grid_cell(a.z.min(b.z))..=grid_cell(a.z.max(b.z)) {
                    for i in grid_cell(a.x.min(b.x))..=grid_cell(a.x.max(b.x)) {
                        cells.entry([i, j]).or_default().push((local(r), local(s)));
                    }
                }
            }
        }
        Self { cells }
    }

    /// The distance from `(x, z)` to the nearest segment of a reach that
    /// `skip` does not name, looked for within `radius`; infinite if none.
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, a cell, a reach, a segment and its ends"
    )]
    fn nearest(
        &self,
        rivers: &[River],
        (x, z): (f64, f64),
        radius: f64,
        skip: impl Fn(usize) -> bool,
    ) -> f64 {
        let mut best = f64::INFINITY;
        for j in grid_cell(z - radius)..=grid_cell(z + radius) {
            for i in grid_cell(x - radius)..=grid_cell(x + radius) {
                let Some(cell) = self.cells.get(&[i, j]) else {
                    continue;
                };
                for &(r, s) in cell {
                    if skip(r as usize) {
                        continue;
                    }
                    let v = &rivers[r as usize].vertices;
                    let (a, b) = (v[s as usize], v[s as usize + 1]);
                    best = best.min(segment_distance(x, z, [a.x, a.z], [b.x, b.z]));
                }
            }
        }
        best
    }
}

/// Each reach's vertices split so no segment but a fall's is longer than
/// `most` metres: a fall stays one stretch, so its record keeps its
/// vertex pair.
fn resample(vertices: &mut Vec<RiverVertex>, most: f64) {
    let mut out = Vec::with_capacity(vertices.len() * 2);
    for pair in vertices.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        out.push(a);
        if a.fall {
            continue;
        }
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let length = (dx * dx + dz * dz).sqrt();
        let parts = (length / most).ceil();
        let mut k = 1.0;
        while k < parts {
            let f = k / parts;
            let lerp = |p: f32, q: f32| single(f64::from(p) + (f64::from(q) - f64::from(p)) * f);
            out.push(RiverVertex {
                x: a.x + dx * f,
                z: a.z + dz * f,
                surface_m: lerp(a.surface_m, b.surface_m),
                discharge_m3s: lerp(a.discharge_m3s, b.discharge_m3s),
                width_m: lerp(a.width_m, b.width_m),
                depth_m: lerp(a.depth_m, b.depth_m),
                cut_m: lerp(a.cut_m, b.cut_m),
                fall: false,
                meander_m: 0.0,
                phase: 0.0,
            });
            k += 1.0;
        }
    }
    if let Some(&last) = vertices.last() {
        out.push(last);
    }
    *vertices = out;
}

/// What the settling reads of the world: the natural ground, the
/// settlements' fade and the lakes a reach must keep out of.
struct Ground<'a> {
    world: &'a ProceduralWorld,
    params: &'a RiverParams,
    sites: &'a [Site],
    shore: &'a Shore,
    lakes: &'a [super::Lake],
}

impl Ground<'_> {
    fn height(&self, x: f64, z: f64) -> f64 {
        self.world.natural_height(x, z)
    }

    /// How free a river is to move at a point ([`freedom`]).
    fn freedom(&self, x: f64, z: f64) -> f64 {
        freedom(self.sites, self.params, x, z)
    }

    /// Whether a point is in a lake's water or right at its shore.
    fn in_lake(&self, x: f64, z: f64) -> bool {
        self.shore
            .lake_here(self.lakes, x, z)
            .is_some_and(|lake| lake.depth() > -LAKE_GUARD_M)
    }

    /// What settling at `(x, z)`, `moved` metres off where the river was,
    /// costs: the ground's height and the slack per metre moved, or `None`
    /// where it may not go (the sea or a lake the river was not in).
    fn cost(&self, x: f64, z: f64, moved: f64, (sea, lake): (bool, bool)) -> Option<(f64, f64)> {
        let h = self.height(x, z);
        if (h < SEA_GUARD_M && !sea) || (!lake && self.in_lake(x, z)) {
            return None;
        }
        Some((h + self.params.settle_slack * moved.abs(), h))
    }
}

/// The least of three costs spaced evenly, refined by the parabola through
/// them: the offset of its vertex from the middle, in steps, within half a
/// step either way.
fn refine(left: Option<f64>, middle: f64, right: Option<f64>) -> f64 {
    match (left, right) {
        (Some(l), Some(r)) => {
            let curve = l - 2.0 * middle + r;
            if curve > 1e-9 {
                (0.5 * (l - r) / curve).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

/// Settles every reach onto the lowest ground near it, bends its
/// tributaries into it and moves the falls' records with it, on up to
/// `threads` threads for the ground (see the module's docs).
pub(crate) fn settle(
    built: &mut Built,
    params: &RiverParams,
    world: &ProceduralWorld,
    threads: usize,
    shore: &Shore,
) {
    for river in &mut built.rivers {
        resample(&mut river.vertices, params.resample_m);
    }
    let n = built.rivers.len();
    let mut upstream: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (r, river) in built.rivers.iter().enumerate() {
        if let RiverEnd::Joins(next) = river.end {
            upstream[next as usize].push(r);
        }
    }
    let ground = Ground {
        world,
        params,
        sites: world.sites(),
        shore,
        lakes: &built.lakes,
    };
    // The junctions first, each once for every reach that meets there,
    // and carried along each of those reaches over its next stretch, so
    // the reach turns into the junction and does not jog at it.
    let junctions: Vec<usize> = (0..n).filter(|&r| !upstream[r].is_empty()).collect();
    let moved = {
        let rivers = &built.rivers;
        let grid = LineGrid::new(rivers);
        super::field::parallel_rows(junctions.len(), threads, |k| {
            let down = junctions[k];
            let v = rivers[down].vertices[0];
            let meets = |r: usize| r == down || upstream[down].contains(&r);
            let room = grid.nearest(rivers, (v.x, v.z), JUNCTION_SEARCH_M * 3.0, meets);
            settle_point(&ground, [v.x, v.z], room)
        })
    };
    for (k, &down) in junctions.iter().enumerate() {
        let from = built.rivers[down].vertices[0];
        let shift = [moved[k][0] - from.x, moved[k][1] - from.z];
        carry(&mut built.rivers[down].vertices, shift, false);
        for &up in &upstream[down] {
            carry(&mut built.rivers[up].vertices, shift, true);
        }
    }
    let rivers = &built.rivers;
    let grid = LineGrid::new(rivers);
    let shaped = super::field::parallel_rows(n, threads, |r| {
        let fed = !upstream[r].is_empty();
        settle_reach(&ground, rivers, &grid, r, fed)
    });
    for (r, (positions, flat)) in shaped.into_iter().enumerate() {
        for ((v, p), f) in built.rivers[r].vertices.iter_mut().zip(positions).zip(flat) {
            v.x = p[0];
            v.z = p[1];
            v.meander_m = single(f);
        }
    }
    let sites = world.sites();
    confluences(built, params, &upstream, &|x, z| {
        freedom(sites, params, x, z)
    });
    // Each lake's fine ground's mean, over its nodes and the ring round
    // them, which its bed takes off (`Lake::detail_m`).
    let details = super::field::parallel_rows(built.lakes.len(), threads, |l| {
        let mut nodes: Vec<[i32; 2]> = Vec::new();
        for node in &built.lakes[l].nodes {
            for dj in -1..=1 {
                for di in -1..=1 {
                    nodes.push([node[0] + di, node[1] + dj]);
                }
            }
        }
        nodes.sort_unstable();
        nodes.dedup();
        let sum: f64 = nodes
            .iter()
            .map(|n| world.natural_detail(super::metres(n[0]), super::metres(n[1])))
            .sum();
        sum / super::real(nodes.len())
    });
    for (lake, detail) in built.lakes.iter_mut().zip(details) {
        lake.detail_m = single(detail);
    }
    // The falls' lips and feet where their vertices now stand.
    let mut next = vec![0_usize; n];
    for fall in &mut built.falls {
        let river = &built.rivers[fall.river as usize];
        let from = next[fall.river as usize];
        if let Some(k) =
            (from..river.vertices.len().saturating_sub(1)).find(|&k| river.vertices[k].fall)
        {
            let (a, b) = (river.vertices[k], river.vertices[k + 1]);
            fall.top[0] = a.x;
            fall.top[2] = a.z;
            fall.bottom[0] = b.x;
            fall.bottom[2] = b.z;
            let dir = normalise(b.x - a.x, b.z - a.z);
            fall.direction = [single(dir[0]), single(dir[1])];
            next[fall.river as usize] = k + 1;
        }
    }
}

/// Over how much of a reach a junction's move is carried, metres.
const JUNCTION_CARRY_M: f64 = 150.0;

/// Moves a reach's end (its last vertex if `at_end`, else its first) by
/// `shift`, and the vertices next to it by less as they lie farther along
/// the reach, to nothing at [`JUNCTION_CARRY_M`].
fn carry(vertices: &mut [RiverVertex], shift: [f64; 2], at_end: bool) {
    let n = vertices.len();
    let mut along = 0.0;
    for i in 0..n {
        let k = if at_end { n - 1 - i } else { i };
        if i > 0 {
            let j = if at_end { k + 1 } else { k - 1 };
            let (dx, dz) = (vertices[k].x - vertices[j].x, vertices[k].z - vertices[j].z);
            along += (dx * dx + dz * dz).sqrt();
        }
        // Never the reach's other end: a junction of its own.
        if along >= JUNCTION_CARRY_M || (i + 1 == n && n > 1) {
            break;
        }
        let w = 1.0 - smoothstep(0.0, JUNCTION_CARRY_M, along);
        vertices[k].x += shift[0] * w;
        vertices[k].z += shift[1] * w;
    }
}

/// How free a river is to move at a point, `0..=1`: nothing near a
/// settlement, whole past the margin's fade. The settling, the bends into
/// a main stem and the meanders all read it.
fn freedom(sites: &[Site], params: &RiverParams, x: f64, z: f64) -> f64 {
    if sites.is_empty() {
        1.0
    } else {
        smoothstep(
            params.site_margin_m,
            params.site_margin_m + params.site_fade_m,
            site_distance(sites, x, z),
        )
    }
}

/// A junction moved onto the lowest ground within [`JUNCTION_SEARCH_M`]
/// that is worth the move, and never more than [`CLEARANCE_SHARE`] of
/// `room` (the way to the nearest reach that does not meet there).
fn settle_point(ground: &Ground, p: [f64; 2], room: f64) -> [f64; 2] {
    let free = ground.freedom(p[0], p[1]);
    let limit = JUNCTION_SEARCH_M.min(room * CLEARANCE_SHARE) * free;
    if limit < 1.0 {
        return p;
    }
    let here = (
        ground.height(p[0], p[1]) < SEA_GUARD_M,
        ground.in_lake(p[0], p[1]),
    );
    let step = JUNCTION_SEARCH_M / f64::from(JUNCTION_STEPS);
    let mut best: Option<(f64, [f64; 2])> = None;
    for j in -JUNCTION_STEPS..=JUNCTION_STEPS {
        for i in -JUNCTION_STEPS..=JUNCTION_STEPS {
            let (ox, oz) = (f64::from(i) * step, f64::from(j) * step);
            let moved = (ox * ox + oz * oz).sqrt();
            if moved > limit {
                continue;
            }
            let q = [p[0] + ox, p[1] + oz];
            if let Some((cost, _)) = ground.cost(q[0], q[1], moved, here)
                && best.is_none_or(|(held, _)| cost < held)
            {
                best = Some((cost, q));
            }
        }
    }
    best.map_or(p, |(_, q)| q)
}

/// One reach's vertices settled sideways (see the module's docs), and how
/// flat the floor is at each, `0..=1`, for its meanders.
#[allow(
    clippy::too_many_lines,
    reason = "the frame, the two searches, the smoothing and the checks, in order"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "the reach, its vertices and the indices the searches run over"
)]
fn settle_reach(
    ground: &Ground,
    rivers: &[River],
    grid: &LineGrid,
    r: usize,
    fed: bool,
) -> (Vec<[f64; 2]>, Vec<f64>) {
    let river = &rivers[r];
    let v = &river.vertices;
    let n = v.len();
    let original: Vec<[f64; 2]> = v.iter().map(|p| [p.x, p.z]).collect();
    if n < 3 {
        return (original, vec![1.0; n]);
    }
    let params = ground.params;
    let mut arc = vec![0.0; n];
    for k in 1..n {
        let (dx, dz) = (v[k].x - v[k - 1].x, v[k].z - v[k - 1].z);
        arc[k] = arc[k - 1] + (dx * dx + dz * dz).sqrt();
    }
    let total = arc[n - 1];
    // A source's head may move; a junction's and a lake's may not here.
    let fixed_start = fed || river.from_lake.is_some();
    let mouth = if river.end == RiverEnd::Sea {
        MOUTH_KEEP_M
    } else {
        0.0
    };
    let normals: Vec<[f64; 2]> = (0..n)
        .map(|k| {
            let (a, b) = (original[k.saturating_sub(1)], original[(k + 1).min(n - 1)]);
            let t = normalise(b[0] - a[0], b[1] - a[1]);
            [-t[1], t[0]]
        })
        .collect();
    // How far each vertex may move: the search, faded in from fixed ends
    // and out near a settlement, and never past its share of the way to
    // another reach. Looking three times as far where the natural ground
    // stands far over the routing's surface (a spur the coarse field does
    // not have) moved no measure: those cuts are the coarse field's whole
    // valleys being elsewhere, not a spur's width.
    let search = params.settle_search_m;
    let limits: Vec<f64> = (0..n)
        .map(|k| {
            let from_start = if fixed_start { arc[k] } else { f64::INFINITY };
            let to_end = total - arc[k] - mouth;
            let taper = smoothstep(0.0, END_TAPER_M, from_start.min(to_end));
            if taper <= 0.0 {
                return 0.0;
            }
            let [x, z] = original[k];
            let free = ground.freedom(x, z) * taper;
            if free <= 0.0 {
                return 0.0;
            }
            let room = grid.nearest(rivers, (x, z), search / CLEARANCE_SHARE, |other| other == r);
            search.min(room * CLEARANCE_SHARE) * free
        })
        .collect();
    // Search every second vertex, and the last free one.
    let mut offsets: Vec<Option<(f64, f64)>> = vec![None; n];
    for k in 0..n {
        let searched = k % 2 == 0 || k + 2 == n;
        if !searched {
            continue;
        }
        let [x, z] = original[k];
        let limit = limits[k];
        let h0 = ground.height(x, z);
        let coarse = search / f64::from(COARSE_STEPS);
        let coarse_steps = COARSE_STEPS;
        let fine = coarse / 3.0;
        let here = (h0 < SEA_GUARD_M, ground.in_lake(x, z));
        if limit < 1.0 {
            offsets[k] = Some((0.0, 1.0));
            continue;
        }
        let at = |o: f64| [x + normals[k][0] * o, z + normals[k][1] * o];
        let try_at = |o: f64| {
            if o.abs() > limit + 1e-9 {
                return None;
            }
            let p = at(o);
            ground.cost(p[0], p[1], o, here)
        };
        // Coarse, then fine about the coarse best.
        let search = |centre: f64, step: f64, steps: i32, on_line: bool| {
            let costs: Vec<Option<(f64, f64)>> = (-steps..=steps)
                .map(|j| {
                    if j == 0 && on_line {
                        Some((h0, h0))
                    } else {
                        try_at(centre + f64::from(j) * step)
                    }
                })
                .collect();
            let best = costs
                .iter()
                .enumerate()
                .filter_map(|(j, c)| c.map(|c| (j, c)))
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.0.cmp(&b.0)));
            (costs, best)
        };
        let (costs, best) = search(0.0, coarse, coarse_steps, true);
        let Some((j, _)) = best else {
            offsets[k] = Some((0.0, 1.0));
            continue;
        };
        let shift = refine(
            j.checked_sub(1).and_then(|i| costs[i]).map(|c| c.0),
            costs[j].map_or(0.0, |c| c.0),
            costs.get(j + 1).copied().flatten().map(|c| c.0),
        );
        let index = |j: usize| f64::from(i32::try_from(j).expect("a handful of candidates"));
        let centre = (index(j) - f64::from(coarse_steps) + shift) * coarse;
        let centre = centre.clamp(-limit, limit);
        let (costs, best) = search(centre, fine, FINE_STEPS, false);
        let Some((j, (_, low))) = best else {
            offsets[k] = Some((0.0, 1.0));
            continue;
        };
        let shift = refine(
            j.checked_sub(1).and_then(|i| costs[i]).map(|c| c.0),
            costs[j].map_or(0.0, |c| c.0),
            costs.get(j + 1).copied().flatten().map(|c| c.0),
        );
        let o = (centre + (index(j) - f64::from(FINE_STEPS) + shift) * fine).clamp(-limit, limit);
        // How far the floor rises either side of where the river settled,
        // from the fine search's outermost points.
        let sides: Vec<f64> = [costs[0], costs[costs.len() - 1]]
            .iter()
            .flatten()
            .map(|c| c.1 - low)
            .collect();
        let rise = if sides.is_empty() {
            FLAT_RISE_M.1
        } else {
            sides.iter().sum::<f64>() / super::real(sides.len())
        };
        offsets[k] = Some((o, 1.0 - smoothstep(FLAT_RISE_M.0, FLAT_RISE_M.1, rise)));
    }
    // The vertices between, by their neighbours; a fixed end stays put.
    let mut offset = vec![0.0; n];
    let mut flat = vec![1.0; n];
    let known: Vec<usize> = (0..n).filter(|&k| offsets[k].is_some()).collect();
    for k in 0..n {
        let after = known.partition_point(|&i| i < k);
        let (o, f) = if let Some(&i) = known.get(after).filter(|&&i| i == k) {
            offsets[i].expect("known")
        } else {
            let lo = after.checked_sub(1).map(|i| known[i]);
            let hi = known.get(after).copied();
            match (lo, hi) {
                (Some(a), Some(b)) => {
                    let (oa, fa) = offsets[a].expect("known");
                    let (ob, fb) = offsets[b].expect("known");
                    let t = (arc[k] - arc[a]) / (arc[b] - arc[a]).max(1e-9);
                    (oa + (ob - oa) * t, fa + (fb - fa) * t)
                }
                (Some(a), None) | (None, Some(a)) => offsets[a].expect("known"),
                (None, None) => (0.0, 1.0),
            }
        };
        offset[k] = o;
        flat[k] = f;
    }
    // Smoothed along the reach, its turn per metre held, and each within
    // its limit.
    for _ in 0..3 {
        let held = offset.clone();
        for k in 1..n - 1 {
            offset[k] = 0.25 * (held[k - 1] + 2.0 * held[k] + held[k + 1]);
        }
    }
    for k in 0..n {
        offset[k] = offset[k].clamp(-limits[k], limits[k]);
    }
    for k in 1..n {
        let step = 0.5 * (arc[k] - arc[k - 1]);
        offset[k] = offset[k].clamp(offset[k - 1] - step, offset[k - 1] + step);
    }
    for k in (0..n - 1).rev() {
        let step = 0.5 * (arc[k + 1] - arc[k]);
        offset[k] = offset[k].clamp(offset[k + 1] - step, offset[k + 1] + step);
    }
    let place = |offset: &[f64]| -> Vec<[f64; 2]> {
        (0..n)
            .map(|k| {
                [
                    original[k][0] + normals[k][0] * offset[k],
                    original[k][1] + normals[k][1] * offset[k],
                ]
            })
            .collect()
    };
    // No segment turned back on itself: where the offsets fold one over,
    // they are halved about it until none is.
    let mut positions = place(&offset);
    for _ in 0..4 {
        let mut folded = false;
        for k in 0..n - 1 {
            let new = normalise(
                positions[k + 1][0] - positions[k][0],
                positions[k + 1][1] - positions[k][1],
            );
            let old = normalise(
                original[k + 1][0] - original[k][0],
                original[k + 1][1] - original[k][1],
            );
            if new[0] * old[0] + new[1] * old[1] < 0.5 {
                offset[k] *= 0.5;
                offset[k + 1] *= 0.5;
                folded = true;
            }
        }
        if !folded {
            break;
        }
        positions = place(&offset);
    }
    (positions, flat)
}

/// Bends each tributary that meets its main stem more square on than
/// [`RiverParams::confluence_tan`] allows into a junction at that angle,
/// pointing downstream, over its last stretch; never where the bend would
/// cross the main stem or the reach below, nor near a settlement.
#[allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "the junction, its reaches and their vertices; the angle, the bend and its check"
)]
fn confluences(
    built: &mut Built,
    params: &RiverParams,
    upstream: &[Vec<usize>],
    freedom: &impl Fn(f64, f64) -> f64,
) {
    // The cosine of the angle off the upstream line a tributary keeps:
    // from the tangent, by a square root.
    let cos = 1.0 / (1.0 + params.confluence_tan * params.confluence_tan).sqrt();
    for (down, ups) in upstream.iter().enumerate() {
        if ups.len() < 2 {
            continue;
        }
        let dv = &built.rivers[down].vertices;
        let j = [dv[0].x, dv[0].z];
        // Downstream: over the reach below's first 60 m.
        let ahead = dv
            .iter()
            .find(|p| (p.x - j[0]) * (p.x - j[0]) + (p.z - j[1]) * (p.z - j[1]) >= 3_600.0)
            .unwrap_or(&dv[dv.len() - 1]);
        let along = normalise(ahead.x - j[0], ahead.z - j[1]);
        if along == [0.0, 0.0] {
            continue;
        }
        let width = params.width(f64::from(dv[0].discharge_m3s));
        let reach = (width * 6.0 + 60.0).clamp(80.0, CONFLUENCE_MOST_M);
        let main = *ups
            .iter()
            .max_by(|&&a, &&b| {
                let q = |r: usize| {
                    let v = &built.rivers[r].vertices;
                    v[v.len() - 1].discharge_m3s
                };
                q(a).total_cmp(&q(b)).then(b.cmp(&a))
            })
            .expect("two reaches meet");
        let left = [-along[1], along[0]];
        let segments = |r: usize, from_end: bool| -> Vec<([f64; 2], [f64; 2])> {
            let v = &built.rivers[r].vertices;
            let near = |p: &RiverVertex| {
                (p.x - j[0]) * (p.x - j[0]) + (p.z - j[1]) * (p.z - j[1]) < 4.0 * reach * reach
            };
            let points: Vec<[f64; 2]> = if from_end {
                v.iter()
                    .rev()
                    .take_while(|p| near(p))
                    .map(|p| [p.x, p.z])
                    .collect()
            } else {
                v.iter()
                    .take_while(|p| near(p))
                    .map(|p| [p.x, p.z])
                    .collect()
            };
            points.windows(2).map(|w| (w[0], w[1])).collect()
        };
        let mut guards = segments(main, true);
        guards.extend(segments(down, false));
        for &trib in ups {
            if trib == main {
                continue;
            }
            let v = &built.rivers[trib].vertices;
            let m = v.len();
            if m < 3 {
                continue;
            }
            // Metres back from the junction, and the vertex about `reach`
            // back that says which way the tributary comes in.
            let mut back = vec![0.0; m];
            for k in (0..m - 1).rev() {
                let (dx, dz) = (v[k + 1].x - v[k].x, v[k + 1].z - v[k].z);
                back[k] = back[k + 1] + (dx * dx + dz * dz).sqrt();
            }
            let far = (0..m).rev().find(|&k| back[k] >= reach).unwrap_or(0);
            let from = normalise(v[far].x - j[0], v[far].z - j[1]);
            // Its cosine off the main stem's upstream line: square on is
            // nought, coming down the main stem's line one.
            let c = -(from[0] * along[0] + from[1] * along[1]);
            if c >= cos || c < -0.2 {
                continue;
            }
            let side = if from[0] * left[0] + from[1] * left[1] < 0.0 {
                -1.0
            } else {
                1.0
            };
            let target = normalise(
                -along[0] + side * params.confluence_tan * left[0],
                -along[1] + side * params.confluence_tan * left[1],
            );
            let mut bent: Vec<[f64; 2]> = v.iter().map(|p| [p.x, p.z]).collect();
            for k in 0..m - 1 {
                if back[k] >= reach {
                    continue;
                }
                let wgt = (1.0 - smoothstep(0.0, reach, back[k])) * freedom(v[k].x, v[k].z);
                let t = [j[0] + target[0] * back[k], j[1] + target[1] * back[k]];
                bent[k] = [
                    bent[k][0] + (t[0] - bent[k][0]) * wgt,
                    bent[k][1] + (t[1] - bent[k][1]) * wgt,
                ];
            }
            let stretch: Vec<([f64; 2], [f64; 2])> = (0..m - 1)
                .filter(|&k| back[k + 1] < reach * 2.0)
                .map(|k| (bent[k], bent[k + 1]))
                .collect();
            let clear = stretch
                .iter()
                .all(|&(a, b)| guards.iter().all(|&(c, d)| !crosses(a, b, c, d)));
            if !clear {
                continue;
            }
            let v = &mut built.rivers[trib].vertices;
            for (p, q) in v.iter_mut().zip(bent) {
                (p.x, p.z) = (q[0], q[1]);
            }
        }
    }
}

/// Gives every reach its meanders: an amplitude per vertex and a phase
/// (see the module's docs). After the surfaces are fitted, whose gradient
/// says how steep the river runs; the settling left in
/// [`RiverVertex::meander_m`] how flat the floor is.
#[allow(
    clippy::too_many_lines,
    reason = "the order, the limits, the smoothing and the phase, in order"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "the reach, its vertices and the indices the passes run over"
)]
pub(crate) fn meander(built: &mut Built, params: &RiverParams, seed: Seed, sites: &[Site]) {
    let n = built.rivers.len();
    let size = Fractal::new(
        seed.derive("rivers/bend-size"),
        Shape::Fbm,
        BEND_NOISE_M.0,
        super::NOISE_OCTAVES,
        super::NOISE_GAIN,
        super::NOISE_LACUNARITY,
    );
    let length = Fractal::new(
        seed.derive("rivers/bend-length"),
        Shape::Fbm,
        BEND_NOISE_M.1,
        super::NOISE_OCTAVES,
        super::NOISE_GAIN,
        super::NOISE_LACUNARITY,
    );
    let phase_seed = seed.derive("rivers/bend-phase");
    let grid = LineGrid::new(&built.rivers);
    let mut upstream: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut below: Vec<Option<usize>> = vec![None; n];
    for (r, river) in built.rivers.iter().enumerate() {
        if let RiverEnd::Joins(next) = river.end {
            upstream[next as usize].push(r);
            below[r] = Some(next as usize);
        }
    }
    // Mouths first (this order reversed), so a reach's phase runs into the
    // one below's.
    let mut waiting: Vec<usize> = upstream.iter().map(Vec::len).collect();
    let mut order: Vec<usize> = (0..n).filter(|&r| waiting[r] == 0).collect();
    let mut at = 0;
    while at < order.len() {
        if let Some(next) = below[order[at]] {
            waiting[next] -= 1;
            if waiting[next] == 0 {
                order.push(next);
            }
        }
        at += 1;
    }
    let mut phase_start = vec![0.0_f64; n];
    for &r in order.iter().rev() {
        let rivers = &built.rivers;
        let v = &rivers[r].vertices;
        let m = v.len();
        let mut arc = vec![0.0; m];
        for k in 1..m {
            let (dx, dz) = (v[k].x - v[k - 1].x, v[k].z - v[k - 1].z);
            arc[k] = arc[k - 1] + (dx * dx + dz * dz).sqrt();
        }
        let total = arc[m - 1];
        let wavelength: Vec<f64> = v
            .iter()
            .map(|p| {
                let width = params.width(f64::from(p.discharge_m3s));
                (params.bend_widths * width * (1.0 + 0.35 * length.get(p.x, p.z).clamp(-1.0, 1.0)))
                    .max(4.0)
            })
            .collect();
        let mut limit = vec![0.0; m];
        let mut amplitude = vec![0.0; m];
        for k in 0..m {
            let p = v[k];
            let q = f64::from(p.discharge_m3s);
            let width = params.width(q);
            let most = params.bend_amplitude(q);
            let (a, b) = (k.saturating_sub(1), (k + 1).min(m - 1));
            let run = arc[b] - arc[a];
            let slope = if run > 0.0 {
                (f64::from(v[a].surface_m) - f64::from(v[b].surface_m)) / run
            } else {
                0.0
            };
            let falls = p.fall || (k > 0 && v[k - 1].fall);
            let lambda = wavelength[k];
            let ends = smoothstep(0.0, 0.75 * lambda, arc[k])
                * smoothstep(0.0, 0.75 * lambda, total - arc[k]);
            let mouth = if rivers[r].end == RiverEnd::Sea {
                smoothstep(0.0, MOUTH_STRAIGHT_M, total - arc[k] - MOUTH_STRAIGHT_M)
            } else {
                1.0
            };
            // The line's own radius of turning here: the meander may take a
            // share of it on the bend's inside.
            let radius = if k > 0 && k + 1 < m {
                let u1 = normalise(p.x - v[k - 1].x, p.z - v[k - 1].z);
                let u2 = normalise(v[k + 1].x - p.x, v[k + 1].z - p.z);
                let chord =
                    ((u2[0] - u1[0]) * (u2[0] - u1[0]) + (u2[1] - u1[1]) * (u2[1] - u1[1])).sqrt();
                if chord > 1e-9 {
                    0.5 * (arc[k + 1] - arc[k - 1]) / chord
                } else {
                    f64::INFINITY
                }
            } else {
                f64::INFINITY
            };
            let room = grid.nearest(
                rivers,
                (p.x, p.z),
                (most * 4.0 + width * 2.0).max(20.0),
                |other| other == r,
            );
            let cap = if falls {
                0.0
            } else {
                most.min(RADIUS_SHARE * radius)
                    .min(ROOM_SHARE * (room - width).max(0.0))
                    * ends
                    * mouth
                    * freedom(sites, params, p.x, p.z)
            };
            let steep = STEEP_LEAST
                + (1.0 - STEEP_LEAST) * (1.0 - smoothstep(MEANDER_SLOPE.0, MEANDER_SLOPE.1, slope));
            // A deep cut is a gorge: its floor holds the river to its line.
            let confined = 1.0
                - CONFINED_LEAST
                    * smoothstep(CONFINED_CUT_M.0, CONFINED_CUT_M.1, f64::from(p.cut_m));
            let flat = f64::from(p.meander_m).clamp(0.0, 1.0) * confined;
            let varied = (0.7 + 0.4 * size.get(p.x, p.z)).clamp(0.2, 1.0);
            limit[k] = cap;
            amplitude[k] = (most * flat * steep * varied).min(cap);
        }
        for _ in 0..2 {
            let held = amplitude.clone();
            for k in 1..m.saturating_sub(1) {
                amplitude[k] = 0.25 * (held[k - 1] + 2.0 * held[k] + held[k + 1]);
            }
        }
        // The phase: every reach that meets at a junction ends there at the
        // phase the reach below starts at, so the wave (the riffles above
        // all, which a straight reach keeps too) is one through a
        // confluence; a reach into the sea, a lake or an edge ends at a
        // hash of where it ends. Set from the mouths up.
        let end_phase = below[r].map_or_else(
            || {
                let last = v[m - 1];
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a world coordinate in metres is far inside an i64"
                )]
                let (xq, zq) = (last.x.round() as i64, last.z.round() as i64);
                unit(mix64(
                    phase_seed
                        ^ mix64(xq.cast_unsigned())
                        ^ mix64(zq.cast_unsigned()).rotate_left(29),
                ))
            },
            |next| phase_start[next],
        );
        let mut phases = vec![0.0; m];
        let mut phase = end_phase;
        for k in (0..m).rev() {
            if k + 1 < m {
                phase -= (arc[k + 1] - arc[k]) / f64::midpoint(wavelength[k + 1], wavelength[k]);
            }
            phases[k] = phase;
        }
        phase_start[r] = phase;
        let v = &mut built.rivers[r].vertices;
        for k in 0..m {
            v[k].meander_m = single(amplitude[k].min(limit[k]).max(0.0));
            v[k].phase = single(phases[k]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_is_smooth_and_its_mean_square_is_its_own() {
        // Value, slope and curvature continuous across every joint, against
        // differences; the apex at a quarter, the crossings at the halves.
        let e = 1e-6;
        let mut sum = 0.0;
        let steps = 100_000;
        for k in 0..steps {
            let phase = -1.0 + 3.0 * f64::from(k) / f64::from(steps);
            let (p, dp, ddp) = sine(phase);
            let (p1, dp1, _) = sine(phase + e);
            let (p0, dp0, _) = sine(phase - e);
            assert!(
                ((p1 - p0) / (2.0 * e) - dp).abs() < 1e-4,
                "slope at {phase}"
            );
            assert!(
                ((dp1 - dp0) / (2.0 * e) - ddp).abs() < 1e-3,
                "curve at {phase}"
            );
            if (0.0..1.0).contains(&phase) {
                sum += p * p;
            }
            // The whole meander likewise, and within its bounds.
            let w = wave(phase * 7.3 - 4.0);
            let (w1, w0) = (wave(phase * 7.3 - 4.0 + e), wave(phase * 7.3 - 4.0 - e));
            assert!(w.offset.abs() <= 1.0 + 1e-9, "{w:?}");
            assert!(((w1.offset - w0.offset) / (2.0 * e) - w.d_offset).abs() < 1e-3);
            assert!(((w1.d_offset - w0.d_offset) / (2.0 * e) - w.dd_offset).abs() < 1e-2);
            assert!(((w1.riffle - w0.riffle) / (2.0 * e) - w.d_riffle).abs() < 1e-3);
        }
        let mean = sum / f64::from(steps / 3);
        assert!((mean - SINE_MEAN_SQUARE).abs() < 1e-3, "{mean}");
        assert!((sine(0.25).0 - 1.0).abs() < 1e-12);
        assert!(sine(0.5).0.abs() < 1e-12 && sine(1.0).0.abs() < 1e-12);
        assert!((sine(0.75).0 + 1.0).abs() < 1e-12);
    }
}
