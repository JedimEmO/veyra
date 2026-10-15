//! World seeds and the mixing functions that derive sub-seeds from them.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::Rng;

/// The seed a world is generated from.
///
/// One number is the whole world (PLAN principle 1), but the stages inside
/// the generator each want their own stream: continents, climate and every
/// biome's relief must not correlate. [`Seed::derive`] gives each a
/// sub-seed by name, so adding a stage never shifts the others, which is
/// what keeps an old seed's world stable across code changes that only add
/// stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seed(pub u64);

impl Seed {
    /// A sub-seed for `label`, as a raw `u64` for feeding into noise
    /// generators and RNGs.
    ///
    /// The label is hashed (FNV-1a) and mixed with the seed through
    /// splitmix64, so two labels that differ by one character give unrelated
    /// values and the same label always gives the same one.
    #[must_use]
    pub fn derive(&self, label: &str) -> u64 {
        let mut state = self.0 ^ fnv1a(stable_label(label).as_bytes());
        // Two rounds: one so a zero seed with a short label still mixes, and
        // one so the output is not the plain finaliser of the input.
        let first = splitmix64(&mut state);
        mix64(first ^ state)
    }

    /// [`Self::derive`] wrapped as a [`Seed`], for nesting: a biome's seed
    /// derives its relief and its scatter seeds in turn.
    #[must_use]
    pub fn derive_seed(&self, label: &str) -> Self {
        Self(self.derive(label))
    }

    /// An RNG seeded from `label`.
    #[must_use]
    pub fn rng(&self, label: &str) -> Rng {
        Rng::new(self.derive(label))
    }
}

impl From<u64> for Seed {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// One step of splitmix64: advances `state` and returns the output.
///
/// Public because both the RNG and the seed derivation use it, and because
/// it is the standard way to expand one `u64` into a full RNG state.
#[must_use]
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    mix64(*state)
}

/// The splitmix64 finaliser: a bijective avalanche of a `u64`.
#[must_use]
pub const fn mix64(x: u64) -> u64 {
    let z = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The data namespace as it was spelled when the worlds were first made,
/// before the engine was renamed `strata` (ticket 0013).
pub const OLD_NAMESPACE: &str = "scrolls:";

/// `label` as a world's seeds and hashes read it: data ids under their old
/// namespace ([`OLD_NAMESPACE`]), so that renaming the ids moved no world.
/// Every seed and world hash salted with an id reads it through this.
#[must_use]
pub fn stable_label(label: &str) -> Cow<'_, str> {
    if label.contains("strata:") {
        Cow::Owned(label.replace("strata:", OLD_NAMESPACE))
    } else {
        Cow::Borrowed(label)
    }
}

/// FNV-1a over bytes. Not a cryptographic hash and not meant to be; it is
/// stable across Rust versions, which `DefaultHasher` is not.
const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn derive_is_deterministic_and_label_sensitive() {
        let seed = Seed(42);
        assert_eq!(seed.derive("continents"), seed.derive("continents"));
        assert_ne!(seed.derive("continents"), seed.derive("climate"));
        assert_ne!(seed.derive("a"), seed.derive("b"));
        assert_ne!(Seed(1).derive("x"), Seed(2).derive("x"));
    }

    #[test]
    fn zero_seed_and_empty_label_still_mix() {
        assert_ne!(Seed(0).derive(""), 0);
        assert_ne!(Seed(0).derive(""), Seed(0).derive("x"));
    }

    #[test]
    fn known_answer() {
        // Pins the derivation so a refactor that changes it is visible: every
        // stored world seed would generate a different world.
        assert_eq!(Seed(42).derive("continents"), 878_707_403_700_548_579);
    }
}
