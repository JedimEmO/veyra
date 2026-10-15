//! A rock's signed distance field: rounded blocks cut by fracture planes,
//! joined, optionally quantised into strata, warped and cut off at an
//! irregular bottom.
//!
//! Plain data rather than composed closures, so a shape can be read in a
//! debugger and moved by changing one field; [`Rock::distance`] is the whole
//! evaluation. The distance is exact only for a lone rounded box; the cuts,
//! the strata and the warp keep it a bound close enough to one for surface
//! nets, Newton steps and ambient occlusion, which is all it feeds.
use ashlar::glam::{DMat3, DVec2, DVec3};

use crate::noise::Noise;

/// A half-space cut: everything in front of the plane is broken away.
#[derive(Clone, Debug)]
pub(crate) struct Cut {
    /// Out of the rock, unit length.
    pub normal: DVec3,
    /// The plane is `normal . p = offset`.
    pub offset: f64,
    /// Radius of the bevel where the plane meets the rest of the block.
    pub soft: f64,
}

/// A rounded box, turned and tilted, with its own fracture planes.
#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub centre: DVec3,
    pub half: DVec3,
    /// Edge rounding, at most the smallest half extent.
    pub round: f64,
    /// The block's axes in the rock's frame, as columns.
    pub axes: DMat3,
    pub cuts: Vec<Cut>,
}

impl Block {
    /// A block turned `yaw` radians about `+Y`.
    pub(crate) fn new(centre: DVec3, half: DVec3, round: f64, yaw: f64) -> Self {
        Self {
            centre,
            half,
            round: round.min(half.min_element() * 0.98),
            axes: DMat3::from_rotation_y(yaw),
            cuts: Vec::new(),
        }
    }

    /// Lean the block `angle` radians about the horizontal axis at
    /// `azimuth`: a boulder that settled at a slant.
    pub(crate) fn tilt(&mut self, azimuth: f64, angle: f64) {
        let axis = DVec3::new(azimuth.cos(), 0.0, -azimuth.sin());
        self.axes = DMat3::from_axis_angle(axis, angle) * self.axes;
    }

    fn to_local(&self, v: DVec3) -> DVec3 {
        self.axes.transpose() * v
    }

    fn distance(&self, p: DVec3) -> f64 {
        let q = self.to_local(p - self.centre).abs() - (self.half - DVec3::splat(self.round));
        let mut d = q.max(DVec3::ZERO).length() + q.max_element().min(0.0) - self.round;
        for cut in &self.cuts {
            d = smooth_max(d, cut.normal.dot(p) - cut.offset, cut.soft);
        }
        d
    }

    /// How far the uncut block reaches along unit direction `dir`, from the
    /// rock's origin: where a cut plane has to sit to take a slice off.
    pub(crate) fn support(&self, dir: DVec3) -> f64 {
        let local = self.to_local(dir).abs();
        let inner = self.half - DVec3::splat(self.round);
        self.centre.dot(dir) + local.dot(inner) + self.round
    }

    /// The box around the uncut block.
    fn bounds(&self) -> [DVec3; 2] {
        let extent = DVec3::new(
            self.support(DVec3::X) - self.centre.x,
            self.support(DVec3::Y) - self.centre.y,
            self.support(DVec3::Z) - self.centre.z,
        );
        [self.centre - extent, self.centre + extent]
    }
}

/// One stratum's own offsets from the rock's body.
#[derive(Clone, Debug)]
pub(crate) struct Layer {
    /// Horizontal shift, metres: the layers do not stack plumb.
    pub shift: DVec2,
    /// How far the layer's face stands back from the body.
    pub inset: f64,
    /// How much further the face stands back at the layer's foot, which is
    /// what undercuts the ledge above it.
    pub undercut: f64,
    /// Rounding of the layer's edges.
    pub round: f64,
}

/// Horizontal strata the body is quantised into: each layer is the body's
/// cross-section, shifted, inset and undercut, then extruded through the
/// layer with a rounded top edge and clipped by the body. The faces step
/// from layer to layer by their insets, and ledges overhang where a soft
/// layer sits under a hard one. The clip means a layer never adds rock, so
/// no thin plate is left standing where the body rounds off.
#[derive(Clone, Debug)]
pub(crate) struct Strata {
    /// Layer boundaries, bottom to top: layer `i` lies between `bounds[i]`
    /// and `bounds[i + 1]`, so bedding can vary in thickness.
    pub bounds: Vec<f64>,
    pub layers: Vec<Layer>,
}

/// A whole rock.
#[derive(Clone, Debug)]
pub(crate) struct Rock {
    pub blocks: Vec<Block>,
    /// Smooth-union radius between blocks.
    pub blend: f64,
    pub strata: Option<Strata>,
    /// Low-frequency domain warp: amplitude in metres along each axis (a
    /// bedded rock keeps its strata level by warping little along `Y`),
    /// then frequency per metre.
    pub warp: (DVec3, f64),
    /// Small surface relief, amplitude and frequency; the material carries
    /// the fine detail, so this stays well under the facets' scale.
    pub bump: (f64, f64),
    /// An irregular floor below which the rock is cut away: the height it
    /// hangs from, how much further it dips, and its frequency. It only
    /// dips, so a cliff piece's foot is never shallower than asked.
    pub floor: Option<(f64, f64, f64)>,
    pub noise: Noise,
    /// Added to every point of the shape, so the prop sits on its origin.
    pub shift: DVec3,
}

impl Rock {
    /// The shape at `p` in the prop's frame: negative inside.
    pub(crate) fn distance(&self, p: DVec3) -> f64 {
        let p = p - self.shift;
        let (amplitude, frequency) = self.warp;
        let q = if amplitude == DVec3::ZERO {
            p
        } else {
            p + self.noise.vector(p * frequency, 2) * amplitude
        };
        let mut d = match &self.strata {
            // The strata carve and step the body but never add to it, so a
            // layer cut through the body's rounded top cannot leave a thin
            // plate standing proud of it.
            Some(strata) => self.layered(strata, q).max(self.body(q)),
            None => self.body(q),
        };
        let (bump, bump_frequency) = self.bump;
        if bump > 0.0 {
            d += bump * self.noise.fbm(q * bump_frequency + DVec3::splat(31.7), 2);
        }
        if let Some((floor, amplitude, frequency)) = self.floor {
            let ground = DVec3::new(q.x * frequency, 0.0, q.z * frequency);
            let bottom =
                floor - amplitude * (0.5 + 0.5 * self.noise.fbm(ground + DVec3::splat(-7.3), 2));
            d = smooth_max(d, bottom - q.y, amplitude * 0.5);
        }
        d
    }

    fn body(&self, p: DVec3) -> f64 {
        let mut blocks = self.blocks.iter().map(|b| b.distance(p));
        let first = blocks.next().unwrap_or(f64::INFINITY);
        blocks.fold(first, |d, b| smooth_min(d, b, self.blend))
    }

    fn layered(&self, strata: &Strata, p: DVec3) -> f64 {
        let at = strata.bounds.partition_point(|&b| b <= p.y);
        let mut best = f64::INFINITY;
        // The layer `p` is in first, then its neighbours, each only when it
        // can still come within the fillet of the best so far: a layer is
        // never nearer than the distance to its slab.
        let here = at.checked_sub(1);
        let order = [here, here.and_then(|i| i.checked_sub(1)), Some(at)];
        for index in order.into_iter().flatten() {
            let (Some(layer), Some(&bottom), Some(&top)) = (
                strata.layers.get(index),
                strata.bounds.get(index),
                strata.bounds.get(index + 1),
            ) else {
                continue;
            };
            if (p.y - top).max(bottom - p.y) >= best + layer.round * 0.6 {
                continue;
            }
            let t = top - bottom;
            // The wider of the body's sections at the layer's foot and top,
            // so the layer loses nothing where the body swells or tapers
            // through it; the clip by the body below takes the excess off.
            let slice = |y: f64| self.body(DVec3::new(p.x - layer.shift.x, y, p.z - layer.shift.y));
            let section = slice(bottom).min(slice(top));
            // Zero at the layer's top, one at its foot: the face leans in
            // towards the foot, so the layer above overhangs it.
            let foot = ((top - p.y) / t).clamp(0.0, 1.0);
            let across = section + layer.inset + layer.undercut * foot * foot;
            // Weathering rounds a layer's top edge; its underside stays a
            // crisper overhang over the softer layer below.
            let layer_d = smooth_max(
                rounded_max(across, p.y - top, layer.round),
                bottom - p.y,
                layer.round * 0.5,
            );
            // Filleted into the layer below: a sharp inside crease is what a
            // dual contouring grid beads along.
            best = smooth_min(best, layer_d, layer.round * 0.6);
        }
        best
    }

    /// A box around the shape, grown by everything that can push the
    /// surface past the blocks.
    pub(crate) fn bounds(&self) -> [DVec3; 2] {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for block in &self.blocks {
            let [a, b] = block.bounds();
            min = min.min(a);
            max = max.max(b);
        }
        let mut grow = self.warp.0.max_element() * 2.0 + self.bump.0 * 2.0 + self.blend;
        if let Some(strata) = &self.strata {
            let shift = strata
                .layers
                .iter()
                .map(|l| l.shift.length())
                .fold(0.0, f64::max);
            grow += shift;
        }
        [
            min - DVec3::splat(grow) + self.shift,
            max + DVec3::splat(grow) + self.shift,
        ]
    }

    /// The field's gradient at `p` by central differences `eps` apart,
    /// normalised: the surface normal a level of that resolution sees.
    pub(crate) fn normal(&self, p: DVec3, eps: f64) -> DVec3 {
        let dx = DVec3::X * eps;
        let dy = DVec3::Y * eps;
        let dz = DVec3::Z * eps;
        DVec3::new(
            self.distance(p + dx) - self.distance(p - dx),
            self.distance(p + dy) - self.distance(p - dy),
            self.distance(p + dz) - self.distance(p - dz),
        )
        .normalize_or(DVec3::Y)
    }
}

/// Polynomial smooth minimum: the union of two shapes with a fillet of
/// radius about `k` where they meet.
pub(crate) fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// Polynomial smooth maximum: an intersection with a bevel of about `k`.
pub(crate) fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    -smooth_min(-a, -b, k)
}

/// The intersection of two distances with an exactly round edge of radius
/// `r`, as a rounded box's edge is.
fn rounded_max(a: f64, b: f64, r: f64) -> f64 {
    let q = DVec2::new(a + r, b + r);
    q.max(DVec2::ZERO).length() + q.max_element().min(0.0) - r
}
