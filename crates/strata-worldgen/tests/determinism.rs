//! The world is a pure function of the seed: same seed, same bits, from any
//! thread, from any number of independently constructed samplers.

mod common;

use proptest::prelude::*;
use rayon::prelude::*;
use strata_worldgen::{Sample, WorldSource};

/// Bitwise comparison: `PartialEq` on floats would also accept `-0.0 ==
/// 0.0`, and a mesh built from `-0.0` is not the same bytes.
fn same_bits(a: &Sample, b: &Sample) -> bool {
    a.height.to_bits() == b.height.to_bits()
        && a.moisture.to_bits() == b.moisture.to_bits()
        && a.hydration.to_bits() == b.hydration.to_bits()
        && a.cover.to_bits() == b.cover.to_bits()
        && a.material == b.material
        && a.biomes
            .iter()
            .zip(b.biomes.iter())
            .all(|((ia, wa), (ib, wb))| ia == ib && wa.to_bits() == wb.to_bits())
        && a.provinces
            .iter()
            .zip(b.provinces.iter())
            .all(|((ia, wa), (ib, wb))| ia == ib && wa.to_bits() == wb.to_bits())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn two_worlds_from_one_seed_agree_everywhere(
        x in -200_000.0f64..200_000.0,
        z in -200_000.0f64..200_000.0,
    ) {
        let a = common::world_within(42, 200_000.0);
        let b = common::world_within(42, 200_000.0);
        prop_assert!(same_bits(&a.sample(x, z), &b.sample(x, z)));
    }

    #[test]
    fn a_sample_is_finite_and_its_weights_sum_to_one(
        x in -200_000.0f64..200_000.0,
        z in -200_000.0f64..200_000.0,
    ) {
        let world = common::world_within(7, 200_000.0);
        let s = world.sample(x, z);
        prop_assert!(s.height.is_finite());
        prop_assert!(s.height > -5_000.0 && s.height < 5_000.0, "{}", s.height);
        prop_assert!((0.0..=1.0).contains(&s.moisture), "{}", s.moisture);
        prop_assert!((0.0..=1.0).contains(&s.hydration), "{}", s.hydration);
        prop_assert!((0.0..=1.0).contains(&s.cover), "{}", s.cover);
        let [(_, w0), (_, w1)] = s.biomes;
        prop_assert!(w0 >= w1, "best biome first: {w0} < {w1}");
        prop_assert!((w0 + w1 - 1.0).abs() < 1e-5, "{w0} + {w1}");
    }
}

#[test]
fn parallel_sampling_matches_serial_sampling() {
    let world = common::world(42);
    let n = 128;
    let step = 37.0;
    let mut serial = Vec::new();
    world.sample_grid(-3_000.0, 12_000.0, step, n, &mut serial);

    let parallel: Vec<Sample> = common::sweep(|| {
        (0..n * n)
            .into_par_iter()
            .map(|k| {
                let (i, j) = (k % n, k / n);
                // Indices are tiny; the cast is exact.
                #[allow(clippy::cast_precision_loss)]
                world.sample(-3_000.0 + i as f64 * step, 12_000.0 + j as f64 * step)
            })
            .collect()
    });

    assert_eq!(serial.len(), parallel.len());
    for (k, (a, b)) in serial.iter().zip(&parallel).enumerate() {
        assert!(same_bits(a, b), "sample {k} differs: {a:?} vs {b:?}");
    }
}

#[test]
fn the_grid_is_row_major_with_x_fastest() {
    let world = common::world(3);
    let mut out = Vec::new();
    world.sample_grid(100.0, 200.0, 2.0, 4, &mut out);
    assert_eq!(out.len(), 16);
    assert!(same_bits(&out[0], &world.sample(100.0, 200.0)));
    assert!(same_bits(&out[1], &world.sample(102.0, 200.0)));
    assert!(same_bits(&out[4], &world.sample(100.0, 202.0)));
    assert!(same_bits(&out[15], &world.sample(106.0, 206.0)));
}

#[test]
fn different_seeds_give_different_worlds() {
    let a = common::world(1);
    let b = common::world(2);
    let differing = (0..64)
        .filter(|k| {
            let x = f64::from(*k) * 1_731.0;
            !same_bits(&a.sample(x, -x * 0.5), &b.sample(x, -x * 0.5))
        })
        .count();
    assert!(
        differing > 60,
        "only {differing} of 64 samples differ between seeds"
    );
}

#[test]
fn the_sampler_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<strata_worldgen::ProceduralWorld>();
}

#[test]
fn port_serein_stands_on_its_own_ground_in_any_world() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let def = &registries.settlements[0];
    let started = std::time::Instant::now();
    let world = strata_worldgen::ProceduralWorld::new(strata_core::Seed(42), &registries);
    let site = world
        .sites()
        .first()
        .expect("Port Serein is placed")
        .clone();
    eprintln!(
        "port serein at {:?} facing {:?}, {:.1} m, platforms {:?}, road {} points, placed in {:?}",
        site.centre,
        site.downhill,
        site.height,
        site.platforms
            .iter()
            .map(|p| (p.id.clone(), p.height))
            .collect::<Vec<_>>(),
        site.road.points.len(),
        started.elapsed()
    );
    assert_eq!(site.centre.map(f64::to_bits), def.at.map(f64::to_bits));
    // The rivers round the city first, on the bake's threads: a sample
    // that met them unbaked would bake them on its own thread.
    let [cx, cz] = site.to_world_xz([0.0, 0.0]);
    world.hydrology().set_threads(16);
    world.prepare_water(cx, cz, 2_000.0);
    // Level where the design fixed its platforms.
    let starport = site
        .platforms
        .iter()
        .find(|p| p.id == "starport")
        .expect("a starport");
    let [x, z] = site.to_world_xz([starport.at[0] + 30.0, starport.at[1] + 20.0]);
    let at = strata_worldgen::WorldSource::sample(&world, x, z);
    assert!(
        (f64::from(at.height) - starport.height).abs() < 1e-3,
        "a level starport"
    );
    // The quay meets the sea.
    let [x, z] = site.to_world_xz([0.0, 240.0]);
    assert!(
        strata_worldgen::WorldSource::sample(&world, x, z).height < 0.0,
        "water below the quay"
    );
    // Another world, the same city: its ground is the settlement's own.
    let other = strata_worldgen::ProceduralWorld::new(strata_core::Seed(7), &registries);
    let there = &other.sites()[0];
    assert_eq!(there.fingerprint(), site.fingerprint());
    other.hydrology().set_threads(16);
    other.prepare_water(cx, cz, 2_000.0);
    for local in [[0.0, 0.0], [-200.0, -250.0], [180.0, 120.0], [0.0, -390.0]] {
        let [x, z] = site.to_world_xz(local);
        let here = strata_worldgen::WorldSource::sample(&world, x, z).height;
        let there = strata_worldgen::WorldSource::sample(&other, x, z).height;
        assert!((here - there).abs() < 1e-3, "{local:?}: {here} and {there}");
    }
    // Built again from the same seed, the same site.
    let again = strata_worldgen::ProceduralWorld::new(strata_core::Seed(42), &registries);
    assert_eq!(again.sites(), world.sites());
}

#[test]
#[ignore = "a probe: coastal slopes near the seed-42 spawn"]
#[allow(
    clippy::cast_possible_truncation,
    reason = "whole metres for a printout"
)]
fn coastal_slopes_near_the_origin() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = strata_worldgen::ProceduralWorld::new(strata_core::Seed(42), &registries);
    let h = |x: f64, z: f64| f64::from(strata_worldgen::WorldSource::sample(&world, x, z).height);
    // Walk a ring of bearings from the origin; along each find the shoreline
    // and how the land rises over the 400 m behind it.
    for step in 0..24 {
        let a = f64::from(step) * std::f64::consts::TAU / 24.0;
        let (dx, dz) = (a.cos(), a.sin());
        let mut shore = None;
        for r in (0..8000).step_by(25) {
            let r = f64::from(r);
            if h(dx * r, dz * r) > 0.0 {
                shore = Some(r);
                break;
            }
        }
        let Some(r) = shore else { continue };
        let rise: Vec<i64> = [100.0, 200.0, 300.0, 400.0]
            .iter()
            .map(|d| h(dx * (r + d), dz * (r + d)).round() as i64)
            .collect();
        let biome =
            strata_worldgen::WorldSource::sample(&world, dx * (r + 200.0), dz * (r + 200.0)).biomes
                [0]
            .0;
        println!(
            "bearing {:>3} deg: shore at {:>5} m ({:>7.0}, {:>7.0}), heights +100..400 m {:?}, biome {:?}",
            step * 15,
            r,
            dx * r,
            dz * r,
            rise,
            registries.biomes.name_of(biome).map(ToString::to_string)
        );
    }
}
