//! Surveying by hand (ticket 0009, step 3): a carried tool pings the ground
//! round the player, and sampling by hand takes lots from what a ping found.
//!
//! A ping is a channel on the player ([`Pinging`]): standing still for the
//! dials' seconds, then [`ping`] reads a grid of points round them, the
//! strongest deposit of the class at each, and the household learns the
//! deposits it met ([`learn`]). Nothing is published beforehand, and a
//! deposit must be pinged before it is sampled. Sampling is a second
//! channel ([`Sampling`]) that gives a lot of the deposit every cycle, more
//! where the concentration is higher.
//!
//! [`can_ping`] and [`can_sample`] are the one rule each, asked by the
//! request and by the screen that greys a button; the dials are
//! `assets/resources/survey.ron` and the tool's own line scales the reach.
//! The skill that scales the rest is the sheet's Surveyor boxes: [`skill`]
//! folds them over the dials and the tool.

use std::collections::BTreeMap;

use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::prelude::*;
use strata_registry::item::{ItemCatalogue, ItemKind, RANGE, Record, STORE_GRADE, ToolReads};
use strata_registry::progression::{Effect, SkillGraph, SurveyRates, XpKind};
use strata_registry::{Attribute, GameRegistries, Namespaced, Reading, ResourceSet};

use super::{
    Deposit, DepositField, DepositId, Found, Lead, NearDeposits, Survey, SurveyUnkept, Surveyed,
    lot, resync, with_lot,
};
use crate::combat::CombatImpact;
use crate::deeds::Ledger;
use crate::items::{Hands, HouseholdStore, OpenStock, Pack, StockAnswer};
use crate::progression::{CharacterSheet, XpGained, learned_effects, survey_field};
use crate::{LocalPlayer, PlayerCommand, Registries, WorldPosition, WorldSourceRes, water_here};

/// What a ping and a sample are worth to whoever makes them: the tool's
/// reach, the grid, the seconds and units of a cycle and how well a deposit
/// is read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurveySkill {
    /// The reach to each side of the player, metres.
    pub range_m: f64,
    /// Points a side of the grid.
    pub grid: u32,
    /// Units a sampling cycle gives at 1000 per-mille.
    pub units: u32,
    /// How well a ping reads what it meets.
    pub reading: Reading,
    /// Seconds the player stands still for a ping.
    pub channel_s: f32,
    /// Seconds of a sampling cycle.
    pub cycle_s: f32,
    /// A ping also tells each met deposit's centre.
    pub peak: bool,
}

/// The skill of `sheet`'s hand holding `tool`: the dials, then the sheet's
/// learned `Survey`, `Read` and `Peak` boxes (Set, Add, Mul per field, see
/// [`strata_registry::progression::Effect::Survey`]), then the tool's own
/// line scaling the reach, so a fine tool multiplies what the boxes added.
/// Anyone can ping untrained: a sheet with no such box reads the dials.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "dials and boxes are small whole numbers, rounded and floored at nought"
)]
pub fn skill(
    set: &ResourceSet,
    items: &ItemCatalogue,
    graph: &SkillGraph,
    tool: &Record,
    sheet: &CharacterSheet,
) -> SurveySkill {
    let rules = set.survey();
    let line = tool.lines.get(RANGE).copied().unwrap_or(STORE_GRADE);
    let factor = items
        .get(&tool.item)
        .map_or(1.0, |def| def.kind.tool_factor(line));
    // A sheet with no box on a field reads the dial untouched, not through f32.
    let field = |name: &str, base: f64| {
        let touched = learned_effects(graph, sheet)
            .any(|effect| matches!(effect, Effect::Survey { field, .. } if field == name));
        if touched {
            f64::from(survey_field(graph, sheet, name, base as f32))
        } else {
            base
        }
    };
    let grid = field("grid", f64::from(rules.ping.grid)).round().max(0.0) as u32;
    let reading = learned_effects(graph, sheet)
        .filter_map(|effect| match effect {
            Effect::Read { reading } => Some(*reading),
            _ => None,
        })
        .fold(rules.reading.untrained, Reading::max);
    SurveySkill {
        range_m: field("range_m", rules.ping.range_m).max(0.0) * factor,
        // Odd, so one point is the player's own place.
        grid: (grid | 1).max(strata_registry::resource::GRID_MIN),
        units: field("units", f64::from(rules.sample.units))
            .round()
            .max(0.0) as u32,
        reading,
        channel_s: field("channel_s", rules.ping.channel_s).max(0.0) as f32,
        cycle_s: field("cycle_s", rules.sample.cycle_s).max(0.0) as f32,
        peak: learned_effects(graph, sheet).any(|effect| matches!(effect, Effect::Peak)),
    }
}

/// The skill of `sheet`'s hand with the best tool the pack carries, or
/// `None` without one: what a request asks for the seconds of a channel.
#[must_use]
pub fn skill_of(
    registries: &GameRegistries,
    pack: &Pack,
    sheet: &CharacterSheet,
) -> Option<SurveySkill> {
    let tool = best_tool(&registries.items, pack)?;
    Some(skill(
        &registries.resources,
        &registries.items,
        &registries.progression,
        &tool,
        sheet,
    ))
}

/// The carried survey tool with the best range line, in the pack or an open
/// bag; `None` without one. A tool is carried and never held.
#[must_use]
pub fn best_tool(items: &ItemCatalogue, pack: &Pack) -> Option<Record> {
    crate::crafting::reachable(items, pack, None)
        .into_iter()
        .map(|(record, _)| record)
        .filter(|record| {
            matches!(
                items.get(&record.item).map(|def| &def.kind),
                Some(ItemKind::Tool {
                    reads: ToolReads::Survey,
                    ..
                })
            )
        })
        .max_by_key(|record| record.lines.get(RANGE).copied().unwrap_or(0))
}

/// Why a ping or a sample was refused. The words a player reads are the
/// screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SurveyRefusal {
    /// There is no local player.
    #[error("nobody is surveying")]
    NoPlayer,
    /// On the bike.
    #[error("not from the saddle")]
    Mounted,
    /// No survey tool is carried.
    #[error("there is no survey tool in the pack")]
    NoTool,
    /// Swimming.
    #[error("not from the water")]
    InWater,
    /// Mid action, downed, calling the bike, or already pinging or sampling.
    #[error("not in the middle of something else")]
    Busy,
    /// No such class of resource.
    #[error("no such class of resource")]
    UnknownClass,
    /// No deposit the household has found holds this spot.
    #[error("no found deposit holds this spot")]
    Nothing,
    /// The pack and its bags have no room for a cycle's lot.
    #[error("there is no room in the pack")]
    NoRoom,
}

/// What the rules need to know of the player, worked out of the sim's
/// components so a screen and a request ask the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent facts of the player the rules read, named by what they are"
)]
pub struct HandState {
    /// There is a local player.
    pub present: bool,
    /// On the bike.
    pub mounted: bool,
    /// In water deeper than the dials' swim depth.
    pub swimming: bool,
    /// Mid action, downed, calling the bike, pinging or sampling.
    pub busy: bool,
}

/// The checks both rules share, in the order a player would fix them: the
/// player, the saddle, the tool, the water, what else is being done.
fn can_survey(
    registries: &GameRegistries,
    state: HandState,
    pack: &Pack,
    sheet: &CharacterSheet,
) -> Result<SurveySkill, SurveyRefusal> {
    if !state.present {
        return Err(SurveyRefusal::NoPlayer);
    }
    if state.mounted {
        return Err(SurveyRefusal::Mounted);
    }
    let tool = best_tool(&registries.items, pack).ok_or(SurveyRefusal::NoTool)?;
    if state.swimming {
        return Err(SurveyRefusal::InWater);
    }
    if state.busy {
        return Err(SurveyRefusal::Busy);
    }
    Ok(skill(
        &registries.resources,
        &registries.items,
        &registries.progression,
        &tool,
        sheet,
    ))
}

/// Whether a ping of `class` can be made now, and with what skill.
///
/// # Errors
/// The first of: no player, the saddle, no tool, swimming, busy, no such
/// class.
pub fn can_ping(
    registries: &GameRegistries,
    state: HandState,
    pack: &Pack,
    sheet: &CharacterSheet,
    class: &Namespaced,
) -> Result<SurveySkill, SurveyRefusal> {
    let skill = can_survey(registries, state, pack, sheet)?;
    if !registries
        .resources
        .classes()
        .iter()
        .any(|each| each.id == *class)
    {
        return Err(SurveyRefusal::UnknownClass);
    }
    Ok(skill)
}

/// The units a sampling cycle gives at `pm` per-mille: `units` times the
/// concentration, rounded, and at least one where there is any reading.
#[must_use]
pub fn cycle_units(units: u32, pm: u16) -> u32 {
    if pm == 0 {
        return 0;
    }
    ((units * u32::from(pm) + 500) / 1000).max(1)
}

/// Whether the player can sample where they stand, from `covering` (every
/// deposit holding the spot with its concentration: `concentration_here`),
/// and which deposit and concentration it would be: the strongest of the
/// deposits the household has found that hold the spot at the dials' least
/// reading or more.
///
/// # Errors
/// The first of: no player, the saddle, no tool, swimming, busy, nothing
/// found here, no room for one cycle's lot.
pub fn can_sample(
    registries: &GameRegistries,
    state: HandState,
    pack: &Pack,
    sheet: &CharacterSheet,
    covering: &[(Deposit, u16)],
    surveyed: &Surveyed,
) -> Result<(Deposit, u16), SurveyRefusal> {
    let skill = can_survey(registries, state, pack, sheet)?;
    let set = &registries.resources;
    let least = set.survey().ping.min_pm;
    let (deposit, pm) = covering
        .iter()
        .filter(|(deposit, pm)| *pm >= least && surveyed.found.contains_key(&deposit.id))
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.id.cmp(&a.0.id)))
        .ok_or(SurveyRefusal::Nothing)?;
    let record = lot(deposit, set, cycle_units(skill.units, *pm)).ok_or(SurveyRefusal::Nothing)?;
    with_lot(&registries.items, pack, &record).ok_or(SurveyRefusal::NoRoom)?;
    Ok((deposit.clone(), *pm))
}

/// One point of a ping's grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PingPoint {
    /// Where, `(x, z)`.
    pub at: [f64; 2],
    /// The strongest deposit of the class holding it at the dials' least
    /// reading or more.
    pub deposit: Option<DepositId>,
    /// Its concentration there, per-mille; nought with no deposit.
    pub pm: u16,
}

/// A deposit a ping met.
#[derive(Debug, Clone, PartialEq)]
pub struct Met {
    /// The deposit.
    pub deposit: Deposit,
    /// Its strongest reading among the grid's points, per-mille.
    pub pm: u16,
    /// The point that read it.
    pub at: [f64; 2],
    /// How well this ping reads it.
    pub reading: Reading,
    /// Where its richest ground is, only when the skill has Peak.
    pub richest: Option<Richest>,
}

/// A deposit's centre and the concentration there, which a ping with the
/// Peak box tells of each deposit it meets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Richest {
    /// The centre, `(x, z)`.
    pub at: [f64; 2],
    /// The concentration there, per-mille.
    pub pm: u16,
}

/// What a ping found.
#[derive(Debug, Clone, PartialEq)]
pub struct PingResult {
    /// The class pinged.
    pub class: Namespaced,
    /// Where the player stood, `(x, z)`: the middle point.
    pub centre: [f64; 2],
    /// Points a side.
    pub grid: u32,
    /// Metres between neighbouring points.
    pub spacing_m: f64,
    /// The grid, row by row from the south (lowest `z`) and across from the
    /// west (lowest `x`) in each: `points[row * grid + column]`.
    pub points: Vec<PingPoint>,
    /// The distinct deposits met, strongest first.
    pub found: Vec<Met>,
}

impl PingResult {
    /// The grid's reach to each side of its middle, in metres.
    #[must_use]
    pub fn reach_m(&self) -> f64 {
        self.spacing_m * f64::from(self.grid / 2)
    }

    /// How far `at` is from where the ping was made, in metres.
    #[must_use]
    pub fn moved_m(&self, at: [f64; 2]) -> f64 {
        (at[0] - self.centre[0]).hypot(at[1] - self.centre[1])
    }

    /// The `(row, column)` of the grid point nearest `at`, the row counted
    /// from the south as `points` is, or `None` when `at` is off the grid by
    /// more than half a spacing: a screen marks the player there and nowhere
    /// else, so the mark is never stale after a walk.
    #[must_use]
    pub fn cell_of(&self, at: [f64; 2]) -> Option<(u32, u32)> {
        let middle = f64::from(self.grid - 1) / 2.0;
        let place = |axis: usize| {
            let cell = ((at[axis] - self.centre[axis]) / self.spacing_m + middle).round();
            (cell >= 0.0 && cell < f64::from(self.grid)).then_some(cell)
        };
        let (column, row) = (place(0)?, place(1)?);
        // Both are whole numbers in `0..grid`, checked just above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some((row as u32, column as u32))
    }

    /// The strongest point of the grid, the nearer the middle on a tie: what
    /// the HUD points to. `None` when the ping met nothing.
    #[must_use]
    pub fn lead(&self) -> Option<Lead> {
        let away = |point: &PingPoint| {
            let (dx, dz) = (point.at[0] - self.centre[0], point.at[1] - self.centre[1]);
            dx * dx + dz * dz
        };
        self.points
            .iter()
            .enumerate()
            .filter_map(|(index, point)| point.deposit.map(|deposit| (index, point, deposit)))
            .min_by(|a, b| {
                b.1.pm
                    .cmp(&a.1.pm)
                    .then_with(|| away(a.1).total_cmp(&away(b.1)))
                    .then_with(|| a.0.cmp(&b.0))
            })
            .map(|(_, point, deposit)| {
                // With Peak the lead goes to the deposit's centre, not to the
                // grid point that happened to read it.
                let richest = self
                    .found
                    .iter()
                    .find(|met| met.deposit.id == deposit)
                    .and_then(|met| met.richest);
                Lead {
                    deposit,
                    at: richest.map_or(point.at, |r| r.at),
                    pm: richest.map_or(point.pm, |r| r.pm),
                }
            })
    }
}

/// A ping from `at`: a `grid` by `grid` lattice of points spaced
/// `2 * range_m / (grid - 1)` round the player, each with the strongest
/// deposit of `class` that `covering` says holds it. Pure in its inputs.
pub fn ping_with(
    covering: &dyn Fn(f64, f64) -> Vec<(Deposit, u16)>,
    set: &ResourceSet,
    skill: &SurveySkill,
    at: [f64; 2],
    class: &Namespaced,
) -> PingResult {
    let least = set.survey().ping.min_pm;
    let side = skill.grid.max(2);
    let middle = f64::from(side - 1) / 2.0;
    let spacing_m = 2.0 * skill.range_m / f64::from(side - 1);
    let mut points = Vec::with_capacity((side * side) as usize);
    let mut met: BTreeMap<DepositId, Met> = BTreeMap::new();
    for row in 0..side {
        for column in 0..side {
            let point = [
                at[0] + (f64::from(column) - middle) * spacing_m,
                at[1] + (f64::from(row) - middle) * spacing_m,
            ];
            let strongest = covering(point[0], point[1])
                .into_iter()
                .filter(|(deposit, pm)| {
                    *pm >= least
                        && set.get(deposit.ty).is_some()
                        && set.class_of(deposit.ty).id == *class
                })
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.id.cmp(&a.0.id)));
            let Some((deposit, pm)) = strongest else {
                points.push(PingPoint {
                    at: point,
                    deposit: None,
                    pm: 0,
                });
                continue;
            };
            points.push(PingPoint {
                at: point,
                deposit: Some(deposit.id),
                pm,
            });
            let entry = met.entry(deposit.id).or_insert_with(|| Met {
                richest: skill.peak.then_some(Richest {
                    at: deposit.at,
                    pm: deposit.peak,
                }),
                deposit,
                pm,
                at: point,
                reading: skill.reading,
            });
            if pm > entry.pm {
                entry.pm = pm;
                entry.at = point;
            }
        }
    }
    let mut found: Vec<Met> = met.into_values().collect();
    found.sort_by(|a, b| {
        b.pm.cmp(&a.pm)
            .then_with(|| a.deposit.id.cmp(&b.deposit.id))
    });
    PingResult {
        class: class.clone(),
        centre: at,
        grid: side,
        spacing_m,
        points,
        found,
    }
}

/// [`ping_with`] over the world's deposits.
#[must_use]
pub fn ping(
    field: &DepositField,
    skill: &SurveySkill,
    at: [f64; 2],
    class: &Namespaced,
) -> PingResult {
    ping_with(&|x, z| field.covering(x, z), field.set(), skill, at, class)
}

/// What a ping taught the household that it did not know, for the XP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Learned {
    /// Deposits the household had not found.
    pub new: u32,
    /// The most the ping raised any already found deposit's best reading by,
    /// per-mille.
    pub raised_pm: u16,
}

/// The household learns what `result` met: each deposit joins the found at
/// the ping's reading (never lowering what is known), with the strongest
/// concentration ever met and where, and the lead becomes the ping's
/// strongest point, or none when it met nothing.
pub fn learn(surveyed: &mut Surveyed, result: &PingResult) -> Learned {
    let mut learned = Learned::default();
    for met in &result.found {
        let before = surveyed.found.get(&met.deposit.id).map(|f| f.best_pm);
        match before {
            None => learned.new += 1,
            Some(best) => learned.raised_pm = learned.raised_pm.max(met.pm.saturating_sub(best)),
        }
        let known = surveyed.found.entry(met.deposit.id).or_insert(Found {
            reading: met.reading,
            sampled: false,
            best_pm: 0,
            best_at: met.at,
        });
        known.reading = known.reading.max(met.reading);
        if met.pm > known.best_pm {
            known.best_pm = met.pm;
            known.best_at = met.at;
        }
    }
    surveyed.lead = result.lead();
    learned
}

/// Something a surveyor did that is worth Survey XP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyEvent {
    /// A ping that met something: a deposit not found before, and/or a found
    /// deposit raised by the dials' `better_pm`. A ping that met nothing
    /// new earns nothing, so standing and pinging is no way to grind.
    Ping {
        /// It met a deposit the household had not found.
        found: bool,
        /// It raised a found deposit's best reading enough to count.
        better: bool,
    },
    /// A sampling cycle at `pm` per-mille.
    Cycle {
        /// The deposit's first cycle ever.
        first: bool,
        /// The concentration sampled, per-mille.
        pm: u16,
    },
    /// Units collected from a field harvester, by whoever collects them.
    Harvest {
        /// How many.
        units: u32,
    },
}

/// The Survey XP of `event`: the rates are `trees.ron`'s.
#[must_use]
pub fn survey_xp(rates: &SurveyRates, event: SurveyEvent) -> u32 {
    match event {
        SurveyEvent::Ping { found, better } => {
            u32::from(found) * rates.find + u32::from(better) * rates.better
        }
        SurveyEvent::Cycle { first, pm } => {
            let each = (rates.cycle * u32::from(pm) / 1000).max(1);
            each + u32::from(first) * rates.first_sample
        }
        SurveyEvent::Harvest { units } => units / 10 * rates.harvest,
    }
}

/// The band of a deposit's figure for `attribute`: 0 poor, 1 fair, 2 good,
/// 3 fine, by the dials' bounds over the type's range for the line. `None`
/// for a line the type's class does not carry.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a share of a range, in per-mille, rounded"
)]
pub fn line_band(set: &ResourceSet, deposit: &Deposit, attribute: Attribute) -> Option<u8> {
    let span = set.get(deposit.ty)?.ranges.get(&attribute)?;
    let value = deposit.figure(attribute)?;
    let share = (span.share(value) * 1000.0).round().clamp(0.0, 1000.0) as u16;
    Some(set.survey().band(share))
}

/// Puts `amount` of Survey XP on `sheet` and tells the HUD; nothing for
/// nothing.
pub(crate) fn credit_survey(
    sheet: &mut CharacterSheet,
    entity: Entity,
    amount: u32,
    gained: &mut MessageWriter<XpGained>,
) {
    if amount == 0 {
        return;
    }
    sheet.credit(XpKind::Survey, amount);
    gained.write(XpGained {
        entity,
        kind: XpKind::Survey,
        amount,
        kill: false,
    });
}

/// A ping being channelled: the player stands still for it, and moving
/// cancels it with no reading.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Pinging {
    /// The class being pinged.
    pub class: Namespaced,
    /// Seconds left.
    pub left: f32,
}

/// Sampling under way: every cycle a lot of the deposit joins the pack.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Sampling {
    /// The deposit.
    pub deposit: DepositId,
    /// Where it began, `(x, z)`; moving the dials' distance from it stops it.
    pub from: [f64; 2],
    /// Seconds to the next cycle.
    pub left: f32,
    /// Units got so far.
    pub got: u32,
}

/// The last ping's result, kept so a screen opened after the answer was sent
/// still draws it.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct LastPing(pub Option<PingResult>);

/// What a ping request came to.
#[derive(Debug, Clone, PartialEq)]
pub enum PingDone {
    /// The channel started.
    Started,
    /// The channel ended and this is what the ground read.
    Met(PingResult),
}

/// The sim's answer to a [`SurveyRequest::Ping`](super::SurveyRequest::Ping),
/// and again when its channel ends.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct PingAnswer {
    /// The class asked for.
    pub class: Namespaced,
    /// What happened, or why nothing did.
    pub outcome: Result<PingDone, SurveyRefusal>,
}

/// What a sample request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleDone {
    /// Sampling started on this deposit, at this concentration.
    Started {
        /// The deposit.
        deposit: DepositId,
        /// Per-mille where the player stands.
        pm: u16,
    },
    /// A cycle gave this lot.
    Cycle {
        /// The deposit.
        deposit: DepositId,
        /// Units added to the pack.
        units: u32,
        /// Units got since sampling began.
        total: u32,
    },
}

/// The sim's answer to a [`SurveyRequest::Sample`](super::SurveyRequest::Sample),
/// and again at every cycle.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleAnswer {
    /// What happened, or why nothing did.
    pub outcome: Result<SampleDone, SurveyRefusal>,
}

/// Why sampling ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleEnd {
    /// The player stopped it.
    Stopped,
    /// The player moved from where it began.
    Moved,
    /// A blow landed on the player.
    Hit,
    /// The player is swimming.
    Swimming,
    /// No room for a cycle's lot.
    NoRoom,
    /// The survey tool is no longer carried.
    NoTool,
}

/// A channel ended without being finished, for a toast.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyNotice {
    /// A ping was cancelled by moving.
    PingMoved,
    /// A ping was stopped by the player.
    PingStopped,
    /// A ping ended with the tool gone.
    PingNoTool,
    /// Sampling ended, and why.
    SampleEnded(SampleEnd),
}

/// `SimTick`, `SimSet::Observe`: the channel of a ping under way. Moving
/// cancels it; at the end the ground is read, the household learns what it
/// met and the answer carries the result.
#[allow(
    clippy::too_many_arguments,
    reason = "the channel, the world and the household in, an answer and a notice out"
)]
pub fn advance_ping(
    mut commands: Commands,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    unkept: Option<Res<SurveyUnkept>>,
    mut surveyed: ResMut<Surveyed>,
    mut last: ResMut<LastPing>,
    mut players: Query<
        (
            Entity,
            &mut Pinging,
            Option<&PlayerCommand>,
            &WorldPosition,
            &Pack,
            Option<&mut CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<PingAnswer>,
    mut notices: MessageWriter<SurveyNotice>,
    mut gained: MessageWriter<XpGained>,
) {
    for (entity, mut pinging, command, at, pack, mut sheet) in &mut players {
        if command.is_some_and(|command| command.0.move_dir.length_squared() > 0.0) {
            commands.entity(entity).remove::<Pinging>();
            notices.write(SurveyNotice::PingMoved);
            continue;
        }
        pinging.left -= time.delta_secs();
        if pinging.left > 0.0 {
            continue;
        }
        commands.entity(entity).remove::<Pinging>();
        let registries = &registries.0;
        let Some(tool) = best_tool(&registries.items, pack) else {
            notices.write(SurveyNotice::PingNoTool);
            continue;
        };
        let skill = skill(
            &registries.resources,
            &registries.items,
            &registries.progression,
            &tool,
            sheet.as_deref().unwrap_or(&CharacterSheet::default()),
        );
        let here = [at.0.x(), at.0.z()];
        let result = match survey.as_deref() {
            Some(survey) => ping(&survey.0, &skill, here, &pinging.class),
            None => ping_with(
                &|_, _| Vec::new(),
                &registries.resources,
                &skill,
                here,
                &pinging.class,
            ),
        };
        let learned = learn(&mut surveyed, &result);
        let rates = &registries.progression.rates.survey;
        let earned = survey_xp(
            rates,
            SurveyEvent::Ping {
                found: learned.new > 0,
                better: learned.raised_pm >= rates.better_pm,
            },
        );
        if let Some(sheet) = sheet.as_deref_mut().filter(|_| unkept.is_none()) {
            credit_survey(sheet, entity, earned, &mut gained);
        }
        last.0 = Some(result.clone());
        answers.write(PingAnswer {
            class: pinging.class.clone(),
            outcome: Ok(PingDone::Met(result)),
        });
    }
}

/// `SimTick`, `SimSet::Observe`: sampling under way. A blow, swimming or
/// moving from where it began ends it, as does a pack with no room for the
/// cycle's lot; each cycle puts a lot in the pack, then a bag, and reads the
/// deposit exactly from then on.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    reason = "the channel, the world, the stock and the household in, two answers and a mirror out; a dial's seconds as a clock's f32"
)]
pub fn advance_sample(
    mut commands: Commands,
    time: Res<bevy::time::Time<bevy::time::Fixed>>,
    mut impacts: MessageReader<CombatImpact>,
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    source: Option<Res<WorldSourceRes>>,
    near: Res<NearDeposits>,
    open: Res<OpenStock>,
    store: Res<HouseholdStore>,
    unkept: Option<Res<SurveyUnkept>>,
    mut surveyed: ResMut<Surveyed>,
    mut players: Query<
        (
            Entity,
            &mut Sampling,
            &WorldPosition,
            &mut Pack,
            &Hands,
            Option<&mut CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut answers: MessageWriter<SampleAnswer>,
    mut notices: MessageWriter<SurveyNotice>,
    mut mirrors: MessageWriter<StockAnswer>,
    (mut gained, mut ledger): (MessageWriter<XpGained>, Ledger),
) {
    let hit: Vec<Entity> = impacts.read().filter_map(|impact| impact.victim).collect();
    let registries = &registries.0;
    let (set, items) = (&registries.resources, &registries.items);
    for (entity, mut sampling, at, mut pack, hands, mut sheet) in &mut players {
        let here = [at.0.x(), at.0.z()];
        let stray = {
            let (dx, dz) = (here[0] - sampling.from[0], here[1] - sampling.from[1]);
            dx.hypot(dz) > set.survey().sample.stops_at_m
        };
        let swimming = source.as_ref().is_some_and(|source| {
            water_here(source.source(), here[0], here[1]).depth_m() > set.survey().ping.swim_m
        });
        let ended = if hit.contains(&entity) {
            Some(SampleEnd::Hit)
        } else if swimming {
            Some(SampleEnd::Swimming)
        } else if stray {
            Some(SampleEnd::Moved)
        } else {
            None
        };
        let end = |why: SampleEnd, commands: &mut Commands, notices: &mut MessageWriter<_>| {
            commands.entity(entity).remove::<Sampling>();
            notices.write(SurveyNotice::SampleEnded(why));
        };
        if let Some(why) = ended {
            end(why, &mut commands, &mut notices);
            continue;
        }
        sampling.left -= time.delta_secs();
        if sampling.left > 0.0 {
            continue;
        }
        let Some(tool) = best_tool(items, &pack) else {
            end(SampleEnd::NoTool, &mut commands, &mut notices);
            continue;
        };
        let skill = skill(
            set,
            items,
            &registries.progression,
            &tool,
            sheet.as_deref().unwrap_or(&CharacterSheet::default()),
        );
        sampling.left += skill.cycle_s;
        let deposit = survey
            .as_ref()
            .and_then(|survey| survey.0.get(sampling.deposit))
            .or_else(|| near.get(sampling.deposit).cloned());
        let Some(deposit) = deposit else {
            end(SampleEnd::Stopped, &mut commands, &mut notices);
            continue;
        };
        let pm = deposit.concentration(sampling.from[0], sampling.from[1]);
        let units = cycle_units(skill.units, pm);
        let taken = lot(&deposit, set, units).and_then(|record| with_lot(items, &pack, &record));
        let Some(next) = taken.filter(|_| units > 0) else {
            end(SampleEnd::NoRoom, &mut commands, &mut notices);
            continue;
        };
        *pack = next;
        sampling.got += units;
        ledger.worked(registries, &deposit, units);
        let first = surveyed.found.get(&deposit.id).is_none_or(|f| !f.sampled);
        if let Some(sheet) = sheet.as_deref_mut().filter(|_| unkept.is_none()) {
            let earned = survey_xp(
                &registries.progression.rates.survey,
                SurveyEvent::Cycle { first, pm },
            );
            credit_survey(sheet, entity, earned, &mut gained);
        }
        let known = surveyed.found.entry(deposit.id).or_insert(Found {
            reading: Reading::Exact,
            sampled: true,
            best_pm: deposit.concentration(sampling.from[0], sampling.from[1]),
            best_at: sampling.from,
        });
        known.sampled = true;
        known.reading = Reading::Exact;
        answers.write(SampleAnswer {
            outcome: Ok(SampleDone::Cycle {
                deposit: deposit.id,
                units,
                total: sampling.got,
            }),
        });
        resync(&open, &store, &pack, hands, &mut mirrors);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "the lattice's points are exact sums of the spacing"
    )]

    use std::sync::{Arc, OnceLock};

    use bevy::app::App;
    use bevy::ecs::message::Messages;
    use bevy::math::Vec2;
    use strata_registry::ResourceTypeId;
    use strata_registry::item::SavedPack;
    use strata_registry::progression::XpKind;

    use super::*;
    use crate::combat::{Action, Combatant};
    use crate::deposits::{SurveyInbox, SurveyRequest};
    use crate::items::Pack;
    use crate::tests::app_in_world;
    use crate::{PlayerIntent, WorldPosition};

    fn registries() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    fn class_of(set: &ResourceSet, deposit: &Deposit) -> Namespaced {
        set.class_of(deposit.ty).id.clone()
    }

    /// A deposit of the type named, round `at`, with a radius of 200 m and
    /// a peak of `peak`: made by hand so no world need be baked.
    fn made(set: &ResourceSet, ty: &str, cell: [i32; 2], at: [f64; 2], peak: u16) -> Deposit {
        let ty: ResourceTypeId = set.type_id(&id(ty)).unwrap();
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

    fn covering_of(deposits: &[Deposit]) -> impl Fn(f64, f64) -> Vec<(Deposit, u16)> + '_ {
        move |x, z| {
            let mut held: Vec<(Deposit, u16)> = deposits
                .iter()
                .filter_map(|deposit| {
                    let pm = deposit.concentration(x, z);
                    (pm > 0).then(|| (deposit.clone(), pm))
                })
                .collect();
            held.sort_by_key(|held| std::cmp::Reverse(held.1));
            held
        }
    }

    const SALT: &str = "strata:evaporite_salt";
    const ALLOY: &str = "strata:light_alloy_ore";

    fn standing() -> HandState {
        HandState {
            present: true,
            ..HandState::default()
        }
    }

    fn arrival_pack(registries: &Registries) -> Pack {
        let starting = registries.0.items.starting().unwrap();
        let arrival = &registries.0.sheets.household.as_ref().unwrap().arrival;
        Pack::from_saved(&starting.packs[&arrival.name], &registries.0.items).unwrap()
    }

    fn bare_pack(registries: &Registries) -> Pack {
        Pack::from_saved(&SavedPack::default(), &registries.0.items).unwrap()
    }

    /// The ping's lattice is `grid` points a side spaced by twice the reach
    /// over one fewer, the middle one the player's own place; the tool's
    /// line scales the reach.
    #[test]
    fn a_ping_reads_a_grid_with_the_spacing_of_the_reach() {
        let registries = registries();
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let graph = &registries.0.progression;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let sheet = CharacterSheet::default();
        let store = skill(set, items, graph, &tool, &sheet);
        assert_eq!(
            (store.grid, store.units, store.reading),
            (5, 8, Reading::Type)
        );
        assert!(
            (store.range_m - 128.0).abs() < 1e-9,
            "store grade is the dial"
        );
        let mut fine = tool.clone();
        fine.lines.insert(RANGE.to_owned(), 1000);
        assert!((skill(set, items, graph, &fine, &sheet).range_m - 192.0).abs() < 1e-9);
        fine.lines.insert(RANGE.to_owned(), 0);
        assert!((skill(set, items, graph, &fine, &sheet).range_m - 96.0).abs() < 1e-9);

        // One salt deposit at the player's feet and one alloy to the east.
        let salt = made(set, SALT, [1, 1], [1_000.0, 2_000.0], 800);
        let alloy = made(set, ALLOY, [2, 1], [1_160.0, 2_000.0], 900);
        let deposits = [salt.clone(), alloy.clone()];
        let covering = covering_of(&deposits);
        let reactive = class_of(set, &salt);
        let result = ping_with(&covering, set, &store, [1_000.0, 2_000.0], &reactive);
        assert_eq!((result.grid, result.points.len()), (5, 25));
        assert!((result.spacing_m - 64.0).abs() < 1e-9);
        // Row by row from the south, across from the west.
        let middle = &result.points[12];
        assert_eq!(middle.at, [1_000.0, 2_000.0]);
        assert_eq!((middle.deposit, middle.pm), (Some(salt.id), 800));
        assert_eq!(result.points[0].at, [872.0, 1_872.0]);
        assert_eq!(result.points[4].at, [1_128.0, 1_872.0]);
        assert_eq!(result.points[20].at, [872.0, 2_128.0]);
        // The alloy is another class, so a ping of salt's never meets it.
        assert!(result.points.iter().all(|p| p.deposit != Some(alloy.id)));
        assert_eq!(result.found.len(), 1);
        let met = &result.found[0];
        assert_eq!(
            (met.deposit.id, met.pm, met.at),
            (salt.id, 800, [1_000.0, 2_000.0])
        );
        assert_eq!(met.reading, Reading::Type);
        // The same call is the same answer.
        assert_eq!(
            result,
            ping_with(&covering, set, &store, [1_000.0, 2_000.0], &reactive)
        );
        // The structural class reads the alloy at the edge of the grid and
        // its strongest point is where the reading is greatest.
        let structural = class_of(set, &alloy);
        let result = ping_with(&covering, set, &store, [1_000.0, 2_000.0], &structural);
        let lead = result.lead().unwrap();
        assert_eq!(lead.deposit, alloy.id);
        assert_eq!(
            lead.at,
            [1_128.0, 2_000.0],
            "the grid point nearest its centre"
        );
        assert_eq!(lead.pm, alloy.concentration(1_128.0, 2_000.0));
        // Under the least reading is nothing: far off, the ping finds none.
        let far = ping_with(&covering, set, &store, [5_000.0, 5_000.0], &reactive);
        assert!(far.found.is_empty() && far.lead().is_none());
        assert!(far.points.iter().all(|p| p.deposit.is_none() && p.pm == 0));
        // A deposit seen only at its thin edge is under the dial's least.
        let edge = made(set, SALT, [3, 3], [6_000.0, 6_000.0], 300);
        let edges = [edge];
        let thin = ping_with(
            &covering_of(&edges),
            set,
            &store,
            [6_318.0, 6_000.0],
            &reactive,
        );
        assert!(thin.found.is_empty(), "{:?}", thin.points);
    }

    /// A ping never lowers what is known and keeps the strongest reading
    /// and where it was; the lead is the last ping's alone.
    #[test]
    fn a_ping_never_lowers_what_is_known() {
        let registries = registries();
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let mut skilled = skill(
            set,
            items,
            &registries.0.progression,
            &tool,
            &CharacterSheet::default(),
        );
        let salt = made(set, SALT, [1, 1], [0.0, 0.0], 800);
        let deposits = [salt.clone()];
        let covering = covering_of(&deposits);
        let reactive = class_of(set, &salt);
        let mut surveyed = Surveyed::default();

        skilled.reading = Reading::Bands;
        let near = ping_with(&covering, set, &skilled, [0.0, 0.0], &reactive);
        learn(&mut surveyed, &near);
        let known = surveyed.found[&salt.id];
        assert_eq!(
            (known.reading, known.best_pm, known.best_at),
            (Reading::Bands, 800, [0.0, 0.0])
        );
        assert!(!known.sampled);
        assert_eq!(surveyed.lead.unwrap().pm, 800);

        // A worse reading from a worse skill and a weaker point changes
        // neither the reading nor the best.
        skilled.reading = Reading::Type;
        let edge = ping_with(&covering, set, &skilled, [100.0, 100.0], &reactive);
        learn(&mut surveyed, &edge);
        let known = surveyed.found[&salt.id];
        assert_eq!(
            (known.reading, known.best_pm, known.best_at),
            (Reading::Bands, 800, [0.0, 0.0])
        );
        assert!(surveyed.lead.unwrap().pm < 800, "the lead is this ping's");

        // A stronger point raises the best.
        let mut raised = salt.clone();
        raised.peak = 950;
        let stronger = [raised];
        let better = ping_with(
            &covering_of(&stronger),
            set,
            &skilled,
            [0.0, 0.0],
            &reactive,
        );
        learn(&mut surveyed, &better);
        assert_eq!(surveyed.found[&salt.id].best_pm, 950);

        // A ping that meets nothing clears the lead and keeps the find.
        let nothing = ping_with(&covering, set, &skilled, [9_000.0, 9_000.0], &reactive);
        learn(&mut surveyed, &nothing);
        assert!(surveyed.lead.is_none());
        assert_eq!(surveyed.found.len(), 1);
    }

    /// Every refusal of a ping, in the order a player would fix them.
    #[test]
    fn a_ping_is_refused_for_each_reason() {
        let registries = registries();
        let sheet = CharacterSheet::default();
        let pack = arrival_pack(&registries);
        let reactive = id("strata:reactive");
        let ask = |state: HandState, pack: &Pack, class: &Namespaced| {
            can_ping(&registries.0, state, pack, &sheet, class)
        };
        assert!(ask(standing(), &pack, &reactive).is_ok());
        assert_eq!(
            ask(HandState::default(), &pack, &reactive),
            Err(SurveyRefusal::NoPlayer)
        );
        let mounted = HandState {
            mounted: true,
            ..standing()
        };
        assert_eq!(ask(mounted, &pack, &reactive), Err(SurveyRefusal::Mounted));
        assert_eq!(
            ask(standing(), &bare_pack(&registries), &reactive),
            Err(SurveyRefusal::NoTool)
        );
        let swimming = HandState {
            swimming: true,
            ..standing()
        };
        assert_eq!(ask(swimming, &pack, &reactive), Err(SurveyRefusal::InWater));
        let busy = HandState {
            busy: true,
            ..standing()
        };
        assert_eq!(ask(busy, &pack, &reactive), Err(SurveyRefusal::Busy));
        assert_eq!(
            ask(standing(), &pack, &id("strata:nonsense")),
            Err(SurveyRefusal::UnknownClass)
        );
    }

    /// Every refusal of a sample, and which deposit it is where two are
    /// found.
    #[test]
    fn a_sample_is_refused_for_each_reason() {
        let registries = registries();
        let set = &registries.0.resources;
        let sheet = CharacterSheet::default();
        let pack = arrival_pack(&registries);
        let weak = made(set, SALT, [1, 1], [0.0, 0.0], 400);
        let strong = made(set, SALT, [2, 2], [10.0, 0.0], 900);
        let covering = vec![(strong.clone(), 880), (weak.clone(), 390)];
        let found = |ids: &[DepositId]| Surveyed {
            found: ids
                .iter()
                .map(|id| {
                    (
                        *id,
                        Found {
                            reading: Reading::Type,
                            sampled: false,
                            best_pm: 1,
                            best_at: [0.0, 0.0],
                        },
                    )
                })
                .collect(),
            ..Surveyed::default()
        };
        let ask = |state: HandState, pack: &Pack, surveyed: &Surveyed, held: &[(Deposit, u16)]| {
            can_sample(&registries.0, state, pack, &sheet, held, surveyed)
        };
        let both = found(&[weak.id, strong.id]);
        let (deposit, pm) = ask(standing(), &pack, &both, &covering).unwrap();
        assert_eq!((deposit.id, pm), (strong.id, 880), "the strongest");
        // Only the found count: with just the weak one found, it is that.
        let (deposit, _) = ask(standing(), &pack, &found(&[weak.id]), &covering).unwrap();
        assert_eq!(deposit.id, weak.id);
        // Nothing found holds the spot: not pinged, not sampled.
        assert_eq!(
            ask(standing(), &pack, &found(&[]), &covering),
            Err(SurveyRefusal::Nothing)
        );
        assert_eq!(
            ask(standing(), &pack, &both, &[]),
            Err(SurveyRefusal::Nothing)
        );
        // Under the least reading is nothing even for a found deposit.
        assert_eq!(
            ask(standing(), &pack, &both, &[(strong.clone(), 19)]),
            Err(SurveyRefusal::Nothing)
        );
        assert_eq!(
            ask(HandState::default(), &pack, &both, &covering),
            Err(SurveyRefusal::NoPlayer)
        );
        let mounted = HandState {
            mounted: true,
            ..standing()
        };
        assert_eq!(
            ask(mounted, &pack, &both, &covering),
            Err(SurveyRefusal::Mounted)
        );
        assert_eq!(
            ask(standing(), &bare_pack(&registries), &both, &covering),
            Err(SurveyRefusal::NoTool)
        );
        let swimming = HandState {
            swimming: true,
            ..standing()
        };
        assert_eq!(
            ask(swimming, &pack, &both, &covering),
            Err(SurveyRefusal::InWater)
        );
        let busy = HandState {
            busy: true,
            ..standing()
        };
        assert_eq!(ask(busy, &pack, &both, &covering), Err(SurveyRefusal::Busy));
        // A pack whose every cell is taken has no room for a lot.
        let mut full = pack.clone();
        crate::crafting::tests::fill(&mut full.cells, 18, &registries.0.items);
        for bag in 0..2 {
            let open = usize::from(full.open_cells(bag, &registries.0.items));
            crate::crafting::tests::fill(&mut full.bags[bag], open, &registries.0.items);
        }
        assert_eq!(
            ask(standing(), &full, &both, &covering),
            Err(SurveyRefusal::NoRoom)
        );
    }

    /// A line's band follows its place in the type's range, and a line the
    /// class does not carry has none.
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a figure of a span, rounded"
    )]
    fn a_lines_band_follows_its_share_of_the_range() {
        let registries = registries();
        let set = &registries.0.resources;
        let mut salt = made(set, SALT, [1, 1], [0.0, 0.0], 800);
        let attribute = salt.figures[0].0;
        let span = set.get(salt.ty).unwrap().ranges[&attribute];
        let mut at = |share: f64| {
            let value = f64::from(span.floor) + share * f64::from(span.cap - span.floor);
            salt.figures[0].1 = value.round() as u16;
            line_band(set, &salt, attribute)
        };
        assert_eq!(at(0.0), Some(0));
        assert_eq!(at(0.4), Some(1));
        assert_eq!(at(0.6), Some(2));
        assert_eq!(at(1.0), Some(3));
        assert_eq!(line_band(set, &salt, Attribute::Hardness), None);
    }

    /// A cycle gives the dial's units in proportion to the concentration,
    /// and one at the least where there is any reading.
    #[test]
    fn a_cycle_gives_units_by_concentration_with_a_floor_of_one() {
        assert_eq!(cycle_units(8, 1000), 8);
        assert_eq!(cycle_units(8, 500), 4);
        assert_eq!(cycle_units(8, 250), 2);
        assert_eq!(cycle_units(8, 20), 1, "a floor of one");
        assert_eq!(cycle_units(8, 1), 1);
        assert_eq!(cycle_units(8, 0), 0, "no reading, no units");
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

    /// A player with the arrival's pack standing `off` metres east of the
    /// nearest deposit's centre, in the world.
    fn playing(off: f64) -> (App, Entity, Deposit) {
        let (field, deposit) = world();
        let mut app = app_in_world();
        let registries = registries();
        let pack = arrival_pack(&registries);
        app.insert_resource(registries)
            .insert_resource(Survey(Arc::clone(field)));
        let player = app
            .world_mut()
            .spawn((
                crate::Player::default(),
                LocalPlayer,
                pack,
                Hands::default(),
                WorldPosition::new(deposit.at[0] + off, 0.0, deposit.at[1]),
                Combatant::default(),
            ))
            .id();
        app.update();
        (app, player, deposit.clone())
    }

    /// What the sim said over some ticks, collected tick by tick since
    /// a message lives two updates only.
    #[derive(Default)]
    struct Heard {
        pings: Vec<PingAnswer>,
        samples: Vec<SampleAnswer>,
        notices: Vec<SurveyNotice>,
    }

    fn tick(app: &mut App, heard: &mut Heard) {
        app.update();
        let world = app.world_mut();
        heard
            .pings
            .extend(world.resource_mut::<Messages<PingAnswer>>().drain());
        heard
            .samples
            .extend(world.resource_mut::<Messages<SampleAnswer>>().drain());
        heard
            .notices
            .extend(world.resource_mut::<Messages<SurveyNotice>>().drain());
    }

    fn send(app: &mut App, request: SurveyRequest) -> Heard {
        app.world_mut()
            .resource_mut::<SurveyInbox>()
            .0
            .push(request);
        let mut heard = Heard::default();
        tick(app, &mut heard);
        heard
    }

    fn run(app: &mut App, ticks: usize) -> Heard {
        let mut heard = Heard::default();
        for _ in 0..ticks {
            tick(app, &mut heard);
        }
        heard
    }

    fn class_id(app: &App, deposit: &Deposit) -> Namespaced {
        class_of(&app.world().resource::<Registries>().0.resources, deposit)
    }

    /// A ping channels for the dials' seconds, then answers with what the
    /// ground read, and the household has found the deposit; sampling
    /// needs that first.
    #[test]
    fn a_ping_channels_then_finds_and_a_sample_needs_the_find() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert_eq!(
            heard.samples[0].outcome,
            Err(SurveyRefusal::Nothing),
            "not pinged, not sampled"
        );
        let started = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        )
        .pings;
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].outcome, Ok(PingDone::Started));
        assert!(app.world().get::<Pinging>(player).is_some());
        // A second ping while one is under way is busy.
        let again = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        )
        .pings;
        assert_eq!(again[0].outcome, Err(SurveyRefusal::Busy));
        // 2.5 s is 50 ticks at 20 Hz.
        let done = run(&mut app, 60).pings;
        assert_eq!(done.len(), 1);
        let Ok(PingDone::Met(result)) = &done[0].outcome else {
            panic!("{done:?}");
        };
        assert_eq!(result.found[0].deposit.id, deposit.id);
        assert_eq!(result.found[0].pm, deposit.peak);
        assert!(app.world().get::<Pinging>(player).is_none());
        let surveyed = app.world().resource::<Surveyed>();
        assert!(surveyed.found.contains_key(&deposit.id));
        assert_eq!(surveyed.lead.unwrap().deposit, deposit.id);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert!(matches!(
            heard.samples[0].outcome,
            Ok(SampleDone::Started { .. })
        ));
    }

    /// Moving cancels a ping with no reading; stopping does too.
    #[test]
    fn moving_cancels_a_ping_with_no_reading() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        run(&mut app, 10);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::Y;
        let heard = run(&mut app, 2);
        assert!(app.world().get::<Pinging>(player).is_none());
        assert_eq!(heard.notices, [SurveyNotice::PingMoved]);
        app.world_mut()
            .get_mut::<PlayerIntent>(player)
            .unwrap()
            .move_dir = Vec2::ZERO;
        let heard = run(&mut app, 60);
        assert!(heard.pings.is_empty(), "no reading");
        assert!(app.world().resource::<Surveyed>().found.is_empty());

        send(&mut app, SurveyRequest::Ping { class });
        let heard = send(&mut app, SurveyRequest::Stop);
        assert_eq!(heard.notices, [SurveyNotice::PingStopped]);
        assert!(app.world().get::<Pinging>(player).is_none());
    }

    /// The pack's survey tool, a rider and the swimmer: refused at the
    /// request, in the sim's own state.
    #[test]
    fn a_player_in_the_saddle_or_without_a_tool_cannot_ping() {
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        let def = app
            .world()
            .resource::<Registries>()
            .0
            .vehicles
            .iter()
            .next()
            .unwrap()
            .clone();
        app.world_mut()
            .entity_mut(player)
            .insert(crate::ride::Riding::new(&def, 0.0));
        let heard = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::Mounted));
        app.world_mut()
            .entity_mut(player)
            .remove::<crate::ride::Riding>();
        let registries = app.world().resource::<Registries>().clone();
        *app.world_mut().get_mut::<Pack>(player).unwrap() = bare_pack(&registries);
        let heard = send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::NoTool));
        // Mid action is busy.
        *app.world_mut().get_mut::<Pack>(player).unwrap() = arrival_pack(&registries);
        app.world_mut().get_mut::<Combatant>(player).unwrap().action = Action::Heavy;
        let heard = send(&mut app, SurveyRequest::Ping { class });
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::Busy));
    }

    /// Start sampling at the deposit's centre: found by a ping first.
    fn sampling_at(off: f64) -> (App, Entity, Deposit) {
        let (mut app, player, deposit) = playing(off);
        let class = class_id(&app, &deposit);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        let heard = send(&mut app, SurveyRequest::Sample);
        assert!(matches!(
            heard.samples[0].outcome,
            Ok(SampleDone::Started { .. })
        ));
        (app, player, deposit)
    }

    fn carried(app: &App, player: Entity, deposit: &Deposit) -> u32 {
        let items = &app.world().resource::<Registries>().0.items;
        let pack = app.world().get::<Pack>(player).unwrap();
        crate::crafting::reachable(items, pack, None)
            .into_iter()
            .filter(|(record, _)| {
                record
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.deposit == deposit.id.0)
            })
            .map(|(record, _)| record.count)
            .sum()
    }

    /// Each cycle puts a lot of the deposit in the pack, the lots of one
    /// deposit stack across cycles, the units follow the concentration and
    /// the deposit is sampled and exact from the first.
    #[test]
    fn sampling_stacks_cycles_by_concentration() {
        let (mut app, player, deposit) = sampling_at(0.0);
        let per = cycle_units(8, deposit.peak);
        // 4 s a cycle is 80 ticks; three cycles.
        let got: Vec<SampleDone> = run(&mut app, 80 * 3 + 2)
            .samples
            .into_iter()
            .map(|answer| answer.outcome.unwrap())
            .collect();
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[2],
            SampleDone::Cycle {
                deposit: deposit.id,
                units: per,
                total: 3 * per
            }
        );
        assert_eq!(carried(&app, player, &deposit), 3 * per);
        // One stack, because the lots are of one deposit.
        let items = &app.world().resource::<Registries>().0.items;
        let pack = app.world().get::<Pack>(player).unwrap();
        let lots = crate::crafting::reachable(items, pack, None)
            .into_iter()
            .filter(|(record, _)| record.origin.is_some())
            .count();
        assert_eq!(lots, 1);
        let found = app.world().resource::<Surveyed>().found[&deposit.id];
        assert!(found.sampled);
        assert_eq!(found.reading, Reading::Exact);
        assert_eq!(app.world().get::<Sampling>(player).unwrap().got, 3 * per);

        // Half way to the edge the cycle gives less, in proportion.
        let off = f64::from(deposit.radius_m) * 0.5;
        let (mut app, player, deposit) = sampling_at(off);
        let pm = deposit.concentration(deposit.at[0] + off, deposit.at[1]);
        assert!(pm < deposit.peak && pm >= 20);
        run(&mut app, 82);
        assert_eq!(carried(&app, player, &deposit), cycle_units(8, pm));
        assert!(carried(&app, player, &deposit) < per || per == 1);
    }

    /// Each way sampling ends says why.
    #[test]
    fn sampling_ends_by_stopping_moving_a_blow_or_a_full_pack() {
        // Stopping.
        let (mut app, player, _) = sampling_at(0.0);
        let heard = send(&mut app, SurveyRequest::Stop);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::Stopped)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());

        // Moving the dial's distance from where it began.
        let (mut app, player, _) = sampling_at(0.0);
        run(&mut app, 5);
        app.world_mut()
            .get_mut::<WorldPosition>(player)
            .unwrap()
            .0
            .0
            .x += 1.0;
        run(&mut app, 2);
        assert!(
            app.world().get::<Sampling>(player).is_some(),
            "1 m is within 1.5"
        );
        app.world_mut()
            .get_mut::<WorldPosition>(player)
            .unwrap()
            .0
            .0
            .x += 1.0;
        let heard = run(&mut app, 2);
        assert!(app.world().get::<Sampling>(player).is_none());
        assert_eq!(heard.notices, [SurveyNotice::SampleEnded(SampleEnd::Moved)]);

        // A blow on the player.
        let (mut app, player, _) = sampling_at(0.0);
        app.world_mut().write_message(CombatImpact {
            attacker: Entity::PLACEHOLDER,
            victim: Some(player),
            from: strata_core::WorldPos::ZERO,
            point: strata_core::WorldPos::ZERO,
            melee: false,
            dealt: 1.0,
            lethal: false,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        let heard = run(&mut app, 2);
        assert_eq!(heard.notices, [SurveyNotice::SampleEnded(SampleEnd::Hit)]);
        assert!(app.world().get::<Sampling>(player).is_none());

        // A pack with no room for the next cycle's lot.
        let (mut app, player, _) = sampling_at(0.0);
        {
            let items = app.world().resource::<Registries>().0.clone();
            let mut pack = app.world_mut().get_mut::<Pack>(player).unwrap();
            crate::crafting::tests::fill(&mut pack.cells, 18, &items.items);
            for bag in 0..2 {
                let open = usize::from(pack.open_cells(bag, &items.items));
                crate::crafting::tests::fill(&mut pack.bags[bag], open, &items.items);
            }
        }
        let heard = run(&mut app, 85);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::NoRoom)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());
    }

    /// The bike is called with `fire` held, and what the sim said of the call
    /// and whether the shot survived.
    fn call_the_bike(
        app: &mut App,
        player: Entity,
    ) -> (
        Result<crate::ride::RideDone, crate::ride::RideRefusal>,
        bool,
    ) {
        use crate::combat::CombatIntent;
        use crate::ride::{RideInbox, RideRequest};
        app.world_mut()
            .get_mut::<CombatIntent>(player)
            .unwrap()
            .fire = true;
        app.world_mut()
            .resource_mut::<RideInbox>()
            .0
            .push(RideRequest::Call);
        app.update();
        let answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<crate::ride::RideAnswer>>()
            .drain()
            .collect();
        (
            answers[0].result,
            app.world().get::<CombatIntent>(player).unwrap().fire,
        )
    }

    fn give_the_bike(app: &mut App) {
        let registries = app.world().resource::<Registries>().0.clone();
        let vehicle = registries.vehicles.iter().next().unwrap().id.clone();
        let bike = crate::ride::store_grade_bike(&registries.items, &vehicle).unwrap();
        *app.world_mut().resource_mut::<crate::ride::Stable>() =
            crate::ride::Stable::with_bike(bike, &registries.items, &registries.vehicles);
    }

    /// While pinging or sampling the combatant fires nothing and the bike
    /// cannot be called: both channels, each on its own.
    #[test]
    fn pinging_and_sampling_hold_fire_and_busy_the_bike() {
        use crate::ride::RideRefusal;
        let (mut app, player, deposit) = playing(0.0);
        let class = class_id(&app, &deposit);
        give_the_bike(&mut app);
        send(&mut app, SurveyRequest::Ping { class });
        assert!(app.world().get::<Pinging>(player).is_some());
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Err(RideRefusal::Busy));
        assert!(!fire, "no shot while pinging");

        // Sampling, once the ping has found the deposit.
        let (mut app, player, _) = sampling_at(0.0);
        give_the_bike(&mut app);
        assert!(app.world().get::<Sampling>(player).is_some());
        assert!(app.world().get::<Pinging>(player).is_none());
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Err(RideRefusal::Busy));
        assert!(!fire, "no shot while sampling");

        // Neither: the call is granted, the shot is the combatant's.
        let (mut app, player, _) = playing(0.0);
        give_the_bike(&mut app);
        let (result, fire) = call_the_bike(&mut app, player);
        assert_eq!(result, Ok(crate::ride::RideDone::Calling));
        assert!(fire, "a held shot survives a tick with no channel");
    }

    /// The first point in the world's sea, as far out as it takes.
    fn deep_water(app: &App) -> [f64; 2] {
        let source = app.world().resource::<WorldSourceRes>().source();
        let swim = app
            .world()
            .resource::<Registries>()
            .0
            .resources
            .survey()
            .ping
            .swim_m;
        for step in 1..80 {
            let r = f64::from(step) * 250.0;
            for k in 0..16 {
                let a = f64::from(k) * std::f64::consts::TAU / 16.0;
                let (x, z) = (r * a.cos(), r * a.sin());
                if water_here(source, x, z).depth_m() > swim + 1.0 {
                    return [x, z];
                }
            }
        }
        panic!("no deep water within 20 km");
    }

    /// Swimming ends a sampling and says so, even before the move away from
    /// where it began would have.
    #[test]
    fn a_sampling_over_deep_water_ends_as_swimming() {
        let (mut app, player, _) = sampling_at(0.0);
        let source = Arc::clone(world().0.world());
        app.insert_resource(WorldSourceRes(source));
        let [x, z] = deep_water(&app);
        {
            let mut at = app.world_mut().get_mut::<WorldPosition>(player).unwrap();
            at.0.0.x = x;
            at.0.0.z = z;
        }
        let heard = run(&mut app, 2);
        assert_eq!(
            heard.notices,
            [SurveyNotice::SampleEnded(SampleEnd::Swimming)]
        );
        assert!(app.world().get::<Sampling>(player).is_none());
        // And the same water refuses a new ping.
        let class = {
            let deposit = world().1.clone();
            class_id(&app, &deposit)
        };
        let heard = send(&mut app, SurveyRequest::Ping { class });
        assert_eq!(heard.pings[0].outcome, Err(SurveyRefusal::InWater));
    }

    /// A deposit to the east lies in the columns of greater x, a deposit to
    /// the north in the rows of greater z, and none in the other half.
    #[test]
    fn the_grid_puts_the_east_in_the_columns_of_greater_x() {
        let registries = registries();
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let graph = &registries.0.progression;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let skill = skill(set, items, graph, &tool, &CharacterSheet::default());
        let at = [1_000.0, 2_000.0];
        let east = made(set, SALT, [1, 0], [1_200.0, 2_000.0], 900);
        let class = class_of(set, &east);
        let deposits = [east.clone()];
        let result = ping_with(&covering_of(&deposits), set, &skill, at, &class);
        let side = result.grid as usize;
        let hit: Vec<usize> = result
            .points
            .iter()
            .enumerate()
            .filter(|(_, p)| p.deposit.is_some())
            .map(|(i, _)| i)
            .collect();
        assert!(!hit.is_empty());
        for i in &hit {
            let (row, column) = (i / side, i % side);
            assert!(column > side / 2, "column {column} is east of the player");
            assert!(row < side);
            assert!(result.points[*i].at[0] > at[0]);
        }
        assert!(
            result.points[..side * side]
                .iter()
                .enumerate()
                .filter(|(i, _)| i % side <= side / 2)
                .all(|(_, p)| p.deposit.is_none()),
            "nothing in the west half"
        );
        // The north: greater z is the higher rows.
        let north = made(set, SALT, [0, 1], [1_000.0, 2_200.0], 900);
        let deposits = [north];
        let result = ping_with(&covering_of(&deposits), set, &skill, at, &class);
        for (i, p) in result.points.iter().enumerate() {
            if p.deposit.is_some() {
                assert!(
                    i / side > side / 2,
                    "row {} is north of the player",
                    i / side
                );
            }
        }
        // The cell of a point east of the player is in the same columns.
        let (row, column) = result.cell_of([1_064.0, 2_000.0]).unwrap();
        assert_eq!((row, column), (2, 3));
    }

    fn sheet_with(registries: &Registries, boxes: &[&str]) -> CharacterSheet {
        let _ = registries;
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// The skill of the arrival, of Ilse Marrow and of a full tree, each
    /// number: anyone can ping untrained, the boxes add to it.
    #[test]
    fn the_surveyors_boxes_scale_the_ping_and_the_sample() {
        let registries = registries();
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let graph = &registries.0.progression;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let of = |sheet: &CharacterSheet| skill(set, items, graph, &tool, sheet);

        let household = registries.0.sheets.household.as_ref().unwrap();
        let arrival = CharacterSheet::authored(&household.arrival.sheet);
        let bare = of(&arrival);
        assert!((bare.range_m - 128.0).abs() < 1e-9);
        assert_eq!((bare.grid, bare.units, bare.reading), (5, 8, Reading::Type));
        assert!(close(bare.channel_s, 2.5) && close(bare.cycle_s, 4.0) && !bare.peak);

        let ilse = household
            .recruits
            .iter()
            .find(|member| member.name == "Ilse Marrow")
            .unwrap();
        let ilse = of(&CharacterSheet::authored(&ilse.sheet));
        assert!((ilse.range_m - 192.0).abs() < 1e-9, "128 + 64");
        assert_eq!(
            (ilse.grid, ilse.units, ilse.reading),
            (5, 9, Reading::Bands)
        );
        assert!(close(ilse.channel_s, 2.0) && close(ilse.cycle_s, 4.0) && !ilse.peak);

        let every: Vec<&str> = graph
            .boxes()
            .filter(|b| b.id.starts_with("surveyor."))
            .map(|b| b.id.as_str())
            .collect();
        let full = of(&sheet_with(&registries, &every));
        assert!((full.range_m - 448.0).abs() < 1e-9, "128 + 64 + 128 + 128");
        assert_eq!(
            (full.grid, full.units, full.reading),
            (7, 14, Reading::Exact)
        );
        assert!(close(full.channel_s, 1.2), "2.5 x 0.8 x 0.6");
        assert!(close(full.cycle_s, 3.2) && full.peak);

        // The tool's own line scales what the boxes made, after them.
        let mut fine = tool.clone();
        fine.lines.insert(RANGE.to_owned(), 1000);
        let fine = skill(set, items, graph, &fine, &sheet_with(&registries, &every));
        assert!((fine.range_m - 672.0).abs() < 1e-9, "448 x 1.5");

        // Reading is the best learned, never lower than the dials'.
        let two = of(&sheet_with(
            &registries,
            &["surveyor.novice", "surveyor.reading.2"],
        ));
        assert_eq!(two.reading, Reading::Bands.max(Reading::Exact));
        // The grid stays odd.
        assert_eq!(full.grid % 2, 1);
    }

    /// Each event's XP, and that a find, a better reading and a first
    /// sample pay once.
    #[test]
    fn survey_xp_pays_for_each_event_and_a_find_pays_once() {
        let registries = registries();
        let rates = &registries.0.progression.rates.survey;
        let ping = |found, better| survey_xp(rates, SurveyEvent::Ping { found, better });
        assert_eq!(ping(true, false), rates.find);
        assert_eq!(ping(false, true), rates.better);
        assert_eq!(ping(false, false), 0, "an empty ping earns nothing");
        let cycle = |first, pm| survey_xp(rates, SurveyEvent::Cycle { first, pm });
        assert_eq!(cycle(false, 1000), rates.cycle);
        assert_eq!(cycle(false, 500), rates.cycle / 2);
        assert_eq!(cycle(false, 20), 1, "at least one");
        assert_eq!(cycle(true, 1000), rates.cycle + rates.first_sample);

        // Learning: the first ping finds, the same ping again finds nothing,
        // and a stronger reading counts only past the dial.
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let skill = skill(
            set,
            items,
            &registries.0.progression,
            &tool,
            &CharacterSheet::default(),
        );
        let salt = made(set, SALT, [1, 1], [0.0, 0.0], 800);
        let class = class_of(set, &salt);
        let mut surveyed = Surveyed::default();
        let first = learn(
            &mut surveyed,
            &ping_with(
                &covering_of(std::slice::from_ref(&salt)),
                set,
                &skill,
                [0.0, 0.0],
                &class,
            ),
        );
        assert_eq!((first.new, first.raised_pm), (1, 0));
        let again = learn(
            &mut surveyed,
            &ping_with(
                &covering_of(std::slice::from_ref(&salt)),
                set,
                &skill,
                [0.0, 0.0],
                &class,
            ),
        );
        assert_eq!((again.new, again.raised_pm), (0, 0));
        let mut richer = salt.clone();
        richer.peak = 950;
        let better = learn(
            &mut surveyed,
            &ping_with(
                &covering_of(&[richer.clone()]),
                set,
                &skill,
                [0.0, 0.0],
                &class,
            ),
        );
        assert_eq!((better.new, better.raised_pm), (0, 150));
        assert!(better.raised_pm >= rates.better_pm);
        let same = learn(
            &mut surveyed,
            &ping_with(&covering_of(&[richer]), set, &skill, [0.0, 0.0], &class),
        );
        assert_eq!(same.raised_pm, 0, "the better reading pays once");
    }

    /// In the sim: a ping that finds earns Survey XP once, a second ping
    /// there earns none, and sampling pays a first bonus once.
    #[test]
    fn surveying_earns_survey_xp_in_the_tick() {
        let (mut app, player, deposit) = playing(0.0);
        app.world_mut()
            .entity_mut(player)
            .insert(CharacterSheet::default());
        let rates = app
            .world()
            .resource::<Registries>()
            .0
            .progression
            .rates
            .survey
            .clone();
        let class = class_id(&app, &deposit);
        send(
            &mut app,
            SurveyRequest::Ping {
                class: class.clone(),
            },
        );
        run(&mut app, 60);
        let xp = |app: &App| {
            app.world()
                .get::<CharacterSheet>(player)
                .unwrap()
                .xp(XpKind::Survey)
        };
        assert_eq!(xp(&app), rates.find);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        assert_eq!(
            xp(&app),
            rates.find,
            "pinging the same ground earns nothing"
        );

        send(&mut app, SurveyRequest::Sample);
        run(&mut app, 82);
        let each = survey_xp(
            &rates,
            SurveyEvent::Cycle {
                first: false,
                pm: deposit.peak,
            },
        );
        assert_eq!(xp(&app), rates.find + each + rates.first_sample);
        run(&mut app, 80);
        assert_eq!(xp(&app), rates.find + 2 * each + rates.first_sample);
    }

    /// A household whose record is not kept this run earns no Survey XP, so a
    /// restart cannot pay the same finds again.
    #[test]
    fn an_unkept_record_earns_no_survey_xp() {
        let (mut app, player, deposit) = playing(0.0);
        app.world_mut()
            .entity_mut(player)
            .insert(CharacterSheet::default());
        app.insert_resource(SurveyUnkept);
        let class = class_id(&app, &deposit);
        send(&mut app, SurveyRequest::Ping { class });
        run(&mut app, 60);
        send(&mut app, SurveyRequest::Sample);
        run(&mut app, 82);
        let sheet = app.world().get::<CharacterSheet>(player).unwrap();
        assert_eq!(sheet.xp(XpKind::Survey), 0);
    }

    /// With Peak a ping tells each met deposit's centre and the lead goes
    /// there; without it, neither.
    #[test]
    fn peak_tells_the_centre_and_sends_the_lead_there() {
        let registries = registries();
        let set = &registries.0.resources;
        let items = &registries.0.items;
        let graph = &registries.0.progression;
        let tool = best_tool(items, &arrival_pack(&registries)).unwrap();
        let mut skilled = skill(set, items, graph, &tool, &CharacterSheet::default());
        let salt = made(set, SALT, [1, 1], [1_160.0, 2_000.0], 900);
        let class = class_of(set, &salt);
        let deposits = [salt.clone()];
        let at = [1_000.0, 2_000.0];
        let plain = ping_with(&covering_of(&deposits), set, &skilled, at, &class);
        assert!(plain.found[0].richest.is_none());
        assert_ne!(plain.lead().unwrap().at, salt.at);
        skilled.peak = true;
        let peaked = ping_with(&covering_of(&deposits), set, &skilled, at, &class);
        let richest = peaked.found[0].richest.unwrap();
        assert_eq!((richest.at, richest.pm), (salt.at, 900));
        let lead = peaked.lead().unwrap();
        assert_eq!((lead.at, lead.pm), (salt.at, 900));
        // Nothing met, nothing told.
        let far = ping_with(
            &covering_of(&deposits),
            set,
            &skilled,
            [9_000.0, 9_000.0],
            &class,
        );
        assert!(far.found.is_empty() && far.lead().is_none());
    }
}
