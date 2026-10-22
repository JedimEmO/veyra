//! The road's grade (ticket 0005, step 6; ADR 0012, decision 7): a profile
//! of its own for every service road and track the bake lays, and the
//! ground a sample takes from it.
//!
//! - **The profile** ([`profile_all`]), in the bake once the roads are
//!   recorded, on a grid every [`PROFILE_STEP_M`] along each road: the
//!   recorded ground averaged over [`Way::balance_m`], so cut and fill
//!   balance along the road; held to the pad's level at a site, to the
//!   through road's level at a junction (each road after those it ends on,
//!   so every arm meets it at one height), to a bridge's or a culvert's
//!   deck between its abutments and a shelf behind each, and to a ford's
//!   bed between its wetted edges; held to the class's greatest gradient
//!   (the mean of a forward and a backward pass, so neither end is
//!   favoured), let steeper only where holding it would cut deeper than
//!   [`Way::cut_most_m`] or fill higher than [`Way::fill_most_m`]; and its
//!   rise smoothed over [`Way::curve_m`] and put back on its pins, so it
//!   bends into and out of a climb in vertical curves. Each recorded point
//!   keeps the level and the rise there; between points the level is the
//!   cubic of both ([`crate::roads::Road::level_at`]), continuous with its
//!   rise. A trail gets none: it follows the ground.
//! - **The ground** ([`RoadGrid::shape`]), in the sample right after the
//!   carve and before a site's pad: on the carriageway the profile less a
//!   crown's fall, carried on across the verge; past the verge a cut bank
//!   up to the ground (soil at [`Way::soil`] for its first metres, steeper
//!   at the rock's [`Way::rock`] above them) or a fill bank down to it (a
//!   grassed [`Way::fill`], steepening past [`Way::fill_drop_m`]), its
//!   shoulder and its toe rounded, its foot, its angle and where it turns
//!   to rock wandering with a noise tens of metres long, so no edge runs
//!   straight or parallel to the road. Every number carries its gradient,
//!   so the sample's gradient is the graded height's. At a bridge the
//!   ground stops at the abutments: past each the fill's slope runs down
//!   into the gap, and the river's valley under the deck is left as the
//!   carve made it. A trail only clears and wears its line.
//! - **The lookup.** A sampled lattice cell gathers, once, every segment of
//!   every road whose ground can reach it into buckets [`BUCKET_M`] wide, so
//!   a point far from any road reads one empty bucket and a point by one a
//!   handful of segments. Roads are laid over each other arms first, so the
//!   road an arm ends on has the last word on its own carriageway.

use std::sync::Arc;

use strata_data::RoadClass;

use super::dual::{D, smooth_c, soft};
use super::field::parallel_rows;
use super::{LATTICE_M, Network};
use crate::noise::smoothstep;
use crate::roads::{Crossing, CrossingKind, Junction, Road, RoadEnd, RoadPoint, hermite};
use crate::sites::ProceduralSite;

/// What a class of road does to the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Way {
    /// Whether the class has a profile; a trail follows the ground.
    pub graded: bool,
    /// Half the carriageway, metres.
    pub half_m: f64,
    /// The verge past it, metres.
    pub verge_m: f64,
    /// The carriageway's fall from its crown at its edge, rise over run;
    /// the verge carries it on.
    pub crossfall: f64,
    /// A cut bank's slope at its foot, rise over run: soil, as steep as a
    /// grassed bank stands.
    pub soil: f64,
    /// How high a cut bank climbs at the soil's slope before it steepens.
    pub soil_rise_m: f64,
    /// A deep cut's slope above that: the rock's.
    pub rock: f64,
    /// A fill bank's slope, rise over run.
    pub fill: f64,
    /// How far a fill drops at that before it steepens.
    pub fill_drop_m: f64,
    /// A high fill's slope past that.
    pub fill_steep: f64,
    /// How far past the verge a bank reaches before it is faded into the
    /// ground, metres: one on ground steeper than the bank never meets it.
    pub reach_m: f64,
    /// A bridge's deck over the water, metres.
    pub freeboard_m: f64,
    /// The level shelf behind each of a bridge's abutments, metres.
    pub shelf_m: f64,
    /// The window the profile's target averages the ground over, metres.
    pub balance_m: f64,
    /// The window the profile's rise is smoothed over, metres: a vertical
    /// curve's length.
    pub curve_m: f64,
    /// The deepest cut the profile makes to hold its grade, metres.
    pub cut_most_m: f64,
    /// The highest fill.
    pub fill_most_m: f64,
}

/// What `class` does to the ground.
#[must_use]
pub(crate) const fn way(class: RoadClass) -> Way {
    let half_m = crate::sites::way_in_half_m(class);
    match class {
        RoadClass::Service => Way {
            graded: true,
            half_m,
            verge_m: 2.5,
            crossfall: 0.025,
            soil: 0.6,
            soil_rise_m: 3.0,
            rock: 1.8,
            fill: 0.5,
            fill_drop_m: 4.0,
            fill_steep: 1.0,
            reach_m: 40.0,
            freeboard_m: 2.5,
            shelf_m: 6.0,
            balance_m: 120.0,
            curve_m: 48.0,
            cut_most_m: 18.0,
            fill_most_m: 10.0,
        },
        RoadClass::Track => Way {
            graded: true,
            half_m,
            verge_m: 1.5,
            crossfall: 0.03,
            soil: 0.65,
            soil_rise_m: 2.5,
            rock: 1.8,
            fill: 0.55,
            fill_drop_m: 3.0,
            fill_steep: 1.0,
            reach_m: 30.0,
            freeboard_m: 2.0,
            shelf_m: 4.0,
            balance_m: 70.0,
            curve_m: 28.0,
            cut_most_m: 12.0,
            fill_most_m: 7.0,
        },
        RoadClass::Trail => Way {
            graded: false,
            half_m,
            verge_m: 1.0,
            crossfall: 0.0,
            soil: 0.0,
            soil_rise_m: 0.0,
            rock: 0.0,
            fill: 0.0,
            fill_drop_m: 0.0,
            fill_steep: 0.0,
            reach_m: 0.0,
            freeboard_m: 1.5,
            shelf_m: 2.0,
            balance_m: 0.0,
            curve_m: 0.0,
            cut_most_m: 0.0,
            fill_most_m: 0.0,
        },
    }
}

/// Metres between the nodes a profile is fitted on.
pub(crate) const PROFILE_STEP_M: f64 = 4.0;

/// Rounds of balancing a profile's cut against its fill.
const BALANCE_ROUNDS: usize = 6;

/// How far past a creek's water a culvert's fill is held at its deck,
/// metres.
const CULVERT_HELD_M: f64 = 2.0;

/// How far the verge's edge, and so a bank's foot, wanders either way,
/// metres.
const EDGE_WANDER_M: f64 = 1.0;

/// The softness of a bank's foot, metres: a cut bank rises out of the verge
/// in a hollow this long, not a crease.
const FOOT_M: f64 = 1.0;

/// The softness of a bank's turn to the steeper slope above, metres.
const TURN_M: f64 = 1.5;

/// The roundest a cut's shoulder or a fill's toe is, metres of height.
const SHOULDER_MOST_M: f64 = 4.0;

/// Over its last this many metres a bank's reach fades it into the ground.
const FADE_M: f64 = 10.0;

/// How far into a bridge's gap, per metre, the carriageway's material and
/// the cleared way are taken to end: the abutment's face.
const GAP_EDGE: f64 = 4.0;

/// How many metres of a bank a metre into a bridge's gap counts as: the
/// abutment's face falls at about this times a fill's slope, so the ground
/// stops at the abutment and the gap under the deck is left to the river.
const ABUTMENT_FACE: f64 = 6.0;

/// Metres along an arm from its junction over which its carriageway widens
/// into the road it ends on.
const MOUTH_M: f64 = 14.0;

/// How many times over the distance past an arm's end on a junction counts:
/// its carriageway, a few metres round, ends within the through road's,
/// which is at least 3.5 m to its verge either side.
const END_SQUASH: f64 = 3.0;

/// How developed a trail's worn line is at its middle: wholly, so nothing
/// grows on it and its soil shows.
const TRAIL_WORN: f64 = 1.0;

/// Every road's profile, set on `roads` in place: each after the roads it
/// ends on, on `threads`. A pure function of the records, so the same on
/// any thread count.
pub(crate) fn profile_all(
    roads: &mut [Road],
    junctions: &[Junction],
    sites: &[ProceduralSite],
    floor: &(dyn Fn(f64, f64) -> Option<f64> + Sync),
    threads: usize,
) {
    // A road ends only on one laid before it: one past the deepest of
    // those.
    let mut depth = vec![0_usize; roads.len()];
    for r in 0..roads.len() {
        for end in roads[r].ends {
            if let RoadEnd::Junction(j) = end {
                let through = junctions[j as usize].road as usize;
                if through < r {
                    depth[r] = depth[r].max(depth[through] + 1);
                }
            }
        }
    }
    let mut keys: Vec<(u64, usize)> = sites
        .iter()
        .enumerate()
        .map(|(k, site)| (site.id.key(), k))
        .collect();
    keys.sort_unstable();
    let deepest = depth.iter().copied().max().unwrap_or(0);
    for level in 0..=deepest {
        let rows: Vec<usize> = (0..roads.len()).filter(|&r| depth[r] == level).collect();
        let fitted = {
            let laid: &[Road] = roads;
            parallel_rows(rows.len(), threads, |i| {
                fit_road(laid, rows[i], junctions, sites, &keys, floor)
            })
        };
        for (i, profile) in fitted.into_iter().enumerate() {
            roads[rows[i]].profile = profile;
        }
    }
}

/// Where along a road of `way` its profile is held at a crossing's deck,
/// `[from, to]` metres: a bridge's abutments and a shelf behind each, a
/// culvert's channel and its banks, a ford's wetted width. The bake records
/// a point at each end, so the profile between them is level.
pub(crate) fn held(way: &Way, c: &Crossing) -> [f64; 2] {
    let [from, to] = c.abutments_m;
    match c.kind {
        CrossingKind::Bridge => [from - way.shelf_m, to + way.shelf_m],
        // A culvert's fill is held over the channel and its banks alone:
        // its recorded span runs on over a floodplain lower than the creek,
        // which the profile fills or not as the grade says.
        CrossingKind::Culvert => {
            let half = 0.5 * c.width_m + CULVERT_HELD_M;
            [c.along_m - half, c.along_m + half]
        }
        CrossingKind::Ford => [from, to],
    }
}

/// The most metres along an arm its junction holds it level: an arm meeting
/// its road very obliquely is held this far and no further.
const JUNCTION_HOLD_MOST_M: f64 = 40.0;

/// How an arm ending at `side` on `through` (at its junction `at`, `along`
/// metres along `through`) is held: the metres along the arm from that end
/// to where it stands clear of the through road's verge and the foot of its
/// bank, measured square to the through road, and how far under the through
/// road's centreline level its crown has fallen there. The arm runs level
/// at that height until it is clear, so it leaves the through road's
/// carriageway at its crowned surface and only then climbs: pinned at the
/// centreline alone, an arm on a slope met the through road's verge with a
/// lip as steep as a soil bank.
#[allow(
    clippy::many_single_char_names,
    reason = "a road's segment, its direction and the share along it"
)]
pub(crate) fn junction_hold(
    arm: &Road,
    side: usize,
    through: &Road,
    at: [f64; 2],
    along: f64,
) -> (f64, f64) {
    let w = way(through.class);
    let clear = w.half_m + w.verge_m + EDGE_WANDER_M + FOOT_M;
    let drop = if through.graded() {
        (clear - 0.5 * w.half_m) * w.crossfall
    } else {
        0.0
    };
    let points = &through.points;
    if points.len() < 2 || arm.points.len() < 2 {
        return (0.0, drop);
    }
    let k = points
        .partition_point(|p| p.along_m <= along)
        .clamp(1, points.len() - 1);
    let (a, b) = (points[k - 1], points[k]);
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let l = (dx * dx + dz * dz).sqrt().max(1e-9);
    let t = [dx / l, dz / l];
    let square = |p: &RoadPoint| ((p.x - at[0]) * t[1] - (p.z - at[1]) * t[0]).abs();
    let length = arm.length_m();
    let from_end = |p: &RoadPoint| {
        if side == 0 {
            p.along_m
        } else {
            length - p.along_m
        }
    };
    let n = arm.points.len();
    let mut last = (0.0, 0.0);
    for i in 0..n {
        let p = if side == 0 {
            &arm.points[i]
        } else {
            &arm.points[n - 1 - i]
        };
        let (s, d) = (from_end(p), square(p));
        if s > JUNCTION_HOLD_MOST_M {
            break;
        }
        if d >= clear {
            let share = if d > last.1 {
                (clear - last.1) / (d - last.1)
            } else {
                1.0
            };
            return (last.0 + (s - last.0) * share.clamp(0.0, 1.0), drop);
        }
        last = (s, d);
    }
    (JUNCTION_HOLD_MOST_M.min(length), drop)
}

/// The ground a road's record has under it `along` metres from its first
/// end, by its points.
fn ground_at(points: &[RoadPoint], along: f64) -> f64 {
    let k = points
        .partition_point(|p| p.along_m <= along)
        .clamp(1, points.len().max(2) - 1);
    if points.len() < 2 {
        return points.first().map_or(0.0, |p| f64::from(p.ground_m));
    }
    let (a, b) = (points[k - 1], points[k]);
    let span = b.along_m - a.along_m;
    let t = if span > 0.0 {
        ((along - a.along_m) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    f64::from(a.ground_m) + (f64::from(b.ground_m) - f64::from(a.ground_m)) * t
}

/// Road `r`'s profile: its pins from its ends and crossings, then [`fit`].
fn fit_road(
    roads: &[Road],
    r: usize,
    junctions: &[Junction],
    sites: &[ProceduralSite],
    keys: &[(u64, usize)],
    floor: &(dyn Fn(f64, f64) -> Option<f64> + Sync),
) -> Vec<[f32; 2]> {
    let road = &roads[r];
    let way = way(road.class);
    if !way.graded || road.is_settlements() || road.points.len() < 2 {
        return Vec::new();
    }
    // The water's floor at each point and across the road out to its banks'
    // reach, so a bank does not cut a lake's rim under its level either.
    let floors: Vec<f64> = (0..road.points.len())
        .map(|k| {
            let (a, b) = (
                road.points[k.saturating_sub(1)],
                road.points[(k + 1).min(road.points.len() - 1)],
            );
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let length = (dx * dx + dz * dz).sqrt().max(1e-9);
            let normal = [-dz / length, dx / length];
            let p = road.points[k];
            [-1.0, -0.5, 0.0, 0.5, 1.0]
                .iter()
                .filter_map(|f| {
                    let out = f * (way.half_m + way.verge_m + way.reach_m);
                    floor(p.x + normal[0] * out, p.z + normal[1] * out)
                })
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect();
    // `[from, to, level]`, metres along; the first pin on a node holds it.
    let mut pins: Vec<[f64; 3]> = Vec::new();
    let length = road.length_m();
    for (side, end) in road.ends.iter().enumerate() {
        let p = if side == 0 {
            road.points[0]
        } else {
            road.points[road.points.len() - 1]
        };
        match *end {
            RoadEnd::Site(id) => {
                let Ok(k) = keys.binary_search_by_key(&id.key(), |&(key, _)| key) else {
                    continue;
                };
                let level = sites[keys[k].1].pad_height_at(p.x, p.z);
                pins.push([p.along_m, p.along_m, level]);
            }
            RoadEnd::Junction(j) => {
                // Level with the through road out past its verge, at its
                // crowned surface there (see `junction_hold`).
                let junction = junctions[j as usize];
                let through = &roads[junction.road as usize];
                let level = through
                    .level_at(junction.along_m)
                    .map_or_else(|| ground_at(&through.points, junction.along_m), |l| l.0);
                let (hold, drop) =
                    junction_hold(road, side, through, junction.at, junction.along_m);
                let run = if side == 0 {
                    [0.0, hold]
                } else {
                    [length - hold, length]
                };
                pins.push([run[0], run[1], level - drop]);
            }
            RoadEnd::Settlement(_) | RoadEnd::RoadHead(_) => {}
        }
    }
    for c in &road.crossings {
        let [from, to] = held(&way, c);
        pins.push([from, to, c.deck_m]);
    }
    fit(
        &road.points,
        &way,
        crate::roads::spec(road.class).max_grade,
        &pins,
        &floors,
    )
}

/// A box average of `values`, `half` nodes either side, the window cut at
/// the ends: by prefix sums, so each node costs the same.
fn box_average(values: &[f64], half: usize) -> Vec<f64> {
    let n = values.len();
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0);
    let mut sum = 0.0;
    for v in values {
        sum += v;
        prefix.push(sum);
    }
    (0..n)
        .map(|k| {
            let lo = k.saturating_sub(half);
            let hi = (k + half).min(n - 1);
            #[allow(
                clippy::cast_precision_loss,
                reason = "a window of a few hundred nodes"
            )]
            let count = (hi - lo + 1) as f64;
            (prefix[hi + 1] - prefix[lo]) / count
        })
        .collect()
}

/// Each node held within `limit` of the one before it, pinned nodes kept.
fn forward(h: &mut [f64], limit: &[f64], pin: &[Option<f64>]) {
    for k in 1..h.len() {
        if pin[k].is_none() {
            h[k] = h[k].clamp(h[k - 1] - limit[k - 1], h[k - 1] + limit[k - 1]);
        }
    }
}

/// Each node held within `limit` of the one after it, pinned nodes kept.
fn backward(h: &mut [f64], limit: &[f64], pin: &[Option<f64>]) {
    for k in (0..h.len().saturating_sub(1)).rev() {
        if pin[k].is_none() {
            h[k] = h[k].clamp(h[k + 1] - limit[k], h[k + 1] + limit[k]);
        }
    }
}

/// A profile over `points` for a class of `way` climbing at most `grade`,
/// held to `pins` (`[from, to, level]`, metres along): per point its level
/// and its rise per metre.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "node counts of a road at a few metres, far inside every width"
)]
#[allow(
    clippy::too_many_lines,
    reason = "the grid, the pins, the target, the grade, the depth and the curves, in order"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "grid size, ground, levels and indices, as the fit's maths names them"
)]
pub(crate) fn fit(
    points: &[RoadPoint],
    way: &Way,
    grade: f64,
    pins: &[[f64; 3]],
    floors: &[f64],
) -> Vec<[f32; 2]> {
    let length = points.last().map_or(0.0, |p| p.along_m);
    if points.len() < 2 || length <= 0.0 {
        return points.iter().map(|p| [p.ground_m, 0.0]).collect();
    }
    let n = ((length / PROFILE_STEP_M).ceil() as usize).max(1) + 1;
    let step = length / (n - 1) as f64;
    let along = |k: usize| k as f64 * step;
    // The ground on the grid, and the water's floor: a road beside a river
    // or a lake never runs, nor cuts its banks, under the water's level.
    let mut g = Vec::with_capacity(n);
    let mut low = Vec::with_capacity(n);
    let mut i = 0;
    for k in 0..n {
        let s = along(k);
        while i + 2 < points.len() && points[i + 1].along_m < s {
            i += 1;
        }
        let (a, b) = (points[i], points[i + 1]);
        let span = b.along_m - a.along_m;
        let t = if span > 0.0 {
            ((s - a.along_m) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        g.push(f64::from(a.ground_m) + (f64::from(b.ground_m) - f64::from(a.ground_m)) * t);
        low.push(
            floors
                .get(i)
                .copied()
                .unwrap_or(f64::NEG_INFINITY)
                .max(floors.get(i + 1).copied().unwrap_or(f64::NEG_INFINITY)),
        );
    }
    // The pins on the grid: every node from the one at or before a pin's
    // start to the one at or after its end, so a point recorded at either
    // end lies on the held run and stands at its level.
    let mut pin: Vec<Option<f64>> = vec![None; n];
    for &[from, to, level] in pins {
        let lo = (from / step).floor().max(0.0) as usize;
        let hi = ((to / step).ceil().max(0.0) as usize).min(n - 1);
        let (lo, hi) = if lo > hi || lo >= n {
            let mid = ((f64::midpoint(from, to) / step).round().max(0.0) as usize).min(n - 1);
            (mid, mid)
        } else {
            (lo, hi)
        };
        for slot in &mut pin[lo..=hi] {
            if slot.is_none() {
                *slot = Some(level);
            }
        }
    }
    // The steepest each step may climb: the class's, or what two pins
    // need between them where they stand further apart in height.
    let base = grade * step;
    let mut limit = vec![base; n - 1];
    let mut last: Option<(usize, f64)> = None;
    for (k, held) in pin.iter().enumerate() {
        let Some(v) = *held else { continue };
        if let Some((p, w)) = last {
            let need = (v - w).abs() / (k - p) as f64;
            if need > base {
                for slot in &mut limit[p..k] {
                    *slot = need * (1.0 + 1e-9) + 1e-9;
                }
            }
        }
        last = Some((k, v));
    }
    // The water's floor never asks more than the grade of a held level: a
    // ford's bed lies under the floor its own river sets beside it, and a
    // floor that stood at full height on the node past the wetted edge
    // made the approach jump to it in one step. Capped at each pin's level
    // plus the climb the grade allows from it, the approach rises out of
    // the water at its grade.
    let mut cap = vec![f64::INFINITY; n];
    for k in 0..n {
        if let Some(v) = pin[k] {
            cap[k] = v;
        } else if k > 0 {
            cap[k] = cap[k - 1] + limit[k - 1];
        }
    }
    for k in (0..n - 1).rev() {
        cap[k] = cap[k].min(cap[k + 1] + limit[k]);
    }
    // After the vertical curves, whose rounding runs under the graded
    // line for a curve's length past a held run, the cap holds at the
    // pin's level a curve's length out before it climbs.
    let reach = (way.curve_m / step).ceil() as usize;
    let low_curved: Vec<f64> = (0..n)
        .map(|k| {
            let lo = k.saturating_sub(reach);
            let hi = (k + reach).min(n - 1);
            let held = cap[lo..=hi].iter().copied().fold(f64::INFINITY, f64::min);
            low[k].min(held)
        })
        .collect();
    for k in 0..n {
        low[k] = low[k].min(cap[k]);
    }
    // The target: the ground averaged along the road, so cut and fill
    // balance, inside what the pins allow.
    let target = box_average(&g, (way.balance_m * 0.5 / step).round() as usize);
    let mut hi = vec![f64::INFINITY; n];
    let mut lo = vec![f64::NEG_INFINITY; n];
    for k in 0..n {
        if let Some(v) = pin[k] {
            hi[k] = v;
            lo[k] = v;
        }
        if k > 0 {
            hi[k] = hi[k].min(hi[k - 1] + limit[k - 1]);
            lo[k] = lo[k].max(lo[k - 1] - limit[k - 1]);
        }
        if pin[k].is_none() {
            lo[k] = lo[k].max(low[k]);
        }
    }
    for k in (0..n - 1).rev() {
        hi[k] = hi[k].min(hi[k + 1] + limit[k]);
        lo[k] = lo[k].max(lo[k + 1] - limit[k]);
    }
    let within = |k: usize, v: f64| {
        pin[k].unwrap_or_else(|| {
            if lo[k] <= hi[k] {
                v.clamp(lo[k], hi[k])
            } else {
                f64::midpoint(lo[k], hi[k])
            }
        })
    };
    // The grade: forward then back, and back then forward, averaged.
    let graded = |start: Vec<f64>| -> Vec<f64> {
        let mut a = start.clone();
        forward(&mut a, &limit, &pin);
        backward(&mut a, &limit, &pin);
        let mut b = start;
        backward(&mut b, &limit, &pin);
        forward(&mut b, &limit, &pin);
        a.iter().zip(&b).map(|(a, b)| 0.5 * (a + b)).collect()
    };
    let mut h = graded((0..n).map(|k| within(k, target[k])).collect());
    // Held to its grade the profile cuts every crest and fills every
    // hollow; each round moves it by the mean of what it is off its target
    // round each node and holds it to the grade again, so a climb that
    // cannot follow the ground runs through it, half in cut and half in
    // fill.
    let half_balance = (way.balance_m * 0.5 / step).round() as usize;
    for _ in 0..BALANCE_ROUNDS {
        let off: Vec<f64> = target.iter().zip(&h).map(|(t, h)| t - h).collect();
        let spread = box_average(&off, half_balance);
        h = graded((0..n).map(|k| within(k, h[k] + spread[k])).collect());
    }
    // Steeper where the grade would cut or fill past the class's most; and
    // never under the water's floor.
    for k in 0..n {
        if pin[k].is_none() {
            h[k] = h[k]
                .clamp(g[k] - way.cut_most_m, g[k] + way.fill_most_m)
                .max(low[k]);
        }
    }
    // The vertical curves: the rise smoothed, then the levels rebuilt from
    // it and put back on their pins, a pin's mismatch spread over the
    // steps between it and the one before.
    let mut rise: Vec<f64> = (0..n - 1).map(|k| (h[k + 1] - h[k]) / step).collect();
    // Only within each free stretch: a rise mixed across a held run is
    // lost when the run is put back on its pin, and its height then spread
    // over the whole stretch.
    let half = (way.curve_m * 0.5 / step).round() as usize;
    let mut k = 0;
    while k < n - 1 {
        let fixed = |k: usize| pin[k].is_some() && pin[k + 1].is_some();
        if fixed(k) {
            k += 1;
            continue;
        }
        let mut end = k + 1;
        while end < n - 1 && !fixed(end) {
            end += 1;
        }
        // The held runs either side lend the window their own rise, so a
        // climb rounds into a level deck rather than meeting it in a kink.
        let lo = {
            let mut lo = k;
            while lo > 0 && k - lo < 2 * half && fixed(lo - 1) {
                lo -= 1;
            }
            lo
        };
        let hi = {
            let mut hi = end;
            while hi < n - 1 && hi - end < 2 * half && fixed(hi) {
                hi += 1;
            }
            hi
        };
        let mut stretch = rise[lo..hi].to_vec();
        for _ in 0..2 {
            stretch = box_average(&stretch, half);
        }
        rise[k..end].copy_from_slice(&stretch[k - lo..end - lo]);
        k = end;
    }
    let pinned: Vec<usize> = (0..n).filter(|&k| pin[k].is_some()).collect();
    let mut out = vec![0.0; n];
    if let (Some(&first), Some(&final_pin)) = (pinned.first(), pinned.last()) {
        out[first] = pin[first].unwrap_or(h[first]);
        for k in (0..first).rev() {
            out[k] = out[k + 1] - rise[k] * step;
        }
        for pair in pinned.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let (vp, vq) = (pin[p].unwrap_or(h[p]), pin[q].unwrap_or(h[q]));
            let climbed: f64 = rise[p..q].iter().sum::<f64>() * step;
            let spread = (vq - vp - climbed) / (q - p) as f64;
            out[p] = vp;
            for k in p..q {
                out[k + 1] = out[k] + rise[k] * step + spread;
            }
            out[q] = vq;
        }
        for k in final_pin..n - 1 {
            out[k + 1] = out[k] + rise[k] * step;
        }
    } else {
        // No pin: rebuilt from the first node and moved back onto the
        // grade's mean level.
        for k in 0..n - 1 {
            out[k + 1] = out[k] + rise[k] * step;
        }
        let shift = (h.iter().sum::<f64>() - out.iter().sum::<f64>()) / n as f64;
        for v in &mut out {
            *v += shift;
        }
    }
    for k in 0..n {
        if pin[k].is_none() {
            out[k] = out[k].max(low_curved[k]);
        }
    }
    // Each point's level and rise, from the grid.
    // A pinned run's own rise at its ends, so a level deck stays level up to
    // its last node and does not bulge towards the climb beside it.
    let slope_at = |k: usize| -> f64 {
        let held = |k: usize| pin[k].is_some();
        let run = held(k) && ((k > 0 && held(k - 1)) || (k + 1 < n && held(k + 1)));
        let ahead = k + 1 < n && (!run || held(k + 1));
        let behind = k > 0 && (!run || held(k - 1));
        match (behind, ahead) {
            (true, true) => (out[k + 1] - out[k - 1]) / (2.0 * step),
            (false, true) => (out[k + 1] - out[k]) / step,
            (true, false) => (out[k] - out[k - 1]) / step,
            (false, false) => 0.0,
        }
    };
    points
        .iter()
        .map(|p| {
            let u = (p.along_m / step).clamp(0.0, (n - 1) as f64);
            let k = (u.floor() as usize).min(n - 2);
            let t = u - k as f64;
            let level = out[k] + (out[k + 1] - out[k]) * t;
            let rise = slope_at(k) + (slope_at(k + 1) - slope_at(k)) * t;
            [level as f32, rise as f32]
        })
        .collect()
}

/// The side of a cell's road bucket, metres: a lattice cell in 16 by 16.
pub(crate) const BUCKET_M: f64 = 32.0;

/// Buckets along a cell's side.
const BUCKETS: usize = 16;

/// The last bucket's index, as a coordinate.
const LAST_BUCKET: f64 = 15.0;

const _: () = assert!(BUCKET_M * (LAST_BUCKET + 1.0) == LATTICE_M);

/// How far from its centreline a road of `way` can change a sample,
/// metres: its verge, its edge's wander and its banks' reach, and the
/// neighbours' blend past that (see [`RoadGrid::apply`]).
fn influence_m(way: &Way) -> f64 {
    let ground = way.half_m + way.verge_m + EDGE_WANDER_M + way.reach_m;
    ground + blend_m(ground)
}

/// Over how much more distance than the nearest segment's a road's other
/// segments still take a share of a point, metres: wider out on a bank,
/// where the inside of a bend reads several segments nearly as near.
fn blend_m(nearest: f64) -> f64 {
    1.0 + 0.2 * nearest
}

/// The furthest any class reaches.
fn influence_most_m() -> f64 {
    influence_m(&way(RoadClass::Service))
        .max(influence_m(&way(RoadClass::Track)))
        .max(influence_m(&way(RoadClass::Trail)))
}

/// A road segment as a cell's buckets hold it.
#[derive(Debug, Clone, Copy)]
struct Seg {
    a: [f64; 2],
    dir: [f64; 2],
    len: f64,
    along: [f64; 2],
    /// Metres along the road per metre along the chord.
    stretch: f64,
    profile: [[f32; 2]; 2],
    /// A bridge's gap along the road, `[from, to]` metres; empty where none
    /// is near.
    gap: [f64; 2],
    /// The nearest crossing's held stretch along the road, of any kind,
    /// `[from, to]` metres; empty where none is near.
    wet: [f64; 2],
    /// Whether its first and its second point are its road's end on a
    /// junction, past which its ground is squashed (see [`END_SQUASH`]).
    junction_end: [bool; 2],
    /// Where along its road its ends on junctions lie, metres: nought and
    /// its length where they do, an infinity where an end does not.
    mouths: [f64; 2],
    /// Its road's place in the order roads are laid over each other.
    group: u32,
    class: RoadClass,
    graded: bool,
}

/// The roads round one lattice cell, by bucket.
#[derive(Debug, Default)]
pub(crate) struct RoadGrid {
    origin: [f64; 2],
    segs: Vec<Seg>,
    /// Where each bucket's run of `list` starts; one more for the end.
    starts: Vec<u32>,
    list: Vec<u32>,
}

/// What a road makes of a sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Graded {
    /// The ground's height, with its gradient.
    pub height: D,
    /// How much of the point a carriageway or a trail's line has taken,
    /// `0..=1`.
    pub developed: f64,
    /// How much of it is a road's cleared way, its carriageway and verge.
    pub cleared: f64,
    /// The class whose carriageway takes the most of the point.
    pub class: Option<RoadClass>,
    /// How much the point is a crossing's ground, `0..=1` with its
    /// gradient: one along a road within a bank's reach of one of its
    /// crossings' held stretch, nought [`WET_FADE_M`] further. Only there
    /// may a road's ground reach into a river's channel (see
    /// [`channel_share`]).
    pub crossing: D,
}

/// Over how many metres along a road past a bank's reach of its crossing a
/// river's channel is closed to its ground again.
const WET_FADE_M: f64 = 30.0;

/// How much of a road's change to the ground a point keeps by a river's
/// channel, `0..=1` with its gradient: none within the channel's own
/// half-width `half`, all past its bank `bank` and [`CHANNEL_FADE_M`] more,
/// so a fill beside a river does not spill into its water away from a
/// crossing; all of it, the larger by value, where `crossing` (a crossing's
/// own ground) says so.
pub(crate) fn channel_share(channel: D, half: f64, bank: f64, crossing: D) -> D {
    let open = smooth_c(half, half + bank + CHANNEL_FADE_M, channel);
    if crossing.v > open.v { crossing } else { open }
}

/// Metres past a channel's bank over which a road's ground is let back in.
const CHANNEL_FADE_M: f64 = 8.0;

/// A noise in about `-1..1` with its gradient, `wavelength` metres long.
/// Value noise, smoothstepped between hashed lattice values: integer
/// hashing, adds and multiplies only, so the same bits on every platform,
/// and a few times cheaper than a gradient noise, which a sample by a road
/// pays for three times.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "lattice indices of a world coordinate over tens of metres, and 53 bits of a hash"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, its cell, its corners and the shares between, as value noise writes them"
)]
fn wander(x: f64, z: f64, wavelength: f64, salt: u64) -> D {
    let (u, v) = (x / wavelength, z / wavelength);
    let (i, j) = (u.floor(), v.floor());
    let (fu, fv) = (u - i, v - j);
    let (i, j) = (i as i64, j as i64);
    let at = |a: i64, b: i64| -> f64 {
        let mut k = salt
            ^ a.cast_unsigned().wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ b.cast_unsigned().wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
        k ^= k >> 29;
        k = k.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        k ^= k >> 32;
        (k >> 11) as f64 / (1_u64 << 52) as f64 - 1.0
    };
    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
    let (su, sv) = (fu * fu * (3.0 - 2.0 * fu), fv * fv * (3.0 - 2.0 * fv));
    let (du, dv) = (6.0 * fu * (1.0 - fu), 6.0 * fv * (1.0 - fv));
    let top = a + (b - a) * su;
    let bottom = c + (d - c) * su;
    D::new(
        top + (bottom - top) * sv,
        [
            ((b - a) + ((d - c) - (b - a)) * sv) * du / wavelength,
            (bottom - top) * dv / wavelength,
        ],
    )
}

/// Two independent [`wander`]s from one lattice, each in about `-1..1` with
/// its gradient: the high and the low bits of one hash per corner.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "lattice indices of a world coordinate over tens of metres, and 26 bits of a hash"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, its cell, its corners and the shares between, as value noise writes them"
)]
pub(crate) fn wander_pair(x: f64, z: f64, wavelength: f64, salt: u64) -> (D, D) {
    let (u, v) = (x / wavelength, z / wavelength);
    let (i, j) = (u.floor(), v.floor());
    let (fu, fv) = (u - i, v - j);
    let (i, j) = (i as i64, j as i64);
    let at = |a: i64, b: i64| -> (f64, f64) {
        let mut k = salt
            ^ a.cast_unsigned().wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ b.cast_unsigned().wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
        k ^= k >> 29;
        k = k.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        k ^= k >> 32;
        let scale = 1.0 / f64::from(1_u32 << 25);
        (
            (k >> 38) as f64 * scale - 1.0,
            ((k >> 6) & 0x03ff_ffff) as f64 * scale - 1.0,
        )
    };
    let corners = [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)];
    let (su, sv) = (fu * fu * (3.0 - 2.0 * fu), fv * fv * (3.0 - 2.0 * fv));
    let (du, dv) = (6.0 * fu * (1.0 - fu), 6.0 * fv * (1.0 - fv));
    let blend = |pick: fn((f64, f64)) -> f64| {
        let [a, b, c, d] = corners.map(pick);
        let top = a + (b - a) * su;
        let bottom = c + (d - c) * su;
        D::new(
            top + (bottom - top) * sv,
            [
                ((b - a) + ((d - c) - (b - a)) * sv) * du / wavelength,
                (bottom - top) * dv / wavelength,
            ],
        )
    };
    (blend(|c| c.0), blend(|c| c.1))
}

/// The smaller of `a` and `b`, rounded over `k`: the polynomial soft
/// minimum, the hard one where they are `k` or more apart or `k` is
/// nought.
fn smin(a: D, b: D, k: D) -> D {
    let diff = a - b;
    let low = if diff.v <= 0.0 { a } else { b };
    if k.v <= 0.0 {
        return low;
    }
    let gap = if diff.v < 0.0 { -diff } else { diff };
    if gap.v >= k.v {
        return low;
    }
    let h = 1.0 - gap / k;
    low - h * h * k * 0.25
}

/// The larger, rounded the same way.
fn smax(a: D, b: D, k: D) -> D {
    -smin(-a, -b, k)
}

const SALT_EDGE: u64 = 0x6772_6164_6501;
const SALT_BANK: u64 = 0x6772_6164_6502;
const SALT_FINE: u64 = 0x6772_6164_6503;

impl RoadGrid {
    /// The grid of lattice cell `key` over `networks` (in key order), or
    /// `None` where no road's ground reaches it.
    pub(crate) fn new(key: [i32; 2], networks: &[&Arc<Network>]) -> Option<Self> {
        let origin = [f64::from(key[0]) * LATTICE_M, f64::from(key[1]) * LATTICE_M];
        let reach = influence_most_m();
        let around = [
            origin[0] - reach,
            origin[1] - reach,
            origin[0] + LATTICE_M + reach,
            origin[1] + LATTICE_M + reach,
        ];
        let mut pairs = Vec::new();
        let mut chosen: Vec<(usize, u32, u32)> = Vec::new();
        for (n, network) in networks.iter().enumerate() {
            network.road_index.within(around, &mut pairs);
            for &(r, k) in &pairs {
                if !network.roads[r as usize].is_settlements() {
                    chosen.push((n, r, k));
                }
            }
        }
        if chosen.is_empty() {
            return None;
        }
        Self::from_segments(origin, &chosen, |n, r| &networks[n].roads[r as usize])
    }

    /// The grid at `origin` of the segments `chosen` (`(network, road,
    /// segment)`), each road read through `road`.
    #[allow(
        clippy::too_many_lines,
        reason = "the segments in order, then their buckets"
    )]
    fn from_segments<'a>(
        origin: [f64; 2],
        chosen: &[(usize, u32, u32)],
        road: impl Fn(usize, u32) -> &'a Road,
    ) -> Option<Self> {
        // Arms first, the roads they end on last: within a network a later
        // road over an earlier one, networks by key.
        let mut order: Vec<(usize, std::cmp::Reverse<u32>, u32)> = chosen
            .iter()
            .map(|&(n, r, k)| (n, std::cmp::Reverse(r), k))
            .collect();
        order.sort_unstable();
        order.dedup();
        let mut segs: Vec<Seg> = Vec::with_capacity(order.len());
        let mut reaches: Vec<f64> = Vec::with_capacity(order.len());
        let mut group = 0_u32;
        let mut last: Option<(usize, u32)> = None;
        for &(n, std::cmp::Reverse(r), k) in &order {
            let line = road(n, r);
            let w = way(line.class);
            let reach = influence_m(&w);
            let (p, q) = (line.points[k as usize], line.points[k as usize + 1]);
            // Only a segment whose ground can reach the cell.
            if p.x.max(q.x) + reach < origin[0]
                || p.x.min(q.x) - reach > origin[0] + LATTICE_M
                || p.z.max(q.z) + reach < origin[1]
                || p.z.min(q.z) - reach > origin[1] + LATTICE_M
            {
                continue;
            }
            if let Some(held) = last
                && held != (n, r)
            {
                group += 1;
            }
            last = Some((n, r));
            let (dx, dz) = (q.x - p.x, q.z - p.z);
            let len = (dx * dx + dz * dz).sqrt();
            let dir = if len > 0.0 {
                [dx / len, dz / len]
            } else {
                [1.0, 0.0]
            };
            let profile = if line.graded() {
                [line.profile[k as usize], line.profile[k as usize + 1]]
            } else {
                [[p.ground_m, 0.0], [q.ground_m, 0.0]]
            };
            // The nearest bridge's gap within reach of the segment, and the
            // nearest crossing's held stretch of any kind.
            let mut gap = [f64::INFINITY, f64::NEG_INFINITY];
            let mut wet = [f64::INFINITY, f64::NEG_INFINITY];
            let (mut best, mut best_wet) = (f64::INFINITY, f64::INFINITY);
            for c in &line.crossings {
                let [from, to] = held(&w, c);
                let off = (from - q.along_m).max(p.along_m - to).max(0.0);
                if off <= reach + w.reach_m + WET_FADE_M && off < best_wet {
                    best_wet = off;
                    wet = [from, to];
                }
                if c.kind != CrossingKind::Bridge {
                    continue;
                }
                let [from, to] = c.abutments_m;
                let off = (from - q.along_m).max(p.along_m - to).max(0.0);
                if off <= reach && off < best {
                    best = off;
                    gap = [from, to];
                }
            }
            segs.push(Seg {
                a: [p.x, p.z],
                dir,
                len,
                along: [p.along_m, q.along_m],
                stretch: if len > 0.0 {
                    (q.along_m - p.along_m) / len
                } else {
                    1.0
                },
                profile,
                gap,
                wet,
                mouths: [
                    if matches!(line.ends[0], RoadEnd::Junction(_)) {
                        0.0
                    } else {
                        f64::NEG_INFINITY
                    },
                    if matches!(line.ends[1], RoadEnd::Junction(_)) {
                        line.length_m()
                    } else {
                        f64::INFINITY
                    },
                ],
                junction_end: [
                    k == 0 && matches!(line.ends[0], RoadEnd::Junction(_)),
                    k as usize + 2 == line.points.len()
                        && matches!(line.ends[1], RoadEnd::Junction(_)),
                ],
                group,
                class: line.class,
                graded: line.graded(),
            });
            reaches.push(reach);
        }
        if segs.is_empty() {
            return None;
        }
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); BUCKETS * BUCKETS];
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the cell's buckets first"
        )]
        let cell = |v: f64, o: f64| ((v - o) / BUCKET_M).floor().clamp(0.0, LAST_BUCKET) as usize;
        for (s, seg) in segs.iter().enumerate() {
            let b = [
                seg.a[0] + seg.dir[0] * seg.len,
                seg.a[1] + seg.dir[1] * seg.len,
            ];
            let r = reaches[s];
            let (x0, x1) = (seg.a[0].min(b[0]) - r, seg.a[0].max(b[0]) + r);
            let (z0, z1) = (seg.a[1].min(b[1]) - r, seg.a[1].max(b[1]) + r);
            if x1 < origin[0]
                || x0 > origin[0] + LATTICE_M
                || z1 < origin[1]
                || z0 > origin[1] + LATTICE_M
            {
                continue;
            }
            let s = u32::try_from(s).expect("fewer than 4 billion segments in a cell");
            // A bucket whose centre stands within reach and half its
            // diagonal of the segment itself, not of its box: a diagonal
            // segment's box holds buckets far past its reach.
            let half_diagonal = BUCKET_M * std::f64::consts::FRAC_1_SQRT_2;
            let r2 = (r + half_diagonal) * (r + half_diagonal);
            for j in cell(z0, origin[1])..=cell(z1, origin[1]) {
                for i in cell(x0, origin[0])..=cell(x1, origin[0]) {
                    #[allow(clippy::cast_precision_loss, reason = "a bucket index under sixteen")]
                    let centre = [
                        origin[0] + (i as f64 + 0.5) * BUCKET_M,
                        origin[1] + (j as f64 + 0.5) * BUCKET_M,
                    ];
                    let (px, pz) = (centre[0] - seg.a[0], centre[1] - seg.a[1]);
                    let t = (px * seg.dir[0] + pz * seg.dir[1]).clamp(0.0, seg.len);
                    let (ex, ez) = (px - seg.dir[0] * t, pz - seg.dir[1] * t);
                    if ex * ex + ez * ez <= r2 {
                        buckets[j * BUCKETS + i].push(s);
                    }
                }
            }
        }
        let mut starts = Vec::with_capacity(BUCKETS * BUCKETS + 1);
        let mut entries = Vec::new();
        for bucket in &buckets {
            starts.push(u32::try_from(entries.len()).expect("a cell's lists fit a u32"));
            entries.extend_from_slice(bucket);
        }
        starts.push(u32::try_from(entries.len()).expect("a cell's lists fit a u32"));
        Some(Self {
            origin,
            segs,
            starts,
            list: entries,
        })
    }

    /// The grid of a cell at `origin` over `roads` alone, every segment of
    /// each: for tests.
    #[cfg(test)]
    pub(crate) fn of_roads(origin: [f64; 2], roads: &[Road]) -> Option<Self> {
        let chosen: Vec<(usize, u32, u32)> = roads
            .iter()
            .enumerate()
            .flat_map(|(r, road)| {
                let r = u32::try_from(r).expect("a test's roads");
                (0..road.points.len().saturating_sub(1))
                    .map(move |k| (0, r, u32::try_from(k).expect("a test's points")))
            })
            .collect();
        Self::from_segments(origin, &chosen, |_, r| &roads[r as usize])
    }

    /// What the roads make of ground `height` (with its gradient) at `(x,
    /// z)`, a point of this grid's cell; `None` where none reaches it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the cell's buckets first"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, its bucket and a run's ends"
    )]
    pub(crate) fn shape(&self, x: f64, z: f64, height: D) -> Option<Graded> {
        let bucket = |v: f64, o: f64| ((v - o) / BUCKET_M).floor().clamp(0.0, LAST_BUCKET) as usize;
        let b = bucket(z, self.origin[1]) * BUCKETS + bucket(x, self.origin[0]);
        let list = &self.list[self.starts[b] as usize..self.starts[b + 1] as usize];
        if list.is_empty() {
            return None;
        }
        let mut out = Graded {
            height,
            developed: 0.0,
            cleared: 0.0,
            class: None,
            crossing: D::c(0.0),
        };
        let mut touched = false;
        let mut i = 0;
        while i < list.len() {
            let group = self.segs[list[i] as usize].group;
            let mut j = i + 1;
            while j < list.len() && self.segs[list[j] as usize].group == group {
                j += 1;
            }
            touched |= self.apply(&list[i..j], x, z, &mut out);
            i = j;
        }
        touched.then_some(out)
    }

    /// One road's segments `run` over what the roads after it left at
    /// `(x, z)`; whether it reached the point.
    ///
    /// The road's reading is a blend of its segments': each within
    /// [`blend_m`] of the nearest takes a share falling to nothing there, so
    /// on the inside of a bend, where two segments' nearest points stand
    /// apart along the road, the level and the distance pass from one to
    /// the other without a step.
    #[allow(
        clippy::many_single_char_names,
        reason = "distance, along, level and gap, as the section names them"
    )]
    #[allow(
        clippy::too_many_lines,
        reason = "the nearest segment, the blend, then what the road makes of the point"
    )]
    fn apply(&self, run: &[u32], x: f64, z: f64, out: &mut Graded) -> bool {
        let first = &self.segs[run[0] as usize];
        let w = way(first.class);
        let ground = w.half_m + w.verge_m + EDGE_WANDER_M + w.reach_m;
        let reach = if first.graded {
            ground
        } else {
            w.half_m + w.verge_m + EDGE_WANDER_M
        };
        // The nearest segment, by its distance's square.
        let square = |seg: &Seg| {
            let (px, pz) = (x - seg.a[0], z - seg.a[1]);
            let t = (px * seg.dir[0] + pz * seg.dir[1]).clamp(0.0, seg.len);
            let (ex, ez) = (px - seg.dir[0] * t, pz - seg.dir[1] * t);
            ex * ex + ez * ez
        };
        let mut least = (f64::INFINITY, 0_u32);
        for &s in run {
            let d2 = square(&self.segs[s as usize]);
            if d2 < least.0 {
                least = (d2, s);
            }
        }
        if least.0 > reach * reach {
            return false;
        }
        let read = |seg: &Seg| -> (D, D, D, D) {
            let (px, pz) = (x - seg.a[0], z - seg.a[1]);
            let t = px * seg.dir[0] + pz * seg.dir[1];
            let (t, moving) = if t <= 0.0 {
                (0.0, 0.0)
            } else if t >= seg.len {
                (seg.len, 0.0)
            } else {
                (t, 1.0)
            };
            let (ex, ez) = (px - seg.dir[0] * t, pz - seg.dir[1] * t);
            // Past an end on a junction the distance along the road counts
            // `END_SQUASH` times over: the arm's carriageway stops inside
            // the through road's, which lays its own over it, and does not
            // round out past it onto the far side.
            let beyond = px * seg.dir[0] + pz * seg.dir[1] - t;
            let squashed =
                (beyond < 0.0 && seg.junction_end[0]) || (beyond > 0.0 && seg.junction_end[1]);
            let d = if squashed {
                let normal = [-seg.dir[1], seg.dir[0]];
                let lateral = px * normal[0] + pz * normal[1];
                let along = beyond * END_SQUASH;
                let dist = (lateral * lateral + along * along).sqrt();
                if dist > 1e-9 {
                    D::new(
                        dist,
                        [
                            (lateral * normal[0] + along * END_SQUASH * seg.dir[0]) / dist,
                            (lateral * normal[1] + along * END_SQUASH * seg.dir[1]) / dist,
                        ],
                    )
                } else {
                    D::c(0.0)
                }
            } else {
                let dist = (ex * ex + ez * ez).sqrt();
                if dist > 1e-9 {
                    D::new(dist, [ex / dist, ez / dist])
                } else {
                    D::c(0.0)
                }
            };
            let k = seg.stretch * moving;
            let s = D::new(
                seg.along[0] + t * seg.stretch,
                [seg.dir[0] * k, seg.dir[1] * k],
            );
            let (level, rise) = hermite(seg.along, seg.profile, s.v);
            let p = D::new(level, [s.g[0] * rise, s.g[1] * rise]);
            let a = if s.v > seg.gap[0] && s.v < seg.gap[1] {
                let (into, out) = (s - seg.gap[0], seg.gap[1] - s);
                if into.v <= out.v { into } else { out }
            } else {
                D::c(0.0)
            };
            // How far outside the nearest crossing's held stretch, past a
            // bank's reach of it.
            let c = if seg.wet[0] <= seg.wet[1] {
                let (before, after) = (seg.wet[0] - s, s - seg.wet[1]);
                let outside = if before.v >= after.v { before } else { after };
                1.0 - smooth_c(w.reach_m, w.reach_m + WET_FADE_M, outside)
            } else {
                D::c(0.0)
            };
            (d, p, a, c)
        };
        // Those within the blend of the nearest, read whole.
        let nearest = least.0.sqrt();
        let (d_near, p_near, a_near, c_near) = read(&self.segs[least.1 as usize]);
        let spread = d_near.scale(0.2) + 1.0;
        // A little wider by value, so no segment the weights reach is
        // passed over for a rounding.
        let wide = blend_m(nearest) + 1e-6;
        let (mut sum, mut d, mut p, mut a, mut c) = (D::c(1.0), d_near, p_near, a_near, c_near);
        for &s in run {
            if s == least.1 {
                continue;
            }
            let seg = &self.segs[s as usize];
            let d2 = square(seg);
            if d2 >= (nearest + wide) * (nearest + wide) {
                continue;
            }
            let (dj, pj, aj, cj) = read(seg);
            if dj.v - d_near.v >= spread.v {
                continue;
            }
            let r = (dj - d_near) / spread;
            let one = 1.0 - r;
            let weight = one * one;
            sum = sum + weight;
            d = d + weight * dj;
            p = p + weight * pj;
            a = a + weight * aj;
            c = c + weight * cj;
        }
        if sum.v <= 0.0 {
            return false;
        }
        let (d, p, a, c) = (d / sum, p / sum, a / sum, c / sum);
        if c.v > out.crossing.v {
            out.crossing = c;
        }
        let edge = d.v + GAP_EDGE * a.v;
        // An arm's carriageway widens over its verge as it meets the road
        // it ends on, so the two meet in a rounded mouth and not at a
        // right angle.
        let flare = {
            let near = &self.segs[least.1 as usize];
            let t = ((x - near.a[0]) * near.dir[0] + (z - near.a[1]) * near.dir[1])
                .clamp(0.0, near.len);
            let s = near.along[0] + t * near.stretch;
            let from_end = (s - near.mouths[0]).min(near.mouths[1] - s);
            let open = (1.0 - from_end / MOUTH_M).clamp(0.0, 1.0);
            w.verge_m * open * open
        };
        let edge = edge - flare;
        // The carriageway's edge breaks up over a metre or so: its noise is
        // read only there.
        let fine = if (edge - w.half_m).abs() < 2.0 || !first.graded {
            0.7 * wander(x, z, 9.0, SALT_FINE).v + 0.3 * wander(x, z, 3.1, SALT_FINE ^ 1).v
        } else {
            0.0
        };
        let (developed, cleared) = if first.graded {
            (
                1.0 - smoothstep(w.half_m - 0.4, w.half_m + 0.4, edge + 0.8 * fine),
                1.0 - smoothstep(w.half_m + w.verge_m - 0.3, w.half_m + w.verge_m + 0.8, edge),
            )
        } else {
            (
                TRAIL_WORN * (1.0 - smoothstep(0.3 * w.half_m, w.half_m, edge + 0.4 * fine)),
                1.0 - smoothstep(w.half_m, w.half_m + w.verge_m, edge),
            )
        };
        if developed > out.developed {
            out.developed = developed;
            out.class = Some(first.class);
        }
        out.cleared = out.cleared.max(cleared);
        if first.graded {
            out.height = section(&w, x, z, d, p, a, out.height);
        }
        true
    }
}

/// The ground across a graded road of `way` at `(x, z)`: `d` metres from its
/// centreline, `level` its profile there, `gap` metres into a bridge's gap
/// (nought off one), over ground `height`.
#[allow(
    clippy::many_single_char_names,
    reason = "distance, level and the bank's own letters"
)]
fn section(way: &Way, x: f64, z: f64, d: D, level: D, gap: D, height: D) -> D {
    let edge = wander(x, z, 47.0, SALT_EDGE);
    // The verge's edge, where a bank starts.
    let e = edge * EDGE_WANDER_M + (way.half_m + way.verge_m);
    if d.v <= e.v && gap.v <= 0.0 {
        // On the carriageway and its verge: the profile, crowned.
        let crown = if d.v < way.half_m {
            d * d * (way.crossfall / (2.0 * way.half_m))
        } else {
            (d - way.half_m * 0.5) * way.crossfall
        };
        return level - crown;
    }
    let bank = wander(x, z, 67.0, SALT_BANK);
    // The carriageway's crown, its tangent carried across the verge.
    let crown = if d.v < way.half_m {
        d * d * (way.crossfall / (2.0 * way.half_m))
    } else {
        (d - way.half_m * 0.5) * way.crossfall
    };
    let road = level - crown;
    let off = height - road;
    // How far past the verge, and into a bridge's gap past its abutment,
    // where the ground falls away as the abutment's face: steeply, so the
    // valley under the deck is the river's.
    let u = if gap.v > 0.0 {
        let lateral = (d - e).max(D::c(0.0));
        let into = gap * ABUTMENT_FACE;
        (lateral * lateral + into * into).sqrt()
    } else {
        d - e
    };
    // The cut: soil at its foot, steepening to the rock's above a rise
    // that wanders, as the soil's slope does.
    let soil = (bank * 0.25 + 1.0) * way.soil;
    let rise = (bank * -0.4 + 1.0) * way.soil_rise_m;
    let turn = rise / soil;
    let cut = soil * soft(u - FOOT_M, D::c(FOOT_M))
        + (way.rock - soil) * soft(u - turn - TURN_M, D::c(TURN_M));
    // The fill: grassed, steepening once it has dropped its first metres.
    let fill = soft(u - FOOT_M, D::c(FOOT_M)) * way.fill
        + soft(u - way.fill_drop_m / way.fill - TURN_M, D::c(TURN_M)) * (way.fill_steep - way.fill);
    // The shoulder's and the toe's rounding grows with the bank and with
    // the distance out, so a shallow cut's top edge is no crease either.
    let out = soft(u - FOOT_M, D::c(FOOT_M)) * 0.25;
    // Held under its most softly, so the rounding's own gradient has no
    // kink where it reaches it.
    let most = |k: D| k - soft(k - SHOULDER_MOST_M, D::c(1.0));
    let y = smin(off, cut, most(cut * 0.6 + out));
    let y = smax(y, -fill, most(fill * 0.6 + out));
    // Faded into the ground over the last of the reach; where the bank met
    // the ground the ground is left to the bit.
    let keep = 1.0 - smooth_c(way.reach_m - FADE_M, way.reach_m, u);
    if keep.v <= 0.0 {
        return height;
    }
    height + (y - off) * keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, along: f64, ground: f64) -> RoadPoint {
        #[allow(clippy::cast_possible_truncation, reason = "a test's heights")]
        RoadPoint {
            x,
            z: 0.0,
            along_m: along,
            ground_m: ground as f32,
        }
    }

    /// A straight road along `x` over ground `ground(x)`, a point every
    /// 20 m.
    fn straight(class: RoadClass, length: f64, ground: impl Fn(f64) -> f64) -> Road {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a test's points"
        )]
        let n = (length / 20.0) as usize;
        #[allow(clippy::cast_precision_loss, reason = "a test's points")]
        let points = (0..=n)
            .map(|k| {
                let x = k as f64 * 20.0;
                point(x, x, ground(x))
            })
            .collect();
        Road {
            class,
            points,
            ends: [RoadEnd::Junction(0), RoadEnd::Junction(0)],
            crossings: Vec::new(),
            profile: Vec::new(),
        }
    }

    fn fitted(mut road: Road, pins: &[[f64; 3]]) -> Road {
        let w = way(road.class);
        road.profile = fit(
            &road.points,
            &w,
            crate::roads::spec(road.class).max_grade,
            pins,
            &[],
        );
        road
    }

    #[test]
    fn the_profile_holds_its_grade_over_a_climb_and_its_pins() {
        // A hill 30 m high over 400 m: 15 % on its flanks.
        let hill = |x: f64| {
            let u = ((x - 600.0) / 200.0).clamp(-1.0, 1.0);
            30.0 * (1.0 - u * u)
        };
        let road = fitted(
            straight(RoadClass::Track, 1_200.0, hill),
            &[[0.0, 0.0, 0.0], [1_200.0, 1_200.0, 0.0]],
        );
        let grade = crate::roads::spec(RoadClass::Track).max_grade;
        for k in 0..road.points.len() - 1 {
            let (a, b) = (road.points[k].along_m, road.points[k + 1].along_m);
            for i in 0..=20 {
                let s = a + (b - a) * f64::from(i) / 20.0;
                let (_, rise) = road.level_at(s).expect("a profile");
                assert!(rise.abs() <= grade + 0.004, "rise {rise} at {s}");
            }
        }
        assert!(road.level_at(0.0).expect("a profile").0.abs() < 1e-4);
        assert!(road.level_at(1_200.0).expect("a profile").0.abs() < 1e-4);
        // Cut through the crest, filled at its feet: the top is cut by
        // metres, not left standing, and the fill at the feet is about the
        // cut at the top.
        let top = road.level_at(600.0).expect("a profile").0;
        assert!(top < 30.0 - 1.0 && top > 30.0 - 12.0, "{top}");
        let (mut cut, mut fill) = (0.0, 0.0);
        for p in &road.points {
            let off = road.level_at(p.along_m).expect("a profile").0 - f64::from(p.ground_m);
            if off > 0.0 {
                fill += off;
            } else {
                cut -= off;
            }
        }
        assert!(
            fill > 0.4 * cut && cut > 0.4 * fill,
            "cut {cut} against fill {fill}"
        );
    }

    #[test]
    fn a_profile_is_level_on_a_bridges_deck_and_rounds_into_a_climb() {
        let ramp = |x: f64| 0.05 * x;
        let mut road = straight(RoadClass::Service, 1_000.0, ramp);
        road.crossings.push(Crossing {
            kind: CrossingKind::Bridge,
            at: [500.0, 0.0],
            along_m: 500.0,
            bearing: [1.0, 0.0],
            span_m: 30.0,
            deck_m: 30.0,
            surface_m: 20.0,
            banks_m: [30.0, 30.0],
            width_m: 8.0,
            depth_m: 1.0,
            river: 0,
            abutments_m: [485.0, 515.0],
        });
        let pins = [[479.0, 521.0, 30.0]];
        let road = fitted(road, &pins);
        for s in [480.0, 490.0, 500.0, 510.0, 520.0] {
            let (level, _) = road.level_at(s).expect("a profile");
            assert!((level - 30.0).abs() < 0.05, "{level} at {s}");
        }
        // The rise turns gently: no step in it from one metre to the next.
        let mut last = road.level_at(0.0).expect("a profile").1;
        for i in 1..1_000 {
            let (_, rise) = road.level_at(f64::from(i)).expect("a profile");
            assert!(
                (rise - last).abs() < 0.01,
                "a kink at {i}: {last} to {rise}"
            );
            last = rise;
        }
    }

    /// The ground a grid makes over flat ground at `height`, and its
    /// gradient.
    fn ground(grid: &RoadGrid, x: f64, z: f64, natural: impl Fn(f64, f64) -> D) -> D {
        grid.shape(x, z, natural(x, z))
            .map_or_else(|| natural(x, z), |g| g.height)
    }

    #[test]
    fn the_ground_across_a_road_is_continuous_with_its_gradient() {
        // A sidehill rising 0.3 to +z, a track along x climbing 5 % with it,
        // a hump on the road's line it cuts through.
        let hump = |x: f64| {
            let u = ((x - 250.0) / 60.0).clamp(-1.0, 1.0);
            6.0 * (1.0 - u * u) * (1.0 - u * u)
        };
        let hillside = |x: f64, z: f64| {
            let u = ((x - 250.0) / 60.0).clamp(-1.0, 1.0);
            let dh = if u.abs() < 1.0 {
                -24.0 * u * (1.0 - u * u) / 60.0
            } else {
                0.0
            };
            D::new(0.05 * x + 0.3 * z + hump(x), [0.05 + dh, 0.3])
        };
        let road = fitted(
            straight(RoadClass::Track, 480.0, |x| 0.05 * x + hump(x)),
            &[],
        );
        let roads = [road];
        let grid = RoadGrid::of_roads([0.0, -256.0], &roads).expect("a grid");
        // Across the road every 2 cm, at several places along it.
        for x in [100.0, 213.7, 250.0] {
            let mut last = ground(&grid, x, -60.0, hillside);
            let mut z = -60.0;
            while z < 60.0 {
                z += 0.02;
                let here = ground(&grid, x, z, hillside);
                let step = here.v - last.v;
                assert!(step.abs() < 0.06, "a step of {step} at ({x}, {z})");
                let measured = step / 0.02;
                let carried = f64::midpoint(here.g[1], last.g[1]);
                assert!(
                    (measured - carried).abs() < 0.05 + 0.05 * carried.abs(),
                    "gradient {carried} against {measured} at ({x}, {z})"
                );
                last = here;
            }
        }
        // On the carriageway the ground is the profile, crowned.
        let (level, _) = roads[0].level_at(250.0).expect("a profile");
        let on = ground(&grid, 250.0, 0.0, hillside);
        assert!((on.v - level).abs() < 1e-9);
        // Far off the road, the ground is left to the bit.
        let far = ground(&grid, 250.0, 100.0, hillside);
        assert_eq!(far.v.to_bits(), hillside(250.0, 100.0).v.to_bits());
    }

    /// What [`RoadGrid::shape`] costs alone, by a curving service road and
    /// a track ending on it: release, by hand.
    #[test]
    #[ignore = "a timing: release, by hand"]
    fn probe_shape_cost() {
        let mut service = straight(RoadClass::Service, 600.0, |x| 0.04 * x);
        for p in &mut service.points {
            p.z = 30.0 * (p.x / 90.0).sin();
        }
        let mut track = straight(RoadClass::Track, 300.0, |_| 10.0);
        for p in &mut track.points {
            (p.x, p.z) = (300.0 + 0.3 * p.along_m, p.along_m * 0.95);
        }
        let roads = [fitted(service, &[]), fitted(track, &[])];
        let grid = RoadGrid::of_roads([0.0, -256.0], &roads).expect("a grid");
        let natural = |x: f64, z: f64| D::new(0.1 * z + 0.02 * x, [0.02, 0.1]);
        let mut points = Vec::new();
        for j in 0..200 {
            for i in 0..200 {
                points.push((50.0 + f64::from(i) * 2.0, -100.0 + f64::from(j)));
            }
        }
        let mut reached = 0;
        for &(x, z) in &points {
            reached += usize::from(grid.shape(x, z, natural(x, z)).is_some());
        }
        let started = strata_core::platform::time::Instant::now();
        for _ in 0..10 {
            for &(x, z) in &points {
                std::hint::black_box(grid.shape(x, z, natural(x, z)));
            }
        }
        #[allow(clippy::cast_precision_loss, reason = "a timing's counts")]
        let ns = started.elapsed().as_secs_f64() * 1e9 / (10 * points.len()) as f64;
        println!(
            "shape: {ns:.0} ns a point, {reached} of {} reached",
            points.len()
        );
    }

    #[test]
    fn a_bridges_gap_is_left_to_the_river_and_a_trail_moves_no_ground() {
        let valley = |x: f64, _z: f64| {
            let u = (x - 500.0) / 60.0;
            D::new(-10.0 * (1.0 - u * u).max(0.0), [0.0, 0.0])
        };
        let mut road = straight(RoadClass::Service, 1_000.0, |x| valley(x, 0.0).v);
        road.crossings.push(Crossing {
            kind: CrossingKind::Bridge,
            at: [500.0, 0.0],
            along_m: 500.0,
            bearing: [1.0, 0.0],
            span_m: 110.0,
            deck_m: 0.0,
            surface_m: -9.0,
            banks_m: [0.0, 0.0],
            width_m: 4.0,
            depth_m: 1.0,
            river: 0,
            abutments_m: [445.0, 555.0],
        });
        let road = fitted(road, &[[439.0, 561.0, 0.0]]);
        let trail = straight(RoadClass::Trail, 1_000.0, |_| 0.0);
        let mut trail = trail;
        trail.points.iter_mut().for_each(|p| p.z = 100.0);
        let roads = [road, trail];
        let grid = RoadGrid::of_roads([256.0, -256.0], &roads).expect("a grid");
        // Mid-gap the valley is untouched.
        for x in [480.0, 500.0, 520.0] {
            let here = ground(&grid, x, 0.0, valley);
            assert_eq!(here.v.to_bits(), valley(x, 0.0).v.to_bits(), "at {x}");
        }
        // On the abutments' shelves the road stands at the deck.
        for x in [440.0, 560.0] {
            assert!(ground(&grid, x, 0.0, valley).v.abs() < 0.05, "at {x}");
        }
        // The trail clears its line and leaves its height.
        let natural = |_: f64, _: f64| D::new(3.0, [0.0, 0.0]);
        let on = grid
            .shape(300.0, 100.0, natural(300.0, 100.0))
            .expect("on the trail");
        assert_eq!(on.height.v.to_bits(), 3.0_f64.to_bits());
        assert!(on.developed > 0.9 && on.cleared > 0.9);
    }
}
