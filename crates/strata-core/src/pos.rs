//! `f64` world positions.

use core::ops::{Add, AddAssign, Sub, SubAssign};

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// A position in the world, in metres, `y` up.
///
/// This is the only position type the simulation, the world generator and
/// the streaming logic use. It is `f64` so a world tens of kilometres across
/// keeps sub-millimetre precision everywhere; the render layer subtracts a
/// `RenderOrigin` and casts to `f32` at the last moment (ADR 0002). A newtype
/// rather than a bare `DVec3` so an `f32` `Transform` can never be mistaken
/// for a world position, and so that world and render space differ in type,
/// not just in convention.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorldPos(pub DVec3);

impl WorldPos {
    /// The world origin.
    pub const ZERO: Self = Self(DVec3::ZERO);

    /// A position from its three components.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }

    /// A position on the `y = 0` plane. Region corners and sample points are
    /// horizontal; height comes from the world generator.
    #[must_use]
    pub const fn from_xz(x: f64, z: f64) -> Self {
        Self(DVec3::new(x, 0.0, z))
    }

    /// East-west coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.0.x
    }

    /// Height.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.0.y
    }

    /// North-south coordinate.
    #[must_use]
    pub const fn z(self) -> f64 {
        self.0.z
    }

    /// The horizontal components, which is what terrain and streaming
    /// distance checks use.
    #[must_use]
    pub const fn xz(self) -> DVec2 {
        DVec2::new(self.0.x, self.0.z)
    }

    /// The same horizontal position at another height.
    #[must_use]
    pub const fn with_y(self, y: f64) -> Self {
        Self(DVec3::new(self.0.x, y, self.0.z))
    }

    /// Straight-line distance.
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        self.0.distance(other.0)
    }

    /// Distance ignoring height. LOD selection is horizontal, since a
    /// mountain top and its valley are meshed at the same detail.
    #[must_use]
    pub fn horizontal_distance(self, other: Self) -> f64 {
        self.xz().distance(other.xz())
    }

    /// The offset from `origin` to this position, still in `f64`. The render
    /// layer casts this to `f32`; nothing below it does.
    #[must_use]
    pub fn relative_to(self, origin: Self) -> DVec3 {
        self.0 - origin.0
    }
}

impl From<DVec3> for WorldPos {
    fn from(v: DVec3) -> Self {
        Self(v)
    }
}

impl From<WorldPos> for DVec3 {
    fn from(p: WorldPos) -> Self {
        p.0
    }
}

impl Add<DVec3> for WorldPos {
    type Output = Self;

    fn add(self, rhs: DVec3) -> Self {
        Self(self.0 + rhs)
    }
}

impl AddAssign<DVec3> for WorldPos {
    fn add_assign(&mut self, rhs: DVec3) {
        self.0 += rhs;
    }
}

impl Sub<DVec3> for WorldPos {
    type Output = Self;

    fn sub(self, rhs: DVec3) -> Self {
        Self(self.0 - rhs)
    }
}

impl SubAssign<DVec3> for WorldPos {
    fn sub_assign(&mut self, rhs: DVec3) {
        self.0 -= rhs;
    }
}

/// Position minus position is a displacement, not a position.
impl Sub for WorldPos {
    type Output = DVec3;

    fn sub(self, rhs: Self) -> DVec3 {
        self.0 - rhs.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn displacement_and_relative_agree() {
        let a = WorldPos::new(10.0, 2.0, -3.0);
        let b = WorldPos::new(4.0, 0.0, 1.0);
        assert_eq!(a - b, a.relative_to(b));
        assert_eq!(b + (a - b), a);
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "a 3-4-5 triangle is exact")]
    fn horizontal_distance_ignores_height() {
        let a = WorldPos::new(0.0, 100.0, 0.0);
        let b = WorldPos::new(3.0, -50.0, 4.0);
        assert_eq!(a.horizontal_distance(b), 5.0);
        assert!(a.distance(b) > 5.0);
    }

    #[test]
    fn serde_is_transparent() {
        let p = WorldPos::new(1.0, 2.0, 3.0);
        let text = ron::to_string(&p).expect("serialises");
        assert_eq!(text, "(1.0,2.0,3.0)");
        let back: WorldPos = ron::from_str(&text).expect("round trips");
        assert_eq!(back, p);
    }
}
