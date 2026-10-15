//! The `-rivers` map (ticket 0005, step 1): the river network of every land
//! mass in the square over the hillshade. Rivers are drawn by discharge,
//! wider and darker as they grow and never thinner than a pixel, so a creek
//! a few metres wide still reads at a hundred metres a pixel; lakes are
//! filled, falls marked by a ring the size of their drop, the borders of a
//! cut mass's cells drawn where one is in view, and Port Serein's extent
//! outlined in yellow.

use std::sync::Arc;

use image::{ImageBuffer, Rgb};
use rayon::prelude::*;
use strata_worldgen::hydrology::{CAP_CELL_NODES, LATTICE_M, Network, WaterKind};
use strata_worldgen::{ProceduralWorld, Sample, WorldSource};

/// Where the map lies: its centre, half its side and its metres a pixel.
#[derive(Debug, Clone, Copy)]
pub struct Square {
    pub cx: f64,
    pub cz: f64,
    pub half: f64,
    pub metres_per_px: f64,
}

impl Square {
    /// A world point in pixels, `x` east and `y` down the image.
    pub fn pixel(self, x: f64, z: f64) -> (f64, f64) {
        (
            (x - (self.cx - self.half)) / self.metres_per_px,
            ((self.cz + self.half) - z) / self.metres_per_px,
        )
    }

    /// A pixel's centre in world metres.
    fn world(self, px: u32, py: u32) -> (f64, f64) {
        (
            self.cx - self.half + (f64::from(px) + 0.5) * self.metres_per_px,
            self.cz + self.half - (f64::from(py) + 0.5) * self.metres_per_px,
        )
    }
}

/// Bakes every mass in the square on `threads` threads, logs what it found
/// and paints the map.
pub fn rivers_map(
    world: &ProceduralWorld,
    rows: &[Vec<Sample>],
    square: Square,
    exaggeration: f64,
    threads: usize,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let hydrology = world.hydrology();
    hydrology.set_threads(threads);
    let started = std::time::Instant::now();
    let networks = hydrology.bake_area(
        world,
        (square.cx - square.half, square.cz - square.half),
        (square.cx + square.half, square.cz + square.half),
    );
    log(&networks, started.elapsed().as_secs_f64());
    paint(world, rows, square, exaggeration, &networks)
}

fn log(networks: &[Arc<Network>], seconds: f64) {
    let largest = networks.iter().max_by_key(|n| n.stats().nodes);
    let km2 = |nodes: usize| {
        // Node counts are far under 2^53.
        #[allow(clippy::cast_precision_loss)]
        let nodes = nodes as f64;
        nodes * LATTICE_M * LATTICE_M / 1e6
    };
    println!(
        "rivers: {} land masses under the map baked in {seconds:.2} s, {} capped; {} reaches, {} lakes, {} falls",
        networks.len(),
        networks.iter().filter(|n| n.capped()).count(),
        networks.iter().map(|n| n.rivers().len()).sum::<usize>(),
        networks.iter().map(|n| n.lakes().len()).sum::<usize>(),
        networks.iter().map(|n| n.falls().len()).sum::<usize>(),
    );
    if let Some(n) = largest {
        println!(
            "rivers: the largest mass {:?}, {} nodes ({:.0} km2), its own bake {:.2} s; rivers by discharge (light 0.25 m3/s and under to deep blue 5 and over), lakes filled, falls white and ringed red, Port Serein yellow",
            n.key(),
            n.stats().nodes,
            km2(n.stats().nodes),
            n.stats().seconds
        );
    }
}

/// Blue for a discharge: light at the threshold, deep from 5 m3/s.
fn river_colour(discharge: f64) -> [f64; 3] {
    let t = ((discharge.max(0.0).sqrt() - 0.5) / (5.0_f64.sqrt() - 0.5)).clamp(0.0, 1.0);
    let light = [110.0, 190.0, 250.0];
    let deep = [10.0, 45.0, 160.0];
    [0, 1, 2].map(|c| light[c] + (deep[c] - light[c]) * t)
}

#[allow(clippy::too_many_lines, reason = "the layers in drawing order")]
fn paint(
    world: &ProceduralWorld,
    rows: &[Vec<Sample>],
    square: Square,
    exaggeration: f64,
    networks: &[Arc<Network>],
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let sea = world.sea_level();
    // Rows came from `0..n` with `n: u32`.
    #[allow(clippy::cast_possible_truncation)]
    let n = rows.len() as u32;
    // The hillshade, a touch warm, and the lakes from the query.
    let pixels: Vec<Vec<[f64; 3]>> = (0..n)
        .into_par_iter()
        .map(|py| {
            (0..n)
                .map(|px| {
                    let sample = &rows[py as usize][px as usize];
                    if sample.is_submerged(sea) {
                        return [44.0, 62.0, 86.0];
                    }
                    let (x, z) = square.world(px, py);
                    if let Some(water) = world.water_at(x, z)
                        && water.kind == WaterKind::Lake
                    {
                        return [70.0, 140.0, 200.0];
                    }
                    let lit = super::sunlight(
                        rows,
                        (px as usize, py as usize),
                        sea,
                        square.metres_per_px,
                        exaggeration,
                    );
                    let grey = 40.0 + 190.0 * lit;
                    [grey * 1.02, grey, grey * 0.94]
                })
                .collect()
        })
        .collect();
    let mut canvas = Canvas {
        pixels,
        side: f64::from(n),
    };

    // Rivers, small first so the main stems lie on top: each channel's
    // centreline as it winds about its reach's line (ticket 0005, step
    // 3b), a fall over it from its vertices.
    let mut reaches: Vec<(f64, &strata_worldgen::hydrology::River)> = networks
        .iter()
        .flat_map(|net| net.rivers().iter())
        .map(|river| {
            let q = river
                .vertices
                .last()
                .map_or(0.0, |v| f64::from(v.discharge_m3s));
            (q, river)
        })
        .collect();
    reaches.sort_by(|a, b| a.0.total_cmp(&b.0));
    let params = strata_worldgen::hydrology::RiverParams::DEFAULT;
    let step = (square.metres_per_px * 0.75).max(1.0);
    for (q, river) in reaches {
        let colour = river_colour(q);
        let points = river.centreline(&params, step);
        for pair in points.windows(2) {
            let width_px = pair[0].width_m / square.metres_per_px;
            // The root of the discharge, from the width (`4.5 Q^0.5`).
            let root = pair[0].width_m / params.width_coefficient;
            let radius = (0.55 + 0.45 * root).max(width_px * 0.5).min(8.0);
            let a = square.pixel(pair[0].x, pair[0].z);
            let b = square.pixel(pair[1].x, pair[1].z);
            canvas.line(a, b, radius, colour);
        }
        for pair in river.vertices.windows(2).filter(|pair| pair[0].fall) {
            let q = f64::from(pair[0].discharge_m3s);
            let width_px = f64::from(pair[0].width_m) / square.metres_per_px;
            let radius = (0.55 + 0.45 * q.sqrt()).max(width_px * 0.5).min(8.0);
            let a = square.pixel(pair[0].x, pair[0].z);
            let b = square.pixel(pair[1].x, pair[1].z);
            canvas.line(a, b, radius, [230.0, 240.0, 255.0]);
        }
    }

    // Falls: a red ring, wider for a taller drop.
    for net in networks {
        for fall in net.falls() {
            let drop = fall.top[1] - fall.bottom[1];
            let centre = square.pixel(
                f64::midpoint(fall.top[0], fall.bottom[0]),
                f64::midpoint(fall.top[2], fall.bottom[2]),
            );
            let radius = 2.5 + (drop / 40.0).min(6.0);
            canvas.ring(centre, radius, 1.2, [230.0, 40.0, 30.0]);
        }
    }

    // A cut mass's cells: their borders, magenta, where one is in view.
    if networks.iter().any(|net| net.capped()) {
        let cell = f64::from(CAP_CELL_NODES) * LATTICE_M;
        let lo = ((square.cx - square.half) / cell).floor();
        let hi = ((square.cx + square.half) / cell).ceil();
        let mut k = lo;
        while k <= hi {
            let x = k * cell;
            let (px, _) = square.pixel(x, 0.0);
            canvas.line((px, 0.0), (px, canvas.side), 0.8, [220.0, 40.0, 220.0]);
            k += 1.0;
        }
        let lo = ((square.cz - square.half) / cell).floor();
        let hi = ((square.cz + square.half) / cell).ceil();
        let mut k = lo;
        while k <= hi {
            let z = k * cell;
            let (_, py) = square.pixel(0.0, z);
            canvas.line((0.0, py), (canvas.side, py), 0.8, [220.0, 40.0, 220.0]);
            k += 1.0;
        }
        println!("rivers: cut cells' borders magenta (forced divides)");
    }

    // Port Serein: its extent's outline, and a ring round it at small
    // scales so it is found.
    for site in world.sites() {
        let band = square.metres_per_px.max(4.0);
        for py in 0..n {
            for px in 0..n {
                let (x, z) = square.world(px, py);
                let d = site.extent_distance_at(x, z);
                if d <= 0.0 && d > -1.5 * band {
                    canvas.set(px, py, [250.0, 215.0, 40.0]);
                }
            }
        }
        let centre = square.pixel(site.centre[0], site.centre[1]);
        canvas.ring(centre, 9.0, 1.5, [250.0, 215.0, 40.0]);
    }
    canvas.image()
}

/// The map being drawn, in floats.
struct Canvas {
    pixels: Vec<Vec<[f64; 3]>>,
    side: f64,
}

impl Canvas {
    fn set(&mut self, px: u32, py: u32, colour: [f64; 3]) {
        self.pixels[py as usize][px as usize] = colour;
    }

    /// Covers every pixel whose centre is within `radius` of `centre` and
    /// whose distance passes `keep`.
    fn stamp(
        &mut self,
        centre: (f64, f64),
        radius: f64,
        colour: [f64; 3],
        keep: impl Fn(f64) -> bool,
    ) {
        let reach = radius.ceil() + 1.0;
        let (x0, x1) = (
            (centre.0 - reach).max(0.0),
            (centre.0 + reach).min(self.side - 1.0),
        );
        let (y0, y1) = (
            (centre.1 - reach).max(0.0),
            (centre.1 + reach).min(self.side - 1.0),
        );
        if x0 > x1 || y0 > y1 {
            return;
        }
        // Clamped into the image above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x0, x1, y0, y1) = (x0 as u32, x1 as u32, y0 as u32, y1 as u32);
        for py in y0..=y1 {
            for px in x0..=x1 {
                let (dx, dy) = (
                    f64::from(px) + 0.5 - centre.0,
                    f64::from(py) + 0.5 - centre.1,
                );
                if keep((dx * dx + dy * dy).sqrt()) {
                    self.set(px, py, colour);
                }
            }
        }
    }

    /// A line of discs from `a` to `b`.
    fn line(&mut self, from: (f64, f64), to: (f64, f64), radius: f64, colour: [f64; 3]) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = (dx * dx + dy * dy).sqrt();
        let steps = (length / 0.5).ceil().max(1.0);
        let reach = radius.max(0.5);
        let mut step = 0.0;
        while step <= steps {
            let along = step / steps;
            self.stamp(
                (from.0 + dx * along, from.1 + dy * along),
                reach,
                colour,
                |d| d <= reach,
            );
            step += 1.0;
        }
    }

    /// A ring of `thickness` at `radius`.
    fn ring(&mut self, centre: (f64, f64), radius: f64, thickness: f64, colour: [f64; 3]) {
        self.stamp(centre, radius + thickness, colour, |d| {
            (d - radius).abs() <= thickness
        });
    }

    fn image(&self) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        // The canvas's side came from a u32.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = self.side as u32;
        let mut image = ImageBuffer::new(n, n);
        for (py, row) in self.pixels.iter().enumerate() {
            for (px, c) in row.iter().enumerate() {
                // Pixel indices came from `0..n`.
                #[allow(clippy::cast_possible_truncation)]
                let (ix, iy) = (px as u32, py as u32);
                #[allow(clippy::cast_possible_truncation)]
                let rgb = c.map(|v| super::to_byte((v / 255.0) as f32));
                image.put_pixel(ix, iy, Rgb(rgb));
            }
        }
        image
    }
}
