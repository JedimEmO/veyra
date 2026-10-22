//! The player: the one entity in the simulation that a person drives.
//!
//! A [`Player`] is sim state like anything else. It has a [`WorldPosition`]
//! and it moves only when the simulation steps it. What the person wants is
//! a [`PlayerIntent`], written by the client's input systems (or, later, by
//! the network on the server) whenever they like, at whatever rate the
//! frames come. Once per tick, in [`SimSet::Input`](crate::SimSet::Input),
//! the intent is consumed into a [`PlayerCommand`]: the command is what the
//! tick acts on, and the one-shot part of the intent (the jump) is cleared,
//! so a press between two ticks is one jump and not none or two. The
//! movement itself is stepped by the physics crate in
//! [`SimSet::Step`](crate::SimSet::Step); this crate never decides how far
//! the player got.
//!
//! A player is a [`WorldObserver`]: the server streams regions around every
//! connected one. The [`LocalPlayer`] is the one this process controls and
//! is the [`PrimaryObserver`] the render origin follows, the role the flycam
//! had alone before there was a player (ADR 0002).
//!
//! [`PreviousWorldPosition`] is recorded at the start of every tick so a
//! client can draw the player between two ticks; a 20 Hz step under a 200
//! fps camera would otherwise show as a stutter.

use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_core::WorldPos;

pub use strata_types::{CombatMotion, PlayerCommand, PlayerIntent};

use crate::position::{PrimaryObserver, WorldObserver, WorldPosition};

/// Identifies a player across processes. Assigned by whoever owns the
/// session (the app in the slice, the server later) and replicated with the
/// player so two clients name the same person the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PlayerId(pub u64);

/// A player in the simulation. Requires a position, an observer role for
/// the streamer, an intent to read and a command to act on.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[require(WorldObserver, PlayerIntent, PlayerCommand, PreviousWorldPosition)]
pub struct Player {
    /// Who this is.
    pub id: PlayerId,
}

/// The player this process controls. The primary observer: the render
/// origin and the streaming priority follow it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[require(PrimaryObserver)]
pub struct LocalPlayer;

/// Where the entity was at the start of the current tick, for interpolation
/// between ticks. `None` until the first tick has recorded it, so a client
/// never interpolates from nowhere.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct PreviousWorldPosition(pub Option<WorldPos>);

impl PreviousWorldPosition {
    /// The position `t` of the way from the previous tick to `current`,
    /// which is `current` itself when nothing was recorded yet.
    #[must_use]
    pub fn interpolate(&self, current: WorldPos, t: f64) -> WorldPos {
        match self.0 {
            Some(previous) => WorldPos(previous.0.lerp(current.0, t.clamp(0.0, 1.0))),
            None => current,
        }
    }
}

/// An intent written by a script rather than by input: the headless run's
/// walking player and the client's `--auto-walk`. Copied over the
/// [`PlayerIntent`] every tick, so a `jump` here jumps every tick.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct ScriptedIntent(pub PlayerIntent);

/// Records every [`PreviousWorldPosition`] before anything in the tick
/// moves.
pub fn record_previous_positions(mut query: Query<(&WorldPosition, &mut PreviousWorldPosition)>) {
    for (position, mut previous) in &mut query {
        previous.0 = Some(position.0);
    }
}

/// Copies scripted intents over the live ones.
pub fn apply_scripted_intents(mut query: Query<(&ScriptedIntent, &mut PlayerIntent)>) {
    for (scripted, mut intent) in &mut query {
        *intent = scripted.0;
    }
}

/// Takes this tick's [`PlayerCommand`] from the [`PlayerIntent`] and clears
/// the jump, which is an edge and not a level.
pub fn consume_player_intents(mut query: Query<(&mut PlayerIntent, &mut PlayerCommand)>) {
    for (mut intent, mut command) in &mut query {
        command.0 = *intent;
        intent.jump = false;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::tests::app_in_world;
    use bevy::app::App;
    use bevy::math::{Vec2, Vec3};

    fn command(app: &App, e: Entity) -> PlayerIntent {
        app.world().get::<PlayerCommand>(e).unwrap().0
    }

    #[test]
    fn a_player_observes_the_world() {
        let mut app = app_in_world();
        let p = app.world_mut().spawn((Player::default(), LocalPlayer)).id();
        assert!(app.world().get::<WorldObserver>(p).is_some());
        assert!(app.world().get::<PrimaryObserver>(p).is_some());
        assert!(app.world().get::<WorldPosition>(p).is_some());
        assert!(app.world().get::<PlayerIntent>(p).is_some());
    }

    #[test]
    fn a_jump_is_consumed_by_exactly_one_tick() {
        let mut app = app_in_world();
        let p = app.world_mut().spawn(Player::default()).id();
        app.world_mut().get_mut::<PlayerIntent>(p).unwrap().jump = true;
        app.update();
        assert!(command(&app, p).jump, "the first tick sees the jump");
        assert!(!app.world().get::<PlayerIntent>(p).unwrap().jump);
        app.update();
        assert!(!command(&app, p).jump, "the second tick does not");
    }

    #[test]
    fn the_command_is_a_copy_of_the_intent_at_the_tick() {
        let mut app = app_in_world();
        let p = app.world_mut().spawn(Player::default()).id();
        *app.world_mut().get_mut::<PlayerIntent>(p).unwrap() = PlayerIntent {
            move_dir: Vec2::Y,
            yaw: 1.0,
            jump: false,
            sprint: true,
        };
        app.update();
        let c = command(&app, p);
        assert_eq!(c.move_dir, Vec2::Y);
        assert!(c.sprint);
        assert!((c.yaw - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_scripted_intent_overrides_the_live_one() {
        let mut app = app_in_world();
        let p = app
            .world_mut()
            .spawn((
                Player::default(),
                ScriptedIntent(PlayerIntent {
                    move_dir: Vec2::Y,
                    ..Default::default()
                }),
            ))
            .id();
        app.world_mut().get_mut::<PlayerIntent>(p).unwrap().move_dir = Vec2::X;
        app.update();
        assert_eq!(command(&app, p).move_dir, Vec2::Y);
    }

    #[test]
    fn the_previous_position_lags_one_tick() {
        let mut app = app_in_world();
        let p = app
            .world_mut()
            .spawn((Player::default(), WorldPosition::new(1.0, 0.0, 0.0)))
            .id();
        let prev = *app.world().get::<PreviousWorldPosition>(p).unwrap();
        assert_eq!(prev.0, None);
        assert_eq!(
            prev.interpolate(WorldPos::new(1.0, 0.0, 0.0), 0.5),
            WorldPos::new(1.0, 0.0, 0.0)
        );
        app.update();
        app.world_mut().get_mut::<WorldPosition>(p).unwrap().0 = WorldPos::new(3.0, 0.0, 0.0);
        app.update();
        let prev = app.world().get::<PreviousWorldPosition>(p).unwrap();
        assert_eq!(prev.0, Some(WorldPos::new(3.0, 0.0, 0.0)));
        assert_eq!(
            PreviousWorldPosition(Some(WorldPos::new(1.0, 0.0, 0.0)))
                .interpolate(WorldPos::new(3.0, 0.0, 0.0), 0.5),
            WorldPos::new(2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn directions_follow_the_camera_convention() {
        let intent = PlayerIntent {
            move_dir: Vec2::Y,
            yaw: 0.0,
            ..Default::default()
        };
        assert!((intent.world_direction() - Vec3::NEG_Z).length() < 1e-6);
        let intent = PlayerIntent {
            move_dir: Vec2::X,
            yaw: core::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        assert!((intent.world_direction() - Vec3::NEG_Z).length() < 1e-6);
        let intent = PlayerIntent {
            move_dir: Vec2::new(3.0, 4.0),
            ..Default::default()
        };
        assert!((intent.world_direction().length() - 1.0).abs() < 1e-6);
    }
}
