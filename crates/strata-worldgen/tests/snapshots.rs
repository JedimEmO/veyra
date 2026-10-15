//! Pinned samples. If one of these changes, the world changed for every
//! player with that seed; that is sometimes intended (retuning) and always
//! worth noticing.

mod common;

use strata_worldgen::WorldSource;

#[test]
fn seed_42_samples() {
    let world = common::world(42);
    let points = [
        (0.0, 0.0),
        (1_000.0, 1_000.0),
        (-12_345.0, 6_789.0),
        (20_000.0, -5_000.0),
        (45_000.0, 45_000.0),
        (-30_000.0, -48_000.0),
        (0.5, 0.5),
        (99_999.0, 1.0),
        // In the carve (ticket 0005, step 2): a river's channel below the
        // confluence south of Port Serein, its bank, and a lake's bed. The
        // river moved with its shape (step 3b), and the two points with it
        // from (-1 516, -3 977) and (-1 519, -3 980).
        (-1_587.3, -3_972.5),
        (-1_588.0, -3_977.0),
        (22_340.0, -4_781.0),
    ];
    let samples: Vec<_> = points
        .iter()
        .map(|&(x, z)| ((x, z), world.sample(x, z)))
        .collect();
    // The generator's version rides along, so a re-bless that moves the
    // ground shows whether the number moved with it.
    insta::assert_ron_snapshot!((strata_worldgen::GENERATOR_VERSION, samples));
}

#[test]
fn seed_42_stages() {
    let world = common::world(42);
    let continent = world.continent(20_000.0, -5_000.0);
    let climate = world.climate(20_000.0, -5_000.0, &continent);
    let class = world.classify(&climate);
    let provinces = world.provinces(20_000.0, -5_000.0);
    insta::assert_debug_snapshot!((
        continent,
        climate,
        class,
        world.biome_id(class.best),
        provinces
    ));
}
