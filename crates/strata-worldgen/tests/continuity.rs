//! The world is continuous: no seams between noise cells, no steps at
//! biome borders, no cliffs the relief did not ask for.

mod common;

use strata_core::Rng;
use strata_worldgen::WorldSource;

/// Steepest rise per metre travelled the shipped relief may produce
/// anywhere. A seam or a hard biome border shows up as tens of metres per
/// metre; the mountains' own ridge flanks reach about five at their worst.
const MAX_RISE_PER_M: f32 = 8.0;

#[test]
fn adjacent_metres_never_step() {
    let world = common::world(42);
    let mut rng = Rng::new(1);
    let mut worst = (0.0f32, 0.0, 0.0);
    for _ in 0..4_000 {
        let x = rng.range_f64(-60_000.0, 60_000.0);
        let z = rng.range_f64(-60_000.0, 60_000.0);
        let here = world.sample(x, z).height;
        for (dx, dz) in [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let there = world.sample(x + dx, z + dz).height;
            // Rise per metre travelled, so the diagonal is not penalised
            // for being longer.
            #[allow(clippy::cast_possible_truncation)]
            let rise = (there - here).abs() / (dx * dx + dz * dz).sqrt() as f32;
            if rise > worst.0 {
                worst = (rise, x, z);
            }
        }
    }
    let (rise, x, z) = worst;
    println!("steepest one-metre rise: {rise} m at ({x:.0}, {z:.0})");
    assert!(
        rise < MAX_RISE_PER_M,
        "a one-metre step rose {rise} m at ({x:.0}, {z:.0}): {:?}",
        world.sample(x, z)
    );
}

#[test]
fn a_walk_across_a_mountain_range_is_smooth() {
    let world = common::world(42);
    // Through the range the seed-42 map shows near (20 km, -5 km).
    let mut previous = world.sample(14_000.0, -11_000.0).height;
    let mut worst = (0.0f32, 0.0);
    for k in 1..=12_000 {
        let t = f64::from(k);
        let h = world.sample(14_000.0 + t, -11_000.0 + t).height;
        let rise = (h - previous).abs() / std::f32::consts::SQRT_2;
        if rise > worst.0 {
            worst = (rise, t);
        }
        previous = h;
    }
    let (rise, t) = worst;
    assert!(
        rise < MAX_RISE_PER_M,
        "a diagonal step rose {rise} m per metre at t = {t}: {:?}",
        world.sample(14_000.0 + t, -11_000.0 + t)
    );
}

#[test]
fn biome_weights_change_gradually() {
    let world = common::world(42);
    let mut worst = 0.0f32;
    let mut previous = world.sample(-50_000.0, 3_000.0).biomes;
    for k in 1..=10_000 {
        let x = -50_000.0 + f64::from(k) * 10.0;
        let biomes = world.sample(x, 3_000.0).biomes;
        // The dominant biome's weight may only move a little per 10 m, and
        // when the winner changes hands the two weights are near even.
        if biomes[0].0 == previous[0].0 {
            worst = worst.max((biomes[0].1 - previous[0].1).abs());
        } else {
            assert!(
                biomes[0].1 < 0.6 || previous[0].1 < 0.6,
                "the winner changed with a hard edge at x = {x}: {previous:?} -> {biomes:?}"
            );
        }
        previous = biomes;
    }
    assert!(worst < 0.25, "a weight jumped by {worst} over 10 m");
}

use common::{one_sweep_at_a_time, pool};

/// Land points scattered over the 100 km square round the origin, from a
/// hash of their index so any count is the start of a larger one, with
/// each point's sample.
fn scattered_land(
    world: &strata_worldgen::ProceduralWorld,
    count: u64,
) -> Vec<(f64, f64, strata_worldgen::Sample)> {
    use rayon::prelude::*;
    let unit = |k: u64| {
        // Top 53 bits of a mix, as a fraction in 0..1.
        #[allow(clippy::cast_precision_loss)]
        let fraction = (strata_core::mix64(k) >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    };
    let pool = pool();
    pool.install(|| {
        (0..count)
            .into_par_iter()
            .filter_map(|i| {
                let x = (unit(2 * i) * 2.0 - 1.0) * 50_000.0;
                let z = (unit(2 * i + 1) * 2.0 - 1.0) * 50_000.0;
                let sample = world.sample(x, z);
                (sample.height > 2.0).then_some((x, z, sample))
            })
            .collect()
    })
}

/// Lines [`no_centimetre_steps_on_a_ranges_flank`] walks.
const STEP_LINES: u64 = 1_500;

/// A cliff the relief did not ask for, found by walking: the erosion filter
/// cuts its gullies across the slope it is steered by, so a steer that
/// turns between two points a centimetre apart cuts two depths there.
/// Ticket 0004's step 4 steered the gullies partly by a range's own slope,
/// read from the range field's second derivatives, which jump where each
/// simplex corner's reach ends: its review found 55 steps of 0.3 to 14.7 m
/// within a centimetre on 2 000 random lines of 2 km, which the scattered
/// metres of [`adjacent_metres_never_step`] never met (none before step 4).
/// Lines of a kilometre from points on a range's flank (where its own
/// metres stand), at a metre; every metre that rises more than 1.5 m is
/// walked again by the centimetre, and no centimetre may rise 0.3 m, thirty
/// in a metre, over three times the steepest the relief makes.
#[test]
fn no_centimetre_steps_on_a_ranges_flank() {
    use rayon::prelude::*;
    let _sweep = one_sweep_at_a_time();
    let world = common::world(42);
    let unit = |k: u64| {
        // Top 53 bits of a mix, as a fraction in 0..1.
        #[allow(clippy::cast_precision_loss, reason = "53 bits, exact")]
        let fraction = (strata_core::mix64(k) >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    };
    let pool = pool();
    let height = |x: f64, z: f64| f64::from(world.sample(x, z).height);
    let (walked, steps): (usize, Vec<(f64, f64, f64)>) = pool.install(|| {
        let lines: Vec<Vec<(f64, f64, f64)>> = (0..STEP_LINES)
            .into_par_iter()
            .filter_map(|i| {
                let x0 = (unit(3 * i) * 2.0 - 1.0) * 60_000.0;
                let z0 = (unit(3 * i + 1) * 2.0 - 1.0) * 60_000.0;
                if world.continent(x0, z0).range_m <= 1.0 {
                    return None;
                }
                let angle = unit(3 * i + 2) * std::f64::consts::TAU;
                let (dx, dz) = (angle.cos(), angle.sin());
                let mut found = Vec::new();
                let mut previous = height(x0, z0);
                for k in 1..=1_000 {
                    let t = f64::from(k);
                    let here = height(x0 + dx * t, z0 + dz * t);
                    if (here - previous).abs() > 1.5 {
                        let mut before = previous;
                        for j in 1..=100 {
                            let s = t - 1.0 + f64::from(j) * 0.01;
                            let (x, z) = (x0 + dx * s, z0 + dz * s);
                            let now = height(x, z);
                            if (now - before).abs() > 0.3 {
                                found.push((x, z, now - before));
                            }
                            before = now;
                        }
                    }
                    previous = here;
                }
                Some(found)
            })
            .collect();
        (lines.len(), lines.into_iter().flatten().collect())
    });
    println!(
        "{walked} lines of a kilometre on range flanks: {} centimetre steps over 0.3 m",
        steps.len()
    );
    assert!(walked > 150, "only {walked} lines start on a flank");
    assert!(
        steps.is_empty(),
        "{} centimetre steps over 0.3 m on {walked} lines, the first {:?}",
        steps.len(),
        &steps[..steps.len().min(5)]
    );
}

/// The ground's slope in degrees by central differences over 25 m: the
/// scale of a face a player sees, above the grain of the gullies.
fn slope_25m(world: &strata_worldgen::ProceduralWorld, x: f64, z: f64) -> f64 {
    let h = |x, z| f64::from(world.sample(x, z).height);
    let gx = (h(x + 25.0, z) - h(x - 25.0, z)) / 50.0;
    let gz = (h(x, z + 25.0) - h(x, z - 25.0)) / 50.0;
    gx.hypot(gz).atan().to_degrees()
}

fn mountains() -> strata_data::BiomeId {
    common::registries()
        .biome_id(&strata_data::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("ships")
}

/// Where the mountains join the blend their weight ramps over a band as
/// narrow as 30 m on a steep flank, so a mountain relief already standing
/// there, hundreds of metres over the foothills', is switched on across it
/// as a scarp that rings the massif. Ticket 0004's step 2 made one (the
/// band's mean slope 35 degrees at 25 m, 29 % of it over 45) by moving the
/// mountains' window up the range while their relief still began low on
/// it; the relief now waits for the mountains to stand alone.
///
/// **Restated in step 4**, as step 2b's record said it would have to be:
/// the band lies on a range's flank, and since step 4 that flank is a
/// steep smooth slope of the range's own (the continent stage's base and
/// [`Continent::range_m`](strata_worldgen::Continent::range_m)), steepest
/// near the crest, which is the flank the ticket asked for and not a
/// scarp. So the scarp is held where it would be, in the relief: the ground
/// less the continent stage, at 25 m, under the old bounds (a mean under 20
/// degrees, under 3 % over 45; 13.6 degrees and 0.1 % before step 4, 10.7
/// and 1.1 % after, 9.4 and 0.5 % after its review). The whole ground,
/// which the test held until step 4 (14.3 degrees, 0.1 % before it), is
/// 21.3 degrees and 3.7 % after (20.2 and 2.3 % after its review); it is
/// held under 25 degrees and 5 % over 45 so that the flank cannot steepen
/// unnoticed. That is a looser bound on the whole ground than before step
/// 4, recorded in the ticket as the owner's to accept.
#[test]
fn mountains_join_the_blend_without_a_scarp() {
    let _sweep = one_sweep_at_a_time();
    let world = common::world(42);
    let mountains = mountains();
    let relief = |x: f64, z: f64| {
        let t = world.terms(x, z);
        t.height_m - t.continent.base_m - t.continent.range_m
    };
    let band: Vec<(f64, f64)> = scattered_land(&world, 400_000)
        .into_iter()
        .filter_map(|(x, z, s)| {
            let weight = s
                .biomes
                .iter()
                .find(|(id, _)| *id == mountains)
                .map_or(0.0, |(_, w)| *w);
            (weight > 0.005 && weight < 0.35).then(|| {
                let gx = (relief(x + 25.0, z) - relief(x - 25.0, z)) / 50.0;
                let gz = (relief(x, z + 25.0) - relief(x, z - 25.0)) / 50.0;
                (gx.hypot(gz).atan().to_degrees(), slope_25m(&world, x, z))
            })
        })
        .collect();
    assert!(band.len() > 1_000, "only {} band points", band.len());
    // A count of a few thousand; exact in f64.
    #[allow(clippy::cast_precision_loss)]
    let n = band.len() as f64;
    let stats = |pick: fn(&(f64, f64)) -> f64| {
        let mean = band.iter().map(pick).sum::<f64>() / n;
        #[allow(clippy::cast_precision_loss)]
        let steep = band.iter().filter(|b| pick(b) > 45.0).count() as f64 / n;
        (mean, steep)
    };
    let (mean, steep) = stats(|b| b.0);
    let (whole_mean, whole_steep) = stats(|b| b.1);
    println!(
        "mountains' entry band: {} points; the relief's slope {mean:.1} deg mean, {:.1} % over 45; the whole ground's {whole_mean:.1} deg, {:.1} %",
        band.len(),
        steep * 100.0,
        whole_steep * 100.0
    );
    assert!(
        mean < 20.0 && steep < 0.03,
        "the mountains' entry band is a scarp: the relief's mean slope {mean:.1} deg at 25 m, {:.1} % over 45",
        steep * 100.0
    );
    assert!(
        whole_mean < 25.0 && whole_steep < 0.05,
        "the flank under the mountains' entry band steepened: mean {whole_mean:.1} deg at 25 m, {:.1} % over 45",
        whole_steep * 100.0
    );
}

/// The landform's analytic slope is what the material and the flora read,
/// so it must see the steep ground the heights make. A mountain relief
/// grows with the elevation across its rise, and a slope that left that
/// out read a tenth of the mountains' ground (steeper than 35 degrees at
/// 25 m) as flatter than the 0.35 at which the mountains turn from snow to
/// rock, and let trees stand on cliffs. With it, one point in a hundred.
///
/// Where a province's beds shape the ground (ticket 0004, step 5) the
/// slope is held at a metre, not at 25 m: a flank of beds is benches a few
/// metres to tens of metres wide between cliffs, and 25 m averages a bench
/// with the cliff over it, so the bench, which is flat and which the
/// analytic slope rightly calls flat, read as a misread (154 of the 2 249
/// bedded mountain points on step 5's ground, 7 of the 3 762 without beds).
#[test]
fn the_landform_sees_the_mountains_steep_ground() {
    let _sweep = one_sweep_at_a_time();
    let world = common::world(42);
    let mountains = mountains();
    let points: Vec<(f64, f64)> = scattered_land(&world, 60_000)
        .into_iter()
        .filter(|(_, _, s)| s.dominant_biome() == mountains)
        .map(|(x, z, s)| {
            let bedded = s.landform.bed != 0 || s.landform.bed_phase != 0.0;
            let real = if bedded {
                strata_worldgen::measure::slope_deg(&world, x, z)
            } else {
                slope_25m(&world, x, z)
            };
            (real, f64::from(s.landform.slope))
        })
        .collect();
    assert!(
        points.len() > 2_000,
        "only {} mountain points",
        points.len()
    );
    let misread = points
        .iter()
        .filter(|&&(real, analytic)| real > 35.0 && analytic < 0.35)
        .count();
    println!(
        "{misread} of {} mountain points steeper than 35 deg read under 0.35",
        points.len()
    );
    assert!(
        misread * 100 < points.len() * 3,
        "{misread} of {} mountain points steeper than 35 deg at 25 m read as under 0.35",
        points.len()
    );
}

/// The elevation band over which the mountains' relief rises, from the
/// shipped windows the way the sampler works it out: from
/// [`WorldParams::mountain_rise_lead`](strata_worldgen::WorldParams::mountain_rise_lead)
/// under where every other biome is the blend gap behind the mountains,
/// over [`WorldParams::mountain_rise`](strata_worldgen::WorldParams::mountain_rise).
/// Until the review of ticket 0004's step 4 the lead was left out, so the
/// band ran 0.06 over the sampler's (the statistic read 0.75 % on it and
/// 0.74 % on the sampler's).
fn mountains_rise(world: &strata_worldgen::ProceduralWorld) -> (f64, f64) {
    let registries = common::registries();
    let mountains = mountains();
    let gap = world.params().blend_gap;
    let min = registries
        .biomes
        .iter()
        .find(|(id, _, _)| *id == mountains)
        .map(|(_, _, def)| f64::from(def.climate.elevation.min))
        .expect("the mountains are registered");
    let low = registries
        .biomes
        .iter()
        .filter(|(id, _, def)| *id != mountains && def.climate.elevation.max < 1.0)
        .map(|(_, _, def)| (min + f64::from(def.climate.elevation.max) + gap) * 0.5)
        .fold(min, f64::max)
        - world.params().mountain_rise_lead;
    (low, low + world.params().mountain_rise)
}

/// A range meeting the coast has to be gone at the shore, and the land
/// mask used to take it away across its own ramp, about 1.3 km wide; the
/// mountains' relief, which rises over a narrow band of elevation, then
/// switched on across a few hundred metres of that ramp: 1 550 m in 340 m
/// 1.5 km from a beach at (-34 km, 17.6 km), and most of the steepest
/// ground in the world (ticket 0004's review of step 2b). The range now
/// fades over a wider band of the continent field, eased so the fade is
/// steep only below the mountains' band. Over 70 degrees at 25 m, in the
/// band where the relief rises and the land mask is under one: 15.1 % of
/// these points with the old fade, 8.2 % after that review, 0.8 % after
/// step 4 and 0.7 % after its review; about 1.5 % where the field is over
/// 0.3, far inland (before step 4). The coast is still steeper than the inland
/// flanks, where the elevation crosses the band by the range's own bell.
#[test]
fn a_range_meeting_the_coast_is_no_wall() {
    use rayon::prelude::*;
    let _sweep = one_sweep_at_a_time();
    let world = common::world(42);
    let mountains = mountains();
    let (low, high) = mountains_rise(&world);
    let unit = |k: u64| {
        // Top 53 bits of a mix, as a fraction in 0..1.
        #[allow(clippy::cast_precision_loss, reason = "53 bits, exact")]
        let fraction = (strata_core::mix64(k) >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    };
    let pool = pool();
    // The continent stage alone chooses the points, which is cheap; only
    // those in the band are sampled whole.
    let band: Vec<f64> = pool.install(|| {
        (0..400_000_u64)
            .into_par_iter()
            .filter_map(|i| {
                let x = (unit(2 * i) * 2.0 - 1.0) * 50_000.0;
                let z = (unit(2 * i + 1) * 2.0 - 1.0) * 50_000.0;
                let continent = world.continent(x, z);
                // Where the land mask, a smoothstep of the field from 0.02
                // to 0.15, is under one.
                if continent.land <= 0.02 || continent.land >= 0.15 {
                    return None;
                }
                if continent.elevation < low || continent.elevation >= high {
                    return None;
                }
                let sample = world.sample(x, z);
                let alone = sample
                    .biomes
                    .iter()
                    .find(|(id, _)| *id == mountains)
                    .is_some_and(|(_, w)| *w >= 0.999);
                (sample.height > 2.0 && alone).then(|| slope_25m(&world, x, z))
            })
            .collect()
    });
    assert!(
        band.len() > 1_000,
        "only {} coastal band points",
        band.len()
    );
    #[allow(clippy::cast_precision_loss, reason = "a count of thousands")]
    let steep = band.iter().filter(|&&s| s > 70.0).count() as f64 / band.len() as f64;
    println!(
        "the coast under a range: {} points in the mountains' rise, {:.1} % over 70 deg",
        band.len(),
        steep * 100.0
    );
    assert!(
        steep < 0.12,
        "a range meeting the coast is cut off as a wall: {:.1} % of the mountains' rise within the land mask's ramp is over 70 deg at 25 m",
        steep * 100.0
    );
}
