//! The roadside's markers and lamps on the client (ticket 0005, step 8).
//!
//! Every piece the roadside list stands (`strata::sim::roadside::NearRoadside`)
//! within its kind's distance of the observer is spawned from its baked file
//! in the `generated://` source, at the pose the physics gives its body
//! (`strata::physics::roadside::piece_pose`), so what is drawn is what the
//! server collides with; past that and a margin it is despawned. A lamp is
//! drawn as far as a crossing ([`crate::crossings::DRAW_M`]): its lantern is
//! what a road shows at dusk from kilometres off. A marker is drawn to
//! [`MARKER_DRAW_M`]: an 11 cm post is a fifth of a pixel there, and a
//! service road stands twenty a kilometre.
//!
//! At night the lamps nearest the observer get a warm point light each,
//! from a small pool of their own, as the city's lamps do from theirs.

use std::collections::{BTreeMap, HashMap};

use ashlar_bevy::prelude::{AshlarBuilding, MaterialLibraryAsset};
use bevy::prelude::*;
use strata::physics::roadside::piece_pose;
use strata::prelude::*;
use strata::sim::roadside::{NEAR_ROADSIDE_M, NearPiece, NearRoadside, Roadside, RoadsidePieceKey};
use strata::sim::sites::REFRESH_M;
use strata::sim::{GeneratedRoot, Observers};

/// The material library the content step writes, in the `generated://` source.
const LIBRARY: &str = "generated://materials/library.materials.ron";

/// A marker is drawn within this of the observer, metres.
pub const MARKER_DRAW_M: f64 = 1_500.0;

/// A piece is taken out this much past its distance, metres, so an
/// observer at the line does not churn it.
const DROP_MARGIN_M: f64 = 400.0;

/// The most piece roots spawned in one frame, nearest first.
const SPAWNS_PER_FRAME: usize = 24;

/// How many lamps shine at once at night: the nearest.
const LIGHTS: usize = 8;

/// A lamp shines on the road within this of the observer, metres.
const LIGHT_M: f64 = 300.0;

// The roadside list holds every piece this client keeps drawn, after the
// observer has moved a refresh from where the list was made.
const _: () = assert!(
    crate::crossings::DRAW_M + DROP_MARGIN_M + REFRESH_M <= NEAR_ROADSIDE_M,
    "the roadside list is shorter than a lamp is drawn"
);

/// The lantern's light: where it hangs on the piece (under the lantern at
/// the arm's end), its colour (the city's lamps' warm one), power and
/// reach.
const LANTERN_AT: [f64; 3] = veyra_buildings::roadside::LANTERN_AT;
const LANTERN_COLOR: [f32; 3] = [1.0, 0.62, 0.3];
const LANTERN_LUMENS: f32 = 10_000.0;
const LANTERN_RANGE_M: f32 = 22.0;

/// A roadside piece's root, named for queries and the inspector.
#[derive(Component, Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the inspector and by queries while debugging"
)]
pub struct RoadsidePiece {
    /// The baked file.
    pub file: String,
}

/// What is drawn, and which files exist.
#[derive(Resource, Default)]
struct Drawn {
    roots: BTreeMap<RoadsidePieceKey, Entity>,
    /// Whether each file asked for is on disk; a missing one is said once.
    files: HashMap<String, bool>,
}

/// The lamps' point lights, reused for whichever are nearest.
#[derive(Resource, Default)]
struct LampLights {
    lights: Vec<Entity>,
    at: Option<WorldPos>,
    darkness: f32,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Drawn>()
        .init_resource::<LampLights>()
        .add_systems(
            Update,
            (stream_roadside, lamp_lights).run_if(in_state(AppState::InWorld)),
        )
        .add_systems(OnExit(AppState::InWorld), clear);
}

/// Whether a piece is a lamp: the kit's lamp file, `lamp`.
fn is_lamp(piece: &NearPiece, lamp: Option<&str>) -> bool {
    lamp.is_some_and(|lamp| piece.piece.file == lamp)
}

/// The kit's lamp file, if the kit has one.
fn lamp_file(roadside: Option<&Roadside>) -> Option<&str> {
    roadside?.kit.as_deref()?.lamp.as_deref()
}

/// How far a piece is drawn, metres.
fn draw_m(piece: &NearPiece, lamp: Option<&str>) -> f64 {
    if is_lamp(piece, lamp) {
        crate::crossings::DRAW_M
    } else {
        MARKER_DRAW_M
    }
}

/// The squared horizontal distance from `eye` to a piece.
fn distance2(eye: WorldPos, piece: &NearPiece) -> f64 {
    let (dx, dz) = (piece.piece.at[0] - eye.x(), piece.piece.at[2] - eye.z());
    dx * dx + dz * dz
}

/// The pieces a client draws seen from `eye` that are not drawn yet, each
/// within its kind's distance (`lamp` is the kit's lamp file), nearest
/// first, at most `most`.
pub(crate) fn wanted<'a>(
    near: &'a NearRoadside,
    eye: WorldPos,
    lamp: Option<&str>,
    drawn: &BTreeMap<RoadsidePieceKey, Entity>,
    most: usize,
) -> Vec<&'a NearPiece> {
    let mut out: Vec<(f64, &NearPiece)> = near
        .pieces
        .iter()
        .filter(|p| !drawn.contains_key(&p.key))
        .map(|p| (distance2(eye, p), p))
        .filter(|(d2, p)| *d2 < draw_m(p, lamp) * draw_m(p, lamp))
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.key.cmp(&b.1.key)));
    out.into_iter().take(most).map(|(_, p)| p).collect()
}

/// Spawns the pieces within their distance of the primary observer,
/// nearest first, and despawns those past it and its margin.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn stream_roadside(
    mut commands: Commands,
    near: Option<Res<NearRoadside>>,
    roadside: Option<Res<Roadside>>,
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
    let lamp = lamp_file(roadside.as_deref());
    let keep: std::collections::BTreeSet<RoadsidePieceKey> = near
        .pieces
        .iter()
        .filter(|p| {
            let reach = draw_m(p, lamp) + DROP_MARGIN_M;
            distance2(eye, p) < reach * reach
        })
        .map(|p| p.key)
        .collect();
    let gone: Vec<RoadsidePieceKey> = drawn
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
    for piece in wanted(&near, eye, lamp, &drawn.roots, SPAWNS_PER_FRAME) {
        let file = piece.piece.file.as_str();
        let exists = if let Some(&exists) = drawn.files.get(file) {
            exists
        } else {
            let exists = root.exists(file);
            if !exists {
                warn!(
                    "no baked roadside piece at generated/{}; it is left out until `just content`",
                    file
                );
            }
            drawn.files.insert(file.to_owned(), exists);
            exists
        };
        if !exists {
            continue;
        }
        let (at, rotation) = piece_pose(&piece.piece);
        let materials = library.get_or_insert_with(|| server.load(LIBRARY)).clone();
        let entity = commands
            .spawn((
                Name::new(file.to_owned()),
                crate::pieces::Streamed,
                RoadsidePiece {
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
        drawn.roots.insert(piece.key, entity);
    }
}

/// Where a lamp's lantern hangs, in the world.
fn lantern_at(piece: &NearPiece) -> WorldPos {
    let (sin, cos) = piece.piece.yaw.sin_cos();
    let [across, up, out] = LANTERN_AT;
    // The piece's `+z` turned by its yaw, as `Quat::from_rotation_y` turns.
    WorldPos::new(
        piece.piece.at[0] + across * cos + out * sin,
        piece.piece.at[1] + up,
        piece.piece.at[2] - across * sin + out * cos,
    )
}

/// Puts the pool's point lights under the lanterns nearest the observer,
/// at night; by day they are all off.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn lamp_lights(
    mut commands: Commands,
    mut pool: ResMut<LampLights>,
    near: Option<Res<NearRoadside>>,
    roadside: Option<Res<Roadside>>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    night: Option<Res<strata::atmosphere::Night>>,
    mut lights: Query<(&mut PointLight, &mut WorldPosition, &mut Visibility)>,
) {
    let (Some(near), Some(eye), Some(night)) = (near, observers.primary, night) else {
        return;
    };
    if pool.lights.is_empty() {
        for _ in 0..LIGHTS {
            let entity = commands
                .spawn((
                    Name::new("road lamp light"),
                    PointLight {
                        intensity: 0.0,
                        shadow_maps_enabled: false,
                        color: Color::linear_rgb(
                            LANTERN_COLOR[0],
                            LANTERN_COLOR[1],
                            LANTERN_COLOR[2],
                        ),
                        range: LANTERN_RANGE_M,
                        ..default()
                    },
                    WorldPosition(eye),
                    Transform::from_translation(origin.to_render(eye)),
                    Visibility::Hidden,
                ))
                .id();
            pool.lights.push(entity);
        }
        return;
    }
    let moved = pool.at.is_none_or(|at| {
        let (dx, dz) = (at.x() - eye.x(), at.z() - eye.z());
        dx * dx + dz * dz > 16.0
    });
    if !moved && (pool.darkness - night.darkness).abs() < 0.02 && !near.is_changed() {
        return;
    }
    pool.at = Some(eye);
    pool.darkness = night.darkness;
    let mut nearest: Vec<(f64, WorldPos)> = Vec::new();
    let lamp = lamp_file(roadside.as_deref());
    if night.darkness > 0.05 {
        nearest = near
            .pieces
            .iter()
            .filter(|p| is_lamp(p, lamp))
            .map(|p| (distance2(eye, p), lantern_at(p)))
            .filter(|(d2, _)| *d2 < LIGHT_M * LIGHT_M)
            .collect();
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
        nearest.truncate(LIGHTS);
    }
    for (i, entity) in pool.lights.iter().enumerate() {
        let Ok((mut light, mut position, mut visibility)) = lights.get_mut(*entity) else {
            continue;
        };
        if let Some((_, at)) = nearest.get(i) {
            light.intensity = LANTERN_LUMENS * night.darkness;
            position.0 = *at;
            *visibility = Visibility::Inherited;
        } else if *visibility != Visibility::Hidden {
            light.intensity = 0.0;
            *visibility = Visibility::Hidden;
        }
    }
}

/// Leaving the world takes every drawn piece and light with it.
fn clear(mut commands: Commands, mut drawn: ResMut<Drawn>, mut pool: ResMut<LampLights>) {
    for (_, root) in std::mem::take(&mut drawn.roots) {
        commands.entity(root).despawn();
    }
    for light in std::mem::take(&mut pool.lights) {
        commands.entity(light).despawn();
    }
    pool.at = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata::physics::roadside::COLLIDE_M;
    use strata::sim::roadside::pieces_within;
    use strata::worldgen::crossings::PlacedPiece;

    fn piece(x: f64, i: usize, lamp: bool) -> NearPiece {
        NearPiece {
            key: (9, i),
            piece: PlacedPiece {
                file: if lamp {
                    "crossings/veyra/road-lamp.ashlar"
                } else {
                    "crossings/veyra/road-marker.ashlar"
                }
                .to_owned(),
                at: [x, 10.0, 0.0],
                yaw: 0.4,
            },
        }
    }

    /// What the client draws within the physics' reach is exactly what the
    /// physics collides with, at the same poses; markers are drawn nearer
    /// than lamps, and the nearest come first.
    #[test]
    fn the_client_draws_what_the_server_collides_with() {
        let near = NearRoadside {
            pieces: vec![
                piece(300.0, 0, false),
                piece(100.0, 1, false),
                piece(2_000.0, 2, false),
                piece(2_000.0, 3, true),
                piece(9_000.0, 4, true),
            ],
            around: Vec::new(),
        };
        let eye = WorldPos::new(0.0, 10.0, 0.0);
        let lamp = Some("crossings/veyra/road-lamp.ashlar");
        let drawn: Vec<_> = wanted(&near, eye, lamp, &BTreeMap::new(), 10)
            .into_iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(drawn, [(9, 1), (9, 0), (9, 3)]);
        let collides: Vec<_> = pieces_within(&near, &[eye], COLLIDE_M)
            .map(|p| p.key)
            .collect();
        assert_eq!(collides, [(9, 0), (9, 1)]);
        for key in collides {
            assert!(drawn.contains(&key));
        }
        // A lamp's light hangs under its arm, out along its turned +z.
        let lamp = piece(0.0, 5, true);
        let at = lantern_at(&lamp);
        let reach = (at.x().powi(2) + at.z().powi(2)).sqrt();
        assert!((reach - LANTERN_AT[2]).abs() < 1e-9);
        assert!(at.x() > 0.0 && at.z() > 0.0, "{at:?}");
    }
}
