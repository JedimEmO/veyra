//! Reworking a weapon's mounts (ticket 0007, step 9; `docs/design/crafting.md`,
//! 4.2).
//!
//! A smith at a bench spends one mount blank on the weapon in a hand and
//! rolls once: the weapon's moddability moves toward the blank's temper by
//! the share the outcome closes (`assets/crafting/rework.ron`), so a bought
//! carbine can be made a platform. Every figure is whole per-mille and every
//! dial is data. [`check_rework`] is the one rule, asked by the request and
//! by the screen alike; a refusal changes nothing, not a blank and not a
//! roll.
//!
//! Local-only like the stock it changes: nothing here is replicated.

use bevy_ecs::prelude::*;

use strata_registry::GameRegistries;

use strata_registry::item::{ItemCatalogue, ItemKind, Record};

use strata_registry::rework::ReworkRules;

use strata_registry::schematic::{Hand, Outcome};

use crate::crafting::{self, CraftState, kind_of, reachable};

use crate::fitting::{self, WorkbenchAccess};

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::progression::{CharacterSheet, Reach};

/// Why a weapon would not be reworked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReworkRefusal {
    /// No workbench is within reach.
    #[error("a rework is done at a workbench")]
    NoBench,
    /// The character has not learned the box that teaches it.
    #[error("Needs {needs}.")]
    NotTaught {
        /// The box, as a screen words it.
        needs: String,
    },
    /// Nothing in that hand, or a thing with no moddability line.
    #[error("there is no weapon in that hand to rework")]
    NoWeapon,
    /// A menu's cursor holds a stack, which a rework would leave behind.
    #[error("put down what is on the cursor first")]
    Busy,
    /// No mount blank in the pack, its bags or the store.
    #[error("no mount blank to hand")]
    NoBlank,
    /// The best blank is no harder than the mounts already are.
    #[error("this blank is no harder than the mounts: temper {temper}, moddability {moddability}")]
    NoGain {
        /// The best blank's temper.
        temper: u16,
        /// The weapon's moddability.
        moddability: u16,
    },
}

/// What a rework would do, for a screen to say before it is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outlook {
    /// The weapon's moddability now.
    pub from: u16,
    /// The temper of the blank that would be spent.
    pub temper: u16,
    /// Where the likeliest outcome puts it.
    pub most_often: u16,
    /// Where the worst outcome with odds above nought puts it.
    pub least: u16,
    /// Where the best outcome with odds above nought puts it.
    pub most: u16,
    /// The blank, as a kind of stack.
    pub blank: Record,
}

/// A weapon reworked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reworked {
    /// The hand.
    pub hand: usize,
    /// What the roll came to.
    pub outcome: Outcome,
    /// The moddability before.
    pub from: u16,
    /// The moddability after.
    pub to: u16,
    /// The mounts that were shut and are open now.
    pub opened: Vec<String>,
}

fn line_of(record: &Record, line: &str) -> u16 {
    record.lines.get(line).copied().unwrap_or(0)
}

/// The blank with the highest temper there is to hand: in the pack, its
/// bags and, as the bench's candidates reach it, the store. Of two as hard
/// the first met, the pack's before the store's.
#[must_use]
pub fn best_blank(
    registries: &GameRegistries,
    pack: &Pack,
    store: Option<&HouseholdStore>,
) -> Option<Record> {
    let rules = registries.rework.as_ref()?;
    let mut best: Option<Record> = None;
    for (record, _) in reachable(&registries.items, pack, store) {
        if record.item != rules.blank {
            continue;
        }
        let harder = best.as_ref().is_none_or(|held| {
            line_of(&record, &rules.blank_line) > line_of(held, &rules.blank_line)
        });
        if harder {
            best = Some(kind_of(&record));
        }
    }
    best
}

/// The moddability a weapon of `from` takes from a blank of `temper` when
/// the roll comes to `outcome`: the share it closes of the gap, rounded down.
#[must_use]
pub fn result_of(rules: &ReworkRules, outcome: Outcome, from: u16, temper: u16) -> u16 {
    let gap = u32::from(temper.saturating_sub(from));
    let moved = u32::from(rules.closes.of(outcome)) * gap / 1000;
    from + u16::try_from(moved).unwrap_or(0)
}

/// Whether the weapon in `hand` can be reworked at this bench by this sheet
/// from what `pack` carries and `store` holds, and what it would do.
/// Asked in the order a player would fix things: the bench, the box, the
/// weapon, a menu's cursor, a blank, the gain.
///
/// # Errors
/// The first of those that fails.
#[allow(
    clippy::too_many_arguments,
    reason = "the data, the bench rule, the sheet and the stock the rule reads, and the hand"
)]
pub fn check_rework(
    registries: &GameRegistries,
    access: WorkbenchAccess,
    reach: &Reach,
    sheet: &CharacterSheet,
    pack: &Pack,
    hands: &Hands,
    store: Option<&HouseholdStore>,
    hand: usize,
) -> Result<Outlook, ReworkRefusal> {
    let Some(rules) = &registries.rework else {
        return Err(ReworkRefusal::NoWeapon);
    };
    if !fitting::at_workbench(access, reach) {
        return Err(ReworkRefusal::NoBench);
    }
    if !sheet.has(&rules.r#box) {
        let needs = registries
            .progression
            .find_with_tree(&rules.r#box)
            .map_or_else(
                || rules.r#box.clone(),
                |(tree, found)| format!("{}: {}", tree.name, found.name),
            );
        return Err(ReworkRefusal::NotTaught { needs });
    }
    let items = &registries.items;
    let weapon = hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
        .filter(|record| {
            matches!(
                items.get(&record.item).map(|def| &def.kind),
                Some(ItemKind::Weapon { .. })
            ) && record.lines.contains_key(&rules.weapon_line)
        })
        .ok_or(ReworkRefusal::NoWeapon)?;
    if pack.cursor.is_some() {
        return Err(ReworkRefusal::Busy);
    }
    let blank = best_blank(registries, pack, store).ok_or(ReworkRefusal::NoBlank)?;
    let from = line_of(&weapon, &rules.weapon_line);
    let temper = line_of(&blank, &rules.blank_line);
    if temper <= from {
        return Err(ReworkRefusal::NoGain {
            temper,
            moddability: from,
        });
    }
    let odds = registries
        .schematics
        .rules()
        .odds(Hand::Steady, registries.schematics.rules().base_row());
    let reached = |outcome: Outcome| result_of(rules, outcome, from, temper);
    let possible = || {
        Outcome::ALL
            .into_iter()
            .filter(|outcome| odds[outcome.index()] > 0)
            .map(reached)
    };
    // The likeliest, and of two as likely the better.
    let most_often = Outcome::ALL
        .into_iter()
        .rev()
        .max_by_key(|outcome| odds[outcome.index()])
        .map_or(from, reached);
    Ok(Outlook {
        from,
        temper,
        most_often,
        least: possible().min().unwrap_or(from),
        most: possible().max().unwrap_or(from),
        blank,
    })
}

/// The mounts of `weapon` that its moddability keeps shut.
fn shut(items: &ItemCatalogue, weapon: &Record) -> Vec<String> {
    let Some(ItemKind::Weapon { mounts, .. }) = items.get(&weapon.item).map(|def| &def.kind) else {
        return Vec::new();
    };
    mounts
        .iter()
        .filter(|mount| {
            items
                .mount_opens(weapon, &mount.slot)
                .is_some_and(|o| o.is_err())
        })
        .map(|mount| mount.slot.clone())
        .collect()
}

/// Do the rework `outlook` was drawn for: one roll from the household's
/// counter on the steady odds at the base row, one blank leaves the stock
/// whatever the outcome, and the weapon's moddability moves. Fitted parts,
/// condition, batch and maker are untouched. One transaction: it errs
/// before anything changes.
///
/// # Errors
/// [`ReworkRefusal::NoWeapon`] or [`ReworkRefusal::NoBlank`] if the stock is
/// not what the outlook was drawn from.
pub fn rework(
    registries: &GameRegistries,
    seed: strata_core::Seed,
    state: &mut CraftState,
    stock: &mut Stock<'_>,
    hand: usize,
    outlook: &Outlook,
) -> Result<Reworked, ReworkRefusal> {
    let rules = registries.rework.as_ref().ok_or(ReworkRefusal::NoWeapon)?;
    let items = &registries.items;
    let weapon = stock
        .hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
        .ok_or(ReworkRefusal::NoWeapon)?;
    let spend = items::write(&outlook.blank, items).map_err(|_| ReworkRefusal::NoBlank)?;
    let mut scratch = crafting::Scratch::of(stock);
    if !scratch.take(items, &spend, 1) {
        return Err(ReworkRefusal::NoBlank);
    }
    let sheet_rules = registries.schematics.rules();
    let odds = sheet_rules.odds(Hand::Steady, sheet_rules.base_row());
    let outcome = crafting::roll(seed, state.rolls, &odds);
    state.rolls = state.rolls.wrapping_add(1);
    let from = line_of(&weapon, &rules.weapon_line);
    let to = result_of(rules, outcome, from, outlook.temper).max(from);
    let mut next = weapon.clone();
    next.lines.insert(rules.weapon_line.clone(), to);
    let before = shut(items, &weapon);
    let after = shut(items, &next);
    let opened = before
        .into_iter()
        .filter(|slot| !after.contains(slot))
        .collect();
    let written = items::write(&next, items).map_err(|_| ReworkRefusal::NoWeapon)?;
    scratch.hands.0.set(hand, Some(written));
    scratch.hands.0.clear_changed();
    scratch.commit(stock);
    Ok(Reworked {
        hand,
        outcome,
        from,
        to,
        opened,
    })
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    use crate::crafting::CraftState;

    use crate::crafting::test_support::CORE;
    use crate::crafting::test_support::id;
    use crate::crafting::test_support::part;

    use crate::crafting::test_support::shipped;

    use crate::progression::Reach;

    use strata_registry::item::Record;

    use strata_registry::schematic::Outcome;
    use strata_types::Registries;

    pub const BLANK: &str = "veyra:component.mount_blank";

    pub const COMPENSATOR: &str = "veyra:module.vigil_compensator";

    pub fn blank(registries: &Registries, temper: u16) -> Record {
        if temper == 500 {
            part(registries, BLANK, 1, None)
        } else {
            part(registries, BLANK, 1, Some((3, &[temper])))
        }
    }

    pub fn at_bench() -> Reach {
        Reach {
            workbench: true,
            ..Reach::default()
        }
    }

    /// Every outcome closes its share of the gap, rounded down.
    #[test]
    fn each_outcome_closes_its_share_of_the_gap() {
        let registries = shipped();
        let rules = registries.0.rework.as_ref().unwrap();
        let reached = |outcome| result_of(rules, outcome, 258, 500);
        assert_eq!(
            Outcome::ALL.map(reached),
            [500, 439, 379, 318, 258],
            "a gap of 242 closes by 1000, 750, 500, 250 and 0"
        );
        assert_eq!(result_of(rules, Outcome::Exceptional, 500, 700), 700);
        assert_eq!(result_of(rules, Outcome::Good, 500, 700), 650);
    }

    /// The bests survive a save, and a key no schematic line owns is dropped
    /// with a word.
    #[test]
    fn the_bests_survive_a_save_and_a_stranger_is_dropped() {
        let registries = shipped();
        let mut state = CraftState::default();
        state
            .bests
            .insert("veyra:schematic.cell_core#purity".to_owned(), 640);
        let line = &registries.0.schematics.get(&id(CORE)).unwrap().def.lines[0].line;
        state.bests.insert(format!("{CORE}#{line}"), 612);
        let text = ron::ser::to_string_pretty(&state, ron::ser::PrettyConfig::default()).unwrap();
        let mut read: CraftState = ron::from_str(&text).unwrap();
        assert_eq!(read, state);
        read.bests.insert("veyra:schematic.nothing#x".to_owned(), 1);
        read.bests.insert("garbage".to_owned(), 1);
        let said = read.repair(&registries.0);
        assert_eq!(said.len(), 3, "{said:?}");
        assert_eq!(read.bests.len(), 1);
        assert_eq!(read.bests[&format!("{CORE}#{line}")], 612);
    }

    /// A workbench teaches the trees that say so and a trainer the ones it
    /// names; neither teaches the other's.
    #[test]
    fn a_bench_teaches_its_trees_and_a_trainer_its_own() {
        let registries = shipped();
        let graph = &registries.0.progression;
        let bench = at_bench();
        assert!(bench.teaches(graph, "weaponsmith.frames.2"));
        assert!(bench.teaches(graph, "fabricator.fixtures.1"));
        assert!(!bench.teaches(graph, "blade.cuts.1"));
        assert!(!Reach::default().teaches(graph, "weaponsmith.frames.2"));
        let trainer = Reach {
            trees: ["blade".to_owned()].into(),
            ..Reach::default()
        };
        assert!(trainer.teaches(graph, "blade.cuts.1"));
        assert!(!trainer.teaches(graph, "weaponsmith.frames.2"));
    }
}
