//! `assets/combat/tiers.ron`: how far from Port Serein a fight is and what
//! that does to the people in it (ticket 0012, step 2).
//!
//! The game and the duel harness both ask [`opponent_pools`], so a tier
//! means one thing in the world and in the numbers that tune it.

use serde::{Deserialize, Serialize};

use crate::combat_tables::current;

/// The factors one tier puts on an opponent's pools and blows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierRow {
    /// On `opponents.ron`'s `health`.
    pub health: f32,
    /// On its `shield`, pool and capacity.
    pub shield: f32,
    /// On its `damage_scale`.
    pub damage: f32,
}

impl TierRow {
    /// The row of tier 0, which changes nothing.
    pub const NONE: Self = Self {
        health: 1.0,
        shield: 1.0,
        damage: 1.0,
    };
}

/// What `assets/combat/tiers.ron` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierTable {
    /// See [`crate::combat_tables::ActionTable::version`].
    pub version: u32,
    /// Port Serein's centre, world `(x, z)`.
    pub centre: (f64, f64),
    /// The distance each tier ends at, metres, rising; the last tier has
    /// no end.
    pub bands_m: Vec<f64>,
    /// One row a tier, tier 1 first: one more than there are bands.
    pub tiers: Vec<TierRow>,
}

impl TierTable {
    /// Whether the table can be played.
    ///
    /// # Errors
    /// What is wrong with it.
    pub fn check(&self) -> Result<(), String> {
        if self.tiers.len() != self.bands_m.len() + 1 {
            return Err("tiers needs one row more than bands_m".to_owned());
        }
        if self.bands_m.iter().any(|b| !(*b > 0.0 && b.is_finite()))
            || self.bands_m.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("bands_m must rise from above nought".to_owned());
        }
        for row in &self.tiers {
            for factor in [row.health, row.shield, row.damage] {
                if !(factor > 0.0 && factor.is_finite()) {
                    return Err("a tier's factors must be above nought".to_owned());
                }
            }
        }
        Ok(())
    }

    /// The tier of the point `(x, z)`, 1-based.
    #[must_use]
    pub fn tier_at(&self, x: f64, z: f64) -> u8 {
        let distance = (x - self.centre.0).hypot(z - self.centre.1);
        let under = self.bands_m.iter().take_while(|b| distance >= **b).count();
        u8::try_from(under + 1).unwrap_or(u8::MAX)
    }

    /// The row of `tier`; tier 0 and a tier past the table read as none and
    /// as the last.
    #[must_use]
    pub fn row(&self, tier: u8) -> TierRow {
        match tier {
            0 => TierRow::NONE,
            n => self
                .tiers
                .get(usize::from(n) - 1)
                .or_else(|| self.tiers.last())
                .copied()
                .unwrap_or(TierRow::NONE),
        }
    }
}

/// The tier of `(x, z)` under the installed tables.
#[must_use]
pub fn tier_at(x: f64, z: f64) -> u8 {
    current().tiers.tier_at(x, z)
}

/// An opponent's pools and pressure after its tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpponentPools {
    /// Health.
    pub health: f32,
    /// Shield, and its capacity.
    pub shield: f32,
    /// The damage multiplier.
    pub damage_scale: f32,
}

/// The pools of a hostile of `tier` (0 for the staging fight, which is
/// `opponents.ron` as written), from the installed tables. The one place a
/// tier scales an opponent.
#[must_use]
pub fn opponent_pools(tier: u8) -> OpponentPools {
    let tables = current();
    let row = tables.tiers.row(tier);
    OpponentPools {
        health: tables.opponents.health * row.health,
        shield: tables.opponents.shield * row.shield,
        damage_scale: tables.opponents.damage_scale * row.damage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_tables::CombatTables;

    #[test]
    fn the_bands_pick_a_tier_by_distance_from_the_centre() {
        let table = CombatTables::shipped().tiers;
        let (cx, cz) = table.centre;
        assert_eq!(table.tier_at(cx, cz), 1);
        assert_eq!(table.tier_at(cx + 5_999.0, cz), 1);
        assert_eq!(table.tier_at(cx, cz - 6_000.0), 2);
        assert_eq!(table.tier_at(cx - 14_999.0, cz), 2);
        assert_eq!(table.tier_at(cx + 15_000.0, cz), 3);
        assert_eq!(table.tier_at(cx + 90_000.0, cz + 1.0), 3);
    }

    #[test]
    fn tier_zero_changes_nothing_and_a_tier_scales_the_rows() {
        let tables = CombatTables::shipped();
        assert_eq!(tables.tiers.row(0), TierRow::NONE);
        let rows = &tables.tiers;
        assert!(rows.row(3).health > rows.row(2).health);
        assert_eq!(rows.row(9), rows.row(3));
    }

    #[test]
    fn the_centre_is_port_serein() {
        let registries =
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the assets");
        let city = registries.settlements.first().expect("the city");
        let table = CombatTables::shipped().tiers;
        assert!((table.centre.0 - city.at[0]).abs() < 1e-9);
        assert!((table.centre.1 - city.at[1]).abs() < 1e-9);
    }

    #[test]
    fn a_table_that_cannot_be_played_is_refused() {
        let mut table = CombatTables::shipped().tiers;
        table.bands_m = vec![9_000.0, 4_000.0];
        assert!(table.check().is_err());
        let mut table = CombatTables::shipped().tiers;
        table.tiers.pop();
        assert!(table.check().is_err());
    }
}
