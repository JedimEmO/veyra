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

use bevy_ecs::prelude::*;

use strata_registry::item::{ItemCatalogue, ItemKind, RANGE, Record, STORE_GRADE, ToolReads};

use strata_registry::progression::{Effect, SkillGraph, SurveyRates};

use strata_registry::{Attribute, GameRegistries, Namespaced, Reading, ResourceSet};

use super::{Deposit, DepositField, DepositId, Found, Lead, Surveyed, lot, with_lot};

use crate::items::Pack;

use crate::progression::{CharacterSheet, learned_effects, survey_field};

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

/// The sim's answer to a `SurveyRequest::Ping`,
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

/// The sim's answer to a `SurveyRequest::Sample`,
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
    #![allow(
        clippy::float_cmp,
        reason = "the lattice's points are exact sums of the spacing"
    )]

    use super::super::Deposit;
    use super::super::DepositField;
    use super::super::DepositId;
    use super::super::Found;

    use super::super::Surveyed;

    use super::*;

    use crate::items::Pack;
    use crate::progression::CharacterSheet;

    use std::sync::Arc;
    use std::sync::OnceLock;
    use strata_registry::Attribute;

    use strata_registry::Namespaced;
    use strata_registry::Reading;
    use strata_registry::ResourceSet;
    use strata_registry::ResourceTypeId;

    use strata_registry::item::RANGE;

    use strata_registry::item::SavedPack;

    use strata_types::Registries;

    pub fn registries() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    pub fn class_of(set: &ResourceSet, deposit: &Deposit) -> Namespaced {
        set.class_of(deposit.ty).id.clone()
    }

    /// A deposit of the type named, round `at`, with a radius of 200 m and
    /// a peak of `peak`: made by hand so no world need be baked.
    pub fn made(set: &ResourceSet, ty: &str, cell: [i32; 2], at: [f64; 2], peak: u16) -> Deposit {
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

    pub fn covering_of(deposits: &[Deposit]) -> impl Fn(f64, f64) -> Vec<(Deposit, u16)> + '_ {
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

    pub const SALT: &str = "strata:evaporite_salt";

    pub const ALLOY: &str = "strata:light_alloy_ore";

    pub fn standing() -> HandState {
        HandState {
            present: true,
            ..HandState::default()
        }
    }

    pub fn arrival_pack(registries: &Registries) -> Pack {
        let starting = registries.0.items.starting().unwrap();
        let arrival = &registries.0.sheets.household.as_ref().unwrap().arrival;
        Pack::from_saved(&starting.packs[&arrival.name], &registries.0.items).unwrap()
    }

    pub fn bare_pack(registries: &Registries) -> Pack {
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
        crate::crafting::test_support::fill(&mut full.cells, 18, &registries.0.items);
        for bag in 0..2 {
            let open = usize::from(full.open_cells(bag, &registries.0.items));
            crate::crafting::test_support::fill(&mut full.bags[bag], open, &registries.0.items);
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

    /// What the sim said over some ticks, collected tick by tick since
    /// a message lives two updates only.
    #[derive(Default)]
    pub struct Heard {
        pub pings: Vec<PingAnswer>,
        pub samples: Vec<SampleAnswer>,
        pub notices: Vec<SurveyNotice>,
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

    pub fn sheet_with(registries: &Registries, boxes: &[&str]) -> CharacterSheet {
        let _ = registries;
        CharacterSheet {
            learned: boxes.iter().map(|b| (*b).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    pub fn close(a: f32, b: f32) -> bool {
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
