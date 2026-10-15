//! The road network on seed 42 (ticket 0005, step 5; ADR 0012, decision 7):
//! the same on any thread count; every site reached; no road in a lake, the
//! sea or Port Serein's extent; a river crossed only where a crossing is
//! recorded; each class's climb within its grade but where the ground
//! leaves no way; no two roads running side by side; and the nearest-road
//! query agreeing with the lines.

mod common;

use std::collections::HashMap;

use strata_core::Seed;
use strata_worldgen::hydrology::{CANARY_MASS, Hydrology, LATTICE_M, Network, WaterKind};
use strata_worldgen::roads::{self, Road, RoadClass, RoadEnd};
use strata_worldgen::{ProceduralWorld, WorldSource};

/// Port Serein's ground fingerprint, which the city's layout is fitted to
/// (ADR 0009); the roads must not move it.
const FINGERPRINT: u64 = 6_857_689_028_393_273_736;

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])).sqrt()
}

/// The mass under Port Serein, baked with the shared world.
fn home(world: &ProceduralWorld) -> std::sync::Arc<Network> {
    let city = world.sites()[0].centre;
    world
        .hydrology()
        .bake_area(world, (city[0], city[1]), (city[0], city[1]))
        .into_iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass")
}

/// Whether any point of a road lies within `r` of `p`.
fn near(road: &Road, p: [f64; 2], r: f64) -> bool {
    road.points.iter().any(|q| distance([q.x, q.z], p) <= r)
}

/// The canary island's roads, baked by two fresh caches on one thread and
/// on four, are the same to the bit, as is the network's hash.
#[test]
fn roads_are_the_same_on_any_thread_count() {
    let world = ProceduralWorld::new(Seed(42), &common::registries());
    let at = (
        f64::from(CANARY_MASS[0]) * LATTICE_M,
        f64::from(CANARY_MASS[1]) * LATTICE_M,
    );
    let bake = |threads: usize| {
        let cache = Hydrology::new(Seed(42));
        cache.set_threads(threads);
        cache
            .bake_area(&world, at, at)
            .into_iter()
            .find(|network| network.key() == CANARY_MASS)
            .expect("the canary island")
    };
    let (one, four) = (bake(1), bake(4));
    assert!(one.roads().len() > 10, "{} roads", one.roads().len());
    assert_eq!(one.roads(), four.roads());
    assert_eq!(one.junctions(), four.junctions());
    assert_eq!(one.content_hash(), four.content_hash());
}

/// Within 60 km of the city every site has a road ending at it, and the
/// city's own road is the network's first link, its fingerprint as it was.
#[test]
fn every_site_is_reached() {
    let world = common::world(42);
    assert_eq!(world.sites()[0].fingerprint(), FINGERPRINT);
    let network = home(&world);
    let city = world.sites()[0].centre;
    assert_eq!(network.roads()[0].ends[0], RoadEnd::Settlement(0));
    assert_eq!(network.roads()[0].ends[1], RoadEnd::RoadHead(0));
    let mut ended: Vec<_> = network
        .roads()
        .iter()
        .flat_map(|r| r.ends)
        .filter_map(|e| match e {
            RoadEnd::Site(id) => Some(id),
            _ => None,
        })
        .collect();
    ended.sort();
    let mut seen = 0;
    for site in network.sites() {
        if distance(site.centre, city) > 60_000.0 {
            continue;
        }
        seen += 1;
        assert!(
            ended.binary_search(&site.id).is_ok(),
            "{} ({}) at ({:.0}, {:.0}) has no road",
            site.name,
            site.id,
            site.centre[0],
            site.centre[1]
        );
    }
    assert!(seen > 300, "only {seen} sites within 60 km");
}

/// Within 25 km of the city, every point of every road, on the ground as
/// every consumer samples it: over the sea, in no lake, in a river's water
/// only at one of the road's crossings, and outside Port Serein's extent
/// but on the city's own road.
#[test]
fn no_road_in_a_lake_the_sea_or_port_serein() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let mut checked = 0;
    for (r, road) in network.roads().iter().enumerate() {
        if !near(road, city, 25_000.0) {
            continue;
        }
        let own = road.ends.contains(&RoadEnd::Settlement(0));
        for p in &road.points {
            checked += 1;
            let s = world.sample(p.x, p.z);
            assert!(
                s.height > 0.0,
                "road {r} ({:?}) over the sea at ({:.0}, {:.0}): {}",
                road.class,
                p.x,
                p.z,
                s.height
            );
            if let Some(water) = s.standing_water() {
                assert!(
                    water.kind != WaterKind::Lake,
                    "road {r} in a lake at ({:.0}, {:.0})",
                    p.x,
                    p.z
                );
                assert!(
                    road.crossings
                        .iter()
                        .any(|c| distance(c.at, [p.x, p.z]) < 0.5 * c.span_m + 12.0),
                    "road {r} ({:?}) in a river's water at ({:.0}, {:.0}) with no crossing",
                    road.class,
                    p.x,
                    p.z
                );
            }
            if !own {
                let inside = world.sites()[0].extent_distance_at(p.x, p.z);
                assert!(
                    inside > 0.0,
                    "road {r} in Port Serein at ({:.0}, {:.0})",
                    p.x,
                    p.z
                );
            }
        }
    }
    assert!(checked > 2_000, "only {checked} points");
}

/// Within 25 km of the city no road's point stands on the pad or the band
/// of a site it does not end at: the search keeps off them, and the
/// smoothing and the easing onto another road are put back where they
/// came onto one. A pad laid over a road would take its ground with no
/// record of it.
#[test]
fn no_road_on_another_sites_pad() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let sites: Vec<_> = network
        .sites()
        .iter()
        .filter(|s| distance(s.centre, city) < 27_000.0)
        .collect();
    let mut checked = 0;
    for (r, road) in network.roads().iter().enumerate() {
        if !near(road, city, 25_000.0) {
            continue;
        }
        let own: Vec<_> = road
            .ends
            .iter()
            .filter_map(|e| match e {
                RoadEnd::Site(id) => Some(*id),
                _ => None,
            })
            .collect();
        for p in &road.points {
            checked += 1;
            for site in &sites {
                if own.contains(&site.id) {
                    continue;
                }
                assert!(
                    distance(site.centre, [p.x, p.z]) >= site.outer_m(),
                    "road {r} ({:?}) on {}'s ground at ({:.0}, {:.0})",
                    road.class,
                    site.name,
                    p.x,
                    p.z
                );
            }
        }
    }
    assert!(checked > 2_000, "only {checked} points");
}

/// Within 25 km of the city, every place a road's centreline meets a
/// river's channel (each walked every 2 m) is one of the road's recorded
/// crossings.
#[test]
fn a_river_is_crossed_only_at_a_recorded_crossing() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let params = world.hydrology().params();
    // The roads' segments in 64 m buckets.
    let mut buckets: HashMap<[i64; 2], Vec<(usize, usize)>> = HashMap::new();
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a bucket of a world coordinate"
    )]
    let cell = |v: f64| (v / 64.0).floor() as i64;
    for (r, road) in network.roads().iter().enumerate() {
        if !near(road, city, 26_000.0) {
            continue;
        }
        for k in 0..road.points.len() - 1 {
            let (a, b) = (road.points[k], road.points[k + 1]);
            for j in cell(a.z.min(b.z))..=cell(a.z.max(b.z)) {
                for i in cell(a.x.min(b.x))..=cell(a.x.max(b.x)) {
                    buckets.entry([i, j]).or_default().push((r, k));
                }
            }
        }
    }
    let mut met = 0;
    for river in network.rivers() {
        let line = river.centreline(params, 2.0);
        for pair in line.windows(2) {
            let (p, q) = ([pair[0].x, pair[0].z], [pair[1].x, pair[1].z]);
            if distance(p, city) > 25_000.0 {
                continue;
            }
            let mut tested = Vec::new();
            for j in cell(p[1].min(q[1]))..=cell(p[1].max(q[1])) {
                for i in cell(p[0].min(q[0]))..=cell(p[0].max(q[0])) {
                    for &(r, k) in buckets.get(&[i, j]).map_or(&[][..], Vec::as_slice) {
                        if tested.contains(&(r, k)) {
                            continue;
                        }
                        tested.push((r, k));
                        let road = &network.roads()[r];
                        let (a, b) = (road.points[k], road.points[k + 1]);
                        let Some(at) = cross([a.x, a.z], [b.x, b.z], p, q) else {
                            continue;
                        };
                        met += 1;
                        assert!(
                            road.crossings.iter().any(|c| distance(c.at, at) < 10.0),
                            "road {r} ({:?}) meets a river at ({:.1}, {:.1}) with no crossing there",
                            road.class,
                            at[0],
                            at[1]
                        );
                    }
                }
            }
        }
    }
    assert!(met > 10, "only {met} crossings met");
}

/// Where segments `a`-`b` and `p`-`q` cross.
#[allow(
    clippy::many_single_char_names,
    reason = "two segments' ends, as the geometry names them"
)]
fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2], q: [f64; 2]) -> Option<[f64; 2]> {
    let d1 = [b[0] - a[0], b[1] - a[1]];
    let d2 = [q[0] - p[0], q[1] - p[1]];
    let denom = d1[0] * d2[1] - d1[1] * d2[0];
    if denom.abs() < 1e-12 {
        return None;
    }
    let w = [p[0] - a[0], p[1] - a[1]];
    let t = (w[0] * d2[1] - w[1] * d2[0]) / denom;
    let u = (w[0] * d1[1] - w[1] * d1[0]) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then(|| [a[0] + d1[0] * t, a[1] + d1[1] * t])
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

/// Within 60 km of the city, the share of each class's length whose ground
/// climbs over the class's greatest gradient at the scale of a road's
/// profile (100 m, the ground averaged over 40 m at each end): what the
/// grade (step 6) has to cut, fill or wind more. Most of it is tracks and
/// trails to stations and landmarks on summits and gorges' rims, where the
/// ground leaves no gentler way within the search's corridor; the bounds
/// hold the shares the network was built with.
#[test]
#[allow(clippy::cast_precision_loss, reason = "counts of 10 m steps")]
fn each_class_climbs_within_its_grade_but_where_the_ground_has_no_way() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let mut over = [(0.0_f64, 0.0_f64); 3];
    for road in network.roads() {
        if !near(road, city, 60_000.0) {
            continue;
        }
        let limit = roads::spec(road.class).max_grade;
        let length = road.length_m();
        let mean = |s: f64| {
            (-4..=4)
                .map(|k| ground_at(road, (s + 5.0 * f64::from(k)).clamp(0.0, length)))
                .sum::<f64>()
                / 9.0
        };
        let c = road.class as usize;
        let mut s = 0.0;
        while s <= length {
            let (a, b) = ((s - 50.0).max(0.0), (s + 50.0).min(length));
            if b - a > 40.0 {
                over[c].1 += 1.0;
                if (mean(b) - mean(a)).abs() / (b - a) > limit {
                    over[c].0 += 1.0;
                }
            }
            s += 10.0;
        }
    }
    let share = |c: RoadClass| over[c as usize].0 / over[c as usize].1.max(1.0);
    println!(
        "over the grade: service {:.3}, track {:.3}, trail {:.3}",
        share(RoadClass::Service),
        share(RoadClass::Track),
        share(RoadClass::Trail)
    );
    assert!(share(RoadClass::Service) < 0.08);
    assert!(share(RoadClass::Track) < 0.16);
    assert!(share(RoadClass::Trail) < 0.19);
}

/// Within 25 km of the city no road runs within 100 m of another, the two
/// alongside, for over 200 m, away from where they meet: a later road takes
/// an earlier one rather than running beside it.
#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "roads, points and bearings, as the geometry names them"
)]
fn no_two_roads_run_side_by_side() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let roads = network.roads();
    // Every road's points every 20 m or so, with its bearing, in 100 m
    // buckets.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a bucket of a world coordinate"
    )]
    let cell = |v: f64| (v / 100.0).floor() as i64;
    let mut buckets: HashMap<[i64; 2], Vec<(usize, [f64; 2], [f64; 2])>> = HashMap::new();
    let mut samples: Vec<Vec<([f64; 2], [f64; 2], f64)>> = vec![Vec::new(); roads.len()];
    for (r, road) in roads.iter().enumerate() {
        if !near(road, city, 26_000.0) {
            continue;
        }
        let mut next = 0.0;
        for k in 0..road.points.len() - 1 {
            let (a, b) = (road.points[k], road.points[k + 1]);
            let l = b.along_m - a.along_m;
            if l <= 0.0 {
                continue;
            }
            let bearing = [(b.x - a.x) / l, (b.z - a.z) / l];
            while next <= b.along_m {
                let t = (next - a.along_m) / l;
                let p = [a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t];
                samples[r].push((p, bearing, next));
                buckets
                    .entry([cell(p[0]), cell(p[1])])
                    .or_default()
                    .push((r, p, bearing));
                next += 20.0;
            }
        }
    }
    // Where each pair of roads meets: their ends on each other and the
    // places both end at.
    let end_point = |road: &Road, side: usize| {
        let p = if side == 0 {
            road.points[0]
        } else {
            road.points[road.points.len() - 1]
        };
        [p.x, p.z]
    };
    let meets = |a: usize, b: usize| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for (x, y) in [(a, b), (b, a)] {
            for side in 0..2 {
                let p = end_point(&roads[x], side);
                if near(&roads[y], p, 30.0) {
                    out.push(p);
                }
            }
        }
        out
    };
    let mut cache: HashMap<(usize, usize), Vec<[f64; 2]>> = HashMap::new();
    for (r, points) in samples.iter().enumerate() {
        // Per other road, the length of the current run alongside it.
        let mut runs: HashMap<usize, f64> = HashMap::new();
        for &(p, bearing, along) in points {
            if distance(p, city) > 25_000.0 {
                runs.clear();
                continue;
            }
            let mut alongside: Vec<usize> = Vec::new();
            for j in cell(p[1]) - 1..=cell(p[1]) + 1 {
                for i in cell(p[0]) - 1..=cell(p[0]) + 1 {
                    for &(o, q, ob) in buckets.get(&[i, j]).map_or(&[][..], Vec::as_slice) {
                        if o == r || alongside.contains(&o) || distance(p, q) > 100.0 {
                            continue;
                        }
                        let parallel = (bearing[0] * ob[0] + bearing[1] * ob[1]).abs() > 0.8;
                        let meet = cache
                            .entry((r.min(o), r.max(o)))
                            .or_insert_with(|| meets(r, o));
                        let by_a_meeting = meet.iter().any(|&m| distance(m, p) < 250.0);
                        if parallel && !by_a_meeting {
                            alongside.push(o);
                        }
                    }
                }
            }
            runs.retain(|o, _| alongside.contains(o));
            for o in alongside {
                let run = runs.entry(o).or_insert(0.0);
                *run += 20.0;
                assert!(
                    *run <= 200.0,
                    "road {r} ({:?}) runs beside road {o} ({:?}) for {run:.0} m to ({:.0}, {:.0}), {along:.0} m along",
                    roads[r].class,
                    roads[o].class,
                    p[0],
                    p[1]
                );
            }
        }
    }
}

/// The nearest-road query agrees with the lines: at points scattered over
/// 12 km round the city the mass's query finds the road and the distance a
/// search of every segment finds, and the bearing of that segment, and the
/// world's finds the distance a search of every mass's segments finds.
#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, a segment's ends and the share along it"
)]
fn the_query_agrees_with_the_lines() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let all = world.hydrology().bake_area(
        &world,
        (city[0] - 7_000.0, city[1] - 7_000.0),
        (city[0] + 7_000.0, city[1] + 7_000.0),
    );
    let reach = 400.0;
    // Every segment of every mass's roads that comes within reach of the
    // square: `(mass, road, a, b)`.
    let edge = 6_000.0 + reach;
    let mut segments: Vec<([i32; 2], usize, [f64; 2], [f64; 2])> = Vec::new();
    for other in &all {
        for (r, road) in other.roads().iter().enumerate() {
            for pair in road.points.windows(2) {
                let (a, b) = ([pair[0].x, pair[0].z], [pair[1].x, pair[1].z]);
                if a[0].min(b[0]) > city[0] + edge
                    || a[0].max(b[0]) < city[0] - edge
                    || a[1].min(b[1]) > city[1] + edge
                    || a[1].max(b[1]) < city[1] - edge
                {
                    continue;
                }
                segments.push((other.key(), r, a, b));
            }
        }
    }
    let nearest = |p: [f64; 2], mass: Option<[i32; 2]>| {
        let mut best: Option<(f64, usize)> = None;
        for &(key, r, a, b) in &segments {
            if mass.is_some_and(|m| m != key) {
                continue;
            }
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let u = if l2 > 0.0 {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = distance(p, [a[0] + dx * u, a[1] + dz * u]);
            if best.is_none_or(|(held, _)| d < held) {
                best = Some((d, r));
            }
        }
        best.filter(|&(d, _)| d <= reach)
    };
    let mut rng = strata_core::Rng::new(5);
    let mut found = 0;
    for _ in 0..2_000 {
        let p = [
            city[0] + (rng.next_f64() - 0.5) * 12_000.0,
            city[1] + (rng.next_f64() - 0.5) * 12_000.0,
        ];
        let here = network.nearest_road(p[0], p[1], reach);
        match (nearest(p, Some(network.key())), here) {
            (None, None) => {}
            (Some((d, r)), Some(here)) => {
                found += 1;
                assert!((here.distance_m - d).abs() < 1e-6, "{here:?} against {d}");
                assert!(
                    here.road as usize == r || (here.distance_m - d).abs() < 1e-9,
                    "{here:?} against road {r}"
                );
                assert_eq!(here.class, network.roads()[here.road as usize].class);
                let b = here.bearing;
                assert!(((b[0] * b[0] + b[1] * b[1]).sqrt() - 1.0).abs() < 1e-9);
                assert!(
                    distance(here.at, p) - here.distance_m < 1e-6,
                    "{here:?} at {p:?}"
                );
            }
            (a, b) => panic!("at {p:?}: the lines say {a:?}, the query {b:?}"),
        }
        // The world's query over every mass: the nearest of all of them.
        match (nearest(p, None), world.nearest_road(p[0], p[1], reach)) {
            (None, None) => {}
            (Some((d, _)), Some(h)) => {
                assert!(
                    (h.distance_m - d).abs() < 1e-6,
                    "the world's {h:?} against {d}"
                );
            }
            (a, b) => panic!("at {p:?}: every mass's lines say {a:?}, the world {b:?}"),
        }
    }
    assert!(found > 200, "only {found} points near a road");
}
