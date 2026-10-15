//! A small deterministic random number generator.

use crate::splitmix64;

/// xoshiro256** with a splitmix64-seeded state.
///
/// Our own implementation rather than the `rand` crate because the output
/// stream is part of the world's definition: a seed must produce the same
/// world next year, and a dependency's algorithm choice is not ours to pin.
/// Sixteen lines of arithmetic, no `unsafe`, `Send + Sync`, and cheap to
/// create per task, which is how the generator's stages use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// An RNG whose whole stream is determined by `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        Self {
            s: [
                splitmix64(&mut state),
                splitmix64(&mut state),
                splitmix64(&mut state),
                splitmix64(&mut state),
            ],
        }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// The next 32 random bits, from the high half of a 64-bit output,
    /// which is the better half in this generator.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the shift keeps the high 32 bits"
    )]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `[0, 1)` with 53 bits of precision.
    #[allow(
        clippy::cast_precision_loss,
        reason = "53 bits fit an f64 mantissa exactly"
    )]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[0, 1)` with 24 bits of precision.
    #[allow(
        clippy::cast_precision_loss,
        reason = "24 bits fit an f32 mantissa exactly"
    )]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform in `[0, n)`. `n == 0` returns 0.
    ///
    /// Lemire's multiply-shift without the rejection step: the bias is below
    /// `n / 2^32`, which no use in a world generator can observe.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the shift keeps the high 32 bits"
    )]
    pub fn below(&mut self, n: u32) -> u32 {
        ((u64::from(self.next_u32()) * u64::from(n)) >> 32) as u32
    }

    /// Uniform in `[lo, hi)`. `hi <= lo` returns `lo`.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lo + below(hi - lo) is below hi, which is an i32"
    )]
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        // At most 2^32 - 1, so the conversion cannot fail; the fallback only
        // keeps the function total without an `expect`. The sum is done in
        // i64 because `lo + offset` overflows i32 for the full range.
        let span = u32::try_from(i64::from(hi) - i64::from(lo)).unwrap_or(u32::MAX);
        (i64::from(lo) + i64::from(self.below(span))) as i32
    }

    /// A coin flip.
    pub fn bool(&mut self) -> bool {
        self.next_u64() >> 63 == 1
    }

    /// A fresh generator seeded from this one's stream, for handing to a
    /// sub-task so it does not share (and race on) the parent's state.
    #[must_use]
    pub fn fork(&mut self) -> Self {
        Self::new(self.next_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
    }

    #[test]
    fn reference_vector() {
        // xoshiro256** seeded through splitmix64 from 0, the pairing the
        // reference implementation documents. Pins the algorithm.
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 11_091_344_671_253_066_420);
    }

    #[test]
    fn ranges_hold() {
        let mut rng = Rng::new(3);
        for _ in 0..10_000 {
            let f = rng.next_f64();
            assert!((0.0..1.0).contains(&f));
            let g = rng.next_f32();
            assert!((0.0..1.0).contains(&g));
            assert!(rng.below(10) < 10);
            assert_eq!(rng.below(0), 0);
            let i = rng.range_i32(-5, 5);
            assert!((-5..5).contains(&i));
            assert_eq!(rng.range_i32(3, 3), 3);
        }
        let mut rng = Rng::new(11);
        let extremes = (0..1000).fold((false, false), |acc, _| {
            let v = rng.range_i32(i32::MIN, i32::MAX);
            (acc.0 || v < 0, acc.1 || v > 0)
        });
        assert_eq!(extremes, (true, true));
    }

    #[test]
    fn fork_is_independent_of_the_parent() {
        let mut parent = Rng::new(1);
        let mut child = parent.fork();
        let parent_next = parent.next_u64();
        assert_ne!(child.next_u64(), parent_next);
        // Forking is itself deterministic.
        let mut again = Rng::new(1);
        assert_eq!(again.fork(), Rng::new(1).fork());
    }
}
