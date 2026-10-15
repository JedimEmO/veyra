//! Small native meshes, not scaled-down trees. Each LOD keeps the same
//! rooted silhouette while dropping secondary foliage and tube sides.

use super::{Builder, Quat, Streams, TreeGeometry, TreeLod, TreeParams, Vec3, variant_height};
use crate::hash_unit;

pub(super) fn build(params: &TreeParams, seed: u64, variant: u8) -> TreeGeometry {
    let size = variant_height(params, seed, variant);
    let seed = seed ^ (u64::from(variant) << 32);
    let lods: Vec<_> = (0..3).map(|lod| level(params, seed, size, lod)).collect();
    let height = lods[0]
        .trunk
        .positions
        .iter()
        .chain(&lods[0].leaves.positions)
        .map(|p| p[1])
        .fold(0.0_f32, f32::max);
    TreeGeometry { lods, height }
}

fn level(params: &TreeParams, seed: u64, size: f32, lod: usize) -> TreeLod {
    let mut b = Builder {
        seed,
        counter: 0,
        height: size,
        sides: if lod == 0 { 5 } else { 3 },
        cards_up: false,
        trunk: Streams::default(),
        leaves: Streams::default(),
    };
    match params.form.as_str() {
        "fern" => fern(&mut b, lod),
        // A reed is baked; without the content step it is drawn as flowers.
        "flower" | "reed" => flowers(&mut b, lod),
        "log" => log(&mut b, lod),
        _ => shrub(&mut b, lod),
    }
    TreeLod {
        trunk: b.trunk,
        leaves: b.leaves,
    }
}

fn radial(angle: f32) -> Vec3 {
    Vec3::new(angle.cos(), 0.0, angle.sin())
}

/// A folded lance-shaped leaf/petal. Unlike a rectangle, its silhouette
/// remains a leaf under close inspection even with a solid texture.
#[allow(
    clippy::cast_possible_truncation,
    reason = "small meshes are capped below 600 triangles"
)]
fn lance(stream: &mut Streams, root: Vec3, tip: Vec3, width: f32, height: f32) {
    let axis = (tip - root).normalize();
    let side = axis.cross(Vec3::Y).normalize_or(Vec3::X) * width;
    let middle = root.lerp(tip, 0.46);
    let ridge = middle + Vec3::Y * width * 0.22;
    let points = [root, middle + side, tip, middle - side, ridge];
    for face in [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]] {
        let [a, b, c] = face.map(|i| points[i]);
        let normal = (b - a).cross(c - a).normalize();
        let tangent = (b - a).normalize();
        let base = stream.positions.len() as u32;
        for (p, uv) in [(a, [0.0, 0.0]), (b, [1.0, 0.0]), (c, [0.5, 1.0])] {
            stream.positions.push(p.to_array());
            stream.normals.push(normal.to_array());
            stream.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
            stream.uvs.push(uv);
            stream
                .foliage_a
                .push([0.0, 0.0, (p.y / height).clamp(0.0, 1.0), height]);
            stream.foliage_b.push([0.0, 0.75, 0.15, 0.0]);
        }
        stream.indices.extend([base, base + 1, base + 2]);
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bounded small frond/segment indices"
)]
fn fern(b: &mut Builder, lod: usize) {
    let h = b.height;
    let count = 7 - lod * 2;
    let steps = 5 - lod;
    b.sides = 3;
    for frond in 0..count {
        let angle = frond as f32 * 2.399_963 + b.rnd(100) * 0.3;
        let outward = radial(angle);
        let side = radial(angle + core::f32::consts::FRAC_PI_2);
        let length = h * (0.7 + b.rnd(101) * 0.3);
        let mut previous = Vec3::ZERO;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let at = outward * (length * t * 0.85) + Vec3::Y * (length * (1.8 * t - 1.05 * t * t));
            b.tube(previous, at, h * 0.009 * (1.1 - t), h * 0.004, 0.75, 0.0);
            let leaf_length = length * 0.28 * (1.0 - t * 0.75);
            for sign in [-1.0, 1.0] {
                let tip = at
                    + side * (sign * leaf_length)
                    + outward * leaf_length * 0.42
                    + Vec3::Y * leaf_length * 0.12;
                lance(&mut b.leaves, at, tip, leaf_length * 0.20, h);
            }
            previous = at;
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bounded small stem/petal indices"
)]
fn flowers(b: &mut Builder, lod: usize) {
    let h = b.height;
    b.sides = 3;
    for stem in 0..(5 - lod) {
        let angle = stem as f32 * 2.399_963 + b.rnd(103);
        let outward = radial(angle);
        let top =
            outward * (h * (0.12 + b.rnd(104) * 0.2)) + Vec3::Y * (h * (0.65 + b.rnd(105) * 0.35));
        let bend = top * 0.48 + outward * h * 0.08;
        b.tube(Vec3::ZERO, bend, h * 0.011, h * 0.008, 0.75, 0.0);
        b.tube(bend, top, h * 0.008, h * 0.004, 0.75, 0.0);
        for sign in [-1.0, 1.0] {
            let at = bend * if sign > 0.0 { 1.0 } else { 0.65 };
            lance(
                &mut b.trunk,
                at,
                at + outward * (h * 0.22 * sign) + Vec3::Y * h * 0.1,
                h * 0.038,
                h,
            );
        }
        let petals = 6 - lod;
        for petal in 0..petals {
            let direction = radial(angle + petal as f32 / petals as f32 * core::f32::consts::TAU);
            lance(
                &mut b.leaves,
                top,
                top + direction * h * 0.16 + Vec3::Y * h * 0.035,
                h * 0.065,
                h,
            );
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bounded small shrub stem/card indices"
)]
fn shrub(b: &mut Builder, lod: usize) {
    let h = b.height;
    for stem in 0..(6 - lod * 2) {
        let angle = stem as f32 * 2.399_963 + b.rnd(108) * 0.4;
        let outward = radial(angle);
        let end =
            outward * (h * (0.3 + b.rnd(109) * 0.25)) + Vec3::Y * h * (0.55 + b.rnd(110) * 0.2);
        let elbow = end * 0.45 - outward * h * 0.08;
        b.tube(Vec3::ZERO, elbow, h * 0.023, h * 0.014, 0.8, 0.0);
        b.tube(elbow, end, h * 0.014, h * 0.004, 0.8, 0.0);
        for node in 0..(4 - lod) {
            let at = elbow.lerp(end, 0.55 + node as f32 * 0.15);
            let rotation = Quat::from_rotation_y(angle + node as f32 * 1.5)
                * Quat::from_rotation_x(0.25 + node as f32 * 0.2);
            b.card(at, h * 0.65, rotation, 0.8, 0.0);
            b.card(
                at,
                h * 0.65,
                rotation * Quat::from_rotation_y(1.57),
                0.8,
                0.0,
            );
        }
    }
}

fn log(b: &mut Builder, lod: usize) {
    let length = b.height;
    let radius = length * (0.065 + hash_unit(b.seed, 0, 0, 114) * 0.025);
    b.sides = if lod == 0 { 8 } else { 5 };
    // Rest along X, with the largest ring touching the ground. Tangential
    // variation and tapered sections avoid a perfect manufactured cylinder.
    let a = Vec3::new(-length * 0.5, radius, 0.0);
    let middle = Vec3::new(0.0, radius * 0.85, radius * 0.15);
    let end = Vec3::new(length * 0.5, radius * 0.6, radius * 0.35);
    b.tube(a, middle, radius, radius * 0.85, 1.0, 0.0);
    b.tube(middle, end, radius * 0.85, radius * 0.6, 1.0, 0.0);
    for (t, sign) in [(0.25, -1.0), (0.65, 1.0)] {
        let at = if t < 0.5 {
            a.lerp(middle, t * 2.0)
        } else {
            middle.lerp(end, (t - 0.5) * 2.0)
        };
        b.tube(
            at,
            at + Vec3::new(radius * 0.5, radius * 1.4, radius * sign),
            radius * 0.28,
            radius * 0.13,
            1.0,
            0.0,
        );
    }
    for wind in &mut b.trunk.foliage_b {
        wind[3] = -1.0;
    }
    for p in &mut b.trunk.positions {
        p[1] -= 0.02;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::MetreRange;

    #[test]
    fn all_small_variants_have_rooted_finite_bounds() {
        for form in ["fern", "flower", "shrub", "log"] {
            for variant in 0..16 {
                let params = TreeParams {
                    form: form.into(),
                    height_m: MetreRange::new(0.3, 1.5),
                    ..TreeParams::default()
                };
                let tree = build(&params, 42, variant);
                assert!(tree.height.is_finite() && tree.height > 0.0);
                for lod in &tree.lods {
                    for p in lod.trunk.positions.iter().chain(&lod.leaves.positions) {
                        assert!(p[1] >= -0.035, "{form} variant {variant}: {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn understory_is_deterministic_bounded_and_has_complete_vertex_streams() {
        for form in ["fern", "flower", "shrub", "log"] {
            let params = TreeParams {
                form: form.into(),
                height_m: MetreRange::new(1.0, 1.5),
                ..TreeParams::default()
            };
            let tree = build(&params, 42, 0);
            assert_eq!(tree, build(&params, 42, 0));
            assert_ne!(tree, build(&params, 42, 1));
            assert_eq!(tree.lods.len(), 3);
            let mut previous = usize::MAX;
            for lod in &tree.lods {
                let triangles = (lod.trunk.indices.len() + lod.leaves.indices.len()) / 3;
                assert!(triangles < 600, "{form}: {triangles}");
                assert!(triangles <= previous);
                previous = triangles;
                for stream in [&lod.trunk, &lod.leaves] {
                    let n = stream.positions.len();
                    assert_eq!(stream.normals.len(), n);
                    assert_eq!(stream.tangents.len(), n);
                    assert_eq!(stream.uvs.len(), n);
                    assert_eq!(stream.foliage_a.len(), n);
                    assert_eq!(stream.foliage_b.len(), n);
                    assert!(stream.indices.iter().all(|&i| (i as usize) < n));
                    assert!(stream.positions.iter().flatten().all(|x| x.is_finite()));
                    assert!(
                        stream.positions.iter().all(|p| p[1] >= -0.03),
                        "{form}: {:?}",
                        stream.positions.iter().map(|p| p[1]).reduce(f32::min)
                    );
                    assert!(
                        stream
                            .normals
                            .iter()
                            .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-5)
                    );
                    if form == "log" {
                        assert!(
                            stream
                                .foliage_b
                                .iter()
                                .all(|b| b[3].to_bits() == (-1.0_f32).to_bits())
                        );
                    }
                }
            }
        }
    }
}
