//! Active skills (ticket 0010, step 5): the table of what an ability is, the
//! bar a member chooses, and the rules a press and a change of the bar ask.
//!
//! An ability is a row of `assets/combat/abilities.ron`. A member knows the
//! abilities its learned boxes grant ([`known`]), keeps a [`Bar`] of them
//! (three places for each weapon class, two utility places that stay), and a
//! press of a place begins the ability or is refused with a reason
//! ([`can_use`], which a screen asks every frame to grey a place). The bar
//! changes by a [`BarRequest`] in the [`BarInbox`], consumed in
//! `SimSet::Input` and answered with a [`BarAnswer`]; [`can_slot`] is the
//! one rule the request and a screen both ask. The commitment, the cooldown
//! and the effects themselves are [`Combatant`]'s, which reads this module's
//! rows.
//!
//! Local-only like the sheet it sits beside: nothing here is replicated.

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use serde::{Deserialize, Serialize};
use strata_registry::progression::{Effect, Op, SkillGraph};

use crate::combat::{Action, CombatEncounter, Combatant, Weapon};
use crate::combat_tables::{self};
use crate::encounters::enemies;
use crate::progression::{CharacterSheet, learned_effects};
use crate::{LocalPlayer, Registries, SimSet, SimTick};

/// Someone a bolt's landing may strike besides its target, as the physics
/// finds them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate<T> {
    /// Who, in whatever the caller names actors by.
    pub who: T,
    /// Where the body is.
    pub at: Vec3,
    /// The side.
    pub team: u8,
    /// Not dead.
    pub alive: bool,
}

/// Which effect of a bolt struck someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplashKind {
    /// The burst at the first contact.
    Blast,
    /// The bolt that left again.
    Jump,
}

/// A further hit a bolt's landing causes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Splash<T> {
    /// Who is struck.
    pub who: T,
    /// Health damage of the blow.
    pub damage: f32,
    /// Posture damage of the blow.
    pub posture: f32,
    /// Which effect.
    pub kind: SplashKind,
}

/// Who else a bolt strikes where it first made contact, and with what: the
/// one rule the physics asks and decides nothing by. `team` is the side of
/// the one who fired it, `damage` and `posture` the bolt's at contact (a
/// blast's own figures already carry the shooter's scale, set where the bolt
/// is made), `struck` the target the bolt
/// landed on (none for the world, or a hit that was parried), `contact` the
/// point. A blast takes every living hostile within its radius and the one
/// struck directly, once each, never the user's side; a jump takes the
/// nearest living hostile other than the one struck, within its radius, with
/// its share of the bolt, and nobody when there is none.
#[must_use]
pub fn splash<T: Copy + PartialEq>(
    effects: &ShotEffects,
    team: u8,
    (damage, posture): (f32, f32),
    struck: Option<T>,
    contact: Vec3,
    others: &[Candidate<T>],
) -> Vec<Splash<T>> {
    let mut out = Vec::new();
    let hostile = |who: &&Candidate<T>| who.alive && enemies(who.team, team);
    if let Some(blast) = effects.blast {
        for who in others.iter().filter(hostile) {
            if Some(who.who) == struck || who.at.distance(contact) <= blast.radius {
                out.push(Splash {
                    who: who.who,
                    damage: blast.damage,
                    posture: blast.posture,
                    kind: SplashKind::Blast,
                });
            }
        }
    }
    if let Some(jump) = effects.jump
        && struck.is_some()
        && let Some(next) = others
            .iter()
            .filter(hostile)
            .filter(|who| Some(who.who) != struck && who.at.distance(contact) <= jump.radius)
            .min_by(|a, b| a.at.distance(contact).total_cmp(&b.at.distance(contact)))
    {
        out.push(Splash {
            who: next.who,
            damage: damage * jump.share,
            posture: posture * jump.share,
            kind: SplashKind::Jump,
        });
    }
    out
}

/// Which kind of place an ability goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    /// A place of the weapon class in hand.
    Weapon,
    /// A place that stays whatever is in hand.
    Utility,
}

/// Where the shot of a [`AbilityEffect::Shot`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShotCone {
    /// The weapon's aimed cone, whatever the stance.
    Aimed,
    /// No cone at all.
    Exact,
    /// The weapon's hip cone as it stands, with the bloom and the move share
    /// when the shot reads bloom.
    Hip,
}

/// A status a hit puts on whoever it lands on, unguarded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnHit {
    /// The status's id, a row of the same file.
    pub status: String,
    /// How long it lasts; one borne already starts over.
    pub seconds: f32,
}

/// A bolt that leaves again from the point of contact, once.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jump {
    /// Metres from the point of contact within which the nearest other
    /// living hostile is looked for.
    pub radius: f32,
    /// The share of the bolt's damage and posture the second hit carries.
    pub share: f32,
}

/// A burst at the bolt's first contact.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blast {
    /// Metres from the point of contact.
    pub radius: f32,
    /// Health damage to each living hostile within it.
    pub damage: f32,
    /// Posture damage to each.
    pub posture: f32,
}

/// Which way a [`AbilityEffect::Move`] carries its user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Along {
    /// The facing at the press.
    Aim,
    /// The direction being walked when the windup ends, the facing when
    /// standing.
    Input,
}

/// The strike a [`AbilityEffect::Move`] lands where the movement ends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThenStrike {
    /// Metres from the user, or a factor on the weapon's reach when scaled.
    pub reach: f32,
    /// Degrees either side of the facing.
    pub arc: f32,
    /// Health damage, or a factor on the light cut's when scaled.
    pub damage: f32,
    /// Posture damage, or a factor on the light cut's when scaled.
    pub posture: f32,
    /// Whether the figures are factors on the weapon in hand's own.
    #[serde(default)]
    pub scaled: bool,
}

/// How a [`AbilityEffect::Move`] covers its user: a bolt from within `arc`
/// degrees either side of the direction of the movement is met as a block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cover {
    /// Degrees either side of the direction of the movement.
    pub arc: f32,
    /// The share of the bolt's damage taken off; all of its posture is.
    pub share: f32,
}

/// What a bolt of an ability's shot carries beyond its damage, for the
/// physics to land. Nothing at all for a bolt a weapon fires on its own.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShotEffects {
    /// A status put on whoever the bolt hits and is not guarded against.
    pub on_hit: Option<OnHit>,
    /// A second hit on the nearest other hostile.
    pub jump: Option<Jump>,
    /// A burst at the first contact.
    pub blast: Option<Blast>,
    /// The bolt empties its target's shield before its damage is counted.
    pub breach: bool,
    /// Sent back by a guard: never sent back again.
    pub reflected: bool,
}

/// What an ability does to its user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Own {
    /// The magazine is full at once.
    Reload,
    /// The shield's wait before it recovers ends now.
    Shield,
    /// A status, for so many seconds.
    Status {
        /// The status's id, a row of the same file.
        id: String,
        /// How long it lasts; a status given again takes the longer time.
        seconds: f32,
    },
}

/// The one effect of an ability, from the fixed set the sim implements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AbilityEffect {
    /// Begin that existing cut by its own row; the ability's own stamina,
    /// windup, length, cancel and pace are nought, so the action's row is the
    /// cost and the commitment and only the cooldown is the ability's.
    Cut(Action),
    /// At the windup's end the weapon in hand fires `rounds` bolts, `gap`
    /// seconds apart, each taking a round from the magazine.
    Shot {
        /// Bolts fired.
        rounds: u8,
        /// Seconds between one bolt and the next.
        gap: f32,
        /// Which cone they leave in.
        cone: ShotCone,
        /// A factor on the weapon's own damage.
        damage: f32,
        /// A factor on the weapon's own posture damage.
        posture: f32,
        /// Whether the bolts read and add the weapon's bloom.
        bloom: bool,
        /// A status put on whoever a bolt hits and is not guarded against.
        #[serde(default)]
        on_hit: Option<OnHit>,
        /// On its first hit the bolt leaves again toward the nearest other
        /// hostile, once.
        #[serde(default)]
        jump: Option<Jump>,
        /// A burst at the point of first contact.
        #[serde(default)]
        blast: Option<Blast>,
        /// The bolt empties what is left of its target's shield first.
        #[serde(default)]
        breach: bool,
    },
    /// At the windup's end everything hostile within `reach` metres and
    /// `arc` degrees either side of the facing takes the blow once, as a
    /// cut does: it can be guarded and parried.
    Strike {
        /// Metres from the user.
        reach: f32,
        /// Degrees either side of the facing.
        arc: f32,
        /// Health damage of the blow.
        damage: f32,
        /// Posture damage of the blow.
        posture: f32,
        /// When true, `reach` is a factor on the weapon in hand's reach and
        /// `damage` and `posture` are factors on its class's light cut, so
        /// one row serves two classes.
        #[serde(default)]
        scaled: bool,
        /// A status put on what the blow lands on unguarded.
        #[serde(default)]
        on_hit: Option<OnHit>,
    },
    /// One attack made with whatever is in hand that puts a status on what
    /// it lands on: with a gun one bolt at the aimed cone, a round from the
    /// magazine, `damage` and `posture` factors on the weapon's own; with a
    /// melee weapon a scaled strike at the weapon's reach in a narrow arc,
    /// factors on the light cut.
    Hit {
        /// A factor on the weapon's health damage.
        damage: f32,
        /// A factor on the weapon's posture damage.
        posture: f32,
        /// The status put on, unless the guard met the hit.
        status: String,
        /// How long it lasts.
        seconds: f32,
    },
    /// From the windup's end the user is carried `distance` metres over
    /// `seconds`, as an action's travel is: a wall or a body stops it.
    Move {
        /// Metres.
        distance: f32,
        /// Seconds the movement takes.
        seconds: f32,
        /// Which way.
        along: Along,
        /// A strike landed where the movement ends or is stopped.
        #[serde(default)]
        then: Option<ThenStrike>,
        /// Bolts from ahead are met as blocks while the movement lasts.
        #[serde(default)]
        cover: Option<Cover>,
    },
    /// At the windup's end, on the user. With a length of nought it is
    /// instant: no commitment, nothing the user was doing is ended.
    Own(Own),
}

/// A row of `abilities.ron`: one active skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityRow {
    /// Snake case, unique.
    pub id: String,
    /// What a card calls it.
    pub name: String,
    /// A painted square of the icon catalogue, by id.
    pub icon: String,
    /// The one line a card shows.
    pub says: String,
    /// Which kind of place it goes in.
    pub slot: SlotKind,
    /// The weapon classes it works with; for a utility, the classes it needs
    /// in hand, empty for any.
    pub works_with: Vec<Weapon>,
    /// Stamina paid at the press.
    #[serde(default)]
    pub stamina: f32,
    /// Seconds before the press can be made again.
    #[serde(default)]
    pub cooldown: f32,
    /// Seconds from the press to the effect.
    #[serde(default)]
    pub windup: f32,
    /// Seconds of the whole commitment, at least the windup.
    #[serde(default)]
    pub length: f32,
    /// Seconds after which a dodge may cut it short.
    #[serde(default)]
    pub cancel: f32,
    /// The share of walking speed kept during it.
    #[serde(default)]
    pub pace: f32,
    /// Whether the trigger fires as usual during the commitment, so the
    /// slide keeps the weapon up.
    #[serde(default)]
    pub armed: bool,
    /// What it does.
    pub effect: AbilityEffect,
}

impl AbilityRow {
    /// A figure of this row by its name in a box's `Ability` effect, changed
    /// by `op`. `false`, and nothing changed, when the row's effect has no
    /// such figure (a cut's cost and timing are the action's, a status has
    /// no damage).
    pub fn modify(&mut self, field: &str, op: Op, value: f32) -> bool {
        let cut = matches!(self.effect, AbilityEffect::Cut(_));
        let slot: &mut f32 = match (field, &mut self.effect) {
            ("cooldown", _) => &mut self.cooldown,
            ("stamina", _) if !cut => &mut self.stamina,
            ("windup", _) if !cut => &mut self.windup,
            ("length", _) if !cut => &mut self.length,
            (
                "damage",
                AbilityEffect::Shot { damage, .. } | AbilityEffect::Strike { damage, .. },
            ) => damage,
            (
                "posture",
                AbilityEffect::Shot { posture, .. } | AbilityEffect::Strike { posture, .. },
            ) => posture,
            ("seconds", AbilityEffect::Own(Own::Status { seconds, .. })) => seconds,
            _ => return false,
        };
        *slot = op.apply(*slot, value);
        true
    }

    /// Whether the weapon class `weapon` may be used with it: any class when
    /// the list is empty.
    #[must_use]
    pub fn works_with(&self, weapon: Weapon) -> bool {
        self.works_with.is_empty() || self.works_with.contains(&weapon)
    }

    /// Whether the press does its work in the tick it is made and commits the
    /// user to nothing: an effect on the user with no length.
    #[must_use]
    pub fn is_instant(&self) -> bool {
        self.commitment() <= 0. && matches!(self.effect, AbilityEffect::Own(_))
    }

    /// The length of the commitment, never under the windup (a box may move
    /// either figure).
    #[must_use]
    pub fn commitment(&self) -> f32 {
        self.length.max(self.windup)
    }

    /// The figures of the row for a card, in the game's units: seconds,
    /// metres, degrees, whole per cent for a factor. Worked out here so no
    /// screen decides what a figure is.
    #[must_use]
    pub fn facts(&self) -> AbilityFacts {
        let percent = |factor: f32| factor * 100.;
        let mut figures = Vec::new();
        let (what, subject) = match &self.effect {
            AbilityEffect::Cut(action) => (EffectKind::Cut, Some(action.name().to_owned())),
            AbilityEffect::Shot {
                rounds,
                damage,
                posture,
                ..
            } => {
                figures.push((Figure::Rounds, f32::from(*rounds)));
                figures.push((Figure::DamagePercent, percent(*damage)));
                figures.push((Figure::PosturePercent, percent(*posture)));
                (EffectKind::Shot, None)
            }
            AbilityEffect::Strike {
                reach,
                arc,
                damage,
                posture,
                scaled,
                ..
            } => {
                if *scaled {
                    figures.push((Figure::ReachPercent, percent(*reach)));
                    figures.push((Figure::DamagePercent, percent(*damage)));
                    figures.push((Figure::PosturePercent, percent(*posture)));
                } else {
                    figures.push((Figure::Reach, *reach));
                    figures.push((Figure::Damage, *damage));
                    figures.push((Figure::Posture, *posture));
                }
                figures.push((Figure::Arc, *arc));
                (EffectKind::Strike, None)
            }
            AbilityEffect::Hit {
                damage,
                posture,
                status,
                seconds,
            } => {
                figures.push((Figure::DamagePercent, percent(*damage)));
                figures.push((Figure::PosturePercent, percent(*posture)));
                figures.push((Figure::Seconds, *seconds));
                (EffectKind::Hit, Some(status.clone()))
            }
            AbilityEffect::Move {
                distance, seconds, ..
            } => {
                figures.push((Figure::Distance, *distance));
                figures.push((Figure::Seconds, *seconds));
                (EffectKind::Move, None)
            }
            AbilityEffect::Own(Own::Reload) => (EffectKind::Reload, None),
            AbilityEffect::Own(Own::Shield) => (EffectKind::Shield, None),
            AbilityEffect::Own(Own::Status { id, seconds }) => {
                figures.push((Figure::Seconds, *seconds));
                (EffectKind::Status, Some(id.clone()))
            }
        };
        AbilityFacts {
            stamina: self.stamina,
            cooldown: self.cooldown,
            windup: self.windup,
            commitment: self.commitment(),
            what,
            subject,
            figures,
        }
    }
}

/// What an ability does, as a card names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// A cut of the melee weapon.
    Cut,
    /// Bolts from the gun in hand.
    Shot,
    /// A blow on everything in reach and arc.
    Strike,
    /// One attack that puts a status on.
    Hit,
    /// A movement.
    Move,
    /// The magazine made full.
    Reload,
    /// The shield recovers at once.
    Shield,
    /// A status on the user.
    Status,
}

/// A figure of an ability's effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Figure {
    /// Bolts fired.
    Rounds,
    /// Health damage in points.
    Damage,
    /// Health damage as per cent of the weapon's.
    DamagePercent,
    /// Posture damage in points.
    Posture,
    /// Posture damage as per cent of the weapon's.
    PosturePercent,
    /// Metres.
    Reach,
    /// Reach as per cent of the weapon's.
    ReachPercent,
    /// Degrees either side of the facing.
    Arc,
    /// Metres moved.
    Distance,
    /// Seconds the effect lasts or takes.
    Seconds,
}

/// An ability's figures for a card, from [`AbilityRow::facts`].
#[derive(Debug, Clone, PartialEq)]
pub struct AbilityFacts {
    /// Stamina paid at the press.
    pub stamina: f32,
    /// Seconds before it can be pressed again.
    pub cooldown: f32,
    /// Seconds from the press to the effect.
    pub windup: f32,
    /// Seconds the whole press commits the user for.
    pub commitment: f32,
    /// What it does.
    pub what: EffectKind,
    /// The cut's action or the status's id, when the effect names one.
    pub subject: Option<String>,
    /// The effect's figures, in the order a card words them.
    pub figures: Vec<(Figure, f32)>,
}

/// A row of the file's `statuses`: a name, an icon, a line, and what the sim
/// reads of it. Each field is absent when the status does nothing there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusRow {
    /// Snake case, unique.
    pub id: String,
    /// What a card calls it.
    pub name: String,
    /// A painted square of the icon catalogue, by id.
    pub icon: String,
    /// The one line a card shows.
    pub says: String,
    /// The bearer's cone is its aimed cone at its tightest, moving or not,
    /// aiming or not, with no bloom.
    #[serde(default)]
    pub tight_cone: bool,
    /// A factor on the posture damage the bearer takes.
    #[serde(default)]
    pub posture_taken: Option<f32>,
    /// A factor on the damage the bearer takes, read after any guard.
    #[serde(default)]
    pub damage_taken: Option<f32>,
    /// A share of the bearer's moving speed (walking; the travel of a cut or
    /// a dodge is not scaled).
    #[serde(default)]
    pub pace: Option<f32>,
    /// The bearer's shield does not recover while this lasts, and its wait
    /// starts over when it ends.
    #[serde(default)]
    pub shield_locked: bool,
    /// The bearer can begin no shot, no cut and no ability; it may still
    /// move, dodge, guard, reload and swap.
    #[serde(default)]
    pub disarmed: bool,
    /// The share of a bolt's damage and posture a guard that blocks or
    /// parries it sends back at whoever fired it.
    #[serde(default)]
    pub reflects: Option<f32>,
}

/// How many places a bar has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarSize {
    /// Places for each weapon class.
    pub weapon: usize,
    /// Places that stay.
    pub utility: usize,
}

/// What `assets/combat/abilities.ron` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityTable {
    /// See [`crate::combat_tables::ActionTable::version`].
    pub version: u32,
    /// The size of a bar.
    pub bar: BarSize,
    /// The arc in degrees of the strike a Hit makes with a melee weapon: a
    /// table figure, so no ability row has to repeat it.
    pub hit_arc: f32,
    /// The statuses.
    pub statuses: Vec<StatusRow>,
    /// The abilities, in the file's order.
    pub abilities: Vec<AbilityRow>,
}

impl AbilityTable {
    /// The ability called `id`.
    #[must_use]
    pub fn row(&self, id: &str) -> Option<&AbilityRow> {
        self.abilities.iter().find(|row| row.id == id)
    }

    /// The status called `id`.
    #[must_use]
    pub fn status(&self, id: &str) -> Option<&StatusRow> {
        self.statuses.iter().find(|row| row.id == id)
    }

    /// Whether every dial can be played: the first row that cannot, and why.
    /// The id is in the message, since a row is named by a string.
    ///
    /// # Errors
    /// The first thing wrong.
    pub fn check(&self) -> Result<(), String> {
        if self.bar.weapon == 0 {
            return Err("a bar has at least one weapon place".to_owned());
        }
        if !(self.hit_arc > 0. && self.hit_arc <= 180.) {
            return Err("hit_arc is an arc over 0 and up to 180 degrees".to_owned());
        }
        let snake = |id: &str| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        let mut seen = BTreeSet::new();
        for status in &self.statuses {
            if !snake(&status.id) {
                return Err(format!("status `{}` is not snake case", status.id));
            }
            if !seen.insert(status.id.as_str()) {
                return Err(format!("status `{}` is named twice", status.id));
            }
            if let Some(taken) = status.posture_taken
                && !(taken.is_finite() && taken >= 0.)
            {
                return Err(format!(
                    "status `{}`: posture_taken may not be negative",
                    status.id
                ));
            }
            if let Some(taken) = status.damage_taken
                && !(taken.is_finite() && taken >= 0.)
            {
                return Err(format!(
                    "status `{}`: damage_taken may not be negative",
                    status.id
                ));
            }
            if let Some(pace) = status.pace
                && !(pace.is_finite() && (0. ..=1.).contains(&pace))
            {
                return Err(format!(
                    "status `{}`: pace is a share of moving speed",
                    status.id
                ));
            }
            if let Some(share) = status.reflects
                && !(share.is_finite() && share > 0.)
            {
                return Err(format!("status `{}`: reflects is above nought", status.id));
            }
        }
        let mut seen = BTreeSet::new();
        for row in &self.abilities {
            if !snake(&row.id) {
                return Err(format!("ability `{}` is not snake case", row.id));
            }
            if !seen.insert(row.id.as_str()) {
                return Err(format!("ability `{}` is named twice", row.id));
            }
            self.check_row(row)
                .map_err(|message| format!("ability `{}`: {message}", row.id))?;
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm a kind of effect, each with its own figures to hold"
    )]
    fn check_row(&self, row: &AbilityRow) -> Result<(), String> {
        let figures = [
            row.stamina,
            row.cooldown,
            row.windup,
            row.length,
            row.cancel,
            row.pace,
        ];
        if figures.iter().any(|v| !(v.is_finite() && *v >= 0.)) {
            return Err("a figure may not be negative".to_owned());
        }
        if row.slot == SlotKind::Weapon && row.works_with.is_empty() {
            return Err("a weapon ability names the classes it works with".to_owned());
        }
        let unique: BTreeSet<_> = row.works_with.iter().collect();
        if unique.len() != row.works_with.len() {
            return Err("works_with names a class twice".to_owned());
        }
        match &row.effect {
            AbilityEffect::Cut(action) => {
                if !action.is_melee() {
                    return Err("a cut begins a melee action".to_owned());
                }
                if figures[0] != 0. || figures[2..].iter().any(|v| *v != 0.) {
                    return Err(
                        "a cut's cost and commitment are the action's row: stamina, windup, length, cancel and pace stay out"
                            .to_owned(),
                    );
                }
                if row.works_with.iter().any(|w| !w.is_melee()) {
                    return Err("a cut works with melee classes".to_owned());
                }
                return Ok(());
            }
            AbilityEffect::Shot {
                rounds,
                gap,
                damage,
                posture,
                on_hit,
                jump,
                blast,
                ..
            } => {
                self.check_on_hit(on_hit.as_ref())?;
                if let Some(jump) = jump
                    && !(jump.radius.is_finite()
                        && jump.radius > 0.
                        && jump.share.is_finite()
                        && jump.share > 0.)
                {
                    return Err("a jump's radius and share are above nought".to_owned());
                }
                if let Some(blast) = blast
                    && !(blast.radius.is_finite()
                        && blast.radius > 0.
                        && blast.damage.is_finite()
                        && blast.damage > 0.
                        && blast.posture.is_finite()
                        && blast.posture > 0.)
                {
                    return Err("a blast's radius, damage and posture are above nought".to_owned());
                }
                if *rounds == 0 {
                    return Err("a shot fires a round at least".to_owned());
                }
                if !(gap.is_finite() && *gap >= 0.) {
                    return Err("a shot's gap may not be negative".to_owned());
                }
                if !(damage.is_finite() && *damage > 0. && posture.is_finite() && *posture >= 0.) {
                    return Err(
                        "a shot's factors: damage above nought, posture not below".to_owned()
                    );
                }
                if row.works_with.iter().any(|w| w.is_melee()) {
                    return Err("a shot works with guns".to_owned());
                }
                let last = row.windup + gap * f32::from(rounds.saturating_sub(1));
                if last > row.length {
                    return Err("the last round leaves after the commitment ends".to_owned());
                }
            }
            AbilityEffect::Strike {
                reach,
                arc,
                damage,
                posture,
                scaled,
                on_hit,
            } => {
                Self::check_strike(
                    &ThenStrike {
                        reach: *reach,
                        arc: *arc,
                        damage: *damage,
                        posture: *posture,
                        scaled: *scaled,
                    },
                    row,
                )?;
                self.check_on_hit(on_hit.as_ref())?;
            }
            AbilityEffect::Hit {
                damage,
                posture,
                status,
                seconds,
            } => {
                self.check_on_hit(Some(&OnHit {
                    status: status.clone(),
                    seconds: *seconds,
                }))?;
                if !(damage.is_finite() && *damage > 0. && posture.is_finite() && *posture >= 0.) {
                    return Err(
                        "a hit's factors: damage above nought, posture not below".to_owned()
                    );
                }
            }
            AbilityEffect::Move {
                distance,
                seconds,
                then,
                cover,
                ..
            } => {
                if let Some(cover) = cover
                    && !(cover.arc.is_finite()
                        && (0. ..=180.).contains(&cover.arc)
                        && cover.share.is_finite()
                        && (0. ..=1.).contains(&cover.share))
                {
                    return Err("a cover's arc is 0 to 180 degrees, its share 0 to 1".to_owned());
                }
                if !(distance.is_finite() && *distance > 0. && seconds.is_finite() && *seconds > 0.)
                {
                    return Err("a move's distance and seconds are above nought".to_owned());
                }
                if row.windup + seconds > row.length + 1e-4 {
                    return Err(
                        "a move's seconds pass the length: windup plus seconds must fit".to_owned(),
                    );
                }
                if let Some(then) = then {
                    Self::check_strike(then, row)?;
                }
            }
            AbilityEffect::Own(own) => {
                if let Own::Status { id, seconds } = own {
                    if self.status(id).is_none() {
                        return Err(format!("names the status `{id}`, which has no row"));
                    }
                    if !(seconds.is_finite() && *seconds > 0.) {
                        return Err("a status lasts above nought seconds".to_owned());
                    }
                }
                if matches!(own, Own::Reload) && row.works_with.iter().any(|w| w.is_melee()) {
                    return Err("a reload works with guns".to_owned());
                }
            }
        }
        if row.length <= 0. && !matches!(row.effect, AbilityEffect::Own(_)) {
            return Err(
                "only an effect on the user may be instant, with a length of nought".to_owned(),
            );
        }
        if row.length < row.windup {
            return Err("the length is under the windup".to_owned());
        }
        if row.cancel > row.length {
            return Err("a dodge cancels within the length".to_owned());
        }
        if row.pace > 1. {
            return Err("pace is a share of walking speed".to_owned());
        }
        Ok(())
    }

    /// A status a hit puts on must be a row, for above nought seconds.
    fn check_on_hit(&self, on_hit: Option<&OnHit>) -> Result<(), String> {
        let Some(on_hit) = on_hit else {
            return Ok(());
        };
        if on_hit.status.is_empty() || self.status(&on_hit.status).is_none() {
            return Err(format!(
                "puts on the status `{}`, which has no row",
                on_hit.status
            ));
        }
        if !(on_hit.seconds.is_finite() && on_hit.seconds > 0.) {
            return Err("a status lasts above nought seconds".to_owned());
        }
        Ok(())
    }

    /// A strike's figures: reach and arc, a blow that is not negative, and
    /// for a scaled one a melee class and a reach factor of three at most.
    fn check_strike(strike: &ThenStrike, row: &AbilityRow) -> Result<(), String> {
        if !(strike.reach.is_finite() && strike.reach > 0.) {
            return Err("a strike's reach is above nought".to_owned());
        }
        if !(strike.arc.is_finite() && strike.arc > 0. && strike.arc <= 180.) {
            return Err("a strike's arc is above nought and at most 180 degrees".to_owned());
        }
        if !(strike.damage.is_finite()
            && strike.damage >= 0.
            && strike.posture.is_finite()
            && strike.posture >= 0.)
        {
            return Err("a strike's blow may not be negative".to_owned());
        }
        if strike.scaled {
            if strike.reach > 3. {
                return Err("a scaled strike's reach is a factor of three at most".to_owned());
            }
            if row.works_with.iter().any(|w| !w.is_melee()) {
                return Err("a scaled strike works with melee classes".to_owned());
            }
        }
        Ok(())
    }

    /// Whether every icon the table names is one `painted` says is a painted
    /// ability square.
    ///
    /// # Errors
    /// The first row whose icon is not.
    pub fn check_icons(&self, painted: impl Fn(&str) -> bool) -> Result<(), String> {
        let icons = self
            .abilities
            .iter()
            .map(|row| (row.id.as_str(), row.icon.as_str()))
            .chain(
                self.statuses
                    .iter()
                    .map(|row| (row.id.as_str(), row.icon.as_str())),
            );
        for (id, icon) in icons {
            if !painted(icon) {
                return Err(format!("`{id}`: `{icon}` is no painted ability square"));
            }
        }
        Ok(())
    }
}

/// Where an ability sits on a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Place {
    /// A place of one weapon class.
    Weapon(Weapon, usize),
    /// A utility place.
    Utility(usize),
}

impl Place {
    /// The place a press of `index` means with `weapon` in hand: the first
    /// `bar.weapon` indices are the class's places, the next are the
    /// utility's.
    #[must_use]
    pub fn of_press(index: usize, weapon: Weapon, size: BarSize) -> Option<Self> {
        if index < size.weapon {
            Some(Self::Weapon(weapon, index))
        } else if index - size.weapon < size.utility {
            Some(Self::Utility(index - size.weapon))
        } else {
            None
        }
    }
}

/// A member's bar: for each weapon class its places, and the utility places,
/// each empty or an ability id, beside what the member knew when it was last
/// held to the table (so a newly learned ability is placed once, and one the
/// member cleared stays cleared).
#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    /// A class's places, as many as the table's bar has once held to it.
    #[serde(default)]
    pub weapon: BTreeMap<Weapon, Vec<Option<String>>>,
    /// The utility places.
    #[serde(default)]
    pub utility: Vec<Option<String>>,
    /// The abilities known when the bar was last held to the table.
    #[serde(default)]
    pub known: BTreeSet<String>,
    /// Abilities known without a box that grants them: a still's or a
    /// headless run's `--grant`, which has no box to learn yet. Empty in a
    /// played household.
    #[serde(default)]
    pub granted: BTreeSet<String>,
}

impl Bar {
    /// The ability at `place`, if there is one.
    #[must_use]
    pub fn get(&self, place: Place) -> Option<&str> {
        match place {
            Place::Weapon(weapon, at) => self.weapon.get(&weapon)?.get(at)?.as_deref(),
            Place::Utility(at) => self.utility.get(at)?.as_deref(),
        }
    }

    /// Every place that holds `id`, in the bar's order.
    #[must_use]
    pub fn places_of(&self, id: &str) -> Vec<Place> {
        let mut found = Vec::new();
        for (weapon, places) in &self.weapon {
            for (at, held) in places.iter().enumerate() {
                if held.as_deref() == Some(id) {
                    found.push(Place::Weapon(*weapon, at));
                }
            }
        }
        for (at, held) in self.utility.iter().enumerate() {
            if held.as_deref() == Some(id) {
                found.push(Place::Utility(at));
            }
        }
        found
    }

    /// Put `value` at `place`, growing the vector to reach it.
    fn put(&mut self, place: Place, value: Option<String>) {
        let places = match place {
            Place::Weapon(weapon, _) => self.weapon.entry(weapon).or_default(),
            Place::Utility(_) => &mut self.utility,
        };
        let at = match place {
            Place::Weapon(_, at) | Place::Utility(at) => at,
        };
        if places.len() <= at {
            places.resize(at + 1, None);
        }
        places[at] = value;
    }

    /// Put `id` at `place` unchecked; [`can_slot`] is the rule.
    pub fn set(&mut self, place: Place, id: &str) {
        self.put(place, Some(id.to_owned()));
    }

    /// Empty `place`.
    pub fn clear(&mut self, place: Place) {
        if self.get(place).is_some() {
            self.put(place, None);
        }
    }

    /// Hold the bar to what is known and to the table as it now is: an id no
    /// longer known, no longer a row, or in a place it may not go is dropped;
    /// an ability that became known since the last time goes into the first
    /// empty place it can go (each class it works with, or the utility
    /// places); the vectors take the table's sizes. The one place a learn, a
    /// forget and a save read-back go through.
    pub fn follow(&mut self, table: &AbilityTable, known: &BTreeSet<String>) {
        let size = table.bar;
        let keeps = |id: &str, place: Place| -> bool {
            known.contains(id) && table.row(id).is_some() && can_slot_kind(table, id, place).is_ok()
        };
        for weapon in Weapon::ALL {
            let mut places = self.weapon.remove(&weapon).unwrap_or_default();
            places.resize(size.weapon, None);
            for (at, held) in places.iter_mut().enumerate() {
                if held
                    .as_deref()
                    .is_some_and(|id| !keeps(id, Place::Weapon(weapon, at)))
                {
                    *held = None;
                }
            }
            self.weapon.insert(weapon, places);
        }
        self.utility.resize(size.utility, None);
        for at in 0..self.utility.len() {
            if self.utility[at]
                .as_deref()
                .is_some_and(|id| !keeps(id, Place::Utility(at)))
            {
                self.utility[at] = None;
            }
        }
        // A class holds an ability once.
        for places in self.weapon.values_mut() {
            let mut seen = BTreeSet::new();
            for held in places.iter_mut() {
                if let Some(id) = held.as_deref()
                    && !seen.insert(id.to_owned())
                {
                    *held = None;
                }
            }
        }
        let mut seen = BTreeSet::new();
        for held in &mut self.utility {
            if let Some(id) = held.as_deref()
                && !seen.insert(id.to_owned())
            {
                *held = None;
            }
        }
        // The newly known, in the table's order, so the result does not
        // depend on the order boxes were learned in.
        for row in &table.abilities {
            if !known.contains(&row.id) || self.known.contains(&row.id) {
                continue;
            }
            match row.slot {
                SlotKind::Weapon => {
                    for weapon in Weapon::ALL.into_iter().filter(|w| row.works_with(*w)) {
                        let places = self.weapon.entry(weapon).or_default();
                        if !places.iter().any(|held| held.as_deref() == Some(&row.id))
                            && let Some(at) = places.iter().position(Option::is_none)
                        {
                            places[at] = Some(row.id.clone());
                        }
                    }
                }
                SlotKind::Utility => {
                    if !self
                        .utility
                        .iter()
                        .any(|held| held.as_deref() == Some(&row.id))
                        && let Some(at) = self.utility.iter().position(Option::is_none)
                    {
                        self.utility[at] = Some(row.id.clone());
                    }
                }
            }
        }
        self.known.clone_from(known);
    }
}

/// Every ability the sheet's learned boxes grant.
#[must_use]
pub fn known(graph: &SkillGraph, sheet: &CharacterSheet) -> BTreeSet<String> {
    learned_effects(graph, sheet)
        .filter_map(|effect| match effect {
            Effect::Grant { ability } => Some(ability.clone()),
            _ => None,
        })
        .collect()
}

/// What a member knows: the boxes' grants and what was given by hand
/// (`--grant`). This is what a screen passes to [`can_slot`].
#[must_use]
pub fn known_to(graph: &SkillGraph, sheet: &CharacterSheet, bar: &Bar) -> BTreeSet<String> {
    let mut all = known(graph, sheet);
    all.extend(bar.granted.iter().cloned());
    all
}

/// The box that gives `id` to this member, for a screen's "taught by" line.
/// Several boxes can grant one ability, so a learned one wins; an ability
/// known only by hand has none (`None`), and one nobody knows names the first
/// box that would teach it.
#[must_use]
pub fn taught_by<'a>(
    graph: &'a SkillGraph,
    sheet: &CharacterSheet,
    bar: &Bar,
    id: &str,
) -> Option<&'a strata_registry::progression::BoxDef> {
    let grants = |b: &&strata_registry::progression::BoxDef| {
        b.effects
            .iter()
            .any(|effect| matches!(effect, Effect::Grant { ability } if ability == id))
    };
    if let Some(learned) = graph
        .boxes()
        .filter(grants)
        .find(|b| sheet.learned.contains(&b.id))
    {
        return Some(learned);
    }
    if bar.granted.contains(id) {
        return None;
    }
    graph.boxes().find(grants)
}

/// An ability a box grants that the member has not learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlearned {
    /// The ability's id.
    pub ability: String,
    /// The first box that grants it.
    pub box_id: String,
    /// That box's name.
    pub box_name: String,
    /// The tree it is in, by name.
    pub tree_name: String,
}

/// The abilities the trees on the sheet still teach: one a box grants that
/// the member does not know, with the first box that would give it, in the
/// trees' order. A screen lists these so a player sees what is left to
/// earn, not only what is held.
#[must_use]
pub fn unlearned(graph: &SkillGraph, sheet: &CharacterSheet) -> Vec<Unlearned> {
    let held = known(graph, sheet);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for tree in graph.trees.iter().filter(|tree| tree.in_game()) {
        for b in tree.boxes() {
            for effect in &b.effects {
                let Effect::Grant { ability } = effect else {
                    continue;
                };
                if !held.contains(ability) && seen.insert(ability.clone()) {
                    out.push(Unlearned {
                        ability: ability.clone(),
                        box_id: b.id.clone(),
                        box_name: b.name.clone(),
                        tree_name: tree.name.clone(),
                    });
                }
            }
        }
    }
    out
}

/// Whether every `Grant` and every `Ability` of the graph names a row of the
/// table: where the two files meet.
///
/// # Errors
/// The first box that names an ability the table has not.
pub fn check_grants(graph: &SkillGraph, table: &AbilityTable) -> Result<(), String> {
    for b in graph.boxes() {
        for effect in &b.effects {
            let (Effect::Grant { ability: id } | Effect::Ability { id, .. }) = effect else {
                continue;
            };
            let Some(row) = table.row(id) else {
                return Err(format!(
                    "box `{}` names the ability `{id}`, which abilities.ron has no row for",
                    b.id
                ));
            };
            if let Effect::Ability {
                field, op, value, ..
            } = effect
                && !row.clone().modify(field, *op, *value)
            {
                return Err(format!(
                    "box `{}` moves `{field}` of `{id}`, which that ability has not",
                    b.id
                ));
            }
        }
    }
    Ok(())
}

/// Why an ability would not go in a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SlotRefusal {
    /// The bar has no such place.
    #[error("the bar has no such place")]
    NoPlace,
    /// The table has no such ability.
    #[error("there is no such ability")]
    Unknown,
    /// The member has not learned it.
    #[error("not learned")]
    NotKnown,
    /// A weapon ability for a utility place or the other way round.
    #[error("that goes in the other kind of place")]
    WrongKind,
    /// It does not work with that weapon class.
    #[error("it does not work with that weapon")]
    WrongWeapon,
    /// It is in another place of the same class already.
    #[error("it is in another place already")]
    Placed(Place),
}

/// The checks of [`can_slot`] that need no bar: the place exists, the
/// ability exists, the kind and the class fit.
fn can_slot_kind(table: &AbilityTable, ability: &str, place: Place) -> Result<(), SlotRefusal> {
    let in_range = match place {
        Place::Weapon(_, at) => at < table.bar.weapon,
        Place::Utility(at) => at < table.bar.utility,
    };
    if !in_range {
        return Err(SlotRefusal::NoPlace);
    }
    let row = table.row(ability).ok_or(SlotRefusal::Unknown)?;
    match (place, row.slot) {
        (Place::Weapon(weapon, _), SlotKind::Weapon) => {
            if row.works_with(weapon) {
                Ok(())
            } else {
                Err(SlotRefusal::WrongWeapon)
            }
        }
        (Place::Utility(_), SlotKind::Utility) => Ok(()),
        _ => Err(SlotRefusal::WrongKind),
    }
}

/// Whether `ability` may go in `place` of `bar`: the place exists, the
/// ability is a row and known, it is the right kind of ability for the place
/// and works with its class, and it is not in another place of the same
/// class. The one rule the request and a screen both ask.
///
/// # Errors
/// The first rule that refuses.
pub fn can_slot(
    table: &AbilityTable,
    known: &BTreeSet<String>,
    bar: &Bar,
    place: Place,
    ability: &str,
) -> Result<(), SlotRefusal> {
    can_slot_kind(table, ability, place)?;
    if !known.contains(ability) {
        return Err(SlotRefusal::NotKnown);
    }
    let same_class = |other: &Place| match (place, other) {
        (Place::Weapon(a, _), Place::Weapon(b, _)) => a == *b,
        (Place::Utility(_), Place::Utility(_)) => true,
        _ => false,
    };
    if let Some(other) = bar
        .places_of(ability)
        .into_iter()
        .find(|other| *other != place && same_class(other))
    {
        return Err(SlotRefusal::Placed(other));
    }
    Ok(())
}

/// Why a press of a place would not begin its ability.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum AbilityRefusal {
    /// Nothing in the place; said by nothing.
    #[error("nothing there")]
    Empty,
    /// The weapon in hand is not one it works with.
    #[error("not with this weapon")]
    WrongWeapon,
    /// Still cooling down, so many seconds more.
    #[error("cooling down for {0} s")]
    Cooldown(f32),
    /// Short of stamina by so much.
    #[error("short of stamina by {0}")]
    Stamina(f32),
    /// The magazine holds fewer rounds than it needs.
    #[error("not enough rounds")]
    NoRounds,
    /// It would change nothing: a reload of a full magazine, a recharge of a
    /// full shield.
    #[error("it would change nothing")]
    Nothing,
    /// Another commitment is under way that it cannot follow.
    #[error("busy")]
    Busy,
    /// A status borne forbids every shot, cut and ability.
    #[error("disarmed")]
    Disarmed,
}

/// What a press asks about the actor, copied out so the rule is pure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseState {
    /// The weapon class in hand.
    pub weapon: Weapon,
    /// Seconds left of this ability's cooldown.
    pub cooldown_left: f32,
    /// Stamina on hand.
    pub stamina: f32,
    /// Rounds in the magazine in hand.
    pub ammo: u16,
    /// What the magazine holds when full.
    pub magazine: u16,
    /// The shield now.
    pub shield: f32,
    /// What the shield holds when full.
    pub shield_capacity: f32,
    /// Another commitment is under way.
    pub busy: bool,
    /// The sheet certifies the class in hand. A cut asks its own gate for
    /// this; a shot or a strike would otherwise slip past it.
    pub certified: bool,
    /// A status borne forbids every shot, cut and ability.
    pub disarmed: bool,
}

/// What a press of `row` would do in `state`: nothing is changed.
///
/// # Errors
/// The first refusal, in the order wrong weapon, disarmed, cooldown, busy, stamina,
/// rounds, nothing to change.
pub fn can_use(row: &AbilityRow, state: &UseState) -> Result<(), AbilityRefusal> {
    if !row.works_with(state.weapon)
        || (!state.certified && !matches!(row.effect, AbilityEffect::Cut(_)))
    {
        return Err(AbilityRefusal::WrongWeapon);
    }
    if state.disarmed {
        return Err(AbilityRefusal::Disarmed);
    }
    if state.cooldown_left > 0. {
        return Err(AbilityRefusal::Cooldown(state.cooldown_left));
    }
    if state.busy {
        return Err(AbilityRefusal::Busy);
    }
    if state.stamina < row.stamina {
        return Err(AbilityRefusal::Stamina(row.stamina - state.stamina));
    }
    match &row.effect {
        AbilityEffect::Shot { rounds, .. } if state.ammo < u16::from(*rounds) => {
            Err(AbilityRefusal::NoRounds)
        }
        AbilityEffect::Hit { .. } if !state.weapon.is_melee() && state.ammo == 0 => {
            Err(AbilityRefusal::NoRounds)
        }
        AbilityEffect::Own(Own::Reload) if state.ammo >= state.magazine => {
            Err(AbilityRefusal::Nothing)
        }
        AbilityEffect::Own(Own::Shield) if state.shield >= state.shield_capacity => {
            Err(AbilityRefusal::Nothing)
        }
        _ => Ok(()),
    }
}

/// What a request or a screen asks of the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarRequest {
    /// Put an ability in a place; one that sits in another place of the
    /// class moves.
    Set {
        /// Where.
        place: Place,
        /// Which, by id.
        ability: String,
    },
    /// Empty a place.
    Clear {
        /// Which.
        place: Place,
    },
}

/// The requests not yet consumed.
#[derive(Resource, Debug, Default)]
pub struct BarInbox(pub Vec<BarRequest>);

/// Why a request changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BarRefusal {
    /// Not while fighting.
    #[error("not in a fight")]
    InFight,
    /// There is no player to hold a bar.
    #[error("no one to hold a bar")]
    NoPlayer,
    /// The rule of [`can_slot`] refused.
    #[error("{0}")]
    Slot(SlotRefusal),
}

/// The sim's answer to a [`BarRequest`].
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarAnswer {
    /// The place that changed, or why nothing did.
    pub result: Result<Place, BarRefusal>,
}

/// Apply one request to `bar`, as `known` and the table allow.
///
/// # Errors
/// What [`can_slot`] refuses; a clear of a place the bar has not refuses with
/// [`SlotRefusal::NoPlace`].
pub fn change(
    table: &AbilityTable,
    known: &BTreeSet<String>,
    bar: &mut Bar,
    request: &BarRequest,
) -> Result<Place, SlotRefusal> {
    match request {
        BarRequest::Set { place, ability } => {
            // An ability that sits in another place of the class moves: take
            // it out of there and ask the rule on what is left.
            let mut next = bar.clone();
            for other in next.places_of(ability) {
                let same = match (place, &other) {
                    (Place::Weapon(a, _), Place::Weapon(b, _)) => a == b,
                    (Place::Utility(_), Place::Utility(_)) => true,
                    _ => false,
                };
                if same && other != *place {
                    next.clear(other);
                }
            }
            can_slot(table, known, &next, *place, ability)?;
            next.set(*place, ability);
            *bar = next;
            Ok(*place)
        }
        BarRequest::Clear { place } => {
            let in_range = match place {
                Place::Weapon(_, at) => *at < table.bar.weapon,
                Place::Utility(at) => *at < table.bar.utility,
            };
            if !in_range {
                return Err(SlotRefusal::NoPlace);
            }
            bar.clear(*place);
            Ok(*place)
        }
    }
}

/// `SimTick`, `SimSet::Input`, after equip's: every request made since the
/// last tick, in order, each on the bar the one before left, refused while
/// the player is fighting.
pub fn consume(
    mut inbox: ResMut<BarInbox>,
    registries: Res<Registries>,
    encounter: Option<Res<CombatEncounter>>,
    mut players: Query<(&CharacterSheet, &mut Bar, &Combatant), With<LocalPlayer>>,
    mut answers: MessageWriter<BarAnswer>,
) {
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let engaged = encounter.is_some_and(|e| e.engaged && e.remaining > 0);
    let mut player = players.single_mut().ok();
    for request in requests {
        let result = match player.as_mut() {
            None => Err(BarRefusal::NoPlayer),
            Some((sheet, bar, actor)) => {
                if actor.in_combat(engaged) {
                    Err(BarRefusal::InFight)
                } else {
                    let known = known_to(&registries.0.progression, sheet, bar);
                    let mut next = (**bar).clone();
                    let done = combat_tables::with(|tables| {
                        change(&tables.abilities, &known, &mut next, &request)
                    })
                    .map_err(BarRefusal::Slot);
                    if done.is_ok() && **bar != next {
                        **bar = next;
                    }
                    done
                }
            }
        };
        answers.write(BarAnswer { result });
    }
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: a member whose sheet or
/// bar changed has its bar held to what the sheet grants (a learn places,
/// a forget takes out, a save read back is mended), and the combatant is
/// given the bar. The sheet is fitted wherever it changes; this follows it.
pub fn follow_sheet(
    registries: Res<Registries>,
    mut members: Query<
        (&CharacterSheet, &mut Bar, &mut Combatant),
        Or<(Changed<CharacterSheet>, Changed<Bar>)>,
    >,
) {
    for (sheet, mut bar, mut actor) in &mut members {
        let known = known_to(&registries.0.progression, sheet, &bar);
        let mut next = bar.clone();
        combat_tables::with(|tables| next.follow(&tables.abilities, &known));
        if *bar != next {
            *bar = next;
        }
        actor.set_bar(&bar);
    }
}

/// Registers the bar's request and consumes it in the tick. Added by
/// `SimPlugin`.
#[derive(Debug, Clone, Copy)]
pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BarInbox>()
            .add_message::<BarAnswer>()
            .add_systems(
                SimTick,
                (consume, follow_sheet)
                    .chain()
                    .in_set(SimSet::Input)
                    .after(crate::equip::consume),
            );
    }
}

#[cfg(test)]
mod effects_tests;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "a table figure and a factor on it are compared to the bit"
    )]

    use super::*;
    use crate::PlayerIntent;
    use crate::combat::{Blow, BlowKind, CombatIntent, CombatMotion, GuardOutcome, Refusal};
    use crate::combat_tables::CombatTables;
    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::math::Vec2;
    use bevy::math::Vec3;
    use std::sync::Arc;
    use strata_registry::registries::GameRegistries;

    pub(super) const ABILITIES: &str = include_str!("../../../assets/combat/abilities.ron");
    pub(super) const DT: f32 = 1. / 60.;

    pub(super) fn table() -> AbilityTable {
        CombatTables::shipped().abilities
    }

    #[test]
    fn every_row_words_its_figures_and_a_shot_is_in_per_cent() {
        let table = table();
        for row in &table.abilities {
            let facts = row.facts();
            assert!(
                (facts.cooldown - row.cooldown).abs() < f32::EPSILON,
                "{}",
                row.id
            );
            assert!(facts.commitment >= facts.windup, "{}", row.id);
        }
        let shot = table
            .abilities
            .iter()
            .find(|row| matches!(row.effect, AbilityEffect::Shot { .. }))
            .expect("a shot ability ships")
            .facts();
        assert_eq!(shot.what, EffectKind::Shot);
        assert_eq!(shot.figures[0].0, Figure::Rounds);
        assert!(
            shot.figures
                .iter()
                .any(|(f, _)| *f == Figure::DamagePercent)
        );
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn bar_with(places: &[(Place, &str)]) -> Bar {
        let mut bar = Bar::default();
        for (place, id) in places {
            bar.set(*place, id);
        }
        bar
    }

    pub(super) fn rig(actor: Combatant, places: &[(Place, &str)]) -> (Combatant, CombatIntent) {
        let mut actor = actor;
        actor.set_bar(&bar_with(places));
        (actor, CombatIntent::default())
    }

    /// One tick; `true` when a bolt left.
    pub(super) fn tick(actor: &mut Combatant, input: &mut CombatIntent) -> bool {
        tick_moving(actor, input, &PlayerIntent::default())
    }

    pub(super) fn tick_moving(
        actor: &mut Combatant,
        input: &mut CombatIntent,
        movement: &PlayerIntent,
    ) -> bool {
        let mut motion = CombatMotion::default();
        actor.step(input, movement, DT, &mut motion).1
    }

    pub(super) fn pistol() -> Combatant {
        Combatant::default().carrying([Some(Weapon::Pistol), Some(Weapon::Carbine)], 0)
    }

    pub(super) fn gun_place(weapon: Weapon, at: usize) -> Place {
        Place::Weapon(weapon, at)
    }

    #[test]
    fn the_shipped_table_passes_its_own_checks() {
        let table = table();
        table.check().unwrap();
        assert_eq!(table.abilities.len(), 19);
        assert_eq!(table.statuses.len(), 6);
    }

    #[test]
    fn a_table_that_cannot_be_played_is_refused() {
        let refused = |text: String, why: &str| {
            let table: AbilityTable = ron::from_str(&text).expect("still RON");
            table.check().expect_err(why)
        };
        // A cut with a cost of its own.
        let text = ABILITIES.replace(
            "            cooldown: 0.0,\n            effect: Cut(Heavy),",
            "            stamina: 5.0,\n            cooldown: 0.0,\n            effect: Cut(Heavy),",
        );
        assert_ne!(text, ABILITIES);
        assert!(refused(text, "cut cost").contains("action's row"));
        // A windup past the length.
        let text = ABILITIES.replace("windup: 0.12,", "windup: 0.9,");
        assert!(refused(text, "windup").contains("length"));
        // A status named twice.
        let text = ABILITIES.replace("id: \"fortified\"", "id: \"focused\"");
        assert!(refused(text, "status twice").contains("named twice"));
        // An ability named twice.
        let text = ABILITIES.replace("id: \"focus\"", "id: \"butt_strike\"");
        assert!(refused(text, "ability twice").contains("named twice"));
        // A status with no row.
        let text = ABILITIES.replace("Status(id: \"focused\"", "Status(id: \"dazed\"");
        assert!(refused(text, "no status").contains("no row"));
        // The last round leaves after the commitment.
        let text = ABILITIES.replace("gap: 0.08,", "gap: 0.3,");
        assert!(refused(text, "late round").contains("last round"));
        // A weapon ability with no class.
        let text = ABILITIES.replace("works_with: [Pistol],", "works_with: [],");
        assert!(refused(text, "no class").contains("classes"));
        // An icon that is no painted square.
        let table = table();
        let error = table
            .check_icons(|icon| icon != "veyra:icon.ability.focused")
            .expect_err("a symbol is no square");
        assert!(error.contains("focused"), "{error}");
        table.check_icons(|_| true).unwrap();
    }

    #[test]
    fn every_icon_is_a_painted_ability_square_of_the_catalogue() {
        let catalogue = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/catalogue.json"
        ))
        .expect("the icon catalogue is committed");
        let catalogue =
            strata_registry::IconCatalogue::parse(&catalogue).expect("the shipped catalogue loads");
        let painted = |icon: &str| catalogue.is_square(icon);
        table().check_icons(painted).unwrap();
        assert!(!painted("veyra:icon.weapon.pistol"), "a symbol is not one");
        assert!(!painted("veyra:icon.ability.no-such"));
    }

    #[test]
    fn can_slot_refuses_each_way() {
        let table = table();
        let known = set(&["heavy_cut", "butt_strike", "rapid_reload", "precision_shot"]);
        let mut bar = Bar::default();
        let ok = |bar: &Bar, place, id| can_slot(&table, &known, bar, place, id);
        assert_eq!(ok(&bar, gun_place(Weapon::Blade, 0), "heavy_cut"), Ok(()));
        assert_eq!(
            ok(&bar, gun_place(Weapon::Blade, 3), "heavy_cut"),
            Err(SlotRefusal::NoPlace)
        );
        assert_eq!(
            ok(&bar, Place::Utility(2), "rapid_reload"),
            Err(SlotRefusal::NoPlace)
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Blade, 0), "no_such"),
            Err(SlotRefusal::Unknown)
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Carbine, 0), "aimed_burst"),
            Err(SlotRefusal::NotKnown)
        );
        assert_eq!(
            ok(&bar, Place::Utility(0), "heavy_cut"),
            Err(SlotRefusal::WrongKind)
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Carbine, 0), "rapid_reload"),
            Err(SlotRefusal::WrongKind)
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Carbine, 0), "precision_shot"),
            Err(SlotRefusal::WrongWeapon)
        );
        bar.set(gun_place(Weapon::Pistol, 1), "precision_shot");
        assert_eq!(
            ok(&bar, gun_place(Weapon::Pistol, 0), "precision_shot"),
            Err(SlotRefusal::Placed(gun_place(Weapon::Pistol, 1)))
        );
        // The same place is not "another place"; another class has its own.
        assert_eq!(
            ok(&bar, gun_place(Weapon::Pistol, 1), "precision_shot"),
            Ok(())
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Blade, 0), "butt_strike"),
            Err(SlotRefusal::WrongWeapon)
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Pistol, 0), "butt_strike"),
            Ok(())
        );
        assert_eq!(
            ok(&bar, gun_place(Weapon::Carbine, 0), "butt_strike"),
            Ok(())
        );
    }

    #[test]
    fn setting_an_ability_that_sits_elsewhere_moves_it() {
        let table = table();
        let known = set(&["butt_strike"]);
        let mut bar = Bar::default();
        let at = |n| gun_place(Weapon::Carbine, n);
        let set_at = |n| BarRequest::Set {
            place: at(n),
            ability: "butt_strike".to_owned(),
        };
        assert_eq!(change(&table, &known, &mut bar, &set_at(0)), Ok(at(0)));
        assert_eq!(change(&table, &known, &mut bar, &set_at(2)), Ok(at(2)));
        assert_eq!(bar.get(at(0)), None);
        assert_eq!(bar.get(at(2)), Some("butt_strike"));
        assert_eq!(
            change(
                &table,
                &known,
                &mut bar,
                &BarRequest::Clear { place: at(2) }
            ),
            Ok(at(2))
        );
        assert_eq!(bar.get(at(2)), None);
        assert_eq!(
            change(
                &table,
                &known,
                &mut bar,
                &BarRequest::Clear { place: at(3) }
            ),
            Err(SlotRefusal::NoPlace)
        );
    }

    fn world_with(sheet: &[&str]) -> (World, Entity) {
        let mut world = World::new();
        let mut registries = GameRegistries::default();
        registries.progression = SkillGraph::shipped();
        world.insert_resource(Registries(Arc::new(registries)));
        world.init_resource::<BarInbox>();
        world.init_resource::<Messages<BarAnswer>>();
        let player = world
            .spawn((
                LocalPlayer,
                CharacterSheet {
                    learned: sheet.iter().map(|id| (*id).to_owned()).collect(),
                    ..CharacterSheet::default()
                },
                Combatant::default(),
            ))
            .id();
        (world, player)
    }

    fn answers(world: &mut World) -> Vec<BarAnswer> {
        world
            .resource_mut::<Messages<BarAnswer>>()
            .drain()
            .collect()
    }

    #[test]
    fn a_request_in_a_fight_is_refused_and_out_of_one_is_answered() {
        let (mut world, player) = world_with(&["blade.novice"]);
        world.run_system_once(follow_sheet).unwrap();
        let place = gun_place(Weapon::Glaive, 2);
        let ask = |world: &mut World| {
            world.resource_mut::<BarInbox>().0.push(BarRequest::Set {
                place,
                ability: "heavy_cut".to_owned(),
            });
            world.run_system_once(consume).unwrap();
            answers(world)
        };
        // The cut learned stands in the first place of each class it works with.
        let bar = world.get::<Bar>(player).unwrap();
        assert_eq!(bar.get(gun_place(Weapon::Glaive, 0)), Some("heavy_cut"));
        world.get_mut::<Combatant>(player).unwrap().action = Action::Light;
        assert_eq!(
            ask(&mut world),
            [BarAnswer {
                result: Err(BarRefusal::InFight)
            }]
        );
        assert_eq!(
            world.get::<Bar>(player).unwrap().get(place),
            None,
            "a refusal changes nothing"
        );
        world.get_mut::<Combatant>(player).unwrap().action = Action::Ready;
        assert_eq!(ask(&mut world), [BarAnswer { result: Ok(place) }]);
        let bar = world.get::<Bar>(player).unwrap();
        assert_eq!(bar.get(place), Some("heavy_cut"));
        assert_eq!(bar.get(gun_place(Weapon::Glaive, 0)), None, "it moved");
        // The encounter being engaged is a fight too.
        world.insert_resource(CombatEncounter {
            engaged: true,
            remaining: 1,
            ..CombatEncounter::default()
        });
        assert_eq!(
            ask(&mut world),
            [BarAnswer {
                result: Err(BarRefusal::InFight)
            }]
        );
        // A won fight is over: engaged stays set, nothing remains.
        world.insert_resource(CombatEncounter {
            engaged: true,
            remaining: 0,
            ..CombatEncounter::default()
        });
        assert_eq!(ask(&mut world), [BarAnswer { result: Ok(place) }]);
    }

    #[test]
    fn a_learned_grant_fills_the_first_empty_place_and_a_forgotten_one_leaves() {
        let table = table();
        let mut bar = Bar::default();
        // Something already in the first Blade place.
        bar.follow(&table, &set(&[]));
        bar.set(gun_place(Weapon::Blade, 0), "heavy_cut");
        let mut known = set(&["heavy_cut", "butt_strike", "rapid_reload"]);
        bar.follow(&table, &known);
        // The held place is kept; a newly known fills the first empty.
        assert_eq!(bar.get(gun_place(Weapon::Blade, 0)), Some("heavy_cut"));
        assert_eq!(bar.get(gun_place(Weapon::Carbine, 0)), Some("butt_strike"));
        assert_eq!(bar.get(gun_place(Weapon::Pistol, 0)), Some("butt_strike"));
        assert_eq!(bar.get(gun_place(Weapon::Rifle, 0)), Some("butt_strike"));
        assert_eq!(bar.get(Place::Utility(0)), Some("rapid_reload"));
        assert_eq!(bar.get(gun_place(Weapon::Blade, 1)), None);
        // A cleared place stays cleared: it is not new any more.
        bar.clear(gun_place(Weapon::Carbine, 0));
        bar.follow(&table, &known);
        assert_eq!(bar.get(gun_place(Weapon::Carbine, 0)), None);
        // A second newly known lands in the next empty place.
        known.insert("aimed_burst".to_owned());
        bar.follow(&table, &known);
        assert_eq!(bar.get(gun_place(Weapon::Carbine, 0)), Some("aimed_burst"));
        // Forgetting takes it out of every place.
        known.remove("butt_strike");
        bar.follow(&table, &known);
        assert!(bar.places_of("butt_strike").is_empty());
        assert_eq!(bar.get(gun_place(Weapon::Pistol, 0)), None);
    }

    #[test]
    fn learning_and_forgetting_the_shipped_box_goes_through_the_sheet() {
        let (mut world, player) = world_with(&[]);
        world.run_system_once(follow_sheet).unwrap();
        assert!(
            world
                .get::<Bar>(player)
                .unwrap()
                .places_of("heavy_cut")
                .is_empty()
        );
        world
            .get_mut::<CharacterSheet>(player)
            .unwrap()
            .learned
            .insert("blade.novice".to_owned());
        world.run_system_once(follow_sheet).unwrap();
        let places = world.get::<Bar>(player).unwrap().places_of("heavy_cut");
        assert_eq!(places.len(), 3, "one in each melee class: {places:?}");
        assert_eq!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .bar()
                .places_of("heavy_cut"),
            places,
            "and the combatant has it"
        );
        world
            .get_mut::<CharacterSheet>(player)
            .unwrap()
            .learned
            .clear();
        world.run_system_once(follow_sheet).unwrap();
        assert!(
            world
                .get::<Bar>(player)
                .unwrap()
                .places_of("heavy_cut")
                .is_empty()
        );
        assert!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .bar()
                .places_of("heavy_cut")
                .is_empty()
        );
    }

    #[test]
    fn a_saved_bar_is_held_to_what_is_known_and_to_the_table() {
        let table = table();
        let known = set(&["heavy_cut", "precision_shot"]);
        let saved = ron::to_string(&bar_with(&[
            (gun_place(Weapon::Blade, 0), "heavy_cut"),
            (gun_place(Weapon::Blade, 1), "heavy_cut"),
            (gun_place(Weapon::Pistol, 0), "precision_shot"),
            (gun_place(Weapon::Carbine, 0), "precision_shot"),
            (gun_place(Weapon::Rifle, 0), "butt_strike"),
            (gun_place(Weapon::Rifle, 1), "no_longer_a_row"),
            (Place::Utility(0), "heavy_cut"),
        ]))
        .unwrap();
        let mut bar: Bar = ron::from_str(&saved).unwrap();
        bar.known = known.clone();
        bar.follow(&table, &known);
        assert_eq!(bar.get(gun_place(Weapon::Blade, 0)), Some("heavy_cut"));
        assert_eq!(bar.get(gun_place(Weapon::Blade, 1)), None, "once a class");
        assert_eq!(
            bar.get(gun_place(Weapon::Pistol, 0)),
            Some("precision_shot")
        );
        assert_eq!(bar.get(gun_place(Weapon::Carbine, 0)), None, "wrong class");
        assert_eq!(bar.get(gun_place(Weapon::Rifle, 0)), None, "not known");
        assert_eq!(bar.get(gun_place(Weapon::Rifle, 1)), None, "no such row");
        assert_eq!(bar.get(Place::Utility(0)), None, "wrong kind");
        // A file with no bar reads as an empty one, and a bar of the wrong
        // size takes the table's.
        let bare: Bar = ron::from_str("()").unwrap();
        assert_eq!(bare, Bar::default());
        let mut long = Bar::default();
        long.weapon.insert(Weapon::Blade, vec![None; 9]);
        long.known = known.clone();
        long.follow(&table, &known);
        assert_eq!(long.weapon[&Weapon::Blade].len(), table.bar.weapon);
        assert_eq!(long.utility.len(), table.bar.utility);
    }

    fn state(row: &AbilityRow) -> UseState {
        UseState {
            weapon: row.works_with.first().copied().unwrap_or(Weapon::Carbine),
            cooldown_left: 0.,
            stamina: 100.,
            ammo: 10,
            magazine: 24,
            shield: 0.,
            shield_capacity: 50.,
            busy: false,
            certified: true,
            disarmed: false,
        }
    }

    #[test]
    fn can_use_refuses_each_way() {
        let table = table();
        let row = |id: &str| table.row(id).unwrap().clone();
        let precise = row("precision_shot");
        assert_eq!(can_use(&precise, &state(&precise)), Ok(()));
        let mut uncertified = state(&precise);
        uncertified.certified = false;
        assert_eq!(
            can_use(&precise, &uncertified),
            Err(AbilityRefusal::WrongWeapon),
            "a shot from a class the sheet does not certify"
        );
        let with = |f: &dyn Fn(&mut UseState)| {
            let mut s = state(&precise);
            f(&mut s);
            can_use(&precise, &s)
        };
        assert_eq!(
            with(&|s| s.weapon = Weapon::Carbine),
            Err(AbilityRefusal::WrongWeapon)
        );
        assert_eq!(
            with(&|s| s.cooldown_left = 3.5),
            Err(AbilityRefusal::Cooldown(3.5))
        );
        assert_eq!(with(&|s| s.busy = true), Err(AbilityRefusal::Busy));
        assert_eq!(
            with(&|s| s.stamina = 10.),
            Err(AbilityRefusal::Stamina(15.))
        );
        let burst = row("aimed_burst");
        let mut s = state(&burst);
        s.ammo = 4;
        assert_eq!(can_use(&burst, &s), Err(AbilityRefusal::NoRounds));
        s.ammo = 5;
        assert_eq!(can_use(&burst, &s), Ok(()));
        let reload = row("rapid_reload");
        let mut s = state(&reload);
        s.ammo = s.magazine;
        assert_eq!(can_use(&reload, &s), Err(AbilityRefusal::Nothing));
        let shield = row("shield_recharge");
        let mut s = state(&shield);
        s.shield = s.shield_capacity;
        assert_eq!(can_use(&shield, &s), Err(AbilityRefusal::Nothing));
        s.weapon = Weapon::Blade;
        s.shield = 0.;
        assert_eq!(can_use(&shield, &s), Ok(()), "a utility for any weapon");
        // Empty is the actor's: no row in the place.
        let actor = Combatant::default();
        assert_eq!(actor.can_use(0).unwrap_err(), AbilityRefusal::Empty);
        assert_eq!(actor.can_use(9).unwrap_err(), AbilityRefusal::Empty);
    }

    #[test]
    fn a_press_costs_stamina_starts_the_cooldown_and_commits() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
        );
        let stamina = actor.stamina;
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Ability);
        assert_eq!(actor.ability_in_use(), Some("butt_strike"));
        assert_eq!(actor.stamina, stamina - 20.);
        assert_eq!(actor.cooldown_left("butt_strike"), 6.);
        assert_eq!(input.ability, None, "the edge was taken");
        // The commitment is the row's length.
        for _ in 0..10 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.action, Action::Ability);
        for _ in 0..30 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.action, Action::Ready);
        // On cooldown: refused, once, with the seconds left.
        let refusals = actor.refusals;
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(actor.refusals, refusals + 1);
        let Some(Refusal::Ability(AbilityRefusal::Cooldown(left))) = actor.refused else {
            panic!("expected a cooldown, got {:?}", actor.refused);
        };
        assert!(left > 5. && left < 6., "{left}");
        assert_eq!(actor.action, Action::Ready);
    }

    #[test]
    fn a_short_pool_is_refused_with_how_short() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
        );
        actor.stamina = 5.;
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        let Some(Refusal::Ability(AbilityRefusal::Stamina(short))) = actor.refused else {
            panic!("expected stamina, got {:?}", actor.refused);
        };
        assert!((short - 15.).abs() < 1., "{short}");
        assert_eq!(actor.action, Action::Ready);
        assert_eq!(actor.cooldown_left("butt_strike"), 0.);
    }

    #[test]
    fn a_cooldown_survives_a_swap() {
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Carbine), Some(Weapon::Pistol)], 0);
        actor.set_bar(&bar_with(&[
            (gun_place(Weapon::Carbine, 0), "butt_strike"),
            (gun_place(Weapon::Pistol, 0), "butt_strike"),
        ]));
        let mut input = CombatIntent {
            ability: Some(0),
            ..CombatIntent::default()
        };
        tick(&mut actor, &mut input);
        for _ in 0..40 {
            tick(&mut actor, &mut input);
        }
        input.weapon = Some(1);
        for _ in 0..120 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.weapon, Weapon::Pistol);
        let left = actor.cooldown_left("butt_strike");
        assert!(left > 0. && left < 6., "{left}");
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert!(matches!(
            actor.refused,
            Some(Refusal::Ability(AbilityRefusal::Cooldown(_)))
        ));
    }

    #[test]
    fn a_press_made_while_busy_is_kept_and_asked_again() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
        );
        input.dodge = true;
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Dodge);
        let length = actor.spec(Action::Dodge).duration;
        // Press with a few ticks of the dodge left, inside the buffer.
        while actor.age + 0.06 < length {
            tick(&mut actor, &mut input);
        }
        let refusals = actor.refusals;
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Dodge, "busy: not begun");
        assert_eq!(actor.refusals, refusals, "and not refused");
        for _ in 0..8 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.action, Action::Ability, "begun when the dodge ended");
        // A press that finds it busy past the buffer is dropped, unsaid.
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
        );
        input.dodge = true;
        tick(&mut actor, &mut input);
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        for _ in 0..80 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.action, Action::Ready);
        assert_eq!(actor.cooldown_left("butt_strike"), 0.);
    }

    #[test]
    fn an_ability_cut_short_before_its_windup_gives_nothing_and_keeps_its_cost() {
        let (mut actor, mut input) = rig(
            pistol(),
            &[(gun_place(Weapon::Pistol, 0), "precision_shot")],
        );
        let (stamina, ammo) = (actor.stamina, actor.ammo);
        input.ability = Some(0);
        let mut fired = tick(&mut actor, &mut input);
        assert_eq!(actor.stamina, stamina - 25., "paid at the press");
        for _ in 0..8 {
            fired |= tick(&mut actor, &mut input);
        }
        assert!(actor.age < 0.35 && !fired);
        actor.hit(Blow::bare(0., 10_000.));
        assert_eq!(actor.action, Action::Stagger);
        for _ in 0..90 {
            fired |= tick(&mut actor, &mut input);
        }
        assert!(!fired, "no bolt");
        assert_eq!(actor.ammo, ammo);
        assert!(
            actor.cooldown_left("precision_shot") > 0.,
            "and so does the cooldown"
        );
    }

    #[test]
    fn an_ability_whose_windup_has_passed_stands_when_a_dodge_ends_it() {
        let (mut actor, mut input) = rig(
            pistol(),
            &[(gun_place(Weapon::Pistol, 0), "precision_shot")],
        );
        let ammo = actor.ammo;
        input.ability = Some(0);
        let mut bolts = 0;
        for _ in 0..24 {
            bolts += u32::from(tick(&mut actor, &mut input));
        }
        assert_eq!(bolts, 1, "the bolt left at the windup's end");
        assert_eq!(actor.ammo, ammo - 1);
        // A dodge before the cancel point is not taken; after it, it ends it.
        input.dodge = true;
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Ability);
        while actor.age < 0.50 {
            tick(&mut actor, &mut input);
        }
        input.dodge = true;
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Dodge);
        assert_eq!(actor.ammo, ammo - 1, "what it gave stands");
    }

    #[test]
    fn a_precision_shot_is_one_exact_bolt_at_two_point_two() {
        let (mut actor, mut input) = rig(
            pistol(),
            &[(gun_place(Weapon::Pistol, 0), "precision_shot")],
        );
        let (ammo, spread) = (actor.ammo, actor.weapon_spec());
        assert!(spread.spread > 0.);
        input.ability = Some(0);
        let mut bolts = Vec::new();
        for _ in 0..60 {
            if tick(&mut actor, &mut input) {
                bolts.push((actor.shot_cone(), actor.shot_factors(), actor.age));
            }
        }
        assert_eq!(bolts.len(), 1);
        let (cone, factors, age) = bolts[0];
        assert_eq!(cone, 0., "no cone at all");
        assert_eq!(factors, (2.2, 1.5));
        assert!((age - 0.35).abs() < 0.02, "{age}");
        assert_eq!(actor.ammo, ammo - 1);
        assert_eq!(actor.bloom(), 0., "and no bloom");
        // The next plain round carries no factors.
        input.fire = true;
        let mut plain = false;
        for _ in 0..60 {
            plain |= tick(&mut actor, &mut input);
            if plain {
                break;
            }
        }
        assert!(plain);
        assert_eq!(actor.shot_factors(), (1., 1.));
        assert!(actor.bloom() > 0.);
    }

    #[test]
    fn an_aimed_burst_is_five_rounds_in_the_aimed_cone_with_no_bloom() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "aimed_burst")],
        );
        let ammo = actor.ammo;
        let aimed = actor.weapon_spec().aim_spread;
        input.ability = Some(0);
        let mut times = Vec::new();
        for _ in 0..90 {
            if tick(&mut actor, &mut input) {
                times.push(actor.age);
                assert_eq!(actor.shot_cone(), aimed);
                assert_eq!(actor.shot_factors(), (1., 1.));
            }
        }
        assert_eq!(times.len(), 5);
        assert_eq!(actor.ammo, ammo - 5);
        assert_eq!(actor.bloom(), 0.);
        for pair in times.windows(2) {
            assert!((pair[1] - pair[0] - 0.08).abs() < 0.02, "{times:?}");
        }
        // With fewer than five rounds it is refused and nothing is paid.
        let stamina = actor.stamina;
        actor.ammo = 4;
        for _ in 0..700 {
            tick(&mut actor, &mut input);
        }
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(
            actor.refused,
            Some(Refusal::Ability(AbilityRefusal::NoRounds))
        );
        assert_eq!(actor.ammo, 4);
        assert!(actor.stamina >= stamina - 1.);
    }

    #[test]
    fn a_butt_strike_lands_its_blow_once_at_the_windup() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
        );
        input.ability = Some(0);
        let mut landed = Vec::new();
        for _ in 0..60 {
            tick(&mut actor, &mut input);
            if let Some(blow) = actor.take_strike() {
                landed.push((actor.age, blow));
            }
        }
        assert_eq!(landed.len(), 1);
        let (age, blow) = landed[0].clone();
        assert!((age - 0.12).abs() < 0.02, "{age}");
        assert_eq!(
            (blow.reach, blow.arc, blow.damage, blow.posture),
            (2.2, 50., 8., 105.)
        );
    }

    #[test]
    fn a_rapid_reload_fills_the_magazine_and_a_full_one_is_refused() {
        let (mut actor, mut input) =
            rig(Combatant::default(), &[(Place::Utility(0), "rapid_reload")]);
        let full = actor.magazine();
        actor.ammo = 3;
        input.ability = Some(3);
        for _ in 0..5 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.ammo, 3, "not before the windup");
        for _ in 0..30 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.ammo, full);
        let stamina = actor.stamina;
        for _ in 0..1300 {
            tick(&mut actor, &mut input);
        }
        input.ability = Some(3);
        tick(&mut actor, &mut input);
        assert_eq!(
            actor.refused,
            Some(Refusal::Ability(AbilityRefusal::Nothing))
        );
        assert!(actor.stamina >= stamina);
        // A blade in hand has no magazine to fill.
        let (mut blade, _) = rig(
            Combatant::default().carrying([Some(Weapon::Blade), None], 0),
            &[(Place::Utility(0), "rapid_reload")],
        );
        assert_eq!(blade.can_use(3).unwrap_err(), AbilityRefusal::WrongWeapon);
        let mut press = CombatIntent {
            ability: Some(3),
            ..CombatIntent::default()
        };
        tick(&mut blade, &mut press);
        assert_eq!(
            blade.refused,
            Some(Refusal::Ability(AbilityRefusal::WrongWeapon))
        );
    }

    #[test]
    fn a_shield_recharge_ends_the_wait_and_a_full_shield_is_refused() {
        let run = |press: bool| {
            let (mut actor, mut input) = rig(
                Combatant::default(),
                &[(Place::Utility(0), "shield_recharge")],
            );
            let full = actor.shield;
            actor.hit(Blow::bare(20., 0.));
            let hurt = actor.shield;
            assert!(hurt < full);
            input.ability = press.then_some(3);
            for _ in 0..90 {
                tick(&mut actor, &mut input);
            }
            (hurt, actor.shield, full)
        };
        let (hurt, with, full) = run(true);
        let (_, without, _) = run(false);
        assert_eq!(without, hurt, "still waiting");
        assert!(with > hurt && with <= full, "recovering: {with}");
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(Place::Utility(0), "shield_recharge")],
        );
        input.ability = Some(3);
        tick(&mut actor, &mut input);
        assert_eq!(
            actor.refused,
            Some(Refusal::Ability(AbilityRefusal::Nothing))
        );
    }

    #[test]
    fn focus_closes_the_cone_while_moving_and_lapses() {
        let (mut actor, mut input) = rig(
            Combatant::default().carrying([Some(Weapon::Rifle), None], 0),
            &[(gun_place(Weapon::Rifle, 0), "focus")],
        );
        let row = actor.weapon_spec();
        let walking = PlayerIntent {
            move_dir: Vec2::new(0., 1.),
            ..PlayerIntent::default()
        };
        tick_moving(&mut actor, &mut input, &walking);
        assert!(actor.cone_when(false) > row.aim_spread, "moving, hip");
        input.ability = Some(0);
        for _ in 0..30 {
            tick_moving(&mut actor, &mut input, &walking);
        }
        let borne = actor.statuses().to_vec();
        assert_eq!(borne.len(), 1);
        assert_eq!(borne[0].0, "focused");
        assert!(borne[0].1 > 4.5 && borne[0].1 <= 5., "{borne:?}");
        assert_eq!(actor.cone_when(false), row.aim_spread, "tightest, moving");
        assert_eq!(actor.cone_when(true), row.aim_spread);
        assert_eq!(actor.cone(), row.aim_spread);
        for _ in 0..330 {
            tick_moving(&mut actor, &mut input, &walking);
        }
        assert!(actor.statuses().is_empty());
        assert!(actor.cone_when(false) > row.aim_spread);
    }

    #[test]
    fn an_instant_focus_applies_in_the_tick_of_its_press_and_never_busies_the_hand() {
        let (mut actor, mut input) = rig(
            Combatant::default().carrying([Some(Weapon::Rifle), None], 0),
            &[(gun_place(Weapon::Rifle, 0), "focus")],
        );
        assert!(actor.ability("focus").unwrap().is_instant());
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(actor.statuses()[0].0, "focused");
        assert!(actor.ability_in_use().is_none());
        assert_eq!(actor.action, Action::Ready);
    }

    #[test]
    fn fortified_halves_every_blow_and_leaves_other_bearers_whole() {
        let (mut actor, mut input) = rig(
            Combatant::default().carrying([Some(Weapon::Greatblade), None], 0),
            &[(gun_place(Weapon::Greatblade, 0), "kinetic_absorb")],
        );
        input.ability = Some(0);
        for _ in 0..30 {
            tick(&mut actor, &mut input);
        }
        let mut plain = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        let pool = |c: &Combatant| c.health + c.shield;
        let (before, plain_before) = (pool(&actor), pool(&plain));
        actor.hit(Blow::bare(20., 0.));
        plain.hit(Blow::bare(20., 0.));
        assert!((before - pool(&actor) - 10.).abs() < 1e-3);
        assert!((plain_before - pool(&plain) - 20.).abs() < 1e-3);
        // Behind a raised guard the share the guard lets through is halved too.
        let guard_held = |fortified: bool| {
            let mut c = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
            let mut held = CombatIntent {
                aim: true,
                ..Default::default()
            };
            for _ in 0..40 {
                tick(&mut c, &mut held);
            }
            c.shield = 0.;
            if fortified {
                c.give_status("fortified", 5.);
            }
            let before = c.health;
            let blow = Blow::new(20., 0., Vec3::new(0., 0., -4.), BlowKind::Cut);
            let dealt = c.hit(blow).expect("lands");
            assert_eq!(dealt.guard, GuardOutcome::Blocked);
            before - c.health
        };
        let (through_fortified, through_plain) = (guard_held(true), guard_held(false));
        assert!(through_plain > 0.);
        assert!(
            (through_fortified * 2. - through_plain).abs() < 1e-3,
            "{through_fortified} {through_plain}"
        );
    }

    #[test]
    fn kinetic_absorb_stops_posture_damage_and_a_status_given_again_takes_the_longer() {
        let (mut actor, mut input) = rig(
            Combatant::default().carrying([Some(Weapon::Greatblade), None], 0),
            &[(gun_place(Weapon::Greatblade, 0), "kinetic_absorb")],
        );
        input.ability = Some(0);
        for _ in 0..30 {
            tick(&mut actor, &mut input);
        }
        assert_eq!(actor.statuses()[0].0, "fortified");
        let posture = actor.posture;
        actor.hit(Blow::bare(1., 10_000.));
        assert_eq!(actor.posture, posture);
        assert_ne!(actor.action, Action::Stagger);
        actor.give_status("fortified", 1.);
        assert!(
            actor.statuses()[0].1 > 1.5,
            "the shorter did not replace it"
        );
        actor.give_status("fortified", 9.);
        assert_eq!(actor.statuses()[0].1, 9.);
        assert_eq!(actor.statuses().len(), 1);
        // Without it the same blow staggers.
        let mut plain = Combatant::default().carrying([Some(Weapon::Greatblade), None], 0);
        plain.hit(Blow::bare(1., 10_000.));
        assert_eq!(plain.action, Action::Stagger);
    }

    pub(super) fn sheet_of(ids: &[&str]) -> CharacterSheet {
        CharacterSheet {
            learned: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..CharacterSheet::default()
        }
    }

    #[test]
    fn a_boxs_ability_effect_moves_one_figure_and_a_cut_keeps_its_cost_out() {
        let mut graph = SkillGraph::shipped();
        graph.trees[0].novice.effects.extend([
            Effect::Ability {
                id: "precision_shot".to_owned(),
                field: "stamina".to_owned(),
                op: Op::Mul,
                value: 0.5,
            },
            Effect::Ability {
                id: "precision_shot".to_owned(),
                field: "damage".to_owned(),
                op: Op::Add,
                value: 0.3,
            },
            Effect::Ability {
                id: "heavy_cut".to_owned(),
                field: "stamina".to_owned(),
                op: Op::Set,
                value: 50.,
            },
            Effect::Ability {
                id: "heavy_cut".to_owned(),
                field: "cooldown".to_owned(),
                op: Op::Set,
                value: 4.,
            },
            Effect::Ability {
                id: "focus".to_owned(),
                field: "seconds".to_owned(),
                op: Op::Mul,
                value: 2.,
            },
        ]);
        let id = graph.trees[0].novice.id.clone();
        let mut actor = Combatant::default();
        let before = actor.ability("precision_shot").unwrap();
        assert_eq!(before.stamina, 25.);
        actor.fit(&graph, &sheet_of(&[&id]));
        let row = actor.ability("precision_shot").unwrap();
        assert_eq!(row.stamina, 12.5);
        let AbilityEffect::Shot { damage, .. } = row.effect else {
            panic!("a shot");
        };
        assert_eq!(damage, 2.2 + 0.3);
        let cut = actor.ability("heavy_cut").unwrap();
        assert_eq!((cut.stamina, cut.cooldown), (0., 4.));
        let AbilityEffect::Own(Own::Status { seconds, .. }) =
            actor.ability("focus").unwrap().effect
        else {
            panic!("a status");
        };
        assert_eq!(seconds, 10.);
        // Another member's rows are the table's.
        let mut other = Combatant::default();
        other.fit(&graph, &sheet_of(&[]));
        assert_eq!(other.ability("precision_shot").unwrap(), before);
    }

    #[test]
    fn a_cut_abilitys_cooldown_starts_when_the_cut_does() {
        let mut graph = SkillGraph::shipped();
        graph.trees[0].novice.effects.push(Effect::Ability {
            id: "heavy_cut".to_owned(),
            field: "cooldown".to_owned(),
            op: Op::Set,
            value: 4.,
        });
        let id = graph.trees[0].novice.id.clone();
        let make = |stamina: f32| {
            let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
            actor.fit(
                &graph,
                &sheet_of(&[
                    &id,
                    "blade.novice",
                    "blade.cuts.1",
                    "blade.cuts.2",
                    "blade.cuts.3",
                ]),
            );
            actor.stamina = stamina;
            rig(actor, &[(gun_place(Weapon::Blade, 0), "heavy_cut")])
        };
        let (mut spent, mut input) = make(0.);
        input.ability = Some(0);
        for _ in 0..30 {
            tick(&mut spent, &mut input);
        }
        assert_ne!(spent.action, Action::Heavy);
        assert_eq!(spent.cooldown_left("heavy_cut"), 0., "no cut, no cooldown");
        let (mut fresh, mut input) = make(100.);
        input.ability = Some(0);
        tick(&mut fresh, &mut input);
        assert_eq!(fresh.action, Action::Heavy);
        assert!(fresh.cooldown_left("heavy_cut") > 3.9);
    }

    #[test]
    fn the_heavy_cut_from_its_place_is_the_heavy_edge_to_the_bit() {
        let graph = SkillGraph::shipped();
        for (sheet, cuts) in [
            (None, true),
            (Some(sheet_of(&["marksman.novice"])), false),
            (Some(sheet_of(&["blade.novice"])), true),
            (
                Some(sheet_of(&[
                    "blade.novice",
                    "blade.cuts.1",
                    "blade.cuts.2",
                    "blade.cuts.3",
                ])),
                true,
            ),
        ] {
            let make = || {
                let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
                if let Some(sheet) = &sheet {
                    actor.fit(&graph, sheet);
                }
                actor
            };
            let (mut edge, mut edge_in) = (make(), CombatIntent::default());
            let (mut placed, mut placed_in) =
                rig(make(), &[(gun_place(Weapon::Blade, 0), "heavy_cut")]);
            let mut motions = [CombatMotion::default(), CombatMotion::default()];
            let mut moved = false;
            for step in 0..240 {
                if step == 5 || step == 150 {
                    edge_in.heavy = true;
                    placed_in.ability = Some(0);
                }
                let a = edge.step(&mut edge_in, &PlayerIntent::default(), DT, &mut motions[0]);
                let b = placed.step(
                    &mut placed_in,
                    &PlayerIntent::default(),
                    DT,
                    &mut motions[1],
                );
                assert_eq!(a, b, "step {step}");
                assert_eq!(edge.action, placed.action, "step {step}");
                assert_eq!(edge.age.to_bits(), placed.age.to_bits());
                assert_eq!(edge.stamina.to_bits(), placed.stamina.to_bits());
                assert_eq!(edge.sequence, placed.sequence);
                assert_eq!(edge.refused, placed.refused, "step {step}");
                assert_eq!(
                    motions[0].velocity.map(|v| v.to_array().map(f32::to_bits)),
                    motions[1].velocity.map(|v| v.to_array().map(f32::to_bits))
                );
                moved |= edge.action == Action::Heavy;
            }
            assert_eq!(moved, cuts, "a heavy cut began only where it is open");
            assert_eq!(
                edge.refused.is_some(),
                !cuts,
                "and the locked one said so, to the bit with the edge"
            );
        }
    }

    #[test]
    fn a_gun_in_hand_refuses_the_heavy_cut_by_its_weapon() {
        let (mut actor, mut input) = rig(
            Combatant::default(),
            &[(gun_place(Weapon::Carbine, 0), "heavy_cut")],
        );
        input.ability = Some(0);
        tick(&mut actor, &mut input);
        assert_eq!(
            actor.refused,
            Some(Refusal::Ability(AbilityRefusal::WrongWeapon))
        );
        assert_eq!(actor.action, Action::Ready);
    }

    #[test]
    fn every_grant_and_ability_of_the_trees_names_a_row() {
        let tables = CombatTables::shipped();
        let graph = SkillGraph::shipped();
        check_grants(&graph, &tables.abilities).unwrap();
        let mut broken = graph.clone();
        broken.trees[0].novice.effects.push(Effect::Grant {
            ability: "no_such_row".to_owned(),
        });
        let error = check_grants(&broken, &tables.abilities).unwrap_err();
        assert!(error.contains("no_such_row"), "{error}");
        let mut dead_field = graph.clone();
        dead_field.trees[0].novice.effects.push(Effect::Ability {
            id: "focus".to_owned(),
            field: "damage".to_owned(),
            op: Op::Mul,
            value: 2.,
        });
        let error = check_grants(&dead_field, &tables.abilities).unwrap_err();
        assert!(
            error.contains("focus") && error.contains("damage"),
            "{error}"
        );
    }

    #[test]
    fn unlearned_lists_what_the_trees_still_grant_and_drops_what_is_learned() {
        let graph = SkillGraph::shipped();
        let bare = unlearned(&graph, &CharacterSheet::default());
        let granted = known(&graph, &everything(&graph));
        assert_eq!(bare.len(), granted.len(), "a bare sheet lacks every grant");
        for row in &bare {
            let b = graph.find(&row.box_id).unwrap();
            assert_eq!(b.name, row.box_name);
            assert_eq!(graph.tree_of(&row.box_id).unwrap().name, row.tree_name);
        }
        assert!(unlearned(&graph, &everything(&graph)).is_empty());
        let first = bare[0].clone();
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert(first.box_id.clone());
        let after = unlearned(&graph, &sheet);
        assert!(after.iter().all(|row| row.ability != first.ability));
        assert_eq!(after.len(), bare.len() - 1);
    }

    fn everything(graph: &SkillGraph) -> CharacterSheet {
        let mut sheet = CharacterSheet::default();
        for b in graph.boxes() {
            sheet.learned.insert(b.id.clone());
        }
        sheet
    }

    #[test]
    fn a_member_with_every_guard_box_keeps_every_row_inside_the_tables_bounds() {
        let tables = CombatTables::shipped();
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        for b in graph.boxes() {
            sheet.learned.insert(b.id.clone());
        }
        let trained = crate::progression::apply(&tables, &graph, &sheet);
        assert!(!trained.guards.is_empty());
        for (weapon, guard) in &trained.guards {
            let name = weapon.name();
            assert!(guard.share > 0. && guard.share <= 1., "{name} share");
            assert!(guard.posture >= 0. && guard.posture <= 1., "{name} posture");
            assert!(guard.arc > 0. && guard.arc <= 180., "{name} arc");
            assert!(guard.stamina >= 0., "{name} stamina");
            assert!(guard.riposte >= 0., "{name} riposte");
            assert!(
                guard.parry >= 0. && guard.parry < tables.melee.parry_rest,
                "{name} parry"
            );
        }
    }

    #[test]
    fn every_ability_is_taught_by_a_box_of_a_tree_that_can_use_it() {
        use strata_registry::progression::GUARD_FIELDS;
        let tables = CombatTables::shipped();
        let graph = SkillGraph::shipped();
        let mut granted = BTreeSet::new();
        for tree in &graph.trees {
            // What the tree and the boxes it needs can certify.
            let mut weapons = BTreeSet::new();
            let needed = tree
                .prerequisites
                .iter()
                .filter_map(|id| graph.tree_of(id))
                .chain([tree]);
            for t in needed {
                for effect in t.boxes().flat_map(|b| b.effects.iter()) {
                    if let Effect::Certify { weapon } = effect
                        && let Some(weapon) = Weapon::from_name(weapon)
                    {
                        weapons.insert(weapon);
                    }
                }
            }
            for b in tree.boxes() {
                for effect in &b.effects {
                    match effect {
                        Effect::Grant { ability } => {
                            let row = tables.abilities.row(ability).expect("a row");
                            assert!(
                                row.works_with.is_empty()
                                    || row.works_with.iter().any(|w| weapons.contains(w)),
                                "{} grants {ability}, which works with no weapon of {}",
                                b.id,
                                tree.id
                            );
                            granted.insert(ability.clone());
                        }
                        Effect::Guard { field, .. } => {
                            assert!(GUARD_FIELDS.contains(&field.as_str()), "{}", b.id);
                        }
                        _ => {}
                    }
                }
            }
        }
        for row in &tables.abilities.abilities {
            assert!(granted.contains(&row.id), "no box teaches {}", row.id);
        }
        assert_eq!(granted.len(), 19);
        // The disarm is the pistol's and the blade's; the armour break the
        // rifle's and the greatblade's.
        let teaches = |ability: &str| -> BTreeSet<&str> {
            graph
                .trees
                .iter()
                .filter(|t| {
                    t.boxes().any(|b| {
                        b.effects
                            .iter()
                            .any(|e| matches!(e, Effect::Grant { ability: a } if a == ability))
                    })
                })
                .map(|t| t.id.as_str())
                .collect()
        };
        assert_eq!(teaches("disarm"), BTreeSet::from(["marksman", "blade"]));
        assert_eq!(teaches("armour_break"), BTreeSet::from(["blade", "ranger"]));
    }
}
