//! Meshing a field on a grid: surface nets topology with dual contouring's
//! vertex placement.
//!
//! Every grid cell the surface passes through gets one vertex and every grid
//! edge it crosses gets a quad joining the four cells around that edge, so
//! the output is welded and closed by construction. Where surface nets puts
//! the vertex at the mean of the edge crossings, which rounds every edge
//! off to the grid's scale, this solves for the point nearest all the
//! crossings' tangent planes (a quadric error function, pulled a little
//! towards the mean so a flat patch stays put). A facet's edge therefore
//! lands on the edge, which is what keeps a coarse level's silhouette
//! chunky rather than melted.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, a, b"
)]
use std::collections::HashMap;

use ashlar::glam::{DMat3, DVec3};

use crate::field::Rock;

/// A welded, closed, outward-wound triangle mesh.
#[derive(Clone, Debug, Default)]
pub(crate) struct Raw {
    pub positions: Vec<DVec3>,
    pub indices: Vec<u32>,
}

impl Raw {
    pub(crate) fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Grid cells per sparse block along each axis.
const BLOCK: usize = 6;

/// The twelve edges of a grid cell, as corner offsets.
const EDGES: [([usize; 3], [usize; 3]); 12] = [
    ([0, 0, 0], [1, 0, 0]),
    ([0, 1, 0], [1, 1, 0]),
    ([0, 0, 1], [1, 0, 1]),
    ([0, 1, 1], [1, 1, 1]),
    ([0, 0, 0], [0, 1, 0]),
    ([1, 0, 0], [1, 1, 0]),
    ([0, 0, 1], [0, 1, 1]),
    ([1, 0, 1], [1, 1, 1]),
    ([0, 0, 0], [0, 0, 1]),
    ([1, 0, 0], [1, 0, 1]),
    ([0, 1, 0], [0, 1, 1]),
    ([1, 1, 0], [1, 1, 1]),
];

/// How far the vertex is pulled towards the crossings' mean, per crossing.
const BIAS: f64 = 0.15;

struct Grid {
    origin: DVec3,
    step: f64,
    /// Sample points along each axis.
    n: [usize; 3],
}

impl Grid {
    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.n[0] * (y + self.n[1] * z)
    }

    #[allow(clippy::cast_precision_loss, reason = "grid indices are small")]
    fn point(&self, x: usize, y: usize, z: usize) -> DVec3 {
        self.origin + DVec3::new(x as f64, y as f64, z as f64) * self.step
    }
}

/// Mesh `rock` inside `bounds` with grid cells `step` metres on a side.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    reason = "grid sizes are positive and small; the mesher reads best in one piece"
)]
pub(crate) fn mesh(rock: &Rock, bounds: [DVec3; 2], step: f64) -> Raw {
    let [min, max] = bounds;
    let pad = step * 1.5;
    let origin = min - DVec3::splat(pad);
    let span = max - min + DVec3::splat(2.0 * pad);
    let n = [
        (span.x / step).ceil() as usize + 1,
        (span.y / step).ceil() as usize + 1,
        (span.z / step).ceil() as usize + 1,
    ];
    let grid = Grid { origin, step, n };
    let values = sample(rock, &grid);

    // One vertex per cell the surface passes through.
    let cells = [n[0] - 1, n[1] - 1, n[2] - 1];
    let cell_index = |x: usize, y: usize, z: usize| x + cells[0] * (y + cells[1] * z);
    let mut vertex_of = vec![u32::MAX; cells[0] * cells[1] * cells[2]];
    let mut positions = Vec::new();
    let eps = step * 0.1;
    // Keyed lookups only, never iterated, so the map's order cannot reach
    // the output.
    let mut crossing: HashMap<usize, (DVec3, DVec3)> = HashMap::new();
    for z in 0..cells[2] {
        for y in 0..cells[1] {
            for x in 0..cells[0] {
                let corner = |c: [usize; 3]| values[grid.index(x + c[0], y + c[1], z + c[2])];
                let inside = (0..8)
                    .filter(|&i| corner([i & 1, (i >> 1) & 1, (i >> 2) & 1]) < 0.0)
                    .count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let mut ata = DMat3::ZERO;
                let mut atb = DVec3::ZERO;
                let mut mass = DVec3::ZERO;
                let mut count = 0.0;
                for (a, b) in EDGES {
                    let (va, vb) = (corner(a), corner(b));
                    if (va < 0.0) == (vb < 0.0) {
                        continue;
                    }
                    // Four cells share an edge; its crossing is found once.
                    let axis = (0..3).find(|&k| a[k] != b[k]).unwrap_or(0);
                    let slot = grid.index(x + a[0], y + a[1], z + a[2]) * 3 + axis;
                    let (p, normal) = if let Some(&known) = crossing.get(&slot) {
                        known
                    } else {
                        let t = va / (va - vb);
                        let pa = grid.point(x + a[0], y + a[1], z + a[2]);
                        let pb = grid.point(x + b[0], y + b[1], z + b[2]);
                        let p = pa.lerp(pb, t);
                        let found = (p, rock.normal(p, eps));
                        crossing.insert(slot, found);
                        found
                    };
                    ata += outer(normal);
                    atb += normal * normal.dot(p);
                    mass += p;
                    count += 1.0;
                }
                let mass = mass / count;
                let bias = BIAS * count;
                let system = ata + DMat3::from_diagonal(DVec3::splat(bias));
                let solved = system.inverse() * (atb + mass * bias);
                let low = grid.point(x, y, z);
                let high = low + DVec3::splat(step);
                let p = if solved.is_finite() {
                    solved.clamp(low, high)
                } else {
                    mass
                };
                vertex_of[cell_index(x, y, z)] =
                    u32::try_from(positions.len()).expect("a rock is far under 4 billion vertices");
                positions.push(p);
            }
        }
    }

    // One quad per grid edge the surface crosses, joining its four cells.
    let mut indices = Vec::new();
    for z in 1..n[2] - 1 {
        for y in 1..n[1] - 1 {
            for x in 1..n[0] - 1 {
                let here = values[grid.index(x, y, z)];
                // Each axis: the neighbour along it, and the four cells
                // round the edge in the order that winds them about +axis.
                for axis in 0..3 {
                    let (nx, ny, nz) = match axis {
                        0 => (x + 1, y, z),
                        1 => (x, y + 1, z),
                        _ => (x, y, z + 1),
                    };
                    if nx >= n[0] || ny >= n[1] || nz >= n[2] {
                        continue;
                    }
                    let there = values[grid.index(nx, ny, nz)];
                    if (here < 0.0) == (there < 0.0) {
                        continue;
                    }
                    let ring = match axis {
                        0 => [
                            cell_index(x, y - 1, z - 1),
                            cell_index(x, y, z - 1),
                            cell_index(x, y, z),
                            cell_index(x, y - 1, z),
                        ],
                        1 => [
                            cell_index(x - 1, y, z - 1),
                            cell_index(x - 1, y, z),
                            cell_index(x, y, z),
                            cell_index(x, y, z - 1),
                        ],
                        _ => [
                            cell_index(x - 1, y - 1, z),
                            cell_index(x, y - 1, z),
                            cell_index(x, y, z),
                            cell_index(x - 1, y, z),
                        ],
                    };
                    let mut quad = ring.map(|c| vertex_of[c]);
                    if quad.contains(&u32::MAX) {
                        continue;
                    }
                    // Wound about +axis when the inside is behind; reversed
                    // when the inside is ahead.
                    if there < 0.0 {
                        quad.reverse();
                    }
                    let [a, b, c, d] = quad;
                    let p = |i: u32| positions[i as usize];
                    if p(a).distance_squared(p(c)) <= p(b).distance_squared(p(d)) {
                        indices.extend([a, b, c, a, c, d]);
                    } else {
                        indices.extend([b, c, d, b, d, a]);
                    }
                }
            }
        }
    }
    keep_solid(Raw { positions, indices })
}

/// Drop the small closed pieces a thin feature can break off into (a lip
/// of a stratum cut through by its own undercut): anything under a tenth of
/// the largest piece's triangles, which a rock never means to have.
fn keep_solid(raw: Raw) -> Raw {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let count = raw.positions.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for tri in raw.indices.as_chunks::<3>().0 {
        let a = root(&mut parent, tri[0] as usize);
        for &other in &tri[1..] {
            let b = root(&mut parent, other as usize);
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut size = vec![0_usize; count];
    for tri in raw.indices.as_chunks::<3>().0 {
        size[root(&mut parent, tri[0] as usize)] += 1;
    }
    let largest = size.iter().copied().max().unwrap_or(0);
    let mut remap = vec![u32::MAX; count];
    let mut positions = Vec::with_capacity(count);
    let mut indices = Vec::with_capacity(raw.indices.len());
    for tri in raw.indices.as_chunks::<3>().0 {
        if size[root(&mut parent, tri[0] as usize)] * 10 < largest {
            continue;
        }
        for &i in tri {
            let slot = &mut remap[i as usize];
            if *slot == u32::MAX {
                *slot = u32::try_from(positions.len()).expect("fewer vertices than before");
                positions.push(raw.positions[i as usize]);
            }
            indices.push(*slot);
        }
    }
    Raw { positions, indices }
}

fn outer(v: DVec3) -> DMat3 {
    DMat3::from_cols(v * v.x, v * v.y, v * v.z)
}

/// The field at every grid point, skipping the insides of blocks the
/// surface cannot reach: a block whose centre is further from the surface
/// than its own half diagonal takes the centre's sign throughout.
#[allow(clippy::cast_precision_loss, reason = "block sizes are small")]
fn sample(rock: &Rock, grid: &Grid) -> Vec<f64> {
    let [nx, ny, nz] = grid.n;
    let mut values = vec![0.0; nx * ny * nz];
    let reach = (BLOCK as f64) * grid.step * 0.5 * 3.0_f64.sqrt();
    for bz in (0..nz).step_by(BLOCK) {
        for by in (0..ny).step_by(BLOCK) {
            for bx in (0..nx).step_by(BLOCK) {
                let ex = (bx + BLOCK).min(nx);
                let ey = (by + BLOCK).min(ny);
                let ez = (bz + BLOCK).min(nz);
                let centre = (grid.point(bx, by, bz) + grid.point(ex - 1, ey - 1, ez - 1)) * 0.5;
                let d = rock.distance(centre);
                // The warp and the strata stretch the field a little, so
                // the margin is generous.
                let far = d.abs() > reach * 1.6 + grid.step;
                for z in bz..ez {
                    for y in by..ey {
                        for x in bx..ex {
                            values[grid.index(x, y, z)] = if far {
                                d
                            } else {
                                rock.distance(grid.point(x, y, z))
                            };
                        }
                    }
                }
            }
        }
    }
    values
}
