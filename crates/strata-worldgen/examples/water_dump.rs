//! Prints what the river cache answers, for comparing two ways of getting
//! the same cache (a bake, the per-mass files of one layout, the files of
//! another):
//! `cargo run --release -p strata-worldgen --example water_dump -- <dir> <out.txt> [points]`
//! where `<dir>` is a `generated/hydrology`, `-` or `bake=<dir>`.
//!
//! With a directory every mass its index names is read from its
//! `water.index` and `mass/` files and the run says how many merged, so a
//! file that was refused (and quietly baked instead) cannot pass for a match.
//! With `-` the same square is baked. The output is one line a mass (its
//! content hash) and one a sample point (the whole `Sample` and the water
//! record, `Debug`, which prints floats shortest-round-trip, so equal text is
//! equal bits), over a grid, over the home mass's road points and over its
//! river vertices.

use std::fmt::Write as _;

/// A line of output; a string cannot fail to take one.
macro_rules! line {
    ($text:expr, $($args:tt)*) => {
        let _ = writeln!($text, $($args)*);
    };
}

use strata_core::Seed;
use strata_worldgen::hydrology::{MAX_THREADS, MassLoad, WEB_PREPARE_RADIUS_M, WaterIndex};
use strata_worldgen::{ProceduralWorld, WorldParams, WorldSource};

#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a script: modes, then one pass of output; a random fraction of 53 bits"
)]
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("a directory or -");
    let out = args.next().expect("an output path");
    let count: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(400);
    let seed = Seed(42);
    let params = WorldParams::default();
    let registries =
        strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::with_params(seed, &registries, params);
    let stamp = strata_worldgen::version::water_stamp(seed, &params, &registries);
    let mut text = String::new();
    if let Some(target) = dir.strip_prefix("bake=") {
        // Bake the square and write it as the layout this build writes.
        let masses = world.prepare_water(0.0, 0.0, WEB_PREPARE_RADIUS_M);
        let files = world.hydrology().save_masses(stamp, |_| true);
        let root = std::path::Path::new(target);
        std::fs::create_dir_all(root.join("mass")).expect("a directory");
        let mut entries = Vec::new();
        let mut total = 0;
        for (entry, bytes) in files {
            let name = entry.path();
            let name = name.trim_start_matches("generated/hydrology/");
            std::fs::write(root.join(name), &bytes).expect("a mass file");
            total += bytes.len();
            entries.push(entry);
        }
        std::fs::write(
            root.join("water.index"),
            WaterIndex::new(seed, stamp, entries).to_bytes(),
        )
        .expect("an index");
        line!(text, "baked {masses}, wrote {total} bytes");
    } else if dir == "-" {
        let masses = world.prepare_water(0.0, 0.0, WEB_PREPARE_RADIUS_M);
        line!(text, "baked {masses}");
    } else {
        let root = std::path::Path::new(&dir);
        let bytes = std::fs::read(root.join("water.index")).expect("an index");
        let index = WaterIndex::read(&bytes, seed, stamp).expect("an index of this world");
        let (mut merged, mut other) = (0, 0);
        for entry in &index.masses {
            let name = entry.path();
            let name = name.trim_start_matches("generated/hydrology/");
            let bytes = std::fs::read(root.join(name)).expect("a mass file");
            match world
                .hydrology()
                .load_mass(&world, &bytes, stamp, MAX_THREADS)
            {
                Ok(MassLoad::Merged) => merged += 1,
                other_result => {
                    other += 1;
                    eprintln!("{:?}: {other_result:?}", entry.key);
                }
            }
        }
        line!(text, "merged {merged} other {other}");
        assert_eq!(other, 0, "every file must merge");
    }
    // The meet-everything pass: a sample near the origin meets masses the
    // file read did not, which bake as they always did.
    let mut networks = world.hydrology().networks();
    networks.sort_by_key(|n| (n.key(), n.capped()));
    for network in &networks {
        line!(
            text,
            "mass {:?} {} hash {:016x} rivers {} roads {} sites {}",
            network.key(),
            network.capped(),
            network.content_hash(),
            network.rivers().len(),
            network.roads().len(),
            network.sites().len(),
        );
    }
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1_u64 << 53) as f64
    };
    for _ in 0..count {
        points.push(((next() - 0.5) * 80_000.0, (next() - 0.5) * 80_000.0));
    }
    // Along the biggest mass's roads and rivers, where a changed record shows.
    if let Some(home) = networks.iter().max_by_key(|n| n.roads().len()) {
        for road in home.roads().iter().step_by(37) {
            for p in road.points.iter().step_by(23).take(6) {
                points.push((p.x + 3.0, p.z - 2.0));
            }
        }
        for river in home.rivers().iter().step_by(41) {
            for v in river.vertices.iter().step_by(5).take(4) {
                points.push((v.x + 1.5, v.z + 0.5));
            }
        }
    }
    for (x, z) in points {
        line!(text, "{x} {z} {:?}", world.sample(x, z));
        line!(text, "{x} {z} water {:?}", world.water_at(x, z));
    }
    std::fs::write(&out, text).expect("the output is written");
}
