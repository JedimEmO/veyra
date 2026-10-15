//! The rivers' shape (ticket 0005, step 3b), printed: the bake's time, how
//! deep the valleys are cut, how a river winds and whether the lakes hold
//! their water. Ignored: it bakes the rivers over 100 km round the origin.
//! Run in release, alone:
//! `cargo test --release -p strata-worldgen --test shape_measure -- --ignored --nocapture --test-threads=1`.

// Probes, printed and never gated: their casts, their letters and their
// length are a measure's, not the crate's code.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::unnecessary_sort_by,
    clippy::too_many_lines,
    reason = "probes, printed and never gated"
)]

mod common;

use std::time::Instant;

use strata_core::Seed;
use strata_worldgen::hydrology::{River, WaterKind};
use strata_worldgen::{ProceduralWorld, WorldSource};

fn length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
        .sum()
}

fn line(river: &River) -> Vec<[f64; 2]> {
    river.vertices.iter().map(|v| [v.x, v.z]).collect()
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Whether a lake's water stands against ground at least its level all
/// round: every point of the lake's water whose neighbour 24 m off is not
/// that water must have the neighbour's ground at the level, or the
/// neighbour must be a river's water at the level (its outlet or an
/// inlet). Returns (wet points, leaks, where the first leak is).
pub fn lake_holds(
    world: &ProceduralWorld,
    level: f32,
    nodes: &[[i32; 2]],
) -> (usize, usize, Option<[f64; 2]>) {
    let step = 24.0;
    let (mut i0, mut j0, mut i1, mut j1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for n in nodes {
        i0 = i0.min(n[0]);
        j0 = j0.min(n[1]);
        i1 = i1.max(n[0]);
        j1 = j1.max(n[1]);
    }
    let (x0, z0) = (f64::from(i0 - 1) * 512.0, f64::from(j0 - 1) * 512.0);
    let (x1, z1) = (f64::from(i1 + 1) * 512.0, f64::from(j1 + 1) * 512.0);
    let nx = ((x1 - x0) / step) as usize + 1;
    let nz = ((z1 - z0) / step) as usize + 1;
    let mut grid = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let (x, z) = (x0 + i as f64 * step, z0 + j as f64 * step);
            let s = world.sample(x, z);
            let water = s.standing_water().copied();
            let wet = water
                .is_some_and(|w| w.kind == WaterKind::Lake && (w.surface_m - level).abs() < 0.01);
            let river_at_level =
                water.is_some_and(|w| w.kind != WaterKind::Lake && w.surface_m >= level - 0.5);
            grid.push((wet, river_at_level, s.height));
        }
    }
    let (mut wet, mut leaks, mut first) = (0, 0, None);
    for j in 0..nz {
        for i in 0..nx {
            if !grid[j * nx + i].0 {
                continue;
            }
            wet += 1;
            for (di, dj) in [(1_i64, 0_i64), (-1, 0), (0, 1), (0, -1)] {
                let (ii, jj) = (i as i64 + di, j as i64 + dj);
                if ii < 0 || jj < 0 || ii >= nx as i64 || jj >= nz as i64 {
                    continue;
                }
                let (w, river, h) = grid[jj as usize * nx + ii as usize];
                if !w && !river && h < level - 0.05 {
                    leaks += 1;
                    first.get_or_insert([x0 + ii as f64 * step, z0 + jj as f64 * step]);
                }
            }
        }
    }
    (wet, leaks, first)
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_shape() {
    // Port Serein's mass alone, timed, before anything else bakes it: every
    // world of a seed in a process shares one cache.
    let registries = common::registries();
    let fresh = ProceduralWorld::new(Seed(42), &registries);
    fresh.hydrology().set_threads(16);
    let started = Instant::now();
    // On the 16 threads `set_threads` gave: `network_at` would bake on one.
    let baked = fresh
        .hydrology()
        .bake_area(&fresh, (-700.0, -1_200.0), (-700.0, -1_200.0));
    for network in &baked {
        println!(
            "  baked {:?}: {} reaches in {:.2} s",
            network.key(),
            network.rivers().len(),
            network.stats().seconds
        );
    }
    let home = fresh
        .hydrology()
        .network_at(&fresh, -700.0, -1_200.0)
        .expect("Port Serein stands on land");
    println!(
        "Port Serein's mass: {} reaches, {} lakes, {} segments, {} bucket entries, baked in {:.2} s ({:.2} s wall)",
        home.rivers().len(),
        home.lakes().len(),
        home.stats().segments,
        home.stats().bucket_entries,
        home.stats().seconds,
        started.elapsed().as_secs_f64()
    );
    let vertices: usize = home.rivers().iter().map(|r| r.vertices.len()).sum();
    let km: f64 = home.rivers().iter().map(|r| length(&line(r))).sum::<f64>() / 1_000.0;
    println!("  {vertices} vertices over {km:.0} km of river");

    let world = common::world(42);
    let networks = world.hydrology().networks();
    let city = world.sites()[0].to_world_xz([0.0, 0.0]);

    // How deep the valleys are cut.
    let (mut total, mut over60, mut over200) = (0usize, 0usize, 0usize);
    let mut cuts = Vec::new();
    let mut worst: Vec<(f32, [f64; 2])> = Vec::new();
    for network in &networks {
        for river in network.rivers() {
            for v in &river.vertices {
                if v.x.abs() > 100_000.0 || v.z.abs() > 100_000.0 {
                    continue;
                }
                total += 1;
                over60 += usize::from(v.cut_m > 60.0);
                over200 += usize::from(v.cut_m > 200.0);
                cuts.push(f64::from(v.cut_m));
                if v.cut_m > 200.0 {
                    worst.push((v.cut_m, [v.x, v.z]));
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "cut, {total} vertices within 100 km: over 60 m {over60} ({:.2} %), over 200 m {over200}; percentiles 50 {:.1} 90 {:.1} 95 {:.1} 99 {:.1} max {:.1}; worst {:?}",
        over60 as f64 * 100.0 / total as f64,
        pct(&cuts, 0.5),
        pct(&cuts, 0.9),
        pct(&cuts, 0.95),
        pct(&cuts, 0.99),
        pct(&cuts, 1.0),
        &worst[..worst.len().min(5)]
    );

    // How a river winds: reaches within 60 km of the city, at least 400 m
    // long, by their surface's mean slope.
    let mut classes: [(Vec<f64>, Vec<f64>); 3] = Default::default();
    for network in &networks {
        for river in network.rivers() {
            let points = line(river);
            if !points
                .iter()
                .any(|p| ((p[0] - city[0]).powi(2) + (p[1] - city[1]).powi(2)).sqrt() < 60_000.0)
            {
                continue;
            }
            let along = length(&points);
            if along < 400.0 {
                continue;
            }
            let first = river.vertices[0];
            let last = river.vertices[river.vertices.len() - 1];
            let chord = ((last.x - first.x).powi(2) + (last.z - first.z).powi(2)).sqrt();
            let slope = f64::from(first.surface_m - last.surface_m) / along;
            let channel = river.channel(4.0);
            let class = if slope < 0.005 {
                0
            } else if slope > 0.03 {
                2
            } else {
                1
            };
            classes[class].0.push(length(&channel) / chord.max(1.0));
            classes[class].1.push(length(&channel) / along);
        }
    }
    for (name, (whole, own)) in ["lowland (< 0.5 %)", "middle", "steep (> 3 %)"]
        .iter()
        .zip(classes.iter_mut())
    {
        whole.sort_by(f64::total_cmp);
        own.sort_by(f64::total_cmp);
        println!(
            "sinuosity, {name}: {} reaches; channel over chord 10/50/90 {:.3} {:.3} {:.3}; channel over its valley line 10/50/90 {:.3} {:.3} {:.3}",
            whole.len(),
            pct(whole, 0.1),
            pct(whole, 0.5),
            pct(whole, 0.9),
            pct(own, 0.1),
            pct(own, 0.5),
            pct(own, 0.9)
        );
    }

    // The lakes within 60 km: perched (a leak) or dry.
    let started = Instant::now();
    let (mut lakes, mut perched, mut dry) = (0, 0, 0);
    let mut leaky: Vec<(usize, [f64; 2], f32)> = Vec::new();
    for network in &networks {
        for lake in network.lakes() {
            let [i, j] = lake.nodes[0];
            let (x, z) = (f64::from(i) * 512.0, f64::from(j) * 512.0);
            if ((x - city[0]).powi(2) + (z - city[1]).powi(2)).sqrt() > 60_000.0 {
                continue;
            }
            lakes += 1;
            let (wet, leaks, first) = lake_holds(&world, lake.level_m, &lake.nodes);
            if wet == 0 {
                dry += 1;
            }
            if leaks > 0 {
                perched += 1;
                leaky.push((leaks, first.unwrap_or([x, z]), lake.level_m));
            }
        }
    }
    leaky.sort_by(|a, b| b.0.cmp(&a.0));
    println!(
        "lakes within 60 km: {lakes}; perched (a leak) {perched}, dry {dry}; worst {:?} ({:.1} s)",
        &leaky[..leaky.len().min(8)],
        started.elapsed().as_secs_f64()
    );
}

/// Prints the samples on a 5 by 5 grid 24 m apart round the point in
/// `PROBE_AT` (`x,z`): what a leak looks like.
#[test]
#[ignore = "a probe"]
fn probe_point() {
    let at: Vec<f64> = std::env::var("PROBE_AT")
        .unwrap_or_else(|_| "22408,-5544".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    for j in -3..=3 {
        for i in -3..=3 {
            let (x, z) = (at[0] + f64::from(i) * 24.0, at[1] + f64::from(j) * 24.0);
            let s = world.sample(x, z);
            let natural = world.terms(x, z).height_m;
            println!(
                "({x:.0}, {z:.0}): h {:.2} natural {natural:.2} standing {:?} water {:?}",
                s.height,
                s.standing_water().map(|w| (w.kind, w.surface_m)),
                s.water
                    .map(|w| (w.kind, w.surface_m, w.signed_distance_m, w.width_m))
            );
        }
    }
}

/// Prints the vertices of the reach nearest the point in `PROBE_AT`.
#[test]
#[ignore = "a probe"]
fn probe_reach() {
    let at: Vec<f64> = std::env::var("PROBE_AT")
        .unwrap_or_else(|_| "2200,-5600".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    let networks = world.hydrology().networks();
    let (river, _) = networks
        .iter()
        .flat_map(|n| n.rivers().iter())
        .map(|r| {
            let d = r
                .vertices
                .iter()
                .map(|v| ((v.x - at[0]).powi(2) + (v.z - at[1]).powi(2)).sqrt())
                .fold(f64::INFINITY, f64::min);
            (r, d)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a reach");
    println!(
        "order {} end {:?} from_lake {:?}",
        river.order, river.end, river.from_lake
    );
    for v in &river.vertices {
        println!(
            "({:.0}, {:.0}) s {:.2} q {:.2} w {:.1} cut {:.1} fall {} meander {:.2} phase {:.2}",
            v.x,
            v.z,
            v.surface_m,
            v.discharge_m3s,
            v.width_m,
            v.cut_m,
            v.fall,
            v.meander_m,
            v.phase
        );
    }
}

/// Heights along the line `PROBE_LINE` (`x0,z0,x1,z1`) every 3 m, offset
/// sideways by `PROBE_OFF` metres: the carved ground, the natural one and
/// the water record.
#[test]
#[ignore = "a probe"]
fn probe_line() {
    let l: Vec<f64> = std::env::var("PROBE_LINE")
        .unwrap_or_else(|_| "1972,-6005,1545,-5598".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let off: f64 = std::env::var("PROBE_OFF").map_or(50.0, |v| v.parse().expect("a number"));
    let world = common::world(42);
    let (dx, dz) = (l[2] - l[0], l[3] - l[1]);
    let len = (dx * dx + dz * dz).sqrt();
    let (nx, nz) = (-dz / len, dx / len);
    let mut s = 0.0;
    while s <= len {
        let (x, z) = (
            l[0] + dx * s / len + nx * off,
            l[1] + dz * s / len + nz * off,
        );
        let a = world.sample(x, z);
        println!(
            "{s:.0}: h {:.2} natural {:.2} slope {:.3} water {:?}",
            a.height,
            world.terms(x, z).height_m,
            a.landform.slope,
            a.water.map(|w| (w.signed_distance_m, w.width_m, w.cut_m()))
        );
        s += 3.0;
    }
}

/// Prints the lakes within 30 km of the point in `PROBE_AT`.
#[test]
#[ignore = "a probe"]
fn probe_lakes() {
    let at: Vec<f64> = std::env::var("PROBE_AT")
        .unwrap_or_else(|_| "22340,-4781".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    for network in world.hydrology().networks() {
        for lake in network.lakes() {
            let [i, j] = lake.nodes[0];
            let (x, z) = (f64::from(i) * 512.0, f64::from(j) * 512.0);
            if ((x - at[0]).powi(2) + (z - at[1]).powi(2)).sqrt() < 30_000.0 {
                println!(
                    "lake level {:.2} depth {:.1} detail {:.1} outlet {:?} nodes {:?}",
                    lake.level_m, lake.depth_m, lake.detail_m, lake.outlet, lake.nodes
                );
            }
        }
    }
}

/// The worst second difference along `PROBE_LINE` every 2 cm, and the
/// profile round it.
#[test]
#[ignore = "a probe"]
fn probe_crease() {
    let l: Vec<f64> = std::env::var("PROBE_LINE")
        .unwrap_or_else(|_| "17920,7669,17920,7629".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    let n = ((l[2] - l[0]).hypot(l[3] - l[1]) / 0.02).ceil() as usize;
    let at = |k: usize| {
        let f = k as f64 / n as f64;
        (l[0] + (l[2] - l[0]) * f, l[1] + (l[3] - l[1]) * f)
    };
    let h: Vec<f64> = (0..=n)
        .map(|k| {
            let (x, z) = at(k);
            f64::from(world.sample(x, z).height)
        })
        .collect();
    let (k, bend) = h
        .windows(3)
        .enumerate()
        .map(|(k, w)| (k + 1, (w[2] - 2.0 * w[1] + w[0]).abs()))
        .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    let (x, z) = at(k);
    let s = world.sample(x, z);
    println!(
        "worst {bend:.6} at ({x:.2}, {z:.2}) h {:.3} natural {:.3} water {:?}",
        h[k],
        world.terms(x, z).height_m,
        s.water
    );
    for d in [-200_i64, -50, -10, -2, 0, 2, 10, 50, 200] {
        let j = (k as i64 + d).clamp(0, n as i64) as usize;
        println!("  {d}: {:.4}", h[j]);
    }
}

/// The record round a point on `PROBE_LINE` every 2 cm: `PROBE_K` steps
/// from its start, ten either side.
#[test]
#[ignore = "a probe"]
fn probe_record() {
    let l: Vec<f64> = std::env::var("PROBE_LINE")
        .unwrap_or_else(|_| "2074.09,-6007.59,2068.10,-6007.32".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    let n = ((l[2] - l[0]).hypot(l[3] - l[1]) / 0.02).ceil() as usize;
    let centre: i64 =
        std::env::var("PROBE_K").map_or(n as i64 / 2, |v| v.parse().expect("a number"));
    for d in -10..=10_i64 {
        let k = (centre + d).clamp(0, n as i64) as f64 / n as f64;
        let (x, z) = (l[0] + (l[2] - l[0]) * k, l[1] + (l[3] - l[1]) * k);
        let s = world.sample(x, z);
        let w = s.water.expect("water");
        println!(
            "{d}: h {:.5} sd {:.5} w {:.5} bed {:.5} s {:.5} slope {:.4}",
            s.height, w.signed_distance_m, w.width_m, w.bed_m, w.surface_m, s.landform.slope
        );
    }
}

/// Where the confluence south of Port Serein now is, a point in its
/// channel below it and the lake's bed: the pinned water samples.
#[test]
#[ignore = "a probe"]
fn probe_snapshot_points() {
    let world = common::world(42);
    let params = strata_worldgen::hydrology::RiverParams::DEFAULT;
    let networks = world.hydrology().networks();
    let rivers: Vec<_> = networks.iter().flat_map(|n| n.rivers().iter()).collect();
    let (down, _) = rivers
        .iter()
        .filter(|r| r.vertices.len() > 3)
        .map(|r| {
            let v = r.vertices[0];
            (r, (v.x + 1_516.0).hypot(v.z + 3_977.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a reach");
    println!(
        "junction at ({:.1}, {:.1})",
        down.vertices[0].x, down.vertices[0].z
    );
    for p in down.centreline(&params, 5.0).iter().take(12) {
        let s = world.sample(p.x, p.z);
        println!(
            "  ({:.1}, {:.1}) width {:.2} depth {:.2} -> h {:.2} water {:?}",
            p.x,
            p.z,
            p.width_m,
            p.depth_m,
            s.height,
            s.water.map(|w| (w.kind, w.surface_m, w.signed_distance_m))
        );
    }
    let s = world.sample(22_340.0, -4_781.0);
    println!("lake point: h {} water {:?}", s.height, s.water);
}

/// The samples either side of a step at `PROBE_AT`, a centimetre apart on
/// both axes.
#[test]
#[ignore = "a probe"]
fn probe_step() {
    let at: Vec<f64> = std::env::var("PROBE_AT")
        .unwrap_or_else(|_| "3429.885264117003,29081.51575167635".into())
        .split(',')
        .map(|v| v.parse().expect("a number"))
        .collect();
    let world = common::world(42);
    for (dx, dz) in [
        (0.0, 0.0),
        (-0.01, 0.0),
        (0.01, 0.0),
        (0.0, -0.01),
        (0.0, 0.01),
        (-0.006, -0.008),
        (0.006, 0.008),
    ] {
        let (x, z) = (at[0] + dx, at[1] + dz);
        let s = world.sample(x, z);
        println!(
            "({x:.3}, {z:.3}): h {:.3} natural {:.3} water {:?}",
            s.height,
            world.terms(x, z).height_m,
            s.water.map(|w| (
                w.kind,
                w.surface_m,
                w.signed_distance_m,
                w.width_m,
                w.cut_m()
            ))
        );
    }
}
