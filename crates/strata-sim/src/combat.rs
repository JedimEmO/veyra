//! Action clocks and resource rules shared by a headless encounter and its view.
//!
//! Poses never decide damage. A physics adapter resolves the requested contact
//! intervals, then submits hits. This also keeps presentation hitstop local.

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::time::{Fixed, Time};
use serde::{Deserialize, Serialize};
use strata_core::WorldPos;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::ability::{
    AbilityEffect, AbilityRefusal, AbilityRow, Along, Bar, OnHit, Own, Place, ShotCone,
    ShotEffects, StatusRow, UseState, can_use,
};
use crate::combat_tables::{self, ActorTable, GuardRow, MeleeClass};
use crate::fitting::{Loadout, Row, merge};
use crate::progression::{CharacterSheet, Effective, apply};
use crate::{PlayerCommand, PlayerIntent, SimSet, SimTick};
use strata_registry::progression::{Op, SkillGraph};

/// Actions have explicit commitments rather than inheriting a clip's duration.
///
/// Ordered and hashable so a table can be keyed by it; the order is the
/// declaration order and means nothing.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Action {
    /// Locomotion and firing are available.
    #[default]
    Ready,
    /// Opening cut.
    Light,
    /// Buffered return cut.
    Followup,
    /// Slow posture-breaking cut.
    Heavy,
    /// Direction locked when the dodge begins.
    Dodge,
    /// Ammunition becomes available only at completion.
    Reload,
    /// A posture break interrupts the current commitment.
    Stagger,
    /// Holstering one weapon and drawing the other; no fire until it ends.
    Swap,
    /// Input cannot resurrect an actor.
    Dead,
    /// An active skill under way. It has no row in `actions.ron`: the
    /// ability's own row is its windup, length, dodge cancel and pace, read
    /// through [`Combatant::spec`], and it is in no table's `ALL`.
    Ability,
}

/// The weapons an actor can carry, two at a time and one in hand. The guns
/// have a row in `weapons.ron`; a melee class has one in `melee.ron`, which
/// scales the cut rows of `actions.ron`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Weapon {
    /// Two-handed, slower cadence, heavier bolt.
    #[default]
    Carbine,
    /// One-handed, quick, light bolt; the blade's companion.
    Pistol,
    /// One hand: the cuts as the actions table has them.
    Blade,
    /// Two hands: slower and harder cuts, hard to stagger mid swing.
    Greatblade,
    /// A polearm: longer and wider cuts, a little slower.
    Glaive,
    /// Two-handed, one slow hard shot; tight only when aimed and still.
    /// Last in the enum so the shot draw of every older weapon is unmoved.
    Rifle,
}

/// How a press of the trigger turns into rounds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FireMode {
    /// One round a press; the trigger must come up before the next.
    Single,
    /// A press commits to `count` rounds `gap` seconds apart, which leave
    /// even if the trigger is let go.
    Burst {
        /// Rounds a press commits to.
        count: u8,
        /// Seconds between the rounds of a burst.
        gap: f32,
    },
    /// A round every `interval` while the trigger is held.
    Auto,
}

impl FireMode {
    /// Why this mode cannot be played, if it cannot: a burst of nothing or
    /// a negative gap, which would fire a round every tick.
    ///
    /// # Errors
    /// A sentence naming what is wrong.
    pub fn check(&self) -> Result<(), String> {
        if let Self::Burst { count, gap } = *self {
            if count == 0 {
                return Err("a burst of nought rounds".to_owned());
            }
            if !gap.is_finite() || gap < 0. {
                return Err("a burst's gap is negative".to_owned());
            }
        }
        Ok(())
    }
}

/// What a trigger held past a burst's last round does: keeps firing, and
/// pays for it in cone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hold {
    /// Seconds between the rounds of the hold.
    pub gap: f32,
    /// Degrees each round of the hold adds to the cone, on top of the
    /// row's own bloom.
    pub bloom: f32,
}

/// How a weapon's cone grows as it fires and closes again.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bloom {
    /// Degrees a round adds to the cone.
    pub shot: f32,
    /// The most the added cone may reach, in degrees.
    pub cap: f32,
    /// Seconds after the last round before the cone starts to close.
    pub wait: f32,
    /// Degrees a second it then closes by.
    pub settle: f32,
}

/// How a bolt's damage fades with the distance it has flown.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Falloff {
    /// Metres a bolt deals all of its damage to.
    pub full: f32,
    /// Metres at which it has fallen to `share`.
    pub far: f32,
    /// The share of damage and posture kept from `far` on.
    pub share: f32,
}

impl Falloff {
    /// The share of a bolt's damage and posture it deals after `flown`
    /// metres: all of it up to `full`, a straight line down to `share` at
    /// `far`, and `share` beyond. The one function the bolt and the
    /// screens ask.
    #[must_use]
    pub fn share_at(self, flown: f32) -> f32 {
        if flown <= self.full {
            1.
        } else if flown >= self.far {
            self.share
        } else {
            let along = (flown - self.full) / (self.far - self.full);
            1. + (self.share - 1.) * along
        }
    }
}

/// What a weapon does per shot, independent of who holds it. A row of
/// `assets/combat/weapons.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeaponSpec {
    /// Seconds between shots; in a burst mode, from a burst's last round to
    /// the next burst's first.
    pub interval: f32,
    /// Bolt speed, metres per second.
    pub bolt_speed: f32,
    /// Health damage before shields, per bolt.
    pub damage: f32,
    /// Posture damage per bolt.
    pub posture: f32,
    /// Rounds per magazine.
    pub magazine: u16,
    /// Seconds a reload takes.
    pub reload: f32,
    /// Half angle, in degrees, of the cone a bolt leaves in from the hip.
    pub spread: f32,
    /// Half angle, in degrees, of that cone while aiming. A sight moves
    /// this one and not the hip's.
    pub aim_spread: f32,
    /// How a press becomes rounds.
    pub mode: FireMode,
    /// What a trigger held past a burst does, if anything.
    #[serde(default)]
    pub hold: Option<Hold>,
    /// How the cone grows and closes.
    pub bloom: Bloom,
    /// Degrees added to the cone at a full run.
    pub move_spread: f32,
    /// Metres a bolt flies before it dies.
    pub range: f32,
    /// How damage fades along that range.
    pub falloff: Falloff,
    /// Seconds the cone takes to close from the hip figure to the aimed one
    /// once aim is held, and to open again once it is dropped. Nought steps
    /// at once.
    pub aim_time: f32,
    /// How many times the aimed view magnifies over the ordinary aim view;
    /// read by the client alone.
    pub zoom: f32,
}

/// What a screen says of a gun's trigger and reach, worked out here so no
/// screen decides a rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponFacts {
    /// How a press turns into rounds.
    pub mode: FireMode,
    /// A trigger held past a burst keeps firing.
    pub hold: bool,
    /// Metres of full damage.
    pub full: f32,
    /// Metres at which the damage has fallen to its share.
    pub far: f32,
    /// The share kept from `far` on, in whole per cent.
    pub share_percent: u16,
    /// Metres a bolt flies before it dies.
    pub reach: f32,
    /// Seconds the aim takes to settle; nought when it settles at once.
    pub aim_time: f32,
}

impl WeaponSpec {
    /// What a melee class reads where a gun has its row: no magazine and no
    /// bolt, and a damage and a posture of one, which are not figures but
    /// the unit a fitted factor multiplies (the cut's own figures come from
    /// the action rows and the class).
    pub const MELEE: Self = Self {
        interval: 1.,
        bolt_speed: 0.,
        damage: 1.,
        posture: 1.,
        magazine: 0,
        reload: 0.,
        spread: 0.,
        aim_spread: 0.,
        mode: FireMode::Single,
        hold: None,
        bloom: Bloom {
            shot: 0.,
            cap: 0.,
            wait: 0.,
            settle: 0.,
        },
        move_spread: 0.,
        range: 1.,
        falloff: Falloff {
            full: 0.,
            far: 0.,
            share: 1.,
        },
        aim_time: 0.,
        zoom: 1.,
    };

    /// The trigger and reach of this row for a screen, the share rounded to
    /// a whole per cent once, here.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share between nought and one, as per cent"
    )]
    pub fn facts(&self) -> WeaponFacts {
        WeaponFacts {
            mode: self.mode,
            hold: self.hold.is_some(),
            full: self.falloff.full,
            far: self.falloff.far,
            share_percent: (self.falloff.share.clamp(0., 1.) * 100.).round() as u16,
            reach: self.range,
            aim_time: self.aim_time,
        }
    }

    /// The share of a bolt's damage and posture that reaches a target
    /// `flown` metres from the muzzle: the falloff's, and nothing past the
    /// range, where the bolt is already gone.
    #[must_use]
    pub fn share_at(&self, flown: f32) -> f32 {
        if flown > self.range {
            0.
        } else {
            self.falloff.share_at(flown)
        }
    }

    /// Why this row cannot be played, if it cannot: a negative gap, a
    /// burst of nothing, a falloff that ends before it starts or keeps
    /// more than it had, a range of nothing.
    ///
    /// # Errors
    /// A sentence naming the first dial that is wrong.
    pub fn check(&self) -> Result<(), String> {
        let finite = [
            self.interval,
            self.bolt_speed,
            self.damage,
            self.posture,
            self.reload,
            self.spread,
            self.aim_spread,
            self.bloom.shot,
            self.bloom.cap,
            self.bloom.wait,
            self.bloom.settle,
            self.move_spread,
            self.range,
            self.falloff.full,
            self.falloff.far,
            self.falloff.share,
            self.aim_time,
        ];
        if finite.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("a dial is negative or not a number".to_owned());
        }
        if self.interval <= 0. {
            return Err("interval must be above nought".to_owned());
        }
        if !self.zoom.is_finite() || self.zoom < 1. {
            return Err("zoom is under one".to_owned());
        }
        self.mode.check()?;
        if let Some(hold) = self.hold {
            if !hold.gap.is_finite() || hold.gap <= 0. || !hold.bloom.is_finite() || hold.bloom < 0.
            {
                return Err("a hold's gap must be above nought and its bloom not below".to_owned());
            }
            if !matches!(self.mode, FireMode::Burst { .. }) {
                return Err("a hold belongs to a burst".to_owned());
            }
        }
        if self.range <= 0. {
            return Err("range must be above nought".to_owned());
        }
        if self.falloff.far < self.falloff.full {
            return Err("falloff far is under full".to_owned());
        }
        if self.falloff.share > 1. {
            return Err("falloff share is over one".to_owned());
        }
        Ok(())
    }

    /// Change a field by its name in the file, for a progression effect.
    /// The magazine is a count: the result is rounded to whole rounds.
    /// Returns whether the field exists.
    pub fn modify(&mut self, field: &str, op: Op, value: f32) -> bool {
        let slot = match field {
            "interval" => &mut self.interval,
            "bolt_speed" => &mut self.bolt_speed,
            "damage" => &mut self.damage,
            "posture" => &mut self.posture,
            "reload" => &mut self.reload,
            "spread" => &mut self.spread,
            "aim_spread" => &mut self.aim_spread,
            "move_spread" => &mut self.move_spread,
            "range" => &mut self.range,
            "aim_time" => &mut self.aim_time,
            "magazine" => {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a magazine is a few dozen rounds, never negative"
                )]
                {
                    self.magazine =
                        op.apply(f32::from(self.magazine), value).round().max(0.) as u16;
                }
                return true;
            }
            _ => return false,
        };
        *slot = op.apply(*slot, value);
        true
    }
}

/// Turn `direction` (a unit vector) to a point uniform over the disc of a
/// cone of `half_angle_degrees` round it. `draw` is two numbers in `0..1`:
/// the radius (by its square root, so the disc is even) and the turn round
/// the axis. A half angle of nothing returns `direction` untouched, so a
/// weapon without spread flies exactly as before.
#[must_use]
pub fn deviate(direction: Vec3, half_angle_degrees: f32, draw: [f32; 2]) -> Vec3 {
    if half_angle_degrees <= 0. {
        return direction;
    }
    let helper = if direction.x.abs() < 0.6 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let u = direction.cross(helper).normalize();
    let v = direction.cross(u);
    let angle = half_angle_degrees.to_radians() * draw[0].sqrt();
    let turn = std::f32::consts::TAU * draw[1];
    let (sin, cos) = angle.sin_cos();
    (direction * cos + (u * turn.cos() + v * turn.sin()) * sin).normalize()
}

/// The two numbers of shot `count` for a shooter of `team` with `weapon`:
/// a hash of those three and nothing else, so two runs of one bout draw
/// alike (splitmix64, the finaliser of a counter-based generator).
#[must_use]
pub fn shot_draw(count: u64, team: u8, weapon: Weapon) -> [f32; 2] {
    let mut state =
        count ^ (u64::from(team) << 56) ^ ((weapon as u64) << 48) ^ 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        let z = strata_core::splitmix64(&mut state);
        // The top 24 bits are exactly representable, which keeps the
        // draw strictly below one.
        #[allow(
            clippy::cast_precision_loss,
            reason = "24 bits fit an f32 mantissa exactly"
        )]
        {
            (z >> 40) as f32 / 16_777_216.
        }
    };
    [next(), next()]
}

impl Weapon {
    /// Every weapon, in the order the registry's vocabulary lists them.
    pub const ALL: [Self; 6] = [
        Self::Carbine,
        Self::Pistol,
        Self::Blade,
        Self::Greatblade,
        Self::Glaive,
        Self::Rifle,
    ];

    /// The guns: the weapons `weapons.ron` has a row for.
    pub const GUNS: [Self; 3] = [Self::Carbine, Self::Pistol, Self::Rifle];

    /// The melee classes: the weapons `melee.ron` has a row for.
    pub const MELEE: [Self; 3] = [Self::Blade, Self::Greatblade, Self::Glaive];

    /// Whether this is a melee class, which fights by the cuts and has no
    /// magazine.
    #[must_use]
    pub const fn is_melee(self) -> bool {
        matches!(self, Self::Blade | Self::Greatblade | Self::Glaive)
    }

    /// The variant's name, as a table keys it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Carbine => "Carbine",
            Self::Pistol => "Pistol",
            Self::Blade => "Blade",
            Self::Greatblade => "Greatblade",
            Self::Glaive => "Glaive",
            Self::Rifle => "Rifle",
        }
    }

    /// The variant a table or a box names, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }

    /// This weapon's row of the installed tables
    /// ([`combat_tables`]), where the numbers live. An actor reads its own
    /// row through [`Combatant::weapon_spec`] instead, which is this unless
    /// a sheet changed it.
    #[must_use]
    pub fn spec(self) -> WeaponSpec {
        combat_tables::with(|tables| tables.weapons.spec(self))
    }
}

/// Timings are in seconds and displacement in metres, independent of clip FPS.
/// A row of `assets/combat/actions.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActionSpec {
    /// Contact starts after readable anticipation.
    pub contact_start: f32,
    /// Contact is a half-open interval.
    pub contact_end: f32,
    /// Locomotion recovers here.
    pub duration: f32,
    /// Dodge can cancel the late recovery, after the damage window.
    pub dodge_cancel: f32,
    /// Paid once when an action is accepted.
    pub stamina: f32,
    /// Health damage before shields.
    pub damage: f32,
    /// Posture damage before a stagger.
    pub posture: f32,
    /// Forward displacement during the anticipation/contact interval.
    pub travel: f32,
}

impl ActionSpec {
    /// The row of an action with nothing in it: what [`Action::Ability`]
    /// reads from the tables, since its row is the ability's own.
    pub const NONE: Self = Self {
        contact_start: 0.,
        contact_end: 0.,
        duration: 0.,
        dodge_cancel: 0.,
        stamina: 0.,
        damage: 0.,
        posture: 0.,
        travel: 0.,
    };

    /// A field by its name in the file, for a progression effect to change.
    #[must_use]
    pub fn field_mut(&mut self, field: &str) -> Option<&mut f32> {
        Some(match field {
            "contact_start" => &mut self.contact_start,
            "contact_end" => &mut self.contact_end,
            "duration" => &mut self.duration,
            "dodge_cancel" => &mut self.dodge_cancel,
            "stamina" => &mut self.stamina,
            "damage" => &mut self.damage,
            "posture" => &mut self.posture,
            "travel" => &mut self.travel,
            _ => return None,
        })
    }
}

impl Action {
    /// Every action a table has a row for (not [`Action::Ability`], whose row
    /// is the ability's), for a table to be checked against.
    pub const ALL: [Self; 9] = [
        Self::Ready,
        Self::Light,
        Self::Followup,
        Self::Heavy,
        Self::Dodge,
        Self::Reload,
        Self::Stagger,
        Self::Swap,
        Self::Dead,
    ];

    /// The variant's name, as a table keys it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Light => "Light",
            Self::Followup => "Followup",
            Self::Heavy => "Heavy",
            Self::Dodge => "Dodge",
            Self::Reload => "Reload",
            Self::Stagger => "Stagger",
            Self::Swap => "Swap",
            Self::Dead => "Dead",
            Self::Ability => "Ability",
        }
    }

    /// The variant a table or a box names, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    /// This action's row of the installed tables ([`combat_tables`]),
    /// where the numbers live; a deliberately small moveset. An actor
    /// reads its own row through [`Combatant::spec`] instead, which is
    /// this unless a sheet changed it.
    #[must_use]
    pub fn spec(self) -> ActionSpec {
        combat_tables::with(|tables| tables.actions.spec(self))
    }

    /// Melee actions issue swept contact intervals.
    #[must_use]
    pub const fn is_melee(self) -> bool {
        matches!(self, Self::Light | Self::Followup | Self::Heavy)
    }
}

/// A cut's time in its baked clip, `elapsed` seconds into the action: the
/// clip itself up to the end of contact, then the tail played over its
/// scaled length, so a part's recovery factor slows or quickens the
/// recovery and nothing before it. Continuous at the end of contact, and
/// `elapsed` to the bit at a factor of one.
#[must_use]
pub fn cut_clip_time(elapsed: f32, contact_end: f32, recovery: f32) -> f32 {
    if recovery.to_bits() == 1_f32.to_bits() || elapsed <= contact_end {
        elapsed
    } else {
        contact_end + (elapsed - contact_end) / recovery
    }
}

/// Input edges are latched until a simulation tick consumes them.
#[derive(Component, Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent held buttons and latched edges"
)]
pub struct CombatIntent {
    /// Held trigger, rate limited by the simulation.
    pub fire: bool,
    /// Held aim, reducing locomotion speed.
    pub aim: bool,
    /// Light-attack edge.
    pub light: bool,
    /// Heavy-attack edge.
    pub heavy: bool,
    /// Dodge edge.
    pub dodge: bool,
    /// Reload edge.
    pub reload: bool,
    /// Draw the weapon in this slot (0 or 1, the keys 1 and 2); ignored
    /// when it is already in hand or the slot is empty.
    pub weapon: Option<usize>,
    /// Press the place of the bar with this index: 0, 1 and 2 are the three
    /// places of the weapon class in hand, 3 and 4 the utility places. An
    /// edge, taken by the next step.
    pub ability: Option<usize>,
    /// Crosshair intersection, in world coordinates. The muzzle checks cover again.
    pub aim_point: WorldPos,
}

impl Default for CombatIntent {
    fn default() -> Self {
        Self {
            fire: false,
            aim: false,
            light: false,
            heavy: false,
            dodge: false,
            reload: false,
            weapon: None,
            ability: None,
            aim_point: WorldPos::new(0., 0., -100.),
        }
    }
}

/// The combat layer constrains the motor without changing its collision policy.
#[derive(Component, Debug, Clone, Copy)]
pub struct CombatMotion {
    /// Analog locomotion multiplier when no authored displacement is active.
    pub speed_scale: f32,
    /// Exact average velocity of this tick's authored displacement curve.
    pub velocity: Option<Vec3>,
}

impl Default for CombatMotion {
    fn default() -> Self {
        Self {
            speed_scale: 1.,
            velocity: None,
        }
    }
}

/// A press the actor's sheet refused: a weapon it is not certified on, an
/// action no box of its has opened. Dropped, not buffered, and announced
/// through [`Combatant::refusals`] so a HUD can say why nothing happened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal {
    /// The trigger on a weapon the actor is not certified on.
    Fire(Weapon),
    /// A draw of a weapon the actor is not certified on.
    Draw(Weapon),
    /// A cut asked with a gun in hand.
    NoBlade,
    /// An action the actor's boxes have not opened.
    Action(Action),
    /// A press of a place of the bar the rule of [`can_use`] refused.
    Ability(AbilityRefusal),
}

/// An ability's effect under way: its row as it was at the press, whether
/// the windup has passed, and the rounds a shot still owes.
#[derive(Debug, Clone)]
struct Active {
    row: AbilityRow,
    fired: bool,
    owed: u8,
    /// The age at which the next owed round leaves.
    next: f32,
    /// The direction a movement carries the user, fixed at the windup's end.
    move_dir: Option<Vec3>,
    /// The age at which the strike after a movement lands, until it has.
    then_due: Option<f32>,
}

/// A blow an ability's strike lands on what stands in front of the actor,
/// for the physics to resolve once.
#[derive(Debug, Clone, PartialEq)]
pub struct StrikeBlow {
    /// Metres from the actor.
    pub reach: f32,
    /// Degrees either side of the facing.
    pub arc: f32,
    /// Health damage.
    pub damage: f32,
    /// Posture damage.
    pub posture: f32,
    /// A status the blow puts on what it lands on unguarded.
    pub on_hit: Option<OnHit>,
}

/// State needed to resume or eventually reconcile a combatant's current action.
#[derive(Component, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent latches of the trigger and the scheduler"
)]
#[require(CombatIntent, CombatMotion)]
pub struct Combatant {
    /// Allies do not damage one another in the study.
    pub team: u8,
    /// Remaining health. Zero is terminal until encounter reset.
    pub health: f32,
    /// A small shield absorbs incoming damage before health.
    pub shield: f32,
    /// Recovery cannot exceed the equipment's shield capacity.
    pub shield_capacity: f32,
    /// Commitment budget; ordinary movement and shooting cost none.
    pub stamina: f32,
    /// A break interrupts the current action, including a heavy windup.
    pub posture: f32,
    /// The action currently owning the body.
    pub action: Action,
    /// Seconds since the accepted action began.
    pub age: f32,
    /// Monotonic identity prevents repeated contact in a single swing.
    pub sequence: u64,
    /// Shots fired, independent of rendering frame rate. The count (with
    /// `shot_salt`) seeds each shot's draw, so a bolt's turn inside its cone
    /// is a function of how many it follows.
    pub shots: u64,
    /// Added to the shot count before the draw. Zero in play; a bout of the
    /// balance harness sets it from its seed, so its bouts sample the cone
    /// instead of all firing the same first shots.
    pub shot_salt: u64,
    /// A presentation pulse for confirmed incoming hits.
    pub hurts: u64,
    /// Rounds left in the current magazine.
    pub ammo: u16,
    /// The weapon in hand.
    pub weapon: Weapon,
    /// Rounds left in the holstered weapon's magazine; none for a melee
    /// class or an empty slot.
    pub holstered_ammo: u16,
    /// The family each of the two weapon slots holds, if it holds one. The
    /// weapon in hand is one of them; the other is the one a draw swaps to.
    slots: [Option<Weapon>; 2],
    /// Heading locked during committed actions.
    pub facing: f32,
    /// Last accepted direction of authored movement.
    pub travel_direction: Vec3,
    /// The guard is up: a Ready actor with a melee weapon in hand and the
    /// aim button held. The view raises the blade for as long as this is
    /// true, and a hit meets it.
    pub guard_up: bool,
    /// Seconds the guard has been up; the parry window counts from the
    /// raise.
    guard_for: f32,
    /// Seconds the guard has been down, which `parry_rest` is measured
    /// against.
    guard_off: f32,
    /// Seconds since the guard last met a cut, and a very long time for one
    /// that never has; a bolt on the guard does not count.
    guard_met: f32,
    /// Seconds a guard must have been down to parry when raised, in place
    /// of `melee.ron`'s `parry_rest`. None for everyone but a trained
    /// duelist, whose parry is the hand's skill and not a rested button.
    pub parry_rest: Option<f32>,
    /// Whether the guard had been down for `parry_rest` when it was raised:
    /// only a rested guard parries.
    guard_rested: bool,
    /// Whether the guard button was down last tick: a guard that comes back
    /// up under a held button was not raised by the player and never parries.
    guard_held: bool,
    /// Whether the view adopts the aimed pose, and the shot leaves in the
    /// weapon's aimed cone instead of its hip cone.
    pub aiming: bool,
    /// Damage multiplier lets encounter opponents pressure without a player damage nerf.
    pub damage_scale: f32,
    /// Finite, non-homing projectile speed in metres per second.
    pub bolt_speed: f32,
    /// How this actor fires whatever its weapon's row says; an opponent's
    /// is set from its table, `None` reads the row's.
    pub mode_override: Option<FireMode>,
    /// Telegraph strength, authored by AI before it releases a shot.
    pub charge: f32,
    /// A presentation pulse for presses the sheet refused, one a press;
    /// [`Combatant::refused`] says which.
    pub refusals: u64,
    /// The last press the sheet refused.
    pub refused: Option<Refusal>,
    /// This actor's own rows, once a sheet has been fitted; `None` reads
    /// the installed tables live and may do everything, which is what an
    /// actor with no sheet (the harness's sides, a bare test) gets.
    effective: Option<Effective>,
    /// What is fitted to this actor's weapons, and the rows that makes of
    /// them; `None` with nothing fitted, when every read is the sheet's
    /// row or the tables' as if fitting did not exist. That is the whole
    /// guarantee for an opponent, a harness side and a bare player, and
    /// why taking a module off restores the row to the bit.
    equipped: Option<Arc<Equipped>>,
    queued: Option<(Action, f32)>,
    shot_wait: f32,
    /// Whether the trigger was down last tick, for the press edge.
    trigger: bool,
    /// Seconds a press that could not fire yet stays good for, so a hand a
    /// little early still gets its shot; the actor table's `buffer`.
    press_wait: f32,
    /// The trigger has stayed down since the burst's opening press, which is
    /// what lets the burst run on into a hold.
    unbroken: bool,
    /// Rounds a committed burst still owes.
    burst_left: u8,
    /// A hold is firing: the trigger stayed down past the burst.
    holding: bool,
    /// How far the aim has closed, nought at the hip and one aimed, which
    /// the row's `aim_time` moves while aim is held and back while it is
    /// not.
    aim_share: f32,
    /// Degrees the cone has grown by firing.
    bloom: f32,
    /// Seconds before the bloom starts to settle.
    bloom_wait: f32,
    /// The share of full pace the actor asked to move at last tick.
    move_share: f32,
    /// The cone the last round left in, before its own bloom.
    shot_cone: f32,
    recovery_wait: f32,
    hurt_wait: f32,
    /// The shield's own wait: a hurt starts it with `hurt_wait`, and the end
    /// of Shield broken starts it alone, so the posture does not stall too.
    shield_wait: f32,
    /// A factor on the length of the stagger under way: one, or the melee
    /// table's own when a caught cut was what broke the bearer.
    stagger_scale: f32,
    combo_wait: f32,
    /// The trigger is held on an uncertified weapon and has been refused
    /// once already; a held trigger is one refusal, not one a tick.
    fire_refused: bool,
    /// The member's bar, as the sim copied it in.
    bar: Arc<Bar>,
    /// Seconds left of each ability's cooldown, by id; the member's, so a
    /// swap does not reset them.
    cooldowns: BTreeMap<String, f32>,
    /// The statuses borne and the seconds each has left.
    statuses: Vec<(String, f32)>,
    /// The ability under way, while the action is [`Action::Ability`].
    active: Option<Active>,
    /// A press that found the actor busy, kept for the buffer.
    ability_press: Option<(usize, f32)>,
    /// A cut ability pressed and not yet begun: its action and the cooldown
    /// it starts once the cut does, so a cut the buffer drops costs nothing.
    pending_cut: Option<(Action, String, f32)>,
    /// The factors on the damage and posture of the bolt this tick's step
    /// let leave: one, one for a round of the scheduler.
    shot_factors: (f32, f32),
    /// A strike the windup's end landed this tick.
    strike: Option<StrikeBlow>,
    /// What the bolt this tick's step let leave carries beyond its damage.
    shot_effects: ShotEffects,
}

/// Which kind of round the scheduler is letting leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    /// A single shot, or an automatic weapon's round.
    Plain,
    /// The first round of a burst.
    Open,
    /// A later round of a committed burst.
    Burst,
    /// A round of the hold after a burst.
    Hold,
}

/// A loadout and the rows it gives the weapons it changes.
#[derive(Debug, Clone, PartialEq)]
struct Equipped {
    loadout: Loadout,
    /// One row a weapon with a fitted factor; a weapon with none is not
    /// here and reads as it always did.
    rows: BTreeMap<Weapon, Row>,
}

impl Default for Combatant {
    /// A fresh actor on the installed tables' pools, the carbine in hand
    /// and the pistol holstered, both full.
    fn default() -> Self {
        let (actor, carbine, pistol) = combat_tables::with(|tables| {
            (
                tables.actor.clone(),
                tables.weapons.spec(Weapon::Carbine),
                tables.weapons.spec(Weapon::Pistol),
            )
        });
        Self {
            team: 0,
            health: actor.health,
            shield: actor.shield,
            shield_capacity: actor.shield,
            stamina: actor.stamina,
            posture: actor.posture,
            action: Action::Ready,
            age: 0.,
            sequence: 0,
            shots: 0,
            shot_salt: 0,
            hurts: 0,
            ammo: carbine.magazine,
            weapon: Weapon::Carbine,
            holstered_ammo: pistol.magazine,
            slots: [Some(Weapon::Carbine), Some(Weapon::Pistol)],
            facing: 0.,
            travel_direction: Vec3::NEG_Z,
            guard_up: false,
            guard_for: 0.,
            guard_met: f32::MAX,
            guard_off: f32::INFINITY,
            parry_rest: None,
            guard_rested: true,
            guard_held: false,
            aiming: false,
            damage_scale: 1.,
            bolt_speed: carbine.bolt_speed,
            mode_override: None,
            charge: 0.,
            refusals: 0,
            refused: None,
            effective: None,
            equipped: None,
            queued: None,
            shot_wait: 0.,
            trigger: false,
            press_wait: 0.,
            unbroken: false,
            burst_left: 0,
            holding: false,
            bloom: 0.,
            bloom_wait: 0.,
            move_share: 0.,
            aim_share: 0.,
            shot_cone: 0.,
            recovery_wait: 0.,
            hurt_wait: 0.,
            shield_wait: 0.,
            stagger_scale: 1.,
            combo_wait: 0.,
            fire_refused: false,
            bar: Arc::new(Bar::default()),
            cooldowns: BTreeMap::new(),
            statuses: Vec::new(),
            active: None,
            ability_press: None,
            pending_cut: None,
            shot_factors: (1., 1.),
            strike: None,
            shot_effects: ShotEffects::default(),
        }
    }
}

impl Combatant {
    /// Spawn an actor without exposing the action scheduler's internal timers.
    #[must_use]
    pub fn on_team(team: u8, health: f32) -> Self {
        Self {
            team,
            health,
            ..Self::default()
        }
    }

    /// Fit a sheet: from now on this actor reads the installed tables with
    /// the sheet's boxes merged in ([`apply`]) and may begin only what its
    /// boxes unlock and fire only what they certify. Called at spawn and
    /// after every learn or forget. Pools are kept; one that was full
    /// follows its cap, so a box that raises the posture cap is felt at
    /// once and one that lowers it clamps. The shield's capacity follows
    /// the same way, so an opponent's own capacity from `opponents.ron`
    /// (never the actor table's) is left alone.
    pub fn fit(&mut self, graph: &SkillGraph, sheet: &CharacterSheet) {
        let old = self.with_actor(ActorTable::clone);
        let effective = combat_tables::with(|tables| apply(tables, graph, sheet));
        let new = effective.actor.clone();
        self.effective = Some(Effective::new(effective));
        let follow = |pool: f32, old_cap: f32, new_cap: f32| {
            if pool >= old_cap {
                new_cap
            } else {
                pool.min(new_cap)
            }
        };
        self.health = follow(self.health, old.health, new.health);
        self.shield_capacity = follow(self.shield_capacity, old.shield, new.shield);
        self.shield = follow(self.shield, old.shield, self.shield_capacity);
        self.stamina = follow(self.stamina, old.stamina, new.stamina);
        self.posture = follow(self.posture, old.posture, new.posture);
        // The boxes come before the modules in the merge, so a new sheet
        // is new rows under the same loadout. An actor with nothing fitted
        // is left exactly as a fit always left it.
        if let Some(equipped) = self.equipped.clone() {
            self.equip(&equipped.loadout);
        }
    }

    /// Equip a loadout: from now on each weapon it has factors for is read
    /// through its own row, [`Combatant::bare`] with every factor applied
    /// once ([`merge`]). An empty loadout unequips, and every read is the
    /// sheet's or the tables' again. Called when what is fitted changes and
    /// after every [`Combatant::fit`]. The rounds in both magazines are
    /// kept and clamped to their new sizes, never refilled: a larger
    /// magazine is filled by a reload, not by fitting it.
    pub fn equip(&mut self, loadout: &Loadout) {
        self.equipped = None;
        let rows: BTreeMap<Weapon, Row> = loadout
            .mods
            .iter()
            .filter(|(_, factors)| !factors.is_empty())
            .map(|(weapon, factors)| (*weapon, merge(&self.bare(*weapon), factors, loadout.count)))
            .collect();
        if !rows.is_empty() {
            self.equipped = Some(Arc::new(Equipped {
                loadout: loadout.clone(),
                rows,
            }));
        }
        self.ammo = self.ammo.min(self.spec_of(self.weapon).magazine);
        self.holstered_ammo = self.holstered_ammo.min(
            self.holstered()
                .map_or(0, |weapon| self.spec_of(weapon).magazine),
        );
    }

    /// The family each weapon slot holds.
    #[must_use]
    pub const fn slots(&self) -> [Option<Weapon>; 2] {
        self.slots
    }

    /// The slot the weapon in hand is in. A weapon set into `weapon` by
    /// hand and not in either slot reads as the first.
    #[must_use]
    pub fn hand(&self) -> usize {
        usize::from(self.slots[0] != Some(self.weapon) && self.slots[1] == Some(self.weapon))
    }

    /// The weapon in the slot that is not in hand, if that slot holds one.
    #[must_use]
    pub fn holstered(&self) -> Option<Weapon> {
        self.slots[1 - self.hand()]
    }

    /// Carry `slots`, `hand` of them in hand, each gun full. For an actor
    /// built with a weapon other than the default two: an opponent, a
    /// harness side, a test.
    #[must_use]
    pub fn carrying(mut self, slots: [Option<Weapon>; 2], hand: usize) -> Self {
        let hand = hand.min(1);
        self.slots = slots;
        if let Some(weapon) = slots[hand] {
            self.weapon = weapon;
        }
        let full =
            |weapon: Option<Weapon>| weapon.map_or(0, |weapon| self.spec_of(weapon).magazine);
        let (ammo, holstered) = (full(Some(self.weapon)), full(slots[1 - hand]));
        self.ammo = ammo;
        self.holstered_ammo = holstered;
        self.bolt_speed = self.weapon_spec().bolt_speed;
        self
    }

    /// The weapon slots follow what the hands now carry. A slot whose
    /// family is unchanged keeps its magazine; a gun new to a slot comes
    /// with a full one (the sim keeps no stock of rounds, a reload is free
    /// and a take should not leave the new gun empty). If the slot in hand
    /// changed its weapon, the new one is in hand; if it was emptied, the
    /// other is drawn, and with both empty nothing is touched.
    pub fn carry(&mut self, slots: [Option<Weapon>; 2]) {
        if slots == self.slots {
            return;
        }
        let hand = self.hand();
        let rounds = [
            if hand == 0 {
                self.ammo
            } else {
                self.holstered_ammo
            },
            if hand == 1 {
                self.ammo
            } else {
                self.holstered_ammo
            },
        ];
        let old = self.slots;
        let mut next = [0, 0];
        for slot in 0..2 {
            next[slot] = match slots[slot] {
                Some(weapon) if old[slot] == Some(weapon) => rounds[slot],
                Some(weapon) => self.spec_of(weapon).magazine,
                None => 0,
            };
        }
        self.slots = slots;
        let drawn = if slots[hand].is_some() {
            Some(hand)
        } else if slots[1 - hand].is_some() {
            Some(1 - hand)
        } else {
            None
        };
        let Some(drawn) = drawn else {
            return;
        };
        let changed = slots[drawn] != Some(self.weapon);
        if let Some(weapon) = slots[drawn] {
            self.weapon = weapon;
        }
        self.ammo = next[drawn];
        self.holstered_ammo = next[1 - drawn];
        if changed {
            // What was queued or under way belonged to the weapon put away.
            self.queued = None;
            if self.action.is_melee() {
                self.action = Action::Ready;
                self.age = 0.;
            }
            self.bolt_speed = self.weapon_spec().bolt_speed;
            self.bloom = 0.;
            self.bloom_wait = 0.;
            self.aim_share = 0.;
            self.burst_left = 0;
            self.holding = false;
        }
    }

    /// The class row of the melee weapon in hand, if one is.
    fn melee_class(&self) -> Option<MeleeClass> {
        let mut class = self
            .weapon
            .is_melee()
            .then(|| combat_tables::with(|tables| tables.melee.class(self.weapon)))
            .flatten()?;
        if let Some(guard) = self.guard_row(self.weapon) {
            class.guard = guard;
        }
        Some(class)
    }

    /// The guard row of a melee class as this actor has it: the table's,
    /// after its sheet's `Guard` effects when it was fitted. The one reader
    /// of the guard, so a screen shows the figures a blow meets.
    #[must_use]
    pub fn guard_row(&self, weapon: Weapon) -> Option<GuardRow> {
        match self.effective() {
            Some(effective) => effective.guards.get(&weapon).copied(),
            None => combat_tables::with(|tables| tables.melee.class(weapon).map(|c| c.guard)),
        }
    }

    /// Seconds since this actor's guard last met a cut: what a brain that
    /// can be held behind its guard asks. A bolt on the guard is not counted,
    /// and an actor whose guard never met a cut answers a very long time.
    #[must_use]
    pub fn guard_met_for(&self) -> f32 {
        self.guard_met
    }

    /// Put `row` in as the guard of `weapon` in this actor's own tables, after
    /// it is fitted: the shield bearer's guard is its own and not a class's.
    /// [`Combatant::guard_row`] stays the one reader. Does nothing for an
    /// actor never fitted, which reads the tables live and has no copy to
    /// change.
    #[must_use]
    pub fn with_guard(mut self, weapon: Weapon, row: GuardRow) -> Self {
        if let Some(effective) = &mut self.effective {
            std::sync::Arc::make_mut(effective)
                .guards
                .insert(weapon, row);
        }
        self
    }

    /// The factor on how far the sweep reaches along the baked blade: the
    /// class's, one for anything else.
    #[must_use]
    pub fn reach_factor(&self) -> f32 {
        self.melee_class().map_or(1., |class| class.reach)
    }

    /// The sweep's reach factor of a class by family, for a view that
    /// shows a weapon other than the one in hand mid swap; one for a gun.
    #[must_use]
    pub fn reach_of(weapon: Weapon) -> f32 {
        weapon
            .is_melee()
            .then(|| combat_tables::with(|tables| tables.melee.class(weapon)))
            .flatten()
            .map_or(1., |class| class.reach)
    }

    /// The length the baked clip is played over for `action`: the table's
    /// row scaled by the class's time, so the clip plays slower with the
    /// class and the sweep follows it. Not the sheet's: the clip was never
    /// retimed by a box.
    #[must_use]
    pub fn clip_duration(&self, action: Action) -> f32 {
        let time = self
            .melee_class()
            .and_then(|class| class.scale(action).map(|scale| scale.time))
            .unwrap_or(1.);
        action.spec().duration * time
    }

    /// What is fitted, if anything is.
    #[must_use]
    pub fn loadout(&self) -> Option<&Loadout> {
        self.equipped.as_deref().map(|equipped| &equipped.loadout)
    }

    /// `weapon`'s row with nothing fitted: the tables' with this actor's
    /// boxes merged in, the draw its Swap row gives, its aimed pace, and a
    /// cut at its full cost. What a loadout is merged onto, and what a
    /// screen sets a fitted figure against.
    #[must_use]
    pub fn bare(&self, weapon: Weapon) -> Row {
        Row {
            spec: self.sheet_spec_of(weapon),
            draw: self.sheet_spec(Action::Swap).duration,
            aim_speed: self.with_actor(|actor| actor.aim_speed_scale),
            cut_stamina: 1.,
            recovery: 1.,
            tail: self.bare_tail(weapon),
            melee: weapon.is_melee(),
        }
    }

    /// Seconds from the end of the light cut's contact to its end for
    /// `weapon` with nothing fitted: the sheet's row retimed by the class.
    /// Nought for a gun.
    fn bare_tail(&self, weapon: Weapon) -> f32 {
        if !weapon.is_melee() {
            return 0.;
        }
        let time = combat_tables::with(|tables| tables.melee.class(weapon))
            .and_then(|class| class.scale(Action::Light).map(|scale| scale.time))
            .unwrap_or(1.);
        let light = self.sheet_spec(Action::Light);
        (light.duration - light.contact_end) * time
    }

    /// The seconds `weapon` takes to draw: its fitted row's, or the Swap
    /// row's.
    #[must_use]
    pub fn fitted_draw(&self, weapon: Weapon) -> f32 {
        self.fitted_row(weapon)
            .map_or_else(|| self.sheet_spec(Action::Swap).duration, |row| row.draw)
    }

    /// The fitted row of `weapon`, when a loadout changes it.
    fn fitted_row(&self, weapon: Weapon) -> Option<&Row> {
        self.equipped.as_deref()?.rows.get(&weapon)
    }

    /// Announce a press the sheet refused.
    fn refuse(&mut self, refusal: Refusal) {
        self.refused = Some(refusal);
        self.refusals = self.refusals.wrapping_add(1);
    }

    /// The fitted rows, if a sheet was fitted.
    #[must_use]
    pub fn effective(&self) -> Option<&crate::progression::EffectiveTables> {
        self.effective.as_deref()
    }

    /// This actor's row for `action`: the fitted one, or the installed
    /// table's. A cut costs what the weapon in hand makes it cost, when
    /// something fitted to it says so.
    #[must_use]
    pub fn spec(&self, action: Action) -> ActionSpec {
        if action == Action::Ability {
            return self.active.as_ref().map_or(ActionSpec::NONE, |active| {
                let row = &active.row;
                ActionSpec {
                    contact_start: row.windup,
                    contact_end: row.windup,
                    duration: row.commitment(),
                    dodge_cancel: row.cancel.min(row.commitment()),
                    ..ActionSpec::NONE
                }
            });
        }
        let mut spec = self.sheet_spec(action);
        if !action.is_melee() {
            return spec;
        }
        // The sheet's row, scaled by the class in hand: a Blade's factors
        // are all one, so it reads the sheet's row to the bit.
        if let Some(class) = self.melee_class()
            && let Some(scale) = class.scale(action)
        {
            spec.contact_start *= scale.time;
            spec.contact_end *= scale.time;
            spec.duration *= scale.time;
            spec.dodge_cancel *= scale.time;
            spec.damage *= scale.damage;
            spec.posture *= scale.posture;
            spec.stamina *= scale.stamina;
            spec.travel *= scale.travel;
        }
        if let Some(row) = self.fitted_row(self.weapon) {
            spec.stamina *= row.cut_stamina;
            // A melee row holds one where a gun's holds its damage, so a
            // fitted or worn factor on it scales the cut.
            if self.weapon.is_melee() {
                spec.damage *= row.spec.damage;
                spec.posture *= row.spec.posture;
                // A part's recovery factor moves only what lies after the
                // contact, so a factor of one leaves every bit as it was.
                if row.recovery.to_bits() != 1_f32.to_bits() {
                    let end = spec.contact_end;
                    spec.duration = end + (spec.duration - end) * row.recovery;
                    if spec.dodge_cancel > end {
                        spec.dodge_cancel = end + (spec.dodge_cancel - end) * row.recovery;
                    }
                }
            }
        }
        spec
    }

    /// The factor the weapon in hand's fitted parts put on the tail of its
    /// cuts: one when nothing does.
    #[must_use]
    pub fn recovery_factor(&self) -> f32 {
        self.fitted_row(self.weapon)
            .filter(|_| self.weapon.is_melee())
            .map_or(1., |row| row.recovery)
    }

    /// How far through its baked clip `action` is, `elapsed` seconds in, as
    /// a share of the clip: the clip plays over the row's duration without
    /// the parts' recovery factor, so a longer tail stretches its end.
    #[must_use]
    pub fn clip_place(&self, action: Action, elapsed: f32) -> f32 {
        let spec = self.spec(action);
        let factor = self.recovery_factor();
        // Only a cut has a scaled tail; a dodge's clip plays as it always did.
        if !action.is_melee() || factor.to_bits() == 1_f32.to_bits() {
            return elapsed / spec.duration;
        }
        let whole = spec.contact_end + (spec.duration - spec.contact_end) / factor;
        cut_clip_time(elapsed, spec.contact_end, factor) / whole
    }

    /// The row for `action` by the sheet alone, whatever is fitted.
    fn sheet_spec(&self, action: Action) -> ActionSpec {
        self.effective
            .as_ref()
            .map_or_else(|| action.spec(), |e| e.spec(action))
    }

    /// This actor's row for `weapon`: its fitted row, or the sheet's.
    #[must_use]
    pub fn spec_of(&self, weapon: Weapon) -> WeaponSpec {
        self.fitted_row(weapon)
            .map_or_else(|| self.sheet_spec_of(weapon), |row| row.spec)
    }

    /// The row for `weapon` by the sheet alone, whatever is fitted.
    fn sheet_spec_of(&self, weapon: Weapon) -> WeaponSpec {
        self.effective
            .as_ref()
            .map_or_else(|| weapon.spec(), |e| e.weapon_spec(weapon))
    }

    /// The share of its pace this actor keeps while aiming, reloading or
    /// drawing: the weapon in hand's when something fitted changes it,
    /// else the actor table's.
    fn aim_speed(&self, actor: &ActorTable) -> f32 {
        self.fitted_row(self.weapon)
            .map_or(actor.aim_speed_scale, |row| row.aim_speed)
    }

    /// This actor's row for the weapon in hand.
    #[must_use]
    pub fn weapon_spec(&self) -> WeaponSpec {
        self.spec_of(self.weapon)
    }

    /// The half angle, in degrees, of the cone a bolt leaves in when
    /// `aiming`: the one reader of which end applies, for the shot and for
    /// the reticle that draws it.
    #[must_use]
    pub fn cone_when(&self, aiming: bool) -> f32 {
        let row = self.weapon_spec();
        if self.focused() {
            return row.aim_spread;
        }
        let end = if aiming { row.aim_spread } else { row.spread };
        end + self.bloom + row.move_spread * self.move_share
    }

    /// The cone the last round left in: the cone before that round added
    /// its own bloom, which is what the physics turns the bolt inside.
    #[must_use]
    pub fn shot_cone(&self) -> f32 {
        self.shot_cone
    }

    /// Degrees the cone has grown by firing, for a screen and a test.
    #[must_use]
    pub fn bloom(&self) -> f32 {
        self.bloom
    }

    /// Whether a press on the next tick of `dt` seconds would open a shot or
    /// a burst: ready, loaded, not mid burst and past the interval. For a
    /// policy that taps the trigger exactly when the weapon takes it.
    #[must_use]
    pub fn ready_for_press(&self, dt: f32) -> bool {
        self.action == Action::Ready
            && self.ammo > 0
            && self.burst_left == 0
            && self.shot_wait - dt <= 0.
    }

    /// How this actor's trigger works: its override, or its weapon's row.
    #[must_use]
    pub fn mode(&self) -> FireMode {
        self.mode_override.unwrap_or(self.weapon_spec().mode)
    }

    /// The cone the next shot leaves in. A weapon with an `aim_time` blends
    /// its two ends by how far the aim has closed, which also opens it again
    /// once aim is dropped; one without steps with `aiming`, to the bit.
    #[must_use]
    pub fn cone(&self) -> f32 {
        let row = self.weapon_spec();
        if self.focused() {
            return row.aim_spread;
        }
        if row.aim_time <= 0. {
            return self.cone_when(self.aiming);
        }
        let end = row.spread + (row.aim_spread - row.spread) * self.aim_share;
        end + self.bloom + row.move_spread * self.move_share
    }

    /// How far the aim has closed, nought at the hip and one aimed: what a
    /// screen blends the two [`Combatant::cone_when`] ends by.
    #[must_use]
    pub fn aim_share(&self) -> f32 {
        if self.weapon_spec().aim_time <= 0. {
            return f32::from(u8::from(self.aiming));
        }
        self.aim_share
    }

    /// Close the aim while it is held in a Ready hand, open it while it is
    /// not, and drop it to the hip in any other action: a reload, a draw, a
    /// dodge and a stagger each start it over.
    fn move_aim(&mut self, dt: f32) {
        // An ability keeps the aim as it was; it neither closes nor opens it.
        if self.action == Action::Ability {
            return;
        }
        let time = self.weapon_spec().aim_time;
        if self.action != Action::Ready || time <= 0. {
            self.aim_share = if self.action == Action::Ready && self.aiming {
                1.
            } else {
                0.
            };
            return;
        }
        let step = dt / time;
        self.aim_share = if self.aiming {
            (self.aim_share + step).min(1.)
        } else {
            (self.aim_share - step).max(0.)
        };
    }

    /// Copy the member's bar in, for a press to read its places from.
    pub fn set_bar(&mut self, bar: &Bar) {
        if *self.bar != *bar {
            self.bar = Arc::new(bar.clone());
        }
    }

    /// The bar the actor presses from.
    #[must_use]
    pub fn bar(&self) -> &Bar {
        &self.bar
    }

    /// The row of the ability `id` as this actor reads it: the table's with
    /// the sheet's `Ability` effects applied, or the table's alone for an
    /// actor never fitted. The one reader of an ability's figures.
    #[must_use]
    pub fn ability(&self, id: &str) -> Option<AbilityRow> {
        match &self.effective {
            Some(effective) => effective.abilities.get(id).cloned(),
            None => combat_tables::with(|tables| tables.abilities.row(id).cloned()),
        }
    }

    /// The place a press of `index` means with the weapon in hand.
    #[must_use]
    pub fn place_of(&self, index: usize) -> Option<Place> {
        let size = combat_tables::with(|tables| tables.abilities.bar);
        Place::of_press(index, self.weapon, size)
    }

    /// The ability in the place a press of `index` means, if one is there.
    #[must_use]
    pub fn ability_at(&self, index: usize) -> Option<AbilityRow> {
        let id = self.bar.get(self.place_of(index)?)?;
        self.ability(id)
    }

    /// Seconds left of the cooldown of the ability `id`.
    #[must_use]
    pub fn cooldown_left(&self, id: &str) -> f32 {
        self.cooldowns.get(id).copied().unwrap_or(0.)
    }

    /// What a press of `index` would do now, by the one rule of
    /// [`can_use`]: the ability that would begin, or why not. A screen asks
    /// this every frame to grey a place.
    ///
    /// # Errors
    /// [`AbilityRefusal::Empty`] for a place with nothing in it, else the
    /// rule's first refusal.
    pub fn can_use(&self, index: usize) -> Result<AbilityRow, AbilityRefusal> {
        let row = self.ability_at(index).ok_or(AbilityRefusal::Empty)?;
        can_use(&row, &self.use_state(&row)).map(|()| row)
    }

    /// The facts about this actor that [`can_use`] asks.
    fn use_state(&self, row: &AbilityRow) -> UseState {
        UseState {
            weapon: self.weapon,
            cooldown_left: self.cooldown_left(&row.id),
            stamina: self.stamina,
            ammo: self.ammo,
            magazine: self.magazine(),
            shield: self.shield,
            shield_capacity: self.shield_capacity,
            // An instant ability commits to nothing, so it can follow any
            // action but the two a body cannot act in.
            busy: if row.is_instant() {
                matches!(self.action, Action::Stagger | Action::Dead)
            } else {
                self.action != Action::Ready
            },
            certified: self.certified(self.weapon),
            disarmed: self.disarmed(),
        }
    }

    /// The id of the ability under way, if one is.
    #[must_use]
    pub fn ability_in_use(&self) -> Option<&str> {
        self.active
            .as_ref()
            .filter(|_| self.action == Action::Ability)
            .map(|active| active.row.id.as_str())
    }

    /// The statuses borne, each with the seconds it has left: the one
    /// reader of them.
    #[must_use]
    pub fn statuses(&self) -> &[(String, f32)] {
        &self.statuses
    }

    /// Bear the status `id` for `seconds`; one given again takes the longer
    /// time.
    pub fn give_status(&mut self, id: &str, seconds: f32) {
        match self.statuses.iter_mut().find(|(held, _)| held == id) {
            Some((_, left)) => *left = left.max(seconds),
            None => self.statuses.push((id.to_owned(), seconds)),
        }
    }

    /// Whether any status borne satisfies `test`: the one reader of the
    /// rows behind the flags below.
    fn borne(&self, test: impl Fn(&StatusRow) -> bool) -> bool {
        !self.statuses.is_empty()
            && combat_tables::with(|tables| {
                self.statuses
                    .iter()
                    .any(|(id, _)| tables.abilities.status(id).is_some_and(&test))
            })
    }

    /// Whether a status borne holds the cone at its tightest.
    #[must_use]
    pub fn focused(&self) -> bool {
        self.borne(|status| status.tight_cone)
    }

    /// Whether a status borne forbids every shot, cut and ability.
    #[must_use]
    pub fn disarmed(&self) -> bool {
        self.borne(|status| status.disarmed)
    }

    /// Whether a status borne holds the shield from recovering.
    #[must_use]
    pub fn shield_locked(&self) -> bool {
        self.borne(|status| status.shield_locked)
    }

    /// The share of moving speed the statuses borne leave: one when none
    /// slows.
    fn pace_share(&self) -> f32 {
        if self.statuses.is_empty() {
            return 1.;
        }
        combat_tables::with(|tables| {
            self.statuses
                .iter()
                .filter_map(|(id, _)| tables.abilities.status(id)?.pace)
                .product()
        })
    }

    /// The share of a guarded bolt's damage and posture this actor sends
    /// back at whoever fired it, if a status borne reflects.
    #[must_use]
    pub fn reflects(&self) -> Option<f32> {
        if self.statuses.is_empty() {
            return None;
        }
        combat_tables::with(|tables| {
            self.statuses
                .iter()
                .filter_map(|(id, _)| tables.abilities.status(id)?.reflects)
                .reduce(f32::max)
        })
    }

    /// The factor the statuses borne put on the posture damage taken.
    fn posture_taken(&self) -> f32 {
        combat_tables::with(|tables| {
            self.statuses
                .iter()
                .filter_map(|(id, _)| tables.abilities.status(id)?.posture_taken)
                .product()
        })
    }

    /// The factor the statuses borne put on the damage taken.
    fn damage_taken(&self) -> f32 {
        combat_tables::with(|tables| {
            self.statuses
                .iter()
                .filter_map(|(id, _)| tables.abilities.status(id)?.damage_taken)
                .product()
        })
    }

    /// The factors on the damage and posture of the bolt the last step let
    /// leave: one and one unless an ability's shot fired it.
    #[must_use]
    pub const fn shot_factors(&self) -> (f32, f32) {
        self.shot_factors
    }

    /// What the bolt the last step let leave carries beyond its damage: a
    /// status, a jump, a blast, a breach. Nothing for a round of the
    /// scheduler.
    #[must_use]
    pub const fn shot_effects(&self) -> &ShotEffects {
        &self.shot_effects
    }

    /// The strike the last step landed, taken: the physics resolves it once.
    pub fn take_strike(&mut self) -> Option<StrikeBlow> {
        self.strike.take()
    }

    /// Read this actor's actor table without cloning it.
    pub fn with_actor<T>(&self, read: impl FnOnce(&ActorTable) -> T) -> T {
        match &self.effective {
            Some(effective) => read(&effective.actor),
            None => combat_tables::with(|tables| read(&tables.actor)),
        }
    }

    /// Whether this actor may begin `action`: everything, with no sheet;
    /// otherwise what no box gates plus what its boxes unlocked.
    #[must_use]
    pub fn unlocked(&self, action: Action) -> bool {
        self.effective
            .as_ref()
            .is_none_or(|e| e.unlocked.contains(&action))
    }

    /// Whether this actor may fire and draw `weapon`; see
    /// [`Combatant::unlocked`].
    #[must_use]
    pub fn certified(&self, weapon: Weapon) -> bool {
        self.effective
            .as_ref()
            .is_none_or(|e| e.certified.contains(&weapon))
    }

    /// The action waiting in the buffer, for a HUD or a test.
    #[must_use]
    pub fn queued(&self) -> Option<Action> {
        self.queued.map(|(action, _)| action)
    }

    /// Whether this actor is fighting, for the rules that refuse while it
    /// is (forgetting a box): committed to an action, or `engaged` as the
    /// encounter sees it.
    #[must_use]
    pub fn in_combat(&self, engaged: bool) -> bool {
        self.action != Action::Ready || engaged
    }

    /// Rounds the weapon in hand holds when full; finite magazines create
    /// openings for melee and repositioning.
    #[must_use]
    pub fn magazine(&self) -> u16 {
        self.weapon_spec().magazine
    }

    /// Seconds the weapon in hand takes to reload; the view scales its
    /// overlay to it.
    #[must_use]
    pub fn reload_time(&self) -> f32 {
        self.weapon_spec().reload
    }

    /// Visible startup and recovery surround the invulnerable dodge
    /// interval, which is the Dodge row's contact interval: a dodge has no
    /// sweep, so the same two numbers mean when it cannot be struck.
    #[must_use]
    pub fn invulnerable(&self) -> bool {
        if self.action != Action::Dodge {
            return false;
        }
        let spec = self.spec(Action::Dodge);
        (spec.contact_start..spec.contact_end).contains(&self.age)
    }

    /// Resetting input cannot erase an already accepted commitment.
    pub fn clear_buffer(&mut self) {
        self.queued = None;
        self.ability_press = None;
        self.pending_cut = None;
    }

    /// Whether `begin` would take `action` now: the one rule an opponent's
    /// brain asks before it presses, so it never restates the stamina,
    /// weapon or disarm rules.
    #[must_use]
    pub fn can_begin(&self, action: Action) -> bool {
        // A cut queued across a draw must not begin once a gun is in hand.
        !((action.is_melee() && !self.weapon.is_melee())
            || self.stamina < self.spec(action).stamina
            || self.action == Action::Dead
            || !self.unlocked(action)
            || (action.is_melee() && self.disarmed()))
    }

    fn begin(&mut self, action: Action, movement: &PlayerIntent) -> bool {
        if !self.can_begin(action) {
            return false;
        }
        let spec = self.spec(action);
        self.stamina -= spec.stamina;
        self.recovery_wait = self.with_actor(|a| a.recovery_wait);
        self.action = action;
        self.age = 0.;
        self.sequence = self.sequence.wrapping_add(1);
        self.facing = movement.yaw;
        let wish = movement.world_direction();
        self.travel_direction = if action == Action::Dodge && wish.length_squared() > 0.01 {
            wish.normalize()
        } else {
            PlayerIntent::forward(self.facing)
        };
        if action == Action::Dodge {
            self.facing = (-self.travel_direction.x).atan2(-self.travel_direction.z);
        }
        true
    }

    /// Resolve the outgoing sweep before changing its sequence, facing or
    /// action: a Light whose contact has ended becomes its queued return
    /// cut. The view then sees the follow-up on the same tick contact
    /// finishes. Run by the plugin in [`CombatSet::Finish`]; public for a
    /// headless driver that steps an actor by hand.
    pub fn finish_contacts(&mut self, movement: &PlayerIntent) {
        if self.action == Action::Light
            && self.age + 0.000_001 >= self.spec(Action::Light).contact_end
            && self
                .queued
                .is_some_and(|(action, _)| action == Action::Light)
        {
            if self.unlocked(Action::Followup) {
                self.begin(Action::Followup, movement);
                self.queued = None;
                self.combo_wait = 0.;
            } else {
                // The press in contact asked for the return cut; said once
                // here, where the chain would have begun, not every tick
                // the queue lingers.
                self.queued = None;
                self.refuse(Refusal::Action(Action::Followup));
            }
        }
    }

    /// The numerical core can be exercised without Bevy, a window, or a GPU.
    /// Returns the contact interval and whether a shot was fired this tick.
    #[allow(
        clippy::too_many_lines,
        reason = "one ordered action transition with its resource and motion outputs"
    )]
    pub fn step(
        &mut self,
        input: &mut CombatIntent,
        movement: &PlayerIntent,
        dt: f32,
        motion: &mut CombatMotion,
    ) -> (Option<(f32, f32)>, bool) {
        *motion = CombatMotion::default();
        let actor = self.with_actor(ActorTable::clone);
        self.shot_factors = (1., 1.);
        self.shot_effects = ShotEffects::default();
        if self.action != Action::Ability {
            self.active = None;
        }
        // A press of the bar: a cut ability is the cut's own edge, to the
        // bit; any other is kept for the buffer and begun when the rule
        // allows.
        let (ability_cut, ability_press) = match input.ability.take() {
            Some(index) => self.route_press(index),
            None => (None, None),
        };
        // A draw of a weapon this actor is not certified on is refused at
        // once, as is a cut or a heavy its boxes have not opened: dropped,
        // not buffered, so a press on a locked key never fires later.
        // A draw names a slot: an empty one, or the one in hand, is dropped.
        let hand = self.hand();
        if let Some(slot) = input.weapon
            && slot != hand
            && let Some(weapon) = self.slots.get(slot).copied().flatten()
            && !self.certified(weapon)
        {
            input.weapon = None;
            self.refuse(Refusal::Draw(weapon));
        }
        if input
            .weapon
            .is_some_and(|slot| slot == hand || self.slots.get(slot).copied().flatten().is_none())
        {
            input.weapon = None;
        }
        let melee = self.weapon.is_melee();
        // With a melee weapon the trigger's press is the light cut, as the
        // light edge is; a held trigger is one cut. With a gun in hand a
        // cut is refused, once a press.
        let trigger_edge = melee && input.fire && !self.trigger;
        let light = input.light || trigger_edge || ability_cut == Some(Action::Light);
        let heavy = input.heavy || ability_cut == Some(Action::Heavy);
        if !melee && (light || heavy) {
            self.refuse(Refusal::NoBlade);
        }
        let (light, heavy) = (light && melee, heavy && melee);
        // A cut a status forbids is said once and dropped.
        let (light, heavy) = if (light || heavy) && self.disarmed() {
            self.refuse(Refusal::Ability(AbilityRefusal::Disarmed));
            (false, false)
        } else {
            (light, heavy)
        };
        // A class forgotten with its weapon drawn cuts no more, as a gun
        // the sheet does not certify fires no more.
        let (light, heavy) = if melee && (light || heavy) && !self.certified(self.weapon) {
            self.refuse(Refusal::Fire(self.weapon));
            (false, false)
        } else {
            (light, heavy)
        };
        for action in [Action::Dodge, Action::Heavy, Action::Light] {
            let pressed = match action {
                Action::Dodge => input.dodge,
                Action::Heavy => heavy,
                _ => light,
            };
            if pressed && !self.unlocked(action) {
                self.refuse(Refusal::Action(action));
            }
        }
        if !input.fire {
            self.fire_refused = false;
        }
        let requested = if input.dodge && self.unlocked(Action::Dodge) {
            Some(Action::Dodge)
        } else if heavy && self.unlocked(Action::Heavy) {
            Some(Action::Heavy)
        } else if light && self.unlocked(Action::Light) {
            Some(Action::Light)
        } else if input.reload && !melee {
            Some(Action::Reload)
        } else if self.action != Action::Swap && input.weapon.is_some() {
            Some(Action::Swap)
        } else {
            None
        };
        input.light = false;
        input.heavy = false;
        input.dodge = false;
        input.reload = false;
        if !dt.is_finite() || dt <= 0. {
            return (None, false);
        }
        // Cooldowns and statuses run whatever the actor does.
        self.cooldowns.retain(|_, left| {
            *left -= dt;
            *left > 0.
        });
        let locked = self.shield_locked();
        self.statuses.retain_mut(|(_, left)| {
            *left -= dt;
            *left > 0.
        });
        // The wait before the shield recovers starts over when the hold on
        // it ends.
        if locked && !self.shield_locked() {
            self.shield_wait = actor.hurt_wait;
        }
        if self.health <= 0. {
            self.action = Action::Dead;
        }
        if self.action == Action::Dead {
            self.statuses.clear();
            self.ability_press = None;
            motion.speed_scale = 0.;
            self.guard_up = false;
            self.queued = None;
            self.press_wait = 0.;
            self.age += dt;
            return (None, false);
        }
        let row = self.weapon_spec();
        // A press the weapon could not take is kept for the buffer; the
        // press is the edge, so a held trigger never renews it.
        self.press_wait = (self.press_wait - dt).max(0.);
        if input.fire && !self.trigger && !melee {
            self.press_wait = actor.buffer;
        }
        if !input.fire {
            self.unbroken = false;
        }
        // The cone closes in any action, after its wait; the wait is spent
        // first so the settling is exact at every tick rate.
        let waited = self.bloom_wait.min(dt);
        self.bloom_wait -= waited;
        self.bloom = (self.bloom - row.bloom.settle * (dt - waited)).max(0.);
        self.move_share = if movement.move_dir.is_finite() {
            movement.move_dir.length().min(1.)
        } else {
            0.
        };
        // Letting go of a hold starts the burst's pause from the last round,
        // not from the hold's shorter gap.
        if self.holding && !input.fire {
            self.holding = false;
            if let Some(hold) = row.hold {
                self.shot_wait += (row.interval - hold.gap).max(0.);
            }
        }
        // An armed ability keeps the trigger as Ready has it.
        let live = self.action == Action::Ready || self.armed();
        // Leaving Ready or running dry ends a burst or a hold; letting go
        // of the trigger does not end a burst.
        if !live || self.ammo == 0 {
            self.burst_left = 0;
            self.holding = false;
        }
        // Carry sub-tick overshoot so 120 ms does not round up to 133 ms at 60 Hz.
        self.shot_wait -= dt;
        if !(input.fire || self.burst_left > 0 || self.press_wait > 0.) || !live {
            self.shot_wait = self.shot_wait.max(0.);
        }
        self.recovery_wait = (self.recovery_wait - dt).max(0.);
        self.hurt_wait = (self.hurt_wait - dt).max(0.);
        self.shield_wait = (self.shield_wait - dt).max(0.);
        self.guard_met += dt;
        self.combo_wait = (self.combo_wait - dt).max(0.);
        if self.action == Action::Ready && self.recovery_wait == 0. {
            let kept = if self.guard_up {
                combat_tables::with(|tables| tables.melee.guard_regen)
            } else {
                1.
            };
            self.stamina = (self.stamina + actor.stamina_regen * kept * dt).min(actor.stamina);
        }
        if self.hurt_wait == 0. && self.shield_wait == 0. && !self.shield_locked() {
            self.shield = (self.shield + actor.shield_regen * dt).min(self.shield_capacity);
        }
        if self.hurt_wait == 0. {
            self.posture = (self.posture + actor.posture_regen * dt).min(actor.posture);
        }
        if let Some((_, ttl)) = &mut self.queued {
            *ttl -= dt;
        }
        if self.queued.is_some_and(|(_, ttl)| ttl < 0.) {
            self.queued = None;
        }
        if let Some(action) = requested {
            self.queued = Some((action, actor.buffer));
        }
        if self.queued.is_none() {
            self.pending_cut = None;
        }
        if let Some((_, ttl)) = &mut self.ability_press {
            *ttl -= dt;
        }
        if self.ability_press.is_some_and(|(_, ttl)| ttl < 0.) {
            self.ability_press = None;
        }
        if let Some(index) = ability_press {
            self.ability_press = Some((index, actor.buffer));
        }
        if self.action != Action::Ready && self.age + dt * 0.0001 >= self.duration() {
            if self.action == Action::Reload {
                self.ammo = self.magazine();
            }
            if self.action == Action::Swap {
                input.weapon = None;
                if self
                    .queued
                    .is_some_and(|(action, _)| action == Action::Swap)
                {
                    self.queued = None;
                }
                if let Some(drawn) = self.holstered() {
                    self.weapon = drawn;
                    std::mem::swap(&mut self.ammo, &mut self.holstered_ammo);
                }
                self.bolt_speed = self.weapon_spec().bolt_speed;
                // The cone the other gun grew is not this gun's to carry.
                self.bloom = 0.;
                self.bloom_wait = 0.;
                self.aim_share = 0.;
            }
            if self.action == Action::Light {
                self.combo_wait = actor.combo_window;
            }
            if self.action == Action::Ability {
                self.active = None;
            }
            self.action = Action::Ready;
            self.age = 0.;
        }
        if let Some((mut action, _)) = self.queued {
            let can_start = self.action == Action::Ready
                || (action == Action::Dodge && self.age >= self.dodge_cancel());
            if can_start {
                if action == Action::Light
                    && self.combo_wait > 0.
                    && self.unlocked(Action::Followup)
                {
                    action = Action::Followup;
                }
                if action.is_melee() && !self.weapon.is_melee() {
                    self.refuse(Refusal::NoBlade);
                } else if (action != Action::Reload || self.ammo < self.magazine())
                    && self.begin(action, movement)
                    && let Some((_, id, cooldown)) = self.pending_cut.take_if(|p| p.0 == action)
                {
                    self.cooldowns.insert(id, cooldown);
                }
                self.queued = None;
                self.pending_cut = None;
            }
        }
        if let Some((index, _)) = self.ability_press {
            match self.can_use(index) {
                Ok(row) if row.is_instant() => {
                    self.ability_press = None;
                    self.use_instant(&row);
                }
                Ok(row) => {
                    self.ability_press = None;
                    self.begin_ability(row, movement);
                }
                Err(AbilityRefusal::Busy) => {}
                Err(AbilityRefusal::Empty) => self.ability_press = None,
                Err(why) => {
                    self.ability_press = None;
                    self.refuse(Refusal::Ability(why));
                }
            }
        }
        self.aiming = input.aim && !melee;
        self.move_aim(dt);
        let pressed = input.aim && melee;
        self.keep_guard(pressed && self.action == Action::Ready, pressed, dt);
        let mut shot = false;
        if self.action == Action::Ready {
            self.facing = if input.aim
                || input.fire
                || self.burst_left > 0
                || movement.move_dir.length_squared() < 0.01
            {
                movement.yaw
            } else {
                let direction = movement.world_direction();
                (-direction.x).atan2(-direction.z)
            };
            motion.speed_scale = if input.aim {
                self.aim_speed(&actor)
            } else {
                1.
            };
            shot = self.pull_trigger(input.fire, melee);
        } else if self.action == Action::Reload || self.action == Action::Swap {
            motion.speed_scale = self.aim_speed(&actor);
        } else if self.action == Action::Ability {
            motion.speed_scale = self.active.as_ref().map_or(0., |active| active.row.pace);
            shot = self.armed() && self.pull_trigger(input.fire, melee);
        } else {
            motion.speed_scale = 0.;
        }
        // Slowed is read where an ability's own pace is: the share of moving
        // speed, never the travel of a cut or a dodge.
        motion.speed_scale *= self.pace_share();
        self.trigger = input.fire;
        let spec = self.spec(self.action);
        let previous = self.age;
        self.age += dt;
        if self.action == Action::Dodge || self.action.is_melee() {
            let duration = if self.action == Action::Dodge {
                spec.duration
            } else {
                spec.contact_end
            };
            let distance = displacement(self.age, duration, spec.travel)
                - displacement(previous, duration, spec.travel);
            motion.velocity = Some(self.travel_direction * (distance / dt));
        }
        let contact = if self.action.is_melee() {
            contact_interval(previous, self.age, spec.contact_start, spec.contact_end)
        } else {
            None
        };
        let ability_shot = self.run_ability(movement);
        if self.action == Action::Ability
            && let Some(active) = &self.active
            && let Some(direction) = active.move_dir
            && let AbilityEffect::Move {
                distance, seconds, ..
            } = active.row.effect
        {
            let windup = active.row.windup;
            let carried = displacement(self.age - windup, seconds, distance)
                - displacement(previous - windup, seconds, distance);
            motion.velocity = Some(direction * (carried / dt));
        }
        (contact, shot || ability_shot)
    }

    /// Whether the ability under way keeps the trigger live.
    fn armed(&self) -> bool {
        self.action == Action::Ability
            && self.active.as_ref().is_some_and(|active| active.row.armed)
    }

    /// The trigger's turn this tick, for a gun: a round of the scheduler
    /// leaves, or a refusal is said once. `true` when a bolt leaves.
    fn pull_trigger(&mut self, fire: bool, melee: bool) -> bool {
        if melee {
            // The trigger was the cut; there is no round to spend.
            return false;
        }
        if self.disarmed() {
            self.press_wait = 0.;
            self.burst_left = 0;
            self.holding = false;
            if fire && !self.fire_refused {
                self.fire_refused = true;
                self.refuse(Refusal::Ability(AbilityRefusal::Disarmed));
            }
            return false;
        }
        if fire && !self.certified(self.weapon) {
            if !self.fire_refused {
                self.fire_refused = true;
                self.refuse(Refusal::Fire(self.weapon));
            }
            return false;
        }
        if let Some(round) = self.next_round(fire) {
            self.fire_round(round, fire);
            return true;
        }
        false
    }

    /// What a press of `index` comes to at the moment it is made. A cut
    /// ability is asked the rule's first refusals only (the weapon, the
    /// cooldown): its stamina and its readiness are the action's own, which
    /// the cut's edge meets as it always did, so it returns the action to be
    /// pressed. Any other is returned to be kept for the buffer; a refusal is
    /// said once, here, and the press dropped.
    fn route_press(&mut self, index: usize) -> (Option<Action>, Option<usize>) {
        let Some(row) = self.ability_at(index) else {
            return (None, None);
        };
        let AbilityEffect::Cut(action) = row.effect else {
            return (None, Some(index));
        };
        match can_use(&row, &self.use_state(&row)) {
            Ok(()) | Err(AbilityRefusal::Busy | AbilityRefusal::Stamina(_)) => {
                let action = if action == Action::Followup {
                    Action::Light
                } else {
                    action
                };
                self.pending_cut = (row.cooldown > 0.).then_some((action, row.id, row.cooldown));
                (Some(action), None)
            }
            Err(why) => {
                self.refuse(Refusal::Ability(why));
                (None, None)
            }
        }
    }

    /// Pay for an instant ability, start its cooldown and do its work now.
    /// The action, the guard and the aim are left as they were.
    fn use_instant(&mut self, row: &AbilityRow) {
        let recovery_wait = self.with_actor(|a| a.recovery_wait);
        self.stamina -= row.stamina;
        if row.cooldown > 0. {
            self.cooldowns.insert(row.id.clone(), row.cooldown);
        }
        self.recovery_wait = recovery_wait;
        if let AbilityEffect::Own(own) = &row.effect {
            self.apply_own(own);
        }
    }

    /// An effect on the user, at the windup's end or at once.
    fn apply_own(&mut self, own: &Own) {
        match own {
            Own::Reload => self.ammo = self.magazine(),
            Own::Shield => {
                self.hurt_wait = 0.;
                self.shield_wait = 0.;
            }
            Own::Status { id, seconds } => self.give_status(id, *seconds),
        }
    }

    /// Pay for the ability, start its cooldown and commit the actor to it.
    fn begin_ability(&mut self, row: AbilityRow, movement: &PlayerIntent) {
        let recovery_wait = self.with_actor(|a| a.recovery_wait);
        self.stamina -= row.stamina;
        if row.cooldown > 0. {
            self.cooldowns.insert(row.id.clone(), row.cooldown);
        }
        self.recovery_wait = recovery_wait;
        self.action = Action::Ability;
        self.age = 0.;
        self.sequence = self.sequence.wrapping_add(1);
        self.facing = movement.yaw;
        self.travel_direction = PlayerIntent::forward(self.facing);
        self.burst_left = 0;
        self.holding = false;
        self.active = Some(Active {
            row,
            fired: false,
            owed: 0,
            next: 0.,
            move_dir: None,
            then_due: None,
        });
    }

    /// A strike's figures as the physics resolves them: a scaled one reads
    /// its reach as a factor on the weapon in hand's and its blow as
    /// factors on the class's light cut as this actor reads it (sheet, class
    /// and fitted parts).
    fn strike_blow(
        &self,
        figures: (f32, f32, f32, f32),
        scaled: bool,
        on_hit: Option<OnHit>,
    ) -> StrikeBlow {
        let (reach, arc, damage, posture) = figures;
        if !scaled {
            return StrikeBlow {
                reach,
                arc,
                damage,
                posture,
                on_hit,
            };
        }
        let light = self.spec(Action::Light);
        StrikeBlow {
            reach: reach * combat_tables::with(|tables| tables.melee.reach_m(self.weapon)),
            arc,
            damage: damage * light.damage,
            posture: posture * light.posture,
            on_hit,
        }
    }

    /// Land the effect of the ability under way when its windup has passed,
    /// and let a shot's owed rounds leave on their gaps. `true` when a bolt
    /// leaves this tick. An ability that ended before its windup (a stagger,
    /// a death, a dodge) is no longer here and gives nothing.
    fn run_ability(&mut self, movement: &PlayerIntent) -> bool {
        if self.action != Action::Ability {
            return false;
        }
        let Some(mut active) = self.active.take() else {
            return false;
        };
        let shot = self.run_active(&mut active, movement);
        self.active = Some(active);
        shot
    }

    /// The ability `active` at this tick: see [`Self::run_ability`].
    #[allow(
        clippy::too_many_lines,
        reason = "the windup's effect, the movement's strike and the owed rounds in tick order"
    )]
    fn run_active(&mut self, active: &mut Active, movement: &PlayerIntent) -> bool {
        // Float sums of ticks meet a windup a hair short, as a cut's do.
        const EPSILON: f32 = 0.000_001;
        let (age, row) = (self.age, active.row.clone());
        let hit_arc = combat_tables::with(|tables| tables.abilities.hit_arc);
        if !active.fired && age + EPSILON >= row.windup {
            active.fired = true;
            match row.effect {
                AbilityEffect::Shot { rounds, .. } => {
                    active.owed = rounds;
                    active.next = row.windup;
                }
                AbilityEffect::Hit {
                    damage,
                    posture,
                    ref status,
                    seconds,
                } => {
                    let on_hit = Some(OnHit {
                        status: status.clone(),
                        seconds,
                    });
                    if self.weapon.is_melee() {
                        self.strike =
                            Some(self.strike_blow((1., hit_arc, damage, posture), true, on_hit));
                    } else {
                        active.owed = 1;
                        active.next = row.windup;
                    }
                }
                AbilityEffect::Strike {
                    reach,
                    arc,
                    damage,
                    posture,
                    scaled,
                    ref on_hit,
                } => {
                    self.strike = Some(self.strike_blow(
                        (reach, arc, damage, posture),
                        scaled,
                        on_hit.clone(),
                    ));
                }
                AbilityEffect::Move {
                    seconds,
                    along,
                    then,
                    ..
                } => {
                    let wish = movement.world_direction();
                    if along == Along::Input && wish.length_squared() > 0.01 {
                        self.travel_direction = wish.normalize();
                    }
                    active.move_dir = Some(self.travel_direction);
                    active.then_due = then.map(|_| row.windup + seconds);
                }
                AbilityEffect::Own(ref own) => self.apply_own(own),
                AbilityEffect::Cut(_) => {}
            }
        }
        if let Some(due) = active.then_due
            && age + EPSILON >= due
            && let AbilityEffect::Move {
                then: Some(then), ..
            } = row.effect
        {
            active.then_due = None;
            self.strike = Some(self.strike_blow(
                (then.reach, then.arc, then.damage, then.posture),
                then.scaled,
                None,
            ));
        }
        let Some((gap, cone, damage, posture, bloom, effects)) = self.shot_plan(&row.effect) else {
            return false;
        };
        if self.ammo == 0 || self.disarmed() {
            active.owed = 0;
        }
        if active.owed == 0 || age + EPSILON < active.next {
            return false;
        }
        active.owed -= 1;
        active.next += gap;
        let weapon = self.weapon_spec();
        self.shot_cone = match cone {
            ShotCone::Exact => 0.,
            ShotCone::Aimed if bloom => {
                weapon.aim_spread + self.bloom + weapon.move_spread * self.move_share
            }
            ShotCone::Aimed => weapon.aim_spread,
            ShotCone::Hip if bloom => {
                weapon.spread + self.bloom + weapon.move_spread * self.move_share
            }
            ShotCone::Hip => weapon.spread,
        };
        if bloom {
            self.bloom = (self.bloom + weapon.bloom.shot).min(weapon.bloom.cap);
            self.bloom_wait = weapon.bloom.wait;
        }
        self.shot_factors = (damage, posture);
        self.shot_effects = effects;
        self.ammo -= 1;
        self.shots = self.shots.wrapping_add(1);
        true
    }

    /// The bolts an effect fires, if it is one that fires: the gap between
    /// them, the cone, the factors, whether it reads bloom and what each
    /// carries. A hit made with a gun is one aimed bolt carrying a status.
    fn shot_plan(
        &self,
        effect: &AbilityEffect,
    ) -> Option<(f32, ShotCone, f32, f32, bool, ShotEffects)> {
        match effect {
            AbilityEffect::Shot {
                gap,
                cone,
                damage,
                posture,
                bloom,
                on_hit,
                jump,
                blast,
                breach,
                ..
            } => Some((
                *gap,
                *cone,
                *damage,
                *posture,
                *bloom,
                ShotEffects {
                    on_hit: on_hit.clone(),
                    jump: *jump,
                    blast: *blast,
                    breach: *breach,
                    reflected: false,
                },
            )),
            AbilityEffect::Hit {
                damage,
                posture,
                status,
                seconds,
            } if !self.weapon.is_melee() => Some((
                0.,
                ShotCone::Aimed,
                *damage,
                *posture,
                false,
                ShotEffects {
                    on_hit: Some(OnHit {
                        status: status.clone(),
                        seconds: *seconds,
                    }),
                    ..ShotEffects::default()
                },
            )),
            _ => None,
        }
    }

    /// Raise, hold or drop the guard for this tick and keep its clocks.
    ///
    /// `pressed` is the button itself, so the window opens only on a raise
    /// the player made and a held button cannot farm `parry_rest`.
    fn keep_guard(&mut self, wanted: bool, pressed: bool, dt: f32) {
        if wanted {
            if self.guard_up {
                self.guard_for += dt;
            } else {
                let rest = self
                    .parry_rest
                    .unwrap_or_else(|| combat_tables::with(|tables| tables.melee.parry_rest));
                self.guard_up = true;
                self.guard_rested = !self.guard_held && self.guard_off >= rest;
                self.guard_for = 0.;
            }
            self.guard_off = 0.;
        } else {
            self.guard_up = false;
            self.guard_for = 0.;
            self.guard_off += dt;
        }
        self.guard_held = pressed;
    }

    /// The guard as a hit finds it, for the pure outcome function.
    #[must_use]
    pub fn guard_state(&self) -> GuardState {
        GuardState {
            up: self.guard_up && self.weapon.is_melee(),
            up_for: self.guard_for,
            rested: self.guard_rested,
        }
    }

    /// Which round the scheduler lets leave now, if any: the trigger's
    /// press edge for a single shot or a burst's opening, the burst's
    /// owed rounds, a hold's, or an automatic weapon's held trigger.
    fn next_round(&self, fire: bool) -> Option<Round> {
        if self.ammo == 0 || self.shot_wait > 0. {
            return None;
        }
        let pressed = fire && !self.trigger || self.press_wait > 0.;
        match self.mode() {
            FireMode::Auto => fire.then_some(Round::Plain),
            FireMode::Single => pressed.then_some(Round::Plain),
            FireMode::Burst { .. } => {
                if self.burst_left > 0 {
                    Some(Round::Burst)
                } else if self.holding {
                    fire.then_some(Round::Hold)
                } else {
                    pressed.then_some(Round::Open)
                }
            }
        }
    }

    /// Spend a round: ammunition, the shot count, the cone it left in, the
    /// bloom it adds and the wait until the next one.
    fn fire_round(&mut self, round: Round, fire: bool) {
        let row = self.weapon_spec();
        self.shot_cone = self.cone();
        self.ammo -= 1;
        self.shots = self.shots.wrapping_add(1);
        let mut added = row.bloom.shot;
        let (count, gap) = match self.mode() {
            FireMode::Burst { count, gap } => (count, gap),
            FireMode::Single | FireMode::Auto => (1, 0.),
        };
        if matches!(round, Round::Open | Round::Plain) {
            self.press_wait = 0.;
        }
        if round == Round::Open {
            self.unbroken = true;
        }
        match round {
            Round::Plain => self.shot_wait += row.interval,
            Round::Open | Round::Burst => {
                self.burst_left = if round == Round::Open {
                    count.saturating_sub(1)
                } else {
                    self.burst_left - 1
                };
                if self.burst_left > 0 {
                    self.shot_wait += gap;
                } else if let (true, Some(hold)) = (fire && self.unbroken, row.hold) {
                    self.holding = true;
                    self.shot_wait += hold.gap;
                } else {
                    self.shot_wait += row.interval;
                }
            }
            Round::Hold => {
                if let Some(hold) = row.hold {
                    added += hold.bloom;
                    self.shot_wait += hold.gap;
                } else {
                    self.shot_wait += row.interval;
                }
            }
        }
        self.bloom = (self.bloom + added).min(row.bloom.cap);
        self.bloom_wait = row.bloom.wait;
    }

    /// How far into the current action a dodge may cut it short. A draw
    /// whose length something fitted changes keeps its window at the same
    /// share of itself: the tables give a draw a window as long as it is,
    /// which is never, and a slower sight must not open one at its end
    /// that no bare actor has.
    fn dodge_cancel(&self) -> f32 {
        let spec = self.spec(self.action);
        if self.action == Action::Swap
            && spec.duration > 0.
            && let Some(row) = self.holstered().and_then(|weapon| self.fitted_row(weapon))
        {
            spec.dodge_cancel * (row.draw / spec.duration)
        } else {
            spec.dodge_cancel
        }
    }

    /// The current action's length; a reload's depends on the weapon, and
    /// a swap's on the weapon being drawn when something fitted to it
    /// changes its draw.
    #[must_use]
    pub fn duration(&self) -> f32 {
        if self.action == Action::Reload {
            self.reload_time()
        } else if self.action == Action::Swap
            && let Some(row) = self.holstered().and_then(|weapon| self.fitted_row(weapon))
        {
            row.draw
        } else if self.action == Action::Stagger {
            self.spec(self.action).duration * self.stagger_scale
        } else {
            self.spec(self.action).duration
        }
    }

    /// How this actor's guard, or its cover, meets `blow`.
    fn guard_against(&self, blow: &Blow, class: Option<MeleeClass>, covered: bool) -> GuardOutcome {
        if covered {
            return GuardOutcome::Blocked;
        }
        class.map_or(GuardOutcome::Open, |class| {
            guard_outcome(
                &class.guard,
                self.guard_state(),
                self.facing,
                blow,
                self.stamina,
            )
        })
    }

    /// Whether `blow` is a cut made with a melee weapon on a bearer whose
    /// hand holds a gun.
    fn is_caught_by(&self, blow: &Blow) -> bool {
        blow.from_melee && blow.kind == BlowKind::Cut && !self.weapon.is_melee()
    }

    /// The share of a bolt's damage a closing move's cover takes, if one is
    /// under way and the bolt comes from within its arc of the direction
    /// of the movement: from the windup's end until the movement ends.
    fn cover_share(&self, blow: &Blow) -> Option<f32> {
        if blow.kind != BlowKind::Bolt || self.action != Action::Ability {
            return None;
        }
        let active = self.active.as_ref()?;
        let AbilityEffect::Move {
            seconds,
            cover: Some(cover),
            ..
        } = active.row.effect
        else {
            return None;
        };
        let direction = active.move_dir?;
        if self.age >= active.row.windup + seconds {
            return None;
        }
        let toward = Vec3::new(blow.from.x, 0., blow.from.z).try_normalize()?;
        let ahead = Vec3::new(direction.x, 0., direction.z).try_normalize()?;
        (ahead.dot(toward) >= cover.arc.to_radians().cos()).then_some(cover.share)
    }

    /// Only a spatially confirmed hit may spend health or posture. This
    /// one knows nothing of where it came from, so no guard meets it.
    pub fn damage(&mut self, damage: f32, posture: f32) -> bool {
        self.hit(Blow::bare(damage, posture)).is_some()
    }

    /// [`Combatant::damage`], telling what it took: the pools it removed,
    /// whether it killed (which the ledger credits) and how the guard met
    /// it. `None` when the hit did not land (dead already, or inside a
    /// dodge, whose safe window comes before the guard).
    pub fn hit(&mut self, blow: Blow) -> Option<Dealt> {
        self.take(blow, true)
    }

    /// A parried cut's riposte on its swinger: the guard row's own posture
    /// figure, which the swinger's `swing_posture` does not scale (that share
    /// is a class's bracing against a blow met mid cut, and a riposte is a
    /// price of the parry, so the same parry costs a greatblade and a blade
    /// alike).
    pub fn riposted(&mut self, posture: f32) -> Option<Dealt> {
        self.take(Blow::bare(0., posture), false)
    }

    fn take(&mut self, blow: Blow, braces: bool) -> Option<Dealt> {
        if self.action == Action::Dead || self.invulnerable() {
            return None;
        }
        let class = self.melee_class();
        // A closing move's cover meets a bolt from ahead as a block would,
        // at no cost: the guard does not come into it.
        let cover = self.cover_share(&blow);
        let guard = self.guard_against(&blow, class, cover.is_some());
        if guard == GuardOutcome::Parried {
            let riposte = match (class, blow.kind) {
                (Some(class), BlowKind::Cut) => class.guard.riposte,
                _ => 0.,
            };
            return Some(Dealt {
                pools: 0.,
                lethal: false,
                guard,
                riposte,
            });
        }
        let (hurt_wait, posture_full) = self.with_actor(|a| (a.hurt_wait, a.posture));
        self.hurts = self.hurts.wrapping_add(1);
        self.hurt_wait = hurt_wait;
        self.shield_wait = hurt_wait;
        let (mut damage, mut posture) = (blow.damage.max(0.), blow.posture);
        if let Some(share) = cover {
            damage -= damage * share;
            posture = 0.;
        } else if let (Some(class), GuardOutcome::Blocked | GuardOutcome::Broken) = (class, guard) {
            let (share, price, keeps) = class.guard.figures(blow.kind == BlowKind::Bolt);
            let cost = damage * share * price;
            if blow.kind == BlowKind::Cut {
                self.guard_met = 0.;
            }
            damage -= damage * share;
            posture *= keeps;
            self.stamina = if guard == GuardOutcome::Broken {
                0.
            } else {
                (self.stamina - cost).max(0.)
            };
        }
        // A gun is no guard: a cut of a melee weapon that nothing met, on
        // a bearer whose hand holds a gun, breaks it sooner.
        let mut stagger_scale = 1.;
        if guard == GuardOutcome::Open && self.is_caught_by(&blow) {
            let caught = combat_tables::with(|tables| tables.melee.caught);
            posture *= caught.posture;
            damage *= caught.damage;
            stagger_scale = caught.stagger;
        }
        // A cut under way braces the holder: a class that does not flinch
        // mid swing takes a share of the posture damage.
        let posture = match class {
            Some(class) if braces && self.action.is_melee() => posture * class.swing_posture,
            _ => posture,
        };
        // A status borne may stop posture damage altogether.
        let posture = if self.statuses.is_empty() {
            posture
        } else {
            posture * self.posture_taken()
        };
        // A status borne may take a share of the damage, after the guard.
        if !self.statuses.is_empty() {
            damage *= self.damage_taken();
        }
        let before = self.health + self.shield;
        // A breaching blow empties the shield first, so all of it is health.
        if blow.breach {
            self.shield = 0.;
        }
        let absorbed = self.shield.min(damage);
        self.shield -= absorbed;
        self.health = (self.health - (damage - absorbed)).max(0.);
        self.posture -= posture.max(0.);
        let mut dealt = Dealt {
            pools: before - (self.health + self.shield),
            lethal: false,
            guard,
            riposte: 0.,
        };
        // A hit the guard did not meet lands its status; one that killed
        // ends them all.
        if guard == GuardOutcome::Open
            && self.health > 0.
            && let Some(on_hit) = &blow.on_hit
        {
            self.give_status(&on_hit.status, on_hit.seconds);
        }
        if self.health == 0. {
            self.action = Action::Dead;
            self.age = 0.;
            self.queued = None;
            self.ability_press = None;
            self.guard_up = false;
            self.statuses.clear();
            dealt.lethal = true;
        } else if self.posture <= 0. || guard == GuardOutcome::Broken {
            self.action = Action::Stagger;
            self.age = 0.;
            self.stagger_scale = stagger_scale;
            self.posture = posture_full;
            self.sequence = self.sequence.wrapping_add(1);
            self.queued = None;
            self.ability_press = None;
            self.guard_up = false;
            self.aim_share = 0.;
        }
        Some(dealt)
    }
}

/// What a confirmed hit took off its victim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dealt {
    /// Health and shield removed.
    pub pools: f32,
    /// The victim died of it.
    pub lethal: bool,
    /// How the victim's guard met the hit.
    pub guard: GuardOutcome,
    /// Posture damage the attacker owes for a parried cut; the adapter
    /// lands it on them.
    pub riposte: f32,
}

/// What kind of blow a hit is, which a parry treats differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlowKind {
    /// A swing of a blade; a parried one staggers its attacker.
    Cut,
    /// A bolt; a parried one is turned aside.
    Bolt,
}

/// A hit as it reaches its victim: what it carries, where it came from and
/// whether it is a cut or a bolt.
#[derive(Debug, Clone, PartialEq)]
pub struct Blow {
    /// Damage before any guard.
    pub damage: f32,
    /// Posture damage before any guard.
    pub posture: f32,
    /// The horizontal direction from the victim toward the attacker, not
    /// necessarily unit; nought when it is not known, which no guard meets.
    pub from: Vec3,
    /// Cut or bolt.
    pub kind: BlowKind,
    /// A status put on the victim if no guard meets the blow.
    pub on_hit: Option<OnHit>,
    /// The blow empties what is left of the victim's shield first.
    pub breach: bool,
    /// A cut made with a melee weapon in hand, which a bearer holding a gun
    /// is caught by; a gun's butt strike is a cut that is not.
    pub from_melee: bool,
}

impl Blow {
    /// A blow with no direction, which no guard meets: a scripted hurt, a
    /// test, a riposte.
    #[must_use]
    pub fn bare(damage: f32, posture: f32) -> Self {
        Self {
            damage,
            posture,
            from: Vec3::ZERO,
            kind: BlowKind::Cut,
            on_hit: None,
            breach: false,
            from_melee: false,
        }
    }

    /// A blow of `kind` coming from the side `from` points to.
    #[must_use]
    pub fn new(damage: f32, posture: f32, from: Vec3, kind: BlowKind) -> Self {
        Self {
            damage,
            posture,
            from,
            kind,
            on_hit: None,
            breach: false,
            from_melee: false,
        }
    }

    /// A blast's blow: it comes off the ground beside the target and not down
    /// the line of fire, so it has no direction and no guard meets it.
    #[must_use]
    pub fn blast(damage: f32, posture: f32) -> Self {
        Self::new(damage, posture, Vec3::ZERO, BlowKind::Bolt)
    }

    /// This blow, putting `on_hit` on a victim no guard meets.
    #[must_use]
    pub fn with_status(mut self, on_hit: Option<OnHit>) -> Self {
        self.on_hit = on_hit;
        self
    }

    /// This blow, made with a melee weapon in hand when `melee`.
    #[must_use]
    pub const fn made_with_melee(mut self, melee: bool) -> Self {
        self.from_melee = melee;
        self
    }

    /// This blow, emptying the victim's shield first when `breach`.
    #[must_use]
    pub const fn breaching(mut self, breach: bool) -> Self {
        self.breach = breach;
        self
    }
}

/// How a guard met a blow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GuardOutcome {
    /// Not at all: the guard was down or the blow came from outside its arc.
    #[default]
    Open,
    /// Part of the blow was taken off, at a price in stamina.
    Blocked,
    /// Raised just in time: nothing landed.
    Parried,
    /// The block asked more stamina than was left.
    Broken,
}

/// The guard of a combatant at the moment a blow arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuardState {
    /// The guard is up.
    pub up: bool,
    /// Seconds since it was raised.
    pub up_for: f32,
    /// It had been down for `parry_rest` before it was raised.
    pub rested: bool,
}

/// Which of the four outcomes `blow` has against a guard of class row `row`
/// in `state`, for a holder facing `facing` with `stamina` left. Pure, so
/// the tests, the harness and a screen ask the one rule.
#[must_use]
pub fn guard_outcome(
    row: &GuardRow,
    state: GuardState,
    facing: f32,
    blow: &Blow,
    stamina: f32,
) -> GuardOutcome {
    if !state.up {
        return GuardOutcome::Open;
    }
    let toward = Vec3::new(blow.from.x, 0., blow.from.z);
    let Some(toward) = toward.try_normalize() else {
        return GuardOutcome::Open;
    };
    if PlayerIntent::forward(facing).dot(toward) < row.arc.to_radians().cos() {
        return GuardOutcome::Open;
    }
    if state.rested && row.parry > 0. && state.up_for <= row.parry {
        return GuardOutcome::Parried;
    }
    let (share, cost, _) = row.figures(blow.kind == BlowKind::Bolt);
    if blow.damage.max(0.) * share * cost > stamina {
        GuardOutcome::Broken
    } else {
        GuardOutcome::Blocked
    }
}

/// Integrated ease-out travel gives exactly the same range at every step size.
#[must_use]
pub fn displacement(t: f32, duration: f32, distance: f32) -> f32 {
    let u = (t / duration).clamp(0., 1.);
    distance * (1. - (1. - u).powi(3))
}

/// Intersect intervals instead of checking a single pose, including slow frames.
#[must_use]
pub fn contact_interval(a: f32, b: f32, start: f32, end: f32) -> Option<(f32, f32)> {
    let from = a.max(start);
    let to = b.min(end);
    (to > from).then_some((from, to))
}

/// Physics resolves intent; the sim never imports its spatial-query backend.
#[derive(Message, Debug, Clone)]
pub struct AttackRequest {
    /// Entity initiating contact.
    pub attacker: Entity,
    /// Identity of the committed action.
    pub sequence: u64,
    /// Attack geometry to resolve this tick.
    pub kind: AttackKind,
}

/// Contact time is carried to the physics adapter explicitly.
#[derive(Debug, Clone)]
pub enum AttackKind {
    /// Fire toward a world-space crosshair point from the gameplay muzzle.
    Shot(WorldPos),
    /// Sweep the blade over this part of an action's contact interval.
    Melee {
        /// Selects the authored sweep and damage profile.
        action: Action,
        /// Start of this tick's clipped contact interval.
        from: f32,
        /// End of this tick's clipped contact interval.
        to: f32,
    },
    /// An ability's blow on everything hostile within `reach` metres and
    /// `arc` degrees either side of the facing, once.
    Strike {
        /// Metres from the attacker.
        reach: f32,
        /// Degrees either side of the facing.
        arc: f32,
        /// Health damage before the attacker's scale.
        damage: f32,
        /// Posture damage before the attacker's scale.
        posture: f32,
        /// A status the blow puts on what it lands on unguarded.
        on_hit: Option<OnHit>,
    },
}

/// Local encounter objective, optional in worlds that provide their own AI.
#[derive(Resource, Debug, Default)]
pub struct CombatEncounter {
    /// Placement waits for streamed collision and visible staging positions.
    pub ready: bool,
    /// Aggression begins after approaching or damaging a hostile.
    pub engaged: bool,
    /// Living hostiles, excluding the passive practice target.
    pub remaining: usize,
}

/// Launch feedback happens immediately; damage waits for the bolt to arrive.
#[derive(Message, Debug, Clone, Copy)]
pub struct ShotFired {
    /// Entity releasing the bolt.
    pub attacker: Entity,
    /// Gameplay muzzle after checking nearby cover.
    pub from: WorldPos,
    /// Locked direction, never updated to follow a target.
    pub direction: Vec3,
}

/// Authoritative projectile data stays in world coordinates across rebases.
#[derive(Component, Debug, Clone)]
pub struct BlasterBolt {
    /// Friendly fire and the launcher's collider are excluded from damage.
    pub owner: Entity,
    /// Team at launch, even if the owner despawns.
    pub team: u8,
    /// Previous tick's position for visual interpolation.
    pub previous: WorldPos,
    /// Current tip position.
    pub position: WorldPos,
    /// Constant velocity; bolts have neither gravity nor homing.
    pub velocity: Vec3,
    /// Damage at contact.
    pub damage: f32,
    /// Posture pressure at contact.
    pub posture: f32,
    /// Remaining range bounds lifetime, including shots into the sky.
    pub remaining: f32,
    /// Metres flown since the muzzle, which damage fades by.
    pub flown: f32,
    /// How the firing weapon's damage fades with `flown`.
    pub falloff: Falloff,
    /// The weapon that fired it, for the ledger of what it hit.
    pub weapon: Weapon,
    /// What an ability's shot gave it beyond its damage.
    pub effects: ShotEffects,
}

/// The spatial adapter reports both hits and impacts on cover for presentation.
#[derive(Message, Debug, Clone, Copy)]
pub struct CombatImpact {
    /// Who fired or swung.
    pub attacker: Entity,
    /// Damaged actor, absent when the strike hit scenery or missed.
    pub victim: Option<Entity>,
    /// Start of the trace, for muzzle flashes and tracers.
    pub from: WorldPos,
    /// End of the trace, preserved across origin rebases.
    pub point: WorldPos,
    /// Melee feedback has a heavier envelope than a bullet.
    pub melee: bool,
    /// Health and shield the hit took off the victim; zero on scenery.
    pub dealt: f32,
    /// The hit killed the victim.
    pub lethal: bool,
    /// How the victim's guard met it; [`GuardOutcome::Open`] on scenery.
    pub guard: GuardOutcome,
    /// The weapon and the metres its bolt flew, for a bolt that struck;
    /// `None` for a cut.
    pub shot: Option<(Weapon, f32)>,
}

/// Physics orders its motor after this set and contacts after the motor.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombatSet {
    /// Accept inputs and advance the action clock.
    Decide,
    /// A Light whose sweep finished becomes its queued return cut, in
    /// `SimSet::Observe`; a recorder that reads actions orders after it.
    Finish,
}

/// Installed even headless; an empty encounter does no work.
#[derive(Debug, Clone, Copy)]
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AttackRequest>()
            .add_message::<CombatImpact>()
            .add_message::<ShotFired>()
            .add_systems(SimTick, tick.in_set(CombatSet::Decide).in_set(SimSet::Step))
            .add_systems(
                SimTick,
                finish_contacts
                    .in_set(CombatSet::Finish)
                    .in_set(SimSet::Observe),
            );
    }
}

fn finish_contacts(mut actors: Query<(&mut Combatant, &PlayerCommand)>) {
    for (mut actor, movement) in &mut actors {
        actor.finish_contacts(&movement.0);
    }
}

fn tick(
    time: Res<Time<Fixed>>,
    mut attacks: MessageWriter<AttackRequest>,
    mut actors: Query<(
        Entity,
        &mut Combatant,
        &mut CombatIntent,
        &mut CombatMotion,
        &PlayerCommand,
    )>,
) {
    for (entity, mut actor, mut input, mut motion, movement) in &mut actors {
        let (contact, shot) = actor.step(&mut input, &movement.0, time.delta_secs(), &mut motion);
        if let Some((from, to)) = contact {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.sequence,
                kind: AttackKind::Melee {
                    action: actor.action,
                    from,
                    to,
                },
            });
        }
        if shot {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.shots,
                kind: AttackKind::Shot(input.aim_point),
            });
        }
        if let Some(blow) = actor.take_strike() {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.sequence,
                kind: AttackKind::Strike {
                    reach: blow.reach,
                    arc: blow.arc,
                    damage: blow.damage,
                    posture: blow.posture,
                    on_hit: blow.on_hit,
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp, reason = "bit for bit is the claim")]
    fn a_shot_turns_inside_its_cone_and_only_by_its_count() {
        let aim = Vec3::new(0.3, -0.2, -0.9).normalize();
        assert_eq!(deviate(aim, 0., [0.7, 0.2]).to_array(), aim.to_array());
        let mut widest = 0_f32;
        for count in 0..1000 {
            let turned = deviate(aim, 1.2, shot_draw(count, 1, Weapon::Carbine));
            let angle = aim
                .cross(turned)
                .length()
                .atan2(aim.dot(turned))
                .to_degrees();
            assert!(angle <= 1.2 + 1e-3, "{count}: {angle} degrees");
            widest = widest.max(angle);
        }
        assert!(widest > 0.6, "the cone is used: {widest}");
        assert_eq!(
            shot_draw(7, 1, Weapon::Carbine),
            shot_draw(7, 1, Weapon::Carbine)
        );
        assert_ne!(
            shot_draw(7, 1, Weapon::Carbine),
            shot_draw(8, 1, Weapon::Carbine)
        );
    }

    #[test]
    fn dodge_distance_is_invariant_to_tick_rate() {
        for hz in [30., 60., 120.] {
            let dt = 1. / hz;
            let mut actor = Combatant::default();
            let mut input = CombatIntent {
                dodge: true,
                ..Default::default()
            };
            let mut motion = CombatMotion::default();
            let mut distance = 0.;
            for _ in 0..120 {
                actor.step(&mut input, &PlayerIntent::default(), dt, &mut motion);
                distance += motion.velocity.unwrap_or_default().length() * dt;
            }
            assert!((distance - 3.4).abs() < 0.0001, "{hz}: {distance}");
        }
    }

    /// An actor with a blade in hand and the pistol holstered, which is
    /// what a cut needs now.
    fn cutter() -> Combatant {
        Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0)
    }

    #[test]
    fn a_tick_can_cross_the_entire_contact_window() {
        assert_eq!(contact_interval(0.1, 0.4, 0.18, 0.28), Some((0.18, 0.28)));
        assert_eq!(contact_interval(0.28, 0.5, 0.18, 0.28), None);
    }

    #[test]
    fn buffered_followup_spends_once_and_dodge_cannot_cancel_contact() {
        let mut actor = cutter();
        let mut input = CombatIntent {
            light: true,
            ..Default::default()
        };
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &movement, 0.01, &mut motion);
        assert_eq!(actor.action, Action::Light);
        input.dodge = true;
        for _ in 0..30 {
            actor.step(&mut input, &movement, 0.01, &mut motion);
        }
        assert_eq!(actor.action, Action::Light);
        while actor.age < 0.34 {
            actor.step(&mut input, &movement, 0.01, &mut motion);
        }
        input.light = true;
        for _ in 0..12 {
            actor.step(&mut input, &movement, 0.01, &mut motion);
        }
        assert_eq!(actor.action, Action::Followup);
        assert!((actor.stamina - 64.).abs() < 0.001);
    }

    #[test]
    fn contact_buffer_chains_on_the_final_sweep_tick_at_all_rates() {
        for hz in [30., 60., 120.] {
            let dt = 1. / hz;
            let movement = PlayerIntent::default();
            let mut actor = cutter();
            let mut input = CombatIntent {
                light: true,
                ..Default::default()
            };
            let mut motion = CombatMotion::default();
            let mut covered = 0.;
            let mut queued = false;
            for _ in 0..60 {
                if actor.age >= Action::Light.spec().contact_start && !queued {
                    input.light = true;
                    queued = true;
                }
                let (contact, _) = actor.step(&mut input, &movement, dt, &mut motion);
                if let Some((from, to)) = contact {
                    assert_eq!(actor.action, Action::Light);
                    assert_eq!(actor.sequence, 1, "last sweep keeps its damage identity");
                    covered += to - from;
                }
                let finished = actor.age + 0.000_001 >= Action::Light.spec().contact_end;
                actor.finish_contacts(&movement);
                if finished {
                    assert_eq!(actor.action, Action::Followup, "{hz} Hz");
                    assert_eq!(actor.sequence, 2);
                    assert!(actor.age.abs() < f32::EPSILON);
                    assert!((actor.stamina - 64.).abs() < 0.001);
                    assert!((covered - 0.09).abs() < 0.0001, "entire contact survives");
                    break;
                }
                assert_eq!(actor.action, Action::Light);
            }
            assert_eq!(actor.action, Action::Followup);
        }
    }

    #[test]
    fn chain_cannot_erase_stagger_or_spend_missing_stamina() {
        let movement = PlayerIntent::default();
        for interrupted in [false, true] {
            let mut actor = Combatant {
                action: Action::Light,
                age: 0.22,
                stamina: 0.,
                queued: Some((Action::Light, 0.15)),
                ..Default::default()
            };
            if interrupted {
                actor.damage(1., 101.);
            }
            actor.finish_contacts(&movement);
            assert_eq!(
                actor.action,
                if interrupted {
                    Action::Stagger
                } else {
                    Action::Light
                }
            );
            assert!(actor.stamina.abs() < f32::EPSILON);
        }
    }

    #[test]
    fn exhaustion_and_death_do_not_accept_actions() {
        let mut actor = Combatant {
            stamina: 1.,
            ..Default::default()
        };
        let mut input = CombatIntent {
            heavy: true,
            ..Default::default()
        };
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &PlayerIntent::default(), 0.01, &mut motion);
        assert_eq!(actor.action, Action::Ready);
        assert!(actor.damage(200., 1.));
        input.dodge = true;
        actor.step(&mut input, &PlayerIntent::default(), 0.01, &mut motion);
        assert_eq!(actor.action, Action::Dead);
        assert!(!actor.damage(1., 1.));
    }

    #[test]
    fn dodge_has_startup_and_recovery_vulnerability() {
        let mut actor = Combatant {
            action: Action::Dodge,
            ..Default::default()
        };
        actor.age = 0.05;
        assert!(actor.damage(5., 0.));
        actor.age = 0.06;
        assert!(!actor.damage(5., 0.));
        actor.age = 0.24;
        assert!(actor.damage(5., 0.));
    }

    #[test]
    fn reload_only_refills_after_completion() {
        let mut actor = Combatant {
            ammo: 0,
            ..Default::default()
        };
        let mut input = CombatIntent {
            reload: true,
            ..Default::default()
        };
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &PlayerIntent::default(), 0.01, &mut motion);
        assert_eq!(actor.ammo, 0);
        for _ in 0..140 {
            actor.step(&mut input, &PlayerIntent::default(), 0.01, &mut motion);
        }
        assert_eq!(actor.ammo, Weapon::Carbine.spec().magazine);
    }

    #[test]
    fn held_fire_preserves_cadence_across_tick_rates() {
        for hz in [30_u16, 60, 120] {
            let mut actor = Combatant {
                mode_override: Some(FireMode::Auto),
                ..Combatant::default()
            };
            let mut input = CombatIntent {
                fire: true,
                ..Default::default()
            };
            for _ in 0..hz * 2 {
                actor.step(
                    &mut input,
                    &PlayerIntent::default(),
                    1. / f32::from(hz),
                    &mut CombatMotion::default(),
                );
            }
            assert_eq!(actor.shots, 5, "two seconds at {hz} Hz");
        }
    }

    #[test]
    fn interrupted_reload_cannot_refill_and_death_clock_keeps_advancing() {
        let mut actor = Combatant {
            ammo: 0,
            ..Default::default()
        };
        let mut input = CombatIntent {
            reload: true,
            ..Default::default()
        };
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &PlayerIntent::default(), 0.1, &mut motion);
        input.dodge = true;
        actor.step(&mut input, &PlayerIntent::default(), 0.1, &mut motion);
        assert_eq!(actor.action, Action::Dodge);
        assert_eq!(actor.ammo, 0);
        actor.age = 0.3;
        actor.damage(1000., 0.);
        actor.step(&mut input, &PlayerIntent::default(), 0.1, &mut motion);
        assert_eq!(actor.action, Action::Dead);
        assert!((actor.age - 0.1).abs() < 1e-6);
        assert!(motion.speed_scale.abs() < 1e-6);
    }

    /// An actor equipped with an empty loadout, and one fitted to a sheet
    /// and then equipped with one, are the actor that never heard of a
    /// loadout: over ten seconds of every kind of press, each tick's
    /// outputs and the whole of its state, private clocks included, are the
    /// same to the bit.
    #[test]
    fn an_empty_loadout_leaves_every_read_alone() {
        use crate::fitting::Loadout;
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        // The whole moveset and both weapons, with boxes that move the
        // rows a loadout would be merged onto.
        for learned in [
            "blade.novice",
            "blade.cuts.1",
            "blade.cuts.2",
            "blade.cuts.3",
            "marksman.novice",
            "marksman.carbine.1",
            "marksman.carbine.2",
            "marksman.pistol.1",
            "marksman.pistol.2",
        ] {
            sheet.learned.insert(learned.to_owned());
        }
        for sheet in [None, Some(&sheet)] {
            // The gun for the shots and the reload, the blade for the cuts.
            let mut plain =
                Combatant::default().carrying([Some(Weapon::Carbine), Some(Weapon::Blade)], 0);
            let mut equipped = plain.clone();
            if let Some(sheet) = sheet {
                plain.fit(&graph, sheet);
                equipped.fit(&graph, sheet);
            }
            equipped.equip(&Loadout::default());
            assert!(equipped.loadout().is_none());
            let mut inputs = [CombatIntent::default(), CombatIntent::default()];
            let mut motions = [CombatMotion::default(), CombatMotion::default()];
            let dt = 1. / 60.;
            for tick in 0..600_u32 {
                let movement = PlayerIntent {
                    yaw: 0.01 * f32::from(u16::try_from(tick).expect("a short run")),
                    ..PlayerIntent::default()
                };
                for input in &mut inputs {
                    input.fire = (40..200).contains(&tick) || tick > 520;
                    input.aim = tick % 90 < 45;
                    input.reload |= tick == 210 || tick == 400;
                    input.light |= tick % 150 == 20 || tick % 150 == 45;
                    input.heavy |= tick == 330;
                    input.dodge |= tick == 370;
                    if tick == 250 {
                        input.weapon = Some(1);
                    }
                    if tick == 450 {
                        input.weapon = Some(0);
                    }
                }
                // Re-equipped mid-run, as a screen that changed nothing
                // might ask.
                if tick == 300 {
                    equipped.equip(&Loadout::default());
                }
                let a = plain.step(&mut inputs[0], &movement, dt, &mut motions[0]);
                let b = equipped.step(&mut inputs[1], &movement, dt, &mut motions[1]);
                plain.finish_contacts(&movement);
                equipped.finish_contacts(&movement);
                assert_eq!(a, b, "tick {tick}");
                assert_eq!(
                    format!("{plain:?} {:?}", motions[0]),
                    format!("{equipped:?} {:?}", motions[1]),
                    "tick {tick}"
                );
                for weapon in Weapon::ALL {
                    assert_eq!(plain.spec_of(weapon), equipped.spec_of(weapon));
                }
                for action in Action::ALL {
                    assert_eq!(plain.spec(action), equipped.spec(action));
                }
                assert_eq!(plain.duration().to_bits(), equipped.duration().to_bits());
            }
            assert!(
                plain.shots > 8 && plain.sequence > 5,
                "the run did things: {} shots, {} actions",
                plain.shots,
                plain.sequence
            );
            assert_eq!(plain.weapon, Weapon::Carbine, "and swapped there and back");
        }
    }

    #[test]
    fn a_swap_holsters_the_magazine_and_fires_at_the_new_cadence() {
        let mut actor = Combatant::default();
        let mut input = CombatIntent::default();
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        input.fire = true;
        for _ in 0..6 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        let spent = Weapon::Carbine.spec().magazine - actor.ammo;
        assert!(spent >= 1, "the carbine fired");
        input.weapon = Some(1);
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Swap);
        assert_eq!(actor.weapon, Weapon::Carbine, "the sim swaps at the end");
        let shots = actor.shots;
        for _ in 0..30 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert_eq!(actor.weapon, Weapon::Pistol);
        let since = actor.shots - shots;
        assert!(since <= 1, "no shots during the swap, one after: {since}");
        #[allow(clippy::cast_possible_truncation, reason = "at most one")]
        let since = since as u16;
        assert_eq!(actor.ammo, Weapon::Pistol.spec().magazine - since);
        assert_eq!(
            actor.holstered_ammo,
            Weapon::Carbine.spec().magazine - spent
        );
        assert_eq!(actor.magazine(), Weapon::Pistol.spec().magazine);
        let shots = actor.shots;
        for tick in 0..240 {
            // A pistol fires a press at a time.
            input.fire = tick % 2 == 0;
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        input.fire = true;
        assert!(
            actor.shots - shots >= u64::from(Weapon::Pistol.spec().magazine),
            "the pistol empties its magazine in under four seconds"
        );
        assert_eq!(actor.ammo, 0);
        input.weapon = Some(1);
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_ne!(
            actor.action,
            Action::Swap,
            "drawing the weapon in hand is a no-op"
        );
        input.fire = false;
        input.weapon = Some(0);
        for _ in 0..40 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert_eq!(actor.weapon, Weapon::Carbine);
        assert_eq!(
            actor.ammo,
            Weapon::Carbine.spec().magazine - spent,
            "the carbine keeps its rounds"
        );
        assert_eq!(actor.holstered_ammo, 0);
    }

    /// Step `actor` for `seconds` at `hz`, the trigger as `fire` says at
    /// each tick's start; the time of each tick that fired a round.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "tick counts of a few hundred"
    )]
    fn fired_at(
        actor: &mut Combatant,
        hz: u16,
        seconds: f32,
        fire: impl Fn(f32) -> bool,
    ) -> Vec<f32> {
        let dt = 1. / f32::from(hz);
        let mut input = CombatIntent::default();
        let mut times = Vec::new();
        for tick in 0..(seconds * f32::from(hz)).round() as u32 {
            let now = tick as f32 * dt;
            input.fire = fire(now);
            let (_, shot) = actor.step(
                &mut input,
                &PlayerIntent::default(),
                dt,
                &mut CombatMotion::default(),
            );
            if shot {
                times.push(now + dt);
            }
        }
        times
    }

    fn pistol() -> Combatant {
        Combatant {
            weapon: Weapon::Pistol,
            ammo: Weapon::Pistol.spec().magazine,
            ..Combatant::default()
        }
    }

    #[test]
    fn a_single_shot_fires_once_a_press_and_not_again_while_held() {
        for hz in [30_u16, 60, 120] {
            let mut actor = pistol();
            let held = fired_at(&mut actor, hz, 2., |_| true);
            assert_eq!(held.len(), 1, "a held trigger is one shot at {hz} Hz");
            // Tapped every 0.2 s, faster than the interval allows: the
            // shots keep to the interval at best.
            let mut actor = pistol();
            let interval = Weapon::Pistol.spec().interval;
            let tapped = fired_at(&mut actor, hz, 2., |t| (t % 0.2) < 0.1);
            assert!(tapped.len() >= 3, "taps fire: {tapped:?}");
            for pair in tapped.windows(2) {
                assert!(
                    pair[1] - pair[0] >= interval - 1. / f32::from(hz) - 1e-4,
                    "{hz} Hz: {tapped:?}"
                );
            }
        }
    }

    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "tick counts of a few hundred"
    )]
    fn a_burst_is_three_rounds_a_gap_apart_and_finishes_after_the_trigger_is_let_go() {
        let spec = Weapon::Carbine.spec();
        let FireMode::Burst { count, gap } = spec.mode else {
            panic!("the carbine bursts");
        };
        for hz in [30_u16, 60, 120] {
            let dt = 1. / f32::from(hz);
            // The trigger is down for one tick only.
            let mut actor = Combatant::default();
            let shots = fired_at(&mut actor, hz, 1., |t| t < dt * 0.5);
            assert_eq!(shots.len(), usize::from(count), "{hz} Hz: {shots:?}");
            assert_eq!(actor.ammo, spec.magazine - u16::from(count));
            for (round, time) in shots.iter().enumerate() {
                let want = dt + gap * round as f32;
                assert!((time - want).abs() <= dt + 1e-4, "{hz} Hz: {shots:?}");
            }
            // Pressed again at once, the next burst waits out the interval
            // from the last round.
            let mut actor = Combatant::default();
            let shots = fired_at(&mut actor, hz, 2., |t| (t % 0.3) < 0.1 && t < 1.3);
            let last = shots[2];
            let next = shots[3];
            assert!(
                next - last >= spec.interval - dt - 1e-4,
                "{hz} Hz: {shots:?}"
            );
        }
    }

    #[test]
    fn the_facts_of_a_row_are_its_own_figures() {
        let facts = Weapon::Carbine.spec().facts();
        assert!(facts.hold);
        assert_eq!(facts.share_percent, 55);
        assert!((facts.reach - 70.).abs() < f32::EPSILON);
        assert!(!Weapon::Pistol.spec().facts().hold);
    }

    #[test]
    fn the_hold_keeps_the_same_rounds_at_every_tick_rate() {
        let counts: Vec<usize> = [30_u16, 60, 120]
            .map(|hz| fired_at(&mut Combatant::default(), hz, 1.5, |t| t < 1.))
            .iter()
            .map(Vec::len)
            .collect();
        let most = counts.iter().max().expect("three rates");
        let least = counts.iter().min().expect("three rates");
        assert!(most - least <= 1, "{counts:?}");
    }

    #[test]
    fn an_early_press_is_kept_for_the_buffer_and_a_much_earlier_one_is_not() {
        let interval = Weapon::Pistol.spec().interval;
        for hz in [30_u16, 60, 120] {
            let dt = 1. / f32::from(hz);
            // A tap 0.05 s before the interval ends, released at once.
            let early = interval - 0.05;
            let shots = fired_at(&mut pistol(), hz, 1., |t| {
                t < dt * 0.5 || (early..early + dt).contains(&t)
            });
            assert_eq!(shots.len(), 2, "{hz} Hz: {shots:?}");
            assert!(
                (shots[1] - shots[0] - interval).abs() <= 2. * dt + 1e-4,
                "{hz} Hz: it fires when the interval ends: {shots:?}"
            );
            // A carbine press 0.1 s before its pause ends opens one burst.
            let carbine = Weapon::Carbine.spec();
            let FireMode::Burst { count, gap } = carbine.mode else {
                panic!("the carbine bursts");
            };
            let last = gap * f32::from(count - 1);
            let early = last + carbine.interval - 0.1;
            let shots = fired_at(&mut Combatant::default(), hz, 1.5, |t| {
                t < dt * 0.5 || (early..early + dt).contains(&t)
            });
            assert_eq!(shots.len(), 2 * usize::from(count), "{hz} Hz: {shots:?}");
            // A press 0.3 s early is dropped.
            let early = last + carbine.interval - 0.3;
            let shots = fired_at(&mut Combatant::default(), hz, 1.5, |t| {
                t < dt * 0.5 || (early..early + dt).contains(&t)
            });
            assert_eq!(shots.len(), usize::from(count), "{hz} Hz: {shots:?}");
        }
    }

    #[test]
    fn a_second_click_during_a_burst_is_not_a_hold() {
        for hz in [30_u16, 60, 120] {
            let shots = fired_at(&mut Combatant::default(), hz, 1., |t| t < 0.01 || t >= 0.15);
            assert_eq!(shots.len(), 3, "{hz} Hz: {shots:?}");
        }
    }

    #[test]
    fn a_swap_leaves_the_old_guns_bloom_behind() {
        let mut actor = Combatant {
            ammo: 1000,
            ..Combatant::default()
        };
        fired_at(&mut actor, 60, 2., |_| true);
        assert!(actor.bloom() > 3.);
        let mut input = CombatIntent {
            weapon: Some(1),
            ..Default::default()
        };
        for _ in 0..60 {
            actor.step(
                &mut input,
                &PlayerIntent::default(),
                1. / 60.,
                &mut CombatMotion::default(),
            );
        }
        assert_eq!(actor.weapon, Weapon::Pistol);
        assert!(actor.bloom() < 1e-6, "{}", actor.bloom());
    }

    #[test]
    fn a_burst_ends_on_an_empty_magazine_and_on_a_stagger() {
        let mut actor = Combatant {
            ammo: 2,
            ..Combatant::default()
        };
        let shots = fired_at(&mut actor, 60, 1., |t| t < 0.01);
        assert_eq!(shots.len(), 2, "the magazine ran dry mid burst");
        assert_eq!(actor.ammo, 0);

        let mut actor = Combatant::default();
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let (_, first) = actor.step(&mut input, &movement, 1. / 60., &mut motion);
        assert!(first);
        input.fire = false;
        actor.damage(0., 1000.);
        assert_eq!(actor.action, Action::Stagger);
        for _ in 0..30 {
            actor.step(&mut input, &movement, 1. / 60., &mut motion);
        }
        assert_eq!(actor.shots, 1, "the stagger ended the burst");
    }

    #[test]
    fn the_hold_fires_at_its_own_gap_after_the_burst_and_stops_on_release() {
        let spec = Weapon::Carbine.spec();
        let hold = spec.hold.expect("the carbine has a hold");
        for hz in [30_u16, 60, 120] {
            let dt = 1. / f32::from(hz);
            let mut actor = Combatant::default();
            let shots = fired_at(&mut actor, hz, 2., |t| t < 1.);
            assert!(shots.len() > 6, "{hz} Hz: {shots:?}");
            let later: Vec<f32> = shots.iter().copied().skip(2).collect();
            for pair in later.windows(2) {
                assert!(
                    (pair[1] - pair[0] - hold.gap).abs() <= dt + 1e-4,
                    "{hz} Hz: {shots:?}"
                );
            }
            assert!(
                shots.iter().all(|t| *t <= 1. + dt + 1e-4),
                "nothing fires after the release: {shots:?}"
            );
            // A new press after the release is a burst again.
            let again = fired_at(&mut actor, hz, 1., |t| t < dt * 0.5);
            assert_eq!(again.len(), 3, "{hz} Hz: a burst, not another hold");
        }
    }

    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "tick counts of a few hundred"
    )]
    fn bloom_adds_caps_waits_and_settles_by_its_figures() {
        let spec = Weapon::Pistol.spec();
        let mut actor = pistol();
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 100.;
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        assert!(
            (actor.bloom() - spec.bloom.shot).abs() < 1e-6,
            "one round adds"
        );
        assert!(
            (actor.cone_when(false) - (spec.spread + spec.bloom.shot)).abs() < 1e-6,
            "the cone is the row's end plus the bloom"
        );
        input.fire = false;
        let waiting = (spec.bloom.wait / dt).floor() as u32 - 1;
        for _ in 0..waiting {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert!(
            (actor.bloom() - spec.bloom.shot).abs() < 1e-6,
            "it waits before it settles"
        );
        // Five ticks after the wait, while there is still bloom to see.
        for _ in 0..5 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        let settled_for = (waiting + 5) as f32 * dt - spec.bloom.wait;
        let want = spec.bloom.shot - spec.bloom.settle * settled_for;
        assert!(want > 0.1, "the check reads a bloom not yet at nought");
        assert!(
            (actor.bloom() - want).abs() < spec.bloom.settle * dt * 1.01,
            "it settles by its figure: {} against {want}",
            actor.bloom()
        );
        // A hold never gives the cone time to settle, and stops at the cap.
        let carbine = Weapon::Carbine.spec();
        let mut actor = Combatant {
            ammo: 1000,
            ..Combatant::default()
        };
        fired_at(&mut actor, 60, 5., |_| true);
        assert!(
            (actor.bloom() - carbine.bloom.cap).abs() < 1e-6,
            "{}",
            actor.bloom()
        );
    }

    #[test]
    fn the_cone_is_the_row_plus_bloom_plus_the_moving_share() {
        let spec = Weapon::Carbine.spec();
        let mut actor = Combatant::default();
        let mut input = CombatIntent::default();
        let mut motion = CombatMotion::default();
        let mut movement = PlayerIntent::default();
        actor.step(&mut input, &movement, 0.01, &mut motion);
        assert!((actor.cone_when(true) - spec.aim_spread).abs() < 1e-6);
        assert!((actor.cone_when(false) - spec.spread).abs() < 1e-6);
        movement.move_dir = bevy::math::Vec2::new(0., 1.);
        actor.step(&mut input, &movement, 0.01, &mut motion);
        assert!(
            (actor.cone_when(false) - (spec.spread + spec.move_spread)).abs() < 1e-6,
            "a full run adds the whole figure"
        );
        movement.move_dir = bevy::math::Vec2::new(0., 0.5);
        actor.step(&mut input, &movement, 0.01, &mut motion);
        assert!((actor.cone_when(true) - (spec.aim_spread + 0.5 * spec.move_spread)).abs() < 1e-6);
        input.fire = true;
        actor.step(&mut input, &movement, 0.01, &mut motion);
        assert!(
            (actor.cone_when(true) - (spec.aim_spread + 0.5 * spec.move_spread + spec.bloom.shot))
                .abs()
                < 1e-3
        );
        assert!(
            actor.shot_cone() < actor.cone(),
            "the round left in the cone before its own bloom"
        );
    }

    #[test]
    fn the_falloff_is_a_straight_line_between_full_and_far() {
        let falloff = Falloff {
            full: 20.,
            far: 45.,
            share: 0.5,
        };
        for (flown, want) in [(0., 1.), (20., 1.), (32.5, 0.75), (45., 0.5), (200., 0.5)] {
            assert!(
                (falloff.share_at(flown) - want).abs() < 1e-6,
                "{flown} m: {}",
                falloff.share_at(flown)
            );
        }
        let spec = Weapon::Carbine.spec();
        assert!((spec.share_at(spec.falloff.far) - spec.falloff.share).abs() < 1e-6);
        assert!((spec.share_at(spec.range) - spec.falloff.share).abs() < 1e-6);
        assert!(spec.share_at(spec.range + 1.).abs() < f32::EPSILON);
    }

    /// A row's eight figures as bits, so equal means equal to the bit.
    fn row_bits(spec: &ActionSpec) -> [u32; 8] {
        [
            spec.contact_start,
            spec.contact_end,
            spec.duration,
            spec.dodge_cancel,
            spec.stamina,
            spec.damage,
            spec.posture,
            spec.travel,
        ]
        .map(f32::to_bits)
    }

    const CUTS: [Action; 3] = [Action::Light, Action::Followup, Action::Heavy];

    #[test]
    fn a_blade_reads_the_bits_of_the_row_bare_and_with_each_blade_box() {
        let graph = SkillGraph::shipped();
        let blade = cutter();
        for action in CUTS {
            assert_eq!(
                row_bits(&blade.spec(action)),
                row_bits(&action.spec()),
                "{action:?} bare"
            );
        }
        let boxes: Vec<String> = graph
            .tree("blade")
            .expect("ships")
            .boxes()
            .map(|b| b.id.clone())
            .collect();
        assert!(boxes.len() > 10, "the whole Blade tree");
        for id in &boxes {
            let mut sheet = CharacterSheet::default();
            sheet.learned.insert(id.clone());
            let mut actor = cutter();
            actor.fit(&graph, &sheet);
            let effective = apply(&combat_tables::current(), &graph, &sheet);
            for action in CUTS {
                assert_eq!(
                    row_bits(&actor.spec(action)),
                    row_bits(&effective.spec(action)),
                    "{id} {action:?}"
                );
            }
        }
        // All of them together, Sets and Muls and all.
        let mut sheet = CharacterSheet::default();
        sheet.learned.extend(boxes);
        let mut actor = cutter();
        actor.fit(&graph, &sheet);
        let effective = apply(&combat_tables::current(), &graph, &sheet);
        for action in CUTS {
            assert_eq!(
                row_bits(&actor.spec(action)),
                row_bits(&effective.spec(action)),
                "the whole tree {action:?}"
            );
        }
    }

    /// A blade carrying a recovery factor of `by` and nothing else.
    fn tailed(by: f32) -> Combatant {
        use crate::fitting::{Factor, Loadout};
        use strata_registry::item::{CountRule, Property};
        let mut actor = cutter();
        actor.equip(&Loadout {
            mods: BTreeMap::from([(
                Weapon::Blade,
                vec![Factor {
                    property: Property::RecoveryTime,
                    factor: by,
                }],
            )]),
            count: CountRule::Nearest,
        });
        actor
    }

    #[test]
    fn a_recovery_factor_moves_only_the_tail_and_one_is_the_row_to_the_bit() {
        let graph = SkillGraph::shipped();
        // A factor of one reads every bit of every cut, bare and with each
        // box of the whole Blade tree.
        let boxes: Vec<String> = graph
            .tree("blade")
            .expect("ships")
            .boxes()
            .map(|b| b.id.clone())
            .collect();
        let mut sheets = vec![CharacterSheet::default()];
        for id in &boxes {
            let mut sheet = CharacterSheet::default();
            sheet.learned.insert(id.clone());
            sheets.push(sheet);
        }
        let mut all = CharacterSheet::default();
        all.learned.extend(boxes);
        sheets.push(all);
        for sheet in &sheets {
            let (mut plain, mut one) = (cutter(), tailed(1.));
            plain.fit(&graph, sheet);
            one.fit(&graph, sheet);
            one.equip(one.loadout().cloned().as_ref().expect("equipped"));
            for action in CUTS {
                assert_eq!(
                    row_bits(&one.spec(action)),
                    row_bits(&plain.spec(action)),
                    "{action:?}"
                );
            }
        }
        // A longer tail moves what lies after the contact and nothing else.
        for by in [0.5_f32, 1.1, 1.5] {
            let (plain, tail) = (cutter(), tailed(by));
            for action in CUTS {
                let (was, now) = (plain.spec(action), tail.spec(action));
                assert_eq!(was.contact_start.to_bits(), now.contact_start.to_bits());
                assert_eq!(was.contact_end.to_bits(), now.contact_end.to_bits());
                assert_eq!(was.damage.to_bits(), now.damage.to_bits());
                assert_eq!(was.posture.to_bits(), now.posture.to_bits());
                assert_eq!(was.stamina.to_bits(), now.stamina.to_bits());
                assert_eq!(was.travel.to_bits(), now.travel.to_bits());
                let end = was.contact_end;
                assert!((now.duration - (end + (was.duration - end) * by)).abs() < 1e-6);
                if was.dodge_cancel > end {
                    assert!(
                        (now.dodge_cancel - (end + (was.dodge_cancel - end) * by)).abs() < 1e-6
                    );
                } else {
                    assert_eq!(was.dodge_cancel.to_bits(), now.dodge_cancel.to_bits());
                }
            }
        }
    }

    #[test]
    fn the_clip_is_continuous_at_the_end_of_contact_and_whole_at_the_end() {
        let end = 0.28;
        for by in [0.5_f32, 0.92, 1., 1.1, 1.5] {
            let before = cut_clip_time(end, end, by);
            let after = cut_clip_time(end + 1e-4, end, by);
            assert!((after - before).abs() < 2e-4, "{by}: {before} {after}");
            assert_eq!(
                before.to_bits(),
                end.to_bits(),
                "contact is the clip itself"
            );
        }
        assert_eq!(cut_clip_time(0.123, end, 1.).to_bits(), 0.123_f32.to_bits());
        for by in [0.5_f32, 1.1, 1.5] {
            let actor = tailed(by);
            for action in CUTS {
                let spec = actor.spec(action);
                let whole = actor.clip_place(action, spec.duration);
                assert!((whole - 1.).abs() < 1e-5, "{action:?} {by}: {whole}");
                let mid = actor.clip_place(action, spec.contact_end);
                let plain = cutter().clip_place(action, spec.contact_end);
                assert!(
                    (mid - plain).abs() < 1e-5,
                    "{action:?} {by}: the clip's contact does not move"
                );
            }
        }
        // A dodge is no cut: a recovery part leaves its clip alone.
        for by in [0.92_f32, 1.1] {
            let actor = tailed(by);
            let spec = actor.spec(Action::Dodge);
            let mid = spec.duration / 2.;
            assert_eq!(
                actor.clip_place(Action::Dodge, mid).to_bits(),
                (mid / spec.duration).to_bits()
            );
        }
        // At one the place is what the pose read before.
        let plain = cutter();
        let spec = plain.spec(Action::Light);
        assert_eq!(
            plain.clip_place(Action::Light, 0.2).to_bits(),
            (0.2 / spec.duration).to_bits()
        );
    }

    #[test]
    fn a_greatblade_and_a_glaive_are_the_factors_over_the_blades_rows() {
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("blade.breaking.1".to_owned());
        sheet.learned.insert("blade.footwork.2".to_owned());
        for weapon in [Weapon::Greatblade, Weapon::Glaive] {
            let class = combat_tables::with_melee(|melee| melee.class(weapon)).expect("a class");
            for fitted in [false, true] {
                let mut actor =
                    Combatant::default().carrying([Some(weapon), Some(Weapon::Pistol)], 0);
                if fitted {
                    actor.fit(&graph, &sheet);
                }
                let base = |action: Action| {
                    if fitted {
                        apply(&combat_tables::current(), &graph, &sheet).spec(action)
                    } else {
                        action.spec()
                    }
                };
                for action in CUTS {
                    let scale = if action == Action::Heavy {
                        class.heavy
                    } else {
                        class.light
                    };
                    let row = base(action);
                    let want = ActionSpec {
                        contact_start: row.contact_start * scale.time,
                        contact_end: row.contact_end * scale.time,
                        duration: row.duration * scale.time,
                        dodge_cancel: row.dodge_cancel * scale.time,
                        stamina: row.stamina * scale.stamina,
                        damage: row.damage * scale.damage,
                        posture: row.posture * scale.posture,
                        travel: row.travel * scale.travel,
                    };
                    assert_eq!(
                        row_bits(&actor.spec(action)),
                        row_bits(&want),
                        "{weapon:?} {action:?} fitted {fitted}"
                    );
                }
                // The other rows are no class's business.
                assert_eq!(
                    row_bits(&actor.spec(Action::Dodge)),
                    row_bits(&base(Action::Dodge))
                );
            }
        }
        // And the Greatblade is slower and harder, the Glaive longer.
        let great = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        let glaive = Combatant::default().carrying([Some(Weapon::Glaive), None], 0);
        assert!(great.spec(Action::Light).duration > Action::Light.spec().duration);
        assert!(great.spec(Action::Light).damage > Action::Light.spec().damage);
        assert!(glaive.reach_factor() > great.reach_factor() && great.reach_factor() > 1.);
    }

    #[test]
    fn swing_posture_scales_a_hit_taken_mid_swing_and_not_one_at_rest() {
        let swing = combat_tables::with_melee(|melee| melee.class(Weapon::Greatblade))
            .expect("a class")
            .swing_posture;
        assert!(swing < 1.);
        let mut resting = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        let before = resting.posture;
        resting.hit(Blow::bare(0., 40.));
        assert!(
            (before - resting.posture - 40.).abs() < 1e-4,
            "at rest in full"
        );
        let mut cutting = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        cutting.action = Action::Light;
        cutting.hit(Blow::bare(0., 40.));
        assert!(
            (before - cutting.posture - 40. * swing).abs() < 1e-4,
            "mid swing a share: {}",
            cutting.posture
        );
        // A blade has no such share, and a gun is no swing.
        let mut blade = cutter();
        blade.action = Action::Light;
        blade.hit(Blow::bare(0., 40.));
        assert!((before - blade.posture - 40.).abs() < 1e-4);
    }

    #[test]
    fn a_riposte_is_the_guard_rows_figure_whatever_the_swingers_class() {
        let swing = combat_tables::with_melee(|melee| melee.class(Weapon::Greatblade))
            .expect("a class")
            .swing_posture;
        assert!(swing < 1.);
        let mut great = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        let before = great.posture;
        great.action = Action::Light;
        great.riposted(40.);
        assert!(
            (before - great.posture - 40.).abs() < 1e-4,
            "a riposte mid swing is not braced against: {}",
            great.posture
        );
        let mut braced = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        braced.action = Action::Light;
        braced.hit(Blow::bare(0., 40.));
        assert!(
            braced.posture > great.posture,
            "a blow met mid swing still is"
        );
    }

    #[test]
    fn a_cut_pressed_late_in_a_draw_does_not_begin_with_the_gun_up() {
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0);
        let mut draw = CombatIntent {
            weapon: Some(1),
            ..Default::default()
        };
        actor.step(&mut draw, &movement, 1. / 60., &mut motion);
        assert_eq!(actor.action, Action::Swap);
        let swaps = actor.sequence;
        let mut ticks = 0;
        while actor.action == Action::Swap && ticks < 200 {
            let remaining = actor.duration() - actor.age;
            let mut input = CombatIntent {
                fire: remaining < 0.1,
                heavy: remaining < 0.1 && ticks % 2 == 0,
                ..Default::default()
            };
            actor.step(&mut input, &movement, 1. / 60., &mut motion);
            ticks += 1;
        }
        for _ in 0..30 {
            actor.step(
                &mut CombatIntent::default(),
                &movement,
                1. / 60.,
                &mut motion,
            );
            assert!(!actor.action.is_melee());
        }
        assert_eq!(actor.weapon, Weapon::Carbine);
        assert_eq!(actor.sequence, swaps);
    }

    #[test]
    fn a_cut_with_a_gun_in_hand_is_refused_once_a_press_and_nothing_begins() {
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let mut actor = Combatant::default();
        for (light, heavy) in [(true, false), (false, true)] {
            let mut input = CombatIntent {
                light,
                heavy,
                ..Default::default()
            };
            let before = actor.refusals;
            actor.step(&mut input, &movement, 1. / 60., &mut motion);
            assert_eq!(actor.refused, Some(Refusal::NoBlade));
            assert_eq!(actor.refusals, before + 1, "one a press");
            // The edge is spent: the next ticks refuse nothing more.
            for _ in 0..10 {
                actor.step(&mut input, &movement, 1. / 60., &mut motion);
            }
            assert_eq!(actor.refusals, before + 1);
        }
        assert_eq!(
            (actor.action, actor.sequence, actor.queued()),
            (Action::Ready, 0, None)
        );
        assert!((actor.stamina - 100.).abs() < 1e-3, "nothing was paid");
    }

    #[test]
    fn the_triggers_edge_cuts_and_chains_with_a_blade_and_a_held_trigger_cuts_once() {
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        // A held trigger: one cut, never a second.
        let mut actor = cutter();
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        for _ in 0..90 {
            actor.step(&mut input, &movement, dt, &mut motion);
            actor.finish_contacts(&movement);
        }
        assert_eq!((actor.sequence, actor.shots), (1, 0), "one cut and no bolt");
        // A second press in the cut's contact chains the return cut.
        let mut actor = cutter();
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Light);
        input.fire = false;
        let mut saw = false;
        let mut pressed = false;
        for _ in 0..90 {
            if actor.action == Action::Light && actor.age >= 0.14 && !pressed {
                input.fire = true;
                pressed = true;
            } else {
                input.fire = false;
            }
            actor.step(&mut input, &movement, dt, &mut motion);
            actor.finish_contacts(&movement);
            saw |= actor.action == Action::Followup;
        }
        assert!(saw, "the second press is the return cut");
        // The heavy edge is the Heavy.
        let mut actor = cutter();
        let mut input = CombatIntent {
            heavy: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Heavy);
    }

    #[test]
    fn reload_does_nothing_and_aim_is_a_guard_with_a_blade_in_hand() {
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let mut actor = cutter();
        assert_eq!((actor.ammo, actor.magazine()), (0, 0), "no magazine");
        let mut input = CombatIntent {
            reload: true,
            aim: true,
            ..Default::default()
        };
        for _ in 0..30 {
            actor.step(&mut input, &movement, 1. / 60., &mut motion);
            // The aim button is the guard now, which moves at the aim pace.
            assert_eq!(
                motion.speed_scale.to_bits(),
                actor.with_actor(|a| a.aim_speed_scale).to_bits()
            );
        }
        assert_eq!(actor.action, Action::Ready);
        assert!(!actor.aiming && actor.guard_up);
        assert_eq!(actor.refusals, 0);
    }

    #[test]
    fn a_draw_names_a_slot_and_an_empty_one_the_one_in_hand_or_an_uncertified_class_is_refused() {
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        let mut draw = |actor: &mut Combatant, slot: usize| {
            let mut input = CombatIntent {
                weapon: Some(slot),
                ..Default::default()
            };
            for _ in 0..60 {
                actor.step(&mut input, &movement, dt, &mut motion);
            }
        };
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Carbine), Some(Weapon::Blade)], 0);
        draw(&mut actor, 0);
        assert_eq!(
            (actor.weapon, actor.action),
            (Weapon::Carbine, Action::Ready),
            "in hand"
        );
        draw(&mut actor, 5);
        assert_eq!(actor.weapon, Weapon::Carbine, "no such slot");
        draw(&mut actor, 1);
        assert_eq!(actor.weapon, Weapon::Blade, "the second slot");
        assert_eq!(actor.hand(), 1);
        draw(&mut actor, 0);
        assert_eq!(actor.weapon, Weapon::Carbine, "and back");
        // An empty slot.
        let mut empty = Combatant::default().carrying([Some(Weapon::Carbine), None], 0);
        draw(&mut empty, 1);
        assert_eq!(
            (empty.weapon, empty.action, empty.refusals),
            (Weapon::Carbine, Action::Ready, 0)
        );
        // A class the sheet has not certified.
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("marksman.novice".to_owned());
        let mut gun_only =
            Combatant::default().carrying([Some(Weapon::Pistol), Some(Weapon::Glaive)], 0);
        gun_only.fit(&graph, &sheet);
        draw(&mut gun_only, 1);
        assert_eq!(gun_only.weapon, Weapon::Pistol);
        assert_eq!(gun_only.refused, Some(Refusal::Draw(Weapon::Glaive)));
        // With the box, the class draws.
        sheet.learned.insert("blade.footwork.1".to_owned());
        gun_only.fit(&graph, &sheet);
        draw(&mut gun_only, 1);
        assert_eq!(gun_only.weapon, Weapon::Glaive);
    }

    #[test]
    fn the_slots_follow_the_hands_and_an_emptied_hand_draws_the_other() {
        let mut actor = Combatant::default();
        let rounds = actor.ammo;
        // The pistol's slot takes a glaive; the carbine in hand is kept.
        actor.carry([Some(Weapon::Carbine), Some(Weapon::Glaive)]);
        assert_eq!((actor.weapon, actor.ammo), (Weapon::Carbine, rounds));
        assert_eq!(
            (actor.holstered(), actor.holstered_ammo),
            (Some(Weapon::Glaive), 0)
        );
        // The hand in use takes a blade: it is in hand, and has no rounds.
        actor.carry([Some(Weapon::Blade), Some(Weapon::Glaive)]);
        assert_eq!(
            (actor.weapon, actor.ammo, actor.hand()),
            (Weapon::Blade, 0, 0)
        );
        // A gun new to a slot comes full.
        actor.carry([Some(Weapon::Blade), Some(Weapon::Pistol)]);
        assert_eq!(actor.holstered_ammo, Weapon::Pistol.spec().magazine);
        // The slot in hand emptied: the other is drawn.
        actor.carry([None, Some(Weapon::Pistol)]);
        assert_eq!((actor.weapon, actor.hand()), (Weapon::Pistol, 1));
        assert_eq!(actor.ammo, Weapon::Pistol.spec().magazine);
    }

    // The guard (ticket 0010, step 2b).

    const TICK: f32 = 1. / 60.;

    /// Whole ticks that cover `seconds`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few seconds of ticks, never negative"
    )]
    fn ticks_of(seconds: f32) -> u32 {
        (seconds / TICK).ceil() as u32
    }

    fn guard_row(weapon: Weapon) -> GuardRow {
        combat_tables::with_melee(|melee| melee.class(weapon))
            .expect("a class")
            .guard
    }

    /// Step `actor` for `ticks` with the aim button as given.
    fn hold(actor: &mut Combatant, aim: bool, ticks: u32) {
        let mut motion = CombatMotion::default();
        for _ in 0..ticks {
            let mut input = CombatIntent {
                aim,
                ..Default::default()
            };
            actor.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        }
    }

    /// A blow from straight ahead of a holder facing along the default yaw.
    fn ahead(damage: f32, posture: f32, kind: BlowKind) -> Blow {
        Blow::new(damage, posture, Vec3::new(0., 0., -4.), kind)
    }

    /// A guarded blade with no shield, the guard held for `ticks`.
    fn guarded(weapon: Weapon, ticks: u32) -> Combatant {
        let mut actor = Combatant::default().carrying([Some(weapon), None], 0);
        hold(&mut actor, true, ticks);
        actor.shield = 0.;
        actor
    }

    #[test]
    fn the_outcome_function_names_all_four_for_each_class() {
        for weapon in Weapon::MELEE {
            let row = guard_row(weapon);
            let state = |up_for, rested| GuardState {
                up: true,
                up_for,
                rested,
            };
            let blow = ahead(40., 30., BlowKind::Cut);
            let ask = |state, from: Vec3, stamina| {
                guard_outcome(
                    &row,
                    state,
                    0.,
                    &Blow {
                        from,
                        ..blow.clone()
                    },
                    stamina,
                )
            };
            let front = Vec3::new(0., 0., -3.);
            let late = row.parry + 0.05;
            assert_eq!(ask(state(late, true), front, 100.), GuardOutcome::Blocked);
            assert_eq!(
                ask(state(row.parry * 0.5, true), front, 100.),
                GuardOutcome::Parried,
                "{weapon:?}"
            );
            assert_eq!(
                ask(state(row.parry * 0.5, false), front, 100.),
                GuardOutcome::Blocked,
                "an unrested guard does not parry"
            );
            let cost = 40. * row.share * row.stamina;
            assert_eq!(
                ask(state(late, true), front, cost - 0.5),
                GuardOutcome::Broken
            );
            assert_eq!(ask(state(late, true), front, cost), GuardOutcome::Blocked);
            assert_eq!(
                ask(state(0., true), Vec3::new(0., 0., 3.), 100.),
                GuardOutcome::Open,
                "from behind"
            );
            assert_eq!(
                ask(
                    GuardState {
                        up: false,
                        up_for: 0.,
                        rested: true
                    },
                    front,
                    100.
                ),
                GuardOutcome::Open,
                "guard down"
            );
            // Just inside and just outside the arc, either side.
            for side in [-1., 1.] {
                let at = |degrees: f32| {
                    let angle = degrees.to_radians();
                    Vec3::new(side * angle.sin(), 0., -angle.cos())
                };
                assert_ne!(
                    ask(state(late, true), at(row.arc - 1.), 100.),
                    GuardOutcome::Open
                );
                assert_eq!(
                    ask(state(late, true), at(row.arc + 1.), 100.),
                    GuardOutcome::Open
                );
            }
            assert_eq!(
                ask(state(late, true), Vec3::ZERO, 100.),
                GuardOutcome::Open,
                "no known direction"
            );
        }
    }

    #[test]
    fn a_trained_parry_window_turns_a_block_at_its_edge_into_a_parry() {
        use crate::progression::CharacterSheet;
        let graph = SkillGraph::shipped();
        let sheet = CharacterSheet {
            learned: ["blade.novice", "blade.guard.1", "blade.guard.2"]
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
            ..CharacterSheet::default()
        };
        let mut trained = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        trained.fit(&graph, &sheet);
        let plain = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        let table = guard_row(Weapon::Blade);
        let row = trained.guard_row(Weapon::Blade).expect("row");
        assert!((row.parry - (table.parry + 0.03)).abs() < 1e-6);
        // A hit 0.015 s past the table's window and inside the trained one.
        let state = GuardState {
            up: true,
            up_for: table.parry + 0.015,
            rested: true,
        };
        let blow = ahead(40., 30., BlowKind::Cut);
        assert_eq!(
            guard_outcome(
                &plain.guard_row(Weapon::Blade).expect("row"),
                state,
                0.,
                &blow,
                100.
            ),
            GuardOutcome::Blocked
        );
        assert_eq!(
            guard_outcome(&row, state, 0., &blow, 100.),
            GuardOutcome::Parried
        );
        // And the combatant itself meets a blow with the trained row.
        let hold_for = |actor: &mut Combatant| {
            hold(actor, true, ticks_of(table.parry + 0.015) + 1);
            actor.shield = 0.;
            actor.hit(blow.clone()).expect("met").guard
        };
        let (mut a, mut b) = (trained.clone(), plain.clone());
        let (trained_outcome, plain_outcome) = (hold_for(&mut a), hold_for(&mut b));
        assert_eq!(plain_outcome, GuardOutcome::Blocked);
        assert_eq!(trained_outcome, GuardOutcome::Parried);
    }

    #[test]
    fn a_block_takes_the_share_off_damage_and_posture_and_pays_stamina() {
        let row = guard_row(Weapon::Blade);
        let mut actor = guarded(Weapon::Blade, 40);
        assert!(actor.guard_up);
        let (stamina, posture, health) = (actor.stamina, actor.posture, actor.health);
        let dealt = actor.hit(ahead(40., 30., BlowKind::Cut)).expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Blocked);
        assert!((dealt.pools - 40. * (1. - row.share)).abs() < 1e-3);
        assert!((health - actor.health - 40. * (1. - row.share)).abs() < 1e-3);
        assert!((posture - actor.posture - 30. * row.posture).abs() < 1e-3);
        assert!((stamina - actor.stamina - 40. * row.share * row.stamina).abs() < 1e-3);
        assert!(actor.guard_up, "a block does not drop the guard");
    }

    #[test]
    fn with_no_bolts_row_a_bolt_reads_the_guards_own_figures_and_with_one_a_bolt_reads_it() {
        for weapon in Weapon::MELEE {
            let row = guard_row(weapon);
            assert!(row.bolts.is_none(), "no shipped class sets bolts");
            assert_eq!(row.figures(true), row.figures(false));
            let mut actor = guarded(weapon, 40);
            let (stamina, health) = (actor.stamina, actor.health);
            actor.hit(ahead(40., 30., BlowKind::Bolt)).expect("lands");
            assert!((health - actor.health - 40. * (1. - row.share)).abs() < 1e-3);
            assert!((stamina - actor.stamina - 40. * row.share * row.stamina).abs() < 1e-3);
        }
        let row = GuardRow {
            bolts: Some(combat_tables::BoltGuard {
                share: 0.9,
                stamina: 0.1,
                posture: 0.,
            }),
            ..guard_row(Weapon::Blade)
        };
        assert_eq!(row.figures(true), (0.9, 0.1, 0.));
        assert_eq!(row.figures(false), (row.share, row.stamina, row.posture));
        // A break by bolts is by the bolts' stamina: 40 * 0.9 * 0.1 = 3.6 and a hair.
        let state = GuardState {
            up: true,
            up_for: 1.,
            rested: true,
        };
        let bolt = ahead(40., 30., BlowKind::Bolt);
        assert_eq!(
            guard_outcome(&row, state, 0., &bolt, 3.5),
            GuardOutcome::Broken
        );
        assert_eq!(
            guard_outcome(&row, state, 0., &bolt, 3.61),
            GuardOutcome::Blocked
        );
        // The same stamina against a cut is the row's own price.
        let cut = ahead(40., 30., BlowKind::Cut);
        assert_eq!(
            guard_outcome(&row, state, 0., &cut, 3.61),
            GuardOutcome::Broken
        );
    }

    #[test]
    fn a_blast_is_met_by_no_guard() {
        let mut actor = guarded(Weapon::Blade, 40);
        let health = actor.health;
        let dealt = actor.hit(Blow::blast(30., 20.)).expect("lands");
        assert_eq!(dealt.guard, GuardOutcome::Open);
        assert!((health - actor.health - 30.).abs() < 1e-3);
    }

    #[test]
    fn a_hit_from_behind_or_the_side_lands_in_full() {
        for from in [Vec3::new(0., 0., 4.), Vec3::new(4., 0., 0.)] {
            let mut actor = guarded(Weapon::Blade, 40);
            let stamina = actor.stamina;
            let dealt = actor
                .hit(Blow::new(40., 30., from, BlowKind::Cut))
                .expect("it lands");
            assert_eq!(dealt.guard, GuardOutcome::Open);
            assert!((dealt.pools - 40.).abs() < 1e-3);
            assert!((actor.posture - (actor.with_actor(|a| a.posture) - 30.)).abs() < 1e-3);
            assert_eq!(actor.stamina.to_bits(), stamina.to_bits());
        }
    }

    #[test]
    fn a_guard_raised_inside_the_window_parries_and_a_later_one_blocks() {
        let row = guard_row(Weapon::Blade);
        // Raised this tick, nothing waited.
        let mut quick = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        hold(&mut quick, true, 3);
        let (health, stamina, posture) = (quick.health, quick.stamina, quick.posture);
        let hurts = quick.hurts;
        let dealt = quick.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Parried);
        assert!(dealt.pools.abs() < f32::EPSILON && !dealt.lethal);
        assert!((dealt.riposte - row.riposte).abs() < 1e-3);
        assert_eq!(quick.health.to_bits(), health.to_bits());
        assert_eq!(quick.stamina.to_bits(), stamina.to_bits());
        assert_eq!(quick.posture.to_bits(), posture.to_bits());
        assert_eq!(quick.hurts, hurts, "no flinch, no hurt wait");
        // Held past the window.
        let ticks = ticks_of(row.parry + 0.1);
        let mut slow = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        hold(&mut slow, true, ticks);
        let dealt = slow.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Blocked);
        assert!(dealt.riposte.abs() < f32::EPSILON);
    }

    #[test]
    fn a_guard_dropped_and_raised_inside_the_rest_does_not_parry() {
        let rest = combat_tables::with_melee(|melee| melee.parry_rest);
        let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        hold(&mut actor, true, 60);
        hold(&mut actor, false, 6);
        hold(&mut actor, true, 3);
        let dealt = actor.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Blocked, "a hammered button");
        // Down for longer than the rest, it parries again.
        let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        hold(&mut actor, true, 60);
        hold(&mut actor, false, ticks_of(rest + 0.1));
        hold(&mut actor, true, 3);
        let dealt = actor.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Parried);
    }

    #[test]
    fn a_held_button_through_a_cut_does_not_win_a_parry() {
        let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        let mut motion = CombatMotion::default();
        hold(&mut actor, true, 20);
        let mut input = CombatIntent {
            aim: true,
            heavy: true,
            ..Default::default()
        };
        actor.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        assert!(!actor.guard_up, "the cut puts the guard down");
        let mut came_back = false;
        for _ in 0..240 {
            hold(&mut actor, true, 1);
            if actor.guard_up {
                came_back = true;
                break;
            }
        }
        assert!(came_back, "the guard returns under the held button");
        actor.shield = 0.;
        let dealt = actor.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Blocked, "held, not raised");
    }

    #[test]
    fn a_parried_cut_staggers_a_swinger_the_riposte_empties_and_dents_one_it_does_not() {
        let mut guard = guarded(Weapon::Blade, 3);
        let dealt = guard.hit(ahead(40., 30., BlowKind::Cut)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Parried);
        let full = Combatant::default().with_actor(|a| a.posture);
        // The adapter lands the riposte as a posture hit on the swinger.
        let mut weak = Combatant {
            posture: dealt.riposte - 1.,
            ..Default::default()
        };
        weak.riposted(dealt.riposte);
        assert_eq!(weak.action, Action::Stagger);
        let mut strong = Combatant::default();
        strong.riposted(dealt.riposte);
        assert_eq!(strong.action, Action::Ready);
        assert!((full - strong.posture - dealt.riposte).abs() < 1e-3);
    }

    #[test]
    fn a_parried_bolt_deals_nothing_and_owes_no_riposte() {
        let mut guard = guarded(Weapon::Blade, 3);
        let dealt = guard.hit(ahead(40., 30., BlowKind::Bolt)).expect("met");
        assert_eq!(dealt.guard, GuardOutcome::Parried);
        assert!(dealt.riposte.abs() < f32::EPSILON);
        assert_eq!(
            guard.health.to_bits(),
            Combatant::default().health.to_bits()
        );
    }

    #[test]
    fn a_guard_out_of_stamina_breaks_into_a_stagger() {
        let mut actor = guarded(Weapon::Blade, 40);
        actor.stamina = 5.;
        let dealt = actor.hit(ahead(40., 10., BlowKind::Cut)).expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Broken);
        assert_eq!(actor.action, Action::Stagger);
        assert!(actor.stamina.abs() < f32::EPSILON);
        assert!(
            (dealt.pools - 40. * (1. - guard_row(Weapon::Blade).share)).abs() < 1e-3,
            "it lands at its blocked figures"
        );
        hold(&mut actor, true, 1);
        assert!(!actor.guard_up, "a stagger puts the guard down");
    }

    #[test]
    fn the_guard_is_down_during_a_cut_a_dodge_and_a_swap() {
        let mut motion = CombatMotion::default();
        let movement = PlayerIntent::default();
        for press in 0..3 {
            let mut actor =
                Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0);
            hold(&mut actor, true, 30);
            assert!(actor.guard_up);
            let mut input = CombatIntent {
                aim: true,
                light: press == 0,
                dodge: press == 1,
                weapon: (press == 2).then_some(1),
                ..Default::default()
            };
            actor.step(&mut input, &movement, TICK, &mut motion);
            assert_ne!(actor.action, Action::Ready, "press {press} began");
            assert!(!actor.guard_up, "press {press}");
            assert!(!actor.guard_state().up);
            // And a held button does not raise it again until Ready.
            for _ in 0..3 {
                let mut input = CombatIntent {
                    aim: true,
                    ..Default::default()
                };
                actor.step(&mut input, &movement, TICK, &mut motion);
                if actor.action != Action::Ready {
                    assert!(!actor.guard_up);
                }
            }
        }
        // A gun in hand raises none.
        let mut gun = Combatant::default();
        hold(&mut gun, true, 30);
        assert!(!gun.guard_up && gun.aiming);
    }

    #[test]
    fn stamina_regenerates_at_the_guards_share_while_it_is_up() {
        let regen = combat_tables::with_melee(|melee| melee.guard_regen);
        let rate = Combatant::default().with_actor(|a| a.stamina_regen);
        let mut up = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        let mut down = up.clone();
        hold(&mut up, true, 2);
        up.stamina = 10.;
        down.stamina = 10.;
        hold(&mut up, true, 30);
        hold(&mut down, false, 30);
        let gained = (up.stamina - 10.) / (down.stamina - 10.);
        assert!((gained - regen).abs() < 0.01, "{gained} against {regen}");
        assert!((down.stamina - 10. - rate * 0.5).abs() < 0.01);
        // Holding costs nothing.
        let mut held = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        hold(&mut held, true, 120);
        assert!((held.stamina - held.with_actor(|a| a.stamina)).abs() < f32::EPSILON);
    }

    #[test]
    fn the_guard_moves_at_the_aim_pace_and_a_gun_aims_as_before() {
        let aim = Combatant::default().with_actor(|a| a.aim_speed_scale);
        let mut motion = CombatMotion::default();
        let mut blade = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        let mut input = CombatIntent {
            aim: true,
            ..Default::default()
        };
        blade.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        assert_eq!(motion.speed_scale.to_bits(), aim.to_bits());
        let mut gun = Combatant::default();
        let mut input = CombatIntent {
            aim: true,
            ..Default::default()
        };
        gun.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        assert_eq!(motion.speed_scale.to_bits(), aim.to_bits());
        assert!(gun.aiming && !gun.guard_up);
        assert_eq!(
            gun.cone().to_bits(),
            gun.weapon_spec().aim_spread.to_bits(),
            "the cone is the row's aimed end, no bloom, no move share"
        );
    }

    #[test]
    fn the_dodge_window_comes_before_the_guard() {
        let mut actor = guarded(Weapon::Blade, 40);
        actor.action = Action::Dodge;
        actor.age = 0.15;
        assert!(actor.invulnerable());
        assert!(actor.hit(ahead(40., 30., BlowKind::Cut)).is_none());
    }
    /// An actor with the rifle in hand and the pistol holstered.
    fn rifleman() -> Combatant {
        Combatant::default().carrying([Some(Weapon::Rifle), Some(Weapon::Pistol)], 0)
    }

    /// Step `actor` for `ticks` of `TICK` with the aim held or not, and
    /// anything else `edit` sets on the intent.
    fn aim_for(actor: &mut Combatant, aim: bool, ticks: u32, edit: impl Fn(&mut CombatIntent)) {
        let mut motion = CombatMotion::default();
        for _ in 0..ticks {
            let mut input = CombatIntent {
                aim,
                ..Default::default()
            };
            edit(&mut input);
            actor.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        }
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::float_cmp,
        reason = "a few dozen ticks; a share clamped to nought or one is exact"
    )]
    fn the_rifles_aim_closes_and_opens_over_its_aim_time() {
        let row = Weapon::Rifle.spec();
        assert!(row.aim_time > 0. && row.aim_spread < row.spread);
        let mut actor = rifleman();
        assert_eq!(actor.aim_share(), 0.);
        assert!((actor.cone() - row.spread).abs() < 1e-6, "the hip at first");
        // Half the time in, half closed, and the cone with it.
        let whole = (row.aim_time / TICK).round() as u32;
        assert_eq!(whole % 2, 0, "the check wants an even number of ticks");
        aim_for(&mut actor, true, whole / 2, |_| {});
        let share = actor.aim_share();
        assert!((share - 0.5).abs() < 0.02, "{share}");
        let want = row.spread + (row.aim_spread - row.spread) * share;
        assert!((actor.cone() - want).abs() < 1e-5);
        // The two ends stay what they were: the reticle blends them by the share.
        assert!((actor.cone_when(true) - row.aim_spread).abs() < 1e-6);
        assert!((actor.cone_when(false) - row.spread).abs() < 1e-6);
        // Closed after the whole time, and no further.
        aim_for(&mut actor, true, whole * 4, |_| {});
        assert_eq!(actor.aim_share(), 1.);
        assert!((actor.cone() - row.aim_spread).abs() < 1e-6);
        // Dropped, it opens by the same time.
        aim_for(&mut actor, false, whole / 2, |_| {});
        assert!(
            (actor.aim_share() - 0.5).abs() < 0.02,
            "{}",
            actor.aim_share()
        );
        assert!(actor.cone() > row.aim_spread && actor.cone() < row.spread);
        aim_for(&mut actor, false, whole * 4, |_| {});
        assert_eq!(actor.aim_share(), 0.);
        assert!((actor.cone() - row.spread).abs() < 1e-6);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "a share clamped to nought or one is exact"
    )]
    fn a_swap_a_reload_a_dodge_and_a_stagger_each_open_the_aim() {
        let closed = || {
            let mut actor = rifleman();
            aim_for(&mut actor, true, 60, |_| {});
            assert_eq!(actor.aim_share(), 1.);
            actor.ammo = 1;
            actor
        };
        let mut actor = closed();
        aim_for(&mut actor, true, 1, |input| input.reload = true);
        assert_eq!((actor.action, actor.aim_share()), (Action::Reload, 0.));
        let mut actor = closed();
        aim_for(&mut actor, true, 1, |input| input.weapon = Some(1));
        assert_eq!((actor.action, actor.aim_share()), (Action::Swap, 0.));
        let mut actor = closed();
        aim_for(&mut actor, true, 1, |input| input.dodge = true);
        assert_eq!((actor.action, actor.aim_share()), (Action::Dodge, 0.));
        let mut actor = closed();
        actor.hit(Blow::bare(0., 10_000.));
        assert_eq!(actor.action, Action::Stagger);
        assert_eq!(
            actor.aim_share(),
            0.,
            "the hit itself, before the next tick"
        );
        aim_for(&mut actor, true, 3, |_| {});
        assert_eq!(actor.aim_share(), 0., "and it stays open while staggered");
        // Back in a Ready hand with the aim held, it starts closing again.
        let mut actor = closed();
        aim_for(&mut actor, true, 1, |input| input.reload = true);
        aim_for(&mut actor, true, 240, |_| {});
        assert_eq!(actor.action, Action::Ready);
        assert_eq!(actor.aim_share(), 1.);
        // A weapon put in the hand from outside the swap starts open too.
        let mut actor = closed();
        actor.carry([Some(Weapon::Carbine), Some(Weapon::Pistol)]);
        actor.carry([Some(Weapon::Rifle), Some(Weapon::Pistol)]);
        assert_eq!(actor.weapon, Weapon::Rifle);
        assert_eq!(actor.aim_share(), 0.);
    }

    #[test]
    fn the_carbines_and_the_pistols_cones_step_with_the_aim_to_the_bit() {
        for gun in [Weapon::Carbine, Weapon::Pistol] {
            let row = gun.spec();
            assert_eq!(row.aim_time.to_bits(), 0_f32.to_bits());
            assert_eq!(row.zoom.to_bits(), 1_f32.to_bits());
            let mut actor = Combatant::default().carrying([Some(gun), None], 0);
            let mut motion = CombatMotion::default();
            for tick in 0..240_u32 {
                let mut input = CombatIntent {
                    aim: (tick / 40) % 2 == 1,
                    fire: tick % 50 < 5,
                    ..Default::default()
                };
                actor.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
                assert_eq!(
                    actor.cone().to_bits(),
                    actor.cone_when(actor.aiming).to_bits(),
                    "{gun:?} at {tick}"
                );
                let end = if actor.aiming {
                    row.aim_spread
                } else {
                    row.spread
                };
                assert_eq!(
                    actor.cone().to_bits(),
                    (end + actor.bloom() + row.move_spread * 0.).to_bits(),
                    "{gun:?} at {tick}"
                );
                assert_eq!(
                    actor.aim_share().to_bits(),
                    f32::from(u8::from(actor.aiming)).to_bits()
                );
            }
        }
    }

    #[test]
    fn the_rifles_shot_leaves_in_the_cone_the_aim_had_reached() {
        let row = Weapon::Rifle.spec();
        let mut actor = rifleman();
        aim_for(&mut actor, true, 25, |_| {});
        let share = actor.aim_share();
        assert!(share > 0.3 && share < 0.7, "{share}");
        aim_for(&mut actor, true, 1, |input| input.fire = true);
        assert!(actor.shots > 0);
        let want = row.spread + (row.aim_spread - row.spread) * actor.aim_share();
        assert!(
            (actor.shot_cone() - want).abs() < 1e-5,
            "no bloom of its own in it"
        );
        assert!(actor.shot_cone() > row.aim_spread && actor.shot_cone() < row.spread);
    }
}
