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
//! Plant, Collect, `PickUp` and Lead are requests in `HarvestInbox`,
//! consumed in `SimSet::Input` after the survey's and answered with a
//! `HarvestAnswer`. Local-only, like the stock: nothing here is
//! replicated, and the game saves [`Harvesters`] with the household.

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use strata_core::SIM_TICK_HZ;

use strata_registry::item::{HARVEST_HOPPER, HARVEST_RATE, ItemCatalogue, ItemKind, Record};

use strata_registry::progression::SkillGraph;

use strata_registry::{GameRegistries, HarvestRules};

use crate::crafting::kind_of;

use crate::deposits::{Deposit, DepositId, HandState, Surveyed, with_lot};

use crate::equip::{Cell, find};

use crate::items::Pack;

use crate::progression::{CharacterSheet, station_field};

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
/// Public for the adapter half in `veyra-sim`, which calls it.
pub fn ticks_per_hour() -> u64 {
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
pub fn without_machine(items: &ItemCatalogue, pack: &Pack, machine: &Record) -> Option<Pack> {
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

    use crate::deposits::Deposit;
    use crate::deposits::DepositId;
    use crate::deposits::Found;
    use crate::deposits::HandState;

    use crate::deposits::Surveyed;

    use crate::deposits::lot;

    use crate::items;

    use crate::items::Pack;
    use crate::progression::CharacterSheet;

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::OnceLock;

    use strata_registry::Namespaced;
    use strata_registry::Reading;
    use strata_registry::ResourceSet;
    use strata_registry::item::HARVEST_HOPPER;
    use strata_registry::item::HARVEST_RATE;

    use strata_registry::item::Record;
    use strata_registry::item::STORE_GRADE;
    use strata_registry::item::SavedPack;

    use strata_types::Registries;
    use veyra_world::deposits::DepositField;

    pub const HARVESTER: &str = "veyra:station.field_harvester";

    pub fn registries() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    /// A harvester record with both lines at `line`.
    pub fn machine(line: u16) -> Record {
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
    pub fn made(set: &ResourceSet, cell: [i32; 2], at: [f64; 2], peak: u16) -> Deposit {
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

    pub fn sheet_of(registries: &Registries, boxes: &[&str]) -> CharacterSheet {
        let _ = registries;
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    pub fn ilse(registries: &Registries) -> CharacterSheet {
        let household = registries.0.sheets.household.as_ref().unwrap();
        let member = household
            .recruits
            .iter()
            .find(|member| member.name == "Ilse Marrow")
            .unwrap();
        CharacterSheet::authored(&member.sheet)
    }

    pub fn pack_with(registries: &Registries, record: Option<&Record>) -> Pack {
        let items = &registries.0.items;
        let mut pack = Pack::from_saved(&SavedPack::default(), items).unwrap();
        if let Some(record) = record {
            let stack = items::write(record, items).unwrap();
            assert!(items::place_in(items, &mut pack.cells, stack).is_none());
        }
        pack
    }

    /// The figures for the tests that plant by hand: a salt deposit, found.
    pub fn field(registries: &Registries) -> (Deposit, Surveyed) {
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

    pub fn standing() -> HandState {
        HandState {
            present: true,
            ..HandState::default()
        }
    }

    /// A machine planted by hand on `deposit` at `pm`, store grade, with
    /// a hopper of `hopper`.
    pub fn planted(
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
    pub fn world() -> &'static (Arc<DepositField>, Deposit) {
        static WORLD: OnceLock<(Arc<DepositField>, Deposit)> = OnceLock::new();
        WORLD.get_or_init(|| {
            let registries = registries();
            let world = Arc::new(strata_worldgen::ProceduralWorld::with_params(
                strata_core::Seed(42),
                &registries.0,
                strata_worldgen::WorldParams::default(),
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

    pub fn units_of(registries: &Registries, pack: &Pack, deposit: &Deposit) -> (u32, usize) {
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
}
