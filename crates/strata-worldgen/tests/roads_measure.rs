//! The measures behind ticket 0005's step 5 record (the road network):
//! ignored probes, run in release, one at a time:
//!
//! ```text
//! cargo test --release -p strata-worldgen --test roads_measure -- --ignored --nocapture --test-threads=1
//! ```

mod common;

use std::collections::HashMap;
use std::time::Instant;

use strata_core::Seed;
use strata_worldgen::ProceduralWorld;
use strata_worldgen::hydrology::{LATTICE_M, Network, WaterKind};
use strata_worldgen::roads::{self, Road, RoadEnd};
use strata_worldgen::sites::{ProceduralSite, SiteId};

/// The bake of Port Serein's mass with its roads: the time on 16 threads,
/// what the roads add, and their counts.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(clippy::cast_precision_loss, reason = "a probe's counts")]
fn probe_bake() {
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
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let stats = home.stats();
    println!(
        "home mass {:?}: {} nodes ({:.0} km2), bake {:.3} s: sites {:.3} s ({}), roads {:.3} s ({} roads, {:.0} km, {} sites unreached); probe {:.2} s",
        home.key(),
        stats.nodes,
        stats.nodes as f64 * LATTICE_M * LATTICE_M / 1e6,
        stats.seconds,
        stats.sites_seconds,
        stats.sites,
        stats.roads_seconds,
        stats.roads,
        stats.road_km,
        stats.unreached,
        started.elapsed().as_secs_f64()
    );
}

/// Where a road meets the rest of the network, for routing over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Site(SiteId),
    Exit(u32),
    Head(u32),
    Junction(u32),
}

fn end_key(end: RoadEnd) -> Key {
    match end {
        RoadEnd::Site(id) => Key::Site(id),
        RoadEnd::Settlement(s) => Key::Exit(s),
        RoadEnd::RoadHead(s) => Key::Head(s),
        RoadEnd::Junction(j) => Key::Junction(j),
    }
}

/// One stretch of one road between two keys.
#[derive(Debug, Clone, Copy)]
struct Stretch {
    road: usize,
    from: f64,
    to: f64,
}

/// The network of one mass as a graph of its ends and junctions.
struct Graph {
    keys: HashMap<Key, usize>,
    edges: Vec<Vec<(usize, Stretch)>>,
}

impl Graph {
    fn new(network: &Network) -> Self {
        let mut keys: HashMap<Key, usize> = HashMap::new();
        let mut edges: Vec<Vec<(usize, Stretch)>> = Vec::new();
        let mut id = |key: Key, edges: &mut Vec<Vec<(usize, Stretch)>>| {
            let n = keys.len();
            *keys.entry(key).or_insert_with(|| {
                edges.push(Vec::new());
                n
            })
        };
        for (r, road) in network.roads().iter().enumerate() {
            let length = road.length_m();
            let mut cuts: Vec<(f64, Key)> = vec![
                (0.0, end_key(road.ends[0])),
                (length, end_key(road.ends[1])),
            ];
            for (j, junction) in network.junctions().iter().enumerate() {
                if junction.road as usize == r {
                    cuts.push((
                        junction.along_m,
                        Key::Junction(u32::try_from(j).expect("a junction index")),
                    ));
                }
            }
            cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in cuts.windows(2) {
                let (a, b) = (id(pair[0].1, &mut edges), id(pair[1].1, &mut edges));
                let s = Stretch {
                    road: r,
                    from: pair[0].0,
                    to: pair[1].0,
                };
                edges[a].push((b, s));
                edges[b].push((
                    a,
                    Stretch {
                        road: r,
                        from: pair[1].0,
                        to: pair[0].0,
                    },
                ));
            }
        }
        Self { keys, edges }
    }

    /// The shortest way from `a` to `b`: its stretches in order.
    fn route(&self, a: Key, b: Key) -> Option<Vec<Stretch>> {
        let (&start, &goal) = (self.keys.get(&a)?, self.keys.get(&b)?);
        let mut best = vec![f64::INFINITY; self.edges.len()];
        let mut back: Vec<Option<(usize, Stretch)>> = vec![None; self.edges.len()];
        let mut heap = std::collections::BinaryHeap::new();
        best[start] = 0.0;
        heap.push((std::cmp::Reverse(0_u64), start));
        while let Some((std::cmp::Reverse(d), n)) = heap.pop() {
            #[allow(clippy::cast_precision_loss)]
            let d = d as f64 / 1e3;
            if d > best[n] + 1e-6 {
                continue;
            }
            if n == goal {
                break;
            }
            for &(m, s) in &self.edges[n] {
                let nd = d + (s.to - s.from).abs();
                if nd < best[m] {
                    best[m] = nd;
                    back[m] = Some((n, s));
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    heap.push((std::cmp::Reverse((nd * 1e3) as u64), m));
                }
            }
        }
        if !best[goal].is_finite() {
            return None;
        }
        let mut out = Vec::new();
        let mut at = goal;
        while let Some((prev, s)) = back[at] {
            out.push(s);
            at = prev;
        }
        out.reverse();
        Some(out)
    }
}

/// The ground under a road at `along`, between its points.
#[allow(
    clippy::many_single_char_names,
    reason = "a road's points, an index and the share between"
)]
fn ground_at(road: &Road, along: f64) -> f64 {
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
    f64::from(a.ground_m) + (f64::from(b.ground_m) - f64::from(a.ground_m)) * t
}

/// The ground's grade at the scale of a road's profile, every 10 m: over a
/// 100 m base, the ground averaged over 40 m at each end, so the relief's
/// finest bumps (which any grade cuts and fills) do not read as climbs.
fn profile_grades(road: &Road) -> Vec<f64> {
    let length = road.length_m();
    let mean = |s: f64| {
        let mut sum = 0.0;
        for k in -4..=4 {
            sum += ground_at(road, (s + 5.0 * f64::from(k)).clamp(0.0, length));
        }
        sum / 9.0
    };
    let mut out = Vec::new();
    let mut s = 0.0;
    while s <= length {
        let (a, b) = ((s - 50.0).max(0.0), (s + 50.0).min(length));
        if b - a > 40.0 {
            out.push((mean(b) - mean(a)).abs() / (b - a));
        }
        s += 10.0;
    }
    out
}

/// The ground's grade along a road over a 50 m base, every 10 m: what a
/// road on that line climbs before any grade cuts or fills it.
fn grades(road: &Road) -> Vec<f64> {
    let length = road.length_m();
    let mut out = Vec::new();
    let mut s = 0.0;
    while s <= length {
        let (a, b) = ((s - 25.0).max(0.0), (s + 25.0).min(length));
        if b - a > 10.0 {
            out.push((ground_at(road, b) - ground_at(road, a)).abs() / (b - a));
        }
        s += 10.0;
    }
    out
}

/// The measures of ticket 0005's step 5 record: road by class and crossings
/// by kind within 12, 60 and 200 km of Port Serein; the route from the city
/// to its nearest town, hamlet and landmark; how much of the network runs
/// within 150 m of a river; and the share of each class over its gradient.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    reason = "a probe's report, in order"
)]
fn probe_measures() {
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let city = world.sites()[0].centre;
    let networks = world.hydrology().bake_area(
        &world,
        (city[0] - 200_000.0, city[1] - 200_000.0),
        (city[0] + 200_000.0, city[1] + 200_000.0),
    );
    let from_city = |p: [f64; 2]| ((p[0] - city[0]).powi(2) + (p[1] - city[1]).powi(2)).sqrt();
    println!("within      service km  track km  trail km   bridges fords culverts");
    for r in [12_000.0, 60_000.0, 200_000.0] {
        let mut km = [0.0; 3];
        let mut kinds = [0; 3];
        for network in &networks {
            for road in network.roads() {
                let c = road.class as usize;
                for pair in road.points.windows(2) {
                    if from_city([pair[0].x, pair[0].z]) <= r {
                        km[c] += (pair[1].along_m - pair[0].along_m) / 1e3;
                    }
                }
                for x in &road.crossings {
                    if from_city(x.at) <= r {
                        kinds[x.kind as usize] += 1;
                    }
                }
            }
        }
        println!(
            "{:>4.0} km    {:10.1} {:9.1} {:9.1}   {:7} {:5} {:8}",
            r / 1e3,
            km[0],
            km[1],
            km[2],
            kinds[0],
            kinds[1],
            kinds[2]
        );
    }
    // Near a river, and over the grade, within 60 km.
    let (mut near, mut total) = (0.0, 0.0);
    let mut over = [(0.0, 0.0); 3];
    let mut dry = [(0.0_f64, 0.0_f64); 3];
    let mut profile = [(0.0_f64, 0.0_f64); 3];
    for network in &networks {
        for road in network.roads() {
            if !road
                .points
                .iter()
                .any(|p| from_city([p.x, p.z]) <= 60_000.0)
            {
                continue;
            }
            for pair in road.points.windows(2) {
                let l = pair[1].along_m - pair[0].along_m;
                total += l;
                if network.water_at(pair[0].x, pair[0].z).is_some_and(|w| {
                    w.kind != WaterKind::Lake && f64::from(w.signed_distance_m.abs()) < 150.0
                }) {
                    near += l;
                }
            }
            let limit = roads::spec(road.class).max_grade;
            let g = grades(road);
            let c = road.class as usize;
            let p = profile_grades(road);
            profile[c].0 += p.iter().filter(|&&x| x > limit).count() as f64;
            profile[c].1 += p.len() as f64;
            over[c].0 += g.iter().filter(|&&x| x > limit).count() as f64;
            over[c].1 += g.len() as f64;
            // Away from crossings: the valley's carved walls a crossing's
            // approaches take are the grade's to fill.
            for (k, &x) in g.iter().enumerate() {
                let along = 10.0 * k as f64;
                if road
                    .crossings
                    .iter()
                    .any(|c| (c.along_m - along).abs() < 80.0)
                {
                    continue;
                }
                dry[c].1 += 1.0;
                if x > limit {
                    dry[c].0 += 1.0;
                }
            }
        }
    }
    println!(
        "within 60 km: {:.1} % of {:.0} km within 150 m of a river; over its grade: service {:.1} %, track {:.1} %, trail {:.1} %",
        100.0 * near / total,
        total / 1e3,
        100.0 * over[0].0 / over[0].1.max(1.0),
        100.0 * over[1].0 / over[1].1.max(1.0),
        100.0 * over[2].0 / over[2].1.max(1.0),
    );
    println!(
        "  away from crossings: service {:.1} %, track {:.1} %, trail {:.1} %",
        100.0 * dry[0].0 / dry[0].1.max(1.0),
        100.0 * dry[1].0 / dry[1].1.max(1.0),
        100.0 * dry[2].0 / dry[2].1.max(1.0),
    );
    println!(
        "  at a profile's scale (100 m, ground averaged over 40 m): service {:.1} %, track {:.1} %, trail {:.1} %",
        100.0 * profile[0].0 / profile[0].1.max(1.0),
        100.0 * profile[1].0 / profile[1].1.max(1.0),
        100.0 * profile[2].0 / profile[2].1.max(1.0),
    );
    // The routes out of the city.
    let home = networks
        .iter()
        .find(|n| {
            n.roads()
                .iter()
                .any(|r| r.ends.contains(&RoadEnd::Settlement(0)))
        })
        .expect("the city's mass has its road");
    let graph = Graph::new(home);
    let kinds = world.site_kinds();
    for name in ["Town site", "Hamlet", "Landmark"] {
        let kind = kinds
            .iter()
            .find(|k| k.def.name == name)
            .expect("a shipped kind");
        let mut of: Vec<&ProceduralSite> = home
            .sites()
            .iter()
            .filter(|s| s.kind() == kind.id)
            .collect();
        of.sort_by(|a, b| from_city(a.centre).total_cmp(&from_city(b.centre)));
        let Some(site) = of.first() else { continue };
        let Some(route) = graph.route(Key::Exit(0), Key::Site(site.id)) else {
            println!(
                "{name}: {} at {:.1} km, no route",
                site.name,
                from_city(site.centre) / 1e3
            );
            continue;
        };
        let (mut length, mut climb, mut steepest) = (0.0, 0.0, 0.0_f64);
        let mut crossings = Vec::new();
        let mut steep_at = String::new();
        for s in &route {
            let road = &home.roads()[s.road];
            length += (s.to - s.from).abs();
            let (lo, hi) = (s.from.min(s.to), s.from.max(s.to));
            let mut a = s.from;
            let step = if s.to >= s.from { 10.0 } else { -10.0 };
            while (step > 0.0 && a + step <= s.to) || (step < 0.0 && a + step >= s.to) {
                let rise = ground_at(road, a + step) - ground_at(road, a);
                climb += rise.max(0.0);
                a += step;
            }
            let mut b = lo;
            while b + 100.0 <= hi {
                let g = (ground_at(road, b + 100.0) - ground_at(road, b)).abs() / 100.0;
                if g > steepest {
                    steepest = g;
                    let k = road.points.partition_point(|q| q.along_m <= b);
                    let p = road.points[k.min(road.points.len() - 1)];
                    steep_at = format!(
                        "road {} {:?} at ({:.0}, {:.0}) {:.0} m along",
                        s.road, road.class, p.x, p.z, b
                    );
                }
                b += 10.0;
            }
            for x in &road.crossings {
                if x.along_m >= lo && x.along_m <= hi {
                    crossings.push(format!(
                        "{:?} {:.0} m wide, span {:.0} m",
                        x.kind, x.width_m, x.span_m
                    ));
                }
            }
        }
        println!(
            "{name}: {} at {:.1} km as the crow flies: {:.1} km by road over {} roads, climb {:.0} m, steepest 100 m {:.1} %, crossings {}",
            site.name,
            from_city(site.centre) / 1e3,
            length / 1e3,
            route.len(),
            climb,
            100.0 * steepest,
            if crossings.is_empty() {
                "none".to_string()
            } else {
                crossings.join("; ")
            }
        );
        println!("  steepest on {steep_at}");
    }
    let stats = home.stats();
    println!(
        "the city's mass: {} roads, {:.0} km, {} junctions, {} sites unreached of {}",
        stats.roads,
        stats.road_km,
        home.junctions().len(),
        stats.unreached,
        stats.sites
    );
}

/// The longest stretches over their class's grade within 25 km of the city,
/// away from crossings: where to look.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(clippy::cast_precision_loss, reason = "a probe's report")]
fn probe_steep() {
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let city = world.sites()[0].centre;
    let networks = world
        .hydrology()
        .bake_area(&world, (city[0], city[1]), (city[0], city[1]));
    let mut found = Vec::new();
    for network in &networks {
        for (r, road) in network.roads().iter().enumerate() {
            let limit = roads::spec(road.class).max_grade;
            let g = grades(road);
            let mut run = 0;
            for (k, &x) in g.iter().enumerate() {
                let along = 10.0 * k as f64;
                let near_crossing = road
                    .crossings
                    .iter()
                    .any(|c| (c.along_m - along).abs() < 80.0);
                if x > limit && !near_crossing {
                    run += 1;
                } else {
                    if run >= 8 {
                        let s = 10.0 * (k - run) as f64;
                        let p = road.points[road
                            .points
                            .partition_point(|q| q.along_m <= s)
                            .min(road.points.len() - 1)];
                        let d = ((p.x - city[0]).powi(2) + (p.z - city[1]).powi(2)).sqrt();
                        if d < 25_000.0 {
                            found.push((run, r, road.class, p.x, p.z, s, x));
                        }
                    }
                    run = 0;
                }
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    for f in found.iter().take(12) {
        println!(
            "{} m over on road {} {:?} from ({:.0}, {:.0}) at {:.0} m along",
            f.0 * 10,
            f.1,
            f.2,
            f.3,
            f.4,
            f.5
        );
    }
}

/// What the game's startup bake (every mass within 128 km of the origin,
/// `strata_sim::PrepareWater`) takes with the roads, on 16 threads.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_startup() {
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let started = Instant::now();
    let masses = world.prepare_water(0.0, 0.0, 128_000.0);
    let networks = world.hydrology().networks();
    println!(
        "startup: {masses} masses in {:.2} s; roads {:.2} s of their bakes, sites {:.2} s, {} roads, {:.0} km",
        started.elapsed().as_secs_f64(),
        networks
            .iter()
            .map(|n| n.stats().roads_seconds)
            .sum::<f64>(),
        networks
            .iter()
            .map(|n| n.stats().sites_seconds)
            .sum::<f64>(),
        networks.iter().map(|n| n.stats().roads).sum::<usize>(),
        networks.iter().map(|n| n.stats().road_km).sum::<f64>(),
    );
}
