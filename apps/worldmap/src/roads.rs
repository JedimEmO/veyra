//! The `-roads` map (ticket 0005, step 5; ADR 0012, decision 7): the road
//! network of every land mass in the square over the `-rivers` map, with
//! the sites drawn over it.
//!
//! Service roads are wide and red with a dark casing, tracks narrower and
//! amber, trails thin, white and dashed, as a road map draws them, so the
//! backbone reads first and the paths last. Junctions are small dark dots
//! where the map is fine enough to tell them from a bend; crossings are a
//! bridge's black bar across the road, a ford's blue diamond and a culvert's
//! small grey square.

use image::{ImageBuffer, Rgb};
use strata_worldgen::ProceduralWorld;
use strata_worldgen::roads::{CrossingKind, Road, RoadClass};

use crate::rivers::Square;
use crate::sites::Canvas;

/// Draws the roads on a copy of `base` and logs what it drew.
#[allow(
    clippy::too_many_lines,
    reason = "each class, the junctions, the crossings and the legend, in order"
)]
pub fn roads_map(
    world: &ProceduralWorld,
    base: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    square: Square,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let networks = world.hydrology().bake_area(
        world,
        (square.cx - square.half, square.cz - square.half),
        (square.cx + square.half, square.cz + square.half),
    );
    let mut image = base.clone();
    let mut canvas = Canvas {
        image: &mut image,
        labels: Vec::new(),
    };
    let fine = square.metres_per_px <= 30.0;
    let wide = square.metres_per_px > 100.0;
    let inside = |x: f64, z: f64| {
        (x - square.cx).abs() <= square.half + 500.0 && (z - square.cz).abs() <= square.half + 500.0
    };
    // Faintest first, so the backbone draws over its branches.
    let mut drawn = [0.0_f64; 3];
    for (k, class) in [RoadClass::Trail, RoadClass::Track, RoadClass::Service]
        .into_iter()
        .enumerate()
    {
        let (casing, width, colour) = match class {
            RoadClass::Service => (
                if wide { 2.6 } else { 4.2 },
                if wide { 1.6 } else { 2.6 },
                [225, 45, 35],
            ),
            RoadClass::Track => (
                if wide { 1.8 } else { 2.8 },
                if wide { 1.0 } else { 1.6 },
                [245, 175, 40],
            ),
            RoadClass::Trail => (0.0, if wide { 0.8 } else { 1.1 }, [250, 250, 250]),
        };
        for pass in 0..2 {
            if pass == 0 && casing <= 0.0 {
                continue;
            }
            for network in &networks {
                for road in network.roads().iter().filter(|r| r.class == class) {
                    if !road.points.iter().any(|p| inside(p.x, p.z)) {
                        continue;
                    }
                    let length = draw(&mut canvas, road, square, |c, a, b, along| {
                        if pass == 0 {
                            c.line(a, b, casing, [25, 20, 20]);
                        } else if class == RoadClass::Trail {
                            // Dashes of 6 px, gaps of 4.
                            if (along / square.metres_per_px) % 10.0 < 6.0 {
                                c.line(a, b, width, colour);
                            }
                        } else {
                            c.line(a, b, width, colour);
                        }
                    });
                    if pass == 1 {
                        drawn[k] += length;
                    }
                }
            }
        }
    }
    let mut junctions = 0;
    let mut crossings = [0_usize; 3];
    for network in &networks {
        if fine {
            for j in network.junctions() {
                if inside(j.at[0], j.at[1]) {
                    let (px, py) = square.pixel(j.at[0], j.at[1]);
                    canvas.disc(px, py, 2.6, [20, 20, 20]);
                    canvas.disc(px, py, 1.3, [255, 255, 255]);
                    junctions += 1;
                }
            }
        }
        for road in network.roads() {
            for c in &road.crossings {
                if !inside(c.at[0], c.at[1]) {
                    continue;
                }
                let (px, py) = square.pixel(c.at[0], c.at[1]);
                let s = if wide { 2.0 } else { 3.5 };
                match c.kind {
                    CrossingKind::Bridge => {
                        crossings[0] += 1;
                        // A bar across the road, the span to scale where it
                        // is wider than the symbol.
                        let half = (0.5 * c.span_m / square.metres_per_px).max(s);
                        let (bx, by) = (c.bearing[0], -c.bearing[1]);
                        let (nx, ny) = (-by, bx);
                        for side in [-1.0, 1.0] {
                            canvas.line(
                                (
                                    px - bx * half + nx * side * s,
                                    py - by * half + ny * side * s,
                                ),
                                (
                                    px + bx * half + nx * side * s,
                                    py + by * half + ny * side * s,
                                ),
                                1.6,
                                [10, 10, 10],
                            );
                        }
                    }
                    CrossingKind::Ford => {
                        crossings[1] += 1;
                        // A diamond, black-edged, row by row.
                        for (r, colour) in [(s + 1.2, [10, 10, 10]), (s, [60, 150, 255])] {
                            let mut y = -r.ceil();
                            while y <= r.ceil() {
                                let half = r - y.abs();
                                if half >= 0.0 {
                                    canvas.line(
                                        (px - half, py + y),
                                        (px + half, py + y),
                                        1.0,
                                        colour,
                                    );
                                }
                                y += 1.0;
                            }
                        }
                    }
                    CrossingKind::Culvert => {
                        crossings[2] += 1;
                        if !wide {
                            canvas.disc(px, py, 2.0, [10, 10, 10]);
                            canvas.disc(px, py, 1.1, [170, 170, 170]);
                        }
                    }
                }
            }
        }
    }
    println!(
        "roads: service {:.0} km, track {:.0} km, trail {:.0} km drawn; {junctions} junctions; bridges {}, fords {}, culverts {}",
        drawn[2] / 1e3,
        drawn[1] / 1e3,
        drawn[0] / 1e3,
        crossings[0],
        crossings[1],
        crossings[2]
    );
    image
}

/// Draws a road's line through `segment`, skipping points closer than a
/// pixel apart, and returns the metres of it inside the map.
fn draw(
    canvas: &mut Canvas<'_>,
    road: &Road,
    square: Square,
    mut segment: impl FnMut(&mut Canvas<'_>, (f64, f64), (f64, f64), f64),
) -> f64 {
    let mut length = 0.0;
    let Some(first) = road.points.first() else {
        return 0.0;
    };
    let mut last = square.pixel(first.x, first.z);
    let mut last_along = first.along_m;
    for p in &road.points[1..] {
        let here = square.pixel(p.x, p.z);
        let (dx, dy) = (here.0 - last.0, here.1 - last.1);
        if dx * dx + dy * dy < 0.8 && p.along_m < road.length_m() {
            continue;
        }
        let on = |q: (f64, f64)| {
            q.0 >= 0.0
                && q.1 >= 0.0
                && q.0 < f64::from(canvas.image.width())
                && q.1 < f64::from(canvas.image.height())
        };
        if on(last) || on(here) {
            segment(canvas, last, here, p.along_m);
            length += p.along_m - last_along;
        }
        last = here;
        last_along = p.along_m;
    }
    length
}

/// A key in the bottom right, clear of the sites' key: each class's line and each crossing's symbol.
/// Draws the key on `image`, once the sites are drawn over the roads, so
/// none of their symbols lands on it.
pub fn legend(image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, square: Square) {
    let mut canvas = Canvas {
        image,
        labels: Vec::new(),
    };
    key(&mut canvas, square.metres_per_px > 100.0);
}

#[allow(clippy::cast_precision_loss, reason = "a label of a dozen glyphs")]
fn key(canvas: &mut Canvas<'_>, wide: bool) {
    let (w, h) = (
        f64::from(canvas.image.width()),
        f64::from(canvas.image.height()),
    );
    let (x0, y0) = (w - 124.0, h - 74.0);
    canvas.labels.clear();
    for y in 0..70 {
        canvas.line(
            (x0 - 4.0, y0 - 4.0 + f64::from(y)),
            (x0 + 118.0, y0 - 4.0 + f64::from(y)),
            1.0,
            [20, 20, 24],
        );
    }
    let rows: [(&str, [u8; 3], f64); 3] = [
        ("SERVICE ROAD", [225, 45, 35], 2.6),
        ("TRACK", [245, 175, 40], 1.6),
        ("TRAIL", [250, 250, 250], 1.1),
    ];
    for (k, (name, colour, width)) in rows.iter().enumerate() {
        let y = y0 + 6.0 + 14.0 * f64::from(u8::try_from(k).unwrap_or(0));
        canvas.line((x0, y), (x0 + 22.0, y), *width + 1.4, [25, 20, 20]);
        canvas.line((x0, y), (x0 + 22.0, y), *width, *colour);
        canvas.label(
            x0 + 30.0 + 3.0 * name.len() as f64,
            y - 3.0,
            name,
            1,
            [230, 230, 230],
            true,
        );
    }
    let y = y0 + 48.0;
    canvas.line((x0 + 2.0, y - 3.0), (x0 + 14.0, y - 3.0), 1.6, [10, 10, 10]);
    canvas.line((x0 + 2.0, y + 3.0), (x0 + 14.0, y + 3.0), 1.6, [10, 10, 10]);
    canvas.disc(x0 + 30.0, y, 3.0, [60, 150, 255]);
    if !wide {
        canvas.disc(x0 + 48.0, y, 2.0, [170, 170, 170]);
    }
    canvas.label(x0 + 90.0, y - 3.0, "BR FD CV", 1, [230, 230, 230], true);
}
