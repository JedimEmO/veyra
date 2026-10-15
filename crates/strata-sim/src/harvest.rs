//! The field harvester (ticket 0009, step 6): a machine a surveyor plants on
//! ground the household has found, which fills over played time and is
//! emptied by hand.
//!
//! [`can_plant`] is the one rule for planting, asked by the request and by
//! the screen that greys its button; the planter's boxes and the machine's
//! lines are worked out once, at the planting, by [`station`], and kept with
//! the machine, as the factory keeps a job's pace. The concentration where
//! it stands is read then too: the ground does not change, so the tick
//! samples no world.
//!
//! Time is the factory's: the tick runs in nominal ticks
//! ([`crate::factory::nominal_ticks`]), so a machine gives the same units
//! whatever rate the sim is stepped at. Each tick adds the rate times the
//! concentration to a carry in whole numbers, and a unit is made when the
//! carry reaches an hour's worth of ticks at a thousand, so an hour at 1000
//! per-mille is exactly the rate. A full hopper stops it ([`HarvestNotice`]
//! says so once). A lot it gives is [`lot`](crate::deposits::lot)'s, so it
//! stacks with what was sampled by hand at the same deposit.
//!
//! Plant, Collect, `PickUp` and Lead are requests in [`HarvestInbox`],
//! consumed in `SimSet::Input` after the survey's and answered with a
//! [`HarvestAnswer`]. Local-only, like the stock: nothing here is
//! replicated, and the game saves [`Harvesters`] with the household.

use bevy::app::{App, Plugin};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::name::Name;
use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_core::SIM_TICK_HZ;
use strata_registry::item::{HARVEST_HOPPER, HARVEST_RATE, ItemCatalogue, ItemKind, Record};
use strata_registry::progression::SkillGraph;
use strata_registry::{GameRegistries, HarvestRules};

use crate::crafting::kind_of;
use crate::deeds::Ledger;
use crate::deposits::{
    Deposit, DepositId, HandState, Lead, SurveyEvent, SurveyUnkept, Surveyed, concentration_here,
    credit_survey, hand_state, resync, survey_xp, with_lot,
};
use crate::equip::{Cell, find};
use crate::factory::nominal_ticks;
use crate::items::{Hands, HouseholdStore, OpenStock, Pack, StockAnswer};
use crate::progression::{CharacterSheet, XpGained, station_field};
use crate::ride::Riding;
use crate::{
    LocalPlayer, Registries, SimClock, SimSet, SimTick, WorldPosition, WorldSourceRes,
    deposits::Survey,
};

/// What a planter's boxes and a machine's lines make of the dials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Station {
    /// How many the household may have planted: nought without a Stations
    /// box.
    pub count: u32,
    /// Units a played hour at 1000 per-mille.
    pub rate: u32,
    /// Units it holds.
    pub hopper: u32,
}

/// The dials as `sheet`'s boxes and `machine`'s lines make them: the file's
/// figure, then the planter's `Station` effects (Set, Add, Mul), then the
/// machine's line as a factor. A sheet with no box on a field reads the dial
/// untouched. Whole units, rounded.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "dials and boxes are small whole numbers, rounded and floored at nought"
)]
pub fn station(
    rules: &HarvestRules,
    items: &ItemCatalogue,
    machine: &Record,
    graph: &SkillGraph,
    sheet: &CharacterSheet,
) -> Station {
    let line = |id: &str| {
        machine
            .lines
            .get(id)
            .copied()
            .unwrap_or(strata_registry::item::STORE_GRADE)
    };
    let (rate_factor, hopper_factor) = items.get(&machine.item).map_or((1.0, 1.0), |def| {
        def.kind
            .harvester_factors(line(HARVEST_RATE), line(HARVEST_HOPPER))
    });
    let worked = |name: &str, dial: u32, factor: f64| {
        let boxed = f64::from(station_field(graph, sheet, name, dial as f32));
        (boxed * factor).round().max(0.0) as u32
    };
    Station {
        count: count_of(graph, sheet),
        rate: worked("rate", rules.rate, rate_factor),
        hopper: worked("hopper", rules.hopper, hopper_factor),
    }
}

/// How many harvesters `sheet` may have planted: the boxes' `count` over
/// nought.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a few boxes, floored at nought"
)]
pub fn count_of(graph: &SkillGraph, sheet: &CharacterSheet) -> u32 {
    station_field(graph, sheet, "count", 0.0).round().max(0.0) as u32
}

/// One planted machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Harvester {
    /// Its number, from [`Harvesters::next`].
    pub id: u32,
    /// The deposit it stands on.
    pub deposit: DepositId,
    /// Where, world `(x, z)`.
    pub at: [f64; 2],
    /// The concentration at that point, per-mille, read at the planting.
    pub pm: u16,
    /// The machine as it was carried, which a pick-up gives back.
    pub item: Record,
    /// Who planted it, by name.
    pub by: String,
    /// Units a played hour at 1000 per-mille, kept from the planting.
    pub rate: u32,
    /// Units it holds at most, kept from the planting.
    pub hopper: u32,
    /// One unit of what it holds: the deposit's lot, the lines and origin
    /// exactly, for however many are collected.
    pub lot: Record,
    /// Units in the hopper.
    pub held: u32,
    /// The part of a unit made, in the rate's own terms (see `unit_cost`).
    #[serde(default)]
    pub carry: u64,
    /// Whether it has said it is full.
    #[serde(default)]
    pub full: bool,
}

impl Harvester {
    /// Whether the hopper is at its mark.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.held >= self.hopper
    }

    /// Units a played hour makes at its point, rounded: what the screen
    /// shows beside the rate the file names at 1000 per-mille.
    #[must_use]
    pub fn per_hour(&self) -> u32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a rate times a per-mille over a thousand"
        )]
        {
            ((u64::from(self.rate) * u64::from(self.pm) + 500) / 1000) as u32
        }
    }

    /// Played minutes until the hopper is full, rounded up; `None` when it
    /// is full or makes nothing.
    #[must_use]
    pub fn minutes_to_full(&self) -> Option<u32> {
        let per_hour = u64::from(self.rate) * u64::from(self.pm);
        let left = u64::from(self.hopper.saturating_sub(self.held));
        if per_hour == 0 || left == 0 {
            return None;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "minutes of a few hundred units"
        )]
        Some((left * 60_000).div_ceil(per_hour) as u32)
    }
}

/// The planted machines of the household.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Harvesters {
    /// Every machine, in the order planted.
    #[serde(default)]
    pub list: Vec<Harvester>,
    /// The number the next one gets, never reused.
    #[serde(default)]
    pub next: u32,
}

impl Harvesters {
    /// The machine `id`.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Harvester> {
        self.list.iter().find(|each| each.id == id)
    }

    /// What the machines on `deposit` hold, or `None` with none there: the
    /// register's row says it.
    #[must_use]
    pub fn holding(&self, deposit: DepositId) -> Option<Holding> {
        let mut on = self.list.iter().filter(|each| each.deposit == deposit);
        let first = on.next()?;
        let mut holding = Holding {
            count: 1,
            held: first.held,
            hopper: first.hopper,
            full: first.is_full(),
        };
        for each in on {
            holding.count += 1;
            holding.held += each.held;
            holding.hopper += each.hopper;
            holding.full &= each.is_full();
        }
        Some(holding)
    }
}

/// What the harvesters on one deposit hold together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holding {
    /// How many machines stand on it.
    pub count: u32,
    /// Units held in all of them.
    pub held: u32,
    /// Units they hold at most.
    pub hopper: u32,
    /// Whether every one is full.
    pub full: bool,
}

/// Nominal ticks in a played hour.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the sim's rate, twenty"
)]
fn ticks_per_hour() -> u64 {
    SIM_TICK_HZ.round() as u64 * 3600
}

/// The carry a unit costs: an hour's ticks at 1000 per-mille, so a tick at a
/// rate and a concentration adds `rate * pm` and an hour at the rate's own
/// 1000 per-mille makes exactly `rate` units.
fn unit_cost() -> u64 {
    ticks_per_hour() * 1000
}

/// One nominal tick of `machine`: whether it became full by it.
fn tick_one(machine: &mut Harvester) -> bool {
    if machine.is_full() {
        return false;
    }
    machine.carry += u64::from(machine.rate) * u64::from(machine.pm);
    let cost = unit_cost();
    let made = machine.carry / cost;
    machine.carry %= cost;
    let room = u64::from(machine.hopper - machine.held);
    let kept = made.min(room);
    #[allow(clippy::cast_possible_truncation, reason = "at most the hopper, a u32")]
    {
        machine.held += kept as u32;
    }
    if machine.is_full() {
        // A part of a unit has nowhere to go in a full hopper.
        machine.carry = 0;
        let said = machine.full;
        machine.full = true;
        return !said;
    }
    false
}

/// The units a machine of `rate` at `pm` per-mille has made after `played_s`
/// played seconds, at most its `hopper`: the same arithmetic as the tick, for
/// a model that asks what a stretch of time gave without running the ticks.
#[must_use]
pub fn made_in(rate: u32, pm: u16, hopper: u32, played_s: u32) -> u32 {
    let made = u64::from(rate) * u64::from(pm) * u64::from(played_s) / (3600 * 1000);
    u32::try_from(made.min(u64::from(hopper))).unwrap_or(hopper)
}

/// Every machine over `ticks` nominal ticks; the ids of those that became
/// full by it.
pub fn advance_ticks(harvesters: &mut Harvesters, ticks: u32) -> Vec<u32> {
    let mut filled = Vec::new();
    for machine in &mut harvesters.list {
        for _ in 0..ticks {
            if tick_one(machine) {
                filled.push(machine.id);
            }
        }
    }
    filled
}

/// Why a request was refused. The words a player reads are the screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HarvestRefusal {
    /// There is no local player.
    #[error("nobody is planting")]
    NoPlayer,
    /// On the bike.
    #[error("not from the saddle")]
    Mounted,
    /// Swimming.
    #[error("not from the water")]
    InWater,
    /// No harvester is carried.
    #[error("there is no field harvester in the pack")]
    NoHarvester,
    /// The member has no Stations box.
    #[error("no box teaches planting")]
    NotTaught,
    /// The household has as many planted as this member may.
    #[error("{count} planted already")]
    AtLimit {
        /// How many this member may have.
        count: u32,
    },
    /// No deposit the household has found reads enough here.
    #[error("no found deposit reads enough here")]
    NothingHere,
    /// Another stands too near.
    #[error("another harvester stands too near")]
    TooNear,
    /// The machine is not within reach, or there is none of that number.
    #[error("not within reach of it")]
    NotInReach,
    /// Nothing in the hopper.
    #[error("it holds nothing")]
    Empty,
    /// Something in the hopper, which a pick-up would lose.
    #[error("it still holds something")]
    NotEmpty,
    /// The pack and its bags have no room.
    #[error("there is no room in the pack")]
    NoRoom,
}

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
#[derive(Resource, Debug, Default)]
pub struct HarvestInbox(pub Vec<HarvestRequest>);

/// The sim's answer to a [`HarvestRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct HarvestAnswer {
    /// What happened, or why nothing did.
    pub outcome: Result<HarvestDone, HarvestRefusal>,
}

/// Something a machine did on its own.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarvestNotice {
    /// A hopper reached its mark and the machine stopped.
    Full {
        /// The machine.
        id: u32,
        /// The deposit it stands on.
        deposit: DepositId,
    },
}

/// What planting would do, if allowed.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The deposit.
    pub deposit: Deposit,
    /// The concentration, per-mille.
    pub pm: u16,
    /// The machine that would leave the pack.
    pub machine: Record,
    /// The figures it would be planted with.
    pub station: Station,
}

/// The carried harvester with the best lines, in the pack or an open bag.
#[must_use]
pub fn best_machine(items: &ItemCatalogue, pack: &Pack) -> Option<Record> {
    crate::crafting::reachable(items, pack, None)
        .into_iter()
        .map(|(record, _)| record)
        .filter(|record| {
            matches!(
                items.get(&record.item).map(|def| &def.kind),
                Some(ItemKind::Harvester { .. })
            )
        })
        .max_by_key(|record| {
            let line = |id: &str| u32::from(record.lines.get(id).copied().unwrap_or(0));
            line(HARVEST_RATE) + line(HARVEST_HOPPER)
        })
        .map(|record| kind_of(&record))
}

/// Whether the player can plant where they stand, from `covering` (every
/// deposit holding the spot with its concentration), and what it would be.
///
/// # Errors
/// The first of: no player, the saddle, swimming, no harvester carried, no
/// box, as many planted as the member's count, nothing found here that
/// reads enough (or a survey record this world does not keep), another
/// machine too near.
#[allow(
    clippy::too_many_arguments,
    reason = "the member, the pack, the household's machines and record, and the spot: all the rule reads"
)]
pub fn can_plant(
    registries: &GameRegistries,
    state: HandState,
    pack: &Pack,
    sheet: &CharacterSheet,
    harvesters: &Harvesters,
    covering: &[(Deposit, u16)],
    surveyed: &Surveyed,
    kept: bool,
    at: [f64; 2],
) -> Result<Plan, HarvestRefusal> {
    if !state.present {
        return Err(HarvestRefusal::NoPlayer);
    }
    if state.mounted {
        return Err(HarvestRefusal::Mounted);
    }
    if state.swimming {
        return Err(HarvestRefusal::InWater);
    }
    let rules = registries.resources.harvest();
    let machine = best_machine(&registries.items, pack).ok_or(HarvestRefusal::NoHarvester)?;
    let made = station(
        rules,
        &registries.items,
        &machine,
        &registries.progression,
        sheet,
    );
    if made.count == 0 {
        return Err(HarvestRefusal::NotTaught);
    }
    if harvesters.list.len() >= made.count as usize {
        return Err(HarvestRefusal::AtLimit { count: made.count });
    }
    // A record not kept in this world is never written, so a machine
    // planted against it would be lost on quit.
    let (deposit, pm) = covering
        .iter()
        .filter(|(deposit, pm)| {
            kept && *pm >= rules.min_pm && surveyed.found.contains_key(&deposit.id)
        })
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.id.cmp(&a.0.id)))
        .ok_or(HarvestRefusal::NothingHere)?;
    if harvesters
        .list
        .iter()
        .any(|each| distance(each.at, at) < rules.apart_m)
    {
        return Err(HarvestRefusal::TooNear);
    }
    Ok(Plan {
        deposit: deposit.clone(),
        pm: *pm,
        machine,
        station: made,
    })
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Whether the player at `at` can reach the machine `id`: it is there and
/// within the dials' reach.
///
/// # Errors
/// [`HarvestRefusal::NotInReach`].
pub fn in_reach<'a>(
    rules: &HarvestRules,
    harvesters: &'a Harvesters,
    id: u32,
    at: [f64; 2],
) -> Result<&'a Harvester, HarvestRefusal> {
    harvesters
        .get(id)
        .filter(|machine| distance(machine.at, at) <= rules.reach_m)
        .ok_or(HarvestRefusal::NotInReach)
}

/// How many units of `machine` the pack can take now, up to what it holds:
/// the largest lot the pack's rule puts away. Nought is no room.
#[must_use]
pub fn room_for(items: &ItemCatalogue, pack: &Pack, machine: &Harvester) -> u32 {
    let fits = |units: u32| {
        let record = Record {
            count: units,
            ..machine.lot.clone()
        };
        with_lot(items, pack, &record).is_some()
    };
    // Whether a lot fits only falls as the lot grows, so the largest that
    // does is found by halving.
    let (mut low, mut high) = (0, machine.held);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if fits(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// Whether the machine `id` can be collected from `at`, and how many units
/// go into the pack.
///
/// # Errors
/// The first of: not in reach, empty, no room for even one unit.
pub fn can_collect(
    registries: &GameRegistries,
    harvesters: &Harvesters,
    pack: &Pack,
    id: u32,
    at: [f64; 2],
) -> Result<u32, HarvestRefusal> {
    let machine = in_reach(registries.resources.harvest(), harvesters, id, at)?;
    if machine.held == 0 {
        return Err(HarvestRefusal::Empty);
    }
    match room_for(&registries.items, pack, machine) {
        0 => Err(HarvestRefusal::NoRoom),
        units => Ok(units),
    }
}

/// Whether the machine `id` can be picked up from `at`.
///
/// # Errors
/// The first of: not in reach, something in the hopper, no room for it.
pub fn can_pick_up(
    registries: &GameRegistries,
    harvesters: &Harvesters,
    pack: &Pack,
    id: u32,
    at: [f64; 2],
) -> Result<(), HarvestRefusal> {
    let machine = in_reach(registries.resources.harvest(), harvesters, id, at)?;
    if machine.held > 0 {
        return Err(HarvestRefusal::NotEmpty);
    }
    with_lot(&registries.items, pack, &machine.item)
        .map(|_| ())
        .ok_or(HarvestRefusal::NoRoom)
}

/// The pack with the one carried `machine` taken out of it, or `None` when
/// it is not there.
fn without_machine(items: &ItemCatalogue, pack: &Pack, machine: &Record) -> Option<Pack> {
    let mut next = pack.clone();
    let cell = find(items, &next, None, &kind_of(machine))?;
    let inventory = match cell {
        Cell::Pack(_) => &mut next.cells,
        Cell::Bag(bag, _) => &mut next.bags[bag],
        Cell::Store(_) => return None,
    };
    // A harvester stacks to one.
    inventory.take(cell.index())?;
    inventory.clear_changed();
    Some(next)
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
    mut players: Query<PlayerRow, With<LocalPlayer>>,
    mut answers: MessageWriter<HarvestAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
    mut gained: MessageWriter<XpGained>,
    mut ledger: Ledger,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let registries = &registries.0;
    let Ok((mut pack, hands, at, mut sheet, name, riding, entity)) = players.single_mut() else {
        for _ in requests {
            answers.write(HarvestAnswer {
                outcome: Err(HarvestRefusal::NoPlayer),
            });
        }
        return;
    };
    let here = [at.0.x(), at.0.z()];
    let mut moved = false;
    for request in requests {
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
    if moved {
        resync(&open, &store, &pack, hands, &mut mirrors);
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
            .init_resource::<HarvestInbox>()
            .add_message::<HarvestAnswer>()
            .add_message::<HarvestNotice>()
            .add_systems(
                SimTick,
                (consume, run)
                    .chain()
                    .in_set(SimSet::Input)
                    .after(crate::deposits::consume),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use std::collections::BTreeMap;
    use std::sync::{Arc, OnceLock};

    use bevy::ecs::message::Messages;
    use strata_registry::item::{STORE_GRADE, SavedPack};
    use strata_registry::{Namespaced, Reading, ResourceSet};
    use strata_worldgen::deposits::DepositField;

    use super::*;
    use crate::Player;
    use crate::combat::Combatant;
    use crate::deposits::{Found, lot};
    use crate::items::{self, Pack};
    use crate::tests::app_in_world;

    const HARVESTER: &str = "veyra:station.field_harvester";

    fn registries() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    /// A harvester record with both lines at `line`.
    fn machine(line: u16) -> Record {
        Record {
            item: id(HARVESTER),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                (HARVEST_RATE.to_owned(), line),
                (HARVEST_HOPPER.to_owned(), line),
            ]),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    /// A salt deposit made by hand, `at` its centre with `peak` there.
    fn made(set: &ResourceSet, cell: [i32; 2], at: [f64; 2], peak: u16) -> Deposit {
        let ty = set.type_id(&id("strata:evaporite_salt")).unwrap();
        let def = set.get(ty).unwrap();
        Deposit {
            id: DepositId(cell),
            ty,
            root: "Calder".to_owned(),
            place: format!("Calder {}", def.grounds[0]),
            at,
            height_m: 10.0,
            figures: set
                .class_of(ty)
                .carries
                .iter()
                .map(|attribute| (*attribute, def.ranges[attribute].floor + 100))
                .collect(),
            radius_m: 200.0,
            peak,
            grade_pm: 500,
        }
    }

    fn sheet_of(registries: &Registries, boxes: &[&str]) -> CharacterSheet {
        let _ = registries;
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    fn ilse(registries: &Registries) -> CharacterSheet {
        let household = registries.0.sheets.household.as_ref().unwrap();
        let member = household
            .recruits
            .iter()
            .find(|member| member.name == "Ilse Marrow")
            .unwrap();
        CharacterSheet::authored(&member.sheet)
    }

    fn pack_with(registries: &Registries, record: Option<&Record>) -> Pack {
        let items = &registries.0.items;
        let mut pack = Pack::from_saved(&SavedPack::default(), items).unwrap();
        if let Some(record) = record {
            let stack = items::write(record, items).unwrap();
            assert!(items::place_in(items, &mut pack.cells, stack).is_none());
        }
        pack
    }

    /// The figures for the tests that plant by hand: a salt deposit, found.
    fn field(registries: &Registries) -> (Deposit, Surveyed) {
        let deposit = made(&registries.0.resources, [1, 1], [1_000.0, 2_000.0], 800);
        let mut surveyed = Surveyed::default();
        surveyed.found.insert(
            deposit.id,
            Found {
                reading: Reading::Exact,
                sampled: true,
                best_pm: 800,
                best_at: deposit.at,
            },
        );
        (deposit, surveyed)
    }

    fn standing() -> HandState {
        HandState {
            present: true,
            ..HandState::default()
        }
    }

    /// A machine planted by hand on `deposit` at `pm`, store grade, with
    /// a hopper of `hopper`.
    fn planted(
        registries: &Registries,
        deposit: &Deposit,
        at: [f64; 2],
        pm: u16,
        held: u32,
    ) -> Harvester {
        let item = machine(STORE_GRADE);
        let rules = registries.0.resources.harvest();
        Harvester {
            id: 0,
            deposit: deposit.id,
            at,
            pm,
            item,
            by: "Ilse Marrow".to_owned(),
            rate: rules.rate,
            hopper: rules.hopper,
            lot: lot(deposit, &registries.0.resources, 1).unwrap(),
            held,
            carry: 0,
            full: false,
        }
    }

    /// The dials, the machine's lines and the planter's boxes, each worked
    /// out once: no box, Ilse Marrow and a whole Stations branch, at lines
    /// 0, 500 and 1000.
    #[test]
    fn a_station_is_the_dial_the_boxes_and_the_lines() {
        let registries = registries();
        let rules = registries.0.resources.harvest();
        let (items, graph) = (&registries.0.items, &registries.0.progression);
        let read = |sheet: &CharacterSheet| {
            [0, 500, 1000].map(|line| {
                let made = station(rules, items, &machine(line), graph, sheet);
                (made.count, made.rate, made.hopper)
            })
        };
        // Nobody with no box may plant, and the lines scale the dials:
        // 800 to 1250 for the rate, 800 to 1500 for the hopper.
        assert_eq!(
            read(&CharacterSheet::default()),
            [(0, 960, 800), (0, 1200, 1000), (0, 1500, 1500)]
        );
        // Ilse knows Stations 1: one machine, the dials untouched.
        assert_eq!(
            read(&ilse(&registries)),
            [(1, 960, 800), (1, 1200, 1000), (1, 1500, 1500)]
        );
        // The whole branch: three, a quarter faster, half as much again.
        let branch: Vec<&str> = graph
            .boxes()
            .filter(|b| b.id.starts_with("surveyor.stations."))
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(branch.len(), 4);
        assert_eq!(
            read(&sheet_of(&registries, &branch)),
            [(3, 1200, 1200), (3, 1500, 1500), (3, 1875, 2250)]
        );
        // The count is the boxes' alone, with no machine in hand.
        assert_eq!(count_of(graph, &CharacterSheet::default()), 0);
        assert_eq!(count_of(graph, &sheet_of(&registries, &branch)), 3);
    }

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

    /// An hour of nominal ticks gives the rate times the concentration,
    /// exactly, in whole units with nothing left over.
    #[test]
    fn an_hour_gives_the_rate_by_the_concentration_exactly() {
        let registries = registries();
        let (deposit, _) = field(&registries);
        let hour = u32::try_from(ticks_per_hour()).unwrap();
        for (pm, units) in [(300, 360), (600, 720), (1000, 1200)] {
            let mut machines = Harvesters::default();
            let mut one = planted(&registries, &deposit, [0.0, 0.0], pm, 0);
            one.hopper = 10_000;
            machines.list.push(one);
            advance_ticks(&mut machines, hour / 2);
            assert_eq!(machines.list[0].held, units / 2, "{pm} half an hour");
            advance_ticks(&mut machines, hour / 2);
            assert_eq!(
                (machines.list[0].held, machines.list[0].carry),
                (units, 0),
                "{pm} an hour"
            );
        }
        // A rate that does not divide keeps the part of a unit.
        let mut machines = Harvesters::default();
        let mut one = planted(&registries, &deposit, [0.0, 0.0], 333, 0);
        one.hopper = 10_000;
        machines.list.push(one);
        advance_ticks(&mut machines, hour);
        assert_eq!(machines.list[0].held, 399, "399.6 of a unit");
        assert!(machines.list[0].carry > 0);
        advance_ticks(&mut machines, hour / 4);
        assert_eq!(machines.list[0].held, 499, "499.5");
    }

    /// At the hopper it stops, says it is full once, and holds there.
    #[test]
    fn a_full_hopper_stops_it_and_says_so_once() {
        let registries = registries();
        let (deposit, _) = field(&registries);
        let hour = u32::try_from(ticks_per_hour()).unwrap();
        let mut machines = Harvesters::default();
        machines
            .list
            .push(planted(&registries, &deposit, [0.0, 0.0], 1000, 0));
        // 1000 units at 1200 an hour is fifty minutes.
        let told = advance_ticks(&mut machines, hour / 2);
        assert!(told.is_empty() && machines.list[0].held == 600);
        let told = advance_ticks(&mut machines, hour / 2);
        assert_eq!(told, [0], "full once");
        assert_eq!(
            (machines.list[0].held, machines.list[0].carry),
            (1000, 0),
            "held at the hopper"
        );
        assert!(machines.list[0].is_full() && machines.list[0].full);
        assert!(advance_ticks(&mut machines, hour).is_empty());
        assert_eq!(machines.list[0].held, 1000);
    }

    /// The part of a machine that is saved is all of it: a part filled one
    /// with the carry of a unit comes back the same, and goes on from there.
    #[test]
    fn the_harvesters_survive_a_save() {
        let registries = registries();
        let (deposit, _) = field(&registries);
        let mut machines = Harvesters::default();
        let mut one = planted(&registries, &deposit, [1_000.5, 2_000.25], 333, 0);
        one.hopper = 400;
        one.id = machines.next;
        machines.next += 1;
        machines.list.push(one);
        advance_ticks(&mut machines, 12_345);
        assert!(machines.list[0].held > 0 && machines.list[0].carry > 0);
        let text = ron::to_string(&machines).unwrap();
        let mut back: Harvesters = ron::from_str(&text).unwrap();
        assert_eq!(back, machines);
        advance_ticks(&mut machines, 5_000);
        advance_ticks(&mut back, 5_000);
        assert_eq!(back, machines, "the same units from the same ticks");
        // And a file from before it had a carry or a number reads.
        let old: Harvesters = ron::from_str("(list: [])").unwrap();
        assert_eq!(old, Harvesters::default());
    }

    /// The deposits of one cell near the city, the world's own.
    fn world() -> &'static (Arc<DepositField>, Deposit) {
        static WORLD: OnceLock<(Arc<DepositField>, Deposit)> = OnceLock::new();
        WORLD.get_or_init(|| {
            let registries = registries();
            let world = Arc::new(strata_worldgen::ProceduralWorld::with_params(
                strata_core::Seed(42),
                &registries.0,
                crate::world_params(),
            ));
            let city = world.sites()[0].centre;
            let field = Arc::new(DepositField::new(
                world,
                strata_core::Seed(42),
                Arc::clone(&registries.0),
            ));
            let nearest = field.near(city[0], city[1], 2_000.0)[0].clone();
            (field, nearest)
        })
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
        app.world_mut()
            .resource_mut::<HarvestInbox>()
            .0
            .push(request);
        app.update();
        let mut heard: Vec<HarvestAnswer> = app
            .world_mut()
            .resource_mut::<Messages<HarvestAnswer>>()
            .drain()
            .collect();
        assert_eq!(heard.len(), 1);
        heard.remove(0).outcome
    }

    fn units_of(registries: &Registries, pack: &Pack, deposit: &Deposit) -> (u32, usize) {
        let items = &registries.0.items;
        let mut units = 0;
        let mut stacks = 0;
        for stack in pack.cells.slots().iter().flatten().chain(
            pack.bags
                .iter()
                .flat_map(|bag| bag.slots().iter().flatten()),
        ) {
            let record = items::read(stack, items).unwrap();
            if record
                .origin
                .as_ref()
                .is_some_and(|origin| origin.deposit == deposit.id.0)
            {
                units += record.count;
                stacks += 1;
            }
        }
        (units, stacks)
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
