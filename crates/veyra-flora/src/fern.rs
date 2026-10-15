//! Ferns: a rosette of fronds arching out of one crown at the ground.
//!
//! Each frond is one leaf card, a whole painted frond, bent along the arch
//! of its rachis: the card's rows follow the curve, so a frond bows over
//! instead of reading as a flat tile. Below the card the bare stipe is a
//! tube from the crown to the card's stem, grown as a skeleton stem so it
//! goes through the tube mesher like any branch; the painted rachis carries
//! on from there. A young fern still has fiddleheads coiled at its heart; an
//! old one has a couple of fronds snapped and lying over.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::{DQuat, DVec3};

use crate::TreeRecipe;
use crate::bark::{self, BarkDetail};
use crate::crown::CrownField;
use crate::foliage::{Card, STEM};
use crate::levels::{
    self, Built, Piece, Pivot, Pivots, SMALL_BUDGET, Shade, assemble, card_normal, occupancy,
};
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::{Age, Form};

/// Where the painted rachis ends, in card `v`: the frond's tip.
const TIP: f64 = 0.06;

/// A frond card's width over its length. The painted frond is a broad lance
/// on a square card; laid square, a frond would be as wide as it is long.
/// Narrowed, it is the two to one a fern frond is, with its pinnae swept
/// further forward. UVs still cover the whole card.
const NARROW: f64 = 0.6;

/// One frond as grown: its whole curve, the stipe's nodes in the skeleton,
/// and what it became.
struct Frond {
    /// The centreline from the crown to the tip.
    curve: Curve,
    /// Across the frond's plane, horizontal.
    across: DVec3,
    /// The skeleton nodes of the stipe, or of the whole coil for a
    /// fiddlehead.
    nodes: Vec<usize>,
    /// Arc length where the card's bottom edge sits; the stem is a little
    /// above it.
    start: f64,
    /// Still coiled: a rachis and no card.
    fiddlehead: bool,
}

pub(crate) fn build(recipe: &TreeRecipe, variant: u32) -> Built {
    let form = Form::new(recipe, variant);
    let (skeleton, fronds) = grow(recipe, &form);
    let branches = skeleton.branches();
    let height = form.height;
    let pivots = pivots(
        &skeleton,
        &fronds,
        branches.len(),
        &form,
        &mut form.rng("pivots"),
    );

    let cards: Vec<(usize, FrondCard)> = fronds
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.fiddlehead)
        .map(|(k, f)| (k, FrondCard::new(f)))
        .collect();
    let proxies: Vec<Card> = cards
        .iter()
        .flat_map(|(k, c)| c.proxies(&fronds[*k]))
        .collect();
    let field = CrownField::new(
        &proxies,
        DVec3::new(0.0, height * 0.4, 0.0),
        false,
        form.clumps,
    );
    let occupancy = occupancy(&skeleton, &proxies, &form);
    let card_offsets = stem_offsets(&skeleton, &fronds, &cards);

    let tree = bark::Tree {
        recipe,
        form: &form,
        skeleton: &skeleton,
        branches: &branches,
        pivots: &pivots.of_branch,
        tile: levels::bark_tile(recipe),
        root_flare: false,
        cap_base: false,
        splinter: 0.0,
    };
    let shade = Shade {
        pivots: &pivots,
        field: &field,
        occupancy: &occupancy,
        height,
    };
    let mut levels = Vec::with_capacity(2);
    let mut rings = Vec::with_capacity(2);
    // Rows along a card, columns across it, stipe sides, and stipe rings
    // apart in segments: the first option that fits the budget is used.
    let near = [
        (12, 5, 4, 1.0),
        (10, 5, 4, 1.0),
        (8, 3, 3, 1.5),
        (6, 3, 3, 2.0),
    ];
    let far = [
        (3, 2, 3, 3.0),
        (2, 2, 3, 5.0),
        (1, 2, 3, 8.0),
        (1, 2, 3, 20.0),
    ];
    for (level, options) in [near, far].into_iter().enumerate() {
        let mut built = None;
        for (rows, columns, sides, apart) in options {
            let detail = BarkDetail {
                max_sides: sides,
                turn: if level == 0 { 0.2 } else { 0.9 },
                spacing: 1.0,
                spacing_max: form.segment * apart * 2.0,
                spacing_min: form.segment * apart,
                min_radius: 0.0,
                fine_base: false,
            };
            let (stipes, level_rings) = Piece::from_bark(bark::mesh(&tree, &detail));
            let mut leaves = Piece::default();
            for (k, card) in &cards {
                // Frond k is branch and pivot k + 1.
                let pivot = pivots.of_branch[k + 1];
                card.mesh(&fronds[*k], &mut leaves, &field, pivot, columns, rows);
            }
            let triangles = stipes.mesh.triangle_count() + leaves.mesh.triangle_count();
            let fits = triangles <= SMALL_BUDGET[level];
            built = Some((stipes, leaves, level_rings));
            if fits {
                break;
            }
        }
        let (stipes, leaves, level_rings) = built.expect("at least one option");
        rings.push(level_rings);
        levels.push([stipes.finish(&shade), leaves.finish(&shade)]);
    }
    Built {
        tree: assemble(recipe, variant, levels, Vec::new(), pivots),
        rings,
        field,
        pipe_ratios: Vec::new(),
        card_offsets,
    }
}

/// One pivot at the crown and one per frond, parented to it. A frond's
/// pivot spans the whole frond, card and all, not only the stipe the
/// skeleton holds, so its frequency is the frond's.
fn pivots(
    skeleton: &Skeleton,
    fronds: &[Frond],
    branches: usize,
    form: &Form,
    rng: &mut Rng,
) -> Pivots {
    let mut pivots = Pivots::for_branches(branches);
    pivots.of_branch[0] = pivots.push(
        &Pivot {
            pos: DVec3::ZERO,
            parent: None,
            axis: DVec3::Y,
            length: form.height,
            radius: skeleton.nodes[0].radius,
            stiffness: 1.0,
        },
        rng,
    );
    for (k, frond) in fronds.iter().enumerate() {
        let base = frond.curve.points[0];
        let (tip, _) = frond.curve.at(frond.curve.length());
        pivots.of_branch[k + 1] = pivots.push(
            &Pivot {
                pos: base,
                parent: Some(pivots.of_branch[0]),
                axis: (tip - base).normalize_or(DVec3::Y),
                length: frond.curve.length(),
                radius: skeleton.nodes[frond.nodes[0]].radius,
                stiffness: 0.6,
            },
            rng,
        );
    }
    pivots
}

/// The fronds: arching in their own vertical plane, faster towards the tip,
/// with a little wander, and none of them below the ground.
fn grow(recipe: &TreeRecipe, form: &Form) -> (Skeleton, Vec<Frond>) {
    let mut rng = form.rng("fronds");
    let height = form.height;
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.02, 0.0));
    skeleton.multi_stem = true;
    let turn = recipe.alien.phyllotaxis_deg.to_radians();
    let start = rng.range(0.0, core::f64::consts::TAU);
    let (fiddleheads, broken) = match form.age {
        Age::Young => (2, 0),
        Age::Mature => (u32::from(rng.chance(0.4)), 0),
        Age::Old => (0, 2),
    };
    let upright = if form.age == Age::Young { 0.7 } else { 1.0 };
    let bare = if form.age == Age::Young { 0.15 } else { 0.2 };
    let mut fronds = Vec::new();
    for k in 0..form.stems + fiddleheads {
        let fiddlehead = k >= form.stems;
        let azimuth = start + f64::from(k) * turn + rng.signed() * 0.25;
        let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let across = DVec3::Y.cross(out).normalize();
        let tilt = if fiddlehead {
            rng.range(0.05, 0.25)
        } else {
            rng.range(0.2, 0.6) * upright
        };
        let length = height
            * if fiddlehead {
                rng.range(0.25, 0.4)
            } else {
                rng.range(1.15, 1.55) * form.crown_radius / height / 0.9
            };
        let arch = form.droop * rng.range(1.1, 1.7);
        let foot = out * rng.range(0.0, 0.04 * height) + DVec3::new(0.0, -0.02, 0.0);
        let curve = arch_curve(
            &mut rng,
            &ArchSpec {
                base: foot,
                out,
                across,
                tilt,
                length,
                arch,
                step: form.segment,
                fiddlehead,
                snapped: k < broken,
            },
        );
        // The skeleton holds the stipe, up to a little past the card's
        // stem so the tube meets the painted rachis; a fiddlehead is all
        // stipe.
        let card_start = curve.length() * bare;
        let size = curve.length() - card_start;
        let stipe_end = if fiddlehead {
            curve.length()
        } else {
            card_start + size * (1.0 - STEM[1] + 0.03)
        };
        let mut nodes = Vec::new();
        let mut at = 0;
        for (p, &a) in curve.points.iter().zip(&curve.arc) {
            if a >= stipe_end {
                break;
            }
            at = skeleton.add(at, *p, across);
            nodes.push(at);
        }
        let (end, _) = curve.at(stipe_end);
        at = skeleton.add(at, end, across);
        nodes.push(at);
        fronds.push(Frond {
            curve,
            across,
            nodes,
            start: card_start,
            fiddlehead,
        });
    }
    // The stipe tapers from the crown; a fiddlehead to its coiled tip.
    let r0 = form.trunk_radius;
    skeleton.nodes[0].radius = r0 * 1.5;
    for frond in &fronds {
        let mut s = 0.0;
        for (i, &n) in frond.nodes.iter().enumerate() {
            if i > 0 {
                s += skeleton.segment_length(n);
            }
            let scale = if frond.fiddlehead { 0.8 } else { 1.0 };
            let taper = 1.0 - 0.65 * s / frond.curve.length().max(1e-6);
            skeleton.nodes[n].radius = r0 * scale * taper;
        }
    }
    skeleton.compute_ages();
    (skeleton, fronds)
}

/// How one frond arches.
struct ArchSpec {
    base: DVec3,
    out: DVec3,
    across: DVec3,
    tilt: f64,
    length: f64,
    arch: f64,
    step: f64,
    fiddlehead: bool,
    snapped: bool,
}

/// A frond's centreline: rising at its tilt, bending over faster towards
/// the tip, or, for a fiddlehead, straight and then coiling ever tighter.
fn arch_curve(rng: &mut Rng, spec: &ArchSpec) -> Curve {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = ((spec.length / spec.step).round() as usize).max(5);
    #[allow(clippy::cast_precision_loss)]
    let count = steps as f64;
    let seg = spec.length / count;
    let mut dir = (DVec3::Y * spec.tilt.cos() + spec.out * spec.tilt.sin()).normalize();
    let mut wander = rng.direction();
    let mut points = vec![spec.base];
    for i in 0..steps {
        #[allow(clippy::cast_precision_loss)]
        let t = i as f64 / count;
        let bend = if spec.fiddlehead {
            36.0 * (t - 0.45).max(0.0) / count
        } else {
            let snap = if spec.snapped && i == steps / 2 {
                1.3
            } else {
                0.0
            };
            spec.arch / count * (0.4 + 1.6 * t) + snap
        };
        wander = (wander + rng.direction() * 0.3).normalize();
        dir = (DQuat::from_axis_angle(spec.across, bend) * dir + wander * 0.04).normalize();
        let mut p = *points.last().expect("a base") + dir * seg;
        p.y = p.y.max(0.01);
        points.push(p);
    }
    Curve::new(points)
}

/// A polyline sampled by arc length.
struct Curve {
    points: Vec<DVec3>,
    arc: Vec<f64>,
}

impl Curve {
    fn new(points: Vec<DVec3>) -> Self {
        let mut arc = vec![0.0; points.len()];
        for k in 1..points.len() {
            arc[k] = arc[k - 1] + points[k].distance(points[k - 1]);
        }
        Self { points, arc }
    }

    fn length(&self) -> f64 {
        *self.arc.last().expect("a curve has points")
    }

    /// Position and tangent at arc length `s`.
    fn at(&self, s: f64) -> (DVec3, DVec3) {
        let s = s.clamp(0.0, self.length());
        let k = self
            .arc
            .partition_point(|&a| a <= s)
            .clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[k - 1], self.points[k]);
        let span = (self.arc[k] - self.arc[k - 1]).max(1e-9);
        let t = ((s - self.arc[k - 1]) / span).clamp(0.0, 1.0);
        (a.lerp(b, t), (b - a).normalize_or(DVec3::Y))
    }
}

/// A frond's one card: narrowed to [`NARROW`], its bottom edge where the stipe gives way
/// to the blade and its painted tip at the frond's tip.
struct FrondCard {
    start: f64,
    size: f64,
    stem: f64,
}

impl FrondCard {
    fn new(frond: &Frond) -> Self {
        // The painted frond runs from the stem to `TIP`; that stretch is
        // the blade, from the card's stem to the curve's end.
        let blade = frond.curve.length() - frond.start;
        let size = blade / (1.0 - TIP);
        Self {
            start: frond.start,
            size,
            stem: frond.start + size * (1.0 - STEM[1]),
        }
    }

    /// Arc length at card `v`.
    fn arc(&self, v: f64) -> f64 {
        self.start + (1.0 - v) * self.size
    }

    /// Across and up at arc length `s`: square to the rachis, facing up and
    /// out of the rosette.
    fn frame(frond: &Frond, s: f64) -> (DVec3, DVec3, DVec3) {
        let (p, t) = frond.curve.at(s);
        let across = (frond.across - t * frond.across.dot(t)).normalize_or(frond.across);
        let mut up = across.cross(t).normalize_or(DVec3::Y);
        if up.y < 0.0 {
            up = -up;
        }
        (p, across, up)
    }

    /// The card as three flat cards along the arch, together its area, for
    /// the crown field and the occupancy grid.
    fn proxies(&self, frond: &Frond) -> Vec<Card> {
        let size = self.size * (NARROW / 3.0).sqrt();
        [5.0 / 6.0, 0.5, 1.0 / 6.0]
            .into_iter()
            .map(|v| {
                let (centre, across, _) = Self::frame(frond, self.arc(v));
                let (_, axis) = frond.curve.at(self.arc(v));
                Card {
                    attach: centre - axis * size * (STEM[1] - 0.5),
                    axis,
                    across,
                    size,
                    node: frond.nodes[0],
                }
            })
            .collect()
    }

    /// The card as a grid bent along the rachis, `columns` across and
    /// `rows` along, its edges curling down as a frond's pinnae do. Past
    /// the curve's end it carries straight on along the last tangent, so
    /// the clear margin above the painted tip is not squashed.
    fn mesh(
        &self,
        frond: &Frond,
        piece: &mut Piece,
        field: &CrownField,
        pivot: u32,
        columns: u32,
        rows: u32,
    ) {
        let length = frond.curve.length();
        let mut first = None;
        for row in 0..=rows {
            let v = 1.0 - f64::from(row) / f64::from(rows);
            let s = self.arc(v);
            let (mut p, across, up) = Self::frame(frond, s);
            if s > length {
                let (_, tangent) = frond.curve.at(length);
                p += tangent * (s - length);
            }
            for col in 0..columns {
                let u = f64::from(col) / f64::from(columns - 1);
                let e = 2.0 * u - 1.0;
                let q = p + across * (e * 0.5 * self.size * NARROW)
                    - up * (0.1 * self.size * NARROW * e * e);
                // Still at the stem, most at the tip and the pinnae's ends.
                let along = ((s - self.stem) / (length - self.stem)).clamp(0.0, 1.0);
                let flutter = (0.8 * along.powf(1.2) + 0.3 * e.abs() * along).clamp(0.0, 1.0);
                let index = piece.vertex(
                    q,
                    card_normal(field, q, up),
                    [u, v],
                    pivot,
                    flutter,
                    field.curvature(q),
                );
                first.get_or_insert(index);
            }
        }
        piece.grid(first.expect("a card has vertices"), columns, rows + 1);
    }
}

/// How far each card's stem is from its stipe tube, which reaches a little
/// past it.
fn stem_offsets(skeleton: &Skeleton, fronds: &[Frond], cards: &[(usize, FrondCard)]) -> Vec<f64> {
    cards
        .iter()
        .map(|(k, card)| {
            let frond = &fronds[*k];
            let (stem, _) = frond.curve.at(card.stem);
            frond
                .nodes
                .windows(2)
                .map(|w| {
                    distance_to_segment(stem, skeleton.nodes[w[0]].pos, skeleton.nodes[w[1]].pos)
                })
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn distance_to_segment(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
