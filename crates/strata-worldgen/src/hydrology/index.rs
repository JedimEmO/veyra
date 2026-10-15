//! The point query: a bucket grid over the reaches' segments, and the
//! lakes' nodes with the ground round them.

use super::carve::valley_bound;
use super::dual::{D, soft};
use super::mass::{Domain, NEIGHBOURS};
use super::route::{Grid, NONE, Routed};
use super::shape;
use super::{
    LATTICE_M, Lake, NodeMap, River, RiverEnd, RiverParams, Water, WaterKind, local, node_of, real,
    single,
};

/// One stretch of a reach between two vertices.
#[derive(Debug, Clone, Copy)]
struct Segment {
    a: [f64; 2],
    b: [f64; 2],
    /// `b - a`, one over its squared length (zero for a point) and its unit
    /// normal `(-dz, dx) / length`: what every query of the segment needs,
    /// worked out once.
    d: [f64; 2],
    inv_length2: f64,
    normal: [f64; 2],
    surface: [f32; 2],
    /// How the surface runs on past `a` and past `b`, metres a metre: the
    /// gentler of this segment's slope and its neighbour's there, zero at
    /// a reach's ends.
    run_on: [f64; 2],
    discharge: [f32; 2],
    cut: [f32; 2],
    /// The meanders' amplitude, metres, their share of a full meander
    /// ([`shape::bend_share`]) and their phase, at `a` and at `b`
    /// ([`super::RiverVertex::meander_m`]).
    amplitude: [f32; 2],
    share: [f32; 2],
    phase: [f32; 2],
    order: u8,
    kind: WaterKind,
}

/// The segments, and for each bucket of [`RiverParams::bucket_m`] the ones
/// within [`RiverParams::reach_m`] of it, so a query reads one bucket.
#[derive(Debug)]
pub(crate) struct SegmentIndex {
    segments: Vec<Segment>,
    /// Per bucket, its run in `entries`.
    buckets: NodeMap<Run>,
    entries: Vec<u32>,
}

/// The bucket a coordinate is in.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over a bucket is far inside an i32"
)]
fn bucket(metres: f64, side: f64) -> i32 {
    (metres / side).floor() as i32
}

/// The softness of the wider weights the surface, the discharge and the
/// rest are blended by, metres.
const SOFT_M: f64 = 6.0;

/// Where a segment's weight is cut off, by its squared distance (plus the
/// wider softness's) over the nearest segment's: whole to one and a half
/// times as far, nothing from twice, smoothly between. A segment one and a
/// half times as far as the nearest weighs a twenty-fifth of it, twice as
/// far a 256th, and skipping the far ones makes a point in a valley read a
/// handful of segments where its bucket holds dozens (from three times as
/// far, as step 2 was first built, the blend cost a twentieth of a
/// microsecond more along a river valley).
const CUTOFF: (f64, f64) = (2.25, 4.0);

/// [`CUTOFF`]'s factor and its derivative by the ratio.
fn cutoff(ratio: f64) -> (f64, f64) {
    if ratio <= CUTOFF.0 {
        return (1.0, 0.0);
    }
    let span = CUTOFF.1 - CUTOFF.0;
    let t = ((ratio - CUTOFF.0) / span).min(1.0);
    (1.0 - t * t * (3.0 - 2.0 * t), -6.0 * t * (1.0 - t) / span)
}

/// The nearest segment to a point over every run that reaches it, by its
/// weighed distance ([`Reading`]): that squared, and half its gradient.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Near {
    pub c2: f64,
    pub q: [f64; 2],
    /// The least true distance squared to any segment read: what says no
    /// valley reaches the point.
    pub min_c2: f64,
}

impl Default for Near {
    fn default() -> Self {
        Self {
            c2: f64::INFINITY,
            q: [0.0, 0.0],
            min_c2: f64::INFINITY,
        }
    }
}

/// How much farther a segment counts for each metre the point lies past
/// one of its ends, along it, in the weights.
const OVERSHOOT: f64 = 2.0;

/// Over how many metres past an end [`OVERSHOOT`] eases in.
const OVERSHOOT_EASE_M: f64 = 4.0;

/// A segment read from a point.
#[derive(Debug, Clone, Copy)]
struct Reading {
    /// Where the point projects along the segment, `0` at `a`, `1` at `b`.
    t: f64,
    /// From the segment's nearest point to the point.
    r: [f64; 2],
    /// `r` squared.
    c2: f64,
    /// The distance it is weighed by, squared: the distance, and
    /// [`OVERSHOOT`] times how far past an end the point projects. A
    /// segment the point lies opposite counts at its distance; its
    /// neighbours, which the point lies past the end of, count farther, so
    /// they barely lift the weighted distance (by a fifth of a percent at
    /// 100 m, where at their own distance they lifted it by one and a half
    /// and rippled the valley's walls at every vertex). On the outside of a
    /// bend, where two segments are nearest at the vertex they share, both
    /// lie past an end by about as much, and weigh alike.
    e2: f64,
    /// Half the gradient of `e2`.
    q: [f64; 2],
}

impl Segment {
    /// The segment read from `(x, z)`, or `None` if it is `bound` (a
    /// squared distance) or farther: the weighed distance is never under
    /// the distance, so a segment that far is skipped before any root.
    #[allow(
        clippy::many_single_char_names,
        reason = "the geometry's own letters: the point, the projection, the offset"
    )]
    fn read(&self, x: f64, z: f64, bound: f64) -> Option<Reading> {
        let [dx, dz] = self.d;
        let (ax, az) = (x - self.a[0], z - self.a[1]);
        let t = (ax * dx + az * dz) * self.inv_length2;
        let u = t.clamp(0.0, 1.0);
        let r = [ax - dx * u, az - dz * u];
        let c2 = r[0] * r[0] + r[1] * r[1];
        if c2 >= bound {
            return None;
        }
        let past = (-t).max(t - 1.0);
        if past <= 0.0 || self.inv_length2 <= 0.0 {
            return Some(Reading {
                t,
                r,
                c2,
                e2: c2,
                q: r,
            });
        }
        let length = 1.0 / self.inv_length2.sqrt();
        let c = c2.sqrt();
        // Metres past the end, eased in over the first few so the weight's
        // gradient does not jump at the end (it creased the bank there).
        let metres = past * length;
        let (eased, slope) = if metres < OVERSHOOT_EASE_M {
            (
                metres * metres / (2.0 * OVERSHOOT_EASE_M),
                metres / OVERSHOOT_EASE_M,
            )
        } else {
            (metres - 0.5 * OVERSHOOT_EASE_M, 1.0)
        };
        let e = c + OVERSHOOT * eased;
        // Past `b` the overshoot grows along the segment, past `a` against.
        let sign = if t > 1.0 { 1.0 } else { -1.0 };
        let along = OVERSHOOT * slope * sign / length;
        let gc = if c > 0.0 {
            [r[0] / c, r[1] / c]
        } else {
            [0.0, 0.0]
        };
        Some(Reading {
            t,
            r,
            c2,
            e2: e * e,
            q: [e * (gc[0] + along * dx), e * (gc[1] + along * dz)],
        })
    }
}

/// A segment a scan read, for the blend.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Found {
    /// Which of the cell's runs it is in.
    pub network: u16,
    segment: u32,
    reading: Reading,
}

/// The softness of a centreline's weight, metres: a point on a segment
/// weighs `1 / EPSILON^8` against `1 / d^8` for one `d` off.
const EPSILON_M: f64 = 0.5;

/// What the carve reads of the rivers round a point: each segment within
/// reach weighted by `(1 - c^2/R^2)^2 / (c^2 + e^2)^4` of its distance `c`,
/// so the nearest dominates by the eighth power and one leaving the reach
/// fades to nothing, and every sum carries its gradient. A sum of weights
/// rather than the nearest segment alone, because the nearest flips at a
/// bend's inner bisector and between two reaches, and what it says (the
/// surface above all) would step there.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Blend {
    w: f64,
    gw: [f64; 2],
    /// Sum of weight times distance from the segment's line.
    wd: f64,
    gwd: [f64; 2],
    /// The wider weights, [`SOFT_M`]'s: what the surface, the discharge
    /// and the rest are blended by, so that at a confluence they turn from
    /// one reach's to another's over metres and not in the half metre the
    /// distance's weights do.
    v: f64,
    gv: [f64; 2],
    /// Sum of the wider weight times the surface at the nearest point.
    ws: f64,
    gws: [f64; 2],
    /// The wider weights times the discharge, the cut, the order and the
    /// direction downstream, each with its gradient (the direction's by
    /// component, `gwf[i]` being component `i`'s).
    wq: f64,
    gwq: [f64; 2],
    wc: f64,
    gwc: [f64; 2],
    wo: f64,
    gwo: [f64; 2],
    wf: [f64; 2],
    gwf: [[f64; 2]; 2],
    /// The wider weights times the meanders' numbers at the point's
    /// projection on each segment: their phase, its rate along the line
    /// (per metre), their amplitude and its rate, and their share of a
    /// full meander, each with its gradient. The wave itself is read once,
    /// at the blended phase, in [`Blend::finish`]: per segment it cost
    /// the carve a fifth of a microsecond along a river.
    wp: f64,
    gwp: [f64; 2],
    wpr: f64,
    gwpr: [f64; 2],
    wa: f64,
    gwa: [f64; 2],
    war: f64,
    gwar: [f64; 2],
    wg: f64,
    gwg: [f64; 2],
    /// The nearest segment: its squared distance, the vector from it to
    /// the point, which side, its order and kind.
    near2: f64,
    near_r: [f64; 2],
    near_order: u8,
    near_kind: Option<WaterKind>,
}

/// One segment's surface as the blend read it, kept for the soft minimum
/// [`Blend::finish`] takes over them: the wider weight, the surface, and
/// their gradients.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceReading {
    v: f64,
    gv: [f64; 2],
    s: f64,
    gs: [f64; 2],
    /// The point's signed distance from the segment's line, and that
    /// line's normal (its gradient): what the channel's offset is read
    /// from, once the meander at the point is known.
    line: f64,
    normal: [f64; 2],
}

/// The least scale of the soft minimum over the segments' surfaces, metres:
/// a segment reading the spread of the readings (or this, where they all
/// agree) over their weighted mean weighs a quarter, three times that a
/// hundredth.
const SOFT_MIN_M: f64 = 0.05;

/// The softness of where a segment's surface starts to count as over the
/// mean, as a share of the soft minimum's scale.
const SOFT_MIN_RAMP: f64 = 0.5;

/// How far out from the meanders' belt the channel's distance is read, in
/// the channel's widest widths and metres more: past every bank's band
/// and every wall's ease, where nothing the channel's distance gates is
/// still open.
const CHANNEL_REACH_WIDTHS: f64 = 3.0;
const CHANNEL_REACH_M: f64 = 25.0;

/// Over how much of a segment's offset about nought its size is rounded,
/// metres: the channel's distance reads half this under the offset's
/// size away from its centre.
const CHANNEL_ROUND_M: f64 = 0.3;

/// What [`Blend`] says, normalised, each number with its gradient.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RiverHere {
    /// Distance from the centrelines, metres.
    pub distance: D,
    /// The surface, metres.
    pub surface: D,
    /// Discharge, m3/s.
    pub discharge: D,
    /// How deep the valley is cut under the natural ground, metres.
    pub cut: D,
    /// Strahler order, blended.
    pub order: D,
    /// Downstream, a unit `(x, z)`.
    pub flow: [f64; 2],
    /// The segments' directions downstream averaged by their wider
    /// weights and not made a unit, and its components' gradients: as
    /// long as [`flow`](Self::flow) along one reach, shorter where reaches
    /// running different ways meet. What the grain's narrowing reads: the
    /// unit vector turns through a right angle in a few centimetres where
    /// two reaches' directions nearly cancel, which swung a valley's
    /// half-width by tens of metres there and stood a wall metres high in
    /// centimetres (ticket 0005, the review of step 3b).
    pub course: [D; 2],
    /// The nearest segment's distance, and its order and kind.
    pub nearest: D,
    pub nearest_order: u8,
    pub kind: WaterKind,
    /// The distance from the meandering channel's centreline, metres:
    /// the offset from the wave across the line, over the wave's own
    /// slope's secant, so a channel crossing the line at a slant is its
    /// width wide across itself.
    pub channel: D,
    /// Which side of the channel's centreline the point is, `1` the side a
    /// positive signed distance lies on.
    pub channel_side: f64,
    /// The meander's amplitude, metres: how far the channel swings either
    /// side of the line, which the valley's floor makes room for.
    pub amplitude: D,
    /// The channel's width over its mean here ([`shape::riffle`]).
    pub riffle: D,
    /// How much of a full bend the channel is in, signed by which way it
    /// swings (positive towards the positive side): the inside of a bend,
    /// where its gravel bar lies, is the other side.
    pub bend: f64,
}

impl Blend {
    /// The blend normalised; `None` when no segment was in reach. The
    /// surface is a soft minimum of the segments' `readings`, not their
    /// weighted mean: where a steep tributary meets its main stem at a
    /// narrow angle, the tributary's surface a few metres off the main
    /// stem's centreline stands metres over the main stem's, and the mean,
    /// as the tributary's weight grew towards the confluence, lifted the
    /// main stem's bed by up to 0.4 m on its way down. A segment reading
    /// over the mean weighs less the higher it reads, so the lower water
    /// sets the level where two meet, and the higher one's last metres ease
    /// down to it.
    #[allow(
        clippy::too_many_lines,
        reason = "the means, the spread, the soft minimum and the flow, each with its gradient"
    )]
    pub fn finish(&self, readings: &[SurfaceReading], params: &RiverParams) -> Option<RiverHere> {
        let kind = self.near_kind?;
        if self.w <= 0.0 {
            return None;
        }
        let inv = 1.0 / self.w;
        let distance = self.wd * inv;
        let wide = 1.0 / self.v;
        let normalised = |sum: f64, g: [f64; 2]| {
            let value = sum * wide;
            D::new(
                value,
                [
                    (g[0] - value * self.gv[0]) * wide,
                    (g[1] - value * self.gv[1]) * wide,
                ],
            )
        };
        let mean = normalised(self.ws, self.gws);
        // The spread of the readings about the mean, which sets the soft
        // minimum's scale: where two reaches meet a metre apart in level the
        // higher weighs less by a fraction of a metre over the mean, and
        // where a reach steepens round a bend and its segments read tens of
        // metres apart, by tens of metres, so the surface turns from one to
        // the other over as many metres as the mean did and stands no
        // cliff where the weights cross (a fixed scale did, 50 in one).
        let (mut var, mut gvar) = (0.0, [0.0; 2]);
        for r in readings {
            let delta = r.s - mean.v;
            let gdelta = [r.gs[0] - mean.g[0], r.gs[1] - mean.g[1]];
            var += r.v * delta * delta;
            gvar[0] += r.gv[0] * delta * delta + 2.0 * r.v * delta * gdelta[0];
            gvar[1] += r.gv[1] * delta * delta + 2.0 * r.v * delta * gdelta[1];
        }
        let var = D::new(
            var * wide,
            [
                (gvar[0] - var * wide * self.gv[0]) * wide,
                (gvar[1] - var * wide * self.gv[1]) * wide,
            ],
        );
        let tau = (var + SOFT_MIN_M * SOFT_MIN_M).sqrt();
        let (mut num, mut gnum, mut den, mut gden) = (0.0, [0.0; 2], 0.0, [0.0; 2]);
        for r in readings {
            let delta = D::new(r.s - mean.v, [r.gs[0] - mean.g[0], r.gs[1] - mean.g[1]]);
            let q = soft(delta, tau * SOFT_MIN_RAMP) / tau;
            let base = q * q + 1.0;
            let psi = D::c(1.0) / (base * base);
            let weight = D::new(r.v, r.gv) * psi;
            num += weight.v * r.s;
            gnum[0] += weight.g[0] * r.s + weight.v * r.gs[0];
            gnum[1] += weight.g[1] * r.s + weight.v * r.gs[1];
            den += weight.v;
            gden[0] += weight.g[0];
            gden[1] += weight.g[1];
        }
        let surface = if den > 0.0 {
            let value = num / den;
            D::new(
                value,
                [
                    (gnum[0] - value * gden[0]) / den,
                    (gnum[1] - value * gden[1]) / den,
                ],
            )
        } else {
            mean
        };
        let flow_len = (self.wf[0] * self.wf[0] + self.wf[1] * self.wf[1]).sqrt();
        let flow = if flow_len > 0.0 {
            [self.wf[0] / flow_len, self.wf[1] / flow_len]
        } else {
            [0.0, 0.0]
        };
        let course = [
            normalised(self.wf[0], self.gwf[0]),
            normalised(self.wf[1], self.gwf[1]),
        ];
        let nearest = self.near2.sqrt();
        // The meanders at the point. Far out on the valley's floor and
        // walls the channel's distance only gates what is shut there (its
        // banks, the floor's rise from it, the walls' foot), so it is the
        // distance from the meanders' belt and no wave is read: a third of
        // a valley's points, at a tenth of a microsecond each.
        let amplitude = normalised(self.wa, self.gwa).at_least(0.0);
        let discharge = normalised(self.wq, self.gwq);
        let line = D::new(
            distance,
            [
                (self.gwd[0] - distance * self.gw[0]) * inv,
                (self.gwd[1] - distance * self.gw[1]) * inv,
            ],
        );
        let widest = (params.width(discharge.v) * (1.0 + params.riffle_share)).max(4.0);
        let (channel, signed, riffle, bend) =
            if line.v > amplitude.v + CHANNEL_REACH_WIDTHS * widest + CHANNEL_REACH_M {
                let signed: f64 = readings.iter().map(|r| r.v * r.line).sum();
                (line - amplitude, signed, D::c(1.0), 0.0)
            } else {
                // The meanders at the point: the wave at the blended phase, its
                // offset and its slope along the line, each with its gradient.
                let phase = normalised(self.wp, self.gwp);
                let rate = normalised(self.wpr, self.gwpr);
                let amplitude_rate = normalised(self.war, self.gwar);
                let share = normalised(self.wg, self.gwg).at_least(0.0).at_most(1.0);
                let wave = shape::wave(phase.v);
                let w = D::new(
                    wave.offset,
                    [wave.d_offset * phase.g[0], wave.d_offset * phase.g[1]],
                );
                let dw = D::new(
                    wave.d_offset,
                    [wave.dd_offset * phase.g[0], wave.dd_offset * phase.g[1]],
                );
                let offset = amplitude * w;
                let slope = amplitude_rate * w + amplitude * dw * rate;
                // The distance from the meandering centreline: each segment's line
                // less the offset, by its size, so two reaches' lines at a
                // confluence never cancel into a channel between them; over the
                // wave's secant, so a channel crossing the line at a slant is its
                // width across itself.
                let (mut sum, mut g, mut signed) = (0.0, [0.0; 2], 0.0);
                for r in readings {
                    let c = r.line - offset.v;
                    let gc = [r.normal[0] - offset.g[0], r.normal[1] - offset.g[1]];
                    // Its size, rounded over [`CHANNEL_ROUND_M`] about nought:
                    // near a vertex, where two segments' frames differ, one's
                    // offset crosses nought off the channel's centre, and its
                    // corner creased the bank.
                    let (size, slope) = if c.abs() < CHANNEL_ROUND_M {
                        (c * c / (2.0 * CHANNEL_ROUND_M), c / CHANNEL_ROUND_M)
                    } else if c < 0.0 {
                        (-c - 0.5 * CHANNEL_ROUND_M, -1.0)
                    } else {
                        (c - 0.5 * CHANNEL_ROUND_M, 1.0)
                    };
                    sum += r.v * size;
                    g[0] += r.gv[0] * size + r.v * gc[0] * slope;
                    g[1] += r.gv[1] * size + r.v * gc[1] * slope;
                    signed += r.v * c;
                }
                let size = normalised(sum, g);
                let secant = (slope * slope + 1.0).sqrt();
                let channel = size / secant;
                let pools = shape::riffle_share(share.v);
                let riffle = D::new(
                    1.0 + params.riffle_share * pools * wave.riffle,
                    [
                        params.riffle_share
                            * ((1.0 - shape::RIFFLE_LEAST) * share.g[0] * wave.riffle
                                + pools * wave.d_riffle * phase.g[0]),
                        params.riffle_share
                            * ((1.0 - shape::RIFFLE_LEAST) * share.g[1] * wave.riffle
                                + pools * wave.d_riffle * phase.g[1]),
                    ],
                );
                (channel, signed, riffle, share.v * wave.bend)
            };
        Some(RiverHere {
            channel,
            channel_side: if signed < 0.0 { -1.0 } else { 1.0 },
            amplitude,
            riffle,
            bend,
            distance: D::new(
                distance,
                [
                    (self.gwd[0] - distance * self.gw[0]) * inv,
                    (self.gwd[1] - distance * self.gw[1]) * inv,
                ],
            ),
            surface,
            discharge: normalised(self.wq, self.gwq),
            cut: normalised(self.wc, self.gwc),
            order: normalised(self.wo, self.gwo),
            flow,
            course,
            nearest: D::new(
                nearest,
                if nearest > 0.0 {
                    [self.near_r[0] / nearest, self.near_r[1] / nearest]
                } else {
                    [0.0, 0.0]
                },
            ),
            nearest_order: self.near_order,
            kind,
        })
    }
}

/// A bucket's run of segments in [`SegmentIndex::entries`], and the widest
/// any valley of them can reach from its centreline, metres: a point
/// farther than that from every segment of every run that reaches it is not
/// carved, and the carve skips the blend there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run {
    pub start: u32,
    pub len: u32,
    pub valley_m: f32,
}

impl SegmentIndex {
    #[allow(
        clippy::too_many_lines,
        reason = "the segments, their buckets and each bucket's reach, in order"
    )]
    pub fn new(rivers: &[River], params: &RiverParams) -> Self {
        let mut segments = Vec::new();
        for river in rivers {
            let last = river.vertices.len().saturating_sub(1);
            // Each segment's slope, surface over length, downstream.
            let slopes: Vec<f64> = river
                .vertices
                .windows(2)
                .map(|pair| {
                    let (dx, dz) = (pair[1].x - pair[0].x, pair[1].z - pair[0].z);
                    let length = (dx * dx + dz * dz).sqrt();
                    if length > 0.0 {
                        (f64::from(pair[1].surface_m) - f64::from(pair[0].surface_m)) / length
                    } else {
                        0.0
                    }
                })
                .collect();
            for (i, pair) in river.vertices.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let kind = if a.fall {
                    WaterKind::Fall
                } else if i + 1 == last && river.end == RiverEnd::Sea {
                    WaterKind::Mouth
                } else {
                    WaterKind::Channel
                };
                let d = [b.x - a.x, b.z - a.z];
                let length2 = d[0] * d[0] + d[1] * d[1];
                let length = length2.sqrt();
                segments.push(Segment {
                    a: [a.x, a.z],
                    b: [b.x, b.z],
                    d,
                    inv_length2: if length2 > 0.0 { 1.0 / length2 } else { 0.0 },
                    normal: if length > 0.0 {
                        [-d[1] / length, d[0] / length]
                    } else {
                        [0.0, 0.0]
                    },
                    surface: [a.surface_m, b.surface_m],
                    run_on: [
                        if i > 0 {
                            slopes[i - 1].max(slopes[i])
                        } else {
                            0.0
                        },
                        slopes.get(i + 1).map_or(0.0, |&next| next.max(slopes[i])),
                    ],
                    discharge: [a.discharge_m3s, b.discharge_m3s],
                    cut: [a.cut_m, b.cut_m],
                    amplitude: [a.meander_m, b.meander_m],
                    share: [a, b].map(|v| {
                        single(shape::bend_share(
                            params,
                            f64::from(v.meander_m),
                            f64::from(v.discharge_m3s),
                        ))
                    }),
                    phase: [a.phase, b.phase],
                    order: river.order,
                    kind,
                });
            }
        }
        let (side, reach) = (params.bucket_m, params.reach_m);
        let mut pairs: Vec<([i32; 2], u32)> = Vec::new();
        for (s, seg) in segments.iter().enumerate() {
            let (x0, x1) = (
                seg.a[0].min(seg.b[0]) - reach,
                seg.a[0].max(seg.b[0]) + reach,
            );
            let (z0, z1) = (
                seg.a[1].min(seg.b[1]) - reach,
                seg.a[1].max(seg.b[1]) + reach,
            );
            for j in bucket(z0, side)..=bucket(z1, side) {
                for i in bucket(x0, side)..=bucket(x1, side) {
                    pairs.push(([i, j], local(s)));
                }
            }
        }
        pairs.sort_unstable();
        let reaches: Vec<f32> = segments
            .iter()
            .map(|seg| {
                single(valley_bound(
                    params,
                    f64::from(seg.discharge[0].max(seg.discharge[1])),
                    f64::from(seg.cut[0].max(seg.cut[1])),
                    f64::from(seg.amplitude[0].max(seg.amplitude[1])),
                ))
            })
            .collect();
        let mut buckets = NodeMap::default();
        let mut entries = Vec::with_capacity(pairs.len());
        let mut start = 0;
        while start < pairs.len() {
            let key = pairs[start].0;
            let mut end = start;
            let mut valley_m = 0.0_f32;
            while end < pairs.len() && pairs[end].0 == key {
                entries.push(pairs[end].1);
                valley_m = valley_m.max(reaches[pairs[end].1 as usize]);
                end += 1;
            }
            buckets.insert(
                key,
                Run {
                    start: local(start),
                    len: local(end - start),
                    valley_m,
                },
            );
            start = end;
        }
        Self {
            segments,
            buckets,
            entries,
        }
    }

    /// The run of segments of bucket `key`, if any segment reaches it.
    pub fn run(&self, key: [i32; 2]) -> Option<Run> {
        self.buckets.get(&key).copied()
    }

    /// The nearest segment's water within reach of `(x, z)`.
    pub fn nearest(&self, params: &RiverParams, x: f64, z: f64) -> Option<Water> {
        let key = [bucket(x, params.bucket_m), bucket(z, params.bucket_m)];
        self.nearest_in(self.run(key)?, params, x, z)
    }

    /// The nearest segment's water within reach of `(x, z)`, of those in a
    /// bucket's run.
    pub fn nearest_in(&self, run: Run, params: &RiverParams, x: f64, z: f64) -> Option<Water> {
        let mut best: Option<(f64, u32, f64)> = None;
        for &s in &self.entries[run.start as usize..(run.start + run.len) as usize] {
            let seg = &self.segments[s as usize];
            let (dx, dz) = (seg.b[0] - seg.a[0], seg.b[1] - seg.a[1]);
            let length2 = dx * dx + dz * dz;
            let u = if length2 > 0.0 {
                (((x - seg.a[0]) * dx + (z - seg.a[1]) * dz) / length2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, pz) = (seg.a[0] + dx * u - x, seg.a[1] + dz * u - z);
            let d2 = px * px + pz * pz;
            if best.is_none_or(|(held, _, _)| d2 < held) {
                best = Some((d2, s, u));
            }
        }
        let (d2, s, u) = best?;
        let distance = d2.sqrt();
        if distance > params.reach_m {
            return None;
        }
        let seg = &self.segments[s as usize];
        let (dx, dz) = (seg.b[0] - seg.a[0], seg.b[1] - seg.a[1]);
        let length = (dx * dx + dz * dz).sqrt().max(f64::MIN_POSITIVE);
        let flow = [dx / length, dz / length];
        let side = (x - seg.a[0]) * -flow[1] + (z - seg.a[1]) * flow[0];
        let lerp =
            |pair: [f32; 2]| f64::from(pair[0]) + (f64::from(pair[1]) - f64::from(pair[0])) * u;
        let surface = lerp(seg.surface);
        let discharge = lerp(seg.discharge);
        let depth = params.depth(discharge);
        Some(Water {
            surface_m: single(surface),
            bed_m: single(surface - depth),
            depth_m: single(depth),
            width_m: single(params.width(discharge)),
            signed_distance_m: single(if side < 0.0 { -distance } else { distance }),
            flow_dir: [single(flow[0]), single(flow[1])],
            speed_mps: single(params.speed(discharge)),
            discharge_m3s: single(discharge),
            order: seg.order,
            kind: seg.kind,
        })
    }

    /// Adds every segment of a bucket's run within reach of `(x, z)` to
    /// `blend`. The distance a segment adds is to the segment itself: the
    /// weighted sum is exact on the outside of a bend, where every segment
    /// near is nearest at the vertex they share, and smooth on its inside,
    /// where the nearest flips between two; a farther segment of the reach
    /// lifts it by a few hundredths of the distance, a ripple of a few
    /// decimetres at 100 m. Distances to the segments' lines, tried first,
    /// dipped by a few metres in the wedge outside each bend and drew a
    /// groove down the valley wall from every vertex.
    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_lines,
        reason = "the geometry's own letters, and every sum the blend keeps with its gradient"
    )]
    pub fn blend(
        &self,
        found: &Found,
        reach: f64,
        (x, z): (f64, f64),
        near: Near,
        blend: &mut Blend,
        readings: &mut Vec<SurfaceReading>,
    ) {
        let reach2 = reach * reach;
        let e2 = EPSILON_M * EPSILON_M;
        let s2 = SOFT_M * SOFT_M;
        let m2 = near.c2 + s2;
        {
            let seg = &self.segments[found.segment as usize];
            let [dx, dz] = seg.d;
            let (ax, az) = (x - seg.a[0], z - seg.a[1]);
            let Reading {
                t,
                r,
                c2,
                e2: ce2,
                q,
            } = found.reading;
            let u = t.clamp(0.0, 1.0);
            let ratio = (ce2 + s2) / m2;
            if ce2 >= reach2 || ratio >= CUTOFF.1 {
                return;
            }
            // The weight and its gradient, `-B (4A/R^2 + 8A^2/(c^2+e^2)) q`,
            // `q` being half the gradient of the weighed distance squared.
            let a = 1.0 - ce2 / reach2;
            // Times the cut-off by the distance over the nearest's, and its
            // gradient through both distances.
            let (f, df) = cutoff(ratio);
            let weigh = |soft2: f64| {
                let t4 = ce2 + soft2;
                let t4_2 = t4 * t4;
                let b = 1.0 / (t4_2 * t4_2);
                let w0 = a * a * b;
                let k = -b * (4.0 * a / reach2 + 8.0 * a * a / t4);
                let k_ratio = df * w0 * 2.0 / m2;
                (
                    w0 * f,
                    [
                        k * q[0] * f + k_ratio * (q[0] - ratio * near.q[0]),
                        k * q[1] * f + k_ratio * (q[1] - ratio * near.q[1]),
                    ],
                )
            };
            let (w, gw) = weigh(e2);
            let (v, gv) = weigh(s2);
            // The distance and its gradient.
            let d = c2.sqrt();
            let gd = if d > 0.0 {
                [r[0] / d, r[1] / d]
            } else {
                [0.0, 0.0]
            };
            // The surface at the point's projection on the segment's line,
            // and its slope. Past the segment's ends it runs on at the
            // gentler of its slope and its neighbour's (and level past a
            // reach's ends), the weights falling away as it goes: read at the end
            // itself, the surface stood still over the wedge on the outside
            // of each bend, and the ground along the river kinked there; run
            // on at its own slope, the blend of two segments rose where a
            // river steepens. At the gentler slope the upstream segment's
            // surface is nowhere under the downstream one's about the vertex
            // they share, so the blend, as its weight passes from the one to
            // the other, eases the change of slope over a few metres and
            // never rises.
            let along = t;
            let (s0, s1) = (f64::from(seg.surface[0]), f64::from(seg.surface[1]));
            let length = 1.0 / seg.inv_length2.max(f64::MIN_POSITIVE).sqrt();
            let (surface, slope) = if along < 0.0 {
                (s0 + seg.run_on[0] * along * length, seg.run_on[0] / length)
            } else if along > 1.0 {
                (
                    s1 + seg.run_on[1] * (along - 1.0) * length,
                    seg.run_on[1] / length,
                )
            } else {
                (s0 + (s1 - s0) * along, (s1 - s0) * seg.inv_length2)
            };
            let gs = [slope * dx, slope * dz];
            blend.w += w;
            blend.gw[0] += gw[0];
            blend.gw[1] += gw[1];
            blend.wd += w * d;
            blend.gwd[0] += gw[0] * d + w * gd[0];
            blend.gwd[1] += gw[1] * d + w * gd[1];
            blend.v += v;
            blend.gv[0] += gv[0];
            blend.gv[1] += gv[1];
            blend.ws += v * surface;
            blend.gws[0] += gv[0] * surface + v * gs[0];
            blend.gws[1] += gv[1] * surface + v * gs[1];
            readings.push(SurfaceReading {
                v,
                gv,
                s: surface,
                gs,
                line: ax * seg.normal[0] + az * seg.normal[1],
                normal: seg.normal,
            });
            // What runs along the segment between its ends, and is held
            // past them: its gradient is the change over the segment, along
            // it, inside, and nothing outside.
            let inside = if t > 0.0 && t < 1.0 {
                seg.inv_length2
            } else {
                0.0
            };
            let along = |pair: [f32; 2], sum: &mut f64, g: &mut [f64; 2]| {
                let (p0, p1) = (f64::from(pair[0]), f64::from(pair[1]));
                let value = p0 + (p1 - p0) * u;
                let change = (p1 - p0) * inside;
                *sum += v * value;
                g[0] += gv[0] * value + v * change * dx;
                g[1] += gv[1] * value + v * change * dz;
            };
            along(seg.discharge, &mut blend.wq, &mut blend.gwq);
            along(seg.cut, &mut blend.wc, &mut blend.gwc);
            let order = f64::from(seg.order);
            blend.wo += v * order;
            blend.gwo[0] += gv[0] * order;
            blend.gwo[1] += gv[1] * order;
            // The meanders' numbers at the point's projection on the
            // segment's line, run on past its ends as they run along it:
            // held there, the phase stood still past one end and moved
            // once inside, and the channel's bank creased where the point's
            // projection crossed the end.
            let grad_t = [dx * seg.inv_length2, dz * seg.inv_length2];
            let (a0, a1) = (f64::from(seg.amplitude[0]), f64::from(seg.amplitude[1]));
            let (p0, p1) = (f64::from(seg.phase[0]), f64::from(seg.phase[1]));
            let (g0, g1) = (f64::from(seg.share[0]), f64::from(seg.share[1]));
            let (da, dp, dg) = (a1 - a0, p1 - p0, g1 - g0);
            let amplitude = a0 + da * t;
            let share = g0 + dg * t;
            let phase = p0 + dp * t;
            let (rate, amplitude_rate) = (dp / length, da / length);
            let add = |sum: &mut f64, g: &mut [f64; 2], value: f64, change: f64| {
                *sum += v * value;
                g[0] += gv[0] * value + v * change * grad_t[0];
                g[1] += gv[1] * value + v * change * grad_t[1];
            };
            add(&mut blend.wp, &mut blend.gwp, phase, dp);
            add(&mut blend.wpr, &mut blend.gwpr, rate, 0.0);
            add(&mut blend.wa, &mut blend.gwa, amplitude, da);
            add(&mut blend.war, &mut blend.gwar, amplitude_rate, 0.0);
            add(&mut blend.wg, &mut blend.gwg, share, dg);
            // Downstream is the normal turned back a quarter.
            let down = [seg.normal[1], -seg.normal[0]];
            for (i, component) in down.iter().enumerate() {
                blend.wf[i] += v * component;
                blend.gwf[i][0] += gv[0] * component;
                blend.gwf[i][1] += gv[1] * component;
            }
            if blend.near_kind.is_none() || c2 < blend.near2 {
                blend.near2 = c2;
                blend.near_r = r;
                blend.near_order = seg.order;
                blend.near_kind = Some(seg.kind);
            }
        }
    }

    /// Reads every segment of a bucket's run that the blend may keep from
    /// `(x, z)` into `found` (tagged `network`), and the nearest of them
    /// into `near`: what [`blend`](Self::blend) measures every other
    /// segment against. One pass: a segment is read once, against the bound
    /// the nearest found so far allows, which only shrinks, so `found`
    /// holds every segment the final bound keeps and some it does not.
    pub fn scan(
        &self,
        run: Run,
        (x, z): (f64, f64),
        reach: f64,
        near: &mut Near,
        network: u16,
        found: &mut Vec<Found>,
    ) {
        let reach2 = reach * reach;
        let s2 = SOFT_M * SOFT_M;
        for &s in &self.entries[run.start as usize..(run.start + run.len) as usize] {
            // A segment skipped here is at least as far as the bound, which
            // is never under the nearest true distance found: the least of
            // those is still the least of all.
            let bound = reach2.min(CUTOFF.1 * (near.c2 + s2) - s2);
            let Some(reading) = self.segments[s as usize].read(x, z, bound) else {
                continue;
            };
            near.min_c2 = near.min_c2.min(reading.c2);
            if reading.e2 < near.c2 {
                near.c2 = reading.e2;
                near.q = reading.q;
            }
            found.push(Found {
                network,
                segment: s,
                reading,
            });
        }
    }

    /// Segments held and bucket entries, for the measures.
    pub fn size(&self) -> (usize, usize) {
        (self.segments.len(), self.entries.len())
    }
}

/// How far over a lake's level the ground of a node that is not the lake's
/// is read, at least, metres: so the shore always falls between the lake's
/// own nodes and the rest, out at its outlet too (the ground falls away
/// past the outlet node, and read as it is the lake would run on down the
/// valley below), and the lake's carve, which ends 10 m of coarse height
/// over the level, ends inside the cells with one of the lake's nodes at a
/// corner.
const SHORE_LIFT_M: f64 = 12.0;

/// The least slope a lake's distance inside its shore is read over, rise
/// over run ([`LakeHere::slope`]).
const SLOPE_FLOOR: f64 = 0.01;

/// The lakes' nodes with the coarse ground of every node round them, so a
/// point's lake and its shore come from the four nodes about it.
#[derive(Debug)]
pub(crate) struct Shore {
    nodes: NodeMap<(u32, f32)>,
}

/// One lake at a point, from the four nodes about it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LakeHere {
    /// Which lake.
    pub id: u32,
    /// Its level, metres.
    pub level: f64,
    /// The coarse ground under the point, eased between the four nodes so
    /// that it and its gradient are continuous across a cell's edge, and
    /// that gradient.
    pub ground: f64,
    pub gradient: (f64, f64),
    /// The lake's [`Lake::detail_m`].
    pub detail: f64,
}

impl LakeHere {
    /// How far the level stands over the ground, metres: positive in the
    /// lake.
    pub fn depth(&self) -> f64 {
        self.level - self.ground
    }

    /// How far inside the coarse shore the point lies, metres (negative
    /// outside): the depth over the eased ground's slope, an estimate.
    pub fn inside_m(&self) -> f64 {
        self.depth() / self.slope()
    }

    /// The eased coarse ground's slope, held off nought by a hundredth
    /// softly: the eased ground is flat at every node, and a hard floor
    /// creased the distance inside the shore, and with it a lake's bed,
    /// where the slope crossed it (ticket 0005, the review of step 3b).
    pub fn slope(&self) -> f64 {
        (self.gradient.0 * self.gradient.0
            + self.gradient.1 * self.gradient.1
            + SLOPE_FLOOR * SLOPE_FLOOR)
            .sqrt()
    }
}

impl Shore {
    pub fn new(domain: &Domain, routed: &Routed) -> Self {
        let grid = Grid::new(&domain.nodes);
        let mut nodes = NodeMap::default();
        for lake in &routed.lakes {
            for &m in &lake.nodes {
                let at = domain.nodes[m as usize];
                for d in NEIGHBOURS.iter().chain(&[[0, 0]]) {
                    let n = [at[0] + d[0], at[1] + d[1]];
                    let entry = grid.at(n).map_or((NONE, f32::MAX), |k| {
                        (
                            routed.lake[k as usize],
                            single(domain.field[k as usize].height),
                        )
                    });
                    nodes.insert(n, entry);
                }
            }
        }
        Self { nodes }
    }

    /// Whether node `n` is under a lake.
    pub fn has_lake(&self, n: [i32; 2]) -> bool {
        self.nodes.get(&n).is_some_and(|&(id, _)| id != NONE)
    }

    /// The lake whose level stands deepest over the ground at `(x, z)`, of
    /// those with a node at a corner of the point's cell; `None` with
    /// none, or where every one ends `SHORE_LIFT_M` of coarse height under
    /// the point.
    pub fn lake_here(&self, lakes: &[Lake], x: f64, z: f64) -> Option<LakeHere> {
        if self.nodes.is_empty() {
            return None;
        }
        let (i0, j0) = (node_of(x), node_of(z));
        let corners = [[i0, j0], [i0 + 1, j0], [i0, j0 + 1], [i0 + 1, j0 + 1]];
        let entries = corners.map(|n| self.nodes.get(&n).copied());
        let (across, down) = (x / LATTICE_M - f64::from(i0), z / LATTICE_M - f64::from(j0));
        // Smoothstep weights: the eased ground's gradient across a cell's
        // edge is zero on both sides, so the shore has no corner there.
        let sx = across * across * (3.0 - 2.0 * across);
        let sz = down * down * (3.0 - 2.0 * down);
        let dsx = 6.0 * across * (1.0 - across) / LATTICE_M;
        let dsz = 6.0 * down * (1.0 - down) / LATTICE_M;
        let mut best: Option<LakeHere> = None;
        for &(id, _) in entries.iter().flatten() {
            if id == NONE || best.is_some_and(|held| held.id == id) {
                continue;
            }
            let level = f64::from(lakes[id as usize].level_m);
            let corner = entries.map(|entry| match entry {
                Some((owner, ground)) if owner == id => f64::from(ground),
                Some((_, ground)) if ground < f32::MAX => {
                    f64::from(ground).max(level + SHORE_LIFT_M)
                }
                _ => level + 50.0,
            });
            let ground = (corner[0] * (1.0 - sx) + corner[1] * sx) * (1.0 - sz)
                + (corner[2] * (1.0 - sx) + corner[3] * sx) * sz;
            let here = LakeHere {
                id,
                level,
                detail: f64::from(lakes[id as usize].detail_m),
                ground,
                gradient: (
                    dsx * ((corner[1] - corner[0]) * (1.0 - sz) + (corner[3] - corner[2]) * sz),
                    dsz * ((corner[2] - corner[0]) * (1.0 - sx) + (corner[3] - corner[1]) * sx),
                ),
            };
            if here.depth() > -SHORE_LIFT_M
                && best.is_none_or(|held| (here.depth(), id) > (held.depth(), held.id))
            {
                best = Some(here);
            }
        }
        best
    }

    /// The lake water at `(x, z)`, if the point is under a lake's level.
    pub fn lake_at(&self, lakes: &[Lake], x: f64, z: f64) -> Option<Water> {
        let here = self.lake_here(lakes, x, z)?;
        let depth = here.depth();
        if depth <= 0.0 {
            return None;
        }
        Some(lake_water(&lakes[here.id as usize], &here))
    }
}

/// The record of a point in or by a lake: its distance from the shore is
/// the depth over the eased ground's slope, an estimate.
pub(crate) fn lake_water(lake: &Lake, here: &LakeHere) -> Water {
    let depth = here.depth();
    let slope = (here.gradient.0 * here.gradient.0 + here.gradient.1 * here.gradient.1)
        .sqrt()
        .max(1e-3);
    let side = (real(lake.nodes.len()) * LATTICE_M * LATTICE_M).sqrt();
    Water {
        surface_m: lake.level_m,
        bed_m: single(here.ground),
        depth_m: single(depth.max(0.0)),
        width_m: single(side),
        signed_distance_m: single((-depth / slope).clamp(-side, side)),
        flow_dir: [0.0, 0.0],
        speed_mps: 0.0,
        discharge_m3s: lake.discharge_m3s,
        order: 0,
        kind: WaterKind::Lake,
    }
}
