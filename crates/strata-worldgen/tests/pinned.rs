//! The world's identity is pinned: moving code between crates must not move a
//! world. These are the values the shipped data and `WorldParams::default()`
//! gave before the registry was split, so a change here is a changed world
//! (bump `GENERATOR_VERSION`) and never a refactor.

mod common;

use strata_core::Seed;
use strata_worldgen::version::water_stamp;
use strata_worldgen::{WorldParams, world_version};

#[test]
fn the_shipped_world_keeps_its_version_and_water_stamps() {
    let registries = common::registries();
    let params = WorldParams::default();
    assert_eq!(
        world_version(&params, &registries),
        16_403_434_710_341_527_553
    );
    assert_eq!(
        water_stamp(Seed(42), &params, &registries),
        3_758_177_629_584_053_766
    );
    assert_eq!(
        water_stamp(Seed(7), &params, &registries),
        17_688_419_056_617_006_875
    );
}
