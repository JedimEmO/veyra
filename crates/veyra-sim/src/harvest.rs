//! The adapter half of the `harvest` rules: its requests, systems and plugin.

pub use veyra_rules::harvest::*;

use bevy::app::{App, Plugin};

use bevy::ecs::message::MessageWriter;

use bevy::ecs::name::Name;

use bevy::ecs::prelude::*;

use strata_core::SIM_TICK_HZ;

use strata_registry::item::Record;

use crate::deeds::Ledger;

use crate::deposits::{
    DepositId, Lead, SurveyEvent, SurveyUnkept, Surveyed, concentration_here, credit_survey,
    hand_state, resync, survey_xp, with_lot,
};

use crate::factory::nominal_ticks;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, StockAnswer};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, XpGained};

use crate::ride::Riding;

use crate::{
    AppRequestExt, Inbox, Registries, Request, SimClock, WorldPosition, WorldSourceRes,
    deposits::Survey,
};

/// What a request came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarvestDone {
    /// A machine was planted.
    Planted {
        /// Its number.
        id: u32,
        /// The deposit.
        deposit: DepositId,
        /// The concentration where it stands, per-mille.
        pm: u16,
    },
    /// Units were collected.
    Collected {
        /// The machine.
        id: u32,
        /// Units that went into the pack.
        units: u32,
        /// Units left in the hopper.
        left: u32,
    },
    /// The machine went back into the pack.
    PickedUp {
        /// Its number, now spent.
        id: u32,
    },
    /// The HUD points to the machine.
    Led {
        /// The machine.
        id: u32,
    },
}

/// What a screen asks of the harvesters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarvestRequest {
    /// Plant a carried harvester where the player stands.
    Plant,
    /// Empty a machine in reach into the pack.
    Collect {
        /// The machine.
        id: u32,
    },
    /// Take an empty machine in reach back into the pack.
    PickUp {
        /// The machine.
        id: u32,
    },
    /// Point the HUD's lead marker at a machine.
    Lead {
        /// The machine.
        id: u32,
    },
}

/// The requests not yet consumed, a resource for the reason the stock's
/// inbox is one.
pub type HarvestInbox = Inbox<HarvestRequest>;

/// With nobody to work a machine the answer says so.
impl Request for HarvestRequest {
    type Answer = HarvestAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(HarvestAnswer {
            outcome: Err(HarvestRefusal::NoPlayer),
        });
    }
}

/// The sim's answer to a [`HarvestRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct HarvestAnswer {
    /// What happened, or why nothing did.
    pub outcome: Result<HarvestDone, HarvestRefusal>,
}

/// What the player carries and is, as the harvesters' requests read it.
type PlayerRow = (
    &'static mut Pack,
    &'static Hands,
    &'static WorldPosition,
    Option<&'static mut CharacterSheet>,
    Option<&'static Name>,
    Has<Riding>,
    Entity,
);

/// `SimTick`, `SimSet::Input`, after the survey's requests: every plant,
/// collect, pick-up and lead asked for since the last tick, in order.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the requests, the world, the household and the player in, an answer, the stock's mirror and the XP out"
)]
pub fn consume(
    mut inbox: ResMut<HarvestInbox>,
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    source: Option<Res<WorldSourceRes>>,
    open: Res<OpenStock>,
    store: Res<HouseholdStore>,
    unkept: Option<Res<SurveyUnkept>>,
    mut harvesters: ResMut<Harvesters>,
    mut surveyed: ResMut<Surveyed>,
    mut players: Query<PlayerRow>,
    mut commands: Commands,
    mut answers: MessageWriter<HarvestAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut gained: MessageWriter<XpGained>,
    mut ledger: Ledger,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let registries = &registries.0;
    let mut moved = false;
    let mut last = None;
    for (actor, request) in requests {
        let Ok((mut pack, _, at, mut sheet, name, riding, entity)) = players.get_mut(actor.0)
        else {
            request.no_actor(&mut commands);
            continue;
        };
        last = Some(actor.0);
        let here = [at.0.x(), at.0.z()];
        let outcome = match request {
            HarvestRequest::Plant => {
                let state = hand_state(
                    at,
                    None,
                    [riding, false, false, false],
                    source.as_deref(),
                    registries.resources.survey().ping.swim_m,
                );
                let covering = survey
                    .as_deref()
                    .map(|survey| concentration_here(survey, here[0], here[1]))
                    .unwrap_or_default();
                let default_sheet = CharacterSheet::default();
                can_plant(
                    registries,
                    state,
                    &pack,
                    sheet.as_deref().unwrap_or(&default_sheet),
                    &harvesters,
                    &covering,
                    &surveyed,
                    unkept.is_none(),
                    here,
                )
                .and_then(|plan| {
                    let next = without_machine(&registries.items, &pack, &plan.machine)
                        .ok_or(HarvestRefusal::NoHarvester)?;
                    let lot = crate::deposits::lot(&plan.deposit, &registries.resources, 1)
                        .ok_or(HarvestRefusal::NothingHere)?;
                    *pack = next;
                    moved = true;
                    ledger.planted(registries, &plan.deposit);
                    let id = harvesters.next;
                    harvesters.next += 1;
                    harvesters.list.push(Harvester {
                        id,
                        deposit: plan.deposit.id,
                        at: here,
                        pm: plan.pm,
                        item: plan.machine,
                        by: name.map_or_else(String::new, |name| name.as_str().to_owned()),
                        rate: plan.station.rate,
                        hopper: plan.station.hopper,
                        lot,
                        held: 0,
                        carry: 0,
                        full: false,
                    });
                    Ok(HarvestDone::Planted {
                        id,
                        deposit: plan.deposit.id,
                        pm: plan.pm,
                    })
                })
            }
            HarvestRequest::Collect { id } => can_collect(registries, &harvesters, &pack, id, here)
                .and_then(|units| {
                    let machine = harvesters
                        .list
                        .iter_mut()
                        .find(|each| each.id == id)
                        .ok_or(HarvestRefusal::NotInReach)?;
                    let record = Record {
                        count: units,
                        ..machine.lot.clone()
                    };
                    let next = with_lot(&registries.items, &pack, &record)
                        .ok_or(HarvestRefusal::NoRoom)?;
                    *pack = next;
                    moved = true;
                    machine.held -= units;
                    machine.full = machine.is_full();
                    let left = machine.held;
                    if let Some(deposit) = survey
                        .as_deref()
                        .and_then(|survey| survey.0.get(machine.deposit))
                    {
                        ledger.worked(registries, &deposit, units);
                    }
                    if let Some(sheet) = sheet.as_deref_mut().filter(|_| unkept.is_none()) {
                        let earned = survey_xp(
                            &registries.progression.rates.survey,
                            SurveyEvent::Harvest { units },
                        );
                        credit_survey(sheet, entity, earned, &mut gained);
                    }
                    Ok(HarvestDone::Collected { id, units, left })
                }),
            HarvestRequest::PickUp { id } => can_pick_up(registries, &harvesters, &pack, id, here)
                .and_then(|()| {
                    let index = harvesters
                        .list
                        .iter()
                        .position(|each| each.id == id)
                        .ok_or(HarvestRefusal::NotInReach)?;
                    let next = with_lot(&registries.items, &pack, &harvesters.list[index].item)
                        .ok_or(HarvestRefusal::NoRoom)?;
                    *pack = next;
                    moved = true;
                    harvesters.list.remove(index);
                    Ok(HarvestDone::PickedUp { id })
                }),
            HarvestRequest::Lead { id } => harvesters
                .get(id)
                .map(|machine| {
                    surveyed.lead = Some(Lead {
                        deposit: machine.deposit,
                        at: machine.at,
                        pm: machine.pm,
                    });
                    HarvestDone::Led { id }
                })
                .ok_or(HarvestRefusal::NotInReach),
        };
        answers.write(HarvestAnswer { outcome });
    }
    if moved
        && let Some(actor) = last
        && let Ok((pack, hands, ..)) = players.get(actor)
    {
        resync(&open, &store, pack, hands, &mut mirrors);
    }
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: the machines' tick, as
/// many nominal ticks as this step of the sim is worth. What it changes is
/// marked changed only when a unit was made or a hopper filled, which is
/// what the household saves on.
pub fn run(
    sim: Option<Res<SimClock>>,
    mut harvesters: ResMut<Harvesters>,
    mut notices: MessageWriter<HarvestNotice>,
    mut phase: Local<u32>,
) {
    if harvesters.list.is_empty() {
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
    let before: u32 = harvesters.list.iter().map(|each| each.held).sum();
    let mut filled = Vec::new();
    for _ in 0..nominal_ticks(&mut phase, stepped) {
        filled.extend(advance_ticks(harvesters.bypass_change_detection(), 1));
    }
    let after: u32 = harvesters.list.iter().map(|each| each.held).sum();
    if after != before || !filled.is_empty() {
        harvesters.set_changed();
    }
    for id in filled {
        if let Some(machine) = harvesters.get(id) {
            notices.write(HarvestNotice::Full {
                id,
                deposit: machine.deposit,
            });
        }
    }
}

/// Registers the harvesters and their requests, consumes them in the tick
/// and runs the machines. Added by `SimPlugin`, so a server and a harness
/// have it too; with no machine and no request, which is all either ever
/// has, it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct HarvestPlugin;

impl Plugin for HarvestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Harvesters>()
            .add_message::<HarvestNotice>()
            .add_request::<HarvestRequest, _>(RequestSet::Harvest, (consume, run).chain());
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use std::sync::Arc;

    use bevy::ecs::message::Messages;
    use strata_registry::item::STORE_GRADE;

    use strata_registry::Reading;

    use super::*;
    use crate::LocalPlayer;
    use crate::Player;
    use crate::combat::Combatant;
    use crate::deposits::Found;
    use crate::deposits::lot;
    use crate::items;
    use crate::items::Pack;
    use crate::tests::app_in_world;
    use bevy::app::App;

    use bevy::ecs::name::Name;

    use strata_registry::item::Record;

    use crate::deposits::Deposit;
    use crate::deposits::DepositId;
    use crate::deposits::HandState;

    use crate::deposits::SurveyUnkept;
    use crate::deposits::Surveyed;

    use crate::items::Hands;

    use crate::progression::CharacterSheet;

    use crate::Registries;

    use crate::WorldPosition;

    use crate::deposits::Survey;

    pub(crate) use veyra_rules::harvest::test_support::*;

    /// Every refusal of planting, in the order it is declared: each is
    /// cleared in turn and the next shows.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one table of the refusals in their order"
    )]
    fn planting_is_refused_in_order() {
        let registries = registries();
        let (deposit, surveyed) = field(&registries);
        let there = [1_000.0, 2_000.0];
        let carrying = pack_with(&registries, Some(&machine(STORE_GRADE)));
        let bare = pack_with(&registries, None);
        let (arrival, taught) = (CharacterSheet::default(), ilse(&registries));
        let rich = [(deposit.clone(), 800)];
        let thin = [(deposit.clone(), 100)];
        let mut crowded = Harvesters::default();
        crowded
            .list
            .push(planted(&registries, &deposit, [1_010.0, 2_000.0], 800, 0));
        let ask = |state: HandState,
                   pack: &Pack,
                   sheet: &CharacterSheet,
                   machines: &Harvesters,
                   covering: &[(Deposit, u16)],
                   found: &Surveyed| {
            can_plant(
                &registries.0,
                state,
                pack,
                sheet,
                machines,
                covering,
                found,
                true,
                there,
            )
            .map(|_| ())
        };
        let none = Harvesters::default();
        let all_bad = HandState {
            present: false,
            mounted: true,
            swimming: true,
            busy: false,
        };
        let nothing = Surveyed::default();
        assert_eq!(
            ask(all_bad, &bare, &arrival, &crowded, &thin, &nothing),
            Err(HarvestRefusal::NoPlayer)
        );
        let state = HandState {
            present: true,
            ..all_bad
        };
        assert_eq!(
            ask(state, &bare, &arrival, &crowded, &thin, &nothing),
            Err(HarvestRefusal::Mounted)
        );
        let state = HandState {
            mounted: false,
            ..state
        };
        assert_eq!(
            ask(state, &bare, &arrival, &crowded, &thin, &nothing),
            Err(HarvestRefusal::InWater)
        );
        let state = standing();
        assert_eq!(
            ask(state, &bare, &arrival, &crowded, &thin, &nothing),
            Err(HarvestRefusal::NoHarvester)
        );
        assert_eq!(
            ask(state, &carrying, &arrival, &crowded, &thin, &nothing),
            Err(HarvestRefusal::NotTaught)
        );
        assert_eq!(
            ask(state, &carrying, &taught, &crowded, &thin, &nothing),
            Err(HarvestRefusal::AtLimit { count: 1 })
        );
        assert_eq!(
            ask(state, &carrying, &taught, &none, &thin, &nothing),
            Err(HarvestRefusal::NothingHere),
            "a deposit not found, and one found but too thin"
        );
        assert_eq!(
            ask(state, &carrying, &taught, &none, &thin, &surveyed),
            Err(HarvestRefusal::NothingHere)
        );
        assert_eq!(
            ask(state, &carrying, &taught, &none, &rich, &nothing),
            Err(HarvestRefusal::NothingHere)
        );
        // Two boxes allow two, and the second stands too near the first.
        let two = sheet_of(
            &registries,
            &[
                "surveyor.novice",
                "surveyor.stations.1",
                "surveyor.stations.2",
                "surveyor.stations.3",
            ],
        );
        assert_eq!(
            ask(state, &carrying, &two, &crowded, &rich, &surveyed),
            Err(HarvestRefusal::TooNear)
        );
        let plan = can_plant(
            &registries.0,
            state,
            &carrying,
            &taught,
            &none,
            &rich,
            &surveyed,
            true,
            there,
        )
        .unwrap();
        assert_eq!(
            can_plant(
                &registries.0,
                state,
                &carrying,
                &taught,
                &none,
                &rich,
                &surveyed,
                false,
                there,
            )
            .map(|_| ()),
            Err(HarvestRefusal::NothingHere),
            "a record this world does not keep"
        );
        assert_eq!((plan.pm, plan.deposit.id), (800, deposit.id));
        assert_eq!(
            (plan.station.count, plan.station.rate, plan.station.hopper),
            (1, 1200, 1000)
        );
    }

    /// A player standing on the nearest deposit's centre, carrying
    /// `carried`, who has found it and knows `sheet`.
    fn playing(carried: Option<&Record>, sheet: CharacterSheet) -> (App, Entity, Deposit) {
        let (survey, deposit) = world();
        let mut app = app_in_world();
        let registries = registries();
        let pack = pack_with(&registries, carried);
        app.insert_resource(registries)
            .insert_resource(Survey(Arc::clone(survey)));
        app.world_mut().resource_mut::<Surveyed>().found.insert(
            deposit.id,
            Found {
                reading: Reading::Exact,
                sampled: true,
                best_pm: deposit.peak,
                best_at: deposit.at,
            },
        );
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                LocalPlayer,
                pack,
                Hands::default(),
                WorldPosition::new(deposit.at[0], 0.0, deposit.at[1]),
                Combatant::default(),
                sheet,
                Name::new("Ilse Marrow"),
            ))
            .id();
        app.update();
        (app, player, deposit.clone())
    }

    /// What the sim answered over some ticks.
    fn ask(app: &mut App, request: HarvestRequest) -> Result<HarvestDone, HarvestRefusal> {
        app.world_mut().resource_mut::<HarvestInbox>().push(request);
        app.update();
        let mut heard: Vec<HarvestAnswer> = app
            .world_mut()
            .resource_mut::<Messages<HarvestAnswer>>()
            .drain()
            .collect();
        assert_eq!(heard.len(), 1);
        heard.remove(0).outcome
    }

    /// Plant, run an hour of played time, collect and pick up, the whole of
    /// a machine's life through the sim's requests, with the XP and the
    /// notice.
    #[test]
    fn a_machine_is_planted_fills_is_collected_and_picked_up() {
        let (mut app, player, deposit) = playing(Some(&machine(STORE_GRADE)), ilse(&registries()));
        let planted = ask(&mut app, HarvestRequest::Plant).unwrap();
        let HarvestDone::Planted { id, pm, .. } = planted else {
            panic!("{planted:?}");
        };
        assert_eq!(pm, deposit.peak, "at the centre");
        {
            let machines = app.world().resource::<Harvesters>();
            let one = machines.get(id).unwrap();
            assert_eq!(
                (one.rate, one.hopper, one.by.as_str()),
                (1200, 1000, "Ilse Marrow")
            );
            assert_eq!(one.lot.origin.as_ref().unwrap().deposit, deposit.id.0);
        }
        let registries = app.world().resource::<Registries>().clone();
        // The machine left the pack, and a second is refused for want of one.
        let pack = app.world().get::<Pack>(player).unwrap().clone();
        assert!(best_machine(&registries.0.items, &pack).is_none());
        assert_eq!(
            ask(&mut app, HarvestRequest::Plant),
            Err(HarvestRefusal::NoHarvester)
        );
        // Nothing yet to collect or pick up but the machine itself.
        assert_eq!(
            ask(&mut app, HarvestRequest::Collect { id }),
            Err(HarvestRefusal::Empty)
        );
        // Six minutes of played time, a tick an update: 7200 updates is
        // too many to step, so the clock is the machine's own ticks. (A
        // longer wait would hold more than the pack has room for.)
        let ticks = u32::try_from(ticks_per_hour()).unwrap() / 10;
        {
            let mut machines = app.world_mut().resource_mut::<Harvesters>();
            advance_ticks(&mut machines, ticks);
        }
        let held = app.world().resource::<Harvesters>().get(id).unwrap().held;
        assert_eq!(
            held,
            u32::from(pm) * 1200 / 10_000,
            "six minutes at its rate"
        );
        assert_eq!(
            ask(&mut app, HarvestRequest::PickUp { id }),
            Err(HarvestRefusal::NotEmpty)
        );
        // A hand sample of the same deposit is in the pack already, and the
        // collected lot stacks with it.
        {
            let sample = lot(&deposit, &registries.0.resources, 5).unwrap();
            let items = &registries.0.items;
            let mut pack = app.world_mut().get_mut::<Pack>(player).unwrap();
            let stack = items::write(&sample, items).unwrap();
            assert!(items::place_in(items, &mut pack.cells, stack).is_none());
        }
        let xp_before = app
            .world()
            .get::<CharacterSheet>(player)
            .unwrap()
            .xp(strata_registry::progression::XpKind::Survey);
        let collected = ask(&mut app, HarvestRequest::Collect { id }).unwrap();
        assert_eq!(
            collected,
            HarvestDone::Collected {
                id,
                units: held,
                left: 0
            }
        );
        let pack = app.world().get::<Pack>(player).unwrap();
        let (units, stacks) = units_of(&registries, pack, &deposit);
        assert_eq!((units, stacks), (held + 5, 1), "one stack of the deposit");
        // Ten units a point of Survey XP.
        let xp = app
            .world()
            .get::<CharacterSheet>(player)
            .unwrap()
            .xp(strata_registry::progression::XpKind::Survey);
        let rates = &registries.0.progression.rates.survey;
        assert_eq!(xp - xp_before, held / 10 * rates.harvest);
        // Empty, it is picked up, and the pack has the machine back.
        assert_eq!(
            ask(&mut app, HarvestRequest::PickUp { id }),
            Ok(HarvestDone::PickedUp { id })
        );
        assert!(app.world().resource::<Harvesters>().list.is_empty());
        let pack = app.world().get::<Pack>(player).unwrap();
        assert_eq!(
            best_machine(&registries.0.items, pack).unwrap(),
            machine(STORE_GRADE)
        );
        // Out of reach is out of reach.
        assert_eq!(
            ask(&mut app, HarvestRequest::Collect { id }),
            Err(HarvestRefusal::NotInReach)
        );
    }

    /// Where the household's record is not kept, nothing is planted: the
    /// machine would be lost on quit, so the pack keeps it.
    #[test]
    fn nothing_is_planted_where_the_record_is_not_kept() {
        let (mut app, player, _) = playing(Some(&machine(STORE_GRADE)), ilse(&registries()));
        app.insert_resource(SurveyUnkept);
        let before = app.world().get::<Pack>(player).unwrap().clone();
        assert_eq!(
            ask(&mut app, HarvestRequest::Plant),
            Err(HarvestRefusal::NothingHere)
        );
        assert!(app.world().resource::<Harvesters>().list.is_empty());
        assert_eq!(*app.world().get::<Pack>(player).unwrap(), before);
    }

    /// A harvester that was planted from the world's own tick fills on the
    /// sim's clock, says it is full once and keeps the lot's lines.
    #[test]
    fn the_sims_tick_runs_a_machine_and_tells_when_it_is_full() {
        let (mut app, _, deposit) = playing(None, CharacterSheet::default());
        let registries = app.world().resource::<Registries>().clone();
        let mut one = planted(&registries, &deposit, deposit.at, 1000, 0);
        one.hopper = 3;
        one.rate = 72_000; // a unit a nominal tick... in an hour of 72000
        app.world_mut().resource_mut::<Harvesters>().list.push(one);
        let mut told = Vec::new();
        for _ in 0..10 {
            app.update();
            told.extend(
                app.world_mut()
                    .resource_mut::<Messages<HarvestNotice>>()
                    .drain(),
            );
        }
        assert_eq!(
            told,
            [HarvestNotice::Full {
                id: 0,
                deposit: deposit.id
            }]
        );
        let machines = app.world().resource::<Harvesters>();
        assert_eq!(machines.list[0].held, 3);
        assert_eq!(
            machines.holding(deposit.id),
            Some(Holding {
                count: 1,
                held: 3,
                hopper: 3,
                full: true
            })
        );
        assert_eq!(machines.holding(DepositId([9, 9])), None);
    }

    /// Collect takes what fits and leaves the rest, and with no room at all
    /// takes nothing.
    #[test]
    fn collect_takes_what_fits_and_leaves_the_rest() {
        let (mut app, player, deposit) = playing(None, CharacterSheet::default());
        let registries = app.world().resource::<Registries>().clone();
        let (items, set) = (&registries.0.items, &registries.0.resources);
        let mut one = planted(&registries, &deposit, deposit.at, 1000, 0);
        one.hopper = 1_000;
        one.held = 1_000;
        app.world_mut().resource_mut::<Harvesters>().list.push(one);
        let lot_stack = items
            .get(&lot(&deposit, set, 1).unwrap().item)
            .unwrap()
            .stack;
        // Every cell holds another deposit's full stack but the last one.
        let other = made(set, [9, 9], [5.0, 5.0], 500);
        let filler = items::write(
            &Record {
                count: lot_stack,
                ..lot(&other, set, 1).unwrap()
            },
            items,
        )
        .unwrap();
        let fill = |app: &mut App, spare: usize| {
            let mut pack = app.world_mut().get_mut::<Pack>(player).unwrap();
            let mut at: Vec<(Option<usize>, usize)> =
                (0..pack.cells.len()).map(|cell| (None, cell)).collect();
            for bag in 0..pack.bags.len() {
                at.extend(
                    (0..usize::from(pack.open_cells(bag, items))).map(|cell| (Some(bag), cell)),
                );
            }
            let total = at.len();
            for (index, (bag, cell)) in at.into_iter().enumerate() {
                let inventory = match bag {
                    None => &mut pack.cells,
                    Some(bag) => &mut pack.bags[bag],
                };
                inventory.set(cell, (index + spare < total).then(|| filler.clone()));
            }
        };
        fill(&mut app, 1);
        let done = ask(&mut app, HarvestRequest::Collect { id: 0 }).unwrap();
        assert_eq!(
            done,
            HarvestDone::Collected {
                id: 0,
                units: lot_stack,
                left: 1_000 - lot_stack
            }
        );
        assert_eq!(
            app.world().resource::<Harvesters>().list[0].held,
            1_000 - lot_stack
        );
        // No room left: nothing is taken, and nothing changes.
        assert_eq!(
            ask(&mut app, HarvestRequest::Collect { id: 0 }),
            Err(HarvestRefusal::NoRoom)
        );
        assert_eq!(
            app.world().resource::<Harvesters>().list[0].held,
            1_000 - lot_stack
        );
        // And a machine to pick up needs room for it too.
        app.world_mut().resource_mut::<Harvesters>().list[0].held = 0;
        assert_eq!(
            ask(&mut app, HarvestRequest::PickUp { id: 0 }),
            Err(HarvestRefusal::NoRoom)
        );
        // Far from it is out of reach, whatever it holds.
        app.world_mut().get_mut::<WorldPosition>(player).unwrap().0 =
            strata_core::WorldPos::new(deposit.at[0] + 50.0, 0.0, deposit.at[1]);
        assert_eq!(
            ask(&mut app, HarvestRequest::PickUp { id: 0 }),
            Err(HarvestRefusal::NotInReach)
        );
    }

    /// A lead points the HUD's one marker at a machine.
    #[test]
    fn a_machine_can_be_led_to() {
        let (mut app, _, deposit) = playing(None, CharacterSheet::default());
        let registries = app.world().resource::<Registries>().clone();
        app.world_mut()
            .resource_mut::<Harvesters>()
            .list
            .push(planted(&registries, &deposit, [10.0, 20.0], 640, 0));
        assert_eq!(
            ask(&mut app, HarvestRequest::Lead { id: 0 }),
            Ok(HarvestDone::Led { id: 0 })
        );
        let lead = app.world().resource::<Surveyed>().lead.unwrap();
        assert_eq!(
            (lead.at, lead.pm, lead.deposit),
            ([10.0, 20.0], 640, deposit.id)
        );
        assert_eq!(
            ask(&mut app, HarvestRequest::Lead { id: 7 }),
            Err(HarvestRefusal::NotInReach)
        );
    }
}
