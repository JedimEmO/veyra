//! Fractal noise over the `noise` crate's simplex kernel.
//!
//! The crate's own `Fbm`, `RidgedMulti` and `Billow` are not used directly
//! for two reasons. Their permutation-table hash wraps every 256 lattice
//! cells, so a relief octave with a 200 m wavelength would repeat exactly
//! every 51 km, which the world must not do. And their fractal loops hide
//! the per-octave seeds, while the world needs every octave's seed to derive
//! from the world seed by name so that "same seed, same world" holds bit for
//! bit. So this module keeps the kernel ([`simplex_2d`]) and supplies its
//! own lattice hash and its own fractal shapes: the three every biome has
//! drawn since the start, and the three a geological province may choose
//! for a biome's relief (ticket 0004, step 4): [`Shape::Damped`],
//! [`Shape::Hybrid`] and [`Shape::Cells`]. Each carries its gradient
//! analytically, so a slope costs no extra evaluations, and each uses only
//! correctly rounded operations per point (the crate's float rule).

use noise::Vector2;
use noise::core::simplex::simplex_2d;
use noise::permutationtable::NoiseHasher;
use strata_core::mix64;

/// The most octaves a [`Fractal`] holds. Matches the registry's cap on a
/// relief definition.
pub const MAX_OCTAVES: usize = 16;

/// A period-free hash of lattice coordinates.
///
/// Each integer coordinate is folded into a 64-bit mix, so the gradient
/// choice never repeats along an axis the way a 256-entry permutation table
/// does. The kernel only reads the low three bits, hence the shift to bring
/// the well-mixed high bits down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatticeHash {
    seed: u64,
}

impl LatticeHash {
    /// A hash whose lattice is fixed by `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl LatticeHash {
    /// The full 64-bit mix of a lattice cell, for draws that need more than
    /// the kernel's three bits: [`Shape::Cells`]'s jitter.
    fn mix(self, i: i64, j: i64) -> u64 {
        let mut h = self.seed;
        for v in [i, j] {
            h = mix64(h ^ v.cast_unsigned().wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        h
    }
}

impl NoiseHasher for LatticeHash {
    fn hash(&self, to_hash: &[isize]) -> usize {
        let mut h = self.seed;
        for &v in to_hash {
            // Lattice cells are far inside i64 on every target; widening to
            // i64 is what `cast_unsigned` needs to give a u64.
            #[allow(clippy::cast_possible_wrap)]
            let cell = (v as i64).cast_unsigned();
            h = mix64(h ^ cell.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        // The kernel indexes eight gradients with `% 8`; the high bits of
        // the mix are the well-distributed ones, and a usize is at least 32
        // bits wide on every target the game builds for.
        #[allow(clippy::cast_possible_truncation)]
        let folded = (h >> 32) as usize;
        folded
    }
}

/// How octaves combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Plain fractal Brownian motion: the sum of octaves, in `-1..=1`.
    Fbm,
    /// Ridged multifractal: `(1 - |n|)^2` per octave, each weighted by the
    /// one below it so ridges sharpen into crests. In `0..=1`.
    Ridged,
    /// Billow: `|n|` per octave, rounded cushions. In `0..=1`.
    Billow,
    /// fbm whose every octave is divided by `1 + k |D|^2`, `D` the slope the
    /// octaves so far have made (Quilez, "More noise"): mid-slopes lie down
    /// into benches, the ground steepens near each rise's top and foot, and
    /// the fine octaves fade on steep ground. Flat floors and steep sides, as
    /// flat-lying beds weather. `k` is [`Tune::damping`]. In `-1..=1`.
    Damped,
    /// A hybrid multifractal (Musgrave): each octave's `n + offset` weighted
    /// by the product of the ones before, clamped to `0..=1`, so low ground
    /// takes few octaves and stays smooth and high ground takes them all and
    /// is rough. [`Tune::offset`] is the offset. In `-1..=1`.
    Hybrid,
    /// A cellular field: a lattice of jittered points, and round each a bowl
    /// `(d / r)^2` out to [`CELL_BOWL`] of a cell (`d` the distance to the
    /// nearest point, by `sqrt`-free squares), flat beyond, with the other
    /// octaves' simplex noise over it as detail. Closed hollows in a
    /// plateau, which fbm cannot make (its lows are saddles): the karst's
    /// dolines. The nearest point changes along a cell's border, where the
    /// bowls meet in a crease. In `0..=1`.
    Cells,
}

/// How far a [`Shape::Cells`] bowl reaches from its point, in cells. Points
/// are jittered in the middle half of their cells, so two lie at least half
/// a cell apart: at 0.45 most bowls meet a neighbour's somewhere (a rim
/// between two hollows) and leave a plateau between three or more, a
/// country pocked with hollows rather than one of cones.
pub const CELL_BOWL: f64 = 0.45;

/// How far, in cells, a [`Shape::Cells`] field's lattice is bent at the
/// bending noise's extreme.
pub const CELL_BEND: f64 = 0.3;

/// How much a [`Shape::Cells`] field's detail octaves weigh against a
/// plain fractal's, which would weigh one.
pub const CELL_DETAIL: f64 = 0.5;

/// The parameters of a shape that a province may blend across its border,
/// where one noise is evaluated with the blend of both provinces' values
/// (ticket 0004, step 4). The other shapes ignore them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tune {
    /// [`Shape::Damped`]'s `k` in `1 / (1 + k |D|^2)`, at least 0; 0 is fbm.
    pub damping: f64,
    /// [`Shape::Hybrid`]'s offset.
    pub offset: f64,
}

impl Tune {
    /// No damping and Musgrave's customary offset.
    pub const DEFAULT: Self = Self {
        damping: 0.0,
        offset: 0.7,
    };
}

impl Shape {
    /// Whether the shape's values run from `-1` (signed) or from `0`.
    #[must_use]
    pub const fn signed(self) -> bool {
        matches!(self, Self::Fbm | Self::Damped | Self::Hybrid)
    }
}

/// A fractal's anisotropy: its features stretched along an axis.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stretch {
    cos: f64,
    sin: f64,
    /// `1 / stretch`, so a point costs a multiply.
    inverse: f64,
}

/// A stack of simplex octaves with one shape.
///
/// Evaluation allocates nothing: the per-octave hashes live inline.
#[derive(Debug, Clone)]
pub struct Fractal {
    shape: Shape,
    octaves: usize,
    frequency: f64,
    lacunarity: f64,
    persistence: f64,
    /// `1 / sum(persistence^k)`, so the sum lands in the shape's range
    /// whatever the octave count.
    norm: f64,
    hashes: [LatticeHash; MAX_OCTAVES],
    /// Along which axis and how far the features are stretched; `None` is
    /// round.
    stretch: Option<Stretch>,
    /// `(cos, sin)` of the angle [`Shape::Cells`] turns its lattice by, so
    /// its rows of hollows follow no axis of the world.
    cell_turn: (f64, f64),
}

impl Fractal {
    /// A fractal whose largest feature is `wavelength_m` metres across.
    ///
    /// Every octave gets its own lattice, hashed from `seed` and the octave
    /// index, so octaves never line up. `octaves` is clamped to
    /// `1..=MAX_OCTAVES`.
    #[must_use]
    pub fn new(
        seed: u64,
        shape: Shape,
        wavelength_m: f64,
        octaves: usize,
        persistence: f64,
        lacunarity: f64,
    ) -> Self {
        let octaves = octaves.clamp(1, MAX_OCTAVES);
        let mut hashes = [LatticeHash::new(0); MAX_OCTAVES];
        for (k, hash) in hashes.iter_mut().enumerate() {
            *hash = LatticeHash::new(mix64(
                seed ^ (k as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93),
            ));
        }
        let mut norm = 0.0;
        let mut amplitude = 1.0;
        for _ in 0..octaves {
            norm += amplitude;
            amplitude *= persistence;
        }
        // A turn chosen by the seed, in the constructor, through `libm`
        // (the crate's float rule allows a transcendental only here).
        #[allow(clippy::cast_precision_loss, reason = "53 bits of a hash, exactly")]
        let angle = (mix64(seed ^ 0x6A09_E667_F3BC_C909) >> 11) as f64 / (1_u64 << 53) as f64
            * core::f64::consts::TAU;
        Self {
            shape,
            octaves,
            frequency: 1.0 / wavelength_m,
            lacunarity,
            persistence,
            norm: 1.0 / norm,
            hashes,
            stretch: None,
            cell_turn: (libm::cos(angle), libm::sin(angle)),
        }
    }

    /// The same fractal with its features `stretch` times longer along the
    /// unit axis `(cos, sin)` than across it (a province's fold grain,
    /// ticket 0004, step 4). A stretch of one or less leaves it round, and
    /// the same bits as before.
    #[must_use]
    pub fn stretched(mut self, axis: (f64, f64), stretch: f64) -> Self {
        self.stretch = (stretch > 1.0).then(|| Stretch {
            cos: axis.0,
            sin: axis.1,
            inverse: 1.0 / stretch,
        });
        self
    }

    /// The shape.
    #[must_use]
    pub const fn shape(&self) -> Shape {
        self.shape
    }

    /// The point in the fractal's own frame: turned and squeezed along the
    /// stretch axis, or as it is.
    fn frame(&self, x: f64, z: f64) -> (f64, f64) {
        match self.stretch {
            None => (x, z),
            Some(s) => ((s.cos * x + s.sin * z) * s.inverse, -s.sin * x + s.cos * z),
        }
    }

    /// A gradient in the fractal's frame carried back to the world's.
    fn unframe(&self, (ga, gc): (f64, f64)) -> (f64, f64) {
        match self.stretch {
            None => (ga, gc),
            Some(s) => (
                ga * s.cos * s.inverse - gc * s.sin,
                ga * s.sin * s.inverse + gc * s.cos,
            ),
        }
    }

    /// The noise at a point, with the blendable parameters of the shapes
    /// that have them. For the three first shapes, round, this is
    /// [`get`](Self::get) to the bit.
    #[must_use]
    pub fn get_tuned(&self, x: f64, z: f64, tune: Tune) -> f64 {
        let (x, z) = self.frame(x, z);
        self.round_value(x, z, tune)
    }

    /// The noise at a point and its gradient per metre, with the blendable
    /// parameters of the shapes that have them. For the three first
    /// shapes, round, this is [`get_with_gradient`](Self::get_with_gradient)
    /// to the bit.
    #[must_use]
    pub fn get_with_gradient_tuned(&self, x: f64, z: f64, tune: Tune) -> (f64, (f64, f64)) {
        let (fx, fz) = self.frame(x, z);
        let (value, gradient) = self.round_gradient(fx, fz, tune);
        (value, self.unframe(gradient))
    }

    /// A round [`Shape::Fbm`]'s value and gradient per metre at several
    /// points, each [`get_with_gradient`](Self::get_with_gradient)'s bits,
    /// sharing each octave's lattice lookups among points in one simplex
    /// ([`simplex_2d_gradient_many`]). For the range field's taps (ticket
    /// 0004's review of step 4).
    ///
    /// # Panics
    ///
    /// In a debug build, for another shape or a stretched fractal.
    #[must_use]
    pub fn fbm_with_gradient_many<const N: usize>(
        &self,
        points: [(f64, f64); N],
    ) -> [(f64, (f64, f64)); N] {
        debug_assert!(self.shape == Shape::Fbm && self.stretch.is_none());
        let mut at = points.map(|(x, z)| [x * self.frequency, z * self.frequency]);
        let mut frequency = self.frequency;
        let mut amplitude = 1.0;
        let mut sums = [(0.0, (0.0, 0.0)); N];
        for hash in &self.hashes[..self.octaves] {
            let octave = simplex_2d_gradient_many(at, hash);
            for (sum, (n, [dnx, dnz])) in sums.iter_mut().zip(octave) {
                let dn = (dnx * frequency, dnz * frequency);
                sum.0 += n * amplitude;
                sum.1.0 += dn.0 * amplitude;
                sum.1.1 += dn.1 * amplitude;
            }
            amplitude *= self.persistence;
            // `Vector2 *= lacunarity`, as the single-point path scales it.
            for p in &mut at {
                *p = [p[0] * self.lacunarity, p[1] * self.lacunarity];
            }
            frequency *= self.lacunarity;
        }
        sums.map(|(sum, d_sum)| {
            let value = sum * self.norm;
            if value <= -1.0 || value >= 1.0 {
                (value.clamp(-1.0, 1.0), (0.0, 0.0))
            } else {
                (value, (d_sum.0 * self.norm, d_sum.1 * self.norm))
            }
        })
    }

    /// [`Shape::Damped`], in the fractal's frame: the value and, if asked,
    /// its gradient.
    ///
    /// `D`, the slope so far, is the gradient of the octaves summed up to
    /// and including this one, times the first octave's wavelength: at a
    /// persistence of a half and a lacunarity of two it is Quilez's sum of
    /// the octaves' own derivatives. The gradient is exact: the damping's
    /// own derivative needs each octave's second derivatives, which
    /// [`simplex_2d_hessian`] gives.
    #[allow(
        clippy::many_single_char_names,
        clippy::similar_names,
        reason = "the point, Quilez's k and D, as the formula names them"
    )]
    fn damped(&self, x: f64, z: f64, k: f64, with_gradient: bool) -> (f64, (f64, f64)) {
        let mut point = [x * self.frequency, z * self.frequency];
        let mut frequency = self.frequency;
        let mut amplitude = 1.0;
        // Amplitude times lacunarity to the octave: what an octave's own
        // derivative weighs in `D`.
        let mut slope_weight = 1.0;
        let mut d = [0.0, 0.0];
        // d D / d (x, z): rows are D's components.
        let mut dd = [[0.0, 0.0], [0.0, 0.0]];
        let mut sum = 0.0;
        let mut d_sum = (0.0, 0.0);
        for hash in &self.hashes[..self.octaves] {
            // `D` is first-order: only the gradient's own slope needs the
            // kernel's second derivatives, and a value alone (the relief's
            // five coarse taps) is the same bits without them at a fraction
            // of the cost (the review of ticket 0004's step 4).
            let (n, g, h) = if with_gradient {
                simplex_2d_hessian(point, hash)
            } else {
                let (n, g) = simplex_2d_gradient(point, hash);
                (n, g, [0.0; 3])
            };
            d[0] += slope_weight * g[0];
            d[1] += slope_weight * g[1];
            let q = 1.0 + k * (d[0] * d[0] + d[1] * d[1]);
            sum += amplitude * n / q;
            if with_gradient {
                let s = slope_weight * frequency;
                dd[0][0] += s * h[0];
                dd[0][1] += s * h[1];
                dd[1][0] += s * h[1];
                dd[1][1] += s * h[2];
                // dq/dx = 2k (D . dD/dx).
                let dq = (
                    2.0 * k * (d[0] * dd[0][0] + d[1] * dd[1][0]),
                    2.0 * k * (d[0] * dd[0][1] + d[1] * dd[1][1]),
                );
                let q2 = q * q;
                d_sum.0 += amplitude * (g[0] * frequency * q - n * dq.0) / q2;
                d_sum.1 += amplitude * (g[1] * frequency * q - n * dq.1) / q2;
            }
            amplitude *= self.persistence;
            slope_weight *= self.persistence * self.lacunarity;
            point = [point[0] * self.lacunarity, point[1] * self.lacunarity];
            frequency *= self.lacunarity;
        }
        clamped(
            sum * self.norm,
            (d_sum.0 * self.norm, d_sum.1 * self.norm),
            -1.0,
        )
    }

    /// [`Shape::Hybrid`], in the fractal's frame, and its gradient.
    fn hybrid(&self, x: f64, z: f64, offset: f64) -> (f64, (f64, f64)) {
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut frequency = self.frequency;
        let mut amplitude = 1.0;
        let mut result = 0.0;
        let mut d_result = (0.0, 0.0);
        // The weight the next octave is added with, and its gradient.
        let mut weight = 1.0;
        let mut d_weight = (0.0, 0.0);
        for (k, hash) in self.hashes[..self.octaves].iter().enumerate() {
            let (n, [gx, gz]) = simplex_2d_gradient([point.x, point.y], hash);
            let signal = (n + offset) * amplitude;
            let d_signal = (gx * frequency * amplitude, gz * frequency * amplitude);
            if k == 0 {
                result = signal;
                d_result = d_signal;
                weight = signal;
                d_weight = d_signal;
            } else {
                result += weight * signal;
                d_result.0 += d_weight.0 * signal + weight * d_signal.0;
                d_result.1 += d_weight.1 * signal + weight * d_signal.1;
                d_weight = (
                    d_weight.0 * signal + weight * d_signal.0,
                    d_weight.1 * signal + weight * d_signal.1,
                );
                weight *= signal;
            }
            // Clamped to the unit: a weight over one would let a peak's
            // octaves grow without bound, and one under zero would turn a
            // valley's detail upside down.
            if weight >= 1.0 {
                weight = 1.0;
                d_weight = (0.0, 0.0);
            } else if weight <= 0.0 {
                weight = 0.0;
                d_weight = (0.0, 0.0);
            }
            amplitude *= self.persistence;
            point *= self.lacunarity;
            frequency *= self.lacunarity;
        }
        // At its highest every signal is `(1 + offset)` times its amplitude
        // and every weight one, so `result * norm` is at most `1 + offset`;
        // less the offset it is at most one, and about zero on average.
        clamped(
            result * self.norm - offset,
            (d_result.0 * self.norm, d_result.1 * self.norm),
            -1.0,
        )
    }

    /// [`Shape::Cells`], in the fractal's frame, and its gradient.
    #[allow(
        clippy::many_single_char_names,
        clippy::similar_names,
        reason = "a lattice's cell coordinates and its bend's partials, named by their axes"
    )]
    fn cells(&self, x: f64, z: f64) -> (f64, (f64, f64)) {
        let (cos, sin) = self.cell_turn;
        // The lattice's frame: turned, in cells, and bent by a noise of
        // twice the cell's size, so the hollows do not lie in rows: on the
        // bare jittered lattice they read as a honeycomb from a ridge.
        let (u0, v0) = (
            (cos * x + sin * z) * self.frequency,
            (-sin * x + cos * z) * self.frequency,
        );
        let (bu, [bu_du, bu_dv]) =
            simplex_2d_gradient([u0 * 0.5, v0 * 0.5], &self.hashes[MAX_OCTAVES - 1]);
        let (bv, [bv_du, bv_dv]) = simplex_2d_gradient(
            [u0 * 0.5 + 31.7, v0 * 0.5 - 17.3],
            &self.hashes[MAX_OCTAVES - 2],
        );
        let (u, v) = (u0 + CELL_BEND * bu, v0 + CELL_BEND * bv);
        // d(u, v) / d(u0, v0).
        let (ju_u, ju_v) = (1.0 + CELL_BEND * 0.5 * bu_du, CELL_BEND * 0.5 * bu_dv);
        let (jv_u, jv_v) = (CELL_BEND * 0.5 * bv_du, 1.0 + CELL_BEND * 0.5 * bv_dv);
        let (cu, cv) = (u.floor(), v.floor());
        // Cells are far inside i64 for any point a world can place.
        #[allow(clippy::cast_possible_truncation, reason = "lattice cells")]
        let (iu, iv) = (cu as i64, cv as i64);
        let hash = &self.hashes[0];
        let mut nearest = (f64::INFINITY, 0.0, 0.0);
        for j in -1..=1 {
            for i in -1..=1 {
                let (pu, pv) = cell_point(*hash, iu + i, iv + j);
                // i64 cell offsets of one; exact in f64.
                #[allow(clippy::cast_precision_loss, reason = "offsets of one")]
                let (du, dv) = (u - (cu + i as f64 + pu), v - (cv + j as f64 + pv));
                let d2 = du * du + dv * dv;
                if d2 < nearest.0 {
                    nearest = (d2, du, dv);
                }
            }
        }
        let (d2, du, dv) = nearest;
        let r2 = CELL_BOWL * CELL_BOWL;
        let (bowl, d_bowl) = if d2 < r2 {
            // d/du of du^2 / r^2 is 2 du / r^2, per cell; the bend's
            // Jacobian carries it to the turned frame, a cell is a
            // wavelength, and the turn carries it back.
            let (pu, pv) = (2.0 * du / r2, 2.0 * dv / r2);
            let (gu, gv) = (
                (pu * ju_u + pv * jv_u) * self.frequency,
                (pu * ju_v + pv * jv_v) * self.frequency,
            );
            (d2 / r2, (cos * gu - sin * gv, sin * gu + cos * gv))
        } else {
            (1.0, (0.0, 0.0))
        };
        let mut sum = bowl;
        let mut d_sum = d_bowl;
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut frequency = self.frequency;
        let mut amplitude = 1.0;
        let mut total = 1.0;
        for hash in &self.hashes[1..self.octaves] {
            amplitude *= self.persistence;
            point *= self.lacunarity;
            frequency *= self.lacunarity;
            let (n, [gx, gz]) = simplex_2d_gradient([point.x, point.y], hash);
            // Unsigned detail, `(n + 1) / 2`, so it only roughens, at half
            // an octave's weight: at a whole one it hid the plateau the
            // hollows are pocked into.
            let weight = amplitude * CELL_DETAIL;
            sum += f64::midpoint(n, 1.0) * weight;
            d_sum.0 += gx * frequency * 0.5 * weight;
            d_sum.1 += gz * frequency * 0.5 * weight;
            total += weight;
        }
        clamped(sum / total, (d_sum.0 / total, d_sum.1 / total), 0.0)
    }

    /// The noise at a point and at four taps `radius` metres from it,
    /// `[centre, +x, -x, +z, -z]`, with the blendable parameters: what the
    /// relief's coarse taps read (hydration and the gullies' steer). Each is
    /// [`get_tuned`](Self::get_tuned) at its tap, to the bit.
    ///
    /// A cellular field's five taps are worked out together, for cost: five
    /// bends and five 3 by 3 searches of the lattice cost as much as the rest
    /// of a sample in karst (the review of ticket 0004's step 4). The bends
    /// are read without their gradients, and each lattice cell's point is
    /// looked up once for all the taps.
    #[must_use]
    pub fn ring_tuned(&self, x: f64, z: f64, radius: f64, tune: Tune) -> [f64; 5] {
        let taps = [
            (0.0, 0.0),
            (radius, 0.0),
            (-radius, 0.0),
            (0.0, radius),
            (0.0, -radius),
        ];
        if self.shape != Shape::Cells {
            return taps.map(|(dx, dz)| self.get_tuned(x + dx, z + dz, tune));
        }
        let points = taps.map(|(dx, dz)| self.frame(x + dx, z + dz));
        self.cells_ring(points)
    }

    /// [`Shape::Cells`]'s values at five points of its own frame near one
    /// another, the same bits as [`cells`](Self::cells)' at each.
    #[allow(
        clippy::many_single_char_names,
        clippy::similar_names,
        reason = "a lattice's cell coordinates, named by their axes"
    )]
    fn cells_ring(&self, points: [(f64, f64); 5]) -> [f64; 5] {
        let (cos, sin) = self.cell_turn;
        let hash = self.hashes[0];
        // The cells two either side of the first point's, looked up once
        // each; a cell outside them is looked up where it is needed.
        let mut cache = [(0.0, 0.0); 25];
        let mut known = 0_u32;
        let mut origin = None;
        let r2 = CELL_BOWL * CELL_BOWL;
        points.map(|(x, z)| {
            let (u0, v0) = (
                (cos * x + sin * z) * self.frequency,
                (-sin * x + cos * z) * self.frequency,
            );
            let (bu, _) = simplex_2d(
                Vector2::new(u0 * 0.5, v0 * 0.5),
                &self.hashes[MAX_OCTAVES - 1],
            );
            let (bv, _) = simplex_2d(
                Vector2::new(u0 * 0.5 + 31.7, v0 * 0.5 - 17.3),
                &self.hashes[MAX_OCTAVES - 2],
            );
            let (u, v) = (u0 + CELL_BEND * bu, v0 + CELL_BEND * bv);
            let (cu, cv) = (u.floor(), v.floor());
            // Cells are far inside i64 for any point a world can place.
            #[allow(clippy::cast_possible_truncation, reason = "lattice cells")]
            let (iu, iv) = (cu as i64, cv as i64);
            let (iu0, iv0) = *origin.get_or_insert((iu, iv));
            let mut nearest = f64::INFINITY;
            for j in -1..=1 {
                for i in -1..=1 {
                    let (ci, cj) = (iu + i - iu0 + 2, iv + j - iv0 + 2);
                    let (pu, pv) = if (0..5).contains(&ci) && (0..5).contains(&cj) {
                        #[allow(
                            clippy::cast_sign_loss,
                            clippy::cast_possible_truncation,
                            reason = "an index under 25, by the test above"
                        )]
                        let slot = (cj * 5 + ci) as usize;
                        if known & (1 << slot) == 0 {
                            cache[slot] = cell_point(hash, iu + i, iv + j);
                            known |= 1 << slot;
                        }
                        cache[slot]
                    } else {
                        cell_point(hash, iu + i, iv + j)
                    };
                    #[allow(clippy::cast_precision_loss, reason = "offsets of one")]
                    let (du, dv) = (u - (cu + i as f64 + pu), v - (cv + j as f64 + pv));
                    let d2 = du * du + dv * dv;
                    if d2 < nearest {
                        nearest = d2;
                    }
                }
            }
            let bowl = if nearest < r2 { nearest / r2 } else { 1.0 };
            let (sum, total) = self.cell_detail(x, z, bowl);
            (sum / total).clamp(0.0, 1.0)
        })
    }

    /// A cellular field's detail octaves over its bowl, unscaled: the sum
    /// and what it is to be divided by.
    fn cell_detail(&self, x: f64, z: f64, bowl: f64) -> (f64, f64) {
        let mut sum = bowl;
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut amplitude = 1.0;
        let mut total = 1.0;
        for hash in &self.hashes[1..self.octaves] {
            amplitude *= self.persistence;
            point *= self.lacunarity;
            let (n, _) = simplex_2d(point, hash);
            let weight = amplitude * CELL_DETAIL;
            sum += f64::midpoint(n, 1.0) * weight;
            total += weight;
        }
        (sum, total)
    }

    /// The same fractal with only its first `octaves` octaves, on the
    /// original's scale.
    ///
    /// The normalisation is deliberately not recomputed: a truncated stack
    /// has to be the low-frequency part of the full one, not a field of its
    /// own with the same amplitude. Renormalising would make a two-octave
    /// version of a four-octave continent nearly twice as tall, which moves
    /// every coast, and the rain march walks a coarse continent precisely
    /// so that it agrees with the real one about where the land is.
    #[must_use]
    pub fn truncated(&self, octaves: usize) -> Self {
        Self {
            octaves: octaves.clamp(1, self.octaves),
            ..self.clone()
        }
    }

    /// The noise at a point in metres.
    #[must_use]
    pub fn get(&self, x: f64, z: f64) -> f64 {
        if self.stretch.is_some() {
            return self.get_tuned(x, z, Tune::DEFAULT);
        }
        self.round_value(x, z, Tune::DEFAULT)
    }

    /// The value in the fractal's own frame.
    fn round_value(&self, x: f64, z: f64, tune: Tune) -> f64 {
        match self.shape {
            Shape::Fbm => self.fbm(x, z),
            Shape::Ridged => self.ridged(x, z),
            Shape::Billow => self.billow(x, z),
            Shape::Damped => self.damped(x, z, tune.damping, false).0,
            Shape::Hybrid => self.hybrid(x, z, tune.offset).0,
            Shape::Cells => self.cells(x, z).0,
        }
    }

    /// The noise at a point and its gradient, `(d/dx, d/dz)` per metre.
    ///
    /// From the kernel's own analytic derivative carried through each
    /// octave, so a slope costs no extra evaluations. Where the shape's range
    /// clamps the value, the gradient is zero, as the value there is flat.
    #[must_use]
    pub fn get_with_gradient(&self, x: f64, z: f64) -> (f64, (f64, f64)) {
        if self.stretch.is_some() {
            return self.get_with_gradient_tuned(x, z, Tune::DEFAULT);
        }
        self.round_gradient(x, z, Tune::DEFAULT)
    }

    /// The value and gradient in the fractal's own frame.
    fn round_gradient(&self, x: f64, z: f64, tune: Tune) -> (f64, (f64, f64)) {
        match self.shape {
            Shape::Damped => return self.damped(x, z, tune.damping, true),
            Shape::Hybrid => return self.hybrid(x, z, tune.offset),
            Shape::Cells => return self.cells(x, z),
            Shape::Fbm | Shape::Ridged | Shape::Billow => {}
        }
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut frequency = self.frequency;
        let mut amplitude = 1.0;
        let mut weight = 1.0;
        let mut d_weight = (0.0, 0.0);
        let mut sum = 0.0;
        let mut d_sum = (0.0, 0.0);
        for hash in &self.hashes[..self.octaves] {
            let (n, [dnx, dnz]) = simplex_2d_gradient([point.x, point.y], hash);
            // The kernel differentiates by its own input; the input is the
            // point in metres times this octave's frequency.
            let dn = (dnx * frequency, dnz * frequency);
            let (signal, d_signal) = match self.shape {
                Shape::Fbm | Shape::Damped | Shape::Hybrid | Shape::Cells => (n, dn),
                Shape::Billow => {
                    let sign = if n < 0.0 { -1.0 } else { 1.0 };
                    (n.abs(), (dn.0 * sign, dn.1 * sign))
                }
                Shape::Ridged => {
                    let sign = if n < 0.0 { -1.0 } else { 1.0 };
                    let ridge = 1.0 - n.abs();
                    let d_ridge = (-dn.0 * sign, -dn.1 * sign);
                    let signal = ridge * ridge * weight;
                    let d_signal = (
                        2.0 * ridge * d_ridge.0 * weight + ridge * ridge * d_weight.0,
                        2.0 * ridge * d_ridge.1 * weight + ridge * ridge * d_weight.1,
                    );
                    let raised = signal * 2.0;
                    if raised > 0.0 && raised < 1.0 {
                        weight = raised;
                        d_weight = (d_signal.0 * 2.0, d_signal.1 * 2.0);
                    } else {
                        weight = raised.clamp(0.0, 1.0);
                        d_weight = (0.0, 0.0);
                    }
                    (signal, d_signal)
                }
            };
            sum += signal * amplitude;
            d_sum.0 += d_signal.0 * amplitude;
            d_sum.1 += d_signal.1 * amplitude;
            amplitude *= self.persistence;
            point *= self.lacunarity;
            frequency *= self.lacunarity;
        }
        let (low, high) = if self.shape.signed() {
            (-1.0, 1.0)
        } else {
            (0.0, 1.0)
        };
        let value = sum * self.norm;
        if value <= low || value >= high {
            (value.clamp(low, high), (0.0, 0.0))
        } else {
            (value, (d_sum.0 * self.norm, d_sum.1 * self.norm))
        }
    }

    fn fbm(&self, x: f64, z: f64) -> f64 {
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut amplitude = 1.0;
        let mut sum = 0.0;
        for hash in &self.hashes[..self.octaves] {
            let (n, _) = simplex_2d(point, hash);
            sum += n * amplitude;
            amplitude *= self.persistence;
            point *= self.lacunarity;
        }
        (sum * self.norm).clamp(-1.0, 1.0)
    }

    fn billow(&self, x: f64, z: f64) -> f64 {
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut amplitude = 1.0;
        let mut sum = 0.0;
        for hash in &self.hashes[..self.octaves] {
            let (n, _) = simplex_2d(point, hash);
            sum += n.abs() * amplitude;
            amplitude *= self.persistence;
            point *= self.lacunarity;
        }
        (sum * self.norm).clamp(0.0, 1.0)
    }

    fn ridged(&self, x: f64, z: f64) -> f64 {
        let mut point = Vector2::new(x * self.frequency, z * self.frequency);
        let mut amplitude = 1.0;
        let mut weight = 1.0;
        let mut sum = 0.0;
        for hash in &self.hashes[..self.octaves] {
            let (n, _) = simplex_2d(point, hash);
            let ridge = 1.0 - n.abs();
            let signal = ridge * ridge * weight;
            // The next octave only adds detail where this one is high, which
            // is what turns a sum of ridges into crests with smooth valleys.
            weight = (signal * 2.0).clamp(0.0, 1.0);
            sum += signal * amplitude;
            amplitude *= self.persistence;
            point *= self.lacunarity;
        }
        (sum * self.norm).clamp(0.0, 1.0)
    }
}

/// The mean value of a shape at an octave count, persistence, lacunarity
/// and tune: a property of the shape, not of a seed or a wavelength, so two
/// fractals of the same shape and octaves have one mean exactly.
///
/// fbm and the damped shape are odd (the same noise with every gradient
/// turned round is the field negated), so their mean is zero exactly. The
/// others are measured once, over a lattice of 128 by 128 points 0.37 of a
/// wavelength apart, about 47 wavelengths a side: within about a hundredth
/// of the value's range. For what a relief of one sign mapped onto a class
/// of the other is centred on (the review of ticket 0004's step 4: `(n -
/// 0.5)` for a ridged or billowed relief on hills lowered the granite and
/// folded highlands by 66 and 40 m on average and cut a trench under the
/// sea 1 km wide inland).
#[must_use]
pub fn shape_mean(
    shape: Shape,
    octaves: usize,
    persistence: f64,
    lacunarity: f64,
    tune: Tune,
) -> f64 {
    const SIDE: u32 = 128;
    if matches!(shape, Shape::Fbm | Shape::Damped) {
        return 0.0;
    }
    let fractal = Fractal::new(
        0x5eed_3ea2,
        shape,
        1_000.0,
        octaves,
        persistence,
        lacunarity,
    );
    let mut sum = 0.0;
    for j in 0..SIDE {
        for i in 0..SIDE {
            sum += fractal.get_tuned(
                f64::from(i) * 370.0 + 13.7,
                f64::from(j) * 370.0 - 51.1,
                tune,
            );
        }
    }
    sum / f64::from(SIDE * SIDE)
}

/// A value clamped to `low..=1`, with its gradient zeroed where the clamp
/// holds, as the value there is flat.
fn clamped(value: f64, gradient: (f64, f64), low: f64) -> (f64, (f64, f64)) {
    if value <= low || value >= 1.0 {
        (value.clamp(low, 1.0), (0.0, 0.0))
    } else {
        (value, gradient)
    }
}

/// The jittered point of a [`Shape::Cells`] lattice cell, as its offset
/// in the cell: in the middle half, so the 3 by 3 cells round a point hold
/// its nearest point (the nearest point of a cell two away is at least
/// 1.25 cells off, and the point's own at most 1.07).
#[allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "a cell's indices and its two draws"
)]
fn cell_point(hash: LatticeHash, i: i64, j: i64) -> (f64, f64) {
    // Two draws from one mix: 26 bits each, exact in f64.
    let h = hash.mix(i, j);
    #[allow(clippy::cast_precision_loss, reason = "26 bits, exactly")]
    let (a, b) = (
        (h >> 38) as f64 / (1_u64 << 26) as f64,
        ((h >> 12) & ((1 << 26) - 1)) as f64 / (1_u64 << 26) as f64,
    );
    (0.25 + 0.5 * a, 0.25 + 0.5 * b)
}

/// [`simplex_2d_gradient`] and the kernel's second derivatives, `[d2/dx2,
/// d2/dxdz, d2/dz2]`, for [`Shape::Damped`]'s exact gradient.
///
/// A corner contributes `F(t) (p . g)` with `F = 2 t^2 + t^4` and `t = 1 -
/// 2 |p|^2`; with `F' = 4 t (1 + t^2)` and `F'' = 4 + 12 t^2` its Hessian
/// is `-4 F' (g p^T + p g^T) + 16 F'' (p . g) p p^T - 4 F' (p . g) I`.
/// `F''(0)` is 4, not 0, so the Hessian jumps where a corner's reach ends:
/// a slope built from it is exact but not continuous, and nothing that
/// steers the ground (the erosion's direction) may read one (the review of
/// ticket 0004's step 4).
#[must_use]
pub fn simplex_2d_hessian(point: [f64; 2], hasher: &LatticeHash) -> (f64, [f64; 2], [f64; 3]) {
    let skew_factor = (3.0_f64.sqrt() - 1.0) / 2.0;
    let unskew_factor = (1.0 - 1.0 / 3.0_f64.sqrt()) / 2.0;
    let skew = (point[0] + point[1]) * skew_factor;
    let floor = [(point[0] + skew).floor(), (point[1] + skew).floor()];
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lattice cells are far inside isize for any point a world can place"
    )]
    let cell = [floor[0] as isize, floor[1] as isize];
    let unskew = (floor[0] + floor[1]) * unskew_factor;
    let offset1 = [
        point[0] - (floor[0] - unskew),
        point[1] - (floor[1] - unskew),
    ];
    let (order, step) = if offset1[0] > offset1[1] {
        ([1.0, 0.0], [1, 0])
    } else {
        ([0.0, 1.0], [0, 1])
    };
    let offset2 = [
        offset1[0] - order[0] + unskew_factor,
        offset1[1] - order[1] + unskew_factor,
    ];
    let offset3 = [
        offset1[0] - 1.0 + 2.0 * unskew_factor,
        offset1[1] - 1.0 + 2.0 * unskew_factor,
    ];
    let corners = [
        (hasher.hash(&cell), offset1),
        (
            hasher.hash(&[cell[0] + step[0], cell[1] + step[1]]),
            offset2,
        ),
        (hasher.hash(&[cell[0] + 1, cell[1] + 1]), offset3),
    ];
    let mut value = 0.0;
    let mut gradient = [0.0, 0.0];
    let mut hessian = [0.0, 0.0, 0.0];
    for (index, p) in corners {
        let t = 1.0 - (p[0] * p[0] + p[1] * p[1]) * 2.0;
        if t > 0.0 {
            let g = grad2(index);
            let t2 = t * t;
            let t4 = t2 * t2;
            let dot = p[0] * g[0] + p[1] * g[1];
            let falloff = 2.0 * t2 + t4;
            value += falloff * dot;
            let bend = -16.0 * t * (1.0 + t2) * dot;
            gradient[0] += falloff * g[0] + bend * p[0];
            gradient[1] += falloff * g[1] + bend * p[1];
            let f1 = 4.0 * t * (1.0 + t2);
            let f2 = 4.0 + 12.0 * t2;
            let pp = 16.0 * f2 * dot;
            let diag = -4.0 * f1 * dot;
            hessian[0] += -8.0 * f1 * g[0] * p[0] + pp * p[0] * p[0] + diag;
            hessian[1] += -4.0 * f1 * (g[0] * p[1] + g[1] * p[0]) + pp * p[0] * p[1];
            hessian[2] += -8.0 * f1 * g[1] * p[1] + pp * p[1] * p[1] + diag;
        }
    }
    (value, gradient, hessian)
}

/// The `noise` crate's 2D simplex kernel with a correct gradient.
///
/// The crate's own `simplex_2d` returns a derivative that does not belong to
/// its value (it differentiates a `t^4` falloff where the value uses
/// `2 t^2 + t^4`, and adds an offset it should multiply). This repeats the
/// value operation for operation, so it is the same bits as the crate's, and
/// differentiates it: each corner contributes `(2 t^2 + t^4) (p . g)` with
/// `t = 1 - 2 |p|^2`, whose gradient is `(2 t^2 + t^4) g - 16 t (1 + t^2)
/// (p . g) p`.
#[must_use]
pub fn simplex_2d_gradient(point: [f64; 2], hasher: &LatticeHash) -> (f64, [f64; 2]) {
    // The crate computes both factors from `sqrt` at run time; `sqrt` is
    // correctly rounded, so these are its bits exactly.
    let skew_factor = (3.0_f64.sqrt() - 1.0) / 2.0;
    let unskew_factor = (1.0 - 1.0 / 3.0_f64.sqrt()) / 2.0;
    let skew = (point[0] + point[1]) * skew_factor;
    let floor = [(point[0] + skew).floor(), (point[1] + skew).floor()];
    // Lattice cells are far inside isize for any point a world can place.
    #[allow(clippy::cast_possible_truncation)]
    let cell = [floor[0] as isize, floor[1] as isize];
    let unskew = (floor[0] + floor[1]) * unskew_factor;
    let offset1 = [
        point[0] - (floor[0] - unskew),
        point[1] - (floor[1] - unskew),
    ];
    let (order, step) = if offset1[0] > offset1[1] {
        ([1.0, 0.0], [1, 0])
    } else {
        ([0.0, 1.0], [0, 1])
    };
    let offset2 = [
        offset1[0] - order[0] + unskew_factor,
        offset1[1] - order[1] + unskew_factor,
    ];
    let offset3 = [
        offset1[0] - 1.0 + 2.0 * unskew_factor,
        offset1[1] - 1.0 + 2.0 * unskew_factor,
    ];
    let corners = [
        (hasher.hash(&cell), offset1),
        (
            hasher.hash(&[cell[0] + step[0], cell[1] + step[1]]),
            offset2,
        ),
        (hasher.hash(&[cell[0] + 1, cell[1] + 1]), offset3),
    ];
    let mut value = 0.0;
    let mut gradient = [0.0, 0.0];
    for (index, p) in corners {
        let t = 1.0 - (p[0] * p[0] + p[1] * p[1]) * 2.0;
        if t > 0.0 {
            let g = grad2(index);
            let t2 = t * t;
            let t4 = t2 * t2;
            let dot = p[0] * g[0] + p[1] * g[1];
            let falloff = 2.0 * t2 + t4;
            value += falloff * dot;
            let bend = -16.0 * t * (1.0 + t2) * dot;
            gradient[0] += falloff * g[0] + bend * p[0];
            gradient[1] += falloff * g[1] + bend * p[1];
        }
    }
    (value, gradient)
}

/// [`simplex_2d_gradient`] at several points, each its bits, looking a
/// simplex's corners up once for every point that lies in it: points a
/// fraction of a lattice cell apart (the range field's taps, 100 m apart at
/// a wavelength of kilometres) nearly always share all three, and the
/// hashes are most of the kernel's cost.
#[must_use]
pub fn simplex_2d_gradient_many<const N: usize>(
    points: [[f64; 2]; N],
    hasher: &LatticeHash,
) -> [(f64, [f64; 2]); N] {
    let skew_factor = (3.0_f64.sqrt() - 1.0) / 2.0;
    let unskew_factor = (1.0 - 1.0 / 3.0_f64.sqrt()) / 2.0;
    // The last simplex looked up: its floor, which of its two triangles,
    // and its three corners' gradient indices.
    let mut last: Option<([f64; 2], bool, [usize; 3])> = None;
    points.map(|point| {
        let skew = (point[0] + point[1]) * skew_factor;
        let floor = [(point[0] + skew).floor(), (point[1] + skew).floor()];
        let unskew = (floor[0] + floor[1]) * unskew_factor;
        let offset1 = [
            point[0] - (floor[0] - unskew),
            point[1] - (floor[1] - unskew),
        ];
        let lower = offset1[0] > offset1[1];
        let (order, step) = if lower {
            ([1.0, 0.0], [1, 0])
        } else {
            ([0.0, 1.0], [0, 1])
        };
        let offset2 = [
            offset1[0] - order[0] + unskew_factor,
            offset1[1] - order[1] + unskew_factor,
        ];
        let offset3 = [
            offset1[0] - 1.0 + 2.0 * unskew_factor,
            offset1[1] - 1.0 + 2.0 * unskew_factor,
        ];
        #[allow(clippy::float_cmp, reason = "the same lattice cell, to the bit")]
        let shared = last.filter(|(f, l, _)| *f == floor && *l == lower);
        let indices = shared.map_or_else(
            || {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "lattice cells are far inside isize for any point a world can place"
                )]
                let cell = [floor[0] as isize, floor[1] as isize];
                let indices = [
                    hasher.hash(&cell),
                    hasher.hash(&[cell[0] + step[0], cell[1] + step[1]]),
                    hasher.hash(&[cell[0] + 1, cell[1] + 1]),
                ];
                last = Some((floor, lower, indices));
                indices
            },
            |(.., indices)| indices,
        );
        let mut value = 0.0;
        let mut gradient = [0.0, 0.0];
        for (index, p) in indices.into_iter().zip([offset1, offset2, offset3]) {
            let t = 1.0 - (p[0] * p[0] + p[1] * p[1]) * 2.0;
            if t > 0.0 {
                let g = grad2(index);
                let t2 = t * t;
                let t4 = t2 * t2;
                let dot = p[0] * g[0] + p[1] * g[1];
                let falloff = 2.0 * t2 + t4;
                value += falloff * dot;
                let bend = -16.0 * t * (1.0 + t2) * dot;
                gradient[0] += falloff * g[0] + bend * p[0];
                gradient[1] += falloff * g[1] + bend * p[1];
            }
        }
        (value, gradient)
    })
}

/// The `noise` crate's eight 2D gradients, in its order.
fn grad2(index: usize) -> [f64; 2] {
    const DIAG: f64 = core::f64::consts::FRAC_1_SQRT_2;
    match index % 8 {
        0 => [1.0, 0.0],
        1 => [-1.0, 0.0],
        2 => [0.0, 1.0],
        3 => [0.0, -1.0],
        4 => [DIAG, DIAG],
        5 => [-DIAG, DIAG],
        6 => [DIAG, -DIAG],
        _ => [-DIAG, -DIAG],
    }
}

/// Hermite smoothstep from `0` at `edge0` to `1` at `edge1`, clamped.
#[must_use]
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_stay_in_their_ranges() {
        for (shape, lo, hi) in [
            (Shape::Fbm, -1.0, 1.0),
            (Shape::Ridged, 0.0, 1.0),
            (Shape::Billow, 0.0, 1.0),
        ] {
            let fractal = Fractal::new(7, shape, 100.0, 5, 0.5, 2.0);
            let mut min = f64::MAX;
            let mut max = f64::MIN;
            for i in 0..200 {
                for j in 0..200 {
                    let v = fractal.get(f64::from(i) * 13.7, f64::from(j) * 11.3);
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            assert!(min >= lo && max <= hi, "{shape:?}: {min}..{max}");
            assert!(max - min > 0.3, "{shape:?} is not flat: {min}..{max}");
        }
    }

    #[test]
    fn the_lattice_does_not_repeat_every_256_cells() {
        let fractal = Fractal::new(3, Shape::Fbm, 1.0, 1, 0.5, 2.0);
        let mut same = 0;
        for i in 0..64 {
            let x = f64::from(i) * 0.37 + 0.1;
            let a = fractal.get(x, 0.2);
            let b = fractal.get(x + 256.0, 0.2);
            if (a - b).abs() < 1e-12 {
                same += 1;
            }
        }
        assert!(same < 4, "{same} of 64 samples repeated after 256 cells");
    }

    #[test]
    fn a_truncated_fractal_tracks_the_full_one() {
        let full = Fractal::new(11, Shape::Fbm, 4_000.0, 4, 0.5, 2.0);
        let coarse = full.truncated(2);
        let mut worst = 0.0f64;
        for k in 0..500 {
            let x = f64::from(k) * 97.0;
            worst = worst.max((full.get(x, 13.0) - coarse.get(x, 13.0)).abs());
        }
        // The dropped octaves carry a third of the amplitude at most, so a
        // renormalised truncation would be off by far more than this.
        assert!(worst < 0.25, "truncation drifts by {worst}");
    }

    #[test]
    fn different_seeds_give_different_noise() {
        let a = Fractal::new(1, Shape::Fbm, 100.0, 3, 0.5, 2.0);
        let b = Fractal::new(2, Shape::Fbm, 100.0, 3, 0.5, 2.0);
        assert!((a.get(12.5, 33.0) - b.get(12.5, 33.0)).abs() > 1e-9);
        assert!((a.get(12.5, 33.0) - a.get(12.5, 33.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn the_gradient_kernel_is_the_crates_value_bit_for_bit() {
        let hash = LatticeHash::new(99);
        for i in 0..2_000 {
            let point = [f64::from(i) * 0.731 - 400.0, f64::from(i) * -0.377 + 90.0];
            let (ours, _) = simplex_2d_gradient(point, &hash);
            let (theirs, _) = simplex_2d(Vector2::new(point[0], point[1]), &hash);
            assert_eq!(ours.to_bits(), theirs.to_bits(), "at {point:?}");
        }
    }

    #[test]
    fn the_analytic_gradient_matches_finite_differences() {
        for shape in [Shape::Fbm, Shape::Ridged, Shape::Billow] {
            let fractal = Fractal::new(11, shape, 300.0, 5, 0.5, 2.0);
            let h = 1e-4;
            let mut checked = 0;
            for i in 0..400 {
                let (x, z) = (f64::from(i) * 37.3 + 0.17, f64::from(i) * 11.9 - 3.1);
                let (value, (gx, gz)) = fractal.get_with_gradient(x, z);
                assert!(value.to_bits() == fractal.get(x, z).to_bits(), "{shape:?}");
                let fx = (fractal.get(x + h, z) - fractal.get(x - h, z)) / (2.0 * h);
                let fz = (fractal.get(x, z + h) - fractal.get(x, z - h)) / (2.0 * h);
                // A kink (|n| at zero, the ridged weight at its clamp) inside
                // the step is not a disagreement; skip the rare ones.
                let scale = fx.abs().max(fz.abs()).max(1e-3);
                if (gx - fx).abs() / scale < 1e-3 && (gz - fz).abs() / scale < 1e-3 {
                    checked += 1;
                }
            }
            assert!(checked > 390, "{shape:?}: only {checked} of 400 agree");
        }
    }

    /// The shapes a province may choose stay in their ranges, are not flat,
    /// and carry their gradient: the analytic one against central
    /// differences, round and stretched, with the blendable parameters set.
    #[test]
    fn the_province_shapes_stay_in_range_and_carry_their_gradient() {
        let tunes = [
            Tune::DEFAULT,
            Tune {
                damping: 3.0,
                offset: 0.3,
            },
            Tune {
                damping: 0.5,
                offset: 1.2,
            },
        ];
        for shape in [Shape::Damped, Shape::Hybrid, Shape::Cells] {
            for (stretch, tune) in [(1.0, tunes[0]), (1.0, tunes[1]), (4.0, tunes[2])] {
                let fractal =
                    Fractal::new(23, shape, 400.0, 5, 0.5, 2.0).stretched((0.6, 0.8), stretch);
                let (lo, hi) = if shape.signed() {
                    (-1.0, 1.0)
                } else {
                    (0.0, 1.0)
                };
                let (mut min, mut max) = (f64::MAX, f64::MIN);
                let h = 1e-4;
                let mut agree = 0;
                for i in 0..600 {
                    let (x, z) = (f64::from(i) * 37.3 + 0.17, f64::from(i) * -21.9 + 3.1);
                    let (value, (gx, gz)) = fractal.get_with_gradient_tuned(x, z, tune);
                    assert_eq!(
                        value.to_bits(),
                        fractal.get_tuned(x, z, tune).to_bits(),
                        "{shape:?}"
                    );
                    min = min.min(value);
                    max = max.max(value);
                    let v = |x, z| fractal.get_tuned(x, z, tune);
                    let fx = (v(x + h, z) - v(x - h, z)) / (2.0 * h);
                    let fz = (v(x, z + h) - v(x, z - h)) / (2.0 * h);
                    let scale = fx.abs().max(fz.abs()).max(1e-3);
                    if (gx - fx).abs() / scale < 1e-3 && (gz - fz).abs() / scale < 1e-3 {
                        agree += 1;
                    }
                }
                assert!(min >= lo && max <= hi, "{shape:?} {tune:?}: {min}..{max}");
                assert!(max - min > 0.3, "{shape:?} {tune:?} is flat: {min}..{max}");
                assert!(
                    agree > 590,
                    "{shape:?} {tune:?} x{stretch}: {agree} of 600 agree"
                );
            }
        }
    }

    /// The kernel's second derivatives against central differences of its
    /// gradient.
    #[test]
    #[allow(
        clippy::many_single_char_names,
        reason = "differences, named as in the formula"
    )]
    fn the_kernel_hessian_is_its_gradients_slope() {
        let hash = LatticeHash::new(5);
        let h = 1e-6;
        for i in 0..500 {
            let p = [f64::from(i) * 0.137 - 20.0, f64::from(i) * 0.291 + 4.0];
            let (value, gradient, hessian) = simplex_2d_hessian(p, &hash);
            let (v2, g2) = simplex_2d_gradient(p, &hash);
            assert_eq!(value.to_bits(), v2.to_bits());
            assert_eq!(gradient.map(f64::to_bits), g2.map(f64::to_bits));
            let gx = |dx: f64, dz: f64| simplex_2d_gradient([p[0] + dx, p[1] + dz], &hash).1;
            let (a, b) = (gx(h, 0.0), gx(-h, 0.0));
            let (c, d) = (gx(0.0, h), gx(0.0, -h));
            let fd = [
                (a[0] - b[0]) / (2.0 * h),
                (c[0] - d[0]) / (2.0 * h),
                (c[1] - d[1]) / (2.0 * h),
            ];
            for k in 0..3 {
                assert!(
                    (fd[k] - hessian[k]).abs() < 1e-4,
                    "{fd:?} against {hessian:?} at {p:?}"
                );
            }
        }
    }

    /// Points evaluated together are each point's own evaluation, to the
    /// bit, whether they share a simplex or not.
    #[test]
    fn many_points_are_each_its_own() {
        let fbm = Fractal::new(17, Shape::Fbm, 5_000.0, 2, 0.4, 2.0);
        for i in 0..3_000 {
            let (x, z) = (
                f64::from(i) * 173.1 - 90_000.0,
                f64::from(i) * -61.7 + 400.0,
            );
            // A tap 100 m on, another a quarter of a lattice cell on.
            let points = [(x, z), (x + 100.0, z), (x, z + 1_250.0)];
            for (got, &(px, pz)) in fbm.fbm_with_gradient_many(points).iter().zip(&points) {
                let want = fbm.get_with_gradient(px, pz);
                assert_eq!(
                    (got.0.to_bits(), got.1.0.to_bits(), got.1.1.to_bits()),
                    (want.0.to_bits(), want.1.0.to_bits(), want.1.1.to_bits()),
                    "at ({px}, {pz})"
                );
            }
        }
    }

    /// A ring of taps is each tap's own value, to the bit, a cellular
    /// field's (whose taps are worked out together) included.
    #[test]
    fn a_ring_is_its_taps() {
        let tune = Tune {
            damping: 1.5,
            offset: 0.7,
        };
        for shape in [
            Shape::Fbm,
            Shape::Ridged,
            Shape::Billow,
            Shape::Damped,
            Shape::Hybrid,
            Shape::Cells,
        ] {
            let fractal = Fractal::new(23, shape, 600.0, 2, 0.5, 2.0).stretched((0.6, 0.8), 2.0);
            for i in 0..2_000 {
                let (x, z) = (
                    f64::from(i) * 91.3 - 40_000.0,
                    f64::from(i) * -37.1 + 9_000.0,
                );
                let ring = fractal.ring_tuned(x, z, 250.0, tune);
                let taps = [
                    (0.0, 0.0),
                    (250.0, 0.0),
                    (-250.0, 0.0),
                    (0.0, 250.0),
                    (0.0, -250.0),
                ]
                .map(|(dx, dz)| fractal.get_tuned(x + dx, z + dz, tune));
                for (a, b) in ring.iter().zip(taps) {
                    assert_eq!(a.to_bits(), b.to_bits(), "{shape:?} at ({x}, {z})");
                }
            }
        }
    }

    /// Damping lays the mid-slopes down: a damped field has more of its
    /// ground nearly level, and more of it steep, than the same fbm.
    #[test]
    fn damping_makes_benches_and_bluffs() {
        let fbm = Fractal::new(9, Shape::Fbm, 1_000.0, 5, 0.5, 2.0);
        let damped = Fractal::new(9, Shape::Damped, 1_000.0, 5, 0.5, 2.0);
        let tune = Tune {
            damping: 3.0,
            offset: 0.0,
        };
        let (mut flat, mut flat_fbm) = (0, 0);
        let mut slopes = Vec::new();
        let mut slopes_fbm = Vec::new();
        for i in 0..200 {
            for j in 0..200 {
                let (x, z) = (f64::from(i) * 25.0, f64::from(j) * 25.0);
                let (_, (gx, gz)) = damped.get_with_gradient_tuned(x, z, tune);
                let (_, (fx, fz)) = fbm.get_with_gradient(x, z);
                let (s, sf) = ((gx * gx + gz * gz).sqrt(), (fx * fx + fz * fz).sqrt());
                slopes.push(s);
                slopes_fbm.push(sf);
            }
        }
        let mean = |v: &[f64]| v.iter().sum::<f64>() / 40_000.0;
        let (m, mf) = (mean(&slopes), mean(&slopes_fbm));
        for (&s, &sf) in slopes.iter().zip(&slopes_fbm) {
            flat += usize::from(s < 0.25 * m);
            flat_fbm += usize::from(sf < 0.25 * mf);
        }
        let steep = slopes.iter().filter(|&&s| s > 2.5 * m).count();
        let steep_fbm = slopes_fbm.iter().filter(|&&s| s > 2.5 * mf).count();
        assert!(
            flat > flat_fbm * 3 / 2,
            "level: {flat} damped, {flat_fbm} fbm"
        );
        assert!(steep > steep_fbm, "steep: {steep} damped, {steep_fbm} fbm");
    }

    /// A cellular field's lows are closed: along every direction from a
    /// hollow's bottom the ground rises to a rim within three quarters of a cell.
    #[test]
    fn cells_are_closed_hollows() {
        let cells = Fractal::new(4, Shape::Cells, 600.0, 1, 0.5, 2.0);
        let mut hollows = 0;
        for i in 0..40 {
            for j in 0..40 {
                let (x, z) = (f64::from(i) * 60.0, f64::from(j) * 60.0);
                let here = cells.get(x, z);
                if here > 0.02 {
                    continue;
                }
                hollows += 1;
                for a in 0..16 {
                    let angle = f64::from(a) * core::f64::consts::TAU / 16.0;
                    // The highest ground on the ray within 0.75 of a cell.
                    let rim = (1..=30)
                        .map(|k| {
                            let r = f64::from(k) * 0.025 * 600.0;
                            cells.get(x + r * angle.cos(), z + r * angle.sin())
                        })
                        .fold(f64::MIN, f64::max);
                    assert!(
                        rim > here + 0.3,
                        "open at ({x}, {z}) towards {a}: {rim} against {here}"
                    );
                }
            }
        }
        assert!(hollows > 5, "only {hollows} hollow bottoms on the lattice");
    }

    /// A stretched fractal's features are long along its axis: the gradient
    /// points across the axis far more than along it.
    #[test]
    fn a_stretch_lays_features_along_its_axis() {
        let axis = (0.8, 0.6);
        let round = Fractal::new(2, Shape::Ridged, 500.0, 4, 0.5, 2.0);
        let long = round.clone().stretched(axis, 4.0);
        let ratio = |f: &Fractal| {
            let (mut along, mut across) = (0.0, 0.0);
            for i in 0..150 {
                for j in 0..150 {
                    let (_, (gx, gz)) =
                        f.get_with_gradient(f64::from(i) * 31.0, f64::from(j) * 29.0);
                    let a = gx * axis.0 + gz * axis.1;
                    let c = -gx * axis.1 + gz * axis.0;
                    along += a * a;
                    across += c * c;
                }
            }
            (across / along).sqrt()
        };
        let (r, l) = (ratio(&round), ratio(&long));
        assert!(r < 1.3 && l > 3.0, "round {r}, stretched {l}");
        // A stretch of one is the round fractal to the bit.
        let same = round.clone().stretched(axis, 1.0);
        for i in 0..100 {
            let (x, z) = (f64::from(i) * 13.0, f64::from(i) * 7.0);
            assert_eq!(
                round.get(x, z).to_bits(),
                same.get_tuned(x, z, Tune::DEFAULT).to_bits()
            );
        }
    }

    #[test]
    fn smoothstep_is_clamped_and_monotone() {
        assert!(smoothstep(0.0, 1.0, -1.0).abs() < f64::EPSILON);
        assert!((smoothstep(0.0, 1.0, 2.0) - 1.0).abs() < f64::EPSILON);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-12);
        assert!(smoothstep(0.0, 1.0, 0.25) < smoothstep(0.0, 1.0, 0.75));
    }
}
