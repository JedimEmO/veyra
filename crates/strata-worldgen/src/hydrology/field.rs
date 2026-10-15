//! The coarse field on the lattice, a tile at a time.
//!
//! A tile is computed whole on a capped pool the first time anything asks
//! about one of its nodes and kept: the masses' flood fills, the bakes and
//! the queries read it, and the queries also find there which mass each
//! node is on. A tile's rain and province rules are read only by the bake of
//! a mass on it, so once every land node of the tile is baked they are
//! dropped; its heights and labels stay, since the flood fills and the
//! queries read them for any mass next door.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

use super::{FIRST, LATTICE_M, RiverParams, TILE_NODES, UNKNOWN};
use crate::ProceduralWorld;

/// One tile of the coarse field: per node, row-major with `x` fastest.
pub(crate) struct Tile {
    /// Metres, at the field's own precision once stored.
    pub height: Box<[f32]>,
    /// Which mass the node is on, as the cache labels them.
    pub label: Box<[AtomicU32]>,
    /// What only a bake reads, `None` once the tile's land is all baked.
    rest: RwLock<Option<Rest>>,
}

/// The part of a tile only a bake reads.
struct Rest {
    /// `0..=1`.
    rain: Box<[f32]>,
    /// The province's `keep_pits_m`, or the world default.
    keep: Box<[f32]>,
    /// The province's `sink`.
    sink: Box<[f32]>,
    /// The province's `along_grain`.
    grain: Box<[f32]>,
}

/// One node's field, as a domain carries it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Node {
    pub height: f64,
    pub rain: f64,
    pub keep: f64,
    pub sink: f64,
    /// How much the routing prefers the fold axis here.
    pub grain: f64,
}

impl Tile {
    /// The tile whose first node is `key * TILE_NODES`.
    pub fn compute(
        world: &ProceduralWorld,
        hydrology: &super::Hydrology,
        key: [i32; 2],
        threads: usize,
    ) -> Self {
        let all = nodes(world, hydrology, key, threads);
        Self {
            height: all.iter().map(|n| super::single(n.height)).collect(),
            label: (0..all.len()).map(|_| AtomicU32::new(UNKNOWN)).collect(),
            rest: RwLock::new(Some(Rest::of(&all))),
        }
    }

    /// Node `k`'s field, as the tile stored it, or `None` if its rain was
    /// dropped (every land node of the tile is baked).
    pub fn node(&self, k: usize) -> Option<Node> {
        let rest = self.rest.read().unwrap_or_else(PoisonError::into_inner);
        let rest = rest.as_ref()?;
        Some(Node {
            height: f64::from(self.height[k]),
            rain: f64::from(rest.rain[k]),
            keep: f64::from(rest.keep[k]),
            sink: f64::from(rest.sink[k]),
            grain: f64::from(rest.grain[k]),
        })
    }

    /// Computes the dropped part again: a bake that needs a node whose
    /// tile was dropped (a mass's count can reach a tile that a
    /// neighbouring mass's bake emptied of unbaked land).
    pub fn restore(
        &self,
        world: &ProceduralWorld,
        hydrology: &super::Hydrology,
        key: [i32; 2],
        threads: usize,
    ) {
        let mut rest = self.rest.write().unwrap_or_else(PoisonError::into_inner);
        if rest.is_none() {
            *rest = Some(Rest::of(&nodes(world, hydrology, key, threads)));
        }
    }

    /// Drops the rain and the rules if every land node is baked; whether
    /// it did.
    pub fn drop_if_baked(&self) -> bool {
        let baked = self
            .height
            .iter()
            .zip(&self.label)
            .all(|(&h, label)| h < 0.0 || label.load(Ordering::Acquire) >= FIRST);
        if baked {
            *self.rest.write().unwrap_or_else(PoisonError::into_inner) = None;
        }
        baked
    }

    /// Whether the rain and the rules are held.
    pub fn holds_rest(&self) -> bool {
        self.rest
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
}

impl Rest {
    fn of(all: &[Node]) -> Self {
        Self {
            rain: all.iter().map(|n| super::single(n.rain)).collect(),
            keep: all.iter().map(|n| super::single(n.keep)).collect(),
            sink: all.iter().map(|n| super::single(n.sink)).collect(),
            grain: all.iter().map(|n| super::single(n.grain)).collect(),
        }
    }
}

/// Every node of tile `key`, row-major.
fn nodes(
    world: &ProceduralWorld,
    hydrology: &super::Hydrology,
    key: [i32; 2],
    threads: usize,
) -> Vec<Node> {
    let side = TILE_NODES as usize;
    let rows = parallel_rows(side, threads, |row| {
        let j = key[1] * TILE_NODES + i32::try_from(row).expect("a tile row");
        (0..TILE_NODES)
            .map(|col| node(world, hydrology, [key[0] * TILE_NODES + col, j]))
            .collect::<Vec<_>>()
    });
    rows.into_iter().flatten().collect()
}

/// The field at lattice node `n`, rounded to the tile's precision so a
/// window bake and a tiled one agree to the bit: the coarse ground with
/// the valleys the routing assumes on land
/// ([`RiverParams::valley_range_share`]).
pub(crate) fn node(world: &ProceduralWorld, hydrology: &super::Hydrology, n: [i32; 2]) -> Node {
    let (x, z) = (super::metres(n[0]), super::metres(n[1]));
    let mut ground = world.coarse_ground(x, z);
    if ground.height >= 0.0 {
        let p = &hydrology.params;
        let valleys =
            hydrology.valleys.get(x, z) * (p.valley_m + p.valley_range_share * ground.range_m);
        // Valleys do not reach under the sea: the coast is the field's.
        ground.height = (ground.height + valleys).max(0.0);
    }
    let (keep, sink, grain) =
        ground
            .province
            .unwrap_or((RiverParams::DEFAULT.keep_pits_m, 0.0, 0.0));
    let round = |v: f64| f64::from(super::single(v));
    Node {
        height: round(ground.height),
        rain: round(ground.rain),
        keep: round(keep),
        sink: round(sink),
        grain: round(grain),
    }
}

/// Which tile a node is in and its index there.
pub(crate) fn locate(n: [i32; 2]) -> ([i32; 2], usize) {
    let key = [n[0].div_euclid(TILE_NODES), n[1].div_euclid(TILE_NODES)];
    let (i, j) = (n[0].rem_euclid(TILE_NODES), n[1].rem_euclid(TILE_NODES));
    let k = usize::try_from(j * TILE_NODES + i).expect("inside a tile");
    (key, k)
}

/// `f` over `0..rows` on up to `threads` threads (at most
/// [`MAX_THREADS`](super::MAX_THREADS)), the results in order. One call at a
/// time per process, as the measures' pool is: two pools at once are the
/// load the development machine's CPU cannot take.
pub(crate) fn parallel_rows<T: Send>(
    rows: usize,
    threads: usize,
    f: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let threads = threads.min(rows).clamp(1, super::MAX_THREADS);
    if threads == 1 {
        return (0..rows).map(f).collect();
    }
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    strata_core::platform::par::map_index(rows, threads, f)
}

/// The ground one node drains, square metres: its rain times this is its
/// runoff.
pub(crate) const NODE_AREA_M2: f64 = LATTICE_M * LATTICE_M;
