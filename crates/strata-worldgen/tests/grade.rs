//! The road's grade on seed 42 (ticket 0005, step 6; ADR 0012, decision 7):
//! each profile within its class's gradient but where the ground leaves no
//! way, held to its pads, its junctions and its bridges' decks; the ground
//! continuous with its gradient across a road, along it, at a junction, at
//! a pad's edge, at a bridge's abutments and at a ford; the ground far from
//! any road and Port Serein's own as they were. Run in release:
//!
//! ```text
//! cargo test --release -p strata-worldgen --test grade -- --test-threads=4
//! ```

#![allow(
    clippy::many_single_char_names,
    reason = "points, directions and shares along a road, as the checks walk them"
)]

mod common;

use strata_worldgen::hydrology::Network;
use strata_worldgen::roads::{self, CrossingKind, Road, RoadEnd};
use strata_worldgen::{ProceduralWorld, Sample, WorldSource};

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

/// A road's centreline at `along`, and its direction there.
fn on(road: &Road, along: f64) -> ([f64; 2], [f64; 2]) {
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
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let l = (dx * dx + dz * dz).sqrt().max(1e-9);
    ([a.x + dx * t, a.z + dz * t], [dx / l, dz / l])
}

/// The ground's gradient a sample carries, `(d/dx, d/dz)`.
fn gradient(s: &Sample) -> [f64; 2] {
    let slope = f64::from(s.landform.slope);
    [
        -slope * f64::from(s.landform.aspect[0]),
        -slope * f64::from(s.landform.aspect[1]),
    ]
}

const STEP_M: f64 = 0.05;

/// How a line through the ground went.
#[derive(Debug, Default)]
struct Walked {
    /// The largest step, metres, and the natural ground's.
    step: f64,
    natural_step: f64,
    /// The largest second difference, and the natural ground's.
    bend: f64,
    natural_bend: f64,
    /// Points where the carried gradient along the line is off the
    /// measured one by more than the natural ground's own error, 0.05 and a
    /// tenth of it.
    off: usize,
    points: usize,
}

/// Walks `from` to `to` every [`STEP_M`] on the graded ground and on the
/// ground without the roads.
fn walk(world: &ProceduralWorld, from: [f64; 2], to: [f64; 2]) -> Walked {
    let length = distance(from, to);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test line's step count"
    )]
    let n = (length / STEP_M).ceil() as usize;
    let dir = [(to[0] - from[0]) / length, (to[1] - from[1]) / length];
    let mut graded = Vec::with_capacity(n + 1);
    let mut natural = Vec::with_capacity(n + 1);
    for k in 0..=n {
        #[allow(clippy::cast_precision_loss, reason = "a test line's step count")]
        let f = k as f64 / n as f64;
        let (x, z) = (
            from[0] + (to[0] - from[0]) * f,
            from[1] + (to[1] - from[1]) * f,
        );
        graded.push(world.sample(x, z));
        natural.push(world.sample_without_roads(x, z));
    }
    #[allow(clippy::cast_precision_loss, reason = "a test line's step count")]
    let spacing = length / n as f64;
    let rough = |s: &[Sample]| {
        let h: Vec<f64> = s.iter().map(|s| f64::from(s.height)).collect();
        let step = h
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);
        let bend = h
            .windows(3)
            .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
            .fold(0.0, f64::max);
        let error: Vec<f64> = s
            .windows(3)
            .map(|w| {
                let measured = (f64::from(w[2].height) - f64::from(w[0].height)) / (2.0 * spacing);
                let g = gradient(&w[1]);
                measured - (g[0] * dir[0] + g[1] * dir[1])
            })
            .collect();
        (step, bend, error)
    };
    let (step, bend, error) = rough(&graded);
    let (natural_step, natural_bend, natural_error) = rough(&natural);
    // The natural sample's carried gradient leaves out the continent's base
    // slope (`Landform::slope`), so its error is the measure: a graded
    // point is off when it is further off than the natural ground's own.
    let off = error
        .iter()
        .zip(&natural_error)
        .filter(|(e, n)| e.abs() > n.abs() + 0.05 + 0.1 * n.abs())
        .count();

    Walked {
        step,
        natural_step,
        bend,
        natural_bend,
        off,
        points: n,
    }
}

/// No step past the steepest bank, no crease past the natural ground's own,
/// and the carried gradient as true as the natural ground's.
fn assert_smooth(world: &ProceduralWorld, what: &str, from: [f64; 2], to: [f64; 2]) {
    assert_smooth_to(world, what, from, to, 2.4);
}

/// [`assert_smooth`] with a bank as steep as `steepest` rise over run.
fn assert_smooth_to(
    world: &ProceduralWorld,
    what: &str,
    from: [f64; 2],
    to: [f64; 2],
    steepest: f64,
) {
    let w = walk(world, from, to);
    // A turn of the slope by a fifth within one step is a crease on a bank
    // (whose feet and shoulders turn by their slope over a metre or more);
    // an abutment's face, six times as steep, turns as much faster.
    let turn = 0.2 * steepest / 2.4;
    // The steepest a bank stands is the rock's 1.8 steepened a quarter by
    // its wander; the natural ground may stand steeper.
    let steepest = (steepest * STEP_M).max(w.natural_step * 1.2) + 1e-4;
    assert!(
        w.step < steepest,
        "{what}: a step of {:.4} m in {STEP_M} m (allowed {steepest:.4}) from {from:?} to {to:?}",
        w.step
    );
    let creased = 2.0 * w.natural_bend + turn * STEP_M + 1e-4;
    assert!(
        w.bend < creased,
        "{what}: a crease of {:.5} m (allowed {creased:.5}) from {from:?} to {to:?}",
        w.bend
    );
    assert!(
        w.off <= w.points / 100,
        "{what}: the carried gradient is further off than the natural ground's at {} of {} points from {from:?} to {to:?}",
        w.off,
        w.points
    );
}

/// Within 25 km of Port Serein a profile climbs within its class's grade
/// over all but a few per cent of its length (where holding it would cut or
/// fill past the class's most, or a pin forces it), meets every pad at the
/// pad's level, every road it ends on at that road's level, and stands
/// level at a bridge's two abutments.
#[test]
fn profiles_hold_their_grade_and_their_pins() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let roads = network.roads();
    let mut length = [0.0_f64; 2];
    let mut over = [0.0_f64; 2];
    let (mut pads, mut junctions, mut decks) = (0, 0, 0);
    for road in roads {
        if !road.graded() || distance([road.points[0].x, road.points[0].z], city) > 25_000.0 {
            continue;
        }
        let class = usize::from(road.class != roads::RoadClass::Service);
        let grade = roads::spec(road.class).max_grade;
        let mut s = 0.0;
        while s < road.length_m() {
            let (_, rise) = road.level_at(s).expect("a profile");
            length[class] += 1.0;
            if rise.abs() > grade + 0.005 {
                over[class] += 1.0;
            }
            s += 1.0;
        }
        for (side, end) in road.ends.iter().enumerate() {
            let p = if side == 0 {
                road.points[0]
            } else {
                road.points[road.points.len() - 1]
            };
            let level = road.level_at(p.along_m).expect("a profile").0;
            match *end {
                RoadEnd::Site(id) => {
                    let site = network
                        .sites()
                        .iter()
                        .find(|site| site.id == id)
                        .expect("its site");
                    let pad = site.pad_height_at(p.x, p.z);
                    assert!(
                        (level - pad).abs() < 0.01,
                        "a road meets {} at {level:.3} against its pad's {pad:.3}",
                        site.name
                    );
                    pads += 1;
                }
                RoadEnd::Junction(j) => {
                    // At the through road's level where it has a profile,
                    // its crown's fall at its verge under it; on its
                    // recorded ground where it has none (Port Serein's own
                    // road, which the settlement lays).
                    let junction = network.junctions()[j as usize];
                    let through = &roads[junction.road as usize];
                    let there = through
                        .level_at(junction.along_m)
                        .map_or_else(|| ground_on(through, junction.along_m), |l| l.0);
                    assert!(
                        level <= there + 0.01 && there - level < 0.2,
                        "an arm meets its road at {level:.3} against {there:.3} at {:?}",
                        junction.at
                    );
                    junctions += 1;
                }
                RoadEnd::Settlement(_) | RoadEnd::RoadHead(_) => {}
            }
        }
        for c in &road.crossings {
            if c.kind != CrossingKind::Bridge {
                continue;
            }
            let [a, b] = c
                .abutments_m
                .map(|s| road.level_at(s).expect("a profile").0);
            assert!(
                (a - c.deck_m).abs() < 0.02 && (b - c.deck_m).abs() < 0.02,
                "a bridge's abutments at {a:.3} and {b:.3} against its deck's {:.3} at {:?}",
                c.deck_m,
                c.at
            );
            decks += 1;
        }
    }
    let share = [over[0] / length[0], over[1] / length[1]];
    assert!(
        share[0] < 0.04 && share[1] < 0.1,
        "over their grade: service {:.1} %, tracks {:.1} %",
        100.0 * share[0],
        100.0 * share[1]
    );
    assert!(
        pads > 20 && junctions > 30 && decks > 3,
        "{pads} pads, {junctions} junctions, {decks} bridges"
    );
}

/// The ground a road's record has under it at `along`, between its points.
fn ground_on(road: &Road, along: f64) -> f64 {
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

/// An arm leaves the road it ends on at that road's surface and only then
/// climbs: along its centreline from the junction out 40 m, every half
/// metre, the ground climbs no steeper over any 2 m than the steeper of the
/// two roads' grades and the through road's crossfall, where the arm's own
/// profile there holds its grade. Pinned at the through road's centreline
/// alone, an arm on a slope met the verge with a lip at a soil bank's
/// slope.
#[test]
fn an_arm_leaves_its_road_at_that_roads_level() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let roads = network.roads();
    let (mut checked, mut worst) = (0, (0.0_f64, [0.0; 2]));
    for road in roads {
        if !road.graded() || distance([road.points[0].x, road.points[0].z], city) > 25_000.0 {
            continue;
        }
        for (side, end) in road.ends.iter().enumerate() {
            let RoadEnd::Junction(j) = *end else { continue };
            let through = &roads[network.junctions()[j as usize].road as usize];
            let along = |s: f64| if side == 0 { s } else { road.length_m() - s };
            let grade = roads::spec(road.class).max_grade;
            // Where the arm's own profile is let past its grade (a cut or
            // fill past its class's most), so is its ground.
            if (0..=40).any(|s| {
                road.level_at(along(f64::from(s)))
                    .is_some_and(|(_, rise)| rise.abs() > grade + 0.005)
            }) {
                continue;
            }
            let limit = grade.max(roads::spec(through.class).max_grade) + 0.06;
            let heights: Vec<(f64, [f64; 2])> = (0..=80)
                .map(|k| {
                    let (p, _) = on(road, along(f64::from(k) * 0.5));
                    (f64::from(world.sample(p[0], p[1]).height), p)
                })
                .collect();
            // Off a site's band: a pad laid over the arm is the pad's.
            if heights
                .iter()
                .any(|(_, p)| world.site_at(p[0], p[1]).is_some())
            {
                continue;
            }
            for w in heights.windows(5) {
                let slope = (w[4].0 - w[0].0).abs() / 2.0;
                if slope > worst.0 {
                    worst = (slope, w[0].1);
                }
                assert!(
                    slope < limit,
                    "an arm of a {:?} climbs {slope:.3} off its road at {:?} (allowed {limit:.3})",
                    road.class,
                    w[0].1
                );
            }
            checked += 1;
        }
    }
    eprintln!("{checked} arms, steepest {:.3} at {:?}", worst.0, worst.1);
    assert!(checked > 30, "{checked} arms");
}

/// A ford's approach rises out of the water at its class's grade, and
/// never by a jump: within 12 m of each wetted edge the profile climbs no
/// steeper (but for the cubic's overshoot between points) than the class's
/// grade or than it climbs further out (where a
/// pin it must reach, or a cut or a fill past the class's most, lets it
/// steepen, as anywhere along a road). The water's floor beside the river
/// once stood the node past each edge half a metre over the surface, a
/// rise of up to a metre in one 4 m step, 25 to 30 % on a track.
#[test]
fn a_fords_approach_holds_its_grade() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let (mut edges, mut over) = (0, 0);
    let mut worst = (0.0_f64, [0.0; 2]);
    for road in network.roads() {
        if !road.graded() || distance([road.points[0].x, road.points[0].z], city) > 60_000.0 {
            continue;
        }
        let grade = roads::spec(road.class).max_grade;
        let steepest = |from: f64, to: f64| {
            let (from, to) = (from.max(0.0), to.min(road.length_m()));
            let mut s = from;
            let mut most = 0.0_f64;
            while s <= to {
                most = most.max(road.level_at(s).expect("a profile").1.abs());
                s += 0.5;
            }
            most
        };
        for c in road
            .crossings
            .iter()
            .filter(|c| c.kind == CrossingKind::Ford)
        {
            let [a, b] = c.abutments_m;
            for (near, far) in [
                (steepest(a - 12.0, a), steepest(a - 40.0, a - 12.0)),
                (steepest(b, b + 12.0), steepest(b + 12.0, b + 40.0)),
            ] {
                edges += 1;
                if near > grade + 0.01 {
                    over += 1;
                }
                // A fifth over, for the cubic between a held run's end and
                // the point before it, which overshoots its chord's slope.
                let allowed = grade.max(far) * 1.2 + 0.01;
                if near - allowed > worst.0 {
                    worst = (near - allowed, c.at);
                }
                assert!(
                    near <= allowed,
                    "a ford's approach rises {near:.3} by its edge against {far:.3} further out, by {:?}",
                    c.at
                );
            }
        }
    }
    eprintln!("{edges} ford edges, {over} steeper than the grade by them");
    assert!(edges > 40, "{edges} ford edges");
}

/// Across every graded road near Port Serein, along it, through its
/// junctions, onto its pads, through its bridges' abutments and its fords,
/// the ground is continuous with its gradient.
#[test]
fn the_ground_is_continuous_across_along_and_at_every_end() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let roads = network.roads();
    let (mut across, mut ends, mut bridges, mut fords) = (0, 0, 0, 0);
    for (r, road) in roads.iter().enumerate() {
        let off = distance([road.points[0].x, road.points[0].z], city);
        if !road.graded() || off > 25_000.0 {
            continue;
        }
        // Bridges and fords out to 25 km, the rest within 12.
        if off > 12_000.0 {
            check_crossings(&world, r, road, &mut bridges, &mut fords);
            continue;
        }
        // Across it at a third and two thirds, and along it for 100 m.
        for f in [1.0 / 3.0, 2.0 / 3.0] {
            let (p, d) = on(road, road.length_m() * f);
            let n = [-d[1], d[0]];
            assert_smooth(
                &world,
                &format!("across road {r}"),
                [p[0] - n[0] * 60.0, p[1] - n[1] * 60.0],
                [p[0] + n[0] * 60.0, p[1] + n[1] * 60.0],
            );
            across += 1;
        }
        let (p, d) = on(road, road.length_m() * 0.5);
        assert_smooth(
            &world,
            &format!("along road {r}"),
            [p[0] - d[0] * 50.0, p[1] - d[1] * 50.0],
            [p[0] + d[0] * 50.0, p[1] + d[1] * 50.0],
        );
        // Through each end: along the road onto its pad or its junction,
        // and across it there.
        for (side, end) in road.ends.iter().enumerate() {
            let s = if side == 0 { 0.0 } else { road.length_m() };
            let (p, d) = on(road, s);
            let out = if side == 0 { -1.0 } else { 1.0 };
            let what = match end {
                RoadEnd::Site(_) => "onto a pad",
                RoadEnd::Junction(_) => "through a junction",
                _ => continue,
            };
            assert_smooth(
                &world,
                &format!("{what} from road {r}"),
                [p[0] - d[0] * 40.0 * out, p[1] - d[1] * 40.0 * out],
                [p[0] + d[0] * 30.0 * out, p[1] + d[1] * 30.0 * out],
            );
            let n = [-d[1], d[0]];
            assert_smooth(
                &world,
                &format!("across {what} from road {r}"),
                [p[0] - n[0] * 40.0, p[1] - n[1] * 40.0],
                [p[0] + n[0] * 40.0, p[1] + n[1] * 40.0],
            );
            ends += 1;
        }
        check_crossings(&world, r, road, &mut bridges, &mut fords);
    }
    assert!(
        across > 20 && ends > 20 && bridges > 0 && fords > 0,
        "{across} roads crossed, {ends} ends, {bridges} bridges, {fords} fords"
    );
}

/// Through each of a road's bridges' abutments and each of its fords.
fn check_crossings(
    world: &ProceduralWorld,
    r: usize,
    road: &Road,
    bridges: &mut usize,
    fords: &mut usize,
) {
    {
        for c in &road.crossings {
            match c.kind {
                CrossingKind::Bridge => {
                    // Through each abutment, from the shelf into the gap.
                    for (k, s) in c.abutments_m.iter().enumerate() {
                        let (p, d) = on(road, *s);
                        let into = if k == 0 { 1.0 } else { -1.0 };
                        // An abutment's face is steep by design.
                        assert_smooth_to(
                            world,
                            &format!("through an abutment of road {r} at {:?}", c.at),
                            [p[0] - d[0] * 20.0 * into, p[1] - d[1] * 20.0 * into],
                            [p[0] + d[0] * 15.0 * into, p[1] + d[1] * 15.0 * into],
                            12.0,
                        );
                    }
                    *bridges += 1;
                }
                CrossingKind::Ford => {
                    let (p, d) = on(road, c.along_m);
                    assert_smooth(
                        world,
                        &format!("through a ford of road {r} at {:?}", c.at),
                        [p[0] - d[0] * 40.0, p[1] - d[1] * 40.0],
                        [p[0] + d[0] * 40.0, p[1] + d[1] * 40.0],
                    );
                    *fords += 1;
                }
                CrossingKind::Culvert => {}
            }
        }
    }
}

/// Under a bridge the river's valley is the carve's to the bit, and on its
/// abutments' shelves the ground stands at the deck.
#[test]
fn a_bridge_leaves_its_gap_to_the_river() {
    let world = common::world(42);
    let network = home(&world);
    let city = world.sites()[0].centre;
    let mut seen = 0;
    for road in network.roads() {
        if !road.graded() || distance([road.points[0].x, road.points[0].z], city) > 25_000.0 {
            continue;
        }
        for c in road
            .crossings
            .iter()
            .filter(|c| c.kind == CrossingKind::Bridge)
        {
            let [a, b] = c.abutments_m;
            if b - a < 12.0 {
                continue;
            }
            // The middle of the gap, past the fill's spill from each side.
            let (p, _) = on(road, c.along_m);
            let (graded, natural) = (
                world.sample(p[0], p[1]),
                world.sample_without_roads(p[0], p[1]),
            );
            if (c.along_m - a).min(b - c.along_m) > 12.0 {
                assert_eq!(
                    graded.height.to_bits(),
                    natural.height.to_bits(),
                    "under the bridge at {:?}",
                    c.at
                );
            }
            for s in [a - 2.0, b + 2.0] {
                let (q, _) = on(road, s);
                let h = f64::from(world.sample(q[0], q[1]).height);
                assert!(
                    (h - c.deck_m).abs() < 0.1,
                    "a shelf at {h:.2} against the deck's {:.2} at {:?}",
                    c.deck_m,
                    c.at
                );
            }
            seen += 1;
        }
    }
    assert!(seen > 0, "no bridge within 25 km");
}

/// Far from any road the ground is what it was without the roads, to the
/// bit; and inside Port Serein's extent the city's own.
#[test]
fn far_from_roads_and_in_port_serein_nothing_moved() {
    let world = common::world(42);
    let city = world.sites()[0].centre;
    let _ = home(&world);
    let mut far = 0;
    let mut k = 0_u64;
    while far < 2_000 {
        k += 1;
        // A scatter over 20 km by a fixed sequence.
        #[allow(clippy::cast_precision_loss, reason = "a test's sequence")]
        let (x, z) = (
            city[0] + ((k * 7_919) % 40_000) as f64 - 20_000.0,
            city[1] + ((k * 104_729) % 40_000) as f64 - 20_000.0,
        );
        if world.nearest_road(x, z, 120.0).is_some() {
            continue;
        }
        far += 1;
        assert_eq!(
            world.sample(x, z),
            world.sample_without_roads(x, z),
            "at ({x}, {z})"
        );
    }
    let site = &world.sites()[0];
    let [x0, z0, x1, z1] = site.owned_bounds();
    let mut inside = 0;
    let mut z = z0;
    while z < z1 {
        let mut x = x0;
        while x < x1 {
            if site.extent_distance_at(x, z) < 0.0 {
                inside += 1;
                assert_eq!(
                    world.sample(x, z).height.to_bits(),
                    world.sample_without_roads(x, z).height.to_bits(),
                    "in Port Serein at ({x}, {z})"
                );
            }
            x += 7.0;
        }
        z += 7.0;
    }
    assert!(inside > 1_000, "{inside} points in the city");
}
