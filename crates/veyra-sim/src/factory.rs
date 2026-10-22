//! The adapter half of the `factory` rules: its requests, systems and plugin.

pub use veyra_rules::factory::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_core::SIM_TICK_HZ;

use crate::blueprint::Blueprints;

use crate::crafting::CraftState;

use crate::fitting::WorkbenchAccess;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, Reach};

use crate::{AppRequestExt, Inbox, LocalPlayer, Registries, Request, SimClock};

/// What a screen asks of the fabricator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryRequest {
    /// Start a run.
    Start {
        /// The blueprint, by its number.
        blueprint: u32,
        /// How many to make.
        batch: u32,
        /// The kinds that feed each slot.
        feeds: Feeds,
        /// The benched member to work it, by name; nobody, and it runs at
        /// the unattended rate.
        worker: Option<String>,
    },
    /// Put somebody, or nobody, on the run under way.
    Assign {
        /// The benched member, by name.
        worker: Option<String>,
    },
    /// Stop the run under way.
    Stop,
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message, for the reason the stock's inbox is one: a screen
/// writes it in `Update`, the tick may not run that frame or may run
/// twice, and a request is consumed exactly once.
pub type FactoryInbox = Inbox<FactoryRequest>;

/// With nobody at the fabricator, a request is refused as such.
impl Request for FactoryRequest {
    type Answer = FactoryAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(FactoryAnswer {
            request: self.clone(),
            outcome: Err(FactoryRefusal::NoPlayer),
        });
    }
}

/// What a request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryDone {
    /// The inputs are committed and the run is on.
    Started {
        /// The batch number its units carry.
        batch: u32,
        /// The played seconds it will take at the rate it starts at.
        seconds: u32,
    },
    /// The worker was changed.
    Assigned,
    /// The run is stopped and what it held is back in the store.
    Stopped {
        /// How many it had made.
        made: u32,
    },
}

/// The sim's answer to a [`FactoryRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct FactoryAnswer {
    /// The request it answers.
    pub request: FactoryRequest,
    /// What happened, or why nothing did.
    pub outcome: Result<FactoryDone, FactoryRefusal>,
}

/// The truth for an open stores screen whose mirror was not told of a
/// change it did not click.
fn resync(
    open: &OpenStock,
    pack: &Pack,
    hands: &Hands,
    store: &HouseholdStore,
    mirrors: &mut MessageWriter<StockAnswer>,
) {
    let Some(opened) = &open.0 else {
        return;
    };
    // The snapshot only reads; the copies keep the components' change
    // detection out of it.
    let mut truth = (pack.clone(), hands.clone(), store.clone());
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

/// `SimTick`, `SimSet::Input`, after the bench's requests: every factory
/// request made since the last tick, in order, each on what the one before
/// left.
///
/// The fabricator rule is asked at every request and not when the screen
/// opened, since the world runs under an open screen. With a fabricator in
/// reach the household's store is to hand, as at a workbench, so a
/// blueprint and its inputs come from the pack, its bags or the store. The
/// stores screen, if it is open, is answered with the truth.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock, the bench and the three ledgers in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<FactoryInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    benched: Res<Benched>,
    prints: Res<Blueprints>,
    mut craft: ResMut<CraftState>,
    mut state: ResMut<FactoryState>,
    mut clock: ResMut<FactoryClock>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands, Option<&CharacterSheet>)>,
    mut answers: MessageWriter<FactoryAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let registries = &registries.0;
    let in_reach = at_fabricator(*access, &reach);
    let mut moved = false;
    let mut last = None;
    for (actor, request) in requests {
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
        let outcome = match player.as_mut() {
            None => Err(FactoryRefusal::NoPlayer),
            Some(_) if !in_reach => Err(FactoryRefusal::NoFabricator),
            Some((pack, hands, sheet)) => {
                // Worked on copies and written back only where something
                // moved, so a refusal leaves no change for a reader to
                // chase.
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let (mut ledger, mut run, mut ticked) = (craft.clone(), state.clone(), *clock);
                // The run is kept at the pace of whoever starts it.
                let starters = registries
                    .schematics
                    .factory()
                    .zip(*sheet)
                    .and_then(|(rules, sheet)| pace_of(rules, &registries.progression, sheet));
                let done = match &request {
                    FactoryRequest::Start {
                        blueprint,
                        batch,
                        feeds,
                        worker,
                    } => {
                        let mut stock = Stock {
                            saddle: None,
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: Some(&mut next.2),
                        };
                        start(
                            registries,
                            &mut stock,
                            &prints,
                            &mut ledger,
                            &mut run,
                            &benched,
                            (*blueprint, *batch),
                            feeds,
                            (worker.as_deref(), starters),
                        )
                        .map(|(batch, seconds)| {
                            ticked.0 = 0;
                            FactoryDone::Started { batch, seconds }
                        })
                    }
                    FactoryRequest::Assign { worker } => {
                        assign(&mut run, &benched, worker.as_deref())
                            .map(|()| FactoryDone::Assigned)
                    }
                    FactoryRequest::Stop => {
                        stop(registries, &prints, &mut run, &mut ticked, &mut next.2)
                            .map(|made| FactoryDone::Stopped { made })
                    }
                };
                if done.is_ok() {
                    if **pack != next.0 {
                        **pack = next.0;
                        moved = true;
                    }
                    if *store != next.2 {
                        *store = next.2;
                        moved = true;
                    }
                    if *craft != ledger {
                        *craft = ledger;
                    }
                    if *state != run {
                        *state = run;
                    }
                    if *clock != ticked {
                        *clock = ticked;
                    }
                }
                done
            }
        };
        answers.write(FactoryAnswer { request, outcome });
    }
    if moved
        && let Some(actor) = last
        && let Ok((pack, hands, _)) = players.get(actor)
    {
        resync(&open, pack, hands, &store, &mut mirrors);
    }
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: the run's tick
/// ([`advance`]), as many nominal ticks as this step of the sim is worth
/// ([`nominal_ticks`]). What the run changes is marked changed when it
/// changes, which is once a unit: the household saves on a change, and a
/// file a tick would be twenty a second.
#[allow(
    clippy::too_many_arguments,
    reason = "the three ledgers a tick may write, the two clocks, the store, and the two things it tells"
)]
pub fn run(
    registries: Res<Registries>,
    open: Res<OpenStock>,
    sim: Option<Res<SimClock>>,
    mut prints: ResMut<Blueprints>,
    mut state: ResMut<FactoryState>,
    mut clock: ResMut<FactoryClock>,
    mut store: ResMut<HouseholdStore>,
    players: Query<(&Pack, &Hands), With<LocalPlayer>>,
    mut notices: MessageWriter<FactoryNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut phase: Local<u32>,
) {
    let read = |state: &FactoryState| state.job.as_ref().map(|job| (job.made, job.paused));
    let Some(before) = read(&state) else {
        return;
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let mut told = Vec::new();
    for _ in 0..nominal_ticks(&mut phase, stepped) {
        told.extend(advance(
            &registries.0,
            prints.bypass_change_detection(),
            state.bypass_change_detection(),
            clock.bypass_change_detection(),
            store.bypass_change_detection(),
        ));
    }
    let after = read(&state);
    if after != Some(before) {
        state.set_changed();
    }
    // A unit made or the blueprint handed back: the store and the
    // blueprint's runs moved.
    if after.is_none_or(|(made, _)| made != before.0) {
        prints.set_changed();
        store.set_changed();
        if let Ok((pack, hands)) = players.single()
            && open.0.as_ref().is_some_and(|opened| opened.with_store)
        {
            resync(&open, pack, hands, &store, &mut mirrors);
        }
    }
    for notice in told {
        notices.write(notice);
    }
}

/// Registers the fabricator's state and requests, consumes them in the
/// tick and runs the job. Added by `SimPlugin`, so a server and a harness
/// have it too; with no request and no job, which is all either ever has,
/// it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct FactoryPlugin;

impl Plugin for FactoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FactoryState>()
            .init_resource::<FactoryClock>()
            .init_resource::<Benched>()
            .add_message::<FactoryNotice>()
            .add_request::<FactoryRequest, _>(RequestSet::Factory, (consume, run).chain());
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::blueprint;

    use crate::crafting::tests::CORE;
    use crate::crafting::tests::CORE_ITEM;

    use crate::crafting::tests::MAKER;

    use crate::crafting::tests::ask;

    use crate::crafting::tests::first_day;
    use crate::crafting::tests::id;

    use crate::crafting::tests::prop;
    use crate::crafting::tests::schematic;
    use crate::crafting::tests::shipped;

    use crate::crafting::CraftDone;
    use crate::crafting::CraftRequest;
    use crate::crafting::Picks;
    use crate::crafting::tests::truth;
    use crate::crafting::tests::units;
    use crate::crafting::tests::work_and_finish;

    use crate::progression::CharacterSheet;
    use crate::progression::Interaction;
    use crate::progression::InteractionKind;
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;

    use bevy::app::App;
    use std::collections::BTreeMap;

    use strata_registry::item::Record;

    use strata_registry::schematic::FactoryRules;

    use strata_registry::Namespaced;

    use crate::blueprint::Blueprints;

    use crate::crafting;
    use crate::crafting::CraftState;

    use crate::fitting::WorkbenchAccess;

    use crate::progression::Reach;

    use crate::LocalPlayer;
    use crate::Registries;

    pub(crate) use veyra_rules::factory::test_support::*;

    /// The step's bar: a hundred cores from one run are one stack of one
    /// batch, every one with the blueprint's lines exactly, the stock that
    /// fed them is spent to the unit, and the blueprint with its last run.
    #[test]
    fn a_hundred_identical_cores() {
        // 2,000 salt and 3,000 brine once the prototype has taken its own.
        let mut shop = shop(
            |registries| {
                let mut lots = first(registries);
                lots.extend((0..9).map(|_| tolley(registries, 200)));
                lots.push(tolley(registries, 60));
                lots.extend((0..14).map(|_| oxbow(registries, 200)));
                lots.push(oxbow(registries, 70));
                lots
            },
            1000,
        );
        let (salt, brine) = (tolley(&shop.registries, 1), oxbow(&shop.registries, 1));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (2000, 3000));
        assert_eq!(
            shop.limit(),
            Limit {
                most: 100,
                held_by: Held::Runs { left: 100 }
            }
        );
        let feeds = shop.cheapest(100);
        assert_eq!(feeds, feed(&[&salt], &[&brine]));
        assert_eq!(shop.check(100, &feeds), Ok(()));
        // Forty minutes with nobody on it.
        assert_eq!(shop.start(100, &feeds, None), Ok((5, 2400)));
        assert_eq!((shop.held(&salt), shop.held(&brine)), (0, 0));
        assert!(shop.cores().is_empty(), "no time has passed");

        // The first unit, as the blueprint says it is.
        assert_eq!(shop.ticks(480), []);
        let expected = Record {
            item: id(CORE_ITEM),
            count: 1,
            batch: 5,
            by: Some(MAKER.to_owned()),
            lines: BTreeMap::from([
                ("charge_density".to_owned(), 513),
                ("stability".to_owned(), 209),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        assert_eq!(shop.cores(), std::slice::from_ref(&expected));
        assert_eq!(expected, shop.print().unit(5, &shop.registries.0.items));
        assert_eq!((shop.job().made, shop.print().runs_left), (1, 99));

        // The other ninety-nine: told once, at the end.
        let told = shop.ticks(99 * 480);
        assert_eq!(
            told,
            [(
                99 * 480,
                FactoryNotice::Finished {
                    item: id(CORE_ITEM),
                    made: 100,
                    batch: 5,
                    spent: true,
                }
            )]
        );
        // One stack of a hundred: one item, one batch, one maker, the
        // blueprint's lines, since stacks merge only when all of it is
        // equal.
        assert_eq!(
            shop.cores(),
            [Record {
                count: 100,
                ..expected
            }]
        );
        assert_eq!(shop.cores()[0].lines, shop.print().lines);
        assert_eq!(shop.state.job, None);
        // The blueprint's runs fell by a hundred, and its item went with
        // the last.
        assert_eq!((shop.print().runs, shop.print().runs_left), (100, 0));
        let held = blueprint::held(
            &shop.registries.0,
            &shop.pack,
            Some(&shop.store),
            &shop.prints,
        );
        assert!(held.is_empty(), "{held:?}");
        assert_eq!((shop.held(&salt), shop.held(&brine)), (0, 0));
        // The ledger says what a unit was made from and what it was run
        // off; the prototype is in the pack as it was.
        let filed = &shop.craft.made[&5];
        assert_eq!(
            (filed.item.clone(), filed.by.as_str(), filed.blueprint),
            (id(CORE_ITEM), MAKER, Some(4))
        );
        assert_eq!(filed.schematic, Some(id(CORE)));
        let from: Vec<(&str, u32, &Namespaced)> = filed
            .from
            .iter()
            .map(|(slot, record)| (slot.as_str(), record.count, &record.item))
            .collect();
        assert_eq!(from, [("salt", 20, &salt.item), ("brine", 30, &brine.item)]);
        assert_eq!(shop.craft.batches, 5);
        assert_eq!(shop.craft.check(&shop.registries.0), Ok(()));
        let prototype = Record {
            batch: 3,
            ..shop.print().unit(3, &shop.registries.0.items)
        };
        assert_eq!(
            units(&shop.pack.cells, &prototype, &shop.registries.0.items),
            1
        );
        // A spent blueprint starts nothing more.
        shop.state = FactoryState::default();
        assert_eq!(shop.limit().most, 0);
        assert_eq!(shop.tick(), None);
    }

    /// Stock better than the prototype's is accepted and gains nothing:
    /// the units have the blueprint's lines, not what a bench would have
    /// made of it.
    #[test]
    fn better_stock_gains_nothing() {
        let mut shop = shop(
            |registries| {
                let mut lots = first(registries);
                lots.extend([calder(registries, 100), deepwell(registries, 100)]);
                lots
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (salt, brine) = (calder(&registries, 1), deepwell(&registries, 1));
        // At a bench the same stock has a ceiling of 760.
        let picks = Picks::from([
            ("salt".to_owned(), salt.clone()),
            ("brine".to_owned(), brine.clone()),
        ]);
        assert_eq!(
            crafting::ceiling(schematic(&registries, CORE), 0, &picks),
            Some(760)
        );
        let fed = feed(&[&salt], &[&brine]);
        assert_eq!(shop.start(2, &fed, None), Ok((5, 48)));
        let told = shop.ticks(960);
        assert!(matches!(
            told[..],
            [(
                960,
                FactoryNotice::Finished {
                    made: 2,
                    spent: false,
                    ..
                }
            )]
        ));
        let cores = shop.cores();
        assert_eq!(cores.len(), 1);
        assert_eq!((cores[0].count, cores[0].batch), (2, 5));
        assert_eq!(
            (
                cores[0].lines["charge_density"],
                cores[0].lines["stability"]
            ),
            (513, 209)
        );
        assert_eq!((shop.held(&salt), shop.held(&brine)), (60, 40));
        // The ledger says they were made of the better stock all the same.
        assert_eq!(shop.craft.made[&5].from[0].1.lines["potency"], 820);
    }

    /// A slot is fed from several stacks when each meets the minimum: they
    /// are spent in the order given, a unit may span two, and one too
    /// small on its own is short with what it has.
    #[test]
    fn a_slot_is_fed_from_several_stacks() {
        let mut shop = shop(
            |registries| {
                vec![
                    tolley(registries, 50),
                    oxbow(registries, 160),
                    calder(registries, 100),
                ]
            },
            1000,
        );
        let registries = shop.registries.clone();
        let (own, better, brine) = (
            tolley(&registries, 1),
            calder(&registries, 1),
            oxbow(&registries, 1),
        );
        assert_eq!(shop.held(&own), 30);
        assert_eq!(
            shop.check(3, &feed(&[&own], &[&brine])),
            Err(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Short {
                    have: 30,
                    needs: 60
                }
            })
        );
        // Named twice, a kind is still counted once.
        assert!(shop.check(3, &feed(&[&own, &own], &[&brine])).is_err());
        let fed = feed(&[&own, &better], &[&brine]);
        assert_eq!(shop.cheapest(3), fed, "which is also the cheapest");
        assert_eq!(shop.start(3, &fed, None), Ok((5, 72)));
        let hopper = |shop: &Shop| -> Vec<(String, String, u32)> {
            shop.job()
                .inputs
                .iter()
                .map(|input| {
                    let root = input.kind.origin.as_ref().unwrap().root.clone();
                    (input.slot.clone(), root, input.left)
                })
                .collect()
        };
        let row = |slot: &str, root: &str, left: u32| (slot.to_owned(), root.to_owned(), left);
        assert_eq!(
            hopper(&shop),
            [
                row("salt", "Tolley", 30),
                row("salt", "Calder", 30),
                row("brine", "Oxbow", 90)
            ]
        );
        assert_eq!(
            (shop.held(&own), shop.held(&better), shop.held(&brine)),
            (0, 70, 40)
        );
        shop.ticks(480);
        assert_eq!(
            hopper(&shop),
            [
                row("salt", "Tolley", 10),
                row("salt", "Calder", 30),
                row("brine", "Oxbow", 60)
            ]
        );
        // The second unit takes the last ten of one lot and ten of the
        // next.
        shop.ticks(480);
        assert_eq!(
            hopper(&shop),
            [row("salt", "Calder", 20), row("brine", "Oxbow", 30)]
        );
        let told = shop.ticks(480);
        assert!(matches!(
            told[..],
            [(480, FactoryNotice::Finished { made: 3, .. })]
        ));
        assert_eq!(shop.cores()[0].count, 3, "and all three are one stack");
        // The ledger names both salts.
        let from: Vec<(&str, &str, u32)> = shop.craft.made[&5]
            .from
            .iter()
            .map(|(slot, record)| {
                let root = record.origin.as_ref().unwrap().root.as_str();
                (slot.as_str(), root, record.count)
            })
            .collect();
        assert_eq!(
            from,
            [
                ("salt", "Tolley", 20),
                ("salt", "Calder", 20),
                ("brine", "Oxbow", 30)
            ]
        );
        assert_eq!(shop.craft.check(&registries.0), Ok(()));
    }

    /// The dial: a unit of complexity 1 is twelve played seconds with a
    /// worker and twenty-four with nobody, so three cores take 36 s and
    /// 72 s, twenty four minutes and eight, and the ticks a run takes are
    /// exactly those.
    #[test]
    fn three_take_36_and_72_seconds_and_twenty_four_minutes() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        assert_eq!(
            (seconds(&rules, 1, 3, true), seconds(&rules, 1, 3, false)),
            (36, 72)
        );
        assert_eq!(
            (seconds(&rules, 1, 20, true), seconds(&rules, 1, 20, false)),
            (240, 480)
        );
        assert_eq!(
            (
                seconds(&rules, 1, 100, true),
                seconds(&rules, 1, 100, false)
            ),
            (1200, 2400)
        );
        assert_eq!(
            (seconds(&rules, 2, 3, true), seconds(&rules, 2, 3, false)),
            (72, 144),
            "a magazine is twice a core"
        );
        assert_eq!(seconds(&rules, 1, 0, false), 0);

        let fed = shop.cheapest(3);
        assert_eq!(shop.start(3, &fed, None), Ok((5, 72)));
        let left = |shop: &Shop| left(&rules, shop.print(), shop.job(), &shop.clock);
        assert_eq!(left(&shop), 72);
        // Half a unit in: twelve seconds of it done.
        assert_eq!(shop.ticks(240), []);
        assert_eq!((shop.job().made, left(&shop)), (0, 60));
        assert_eq!(shop.ticks(239), []);
        assert_eq!(
            (shop.job().made, left(&shop)),
            (0, 49),
            "a tick short, rounded up"
        );
        assert_eq!(shop.tick(), None);
        assert_eq!((shop.job().made, left(&shop)), (1, 48));
        let told = shop.ticks(960);
        assert!(
            matches!(told[..], [(960, FactoryNotice::Finished { made: 3, .. })]),
            "1,440 ticks at twenty a second: {told:?}"
        );

        // With somebody on it, half that.
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, Some(WORKER)), Ok((6, 12)));
        assert_eq!(left(&shop), 12);
        let told = shop.ticks(240);
        assert!(matches!(
            told[..],
            [(
                240,
                FactoryNotice::Finished {
                    made: 1,
                    batch: 6,
                    ..
                }
            )]
        ));
    }

    /// A share of the worked rate that does not divide a unit (0.7 of it:
    /// 342.86 ticks a core) loses nothing between units: what the clock
    /// has over a unit goes toward the next, so seven cores take the 2,400
    /// ticks [`seconds`] and [`left`] say, where a clock begun again at
    /// each unit would take 343 a core and 2,401.
    #[test]
    fn a_share_that_does_not_divide_a_unit_loses_nothing() {
        let mut shop = shop(
            |registries| {
                vec![
                    tolley(registries, 200),
                    oxbow(registries, 200),
                    oxbow(registries, 60),
                ]
            },
            1000,
        );
        let assets = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/crafting/factory.ron"
        );
        let text = std::fs::read_to_string(assets).unwrap();
        assert!(text.contains("unattended: 0.5"), "the dial this test turns");
        let dials = FactoryRules::parse(&text.replace("unattended: 0.5", "unattended: 0.7"));
        let mut turned = (*shop.registries.0).clone();
        turned.schematics = turned
            .schematics
            .with_factory(dials.unwrap(), &turned.items)
            .unwrap();
        shop.registries = Registries(std::sync::Arc::new(turned));
        let rules = shop.rules().clone();
        assert_eq!(rules.rate_pm(false), 700);
        // 240,000 a unit at 700 a tick: 342.86 ticks, 17.14 s.
        assert_eq!(seconds(&rules, 1, 1, false), 18);
        assert_eq!(seconds(&rules, 1, 7, false), 120);
        assert_eq!(seconds(&rules, 1, 100, false), 1715);

        let fed = shop.cheapest(7);
        assert_eq!(shop.start(7, &fed, None), Ok((5, 120)));
        assert_eq!(shop.ticks(342), []);
        shop.tick();
        assert_eq!((shop.job().made, shop.clock), (1, FactoryClock(100)));
        assert_eq!(
            left(&rules, shop.print(), shop.job(), &shop.clock),
            103,
            "2,057 ticks: what was over is counted"
        );
        // The second starts a hundred up: 2 x 240,000 is passed at tick
        // 686, and by the seventh what is over has saved a whole tick.
        assert_eq!(shop.ticks(342), []);
        shop.tick();
        assert_eq!((shop.job().made, shop.clock), (2, FactoryClock(200)));
        let told = shop.ticks(2400 - 686);
        assert!(
            matches!(told[..], [(1714, FactoryNotice::Finished { made: 7, .. })]),
            "2,400 ticks in all: {told:?}"
        );
        assert_eq!(shop.clock, FactoryClock(0));
    }

    /// A pause is saved and the clock is not. A run read back standing,
    /// with the clock a start has, still has the unit that was waiting: it
    /// says nothing again while the store is full, makes the unit on the
    /// first tick with room, says it goes on, and takes a whole unit's
    /// time for the next. The blueprint waiting to come back at the end is
    /// handed over the same way.
    #[test]
    fn a_paused_run_read_back_from_a_save_goes_on_when_there_is_room() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        let fed = shop.cheapest(2);
        assert_eq!(shop.start(2, &fed, None), Ok((5, 48)));
        let free = shop.crowd();
        shop.ticks(480);
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (0, Some(Pause::NoRoom))
        );
        let reload = |shop: &mut Shop| {
            let saved = ron::to_string(&shop.state).unwrap();
            shop.state = ron::from_str(&saved).unwrap();
            shop.clock = FactoryClock::default();
        };
        reload(&mut shop);
        assert_eq!(shop.job().paused, Some(Pause::NoRoom));
        assert_eq!(
            left(&rules, shop.print(), shop.job(), &shop.clock),
            24,
            "the unit that waits is not counted again"
        );
        assert_eq!(shop.ticks(100), [], "still full: it stands, told once");
        assert_eq!(shop.job().made, 0);

        shop.free(free[0]);
        assert_eq!(shop.tick(), Some(FactoryNotice::Resumed));
        assert_eq!((shop.job().made, shop.job().paused), (1, None));
        assert_eq!(shop.cores()[0].count, 1);
        assert_eq!(shop.print().runs_left, 99);
        assert_eq!(shop.clock, FactoryClock(0));
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 24);

        // The second merges onto the first and the blueprint has no cell:
        // standing again, read back again, and handed over when there is
        // one.
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.tick(), Some(FactoryNotice::Paused(Pause::NoRoom)));
        assert_eq!(
            (shop.job().made, shop.job().paused),
            (2, Some(Pause::NoRoom))
        );
        reload(&mut shop);
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 0);
        assert_eq!(shop.ticks(10), []);
        shop.free(free[1]);
        assert_eq!(
            shop.tick(),
            Some(FactoryNotice::Finished {
                item: id(CORE_ITEM),
                made: 2,
                batch: 5,
                spent: false
            })
        );
        assert_eq!(shop.cores()[0].count, 2);
        assert_eq!(shop.state.job, None);
    }

    /// A benched member put on a run works it at the full rate, and can be
    /// taken off and put back while it runs; nobody else can be named.
    #[test]
    fn a_worker_must_be_benched_and_doubles_the_rate() {
        let mut shop = shop(first, 1000);
        let rules = shop.rules().clone();
        let fed = shop.cheapest(3);
        assert_eq!(
            assign(&mut shop.state, &shop.benched, Some(WORKER)),
            Err(FactoryRefusal::NoJob)
        );
        let before = shop.everything();
        assert_eq!(
            shop.start(3, &fed, Some("Frontier ranger")),
            Err(FactoryRefusal::NotOnBench),
            "whoever is out is not on the bench"
        );
        assert_eq!(shop.everything(), before);
        assert_eq!(shop.start(3, &fed, Some(WORKER)), Ok((5, 36)));
        assert_eq!(shop.job().worker.as_deref(), Some(WORKER));
        assert_eq!(shop.ticks(239), []);
        assert_eq!(shop.job().made, 0);
        shop.tick();
        assert_eq!(shop.job().made, 1, "a unit in 240 ticks");
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 24);

        // Taken off, it goes on at half the rate.
        assert_eq!(assign(&mut shop.state, &shop.benched, None), Ok(()));
        assert_eq!(shop.job().worker, None);
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 48);
        assert_eq!(shop.ticks(479), []);
        assert_eq!(shop.job().made, 1);
        shop.tick();
        assert_eq!(shop.job().made, 2);
        assert_eq!(
            assign(&mut shop.state, &shop.benched, Some("A stranger")),
            Err(FactoryRefusal::NotOnBench)
        );
        assert_eq!(shop.job().worker, None);
        // Put back in the middle of a unit, the unit keeps what it had.
        shop.ticks(240);
        assert_eq!(assign(&mut shop.state, &shop.benched, Some(WORKER)), Ok(()));
        assert_eq!(left(&rules, shop.print(), shop.job(), &shop.clock), 6);
        let told = shop.ticks(120);
        assert!(matches!(
            told[..],
            [(120, FactoryNotice::Finished { made: 3, .. })]
        ));
    }

    fn ask_factory(app: &mut App, request: FactoryRequest) -> Result<FactoryDone, FactoryRefusal> {
        app.world_mut()
            .resource_mut::<FactoryInbox>()
            .push(request.clone());
        app.update();
        let mut answers: Vec<FactoryAnswer> = app
            .world_mut()
            .resource_mut::<Messages<FactoryAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        let answer = answers.pop().unwrap();
        assert_eq!(answer.request, request);
        answer.outcome
    }

    fn told(app: &mut App) -> Vec<FactoryNotice> {
        app.world_mut()
            .resource_mut::<Messages<FactoryNotice>>()
            .drain()
            .collect()
    }

    /// A blueprint is recorded and run by anyone: the box a schematic needs
    /// is asked when a session opens, and neither a recording nor a run
    /// opens one.
    #[test]
    fn a_blueprint_run_needs_no_box() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        crate::crafting::tests::assemble_best(&mut app, "veyra:schematic.patrol_carbine").unwrap();
        work_and_finish(&mut app);
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = CharacterSheet::default();
        }
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        assert!(
            matches!(recorded, Ok(CraftDone::Recorded { .. })),
            "{recorded:?}"
        );
        app.world_mut().despawn(bench);
        prop(&mut app, InteractionKind::Fabricator);
        let print = app.world().resource::<Blueprints>().0[&2].clone();
        // A weapon's unit is whole, or the catalogue refuses it and the run
        // is Stale for ever.
        assert!(print.stands(&registries.0), "a recorded carbine stands");
        assert_eq!(
            print.unit(1, &registries.0.items).condition,
            Some(registries.0.items.wear().full)
        );
        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 1);
        let started = ask_factory(
            &mut app,
            FactoryRequest::Start {
                blueprint: 2,
                batch: 1,
                feeds: fed,
                worker: None,
            },
        );
        assert!(
            matches!(started, Ok(FactoryDone::Started { .. })),
            "{started:?}"
        );
    }

    /// Through the tick: a core is made and recorded at a workbench, and a
    /// run is started, worked and stopped only with a fabricator in reach;
    /// the run itself goes on wherever the player is, and says when it is
    /// done.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one household, from the bench to the fabricator and away from it"
    )]
    fn away_from_a_fabricator_is_refused() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        assert_eq!(
            crate::crafting::tests::assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 3 })
        );
        let core = work_and_finish(&mut app);
        assert_eq!(core.batch, 1);
        // Recorded by request, with the workbench in reach.
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        let Ok(CraftDone::Recorded {
            blueprint,
            runs,
            item,
        }) = recorded
        else {
            panic!("{recorded:?}");
        };
        assert_eq!((blueprint, runs), (2, 100));
        assert_eq!((item.item.clone(), item.batch), (id(BLUEPRINT), 2));
        assert_eq!(
            ask(
                &mut app,
                CraftRequest::Record {
                    batch: 1,
                    derate: 850
                }
            ),
            Err(crate::crafting::CraftRefusal::Recorded { blueprint: 2 })
        );
        let print = app.world().resource::<Blueprints>().0[&2].clone();
        assert_eq!(print.lines, core.lines);
        assert_eq!(print.by, MAKER);

        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 2);
        let start = |worker: Option<&str>| FactoryRequest::Start {
            blueprint: 2,
            batch: 2,
            feeds: fed.clone(),
            worker: worker.map(str::to_owned),
        };
        let stock = |app: &mut App| {
            let (pack, _, store, _, craft) = truth(app);
            (
                pack,
                store,
                craft,
                app.world().resource::<FactoryState>().clone(),
            )
        };
        // A workbench is no fabricator, and neither is the open.
        let before = stock(&mut app);
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Err(FactoryRefusal::NoFabricator)
        );
        app.world_mut().despawn(bench);
        app.update();
        for request in [
            start(None),
            FactoryRequest::Stop,
            FactoryRequest::Assign { worker: None },
        ] {
            assert_eq!(
                ask_factory(&mut app, request),
                Err(FactoryRefusal::NoFabricator)
            );
        }
        assert_eq!(stock(&mut app), before, "nothing moved");

        // At one, a worker who is not on the bench is refused and then the
        // run starts with nobody.
        let fabricator = prop(&mut app, InteractionKind::Fabricator);
        assert!(app.world().resource::<Reach>().fabricator);
        assert_eq!(
            ask_factory(&mut app, start(Some(WORKER))),
            Err(FactoryRefusal::NotOnBench)
        );
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Ok(FactoryDone::Started {
                batch: 3,
                seconds: 48
            })
        );
        assert_eq!(
            ask_factory(&mut app, start(None)),
            Err(FactoryRefusal::Busy)
        );
        app.insert_resource(Benched(vec![WORKER.to_owned()]));
        assert_eq!(
            ask_factory(
                &mut app,
                FactoryRequest::Assign {
                    worker: Some(WORKER.to_owned())
                }
            ),
            Ok(FactoryDone::Assigned)
        );
        let job = app.world().resource::<FactoryState>().job.clone().unwrap();
        assert_eq!(
            (job.batch, job.size, job.worker.as_deref()),
            (3, 2, Some(WORKER))
        );
        assert_eq!(
            app.world().resource::<CraftState>().made[&3].blueprint,
            Some(2)
        );

        // Walked away from: it cannot be stopped from the open, and it
        // goes on all the same.
        app.world_mut().despawn(fabricator);
        app.update();
        assert_eq!(
            ask_factory(&mut app, FactoryRequest::Stop),
            Err(FactoryRefusal::NoFabricator)
        );
        assert!(told(&mut app).is_empty());
        let mut finished = Vec::new();
        for _ in 0..480 {
            app.update();
            finished.extend(told(&mut app));
            if !finished.is_empty() {
                break;
            }
        }
        assert_eq!(
            finished,
            [FactoryNotice::Finished {
                item: id(CORE_ITEM),
                made: 2,
                batch: 3,
                spent: false
            }]
        );
        let (pack, _, store, ..) = truth(&mut app);
        let items = &registries.0.items;
        let made = Record {
            count: 2,
            ..print.unit(3, items)
        };
        assert_eq!(units(&store.0, &made, items), 2);
        assert_eq!(made.lines, core.lines, "the prototype's lines, exactly");
        let prints = app.world().resource::<Blueprints>().clone();
        assert_eq!(prints.0[&2].runs_left, 98);
        assert_eq!(
            blueprint::held(&registries.0, &pack, Some(&store), &prints),
            [(2, true)]
        );
        assert_eq!(app.world().resource::<FactoryState>().job, None);

        // A run that lifts the rule (a still of the screen) works
        // anywhere, and with nobody to work it, nobody does.
        app.insert_resource(WorkbenchAccess::Anywhere);
        let fed = cheapest(&registries.0, &pack, Some(&store), &prints.0[&2], 1);
        let again = FactoryRequest::Start {
            blueprint: 2,
            batch: 1,
            feeds: fed,
            worker: None,
        };
        assert_eq!(
            ask_factory(&mut app, again.clone()),
            Ok(FactoryDone::Started {
                batch: 4,
                seconds: 24
            })
        );
        assert_eq!(
            ask_factory(&mut app, FactoryRequest::Stop),
            Ok(FactoryDone::Stopped { made: 0 })
        );
        let mut empty = app_in_world();
        empty.insert_resource(shipped());
        assert_eq!(
            ask_factory(&mut empty, again),
            Err(FactoryRefusal::NoPlayer)
        );
        // The props of the test are gone; nothing else was spawned.
        let mut props = app.world_mut().query::<&Interaction>();
        assert_eq!(props.iter(app.world()).count(), 0);
    }

    /// A run is kept at the pace of whoever started it: the sheet's Fixtures
    /// quicken a unit and the unattended share, the figures ride with the job
    /// through a save and a change of who is at the bench, and a job saved
    /// before the boxes had effects reads the dials.
    #[test]
    fn a_run_keeps_its_starters_pace_across_a_switch_and_a_save() {
        let mut shop = shop(first, 1000);
        let full = crate::crafting::tests::whole_tree(&shop.registries, "fabricator");
        let pace = pace_of(shop.rules(), &shop.registries.0.progression, &full);
        assert_eq!(
            pace,
            Some(Pace {
                ticks_per_point: 138,
                unattended_pm: 650
            })
        );
        let fed = shop.cheapest(3);
        // Three units of 6.9 s at 0.65 with nobody on it: 414 / 0.65 ticks.
        assert_eq!(
            shop.start_paced(3, &fed, (None, pace)),
            Ok((5, 32)),
            "636.9 ticks of twenty a second"
        );
        let rules = shop.rules().clone();
        let left = |shop: &Shop| left(&rules, shop.print(), shop.job(), &shop.clock);
        assert_eq!(left(&shop), 32);
        // A save, a bench of another member and a retuned sheet change
        // nothing: the job alone says how fast it goes.
        let saved = ron::to_string(&shop.state).unwrap();
        shop.state = ron::from_str(&saved).unwrap();
        assert_eq!(shop.job().pace, pace);
        assert_eq!(left(&shop), 32);
        assert_eq!(shop.ticks(636), []);
        let told = shop.tick();
        assert!(
            matches!(told, Some(FactoryNotice::Finished { made: 3, .. })),
            "{told:?}"
        );
        // The dials as they are for a run that kept none, which is also what
        // an old save without the key reads.
        let fed = shop.cheapest(1);
        assert_eq!(shop.start(1, &fed, None), Ok((6, 24)));
        let saved = ron::to_string(&shop.state).unwrap();
        let old = saved.replace(",pace:None", "");
        assert_ne!(old, saved, "the key is written");
        let read: FactoryState = ron::from_str(&old).unwrap();
        assert_eq!(read.job.unwrap().pace, None);
    }

    /// Through the tick: a blueprint recorded by a member with Fixtures 2 has
    /// a quarter more runs, a run started by one with Fixtures 1 and 3 is
    /// kept at their pace, and a switch to a sheet with no box afterwards
    /// leaves it.
    #[test]
    fn a_recording_and_a_start_read_the_sheet_of_the_member_at_the_bench() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let bench = prop(&mut app, InteractionKind::Workbench);
        let tree = crate::crafting::tests::whole_tree(&registries, "fabricator");
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = tree;
        }
        assert_eq!(
            crate::crafting::tests::assemble_best(&mut app, CORE),
            Ok(CraftDone::Assembled { points: 6 })
        );
        work_and_finish(&mut app);
        let recorded = ask(
            &mut app,
            CraftRequest::Record {
                batch: 1,
                derate: 1000,
            },
        );
        let Ok(CraftDone::Recorded {
            runs, blueprint, ..
        }) = recorded
        else {
            panic!("{recorded:?}");
        };
        assert_eq!(runs, 125, "a hundred and a quarter");
        assert_eq!(app.world().resource::<Blueprints>().0[&blueprint].runs, 125);
        app.world_mut().despawn(bench);
        prop(&mut app, InteractionKind::Fabricator);
        let print = app.world().resource::<Blueprints>().0[&blueprint].clone();
        let (pack, _, store, ..) = truth(&mut app);
        let fed = cheapest(&registries.0, &pack, Some(&store), &print, 2);
        let started = ask_factory(
            &mut app,
            FactoryRequest::Start {
                blueprint,
                batch: 2,
                feeds: fed,
                worker: None,
            },
        );
        // Two units of 6.9 s with nobody on it, 0.65 of the rate.
        assert_eq!(
            started,
            Ok(FactoryDone::Started {
                batch: 3,
                seconds: 22
            })
        );
        let kept = Some(Pace {
            ticks_per_point: 138,
            unattended_pm: 650,
        });
        assert_eq!(
            app.world()
                .resource::<FactoryState>()
                .job
                .as_ref()
                .unwrap()
                .pace,
            kept
        );
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut CharacterSheet, With<LocalPlayer>>();
            *players.single_mut(world).unwrap() = CharacterSheet::default();
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<FactoryState>()
                .job
                .as_ref()
                .unwrap()
                .pace,
            kept
        );
    }
}
