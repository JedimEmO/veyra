//! `procurement/postings.ron`: what a holder's board posts about the camps
//! near it (ticket 0012, step 4b). Every figure is a dial here and no other
//! place; the sim's `postings` module reads them.

use serde::{Deserialize, Serialize};

/// Where the file lives under the assets root.
pub const POSTINGS_FILE: &str = "procurement/postings.ron";

/// The postings' dials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// Postings a board shows at once.
    pub open: u32,
    /// Camps within this of the board's anchor seat are candidates, metres.
    pub reach_m: f64,
    /// Played hours a set of postings stands before the next is drawn.
    pub window_h: f64,
    /// Credits a tier pays, tier 1 first; a tier past the list pays the last.
    pub pay: Vec<u32>,
    /// Jobs held at once.
    pub taken_max: u32,
    /// Radius of the heard-of area round a taken job's camp, metres.
    pub heard_m: f64,
    /// The share of `heard_m` the area's centre may sit off the camp.
    pub heard_offset: f64,
    /// How near the member comes for a camp to be found, metres.
    pub seen_m: f64,
}

impl PostingRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version or a dial that breaks its rule.
    pub fn parse(text: &str) -> Result<Self, String> {
        let rules: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        if rules.version != Self::VERSION {
            return Err(format!(
                "version {}, this build reads {}",
                rules.version,
                Self::VERSION
            ));
        }
        if rules.open == 0 || rules.taken_max == 0 {
            return Err("`open` and `taken_max` are over nought".to_owned());
        }
        let metres = [rules.reach_m, rules.window_h, rules.heard_m, rules.seen_m];
        if metres.iter().any(|dial| !dial.is_finite() || *dial <= 0.0) {
            return Err("`reach_m`, `window_h`, `heard_m` and `seen_m` are over nought".to_owned());
        }
        if !(0.0..=1.0).contains(&rules.heard_offset) {
            return Err("`heard_offset` is a share from 0 to 1".to_owned());
        }
        if rules.pay.is_empty() || rules.pay.contains(&0) {
            return Err("`pay` needs a figure over nought for each tier".to_owned());
        }
        Ok(rules)
    }

    /// Credits a camp of `tier` (1-based) pays.
    #[must_use]
    pub fn pay_of(&self, tier: u8) -> u32 {
        let index = usize::from(tier.max(1)) - 1;
        self.pay
            .get(index)
            .or_else(|| self.pay.last())
            .copied()
            .unwrap_or(0)
    }

    /// A window's length in played seconds.
    #[must_use]
    pub fn window_s(&self) -> f64 {
        self.window_h * 3600.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../assets/procurement/postings.ron");

    /// The shipped dials parse and a bad one is refused.
    #[test]
    fn the_shipped_dials_parse_and_bad_ones_are_refused() {
        let rules = PostingRules::parse(SHIPPED).expect("shipped");
        assert_eq!(rules.open, 3);
        assert_eq!(rules.pay_of(2), 900);
        assert_eq!(rules.pay_of(9), rules.pay_of(3), "past the list");
        for (from, to) in [
            ("open: 3", "open: 0"),
            ("heard_offset: 0.6", "heard_offset: 1.5"),
            ("pay: [400, 900, 1800]", "pay: []"),
        ] {
            let bad = SHIPPED.replace(from, to);
            assert_ne!(bad, SHIPPED);
            assert!(PostingRules::parse(&bad).is_err(), "{to}");
        }
    }
}
