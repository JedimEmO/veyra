//! One bout: a headless Bevy app with the real sim, streamer and physics
//! plugins (the set the physics crate's own combat tests build), a flat
//! floor, the combatants, and the schedule stepped at exactly 60 Hz with no
//! wall clock in it, so a bout runs as fast as the CPU goes and two runs
//! with the same seed and tables step the same bits.

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use bevy::transform::components::Transform;
use serde::{Deserialize, Serialize};
use strata_core::SimTime;
use strata_core::{Rng, WorldPos};
use strata_physics::avian3d::prelude::{Collider, RigidBody};
use strata_physics::{CharacterController, PhysicsPlugin};
use strata_registry::item::Fitted;
use strata_sim::ability::{Bar, BarRequest, Place, SlotKind};
use strata_sim::combat::{
    Action, BlasterBolt, CombatImpact, CombatSet, Combatant, GuardOutcome, Weapon,
};
use strata_sim::combat_curves::MeleeCurves;
use strata_sim::{Player, SimClock, SimPlugin, SimSet, SimTick, WorldPosition};
use strata_stream::StreamPlugin;

use crate::policy::{Driver, Policy, PressAbilities};

/// Ticks per second; the game's combat study rate.
pub const HZ: f64 = 60.;

/// Whose pools and speeds a combatant has: the player's from `actor.ron`,
/// or an opponent's from `opponents.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    /// The player: full pools, full damage, the game's motor.
    Player,
    /// A passive training post: the player's pools and motor, and the
    /// health of the game's training target (`opponents.passive_health`).
    Post,
    /// The encounter's melee opponent.
    Rusher,
    /// The encounter's ranged opponent, with its slow bolt.
    Sentry,
    /// The long-range opponent, fitted from its sheet as the game fits it.
    Marksman,
    /// The guarding melee opponent, fitted from its sheet as the game fits it.
    Duelist,
    /// The shield bearer, fitted from its sheet and given its own guard as
    /// the game does.
    Bulwark,
}

/// One side of a matchup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Side {
    /// What its inputs do.
    pub policy: Policy,
    /// The weapon in hand at the start; the other slot holds [`Side::holstered`].
    pub weapon: Weapon,
    /// Whose numbers it has.
    pub profile: Profile,
    /// How many of them, side by side; one is a duel.
    #[serde(default = "one")]
    pub count: u32,
    /// The boxes on its sheet, as the graph spells them (`blade.cuts.1`),
    /// fitted at spawn. Empty, the side is unfitted and reads the
    /// installed tables live, free to do everything, as the presets did
    /// before the trees existed.
    #[serde(default)]
    pub boxes: Vec<String>,
    /// The modules on its weapons, equipped at spawn after the boxes, as
    /// the game merges them. Empty, the side is not equipped at all and
    /// reads its rows as every side did before fitting existed.
    #[serde(default)]
    pub fitted: Vec<SideMod>,
    /// The condition its weapon starts the bout in, `0..=1000`. None, the
    /// weapon is as made and, with nothing fitted either, the side is not
    /// equipped at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<u16>,
    /// The weapon's own quality lines, by id, on a record of its own: a
    /// bought example (every line at store grade) with these set. None,
    /// the weapon has no record unless a module or a condition gives it
    /// one, and `Some` of nothing is the bought weapon itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<BTreeMap<String, u16>>,
    /// The abilities it knows, by id, slotted in order on its weapon's
    /// places (or the utility's) and pressed by [`crate::policy::PressAbilities`]
    /// whenever the rule allows and their condition holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<String>,
    /// Further members of this side beside its own, each with its own
    /// policy, weapon, profile and count, stood in the same line after it:
    /// a camp's mixed people. Their own `with` is not read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<Side>,
    /// The tier the side's opponents are scaled by, as the game's camps
    /// are (`strata_sim::combat_tiers::opponent_pools`); 0 reads
    /// `opponents.ron` as written. A player's pools are never scaled.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tier: u8,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(tier: &u8) -> bool {
    *tier == 0
}

impl Side {
    /// The weapon in the other slot: the pistol beside a carbine, the
    /// carbine beside anything else. The harness fights with the one in
    /// hand, so this only fills the slot.
    #[must_use]
    pub const fn holstered(&self) -> Weapon {
        match self.weapon {
            Weapon::Carbine => Weapon::Pistol,
            _ => Weapon::Carbine,
        }
    }
}

/// A module on one of a side's weapons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SideMod {
    /// The weapon it is fitted to.
    pub weapon: Weapon,
    /// The module, with its quality lines.
    pub module: Fitted,
}

fn one() -> u32 {
    1
}

/// Two sides and the metres between them at the start, facing each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Matchup {
    /// Its name in the CSV.
    pub name: String,
    /// Team 0, at the origin, facing `-z`.
    pub a: Side,
    /// Team 1, `distance` metres down `-z`, facing back.
    pub b: Side,
    /// The starting separation.
    pub distance: f32,
}

/// What one side did in a bout.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SideStats {
    /// Health and shield taken off the other side.
    pub dealt: f32,
    /// Health and shield lost.
    pub taken: f32,
    /// Bolts fired.
    pub shots: u64,
    /// Bolts that struck an enemy.
    pub hits: u64,
    /// Cuts begun: a Light, its return and a Heavy each count.
    pub cuts: u64,
    /// Cuts that struck an enemy.
    pub cuts_landed: u64,
    /// The lowest stamina any member reached.
    pub stamina_min: f32,
    /// The lowest posture any member reached: how near a chain came to a
    /// break it did not get.
    pub posture_min: f32,
    /// Posture breaks suffered.
    pub breaks: u64,
    /// Dodges begun.
    pub dodges: u64,
    /// Dodges during which an enemy's bolt or blade would have landed.
    pub dodges_avoided: u64,
    /// Hits a raised guard took a share of.
    pub blocks: u64,
    /// Hits a guard raised in time turned aside whole.
    pub parries: u64,
    /// Guards that ran out of stamina and broke.
    pub guard_breaks: u64,
    /// Abilities begun: a press the rule allowed, whatever it did. A cut
    /// ability (the heavy cut) begins its cut and counts under `cuts`.
    pub abilities: u64,
    /// The bout second of the first cut that struck an enemy, or `None`
    /// when no cut landed.
    pub first_cut: Option<f32>,
}

/// How a bout ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winner {
    /// Side `a` has someone standing, side `b` no one.
    A,
    /// The reverse.
    B,
    /// Both standing at the time cap, or neither.
    Draw,
}

impl Winner {
    /// The CSV spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::Draw => "draw",
        }
    }
}

/// The outcome of one bout.
#[derive(Debug, Clone, PartialEq)]
pub struct BoutResult {
    /// Who won.
    pub winner: Winner,
    /// Seconds until the last of the losing side fell, or the cap.
    pub time: f32,
    /// Side `a` then side `b`.
    pub sides: [SideStats; 2],
}

/// What the recorder knew of an actor last tick.
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    entity: Entity,
    health: f32,
    shield: f32,
    sequence: u64,
    shots: u64,
    /// Set while the actor is in a dodge: whether it has avoided anything.
    dodging: Option<bool>,
}

/// Per-tick bookkeeping over the actors and the impact messages. A plain
/// `Vec` rather than a map so iteration order is spawn order on every run.
#[derive(Resource, Default)]
struct Recorder {
    previous: Vec<Snapshot>,
    sides: [SideStats; 2],
}

impl Recorder {
    fn side(&mut self, team: u8) -> &mut SideStats {
        &mut self.sides[usize::from(team.min(1))]
    }
}

/// Melee contact that a dodge would have been inside: the blade's reach
/// plus the capsule and the sweep's slack.
const BLADE_REACH: f32 = 2.6;

/// A bolt passing closer than this to a dodging capsule's centre was
/// going to hit it: the capsule's radius, the bolt's, and a step's slack.
const BOLT_MISS: f64 = 1.0;

/// The distance from `point` to the segment `a..b`.
fn segment_distance(point: WorldPos, a: WorldPos, b: WorldPos) -> f64 {
    let ab = b.0 - a.0;
    let length = ab.length_squared();
    let t = if length > 0. {
        ((point.0 - a.0).dot(ab) / length).clamp(0., 1.)
    } else {
        0.
    };
    point.0.distance(a.0 + ab * t)
}

/// Reads the tick's impacts and the actors' state changes into the stats.
#[allow(
    clippy::too_many_lines,
    reason = "one pass over the actors with every counter in it"
)]
fn record(
    time: Res<Time<Fixed>>,
    mut recorder: ResMut<Recorder>,
    mut impacts: MessageReader<CombatImpact>,
    bolts: Query<&BlasterBolt>,
    actors: Query<(Entity, &Combatant, &WorldPosition)>,
) {
    let dt = time.delta_secs();
    let team_of = |entity: Entity| actors.get(entity).map(|(_, actor, _)| actor.team).ok();
    // Every bolt's path this tick, for the dodges that made one miss.
    let flights: Vec<(u8, WorldPos, WorldPos)> = bolts
        .iter()
        .map(|bolt| (bolt.team, bolt.previous, bolt.position))
        .collect();
    // Hits by attacker team, and who struck each victim this tick.
    let mut struck: Vec<(Entity, u8)> = Vec::new();
    let mut misses: Vec<(u8, WorldPos)> = Vec::new();
    for impact in impacts.read() {
        let Some(team) = team_of(impact.attacker) else {
            continue;
        };
        match impact.victim {
            Some(victim) => {
                struck.push((victim, team));
                // The guard's book is the one who guarded: a parried
                // blow did not land, and so is no hit of the attacker's.
                if let Some(guard) = team_of(victim) {
                    let side = recorder.side(guard);
                    match impact.guard {
                        GuardOutcome::Blocked => side.blocks += 1,
                        GuardOutcome::Parried => side.parries += 1,
                        GuardOutcome::Broken => side.guard_breaks += 1,
                        GuardOutcome::Open => {}
                    }
                }
                let side = recorder.side(team);
                if impact.guard != GuardOutcome::Parried {
                    if impact.melee {
                        side.cuts_landed += 1;
                        side.first_cut.get_or_insert(time.elapsed_secs());
                    } else {
                        side.hits += 1;
                    }
                }
            }
            None => misses.push((team, impact.point)),
        }
    }
    let mut previous = std::mem::take(&mut recorder.previous);
    for (entity, actor, position) in &actors {
        let index = previous
            .iter()
            .position(|snapshot| snapshot.entity == entity);
        // First sight is the spawn: nothing done yet, so what the first
        // tick did counts.
        let index = index.unwrap_or_else(|| {
            previous.push(Snapshot {
                entity,
                health: actor.health,
                shield: actor.shield,
                sequence: 0,
                shots: 0,
                dodging: None,
            });
            let side = recorder.side(actor.team);
            side.stamina_min = actor.stamina;
            side.posture_min = actor.posture;
            previous.len() - 1
        });
        let last = previous[index];
        let pools_lost = (last.health + last.shield) - (actor.health + actor.shield);
        if pools_lost > 0. {
            recorder.side(actor.team).taken += pools_lost;
            let attackers: Vec<u8> = struck
                .iter()
                .filter(|(victim, _)| *victim == entity)
                .map(|(_, team)| *team)
                .collect();
            if attackers.is_empty() {
                recorder.side(1 - actor.team.min(1)).dealt += pools_lost;
            } else {
                #[allow(clippy::cast_precision_loss, reason = "a handful of attackers")]
                let share = pools_lost / attackers.len() as f32;
                for team in attackers {
                    recorder.side(team).dealt += share;
                }
            }
        }
        let side = recorder.side(actor.team);
        side.shots += actor.shots.wrapping_sub(last.shots);
        side.stamina_min = side.stamina_min.min(actor.stamina);
        side.posture_min = side.posture_min.min(actor.posture);
        let mut dodging = last.dodging;
        if actor.sequence != last.sequence {
            if actor.action.is_melee() {
                side.cuts += 1;
            }
            match actor.action {
                Action::Dodge => {
                    side.dodges += 1;
                    dodging = Some(false);
                }
                Action::Stagger => side.breaks += 1,
                Action::Ability => side.abilities += 1,
                _ => {}
            }
        }
        if actor.action == Action::Dodge {
            let here = position.0;
            let was_hit = struck.iter().any(|(victim, _)| *victim == entity);
            // A bolt that struck the capsule while it was immune leaves an
            // impact with no victim at the capsule, and one the dodge
            // stepped out of passes close by; a blade that swept through
            // the capsule leaves nothing, so the sweep is checked directly.
            let bolt = misses
                .iter()
                .any(|(team, point)| *team != actor.team && point.distance(here) < BOLT_MISS)
                || flights.iter().any(|(team, from, to)| {
                    *team != actor.team && segment_distance(here, *from, *to) < BOLT_MISS
                });
            let blade = !was_hit
                && actors.iter().any(|(_, enemy, at)| {
                    enemy.team != actor.team
                        && enemy.action.is_melee()
                        && enemy.age >= enemy.spec(enemy.action).contact_start
                        && enemy.age < enemy.spec(enemy.action).contact_end + dt
                        && at.0.distance(here) < f64::from(BLADE_REACH)
                });
            if bolt || blade {
                dodging = Some(true);
            }
        } else if dodging.take() == Some(true) {
            side.dodges_avoided += 1;
        }
        previous[index] = Snapshot {
            entity,
            health: actor.health,
            shield: actor.shield,
            sequence: actor.sequence,
            shots: actor.shots,
            dodging,
        };
    }
    recorder.previous = previous;
}

/// The app the physics crate's combat tests build, plus the drivers and
/// the recorder in the tick.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        SimPlugin,
        StreamPlugin,
        PhysicsPlugin,
    ));
    app.insert_resource(Time::<Fixed>::from_hz(HZ));
    // The sim's own clock at the same rate, as the game's combat study
    // sets it, so a system that reads it counts the same seconds.
    app.insert_resource(SimClock(SimTime::from_hz(HZ)));
    // Every update is exactly one fixed step; no clock is read.
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(
        MeleeCurves::parse(include_str!("../../../assets/characters/combat/melee.ron"))
            .expect("the shipped blade curves parse"),
    );
    app.init_resource::<Recorder>();
    app.add_systems(
        SimTick,
        crate::policy::drive
            .in_set(SimSet::Input)
            .before(strata_sim::player::consume_player_intents),
    );
    // After the return cut has begun, so a tick's actions are read once
    // the sim has settled them.
    app.add_systems(
        SimTick,
        crate::policy::press_abilities
            .in_set(SimSet::Input)
            .after(crate::policy::drive)
            .before(strata_sim::player::consume_player_intents),
    );
    app.add_systems(
        SimTick,
        record.in_set(SimSet::Observe).after(CombatSet::Finish),
    );
    app.finish();
    app.cleanup();
    // Boot, loading, in world: three frames, as the sim's tests count them.
    for _ in 0..3 {
        app.update();
    }
    // A floor wide enough that no policy walks off it in a bout.
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(600., 1., 600.),
        WorldPosition::new(0., 1.5, 0.),
        Transform::default(),
    ));
    app
}

/// The bar a side's abilities make, each in the first free place it fits: a
/// weapon ability on the side's weapon class, a utility one on the utility
/// places. The side knows exactly its list, and the sim's own `change` says
/// whether each may go, as it does for a member.
///
/// # Errors
/// An ability that is no row, does not work with the side's weapon, is named
/// twice or finds no free place.
fn build_bar(side: &Side) -> anyhow::Result<Bar> {
    let tables = strata_sim::combat_tables::current();
    let known: BTreeSet<String> = side.abilities.iter().cloned().collect();
    let mut bar = Bar::default();
    for id in &side.abilities {
        anyhow::ensure!(bar.places_of(id).is_empty(), "`{id}` is named twice");
        let row = tables
            .abilities
            .row(id)
            .ok_or_else(|| anyhow::anyhow!("`{id}` is no ability of abilities.ron"))?;
        let place = match row.slot {
            SlotKind::Weapon => (0..tables.abilities.bar.weapon)
                .map(|at| Place::Weapon(side.weapon, at))
                .find(|place| bar.get(*place).is_none()),
            SlotKind::Utility => (0..tables.abilities.bar.utility)
                .map(Place::Utility)
                .find(|place| bar.get(*place).is_none()),
        }
        .ok_or_else(|| anyhow::anyhow!("`{id}` finds no free place on the bar"))?;
        strata_sim::ability::change(
            &tables.abilities,
            &known,
            &mut bar,
            &BarRequest::Set {
                place,
                ability: id.clone(),
            },
        )
        .map_err(|why| anyhow::anyhow!("`{id}`: {why}"))?;
    }
    Ok(bar)
}

/// Whether both sides' abilities make a bar, before any bout runs.
///
/// # Errors
/// The first that does not, naming the matchup and the side.
pub fn check_abilities(matchup: &Matchup) -> anyhow::Result<()> {
    for (name, side) in [("a", &matchup.a), ("b", &matchup.b)] {
        build_bar(side).map_err(|why| anyhow::anyhow!("{} {name}: {why}", matchup.name))?;
    }
    Ok(())
}

/// A combatant of `side` on `team` at `at`, facing `yaw`, with the
/// profile's pools and motor and the side's weapon drawn. A side with
/// boxes is fitted first, so its pools, magazine, bolt and motor are the
/// fitted rows' as the game's are, and a side with modules is equipped
/// with them after that, so its magazines start full at their fitted size.
#[allow(
    clippy::too_many_lines,
    reason = "one build of a combatant per profile, each a few lines"
)]
fn spawn(app: &mut App, side: &Side, team: u8, at: WorldPos, yaw: f32, seed: u64) -> Entity {
    let tables = strata_sim::combat_tables::current();
    let opponents = &tables.opponents;
    let pools = strata_sim::combat_tiers::opponent_pools(side.tier);
    let fitted = if side.boxes.is_empty() {
        None
    } else {
        let sheet = crate::boxes::sheet(if team == 0 { "a" } else { "b" }, &side.boxes)
            .expect("the side's boxes are a sheet; the run checks them first");
        let mut actor = Combatant::on_team(team, tables.actor.health);
        actor.fit(&crate::boxes::graph(), &sheet);
        Some(actor)
    };
    let (mut actor, controller) = match side.profile {
        Profile::Player | Profile::Post => {
            let health = if side.profile == Profile::Post {
                tables.opponents.passive_health
            } else {
                tables.actor.health
            };
            let actor = fitted.unwrap_or_else(|| Combatant::on_team(team, health));
            let controller = actor.with_actor(|row| CharacterController {
                responsive: true,
                height: 1.8,
                walk_speed: row.walk_speed,
                sprint_speed: row.sprint_speed,
                jump_speed: row.jump_speed,
                gravity: row.gravity,
                ..Default::default()
            });
            (actor, controller)
        }
        Profile::Rusher | Profile::Sentry => {
            let sentry = side.profile == Profile::Sentry;
            // Fitted from its sheet as the game fits it, unless the preset
            // names boxes of its own.
            let had_boxes = fitted.is_some();
            let mut actor = fitted.unwrap_or_else(|| Combatant::on_team(team, pools.health));
            actor.health = pools.health;
            actor.shield = pools.shield;
            actor.shield_capacity = pools.shield;
            actor.damage_scale = pools.damage_scale;
            if !had_boxes {
                let sheet = crate::boxes::opponent(if sentry { "sentry" } else { "rusher" })
                    .expect("the shipped opponents file has a sheet for each kind");
                actor.fit(&crate::boxes::graph(), &sheet);
            }
            (
                actor,
                CharacterController {
                    responsive: true,
                    height: 1.8,
                    walk_speed: if sentry {
                        opponents.sentry.walk_speed
                    } else {
                        opponents.rusher.walk_speed
                    },
                    sprint_speed: opponents.sprint_speed,
                    ..Default::default()
                },
            )
        }
        Profile::Marksman | Profile::Duelist | Profile::Bulwark => {
            // Built as the game builds them: the pools first, then the
            // sheet of `opponents.ron` fitted over the tables.
            let (name, kind) = match side.profile {
                Profile::Marksman => ("marksman", strata_sim::combat_ai::OpponentKind::Marksman),
                Profile::Duelist => ("duelist", strata_sim::combat_ai::OpponentKind::Duelist),
                _ => ("bulwark", strata_sim::combat_ai::OpponentKind::Bulwark),
            };
            let mut actor = Combatant::on_team(team, pools.health);
            actor.shield = pools.shield;
            actor.shield_capacity = pools.shield;
            actor.damage_scale = pools.damage_scale;
            if side.boxes.is_empty() {
                let sheet = crate::boxes::opponent(name)
                    .expect("the shipped opponents file has a sheet for each kind");
                actor.fit(&crate::boxes::graph(), &sheet);
            } else if let Some(fitted) = fitted {
                actor = fitted;
                actor.health = pools.health;
                actor.shield = pools.shield;
                actor.shield_capacity = pools.shield;
                actor.damage_scale = pools.damage_scale;
            }
            let walk_speed =
                kind.motor_speed(opponents, actor.with_actor(|row| row.aim_speed_scale));
            (
                actor,
                CharacterController {
                    responsive: true,
                    height: 1.8,
                    walk_speed,
                    sprint_speed: opponents.sprint_speed,
                    ..Default::default()
                },
            )
        }
    };
    if !side.fitted.is_empty() || side.condition.is_some() || side.lines.is_some() {
        let loadout = crate::loadout::of(side)
            .expect("the side's modules are a loadout; the run checks them first");
        actor.equip(&loadout);
    }
    let mut actor = actor.carrying([Some(side.weapon), Some(side.holstered())], 0);
    // The shield bearer's guard is its own and never a class's.
    if side.profile == Profile::Bulwark {
        actor = actor.with_guard(side.weapon, opponents.bulwark.guard);
    }
    // Every opponent fires the sentry's slow bolt, as the game gives them.
    let player_like = matches!(side.profile, Profile::Player | Profile::Post);
    actor.bolt_speed = if player_like {
        actor.weapon_spec().bolt_speed
    } else if side.profile == Profile::Marksman {
        opponents.marksman.bolt_speed
    } else {
        opponents.sentry.bolt_speed
    };
    // The marksman fires the rifle's own mode.
    if !player_like && side.profile != Profile::Marksman {
        actor.mode_override = Some(opponents.sentry.mode);
    }
    // Each bout draws its own stretch of the cone; a control and its pair
    // share a seed, so they draw alike.
    actor.shot_salt = seed;
    actor.facing = yaw;
    let intent = strata_sim::PlayerIntent {
        yaw,
        ..Default::default()
    };
    let driver = Driver::new(side.policy.clone(), seed).expect("the side's policy is usable");
    let presser = (!side.abilities.is_empty()).then(|| {
        actor.set_bar(
            &build_bar(side).expect("the side's abilities make a bar; the run checks them first"),
        );
        PressAbilities::default()
    });
    let entity = app
        .world_mut()
        .spawn((
            Player::default(),
            intent,
            controller,
            WorldPosition(at),
            actor,
            driver,
        ))
        .id();
    if let Some(presser) = presser {
        app.world_mut().entity_mut(entity).insert(presser);
    }
    entity
}

/// Run one bout of `matchup` with `seed`, capped at `cap` seconds. The
/// tables are whatever is installed; the caller installs the grid point.
///
/// # Panics
/// A scripted policy whose file cannot be read: a bout cannot go on
/// without one side's inputs.
#[must_use]
pub fn run(matchup: &Matchup, seed: u64, cap: f32) -> BoutResult {
    let mut app = app();
    let mut seeds = Rng::new(seed);
    let feet = 2.;
    let centre = feet + 0.9;
    for (team, side, z, yaw) in [
        (0_u8, &matchup.a, 0_f64, 0_f32),
        (
            1,
            &matchup.b,
            -f64::from(matchup.distance),
            std::f32::consts::PI,
        ),
    ] {
        // The side's own members first, then each of `with`, in one line.
        let members: Vec<&Side> = std::iter::once(side).chain(&side.with).collect();
        let total: u32 = members.iter().map(|m| m.count.max(1)).sum();
        let mut place = 0_u32;
        for member in members {
            for _ in 0..member.count.max(1) {
                // A line abreast, 1.5 m apart, centred on the side's spot.
                let offset = (f64::from(place) - f64::from(total - 1) * 0.5) * 1.5;
                place += 1;
                let at = WorldPos::new(offset, centre, z);
                spawn(&mut app, member, team, at, yaw, seeds.next_u64());
            }
        }
    }
    // The characters get their colliders on the first tick and settle
    // onto the floor; the policies start on the second.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cap in seconds, a few thousand ticks"
    )]
    let ticks = (f64::from(cap) * HZ).ceil() as u64;
    let mut winner = Winner::Draw;
    let mut time = cap;
    for tick in 1..=ticks {
        app.update();
        let mut standing = [false; 2];
        for actor in app.world_mut().query::<&Combatant>().iter(app.world()) {
            if actor.action != Action::Dead {
                standing[usize::from(actor.team.min(1))] = true;
            }
        }
        let decided = match standing {
            [true, false] => Some(Winner::A),
            [false, true] => Some(Winner::B),
            _ => None,
        };
        if let Some(decided) = decided {
            winner = decided;
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "a tick count into seconds"
            )]
            {
                time = (tick as f64 / HZ) as f32;
            }
            break;
        }
    }
    let recorder = app.world().resource::<Recorder>();
    BoutResult {
        winner,
        time,
        sides: recorder.sides,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duel() -> Matchup {
        Matchup {
            name: "test".into(),
            a: Side {
                policy: Policy::HoldAndFire {
                    range: 12.,
                    cadence: 0.,
                    aim: true,
                },
                weapon: Weapon::Carbine,
                profile: Profile::Player,
                count: 1,
                boxes: Vec::new(),
                fitted: Vec::new(),
                condition: None,
                lines: None,
                abilities: Vec::new(),
                with: Vec::new(),
                tier: 0,
            },
            b: Side {
                policy: Policy::Rusher,
                weapon: Weapon::Carbine,
                profile: Profile::Rusher,
                count: 1,
                boxes: Vec::new(),
                fitted: Vec::new(),
                condition: None,
                lines: None,
                abilities: Vec::new(),
                with: Vec::new(),
                tier: 0,
            },
            distance: 18.,
        }
    }

    #[test]
    fn a_carbine_at_range_kills_a_rusher_and_the_counters_agree() {
        let result = run(&duel(), 1, 60.);
        assert_eq!(result.winner, Winner::A, "{result:?}");
        assert!(result.time < 60.);
        let [a, b] = result.sides;
        assert!(a.shots >= a.hits && a.hits > 0, "{a:?}");
        assert!((a.dealt - b.taken).abs() < 1e-3, "{a:?} {b:?}");
        assert!(b.taken >= 85. + 20. - 1e-3, "the rusher's pools: {b:?}");
        assert_eq!(a.cuts, 0);
        assert!(a.posture_min <= 100. && b.posture_min <= 100.);
    }

    #[test]
    fn a_sprinting_closer_reaches_a_standing_target_sooner_than_a_walker() {
        let first_cut = |name: &str| {
            let mut matchup = crate::preset(name).expect("a preset");
            matchup.b.policy = Policy::Stand;
            let result = run(&matchup, 1, 20.);
            result.sides[0].first_cut.expect("the closer reaches it")
        };
        let walker = first_cut("blade-vs-carbine-25m");
        let sprinter = first_cut("sprint-blade-vs-carbine-25m");
        assert!(sprinter + 0.5 < walker, "{sprinter} against {walker}");
    }

    #[test]
    fn a_sprinting_guard_reaches_a_standing_target_sooner_than_a_walking_one() {
        let first_cut = |sprint: bool| {
            let mut matchup = crate::preset("blade-vs-carbine-25m").expect("a preset");
            matchup.a.policy = Policy::GuardAndCut {
                chain: true,
                heavy_every: 0,
                cooldown: 0.,
                reach: strata_sim::combat_tables::with_melee(|melee| melee.reach_m(Weapon::Blade)),
                parry: false,
                reaction: 0.,
                sprint,
            };
            matchup.b.policy = Policy::Stand;
            let result = run(&matchup, 1, 20.);
            result.sides[0].first_cut.expect("the guard reaches it")
        };
        let (walker, sprinter) = (first_cut(false), first_cut(true));
        assert!(sprinter + 0.5 < walker, "{sprinter} against {walker}");
    }

    #[test]
    fn the_sentry_fires_on_its_table_and_a_quicker_table_fires_more() {
        let matchup = Matchup {
            name: "test".into(),
            a: Side {
                policy: Policy::Stand,
                weapon: Weapon::Carbine,
                profile: Profile::Player,
                count: 1,
                boxes: Vec::new(),
                fitted: Vec::new(),
                condition: None,
                lines: None,
                abilities: Vec::new(),
                with: Vec::new(),
                tier: 0,
            },
            b: Side {
                policy: Policy::Sentry,
                weapon: Weapon::Carbine,
                profile: Profile::Sentry,
                count: 1,
                boxes: Vec::new(),
                fitted: Vec::new(),
                condition: None,
                lines: None,
                abilities: Vec::new(),
                with: Vec::new(),
                tier: 0,
            },
            distance: 12.,
        };
        // A dummy against the shipped sentry: cooldown, charge, recover,
        // charge, so two shots in the first four seconds.
        let shipped = run(&matchup, 1, 4.);
        assert_eq!(shipped.sides[1].shots, 2, "{shipped:?}");
        let mut quick = strata_sim::combat_tables::CombatTables::shipped();
        quick.opponents.sentry.charge = 0.1;
        quick.opponents.sentry.recover = 0.1;
        quick.opponents.sentry.cooldown = 0.1;
        strata_sim::combat_tables::install(quick);
        let quicker = run(&matchup, 1, 4.);
        strata_sim::combat_tables::install(strata_sim::combat_tables::CombatTables::shipped());
        assert!(
            quicker.sides[1].shots > shipped.sides[1].shots * 3,
            "{quicker:?}"
        );
    }

    /// A side that names no module is spawned without a loadout, and a
    /// spec written before sides could carry one reads as that side: the
    /// same bout to the bit.
    #[test]
    fn a_side_with_nothing_fitted_is_the_side_it_was() {
        let matchup = duel();
        let older = r#"(
            name: "test",
            a: (policy: HoldAndFire(range: 12.0), weapon: Carbine, profile: Player),
            b: (policy: Rusher, weapon: Carbine, profile: Rusher),
            distance: 18.0,
        )"#;
        let parsed: Matchup = ron::from_str(older).expect("an older spec parses");
        assert_eq!(parsed, matchup);
        assert!(parsed.a.fitted.is_empty() && parsed.b.fitted.is_empty());
        assert_eq!(run(&parsed, 3, 60.), run(&matchup, 3, 60.));

        let mut app = app();
        for (team, side) in [(0, &matchup.a), (1, &matchup.b)] {
            let entity = spawn(&mut app, side, team, WorldPos::new(0., 2.9, 0.), 0., 1);
            let actor = app.world().get::<Combatant>(entity).expect("spawned");
            assert!(actor.loadout().is_none(), "team {team}");
            assert_eq!(actor.ammo, Weapon::Carbine.spec().magazine);
            assert_eq!(actor.weapon_spec(), Weapon::Carbine.spec());
        }
    }

    #[test]
    fn a_closer_sprints_to_its_distance_and_only_then_fights() {
        let mut matchup = crate::preset("carbine-close-vs-marksman-60m").expect("a preset");
        matchup.b.policy = Policy::Stand;
        matchup.b.profile = Profile::Post;
        // 8.5 m a second covers the 40 m to 20 m in under five seconds:
        // no round before, and rounds soon after.
        let early = run(&matchup, 1, 3.);
        assert_eq!(early.sides[0].shots, 0, "{early:?}");
        let later = run(&matchup, 1, 9.);
        assert!(later.sides[0].shots > 0, "{later:?}");
    }

    #[test]
    fn an_unreadable_script_is_refused_at_spawn() {
        let mut matchup = duel();
        matchup.a.policy = Policy::Scripted {
            path: "/nonexistent-for-this-test".into(),
        };
        // A script that cannot be read is a panic at spawn, not a silent
        // draw, so the harness tells the operator; checked by the API.
        assert!(Driver::new(matchup.a.policy.clone(), 0).is_err());
    }
}
