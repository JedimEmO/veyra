//! `crafting/rework.ron`: how a smith raises the moddability of a weapon
//! that already exists (crafting.md, 4.2). Every figure is whole thousandths
//! so the sim's arithmetic is integer.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::item::{ItemCatalogue, ItemError, ItemKind};
use crate::progression::SkillGraph;
use crate::schematic::Outcome;

/// Where the file lives under the assets root.
pub const REWORK_FILE: &str = "crafting/rework.ron";

/// The share of the gap each outcome closes, in thousandths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Closes {
    /// The best a roll gives.
    pub exceptional: u16,
    /// What a steady hand most often gets.
    pub good: u16,
    /// A middling lift.
    pub fair: u16,
    /// Barely a lift.
    pub poor: u16,
    /// Nothing is gained.
    pub setback: u16,
}

impl Closes {
    /// The share `outcome` closes.
    #[must_use]
    pub const fn of(&self, outcome: Outcome) -> u16 {
        match outcome {
            Outcome::Exceptional => self.exceptional,
            Outcome::Good => self.good,
            Outcome::Fair => self.fair,
            Outcome::Poor => self.poor,
            Outcome::Setback => self.setback,
        }
    }
}

/// The rework rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReworkRules {
    /// Bumped by hand when the meaning of a field changes.
    pub version: u32,
    /// The component one rework spends.
    pub blank: Namespaced,
    /// The line of the blank that is worth something.
    pub blank_line: String,
    /// The line of the weapon that is raised.
    pub weapon_line: String,
    /// The skill box that teaches it.
    pub r#box: String,
    /// The share of the gap each outcome closes.
    pub closes: Closes,
}

impl ReworkRules {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Parse the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version, a share over the whole or
    /// shares that rise from Exceptional to Setback.
    pub fn parse(text: &str) -> Result<Self, ItemError> {
        let rules: Self = ron::from_str(text).map_err(|e| ItemError::Ron(e.to_string()))?;
        if rules.version != Self::VERSION {
            return Err(ItemError::Version {
                found: rules.version,
                expected: Self::VERSION,
            });
        }
        let shares = Outcome::ALL.map(|outcome| rules.closes.of(outcome));
        if shares.iter().any(|share| *share > 1000) {
            return Err(ItemError::Rework(
                "a share closes more than the whole gap".to_owned(),
            ));
        }
        if shares.windows(2).any(|pair| pair[0] < pair[1]) {
            return Err(ItemError::Rework(
                "`closes` must not rise from Exceptional to Setback".to_owned(),
            ));
        }
        Ok(rules)
    }

    /// Hold the rules to the items and the skill graph they name.
    ///
    /// # Errors
    /// A blank that is no item or lacks its line, a weapon line no weapon
    /// carries, or a box the graph does not have.
    pub fn check(&self, items: &ItemCatalogue, graph: &SkillGraph) -> Result<(), ItemError> {
        let bad = |why: String| Err(ItemError::Rework(why));
        let Some(blank) = items.get(&self.blank) else {
            return bad(format!("the blank `{}` is no item", self.blank));
        };
        if !blank.lines.iter().any(|line| line.id == self.blank_line) {
            return bad(format!(
                "the blank `{}` has no line `{}`",
                self.blank, self.blank_line
            ));
        }
        let carried = items.iter().any(|def| {
            matches!(def.kind, ItemKind::Weapon { .. })
                && def.lines.iter().any(|line| line.id == self.weapon_line)
        });
        if !carried {
            return bad(format!("no weapon has a line `{}`", self.weapon_line));
        }
        if graph.find(&self.r#box).is_none() {
            return bad(format!(
                "the box `{}` is not in the skill graph",
                self.r#box
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_from_dir;

    const SHIPPED: &str = include_str!("../../../assets/crafting/rework.ron");

    fn assets() -> crate::GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).expect("assets/ loads")
    }

    fn refused(text: &str, registries: &crate::GameRegistries) -> String {
        let rules = match ReworkRules::parse(text) {
            Ok(rules) => rules,
            Err(error) => return error.to_string(),
        };
        rules
            .check(&registries.items, &registries.progression)
            .expect_err("refused")
            .to_string()
    }

    /// The shipped rules load and are held to what they name.
    #[test]
    fn the_shipped_rules_load_and_their_names_are_held() {
        let registries = assets();
        let rules = registries.rework.as_ref().expect("ships");
        assert_eq!(rules.r#box, "weaponsmith.frames.2");
        assert_eq!(rules.closes.of(Outcome::Good), 750);
        let said = refused(
            &SHIPPED.replace("veyra:component.mount_blank", "veyra:component.nothing"),
            &registries,
        );
        assert!(said.contains("veyra:component.nothing"), "{said}");
        let said = refused(
            &SHIPPED.replace("weaponsmith.frames.2", "weaponsmith.frames.9"),
            &registries,
        );
        assert!(said.contains("weaponsmith.frames.9"), "{said}");
        let said = refused(&SHIPPED.replace("\"temper\"", "\"mettle\""), &registries);
        assert!(said.contains("mettle"), "{said}");
        let said = refused(&SHIPPED.replace("setback: 0", "setback: 300"), &registries);
        assert!(said.contains("rise"), "{said}");
    }

    /// Only the two crafting trades are taught at a bench.
    #[test]
    fn only_the_crafting_trees_are_taught_at_a_bench() {
        let registries = assets();
        let benched: Vec<&str> = registries
            .progression
            .trees
            .iter()
            .filter(|tree| tree.bench)
            .map(|tree| tree.id.as_str())
            .collect();
        assert_eq!(benched, ["fabricator", "weaponsmith"]);
    }
}
