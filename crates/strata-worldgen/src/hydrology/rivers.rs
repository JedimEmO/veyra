//! From routed nodes to rivers: reaches, their order, their curves, their
//! surfaces and falls, and the lakes' records.

use strata_core::{Seed, mix64};

use super::index::Shore;
use super::mass::{Domain, OutletKind};
use super::route::{NONE, Root, Routed, site_distance};
use super::{
    Fall, LATTICE_M, Lake, River, RiverEnd, RiverParams, RiverVertex, local, metres, single,
};
use crate::Site;
use crate::noise::{Fractal, Shape, smoothstep};

/// What a bake builds.
pub(crate) struct Built {
    pub rivers: Vec<River>,
    pub lakes: Vec<Lake>,
    pub falls: Vec<Fall>,
}

/// A point on a reach while it is smoothed: where, how far along the node
/// chain (so the surface is read from the chain after smoothing, falls and
/// all), and the discharge.
#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    z: f64,
    t: f64,
    q: f64,
}

impl Point {
    fn lerp(self, other: Self, f: f64) -> Self {
        Self {
            x: self.x + (other.x - self.x) * f,
            z: self.z + (other.z - self.z) * f,
            t: self.t + (other.t - self.t) * f,
            q: self.q + (other.q - self.q) * f,
        }
    }
}

/// Builds the rivers, lakes and falls of a routed domain.
#[allow(
    clippy::too_many_lines,
    reason = "orders, reaches, curves and records, in order"
)]
pub(crate) fn build(
    domain: &Domain,
    routed: &Routed,
    params: &RiverParams,
    seed: Seed,
    sites: &[Site],
) -> Built {
    let n = domain.nodes.len();
    let nodes = &domain.nodes;
    let q = &routed.discharge;
    // River nodes: off the lakes, with enough water, and each lake's outlet
    // when what leaves it is a river.
    let is_river: Vec<bool> = (0..n)
        .map(|k| {
            !routed.blocked[k]
                && q[k] >= params.river_discharge_m3s
                && (routed.lake[k] == NONE
                    || routed.lakes[routed.lake[k] as usize].outlet == local(k))
        })
        .collect();

    // Strahler orders, upstream first; a lake passes on the orders of the
    // rivers that enter it to the river that leaves it.
    let mut children = vec![0_u32; n];
    let mut top = vec![(0_u8, 0_u32); n];
    let mut lake_top = vec![(0_u8, 0_u32); routed.lakes.len()];
    let mut order = vec![0_u8; n];
    let bump = |held: &mut (u8, u32), o: u8| {
        if o > held.0 {
            *held = (o, 1);
        } else if o == held.0 {
            held.1 += 1;
        }
    };
    let strahler = |(max, count): (u8, u32)| match (max, count) {
        (0, _) => 1,
        (m, c) if c >= 2 => m.saturating_add(1),
        (m, _) => m,
    };
    for &k in routed.order.iter().rev() {
        let ki = k as usize;
        if !is_river[ki] {
            continue;
        }
        let lake = routed.lake[ki];
        order[ki] = if lake == NONE {
            strahler(top[ki])
        } else {
            strahler(lake_top[lake as usize])
        };
        let r = routed.receiver[ki];
        if r == NONE {
            continue;
        }
        let ri = r as usize;
        if routed.lake[ri] != NONE {
            bump(&mut lake_top[routed.lake[ri] as usize], order[ki]);
        } else if is_river[ri] {
            children[ri] += 1;
            bump(&mut top[ri], order[ki]);
        }
    }

    let freedom = |x: f64, z: f64| {
        if sites.is_empty() {
            1.0
        } else {
            smoothstep(
                params.site_margin_m,
                params.site_margin_m + params.site_fade_m,
                site_distance(sites, x, z),
            )
        }
    };
    let jitter_seed = seed.derive("rivers/jitter");
    let position = |node: [i32; 2]| {
        let (x, z) = (metres(node[0]), metres(node[1]));
        let free = freedom(x, z);
        let [dx, dz] = jitter(jitter_seed, node, params.jitter * LATTICE_M);
        [x + dx * free, z + dz * free]
    };

    // Reaches start at sources and confluences, in node order.
    let heads: Vec<u32> = (0..n)
        .filter(|&k| is_river[k] && children[k] != 1)
        .map(local)
        .collect();
    let mut reach_of = vec![NONE; n];
    for (id, &h) in heads.iter().enumerate() {
        reach_of[h as usize] = local(id);
    }
    let meander = Fractal::new(
        seed.derive("rivers/meander"),
        Shape::Fbm,
        params.meander_wavelength_m,
        super::NOISE_OCTAVES,
        super::NOISE_GAIN,
        super::NOISE_LACUNARITY,
    );
    let mut rivers = Vec::with_capacity(heads.len());
    let mut falls = Vec::new();
    for &head in &heads {
        // The chain's vertices: where, the surface, the discharge.
        let mut chain: Vec<([f64; 2], f64, f64)> = Vec::new();
        // Each chain node's grain (its province's `along_grain`), which
        // holds the sway down (ticket 0005, step 3b): swayed across a
        // folded belt's grain, its valleys' walls took the belt's
        // elongation from 2.31 to 1.98, under its gate.
        let mut grains: Vec<f64> = Vec::new();
        let mut cur = head as usize;
        let end = loop {
            chain.push((position(nodes[cur]), routed.surface[cur], q[cur]));
            grains.push(domain.field[cur].grain);
            let r = routed.receiver[cur];
            let last_q = q[cur];
            if r == NONE {
                break match routed.root[cur] {
                    Some(Root::Out(outside, kind, outside_height)) => {
                        let (surface, end) = match kind {
                            OutletKind::Sea => (0.0, RiverEnd::Sea),
                            OutletKind::Edge => (routed.surface[cur], RiverEnd::Edge),
                        };
                        // A mouth ends where the coarse ground crosses sea
                        // level between the last land node and the sea
                        // node, not out on the sea node itself, which lies
                        // up to a node's spacing off the coast.
                        let land = domain.field[cur].height;
                        let along = if kind == OutletKind::Sea && land - outside_height > 0.0 {
                            (land / (land - outside_height)).clamp(0.25, 1.0)
                        } else {
                            1.0
                        };
                        let (from, to) = (position(nodes[cur]), position(outside));
                        let at = [
                            from[0] + (to[0] - from[0]) * along,
                            from[1] + (to[1] - from[1]) * along,
                        ];
                        chain.push((at, surface, last_q));
                        grains.push(domain.field[cur].grain);
                        end
                    }
                    _ => RiverEnd::Sink,
                };
            }
            let ri = r as usize;
            if routed.lake[ri] != NONE {
                chain.push((position(nodes[ri]), routed.surface[ri], last_q));
                grains.push(domain.field[ri].grain);
                break RiverEnd::Lake(routed.lake[ri]);
            }
            if is_river[ri] && children[ri] == 1 {
                cur = ri;
                continue;
            }
            chain.push((position(nodes[ri]), routed.surface[ri], last_q));
            grains.push(domain.field[ri].grain);
            break if reach_of[ri] == NONE {
                RiverEnd::Sink
            } else {
                RiverEnd::Joins(reach_of[ri])
            };
        };
        let id = local(rivers.len());
        let surfaces: Vec<f64> = chain.iter().map(|c| c.1).collect();
        // A node step steeper than the threshold falls, in as many equal
        // falls spread along it as keep each under the tallest; zero for a
        // step that slopes. Never a mouth's last step: it runs from the last
        // land node to a coast interpolated across the sea floor's own
        // slope, so its drop says how steep the sea bed is, not the land,
        // and the coarse coast is not the sample's. Read as falls, these
        // were a third of all falls, ringing every coast; the carve grades
        // the stretch to the sample's shore instead.
        let mouth_step = chain.len().saturating_sub(2);
        let steep: Vec<f64> = chain
            .windows(2)
            .enumerate()
            .map(|(k, pair)| {
                if end == RiverEnd::Sea && k == mouth_step {
                    return 0.0;
                }
                let (a, b) = (pair[0], pair[1]);
                let drop = a.1 - b.1;
                let run = ((b.0[0] - a.0[0]) * (b.0[0] - a.0[0])
                    + (b.0[1] - a.0[1]) * (b.0[1] - a.0[1]))
                    .sqrt();
                if drop >= params.fall_drop_m && drop >= params.fall_gradient * run {
                    (drop / params.fall_max_m).ceil().max(1.0)
                } else {
                    0.0
                }
            })
            .collect();
        // How many of step `k`'s falls lie before `f` along it: they stand
        // at the middles of equal parts.
        let passed = |k: usize, f: f64| (f * steep[k] + 0.5).floor().clamp(0.0, steep[k]);
        let mut points: Vec<Point> = chain
            .iter()
            .enumerate()
            .map(|(k, c)| Point {
                x: c.0[0],
                z: c.0[1],
                t: super::real(k),
                q: c.2,
            })
            .collect();
        if points.len() == 2 {
            let mid = points[0].lerp(points[1], 0.5);
            points.insert(1, mid);
        }
        for _ in 0..params.smoothing {
            points = chaikin(&points);
        }
        let grain = |t: f64| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "t runs from 0 to the chain's last index"
            )]
            let k = (t.floor().max(0.0) as usize).min(grains.len() - 1);
            let next = (k + 1).min(grains.len() - 1);
            let f = (t - super::real(k)).clamp(0.0, 1.0);
            grains[k] + (grains[next] - grains[k]) * f
        };
        sway(&mut points, params, &meander, &freedom, &grain);
        let points = split_falls(&points, &steep);
        let surface_at = |t: f64| {
            let last = surfaces.len() - 1;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "t runs from 0 to the chain's last index"
            )]
            let k = (t.floor().max(0.0) as usize).min(last.saturating_sub(1));
            let f = t - super::real(k);
            if last == 0 {
                surfaces[0]
            } else if steep[k] > 0.0 {
                surfaces[k] - (surfaces[k] - surfaces[k + 1]) * passed(k, f) / steep[k]
            } else {
                surfaces[k] + (surfaces[k + 1] - surfaces[k]) * f
            }
        };
        let mut surface: Vec<f64> = points.iter().map(|p| surface_at(p.t)).collect();
        // Set from the mouth upstream: no vertex lower than the next.
        for i in (0..surface.len().saturating_sub(1)).rev() {
            surface[i] = surface[i].max(surface[i + 1]);
        }
        let mut vertices: Vec<RiverVertex> = points
            .iter()
            .zip(&surface)
            .map(|(p, &s)| RiverVertex {
                x: p.x,
                z: p.z,
                surface_m: single(s),
                discharge_m3s: single(p.q),
                width_m: single(params.width(p.q)),
                depth_m: single(params.depth(p.q)),
                cut_m: 0.0,
                fall: false,
                meander_m: 0.0,
                phase: 0.0,
            })
            .collect();
        for i in 0..points.len().saturating_sub(1) {
            // A pair of vertices across one or more of a step's falls is a
            // fall; corner cutting keeps every pair within one step.
            let at = |t: f64| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "t runs from 0 to the chain's last index"
                )]
                let k = (t.floor().max(0.0) as usize).min(steep.len().saturating_sub(1));
                (k, t - super::real(k))
            };
            let ((ka, fa), (kb, fb)) = (at(points[i].t), at(points[i + 1].t));
            let falls_here = if ka == kb {
                steep[ka] > 0.0 && passed(kb, fb) > passed(ka, fa)
            } else {
                (steep[ka] > 0.0 && passed(ka, 1.0) > passed(ka, fa))
                    || (steep[kb] > 0.0 && passed(kb, fb) > 0.0)
            };
            if !falls_here {
                continue;
            }
            vertices[i].fall = true;
            let (dx, dz) = (points[i + 1].x - points[i].x, points[i + 1].z - points[i].z);
            let length = (dx * dx + dz * dz).sqrt().max(f64::MIN_POSITIVE);
            falls.push(Fall {
                top: [points[i].x, surface[i], points[i].z],
                bottom: [points[i + 1].x, surface[i + 1], points[i + 1].z],
                width_m: vertices[i].width_m,
                direction: [single(dx / length), single(dz / length)],
                river: id,
            });
        }
        let from_lake = (routed.lake[head as usize] != NONE).then(|| routed.lake[head as usize]);
        rivers.push(River {
            vertices,
            order: order[head as usize],
            end,
            from_lake,
        });
    }

    let lakes = routed
        .lakes
        .iter()
        .map(|lake| Lake {
            level_m: single(lake.level),
            depth_m: single(lake.depth),
            nodes: lake.nodes.iter().map(|&m| nodes[m as usize]).collect(),
            outlet: position(nodes[lake.outlet as usize]),
            discharge_m3s: single(q[lake.outlet as usize]),
            detail_m: 0.0,
        })
        .collect();
    Built {
        rivers,
        lakes,
        falls,
    }
}

/// A node's offset off the lattice, up to `amount` metres either way on
/// each axis, from its coordinate and the seed alone, so a confluence two
/// reaches share is in one place.
fn jitter(seed: u64, node: [i32; 2], amount: f64) -> [f64; 2] {
    let packed = u64::from(node[0].cast_unsigned()) | (u64::from(node[1].cast_unsigned()) << 32);
    let h = mix64(seed ^ mix64(packed));
    [
        (unit(h) - 0.5) * 2.0 * amount,
        (unit(mix64(h)) - 0.5) * 2.0 * amount,
    ]
}

/// 53 bits of a hash as a float in `0..1`.
#[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1_u64 << 53) as f64
}

/// One pass of Chaikin's corner cutting, the ends kept: every new point
/// lies on the old polyline, a quarter in from a corner, so a curve that
/// kept clear of something still does.
fn chaikin(points: &[Point]) -> Vec<Point> {
    let last = points.len() - 1;
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for i in 0..last {
        let (a, b) = (points[i], points[i + 1]);
        if i > 0 {
            out.push(a.lerp(b, 0.25));
        }
        if i + 1 < last {
            out.push(a.lerp(b, 0.75));
        }
    }
    out.push(points[last]);
    out
}

/// Where the falls of a cascade stand along the chain: the middles of the
/// equal parts of each steep step.
fn fall_positions(steep: &[f64]) -> Vec<f64> {
    let mut out = Vec::new();
    for (k, &count) in steep.iter().enumerate() {
        let mut m = 0.0;
        while m < count {
            out.push(super::real(k) + (m + 0.5) / count);
            m += 1.0;
        }
    }
    out
}

/// The reach with a vertex added between any two falls one pair of
/// vertices would otherwise straddle, so each fall is a stretch of its own
/// and no record lumps a cascade into one drop.
fn split_falls(points: &[Point], steep: &[f64]) -> Vec<Point> {
    let falls = fall_positions(steep);
    if falls.is_empty() {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() + falls.len());
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        out.push(a);
        let inside: Vec<f64> = falls
            .iter()
            .copied()
            .filter(|&t| t > a.t && t <= b.t)
            .collect();
        for between in inside.windows(2) {
            let t = f64::midpoint(between[0], between[1]);
            out.push(a.lerp(b, (t - a.t) / (b.t - a.t)));
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// Sways a smoothed reach sideways by a noise along the ground, faded in
/// from each end (so confluences stay put) and near a settlement, and held
/// down by the province's grain, which the routing already follows.
fn sway(
    points: &mut [Point],
    params: &RiverParams,
    meander: &Fractal,
    freedom: &impl Fn(f64, f64) -> f64,
    grain: &impl Fn(f64) -> f64,
) {
    let len = points.len();
    if len < 3 || params.meander_m <= 0.0 {
        return;
    }
    let mut along = vec![0.0; len];
    for i in 1..len {
        let (dx, dz) = (points[i].x - points[i - 1].x, points[i].z - points[i - 1].z);
        along[i] = along[i - 1] + (dx * dx + dz * dz).sqrt();
    }
    let total = along[len - 1];
    let offsets: Vec<[f64; 2]> = (1..len - 1)
        .map(|i| {
            let (tx, tz) = (
                points[i + 1].x - points[i - 1].x,
                points[i + 1].z - points[i - 1].z,
            );
            let length = (tx * tx + tz * tz).sqrt();
            if length <= 0.0 {
                return [0.0, 0.0];
            }
            // A reach of a lattice step or two still sways in its middle:
            // with the whole taper it ran dead straight for 512 m.
            let taper = smoothstep(
                0.0,
                params.meander_taper_m.min(total * 0.3),
                along[i].min(total - along[i]),
            );
            let amount = params.meander_m
                * (1.0 - grain(points[i].t).clamp(0.0, 1.0))
                * meander.get(points[i].x, points[i].z)
                * taper
                * freedom(points[i].x, points[i].z);
            [-tz / length * amount, tx / length * amount]
        })
        .collect();
    for (point, offset) in points[1..len - 1].iter_mut().zip(offsets) {
        point.x += offset[0];
        point.z += offset[1];
    }
}

/// Grades every reach that ends in the sea to the sample's own shore, on up
/// to `threads` threads: a mouth lies on the lattice's coast, which is up to
/// a node off the real one, so its surface reached sea level out over the
/// water or tens of metres up on land. Where the sample's ground is still
/// land at the mouth the reach runs on straight to the shore and a few
/// metres past it; where it is already sea the shore is found back along
/// the reach. Either way the surface falls in a straight grade over
/// [`RiverParams::mouth_grade_m`] (from the last fall at most) to nought at
/// the shore, and never rises downstream.
pub(crate) fn grade_mouths(
    built: &mut Built,
    params: &RiverParams,
    world: &crate::ProceduralWorld,
    threads: usize,
) {
    let mouths: Vec<usize> = built
        .rivers
        .iter()
        .enumerate()
        .filter(|(_, river)| river.end == RiverEnd::Sea && river.vertices.len() >= 2)
        .map(|(i, _)| i)
        .collect();
    let rivers = &built.rivers;
    let graded = super::field::parallel_rows(mouths.len(), threads, |k| {
        grade_mouth(&rivers[mouths[k]].vertices, params, world)
    });
    for (k, vertices) in graded.into_iter().enumerate() {
        if let Some(vertices) = vertices {
            built.rivers[mouths[k]].vertices = vertices;
        }
    }
}

/// Metres between the points a mouth's shore is looked for at, before it
/// is narrowed down.
const SHORE_STEP_M: f64 = 16.0;

/// The turns a mouth's run-on tries, as `(cos, sin)`: straight on, then
/// half a radian and a radian either way.
const MOUTH_TURNS: [(f64, f64); 5] = [
    (1.0, 0.0),
    (0.877_582_561_890_372_8, 0.479_425_538_604_203),
    (0.877_582_561_890_372_8, -0.479_425_538_604_203),
    (0.540_302_305_868_139_8, 0.841_470_984_807_896_5),
    (0.540_302_305_868_139_8, -0.841_470_984_807_896_5),
];

/// One reach's vertices graded to the sample's shore, or `None` where no
/// shore is found within [`RiverParams::mouth_search_m`].
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "the shore found either way, then the grade; a segment's ends and steps"
)]
fn grade_mouth(
    vertices: &[RiverVertex],
    params: &RiverParams,
    world: &crate::ProceduralWorld,
) -> Option<Vec<RiverVertex>> {
    let n = vertices.len();
    let land = |x: f64, z: f64| world.natural_height(x, z) >= 0.0;
    let mut arc = vec![0.0; n];
    for k in 1..n {
        let (dx, dz) = (
            vertices[k].x - vertices[k - 1].x,
            vertices[k].z - vertices[k - 1].z,
        );
        arc[k] = arc[k - 1] + (dx * dx + dz * dz).sqrt();
    }
    // Where along `from` to `to` the ground turns from land to sea, by
    // halving, both ends given.
    let crossing = |from: [f64; 2], to: [f64; 2]| {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..6 {
            let mid = f64::midpoint(lo, hi);
            let p = [
                from[0] + (to[0] - from[0]) * mid,
                from[1] + (to[1] - from[1]) * mid,
            ];
            if land(p[0], p[1]) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        f64::midpoint(lo, hi)
    };
    let last = vertices[n - 1];
    let mut out = vertices.to_vec();
    let mut arc_out = arc.clone();
    let extended;
    let shore = if land(last.x, last.z) {
        // On along the last segment's line until the sea.
        let prev = vertices[n - 2];
        let (dx, dz) = (last.x - prev.x, last.z - prev.z);
        let length = (dx * dx + dz * dz).sqrt();
        if length <= 0.0 {
            return None;
        }
        let ahead = [dx / length, dz / length];
        // Straight on first; where the last segment's line misses the sea
        // (it turns with the reach's sway and settling, and a mouth on a
        // headland's flank runs along the coast), turned by up to about a
        // radian either way, the nearest shore of those.
        let mut best: Option<(f64, [f64; 2])> = None;
        for (cos, sin) in MOUTH_TURNS {
            let dir = [
                ahead[0] * cos - ahead[1] * sin,
                ahead[0] * sin + ahead[1] * cos,
            ];
            let at = |s: f64| [last.x + dir[0] * s, last.z + dir[1] * s];
            let mut s = SHORE_STEP_M;
            let found = loop {
                if s > params.mouth_search_m
                    || best.is_some_and(|(held, _)| s > held + SHORE_STEP_M)
                {
                    break None;
                }
                let p = at(s);
                if !land(p[0], p[1]) {
                    break Some(
                        s - SHORE_STEP_M + SHORE_STEP_M * crossing(at(s - SHORE_STEP_M), p),
                    );
                }
                s += SHORE_STEP_M;
            };
            if let Some(found) = found
                && best.is_none_or(|(held, _)| found < held)
            {
                best = Some((found, dir));
            }
            if best.is_some() && cos >= 1.0 {
                break;
            }
        }
        let (found, dir) = best?;
        let at = |s: f64| [last.x + dir[0] * s, last.z + dir[1] * s];
        // A few metres out past the shore, in steps of about a lattice
        // spacing's sixteenth, as the corner cutting leaves a reach.
        let beyond = found + f64::from(last.width_m) * 0.5 + 8.0;
        let steps = (beyond / 32.0).ceil().max(1.0);
        let mut k = 1.0;
        while k <= steps {
            let p = at(beyond * k / steps);
            out.push(RiverVertex {
                x: p[0],
                z: p[1],
                fall: false,
                ..last
            });
            arc_out.push(arc[n - 1] + beyond * k / steps);
            k += 1.0;
        }
        extended = true;
        arc[n - 1] + found
    } else {
        // Back along the reach to the last land.
        let mut found = None;
        'back: for k in (0..n - 1).rev() {
            if arc[n - 1] - arc[k + 1] > params.mouth_search_m {
                break;
            }
            let (a, b) = (vertices[k], vertices[k + 1]);
            let length = arc[k + 1] - arc[k];
            let steps = (length / SHORE_STEP_M).ceil().max(1.0);
            let mut i = steps;
            while i >= 1.0 {
                let f0 = (i - 1.0) / steps;
                let p0 = [a.x + (b.x - a.x) * f0, a.z + (b.z - a.z) * f0];
                if land(p0[0], p0[1]) {
                    let f1 = i / steps;
                    let p1 = [a.x + (b.x - a.x) * f1, a.z + (b.z - a.z) * f1];
                    let f = f0 + (f1 - f0) * crossing(p0, p1);
                    found = Some(arc[k] + length * f);
                    break 'back;
                }
                i -= 1.0;
            }
        }
        extended = false;
        found?
    };
    // The grade starts no higher up than the last fall's foot, and for a
    // reach run on past its old mouth no lower than its last vertex but
    // one, whose surface is the last land node's.
    let after_fall = vertices
        .iter()
        .rposition(|v| v.fall)
        .map_or(0.0, |k| arc[k + 1]);
    let mut start = (shore - params.mouth_grade_m).max(after_fall).max(0.0);
    if extended {
        start = start.min(arc[n - 2]).max(after_fall);
    }
    let surface_at = |a: f64| {
        let k = arc.partition_point(|&x| x <= a).clamp(1, n - 1);
        let span = arc[k] - arc[k - 1];
        let f = if span > 0.0 {
            ((a - arc[k - 1]) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (s0, s1) = (
            f64::from(vertices[k - 1].surface_m),
            f64::from(vertices[k].surface_m),
        );
        s0 + (s1 - s0) * f
    };
    let top = surface_at(start);
    // A vertex at the shore itself, so the surface is nought there and not
    // interpolated towards a vertex out at sea: without it the bank's fill
    // stood to a surface metres up at the waterline, and the fill's own end
    // over the sea cut it off in a wall up to seven metres high.
    let at = arc_out.partition_point(|&a| a <= shore);
    if at > 0 && at < out.len() {
        let (a0, a1) = (arc_out[at - 1], arc_out[at]);
        if shore - a0 > SHORE_VERTEX_M && a1 - shore > SHORE_VERTEX_M {
            let (p, q) = (out[at - 1], out[at]);
            let f = (shore - a0) / (a1 - a0);
            let lerp = |u: f32, v: f32| single(f64::from(u) + (f64::from(v) - f64::from(u)) * f);
            out.insert(
                at,
                RiverVertex {
                    x: p.x + (q.x - p.x) * f,
                    z: p.z + (q.z - p.z) * f,
                    surface_m: lerp(p.surface_m, q.surface_m),
                    discharge_m3s: lerp(p.discharge_m3s, q.discharge_m3s),
                    width_m: lerp(p.width_m, q.width_m),
                    depth_m: lerp(p.depth_m, q.depth_m),
                    cut_m: p.cut_m,
                    fall: p.fall,
                    meander_m: p.meander_m,
                    phase: p.phase,
                },
            );
            arc_out.insert(at, shore);
        }
    }
    // The grade's line, never over the ground under a vertex less the
    // freeboard (a beach lower than the line would stand the river on an
    // embankment), nor over the surface the fit gave the vertex (a run-on's
    // the last one's), and never rising downstream.
    let mut held = f64::INFINITY;
    for (v, &a) in out.iter_mut().zip(&arc_out) {
        if a < start {
            held = f64::from(v.surface_m);
            continue;
        }
        let line = if a >= shore || shore <= start {
            0.0
        } else {
            top * (shore - a) / (shore - start)
        };
        let natural = world.natural_height(v.x, v.z);
        let graded = line
            .min(natural - params.freeboard_m)
            .min(f64::from(v.surface_m));
        held = held.min(graded.max(0.0));
        v.surface_m = single(held);
        v.cut_m = single((natural - held).max(0.0));
    }
    Some(out)
}

/// How far past a lake's coarse shore the reach that leaves it is held at
/// its level, metres: past the rim the carve keeps over the level
/// (`carve::RIM_OUT_M` and its fade), so the outlet's cut never lowers it
/// and its channel, at the level, is never filled by it.
pub(crate) const LAKE_HOLD_M: f64 = super::carve::RIM_OUT_M + super::carve::RIM_FADE_M + 20.0;

/// The least distance from an outlet's vertices at which the point where it
/// leaves its lake's rim gets a vertex of its own, metres.
const RIM_VERTEX_M: f64 = 4.0;

/// How far past a reach's end its breaches are graded along the reaches
/// below it, metres.
const LOOKAHEAD_M: f64 = 6_000.0;

/// The steepest a reach runs into a confluence that came down under it,
/// rise over run.
const TAPER: f64 = 0.05;

/// How far under its own level a surface must be held to count as breached,
/// metres.
const BREACH_M: f64 = 0.5;

/// The most a graded breach may lower a surface under where the breach held
/// it, as a share of how far the breach held it under its own level, and
/// the least and most of that, metres.
const BREACH_GRADE: (f64, f64, f64) = (0.5, 5.0, 30.0);

/// Grades every breach of a reach's surfaces: where something rose in the
/// river's way and the surface was held at the level before it (a canal,
/// dead level in its slot for up to a few kilometres, that then dropped down
/// the far side's ground in a ramp), the surface falls instead in a straight
/// line from where it was first held to a vertex past the breach. That
/// vertex is the one whose line is the steepest down that stays under every
/// surface between and lowers none by more than [`BREACH_GRADE`] of the
/// breach's depth: the line is the reach's lower convex hull over the
/// breach and its far side, cut short where it would sink a gorge far
/// deeper than the breach already cut. Only ever lowers, so the surface
/// still never rises downstream, stays over the floors (a line between two
/// surfaces over a floor is over it) and never stands over the ground. The
/// first `keep` surfaces are not lowered and no breach starts among them:
/// an outlet held at its lake's level through the rim is cut through the
/// ground round the lake on purpose, and grading it down from the lake
/// left the channel under the level inside the rim, which the carve then
/// filled back (ticket 0005, the review of step 3b).
fn grade_breaches(surfaces: &mut [f64], targets: &[f64], arc: &[f64], keep: usize) {
    let n = surfaces.len();
    let mut i = keep.saturating_sub(1);
    while i + 1 < n {
        if surfaces[i + 1] >= targets[i + 1] - BREACH_M {
            i += 1;
            continue;
        }
        let a = i;
        let mut b = a + 1;
        let mut deepest = 0.0_f64;
        while b < n && surfaces[b] < targets[b] - BREACH_M {
            deepest = deepest.max(targets[b] - surfaces[b]);
            b += 1;
        }
        let b = b.min(n - 1);
        let allowed = (deepest * BREACH_GRADE.0).clamp(BREACH_GRADE.1, BREACH_GRADE.2);
        let window = 2.0 * (arc[b] - arc[a]) + 500.0;
        let slope_to = |c: usize| {
            let run = arc[c] - arc[a];
            if run > 0.0 {
                (surfaces[c] - surfaces[a]) / run
            } else {
                0.0
            }
        };
        let (mut best, mut best_slope) = (b, slope_to(b));
        for c in b + 1..n {
            if arc[c] - arc[a] > window {
                break;
            }
            let slope = slope_to(c);
            if slope >= best_slope {
                continue;
            }
            let lowers = (a + 1..c)
                .map(|k| surfaces[k] - (surfaces[a] + slope * (arc[k] - arc[a])))
                .fold(0.0_f64, f64::max);
            if lowers <= allowed {
                (best, best_slope) = (c, slope);
            }
        }
        for k in a + 1..best {
            let line = surfaces[a] + best_slope * (arc[k] - arc[a]);
            surfaces[k] = surfaces[k].min(line);
        }
        i = best.max(a + 1);
    }
}

/// The least distance from an existing vertex at which a mouth's shore gets
/// a vertex of its own, metres.
const SHORE_VERTEX_M: f64 = 0.5;

/// Sets every reach's surface onto the sample's own ground along it, on up
/// to `threads` threads for the ground. The network's surfaces are the
/// coarse field's, whose valley noise and missing finer relief put a river
/// tens of metres over or under the real ground (seed 42 round Port Serein,
/// the land vertices: a fifth over a metre above it, one in thirteen over
/// ten; and the median 10 m, the 95th percentile 107 m under it). Carved as
/// they were, those rivers stood on embankments or ran in slots a hundred
/// metres deep that began as pits at their sources. The surface is the
/// ground under each vertex less the freeboard and an incision that grows
/// with the square root of discharge ([`RiverParams::incision_m`]), and is
/// then breached (no vertex higher than one upstream of it): the river
/// never stands over the ground it runs on, sinks into it downstream, and
/// cuts through what rises in its way, on a graded line and not dead level
/// ([`grade_breaches`], over the reaches below it too). Reaches are set
/// from their sources down: a reach begins no higher than every reach that
/// joins it ends, and each of those then ends at its beginning, steepening
/// into it at [`TAPER`] where that came down; a reach leaving a lake begins
/// at its level and keeps it past the lake's rim ([`LAKE_HOLD_M`], read
/// from `shore`), and none ends under the lake it runs into or under the
/// sea. The falls' records take the new surfaces.
#[allow(
    clippy::too_many_lines,
    reason = "the ground, the order, the floors, the surfaces and the falls, in order"
)]
pub(crate) fn fit_surfaces(
    built: &mut Built,
    params: &RiverParams,
    world: &crate::ProceduralWorld,
    threads: usize,
    shore: &Shore,
) {
    // How far along each reach that leaves a lake its surface is held at
    // the lake's level: until it has left the lake's rim (its gate, or
    // [`LAKE_HOLD_M`] past the coarse shore), so the outlet's valley
    // starts at the level and its cut begins past the rim (ticket 0005,
    // step 3b; cut below the level beside the lake, it left the lake
    // perched over a dry bed). The surface runs straight between vertices,
    // so a vertex is set where the reach leaves the rim and held too: one
    // falling from the last vertex inside ran under the level within the
    // rim, which the carve filled back over the channel, and holding the
    // next vertex instead ran a perched lake's outlet out on an embankment
    // past the rim (the review of step 3b).
    let past_rim = {
        let lakes = &built.lakes;
        move |id: u32, x: f64, z: f64| {
            shore
                .lake_here(lakes, x, z)
                .filter(|here| here.id == id)
                .is_none_or(|here| {
                    here.inside_m() < -LAKE_HOLD_M || here.depth() < -super::carve::SHORE_GATE_M
                })
        }
    };
    let mut held_to = vec![0; built.rivers.len()];
    for (r, river) in built.rivers.iter_mut().enumerate() {
        let Some(id) = river.from_lake else {
            continue;
        };
        let v = &mut river.vertices;
        let Some(k) = v.iter().position(|p| past_rim(id, p.x, p.z)) else {
            held_to[r] = v.len();
            continue;
        };
        held_to[r] = k + 1;
        if k == 0 || v[k - 1].fall {
            continue;
        }
        let (a, b) = (v[k - 1], v[k]);
        let (mut inside, mut past) = (0.0, 1.0);
        for _ in 0..12 {
            let mid = f64::midpoint(inside, past);
            if past_rim(id, a.x + (b.x - a.x) * mid, a.z + (b.z - a.z) * mid) {
                past = mid;
            } else {
                inside = mid;
            }
        }
        let length = ((b.x - a.x) * (b.x - a.x) + (b.z - a.z) * (b.z - a.z)).sqrt();
        if past * length > RIM_VERTEX_M && (1.0 - past) * length > RIM_VERTEX_M {
            let lerp = |p: f32, q: f32| single(f64::from(p) + (f64::from(q) - f64::from(p)) * past);
            v.insert(
                k,
                RiverVertex {
                    x: a.x + (b.x - a.x) * past,
                    z: a.z + (b.z - a.z) * past,
                    surface_m: lerp(a.surface_m, b.surface_m),
                    discharge_m3s: lerp(a.discharge_m3s, b.discharge_m3s),
                    width_m: lerp(a.width_m, b.width_m),
                    depth_m: lerp(a.depth_m, b.depth_m),
                    cut_m: lerp(a.cut_m, b.cut_m),
                    fall: false,
                    meander_m: lerp(a.meander_m, b.meander_m),
                    phase: 0.0,
                },
            );
        }
    }
    let rivers = &built.rivers;
    // A reach that starts at a source, not at a confluence or a lake.
    let mut fed = vec![false; rivers.len()];
    for river in rivers {
        if let RiverEnd::Joins(next) = river.end {
            fed[next as usize] = true;
        }
    }
    // Per vertex the natural ground, and the level the river would take
    // under it: the freeboard and the incision down, with a range's valleys
    // as deep as the routing assumed them once a river has the water to cut
    // them. From a source the incision grows in over
    // [`RiverParams::source_taper_m`], so a valley heads in the ground
    // rather than starting as a pit.
    let ground: Vec<Vec<(f64, f64)>> = super::field::parallel_rows(rivers.len(), threads, |r| {
        let river = &rivers[r];
        let source = !fed[r] && river.from_lake.is_none();
        let mut along = 0.0;
        let mut previous: Option<&RiverVertex> = None;
        river
            .vertices
            .iter()
            .map(|v| {
                if let Some(p) = previous {
                    let (dx, dz) = (v.x - p.x, v.z - p.z);
                    along += (dx * dx + dz * dz).sqrt();
                }
                previous = Some(v);
                let natural = world.natural_height(v.x, v.z);
                let range = world.continent(v.x, v.z).range_m.max(0.0);
                let q = f64::from(v.discharge_m3s).max(0.0);
                let incision = params.incision_m
                    + params.incision_per_root_m * q.sqrt()
                    + params.valley_range_share * range * smoothstep(0.1, 2.0, q);
                let grown = if source {
                    smoothstep(0.0, params.source_taper_m, along)
                } else {
                    1.0
                };
                (natural, natural - params.freeboard_m - incision * grown)
            })
            .collect()
    });
    // Sources first: a reach once every reach that joins it is set.
    let mut upstream: Vec<Vec<usize>> = vec![Vec::new(); rivers.len()];
    let mut below: Vec<Option<usize>> = vec![None; rivers.len()];
    for (r, river) in rivers.iter().enumerate() {
        if let RiverEnd::Joins(next) = river.end {
            upstream[next as usize].push(r);
            below[r] = Some(next as usize);
        }
    }
    let mut waiting: Vec<usize> = upstream.iter().map(Vec::len).collect();
    let mut order: Vec<usize> = (0..rivers.len()).filter(|&r| waiting[r] == 0).collect();
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
    // The lowest a reach may run: the level of the lake or the sea its water
    // ends in, set from the sea up so a tributary of a reach into a lake
    // keeps over that lake too.
    let mut floors = vec![f64::NEG_INFINITY; built.rivers.len()];
    for &r in order.iter().rev() {
        floors[r] = match built.rivers[r].end {
            RiverEnd::Lake(lake) => f64::from(built.lakes[lake as usize].level_m),
            RiverEnd::Sea => 0.0,
            RiverEnd::Joins(next) => floors[next as usize],
            RiverEnd::Edge | RiverEnd::Sink => f64::NEG_INFINITY,
        };
    }
    for &r in &order {
        let floor = floors[r];
        let start = built.rivers[r]
            .from_lake
            .map(|lake| f64::from(built.lakes[lake as usize].level_m));
        let joined = upstream[r]
            .iter()
            .map(|&u| {
                let v = &built.rivers[u].vertices;
                f64::from(v[v.len() - 1].surface_m)
            })
            .fold(f64::INFINITY, f64::min);
        let mut held = f64::INFINITY;
        let mut targets = Vec::new();
        let mut surfaces = Vec::new();
        let mut arc = Vec::new();
        let mut along = 0.0;
        // The reach's own vertices, then those of the reaches below it as
        // far as [`LOOKAHEAD_M`], their surfaces as the breach would hold
        // them from this reach's end: a breach that runs on past a
        // confluence is graded as one, and this reach's end, the
        // confluence, comes down with it.
        let mut reach = Some(r);
        let mut own = 0;
        while let Some(at) = reach {
            let vertices = &built.rivers[at].vertices;
            let first = usize::from(at != r);
            for (i, v) in vertices.iter().enumerate().skip(first) {
                if i > 0 {
                    let p = vertices[i - 1];
                    let (dx, dz) = (v.x - p.x, v.z - p.z);
                    along += (dx * dx + dz * dz).sqrt();
                }
                let mut level = ground[at][i].1;
                if at == r && i == 0 {
                    level = start.unwrap_or(level.min(joined));
                }
                let mut level = level.max(if at == r { floor } else { floors[at] });
                if i < held_to[at]
                    && let Some(lake) = built.rivers[at].from_lake
                {
                    level = level.max(f64::from(built.lakes[lake as usize].level_m));
                }
                held = held.min(level);
                targets.push(level);
                surfaces.push(held);
                arc.push(along);
            }
            if at == r {
                own = surfaces.len();
            }
            reach = below[at].filter(|_| along - arc[own - 1] < LOOKAHEAD_M);
        }
        grade_breaches(&mut surfaces, &targets, &arc, held_to[r]);
        let vertices = &mut built.rivers[r].vertices;
        for (i, v) in vertices.iter_mut().enumerate() {
            v.surface_m = single(surfaces[i]);
            v.cut_m = single((ground[r][i].0 - f64::from(v.surface_m)).max(0.0));
        }
        let (begins, cut) = (vertices[0].surface_m, vertices[0].cut_m);
        debug_assert_eq!(own, vertices.len());
        for &u in &upstream[r] {
            let v = &mut built.rivers[u].vertices;
            let last = v.len() - 1;
            v[last].surface_m = begins;
            v[last].cut_m = cut;
            // A reach whose end came down with another's graded breach
            // steepens into the confluence over the metres the drop needs
            // at [`TAPER`], not in its last segment; never its first
            // vertex, which the reaches above it end at, nor an outlet's
            // stretch held at its lake's level through the rim (a short
            // outlet into a lower main stem was steepened back into its
            // lake and dammed by the rim).
            let mut run = 0.0;
            for k in (held_to[u].max(1)..last).rev() {
                let (dx, dz) = (v[k + 1].x - v[k].x, v[k + 1].z - v[k].z);
                run += (dx * dx + dz * dz).sqrt();
                let line = f64::from(begins) + TAPER * run;
                if f64::from(v[k].surface_m) <= line {
                    break;
                }
                let natural = f64::from(v[k].surface_m) + f64::from(v[k].cut_m);
                v[k].surface_m = single(line);
                v[k].cut_m = single((natural - line).max(0.0));
            }
        }
    }
    // The falls' lips and feet, from the vertices they stand between.
    let mut next = vec![0_usize; built.rivers.len()];
    for fall in &mut built.falls {
        let river = &built.rivers[fall.river as usize];
        let from = next[fall.river as usize];
        if let Some(k) =
            (from..river.vertices.len().saturating_sub(1)).find(|&k| river.vertices[k].fall)
        {
            fall.top[1] = f64::from(river.vertices[k].surface_m);
            fall.bottom[1] = f64::from(river.vertices[k + 1].surface_m);
            next[fall.river as usize] = k + 1;
        }
    }
}
