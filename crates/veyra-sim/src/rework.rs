//! The adapter half of the `rework` rules: its requests, systems and plugin.

pub use veyra_rules::rework::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_registry::progression::XpKind;

use crate::WorldSeed;

use crate::crafting::CraftState;

use crate::fitting::WorkbenchAccess;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, Reach, XpGained};

use crate::{AppRequestExt, Inbox, Registries, Request};

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
pub type ReworkInbox = Inbox<ReworkRequest>;

/// With nobody to rework for there is no weapon to rework.
impl Request for ReworkRequest {
    type Answer = ReworkAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(ReworkAnswer {
            request: self.clone(),
            result: Err(ReworkRefusal::NoWeapon),
        });
    }
}

/// The sim's answer to a [`ReworkRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ReworkAnswer {
    /// The request it answers.
    pub request: ReworkRequest,
    /// What happened, or why nothing did.
    pub result: Result<Reworked, ReworkRefusal>,
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
    mut players: Query<(Entity, &mut Pack, &mut Hands, Option<&mut CharacterSheet>)>,
    open: Res<OpenStock>,
    mut answers: MessageWriter<ReworkAnswer>,
    mut gained: MessageWriter<XpGained>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let registries = &registries.0;
    let seed = seed.map_or(strata_core::Seed(0), |seed| seed.0);
    let mut changed = false;
    let mut last = None;
    for (actor, request) in requests {
        let ReworkRequest::Rework { hand } = request;
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
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
        && let Some(actor) = last
        && let Ok((_, pack, hands, _)) = players.get(actor)
    {
        // A stores screen left open under the fitting one must hear that a
        // blank went and the hand changed; the copies keep change
        // detection out of a read.
        let mut truth = ((*pack).clone(), (*hands).clone(), store.clone());
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
        app.add_request::<ReworkRequest, _>(RequestSet::Rework, consume);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::LocalPlayer;
    use crate::crafting::CraftDone;
    use crate::crafting::CraftRequest;
    use crate::crafting::craft_xp;
    use crate::crafting::tests::CORE;
    use crate::crafting::tests::ask as craft;
    use crate::crafting::tests::assemble_best;
    use crate::crafting::tests::first_day;
    use crate::crafting::tests::id;
    use crate::crafting::tests::part;
    use crate::crafting::tests::prop;
    use crate::crafting::tests::put;
    use crate::crafting::tests::shipped;
    use crate::crafting::tests::truth;
    use crate::crafting::tests::work_and_finish;
    use crate::progression::InteractionKind;
    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use std::collections::BTreeMap;
    use strata_registry::item::Fitted;

    use strata_registry::item::Record;
    use strata_registry::progression::XpKind;

    use strata_registry::schematic::Hand;
    use strata_registry::schematic::Outcome;

    use crate::crafting;
    use crate::crafting::CraftState;

    use crate::fitting::WorkbenchAccess;
    use crate::items;
    use crate::items::Hands;

    use crate::items::Pack;

    use crate::items::StockAnswer;

    use crate::progression::CharacterSheet;
    use crate::progression::Reach;
    use crate::progression::XpGained;

    use crate::Registries;

    pub(crate) use veyra_rules::rework::test_support::*;

    fn rework_ask(app: &mut App, hand: usize) -> Result<Reworked, ReworkRefusal> {
        app.world_mut()
            .resource_mut::<ReworkInbox>()
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
}
