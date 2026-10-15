//! Prints every stage of the sampler at one point and one metre to each
//! side of it, for chasing a step or an odd biome by hand:
//! `cargo run -p strata-worldgen --example probe -- 59872 38957 [seed]`.

use strata_core::Seed;
use strata_worldgen::{ProceduralWorld, WorldSource};

fn main() {
    let mut args = std::env::args().skip(1);
    let x: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let z: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::new(Seed(seed), &registries);
    for (dx, dz) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
        let (px, pz) = (x + dx, z + dz);
        let continent = world.continent(px, pz);
        let climate = world.climate(px, pz, &continent);
        let class = world.classify(&climate);
        let sample = world.sample(px, pz);
        let name = |i: usize| {
            registries
                .biomes
                .name_of(world.biome_id(i))
                .map(ToString::to_string)
        };
        println!("({px}, {pz})");
        println!("  {continent:?}");
        println!("  {climate:?}");
        println!(
            "  {class:?} best {:?} second {:?}",
            name(class.best),
            name(class.second)
        );
        println!("  {sample:?}");
    }
}
