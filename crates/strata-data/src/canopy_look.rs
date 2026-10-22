//! The crown's grouping as data (`assets/look/canopy.ron`, review step six,
//! finding F5, ticket 0006 pass 2).
//!
//! A tree vertex carries its crown depth and curvature; the factor here turns
//! them into a dark cool core and lighter outer clumps. The content step bakes
//! it into the impostor atlases and the foliage shaders apply it to the near
//! meshes, from this one file, so the two cannot drift apart at the swap.
//!
//! Look data: it is in no world version and no handshake.

use serde::{Deserialize, Serialize};

/// Where the file sits under the asset root.
pub const CANOPY_FILE: &str = "look/canopy.ron";

/// How a crown's leaves are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CanopyLook {
    /// The albedo share left at the centre of a crown, 0 to 1.
    pub core: f32,
    /// The core's colour lean, a luminance-one-ish multiplier that cools it.
    pub cool: [f32; 3],
    /// How much a bulging clump lifts (and a crease darkens) its albedo.
    pub lift: f32,
    /// The sunlight through the outer leaves seen against the sun, 0 to 2.
    pub rim: f32,
}

impl Default for CanopyLook {
    fn default() -> Self {
        Self {
            core: 0.60,
            cool: [0.92, 1.0, 1.08],
            lift: 0.50,
            rim: 0.04,
        }
    }
}

impl CanopyLook {
    /// A look with no grouping at all: every factor is one.
    pub const NEUTRAL: Self = Self {
        core: 1.0,
        cool: [1.0; 3],
        lift: 0.0,
        rim: 0.0,
    };

    /// Parses and checks a canopy file.
    ///
    /// # Errors
    ///
    /// The parse error, or the first dial out of its range.
    pub fn parse(text: &str) -> Result<Self, String> {
        let look: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        look.check()?;
        Ok(look)
    }

    fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.core) {
            return Err("core must be in 0 to 1".into());
        }
        if self.cool.iter().any(|c| !(0.25..=2.0).contains(c)) {
            return Err("every cool lane must be in 0.25 to 2".into());
        }
        if !(0.0..=1.0).contains(&self.lift) {
            return Err("lift must be in 0 to 1".into());
        }
        if !(0.0..=2.0).contains(&self.rim) {
            return Err("rim must be in 0 to 2".into());
        }
        Ok(())
    }

    /// The albedo multiplier at a vertex of crown `depth` (0 outside, 1
    /// deep inside) and `curvature` (-1 to 1): the one function the impostor
    /// bake runs and the shaders repeat.
    #[must_use]
    pub fn factor(&self, depth: f32, curvature: f32) -> [f32; 3] {
        let depth = depth.clamp(0.0, 1.0);
        let clump = (1.0 + self.lift * curvature.clamp(-1.0, 1.0)).max(0.0);
        // `depth` is 1 inside, so the core's share is the inverse.
        self.cool
            .map(|c| (self.core * c + (1.0 - self.core * c) * (1.0 - depth)) * clump)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_parses_and_is_the_default() {
        let shipped = CanopyLook::parse(include_str!("../../../assets/look/canopy.ron"))
            .expect("the shipped canopy file parses");
        assert_eq!(shipped, CanopyLook::default());
    }

    #[test]
    fn the_core_is_darker_and_a_clump_lighter() {
        let look = CanopyLook::default();
        let outside = look.factor(0.0, 0.0);
        let inside = look.factor(1.0, 0.0);
        assert!(outside.iter().all(|c| (c - 1.0).abs() < 1e-6));
        assert!(inside[1] < 0.7 && inside[2] > inside[0], "{inside:?}");
        assert!(look.factor(0.0, 0.5)[1] > 1.2);
        assert!(
            CanopyLook::NEUTRAL
                .factor(1.0, 0.7)
                .iter()
                .all(|c| (c - 1.0).abs() < 1e-6)
        );
    }

    #[test]
    fn a_bad_dial_is_refused() {
        assert!(CanopyLook::parse("(core: 1.5)").is_err());
        assert!(CanopyLook::parse("(rim: -1.0)").is_err());
        assert!(CanopyLook::parse("(typo: 1)").is_err());
        assert!(CanopyLook::parse("()").is_ok());
    }
}
