//! The [`Authority`] port: who has the final say on what happens in the
//! simulation (ADR 0001).
//!
//! A client expresses what it wants as an [`Intent`] and hands it to the
//! authority. In the slice the authority is [`LocalAuthority`], which
//! accepts everything at once: the prediction is the truth. On a networked
//! client the adapter forwards intents to the server and what comes back is
//! what gets applied; on the server the adapter validates them. The systems
//! in [`SimSet::Input`](crate::SimSet::Input) only ever see accepted
//! intents, so none of them change when the adapter does.
//!
//! Deliberately minimal: two intents and three methods. The shape matters
//! now, the vocabulary grows with the game.

use bevy::ecs::prelude::*;
use bevy::math::DVec3;
use strata_core::WorldPos;

use crate::position::WorldPosition;

/// Something a client wants to happen to the simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    /// Put an entity at a world position: a spawn placement, a teleport, or
    /// the client reporting where its own camera went.
    Place {
        /// The entity to move.
        entity: Entity,
        /// Where it goes.
        at: WorldPos,
    },
    /// Move an entity by a displacement in metres.
    Move {
        /// The entity to move.
        entity: Entity,
        /// How far, in world axes.
        by: DVec3,
    },
}

impl Intent {
    /// The entity the intent is about.
    #[must_use]
    pub fn entity(&self) -> Entity {
        match self {
            Self::Place { entity, .. } | Self::Move { entity, .. } => *entity,
        }
    }
}

/// Why an authority refused an intent. A networked adapter adds variants;
/// a local one never produces any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentRejected {
    /// The authority is not accepting intents right now (not connected,
    /// not yet in the world).
    Unavailable,
    /// The intent was refused by the rules.
    Denied,
}

impl core::fmt::Display for IntentRejected {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the authority is unavailable"),
            Self::Denied => f.write_str("the intent was denied"),
        }
    }
}

impl core::error::Error for IntentRejected {}

/// The port. Implemented by [`LocalAuthority`] here and by a networked
/// adapter in a crate above.
pub trait Authority: Send + Sync + 'static {
    /// Whether this process decides outcomes itself. True on a server and in
    /// single player; false on a networked client, which predicts and waits
    /// to be corrected.
    fn is_authoritative(&self) -> bool;

    /// Submits an intent. `Ok` means it was taken, not that it will be
    /// applied: the accepted ones come back through
    /// [`drain_accepted`](Self::drain_accepted), possibly on a later tick.
    fn apply_intent(&mut self, intent: Intent) -> Result<(), IntentRejected>;

    /// Moves every intent accepted since the last call into `out`, in the
    /// order they are to be applied.
    fn drain_accepted(&mut self, out: &mut Vec<Intent>);
}

/// The single-process authority: every intent is accepted the tick it
/// arrives, in submission order.
#[derive(Debug, Default)]
pub struct LocalAuthority {
    accepted: Vec<Intent>,
}

impl Authority for LocalAuthority {
    fn is_authoritative(&self) -> bool {
        true
    }

    fn apply_intent(&mut self, intent: Intent) -> Result<(), IntentRejected> {
        self.accepted.push(intent);
        Ok(())
    }

    fn drain_accepted(&mut self, out: &mut Vec<Intent>) {
        out.append(&mut self.accepted);
    }
}

/// The authority in use, as a resource. [`AppPlugin`](crate::AppPlugin)
/// inserts a [`LocalAuthority`] unless the app inserted something else
/// before adding the plugin.
#[derive(Resource)]
pub struct SimAuthority(pub Box<dyn Authority>);

impl SimAuthority {
    /// Wraps an adapter.
    pub fn new(adapter: impl Authority) -> Self {
        Self(Box::new(adapter))
    }

    /// Hands an intent to the adapter.
    pub fn submit(&mut self, intent: Intent) -> Result<(), IntentRejected> {
        self.0.apply_intent(intent)
    }

    /// Whether this process decides outcomes itself.
    #[must_use]
    pub fn is_authoritative(&self) -> bool {
        self.0.is_authoritative()
    }
}

impl Default for SimAuthority {
    fn default() -> Self {
        Self::new(LocalAuthority::default())
    }
}

impl core::fmt::Debug for SimAuthority {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimAuthority")
            .field("authoritative", &self.is_authoritative())
            .finish_non_exhaustive()
    }
}

/// Applies the accepted intents to [`WorldPosition`]s. An intent about an
/// entity that no longer exists or has no position is dropped with a log
/// line: on a networked client that is the ordinary race between a despawn
/// and a late ack.
pub fn apply_intents(
    mut authority: ResMut<SimAuthority>,
    mut pending: Local<Vec<Intent>>,
    mut positions: Query<&mut WorldPosition>,
) {
    pending.clear();
    authority.0.drain_accepted(&mut pending);
    for intent in pending.drain(..) {
        let Ok(mut pos) = positions.get_mut(intent.entity()) else {
            tracing::debug!(
                ?intent,
                "intent for an entity without a WorldPosition, dropped"
            );
            continue;
        };
        match intent {
            Intent::Place { at, .. } => pos.0 = at,
            Intent::Move { by, .. } => pos.0 += by,
        }
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

    #[test]
    fn the_local_authority_applies_intents_on_the_next_tick() {
        let mut app = app_in_world();
        let e = app
            .world_mut()
            .spawn(WorldPosition::new(1.0, 0.0, 0.0))
            .id();
        let mut authority = app.world_mut().resource_mut::<SimAuthority>();
        assert!(authority.is_authoritative());
        authority
            .submit(Intent::Place {
                entity: e,
                at: WorldPos::new(10.0, 0.0, 0.0),
            })
            .unwrap();
        authority
            .submit(Intent::Move {
                entity: e,
                by: DVec3::new(0.0, 0.0, 2.5),
            })
            .unwrap();
        // Nothing moves until the sim ticks; one update is one tick here.
        assert_eq!(
            app.world().get::<WorldPosition>(e).unwrap().0,
            WorldPos::new(1.0, 0.0, 0.0)
        );
        app.update();
        assert_eq!(
            app.world().get::<WorldPosition>(e).unwrap().0,
            WorldPos::new(10.0, 0.0, 2.5)
        );
    }

    #[test]
    fn an_intent_for_a_missing_entity_is_dropped() {
        let mut app = app_in_world();
        let e = app.world_mut().spawn_empty().id();
        app.world_mut().despawn(e);
        app.world_mut()
            .resource_mut::<SimAuthority>()
            .submit(Intent::Move {
                entity: e,
                by: DVec3::X,
            })
            .unwrap();
        app.update();
    }

    /// An adapter that refuses everything, standing in for a disconnected
    /// client.
    struct Offline;

    impl Authority for Offline {
        fn is_authoritative(&self) -> bool {
            false
        }

        fn apply_intent(&mut self, _: Intent) -> Result<(), IntentRejected> {
            Err(IntentRejected::Unavailable)
        }

        fn drain_accepted(&mut self, _: &mut Vec<Intent>) {}
    }

    #[test]
    fn the_app_keeps_an_authority_inserted_before_the_plugin() {
        let mut app = bevy::app::App::new();
        app.insert_resource(SimAuthority::new(Offline));
        app.add_plugins((bevy::time::TimePlugin, crate::AppPlugin));
        let mut authority = app.world_mut().resource_mut::<SimAuthority>();
        assert!(!authority.is_authoritative());
        assert_eq!(
            authority.submit(Intent::Move {
                entity: Entity::PLACEHOLDER,
                by: DVec3::ZERO,
            }),
            Err(IntentRejected::Unavailable)
        );
    }
}
