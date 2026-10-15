//! Space colonisation (Runions 2007) inside an art-directed crown envelope,
//! for broadleaves and shrubs.
//!
//! Attraction points are scattered in a few masses inside the envelope
//! rather than evenly, so the crown grows into clumps with sky between them.
//! Each shoot carries a random walk that its children inherit and perturb,
//! which bends whole limbs coherently where independent jitter per node
//! would only make them shiver.
use ashlar::glam::{DQuat, DVec2, DVec3};

use crate::TreeRecipe;
use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::Form;

/// The crown's outline: a superellipsoid split at its widest point, with a
/// lean and a few low harmonics of lobing around the axis.
#[derive(Clone, Debug)]
pub(crate) struct Envelope {
    pub base: f64,
    pub top: f64,
    pub widest: f64,
    pub radius: f64,
    top_exp: f64,
    bottom_exp: f64,
    lean: DVec2,
    lobes: [(f64, f64); 3],
}

impl Envelope {
    pub(crate) fn new(form: &Form) -> Self {
        let base = form.crown_base;
        let top = form.height;
        Self {
            base,
            top,
            widest: base + (top - base) * form.widest,
            radius: form.crown_radius,
            top_exp: form.top,
            bottom_exp: form.bottom,
            lean: form.lean,
            lobes: form.lobes,
        }
    }

    fn axis(&self, y: f64) -> DVec2 {
        self.lean * ((y - self.base) / (self.top - self.base)).clamp(0.0, 1.0)
    }

    /// The crown's middle, where outward is measured from.
    pub(crate) fn centre(&self) -> DVec3 {
        let axis = self.axis(self.widest);
        DVec3::new(axis.x, self.widest, axis.y)
    }

    /// 1 on the envelope, less inside, more outside.
    pub(crate) fn rho(&self, p: DVec3) -> f64 {
        let axis = self.axis(p.y);
        let offset = DVec2::new(p.x, p.z) - axis;
        let t = ((p.y - self.base) / (self.top - self.base)).clamp(0.0, 1.0);
        let angle = offset.y.atan2(offset.x);
        let mut lobe = 1.0;
        for (k, (amplitude, phase)) in (1..).zip(self.lobes) {
            lobe += amplitude * (f64::from(k) * angle + phase + 0.9 * f64::from(k) * t).cos();
        }
        let horizontal = offset.length() / (self.radius * lobe.max(0.5));
        let dy = p.y - self.widest;
        let (e, q) = if dy > 0.0 {
            (dy / (self.top - self.widest), self.top_exp)
        } else {
            (-dy / (self.widest - self.base), self.bottom_exp)
        };
        (horizontal * horizontal + e.abs().powf(q)).sqrt()
    }
}

pub(crate) fn grow(recipe: &TreeRecipe, form: &Form) -> (Skeleton, Envelope) {
    let envelope = Envelope::new(form);
    let mut rng = form.rng("colonise");
    let attractors = scatter(&envelope, form, &mut rng);
    let mut skeleton = if form.stems > 1 {
        stems(recipe, form, &mut rng)
    } else {
        trunk(recipe, form, &envelope, &attractors, &mut rng)
    };
    colonise(&mut skeleton, attractors, recipe, form, &envelope, &mut rng);
    (skeleton, envelope)
}

/// Attraction points in `form.clumps` masses inside the envelope, plus a
/// sparse few in the outer shell so the masses stay joined.
fn scatter(envelope: &Envelope, form: &Form, rng: &mut Rng) -> Vec<DVec3> {
    let reach = envelope.radius * 1.6 + form.lean.length();
    let sample_envelope = |rng: &mut Rng| {
        DVec3::new(
            rng.signed() * reach,
            rng.range(envelope.base, envelope.top),
            rng.signed() * reach,
        )
    };
    let clump_radius = envelope.radius * 0.85 / f64::from(form.clumps).cbrt();
    let mut clumps: Vec<(DVec3, f64)> = Vec::new();
    let mut tries = 0;
    while clumps.len() < form.clumps as usize && tries < 20_000 {
        tries += 1;
        let p = sample_envelope(rng);
        let rho = envelope.rho(p);
        if !(0.45..=0.9).contains(&rho) {
            continue;
        }
        // Keep clumps apart, with sky between them, as long as they fit.
        let apart = clump_radius * if tries < 8_000 { 1.5 } else { 1.0 };
        if clumps.iter().any(|(c, _)| c.distance(p) < apart) {
            continue;
        }
        clumps.push((p, clump_radius * rng.range(0.8, 1.2)));
    }
    let total = form.attractors as usize;
    let mut points = Vec::with_capacity(total);
    let mut tries = 0;
    while points.len() < total && tries < total * 200 {
        tries += 1;
        let p = if rng.chance(0.03) || clumps.is_empty() {
            let p = sample_envelope(rng);
            if envelope.rho(p) < 0.55 {
                continue;
            }
            p
        } else {
            let pick = rng.between(0, u32::try_from(clumps.len() - 1).expect("a few clumps"));
            let (centre, radius) = clumps[pick as usize];
            centre + rng.in_ball() * radius
        };
        if envelope.rho(p) <= 1.0 && p.y >= envelope.base {
            points.push(p);
        }
    }
    points
}

fn tropism(recipe: &TreeRecipe) -> DVec3 {
    DVec3::from_array(recipe.alien.tropism_target).normalize_or(DVec3::Y)
}

/// A single trunk from below the ground to the crown, with an optional lost
/// leader: a dead stub carrying on where the old leader went, and the trunk
/// turning where a side shoot took over.
fn trunk(
    recipe: &TreeRecipe,
    form: &Form,
    envelope: &Envelope,
    attractors: &[DVec3],
    rng: &mut Rng,
) -> Skeleton {
    let d = form.segment;
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.3, 0.0));
    let mut at = 0;
    let mut dir = DVec3::Y;
    let mut wander = rng.direction();
    let lean = DVec3::new(form.lean.x, 0.0, form.lean.y).normalize_or_zero();
    let mut change = form
        .leader_change
        .then(|| form.trunk_height * rng.range(0.45, 0.85));
    let up = tropism(recipe);
    let influence = d * INFLUENCE;
    let mut extra = 0;
    loop {
        let pos = skeleton.nodes[at].pos;
        let reached = pos.y >= form.trunk_height;
        if reached {
            // The crown has to be in reach of the trunk's top, or nothing
            // grows; a low crown is reached at once.
            let near = attractors.iter().any(|a| a.distance(pos) < influence);
            if near || extra > 60 || pos.y > envelope.widest {
                break;
            }
            extra += 1;
            dir = (dir + (envelope.centre() - pos).normalize_or(DVec3::Y) * 0.2).normalize();
        }
        wander = (wander + rng.direction() * 0.35).normalize();
        // Steer towards a leaning target rather than adding the lean each
        // step, which would curl the trunk over.
        let target = (up + lean * (form.lean.length() / form.height) * 1.5).normalize();
        dir = (dir + wander * 0.06 * recipe.growth.wander + (target - dir) * 0.15).normalize();
        if let Some(y) = change
            && pos.y >= y
        {
            change = None;
            let mut stub = at;
            let mut first = None;
            for _ in 0..rng.between(2, 3) {
                let p = skeleton.nodes[stub].pos + dir * d;
                stub = skeleton.add(stub, p, wander);
                first.get_or_insert(stub);
            }
            if let Some(first) = first {
                skeleton.mark_dead(first);
            }
            skeleton.nodes[stub].capped = true;
            let turn = rng.range(0.0, core::f64::consts::TAU);
            let side = DVec3::new(turn.cos(), 0.0, turn.sin());
            let axis = dir.cross(side).normalize_or(DVec3::X);
            dir = DQuat::from_axis_angle(axis, -rng.range(0.3, 0.45)) * dir;
        }
        at = skeleton.add(at, pos + dir * d, wander);
    }
    skeleton
}

/// A shrub's stems, fanning out from the root at the phyllotaxis angle.
fn stems(recipe: &TreeRecipe, form: &Form, rng: &mut Rng) -> Skeleton {
    let d = form.segment;
    let mut skeleton = Skeleton::root(DVec3::new(0.0, -0.1, 0.0));
    skeleton.multi_stem = true;
    let turn = recipe.alien.phyllotaxis_deg.to_radians();
    let start = rng.range(0.0, core::f64::consts::TAU);
    for k in 0..form.stems {
        let azimuth = start + f64::from(k) * turn + rng.signed() * 0.3;
        let out = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
        let tilt = rng.range(0.15, 0.6);
        let mut dir = (DVec3::Y * tilt.cos() + out * tilt.sin()).normalize();
        let mut wander = rng.direction();
        let mut at = skeleton.add(
            0,
            out * rng.range(0.02, 0.07) + DVec3::new(0.0, -0.08, 0.0),
            wander,
        );
        for _ in 0..rng.between(2, 4) {
            wander = (wander + rng.direction() * 0.3).normalize();
            dir = (dir + wander * 0.1).normalize();
            let p = skeleton.nodes[at].pos + dir * d;
            at = skeleton.add(at, p, wander);
        }
    }
    skeleton
}

/// Influence radius of an attraction point, in segments.
const INFLUENCE: f64 = 7.0;
/// Distance at which a shoot has reached an attraction point, in segments.
const KILL: f64 = 1.7;

/// A uniform grid of node indices, for nearest-node queries.
struct Grid {
    origin: DVec3,
    cell: f64,
    dims: [usize; 3],
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(min: DVec3, max: DVec3, cell: f64) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let dim = |span: f64| ((span / cell).ceil() as usize).max(1);
        let size = max - min;
        let dims = [dim(size.x), dim(size.y), dim(size.z)];
        Self {
            origin: min,
            cell,
            dims,
            cells: vec![Vec::new(); dims[0] * dims[1] * dims[2]],
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn coord(&self, p: DVec3) -> [usize; 3] {
        let q = (p - self.origin) / self.cell;
        let clamp = |v: f64, dim: usize| (v.max(0.0) as usize).min(dim - 1);
        [
            clamp(q.x, self.dims[0]),
            clamp(q.y, self.dims[1]),
            clamp(q.z, self.dims[2]),
        ]
    }

    fn insert(&mut self, p: DVec3, index: usize) {
        let [x, y, z] = self.coord(p);
        self.cells[(z * self.dims[1] + y) * self.dims[0] + x].push(index);
    }

    /// Every index stored within one cell of `p`'s cell.
    fn near(&self, p: DVec3, mut visit: impl FnMut(usize)) {
        let [x, y, z] = self.coord(p);
        for cz in z.saturating_sub(1)..=(z + 1).min(self.dims[2] - 1) {
            for cy in y.saturating_sub(1)..=(y + 1).min(self.dims[1] - 1) {
                for cx in x.saturating_sub(1)..=(x + 1).min(self.dims[0] - 1) {
                    for &i in &self.cells[(cz * self.dims[1] + cy) * self.dims[0] + cx] {
                        visit(i);
                    }
                }
            }
        }
    }
}

fn colonise(
    skeleton: &mut Skeleton,
    mut attractors: Vec<DVec3>,
    recipe: &TreeRecipe,
    form: &Form,
    envelope: &Envelope,
    rng: &mut Rng,
) {
    let d = form.segment;
    let influence = d * INFLUENCE;
    let kill = d * KILL;
    let reach = envelope.radius * 2.0 + form.lean.length() + influence;
    let mut grid = Grid::new(
        DVec3::new(-reach, -1.0, -reach),
        DVec3::new(reach, form.height * 1.3 + influence, reach),
        influence,
    );
    for (index, node) in skeleton.nodes.iter().enumerate() {
        grid.insert(node.pos, index);
    }
    let up = tropism(recipe);
    let tropism = recipe.growth.tropism;
    let wander = recipe.growth.wander;
    for _ in 0..600 {
        let count = skeleton.nodes.len();
        let mut pull = vec![DVec3::ZERO; count];
        let mut pulled = vec![false; count];
        attractors.retain(|&a| {
            let mut best = (f64::INFINITY, usize::MAX);
            grid.near(a, |i| {
                let dist = skeleton.nodes[i].pos.distance(a);
                // Ties go to the lower index, so the result is independent
                // of the order the grid holds its nodes in.
                if dist < best.0 || (dist <= best.0 && i < best.1) {
                    best = (dist, i);
                }
            });
            let (dist, nearest) = best;
            if dist < kill {
                return false;
            }
            // Dead wood grows nothing.
            if dist < influence && !skeleton.nodes[nearest].dead {
                pull[nearest] += (a - skeleton.nodes[nearest].pos) / dist;
                pulled[nearest] = true;
            }
            true
        });
        let mut grown = 0;
        for index in 0..count {
            // A bud sprouts once: a node carries its continuation and at most
            // one side shoot, or the trunk grows a comb of parallel laterals.
            if !pulled[index] || (index > 0 && skeleton.nodes[index].children.len() >= 2) {
                continue;
            }
            let node = &skeleton.nodes[index];
            let w = (node.wander + rng.direction() * 0.3).normalize();
            let dir = (pull[index].normalize_or(DVec3::Y) + up * tropism + w * wander * 0.5)
                .normalize_or(DVec3::Y);
            let p = node.pos + dir * d;
            let mut crowded = false;
            grid.near(p, |i| {
                crowded |= skeleton.nodes[i].pos.distance(p) < d * 0.45;
            });
            if crowded {
                continue;
            }
            let new = skeleton.add(index, p, w);
            grid.insert(p, new);
            grown += 1;
        }
        if grown == 0 || skeleton.nodes.len() > 9_000 {
            break;
        }
    }
}

/// Shade pruning and, on an old tree, breakage: the lowest, most shaded
/// side branches die back to stubs; an old tree has lost limbs to storms and
/// carries one dead limb whose twigs are gone. Radii are already set, so a
/// stub keeps the girth it grew to.
pub(crate) fn prune(skeleton: &mut Skeleton, form: &Form, envelope: &Envelope) {
    let mut rng = form.rng("prune");
    skeleton.compute_ages();
    let branches = skeleton.branches();
    let trunk_radius = skeleton.nodes[0].radius.max(form.trunk_radius);
    let tips = |skeleton: &Skeleton, from: usize| {
        let mut exposure = 0.0;
        let mut count = 0.0;
        let mut stack = vec![from];
        while let Some(i) = stack.pop() {
            let node = &skeleton.nodes[i];
            if node.children.is_empty() {
                let lift =
                    ((node.pos.y - envelope.base) / (envelope.top - envelope.base)).clamp(0.0, 1.0);
                exposure += envelope.rho(node.pos).min(1.2) * 0.6 + lift * 0.4;
                count += 1.0;
            }
            stack.extend(node.children.iter().copied());
        }
        if count > 0.0 { exposure / count } else { 0.0 }
    };
    // Shade stubs: small side branches low in the crown.
    let mut candidates: Vec<(f64, usize)> = branches
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            b.order >= 1
                && b.nodes.len() >= 4
                && skeleton.nodes[b.nodes[1]].radius < trunk_radius * 0.45
                && skeleton.nodes[b.nodes[0]].pos.y
                    < envelope.base + (envelope.top - envelope.base) * 0.5
        })
        .map(|(i, b)| (tips(skeleton, b.nodes[1]) + rng.unit() * 0.1, i))
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for &(_, index) in candidates.iter().take(form.dead_stubs as usize) {
        let branch = &branches[index];
        let keep = rng
            .between(1, 3)
            .min(u32::try_from(branch.nodes.len() - 2).unwrap_or(1)) as usize;
        let end = branch.nodes[keep];
        skeleton.cut_after(end);
        skeleton.mark_dead(branch.nodes[1]);
    }
    // Storm breaks: first-order limbs, never the biggest.
    let mut limbs: Vec<(f64, usize)> = branches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.order == 1 && b.nodes.len() >= 7)
        .map(|(i, b)| (b.length, i))
        .collect();
    limbs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut used = Vec::new();
    for &(_, index) in limbs.iter().skip(1).step_by(2).take(form.broken as usize) {
        let branch = &branches[index];
        let n = branch.nodes.len();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let at = ((n as f64 * rng.range(0.35, 0.6)) as usize).clamp(2, n - 2);
        skeleton.cut_after(branch.nodes[at]);
        used.push(index);
    }
    // One dead limb, bare of its finest twigs.
    for &(_, index) in limbs
        .iter()
        .skip(2)
        .filter(|(_, i)| !used.contains(i))
        .take(form.dead_limbs as usize)
    {
        let first = branches[index].nodes[1];
        skeleton.mark_dead(first);
        let mut stack = vec![first];
        while let Some(i) = stack.pop() {
            if skeleton.nodes[i].age < form.segment * 2.5 && i != first {
                skeleton.remove_subtree(i);
                continue;
            }
            stack.extend(skeleton.nodes[i].children.iter().copied());
        }
    }
    skeleton.compact();
    skeleton.compute_ages();
}
