//! Where the water goes: two priority floods, the lakes kept, the pits
//! breached, the rain summed.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::field::NODE_AREA_M2;
use super::mass::{Domain, NEIGHBOURS, OutletKind};
use super::{RiverParams, local};
use crate::Site;

/// No node.
pub(crate) const NONE: u32 = u32::MAX;

/// Seconds in a year of 365.25 days: runoff in metres a year to m3/s.
pub(crate) const SECONDS_PER_YEAR: f64 = 31_557_600.0;

/// How a node with no receiver ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Root {
    /// It drains out of the domain into this node, of this coarse height.
    Out([i32; 2], OutletKind, f64),
    /// Nothing lower is reachable: ground walled in by a settlement's.
    Sink,
}

/// A lake as the routing found it.
pub(crate) struct LakeBuild {
    pub level: f64,
    pub depth: f64,
    /// Its nodes, in order.
    pub nodes: Vec<u32>,
    /// The one lake node that drains out of it.
    pub outlet: u32,
}

/// The routing of a domain.
pub(crate) struct Routed {
    /// Each node's downstream neighbour, [`NONE`] for a root or a blocked
    /// node.
    pub receiver: Vec<u32>,
    /// How each root ends.
    pub root: Vec<Option<Root>>,
    /// The breached ground, a lake's level on a lake: never below the
    /// receiver's, so water never climbs.
    pub surface: Vec<f64>,
    /// Each node's lake, [`NONE`] for none.
    pub lake: Vec<u32>,
    pub lakes: Vec<LakeBuild>,
    /// m3/s through each node.
    pub discharge: Vec<f64>,
    /// Roots first, every node after its receiver.
    pub order: Vec<u32>,
    /// Settlement ground: not routed, no water.
    pub blocked: Vec<bool>,
}

/// The domain's nodes on a dense grid over their bounds.
pub(crate) struct Grid {
    origin: [i32; 2],
    width: i32,
    height: i32,
    cells: Vec<u32>,
}

impl Grid {
    pub fn new(nodes: &[[i32; 2]]) -> Self {
        let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
        for n in nodes {
            for a in 0..2 {
                lo[a] = lo[a].min(n[a]);
                hi[a] = hi[a].max(n[a]);
            }
        }
        if nodes.is_empty() {
            (lo, hi) = ([0, 0], [-1, -1]);
        }
        let (width, height) = (hi[0] - lo[0] + 1, hi[1] - lo[1] + 1);
        // In 64 bits: a thin diagonal mass under the cap can have a box whose
        // area overflows an i32, and the box, not the node count, is the
        // memory. A cut mass's box is at most a cell.
        let area = i64::from(width) * i64::from(height);
        let mut cells = vec![NONE; usize::try_from(area).expect("a mass's bounds fit memory")];
        let mut grid = Self {
            origin: lo,
            width,
            height,
            cells: Vec::new(),
        };
        for (k, &n) in nodes.iter().enumerate() {
            cells[grid.cell(n)] = local(k);
        }
        grid.cells = cells;
        grid
    }

    /// The local index of node `n`, if it is in the domain.
    pub fn at(&self, n: [i32; 2]) -> Option<u32> {
        let (i, j) = (n[0] - self.origin[0], n[1] - self.origin[1]);
        if i < 0 || j < 0 || i >= self.width || j >= self.height {
            return None;
        }
        let k = self.cells[self.cell(n)];
        (k != NONE).then_some(k)
    }

    /// Where node `n`, inside the box, is in `cells`.
    fn cell(&self, n: [i32; 2]) -> usize {
        let (i, j) = (
            i64::from(n[0] - self.origin[0]),
            i64::from(n[1] - self.origin[1]),
        );
        usize::try_from(j * i64::from(self.width) + i).expect("inside the box")
    }
}

/// A heap entry: the level's bits in an order that sorts as the numbers do,
/// then the node, so equal levels go to the lower coordinate.
type Key = Reverse<(u64, [i32; 2], u32)>;

fn key(level: f64, node: [i32; 2], k: u32) -> Key {
    let bits = level.to_bits();
    let ordered = if bits >> 63 == 1 {
        !bits
    } else {
        bits | (1 << 63)
    };
    Reverse((ordered, node, k))
}

/// The second flood's heap entry: [`Key`]'s, and the node a step across the
/// grain comes from ([`NONE`] for a node entered already, to spread from).
type Entry = Reverse<(u64, [i32; 2], u32, u32)>;

fn entry(level: f64, node: [i32; 2], k: u32, from: u32) -> Entry {
    let Reverse((bits, node, k)) = key(level, node, k);
    Reverse((bits, node, k, from))
}

/// How far the flood's level must rise past a node before it is entered
/// across the fold axis where the grain is whole, metres.
const GRAIN_PENALTY_M: f64 = 30.0;

/// Enters node `q` from `c`: its receiver, and its level, which is its floor
/// unless that is under `c`'s: then the path out is lowered to it (a pit
/// breached), or, if that path runs through a lake, the node fills to the
/// path's level instead (a pond too small to keep apart from the lake
/// beside it). Returns the level.
fn enter(
    q: u32,
    c: u32,
    floor: &[f64],
    lake: &[u32],
    surface: &mut [f64],
    receiver: &mut [u32],
) -> f64 {
    let qi = q as usize;
    receiver[qi] = c;
    let mut level = floor[qi];
    if level < surface[c as usize] {
        let mut through_lake = false;
        let mut p = c;
        while p != NONE && surface[p as usize] > level {
            if lake[p as usize] != NONE {
                through_lake = true;
                break;
            }
            p = receiver[p as usize];
        }
        if through_lake {
            level = surface[c as usize];
        } else {
            let mut p = c;
            while p != NONE && surface[p as usize] > level {
                surface[p as usize] = level;
                p = receiver[p as usize];
            }
        }
    }
    surface[qi] = level;
    level
}

/// The distance from `(x, z)` to the nearest settlement's extent, metres.
pub(crate) fn site_distance(sites: &[Site], x: f64, z: f64) -> f64 {
    sites
        .iter()
        .map(|site| site.extent_distance_at(x, z))
        .fold(f64::INFINITY, f64::min)
}

/// Routes the water over a domain.
#[allow(
    clippy::too_many_lines,
    reason = "the floods, the lakes and the sums, in order"
)]
pub(crate) fn route(
    domain: &Domain,
    params: &RiverParams,
    sites: &[Site],
    axis: (f64, f64),
) -> Routed {
    let nodes = &domain.nodes;
    let n = nodes.len();
    let grid = Grid::new(nodes);
    let height: Vec<f64> = domain.field.iter().map(|f| f.height).collect();
    let blocked: Vec<bool> = nodes
        .iter()
        .map(|&[i, j]| {
            site_distance(sites, super::metres(i), super::metres(j)) < params.site_margin_m
        })
        .collect();
    let grid = &grid;
    let neighbours = |k: u32| {
        let at = nodes[k as usize];
        NEIGHBOURS
            .iter()
            .filter_map(move |d| grid.at([at[0] + d[0], at[1] + d[1]]))
    };

    // The first flood fills every pit to its spill: what a lake would be.
    let mut filled = vec![f64::NAN; n];
    let mut heap: BinaryHeap<Key> = BinaryHeap::new();
    let mut seeds = Vec::new();
    for k in 0..n {
        let Some((outside, kind, outside_height)) = domain.outlet[k] else {
            continue;
        };
        // A mouth into a settlement's own water would put the river's last
        // stretch on the ground the layout is fitted to.
        let into_site = site_distance(sites, super::metres(outside[0]), super::metres(outside[1]))
            < params.site_margin_m;
        if !blocked[k] && !into_site {
            filled[k] = height[k];
            heap.push(key(height[k], nodes[k], local(k)));
            seeds.push((local(k), Root::Out(outside, kind, outside_height)));
        }
    }
    loop {
        while let Some(Reverse((_, _, c))) = heap.pop() {
            let level = filled[c as usize];
            for q in neighbours(c) {
                let qi = q as usize;
                if blocked[qi] || !filled[qi].is_nan() {
                    continue;
                }
                filled[qi] = height[qi].max(level);
                heap.push(key(filled[qi], nodes[qi], q));
            }
        }
        // Ground no outlet reaches (walled in by a settlement's) drains to
        // its own lowest node.
        let Some(k) = (0..n)
            .filter(|&k| !blocked[k] && filled[k].is_nan())
            .min_by(|&a, &b| {
                height[a]
                    .total_cmp(&height[b])
                    .then(nodes[a].cmp(&nodes[b]))
            })
        else {
            break;
        };
        filled[k] = height[k];
        heap.push(key(height[k], nodes[k], local(k)));
        seeds.push((local(k), Root::Sink));
    }

    // The pits: connected nodes filled over their ground to one level. A
    // pit deeper than its province keeps is a lake; the rest are breached.
    let mut lake = vec![NONE; n];
    let mut lakes = Vec::new();
    let mut seen = vec![false; n];
    for start in 0..n {
        if seen[start] || blocked[start] || filled[start] <= height[start] {
            continue;
        }
        let level = filled[start];
        let mut members = vec![local(start)];
        seen[start] = true;
        let mut cursor = 0;
        while cursor < members.len() {
            let c = members[cursor];
            cursor += 1;
            for q in neighbours(c) {
                let qi = q as usize;
                if !seen[qi]
                    && !blocked[qi]
                    && filled[qi].to_bits() == level.to_bits()
                    && filled[qi] > height[qi]
                {
                    seen[qi] = true;
                    members.push(q);
                }
            }
        }
        members.sort_unstable();
        let bottom = members
            .iter()
            .copied()
            .min_by(|&a, &b| {
                height[a as usize]
                    .total_cmp(&height[b as usize])
                    .then(nodes[a as usize].cmp(&nodes[b as usize]))
            })
            .expect("a pit has a node");
        let depth = level - height[bottom as usize];
        let keep = domain.field[bottom as usize].keep * params.lake_depth_scale;
        if depth >= keep {
            let id = local(lakes.len());
            for &m in &members {
                lake[m as usize] = id;
            }
            lakes.push(LakeBuild {
                level,
                depth,
                nodes: members,
                outlet: NONE,
            });
        }
    }

    // The second flood routes over the lakes' levels and breaches every
    // other pit: a node entered below the node it was entered from lowers
    // the path out to its own level, unless that path runs through a lake,
    // in which case it fills to the path's level instead (a pond too small
    // to keep apart from the lake beside it).
    let floor: Vec<f64> = (0..n)
        .map(|k| {
            if lake[k] == NONE {
                height[k]
            } else {
                lakes[lake[k] as usize].level
            }
        })
        .collect();
    let mut surface = vec![f64::NAN; n];
    let mut receiver = vec![NONE; n];
    let mut root = vec![None; n];
    let mut second: BinaryHeap<Entry> = BinaryHeap::new();
    for &(k, how) in &seeds {
        let ki = k as usize;
        surface[ki] = floor[ki];
        root[ki] = Some(how);
        second.push(entry(floor[ki], nodes[ki], k, NONE));
    }
    // Where the rock has a grain (a province's `along_grain`), a step
    // across the fold axis is entered only once the flood's level has
    // risen [`GRAIN_PENALTY_M`] past it, by the grain and the step's
    // share across the axis squared, unless the node was entered along
    // the grain first: water gathers along the folds and crosses them
    // where it must, in gaps (trellis drainage). Where there is no grain
    // a node is entered by the first neighbour the flood reaches, as it
    // always was.
    let crossing = |c: u32, q: u32| {
        let grain = domain.field[q as usize].grain;
        if grain <= 0.0 {
            return 0.0;
        }
        let (a, b) = (nodes[c as usize], nodes[q as usize]);
        let (dx, dz) = (f64::from(b[0] - a[0]), f64::from(b[1] - a[1]));
        let across = dx * axis.1 - dz * axis.0;
        GRAIN_PENALTY_M * grain * across * across / (dx * dx + dz * dz)
    };
    while let Some(Reverse((_, _, k, from))) = second.pop() {
        if from != NONE {
            // A step across the grain, taken if nothing entered the node
            // meanwhile.
            if surface[k as usize].is_nan() {
                let level = enter(k, from, &floor, &lake, &mut surface, &mut receiver);
                second.push(entry(level, nodes[k as usize], k, NONE));
            }
            continue;
        }
        let c = k;
        for q in neighbours(c) {
            let qi = q as usize;
            if blocked[qi] || !surface[qi].is_nan() {
                continue;
            }
            let penalty = crossing(c, q);
            if penalty > 0.0 {
                second.push(entry(floor[qi] + penalty, nodes[qi], q, c));
                continue;
            }
            let level = enter(q, c, &floor, &lake, &mut surface, &mut receiver);
            second.push(entry(level, nodes[qi], q, NONE));
        }
    }

    // A lake drains through one outlet: the lake node whose receiver
    // outside is lowest; every other node of it drains to that one. A
    // candidate whose water comes back into the lake (a rim node standing at
    // the level to the bit, entered from the lake, ties with the true spill)
    // would close a loop and lose the catchment's water, so it is passed
    // over. Surfaces never rise along receivers and the lake's nodes stand
    // at its level, so the walk only crosses ground at the level itself.
    for (id, built) in lakes.iter_mut().enumerate() {
        let id = local(id);
        let level = built.level;
        let drains_back = |m: u32| {
            let mut p = receiver[m as usize];
            while p != NONE && surface[p as usize] >= level {
                if lake[p as usize] == id {
                    return true;
                }
                p = receiver[p as usize];
            }
            false
        };
        let candidates: Vec<u32> = built
            .nodes
            .iter()
            .copied()
            .filter(|&m| {
                let r = receiver[m as usize];
                r == NONE || lake[r as usize] != id
            })
            .collect();
        let clear: Vec<u32> = candidates
            .iter()
            .copied()
            .filter(|&m| !drains_back(m))
            .collect();
        let outlet = if clear.is_empty() { candidates } else { clear }
            .into_iter()
            .min_by(|&a, &b| {
                let level = |m: u32| {
                    let r = receiver[m as usize];
                    if r == NONE {
                        f64::NEG_INFINITY
                    } else {
                        surface[r as usize]
                    }
                };
                level(a)
                    .total_cmp(&level(b))
                    .then(nodes[a as usize].cmp(&nodes[b as usize]))
            })
            .expect("a lake drains somewhere");
        for &m in &built.nodes {
            if m != outlet {
                receiver[m as usize] = outlet;
                root[m as usize] = None;
            }
        }
        built.outlet = outlet;
    }

    // Roots first, then every node after its receiver, children in order.
    let mut first_child = vec![NONE; n];
    let mut next_sibling = vec![NONE; n];
    for k in (0..n).rev() {
        let r = receiver[k];
        if r != NONE {
            next_sibling[k] = first_child[r as usize];
            first_child[r as usize] = local(k);
        }
    }
    let mut order: Vec<u32> = (0..n)
        .filter(|&k| !blocked[k] && receiver[k] == NONE)
        .map(local)
        .collect();
    let mut cursor = 0;
    while cursor < order.len() {
        let mut child = first_child[order[cursor] as usize];
        cursor += 1;
        while child != NONE {
            order.push(child);
            child = next_sibling[child as usize];
        }
    }

    let per_node = NODE_AREA_M2 * params.runoff_m_per_year / SECONDS_PER_YEAR;
    let mut discharge: Vec<f64> = (0..n)
        .map(|k| {
            if blocked[k] {
                0.0
            } else {
                let f = &domain.field[k];
                f.rain * (1.0 - f.sink) * per_node
            }
        })
        .collect();
    for &k in order.iter().rev() {
        let r = receiver[k as usize];
        if r != NONE {
            discharge[r as usize] += discharge[k as usize];
        }
    }

    Routed {
        receiver,
        root,
        surface,
        lake,
        lakes,
        discharge,
        order,
        blocked,
    }
}
