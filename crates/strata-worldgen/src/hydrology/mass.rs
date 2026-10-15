//! Which nodes a bake routes over: a land mass found by flood fill, one
//! cell's part of a mass over the cap, or a window.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::field::{self, Node, Tile};
use super::{FIRST, Hydrology, LARGE, NodeMap, UNKNOWN};
use crate::ProceduralWorld;

/// The eight neighbours, sides first.
pub(crate) const NEIGHBOURS: [[i32; 2]; 8] = [
    [-1, 0],
    [1, 0],
    [0, -1],
    [0, 1],
    [-1, -1],
    [1, -1],
    [-1, 1],
    [1, 1],
];

/// How a node leaves the domain without a receiver in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutletKind {
    /// Into the sea.
    Sea,
    /// Over a window's or a cut mass's edge, treated as a coast.
    Edge,
}

/// The nodes a bake routes over and their field.
pub(crate) struct Domain {
    /// In order, `(i, j)` lexicographic: the first is the mass's name.
    pub nodes: Vec<[i32; 2]>,
    /// Each node's field.
    pub field: Vec<Node>,
    /// Where each node may leave the domain directly: the node outside it
    /// (the lowest such neighbour, ties to the lower coordinate), how, and
    /// that node's coarse height, which says where the coast lies between.
    pub outlet: Vec<Option<([i32; 2], OutletKind, f64)>>,
    /// Whether this is one cell's part of a mass over the cap.
    pub capped: bool,
}

/// A set of lattice nodes, a bit a node in tile-sized blocks.
#[derive(Default)]
struct NodeSet {
    blocks: NodeMap<Box<[u64]>>,
    len: usize,
}

impl NodeSet {
    /// Adds `n`; whether it was new.
    fn insert(&mut self, n: [i32; 2]) -> bool {
        let (key, k) = field::locate(n);
        let words = (super::TILE_NODES * super::TILE_NODES) as usize / 64;
        let block = self
            .blocks
            .entry(key)
            .or_insert_with(|| vec![0; words].into_boxed_slice());
        let (word, bit) = (k / 64, 1_u64 << (k % 64));
        let new = block[word] & bit == 0;
        block[word] |= bit;
        self.len += usize::from(new);
        new
    }

    fn contains(&self, n: [i32; 2]) -> bool {
        let (key, k) = field::locate(n);
        self.blocks
            .get(&key)
            .is_some_and(|block| block[k / 64] & (1_u64 << (k % 64)) != 0)
    }
}

/// The tiles a fill reads, held locally so the fill takes no lock a node,
/// and how many it had computed.
struct Tiles<'a> {
    hydrology: &'a Hydrology,
    world: &'a ProceduralWorld,
    held: NodeMap<Arc<Tile>>,
    added: usize,
    threads: usize,
}

impl Tiles<'_> {
    fn get(&mut self, n: [i32; 2]) -> (&Tile, usize) {
        let (key, k) = field::locate(n);
        if !self.held.contains_key(&key) {
            self.added += usize::from(!self.hydrology.has_tile(key));
            let tile = self.hydrology.tile(self.world, key, self.threads);
            self.held.insert(key, tile);
        }
        (&self.held[&key], k)
    }

    /// Node `n`'s whole field, its tile's rain computed again if a bake
    /// next door had dropped it.
    fn node(&mut self, n: [i32; 2]) -> Node {
        let (key, k) = field::locate(n);
        let (hydrology, world, threads) = (self.hydrology, self.world, self.threads);
        let (tile, _) = self.get(n);
        if let Some(node) = tile.node(k) {
            return node;
        }
        tile.restore(world, hydrology, key, threads);
        tile.node(k).expect("just restored")
    }

    fn is_land(&mut self, n: [i32; 2]) -> bool {
        let (tile, k) = self.get(n);
        tile.height[k] >= 0.0
    }
}

/// The domain of the mass land node `start` is on: the whole mass, or if it
/// is over the cap ([`MAX_MASS_NODES`](super::MAX_MASS_NODES)) the part of
/// it inside `start`'s cell of [`CAP_CELL_NODES`](super::CAP_CELL_NODES).
/// `known_large` skips the count when an earlier fill marked the node.
/// `cap` is `(nodes, cell side)`: the cache's own, or the version canary's
/// small one. Tiles are computed on `threads`. Returns the domain and how
/// many tiles it computed.
#[allow(
    clippy::too_many_lines,
    reason = "the count, the cut and the outlets, in order"
)]
pub(crate) fn find(
    hydrology: &Hydrology,
    world: &ProceduralWorld,
    start: [i32; 2],
    known_large: bool,
    cap: (usize, i32),
    threads: usize,
) -> (Domain, usize) {
    let mut tiles = Tiles {
        hydrology,
        world,
        held: NodeMap::default(),
        added: 0,
        threads,
    };
    let capped_label = |label: u32| {
        label == LARGE
            || (label >= FIRST
                && hydrology
                    .networks
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get((label - FIRST) as usize)
                    .is_some_and(|network| network.capped))
    };
    let mut large = known_large;
    let mut members = NodeSet::default();
    if !large {
        let mut stack = vec![start];
        members.insert(start);
        'fill: while let Some(n) = stack.pop() {
            for d in NEIGHBOURS {
                let m = [n[0] + d[0], n[1] + d[1]];
                if !tiles.is_land(m) || members.contains(m) {
                    continue;
                }
                let (tile, k) = tiles.get(m);
                if capped_label(tile.label[k].load(Ordering::Acquire)) {
                    large = true;
                    break 'fill;
                }
                members.insert(m);
                if members.len > cap.0 {
                    large = true;
                    break 'fill;
                }
                stack.push(m);
            }
        }
        if large {
            // Whoever asks next about these nodes skips the count.
            for (&key, block) in &members.blocks {
                let tile = tiles.hydrology.tile(world, key, threads);
                for (word, &bits) in block.iter().enumerate() {
                    for bit in 0..64 {
                        if bits & (1 << bit) != 0 {
                            let _ = tile.label[word * 64 + bit].compare_exchange(
                                UNKNOWN,
                                LARGE,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            );
                        }
                    }
                }
            }
        }
    }
    if large {
        let side = cap.1.max(1);
        let cell = [start[0].div_euclid(side), start[1].div_euclid(side)];
        let in_cell =
            |n: [i32; 2]| n[0].div_euclid(side) == cell[0] && n[1].div_euclid(side) == cell[1];
        members = NodeSet::default();
        members.insert(start);
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            for d in NEIGHBOURS {
                let m = [n[0] + d[0], n[1] + d[1]];
                if in_cell(m) && tiles.is_land(m) && members.insert(m) {
                    stack.push(m);
                }
            }
        }
    }
    let mut nodes = Vec::with_capacity(members.len);
    for (&key, block) in &members.blocks {
        for (word, &bits) in block.iter().enumerate() {
            for bit in 0..64 {
                if bits & (1 << bit) != 0 {
                    let k = i32::try_from(word * 64 + bit).expect("inside a tile");
                    nodes.push([
                        key[0] * super::TILE_NODES + k % super::TILE_NODES,
                        key[1] * super::TILE_NODES + k / super::TILE_NODES,
                    ]);
                }
            }
        }
    }
    nodes.sort_unstable();
    let field: Vec<Node> = nodes.iter().map(|&n| tiles.node(n)).collect();
    let mut outlet = Vec::with_capacity(nodes.len());
    let mut edges = Vec::with_capacity(nodes.len());
    for &n in &nodes {
        let mut sea: Option<(f64, [i32; 2])> = None;
        let mut edge: Option<(f64, [i32; 2])> = None;
        for d in NEIGHBOURS {
            let m = [n[0] + d[0], n[1] + d[1]];
            let (tile, k) = tiles.get(m);
            let h = f64::from(tile.height[k]);
            let slot = if h < 0.0 {
                &mut sea
            } else if members.contains(m) {
                continue;
            } else {
                &mut edge
            };
            if slot.is_none_or(|(held, at)| (h, m) < (held, at)) {
                *slot = Some((h, m));
            }
        }
        outlet.push(sea.map(|(h, m)| (m, OutletKind::Sea, h)));
        edges.push(edge.map(|(h, m)| (m, OutletKind::Edge, h)));
    }
    // A part of a cut mass drains to its own coast across a forced divide;
    // only one with no coast at all drains out over its cut edge.
    if outlet.iter().all(Option::is_none) {
        outlet = edges;
    }
    let added = tiles.added;
    (
        Domain {
            nodes,
            field,
            outlet,
            capped: large,
        },
        added,
    )
}

/// The domain of the land nodes in the window from `from` to `to`
/// inclusive, the window's edge a coast. The field is computed for the
/// window and its ring alone, not for whole tiles, so a small window is
/// cheap.
pub(crate) fn window(
    hydrology: &Hydrology,
    world: &ProceduralWorld,
    from: [i32; 2],
    to: [i32; 2],
    threads: usize,
) -> (Domain, usize) {
    let width = to[0] - from[0] + 3;
    let rows = to[1] - from[1] + 3;
    let ring = field::parallel_rows(usize::try_from(rows).expect("a window"), threads, |row| {
        let j = from[1] - 1 + i32::try_from(row).expect("a window row");
        (0..width)
            .map(|col| field::node(world, hydrology, [from[0] - 1 + col, j]))
            .collect::<Vec<_>>()
    });
    let at = |n: [i32; 2]| -> Node {
        let (col, row) = (n[0] - from[0] + 1, n[1] - from[1] + 1);
        ring[usize::try_from(row).expect("in the ring")][usize::try_from(col).expect("in the ring")]
    };
    let inside = |n: [i32; 2]| n[0] >= from[0] && n[0] <= to[0] && n[1] >= from[1] && n[1] <= to[1];
    let mut nodes = Vec::new();
    for i in from[0]..=to[0] {
        for j in from[1]..=to[1] {
            if at([i, j]).height >= 0.0 {
                nodes.push([i, j]);
            }
        }
    }
    let field = nodes.iter().map(|&n| at(n)).collect();
    let outlet = nodes
        .iter()
        .map(|&n| {
            let mut best: Option<(f64, [i32; 2], OutletKind)> = None;
            for d in NEIGHBOURS {
                let m = [n[0] + d[0], n[1] + d[1]];
                let h = at(m).height;
                let kind = if h < 0.0 {
                    OutletKind::Sea
                } else if inside(m) {
                    continue;
                } else {
                    OutletKind::Edge
                };
                if best.is_none_or(|(held, node, _)| (h, m) < (held, node)) {
                    best = Some((h, m, kind));
                }
            }
            best.map(|(h, m, kind)| (m, kind, h))
        })
        .collect();
    (
        Domain {
            nodes,
            field,
            outlet,
            capped: false,
        },
        0,
    )
}
