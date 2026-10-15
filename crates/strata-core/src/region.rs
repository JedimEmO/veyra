//! The region grid: squares of the world at one level of detail.

use serde::{Deserialize, Serialize};

use crate::WorldPos;

/// Side length in metres of a region at LOD 0.
///
/// 64 m with [`GRID_N`] vertices per side is one vertex per metre at the
/// finest level, which is also the granularity a heightfield collider and
/// persisted terrain edits use (ADR 0003).
pub const REGION_BASE_SIZE: f64 = 64.0;

/// Vertices per side of a region's mesh grid, at every LOD.
///
/// Odd, so the grid has a centre vertex and every LOD's grid lines land on
/// the finer LOD's grid lines.
pub const GRID_N: usize = 65;

/// The coarsest level of detail a [`RegionCoord`] can name.
///
/// `size()` computes `2^lod`, and past 31 the `i32` region indices at LOD 0
/// could not address that square anyway. Nothing in the slice goes past LOD
/// 9 (32 km per region); the bound exists so the arithmetic is total.
pub const MAX_LOD: u8 = 31;

/// A square of the world at one level of detail.
///
/// `x` and `z` index squares of side `REGION_BASE_SIZE * 2^lod`, so the same
/// point of the world is `(x, z)` at LOD 0 and `(x / 2, z / 2)` at LOD 1.
/// The set of coordinates at all LODs is a quadtree, and this type is what
/// the streaming crate selects, the terrain crate meshes, and later the
/// server uses for interest management. Ordered and hashable so it can key
/// maps and sort deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RegionCoord {
    /// East-west index, in units of [`Self::size`].
    pub x: i32,
    /// North-south index, in units of [`Self::size`].
    pub z: i32,
    /// Level of detail; 0 is finest.
    pub lod: u8,
}

impl RegionCoord {
    /// A coordinate from its parts. `lod` above [`MAX_LOD`] is a bug, and a
    /// debug assertion says so; release builds clamp in `size()`.
    #[must_use]
    pub const fn new(x: i32, z: i32, lod: u8) -> Self {
        debug_assert!(lod <= MAX_LOD, "RegionCoord lod above MAX_LOD");
        Self { x, z, lod }
    }

    /// Side length in metres of a region at this LOD.
    #[must_use]
    pub fn size(self) -> f64 {
        Self::size_at(self.lod)
    }

    /// Side length in metres of a region at `lod`.
    #[must_use]
    pub fn size_at(lod: u8) -> f64 {
        REGION_BASE_SIZE * f64::from(1u32 << lod.min(MAX_LOD))
    }

    /// Distance between neighbouring grid vertices in metres, at this LOD.
    #[must_use]
    #[allow(clippy::cast_precision_loss, reason = "GRID_N is 65")]
    pub fn vertex_spacing(self) -> f64 {
        self.size() / (GRID_N - 1) as f64
    }

    /// The corner with the smallest `x` and `z`, at height 0.
    #[must_use]
    pub fn min_corner(self) -> WorldPos {
        let size = self.size();
        WorldPos::from_xz(f64::from(self.x) * size, f64::from(self.z) * size)
    }

    /// The horizontal centre, at height 0.
    #[must_use]
    pub fn center(self) -> WorldPos {
        let half = self.size() * 0.5;
        let min = self.min_corner();
        WorldPos::from_xz(min.x() + half, min.z() + half)
    }

    /// The region at the next coarser LOD that contains this one, or `None`
    /// at [`MAX_LOD`].
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        if self.lod >= MAX_LOD {
            return None;
        }
        Some(Self {
            x: self.x.div_euclid(2),
            z: self.z.div_euclid(2),
            lod: self.lod + 1,
        })
    }

    /// The four regions at the next finer LOD that tile this one, or `None`
    /// at LOD 0. Ordered `(0,0) (1,0) (0,1) (1,1)` in `(x, z)` offsets.
    #[must_use]
    pub fn children(self) -> Option<[Self; 4]> {
        if self.lod == 0 {
            return None;
        }
        let lod = self.lod - 1;
        let (x, z) = (self.x * 2, self.z * 2);
        Some([
            Self { x, z, lod },
            Self { x: x + 1, z, lod },
            Self { x, z: z + 1, lod },
            Self {
                x: x + 1,
                z: z + 1,
                lod,
            },
        ])
    }

    /// The ancestor at `lod`, which is `self` when `lod == self.lod`, or
    /// `None` when `lod` is finer than this region.
    #[must_use]
    pub fn ancestor_at(self, lod: u8) -> Option<Self> {
        if lod < self.lod || lod > MAX_LOD {
            return None;
        }
        let shift = lod - self.lod;
        Some(Self {
            x: self.x >> shift,
            z: self.z >> shift,
            lod,
        })
    }

    /// The region at `lod` whose square contains `pos`. Squares are half-open
    /// on their high edges, so every point belongs to exactly one region per
    /// LOD.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "positions beyond i32 regions are outside the addressable world"
    )]
    pub fn containing(pos: WorldPos, lod: u8) -> Self {
        let size = Self::size_at(lod);
        Self {
            x: (pos.x() / size).floor() as i32,
            z: (pos.z() / size).floor() as i32,
            lod,
        }
    }

    /// Whether `pos` lies inside this region's square, ignoring height.
    #[must_use]
    pub fn contains(self, pos: WorldPos) -> bool {
        let min = self.min_corner();
        let size = self.size();
        pos.x() >= min.x()
            && pos.x() < min.x() + size
            && pos.z() >= min.z()
            && pos.z() < min.z() + size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    #[allow(clippy::float_cmp, reason = "exact powers of two")]
    fn size_doubles_per_lod() {
        assert_eq!(RegionCoord::new(0, 0, 0).size(), 64.0);
        assert_eq!(RegionCoord::new(0, 0, 1).size(), 128.0);
        assert_eq!(RegionCoord::new(0, 0, 8).size(), 16_384.0);
        assert_eq!(RegionCoord::new(0, 0, 0).vertex_spacing(), 1.0);
    }

    #[test]
    fn corners_and_centres() {
        let r = RegionCoord::new(-1, 2, 1);
        assert_eq!(r.min_corner(), WorldPos::from_xz(-128.0, 256.0));
        assert_eq!(r.center(), WorldPos::from_xz(-64.0, 320.0));
    }

    #[test]
    fn negative_parents_round_toward_negative_infinity() {
        assert_eq!(
            RegionCoord::new(-1, -1, 0).parent(),
            Some(RegionCoord::new(-1, -1, 1))
        );
        assert_eq!(
            RegionCoord::new(-2, 1, 0).parent(),
            Some(RegionCoord::new(-1, 0, 1))
        );
    }

    #[test]
    fn edges_are_half_open() {
        let r = RegionCoord::new(0, 0, 0);
        assert!(r.contains(WorldPos::from_xz(0.0, 0.0)));
        assert!(!r.contains(WorldPos::from_xz(64.0, 0.0)));
        assert_eq!(
            RegionCoord::containing(WorldPos::from_xz(64.0, 0.0), 0),
            RegionCoord::new(1, 0, 0)
        );
        assert_eq!(
            RegionCoord::containing(WorldPos::from_xz(-0.001, 0.0), 0),
            RegionCoord::new(-1, 0, 0)
        );
    }

    #[test]
    fn lod_bounds() {
        assert_eq!(RegionCoord::new(0, 0, MAX_LOD).parent(), None);
        assert_eq!(RegionCoord::new(0, 0, 0).children(), None);
        assert_eq!(RegionCoord::new(3, 3, 2).ancestor_at(1), None);
        assert_eq!(
            RegionCoord::new(3, 3, 2).ancestor_at(2),
            Some(RegionCoord::new(3, 3, 2))
        );
    }

    fn region() -> impl Strategy<Value = RegionCoord> {
        // Keep indices where the parent chain stays in i32 and the world
        // position stays far below f64 integer limits.
        (-1_000_000i32..1_000_000, -1_000_000i32..1_000_000, 0u8..=12)
            .prop_map(|(x, z, lod)| RegionCoord::new(x, z, lod))
    }

    proptest! {
        #[test]
        fn containing_recovers_the_region(r in region()) {
            prop_assert_eq!(RegionCoord::containing(r.center(), r.lod), r);
            prop_assert_eq!(RegionCoord::containing(r.min_corner(), r.lod), r);
            prop_assert!(r.contains(r.center()));
        }

        #[test]
        fn children_have_this_parent(r in region()) {
            if let Some(children) = r.children() {
                for c in children {
                    prop_assert_eq!(c.parent(), Some(r));
                    prop_assert!(r.contains(c.center()));
                    prop_assert_eq!(c.ancestor_at(r.lod), Some(r));
                }
            }
        }

        #[test]
        fn parent_lists_this_child(r in region()) {
            let parent = r.parent().expect("lod 12 is below MAX_LOD");
            let children = parent.children().expect("a parent has children");
            prop_assert!(children.contains(&r));
            prop_assert!(parent.contains(r.center()));
        }

        #[test]
        fn containing_commutes_with_parent(r in region()) {
            let pos = r.center();
            let fine = RegionCoord::containing(pos, r.lod);
            let coarse = RegionCoord::containing(pos, r.lod + 1);
            prop_assert_eq!(fine.parent(), Some(coarse));
        }
    }
}
