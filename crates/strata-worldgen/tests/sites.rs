//! Procedural sites on seed 42 (ticket 0005, step 4; ADR 0012): placed the
//! same on any thread count, keeping their spacing and counts, never in
//! water or below the sea or on Port Serein's ground, on a pad that is
//! level and meets the ground round it without a step, named uniquely on
//! their mass; and a world without site files is the world before them.

mod common;

use std::collections::HashSet;

use strata_core::Seed;
use strata_worldgen::hydrology::{CANARY_MASS, Hydrology, LATTICE_M};
use strata_worldgen::sites::ProceduralSite;
use strata_worldgen::{ProceduralWorld, WorldSource};

/// Port Serein's ground fingerprint, which the city's layout is fitted to
/// (ADR 0009); no site may move it.
const FINGERPRINT: u64 = 6_857_689_028_393_273_736;

/// Every site of every mass the shared world has baked, with its mass key.
fn all_sites(world: &ProceduralWorld) -> Vec<([i32; 2], ProceduralSite)> {
    world
        .hydrology()
        .networks()
        .iter()
        .flat_map(|network| {
            network
                .sites()
                .iter()
                .map(move |site| (network.key(), site.clone()))
        })
        .collect()
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])).sqrt()
}

/// The canary island's sites, baked by two fresh caches on one thread and
/// on four, are the same to the bit, as is the network's hash.
#[test]
fn sites_are_the_same_on_any_thread_count() {
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
    assert!(!one.sites().is_empty(), "the island holds sites");
    assert_eq!(one.sites(), four.sites());
    assert_eq!(one.content_hash(), four.content_hash());
}

/// Every pair of sites on a mass keeps the larger spacing of its kinds
/// (and their grounds never meet), every site keeps its kind's distance
/// from Port Serein, and no kind holds more than its count for its mass's
/// area.
#[test]
fn spacing_and_counts_hold() {
    let world = common::world(42);
    let kinds = world.site_kinds();
    let kind_of = |site: &ProceduralSite| {
        kinds
            .iter()
            .find(|kind| kind.id == site.kind())
            .expect("a kind of the world")
    };
    let networks = world.hydrology().networks();
    let mut checked = 0;
    for network in &networks {
        let sites = network.sites();
        for (i, a) in sites.iter().enumerate() {
            let ka = &kind_of(a).def;
            for site in world.sites() {
                let d = site.extent_distance_at(a.centre[0], a.centre[1]);
                assert!(
                    d >= ka.spacing.settlements_m,
                    "{} ({}) is {d:.0} m from {}",
                    a.name,
                    a.id,
                    site.id
                );
            }
            for b in &sites[i + 1..] {
                let kb = &kind_of(b).def;
                let keep = if a.kind() == b.kind() {
                    ka.spacing.own_m
                } else {
                    ka.spacing.others_m.max(kb.spacing.others_m)
                }
                .max(a.outer_m() + b.outer_m());
                let d = distance(a.centre, b.centre);
                assert!(
                    d >= keep,
                    "{} and {} are {d:.0} m apart, under {keep:.0}",
                    a.name,
                    b.name
                );
                checked += 1;
            }
        }
        // A mass's area in its nodes.
        #[allow(clippy::cast_precision_loss)]
        let km2 = network.stats().nodes as f64 * LATTICE_M * LATTICE_M / 1e6;
        for kind in kinds {
            let count = sites.iter().filter(|s| s.kind() == kind.id).count();
            #[allow(clippy::cast_precision_loss)]
            let most = (kind.def.per_1000_km2 * km2 / 1_000.0).round();
            #[allow(clippy::cast_precision_loss)]
            let count_f = count as f64;
            assert!(
                count_f <= most,
                "{count} of {} on mass {:?}, over {most}",
                kind.name,
                network.key()
            );
        }
    }
    assert!(checked > 10_000, "only {checked} pairs checked");
}

/// No two sites on a mass share a name, none takes an authored
/// settlement's, and every name is short and set in its kind's patterns.
#[test]
fn names_are_unique_on_a_mass() {
    let world = common::world(42);
    assert!(!world.settlement_names().is_empty());
    for network in world.hydrology().networks() {
        let mut seen: HashSet<String> = world.settlement_names().iter().cloned().collect();
        for site in network.sites() {
            assert!(
                seen.insert(site.name.clone()),
                "{} twice on mass {:?}",
                site.name,
                network.key()
            );
            assert!(
                site.name.len() <= 24 && site.name.chars().next().is_some_and(char::is_uppercase),
                "{:?}",
                site.name
            );
        }
    }
}

/// On the ground as every consumer samples it, within 60 km of the city:
/// no pad or band in standing water or a channel (the band at a quarter,
/// a half, three quarters and nineteen twentieths of its width), no pad
/// below the sea or on Port Serein's ground (the extent and a kilometre
/// past it at least), and the pad developed so nothing grows on it.
#[test]
fn no_pad_in_water_below_the_sea_or_on_port_serein() {
    let world = common::world(42);
    let city = world.sites()[0].centre;
    let mut seen = 0;
    for (_, site) in all_sites(&world) {
        if distance(site.centre, city) > 60_000.0 {
            continue;
        }
        seen += 1;
        let extent = world.sites()[0].extent_distance_at(site.centre[0], site.centre[1]);
        assert!(
            extent > 1_000.0 + site.outer_m(),
            "{} {extent:.0} m from the city",
            site.name
        );
        for (radius, ring) in [
            (0.0, 1),
            (site.radius_m * 0.5, 12),
            (site.radius_m, 24),
            (site.radius_m + site.blend_m * 0.25, 24),
            (site.radius_m + site.blend_m * 0.5, 24),
            (site.radius_m + site.blend_m * 0.75, 32),
            (site.radius_m + site.blend_m * 0.95, 32),
        ] {
            for k in 0..ring {
                // A turn in `ring` steps, by the angle's own sine and cosine:
                // a test, not a sample.
                #[allow(clippy::cast_precision_loss)]
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(ring);
                let (x, z) = (
                    site.centre[0] + radius * a.cos(),
                    site.centre[1] + radius * a.sin(),
                );
                let s = world.sample(x, z);
                assert!(
                    s.standing_water().is_none(),
                    "{} has water at ({x:.0}, {z:.0}): {s:?}",
                    site.name
                );
                if let Some(water) = s
                    .water
                    .filter(|w| w.kind != strata_worldgen::hydrology::WaterKind::Lake)
                {
                    assert!(
                        water.signed_distance_m.abs() > water.bank_m(),
                        "{} has a channel at ({x:.0}, {z:.0}): {water:?}",
                        site.name
                    );
                }
                if radius <= site.radius_m {
                    assert!(s.height > 0.5, "{} below the sea: {s:?}", site.name);
                    assert!(
                        s.landform.developed > 0.99,
                        "{} undeveloped at ({x:.0}, {z:.0})",
                        site.name
                    );
                }
            }
        }
    }
    assert!(seen > 50, "only {seen} sites within 60 km");
}

/// Inside its footprint the ground is the pad's surface, and from the
/// centre out past the band it has no step: walked every 5 cm, no step is
/// more than a tenth of the fall over the two metres round it (a jump of
/// `j` would be over half of that fall's `j`), so the edge and the band's
/// end are continuous. (The sample's own gradient is not the yardstick: the
/// natural ground's is wrong near some coasts, step 2's record.)
#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "a direction, a point, the distance out and the height"
)]
fn the_pad_is_level_and_meets_the_ground() {
    let world = common::world(42);
    let city = world.sites()[0].centre;
    let mut near: Vec<ProceduralSite> = all_sites(&world)
        .into_iter()
        .map(|(_, site)| site)
        .filter(|site| distance(site.centre, city) < 25_000.0)
        .collect();
    near.sort_by_key(|site| site.id);
    assert!(near.len() >= 6, "{} sites near the city", near.len());
    let step = 0.05;
    for site in near.iter().take(30) {
        for k in 0..4 {
            let a = std::f64::consts::FRAC_PI_2 * f64::from(k) + 0.3;
            let (dx, dz) = (a.cos(), a.sin());
            let mut heights = Vec::new();
            let mut d = 0.0;
            while d <= site.outer_m() + 10.0 {
                let (x, z) = (site.centre[0] + dx * d, site.centre[1] + dz * d);
                let s = world.sample(x, z);
                let h = f64::from(s.height);
                if d <= site.radius_m {
                    let pad = site.pad_height_at(x, z);
                    assert!(
                        (h - pad).abs() < 0.02,
                        "{} at {d:.2} m: {h} against its pad's {pad}",
                        site.name
                    );
                    assert!(f64::from(s.landform.slope) <= site.crown + 1e-3, "{s:?}");
                }
                // A road arriving at the pad lays its own banks across the
                // band, whose continuity `tests/grade.rs` walks (ticket
                // 0005, step 6).
                heights.push((h, s.landform.cut_m != 0.0 || s.landform.cleared > 0.0));
                d += step;
            }
            let span = 20;
            let graded: Vec<bool> = heights.iter().map(|h| h.1).collect();
            let heights: Vec<f64> = heights.iter().map(|h| h.0).collect();
            for i in span..heights.len() - span {
                if graded[i - span..=i + span].iter().any(|&g| g) {
                    continue;
                }
                let fall = (heights[i + span] - heights[i - span]).abs() / 2.0;
                let jump = (heights[i + 1] - heights[i]).abs();
                assert!(
                    jump <= 0.1 * fall + 0.002,
                    "{} steps {jump:.4} m at {:.2} m out, over a fall of {fall:.3} a metre",
                    site.name,
                    step * f64::from(u32::try_from(i).expect("a short walk"))
                );
            }
        }
    }
}

/// A world with no site kinds places nothing, and away from the sites the
/// ground of the shipped world, its roads left out, is that world's to the
/// bit: sites change their pads and nothing else. Port Serein's fingerprint
/// holds.
#[test]
fn a_world_without_sites_is_the_world_before_them() {
    let shipped = common::world(42);
    assert_eq!(shipped.sites()[0].fingerprint(), FINGERPRINT);
    let mut registries = common::registries();
    registries.sites = strata_data::SiteKindStore::default();
    let bare = ProceduralWorld::new(Seed(42), &registries);
    let city = shipped.sites()[0].centre;
    bare.hydrology().set_threads(common::THREADS);
    bare.prepare_water(city[0], city[1], 8_000.0);
    assert!(bare.site_kinds().is_empty());
    assert!(
        bare.hydrology()
            .networks()
            .iter()
            .all(|network| network.sites().is_empty())
    );
    assert_eq!(bare.sites()[0].fingerprint(), FINGERPRINT);
    let sites: Vec<ProceduralSite> = all_sites(&shipped).into_iter().map(|(_, s)| s).collect();
    let (mut same, mut padded) = (0, 0);
    let step = 97.0;
    let mut z = city[1] - 6_000.0;
    while z <= city[1] + 6_000.0 {
        let mut x = city[0] - 6_000.0;
        while x <= city[0] + 6_000.0 {
            let on_site = sites
                .iter()
                .any(|site| distance(site.centre, [x, z]) < site.outer_m() + 1.0);
            // The roads run between the sites (ticket 0005, step 6): the
            // ground without them is the sites' alone.
            let (a, b) = (shipped.sample_without_roads(x, z), bare.sample(x, z));
            if on_site {
                padded += 1;
            } else {
                assert_eq!(a, b, "({x}, {z}) moved off every site");
                same += 1;
            }
            x += step;
        }
        z += step;
    }
    assert!(same > 10_000, "{same} points compared");
    // The windows' sites: a point or two of the grid lands on a pad.
    let _ = padded;
}
