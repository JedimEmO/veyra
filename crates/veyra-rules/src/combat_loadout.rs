//! What a fitted weapon changes in the fight: a [`Factor`] per live effect, the
//! [`Loadout`] an actor carries, the [`Row`] of every number fitting can
//! move and [`merge`], which applies the one to the other.
//!
//! These sat in `fitting`, which reads the stock, so `Combatant` reached up
//! to the gear layer for its own row type. The rules that build a loadout
//! from records stay in `fitting` and re-export these.

use std::collections::BTreeMap;

use strata_registry::item::{CountRule, Property};

use crate::combat::{Weapon, WeaponSpec};

/// Whether the fight has a number for `property` on a weapon that is
/// `melee` or a gun: `live` for a gun, and for a blade one with a tail to
/// its cuts and no magazine. The one place that says it, which the merge,
/// the lines and a screen all ask.
#[must_use]
pub const fn live_on(property: Property, melee: bool) -> bool {
    match property {
        Property::Capacity => !melee,
        Property::RecoveryTime => melee,
        other => other.live(),
    }
}

/// One fitted effect, its factor already moved by its module's quality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Factor {
    /// What it changes.
    pub property: Property,
    /// What it multiplies the weapon's own number by.
    pub factor: f32,
}

/// What an actor's weapons have fitted that the fight can honour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loadout {
    /// The live factors of each weapon that has any, in mount order.
    pub mods: BTreeMap<Weapon, Vec<Factor>>,
    /// How a magazine a factor has moved becomes whole rounds.
    pub count: CountRule,
}

impl Loadout {
    /// Whether nothing fitted changes anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mods.values().all(Vec::is_empty)
    }
}

/// Every number of a weapon that fitting can move, for one holder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// What a shot does and how the magazine behaves.
    pub spec: WeaponSpec,
    /// Seconds to draw it.
    pub draw: f32,
    /// The share of their pace the holder keeps while aiming it.
    pub aim_speed: f32,
    /// What a cut costs with it in hand, as a share of the cut's own
    /// stamina: one with nothing fitted.
    pub cut_stamina: f32,
    /// The factor on the tail of each of its cuts, after the contact.
    pub recovery: f32,
    /// Seconds from the end of the light cut's contact to its end with
    /// nothing fitted, which `recovery` scales: nought for a gun.
    pub tail: f32,
    /// Whether the weapon is a melee one, which decides which of its
    /// properties the fight has a number for ([`live_on`]).
    pub melee: bool,
}

/// `bare` with every live factor applied once, in order. The capacity
/// factors are multiplied together and the magazine made whole once, so
/// two modules never round twice; a factor on a property the fight has no
/// number for changes nothing.
#[must_use]
pub fn merge(bare: &Row, factors: &[Factor], count: CountRule) -> Row {
    let mut row = *bare;
    let mut capacity = None;
    for factor in factors {
        let by = factor.factor;
        if !live_on(factor.property, bare.melee) {
            continue;
        }
        match factor.property {
            Property::Capacity => capacity = Some(capacity.unwrap_or(1.) * by),
            Property::ReloadTime => row.spec.reload *= by,
            Property::RepeatInterval => row.spec.interval *= by,
            Property::EquipTime => row.draw *= by,
            Property::AimMoveSpeed => row.aim_speed *= by,
            Property::HitDamage => row.spec.damage *= by,
            Property::HitPosture => row.spec.posture *= by,
            Property::AttackStamina => row.cut_stamina *= by,
            Property::RecoveryTime => row.recovery *= by,
            // The hip's spread is the weapon's own, never a sight's business.
            Property::AimSpread => row.spec.aim_spread *= by,
            Property::RecoilRecovery | Property::CoolingRate | Property::BraceTime => {}
        }
    }
    if let Some(capacity) = capacity {
        row.spec.magazine = count.whole(f32::from(bare.spec.magazine) * capacity);
    }
    row
}
