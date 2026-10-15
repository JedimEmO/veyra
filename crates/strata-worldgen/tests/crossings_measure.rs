//! The measures behind ticket 0005's step 7 record (the crossings): ignored
//! probes, run in release, one at a time:
//!
//! ```text
//! cargo test --release -p strata-worldgen --test crossings_measure -- --ignored --nocapture --test-threads=1
//! ```

mod common;

use strata_core::Seed;
use strata_worldgen::ProceduralWorld;
use strata_worldgen::roads::{CrossingKind, RoadClass};

/// Every crossing within 200 km of Port Serein by kind and class: the
/// bridges' gaps between their abutments in 4 m bins, their spans, decks
/// over the water and over the bed; culverts' fills; fords' widths.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a probe's report, in order"
)]
fn probe_crossings() {
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
    for radius in [12_000.0, 60_000.0, 200_000.0] {
        println!("== within {:.0} km", radius / 1e3);
        for class in [RoadClass::Service, RoadClass::Track, RoadClass::Trail] {
            let mut gaps = Vec::new();
            let mut over_bed = Vec::new();
            let mut over_water = Vec::new();
            let mut culverts = Vec::new();
            let mut fords = Vec::new();
            for network in &networks {
                for road in network.roads() {
                    if road.class != class {
                        continue;
                    }
                    for c in &road.crossings {
                        if from_city(c.at) > radius {
                            continue;
                        }
                        match c.kind {
                            CrossingKind::Bridge => {
                                gaps.push((c.abutments_m[1] - c.abutments_m[0], c));
                                over_bed.push(c.deck_m - (c.surface_m - c.depth_m));
                                over_water.push(c.deck_m - c.surface_m);
                            }
                            CrossingKind::Culvert => {
                                culverts.push((c.deck_m - c.surface_m, c.width_m, c.span_m));
                            }
                            CrossingKind::Ford => fords.push((c.span_m, c.depth_m)),
                        }
                    }
                }
            }
            gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
            over_bed.sort_by(f64::total_cmp);
            over_water.sort_by(f64::total_cmp);
            let pct = |v: &[f64], q: f64| {
                if v.is_empty() {
                    f64::NAN
                } else {
                    v[((v.len() - 1) as f64 * q).round() as usize]
                }
            };
            let g: Vec<f64> = gaps.iter().map(|g| g.0).collect();
            println!(
                "{class:?}: {} bridges, gap min {:.1} p10 {:.1} p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}; deck over water p50 {:.1} max {:.1}; over bed p50 {:.1} p90 {:.1} max {:.1}",
                g.len(),
                pct(&g, 0.0),
                pct(&g, 0.1),
                pct(&g, 0.5),
                pct(&g, 0.9),
                pct(&g, 0.99),
                pct(&g, 1.0),
                pct(&over_water, 0.5),
                pct(&over_water, 1.0),
                pct(&over_bed, 0.5),
                pct(&over_bed, 0.9),
                pct(&over_bed, 1.0),
            );
            let mut bins = std::collections::BTreeMap::new();
            for &x in &g {
                *bins.entry((x / 4.0).floor() as u32 * 4).or_insert(0) += 1;
            }
            let line: Vec<String> = bins.iter().map(|(b, n)| format!("{b}:{n}")).collect();
            println!("  gaps by 4 m: {}", line.join(" "));
            if radius > 100_000.0 {
                for (gap, c) in gaps.iter().rev().take(5) {
                    println!(
                        "  longest: gap {gap:.1} span {:.1} at ({:.0}, {:.0}) deck {:.1} water {:.1} depth {:.1} width {:.1} banks {:?}",
                        c.span_m,
                        c.at[0],
                        c.at[1],
                        c.deck_m,
                        c.surface_m,
                        c.depth_m,
                        c.width_m,
                        c.banks_m
                    );
                }
                for (gap, c) in gaps.iter().take(3) {
                    println!(
                        "  shortest: gap {gap:.1} span {:.1} at ({:.0}, {:.0}) width {:.1}",
                        c.span_m, c.at[0], c.at[1], c.width_m
                    );
                }
            }
            if !culverts.is_empty() {
                let mut fill: Vec<f64> = culverts.iter().map(|c| c.0).collect();
                fill.sort_by(f64::total_cmp);
                let mut width: Vec<f64> = culverts.iter().map(|c| c.1).collect();
                width.sort_by(f64::total_cmp);
                println!(
                    "  {} culverts: fill over water p10 {:.2} p50 {:.2} p90 {:.2} max {:.2}; creek width p50 {:.2} max {:.2}",
                    culverts.len(),
                    pct(&fill, 0.1),
                    pct(&fill, 0.5),
                    pct(&fill, 0.9),
                    pct(&fill, 1.0),
                    pct(&width, 0.5),
                    pct(&width, 1.0)
                );
            }
            if !fords.is_empty() {
                let mut width: Vec<f64> = fords.iter().map(|f| f.0).collect();
                width.sort_by(f64::total_cmp);
                println!(
                    "  {} fords: wetted width p10 {:.1} p50 {:.1} p90 {:.1} max {:.1}",
                    fords.len(),
                    pct(&width, 0.1),
                    pct(&width, 0.5),
                    pct(&width, 0.9),
                    pct(&width, 1.0)
                );
            }
        }
    }
    // Every crossing within 6 km of the city, for the captures.
    for network in &networks {
        for (r, road) in network.roads().iter().enumerate() {
            for c in &road.crossings {
                if from_city(c.at) <= 6_000.0 {
                    println!(
                        "near: road {r} {:?} {:?} at ({:.0}, {:.0}) bearing ({:.2}, {:.2}) span {:.1} gap {:.1} deck {:.2} water {:.2} depth {:.2} width {:.1} banks {:.1?}",
                        road.class,
                        c.kind,
                        c.at[0],
                        c.at[1],
                        c.bearing[0],
                        c.bearing[1],
                        c.span_m,
                        c.abutments_m[1] - c.abutments_m[0],
                        c.deck_m,
                        c.surface_m,
                        c.depth_m,
                        c.width_m,
                        c.banks_m
                    );
                }
            }
        }
    }
    // The showcase and the next bridge.
    for network in &networks {
        for road in network.roads() {
            for c in &road.crossings {
                for p in [
                    [7_404.0, -4_940.0],
                    [11_934.0, -7_390.0],
                    [7_702.0, -5_078.0],
                ] {
                    if (c.at[0] - p[0]).abs() < 30.0 && (c.at[1] - p[1]).abs() < 30.0 {
                        println!("showcase {p:?}: {:?} {c:?}", road.class);
                    }
                }
            }
        }
    }
}

/// What the baked kit (`generated/crossings/veyra/kit.ron`, `just content`
/// first) stands at every crossing within 200 km of Port Serein: placed or
/// why not, by kind and class; the gaps the bridges leave out; and every
/// crossing within 5 km of the city with its pieces.
#[test]
#[ignore = "a probe: release, by hand, after `just content`"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a probe's report, in order"
)]
fn probe_placed() {
    use strata_worldgen::crossings::{CrossingKit, KIT_PATH, of_network, place_crossing};
    let kit: CrossingKit = ron::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated"))
                .join(KIT_PATH),
        )
        .expect("the kit: run `just content`"),
    )
    .expect("the kit reads");
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
    let mut outcomes: std::collections::BTreeMap<(String, String, String), usize> =
        std::collections::BTreeMap::new();
    let mut pieces_by_kind: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut unbridged = Vec::new();
    let started = std::time::Instant::now();
    let mut count = 0;
    for network in &networks {
        for c in of_network(network, |p| from_city(p) <= 200_000.0) {
            count += 1;
            let placed = place_crossing(&c, &kit, &world);
            let outcome = match &placed {
                Ok(pieces) if pieces.is_empty() => "nothing".to_owned(),
                Ok(_) => "placed".to_owned(),
                Err(why) => format!("{why:?}"),
            };
            if let (Ok(pieces), CrossingKind::Culvert, false) =
                (&placed, c.crossing.kind, c.class == RoadClass::Trail)
            {
                *pieces_by_kind
                    .entry(format!(
                        "{:?} culverts with {} headwalls",
                        c.class,
                        pieces.len()
                    ))
                    .or_default() += 1;
            }
            if let Ok(pieces) = &placed {
                *pieces_by_kind
                    .entry(format!("{:?} {:?}", c.class, c.crossing.kind))
                    .or_default() += pieces.len();
            }
            if c.crossing.kind == CrossingKind::Bridge && placed.is_err() {
                unbridged.push((c, outcome.clone()));
            }
            *outcomes
                .entry((
                    format!("{:?}", c.crossing.kind),
                    format!("{:?}", c.class),
                    outcome,
                ))
                .or_default() += 1;
        }
    }
    println!(
        "{count} crossings within 200 km placed in {:.2} s",
        started.elapsed().as_secs_f64()
    );
    for ((kind, class, outcome), n) in &outcomes {
        println!("  {kind:8} {class:8} {outcome:10} {n}");
    }
    for (what, n) in &pieces_by_kind {
        println!("  pieces {what}: {n}");
    }
    for (c, why) in &unbridged {
        let x = &c.crossing;
        println!(
            "  unbridged {why}: {:?} gap {:.1} at ({:.0}, {:.0}) deck over bed {:.1}, {:.0} km out",
            c.class,
            x.abutments_m[1] - x.abutments_m[0],
            x.at[0],
            x.at[1],
            x.deck_m - (x.surface_m - x.depth_m),
            from_city(x.at) / 1e3
        );
    }
    // Round the city: what the game streams within 5 km of it and of the
    // showcase bridge, and every crossing within 6 km with its pieces.
    for (name, at) in [("the city", city), ("the showcase", [7_404.0, -4_940.0])] {
        let near = world.crossings_near(at[0], at[1], 5_000.0);
        let mut pieces = 0;
        let mut kinds = std::collections::BTreeMap::new();
        for c in &near {
            let placed = place_crossing(c, &kit, &world).unwrap_or_default();
            pieces += placed.len();
            *kinds
                .entry(format!("{:?} {:?}", c.class, c.crossing.kind))
                .or_insert(0) += 1;
        }
        println!(
            "within 5 km of {name}: {} crossings, {pieces} pieces: {kinds:?}",
            near.len()
        );
    }
    for c in world.crossings_near(city[0], city[1], 12_000.0) {
        let placed = place_crossing(&c, &kit, &world);
        let x = &c.crossing;
        println!(
            "near {} {:?} {:?} at ({:.0}, {:.0}) bearing ({:.2}, {:.2}) gap {:.1} deck {:.2} water {:.2}: {}",
            c.id,
            c.class,
            x.kind,
            x.at[0],
            x.at[1],
            x.bearing[0],
            x.bearing[1],
            x.abutments_m[1] - x.abutments_m[0],
            x.deck_m,
            x.surface_m,
            match &placed {
                Ok(p) => p
                    .iter()
                    .map(|p| format!(
                        "{} ({:.1}, {:.2}, {:.1})",
                        p.file.rsplit('/').next().unwrap_or(""),
                        p.at[0],
                        p.at[1],
                        p.at[2]
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                Err(why) => format!("{why:?}"),
            }
        );
    }
    for at in [
        [7_404.0, -4_940.0],
        [11_934.0, -7_390.0],
        [7_702.0, -5_078.0],
    ] {
        for c in world.crossings_near(at[0], at[1], 30.0) {
            println!("showcase {at:?}: {:?}", place_crossing(&c, &kit, &world));
        }
    }
}

/// The ground along a culvert's creek either side of the road, for the
/// headwalls' search.
#[test]
#[ignore = "a probe: release, by hand"]
fn probe_culvert() {
    use std::fmt::Write;
    use strata_worldgen::WorldSource;
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    world.prepare_water(7_000.0, -5_000.0, 6_000.0);
    for at in [[7_702.0, -5_078.0], [-1_534.0, -3_937.0]] {
        for c in world.crossings_near(at[0], at[1], 30.0) {
            let rec = c.crossing;
            let here = world.sample(rec.at[0], rec.at[1]);
            println!(
                "{:?} at {:?} surface {:.2} depth {:.2} width {:.2} water {:?}",
                rec.kind,
                rec.at,
                rec.surface_m,
                rec.depth_m,
                rec.width_m,
                here.water
                    .map(|w| (w.flow, w.signed_distance_m, w.surface_m, w.bed_m))
            );
            let water = here.water.expect("water");
            let length = (water.flow[0].powi(2) + water.flow[1].powi(2)).sqrt();
            let flow = [
                f64::from(water.flow[0] / length),
                f64::from(water.flow[1] / length),
            ];
            for side in [1.0, -1.0] {
                let mut line = String::new();
                for k in 0..30 {
                    let d = f64::from(k) * 2.0;
                    let p = [
                        rec.at[0] + flow[0] * d * side,
                        rec.at[1] + flow[1] * d * side,
                    ];
                    let g = world.sample(p[0], p[1]);
                    let _ = write!(
                        line,
                        " {d:.0}:{:.2}/{:.2}",
                        g.height,
                        g.water.map_or(f32::NAN, |w| w.surface_m)
                    );
                }
                println!("  side {side}: {line}");
            }
        }
    }
}

/// What skew the kit takes (the alignment pass): every graded road's
/// bridge within `RADIUS_KM` (default 60) of Port Serein, binned by the
/// angle between its deck's chord and the square to the river's flow, with
/// how it was placed and, for a placed one, along both edges of its
/// carriageway, whether the river's channel reaches under the deck past
/// the gap (into the shelf the abutment stands on) and how far the ground
/// inside the gap, 2.5 m in from its edges past the abutments' faces, stands
/// over a metre and a half under the deck's top.
#[test]
#[ignore = "a probe: release, by hand, after `just content`"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a probe's report, in order"
)]
fn probe_skew() {
    use strata_worldgen::WorldSource;
    use strata_worldgen::crossings::{CrossingKit, KIT_PATH, of_network, place_crossing};
    let kit: CrossingKit = ron::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated"))
                .join(KIT_PATH),
        )
        .expect("the kit: run `just content`"),
    )
    .expect("the kit reads");
    let radius = std::env::var("RADIUS_KM")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(60.0)
        * 1_000.0;
    let registries = common::registries();
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(common::THREADS);
    let city = world.sites()[0].centre;
    let networks = world.hydrology().bake_area(
        &world,
        (city[0] - radius, city[1] - radius),
        (city[0] + radius, city[1] + radius),
    );
    let from_city = |p: [f64; 2]| ((p[0] - city[0]).powi(2) + (p[1] - city[1]).powi(2)).sqrt();
    // Per 15 degree bin: bridges, placed, unmet, other unplaced, with the
    // channel under a shelf, with ground over the soffit's line, the worst
    // of that.
    let mut bins = [[0.0_f64; 7]; 5];
    for network in &networks {
        for c in of_network(network, |p| from_city(p) <= radius) {
            if c.crossing.kind != CrossingKind::Bridge || c.class == RoadClass::Trail {
                continue;
            }
            let x = &c.crossing;
            let (dx, dz) = (c.edges[1][0] - c.edges[0][0], c.edges[1][1] - c.edges[0][1]);
            let gap = (dx * dx + dz * dz).sqrt().max(1e-6);
            let dir = [dx / gap, dz / gap];
            let Some(water) = world.sample(x.at[0], x.at[1]).water else {
                continue;
            };
            let flow = [f64::from(water.flow[0]), f64::from(water.flow[1])];
            let fl = (flow[0] * flow[0] + flow[1] * flow[1]).sqrt().max(1e-9);
            let square = ((dir[0] * flow[1] - dir[1] * flow[0]) / fl).abs();
            let skew = square.clamp(0.0, 1.0).acos().to_degrees();
            let bin = ((skew / 15.0) as usize).min(4);
            bins[bin][0] += 1.0;
            let placed = place_crossing(&c, &kit, &world);
            match &placed {
                Ok(_) => bins[bin][1] += 1.0,
                Err(why) => {
                    if matches!(why, strata_worldgen::crossings::Unplaced::Unmet) {
                        bins[bin][2] += 1.0;
                    } else {
                        bins[bin][3] += 1.0;
                    }
                    println!(
                        "  unplaced: {:?} at ({:.0}, {:.0}), {why:?}, skew {skew:.0} deg, gap {gap:.1} m",
                        c.id, x.at[0], x.at[1]
                    );
                }
            }
            let Ok(pieces) = placed else { continue };
            let class = kit
                .bridges
                .iter()
                .find(|b| b.file == pieces[0].file)
                .expect("a placed class");
            let centre = [pieces[0].at[0], pieces[0].at[2]];
            let across = [dir[1], -dir[0]];
            let half = strata_worldgen::sites::way_in_half_m(c.class);
            let (mut wet, mut over) = (None, f64::NEG_INFINITY);
            for side in [-1.0, 1.0] {
                let mut z = -0.5 * class.length_m;
                while z <= 0.5 * class.length_m {
                    let p = [
                        centre[0] + dir[0] * z + across[0] * side * half,
                        centre[1] + dir[1] * z + across[1] * side * half,
                    ];
                    let s = world.sample(p[0], p[1]);
                    if z.abs() > 0.5 * gap {
                        // Under the deck past the gap: the shelf.
                        if let Some(w) = s.water.filter(|w| {
                            w.kind != strata_worldgen::hydrology::WaterKind::Lake
                                && f64::from(w.signed_distance_m.abs()) < 0.5 * f64::from(w.width_m)
                        }) {
                            wet.get_or_insert((side, z, w.width_m, w.order, w.signed_distance_m));
                        }
                    } else if z.abs() < 0.5 * gap - 2.5 {
                        over = over.max(f64::from(s.height) - (x.deck_m - 1.5));
                    }
                    z += 1.0;
                }
            }
            if let Some((side, z, width, order, off)) = wet {
                bins[bin][4] += 1.0;
                println!(
                    "  wet: {:?} at ({:.0}, {:.0}), skew {skew:.0} deg, gap {gap:.1} m, record width {:.1} m, river {} (water there {width:.1} m wide, order {order}, {off:.1} m off its line), edge {side} at {z:.0} m",
                    c.id, x.at[0], x.at[1], x.width_m, x.river
                );
            }
            if over > 0.0 {
                bins[bin][5] += 1.0;
            }
            if over > 1.5 {
                println!(
                    "  ground {over:.1} m over the soffit's line: {:?} at ({:.0}, {:.0}), skew {skew:.0} deg, gap {gap:.1} m, deck {:.1} m",
                    c.id, x.at[0], x.at[1], x.deck_m
                );
            }
            bins[bin][6] = bins[bin][6].max(over);
        }
    }
    for (k, b) in bins.iter().enumerate() {
        println!(
            "skew {:2}-{:2} deg: {:3} bridges, {:3} placed, {:2} unmet, {:2} other; of the placed {:2} with the channel under a shelf, {:2} with ground over the soffit's line (worst {:.1} m)",
            k * 15,
            k * 15 + 15,
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6]
        );
    }
}
