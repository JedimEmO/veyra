//! Port Serein's walkability audit on seed 42 (ticket 0015, step 2): can a
//! character walk from the arrival on the quay to every place the first hour
//! sends it, and is every stair the generator lays clear from its foot to its
//! head?
//!
//! The audit reads what the physics builds, through the physics crate's own
//! functions where it has them: every building of the baked layout as its
//! level 0 convex proxies at the building's pose (a building with one solid
//! that will not wrap collides with nothing, as in the game), the terrain as
//! each region's heightfield on its one-metre lattice split on the
//! heightfield's own diagonal, the trunks the region tasks place (the wild
//! ones and the city's planted ones), and the roadside and crossing pieces
//! round the city, and the quest people standing in it (each a kinematic
//! character capsule that blocks the player, put at the stands `locate`
//! computes). The household's props have no collider, so a walker passes
//! through them, and the procedural sites stand far from the city.
//!
//! The walk is a grid in the city's frame at half a metre. A cell's floors
//! are the tops of whatever stands in its column that nothing covers; a
//! floor is standable where it slopes no more than the controller's
//! `max_slope` and the capsule above it, from a step over the floor to the
//! head, meets no collider and no ground. Two neighbouring floors connect
//! when they differ by no more than a step. A fall would carry a walker
//! down further, but every route the audit asks about climbs, so it counts
//! none.
//!
//! The test writes `scratch/combat/nav/audit.png` (reachable green, lighter
//! higher; ground or stair standable but cut off orange; a roof no walk
//! reaches brown; blocked or too steep dark; water blue; stairs outlined cyan
//! when clear and red when not; the walked routes yellow; places magenta, or
//! red when not reached; the arrival white) and `audit.txt` beside it, the
//! frame's `+z` (downhill, to the sea) down the picture. A stair is its
//! flights chained foot to head, and clear when a walk inside its own box
//! and a little round it gets from the ground before its foot to the ground
//! past its head; the list says where one is not, and what stops it.

#![allow(
    clippy::many_single_char_names,
    reason = "frame and cell coordinates x, z, i, j and heights y read as the geometry does"
)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "grid and lattice indices of a few thousand, and metres in f32 within a kilometre of the city's centre"
)]

use super::*;
use std::collections::BinaryHeap;
use std::fmt::Write as _;
use std::sync::Arc;
use strata::core::{GRID_N, RegionCoord};
use strata::physics::Heightfield;
use strata::physics::avian3d::collision::collider::contact_query::intersection_test;
use strata::physics::avian3d::prelude::{Collider, SimpleCollider};
use strata::physics::flora::{FloraPlacement, ShapeCache, build_trunk, place_trunks, trunks_of};
use strata::worldgen::{ProceduralWorld, SettlementLayout, WorldSource};

/// The audit's cell, metres.
const CELL_M: f64 = 0.5;
/// The leaves' index tile, metres.
const TILE_M: f64 = 4.0;
/// How far past the city's own ground the grid reaches, metres.
const MARGIN_M: f64 = 20.0;
/// Two surfaces closer than this are one, metres.
const SAME_M: f32 = 0.05;
/// A floor this far under the sea is swum, not walked, metres.
const SWUM_M: f32 = 0.3;
/// How much ground, in cells (10 m2), a stair's head must open onto beyond
/// the stair's own box for it to lead anywhere.
const HEAD_OPENS_CELLS: usize = 40;
/// The fewest stairs seed 42's Port Serein lays, so an audit that finds none
/// fails instead of passing.
const STAIRS_AT_LEAST: usize = 60;
/// No leaf.
const NONE: u32 = u32::MAX;
/// The blocker of a floor whose capsule meets the ground.
const GROUND: u32 = u32::MAX - 1;
/// The blocker of a floor too steep to stand on.
const STEEP: u32 = u32::MAX - 2;
/// The blocker of a floor under the sea.
const WATER: u32 = u32::MAX - 3;

/// What a collider is, for the report.
#[derive(Debug, Clone)]
struct Source {
    label: String,
    stair: bool,
}

/// One convex collider, posed in the audit's space: the world's axes, its
/// origin the city's centre at height zero.
struct Leaf {
    collider: Collider,
    at: Vec3,
    turn: Quat,
    min: Vec3,
    max: Vec3,
    source: usize,
    column: Column,
}

/// How a leaf meets a vertical line, worked out exactly: a ray cast against
/// a hull by its support map misses now and then where it plainly passes
/// through it, which read as holes in the stairs.
enum Column {
    /// A convex solid's faces, `(normal, offset)` with `normal . p <=
    /// offset` inside, in the audit's space.
    Planes(Vec<([f64; 3], f64)>),
    /// An upright trunk: its axis `(x, z)`, radius, bottom and top.
    Trunk([f64; 2], f64, f64, f64),
    /// Anything else (a log), by rays.
    Rays,
}

impl Leaf {
    /// Every convex part of `collider` at a pose, a leaf each.
    fn parts(collider: &Collider, at: Vec3, turn: Quat, source: usize) -> Vec<Self> {
        let shape = collider.shape();
        if let Some(compound) = shape.as_compound() {
            return compound
                .shapes()
                .iter()
                .flat_map(|(pose, part)| {
                    Self::parts(
                        &Collider::from(part.clone()),
                        at + turn * pose.translation,
                        turn * pose.rotation,
                        source,
                    )
                })
                .collect();
        }
        let column = if let Some(hull) = shape.as_convex_polyhedron() {
            let points = hull.points();
            let corners = hull.vertices_adj_to_face();
            Column::Planes(
                hull.faces()
                    .iter()
                    .map(|face| {
                        let n = (turn * face.normal).as_dvec3();
                        let p = (turn
                            * points[corners[face.first_vertex_or_edge as usize] as usize]
                            + at)
                            .as_dvec3();
                        (n.to_array(), n.dot(p))
                    })
                    .collect(),
            )
        } else if let Some(trunk) = shape.as_cylinder()
            && (turn * Vec3::Y).y > 0.999
        {
            Column::Trunk(
                [f64::from(at.x), f64::from(at.z)],
                f64::from(trunk.radius),
                f64::from(at.y - trunk.half_height),
                f64::from(at.y + trunk.half_height),
            )
        } else {
            Column::Rays
        };
        let aabb = collider.aabb(at, turn);
        vec![Self {
            collider: collider.clone(),
            at,
            turn,
            min: aabb.min,
            max: aabb.max,
            source,
            column,
        }]
    }
}

/// A floor of a cell.
#[derive(Debug, Clone, Copy)]
struct Floor {
    y: f32,
    /// [`NONE`] where a body stands, else what stops it.
    blocker: u32,
    /// The leaf whose top it is, [`GROUND`] for the terrain.
    top: u32,
}

impl Floor {
    fn standable(self) -> bool {
        self.blocker == NONE
    }
}

/// The terrain's heights on the one-metre lattice of the lod 0 regions.
struct Ground {
    gx0: i64,
    gz0: i64,
    nx: usize,
    nz: usize,
    heights: Vec<f32>,
    centre: [f64; 2],
}

impl Ground {
    fn h(&self, i: i64, j: i64) -> f32 {
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "lattice indices inside the sampled square"
        )]
        let (i, j) = (
            (i - self.gx0).clamp(0, self.nx as i64 - 1) as usize,
            (j - self.gz0).clamp(0, self.nz as i64 - 1) as usize,
        );
        self.heights[j * self.nx + i]
    }

    /// The height and the up share of the normal at a point of the audit's
    /// space, on the heightfield's triangles: each cell split from `(i + 1,
    /// j)` to `(i, j + 1)`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lattice indices of a few thousand; heights in f32"
    )]
    fn at(&self, x: f64, z: f64) -> (f32, f32) {
        let (wx, wz) = (x + self.centre[0], z + self.centre[1]);
        let (fi, fj) = (wx.floor(), wz.floor());
        let (i, j) = (fi as i64, fj as i64);
        let (u, v) = ((wx - fi) as f32, (wz - fj) as f32);
        let (h00, h10, h01, h11) = (
            self.h(i, j),
            self.h(i + 1, j),
            self.h(i, j + 1),
            self.h(i + 1, j + 1),
        );
        let (h, gx, gz) = if u + v <= 1.0 {
            (
                h00 + u * (h10 - h00) + v * (h01 - h00),
                h10 - h00,
                h01 - h00,
            )
        } else {
            (
                h11 + (1.0 - u) * (h01 - h11) + (1.0 - v) * (h10 - h11),
                h11 - h01,
                h11 - h10,
            )
        };
        (h, 1.0 / (1.0 + gx * gx + gz * gz).sqrt())
    }
}

/// The character as the physics moves the player.
struct Body {
    radius: f32,
    height: f32,
    step: f32,
    min_up: f32,
    capsule: Collider,
}

impl Body {
    fn player() -> Self {
        let controller = CharacterController {
            height: veyra_game::PLAYER_HEIGHT_M,
            ..CharacterController::default()
        };
        let (radius, step) = (controller.radius, controller.step_height);
        // From a step over the feet to the head: what the knees and the body
        // meet. Below it is a step, which the walk's edges judge.
        let length = (controller.height - step - 2.0 * radius).max(0.0);
        Self {
            radius,
            height: controller.height,
            step,
            min_up: controller.max_slope.cos(),
            capsule: Collider::capsule(radius, length),
        }
    }
}

/// Everything the walk reads.
struct Scene {
    site: strata::worldgen::Site,
    ground: Ground,
    leaves: Vec<Leaf>,
    sources: Vec<Source>,
    /// The leaves by tile of the audit's space.
    tiles: Vec<Vec<u32>>,
    tile0: [f64; 2],
    tiles_n: [usize; 2],
    sea: f32,
    body: Body,
}

impl Scene {
    fn tile_range(&self, min: [f64; 2], max: [f64; 2]) -> ([usize; 2], [usize; 2]) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "tile indices clamped into the grid"
        )]
        let index = |v: f64, origin: f64, n: usize| -> usize {
            (((v - origin) / TILE_M).floor().max(0.0) as usize).min(n - 1)
        };
        (
            [
                index(min[0], self.tile0[0], self.tiles_n[0]),
                index(min[1], self.tile0[1], self.tiles_n[1]),
            ],
            [
                index(max[0], self.tile0[0], self.tiles_n[0]),
                index(max[1], self.tile0[1], self.tiles_n[1]),
            ],
        )
    }

    fn index(&mut self) {
        let ranges: Vec<_> = self
            .leaves
            .iter()
            .map(|leaf| {
                self.tile_range(
                    [f64::from(leaf.min.x), f64::from(leaf.min.z)],
                    [f64::from(leaf.max.x), f64::from(leaf.max.z)],
                )
            })
            .collect();
        for (n, (lo, hi)) in ranges.into_iter().enumerate() {
            for tj in lo[1]..=hi[1] {
                for ti in lo[0]..=hi[0] {
                    #[allow(clippy::cast_possible_truncation, reason = "leaf counts")]
                    self.tiles[tj * self.tiles_n[0] + ti].push(n as u32);
                }
            }
        }
    }

    /// The leaves whose box covers the vertical line at `(x, z)`.
    fn over(&self, x: f64, z: f64) -> impl Iterator<Item = u32> + '_ {
        let ([ti, tj], _) = self.tile_range([x, z], [x, z]);
        #[allow(clippy::cast_possible_truncation, reason = "metres in f32")]
        let (xf, zf) = (x as f32, z as f32);
        self.tiles[tj * self.tiles_n[0] + ti]
            .iter()
            .copied()
            .filter(move |&n| {
                let leaf = &self.leaves[n as usize];
                xf >= leaf.min.x && xf <= leaf.max.x && zf >= leaf.min.z && zf <= leaf.max.z
            })
    }

    /// Where a leaf's solid starts and ends on the vertical line at `(x,
    /// z)`, and the up share of its top's normal.
    #[allow(clippy::cast_possible_truncation, reason = "metres in f32")]
    fn span(&self, n: u32, x: f32, z: f32) -> Option<(f32, f32, f32)> {
        let leaf = &self.leaves[n as usize];
        let (xd, zd) = (f64::from(x), f64::from(z));
        match &leaf.column {
            Column::Planes(planes) => {
                let (mut lo, mut hi, mut up) = (f64::MIN, f64::MAX, 1.0);
                for &([nx, ny, nz], d) in planes {
                    let rest = d - nx * xd - nz * zd;
                    if ny > 1e-9 {
                        if rest / ny < hi {
                            hi = rest / ny;
                            up = ny;
                        }
                    } else if ny < -1e-9 {
                        lo = lo.max(rest / ny);
                    } else if rest < 0.0 {
                        return None;
                    }
                }
                (lo < hi).then_some((lo as f32, hi as f32, up as f32))
            }
            Column::Trunk([cx, cz], r, bottom, top) => {
                ((xd - cx).hypot(zd - cz) <= *r).then_some((*bottom as f32, *top as f32, 1.0))
            }
            Column::Rays => {
                let reach = leaf.max.y - leaf.min.y + 1.0;
                let (down, normal) = leaf.collider.cast_ray(
                    leaf.at,
                    leaf.turn,
                    Vec3::new(x, leaf.max.y + 0.5, z),
                    Vec3::NEG_Y,
                    reach,
                    true,
                )?;
                let (up, _) = leaf.collider.cast_ray(
                    leaf.at,
                    leaf.turn,
                    Vec3::new(x, leaf.min.y - 0.5, z),
                    Vec3::Y,
                    reach,
                    true,
                )?;
                Some((leaf.min.y - 0.5 + up, leaf.max.y + 0.5 - down, normal.y))
            }
        }
    }

    /// The floors of the column at `(x, z)`, lowest first, each judged.
    #[allow(clippy::cast_possible_truncation, reason = "metres in f32")]
    fn floors(&self, x: f64, z: f64, out: &mut Vec<Floor>) {
        let (xf, zf) = (x as f32, z as f32);
        let spans: Vec<(f32, f32, f32, u32)> = self
            .over(x, z)
            .filter_map(|n| self.span(n, xf, zf).map(|(lo, hi, up)| (lo, hi, up, n)))
            .collect();
        let (terrain, up) = self.ground.at(x, z);
        let covered = |y: f32| {
            spans
                .iter()
                .any(|&(lo, hi, _, _)| lo < y - SAME_M && hi > y + SAME_M)
        };
        let mut tops: Vec<(f32, f32, u32)> = Vec::new();
        if !covered(terrain) {
            tops.push((terrain, up, GROUND));
        }
        for &(_, hi, up, n) in &spans {
            if hi > terrain - SAME_M && !covered(hi) {
                tops.push((hi, up, n));
            }
        }
        tops.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Two tops a hair apart are one floor, the higher one.
        let mut merged: Vec<(f32, f32, u32)> = Vec::new();
        for top in tops {
            match merged.last_mut() {
                Some(last) if top.0 - last.0 < SAME_M => *last = top,
                _ => merged.push(top),
            }
        }
        for (y, up, top) in merged {
            out.push(Floor {
                y,
                blocker: self.judge(x, z, y, up),
                top,
            });
        }
    }

    /// What stops a body standing on a floor at `y` with the up share `up`,
    /// or [`NONE`].
    #[allow(clippy::cast_possible_truncation, reason = "metres in f32")]
    fn judge(&self, x: f64, z: f64, y: f32, up: f32) -> u32 {
        if up < self.body.min_up - 1e-3 {
            return STEEP;
        }
        if y < self.sea - SWUM_M {
            return WATER;
        }
        let body = &self.body;
        let r = f64::from(body.radius);
        for k in 0..8 {
            let angle = f64::from(k) * std::f64::consts::FRAC_PI_4;
            let (h, _) = self.ground.at(x + r * angle.cos(), z + r * angle.sin());
            if h > y + body.step + 0.01 {
                return GROUND;
            }
        }
        let centre = Vec3::new(
            x as f32,
            y + body.step + (body.height - body.step) * 0.5,
            z as f32,
        );
        let (low, high) = (y + body.step, y + body.height);
        let (lo, hi) = self.tile_range([x - r, z - r], [x + r, z + r]);
        let mut seen: Vec<u32> = Vec::new();
        for tj in lo[1]..=hi[1] {
            for ti in lo[0]..=hi[0] {
                for &n in &self.tiles[tj * self.tiles_n[0] + ti] {
                    if seen.contains(&n) {
                        continue;
                    }
                    seen.push(n);
                    let leaf = &self.leaves[n as usize];
                    let rf = body.radius;
                    if leaf.max.y < low
                        || leaf.min.y > high
                        || leaf.max.x < centre.x - rf
                        || leaf.min.x > centre.x + rf
                        || leaf.max.z < centre.z - rf
                        || leaf.min.z > centre.z + rf
                    {
                        continue;
                    }
                    if intersection_test(
                        &body.capsule,
                        centre,
                        Quat::IDENTITY,
                        &leaf.collider,
                        leaf.at,
                        leaf.turn,
                    )
                    .unwrap_or(false)
                    {
                        return n;
                    }
                }
            }
        }
        NONE
    }

    /// A point of the city's frame in the audit's space.
    fn local(&self, frame: [f64; 2]) -> [f64; 2] {
        let [x, z] = self.site.to_world_xz(frame);
        [x - self.ground.centre[0], z - self.ground.centre[1]]
    }

    /// A world point in the city's frame.
    fn frame(&self, world: (f64, f64)) -> [f64; 2] {
        self.site.to_local_xz([world.0, world.1])
    }

    /// What a floor's top is, in words.
    fn surface(&self, top: u32) -> String {
        match top {
            GROUND => "the ground".to_owned(),
            n => self.sources[self.leaves[n as usize].source].label.clone(),
        }
    }

    /// What a blocker is, in words.
    fn blocker(&self, blocker: u32) -> String {
        match blocker {
            NONE => "nothing".to_owned(),
            GROUND => "the ground (a bank within the body's reach)".to_owned(),
            STEEP => "a slope too steep to stand on".to_owned(),
            WATER => "water".to_owned(),
            n => self.sources[self.leaves[n as usize].source].label.clone(),
        }
    }
}

/// The grid over the city's frame and its floors.
struct Grid {
    x0: f64,
    z0: f64,
    nx: usize,
    nz: usize,
    /// The floors of cell `c` are `floors[start[c]..start[c + 1]]`.
    start: Vec<u32>,
    floors: Vec<Floor>,
}

impl Grid {
    #[allow(clippy::cast_precision_loss, reason = "cell indices of a few thousand")]
    fn centre(&self, i: usize, j: usize) -> [f64; 2] {
        [
            self.x0 + (i as f64 + 0.5) * CELL_M,
            self.z0 + (j as f64 + 0.5) * CELL_M,
        ]
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a frame point inside the grid"
    )]
    fn cell(&self, [x, z]: [f64; 2]) -> Option<(usize, usize)> {
        let (i, j) = ((x - self.x0) / CELL_M, (z - self.z0) / CELL_M);
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.nx && (j as usize) < self.nz)
            .then_some((i as usize, j as usize))
    }

    fn nodes(&self, i: usize, j: usize) -> std::ops::Range<usize> {
        let c = j * self.nx + i;
        self.start[c] as usize..self.start[c + 1] as usize
    }

    /// Each node's neighbours a walker steps to, with the distance.
    fn edges(&self, i: usize, j: usize, y: f32, step: f32, mut f: impl FnMut(usize, f32)) {
        let linked = |ci: usize, cj: usize| {
            self.nodes(ci, cj).find(|&n| {
                self.floors[n].standable() && (self.floors[n].y - y).abs() <= step + 0.01
            })
        };
        #[allow(clippy::cast_possible_truncation, reason = "half a metre")]
        let (straight, diagonal) = (CELL_M as f32, (CELL_M * std::f64::consts::SQRT_2) as f32);
        let (ii, jj) = (i.cast_signed(), j.cast_signed());
        for (di, dj) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (ni, nj) = (ii + di, jj + dj);
            if ni < 0 || nj < 0 || ni.cast_unsigned() >= self.nx || nj.cast_unsigned() >= self.nz {
                continue;
            }
            if let Some(n) = linked(ni.cast_unsigned(), nj.cast_unsigned()) {
                f(n, straight);
            }
        }
        for (di, dj) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            let (ni, nj) = (ii + di, jj + dj);
            if ni < 0 || nj < 0 || ni.cast_unsigned() >= self.nx || nj.cast_unsigned() >= self.nz {
                continue;
            }
            // Not round a corner: both cells beside the step are walked too.
            if linked(ni.cast_unsigned(), j).is_none() || linked(i, nj.cast_unsigned()).is_none() {
                continue;
            }
            if let Some(n) = linked(ni.cast_unsigned(), nj.cast_unsigned()) {
                f(n, diagonal);
            }
        }
    }
}

/// Every node's cell, so a walk can find its neighbours.
fn cells_of(grid: &Grid) -> Vec<(u32, u32)> {
    let mut cells = vec![(0, 0); grid.floors.len()];
    for j in 0..grid.nz {
        for i in 0..grid.nx {
            for n in grid.nodes(i, j) {
                #[allow(clippy::cast_possible_truncation, reason = "cell indices")]
                {
                    cells[n] = (i as u32, j as u32);
                }
            }
        }
    }
    cells
}

/// The shortest walk from `from` to every node, and the step before each.
fn walk(grid: &Grid, cells: &[(u32, u32)], from: usize, step: f32) -> (Vec<f32>, Vec<u32>) {
    #[derive(PartialEq)]
    struct Next(f32, u32);
    impl Eq for Next {}
    impl PartialOrd for Next {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Next {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
        }
    }
    let mut dist = vec![f32::INFINITY; grid.floors.len()];
    let mut before = vec![NONE; grid.floors.len()];
    let mut heap = BinaryHeap::new();
    dist[from] = 0.0;
    #[allow(clippy::cast_possible_truncation, reason = "node counts")]
    heap.push(Next(0.0, from as u32));
    while let Some(Next(d, n)) = heap.pop() {
        let n = n as usize;
        if d > dist[n] {
            continue;
        }
        let (i, j) = cells[n];
        grid.edges(i as usize, j as usize, grid.floors[n].y, step, |m, w| {
            if d + w < dist[m] {
                dist[m] = d + w;
                #[allow(clippy::cast_possible_truncation, reason = "node counts")]
                {
                    before[m] = n as u32;
                    heap.push(Next(d + w, m as u32));
                }
            }
        });
    }
    (dist, before)
}

/// A flight of a stair, in the city's frame.
#[derive(Debug, Clone)]
struct Flight {
    leaf: u32,
    min: [f64; 2],
    max: [f64; 2],
    /// Along which frame axis it falls (0 for `x`, 1 for `z`), and which
    /// way: `+1.0` when it falls toward the axis's larger end.
    axis: usize,
    sign: f64,
    /// Its top at its high end and at its low end.
    head_y: f32,
    foot_y: f32,
}

impl Flight {
    fn width(&self) -> f64 {
        let cross = 1 - self.axis;
        self.max[cross] - self.min[cross]
    }

    /// The frame coordinate of its high end and of its low end, along its
    /// axis.
    fn ends(&self) -> (f64, f64) {
        let a = self.axis;
        if self.sign > 0.0 {
            (self.min[a], self.max[a])
        } else {
            (self.max[a], self.min[a])
        }
    }
}

/// A stair: its flights, highest first.
struct Stair {
    flights: Vec<Flight>,
    label: String,
}

/// What the audit found about one stair.
struct StairCheck {
    clear: bool,
    reached: bool,
    why: String,
}

/// The audit's result.
struct Audit {
    text: String,
    unreached: Vec<String>,
    blocked: Vec<String>,
    stairs: usize,
    /// The main stair's lowest foot and highest head, the quay's ground and
    /// the Exchange's, metres.
    main_span: Option<(f32, f32)>,
    quay_y: f32,
    exchange_y: f32,
}

/// The seed 42 city, its world and its layout.
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn city() -> (
    Arc<ProceduralWorld>,
    GameRegistries,
    strata::sim::Settlements,
) {
    let registries = shipped();
    let world = ProceduralWorld::with_params(
        strata::core::Seed(42),
        &registries,
        strata::sim::world_params(),
    )
    .with_own_water();
    world.hydrology().set_threads(4);
    let site = world.sites()[0].clone();
    world.prepare_water(site.centre[0], site.centre[1], 8_000.0);
    let path = format!(
        "{}/../../generated/{}",
        env!("CARGO_MANIFEST_DIR"),
        registries.settlements[0].layout
    );
    let layout: SettlementLayout = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(layout.ground, site.fingerprint(), "run `just content`");
    let settlements = strata::sim::Settlements(vec![strata::sim::PlacedSettlement {
        site,
        layout: Some(layout.clone()),
    }]);
    (Arc::new(world), registries, settlements)
}

fn generated() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated"))
}

/// The convex solids of a baked file's level 0, by instance and element.
fn solids(file: &str) -> Vec<(String, String, Vec<Vec3>)> {
    super::placed_proxies(&generated().join(file))
        .into_iter()
        .map(|(instance, element, vertices)| {
            (
                instance,
                element,
                vertices.iter().map(|p| p.as_vec3()).collect(),
            )
        })
        .collect()
}

/// A baked file's proxies as leaves at a world pose, all or none: the
/// physics makes one compound of them and drops the building whole if one
/// will not wrap. The degenerate files are returned for the report.
fn place_file(
    scene: &mut Scene,
    file: &str,
    at: Vec3,
    turn: Quat,
    stairs: &dyn Fn(&str, &str) -> bool,
    dropped: &mut Vec<String>,
) {
    let solids = solids(file);
    let mut made = Vec::with_capacity(solids.len());
    for (instance, element, vertices) in solids {
        let Some(collider) = Collider::convex_hull(vertices) else {
            dropped.push(format!("{file} ({instance}/{element})"));
            return;
        };
        made.push((instance, element, collider));
    }
    let stem = file
        .rsplit('/')
        .next()
        .unwrap_or(file)
        .trim_end_matches(".ashlar");
    for (instance, element, collider) in made {
        scene.sources.push(Source {
            label: format!("{stem} {instance}/{element}"),
            stair: stairs(file, &element),
        });
        let source = scene.sources.len() - 1;
        scene
            .leaves
            .extend(Leaf::parts(&collider, at, turn, source));
    }
}

/// The scene round the city: the terrain, the buildings, the trunks and the
/// roadside, over the frame rectangle `[x0, z0, x1, z1]`.
#[allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    reason = "a scenario over the real world, read in one place"
)]
fn scene(
    world: &Arc<ProceduralWorld>,
    registries: &GameRegistries,
    settlements: &strata::sim::Settlements,
    rect: [f64; 4],
    people: &[(String, (f64, f64))],
) -> (Scene, Vec<String>) {
    let settlement = &settlements.0[0];
    let site = settlement.site.clone();
    let layout = settlement.layout.as_ref().unwrap();
    let centre = site.centre;
    // The world box of the frame rectangle, on the region lattice.
    let corners = [
        [rect[0], rect[1]],
        [rect[2], rect[1]],
        [rect[0], rect[3]],
        [rect[2], rect[3]],
    ]
    .map(|c| site.to_world_xz(c));
    let (wx0, wx1) = corners
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c[0]), b.max(c[0])));
    let (wz0, wz1) = corners
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c[1]), b.max(c[1])));
    let region_m = RegionCoord::size_at(0);
    let (rx0, rx1) = (
        (wx0 / region_m).floor() as i32,
        (wx1 / region_m).floor() as i32,
    );
    let (rz0, rz1) = (
        (wz0 / region_m).floor() as i32,
        (wz1 / region_m).floor() as i32,
    );
    let coords: Vec<RegionCoord> = (rz0..=rz1)
        .flat_map(|z| (rx0..=rx1).map(move |x| RegionCoord::new(x, z, 0)))
        .collect();
    // The city's planted trees, as the loader resolves them.
    let planted: Vec<strata_flora::Planted> = layout
        .plants
        .iter()
        .filter_map(|plant| {
            let species = strata::registry::Namespaced::parse(&plant.species)
                .ok()
                .and_then(|name| registries.flora_id(&name))?;
            let at = settlement.to_world(plant.position);
            Some((species, at, plant))
        })
        .enumerate()
        .map(|(index, (species, at, plant))| strata_flora::Planted {
            species,
            at: [at.x(), at.y(), at.z()],
            yaw: plant.yaw,
            scale: plant.scale,
            variant: plant.variant,
            index: index as u32,
        })
        .collect();
    let library = strata_flora::library::TreeLibrary::read(&generated()).unwrap();
    let flora = FloraPlacement::from_registries(
        &strata::sim::Registries(Arc::new(registries.clone())),
        Some(&strata::sim::TreeLibraryRes(Some(Arc::new(library)))),
    )
    .0;
    let shapes = ShapeCache::new(flora.clone());
    let source: &dyn WorldSource = world.as_ref();
    let regions = strata::core::platform::par::map(4, &coords, |&coord| {
        let mut scratch = Vec::new();
        let field = Heightfield::sample(coord, source, &mut scratch);
        let mut trees = place_trunks(coord, &scratch, source, &flora, strata::core::Seed(42));
        let plants = strata_flora::planted_in(coord, &flora, &planted);
        trees.extend(trunks_of(coord, &plants, &flora));
        let trunks: Vec<_> = trees
            .iter()
            .filter_map(|tree| {
                let body = build_trunk(coord, tree, &shapes)?;
                let species = registries
                    .flora
                    .iter()
                    .find(|(id, _, _)| *id == tree.species)
                    .map_or_else(|| "a plant".to_owned(), |(_, name, _)| name.to_string());
                Some((body, species))
            })
            .collect();
        (field, trunks)
    });
    let n = GRID_N - 1;
    let (gx0, gz0) = (i64::from(rx0) * n as i64, i64::from(rz0) * n as i64);
    let nx = (rx1 - rx0 + 1) as usize * n + 1;
    let nz = (rz1 - rz0 + 1) as usize * n + 1;
    let mut heights = vec![0.0; nx * nz];
    let mut scene = Scene {
        site: site.clone(),
        ground: Ground {
            gx0,
            gz0,
            nx,
            nz,
            heights: Vec::new(),
            centre,
        },
        leaves: Vec::new(),
        sources: Vec::new(),
        tiles: Vec::new(),
        tile0: [0.0; 2],
        tiles_n: [0; 2],
        sea: world.sea_level(),
        body: Body::player(),
    };
    let offset = |p: strata::core::WorldPos| {
        Vec3::new(
            (p.x() - centre[0]) as f32,
            p.y() as f32,
            (p.z() - centre[1]) as f32,
        )
    };
    for (field, trunks) in regions {
        let (bx, bz) = (
            (i64::from(field.coord.x) - i64::from(rx0)) as usize * n,
            (i64::from(field.coord.z) - i64::from(rz0)) as usize * n,
        );
        for (ix, column) in field.heights.iter().enumerate() {
            for (iz, h) in column.iter().enumerate() {
                heights[(bz + iz) * nx + bx + ix] = *h;
            }
        }
        for (body, species) in trunks {
            let (at, turn) = (offset(body.center), body.rotation);
            scene.sources.push(Source {
                label: format!("a tree ({species})"),
                stair: false,
            });
            let source = scene.sources.len() - 1;
            scene
                .leaves
                .extend(Leaf::parts(&body.collider, at, turn, source));
        }
    }
    scene.ground.heights = heights;
    let stairs = |file: &str, element: &str| {
        file.ends_with("/blockout-stair.ashlar")
            || file.ends_with("/blockout-lanes.ashlar")
            || element == "grand-stair"
    };
    let mut dropped = Vec::new();
    for building in &layout.buildings {
        let at = offset(settlement.to_world(building.position));
        let turn = Quat::from_rotation_y(settlement.world_yaw(building.yaw) as f32);
        place_file(&mut scene, &building.file, at, turn, &stairs, &mut dropped);
    }
    // The roadside's and the crossings' pieces near the city.
    let files = strata::registry::files::Files::dir(generated());
    let crossings = strata::sim::crossings::Crossings::read(world.clone(), Some(&files));
    let roadside =
        strata::sim::roadside::Roadside::read(world.clone(), Some(&files), crossings.kit.clone());
    let reach = (rect[2] - rect[0]).hypot(rect[3] - rect[1]);
    let mut pieces: Vec<strata::worldgen::crossings::PlacedPiece> =
        strata::sim::roadside::near_roadside(&roadside, &[centre], reach)
            .into_iter()
            .map(|near| near.piece)
            .collect();
    for near in strata::sim::crossings::near_crossings(&crossings, &[centre], reach) {
        pieces.extend(near.pieces().iter().cloned());
    }
    let none = |_: &str, _: &str| false;
    for piece in &pieces {
        let at = offset(strata::core::WorldPos::new(
            piece.at[0],
            piece.at[1],
            piece.at[2],
        ));
        if f64::from(at.x).abs() > reach || f64::from(at.z).abs() > reach {
            continue;
        }
        let turn = Quat::from_rotation_y(piece.yaw as f32);
        place_file(&mut scene, &piece.file, at, turn, &none, &mut dropped);
    }
    // The quest people who stand in the city: capsules the player cannot
    // pass, resting on the ground where they stand.
    let capsule = CharacterController {
        height: veyra_game::PLAYER_HEIGHT_M,
        ..CharacterController::default()
    };
    for (name, at) in people {
        let (x, z) = (at.0 - centre[0], at.1 - centre[1]);
        let ground = scene.ground.at(x, z).0;
        scene.sources.push(Source {
            label: format!("{name} (a person)"),
            stair: false,
        });
        let source = scene.sources.len() - 1;
        let centre_at = Vec3::new(x as f32, ground + capsule.half_height(), z as f32);
        scene.leaves.extend(Leaf::parts(
            &capsule.collider(),
            centre_at,
            Quat::IDENTITY,
            source,
        ));
    }
    // The index over the audit's space.
    let local = corners.map(|c| [c[0] - centre[0], c[1] - centre[1]]);
    let (lx0, lx1) = local
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c[0]), b.max(c[0])));
    let (lz0, lz1) = local
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c[1]), b.max(c[1])));
    #[allow(clippy::cast_sign_loss, reason = "a positive span")]
    let tiles_n = [
        ((lx1 - lx0) / TILE_M).ceil() as usize + 1,
        ((lz1 - lz0) / TILE_M).ceil() as usize + 1,
    ];
    scene.tile0 = [lx0, lz0];
    scene.tiles_n = tiles_n;
    scene.tiles = vec![Vec::new(); tiles_n[0] * tiles_n[1]];
    scene.index();
    (scene, dropped)
}

/// Every cell's floors over the frame rectangle.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "cell counts of a few thousand"
)]
fn grid(scene: &Scene, rect: [f64; 4]) -> Grid {
    let nx = ((rect[2] - rect[0]) / CELL_M).ceil() as usize;
    let nz = ((rect[3] - rect[1]) / CELL_M).ceil() as usize;
    let mut grid = Grid {
        x0: rect[0],
        z0: rect[1],
        nx,
        nz,
        start: Vec::with_capacity(nx * nz + 1),
        floors: Vec::new(),
    };
    let rows = strata::core::platform::par::map_index(nz, 4, |j| {
        let mut counts = Vec::with_capacity(nx);
        let mut floors = Vec::new();
        for i in 0..nx {
            let [x, z] = scene.local(grid.centre(i, j));
            let before = floors.len();
            scene.floors(x, z, &mut floors);
            counts.push((floors.len() - before) as u32);
        }
        (counts, floors)
    });
    grid.start.push(0);
    for (counts, floors) in rows {
        for count in counts {
            let last = *grid.start.last().unwrap_or(&0);
            grid.start.push(last + count);
        }
        grid.floors.extend(floors);
    }
    grid
}

/// The flights the layout lays, grouped into stairs.
#[allow(
    clippy::too_many_lines,
    reason = "find each flight, then chain them, in one place"
)]
#[allow(clippy::cast_possible_truncation, reason = "metres in f32")]
fn stairs(scene: &Scene) -> Vec<Stair> {
    let mut flights = Vec::new();
    for (n, leaf) in scene.leaves.iter().enumerate() {
        if !scene.sources[leaf.source].stair {
            continue;
        }
        // Its frame box from its world box's corners is loose for a turned
        // stair; every stair runs down a frame axis, so read the hull's
        // top along both from its centre.
        let (cx, cz) = (
            f64::from(leaf.min.x + leaf.max.x) * 0.5,
            f64::from(leaf.min.z + leaf.max.z) * 0.5,
        );
        let centre = scene.frame((cx + scene.ground.centre[0], cz + scene.ground.centre[1]));
        let top = |frame: [f64; 2]| {
            let [x, z] = scene.local(frame);
            #[allow(clippy::cast_possible_truncation, reason = "leaf counts")]
            scene
                .span(n as u32, x as f32, z as f32)
                .map(|(_, hi, _)| hi)
        };
        // Its extent along each frame axis: walk out from the centre while
        // the hull is under the line.
        let mut min = centre;
        let mut max = centre;
        for axis in 0..2 {
            for sign in [-1.0, 1.0] {
                let mut d = 0.0;
                loop {
                    let mut p = centre;
                    p[axis] += sign * (d + 0.1);
                    if top(p).is_none() || d > 120.0 {
                        break;
                    }
                    d += 0.1;
                }
                if sign < 0.0 {
                    min[axis] = centre[axis] - d;
                } else {
                    max[axis] = centre[axis] + d;
                }
            }
        }
        let inset = |axis: usize, sign: f64| {
            let mut p = centre;
            p[axis] = if sign < 0.0 {
                min[axis] + 0.2
            } else {
                max[axis] - 0.2
            };
            top(p).unwrap_or(f32::NAN)
        };
        let falls = [
            (inset(0, -1.0) - inset(0, 1.0), 0),
            (inset(1, -1.0) - inset(1, 1.0), 1),
        ];
        let (fall, axis) = if falls[0].0.abs() > falls[1].0.abs() {
            falls[0]
        } else {
            falls[1]
        };
        if fall.is_nan() || fall.abs() <= 0.3 {
            continue;
        }
        let sign = if fall > 0.0 { 1.0 } else { -1.0 };
        let (head_y, foot_y) = (
            inset(axis, -sign).max(inset(axis, sign)),
            inset(axis, -sign).min(inset(axis, sign)),
        );
        #[allow(clippy::cast_possible_truncation, reason = "leaf counts")]
        flights.push(Flight {
            leaf: n as u32,
            min,
            max,
            axis,
            sign,
            head_y,
            foot_y,
        });
    }
    // Chain each flight to the one whose head its foot meets.
    let mut next: Vec<Option<usize>> = vec![None; flights.len()];
    let mut has_above = vec![false; flights.len()];
    for (a, fa) in flights.iter().enumerate() {
        for (b, fb) in flights.iter().enumerate() {
            if a == b || fa.axis != fb.axis || (fa.sign > 0.0) != (fb.sign > 0.0) {
                continue;
            }
            let cross = 1 - fa.axis;
            let overlap = fa.max[cross].min(fb.max[cross]) - fa.min[cross].max(fb.min[cross]);
            let (_, a_low) = fa.ends();
            let (b_high, _) = fb.ends();
            if (a_low - b_high).abs() < 0.4
                && overlap > 0.5 * fa.width().min(fb.width())
                && (fa.foot_y - fb.head_y).abs() < 0.35
            {
                next[a] = Some(b);
                has_above[b] = true;
            }
        }
    }
    let mut out = Vec::new();
    for first in (0..flights.len()).filter(|&f| !has_above[f]) {
        let mut chain = vec![flights[first].clone()];
        let mut at = first;
        while let Some(b) = next[at] {
            if chain.len() > flights.len() {
                break;
            }
            chain.push(flights[b].clone());
            at = b;
        }
        let label = scene.sources[scene.leaves[flights[first].leaf as usize].source]
            .label
            .clone();
        out.push(Stair {
            flights: chain,
            label,
        });
    }
    out.sort_by(|a, b| {
        let (pa, pb) = (a.flights[0].min, b.flights[0].min);
        pa[0].total_cmp(&pb[0]).then(pa[1].total_cmp(&pb[1]))
    });
    out
}

/// Whether a stair is clear from the ground before its foot to the ground
/// past its head, walking only the stair and a margin round it, and why
/// not.
#[allow(
    clippy::too_many_lines,
    reason = "the walk up the stair and the reason it fails read together"
)]
fn check_stair(
    scene: &Scene,
    grid: &Grid,
    cells: &[(u32, u32)],
    dist: &[f32],
    stair: &Stair,
) -> StairCheck {
    let first = &stair.flights[0];
    let last = &stair.flights[stair.flights.len() - 1];
    let (axis, sign) = (first.axis, first.sign);
    let cross = 1 - axis;
    let (head_at, _) = first.ends();
    let (_, foot_at) = last.ends();
    let (lo, hi) = (
        stair
            .flights
            .iter()
            .map(|f| f.min[cross])
            .fold(f64::MAX, f64::min),
        stair
            .flights
            .iter()
            .map(|f| f.max[cross])
            .fold(f64::MIN, f64::max),
    );
    let mid = f64::midpoint(lo, hi);
    let point = |along: f64, across: f64| {
        let mut p = [0.0; 2];
        p[axis] = along;
        p[cross] = across;
        p
    };
    let step = scene.body.step;
    // The ground a walker stands on before the foot and past the head: the
    // standable floors within a step of the end's tread, a metre or two out.
    let ends = |at: f64, out: f64, level: f32| {
        let mut nodes = Vec::new();
        let mut d = 0.75;
        while d <= 2.0 {
            let mut across = lo + 0.4;
            while across <= hi - 0.4 {
                if let Some((i, j)) = grid.cell(point(at + out * d, across)) {
                    for n in grid.nodes(i, j) {
                        let floor = grid.floors[n];
                        if floor.standable() && (floor.y - level).abs() <= step + 0.01 {
                            nodes.push(n);
                        }
                    }
                }
                across += CELL_M;
            }
            d += CELL_M;
        }
        nodes
    };
    let feet = ends(foot_at, sign, last.foot_y);
    let heads = ends(head_at, -sign, first.head_y);
    let reached = feet.iter().chain(&heads).any(|&n| dist[n].is_finite());
    // The walk inside the stair's own box, grown a little.
    let (amin, amax) = (head_at.min(foot_at) - 2.5, head_at.max(foot_at) + 2.5);
    let inside = |i: usize, j: usize| {
        let p = grid.centre(i, j);
        p[axis] >= amin && p[axis] <= amax && p[cross] >= lo - 1.5 && p[cross] <= hi + 1.5
    };
    let mut seen = vec![false; grid.floors.len()];
    let mut queue: std::collections::VecDeque<usize> = feet.iter().copied().collect();
    for &n in &feet {
        seen[n] = true;
    }
    let goal: BTreeSet<usize> = heads.iter().copied().collect();
    let mut clear = false;
    while let Some(n) = queue.pop_front() {
        if goal.contains(&n) {
            clear = true;
            break;
        }
        let (i, j) = cells[n];
        grid.edges(i as usize, j as usize, grid.floors[n].y, step, |m, _| {
            let (mi, mj) = cells[m];
            if !seen[m] && inside(mi as usize, mj as usize) {
                seen[m] = true;
                queue.push_back(m);
            }
        });
    }
    if clear {
        // The head must open onto ground that goes somewhere: a flood from
        // the head cells that may not use the stair itself must leave its
        // box and cover a few square metres, or the head is a pocket.
        let on_stair = |i: usize, j: usize| {
            let p = grid.centre(i, j);
            p[axis] >= head_at.min(foot_at)
                && p[axis] <= head_at.max(foot_at)
                && p[cross] >= lo
                && p[cross] <= hi
        };
        let mut seen = vec![false; grid.floors.len()];
        let mut queue: std::collections::VecDeque<usize> = heads.iter().copied().collect();
        for &n in &heads {
            seen[n] = true;
        }
        let mut beyond = 0;
        while let Some(n) = queue.pop_front() {
            let (i, j) = cells[n];
            if !inside(i as usize, j as usize) {
                beyond += 1;
                if beyond >= HEAD_OPENS_CELLS {
                    break;
                }
            }
            grid.edges(i as usize, j as usize, grid.floors[n].y, step, |m, _| {
                let (mi, mj) = cells[m];
                if !seen[m] && !on_stair(mi as usize, mj as usize) {
                    seen[m] = true;
                    queue.push_back(m);
                }
            });
        }
        let dead_end = beyond < HEAD_OPENS_CELLS;
        return StairCheck {
            clear: !dead_end,
            reached,
            why: if dead_end {
                "its head opens into a dead end".to_owned()
            } else {
                String::new()
            },
        };
    }
    // Why: up the line of the stair from the ground before its foot, the
    // first place a walker cannot stand where it should, or cannot step to.
    let expected = |along: f64| -> Option<f32> {
        let p = point(along, mid);
        let [x, z] = scene.local(p);
        let on = stair.flights.iter().find_map(|f| {
            scene
                .span(f.leaf, x as f32, z as f32)
                .map(|(_, top, _)| top)
        });
        on.or_else(|| {
            let beyond_foot = (along - foot_at) * sign > 0.0;
            let beyond_head = (along - head_at) * sign < 0.0;
            if beyond_foot {
                Some(last.foot_y)
            } else if beyond_head {
                Some(first.head_y)
            } else {
                None
            }
        })
    };
    let length = (foot_at - head_at).abs() + 3.0;
    let mut d = 0.0;
    let mut previous: Option<f32> = None;
    let mut why = "cut off inside its own box, its line clear".to_owned();
    while d <= length {
        let along = foot_at + sign * 1.5 - sign * d;
        let where_ = if (along - foot_at) * sign > 0.0 {
            "at its foot"
        } else if (along - head_at) * sign < 0.0 {
            "at its head"
        } else {
            "on its treads"
        };
        let Some(level) = expected(along) else {
            d += CELL_M;
            continue;
        };
        let Some((i, j)) = grid.cell(point(along, mid)) else {
            why = format!("{where_}: off the grid");
            break;
        };
        let floors: Vec<Floor> = grid.nodes(i, j).map(|n| grid.floors[n]).collect();
        let near = floors
            .iter()
            .filter(|f| (f.y - level).abs() <= step)
            .min_by(|a, b| (a.y - level).abs().total_cmp(&(b.y - level).abs()));
        let p = point(along, mid);
        match near {
            None => {
                let highest = floors.iter().max_by(|a, b| a.y.total_cmp(&b.y));
                let top = highest.map_or(f32::MIN, |f| f.y);
                let what = highest.map_or_else(String::new, |f| scene.surface(f.top));
                why = if top < level {
                    format!(
                        "a gap {where_} at ({:.1}, {:.1}): the top there ({what}) is {:.2} m below the tread",
                        p[0],
                        p[1],
                        level - top
                    )
                } else {
                    format!(
                        "{where_} at ({:.1}, {:.1}): {what} stands {:.2} m over the tread",
                        p[0],
                        p[1],
                        top - level
                    )
                };
                break;
            }
            Some(floor) if !floor.standable() => {
                why = format!(
                    "{where_} at ({:.1}, {:.1}): blocked by {}",
                    p[0],
                    p[1],
                    scene.blocker(floor.blocker)
                );
                break;
            }
            Some(floor) => {
                if let Some(prev) = previous
                    && (floor.y - prev).abs() > step + 0.01
                {
                    why = format!(
                        "{where_} at ({:.1}, {:.1}): a tread too tall, {:.2} m",
                        p[0],
                        p[1],
                        (floor.y - prev).abs()
                    );
                    break;
                }
                previous = Some(floor.y);
            }
        }
        d += CELL_M;
    }
    StairCheck {
        clear,
        reached,
        why,
    }
}

/// The audit over seed 42's Port Serein, its picture and its list written
/// to `scratch/combat/nav/`.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::unwrap_used,
    reason = "a scenario over the real world, read in one place"
)]
fn audit() -> Audit {
    let started = std::time::Instant::now();
    let (world, registries, settlements) = city();
    let rules = registries.quests.rules().unwrap().clone();
    let site = settlements.0[0].site.clone();
    let source = strata::sim::WorldSourceRes(world.clone() as Arc<dyn WorldSource>);
    // The places: the arrival, the Exchange, every prop of the city's bench
    // place, the training ground.
    let arrival =
        veyra_game::household::people::arrival(&settlements, world.as_ref(), &rules).unwrap();
    let ((px, pz), pyaw) = city_place(Some(&settlements)).unwrap();
    let mut places: Vec<(String, (f64, f64))> = vec![(
        "the Exchange square".to_owned(),
        veyra_game::household::people::offset((px, pz), pyaw, 0.0, rules.exchange_before_m),
    )];
    for prop in bench_place(px, pz, pyaw, CITY_SPACING_M) {
        places.push((prop.name.to_owned(), (prop.x, prop.z)));
    }
    let training = veyra_game::hostiles::default_spawn(&source, Some(&settlements)).unwrap();
    places.push(("the training ground".to_owned(), (training.0, training.2)));
    // The grid: the city's own ground and a margin, and every place.
    let def = &registries.settlements[0];
    let e = &def.extent;
    let mut rect = [
        -e.half_width_m - MARGIN_M,
        -e.uphill_m - MARGIN_M,
        e.half_width_m + MARGIN_M,
        e.downhill_m + MARGIN_M,
    ];
    for (_, at) in &places {
        let [x, z] = site.to_local_xz([at.0, at.1]);
        rect = [
            rect[0].min(x - MARGIN_M),
            rect[1].min(z - MARGIN_M),
            rect[2].max(x + MARGIN_M),
            rect[3].max(z + MARGIN_M),
        ];
    }
    let mut people = vec![("the quay's greeter".to_owned(), arrival.greeter)];
    people.push((
        "the Exchange's host".to_owned(),
        veyra_game::household::people::offset((px, pz), pyaw, 0.0, rules.exchange_before_m),
    ));
    if let Some(((bx, bz), byaw)) = board_view(Some(&settlements)) {
        people.push((
            "the board's clerk".to_owned(),
            veyra_game::household::people::offset((bx, bz), byaw, rules.board_along_m, 0.0),
        ));
    }
    let (scene, dropped) = scene(&world, &registries, &settlements, rect, &people);
    let read = started.elapsed();
    let grid = grid(&scene, rect);
    let cells = cells_of(&grid);
    let gridded = started.elapsed();
    let mut text = String::new();
    writeln!(
        text,
        "Port Serein walkability audit, seed 42: {} leaves ({} files dropped whole: {:?}), {} by {} cells of {CELL_M} m, {} floors",
        scene.leaves.len(),
        dropped.len(),
        dropped,
        grid.nx,
        grid.nz,
        grid.floors.len()
    )
    .unwrap();
    writeln!(
        text,
        "Frame to world: the frame's origin is world ({:.1}, {:.1}); looking uphill is --look {:.4}",
        site.centre[0],
        site.centre[1],
        site.yaw()
    )
    .unwrap();
    // From the arrival: the standable floor nearest the ground there.
    let start_frame = scene.frame(arrival.at);
    let (si, sj) = grid.cell(start_frame).unwrap();
    let ground_y = scene
        .ground
        .at(arrival.at.0 - site.centre[0], arrival.at.1 - site.centre[1])
        .0;
    let from = grid
        .nodes(si, sj)
        .filter(|&n| grid.floors[n].standable())
        .min_by(|&a, &b| {
            (grid.floors[a].y - ground_y)
                .abs()
                .total_cmp(&(grid.floors[b].y - ground_y).abs())
        })
        .expect("the arrival stands on a floor");
    let step = scene.body.step;
    let (dist, before) = walk(&grid, &cells, from, step);
    let walked = started.elapsed();
    // Each place: the nearest reached floor within reach, its ground the
    // highest surface under it as the props are stood.
    let reach = f64::from(REACH_M);
    let mut unreached = Vec::new();
    let mut routes: Vec<usize> = Vec::new();
    writeln!(
        text,
        "\nPlaces, from the arrival at frame ({:.1}, {:.1}):",
        start_frame[0], start_frame[1]
    )
    .unwrap();
    let mut marks: Vec<([f64; 2], bool)> = Vec::new();
    for (name, at) in &places {
        let frame = scene.frame(*at);
        let local = [at.0 - site.centre[0], at.1 - site.centre[1]];
        let terrain = f64::from(scene.ground.at(local[0], local[1]).0);
        // The prop stands where a ray from thirty metres over the terrain
        // first meets something, as `props::ground_at` stands it; a person
        // is not a surface the prop rests on.
        let (stood, on) = scene
            .over(local[0], local[1])
            .filter(|&n| !scene.surface(n).ends_with("(a person)"))
            .filter_map(|n| {
                scene
                    .span(n, local[0] as f32, local[1] as f32)
                    .map(|(_, hi, _)| (f64::from(hi), n))
            })
            .filter(|&(hi, _)| hi <= terrain + 30.0)
            .fold((terrain, GROUND), |a, b| if b.0 > a.0 { b } else { a });
        let on = scene.surface(on);
        let mut best: Option<(f32, usize)> = None;
        let cells_reach = (reach / CELL_M).ceil() as i64;
        if let Some((ci, cj)) = grid.cell(frame) {
            for dj in -cells_reach..=cells_reach {
                for di in -cells_reach..=cells_reach {
                    let (i, j) = (ci as i64 + di, cj as i64 + dj);
                    if i < 0 || j < 0 || i as usize >= grid.nx || j as usize >= grid.nz {
                        continue;
                    }
                    let c = grid.centre(i as usize, j as usize);
                    if (c[0] - frame[0]).hypot(c[1] - frame[1]) > reach {
                        continue;
                    }
                    for n in grid.nodes(i as usize, j as usize) {
                        if dist[n].is_finite()
                            && (f64::from(grid.floors[n].y) - stood).abs() <= 1.0
                            && best.is_none_or(|(d, _)| dist[n] < d)
                        {
                            best = Some((dist[n], n));
                        }
                    }
                }
            }
        }
        let straight = (frame[0] - start_frame[0]).hypot(frame[1] - start_frame[1]);
        if let Some((d, n)) = best {
            writeln!(
                    text,
                    "  reached  {name:<22} walked {d:7.1} m (straight {straight:6.1} m) at frame ({:.1}, {:.1}), ground {stood:.2} on {on}",
                    frame[0], frame[1]
                )
                .unwrap();
            if routes.is_empty() {
                // The way to the Exchange, by what is walked on.
                let mut way = Vec::new();
                let mut m = n;
                while m != from && before[m] != NONE {
                    way.push(m);
                    m = before[m] as usize;
                }
                way.reverse();
                let mut last = (u32::MAX, f32::MIN);
                for &m in &way {
                    let floor = grid.floors[m];
                    let (i, j) = cells[m];
                    if floor.top != last.0 || (floor.y - last.1).abs() > 2.0 {
                        let c = grid.centre(i as usize, j as usize);
                        writeln!(
                            text,
                            "    via ({:.1}, {:.1}) at {:.2} m: {}",
                            c[0],
                            c[1],
                            floor.y,
                            scene.surface(floor.top)
                        )
                        .unwrap();
                        last = (floor.top, floor.y);
                    }
                }
            }
            routes.push(n);
        } else {
            writeln!(
                    text,
                    "  NOT      {name:<22} straight {straight:6.1} m at frame ({:.1}, {:.1}), ground {stood:.2} on {on}",
                    frame[0], frame[1]
                )
                .unwrap();
            unreached.push(name.clone());
        }
        marks.push((frame, best.is_some()));
    }
    // Each stair.
    let stairs = stairs(&scene);
    let mut blocked = Vec::new();
    let mut outlines: Vec<([f64; 2], [f64; 2], bool)> = Vec::new();
    writeln!(text, "\nStairs ({}), foot to head:", stairs.len()).unwrap();
    for stair in &stairs {
        let check = check_stair(&scene, &grid, &cells, &dist, stair);
        let first = &stair.flights[0];
        let last = &stair.flights[stair.flights.len() - 1];
        let (_, foot) = last.ends();
        let cross = 1 - first.axis;
        let centre_cross = f64::midpoint(first.min[cross], first.max[cross]);
        let mut at = [0.0; 2];
        at[first.axis] = foot;
        at[cross] = centre_cross;
        let world_at = site.to_world_xz(at);
        let name = format!(
            "{} ({} flights, foot at frame ({:.1}, {:.1}), world ({:.1}, {:.1}), {:.1} to {:.1} m)",
            stair.label,
            stair.flights.len(),
            at[0],
            at[1],
            world_at[0],
            world_at[1],
            last.foot_y,
            first.head_y
        );
        writeln!(
            text,
            "  {} {}{}{}",
            if check.clear { "clear  " } else { "BLOCKED" },
            name,
            if check.reached {
                ""
            } else {
                ", not reached from the arrival"
            },
            if check.clear {
                String::new()
            } else {
                format!(": {}", check.why)
            }
        )
        .unwrap();
        if !check.clear {
            blocked.push(format!("{name}: {}", check.why));
        }
        for flight in &stair.flights {
            outlines.push((flight.min, flight.max, check.clear));
        }
    }
    // Cells by what a walker meets there. Ground cut off (the terrain or a
    // stair no walk reaches) is what matters; a roof cut off is a roof.
    let mut cut_off = 0usize;
    let mut reached_cells = 0usize;
    let (w, h) = (grid.nx, grid.nz);
    let mut pixels = vec![0u8; w * h * 4];
    let mut lost = vec![false; w * h];
    let (low, high) = grid
        .floors
        .iter()
        .filter(|f| f.standable())
        .fold((f32::MAX, f32::MIN), |(a, b), f| (a.min(f.y), b.max(f.y)));
    let walked_on =
        |top: u32| top == GROUND || scene.sources[scene.leaves[top as usize].source].stair;
    for j in 0..h {
        for i in 0..w {
            let nodes = grid.nodes(i, j);
            let highest = |keep: &dyn Fn(usize) -> bool| {
                nodes
                    .clone()
                    .filter(|&n| keep(n))
                    .max_by(|&a, &b| grid.floors[a].y.total_cmp(&grid.floors[b].y))
            };
            let reach_floor = highest(&|n| dist[n].is_finite());
            let stand_floor = highest(&|n| grid.floors[n].standable());
            let water = nodes.clone().any(|n| grid.floors[n].blocker == WATER);
            let shade = |n: usize| 0.45 + 0.55 * ((grid.floors[n].y - low) / (high - low).max(1.0));
            let colour = if let Some(n) = reach_floor {
                reached_cells += 1;
                let s = shade(n);
                [0.25 * s, 0.85 * s, 0.3 * s]
            } else if let Some(n) = stand_floor {
                let s = shade(n);
                if walked_on(grid.floors[n].top) {
                    cut_off += 1;
                    lost[j * w + i] = true;
                    [1.0 * s, 0.55 * s, 0.1 * s]
                } else {
                    [0.5 * s, 0.38 * s, 0.3 * s]
                }
            } else if water {
                [0.08, 0.15, 0.35]
            } else {
                [0.16, 0.16, 0.18]
            };
            let p = (j * w + i) * 4;
            for k in 0..3 {
                pixels[p + k] = (colour[k].clamp(0.0, 1.0) * 255.0) as u8;
            }
            pixels[p + 3] = 255;
        }
    }
    // The pieces of ground cut off, largest first.
    let mut pieces = Vec::new();
    let mut seen = vec![false; w * h];
    for start in 0..w * h {
        if !lost[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let (mut count, mut sx, mut sz) = (0usize, 0.0, 0.0);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        while let Some(c) = stack.pop() {
            let (i, j) = (c % w, c / w);
            let p = grid.centre(i, j);
            count += 1;
            sx += p[0];
            sz += p[1];
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (ni, nj) = (i as i64 + di, j as i64 + dj);
                if ni < 0 || nj < 0 || ni as usize >= w || nj as usize >= h {
                    continue;
                }
                let m = nj as usize * w + ni as usize;
                if lost[m] && !seen[m] {
                    seen[m] = true;
                    stack.push(m);
                }
            }
        }
        if count >= 100 {
            let n = count as f64;
            pieces.push((count, [sx / n, sz / n], lo, hi));
        }
    }
    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.0));
    writeln!(
        text,
        "\nGround cut off, pieces of 25 m2 or more ({}):",
        pieces.len()
    )
    .unwrap();
    for (count, centre, lo, hi) in pieces.iter().take(40) {
        writeln!(
            text,
            "  {:7.1} m2 round ({:.1}, {:.1}), x {:.1} to {:.1}, z {:.1} to {:.1}",
            *count as f64 * CELL_M * CELL_M,
            centre[0],
            centre[1],
            lo[0],
            hi[0],
            lo[1],
            hi[1]
        )
        .unwrap();
    }
    let mut put = |x: i64, z: i64, rgb: [u8; 3]| {
        if x >= 0 && z >= 0 && (x as usize) < w && (z as usize) < h {
            let p = (z as usize * w + x as usize) * 4;
            pixels[p..p + 3].copy_from_slice(&rgb);
        }
    };
    // The walked routes to every reached place.
    for &end in &routes {
        let mut n = end;
        while n != from && before[n] != NONE {
            let (i, j) = cells[n];
            put(i64::from(i), i64::from(j), [255, 230, 40]);
            n = before[n] as usize;
        }
    }
    let to_pixel = |p: [f64; 2]| {
        (
            ((p[0] - grid.x0) / CELL_M).floor() as i64,
            ((p[1] - grid.z0) / CELL_M).floor() as i64,
        )
    };
    for (min, max, clear) in &outlines {
        let colour = if *clear {
            [40, 220, 240]
        } else {
            [255, 30, 30]
        };
        let (a, b) = (to_pixel(*min), to_pixel(*max));
        for x in a.0..=b.0 {
            for t in 0..2 {
                put(x, a.1 + t, colour);
                put(x, b.1 - t, colour);
            }
        }
        for z in a.1..=b.1 {
            for t in 0..2 {
                put(a.0 + t, z, colour);
                put(b.0 - t, z, colour);
            }
        }
    }
    for (at, reached) in &marks {
        let (x, z) = to_pixel(*at);
        let colour = if *reached {
            [255, 40, 255]
        } else {
            [255, 0, 0]
        };
        for dz in -3..=3 {
            for dx in -3..=3 {
                put(x + dx, z + dz, colour);
            }
        }
    }
    let (sx, sz) = to_pixel(start_frame);
    for dz in -4..=4 {
        for dx in -4..=4 {
            put(sx + dx, sz + dz, [255, 255, 255]);
        }
    }
    writeln!(
        text,
        "\nCells: {reached_cells} reached, {cut_off} of ground standable but cut off. Read {read:.1?}, gridded {:.1?}, walked {:.1?}, in all {:.1?}.",
        gridded.saturating_sub(read),
        walked.saturating_sub(gridded),
        started.elapsed()
    )
    .unwrap();
    let out = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scratch/combat/nav"
    ));
    std::fs::create_dir_all(&out).unwrap();
    image::RgbaImage::from_raw(w as u32, h as u32, pixels)
        .expect("a picture of the audit's size")
        .save(out.join("audit.png"))
        .unwrap();
    std::fs::write(out.join("audit.txt"), &text).unwrap();
    let main = stairs
        .iter()
        .filter(|stair| stair.label.starts_with("blockout-stair "))
        .flat_map(|stair| &stair.flights);
    let main_span = main.fold(None, |span: Option<(f32, f32)>, f| {
        Some(span.map_or((f.foot_y, f.head_y), |(lo, hi)| {
            (lo.min(f.foot_y), hi.max(f.head_y))
        }))
    });
    let exchange = site.to_local_xz(places[0].1.into());
    Audit {
        text,
        unreached,
        blocked,
        stairs: stairs.len(),
        main_span,
        quay_y: ground_y,
        exchange_y: scene.ground.at(exchange[0], exchange[1]).0,
    }
}

/// Ticket 0015, step 2: from the quay a character walks to the Exchange,
/// every prop of the city's bench place and the training ground, and every
/// stair the generator lays is clear from its foot to its head.
#[test]
fn port_serein_is_walked_from_the_quay() {
    let audit = audit();
    println!("{}", audit.text);
    assert!(
        audit.unreached.is_empty() && audit.blocked.is_empty(),
        "not reached: {:?}\nblocked stairs: {:#?}",
        audit.unreached,
        audit.blocked
    );
    assert!(
        audit.stairs >= STAIRS_AT_LEAST,
        "the audit found {} stairs, fewer than {STAIRS_AT_LEAST}: the stairs' names changed",
        audit.stairs
    );
    let (foot, head) = audit.main_span.expect("the main stair was found");
    let step = CharacterController::default().step_height;
    assert!(
        (foot - audit.quay_y).abs() <= step * 2.0 && (head - audit.exchange_y).abs() <= step * 2.0,
        "the main stair runs {foot:.1} to {head:.1} m, not from the quay ({:.1}) to the Exchange ({:.1})",
        audit.quay_y,
        audit.exchange_y
    );
}
