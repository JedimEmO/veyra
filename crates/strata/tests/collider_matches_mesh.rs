//! The ground the character stands on is the ground the player sees: the
//! physics crate's heightfield and the terrain crate's mesh are sampled
//! from the same world at the same points, and this is where the two
//! crates, which do not know each other, are held to it.

#![cfg(feature = "client")]

use strata::core::{GRID_N, RegionCoord, Seed};
use strata::physics::Heightfield;
use strata::registry::load_from_dir;
use strata::terrain::{BiomeLayers, build_region_mesh};
use strata::worldgen::ProceduralWorld;

fn assets() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

#[test]
fn the_collider_heights_are_the_mesh_heights_bit_for_bit() {
    let registries = load_from_dir(assets()).expect("the workspace assets load");
    let world = ProceduralWorld::new(Seed(42), &registries);
    let layers = BiomeLayers::from_layers(vec![0; 16]);
    let mut scratch = Vec::new();
    // Two on the sea bed off Port Serein, a mountainside at 1 700 to 1 800 m
    // (the sampler's timed mountain strip), and tundra far out. The
    // mountainside was (312, -78) until ticket 0004's step 2b moved the
    // ground under it to a plain at 300 m, and the one steep place the two
    // are held to agree on went with it; it moved here in that step's
    // review.
    for coord in [
        RegionCoord::new(0, 0, 0),
        RegionCoord::new(3, -2, 0),
        RegionCoord::new(234, -266, 0),
        RegionCoord::new(-1_000, 999, 0),
    ] {
        let mesh = build_region_mesh(coord, &world, &layers, 0.05, &mut scratch);
        let field = Heightfield::sample(coord, &world, &mut scratch);
        for j in 0..GRID_N {
            for i in 0..GRID_N {
                let mesh_height = mesh.positions[j * GRID_N + i][1];
                let collider_height = field.height_at(i, j);
                assert_eq!(
                    mesh_height.to_bits(),
                    collider_height.to_bits(),
                    "{coord:?} vertex ({i}, {j}): mesh {mesh_height}, collider {collider_height}"
                );
            }
        }
    }
}
