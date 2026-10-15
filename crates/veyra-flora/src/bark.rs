//! Branches as tubes, meshed straight into a `TriangleMesh`.
//!
//! Rings are placed where a branch turns or has run long enough for its
//! radius, carried along by parallel transport so a tube never twists where
//! a Frenet frame would flip. Sides scale with the radius and change along
//! a branch, stitched ring to ring. Bark `u` wraps a whole number of
//! repeats of the bark tile around every ring, so the seam never shows,
//! and `v` is metres along the branch from a random start per branch.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::DVec3;
use ashlar::{FaceSource, TriangleMesh};

use crate::skeleton::{Branch, Skeleton};
use crate::species::Form;
use crate::{Superformula, TreeRecipe};

/// How finely one level meshes bark.
#[derive(Clone, Debug)]
pub(crate) struct BarkDetail {
    /// Sides of the trunk; thinner branches get fewer, never under three.
    pub max_sides: u32,
    /// Turn in radians past which a ring is forced.
    pub turn: f64,
    /// Longest stretch between rings, in radii.
    pub spacing: f64,
    /// Longest stretch between rings, metres.
    pub spacing_max: f64,
    /// Shortest stretch between rings that is not forced by a turn, metres.
    pub spacing_min: f64,
    /// Branches thinner than this at their base are pruned with everything
    /// beyond them, metres.
    pub min_radius: f64,
    /// Extra rings for the root flare and the branch collars.
    pub fine_base: bool,
}

/// A bark mesh and the per-vertex data the shade pass completes.
#[derive(Default)]
pub(crate) struct BarkMesh {
    pub mesh: TriangleMesh,
    /// The pivot each vertex swings about.
    pub pivot: Vec<u32>,
    pub curvature: Vec<f64>,
    /// Vertex ranges of each ring, with the ring's `u` repeat count; the
    /// tests hold the repeats to whole numbers.
    pub rings: Vec<(usize, usize, f64)>,
}

/// Everything about the tree the mesher reads.
pub(crate) struct Tree<'a> {
    pub recipe: &'a TreeRecipe,
    pub form: &'a Form,
    pub skeleton: &'a Skeleton,
    pub branches: &'a [Branch],
    /// Pivot per branch.
    pub pivots: &'a [u32],
    pub tile: [f64; 2],
    /// Whether the trunk flares into the ground; a log lies on it instead.
    pub root_flare: bool,
    /// Whether the trunk's first end is capped too, as a log's broken base
    /// is.
    pub cap_base: bool,
    /// How far a broken end's middle stands out of its cap, in radii, so it
    /// reads as splintered rather than sawn.
    pub splinter: f64,
}

pub(crate) fn mesh(tree: &Tree<'_>, detail: &BarkDetail) -> BarkMesh {
    let mut out = BarkMesh::default();
    let mut rng = tree.form.rng("bark");
    let trunk_radius = tree.form.trunk_radius.max(1e-3);
    let section = Section::new(&tree.recipe.alien.section);
    let mut skipped = vec![false; tree.branches.len()];
    let buttress_phase = rng.range(0.0, core::f64::consts::TAU);
    for (index, branch) in tree.branches.iter().enumerate() {
        // Every branch draws its offset, meshed or not, so a coarser level's
        // bark lines up with the nearer one's.
        let v_offset = if index == 0 { 0.0 } else { rng.range(0.0, 8.0) };
        let gnarl_phase = rng.range(0.0, core::f64::consts::TAU);
        let parent_skipped = branch.parent.is_some_and(|p| skipped[p]);
        let own = branch
            .nodes
            .get(1)
            .map_or(0.0, |&n| tree.skeleton.nodes[n].radius);
        if parent_skipped || (index > 0 && own < detail.min_radius) {
            skipped[index] = true;
            continue;
        }
        // A shrub's root is a point with its stems on it: nothing to mesh,
        // but its stems are.
        if branch.nodes.len() < 2 {
            continue;
        }
        let path = Path::new(tree, branch);
        if path.length < 1e-4 {
            continue;
        }
        let rings = ring_positions(
            &path,
            detail,
            index == 0 && tree.root_flare && !tree.skeleton.multi_stem,
        );
        // Drawn only where ends splinter, so the trees' streams are as they
        // were.
        let splinters = if tree.splinter > 0.0 {
            [rng.range(-0.3, 1.0), rng.range(-0.3, 1.0)].map(|k| k * tree.splinter)
        } else {
            [0.0; 2]
        };
        let context = RingContext {
            tree,
            section: &section,
            trunk: index == 0,
            trunk_radius,
            base_radius: path.radius[0],
            v_offset,
            gnarl_phase,
            buttress_phase,
            pivot: tree.pivots[index],
            detail,
        };
        let last = *branch.nodes.last().expect("a branch has nodes");
        let capped = tree.skeleton.nodes[last].capped;
        let ends = Ends {
            base: index == 0 && tree.cap_base,
            tip: capped,
            splinters,
        };
        emit(&mut out, &path, &rings, &context, &ends);
    }
    let triangles = out.mesh.indices.len() / 3;
    out.mesh.sources = vec![FaceSource::BODY; triangles];
    out
}

/// A branch's centreline with its radius, by arc length.
struct Path {
    points: Vec<DVec3>,
    radius: Vec<f64>,
    arc: Vec<f64>,
    length: f64,
}

impl Path {
    fn new(tree: &Tree<'_>, branch: &Branch) -> Self {
        let nodes = &tree.skeleton.nodes;
        let mut points: Vec<DVec3> = Vec::with_capacity(branch.nodes.len());
        let mut radius: Vec<f64> = Vec::with_capacity(branch.nodes.len());
        let is_trunk = branch.parent.is_none();
        for (k, &n) in branch.nodes.iter().enumerate() {
            if k == 0 && !is_trunk {
                // The child sinks into its parent: its first ring sits half
                // way between the parent's axis and its surface.
                let attach = nodes[n].pos;
                let first = nodes[branch.nodes[1]].pos;
                let dir = (first - attach).normalize_or(DVec3::Y);
                let parent_dir = tree.skeleton.direction(n);
                let sine = dir.cross(parent_dir).length().max(0.35);
                let depth = (0.5 * nodes[n].radius / sine).min(attach.distance(first) * 0.6);
                points.push(attach + dir * depth);
                radius.push(nodes[branch.nodes[1]].radius);
            } else {
                points.push(nodes[n].pos);
                radius.push(nodes[n].radius);
            }
        }
        let mut arc = vec![0.0; points.len()];
        for k in 1..points.len() {
            arc[k] = arc[k - 1] + points[k].distance(points[k - 1]);
        }
        let length = *arc.last().expect("points");
        Self {
            points,
            radius,
            arc,
            length,
        }
    }

    /// Position, tangent and radius at arc length `s`.
    fn at(&self, s: f64) -> (DVec3, DVec3, f64) {
        let k = match self.arc.binary_search_by(|a| a.total_cmp(&s)) {
            Ok(k) => k.min(self.points.len() - 2),
            Err(k) => k.saturating_sub(1).min(self.points.len() - 2),
        };
        let span = (self.arc[k + 1] - self.arc[k]).max(1e-9);
        let t = ((s - self.arc[k]) / span).clamp(0.0, 1.0);
        let p = self.points[k].lerp(self.points[k + 1], t);
        let r = self.radius[k] + (self.radius[k + 1] - self.radius[k]) * t;
        let seg = (self.points[k + 1] - self.points[k]).normalize_or(DVec3::Y);
        // At a vertex the tangent is the mean of the two segments meeting
        // there, so rings stay square to the bend.
        let tangent = if t < 1e-6 && k > 0 {
            (seg + (self.points[k] - self.points[k - 1]).normalize_or(seg)).normalize_or(seg)
        } else if t > 1.0 - 1e-6 && k + 2 < self.points.len() {
            (seg + (self.points[k + 2] - self.points[k + 1]).normalize_or(seg)).normalize_or(seg)
        } else {
            seg
        };
        (p, tangent, r)
    }
}

/// Arc lengths of the rings: the ends, the collar or root flare, and the
/// nodes where the branch turns or has run long enough.
fn ring_positions(path: &Path, detail: &BarkDetail, trunk: bool) -> Vec<f64> {
    let mut rings = vec![0.0];
    let r0 = path.radius[0];
    if detail.fine_base {
        if trunk {
            // The root flare: rings close together where the lobes change.
            let ground = -path.points[0].y;
            for h in [0.0, 0.15, 0.35, 0.6, 0.95, 1.4, 2.0] {
                rings.push(ground + h);
            }
        } else {
            rings.push(r0 * 0.6);
            rings.push(r0 * 1.5);
        }
    } else if trunk {
        let ground = -path.points[0].y;
        rings.push(ground);
        rings.push(ground + 0.5);
    }
    let mut last_s = 0.0;
    let mut last_dir = (path.points[1] - path.points[0]).normalize_or(DVec3::Y);
    for k in 1..path.points.len() - 1 {
        let next_dir = (path.points[k + 1] - path.points[k]).normalize_or(last_dir);
        let spacing = (path.radius[k] * detail.spacing)
            .max(detail.spacing_min)
            .min(detail.spacing_max);
        // Thin wood may bend further between rings: it is seen as a line.
        let turn = detail.turn * (1.0 + detail.spacing_min / (path.radius[k] * 20.0)).min(3.0);
        let turned = last_dir.angle_between(next_dir) > turn;
        let long = path.arc[k + 1] - last_s > spacing;
        if turned || long {
            rings.push(path.arc[k]);
            last_s = path.arc[k];
            last_dir = next_dir;
        }
    }
    rings.push(path.length);
    rings.retain(|&s| (0.0..=path.length).contains(&s));
    rings.sort_by(f64::total_cmp);
    let min_gap = (path.length * 0.01).min(0.02);
    let mut out: Vec<f64> = Vec::with_capacity(rings.len());
    for s in rings {
        if out.last().is_none_or(|&l| s - l > min_gap) {
            out.push(s);
        }
    }
    if out.last().is_some_and(|&l| l < path.length) {
        if out.len() > 1 && path.length - out[out.len() - 1] < min_gap {
            out.pop();
        }
        out.push(path.length);
    }
    out
}

/// The stem cross-section, normalised to a largest radius of one.
struct Section<'a> {
    formula: &'a Superformula,
    scale: f64,
}

impl<'a> Section<'a> {
    fn new(formula: &'a Superformula) -> Self {
        let max = (0..128)
            .map(|i| formula.radius(f64::from(i) / 128.0 * core::f64::consts::TAU))
            .fold(0.0, f64::max);
        Self {
            formula,
            scale: if max > 0.0 { 1.0 / max } else { 1.0 },
        }
    }

    fn at(&self, theta: f64) -> f64 {
        self.formula.radius(theta) * self.scale
    }
}

struct RingContext<'a> {
    tree: &'a Tree<'a>,
    section: &'a Section<'a>,
    trunk: bool,
    trunk_radius: f64,
    base_radius: f64,
    v_offset: f64,
    gnarl_phase: f64,
    buttress_phase: f64,
    pivot: u32,
    detail: &'a BarkDetail,
}

impl RingContext<'_> {
    /// Radius around the ring at angle `theta`, arc length `s`, height `y`,
    /// before the ring's own radius: collar, root flare, buttress lobes,
    /// cross-section and gnarl.
    fn shape(&self, theta: f64, s: f64, y: f64, r: f64) -> f64 {
        let form = self.tree.form;
        let mut m = self.section.at(theta);
        if self.trunk && self.tree.root_flare && !self.tree.skeleton.multi_stem {
            let h = y.max(0.0);
            let k = (0.55 * (form.height / 10.0).sqrt()).max(0.15);
            m *= 1.0 + 0.35 * (-h / k).exp();
            if form.buttress > 0 {
                let a = self.tree.recipe.growth.buttress_amplitude;
                let lobe = (f64::from(form.buttress) * theta + self.buttress_phase)
                    .cos()
                    .max(0.0)
                    .powf(2.0);
                m *= 1.0 + a * (-h / (k * 1.3)).exp() * lobe;
            }
        } else if s < self.base_radius * 1.5 {
            let f = 1.0 - s / (self.base_radius * 1.5);
            m *= 1.0 + 0.3 * f * f;
        }
        if form.gnarl > 0.0 && r > 0.04 {
            let g = (3.0 * theta + s * 1.7 + self.gnarl_phase).sin()
                * (s * 0.9 + self.gnarl_phase * 2.0).sin();
            m *= 1.0 + 0.08 * form.gnarl * g;
        }
        m
    }

    fn sides(&self, r: f64) -> u32 {
        let ratio = (r / self.trunk_radius).clamp(0.0, 1.0).sqrt();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sides = (f64::from(self.detail.max_sides) * ratio).round() as u32;
        sides.clamp(3, self.detail.max_sides.max(3))
    }
}

/// How a tube's two ends close: a cap on a break, a point on a live tip.
struct Ends {
    base: bool,
    tip: bool,
    /// How far each cap's middle stands out, metres over the radius.
    splinters: [f64; 2],
}

fn emit(out: &mut BarkMesh, path: &Path, rings: &[f64], context: &RingContext<'_>, ends: &Ends) {
    let tile = context.tree.tile;
    let mut normal = {
        let (_, t, _) = path.at(0.0);
        let reference = if t.y.abs() < 0.9 { DVec3::Y } else { DVec3::X };
        (reference - t * reference.dot(t)).normalize()
    };
    let mut prev_tangent = path.at(0.0).1;
    let mut previous: Option<(usize, u32)> = None;
    let mut last = (DVec3::ZERO, DVec3::Y, 0.0, 0.0);
    for &s in rings {
        let (centre, tangent, r) = path.at(s);
        // Parallel transport: turn the frame by the rotation that takes the
        // last tangent to this one.
        let axis = prev_tangent.cross(tangent);
        if axis.length_squared() > 1e-14 {
            let angle = prev_tangent.angle_between(tangent);
            normal = ashlar::glam::DQuat::from_axis_angle(axis.normalize(), angle) * normal;
        }
        normal =
            (normal - tangent * normal.dot(tangent)).normalize_or(tangent.any_orthonormal_vector());
        prev_tangent = tangent;
        let binormal = tangent.cross(normal);
        let sides = context.sides(r);
        let repeats = ((core::f64::consts::TAU * r) / tile[0]).round().max(1.0);
        let start = out.mesh.positions.len();
        let v = context.v_offset + s;
        let step = core::f64::consts::TAU / f64::from(sides);
        for j in 0..=sides {
            let theta = f64::from(j) * step;
            let shape = |t: f64| r * context.shape(t, s, centre.y, r);
            let radial = normal * theta.cos() + binormal * theta.sin();
            let rr = shape(theta);
            let d = 1e-3;
            let (r_minus, r_plus) = (shape(theta - d), shape(theta + d));
            let derivative = (r_plus - r_minus) / (2.0 * d);
            let around = (binormal * theta.cos() - normal * theta.sin()) * rr + radial * derivative;
            let mut n = around.cross(tangent).normalize_or(radial);
            if n.dot(radial) < 0.0 {
                n = -n;
            }
            out.mesh.positions.push(centre + radial * rr);
            out.mesh.normals.push(n);
            out.mesh
                .uvs
                .push([f64::from(j) / f64::from(sides) * repeats * tile[0], v]);
            out.pivot.push(context.pivot);
            // Ridges of the lobes are positive, the valleys and the crease
            // where a branch meets its parent negative.
            let second = (r_plus + r_minus - 2.0 * rr) / (d * d * rr.max(1e-6));
            let mut c =
                0.25 * (1.0 - (r / context.trunk_radius).clamp(0.0, 1.0).sqrt()) - second * 0.08;
            if !context.trunk && s < context.base_radius * 1.5 {
                let f = 1.0 - s / (context.base_radius * 1.5);
                c -= 0.8 * f * f;
            }
            out.curvature.push(c.clamp(-1.0, 1.0));
        }
        out.rings.push((start, sides as usize + 1, repeats));
        if let Some((prev_start, prev_sides)) = previous {
            stitch(&mut out.mesh, prev_start, prev_sides, start, sides);
        } else if ends.base {
            let end = End {
                centre,
                outward: -tangent,
                r,
                v,
                capped: true,
                splinter: ends.splinters[0],
            };
            close(out, start, sides, &end, normal, context, true);
        }
        previous = Some((start, sides));
        last = (centre, tangent, r, v);
    }
    if let Some((ring, sides)) = previous {
        let (centre, tangent, r, v) = last;
        let end = End {
            centre,
            outward: tangent,
            r,
            v,
            capped: ends.tip,
            splinter: ends.splinters[1],
        };
        close(out, ring, sides, &end, normal, context, false);
    }
}

/// Join two rings of possibly different side counts, walking both by their
/// fraction around so the triangles stay even.
fn stitch(mesh: &mut TriangleMesh, a: usize, na: u32, b: usize, nb: u32) {
    let u32_of = |i: usize| u32::try_from(i).expect("a tree is far under 4 billion vertices");
    let (mut i, mut j) = (0_u32, 0_u32);
    while i < na || j < nb {
        let next_a = f64::from(i + 1) / f64::from(na);
        let next_b = f64::from(j + 1) / f64::from(nb);
        let ai = u32_of(a + i as usize);
        let bj = u32_of(b + j as usize);
        if j >= nb || (i < na && next_a <= next_b) {
            // Advance on ring a: triangle (a_i, a_i+1, b_j).
            mesh.indices.extend([ai, ai + 1, bj]);
            i += 1;
        } else {
            mesh.indices.extend([ai, bj + 1, bj]);
            j += 1;
        }
    }
}

/// One end of a tube: where it is, which way is out, and how it closes.
struct End {
    centre: DVec3,
    outward: DVec3,
    r: f64,
    v: f64,
    capped: bool,
    splinter: f64,
}

/// Close a branch's end: a cap with its own normals on a break, its middle
/// pushed out into a splinter where the species breaks that way, or a point
/// just past the last ring on a live tip. `reverse` winds the fan the other
/// way round, for the first end of a tube, which faces back along it.
fn close(
    out: &mut BarkMesh,
    ring: usize,
    sides: u32,
    end: &End,
    normal: DVec3,
    context: &RingContext<'_>,
    reverse: bool,
) {
    let tile = context.tree.tile;
    let u32_of = |i: usize| u32::try_from(i).expect("a tree is far under 4 billion vertices");
    let fan = |out: &mut BarkMesh, centre: usize, rim: usize, j: usize| {
        let (a, b) = (u32_of(rim + j), u32_of(rim + j + 1));
        let c = u32_of(centre);
        out.mesh
            .indices
            .extend(if reverse { [c, b, a] } else { [c, a, b] });
    };
    let base = out.mesh.positions.len();
    if end.capped {
        out.mesh
            .positions
            .push(end.centre + end.outward * end.r * end.splinter);
        out.mesh.normals.push(end.outward);
        out.mesh.uvs.push([0.0, end.v]);
        out.pivot.push(context.pivot);
        out.curvature.push(0.0);
        for j in 0..=sides {
            let p = out.mesh.positions[ring + j as usize];
            let d = p - end.centre;
            out.mesh.positions.push(p);
            out.mesh.normals.push(end.outward);
            out.mesh.uvs.push([
                d.dot(normal) + 0.5 * tile[0],
                end.v + d.dot(end.outward.cross(normal)),
            ]);
            out.pivot.push(context.pivot);
            out.curvature.push(0.6);
        }
        for j in 0..sides as usize {
            fan(out, base, base + 1, j);
        }
    } else {
        out.mesh
            .positions
            .push(end.centre + end.outward * end.r * 1.5);
        out.mesh.normals.push(end.outward);
        out.mesh.uvs.push([0.0, end.v + end.r * 1.5]);
        out.pivot.push(context.pivot);
        out.curvature.push(0.3);
        for j in 0..sides as usize {
            fan(out, base, ring, j);
        }
    }
}
