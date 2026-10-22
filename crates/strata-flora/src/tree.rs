//! The understory's plants from a species' parameters: flowers, ferns,
//! bushes and fallen logs as tubes and cards, three levels of detail each.
//!
//! Trees are not grown here any more. The content step grows and bakes
//! them (`veyra-flora`, ADR 0005's 2026-09-26 amendment), and the tree
//! library carries their variants and trunks. The understory stays here,
//! grown on the client, until Phase 10 bakes it too. Output is mesh local
//! with the root at the origin, so the wind attributes' root is `(0, 0)` and
//! `v` is `y / height`. The tube rings use the platform's sine and cosine: a
//! mesh is a picture and need not match across machines; the collider
//! fallback ([`trunk_collider`]) uses none.

use strata_core::glam::{Quat, Vec3};
use strata_data::TreeParams;

use crate::hash_unit;

mod understory;

/// Levels of detail per tree, finest first.
pub const TREE_LODS: usize = 3;

/// Vertex streams of one mesh, in the layout the foliage material binds:
/// position, normal, uv, then the two wind attributes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Streams {
    /// Mesh-local positions.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// `[root_x, root_z, v, h]`: the root in mesh space, the height
    /// fraction, the plant height.
    pub foliage_a: Vec<[f32; 4]>,
    /// `[phase, stiffness, flutter, 0]`.
    pub foliage_b: Vec<[f32; 4]>,
    /// Tangents along `u`, `w` the handedness, for normal maps.
    pub tangents: Vec<[f32; 4]>,
    /// Triangle list.
    pub indices: Vec<u32>,
}

impl Streams {
    /// Whether there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// One level of detail: bark and leaves.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeLod {
    /// Opaque bark.
    pub trunk: Streams,
    /// Masked leaf cards.
    pub leaves: Streams,
}

/// One variant of one species.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeGeometry {
    /// Finest first.
    pub lods: Vec<TreeLod>,
    /// Height of the whole plant at scale one, metres.
    pub height: f32,
}

/// The height of variant `variant` of a species at scale one: a hash
/// within the species' range, so the collider and the mesh agree without
/// either building the other.
#[must_use]
pub fn variant_height(params: &TreeParams, species_seed: u64, variant: u8) -> f32 {
    let t = hash_unit(species_seed, i64::from(variant), 0, 40);
    params.height_m.min + (params.height_m.max - params.height_m.min) * t
}

/// The trunk as a collider: `(radius, height)` in metres, for a plant of
/// `height` at `scale`. The cylinder stands on the ground and reaches the
/// first branches; a character walks into it and around it.
#[must_use]
pub fn trunk_collider(params: &TreeParams, height: f32, scale: f32) -> (f32, f32) {
    let h = height * scale;
    ((params.trunk_ratio * h * 1.5).max(0.1), h * 0.6)
}

struct Builder {
    seed: u64,
    counter: i64,
    height: f32,
    sides: usize,
    /// Far cards face the sky: seen from afar a crown is lit from above,
    /// and a card lit by its own facing goes black edge-on.
    cards_up: bool,
    trunk: Streams,
    leaves: Streams,
}

impl Builder {
    fn rnd(&mut self, salt: u64) -> f32 {
        self.counter += 1;
        hash_unit(self.seed, self.counter, 0, salt)
    }

    /// A tube from `from` to `to`, radius `r0` to `r1`.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn tube(&mut self, from: Vec3, to: Vec3, r0: f32, r1: f32, stiffness: f32, phase: f32) {
        let axis = (to - from).normalize_or_zero();
        self.tube_frame(from, to, r0, r1, stiffness, phase, axis, 0.0);
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "shared frame and bark offset keep adjacent curve rings coincident"
    )]
    fn tube_frame(
        &mut self,
        from: Vec3,
        to: Vec3,
        r0: f32,
        r1: f32,
        stiffness: f32,
        phase: f32,
        axis: Vec3,
        bark_offset: f32,
    ) {
        let any = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let side = axis.cross(any).normalize();
        let across = axis.cross(side);
        let base = self.trunk.positions.len() as u32;
        let height = self.height;
        // Bark textures cover about a metre: `u` runs around the
        // circumference in metres, `v` along the segment in metres.
        let along = (to - from).length();
        for (ring, (centre, radius)) in [(from, r0), (to, r1)].into_iter().enumerate() {
            let around = (core::f32::consts::TAU * radius).max(0.5);
            for step in 0..self.sides {
                let angle = step as f32 / self.sides as f32 * core::f32::consts::TAU;
                let normal = side * angle.cos() + across * angle.sin();
                let tangent = -side * angle.sin() + across * angle.cos();
                let point = centre + normal * radius;
                self.trunk.positions.push(point.to_array());
                self.trunk.normals.push(normal.to_array());
                self.trunk
                    .tangents
                    .push([tangent.x, tangent.y, tangent.z, 1.0]);
                self.trunk.uvs.push([
                    step as f32 / self.sides as f32 * around,
                    bark_offset + ring as f32 * along,
                ]);
                self.trunk
                    .foliage_a
                    .push([0.0, 0.0, (point.y / height).clamp(0.0, 1.0), height]);
                self.trunk.foliage_b.push([phase, stiffness, 0.0, 0.0]);
            }
        }
        let sides = self.sides as u32;
        for step in 0..sides {
            let next = (step + 1) % sides;
            let (a0, a1, b0, b1) = (
                base + step,
                base + next,
                base + sides + step,
                base + sides + next,
            );
            self.trunk
                .indices
                .extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
        // Close the rings: exposed twig ends must not reveal an open tube
        // when inspected from below or through sparse foliage.
        for step in 1..sides - 1 {
            self.trunk.indices.extend_from_slice(&[
                base,
                base + step + 1,
                base + step,
                base + sides,
                base + sides + step,
                base + sides + step + 1,
            ]);
        }
    }

    /// A card's normals lean towards the direction from the crown's centre,
    /// so the canopy shades as one rounded mass instead of a bag of flat
    /// cards (the spherical-normals trick), with a little of the card's
    /// own facing kept so it still catches the light.
    #[allow(clippy::cast_possible_truncation)]
    fn card(&mut self, at: Vec3, size: f32, rot: Quat, stiffness: f32, phase: f32) {
        let base = self.leaves.positions.len() as u32;
        let facing = rot * Vec3::Z;
        let h = self.height;
        let crown = Vec3::new(0.0, h * 0.62, 0.0);
        for (i, (x, y)) in [(-0.5, 0.0), (0.5, 0.0), (0.5, 1.0), (-0.5, 1.0)]
            .into_iter()
            .enumerate()
        {
            // The opaque center of the spray sits on the twig. Offsetting
            // the card upward left the actual foliage floating above it.
            let p = at + rot * Vec3::new(x * size, (y - 0.5) * size, 0.0);
            let radial = (p - crown).normalize_or(Vec3::Y);
            let n = if self.cards_up {
                Vec3::Y
            } else {
                (radial * 0.7 + facing * 0.3).normalize_or(facing)
            };
            let t = rot * Vec3::X;
            self.leaves.positions.push(p.to_array());
            self.leaves.normals.push(n.to_array());
            self.leaves.tangents.push([t.x, t.y, t.z, 1.0]);
            self.leaves.uvs.push([x + 0.5, 1.0 - y]);
            self.leaves
                .foliage_a
                .push([0.0, 0.0, (p.y / h).clamp(0.0, 1.0), h]);
            // The flutter weight grows towards the card's outer edge.
            self.leaves.foliage_b.push([
                phase,
                stiffness,
                if i == 0 || i == 1 { 0.3 } else { 1.0 },
                0.0,
            ]);
        }
        self.leaves.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
        ]);
    }
}

/// The geometry of variant `variant` of an understory species whose seed
/// is `species_seed`, at every level of detail; `None` for a tree, which is
/// baked by the content step and read from the tree library.
#[must_use]
pub fn build_understory(
    params: &TreeParams,
    species_seed: u64,
    variant: u8,
) -> Option<TreeGeometry> {
    (params.form != "tree").then(|| understory::build(params, species_seed, variant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_data::MetreRange;

    #[test]
    fn a_tree_is_not_grown_here() {
        let oak = TreeParams {
            height_m: MetreRange::new(7.0, 11.0),
            ..TreeParams::default()
        };
        assert!(build_understory(&oak, 42, 0).is_none());
        let fern = TreeParams {
            form: "fern".to_owned(),
            height_m: MetreRange::new(0.5, 0.9),
            ..TreeParams::default()
        };
        let grown = build_understory(&fern, 42, 0).expect("a fern is grown");
        assert_eq!(grown.lods.len(), TREE_LODS);
    }

    #[test]
    fn variants_differ_and_the_collider_is_inside_the_height() {
        let oak = TreeParams {
            height_m: MetreRange::new(7.0, 11.0),
            ..TreeParams::default()
        };
        let a = variant_height(&oak, 42, 0);
        let b = variant_height(&oak, 42, 1);
        assert!((a - b).abs() > 1e-3, "variants differ in height");
        let (radius, height) = trunk_collider(&oak, a, 1.0);
        assert!(radius > 0.0 && height < a);
    }
}
