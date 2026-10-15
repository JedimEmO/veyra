//! The crown as masses: what the stylised shading reads.
//!
//! The nearest level's cards are gathered into a few clumps (k-means), and
//! the crown becomes a smooth field of one soft blob per clump over a
//! broad blob for the whole crown. The field's gradient is the envelope
//! normal the cards shade with, so a crown lights as a few rounded masses;
//! its divergence is the curvature, positive on a mass and negative in the
//! crease between two. Crown depth comes from the cards' outer hull, and
//! ambient occlusion from a few rays through a coarse occupancy grid.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::DVec3;

use crate::foliage::Card;

#[derive(Clone, Debug)]
pub(crate) struct CrownField {
    clumps: Vec<(DVec3, f64)>,
    centre: DVec3,
    /// A conifer's broad blob is a column around the trunk, not a ball.
    column: bool,
    spread: DVec3,
    scale: f64,
    hull: Hull,
}

const GLOBAL_WEIGHT: f64 = 0.45;

impl CrownField {
    pub(crate) fn new(cards: &[Card], centre: DVec3, column: bool, clumps: u32) -> Self {
        let points: Vec<DVec3> = cards.iter().map(Card::centre).collect();
        let centres = kmeans(&points, clumps.max(1) as usize, centre);
        // A clump's spread is the RMS distance of its members.
        let mut sums = vec![(0.0_f64, 0.0_f64); centres.len()];
        for p in &points {
            let k = nearest(&centres, *p);
            sums[k].0 += p.distance_squared(centres[k]);
            sums[k].1 += 1.0;
        }
        let (min, max) = points.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(a, b), p| (a.min(*p), b.max(*p)),
        );
        let extent = if points.is_empty() {
            DVec3::ONE
        } else {
            ((max - min) * 0.5).max(DVec3::splat(0.2))
        };
        let floor = extent.length() * 0.12;
        let clumps: Vec<(DVec3, f64)> = centres
            .iter()
            .zip(&sums)
            .filter(|(_, (_, n))| *n > 0.0)
            .map(|(c, (s, n))| (*c, ((s / n).sqrt() * 1.1).max(floor)))
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let scale = if clumps.is_empty() {
            1.0
        } else {
            clumps.iter().map(|c| c.1).sum::<f64>() / clumps.len() as f64
        };
        let hull = Hull::new(cards, centre);
        Self {
            clumps,
            centre,
            column,
            spread: extent,
            scale,
            hull,
        }
    }

    /// Minus the field's gradient: out of the masses.
    fn outward_gradient(&self, p: DVec3) -> DVec3 {
        let mut g = DVec3::ZERO;
        for &(c, sigma) in &self.clumps {
            let d = p - c;
            let s2 = sigma * sigma;
            g += d / s2 * (-d.length_squared() / (2.0 * s2)).exp();
        }
        let mut d = p - self.centre;
        if self.column {
            d.y *= 0.35;
        }
        let s = self.spread;
        let q = DVec3::new(d.x / s.x, d.y / s.y, d.z / s.z);
        let w = (-q.length_squared() * 0.5).exp() * GLOBAL_WEIGHT;
        g + DVec3::new(q.x / s.x, q.y / s.y, q.z / s.z) * w * self.scale
    }

    /// The envelope normal at a point.
    pub(crate) fn normal(&self, p: DVec3) -> DVec3 {
        let g = self.outward_gradient(p);
        if g.length_squared() > 1e-18 {
            g.normalize()
        } else {
            (p - self.centre).normalize_or(DVec3::Y)
        }
    }

    /// The divergence of the normal field against that of a smooth ball the
    /// size of the crown: positive on a mass that bulges out of the crown,
    /// negative in the crease between two, scaled so a clump's crown is
    /// about a half.
    pub(crate) fn curvature(&self, p: DVec3) -> f64 {
        let h = self.scale * 0.15;
        let mut div = 0.0;
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            div += (self.normal(p + axis * h) - self.normal(p - axis * h)).dot(axis) / (2.0 * h);
        }
        let ball = 2.0 / ((self.spread.x + self.spread.y + self.spread.z) / 3.0);
        ((div - ball) * self.scale * 0.5).clamp(-1.0, 1.0)
    }

    /// 0 on the crown's outer surface, 1 deep inside.
    pub(crate) fn depth(&self, p: DVec3) -> f64 {
        self.hull.depth(p)
    }
}

fn nearest(centres: &[DVec3], p: DVec3) -> usize {
    let mut best = (f64::INFINITY, 0);
    for (k, c) in centres.iter().enumerate() {
        let d = c.distance_squared(p);
        if d < best.0 {
            best = (d, k);
        }
    }
    best.1
}

/// Lloyd's k-means from a farthest-point start, which is deterministic and
/// spreads the first centres over the crown.
fn kmeans(points: &[DVec3], k: usize, start: DVec3) -> Vec<DVec3> {
    if points.is_empty() {
        return vec![start];
    }
    let k = k.min(points.len());
    let mut centres = Vec::with_capacity(k);
    let first = points
        .iter()
        .copied()
        .max_by(|a, b| {
            a.distance_squared(start)
                .total_cmp(&b.distance_squared(start))
        })
        .expect("points");
    centres.push(first);
    while centres.len() < k {
        let far = points
            .iter()
            .copied()
            .max_by(|a, b| {
                let da = centres[nearest(&centres, *a)].distance_squared(*a);
                let db = centres[nearest(&centres, *b)].distance_squared(*b);
                da.total_cmp(&db)
            })
            .expect("points");
        centres.push(far);
    }
    for _ in 0..16 {
        let mut sums = vec![(DVec3::ZERO, 0.0_f64); k];
        for p in points {
            let n = nearest(&centres, *p);
            sums[n].0 += *p;
            sums[n].1 += 1.0;
        }
        for (c, (sum, n)) in centres.iter_mut().zip(sums) {
            if n > 0.0 {
                *c = sum / n;
            }
        }
    }
    centres
}

const AZIMUTHS: usize = 24;
const ELEVATIONS: usize = 12;

/// The cards' outer surface as a radius per direction from the crown's
/// middle.
#[derive(Clone, Debug)]
struct Hull {
    centre: DVec3,
    radius: Vec<f64>,
}

impl Hull {
    fn bin(d: DVec3) -> (f64, f64) {
        let azimuth = (d.z.atan2(d.x) / core::f64::consts::TAU + 1.0).fract();
        let elevation = (d.y.clamp(-1.0, 1.0).asin() / core::f64::consts::PI + 0.5).clamp(0.0, 1.0);
        #[allow(clippy::cast_precision_loss)]
        (
            azimuth * AZIMUTHS as f64,
            elevation * (ELEVATIONS - 1) as f64,
        )
    }

    fn new(cards: &[Card], centre: DVec3) -> Self {
        let mut radius = vec![0.0_f64; AZIMUTHS * ELEVATIONS];
        for card in cards {
            for p in [
                card.centre(),
                card.at(0.5, 0.0),
                card.at(0.0, 0.5),
                card.at(1.0, 0.5),
            ] {
                let d = p - centre;
                let len = d.length();
                if len < 1e-6 {
                    continue;
                }
                let (a, e) = Self::bin(d / len);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let index = (e.round() as usize) * AZIMUTHS + (a as usize) % AZIMUTHS;
                radius[index] = radius[index].max(len);
            }
        }
        // Close small holes so a gap between two cards is not a hole in the
        // hull.
        for _ in 0..2 {
            let before = radius.clone();
            for e in 0..ELEVATIONS {
                for a in 0..AZIMUTHS {
                    let mut best: f64 = 0.0;
                    for (de, da) in [(0, 1), (0, AZIMUTHS - 1), (1, 0), (usize::MAX, 0)] {
                        let ne = e.wrapping_add(de);
                        if ne >= ELEVATIONS {
                            continue;
                        }
                        best = best.max(before[ne * AZIMUTHS + (a + da) % AZIMUTHS]);
                    }
                    let here = &mut radius[e * AZIMUTHS + a];
                    *here = here.max(best * 0.8);
                }
            }
        }
        Self { centre, radius }
    }

    fn radius_towards(&self, d: DVec3) -> f64 {
        let (a, e) = Self::bin(d);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (a0, e0) = (a.floor() as usize, e.floor() as usize);
        let (fa, fe) = (a - a.floor(), e - e.floor());
        let e1 = (e0 + 1).min(ELEVATIONS - 1);
        let at = |e: usize, a: usize| self.radius[e * AZIMUTHS + a % AZIMUTHS];
        let low = at(e0, a0) * (1.0 - fa) + at(e0, a0 + 1) * fa;
        let high = at(e1, a0) * (1.0 - fa) + at(e1, a0 + 1) * fa;
        low * (1.0 - fe) + high * fe
    }

    fn depth(&self, p: DVec3) -> f64 {
        let d = p - self.centre;
        let len = d.length();
        if len < 1e-6 {
            return 1.0;
        }
        let r = self.radius_towards(d / len);
        if r <= 1e-6 {
            return 0.0;
        }
        ((1.0 - len / r) / 0.6).clamp(0.0, 1.0)
    }
}

/// How much leaf and wood fills each cell of a coarse grid, as an
/// extinction per metre for the ambient occlusion rays.
pub(crate) struct Occupancy {
    origin: DVec3,
    cell: f64,
    dims: [usize; 3],
    density: Vec<f64>,
}

/// What fraction of a card's area blocks the sky: the painted cluster's
/// coverage.
const LEAF_COVER: f64 = 0.55;

impl Occupancy {
    pub(crate) fn new(min: DVec3, max: DVec3, cell: f64) -> Self {
        let size = max - min;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let dim = |span: f64| ((span / cell).ceil() as usize + 2).max(1);
        let dims = [dim(size.x), dim(size.y), dim(size.z)];
        Self {
            origin: min - DVec3::splat(cell),
            cell,
            dims,
            density: vec![0.0; dims[0] * dims[1] * dims[2]],
        }
    }

    fn index(&self, p: DVec3) -> Option<usize> {
        let q = (p - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 || q.z < 0.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x, y, z) = (q.x as usize, q.y as usize, q.z as usize);
        (x < self.dims[0] && y < self.dims[1] && z < self.dims[2])
            .then(|| (z * self.dims[1] + y) * self.dims[0] + x)
    }

    /// Spread an area of leaf (square metres) at a point.
    pub(crate) fn add_area(&mut self, p: DVec3, area: f64) {
        let volume = self.cell.powi(3);
        if let Some(i) = self.index(p) {
            // A layer of leaf blocks `LEAF_COVER` of the light crossing it;
            // as extinction per metre in the cell that is area over volume.
            self.density[i] += area * LEAF_COVER / volume;
        }
    }

    /// Fill a cell with solid wood.
    pub(crate) fn add_solid(&mut self, p: DVec3, volume: f64) {
        if let Some(i) = self.index(p) {
            self.density[i] += volume / self.cell.powi(3) * 4.0 / self.cell;
        }
    }

    /// The share of open sky seen from `p` facing `n`: rays over the upper
    /// hemisphere and the horizon, weighted by how much they face `n`.
    pub(crate) fn ambient(&self, p: DVec3, n: DVec3) -> f64 {
        let mut open = 0.0;
        let mut total = 0.0;
        let start = p + n * self.cell * 0.5;
        for dir in RAYS {
            let dir = DVec3::from_array(dir);
            let w = (dir.dot(n) * 0.5 + 0.5).powi(2) + 0.05;
            let mut optical = 0.0;
            let mut t = self.cell * 0.5;
            loop {
                let q = start + dir * t;
                let Some(i) = self.index(q) else { break };
                optical += self.density[i] * self.cell * 0.5;
                if optical > 8.0 {
                    break;
                }
                t += self.cell * 0.5;
            }
            open += w * (-optical).exp();
            total += w;
        }
        open / total
    }
}

/// Sixteen directions over the sky and the horizon, unit length.
const RAYS: [[f64; 3]; 16] = [
    [0.0, 1.0, 0.0],
    [0.707, 0.707, 0.0],
    [-0.707, 0.707, 0.0],
    [0.0, 0.707, 0.707],
    [0.0, 0.707, -0.707],
    [0.5, 0.707, 0.5],
    [-0.5, 0.707, -0.5],
    [0.966, 0.259, 0.0],
    [-0.966, 0.259, 0.0],
    [0.0, 0.259, 0.966],
    [0.0, 0.259, -0.966],
    [0.683, 0.259, 0.683],
    [-0.683, 0.259, 0.683],
    [0.683, 0.259, -0.683],
    [-0.683, 0.259, -0.683],
    [-0.5, 0.707, 0.5],
];
