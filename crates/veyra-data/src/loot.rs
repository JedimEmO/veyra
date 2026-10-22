//! What a cleared place leaves behind (ticket 0012, step 4a): a table per
//! tier, one file a kind of encounter, `loot/*.loot.ron`.
//!
//! A table is credits, picks of existing items (salvage: components and
//! modules whose lines are drawn low and wide, so a smith has something to
//! rework), lots of the nearest ground of a class, and rare finds. The sim's
//! `loot::roll` draws from it; this crate holds it to the items, the resource
//! classes and the encounter it names. In no world version and no handshake.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::encounter::{EncounterDefId, EncounterSet};
use crate::item::{ItemCatalogue, QUALITY_MAX};
use crate::resource::ResourceSet;

/// The directory of `.loot.ron` files, relative to the asset root.
pub const LOOT_DIR: &str = "loot";
/// The suffix a loot file must carry to be read.
pub const LOOT_SUFFIX: &str = ".loot.ron";
/// The only version of the file this build reads.
pub const LOOT_VERSION: u32 = 1;

/// Who a salvaged record names as its maker.
pub const SALVAGE_MAKER: &str = "salvage";

/// An inclusive range of whole numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span(pub u32, pub u32);

impl Span {
    /// Whether the low end is not above the high end.
    #[must_use]
    pub const fn is_ordered(self) -> bool {
        self.0 <= self.1
    }
}

/// A find of an existing item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pick {
    /// The item.
    pub item: Namespaced,
    /// How many, when it is found. One for anything that stacks to one.
    pub count: Span,
    /// The chance it is found, per-mille.
    pub chance: u16,
    /// Where each of its lines falls, per-mille. Present for an item with
    /// lines and absent for one without.
    #[serde(default)]
    pub lines: Option<Span>,
}

/// A find of ground: units of the nearest deposit of a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotPick {
    /// The resource class, `strata:structural_metal`.
    pub class: Namespaced,
    /// How many units.
    pub units: Span,
    /// The chance it is found, per-mille.
    pub chance: u16,
}

/// What one tier of a place leaves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierLoot {
    /// Credits, drawn between the two.
    pub credits: Span,
    /// Salvage.
    #[serde(default)]
    pub picks: Vec<Pick>,
    /// Ground.
    #[serde(default)]
    pub lots: Vec<LotPick>,
    /// Rarely a module.
    #[serde(default)]
    pub rare: Vec<Pick>,
}

/// One loot file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LootDef {
    /// Must equal [`LOOT_VERSION`].
    pub version: u32,
    /// The encounter whose cleared places leave it.
    pub encounter: EncounterDefId,
    /// How far from the place a lot's deposit is looked for, metres.
    pub lots_within_m: f64,
    /// One table a tier, tier 1 first.
    pub tiers: Vec<TierLoot>,
}

impl LootDef {
    /// The table of a tier (1-based), the last one for a tier past the list.
    #[must_use]
    pub fn tier(&self, tier: u8) -> Option<&TierLoot> {
        let index = usize::from(tier.max(1)) - 1;
        self.tiers.get(index).or_else(|| self.tiers.last())
    }
}

/// Why a loot file was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LootError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// A file of another version than this build reads.
    #[error("version {found}, this build reads {expected}")]
    Version {
        /// The version the file states.
        found: u32,
        /// The version this build reads.
        expected: u32,
    },
    /// Two files are for one encounter.
    #[error("`{0}` has two loot files")]
    Duplicate(EncounterDefId),
    /// A table breaks a rule.
    #[error("`{encounter}`: {reason}")]
    Table {
        /// The encounter.
        encounter: EncounterDefId,
        /// What is wrong.
        reason: String,
    },
}

/// Every loot file, checked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LootSet {
    defs: Vec<LootDef>,
}

impl LootSet {
    /// Check every file `(path, def)` against the encounters, the items and
    /// the resources.
    ///
    /// # Errors
    /// The path of the first file refused, and why.
    pub fn build(
        defs: Vec<(String, LootDef)>,
        encounters: &EncounterSet,
        items: &ItemCatalogue,
        resources: &ResourceSet,
    ) -> Result<Self, (String, LootError)> {
        let mut kept: Vec<LootDef> = Vec::new();
        for (path, def) in defs {
            if def.version != LOOT_VERSION {
                return Err((
                    path,
                    LootError::Version {
                        found: def.version,
                        expected: LOOT_VERSION,
                    },
                ));
            }
            if kept.iter().any(|each| each.encounter == def.encounter) {
                return Err((path, LootError::Duplicate(def.encounter)));
            }
            check(&def, encounters, items, resources).map_err(|reason| {
                (
                    path,
                    LootError::Table {
                        encounter: def.encounter.clone(),
                        reason,
                    },
                )
            })?;
            kept.push(def);
        }
        kept.sort_by_key(|def| def.encounter.to_string());
        Ok(Self { defs: kept })
    }

    /// What the encounter `id` leaves.
    #[must_use]
    pub fn of(&self, id: &EncounterDefId) -> Option<&LootDef> {
        self.defs.iter().find(|def| def.encounter == *id)
    }

    /// Every file, by encounter id.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &LootDef> {
        self.defs.iter()
    }
}

fn check(
    def: &LootDef,
    encounters: &EncounterSet,
    items: &ItemCatalogue,
    resources: &ResourceSet,
) -> Result<(), String> {
    let Some(encounter) = encounters.get(&def.encounter) else {
        return Err("no such encounter".to_owned());
    };
    if !def.lots_within_m.is_finite() || def.lots_within_m <= 0.0 {
        return Err("`lots_within_m` is over nought".to_owned());
    }
    if def.tiers.len() != encounter.tiers.len() {
        return Err(format!(
            "{} tiers of loot for an encounter of {}",
            def.tiers.len(),
            encounter.tiers.len()
        ));
    }
    for (index, tier) in def.tiers.iter().enumerate() {
        let at = |what: &str| format!("tier {}: {what}", index + 1);
        if !tier.credits.is_ordered() {
            return Err(at("credits run from high to low"));
        }
        for (name, picks) in [("picks", &tier.picks), ("rare", &tier.rare)] {
            for pick in picks {
                check_pick(pick, items).map_err(|why| at(&format!("{name}: {why}")))?;
            }
        }
        for lot in &tier.lots {
            if !resources
                .classes()
                .iter()
                .any(|class| class.id == lot.class)
            {
                return Err(at(&format!("lots: `{}` is no resource class", lot.class)));
            }
            if !lot.units.is_ordered() || lot.units.0 == 0 {
                return Err(at(&format!("lots: `{}` units are no range", lot.class)));
            }
            if lot.chance > 1000 {
                return Err(at(&format!("lots: `{}` chance is over 1000", lot.class)));
            }
        }
    }
    Ok(())
}

fn check_pick(pick: &Pick, items: &ItemCatalogue) -> Result<(), String> {
    let id = &pick.item;
    let Some(def) = items.get(id) else {
        return Err(format!("`{id}` is no item"));
    };
    if pick.chance > 1000 {
        return Err(format!("`{id}` chance is over 1000"));
    }
    if !pick.count.is_ordered() || pick.count.0 == 0 {
        return Err(format!("`{id}` count is no range"));
    }
    if pick.count.1 > def.stack {
        return Err(format!("`{id}` count is past its stack of {}", def.stack));
    }
    match (def.lines.is_empty(), pick.lines) {
        (true, Some(_)) => Err(format!("`{id}` has no lines to range")),
        (false, None) => Err(format!("`{id}` has lines and no range for them")),
        (false, Some(range)) => {
            if !range.is_ordered() || range.1 > u32::from(QUALITY_MAX) {
                Err(format!(
                    "`{id}` lines are no range within 0 to {QUALITY_MAX}"
                ))
            } else {
                Ok(())
            }
        }
        (true, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::load_from_dir;

    const SHIPPED: &str = include_str!("../../../assets/loot/outlaw_camp.loot.ron");

    fn assets() -> crate::GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap()
    }

    fn refused(from: &str, to: &str, reason: &str) {
        let registries = assets();
        let changed = SHIPPED.replace(from, to);
        assert_ne!(changed, SHIPPED, "`{from}` is in the file");
        let def: LootDef = ron::from_str(&changed).unwrap();
        let error = LootSet::build(
            vec![("loot/test.loot.ron".to_owned(), def)],
            &registries.encounters,
            &registries.items,
            &registries.resources,
        )
        .expect_err("the edit is refused")
        .1
        .to_string();
        assert!(error.contains(reason), "`{error}` should say `{reason}`");
    }

    /// The shipped table loads, one table a tier of the camp.
    #[test]
    fn the_shipped_table_loads() {
        let registries = assets();
        let id = Namespaced::parse("strata:outlaw_camp").unwrap();
        let loot = registries.loot.of(&id).expect("the camp's loot");
        assert_eq!(loot.tiers.len(), 3);
        assert_eq!(loot.tier(9), loot.tier(3), "a tier past the list");
    }

    /// A file is held to the items, the classes and its own ranges.
    #[test]
    fn a_table_is_held_to_what_it_names() {
        refused(
            "veyra:component.drive_coil",
            "veyra:component.nothing",
            "is no item",
        );
        refused(
            "strata:structural_metal",
            "strata:no_class",
            "no resource class",
        );
        refused("credits: (60, 140)", "credits: (140, 60)", "high to low");
        refused(
            "lines: Some((150, 650))",
            "lines: None",
            "no range for them",
        );
        refused("strata:outlaw_camp", "strata:nobody", "no such encounter");
    }
}
