//! Gradient noise for the warps and the moss patches: Perlin's lattice with
//! hashed gradients, seeded per rock so no two variants share a pattern.
//!
//! Only additions, multiplications and `floor`, so the same seed gives the
//! same bits everywhere; the rock generator needs nothing smoother.
#![allow(
    clippy::many_single_char_names,
    reason = "Perlin's notation: u, v, w and the lattice corners"
)]
use ashlar::glam::DVec3;

/// Twelve cube-edge gradients, as in improved Perlin noise: they avoid the
/// axis-aligned streaks random unit vectors leave at low octave counts.
const GRADIENTS: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

/// A seeded noise field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Noise {
    seed: u64,
}

impl Noise {
    pub(crate) fn new(seed: u64) -> Self {
        Self { seed }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "lattice coordinates are hashed as bits"
    )]
    fn gradient(self, x: i64, y: i64, z: i64) -> DVec3 {
        let mut h = self.seed
            ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
        h = (h ^ (h >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = (h ^ (h >> 29)).wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 32;
        let [x, y, z] = GRADIENTS[(h % 12) as usize];
        DVec3::new(x, y, z)
    }

    /// One octave, roughly in `-1..1`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "floored lattice coordinates are small integers"
    )]
    pub(crate) fn sample(self, p: DVec3) -> f64 {
        let cell = p.floor();
        let f = p - cell;
        let (ix, iy, iz) = (cell.x as i64, cell.y as i64, cell.z as i64);
        let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v, w) = (fade(f.x), fade(f.y), fade(f.z));
        let corner = |dx: i64, dy: i64, dz: i64| {
            #[allow(clippy::cast_precision_loss, reason = "offsets are 0 or 1")]
            let offset = DVec3::new(dx as f64, dy as f64, dz as f64);
            self.gradient(ix + dx, iy + dy, iz + dz).dot(f - offset)
        };
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u);
        let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u);
        let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u);
        let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u);
        lerp(lerp(x00, x10, v), lerp(x01, x11, v), w)
    }

    /// `octaves` octaves, each twice the frequency and half the amplitude.
    pub(crate) fn fbm(self, p: DVec3, octaves: u32) -> f64 {
        let mut sum = 0.0;
        let mut amplitude = 1.0;
        let mut q = p;
        for octave in 0..octaves {
            let shifted = Self::new(self.seed.wrapping_add(u64::from(octave) * 0x632B_E59B));
            sum += shifted.sample(q) * amplitude;
            amplitude *= 0.5;
            q *= 2.03;
        }
        sum
    }

    /// A vector field of three decorrelated octaves, for domain warps.
    pub(crate) fn vector(self, p: DVec3, octaves: u32) -> DVec3 {
        let a = Self::new(self.seed ^ 0x51ED_2701);
        let b = Self::new(self.seed ^ 0xA3B1_9535);
        DVec3::new(
            self.fbm(p, octaves),
            a.fbm(p + DVec3::new(17.3, 5.1, 9.7), octaves),
            b.fbm(p + DVec3::new(-8.9, 13.7, 2.3), octaves),
        )
    }
}
