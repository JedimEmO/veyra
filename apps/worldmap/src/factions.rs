//! The `-factions` map (ticket 0012, step 1): whose ground it is, over the
//! `-shade` map.
//!
//! Each pixel is washed in its holder's colour, stronger near the holding
//! and thin at the edge of its reach, so the shade still reads under it;
//! contested ground is hatched dark, a border line runs where the holder
//! changes, every site is a dot in its holder's colour (grey for a kind no
//! one holds), and a faction is named at its seat. The run says on the
//! standard output who the factions are, so a world's politics can be read
//! without the picture.

use std::collections::BTreeMap;

use image::{ImageBuffer, Rgb};
use rayon::prelude::*;
use strata_worldgen::sites::ProceduralSite;
use veyra_world::factions::{Faction, FactionId, FactionMap};

use crate::rivers::Square;
use crate::sites::Canvas;

/// The least opacity of a holder's wash where it only just reaches.
const WASH_FLOOR: f64 = 0.10;
/// The most opacity of a wash, so the shade under the strongest still reads.
const WASH_CEIL: f64 = 0.50;
/// The share of the colour contested ground keeps in its dark stripes.
const HATCH_KEEP: f64 = 0.35;
/// The stripes of contested ground: one in this many diagonal pixels.
const HATCH_PERIOD: u32 = 6;
/// A site's dot, pixels.
const DOT_PX: f64 = 3.0;
/// The most factions the key lists.
const KEY_ROWS: usize = 14;
/// The tone of a site no faction holds.
const UNHELD: [u8; 3] = [150, 150, 150];

/// A channel mixed from bytes, back to a byte.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a byte first"
)]
fn byte(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// The map's holder, its strength and whether it is contested, per pixel.
fn grid(map: &FactionMap, square: Square, size: u32) -> Vec<(Option<FactionId>, f64, bool)> {
    let reach = square.half * core::f64::consts::SQRT_2 + map.max_reach_m();
    let holdings = map.holdings_near(square.cx, square.cz, reach);
    (0..size)
        .into_par_iter()
        .flat_map_iter(|py| {
            let holdings = &holdings;
            (0..size).map(move |px| {
                let x = square.cx - square.half + (f64::from(px) + 0.5) * square.metres_per_px;
                let z = square.cz + square.half - (f64::from(py) + 0.5) * square.metres_per_px;
                let t = map.territory_among(holdings, x, z);
                (t.holder, t.strength, t.contested)
            })
        })
        .collect()
}

/// Washes each pixel in its holder's colour and draws the borders.
fn territory(
    image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    cells: &[(Option<FactionId>, f64, bool)],
    factions: &BTreeMap<FactionId, Faction>,
) {
    let size = image.width();
    let at = |x: u32, y: u32| &cells[(y * size + x) as usize];
    for y in 0..size {
        for x in 0..size {
            let &(holder, strength, contested) = at(x, y);
            let Some(faction) = holder.and_then(|id| factions.get(&id)) else {
                continue;
            };
            let alpha = WASH_FLOOR + (WASH_CEIL - WASH_FLOOR) * strength.clamp(0.0, 1.0);
            let mut colour = faction.colour;
            if contested && (x + y) % HATCH_PERIOD < HATCH_PERIOD / 2 {
                colour = colour.map(|c| byte(f64::from(c) * HATCH_KEEP));
            }
            let pixel = image.get_pixel_mut(x, y);
            for (held, wash) in pixel.0.iter_mut().zip(colour) {
                let mixed = f64::from(*held) * (1.0 - alpha) + f64::from(wash) * alpha;
                *held = byte(mixed);
            }
        }
    }
    // A border where the holder changes, in the stronger side's colour.
    for y in 0..size.saturating_sub(1) {
        for x in 0..size.saturating_sub(1) {
            let here = at(x, y).0;
            if here != at(x + 1, y).0 || here != at(x, y + 1).0 {
                if let Some(faction) = here.and_then(|id| factions.get(&id)) {
                    image.put_pixel(x, y, Rgb(faction.colour.map(|c| c.saturating_sub(40))));
                } else if let Some(faction) = at(x + 1, y).0.and_then(|id| factions.get(&id)) {
                    image.put_pixel(x, y, Rgb(faction.colour.map(|c| c.saturating_sub(40))));
                }
            }
        }
    }
}

/// Draws the map on a copy of `base` and logs who the factions are.
#[allow(clippy::too_many_lines, reason = "one picture, drawn layer by layer")]
pub fn factions_map(
    map: &FactionMap,
    base: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    square: Square,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let size = base.width();
    let reach = square.half * core::f64::consts::SQRT_2 + map.max_reach_m();
    let factions: BTreeMap<FactionId, _> = map
        .factions_near(square.cx, square.cz, reach)
        .into_iter()
        .map(|faction| (faction.id, faction))
        .collect();
    let cells = grid(map, square, size);
    let mut image = base.clone();
    territory(&mut image, &cells, &factions);
    let mut canvas = Canvas {
        image: &mut image,
        labels: Vec::new(),
    };
    let world = map.world();
    let networks = world.hydrology().bake_area(
        world,
        (square.cx - square.half, square.cz - square.half),
        (square.cx + square.half, square.cz + square.half),
    );
    let sites: Vec<&ProceduralSite> = networks
        .iter()
        .flat_map(|network| network.sites().iter())
        .filter(|site| {
            (site.centre[0] - square.cx).abs() <= square.half
                && (site.centre[1] - square.cz).abs() <= square.half
        })
        .collect();
    let mut held_by: BTreeMap<FactionId, usize> = BTreeMap::new();
    for site in &sites {
        let (px, py) = square.pixel(site.centre[0], site.centre[1]);
        let holder = map.holder_of(&site.id);
        let colour = holder
            .and_then(|id| factions.get(&id))
            .map_or(UNHELD, |faction| faction.colour);
        if let Some(id) = holder {
            *held_by.entry(id).or_default() += 1;
        }
        canvas.disc(px, py, DOT_PX + 1.0, [15, 15, 15]);
        canvas.disc(px, py, DOT_PX, colour);
    }
    // The authored settlements are held by an anchor: a dot in its colour.
    for settlement in world.sites() {
        let [x, z] = settlement.centre;
        let (px, py) = square.pixel(x, z);
        if let Some(faction) = map.territory(x, z).holder.and_then(|id| factions.get(&id)) {
            canvas.disc(px, py, DOT_PX + 2.0, [15, 15, 15]);
            canvas.disc(px, py, DOT_PX + 1.0, faction.colour);
        }
    }
    for faction in factions.values() {
        let (px, py) = square.pixel(faction.seat[0], faction.seat[1]);
        if px < 0.0 || py < 0.0 || px >= f64::from(size) || py >= f64::from(size) {
            continue;
        }
        canvas.ring(px, py, DOT_PX + 3.0, 1.5, [245, 245, 245]);
        canvas.label(
            px,
            py + DOT_PX + 6.0,
            &faction.name,
            1,
            [245, 245, 245],
            false,
        );
    }
    // The key lists the factions with the most sites in the square, and
    // the authored holders whatever they hold.
    let anchor_ids: Vec<FactionId> = factions
        .values()
        .filter(|faction| {
            map.registries()
                .factions
                .anchors()
                .iter()
                .any(|anchor| anchor.name == faction.name)
        })
        .map(|faction| faction.id)
        .collect();
    let mut listed: Vec<&Faction> = factions.values().collect();
    listed.sort_by_key(|faction| {
        (
            !anchor_ids.contains(&faction.id),
            std::cmp::Reverse(held_by.get(&faction.id).copied().unwrap_or(0)),
            faction.id,
        )
    });
    listed.truncate(KEY_ROWS);
    let rows: Vec<(String, [u8; 3])> = listed
        .iter()
        .map(|faction| (faction.name.clone(), faction.colour))
        .collect();
    canvas.legend(
        rows.iter()
            .map(|(name, colour)| (1, name.as_str(), *colour)),
        false,
    );
    for faction in factions.values().filter(|f| held_by.contains_key(&f.id)) {
        println!(
            "factions: {} ({}) seat {:.0},{:.0}, {} sites in the square",
            faction.name,
            faction.kind,
            faction.seat[0],
            faction.seat[1],
            held_by.get(&faction.id).copied().unwrap_or(0)
        );
    }
    image
}
