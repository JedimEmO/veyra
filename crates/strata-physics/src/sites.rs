//! The procedural sites' buildings as colliders, on the server as on the
//! client (ticket 0005, step 4).
//!
//! [`NearSites`] says which building stands where on every site near an
//! observer; the same list the client draws from. A building within
//! [`COLLIDE_M`] of an observer gets one static body, a compound of its
//! baked file's nearest level's proxies (read with plain `ashlar`, as the
//! settlement's are), and loses it past [`RELEASE_M`]. A flight over
//! hundreds of sites holds only the few near it. A file is read once, on
//! the task pool, and shared by every site that stands it; a file that is
//! missing or unreadable is said once and its building does not collide.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use strata_registry::files::Files;

use ashlar::ConvexSolid;
use avian3d::prelude::{RigidBody, Rotation};
use bevy::ecs::prelude::*;
use bevy::math::Quat;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::components::Transform;
use strata_app::sites::{NearSites, PlacedSiteBuilding, SiteBuildingKey, buildings_within};
use strata_app::{GeneratedRoot, Observers, RenderOrigin, WorldPosition};
use strata_core::WorldPos;
use tracing::warn;

use crate::grounding::collider;
use crate::settlement::read_collision;

/// A site building collides within this of an observer, metres, measured to
/// its site's centre: past the terrain's own colliders' reach round a
/// walker, and short enough that a flight holds a handful.
pub const COLLIDE_M: f64 = 600.0;

/// And stops colliding past this, metres, so an observer at the line does
/// not churn its bodies.
pub const RELEASE_M: f64 = 800.0;

/// A site building's static body.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SiteCollider {
    /// The site's name.
    pub site: String,
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// The building's key: its site's id key and its index there.
    pub key: SiteBuildingKey,
}

/// Where a site building stands and how it is turned: the one pose its
/// body here and its drawing on a client are both given.
#[must_use]
pub fn building_pose(building: &PlacedSiteBuilding) -> (WorldPos, Quat) {
    #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
    let rotation = Quat::from_rotation_y(building.yaw as f32);
    (
        WorldPos::new(building.at[0], building.at[1], building.at[2]),
        rotation,
    )
}

/// The files read so far: their proxies, or `None` for a file that is
/// missing, unreadable or has none.
type Solids = HashMap<String, Option<Arc<Vec<ConvexSolid>>>>;

/// A file read on the task pool: its proxies, or why not.
type Read = Vec<(String, Result<Vec<ConvexSolid>, String>)>;

/// Static bodies of baked pieces kept round the observers, by a key the
/// same everywhere, with the files read so far and the read in flight:
/// what a site's buildings and a crossing's pieces both are to the
/// physics.
pub(crate) struct Streamed<K> {
    bodies: BTreeMap<K, Entity>,
    solids: Solids,
    reading: Option<Task<Read>>,
}

impl<K> Default for Streamed<K> {
    fn default() -> Self {
        Self {
            bodies: BTreeMap::new(),
            solids: Solids::default(),
            reading: None,
        }
    }
}

/// One piece a [`Streamed`] wants a body for: its key, its file, its pose
/// and what makes the marker its body carries. Borrowed, and the marker
/// made only for a body that is spawned: every piece within reach is
/// offered every frame, and nearly all of them already have one.
pub(crate) struct Wanted<'a, K, F> {
    pub key: K,
    pub file: &'a str,
    pub at: WorldPos,
    pub rotation: Quat,
    pub marker: F,
}

impl<K: Ord + Copy> Streamed<K> {
    /// Takes in a finished read, despawns every body whose key is not in
    /// `keep`, and spawns one for every piece of `wanted` that has none and
    /// whose file is read; starts reading the files not read yet. A file
    /// that is missing, unreadable or makes no collider is said once (with
    /// `what` it is) and its pieces do not collide.
    pub(crate) fn stream<'a, C: Component, F: FnOnce() -> C>(
        &mut self,
        commands: &mut Commands,
        keep: &BTreeSet<K>,
        wanted: impl Iterator<Item = Wanted<'a, K, F>>,
        root: Option<Files>,
        origin: &RenderOrigin,
        what: &str,
    ) {
        if let Some(task) = self.reading.as_mut()
            && let Some(read) = block_on(poll_once(task))
        {
            self.reading = None;
            for (file, result) in read {
                let solids = match result {
                    Ok(solids) if !solids.is_empty() => Some(Arc::new(solids)),
                    Ok(_) => None,
                    Err(error) => {
                        warn!("{error}; the {what} will not collide");
                        None
                    }
                };
                self.solids.insert(file, solids);
            }
        }
        let gone: Vec<K> = self
            .bodies
            .keys()
            .filter(|key| !keep.contains(key))
            .copied()
            .collect();
        for key in gone {
            if let Some(entity) = self.bodies.remove(&key) {
                commands.entity(entity).despawn();
            }
        }
        let Some(root) = root else {
            return;
        };
        let mut unread = BTreeSet::new();
        for piece in wanted {
            if self.bodies.contains_key(&piece.key) {
                continue;
            }
            let Some(solids) = self.solids.get(piece.file) else {
                unread.insert(piece.file.to_owned());
                continue;
            };
            let Some(solids) = solids else {
                continue;
            };
            let body = match collider(solids) {
                Ok(body) => body,
                Err(error) => {
                    warn!(file = %piece.file, "{error}; the {what} will not collide");
                    self.solids.insert(piece.file.to_owned(), None);
                    continue;
                }
            };
            let entity = commands
                .spawn((
                    (piece.marker)(),
                    RigidBody::Static,
                    body,
                    WorldPosition(piece.at),
                    avian3d::prelude::Position(origin.to_render(piece.at)),
                    Rotation(piece.rotation),
                    Transform::default(),
                ))
                .id();
            self.bodies.insert(piece.key, entity);
        }
        if !unread.is_empty() && self.reading.is_none() {
            let files: Vec<String> = unread.into_iter().collect();
            self.reading = Some(AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(
                async move {
                    files
                        .into_iter()
                        .map(|file| {
                            let read = read_collision(&root, &file);
                            (file, read)
                        })
                        .collect()
                },
            ));
        }
    }
}

/// The site bodies spawned, the files read, and the read in flight.
#[derive(Resource, Default)]
pub struct SiteBodies(Streamed<SiteBuildingKey>);

impl SiteBodies {
    /// The bodies spawned, by building key.
    #[must_use]
    pub fn bodies(&self) -> &BTreeMap<SiteBuildingKey, Entity> {
        &self.0.bodies
    }
}

/// Keeps a body for every site building within [`COLLIDE_M`] of an
/// observer and none past [`RELEASE_M`], reading the files they need first.
pub(crate) fn stream_site_colliders(
    mut commands: Commands,
    near: Option<Res<NearSites>>,
    observers: Res<Observers>,
    generated: Option<Res<GeneratedRoot>>,
    origin: Res<RenderOrigin>,
    mut state: ResMut<SiteBodies>,
) {
    let Some(near) = near else {
        return;
    };
    let positions: &[WorldPos] = &observers.positions;
    let keep: BTreeSet<SiteBuildingKey> = buildings_within(&near, positions, RELEASE_M)
        .map(|(key, _, _)| key)
        .collect();
    let wanted = buildings_within(&near, positions, COLLIDE_M).map(|(key, site, building)| {
        let (at, rotation) = building_pose(building);
        Wanted {
            key,
            file: building.file.as_str(),
            at,
            rotation,
            marker: move || SiteCollider {
                site: site.site.name.clone(),
                file: building.file.clone(),
                key,
            },
        }
    });
    let root = generated.and_then(|g| g.0.clone());
    state
        .0
        .stream(&mut commands, &keep, wanted, root, &origin, "site building");
}
