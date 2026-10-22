//! A timed sanity check on the sampler's cost, ignored by default because a
//! loaded CI box makes any wall-clock bound flaky. Run it by hand in release:
//! `cargo test -p strata-worldgen --release --test perf -- --ignored --nocapture`.

use std::time::Instant;

use strata_core::{GRID_N, Seed};
use strata_worldgen::{ProceduralWorld, WorldSource};

fn registries() -> strata_data::WorldDefs {
    strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("the shipped assets load")
}

#[test]
#[ignore = "wall-clock bound; run by hand in release"]
fn a_terrain_grid_samples_in_a_few_milliseconds() {
    let world = ProceduralWorld::new(Seed(42), &registries());
    let n = GRID_N + 2;
    let mut out = Vec::new();
    // The rivers under every strip first (ticket 0005, step 2): a sample
    // that met an unbaked mass would bake it, and that is not the cost of a
    // sample.
    world.hydrology().set_threads(16);
    for (x, z) in [
        (14_500.0, 2_000.0),
        (15_000.0, -17_000.0),
        (-100.0, -5_280.0),
        (-58_661.0, -40_773.0),
        (-69_470.0, 18_003.0),
        (34_301.0, -63_201.0),
        (48_678.0, -63_348.0),
        (-1_813.0, 19_461.0),
        (35_969.0, 36_291.0),
    ] {
        world.prepare_water(x, z, 2_000.0);
    }
    // Warm up once so page faults and the first allocation are not timed.
    world.sample_grid(0.0, 0.0, 1.0, n, &mut out);
    // Plains at (14.5 km, 2 km), flat (about 190 m) and uneroded, with three
    // of the rain march's five taps upwind over land, and the eroded
    // mountains of seed 42 at (15 km, -17 km), inland (the
    // rain march's five taps upwind all over land, where each reads the
    // range noise) and part-way up their rise (normalised elevation 0.87 to
    // 0.94), where the erosion filter runs on every sample and the
    // elevation's gradient is read. Sampled every 4 m, each strip of twenty
    // grids is that biome at full weight. Until ticket 0004's step 2 they
    // were (0, 12 km), which was open sea 414 m deep, and (37.2 km, 9.4 km),
    // which the step's nearer ranges left as forest at 163 m; from the
    // review of step 2 until step 2b they were (0, 25 km), which step 2b's
    // climate made tundra (whose relief is eroded, so the filter runs there
    // too: 1.53 to 1.54 us a sample), and (34.5 km, 2.5 km), which its
    // ranges left as plains.
    for (place, z0, x0) in [
        ("lowland", 2_000.0, 14_500.0),
        ("mountains", -17_000.0, 15_000.0),
    ] {
        let runs = 20;
        let start = Instant::now();
        for k in 0..runs {
            world.sample_grid(x0 + f64::from(k) * 64.0, z0, 1.0, n, &mut out);
        }
        let per_grid = start.elapsed() / runs;
        let per_sample = per_grid / u32::try_from(n * n).expect("a small grid");
        // The figure is what the plan records when a stage is added: 465 ns
        // per sample before Phase 5f's rain march, 852 ns after, on the
        // machine the plan's numbers come from.
        println!("{place} {n}x{n} grid: {per_grid:?} ({per_sample:?} per sample)");
        // The world slice plan's Phase 7 bound, per sample in release: 1.5 us
        // until ticket 0004's decision 5 allowed 1.6, and 1.75 us since the
        // owner's decision of 2026-09-30 (the ticket's "Decided after step
        // 2", decision 1). The old reference at (37.2 km, 9.4 km) was a cheap
        // mountain: near the coast, where most of the rain march's taps are
        // over the sea and skip the range noise, so it read 1.46 us where
        // the same code costs 1.575 us inland. The review of step 2 carried
        // the elevation's gradient through the continent stage on every
        // sample, so the landform sees a mountain relief's rise, and that
        // costs about 0.1 us (1.71 us in inland mountains, pinned to one
        // core, 2026-09-29): the owner took that price, to be paid back
        // where the river work touches the sampler. After ticket 0004's step
        // 2b, pinned to one core on 2026-09-30: lowland 0.96 us, mountains
        // 1.62 us, where the world before it measured 1.08 and 1.70 at its
        // strips in the same session (the range field is two octaves of fbm
        // where it was three of ridged noise); after its review, 0.96 and
        // 1.62 us. The bound holds here and not everywhere in the
        // mountains: of 40 random inland strips where they lead, those at
        // full weight throughout cost at most 1.69 us, but three of the nine
        // that blend into the foothills (their elevation 0.86 to 0.88, where
        // both biomes' relief and the erosion run) cost 1.76 to 1.79 us,
        // and the world before step 2b had four such strips over 1.75, up
        // to 1.81 us (ticket 0004's review of step 2b). The owner's 1.75 is
        // met at full weight, not at the mountains' lower edge. Ticket 0004's
        // step 3 added the province field, allowed 0.06 us: pinned to one
        // core on 2026-09-30, alternating with the binary before it four
        // times each, lowland 0.95 to 0.97 us against 0.91 to 0.93, and
        // mountains 1.58 to 1.60 us against 1.54 (the field alone costs 55
        // to 58 ns a sample on the mesher's grids, 94 ns at random points).
        // Ticket 0004's step 4 (the provinces' relief shapes, and a range's
        // lift and height read from the distance to its crest line, which
        // needs the range field's second derivatives): pinned to one core on
        // 2026-09-30 at a load average of 5, alternating with the binary
        // before it three times each, lowland 1.05 to 1.08 us against 0.98
        // to 1.01, mountains 1.68 to 1.71 us against 1.62 to 1.66. The
        // strips are what they were: the plains at full weight, and the
        // mountains at elevation 0.90, a third of the way up their rise. The
        // rain march reads the fold of the range field and not the distance
        // (`WorldParams::rain_range_foot`): the distance at every tap cost
        // 0.2 us a sample, which the bound could not take. Its review (the
        // distance divided by a size of the gradient read from differences,
        // the three taps sharing their lattice lookups): pinned to one core,
        // load under 1, alternating five times with step 4's binary,
        // mountains 1.707 to 1.718 us against 1.72 to 1.73 (HEAD's 1.58 to
        // 1.60), lowland 1.08 to 1.10 against 1.10 to 1.11; one run in
        // twelve of each binary read about 1.79, over the bound.
        // Ticket 0005's decision 8 (the owner, 2026-10-04): 1.9 us in the
        // mountains from the carve on, which every sample pays one cell's
        // lookup for and a sample near a river its blend.
        assert!(
            per_sample.as_nanos() < 1_900,
            "{place}: a sample took {per_sample:?}"
        );
    }
    // Along a river valley (ticket 0005, step 2): the order-2 river's level
    // upland stretch 5 km south of Port Serein, whose centreline crosses the
    // strip, every sample within its valley's reach and so paying the
    // blend of the segments round it. The strip's own ground costs about
    // 1.95 us before the carve, over the mountains' bound already, so what
    // is held is the carve's own cost there: the strip carved against the
    // same points sampled before the carve, in the same run. 0.30 to 0.39 us
    // as step 2 was built, pinned to one core; 0.31 us after its review's
    // savings (one pass over a bucket's segments, a tighter cut-off, no
    // blend where no valley reaches). The rivers' shape (step 3b, the
    // meanders, their riffles and the stepped walls) took it to 0.435 to
    // 0.438 us, pinned to one core, the wave read once a point and only
    // near the channel; 0.431 to 0.451 over six runs after its review.
    {
        let (x0, z0) = (-100.0, -5_280.0);
        let runs = 20;
        // Without the roads' grade (ticket 0005, step 6), which is held on
        // its own below.
        let start = Instant::now();
        for k in 0..runs {
            for j in 0..n {
                for i in 0..n {
                    #[allow(clippy::cast_precision_loss, reason = "a small grid's index")]
                    std::hint::black_box(
                        world.sample_without_roads(
                            x0 + f64::from(k) * 64.0 + i as f64,
                            z0 + j as f64,
                        ),
                    );
                }
            }
        }
        let carved = start.elapsed() / runs / u32::try_from(n * n).expect("a small grid");
        let start = Instant::now();
        for k in 0..runs {
            for j in 0..n {
                for i in 0..n {
                    #[allow(clippy::cast_precision_loss, reason = "a small grid's index")]
                    std::hint::black_box(
                        world.sample_before_carve(
                            x0 + f64::from(k) * 64.0 + i as f64,
                            z0 + j as f64,
                        ),
                    );
                }
            }
        }
        let before = start.elapsed() / runs / u32::try_from(n * n).expect("a small grid");
        let added = carved.saturating_sub(before);
        println!(
            "river valley: {carved:?} per sample, {before:?} before the carve, the carve {added:?}"
        );
        assert!(
            added.as_nanos() < 450,
            "the carve added {added:?} a sample along a river valley"
        );
    }
    // Each province's steep highlands, where one province stands alone
    // along the whole strip (weight 0.999 or more) and the mean slope is
    // over 0.36: the review of ticket 0004's step 4 found the relief shapes
    // costly there and not at the two strips above, which are granite (whose
    // rule is the billow the biome's own fbm costs). Printed and not held:
    // the owner's 1.75 us is for inland mountains, and these are not
    // (karst's cellular and basalt's damped highlands cost about 2.2 us on
    // the review's machine after its savings, 2.5 and 2.3 before them).
    for (place, x0, z0) in [
        ("badlands highlands", -58_661.0, -40_773.0),
        ("basalt highlands", -69_470.0, 18_003.0),
        ("folded highlands", 34_301.0, -63_201.0),
        ("granite highlands", 48_678.0, -63_348.0),
        ("karst highlands", -1_813.0, 19_461.0),
        ("sandstone highlands", 35_969.0, 36_291.0),
    ] {
        let runs = 20;
        let start = Instant::now();
        for k in 0..runs {
            world.sample_grid(x0 + f64::from(k) * 64.0, z0, 1.0, n, &mut out);
        }
        let per_sample = start.elapsed() / runs / u32::try_from(n * n).expect("a small grid");
        println!("{place} (not held): {per_sample:?} per sample");
    }
}

/// What the roads' grade adds to a sample by a road (ticket 0005, step 6):
/// 65 by 65 grids at a metre centred on the service road that leaves Port
/// Serein's own, every 600 m of it, each sample within its banks' reach,
/// against the same points without the grade. Pinned to one core, 130 to
/// 160 ns as built. Then the decision's absolute bound by a road in the
/// mountains.
#[test]
#[ignore = "wall-clock bound; run by hand in release"]
fn a_road_adds_little_to_a_sample() {
    let world = ProceduralWorld::new(Seed(42), &registries());
    world.hydrology().set_threads(16);
    let city = world.sites()[0].centre;
    let home = world
        .hydrology()
        .bake_area(&world, (city[0], city[1]), (city[0], city[1]))
        .into_iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let road = home
        .roads()
        .iter()
        .find(|road| {
            road.graded()
                && road.ends.iter().any(|end| match end {
                    strata_worldgen::roads::RoadEnd::Junction(j) => {
                        home.junctions()[*j as usize].road == 0
                    }
                    _ => false,
                })
        })
        .expect("the road out of the city");
    let centres: Vec<[f64; 2]> = road
        .points
        .iter()
        .step_by(30)
        .take(20)
        .map(|p| [p.x, p.z])
        .collect();
    let (with, without) = time_grids(&world, &centres);
    let added = with.saturating_sub(without);
    println!("by a road: {with:?} per sample, {without:?} without the grade, the grade {added:?}");
    assert!(
        added.as_nanos() < 250,
        "the grade added {added:?} a sample by a road"
    );
    // Ticket 0005's decision 8 bound (1.9 us in the mountains, the carve
    // on) where a graded road runs through them: the graded road nearest
    // the mountain strip above (15 km, -17 km), 65 by 65 grids every
    // 600 m along it within 6 km of the strip. The road out of the city
    // above runs over the order-2 river's valley, whose ground is over the
    // bound before any road (the river valley's own check), so that one is
    // held relatively.
    let strip = [15_000.0, -17_000.0];
    let networks = world.hydrology().bake_area(
        &world,
        (strip[0] - 6_000.0, strip[1] - 6_000.0),
        (strip[0] + 6_000.0, strip[1] + 6_000.0),
    );
    let near =
        |p: &strata_worldgen::roads::RoadPoint| (p.x - strip[0]).hypot(p.z - strip[1]) < 6_000.0;
    let mut roads: Vec<&strata_worldgen::roads::Road> = networks
        .iter()
        .flat_map(|n| n.roads())
        .filter(|road| road.graded() && road.points.iter().any(near))
        .collect();
    roads.sort_by(|a, b| {
        let d = |r: &strata_worldgen::roads::Road| {
            r.points
                .iter()
                .map(|p| (p.x - strip[0]).hypot(p.z - strip[1]))
                .fold(f64::INFINITY, f64::min)
        };
        d(a).total_cmp(&d(b))
    });
    let road = roads
        .first()
        .expect("a graded road within 6 km of the mountain strip");
    let centres: Vec<[f64; 2]> = road
        .points
        .iter()
        .filter(|p| near(p))
        .step_by(30)
        .take(20)
        .map(|p| [p.x, p.z])
        .collect();
    let (with, without) = time_grids(&world, &centres);
    println!(
        "by a {:?} in the mountains ({} grids from ({:.0}, {:.0})): {with:?} per sample, {without:?} without the grade",
        road.class,
        centres.len(),
        centres[0][0],
        centres[0][1]
    );
    assert!(
        with.as_nanos() < 1_900,
        "a sample by a road in the mountains took {with:?}"
    );
}

/// The mean cost of a sample on 65 by 65 grids at a metre round each of
/// `centres`, with the roads' grade and without it, warmed once.
fn time_grids(
    world: &ProceduralWorld,
    centres: &[[f64; 2]],
) -> (std::time::Duration, std::time::Duration) {
    let n = GRID_N + 2;
    let time = |graded: bool| {
        let start = Instant::now();
        for c in centres {
            for j in 0..n {
                for i in 0..n {
                    #[allow(clippy::cast_precision_loss, reason = "a small grid's index")]
                    let (x, z) = (c[0] - 32.0 + i as f64, c[1] - 32.0 + j as f64);
                    if graded {
                        std::hint::black_box(world.sample(x, z));
                    } else {
                        std::hint::black_box(world.sample_without_roads(x, z));
                    }
                }
            }
        }
        start.elapsed() / u32::try_from(centres.len() * n * n).expect("a few grids")
    };
    time(true);
    (time(true), time(false))
}

/// The bake of Port Serein's mass, its sites, roads and grade included, on
/// 16 threads, and the world's version, which every handshake computes on
/// one thread (its canaries lay their roads): held where ticket 0005's steps
/// 5 and 6 left them, so they grow no further unseen. The ticket's targets
/// were about 3 s and the 223 ms the version took before the roads; what to
/// trade for them is the owner's (the ticket's "Steps 5 and 6, as built").
#[test]
#[ignore = "wall-clock bound; run by hand in release"]
fn the_bake_and_the_version_stay_within_what_the_roads_cost() {
    let world = ProceduralWorld::new(Seed(42), &registries());
    let city = world.sites()[0].centre;
    let cache = strata_worldgen::hydrology::Hydrology::new(Seed(42));
    cache.set_threads(16);
    let start = Instant::now();
    let home = cache
        .bake_area(&world, (city[0], city[1]), (city[0], city[1]))
        .into_iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let bake = start.elapsed();
    let stats = home.stats();
    println!(
        "Port Serein's mass: {bake:?}, of which roads {:.2} s (profiles {:.2} s), {} roads",
        stats.roads_seconds, stats.grade_seconds, stats.roads
    );
    let start = Instant::now();
    let version =
        strata_worldgen::world_version(&strata_worldgen::WorldParams::default(), &registries());
    let handshake = start.elapsed();
    println!("world_version {version:#x}: {handshake:?}");
    assert!(bake.as_secs_f64() < 4.0, "the bake took {bake:?}");
    assert!(
        handshake.as_millis() < 600,
        "the world's version took {handshake:?}"
    );
}
