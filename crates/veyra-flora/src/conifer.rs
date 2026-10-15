//! Conifers: a monopodial leader with whorls of branches, Weber and Penn in
//! spirit, where space colonisation would round the cone off.
//!
//! Whorls step up the leader at irregular spacing with the odd whorl
//! missing, turn by the phyllotaxis angle from one to the next, and carry
//! branches whose length follows their height in the crown, which is what
//! makes the cone (fir) or the high rounded crown (pine). Side shoots fan
//! out along each branch in its own plane. Below the crown the trunk keeps
//! the stubs of branches the shade killed.
use ashlar::glam::{DQuat, DVec3};

use crate::TreeRecipe;
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::{Age, Form};

pub(crate) fn grow(recipe: &TreeRecipe, form: &Form) -> Skeleton {
    let mut rng = form.rng("whorls");
    let d = form.segment;
    let up = DVec3::from_array(recipe.alien.tropism_target).normalize_or(DVec3::Y);
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.3, 0.0));

    // The leader, with its lean and, sometimes, a kink where it was lost.
    let lean = DVec3::new(form.lean.x, 0.0, form.lean.y);
    let lean_dir = lean.normalize_or_zero();
    let mut leader = vec![0_usize];
    let mut dir = DVec3::Y;
    let mut wander = rng.direction();
    let kink = form
        .leader_change
        .then(|| form.height * rng.range(0.55, 0.85));
    let mut kinked = false;
    while skeleton.nodes[*leader.last().expect("the root")].pos.y < form.height {
        let at = *leader.last().expect("the root");
        let pos = skeleton.nodes[at].pos;
        wander = (wander + rng.direction() * 0.3).normalize();
        let bias = lean_dir * (lean.length() / form.height) * 0.6;
        dir = (dir + wander * 0.03 * recipe.growth.wander + bias * 0.1 + up * 0.08).normalize();
        if let Some(y) = kink
            && !kinked
            && pos.y >= y
        {
            kinked = true;
            let turn = rng.range(0.0, core::f64::consts::TAU);
            dir = (dir + DVec3::new(turn.cos(), 0.0, turn.sin()) * 0.35).normalize();
        }
        leader.push(skeleton.add(at, pos + dir * d, wander));
    }

    bare_stubs(&mut skeleton, form, &leader, &mut rng);

    // Whorls.
    let turn = recipe.alien.phyllotaxis_deg.to_radians();
    let [fewest, most] = recipe.growth.whorl_branches;
    let gap = if recipe.foliage.tuft > 1 { 0.18 } else { 0.08 };
    let span = form.height - form.crown_base;
    let mut y = form.crown_base;
    let mut whorl = 0_u32;
    let mut azimuth = rng.range(0.0, core::f64::consts::TAU);
    let mut branches = Vec::new();
    while y < form.height - d * 1.5 {
        let t = ((y - form.crown_base) / span).clamp(0.0, 1.0);
        if whorl == 0 || !rng.chance(gap) {
            let at = node_at(&skeleton, &leader, y);
            let count = rng.between(fewest, most);
            for k in 0..count {
                if recipe.foliage.tuft > 1 && rng.chance(0.15) {
                    continue;
                }
                let a = azimuth
                    + f64::from(k) * core::f64::consts::TAU / f64::from(count)
                    + rng.signed() * 0.35;
                let out = DVec3::new(a.cos(), 0.0, a.sin());
                let length = branch_length(form, t, a)
                    * rng.range(0.8, 1.2)
                    * (1.0 + out.dot(lean) / form.crown_radius.max(0.1) * 0.5).clamp(0.5, 1.5);
                let pitch = if recipe.foliage.flatness > 0.5 {
                    -0.35 + 0.75 * t
                } else {
                    0.05 + 0.55 * t
                } + rng.signed() * 0.12;
                let start = (out * pitch.cos() + DVec3::Y * pitch.sin()).normalize();
                if length > d * 0.8 {
                    branches.push(limb(
                        &mut skeleton,
                        recipe,
                        form,
                        at,
                        start,
                        length,
                        &mut rng,
                    ));
                }
            }
            azimuth += turn + rng.signed() * 0.2;
        }
        whorl += 1;
        y += form.whorl_spacing * rng.range(0.7, 1.3) * (1.0 - 0.3 * t);
    }

    if form.age == Age::Old {
        weather(&mut skeleton, form, &branches, &mut rng);
    }
    skeleton.compute_ages();
    skeleton
}

/// Branch length by height in the crown: widest at `form.widest`, shrinking
/// to the top, with a lobed outline around the trunk.
fn branch_length(form: &Form, t: f64, azimuth: f64) -> f64 {
    let w = form.widest.clamp(0.05, 0.9);
    let profile = if t < w {
        0.65 + 0.35 * (t / w)
    } else {
        ((1.0 - t) / (1.0 - w))
            .max(0.0)
            .powf(1.0 / form.top.max(0.5))
    };
    let mut lobe = 1.0;
    for (k, (amplitude, phase)) in (1..).zip(form.lobes) {
        lobe += amplitude * (f64::from(k) * azimuth + phase + 1.3 * f64::from(k) * t).cos();
    }
    form.crown_radius * profile * lobe.max(0.4) + form.segment * 0.5
}

/// One whorl branch and its side shoots. Answers the branch's own nodes.
fn limb(
    skeleton: &mut Skeleton,
    recipe: &TreeRecipe,
    form: &Form,
    at: usize,
    start: DVec3,
    length: f64,
    rng: &mut Rng,
) -> Vec<usize> {
    let d = form.segment;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = ((length / d).round() as usize).max(1);
    let up = DVec3::Y;
    let mut wander = rng.direction();
    let mut dir = start;
    let mut nodes = Vec::with_capacity(steps);
    let mut prev = at;
    // Pines turn their tips up towards the light.
    let upturn = if recipe.foliage.flatness > 0.5 {
        0.0
    } else {
        recipe.growth.tropism
    };
    for step in 0..steps {
        #[allow(clippy::cast_precision_loss)]
        let s = step as f64 / steps as f64;
        wander = (wander + rng.direction() * 0.35).normalize();
        dir = (dir + wander * recipe.growth.wander * 0.15 + up * upturn * s * 0.4).normalize();
        let p = skeleton.nodes[prev].pos + dir * d;
        prev = skeleton.add(prev, p, wander);
        nodes.push(prev);
    }
    // Side shoots in the branch's plane, alternating sides.
    let laterals = recipe.growth.laterals;
    if laterals > 0.0 {
        let angle = recipe.growth.lateral_angle_deg.to_radians();
        let mut side = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let mut along = rng.range(0.25, 0.4) * length;
        while along < length * 0.85 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = ((along / d) as usize).min(nodes.len() - 1);
            let base = nodes[index];
            let parent_dir = skeleton.direction(base);
            let plane = parent_dir.cross(up).normalize_or(DVec3::X);
            let normal = plane.cross(parent_dir).normalize_or(DVec3::Y);
            let lateral =
                DQuat::from_axis_angle(normal, side * angle * rng.range(0.8, 1.15)) * parent_dir;
            let remaining = length - along;
            let reach = (remaining * 0.55).min(length * 0.45) * rng.range(0.7, 1.1);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = ((reach / (d * 0.8)).round() as usize).max(1);
            let mut prev = base;
            let mut dir = lateral;
            for _ in 0..count {
                wander = (wander + rng.direction() * 0.3).normalize();
                dir = (dir + wander * recipe.growth.wander * 0.1 + up * upturn * 0.1).normalize();
                let p = skeleton.nodes[prev].pos + dir * d * 0.8;
                prev = skeleton.add(prev, p, wander);
            }
            side = -side;
            along += rng.range(0.7, 1.3) / laterals;
        }
    }
    nodes
}

/// The leader node nearest a height.
fn node_at(skeleton: &Skeleton, leader: &[usize], y: f64) -> usize {
    leader
        .iter()
        .copied()
        .min_by(|&a, &b| {
            (skeleton.nodes[a].pos.y - y)
                .abs()
                .total_cmp(&(skeleton.nodes[b].pos.y - y).abs())
        })
        .expect("a leader")
}

/// Dead stubs on the bare trunk below the crown, where the shade killed
/// the lowest whorls.
fn bare_stubs(skeleton: &mut Skeleton, form: &Form, leader: &[usize], rng: &mut Rng) {
    let d = form.segment;
    let bare_low = (form.height * 0.08).max(0.6);
    if form.crown_base > bare_low + d {
        for _ in 0..form.dead_stubs {
            let y = rng.range(bare_low, form.crown_base);
            let at = node_at(skeleton, leader, y);
            let azimuth = rng.range(0.0, core::f64::consts::TAU);
            let out = DVec3::new(azimuth.cos(), rng.range(-0.25, 0.1), azimuth.sin()).normalize();
            let mut stub = at;
            let mut first = None;
            for _ in 0..rng.between(1, 3) {
                let p = skeleton.nodes[stub].pos + out * d * rng.range(0.6, 1.0);
                stub = skeleton.add(stub, p, out);
                first.get_or_insert(stub);
            }
            skeleton.mark_dead(first.expect("at least one segment"));
            skeleton.nodes[stub].capped = true;
        }
    }
}

/// An old tree has lost some limbs to storms and carries a dead one.
fn weather(skeleton: &mut Skeleton, form: &Form, branches: &[Vec<usize>], rng: &mut Rng) {
    let mut order: Vec<(u64, usize)> = (0..branches.len()).map(|i| (rng.next_u64(), i)).collect();
    order.sort_unstable();
    for &(_, i) in order.iter().take(form.broken as usize) {
        let limb = &branches[i];
        if limb.len() > 3 {
            skeleton.cut_after(limb[limb.len() / 2]);
        }
    }
    // The dead limb is a low one, where the shade is.
    let low = branches.iter().filter(|b| b.len() > 3).min_by(|a, b| {
        skeleton.nodes[a[0]]
            .pos
            .y
            .total_cmp(&skeleton.nodes[b[0]].pos.y)
    });
    if let Some(low) = low
        && form.dead_limbs > 0
    {
        skeleton.mark_dead(low[0]);
    }
    skeleton.compact();
}
