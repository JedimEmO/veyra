//! One streamer for the pieces of every baked building the game stands:
//! the city's, the procedural sites', the crossings' and the roadside's.
//!
//! A piece draws only within its level's band, but Bevy checks every piece
//! against its range every frame, extracts it and scans it for changes, and
//! most of a building's pieces are a level the observer is nowhere near. So
//! each piece of a root wearing [`Streamed`] is filed by building, 40 m cell,
//! level and band end, and a group whose bands cannot reach the observer from
//! anywhere in its cell is `Disabled`, out of every query, until the observer
//! comes back (the settlement module has the city's account of why). The test
//! is conservative, so the per-piece range still decides exactly what draws.
//!
//! A group also stops casting the sun's shadow past the cascades buildings
//! cast into, in or out of play: a group's members carry `NotShadowCaster`
//! exactly while it does not cast.
//!
//! Nothing here moves a piece: where a thing stands is the placement's, and
//! the colliders are built from it by the physics, not from these entities.

use std::collections::{HashMap, HashSet};

use ashlar_bevy::prelude::AshlarPiece;
use bevy::camera::visibility::VisibilityRange;
use bevy::ecs::entity_disabling::Disabled;
use bevy::light::{CascadeShadowConfig, NotShadowCaster};
use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::Observers;

/// On the root of a building whose pieces this module streams. A skyline
/// is not one: its pieces have no bands to stream.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(crate) struct Streamed;

/// The streamed roots whose pieces must stay out of play wherever the
/// observer stands: a far city's, whose roots are hidden so its pieces would
/// draw nothing and cost every pass a visit. Rewritten each frame by its
/// owner before [`stream_pieces`] runs.
#[derive(Resource, Default)]
pub(crate) struct Withheld(pub(crate) HashSet<Entity>);

/// The side of a streaming cell, metres: small enough that a cell past a
/// band's edge is not held in play by its near corner, large enough that the
/// city is a few thousand groups.
pub(crate) const CELL: f32 = 40.0;

/// How far past its band a group comes into play, and how much further it
/// must be before it leaves again, metres: brought in a moment before its
/// first piece is in range, and not switched every frame by an observer
/// standing on the boundary.
pub(crate) const EARLY: f32 = 8.0;
pub(crate) const LATE: f32 = 16.0;

/// How finely a group tells its pieces' band ends apart, metres.
///
/// Not the band itself: a piece handing over to a concatenated level has its
/// band's end pushed out by its own distance to its cell's centre, so every
/// piece of that level ends somewhere else and keying on the exact end
/// would make a group of nearly every piece. Nor the level alone: a level
/// holds pieces that end at their own level's boundary and pieces carried
/// on unchanged to the next, and a group is in play to its farthest
/// member's end, so the finest level's pieces, which end at 60 m, would be
/// held in play to 300 m by the carried ones beside them. So the end is
/// bucketed: a group reaches at most this much past any member's end.
pub(crate) const END_STEP: f32 = 32.0;

/// A group's key: its building, its cell, its pieces' level, and the bucket
/// of their band's end ([`END_STEP`]).
pub(crate) type GroupKey = (Entity, i32, i32, usize, i32);

/// The key of a piece of `root` standing at `at` in the building's frame,
/// first drawn at `level`, whose band ends at `end`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "cells of a city a kilometre across, and band ends in steps of tens of metres"
)]
pub(crate) fn group_key(root: Entity, at: Vec3, level: usize, end: f32) -> GroupKey {
    // The last level draws to any distance, `f32::MAX`.
    let bucket = if end < f32::MAX / 2.0 {
        (end / END_STEP).floor() as i32
    } else {
        i32::MAX
    };
    (
        root,
        (at.x / CELL).floor() as i32,
        (at.z / CELL).floor() as i32,
        level,
        bucket,
    )
}

/// The pieces of one building, one level, one cell and one bucket of band
/// ends.
pub(crate) struct Group {
    pub(crate) key: GroupKey,
    pub(crate) root: Entity,
    /// The bounds of the pieces' origins in the building's frame: a range is
    /// measured to a piece's origin.
    pub(crate) low: Vec3,
    pub(crate) high: Vec3,
    /// The nearest start and the farthest end of its members' bands, so the
    /// group is in play wherever any of them could be.
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) members: Vec<Entity>,
    pub(crate) enabled: bool,
    /// Whether its pieces cast the sun's shadow ([`casts`]).
    pub(crate) casts: bool,
}

// Far buildings cast no shadow (lighting plan phase 6, the profile's 4.3).
/// How much farther than the buildings' cascade reach a group still casts,
/// when no perspective camera says ([`view_slack`]): the corner of a 16:9
/// frame at 45 degrees.
pub(crate) const CASTER_VIEW_SLACK: f32 = 1.31;
/// The group's bounds are its pieces' origins, so this covers the half of a
/// far level's 80 m cell a piece reaches past its origin.
pub(crate) const CASTER_CELL_SLACK: f32 = 40.0;

/// How much farther than a depth along the view the frame's corner is.
///
/// A cascade's bound is a depth along the view, a group's distance is
/// measured from the camera, and at the corner of the frame a depth is the
/// cosine of the corner's angle off the axis times the distance: 0.76 at 45
/// degrees and 16:9, 0.38 at the settings' 100. The reach over that cosine
/// is where no piece within the bound's depth is ever taken out.
pub(crate) fn view_slack(projection: &Projection) -> f32 {
    match projection {
        Projection::Perspective(p) => {
            let tan_v = (p.fov * 0.5).tan();
            let tan_h = tan_v * p.aspect_ratio;
            (1.0 + tan_v * tan_v + tan_h * tan_h).sqrt()
        }
        _ => CASTER_VIEW_SLACK,
    }
}

/// How far from the camera a group still casts, metres, when buildings cast
/// to `reach` metres of view depth (`None`: to every cascade).
pub(crate) fn caster_limit(reach: Option<f32>, slack: f32) -> Option<f32> {
    reach.map(|reach| reach * slack + CASTER_CELL_SLACK)
}

/// And how much nearer than its reach a group that stopped casting must come
/// before it casts again, metres: an observer walking along the line does
/// not switch it every few steps.
pub(crate) const CASTER_HYSTERESIS: f32 = 16.0;

/// Whether `group` casts the sun's shadow seen from the camera at `eye`, in
/// the building's frame, within `limit` metres of it ([`caster_limit`];
/// `None`: at every distance). Past it the cascades a piece would cast into
/// are metres to a texel, and the far levels there were most of the city's
/// caster draws (`docs/perf/profile-2026-10-03.md`, 4.3).
pub(crate) fn casts(group: &Group, eye: Vec3, limit: Option<f32>) -> bool {
    let Some(limit) = limit else {
        return true;
    };
    let nearest = eye.clamp(group.low, group.high).distance(eye);
    if group.casts {
        nearest < limit
    } else {
        nearest < limit - CASTER_HYSTERESIS
    }
}

/// How far buildings cast, from the sky settings and the sun's cascades.
pub(crate) fn building_reach(
    sky: Option<&strata::atmosphere::SkySettings>,
    suns: &Query<&CascadeShadowConfig, With<strata::atmosphere::Sun>>,
) -> Option<f32> {
    let sky = sky?;
    sky.building_shadow_reach(suns.iter().next()?)
}

/// The cameras whose views the sun's cascades are fitted to.
pub(crate) type ShadowCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        &'static GlobalTransform,
        &'static Projection,
    ),
    (With<Camera3d>, Without<strata::terrain::OffWorld>),
>;

/// The active 3D camera's position in render space and its frame's slack
/// ([`view_slack`]), if there is one.
pub(crate) fn shadow_view(cameras: &ShadowCameras) -> Option<(Vec3, f32)> {
    cameras
        .iter()
        .find(|(camera, _, _)| camera.is_active)
        .map(|(_, transform, projection)| (transform.translation(), view_slack(projection)))
}

/// Every group of every building in the world.
#[derive(Resource, Default)]
pub(crate) struct Groups {
    pub(crate) groups: Vec<Group>,
    pub(crate) index: HashMap<GroupKey, usize>,
}

impl Groups {
    /// Adds a piece standing at `at` with the band `near..far` to its group,
    /// and says whether that group is in play: a piece joining a group
    /// already out of play goes out with it.
    pub(crate) fn add(
        &mut self,
        key: GroupKey,
        piece: Entity,
        at: Vec3,
        near: f32,
        far: f32,
    ) -> bool {
        let groups = &mut self.groups;
        let index = *self.index.entry(key).or_insert_with(|| {
            groups.push(Group {
                key,
                root: key.0,
                low: at,
                high: at,
                near,
                far,
                members: Vec::new(),
                enabled: true,
                casts: true,
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        group.low = group.low.min(at);
        group.high = group.high.max(at);
        group.near = group.near.min(near);
        group.far = group.far.max(far);
        group.members.push(piece);
        group.enabled
    }

    /// Whether the group under `key` casts; a group not yet made does.
    pub(crate) fn casts(&self, key: &GroupKey) -> bool {
        self.index.get(key).is_none_or(|&i| self.groups[i].casts)
    }
}

/// Groups each newly spawned piece of a streamed building by building, cell, level and
/// band end ([`group_key`]). A piece with no band draws at every distance
/// and is never streamed.
pub(crate) fn group_pieces(
    mut commands: Commands,
    mut groups: ResMut<Groups>,
    buildings: Query<(), With<Streamed>>,
    pieces: Query<
        (Entity, &ChildOf, &Transform, &AshlarPiece, &VisibilityRange),
        Added<AshlarPiece>,
    >,
) {
    for (entity, parent, transform, piece, range) in &pieces {
        let root = parent.parent();
        if !buildings.contains(root) {
            continue;
        }
        let at = transform.translation;
        let key = group_key(root, at, piece.level, range.end_margin.end);
        let in_play = groups.add(
            key,
            entity,
            at,
            range.start_margin.start,
            range.end_margin.end,
        );
        if !in_play {
            commands.entity(entity).try_insert(Disabled);
        }
        // Far buildings cast no shadow (lighting plan phase 6): a piece
        // joining a group that does not cast goes without, as the group's
        // other members do.
        if !groups.casts(&key) {
            commands.entity(entity).try_insert(NotShadowCaster);
        }
    }
}

/// Whether `group` should be in play: its building draws (its settlement is
/// near, or has no skyline to stand in for it), and one of its pieces could
/// be in band seen from `eye`, in the building's frame. A far city's pieces
/// are under a hidden root and draw nothing wherever the observer is.
pub(crate) fn should_play(drawn: bool, group: &Group, eye: Vec3) -> bool {
    drawn && in_play(group, eye)
}

/// Whether any piece of `group` could be in its band seen from `eye`, in the
/// building's frame, with the slack that keeps a group from switching every
/// frame.
pub(crate) fn in_play(group: &Group, eye: Vec3) -> bool {
    let slack = if group.enabled { LATE } else { EARLY };
    let nearest = eye.clamp(group.low, group.high).distance(eye);
    let farthest = (eye - group.low)
        .abs()
        .max((eye - group.high).abs())
        .length();
    farthest + slack >= group.near && nearest - slack < group.far
}

/// Takes groups out of play and brings them back as the observer moves, and
/// as the settlement goes far and near ([`should_play`]).
///
/// A group comes back with its `Transform` and `Visibility` inserted again:
/// while it was disabled no propagation reached it, and the render origin may
/// have moved, so what it had is stale; inserting them marks both changed.
/// A piece's own `Visibility` is `Inherited`, so marked changed it takes its
/// root's again, shown on the frame the city goes near (see `crate::settlement::install` for
/// why that is the same frame).
///
/// Every piece of a far city is disabled; ashlar-bevy's reload of a modified
/// file queries with `Allow<Disabled>`, so a hot reload still despawns them.
///
/// It also takes a group's shadow away past the cascades buildings cast
/// into and gives it back as the observer comes near ([`casts`]), in or out
/// of play: a group's members carry `NotShadowCaster` exactly while it does
/// not cast.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
pub(crate) fn stream_pieces(
    mut commands: Commands,
    mut groups: ResMut<Groups>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    withheld: Res<Withheld>,
    sky: Option<Res<strata::atmosphere::SkySettings>>,
    suns: Query<&CascadeShadowConfig, With<strata::atmosphere::Sun>>,
    cameras: ShadowCameras,
    roots: Query<&GlobalTransform, With<Streamed>>,
    pieces: Query<(&Transform, &Visibility), Allow<Disabled>>,
) {
    let Some(eye) = observers.primary else {
        return;
    };
    let eye = origin.to_render(eye);
    // The cascades are fitted to the camera's frustum, which trails the
    // observer and may be wider than the default.
    let (camera, slack) = shadow_view(&cameras).unwrap_or((eye, CASTER_VIEW_SLACK));
    let limit = caster_limit(building_reach(sky.as_deref(), &suns), slack);
    let groups = &mut *groups;
    // A building that left took its pieces with it.
    let before = groups.groups.len();
    groups.groups.retain(|g| roots.contains(g.root));
    if groups.groups.len() != before {
        groups.index = groups
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.key, i))
            .collect();
    }
    let mut frames: HashMap<Entity, (Vec3, Vec3)> = HashMap::new();
    for group in &mut groups.groups {
        let Some((local, local_camera)) = frames.get(&group.root).copied().or_else(|| {
            let to_local = roots.get(group.root).ok()?.affine().inverse();
            let local = (
                to_local.transform_point3(eye),
                to_local.transform_point3(camera),
            );
            frames.insert(group.root, local);
            Some(local)
        }) else {
            continue;
        };
        let cast = casts(group, local_camera, limit);
        if cast != group.casts {
            group.casts = cast;
            for &member in &group.members {
                if cast {
                    commands.entity(member).try_remove::<NotShadowCaster>();
                } else {
                    commands.entity(member).try_insert(NotShadowCaster);
                }
            }
        }
        let wanted = should_play(!withheld.0.contains(&group.root), group, local);
        if wanted == group.enabled {
            continue;
        }
        group.enabled = wanted;
        for &member in &group.members {
            if wanted {
                if let Ok((transform, visibility)) = pieces.get(member) {
                    commands
                        .entity(member)
                        .try_remove::<Disabled>()
                        .try_insert((*transform, *visibility));
                }
            } else {
                commands.entity(member).try_insert(Disabled);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn piece(level: usize) -> AshlarPiece {
        AshlarPiece {
            level,
            label: String::new(),
            storey: None,
            side: ashlar_bevy::ashlar::Side::Exterior,
        }
    }

    /// A band from `start` to `end` metres, with no fade.
    fn band(start: f32, end: f32) -> VisibilityRange {
        VisibilityRange {
            start_margin: start..start,
            end_margin: end..end,
            use_aabb: false,
        }
    }

    /// A site-like building of two levels at the origin, streamed: the
    /// finest to 60 m, the coarsest from there.
    fn world_with_building() -> (World, Entity, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(Groups::default());
        world.insert_resource(Withheld::default());
        world.insert_resource(RenderOrigin(WorldPos::new(0.0, 0.0, 0.0)));
        world.insert_resource(Observers {
            positions: vec![WorldPos::new(0.0, 0.0, 0.0)],
            primary: Some(WorldPos::new(0.0, 0.0, 0.0)),
        });
        let root = world
            .spawn((Streamed, Transform::default(), GlobalTransform::default()))
            .id();
        let fine = world
            .spawn((
                piece(0),
                Transform::from_xyz(1.0, 0.0, 1.0),
                Visibility::default(),
                band(0.0, 60.0),
                ChildOf(root),
            ))
            .id();
        let coarse = world
            .spawn((
                piece(1),
                Transform::from_xyz(1.0, 0.0, 1.0),
                Visibility::default(),
                band(60.0, f32::MAX),
                ChildOf(root),
            ))
            .id();
        (world, root, fine, coarse)
    }

    fn step(world: &mut World, at: f64) {
        world.insert_resource(Observers {
            positions: vec![WorldPos::new(at, 0.0, 0.0)],
            primary: Some(WorldPos::new(at, 0.0, 0.0)),
        });
        world.run_system_once(group_pieces).expect("group runs");
        world.run_system_once(stream_pieces).expect("stream runs");
    }

    fn off(world: &World, piece: Entity) -> bool {
        world.entity(piece).contains::<Disabled>()
    }

    /// A streamed site building holds only the level its observer can see,
    /// brings the other back with a margin, and a withheld root holds none.
    #[test]
    fn a_streamed_building_holds_only_the_levels_in_reach() {
        let (mut world, root, fine, coarse) = world_with_building();
        step(&mut world, 10.0);
        assert!(!off(&world, fine));
        assert!(off(&world, coarse), "the coarse level is out of play near");
        step(&mut world, 500.0);
        assert!(off(&world, fine), "the fine level is out of play far off");
        assert!(!off(&world, coarse));
        // Standing at 55 m both are in reach (the fade and the margin).
        step(&mut world, 55.0);
        assert!(!off(&world, fine) && !off(&world, coarse));
        // A withheld root keeps all of it out wherever the observer stands.
        world.resource_mut::<Withheld>().0.insert(root);
        step(&mut world, 10.0);
        assert!(off(&world, fine) && off(&world, coarse));
    }
}
