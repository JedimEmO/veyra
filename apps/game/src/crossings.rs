//! The crossings' pieces on the client (ticket 0005, step 7).
//!
//! Every piece the crossing list stands (`strata::sim::crossings::NearCrossings`)
//! within [`DRAW_M`] of the observer is spawned from its baked file in the
//! `generated://` source, at the pose the physics gives its body
//! (`strata::physics::crossings::piece_pose`), so what is drawn is what the
//! server collides with. Past [`DROP_M`] it is despawned.
//!
//! Far detail: a bridge is a few dozen elements whose farthest level (from
//! 250 m) keeps its deck, its girders, its piers and its abutments, so it
//! carries itself to [`DRAW_M`]; a culvert's headwall, a post and a stone
//! are dropped by their own ladder long before, at no cost but their entity.
//!
//! A file that is missing is said once and its piece left out, as the
//! server's physics leaves it out.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ashlar_bevy::prelude::{AshlarBuilding, MaterialLibraryAsset};
use bevy::prelude::*;
use strata::physics::crossings::piece_pose;
use strata::prelude::*;
use strata::sim::crossings::{CrossingPieceKey, NearCrossings, pieces_within};
use strata::sim::{GeneratedRoot, Observers};

/// The material library the content step writes, in the `generated://` source.
const LIBRARY: &str = "generated://materials/library.materials.ron";

/// A crossing's pieces are drawn within this of the observer, metres,
/// measured to the crossing: as far as a site's buildings. The terrain's
/// paint carries a road over a bridge's gap only past it, so it is the
/// paint's constant.
pub const DRAW_M: f64 = strata::terrain::paint::DECK_DRAWN_M;

/// And taken out past this, metres.
pub const DROP_M: f64 = 4_400.0;

/// The most piece roots spawned in one frame.
const SPAWNS_PER_FRAME: usize = 16;

/// A crossing piece's root, named for queries and the inspector.
#[derive(Component, Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the inspector and by queries while debugging"
)]
pub struct CrossingPiece {
    /// The baked file.
    pub file: String,
}

/// What is drawn, and which files exist.
#[derive(Resource, Default)]
struct Drawn {
    roots: BTreeMap<CrossingPieceKey, Entity>,
    /// Whether each file asked for is on disk; a missing one is said once.
    files: HashMap<String, bool>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Drawn>()
        .add_systems(Update, stream_crossings.run_if(in_state(AppState::InWorld)))
        .add_systems(OnExit(AppState::InWorld), clear);
}

/// The pieces a client draws seen from `eye`: every piece of the list
/// within `radius_m`, its key, its file and its pose. Borrowed and lazy:
/// it runs every frame over every piece within reach, and only the few not
/// drawn yet need anything copied.
pub(crate) fn drawn_from<'a>(
    near: &'a NearCrossings,
    eye: &'a [WorldPos; 1],
    radius_m: f64,
) -> impl Iterator<Item = (CrossingPieceKey, &'a str, WorldPos, Quat)> + 'a {
    pieces_within(near, eye, radius_m).map(|(key, _, piece)| {
        let (at, rotation) = piece_pose(piece);
        (key, piece.file.as_str(), at, rotation)
    })
}

/// Spawns the pieces within [`DRAW_M`] of the primary observer and
/// despawns those past [`DROP_M`].
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn stream_crossings(
    mut commands: Commands,
    near: Option<Res<NearCrossings>>,
    observers: Res<Observers>,
    generated: Option<Res<GeneratedRoot>>,
    server: Res<AssetServer>,
    origin: Res<RenderOrigin>,
    mut drawn: ResMut<Drawn>,
    mut library: Local<Option<Handle<MaterialLibraryAsset>>>,
) {
    let (Some(near), Some(eye)) = (near, observers.primary) else {
        return;
    };
    let keep: BTreeSet<CrossingPieceKey> = pieces_within(&near, &[eye], DROP_M)
        .map(|(key, _, _)| key)
        .collect();
    let gone: Vec<CrossingPieceKey> = drawn
        .roots
        .keys()
        .filter(|key| !keep.contains(key))
        .copied()
        .collect();
    for key in gone {
        if let Some(entity) = drawn.roots.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    let Some(root) = generated.and_then(|g| g.0.clone()) else {
        return;
    };
    let mut spawned = 0;
    let eye = [eye];
    for (key, file, at, rotation) in drawn_from(&near, &eye, DRAW_M) {
        if spawned >= SPAWNS_PER_FRAME {
            break;
        }
        if drawn.roots.contains_key(&key) {
            continue;
        }
        let exists = if let Some(&exists) = drawn.files.get(file) {
            exists
        } else {
            let exists = root.exists(file);
            if !exists {
                warn!(
                    "no baked crossing piece at generated/{}; it is left out until `just content`",
                    file
                );
            }
            drawn.files.insert(file.to_owned(), exists);
            exists
        };
        if !exists {
            continue;
        }
        let materials = library.get_or_insert_with(|| server.load(LIBRARY)).clone();
        let entity = commands
            .spawn((
                Name::new(file.to_owned()),
                CrossingPiece {
                    file: file.to_owned(),
                },
                AshlarBuilding {
                    building: server.load(format!("generated://{file}")),
                    materials,
                },
                WorldPosition(at),
                Transform::from_translation(origin.to_render(at)).with_rotation(rotation),
                Visibility::default(),
            ))
            .id();
        drawn.roots.insert(key, entity);
        spawned += 1;
    }
}

/// Leaving the world takes every drawn piece with it.
fn clear(mut commands: Commands, mut drawn: ResMut<Drawn>) {
    for (_, root) in std::mem::take(&mut drawn.roots) {
        commands.entity(root).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata::physics::crossings::COLLIDE_M;
    use strata::sim::crossings::{CrossingId, NearCrossing, PlacedPiece, ProceduralCrossing};
    use strata::worldgen::roads::{Crossing, CrossingKind, RoadClass};

    fn crossing(x: f64, index: u32) -> NearCrossing {
        NearCrossing {
            crossing: ProceduralCrossing {
                id: CrossingId {
                    mass: [0, 0],
                    road: 1,
                    index,
                },
                class: RoadClass::Service,
                crossing: Crossing {
                    kind: CrossingKind::Bridge,
                    at: [x, 0.0],
                    along_m: 50.0,
                    bearing: [0.0, 1.0],
                    span_m: 20.0,
                    deck_m: 12.5,
                    surface_m: 10.0,
                    banks_m: [10.3, 10.3],
                    width_m: 6.0,
                    depth_m: 0.5,
                    river: 0,
                    abutments_m: [40.0, 60.0],
                },
                edges: [[x, -10.0], [x, 10.0]],
            },
            placed: Ok((0..2)
                .map(|i| PlacedPiece {
                    file: format!("crossings/veyra/piece-{i}.ashlar"),
                    at: [x, 12.5 - f64::from(i), f64::from(i) * 7.0],
                    yaw: 0.3 + f64::from(i),
                })
                .collect()),
        }
    }

    /// What the client draws within the physics' reach is exactly what the
    /// physics collides with, at the same poses, and the client draws
    /// further.
    #[test]
    fn the_client_draws_what_the_server_collides_with() {
        let near = NearCrossings {
            crossings: vec![
                crossing(100.0, 1),
                crossing(2_500.0, 2),
                crossing(9_000.0, 3),
            ],
            around: Vec::new(),
        };
        let eye = [WorldPos::new(0.0, 10.0, 0.0)];
        let drawn: Vec<_> = drawn_from(&near, &eye, DRAW_M).collect();
        assert_eq!(
            drawn.len(),
            4,
            "the two crossings within reach, two pieces each"
        );
        let collides: Vec<_> = pieces_within(&near, &eye, COLLIDE_M)
            .map(|(key, _, piece)| (key, piece_pose(piece)))
            .collect();
        assert_eq!(collides.len(), 2);
        for (key, (at, rotation)) in collides {
            let (_, _, drawn_at, drawn_rotation) = drawn
                .iter()
                .find(|d| d.0 == key)
                .expect("a body the client does not draw");
            assert_eq!((*drawn_at, *drawn_rotation), (at, rotation));
        }
    }
}
