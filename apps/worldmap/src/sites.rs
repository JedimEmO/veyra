//! The `-sites` map (ticket 0005, step 4; ADR 0012): the procedural sites
//! of every land mass in the square over the `-rivers` map, each drawn by
//! its kind with its name, and Port Serein marked.
//!
//! A kind's symbol and colour come from its place in the placement order,
//! so a new kind file is drawn without a change here. Names are written in
//! a small built-in capital face with a dark halo, the higher ranks first;
//! a name that would overlap one already written is left out, and at the
//! wider scales only the first ranks are named at all. Where the map is
//! fine enough, each pad is drawn to scale with a tick the way it faces and
//! a dot where its road arrives.

use image::{ImageBuffer, Rgb};
use strata_worldgen::ProceduralWorld;
use strata_worldgen::sites::ProceduralSite;

use crate::rivers::Square;

/// The colours of the kinds in placement order, then round again.
const COLOURS: [[u8; 3]; 9] = [
    [200, 30, 40],
    [245, 150, 30],
    [170, 60, 210],
    [30, 200, 220],
    [150, 100, 50],
    [40, 170, 60],
    [240, 240, 240],
    [60, 90, 220],
    [250, 120, 170],
];

/// Draws the map on a copy of `base` (the `-rivers` map) and logs what it
/// drew.
pub fn sites_map(
    world: &ProceduralWorld,
    base: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    square: Square,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let networks = world.hydrology().bake_area(
        world,
        (square.cx - square.half, square.cz - square.half),
        (square.cx + square.half, square.cz + square.half),
    );
    let mut sites: Vec<&ProceduralSite> = networks
        .iter()
        .flat_map(|network| network.sites().iter())
        .filter(|site| {
            (site.centre[0] - square.cx).abs() <= square.half
                && (site.centre[1] - square.cz).abs() <= square.half
        })
        .collect();
    let kinds = world.site_kinds();
    let order = |site: &ProceduralSite| {
        kinds
            .iter()
            .position(|kind| kind.id == site.kind())
            .unwrap_or(0)
    };
    // Higher ranks last, so they draw over the rest; names go the other way.
    sites.sort_by_key(|site| std::cmp::Reverse(order(site)));
    let mut image = base.clone();
    let mut canvas = Canvas {
        image: &mut image,
        labels: Vec::new(),
    };

    // Port Serein first: its extent is outlined on the base already.
    for site in world.sites() {
        let (px, py) = square.pixel(site.centre[0], site.centre[1]);
        canvas.ring(px, py, 11.0, 2.0, [250, 215, 40]);
        canvas.label(px, py + 14.0, "PORT SEREIN", 2, [250, 215, 40], true);
    }

    let to_scale = square.metres_per_px <= 20.0;
    for site in &sites {
        let k = order(site);
        let colour = COLOURS[k % COLOURS.len()];
        let (px, py) = square.pixel(site.centre[0], site.centre[1]);
        if to_scale {
            let pad = site.radius_m / square.metres_per_px;
            let outer = site.outer_m() / square.metres_per_px;
            canvas.ring(px, py, outer, 0.6, [90, 90, 90]);
            canvas.ring(px, py, pad, 1.0, colour);
            let tick = (pad + 6.0).max(9.0);
            canvas.line(
                (px, py),
                (px + site.facing[0] * tick, py - site.facing[1] * tick),
                1.0,
                [255, 255, 255],
            );
            let (ax, ay) = square.pixel(site.arrival[0], site.arrival[1]);
            canvas.disc(ax, ay, 2.0, [20, 20, 20]);
            canvas.disc(ax, ay, 1.2, [255, 255, 120]);
        }
        let size = if k == 0 { 5.5 } else { 3.5 };
        canvas.symbol(k, px, py, size, colour);
    }
    // Names, the first ranks first; at wide scales only theirs.
    let named = if square.metres_per_px <= 20.0 {
        usize::MAX
    } else if square.metres_per_px <= 80.0 {
        4
    } else {
        1
    };
    sites.reverse();
    for site in &sites {
        let k = order(site);
        if k >= named {
            continue;
        }
        let (px, py) = square.pixel(site.centre[0], site.centre[1]);
        let colour = COLOURS[k % COLOURS.len()];
        let scale = if k == 0 && square.metres_per_px <= 80.0 {
            2
        } else {
            1
        };
        let light = [
            colour[0].saturating_add(60),
            colour[1].saturating_add(60),
            colour[2].saturating_add(60),
        ];
        canvas.label(px, py + 7.0, &site.name, scale, light, false);
    }
    canvas.legend(key(kinds).into_iter(), false);

    let mut counts = vec![0_usize; kinds.len()];
    for site in &sites {
        counts[order(site)] += 1;
    }
    let summary: Vec<String> = kinds
        .iter()
        .zip(&counts)
        .map(|(kind, count)| format!("{} {count}", kind.def.name))
        .collect();
    println!(
        "sites: {} in the square ({}); pads to scale {to_scale}",
        sites.len(),
        summary.join(", ")
    );
    image
}

/// The key's rows: each kind's place in the placement order, which is its
/// symbol, its name and its colour.
fn key(kinds: &[strata_worldgen::sites::SiteKind]) -> Vec<(usize, &str, [u8; 3])> {
    kinds
        .iter()
        .enumerate()
        .map(|(k, kind)| (k, kind.def.name.as_str(), COLOURS[k % COLOURS.len()]))
        .collect()
}

/// The image being drawn and the boxes of the names written so far.
pub(crate) struct Canvas<'a> {
    pub(crate) image: &'a mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    pub(crate) labels: Vec<[f64; 4]>,
}

impl Canvas<'_> {
    pub(crate) fn put(&mut self, x: f64, y: f64, colour: [u8; 3]) {
        if x < 0.0 || y < 0.0 {
            return;
        }
        // Checked against the image's size below; truncation is the pixel.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (px, py) = (x as u32, y as u32);
        if px < self.image.width() && py < self.image.height() {
            self.image.put_pixel(px, py, Rgb(colour));
        }
    }

    /// Every pixel whose centre passes `inside`, offset from `(cx, cy)`, in
    /// a box of `reach`.
    fn fill(
        &mut self,
        cx: f64,
        cy: f64,
        reach: f64,
        colour: [u8; 3],
        inside: impl Fn(f64, f64) -> bool,
    ) {
        let r = reach.ceil() + 1.0;
        let mut y = (cy - r).floor();
        while y <= cy + r {
            let mut x = (cx - r).floor();
            while x <= cx + r {
                if inside(x + 0.5 - cx, y + 0.5 - cy) {
                    self.put(x, y, colour);
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }

    pub(crate) fn disc(&mut self, cx: f64, cy: f64, radius: f64, colour: [u8; 3]) {
        self.fill(cx, cy, radius, colour, |dx, dy| {
            dx * dx + dy * dy <= radius * radius
        });
    }

    pub(crate) fn ring(&mut self, cx: f64, cy: f64, radius: f64, thickness: f64, colour: [u8; 3]) {
        self.fill(cx, cy, radius + thickness, colour, |dx, dy| {
            ((dx * dx + dy * dy).sqrt() - radius).abs() <= thickness * 0.5 + 0.25
        });
    }

    pub(crate) fn line(&mut self, from: (f64, f64), to: (f64, f64), radius: f64, colour: [u8; 3]) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let steps = ((dx * dx + dy * dy).sqrt() / 0.5).ceil().max(1.0);
        let mut i = 0.0;
        while i <= steps {
            let t = i / steps;
            self.disc(from.0 + dx * t, from.1 + dy * t, radius * 0.5, colour);
            i += 1.0;
        }
    }

    /// The kind's symbol, black-edged: a square, a disc, a triangle, a
    /// diamond, a chevron, a star, then round again.
    pub(crate) fn symbol(&mut self, kind: usize, cx: f64, cy: f64, size: f64, colour: [u8; 3]) {
        let shape = |s: f64| {
            move |dx: f64, dy: f64| match kind % 6 {
                0 => dx.abs() <= s && dy.abs() <= s,
                1 => dx * dx + dy * dy <= s * s,
                2 => dy <= s * 0.8 && dy >= -s && dx.abs() <= (dy + s) * 0.6,
                3 => dx.abs() + dy.abs() <= s * 1.2,
                4 => dy <= s && dy >= -s * 0.2 - 0.8 * (s - dx.abs()) && dy <= s - dx.abs() * 0.4,
                _ => {
                    let (ax, ay) = (dx.abs(), dy.abs());
                    (ax <= s * 0.35 && ay <= s * 1.1)
                        || (ay <= s * 0.35 && ax <= s * 1.1)
                        || ax + ay <= s * 0.9
                }
            }
        };
        self.fill(cx, cy, size + 2.0, [15, 15, 15], shape(size + 1.3));
        self.fill(cx, cy, size + 1.0, colour, shape(size));
    }

    /// `text` centred under `(cx, top)`, `scale` pixels a dot, with a dark
    /// halo; left out where it would cover a name already written, unless
    /// `force`.
    #[allow(clippy::cast_precision_loss, reason = "a label is a few dozen glyphs")]
    pub(crate) fn label(
        &mut self,
        cx: f64,
        top: f64,
        text: &str,
        scale: u32,
        colour: [u8; 3],
        force: bool,
    ) {
        let s = f64::from(scale);
        let width = text.chars().count() as f64 * 6.0 * s - s;
        let (x0, y0) = ((cx - width * 0.5).round(), top.round());
        let rect = [x0 - 2.0, y0 - 2.0, x0 + width + 2.0, y0 + 7.0 * s + 2.0];
        let overlaps = self
            .labels
            .iter()
            .any(|r| rect[0] < r[2] && r[0] < rect[2] && rect[1] < r[3] && r[1] < rect[3]);
        if overlaps && !force {
            return;
        }
        self.labels.push(rect);
        for pass in 0..2 {
            for (i, ch) in text.chars().enumerate() {
                let rows = glyph(ch);
                let gx = x0 + i as f64 * 6.0 * s;
                for (row, bits) in rows.iter().enumerate() {
                    for col in 0..5_u32 {
                        if bits & (0b1_0000 >> col) == 0 {
                            continue;
                        }
                        let (px, py) = (gx + f64::from(col) * s, y0 + row as f64 * s);
                        for oy in 0..scale {
                            for ox in 0..scale {
                                let (x, y) = (px + f64::from(ox), py + f64::from(oy));
                                if pass == 0 {
                                    for (hx, hy) in
                                        [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
                                    {
                                        self.put(x + hx, y + hy, [10, 10, 10]);
                                    }
                                } else {
                                    self.put(x, y, colour);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// A key in a top corner, the left or, `right`, the right: a row an
    /// entry, its symbol (by its index), its colour and its name. Returns
    /// where each row's symbol was drawn, so a caller can mark one further.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a handful of kinds with short names"
    )]
    pub(crate) fn legend<'a>(
        &mut self,
        entries: impl Iterator<Item = (usize, &'a str, [u8; 3])>,
        right: bool,
    ) -> Vec<(f64, f64)> {
        let entries: Vec<(usize, &str, [u8; 3])> = entries.collect();
        let height = entries.len() as f64 * 14.0 + 10.0;
        let width = entries.iter().map(|(_, n, _)| n.len()).max().unwrap_or(0);
        let width = width as f64 * 6.0 + 34.0;
        let x0 = if right {
            (f64::from(self.image.width()) - width - 1.0).max(0.0)
        } else {
            0.0
        };
        self.fill(x0, 0.0, width.max(height), [20, 20, 24], |dx, dy| {
            dx >= 0.0 && dy >= 0.0 && dx <= width && dy <= height
        });
        let mut marks = Vec::with_capacity(entries.len());
        let kept = std::mem::take(&mut self.labels);
        for (row, (symbol, name, colour)) in entries.iter().enumerate() {
            let y = 12.0 + row as f64 * 14.0;
            self.symbol(*symbol, x0 + 12.0, y, 3.5, *colour);
            marks.push((x0 + 12.0, y));
            self.labels.clear();
            self.label(
                x0 + 24.0 + name.len() as f64 * 3.0,
                y - 3.0,
                name,
                1,
                [230, 230, 230],
                true,
            );
        }
        // A second key keeps the first one's box clear too.
        self.labels = if right { kept } else { Vec::new() };
        self.labels.push([x0, 0.0, x0 + width, height]);
        marks
    }
}

/// A 5 by 7 capital, each row's five dots in the low bits, the left one
/// highest. Small letters are drawn as capitals.
fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        '\'' => [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x0c],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0c, 0x04, 0x08],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        ':' => [0x00, 0x0c, 0x0c, 0x00, 0x0c, 0x0c, 0x00],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        _ => [0x00; 7],
    }
}
