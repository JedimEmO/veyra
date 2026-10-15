//! Gullies and ridges carved into a slope, one point at a time.
//!
//! Real mountains are shaped by water running downhill, which leaves
//! branching gullies between sharp ridges. Simulating that needs the whole
//! map at once, and this world is a pure function of the point (ADR 0006
//! turned down a baked grid for the same reason). So this is a filter
//! instead: stripes of directional noise laid across the downhill direction
//! and cut into the ground, octave after octave, each finer octave following
//! the slope of the gullies before it. That is what makes them branch. The
//! idea is runevision's erosion filter (2026) and its directional noise; this
//! is an implementation of the described technique, not a port of its code.
//!
//! The worldgen float rule holds: stripes use a polynomial sine and cosine
//! and the cells blend with a polynomial kernel, so every operation is
//! `+ - * /`, `sqrt` or `floor`, and the answer is the same bits everywhere.
//!
//! Three things keep it from looking like a filter:
//!
//! - **Masking.** Where the ground is flat (a summit, a valley floor) there is
//!   no downhill to follow, so gullies fade out. Each octave also fades on the
//!   crests and creases of the octave before it, so fine gullies cut the
//!   walls of coarse ones and not their floors.
//! - **Fading to a target.** Where a gully fades it does not fade to flat: it
//!   fades to a ridge high up and to a crease low down, so peaks stay sharp
//!   and valleys stay valleys.
//! - **Cutting, not adding.** A stripe's crest is the ground as it was and its
//!   trough is cut below it, so the filter never raises a mountain.

use crate::noise::smoothstep;

/// Cells sampled per axis: a block around the nearest cell corner, which
/// holds every pivot within [`REACH`] of the point.
const CELLS: i64 = 4;

/// How far a pivot's stripes reach, in cells. Any pivot closer than this is
/// in the [`CELLS`] block, and the kernel is zero with zero slope at the
/// reach, so the blend never gains or loses a contributor abruptly. At one
/// and a half cells a point nearly always has a pivot well inside its reach;
/// at one, a fifth of the ground had only faint stripes.
const REACH: f64 = 1.5;

/// How strongly a blended stripe pair is renormalised: past half its full
/// length it counts as full, which keeps stripe amplitude even where cells
/// disagree, without blowing up where they cancel.
const RENORMALISE: f64 = 2.0;

/// The erosion filter's settings, shared by the whole world. Only strength
/// varies by biome; a wavelength or seed that changed at a biome border would
/// move every gully there, and the ground with it.
#[derive(Debug, Clone)]
pub struct Erosion {
    seeds: Vec<u64>,
    wavelength_m: f64,
    slope_reference: f64,
    detail: f64,
}

/// What the filter did at one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eroded {
    /// Metres to add to the height: zero or less, since gullies only cut.
    pub offset_m: f64,
    /// The change the gullies make to the gradient, `(d/dx, d/dz)`. An
    /// estimate: the slopes of the stripes without the slopes of their masks.
    pub gradient: (f64, f64),
    /// The finest octave's shape: `-1` at the bottom of a gully, `1` on a
    /// ridge, `0` where nothing was carved.
    pub ridge: f64,
}

impl Eroded {
    /// The filter at a point it leaves alone.
    pub const NONE: Self = Self {
        offset_m: 0.0,
        gradient: (0.0, 0.0),
        ridge: 0.0,
    };
}

impl Erosion {
    /// A filter with its own seed per octave. `wavelength_m` is the spacing of
    /// the coarsest gullies; each octave after it halves. `slope_reference` is
    /// the gradient at which gullies reach full depth, and `detail` how much
    /// each octave's crests and creases hold back the next one (higher keeps
    /// more fine detail).
    #[must_use]
    pub fn new(seeds: Vec<u64>, wavelength_m: f64, slope_reference: f64, detail: f64) -> Self {
        Self {
            seeds,
            wavelength_m,
            slope_reference,
            detail,
        }
    }

    /// Carve the point `(x, z)` whose ground has gradient `gradient` (metres
    /// up per metre, `(d/dx, d/dz)`). `strength` scales the depth; zero is
    /// the ground untouched. `height` places the point between the valley
    /// (`0`) and the peak (`1`) of its relief, for what gullies fade to.
    #[must_use]
    pub fn carve(
        &self,
        x: f64,
        z: f64,
        gradient: (f64, f64),
        strength: f64,
        height: f64,
    ) -> Eroded {
        if strength <= 0.0 {
            return Eroded::NONE;
        }
        let mut offset = 0.0;
        let (mut gx, mut gz) = (0.0, 0.0);
        let mut mask = 1.0;
        // High ground fades to a ridge, low ground to a crease.
        let mut target = height.clamp(0.0, 1.0) * 2.0 - 1.0;
        let mut wavelength = self.wavelength_m;
        for &seed in &self.seeds {
            // The slope the next gullies follow: the ground's, bent by the
            // gullies already cut.
            let (sx, sz) = (gradient.0 + gx, gradient.1 + gz);
            let slope = (sx * sx + sz * sz).sqrt();
            if slope < 1e-9 {
                break;
            }
            let across = (-sz / slope, sx / slope);
            let (cos, sin) = stripes(seed, x, z, across, wavelength);
            let steep = ease_out(slope / self.slope_reference);
            let weight = mask * steep;
            let gully = target + (cos - target) * weight;
            // A gully as deep as the ground falls over half its width, so
            // its walls are about as steep as the slope it is cut into.
            let depth = strength * slope * wavelength / core::f64::consts::TAU;
            offset += (gully - 1.0) * 0.5 * depth;
            // d/dp of cos(2 pi p / wavelength) is -sin times 2 pi / wavelength,
            // and depth carries wavelength / 2 pi, so the wall slope is the
            // ground's slope times the strength.
            let wall = -sin * weight * 0.5 * strength * slope;
            gx += wall * across.0;
            gz += wall * across.1;
            // The next octave keeps off this one's crests and creases.
            mask = pow_inv(mask, self.detail) * ease_out(sin.abs());
            target = gully;
            wavelength *= 0.5;
        }
        Eroded {
            offset_m: offset,
            gradient: (gx, gz),
            ridge: target,
        }
    }
}

/// Directional stripes at `(x, z)`: the blended cosine and sine of a wave
/// running along `across`, one pivot per cell, renormalised so the stripes
/// keep their amplitude where neighbouring cells disagree.
fn stripes(seed: u64, x: f64, z: f64, across: (f64, f64), wavelength: f64) -> (f64, f64) {
    let cell = wavelength;
    let (cell_x, cell_z) = (x / cell, z / cell);
    // The block around the nearest cell corner: the point is at least one
    // and a half cells from its edge in every direction. Cell indices are
    // far inside i64 for any world a float can place.
    #[allow(clippy::cast_possible_truncation)]
    let (i0, j0) = (
        (cell_x - 0.5).floor() as i64 - 1,
        (cell_z - 0.5).floor() as i64 - 1,
    );
    let (mut cos_sum, mut sin_sum, mut total) = (0.0, 0.0, 0.0);
    for j in j0..j0 + CELLS {
        for i in i0..i0 + CELLS {
            let (ox, oz) = pivot(seed, i, j);
            // i64 cell indices convert exactly within any reachable world.
            #[allow(clippy::cast_precision_loss)]
            let (dx, dz) = (cell_x - (i as f64 + ox), cell_z - (j as f64 + oz));
            let d2 = dx * dx + dz * dz;
            let reach = REACH * REACH;
            if d2 >= reach {
                continue;
            }
            // A compact bell: one at the pivot, zero with zero slope at the
            // reach, so a pivot entering the blend moves nothing.
            let fall = 1.0 - d2 / reach;
            let weight = fall * fall * fall;
            // The wave is anchored at the pivot, so each cell's stripes are
            // offset from its neighbours'.
            let phase = (dx * across.0 + dz * across.1) * cell / wavelength;
            let (sin, cos) = wave(phase);
            cos_sum += cos * weight;
            sin_sum += sin * weight;
            total += weight;
        }
    }
    // Divide by the weight, but never by less than one pivot's full weight:
    // a point with only a far pivot in reach would otherwise take that
    // pivot's stripes at full strength, and snap to the next pivot's as the
    // first leaves. Where pivots are sparse the stripes fade instead.
    let (c, s) = (cos_sum / total.max(1.0), sin_sum / total.max(1.0));
    let length = (c * c + s * s).sqrt();
    if length < 1e-12 {
        return (0.0, 0.0);
    }
    let scale = (length * RENORMALISE).min(1.0) / length;
    (c * scale, s * scale)
}

/// A cell's pivot, somewhere inside it, from the octave's seed.
fn pivot(seed: u64, i: i64, j: i64) -> (f64, f64) {
    // Two's-complement reinterpretation: negative cells hash as well as
    // positive ones, and the bits are all that matter.
    #[allow(clippy::cast_sign_loss)]
    let h = mix(seed
        ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    // The two halves of one hash are two independent 32-bit fractions.
    (unit32(h >> 32), unit32(h & 0xFFFF_FFFF))
}

/// `splitmix64`'s finaliser: every input bit reaches every output bit.
const fn mix(mut h: u64) -> u64 {
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// A 32-bit hash as a fraction in `[0, 1)`: fine enough for where in its
/// cell a pivot sits.
fn unit32(h: u64) -> f64 {
    // 32 bits fit an f64 mantissa exactly.
    #[allow(clippy::cast_precision_loss)]
    let fraction = h as f64 / 4_294_967_296.0;
    fraction
}

/// A stripe's wave at `turns`: `sin` and `cos` of a whole turn, closely
/// enough for a gully and cheaply, as a parabola with one correction (about
/// a thousandth off, never more), made only of operations that round the
/// same on every platform. Stripes run this for every pivot in reach of
/// every octave, and a Taylor series there was a third of the filter.
#[must_use]
pub fn wave(turns: f64) -> (f64, f64) {
    (parabolic_sin(turns), parabolic_sin(turns + 0.25))
}

/// `sin(2 pi turns)` as a corrected parabola.
fn parabolic_sin(turns: f64) -> f64 {
    // Into [-1, 1), where sin(pi x) is a parabola 4x(1 - |x|) to within a
    // few per cent, and the correction takes that to a tenth of one.
    let x = (turns - (turns + 0.5).floor()) * 2.0;
    let y = 4.0 * x * (1.0 - x.abs());
    0.225 * (y * y.abs() - y) + y
}

/// `1 - (1 - t)^2` over `t` clamped to `0..=1`: rises fast, then levels off.
fn ease_out(t: f64) -> f64 {
    let v = 1.0 - t.clamp(0.0, 1.0);
    1.0 - v * v
}

/// `1 - (1 - t)^power` over `t` clamped to `0..=1`, for a power that may not
/// be whole: the integer part by repeated multiplication and the fraction
/// by blending towards the next power, so no `powf`.
fn pow_inv(t: f64, power: f64) -> f64 {
    let v = 1.0 - t.clamp(0.0, 1.0);
    let whole = power.floor().max(0.0);
    let mut p = 1.0;
    // A handful at most; `detail` is a small number.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for _ in 0..whole as u32 {
        p *= v;
    }
    let p = p + (p * v - p) * (power - whole);
    1.0 - p
}

/// Where between its valley and its peak a relief value sits, `0..=1`.
#[must_use]
pub fn height_share(relief: f64, low: f64, high: f64) -> f64 {
    smoothstep(low, high, relief)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stripe_wave_is_close_to_a_sine() {
        for step in -400..=400 {
            let turns = f64::from(step) / 97.0;
            let (sin, cos) = wave(turns);
            let angle = turns * core::f64::consts::TAU;
            assert!((sin - libm::sin(angle)).abs() < 2e-3, "sin {turns}");
            assert!((cos - libm::cos(angle)).abs() < 2e-3, "cos {turns}");
        }
    }

    #[test]
    fn stripes_keep_their_amplitude() {
        let mut total = 0.0;
        let mut samples = 0.0;
        for i in 0..200 {
            for j in 0..200 {
                // Thousands of cells, so the mean is the filter's and not a
                // few pivots'.
                let (c, s) = stripes(7, f64::from(i) * 3.1, f64::from(j) * 2.7, (0.6, 0.8), 10.0);
                let length = (c * c + s * s).sqrt();
                assert!(length <= 1.0 + 1e-9);
                total += length;
                samples += 1.0;
            }
        }
        // What is missing from one is where neighbouring cells' waves cross
        // and partly cancel: blending unaligned stripes always costs that.
        // Measured over thousands of cells it is 0.82.
        assert!(total / samples > 0.8, "mean length {}", total / samples);
    }

    fn filter() -> Erosion {
        Erosion::new(vec![1, 2, 3, 4], 400.0, 0.4, 1.5)
    }

    #[test]
    fn gullies_only_cut() {
        let erosion = filter();
        for i in 0..300 {
            let x = f64::from(i) * 13.7;
            let eroded = erosion.carve(x, x * 0.3, (0.5, 0.2), 1.0, 0.5);
            assert!(eroded.offset_m <= 0.0, "{eroded:?}");
            assert!((-1.0..=1.0).contains(&eroded.ridge));
        }
    }

    #[test]
    fn flat_ground_and_zero_strength_are_left_alone() {
        let erosion = filter();
        assert_eq!(
            erosion.carve(10.0, 20.0, (0.0, 0.0), 1.0, 0.5),
            Eroded::NONE
        );
        assert_eq!(
            erosion.carve(10.0, 20.0, (0.6, 0.1), 0.0, 0.5),
            Eroded::NONE
        );
    }

    #[test]
    fn the_filter_is_continuous() {
        // Neighbouring points a centimetre apart differ by centimetres, not
        // metres: no cell entering the blend moves the ground.
        let erosion = filter();
        let mut previous = erosion.carve(0.0, 0.0, (0.5, 0.3), 1.0, 0.5).offset_m;
        for i in 1..20_000 {
            let x = f64::from(i) * 0.01;
            let here = erosion.carve(x, x * 0.7, (0.5, 0.3), 1.0, 0.5).offset_m;
            assert!(
                (here - previous).abs() < 0.05,
                "at {x}: {previous} to {here}"
            );
            previous = here;
        }
    }

    #[test]
    fn a_fractional_power_blends_between_whole_ones() {
        assert!((pow_inv(0.5, 1.0) - 0.5).abs() < 1e-12);
        assert!((pow_inv(0.5, 2.0) - 0.75).abs() < 1e-12);
        let half = pow_inv(0.5, 1.5);
        assert!(half > 0.5 && half < 0.75);
    }
}
