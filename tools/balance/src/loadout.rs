//! Modules as a dimension of the harness. A side may carry modules on its
//! weapons; at spawn the bout equips its combatant with the loadout the
//! game would give it, through the same `veyra_rules::fitting` the game
//! uses, so what a module is worth here is what it is worth in play. A
//! side with none is not equipped at all, and every older row and band
//! keeps its number.
//!
//! A harness side has no weapon records of its own, only a family in each
//! hand, so a module is fitted to a bought example of the weapon it is
//! made for: every line at store grade, which opens the mounts a bought
//! weapon has and no more.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use strata_registry::item::{
    Fitted, HANDS, ItemCatalogue, ItemKind, QUALITY_MAX, Record, STORE_GRADE,
};
use strata_registry::{Namespaced, load_from_dir};
use veyra_rules::combat::Weapon;
use veyra_rules::fitting::{self, Loadout};

use crate::bout::{Matchup, Side, SideMod};

/// The items the sides' modules are read against: process-wide like the
/// combat tables and the graph, since a bout is built from a `Matchup`
/// that names modules and nothing else. The checkout's own `assets/`
/// unless `install` is called; nothing at all when that does not load,
/// and then every module is refused by name.
static ITEMS: LazyLock<RwLock<Arc<ItemCatalogue>>> = LazyLock::new(|| {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let items = load_from_dir(assets)
        .map(|registries| registries.items)
        .unwrap_or_default();
    RwLock::new(Arc::new(items))
});

/// Install the items every later bout reads its sides' modules against.
pub fn install(items: ItemCatalogue) {
    *ITEMS.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(items);
}

/// The installed items.
#[must_use]
pub fn items() -> Arc<ItemCatalogue> {
    ITEMS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The family a module of `fits` goes on: the first weapon it is made for
/// that the sim has a family for, with that weapon's item.
fn host<'a>(items: &ItemCatalogue, fits: &'a [Namespaced]) -> Option<(Weapon, &'a Namespaced)> {
    fits.iter().find_map(|item| match &items.get(item)?.kind {
        ItemKind::Weapon { family, .. } => Some((Weapon::from_name(family)?, item)),
        _ => None,
    })
}

/// A module from the command line's spelling: its item id, then, after an
/// `@`, any of its quality lines as `line=value`, comma separated. A line
/// left out is store grade, so the bare id is the bought module.
/// `veyra:module.patrol_extended_feed@capacity=610,reload_time=433` is the
/// batch of the first day's store.
///
/// # Errors
/// An id that is no item or no module, a line the module has not got, or
/// a value that is no quality.
pub fn parse(text: &str) -> anyhow::Result<SideMod> {
    let items = items();
    let (name, graded) = text.trim().split_once('@').unwrap_or((text.trim(), ""));
    let id = Namespaced::parse(name).map_err(|error| anyhow::anyhow!("`{name}`: {error}"))?;
    let Some(def) = items.get(&id) else {
        let modules: Vec<String> = items
            .iter()
            .filter(|def| matches!(def.kind, ItemKind::Module { .. }))
            .map(|def| def.id.to_string())
            .collect();
        anyhow::bail!("no item `{id}`; the modules are {}", modules.join(", "));
    };
    let ItemKind::Module { fits, .. } = &def.kind else {
        anyhow::bail!("`{id}` is no module");
    };
    let Some((weapon, _)) = host(&items, fits) else {
        anyhow::bail!("`{id}` fits no weapon the sim has a family for");
    };
    let mut lines: BTreeMap<String, u16> = def
        .lines
        .iter()
        .map(|line| (line.id.clone(), STORE_GRADE))
        .collect();
    for pair in graded
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
    {
        let Some((line, value)) = pair.split_once('=') else {
            anyhow::bail!("`{pair}`: a line is `name=value`");
        };
        let Some(slot) = lines.get_mut(line.trim()) else {
            anyhow::bail!(
                "`{id}` has no line `{}`; it has {}",
                line.trim(),
                def.lines
                    .iter()
                    .map(|line| line.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        *slot = value
            .trim()
            .parse()
            .ok()
            .filter(|quality| *quality <= QUALITY_MAX)
            .ok_or_else(|| anyhow::anyhow!("`{pair}`: a quality is 0 to {QUALITY_MAX}"))?;
    }
    Ok(SideMod {
        weapon,
        module: Fitted {
            item: id,
            batch: 0,
            by: None,
            lines,
            condition: None,
        },
    })
}

/// A bought example of `item`: every line at store grade, in full
/// condition, nothing fitted.
fn bought(items: &ItemCatalogue, item: &Namespaced) -> Option<Record> {
    let def = items.get(item)?;
    Some(Record {
        item: item.clone(),
        count: 1,
        batch: 0,
        by: None,
        lines: def
            .lines
            .iter()
            .map(|line| (line.id.clone(), STORE_GRADE))
            .collect(),
        condition: Some(QUALITY_MAX),
        fitted: BTreeMap::new(),
        origin: None,
    })
}

/// The hand `weapon` is in: the one holding its family, else the first
/// that is empty.
fn slot_for(
    items: &ItemCatalogue,
    hands: &[Option<Record>; HANDS],
    weapon: Weapon,
) -> anyhow::Result<usize> {
    hands
        .iter()
        .position(|held| {
            held.as_ref()
                .is_some_and(|held| items.family_of(held) == Some(weapon.name()))
        })
        .or_else(|| hands.iter().position(Option::is_none))
        .ok_or_else(|| anyhow::anyhow!("no hand free for the {}", weapon.name()))
}

/// The loadout of `side`: each of its modules fitted to a bought example
/// of the weapon it is made for, in the hand of that weapon's family, and
/// the hands read as the game reads a player's.
///
/// # Errors
/// A module that is no module, that names a family it is not made for, or
/// that the weapon refuses: a shut mount, a mount already taken by
/// another of the side's modules.
pub fn of(side: &Side) -> anyhow::Result<Loadout> {
    let items = items();
    let mut hands: [Option<Record>; HANDS] = std::array::from_fn(|_| None);
    for SideMod { weapon, module } in &side.fitted {
        let id = &module.item;
        let Some(ItemKind::Module { slot, fits, .. }) = items.get(id).map(|def| &def.kind) else {
            anyhow::bail!("`{id}` is no module");
        };
        let host = fits.iter().find(|item| {
            matches!(
                items.get(item).map(|def| &def.kind),
                Some(ItemKind::Weapon { family, .. }) if family == weapon.name()
            )
        });
        let Some(host) = host else {
            anyhow::bail!("`{id}` is made for no {}", weapon.name());
        };
        let hand = slot_for(&items, &hands, *weapon)?;
        let held = match hands[hand].take() {
            Some(held) => held,
            None => bought(&items, host).ok_or_else(|| anyhow::anyhow!("no item `{host}`"))?,
        };
        if held.fitted.contains_key(slot) {
            anyhow::bail!("`{id}`: the {}'s {slot} is taken already", weapon.name());
        }
        let fitted = fitting::with(&items, &held, slot, Some(module))
            .map_err(|refusal| anyhow::anyhow!("`{id}` on `{}`: {refusal}", held.item))?;
        hands[hand] = Some(fitted);
    }
    // A worn weapon, or one with lines of its own, is the side's weapon in a
    // bought example with those set, before the loadout reads it.
    if side.condition.is_some() || side.lines.is_some() {
        let hand = slot_for(&items, &hands, side.weapon)?;
        let mut held = if let Some(held) = hands[hand].take() {
            held
        } else {
            let item = items
                .iter()
                .find(|def| {
                    matches!(&def.kind, ItemKind::Weapon { family, .. } if family == side.weapon.name())
                })
                .map(|def| def.id.clone())
                .ok_or_else(|| anyhow::anyhow!("no weapon of the {}", side.weapon.name()))?;
            bought(&items, &item).ok_or_else(|| anyhow::anyhow!("no item `{item}`"))?
        };
        for (line, value) in side.lines.iter().flatten() {
            let Some(slot) = held.lines.get_mut(line) else {
                anyhow::bail!(
                    "`{}` has no line `{line}`; it has {}",
                    held.item,
                    held.lines.keys().cloned().collect::<Vec<_>>().join(", ")
                );
            };
            if *value > QUALITY_MAX {
                anyhow::bail!("`{line}={value}`: a quality is 0 to {QUALITY_MAX}");
            }
            *slot = *value;
        }
        if let Some(condition) = side.condition {
            held.condition = Some(condition);
        }
        hands[hand] = Some(held);
    }
    items
        .check_hands(&hands)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(fitting::loadout(&items, &hands))
}

/// Whether both sides' modules are loadouts, before any bout runs.
///
/// # Errors
/// The first that is not, naming the matchup and the side.
pub fn check(matchup: &Matchup) -> anyhow::Result<()> {
    for (name, side) in [("a", &matchup.a), ("b", &matchup.b)] {
        of(side).map_err(|error| anyhow::anyhow!("{} {name}: {error}", matchup.name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::item::Property;

    const FEED: &str = "veyra:module.patrol_extended_feed";

    fn side(fitted: Vec<SideMod>) -> Side {
        Side {
            fitted,
            ..crate::preset("carbine-vs-dummy").expect("the preset").a
        }
    }

    #[test]
    fn an_unknown_module_is_refused() {
        let error = parse("veyra:module.no_such_feed").expect_err("no such item");
        assert!(
            error.to_string().contains("no item") && error.to_string().contains(FEED),
            "{error}"
        );
        assert!(parse("veyra:weapon.patrol_carbine").is_err(), "a weapon");
        assert!(
            parse(&format!("{FEED}@spread=600")).is_err(),
            "not its line"
        );
        assert!(
            parse(&format!("{FEED}@capacity=1001")).is_err(),
            "over the top"
        );
        assert!(parse(&format!("{FEED}@capacity")).is_err(), "no value");

        // A module that parsed, put on the wrong family, is refused when
        // the run checks its sides and not by a panic at a spawn.
        let mut feed = parse(FEED).expect("the bought magazine");
        feed.weapon = Weapon::Pistol;
        let error = of(&side(vec![feed.clone()])).expect_err("the pistol takes no patrol feed");
        assert!(error.to_string().contains("no Pistol"), "{error}");
        let mut matchup = crate::preset("carbine-vs-dummy").expect("the preset");
        matchup.b.fitted = vec![feed];
        let error = check(&matchup).expect_err("side b is wrong");
        assert!(
            error.to_string().starts_with("carbine-vs-dummy b:"),
            "{error}"
        );
        // Two for one mount is one too many.
        let feed = parse(FEED).expect("the bought magazine");
        assert!(of(&side(vec![feed.clone(), feed])).is_err());
    }

    #[test]
    fn a_spelling_is_a_module_with_its_lines() {
        let bought = parse(FEED).expect("the bought magazine");
        assert_eq!(bought.weapon, Weapon::Carbine);
        assert_eq!(
            bought.module.lines,
            BTreeMap::from([
                ("capacity".to_owned(), 500),
                ("reload_time".to_owned(), 500)
            ])
        );
        let made = parse(&format!(" {FEED}@capacity=610, reload_time=433 ")).expect("batch 412");
        assert_eq!(made.module.lines["capacity"], 610);
        assert_eq!(made.module.lines["reload_time"], 433);
        let cell = parse("veyra:module.pistol_extended_cell").expect("the cell");
        assert_eq!(cell.weapon, Weapon::Pistol);

        // The loadout is the game's: the feed's two factors on the carbine,
        // the cell's on the pistol, and the sight's spread and draw.
        let sight = parse("veyra:module.vigil_reflex").expect("the sight");
        let loadout = of(&side(vec![bought, cell, sight])).expect("three modules");
        let properties = |weapon| -> Vec<Property> {
            loadout.mods[&weapon]
                .iter()
                .map(|factor| factor.property)
                .collect()
        };
        assert_eq!(
            properties(Weapon::Carbine),
            [
                Property::AimSpread,
                Property::EquipTime,
                Property::Capacity,
                Property::ReloadTime,
                // The bought carbine's own action line, after its parts.
                Property::RepeatInterval
            ]
        );
        assert_eq!(
            properties(Weapon::Pistol),
            [Property::Capacity, Property::ReloadTime]
        );
        assert!(of(&side(Vec::new())).expect("nothing").is_empty());
    }

    /// A crafted magazine is a module with line figures, which is how the
    /// harness sweeps quality with no bench and no resources: the capacity
    /// line a session finishes at is the number after `capacity=`. The
    /// patrol magazine's thirtieth round comes at 622, a bought one is 29,
    /// and what the first day's lots make (a line in the 400s to 500s) is
    /// 28 or 29: the factors the fight gets are the fitting step's own.
    #[test]
    fn a_crafted_line_is_a_figure_on_the_command_line() {
        let rounds = |spelling: &str| {
            let feed = parse(spelling).expect("a patrol magazine");
            let loadout = of(&side(vec![feed])).expect("one module");
            let capacity = loadout.mods[&Weapon::Carbine]
                .iter()
                .find(|factor| factor.property == Property::Capacity)
                .expect("a magazine moves the capacity")
                .factor;
            loadout
                .count
                .whole(f32::from(Weapon::Carbine.spec().magazine) * capacity)
        };
        assert_eq!(Weapon::Carbine.spec().magazine, 24);
        assert_eq!(rounds(FEED), 29);
        assert_eq!(rounds(&format!("{FEED}@capacity=622")), 30);
        assert_eq!(rounds(&format!("{FEED}@capacity=621")), 29);
        assert_eq!(rounds(&format!("{FEED}@capacity=448")), 29);
        assert_eq!(rounds(&format!("{FEED}@capacity=447")), 28);
        // The worked example's finished magazine (crafting.md, 5.5).
        assert_eq!(rounds(&format!("{FEED}@capacity=608,reload_time=437")), 29);
        // Quality moves the reload too, the other way: a drawback softens.
        let reload = |spelling: &str| {
            let feed = parse(spelling).expect("a patrol magazine");
            of(&side(vec![feed])).expect("one module").mods[&Weapon::Carbine]
                .iter()
                .find(|factor| factor.property == Property::ReloadTime)
                .expect("a magazine moves the reload")
                .factor
        };
        assert!(reload(&format!("{FEED}@reload_time=437")) > reload(FEED));
        assert!(reload(&format!("{FEED}@reload_time=620")) < reload(FEED));
    }
}
