//! `worldmap`: a seed in, eleven PNGs out.
//!
//! The biome map paints each pixel with the dominant biome's tint (sea in
//! blue where the height is below sea level); the height map is grey from
//! the deepest floor to the highest peak; moisture is a blue ramp and
//! hydration and cover are grey, each over the unit range so the maps of
//! two seeds are comparable. All five are the worldgen dev loop: change a
//! `.biome.ron` or a parameter, run this, look. A climate is tuned by
//! looking at it (ADR 0006), which is why the derived fields get maps of
//! their own rather than being read off the biomes.
//!
//! The sixth, `-fog`, is the fog field of `strata-weather`, by default
//! under the fullest weather (every basin at its top, sea fog banks over
//! most of the sea; `--fog` and `--sea-fog` pick another): the brightness
//! is the density, the colour the height of the fog's top over the map's
//! own range, blue low to red high, over the relief in dark grey. A basin
//! filled to a level reads as one colour; it is how the field is reviewed
//! before a frame is rendered (ticket 0003).
//!
//! `-geology` is the province field (ticket 0004, step 3): each province's
//! map tint by its weight, so borders blend as the field blends them,
//! times the hillshade so the ranges read through it, the sea blue and the
//! coast drawn black. Written only for a registry set with provinces.
//!
//! `-rivers` is the river network (ticket 0005, step 1; ADR 0010): every
//! land mass under the map is baked, and its rivers drawn by discharge over
//! the hillshade, lakes filled, falls ringed, a cut mass's cells and Port
//! Serein's extent outlined (see `rivers.rs`). `--no-rivers` leaves it out.
//!
//! `-sites` is the procedural sites (ticket 0005, step 4; ADR 0012) over
//! the `-rivers` map: each site by its kind's symbol with its name, Port
//! Serein ringed, the pads to scale with their facing and their road's
//! arrival where the map is fine enough (see `sites.rs`). `--no-sites`
//! leaves it out; it needs the rivers.
//!
//! `-deposits` is the deposits (ticket 0007, step 3) over the `-sites`
//! map: a mark a deposit on the disc of its area (opacity by its peak), its
//! shape and colour its class's, with a key, and
//! on the standard output how many of each type there are and how far the
//! nearest of each is (see `deposits.rs`). `--no-deposits` leaves it out;
//! it needs the rivers.
//!
//! `-factions` is whose ground it is (ticket 0012, step 1) over the `-shade`
//! map: each faction's territory in its colour, the borders, every site as a
//! dot in its holder's colour and each faction named at its seat (see
//! `factions.rs`). `--no-factions` leaves it out.
//!
//! `-roads` is the road network (ticket 0005, step 5; ADR 0012, decision
//! 7) over the `-rivers` map with the sites over it: roads by class,
//! junctions, and crossings by kind (see `roads.rs`). `--no-roads` leaves
//! it out; it needs the rivers.
//!
//! The last two are how the ground's drama is reviewed (ticket 0004): `-shade`
//! is the height field hillshaded, lit from the north-west, the sea one flat
//! tone, so a ridge, a gully and a plain read as shapes rather than as greys;
//! `-view` is the measure `strata_worldgen::measure::view_deg`, the tallest
//! thing in view from a standing eye, blue at level or under to red at
//! [`VIEW_SCALE_DEG`] or more, on a coarser lattice than the map because a
//! view is two thousand samples. The log line says the scale and the land's
//! median. Both are slow in a debug build; run the tool with `--release`,
//! or leave the view out with `--no-view`.

mod deposits;
mod factions;
mod rivers;
mod roads;
mod sites;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use image::{ImageBuffer, Rgb};
use rayon::prelude::*;
use strata_core::Seed;
use strata_weather::{BasinTile, FogField, FogSample, TileCoord, Weather};
use strata_worldgen::measure::view_deg;
use strata_worldgen::{ProceduralWorld, ProvinceMix, Sample, WorldSource};

/// The view the `-view` map paints red, in degrees; anything steeper is as
/// red. Fixed rather than the map's own range so two maps compare.
const VIEW_SCALE_DEG: f64 = 30.0;

#[derive(Parser, Debug)]
#[command(name = "worldmap", version, about)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a command line's switches, each a map left out"
)]
struct Cli {
    /// The world seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Width and height of the mapped square, in kilometres.
    #[arg(long, default_value_t = 100.0)]
    size_km: f64,
    /// `x` of the square's centre, in kilometres.
    #[arg(long, default_value_t = 0.0)]
    center_x_km: f64,
    /// `z` of the square's centre, in kilometres. North (`+z`) is up.
    #[arg(long, default_value_t = 0.0)]
    center_z_km: f64,
    /// Pixels per side.
    #[arg(long, default_value_t = 1024)]
    resolution: u32,
    /// Where to write the biome map. The height, moisture, hydration,
    /// cover, fog, hillshade, geology, rivers, sites, deposits and view maps
    /// go next to it with `-height`, `-moisture`, `-hydration`, `-cover`,
    /// `-fog`, `-shade`, `-geology`, `-rivers`, `-sites`, `-deposits`, `-factions` and
    /// `-view` suffixes.
    #[arg(long, default_value = "worldmap.png")]
    out: PathBuf,
    /// The asset root holding `biomes/`, `terrain/` and `geology/`.
    #[arg(long, default_value = "assets")]
    assets: PathBuf,
    /// Basin fog for the `-fog` map, `0..=1`. The default is the fullest.
    #[arg(long, default_value_t = 1.0)]
    fog: f32,
    /// Sea fog for the `-fog` map, `0..=1`. The default is the fullest.
    #[arg(long, default_value_t = 1.0)]
    sea_fog: f32,
    /// How much the `-shade` map exaggerates heights: 1 is the ground as
    /// it is, which at 100 m a pixel leaves the lowland nearly flat.
    #[arg(long, default_value_t = 1.0)]
    shade_exaggeration: f64,
    /// Points a side of the lattice the `-view` map is measured on, scaled
    /// up to the map. A view is about two thousand samples, so the map's
    /// own resolution would take minutes.
    #[arg(long, default_value_t = 256)]
    view_px: u32,
    /// Leave the `-view` map out: it is most of the run's time (about 130
    /// million samples at the default lattice), minutes in a debug build.
    #[arg(long)]
    no_view: bool,
    /// Leave the `-rivers` map out, and with it the bake of every land mass
    /// under the map.
    #[arg(long)]
    no_rivers: bool,
    /// Leave the `-sites` map out.
    #[arg(long)]
    no_sites: bool,
    /// Leave the `-roads` map out.
    #[arg(long)]
    no_roads: bool,
    /// Leave the `-deposits` map out.
    #[arg(long)]
    no_deposits: bool,
    /// Leave the `-factions` map out.
    #[arg(long)]
    no_factions: bool,
    /// Threads to map on. Not every core by default, and never more than
    /// 16 without asking: a debug build on the development machine (an
    /// i9-13900KF) dies with SIGSEGV on all 32, which is that CPU under
    /// load; rerun, do not trace it.
    #[arg(long, default_value_t = default_threads())]
    threads: usize,
}

/// Every core, up to the 16 the development machine's CPU takes.
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(16)
}

/// The one rayon pool of the run, `threads` wide.
fn limit_threads(threads: usize) -> anyhow::Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build_global()
        .context("building the thread pool")
}

#[allow(clippy::too_many_lines, reason = "one straight run of the maps")]
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    limit_threads(cli.threads)?;
    let registries = strata_registry::load_from_dir(&cli.assets)
        .with_context(|| format!("loading registries from {}", cli.assets.display()))?;
    let registries = std::sync::Arc::new(registries);
    let world = std::sync::Arc::new(ProceduralWorld::new(Seed(cli.seed), &registries));
    let tints: Vec<[u8; 3]> = registries
        .biomes
        .values()
        .map(|def| def.tint.to_rgb8())
        .collect();

    let n = cli.resolution;
    let metres_per_px = cli.size_km * 1000.0 / f64::from(n);
    let half = cli.size_km * 500.0;
    let (cx, cz) = (cli.center_x_km * 1000.0, cli.center_z_km * 1000.0);
    // Every sample is carved by the rivers round it (ticket 0005, step 2):
    // bake the masses under the map on the map's threads first, not one at
    // a time on whichever row meets each.
    world.hydrology().set_threads(cli.threads);
    let started = std::time::Instant::now();
    let masses = world.prepare_water(cx, cz, half);
    println!(
        "rivers: {masses} land masses prepared in {:.2} s",
        started.elapsed().as_secs_f64()
    );
    let rows: Vec<Vec<Sample>> = (0..n)
        .into_par_iter()
        .map(|py| {
            // The image's top row is the far (+z) edge so north is up.
            let z = cz + half - (f64::from(py) + 0.5) * metres_per_px;
            let x0 = cx - half + 0.5 * metres_per_px;
            (0..n)
                .map(|px| world.sample(x0 + f64::from(px) * metres_per_px, z))
                .collect()
        })
        .collect();

    let sea = world.sea_level();
    let (min_h, max_h) = rows
        .iter()
        .flatten()
        .fold((f32::MAX, f32::MIN), |(lo, hi), s| {
            (lo.min(s.height), hi.max(s.height))
        });

    let weather = Weather {
        fog: cli.fog,
        sea_fog: cli.sea_fog,
        ..Weather::FULL
    };
    let fog = fog_rows(&world, cli.seed, &weather, (cx, cz), half, metres_per_px, n);
    let mut maps = paint(&rows, sea, (min_h, max_h), &tints);
    maps.fog = paint_fog(&rows, &fog, sea, (min_h, max_h));
    maps.shade = paint_shade(&rows, sea, metres_per_px, cli.shade_exaggeration);
    println!(
        "shade: lit from the north-west at 45 deg, heights exaggerated {}x, sea flat",
        cli.shade_exaggeration
    );
    if world.province_field().is_some() {
        maps.geology = geology_map(
            &world,
            &registries,
            &rows,
            (cx, cz, half, metres_per_px),
            cli.shade_exaggeration,
        );
    }
    if !cli.no_rivers {
        let square = rivers::Square {
            cx,
            cz,
            half,
            metres_per_px,
        };
        maps.rivers =
            rivers::rivers_map(&world, &rows, square, cli.shade_exaggeration, cli.threads);
        if !cli.no_sites {
            maps.sites = sites::sites_map(&world, &maps.rivers, square);
        }
        if !cli.no_deposits && !registries.resources.is_empty() {
            // Over the sites where they are drawn, so a deposit is read
            // against the places its terms come from.
            let base = if cli.no_sites {
                &maps.rivers
            } else {
                &maps.sites
            };
            let field = veyra_world::DepositField::new(
                std::sync::Arc::clone(&world),
                Seed(cli.seed),
                std::sync::Arc::clone(&registries),
            );
            maps.deposits = deposits::deposits_map(&field, base, square).0;
        }
        if !cli.no_roads {
            let roads = roads::roads_map(&world, &maps.rivers, square);
            maps.roads = sites::sites_map(&world, &roads, square);
            roads::legend(&mut maps.roads, square);
        }
    }
    if !cli.no_factions && !registries.factions.is_empty() {
        let map = veyra_world::FactionMap::new(
            std::sync::Arc::clone(&world),
            Seed(cli.seed),
            std::sync::Arc::clone(&registries),
        );
        let square = rivers::Square {
            cx,
            cz,
            half,
            metres_per_px,
        };
        maps.factions = factions::factions_map(&map, &maps.shade, square);
    }
    if !cli.no_view {
        let views = view_lattice(&world, (cx, cz), half, cli.view_px.max(2));
        maps.view = paint_view(&rows, &views, sea);
    }
    let written = maps
        .save(&cli.out)
        .context("writing the map set")?
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let land = rows
        .iter()
        .flatten()
        .filter(|s| !s.is_submerged(sea))
        .count();
    let total = rows.len() * rows.len();
    let (wx, wz) = world.wind_at(cx, cz);
    let mean = |f: fn(&Sample) -> f32| {
        let sum: f64 = rows
            .iter()
            .flatten()
            .filter(|s| !s.is_submerged(sea))
            .map(|s| f64::from(f(s)))
            .sum();
        // Land pixels are a pixel count; exact in f64 far beyond any map.
        #[allow(clippy::cast_precision_loss)]
        let n = land.max(1) as f64;
        sum / n
    };
    println!(
        "worldmap: seed {} over {} km at {:.0} m/px: heights {min_h:.0}..{max_h:.0} m, {}% land, wind ({wx:.2}, {wz:.2}), land means moisture {:.2} hydration {:.2} cover {:.2}, wrote {written}",
        cli.seed,
        cli.size_km,
        metres_per_px,
        land * 100 / total,
        mean(|s| s.moisture),
        mean(|s| s.hydration),
        mean(|s| s.cover),
    );
    let mut counts = vec![0usize; tints.len()];
    for s in rows.iter().flatten() {
        counts[s.dominant_biome().0 as usize] += 1;
    }
    for ((_, name, _), count) in registries.biomes.iter().zip(counts) {
        // Tenths of a percent from integer arithmetic; the counts are pixel
        // counts, so nothing is lost to a float here.
        let tenths = count * 1000 / total;
        println!("  {name:<20} {:3}.{}%", tenths / 10, tenths % 10);
    }
    Ok(())
}

/// The PNGs one run produces, in memory.
struct Maps {
    biome: ImageBuffer<Rgb<u8>, Vec<u8>>,
    height: ImageBuffer<Rgb<u8>, Vec<u8>>,
    moisture: ImageBuffer<Rgb<u8>, Vec<u8>>,
    hydration: ImageBuffer<Rgb<u8>, Vec<u8>>,
    cover: ImageBuffer<Rgb<u8>, Vec<u8>>,
    fog: ImageBuffer<Rgb<u8>, Vec<u8>>,
    shade: ImageBuffer<Rgb<u8>, Vec<u8>>,
    geology: ImageBuffer<Rgb<u8>, Vec<u8>>,
    rivers: ImageBuffer<Rgb<u8>, Vec<u8>>,
    sites: ImageBuffer<Rgb<u8>, Vec<u8>>,
    deposits: ImageBuffer<Rgb<u8>, Vec<u8>>,
    factions: ImageBuffer<Rgb<u8>, Vec<u8>>,
    roads: ImageBuffer<Rgb<u8>, Vec<u8>>,
    view: ImageBuffer<Rgb<u8>, Vec<u8>>,
}

impl Maps {
    /// Writes the set: `out` for the biome map and a suffixed sibling for
    /// each of the others. Returns the paths written, for the log line.
    fn save(&self, out: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
        let mut written = vec![out.to_path_buf()];
        self.biome
            .save(out)
            .with_context(|| format!("writing {}", out.display()))?;
        for (suffix, image) in [
            ("height", &self.height),
            ("moisture", &self.moisture),
            ("hydration", &self.hydration),
            ("cover", &self.cover),
            ("fog", &self.fog),
            ("shade", &self.shade),
            ("geology", &self.geology),
            ("rivers", &self.rivers),
            ("sites", &self.sites),
            ("deposits", &self.deposits),
            ("factions", &self.factions),
            ("roads", &self.roads),
            ("view", &self.view),
        ] {
            // A map left out (`--no-view`, `--no-rivers`, or `-geology` in a world
            // without provinces) is empty and not written.
            if image.width() == 0 {
                continue;
            }
            let path = sibling(out, suffix);
            image
                .save(&path)
                .with_context(|| format!("writing {}", path.display()))?;
            written.push(path);
        }
        Ok(written)
    }
}

/// Paints the five maps from the sampled rows. `heights` is the map's own
/// range, so the height map uses its full contrast wherever it is centred.
fn paint(rows: &[Vec<Sample>], sea: f32, heights: (f32, f32), tints: &[[u8; 3]]) -> Maps {
    // Rows came from `0..n` with `n: u32`, and each row is that long.
    #[allow(clippy::cast_possible_truncation)]
    let n = rows.len() as u32;
    let (min_h, max_h) = heights;
    let mut maps = Maps {
        biome: ImageBuffer::new(n, n),
        height: ImageBuffer::new(n, n),
        moisture: ImageBuffer::new(n, n),
        hydration: ImageBuffer::new(n, n),
        cover: ImageBuffer::new(n, n),
        fog: ImageBuffer::new(0, 0),
        shade: ImageBuffer::new(0, 0),
        geology: ImageBuffer::new(0, 0),
        rivers: ImageBuffer::new(0, 0),
        sites: ImageBuffer::new(0, 0),
        deposits: ImageBuffer::new(0, 0),
        factions: ImageBuffer::new(0, 0),
        roads: ImageBuffer::new(0, 0),
        view: ImageBuffer::new(0, 0),
    };
    for (py, row) in rows.iter().enumerate() {
        for (px, sample) in row.iter().enumerate() {
            // Pixel indices came from `0..n` as u32.
            #[allow(clippy::cast_possible_truncation)]
            let (px, py) = (px as u32, py as u32);
            let rgb = if sample.is_submerged(sea) {
                // Darker the deeper, so shelves read differently from trenches.
                let depth = ((sea - sample.height) / (sea - min_h).max(1.0)).clamp(0.0, 1.0);
                let shade = 1.0 - 0.6 * depth;
                [
                    to_byte(0.10 * shade),
                    to_byte(0.30 * shade),
                    to_byte(0.60 * shade),
                ]
            } else {
                let tint = tints[sample.dominant_biome().0 as usize];
                // A touch of height shading so relief shows through the tint.
                let lift = ((sample.height - sea) / (max_h - sea).max(1.0)).clamp(0.0, 1.0);
                let shade = 0.75 + 0.25 * lift;
                [
                    to_byte(f32::from(tint[0]) / 255.0 * shade),
                    to_byte(f32::from(tint[1]) / 255.0 * shade),
                    to_byte(f32::from(tint[2]) / 255.0 * shade),
                ]
            };
            maps.biome.put_pixel(px, py, Rgb(rgb));
            let grey = to_byte((sample.height - min_h) / (max_h - min_h).max(1.0));
            maps.height.put_pixel(px, py, Rgb([grey; 3]));
            // Sea is one flat dark tone in the three climate maps: the eye
            // should read the coastline and the land's gradient, and a
            // climate over water is not what is being tuned.
            if sample.is_submerged(sea) {
                let ocean = Rgb([16, 20, 28]);
                maps.moisture.put_pixel(px, py, ocean);
                maps.hydration.put_pixel(px, py, ocean);
                maps.cover.put_pixel(px, py, ocean);
            } else {
                // Dry sand to wet blue, so a rain shadow reads as a colour
                // and not only as a shade.
                let m = sample.moisture;
                maps.moisture.put_pixel(
                    px,
                    py,
                    Rgb([
                        to_byte(0.90 - 0.80 * m),
                        to_byte(0.80 - 0.35 * m),
                        to_byte(0.45 + 0.45 * m),
                    ]),
                );
                maps.hydration
                    .put_pixel(px, py, Rgb([to_byte(sample.hydration); 3]));
                maps.cover
                    .put_pixel(px, py, Rgb([to_byte(sample.cover); 3]));
            }
        }
    }
    maps
}

/// The fog field over the map under `weather`, one sample per pixel
/// in the same order as the world's rows. Bakes every tile the square
/// touches, in parallel, and logs what that cost.
fn fog_rows(
    world: &ProceduralWorld,
    seed: u64,
    weather: &Weather,
    centre: (f64, f64),
    half: f64,
    metres_per_px: f64,
    n: u32,
) -> Vec<Vec<FogSample>> {
    let (cx, cz) = centre;
    let coords: Vec<TileCoord> =
        TileCoord::covering(cx - half, cz - half, cx + half, cz + half).collect();
    let started = std::time::Instant::now();
    let baked: Vec<(BasinTile, usize)> = coords
        .par_iter()
        .map(|&coord| {
            let (tile, stats) = BasinTile::bake_with_stats(world, coord);
            (tile, stats.samples)
        })
        .collect();
    let elapsed = started.elapsed();
    let samples: usize = baked.iter().map(|(_, s)| s).sum();
    let mut field = FogField::new(Seed(seed));
    for (tile, _) in baked {
        field.insert(tile).expect("a bake is well formed");
    }
    println!(
        "fog: {} tiles baked in {:.2} s on all cores, {samples} samples",
        coords.len(),
        elapsed.as_secs_f64()
    );
    (0..n)
        .into_par_iter()
        .map(|py| {
            let z = cz + half - (f64::from(py) + 0.5) * metres_per_px;
            let x0 = cx - half + 0.5 * metres_per_px;
            (0..n)
                .map(|px| {
                    field
                        .sample(x0 + f64::from(px) * metres_per_px, z, weather)
                        .unwrap_or(FogSample::NONE)
                })
                .collect()
        })
        .collect()
}

/// The fog map: the relief in dark grey (the sea darker and blue), the fog
/// over it brighter the denser, coloured by its top over the range of tops
/// the map holds wherever the fog is more than a trace.
fn paint_fog(
    rows: &[Vec<Sample>],
    fog: &[Vec<FogSample>],
    sea: f32,
    heights: (f32, f32),
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    // Rows came from `0..n` with `n: u32`.
    #[allow(clippy::cast_possible_truncation)]
    let n = rows.len() as u32;
    let (min_h, max_h) = heights;
    let (lo, hi) = fog
        .iter()
        .flatten()
        .filter(|f| f.density > 0.05)
        .fold((f32::MAX, f32::MIN), |(lo, hi), f| {
            (lo.min(f.top), hi.max(f.top))
        });
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (0.0, 1.0) };
    println!("fog: tops {lo:.0}..{hi:.0} m where the density is over 0.05 (blue to red)");
    let mut image = ImageBuffer::new(n, n);
    for (py, (row, fog_row)) in rows.iter().zip(fog).enumerate() {
        for (px, (sample, f)) in row.iter().zip(fog_row).enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let (px, py) = (px as u32, py as u32);
            let base = if sample.is_submerged(sea) {
                [0.03, 0.05, 0.12]
            } else {
                let g = 0.08 + 0.3 * ((sample.height - min_h) / (max_h - min_h).max(1.0));
                [g, g, g]
            };
            let colour = palette(((f.top - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0));
            // Linear: the brightness is the density.
            let a = f.density.clamp(0.0, 1.0);
            let rgb = [0, 1, 2].map(|k| to_byte(base[k] * (1.0 - a) + colour[k] * a));
            image.put_pixel(px, py, Rgb(rgb));
        }
    }
    image
}

/// The hillshade: each land pixel lit by a sun in the north-west, 45
/// degrees up, over a normal from its neighbours' heights (the sea taken as
/// its surface, so a coast is not a cliff into the sea bed); the sea one
/// flat tone.
fn paint_shade(
    rows: &[Vec<Sample>],
    sea: f32,
    metres_per_px: f64,
    exaggeration: f64,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let n = rows.len();
    // Rows came from `0..n` with `n: u32`.
    #[allow(clippy::cast_possible_truncation)]
    let mut image = ImageBuffer::new(n as u32, n as u32);
    for (py, row) in rows.iter().enumerate() {
        for (px, sample) in row.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let (ix, iy) = (px as u32, py as u32);
            if sample.is_submerged(sea) {
                image.put_pixel(ix, iy, Rgb([40, 58, 82]));
                continue;
            }
            let lit = sunlight(rows, (px, py), sea, metres_per_px, exaggeration);
            // A little ambient so faces turned from the sun are not black.
            #[allow(clippy::cast_possible_truncation)]
            let grey = to_byte((0.12 + 0.88 * lit) as f32);
            image.put_pixel(ix, iy, Rgb([grey; 3]));
        }
    }
    image
}

/// How much of a sun in the north-west, 45 degrees up, falls on the ground
/// at a pixel, `0..=1`, over a normal from its neighbours' heights (the sea
/// taken as its surface, so a coast is not a cliff into the sea bed).
fn sunlight(
    rows: &[Vec<Sample>],
    (px, py): (usize, usize),
    sea: f32,
    metres_per_px: f64,
    exaggeration: f64,
) -> f64 {
    let n = rows.len();
    let h = |px: usize, py: usize| f64::from(rows[py][px].height.max(sea)) * exaggeration;
    // North-west and up: `x` east, `y` north (the image's up), `z` up.
    let (l, up) = (
        core::f64::consts::FRAC_1_SQRT_2,
        core::f64::consts::FRAC_1_SQRT_2,
    );
    let light = [-l * up, l * up, up];
    let (west, east) = (px.saturating_sub(1), (px + 1).min(n - 1));
    let (north, south) = (py.saturating_sub(1), (py + 1).min(n - 1));
    // Pixel spans; exact in f64.
    #[allow(clippy::cast_precision_loss)]
    let (across, down) = (
        (east - west) as f64 * metres_per_px,
        (south - north) as f64 * metres_per_px,
    );
    let dx = (h(east, py) - h(west, py)) / across.max(f64::MIN_POSITIVE);
    // Image rows grow southwards; the slope northwards is the negative of
    // the one down the image.
    let dy = (h(px, north) - h(px, south)) / down.max(f64::MIN_POSITIVE);
    let normal = [-dx, -dy, 1.0];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + 1.0).sqrt();
    ((normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length).max(0.0)
}

/// The `-geology` map of a world with provinces, and a log line of the
/// land's shares. `square` is the map's centre, half its side and its
/// metres a pixel.
fn geology_map(
    world: &ProceduralWorld,
    registries: &strata_registry::GameRegistries,
    rows: &[Vec<Sample>],
    square: (f64, f64, f64, f64),
    exaggeration: f64,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (cx, cz, half, metres_per_px) = square;
    let sea = world.sea_level();
    // Rows came from `0..n` with `n: u32`.
    #[allow(clippy::cast_possible_truncation)]
    let n = rows.len() as u32;
    let tints: Vec<[f32; 3]> = registries
        .provinces
        .values()
        .map(|def| def.tint.0)
        .collect();
    let mixes: Vec<Vec<ProvinceMix>> = (0..n)
        .into_par_iter()
        .map(|py| {
            let z = cz + half - (f64::from(py) + 0.5) * metres_per_px;
            let x0 = cx - half + 0.5 * metres_per_px;
            (0..n)
                .map(|px| {
                    world
                        .provinces(x0 + f64::from(px) * metres_per_px, z)
                        .expect("the field is on")
                })
                .collect()
        })
        .collect();
    let image = paint_geology(rows, &mixes, sea, &tints, metres_per_px, exaggeration);
    let mut land = vec![0usize; tints.len()];
    let mut blended = 0usize;
    for (row, mix_row) in rows.iter().zip(&mixes) {
        for (sample, mix) in row.iter().zip(mix_row) {
            if !sample.is_submerged(sea) {
                land[mix.dominant().0 as usize] += 1;
                blended += usize::from(mix.entries()[0].1 < 0.999);
            }
        }
    }
    let total: usize = land.iter().sum::<usize>().max(1);
    let shares = registries
        .provinces
        .iter()
        .zip(&land)
        .map(|((_, name, _), count)| {
            // Tenths of a percent from pixel counts.
            let tenths = count * 1000 / total;
            format!("{name} {}.{}%", tenths / 10, tenths % 10)
        })
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "geology: province tints by weight over the hillshade; land by province: {shares}; {}% of land in a blend",
        blended * 100 / total
    );
    image
}

/// The geology map: each land pixel the tints of the provinces weighing
/// there, by their weights, times the hillshade (with an ambient, so a
/// shaded face keeps its colour); the sea blue, darker the deeper, and the
/// sea pixels beside land black, so the coast is a line.
fn paint_geology(
    rows: &[Vec<Sample>],
    mixes: &[Vec<ProvinceMix>],
    sea: f32,
    tints: &[[f32; 3]],
    metres_per_px: f64,
    exaggeration: f64,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let n = rows.len();
    let dry = |px: usize, py: usize| !rows[py][px].is_submerged(sea);
    // Rows came from `0..n` with `n: u32`.
    #[allow(clippy::cast_possible_truncation)]
    let mut image = ImageBuffer::new(n as u32, n as u32);
    for (py, (row, mix_row)) in rows.iter().zip(mixes).enumerate() {
        for (px, (sample, mix)) in row.iter().zip(mix_row).enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let (ix, iy) = (px as u32, py as u32);
            if sample.is_submerged(sea) {
                let coast = (px > 0 && dry(px - 1, py))
                    || (px + 1 < n && dry(px + 1, py))
                    || (py > 0 && dry(px, py - 1))
                    || (py + 1 < n && dry(px, py + 1));
                let rgb = if coast {
                    [10, 10, 10]
                } else {
                    let depth = ((sea - sample.height) / 400.0).clamp(0.0, 1.0);
                    [
                        to_byte((70.0 - 45.0 * depth) / 255.0),
                        to_byte((120.0 - 55.0 * depth) / 255.0),
                        to_byte((165.0 - 40.0 * depth) / 255.0),
                    ]
                };
                image.put_pixel(ix, iy, Rgb(rgb));
                continue;
            }
            let mut colour = [0.0f32; 3];
            for &(id, weight) in mix.entries() {
                let tint = tints[id.0 as usize];
                for (c, t) in colour.iter_mut().zip(tint) {
                    // A weight is a share; f32 is the image's precision.
                    #[allow(clippy::cast_possible_truncation)]
                    let w = weight as f32;
                    *c += t * w;
                }
            }
            let lit = sunlight(rows, (px, py), sea, metres_per_px, exaggeration);
            #[allow(clippy::cast_possible_truncation)]
            let k = (0.35 + 0.8 * lit) as f32;
            image.put_pixel(ix, iy, Rgb(colour.map(|c| to_byte(c * k))));
        }
    }
    image
}

/// The tallest thing in view, in degrees, on a `side` by `side` lattice of
/// points at the centres of equal cells over the map's square, in parallel.
/// Row-major, the top row the far (+z) edge as the map's.
fn view_lattice(
    world: &ProceduralWorld,
    centre: (f64, f64),
    half: f64,
    side: u32,
) -> Vec<Vec<f64>> {
    let (cx, cz) = centre;
    let step = 2.0 * half / f64::from(side);
    let started = std::time::Instant::now();
    let views: Vec<Vec<f64>> = (0..side)
        .into_par_iter()
        .map(|j| {
            let z = cz + half - (f64::from(j) + 0.5) * step;
            (0..side)
                .map(|i| view_deg(world, cx - half + (f64::from(i) + 0.5) * step, z))
                .collect()
        })
        .collect();
    println!(
        "view: {side}x{side} points {step:.0} m apart measured in {:.1} s on all cores",
        started.elapsed().as_secs_f64()
    );
    views
}

/// The view map: each land pixel the view bilinearly between the lattice
/// points round it, blue at level or under to red at [`VIEW_SCALE_DEG`];
/// the sea one flat dark tone. Logs the scale and the land's median.
fn paint_view(rows: &[Vec<Sample>], views: &[Vec<f64>], sea: f32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let n = rows.len();
    let side = views.len();
    // Pixel and lattice indices are small; exact in f64.
    #[allow(clippy::cast_precision_loss)]
    let (n_f, side_f) = (n as f64, side as f64);
    // The pixel's centre in lattice coordinates, lattice points at the
    // centres of their cells: the cell below it and how far into the next.
    let lattice = |pixel: usize| {
        #[allow(clippy::cast_precision_loss)]
        let along = ((pixel as f64 + 0.5) / n_f * side_f - 0.5).clamp(0.0, side_f - 1.0);
        // Clamped to the lattice first, so the floor is in range.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell = (along.floor() as usize).min(side - 2);
        #[allow(clippy::cast_precision_loss)]
        let fraction = along - cell as f64;
        (cell, fraction)
    };
    let at = |px: usize, py: usize| {
        let ((col, across), (row, down)) = (lattice(px), lattice(py));
        let top = views[row][col] * (1.0 - across) + views[row][col + 1] * across;
        let bottom = views[row + 1][col] * (1.0 - across) + views[row + 1][col + 1] * across;
        top * (1.0 - down) + bottom * down
    };
    #[allow(clippy::cast_possible_truncation)]
    let mut image = ImageBuffer::new(n as u32, n as u32);
    let mut land = Vec::new();
    for (py, row) in rows.iter().enumerate() {
        for (px, sample) in row.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let (ix, iy) = (px as u32, py as u32);
            if sample.is_submerged(sea) {
                image.put_pixel(ix, iy, Rgb([16, 20, 28]));
                continue;
            }
            let view = at(px, py);
            land.push(view);
            #[allow(clippy::cast_possible_truncation)]
            let colour = palette((view / VIEW_SCALE_DEG).clamp(0.0, 1.0) as f32);
            image.put_pixel(ix, iy, Rgb(colour.map(to_byte)));
        }
    }
    land.sort_by(f64::total_cmp);
    let median = land.get(land.len() / 2).copied().unwrap_or(f64::NAN);
    let ten = land.iter().filter(|&&v| v >= 10.0).count();
    println!(
        "view: tallest thing in view from a 1.7 m eye out to 12 km, 0 deg or under blue, {VIEW_SCALE_DEG:.0} deg or over red (blue, cyan 7.5, green 15, yellow 22.5, red 30); land median {median:.1} deg, {}% of land pixels see 10 deg or more",
        ten * 100 / land.len().max(1)
    );
    image
}

/// Blue, cyan, green, yellow, red over `0..=1`.
fn palette(t: f32) -> [f32; 3] {
    const STOPS: [[f32; 3]; 5] = [
        [0.20, 0.35, 1.00],
        [0.20, 0.90, 1.00],
        [0.30, 1.00, 0.35],
        [1.00, 0.95, 0.25],
        [1.00, 0.30, 0.20],
    ];
    let x = t * 4.0;
    // In 0..=4 after the clamp; the index is its floor.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k = (x.floor() as usize).min(3);
    #[allow(clippy::cast_precision_loss)]
    let f = x - k as f32;
    [0, 1, 2].map(|c| STOPS[k][c] + (STOPS[k + 1][c] - STOPS[k][c]) * f)
}

fn to_byte(unit: f32) -> u8 {
    // Clamped first, so the truncating cast is in range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (unit.clamp(0.0, 1.0) * 255.0).round() as u8;
    byte
}

/// `<stem>-<suffix>.png` next to `out`, so one `--out` names the whole set.
fn sibling(out: &std::path::Path, suffix: &str) -> PathBuf {
    let stem = out
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("worldmap");
    out.with_file_name(format!("{stem}-{suffix}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two provinces blended half and half paint the mean of their tints,
    /// on flat land the same shade everywhere; the sea beside land is the
    /// coast's black.
    #[test]
    fn geology_blends_tints_by_weight() {
        let registries =
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load");
        let world = ProceduralWorld::new(Seed(42), &registries);
        // A border point, found on a lattice: two provinces near half.
        let (x, z) = (0..400 * 400)
            .map(|k| {
                (
                    f64::from(k % 400) * 250.0 - 50_000.0,
                    f64::from(k / 400) * 250.0 - 50_000.0,
                )
            })
            .find(|&(x, z)| {
                let m = world.provinces(x, z).expect("geology ships");
                m.entries().len() == 2 && (m.entries()[0].1 - 0.5).abs() < 0.05
            })
            .expect("a border within 50 km");
        let mix = world.provinces(x, z).expect("geology ships");
        world.hydrology().set_threads(16);
        world.prepare_water(x, z, 0.0);
        let mut flat = world.sample(x, z);
        flat.height = 100.0;
        let mut wet = flat;
        wet.height = -10.0;
        let rows = vec![vec![flat, flat, wet]; 3];
        let mixes = vec![vec![mix; 3]; 3];
        let tints: Vec<[f32; 3]> = registries.provinces.values().map(|d| d.tint.0).collect();
        let image = paint_geology(&rows, &mixes, 0.0, &tints, 100.0, 1.0);
        let [(a, wa), (b, wb)] = [mix.entries()[0], mix.entries()[1]];
        let lit = 0.35 + 0.8 * core::f64::consts::FRAC_1_SQRT_2;
        let (ta, tb) = (tints[a.0 as usize], tints[b.0 as usize]);
        for (c, (&got, (&ca, &cb))) in image
            .get_pixel(0, 1)
            .0
            .iter()
            .zip(ta.iter().zip(&tb))
            .enumerate()
        {
            #[allow(clippy::cast_possible_truncation)]
            let want = ((f64::from(ca) * wa + f64::from(cb) * wb) * lit) as f32;
            assert!(
                (i32::from(got) - i32::from(to_byte(want))).abs() <= 1,
                "channel {c}"
            );
        }
        assert_eq!(image.get_pixel(2, 1), &Rgb([10, 10, 10]));
    }
}
