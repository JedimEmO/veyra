//! Synthetic trees at the Phase 9 budget, for measuring what the runtime can
//! carry.
//!
//! They are not meant to look like trees. They are meant to cost what a tree
//! will: a tapered trunk and branches as tubes, clusters of cut-out leaf
//! cards at the branch ends, three levels of about 15 000, 5 000 and 1 500
//! triangles, and per-vertex colour and wind data. Everything comes from a
//! hash of the species and variant, so the same fixture is baked every time.
use ashlar::glam::DVec3;
use ashlar::prop::{Prop, PropCollider, PropLevel, PropPiece};
use ashlar::{FaceSource, TriangleMesh, VertexData};

/// Species in the fixture.
pub const SPECIES: u32 = 20;

/// Variants per species.
pub const VARIANTS: u32 = 8;

/// The bark and leaf material keys every capacity tree binds.
pub const BARK: &str = "capacity:bark";
/// See [`BARK`].
pub const LEAF: &str = "capacity:leaf";

/// Where each level stops drawing, in metres: the plan's LOD chain.
const UNTIL: [f32; 3] = [60.0, 150.0, 350.0];

/// How one level is built: tube sides and rings for the trunk and branches,
/// how many branches get a tube, and how many leaf cards there are.
struct Detail {
    trunk_sides: u32,
    trunk_rings: u32,
    branch_sides: u32,
    branch_rings: u32,
    branches: u32,
    cards: u32,
}

const DETAIL: [Detail; 3] = [
    Detail {
        trunk_sides: 16,
        trunk_rings: 24,
        branch_sides: 8,
        branch_rings: 10,
        branches: 36,
        cards: 4_200,
    },
    Detail {
        trunk_sides: 8,
        trunk_rings: 12,
        branch_sides: 4,
        branch_rings: 5,
        branches: 36,
        cards: 1_700,
    },
    Detail {
        trunk_sides: 6,
        trunk_rings: 6,
        branch_sides: 3,
        branch_rings: 3,
        branches: 12,
        cards: 600,
    },
];

/// The path the content step writes a capacity tree to, under `generated/`.
#[must_use]
pub fn path(species: u32, variant: u32) -> String {
    format!("capacity/tree-{species:02}-{variant}.ashprop")
}

/// The directory under `generated/` a species' impostor atlas goes to.
#[must_use]
pub fn impostor_directory(species: u32) -> String {
    format!("capacity/impostor-{species:02}")
}

/// One synthetic tree.
#[must_use]
pub fn tree(species: u32, variant: u32) -> Prop {
    let mut rng = Hash::new(u64::from(species) << 32 | u64::from(variant));
    let height = 9.0 + rng.unit() * 8.0;
    let radius = 0.25 + rng.unit() * 0.2;
    let spread = 2.5 + rng.unit() * 2.5;
    // The branch skeleton is shared by every level, so a coarser level is
    // the same tree with fewer triangles, as a real LOD chain would be.
    let branches: Vec<Branch> = (0..DETAIL[0].branches)
        .map(|i| {
            let along = 0.35 + 0.6 * f64::from(i) / f64::from(DETAIL[0].branches);
            let turn = rng.unit() * core::f64::consts::TAU;
            let rise = 0.3 + rng.unit() * 0.6;
            let length = spread * (1.1 - along * 0.6) * (0.7 + rng.unit() * 0.6);
            let start = DVec3::new(0.0, height * along, 0.0);
            let dir = DVec3::new(turn.cos(), rise, turn.sin()).normalize();
            Branch {
                start,
                end: start + dir * length,
                radius: radius * 0.35 * (1.2 - along),
            }
        })
        .collect();
    let levels = DETAIL
        .iter()
        .zip(UNTIL)
        .enumerate()
        .map(|(index, (detail, until))| {
            let mut bark = TriangleMesh::default();
            tube(
                &mut bark,
                DVec3::ZERO,
                DVec3::new(0.0, height, 0.0),
                radius,
                radius * 0.15,
                detail.trunk_sides,
                detail.trunk_rings,
                0,
            );
            // A coarse level keeps the longest branches.
            let step = (DETAIL[0].branches / detail.branches).max(1) as usize;
            for (b, branch) in branches.iter().enumerate().step_by(step) {
                tube(
                    &mut bark,
                    branch.start,
                    branch.end,
                    branch.radius,
                    branch.radius * 0.3,
                    detail.branch_sides,
                    detail.branch_rings,
                    u32::try_from(b + 1).expect("a few dozen branches"),
                );
            }
            let mut leaves = TriangleMesh::default();
            let mut cards = Hash::new(u64::from(species) * 7919 + u64::from(variant));
            for c in 0..detail.cards {
                let branch = &branches[c as usize % branches.len()];
                let along = 0.5 + cards.unit() * 0.5;
                let at = branch.start.lerp(branch.end, along)
                    + DVec3::new(cards.signed(), cards.signed(), cards.signed()) * 0.8;
                // Coarser levels spread fewer, larger cards over the same crown.
                let size = 0.45 * (f64::from(DETAIL[0].cards) / f64::from(detail.cards)).sqrt();
                card(
                    &mut leaves,
                    at,
                    size,
                    cards.unit() * core::f64::consts::TAU,
                    u32::try_from(c as usize % branches.len() + 1).expect("a few dozen branches"),
                    height,
                );
            }
            PropLevel {
                until: Some(until),
                // Only the nearest level casts: far levels' shadows are
                // under a shadow texel and cost a pass (the foliage spike).
                cast_shadows: index == 0,
                pieces: vec![
                    PropPiece {
                        mesh: finish(bark, height),
                        material: BARK.to_owned(),
                    },
                    PropPiece {
                        mesh: finish(leaves, height),
                        material: LEAF.to_owned(),
                    },
                ],
                impostor: None,
            }
        })
        .collect();
    Prop::new(
        format!("capacity:tree-{species:02}-{variant}"),
        levels,
        vec![PropCollider::Capsule {
            a: DVec3::new(0.0, radius, 0.0),
            b: DVec3::new(0.0, height * 0.6, 0.0),
            radius,
        }],
    )
}

struct Branch {
    start: DVec3,
    end: DVec3,
    radius: f64,
}

/// A tapered tube from `a` to `b`, with UVs in metres around and along it,
/// and a pivot index in its wind data.
#[allow(
    clippy::too_many_arguments,
    reason = "a tube is its ends, radii, detail and pivot"
)]
fn tube(
    mesh: &mut TriangleMesh,
    a: DVec3,
    b: DVec3,
    r0: f64,
    r1: f64,
    sides: u32,
    rings: u32,
    pivot: u32,
) {
    let axis = (b - a).normalize();
    let side = axis.any_orthonormal_vector();
    let up = axis.cross(side);
    let length = (b - a).length();
    let base = u32::try_from(mesh.positions.len()).expect("a tree is far under 4 billion vertices");
    for ring in 0..=rings {
        let t = f64::from(ring) / f64::from(rings);
        let centre = a + (b - a) * t;
        let radius = r0 + (r1 - r0) * t;
        for s in 0..=sides {
            let angle = f64::from(s) / f64::from(sides) * core::f64::consts::TAU;
            let normal = side * angle.cos() + up * angle.sin();
            mesh.positions.push(centre + normal * radius);
            mesh.normals.push(normal);
            mesh.uvs.push([angle * r0, t * length]);
            push_wind(mesh, pivot, t);
        }
    }
    for ring in 0..rings {
        for s in 0..sides {
            let row = base + ring * (sides + 1);
            let next = row + sides + 1;
            mesh.indices
                .extend_from_slice(&[row + s, next + s, row + s + 1]);
            mesh.indices
                .extend_from_slice(&[row + s + 1, next + s, next + s + 1]);
        }
    }
}

/// A leaf card: a quad facing a random way, UVs over the whole card.
fn card(mesh: &mut TriangleMesh, at: DVec3, size: f64, turn: f64, pivot: u32, height: f64) {
    let across = DVec3::new(turn.cos(), 0.0, turn.sin()) * size * 0.5;
    let up = DVec3::Y * size * 0.5;
    let normal = across.cross(up).normalize();
    let base = u32::try_from(mesh.positions.len()).expect("a tree is far under 4 billion vertices");
    for (corner, uv) in [
        (-across - up, [0.0, 1.0]),
        (across - up, [1.0, 1.0]),
        (across + up, [1.0, 0.0]),
        (-across + up, [0.0, 0.0]),
    ] {
        mesh.positions.push(at + corner);
        mesh.normals.push(normal);
        mesh.uvs.push(uv);
        push_wind(mesh, pivot, (at.y / height).clamp(0.0, 1.0));
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The wind data a Phase 9 tree carries per vertex: its pivot, its level in
/// the hierarchy, how far along its branch it sits, and a phase.
fn push_wind(mesh: &mut TriangleMesh, pivot: u32, along: f64) {
    let entry = mesh
        .attributes
        .entry("wind".to_owned())
        .or_insert_with(|| VertexData::F32x4(Vec::new()));
    if let VertexData::F32x4(values) = entry {
        // Pivot indices are small; the conversion is exact.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        values.push([pivot as f32, 1.0, along as f32, (pivot % 7) as f32 / 7.0]);
    }
}

/// Face sources, and the per-vertex colour a stylised shader reads: ambient
/// occlusion darker towards the trunk and the bottom of the crown.
fn finish(mut mesh: TriangleMesh, height: f64) -> TriangleMesh {
    mesh.sources = vec![FaceSource::BODY; mesh.indices.len() / 3];
    // Colours in the prop's frame, narrowed at upload anyway.
    #[allow(clippy::cast_possible_truncation)]
    let colors = mesh
        .positions
        .iter()
        .map(|p| {
            let reach = (p.x * p.x + p.z * p.z).sqrt().min(4.0) / 4.0;
            let lift = (p.y / height).clamp(0.0, 1.0);
            let ao = (0.45 + 0.35 * reach + 0.2 * lift) as f32;
            [ao, ao, ao, 1.0]
        })
        .collect();
    mesh.colors = Some(colors);
    mesh
}

/// A tiny deterministic hash stream: the fixture must be the same every bake.
struct Hash(u64);

impl Hash {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 53 bits fit an f64 mantissa exactly.
        #[allow(clippy::cast_precision_loss)]
        let fraction = (z >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    }

    fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_meet_the_plan_budget() {
        let prop = tree(3, 5);
        prop.check().expect("a capacity tree validates");
        let triangles = prop.triangles();
        assert_eq!(triangles.len(), 3);
        assert!((13_000..=15_000).contains(&triangles[0]), "{triangles:?}");
        assert!((4_000..=5_500).contains(&triangles[1]), "{triangles:?}");
        assert!((1_000..=1_700).contains(&triangles[2]), "{triangles:?}");
    }

    #[test]
    fn the_fixture_is_the_same_every_bake() {
        assert_eq!(
            tree(7, 2).write().expect("writes"),
            tree(7, 2).write().expect("writes")
        );
    }

    #[test]
    fn every_vertex_carries_colour_and_wind() {
        let prop = tree(0, 0);
        for level in &prop.levels {
            for piece in &level.pieces {
                let n = piece.mesh.positions.len();
                assert_eq!(piece.mesh.colors.as_ref().map(Vec::len), Some(n));
                assert_eq!(piece.mesh.attributes["wind"].len(), n);
            }
        }
    }

    #[test]
    fn species_differ() {
        assert_ne!(tree(1, 0).write().ok(), tree(2, 0).write().ok());
    }
}
