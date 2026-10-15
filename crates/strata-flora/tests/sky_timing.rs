//! The placer's time per cell on the real seed-42 world, for the lighting
//! plan's phase 3a ceiling (the sky term may cost the placer 10 percent).
//! Ignored: a timing, not a check, and only meaningful in a release build
//! on an idle machine:
//!
//! ```text
//! cargo test --release -p strata-flora --test sky_timing -- --ignored --nocapture
//! ```

mod common;

use std::time::Instant;

use strata_core::{RegionCoord, Seed};
use strata_flora::{place_cell_with, sky_terms};

#[test]
#[ignore = "a timing; run in release on an idle machine"]
fn placer_time_per_cell() {
    let (world, table) = common::world_and_table(42);
    let mut scratch = Vec::new();
    let mut cells = Vec::new();
    for grass in [false, true] {
        let mut times = Vec::new();
        let mut plants = 0;
        let mut blades = 0;
        // The canopy fixture's woodland (`study::SCENES`, x -7500, z 21500).
        for z in 330..342 {
            for x in -123..-111 {
                let coord = RegionCoord::new(x, z, 0);
                let started = Instant::now();
                let cell = place_cell_with(coord, &world, &table, Seed(42), &mut scratch, grass);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                plants += cell.plants.len();
                blades += cell.grass.len();
                if !grass {
                    cells.push(cell);
                }
            }
        }
        times.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss, reason = "a count of 144")]
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "grass {grass}: mean {mean:.3} ms, median {:.3} ms over {} cells, {plants} plants, {blades} blades",
            times[times.len() / 2],
            times.len()
        );
    }
    // The plants' sky terms alone (lighting plan phase 3a), each cell's
    // plants as their own neighbours: the part of the placer's time that
    // is the term, without the noise of the whole placement.
    let started = Instant::now();
    for _ in 0..10 {
        for cell in &mut cells {
            let neighbours = cell.plants.clone();
            sky_terms(&mut cell.plants, &neighbours, &table);
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "a count of 144")]
    let per_cell = started.elapsed().as_secs_f64() * 1000.0 / (10.0 * cells.len() as f64);
    println!("sky terms alone: {per_cell:.4} ms a cell");
}
