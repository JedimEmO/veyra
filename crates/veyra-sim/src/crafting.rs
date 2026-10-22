//! The adapter half of the `crafting` rules: its requests, systems and plugin.

pub use veyra_rules::crafting::*;

use bevy::app::{App, Plugin};

use bevy::ecs::name::Name;

use bevy::ecs::prelude::*;

use strata_core::Seed;

use strata_registry::item::Record;

use strata_registry::progression::XpKind;

use strata_registry::schematic::Hand;

use strata_registry::{GameRegistries, Namespaced};

use crate::blueprint::{self, Blueprints};

use crate::fitting::{self, WorkbenchAccess};

use crate::household::Purse;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

pub use crate::items::{kind_of, reachable};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, Reach, XpGained};

use crate::{AppRequestExt, Inbox, Registries, Request, WorldSeed};

/// What a screen asks of the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraftRequest {
    /// Assemble a schematic from what is picked for its slots.
    Assemble {
        /// The schematic.
        schematic: Namespaced,
        /// The kind picked for each slot.
        picks: Picks,
    },
    /// Spend a point on a line.
    Roll {
        /// The product's line, by its id.
        line: String,
        /// The hand it is spent with.
        hand: Hand,
    },
    /// Finish what is on the bench.
    Finish,
    /// Buy a part at store grade.
    Buy {
        /// The item.
        item: Namespaced,
        /// How many.
        count: u32,
    },
    /// Record a finished thing in reach as a production blueprint. The
    /// prototype is not consumed.
    Record {
        /// The prototype, by its batch number.
        batch: u32,
        /// How far under the prototype the blueprint is recorded,
        /// per-mille: one of the dial's steps.
        derate: u16,
    },
}

/// [`blueprint::record`], answered as a request is: with the blueprint's
/// number, its item and the runs it starts with.
fn record(
    registries: &GameRegistries,
    stock: &mut Stock<'_>,
    state: &mut CraftState,
    prints: &mut Blueprints,
    (batch, derate): (u32, u16),
    by: (&str, &CharacterSheet),
) -> Result<CraftDone, CraftRefusal> {
    let (blueprint, item) =
        blueprint::record_for(registries, stock, state, prints, (batch, derate), by)?;
    let runs = prints.0.get(&blueprint).map_or(0, |print| print.runs);
    Ok(CraftDone::Recorded {
        blueprint,
        runs,
        item,
    })
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message, for the reason the stock's inbox is one: a screen
/// writes it in `Update`, the tick may not run that frame or may run
/// twice, and a request is consumed exactly once.
pub type CraftInbox = Inbox<CraftRequest>;

/// With nobody at the bench, a request is refused as such.
impl Request for CraftRequest {
    type Answer = CraftAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(CraftAnswer {
            request: self.clone(),
            outcome: Err(CraftRefusal::NoPlayer),
        });
    }
}

/// What a request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraftDone {
    /// The materials are spent and the session is open.
    Assembled {
        /// The points it has.
        points: u8,
    },
    /// A point was spent.
    Rolled(Rolled),
    /// The product is made and put away.
    Finished {
        /// What was made.
        product: Record,
    },
    /// A part was bought and put away.
    Bought {
        /// The item.
        item: Namespaced,
        /// What was paid.
        cost: u32,
        /// What the purse holds now.
        purse: u32,
    },
    /// A blueprint was recorded and its item put away.
    Recorded {
        /// The blueprint's number.
        blueprint: u32,
        /// The runs it starts with.
        runs: u32,
        /// The blueprint's item, as the stock now holds it.
        item: Record,
    },
}

/// The sim's answer to a [`CraftRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct CraftAnswer {
    /// The request it answers.
    pub request: CraftRequest,
    /// What happened, or why nothing did.
    pub outcome: Result<CraftDone, CraftRefusal>,
}

/// `SimTick`, `SimSet::Input`, after the survey's requests: every crafting
/// request made since the last tick, in order, each on what the one before
/// left.
///
/// The workbench rule is asked at every request and not when the screen
/// opened, since the world runs under an open screen: all five are held to
/// it, a roll, a purchase and a recording as much as an assembly. With a workbench in
/// reach the household's store is to hand, so stock comes from it and what
/// is made may go to it. The maker is the local player by name. The stores
/// screen, if it is open, is answered with the truth, since its mirror was
/// not told of a change it did not click.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the requests, the rule, the seed, the stock, the purse, the session, the sheet and the blueprints in, two answers out, one arm a request"
)]
pub fn consume(
    mut inbox: ResMut<CraftInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    seed: Option<Res<WorldSeed>>,
    open: Res<OpenStock>,
    mut state: ResMut<CraftState>,
    mut prints: ResMut<Blueprints>,
    mut store: ResMut<HouseholdStore>,
    mut purse: ResMut<Purse>,
    mut players: Query<(
        Entity,
        &mut Pack,
        &mut Hands,
        Option<&Name>,
        Option<&mut CharacterSheet>,
    )>,
    mut answers: MessageWriter<CraftAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut gained: MessageWriter<XpGained>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let registries = &registries.0;
    let seed = seed.map_or(Seed(0), |seed| seed.0);
    let at_workbench = fitting::at_workbench(*access, &reach);
    let mut moved = false;
    let mut last = None;
    for (actor, request) in requests {
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
        let outcome = match player.as_mut() {
            None => Err(CraftRefusal::NoPlayer),
            Some(_) if !at_workbench => Err(CraftRefusal::NoWorkbench),
            Some((entity, pack, hands, name, sheet)) => {
                // Worked on copies and written back only where something
                // moved, so a refusal leaves no change for a reader to
                // chase.
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let (mut session, mut paid) = (state.clone(), *purse);
                // Copied only for the one request that writes them.
                let mut recorded = None;
                let mut earned = 0;
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = match &request {
                    CraftRequest::Assemble { schematic, picks } => {
                        let by = name.map_or(UNNAMED, |name| name.as_str());
                        let sheet = sheet.as_deref().cloned().unwrap_or_default();
                        assemble(
                            registries,
                            &mut stock,
                            &mut session,
                            schematic,
                            picks,
                            by,
                            &sheet,
                        )
                        .map(|points| CraftDone::Assembled { points })
                    }
                    CraftRequest::Roll { line, hand } => {
                        roll_line(registries, seed, &mut session, line, *hand)
                            .map(CraftDone::Rolled)
                    }
                    CraftRequest::Finish => {
                        let open = session
                            .session
                            .as_ref()
                            .map(|open| (open.schematic.clone(), open.figures()));
                        // The trade is the maker's: whoever presses Finish
                        // for another's session notes the bests but earns
                        // nothing.
                        let maker = session.session.as_ref().is_some_and(|open| {
                            open.by == name.map_or(UNNAMED, |name| name.as_str())
                        });
                        let first = open
                            .as_ref()
                            .is_some_and(|(id, _)| !session.has_made(registries, id));
                        let done = finish(registries, &mut stock, &mut session);
                        if done.is_ok()
                            && let Some((id, figures)) = open
                        {
                            let beaten = session.note_bests(&id, &figures);
                            let complexity = registries
                                .schematics
                                .get(&id)
                                .map_or(1, |found| found.def.complexity);
                            if maker {
                                earned = craft_xp(
                                    &registries.progression.rates.craft,
                                    complexity,
                                    first,
                                    beaten,
                                );
                            }
                        }
                        done.map(|product| CraftDone::Finished { product })
                    }
                    CraftRequest::Buy { item, count } => {
                        buy(registries, &mut stock, &mut paid, item, *count).map(|cost| {
                            CraftDone::Bought {
                                item: item.clone(),
                                cost,
                                purse: paid.0,
                            }
                        })
                    }
                    CraftRequest::Record { batch, derate } => {
                        let by = name.map_or(UNNAMED, |name| name.as_str());
                        let sheet = sheet.as_deref().cloned().unwrap_or_default();
                        let ledger = recorded.insert(prints.clone());
                        record(
                            registries,
                            &mut stock,
                            &mut session,
                            ledger,
                            (*batch, *derate),
                            (by, &sheet),
                        )
                    }
                };
                if done.is_ok() {
                    if let Some(ledger) = recorded
                        && *prints != ledger
                    {
                        *prints = ledger;
                    }
                    if **pack != next.0 {
                        **pack = next.0;
                        moved = true;
                    }
                    if *store != next.2 {
                        *store = next.2;
                        moved = true;
                    }
                    if *state != session {
                        *state = session;
                    }
                    if *purse != paid {
                        *purse = paid;
                    }
                    if earned > 0
                        && let Some(sheet) = sheet
                    {
                        sheet.credit(XpKind::Fabrication, earned);
                        gained.write(XpGained {
                            entity: *entity,
                            kind: XpKind::Fabrication,
                            amount: earned,
                            kill: false,
                        });
                    }
                }
                done
            }
        };
        answers.write(CraftAnswer { request, outcome });
    }
    if moved
        && let Some(opened) = &open.0
        && let Some(actor) = last
        && let Ok((_, pack, hands, ..)) = players.get_mut(actor)
    {
        // The snapshot only reads; the copies keep the components' change
        // detection out of it.
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

/// Registers the crafting state and requests and consumes them in the
/// tick. Added by `SimPlugin`, so a server and a harness have it too; with
/// no request, which is all either ever has, it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct CraftingPlugin;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CraftState>()
            .init_resource::<Blueprints>()
            .add_request::<CraftRequest, _>(RequestSet::Crafting, consume);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::LocalPlayer;
    use crate::fitting::FitAnswer;
    use crate::fitting::FitInbox;
    use crate::fitting::FitRequest;
    use crate::fitting::Unit;
    use crate::progression::Interaction;
    use crate::progression::InteractionKind;
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;

    use std::collections::BTreeMap;

    use bevy::app::App;

    use bevy::ecs::name::Name;

    use strata_registry::item::Fitted;

    use strata_registry::item::ItemKind;

    use strata_registry::item::Property;
    use strata_registry::item::QUALITY_MAX;
    use strata_registry::item::Record;
    use strata_registry::item::STORE_GRADE;

    use strata_registry::schematic::Hand;

    use strata_registry::wear::DURABILITY;

    use crate::combat::Combatant;

    use crate::combat::Weapon;
    use crate::fitting;

    use crate::fitting::WorkbenchAccess;
    use crate::household::Purse;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;

    use crate::items::Pack;

    use crate::progression::CharacterSheet;
    use crate::progression::Reach;

    use crate::Registries;

    pub(crate) use veyra_rules::crafting::test_support::*;

    /// The first day in a running sim: the arrival by name, with the first
    /// day's pack and weapons, the store with the first lots in it, the
    /// first day's purse, and nothing within reach.
    pub(crate) fn first_day() -> App {
        let mut app = app_in_world();
        let registries = shipped();
        let (pack, hands, store) = bench(&registries, &first_lots(&registries));
        let purse = registries.0.items.starting().unwrap().purse;
        app.world_mut().spawn((
            LocalPlayer,
            Name::new(MAKER),
            crate::WorldPosition::new(0., 0., 0.),
            pack,
            hands,
            Combatant::default(),
            smith(),
        ));
        app.insert_resource(store)
            .insert_resource(Purse(purse))
            .insert_resource(registries);
        app.update();
        app
    }

    /// A prop the player stands at; a tick passes before it is in reach.
    pub(crate) fn prop(app: &mut App, kind: InteractionKind) -> Entity {
        let prop = app
            .world_mut()
            .spawn((
                crate::WorldPosition::new(0., 0., 0.),
                Interaction { kind, radius: 2.5 },
            ))
            .id();
        app.update();
        prop
    }

    pub(crate) fn ask(app: &mut App, request: CraftRequest) -> Result<CraftDone, CraftRefusal> {
        app.world_mut()
            .resource_mut::<CraftInbox>()
            .push(request.clone());
        app.update();
        let mut answers: Vec<CraftAnswer> = app
            .world_mut()
            .resource_mut::<Messages<CraftAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        let answer = answers.pop().unwrap();
        assert_eq!(answer.request, request);
        answer.outcome
    }

    /// The stock, the purse and the crafting state as they stand.
    pub(crate) fn truth(app: &mut App) -> (Pack, Hands, HouseholdStore, Purse, CraftState) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<(&Pack, &Hands), With<LocalPlayer>>();
        let (pack, hands) = players.single(world).unwrap();
        let (pack, hands) = (pack.clone(), hands.clone());
        (
            pack,
            hands,
            world.resource::<HouseholdStore>().clone(),
            *world.resource::<Purse>(),
            world.resource::<CraftState>().clone(),
        )
    }

    /// Assemble `name` from the best stock, through the tick.
    pub(crate) fn assemble_best(app: &mut App, name: &str) -> Result<CraftDone, CraftRefusal> {
        let registries = app.world().resource::<Registries>().clone();
        let (pack, _, store, ..) = truth(app);
        let picks = best_stock(
            &registries.0,
            &pack,
            Some(&store),
            schematic(&registries, name),
        );
        ask(
            app,
            CraftRequest::Assemble {
                schematic: id(name),
                picks,
            },
        )
    }

    /// Spend every point steady on the first line with room, then finish:
    /// the scripted walk's craft, and the fewest presses a screen takes.
    pub(crate) fn work_and_finish(app: &mut App) -> Record {
        loop {
            let state = app.world().resource::<CraftState>().clone();
            let session = state.session.unwrap();
            let Some(line) = session.lines.iter().find(|line| !line.at_ceiling()) else {
                break;
            };
            if session.points == 0 {
                break;
            }
            let request = CraftRequest::Roll {
                line: line.line.clone(),
                hand: Hand::Steady,
            };
            let Ok(CraftDone::Rolled(rolled)) = ask(app, request) else {
                panic!("a roll with points and room is made");
            };
            assert_eq!(rolled.points, session.points - 1);
        }
        match ask(app, CraftRequest::Finish) {
            Ok(CraftDone::Finished { product }) => product,
            other => panic!("{other:?}"),
        }
    }

    /// Somebody with no name at the bench (a harness actor) makes a thing
    /// whose maker is the empty name: no English is written into a record,
    /// the items still hold it as a crafted batch, and the ledger files it
    /// under the same name, so its card finds what it was made from.
    #[test]
    fn a_maker_with_no_name_is_the_empty_name() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let player = {
            let world = app.world_mut();
            let mut players = world.query_filtered::<Entity, With<LocalPlayer>>();
            players.single(world).unwrap()
        };
        app.world_mut().entity_mut(player).remove::<Name>();
        assert_eq!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let Ok(CraftDone::Finished { product }) = ask(&mut app, CraftRequest::Finish) else {
            panic!("a session with room is finished");
        };
        assert_eq!((product.batch, product.by.as_deref()), (1, Some("")));
        let registries = app.world().resource::<Registries>().clone();
        assert_eq!(registries.0.items.check_record(&product), Ok(()));
        let (pack, .., state) = truth(&mut app);
        assert!(
            pack.saved(&registries.0.items)
                .unwrap()
                .cells
                .contains(&Some(product.clone())),
            "and it reads back from the pack as it was made"
        );
        assert_eq!(state.made[&1].by, "");
        assert_eq!(state.check(&registries.0), Ok(()));
    }

    #[test]
    fn nothing_is_crafted_away_from_a_workbench() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let before = truth(&mut app);
        let picks = best_stock(
            &registries.0,
            &before.0,
            Some(&before.2),
            schematic(&registries, CORE),
        );
        assert_eq!(picks.len(), 2, "a reading works anywhere");
        let requests = || {
            [
                CraftRequest::Assemble {
                    schematic: id(CORE),
                    picks: picks.clone(),
                },
                CraftRequest::Roll {
                    line: "charge_density".to_owned(),
                    hand: Hand::Steady,
                },
                CraftRequest::Finish,
                CraftRequest::Buy {
                    item: id(HARNESS),
                    count: 1,
                },
            ]
        };
        // In the open, and at a locker, which is not a bench.
        for request in requests() {
            assert_eq!(ask(&mut app, request), Err(CraftRefusal::NoWorkbench));
        }
        let locker = prop(&mut app, InteractionKind::Locker);
        for request in requests() {
            assert_eq!(ask(&mut app, request), Err(CraftRefusal::NoWorkbench));
        }
        assert_eq!(truth(&mut app), before, "nothing moved");
        app.world_mut().despawn(locker);

        // At the bench all four are done, in order, each on what the one
        // before left.
        prop(&mut app, InteractionKind::Workbench);
        let [assemble, roll, finish, buy] = requests();
        assert_eq!(
            ask(&mut app, assemble.clone()),
            Ok(CraftDone::Assembled { points: 3 })
        );
        assert!(matches!(ask(&mut app, roll), Ok(CraftDone::Rolled(_))));
        let Ok(CraftDone::Finished { product }) = ask(&mut app, finish.clone()) else {
            panic!("the core is finished");
        };
        assert_eq!((product.batch, product.by.as_deref()), (1, Some(MAKER)));
        assert_eq!(
            ask(&mut app, buy),
            Ok(CraftDone::Bought {
                item: id(HARNESS),
                cost: 80,
                purse: 2420
            })
        );
        // The session, opened at the bench and walked away from, is still
        // there, and nothing is done to it from the open.
        assert_eq!(
            ask(&mut app, assemble),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let open = truth(&mut app);
        assert!(open.4.session.is_some());
        app.insert_resource(Reach::default());
        let mut benches = app
            .world_mut()
            .query_filtered::<Entity, With<Interaction>>();
        let bench: Vec<Entity> = benches.iter(app.world()).collect();
        for bench in bench {
            app.world_mut().despawn(bench);
        }
        app.update();
        assert_eq!(
            ask(&mut app, finish.clone()),
            Err(CraftRefusal::NoWorkbench)
        );
        assert_eq!(truth(&mut app), open);
        // A run that lifts the rule (a still of the screen) works anywhere.
        app.insert_resource(WorkbenchAccess::Anywhere);
        assert!(matches!(
            ask(&mut app, finish.clone()),
            Ok(CraftDone::Finished { .. })
        ));
        assert_eq!(ask(&mut app, finish.clone()), Err(CraftRefusal::NoSession));

        // With nobody to work it, nobody does.
        let mut empty = app_in_world();
        empty.insert_resource(shipped());
        assert_eq!(ask(&mut empty, finish), Err(CraftRefusal::NoPlayer));
    }

    /// Two cores from the same lots, by the same hand on the same line,
    /// differ: the counter moved between them. And a craft is a function of
    /// the counter: the same state gives the same core again.
    #[test]
    fn two_crafts_from_the_same_lots_differ() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let start = truth(&mut app);
        assert_eq!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let first = work_and_finish(&mut app);
        assert_eq!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let second = work_and_finish(&mut app);
        assert_eq!((first.batch, second.batch), (1, 2));
        assert_eq!(first.item, second.item);
        let made = app.world().resource::<CraftState>().made.clone();
        assert_eq!(
            made[&1].from, made[&2].from,
            "the same lots, the same amounts"
        );
        assert_ne!(first.lines, second.lines, "{:?} twice", first.lines);
        // Only what was worked differs: stability stood at its floor.
        assert_eq!(
            (first.lines["stability"], second.lines["stability"]),
            (209, 209)
        );
        for core in [&first, &second] {
            let charge = core.lines["charge_density"];
            assert!((358..=597).contains(&charge), "{charge}");
        }
        assert_eq!(app.world().resource::<CraftState>().rolls, 6);
        // In the world of seed 42 the first nine steady rolls are poor,
        // fair, good; good, fair, good; fair, fair, good: 0.78, 0.86 and
        // 0.82 of a ceiling of 597. A change to the roll's stream moves
        // these, and with them every household's next roll.
        assert_eq!(
            assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let third = work_and_finish(&mut app);
        assert_eq!(
            [&first, &second, &third].map(|core| core.lines["charge_density"]),
            [466, 513, 490]
        );
        // Each is a stack of its own in the pack.
        let after = truth(&mut app);
        let items = &app.world().resource::<Registries>().0.items.clone();
        assert_eq!(
            items::read(after.0.cells.get(0).unwrap(), items).unwrap(),
            first
        );
        assert_eq!(
            items::read(after.0.cells.get(1).unwrap(), items).unwrap(),
            second
        );

        // The day played again from the same counter is the same day.
        let mut again = first_day();
        prop(&mut again, InteractionKind::Workbench);
        assert_eq!(truth(&mut again), start);
        assemble_best(&mut again, CORE).unwrap();
        assert_eq!(work_and_finish(&mut again), first);
        // And from a counter moved on, another.
        let mut later = first_day();
        prop(&mut later, InteractionKind::Workbench);
        later.world_mut().resource_mut::<CraftState>().rolls = 3;
        assemble_best(&mut later, CORE).unwrap();
        let moved = work_and_finish(&mut later);
        assert_eq!(moved.lines, second.lines, "the second core's counters");
    }

    /// The chain, through the tick: a core from the first lots, the patrol
    /// magazine from it, fitted to the carbine, and the `Combatant` holds
    /// the rounds and the reload the bench showed before the fit.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the chain from the lots to the Combatant, in order"
    )]
    fn a_made_magazine_fits_and_holds_what_was_previewed() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let [salt, brine, alloy] = first_lots(&registries);
        assemble_best(&mut app, CORE).unwrap();
        let core = work_and_finish(&mut app);

        // The best stock for the magazine is the core just made.
        let (pack, hands, store, ..) = truth(&mut app);
        let feed = schematic(&registries, FEED);
        let picks = best_stock(&registries.0, &pack, Some(&store), feed);
        assert_eq!(picks["core"], core);
        let holder = Holder::of(items, &hands, Some(&Combatant::default()));
        assert_eq!(holder.drawn, 0);
        let promised = preview(&registries.0, &holder, feed, &picks);
        assert_eq!(
            assemble_best(&mut app, FEED),
            Ok(CraftDone::Assembled { points: 5 })
        );
        let on_bench = app
            .world()
            .resource::<CraftState>()
            .session
            .clone()
            .unwrap();
        for (line, promised) in on_bench.lines.iter().zip(&promised) {
            let promised = promised.as_ref().unwrap();
            assert_eq!(
                (line.ceiling, line.figure()),
                (promised.ceiling, promised.floor)
            );
        }
        let magazine = work_and_finish(&mut app);
        assert_eq!(magazine.item, id(FEED_ITEM));
        assert_eq!((magazine.batch, magazine.by.as_deref()), (2, Some(MAKER)));
        for (line, promised) in ["capacity", "reload_time"].into_iter().zip(&promised) {
            let promised = promised.as_ref().unwrap();
            assert!(
                (promised.floor..=promised.ceiling).contains(&magazine.lines[line]),
                "{line}: {} of {promised:?}",
                magazine.lines[line]
            );
        }
        // The stock is down by what the two crafts took, and no more.
        let (pack, _, store, purse, state) = truth(&mut app);
        assert_eq!(units(&store.0, &salt, items), 140);
        assert_eq!(units(&store.0, &brine, items), 130);
        assert_eq!(units(&store.0, &alloy, items), 160);
        assert_eq!(
            units(&store.0, &part(&registries, HARNESS, 1, None), items),
            2
        );
        assert_eq!(
            units(&store.0, &part(&registries, SEALS, 1, None), items),
            5
        );
        assert_eq!(units(&pack.cells, &core, items), 0, "the core went into it");
        assert_eq!(purse, Purse(2500), "nothing was bought");
        let from: Vec<&str> = state.made[&2]
            .from
            .iter()
            .map(|(slot, _)| slot.as_str())
            .collect();
        assert_eq!(from, ["core", "harness", "casing", "seals"]);
        assert_eq!(
            state.made[&2].from[0].1, core,
            "made from the core of batch 1"
        );
        assert_eq!((state.rolls, state.batches), (8, 2));

        // What the bench says the finished magazine does, before the fit.
        let finished = Session {
            schematic: id(FEED),
            by: MAKER.to_owned(),
            inputs: Vec::new(),
            lines: on_bench
                .lines
                .iter()
                .map(|line| SessionLine {
                    share: 1000,
                    ceiling: magazine.lines[&line.line],
                    ..line.clone()
                })
                .collect(),
            points: 0,
            workability: None,
        };
        let says = now(&registries.0, &holder, &finished);
        let rounds = says[0].means.unwrap().high;
        let reload = says[1].means.unwrap().high;
        assert_eq!((rounds.unit, reload.unit), (Unit::Rounds, Unit::Seconds));

        // Fitted by the fitting step's own request, nothing changed there.
        app.world_mut()
            .resource_mut::<FitInbox>()
            .push(FitRequest::Fit {
                hand: 0,
                mount: "feed".to_owned(),
                module: Fitted {
                    item: magazine.item.clone(),
                    batch: magazine.batch,
                    by: magazine.by.clone(),
                    lines: magazine.lines.clone(),
                    condition: None,
                },
            });
        app.update();
        let answers: Vec<FitAnswer> = app
            .world_mut()
            .resource_mut::<Messages<FitAnswer>>()
            .drain()
            .collect();
        assert!(answers[0].outcome.is_ok(), "{answers:?}");
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Combatant, With<LocalPlayer>>();
        let actor = players.single(world).unwrap();
        assert_eq!(i32::from(actor.magazine()), rounds.number);
        assert!(
            (28..=29).contains(&actor.magazine()),
            "{}",
            actor.magazine()
        );
        let seconds = f64::from(actor.reload_time());
        // The figure is the reload rounded to the hundredth it is read at.
        assert!((seconds * 100. - f64::from(reload.number)).abs() <= 0.5);
        assert_eq!(reload.decimals, 2);
    }

    /// The player's sheet, replaced.
    fn teach(app: &mut App, sheet: CharacterSheet) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
        *players.single_mut(world).unwrap() = sheet;
    }

    /// Without the box a schematic names it is refused, whole and with the
    /// box; and a session already open is finished by whoever stands there.
    #[test]
    fn a_schematic_with_a_profession_needs_its_box() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let registries = app.world().resource::<Registries>().clone();
        let carbine = schematic(&registries, CARBINE);
        let (pack, _, store, ..) = truth(&mut app);
        let taught = knowing(&[FRAMES_1]);
        assert!(!can_make(
            &registries.0,
            &pack,
            Some(&store),
            carbine,
            &CharacterSheet::default()
        ));
        assert!(can_make(
            &registries.0,
            &pack,
            Some(&store),
            carbine,
            &taught
        ));
        let picks = best_stock(&registries.0, &pack, Some(&store), carbine);
        let needs = CraftRefusal::NotTaught {
            needs: FRAMES_1.to_owned(),
        };
        assert_eq!(
            check_picks(
                &registries.0,
                &pack,
                Some(&store),
                carbine,
                &picks,
                &CharacterSheet::default()
            ),
            Err(needs.clone())
        );
        teach(&mut app, CharacterSheet::default());
        let before = truth(&mut app);
        assert_eq!(assemble_best(&mut app, CARBINE), Err(needs));
        assert_eq!(truth(&mut app), before, "nothing was taken");
        // A schematic with no profession is open to everyone.
        assert!(assemble_best(&mut app, CORE).is_ok());
        teach(&mut app, CharacterSheet::default());
        assert_eq!(work_and_finish(&mut app).item, id(CORE_ITEM));
        teach(&mut app, taught);
        assert_eq!(
            assemble_best(&mut app, CARBINE),
            Ok(CraftDone::Assembled { points: 8 })
        );
        // The gate is on opening it: another hand finishes the carbine.
        teach(&mut app, CharacterSheet::default());
        assert_eq!(work_and_finish(&mut app).item, id(CARBINE_ITEM));
    }

    /// Edges 1 teaches the service blade and the hooked edge: a smith with
    /// it makes the blade at the bench, makes the edge, and the edge fitted
    /// reads its posture on the `Combatant` by the figure its made line
    /// gives, while a hand without the box is refused both.
    #[test]
    fn a_smith_with_edges_one_makes_a_blade_and_a_hooked_edge_that_shows() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let (blade_schematic, edge_schematic) = (
            "veyra:schematic.service_blade",
            "veyra:schematic.blade_hooked_edge",
        );
        let needs = CraftRefusal::NotTaught {
            needs: "weaponsmith.edges.1".to_owned(),
        };
        teach(&mut app, CharacterSheet::default());
        assert_eq!(assemble_best(&mut app, blade_schematic), Err(needs.clone()));
        assert_eq!(assemble_best(&mut app, edge_schematic), Err(needs));
        teach(&mut app, knowing(&["weaponsmith.edges.1"]));
        assert!(assemble_best(&mut app, blade_schematic).is_ok());
        let blade = work_and_finish(&mut app);
        assert_eq!(blade.item, id("veyra:weapon.service_blade"));
        assert!(assemble_best(&mut app, edge_schematic).is_ok());
        let edge = work_and_finish(&mut app);
        assert_eq!(edge.item, id("veyra:module.blade_hooked_edge"));
        assert!(edge.lines.contains_key("hit_posture") && edge.lines.contains_key("recovery_time"));
        let fitted = fitting::with(
            items,
            &blade,
            "edge",
            Some(&Fitted {
                item: edge.item.clone(),
                batch: edge.batch,
                by: edge.by.clone(),
                lines: edge.lines.clone(),
                condition: None,
            }),
        )
        .expect("a made edge fits a made blade");
        let carry = |record: &Record| {
            let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
            actor.equip(&fitting::loadout(items, &[Some(record.clone()), None]));
            actor
        };
        let (plain, edged) = (carry(&blade), carry(&fitted));
        let effect = |property| {
            let ItemKind::Module { effects, .. } = &items.get(&edge.item).unwrap().kind else {
                panic!("a module");
            };
            effects
                .iter()
                .find(|e| e.property == property)
                .unwrap()
                .clone()
        };
        let rules = items.fitting();
        let by = rules.factor(&effect(Property::HitPosture), edge.lines["hit_posture"]);
        assert!(by > 1., "{by}: a made edge adds posture");
        let tail_by = rules.factor(&effect(Property::RecoveryTime), edge.lines["recovery_time"]);
        for action in [crate::combat::Action::Light, crate::combat::Action::Heavy] {
            let (was, now) = (plain.spec(action), edged.spec(action));
            assert!((now.posture - was.posture * by).abs() < 1e-4, "{action:?}");
            let tail = (was.duration - was.contact_end) * tail_by;
            assert!(
                (now.duration - was.contact_end - tail).abs() < 1e-5,
                "{action:?}"
            );
        }
    }

    /// Frames 3 teaches the marksman rifle: a smith without it is refused, a
    /// smith with it makes the rifle at the bench, and it is a rifle with
    /// the bore line and the open mounts a made one has.
    #[test]
    fn a_smith_with_frames_three_makes_a_rifle_at_the_bench() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let rifle_schematic = "veyra:schematic.vigil_marksman_rifle";
        teach(&mut app, CharacterSheet::default());
        assert_eq!(
            assemble_best(&mut app, rifle_schematic),
            Err(CraftRefusal::NotTaught {
                needs: "weaponsmith.frames.3".to_owned()
            })
        );
        teach(&mut app, knowing(&[FRAMES_1, "weaponsmith.frames.2"]));
        assert!(
            assemble_best(&mut app, rifle_schematic).is_err(),
            "Frames 3 or nothing"
        );
        teach(&mut app, knowing(&["weaponsmith.frames.3"]));
        assert!(assemble_best(&mut app, rifle_schematic).is_ok());
        let rifle = work_and_finish(&mut app);
        assert_eq!(rifle.item, id("veyra:weapon.marksman_rifle"));
        assert!(
            ["bore", "durability", "moddability"]
                .iter()
                .all(|line| rifle.lines.contains_key(*line))
        );
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        assert!(items.check_record(&rifle).is_ok());
        assert_eq!(items.family_of(&rifle), Some("Rifle"));
        // It is fitted and held like any weapon: its sight mount is open.
        assert!(
            items
                .mount_opens(&rifle, "sight")
                .is_some_and(|open| open.is_ok())
        );
    }

    /// Each tuning box is one more point in every session its holder opens.
    #[test]
    fn tuning_boxes_add_points_to_a_session() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let registries = app.world().resource::<Registries>().clone();
        let rules = registries.0.schematics.rules();
        let graph = &registries.0.progression;
        let tuned = knowing(&[FRAMES_1, "weaponsmith.tuning.1", "weaponsmith.tuning.2"]);
        let complexity = schematic(&registries, CARBINE).def.complexity;
        assert_eq!(complexity, 3);
        let dials = rules.points(complexity);
        assert_eq!(
            points(rules, graph, &knowing(&[FRAMES_1]), complexity),
            dials
        );
        assert_eq!(points(rules, graph, &tuned, complexity), dials + 2);
        teach(&mut app, tuned);
        assert_eq!(
            assemble_best(&mut app, CARBINE),
            Ok(CraftDone::Assembled { points: dials + 2 })
        );
    }

    /// A blade wears by the cut, a carbine by the shot, and the tab says
    /// which, with the cut's dearer cost.
    #[test]
    fn a_blades_durability_reads_in_cuts_and_a_carbines_in_shots() {
        let app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let means = |item: &str| {
            let def = items.get(&id(item)).unwrap();
            weapon_means(items, def, DURABILITY, (400, 800))
        };
        assert!(matches!(
            means("veyra:weapon.service_blade"),
            WeaponMeans::Cuts { low, high } if low > 0 && high >= low
        ));
        assert!(matches!(means(CARBINE_ITEM), WeaponMeans::Shots { .. }));
    }

    /// A frame cut from hard, strong stock, every point on moddability,
    /// opens more mounts than a bought carbine, and the muzzle takes a part
    /// the bought one's shut mount refuses.
    #[test]
    fn a_smiths_carbine_has_more_open_mounts_than_a_bought_one() {
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        // A hard ferrous lot, Hardness 800 and Strength 700: light alloy
        // runs no harder than 600 and no stronger than 700, so the platform
        // metal is ferrous ore.
        let frame = lot(
            &registries,
            "strata:ferrous_ore",
            "Anvil",
            200,
            &[600, 700, 600, 800, 500, 500, 700, 700],
        );
        {
            let mut store = app.world_mut().resource_mut::<HouseholdStore>();
            let cell = store.0.slots().iter().position(Option::is_none).unwrap();
            put(&mut store.0, cell, &frame, items);
        }
        let carbine = schematic(&registries, CARBINE);
        let (pack, _, store, ..) = truth(&mut app);
        let picks = best_stock(&registries.0, &pack, Some(&store), carbine);
        assert_eq!(
            picks["frame"],
            kind_of(&frame),
            "the harder lot is the best"
        );
        // 0.6 * 800 + 0.4 * 700.
        assert_eq!(ceiling(carbine, 2, &picks), Some(760));
        let shown = preview(&registries.0, &Holder::default(), carbine, &picks);
        let moddability = shown[2].as_ref().unwrap();
        assert_eq!(
            moddability.weapon,
            Some(WeaponMeans::Mounts {
                low: 4,
                high: 5,
                of: 6
            })
        );
        assert_eq!(
            shown[1]
                .as_ref()
                .unwrap()
                .weapon
                .map(|w| matches!(w, WeaponMeans::Shots { .. })),
            Some(true)
        );
        // The action line is the carbine's signature: seconds a shot from
        // the table's 0.42, 1.30 to 0.90 of it over the line's whole scale.
        let Some(WeaponMeans::Row {
            property,
            low,
            high,
            ..
        }) = shown[0].as_ref().unwrap().weapon
        else {
            panic!("the action line is a number of the row");
        };
        assert_eq!(property, Property::RepeatInterval);
        assert_eq!(
            (low.text(), high.text()),
            ("0.496".to_owned(), "0.462".to_owned())
        );

        assert!(assemble_best(&mut app, CARBINE).is_ok());
        while let Ok(CraftDone::Rolled(_)) = ask(
            &mut app,
            CraftRequest::Roll {
                line: "moddability".to_owned(),
                hand: Hand::Steady,
            },
        ) {}
        let Ok(CraftDone::Finished { product }) = ask(&mut app, CraftRequest::Finish) else {
            panic!("the carbine is finished");
        };
        let open = |weapon: &Record| {
            ["sight", "feed", "grip", "stock", "muzzle", "fore_end"]
                .into_iter()
                .filter(|mount| items.mount_opens(weapon, mount) == Some(Ok(())))
                .count()
        };
        let bought = part(&registries, CARBINE_ITEM, 1, None);
        assert_eq!(open(&bought), 4);
        assert!(
            (5..=6).contains(&open(&product)),
            "{}",
            product.lines["moddability"]
        );
        assert_eq!(product.condition, Some(QUALITY_MAX), "made whole");
        assert!(product.fitted.is_empty());
        assert_eq!((product.batch, product.by.as_deref()), (1, Some(MAKER)));
        let vented = Fitted {
            item: id("veyra:module.vigil_compensator"),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("recoil_recovery".to_owned(), STORE_GRADE),
                ("aim_move_speed".to_owned(), STORE_GRADE),
            ]),
            condition: None,
        };
        let fitted = fitting::with(items, &product, "muzzle", Some(&vented)).unwrap();
        assert!(fitted.fitted.contains_key("muzzle"));
        assert_eq!(
            fitting::with(items, &bought, "muzzle", Some(&vented)),
            Err(fitting::FitRefusal::Shut { opens_at: 600 })
        );
    }

    /// Each of step 5's eight schematics and step 6's harvester is made once at a workbench by a
    /// member who knows only its box, from stock given for it (what can be
    /// sampled and what the dealer sells), and each line of the product
    /// lands between the floor and the ceiling its preview promised.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "nine makes, each a stock, a make and a table of what came out"
    )]
    fn the_new_schematics_are_each_made_from_stock_given_for_it() {
        // Purity, Conductivity, HeatTolerance, Stability, Density,
        // Workability.
        let conductor = [800, 900, 600, 500, 600, 700];
        // Purity, Strength, Toughness, Hardness, HeatTolerance, Stability,
        // Density, Workability.
        let metal = [600, 700, 650, 800, 500, 550, 700, 600];
        let mut report = Vec::new();
        for (name, parts) in [
            ("veyra:schematic.drive_coil", Vec::new()),
            ("veyra:schematic.hull_plate", Vec::new()),
            (
                "veyra:schematic.survey_tool",
                vec![("veyra:component.drive_coil", 1, Some((9, vec![700])))],
            ),
            (
                "veyra:schematic.courier_bike",
                vec![
                    ("veyra:component.hull_plate", 2, Some((9, vec![640]))),
                    ("veyra:component.drive_coil", 2, Some((10, vec![700]))),
                    (CORE_ITEM, 1, Some((11, vec![600, 500]))),
                ],
            ),
            (
                "veyra:schematic.field_harvester",
                vec![
                    ("veyra:component.hull_plate", 2, Some((9, vec![640]))),
                    ("veyra:component.drive_coil", 1, Some((10, vec![700]))),
                    (CORE_ITEM, 1, Some((11, vec![600, 500]))),
                ],
            ),
            ("veyra:schematic.vigil_reflex", Vec::new()),
            ("veyra:schematic.vigil_angled_grip", Vec::new()),
            ("veyra:schematic.vigil_compensator", Vec::new()),
            ("veyra:schematic.service_pistol", Vec::new()),
        ] {
            let mut app = first_day();
            prop(&mut app, InteractionKind::Workbench);
            let registries = app.world().resource::<Registries>().clone();
            let items = &registries.0.items;
            let made = schematic(&registries, name);
            teach(
                &mut app,
                knowing(&[made.def.profession.as_deref().unwrap()]),
            );
            // The ground gives a conductor and a structural metal; a brine
            // the first day has; the dealer sells the seals, a harness, and
            // a coil and a plate at store grade.
            let mut given = vec![
                lot(&registries, "strata:red_conductor", "Vane", 100, &conductor),
                lot(&registries, "strata:ferrous_ore", "Anvil", 200, &metal),
            ];
            for (item, count, made) in &parts {
                given.push(part(
                    &registries,
                    item,
                    *count,
                    made.as_ref()
                        .map(|(batch, lines)| (*batch, lines.as_slice())),
                ));
            }
            {
                let mut store = app.world_mut().resource_mut::<HouseholdStore>();
                for record in &given {
                    let cell = store.0.slots().iter().position(Option::is_none).unwrap();
                    put(&mut store.0, cell, record, items);
                }
            }
            let (pack, _, store, ..) = truth(&mut app);
            let picks = best_stock(&registries.0, &pack, Some(&store), made);
            assert_eq!(picks.len(), made.def.slots.len(), "{name}: every slot");
            let promised = preview(&registries.0, &Holder::default(), made, &picks);
            let assembled = ask(
                &mut app,
                CraftRequest::Assemble {
                    schematic: made.def.id.clone(),
                    picks,
                },
            );
            assert!(assembled.is_ok(), "{name}: {assembled:?}");
            let product = work_and_finish(&mut app);
            assert_eq!(product.item, made.def.makes, "{name}");
            assert_eq!(items.check_record(&product), Ok(()), "{name}");
            let def = items.get(&product.item).unwrap();
            assert_eq!(product.lines.len(), def.lines.len(), "{name}");
            for (line, shown) in def.lines.iter().zip(&promised) {
                let shown = shown.as_ref().unwrap();
                let made = product.lines[&line.id];
                assert!(
                    (shown.floor..=shown.ceiling).contains(&made),
                    "{name} {}: {made} not in {}..={}",
                    line.id,
                    shown.floor,
                    shown.ceiling
                );
                report.push(format!(
                    "{} {} floor {} ceiling {} made {made}",
                    name.rsplit('.').next().unwrap(),
                    line.id,
                    shown.floor,
                    shown.ceiling
                ));
            }
            // A weapon and a bike leave the bench whole, the rest have none.
            assert_eq!(
                product.condition,
                matches!(def.kind, ItemKind::Weapon { .. } | ItemKind::Vehicle { .. })
                    .then_some(items.wear().full),
                "{name}"
            );
        }
        println!("{}", report.join("\n"));
    }
}
