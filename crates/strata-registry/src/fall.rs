//! `household/fall.ron`: what a fall in the world costs (ticket 0012, step
//! 4a). The member is carried home and sits out played hours; nothing carried
//! is lost. Every figure is a dial here and no other place.

use serde::{Deserialize, Serialize};

/// Where the file lives under the assets root.
pub const FALL_FILE: &str = "household/fall.ron";

/// The fall's dials.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// Played hours a fallen member is out of play, injured.
    pub out_h: f32,
    /// Seconds of the client's fade while the member is carried home.
    pub fade_s: f32,
    /// Played minutes the soonest to heal still needs when nobody is able:
    /// they stand up at once, with this much of the injury left.
    pub early_min: f32,
}

impl FallRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version or a dial that is not a
    /// finite, non-negative number (the fade and the injury may not be nought).
    pub fn parse(text: &str) -> Result<Self, String> {
        let rules: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        if rules.version != Self::VERSION {
            return Err(format!(
                "version {}, this build reads {}",
                rules.version,
                Self::VERSION
            ));
        }
        let dials = [rules.out_h, rules.fade_s, rules.early_min];
        if dials.iter().any(|dial| !dial.is_finite() || *dial < 0.0) {
            return Err("every dial is a finite number, nought or more".to_owned());
        }
        if rules.out_h <= 0.0 || rules.fade_s <= 0.0 {
            return Err("`out_h` and `fade_s` are over nought".to_owned());
        }
        Ok(rules)
    }

    /// The injury in played seconds.
    #[must_use]
    pub fn out_s(&self) -> f64 {
        f64::from(self.out_h) * 3600.0
    }

    /// The injury left a member who is healed early, in played seconds.
    #[must_use]
    pub fn early_s(&self) -> f64 {
        f64::from(self.early_min) * 60.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../assets/household/fall.ron");

    /// The shipped dials parse and a bad one is refused.
    #[test]
    fn the_shipped_dials_parse() {
        let rules = FallRules::parse(SHIPPED).expect("shipped");
        assert!(rules.out_h > 0.0 && rules.fade_s > 0.0);
        assert!((rules.out_s() - f64::from(rules.out_h) * 3600.0).abs() < 1e-9);
        let bad = SHIPPED.replace("out_h: 2.0", "out_h: 0.0");
        assert_ne!(bad, SHIPPED);
        assert!(FallRules::parse(&bad).is_err());
    }
}
