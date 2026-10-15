//! Wildflowers: a clump of stems, each with a head of petals, over a few
//! strap leaves at the ground.
//!
//! Flowers wear no texture. Both pieces bind the plain white definition and
//! carry their colour per vertex, which the runtime multiplies in: green
//! stems and leaves, petals darker at their base, and a disc of the centre
//! colour. Petals are real geometry, cupped by the recipe's `cup`, because
//! at a flower's size a card would read as a card. Stems go through the
//! same tube mesher as a tree's branches, and each stem is a wind pivot
//! under one at the root.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::{DQuat, DVec3};

use crate::bark::{self, BarkDetail};
use crate::crown::CrownField;
use crate::foliage::Card;
use crate::levels::{self, Built, Piece, Pivots, SMALL_BUDGET, Shade, assemble, occupancy};
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::{Age, Form};
use crate::{Bloom, TreeRecipe};

/// A flowering stem as grown.
struct Stem {
    /// The stem's head: where it sits and which way it faces.
    head: DVec3,
    facing: DVec3,
    /// Petal count and length, and a turn so neighbouring heads differ.
    petals: u32,
    length: f64,
    turn: f64,
    /// Still closed.
    bud: bool,
    /// How much this stem's greens and petals are lighter or darker.
    tint: f32,
    /// Where a small leaf sits on the stem, and which way it points.
    leaf: (DVec3, DVec3),
}

/// A strap leaf from the root: its base, the plane it arches in, how far
/// it bends over its length (radians), its size and colour.
pub(crate) struct Leaf {
    pub base: DVec3,
    pub out: DVec3,
    pub tilt: f64,
    pub arch: f64,
    pub length: f64,
    pub width: f64,
    pub colour: [f32; 3],
    pub tint: f32,
}

/// How far a flower's strap leaf bends over its length, radians: out and
/// over, its tip near the ground.
const FLOWER_ARCH: f64 = 1.4;

/// How finely one level draws petals and leaves.
#[derive(Clone, Copy)]
pub(crate) struct Detail {
    /// Segments along a petal; 0 draws a petal as one triangle.
    pub petal_rows: u32,
    /// Segments along a leaf.
    pub leaf_rows: u32,
    /// Vertices across a petal or leaf: 3 folds them along the middle.
    pub across: u32,
    /// Sides of a stem.
    pub sides: u32,
    /// The disc at a flower's middle.
    pub centre: bool,
}

/// The nearest level's options, finest first: the first that fits the
/// budget is used.
const NEAR: [Detail; 3] = [
    Detail {
        petal_rows: 2,
        leaf_rows: 4,
        across: 3,
        sides: 4,
        centre: true,
    },
    Detail {
        petal_rows: 1,
        leaf_rows: 3,
        across: 3,
        sides: 4,
        centre: true,
    },
    Detail {
        petal_rows: 1,
        leaf_rows: 2,
        across: 3,
        sides: 3,
        centre: true,
    },
];

/// The far level's options, as [`NEAR`]'s.
const FAR: [Detail; 3] = [
    Detail {
        petal_rows: 1,
        leaf_rows: 2,
        across: 2,
        sides: 3,
        centre: false,
    },
    Detail {
        petal_rows: 0,
        leaf_rows: 2,
        across: 2,
        sides: 3,
        centre: false,
    },
    Detail {
        petal_rows: 0,
        leaf_rows: 1,
        across: 2,
        sides: 3,
        centre: false,
    },
];

pub(crate) fn build(recipe: &TreeRecipe, variant: u32) -> Built {
    let form = Form::new(recipe, variant);
    let bloom = recipe.bloom.as_ref().expect("a flower recipe blooms");
    let mut rng = form.rng("flowers");
    let (skeleton, stems) = grow(recipe, &form, bloom, &mut rng);
    let leaves = basal_leaves(&form, bloom, &mut rng);
    let branches = skeleton.branches();
    // Stem k is branch and pivot k + 1: the root's children in the order
    // they grew.
    let pivots = Pivots::rosette(&skeleton, &branches, &form, 0.4, &mut form.rng("pivots"));
    let proxies = proxies(&stems, &leaves);
    let field = CrownField::new(
        &proxies,
        DVec3::new(0.0, form.height * 0.5, 0.0),
        false,
        form.clumps,
    );
    let occupancy = occupancy(&skeleton, &proxies, &form);
    let clump = Clump {
        tree: bark::Tree {
            recipe,
            form: &form,
            skeleton: &skeleton,
            branches: &branches,
            pivots: &pivots.of_branch,
            tile: levels::bark_tile(recipe),
            root_flare: false,
            cap_base: false,
            splinter: 0.0,
        },
        stems: &stems,
        leaves: &leaves,
        bloom,
        pivots: &pivots,
        field: &field,
    };
    let shade = Shade {
        pivots: &pivots,
        field: &field,
        occupancy: &occupancy,
        height: form.height,
    };
    let mut levels = Vec::with_capacity(2);
    let mut rings = Vec::with_capacity(2);
    for (level, options) in [NEAR, FAR].into_iter().enumerate() {
        let mut built = None;
        for detail in options {
            let (stalks, blooms, level_rings) = clump.draw(level, detail);
            let triangles = stalks.mesh.triangle_count() + blooms.mesh.triangle_count();
            let fits = triangles <= SMALL_BUDGET[level];
            built = Some((stalks, blooms, level_rings));
            if fits {
                break;
            }
        }
        let (stalks, blooms, level_rings) = built.expect("at least one option");
        rings.push(level_rings);
        levels.push([stalks.finish(&shade), blooms.finish(&shade)]);
    }
    Built {
        tree: assemble(recipe, variant, levels, Vec::new(), pivots),
        rings,
        field,
        pipe_ratios: Vec::new(),
        card_offsets: Vec::new(),
    }
}

/// Heads and leaves as flat cards, for the crown field and the grid.
fn proxies(stems: &[Stem], leaves: &[Leaf]) -> Vec<Card> {
    let heads = stems.iter().map(|s| {
        let across = s.facing.any_orthonormal_vector();
        let axis = s.facing.cross(across);
        let size = s.length * 2.5;
        Card {
            attach: s.head - axis * size * 0.45,
            axis,
            across,
            size,
            node: 0,
        }
    });
    let straps = leaves.iter().map(|l| {
        let axis = (DVec3::Y * l.tilt.cos() + l.out * l.tilt.sin()).normalize();
        Card {
            attach: l.base,
            axis,
            across: DVec3::Y.cross(l.out).normalize(),
            size: l.length * 0.7,
            node: 0,
        }
    });
    heads.chain(straps).collect()
}

/// Everything a level of a clump is drawn from.
struct Clump<'a> {
    tree: bark::Tree<'a>,
    stems: &'a [Stem],
    leaves: &'a [Leaf],
    bloom: &'a Bloom,
    pivots: &'a Pivots,
    field: &'a CrownField,
}

impl Clump<'_> {
    /// One level: the stems, green, and the heads and leaves, coloured.
    fn draw(&self, level: usize, detail: Detail) -> (Piece, Piece, Vec<(usize, usize, f64)>) {
        let segment = self.tree.form.segment;
        let bark_detail = BarkDetail {
            max_sides: detail.sides,
            turn: if level == 0 { 0.25 } else { 0.8 },
            spacing: 1.0,
            spacing_max: segment * if level == 0 { 1.5 } else { 4.0 },
            spacing_min: segment * if level == 0 { 1.0 } else { 3.0 },
            min_radius: 0.0,
            fine_base: false,
        };
        let bark_mesh = bark::mesh(&self.tree, &bark_detail);
        // Each stem a little lighter or darker; pivot k + 1 is stem k.
        let colours = bark_mesh
            .pivot
            .iter()
            .map(|&p| {
                let tint = (p as usize)
                    .checked_sub(1)
                    .and_then(|k| self.stems.get(k))
                    .map_or(1.0, |s| s.tint);
                rgba(self.bloom.stem, tint)
            })
            .collect();
        let (mut stalks, rings) = Piece::from_bark(bark_mesh);
        stalks.mesh.colors = Some(colours);
        let height = self.tree.form.height;
        let mut blooms = Coloured::default();
        for (k, stem) in self.stems.iter().enumerate() {
            let pivot = self.pivots.of_branch[k + 1];
            head(&mut blooms, stem, self.bloom, pivot, self.field, detail);
            let leaf = Leaf {
                base: stem.leaf.0,
                out: stem.leaf.1,
                tilt: 1.0,
                arch: FLOWER_ARCH,
                length: height * 0.16,
                width: height * 0.035,
                colour: self.bloom.stem,
                tint: stem.tint,
            };
            strap(&mut blooms, &leaf, pivot, self.field, detail);
        }
        for leaf in self.leaves {
            let root = self.pivots.of_branch[0];
            strap(&mut blooms, leaf, root, self.field, detail);
        }
        (stalks, blooms.done(), rings)
    }
}

pub(crate) fn rgba(colour: [f32; 3], tint: f32) -> [f32; 4] {
    [
        (colour[0] * tint).clamp(0.0, 1.0),
        (colour[1] * tint).clamp(0.0, 1.0),
        (colour[2] * tint).clamp(0.0, 1.0),
        1.0,
    ]
}

/// The stems: a clump from a small foot, leaning out a little and curving
/// as they rise. A flower that cups nods its head; a daisy faces the sky.
fn grow(recipe: &TreeRecipe, form: &Form, bloom: &Bloom, rng: &mut Rng) -> (Skeleton, Vec<Stem>) {
    let height = form.height;
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.02, 0.0));
    skeleton.multi_stem = true;
    let turn = recipe.alien.phyllotaxis_deg.to_radians();
    let start = rng.range(0.0, core::f64::consts::TAU);
    let foot = form.crown_radius * 0.25;
    let bud_chance = match form.age {
        Age::Young => 0.6,
        Age::Mature => 0.15,
        Age::Old => 0.05,
    };
    let mut stems = Vec::new();
    for k in 0..form.stems {
        let azimuth = start + f64::from(k) * turn + rng.signed() * 0.3;
        let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let reach = if k == 0 { 0.95 } else { rng.range(0.6, 1.0) };
        let length = height * reach;
        let tilt = rng.range(0.05, 0.35) * form.crown_radius / (height * 0.35);
        let mut dir = (DVec3::Y * tilt.cos() + out * tilt.sin()).normalize();
        let mut wander = rng.direction();
        let base = out * foot * rng.unit().sqrt() + DVec3::new(0.0, -0.02, 0.0);
        let mut at = skeleton.add(0, base, wander);
        let first = at;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let segments = ((length / form.segment).round() as usize).max(4);
        #[allow(clippy::cast_precision_loss)]
        let seg = length / segments as f64;
        let across = DVec3::Y.cross(out).normalize();
        for _ in 0..segments {
            wander = (wander + rng.direction() * 0.3).normalize();
            dir = (DQuat::from_axis_angle(across, 0.04) * dir + wander * 0.06).normalize();
            let p = skeleton.nodes[at].pos + dir * seg;
            at = skeleton.add(at, p, wander);
        }
        let tip = skeleton.nodes[at].pos;
        let nod = bloom.cup * 0.9 * rng.range(0.6, 1.2);
        let facing =
            (DQuat::from_axis_angle(across, nod) * (dir * 0.5 + DVec3::Y * 0.5)).normalize();
        let [fewest, most] = bloom.petals;
        let bud = rng.chance(bud_chance) && k > 0;
        let mid = skeleton.nodes[first].pos.lerp(tip, rng.range(0.25, 0.5));
        let leaf_turn = rng.range(0.0, core::f64::consts::TAU);
        stems.push(Stem {
            head: tip,
            facing,
            petals: rng.between(fewest, most),
            length: height * bloom.petal_length * rng.range(0.85, 1.15),
            turn: rng.range(0.0, core::f64::consts::TAU),
            bud,
            #[allow(clippy::cast_possible_truncation)]
            tint: rng.range(0.85, 1.15) as f32,
            leaf: (mid, DVec3::new(leaf_turn.cos(), 0.0, leaf_turn.sin())),
        });
    }
    let r0 = form.trunk_radius;
    for index in 1..skeleton.nodes.len() {
        let t = (skeleton.nodes[index].pos.y / height).clamp(0.0, 1.0);
        skeleton.nodes[index].radius = r0 * (1.0 - 0.4 * t);
    }
    skeleton.nodes[0].radius = r0 * 2.0;
    skeleton.compute_ages();
    (skeleton, stems)
}

/// Strap leaves round the foot of the clump, arching out and over.
fn basal_leaves(form: &Form, bloom: &Bloom, rng: &mut Rng) -> Vec<Leaf> {
    let count = match form.age {
        Age::Young => 3,
        Age::Mature => rng.between(4, 6),
        Age::Old => 7,
    };
    let start = rng.range(0.0, core::f64::consts::TAU);
    (0..count)
        .map(|k| {
            let azimuth = start + f64::from(k) * 2.4 + rng.signed() * 0.4;
            let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
            Leaf {
                base: out * form.crown_radius * 0.05,
                out,
                tilt: rng.range(0.35, 0.8),
                arch: FLOWER_ARCH,
                length: form.height * rng.range(0.35, 0.6),
                width: form.height * rng.range(0.045, 0.06),
                colour: bloom.stem,
                #[allow(clippy::cast_possible_truncation)]
                tint: rng.range(0.8, 1.1) as f32,
            }
        })
        .collect()
}

/// A piece whose vertices each carry a colour.
#[derive(Default)]
pub(crate) struct Coloured {
    pub piece: Piece,
    colours: Vec<[f32; 4]>,
}

impl Coloured {
    #[allow(clippy::too_many_arguments, reason = "a vertex and all it carries")]
    pub(crate) fn vertex(
        &mut self,
        p: DVec3,
        n: DVec3,
        uv: [f64; 2],
        pivot: u32,
        flutter: f64,
        curvature: f64,
        colour: [f32; 4],
    ) -> u32 {
        self.colours.push(colour);
        self.piece.vertex(p, n, uv, pivot, flutter, curvature)
    }

    pub(crate) fn done(mut self) -> Piece {
        self.piece.mesh.colors = Some(self.colours);
        self.piece
    }
}

/// A petal or leaf normal: its own surface, softened towards the clump's
/// masses by half, which is enough for a flower: its shape is the point.
pub(crate) fn soft(field: &CrownField, p: DVec3, n: DVec3) -> DVec3 {
    (n + field.normal(p)).normalize_or(n)
}

/// One flower head: petals round a centre disc, cupped by the bloom's
/// `cup`, or a closed bud.
fn head(
    out: &mut Coloured,
    stem: &Stem,
    bloom: &Bloom,
    pivot: u32,
    field: &CrownField,
    detail: Detail,
) {
    let facing = stem.facing;
    let reference = facing.any_orthonormal_vector();
    let (cup, length) = if stem.bud {
        (0.95, stem.length * 0.45)
    } else {
        (bloom.cup, stem.length)
    };
    let centre_radius = length * if stem.bud { 0.15 } else { 0.3 };
    // Petals meet about half way out and overlap a little.
    let width =
        1.15 * core::f64::consts::TAU * (centre_radius + 0.5 * length) / f64::from(stem.petals);
    let rise = (10.0 + cup * 70.0).to_radians();
    for j in 0..stem.petals {
        let angle = stem.turn + f64::from(j) * core::f64::consts::TAU / f64::from(stem.petals);
        let dir = DQuat::from_axis_angle(facing, angle) * reference;
        let side = facing.cross(dir);
        let at = |t: f64, e: f64| {
            // Flat petals droop at the tip; cupped ones curl in.
            let lift = rise * (0.6 + 0.8 * t) - if cup < 0.3 { 0.3 * t * t } else { 0.0 };
            let outward = dir * lift.cos() + facing * lift.sin();
            let w = width * (0.35 + 0.65 * (core::f64::consts::PI * t.powf(0.8)).sin().max(0.0));
            stem.head
                + dir * centre_radius
                + outward * t * length
                + side * (e * 0.5 * w)
                + facing * (0.12 * w * e * e)
        };
        if detail.petal_rows == 0 {
            // One triangle from the heart to either side of the tip.
            let points = [at(0.0, 0.0), at(1.0, -1.0), at(1.0, 1.0)];
            let n = (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize_or(facing);
            let n = if n.dot(facing) < 0.0 { -n } else { n };
            let first = out.piece.mesh.positions.len();
            for (k, p) in points.into_iter().enumerate() {
                let t = if k == 0 { 0.0 } else { 1.0 };
                out.vertex(
                    p,
                    soft(field, p, n),
                    [0.5, 1.0 - t],
                    pivot,
                    0.5 * t,
                    field.curvature(p),
                    petal_colour(bloom, stem, t),
                );
            }
            let first = u32::try_from(first).expect("a flower is small");
            out.piece.mesh.indices.extend([first, first + 1, first + 2]);
            continue;
        }
        let rows = detail.petal_rows;
        let mut first = None;
        for row in 0..=rows {
            let t = f64::from(row) / f64::from(rows);
            for col in 0..detail.across {
                let e = 2.0 * f64::from(col) / f64::from(detail.across - 1) - 1.0;
                let p = at(t, e);
                let dt = at((t + 0.05).min(1.0), e) - at((t - 0.05).max(0.0), e);
                let de = at(t, (e + 0.1).min(1.0)) - at(t, (e - 0.1).max(-1.0));
                let mut n = dt.cross(de).normalize_or(facing);
                if n.dot(facing) < 0.0 {
                    n = -n;
                }
                let index = out.vertex(
                    p,
                    soft(field, p, n),
                    [0.5 + 0.5 * e, 1.0 - t],
                    pivot,
                    (0.5 * t + 0.15 * e.abs()).min(1.0),
                    field.curvature(p),
                    petal_colour(bloom, stem, t),
                );
                first.get_or_insert(index);
            }
        }
        out.piece.grid(
            first.expect("a petal has vertices"),
            detail.across,
            rows + 1,
        );
    }
    if detail.centre && !stem.bud {
        centre_disc(out, stem, bloom, pivot, field, reference, centre_radius);
    }
}
/// A low dome of the centre colour in the middle of an open flower.
fn centre_disc(
    out: &mut Coloured,
    stem: &Stem,
    bloom: &Bloom,
    pivot: u32,
    field: &CrownField,
    reference: DVec3,
    centre_radius: f64,
) {
    let facing = stem.facing;
    let first = u32::try_from(out.piece.mesh.positions.len()).expect("a flower is small");
    let top = stem.head + facing * centre_radius * 0.6;
    out.vertex(
        top,
        soft(field, top, facing),
        [0.5, 0.5],
        pivot,
        0.0,
        0.5,
        rgba(bloom.centre, stem.tint),
    );
    let ring = 6;
    for j in 0..ring {
        let angle = f64::from(j) * core::f64::consts::TAU / f64::from(ring);
        let d = DQuat::from_axis_angle(facing, angle) * reference;
        let p = stem.head + d * centre_radius * 1.05;
        out.vertex(
            p,
            soft(field, p, (d + facing).normalize()),
            [0.5, 0.5],
            pivot,
            0.0,
            0.3,
            rgba(bloom.centre, stem.tint * 0.8),
        );
    }
    for j in 0..ring {
        let (a, b) = (first + 1 + j, first + 1 + (j + 1) % ring);
        out.piece.mesh.indices.extend([first, a, b]);
    }
}

/// Petals are darker where they join the head and brightest at the tip.
fn petal_colour(bloom: &Bloom, stem: &Stem, t: f64) -> [f32; 4] {
    #[allow(clippy::cast_possible_truncation)]
    let shade = 0.8 + 0.2 * t as f32;
    rgba(bloom.petal, shade * (0.9 + 0.1 * stem.tint))
}

/// A strap leaf: a strip arching out of its base and over by its `arch`,
/// folded along its middle, pointed at the tip, lighter towards it.
pub(crate) fn strap(
    out: &mut Coloured,
    leaf: &Leaf,
    pivot: u32,
    field: &CrownField,
    detail: Detail,
) {
    let across = DVec3::Y.cross(leaf.out).normalize();
    let rows = detail.leaf_rows;
    let steps = 12;
    // The centreline, arching in the leaf's vertical plane.
    let mut spine = vec![leaf.base];
    let mut dir = (DVec3::Y * leaf.tilt.cos() + leaf.out * leaf.tilt.sin()).normalize();
    for _ in 0..steps {
        dir = DQuat::from_axis_angle(across, leaf.arch / f64::from(steps)) * dir;
        let mut p = *spine.last().expect("a spine") + dir * (leaf.length / f64::from(steps));
        p.y = p.y.max(0.005);
        spine.push(p);
    }
    let at = |t: f64| {
        let x = t * f64::from(steps);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k = (x.floor() as u32).min(steps - 1);
        let f = (x - f64::from(k)).clamp(0.0, 1.0);
        let (a, b) = (spine[k as usize], spine[k as usize + 1]);
        (a.lerp(b, f), (b - a).normalize_or(DVec3::Y))
    };
    let mut first = None;
    for row in 0..=rows {
        let t = f64::from(row) / f64::from(rows);
        let (p, tangent) = at(t);
        let mut up = across.cross(tangent).normalize_or(DVec3::Y);
        if up.y < 0.0 {
            up = -up;
        }
        let w = leaf.width
            * (core::f64::consts::PI * (0.08 + 0.92 * t))
                .sin()
                .max(0.0)
                .powf(0.6);
        for col in 0..detail.across {
            let e = 2.0 * f64::from(col) / f64::from(detail.across - 1) - 1.0;
            let q = p + across * (e * 0.5 * w) + up * (0.25 * w * e.abs());
            #[allow(clippy::cast_possible_truncation)]
            let light = 0.85 + 0.3 * t as f32;
            let index = out.vertex(
                q,
                soft(field, q, up),
                [0.5 + 0.5 * e, 1.0 - t],
                pivot,
                (0.7 * t + 0.1 * e.abs()).min(1.0),
                field.curvature(q),
                rgba(leaf.colour, leaf.tint * light),
            );
            first.get_or_insert(index);
        }
    }
    out.piece
        .grid(first.expect("a leaf has vertices"), detail.across, rows + 1);
}
