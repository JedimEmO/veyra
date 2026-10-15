//! The input writers. A policy is what a hand on the controls would do,
//! reduced to a few parameters: it reads what a player could see (where the
//! opponent is, what it is doing, whether the muzzle has a clear line) and
//! writes a [`PlayerIntent`] and a [`CombatIntent`] every tick, the way the
//! game's input and its encounter AI do. It never writes a `Combatant`: the
//! sim and the physics decide what the inputs are worth, so a bout here is
//! the one model, not an imitation of it.
//!
//! [`Policy::Sentry`], [`Policy::Rusher`], [`Policy::Marksman`] and
//! [`Policy::Duelist`] are the game's own opponents:
//! they run `strata_sim::combat_ai`, the code the encounter runs, on the
//! installed `opponents.ron` row every tick, so a sweep of an opponent
//! number here is a sweep of the game's.

use bevy::ecs::prelude::*;
use bevy::math::{DVec3, Dir3, Vec3};
use bevy::time::{Fixed, Time};
use serde::{Deserialize, Serialize};
use strata_core::{Rng, WorldPos};
use strata_physics::CharacterController;
use strata_physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata_sim::ability::{AbilityEffect, AbilityRow, Along, Own};
use strata_sim::combat::{Action, BlasterBolt, CombatIntent, Combatant};
use strata_sim::combat_ai::{OpponentBrain, OpponentKind, Seen};
use strata_sim::{PlayerIntent, RenderOrigin, WorldPosition};

/// Serde's default for a flag that is on unless a file says otherwise.
fn yes() -> bool {
    true
}

/// What a combatant does with its inputs. Every number a policy needs is a
/// field here, so a grid can vary a policy as easily as a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Policy {
    /// Keeps `range` by backing off and advancing, strafes between, fires
    /// when the muzzle has a line and reloads when empty. `cadence` is the
    /// seconds between trigger pulls: zero holds the trigger and lets the
    /// weapon's interval set the rate.
    HoldAndFire {
        /// Metres it tries to hold.
        range: f32,
        /// Seconds between trigger pulls; zero holds the trigger.
        #[serde(default)]
        cadence: f32,
        /// Whether the aim is held while it fights; false fires from the
        /// hip, which a preset reads the cost of not aiming by.
        #[serde(default = "yes")]
        aim: bool,
    },
    /// Stands where it was put, aims and fires when the muzzle has a line,
    /// reloading when empty: a shooter with no footwork in its number, so a
    /// weapon's rate at one distance reads on its own. `cadence` is as for
    /// [`Policy::HoldAndFire`].
    Aim {
        /// Seconds between trigger pulls; zero holds the trigger.
        #[serde(default)]
        cadence: f32,
    },
    /// Walks to `reach` metres and cuts. With `chain` a Light is followed
    /// by its return cut; every `heavy_every`th opening is a Heavy instead
    /// (one means every opening). `cooldown` is the seconds between
    /// openings, a player's none.
    CloseAndCut {
        /// Queue the return cut behind every Light.
        chain: bool,
        /// A Heavy every this many openings; zero means never.
        heavy_every: u32,
        /// Seconds between openings.
        #[serde(default)]
        cooldown: f32,
        /// Metres from the target at which it begins cutting.
        reach: f32,
        /// At a Heavy opening, wait until stamina covers the cut instead
        /// of dropping the press.
        #[serde(default)]
        wait_stamina: bool,
        /// Sprint straight at the target while farther than twice `reach`.
        #[serde(default)]
        sprint: bool,
    },
    /// Walks to `reach` metres behind a raised guard, lowers it to cut as
    /// [`Policy::CloseAndCut`] does and raises it again, never opening
    /// while a blow is on its way. With `parry` it keeps the guard down until an opponent's swing or a bolt is about
    /// `reaction` seconds from landing, then raises it, so the guard is
    /// fresh when the blow arrives.
    GuardAndCut {
        /// Queue the return cut behind every Light.
        chain: bool,
        /// A Heavy every this many openings; zero means never.
        heavy_every: u32,
        /// Seconds between openings, counted from the end of a cut.
        cooldown: f32,
        /// Metres from the target at which it begins cutting.
        reach: f32,
        /// Keep the guard down until a blow is about to land.
        #[serde(default)]
        parry: bool,
        /// Seconds before a blow lands at which a parrying hand raises
        /// the guard; each blow draws between 0.6 and 1.4 of it.
        #[serde(default)]
        reaction: f32,
        /// Sprint straight at the target, guard down, while farther than twice `reach`.
        #[serde(default)]
        sprint: bool,
    },
    /// Strafes at `range`, fires when it can, and with `dodge_on_telegraph`
    /// dodges sideways when the opponent's swing begins, its charge shows
    /// (after a reaction) or a bolt leaves its muzzle. Without the flag
    /// the same policy never dodges.
    Kite {
        /// Dodge on the opponent's wind-up, its charge and a bolt's launch.
        dodge_on_telegraph: bool,
        /// Metres it tries to hold.
        range: f32,
    },
    /// Stands its ground facing the opponent, never fires, and with
    /// `dodge_on_telegraph` dodges as [`Policy::Kite`] does. The dodge's
    /// pair: with the gun and the footwork out of the number, what one
    /// side takes over the other is what the dodge is worth.
    Brace {
        /// Dodge on the opponent's wind-up, its charge and a bolt's launch.
        dodge_on_telegraph: bool,
    },
    /// The game's sentry, `strata_sim::combat_ai` on the installed
    /// `opponents.sentry` row: holds between its near and far distances,
    /// charges with its aim locked, fires, recovers.
    Sentry,
    /// The game's rusher, the same code on `opponents.rusher`: closes to
    /// its reach and swings a Heavy every cooldown.
    Rusher,
    /// The game's marksman, the same code on `opponents.marksman`: holds
    /// 40 to 80 m behind a beam, locks its aim and fires a fast bolt.
    Marksman,
    /// The game's duelist, the same code on `opponents.duelist`: closes
    /// behind a guard, parries a slow cut it sees begin, and is open while
    /// it swings and rests.
    Duelist,
    /// The game's bulwark, the same code on `opponents.bulwark`: walks in
    /// behind a shield that holds bolts from the front and lowers it to cut.
    Bulwark,
    /// A gun worked by a hand that plays against a guard. While its target's
    /// guard is up it holds its fire, keeps `range` metres away by backing
    /// off (it cannot outrun a bulwark, and that is the point) and faces it.
    /// When it sees the target begin a cut inside one and a half times the
    /// bulwark's reach it dodges away once, after a reaction drawn from the
    /// lower half of [`REACTION`]. While the guard is down, in a cut, its rest
    /// or a stagger, it fires at the weapon's own pace; it reloads when empty
    /// and the guard is up.
    Punish {
        /// Metres it tries to keep.
        range: f32,
    },
    /// A long gun worked by a hand that will not stand in the open: it
    /// stands, aims and fires, then walks across the line for `walk`
    /// seconds, then stands again, and pulls the trigger only once the aim
    /// has been closed for `stand` seconds. The rifle's answer to a gun
    /// that locks its aim and fires where the target was.
    StopAndShoot {
        /// Seconds it strafes after each shot.
        walk: f32,
        /// Seconds the aim is held closed before the trigger is pulled.
        stand: f32,
    },
    /// Sprints at the target until it is within `within` metres, then
    /// fights by `then`: "close to 20 m, then fire".
    Close {
        /// Metres from the target at which `then` takes over.
        within: f32,
        /// What it does once there.
        then: Box<Policy>,
    },
    /// Stands where it was put and does nothing: the target dummy a
    /// weapon's rate or a chain's reliability is measured against, with
    /// no policy of its own in the number.
    Stand,
    /// Replays a CSV of inputs, one row per tick that changes them: the
    /// columns are `tick,move_x,move_y,yaw,sprint,fire,aim,light,heavy,dodge,reload,weapon`,
    /// a row holds until the next, and `weapon` is empty, `Carbine` or
    /// `Pistol`. Later a played session's inputs go in here.
    Scripted {
        /// The CSV, relative to the working directory.
        path: String,
    },
}

/// One row of a [`Policy::Scripted`] file.
#[derive(Debug, Clone, Copy)]
pub struct ScriptRow {
    /// The tick the row takes effect on.
    pub tick: u64,
    /// The locomotion to write.
    pub movement: PlayerIntent,
    /// The combat inputs to write; edges fire once, on this tick.
    pub combat: CombatIntent,
    /// A weapon named by family, drawn from whichever slot holds it on the
    /// side the script drives; a number in the file is a slot instead.
    pub family: Option<strata_sim::combat::Weapon>,
}

impl ScriptRow {
    /// Parse a whole script. The header is skipped by its first column.
    ///
    /// # Errors
    /// A row with the wrong number of columns or a column that does not
    /// parse, with its line number.
    pub fn parse_csv(text: &str) -> anyhow::Result<Vec<Self>> {
        let mut rows = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("tick") || line.starts_with('#') {
                continue;
            }
            let columns: Vec<&str> = line.split(',').map(str::trim).collect();
            anyhow::ensure!(
                columns.len() == 12,
                "script line {}: {} columns, want 12",
                index + 1,
                columns.len()
            );
            let number = |column: usize| -> anyhow::Result<f32> {
                columns[column]
                    .parse()
                    .map_err(|error| anyhow::anyhow!("script line {}: {error}", index + 1))
            };
            let flag = |column: usize| matches!(columns[column], "1" | "true" | "t" | "yes");
            let named = |name: &str| {
                let mut chars = name.chars();
                let first = chars.next()?.to_ascii_uppercase();
                strata_sim::combat::Weapon::from_name(&format!("{first}{}", chars.as_str()))
            };
            let (weapon, family) = match columns[11] {
                "" | "-" => (None, None),
                "0" => (Some(0), None),
                "1" => (Some(1), None),
                other => match named(other) {
                    Some(family) => (None, Some(family)),
                    None => anyhow::bail!("script line {}: unknown weapon {other}", index + 1),
                },
            };
            rows.push(Self {
                tick: columns[0]
                    .parse()
                    .map_err(|error| anyhow::anyhow!("script line {}: {error}", index + 1))?,
                movement: PlayerIntent {
                    move_dir: bevy::math::Vec2::new(number(1)?, number(2)?),
                    yaw: number(3)?,
                    jump: false,
                    sprint: flag(4),
                },
                combat: CombatIntent {
                    fire: flag(5),
                    aim: flag(6),
                    light: flag(7),
                    heavy: flag(8),
                    dodge: flag(9),
                    reload: flag(10),
                    weapon,
                    ..Default::default()
                },
                family,
            });
        }
        rows.sort_by_key(|row| row.tick);
        Ok(rows)
    }
}

/// The seconds a policy waits before its first trigger pull or opening,
/// and before it answers a charge it sees, drawn from the seed: the band
/// of a human reaction with a decision in it. A policy constant, not a
/// table one, so no grid varies it.
pub const REACTION: (f32, f32) = (0.15, 0.45);

/// The most flight time a gun's lead accounts for, in seconds: a hand
/// leads a near target well and a far one not at all, which is why a slow
/// bolt at the encounter's 27 m misses a strafing player. A policy
/// constant, like [`REACTION`].
pub const LEAD_CAP: f64 = 0.6;

/// Metres within which an opponent's swing is a threat to a guard.
pub const THREAT_REACH: f32 = 4.;

/// Metres either side of its flight a bolt must pass to be a threat.
pub const BOLT_PATH: f64 = 1.;

/// The spread of a parrying hand's lead, as shares of its reaction.
pub const LEAD_SPREAD: (f64, f64) = (0.6, 1.4);

/// Seconds a cutting hand waits, once, the first time its target is
/// inside its reach, before its first opening: a person arrives, sees
/// where the other is and then commits. Drawn once a bout, a hand's
/// figure, so no grid varies it.
pub const ARRIVAL: (f32, f32) = (0., 0.25);

/// The share of its own pause a cutting hand takes after an opening, drawn
/// each time: nobody counts a second the same way twice.
pub const PAUSE_SPREAD: (f32, f32) = (0.85, 1.35);

/// The share of its own reach a cutting hand closes to, drawn once a bout:
/// a hand does not stop at the same centimetre every time, but it never
/// stops beyond the weapon's real reach, where a cut could not land, so a
/// draw over one reads as one.
pub const STEP_SPREAD: (f32, f32) = (0.9, 1.1);

/// Seconds past a Light's contact start before the return cut is pressed,
/// drawn each chain: the press follows the sight of the first cut, not the
/// tick it lands on.
pub const CHAIN_LAG: (f32, f32) = (0., 0.08);

/// A charge the policy has seen and the reaction it owes it.
#[derive(Debug, Clone, Copy)]
struct Telegraph {
    /// Seconds until the dodge, then negative.
    left: f32,
    /// The dodge for this charge was pressed.
    answered: bool,
}

/// The pause a cutting policy starts on (its own cooldown figure), or
/// `None` for a policy that does not cut.
fn cutting_pause(policy: &Policy) -> Option<f32> {
    match policy {
        Policy::CloseAndCut { cooldown, .. } | Policy::GuardAndCut { cooldown, .. } => {
            Some(*cooldown)
        }
        Policy::Close { then, .. } => cutting_pause(then),
        _ => None,
    }
}

/// A policy with the little state it keeps between ticks: the clocks it
/// runs on and the generator its small choices come from.
#[derive(Component, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags of a policy's moment"
)]
pub struct Driver {
    /// What it does.
    pub policy: Policy,
    rng: Rng,
    /// Seconds until the next trigger pull or opening.
    cooldown: f32,
    /// Seconds a stop-and-shoot hand still has to walk.
    walking: f32,
    /// Seconds it has held its aim closed since it last stood.
    stood: f32,
    /// The weapon is ready and the line clear but the hand is still on its
    /// walk, so it would fire loose; a tight cone is worth pressing now.
    ready_loose: bool,
    /// The shots this side had fired at the last tick, to know a new one.
    own_shots: u64,
    /// A guarding policy has reached its opponent once and opened.
    arrived: bool,
    /// A cutting hand's wait on first reaching its target ([`ARRIVAL`]).
    arrival: f32,
    /// The share of its reach a cutting hand closes to ([`STEP_SPREAD`]).
    step: f32,
    /// Seconds past the contact start before the return cut ([`CHAIN_LAG`]).
    chain_lag: f32,
    /// The pause last drawn ([`PAUSE_SPREAD`]), the figure a guard waits on.
    pause: f32,
    /// A closing policy has been inside its reach once.
    reached: bool,
    /// Seconds before a blow lands at which the guard goes up, drawn when
    /// the blow first shows.
    lead: Option<f32>,
    /// Seconds a blow has been in sight, so a guard is never raised sooner
    /// than a perception delay ([`REACTION`]) after it first shows.
    seen: f32,
    /// The strafe direction, `-1` or `1`.
    strafe: f32,
    /// Seconds until the strafe flips.
    strafe_left: f32,
    /// Openings so far, for the heavy count.
    openings: u32,
    /// The return cut was already queued behind the current Light.
    chained: bool,
    /// The opponent's shot count last tick: a change is a launch.
    seen_shots: u64,
    /// The charge being watched, for [`Policy::Kite`].
    telegraph: Option<Telegraph>,
    /// The game's opponent, for the four opponent policies.
    brain: Option<OpponentBrain>,
    /// The parsed script, for [`Policy::Scripted`].
    script: Vec<ScriptRow>,
    /// Ticks driven so far.
    tick: u64,
    /// Seconds until a tapping policy pulls the trigger again.
    pull_wait: f32,
}

impl Driver {
    /// Whether the trigger is down this tick. Zero `cadence` holds it. A
    /// positive one taps it for one tick every `cadence` seconds, a hand's
    /// fixed pace; a press that comes a little before the weapon is ready
    /// is latched by the weapon, as a player's would be.
    fn pull(&mut self, cadence: f32) -> bool {
        if cadence <= 0. {
            return true;
        }
        let press = self.pull_wait <= 0.;
        if press {
            self.pull_wait += cadence;
        }
        press
    }

    /// A driver for `policy`, its choices seeded by `seed`. A scripted
    /// policy reads its file here; an opponent policy takes its clocks from
    /// the tables installed now, which is the grid cell being run.
    ///
    /// # Errors
    /// A script that cannot be read or parsed.
    pub fn new(policy: Policy, seed: u64) -> anyhow::Result<Self> {
        let script = match &policy {
            Policy::Scripted { path } => {
                let text = std::fs::read_to_string(path)
                    .map_err(|error| anyhow::anyhow!("script {path}: {error}"))?;
                ScriptRow::parse_csv(&text)?
            }
            _ => Vec::new(),
        };
        let brain = match &policy {
            Policy::Sentry
            | Policy::Rusher
            | Policy::Marksman
            | Policy::Duelist
            | Policy::Bulwark => {
                let kind = match policy {
                    Policy::Sentry => OpponentKind::Sentry,
                    Policy::Rusher => OpponentKind::Rusher,
                    Policy::Marksman => OpponentKind::Marksman,
                    Policy::Bulwark => OpponentKind::Bulwark,
                    _ => OpponentKind::Duelist,
                };
                Some(OpponentBrain::new(
                    kind,
                    &strata_sim::combat_tables::current().opponents,
                ))
            }
            _ => None,
        };
        let mut rng = Rng::new(seed);
        let strafe = if rng.bool() { 1. } else { -1. };
        let strafe_left = rng.range_f64(1.5, 3.5);
        // A hand takes a moment to act once the opponent is in view; the
        // seed decides how long, which is what spreads the bouts of a cell.
        let reaction = rng.range_f64(f64::from(REACTION.0), f64::from(REACTION.1));
        #[allow(clippy::cast_possible_truncation, reason = "seconds, a few")]
        let (strafe_left, reaction) = (strafe_left as f32, reaction as f32);
        // The cutting hand's two bout-long draws come after every draw a
        // gun's driver makes, so a gun's bout reads as it always did.
        let (arrival, step, pause) = match cutting_pause(&policy) {
            Some(pause) => {
                let arrival = rng.range_f64(f64::from(ARRIVAL.0), f64::from(ARRIVAL.1));
                let step = rng.range_f64(f64::from(STEP_SPREAD.0), f64::from(STEP_SPREAD.1));
                #[allow(clippy::cast_possible_truncation, reason = "a share")]
                (arrival as f32, step as f32, pause)
            }
            None => (0., 1., 0.),
        };
        Ok(Self {
            policy,
            rng,
            cooldown: reaction,
            walking: 0.,
            stood: 0.,
            ready_loose: false,
            own_shots: 0,
            lead: None,
            seen: 0.,
            arrived: false,
            arrival,
            step,
            chain_lag: 0.,
            pause,
            reached: false,
            strafe,
            strafe_left,
            openings: 0,
            chained: false,
            seen_shots: 0,
            telegraph: None,
            brain,
            script,
            tick: 0,
            pull_wait: 0.,
        })
    }

    /// A draw from `range`, in the driver's own order.
    fn draw(&mut self, range: (f32, f32)) -> f32 {
        #[allow(clippy::cast_possible_truncation, reason = "a share or seconds")]
        {
            self.rng.range_f64(f64::from(range.0), f64::from(range.1)) as f32
        }
    }

    /// The wait after an opening or a press: the policy's own pause times a
    /// draw of [`PAUSE_SPREAD`], and the lag of a hand's press
    /// ([`CHAIN_LAG`]), which acts even on a pause of nought.
    fn next_pause(&mut self, base: f32) -> f32 {
        let spread = self.draw(PAUSE_SPREAD);
        base * spread + self.draw(CHAIN_LAG)
    }

    /// Whether the hand may start a melee press: its own wait is over and
    /// it has arrived, so an ability shares the clock of the cuts.
    fn hand_free(&self) -> bool {
        self.cooldown <= 0. && (self.reached || self.arrived)
    }

    /// Whether this policy aims a gun: its aim leads the target.
    fn ranged(&self) -> bool {
        fn aims(policy: &Policy) -> bool {
            match policy {
                Policy::HoldAndFire { .. }
                | Policy::Aim { .. }
                | Policy::Kite { .. }
                | Policy::Punish { .. }
                | Policy::StopAndShoot { .. } => true,
                Policy::Close { then, .. } => aims(then),
                _ => false,
            }
        }
        aims(&self.policy)
    }

    /// Whether to dodge this tick, `awake` past the first reaction: on
    /// the opponent's swing within reach, on a bolt's launch, or on its
    /// charge once a reaction has passed since the charge showed, once
    /// per charge. The charge is the telegraph the game's sentry shows.
    fn dodge_due(&mut self, target: &Target, distance: f32, awake: bool, dt: f32) -> bool {
        let swing = target.action.is_melee() && target.age < target.contact_start && distance < 4.;
        let launch = target.shots > self.seen_shots && distance > 3.;
        self.seen_shots = target.shots;
        let charge = if target.charge > 0. {
            let telegraph = self.telegraph.get_or_insert_with(|| {
                let left = self
                    .rng
                    .range_f64(f64::from(REACTION.0), f64::from(REACTION.1));
                #[allow(clippy::cast_possible_truncation, reason = "seconds, a few")]
                Telegraph {
                    left: left as f32,
                    answered: false,
                }
            });
            telegraph.left -= dt;
            let due = telegraph.left <= 0. && !telegraph.answered;
            if due {
                telegraph.answered = true;
            }
            due
        } else {
            self.telegraph = None;
            false
        };
        (awake && (swing || launch)) || charge
    }
}

/// What a policy can see of its opponent.
struct Target {
    position: WorldPos,
    velocity: Vec3,
    action: Action,
    /// Seconds into the action before its blade can land: the opponent's
    /// own row, which a sheet may have moved.
    contact_start: f32,
    contact_end: f32,
    age: f32,
    shots: u64,
    charge: f32,
    /// What an opponent's eye reads off it.
    looks: Seen,
}

/// Yaw that faces `delta`, the sim's convention (zero faces `-z`).
#[allow(clippy::cast_possible_truncation, reason = "a heading is f32")]
fn yaw_to(delta: DVec3) -> f32 {
    (-delta.x).atan2(-delta.z) as f32
}

/// Seconds until the soonest blow aimed at `me` lands: the opponent's swing
/// within reach, and any enemy bolt flying at `me` along a path that passes
/// within [`BOLT_PATH`] of it. A swing in contact lands now.
fn landing_in(
    me: WorldPos,
    team: u8,
    target: &Target,
    distance: f32,
    bolts: &[(u8, WorldPos, Vec3)],
) -> Option<f32> {
    let mut soonest: Option<f32> = None;
    let mut note = |left: f32| soonest = Some(soonest.map_or(left, |s| s.min(left)));
    if target.action.is_melee() && distance < THREAT_REACH && target.age < target.contact_end {
        note((target.contact_start - target.age).max(0.));
    }
    for (bolt_team, at, velocity) in bolts {
        let speed = velocity.length();
        if *bolt_team == team || speed <= 0. {
            continue;
        }
        let along_dir = velocity.as_dvec3() / f64::from(speed);
        let to = me.0 - at.0;
        let along = to.dot(along_dir);
        if along <= 0. || (to - along_dir * along).length() > BOLT_PATH {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, reason = "seconds, a few")]
        note((along / f64::from(speed)) as f32);
    }
    soonest
}

/// The game's own test: a ray from the shoulder towards the aim point,
/// stopped short of the target's capsule, hits nothing.
fn line_of_sight(
    spatial: &SpatialQuery,
    origin: &RenderOrigin,
    shooter: Entity,
    from: WorldPos,
    aim: WorldPos,
) -> bool {
    let shoulder = origin.to_render(from) + Vec3::Y * 0.45;
    let path = origin.to_render(aim) - shoulder;
    let Ok(direction) = Dir3::new(path) else {
        return true;
    };
    spatial
        .cast_ray(
            shoulder,
            direction,
            (path.length() - 0.8).max(0.),
            true,
            &SpatialQueryFilter::from_excluded_entities([shooter]),
        )
        .is_none()
}

/// The pressing of a side's abilities, which wraps whatever policy the side
/// has: after the policy has written its tick, a place of the bar is pressed
/// whenever the sim's own rule allows it and its condition holds. The
/// conditions are what a hand would look at; nothing here is a rule.
#[derive(Component, Debug, Clone)]
pub struct PressAbilities {
    /// Seconds before the first press: the slowest reaction a policy has
    /// ([`REACTION`]), so a first bolt is not launched before the
    /// opponent's body has been in the world a moment.
    wait: f32,
    /// A melee foe has been seen with its guard up in this bout, so a blast
    /// is held back for its openings and not for a foe that never guards.
    seen_guard: bool,
    /// Seconds left of the half second after the side last lost health or
    /// shield: it is being shot at.
    hit_left: f32,
    /// Health plus shield as last seen, to know a loss.
    pool: f32,
}

impl Default for PressAbilities {
    fn default() -> Self {
        Self {
            wait: REACTION.1,
            seen_guard: false,
            hit_left: 0.,
            pool: f32::MAX,
        }
    }
}

/// A magazine that is short to the policy: at most this share of its size
/// (a quarter), where a hand would rather reload than fire the last rounds.
const SHORT_MAGAZINE: (u16, u16) = (1, 4);

/// What a hand sees when it considers a press: how far the nearest enemy is,
/// whether a bolt can leave for it, and whether a bolt of the enemy's is
/// coming while the guard is up.
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent things a hand sees, each asked on its own"
)]
struct Sight {
    /// Metres to the nearest living enemy.
    metres: f64,
    /// A clear line of fire to it.
    clear: bool,
    /// An enemy bolt in the air, the guard raised.
    guarded_flight: bool,
    /// Hit within the last [`HIT_MEMORY`] seconds, or a bolt of the enemy's
    /// in the air.
    hit: bool,
    /// A bolt of the enemy's in the air, guard or none.
    incoming: bool,
    /// The nearest enemy holds a melee weapon, so it has a guard to wait for.
    bearer: bool,
    /// Its guard is up.
    guarded: bool,
    /// It has been seen guarding in this bout.
    guards: bool,
    /// The policy means to fire this tick with its aim not closed, or while
    /// it moves: the shot a tight cone would help.
    loose_shot: bool,
    /// The nearest enemy is beginning a cut (not yet in contact) and stands
    /// inside one and a half times its own reach.
    cut_coming: bool,
    /// The policy means to commit a slow cut (a heavy, or any cut of a class
    /// slower than a blade) with the enemy in its reach.
    slow_cut_due: bool,
}

/// How long a side counts itself as under fire after a hit, and the stretch
/// of extra distance a covered move is pressed from while it is.
const HIT_MEMORY: f32 = 0.5;
/// A covered move is pressed from this many times its usual range under fire.
const UNDER_FIRE_REACH: f64 = 1.5;

impl PressAbilities {
    /// Whether the condition of `row` holds: in reach of an enemy for a
    /// strike (a scaled one at the weapon's own reach), a clear line to one
    /// for a shot, a short magazine for a reload, a hurt shield for a
    /// recharge, the fight's start for a status and, for a status that
    /// reflects, the first bolt in the air at a raised guard. A movement
    /// along the aim is pressed from outside the weapon's reach and inside
    /// the distance plus the strike it ends in; one along the walked
    /// direction, the player's slide, only while a bolt is in the air.
    #[allow(
        clippy::unused_self,
        reason = "the rules are the presser's, kept a method beside its state"
    )]
    fn wants(&self, row: &AbilityRow, actor: &Combatant, sight: &Sight) -> bool {
        let weapon_reach =
            || strata_sim::combat_tables::with_melee(|melee| melee.reach_m(actor.weapon));
        let within = |metres: f32| sight.metres < f64::from(metres);
        match &row.effect {
            AbilityEffect::Cut(_) => within(crate::BLADE_RANGE),
            // A gun's butt strike is for a gunner caught in reach: against a
            // foe that guards it waits for the guard to be down or a cut to
            // begin, as a punishing gun waits to fire, so it is not spent on
            // a raised slab.
            AbilityEffect::Strike { reach, scaled, .. } => {
                // A melee hand does not press a strike into a raised guard:
                // the slab meets it and the commitment, longer than the
                // guard's patience, leaves the foe a cut for nothing.
                (!actor.weapon.is_melee() || !sight.guarded)
                    && within(if *scaled {
                        reach * weapon_reach()
                    } else {
                        *reach
                    })
                    && (actor.weapon.is_melee() || !sight.bearer || !sight.guarded)
            }
            // A blast comes off the ground and no guard meets it, so against
            // a foe that guards it is pressed while the guard is up and a
            // plain bolt would be wasted on it; a foe never seen guarding
            // (a rusher) gets it whenever the line is clear.
            AbilityEffect::Shot { blast, .. } => {
                sight.clear && (blast.is_none() || !(sight.bearer && sight.guards) || sight.guarded)
            }
            AbilityEffect::Hit { .. } => {
                if actor.weapon.is_melee() {
                    !sight.guarded && within(weapon_reach())
                } else {
                    sight.clear
                }
            }
            AbilityEffect::Move {
                distance,
                along: Along::Aim,
                then: Some(then),
                cover,
                ..
            } => {
                let reach = if then.scaled {
                    then.reach * weapon_reach()
                } else {
                    then.reach
                };
                let far = f64::from(distance + reach)
                    * if cover.is_some() && sight.hit {
                        UNDER_FIRE_REACH
                    } else {
                        1.
                    };
                sight.metres >= f64::from(weapon_reach()) && sight.metres < far
            }
            // A slide leaves the line of a bolt in the air, the way the hand
            // is already moving; with no bolt there is nothing to leave.
            AbilityEffect::Move {
                along: Along::Input,
                ..
            } => sight.incoming,
            AbilityEffect::Move { .. } => false,
            AbilityEffect::Own(Own::Reload) => {
                u32::from(actor.ammo) * u32::from(SHORT_MAGAZINE.1)
                    <= u32::from(actor.magazine()) * u32::from(SHORT_MAGAZINE.0)
            }
            AbilityEffect::Own(Own::Shield) => actor.shield < actor.shield_capacity,
            // A status is pressed when it can help, not at the start, and not
            // while it is already borne: a tight cone when the hand is about
            // to fire loose or on the move; a status that takes damage or
            // posture when a cut is coming at it or its own slow cut is about
            // to commit in reach; a reflection when a bolt meets the guard.
            AbilityEffect::Own(Own::Status { id, .. }) => {
                if actor.statuses().iter().any(|(borne, _)| borne == id) {
                    return false;
                }
                let tables = strata_sim::combat_tables::current();
                let Some(status) = tables.abilities.status(id) else {
                    return false;
                };
                if status.reflects.is_some() {
                    sight.guarded_flight
                } else if status.tight_cone {
                    sight.loose_shot
                } else if status.posture_taken.is_some() || status.damage_taken.is_some() {
                    sight.cut_coming || sight.slow_cut_due
                } else {
                    true
                }
            }
        }
    }
}

/// The reach in metres of the weapon `actor` holds.
fn reach_of(actor: &Combatant) -> f64 {
    f64::from(strata_sim::combat_tables::with_melee(|melee| {
        melee.reach_m(actor.weapon)
    }))
}

/// The nearest enemy is beginning a cut, not yet in contact, from inside
/// one and a half times its own reach: the blow a hand braces for.
fn cut_coming(foe: &Combatant, metres: f64) -> bool {
    foe.weapon.is_melee()
        && foe.action.is_melee()
        && foe.age < foe.spec(foe.action).contact_start
        && metres < 1.5 * reach_of(foe)
}

/// The hand means to commit a heavy with the enemy in its reach. Only a
/// heavy is slow enough to be worth bracing for: a light of a greatblade or
/// glaive pressed absorb before every first cut and starved the cuts of
/// stamina.
fn slow_cut_due(actor: &Combatant, heavy: bool, metres: f64) -> bool {
    actor.weapon.is_melee() && heavy && metres < reach_of(actor)
}

/// A cutting hand keeps the stamina of its next cut: three presses in a row
/// left a greatblade waiting on its breath in the foe's reach, slower and
/// bloodier than the same hand with an empty bar. A cut on the bar is itself
/// the next cut, and a gun has none to keep.
fn keeps_a_cut(actor: &Combatant, row: &AbilityRow) -> bool {
    !actor.weapon.is_melee()
        || matches!(row.effect, AbilityEffect::Cut(_))
        || actor.stamina - row.stamina >= actor.spec(Action::Light).stamina
}

/// Presses the places of every side that has abilities, after its policy
/// has written the tick and before the sim consumes the intents. One place
/// a tick, the first the rule allows and the condition wants, and only when
/// the actor is free, so a press is never left to the buffer.
pub fn press_abilities(
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    mut pressers: Query<(
        Entity,
        &mut PressAbilities,
        &mut CombatIntent,
        &Combatant,
        &WorldPosition,
        &mut Driver,
        &PlayerIntent,
    )>,
    others: Query<(Entity, &Combatant, &WorldPosition)>,
    bolts: Query<&BlasterBolt>,
) {
    for (entity, mut presser, mut input, actor, position, mut driver, movement) in &mut pressers {
        presser.wait -= time.delta_secs();
        presser.hit_left = (presser.hit_left - time.delta_secs()).max(0.);
        let pool = actor.health + actor.shield;
        if pool < presser.pool {
            presser.hit_left = HIT_MEMORY;
        }
        presser.pool = pool;
        if actor.action != Action::Ready || presser.wait > 0. {
            continue;
        }
        let nearest = others
            .iter()
            .filter(|(other, enemy, _)| {
                *other != entity && enemy.team != actor.team && enemy.action != Action::Dead
            })
            .min_by(|(_, _, a), (_, _, b)| {
                a.0.distance(position.0)
                    .total_cmp(&b.0.distance(position.0))
            });
        let Some((_, foe, enemy)) = nearest else {
            continue;
        };
        let metres = enemy.0.distance(position.0);
        let clear = line_of_sight(
            &spatial,
            &origin,
            entity,
            position.0,
            WorldPos(enemy.0.0 + DVec3::Y * 0.2),
        );
        // A hand that will cover itself with a move has its reflection up
        // for the bolts the cover meets, as a raised guard has.
        let size = strata_sim::combat_tables::current().abilities.bar;
        let covered = (0..size.weapon + size.utility).any(|index| {
            matches!(
                actor.ability_at(index).map(|row| row.effect),
                Some(AbilityEffect::Move { cover: Some(_), .. })
            )
        });
        presser.seen_guard |= foe.weapon.is_melee() && foe.guard_up;
        let sight = Sight {
            metres,
            clear,
            guarded_flight: (input.aim || covered)
                && actor.weapon.is_melee()
                && bolts.iter().any(|bolt| bolt.team != actor.team),
            hit: presser.hit_left > 0.,
            incoming: bolts.iter().any(|bolt| bolt.team != actor.team),
            bearer: foe.weapon.is_melee(),
            guarded: foe.guard_up,
            guards: presser.seen_guard,
            loose_shot: driver.ready_loose
                || input.fire
                    && (movement.move_dir.length_squared() > 0.
                        || !input.aim
                        || actor.aim_share() < 1.),
            cut_coming: cut_coming(foe, metres),
            slow_cut_due: slow_cut_due(actor, input.heavy, metres),
        };
        for index in 0..size.weapon + size.utility {
            let Ok(row) = actor.can_use(index) else {
                continue;
            };
            if !presser.wants(&row, actor, &sight) {
                continue;
            }
            // A cutting hand's presses of a cut, strike or hit run on the
            // clock of its cuts, so the seed that spreads them spreads these.
            let cutting = cutting_pause(&driver.policy);
            let melee_press = matches!(
                row.effect,
                AbilityEffect::Cut(_) | AbilityEffect::Strike { .. } | AbilityEffect::Hit { .. }
            );
            if cutting.is_some() && melee_press && !driver.hand_free() {
                continue;
            }
            if !keeps_a_cut(actor, &row) {
                continue;
            }
            input.ability = Some(index);
            if let Some(base) = cutting {
                driver.pause = driver.next_pause(base);
            }
            // A hand presses one thing: the policy's own cut or pull of
            // this tick gives way to the ability. An instant press commits
            // the hand to nothing, so the pull stands beside it; taking it
            // away cost a punishing rifle the one opening it had waited for.
            if row.length > 0. {
                input.fire = false;
                input.light = false;
                input.heavy = false;
            }
            break;
        }
    }
}

/// Every driver writes its intents for this tick. Runs in `SimSet::Input`
/// before the intents are consumed, where the game's AI runs.
#[allow(
    clippy::too_many_lines,
    reason = "one match over the policies, each a handful of rules"
)]
pub fn drive(
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    bolts: Query<&BlasterBolt>,
    mut drivers: Query<(
        Entity,
        &mut Driver,
        &mut PlayerIntent,
        &mut CombatIntent,
        &mut Combatant,
        &WorldPosition,
        &CharacterController,
    )>,
) {
    let dt = time.delta_secs();
    // What everyone can see of everyone, read before anyone acts, so the
    // order the drivers run in does not show in what they saw.
    let seen: Vec<(Entity, u8, Target)> = drivers
        .iter()
        .map(|(entity, _, _, _, actor, at, motor)| {
            (
                entity,
                actor.team,
                Target {
                    position: at.0,
                    velocity: motor.horizontal_velocity,
                    action: actor.action,
                    contact_start: actor.spec(actor.action).contact_start,
                    contact_end: actor.spec(actor.action).contact_end,
                    age: actor.age,
                    shots: actor.shots,
                    charge: actor.charge,
                    looks: Seen::of(actor, 0.),
                },
            )
        })
        .collect();
    let flights: Vec<(u8, WorldPos, Vec3)> = bolts
        .iter()
        .map(|bolt| (bolt.team, bolt.position, bolt.velocity))
        .collect();
    for (entity, mut driver, mut movement, mut input, mut actor, position, _) in &mut drivers {
        let tick = driver.tick;
        driver.tick += 1;
        // Idle time (a reload) is not owed back as a volley of pulls.
        driver.pull_wait = (driver.pull_wait - dt).max(-dt);
        if let Policy::Scripted { .. } = driver.policy {
            // The last row at or before this tick holds; its edges fire on
            // its own tick only, as a key press would.
            let index = driver.script.partition_point(|row| row.tick <= tick);
            if let Some(row) = index.checked_sub(1).map(|i| driver.script[i]) {
                *movement = row.movement;
                let edges = row.tick == tick;
                let named = row
                    .family
                    .and_then(|family| actor.slots().iter().position(|slot| *slot == Some(family)));
                let weapon = input
                    .weapon
                    .or(row.combat.weapon.or(named).filter(|_| edges));
                *input = CombatIntent {
                    light: row.combat.light && edges,
                    heavy: row.combat.heavy && edges,
                    dodge: row.combat.dodge && edges,
                    reload: row.combat.reload && edges,
                    weapon,
                    ..row.combat
                };
            }
            continue;
        }
        *movement = PlayerIntent::default();
        // A draw request is held, not an edge, as the game's script keeps it.
        let weapon = input.weapon;
        *input = CombatIntent::default();
        input.weapon = weapon;
        if actor.action == Action::Dead {
            continue;
        }
        let target = seen
            .iter()
            .filter(|(other, team, enemy)| {
                *other != entity && *team != actor.team && enemy.action != Action::Dead
            })
            .min_by(|(_, _, a), (_, _, b)| {
                a.position
                    .distance(position.0)
                    .total_cmp(&b.position.distance(position.0))
            })
            .map(|(_, _, target)| target);
        let Some(target) = target else {
            continue;
        };
        let driver = &mut *driver;
        if let Some(brain) = &mut driver.brain {
            // The game's opponent sees its target's feet and decides the
            // rest itself, on the installed row, as the encounter does.
            let tables = strata_sim::combat_tables::current();
            let ground = target.position.0 - position.0.0;
            #[allow(clippy::cast_possible_truncation, reason = "metres, render range")]
            let looks = Seen {
                distance: ground.x.hypot(ground.z) as f32,
                ..target.looks
            };
            brain.drive(
                dt,
                position.0,
                target.position,
                &looks,
                &tables.opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            continue;
        }
        driver.cooldown -= dt;
        driver.strafe_left -= dt;
        if driver.strafe_left <= 0. {
            driver.strafe = if driver.rng.bool() { 1. } else { -1. };
            let next = driver.rng.range_f64(1.5, 3.5);
            #[allow(clippy::cast_possible_truncation, reason = "seconds, a few")]
            {
                driver.strafe_left = next as f32;
            }
        }
        let delta = target.position.0 - position.0.0;
        let distance = delta.x.hypot(delta.z);
        // A gun leads a moving target by the bolt's flight, as the game's
        // script does; a blade aims at where the opponent is.
        let lead = if driver.ranged() {
            target.velocity.as_dvec3() * (distance / f64::from(actor.bolt_speed)).min(LEAD_CAP)
        } else {
            DVec3::ZERO
        };
        input.aim_point = WorldPos(target.position.0 + lead + DVec3::Y * 0.2);
        movement.yaw = yaw_to(input.aim_point.0 - position.0.0);
        #[allow(clippy::cast_possible_truncation, reason = "metres, render range")]
        let distance = distance as f32;
        // A closer sprints until it is in range, then plays its inner policy.
        let mut policy = driver.policy.clone();
        while let Policy::Close { within, then } = policy {
            if distance > within {
                movement.move_dir.y = 1.;
                movement.sprint = true;
                policy = Policy::Stand;
                break;
            }
            policy = *then;
        }
        match policy {
            Policy::HoldAndFire {
                range,
                cadence,
                aim,
            } => {
                // A tight cone spares the hand its aim: it fires on the move.
                input.aim = aim && !actor.focused();
                let clear = line_of_sight(&spatial, &origin, entity, position.0, input.aim_point);
                if distance > range * 1.5 || !clear {
                    movement.move_dir.y = 0.8;
                } else if distance < range * 0.6 {
                    movement.move_dir.y = -0.7;
                } else {
                    movement.move_dir.x = 0.6 * driver.strafe;
                }
                if actor.ammo == 0 {
                    input.reload = true;
                } else if clear
                    && distance < range * 2.5
                    && actor.action == Action::Ready
                    && driver.cooldown <= 0.
                {
                    input.fire = driver.pull(cadence);
                }
            }
            Policy::Aim { cadence } => {
                input.aim = true;
                let clear = line_of_sight(&spatial, &origin, entity, position.0, input.aim_point);
                if actor.ammo == 0 {
                    input.reload = true;
                } else if clear
                    && actor.action == Action::Ready
                    && actor.aim_share() >= 1.
                    && driver.cooldown <= 0.
                {
                    input.fire = driver.pull(cadence);
                }
            }
            Policy::StopAndShoot { walk, stand } => {
                driver.ready_loose = false;
                if actor.shots != driver.own_shots {
                    driver.own_shots = actor.shots;
                    driver.walking = walk;
                    driver.stood = 0.;
                }
                let clear = line_of_sight(&spatial, &origin, entity, position.0, input.aim_point);
                if actor.focused() {
                    // The cone is closed whatever the hand does, so standing
                    // to close it is the cost the status spares: it crosses
                    // the line and fires as the weapon is ready.
                    movement.move_dir.x = 0.8 * driver.strafe;
                    if actor.ammo == 0 {
                        input.reload = true;
                    } else if clear
                        && actor.action == Action::Ready
                        && driver.cooldown <= 0.
                        && actor.ready_for_press(dt)
                    {
                        input.fire = true;
                    }
                } else if driver.walking > 0. {
                    driver.walking -= dt;
                    movement.move_dir.x = 0.8 * driver.strafe;
                    driver.ready_loose = clear
                        && actor.ammo > 0
                        && actor.action == Action::Ready
                        && driver.cooldown <= 0.
                        && actor.ready_for_press(dt);
                } else {
                    input.aim = true;
                    if actor.aim_share() >= 1. {
                        driver.stood += dt;
                    }
                    // Ready to fire but held back by the stand: a shot a
                    // tight cone would let go at once.
                    driver.ready_loose = clear
                        && actor.ammo > 0
                        && actor.action == Action::Ready
                        && driver.cooldown <= 0.
                        && actor.ready_for_press(dt)
                        && driver.stood < stand;
                    if actor.ammo == 0 {
                        input.reload = true;
                    } else if clear
                        && actor.action == Action::Ready
                        && driver.cooldown <= 0.
                        && driver.stood >= stand
                    {
                        input.fire = true;
                    }
                }
            }
            Policy::CloseAndCut {
                chain,
                heavy_every,
                cooldown,
                reach,
                wait_stamina,
                sprint,
            } => {
                let heavy_due = heavy_every > 0 && (driver.openings + 1) % heavy_every == 0;
                let short =
                    wait_stamina && heavy_due && actor.stamina < actor.spec(Action::Heavy).stamina;
                let stop = (reach * driver.step).min(reach);
                // The next opening is counted from the end of a cut, so a
                // pause of nought still leaves the lag of a hand's press.
                if actor.action != Action::Ready {
                    driver.cooldown = driver.cooldown.max(driver.pause);
                }
                if distance > stop {
                    movement.move_dir.y = 1.;
                    movement.sprint |= sprint && distance > reach * 2.;
                } else {
                    if !driver.reached {
                        driver.reached = true;
                        driver.cooldown = driver.cooldown.max(driver.arrival);
                    }
                    if actor.action == Action::Ready && driver.cooldown <= 0. && !short {
                        driver.openings += 1;
                        if heavy_every > 0 && driver.openings % heavy_every == 0 {
                            input.heavy = true;
                        } else {
                            input.light = true;
                            driver.chained = false;
                            driver.chain_lag = if chain { driver.draw(CHAIN_LAG) } else { 0. };
                        }
                        driver.pause = driver.next_pause(cooldown);
                    }
                }
                // The return cut is queued once the opening is committed,
                // which is when a player would press again.
                if chain
                    && actor.action == Action::Light
                    && actor.age >= actor.spec(Action::Light).contact_start + driver.chain_lag
                    && !driver.chained
                {
                    input.light = true;
                    driver.chained = true;
                }
            }
            Policy::GuardAndCut {
                chain,
                heavy_every,
                cooldown,
                reach,
                parry,
                reaction,
                sprint,
            } => {
                let ready = actor.action == Action::Ready;
                // The next opening is counted from the end of a cut, so
                // the guard has a stretch to be up in.
                if !ready {
                    driver.cooldown = driver.cooldown.max(driver.pause);
                }
                let threat = landing_in(position.0, actor.team, target, distance, &flights);
                driver.seen = if threat.is_some() {
                    driver.seen + dt
                } else {
                    0.
                };
                let lead = if threat.is_some() {
                    if driver.lead.is_none() {
                        let draw = driver.rng.range_f64(LEAD_SPREAD.0, LEAD_SPREAD.1);
                        #[allow(clippy::cast_possible_truncation, reason = "a share")]
                        {
                            driver.lead = Some(reaction * draw as f32);
                        }
                    }
                    driver.lead.unwrap_or(0.)
                } else {
                    driver.lead = None;
                    0.
                };
                let covering = threat.is_some_and(|left| left <= lead) && driver.seen >= REACTION.0;
                let mut cutting = false;
                let sprinting = sprint && distance > reach * 2.;
                if distance > (reach * driver.step).min(reach) {
                    movement.move_dir.y = 1.;
                    movement.sprint |= sprinting;
                    // A guard that has walked in waits behind it for the
                    // opponent's blow before its first opening.
                    if !driver.arrived {
                        driver.cooldown = driver.cooldown.max(driver.pause);
                    }
                } else {
                    if !driver.arrived {
                        // The guard's wait behind its shield is already
                        // running; the arrival is the look before it commits.
                        driver.cooldown += driver.arrival;
                    }
                    driver.arrived = true;
                    if ready && driver.cooldown <= 0. && threat.is_none() {
                        driver.openings += 1;
                        if heavy_every > 0 && driver.openings % heavy_every == 0 {
                            input.heavy = true;
                        } else {
                            input.light = true;
                            driver.chained = false;
                            driver.chain_lag = if chain { driver.draw(CHAIN_LAG) } else { 0. };
                        }
                        driver.pause = driver.next_pause(cooldown);
                        driver.cooldown = driver.pause;
                        cutting = true;
                    }
                }
                if chain
                    && actor.action == Action::Light
                    && actor.age >= actor.spec(Action::Light).contact_start + driver.chain_lag
                    && !driver.chained
                {
                    input.light = true;
                    driver.chained = true;
                }
                input.aim = ready && !cutting && !sprinting && (!parry || covering);
            }
            Policy::Kite {
                dodge_on_telegraph,
                range,
            } => {
                input.aim = true;
                let clear = line_of_sight(&spatial, &origin, entity, position.0, input.aim_point);
                movement.move_dir.x = driver.strafe;
                if distance > range * 1.5 || !clear {
                    movement.move_dir.y = 0.8;
                } else if distance < range * 0.6 {
                    movement.move_dir.y = -0.7;
                }
                let awake = driver.cooldown <= 0.;
                if actor.ammo == 0 {
                    input.reload = true;
                } else if clear && awake && actor.action == Action::Ready {
                    input.fire = true;
                }
                let dodge = driver.dodge_due(target, distance, awake, dt);
                if dodge_on_telegraph && dodge {
                    input.dodge = true;
                    movement.move_dir = bevy::math::Vec2::new(driver.strafe, 0.);
                }
            }
            Policy::Brace { dodge_on_telegraph } => {
                let awake = driver.cooldown <= 0.;
                let dodge = driver.dodge_due(target, distance, awake, dt);
                if dodge_on_telegraph && dodge {
                    input.dodge = true;
                    movement.move_dir = bevy::math::Vec2::new(driver.strafe, 0.);
                }
            }
            Policy::Punish { range } => {
                input.aim = true;
                let guarded = target.looks.melee && target.looks.aiming;
                let clear = line_of_sight(&spatial, &origin, entity, position.0, input.aim_point);
                if distance < range {
                    movement.move_dir.y = -0.7;
                }
                // A cut begun in its reach is dodged once, a reaction after
                // it shows, and the next one after the target has been out
                // of its cut.
                let reach = strata_sim::combat_tables::current().opponents.bulwark.reach;
                if target.action.is_melee() && distance < reach * 1.5 {
                    let telegraph = driver.telegraph.get_or_insert_with(|| {
                        let left = driver.rng.range_f64(
                            f64::from(REACTION.0),
                            f64::from(REACTION.0 + REACTION.1) * 0.5,
                        );
                        #[allow(clippy::cast_possible_truncation, reason = "seconds, a few")]
                        Telegraph {
                            left: left as f32,
                            answered: false,
                        }
                    });
                    telegraph.left -= dt;
                    if telegraph.left <= 0. && !telegraph.answered {
                        telegraph.answered = true;
                        input.dodge = true;
                        movement.move_dir = bevy::math::Vec2::new(0.5 * driver.strafe, -0.8);
                    }
                } else if !target.action.is_melee() {
                    driver.telegraph = None;
                }
                if guarded {
                    if actor.ammo == 0 {
                        input.reload = true;
                    }
                } else if clear
                    && actor.ammo > 0
                    && actor.action == Action::Ready
                    && actor.aim_share() >= 1.
                    && driver.cooldown <= 0.
                {
                    input.fire = driver.pull(crate::tap_of(actor.weapon));
                }
            }
            Policy::Stand => {}
            Policy::Sentry
            | Policy::Rusher
            | Policy::Marksman
            | Policy::Duelist
            | Policy::Bulwark
            | Policy::Close { .. }
            | Policy::Scripted { .. } => {
                unreachable!("handled above")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_sim::combat::Weapon;

    #[test]
    fn a_script_row_holds_until_the_next_and_its_edges_fire_once() {
        let rows = ScriptRow::parse_csv(
            "tick,move_x,move_y,yaw,sprint,fire,aim,light,heavy,dodge,reload,weapon\n\
             5,0,1,0.5,0,1,1,1,0,0,0,Pistol\n\
             2,1,0,0,1,0,0,0,0,0,0,\n",
        )
        .expect("parses");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tick, 2, "sorted");
        assert!(rows[0].movement.sprint);
        assert_eq!(rows[1].family, Some(strata_sim::combat::Weapon::Pistol));
        assert!(rows[1].combat.light);
        assert!(ScriptRow::parse_csv("1,2,3").is_err());
    }

    #[test]
    fn yaw_faces_the_delta() {
        assert!(yaw_to(DVec3::new(0., 0., -1.)).abs() < 1e-6);
        assert!((yaw_to(DVec3::new(-1., 0., 0.)) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    fn calm() -> Sight {
        Sight {
            metres: 10.,
            clear: true,
            guarded_flight: false,
            hit: false,
            incoming: false,
            bearer: false,
            guarded: false,
            guards: false,
            loose_shot: false,
            cut_coming: false,
            slow_cut_due: false,
        }
    }

    /// A fresh actor with `weapon` in its hand.
    fn bearing(weapon: Weapon) -> Combatant {
        Combatant::default().carrying([Some(weapon), None], 0)
    }

    fn row(id: &str) -> AbilityRow {
        strata_sim::combat_tables::current()
            .abilities
            .row(id)
            .expect("a shipped ability")
            .clone()
    }

    #[test]
    fn a_tight_cone_is_pressed_for_a_loose_shot_and_not_while_borne() {
        let mut actor = Combatant::default();
        let presser = PressAbilities::default();
        let focus = row("focus");
        assert!(!presser.wants(&focus, &actor, &calm()), "nothing to fire");
        let loose = Sight {
            loose_shot: true,
            ..calm()
        };
        assert!(presser.wants(&focus, &actor, &loose));
        actor.give_status("focused", 5.);
        assert!(!presser.wants(&focus, &actor, &loose), "already borne");
    }

    #[test]
    fn a_fortifying_status_is_pressed_for_a_cut_coming_or_a_slow_cut_due() {
        let actor = Combatant::default();
        let presser = PressAbilities::default();
        let absorb = row("kinetic_absorb");
        assert!(!presser.wants(&absorb, &actor, &calm()), "not at the start");
        for sight in [
            Sight {
                cut_coming: true,
                ..calm()
            },
            Sight {
                slow_cut_due: true,
                ..calm()
            },
        ] {
            assert!(presser.wants(&absorb, &actor, &sight));
        }
    }

    #[test]
    fn a_cut_is_coming_only_before_its_contact_and_inside_its_reach() {
        let mut foe = bearing(Weapon::Blade);
        assert!(!cut_coming(&foe, 1.), "a foe that stands ready");
        foe.action = Action::Light;
        foe.age = 0.;
        assert!(cut_coming(&foe, 1.));
        assert!(!cut_coming(&foe, 1.5 * reach_of(&foe) + 0.1), "too far");
        foe.age = foe.spec(Action::Light).contact_start;
        assert!(!cut_coming(&foe, 1.), "the blow is already landing");
        let gunner = bearing(Weapon::Carbine);
        assert!(!cut_coming(&gunner, 1.), "a gun cuts nothing");
    }

    #[test]
    fn only_a_heavy_in_reach_is_a_slow_cut_due() {
        let blade = bearing(Weapon::Greatblade);
        assert!(slow_cut_due(&blade, true, 1.));
        assert!(
            !slow_cut_due(&blade, false, 1.),
            "a light is not braced for"
        );
        assert!(!slow_cut_due(&blade, true, reach_of(&blade) + 0.1));
        let gunner = bearing(Weapon::Carbine);
        assert!(!slow_cut_due(&gunner, true, 1.));
    }

    #[test]
    fn a_cutting_hand_keeps_the_stamina_of_its_next_cut() {
        let mut blade = bearing(Weapon::Greatblade);
        let absorb = row("kinetic_absorb");
        let light = blade.spec(Action::Light).stamina;
        blade.stamina = absorb.stamina + light + 1.;
        assert!(keeps_a_cut(&blade, &absorb));
        blade.stamina = absorb.stamina + light - 1.;
        assert!(
            !keeps_a_cut(&blade, &absorb),
            "the press would cost the cut"
        );
        assert!(keeps_a_cut(&blade, &row("heavy_cut")), "a cut is the cut");
        let mut gunner = bearing(Weapon::Carbine);
        gunner.stamina = 0.;
        assert!(
            keeps_a_cut(&gunner, &row("butt_strike")),
            "a gun keeps none"
        );
    }

    #[test]
    fn a_reflection_waits_for_a_bolt_at_the_guard() {
        let actor = Combatant::default();
        let presser = PressAbilities::default();
        let reflect = row("reflect_shot");
        assert!(!presser.wants(&reflect, &actor, &calm()));
        let flight = Sight {
            guarded_flight: true,
            ..calm()
        };
        assert!(presser.wants(&reflect, &actor, &flight));
    }

    #[test]
    fn a_gunners_butt_strike_waits_for_a_guard_to_be_down() {
        let actor = Combatant::default();
        let presser = PressAbilities::default();
        let butt = row("butt_strike");
        let shut = Sight {
            metres: 1.,
            bearer: true,
            guarded: true,
            ..calm()
        };
        assert!(!presser.wants(&butt, &actor, &shut));
        let open = Sight {
            guarded: false,
            ..shut
        };
        assert!(presser.wants(&butt, &actor, &open));
    }
}
