//! `crafting/wear.ron`: how an item wears and what a repair kit restores
//! (crafting.md, 4.6). Every figure is whole thousandths or points so the
//! sim's wear arithmetic is integer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::item::{ItemError, Property, QUALITY_MAX};

/// The id of the line a repair kit carries, which says how much it restores.
pub const RESTORE: &str = "restore";

/// The id of the line that says how long an item lasts: a cost of wear is
/// scaled by it, and an item without it reads store grade.
pub const DURABILITY: &str = "durability";

/// What one kit restores away from a workbench: `at_zero + per_quality *
/// line / 1000` points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRepair {
    /// Points a kit with a restore line of nought restores.
    pub at_zero: u32,
    /// What a whole scale of line adds to that.
    pub per_quality: u32,
}

/// The wear rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WearRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// Condition runs from nought to this.
    pub full: u16,
    /// At or above this an item performs as made; below it its lines slide.
    pub slide_below: u16,
    /// The HUD shows the weapon's bar only below this.
    pub show_below: u16,
    /// What one shot costs the weapon in hand, in thousandths of a point,
    /// at durability 500.
    pub shot: u32,
    /// What one cut costs, likewise.
    pub cut: u32,
    /// Points off everything carried that has a condition when its bearer
    /// goes down.
    pub down: u16,
    /// A cost is scaled by `(durability_at_zero - line) / 1000`.
    pub durability_at_zero: u32,
    /// The record's condition moves in whole steps of this many points, so
    /// a save is not written a shot.
    pub step: u16,
    /// What the weapon's own row is multiplied by at condition nought.
    pub worst: BTreeMap<Property, f32>,
    /// What one kit restores away from a workbench.
    pub field: FieldRepair,
}

impl Default for WearRules {
    /// A data set without the file: nothing wears and a kit restores
    /// nothing, so every item stays as made.
    fn default() -> Self {
        Self {
            version: Self::VERSION,
            full: QUALITY_MAX,
            slide_below: 0,
            show_below: 0,
            shot: 0,
            cut: 0,
            down: 0,
            durability_at_zero: 1000,
            step: 1,
            worst: BTreeMap::new(),
            field: FieldRepair {
                at_zero: 0,
                per_quality: 0,
            },
        }
    }
}

impl WearRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse and check the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, thresholds out of order, a
    /// step that does not divide the scale, or a `worst` on a property the
    /// fight has no number for.
    pub fn parse(text: &str) -> Result<Self, ItemError> {
        let rules: Self = ron::from_str(text).map_err(|e| ItemError::Ron(e.to_string()))?;
        if rules.version != Self::VERSION {
            return Err(ItemError::Version {
                found: rules.version,
                expected: Self::VERSION,
            });
        }
        let bad = |why: String| Err(ItemError::Wear(why));
        if rules.full != QUALITY_MAX {
            return bad(format!("`full` is {QUALITY_MAX}, the scale's top"));
        }
        if rules.slide_below > rules.show_below || rules.show_below > rules.full {
            return bad("thresholds run slide_below <= show_below <= full".to_owned());
        }
        if rules.step == 0 || !rules.full.is_multiple_of(rules.step) {
            return bad("`step` must divide `full`".to_owned());
        }
        if rules.durability_at_zero < 1000 {
            return bad(
                "`durability_at_zero` is under 1000, so a cost would go negative".to_owned(),
            );
        }
        if rules.field.at_zero + rules.field.per_quality > u32::from(rules.full) {
            return bad("a field repair would restore more than `full`".to_owned());
        }
        for (property, factor) in &rules.worst {
            if !property.live() {
                return bad(format!(
                    "`worst` names {property:?}, which the fight has no number for"
                ));
            }
            if !factor.is_finite() || *factor <= 0. {
                return bad(format!("`worst` for {property:?} is not a positive factor"));
            }
        }
        Ok(rules)
    }
}
