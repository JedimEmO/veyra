//! What stands at the roads' crossings of rivers near the observers, the
//! same on a server and on every client (ticket 0005, step 7).
//!
//! The world records every crossing in its road bake and the content step
//! bakes the crossings' kit ([`CrossingKit`]): bridges in span classes,
//! piers in a few heights, culvert headwalls, ford posts and stepping
//! stones. The loader reads the kit's layout ([`Crossings`]) and
//! [`refresh_near_crossings`] keeps [`NearCrossings`]: every crossing
//! within [`NEAR_CROSSINGS_M`] of an observer with the pieces
//! `strata_worldgen::crossings::place_crossing` stands there, a pure
//! function of the record, the layout and the world's ground. The physics
//! builds colliders from that list and the client draws from it, each
//! within its own distance ([`pieces_within`]), so what a client draws is
//! what the server collides with and nothing about a crossing is
//! replicated.
//!
//! As the sites' list is, it is made on the task pool and only when an
//! observer has moved [`REFRESH_M`]: a mass no
//! one has baked is baked by whoever asks.

use std::sync::Arc;

use bevy::ecs::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use strata_core::WorldPos;
use strata_registry::AssetSource;

use crate::sites::read_text;
use strata_worldgen::ProceduralWorld;
use strata_worldgen::crossings::place_crossing;
pub use strata_worldgen::crossings::{
    CrossingId, CrossingKit, KIT_PATH, PlacedPiece, ProceduralCrossing, Unplaced,
};
use tracing::{info, warn};

use crate::Observers;
use crate::sites::REFRESH_M;

/// How far round each observer [`NearCrossings`] lists crossings, metres:
/// as far as the sites' list, past the farthest any consumer stands a
/// piece by more than a refresh.
pub const NEAR_CROSSINGS_M: f64 = 5_000.0;

/// The world's crossings and the kit stood at them, published with the
/// world.
#[derive(Resource, Clone)]
pub struct Crossings {
    /// The world the crossings are read from, and whose ground places them.
    pub world: Arc<ProceduralWorld>,
    /// The kit as the content step baked it, or `None` when its layout is
    /// missing or unreadable: every crossing then stands nothing.
    pub kit: Option<Arc<CrossingKit>>,
}

impl Crossings {
    /// The kit read from `generated` (the content step's output; `None`
    /// reads nothing). A missing layout is said once, here.
    #[must_use]
    pub fn read(world: Arc<ProceduralWorld>, generated: Option<&dyn AssetSource>) -> Self {
        let kit = generated.and_then(|files| read_kit(files).map(Arc::new));
        Self { world, kit }
    }
}

/// The kit's layout, or `None` with a warning when it is missing or
/// unreadable.
fn read_kit(generated: &dyn AssetSource) -> Option<CrossingKit> {
    let text = read_text(generated, KIT_PATH)
        .map_err(|error| {
            warn!(
                "no crossings kit at {KIT_PATH}: {error}; bridges, culverts and fords stand empty until `just content`"
            );
        })
        .ok()?;
    let kit: CrossingKit = ron::from_str(&text)
        .map_err(|error| warn!("crossings kit {KIT_PATH}: {error}"))
        .ok()?;
    info!(
        bridges = kit.bridges.len(),
        files = kit.files().len(),
        "crossings kit read"
    );
    Some(kit)
}

/// One crossing near an observer and what stands at it.
#[derive(Debug, Clone, PartialEq)]
pub struct NearCrossing {
    /// The crossing.
    pub crossing: ProceduralCrossing,
    /// Its pieces, or why it has none.
    pub placed: Result<Vec<PlacedPiece>, Unplaced>,
}

impl NearCrossing {
    /// Its pieces, none if it has none.
    #[must_use]
    pub fn pieces(&self) -> &[PlacedPiece] {
        self.placed.as_deref().unwrap_or(&[])
    }
}

/// The crossings within [`NEAR_CROSSINGS_M`] of the observers, by id, and
/// where the list was last made round.
#[derive(Resource, Debug, Default, Clone)]
pub struct NearCrossings {
    /// The crossings, ordered by id: the same order everywhere.
    pub crossings: Vec<NearCrossing>,
    /// The observers' places when it was made, `(x, z)`.
    pub around: Vec<[f64; 2]>,
}

/// Every crossing within `radius_m` of any of `around`, each once, ordered
/// by id, with the kit stood at it. Pure in its inputs: a server and a
/// client with the same world and the same kit get the same list. Empty
/// without a kit.
#[must_use]
pub fn near_crossings(
    crossings: &Crossings,
    around: &[[f64; 2]],
    radius_m: f64,
) -> Vec<NearCrossing> {
    let Some(kit) = crossings.kit.as_deref() else {
        return Vec::new();
    };
    let mut found: std::collections::BTreeMap<CrossingId, ProceduralCrossing> =
        std::collections::BTreeMap::new();
    for &[x, z] in around {
        for c in crossings.world.crossings_near(x, z, radius_m) {
            found.entry(c.id).or_insert(c);
        }
    }
    found
        .into_values()
        .map(|crossing| NearCrossing {
            placed: place_crossing(&crossing, kit, crossings.world.as_ref()),
            crossing,
        })
        .collect()
}

/// A piece of a crossing, as one key the same everywhere: the crossing's
/// id key and the piece's index there.
pub type CrossingPieceKey = (u64, usize);

/// Every piece of `near` whose crossing is within `radius_m` of any of
/// `observers`, horizontally, with its key: the one selection the physics'
/// colliders and the client's drawing make, each with its own radius.
pub fn pieces_within<'a>(
    near: &'a NearCrossings,
    observers: &'a [WorldPos],
    radius_m: f64,
) -> impl Iterator<Item = (CrossingPieceKey, &'a NearCrossing, &'a PlacedPiece)> + 'a {
    near.crossings
        .iter()
        .filter(move |near| {
            let [x, z] = near.crossing.crossing.at;
            observers.iter().any(|o| {
                let (dx, dz) = (o.x() - x, o.z() - z);
                dx * dx + dz * dz < radius_m * radius_m
            })
        })
        .flat_map(|near| {
            near.pieces()
                .iter()
                .enumerate()
                .map(move |(i, piece)| ((near.crossing.id.key(), i), near, piece))
        })
}

/// The list being made, and the crossings already said to stand nothing.
#[derive(Default)]
pub struct Refreshing {
    task: Option<Task<(Vec<[f64; 2]>, Vec<NearCrossing>)>>,
    said: std::collections::HashSet<CrossingId>,
}

/// Says once per crossing, as a warning, why one near the observers stands
/// nothing where something was meant to: a graded road whose bridge is
/// missing ends at the shelf's edge over the river, and that should be in
/// the log and not found by falling. A trail's creek in a gorge
/// ([`Unplaced::NoWater`]) is not one: nothing is meant to stand there.
fn say_unplaced(list: &[NearCrossing], said: &mut std::collections::HashSet<CrossingId>) {
    for near in list {
        let Err(why) = near.placed else {
            continue;
        };
        if why == Unplaced::NoWater || !said.insert(near.crossing.id) {
            continue;
        }
        let x = &near.crossing.crossing;
        warn!(
            "{} ({:?} {:?} at {:.0}, {:.0}, gap {:.1} m, deck {:.1} m) stands nothing: {why:?}",
            near.crossing.id,
            near.crossing.class,
            x.kind,
            x.at[0],
            x.at[1],
            x.abutments_m[1] - x.abutments_m[0],
            x.deck_m,
        );
    }
}

/// Makes [`NearCrossings`] again, on the task pool, whenever an observer
/// has moved [`REFRESH_M`] from every place it was last made round (or the
/// observers came or went), and publishes the list when it is done.
pub fn refresh_near_crossings(
    crossings: Option<Res<Crossings>>,
    observers: Res<Observers>,
    mut near: ResMut<NearCrossings>,
    mut refreshing: Local<Refreshing>,
) {
    if let Some(task) = refreshing.task.as_mut() {
        let Some((around, list)) = block_on(poll_once(task)) else {
            return;
        };
        refreshing.task = None;
        say_unplaced(&list, &mut refreshing.said);
        near.around = around;
        near.crossings = list;
    }
    let Some(crossings) = crossings else {
        return;
    };
    if crossings.kit.is_none() {
        return;
    }
    let here: Vec<[f64; 2]> = observers.positions.iter().map(|p| [p.x(), p.z()]).collect();
    let stale = here.len() != near.around.len()
        || here.iter().any(|&[x, z]| {
            near.around.iter().all(|&[ax, az]| {
                let (dx, dz) = (x - ax, z - az);
                dx * dx + dz * dz > REFRESH_M * REFRESH_M
            })
        });
    if !stale {
        return;
    }
    if here.is_empty() {
        near.around.clear();
        near.crossings.clear();
        return;
    }
    let crossings = crossings.clone();
    refreshing.task = Some(
        AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            let list = near_crossings(&crossings, &here, NEAR_CROSSINGS_M);
            (here, list)
        }),
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use strata_worldgen::crossings::{BridgeClass, DeckEnd, HeadwallFile, PierFile};
    use strata_worldgen::roads::RoadClass;

    fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    /// A kit like the content step's: bridge classes every 4 m to 64 m for
    /// both graded classes, 4 m to 32 m for trails, piers, headwalls in
    /// three box heights, a post and five stones.
    pub(crate) fn kit() -> CrossingKit {
        let mut bridges = Vec::new();
        for (road, most) in [
            (RoadClass::Service, 64),
            (RoadClass::Track, 64),
            (RoadClass::Trail, 32),
        ] {
            for k in 1..=most / 4 {
                let clear_m = f64::from(k * 4);
                let piers = if road == RoadClass::Trail || clear_m <= 24.0 {
                    0
                } else {
                    (k * 4_u32).div_ceil(22).max(2) - 1
                };
                bridges.push(BridgeClass {
                    road,
                    file: format!("crossings/bridge-{road:?}-{k}.ashlar"),
                    clear_m,
                    gaps_m: [
                        if k == 1 { clear_m - 1.0 } else { clear_m + 0.5 },
                        clear_m + 4.5,
                    ],
                    length_m: clear_m + 9.5,
                    underside_m: if piers > 0 { 1.27 } else { 1.0 },
                    piers,
                    girder_most_m: 27.5,
                });
            }
        }
        let mut piers = Vec::new();
        for road in [RoadClass::Service, RoadClass::Track] {
            for height_m in [6.0, 12.0, 24.0, 48.0] {
                piers.push(PierFile {
                    road,
                    file: format!("crossings/pier-{road:?}-{height_m}.ashlar"),
                    height_m,
                    half_along_m: 0.9 + height_m / 60.0,
                });
            }
        }
        CrossingKit {
            bridges,
            deck_end: DeckEnd {
                proud_m: 0.02,
                ramp_m: 1.2,
                drop_m: 0.12,
            },
            piers,
            headwalls: [0.45, 0.65, 0.85]
                .map(|box_m| HeadwallFile {
                    file: format!("crossings/headwall-{box_m}.ashlar"),
                    box_m,
                    top_m: box_m + 0.4,
                })
                .to_vec(),
            ford_post: Some("crossings/post.ashlar".to_owned()),
            stones: (0..5)
                .map(|k| format!("crossings/stone-{k}.ashlar"))
                .collect(),
            stone_proud_m: 0.12,
        }
    }

    /// Two worlds built apart from the same seed and registries, each with
    /// its own river bake on its own thread count, as a server's and a
    /// client's in two processes are, list the same crossings round Port
    /// Serein's first bridge and stand the same pieces at them, bit for bit.
    ///
    /// Two river bakes of the city's mass take two minutes in a debug
    /// build; by hand, in release:
    /// `cargo test --release -p strata-app crossings:: -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "two river bakes: by hand, in release"]
    fn a_server_and_a_client_stand_the_same_crossings() {
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        let make = || {
            ProceduralWorld::with_params(strata_core::Seed(42), &registries, crate::world_params())
        };
        let kit = Arc::new(kit());
        let server = Crossings {
            world: Arc::new(make()),
            kit: Some(Arc::clone(&kit)),
        };
        let client = Crossings {
            world: Arc::new(make().with_own_water()),
            kit: Some(kit),
        };
        assert!(!server.world.shares_water_with(&client.world));
        // Round the backbone's first bridge, the showcase.
        let around = [[7_579.0, -5_029.0]];
        server
            .world
            .prepare_water_on(around[0][0], around[0][1], 5_500.0, 4);
        client
            .world
            .prepare_water_on(around[0][0], around[0][1], 5_500.0, 3);
        let a = near_crossings(&server, &around, 5_000.0);
        let b = near_crossings(&client, &around, 5_000.0);
        assert_eq!(a, b);
        let placed: Vec<_> = a.iter().filter(|c| c.placed.is_ok()).collect();
        assert!(
            a.iter().any(|c| c.crossing.crossing.kind
                == strata_worldgen::roads::CrossingKind::Bridge
                && !c.pieces().is_empty()),
            "no bridge stood within 5 km: {a:?}"
        );
        assert!(placed.len() >= 5, "{} of {} placed", placed.len(), a.len());
        assert!(
            a.iter().any(|c| c.crossing.crossing.kind
                == strata_worldgen::roads::CrossingKind::Culvert
                && !c.pieces().is_empty()),
            "no culvert stood"
        );
        for near in &a {
            for piece in near.pieces() {
                assert!(piece.at.iter().all(|v| v.is_finite()) && piece.yaw.is_finite());
            }
        }
        let observers = [WorldPos::new(around[0][0], 0.0, around[0][1])];
        let list = NearCrossings {
            crossings: a,
            around: around.to_vec(),
        };
        let keys: Vec<_> = pieces_within(&list, &observers, 4_000.0)
            .map(|(key, _, p)| (key, p.file.clone(), p.at.map(f64::to_bits)))
            .collect();
        assert!(!keys.is_empty());
    }

    /// Without a kit nothing stands at any crossing, and the list is empty.
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
        let crossings = Crossings::read(world, Some(&missing));
        assert!(crossings.kit.is_none());
        assert!(near_crossings(&crossings, &[[0.0, 0.0]], 4_000.0).is_empty());
    }
}
