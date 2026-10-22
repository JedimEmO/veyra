//! What stands along the roads near the observers, the same on a server
//! and on every client (ticket 0005, step 8): marker posts and lamps.
//!
//! The content step bakes the roadside kit ([`RoadsideKit`]) beside the
//! crossings'; the loader reads its layout ([`Roadside`]) and
//! [`refresh_near_roadside`] keeps [`NearRoadside`]: every piece
//! `strata_worldgen::roadside::place_roadside` stands on a road that
//! comes within [`NEAR_ROADSIDE_M`] of an observer, and within that of
//! one. Placement is a pure function of the road, the two kits and the
//! world's ground, so the physics collides what the client draws, each
//! within its own distance ([`pieces_within`]), and nothing about a piece
//! is replicated.
//!
//! As the crossings' list is, it is made on the task pool and only when an
//! observer has moved [`REFRESH_M`]; a mass no one has baked is baked by
//! whoever asks. A road's pieces are placed once and kept while the road is
//! in the list ([`Roadside::placed`]): placing one samples the ground at
//! every station and stands every bridge again, a millisecond or more for
//! a long road, and a refresh mostly asks about the same roads.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use bevy::ecs::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use strata_core::WorldPos;
use strata_registry::AssetSource;
use strata_worldgen::ProceduralWorld;
use strata_worldgen::crossings::{CrossingKit, PlacedPiece};
use strata_worldgen::roadside::place_roadside;
pub use strata_worldgen::roadside::{ROADSIDE_PATH, RoadId, RoadsideKit};
use tracing::{info, warn};

use crate::Observers;
use crate::sites::{REFRESH_M, read_text};

/// How far round each observer [`NearRoadside`] lists pieces, metres: the
/// farthest a consumer keeps one (a client draws a lamp to 4 km, as far as
/// a crossing, and drops it 400 m past that) and a refresh, so a piece
/// still drawn is still listed after the observer moves. The client
/// asserts it against its own distances.
pub const NEAR_ROADSIDE_M: f64 = 4_650.0;

/// The roadside kit and the world it is stood in, published with the world.
#[derive(Resource, Clone)]
pub struct Roadside {
    /// The world whose roads the pieces stand along.
    pub world: Arc<ProceduralWorld>,
    /// The kit as the content step baked it, or `None` when its layout is
    /// missing or unreadable: no road then has anything along it.
    pub kit: Option<Arc<RoadsideKit>>,
    /// The crossings' kit, for the bridges whose ends the lamps mark.
    pub crossings: Option<Arc<CrossingKit>>,
    /// Each road's pieces as [`place_roadside`] stood them, for the roads
    /// the last list held. Placement is pure in the road, the world and the
    /// kits, all fixed for this resource, so a kept list is the one placing
    /// it again would make; a new world or kit is a new resource and starts
    /// empty.
    pub placed: Arc<Mutex<HashMap<RoadId, Arc<[PlacedPiece]>>>>,
}

impl Roadside {
    /// The kit read from `generated` (the content step's output; `None`
    /// reads nothing), with the crossings' kit as the crossings read it. A
    /// missing layout is said once, here.
    #[must_use]
    pub fn read(
        world: Arc<ProceduralWorld>,
        generated: Option<&dyn AssetSource>,
        crossings: Option<Arc<CrossingKit>>,
    ) -> Self {
        let kit = generated.and_then(|files| read_kit(files).map(Arc::new));
        Self {
            world,
            kit,
            crossings,
            placed: Arc::default(),
        }
    }
}

/// The kit's layout, or `None` with a warning when it is missing or
/// unreadable.
fn read_kit(generated: &dyn AssetSource) -> Option<RoadsideKit> {
    let text = read_text(generated, ROADSIDE_PATH)
        .map_err(|error| {
            warn!(
                "no roadside kit at {ROADSIDE_PATH}: {error}; the roads stand bare until `just content`"
            );
        })
        .ok()?;
    let kit: RoadsideKit = ron::from_str(&text)
        .map_err(|error| warn!("roadside kit {ROADSIDE_PATH}: {error}"))
        .ok()?;
    info!(files = kit.files().len(), "roadside kit read");
    Some(kit)
}

/// A roadside piece, as one key the same everywhere: its road's id key and
/// the piece's index along the road.
pub type RoadsidePieceKey = (u64, usize);

/// One piece near an observer.
#[derive(Debug, Clone, PartialEq)]
pub struct NearPiece {
    /// Its key.
    pub key: RoadsidePieceKey,
    /// What stands where.
    pub piece: PlacedPiece,
}

/// The pieces within [`NEAR_ROADSIDE_M`] of the observers, by key, and
/// where the list was last made round.
#[derive(Resource, Debug, Default, Clone)]
pub struct NearRoadside {
    /// The pieces, ordered by key: the same order everywhere.
    pub pieces: Vec<NearPiece>,
    /// The observers' places when it was made, `(x, z)`.
    pub around: Vec<[f64; 2]>,
}

/// Whether `(x, z)` is within `radius_m` of any of `around`, horizontally.
fn near_any(around: &[[f64; 2]], [x, z]: [f64; 2], radius_m: f64) -> bool {
    around.iter().any(|&[ax, az]| {
        let (dx, dz) = (x - ax, z - az);
        dx * dx + dz * dz < radius_m * radius_m
    })
}

/// Every piece standing within `radius_m` of any of `around`, each once,
/// ordered by key. Pure in its inputs: a server and a client with the same
/// world and the same kits get the same list. Empty without a kit.
#[must_use]
pub fn near_roadside(roadside: &Roadside, around: &[[f64; 2]], radius_m: f64) -> Vec<NearPiece> {
    let Some(kit) = roadside.kit.as_deref() else {
        return Vec::new();
    };
    let world = roadside.world.as_ref();
    let mut roads = std::collections::BTreeMap::new();
    for &[x, z] in around {
        let networks = world.hydrology().bake_area_on(
            world,
            (x - radius_m, z - radius_m),
            (x + radius_m, z + radius_m),
            1,
        );
        for network in networks {
            for (r, road) in network.roads().iter().enumerate() {
                let id = RoadId {
                    mass: network.key(),
                    road: u32::try_from(r).expect("fewer than 4 billion roads"),
                };
                if roads.contains_key(&id)
                    || !road
                        .points
                        .iter()
                        .any(|p| near_any(around, [p.x, p.z], radius_m))
                {
                    continue;
                }
                roads.insert(id, (Arc::clone(&network), r));
            }
        }
    }
    // The roads placed before, taken out of the cache under a short lock,
    // the rest placed without it, and the cache left holding this list's.
    let mut kept: HashMap<RoadId, Arc<[PlacedPiece]>> = {
        let cache = roadside
            .placed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        roads
            .keys()
            .filter_map(|id| cache.get(id).map(|placed| (*id, Arc::clone(placed))))
            .collect()
    };
    let mut out = Vec::new();
    for (id, (network, r)) in roads {
        let placed = kept.entry(id).or_insert_with(|| {
            let road = &network.roads()[r];
            place_roadside(id, road, kit, roadside.crossings.as_deref(), world).into()
        });
        for (i, piece) in placed.iter().enumerate() {
            if near_any(around, [piece.at[0], piece.at[2]], radius_m) {
                out.push(NearPiece {
                    key: (id.key(), i),
                    piece: piece.clone(),
                });
            }
        }
    }
    *roadside
        .placed
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = kept;
    out.sort_by_key(|p| p.key);
    out.dedup_by_key(|p| p.key);
    out
}

/// Every piece of `near` within `radius_m` of any of `observers`,
/// horizontally, measured to the piece: the one selection the physics'
/// colliders and the client's drawing make, each with its own radius.
pub fn pieces_within<'a>(
    near: &'a NearRoadside,
    observers: &'a [WorldPos],
    radius_m: f64,
) -> impl Iterator<Item = &'a NearPiece> + 'a {
    near.pieces.iter().filter(move |p| {
        observers.iter().any(|o| {
            let (dx, dz) = (o.x() - p.piece.at[0], o.z() - p.piece.at[2]);
            dx * dx + dz * dz < radius_m * radius_m
        })
    })
}

/// The list being made.
#[derive(Default)]
pub struct Refreshing {
    task: Option<Task<(Vec<[f64; 2]>, Vec<NearPiece>)>>,
}

/// Makes [`NearRoadside`] again, on the task pool, whenever an observer has
/// moved [`REFRESH_M`] from every place it was last made round (or the
/// observers came or went), and publishes the list when it is done.
pub fn refresh_near_roadside(
    roadside: Option<Res<Roadside>>,
    observers: Res<Observers>,
    mut near: ResMut<NearRoadside>,
    mut refreshing: Local<Refreshing>,
) {
    if let Some(task) = refreshing.task.as_mut() {
        let Some((around, list)) = block_on(poll_once(task)) else {
            return;
        };
        refreshing.task = None;
        near.around = around;
        near.pieces = list;
    }
    let Some(roadside) = roadside else {
        return;
    };
    if roadside.kit.is_none() {
        return;
    }
    let here: Vec<[f64; 2]> = observers.positions.iter().map(|p| [p.x(), p.z()]).collect();
    let stale = here.len() != near.around.len()
        || here
            .iter()
            .any(|&p| !near_any(&near.around, p, REFRESH_M + f64::EPSILON));
    if !stale {
        return;
    }
    if here.is_empty() {
        near.around.clear();
        near.pieces.clear();
        return;
    }
    let roadside = roadside.clone();
    refreshing.task = Some(
        AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            let list = near_roadside(&roadside, &here, NEAR_ROADSIDE_M);
            (here, list)
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    fn kit() -> RoadsideKit {
        RoadsideKit {
            marker: Some("crossings/veyra/road-marker.ashlar".to_owned()),
            lamp: Some("crossings/veyra/road-lamp.ashlar".to_owned()),
        }
    }

    /// Two worlds built apart from the same seed, each with its own river
    /// bake on its own thread count, as a server's and a client's are, list
    /// the same pieces round the showcase bridge, bit for bit: markers on
    /// its service road and lamps at its ends.
    ///
    /// Two river bakes of the city's mass take minutes in a debug build;
    /// by hand, in release:
    /// `cargo test --release -p strata-app roadside:: -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "two river bakes: by hand, in release"]
    fn a_server_and_a_client_stand_the_same_roadside() {
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        let make = || {
            ProceduralWorld::with_params(strata_core::Seed(42), &registries, crate::world_params())
        };
        let crossings = Some(Arc::new(crate::crossings::tests::kit()));
        let server = Roadside {
            world: Arc::new(make()),
            kit: Some(Arc::new(kit())),
            crossings: crossings.clone(),
            placed: Arc::default(),
        };
        let client = Roadside {
            world: Arc::new(make().with_own_water()),
            kit: Some(Arc::new(kit())),
            crossings,
            placed: Arc::default(),
        };
        let around = [[7_404.0, -4_940.0]];
        server
            .world
            .prepare_water_on(around[0][0], around[0][1], 2_500.0, 4);
        client
            .world
            .prepare_water_on(around[0][0], around[0][1], 2_500.0, 3);
        let a = near_roadside(&server, &around, 2_000.0);
        let b = near_roadside(&client, &around, 2_000.0);
        assert_eq!(a, b);
        // Asked again from the cache, and from a step along, the list is
        // the one placing every road afresh makes.
        assert_eq!(a, near_roadside(&server, &around, 2_000.0));
        let along = [[7_604.0, -4_940.0]];
        let cached = near_roadside(&server, &along, 2_000.0);
        assert!(!server.placed.lock().expect("unpoisoned").is_empty());
        assert_eq!(cached, near_roadside(&client.uncached(), &along, 2_000.0));
        let markers = a
            .iter()
            .filter(|p| p.piece.file.ends_with("road-marker.ashlar"))
            .count();
        let lamps: Vec<_> = a
            .iter()
            .filter(|p| p.piece.file.ends_with("road-lamp.ashlar"))
            .collect();
        assert!(markers >= 10, "{markers} markers within 2 km");
        // The showcase bridge's four, within 40 m of it.
        let at_bridge = lamps
            .iter()
            .filter(|p| {
                let (dx, dz) = (p.piece.at[0] - 7_404.0, p.piece.at[2] + 4_940.0);
                dx.hypot(dz) < 60.0
            })
            .count();
        assert_eq!(at_bridge, 4, "{lamps:?}");
        for p in &a {
            assert!(p.piece.at.iter().all(|v| v.is_finite()) && p.piece.yaw.is_finite());
        }
        let observers = [WorldPos::new(around[0][0], 0.0, around[0][1])];
        let list = NearRoadside {
            pieces: a,
            around: around.to_vec(),
        };
        assert!(pieces_within(&list, &observers, 600.0).count() >= 4);
    }

    impl Roadside {
        /// The same world and kits with nothing placed yet.
        fn uncached(&self) -> Self {
            Self {
                placed: Arc::default(),
                ..self.clone()
            }
        }
    }

    /// Without a kit nothing stands by any road.
    #[test]
    fn a_missing_kit_stands_nothing() {
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        let world = Arc::new(ProceduralWorld::with_params(
            strata_core::Seed(42),
            &registries,
            crate::world_params(),
        ));
        let missing = strata_registry::files::Files::dir(
            std::env::temp_dir().join("strata-no-generated-here"),
        );
        let roadside = Roadside::read(world, Some(&missing), None);
        assert!(roadside.kit.is_none());
        assert!(near_roadside(&roadside, &[[0.0, 0.0]], 4_000.0).is_empty());
    }

    /// The selection is each piece's own distance, not its road's.
    #[test]
    fn pieces_are_selected_by_their_own_distance() {
        let piece = |x: f64, i: usize| NearPiece {
            key: (7, i),
            piece: PlacedPiece {
                file: "m".to_owned(),
                at: [x, 0.0, 0.0],
                yaw: 0.0,
            },
        };
        let near = NearRoadside {
            pieces: vec![piece(100.0, 0), piece(700.0, 1), piece(5_000.0, 2)],
            around: Vec::new(),
        };
        let eye = [WorldPos::new(0.0, 0.0, 0.0)];
        let keys: Vec<_> = pieces_within(&near, &eye, 800.0).map(|p| p.key).collect();
        assert_eq!(keys, [(7, 0), (7, 1)]);
    }
}
