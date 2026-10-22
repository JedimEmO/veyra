//! The `-deposits` map (ticket 0007, step 3): every deposit of the square
//! over the `-sites` map, a mark each, on the disc of its area.
//!
//! A deposit is an area (ticket 0009): a disc of its radius at the map's
//! scale, never smaller than its mark, washed in its class's colour with
//! an opacity that follows its peak, so a rich deposit reads denser than a
//! thin one. The mark sits at the centre.
//!
//! A class is told by its mark's shape as well as its colour, the colour
//! the class's own from the resource registry and the shape its place in
//! the class tree, so a new class file is drawn without a change here. The
//! types of one class share its mark; the second has a dark centre, the
//! third a light one, so two deposits of one class that are of two types
//! tell apart on the map as they do on the card. Where the map is fine
//! enough each deposit is named with its best line, the one highest in its
//! type's span: `CALDER PT 820`.
//!
//! The run also says what it drew: how many of each type the square holds
//! and how many lie within the register's range of the map's centre, and
//! how far the nearest of each type is from the centre and from each
//! authored settlement. That table is what the lattice's dials in
//! `assets/resources/deposits.ron` are tuned by.

use image::{ImageBuffer, Rgb};
use strata_registry::{Attribute, ResourceSet, ResourceTypeId};
use veyra_world::{Deposit, DepositField};

use crate::rivers::Square;
use crate::sites::Canvas;

/// A class's colour as the map's bytes.
fn colour_of(set: &ResourceSet, ty: ResourceTypeId) -> [u8; 3] {
    let [r, g, b, _] = set.class_of(ty).color.to_u8();
    [r, g, b]
}

/// Which of its class's types `ty` is, counting in id order from nought.
fn rank_in_class(set: &ResourceSet, ty: ResourceTypeId) -> usize {
    let class = set.class_index(ty);
    set.type_ids()
        .filter(|other| set.class_index(*other) == class)
        .position(|other| other == ty)
        .unwrap_or(0)
}

/// The centre that tells a class's types apart: none for its first, dark
/// for its second, light after.
fn centre_of(rank: usize) -> Option<[u8; 3]> {
    match rank {
        0 => None,
        1 => Some([15, 15, 15]),
        _ => Some([250, 250, 250]),
    }
}

/// The line a deposit is best in: the one highest in its type's span, the
/// earlier in the class's order on a tie.
fn best_line(set: &ResourceSet, deposit: &Deposit) -> Option<(Attribute, u16)> {
    let def = set.get(deposit.ty)?;
    let mut best: Option<(f64, Attribute, u16)> = None;
    for (attribute, value) in &deposit.figures {
        let share = def.ranges.get(attribute)?.share(*value);
        if best.is_none_or(|(held, ..)| share > held) {
            best = Some((share, *attribute, *value));
        }
    }
    best.map(|(_, attribute, value)| (attribute, value))
}

/// The least opacity of a disc, so the thinnest deposit still shows.
const WASH_FLOOR: f64 = 0.15;
/// The most opacity of a disc, so the ground under the richest still reads.
const WASH_CEIL: f64 = 0.65;

/// Washes the disc of `radius` pixels at `(cx, cy)` with `colour` at
/// `alpha` over what the image holds, so overlapping areas darken.
fn wash(
    image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    cx: f64,
    cy: f64,
    radius: f64,
    colour: [u8; 3],
    alpha: f64,
) {
    let (width, height) = (f64::from(image.width()), f64::from(image.height()));
    // The pixel box is clamped to the image first, so the casts are in range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, x1, y0, y1) = (
        (cx - radius).floor().clamp(0.0, width) as u32,
        (cx + radius).ceil().clamp(0.0, width) as u32,
        (cy - radius).floor().clamp(0.0, height) as u32,
        (cy + radius).ceil().clamp(0.0, height) as u32,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let pixel = image.get_pixel_mut(x, y);
            for (held, wash) in pixel.0.iter_mut().zip(colour) {
                let mixed = f64::from(*held) * (1.0 - alpha) + f64::from(wash) * alpha;
                // A mix of two bytes is a byte.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    *held = mixed.round() as u8;
                }
            }
        }
    }
}

/// Draws the one-pixel edge of the disc of `radius` pixels at `(cx, cy)`
/// in `colour`, whatever the wash under it.
fn ring(image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, cx: f64, cy: f64, radius: f64, colour: [u8; 3]) {
    let (width, height) = (f64::from(image.width()), f64::from(image.height()));
    // The pixel box is clamped to the image first, so the casts are in range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, x1, y0, y1) = (
        (cx - radius - 1.0).floor().clamp(0.0, width) as u32,
        (cx + radius + 1.0).ceil().clamp(0.0, width) as u32,
        (cy - radius - 1.0).floor().clamp(0.0, height) as u32,
        (cy + radius + 1.0).ceil().clamp(0.0, height) as u32,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if (d - radius).abs() <= 0.5 {
                image.put_pixel(x, y, Rgb(colour));
            }
        }
    }
}

/// What a run of the layer found.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// How many deposits of each type the square holds, by type id.
    pub in_square: Vec<usize>,
    /// How many of each type lie within the register's range of the map's
    /// centre.
    pub in_range: Vec<usize>,
    /// From the map's centre and from each authored settlement's: its name
    /// and the metres to the nearest deposit of each type, `None` for a
    /// type with none within the register's range.
    pub nearest: Vec<(String, Vec<Option<f64>>)>,
}

/// Draws the deposits on a copy of `base` (the `-sites` map) and logs what
/// it drew.
pub fn deposits_map(
    field: &DepositField,
    base: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    square: Square,
) -> (ImageBuffer<Rgb<u8>, Vec<u8>>, Summary) {
    let set = field.set();
    // The square's corners are a diagonal's half from its centre.
    let reach = square.half * core::f64::consts::SQRT_2;
    let deposits: Vec<Deposit> = field
        .near(square.cx, square.cz, reach)
        .into_iter()
        .filter(|deposit| {
            (deposit.at[0] - square.cx).abs() <= square.half
                && (deposit.at[1] - square.cz).abs() <= square.half
        })
        .collect();
    let mut image = base.clone();
    let mut canvas = Canvas {
        image: &mut image,
        labels: Vec::new(),
    };
    let fine = square.metres_per_px <= 20.0;
    let size = if fine { 5.0 } else { 3.5 };
    let most_peak = f64::from(set.rules().area.peak.1.max(1));
    // The areas under every mark, the farthest first like the marks.
    for deposit in deposits.iter().rev() {
        let (px, py) = square.pixel(deposit.at[0], deposit.at[1]);
        let radius = (f64::from(deposit.radius_m) / square.metres_per_px).max(size);
        let alpha =
            WASH_FLOOR + (WASH_CEIL - WASH_FLOOR) * (f64::from(deposit.peak) / most_peak).min(1.0);
        wash(
            canvas.image,
            px,
            py,
            radius,
            colour_of(set, deposit.ty),
            alpha,
        );
    }
    // An edge on every area, so a thin one still shows its extent.
    for deposit in deposits.iter().rev() {
        let (px, py) = square.pixel(deposit.at[0], deposit.at[1]);
        let radius = (f64::from(deposit.radius_m) / square.metres_per_px).max(size);
        ring(canvas.image, px, py, radius, colour_of(set, deposit.ty));
    }
    // The farthest first, so the marks nearest the centre are whole.
    for deposit in deposits.iter().rev() {
        let (px, py) = square.pixel(deposit.at[0], deposit.at[1]);
        canvas.symbol(
            set.class_index(deposit.ty),
            px,
            py,
            size,
            colour_of(set, deposit.ty),
        );
        if let Some(centre) = centre_of(rank_in_class(set, deposit.ty)) {
            canvas.disc(px, py, size * 0.4, centre);
        }
    }
    let rows: Vec<(usize, String, [u8; 3], usize)> = set
        .type_ids()
        .filter_map(|ty| {
            let def = set.get(ty)?;
            Some((
                set.class_index(ty),
                format!("{}: {}", set.class_of(ty).name, def.name),
                colour_of(set, ty),
                rank_in_class(set, ty),
            ))
        })
        .collect();
    let mut ordered = rows.clone();
    ordered.sort_by_key(|(class, _, _, rank)| (*class, *rank));
    let marks = canvas.legend(
        ordered
            .iter()
            .map(|(class, name, colour, _)| (*class, name.as_str(), *colour)),
        true,
    );
    for ((x, y), (.., rank)) in marks.iter().zip(&ordered) {
        if let Some(centre) = centre_of(*rank) {
            canvas.disc(*x, *y, 1.4, centre);
        }
    }
    if fine {
        for deposit in &deposits {
            let (px, py) = square.pixel(deposit.at[0], deposit.at[1]);
            let name = match best_line(set, deposit) {
                Some((attribute, value)) => {
                    format!("{} {} {value}", deposit.root, attribute.code())
                }
                None => deposit.root.clone(),
            };
            canvas.label(px, py + size + 3.0, &name, 1, [235, 235, 235], false);
        }
    }
    let summary = summarise(field, square, &deposits);
    report(set, &summary, deposits.len(), set.rules().register.range_m);
    (image, summary)
}

/// What the layer found: the counts a type and the nearest of each from
/// the map's centre and from each authored settlement.
fn summarise(field: &DepositField, square: Square, deposits: &[Deposit]) -> Summary {
    let set = field.set();
    let types = set.types().len();
    let count = |list: &[&Deposit]| {
        let mut counts = vec![0_usize; types];
        for deposit in list {
            counts[usize::from(deposit.ty.0)] += 1;
        }
        counts
    };
    let range = set.rules().register.range_m;
    let from = |x: f64, z: f64| field.near(x, z, range);
    let centre = from(square.cx, square.cz);
    let nearest_of = |list: &[Deposit], x: f64, z: f64| -> Vec<Option<f64>> {
        set.type_ids()
            .map(|ty| {
                list.iter()
                    .find(|deposit| deposit.ty == ty)
                    .map(|deposit| deposit.distance_m(x, z))
            })
            .collect()
    };
    let mut nearest = vec![(
        "the map's centre".to_owned(),
        nearest_of(&centre, square.cx, square.cz),
    )];
    let world = field.world();
    for (site, name) in world.sites().iter().zip(world.settlement_names()) {
        let [x, z] = site.centre;
        nearest.push((name.clone(), nearest_of(&from(x, z), x, z)));
    }
    Summary {
        in_square: count(&deposits.iter().collect::<Vec<_>>()),
        in_range: count(&centre.iter().collect::<Vec<_>>()),
        nearest,
    }
}

/// The run's lines: the counts a type, then a row of distances a place.
fn report(set: &ResourceSet, summary: &Summary, total: usize, range_m: f64) {
    let names: Vec<&str> = set.types().iter().map(|def| def.name.as_str()).collect();
    let listed = |counts: &[usize]| {
        names
            .iter()
            .zip(counts)
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!(
        "deposits: {total} in the square ({})",
        listed(&summary.in_square)
    );
    println!(
        "deposits: {} within {:.0} km of the map's centre ({})",
        summary.in_range.iter().sum::<usize>(),
        range_m / 1000.0,
        listed(&summary.in_range)
    );
    for (place, distances) in &summary.nearest {
        let row = names
            .iter()
            .zip(distances)
            .map(|(name, distance)| match distance {
                Some(metres) => format!("{name} {:.2} km", metres / 1000.0),
                None => format!("{name} none"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("deposits: nearest from {place}: {row}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use strata_core::Seed;
    use strata_worldgen::ProceduralWorld;

    use super::*;

    /// Twelve kilometres round Port Serein at 12 m a pixel: every class has
    /// a deposit in the square and its colour on the map where a blank base
    /// had none, the legend has a row a type, and the summary counts what
    /// was drawn.
    #[test]
    fn the_deposits_layer_marks_every_class() {
        let registries = Arc::new(
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load"),
        );
        let world = Arc::new(ProceduralWorld::new(Seed(42), &registries));
        let city = world.sites()[0].centre;
        world.hydrology().set_threads(16);
        world.prepare_water(city[0], city[1], 12_000.0);
        let field = DepositField::new(Arc::clone(&world), Seed(42), Arc::clone(&registries));
        let square = Square {
            cx: city[0],
            cz: city[1],
            half: 6_000.0,
            metres_per_px: 12.0,
        };
        let base = ImageBuffer::from_pixel(1000, 1000, Rgb([0, 96, 0]));
        let (image, summary) = deposits_map(&field, &base, square);
        let set = field.set();
        assert_eq!(summary.in_square.len(), 7);
        let drawn: usize = summary.in_square.iter().sum();
        assert!(drawn > 20, "only {drawn} deposits in twelve kilometres");
        for (class, def) in set.classes().iter().enumerate() {
            let held: usize = set
                .type_ids()
                .filter(|ty| set.class_index(*ty) == class)
                .map(|ty| summary.in_square[usize::from(ty.0)])
                .sum();
            assert!(held > 0, "no {} in the square", def.name);
            let [r, g, b, _] = def.color.to_u8();
            let pixels = image.pixels().filter(|p| p.0 == [r, g, b]).count();
            assert!(pixels > 30, "{}: {pixels} pixels of its colour", def.name);
            assert_eq!(base.pixels().filter(|p| p.0 == [r, g, b]).count(), 0);
        }
        // Two types of a class share its colour and differ by the centre.
        let salt = set
            .type_id(&"strata:evaporite_salt".parse().expect("a valid id"))
            .expect("ships");
        let sulphur = set
            .type_id(&"strata:sulphur".parse().expect("a valid id"))
            .expect("ships");
        assert_eq!(set.class_index(salt), set.class_index(sulphur));
        assert_eq!(colour_of(set, salt), colour_of(set, sulphur));
        assert_ne!(
            centre_of(rank_in_class(set, salt)),
            centre_of(rank_in_class(set, sulphur))
        );
        // The places measured: the centre, then Port Serein, which is the
        // centre here, so the two rows agree.
        assert_eq!(summary.nearest.len(), 1 + world.sites().len());
        assert_eq!(summary.nearest[0].1, summary.nearest[1].1);
        assert!(summary.in_range.iter().sum::<usize>() >= drawn);
    }
}
