//! The measures behind ticket 0005's step 4 record (procedural sites):
//! ignored probes, run in release, one at a time:
//!
//! ```text
//! cargo test --release -p strata-worldgen --test sites_measure -- --ignored --nocapture --test-threads=1
//! ```

mod common;

use std::time::Instant;

use strata_core::Seed;
use strata_worldgen::hydrology::LATTICE_M;
use strata_worldgen::sites::ProceduralSite;
use strata_worldgen::{ProceduralWorld, WorldSource};

/// Sites by kind within 12, 60 and 200 km of Port Serein, the nearest of
/// each kind, the spacing achieved, and what placement adds to the bake of
/// the city's mass.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a probe's report, in order, of counts far under 2^53"
)]
fn probe_sites() {
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let city = world.sites()[0].centre;
    let started = Instant::now();
    let home = world
        .hydrology()
        .bake_area(&world, (city[0], city[1]), (city[0], city[1]));
    let home = home
        .iter()
        .find(|n| {
            n.sites()
                .iter()
                .any(|s| ((s.centre[0] - city[0]).hypot(s.centre[1] - city[1])) < 30_000.0)
        })
        .expect("the city's mass");
    let stats = home.stats();
    println!(
        "home mass {:?}: {} nodes ({:.0} km2), bake {:.3} s of which sites {:.3} s, {} sites; probe {:.2} s",
        home.key(),
        stats.nodes,
        stats.nodes as f64 * LATTICE_M * LATTICE_M / 1e6,
        stats.seconds,
        stats.sites_seconds,
        stats.sites,
        started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let all = world.hydrology().bake_area(
        &world,
        (city[0] - 200_000.0, city[1] - 200_000.0),
        (city[0] + 200_000.0, city[1] + 200_000.0),
    );
    println!(
        "{} masses within 200 km baked in {:.2} s more, placement {:.2} s of their own bakes",
        all.len(),
        started.elapsed().as_secs_f64(),
        all.iter().map(|n| n.stats().sites_seconds).sum::<f64>()
    );
    let sites: Vec<&ProceduralSite> = all.iter().flat_map(|n| n.sites().iter()).collect();
    let distance = |s: &ProceduralSite| (s.centre[0] - city[0]).hypot(s.centre[1] - city[1]);
    let kinds = world.site_kinds();
    println!("kind              12 km  60 km 200 km  nearest");
    for kind in kinds {
        let of: Vec<&&ProceduralSite> = sites.iter().filter(|s| s.kind() == kind.id).collect();
        let within = |r: f64| of.iter().filter(|s| distance(s) <= r).count();
        let nearest = of.iter().min_by(|a, b| distance(a).total_cmp(&distance(b)));
        println!(
            "{:<16} {:6} {:6} {:6}  {}",
            kind.name.to_string(),
            within(12_000.0),
            within(60_000.0),
            within(200_000.0),
            nearest.map_or("-".to_owned(), |s| format!(
                "{} at ({:.0}, {:.0}), {:.2} km, level {:.1} m, facing ({:.2}, {:.2}), arrival ({:.0}, {:.0})",
                s.name,
                s.centre[0],
                s.centre[1],
                distance(s) / 1000.0,
                s.level_m,
                s.facing[0],
                s.facing[1],
                s.arrival[0],
                s.arrival[1]
            ))
        );
    }
    // The spacing achieved within 60 km: per kind, its nearest of its own
    // kind (median and least), and the least to any other.
    println!("kind              own: least median   others: least");
    for kind in kinds {
        let of: Vec<&&ProceduralSite> = sites
            .iter()
            .filter(|s| s.kind() == kind.id && distance(s) <= 60_000.0)
            .collect();
        let mut own: Vec<f64> = of
            .iter()
            .filter_map(|s| {
                sites
                    .iter()
                    .filter(|o| o.kind() == kind.id && o.id != s.id)
                    .map(|o| (o.centre[0] - s.centre[0]).hypot(o.centre[1] - s.centre[1]))
                    .min_by(f64::total_cmp)
            })
            .collect();
        own.sort_by(f64::total_cmp);
        let others = of
            .iter()
            .filter_map(|s| {
                sites
                    .iter()
                    .filter(|o| o.kind() != kind.id)
                    .map(|o| (o.centre[0] - s.centre[0]).hypot(o.centre[1] - s.centre[1]))
                    .min_by(f64::total_cmp)
            })
            .min_by(f64::total_cmp);
        println!(
            "{:<16} {:>10.0} {:>7.0} {:>15.0}",
            kind.name.to_string(),
            own.first().copied().unwrap_or(f64::NAN),
            own.get(own.len() / 2).copied().unwrap_or(f64::NAN),
            others.unwrap_or(f64::NAN)
        );
    }
    println!("within 12 km:");
    let mut near: Vec<&&ProceduralSite> =
        sites.iter().filter(|s| distance(s) <= 12_000.0).collect();
    near.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    for s in near {
        let kind = kinds.iter().find(|k| k.id == s.kind()).expect("a kind");
        let ground = world.sample(s.centre[0], s.centre[1]);
        println!(
            "  {:<22} {:<12} ({:7.0}, {:7.0}) {:5.2} km  level {:6.1} m cut {:4.1} by {:?} score {:.2} slope {:.3} facing ({:.2}, {:.2})",
            s.name,
            kind.def.name,
            s.centre[0],
            s.centre[1],
            distance(s) / 1000.0,
            s.level_m,
            s.cut_m,
            s.by,
            s.score,
            ground.landform.slope,
            s.facing[0],
            s.facing[1],
        );
    }
}

/// How far each site within 60 km of the city stands from the water it is
/// named for: from the pad's centre and from its edge, out to the first
/// point in 32 directions, every 2 m, with standing water, a channel or the
/// sea; median and 90th percentile by kind and by the feature it stands by.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a probe's report: counts far under 2^53, a quantile's index"
)]
fn probe_water_in_sight() {
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let city = world.sites()[0].centre;
    let all = world.hydrology().bake_area(
        &world,
        (city[0] - 60_000.0, city[1] - 60_000.0),
        (city[0] + 60_000.0, city[1] + 60_000.0),
    );
    let wet = |x: f64, z: f64| {
        let s = world.sample(x, z);
        s.height < 0.0
            || s.standing_water().is_some()
            || s.water.is_some_and(|w| {
                w.kind != strata_worldgen::hydrology::WaterKind::Lake
                    && w.signed_distance_m.abs() <= 0.5 * w.width_m
            })
    };
    let mut rows: std::collections::BTreeMap<String, Vec<f64>> = std::collections::BTreeMap::new();
    for site in all.iter().flat_map(|n| n.sites().iter()) {
        if (site.centre[0] - city[0]).hypot(site.centre[1] - city[1]) > 60_000.0 {
            continue;
        }
        let kind = world
            .site_kinds()
            .iter()
            .find(|k| k.id == site.kind())
            .expect("a kind");
        let mut nearest = f64::INFINITY;
        for k in 0..32 {
            let a = std::f64::consts::TAU * f64::from(k) / 32.0;
            let mut d = 0.0;
            while d < 600.0 && d < nearest {
                if wet(site.centre[0] + d * a.cos(), site.centre[1] + d * a.sin()) {
                    nearest = d;
                    break;
                }
                d += 2.0;
            }
        }
        let by = site.by.map_or("-".to_owned(), |f| format!("{f:?}"));
        for key in [kind.def.name.clone(), format!("{} by {by}", kind.def.name)] {
            rows.entry(key).or_default().push(nearest - site.radius_m);
        }
    }
    println!("pad edge to water (m), within 60 km; 600+ is none within 600 m of the centre");
    for (key, mut edge) in rows {
        edge.sort_by(f64::total_cmp);
        let at = |q: f64| edge[((edge.len() - 1) as f64 * q) as usize];
        println!(
            "  {key:<28} n {:4}  least {:6.0}  median {:6.0}  90th {:6.0}",
            edge.len(),
            at(0.0),
            at(0.5),
            at(0.9)
        );
    }
}

/// What a sample costs on a pad and far from one, one thread: the points
/// of every pad within 30 km of the city against points of the same land a
/// kilometre from any site, each set sampled five times over, best of five.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(clippy::cast_precision_loss, reason = "counts far under 2^53")]
fn probe_sample_cost() {
    let world = common::world(42);
    let city = world.sites()[0].centre;
    let sites: Vec<ProceduralSite> = world
        .hydrology()
        .networks()
        .iter()
        .flat_map(|n| n.sites().iter().cloned())
        .filter(|s| (s.centre[0] - city[0]).hypot(s.centre[1] - city[1]) < 30_000.0)
        .collect();
    let mut on = Vec::new();
    for site in &sites {
        for k in 0..16 {
            let a = std::f64::consts::TAU * f64::from(k) / 16.0;
            let r = site.radius_m * 0.8 * f64::from(k % 4 + 1) / 4.0;
            on.push((site.centre[0] + r * a.cos(), site.centre[1] + r * a.sin()));
        }
    }
    let mut off = Vec::new();
    for &(x, z) in &on {
        let (x, z) = (x + 1_000.0, z + 1_000.0);
        let clear = sites
            .iter()
            .all(|s| (s.centre[0] - x).hypot(s.centre[1] - z) > s.outer_m() + 200.0);
        if clear && world.sample(x, z).height > 0.0 {
            off.push((x, z));
        }
    }
    let time = |world: &ProceduralWorld, points: &[(f64, f64)]| {
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let started = Instant::now();
            let mut sum = 0.0_f32;
            for _ in 0..5 {
                for &(x, z) in points {
                    sum += world.sample(x, z).height;
                }
            }
            std::hint::black_box(sum);
            best = best.min(started.elapsed().as_secs_f64() / (5.0 * points.len() as f64));
        }
        best * 1e6
    };
    // The same points in a world without site kinds: what the pads add.
    let mut registries = common::registries();
    registries.sites = strata_registry::SiteKindStore::default();
    let bare = ProceduralWorld::new(Seed(42), &registries);
    bare.hydrology().set_threads(common::THREADS);
    bare.prepare_water(city[0], city[1], 32_000.0);
    // Warm every cell first.
    for w in [&world, &bare] {
        let _ = time(w, &on);
        let _ = time(w, &off);
    }
    println!(
        "{} points on {} pads: {:.3} us a sample, {:.3} us without sites; {} points off them: {:.3} us, {:.3} us without",
        on.len(),
        sites.len(),
        time(&world, &on),
        time(&bare, &on),
        off.len(),
        time(&world, &off),
        time(&bare, &off),
    );
}

/// What the sites add to a startup's bake: every mass within 128 km of the
/// origin (the game's `PrepareWater` radius round the default spawn), on 16
/// threads, with and without the shipped site kinds. Two fresh caches, one
/// after the other.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_startup_bake() {
    let registries = common::registries();
    let mut bare_registries = registries.clone();
    bare_registries.sites = strata_registry::SiteKindStore::default();
    for (label, registries) in [
        ("without sites", &bare_registries),
        ("with sites", &registries),
    ] {
        let world = ProceduralWorld::new(Seed(42), registries);
        world.hydrology().set_threads(common::THREADS);
        let started = Instant::now();
        let masses = world.prepare_water(0.0, 0.0, 128_000.0);
        let networks = world.hydrology().networks();
        println!(
            "{label}: {masses} masses within 128 km in {:.2} s, placement {:.2} s of it, {} sites",
            started.elapsed().as_secs_f64(),
            networks
                .iter()
                .map(|n| n.stats().sites_seconds)
                .sum::<f64>(),
            networks.iter().map(|n| n.sites().len()).sum::<usize>(),
        );
        let mut by_size: Vec<(usize, f64, f64)> = networks
            .iter()
            .map(|n| (n.stats().nodes, n.stats().seconds, n.stats().sites_seconds))
            .collect();
        by_size.sort_by_key(|m| std::cmp::Reverse(m.0));
        for (nodes, seconds, sites) in by_size.iter().take(6) {
            println!("  {nodes} nodes: bake {seconds:.3} s, placement {sites:.3} s");
        }
        let small: Vec<_> = by_size.iter().filter(|m| m.0 < 20_000).collect();
        println!(
            "  {} masses under 20 000 nodes: bake {:.3} s, placement {:.3} s",
            small.len(),
            small.iter().map(|m| m.1).sum::<f64>(),
            small.iter().map(|m| m.2).sum::<f64>()
        );
    }
}
