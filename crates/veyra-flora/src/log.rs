//! A fallen log: a trunk lying along +X on the ground, broken at both ends,
//! with a couple of snapped stub branches and moss on its upper side.
//!
//! The log is a skeleton lying down, so it goes through the same tube
//! mesher as a standing tree, with integer bark repeats round every ring;
//! both ends are capped and splintered. Moss is a green tint in the vertex
//! colours where the bark faces the sky, in patches, which the runtime
//! multiplies into the bark. Its leaf piece is a few sprouts from the stubs
//! and the top, the shoots a fallen trunk puts up; it keeps every prop at
//! two pieces a level. A log does not move: one pivot, stiff.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::{DQuat, DVec3};
use ashlar::prop::PropCollider;

use crate::TreeRecipe;
use crate::bark::{self, BarkDetail};
use crate::crown::CrownField;
use crate::foliage::Card;
use crate::levels::{
    self, BUSH_BUDGET, Built, Piece, Pivot, Pivots, Shade, assemble, card_mesh, node_branches,
    occupancy,
};
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::{Age, Form};

/// Linear moss green, multiplied into the bark where it grows.
const MOSS: [f64; 3] = [0.45, 0.9, 0.3];

/// Stiffness of a thing that does not sway: far above any wood's 0 to 1.
const STILL: f64 = 10_000.0;

pub(crate) fn build(recipe: &TreeRecipe, variant: u32) -> Built {
    let form = Form::new(recipe, variant);
    let mut rng = form.rng("log");
    let (skeleton, anchors) = grow(&form, &mut rng);
    let branches = skeleton.branches();
    let length = form.height;
    let radius = form.trunk_radius;

    let mut pivot_rng = form.rng("pivots");
    let mut pivots = Pivots::for_branches(branches.len());
    pivots.of_branch[0] = pivots.push(
        &Pivot {
            pos: DVec3::ZERO,
            parent: None,
            axis: DVec3::X,
            length,
            radius,
            stiffness: STILL,
        },
        &mut pivot_rng,
    );
    pivots.inherit(&branches);

    let cards = sprouts(&skeleton, &anchors, &form, &mut rng);
    let field = CrownField::new(&cards, DVec3::new(0.0, radius, 0.0), false, 2);
    let occupancy = occupancy(&skeleton, &cards, &form);
    let node_branch = node_branches(&skeleton, &branches);
    let card_offsets = cards
        .iter()
        .map(|c| c.attach.distance(skeleton.nodes[c.node].pos) - skeleton.nodes[c.node].radius)
        .map(|d| d.max(0.0))
        .collect();

    let tree = bark::Tree {
        recipe,
        form: &form,
        skeleton: &skeleton,
        branches: &branches,
        pivots: &pivots.of_branch,
        tile: levels::bark_tile(recipe),
        root_flare: false,
        cap_base: true,
        splinter: 0.6,
    };
    let shade = Shade {
        pivots: &pivots,
        field: &field,
        occupancy: &occupancy,
        height: top(&skeleton),
    };
    let moss = match form.age {
        Age::Young => 0.5,
        Age::Mature => 0.8,
        Age::Old => 1.0,
    };
    let phase = [rng.range(0.0, 6.0), rng.range(0.0, 6.0)];
    let mut levels = Vec::with_capacity(2);
    let mut rings = Vec::with_capacity(2);
    for (level, &budget) in BUSH_BUDGET.iter().enumerate() {
        let near = level == 0;
        let mut detail = BarkDetail {
            max_sides: if near { 12 } else { 6 },
            turn: if near { 0.08 } else { 0.4 },
            spacing: if near { 0.6 } else { 3.0 },
            spacing_max: if near { 0.2 } else { 0.8 },
            spacing_min: form.segment * if near { 1.0 } else { 4.0 },
            min_radius: 0.0,
            fine_base: false,
        };
        let mut bark_mesh = bark::mesh(&tree, &detail);
        let leaves = card_mesh(&cards, near, &node_branch, &pivots, &field);
        while bark_mesh.mesh.triangle_count() + leaves.mesh.triangle_count() > budget
            && detail.spacing_max < length
        {
            detail.spacing_max *= 1.4;
            detail.spacing_min *= 1.4;
            detail.max_sides = (detail.max_sides - 1).max(4);
            bark_mesh = bark::mesh(&tree, &detail);
        }
        let (mut wood, level_rings) = Piece::from_bark(bark_mesh);
        let colours = wood
            .mesh
            .positions
            .iter()
            .zip(&wood.mesh.normals)
            .map(|(p, n)| mossy(*p, *n, moss, phase))
            .collect();
        wood.mesh.colors = Some(colours);
        rings.push(level_rings);
        levels.push([wood.finish(&shade), leaves.finish(&shade)]);
    }

    let collider = capsule(&skeleton, &branches[0].nodes, radius);
    Built {
        tree: assemble(recipe, variant, levels, vec![collider], pivots),
        rings,
        field,
        pipe_ratios: Vec::new(),
        card_offsets,
    }
}

/// The log along +X, resting on the ground as it tapers, with a gentle
/// wander sideways; then its stubs, snapped off and pointing anywhere but
/// into the ground. Answers the skeleton and the nodes sprouts grow from.
fn grow(form: &Form, rng: &mut Rng) -> (Skeleton, Vec<usize>) {
    let length = form.height;
    let radius = form.trunk_radius;
    let r_at = |s: f64| radius * (1.0 - 0.15 * s / length);
    let mut skeleton = Skeleton::root(DVec3::new(-length * 0.5, r_at(0.0) * 0.9, 0.0));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = ((length / form.segment).round() as usize).max(8);
    #[allow(clippy::cast_precision_loss)]
    let seg = length / steps as f64;
    let mut dir = DVec3::X;
    let mut wander = rng.direction();
    let mut trunk = vec![0];
    let mut at = 0;
    skeleton.nodes[0].radius = r_at(0.0);
    for i in 1..=steps {
        wander = (wander + rng.direction() * 0.3).normalize();
        // A gentle sideways wander that straightens again: a log is a
        // trunk, not a snake.
        dir = DVec3::new(1.0, 0.0, dir.z * 0.85 + wander.z * 0.03).normalize();
        #[allow(clippy::cast_precision_loss)]
        let s = seg * i as f64;
        let r = r_at(s) * rng.range(0.97, 1.03);
        let mut p = skeleton.nodes[at].pos + dir * seg;
        p.y = r * 0.9;
        at = skeleton.add(at, p, wander);
        skeleton.nodes[at].radius = r;
        trunk.push(at);
    }
    skeleton.nodes[at].capped = true;
    let mut anchors = Vec::new();
    for _ in 0..form.stems.min(4) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let index = trunk[((trunk.len() as f64) * rng.range(0.2, 0.8)) as usize];
        let around = rng.range(-1.3, 1.3);
        let out = (DQuat::from_axis_angle(DVec3::X, around) * DVec3::Y
            + DVec3::X * rng.range(-0.3, 0.5))
        .normalize();
        let stub_radius = skeleton.nodes[index].radius * rng.range(0.3, 0.45);
        let reach = skeleton.nodes[index].radius + rng.range(0.12, 0.4);
        let mut prev = index;
        let count = rng.between(2, 3);
        for k in 1..=count {
            let p = skeleton.nodes[index].pos + out * reach * f64::from(k) / f64::from(count);
            prev = skeleton.add(prev, p, out);
            skeleton.nodes[prev].radius = stub_radius * (1.0 - 0.1 * f64::from(k));
        }
        skeleton.nodes[prev].capped = true;
        anchors.push(prev);
    }
    // Sprouts also come up from the top of the log.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    anchors.push(trunk[((trunk.len() as f64) * rng.range(0.3, 0.7)) as usize]);
    skeleton.compute_ages();
    (skeleton, anchors)
}

/// A few leafy shoots at each anchor, splayed up and out.
fn sprouts(skeleton: &Skeleton, anchors: &[usize], form: &Form, rng: &mut Rng) -> Vec<Card> {
    let per = match form.age {
        Age::Young => 2,
        Age::Mature => 3,
        Age::Old => 4,
    };
    let limit = form.cards as usize;
    let mut cards = Vec::new();
    for &node in anchors {
        let n = &skeleton.nodes[node];
        // On the trunk a sprout grows from the top of the bark, on a stub
        // from its broken end.
        let base = if n.children.is_empty() && n.capped {
            n.pos
        } else {
            n.pos + DVec3::Y * n.radius
        };
        for _ in 0..per {
            if cards.len() >= limit {
                break;
            }
            let axis = (DVec3::Y + rng.direction() * 0.7).normalize();
            let axis = if axis.y < 0.2 {
                (axis + DVec3::Y).normalize()
            } else {
                axis
            };
            let face = (rng.direction() - axis * rng.direction().dot(axis)).normalize_or(DVec3::X);
            cards.push(Card {
                attach: base,
                axis,
                across: axis.cross(face).normalize_or(DVec3::Z),
                size: form.card_size * rng.range(0.8, 1.2),
                node,
            });
        }
    }
    cards
}

/// The highest bark above the ground, what the shade's height runs over.
fn top(skeleton: &Skeleton) -> f64 {
    skeleton
        .nodes
        .iter()
        .map(|n| n.pos.y + n.radius)
        .fold(0.0, f64::max)
}

/// Moss where the bark faces the sky, in patches, as a tint towards green
/// that the runtime multiplies into the bark.
#[allow(clippy::cast_possible_truncation)]
fn mossy(p: DVec3, n: DVec3, amount: f64, phase: [f64; 2]) -> [f32; 4] {
    let up = ((n.y - 0.2) / 0.55).clamp(0.0, 1.0);
    let up = up * up * (3.0 - 2.0 * up);
    let patch = 0.5 + 0.5 * (p.x * 5.3 + phase[0]).sin() * (p.z * 7.1 + p.x * 2.3 + phase[1]).cos();
    let m = (up * (0.35 + 0.65 * patch) * amount).clamp(0.0, 1.0);
    [
        (1.0 + (MOSS[0] - 1.0) * m) as f32,
        (1.0 + (MOSS[1] - 1.0) * m) as f32,
        (1.0 + (MOSS[2] - 1.0) * m) as f32,
        1.0,
    ]
}

/// One capsule along the log's axis, lying on the ground: from one end to
/// the other less a radius, its middle a radius up.
fn capsule(skeleton: &Skeleton, trunk: &[usize], radius: f64) -> PropCollider {
    let first = skeleton.nodes[trunk[0]].pos;
    let last = skeleton.nodes[*trunk.last().expect("a trunk")].pos;
    let r = radius * 0.93;
    let along = (last - first).normalize_or(DVec3::X);
    let lift = |p: DVec3| DVec3::new(p.x, r, p.z);
    PropCollider::Capsule {
        a: lift(first + along * r),
        b: lift(last - along * r),
        radius: r,
    }
}
