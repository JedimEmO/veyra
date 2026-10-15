//! The measures behind ticket 0005's step 6 record (the road's grade):
//! ignored probes, run in release, one at a time:
//!
//! ```text
//! cargo test --release -p strata-worldgen --test grade_measure -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `probe_cost` is pinned to one core with `taskset -c 4`.

#![allow(
    clippy::many_single_char_names,
    reason = "points, indices and shares along a road, as the probes walk them"
)]

mod common;

use std::time::Instant;

use strata_core::Seed;
use strata_worldgen::hydrology::{LATTICE_M, Network};
use strata_worldgen::roads::{self, Road, RoadClass, RoadEnd};
use strata_worldgen::{ProceduralWorld, WorldSource};

fn world() -> ProceduralWorld {
    let world = ProceduralWorld::new(Seed(42), &common::registries());
    world.hydrology().set_threads(common::THREADS);
    world
}

fn bake(world: &ProceduralWorld, radius: f64) -> Vec<std::sync::Arc<Network>> {
    let city = world.sites()[0].centre;
    world.hydrology().bake_area(
        world,
        (city[0] - radius, city[1] - radius),
        (city[0] + radius, city[1] + radius),
    )
}

/// A road's centreline and recorded ground at `along`.
fn at(road: &Road, along: f64) -> ([f64; 2], f64) {
    let p = &road.points;
    let k = p
        .partition_point(|q| q.along_m <= along)
        .clamp(1, p.len() - 1);
    let (a, b) = (p[k - 1], p[k]);
    let t = if b.along_m > a.along_m {
        ((along - a.along_m) / (b.along_m - a.along_m)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (
        [a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t],
        f64::from(a.ground_m) + (f64::from(b.ground_m) - f64::from(a.ground_m)) * t,
    )
}

/// Whether `along` is under a bridge's deck, where the ground is the
/// river's and not the road's.
fn in_gap(road: &Road, along: f64) -> bool {
    road.crossings.iter().any(|c| {
        c.kind == roads::CrossingKind::Bridge
            && along > c.abutments_m[0]
            && along < c.abutments_m[1]
    })
}

fn quantile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a probe's list"
    )]
    let k = ((values.len() - 1) as f64 * q).round() as usize;
    values[k]
}

fn name(class: RoadClass) -> &'static str {
    match class {
        RoadClass::Service => "service",
        RoadClass::Track => "track",
        RoadClass::Trail => "trail",
    }
}

/// The bake's time with the grade, and the profiles within 60 km of Port
/// Serein: cut and fill along the centreline every 5 m (median, 90th
/// percentile, deepest), cuttings deeper than 5 m, the share of length
/// steeper than the class, the balance of cut against fill, and the
/// sampled ground on the centreline against the profile.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a probe's report, in order"
)]
fn probe_grade() {
    let world = world();
    let city = world.sites()[0].centre;
    let started = Instant::now();
    let networks = bake(&world, 62_000.0);
    println!("bake of 62 km: {:.2} s", started.elapsed().as_secs_f64());
    let home = networks
        .iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let s = home.stats();
    println!(
        "home mass: bake {:.3} s, roads {:.3} s of which profiles {:.3} s; {} roads, {:.0} km",
        s.seconds, s.roads_seconds, s.grade_seconds, s.roads, s.road_km
    );
    for class in [RoadClass::Service, RoadClass::Track] {
        let max_grade = roads::spec(class).max_grade;
        let (mut cuts, mut fills) = (Vec::new(), Vec::new());
        let (mut volume_cut, mut volume_fill) = (0.0, 0.0);
        let (mut length, mut over, mut graded_off) = (0.0_f64, 0.0, Vec::new());
        let (mut cuttings, mut cutting_m, mut deepest_at) = (0_usize, 0.0, ([0.0, 0.0], 0.0));
        let mut steep_cut = 0.0;
        let mut highest_at = ([0.0, 0.0], 0.0);
        for network in &networks {
            for road in network.roads() {
                if road.class != class || !road.graded() {
                    continue;
                }
                let mut run = 0.0;
                let mut s = 0.0;
                while s <= road.length_m() {
                    let (p, ground) = at(road, s);
                    let (dx, dz) = (p[0] - city[0], p[1] - city[1]);
                    if dx * dx + dz * dz < 60_000.0 * 60_000.0 && !in_gap(road, s) {
                        let (level, rise) = road.level_at(s).expect("a profile");
                        let depth = ground - level;
                        length += 5.0;
                        if rise.abs() > max_grade + 0.002 {
                            over += 5.0;
                        }
                        if depth > 0.0 {
                            cuts.push(depth);
                            volume_cut += depth * 5.0;
                        } else {
                            fills.push(-depth);
                            volume_fill += -depth * 5.0;
                            if -depth > highest_at.1 {
                                highest_at = (p, -depth);
                            }
                        }
                        if depth > deepest_at.1 {
                            deepest_at = (p, depth);
                        }
                        if depth > 5.0 {
                            if run == 0.0 {
                                cuttings += 1;
                            }
                            run += 5.0;
                            cutting_m += 5.0;
                        } else {
                            run = 0.0;
                        }
                        if depth > 10.0 {
                            steep_cut += 5.0;
                        }
                        // The ground the world samples on the centreline,
                        // every 50 m, against the profile.
                        if (s % 50.0) < 1.0 {
                            let sampled = f64::from(world.sample(p[0], p[1]).height);
                            graded_off.push((sampled - level).abs());
                            if (sampled - level).abs() > 1.0 {
                                println!(
                                    "  off the profile by {:.2} at ({:.0}, {:.0})",
                                    sampled - level,
                                    p[0],
                                    p[1]
                                );
                            }
                        }
                    }
                    s += 5.0;
                }
            }
        }
        let all = cuts.len() + fills.len();
        println!(
            "{}: {:.0} km within 60 km; in cut {:.0} %, cut median {:.2} m, p90 {:.2}, deepest {:.1} at ({:.0}, {:.0}); fill median {:.2}, p90 {:.2}, highest {:.1} at ({:.0}, {:.0})",
            name(class),
            length / 1e3,
            100.0 * cuts.len() as f64 / all.max(1) as f64,
            quantile(&mut cuts.clone(), 0.5),
            quantile(&mut cuts.clone(), 0.9),
            deepest_at.1,
            deepest_at.0[0],
            deepest_at.0[1],
            quantile(&mut fills.clone(), 0.5),
            quantile(&mut fills.clone(), 0.9),
            quantile(&mut fills.clone(), 1.0),
            highest_at.0[0],
            highest_at.0[1],
        );
        println!(
            "  cuttings over 5 m: {} ({:.1} km), over 10 m: {:.1} km; over its grade {:.1} %; cut {:.0} m2 against fill {:.0} m2 of section; sampled centreline off the profile: median {:.3} m, p99 {:.3}, worst {:.2}",
            cuttings,
            cutting_m / 1e3,
            steep_cut / 1e3,
            100.0 * over / length.max(1.0),
            volume_cut,
            volume_fill,
            quantile(&mut graded_off.clone(), 0.5),
            quantile(&mut graded_off.clone(), 0.99),
            quantile(&mut graded_off.clone(), 1.0),
        );
    }
}

/// The sample's cost on and off a road, one thread: carriageways of the
/// graded roads within 30 km, their banks 12 to 30 m out, and points over
/// 300 m from any road.
#[test]
#[ignore = "a probe: release, by hand, pinned to one core"]
#[allow(clippy::cast_precision_loss, reason = "a probe's counts")]
fn probe_cost() {
    let world = world();
    let networks = bake(&world, 32_000.0);
    let city = world.sites()[0].centre;
    let (mut on, mut bank, mut off) = (Vec::new(), Vec::new(), Vec::new());
    let mut k = 0_u64;
    for network in &networks {
        for road in network.roads() {
            if !road.graded() {
                continue;
            }
            let mut s = 13.0;
            while s < road.length_m() {
                let (p, _) = at(road, s);
                let (dx, dz) = (p[0] - city[0], p[1] - city[1]);
                if dx * dx + dz * dz < 30_000.0 * 30_000.0 {
                    k += 1;
                    on.push((p[0] + 0.7, p[1] - 0.4));
                    let side = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
                    let out = 12.0 + (k % 7) as f64 * 3.0;
                    let (q, _) = at(road, s + 0.5);
                    let (tx, tz) = (q[0] - p[0], q[1] - p[1]);
                    let l = (tx * tx + tz * tz).sqrt().max(1e-9);
                    bank.push((p[0] - tz / l * out * side, p[1] + tx / l * out * side));
                }
                s += 97.0;
            }
        }
    }
    // Off any road: a lattice of points over the 30 km whose nearest road
    // is over 300 m away.
    let mut z = city[1] - 30_000.0;
    while z < city[1] + 30_000.0 {
        let mut x = city[0] - 30_000.0;
        while x < city[0] + 30_000.0 {
            if world.nearest_road(x, z, 300.0).is_none() && world.sample(x, z).height > 2.0 {
                off.push((x, z));
            }
            x += 611.0;
        }
        z += 577.0;
    }
    let time = |points: &[(f64, f64)], graded: bool| {
        let sample = |x: f64, z: f64| {
            if graded {
                world.sample(x, z)
            } else {
                world.sample_without_roads(x, z)
            }
        };
        for &(x, z) in points {
            std::hint::black_box(sample(x, z));
        }
        let mut best = f64::INFINITY;
        for _ in 0..7 {
            let started = Instant::now();
            for _ in 0..4 {
                for &(x, z) in points {
                    std::hint::black_box(sample(x, z));
                }
            }
            best = best.min(started.elapsed().as_secs_f64() * 1e9 / (4 * points.len()) as f64);
        }
        best
    };
    for (place, points) in [
        ("a carriageway", &on),
        ("a bank", &bank),
        ("off any road", &off),
    ] {
        let with = time(points, true);
        let without = time(points, false);
        println!(
            "sample on {place} ({} points): {with:.0} ns, {without:.0} without the grade: the grade {:.0} ns",
            points.len(),
            with - without
        );
    }
    println!("cells {LATTICE_M} m");
}

/// The deepest cuts and highest fills within 60 km, and what holds the
/// profile there: the crossings and ends near them.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_worst() {
    let world = world();
    let city = world.sites()[0].centre;
    let networks = bake(&world, 62_000.0);
    let mut all: Vec<(f64, usize, usize, f64, [f64; 2])> = Vec::new();
    for (n, network) in networks.iter().enumerate() {
        for (r, road) in network.roads().iter().enumerate() {
            if !road.graded() {
                continue;
            }
            let mut s = 0.0;
            while s <= road.length_m() {
                let (p, ground) = at(road, s);
                let (dx, dz) = (p[0] - city[0], p[1] - city[1]);
                if dx * dx + dz * dz < 60_000.0 * 60_000.0 && !in_gap(road, s) {
                    let (level, _) = road.level_at(s).expect("a profile");
                    all.push((ground - level, n, r, s, p));
                }
                s += 5.0;
            }
        }
    }
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let show = |label: &str, (depth, n, r, s, p): (f64, usize, usize, f64, [f64; 2])| {
        let road = &networks[n].roads()[r];
        let near: Vec<String> = road
            .crossings
            .iter()
            .filter(|c| (c.along_m - s).abs() < 400.0)
            .map(|c| {
                format!(
                    "{:?} at {:.0} deck {:.1} surface {:.1} abutments {:.0}..{:.0}",
                    c.kind, c.along_m, c.deck_m, c.surface_m, c.abutments_m[0], c.abutments_m[1]
                )
            })
            .collect();
        println!(
            "{label} {depth:.1} m on {:?} road {r} at {s:.0} of {:.0} ({:.0}, {:.0}); ends {:?}; {near:?}",
            road.class,
            road.length_m(),
            p[0],
            p[1],
            road.ends
        );
    };
    let mut seen = Vec::new();
    for &item in all.iter().take(400) {
        if seen.contains(&(item.1, item.2)) {
            continue;
        }
        seen.push((item.1, item.2));
        show("fill", item);
        if seen.len() >= 6 {
            break;
        }
    }
    seen.clear();
    for &item in all.iter().rev().take(400) {
        if seen.contains(&(item.1, item.2)) {
            continue;
        }
        seen.push((item.1, item.2));
        show("cut", item);
        if seen.len() >= 6 {
            break;
        }
    }
}

/// One place's road: its profile and ground every 10 m round the point.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_place() {
    let world = world();
    let networks = bake(&world, 62_000.0);
    let x: f64 = std::env::var("AT_X")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_426.0);
    let z: f64 = std::env::var("AT_Z")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50_944.0);
    let here = world.nearest_road(x, z, 200.0).expect("a road near");
    let network = networks
        .iter()
        .find(|n| n.key() == here.mass)
        .expect("its mass");
    let road = &network.roads()[here.road as usize];
    println!(
        "{:?} road {} of {:.0} m, ends {:?}, at {:.0}",
        road.class,
        here.road,
        road.length_m(),
        road.ends,
        here.along_m
    );
    for (r, other) in network.roads().iter().enumerate() {
        let near = other
            .points
            .iter()
            .map(|p| ((p.x - x).powi(2) + (p.z - z).powi(2)).sqrt())
            .fold(f64::INFINITY, f64::min);
        if near < 120.0 {
            println!(
                "  near: {:?} road {r} at {near:.0} m, ends {:?}, graded {}",
                other.class,
                other.ends,
                other.graded()
            );
        }
    }
    for c in &road.crossings {
        println!(
            "  {:?} at {:.0} deck {:.1} surface {:.1} abutments {:.0}..{:.0} banks {:?}",
            c.kind, c.along_m, c.deck_m, c.surface_m, c.abutments_m[0], c.abutments_m[1], c.banks_m
        );
    }
    let span: f64 = std::env::var("SPAN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300.0);
    let every: f64 = std::env::var("EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let mut s = (here.along_m - span).max(0.0);
    while s < (here.along_m + span).min(road.length_m()) {
        let (p, ground) = at(road, s);
        let (level, rise) = road.level_at(s).expect("a profile");
        let sampled = world.sample(p[0], p[1]).height;
        let bare = world.sample_without_roads(p[0], p[1]);
        let without = bare.height;
        println!(
            "  {s:7.1}: ground {ground:7.2} level {level:7.2} rise {rise:6.3} sampled {sampled:7.2} without {without:7.2} slope {:.2} carved {:?}",
            bare.landform.slope,
            bare.water.map(|w| (w.cut_m(), w.signed_distance_m))
        );
        s += every;
    }
}

/// The sample's cost as a terrain region pays it: 65 by 65 grids at 1 m
/// centred on a graded road's centreline, every few kilometres of road
/// within 30 km, with the grade and without; one thread.
#[test]
#[ignore = "a probe: release, by hand, pinned to one core"]
#[allow(clippy::cast_precision_loss, reason = "a probe's counts")]
fn probe_cost_grids() {
    let world = world();
    let networks = bake(&world, 32_000.0);
    let city = world.sites()[0].centre;
    let mut centres = Vec::new();
    for network in &networks {
        for road in network.roads() {
            if !road.graded() {
                continue;
            }
            let mut s = 300.0;
            while s < road.length_m() && centres.len() < 60 {
                let (p, _) = at(road, s);
                let (dx, dz) = (p[0] - city[0], p[1] - city[1]);
                if dx * dx + dz * dz < 30_000.0 * 30_000.0 && !in_gap(road, s) {
                    centres.push(p);
                }
                s += 3_100.0;
            }
        }
    }
    let n = 65;
    let mut out = Vec::new();
    let mut time = |graded: bool| {
        let mut total = 0.0;
        for &c in &centres {
            let mut best = f64::INFINITY;
            for _ in 0..5 {
                let started = Instant::now();
                if graded {
                    world.sample_grid(c[0] - 32.0, c[1] - 32.0, 1.0, n, &mut out);
                } else {
                    out.clear();
                    for j in 0..n {
                        for i in 0..n {
                            out.push(world.sample_without_roads(
                                c[0] - 32.0 + i as f64,
                                c[1] - 32.0 + j as f64,
                            ));
                        }
                    }
                }
                std::hint::black_box(&out);
                best = best.min(started.elapsed().as_secs_f64());
            }
            total += best;
        }
        total * 1e9 / (centres.len() * n * n) as f64
    };
    // Warm every cell first.
    time(true);
    let with = time(true);
    let without = time(false);
    println!(
        "grids on roads ({} of 65 x 65 at 1 m): {with:.0} ns a sample, {without:.0} without the grade: the grade {:.0} ns",
        centres.len(),
        with - without
    );
}

/// One point: the sample with and without the roads, and the road near.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_point() {
    let world = world();
    let _ = bake(&world, 62_000.0);
    let x: f64 = std::env::var("AT_X")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let z: f64 = std::env::var("AT_Z")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    for (dx, dz) in [
        (-24.0, 0.0),
        (0.0, 0.0),
        (24.0, 0.0),
        (0.0, -24.0),
        (0.0, 24.0),
    ] {
        let (a, b) = (
            world.sample(x + dx, z + dz),
            world.sample_without_roads(x + dx, z + dz),
        );
        println!(
            "({:.0}, {:.0}): {:.2} without {:.2}, cut {:.2}, cleared {:.2}, developed {:.2}, water {:?}",
            x + dx,
            z + dz,
            a.height,
            b.height,
            a.landform.cut_m,
            a.landform.cleared,
            a.landform.developed,
            a.water.map(|w| (w.kind, w.surface_m, w.signed_distance_m))
        );
    }
    println!("{:?}", world.nearest_road(x, z, 200.0));
}

/// The places the step's captures look at, along the service road that
/// leaves Port Serein's own: its fork, its first bridge, its first cutting
/// over 5 m, its first junction with a track, a track's ford, the nearest
/// hamlet's arrival and a trail's landmark. Printed as `name x z along`.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(clippy::too_many_lines, reason = "a probe's report, in order")]
fn probe_poses() {
    let world = world();
    let networks = bake(&world, 20_000.0);
    let city = world.sites()[0].centre;
    let home = networks
        .iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let roads = home.roads();
    let own = &roads[0];
    let p0 = own.points[0];
    let p1 = own.points[own.points.len().min(12) - 1];
    println!(
        "exit {:.1} {:.1} toward {:.1} {:.1}",
        p0.x, p0.z, p1.x, p1.z
    );
    for p in own.points.iter().step_by(5) {
        println!(
            "  own {:.0}: ({:.1}, {:.1}) ground {:.1}",
            p.along_m, p.x, p.z, p.ground_m
        );
    }
    let show = |r: usize, from: f64, to: f64| {
        let road = &roads[r];
        let mut s = from;
        while s < to.min(road.length_m()) {
            let (p, g) = at(road, s);
            let level = road.level_at(s).map_or(f64::NAN, |l| l.0);
            println!(
                "  road {r} {s:.0}: ({:.1}, {:.1}) ground {g:.1} level {level:.1}",
                p[0], p[1]
            );
            s += 50.0;
        }
    };
    show(54, 0.0, 600.0);
    show(54, 8_400.0, 9_700.0);
    show(54, 3_200.0, 3_450.0);
    // The service road forking from road 0.
    let (r, end) = roads
        .iter()
        .enumerate()
        .find_map(|(r, road)| {
            road.ends.iter().enumerate().find_map(|(side, e)| match e {
                roads::RoadEnd::Junction(j) if home.junctions()[*j as usize].road == 0 => {
                    Some((r, side))
                }
                _ => None,
            })
        })
        .expect("a road off the city's");
    let road = &roads[r];
    let len = road.length_m();
    // Along it away from the fork.
    let along = |t: f64| if end == 0 { t } else { len - t };
    let (fork, _) = at(road, along(0.0));
    println!(
        "fork road {r} {:?} {:.1} {:.1} length {len:.0}",
        road.class, fork[0], fork[1]
    );
    for c in &road.crossings {
        let t = if end == 0 { c.along_m } else { len - c.along_m };
        println!(
            "  crossing {:?} at {:.0} out ({:.1}, {:.1}) span {:.1} deck {:.1} surface {:.1}",
            c.kind, t, c.at[0], c.at[1], c.span_m, c.deck_m, c.surface_m
        );
    }
    let mut run = 0.0;
    let mut t = 0.0;
    while t < len {
        let s = along(t);
        let (p, ground) = at(road, s);
        let (level, _) = road.level_at(s).expect("a profile");
        if ground - level > 5.0 {
            run += 5.0;
            if run >= 30.0 {
                println!(
                    "  cutting at {t:.0} out ({:.1}, {:.1}) depth {:.1}",
                    p[0],
                    p[1],
                    ground - level
                );
                break;
            }
        } else {
            run = 0.0;
        }
        t += 5.0;
    }
    for (j, junction) in home.junctions().iter().enumerate() {
        if junction.road as usize != r {
            continue;
        }
        let arm = roads.iter().position(|o| {
            o.ends.contains(&roads::RoadEnd::Junction(
                u32::try_from(j).expect("a junction"),
            ))
        });
        if let Some(a) = arm {
            let t = if end == 0 {
                junction.along_m
            } else {
                len - junction.along_m
            };
            println!(
                "  junction {j} at {t:.0} out ({:.1}, {:.1}) arm {a} {:?}",
                junction.at[0], junction.at[1], roads[a].class
            );
        }
    }
    // A track's ford and a trail to a landmark, nearest the city.
    let mut fords: Vec<(f64, [f64; 2], usize)> = Vec::new();
    for (k, o) in roads.iter().enumerate() {
        for c in &o.crossings {
            if c.kind == roads::CrossingKind::Ford && o.class == RoadClass::Track {
                let d = ((c.at[0] - city[0]).powi(2) + (c.at[1] - city[1]).powi(2)).sqrt();
                fords.push((d, c.at, k));
            }
        }
    }
    fords.sort_by(|a, b| a.0.total_cmp(&b.0));
    for f in fords.iter().take(3) {
        println!(
            "ford road {} at ({:.1}, {:.1}), {:.0} m from the city",
            f.2, f.1[0], f.1[1], f.0
        );
    }
    for site in world.sites_near(city[0], city[1], 12_000.0).iter().take(12) {
        let road = roads
            .iter()
            .position(|o| o.ends.contains(&roads::RoadEnd::Site(site.id)));
        println!(
            "site {} kind {:?} at ({:.1}, {:.1}) arrival ({:.1}, {:.1}) road {:?} {:?}",
            site.name,
            site.id.kind,
            site.centre[0],
            site.centre[1],
            site.arrival[0],
            site.arrival[1],
            road,
            road.map(|k| roads[k].class)
        );
    }
}

/// A line across a road at a point, every metre: height, cut, slope.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_across() {
    let world = world();
    let _ = bake(&world, 20_000.0);
    let x: f64 = std::env::var("AT_X")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let z: f64 = std::env::var("AT_Z")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let here = world.nearest_road(x, z, 100.0).expect("a road");
    let n = [-here.bearing[1], here.bearing[0]];
    for k in -40..=40 {
        let d = f64::from(k);
        let (px, pz) = (here.at[0] + n[0] * d, here.at[1] + n[1] * d);
        let s = world.sample(px, pz);
        let b = world.sample_without_roads(px, pz);
        println!(
            "{d:4.0}: h {:7.2} natural {:7.2} cut {:6.2} slope {:5.2} (natural {:5.2}) developed {:.2} cleared {:.2} cover {:.2}",
            s.height,
            b.height,
            s.landform.cut_m,
            s.landform.slope,
            b.landform.slope,
            s.landform.developed,
            s.landform.cleared,
            s.cover
        );
    }
}

/// Points of a road round a place, with their positions: for posing a
/// capture.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_line() {
    let world = world();
    let networks = bake(&world, 20_000.0);
    let x: f64 = std::env::var("AT_X")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let z: f64 = std::env::var("AT_Z")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let here = world.nearest_road(x, z, 200.0).expect("a road near");
    let network = networks
        .iter()
        .find(|n| n.key() == here.mass)
        .expect("its mass");
    let road = &network.roads()[here.road as usize];
    let mut s = (here.along_m - 120.0).max(0.0);
    while s < (here.along_m + 120.0).min(road.length_m()) {
        let (p, _) = at(road, s);
        println!("line {s:.0} {:.1} {:.1}", p[0], p[1]);
        s += 20.0;
    }
}

/// The largest second difference along a line every 5 cm (`FROM_X`,
/// `FROM_Z`, `TO_X`, `TO_Z`), with the ground round it: for a crease a test
/// found.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_crease() {
    let world = world();
    let _ = bake(&world, 20_000.0);
    let var = |name: &str| -> f64 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };
    let (from, to) = ([var("FROM_X"), var("FROM_Z")], [var("TO_X"), var("TO_Z")]);
    let length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a probe's step count"
    )]
    let n = (length / 0.05).ceil() as usize;
    #[allow(clippy::cast_precision_loss, reason = "a probe's step count")]
    let at = |k: usize| {
        let f = k as f64 / n as f64;
        [
            from[0] + (to[0] - from[0]) * f,
            from[1] + (to[1] - from[1]) * f,
        ]
    };
    let h: Vec<f64> = (0..=n)
        .map(|k| {
            let p = at(k);
            f64::from(world.sample(p[0], p[1]).height)
        })
        .collect();
    let networks = bake(&world, 20_000.0);
    let roads_of = |q: [f64; 2]| -> Vec<(usize, f64, f64, Option<(f64, f64)>)> {
        let mut out = Vec::new();
        for network in &networks {
            for (r, road) in network.roads().iter().enumerate() {
                let mut best = (f64::INFINITY, 0.0);
                for w in road.points.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let (dx, dz) = (b.x - a.x, b.z - a.z);
                    let l2 = (dx * dx + dz * dz).max(1e-12);
                    let t = (((q[0] - a.x) * dx + (q[1] - a.z) * dz) / l2).clamp(0.0, 1.0);
                    let d = ((q[0] - a.x - dx * t).powi(2) + (q[1] - a.z - dz * t).powi(2)).sqrt();
                    if d < best.0 {
                        best = (d, a.along_m + (b.along_m - a.along_m) * t);
                    }
                }
                if best.0 < 50.0 {
                    out.push((r, best.0, best.1, road.level_at(best.1)));
                }
            }
        }
        out
    };
    let (mut worst, mut k_worst) = (0.0, 0);
    for k in 1..n {
        let b = (h[k + 1] - 2.0 * h[k] + h[k - 1]).abs();
        if b > worst {
            (worst, k_worst) = (b, k);
        }
    }
    let p = at(k_worst);
    println!("crease {worst:.5} at ({:.2}, {:.2})", p[0], p[1]);
    for k in k_worst.saturating_sub(6)..=(k_worst + 6).min(n) {
        let q = at(k);
        let s = world.sample(q[0], q[1]);
        let near = world.nearest_road(q[0], q[1], 60.0);
        println!(
            "{k}: h {:.4} cut {:.3} slope {:.3} road {:?}",
            s.height,
            s.landform.cut_m,
            s.landform.slope,
            near.map(|r| (
                r.road,
                r.class,
                (r.distance_m * 100.0).round() / 100.0,
                r.along_m.round()
            ))
        );
        for (r, d, along, level) in roads_of(q) {
            println!("    road {r}: d {d:.3} along {along:.2} level {level:?}");
        }
    }
}

/// A road's record and profile round a point (`AT_X`, `AT_Z`): each point
/// within 80 m, its ground, level and rise, and its crossings and ends.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_profile() {
    let world = world();
    let networks = bake(&world, 70_000.0);
    let var = |name: &str| -> f64 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };
    let (x, z) = (var("AT_X"), var("AT_Z"));
    let here = world.nearest_road(x, z, 100.0).expect("a road");
    let network = networks
        .iter()
        .find(|n| n.key() == here.mass)
        .expect("its mass");
    let road = &network.roads()[here.road as usize];
    println!(
        "road {} {:?}, {:.0} m, ends {:?}, at {:.1}",
        here.road,
        road.class,
        road.length_m(),
        road.ends,
        here.along_m
    );
    for c in &road.crossings {
        if (c.along_m - here.along_m).abs() < 200.0 {
            println!("  {c:?}");
        }
    }
    for (k, p) in road.points.iter().enumerate() {
        if (p.along_m - here.along_m).abs() < 80.0 {
            println!(
                "  {k}: along {:.1} ground {:.2} profile {:?}",
                p.along_m,
                p.ground_m,
                road.profile.get(k)
            );
        }
    }
}

/// Where the sites named `NAME` (a prefix) stand within 200 km.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_site_named() {
    let world = world();
    let name = std::env::var("NAME").unwrap_or_default();
    for network in bake(&world, 200_000.0) {
        for site in network.sites() {
            if site.name.to_lowercase().contains(&name.to_lowercase()) {
                println!(
                    "{} at ({:.0}, {:.0})",
                    site.name, site.centre[0], site.centre[1]
                );
            }
        }
    }
}

/// The showcase stretch: the service road that leaves Port Serein's own,
/// from the fork, with its crossings and its cuttings deeper than 3 m in
/// order, and a pose's points every 100 m for the first 12 km.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_showcase() {
    let world = world();
    let networks = bake(&world, 20_000.0);
    let city = world.sites()[0].centre;
    let home = networks
        .iter()
        .find(|n| n.roads().first().is_some_and(Road::is_settlements))
        .expect("the city's mass");
    let road0 = &home.roads()[0];
    println!(
        "road 0: {:.0} m from ({:.0}, {:.0}) to ({:.0}, {:.0})",
        road0.length_m(),
        road0.points[0].x,
        road0.points[0].z,
        road0.points[road0.points.len() - 1].x,
        road0.points[road0.points.len() - 1].z
    );
    for (r, road) in home.roads().iter().enumerate() {
        let Some(side) = (0..2).find(|&side| match road.ends[side] {
            RoadEnd::Junction(j) => home.junctions()[j as usize].road == 0,
            _ => false,
        }) else {
            continue;
        };
        println!(
            "road {r} {:?} {:.0} m leaves road 0 at its end {side}, ends {:?}",
            road.class,
            road.length_m(),
            road.ends
        );
        if road.class != RoadClass::Service {
            continue;
        }
        let along = |s: f64| if side == 0 { s } else { road.length_m() - s };
        for c in &road.crossings {
            let s = if side == 0 {
                c.along_m
            } else {
                road.length_m() - c.along_m
            };
            println!(
                "  {:?} {:.0} m out at ({:.0}, {:.0}): span {:.1}, deck {:.1}, water {:.1}, banks {:?}",
                c.kind, s, c.at[0], c.at[1], c.span_m, c.deck_m, c.surface_m, c.banks_m
            );
        }
        let mut s = 0.0;
        let mut run: Option<(f64, f64, [f64; 2])> = None;
        while s < road.length_m().min(15_000.0) {
            let (p, ground) = at(road, along(s));
            let level = road.level_at(along(s)).map_or(ground, |l| l.0);
            let cut = ground - level;
            if cut > 3.0 {
                let held = run.get_or_insert((s, 0.0, p));
                if cut > held.1 {
                    held.1 = cut;
                    held.2 = p;
                }
            } else if let Some((from, deepest, [px, pz])) = run.take() {
                println!(
                    "  cutting {from:.0} to {s:.0} m out, {deepest:.1} m deep at ({px:.0}, {pz:.0})"
                );
            }
            if (s % 500.0) < 1.0 {
                println!(
                    "  pose {s:.0} m: ({:.0}, {:.0}) level {level:.1} ground {ground:.1}",
                    p[0], p[1]
                );
            }
            s += 2.0;
        }
        let _ = city;
    }
}

/// The heading of `road` at `along`, radians: its chord over 10 m.
fn heading(road: &Road, along: f64) -> f64 {
    let (a, _) = at(road, along - 5.0);
    let (b, _) = at(road, along + 5.0);
    libm::atan2(b[1] - a[1], b[0] - a[0])
}

/// The angle between two headings, degrees, in `0..=180`.
fn turn_deg(a: f64, b: f64) -> f64 {
    let mut d = (b - a).abs() % std::f64::consts::TAU;
    if d > std::f64::consts::PI {
        d = std::f64::consts::TAU - d;
    }
    d.to_degrees()
}

/// How a road comes onto a bridge, and how deep it cuts on the way (the
/// alignment pass): within 150 m either side of the deck, the most the
/// line turns over any 50 m and any 150 m and the angle between the deck
/// and the line 150 m out; within 300 m either side the deepest cut.
#[derive(Debug, Clone, Copy)]
struct Approach {
    turn_short: f64,
    turn_long: f64,
    swing: f64,
    cut: f64,
    /// Degrees between the deck's chord and the square to the water's flow.
    skew: f64,
}

fn approach(world: &ProceduralWorld, road: &Road, c: &roads::Crossing, reach: f64) -> Approach {
    let s0 = f64::midpoint(c.abutments_m[0], c.abutments_m[1]);
    let deck = heading(road, s0);
    let length = road.length_m();
    let (mut turn_short, mut turn_long, mut swing) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut s = (s0 - 150.0).max(30.0);
    while s <= (s0 + 150.0).min(length - 30.0) {
        turn_short = turn_short.max(turn_deg(heading(road, s - 25.0), heading(road, s + 25.0)));
        if s - 75.0 > 5.0 && s + 75.0 < length - 5.0 {
            turn_long = turn_long.max(turn_deg(heading(road, s - 75.0), heading(road, s + 75.0)));
        }
        s += 5.0;
    }
    for out in [s0 - 150.0, s0 + 150.0] {
        if out > 5.0 && out < length - 5.0 {
            swing = swing.max(turn_deg(deck, heading(road, out)));
        }
    }
    let mut cut = 0.0_f64;
    let mut s = (s0 - reach).max(0.0);
    while s <= (s0 + reach).min(length) {
        if !in_gap(road, s) {
            let (_, ground) = at(road, s);
            if let Some((level, _)) = road.level_at(s) {
                cut = cut.max(ground - level);
            }
        }
        s += 5.0;
    }
    let (a, _) = at(road, c.abutments_m[0]);
    let (b, _) = at(road, c.abutments_m[1]);
    let chord = libm::atan2(b[1] - a[1], b[0] - a[0]);
    let skew = world.sample(c.at[0], c.at[1]).water.map_or(f64::NAN, |w| {
        let flow = libm::atan2(f64::from(w.flow[1]), f64::from(w.flow[0]));
        (90.0 - turn_deg(chord, flow)).abs()
    });
    Approach {
        turn_short,
        turn_long,
        swing,
        cut,
        skew,
    }
}

/// The alignment pass's measures within 60 km of Port Serein: how every
/// graded road's bridges are approached ([`Approach`]), the cuttings of
/// the service roads (the share of their length in cut over 3 m, the
/// deepest cuttings), the showcase's first bridge (the backbone's out of
/// the city) with the deepest cut within 600 m either side, and the bake.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a probe's report, in order"
)]
fn probe_alignment() {
    let world = world();
    let city = world.sites()[0].centre;
    let started = Instant::now();
    let networks = bake(&world, 62_000.0);
    println!("bake of 62 km: {:.2} s", started.elapsed().as_secs_f64());
    let home = networks
        .iter()
        .find(|n| n.roads().first().is_some_and(Road::is_settlements))
        .expect("the city's mass");
    let st = home.stats();
    println!(
        "home mass: bake {:.3} s, roads {:.3} s of which profiles {:.3} s; {} roads, {:.0} km",
        st.seconds, st.roads_seconds, st.grade_seconds, st.roads, st.road_km
    );
    let within =
        |p: [f64; 2]| (p[0] - city[0]).powi(2) + (p[1] - city[1]).powi(2) < 60_000.0_f64.powi(2);
    for class in [RoadClass::Service, RoadClass::Track] {
        let mut all: Vec<(Approach, [f64; 2])> = Vec::new();
        let mut kinds = [0_usize; 3];
        for network in &networks {
            for road in network.roads() {
                if road.class != class || !road.graded() {
                    continue;
                }
                for c in &road.crossings {
                    if !within(c.at) {
                        continue;
                    }
                    kinds[match c.kind {
                        roads::CrossingKind::Bridge => 0,
                        roads::CrossingKind::Ford => 1,
                        roads::CrossingKind::Culvert => 2,
                    }] += 1;
                    if c.kind == roads::CrossingKind::Bridge {
                        all.push((approach(&world, road, c, 300.0), c.at));
                    }
                }
            }
        }
        let pick = |f: fn(&Approach) -> f64| all.iter().map(|(a, _)| f(a)).collect::<Vec<f64>>();
        let count =
            |f: fn(&Approach) -> f64, over: f64| all.iter().filter(|(a, _)| f(a) > over).count();
        println!(
            "{} bridges {} (fords {}, culverts {}): turn over 50 m median {:.0} deg, p90 {:.0}, most {:.0}, over 45 {} over 60 {} over 80 {}; over 150 m median {:.0}, p90 {:.0}, most {:.0}, over 60 {} over 80 {} over 120 {}; swing median {:.0}, p90 {:.0}, over 60 {}; skew median {:.0}, p90 {:.0}, most {:.0}, over 30 {} over 45 {}; approach cut median {:.1} m, p90 {:.1}, most {:.1}, over 8 m {}",
            name(class),
            all.len(),
            kinds[1],
            kinds[2],
            quantile(&mut pick(|a| a.turn_short), 0.5),
            quantile(&mut pick(|a| a.turn_short), 0.9),
            quantile(&mut pick(|a| a.turn_short), 1.0),
            count(|a| a.turn_short, 45.0),
            count(|a| a.turn_short, 60.0),
            count(|a| a.turn_short, 80.0),
            quantile(&mut pick(|a| a.turn_long), 0.5),
            quantile(&mut pick(|a| a.turn_long), 0.9),
            quantile(&mut pick(|a| a.turn_long), 1.0),
            count(|a| a.turn_long, 60.0),
            count(|a| a.turn_long, 80.0),
            count(|a| a.turn_long, 120.0),
            quantile(&mut pick(|a| a.swing), 0.5),
            quantile(&mut pick(|a| a.swing), 0.9),
            count(|a| a.swing, 60.0),
            quantile(&mut pick(|a| a.skew), 0.5),
            quantile(&mut pick(|a| a.skew), 0.9),
            quantile(&mut pick(|a| a.skew), 1.0),
            count(|a| a.skew, 30.0),
            count(|a| a.skew, 45.0),
            quantile(&mut pick(|a| a.cut), 0.5),
            quantile(&mut pick(|a| a.cut), 0.9),
            quantile(&mut pick(|a| a.cut), 1.0),
            count(|a| a.cut, 8.0),
        );
        all.sort_by(|a, b| b.0.turn_short.total_cmp(&a.0.turn_short));
        for (a, p) in all.iter().take(5) {
            println!(
                "  sharpest at ({:.0}, {:.0}): 50 m {:.0}, 150 m {:.0}, swing {:.0}, skew {:.0}, cut {:.1}",
                p[0], p[1], a.turn_short, a.turn_long, a.swing, a.skew, a.cut
            );
        }
        // Cuttings along the class's whole length within 60 km.
        let (mut length, mut over3, mut over8) = (0.0_f64, 0.0, 0.0);
        let mut over8_crossing = 0.0;
        let mut deepest: Vec<(f64, [f64; 2])> = Vec::new();
        for network in &networks {
            for road in network.roads() {
                if road.class != class || !road.graded() {
                    continue;
                }
                let mut run: Option<(f64, [f64; 2])> = None;
                let mut s = 0.0;
                while s <= road.length_m() {
                    let (p, ground) = at(road, s);
                    if within(p) && !in_gap(road, s) {
                        let (level, _) = road.level_at(s).expect("a profile");
                        let cut = ground - level;
                        length += 5.0;
                        if cut > 3.0 {
                            over3 += 5.0;
                            let held = run.get_or_insert((cut, p));
                            if cut > held.0 {
                                *held = (cut, p);
                            }
                        } else if let Some(r) = run.take() {
                            deepest.push(r);
                        }
                        if cut > 8.0 {
                            over8 += 5.0;
                            if road.crossings.iter().any(|c| (c.along_m - s).abs() < 300.0) {
                                over8_crossing += 5.0;
                            }
                        }
                    }
                    s += 5.0;
                }
                if let Some(r) = run.take() {
                    deepest.push(r);
                }
            }
        }
        deepest.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!(
            "  {:.0} km: in cut over 3 m {:.1} %, over 8 m {:.2} % ({:.0} % of it within 300 m of a crossing); cuttings over 3 m {}, over 8 m {}, over 12 m {}, over 16 m {}; deepest {:?}",
            length / 1e3,
            100.0 * over3 / length.max(1.0),
            100.0 * over8 / length.max(1.0),
            100.0 * over8_crossing / over8.max(1.0),
            deepest.len(),
            deepest.iter().filter(|d| d.0 > 8.0).count(),
            deepest.iter().filter(|d| d.0 > 12.0).count(),
            deepest.iter().filter(|d| d.0 > 16.0).count(),
            deepest
                .iter()
                .take(6)
                .map(|(d, p)| format!("{d:.1} at ({:.0}, {:.0})", p[0], p[1]))
                .collect::<Vec<_>>()
        );
    }
    // The showcase: the service road off Port Serein's own, its first
    // bridge from the fork.
    for road in home.roads() {
        let Some(side) = (0..2).find(|&side| match road.ends[side] {
            RoadEnd::Junction(j) => home.junctions()[j as usize].road == 0,
            _ => false,
        }) else {
            continue;
        };
        if road.class != RoadClass::Service {
            continue;
        }
        let mut bridges: Vec<&roads::Crossing> = road
            .crossings
            .iter()
            .filter(|c| c.kind == roads::CrossingKind::Bridge)
            .collect();
        if side == 1 {
            bridges.reverse();
        }
        let Some(c) = bridges.first() else { continue };
        let a = approach(&world, road, c, 600.0);
        let s0 = f64::midpoint(c.abutments_m[0], c.abutments_m[1]);
        let cut_side = |from: f64, to: f64| {
            let (mut most, mut at_p) = (0.0_f64, [0.0, 0.0]);
            let mut s = from.max(0.0);
            while s <= to.min(road.length_m()) {
                if !in_gap(road, s) {
                    let (p, ground) = at(road, s);
                    if let Some((level, _)) = road.level_at(s)
                        && ground - level > most
                    {
                        most = ground - level;
                        at_p = p;
                    }
                }
                s += 2.0;
            }
            (most, at_p)
        };
        let (before, after) = (cut_side(s0 - 600.0, s0), cut_side(s0, s0 + 600.0));
        println!(
            "showcase: service road of {:.0} m, first bridge at ({:.0}, {:.0}), {:.0} m along, span {:.1}, bearing ({:.2}, {:.2}): turn over 50 m {:.0} deg, over 150 m {:.0}, swing {:.0}, skew {:.0}; deepest cut within 600 m {:.1} at ({:.0}, {:.0}) and {:.1} at ({:.0}, {:.0})",
            road.length_m(),
            c.at[0],
            c.at[1],
            if side == 0 { s0 } else { road.length_m() - s0 },
            c.abutments_m[1] - c.abutments_m[0],
            c.bearing[0],
            c.bearing[1],
            a.turn_short,
            a.turn_long,
            a.swing,
            a.skew,
            before.0,
            before.1[0],
            before.1[1],
            after.0,
            after.1[0],
            after.1[1],
        );
        // The steepest the profile climbs over any 10 m within 300 m either
        // side, and every crossing within 600 m: what the line asks of the
        // kit there.
        let steepest = |from: f64, to: f64| {
            let (mut most, mut at_s) = (0.0_f64, from);
            let mut s = from.max(0.0);
            while s + 10.0 <= to.min(road.length_m()) {
                if let (Some((a, _)), Some((b, _))) = (road.level_at(s), road.level_at(s + 10.0))
                    && (b - a).abs() / 10.0 > most
                {
                    most = (b - a).abs() / 10.0;
                    at_s = s;
                }
                s += 2.0;
            }
            (100.0 * most, at_s - s0)
        };
        let (back, back_s) = steepest(s0 - 300.0, s0);
        let (on, on_s) = steepest(s0, s0 + 300.0);
        println!(
            "  steepest grade within 300 m: {back:.1} % at {back_s:+.0} m and {on:.1} % at {on_s:+.0} m"
        );
        for x in &road.crossings {
            let mid = f64::midpoint(x.abutments_m[0], x.abutments_m[1]);
            if (mid - s0).abs() <= 600.0 {
                println!(
                    "  crossing {:?} at ({:.0}, {:.0}) {:+.0} m: gap {:.1}, deck {:.1}, surface {:.1}, banks {:.1} / {:.1}, width {:.1}",
                    x.kind,
                    x.at[0],
                    x.at[1],
                    mid - s0,
                    x.abutments_m[1] - x.abutments_m[0],
                    x.deck_m,
                    x.surface_m,
                    x.banks_m[0],
                    x.banks_m[1],
                    x.width_m
                );
            }
        }
        let mut s = s0 - 300.0;
        while s <= s0 + 300.0 {
            let (p, ground) = at(road, s);
            let level = road.level_at(s).map_or(f64::NAN, |l| l.0);
            println!(
                "  {:+.0}: ({:.0}, {:.0}) heading {:.0} ground {:.1} level {:.1}",
                s - s0,
                p[0],
                p[1],
                heading(road, s).to_degrees(),
                ground,
                level
            );
            s += 25.0;
        }
    }
}

/// The ground on the road search's 64 m nodes (or every `AT_STEP` metres)
/// round a point (`AT_X`, `AT_Z`, default the showcase's first bridge),
/// without the roads: what
/// the search sees of a valley, a row a line, north (`+z`) first.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_nodes() {
    let world = world();
    let _ = bake(&world, 62_000.0);
    let var = |name: &str, or: f64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(or)
    };
    let (x, z) = (var("AT_X", 7_404.0), var("AT_Z", -4_940.0));
    let half = var("AT_HALF", 12.0);
    // `AT_STEP` 16 shows the ground the wall search near a crossing sees.
    let step = var("AT_STEP", 64.0);
    #[allow(clippy::cast_possible_truncation, reason = "a node index")]
    let (ci, cj, h, st) = (
        (x / step).round() as i32,
        (z / step).round() as i32,
        half as i32,
        step as i32,
    );
    print!("{:>7}", "");
    for i in ci - h..=ci + h {
        print!("{:>6}", i * st);
    }
    println!();
    for j in (cj - h..=cj + h).rev() {
        print!("{:>7}", j * st);
        for i in ci - h..=ci + h {
            let s = world.sample_without_roads(f64::from(i) * step, f64::from(j) * step);
            let mark = if s.in_water() { '~' } else { ' ' };
            print!("{:>5.0}{mark}", s.height);
        }
        println!();
    }
}
