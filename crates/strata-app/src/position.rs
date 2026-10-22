//! World positions as components, and the observers that drive streaming.

use bevy::ecs::prelude::*;
use serde::{Deserialize, Serialize};
use strata_core::WorldPos;

/// Where an entity is in the world, in `f64` metres (ADR 0002).
///
/// A newtype over [`WorldPos`] rather than a `Component` impl on the core
/// type, so `strata-core` stays free of Bevy. This is the position the
/// simulation moves; a rendered entity's `Transform` is derived from it in
/// [`OriginSet::Sync`](crate::OriginSet::Sync) every frame and must not be
/// written by anything else. Positions below the render layer never see an
/// `f32`. Serialised as the three `f64`s it is: a replicated position
/// has to survive the wire at the precision the sim keeps it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldPosition(pub WorldPos);

impl WorldPosition {
    /// A position from its components.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self(WorldPos::new(x, y, z))
    }
}

impl From<WorldPos> for WorldPosition {
    fn from(pos: WorldPos) -> Self {
        Self(pos)
    }
}

/// An entity whose position drives streaming: the camera in the slice, each
/// player on a server later.
///
/// Named `WorldObserver` and not `Observer` because `bevy_ecs::Observer` is
/// a component too, and a marker that shadows it in the prelude is a bug
/// that compiles. Requires [`WorldPosition`], since an observer without a
/// position observes nothing.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[require(WorldPosition)]
pub struct WorldObserver;

/// The observer the render origin follows (ADR 0002). One per client; a
/// server has none and never rebases. When absent the first
/// [`WorldObserver`] stands in, so a single-observer app need not mark it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[require(WorldObserver)]
pub struct PrimaryObserver;

/// The observer positions as of the last collection, for systems that want
/// a slice rather than a query: the pure streaming selection takes
/// `&[WorldPos]`.
///
/// Refreshed at the end of every [`SimTick`](crate::SimTick) and again in
/// `PostUpdate` before the rebase, so a camera moved in `Update` is seen the
/// same frame by the origin and the next frame by the streamer. One frame
/// of streaming latency is nothing against a load budget.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct Observers {
    /// Every observer, in query order.
    pub positions: Vec<WorldPos>,
    /// The [`PrimaryObserver`], or the first observer when none is marked.
    pub primary: Option<WorldPos>,
}

impl Observers {
    /// True when there is nothing to stream for.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Copies [`WorldObserver`] positions into [`Observers`].
pub fn collect_observers(
    mut observers: ResMut<Observers>,
    query: Query<(&WorldPosition, Has<PrimaryObserver>), With<WorldObserver>>,
) {
    observers.positions.clear();
    observers.primary = None;
    for (pos, primary) in &query {
        observers.positions.push(pos.0);
        if primary {
            observers.primary = Some(pos.0);
        }
    }
    if observers.primary.is_none() {
        observers.primary = observers.positions.first().copied();
    }
}
