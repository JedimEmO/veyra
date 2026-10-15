//! The carve's measures (ticket 0005, step 2), printed. Ignored: each bakes
//! the rivers over 100 km round the origin. Run in release, one at a time:
//! `cargo test --release -p strata-worldgen --test carve_measure -- --ignored --nocapture --test-threads=1`.

// Probes, printed and never gated: their casts, their letters and their
// length are a measure's, not the crate's code.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::type_complexity,
    reason = "probes, printed and never gated"
)]

mod common;

use std::sync::OnceLock;
use std::time::Instant;

use strata_core::GRID_N;
use strata_worldgen::hydrology::{River, RiverEnd, WaterKind};
use strata_worldgen::measure;
use strata_worldgen::{ProceduralWorld, Sample, WorldSource};

fn world() -> &'static ProceduralWorld {
    static WORLD: OnceLock<ProceduralWorld> = OnceLock::new();
    WORLD.get_or_init(|| common::world(42))
}

/// Every reach with a vertex within `radius` of `(cx, cz)`.
fn reaches_near(cx: f64, cz: f64, radius: f64) -> Vec<River> {
    world()
        .hydrology()
        .networks()
        .iter()
        .flat_map(|network| network.rivers().to_vec())
        .filter(|river| {
            river
                .vertices
                .iter()
                .any(|v| ((v.x - cx).powi(2) + (v.z - cz).powi(2)).sqrt() < radius)
        })
        .collect()
}

fn h(x: f64, z: f64) -> f64 {
    f64::from(world().sample(x, z).height)
}

fn natural(x: f64, z: f64) -> f64 {
    world().terms(x, z).height_m
}

/// The world as it was before the carve.
struct Before<'a>(&'a ProceduralWorld);

impl WorldSource for Before<'_> {
    fn sample(&self, x: f64, z: f64) -> Sample {
        self.0.sample_before_carve(x, z)
    }
    fn sea_level(&self) -> f32 {
        0.0
    }
}

/// The carved world with its slope and aspect from central differences of
/// its heights instead of the gradient it carries.
struct Differenced<'a>(&'a ProceduralWorld);

impl WorldSource for Differenced<'_> {
    #[allow(clippy::cast_possible_truncation, reason = "a sample's f32")]
    fn sample(&self, x: f64, z: f64) -> Sample {
        let mut s = self.0.sample(x, z);
        let e = 0.05;
        let gx = (f64::from(self.0.sample(x + e, z).height)
            - f64::from(self.0.sample(x - e, z).height))
            / (2.0 * e);
        let gz = (f64::from(self.0.sample(x, z + e).height)
            - f64::from(self.0.sample(x, z - e).height))
            / (2.0 * e);
        let slope = (gx * gx + gz * gz).sqrt();
        s.landform.slope = slope as f32;
        s.landform.aspect = if slope > 0.0 {
            [(-gx / slope) as f32, (-gz / slope) as f32]
        } else {
            [0.0, 0.0]
        };
        s
    }
    fn sea_level(&self) -> f32 {
        0.0
    }
}

/// The carved world, but where water is near the point the ground before
/// the carve: the elongation without the valleys.
struct Dry<'a>(&'a ProceduralWorld);

impl WorldSource for Dry<'_> {
    fn sample(&self, x: f64, z: f64) -> Sample {
        let s = self.0.sample(x, z);
        if s.water.is_some() {
            self.0.sample_before_carve(x, z)
        } else {
            s
        }
    }
    fn sea_level(&self) -> f32 {
        0.0
    }
}

fn tiles<W: WorldSource + Sync>(w: &W) -> Vec<measure::ProvinceTiles> {
    let registries = common::registries();
    let mountains = registries
        .biome_id(&strata_registry::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("ships");
    common::sweep(|| {
        measure::province_tiles(
            w,
            |s| f64::from(s.height) > measure::LAND_MIN_M && s.dominant_biome() != mountains,
            measure::PATCH_RADIUS_M,
            8,
            common::THREADS,
        )
    })
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_elongation() {
    let w = world();
    let registries = common::registries();
    for (name, t) in [
        ("carved", tiles(w)),
        ("before", tiles(&Before(w))),
        ("dry", tiles(&Dry(w))),
        ("differenced", tiles(&Differenced(w))),
    ] {
        let line: Vec<String> = t
            .iter()
            .map(|p| {
                let name = registries
                    .provinces
                    .iter()
                    .find(|(id, _, _)| *id == p.province)
                    .map_or("?".to_owned(), |(_, n, _)| n.to_string());
                format!(
                    "{name} {:.2} ({:.2} hollows)",
                    p.elongation, p.hollows_per_km2
                )
            })
            .collect();
        println!("{name}: {}", line.join(", "));
    }
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_mouths() {
    let mut worst: Vec<(f64, [f64; 2])> = Vec::new();
    let mut mouths = 0;
    for river in reaches_near(0.0, 0.0, 25_000.0) {
        if river.end != RiverEnd::Sea {
            continue;
        }
        mouths += 1;
        let v = &river.vertices;
        let n = v.len();
        let mut biggest = (0.0_f64, [0.0, 0.0]);
        for k in n.saturating_sub(8)..n - 1 {
            let (a, b) = (v[k], v[k + 1]);
            let length = ((b.x - a.x).powi(2) + (b.z - a.z).powi(2)).sqrt();
            let mut s = 0.0;
            let mut last: Option<(f64, f64)> = None;
            while s <= length {
                let f = s / length;
                let (x, z) = (a.x + (b.x - a.x) * f, a.z + (b.z - a.z) * f);
                let (hh, nn) = (h(x, z), natural(x, z));
                if let Some((h0, n0)) = last {
                    let step = (hh - h0).abs() - (nn - n0).abs();
                    if step > biggest.0 {
                        biggest = (step, [x, z]);
                    }
                }
                last = Some((hh, nn));
                s += 0.25;
            }
        }
        worst.push(biggest);
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = worst.iter().filter(|w| w.0 > 0.5).count();
    println!(
        "mouths {mouths}: {over} with a step over 0.5 m in 0.25 m beyond the natural's; worst {:?}",
        &worst[..worst.len().min(5)]
    );
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_bed() {
    let mut rises: Vec<(f64, [f64; 2])> = Vec::new();
    let mut walked = 0.0;
    for river in reaches_near(0.0, 0.0, 25_000.0) {
        let mut held = f64::INFINITY;
        let mut lake_or_sea = false;
        for pair in river.vertices.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = ((b.x - a.x).powi(2) + (b.z - a.z).powi(2)).sqrt();
            let mut s = 0.0;
            while s < length {
                let f = s / length;
                let (x, z) = (a.x + (b.x - a.x) * f, a.z + (b.z - a.z) * f);
                s += 2.0;
                let sample = world().sample(x, z);
                if natural(x, z) < 1.0 || sample.water.is_some_and(|w| w.kind == WaterKind::Lake) {
                    held = f64::INFINITY;
                    lake_or_sea = true;
                    continue;
                }
                let bed = f64::from(sample.height);
                if bed > held + 0.05 && !lake_or_sea {
                    rises.push((bed - held, [x, z]));
                }
                lake_or_sea = false;
                held = held.min(bed);
                walked += 2.0;
            }
        }
    }
    rises.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "walked {walked:.0} m; {} rises over 5 cm above the running minimum; worst {:?}",
        rises.len(),
        &rises[..rises.len().min(8)]
    );
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_profiles() {
    // Stretches of a reach whose surface stays within 0.1 m for over 300 m
    // while cut more than 15 m under the ground: the flat canals.
    let mut flats: Vec<(f64, f64, [f64; 2])> = Vec::new();
    let mut cuts: Vec<f64> = Vec::new();
    for river in reaches_near(0.0, 0.0, 25_000.0) {
        let v = &river.vertices;
        let mut start = 0;
        let mut run = 0.0;
        for k in 1..v.len() {
            cuts.push(f64::from(v[k].cut_m));
            let d = ((v[k].x - v[k - 1].x).powi(2) + (v[k].z - v[k - 1].z).powi(2)).sqrt();
            if (v[start].surface_m - v[k].surface_m).abs() < 0.1 {
                run += d;
            } else {
                let deep = v[start..k].iter().map(|x| x.cut_m).fold(0.0_f32, f32::max);
                if run > 300.0 && deep > 15.0 {
                    flats.push((run, f64::from(deep), [v[start].x, v[start].z]));
                }
                start = k;
                run = 0.0;
            }
        }
    }
    flats.sort_by(|a, b| b.0.total_cmp(&a.0));
    cuts.sort_by(f64::total_cmp);
    let pct = |p: f64| cuts[((cuts.len() - 1) as f64 * p) as usize];
    println!(
        "flat canals over 300 m cut over 15 m: {}; longest {:?}",
        flats.len(),
        &flats[..flats.len().min(6)]
    );
    println!(
        "cut percentiles 50 {:.1} 90 {:.1} 95 {:.1} 99 {:.1} max {:.1}",
        pct(0.5),
        pct(0.9),
        pct(0.95),
        pct(0.99),
        pct(1.0)
    );
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_gradient() {
    let mut off = 0;
    let mut total = 0;
    let mut worst: Vec<(f64, f64, f64, [f64; 2])> = Vec::new();
    for river in reaches_near(0.0, 0.0, 25_000.0) {
        for (k, v) in river.vertices.iter().enumerate().step_by(7) {
            let next = river.vertices[(k + 1).min(river.vertices.len() - 1)];
            let (dx, dz) = (next.x - v.x, next.z - v.z);
            let l = (dx * dx + dz * dz).sqrt().max(1e-9);
            let nrm = [-dz / l, dx / l];
            for o in [0.0, 3.0, 9.0, 20.0, 45.0, 80.0, 130.0, 190.0, 260.0] {
                for side in [-1.0, 1.0] {
                    let (x, z) = (v.x + nrm[0] * o * side, v.z + nrm[1] * o * side);
                    let s = world().sample(x, z);
                    let b = world().sample_before_carve(x, z);
                    if s.water.is_none() || natural(x, z) < 1.0 || s.height == b.height {
                        continue;
                    }
                    let e = 0.05;
                    let gx = (h(x + e, z) - h(x - e, z)) / (2.0 * e);
                    let gz = (h(x, z + e) - h(x, z - e)) / (2.0 * e);
                    let actual = (gx * gx + gz * gz).sqrt();
                    let said = f64::from(s.landform.slope);
                    // The natural sample's own error at the point, which the
                    // carve carries where it does not replace the ground.
                    let bh = |x: f64, z: f64| f64::from(world().sample_before_carve(x, z).height);
                    let bx = (bh(x + e, z) - bh(x - e, z)) / (2.0 * e);
                    let bz = (bh(x, z + e) - bh(x, z - e)) / (2.0 * e);
                    let natural_err =
                        ((bx * bx + bz * bz).sqrt() - f64::from(b.landform.slope)).abs();
                    total += 1;
                    let err = (actual - said).abs();
                    if err > 0.05 + 0.1 * actual + natural_err {
                        off += 1;
                        worst.push((err, said, actual, [x, z]));
                    }
                }
            }
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "gradient: {off} of {total} carved points off by over 0.05 + 10 %; worst {:?}",
        &worst[..worst.len().min(6)]
    );
}

#[test]
#[ignore = "wall clock; run in release"]
fn probe_cost() {
    let w = world();
    let n = GRID_N + 2;
    let mut out = Vec::new();
    w.sample_grid(0.0, 0.0, 1.0, n, &mut out);
    for (place, x0, z0) in [
        ("river valley", -100.0, -5_280.0),
        ("mountains", 15_000.0, -17_000.0),
    ] {
        let runs = 20;
        let start = Instant::now();
        for k in 0..runs {
            w.sample_grid(x0 + f64::from(k) * 64.0, z0, 1.0, n, &mut out);
        }
        let carved = start.elapsed() / runs / u32::try_from(n * n).expect("small");
        let start = Instant::now();
        for k in 0..runs {
            for j in 0..n {
                for i in 0..n {
                    #[allow(clippy::cast_precision_loss, reason = "small")]
                    let _ = std::hint::black_box(
                        w.sample_before_carve(x0 + f64::from(k) * 64.0 + i as f64, z0 + j as f64),
                    );
                }
            }
        }
        let before = start.elapsed() / runs / u32::try_from(n * n).expect("small");
        let wet = out.iter().filter(|s| s.water.is_some()).count();
        println!(
            "{place}: carved {carved:?}, before the carve {before:?}; last grid {wet} of {} by water",
            out.len()
        );
    }
}

fn unit_hash(i: u64) -> f64 {
    let mut h = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 11) as f64 / (1_u64 << 53) as f64
}

fn elong([a, b, c]: [f64; 3]) -> f64 {
    let mid = f64::midpoint(a, c);
    let spread = ((a - c) * 0.5 * (a - c) * 0.5 + b * b).sqrt();
    ((mid + spread) / (mid - spread)).sqrt()
}

/// The folded belt's tiles as the drama measure picks them, and what the
/// carved points add to its gradient tensor.
#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_folded() {
    let registries = common::registries();
    let mountains = registries
        .biome_id(&strata_registry::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("ships");
    for name in ["strata:folded", "strata:basalt", "strata:sandstone"] {
        let folded = registries
            .province_id(&strata_registry::Namespaced::parse(name).expect("valid"))
            .expect("ships");
        let w = world();
        let tile_m = measure::TILE_M;
        let n = (measure::PATCH_RADIUS_M / tile_m).floor() as i64;
        let centres: Vec<[f64; 2]> = (-n..=n)
            .flat_map(|j| (-n..=n).map(move |i| [i as f64 * tile_m, j as f64 * tile_m]))
            .filter(|[x, z]| x * x + z * z <= measure::PATCH_RADIUS_M * measure::PATCH_RADIUS_M)
            .collect();
        let mut mine: Vec<(u64, [f64; 2])> = Vec::new();
        for (k, &[cx, cz]) in centres.iter().enumerate() {
            let mut ok = true;
            'l: for j in 0..5 {
                for i in 0..5 {
                    let (x, z) = (
                        cx + (f64::from(i) / 4.0 - 0.5) * tile_m,
                        cz + (f64::from(j) / 4.0 - 0.5) * tile_m,
                    );
                    let s = w.sample(x, z);
                    let (id, wt) = s.provinces[0];
                    if id != folded
                        || wt < measure::PROVINCE_ALONE
                        || !(f64::from(s.height) > measure::LAND_MIN_M
                            && s.dominant_biome() != mountains)
                    {
                        ok = false;
                        break 'l;
                    }
                }
            }
            if ok {
                mine.push((unit_hash(k as u64 ^ 0x5EED).to_bits(), [cx, cz]));
            }
        }
        mine.sort_unstable_by_key(|(h, _)| *h);
        mine.truncate(8);
        let (mut all, mut carved_t, mut before_t, mut dry_t) =
            ([0.0; 3], [0.0; 3], [0.0; 3], [0.0; 3]);
        let (mut carved_n, mut total_n) = (0usize, 0usize);
        let mut flows = [0.0; 3];
        for &(_, [cx, cz]) in &mine {
            let steps = (tile_m / measure::TILE_STEP_M).round() as usize;
            for j in 0..=steps {
                for i in 0..=steps {
                    let (x, z) = (
                        cx - tile_m * 0.5 + i as f64 * measure::TILE_STEP_M,
                        cz - tile_m * 0.5 + j as f64 * measure::TILE_STEP_M,
                    );
                    let s = w.sample(x, z);
                    let b = w.sample_before_carve(x, z);
                    let g = |s: &Sample| {
                        let slope = f64::from(s.landform.slope);
                        let (gx, gz) = (
                            -f64::from(s.landform.aspect[0]) * slope,
                            -f64::from(s.landform.aspect[1]) * slope,
                        );
                        [gx * gx, gx * gz, gz * gz]
                    };
                    let gs = g(&s);
                    for k in 0..3 {
                        all[k] += gs[k];
                    }
                    total_n += 1;
                    if s.height.to_bits() == b.height.to_bits() {
                        for k in 0..3 {
                            dry_t[k] += gs[k];
                        }
                    } else {
                        carved_n += 1;
                        let gb = g(&b);
                        for k in 0..3 {
                            carved_t[k] += gs[k];
                            before_t[k] += gb[k];
                        }
                        if let Some(water) = s.water {
                            let f = water.flow;
                            let l = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9);
                            let (fx, fz) = (f64::from(f[0] / l), f64::from(f[1] / l));
                            flows[0] += fx * fx;
                            flows[1] += fx * fz;
                            flows[2] += fz * fz;
                        }
                    }
                }
            }
        }
        let mut no_carve = dry_t;
        for k in 0..3 {
            no_carve[k] += before_t[k];
        }
        println!(
            "{name}: {} tiles, carved {carved_n} of {total_n}; elongation all {:.2}, dry part {:.2}, carved part {:.2} (before the carve {:.2}), without the carve {:.2}; tensor shares: carved {:.2} of all; flow direction tensor elongation {:.2} (xx {:.2} xz {:.2} zz {:.2}); dry tensor {:?}, carved tensor {:?}",
            mine.len(),
            elong(all),
            elong(dry_t),
            elong(carved_t),
            elong(before_t),
            elong(no_carve),
            (carved_t[0] + carved_t[2]) / (all[0] + all[2]),
            elong(flows),
            flows[0],
            flows[1],
            flows[2],
            dry_t,
            carved_t
        );
    }
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_deep_cuts() {
    let mut counts = [0usize; 5];
    let mut total = 0usize;
    let mut worst: Vec<(f32, [f64; 2], f32, f32)> = Vec::new();
    for network in world().hydrology().networks() {
        for river in network.rivers() {
            for v in &river.vertices {
                if v.x.abs() > 100_000.0 || v.z.abs() > 100_000.0 {
                    continue;
                }
                total += 1;
                for (k, limit) in [50.0, 100.0, 200.0, 400.0, 800.0].iter().enumerate() {
                    if v.cut_m > *limit {
                        counts[k] += 1;
                    }
                }
                if v.cut_m > 200.0 {
                    worst.push((v.cut_m, [v.x, v.z], v.surface_m, v.discharge_m3s));
                }
            }
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "{total} vertices; cut over 50/100/200/400/800 m: {counts:?}; worst {:?}",
        &worst[..worst.len().min(8)]
    );
}

#[test]
#[ignore = "bakes 100 km; run in release"]
fn probe_fixtures() {
    let w = world();
    let site = &w.sites()[0];
    let city = site.to_world_xz([0.0, 0.0]);
    let road = site
        .road
        .points
        .last()
        .map(|p| (p[0], p[1]))
        .expect("a road");
    let mut points: Vec<(String, (f64, f64))> = vec![
        ("woodland".into(), (-7500.0, 21500.0)),
        ("city-bay".into(), (-146.0, -289.0)),
        ("city-terraces".into(), (-542.0, -424.0)),
        ("city".into(), (city[0], city[1])),
        ("road end".into(), road),
        ("origin".into(), (0.0, 0.0)),
    ];
    for (name, x, z) in STUDY {
        points.push((name.to_string(), (*x, *z)));
    }
    for (name, (x, z)) in &points {
        let s = w.sample(*x, *z);
        let b = w.sample_before_carve(*x, *z);
        println!(
            "{name} ({x:.0}, {z:.0}): in water {}, water {}, moved {:.3} m",
            s.in_water(),
            s.water.is_some(),
            s.height - b.height
        );
    }
    // The walk west 800 m from (-1200, -200), the flight west 14 km from
    // (15000, -12600).
    for (name, (x0, z0), length) in [
        ("walk", (-1200.0, -200.0), 800.0),
        ("flight", (15000.0, -12600.0), 14_000.0),
    ] {
        let (mut wet, mut moved, mut n) = (0, 0, 0);
        let mut s = 0.0;
        while s <= length {
            let (x, z) = (x0 - s, z0);
            let a = w.sample(x, z);
            if a.in_water() {
                wet += 1;
            }
            if a.height != w.sample_before_carve(x, z).height {
                moved += 1;
            }
            n += 1;
            s += 5.0;
        }
        println!("{name}: {n} points, {wet} in water, {moved} moved by the carve");
    }
}

const STUDY: &[(&str, f64, f64)] = &[
    ("grove", -43500.0, -2500.0),
    ("valley", 34270.0, 13030.0),
    ("coast", 21949.0, -15945.0),
];
