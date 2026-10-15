//! From a recipe to a prop: shape the field, sit it on the ground, mesh it
//! at three resolutions inside their triangle budgets, then give every
//! vertex its UVs, shade data and tint and the prop its hulls.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, h"
)]
use ashlar::glam::DVec3;
use ashlar::prop::{Prop, PropCollider, PropLevel, PropPiece};
use ashlar::{ConvexSolid, FaceSource, TriangleMesh, VertexData};

use crate::field::Rock;
use crate::hull::{hull_points, rough_hull};
use crate::mesher::{self, Raw};
use crate::noise::Noise;
use crate::rng::{Rng, hash};
use crate::{GrownRock, RockKind, RockRecipe, shape};

/// The moss colour the tint mixes towards, linear RGB.
pub(crate) const MOSS: [f64; 3] = [0.35, 0.45, 0.2];

/// Most hulls a cliff piece is split into.
pub(crate) const MAX_HULLS: usize = 4;

pub(crate) fn build(recipe: &RockRecipe, variant: u32) -> GrownRock {
    let seed = Rng::new(hash(recipe.species)).fork(&format!("variant {variant}"));
    let mut field = shape::shape(recipe, variant, &mut seed.fork("shape"));

    // A coarse probe finds where the shape really is, so the level grids
    // are tight and their steps can be predicted.
    let loose = field.bounds();
    let probe_step = (loose[1] - loose[0]).max_element() / 36.0;
    let probe = mesher::mesh(&field, loose, probe_step);
    // The probe's box, grown by a few of its steps for anything thin it
    // stepped over, but never past the analytic bounds: a level grid that
    // clipped the surface would leave a hole.
    let [low, high] = extent(&probe.positions);
    let margin = DVec3::splat(probe_step * 4.0);
    let bounds = [(low - margin).max(loose[0]), (high + margin).min(loose[1])];

    let detail = recipe.detail;
    let budgets = detail.budget();
    let mut steps = [0.0; 3];
    let mut raws: Vec<Raw> = (0..3)
        .map(|level| {
            let (raw, step) = fit(&field, bounds, budgets[level], &probe, probe_step);
            steps[level] = step;
            raw
        })
        .collect();

    // A loose rock is centred and sunk by its nearest level's own extent,
    // the one the ground will be seen against. Every level moves together,
    // and the field with them.
    if recipe.kind == RockKind::Boulder {
        let [low, high] = extent(&raws[0].positions);
        let buried = seed.fork("place").range(0.10, 0.15);
        let shift = DVec3::new(
            -low.x.midpoint(high.x),
            -low.y - buried * (high.y - low.y),
            -low.z.midpoint(high.z),
        );
        field.shift = shift;
        for raw in &mut raws {
            for p in &mut raw.positions {
                *p += shift;
            }
        }
    }
    let height = raws[0].positions.iter().fold(0.0_f64, |h, p| h.max(p.y));
    let radius = raws[0]
        .positions
        .iter()
        .filter(|p| p.y >= 0.0)
        .fold(0.0_f64, |r, p| r.max(p.x.hypot(p.z)));

    let surface = Surface::new(recipe, &field, &seed, height, radius);
    let until = detail.until();
    let levels: Vec<PropLevel> = raws
        .iter()
        .enumerate()
        .map(|(level, raw)| PropLevel {
            until: Some(until[level]),
            cast_shadows: detail.casts_shadows(level),
            pieces: vec![PropPiece {
                mesh: surface.finish(raw, steps[level]),
                material: recipe.material_key(),
            }],
            impostor: None,
        })
        .collect();

    // Hull points are chosen from the nearest level: a coarse level's
    // vertices sit up to half its grid step off the surface at every corner,
    // which is a hull a character floats on or sinks into by a hand's width.
    let colliders = match recipe.kind {
        RockKind::Boulder => vec![hull_collider(&hull_points(&raws[0].positions))],
        RockKind::Cliff => cliff_hulls(&raws[0].positions, steps[0] * 2.0)
            .iter()
            .map(|points| hull_collider(points))
            .collect(),
    };
    let mut prop = Prop::new(recipe.prop_id(variant), levels, colliders);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the pivot table is f32 by contract"
    )]
    prop.tables.insert(
        "pivots".to_owned(),
        vec![
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            1.0,
            0.0,
            height as f32,
            radius as f32,
            0.1,
            10_000.0,
            0.0,
        ],
    );
    GrownRock {
        prop,
        height,
        radius,
    }
}

fn extent(points: &[DVec3]) -> [DVec3; 2] {
    points.iter().fold(
        [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)],
        |[a, b], p| [a.min(*p), b.max(*p)],
    )
}

/// The finest mesh of `field` within `budget` triangles. Triangle count
/// goes as the inverse square of the grid step, so the probe predicts the
/// step and a few corrections land it close under the budget.
#[allow(clippy::cast_precision_loss, reason = "triangle counts are small")]
fn fit(
    field: &Rock,
    bounds: [DVec3; 2],
    budget: usize,
    probe: &Raw,
    probe_step: f64,
) -> (Raw, f64) {
    let aim = budget as f64 * 0.92;
    let mut step = probe_step * (probe.triangles() as f64 / aim).sqrt();
    let mut best: Option<(Raw, f64)> = None;
    for attempt in 0..12 {
        let raw = mesher::mesh(field, bounds, step);
        let count = raw.triangles();
        let ratio = (count as f64 / aim).sqrt();
        if count <= budget {
            let better = best.as_ref().is_none_or(|(b, _)| count > b.triangles());
            let close = count as f64 >= budget as f64 * 0.82;
            if better {
                best = Some((raw, step));
            }
            if close || attempt >= 4 {
                break;
            }
            step *= ratio.max(0.9);
        } else {
            step *= ratio.max(1.02);
        }
    }
    best.expect("a coarse enough grid always fits the budget")
}

/// What every level's vertices read from the field: the shade terms and
/// the tint, so a level switch changes the silhouette and nothing else.
struct Surface<'a> {
    field: &'a Rock,
    height: f64,
    /// The scale ambient occlusion and curvature are measured at, metres:
    /// the rock's size, or a cliff's feature size.
    scale: f64,
    moss: f64,
    moss_noise: Noise,
    value: f64,
    value_noise: Noise,
}

impl<'a> Surface<'a> {
    fn new(recipe: &RockRecipe, field: &'a Rock, seed: &Rng, height: f64, radius: f64) -> Self {
        let scale = match recipe.kind {
            RockKind::Boulder => (radius * 2.0).max(height),
            RockKind::Cliff => 5.0,
        };
        let mut colour = seed.fork("colour");
        Self {
            field,
            height,
            scale,
            moss: recipe.moss,
            moss_noise: Noise::new(colour.next_u64()),
            value: 1.0 + colour.signed() * 0.08,
            value_noise: Noise::new(colour.next_u64()),
        }
    }

    /// The field with the ground beside it, which occludes the rock's foot.
    fn scene(&self, p: DVec3) -> f64 {
        self.field.distance(p).min(p.y)
    }

    /// Quilez's distance-field ambient occlusion: step out along the normal
    /// and count how much closer the scene is than the step.
    fn ambient(&self, p: DVec3, n: DVec3) -> f64 {
        let mut occlusion = 0.0;
        let mut weight = 1.0;
        for i in 1..=5 {
            let h = self.scale * (0.015 + 0.03 * f64::from(i));
            occlusion += (h - self.scene(p + n * h)).max(0.0) * weight;
            weight *= 0.75;
        }
        (1.0 - occlusion * 2.2 / self.scale).clamp(0.0, 1.0)
    }

    /// The field's Laplacian at the rock's scale: twice the mean curvature,
    /// positive on ridges and negative in hollows, mapped to 0.5 flat.
    fn curvature(&self, p: DVec3) -> f64 {
        let e = self.scale * 0.04;
        let centre = self.field.distance(p);
        let sum: f64 = [DVec3::X, DVec3::Y, DVec3::Z]
            .iter()
            .map(|&axis| self.field.distance(p + axis * e) + self.field.distance(p - axis * e))
            .sum();
        let laplacian = (sum - 6.0 * centre) / (e * e);
        0.5 + 0.5 * (laplacian * self.scale * 0.1).tanh()
    }

    #[allow(clippy::cast_possible_truncation, reason = "shader data is f32")]
    fn shade(&self, p: DVec3, n: DVec3) -> ([f32; 4], [f32; 4]) {
        let ao = self.ambient(p, n);
        let up = (p.y / self.height).clamp(0.0, 1.0);
        let curvature = self.curvature(p);
        let facing = smoothstep(0.3, 0.85, n.y);
        let near_top = 0.3 + 0.7 * smoothstep(0.2, 0.85, up);
        let hollow = (1.0 - (curvature - 0.5) * 2.4).clamp(0.25, 1.2);
        let shelter = 0.8 + 0.6 * (1.0 - ao);
        let patches = smoothstep(-0.3, 0.3, self.moss_noise.fbm(p * (3.0 / self.scale), 3));
        let moss = if p.y < 0.0 {
            0.0
        } else {
            (facing * near_top * hollow * shelter * patches * self.moss).clamp(0.0, 1.0)
        };
        let value = self.value * (1.0 + 0.03 * self.value_noise.sample(p * (1.5 / self.scale)));
        let tint = [0, 1, 2].map(|c| {
            let rock = value;
            (rock + (MOSS[c] * value - rock) * moss * 0.9) as f32
        });
        let shade = [ao as f32, up as f32, curvature as f32, moss as f32];
        (shade, [tint[0], tint[1], tint[2], 1.0])
    }

    /// The level's drawable mesh: normals from the field at the level's own
    /// resolution, box-projected metre UVs (a vertex is split wherever its
    /// triangles project along different axes), and the shade data.
    #[allow(clippy::cast_possible_truncation, reason = "shader data is f32")]
    fn finish(&self, raw: &Raw, step: f64) -> TriangleMesh {
        let normals: Vec<DVec3> = raw
            .positions
            .iter()
            .map(|&p| self.field.normal(p, step * 0.5))
            .collect();
        let data: Vec<([f32; 4], [f32; 4])> = raw
            .positions
            .iter()
            .zip(&normals)
            .map(|(&p, &n)| self.shade(p, n))
            .collect();
        let mut mesh = TriangleMesh::default();
        let mut shade = Vec::new();
        let mut colours = Vec::new();
        let mut split = vec![[u32::MAX; 6]; raw.positions.len()];
        for tri in raw.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| raw.positions[i as usize]);
            let face = (b - a).cross(c - a);
            let axis = face.abs().max_position();
            let side = axis * 2 + usize::from(face[axis] < 0.0);
            for &old in tri {
                let slot = &mut split[old as usize][side];
                if *slot == u32::MAX {
                    let p = raw.positions[old as usize];
                    *slot = u32::try_from(mesh.positions.len())
                        .expect("a rock is far under 4 billion vertices");
                    mesh.positions.push(p);
                    mesh.normals.push(normals[old as usize]);
                    mesh.uvs.push(box_uv(p, side));
                    shade.push(data[old as usize].0);
                    colours.push(data[old as usize].1);
                }
                mesh.indices.push(*slot);
            }
        }
        let count = mesh.positions.len();
        mesh.sources = vec![FaceSource::BODY; mesh.triangle_count()];
        mesh.colors = Some(colours);
        mesh.attributes
            .insert("shade".to_owned(), VertexData::F32x4(shade));
        mesh.attributes
            .insert("wind".to_owned(), VertexData::F32x4(vec![[0.0; 4]; count]));
        mesh
    }
}

/// Metre UVs on the plane of axis side `side` (`+X`, `-X`, `+Y`, `-Y`, `+Z`,
/// `-Z`). On a wall `u` runs along the face, unmirrored from outside, and
/// `v` runs down the world, because sandstone's strata are painted across
/// `v` and its streaks along it; on a top or underside `u` is `x` and `v`
/// is `z`.
fn box_uv(p: DVec3, side: usize) -> [f64; 2] {
    match side {
        0 => [-p.z, -p.y],
        1 => [p.z, -p.y],
        2 | 3 => [p.x, p.z],
        4 => [p.x, -p.y],
        _ => [-p.x, -p.y],
    }
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hull_collider(points: &[DVec3]) -> PropCollider {
    PropCollider::ConvexHull(ConvexSolid {
        vertices: points.to_vec(),
    })
}

/// A cliff's hulls: its vertices split into four slabs, across its width,
/// up its height or both, whichever wraps the rock in the least volume, so
/// a character stands on the rock and not on air beside it. Neighbouring
/// slabs overlap by `overlap` so no seam opens between hulls.
pub(crate) fn cliff_hulls(points: &[DVec3], overlap: f64) -> Vec<Vec<DVec3>> {
    let [low, high] = extent(points);
    let across = if high.x - low.x >= high.z - low.z {
        0
    } else {
        2
    };
    let mut best: Option<(f64, Vec<Vec<DVec3>>)> = None;
    for (wide, tall) in [(4, 1), (2, 2), (1, 4)] {
        let mut slabs = Vec::new();
        let mut volume = 0.0;
        for i in 0..wide {
            for j in 0..tall {
                let band = |axis: usize, index: usize, parts: usize| {
                    #[allow(clippy::cast_precision_loss, reason = "a few parts")]
                    let (index, parts) = (index as f64, parts as f64);
                    let size = (high[axis] - low[axis]) / parts;
                    let start = low[axis] + size * index - overlap;
                    (start, start + size + 2.0 * overlap)
                };
                let (x0, x1) = band(across, i, wide);
                let (y0, y1) = band(1, j, tall);
                let inside: Vec<DVec3> = points
                    .iter()
                    .filter(|p| (x0..=x1).contains(&p[across]) && (y0..=y1).contains(&p.y))
                    .copied()
                    .collect();
                if let Some(hull) = rough_hull(&inside) {
                    volume += hull.volume();
                    slabs.push(inside);
                }
            }
        }
        if slabs.len() <= MAX_HULLS
            && !slabs.is_empty()
            && best.as_ref().is_none_or(|(v, _)| volume < *v)
        {
            best = Some((volume, slabs));
        }
    }
    best.map_or_else(
        || vec![hull_points(points)],
        |(_, slabs)| slabs.iter().map(|slab| hull_points(slab)).collect(),
    )
}
