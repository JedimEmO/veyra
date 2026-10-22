//! The height gradient of built surfaces as data (`assets/look/buildings.ron`,
//! review step six, finding F7, ticket 0006 pass 3).
//!
//! Ashlar's anti-tiling shader darkens and warms a wall's foot and lifts its
//! cornice, from the building's base. The dials live here, in one file, and
//! the client writes them to every building material; the shader holds none.
//!
//! Look data: it is in no world version and no handshake.

use serde::{Deserialize, Serialize};

/// Where the file sits under the asset root.
pub const BUILDINGS_FILE: &str = "look/buildings.ron";

/// The gradient of a wall from its base to its cornice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BuildingLook {
    /// Metres of ground grime at the foot of a wall.
    pub grime_m: f32,
    /// How much darker the very foot is than the plain wall, 0 to 1.
    pub grime: f32,
    /// The grime's colour lean at the foot, a multiplier per channel.
    pub warm: [f32; 3],
    /// Metres above the grime over which the wall lifts toward the cornice.
    pub reach_m: f32,
    /// How much lighter the cornice is than the plain wall, 0 to 1.
    pub lift: f32,
}

impl Default for BuildingLook {
    fn default() -> Self {
        Self {
            grime_m: 1.5,
            grime: 0.30,
            warm: [1.06, 1.0, 0.88],
            reach_m: 10.0,
            lift: 0.10,
        }
    }
}

impl BuildingLook {
    /// A look with no gradient: the shader skips it.
    pub const NEUTRAL: Self = Self {
        grime_m: 1.5,
        grime: 0.0,
        warm: [1.0; 3],
        reach_m: 7.0,
        lift: 0.0,
    };

    /// Parses and checks a buildings file.
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
        if !(0.05..=10.0).contains(&self.grime_m) {
            return Err("grime_m must be in 0.05 to 10".into());
        }
        if !(0.05..=60.0).contains(&self.reach_m) {
            return Err("reach_m must be in 0.05 to 60".into());
        }
        if !(0.0..=1.0).contains(&self.grime) {
            return Err("grime must be in 0 to 1".into());
        }
        if !(0.0..=1.0).contains(&self.lift) {
            return Err("lift must be in 0 to 1".into());
        }
        if self.warm.iter().any(|c| !(0.25..=2.0).contains(c)) {
            return Err("every warm lane must be in 0.25 to 2".into());
        }
        Ok(())
    }

    /// The albedo multiplier `height` metres above a building's base: the
    /// function the shader repeats, kept here so the numbers can be tested
    /// and the report can quote them.
    #[must_use]
    pub fn factor(&self, height: f32) -> [f32; 3] {
        let smooth = |edge0: f32, edge1: f32, x: f32| {
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let grime = 1.0 - smooth(0.0, self.grime_m, height);
        let cornice = smooth(self.grime_m, self.grime_m + self.reach_m, height);
        self.warm.map(|w| {
            (1.0 - self.grime * grime) * (1.0 + (w - 1.0) * grime) * (1.0 + self.lift * cornice)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_parses_and_is_the_default() {
        let shipped = BuildingLook::parse(include_str!("../../../assets/look/buildings.ron"))
            .expect("the shipped buildings file parses");
        assert_eq!(shipped, BuildingLook::default());
    }

    #[test]
    fn the_foot_is_darker_and_warmer_and_the_cornice_lighter() {
        let look = BuildingLook::default();
        let foot = look.factor(0.0);
        let mid = look.factor(1.5);
        let top = look.factor(12.0);
        assert!(foot[1] < 0.75 && foot[0] > foot[2], "{foot:?}");
        assert!(mid.iter().all(|c| (c - 1.0).abs() < 1e-6), "{mid:?}");
        assert!(top.iter().all(|c| *c > 1.07), "{top:?}");
        assert!(
            BuildingLook::NEUTRAL
                .factor(0.3)
                .iter()
                .all(|c| (c - 1.0).abs() < 1e-6)
        );
    }

    #[test]
    fn a_bad_dial_is_refused() {
        assert!(BuildingLook::parse("(grime: 1.5)").is_err());
        assert!(BuildingLook::parse("(reach_m: 0.0)").is_err());
        assert!(BuildingLook::parse("(typo: 1)").is_err());
        assert!(BuildingLook::parse("()").is_ok());
    }
}
