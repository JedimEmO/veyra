//! The mesher's time per region on the real seed-42 world, by level of
//! detail, for the lighting plan's phase 3a ceiling (the sky bake may cost
//! the mesher 15 percent). Ignored: a timing, not a check, and only
//! meaningful in a release build on an idle machine:
//!
//! ```text
//! cargo test --release -p strata-terrain --test sky_timing -- --ignored --nocapture
//! ```

use std::time::Instant;

use strata_core::{RegionCoord, Seed};
use strata_terrain::mesh::{BiomeLayers, build_region_mesh};
use strata_worldgen::{ProceduralWorld, WorldSource};

#[test]
#[ignore = "a timing; run in release on an idle machine"]
fn mesher_time_per_region() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::new(Seed(42), &registries);
    let layers = BiomeLayers::from_registries(&registries);
    let mut scratch = Vec::new();

    // One sample alone, so the bake's extra samples can be priced.
    let started = Instant::now();
    let mut sink = 0.0_f32;
    for i in 0..4_000 {
        let s = world.sample(f64::from(i) * 7.3, f64::from(i % 97) * 11.1);
        sink += s.height;
    }
    let per_sample_us = started.elapsed().as_secs_f64() * 1e6 / 4_000.0;
    println!("one sample: {per_sample_us:.2} us (sink {sink:.0})");

    // Warm the allocator and caches.
    let _ = build_region_mesh(
        RegionCoord::new(0, 0, 0),
        &world,
        &layers,
        0.05,
        &mut scratch,
    );
    let mut total = 0.0;
    for lod in 0..=6_u8 {
        let mut times = Vec::new();
        // Around the canopy fixture's woodland (`study::SCENES`), rolling
        // forest: neither the sea nor a flat plain.
        let size = RegionCoord::size_at(lod);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "region indices near the fixture"
        )]
        let (cx, cz) = (
            (-7_500.0 / size).floor() as i32,
            (21_500.0 / size).floor() as i32,
        );
        for z in cz - 3..cz + 3 {
            for x in cx - 3..cx + 3 {
                let coord = RegionCoord::new(x, z, lod);
                let started = Instant::now();
                let mesh = build_region_mesh(coord, &world, &layers, 0.05, &mut scratch);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                std::hint::black_box(mesh);
            }
        }
        times.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss, reason = "a count of 36")]
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        total += mean;
        println!(
            "lod {lod}: mean {mean:.3} ms, median {:.3} ms over {} regions",
            times[times.len() / 2],
            times.len()
        );
    }
    println!("sum of the means: {total:.3} ms");
}

/// Where the bake sees least sky, for a capture pose that shows a gully:
/// 24 by 24 regions of level `SKY_LOD` (default 3) round region `SKY_X`,
/// `SKY_Z` (default the origin), the darkest vertices on land with their
/// world position. The ravine of the phase 3a captures is `SKY_LOD=2
/// SKY_X=33 SKY_Z=-59`.
#[test]
#[ignore = "a search for a pose, not a check"]
fn darkest_gullies_near_the_spawn() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::new(Seed(42), &registries);
    let layers = BiomeLayers::from_registries(&registries);
    let mut scratch = Vec::new();
    let lod: u8 = std::env::var("SKY_LOD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let (cx, cz): (i32, i32) = (
        std::env::var("SKY_X")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        std::env::var("SKY_Z")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
    let mut found = Vec::new();
    for z in cz - 12..cz + 12 {
        for x in cx - 12..cx + 12 {
            let coord = RegionCoord::new(x, z, lod);
            let mesh = build_region_mesh(coord, &world, &layers, 0.05, &mut scratch);
            let corner = coord.min_corner();
            for (v, sky) in mesh.sky.iter().enumerate().take(65 * 65) {
                let p = mesh.positions[v];
                if p[1] > 5.0 {
                    found.push((
                        sky[0],
                        corner.x() + f64::from(p[0]),
                        corner.z() + f64::from(p[2]),
                        p[1],
                    ));
                }
            }
        }
    }
    found.sort_by_key(|f| f.0);
    for f in found.iter().take(12) {
        println!(
            "visibility {} at x {:.0} z {:.0}, {:.0} m up",
            f.0, f.1, f.2, f.3
        );
    }
    #[allow(clippy::cast_precision_loss, reason = "a vertex count far under 2^52")]
    let mean = found.iter().map(|f| f64::from(f.0)).sum::<f64>() / found.len() as f64;
    println!(
        "mean visibility on land {mean:.1} of 255 over {} vertices",
        found.len()
    );
}

/// The far paint's cost per region against the mesher's own, by level, in
/// the river country round the showcase bridge at (7 404, -4 940) (ticket
/// 0005, step 8a): 36 regions a level, each meshed and then painted, the
/// networks' lines warmed first as a running game has them. Ignored, as
/// the timing above:
///
/// ```text
/// cargo test --release -p strata-terrain --test sky_timing -- --ignored --nocapture paint_time
/// ```
#[test]
#[ignore = "a timing; run in release on an idle machine"]
fn paint_time_per_region() {
    use std::sync::Arc;

    use strata_terrain::build_region_mesh_painted;
    use strata_terrain::paint::{PaintCache, PaintSource, PaintedWays, pack_paint, region_paint};

    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = Arc::new(ProceduralWorld::new(Seed(42), &registries));
    world.prepare_water(7_404.0, -4_940.0, 40_000.0);
    let layers = BiomeLayers::from_registries(&registries);
    let source = PaintSource {
        world: Arc::clone(&world),
        cache: Arc::new(PaintCache::default()),
        ways: layers.ways(),
    };
    let mut scratch = Vec::new();
    // Warm the allocator, the bakes and the lines' cache.
    let warm = RegionCoord::new(0, -1, 8);
    let _ = build_region_mesh(warm, world.as_ref(), &layers, 0.05, &mut scratch);
    let _ = region_paint(warm, &source);
    let (mut mesh_total, mut paint_total) = (0.0, 0.0);
    for lod in 0..=8_u8 {
        let size = RegionCoord::size_at(lod);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "region indices near the showcase"
        )]
        let (cx, cz) = (
            (7_404.0 / size).floor() as i32,
            (-4_940.0 / size).floor() as i32,
        );
        let (mut mesh_ms, mut paint_ms) = (Vec::new(), Vec::new());
        let (mut strokes, mut dropped, mut bytes) = (0_usize, 0_u32, 0_usize);
        let (mut fullest, mut most, mut widened, mut listed) = (0_u32, 0_usize, 0_usize, 0_usize);
        for z in cz - 3..cz + 3 {
            for x in cx - 3..cx + 3 {
                let coord = RegionCoord::new(x, z, lod);
                // In the game's order: the paint, then the mesh, which
                // leaves the roads painted out of its splat.
                let started = Instant::now();
                let paint = region_paint(coord, &source);
                let words = pack_paint(&paint, coord);
                paint_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                let started = Instant::now();
                let ways = PaintedWays::of(&paint, coord.vertex_spacing());
                let mesh = build_region_mesh_painted(
                    coord,
                    world.as_ref(),
                    &layers,
                    (0.05, false),
                    &ways,
                    &mut scratch,
                );
                mesh_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                std::hint::black_box(mesh);
                widened += usize::from(paint.widened > 1);
                strokes += paint.strokes.len();
                most = most.max(paint.strokes.len());
                fullest = fullest.max(paint.fullest);
                dropped += paint.dropped;
                bytes += words.len() * 4;
                listed += words.get(1).map_or(0, |&n| n as usize);
            }
        }
        #[allow(clippy::cast_precision_loss, reason = "counts far under 2^52")]
        let (mean, per) = (
            |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64,
            |n: usize| n as f64 / 36.0,
        );
        let (m, p) = (mean(&mesh_ms), mean(&paint_ms));
        mesh_total += m;
        paint_total += p;
        println!(
            "lod {lod}: mesh {m:.3} ms, paint {p:.3} ms ({:.1} %), {:.1} strokes (most {most}) and {:.0} bytes a region, {:.2} strokes a bucket, fullest {fullest} of {}, {widened} widened, {dropped} lines left out",
            100.0 * p / m,
            per(strokes),
            per(bytes),
            per(listed) / 256.0,
            strata_terrain::paint::BUCKET_MOST,
        );
    }
    println!(
        "sums of the means: mesh {mesh_total:.3} ms, paint {paint_total:.3} ms ({:.1} %)",
        100.0 * paint_total / mesh_total
    );
}
