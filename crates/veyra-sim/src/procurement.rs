//! The adapter half of the `procurement` rules: its requests, systems and plugin.

pub use veyra_rules::procurement::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_core::SIM_TICK_HZ;

use strata_registry::item::Record;

use strata_registry::Namespaced;

use veyra_world::factions::FactionId;

use crate::deeds::{LeaseRefusal, Ledger, lease_buyer};

use crate::deposits::Purse;

use crate::fitting::WorkbenchAccess;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::Reach;

use crate::{AppRequestExt, Inbox, Registries, Request, SimClock};

/// What a screen asks of procurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcurementRequest {
    /// Buy one of what the buyer sells.
    Buy {
        /// The buyer.
        buyer: Namespaced,
        /// The item.
        item: Namespaced,
    },
    /// Sell up to `count` of `kind` to a line.
    Sell {
        /// The buyer.
        buyer: Namespaced,
        /// The line.
        line: String,
        /// The kind of stack, as an offer names it.
        kind: Record,
        /// How many at most.
        count: u32,
    },
    /// Buy a lease of a faction's concessions at its board.
    Lease {
        /// The faction whose concessions it opens.
        faction: FactionId,
    },
    /// Take a posting the buyer's board shows, to clear a camp for its pay.
    /// Answered by [`crate::postings::consume`], which runs first.
    TakeJob {
        /// The buyer.
        buyer: Namespaced,
        /// The camp's site key.
        key: u64,
    },
    /// Collect the pay of a job whose camp is cleared. Answered by
    /// [`crate::postings::consume`].
    Collect {
        /// The buyer.
        buyer: Namespaced,
        /// The camp's site key.
        key: u64,
    },
}

/// The requests not yet consumed, a resource for the reason the stock's
/// inbox is one. Two systems drain it: [`crate::postings::consume`] takes
/// the job requests, [`consume`] the rest, and the postings' one registers
/// the inbox and addresses it.
pub type ProcurementInbox = Inbox<ProcurementRequest>;

/// With nobody to buy or sell for, a purchase finds no room and a sale
/// nothing held, as ever. A lease and a job are the household's, and are
/// answered whoever is there.
impl Request for ProcurementRequest {
    type Answer = SaleAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        match self {
            Self::Buy { .. } => {
                commands.write_message(PurchaseAnswer {
                    result: Err(PurchaseRefusal::NoRoom),
                });
            }
            Self::Sell { .. } => {
                commands.write_message(SaleAnswer {
                    result: Err(SaleRefusal::NoneHeld),
                });
            }
            Self::Lease { .. } | Self::TakeJob { .. } | Self::Collect { .. } => {}
        }
    }

    fn needs_actor(&self) -> bool {
        matches!(self, Self::Buy { .. } | Self::Sell { .. })
    }
}

/// `SimTick`, `SimSet::Input`, after the wear requests: every sale asked
/// for since the last tick, in order, each on the stock and the books the
/// one before left, so two in one tick cannot take a line past its
/// quantity. The store is to hand with a board in reach, as it is at a
/// bench.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock, the books and the purse in, two answers out"
)]
#[allow(clippy::too_many_lines, reason = "one arm a request, then the resync")]
pub fn consume(
    mut inbox: ResMut<ProcurementInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    mut state: ResMut<ProcurementState>,
    mut purse: ResMut<Purse>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands)>,
    mut answers: MessageWriter<SaleAnswer>,
    mut purchases: MessageWriter<PurchaseAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut leases: MessageWriter<LeaseAnswer>,
    mut ledger: Ledger,
) {
    // The jobs were taken out by the postings' system, which runs first.
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let mut changed = false;
    let mut last = None;
    for (actor, request) in requests {
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
        let (buyer, line, kind, count) = match request {
            ProcurementRequest::Sell {
                buyer,
                line,
                kind,
                count,
            } => (buyer, line, kind, count),
            // The postings' own system took these out of the inbox first.
            ProcurementRequest::TakeJob { .. } | ProcurementRequest::Collect { .. } => continue,
            ProcurementRequest::Lease { faction } => {
                let result = match ledger.map().and_then(|map| map.faction(faction)) {
                    None => Err(LeaseRefusal::NoLease),
                    Some(faction) => {
                        let board = lease_buyer(&registries.0, &faction)
                            .is_some_and(|buyer| at_board(*access, &reach, &buyer));
                        // A guard of the faction's concession sells it too.
                        let guard = reach.guards.contains(&faction.id);
                        let mut paid = *purse;
                        let done = ledger.lease(&registries.0, board || guard, &mut paid, &faction);
                        if done.is_ok() {
                            *purse = paid;
                        }
                        done
                    }
                };
                leases.write(LeaseAnswer { result });
                continue;
            }
            ProcurementRequest::Buy { buyer, item } => {
                let terms = ledger.terms(&registries.0, &buyer);
                let board = at_board(*access, &reach, &buyer);
                let result = match player.as_mut() {
                    None => Err(PurchaseRefusal::NoRoom),
                    Some((pack, hands)) => {
                        let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                        let mut paid = *purse;
                        let mut stock = Stock {
                            saddle: None,
                            pack: &mut next.0,
                            hands: &mut next.1,
                            store: Some(&mut next.2),
                        };
                        let done = buy_on(
                            &registries.0,
                            board,
                            terms,
                            &mut stock,
                            &mut paid,
                            &buyer,
                            &item,
                        );
                        if done.is_ok() {
                            changed = true;
                            if **pack != next.0 {
                                **pack = next.0;
                            }
                            **hands = next.1;
                            if *store != next.2 {
                                *store = next.2;
                            }
                            *purse = paid;
                        }
                        done
                    }
                };
                purchases.write(PurchaseAnswer { result });
                continue;
            }
        };
        let terms = ledger.terms(&registries.0, &buyer);
        let board = at_board(*access, &reach, &buyer);
        let result = match player.as_mut() {
            None => Err(SaleRefusal::NoneHeld),
            Some((pack, hands)) => {
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut books = (state.clone(), *purse);
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = sell_on(
                    &registries.0,
                    board,
                    terms,
                    &mut stock,
                    &mut books.0,
                    &mut books.1,
                    &buyer,
                    &line,
                    &kind,
                    count,
                );
                if let Ok(sold) = &done {
                    changed = true;
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                    *state = books.0;
                    *purse = books.1;
                    ledger.sold(&registries.0, &buyer, sold.paid);
                }
                done
            }
        };
        answers.write(SaleAnswer { result });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some(actor) = last
        && let Ok((pack, hands)) = players.get(actor)
    {
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

/// `SimTick`, `SimSet::Input`, after [`consume`]: played time refills the
/// upkeep lines, as many nominal ticks as this step of the sim is worth
/// ([`crate::factory::nominal_ticks`]). The state is marked changed only
/// when a count moves, which is once an interval: the household saves on a
/// change.
pub fn refill(
    registries: Res<Registries>,
    sim: Option<Res<SimClock>>,
    mut state: ResMut<ProcurementState>,
    mut clock: ResMut<ProcurementClock>,
    mut phase: Local<u32>,
) {
    if state.taken.is_empty() {
        return;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let ticks = crate::factory::nominal_ticks(&mut phase, stepped);
    if advance(
        &registries.0,
        state.bypass_change_detection(),
        &mut clock,
        ticks,
    ) {
        state.set_changed();
    }
}

/// Registers the books, the clock and the request. Added by `SimPlugin`,
/// so a server and a harness have it too; with no request and nothing
/// taken it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct ProcurementPlugin;

impl Plugin for ProcurementPlugin {
    fn build(&self, app: &mut App) {
        // The inbox, the sale answers and the addressing belong to the
        // postings' `add_request`, in the set before this one.
        app.init_resource::<ProcurementState>()
            .init_resource::<ProcurementClock>()
            .add_message::<PurchaseAnswer>()
            .add_message::<LeaseAnswer>()
            .add_request_consumer(RequestSet::Procurement, (consume, refill).chain());
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
    use crate::crafting::tests::ALLOY;
    use crate::crafting::tests::BRINE;
    use crate::crafting::tests::CORE_ITEM;
    use crate::crafting::tests::first_day;
    use crate::crafting::tests::id;
    use crate::crafting::tests::lot;
    use crate::crafting::tests::part;
    use crate::crafting::tests::prop;
    use crate::crafting::tests::shipped;
    use crate::deeds::Deeds;
    use crate::deeds::Leases;
    use crate::progression::InteractionKind;
    use bevy::ecs::message::Messages;
    use bevy::prelude::App;

    use strata_registry::item::Record;

    use veyra_world::factions::FactionId;

    use crate::deeds::LeaseRefusal;
    use crate::deeds::Leased;

    use crate::deposits::Purse;

    use crate::items;

    use crate::items::HouseholdStore;

    use crate::items::Pack;

    use crate::Registries;

    pub(crate) use veyra_rules::procurement::test_support::*;

    fn board(app: &mut App) {
        prop(app, InteractionKind::Board { buyer: buyer() });
    }

    fn store_in(app: &mut App, record: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let stack = items::write(record, &registries.0.items).unwrap();
        let mut store = app.world_mut().resource_mut::<HouseholdStore>();
        assert!(items::place_in(&registries.0.items, &mut store.0, stack).is_none());
    }

    fn pack_in(app: &mut App, record: &Record) {
        let registries = app.world().resource::<Registries>().clone();
        let stack = items::write(record, &registries.0.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        pack.cells.set(cell, Some(stack));
    }

    /// One tick with every `(line, kind, count)` asked, in order.
    fn sell_all(app: &mut App, asks: &[(&str, &Record, u32)]) -> Vec<Result<Sold, SaleRefusal>> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .extend(
                asks.iter()
                    .map(|(line, kind, count)| ProcurementRequest::Sell {
                        buyer: buyer(),
                        line: (*line).to_owned(),
                        kind: (*kind).clone(),
                        count: *count,
                    }),
            );
        app.update();
        app.world_mut()
            .resource_mut::<Messages<SaleAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect()
    }

    fn held(app: &mut App, kind: &Record) -> (u32, u32) {
        let items = app.world().resource::<Registries>().0.items.clone();
        let (pack, _, store, ..) = crate::crafting::tests::truth(app);
        (
            crate::crafting::tests::units(&pack.cells, kind, &items),
            crate::crafting::tests::units(&store.0, kind, &items),
        )
    }

    fn purse(app: &App) -> u32 {
        app.world().resource::<Purse>().0
    }

    fn state(app: &App) -> ProcurementState {
        app.world().resource::<ProcurementState>().clone()
    }

    #[test]
    fn a_sale_ends_at_the_quantity_and_leaves_the_rest() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 100)]);
        let sold = done[0].clone().unwrap();
        assert_eq!((sold.count, sold.paid), (40, 5_875));
        assert_eq!(purse(&app), before + 5_875);
        assert_eq!(held(&mut app, &kind), (0, 60));
        let again = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(again, vec![Err(SaleRefusal::Filled)]);
        assert_eq!(held(&mut app, &kind), (0, 60));
    }

    #[test]
    fn two_sales_in_one_tick_cannot_oversell_a_line() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let done = sell_all(
            &mut app,
            &[("cell_cores", &kind, 30), ("cell_cores", &kind, 30)],
        );
        let counts: Vec<_> = done
            .iter()
            .map(|each| each.clone().unwrap().count)
            .collect();
        assert_eq!(counts, vec![30, 10]);
        assert_eq!(state(&app).taken.values().sum::<u32>(), 40);
    }

    #[test]
    fn a_kind_under_the_minimum_is_refused_with_its_figure() {
        let mut app = first_day();
        board(&mut app);
        let kind = cores(10, 450);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(
            done,
            vec![Err(SaleRefusal::BelowMinimum {
                has: 450,
                needs: 500
            })]
        );
        assert_eq!(purse(&app), before);
        let registries = shipped();
        let works = registries.0.buyers.get(&buyer()).unwrap();
        let (_, line) = works.line("cell_cores").unwrap();
        let (pack, _, store, ..) = crate::crafting::tests::truth(&mut app);
        assert!(offers(&registries.0, works, line, &pack, Some(&store)).is_empty());
        assert_eq!(
            best_short(&registries.0, line, &pack, Some(&store)),
            Some(450)
        );
        // A magazine worn under the slide is not graded at its made figure.
        let (_, feed) = works.line("patrol_magazines").unwrap();
        let mut mag = part(
            &registries,
            "veyra:module.patrol_extended_feed",
            1,
            Some((1, &[610, 500])),
        );
        assert_eq!(grade_of(&registries.0.items, feed, &mag), Some(610));
        mag.condition = Some(0);
        assert_eq!(grade_of(&registries.0.items, feed, &mag), None);
    }

    #[test]
    fn a_lot_sells_by_units_and_only_of_its_class() {
        let mut app = first_day();
        board(&mut app);
        let registries = shipped();
        let mut ore = lot(
            &registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        );
        ore.lines.insert("strength".to_owned(), 550);
        let brine = lot(&registries, BRINE, "Oxbow", 160, &[534, 605, 495]);
        store_in(&mut app, &ore);
        let before = purse(&app);
        let done = sell_all(
            &mut app,
            &[
                ("structural_metal", &brine, 50),
                ("structural_metal", &ore, 1000),
            ],
        );
        assert_eq!(done[0], Err(SaleRefusal::NotWanted));
        // 200 are in hand, 600 wanted; 2 a unit and 100 points over the minimum.
        let sold = done[1].clone().unwrap();
        assert_eq!((sold.count, sold.paid), (200, 200 * 2 * 1100 / 1000));
        assert_eq!(purse(&app), before + sold.paid);
    }

    #[test]
    fn stock_leaves_the_pack_before_the_store() {
        let mut app = first_day();
        board(&mut app);
        pack_in(&mut app, &cores(30, 724));
        store_in(&mut app, &cores(100, 724));
        let kind = cores(1, 724);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 40)]);
        assert_eq!(done[0].clone().unwrap().count, 40);
        assert_eq!(held(&mut app, &kind), (0, 90));
    }

    #[test]
    fn the_ledger_names_the_batch_and_adds_a_second_sale() {
        let mut app = first_day();
        board(&mut app);
        store_in(&mut app, &cores(100, 724));
        let kind = cores(1, 724);
        sell_all(&mut app, &[("cell_cores", &kind, 15)]);
        sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        let registries = shipped();
        let mut ore = lot(
            &registries,
            ALLOY,
            "Rake",
            200,
            &[533, 298, 312, 231, 451, 633, 165, 492],
        );
        ore.lines.insert("strength".to_owned(), 550);
        pack_in(&mut app, &ore);
        sell_all(&mut app, &[("structural_metal", &ore, 20)]);
        let sales = state(&app).sales;
        assert_eq!(sales.len(), 2);
        let core = &sales[0];
        assert_eq!(
            (
                core.buyer.clone(),
                core.line.as_str(),
                core.item.clone(),
                core.batch
            ),
            (buyer(), "cell_cores", id(CORE_ITEM), 412)
        );
        assert_eq!(
            (core.by.as_deref(), core.count),
            (Some("Frontier ranger"), 25)
        );
        assert_eq!(core.paid, pay_for(146_880, 15) + pay_for(146_880, 10));
        assert_eq!(sales[1].origin, ore.origin);
        assert_eq!((sales[1].batch, sales[1].count), (0, 20));
        // A repeat moves to the end, so the vector is oldest first.
        sell_all(&mut app, &[("cell_cores", &kind, 5)]);
        let sales = state(&app).sales;
        assert_eq!((sales[1].batch, sales[1].count), (412, 30));
    }

    #[test]
    fn upkeep_refills_with_played_time_and_a_project_never_does() {
        let registries = shipped();
        let mut state = ProcurementState::default();
        let mut clock = ProcurementClock::default();
        let cores = key(&buyer(), "cell_cores");
        let intake = key(&buyer(), "intake_metal");
        state.taken.insert(cores.clone(), 10);
        state.taken.insert(intake.clone(), 2000);
        let second = 20;
        assert!(!advance(
            &registries.0,
            &mut state,
            &mut clock,
            599 * second
        ));
        assert_eq!(state.taken[&cores], 10);
        assert!(advance(&registries.0, &mut state, &mut clock, second));
        assert_eq!(state.taken[&cores], 6);
        advance(&registries.0, &mut state, &mut clock, 600 * second);
        advance(&registries.0, &mut state, &mut clock, 600 * second);
        // Never below nought, and a line with nothing taken carries no time.
        assert!(!state.taken.contains_key(&cores));
        assert!(!clock.seconds.contains_key(&cores));
        advance(&registries.0, &mut state, &mut clock, 100_000 * second);
        assert_eq!(state.taken[&intake], 2000);
    }

    #[test]
    fn no_board_in_reach_is_no_sale() {
        let mut app = first_day();
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let done = sell_all(&mut app, &[("cell_cores", &kind, 10)]);
        assert_eq!(done, vec![Err(SaleRefusal::NoBoard)]);
        assert_eq!(held(&mut app, &kind), (0, 100));
        assert!(state(&app).sales.is_empty());
    }

    fn buy_one(app: &mut App, item: &str) -> Result<Bought, PurchaseRefusal> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .push(ProcurementRequest::Buy {
                buyer: buyer(),
                item: id(item),
            });
        app.update();
        let mut answers: Vec<PurchaseAnswer> = app
            .world_mut()
            .resource_mut::<Messages<PurchaseAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        answers.pop().unwrap().result
    }

    fn snapshot(app: &mut App) -> (Pack, HouseholdStore, Purse) {
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&Pack, With<LocalPlayer>>();
        let pack = packs.single(world).unwrap().clone();
        (
            pack,
            world.resource::<HouseholdStore>().clone(),
            *world.resource::<Purse>(),
        )
    }

    /// Every cell of the pack, its open bags and the store taken.
    fn fill_everything(app: &mut App) {
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let world = app.world_mut();
        let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = packs.single_mut(world).unwrap();
        let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
            .map(|bag| usize::from(pack.open_cells(bag, items)))
            .collect();
        let all = pack.cells.len();
        crate::crafting::tests::fill(&mut pack.cells, all, items);
        for (bag, open) in pack.bags.iter_mut().zip(open) {
            crate::crafting::tests::fill(bag, open, items);
        }
        let mut store = world.resource_mut::<HouseholdStore>();
        let all = store.0.len();
        crate::crafting::tests::fill(&mut store.0, all, items);
    }

    /// The board sells a store grade bike for its price: the purse and the
    /// stock change by exactly that, and every refusal changes nothing.
    #[test]
    fn a_bike_is_bought_at_the_board_and_each_refusal_changes_nothing() {
        let mut app = first_day();
        app.insert_resource(Purse(600));
        let registries = app.world().resource::<Registries>().clone();
        let before = snapshot(&mut app);
        // No board in reach.
        assert_eq!(buy_one(&mut app, BIKE), Err(PurchaseRefusal::NotAtBoard));
        assert_eq!(snapshot(&mut app), before);
        board(&mut app);
        // Not a thing this buyer sells.
        assert_eq!(buy_one(&mut app, CORE_ITEM), Err(PurchaseRefusal::NotSold));
        assert_eq!(
            buy_one(&mut app, "veyra:vehicle.nope"),
            Err(PurchaseRefusal::NotSold)
        );
        assert_eq!(snapshot(&mut app), before);
        // The purse is short.
        app.insert_resource(Purse(449));
        assert_eq!(
            buy_one(&mut app, BIKE),
            Err(PurchaseRefusal::Short {
                needs: 450,
                have: 449
            })
        );
        let short = snapshot(&mut app);
        assert_eq!(short.2, Purse(449));
        assert_eq!((&short.0, &short.1), (&before.0, &before.1));
        // No room anywhere.
        app.insert_resource(Purse(600));
        let kept = snapshot(&mut app);
        fill_everything(&mut app);
        let full = snapshot(&mut app);
        assert_eq!(buy_one(&mut app, BIKE), Err(PurchaseRefusal::NoRoom));
        assert_eq!(snapshot(&mut app), full);
        // Room, and the money.
        let mut app = first_day();
        app.insert_resource(Purse(600));
        board(&mut app);
        let before = snapshot(&mut app);
        let done = buy_one(&mut app, BIKE).unwrap();
        assert_eq!((done.paid, done.item), (450, id(BIKE)));
        let (pack, store, purse) = snapshot(&mut app);
        assert_eq!(purse, Purse(150));
        assert_eq!(store, before.1, "the pack had room, the store is as it was");
        let bikes: Vec<Record> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &registries.0.items).ok())
            .filter(|record| record.item == id(BIKE))
            .collect();
        assert_eq!(bikes.len(), 1);
        let bike = &bikes[0];
        assert_eq!(
            (bike.batch, bike.by.as_ref(), bike.condition),
            (0, None, Some(1000))
        );
        assert!(bike.lines.values().all(|line| *line == 500));
        assert_eq!(bike.lines.len(), 3);
        // Exactly one stack more than before, and nothing else moved.
        let count = |pack: &Pack| pack.cells.slots().iter().flatten().count();
        assert_eq!(count(&pack), count(&before.0) + 1);
        let _ = kept;
        // The pure rule says the same as the request.
        let (pack, store, purse) = snapshot(&mut app);
        assert_eq!(
            check_purchase(
                &registries.0,
                true,
                &Purse(449),
                &pack,
                Some(&store),
                &buyer(),
                &id(BIKE)
            ),
            Err(PurchaseRefusal::Short {
                needs: 450,
                have: 449
            })
        );
        assert_eq!(
            check_purchase(
                &registries.0,
                true,
                &purse,
                &pack,
                Some(&store),
                &buyer(),
                &id("veyra:tool.survey")
            ),
            Ok(120)
        );
    }

    /// The board sells the three melee classes at store grade, a whole
    /// weapon with its condition, for the prices of the buyer's file.
    #[test]
    fn the_board_sells_a_blade_a_greatblade_and_a_glaive() {
        let mut app = first_day();
        app.insert_resource(Purse(1000));
        board(&mut app);
        let registries = app.world().resource::<Registries>().clone();
        for (item, price) in [
            ("veyra:weapon.service_blade", 180),
            ("veyra:weapon.yard_greatblade", 260),
            ("veyra:weapon.pier_glaive", 240),
        ] {
            let (pack, store, purse) = snapshot(&mut app);
            assert_eq!(
                check_purchase(
                    &registries.0,
                    true,
                    &purse,
                    &pack,
                    Some(&store),
                    &buyer(),
                    &id(item)
                ),
                Ok(price)
            );
            let done = buy_one(&mut app, item).unwrap();
            assert_eq!((done.paid, done.item), (price, id(item)));
            let (pack, _, after) = snapshot(&mut app);
            assert_eq!(after.0, purse.0 - price);
            let bought: Vec<Record> = pack
                .cells
                .slots()
                .iter()
                .flatten()
                .filter_map(|stack| items::read(stack, &registries.0.items).ok())
                .filter(|record| record.item == id(item))
                .collect();
            assert_eq!(bought.len(), 1);
            assert_eq!((bought[0].batch, bought[0].condition), (0, Some(1000)));
            assert!(bought[0].lines.values().all(|line| *line == 500));
        }
    }

    /// With the pack full the bike goes to the store the board reaches.
    #[test]
    fn a_full_pack_buys_into_the_store() {
        let mut app = first_day();
        app.insert_resource(Purse(500));
        board(&mut app);
        {
            let registries = app.world().resource::<Registries>().clone();
            let items = &registries.0.items;
            let world = app.world_mut();
            let mut packs = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = packs.single_mut(world).unwrap();
            let open: Vec<usize> = (0..strata_registry::item::BAG_SLOTS)
                .map(|bag| usize::from(pack.open_cells(bag, items)))
                .collect();
            let all = pack.cells.len();
            crate::crafting::tests::fill(&mut pack.cells, all, items);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                crate::crafting::tests::fill(bag, open, items);
            }
        }
        let before = snapshot(&mut app);
        assert!(buy_one(&mut app, BIKE).is_ok());
        let after = snapshot(&mut app);
        assert_eq!(after.0, before.0, "the pack was full");
        assert_eq!(
            after.1.0.slots().iter().flatten().count(),
            before.1.0.slots().iter().flatten().count() + 1
        );
        assert_eq!(after.2, Purse(50));
    }

    /// The first day with Pelagic Works' world, so its board has a holder.
    fn with_holder() -> App {
        let mut app = first_day();
        app.insert_resource(crate::factions::Factions(std::sync::Arc::clone(
            &crate::deeds::tests::pelagic().0,
        )));
        board(&mut app);
        app
    }

    #[test]
    fn a_sale_at_pelagics_board_is_a_deed_for_it_and_pays_the_welcoming_price() {
        let mut app = with_holder();
        let kind = cores(100, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        let sold = sell_all(&mut app, &[("cell_cores", &kind, 100)])[0]
            .clone()
            .unwrap();
        // Pelagic starts welcoming: five per cent over the data's 5_875.
        assert_eq!(sold.paid, 6_168);
        assert_eq!(purse(&app), before + 6_168);
        let deeds = app.world().resource::<Deeds>();
        let list = &deeds.by[&pelagic().0];
        assert_eq!(
            (list[0].kind, list[0].amount),
            (crate::deeds::DeedKind::Sold, 6_168)
        );
    }

    #[test]
    fn a_hostile_board_refuses_to_trade() {
        let mut app = with_holder();
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        let kind = cores(10, 724);
        store_in(&mut app, &kind);
        let before = purse(&app);
        assert_eq!(
            sell_all(&mut app, &[("cell_cores", &kind, 10)]),
            vec![Err(SaleRefusal::Hostile)]
        );
        assert_eq!(purse(&app), before);
        let item = shipped()
            .0
            .buyers
            .get(&buyer())
            .unwrap()
            .sells
            .first()
            .unwrap()
            .item
            .clone();
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .push(ProcurementRequest::Buy {
                buyer: buyer(),
                item,
            });
        app.update();
        let bought: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PurchaseAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(bought, vec![Err(PurchaseRefusal::Hostile)]);
    }

    /// The first day beside one of Pelagic's guards and not its board.
    fn beside_a_guard(faction: FactionId) -> App {
        let mut app = first_day();
        app.insert_resource(crate::factions::Factions(std::sync::Arc::clone(
            &crate::deeds::tests::pelagic().0,
        )));
        prop(&mut app, InteractionKind::Guard { faction });
        app
    }

    #[test]
    fn a_guard_sells_a_lease_unless_it_is_anothers_or_hostile_or_short() {
        let claims = shipped().0.factions.claims().unwrap().clone();
        // A guard of some other faction is no seller of Pelagic's lease.
        let mut app = beside_a_guard(FactionId(1));
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::NotAtBoard));
        // Pelagic's own guard sells it at the stance's price.
        let mut app = beside_a_guard(pelagic());
        app.world_mut().insert_resource(Purse(10_000));
        let price = crate::deeds::Terms::scale(claims.lease.price, 950);
        assert_eq!(lease_once(&mut app).unwrap().paid, price);
        assert!(app.world().resource::<Leases>().live(pelagic(), 0.0));
        app.world_mut().insert_resource(Purse(price - 1));
        assert!(matches!(
            lease_once(&mut app),
            Err(LeaseRefusal::Short { .. })
        ));
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::Hostile));
        assert_eq!(purse(&app), 10_000);
    }

    fn lease_once(app: &mut App) -> Result<Leased, LeaseRefusal> {
        app.world_mut()
            .resource_mut::<ProcurementInbox>()
            .push(ProcurementRequest::Lease { faction: pelagic() });
        app.update();
        let mut answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<LeaseAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(answers.len(), 1);
        answers.remove(0)
    }

    #[test]
    fn a_lease_costs_credits_and_runs_for_played_hours_unless_hostile_or_short() {
        let mut app = with_holder();
        let claims = shipped().0.factions.claims().unwrap().clone();
        app.world_mut().insert_resource(Purse(10_000));
        let leased = lease_once(&mut app).unwrap();
        // Welcoming: the lease price at the sell factor.
        let price = crate::deeds::Terms::scale(claims.lease.price, 950);
        assert_eq!(leased.paid, price);
        assert_eq!(purse(&app), 10_000 - price);
        assert!(app.world().resource::<Leases>().live(pelagic(), 0.0));
        // From the played second now, a few ticks in.
        let run = leased.until - claims.lease.hours * 3600.0;
        assert!((0.0..60.0).contains(&run), "{run}");
        // Short of credits: nothing changes.
        app.world_mut().insert_resource(Purse(price - 1));
        assert!(matches!(
            lease_once(&mut app),
            Err(LeaseRefusal::Short { .. })
        ));
        assert_eq!(purse(&app), price - 1);
        // Hostile: refused before the purse is asked.
        let rules = *shipped().0.factions.deeds().unwrap();
        for _ in 0..4 {
            app.world_mut().resource_mut::<Deeds>().write(
                pelagic(),
                crate::deeds::DeedKind::Planted,
                1,
                0.0,
                &rules,
            );
        }
        app.world_mut().insert_resource(Purse(10_000));
        assert_eq!(lease_once(&mut app), Err(LeaseRefusal::Hostile));
        assert_eq!(purse(&app), 10_000);
    }
}
