//! Buyers: who pays for what the household makes and draws, and at what
//! quality (ticket 0007, step 7; `docs/design/crafting.md`, 7), as
//! `procurement/*.buyer.ron` spells them, one a file.
//!
//! A buyer has orders, an upkeep that refills with played time or a project
//! that is bought once and ends, and an order has lines: the item or class
//! of resource wanted, the quality figure it is graded on, the minimum, the
//! quantity still wanted and the price a unit. The sim works the money in
//! whole thousandths (`strata_sim::procurement`); this module only holds
//! the figures and refuses what could not be sold.
//!
//! Everything a line could get wrong is refused at load with the file's
//! name. This step sells stacks of parts, modules, kits and lots only, so a
//! weapon, a bag or a blueprint is not a thing a line may want.
//!
//! None of it is world state: it joins no world version and no handshake.
//! A data set without the directory has no buyers, and that is legal.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::item::{ItemCatalogue, ItemKind};
use crate::resource::{Attribute, ResourceSet};

/// The directory of `.buyer.ron` files, relative to the asset root.
pub const BUYERS_DIR: &str = "procurement";
/// The suffix a buyer file must carry to be read.
pub const BUYER_SUFFIX: &str = ".buyer.ron";
/// The only version of the file this build reads.
pub const BUYER_VERSION: u32 = 1;

/// What a premium is worth: the pay above the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Premium {
    /// Thousandths of the price each point of grade above the minimum adds.
    pub per_point: u32,
    /// The most it adds, in thousandths of the price.
    pub cap: u32,
}

/// What a line wants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wants {
    /// One item, a part, a module, a kit or a lot's own item.
    Item(Namespaced),
    /// A lot of any type of a class of resource.
    Class(Namespaced),
}

/// The figure a line is graded on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradeOn {
    /// A made item's quality line, by its id in the item file.
    Line(String),
    /// A lot's attribute.
    Attribute(Attribute),
}

impl GradeOn {
    /// The key the figure is under in a record.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Line(line) => line,
            Self::Attribute(attribute) => attribute.key(),
        }
    }
}

/// How an upkeep line comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refill {
    /// How many units each interval gives back.
    pub units: u32,
    /// The interval, in played seconds.
    pub every_s: u32,
}

/// Whether an order comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderKind {
    /// Wear and tear: its lines refill with played time.
    Upkeep,
    /// A bill bought once, which ends.
    Project,
}

/// One line of an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineSpec {
    /// Its key within the buyer, which the sim's ledger names it by.
    pub id: String,
    /// What it wants.
    pub wants: Wants,
    /// The figure it grades on.
    pub grade: GradeOn,
    /// The lowest grade bought.
    pub min: u16,
    /// How much is wanted in all.
    pub quantity: u32,
    /// Credits a unit at the minimum.
    pub price: u32,
    /// How it comes back; upkeep only.
    #[serde(default)]
    pub refill: Option<Refill>,
}

/// One order: a name and its lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderDef {
    /// Its key within the buyer.
    pub id: String,
    /// What the board calls it.
    pub name: String,
    /// Upkeep or project.
    pub kind: OrderKind,
    /// What it buys.
    pub lines: Vec<LineSpec>,
}

/// A buyer, as the file spells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuyerDef {
    /// The version of the format.
    pub version: u32,
    /// The namespaced id, `veyra:buyer.pelagic_works`.
    pub id: Namespaced,
    /// What the board calls it.
    pub name: String,
    /// What quality is worth.
    pub premium: Premium,
    /// Its orders, in the order the board lists them.
    pub orders: Vec<OrderDef>,
    /// What it sells the household, in the order the board lists them.
    #[serde(default)]
    pub sells: Vec<Sale>,
}

/// One thing a buyer sells: a store grade one of an item for credits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sale {
    /// The item sold.
    pub item: Namespaced,
    /// Credits for one.
    pub price: u32,
}

impl BuyerDef {
    /// Every line with the order it belongs to, in the file's order.
    pub fn lines(&self) -> impl Iterator<Item = (&OrderDef, &LineSpec)> {
        self.orders
            .iter()
            .flat_map(|order| order.lines.iter().map(move |line| (order, line)))
    }

    /// One line by its id.
    #[must_use]
    pub fn line(&self, id: &str) -> Option<(&OrderDef, &LineSpec)> {
        self.lines().find(|(_, line)| line.id == id)
    }
}

/// Why a buyer was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuyerError {
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
    /// Two files define one buyer.
    #[error("`{0}` is defined twice")]
    Duplicate(Namespaced),
    /// A line or an order breaks a rule.
    #[error("`{buyer}`: {reason}")]
    Buyer {
        /// The buyer.
        buyer: Namespaced,
        /// What is wrong.
        reason: String,
    },
}

/// Every buyer, checked, in id order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuyerSet {
    buyers: Vec<BuyerDef>,
}

impl BuyerSet {
    /// Check `defs` against the items and the resources.
    ///
    /// # Errors
    /// The path of the first file that breaks a rule, and which.
    pub fn build(
        mut defs: Vec<(String, BuyerDef)>,
        items: &ItemCatalogue,
        resources: &ResourceSet,
    ) -> Result<Self, (String, BuyerError)> {
        defs.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
        for pair in defs.windows(2) {
            if pair[0].1.id == pair[1].1.id {
                return Err((
                    pair[1].0.clone(),
                    BuyerError::Duplicate(pair[1].1.id.clone()),
                ));
            }
        }
        for (path, def) in &defs {
            if def.version != BUYER_VERSION {
                return Err((
                    path.clone(),
                    BuyerError::Version {
                        found: def.version,
                        expected: BUYER_VERSION,
                    },
                ));
            }
            check(def, items, resources).map_err(|reason| {
                (
                    path.clone(),
                    BuyerError::Buyer {
                        buyer: def.id.clone(),
                        reason,
                    },
                )
            })?;
        }
        Ok(Self {
            buyers: defs.into_iter().map(|(_, def)| def).collect(),
        })
    }

    /// One buyer by id.
    #[must_use]
    pub fn get(&self, id: &Namespaced) -> Option<&BuyerDef> {
        self.buyers.iter().find(|buyer| buyer.id == *id)
    }

    /// Every buyer, in id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &BuyerDef> {
        self.buyers.iter()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buyers.is_empty()
    }
}

fn check(def: &BuyerDef, items: &ItemCatalogue, resources: &ResourceSet) -> Result<(), String> {
    for sale in &def.sells {
        if items.get(&sale.item).is_none() {
            return Err(format!("sells `{}`, which is no item", sale.item));
        }
        if sale.price == 0 {
            return Err(format!("sells `{}` for nought", sale.item));
        }
    }
    let mut seen = BTreeSet::new();
    for (order, line) in def.lines() {
        let id = &line.id;
        if !seen.insert(id.as_str()) {
            return Err(format!("the line `{id}` is defined twice"));
        }
        if line.quantity == 0 || line.price == 0 {
            return Err(format!("line `{id}` has a quantity or a price of nought"));
        }
        match (order.kind, line.refill) {
            (OrderKind::Project, Some(_)) => {
                return Err(format!("line `{id}` is in a project and may not refill"));
            }
            (OrderKind::Upkeep, None) => {
                return Err(format!("line `{id}` is in an upkeep and must refill"));
            }
            (_, Some(refill)) if refill.units == 0 || refill.every_s == 0 => {
                return Err(format!(
                    "line `{id}` refills nought units or in nought seconds"
                ));
            }
            _ => {}
        }
        match (&line.wants, &line.grade) {
            (Wants::Item(item), grade) => {
                let def = items
                    .get(item)
                    .ok_or_else(|| format!("line `{id}` wants `{item}`, which is no item"))?;
                if matches!(
                    def.kind,
                    ItemKind::Weapon { .. } | ItemKind::Bag { .. } | ItemKind::Blueprint
                ) {
                    return Err(format!(
                        "line `{id}` wants `{item}`, which is a weapon, a bag or a blueprint"
                    ));
                }
                let key = grade.key();
                let known = match (&def.kind, grade) {
                    (ItemKind::Lot { ranges, .. }, GradeOn::Attribute(_)) => {
                        ranges.iter().any(|(attribute, _)| attribute.key() == key)
                    }
                    (_, GradeOn::Line(_)) => def.lines.iter().any(|each| each.id == key),
                    _ => false,
                };
                if !known {
                    return Err(format!(
                        "line `{id}` grades `{item}` on `{key}`, which it lacks"
                    ));
                }
            }
            (Wants::Class(class), grade) => {
                let class = resources
                    .classes()
                    .iter()
                    .find(|each| each.id == *class)
                    .ok_or_else(|| format!("line `{id}` wants `{class}`, which is no class"))?;
                let GradeOn::Attribute(attribute) = grade else {
                    return Err(format!(
                        "line `{id}` grades a class on a line, not an attribute"
                    ));
                };
                if !class.carries.contains(attribute) {
                    return Err(format!(
                        "line `{id}` grades `{}` on {attribute:?}, which it does not carry",
                        class.id
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::load_from_dir;

    fn shipped() -> crate::GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).expect("assets/ loads")
    }

    const FILE: &str = include_str!("../../../assets/procurement/pelagic_works.buyer.ron");

    fn built(text: &str) -> Result<BuyerSet, String> {
        let registries = shipped();
        let def: BuyerDef = ron::from_str(text).expect("the test's buyer parses");
        BuyerSet::build(
            vec![("procurement/test.buyer.ron".to_owned(), def)],
            &registries.items,
            &registries.resources,
        )
        .map_err(|(_, error)| error.to_string())
    }

    #[track_caller]
    fn refused(from: &str, to: &str, reason: &str) {
        let changed = FILE.replace(from, to);
        assert_ne!(changed, FILE, "`{from}` is in the file");
        let error = built(&changed).expect_err("the edit is refused");
        assert!(error.contains(reason), "`{error}` should say `{reason}`");
    }

    #[test]
    fn the_shipped_buyer_loads() {
        let registries = shipped();
        let id = Namespaced::parse("veyra:buyer.pelagic_works").unwrap();
        let buyer = registries.buyers.get(&id).expect("Pelagic Works is there");
        assert_eq!(buyer.name, "Pelagic Works");
        // The upkeep's three, the east intake and the station's record
        // (ticket 0014).
        assert_eq!(buyer.lines().count(), 5);
        let (order, line) = buyer.line("intake_metal").unwrap();
        assert_eq!((order.kind, line.refill), (OrderKind::Project, None));
        assert_eq!(registries.buyers.iter().len(), 1);
    }

    #[test]
    fn what_cannot_be_sold_is_refused() {
        refused(
            "veyra:component.cell_core",
            "veyra:component.nope",
            "no item",
        );
        refused(
            "strata:structural_metal\"), grade: Attribute(Strength), min: 450",
            "strata:nope\"), grade: Attribute(Strength), min: 450",
            "no class",
        );
        refused(
            "Line(\"charge_density\")",
            "Line(\"nope\")",
            "which it lacks",
        );
        refused(
            "Attribute(Strength), min: 450",
            "Attribute(Potency), min: 450",
            "does not carry",
        );
        refused("quantity: 40", "quantity: 0", "nought");
        refused("price: 120", "price: 0", "nought");
        refused(
            "refill: None",
            "refill: Some((units: 1, every_s: 60))",
            "may not refill",
        );
        refused(
            "refill: Some((units: 4, every_s: 600))",
            "refill: None",
            "must refill",
        );
        refused("units: 4,", "units: 0,", "nought");
        refused("every_s: 900", "every_s: 0", "nought");
        refused(
            "id: \"patrol_magazines\"",
            "id: \"cell_cores\"",
            "defined twice",
        );
        refused(
            "Item(\"veyra:component.cell_core\")",
            "Item(\"veyra:weapon.patrol_carbine\")",
            "weapon, a bag or a blueprint",
        );
        refused(
            "Item(\"veyra:component.cell_core\")",
            "Item(\"veyra:bag.field_satchel\")",
            "weapon, a bag or a blueprint",
        );
        refused(
            "Item(\"veyra:component.cell_core\"), grade: Line(\"charge_density\")",
            "Item(\"veyra:blueprint.production\"), grade: Line(\"charge_density\")",
            "weapon, a bag or a blueprint",
        );
    }

    #[test]
    fn what_it_sells_is_checked() {
        let registries = shipped();
        let id = Namespaced::parse("veyra:buyer.pelagic_works").unwrap();
        let buyer = registries.buyers.get(&id).unwrap();
        let sold: Vec<(String, u32)> = buyer
            .sells
            .iter()
            .map(|sale| (sale.item.to_string(), sale.price))
            .collect();
        assert_eq!(
            sold,
            [
                ("veyra:vehicle.courier_bike".to_owned(), 450),
                ("veyra:tool.survey".to_owned(), 120),
                ("veyra:station.field_harvester".to_owned(), 300),
                ("veyra:weapon.service_blade".to_owned(), 180),
                ("veyra:weapon.yard_greatblade".to_owned(), 260),
                ("veyra:weapon.pier_glaive".to_owned(), 240),
                ("veyra:weapon.marksman_rifle".to_owned(), 520)
            ]
        );
        refused(
            "item: \"veyra:vehicle.courier_bike\"",
            "item: \"veyra:vehicle.nope\"",
            "no item",
        );
        refused("price: 450", "price: 0", "for nought");
        let bare = FILE.replace(
            "sells: [\n        (item: \"veyra:vehicle.courier_bike\", price: 450),\n        (item: \"veyra:tool.survey\", price: 120),\n        (item: \"veyra:station.field_harvester\", price: 300),\n        (item: \"veyra:weapon.service_blade\", price: 180),\n        (item: \"veyra:weapon.yard_greatblade\", price: 260),\n        (item: \"veyra:weapon.pier_glaive\", price: 240),\n        (item: \"veyra:weapon.marksman_rifle\", price: 520),\n    ],",
            "",
        );
        assert_ne!(bare, FILE);
        assert!(built(&bare).unwrap().iter().all(|b| b.sells.is_empty()));
    }
}
