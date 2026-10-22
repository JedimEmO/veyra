//! Fitting: what the modules on a weapon do to the row its holder fights
//! by, and the two requests that put one on a mount and take it off
//! (ticket 0007, step 2; `docs/design/crafting.md`, 4.2 to 4.5).
//!
//! A weapon in a hand is a record, and what is fitted to it is that
//! record's `fitted` map. [`factors`] reads the map into factors, each a
//! module's authored factor moved by the quality of its line
//! (`FittingRules::factor`); [`loadout`] is the factors of both hands that
//! the fight has a number for ([`live_on`]); and a `Combatant` equipped with
//! a loadout reads each weapon through [`merge`]: the tables' row, then the
//! sheet's boxes, then every factor once. The rest of a module's effects
//! stay in the data and are shown as what they would do, not faked.
//!
//! Everything a screen shows is worked out here. [`mounts`] says what each
//! mount holds, [`candidates`] what the stock has for one, [`with`] what the
//! weapon would be with a part changed, and [`lines`] the figures of that
//! against the figures now, in the game's units and already rounded as
//! they are read, so a change on the screen is the difference of the two
//! numbers beside it. A screen multiplies nothing.
//!
//! A fit and a removal are requests consumed in the tick (`FitInbox`,
//! `consume`), allowed with a workbench in reach, answered with what
//! happened or why not (`FitAnswer`). Each is one transaction on the
//! stock ([`fit`], [`remove`]): a refusal has changed nothing.
//! `sync_loadout` then equips the holder with what its hands now carry.
//! Local-only, like the stock: nothing here is replicated.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;

use slotted_model::{Inventory, ItemStack, Namespaced};

use strata_registry::item::{
    Better, EffectDef, Fitted, HANDS, ItemCatalogue, ItemKind, Property, Record, STORE_GRADE,
};

use crate::combat::{Action, Combatant, Weapon};

pub use crate::combat_loadout::{Factor, Loadout, Row, live_on, merge};

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::progression::Reach;

use crate::wear;

/// The gun's view of [`live_on`], kept for the tests that hold the two
/// together; a screen asks [`live_on`] or [`live_for`], which know the weapon.
#[cfg(test)]
const fn live(property: Property) -> bool {
    property.live()
}

/// [`live_on`] for the weapon `record` is; a record that is no weapon is
/// read as a gun.
#[must_use]
pub fn live_for(items: &ItemCatalogue, record: &Record, property: Property) -> bool {
    live_on(
        property,
        weapon_of(items, record).is_some_and(Weapon::is_melee),
    )
}

/// The weapon family `record` is, if it is a weapon.
#[must_use]
pub fn weapon_of(items: &ItemCatalogue, record: &Record) -> Option<Weapon> {
    items.family_of(record).and_then(Weapon::from_name)
}

/// Every effect fitted to `weapon`, banded by its module's quality, in the
/// order the weapon lists its mounts. A record that is no weapon has none.
#[must_use]
pub fn factors(items: &ItemCatalogue, weapon: &Record) -> Vec<Factor> {
    let Some(ItemKind::Weapon {
        mounts, signature, ..
    }) = items.get(&weapon.item).map(|def| &def.kind)
    else {
        return Vec::new();
    };
    let rules = items.fitting();
    let wear = items.wear();
    let mut factors = Vec::new();
    for mount in mounts {
        let Some(fitted) = weapon.fitted.get(&mount.slot) else {
            continue;
        };
        let Some(ItemKind::Module { effects, .. }) = items.get(&fitted.item).map(|def| &def.kind)
        else {
            continue;
        };
        // A worn part's quality slides toward nought, and a sound one is
        // read exactly as it was made.
        let slid = u32::from(wear::slide(
            wear,
            wear::condition_of(wear, fitted.condition),
        ));
        factors.extend(effects.iter().map(|effect| {
            // A record the catalogue passed has every line of its module.
            let line = fitted
                .lines
                .get(&effect.line)
                .copied()
                .unwrap_or(STORE_GRADE);
            let line = u16::try_from(u32::from(line) * (1000 - slid) / 1000).unwrap_or(line);
            Factor {
                property: effect.property,
                factor: rules.factor(effect, line),
            }
        }));
    }
    // The weapon's own signature line, after the parts and before the wear.
    // A line a record lacks reads as store grade, as elsewhere.
    if let Some(signature) = signature {
        let line = weapon
            .lines
            .get(&signature.line)
            .copied()
            .unwrap_or(STORE_GRADE);
        factors.push(Factor {
            property: signature.property,
            factor: signature.factor(line),
        });
    }
    // The weapon's own wear moves its own row toward the worst it has.
    let slid = wear::slide(wear, wear::condition_of(wear, weapon.condition));
    if slid > 0 {
        // A worn blade cuts softer; a worn gun's bolts keep their damage, as
        // they did before melee was a weapon.
        let melee = weapon_of(items, weapon).is_some_and(Weapon::is_melee);
        factors.extend(
            wear.worst
                .iter()
                .filter(|(property, _)| melee || **property != Property::HitDamage)
                .map(|(property, worst)| Factor {
                    property: *property,
                    factor: 1. + (worst - 1.) * f32::from(slid) / 1000.,
                }),
        );
    }
    factors
}

/// The loadout of the weapons in `hands`: for each, the fitted factors the
/// fight has a number for. Hands with nothing live fitted and no
/// signature line give an empty loadout, which equips as none at all.
#[must_use]
pub fn loadout(items: &ItemCatalogue, hands: &[Option<Record>; HANDS]) -> Loadout {
    let mut mods = BTreeMap::new();
    for record in hands.iter().flatten() {
        let Some(weapon) = weapon_of(items, record) else {
            continue;
        };
        let fitted: Vec<Factor> = factors(items, record)
            .into_iter()
            .filter(|factor| live_on(factor.property, weapon.is_melee()))
            .collect();
        if !fitted.is_empty() {
            mods.insert(weapon, fitted);
        }
    }
    Loadout {
        mods,
        count: items.fitting().count,
    }
}

/// Why the sim would not fit or remove. The words a player reads are the
/// screen's; this is what is the matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FitRefusal {
    /// There is no local player to hold a weapon.
    #[error("nobody is holding a weapon")]
    NoPlayer,
    /// No workbench is within reach.
    #[error("a weapon is worked on at a workbench")]
    NoWorkbench,
    /// The holder is in the middle of an action.
    #[error("the holder is busy")]
    Busy,
    /// The hand named holds no weapon.
    #[error("there is no weapon in that hand")]
    NoWeapon,
    /// The weapon has no such mount.
    #[error("the weapon has no such mount")]
    NoMount,
    /// The mount is shut on this example of the weapon.
    #[error("the mount opens at moddability {opens_at}")]
    Shut {
        /// The moddability it opens at.
        opens_at: u16,
    },
    /// The part is made for another mount.
    #[error("the part is made for another mount")]
    WrongMount,
    /// The part is no module, or not one this weapon takes.
    #[error("the part does not fit this weapon")]
    DoesNotFit,
    /// The mount already holds exactly this part.
    #[error("that part is fitted already")]
    AlreadyFitted,
    /// The stock holds no unit of the part.
    #[error("the part is not in stock")]
    NotInStock,
    /// The mount holds its standard part, which stays with the weapon.
    #[error("nothing is fitted there")]
    NothingFitted,
    /// The part that would come off has nowhere to go.
    #[error("there is no room for the part that comes off")]
    NoRoom,
}

/// `weapon` with `mount` holding `module`, or its standard part for
/// `None`: the record a fit or a removal would leave, and what a screen
/// previews before either is asked for. Pure; the stock is not consulted.
///
/// # Errors
/// Why the weapon would not take it: no weapon, no such mount, a shut one,
/// a part for another mount or another weapon, or the part it already has.
pub fn with(
    items: &ItemCatalogue,
    weapon: &Record,
    mount: &str,
    module: Option<&Fitted>,
) -> Result<Record, FitRefusal> {
    if !matches!(
        items.get(&weapon.item).map(|def| &def.kind),
        Some(ItemKind::Weapon { .. })
    ) {
        return Err(FitRefusal::NoWeapon);
    }
    match items.mount_opens(weapon, mount) {
        None => return Err(FitRefusal::NoMount),
        Some(Err(opens_at)) => return Err(FitRefusal::Shut { opens_at }),
        Some(Ok(())) => {}
    }
    let mut next = weapon.clone();
    let Some(module) = module else {
        next.fitted.remove(mount);
        return Ok(next);
    };
    let Some(ItemKind::Module { slot, fits, .. }) = items.get(&module.item).map(|def| &def.kind)
    else {
        return Err(FitRefusal::DoesNotFit);
    };
    if slot != mount {
        return Err(FitRefusal::WrongMount);
    }
    if !fits.contains(&weapon.item) {
        return Err(FitRefusal::DoesNotFit);
    }
    if weapon.fitted.get(mount) == Some(module) {
        return Err(FitRefusal::AlreadyFitted);
    }
    next.fitted.insert(mount.to_owned(), module.clone());
    // Whatever else the catalogue holds a record to: the module's own
    // lines, its maker.
    items
        .check_record(&next)
        .map_err(|_| FitRefusal::DoesNotFit)?;
    Ok(next)
}

/// `weapon` with every mount holding the part it was built with: what its
/// fitted figures are set against, and what the last removal leaves.
#[must_use]
pub fn bare_of(weapon: &Record) -> Record {
    let mut bare = weapon.clone();
    bare.fitted.clear();
    bare
}

/// `weapon` as it would read with `mount` holding `module`, or its
/// standard part for `None`, whether or not [`with`] would allow it: a
/// card and a list say what a part would do on a mount that is still shut,
/// and read the part that is fitted against the standard one. Only for
/// figures; a fit goes through [`with`].
#[must_use]
pub fn preview(
    items: &ItemCatalogue,
    weapon: &Record,
    mount: &str,
    module: Option<&Fitted>,
) -> Record {
    with(items, weapon, mount, module).unwrap_or_else(|_| {
        let mut shown = weapon.clone();
        match module {
            Some(module) => shown.fitted.insert(mount.to_owned(), module.clone()),
            None => shown.fitted.remove(mount),
        };
        shown
    })
}

/// `weapon` with the module on `mount` taken off, and the module: what a
/// removal would leave and what it would put away. Pure; the stock is not
/// consulted, so a removal can still find no room.
///
/// # Errors
/// No weapon, no such mount, or nothing but the standard part on it.
pub fn without(
    items: &ItemCatalogue,
    weapon: &Record,
    mount: &str,
) -> Result<(Record, Fitted), FitRefusal> {
    if !matches!(
        items.get(&weapon.item).map(|def| &def.kind),
        Some(ItemKind::Weapon { .. })
    ) {
        return Err(FitRefusal::NoWeapon);
    }
    if items.mount_opens(weapon, mount).is_none() {
        return Err(FitRefusal::NoMount);
    }
    let mut next = weapon.clone();
    let module = next.fitted.remove(mount).ok_or(FitRefusal::NothingFitted)?;
    Ok((next, module))
}

/// Whether `module` is made for `mount` of `weapon`'s kind.
fn made_for(items: &ItemCatalogue, module: &Namespaced, weapon: &Record, mount: &str) -> bool {
    matches!(
        items.get(module).map(|def| &def.kind),
        Some(ItemKind::Module { slot, fits, .. }) if slot == mount && fits.contains(&weapon.item)
    )
}

/// The hand whose weapon a loose `module` is measured on: the first of
/// `hands`, taken from `drawn` on, whose weapon the module is made for and
/// has its mount, open or shut. A part that fits both weapons is read on
/// the one in hand. `None` when nothing carried takes it.
#[must_use]
pub fn host(
    items: &ItemCatalogue,
    hands: &[Option<&Record>; HANDS],
    drawn: usize,
    module: &Namespaced,
) -> Option<usize> {
    let Some(ItemKind::Module { slot, .. }) = items.get(module).map(|def| &def.kind) else {
        return None;
    };
    (0..HANDS).map(|turn| (drawn + turn) % HANDS).find(|hand| {
        hands[*hand].is_some_and(|weapon| {
            made_for(items, module, weapon, slot) && items.mount_opens(weapon, slot).is_some()
        })
    })
}

/// The property a module is fitted for: its first benefit, else the first
/// thing it does. What a list leads a part with and orders it by.
#[must_use]
pub fn lead<'a>(items: &'a ItemCatalogue, module: &Namespaced) -> Option<&'a EffectDef> {
    let Some(ItemKind::Module { effects, .. }) = items.get(module).map(|def| &def.kind) else {
        return None;
    };
    let rules = items.fitting();
    effects
        .iter()
        .find(|effect| rules.is_benefit(effect))
        .or_else(|| effects.first())
}

/// What a mount holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountState {
    /// The part the weapon was built with.
    Standard,
    /// A module.
    Fitted(Fitted),
    /// Nothing but its standard part, and it takes nothing until the
    /// weapon's moddability reaches the threshold.
    Shut {
        /// The moddability it opens at.
        opens_at: u16,
    },
}

/// Every mount of `weapon` with what it holds, in the order its definition
/// lists them. Empty for a record that is no weapon.
#[must_use]
pub fn mounts(items: &ItemCatalogue, weapon: &Record) -> Vec<(String, MountState)> {
    let Some(ItemKind::Weapon { mounts, .. }) = items.get(&weapon.item).map(|def| &def.kind) else {
        return Vec::new();
    };
    mounts
        .iter()
        .map(|mount| {
            let state = match (
                items.mount_opens(weapon, &mount.slot),
                weapon.fitted.get(&mount.slot),
            ) {
                (Some(Err(opens_at)), _) => MountState::Shut { opens_at },
                (_, Some(fitted)) => MountState::Fitted(fitted.clone()),
                (_, None) => MountState::Standard,
            };
            (mount.slot.clone(), state)
        })
        .collect()
}

/// A module the stock holds for a mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The module, as a fit names it.
    pub module: Fitted,
    /// How many of it the character carries, in the pack and its bags.
    pub carried: u32,
    /// How many the household's store holds.
    pub stored: u32,
}

/// A stack of one of `module`, loose.
pub fn loose(module: &Fitted) -> Record {
    Record {
        item: module.item.clone(),
        count: 1,
        batch: module.batch,
        by: module.by.clone(),
        lines: module.lines.clone(),
        condition: module.condition,
        fitted: BTreeMap::new(),
        origin: None,
    }
}

/// [`loose`] for the wear tests, which put a worn module in the pack.
#[cfg(any(test, feature = "test-support"))]
pub fn loose_for_tests(module: &Fitted) -> Record {
    loose(module)
}

/// The cells of `pack` a stack can be in: its own, then each bag's open
/// ones.
pub fn carried_cells<'a>(
    pack: &'a Pack,
    items: &ItemCatalogue,
) -> impl Iterator<Item = &'a ItemStack> + use<'a> {
    let open: [usize; strata_registry::item::BAG_SLOTS] =
        std::array::from_fn(|bag| usize::from(pack.open_cells(bag, items)));
    pack.cells.slots().iter().flatten().chain(
        pack.bags
            .iter()
            .zip(open)
            .flat_map(|(bag, open)| bag.slots().iter().take(open).flatten()),
    )
}

/// How far a module's first benefit moves its property, for the order of a
/// list: nought for a module with no benefit.
fn worth(items: &ItemCatalogue, module: &Fitted) -> f32 {
    let rules = items.fitting();
    lead(items, &module.item)
        .filter(|effect| rules.is_benefit(effect))
        .map_or(0., |effect| {
            (rules.factor(effect, lead_quality(module, effect)) - 1.).abs()
        })
}

/// The quality of the line of `module` that drives `effect`: the figure
/// that tells two batches of one part apart where their effects read the
/// same.
#[must_use]
pub fn lead_quality(module: &Fitted, effect: &EffectDef) -> u16 {
    module
        .lines
        .get(&effect.line)
        .copied()
        .unwrap_or(STORE_GRADE)
}

/// The modules made for `mount` of `weapon` that the pack, its bags and,
/// when given, the household's store hold, each batch once with its
/// counts, best first: by how far its first benefit moves its property,
/// then store grade before a batch and a batch before a later one.
///
/// The list is what exists for the mount, whatever the mount's state: a
/// shut mount and the part already fitted are refusals of [`with`], which
/// a screen shows at the candidate.
#[must_use]
pub fn candidates(
    items: &ItemCatalogue,
    pack: &Pack,
    store: Option<&HouseholdStore>,
    weapon: &Record,
    mount: &str,
) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let stored = store
        .into_iter()
        .flat_map(|store| store.0.slots().iter().flatten())
        .map(|stack| (stack, true));
    let all = carried_cells(pack, items)
        .map(|stack| (stack, false))
        .chain(stored);
    for (stack, in_store) in all {
        let Ok(record) = items::read(stack, items) else {
            continue;
        };
        if !made_for(items, &record.item, weapon, mount) {
            continue;
        }
        let module = Fitted {
            item: record.item,
            batch: record.batch,
            by: record.by,
            lines: record.lines,
            condition: record.condition,
        };
        let index = found
            .iter()
            .position(|candidate| candidate.module == module)
            .unwrap_or_else(|| {
                found.push(Candidate {
                    module,
                    carried: 0,
                    stored: 0,
                });
                found.len() - 1
            });
        let count = if in_store {
            &mut found[index].stored
        } else {
            &mut found[index].carried
        };
        *count += record.count;
    }
    // Stable, so two that tie on everything keep the order they were met
    // in: the pack's before the store's.
    found.sort_by(|a, b| {
        worth(items, &b.module)
            .total_cmp(&worth(items, &a.module))
            .then_with(|| a.module.batch.cmp(&b.module.batch))
            .then_with(|| a.module.item.cmp(&b.module.item))
    });
    found
}

/// What a figure is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Rounds in a magazine.
    Rounds,
    /// Seconds.
    Seconds,
    /// A share of the holder's own pace or cost, in percent.
    Percent,
    /// Points of health, posture or stamina.
    Points,
    /// Degrees of the half angle of the cone a bolt leaves in.
    Degrees,
    /// Metres a melee weapon reaches.
    Metres,
    /// Percent of the weapon's standard value, for a property the fight
    /// has no number for: all that can be said of it.
    OfStandard,
}

/// A figure as it is read: a whole number of the unit's smallest shown
/// step. `149` at two decimals is 1.49.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount {
    /// The figure, in steps of `10^-decimals`.
    pub number: i32,
    /// How many of its digits follow the point.
    pub decimals: u8,
    /// What it counts.
    pub unit: Unit,
}

impl Amount {
    /// `value` rounded to `decimals`, which is where a figure is rounded
    /// and nowhere else: in `f64`, since 1.035 in `f32` must read 1.03 and
    /// 1.485 must read 1.49, as the sim's own numbers do.
    pub fn of(value: f32, decimals: u8, unit: Unit) -> Self {
        let scaled = f64::from(value) * 10_f64.powi(i32::from(decimals));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a combat figure of a few digits, rounded"
        )]
        let number = scaled.round() as i32;
        Self {
            number,
            decimals,
            unit,
        }
    }

    /// `steps` of this figure's smallest step as digits, without a unit
    /// or a sign of its own beyond a minus: `149` reads `1.49`.
    fn digits(self, steps: i32) -> String {
        let scale = 10_u32.pow(u32::from(self.decimals));
        let whole = steps.unsigned_abs() / scale;
        let sign = if steps < 0 { "-" } else { "" };
        if self.decimals == 0 {
            format!("{sign}{whole}")
        } else {
            let part = steps.unsigned_abs() % scale;
            let width = usize::from(self.decimals);
            format!("{sign}{whole}.{part:0width$}")
        }
    }

    /// The figure as digits, without its unit: `29`, `1.49`.
    #[must_use]
    pub fn text(self) -> String {
        self.digits(self.number)
    }

    /// A change of `by` steps in this figure as signed digits: `+5`,
    /// `+0.14`, `-0.02`.
    #[must_use]
    pub fn signed(self, by: i32) -> String {
        let sign = if by > 0 { "+" } else { "" };
        format!("{sign}{}", self.digits(by))
    }
}

/// How a figure differs from the one it is set against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// The difference, in the figure's steps.
    pub by: i32,
    /// Whether it is for the better: more rounds, a shorter reload.
    pub better: bool,
}

/// One property of a weapon, as a screen shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// Which property.
    pub property: Property,
    /// Whether the fight has a number for it; else the figure is a share
    /// of standard and belongs under "not in the fight yet".
    pub live: bool,
    /// The figure.
    pub value: Amount,
    /// How it differs from what it is set against; none when it does not.
    pub change: Option<Change>,
}

impl Line {
    /// For a property the fight has no number for: how many percent off
    /// the weapon's standard the figure is, below it when negative and
    /// nought when it is standard. `None` for a figure in a unit of its
    /// own.
    #[must_use]
    pub fn off_standard(&self) -> Option<i32> {
        (self.value.unit == Unit::OfStandard).then(|| self.value.number - STANDARD_PERCENT)
    }
}

/// A weapon's standard, in the percent a figure with no number in the
/// fight is counted in.
const STANDARD_PERCENT: i32 = 100;

/// The figure of `property` on a weapon whose merged row is `row` and
/// whose fitted factors are `fitted`.
fn amount(property: Property, row: &Row, fitted: &[Factor], melee: bool) -> Amount {
    match property {
        Property::RecoveryTime if melee => Amount::of(row.tail * row.recovery, 2, Unit::Seconds),
        // A melee row holds one where a gun's holds points, so a blade's
        // figure is the share of its class's own cut.
        Property::HitDamage if melee => Amount::of(row.spec.damage * 100., 0, Unit::Percent),
        Property::HitPosture if melee => Amount::of(row.spec.posture * 100., 0, Unit::Percent),
        // A blade has no magazine: its capacity stays in the data.
        Property::Capacity if melee => of_standard(property, fitted),
        Property::Capacity => Amount {
            number: i32::from(row.spec.magazine),
            decimals: 0,
            unit: Unit::Rounds,
        },
        Property::ReloadTime => Amount::of(row.spec.reload, 2, Unit::Seconds),
        Property::RepeatInterval => Amount::of(row.spec.interval, 3, Unit::Seconds),
        Property::EquipTime => Amount::of(row.draw, 2, Unit::Seconds),
        Property::AimMoveSpeed => Amount::of(row.aim_speed * 100., 0, Unit::Percent),
        Property::HitDamage => Amount::of(row.spec.damage, 0, Unit::Points),
        Property::HitPosture => Amount::of(row.spec.posture, 0, Unit::Points),
        Property::AttackStamina => Amount::of(row.cut_stamina * 100., 0, Unit::Percent),
        Property::AimSpread => Amount::of(row.spec.aim_spread, 2, Unit::Degrees),
        Property::RecoilRecovery
        | Property::CoolingRate
        | Property::BraceTime
        | Property::RecoveryTime => of_standard(property, fitted),
    }
}

/// A figure with no number in the fight: the product of the factors fitted
/// on it, in percent of the weapon's standard.
fn of_standard(property: Property, fitted: &[Factor]) -> Amount {
    let composed: f32 = fitted
        .iter()
        .filter(|factor| factor.property == property)
        .map(|factor| factor.factor)
        .product();
    Amount::of(composed * 100., 0, Unit::OfStandard)
}

/// The figures of `then` for each property of `which`, each with its
/// change against the same figure of `now`: a weapon as it would be
/// against the weapon as it is, or as it is against the same weapon with
/// nothing fitted. `bare` is the holder's row for the weapon with nothing
/// fitted (`Combatant::bare`).
///
/// A figure is rounded as it is read and the change is the difference of
/// the two rounded figures, so what a screen shows adds up: 1.35 and
/// +0.14 beside 1.49.
#[must_use]
pub fn lines(
    items: &ItemCatalogue,
    bare: &Row,
    now: &Record,
    then: &Record,
    which: &[Property],
) -> Vec<Line> {
    let rules = items.fitting();
    let figures = |record: &Record| {
        let fitted = factors(items, record);
        (merge(bare, &fitted, rules.count), fitted)
    };
    let (now_row, now_fitted) = figures(now);
    let (then_row, then_fitted) = figures(then);
    let melee = weapon_of(items, now).is_some_and(Weapon::is_melee);
    which
        .iter()
        .map(|&property| {
            let value = amount(property, &then_row, &then_fitted, melee);
            let by = value.number - amount(property, &now_row, &now_fitted, melee).number;
            let higher = rules.better.get(&property) != Some(&Better::Lower);
            Line {
                property,
                live: live_on(property, melee),
                value,
                change: (by != 0).then_some(Change {
                    by,
                    better: (by > 0) == higher,
                }),
            }
        })
        .collect()
}

/// A figure of a melee weapon's cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutFigure {
    /// The light cut's damage.
    LightDamage,
    /// The light cut's length, in seconds.
    LightTime,
    /// The light cut's stamina.
    LightStamina,
    /// The heavy's damage.
    HeavyDamage,
    /// The heavy's posture damage.
    HeavyPosture,
    /// Seconds from the end of the light cut's contact to its end, which a
    /// part's recovery moves.
    LightRecovery,
    /// Seconds to draw it.
    Draw,
    /// How far the weapon reaches, in metres.
    Reach,
    /// The share of a hit's damage its raised guard takes off, in percent.
    GuardShare,
    /// Seconds after the guard is raised in which a hit is parried.
    ParryWindow,
}

/// The figures of a melee weapon as its holder would cut with it: the
/// holder's rows after the sheet's boxes, the class in the tables, then
/// what the record's edge and wear do to them. Read off a `Combatant`
/// carrying it, so a screen shows the numbers the fight uses. Empty for a
/// record that is no melee weapon.
#[must_use]
pub fn cut_lines(
    items: &ItemCatalogue,
    holder: &Combatant,
    record: &Record,
) -> Vec<(CutFigure, Amount)> {
    let Some(weapon) = weapon_of(items, record).filter(|weapon| weapon.is_melee()) else {
        return Vec::new();
    };
    let mut actor = holder.clone().carrying([Some(weapon), None], 0);
    actor.equip(&loadout(items, &[Some(record.clone()), None]));
    let (light, heavy) = (actor.spec(Action::Light), actor.spec(Action::Heavy));
    let draw = actor.fitted_draw(weapon);
    let reach = crate::combat_tables::with(|tables| tables.melee.reach_m(weapon));
    let guard = holder.guard_row(weapon);
    let mut lines = vec![
        (
            CutFigure::LightDamage,
            Amount::of(light.damage, 0, Unit::Points),
        ),
        (
            CutFigure::LightTime,
            Amount::of(light.duration, 2, Unit::Seconds),
        ),
        (
            CutFigure::LightRecovery,
            Amount::of(light.duration - light.contact_end, 2, Unit::Seconds),
        ),
        (
            CutFigure::LightStamina,
            Amount::of(light.stamina, 0, Unit::Points),
        ),
        (CutFigure::Draw, Amount::of(draw, 2, Unit::Seconds)),
        (
            CutFigure::HeavyDamage,
            Amount::of(heavy.damage, 0, Unit::Points),
        ),
        (
            CutFigure::HeavyPosture,
            Amount::of(heavy.posture, 0, Unit::Points),
        ),
        (CutFigure::Reach, Amount::of(reach, 1, Unit::Metres)),
    ];
    if let Some(guard) = guard {
        lines.push((
            CutFigure::GuardShare,
            Amount::of(guard.share * 100., 0, Unit::Percent),
        ));
        lines.push((
            CutFigure::ParryWindow,
            Amount::of(guard.parry, 2, Unit::Seconds),
        ));
    }
    lines
}

/// The weapon in `hand`, as a record.
pub fn held(items: &ItemCatalogue, hands: &Hands, hand: usize) -> Result<Record, FitRefusal> {
    hands
        .0
        .get(hand)
        .and_then(|stack| items::read(stack, items).ok())
        .ok_or(FitRefusal::NoWeapon)
}

/// Take one unit of `wanted`'s kind out of the first stack of it in the
/// stock: the pack's cells, then its bags' open cells, then the store.
pub(crate) fn take_one(items: &ItemCatalogue, stock: &mut Stock<'_>, wanted: &ItemStack) -> bool {
    let open: [u16; strata_registry::item::BAG_SLOTS] =
        std::array::from_fn(|bag| stock.pack.open_cells(bag, items));
    let Pack { cells, bags, .. } = &mut *stock.pack;
    let all = cells.len();
    let mut sources: Vec<(&mut Inventory, usize)> = vec![(cells, all)];
    for (bag, open) in bags.iter_mut().zip(open) {
        sources.push((bag, usize::from(open)));
    }
    if let Some(store) = stock.store.as_mut() {
        let all = store.0.len();
        sources.push((&mut store.0, all));
    }
    for (inventory, open) in sources {
        for cell in 0..open {
            let Some(present) = inventory.get(cell) else {
                continue;
            };
            if !present.same_kind(wanted) {
                continue;
            }
            let mut rest = present.clone();
            rest.count -= 1;
            inventory.set(cell, (!rest.is_empty()).then_some(rest));
            // The dirty mask is the sync layer's and part of an
            // inventory's equality; nothing here reads it.
            inventory.clear_changed();
            return true;
        }
    }
    false
}

/// The stock, copied, for a transaction that must change all of it or
/// none.
struct Scratch {
    pack: Pack,
    hands: Hands,
    store: Option<HouseholdStore>,
}

impl Scratch {
    fn of(stock: &Stock<'_>) -> Self {
        Self {
            pack: stock.pack.clone(),
            hands: stock.hands.clone(),
            store: stock.store.as_deref().cloned(),
        }
    }

    fn stock(&mut self) -> Stock<'_> {
        Stock {
            saddle: None,
            pack: &mut self.pack,
            hands: &mut self.hands,
            store: self.store.as_mut(),
        }
    }

    /// Write `weapon` into `hand`.
    fn hold(
        &mut self,
        items: &ItemCatalogue,
        hand: usize,
        weapon: &Record,
    ) -> Result<(), FitRefusal> {
        let stack = items::write(weapon, items).map_err(|_| FitRefusal::DoesNotFit)?;
        self.hands.0.set(hand, Some(stack));
        self.hands.0.clear_changed();
        Ok(())
    }

    /// Put a module that came off a mount away: with what the character
    /// carries when there is room, and only then in the store. The stock's
    /// own rule ([`items::place`]) would put it onto a stack of its kind in
    /// the store before an empty cell of the pack, which is right for a
    /// stack sent across and wrong for a part just taken off a weapon in
    /// the hand, so the rule is asked twice: without the store, then with.
    fn put_away(&mut self, items: &ItemCatalogue, module: &Fitted) -> Result<(), FitRefusal> {
        let stack = items::write(&loose(module), items).map_err(|_| FitRefusal::NoRoom)?;
        let mut carried = Stock {
            saddle: None,
            pack: &mut self.pack,
            hands: &mut self.hands,
            store: None,
        };
        let Some(rest) = items::place(items, &mut carried, stack) else {
            return Ok(());
        };
        match items::place(items, &mut self.stock(), rest) {
            None => Ok(()),
            Some(_) => Err(FitRefusal::NoRoom),
        }
    }

    fn commit(self, stock: &mut Stock<'_>) {
        *stock.pack = self.pack;
        *stock.hands = self.hands;
        if let (Some(into), Some(store)) = (stock.store.as_mut(), self.store) {
            **into = store;
        }
    }
}

/// Fit one unit of `module` to `mount` of the weapon in `hand`: taken from
/// the first stack of it in the pack, its bags or the store, written into
/// the weapon's record, and whatever module the mount held put away in its
/// place ([`items::place`]). One transaction: a refusal has changed
/// nothing.
///
/// # Errors
/// Why not: what [`with`] refuses, a part the stock has not got, or no
/// room for the part that comes off.
pub fn fit(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    hand: usize,
    mount: &str,
    module: &Fitted,
) -> Result<(), FitRefusal> {
    let weapon = held(items, stock.hands, hand)?;
    let next = with(items, &weapon, mount, Some(module))?;
    let wanted = items::write(&loose(module), items).map_err(|_| FitRefusal::DoesNotFit)?;
    let mut scratch = Scratch::of(stock);
    // The unit leaves its cell before the old part looks for one, so a
    // swap of two singles needs no spare cell.
    if !take_one(items, &mut scratch.stock(), &wanted) {
        return Err(FitRefusal::NotInStock);
    }
    scratch.hold(items, hand, &next)?;
    if let Some(old) = weapon.fitted.get(mount) {
        scratch.put_away(items, old)?;
    }
    scratch.commit(stock);
    Ok(())
}

/// Take the module off `mount` of the weapon in `hand`, leaving its
/// standard part, and put it away: the pack, then a bag, then the store.
/// One transaction: a refusal has changed nothing.
///
/// # Errors
/// Why not: no weapon, no such mount, nothing fitted there, or no room
/// for the module.
pub fn remove(
    items: &ItemCatalogue,
    stock: &mut Stock<'_>,
    hand: usize,
    mount: &str,
) -> Result<Fitted, FitRefusal> {
    let weapon = held(items, stock.hands, hand)?;
    let (next, module) = without(items, &weapon, mount)?;
    let mut scratch = Scratch::of(stock);
    scratch.hold(items, hand, &next)?;
    scratch.put_away(items, &module)?;
    scratch.commit(stock);
    Ok(module)
}

/// From where a weapon is worked on.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkbenchAccess {
    /// Standing at a workbench, which is the rule.
    #[default]
    AtWorkbench,
    /// From anywhere: a run that opens the fitting screen for a still has
    /// no workbench to stand at.
    Anywhere,
}

/// Whether a weapon may be worked on from where the player stands: with a
/// workbench in reach, or from anywhere when the run says so. The rule a
/// request is held to, and what a screen greys its buttons by.
#[must_use]
pub fn at_workbench(access: WorkbenchAccess, reach: &Reach) -> bool {
    access == WorkbenchAccess::Anywhere || reach.workbench
}

/// What the weapon in `hand` holds when full, as its holder will read it
/// once equipped with what `hands` now carry: the same merge on the same
/// bare row. With no `Combatant` to hold it, the installed tables' row.
pub fn magazine(
    items: &ItemCatalogue,
    hands: &Hands,
    hand: usize,
    actor: Option<&Combatant>,
) -> u16 {
    let Ok(record) = held(items, hands, hand) else {
        return 0;
    };
    let Some(weapon) = weapon_of(items, &record) else {
        return 0;
    };
    let bare = actor.map_or_else(
        || Combatant::default().bare(weapon),
        |actor| actor.bare(weapon),
    );
    let fitted = factors(items, &record);
    merge(&bare, &fitted, items.fitting().count).spec.magazine
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
        reason = "a merged row is the product of two table values exactly; a bit off is a wrong merge"
    )]

    use super::*;
    use crate::combat::Action;
    use crate::combat::CombatIntent;
    use crate::combat::CombatMotion;
    use crate::combat::Combatant;
    use crate::combat::Weapon;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;

    use crate::items::Pack;
    use crate::items::Stock;
    use crate::progression::CharacterSheet;

    use crate::progression::Reach;

    use slotted_model::ItemStack;

    use slotted_model::Namespaced;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use strata_registry::item::CountRule;

    use strata_registry::item::Fitted;
    use strata_registry::item::HANDS;
    use strata_registry::item::ItemCatalogue;

    use strata_registry::item::MODDABILITY;
    use strata_registry::item::Property;
    use strata_registry::item::Record;
    use strata_registry::item::STORE_GRADE;
    use strata_registry::progression::SkillGraph;
    use strata_types::PlayerIntent;
    use strata_types::Registries;

    pub const FEED: &str = "veyra:module.patrol_extended_feed";

    pub const CELL: &str = "veyra:module.pistol_extended_cell";

    pub const REFLEX: &str = "veyra:module.vigil_reflex";

    pub const GRIP: &str = "veyra:module.vigil_angled_grip";

    pub const COMPENSATOR: &str = "veyra:module.vigil_compensator";

    pub const OPTIC: &str = "veyra:module.vigil_long_optic";

    pub const COMPACT: &str = "veyra:module.marksman_compact_feed";

    pub const STOCK: &str = "veyra:module.vigil_precision_stock";

    pub const REST: &str = "veyra:module.marksman_field_rest";

    pub const HAUL: &str = "veyra:bag.haul_pack";

    pub fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).unwrap()
    }

    /// A module as the dealer sells it: every line at store grade.
    pub fn bought(items: &ItemCatalogue, name: &str) -> Fitted {
        graded(items, name, STORE_GRADE)
    }

    /// A module with every line at `grade`.
    pub fn graded(items: &ItemCatalogue, name: &str, grade: u16) -> Fitted {
        let def = items.get(&id(name)).unwrap();
        Fitted {
            item: def.id.clone(),
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .map(|line| (line.id.clone(), grade))
                .collect(),
            condition: None,
        }
    }

    /// The patrol magazine Tovan Reyes made, as the first day's store has
    /// it.
    pub fn batch_412(items: &ItemCatalogue) -> Fitted {
        let record = &items.starting().unwrap().store[1];
        assert_eq!(
            (record.batch, record.item.to_string().as_str()),
            (412, FEED)
        );
        Fitted {
            item: record.item.clone(),
            batch: record.batch,
            by: record.by.clone(),
            lines: record.lines.clone(),
            condition: None,
        }
    }

    /// The first day's weapons: the carbine, then the pistol.
    pub fn weapons(items: &ItemCatalogue) -> [Record; HANDS] {
        items.starting().unwrap().hands.clone()
    }

    /// An actor on the shipped tables holding `carbine` and `pistol`.
    pub fn holding(items: &ItemCatalogue, carbine: &Record, pistol: &Record) -> Combatant {
        let mut actor = Combatant::default();
        actor.equip(&loadout(
            items,
            &[Some(carbine.clone()), Some(pistol.clone())],
        ));
        actor
    }

    /// Whether a loadout holds the bought carbine's action line and nothing
    /// else: a carbine with no part fitted still reads its own interval.
    pub fn only_the_action(loadout: Option<&Loadout>) -> bool {
        loadout.is_some_and(|loadout| {
            loadout.mods.iter().all(|(weapon, mods)| {
                *weapon == Weapon::Carbine
                    && mods.len() == 1
                    && mods[0].property == Property::RepeatInterval
            })
        })
    }

    /// A row's numbers as their bits, so equal means equal to the bit.
    pub fn bits(row: &Row) -> [u32; 14] {
        [
            row.spec.interval.to_bits(),
            row.spec.bolt_speed.to_bits(),
            row.spec.damage.to_bits(),
            row.spec.posture.to_bits(),
            u32::from(row.spec.magazine),
            row.spec.reload.to_bits(),
            row.draw.to_bits(),
            row.aim_speed.to_bits(),
            row.cut_stamina.to_bits(),
            row.recovery.to_bits(),
            row.tail.to_bits(),
            u32::from(row.melee),
            row.spec.spread.to_bits(),
            row.spec.aim_spread.to_bits(),
        ]
    }

    /// The row `actor` fights `weapon` by, read through its public reads.
    pub fn read(actor: &Combatant, weapon: Weapon) -> Row {
        let mut drawing = actor.clone();
        drawing.weapon = if weapon == Weapon::Carbine {
            Weapon::Pistol
        } else {
            Weapon::Carbine
        };
        drawing.action = Action::Swap;
        let mut cutting = actor.clone();
        cutting.weapon = weapon;
        let bare = actor.bare(weapon);
        let light = cutting.spec(Action::Light).stamina;
        Row {
            spec: actor.spec_of(weapon),
            draw: drawing.duration(),
            aim_speed: bare.aim_speed,
            cut_stamina: light,
            recovery: bare.recovery,
            tail: bare.tail,
            melee: bare.melee,
        }
    }

    #[test]
    fn the_store_feed_gives_29_rounds_and_1_485_s() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let fitted = with(items, &carbine, "feed", Some(&bought(items, FEED))).unwrap();
        let bare = Combatant::default();
        let actor = holding(items, &fitted, &pistol);
        assert_eq!(bare.magazine(), 24);
        assert_eq!(actor.magazine(), 29);
        assert_eq!(actor.reload_time(), 1.35_f32 * 1.1_f32);
        assert!((actor.reload_time() - 1.485).abs() < 1e-4);
        // Nothing else of the carbine moved but its own action, a bought
        // one's 1.10 of the table's interval, and nothing of the pistol.
        let (now, was) = (actor.weapon_spec(), bare.weapon_spec());
        assert!((now.interval - 0.462).abs() < 1e-6, "{}", now.interval);
        assert_eq!(
            (now.bolt_speed, now.damage, now.posture),
            (was.bolt_speed, was.damage, was.posture)
        );
        assert_eq!(actor.spec_of(Weapon::Pistol), Weapon::Pistol.spec());
        assert_eq!(
            actor.loadout().unwrap().mods.keys().collect::<Vec<_>>(),
            [&Weapon::Carbine]
        );
        // The ammunition was full at 24 and is not topped up by the fit.
        assert_eq!((actor.ammo, actor.holstered_ammo), (24, 10));
    }

    #[test]
    fn quality_runs_27_to_31_and_batch_412_is_29() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let row = |module: &Fitted| {
            let fitted = with(items, &carbine, "feed", Some(module)).unwrap();
            let actor = holding(items, &fitted, &pistol);
            (actor.magazine(), actor.reload_time())
        };
        let near = |seconds: f32, expected: f32| (seconds - expected).abs() < 1e-4;
        let (rounds, reload) = row(&graded(items, FEED, 0));
        assert!(rounds == 26 && near(reload, 1.5525), "{rounds} {reload}");
        let (rounds, reload) = row(&graded(items, FEED, 1000));
        assert!(rounds == 32 && near(reload, 1.4175), "{rounds} {reload}");
        // Capacity 610 is a factor of 1.2264, which is still 29 rounds, and
        // handling 433 a reload a hair longer than the bought one's.
        let made = batch_412(items);
        let (rounds, reload) = row(&made);
        assert!(rounds == 29 && near(reload, 1.4940), "{rounds} {reload}");
        let fitted = with(items, &carbine, "feed", Some(&made)).unwrap();
        let capacity = factors(items, &fitted)[0];
        assert_eq!(capacity.property, Property::Capacity);
        assert!((capacity.factor - 1.2264).abs() < 1e-6);
        // The 30th round needs capacity 622.
        let mut fine = made.clone();
        fine.lines.insert("capacity".to_owned(), 621);
        assert_eq!(row(&fine).0, 29);
        fine.lines.insert("capacity".to_owned(), 622);
        assert_eq!(row(&fine).0, 30);
    }

    #[test]
    fn the_carbines_action_line_moves_its_interval_and_the_pistol_has_none() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let interval = |at: u16| {
            let mut record = carbine.clone();
            record.lines.insert("interval".to_owned(), at);
            let own = factors(items, &record);
            assert_eq!(own.len(), 1, "the signature alone: {own:?}");
            assert_eq!(own[0].property, Property::RepeatInterval);
            own[0].factor
        };
        for (at, expected) in [(0, 1.30), (500, 1.10), (750, 1.00), (1000, 0.90)] {
            assert!((interval(at) - expected).abs() < 1e-6, "{at}");
        }
        // A line a record lacks reads as store grade.
        let mut lacking = carbine.clone();
        lacking.lines.remove("interval");
        assert_eq!(factors(items, &lacking), factors(items, &carbine));
        assert!(factors(items, &pistol).is_empty());
        // The sight goes after the signature's neighbours in mount order
        // and the signature after every part, before the wear.
        let sighted = with(items, &carbine, "sight", Some(&bought(items, REFLEX))).unwrap();
        assert_eq!(
            factors(items, &sighted).last().map(|f| f.property),
            Some(Property::RepeatInterval)
        );
        // BARE keeps it, and an actor with no record reads the table.
        assert_eq!(factors(items, &bare_of(&sighted)), factors(items, &carbine));
        assert_eq!(
            Combatant::default().weapon_spec().interval,
            Weapon::Carbine.spec().interval
        );
        let actor = holding(items, &carbine, &pistol);
        assert!((actor.weapon_spec().interval - 0.462).abs() < 1e-6);
    }

    /// The marksman rifle as the dealer sells it: every line at store grade.
    pub fn rifle(items: &ItemCatalogue) -> Record {
        let record = Record {
            item: id("veyra:weapon.marksman_rifle"),
            count: 1,
            batch: 0,
            by: None,
            lines: ["bore", "durability", "moddability"]
                .map(|line| (line.to_owned(), STORE_GRADE))
                .into(),
            condition: Some(strata_registry::item::QUALITY_MAX),
            fitted: BTreeMap::new(),
            origin: None,
        };
        items
            .check_record(&record)
            .expect("a record of the catalogue");
        record
    }

    #[test]
    fn the_rifles_bore_and_long_optic_tighten_its_aimed_cone() {
        let registries = shipped();
        let items = &registries.0.items;
        let record = rifle(items);
        let bore = |at: u16| {
            let mut record = record.clone();
            record.lines.insert("bore".to_owned(), at);
            let own = factors(items, &record);
            assert_eq!(own.len(), 1, "the signature alone: {own:?}");
            assert_eq!(own[0].property, Property::AimSpread);
            own[0].factor
        };
        for (at, expected) in [(0, 1.60), (500, 1.20), (750, 1.00), (1000, 0.80)] {
            assert!((bore(at) - expected).abs() < 1e-6, "{at}");
        }
        let aimed = |record: &Record| {
            let mut actor =
                Combatant::default().carrying([Some(Weapon::Rifle), Some(Weapon::Pistol)], 0);
            actor.equip(&loadout(items, &[Some(record.clone()), None]));
            let mut motion = CombatMotion::default();
            let mut input = CombatIntent {
                aim: true,
                ..Default::default()
            };
            for _ in 0..90 {
                actor.step(&mut input, &PlayerIntent::default(), 1. / 60., &mut motion);
            }
            assert_eq!(actor.aim_share(), 1.);
            (actor.cone(), actor.spec_of(Weapon::Rifle))
        };
        let table = Weapon::Rifle.spec();
        // A bought rifle aims at 1.20 of the table's cone, the hip's unchanged.
        let (cone, row) = aimed(&record);
        assert!((cone - table.aim_spread * 1.2).abs() < 1e-5, "{cone}");
        assert_eq!(row.spread.to_bits(), table.spread.to_bits());
        // The long optic multiplies it again, 0.86, after the signature.
        let sighted = with(items, &record, "sight", Some(&bought(items, OPTIC))).unwrap();
        let (cone, _) = aimed(&sighted);
        assert!(
            (cone - table.aim_spread * 1.2 * 0.86).abs() < 1e-5,
            "{cone}"
        );
        // A rifle made to 750 matches the table, and the best is tighter.
        let mut made = record.clone();
        made.lines.insert("bore".to_owned(), 750);
        assert!((aimed(&made).0 - table.aim_spread).abs() < 1e-5);
        // The stock opens at 400 and fits, the field rest at 800 does not
        // on a bought rifle, and its brace time has no number in the fight.
        let stocked = with(items, &record, "stock", Some(&bought(items, STOCK))).unwrap();
        assert!((aimed(&stocked).0 - table.aim_spread * 1.2 * 0.97).abs() < 1e-5);
        assert_eq!(
            with(items, &record, "support", Some(&bought(items, REST))),
            Err(FitRefusal::Shut { opens_at: 800 })
        );
        let mut platform = record.clone();
        platform.lines.insert("moddability".to_owned(), 800);
        let rested = with(items, &platform, "support", Some(&bought(items, REST))).unwrap();
        assert!(
            factors(items, &rested)
                .iter()
                .any(|f| f.property == Property::BraceTime)
        );
        let live = loadout(items, &[Some(rested.clone()), None]);
        assert!(
            live.mods[&Weapon::Rifle]
                .iter()
                .all(|f| f.property != Property::BraceTime),
            "a brace time is not in the fight yet"
        );
        // The feed trades rounds for a quicker reload: 4 x 0.85 is 3.
        let fed = with(items, &record, "feed", Some(&bought(items, COMPACT))).unwrap();
        let (_, row) = aimed(&fed);
        assert_eq!(row.magazine, 3);
        // The carbine takes the supported stock too.
        let [carbine, _] = weapons(items);
        assert!(with(items, &carbine, "stock", Some(&bought(items, STOCK))).is_ok());
        // The prism sight and the shrouded muzzle are not items.
        assert!(items.get(&id("veyra:module.vigil_prism")).is_none());
    }

    #[test]
    fn the_pistol_cell_gives_13() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let fitted = with(items, &pistol, "feed", Some(&bought(items, CELL))).unwrap();
        let actor = holding(items, &carbine, &fitted);
        assert_eq!(actor.spec_of(Weapon::Pistol).magazine, 13);
        assert_eq!(actor.spec_of(Weapon::Pistol).reload, 0.9_f32 * 1.15_f32);
        // The holstered weapon's row is read holstered and in hand alike.
        assert_eq!(actor.magazine(), 24, "the carbine in hand is as it was");
        let mut drawn = actor.clone();
        drawn.weapon = Weapon::Pistol;
        assert_eq!(drawn.magazine(), 13);
        assert!((drawn.reload_time() - 1.035).abs() < 1e-4);
    }

    #[test]
    fn the_grip_draws_in_0_43_and_the_sight_in_0_46() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let draw = |mount: &str, module: &str| {
            let fitted = with(items, &carbine, mount, Some(&bought(items, module))).unwrap();
            let mut actor = holding(items, &fitted, &pistol);
            // Drawing the carbine: the pistol in hand, the swap under way.
            actor.weapon = Weapon::Pistol;
            actor.action = Action::Swap;
            let drawing = actor.duration();
            // Holstering it for the pistol is the pistol's draw, unchanged.
            actor.weapon = Weapon::Carbine;
            assert_eq!(actor.duration(), Action::Swap.spec().duration);
            drawing
        };
        let (grip, sight) = (draw("grip", GRIP), draw("sight", REFLEX));
        assert_eq!(grip, 0.45_f32 * 0.95_f32);
        assert_eq!(sight, 0.45_f32 * 1.03_f32);
        let text = |seconds| Amount::of(seconds, 2, Unit::Seconds).text();
        assert_eq!(
            (text(grip).as_str(), text(sight).as_str()),
            ("0.43", "0.46")
        );
        // The swap itself runs to the fitted draw and no further.
        let fitted = with(items, &carbine, "sight", Some(&bought(items, REFLEX))).unwrap();
        let mut actor = holding(items, &fitted, &pistol);
        actor.weapon = Weapon::Pistol;
        std::mem::swap(&mut actor.ammo, &mut actor.holstered_ammo);
        let mut input = crate::combat::CombatIntent {
            weapon: Some(0),
            ..Default::default()
        };
        let mut motion = crate::combat::CombatMotion::default();
        let mut ticks = 0;
        while actor.weapon == Weapon::Pistol {
            actor.step(
                &mut input,
                &strata_types::PlayerIntent::default(),
                0.001,
                &mut motion,
            );
            ticks += 1;
            assert!(ticks < 1000, "the draw ends");
        }
        // One tick to accept the draw, then 0.4635 s of it.
        assert!((462..=466).contains(&ticks), "{ticks} ms");
    }

    /// The tables give a draw a dodge window as long as the draw, which is
    /// never. A sight that slows the draw must not open one in the time it
    /// adds, and a grip that quickens it changes nothing either: with a
    /// dodge asked for on every tick, the draw runs its fitted length and
    /// the dodge begins only once the weapon is up, as it does bare.
    #[test]
    fn a_fitted_draw_is_no_easier_to_dodge_out_of() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let swap = Action::Swap.spec();
        assert_eq!(swap.dodge_cancel, swap.duration, "never, bare");
        let drawn_after = |fitted: Option<(&str, &str)>| {
            let record = fitted.map_or_else(
                || carbine.clone(),
                |(mount, module)| {
                    with(items, &carbine, mount, Some(&bought(items, module))).unwrap()
                },
            );
            let mut actor = holding(items, &record, &pistol);
            actor.weapon = Weapon::Pistol;
            std::mem::swap(&mut actor.ammo, &mut actor.holstered_ammo);
            let mut input = crate::combat::CombatIntent {
                weapon: Some(0),
                ..Default::default()
            };
            let mut motion = crate::combat::CombatMotion::default();
            let mut ticks = 0;
            while actor.weapon == Weapon::Pistol {
                input.dodge = ticks > 0;
                actor.step(
                    &mut input,
                    &strata_types::PlayerIntent::default(),
                    0.001,
                    &mut motion,
                );
                ticks += 1;
                assert!(
                    actor.weapon == Weapon::Carbine || actor.action == Action::Swap,
                    "tick {ticks}: the draw was cut short by {:?}",
                    actor.action
                );
                assert!(ticks < 1000, "the draw ends");
            }
            ticks
        };
        let bare = drawn_after(None);
        assert!((449..=453).contains(&bare), "{bare} ms");
        let sight = drawn_after(Some(("sight", REFLEX)));
        assert!((462..=466).contains(&sight), "{sight} ms");
        let grip = drawn_after(Some(("grip", GRIP)));
        assert!((426..=430).contains(&grip), "{grip} ms");
    }

    #[test]
    fn boxes_come_before_mods_and_each_mod_once() {
        let registries = shipped();
        let items = &registries.0.items;
        let graph = SkillGraph::shipped();
        let [carbine, pistol] = weapons(items);
        let carbine = with(items, &carbine, "feed", Some(&bought(items, FEED))).unwrap();
        let pistol = with(items, &pistol, "feed", Some(&bought(items, CELL))).unwrap();
        let fitted = loadout(items, &[Some(carbine), Some(pistol)]);
        // Carbine 2 cuts the reload by 15 percent and Pistol 2 adds four
        // rounds: an Add before the cell's Mul is (10 + 4) x 1.25 = 18,
        // where the cell first would have been 13 + 4.
        let mut sheet = CharacterSheet::default();
        for learned in ["marksman.carbine.2", "marksman.pistol.2"] {
            sheet.learned.insert(learned.to_owned());
        }
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet);
        let boxed = actor.clone();
        assert_eq!(boxed.spec_of(Weapon::Pistol).magazine, 14);
        actor.equip(&fitted);
        assert_eq!(actor.spec_of(Weapon::Pistol).magazine, 18);
        let reload = (1.35_f32 * 0.85_f32) * 1.1_f32;
        assert_eq!(actor.spec_of(Weapon::Carbine).reload, reload);
        assert_eq!(actor.spec_of(Weapon::Carbine).magazine, 29);
        assert_eq!(
            bits(&actor.bare(Weapon::Carbine)),
            bits(&boxed.bare(Weapon::Carbine))
        );

        // Equipped again, fitted to the sheet again, in either order: each
        // module is still there once.
        let once = actor.clone();
        actor.equip(&fitted);
        actor.fit(&graph, &sheet);
        actor.equip(&fitted);
        for weapon in Weapon::GUNS {
            assert_eq!(bits(&read(&actor, weapon)), bits(&read(&once, weapon)));
        }
        // A box learned under a fitted weapon is felt under the module.
        sheet.learned.insert("marksman.field_craft.2".to_owned());
        actor.fit(&graph, &sheet);
        assert_eq!(
            actor.spec_of(Weapon::Carbine).reload,
            ((1.35_f32 * 0.85_f32) * 0.85_f32) * 1.1_f32
        );
        assert_eq!(actor.loadout(), Some(&fitted));
    }

    #[test]
    fn two_capacity_mods_round_once() {
        let bare = Combatant::default().bare(Weapon::Carbine);
        let capacity = |factor| Factor {
            property: Property::Capacity,
            factor,
        };
        // 24 x 1.02 x 1.02 is 24.97: 25 rounded once, and 24 if each
        // module had rounded its own 24.48 down.
        let two = [capacity(1.02), capacity(1.02)];
        assert_eq!(merge(&bare, &two, CountRule::Nearest).spec.magazine, 25);
        assert_eq!(
            merge(&bare, &two[..1], CountRule::Nearest).spec.magazine,
            24
        );
        assert_eq!(merge(&bare, &two, CountRule::Floor).spec.magazine, 24);
        // The mockup's rule, on the patrol magazine.
        let feed = [capacity(1.2)];
        assert_eq!(merge(&bare, &feed, CountRule::Floor).spec.magazine, 28);
        assert_eq!(merge(&bare, &feed, CountRule::Nearest).spec.magazine, 29);
        // A magazine whose product is whole keeps its last round floored,
        // though 50 x 1.06 falls a hair short of 53 in `f32`.
        let mut drum = bare;
        drum.spec.magazine = 50;
        for rule in [CountRule::Floor, CountRule::Nearest] {
            assert_eq!(merge(&drum, &[capacity(1.06)], rule).spec.magazine, 53);
        }
        assert_eq!(
            merge(&drum, &[capacity(1.18)], CountRule::Floor)
                .spec
                .magazine,
            59
        );
        // A reload and a capacity in either order give the same row.
        let reload = Factor {
            property: Property::ReloadTime,
            factor: 1.1,
        };
        assert_eq!(
            bits(&merge(&bare, &[feed[0], reload], CountRule::Nearest)),
            bits(&merge(&bare, &[reload, feed[0]], CountRule::Nearest))
        );
        assert_eq!(bits(&merge(&bare, &[], CountRule::Nearest)), bits(&bare));
    }

    #[test]
    fn removing_restores_the_row_bit_for_bit() {
        let registries = shipped();
        let items = &registries.0.items;
        let graph = SkillGraph::shipped();
        let [carbine, pistol] = weapons(items);
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("marksman.carbine.2".to_owned());
        for sheet in [None, Some(&sheet)] {
            let mut actor = Combatant::default();
            if let Some(sheet) = sheet {
                actor.fit(&graph, sheet);
            }
            let rows = Weapon::GUNS.map(|weapon| bits(&read(&actor, weapon)));
            // The bought carbine has no part fitted but reads its own
            // action line, so what a removal restores is that, not the table.
            let bought_carbine = loadout(items, &[Some(carbine.clone()), Some(pistol.clone())]);
            let mut before = actor.clone();
            before.equip(&bought_carbine);
            let rows_bought = Weapon::GUNS.map(|weapon| bits(&read(&before, weapon)));
            let mut fitted = with(items, &carbine, "feed", Some(&batch_412(items))).unwrap();
            fitted = with(items, &fitted, "grip", Some(&bought(items, GRIP))).unwrap();
            actor.equip(&loadout(
                items,
                &[Some(fitted.clone()), Some(pistol.clone())],
            ));
            assert_ne!(bits(&read(&actor, Weapon::Carbine)), rows[0]);
            assert_eq!(bits(&read(&actor, Weapon::Pistol)), rows[1]);
            assert_ne!(
                rows_bought[0], rows[0],
                "the action line moves the interval"
            );

            // The removal, as a record and as a loadout.
            let stripped = with(items, &fitted, "feed", None).unwrap();
            let stripped = with(items, &stripped, "grip", None).unwrap();
            assert_eq!(stripped, carbine, "the record is the one it was");
            let empty = loadout(items, &[Some(stripped), Some(pistol.clone())]);
            assert_eq!(empty, bought_carbine);
            actor.equip(&empty);
            assert_eq!(
                Weapon::GUNS.map(|weapon| bits(&read(&actor, weapon))),
                rows_bought
            );
            // Not the rows alone: nothing of the actor remembers the fit.
            assert_eq!(format!("{actor:?}"), format!("{before:?}"));
        }
    }

    /// What a stack is, as `(item, count, batch)`.
    pub fn triple(stack: Option<&ItemStack>, items: &ItemCatalogue) -> Option<(String, u32, u32)> {
        let record = items::read(stack?, items).unwrap();
        Some((record.item.to_string(), record.count, record.batch))
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "set against what a cell holds, which may be nothing"
    )]
    pub fn of(item: &str, count: u32, batch: u32) -> Option<(String, u32, u32)> {
        Some((item.to_owned(), count, batch))
    }

    /// What a screen asks before it asks for a fit: the same answers the
    /// requests would get, and the figures of a part the mount would not
    /// take.
    #[test]
    fn the_rules_a_screen_reads_are_the_requests_own() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let feed = bought(items, FEED);

        // A workbench in reach, or a run that says anywhere.
        let (near, far) = (
            Reach {
                workbench: true,
                ..Reach::default()
            },
            Reach::default(),
        );
        assert!(at_workbench(WorkbenchAccess::AtWorkbench, &near));
        assert!(!at_workbench(WorkbenchAccess::AtWorkbench, &far));
        assert!(at_workbench(WorkbenchAccess::Anywhere, &far));

        // A removal: what it leaves and what comes off.
        let fitted = with(items, &carbine, "feed", Some(&feed)).unwrap();
        assert_eq!(
            without(items, &fitted, "feed"),
            Ok((carbine.clone(), feed.clone()))
        );
        assert_eq!(
            without(items, &carbine, "feed"),
            Err(FitRefusal::NothingFitted)
        );
        assert_eq!(
            without(items, &carbine, "bayonet"),
            Err(FitRefusal::NoMount)
        );
        assert_eq!(bare_of(&fitted), carbine);
        assert_eq!(bare_of(&carbine), carbine);

        // A preview is the fit where the fit is allowed, and the same
        // record written past the refusal where it is not.
        assert_eq!(preview(items, &carbine, "feed", Some(&feed)), fitted);
        assert_eq!(preview(items, &fitted, "feed", Some(&feed)), fitted);
        assert_eq!(preview(items, &fitted, "feed", None), carbine);
        let shut = mounts(items, &carbine)
            .into_iter()
            .find_map(|(mount, state)| matches!(state, MountState::Shut { .. }).then_some(mount))
            .unwrap();
        assert!(with(items, &carbine, &shut, None).is_err());
        assert_eq!(preview(items, &carbine, &shut, None), carbine);

        // A loose module is measured on the weapon that takes it, the one
        // in hand first.
        let both = [Some(&carbine), Some(&pistol)];
        assert_eq!(host(items, &both, 0, &id(FEED)), Some(0));
        assert_eq!(host(items, &both, 1, &id(FEED)), Some(0));
        assert_eq!(host(items, &both, 0, &id(CELL)), Some(1));
        assert_eq!(host(items, &[None, Some(&pistol)], 0, &id(FEED)), None);
        assert_eq!(host(items, &both, 0, &id(HAUL)), None);

        // What a part is fitted for: its first benefit.
        assert_eq!(
            lead(items, &id(FEED)).map(|effect| effect.property),
            Some(Property::Capacity)
        );
        assert_eq!(
            lead_quality(&batch_412(items), lead(items, &id(FEED)).unwrap()),
            610
        );
        assert!(lead(items, &id(HAUL)).is_none());
    }

    /// A pack and a satchel with every cell taken by something that is no
    /// module, so nothing merges onto it.
    pub fn fill(pack: &mut Pack, items: &ItemCatalogue) {
        let haul = items::write(&loose(&bought(items, HAUL)), items).unwrap();
        for cell in 0..pack.cells.len() {
            if pack.cells.get(cell).is_none() {
                pack.cells.set(cell, Some(haul.clone()));
            }
        }
        for bag in 0..pack.bags.len() {
            for cell in 0..usize::from(pack.open_cells(bag, items)) {
                if pack.bags[bag].get(cell).is_none() {
                    pack.bags[bag].set(cell, Some(haul.clone()));
                }
            }
            pack.bags[bag].clear_changed();
        }
        pack.cells.clear_changed();
    }

    #[test]
    fn a_fit_takes_one_unit_and_a_removal_goes_to_the_pack_then_a_bag_then_the_store() {
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let mut pack = Pack::from_saved(&starting.packs["Frontier ranger"], items).unwrap();
        let mut hands = Hands::from_saved(&starting.hands.clone().map(Some), items).unwrap();
        let store: Vec<Option<Record>> = starting.store.iter().cloned().map(Some).collect();
        let mut store = HouseholdStore::from_saved(&store, items).unwrap();
        let feed = bought(items, FEED);
        macro_rules! stock {
            () => {
                &mut Stock {
                    saddle: None,
                    pack: &mut pack,
                    hands: &mut hands,
                    store: Some(&mut store),
                }
            };
        }

        // The pack has none, so the unit is one of the store's four.
        assert_eq!(fit(items, stock!(), 0, "feed", &feed), Ok(()));
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));
        assert_eq!(
            triple(store.0.get(1), items),
            of(FEED, 2, 412),
            "the other batch"
        );
        assert_eq!(
            held(items, &hands, 0).unwrap().fitted.get("feed"),
            Some(&feed)
        );
        // Nothing carried but the survey tool in the last cell.
        assert_eq!(pack.cells.slots().iter().flatten().count(), 1);

        // Off again, it is carried: the pack's first cell, not back onto
        // the store's stack of its kind.
        assert_eq!(remove(items, stock!(), 0, "feed"), Ok(feed.clone()));
        assert_eq!(triple(pack.cells.get(0), items), of(FEED, 1, 0));
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));
        assert_eq!(held(items, &hands, 0).unwrap(), starting.hands[0]);
        assert_eq!(
            remove(items, stock!(), 0, "feed"),
            Err(FitRefusal::NothingFitted)
        );

        // Fitted again, the unit is the pack's before the store's.
        assert_eq!(fit(items, stock!(), 0, "feed", &feed), Ok(()));
        assert_eq!(pack.cells.get(0), None);
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));

        // The pack full, it goes to the satchel's first free cell; the
        // satchel full as well, to the store, onto the stack of its kind.
        let satchel = |pack: &Pack, cell| triple(pack.bags[0].get(cell), items);
        assert_eq!(satchel(&pack, 0), of(CELL, 1, 0));
        let empty = pack.clone();
        for cell in 0..pack.cells.len() {
            let haul = items::write(&loose(&bought(items, HAUL)), items).unwrap();
            pack.cells.set(cell, Some(haul));
        }
        pack.cells.clear_changed();
        assert_eq!(remove(items, stock!(), 0, "feed"), Ok(feed.clone()));
        assert_eq!(satchel(&pack, 1), of(FEED, 1, 0));
        assert_eq!(fit(items, stock!(), 0, "feed", &feed), Ok(()));
        assert_eq!(
            satchel(&pack, 1),
            None,
            "taken from the bag before the store"
        );
        fill(&mut pack, items);
        assert_eq!(remove(items, stock!(), 0, "feed"), Ok(feed.clone()));
        assert_eq!(triple(store.0.get(0), items), of(FEED, 4, 0));

        // A batch is fitted by name and never stands in for another.
        pack = empty;
        let made = batch_412(items);
        assert_eq!(fit(items, stock!(), 0, "feed", &made), Ok(()));
        assert_eq!(triple(store.0.get(1), items), of(FEED, 1, 412));
        assert_eq!(triple(store.0.get(0), items), of(FEED, 4, 0));
        assert_eq!(
            fit(items, stock!(), 0, "feed", &made),
            Err(FitRefusal::AlreadyFitted)
        );
        let mut rare = made.clone();
        rare.batch = 413;
        assert_eq!(
            fit(items, stock!(), 0, "feed", &rare),
            Err(FitRefusal::NotInStock)
        );
        // Onto a mount that holds one, the two change places.
        assert_eq!(fit(items, stock!(), 0, "feed", &feed), Ok(()));
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));
        assert_eq!(triple(pack.cells.get(0), items), of(FEED, 1, 412));
        assert_eq!(
            held(items, &hands, 0).unwrap().fitted.get("feed"),
            Some(&feed)
        );
    }

    #[test]
    fn a_swap_with_no_room_is_refused_and_changes_nothing() {
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let mut pack = Pack::from_saved(&starting.packs["Frontier ranger"], items).unwrap();
        let made = batch_412(items);
        let carbine = with(items, &starting.hands[0], "feed", Some(&made)).unwrap();
        let mut hands =
            Hands::from_saved(&[Some(carbine), Some(starting.hands[1].clone())], items).unwrap();
        // Two bought magazines in the pack's last cell and every other
        // cell taken; no store to hand.
        let feed = bought(items, FEED);
        let mut two = loose(&feed);
        two.count = 2;
        let last = pack.cells.len() - 1;
        pack.cells
            .set(last, Some(items::write(&two, items).unwrap()));
        fill(&mut pack, items);
        let before = (pack.clone(), hands.clone());
        macro_rules! stock {
            () => {
                &mut Stock {
                    saddle: None,
                    pack: &mut pack,
                    hands: &mut hands,
                    store: None,
                }
            };
        }
        // One of the two is taken and the cell is still a stack's, so the
        // magazine that comes off has nowhere to go.
        assert_eq!(
            fit(items, stock!(), 0, "feed", &feed),
            Err(FitRefusal::NoRoom)
        );
        assert_eq!((&pack, &hands), (&before.0, &before.1));
        assert_eq!(remove(items, stock!(), 0, "feed"), Err(FitRefusal::NoRoom));
        assert_eq!((&pack, &hands), (&before.0, &before.1));

        // With one alone in the cell the two change places through it.
        pack.cells
            .set(last, Some(items::write(&loose(&feed), items).unwrap()));
        pack.cells.clear_changed();
        assert_eq!(fit(items, stock!(), 0, "feed", &feed), Ok(()));
        assert_eq!(triple(pack.cells.get(last), items), of(FEED, 1, 412));
        assert_eq!(
            held(items, &hands, 0).unwrap().fitted.get("feed"),
            Some(&feed)
        );
    }

    #[test]
    fn candidates_come_best_first() {
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let [carbine, pistol] = weapons(items);
        let mut pack = Pack::from_saved(&starting.packs["Frontier ranger"], items).unwrap();
        let store: Vec<Option<Record>> = starting.store.iter().cloned().map(Some).collect();
        let store = HouseholdStore::from_saved(&store, items).unwrap();
        let candidate = |module: Fitted, carried, stored| Candidate {
            module,
            carried,
            stored,
        };

        // Tovan Reyes's batch has the better capacity (610 against 500).
        let feeds = candidates(items, &pack, Some(&store), &carbine, "feed");
        assert_eq!(
            feeds,
            [
                candidate(batch_412(items), 0, 2),
                candidate(bought(items, FEED), 0, 4),
            ]
        );
        // The pistol's cell is carried in the satchel and stored as well.
        assert_eq!(
            candidates(items, &pack, Some(&store), &pistol, "feed"),
            [candidate(bought(items, CELL), 1, 3)]
        );
        assert_eq!(
            candidates(items, &pack, Some(&store), &carbine, "grip"),
            [candidate(bought(items, GRIP), 0, 2)]
        );
        assert_eq!(
            candidates(items, &pack, Some(&store), &carbine, "sight"),
            [candidate(bought(items, REFLEX), 0, 1)]
        );
        // What exists for a shut mount is listed all the same: the one
        // vented muzzle in the store, which a bought carbine cannot take.
        assert_eq!(
            candidates(items, &pack, Some(&store), &carbine, "muzzle"),
            [candidate(bought(items, COMPENSATOR), 0, 1)]
        );
        // Away from a workbench the store is not to hand.
        assert_eq!(candidates(items, &pack, None, &carbine, "feed"), []);
        assert_eq!(
            candidates(items, &pack, None, &pistol, "feed"),
            [candidate(bought(items, CELL), 1, 0)]
        );

        // Carried and stored counts of one batch add up apart, a poorer
        // batch sorts under the bought one, and a tie keeps the lower
        // batch first.
        let mut poor = graded(items, FEED, 300);
        poor.batch = 9;
        poor.by = Some("A beginner".to_owned());
        let mut twin = bought(items, FEED);
        twin.batch = 3;
        twin.by = Some("A copyist".to_owned());
        let mut three = loose(&bought(items, FEED));
        three.count = 3;
        for (cell, record) in [three, loose(&poor), loose(&twin)].iter().enumerate() {
            pack.cells
                .set(cell, Some(items::write(record, items).unwrap()));
        }
        let feeds = candidates(items, &pack, Some(&store), &carbine, "feed");
        assert_eq!(
            feeds,
            [
                candidate(batch_412(items), 0, 2),
                candidate(bought(items, FEED), 3, 4),
                candidate(twin, 1, 0),
                candidate(poor, 1, 0),
            ]
        );
    }

    #[test]
    fn a_property_with_no_number_never_reaches_the_row() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let alive: Vec<Property> = Property::ALL.into_iter().filter(|p| live(*p)).collect();
        assert_eq!(alive.len(), 9, "crafting.md, 4.5");
        let dead: Vec<Property> = Property::ALL.into_iter().filter(|p| !live(*p)).collect();
        assert_eq!(
            dead,
            [
                Property::RecoilRecovery,
                Property::CoolingRate,
                Property::BraceTime,
                Property::RecoveryTime,
            ]
        );
        let bare = Combatant::default().bare(Weapon::Carbine);
        let fitted: Vec<Factor> = dead
            .iter()
            .map(|&property| Factor {
                property,
                factor: 0.5,
            })
            .collect();
        assert_eq!(
            bits(&merge(&bare, &fitted, CountRule::Nearest)),
            bits(&bare)
        );
        // And each live one moves exactly its own number.
        for property in alive {
            let one = [Factor {
                property,
                factor: 1.5,
            }];
            let merged = bits(&merge(&bare, &one, CountRule::Nearest));
            let moved = merged
                .iter()
                .zip(bits(&bare))
                .filter(|(a, b)| **a != *b)
                .count();
            assert_eq!(moved, 1, "{property:?}");
        }

        // Both of the sight's effects are in the record's factors and in
        // the loadout.
        let sighted = with(items, &carbine, "sight", Some(&bought(items, REFLEX))).unwrap();
        let all: Vec<Property> = factors(items, &sighted)
            .iter()
            .map(|f| f.property)
            .collect();
        assert_eq!(
            all,
            [
                Property::AimSpread,
                Property::EquipTime,
                Property::RepeatInterval
            ]
        );
        let fitted = loadout(items, &[Some(sighted), Some(pistol.clone())]);
        let kept: Vec<Property> = fitted.mods[&Weapon::Carbine]
            .iter()
            .map(|factor| factor.property)
            .collect();
        assert_eq!(
            kept,
            [
                Property::AimSpread,
                Property::EquipTime,
                Property::RepeatInterval
            ]
        );
        let row = merge(&bare, &fitted.mods[&Weapon::Carbine], fitted.count);
        let table = Weapon::Carbine.spec();
        assert_eq!(
            (bare.spec.spread.to_bits(), bare.spec.aim_spread.to_bits()),
            (table.spread.to_bits(), table.aim_spread.to_bits()),
            "nothing fitted reads the table exactly"
        );
        assert_eq!(row.spec.spread.to_bits(), bare.spec.spread.to_bits());
        assert!((row.spec.aim_spread - bare.spec.aim_spread * 0.70).abs() < 1e-6);
        assert_eq!(fitted.count, CountRule::Nearest);
        // Mount order, whatever order the modules went on in.
        let gripped = with(items, &carbine, "grip", Some(&bought(items, GRIP))).unwrap();
        let both = with(items, &gripped, "sight", Some(&bought(items, REFLEX))).unwrap();
        let order: Vec<Property> = factors(items, &both).iter().map(|f| f.property).collect();
        assert_eq!(
            order,
            [
                Property::AimSpread,
                Property::EquipTime,
                Property::EquipTime,
                Property::RecoilRecovery,
                Property::RepeatInterval,
            ]
        );
        // Nothing in the hands: no loadout at all. A bought carbine with
        // nothing fitted still has its action line.
        let bought_carbine = loadout(items, &[Some(carbine), Some(pistol)]);
        assert!(!bought_carbine.is_empty());
        assert!(only_the_action(Some(&bought_carbine)));
        assert!(loadout(items, &[None, None]).is_empty());
        assert!(Loadout::default().is_empty());
    }

    /// A store grade example of the weapon `name`.
    pub fn example(items: &ItemCatalogue, name: &str) -> Record {
        crate::crafting::bought(items.get(&id(name)).unwrap())
    }

    #[test]
    fn a_blade_in_the_first_hand_and_a_carbine_in_the_second_are_read_by_their_families() {
        let registries = shipped();
        let items = &registries.0.items;
        let blade = example(items, "veyra:weapon.service_blade");
        let carbine = weapons(items)[0].clone();
        let hands = [Some(blade.clone()), Some(carbine.clone())];
        // Each family has its factors: the blade's edge, the carbine's action.
        let loaded = loadout(items, &hands);
        assert_eq!(
            loaded.mods.keys().copied().collect::<Vec<_>>(),
            [Weapon::Carbine, Weapon::Blade]
        );
        assert_eq!(loaded.mods[&Weapon::Blade].len(), 1);
        assert_eq!(loaded.mods[&Weapon::Blade][0].property, Property::HitDamage);
        assert!((loaded.mods[&Weapon::Blade][0].factor - 0.95).abs() < 1e-6);
        // The magazine is the second hand's carbine's, the first hand has none.
        let held = Hands::from_saved(&hands, items).unwrap();
        assert_eq!(magazine(items, &held, 0, None), 0);
        assert_eq!(
            magazine(items, &held, 1, None),
            Weapon::Carbine.spec().magazine
        );
        // The views of a blade: the cuts as the fight reads them.
        let holder = Combatant::default();
        let lines = cut_lines(items, &holder, &blade);
        let shown: Vec<(CutFigure, i32)> = lines.iter().map(|(f, a)| (*f, a.number)).collect();
        assert_eq!(
            shown,
            [
                (CutFigure::LightDamage, 27),
                (CutFigure::LightTime, 40),
                (CutFigure::LightRecovery, 18),
                (CutFigure::LightStamina, 18),
                (CutFigure::Draw, 45),
                (CutFigure::HeavyDamage, 52),
                (CutFigure::HeavyPosture, 80),
                (CutFigure::Reach, 17),
                (CutFigure::GuardShare, 60),
                (CutFigure::ParryWindow, 20),
            ]
        );
        assert_eq!(lines[7].1.unit, Unit::Metres);
        // A holder trained in the guard shows the trained row.
        let mut trained = crate::progression::CharacterSheet::default();
        for id in [
            "blade.novice",
            "blade.guard.1",
            "blade.guard.2",
            "blade.guard.3",
        ] {
            trained.learned.insert(id.to_owned());
        }
        let mut fitted = Combatant::default();
        fitted.fit(
            &strata_registry::progression::SkillGraph::shipped(),
            &trained,
        );
        let shown: Vec<(CutFigure, i32)> = cut_lines(items, &fitted, &blade)
            .iter()
            .map(|(f, a)| (*f, a.number))
            .collect();
        let at = |figure| shown.iter().find(|(f, _)| *f == figure).unwrap().1;
        assert_eq!(at(CutFigure::GuardShare), 65);
        assert_eq!(at(CutFigure::ParryWindow), 23);
        let great = example(items, "veyra:weapon.yard_greatblade");
        let shown: Vec<i32> = cut_lines(items, &holder, &great)
            .iter()
            .map(|(_, a)| a.number)
            .collect();
        assert_eq!(
            shown,
            [44, 70, 31, 30, 45, 86, 132, 22, 80, 10],
            "slower, harder, longer"
        );
        assert!(
            cut_lines(items, &holder, &carbine).is_empty(),
            "a gun has no cuts"
        );
        // It stands on a chassis now: an edge and a grip, both open at 500.
        let held = mounts(items, &blade);
        let shown: Vec<&str> = held.iter().map(|(slot, _)| slot.as_str()).collect();
        assert_eq!(shown, ["edge", "grip"]);
        // Worn to nothing, the same blade cuts for less.
        let mut worn = blade.clone();
        worn.condition = Some(0);
        let worn = cut_lines(items, &holder, &worn);
        assert!(worn[0].1.number < lines[0].1.number, "{worn:?}");
        // And a Combatant carrying it cuts at the figure the screen shows.
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0);
        actor.equip(&loaded);
        assert!((actor.spec(Action::Light).damage - 28. * 0.95).abs() < 1e-4);
    }

    pub const MELEE_PARTS: [(&str, &str, &str); 7] = [
        (
            "veyra:weapon.service_blade",
            "edge",
            "veyra:module.blade_hooked_edge",
        ),
        (
            "veyra:weapon.service_blade",
            "grip",
            "veyra:module.blade_balanced_grip",
        ),
        (
            "veyra:weapon.yard_greatblade",
            "head",
            "veyra:module.cutter_reinforced_head",
        ),
        (
            "veyra:weapon.yard_greatblade",
            "power",
            "veyra:module.cutter_compact_power",
        ),
        (
            "veyra:weapon.yard_greatblade",
            "grip",
            "veyra:module.cutter_wrapped_grip",
        ),
        (
            "veyra:weapon.pier_glaive",
            "edge",
            "veyra:module.glaive_broad_edge",
        ),
        (
            "veyra:weapon.pier_glaive",
            "grip",
            "veyra:module.glaive_counterweight",
        ),
    ];

    /// A store grade example, made well enough to open every mount.
    pub fn platform(items: &ItemCatalogue, name: &str) -> Record {
        let mut record = example(items, name);
        record.lines.insert(MODDABILITY.to_owned(), 600);
        record
    }

    #[test]
    fn what_each_melee_part_does_to_a_store_grade_weapon() {
        let registries = shipped();
        let items = &registries.0.items;
        let holder = Combatant::default();
        for (weapon, mount, part) in MELEE_PARTS {
            let now = platform(items, weapon);
            let then = with(items, &now, mount, Some(&bought(items, part))).unwrap();
            let before = cut_lines(items, &holder, &now);
            let after = cut_lines(items, &holder, &then);
            let moved: Vec<String> = before
                .iter()
                .zip(&after)
                .filter(|(was, is)| was.1.number != is.1.number)
                .map(|(was, is)| format!("{:?} {} -> {}", was.0, was.1.text(), is.1.text()))
                .collect();
            eprintln!("{part}: {moved:?}");
            assert!(
                !moved.is_empty() || part.ends_with("compact_power"),
                "{part} moves nothing"
            );
        }
    }

    #[test]
    fn a_fitted_hooked_edge_reads_its_posture_and_its_tail_on_the_combatant() {
        let registries = shipped();
        let items = &registries.0.items;
        let blade = example(items, "veyra:weapon.service_blade");
        let hooked = with(
            items,
            &blade,
            "edge",
            Some(&bought(items, "veyra:module.blade_hooked_edge")),
        )
        .unwrap();
        let carry = |record: &Record| {
            let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
            actor.equip(&loadout(items, &[Some(record.clone()), None]));
            actor
        };
        let (plain, edged) = (carry(&blade), carry(&hooked));
        for action in [Action::Light, Action::Followup, Action::Heavy] {
            let (was, now) = (plain.spec(action), edged.spec(action));
            assert!(
                (now.posture - was.posture * 1.08).abs() < 1e-4,
                "{action:?}"
            );
            assert_eq!(now.damage.to_bits(), was.damage.to_bits(), "{action:?}");
            assert_eq!(now.contact_end.to_bits(), was.contact_end.to_bits());
            let tail = (was.duration - was.contact_end) * 1.06;
            assert!(
                (now.duration - was.contact_end - tail).abs() < 1e-5,
                "{action:?}"
            );
        }
        // The screen's figures say it before it is made, as a gun's do.
        let bare = plain.bare(Weapon::Blade);
        let shown = lines(
            items,
            &bare,
            &blade,
            &hooked,
            &[
                Property::HitPosture,
                Property::RecoveryTime,
                Property::Capacity,
            ],
        );
        assert!(
            shown[0].live && shown[1].live,
            "a blade's posture and tail are in the fight"
        );
        assert!(!shown[2].live, "a blade has no magazine");
        assert_eq!(shown[0].value.number, 108);
        assert_eq!(shown[0].change.map(|change| change.by), Some(8));
        let tail = shown[1].change.expect("the tail moved");
        assert!(
            !tail.better && tail.by > 0,
            "a longer tail is worse: {tail:?}"
        );
        assert_eq!(shown[1].value.unit, Unit::Seconds);
        // The tail's figure is the light cut's, read the way the card reads it.
        let figure = cut_lines(items, &Combatant::default(), &hooked)
            .into_iter()
            .find(|(figure, _)| *figure == CutFigure::LightRecovery)
            .unwrap();
        assert_eq!(figure.1.number, shown[1].value.number);
    }

    #[test]
    fn capacity_is_live_on_a_gun_and_recovery_on_a_blade_and_never_the_other_way() {
        assert!(live_on(Property::Capacity, false) && !live_on(Property::Capacity, true));
        assert!(live_on(Property::RecoveryTime, true) && !live_on(Property::RecoveryTime, false));
        for property in Property::ALL {
            if !matches!(property, Property::Capacity | Property::RecoveryTime) {
                assert_eq!(live_on(property, true), live(property), "{property:?}");
                assert_eq!(live_on(property, false), live(property), "{property:?}");
            }
        }
        let registries = shipped();
        let items = &registries.0.items;
        // The compact cell's capacity reaches no row and its draw does.
        let great = platform(items, "veyra:weapon.yard_greatblade");
        let cell = bought(items, "veyra:module.cutter_compact_power");
        let fitted = with(items, &great, "power", Some(&cell)).unwrap();
        let loaded = loadout(items, &[Some(fitted.clone()), None]);
        let kept: Vec<Property> = loaded.mods[&Weapon::Greatblade]
            .iter()
            .map(|factor| factor.property)
            .collect();
        assert!(kept.contains(&Property::EquipTime), "{kept:?}");
        assert!(!kept.contains(&Property::Capacity), "{kept:?}");
        let bare = Combatant::default().bare(Weapon::Greatblade);
        let shown = lines(
            items,
            &bare,
            &great,
            &fitted,
            &[Property::Capacity, Property::EquipTime],
        );
        assert!(!shown[0].live && shown[1].live);
        assert_eq!(shown[0].value.unit, Unit::OfStandard, "kept in the data");
        assert_eq!(shown[0].value.number, 85);
        assert!(
            shown[1]
                .change
                .is_some_and(|change| change.by < 0 && change.better)
        );
        // A gun's recovery stays out of its row.
        let [carbine, pistol] = weapons(items);
        let gun = loadout(
            items,
            &[
                Some(with(items, &carbine, "grip", Some(&bought(items, GRIP))).unwrap()),
                Some(pistol),
            ],
        );
        assert!(
            gun.mods
                .values()
                .flatten()
                .all(|factor| factor.property != Property::RecoveryTime)
        );
    }

    #[test]
    fn a_shut_melee_mount_refuses_a_part_until_the_moddability_opens_it() {
        let registries = shipped();
        let items = &registries.0.items;
        let cell = bought(items, "veyra:module.cutter_compact_power");
        let mut great = example(items, "veyra:weapon.yard_greatblade");
        assert_eq!(
            with(items, &great, "power", Some(&cell)),
            Err(FitRefusal::Shut { opens_at: 600 })
        );
        assert_eq!(
            with(items, &great, "head", Some(&cell)),
            Err(FitRefusal::WrongMount)
        );
        great.lines.insert(MODDABILITY.to_owned(), 600);
        assert!(with(items, &great, "power", Some(&cell)).is_ok());
        let glaive = example(items, "veyra:weapon.pier_glaive");
        let broad = bought(items, "veyra:module.glaive_broad_edge");
        assert!(with(items, &glaive, "edge", Some(&broad)).is_ok());
        assert_eq!(
            with(
                items,
                &example(items, "veyra:weapon.service_blade"),
                "edge",
                Some(&broad)
            ),
            Err(FitRefusal::DoesNotFit),
            "the broad edge is the glaive's"
        );
        let mut rough = glaive.clone();
        rough.lines.insert(MODDABILITY.to_owned(), 399);
        let counter = bought(items, "veyra:module.glaive_counterweight");
        assert_eq!(
            with(items, &rough, "grip", Some(&counter)),
            Err(FitRefusal::Shut { opens_at: 400 })
        );
        rough.lines.insert(MODDABILITY.to_owned(), 400);
        assert!(with(items, &rough, "grip", Some(&counter)).is_ok());
    }
}
