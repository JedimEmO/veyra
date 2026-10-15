//! Convex hulls: choosing a collider's few dozen points, and the hull itself
//! for measuring how well it fits.
//!
//! A collider carries points only; the physics engine builds its own hull
//! from them. The faces here are for the tests, which check that the hull
//! holds the visible rock, and for the review overlay, which draws it.
use ashlar::glam::DVec3;

/// Most points a collider carries.
pub(crate) const MAX_POINTS: usize = 64;

/// Directions the first points are chosen along, before refinement: the
/// rock's extremes along a coarse even spread.
const SEED_DIRECTIONS: usize = 14;

/// At most [`MAX_POINTS`] of `points` whose hull wraps them all closely: the
/// extremes along a few directions, then, one at a time, whichever point
/// stands furthest outside the hull of those chosen so far. Each addition
/// takes off the worst miss, so the few dozen points go where the surface
/// bulges rather than being spread evenly over flat faces.
pub(crate) fn hull_points(points: &[DVec3]) -> Vec<DVec3> {
    let mut chosen = support_points(points, SEED_DIRECTIONS);
    // The hull only grows, so a point once inside stays inside and is
    // never measured again.
    let mut outside: Vec<DVec3> = points.to_vec();
    while chosen.len() < MAX_POINTS {
        let Some(hull) = Hull::new(&chosen) else {
            break;
        };
        let misses: Vec<(f64, DVec3)> = outside
            .iter()
            .map(|&p| (hull.outside(p), p))
            .filter(|(miss, _)| *miss > 1e-3)
            .collect();
        let worst = misses
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.0.cmp(&a.0)));
        let Some((_, &(_, point))) = worst else {
            break;
        };
        chosen.push(point);
        outside = misses.into_iter().map(|(_, p)| p).collect();
    }
    chosen
}

/// The hull of just enough of `points` to judge how tightly a split wraps
/// them, far cheaper than [`hull_points`].
pub(crate) fn rough_hull(points: &[DVec3]) -> Option<Hull> {
    Hull::new(&support_points(points, 32))
}

/// The vertices of `points` furthest along each of `count` directions
/// spread evenly over the sphere, deduplicated, in first-found order.
fn support_points(points: &[DVec3], count: usize) -> Vec<DVec3> {
    let mut chosen: Vec<usize> = Vec::new();
    // A Fibonacci sphere: even coverage with no clustering at the poles.
    let golden = core::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    for i in 0..count {
        #[allow(clippy::cast_precision_loss, reason = "a few directions")]
        let (i, n) = (i as f64, count as f64);
        let y = 1.0 - 2.0 * (i + 0.5) / n;
        let flat = (1.0 - y * y).sqrt();
        let dir = DVec3::new((golden * i).cos() * flat, y, (golden * i).sin() * flat);
        let best = points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.dot(dir).total_cmp(&b.1.dot(dir)).then(b.0.cmp(&a.0)))
            .map(|(index, _)| index);
        if let Some(best) = best
            && !chosen.contains(&best)
        {
            chosen.push(best);
        }
    }
    chosen.into_iter().map(|i| points[i]).collect()
}

/// A convex hull as outward-wound triangles over its points.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    /// The points it was built from; some may lie inside.
    pub points: Vec<DVec3>,
    /// Outward-wound triangles, indices into `points`.
    pub faces: Vec<[usize; 3]>,
}

impl Hull {
    /// The hull of `points` by incremental construction, or `None` when
    /// they are fewer than four or all lie in one plane.
    #[must_use]
    pub fn new(points: &[DVec3]) -> Option<Self> {
        if points.len() < 4 {
            return None;
        }
        let (min, max) = points.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(a, b), p| (a.min(*p), b.max(*p)),
        );
        let eps = (max - min).length() * 1e-9;
        // A starting tetrahedron from spread-out points.
        let a = (0..points.len()).min_by(|&i, &j| points[i].x.total_cmp(&points[j].x))?;
        let b = farthest(points, |p| p.distance(points[a]))?;
        let line = (points[b] - points[a]).normalize_or_zero();
        let c = farthest(points, |p| {
            let v = p - points[a];
            (v - line * v.dot(line)).length()
        })?;
        let plane = (points[b] - points[a])
            .cross(points[c] - points[a])
            .normalize_or_zero();
        let d = farthest(points, |p| (p - points[a]).dot(plane).abs())?;
        if (points[d] - points[a]).dot(plane).abs() <= eps || plane == DVec3::ZERO {
            return None;
        }
        let mut faces = vec![[a, b, c], [a, c, d], [a, d, b], [b, d, c]];
        if (points[d] - points[a]).dot(plane) > 0.0 {
            for face in &mut faces {
                face.swap(1, 2);
            }
        }
        let mut hull = Self {
            points: points.to_vec(),
            faces,
        };
        for p in 0..points.len() {
            if [a, b, c, d].contains(&p) {
                continue;
            }
            hull.add(p, eps);
        }
        Some(hull)
    }

    fn plane(&self, face: [usize; 3]) -> (DVec3, f64) {
        let [a, b, c] = face.map(|i| self.points[i]);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        (normal, normal.dot(a))
    }

    fn add(&mut self, p: usize, eps: f64) {
        let point = self.points[p];
        let visible: Vec<bool> = self
            .faces
            .iter()
            .map(|&f| {
                let (normal, offset) = self.plane(f);
                normal.dot(point) - offset > eps
            })
            .collect();
        if !visible.contains(&true) {
            return;
        }
        let edges: Vec<(usize, usize)> = self
            .faces
            .iter()
            .zip(&visible)
            .filter(|(_, v)| **v)
            .flat_map(|(f, _)| [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])])
            .collect();
        let horizon: Vec<(usize, usize)> = edges
            .iter()
            .copied()
            .filter(|&(u, v)| !edges.contains(&(v, u)))
            .collect();
        let mut kept: Vec<[usize; 3]> = self
            .faces
            .iter()
            .zip(&visible)
            .filter(|(_, v)| !**v)
            .map(|(f, _)| *f)
            .collect();
        kept.extend(horizon.into_iter().map(|(u, v)| [u, v, p]));
        self.faces = kept;
    }

    /// How far `p` is outside the hull, as the largest distance past any
    /// face's plane: zero or less inside.
    #[must_use]
    pub fn outside(&self, p: DVec3) -> f64 {
        self.faces
            .iter()
            .map(|&f| {
                let (normal, offset) = self.plane(f);
                normal.dot(p) - offset
            })
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// The enclosed volume, cubic metres.
    #[must_use]
    pub fn volume(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| {
                let [a, b, c] = f.map(|i| self.points[i]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// Every edge once, as pairs of points: what an overlay draws.
    #[must_use]
    pub fn edges(&self) -> Vec<[DVec3; 2]> {
        let mut pairs: Vec<(usize, usize)> = self
            .faces
            .iter()
            .flat_map(|f| [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])])
            .map(|(u, v)| (u.min(v), u.max(v)))
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        pairs
            .into_iter()
            .map(|(u, v)| [self.points[u], self.points[v]])
            .collect()
    }
}

fn farthest(points: &[DVec3], measure: impl Fn(DVec3) -> f64) -> Option<usize> {
    (0..points.len()).max_by(|&i, &j| measure(points[i]).total_cmp(&measure(points[j])))
}
