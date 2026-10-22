//! The movable render origin and the `WorldPosition -> Transform` sync
//! (ADR 0002).

use bevy::ecs::prelude::*;
use bevy::ecs::schedule::SystemSet;
use bevy::math::{DVec3, Vec3};
use bevy::transform::components::Transform;
use strata_core::WorldPos;

use crate::position::{Observers, WorldPosition};

/// The world position that render space's `(0, 0, 0)` currently maps to.
///
/// Rendered entities carry `Transform.translation = (world - origin) as f32`.
/// [`rebase_origin`] moves this when the primary observer strays more than
/// [`OriginConfig::rebase_threshold`] from it, and [`sync_transforms`]
/// rewrites every rendered transform in one pass the same frame. A
/// dedicated server has this resource too and never moves it: with no
/// observer there is no rebase, which is cheaper than a `cfg` in the sync.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct RenderOrigin(pub WorldPos);

impl RenderOrigin {
    /// How far the observer may drift from the origin before a rebase. 2 km
    /// keeps `f32` precision under 0.25 mm inside the rebased volume.
    pub const REBASE_DISTANCE: f64 = 2_000.0;

    /// A world position in render space: the only place below the render
    /// layer that produces an `f32` position.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the cast to f32 is the point; rebasing keeps the value small"
    )]
    pub fn to_render(&self, pos: WorldPos) -> Vec3 {
        let rel = pos.relative_to(self.0);
        Vec3::new(rel.x as f32, rel.y as f32, rel.z as f32)
    }

    /// A render-space position back in the world, for a client that moved
    /// something in `f32` (the flycam) and has to report where it is.
    #[must_use]
    pub fn to_world(&self, render: Vec3) -> WorldPos {
        self.0
            + DVec3::new(
                f64::from(render.x),
                f64::from(render.y),
                f64::from(render.z),
            )
    }
}

/// When the origin moves.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct OriginConfig {
    /// Distance from the origin past which the primary observer pulls it
    /// along. Strictly greater than, so an observer sitting exactly on the
    /// threshold does not rebase.
    pub rebase_threshold: f64,
}

impl Default for OriginConfig {
    fn default() -> Self {
        Self {
            rebase_threshold: RenderOrigin::REBASE_DISTANCE,
        }
    }
}

/// The origin systems, in order, in `PostUpdate` before transform
/// propagation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginSet {
    /// Refresh [`Observers`] so the rebase sees this frame's camera.
    Observe,
    /// Move the origin when the primary observer is far from it.
    Rebase,
    /// Write every `Transform.translation` from its `WorldPosition`.
    Sync,
}

/// The origin moved this frame. Read by anything that caches a render-space
/// position outside a `Transform` (a physics body later, a debug overlay).
/// Entities with a `WorldPosition` need not care: the sync rewrites them.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct RenderOriginChanged {
    /// Where the origin was.
    pub from: WorldPos,
    /// Where it is now.
    pub to: WorldPos,
}

/// The translation [`sync_transforms`] last wrote, kept in debug builds to
/// catch a `Transform` somebody else moved. Inserted by the sync itself; a
/// missing one means the entity has not been synced yet, which is the one
/// frame a hand-set spawn `Transform` is allowed.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct LastSynced(pub Vec3);

/// Moves the origin to the primary observer once it is more than
/// [`OriginConfig::rebase_threshold`] away.
///
/// The new origin is the observer's position itself, not a grid snap, so
/// the observer sits at exactly `(0, 0, 0)` in render space after a
/// rebase and the next one is a full threshold of travel away.
pub fn rebase_origin(
    observers: Res<Observers>,
    config: Res<OriginConfig>,
    mut origin: ResMut<RenderOrigin>,
    mut changed: MessageWriter<RenderOriginChanged>,
) {
    let Some(primary) = observers.primary else {
        return;
    };
    if primary.distance(origin.0) > config.rebase_threshold {
        let from = origin.0;
        origin.0 = primary;
        tracing::debug!(?from, to = ?primary, "render origin rebased");
        changed.write(RenderOriginChanged { from, to: primary });
    }
}

/// `Transform.translation = (world - origin) as f32` for every entity with
/// both, every frame.
///
/// Only writes when the value differs, so a static region does not dirty
/// transform propagation every frame. In debug builds, panics when a
/// translation differs from what this system last wrote: that is a system
/// moving a rendered sim entity through its `Transform`, which the rebase
/// would silently undo (ADR 0002).
pub fn sync_transforms(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut query: Query<(
        Entity,
        &WorldPosition,
        &mut Transform,
        Option<&mut LastSynced>,
    )>,
) {
    for (entity, pos, mut transform, last) in &mut query {
        let translation = origin.to_render(pos.0);
        if cfg!(debug_assertions) {
            match last {
                Some(mut last) => {
                    assert!(
                        transform.translation == last.0,
                        "entity {entity} has a WorldPosition but its Transform.translation was set \
                         by hand ({:?}, last synced {:?}); move it through WorldPosition instead",
                        transform.translation,
                        last.0
                    );
                    last.0 = translation;
                }
                None => {
                    commands.entity(entity).insert(LastSynced(translation));
                }
            }
        }
        if transform.translation != translation {
            transform.translation = translation;
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
    use crate::position::{PrimaryObserver, WorldObserver};
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;

    fn origin_of(app: &App) -> WorldPos {
        app.world().resource::<RenderOrigin>().0
    }

    fn changes(app: &mut App) -> Vec<RenderOriginChanged> {
        app.world_mut()
            .resource_mut::<Messages<RenderOriginChanged>>()
            .drain()
            .collect()
    }

    use bevy::app::App;

    #[test]
    fn nothing_rebases_without_an_observer() {
        let mut app = app_in_world();
        app.world_mut()
            .spawn((WorldPosition::new(50_000.0, 0.0, 0.0), Transform::default()));
        app.update();
        assert_eq!(origin_of(&app), WorldPos::ZERO);
    }

    #[test]
    fn the_origin_follows_the_primary_observer_past_the_threshold() {
        let mut app = app_in_world();
        let camera = app
            .world_mut()
            .spawn((PrimaryObserver, WorldPosition::new(1_999.0, 0.0, 0.0)))
            .id();
        app.update();
        assert_eq!(origin_of(&app), WorldPos::ZERO, "under the threshold");
        assert!(changes(&mut app).is_empty());

        app.world_mut().get_mut::<WorldPosition>(camera).unwrap().0 =
            WorldPos::new(2_000.0, 0.0, 0.0);
        app.update();
        assert_eq!(origin_of(&app), WorldPos::ZERO, "exactly on it");

        let far = WorldPos::new(2_000.5, 10.0, -3.0);
        app.world_mut().get_mut::<WorldPosition>(camera).unwrap().0 = far;
        app.update();
        assert_eq!(origin_of(&app), far);
        let msgs = changes(&mut app);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from, WorldPos::ZERO);
        assert_eq!(msgs[0].to, far);

        // Sitting still: no second rebase.
        app.update();
        assert!(changes(&mut app).is_empty());
    }

    #[test]
    fn a_plain_observer_stands_in_for_a_missing_primary() {
        let mut app = app_in_world();
        app.world_mut()
            .spawn((WorldObserver, WorldPosition::new(0.0, 0.0, 3_000.0)));
        app.update();
        assert_eq!(origin_of(&app), WorldPos::new(0.0, 0.0, 3_000.0));
    }

    #[test]
    fn transforms_are_rewritten_after_a_rebase() {
        let mut app = app_in_world();
        let camera = app
            .world_mut()
            .spawn((
                PrimaryObserver,
                WorldPosition::new(100.0, 30.0, 100.0),
                Transform::default(),
            ))
            .id();
        let region = app
            .world_mut()
            .spawn((WorldPosition::new(64.0, 0.0, 128.0), Transform::default()))
            .id();
        app.update();
        let t = |app: &App, e: Entity| app.world().get::<Transform>(e).unwrap().translation;
        assert_eq!(t(&app, camera), Vec3::new(100.0, 30.0, 100.0));
        assert_eq!(t(&app, region), Vec3::new(64.0, 0.0, 128.0));

        let far = WorldPos::new(10_000.0, 30.0, 100.0);
        app.world_mut().get_mut::<WorldPosition>(camera).unwrap().0 = far;
        app.update();
        assert_eq!(origin_of(&app), far);
        assert_eq!(t(&app, camera), Vec3::ZERO);
        assert_eq!(t(&app, region), Vec3::new(64.0 - 10_000.0, -30.0, 28.0));
    }

    #[test]
    fn precision_survives_far_from_the_world_origin() {
        let mut app = app_in_world();
        let base = WorldPos::new(80_000.0, 0.0, -120_000.0);
        let camera = app
            .world_mut()
            .spawn((PrimaryObserver, WorldPosition(base), Transform::default()))
            .id();
        let nearby = app
            .world_mut()
            .spawn((
                WorldPosition(base + DVec3::new(1.001, 0.0, 0.0)),
                Transform::default(),
            ))
            .id();
        app.update();
        let cam = app.world().get::<Transform>(camera).unwrap().translation;
        let near = app.world().get::<Transform>(nearby).unwrap().translation;
        // 80 km in f32 has an 8 mm step; after the rebase the millimetre is
        // exact.
        assert!((near.x - cam.x - 1.001).abs() < 1e-5);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "set by hand")]
    fn a_hand_set_transform_is_a_bug() {
        let mut app = app_in_world();
        let e = app
            .world_mut()
            .spawn((WorldPosition::new(1.0, 2.0, 3.0), Transform::default()))
            .id();
        app.update();
        app.world_mut()
            .get_mut::<Transform>(e)
            .unwrap()
            .translation
            .x += 1.0;
        app.update();
    }

    #[test]
    fn render_and_world_round_trip() {
        let origin = RenderOrigin(WorldPos::new(5_000.0, 0.0, -5_000.0));
        let p = WorldPos::new(5_001.5, 2.0, -4_999.25);
        assert_eq!(origin.to_render(p), Vec3::new(1.5, 2.0, 0.75));
        assert_eq!(origin.to_world(Vec3::new(1.5, 2.0, 0.75)), p);
    }
}
