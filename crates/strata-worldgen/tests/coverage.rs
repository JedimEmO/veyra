//! Every shipped biome shows up somewhere on a world-sized map, and the map
//! is neither all sea nor all land.

mod common;

use std::collections::BTreeMap;

use strata_worldgen::WorldSource;

#[test]
fn every_biome_appears_on_a_100_km_map() {
    let registries = common::registries();
    let world = common::world(42);
    let mut counts: BTreeMap<_, usize> = BTreeMap::new();
    let mut submerged = 0;
    let mut total = 0;
    for j in -50..=50 {
        for i in -50..=50 {
            let s = world.sample(f64::from(i) * 1_000.0, f64::from(j) * 1_000.0);
            *counts.entry(s.dominant_biome()).or_default() += 1;
            submerged += usize::from(s.is_submerged(world.sea_level()));
            total += 1;
        }
    }
    for (id, name, _) in registries.biomes.iter() {
        let count = counts.get(&id).copied().unwrap_or(0);
        assert!(count > 0, "{name} never wins a point");
    }
    let sea = submerged * 100 / total;
    assert!((20..=80).contains(&sea), "{sea}% of the map is under water");
}

#[test]
fn mountains_reach_the_plan_height_and_plains_stay_low() {
    let registries = common::registries();
    let world = common::world(42);
    let mountains = registries
        .biome_id(&strata_data::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("ships");
    let plains = registries
        .biome_id(&strata_data::Namespaced::parse("strata:plains").expect("valid"))
        .expect("ships");
    let mut peak = f32::MIN;
    let mut plains_max = f32::MIN;
    for j in -250..=250 {
        for i in -250..=250 {
            let s = world.sample(f64::from(i) * 200.0, f64::from(j) * 200.0);
            if s.dominant_biome() == mountains {
                peak = peak.max(s.height);
            } else if s.dominant_biome() == plains {
                plains_max = plains_max.max(s.height);
            }
        }
    }
    assert!(
        (1_500.0..=3_500.0).contains(&peak),
        "highest mountain sample is {peak} m"
    );
    assert!(
        plains_max < 400.0,
        "highest plains sample is {plains_max} m"
    );
}

/// The land within 100 km of the origin keeps its variety (ticket 0004,
/// step 2b, which moved the climate's equator so that Port Serein's country
/// is temperate): every land biome the registry ships is at least 2 % of
/// the land and none more than 35 %. A desert still exists; it is no longer
/// where the city is. Land is the measures' (over 2 m), on a 500 m lattice
/// in the disc. After step 2b the least is savanna at 3.0 % and the most
/// tundra at 22.6 % (22.4 % since its review); with the equator moved and
/// the pole distance kept at 55 km, tundra had 28.6 % and savanna 2.3 %.
#[test]
fn the_land_within_100_km_keeps_its_variety() {
    let registries = common::registries();
    let world = common::world(42);
    let step = 500.0;
    let rows: Vec<Vec<strata_data::BiomeId>> = std::thread::scope(|scope| {
        let world = &world;
        let workers: Vec<_> = (-200_i32..=200)
            .collect::<Vec<_>>()
            .chunks(26)
            .map(|rows| {
                let rows = rows.to_vec();
                scope.spawn(move || {
                    rows.iter()
                        .map(|&j| {
                            (-200_i32..=200)
                                .filter_map(|i| {
                                    let (x, z) = (f64::from(i) * step, f64::from(j) * step);
                                    if x.hypot(z) > 100_000.0 {
                                        return None;
                                    }
                                    let s = world.sample(x, z);
                                    (f64::from(s.height) > strata_worldgen::measure::LAND_MIN_M)
                                        .then(|| s.dominant_biome())
                                })
                                .collect()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a sampling thread panicked"))
            .collect()
    });
    let mut counts: BTreeMap<_, usize> = BTreeMap::new();
    let mut land = 0_usize;
    for id in rows.into_iter().flatten() {
        *counts.entry(id).or_default() += 1;
        land += 1;
    }
    let ocean = registries
        .biome_id(&strata_data::Namespaced::parse("strata:ocean").expect("valid"))
        .expect("ships");
    for (id, name, _) in registries.biomes.iter() {
        if id == ocean {
            continue;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts of a hundred thousand or so, exact in f64"
        )]
        let share = counts.get(&id).copied().unwrap_or(0) as f64 / land as f64;
        assert!(
            (0.02..=0.35).contains(&share),
            "{name} is {:.1} % of the land within 100 km",
            share * 100.0
        );
    }
}
