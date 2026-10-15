//! Reeds (ticket 0005, step 8): a clump of tall, narrow blades from one
//! foot, most nearly upright and the oldest bent over, a few dried to
//! straw, and among them a few stems each with a brown seed spike under
//! its tip, the reedmace a lake shore is fringed with.
//!
//! A reed wears no texture, as a flower does not: both pieces bind the
//! plain white definition and carry their colour per vertex. The blades are
//! the flower's strap leaf, raised and lengthened; the stems go through the
//! same tube mesher as a tree's branches, and each stem is a wind pivot
//! under one at the root, which the blades swing with.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r"
)]
use ashlar::glam::{DQuat, DVec3};

use crate::bark::{self, BarkDetail};
use crate::crown::CrownField;
use crate::flower::{Coloured, Detail, Leaf, rgba, soft, strap};
use crate::foliage::Card;
use crate::levels::{self, Built, Piece, Pivots, REED_BUDGET, Shade, assemble, occupancy};
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::{Age, Form};
use crate::{Bloom, TreeRecipe};

/// A stem as grown: where its spike sits and which way it points, whether
/// it carries one, and how much lighter or darker it is.
struct Stem {
    tip: DVec3,
    dir: DVec3,
    spike: bool,
    tint: f32,
}

/// The nearest level's options, finest first: the first that fits the
/// budget is used. A reed has no petals; `petal_rows` is unused.
const NEAR: [Detail; 2] = [
    Detail {
        petal_rows: 0,
        leaf_rows: 6,
        across: 3,
        sides: 4,
        centre: true,
    },
    Detail {
        petal_rows: 0,
        leaf_rows: 4,
        across: 3,
        sides: 3,
        centre: true,
    },
];

/// The far level's options, as [`NEAR`]'s; a far spike has four sides.
const FAR: [Detail; 2] = [
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

/// The share of a clump's blades that have dried to straw.
const DRY_SHARE: f64 = 0.18;

/// The colour of a dried blade, multiplied by its tint.
const STRAW: [f32; 3] = [0.55, 0.47, 0.27];

pub(crate) fn build(recipe: &TreeRecipe, variant: u32) -> Built {
    let form = Form::new(recipe, variant);
    let bloom = recipe.bloom.as_ref().expect("a reed has its colours");
    let mut rng = form.rng("reeds");
    let (skeleton, stems) = grow(&form, &mut rng);
    let blades = blades(&form, bloom, &mut rng);
    let branches = skeleton.branches();
    let pivots = Pivots::rosette(&skeleton, &branches, &form, 0.55, &mut form.rng("pivots"));
    let proxies = proxies(&stems, &blades, form.height);
    let field = CrownField::new(
        &proxies,
        DVec3::new(0.0, form.height * 0.5, 0.0),
        false,
        form.clumps,
    );
    let occupancy = occupancy(&skeleton, &proxies, &form);
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
        height: form.height,
    };
    let mut levels = Vec::with_capacity(2);
    let mut rings = Vec::with_capacity(2);
    for (level, options) in [&NEAR[..], &FAR[..]].into_iter().enumerate() {
        let mut built = None;
        for &detail in options {
            let drawn = draw(
                &tree, &stems, &blades, bloom, &pivots, &field, level, detail,
            );
            let triangles = drawn.0.mesh.triangle_count() + drawn.1.mesh.triangle_count();
            built = Some(drawn);
            if triangles <= REED_BUDGET[level] {
                break;
            }
        }
        let (stalks, leaves, level_rings) = built.expect("at least one option");
        rings.push(level_rings);
        levels.push([stalks.finish(&shade), leaves.finish(&shade)]);
    }
    Built {
        tree: assemble(recipe, variant, levels, Vec::new(), pivots),
        rings,
        field,
        pipe_ratios: Vec::new(),
        card_offsets: Vec::new(),
    }
}

/// Spikes and blades as flat cards, for the crown field and the grid.
fn proxies(stems: &[Stem], blades: &[Leaf], height: f64) -> Vec<Card> {
    let spikes = stems.iter().filter(|s| s.spike).map(|s| {
        let across = s.dir.any_orthonormal_vector();
        Card {
            attach: s.tip - s.dir * height * 0.2,
            axis: s.dir,
            across,
            size: height * 0.15,
            node: 0,
        }
    });
    let straps = blades.iter().map(|l| {
        let axis = (DVec3::Y * l.tilt.cos() + l.out * l.tilt.sin()).normalize();
        Card {
            attach: l.base,
            axis,
            across: DVec3::Y.cross(l.out).normalize(),
            size: l.length * 0.8,
            node: 0,
        }
    });
    spikes.chain(straps).collect()
}

/// One level: the stems, green, and the spikes and blades, coloured.
#[allow(
    clippy::too_many_arguments,
    reason = "a level and all it is drawn from"
)]
fn draw(
    tree: &bark::Tree<'_>,
    stems: &[Stem],
    blades: &[Leaf],
    bloom: &Bloom,
    pivots: &Pivots,
    field: &CrownField,
    level: usize,
    detail: Detail,
) -> (Piece, Piece, Vec<(usize, usize, f64)>) {
    let segment = tree.form.segment;
    let bark_detail = BarkDetail {
        max_sides: detail.sides,
        turn: if level == 0 { 0.25 } else { 0.8 },
        spacing: 1.0,
        spacing_max: segment * if level == 0 { 1.5 } else { 4.0 },
        spacing_min: segment * if level == 0 { 1.0 } else { 3.0 },
        min_radius: 0.0,
        fine_base: false,
    };
    let bark_mesh = bark::mesh(tree, &bark_detail);
    let colours = bark_mesh
        .pivot
        .iter()
        .map(|&p| {
            let tint = (p as usize)
                .checked_sub(1)
                .and_then(|k| stems.get(k))
                .map_or(1.0, |s| s.tint);
            rgba(bloom.stem, tint * 0.9)
        })
        .collect();
    let (mut stalks, rings) = Piece::from_bark(bark_mesh);
    stalks.mesh.colors = Some(colours);
    let height = tree.form.height;
    let mut leaves = Coloured::default();
    for (k, stem) in stems.iter().enumerate() {
        if stem.spike {
            let pivot = pivots.of_branch[k + 1];
            spike(
                &mut leaves,
                stem,
                height,
                bloom,
                pivot,
                field,
                detail.centre,
            );
        }
    }
    let root = pivots.of_branch[0];
    for blade in blades {
        strap(&mut leaves, blade, root, field, detail);
    }
    (stalks, leaves.done(), rings)
}

/// The stems: nearly upright from a narrow foot, a little curved, each to
/// most of the clump's height.
fn grow(form: &Form, rng: &mut Rng) -> (Skeleton, Vec<Stem>) {
    let height = form.height;
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.02, 0.0));
    skeleton.multi_stem = true;
    let start = rng.range(0.0, core::f64::consts::TAU);
    let foot = form.crown_radius * 0.3;
    let mut stems = Vec::new();
    for k in 0..form.stems {
        let azimuth = start + f64::from(k) * 2.4 + rng.signed() * 0.4;
        let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let length = height * rng.range(0.8, 1.0);
        let tilt = rng.range(0.02, 0.12);
        let mut dir = (DVec3::Y * tilt.cos() + out * tilt.sin()).normalize();
        let base = out * foot * rng.unit().sqrt() + DVec3::new(0.0, -0.02, 0.0);
        let mut wander = rng.direction();
        let mut at = skeleton.add(0, base, wander);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let segments = ((length / form.segment).round() as usize).max(4);
        #[allow(clippy::cast_precision_loss)]
        let seg = length / segments as f64;
        let across = DVec3::Y.cross(out).normalize();
        for _ in 0..segments {
            wander = (wander + rng.direction() * 0.2).normalize();
            dir = (DQuat::from_axis_angle(across, 0.012) * dir + wander * 0.02).normalize();
            let p = skeleton.nodes[at].pos + dir * seg;
            at = skeleton.add(at, p, wander);
        }
        stems.push(Stem {
            tip: skeleton.nodes[at].pos,
            dir,
            // The first stem always carries one: a clump reads as reeds by
            // its spikes.
            spike: rng.chance(match form.age {
                Age::Young => 0.3,
                Age::Mature => 0.75,
                Age::Old => 0.9,
            }) || k == 0,
            #[allow(clippy::cast_possible_truncation)]
            tint: rng.range(0.85, 1.1) as f32,
        });
    }
    let r0 = form.trunk_radius;
    for index in 1..skeleton.nodes.len() {
        let t = (skeleton.nodes[index].pos.y / height).clamp(0.0, 1.0);
        skeleton.nodes[index].radius = r0 * (1.0 - 0.5 * t);
    }
    skeleton.nodes[0].radius = r0 * 2.0;
    skeleton.compute_ages();
    (skeleton, stems)
}

/// The blades: many long straps from the foot, most standing nearly
/// upright and bending over towards their tips, the longest the most; a
/// few dried to straw.
fn blades(form: &Form, bloom: &Bloom, rng: &mut Rng) -> Vec<Leaf> {
    let count = match form.age {
        Age::Young => 10,
        Age::Mature => rng.between(17, 22),
        Age::Old => 26,
    };
    let start = rng.range(0.0, core::f64::consts::TAU);
    (0..count)
        .map(|k| {
            let azimuth = start + f64::from(k) * 2.4 + rng.signed() * 0.5;
            let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
            let reach = rng.range(0.5, 0.95);
            let dry = rng.chance(DRY_SHARE);
            Leaf {
                base: out * form.crown_radius * 0.35 * rng.unit().sqrt(),
                out,
                tilt: rng.range(0.04, 0.28),
                // The long ones bend over; the short ones stand.
                arch: reach * rng.range(0.4, 1.2),
                length: form.height * reach,
                width: form.height * rng.range(0.016, 0.024),
                colour: if dry { STRAW } else { bloom.stem },
                #[allow(clippy::cast_possible_truncation)]
                tint: rng.range(0.8, 1.1) as f32,
            }
        })
        .collect()
}

/// A seed spike: a brown cylinder round the stem a little under its tip,
/// rounded at both ends when `near`, a plain prism of four sides when not.
fn spike(
    out: &mut Coloured,
    stem: &Stem,
    height: f64,
    bloom: &Bloom,
    pivot: u32,
    field: &CrownField,
    near: bool,
) {
    let sides: u32 = if near { 6 } else { 4 };
    let length = height * 0.1;
    let radius = height * 0.013;
    let top = stem.tip - stem.dir * height * 0.06;
    let bottom = top - stem.dir * length;
    let across = stem.dir.any_orthonormal_vector();
    let other = stem.dir.cross(across);
    // Rings from the foot to the head, each its height along the spike and
    // its radius over the spike's.
    let rings: &[(f64, f64)] = if near {
        &[(0.0, 0.55), (0.12, 1.0), (0.88, 1.0), (1.0, 0.5)]
    } else {
        &[(0.0, 0.8), (1.0, 0.8)]
    };
    let first = u32::try_from(out.piece.mesh.positions.len()).expect("a reed is small");
    for &(t, grow) in rings {
        let centre = bottom.lerp(top, t);
        for j in 0..sides {
            let angle = core::f64::consts::TAU * f64::from(j) / f64::from(sides);
            let d = across * angle.cos() + other * angle.sin();
            let p = centre + d * radius * grow;
            #[allow(clippy::cast_possible_truncation)]
            let shade = 0.85 + 0.15 * t as f32;
            out.vertex(
                p,
                soft(field, p, d),
                [f64::from(j) / f64::from(sides), t],
                pivot,
                0.6,
                field.curvature(p),
                rgba(bloom.petal, stem.tint * shade),
            );
        }
    }
    for ring in 0..u32::try_from(rings.len() - 1).expect("a few rings") {
        for j in 0..sides {
            let a = first + ring * sides + j;
            let b = first + ring * sides + (j + 1) % sides;
            let (c, d) = (a + sides, b + sides);
            out.piece.mesh.indices.extend([a, c, b, b, c, d]);
        }
    }
}
