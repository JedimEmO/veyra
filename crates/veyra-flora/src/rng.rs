//! The generator's random numbers: a `SplitMix64` stream, so a variant is the
//! same tree on every run and every machine's integer arithmetic.
use ashlar::glam::DVec3;

/// A deterministic stream. Each part of the generator forks its own, so
/// adding a draw to one part does not reshuffle another.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// A stream for one part of the generator, keyed by a name.
    pub(crate) fn fork(&self, salt: &str) -> Self {
        Self::new(self.0 ^ hash(salt).rotate_left(17))
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn unit(&mut self) -> f64 {
        // 53 bits fit an f64 mantissa exactly.
        #[allow(clippy::cast_precision_loss)]
        let fraction = (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    }

    /// Uniform in `[-1, 1)`.
    pub(crate) fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }

    pub(crate) fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    /// An integer in `low..=high`.
    pub(crate) fn between(&mut self, low: u32, high: u32) -> u32 {
        let span = u64::from(high.saturating_sub(low)) + 1;
        low + u32::try_from(self.next_u64() % span).expect("the span came from a u32")
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// A uniformly distributed unit vector.
    pub(crate) fn direction(&mut self) -> DVec3 {
        loop {
            let v = DVec3::new(self.signed(), self.signed(), self.signed());
            let len = v.length_squared();
            if len > 1e-4 && len <= 1.0 {
                return v / len.sqrt();
            }
        }
    }

    /// A point uniformly inside the unit ball.
    pub(crate) fn in_ball(&mut self) -> DVec3 {
        loop {
            let v = DVec3::new(self.signed(), self.signed(), self.signed());
            if v.length_squared() <= 1.0 {
                return v;
            }
        }
    }
}

/// FNV-1a, for turning names into seeds.
pub(crate) fn hash(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}
