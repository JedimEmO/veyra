//! Rain falls on the windward side of a range and not on its lee.
//!
//! The point of ADR 0006's march: a range has a wet side and a dry side,
//! and which side is which is the prevailing wind's business.

mod common;

use strata_worldgen::WorldSource;

/// The highest point within `radius` of the origin, on a coarse sweep,
/// whose windward and lee fans ([`fan`]) lie wholly on land and whose
/// windward fan lies on low ground (a mean under [`WINDWARD_LOW_M`]).
///
/// Wholly on land because a lee over the sea is not a rain shadow: the
/// march carries full moisture over water, so a coastal peak with the sea
/// behind it is "wetter downwind" by the sea's own number and says nothing
/// of the range. With the ranges 70 km apart the highest peak near the
/// origin was inland; since ticket 0004's step 2 brought them to 35 km it is
/// on the south coast at (-1500, -43500), with nineteen of its twenty lee
/// points at sea, and the question moved to the highest peak it can be
/// asked of: (34000, 37000) on seed 42 after step 2, (24500, -4500) since
/// its review raised the mountains.
///
/// On low ground upwind since step 2b, whose ranges are long lines: the
/// highest inland peak, (9000, -15000), stands on a range running within
/// twenty degrees of the wind, and its windward fan climbs the range's own
/// flank (a mean of 834 m; 838 m since its review) rather than measuring
/// the air arriving at it (windward 0.264, lee 0.272 on step 2b's ground).
/// The peak asked of since is (37000, 39000), whose windward fan is
/// lowland; the test prints the higher peaks it leaves out.
///
/// Returns the peak, and the higher inland peaks the low-ground filter
/// left out with their windward fans' mean ground, so the test can say
/// what it did not ask.
fn highest_inland_peak(
    world: &strata_worldgen::ProceduralWorld,
    radius: f64,
    step: f64,
) -> ((f64, f64), Vec<(f64, f64, f64)>) {
    // Steps are a small count; the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (radius / step) as i64;
    let mut peaks = Vec::new();
    for j in -n..=n {
        for i in -n..=n {
            // Indices are small integers; exact in f64.
            #[allow(clippy::cast_precision_loss)]
            let (x, z) = (i as f64 * step, j as f64 * step);
            peaks.push((world.sample(x, z).height, x, z));
        }
    }
    peaks.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut left_out = Vec::new();
    for (_, x, z) in peaks {
        let (wx, wz) = world.wind_at(x, z);
        let on_land = fan(x, z, -wx, -wz)
            .chain(fan(x, z, wx, wz))
            .all(|(px, pz)| world.sample(px, pz).height > world.sea_level());
        if !on_land {
            continue;
        }
        let upwind = fan(x, z, -wx, -wz)
            .map(|(px, pz)| f64::from(world.sample(px, pz).height))
            .sum::<f64>()
            / 20.0;
        if upwind < WINDWARD_LOW_M {
            return ((x, z), left_out);
        }
        left_out.push((x, z, upwind));
    }
    panic!("seed 42 has no inland peak with low ground upwind")
}

/// The windward fan's mean ground must be under this, in metres, for the
/// peak to be asked of: lowland and foothills, not the range itself.
const WINDWARD_LOW_M: f64 = 500.0;

/// 20 points 5 to 15 km along `(dx, dz)` from `(x, z)`, fanned a little to
/// either side so one dry valley cannot carry the answer.
fn fan(x: f64, z: f64, dx: f64, dz: f64) -> impl Iterator<Item = (f64, f64)> {
    (0..20).map(move |k| {
        let t = 5_000.0 + (f64::from(k) / 19.0) * 10_000.0;
        // A small cross-wind fan: the perpendicular is (-dz, dx).
        let side = (f64::from(k % 5) - 2.0) * 800.0;
        (x + dx * t - dz * side, z + dz * t + dx * side)
    })
}

/// Mean moisture of the [`fan`] along `(dx, dz)` from `(x, z)`.
fn mean_moisture(
    world: &strata_worldgen::ProceduralWorld,
    x: f64,
    z: f64,
    dx: f64,
    dz: f64,
) -> f64 {
    fan(x, z, dx, dz)
        .map(|(px, pz)| f64::from(world.sample(px, pz).moisture))
        .sum::<f64>()
        / 20.0
}

#[test]
fn a_range_is_wet_upwind_and_dry_downwind() {
    let world = common::world(42);
    let ((px, pz), left_out) = highest_inland_peak(&world, 50_000.0, 500.0);
    for (x, z, upwind) in &left_out {
        println!(
            "left out: the inland peak at ({x:.0}, {z:.0}), higher, whose windward fan \
             climbs a range (mean ground {upwind:.0} m)"
        );
    }
    let (wx, wz) = world.wind_at(px, pz);
    let windward = mean_moisture(&world, px, pz, -wx, -wz);
    let lee = mean_moisture(&world, px, pz, wx, wz);
    println!(
        "seed 42: peak at ({px:.0}, {pz:.0}), wind ({wx:.3}, {wz:.3}), \
         windward mean moisture {windward:.3}, lee {lee:.3}"
    );
    assert!(
        windward > lee,
        "the lee of the range at ({px:.0}, {pz:.0}) is not drier: \
         windward {windward:.3}, lee {lee:.3}"
    );
}

/// One peak says little: the march reads a coarse range field, which sits
/// off the real one, and a lee can be wet for a reason of its own. So most
/// of them. Every summit of 800 m or more within 50 km (higher than its
/// eight neighbours a kilometre off) whose two fans are at least three
/// quarters land, their land points compared: the lee was drier at 16 of
/// 27 after ticket 0004's review of step 2, and is at 12 of 21 since its
/// step 2b, whose ranges run nearer the wind's direction. The shadow is
/// real but not universal (the ticket's rain section), so the bound is not
/// a rule; it is 55 % since the review of step 2b (a bare majority until
/// then, which 12 of 21 met by one summit), so that the next change that
/// weakens the shadow shows.
#[test]
fn most_ranges_are_drier_in_their_lee() {
    let world = common::world(42);
    let height = |x: f64, z: f64| world.sample(x, z).height;
    let sea = world.sea_level();
    let step = 1_000.0;
    let (mut drier, mut asked) = (0_u32, 0_u32);
    for j in -50..=50 {
        for i in -50..=50 {
            let (x, z) = (f64::from(i) * step, f64::from(j) * step);
            let h = height(x, z);
            let summit = h >= 800.0
                && (-1..=1)
                    .flat_map(|dj| (-1..=1).map(move |di| (di, dj)))
                    .filter(|&(di, dj)| (di, dj) != (0, 0))
                    .all(|(di, dj)| height(x + f64::from(di) * step, z + f64::from(dj) * step) < h);
            if !summit {
                continue;
            }
            let (wx, wz) = world.wind_at(x, z);
            let land_mean = |dx: f64, dz: f64| {
                let wet: Vec<f64> = fan(x, z, dx, dz)
                    .map(|(px, pz)| world.sample(px, pz))
                    .filter(|s| s.height > sea)
                    .map(|s| f64::from(s.moisture))
                    .collect();
                // At most twenty; exact in f64.
                #[allow(clippy::cast_precision_loss)]
                let n = wet.len() as f64;
                (wet.len() >= 15).then(|| wet.iter().sum::<f64>() / n)
            };
            if let (Some(windward), Some(lee)) = (land_mean(-wx, -wz), land_mean(wx, wz)) {
                asked += 1;
                if lee < windward {
                    drier += 1;
                }
            }
        }
    }
    println!("seed 42: the lee is drier at {drier} of {asked} summits");
    assert!(asked >= 8, "only {asked} summits to ask");
    assert!(
        drier * 20 >= asked * 11,
        "the lee is drier at only {drier} of {asked} summits, under 55 %"
    );
}

#[test]
fn cover_is_patchy_inside_one_forest() {
    let registries = common::registries();
    let world = common::world(42);
    let forest = registries
        .biome_id(&strata_data::Namespaced::parse("strata:forest").expect("valid"))
        .expect("ships");
    // Any forest point will do; the first one a coarse sweep finds keeps the
    // test from depending on a hand-picked coordinate surviving a retune.
    let centre = (-250..=250)
        .flat_map(|j| (-250..=250).map(move |i| (i, j)))
        .map(|(i, j)| (f64::from(i) * 200.0, f64::from(j) * 200.0))
        .find(|&(x, z)| world.sample(x, z).dominant_biome() == forest)
        .expect("seed 42 has forest within 50 km");

    // A 64 m grid: fine enough to land several samples inside one grove or
    // clearing, wide enough to cross a few of them.
    let mut values = Vec::new();
    for j in 0..32 {
        for i in 0..32 {
            let (x, z) = (
                centre.0 + f64::from(i) * 64.0,
                centre.1 + f64::from(j) * 64.0,
            );
            values.push(f64::from(world.sample(x, z).cover));
        }
    }
    // Sample count is tiny; the cast is exact.
    #[allow(clippy::cast_precision_loss)]
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
    let deviation = variance.sqrt();
    println!(
        "seed 42: forest at ({:.0}, {:.0}), cover mean {mean:.3}, sd {deviation:.3}",
        centre.0, centre.1
    );
    assert!(
        deviation > 0.1,
        "cover is flat inside the forest at ({:.0}, {:.0}): mean {mean:.3}, sd {deviation:.3}",
        centre.0,
        centre.1
    );
}

/// The value [`the_rain_stays_where_step_2b_put_it`] holds: seed 42's
/// moisture as the world before ticket 0004's step 4 had it (recorded on
/// that world, HEAD e2de983, by the review of step 4).
const PINNED_MOISTURE: u64 = 0xb2ca_6646_9de4_8f0a;

/// The rain did not move when step 4 read a range's lift from the distance
/// to its crest line: the march keeps the fold of the range field's value
/// (`WorldParams::rain_range_foot`), which the rain was tuned on, and the
/// moisture is the bits it was before that step, over a 64 by 64 lattice at
/// 3.1 km. Until step 4's review this was seen in a map that came out the
/// same, and asserted by nothing. A change that moves the climate on
/// purpose pastes the value the failure prints.
#[test]
fn the_rain_stays_where_step_2b_put_it() {
    let world = common::world(42);
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for j in 0..64 {
        for i in 0..64 {
            let x = -98_000.0 + f64::from(i) * 3_100.0;
            let z = -98_000.0 + f64::from(j) * 3_100.0;
            for b in world.sample(x, z).moisture.to_bits().to_le_bytes() {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    assert_eq!(
        hash, PINNED_MOISTURE,
        "seed 42's moisture hashes to {hash:#018x}, pinned {PINNED_MOISTURE:#018x}"
    );
}
