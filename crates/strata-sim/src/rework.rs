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

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use strata_registry::GameRegistries;
use strata_registry::item::{ItemCatalogue, ItemKind, Record};
use strata_registry::progression::XpKind;
use strata_registry::rework::ReworkRules;
use strata_registry::schematic::{Hand, Outcome};

use crate::WorldSeed;
use crate::crafting::{self, CraftState, kind_of, reachable};
use crate::fitting::{self, WorkbenchAccess};
use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};
use crate::progression::{CharacterSheet, Reach, XpGained};
use crate::{LocalPlayer, Registries, SimSet, SimTick};

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

/// What a request asks of the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReworkRequest {
    /// Rework the weapon in this hand.
    Rework {
        /// The hand: the carbine's, then the pistol's.
        hand: usize,
    },
}

/// The requests not yet consumed, a resource for the reason the stock's
/// inbox is one.
#[derive(Resource, Debug, Default)]
pub struct ReworkInbox(pub Vec<ReworkRequest>);

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

/// The sim's answer to a [`ReworkRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ReworkAnswer {
    /// The request it answers.
    pub request: ReworkRequest,
    /// What happened, or why nothing did.
    pub result: Result<Reworked, ReworkRefusal>,
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

/// `SimTick`, `SimSet::Input`, after equip's own: every rework asked for
/// since the last tick, in order, on the local player's hands, each on what
/// the one before left. A rework that raised the line credits Fabrication XP.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the seed, the stock, the ledger and the sheet in, three messages out"
)]
pub fn consume(
    mut inbox: ResMut<ReworkInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    seed: Option<Res<WorldSeed>>,
    mut state: ResMut<CraftState>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<
        (Entity, &mut Pack, &mut Hands, Option<&mut CharacterSheet>),
        With<LocalPlayer>,
    >,
    open: Res<OpenStock>,
    mut answers: MessageWriter<ReworkAnswer>,
    mut gained: MessageWriter<XpGained>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let registries = &registries.0;
    let seed = seed.map_or(strata_core::Seed(0), |seed| seed.0);
    let mut player = players.single_mut().ok();
    let mut changed = false;
    for request in requests {
        let ReworkRequest::Rework { hand } = request;
        let result = match player.as_mut() {
            None => Err(ReworkRefusal::NoWeapon),
            Some((entity, pack, hands, sheet)) => {
                let blank_sheet = CharacterSheet::default();
                let standing = sheet.as_deref().unwrap_or(&blank_sheet);
                let outlook = check_rework(
                    registries,
                    *access,
                    &reach,
                    standing,
                    pack,
                    hands,
                    Some(&store),
                    hand,
                );
                match outlook {
                    Err(refusal) => Err(refusal),
                    Ok(outlook) => {
                        let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                        let mut ledger = state.clone();
                        let mut stock = Stock {
                            saddle: None,
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: Some(&mut next.2),
                        };
                        let done =
                            rework(registries, seed, &mut ledger, &mut stock, hand, &outlook);
                        if let Ok(done) = &done {
                            if **pack != next.0 {
                                **pack = next.0;
                            }
                            **hands = next.1;
                            if *store != next.2 {
                                *store = next.2;
                            }
                            *state = ledger;
                            changed = true;
                            let amount = registries.progression.rates.craft.rework;
                            if done.to > done.from
                                && amount > 0
                                && let Some(sheet) = sheet
                            {
                                sheet.credit(XpKind::Fabrication, amount);
                                gained.write(XpGained {
                                    entity: *entity,
                                    kind: XpKind::Fabrication,
                                    amount,
                                    kill: false,
                                });
                            }
                        }
                        done
                    }
                }
            }
        };
        answers.write(ReworkAnswer {
            request: ReworkRequest::Rework { hand },
            result,
        });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some((_, pack, hands, _)) = player.as_mut()
    {
        // A stores screen left open under the fitting one must hear that a
        // blank went and the hand changed; the copies keep change
        // detection out of a read.
        let mut truth = ((**pack).clone(), (**hands).clone(), store.clone());
        let stock = Stock {
            saddle: None,
            pack: &mut truth.0,
            hands: &mut truth.1,
            store: opened.with_store.then_some(&mut truth.2),
        };
        mirrors.write(StockAnswer::Resync {
            menu: opened.menu,
            snapshot: stock.snapshot(&opened.state, &opened.saddle),
        });
    }
}

/// Registers the request and consumes it in the tick. Added by `SimPlugin`,
/// so a server and a harness have it too; with no request it touches
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct ReworkPlugin;

impl Plugin for ReworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReworkInbox>()
            .add_message::<ReworkAnswer>()
            .add_systems(
                SimTick,
                consume
                    .in_set(SimSet::Input)
                    .after(crate::equip::consume)
                    .before(fitting::sync_loadout),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::crafting::tests::{
        CORE, ask as craft, assemble_best, first_day, id, part, prop, put, shipped, truth,
        work_and_finish,
    };
    use crate::crafting::{CraftDone, CraftRequest, craft_xp};
    use crate::progression::InteractionKind;
    use bevy::ecs::message::Messages;
    use std::collections::BTreeMap;
    use strata_registry::item::Fitted;

    const BLANK: &str = "veyra:component.mount_blank";
    const COMPENSATOR: &str = "veyra:module.vigil_compensator";

    fn blank(registries: &Registries, temper: u16) -> Record {
        if temper == 500 {
            part(registries, BLANK, 1, None)
        } else {
            part(registries, BLANK, 1, Some((3, &[temper])))
        }
    }

    fn at_bench() -> Reach {
        Reach {
            workbench: true,
            ..Reach::default()
        }
    }

    fn rework_ask(app: &mut App, hand: usize) -> Result<Reworked, ReworkRefusal> {
        app.world_mut()
            .resource_mut::<ReworkInbox>()
            .0
            .push(ReworkRequest::Rework { hand });
        app.update();
        let mut answers: Vec<ReworkAnswer> = app
            .world_mut()
            .resource_mut::<Messages<ReworkAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        answers.pop().unwrap().result
    }

    fn give(app: &mut App, record: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        put(&mut pack.cells, cell, record, &registries.0.items);
    }

    /// A stores screen open on another menu is told the truth after a
    /// rework: the blank gone and the weapon in its hand rewritten.
    #[test]
    fn an_open_stores_screen_is_resynced_by_a_rework() {
        use crate::items::{StockInbox, StockRequest};
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        prop(&mut app, InteractionKind::Workbench);
        give(&mut app, &blank(&registries, 700));
        let menu = slotted_model::MenuId(7);
        app.world_mut()
            .resource_mut::<StockInbox>()
            .0
            .push(StockRequest::Open {
                menu,
                with_store: true,
            });
        app.update();
        app.world_mut()
            .resource_mut::<Messages<StockAnswer>>()
            .drain()
            .for_each(drop);
        rework_ask(&mut app, 0).unwrap();
        let said: Vec<StockAnswer> = app
            .world_mut()
            .resource_mut::<Messages<StockAnswer>>()
            .drain()
            .collect();
        let [
            StockAnswer::Resync {
                menu: told,
                snapshot,
            },
        ] = &said[..]
        else {
            panic!("{said:?}");
        };
        assert_eq!(*told, menu);
        let hands = truth(&mut app).1;
        assert_eq!(snapshot.inventories[items::StockLayout::HANDS], hands.0);
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

    /// The refusals come in the order a player would fix them.
    #[test]
    fn the_refusals_come_in_the_order_a_player_would_fix_them() {
        let registries = shipped();
        let (mut pack, hands, store) = crate::crafting::tests::bench(&registries, &[]);
        let access = WorkbenchAccess::AtWorkbench;
        let ask = |reach: &Reach, sheet: &CharacterSheet, pack: &Pack, hand: usize| {
            check_rework(
                &registries.0,
                access,
                reach,
                sheet,
                pack,
                &hands,
                Some(&store),
                hand,
            )
        };
        let sheet = crate::crafting::tests::smith();
        let nobody = CharacterSheet::default();
        assert_eq!(
            ask(&Reach::default(), &nobody, &pack, 0),
            Err(ReworkRefusal::NoBench)
        );
        assert_eq!(
            ask(&at_bench(), &nobody, &pack, 0),
            Err(ReworkRefusal::NotTaught {
                needs: "Weaponsmith: Frames 2".to_owned()
            })
        );
        assert_eq!(
            ask(&at_bench(), &sheet, &pack, 5),
            Err(ReworkRefusal::NoWeapon)
        );
        pack.cursor = Some(
            items::write(
                &part(&registries, COMPENSATOR, 1, None),
                &registries.0.items,
            )
            .unwrap(),
        );
        assert_eq!(ask(&at_bench(), &sheet, &pack, 0), Err(ReworkRefusal::Busy));
        pack.cursor = None;
        assert_eq!(
            ask(&at_bench(), &sheet, &pack, 0),
            Err(ReworkRefusal::NoBlank)
        );
        let items = &registries.0.items;
        put(&mut pack.cells, 0, &blank(&registries, 500), items);
        assert_eq!(
            ask(&at_bench(), &sheet, &pack, 0),
            Err(ReworkRefusal::NoGain {
                temper: 500,
                moddability: 500
            })
        );
        put(&mut pack.cells, 1, &blank(&registries, 700), items);
        let outlook = ask(&at_bench(), &sheet, &pack, 0).unwrap();
        assert_eq!(
            (
                outlook.from,
                outlook.temper,
                outlook.most_often,
                outlook.least,
                outlook.most
            ),
            (500, 700, 650, 550, 700)
        );
        assert_eq!(outlook.blank, blank(&registries, 700));
        // The anywhere rule stands in for the bench.
        let anywhere = check_rework(
            &registries.0,
            WorkbenchAccess::Anywhere,
            &Reach::default(),
            &sheet,
            &pack,
            &hands,
            None,
            0,
        );
        assert!(anywhere.is_ok());
    }

    /// A refused rework is no transaction: no blank, no roll, no change.
    #[test]
    fn a_refused_rework_spends_no_blank_and_no_roll() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        prop(&mut app, InteractionKind::Workbench);
        give(&mut app, &blank(&registries, 500));
        let before = truth(&mut app);
        assert_eq!(
            rework_ask(&mut app, 0),
            Err(ReworkRefusal::NoGain {
                temper: 500,
                moddability: 500
            })
        );
        assert_eq!(truth(&mut app), before);
    }

    /// A blank of temper 700 lifts a bought carbine past 600, so the muzzle
    /// takes the part it refused; what was fitted, its condition, batch and
    /// maker stand, and one blank leaves the stack.
    #[test]
    fn a_hard_blank_lifts_a_bought_carbine_and_the_muzzle_opens() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        prop(&mut app, InteractionKind::Workbench);
        // A roll that comes to Good: a bought carbine to 650.
        let odds = registries
            .0
            .schematics
            .rules()
            .odds(Hand::Steady, registries.0.schematics.rules().base_row());
        let counter = (0..200)
            .find(|c| crafting::roll(strata_core::Seed(42), *c, &odds) == Outcome::Good)
            .unwrap();
        app.world_mut().resource_mut::<CraftState>().rolls = counter;
        let grip = Fitted {
            item: id("veyra:module.vigil_angled_grip"),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("equip_time".to_owned(), 500),
                ("recoil_recovery".to_owned(), 500),
            ]),
            condition: Some(700),
        };
        let carbine = {
            let (_, hands, ..) = truth(&mut app);
            let held = items::read(hands.0.get(0).unwrap(), items).unwrap();
            let mut held = crate::fitting::with(items, &held, "grip", Some(&grip)).unwrap();
            held.condition = Some(640);
            held.batch = 4;
            held.by = Some("Ilse Marrow".to_owned());
            held
        };
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Hands, With<LocalPlayer>>();
            let mut hands = players.single_mut(world).unwrap();
            put(&mut hands.0, 0, &carbine, items);
        }
        let compensator = {
            let def = items.get(&id(COMPENSATOR)).unwrap();
            Fitted {
                item: def.id.clone(),
                batch: 0,
                by: None,
                lines: def.lines.iter().map(|l| (l.id.clone(), 500)).collect(),
                condition: None,
            }
        };
        assert!(matches!(
            crate::fitting::with(items, &carbine, "muzzle", Some(&compensator)),
            Err(crate::fitting::FitRefusal::Shut { opens_at: 600 })
        ));
        give(&mut app, &blank(&registries, 700));
        let blanks = |app: &mut App| {
            let (pack, ..) = truth(app);
            crate::crafting::tests::units(&pack.cells, &blank(&registries, 700), items)
        };
        assert_eq!(blanks(&mut app), 1);
        let done = rework_ask(&mut app, 0).unwrap();
        assert_eq!(
            done,
            Reworked {
                hand: 0,
                outcome: Outcome::Good,
                from: 500,
                to: 650,
                opened: vec!["muzzle".to_owned()],
            }
        );
        assert_eq!(blanks(&mut app), 0, "one blank left the stack");
        assert_eq!(truth(&mut app).4.rolls, counter + 1);
        let (_, hands, ..) = truth(&mut app);
        let reworked = items::read(hands.0.get(0).unwrap(), items).unwrap();
        let mut expected = carbine.clone();
        expected.lines.insert("moddability".to_owned(), 650);
        assert_eq!(reworked, expected, "only the moddability line moved");
        assert_eq!(reworked.condition, Some(640));
        assert!(
            crate::fitting::with(items, &reworked, "muzzle", Some(&compensator)).is_ok(),
            "the muzzle takes the part it refused"
        );
    }

    /// Crafting earns Fabrication XP once per finished session, a first
    /// batch more, and a beaten best more again; a rework that raised the
    /// line earns its own.
    #[test]
    fn crafting_and_reworking_earn_fabrication_xp() {
        let rates = shipped().0.progression.rates.craft.clone();
        assert_eq!(craft_xp(&rates, 1, true, 0), 40 + 150);
        assert_eq!(craft_xp(&rates, 1, false, 0), 40);
        assert_eq!(craft_xp(&rates, 3, false, 2), 160 + 100);
        assert_eq!(craft_xp(&rates, 2, true, 3), 80 + 150 + 150);

        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let xp = |app: &mut App| {
            let world = app.world_mut();
            let mut sheets = world.query_filtered::<&CharacterSheet, With<LocalPlayer>>();
            sheets.single(world).unwrap().xp(XpKind::Fabrication)
        };
        assert_eq!(xp(&mut app), 0);
        assert!(matches!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { .. })
        ));
        work_and_finish(&mut app);
        assert_eq!(xp(&mut app), craft_xp(&rates, 1, true, 0));
        let said: Vec<XpGained> = app
            .world_mut()
            .resource_mut::<Messages<XpGained>>()
            .drain()
            .collect();
        assert_eq!(
            said.iter()
                .map(|g| (g.kind, g.amount, g.kill))
                .collect::<Vec<_>>(),
            [(XpKind::Fabrication, craft_xp(&rates, 1, true, 0), false)]
        );
        let first = xp(&mut app);
        // A second batch from the same stock beats no best unless a line is
        // higher; either way the rate is the repeat's.
        let before = truth(&mut app).4.bests.clone();
        assert!(!before.is_empty(), "the first batch set its bests");
        assert!(matches!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { .. })
        ));
        let Ok(CraftDone::Finished { product }) = craft(&mut app, CraftRequest::Finish) else {
            panic!("a session is finished");
        };
        let beaten = product
            .lines
            .iter()
            .filter(|(line, figure)| {
                before
                    .get(&format!("{CORE}#{line}"))
                    .is_some_and(|best| *figure > best)
            })
            .count();
        let beaten = u32::try_from(beaten).unwrap();
        assert_eq!(xp(&mut app) - first, craft_xp(&rates, 1, false, beaten));

        // A rework that raised the line.
        let registries = app.world().resource::<Registries>().clone();
        give(&mut app, &blank(&registries, 700));
        let before = xp(&mut app);
        let done = rework_ask(&mut app, 0).unwrap();
        assert!(done.to > done.from);
        assert_eq!(xp(&mut app) - before, rates.rework);
    }

    /// The trade is the maker's: another pressing Finish on a session earns
    /// no XP and no toast, though the thing is made.
    #[test]
    fn a_finisher_who_is_not_the_maker_earns_nothing() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        assert!(matches!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { .. })
        ));
        let player = {
            let world = app.world_mut();
            let mut players = world.query_filtered::<Entity, With<LocalPlayer>>();
            players.single(world).unwrap()
        };
        app.world_mut()
            .entity_mut(player)
            .insert(bevy::ecs::name::Name::new("Somebody Else"));
        app.world_mut()
            .resource_mut::<Messages<XpGained>>()
            .drain()
            .for_each(drop);
        let Ok(CraftDone::Finished { .. }) = craft(&mut app, CraftRequest::Finish) else {
            panic!("a session is finished");
        };
        let world = app.world_mut();
        let mut sheets = world.query_filtered::<&CharacterSheet, With<LocalPlayer>>();
        assert_eq!(sheets.single(world).unwrap().xp(XpKind::Fabrication), 0);
        assert_eq!(world.resource::<Messages<XpGained>>().len(), 0);
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
