//! The encounter's opponents, the sentry, the rusher, the marksman and the
//! duelist of `game --combat`,
//! as a plain function of what an opponent can see. It lives here rather
//! than in the game so the balance harness drives the very opponent the
//! game ships, from the same `assets/combat/opponents.ron` row, every tick:
//! a sweep of a sentry number then predicts the encounter instead of a
//! copy of it, which is the one-model rule of the progression design
//! (section 7). The game decides *when* an opponent fights (grace,
//! engagement, a passive target); this decides what it does once it does.

use bevy::math::DVec3;
use strata_core::WorldPos;

use crate::PlayerIntent;
use crate::combat::{Action, CombatIntent, Combatant};
use crate::combat_tables::{DuelistTable, OpponentTable};

/// Which opponent a brain plays. A row of `opponents.ron` each; the sentry
/// and the rusher are the first slice's two, the marksman and the duelist
/// the ones that ask a player to use a rifle, close a distance and time a cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpponentKind {
    /// Holds 9 to 20 m and charges one slow bolt.
    Sentry,
    /// Walks in and swings one heavy.
    Rusher,
    /// Holds 40 to 80 m behind a beam and fires a fast bolt.
    Marksman,
    /// Guards, parries a slow cut it sees begin, and is open while it swings.
    Duelist,
    /// Walks in behind a shield that holds bolts from the front, and lowers
    /// it to cut.
    Bulwark,
}

impl OpponentKind {
    /// Every kind, in the order a table lists them.
    pub const ALL: [Self; 5] = [
        Self::Sentry,
        Self::Rusher,
        Self::Marksman,
        Self::Duelist,
        Self::Bulwark,
    ];

    /// Its name as a nameplate, a flag and a sheet in
    /// `assets/progression/opponents.ron` spell it, lower case.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sentry => "sentry",
            Self::Rusher => "rusher",
            Self::Marksman => "marksman",
            Self::Duelist => "duelist",
            Self::Bulwark => "bulwark",
        }
    }

    /// The kind a name spells, in lower case.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether it fights with a gun.
    #[must_use]
    pub const fn ranged(self) -> bool {
        matches!(self, Self::Sentry | Self::Marksman)
    }

    /// Its walking speed, from its row.
    #[must_use]
    pub const fn walk_speed(self, opponents: &OpponentTable) -> f32 {
        match self {
            Self::Sentry => opponents.sentry.walk_speed,
            Self::Rusher => opponents.rusher.walk_speed,
            Self::Marksman => opponents.marksman.walk_speed,
            Self::Duelist => opponents.duelist.walk_speed,
            Self::Bulwark => opponents.bulwark.advance_speed,
        }
    }

    /// The walking speed to give its motor: its row's, except for the
    /// bulwark, whose row is the pace it closes at with the guard up and
    /// the guard's aim pace (`aim_scale`, the actor's own) must not slow it.
    #[must_use]
    pub fn motor_speed(self, opponents: &OpponentTable, aim_scale: f32) -> f32 {
        match self {
            Self::Bulwark => opponents.bulwark.advance_speed / aim_scale,
            _ => self.walk_speed(opponents),
        }
    }

    /// Its first cooldown, from its row.
    fn initial_cooldown(self, opponents: &OpponentTable) -> f32 {
        match self {
            Self::Sentry => opponents.sentry.cooldown,
            Self::Rusher => opponents.rusher.initial_cooldown,
            Self::Marksman => opponents.marksman.cooldown,
            Self::Duelist => opponents.duelist.initial_cooldown,
            Self::Bulwark => opponents.bulwark.initial_cooldown,
        }
    }
}

/// What else an opponent can see of its target: the one thing a player's
/// eye reads off a cut, the action and how long it has run. The sentry and
/// the rusher ignore it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    /// What the target is doing.
    pub action: Action,
    /// Seconds the action has run.
    pub age: f32,
    /// Seconds the action takes from its start until it can land: a cut's
    /// contact start, an ability's windup. Nought for anything else.
    pub lands: f32,
    /// Seconds from the start until the cut is spent: a cut's contact end,
    /// an ability's length. Nought for anything else.
    pub ends: f32,
    /// The target holds a melee weapon.
    pub melee: bool,
    /// The target holds aim (a gun's, or a melee weapon's guard).
    pub aiming: bool,
    /// Metres from the opponent to the target, along the ground.
    pub distance: f32,
}

impl Seen {
    /// An opponent looking at nothing in particular: a target at rest at
    /// `distance`, which is all the sentry and the rusher need.
    #[must_use]
    pub const fn at(distance: f32) -> Self {
        Self {
            action: Action::Ready,
            age: 0.,
            lands: 0.,
            ends: 0.,
            melee: false,
            aiming: false,
            distance,
        }
    }

    /// What `target` shows an opponent `distance` metres from it.
    #[must_use]
    pub fn of(target: &Combatant, distance: f32) -> Self {
        let lands = match target.action {
            Action::Light | Action::Followup | Action::Heavy => {
                target.spec(target.action).contact_start
            }
            Action::Ability => target
                .ability_in_use()
                .and_then(|id| target.ability(id))
                .map_or(0., |row| row.windup),
            _ => 0.,
        };
        let ends = match target.action {
            Action::Light | Action::Followup | Action::Heavy => {
                target.spec(target.action).contact_end
            }
            Action::Ability => target
                .ability_in_use()
                .and_then(|id| target.ability(id))
                .map_or(0., |row| row.length),
            _ => 0.,
        };
        Self {
            action: target.action,
            age: target.age,
            lands,
            ends,
            melee: target.weapon.is_melee(),
            aiming: target.aiming || target.guard_up,
            distance,
        }
    }

    /// Whether the target has begun a Heavy, or an ability that lands later
    /// than `react` plus a tenth of a second: a blow slow enough to see
    /// coming and meet.
    #[must_use]
    pub fn slow(&self, row: &DuelistTable) -> bool {
        self.melee
            && (self.action == Action::Heavy
                || (self.action == Action::Ability && self.lands > row.react + row.slow_margin))
    }
}

/// What an opponent keeps between ticks: its clock and, for a charging
/// shooter, the charge it is in.
#[derive(Debug, Clone, PartialEq)]
pub struct OpponentBrain {
    /// Which opponent it plays.
    pub kind: OpponentKind,
    /// Seconds until its next action.
    pub cooldown: f32,
    /// A charge: the aim, and the seconds left. The sentry captures the aim
    /// when the charge begins and never moves it, which is what makes the
    /// bolt readable and a sidestep worth something; the marksman's follows
    /// the target until its lock.
    pub windup: Option<(WorldPos, f32)>,
    /// A duelist's attacks so far, for its heavy count.
    pub openings: u32,
    /// Seconds a duelist keeps its guard down after its own attack.
    pub rest: f32,
    /// A duelist's follow up was already pressed behind the current light.
    pub chained: bool,
    /// A duelist's own light or heavy has begun and it has not been Ready
    /// since: an attack is counted when the sim takes it, not when pressed.
    pub swinging: bool,
    /// A bulwark has stood in reach of its target since the encounter began:
    /// its first attack is counted from then, not from the start.
    pub arrived: bool,
}

impl OpponentBrain {
    /// An opponent at the start of an encounter, on its table's initial
    /// cooldown.
    #[must_use]
    pub fn new(kind: OpponentKind, opponents: &OpponentTable) -> Self {
        let mut brain = Self {
            kind,
            cooldown: 0.,
            windup: None,
            openings: 0,
            rest: 0.,
            chained: false,
            swinging: false,
            arrived: false,
        };
        brain.reset(opponents);
        brain
    }

    /// Back to the start of an encounter.
    pub fn reset(&mut self, opponents: &OpponentTable) {
        self.windup = None;
        self.openings = 0;
        self.rest = 0.;
        self.chained = false;
        self.swinging = false;
        self.arrived = false;
        self.cooldown = self.kind.initial_cooldown(opponents);
    }

    /// One tick of an opponent at `position` fighting a target at `target`
    /// (its feet; the aim is a little above). `movement` and `input` are
    /// written over, so the caller clears them first and only calls this
    /// while the opponent is engaged and alive. `actor.charge` is the
    /// telegraph the view and a player's eye read.
    #[allow(
        clippy::too_many_arguments,
        reason = "an opponent's senses and its outputs, each a separate thing"
    )]
    pub fn drive(
        &mut self,
        dt: f32,
        position: WorldPos,
        target: WorldPos,
        seen: &Seen,
        opponents: &OpponentTable,
        actor: &mut Combatant,
        movement: &mut PlayerIntent,
        input: &mut CombatIntent,
    ) {
        let (sentry, rusher) = (&opponents.sentry, &opponents.rusher);
        actor.charge = 0.;
        if actor.action == Action::Stagger {
            self.windup = None;
            self.chained = false;
            self.swinging = false;
            self.cooldown = match self.kind {
                OpponentKind::Marksman => opponents.marksman.stagger_cooldown,
                OpponentKind::Duelist => opponents.duelist.stagger_cooldown,
                OpponentKind::Bulwark => opponents.bulwark.stagger_cooldown,
                _ => sentry.stagger_cooldown,
            };
            // A bulwark knocked off its guard does not have it back at once:
            // the slab stays down as long as its attack does, and that is the
            // opening a broken guard earns.
            if self.kind == OpponentKind::Bulwark {
                self.rest = opponents.bulwark.stagger_cooldown;
            }
            return;
        }
        let delta = target.0 - position.0;
        let distance = delta.x.hypot(delta.z);
        let target_point = WorldPos(target.0 + DVec3::Y * 0.2);
        self.cooldown -= dt;
        match self.kind {
            OpponentKind::Sentry => {
                input.aim = true;
                if let Some((aim, remaining)) = &mut self.windup {
                    *remaining -= dt;
                    input.aim_point = *aim;
                    actor.charge = (1. - *remaining / sentry.charge).clamp(0.05, 1.);
                    if *remaining <= 0. {
                        input.fire = true;
                        actor.charge = 0.;
                        self.windup = None;
                        self.cooldown = sentry.recover;
                    }
                } else {
                    input.aim_point = target_point;
                    if distance > f64::from(sentry.hold_far) {
                        movement.move_dir.y = 0.7;
                    } else if distance < f64::from(sentry.hold_near) {
                        movement.move_dir.y = -0.7;
                    } else {
                        movement.move_dir.x = 0.25;
                    }
                    if self.cooldown <= 0.
                        && distance < f64::from(sentry.fire_range)
                        && actor.action == Action::Ready
                    {
                        if actor.ammo == 0 {
                            input.reload = true;
                        } else {
                            self.windup = Some((target_point, sentry.charge));
                            actor.charge = 0.05;
                        }
                    }
                }
            }
            OpponentKind::Rusher => {
                input.aim_point = target_point;
                if distance > f64::from(rusher.reach) {
                    movement.move_dir.y = 1.;
                } else if actor.action == Action::Ready && self.cooldown <= 0. {
                    input.heavy = true;
                    self.cooldown = rusher.cooldown;
                }
            }
            OpponentKind::Marksman => {
                self.marksman(
                    dt,
                    distance,
                    target_point,
                    opponents,
                    actor,
                    movement,
                    input,
                );
            }
            OpponentKind::Duelist => {
                input.aim_point = target_point;
                self.duelist(dt, distance, seen, opponents, actor, movement, input);
            }
            OpponentKind::Bulwark => {
                input.aim_point = target_point;
                self.bulwark(dt, distance, opponents, actor, movement, input);
            }
        }
        // Aim stays on the captured point through the charge and the shot;
        // melee's action clock separately locks facing when a swing begins.
        let aim = input.aim_point.0 - position.0;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a local heading is render-space f32"
        )]
        {
            movement.yaw = (-aim.x).atan2(-aim.z) as f32;
        }
    }

    /// The marksman: holds its band, charges an aim that follows the target
    /// until the lock, and fires once the aim has closed.
    #[allow(
        clippy::too_many_arguments,
        reason = "the senses and outputs of drive, passed on"
    )]
    fn marksman(
        &mut self,
        dt: f32,
        distance: f64,
        target_point: WorldPos,
        opponents: &OpponentTable,
        actor: &mut Combatant,
        movement: &mut PlayerIntent,
        input: &mut CombatIntent,
    ) {
        let row = &opponents.marksman;
        input.aim_point = target_point;
        if distance < f64::from(row.flee) {
            // Too close for a rifle: no charge, straight away.
            self.windup = None;
            movement.move_dir.y = -1.;
            return;
        }
        input.aim = true;
        if distance > f64::from(row.hold_far) {
            movement.move_dir.y = row.pace;
        } else if distance < f64::from(row.hold_near) {
            movement.move_dir.y = -row.pace;
        }
        if let Some((aim, remaining)) = &mut self.windup {
            *remaining -= dt;
            if *remaining > row.lock {
                *aim = target_point;
            }
            input.aim_point = *aim;
            actor.charge = (1. - *remaining / row.charge).clamp(0.05, 1.);
            // The trigger waits for the aim to have closed, which only
            // matters after a stagger or a reload cut the charge short.
            if *remaining <= 0. && actor.aim_share() >= 1. {
                input.fire = true;
                actor.charge = 0.;
                self.windup = None;
                self.cooldown = row.recover;
            }
        } else if self.cooldown <= 0.
            && distance < f64::from(row.fire_range)
            && actor.action == Action::Ready
        {
            if actor.ammo == 0 {
                input.reload = true;
            } else {
                self.windup = Some((target_point, row.charge));
                actor.charge = 0.05;
            }
        }
    }

    /// One tick of an opponent that is not yet fighting (before the encounter
    /// engages, or in its grace). Every kind stands as it did, so their
    /// replays do not move; a bulwark keeps its guard up and faces its target,
    /// so a first shot at it is met by the slab and it reads as what it is.
    pub fn idle(
        &self,
        position: WorldPos,
        target: WorldPos,
        actor: &mut Combatant,
        movement: &mut PlayerIntent,
        input: &mut CombatIntent,
    ) {
        if self.kind != OpponentKind::Bulwark || actor.action != Action::Ready {
            return;
        }
        let aim = WorldPos(target.0 + DVec3::Y * 0.2);
        input.aim = true;
        input.aim_point = aim;
        let delta = aim.0 - position.0;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a local heading is render-space f32"
        )]
        {
            movement.yaw = (-delta.x).atan2(-delta.z) as f32;
        }
    }

    /// The bulwark: holds its guard and faces its target every tick, walks at
    /// it at its advance speed whatever the distance, and inside its reach
    /// lowers the guard to cut a light and its follow up. It reads nothing
    /// of what its target does: it never parries, dodges or backs away.
    fn bulwark(
        &mut self,
        dt: f32,
        distance: f64,
        opponents: &OpponentTable,
        actor: &mut Combatant,
        movement: &mut PlayerIntent,
        input: &mut CombatIntent,
    ) {
        let row = &opponents.bulwark;
        if actor.action != Action::Ready {
            // Its own cut under way, whatever else: no guard, no steps, and
            // the next cut and the rest count from the end. The follow up is
            // pressed inside the light's contact, and an attack is counted
            // when the sim takes it, not when pressed.
            if actor.action == Action::Light && !self.swinging {
                self.swinging = true;
                self.openings += 1;
            }
            self.cooldown = row.cooldown;
            self.rest = row.rest;
            if actor.action == Action::Light
                && actor.age >= actor.spec(Action::Light).contact_start
                && !self.chained
            {
                input.light = true;
                self.chained = true;
            }
            return;
        }
        self.swinging = false;
        self.rest = (self.rest - dt).max(0.);
        if self.rest > 0. {
            return;
        }
        input.aim = true;
        if distance > f64::from(row.reach) {
            movement.move_dir.y = 1.;
            return;
        }
        if !self.arrived {
            self.arrived = true;
            self.cooldown = row.initial_cooldown;
        }
        if self.cooldown <= 0.
            && actor.guard_met_for() >= row.pinned
            && actor.can_begin(Action::Light)
        {
            input.aim = false;
            input.light = true;
            self.chained = false;
        }
    }

    /// The duelist: closes behind a guard, attacks inside its reach on its
    /// cooldown, keeps the guard down while it swings and for `rest` after,
    /// and meets a slow cut it sees begin with a guard raised `react`
    /// seconds into it.
    #[allow(
        clippy::too_many_arguments,
        reason = "the senses and outputs of drive, passed on"
    )]
    fn duelist(
        &mut self,
        dt: f32,
        distance: f64,
        seen: &Seen,
        opponents: &OpponentTable,
        actor: &mut Combatant,
        movement: &mut PlayerIntent,
        input: &mut CombatIntent,
    ) {
        let row = &opponents.duelist;
        // A cut slow enough to see coming, begun in reach and not yet spent.
        let coming = seen.slow(row)
            && seen.age <= seen.ends
            && f64::from(seen.distance) < f64::from(row.reach * row.read_reach);
        // Its parry is its own training, not the player's rested button, and
        // only for the raise that meets a slow cut; every other raise goes by
        // melee.ron's rule.
        actor.parry_rest = coming.then_some(row.parry_rest);
        if actor.action != Action::Ready {
            // Mid swing or mid anything: the guard is down (it cannot be
            // held in an action), the next attack and the rest count from
            // the end, and the follow up is pressed inside the light.
            if matches!(actor.action, Action::Light | Action::Heavy) && !self.swinging {
                self.swinging = true;
                self.openings += 1;
            }
            self.cooldown = row.cooldown;
            self.rest = row.rest;
            if actor.action == Action::Light
                && actor.age >= actor.spec(Action::Light).contact_start
                && !self.chained
            {
                input.light = true;
                self.chained = true;
            }
            return;
        }
        self.swinging = false;
        self.rest = (self.rest - dt).max(0.);
        if self.rest > 0. {
            return;
        }
        if distance > f64::from(row.wary) {
            movement.move_dir.y = 1.;
            return;
        }
        // A slow cut begun in reach is met by a guard down until `react`
        // seconds into it and up from then until it is spent, so it is fresh
        // when the cut lands and long spent when a quick one does; it does
        // not swing into one it is reading.
        let reading = coming && seen.age < row.react;
        let heavy = row.heavy_every > 0 && (self.openings + 1).is_multiple_of(row.heavy_every);
        let attack = if heavy { Action::Heavy } else { Action::Light };
        if distance > f64::from(row.reach) {
            movement.move_dir.y = 1.;
            input.aim = !reading;
        } else if self.cooldown <= 0. && !coming && actor.can_begin(attack) {
            if heavy {
                input.heavy = true;
            } else {
                input.light = true;
                self.chained = false;
            }
        } else {
            input.aim = !reading;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "tick counts and metres in a rig, a few hundred at most"
    )]

    use super::*;
    use crate::combat_tables::CombatTables;

    /// Drive `brain` for `seconds` at 60 Hz against a target 12 m away and
    /// return the ticks on which it fired, with the charge seen the tick
    /// before each.
    fn shots(
        brain: &mut OpponentBrain,
        opponents: &OpponentTable,
        seconds: f32,
    ) -> Vec<(u32, f32)> {
        let mut actor = Combatant::on_team(1, 85.);
        let dt = 1. / 60.;
        let mut fired = Vec::new();
        let mut charge_before = 0.;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a tick count"
        )]
        let ticks = (seconds * 60.) as u32;
        for tick in 0..ticks {
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                dt,
                WorldPos::new(0., 1., 0.),
                WorldPos::new(0., 1., -12.),
                &Seen::at(12.),
                opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            if input.fire {
                fired.push((tick, charge_before));
            }
            charge_before = actor.charge;
        }
        fired
    }

    #[test]
    fn the_sentry_waits_charges_fires_and_recovers_on_its_table() {
        let tables = CombatTables::shipped();
        let sentry = &tables.opponents.sentry;
        let mut brain = OpponentBrain::new(OpponentKind::Sentry, &tables.opponents);
        let fired = shots(&mut brain, &tables.opponents, 6.);
        assert_eq!(fired.len(), 2, "{fired:?}");
        // The first shot after the initial cooldown and a full charge, the
        // next after recovery and another charge; a tick of slack each.
        let first = (sentry.cooldown + sentry.charge) * 60.;
        let second = first + (sentry.recover + sentry.charge) * 60.;
        #[allow(clippy::cast_precision_loss, reason = "tick counts")]
        {
            assert!((fired[0].0 as f32 - first).abs() <= 2., "{fired:?}");
            assert!((fired[1].0 as f32 - second).abs() <= 3., "{fired:?}");
        }
        assert!(fired[0].1 > 0.9, "the charge was full before the shot");
    }

    #[test]
    fn a_quicker_table_is_a_quicker_sentry() {
        let mut tables = CombatTables::shipped();
        tables.opponents.sentry.charge = 0.1;
        tables.opponents.sentry.recover = 0.1;
        tables.opponents.sentry.cooldown = 0.1;
        let mut brain = OpponentBrain::new(OpponentKind::Sentry, &tables.opponents);
        let fired = shots(&mut brain, &tables.opponents, 2.);
        assert!(fired.len() >= 8, "{fired:?}");
    }

    #[test]
    fn the_sentry_fires_one_bolt_a_charge_whatever_its_row_says() {
        let tables = CombatTables::shipped();
        for (mode, want) in [(Some(tables.opponents.sentry.mode), 2), (None, 6)] {
            let mut brain = OpponentBrain::new(OpponentKind::Sentry, &tables.opponents);
            let mut actor = Combatant::on_team(1, 85.);
            actor.mode_override = mode;
            let mut motion = crate::combat::CombatMotion::default();
            for _ in 0..360 {
                let mut movement = PlayerIntent::default();
                let mut input = CombatIntent::default();
                brain.drive(
                    1. / 60.,
                    WorldPos::new(0., 1., 0.),
                    WorldPos::new(0., 1., -12.),
                    &Seen::at(12.),
                    &tables.opponents,
                    &mut actor,
                    &mut movement,
                    &mut input,
                );
                actor.step(&mut input, &movement, 1. / 60., &mut motion);
            }
            assert_eq!(actor.shots, want, "{mode:?}");
        }
    }

    #[test]
    fn the_rusher_closes_then_swings_on_its_cooldown() {
        let tables = CombatTables::shipped();
        let rusher = &tables.opponents.rusher;
        let mut brain = OpponentBrain::new(OpponentKind::Rusher, &tables.opponents);
        let mut actor = Combatant::on_team(1, 85.);
        let mut movement = PlayerIntent::default();
        let mut input = CombatIntent::default();
        let far = WorldPos::new(0., 1., -(f64::from(rusher.reach) + 1.));
        let near = WorldPos::new(0., 1., -(f64::from(rusher.reach) - 0.5));
        brain.drive(
            1. / 60.,
            WorldPos::new(0., 1., 0.),
            far,
            &Seen::at(12.),
            &tables.opponents,
            &mut actor,
            &mut movement,
            &mut input,
        );
        assert!(movement.move_dir.y > 0. && !input.heavy);
        // Within reach it waits out the initial cooldown, then swings once
        // and sets the next swing by the table.
        let mut swings = 0;
        for _ in 0..120 {
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                1. / 60.,
                WorldPos::new(0., 1., 0.),
                near,
                &Seen::at(12.),
                &tables.opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            swings += u32::from(input.heavy);
        }
        assert_eq!(swings, 1);
        assert!((brain.cooldown - (rusher.cooldown - 2. + rusher.initial_cooldown)).abs() < 0.1);
    }

    /// FNV-1a over the bits of what an opponent wrote, so a replay of a
    /// fixed fight can be held to the bit.
    fn fold(hash: &mut u64, bits: u32) {
        for byte in bits.to_le_bytes() {
            *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// A fixed fight of one opponent against a target that drifts across
    /// its front: every intent it writes, hashed, for 20 s at 60 Hz.
    fn replay(kind: OpponentKind) -> u64 {
        let tables = CombatTables::shipped();
        let mut brain = OpponentBrain::new(kind, &tables.opponents);
        let mut actor = Combatant::on_team(1, 85.);
        let mut motion = crate::combat::CombatMotion::default();
        let speed = if kind.ranged() {
            tables.opponents.sentry.walk_speed
        } else {
            tables.opponents.rusher.walk_speed
        };
        let dt = 1. / 60.;
        let mut at = WorldPos::new(0., 1., 0.);
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for tick in 0..1200_u32 {
            #[allow(clippy::cast_precision_loss, reason = "a tick count into a drift")]
            let drift = f64::from((tick as f32 * 0.01).sin()) * 3.;
            let target = WorldPos::new(drift, 1., -14. + f64::from(tick % 600) * 0.02);
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                dt,
                at,
                target,
                &Seen::at(12.),
                &tables.opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            let aim = input.aim_point.0 - at.0;
            for bits in [
                movement.move_dir.x.to_bits(),
                movement.move_dir.y.to_bits(),
                movement.yaw.to_bits(),
                actor.charge.to_bits(),
                #[allow(clippy::cast_possible_truncation, reason = "bits of a hash")]
                (aim.x as f32).to_bits(),
                #[allow(clippy::cast_possible_truncation, reason = "bits of a hash")]
                (aim.z as f32).to_bits(),
                u32::from(input.fire)
                    | u32::from(input.aim) << 1
                    | u32::from(input.heavy) << 2
                    | u32::from(input.reload) << 3,
            ] {
                fold(&mut hash, bits);
            }
            actor.step(&mut input, &movement, dt, &mut motion);
            at.0.x += f64::from(movement.move_dir.x * speed * dt);
            at.0.z -= f64::from(movement.move_dir.y * speed * dt);
        }
        fold(&mut hash, u32::try_from(actor.shots).unwrap_or(u32::MAX));
        fold(&mut hash, actor.health.to_bits());
        hash
    }

    #[test]
    fn the_sentry_and_rusher_replay_to_the_bit() {
        // Recorded on the code as it stood before the marksman and the
        // duelist were kinds, so the two older opponents are held to the bit.
        assert_eq!(
            replay(OpponentKind::Sentry),
            0x6c27_9a3a_5e35_43a1,
            "sentry"
        );
        assert_eq!(
            replay(OpponentKind::Rusher),
            0x5976_1d94_d16f_13e3,
            "rusher"
        );
    }

    #[test]
    fn the_marksman_and_duelist_replay_to_the_bit() {
        // Recorded on the code as it stood before the bulwark was a kind.
        assert_eq!(
            replay(OpponentKind::Marksman),
            0xd92e_863b_e629_913c,
            "marksman"
        );
        assert_eq!(
            replay(OpponentKind::Duelist),
            0xadbb_013e_61f2_a653,
            "duelist"
        );
    }

    use crate::combat::{Blow, BlowKind, CombatMotion, GuardOutcome, Weapon, deviate, shot_draw};
    use bevy::math::Vec3;

    const DT: f32 = 1. / 60.;

    /// A marksman at the origin with the rifle in hand.
    fn marksman_rig() -> (CombatTables, OpponentBrain, Combatant) {
        let tables = CombatTables::shipped();
        let brain = OpponentBrain::new(OpponentKind::Marksman, &tables.opponents);
        let mut actor = Combatant::on_team(1, 85.).carrying([Some(Weapon::Rifle), None], 0);
        actor.bolt_speed = tables.opponents.marksman.bolt_speed;
        (tables, brain, actor)
    }

    /// Fight a marksman at the origin for `seconds` against a target that
    /// is at `range` metres and either stands or walks round it at the
    /// player's walking speed. Every shot is flown: the bolt leaves along
    /// the locked aim inside the cone the combatant reports, and it hits if
    /// it passes within a capsule's radius of where the target then is.
    /// Returns shots fired and hits.
    fn marksman_fight(range: f64, strafing: bool, seconds: f32) -> (u32, u32) {
        let (tables, mut brain, mut actor) = marksman_rig();
        let at = WorldPos::new(0., 1., 0.);
        let speed = f64::from(tables.actor.walk_speed);
        let place = |time: f64| {
            let angle = if strafing { speed * time / range } else { 0. };
            WorldPos::new(range * angle.sin(), 1., -range * angle.cos())
        };
        let mut motion = CombatMotion::default();
        let (mut shots, mut hits) = (0, 0);
        let mut time = 0_f64;
        let ticks = (seconds * 60.).round() as u32;
        for _ in 0..ticks {
            let target = place(time);
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                DT,
                at,
                target,
                &Seen::at(range as f32),
                &tables.opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            let before = actor.shots;
            actor.step(&mut input, &movement, DT, &mut motion);
            if actor.shots > before {
                shots += 1;
                let muzzle = at.0 + DVec3::Y * 0.45;
                let line = input.aim_point.0 - muzzle;
                let flight = line.length() / f64::from(actor.bolt_speed);
                let direction = deviate(
                    line.normalize().as_vec3(),
                    actor.shot_cone(),
                    shot_draw(actor.shots, actor.team, Weapon::Rifle),
                );
                let bolt = muzzle + direction.as_dvec3() * line.length();
                let there = place(time + flight).0;
                if (bolt.x - there.x).hypot(bolt.z - there.z) < 0.4 {
                    hits += 1;
                }
            }
            time += f64::from(DT);
        }
        (shots, hits)
    }

    #[test]
    fn a_target_strafing_at_walking_speed_is_missed_and_a_still_one_is_hit() {
        let (shots, hits) = marksman_fight(60., true, 30.);
        assert!(shots >= 5, "{shots} shots");
        assert_eq!(hits, 0, "{shots} shots at a strafing target");
        let (shots, hits) = marksman_fight(60., false, 30.);
        assert!(shots >= 5, "{shots} shots");
        assert_eq!(hits, shots, "a still target is hit by every shot");
    }

    #[test]
    fn the_marksman_locks_its_aim_for_the_last_seconds_of_a_charge() {
        let (tables, mut brain, mut actor) = marksman_rig();
        let row = &tables.opponents.marksman;
        brain.cooldown = 0.;
        let at = WorldPos::new(0., 1., 0.);
        let mut motion = CombatMotion::default();
        let (mut aims, mut charges) = (Vec::new(), Vec::new());
        for tick in 0..240_u32 {
            let target = WorldPos::new(f64::from(tick) * 0.02, 1., -60.);
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                DT,
                at,
                target,
                &Seen::at(60.),
                &tables.opponents,
                &mut actor,
                &mut movement,
                &mut input,
            );
            if actor.charge > 0. {
                aims.push(input.aim_point.0.x);
                charges.push(actor.charge);
            }
            if input.fire {
                break;
            }
            actor.step(&mut input, &movement, DT, &mut motion);
        }
        assert!(charges.first().is_some_and(|c| *c <= 0.06));
        assert!(charges.windows(2).all(|pair| pair[1] >= pair[0]));
        let locked = (row.lock / DT).floor() as usize;
        let tail = &aims[aims.len() - locked..];
        assert!(
            tail.windows(2)
                .all(|pair| pair[0].to_bits() == pair[1].to_bits())
        );
        assert!(
            aims[aims.len() - locked - 4] < tail[0],
            "it followed until the lock"
        );
    }

    #[test]
    fn the_marksman_flees_closes_reloads_and_stops_when_staggered() {
        let (tables, mut brain, mut actor) = marksman_rig();
        let row = &tables.opponents.marksman;
        let at = WorldPos::new(0., 1., 0.);
        let drive = |brain: &mut OpponentBrain, actor: &mut Combatant, range: f64| {
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            brain.drive(
                DT,
                at,
                WorldPos::new(0., 1., -range),
                &Seen::at(range as f32),
                &tables.opponents,
                actor,
                &mut movement,
                &mut input,
            );
            (movement, input)
        };
        // Inside flee: straight away, and never a charge, however long.
        brain.cooldown = 0.;
        for _ in 0..300 {
            let (movement, input) = drive(&mut brain, &mut actor, f64::from(row.flee) - 1.);
            assert!(movement.move_dir.y < 0. && brain.windup.is_none() && !input.fire);
        }
        // Past hold_far it walks in; between its bands it stands; inside
        // hold_near it backs off and still charges.
        let (movement, _) = drive(&mut brain, &mut actor, f64::from(row.hold_far) + 5.);
        assert!(movement.move_dir.y > 0.);
        brain.cooldown = 0.;
        let (movement, _) = drive(&mut brain, &mut actor, 60.);
        assert!(movement.move_dir.length() < f32::EPSILON && brain.windup.is_some());
        brain.reset(&tables.opponents);
        brain.cooldown = 0.;
        let (movement, _) = drive(&mut brain, &mut actor, f64::from(row.hold_near) - 5.);
        assert!(movement.move_dir.y < 0. && brain.windup.is_some());
        // An empty magazine is reloaded, not charged.
        brain.reset(&tables.opponents);
        brain.cooldown = 0.;
        actor.ammo = 0;
        let (_, input) = drive(&mut brain, &mut actor, 60.);
        assert!(input.reload && brain.windup.is_none());
        // A stagger drops the charge and waits.
        actor.ammo = 4;
        brain.cooldown = 0.;
        drive(&mut brain, &mut actor, 60.);
        assert!(brain.windup.is_some());
        actor.action = Action::Stagger;
        let (_, input) = drive(&mut brain, &mut actor, 60.);
        assert!(brain.windup.is_none() && !input.fire);
        assert!((brain.cooldown - row.stagger_cooldown).abs() < f32::EPSILON);
    }

    /// A duelist at the origin facing `-z` and a swinger `range` metres
    /// ahead of it with a blade. The tick feeds the duelist what the swinger
    /// shows, as the game does, and steps both.
    struct Duel {
        tables: CombatTables,
        brain: OpponentBrain,
        duelist: Combatant,
        swinger: Combatant,
        range: f32,
    }

    impl Duel {
        fn new(range: f32) -> Self {
            let tables = CombatTables::shipped();
            let brain = OpponentBrain::new(OpponentKind::Duelist, &tables.opponents);
            let blade = [Some(Weapon::Blade), Some(Weapon::Carbine)];
            Self {
                tables,
                brain,
                duelist: Combatant::on_team(1, 85.).carrying(blade, 0),
                swinger: Combatant::on_team(0, 100.).carrying(blade, 0),
                range,
            }
        }

        /// One tick; the swinger presses `light` or `heavy` on this one.
        fn tick(&mut self, light: bool, heavy: bool) -> CombatIntent {
            let mut movement = PlayerIntent::default();
            let mut input = CombatIntent::default();
            let seen = Seen::of(&self.swinger, self.range);
            self.brain.drive(
                DT,
                WorldPos::new(0., 1., 0.),
                WorldPos::new(0., 1., -f64::from(self.range)),
                &seen,
                &self.tables.opponents,
                &mut self.duelist,
                &mut movement,
                &mut input,
            );
            let drove = input;
            let mut motion = CombatMotion::default();
            self.duelist.step(&mut input, &movement, DT, &mut motion);
            let mut press = CombatIntent {
                light,
                heavy,
                ..CombatIntent::default()
            };
            let facing = PlayerIntent {
                yaw: std::f32::consts::PI,
                ..PlayerIntent::default()
            };
            self.swinger.step(&mut press, &facing, DT, &mut motion);
            drove
        }

        /// Run `seconds` with no press.
        fn wait(&mut self, seconds: f32) {
            for _ in 0..(seconds * 60.) as u32 {
                self.tick(false, false);
            }
        }

        /// The swinger's cut of `action` lands on the tick its contact
        /// begins: how the duelist's guard meets it.
        fn cut_lands(&mut self, action: Action) -> GuardOutcome {
            let (light, heavy) = (action == Action::Light, action == Action::Heavy);
            self.tick(light, heavy);
            assert_eq!(self.swinger.action, action);
            let start = self.swinger.spec(action).contact_start;
            while self.swinger.age < start {
                self.tick(false, false);
            }
            self.blow(Vec3::NEG_Z, BlowKind::Cut)
        }

        fn blow(&mut self, from: Vec3, kind: BlowKind) -> GuardOutcome {
            let blow = Blow::new(20., 30., from, kind).made_with_melee(true);
            let dealt = self.duelist.hit(blow).expect("the duelist is alive");
            if dealt.riposte > 0. {
                self.swinger.riposted(dealt.riposte);
            }
            dealt.guard
        }
    }

    #[test]
    fn a_heavy_at_a_ready_duelist_is_parried_and_a_light_is_blocked() {
        // Outside its reach, inside its wary distance: guard up, closing.
        let mut duel = Duel::new(2.3);
        duel.wait(2.);
        assert!(duel.duelist.guard_up);
        assert_eq!(duel.cut_lands(Action::Light), GuardOutcome::Blocked);
        let mut duel = Duel::new(2.3);
        duel.wait(2.);
        let posture = duel.swinger.posture;
        assert_eq!(duel.cut_lands(Action::Heavy), GuardOutcome::Parried);
        assert!(
            duel.swinger.posture < posture,
            "the riposte lands on the swinger"
        );
    }

    #[test]
    fn a_duelist_is_open_while_it_swings_and_rests_and_from_behind() {
        let mut duel = Duel::new(1.5);
        // Its first attack comes when its cooldown runs out.
        while duel.duelist.action == Action::Ready {
            duel.tick(false, false);
        }
        assert_eq!(duel.blow(Vec3::NEG_Z, BlowKind::Cut), GuardOutcome::Open);
        let mut duel = Duel::new(1.5);
        while duel.duelist.action == Action::Ready {
            duel.tick(false, false);
        }
        while duel.duelist.action != Action::Ready {
            duel.tick(false, false);
        }
        duel.wait(0.1);
        assert!(!duel.duelist.guard_up, "still resting");
        assert_eq!(duel.blow(Vec3::NEG_Z, BlowKind::Cut), GuardOutcome::Open);
        let mut duel = Duel::new(2.3);
        duel.wait(2.);
        assert_eq!(duel.blow(Vec3::Z, BlowKind::Cut), GuardOutcome::Open);
    }

    #[test]
    fn bolts_at_a_duelists_front_are_blocked_until_the_guard_breaks() {
        let mut duel = Duel::new(2.3);
        duel.wait(2.);
        let mut outcomes = Vec::new();
        while outcomes.last() != Some(&GuardOutcome::Broken) && outcomes.len() < 20 {
            let blow = Blow::new(40., 10., Vec3::NEG_Z, BlowKind::Bolt);
            outcomes.push(duel.duelist.hit(blow).expect("alive").guard);
        }
        let (broken, blocked) = outcomes.split_last().expect("some");
        assert_eq!(*broken, GuardOutcome::Broken, "{outcomes:?}");
        assert!(blocked.len() >= 2 && blocked.iter().all(|o| *o == GuardOutcome::Blocked));
        assert_eq!(duel.duelist.action, Action::Stagger);
    }

    #[test]
    fn a_duelist_attacks_only_in_reach_every_third_a_heavy_and_rests_after() {
        let mut far = Duel::new(3.);
        far.wait(5.);
        assert!(far.duelist.guard_up && far.brain.openings == 0);
        let mut duel = Duel::new(1.5);
        let mut kinds = Vec::new();
        let (mut previous, mut down, mut rested) = (Action::Ready, 0_u32, None);
        for _ in 0..(20 * 60) {
            duel.tick(false, false);
            let now = duel.duelist.action;
            if previous == Action::Ready && matches!(now, Action::Light | Action::Heavy) {
                kinds.push(now);
            }
            if previous != Action::Ready && now == Action::Ready {
                down = 0;
            }
            if now == Action::Ready && rested.is_none() && !kinds.is_empty() {
                if duel.duelist.guard_up {
                    rested = Some(down);
                } else {
                    down += 1;
                }
            }
            previous = now;
        }
        assert!(kinds.len() >= 6, "{kinds:?}");
        for (index, kind) in kinds.iter().enumerate() {
            let want = if (index + 1) % 3 == 0 {
                Action::Heavy
            } else {
                Action::Light
            };
            assert_eq!(*kind, want, "attack {}: {kinds:?}", index + 1);
        }
        let rest = duel.tables.opponents.duelist.rest;
        let ticks = rested.expect("the guard came back");
        assert!(((ticks as f32) * DT - rest).abs() < 0.1, "{ticks} ticks");
    }

    #[test]
    fn a_duelist_that_cannot_press_keeps_its_guard_and_counts_nothing() {
        let mut duel = Duel::new(1.5);
        duel.brain.cooldown = 0.;
        duel.duelist.stamina = 0.;
        for _ in 0..30 {
            let drove = duel.tick(false, false);
            assert!(!drove.light && !drove.heavy);
            assert!(duel.duelist.guard_up || duel.duelist.stamina > 0.);
            if duel.duelist.stamina > 0. {
                break;
            }
        }
        assert_eq!(duel.brain.openings, 0);
        // Disarmed: the sim refuses every cut, so none is counted either.
        let mut duel = Duel::new(1.5);
        duel.brain.cooldown = 0.;
        duel.duelist.give_status("disarmed", 5.);
        duel.wait(2.);
        assert_eq!(duel.brain.openings, 0);
        assert!(duel.duelist.guard_up);
    }

    #[test]
    fn a_duelist_ready_to_swing_waits_out_a_heavy_it_is_reading() {
        let mut duel = Duel::new(1.5);
        duel.brain.rest = 0.;
        duel.tick(false, true);
        assert_eq!(duel.swinger.action, Action::Heavy);
        // Its cooldown has run out as it sees the cut begin.
        duel.brain.cooldown = 0.;
        let start = duel.swinger.spec(Action::Heavy).contact_start;
        while duel.swinger.age < start {
            duel.tick(false, false);
            assert!(duel.duelist.action == Action::Ready, "it did not swing");
        }
        assert_eq!(duel.brain.openings, 0);
        let dealt = duel
            .duelist
            .hit(Blow::new(20., 30., Vec3::NEG_Z, BlowKind::Cut).made_with_melee(true))
            .expect("alive");
        assert_eq!(dealt.guard, GuardOutcome::Parried);
    }

    #[test]
    fn a_stagger_drops_the_charge_and_waits() {
        let tables = CombatTables::shipped();
        let mut brain = OpponentBrain::new(OpponentKind::Sentry, &tables.opponents);
        brain.cooldown = 0.;
        let mut actor = Combatant::on_team(1, 85.);
        let mut movement = PlayerIntent::default();
        let mut input = CombatIntent::default();
        let at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -12.);
        brain.drive(
            1. / 60.,
            at,
            target,
            &Seen::at(12.),
            &tables.opponents,
            &mut actor,
            &mut movement,
            &mut input,
        );
        assert!(brain.windup.is_some() && actor.charge > 0.);
        actor.action = Action::Stagger;
        brain.drive(
            1. / 60.,
            at,
            target,
            &Seen::at(12.),
            &tables.opponents,
            &mut actor,
            &mut movement,
            &mut input,
        );
        assert!(brain.windup.is_none());
        assert!((actor.charge).abs() < f32::EPSILON);
        assert!((brain.cooldown - tables.opponents.sentry.stagger_cooldown).abs() < f32::EPSILON);
    }

    /// A bulwark as the game and the harness fit one: its sheet, a Blade in
    /// hand and its own guard, no shield, the guard not yet raised.
    fn bulwark_rig() -> (CombatTables, OpponentBrain, Combatant) {
        let tables = CombatTables::shipped();
        let brain = OpponentBrain::new(OpponentKind::Bulwark, &tables.opponents);
        let sheet = crate::progression::CharacterSheet {
            learned: ["blade.novice", "blade.cuts.1"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        };
        let mut actor = Combatant::on_team(1, tables.opponents.health);
        actor.fit(&strata_registry::progression::SkillGraph::shipped(), &sheet);
        let actor = actor
            .carrying([Some(Weapon::Blade), None], 0)
            .with_guard(Weapon::Blade, tables.opponents.bulwark.guard);
        (tables, brain, actor)
    }

    /// One tick of a bulwark at `at` facing a target at `target`, moving
    /// itself along the ground by what it asked for.
    fn bulwark_tick(
        brain: &mut OpponentBrain,
        tables: &CombatTables,
        actor: &mut Combatant,
        motion: &mut CombatMotion,
        at: &mut WorldPos,
        target: WorldPos,
    ) {
        let mut movement = PlayerIntent::default();
        let mut input = CombatIntent::default();
        let distance = (target.0 - at.0).length() as f32;
        brain.drive(
            DT,
            *at,
            target,
            &Seen::at(distance),
            &tables.opponents,
            actor,
            &mut movement,
            &mut input,
        );
        actor.facing = movement.yaw;
        actor.step(&mut input, &movement, DT, motion);
        actor.finish_contacts(&movement);
        if !actor.action.is_melee() && actor.action != Action::Stagger {
            let aim = actor.with_actor(|a| a.aim_speed_scale);
            let speed = OpponentKind::Bulwark.motor_speed(&tables.opponents, aim);
            at.0.z -= f64::from(movement.move_dir.y * speed * motion.speed_scale * DT);
        }
    }

    #[test]
    fn a_carbine_round_into_the_raised_guard_takes_a_tenth_and_a_magazine_does_not_break_it() {
        let (tables, _, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        for _ in 0..40 {
            let mut input = CombatIntent {
                aim: true,
                ..Default::default()
            };
            actor.step(&mut input, &PlayerIntent::default(), DT, &mut motion);
        }
        actor.shield = 0.;
        assert!(actor.guard_up);
        let round = tables.weapons.weapons[&Weapon::Carbine];
        let (stamina, health) = (actor.stamina, actor.health);
        let front = Vec3::new(0., 0., -15.);
        let dealt = actor
            .hit(Blow::new(
                round.damage,
                round.posture,
                front,
                BlowKind::Bolt,
            ))
            .expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Blocked);
        let bolts = tables
            .opponents
            .bulwark
            .guard
            .bolts
            .expect("its bolt figures");
        assert!((health - actor.health - round.damage * (1. - bolts.share)).abs() < 1e-3);
        assert!(
            (stamina - actor.stamina - round.damage * bolts.share * bolts.stamina).abs() < 1e-3,
            "the bolts' stamina a round"
        );
        for _ in 1..round.magazine {
            let broken = actor
                .hit(Blow::new(
                    round.damage,
                    round.posture,
                    front,
                    BlowKind::Bolt,
                ))
                .is_some_and(|d| d.guard == GuardOutcome::Broken);
            assert!(!broken, "a magazine does not break the guard");
        }
        assert!(actor.guard_up && actor.action != Action::Stagger);
    }

    #[test]
    fn the_same_round_from_behind_lands_whole_and_a_blast_beside_it_too() {
        let (tables, _, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        for _ in 0..40 {
            let mut input = CombatIntent {
                aim: true,
                ..Default::default()
            };
            actor.step(&mut input, &PlayerIntent::default(), DT, &mut motion);
        }
        actor.shield = 0.;
        let round = tables.weapons.weapons[&Weapon::Carbine];
        for from in [Vec3::new(0., 0., 15.), Vec3::new(15., 0., 0.)] {
            let mut other = actor.clone();
            let health = other.health;
            let dealt = other
                .hit(Blow::new(round.damage, round.posture, from, BlowKind::Bolt))
                .expect("it lands");
            assert_eq!(dealt.guard, GuardOutcome::Open);
            assert!((health - other.health - round.damage).abs() < 1e-3);
        }
        let health = actor.health;
        let dealt = actor
            .hit(Blow::blast(30., 20.))
            .expect("a blast lands on a raised guard");
        assert_eq!(dealt.guard, GuardOutcome::Open);
        assert!((health - actor.health - 30.).abs() < 1e-3, "whole");
    }

    #[test]
    fn a_blades_light_costs_its_guard_by_its_own_row_and_a_break_staggers() {
        let (tables, _, mut actor) = bulwark_rig();
        actor.shield = 0.;
        let mut blade = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        blade.shield = 0.;
        let mut motion = CombatMotion::default();
        let up = |c: &mut Combatant, motion: &mut CombatMotion| {
            for _ in 0..40 {
                let mut input = CombatIntent {
                    aim: true,
                    ..Default::default()
                };
                c.step(&mut input, &PlayerIntent::default(), DT, motion);
            }
        };
        up(&mut actor, &mut motion);
        up(&mut blade, &mut motion);
        let light = tables.actions.actions[&Action::Light];
        let front = Vec3::new(0., 0., -2.);
        let count = |c: &mut Combatant| {
            for n in 1..=20 {
                let dealt = c
                    .hit(Blow::new(light.damage, light.posture, front, BlowKind::Cut))
                    .expect("it lands");
                if dealt.guard == GuardOutcome::Broken {
                    return n;
                }
            }
            panic!("never broke");
        };
        let (bulwark_lights, blade_lights) = (count(&mut actor), count(&mut blade));
        // Each light costs the guard what its row says: damage times the
        // share it takes off times the stamina a point costs. A point costs
        // the bulwark's guard more than a Blade's, so it breaks sooner: the
        // slab holds bolts and a blade spends it.
        let row = tables.opponents.bulwark.guard;
        let cost = light.damage * row.share * row.stamina;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a count of lights"
        )]
        let expected = (actor.with_actor(|a| a.stamina) / cost).floor() as u32 + 1;
        assert_eq!(bulwark_lights, expected);
        assert_eq!(blade_lights, 5, "five lights break a Blade's guard");
        assert_eq!(bulwark_lights, 3, "and the third breaks the bulwark's");
        assert_eq!(actor.action, Action::Stagger, "a break staggers it");
    }

    #[test]
    fn the_bulwark_advances_guarded_and_catches_a_gunner_backing_away() {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        let back = 3.85;
        let mut target = WorldPos::new(0., 1., -15.);
        let mut reached = None;
        for tick in 0..(40 * 60) {
            target.0.z -= f64::from(back * DT);
            bulwark_tick(
                &mut brain,
                &tables,
                &mut actor,
                &mut motion,
                &mut at,
                target,
            );
            let gap = (target.0 - at.0).length();
            if gap <= f64::from(tables.opponents.bulwark.reach) {
                reached = Some(tick);
                break;
            }
            if tick > 60 {
                assert!(actor.guard_up, "the guard is up the whole way, tick {tick}");
            }
        }
        let tick = reached.expect("it reaches a target backing away at 3.85 m/s from 15 m");
        assert!(tick < 30 * 60, "in {} s", tick / 60);
    }

    #[test]
    fn a_guard_with_no_parry_window_blocks_even_on_the_tick_it_comes_up() {
        let (tables, _, mut actor) = bulwark_rig();
        actor.shield = 0.;
        let mut motion = CombatMotion::default();
        let mut input = CombatIntent {
            aim: true,
            ..Default::default()
        };
        actor.step(&mut input, &PlayerIntent::default(), DT, &mut motion);
        let round = tables.weapons.weapons[&Weapon::Carbine];
        let dealt = actor
            .hit(Blow::new(
                round.damage,
                round.posture,
                Vec3::new(0., 0., -15.),
                BlowKind::Bolt,
            ))
            .expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Blocked);
    }

    #[test]
    fn an_idle_bulwark_holds_its_guard_and_meets_a_bolt_but_no_other_kind_moves() {
        let (tables, brain, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        let at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -15.);
        for _ in 0..40 {
            let (mut movement, mut input) = (PlayerIntent::default(), CombatIntent::default());
            brain.idle(at, target, &mut actor, &mut movement, &mut input);
            assert!(input.aim);
            actor.facing = movement.yaw;
            actor.step(&mut input, &movement, DT, &mut motion);
        }
        assert!(actor.guard_up);
        actor.shield = 0.;
        let round = tables.weapons.weapons[&Weapon::Carbine];
        let dealt = actor
            .hit(Blow::new(
                round.damage,
                round.posture,
                Vec3::new(0., 0., -15.),
                BlowKind::Bolt,
            ))
            .expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Blocked);
        for kind in [
            OpponentKind::Sentry,
            OpponentKind::Rusher,
            OpponentKind::Marksman,
            OpponentKind::Duelist,
        ] {
            let other = OpponentBrain::new(kind, &tables.opponents);
            let (mut movement, mut input) = (PlayerIntent::default(), CombatIntent::default());
            let mut still = Combatant::on_team(1, 100.);
            other.idle(at, target, &mut still, &mut movement, &mut input);
            assert!(!input.aim && movement.yaw == 0., "{kind:?}");
        }
    }

    #[test]
    fn the_bulwark_is_open_from_its_cut_until_its_rest_after_and_cuts_a_follow_up() {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -1.5);
        let mut open_since_end = 0.;
        let (mut began, mut ended, mut followed) = (false, false, false);
        let mut lights = 0;
        let mut was = Action::Ready;
        // Its first cut comes after its initial cooldown.
        for _ in 0..(12 * 60) {
            bulwark_tick(
                &mut brain,
                &tables,
                &mut actor,
                &mut motion,
                &mut at,
                target,
            );
            if matches!(actor.action, Action::Light | Action::Followup) {
                began = true;
                assert!(!actor.guard_up, "no guard in its own cut");
                open_since_end = 0.;
            } else if began && actor.action == Action::Ready && !ended {
                open_since_end += DT;
                if actor.guard_up {
                    assert!(
                        open_since_end >= tables.opponents.bulwark.rest - 2. * DT,
                        "guard up {open_since_end} s after the cut"
                    );
                    ended = true;
                }
            }
            if was != Action::Light && actor.action == Action::Light {
                lights += 1;
            }
            if actor.action == Action::Followup {
                began = true;
                followed = true;
                assert!(!actor.guard_up, "no guard in its follow up");
            }
            was = actor.action;
            if ended {
                break;
            }
        }
        assert!(began && ended && followed, "a light and its follow up");
        assert_eq!(lights, 1);
        assert_eq!(brain.openings, 1);
    }

    /// A light of the tables from straight ahead of an actor facing `-z`.
    fn light_ahead(tables: &CombatTables) -> Blow {
        let light = tables.actions.actions[&Action::Light];
        Blow::new(
            light.damage,
            light.posture,
            Vec3::new(0., 0., -2.),
            BlowKind::Cut,
        )
    }

    /// Ticks of a bulwark standing in reach of its target until it begins a
    /// cut, with `each` run after every tick; `None` if it has not by `cap`
    /// seconds.
    fn seconds_to_its_cut(
        cap: f32,
        mut each: impl FnMut(f32, &CombatTables, &mut Combatant),
    ) -> Option<f32> {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -1.5);
        let mut t = 0.;
        while t < cap {
            bulwark_tick(
                &mut brain,
                &tables,
                &mut actor,
                &mut motion,
                &mut at,
                target,
            );
            t += DT;
            if actor.action == Action::Light {
                return Some(t);
            }
            each(t, &tables, &mut actor);
        }
        None
    }

    #[test]
    fn its_first_cut_counts_from_its_arrival_in_reach_and_not_from_the_start() {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let row = tables.opponents.bulwark.clone();
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        // Far enough that the walk in outlasts the wait several times over.
        let target = WorldPos::new(0., 1., -f64::from(row.reach + 4. * row.advance_speed));
        let (mut arrived, mut cut) = (None, None);
        let mut t = 0.;
        while cut.is_none() && t < 12. {
            bulwark_tick(
                &mut brain,
                &tables,
                &mut actor,
                &mut motion,
                &mut at,
                target,
            );
            t += DT;
            if arrived.is_none() && (target.0 - at.0).length() <= f64::from(row.reach) {
                arrived = Some(t);
            }
            if actor.action == Action::Light {
                cut = Some(t);
            }
        }
        let (arrived, cut) = (arrived.expect("it gets there"), cut.expect("and cuts"));
        assert!(arrived > 3. * row.initial_cooldown, "a long walk in");
        let waited = cut - arrived;
        assert!(
            (waited - row.initial_cooldown).abs() <= 3. * DT,
            "it stood {waited} s in reach before its first cut"
        );
    }

    #[test]
    fn a_cut_on_its_guard_holds_its_own_cut_back_and_a_bolt_does_not() {
        let row = CombatTables::shipped().opponents.bulwark;
        let free = seconds_to_its_cut(6., |_, _, _| {}).expect("left alone it cuts");
        assert!((free - row.initial_cooldown).abs() <= 3. * DT, "{free}");
        // One light on the slab just before it would have cut: its cut waits
        // `pinned` from the light.
        let at = row.initial_cooldown - 0.1;
        let mut landed = None;
        let held = seconds_to_its_cut(6., |t, tables, actor| {
            if landed.is_none() && t >= at {
                let dealt = actor.hit(light_ahead(tables)).expect("it lands");
                assert_eq!(dealt.guard, GuardOutcome::Blocked);
                landed = Some(t);
            }
        })
        .expect("it cuts once the hold runs out");
        let waited = held - landed.expect("the light landed");
        assert!(
            (waited - row.pinned).abs() <= 3. * DT,
            "its cut came {waited} s after the light on its guard"
        );
        // A bolt on the slab at the same moment holds nothing.
        let mut fired = false;
        let shot = seconds_to_its_cut(6., |t, _, actor| {
            if !fired && t >= at {
                let bolt = Blow::new(24., 10., Vec3::new(0., 0., -2.), BlowKind::Bolt);
                let dealt = actor.hit(bolt).expect("it lands");
                assert_eq!(dealt.guard, GuardOutcome::Blocked);
                fired = true;
            }
        })
        .expect("it cuts");
        assert!((shot - free).abs() <= DT, "a bolt moved its cut to {shot}");
    }

    #[test]
    fn a_blade_that_keeps_cutting_holds_it_behind_its_guard_until_the_guard_breaks() {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let row = tables.opponents.bulwark.clone();
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -1.5);
        // A light every half second, inside `pinned` of the last, from
        // before its first cut is due: it never cuts, and the guard, which
        // gets a little back between lights, breaks within five.
        let gap = 0.5;
        assert!(gap < row.pinned && 0.3 < row.initial_cooldown);
        let (mut t, mut next, mut lights) = (0., 0.3, 0);
        loop {
            bulwark_tick(
                &mut brain,
                &tables,
                &mut actor,
                &mut motion,
                &mut at,
                target,
            );
            t += DT;
            assert_eq!(
                actor.action,
                Action::Ready,
                "held behind its guard at {t} s"
            );
            if t >= next {
                next += gap;
                lights += 1;
                let dealt = actor.hit(light_ahead(&tables)).expect("it lands");
                if dealt.guard == GuardOutcome::Broken {
                    break;
                }
                assert_eq!(dealt.guard, GuardOutcome::Blocked);
            }
            assert!(lights < 6, "the guard never broke");
        }
        assert!((3..=5).contains(&lights), "{lights} lights broke it");
        assert_eq!(actor.action, Action::Stagger);
    }

    #[test]
    fn a_broken_guard_stays_down_for_its_stagger_cooldown() {
        let (tables, mut brain, mut actor) = bulwark_rig();
        let row = tables.opponents.bulwark.clone();
        let stagger = tables.actions.actions[&Action::Stagger].duration;
        let mut motion = CombatMotion::default();
        let mut at = WorldPos::new(0., 1., 0.);
        let target = WorldPos::new(0., 1., -1.5);
        let mut tick = |brain: &mut OpponentBrain, actor: &mut Combatant| {
            bulwark_tick(brain, &tables, actor, &mut motion, &mut at, target);
        };
        for _ in 0..12 {
            tick(&mut brain, &mut actor);
        }
        assert!(actor.guard_up);
        let mut broke = false;
        for _ in 0..3 {
            broke =
                actor.hit(light_ahead(&tables)).expect("it lands").guard == GuardOutcome::Broken;
        }
        assert!(broke, "three lights break it");
        assert_eq!(actor.action, Action::Stagger);
        // Down through the stagger and the cooldown after it, then up or
        // into its cut: never guarded inside that window.
        let mut down = 0.;
        loop {
            tick(&mut brain, &mut actor);
            if actor.guard_up || actor.action == Action::Light {
                break;
            }
            down += DT;
            assert!(down < 6., "it never recovered");
        }
        let window = stagger + row.stagger_cooldown;
        assert!(
            (down - window).abs() <= 4. * DT,
            "open {down} s after the break, wanted {window}"
        );
    }
}
