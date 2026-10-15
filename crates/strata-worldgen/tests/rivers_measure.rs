//! The river network's measures (ticket 0005, step 1), printed. Ignored:
//! it bakes whole masses. Run in release:
//! `cargo test --release -p strata-worldgen --test rivers_measure -- --ignored --nocapture`.

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use strata_worldgen::hydrology::{LATTICE_M, Network, RiverEnd};

/// Length of a reach, metres.
fn length(network: &Network, reach: usize) -> f64 {
    network.rivers()[reach]
        .vertices
        .windows(2)
        .map(|p| ((p[1].x - p[0].x).powi(2) + (p[1].z - p[0].z).powi(2)).sqrt())
        .sum()
}

/// Metres from reach `i`'s source down to where its water leaves the
/// mass, through confluences and lakes.
fn to_end(i: usize, network: &Network, drains: &BTreeMap<u32, usize>, memo: &mut Vec<f64>) -> f64 {
    if !memo[i].is_nan() {
        return memo[i];
    }
    let own = length(network, i);
    let rest = match network.rivers()[i].end {
        RiverEnd::Joins(next) => to_end(next as usize, network, drains, memo),
        RiverEnd::Lake(lake) => drains
            .get(&lake)
            .map_or(0.0, |&next| to_end(next, network, drains, memo)),
        _ => 0.0,
    };
    memo[i] = own + rest;
    memo[i]
}

/// The longest course from any source to the sea, metres, and its source.
fn longest(network: &Network) -> (f64, usize) {
    let rivers = network.rivers();
    let drains: BTreeMap<u32, usize> = rivers
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.from_lake.map(|l| (l, i)))
        .collect();
    let mut memo = vec![f64::NAN; rivers.len()];
    (0..rivers.len())
        .map(|i| (to_end(i, network, &drains, &mut memo), i))
        .fold((0.0, 0), |a, b| if b.0 > a.0 { b } else { a })
}

#[test]
#[ignore = "bakes whole land masses; run in release"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "one report, its counts far under 2^53"
)]
fn measure_rivers() {
    let world = common::world(42);
    let hydrology = world.hydrology();
    hydrology.set_threads(16);
    let city = world.sites()[0].clone();
    let (cx, cz) = (city.centre[0], city.centre[1]);

    let started = Instant::now();
    let home = hydrology
        .network_at(&world, -700.0, -1_200.0)
        .expect("Port Serein stands on land");
    println!(
        "Port Serein's mass {:?}: {} nodes ({:.0} km2), capped {}, baked in {:.2} s ({} tiles), {} reaches, {} lakes, {} falls, {} segments, {} bucket entries, hash {:#018x} ({:.2} s wall)",
        home.key(),
        home.stats().nodes,
        home.stats().nodes as f64 * LATTICE_M * LATTICE_M / 1e6,
        home.capped(),
        home.stats().seconds,
        home.stats().tiles,
        home.rivers().len(),
        home.lakes().len(),
        home.falls().len(),
        home.stats().segments,
        home.stats().bucket_entries,
        home.content_hash(),
        started.elapsed().as_secs_f64(),
    );

    let started = Instant::now();
    let all = hydrology.bake_area(
        &world,
        (cx - 100_000.0, cz - 100_000.0),
        (cx + 100_000.0, cz + 100_000.0),
    );
    println!(
        "200 km around Port Serein: {} masses in {:.2} s",
        all.len(),
        started.elapsed().as_secs_f64()
    );
    let mut by_size: Vec<_> = all.iter().collect();
    by_size.sort_by_key(|n| std::cmp::Reverse(n.stats().nodes));
    for n in by_size.iter().take(6) {
        println!(
            "  mass {:?}: {} nodes ({:.0} km2), capped {}, {:.2} s, {} tiles, {} reaches, {} lakes, {} falls",
            n.key(),
            n.stats().nodes,
            n.stats().nodes as f64 * LATTICE_M * LATTICE_M / 1e6,
            n.capped(),
            n.stats().seconds,
            n.stats().tiles,
            n.rivers().len(),
            n.lakes().len(),
            n.falls().len()
        );
    }

    // The whole set's measures, over every mass met.
    let mut land_nodes = 0usize;
    let mut lake_nodes = 0usize;
    let mut reaches = 0usize;
    let mut mouths = 0usize;
    let mut km = 0.0;
    let mut lakes = 0usize;
    let mut falls = 0usize;
    let mut tallest = (0.0_f64, [0.0; 3]);
    let mut into_sea = 0usize;
    let mut drops = Vec::new();
    let mut biggest = (0.0_f32, 0.0_f32, 0.0, 0.0);
    let mut longest_all = (0.0, [0i32; 2], 0.0, 0.0);
    for n in &all {
        land_nodes += n.stats().nodes;
        lake_nodes += n.lakes().iter().map(|l| l.nodes.len()).sum::<usize>();
        lakes += n.lakes().len();
        reaches += n.rivers().len();
        mouths += n.rivers().iter().filter(|r| r.end == RiverEnd::Sea).count();
        km += (0..n.rivers().len()).map(|i| length(n, i)).sum::<f64>() / 1000.0;
        falls += n.falls().len();
        for f in n.falls() {
            let h = f.top[1] - f.bottom[1];
            into_sea += usize::from(f.bottom[1].abs() < 1e-6);
            drops.push(h);
            if h > tallest.0 {
                tallest = (h, f.top);
            }
        }
        for r in n.rivers() {
            for v in &r.vertices {
                if v.discharge_m3s > biggest.0 {
                    biggest = (v.discharge_m3s, v.width_m, v.x, v.z);
                }
            }
        }
        let (len, source) = longest(n);
        if len > longest_all.0 {
            let v = n.rivers()[source].vertices[0];
            longest_all = (len, n.key(), v.x, v.z);
        }
    }
    let land_km2 = land_nodes as f64 * LATTICE_M * LATTICE_M / 1e6;
    println!(
        "over those masses ({land_km2:.0} km2): {:.1} reaches and {:.1} mouths per 1 000 km2, {:.0} km of river per 1 000 km2",
        reaches as f64 / land_km2 * 1000.0,
        mouths as f64 / land_km2 * 1000.0,
        km / land_km2 * 1000.0
    );
    println!(
        "  longest river {:.1} km (mass {:?}, source ({:.0}, {:.0})); largest discharge {:.2} m3/s, {:.1} m wide, at ({:.0}, {:.0})",
        longest_all.0 / 1000.0,
        longest_all.1,
        longest_all.2,
        longest_all.3,
        biggest.0,
        biggest.1,
        biggest.2,
        biggest.3
    );
    println!(
        "  {lakes} lakes, {:.2}% of land; {falls} falls, tallest {:.0} m at ({:.0}, {:.0})",
        lake_nodes as f64 / land_nodes.max(1) as f64 * 100.0,
        tallest.0,
        tallest.1[0],
        tallest.1[2]
    );

    drops.sort_by(f64::total_cmp);
    println!(
        "  falls: {into_sea} of them into the sea; drops median {:.0} m, 90th percentile {:.0} m",
        drops.get(drops.len() / 2).copied().unwrap_or(0.0),
        drops.get(drops.len() * 9 / 10).copied().unwrap_or(0.0)
    );

    // What comes near Port Serein: each reach's nearest vertex to the
    // city's extent.
    println!("reaches within 10 km of Port Serein's extent:");
    let mut near = Vec::new();
    for n in &all {
        for (i, r) in n.rivers().iter().enumerate() {
            let best = r
                .vertices
                .iter()
                .map(|v| (city.extent_distance_at(v.x, v.z), v))
                .fold(None, |held: Option<(f64, _)>, (d, v)| match held {
                    Some((h, _)) if h <= d => held,
                    _ => Some((d, v)),
                });
            if let Some((d, v)) = best.filter(|(d, _)| *d <= 10_000.0) {
                near.push((d, n.key(), i, *v, r.order));
            }
        }
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, key, i, v, order) in &near {
        println!(
            "  {:5.2} km: mass {key:?} reach {i}, order {order}, {:.2} m3/s, {:.1} m wide, at ({:.0}, {:.0}), surface {:.0} m",
            d / 1000.0,
            v.discharge_m3s,
            v.width_m,
            v.x,
            v.z,
            v.surface_m
        );
    }
    println!(
        "  {} within 3 km, {} within 10 km",
        near.iter().filter(|n| n.0 <= 3_000.0).count(),
        near.len()
    );

    // The query's cost, one thread, a million points over 40 km round the
    // city: the network's own, and the world's (masses found per point).
    let points: Vec<(f64, f64)> = (0..1_000_000u64)
        .map(|i| {
            let h = |s: u64| {
                let mut x = (i ^ s).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                x ^= x >> 31;
                x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                x ^= x >> 29;
                (x >> 11) as f64 / (1u64 << 53) as f64
            };
            (
                cx - 20_000.0 + h(1) * 40_000.0,
                cz - 20_000.0 + h(2) * 40_000.0,
            )
        })
        .collect();
    for _ in 0..2 {
        let started = Instant::now();
        let mut wet = 0usize;
        for &(x, z) in &points {
            wet += usize::from(home.water_at(x, z).is_some());
        }
        let network_ns = started.elapsed().as_secs_f64() * 1e9 / points.len() as f64;
        let started = Instant::now();
        let mut wet_world = 0usize;
        for &(x, z) in &points {
            wet_world += usize::from(world.water_at(x, z).is_some());
        }
        let world_ns = started.elapsed().as_secs_f64() * 1e9 / points.len() as f64;
        println!(
            "query: {network_ns:.0} ns a point on the network ({:.1}% within reach), {world_ns:.0} ns through the world ({:.1}%)",
            wet as f64 / 1e4,
            wet_world as f64 / 1e4
        );
    }
}
