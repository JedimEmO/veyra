//! The vocabulary every other crate in sci-fi-scrolls speaks: `f64` world
//! positions, the region grid the terrain and the server share, seeds that
//! derive sub-seeds by name, a small deterministic RNG and the simulation
//! clock.
//!
//! Nothing here depends on Bevy or does IO. The server, the client, the map
//! tool and the tests all use these types, and the world being a pure
//! function of the seed (PLAN section 2) starts with the seed and the
//! coordinates being plain, deterministic values.
//!
//! - [`WorldPos`] is the only position type below the render layer (ADR 0002).
//! - [`RegionCoord`] is a square of the world at one LOD (ADR 0003).
//! - [`Seed`] and [`Rng`] make "the same seed gives the same world" testable.
//! - [`SimTime`] is the fixed-step clock the simulation advances.

pub mod platform;
pub mod pos;
pub mod region;
pub mod rng;
pub mod seed;
pub mod time;

pub use glam;
pub use pos::WorldPos;
pub use region::{GRID_N, MAX_LOD, REGION_BASE_SIZE, RegionCoord};
pub use rng::Rng;
pub use seed::{OLD_NAMESPACE, Seed, mix64, splitmix64, stable_label};
pub use time::{SIM_TICK_HZ, SimTime};

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn a_seed_names_a_region() {
        let seed = Seed(42);
        let mut rng = seed.rng("smoke");
        let region = RegionCoord::new(rng.range_i32(-100, 100), rng.range_i32(-100, 100), 3);
        assert!(region.contains(region.center()));
        assert_eq!(RegionCoord::containing(region.center(), 3), region);
    }
}
