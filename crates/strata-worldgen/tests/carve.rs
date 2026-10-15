//! The carve (ticket 0005, step 2): the rivers' valleys and channels and the
//! lakes' beds, read from the real network round Port Serein.
//!
//! Continuity is checked on lines sampled every 2 cm: no step (a height
//! difference over a steep wall's rise in one step) and no crease (a second
//! difference over what the carve's own eased profiles bend by, beyond what
//! the natural ground on the same line already does).

mod common;

use std::sync::OnceLock;

use strata_core::Seed;
use strata_worldgen::hydrology::{RiverEnd, RiverParams, WaterKind};
use strata_worldgen::{ProceduralWorld, WorldSource};

fn world() -> &'static ProceduralWorld {
    static WORLD: OnceLock<ProceduralWorld> = OnceLock::new();
    WORLD.get_or_init(|| common::world(42))
}

/// Whether `(x, z)` lies by one of the roads' crossings (ticket 0005, step
/// 6), where a culvert's fill, a ford's dip or a bridge's abutments are the
/// road's ground and not the river's: within half the crossing's span and
/// the reach of a road's banks of where it meets the channel. Anywhere else
/// a road's ground that reached a channel would be a fault, so the river's
/// checks hold there.
fn by_a_crossing(x: f64, z: f64) -> bool {
    static CROSSINGS: OnceLock<Vec<([f64; 2], f64)>> = OnceLock::new();
    let crossings = CROSSINGS.get_or_init(|| {
        world()
            .hydrology()
            .networks()
            .iter()
            .flat_map(|network| network.roads().to_vec())
            .flat_map(|road| road.crossings)
            .map(|c| (c.at, 0.5 * c.span_m.max(c.width_m) + 50.0))
            .collect()
    });
    crossings
        .iter()
        .any(|(at, r)| (at[0] - x).hypot(at[1] - z) < *r)
}

/// Port Serein's centre.
fn city() -> [f64; 2] {
    world().sites()[0].to_world_xz([0.0, 0.0])
}

/// Every reach of the networks with a vertex within `radius` of the city.
fn reaches_near(radius: f64) -> Vec<strata_worldgen::hydrology::River> {
    let [cx, cz] = city();
    world()
        .hydrology()
        .networks()
        .iter()
        .flat_map(|network| network.rivers().to_vec())
        .filter(|river| {
            river
                .vertices
                .iter()
                .any(|v| (v.x - cx).hypot(v.z - cz) < radius)
        })
        .collect()
}

/// Heights along `from` to `to` every `step` metres: the carved ground and
/// the natural one.
fn line(from: [f64; 2], to: [f64; 2], step: f64) -> (Vec<f64>, Vec<f64>) {
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test line's step count"
    )]
    let n = (length / step).ceil() as usize;
    let mut carved = Vec::with_capacity(n + 1);
    let mut natural = Vec::with_capacity(n + 1);
    for k in 0..=n {
        #[allow(clippy::cast_precision_loss, reason = "a test line's step count")]
        let f = k as f64 / n as f64;
        let (x, z) = (
            from[0] + (to[0] - from[0]) * f,
            from[1] + (to[1] - from[1]) * f,
        );
        carved.push(f64::from(world().sample(x, z).height));
        natural.push(world().terms(x, z).height_m);
    }
    (carved, natural)
}

/// The largest step and the largest second difference along a line.
fn roughness(h: &[f64]) -> (f64, f64) {
    let step = h
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    let bend = h
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f64::max);
    (step, bend)
}

const STEP_M: f64 = 0.02;

/// No step and no crease on the line from `from` to `to`.
fn assert_smooth(what: &str, from: [f64; 2], to: [f64; 2]) {
    let (carved, natural) = line(from, to, STEP_M);
    let (step, bend) = roughness(&carved);
    let (_, natural_bend) = roughness(&natural);
    // A wall of rise 6 over run in one step, and f32's rounding.
    assert!(
        step < 6.0 * STEP_M + 1e-4,
        "{what}: a step of {step:.4} m in {STEP_M} m from {from:?} to {to:?}"
    );
    // The bank's ease bends by at most 6 rise / run^2, about one a metre
    // (0.7 m over 2 m), so a crease of 0.1 in the slope stands out at 2 cm;
    // where the
    // natural ground has a crease of its own (a shore's ramp) the carve
    // carries it, scaled by the fill's weight on the height.
    // Along a reach the surface is the network's, straight between
    // vertices; the blend eases each change of its slope on one side of the
    // vertex and leaves half of it there, a bend of up to a few hundredths.
    let allowed = 2.0 * natural_bend + 5.0 * STEP_M * STEP_M + 6e-5;
    assert!(
        bend < allowed,
        "{what}: a crease of {bend:.6} m (allowed {allowed:.6}) from {from:?} to {to:?}"
    );
}

/// A reach's vertex `k` with its unit direction downstream and normal.
fn frame(river: &strata_worldgen::hydrology::River, k: usize) -> ([f64; 2], [f64; 2], [f64; 2]) {
    let v = &river.vertices;
    let (a, b) = if k + 1 < v.len() {
        (v[k], v[k + 1])
    } else {
        (v[k - 1], v[k])
    };
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let length = dx.hypot(dz);
    let dir = [dx / length, dz / length];
    ([v[k].x, v[k].z], dir, [-dir[1], dir[0]])
}

/// The index of a reach's vertex nearest a point.
fn nearest_vertex(river: &strata_worldgen::hydrology::River, x: f64, z: f64) -> usize {
    river
        .vertices
        .iter()
        .enumerate()
        .min_by(|a, b| {
            (a.1.x - x)
                .hypot(a.1.z - z)
                .total_cmp(&(b.1.x - x).hypot(b.1.z - z))
        })
        .map_or(0, |(k, _)| k)
}

/// Whether a reach's vertex is on land well inside it: off the sea, off a
/// lake, a lattice step from either end.
fn inland(river: &strata_worldgen::hydrology::River, k: usize) -> bool {
    let v = &river.vertices[k];
    let first = river.vertices[0];
    let last = river.vertices[river.vertices.len() - 1];
    world().terms(v.x, v.z).height_m > 2.0
        && (v.x - first.x).hypot(v.z - first.z) > 60.0
        && (v.x - last.x).hypot(v.z - last.z) > 60.0
        && world()
            .sample(v.x, v.z)
            .water
            .is_some_and(|w| w.kind != WaterKind::Lake)
}

#[test]
fn the_carve_is_continuous_across_and_along_a_river() {
    let mut checked = 0;
    for river in reaches_near(6_000.0).iter().take(12) {
        for k in (1..river.vertices.len().saturating_sub(2)).step_by(5) {
            if !inland(river, k) || river.vertices[k].fall {
                continue;
            }
            let (at, _, normal) = frame(river, k);
            let half = f64::from(river.vertices[k].width_m) * 0.5 + 12.0;
            assert_smooth(
                "across",
                [at[0] - normal[0] * half, at[1] - normal[1] * half],
                [at[0] + normal[0] * half, at[1] + normal[1] * half],
            );
            checked += 1;
        }
        // Along the channel's centreline and along its bank's ease, where
        // it winds about the line (ticket 0005, step 3b), each point's own
        // way.
        let points = river.centreline(&RiverParams::DEFAULT, 1.0);
        for k in (20..points.len().saturating_sub(20)).step_by(97) {
            let (p, a, b) = (points[k], points[k - 1], points[k + 1]);
            if !inland(river, nearest_vertex(river, p.x, p.z)) {
                continue;
            }
            let length = (b.x - a.x).hypot(b.z - a.z);
            let dir = [(b.x - a.x) / length, (b.z - a.z) / length];
            let normal = [-dir[1], dir[0]];
            for off in [0.0, p.width_m * 0.5] {
                let base = [p.x + normal[0] * off, p.z + normal[1] * off];
                assert_smooth(
                    "along",
                    [base[0] - dir[0] * 3.0, base[1] - dir[1] * 3.0],
                    [base[0] + dir[0] * 3.0, base[1] + dir[1] * 3.0],
                );
            }
        }
    }
    assert!(checked >= 10, "{checked} river points checked");
}

#[test]
fn the_carve_is_continuous_at_a_confluence() {
    let mut checked = 0;
    for river in reaches_near(6_000.0) {
        let RiverEnd::Joins(_) = river.end else {
            continue;
        };
        let j = river.vertices[river.vertices.len() - 1];
        if world().terms(j.x, j.z).height_m < 2.0 {
            continue;
        }
        for (dx, dz) in [(1.0, 0.0), (0.0, 1.0), (0.6, 0.8), (0.8, -0.6)] {
            assert_smooth(
                "confluence",
                [j.x - dx * 20.0, j.z - dz * 20.0],
                [j.x + dx * 20.0, j.z + dz * 20.0],
            );
        }
        checked += 1;
        if checked == 4 {
            break;
        }
    }
    assert!(checked >= 2, "{checked} confluences checked");
}

#[test]
fn the_carve_is_continuous_at_a_mouth() {
    let mut checked = 0;
    for river in reaches_near(8_000.0) {
        if river.end != RiverEnd::Sea {
            continue;
        }
        let k = river.vertices.len() - 1;
        let (at, _, normal) = frame(&river, k);
        // Across the end, out at sea.
        assert_smooth(
            "mouth across",
            [at[0] - normal[0] * 15.0, at[1] - normal[1] * 15.0],
            [at[0] + normal[0] * 15.0, at[1] + normal[1] * 15.0],
        );
        // Along the reach through its shore, where the natural ground crosses
        // sea level: the review of step 2 found the bank's fill standing to
        // a surface metres up there and ending in a wall over the sea.
        let v = &river.vertices;
        let mut shore = None;
        for pair in v.windows(2).rev() {
            let (a, b) = (pair[0], pair[1]);
            if world().terms(a.x, a.z).height_m >= 0.0 && world().terms(b.x, b.z).height_m < 0.0 {
                shore = Some((a, b));
                break;
            }
        }
        let Some((a, b)) = shore else { continue };
        let length = (b.x - a.x).hypot(b.z - a.z);
        let dir = [(b.x - a.x) / length, (b.z - a.z) / length];
        let from = [a.x - dir[0] * 40.0, a.z - dir[1] * 40.0];
        let to = [b.x + dir[0] * 10.0, b.z + dir[1] * 10.0];
        let (carved, natural) = line(from, to, 0.25);
        for (k, pair) in carved.windows(2).enumerate() {
            let natural_step = (natural[k + 1] - natural[k]).abs();
            let step = (pair[1] - pair[0]).abs();
            assert!(
                step < natural_step + 0.25,
                "a mouth steps {step:.3} m in 0.25 m (the natural ground {natural_step:.3}) on the line from {from:?} to {to:?}"
            );
        }
        // It meets the sea: the bed at its end is at or under sea level.
        let end = world().sample(at[0], at[1]);
        assert!(end.height <= 0.0, "a mouth ends {} m up", end.height);
        checked += 1;
        if checked == 4 {
            break;
        }
    }
    assert!(checked >= 2, "{checked} mouths checked");
}

#[test]
fn the_valley_is_continuous_to_its_edge() {
    // Across the whole valley, out past the widest half-width a valley takes
    // (440 m), every 10 cm: its edge, the wall's shoulder and the wall.
    let mut checked = 0;
    for river in reaches_near(6_000.0).iter().take(8) {
        let k = river.vertices.len() / 2;
        if !inland(river, k) || river.vertices[k].fall {
            continue;
        }
        let (at, _, normal) = frame(river, k);
        let (carved, natural) = line(
            [at[0] - normal[0] * 460.0, at[1] - normal[1] * 460.0],
            [at[0] + normal[0] * 460.0, at[1] + normal[1] * 460.0],
            0.1,
        );
        let (step, bend) = roughness(&carved);
        let (natural_step, natural_bend) = roughness(&natural);
        // A slot's wall stands at five in one at the most.
        assert!(
            step < natural_step.max(5.0 * 0.1) + 1e-3,
            "a step of {step:.4} m in 0.1 m across the valley at {at:?}"
        );
        // The tightest ease is the half-metre ramp under a wall of five in
        // one: ten a metre, a tenth of a metre in two 10 cm steps.
        assert!(
            bend < 2.0 * natural_bend + 0.1,
            "a crease of {bend:.4} m across the valley at {at:?}"
        );
        checked += 1;
    }
    assert!(checked >= 4, "{checked} valleys crossed");
}

#[test]
fn the_carve_is_continuous_at_a_lake_shore() {
    // The lake nearest the city, from one of its nodes out past a node of
    // the ground round it: across the bed, the shore and the rim.
    let [cx, cz] = city();
    let networks = world().hydrology().networks();
    let (lake, _) = networks
        .iter()
        .flat_map(|n| n.lakes().iter())
        .map(|lake| {
            let [i, j] = lake.nodes[0];
            let (x, z) = (f64::from(i) * 512.0, f64::from(j) * 512.0);
            (lake, (x - cx).hypot(z - cz))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a lake near Port Serein");
    let node = lake.nodes[0];
    let at = [f64::from(node[0]) * 512.0, f64::from(node[1]) * 512.0];
    let inside = world().sample(at[0], at[1]);
    assert!(inside.in_water(), "a lake node is under water: {inside:?}");
    assert_eq!(inside.water.map(|w| w.kind), Some(WaterKind::Lake));
    let mut crossed = 0;
    for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let out = [at[0] + dx * 700.0, at[1] + dz * 700.0];
        let (carved, _) = line(at, out, 1.0);
        // Only a line that leaves the lake crosses its shore.
        let level = f64::from(inside.water.expect("lake").surface_m);
        let Some(shore) = carved.iter().position(|&h| h >= level) else {
            continue;
        };
        #[allow(clippy::cast_precision_loss, reason = "a line's index")]
        let s = shore as f64;
        let mid = [at[0] + dx * s, at[1] + dz * s];
        assert_smooth(
            "lake shore",
            [mid[0] - dx * 20.0, mid[1] - dz * 20.0],
            [mid[0] + dx * 20.0, mid[1] + dz * 20.0],
        );
        crossed += 1;
    }
    assert!(crossed >= 1, "no line left the lake");
}

#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "a segment's ends, the distance along it and the point"
)]
fn the_water_never_rises_downstream_near_port_serein() {
    // Along each channel's centreline: its surface never rises, and its bed
    // stands under it. The bed itself rises out of each pool onto the
    // riffle below (ticket 0005, step 3b), as a river's does.
    let mut walked = 0.0;
    for river in reaches_near(12_000.0) {
        let mut last: Option<(f64, [f64; 2])> = None;
        let points = river.centreline(&RiverParams::DEFAULT, 4.0);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = (b.x - a.x).hypot(b.z - a.z);
            let mut s = 0.0;
            while s < length {
                let f = s / length;
                let (x, z) = (a.x + (b.x - a.x) * f, a.z + (b.z - a.z) * f);
                s += 2.0;
                // Out at sea the bed is the sea floor's, which rises and
                // falls as it will.
                if world().terms(x, z).height_m < 1.0 {
                    last = None;
                    continue;
                }
                let sample = world().sample(x, z);
                let Some(water) = sample.water.filter(|w| w.kind != WaterKind::Lake) else {
                    last = None;
                    continue;
                };
                // A culvert's fill and a ford's dip are the road's (ticket
                // 0005, step 6), not the river's bed; a road's bank anywhere
                // else is held to the river's rule.
                if sample.landform.cut_m != 0.0 && by_a_crossing(x, z) {
                    continue;
                }
                let surface = f64::from(water.surface_m);
                assert!(
                    sample.height <= water.surface_m + 0.25,
                    "the bed at ({x:.1}, {z:.1}) stands {} over the water",
                    sample.height - water.surface_m
                );
                // Against the lowest surface upstream so far, not the last
                // sample's: a surface creeping up a centimetre a step would
                // pass a test of each step alone. Where a tributary meets
                // its main stem the last of the soft minimum the surfaces
                // take there leaves up to a tenth of a metre.
                if let Some((held, at)) = last {
                    assert!(
                        surface <= held + 0.1,
                        "the surface rises from {held:.3} at {at:?} to {surface:.3} at ({x:.1}, {z:.1})"
                    );
                }
                if last.is_none_or(|(held, _)| surface < held) {
                    last = Some((surface, [x, z]));
                }
                walked += 2.0;
            }
        }
    }
    assert!(walked > 5_000.0, "{walked} m walked");
}

#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, its two samples and the differences' step"
)]
fn the_carved_gradient_is_the_carved_heights() {
    // At points the carve moved, the gradient the sample carries against
    // central differences of its heights, beyond what the natural ground's
    // own carried gradient is off by there. A few points straddle a kink of
    // the surface at a segment's end (half of its change of slope is left
    // there); the review of step 2 found one in thirty off by a tenth.
    let (mut off, mut total) = (0, 0);
    let e = 0.05;
    let slope = |h: &dyn Fn(f64, f64) -> f64, x: f64, z: f64| {
        let gx = (h(x + e, z) - h(x - e, z)) / (2.0 * e);
        let gz = (h(x, z + e) - h(x, z - e)) / (2.0 * e);
        gx.hypot(gz)
    };
    let carved = |x: f64, z: f64| f64::from(world().sample(x, z).height);
    let before = |x: f64, z: f64| f64::from(world().sample_before_carve(x, z).height);
    for river in reaches_near(12_000.0) {
        for k in (0..river.vertices.len()).step_by(4) {
            let (at, _, normal) = frame(&river, k);
            for offset in [-130.0, -45.0, -9.0, 0.0, 3.0, 20.0, 80.0, 190.0] {
                let (x, z) = (at[0] + normal[0] * offset, at[1] + normal[1] * offset);
                let s = world().sample(x, z);
                let b = world().sample_before_carve(x, z);
                if s.height.to_bits() == b.height.to_bits() || world().terms(x, z).height_m < 1.0 {
                    continue;
                }
                let natural_off = (slope(&before, x, z) - f64::from(b.landform.slope)).abs();
                let actual = slope(&carved, x, z);
                total += 1;
                if (actual - f64::from(s.landform.slope)).abs() > 0.05 + 0.1 * actual + natural_off
                {
                    off += 1;
                }
            }
        }
    }
    assert!(total > 300, "{total} carved points");
    eprintln!("{off} of {total} carved points off");
    assert!(off * 100 <= total, "{off} of {total} carved points off");
}

#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, its neighbours and the place beside it"
)]
fn the_bank_stands_over_the_surface() {
    let mut checked = 0;
    for river in reaches_near(6_000.0) {
        // Beside the channel's centreline, where it winds about the line.
        let points = river.centreline(&RiverParams::DEFAULT, 2.0);
        for k in (1..points.len().saturating_sub(1)).step_by(20) {
            let (p, a, b) = (points[k], points[k - 1], points[k + 1]);
            if !inland(&river, nearest_vertex(&river, p.x, p.z)) {
                continue;
            }
            let length = (b.x - a.x).hypot(b.z - a.z);
            let at = [p.x, p.z];
            let normal = [-(b.z - a.z) / length, (b.x - a.x) / length];
            let off = p.width_m * 0.5 + 4.0;
            for side in [-1.0, 1.0] {
                let (x, z) = (
                    at[0] + normal[0] * off * side,
                    at[1] + normal[1] * off * side,
                );
                let s = world().sample(x, z);
                let Some(water) = s.water else { continue };
                // A ford's dip through the bank is the road's (ticket 0005,
                // step 6); a road's bank anywhere else is held to it.
                if water.kind == WaterKind::Lake
                    || world().terms(x, z).height_m < 1.0
                    || (s.landform.cut_m != 0.0 && by_a_crossing(x, z))
                {
                    continue;
                }
                assert!(
                    s.height >= water.surface_m - 1e-3,
                    "the bank at ({x:.1}, {z:.1}) is {} under the surface {}",
                    water.surface_m - s.height,
                    water.surface_m
                );
                assert!(!s.in_water(), "a bank under water at ({x:.1}, {z:.1})");
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "{checked} banks checked");
}

#[test]
fn ground_no_water_reaches_is_the_natural_ground_to_the_bit() {
    let [cx, cz] = city();
    let mut dry = 0;
    for j in 0..60 {
        for i in 0..60 {
            let (x, z) = (
                cx - 15_000.0 + f64::from(i) * 503.0,
                cz - 15_000.0 + f64::from(j) * 497.0,
            );
            // The ground before the roads' grade (ticket 0005, step 6),
            // whose own test holds the ground far from a road to the bit
            // (`tests/grade.rs`): the carve's alone.
            let s = world().sample_without_roads(x, z);
            // A site's pad and the band round it are its ground, not the
            // carve's (ticket 0005, step 4).
            if s.water.is_some() || s.landform.developed > 0.0 || world().site_at(x, z).is_some() {
                continue;
            }
            #[allow(clippy::cast_possible_truncation, reason = "the sample's own cast")]
            let natural = world().terms(x, z).height_m as f32;
            assert_eq!(s.height.to_bits(), natural.to_bits(), "({x}, {z})");
            dry += 1;
        }
    }
    assert!(dry > 1_000, "{dry} dry points");
}

#[test]
fn port_serein_has_no_water_and_its_fingerprint_held() {
    let site = &world().sites()[0];
    // The ground its baked layout (`generated/settlements/port_serein.ron`)
    // was fitted to before the carve: the carve may not move it.
    assert_eq!(site.fingerprint(), 6_857_689_028_393_273_736);
    let [x0, z0, x1, z1] = site.owned_bounds();
    let mut z = z0;
    while z <= z1 {
        let mut x = x0;
        while x <= x1 {
            if site.extent_distance_at(x, z) <= 0.0 {
                let s = world().sample(x, z);
                assert!(s.water.is_none(), "water in the city at ({x}, {z})");
            }
            x += 20.0;
        }
        z += 20.0;
    }
}

#[test]
fn the_startup_bake_covers_the_spawn() {
    // A world of its own seed, so no other test's sampling bakes for it:
    // prepared as the loader prepares the game's spawn, a sample anywhere
    // the prepared square reaches bakes nothing more.
    let world = ProceduralWorld::new(Seed(7), &common::registries());
    world.hydrology().set_threads(common::THREADS);
    let r = 6_000.0;
    world.prepare_water(0.0, 0.0, r);
    let baked = world.hydrology().networks().len();
    for j in -10..=10 {
        for i in -10..=10 {
            let (x, z) = (f64::from(i) * r / 10.0, f64::from(j) * r / 10.0);
            let _ = world.sample(x, z);
        }
    }
    assert_eq!(world.hydrology().networks().len(), baked);
    // And the rain of a tile whose land is all baked is dropped: on seed
    // 42's hundred kilometres some tiles' land is.
    let (held, holding) = self::world().hydrology().tiles_held();
    assert!(
        held > 0 && holding < held,
        "{holding} of {held} tiles keep rain"
    );
}

// The rivers' shape (ticket 0005, step 3b).

/// A polyline's length.
fn length_of(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
        .sum()
}

/// The median of some numbers.
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[test]
fn lowland_rivers_wind_and_steep_ones_run_nearly_straight() {
    // The channel's length over its reach's line: what its meanders add.
    // A lowland river loops; a torrent's meanders shrink to a fifth.
    let (mut lowland, mut steep) = (Vec::new(), Vec::new());
    for river in reaches_near(40_000.0) {
        let line: Vec<[f64; 2]> = river.vertices.iter().map(|v| [v.x, v.z]).collect();
        let along = length_of(&line);
        if along < 400.0 {
            continue;
        }
        let first = river.vertices[0];
        let last = river.vertices[river.vertices.len() - 1];
        let slope = f64::from(first.surface_m - last.surface_m) / along;
        let sinuosity = length_of(&river.channel(4.0)) / along;
        if slope < 0.005 {
            lowland.push(sinuosity);
        } else if slope > 0.03 {
            steep.push(sinuosity);
        }
    }
    assert!(
        lowland.len() >= 5 && steep.len() >= 20,
        "{} lowland, {} steep",
        lowland.len(),
        steep.len()
    );
    let (lowland, steep) = (median(lowland), median(steep));
    eprintln!("sinuosity over the line: lowland {lowland:.3}, steep {steep:.3}");
    assert!(lowland > 1.06, "lowland rivers run straight: {lowland:.3}");
    assert!(steep < 1.04, "steep rivers wind: {steep:.3}");
}

#[test]
#[allow(clippy::cast_possible_truncation, reason = "a grid cell's index")]
fn no_channel_crosses_itself_or_another_near_port_serein() {
    // Every channel's centreline every 2 m, on a grid of 32 m cells: no two
    // of its stretches cross but neighbours, which share a point, and no
    // stretch of another channel (two meet at a junction's shared point).
    let reaches = reaches_near(12_000.0);
    let lines: Vec<Vec<[f64; 2]>> = reaches.iter().map(|r| r.channel(2.0)).collect();
    let mut cells: std::collections::HashMap<(i64, i64), Vec<(usize, usize)>> =
        std::collections::HashMap::new();
    for (r, line) in lines.iter().enumerate() {
        for (k, pair) in line.windows(2).enumerate() {
            let x0 = (pair[0][0].min(pair[1][0]) / 32.0).floor() as i64;
            let x1 = (pair[0][0].max(pair[1][0]) / 32.0).floor() as i64;
            let z0 = (pair[0][1].min(pair[1][1]) / 32.0).floor() as i64;
            let z1 = (pair[0][1].max(pair[1][1]) / 32.0).floor() as i64;
            for i in x0..=x1 {
                for j in z0..=z1 {
                    cells.entry((i, j)).or_default().push((r, k));
                }
            }
        }
    }
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let mut crossings = Vec::new();
    let mut segments = 0;
    for list in cells.values() {
        for (n, &(ra, ka)) in list.iter().enumerate() {
            segments += 1;
            for &(rb, kb) in &list[n + 1..] {
                if ra == rb && ka.abs_diff(kb) <= 1 {
                    continue;
                }
                let (a, b) = (lines[ra][ka], lines[ra][ka + 1]);
                let (c, d) = (lines[rb][kb], lines[rb][kb + 1]);
                let (d1, d2) = (cross(a, b, c), cross(a, b, d));
                let (d3, d4) = (cross(c, d, a), cross(c, d, b));
                if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                    crossings.push(a);
                }
            }
        }
    }
    assert!(segments > 10_000, "{segments} stretches");
    assert!(
        crossings.is_empty(),
        "channels cross at {:?}",
        &crossings[..crossings.len().min(5)]
    );
}

#[test]
#[allow(clippy::cast_precision_loss, reason = "a count of points")]
fn a_riffle_keeps_the_mean_width_and_a_ford_comes_every_400_m() {
    // Along every river under 8 m wide near the city: the channel's width
    // over its length is the hydraulic width's, a riffle shallow enough to
    // ford comes at least every 400 m, and the carve's own record agrees.
    let params = strata_worldgen::hydrology::RiverParams::DEFAULT;
    let (mut reaches, mut sampled) = (0, 0);
    for river in reaches_near(12_000.0) {
        let points = river.centreline(&params, 4.0);
        let Some(last) = points.last() else { continue };
        let widest = points.iter().map(|p| p.width_m).fold(0.0, f64::max);
        if last.along_m < 400.0 || widest > 8.0 * (1.0 + params.riffle_share) {
            continue;
        }
        // Every 4 m along the line, so the mean is over the line's length,
        // as the hydraulic width's is.
        let mean: f64 = points.iter().map(|p| p.width_m).sum::<f64>() / points.len() as f64;
        let hydraulic: f64 = river
            .vertices
            .windows(2)
            .map(|p| {
                let length = (p[1].x - p[0].x).hypot(p[1].z - p[0].z);
                let q = f64::midpoint(f64::from(p[0].discharge_m3s), f64::from(p[1].discharge_m3s));
                params.width(q) * length
            })
            .sum::<f64>()
            / last.along_m;
        assert!(
            (mean / hydraulic - 1.0).abs() < 0.03,
            "a reach's mean width {mean:.2} against its hydraulic {hydraulic:.2}"
        );
        let mut since = 0.0;
        let mut gap = 0.0_f64;
        for pair in points.windows(2) {
            if pair[1].depth_m <= strata_worldgen::hydrology::FORD_DEPTH_M {
                since = pair[1].along_m;
            }
            gap = gap.max(pair[1].along_m - since);
        }
        assert!(
            gap <= 400.0,
            "{gap:.0} m without a ford at ({:.0}, {:.0})",
            points[0].x,
            points[0].z
        );
        // The carve's record at the channel's centre: its width there.
        if sampled < 400 {
            for p in points.iter().step_by(25) {
                // Off a junction, where two reaches' records blend.
                if p.along_m < 80.0 || last.along_m - p.along_m < 80.0 {
                    continue;
                }
                let s = world().sample(p.x, p.z);
                let Some(water) = s.water else { continue };
                if water.kind != WaterKind::Channel || world().terms(p.x, p.z).height_m < 1.0 {
                    continue;
                }
                assert!(
                    (f64::from(water.width_m) / p.width_m - 1.0).abs() < 0.15,
                    "the record's width {} against the centreline's {:.2} at ({:.1}, {:.1})",
                    water.width_m,
                    p.width_m,
                    p.x,
                    p.z
                );
                sampled += 1;
            }
        }
        reaches += 1;
    }
    assert!(
        reaches >= 10 && sampled >= 100,
        "{reaches} reaches, {sampled} samples"
    );
}

/// Whether a lake's water stands against ground at least its level all
/// round: every point of the lake's water whose neighbour 24 m off is not
/// that water must have the neighbour's ground at the level, or be a
/// river's water at the level (its outlet or an inlet). Returns the wet
/// points and the leaks.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "a test grid's indices"
)]
fn lake_holds(level: f32, nodes: &[[i32; 2]]) -> (usize, Vec<[f64; 2]>) {
    let step = 24.0;
    let (mut i0, mut j0, mut i1, mut j1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for n in nodes {
        (i0, j0, i1, j1) = (i0.min(n[0]), j0.min(n[1]), i1.max(n[0]), j1.max(n[1]));
    }
    let (x0, z0) = (f64::from(i0 - 1) * 512.0, f64::from(j0 - 1) * 512.0);
    let nx = (f64::from(i1 - i0 + 2) * 512.0 / step) as usize + 1;
    let nz = (f64::from(j1 - j0 + 2) * 512.0 / step) as usize + 1;
    let mut grid = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let (x, z) = (x0 + i as f64 * step, z0 + j as f64 * step);
            let s = world().sample(x, z);
            let water = s.standing_water().copied();
            let wet = water
                .is_some_and(|w| w.kind == WaterKind::Lake && (w.surface_m - level).abs() < 0.01);
            let river =
                water.is_some_and(|w| w.kind != WaterKind::Lake && w.surface_m >= level - 0.5);
            grid.push((wet, river, s.height));
        }
    }
    let (mut wet, mut leaks) = (0, Vec::new());
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
                    leaks.push([x0 + ii as f64 * step, z0 + jj as f64 * step]);
                }
            }
        }
    }
    (wet, leaks)
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "a test grid's indices"
)]
fn every_lake_within_60_km_holds_its_water() {
    // Not perched (its water never stands against ground under its level
    // but at its outlet), and not dry.
    let [cx, cz] = city();
    let mut lakes = 0;
    for network in world().hydrology().networks() {
        for lake in network.lakes() {
            let [i, j] = lake.nodes[0];
            if (f64::from(i) * 512.0 - cx).hypot(f64::from(j) * 512.0 - cz) > 60_000.0 {
                continue;
            }
            let (wet, leaks) = lake_holds(lake.level_m, &lake.nodes);
            assert!(wet > 0, "the lake at {:?} is dry", lake.nodes[0]);
            assert!(
                leaks.is_empty(),
                "the lake at level {} leaks at {:?}",
                lake.level_m,
                &leaks[..leaks.len().min(4)]
            );
            lakes += 1;
        }
    }
    assert!(lakes >= 20, "{lakes} lakes");
}

#[test]
fn every_outlet_within_60_km_carries_its_water_past_the_rim() {
    // Along each lake outlet's channel over its first 500 m, through the
    // rim the carve keeps over the lake's level and past it: the ground at
    // the centre never stands over the water. A lake whose outlet the rim
    // dams holds its water, so `every_lake_within_60_km_holds_its_water`
    // cannot see this (ticket 0005, the review of step 3b: 18 of 20
    // outlets ran under a land bridge at the level and 0.3 m).
    let [cx, cz] = city();
    let params = RiverParams::DEFAULT;
    let (mut outlets, mut walked) = (0, 0);
    let mut dams = Vec::new();
    for network in world().hydrology().networks() {
        for (id, lake) in network.lakes().iter().enumerate() {
            let [i, j] = lake.nodes[0];
            if (f64::from(i) * 512.0 - cx).hypot(f64::from(j) * 512.0 - cz) > 60_000.0 {
                continue;
            }
            let id = u32::try_from(id).expect("a mass's lakes");
            for river in network.rivers().iter().filter(|r| r.from_lake == Some(id)) {
                outlets += 1;
                let points = river.centreline(&params, 2.0);
                for p in points.iter().take_while(|p| p.along_m < 500.0) {
                    let sample = world().sample(p.x, p.z);
                    let Some(water) = sample.water.filter(|w| w.kind != WaterKind::Lake) else {
                        continue;
                    };
                    walked += 1;
                    let over = f64::from(sample.height) - f64::from(water.surface_m);
                    if over > 0.25 {
                        dams.push((lake.level_m, p.along_m, [p.x, p.z], over));
                    }
                }
            }
        }
    }
    assert!(
        outlets >= 15 && walked > 2_000,
        "{outlets} outlets, {walked} points"
    );
    assert!(
        dams.is_empty(),
        "{} points of outlets under ground, as (level, along, at, over): {:?}",
        dams.len(),
        &dams[..dams.len().min(6)]
    );
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a percentile's index and a share of a count"
)]
fn valleys_near_port_serein_are_cut_no_deeper_than_their_ground_allows() {
    // The cut at each vertex (natural ground over the surface) within 25 km:
    // most rivers run a few metres to tens under their ground; the tail is
    // where the coarse field the network routes on misses the fine ground's
    // relief (ticket 0005, step 2's "What is left").
    let [cx, cz] = city();
    let mut cuts: Vec<f64> = world()
        .hydrology()
        .networks()
        .iter()
        .flat_map(|n| n.rivers().iter())
        .flat_map(|r| r.vertices.iter())
        .filter(|v| (v.x - cx).hypot(v.z - cz) < 25_000.0)
        .map(|v| f64::from(v.cut_m))
        .collect();
    cuts.sort_by(f64::total_cmp);
    let pct = |p: f64| cuts[((cuts.len() - 1) as f64 * p) as usize];
    let over = cuts.iter().filter(|&&c| c > 60.0).count() as f64 / cuts.len() as f64;
    eprintln!(
        "cut within 25 km: 50 {:.1} 90 {:.1} 95 {:.1} 99 {:.1}; over 60 m {:.2} %",
        pct(0.5),
        pct(0.9),
        pct(0.95),
        pct(0.99),
        over * 100.0
    );
    assert!(cuts.len() > 2_000, "{} vertices", cuts.len());
    assert!(
        pct(0.5) < 15.0 && pct(0.95) < 70.0,
        "the cut's median {} and 95th {}",
        pct(0.5),
        pct(0.95)
    );
    assert!(
        over < 0.06,
        "{:.2} % of vertices cut over 60 m",
        over * 100.0
    );
}
