//! Every combat tunable, read from `assets/combat/*.ron` and never from a
//! literal in code, so a balance change is a diff of a data file and the
//! duel harness can sweep a grid of them over the one model.
//!
//! The tables live in a process-wide slot ([`install`], [`current`]) rather
//! than in a Bevy resource or a parameter threaded through every call.
//! That is a trade: a global is the only thing that keeps
//! [`Combatant::step`](crate::combat::Combatant::step), `Action::spec` and
//! `Weapon::spec` plain functions with no world behind them, which is what
//! the balance harness depends on (progression design, section 7), and it
//! lets the view, the physics adapter and the sim read the same numbers
//! with no plumbing. The cost is that a swap is for the whole process: the
//! harness installs one grid point, runs its bouts, then the next, and
//! never runs two points at once on separate threads.
//!
//! A binary that never calls [`install`] runs on the shipped files, which
//! are compiled in, so a test or a tool without an assets directory still
//! has the numbers. The server and the client load the same directory at
//! startup and fold [`CombatTables::version`] into the replication
//! handshake, so a client on other tables is refused and told why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use strata_registry::AssetSource;

use crate::ability::AbilityTable;
use crate::combat::{Action, ActionSpec, FireMode, Weapon, WeaponSpec};
use crate::combat_tiers::TierTable;

/// What `assets/combat/actions.ron` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionTable {
    /// Bumped by hand when the meaning of a row changes; the handshake
    /// hashes the contents as well, so a changed number alone is refused.
    pub version: u32,
    /// One row per [`Action`], every variant present.
    pub actions: BTreeMap<Action, ActionSpec>,
}

impl ActionTable {
    /// The row for `action`. The loader refused a table missing one, so
    /// this cannot fail on an installed table.
    #[must_use]
    pub fn spec(&self, action: Action) -> ActionSpec {
        self.actions
            .get(&action)
            .copied()
            .unwrap_or(ActionSpec::NONE)
    }
}

/// What `assets/combat/weapons.ron` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeaponTable {
    /// See [`ActionTable::version`].
    pub version: u32,
    /// One row per [`Weapon`], every variant present.
    pub weapons: BTreeMap<Weapon, WeaponSpec>,
}

impl WeaponTable {
    /// The row for `weapon`; see [`ActionTable::spec`]. A melee class has
    /// no row here and reads [`WeaponSpec::MELEE`], so a fitted factor on
    /// its damage has a one to multiply.
    #[must_use]
    pub fn spec(&self, weapon: Weapon) -> WeaponSpec {
        self.weapons
            .get(&weapon)
            .copied()
            .unwrap_or(WeaponSpec::MELEE)
    }
}

/// How a melee class changes one family of cuts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CutScale {
    /// On the row's contact start, contact end, duration and dodge cancel.
    pub time: f32,
    /// On its damage.
    pub damage: f32,
    /// On its posture damage.
    pub posture: f32,
    /// On its stamina cost.
    pub stamina: f32,
    /// On the distance it carries the holder.
    pub travel: f32,
}

impl CutScale {
    fn factors(&self) -> [f32; 5] {
        [
            self.time,
            self.damage,
            self.posture,
            self.stamina,
            self.travel,
        ]
    }
}

/// A melee class's row of `assets/combat/melee.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeleeClass {
    /// On how far along and beyond the baked blade the sweep reaches.
    pub reach: f32,
    /// On the Light and the Followup.
    pub light: CutScale,
    /// On the Heavy.
    pub heavy: CutScale,
    /// The share of incoming posture damage taken mid swing.
    pub swing_posture: f32,
    /// What the aim button does with this class in hand.
    pub guard: GuardRow,
}

/// What a cut with a melee weapon does to a bearer whose hand holds a gun
/// and whose guard does not meet it: a gun is no guard.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Caught {
    /// A factor on the cut's posture damage.
    pub posture: f32,
    /// A factor on the cut's damage.
    pub damage: f32,
    /// A factor on the length of the stagger when the cut breaks the
    /// bearer: with nothing to turn the next cut, it is slower to recover.
    pub stagger: f32,
}

/// A melee class's guard, raised with the aim button.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GuardRow {
    /// The part of a hit's damage a guard takes off.
    pub share: f32,
    /// The half angle, in degrees either side of the facing, a hit must
    /// come from to meet the guard.
    pub arc: f32,
    /// Stamina paid a point of damage taken off.
    pub stamina: f32,
    /// The part of a hit's posture damage that still lands through.
    pub posture: f32,
    /// Seconds after the guard is raised in which a hit is parried.
    pub parry: f32,
    /// Posture damage a parried cut gives its attacker.
    pub riposte: f32,
    /// What the guard does against a bolt, where that is not what it does
    /// against a cut: a shield that holds fire and is spent by a blade.
    /// `None` is the row's own three figures for both, as every melee
    /// class ships.
    #[serde(default)]
    pub bolts: Option<BoltGuard>,
}

/// The three figures of a [`GuardRow`] that a bolt reads in its place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoltGuard {
    /// The part of a bolt's damage the guard takes off.
    pub share: f32,
    /// Stamina paid a point of damage taken off.
    pub stamina: f32,
    /// The part of a bolt's posture damage that still lands through.
    pub posture: f32,
}

impl GuardRow {
    /// The share, stamina cost and posture share a blow meets: the bolts'
    /// own figures for a bolt when the row has them, the row's otherwise.
    #[must_use]
    pub fn figures(&self, bolt: bool) -> (f32, f32, f32) {
        match self.bolts {
            Some(bolts) if bolt => (bolts.share, bolts.stamina, bolts.posture),
            _ => (self.share, self.stamina, self.posture),
        }
    }

    /// Whether the figures a row holds can be played: the bounds the
    /// melee table holds a class's guard to, for the three of a bolt too.
    ///
    /// # Errors
    /// What is wrong, in a few words.
    pub fn check(&self) -> Result<(), &'static str> {
        let rows = [(self.share, self.stamina, self.posture)]
            .into_iter()
            .chain(self.bolts.map(|b| (b.share, b.stamina, b.posture)));
        for (share, stamina, posture) in rows {
            if !(share.is_finite() && share > 0. && share <= 1.) {
                return Err("a guard's share is above nought and at most one");
            }
            if !(posture.is_finite() && (0. ..=1.).contains(&posture)) {
                return Err("a guard's posture is a share");
            }
            if !(stamina.is_finite() && stamina >= 0.) {
                return Err("a guard's stamina cost may not be negative");
            }
        }
        if !(self.arc.is_finite() && self.arc > 0. && self.arc <= 180.) {
            return Err("a guard's arc is above nought and at most 180 degrees");
        }
        for value in [self.parry, self.riposte] {
            if !(value.is_finite() && value >= 0.) {
                return Err("a guard's parry window and riposte may not be negative");
            }
        }
        Ok(())
    }

    /// The figure named `field` (one of the registry's `GUARD_FIELDS`), or
    /// `None` for a name the row has not got.
    pub fn field_mut(&mut self, field: &str) -> Option<&mut f32> {
        Some(match field {
            "share" => &mut self.share,
            "arc" => &mut self.arc,
            "stamina" => &mut self.stamina,
            "posture" => &mut self.posture,
            "parry" => &mut self.parry,
            "riposte" => &mut self.riposte,
            _ => return None,
        })
    }
}

impl BoltGuard {
    /// The figure named `field`, or `None` for a name the figures have not got.
    pub fn field_mut(&mut self, field: &str) -> Option<&mut f32> {
        Some(match field {
            "share" => &mut self.share,
            "stamina" => &mut self.stamina,
            "posture" => &mut self.posture,
            _ => return None,
        })
    }
}

impl MeleeClass {
    /// The factors for `action`: the light scale for the chain, the heavy
    /// one for the Heavy, none for anything that is not a cut.
    #[must_use]
    pub fn scale(&self, action: Action) -> Option<&CutScale> {
        match action {
            Action::Light | Action::Followup => Some(&self.light),
            Action::Heavy => Some(&self.heavy),
            _ => None,
        }
    }
}

/// What `assets/combat/melee.ron` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeleeTable {
    /// See [`ActionTable::version`].
    pub version: u32,
    /// Metres the baked sweep reaches from the hand.
    pub blade_reach_m: f32,
    /// Seconds a guard must have been down before raising it can parry
    /// again, so a hammered button is not a standing parry.
    pub parry_rest: f32,
    /// The share of stamina regeneration kept while the guard is up.
    pub guard_regen: f32,
    /// What an unguarded cut does to a bearer holding a gun.
    pub caught: Caught,
    /// One row per melee [`Weapon`], every class present and no gun.
    pub classes: BTreeMap<Weapon, MeleeClass>,
}

impl MeleeTable {
    /// The row for a melee `weapon`. The loader refused a table missing
    /// one; a gun has none.
    #[must_use]
    pub fn class(&self, weapon: Weapon) -> Option<MeleeClass> {
        self.classes.get(&weapon).copied()
    }

    /// The reach of `weapon` in metres, for a screen to say.
    #[must_use]
    pub fn reach_m(&self, weapon: Weapon) -> f32 {
        self.class(weapon)
            .map_or(0., |class| self.blade_reach_m * class.reach)
    }
}

/// What `assets/combat/actor.ron` holds: the pools, rates and scheduler
/// windows every combatant shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorTable {
    /// See [`ActionTable::version`].
    pub version: u32,
    /// Starting health.
    pub health: f32,
    /// Starting shield, and the default shield capacity.
    pub shield: f32,
    /// Starting stamina, and its cap.
    pub stamina: f32,
    /// Starting posture, its cap, and what a stagger resets it to.
    pub posture: f32,
    /// Stamina per second while Ready and past the recovery wait.
    pub stamina_regen: f32,
    /// Shield per second once the hurt wait has passed.
    pub shield_regen: f32,
    /// Posture per second once the hurt wait has passed.
    pub posture_regen: f32,
    /// Seconds after an accepted action before stamina regenerates.
    pub recovery_wait: f32,
    /// Seconds after a confirmed hit before shield and posture regenerate.
    pub hurt_wait: f32,
    /// Seconds one queued action survives.
    pub buffer: f32,
    /// Seconds after a Light in which a queued Light becomes a Followup.
    pub combo_window: f32,
    /// Locomotion multiplier while aiming, reloading or swapping.
    pub aim_speed_scale: f32,
    /// The player's motor while fighting, metres per second: here rather
    /// than in the game so the harness closes at the speed the game does.
    pub walk_speed: f32,
    /// Sprint, metres per second.
    pub sprint_speed: f32,
    /// Jump launch, metres per second.
    pub jump_speed: f32,
    /// Gravity on the player, metres per second squared.
    pub gravity: f32,
}

impl ActorTable {
    /// A field by its name in the file, for a progression effect to change;
    /// `version` is not a number a box may touch.
    #[must_use]
    pub fn field_mut(&mut self, field: &str) -> Option<&mut f32> {
        Some(match field {
            "health" => &mut self.health,
            "shield" => &mut self.shield,
            "stamina" => &mut self.stamina,
            "posture" => &mut self.posture,
            "stamina_regen" => &mut self.stamina_regen,
            "shield_regen" => &mut self.shield_regen,
            "posture_regen" => &mut self.posture_regen,
            "recovery_wait" => &mut self.recovery_wait,
            "hurt_wait" => &mut self.hurt_wait,
            "buffer" => &mut self.buffer,
            "combo_window" => &mut self.combo_window,
            "aim_speed_scale" => &mut self.aim_speed_scale,
            "walk_speed" => &mut self.walk_speed,
            "sprint_speed" => &mut self.sprint_speed,
            "jump_speed" => &mut self.jump_speed,
            "gravity" => &mut self.gravity,
            _ => return None,
        })
    }
}

/// The ranged opponent's numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentryTable {
    /// Its bolt speed, slower than the player's so a bolt can be sidestepped.
    pub bolt_speed: f32,
    /// How it fires, whatever the row it carries says: one slow bolt a charge.
    pub mode: FireMode,
    /// Seconds of visible charge before the shot releases.
    pub charge: f32,
    /// Seconds after a shot before the next charge.
    pub recover: f32,
    /// Seconds before its first action.
    pub cooldown: f32,
    /// Seconds it waits after a stagger.
    pub stagger_cooldown: f32,
    /// Metres per second.
    pub walk_speed: f32,
    /// It backs off inside this distance.
    pub hold_near: f32,
    /// It closes outside this distance.
    pub hold_far: f32,
    /// It fires only inside this distance.
    pub fire_range: f32,
}

/// The melee opponent's numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RusherTable {
    /// Metres per second.
    pub walk_speed: f32,
    /// It swings within this distance.
    pub reach: f32,
    /// Seconds between swings.
    pub cooldown: f32,
    /// Seconds before its first action.
    pub initial_cooldown: f32,
}

/// The long-range opponent's numbers: it holds far, aims a visible beam and
/// fires a fast bolt, so it can be read and avoided and punishes standing
/// in the open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarksmanTable {
    /// Its bolt speed, over the sentry's and under the rifle's own.
    pub bolt_speed: f32,
    /// Seconds from the start of an aim to the shot.
    pub charge: f32,
    /// The last seconds of the charge in which the aim no longer follows.
    pub lock: f32,
    /// Seconds after a shot before the next charge.
    pub recover: f32,
    /// Seconds before its first action.
    pub cooldown: f32,
    /// Seconds it waits after a stagger.
    pub stagger_cooldown: f32,
    /// Metres per second.
    pub walk_speed: f32,
    /// It backs off inside this distance.
    pub hold_near: f32,
    /// It closes outside this distance.
    pub hold_far: f32,
    /// It fires only inside this distance.
    pub fire_range: f32,
    /// Inside this it stops charging and walks straight away.
    pub flee: f32,
    /// The share of its walking speed it moves at when it closes or backs
    /// off with a gun on its target.
    pub pace: f32,
}

impl MarksmanTable {
    /// Whether a charge at `charge` (0 to 1, as `Combatant::charge` runs) is
    /// in its lock, where the aim no longer follows. The brain's own rule
    /// and the beam's colour are this one answer.
    #[must_use]
    pub fn locked(&self, charge: f32) -> bool {
        charge >= 1. - self.lock / self.charge
    }
}

/// The guarding melee opponent's numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuelistTable {
    /// Metres per second.
    pub walk_speed: f32,
    /// It attacks within this distance.
    pub reach: f32,
    /// Seconds between the end of one attack and the start of the next.
    pub cooldown: f32,
    /// Seconds before the first action.
    pub initial_cooldown: f32,
    /// Inside this distance it guards while it closes and between attacks.
    pub wary: f32,
    /// Seconds between seeing a slow cut begin and raising the guard it
    /// lowered to meet it.
    pub react: f32,
    /// Every this many attacks is a heavy; the others a light and its
    /// follow up.
    pub heavy_every: u32,
    /// Seconds the guard stays down after its own attack ends.
    pub rest: f32,
    /// Seconds its guard must have been down to parry when raised: its own
    /// training, in place of the player's `parry_rest`, so a guard dropped
    /// to read a slow cut is a parry when raised on time.
    pub parry_rest: f32,
    /// Seconds it waits after a stagger.
    pub stagger_cooldown: f32,
    /// Seconds beyond `react` an ability must take to land for it to count
    /// as a cut slow enough to meet.
    pub slow_margin: f32,
    /// It reads a slow cut from this many times its reach.
    pub read_reach: f32,
}

/// The shield bearer's numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulwarkTable {
    /// Metres per second it closes at with its guard up, at any distance.
    pub advance_speed: f32,
    /// It attacks within this distance.
    pub reach: f32,
    /// Seconds between the end of one attack and the start of the next.
    pub cooldown: f32,
    /// Seconds it stands in reach behind its guard before the first attack,
    /// counted from when it first gets there: the beat a blade has to put a
    /// cut on the slab.
    pub initial_cooldown: f32,
    /// Seconds after its own attack ends before it may raise the guard.
    pub rest: f32,
    /// Seconds after a stagger before it attacks, and before it has its
    /// guard back: the opening a broken guard earns.
    pub stagger_cooldown: f32,
    /// Seconds after a cut met its guard before it will lower it to attack:
    /// a blade working on the slab holds it behind it, which a bolt never
    /// does, and that is what a melee weapon has here that a gun has not.
    pub pinned: f32,
    /// Its guard, in place of the Blade's row of `melee.ron`: the only
    /// actor whose guard is not a class's.
    pub guard: GuardRow,
}

/// What `assets/combat/opponents.ron` holds: the encounter's opponents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpponentTable {
    /// See [`ActionTable::version`].
    pub version: u32,
    /// A hostile's health.
    pub health: f32,
    /// The passive training target's health.
    pub passive_health: f32,
    /// Every opponent's shield and shield capacity.
    pub shield: f32,
    /// Every opponent's damage multiplier.
    pub damage_scale: f32,
    /// Hostiles aggress once the player is this close to one.
    pub engage_distance: f32,
    /// The ranged opponent.
    pub sentry: SentryTable,
    /// The melee opponent.
    pub rusher: RusherTable,
    /// The long-range opponent.
    pub marksman: MarksmanTable,
    /// The guarding melee opponent.
    pub duelist: DuelistTable,
    /// The shield bearer.
    pub bulwark: BulwarkTable,
    /// Every opponent's sprint speed.
    pub sprint_speed: f32,
}

/// The four tables, parsed and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatTables {
    /// `actions.ron`.
    pub actions: ActionTable,
    /// `weapons.ron`.
    pub weapons: WeaponTable,
    /// `actor.ron`.
    pub actor: ActorTable,
    /// `opponents.ron`.
    pub opponents: OpponentTable,
    /// `melee.ron`.
    pub melee: MeleeTable,
    /// `abilities.ron`.
    pub abilities: AbilityTable,
    /// `tiers.ron`.
    pub tiers: TierTable,
}

/// Why a table could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum CombatTablesError {
    /// The file could not be read.
    #[error("combat table {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid RON for its table.
    #[error("combat table {file}: {message}")]
    Parse {
        /// Which of the four files.
        file: &'static str,
        /// The parser's message, with its position.
        message: String,
    },
    /// A per-variant table lacks a row.
    #[error("combat table {file} has no row for {row}")]
    Missing {
        /// Which of the four files.
        file: &'static str,
        /// The variant with no row.
        row: &'static str,
    },
    /// A row's dials cannot be played.
    #[error("combat table {file}: {row}: {message}")]
    Invalid {
        /// Which of the four files.
        file: &'static str,
        /// The variant whose row is wrong.
        row: &'static str,
        /// What is wrong with it.
        message: String,
    },
}

const ACTIONS: &str = include_str!("../../../assets/combat/actions.ron");
const WEAPONS: &str = include_str!("../../../assets/combat/weapons.ron");
const ACTOR: &str = include_str!("../../../assets/combat/actor.ron");
const OPPONENTS: &str = include_str!("../../../assets/combat/opponents.ron");
const MELEE: &str = include_str!("../../../assets/combat/melee.ron");
const ABILITIES: &str = include_str!("../../../assets/combat/abilities.ron");
const TIERS: &str = include_str!("../../../assets/combat/tiers.ron");

impl CombatTables {
    /// The file names under the directory [`CombatTables::load`] reads.
    pub const FILES: [&'static str; 7] = [
        "actions.ron",
        "weapons.ron",
        "actor.ron",
        "opponents.ron",
        "melee.ron",
        "abilities.ron",
        "tiers.ron",
    ];

    /// The tables as shipped in `assets/combat/`, compiled in.
    ///
    /// # Panics
    /// If the shipped files do not parse, which the sim's tests catch
    /// before a build ships.
    #[must_use]
    pub fn shipped() -> Self {
        Self::parse(ACTIONS, WEAPONS, ACTOR, OPPONENTS, MELEE, ABILITIES)
            .expect("the shipped combat tables parse")
    }

    /// Parse the first six files' text, in the order of
    /// [`CombatTables::FILES`], with the shipped `tiers.ron`: the form a
    /// test that varies one of the six takes.
    ///
    /// # Errors
    /// What [`CombatTables::parse_all`] refuses.
    pub fn parse(
        actions: &str,
        weapons: &str,
        actor: &str,
        opponents: &str,
        melee: &str,
        abilities: &str,
    ) -> Result<Self, CombatTablesError> {
        Self::parse_all(actions, weapons, actor, opponents, melee, abilities, TIERS)
    }

    /// Parse the seven files' text, in the order of [`CombatTables::FILES`].
    ///
    /// # Errors
    /// The first file that does not parse, or an action or weapon table
    /// missing a variant's row.
    pub fn parse_all(
        actions: &str,
        weapons: &str,
        actor: &str,
        opponents: &str,
        melee: &str,
        abilities: &str,
        tiers: &str,
    ) -> Result<Self, CombatTablesError> {
        let actions: ActionTable = parse_one(Self::FILES[0], actions)?;
        for action in Action::ALL {
            if !actions.actions.contains_key(&action) {
                return Err(CombatTablesError::Missing {
                    file: Self::FILES[0],
                    row: action.name(),
                });
            }
        }
        let weapons: WeaponTable = parse_one(Self::FILES[1], weapons)?;
        for weapon in Weapon::GUNS {
            if !weapons.weapons.contains_key(&weapon) {
                return Err(CombatTablesError::Missing {
                    file: Self::FILES[1],
                    row: weapon.name(),
                });
            }
        }
        for (weapon, spec) in &weapons.weapons {
            if weapon.is_melee() {
                return Err(CombatTablesError::Invalid {
                    file: Self::FILES[1],
                    row: weapon.name(),
                    message: "a melee class has its row in melee.ron".to_owned(),
                });
            }
            spec.check().map_err(|message| CombatTablesError::Invalid {
                file: Self::FILES[1],
                row: weapon.name(),
                message,
            })?;
        }
        let opponents: OpponentTable = parse_one(Self::FILES[3], opponents)?;
        opponents
            .bulwark
            .guard
            .check()
            .map_err(|message| CombatTablesError::Invalid {
                file: Self::FILES[3],
                row: "bulwark",
                message: message.to_owned(),
            })?;
        opponents
            .sentry
            .mode
            .check()
            .map_err(|message| CombatTablesError::Invalid {
                file: Self::FILES[3],
                row: "sentry",
                message,
            })?;
        let melee: MeleeTable = parse_one(Self::FILES[4], melee)?;
        check_melee(&melee)?;
        let abilities: AbilityTable = parse_one(Self::FILES[5], abilities)?;
        abilities
            .check()
            .map_err(|message| CombatTablesError::Invalid {
                file: Self::FILES[5],
                row: "abilities",
                message,
            })?;
        let tiers: TierTable = parse_one(Self::FILES[6], tiers)?;
        tiers
            .check()
            .map_err(|message| CombatTablesError::Invalid {
                file: Self::FILES[6],
                row: "tiers",
                message,
            })?;
        Ok(Self {
            actions,
            weapons,
            actor: parse_one(Self::FILES[2], actor)?,
            opponents,
            melee,
            abilities,
            tiers,
        })
    }

    /// Read the seven files from `dir` (the assets' `combat/` directory).
    ///
    /// # Errors
    /// A file that cannot be read, or what [`CombatTables::parse`] refuses.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, CombatTablesError> {
        let dir = dir.as_ref();
        Self::load_with(|name| {
            let path = dir.join(name);
            std::fs::read_to_string(&path)
                .map_err(|source| CombatTablesError::Read { path, source })
        })
    }

    /// Read the seven files from `combat/` of a source laid out as the
    /// assets directory, the installed one in the game.
    ///
    /// # Errors
    /// A file that cannot be read, or what [`CombatTables::parse`] refuses.
    pub fn load_from(assets: &dyn AssetSource) -> Result<Self, CombatTablesError> {
        Self::load_with(|name| {
            let path = format!("combat/{name}");
            crate::sites::read_text(assets, &path).map_err(|source| CombatTablesError::Read {
                path: PathBuf::from(path),
                source: std::io::Error::other(source.to_string()),
            })
        })
    }

    fn load_with(
        read: impl Fn(&str) -> Result<String, CombatTablesError>,
    ) -> Result<Self, CombatTablesError> {
        let [actions, weapons, actor, opponents, melee, abilities, tiers] = Self::FILES;
        Self::parse_all(
            &read(actions)?,
            &read(weapons)?,
            &read(actor)?,
            &read(opponents)?,
            &read(melee)?,
            &read(abilities)?,
            &read(tiers)?,
        )
    }

    /// A hash of the tables' versions and contents, for the
    /// replication handshake: two processes on the same numbers agree,
    /// and a changed number is a changed version whether or not the
    /// file's `version` was bumped.
    #[must_use]
    pub fn version(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut fold = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        };
        fold(b"scrolls combat\0");
        // The RON writer is deterministic for a given value, and the maps
        // are ordered, so the text is a canonical form of the numbers.
        let text = |value: &dyn erased::Value| value.ron();
        for (name, table) in [
            ("actions", &self.actions as &dyn erased::Value),
            ("weapons", &self.weapons),
            ("actor", &self.actor),
            ("opponents", &self.opponents),
            ("melee", &self.melee),
            ("abilities", &self.abilities),
            ("tiers", &self.tiers),
        ] {
            fold(name.as_bytes());
            fold(b"\0");
            fold(text(table).as_bytes());
        }
        hash
    }
}

/// Serialising each table through one trait object keeps the hash loop a
/// loop instead of four copies of it.
mod erased {
    use serde::Serialize;

    pub trait Value {
        fn ron(&self) -> String;
    }

    impl<T: Serialize> Value for T {
        fn ron(&self) -> String {
            ron::to_string(self).expect("a combat table serialises")
        }
    }
}

/// Every melee class present, no gun in the file, every factor above nought.
fn check_melee(table: &MeleeTable) -> Result<(), CombatTablesError> {
    let invalid = |row: &'static str, message: &str| CombatTablesError::Invalid {
        file: CombatTables::FILES[4],
        row,
        message: message.to_owned(),
    };
    if !(table.blade_reach_m.is_finite() && table.blade_reach_m > 0.) {
        return Err(invalid("blade_reach_m", "must be above nought"));
    }
    if !(table.parry_rest.is_finite() && table.parry_rest >= 0.) {
        return Err(invalid("parry_rest", "may not be negative"));
    }
    if !(table.guard_regen.is_finite() && (0. ..=1.).contains(&table.guard_regen)) {
        return Err(invalid("guard_regen", "is a share, nought to one"));
    }
    for factor in [
        table.caught.posture,
        table.caught.damage,
        table.caught.stagger,
    ] {
        if !(factor.is_finite() && factor >= 1.) {
            return Err(invalid("caught", "a factor of one at least"));
        }
    }
    for weapon in table.classes.keys() {
        if !weapon.is_melee() {
            return Err(invalid(weapon.name(), "a gun has its row in weapons.ron"));
        }
    }
    for weapon in Weapon::MELEE {
        let Some(class) = table.classes.get(&weapon) else {
            return Err(CombatTablesError::Missing {
                file: CombatTables::FILES[4],
                row: weapon.name(),
            });
        };
        let swing = class.swing_posture;
        let factors = [class.reach]
            .into_iter()
            .chain(class.light.factors())
            .chain(class.heavy.factors());
        for factor in factors {
            if !(factor.is_finite() && factor > 0.) {
                return Err(invalid(weapon.name(), "a factor must be above nought"));
            }
        }
        class
            .guard
            .check()
            .map_err(|message| invalid(weapon.name(), message))?;
        if !(swing.is_finite() && swing > 0. && swing <= 1.) {
            return Err(invalid(
                weapon.name(),
                "swing_posture is a share, above nought and at most one",
            ));
        }
    }
    Ok(())
}

fn parse_one<T: for<'de> Deserialize<'de>>(
    file: &'static str,
    text: &str,
) -> Result<T, CombatTablesError> {
    ron::from_str(text).map_err(|error| CombatTablesError::Parse {
        file,
        message: error.to_string(),
    })
}

static CURRENT: LazyLock<RwLock<Arc<CombatTables>>> =
    LazyLock::new(|| RwLock::new(Arc::new(CombatTables::shipped())));

/// Make `tables` the ones every `spec` and every `Combatant` reads from
/// now on. Process-wide: see the module docs for why and for what that
/// costs. Actors already stepping keep their pools; the next tick reads
/// the new rates.
pub fn install(tables: CombatTables) {
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(tables);
}

/// The installed tables, or the shipped ones if nothing was installed.
#[must_use]
pub fn current() -> Arc<CombatTables> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The installed melee table, for a tool that builds its sides from the
/// reach a class has.
pub fn with_melee<T>(read: impl FnOnce(&MeleeTable) -> T) -> T {
    with(|tables| read(&tables.melee))
}

/// Read one value out of the installed tables without cloning the handle;
/// what `Action::spec` and `Weapon::spec` go through on every tick.
pub(crate) fn with<T>(read: impl FnOnce(&CombatTables) -> T) -> T {
    read(&CURRENT.read().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{Bloom, Falloff, Hold};

    /// The numbers as they stood in code before the tables existed, kept
    /// here on purpose rather than read from the loader's defaults, so a
    /// table edit that drifts from them fails this test and is noticed.
    #[test]
    #[allow(clippy::too_many_lines, reason = "one table of expected literals")]
    fn the_shipped_tables_reproduce_the_literals_that_were_in_code() {
        let tables =
            CombatTables::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/combat"))
                .expect("the shipped files load");
        assert_eq!(
            tables,
            CombatTables::shipped(),
            "the compiled-in copy is the file"
        );
        let rows = [
            (Action::Light, [0.13, 0.22, 0.40, 0.31, 18., 28., 34., 0.50]),
            (
                Action::Followup,
                [0.12, 0.22, 0.40, 0.31, 18., 34., 42., 0.60],
            ),
            (Action::Heavy, [0.32, 0.45, 0.85, 0.69, 32., 55., 80., 0.85]),
            (Action::Dodge, [0.06, 0.24, 0.48, 0.48, 26., 0., 0., 3.4]),
            (Action::Reload, [0., 0., 1.35, 0., 0., 0., 0., 0.]),
            (Action::Swap, [0., 0., 0.45, 0.45, 0., 0., 0., 0.]),
            (Action::Stagger, [0., 0., 0.42, 0.42, 0., 0., 0., 0.]),
            (Action::Ready, [0., 0., 0., 0., 0., 0., 0., 0.]),
            (Action::Dead, [0., 0., 0., 0., 0., 0., 0., 0.]),
        ];
        for (action, [start, end, duration, cancel, cost, damage, posture, travel]) in rows {
            let spec = tables.actions.spec(action);
            let got = [
                spec.contact_start,
                spec.contact_end,
                spec.duration,
                spec.dodge_cancel,
                spec.stamina,
                spec.damage,
                spec.posture,
                spec.travel,
            ];
            let want = [start, end, duration, cancel, cost, damage, posture, travel];
            assert_eq!(
                got.map(f32::to_bits),
                want.map(f32::to_bits),
                "{action:?}: {got:?} is not {want:?}"
            );
        }
        let carbine = tables.weapons.spec(Weapon::Carbine);
        assert_eq!(
            carbine,
            WeaponSpec {
                interval: 0.42,
                bolt_speed: 45.,
                damage: 24.,
                posture: 15.,
                magazine: 24,
                reload: 1.35,
                spread: 4.0,
                aim_spread: 2.0,
                mode: FireMode::Burst {
                    count: 3,
                    gap: 0.09
                },
                hold: Some(Hold {
                    gap: 0.12,
                    bloom: 0.4
                }),
                bloom: Bloom {
                    shot: 0.25,
                    cap: 4.0,
                    wait: 0.15,
                    settle: 6.0
                },
                move_spread: 1.0,
                range: 70.,
                falloff: Falloff {
                    full: 25.,
                    far: 50.,
                    share: 0.55
                },
                aim_time: 0.,
                zoom: 1.,
            }
        );
        assert_eq!(
            tables.weapons.spec(Weapon::Pistol),
            WeaponSpec {
                interval: 0.32,
                bolt_speed: 60.,
                damage: 24.,
                posture: 16.,
                magazine: 10,
                reload: 0.9,
                spread: 2.4,
                aim_spread: 1.0,
                mode: FireMode::Single,
                hold: None,
                bloom: Bloom {
                    shot: 0.6,
                    cap: 2.5,
                    wait: 0.20,
                    settle: 4.0
                },
                move_spread: 0.3,
                range: 90.,
                falloff: Falloff {
                    full: 30.,
                    far: 60.,
                    share: 0.6
                },
                aim_time: 0.,
                zoom: 1.,
            }
        );
        assert_eq!(
            tables.weapons.spec(Weapon::Rifle),
            WeaponSpec {
                interval: 1.8,
                bolt_speed: 140.,
                damage: 85.,
                posture: 55.,
                magazine: 4,
                reload: 2.4,
                spread: 7.0,
                aim_spread: 0.25,
                mode: FireMode::Single,
                hold: None,
                bloom: Bloom {
                    shot: 2.5,
                    cap: 3.0,
                    wait: 0.5,
                    settle: 4.0
                },
                move_spread: 3.0,
                range: 260.,
                falloff: Falloff {
                    full: 150.,
                    far: 260.,
                    share: 0.7
                },
                aim_time: 0.875,
                zoom: 3.5,
            }
        );
        assert_eq!(tables.weapons.version, 7);
        // The melee classes scale the cut rows above; a Blade is the rows.
        let one = CutScale {
            time: 1.,
            damage: 1.,
            posture: 1.,
            stamina: 1.,
            travel: 1.,
        };
        assert_eq!(tables.melee.version, 4);
        assert_eq!(
            tables.melee.caught,
            Caught {
                posture: 1.6,
                damage: 1.0,
                stagger: 1.5
            }
        );
        assert_eq!(tables.melee.blade_reach_m.to_bits(), 1.7_f32.to_bits());
        assert_eq!(tables.melee.parry_rest.to_bits(), 0.5_f32.to_bits());
        assert_eq!(tables.melee.guard_regen.to_bits(), 0.35_f32.to_bits());
        assert_eq!(tables.melee.classes.len(), 3);
        assert_eq!(
            tables.melee.classes[&Weapon::Blade],
            MeleeClass {
                reach: 1.,
                light: one,
                heavy: one,
                swing_posture: 1.,
                guard: GuardRow {
                    share: 0.60,
                    arc: 70.,
                    stamina: 1.40,
                    posture: 0.40,
                    parry: 0.20,
                    riposte: 70.,
                    bolts: None
                }
            }
        );
        assert_eq!(
            tables.melee.classes[&Weapon::Greatblade],
            MeleeClass {
                reach: 1.3,
                light: CutScale {
                    time: 1.75,
                    damage: 1.65,
                    posture: 1.70,
                    stamina: 1.65,
                    travel: 1.
                },
                heavy: CutScale {
                    time: 1.35,
                    damage: 1.65,
                    posture: 1.65,
                    stamina: 1.50,
                    travel: 1.
                },
                swing_posture: 0.3,
                guard: GuardRow {
                    share: 0.80,
                    arc: 80.,
                    stamina: 0.70,
                    posture: 0.20,
                    parry: 0.10,
                    riposte: 90.,
                    bolts: None
                }
            }
        );
        assert_eq!(
            tables.melee.classes[&Weapon::Glaive],
            MeleeClass {
                reach: 1.75,
                light: CutScale {
                    time: 1.35,
                    damage: 1.15,
                    posture: 1.00,
                    stamina: 1.20,
                    travel: 1.2
                },
                heavy: CutScale {
                    time: 1.20,
                    damage: 1.10,
                    posture: 1.00,
                    stamina: 1.20,
                    travel: 1.4
                },
                swing_posture: 1.,
                guard: GuardRow {
                    share: 0.50,
                    arc: 60.,
                    stamina: 1.70,
                    posture: 0.50,
                    parry: 0.14,
                    riposte: 60.,
                    bolts: None
                }
            }
        );
        assert_eq!(tables.weapons.weapons.len(), 3, "no row for a melee class");
        assert_eq!(
            tables.actor,
            ActorTable {
                version: 3,
                health: 100.,
                shield: 40.,
                stamina: 100.,
                posture: 100.,
                stamina_regen: 24.,
                shield_regen: 8.,
                posture_regen: 35.,
                recovery_wait: 0.55,
                hurt_wait: 3.,
                buffer: 0.15,
                combo_window: 0.25,
                aim_speed_scale: 0.65,
                walk_speed: 5.5,
                sprint_speed: 8.5,
                jump_speed: 5.2,
                gravity: 18.,
            }
        );
        assert_eq!(
            tables.opponents,
            OpponentTable {
                version: 7,
                health: 85.,
                passive_health: 300.,
                shield: 20.,
                damage_scale: 0.5,
                engage_distance: 11.,
                sentry: SentryTable {
                    bolt_speed: 24.,
                    mode: FireMode::Single,
                    charge: 0.65,
                    recover: 1.65,
                    cooldown: 1.0,
                    stagger_cooldown: 0.9,
                    walk_speed: 2.5,
                    hold_near: 9.,
                    hold_far: 20.,
                    fire_range: 32.,
                },
                rusher: RusherTable {
                    walk_speed: 3.1,
                    reach: 1.7,
                    cooldown: 2.6,
                    initial_cooldown: 0.5,
                },
                marksman: MarksmanTable {
                    bolt_speed: 110.,
                    charge: 1.6,
                    lock: 0.35,
                    recover: 2.2,
                    cooldown: 0.9,
                    stagger_cooldown: 1.2,
                    walk_speed: 1.8,
                    hold_near: 40.,
                    hold_far: 80.,
                    fire_range: 120.,
                    flee: 12.,
                    pace: 0.7,
                },
                duelist: DuelistTable {
                    walk_speed: 3.0,
                    reach: 1.9,
                    cooldown: 1.6,
                    initial_cooldown: 0.6,
                    wary: 7.0,
                    react: 0.22,
                    heavy_every: 3,
                    rest: 0.5,
                    parry_rest: 0.,
                    stagger_cooldown: 0.6,
                    slow_margin: 0.1,
                    read_reach: 1.5,
                },
                bulwark: BulwarkTable {
                    advance_speed: 4.6,
                    reach: 2.0,
                    cooldown: 1.8,
                    initial_cooldown: 0.9,
                    rest: 0.45,
                    stagger_cooldown: 1.2,
                    pinned: 0.9,
                    guard: GuardRow {
                        share: 0.60,
                        arc: 75.,
                        stamina: 2.,
                        posture: 0.40,
                        parry: 0.,
                        riposte: 0.,
                        bolts: Some(BoltGuard {
                            share: 0.97,
                            stamina: 0.15,
                            posture: 0.
                        })
                    }
                },
                sprint_speed: 6.,
            }
        );
        assert_eq!(
            tables.tiers,
            TierTable {
                version: 1,
                centre: (-400., -500.),
                bands_m: vec![6000., 15000.],
                tiers: vec![
                    crate::combat_tiers::TierRow {
                        health: 1.0,
                        shield: 1.0,
                        damage: 1.0
                    },
                    crate::combat_tiers::TierRow {
                        health: 1.3,
                        shield: 1.3,
                        damage: 1.15
                    },
                    crate::combat_tiers::TierRow {
                        health: 1.7,
                        shield: 1.7,
                        damage: 1.3
                    },
                ],
            }
        );
    }

    #[test]
    fn a_table_missing_an_action_or_a_weapon_is_refused() {
        let without_heavy = ACTIONS.replace("Heavy:", "Heavier:");
        let error =
            CombatTables::parse(&without_heavy, WEAPONS, ACTOR, OPPONENTS, MELEE, ABILITIES)
                .expect_err("an unknown key is not valid RON for the row type");
        assert!(
            matches!(
                error,
                CombatTablesError::Parse {
                    file: "actions.ron",
                    ..
                }
            ),
            "{error}"
        );
        let start = ACTIONS.find("        Heavy:").expect("the row");
        let end = ACTIONS[start..]
            .find("        // Direction")
            .expect("the next row")
            + start;
        let mut without_heavy = ACTIONS.to_owned();
        without_heavy.replace_range(start..end, "");
        let error =
            CombatTables::parse(&without_heavy, WEAPONS, ACTOR, OPPONENTS, MELEE, ABILITIES)
                .expect_err("a row left out is refused");
        assert!(
            matches!(
                error,
                CombatTablesError::Missing {
                    file: "actions.ron",
                    row: "Heavy"
                }
            ),
            "{error}"
        );
        let start = WEAPONS.find("        Pistol:").expect("the row");
        let end = WEAPONS[start..].find("    },").expect("the end of the map") + start;
        let mut without_pistol = WEAPONS.to_owned();
        without_pistol.replace_range(start..end, "");
        let error =
            CombatTables::parse(ACTIONS, &without_pistol, ACTOR, OPPONENTS, MELEE, ABILITIES)
                .expect_err("a row left out is refused");
        assert!(
            matches!(
                error,
                CombatTablesError::Missing {
                    file: "weapons.ron",
                    row: "Pistol"
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_weapon_row_that_cannot_be_played_is_refused() {
        for (from, to, why) in [
            ("gap: 0.09", "gap: -0.09", "a negative gap"),
            ("count: 3", "count: 0", "a count of nought"),
            ("far: 50.0", "far: 15.0", "a far under full"),
            ("share: 0.55)", "share: 1.5)", "a share over one"),
            ("range: 70.0", "range: 0.0", "no range"),
            ("gap: 0.12", "gap: 0.0", "a hold with no gap"),
            ("aim_time: 0.0", "aim_time: -0.1", "a negative aim time"),
            ("zoom: 1.0", "zoom: 0.5", "a zoom under one"),
        ] {
            let text = WEAPONS.replace(from, to);
            assert_ne!(text, WEAPONS, "{why}: the edit found its place");
            let error = CombatTables::parse(ACTIONS, &text, ACTOR, OPPONENTS, MELEE, ABILITIES)
                .expect_err(why);
            assert!(
                matches!(
                    error,
                    CombatTablesError::Invalid {
                        file: "weapons.ron",
                        row: "Carbine",
                        ..
                    }
                ),
                "{why}: {error}"
            );
        }
    }

    #[test]
    fn a_sentry_mode_that_cannot_be_played_is_refused() {
        let text = OPPONENTS.replace("mode: Single", "mode: Burst(count: 0, gap: 0.1)");
        assert_ne!(text, OPPONENTS);
        let error = CombatTables::parse(ACTIONS, WEAPONS, ACTOR, &text, MELEE, ABILITIES)
            .expect_err("count 0");
        assert!(
            matches!(
                error,
                CombatTablesError::Invalid {
                    file: "opponents.ron",
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn the_version_follows_the_numbers_not_the_text() {
        let shipped = CombatTables::shipped();
        let reformatted = CombatTables::parse(
            &ACTIONS.replace("0.13", "0.130"),
            WEAPONS,
            ACTOR,
            OPPONENTS,
            MELEE,
            ABILITIES,
        )
        .expect("still parses");
        assert_eq!(shipped.version(), reformatted.version());
        let changed = CombatTables::parse(
            &ACTIONS.replace("damage: 55.0", "damage: 56.0"),
            WEAPONS,
            ACTOR,
            OPPONENTS,
            MELEE,
            ABILITIES,
        )
        .expect("still parses");
        assert_ne!(shipped.version(), changed.version());
    }

    #[test]
    fn install_swaps_what_spec_reads() {
        // A field no other test in this binary reads, since the slot is
        // shared by every test thread.
        let mut tables = CombatTables::shipped();
        tables.opponents.engage_distance = 999.;
        install(tables);
        assert!((current().opponents.engage_distance - 999.).abs() < f32::EPSILON);
        install(CombatTables::shipped());
        assert!((current().opponents.engage_distance - 11.).abs() < f32::EPSILON);
    }

    #[test]
    fn a_melee_table_that_cannot_be_played_is_refused() {
        let invalid = |text: &str, why: &str| {
            assert_ne!(text, MELEE, "{why}: the edit found its place");
            let error = CombatTables::parse(ACTIONS, WEAPONS, ACTOR, OPPONENTS, text, ABILITIES)
                .expect_err(why);
            assert!(
                matches!(
                    error,
                    CombatTablesError::Invalid {
                        file: "melee.ron",
                        ..
                    }
                ),
                "{why}: {error}"
            );
        };
        // A class missing.
        let start = MELEE.find("        Glaive:").expect("the row");
        let end =
            MELEE[start..].find("        ),\n").expect("its end") + start + "        ),\n".len();
        let mut without = MELEE.to_owned();
        without.replace_range(start..end, "");
        let error = CombatTables::parse(ACTIONS, WEAPONS, ACTOR, OPPONENTS, &without, ABILITIES)
            .expect_err("a class left out is refused");
        assert!(
            matches!(
                error,
                CombatTablesError::Missing {
                    file: "melee.ron",
                    row: "Glaive"
                }
            ),
            "{error}"
        );
        // A factor of nought, or under.
        invalid(&MELEE.replace("reach: 1.3,", "reach: 0.0,"), "no reach");
        invalid(
            &MELEE.replace("damage: 1.65,", "damage: -1.0,"),
            "a negative damage",
        );
        invalid(
            &MELEE.replace("swing_posture: 0.3", "swing_posture: 0.0"),
            "no swing share",
        );
        invalid(
            &MELEE.replace("caught: (posture: 1.6", "caught: (posture: 0.5"),
            "a gun bearer cut for less than a melee one",
        );
        invalid(
            &MELEE.replace("swing_posture: 0.3", "swing_posture: 1.5"),
            "a share over one",
        );
        invalid(
            &MELEE.replace("share: 0.80", "share: 1.2"),
            "a guard's share over one",
        );
        invalid(&MELEE.replace("arc: 80", "arc: 0"), "a guard with no arc");
        // A bulwark's bolt figures are held to the bounds of the row's own.
        for (from, to) in [
            ("share: 0.97", "share: 1.5"),
            ("posture: 0.0)", "posture: 1.5)"),
            ("stamina: 0.15", "stamina: -1.0"),
        ] {
            assert!(
                CombatTables::parse(
                    ACTIONS,
                    WEAPONS,
                    ACTOR,
                    &OPPONENTS.replace(from, to),
                    MELEE,
                    ABILITIES
                )
                .is_err(),
                "{from}"
            );
        }
        invalid(
            &MELEE.replace("arc: 80", "arc: 200"),
            "a guard's arc over 180",
        );
        invalid(
            &MELEE.replace("parry: 0.10", "parry: -0.1"),
            "a negative parry window",
        );
        invalid(
            &MELEE.replace("parry_rest: 0.5", "parry_rest: -1.0"),
            "a negative rest",
        );
        invalid(
            &MELEE.replace("guard_regen: 0.35", "guard_regen: 1.5"),
            "a regeneration share over one",
        );
        invalid(
            &MELEE.replace("blade_reach_m: 1.7", "blade_reach_m: 0.0"),
            "no blade reach",
        );
        // A gun in the file.
        let gun = MELEE.replace("        Glaive:", "        Carbine:");
        invalid(&gun, "a gun's row");
    }

    #[test]
    fn a_melee_class_in_the_weapons_table_is_refused_for_what_it_is() {
        let extra = WEAPONS.replace(
            "    weapons: {",
            "    weapons: {\n        Blade: (interval: 0.3, bolt_speed: 1.0, damage: 1.0, posture: 1.0, magazine: 1, reload: 1.0, spread: 0.0, aim_spread: 0.0, mode: Single, bloom: (shot: 0.0, cap: 0.0, wait: 0.0, settle: 0.0), move_spread: 0.0, range: 1.0, falloff: (full: 0.0, far: 0.0, share: 1.0), aim_time: 0.0, zoom: 1.0),",
        );
        assert_ne!(extra, WEAPONS);
        let error = CombatTables::parse(ACTIONS, &extra, ACTOR, OPPONENTS, MELEE, ABILITIES)
            .expect_err("a melee class has its row in melee.ron");
        assert!(
            matches!(
                error,
                CombatTablesError::Invalid {
                    file: "weapons.ron",
                    row: "Blade",
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn the_melee_table_is_in_the_version() {
        let shipped = CombatTables::shipped();
        let changed = CombatTables::parse(
            ACTIONS,
            WEAPONS,
            ACTOR,
            OPPONENTS,
            &MELEE.replace("time: 1.75", "time: 1.8"),
            ABILITIES,
        )
        .expect("still parses");
        assert_ne!(shipped.version(), changed.version());
    }

    #[test]
    fn the_abilities_table_is_in_the_version_and_a_bad_one_is_refused_for_what_it_is() {
        let shipped = CombatTables::shipped();
        let changed = CombatTables::parse(
            ACTIONS,
            WEAPONS,
            ACTOR,
            OPPONENTS,
            MELEE,
            &ABILITIES.replace("cooldown: 8.0,", "cooldown: 9.0,"),
        )
        .expect("still parses");
        assert_ne!(shipped.version(), changed.version());
        let error = CombatTables::parse(
            ACTIONS,
            WEAPONS,
            ACTOR,
            OPPONENTS,
            MELEE,
            &ABILITIES.replace("windup: 0.12,", "windup: 0.9,"),
        )
        .expect_err("a windup past the length");
        assert!(
            matches!(
                error,
                CombatTablesError::Invalid {
                    file: "abilities.ron",
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one table, every row of it written out"
    )]
    fn the_shipped_abilities_reproduce_the_design() {
        use crate::ability::{
            AbilityEffect, Along, Blast, Cover, Jump, OnHit, Own, ShotCone, ThenStrike,
        };
        let table = CombatTables::shipped().abilities;
        assert_eq!(table.version, 5);
        assert_eq!((table.bar.weapon, table.bar.utility), (3, 2));
        assert_eq!(table.hit_arc.to_bits(), 40_f32.to_bits());
        // (id, stamina, cooldown, windup, length, cancel, pace)
        let rows = [
            ("heavy_cut", 0., 0., 0., 0., 0., 0.),
            ("butt_strike", 20., 6., 0.12, 0.40, 0.30, 0.3),
            ("precision_shot", 25., 8., 0.35, 0.65, 0.50, 0.3),
            ("aimed_burst", 25., 10., 0.20, 0.75, 0.65, 0.4),
            ("focus", 30., 18., 0., 0., 0., 1.0),
            ("kinetic_absorb", 30., 12., 0.10, 0.30, 0.30, 1.0),
            ("rapid_reload", 15., 10., 0.125, 0.2, 0.2, 0.6),
            ("shield_recharge", 40., 30., 0.40, 0.60, 0.60, 0.5),
            ("ricochet", 20., 9., 0.15, 0.45, 0.35, 0.6),
            ("disarm", 25., 7., 0.10, 0.35, 0.25, 0.5),
            ("suppressing_fire", 30., 14., 0.15, 1.10, 0.85, 0.3),
            ("explosive_shot", 35., 10., 0.20, 0.50, 0.30, 0.4),
            ("charged_shot", 35., 14., 0.90, 1.20, 0.90, 0.2),
            ("armour_break", 30., 18., 0.30, 0.70, 0.55, 0.4),
            ("thrust", 25., 8., 0.15, 0.66, 0.51, 0.0),
            ("reflect_shot", 15., 9., 0., 0., 0., 1.0),
            ("sweeping_cut", 35., 6., 0.35, 0.85, 0.65, 0.2),
            ("vault", 30., 12., 0.10, 0.85, 0.70, 0.0),
            ("combat_slide", 20., 6., 0.0, 0.50, 0.50, 0.0),
        ];
        assert_eq!(table.abilities.len(), rows.len());
        for (row, (id, stamina, cooldown, windup, length, cancel, pace)) in
            table.abilities.iter().zip(rows)
        {
            assert_eq!(row.id, id);
            let got = [
                row.stamina,
                row.cooldown,
                row.windup,
                row.length,
                row.cancel,
                row.pace,
            ];
            let want: [f32; 6] = [stamina, cooldown, windup, length, cancel, pace];
            for (got, want) in got.iter().zip(want) {
                assert_eq!(got.to_bits(), want.to_bits(), "{id}");
            }
        }
        assert_eq!(table.abilities[0].effect, AbilityEffect::Cut(Action::Heavy));
        assert_eq!(
            table.abilities[1].effect,
            AbilityEffect::Strike {
                reach: 2.2,
                arc: 50.,
                damage: 8.,
                posture: 105.,
                scaled: false,
                on_hit: None
            }
        );
        assert_eq!(
            table.abilities[2].effect,
            AbilityEffect::Shot {
                rounds: 1,
                gap: 0.,
                cone: ShotCone::Exact,
                damage: 2.2,
                posture: 1.5,
                bloom: false,
                on_hit: None,
                jump: None,
                blast: None,
                breach: false
            }
        );
        assert_eq!(
            table.abilities[3].effect,
            AbilityEffect::Shot {
                rounds: 5,
                gap: 0.08,
                cone: ShotCone::Aimed,
                damage: 1.,
                posture: 1.,
                bloom: false,
                on_hit: None,
                jump: None,
                blast: None,
                breach: false
            }
        );
        assert_eq!(
            table.abilities[4].effect,
            AbilityEffect::Own(Own::Status {
                id: "focused".to_owned(),
                seconds: 5.
            })
        );
        assert_eq!(
            table.abilities[5].effect,
            AbilityEffect::Own(Own::Status {
                id: "fortified".to_owned(),
                seconds: 2.
            })
        );
        assert_eq!(table.abilities[6].effect, AbilityEffect::Own(Own::Reload));
        assert_eq!(table.abilities[7].effect, AbilityEffect::Own(Own::Shield));
        let shot =
            |rounds, gap, cone, damage, posture, on_hit, jump, blast, breach| AbilityEffect::Shot {
                rounds,
                gap,
                cone,
                damage,
                posture,
                bloom: false,
                on_hit,
                jump,
                blast,
                breach,
            };
        assert_eq!(
            table.abilities[8].effect,
            shot(
                1,
                0.,
                ShotCone::Aimed,
                1.,
                1.,
                None,
                Some(Jump {
                    radius: 8.,
                    share: 0.8
                }),
                None,
                false
            )
        );
        assert_eq!(
            table.abilities[9].effect,
            AbilityEffect::Hit {
                damage: 0.6,
                posture: 0.5,
                status: "disarmed".to_owned(),
                seconds: 3.
            }
        );
        assert_eq!(
            table.abilities[9].works_with,
            [Weapon::Pistol, Weapon::Blade]
        );
        assert_eq!(
            table.abilities[10].effect,
            shot(
                10,
                0.07,
                ShotCone::Hip,
                0.6,
                0.8,
                Some(OnHit {
                    status: "slowed".to_owned(),
                    seconds: 3.
                }),
                None,
                None,
                false
            )
        );
        assert_eq!(
            table.abilities[11].effect,
            shot(
                1,
                0.,
                ShotCone::Aimed,
                1.6,
                1.,
                None,
                None,
                Some(Blast {
                    radius: 4.,
                    damage: 48.,
                    posture: 90.
                }),
                false
            )
        );
        assert_eq!(
            table.abilities[12].effect,
            shot(1, 0., ShotCone::Exact, 1.1, 1.5, None, None, None, true)
        );
        assert_eq!(
            table.abilities[13].effect,
            AbilityEffect::Hit {
                damage: 0.4,
                posture: 0.5,
                status: "shield_broken".to_owned(),
                seconds: 8.
            }
        );
        assert_eq!(
            table.abilities[13].works_with,
            [Weapon::Rifle, Weapon::Greatblade]
        );
        assert_eq!(
            table.abilities[14].effect,
            AbilityEffect::Move {
                distance: 6.,
                seconds: 0.28,
                along: Along::Aim,
                then: Some(ThenStrike {
                    reach: 1.,
                    arc: 20.,
                    damage: 1.3,
                    posture: 1.2,
                    scaled: true
                }),
                cover: Some(Cover {
                    arc: 60.,
                    share: 0.8
                })
            }
        );
        assert_eq!(
            table.abilities[14].works_with,
            [Weapon::Blade, Weapon::Glaive]
        );
        assert_eq!(
            table.abilities[15].effect,
            AbilityEffect::Own(Own::Status {
                id: "reflecting".to_owned(),
                seconds: 8.
            })
        );
        assert_eq!(
            table.abilities[16].effect,
            AbilityEffect::Strike {
                reach: 1.,
                arc: 110.,
                damage: 1.8,
                posture: 1.,
                scaled: true,
                on_hit: None
            }
        );
        assert_eq!(
            table.abilities[16].works_with,
            [Weapon::Greatblade, Weapon::Glaive]
        );
        assert_eq!(
            table.abilities[17].effect,
            AbilityEffect::Move {
                distance: 6.5,
                seconds: 0.55,
                along: Along::Aim,
                then: Some(ThenStrike {
                    reach: 1.,
                    arc: 60.,
                    damage: 1.2,
                    posture: 1.5,
                    scaled: true
                }),
                cover: Some(Cover {
                    arc: 60.,
                    share: 0.4
                })
            }
        );
        assert_eq!(
            table.abilities[18].effect,
            AbilityEffect::Move {
                distance: 6.,
                seconds: 0.45,
                along: Along::Input,
                then: None,
                cover: None
            }
        );
        assert!(table.abilities[18].armed);
        assert!(table.abilities[..18].iter().all(|row| !row.armed));
        assert_eq!(table.statuses.len(), 6);
        let slowed = table.status("slowed").expect("slowed");
        assert_eq!(slowed.pace, Some(0.6));
        assert!(table.status("shield_broken").expect("shield").shield_locked);
        assert!(table.status("disarmed").expect("disarmed").disarmed);
        assert_eq!(
            table.status("reflecting").expect("reflecting").reflects,
            Some(2.)
        );
        let focused = table.status("focused").expect("focused");
        assert!(focused.tight_cone && focused.posture_taken.is_none());
        let fortified = table.status("fortified").expect("fortified");
        assert!(!fortified.tight_cone);
        assert_eq!(fortified.posture_taken, Some(0.));
        assert_eq!(fortified.damage_taken, Some(0.5));
        assert!(focused.damage_taken.is_none());
    }
}
