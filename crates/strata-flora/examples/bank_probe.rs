//! Where the wet bank's reeds and the roads' markers stand near a point,
//! for choosing what to look at (ticket 0005, step 8):
//! `cargo run --release -p strata-flora --example bank_probe -- [x z radius_m]`.
//!
//! Lists the lakes within the radius nearest first, each with how many
//! reeds the cells along its shore hold, the cell with the most and the way
//! to the water from there; then the service roads and tracks within 3 km
//! with their first markers.

use strata_core::{RegionCoord, Seed};
use strata_flora::{FloraTable, TreeLibrary, place_cell};
use strata_worldgen::roadside::{RoadId, RoadsideKit, place_roadside};
use strata_worldgen::{ProceduralWorld, WorldSource};

/// The most shore cells placed per lake.
const CELLS_PER_LAKE: usize = 160;

#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a probe that prints its stages in order; counts of a few thousand"
)]
fn main() {
    let mut args = std::env::args().skip(1);
    let mut num = |default: f64| args.next().and_then(|a| a.parse().ok()).unwrap_or(default);
    let (x, z, radius) = (num(7_404.0), num(-4_940.0), num(12_000.0));
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let registries = strata_data::load_world_from_dir(format!("{root}/assets"))
        .expect("the shipped assets load");
    let library = TreeLibrary::read(std::path::Path::new(&format!("{root}/generated"))).ok();
    let table = FloraTable::from_registries_and_library(&registries, library.as_ref());
    let reed = table
        .species()
        .iter()
        .find(|s| s.ground.bank_m > 0.0)
        .map(|s| s.id)
        .expect("a bank species");
    let world = ProceduralWorld::new(Seed(42), &registries);
    let hydrology = world.hydrology();
    hydrology.set_threads(8);
    let networks = hydrology.bake_area(&world, (x - radius, z - radius), (x + radius, z + radius));
    let mut lakes = Vec::new();
    for network in &networks {
        for lake in network.lakes() {
            let n = lake.nodes.len() as f64;
            let cx = lake.nodes.iter().map(|p| f64::from(p[0])).sum::<f64>() / n * 512.0;
            let cz = lake.nodes.iter().map(|p| f64::from(p[1])).sum::<f64>() / n * 512.0;
            let d = (cx - x).hypot(cz - z);
            if d <= radius {
                lakes.push((d, cx, cz, lake.clone()));
            }
        }
    }
    lakes.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("{} lakes within {radius} m of ({x}, {z})", lakes.len());
    let mut first_best: Option<[f64; 2]> = None;
    for (d, cx, cz, lake) in lakes.iter().take(8) {
        // The cells round the lake's nodes whose centre is within 40 m of
        // its level: the shore.
        let mut cells = Vec::new();
        for node in &lake.nodes {
            for dj in -9..=9 {
                for di in -9..=9 {
                    let c = RegionCoord::new(node[0] * 8 + di, node[1] * 8 + dj, 0);
                    if !cells.contains(&c) {
                        cells.push(c);
                    }
                }
            }
        }
        let shore: Vec<RegionCoord> = cells
            .into_iter()
            .filter(|c| {
                let m = c.min_corner();
                let s = world.sample(m.x() + 32.0, m.z() + 32.0);
                (s.height - lake.level_m).abs() < 3.0
            })
            .take(CELLS_PER_LAKE)
            .collect();
        let mut total = 0;
        let mut best = (0, RegionCoord::new(0, 0, 0), [0.0f64; 2]);
        for coord in &shore {
            let flora = place_cell(*coord, &world, &table, Seed(42), &mut Vec::new());
            let reeds: Vec<_> = flora.plants.iter().filter(|p| p.species == reed).collect();
            total += reeds.len();
            if reeds.len() > best.0 {
                let m = coord.min_corner();
                let k = reeds.len() as f64;
                let mx = reeds.iter().map(|p| f64::from(p.x)).sum::<f64>() / k;
                let mz = reeds.iter().map(|p| f64::from(p.z)).sum::<f64>() / k;
                best = (reeds.len(), *coord, [m.x() + mx, m.z() + mz]);
            }
        }
        print!(
            "lake at ({cx:.0}, {cz:.0}) {d:.0} m off, level {:.1}, {} nodes: {total} reeds in {} shore cells",
            lake.level_m,
            lake.nodes.len(),
            shore.len()
        );
        if best.0 > 0 {
            let [bx, bz] = best.2;
            first_best.get_or_insert([bx, bz]);
            // The way to the water from the reeds' middle: the nearest of
            // sixteen bearings that reaches standing water within 80 m.
            let mut way = None;
            for step in 1..=40 {
                let r = f64::from(step) * 2.0;
                for k in 0..16 {
                    let a = std::f64::consts::TAU * f64::from(k) / 16.0;
                    let (px, pz) = (bx + r * a.sin(), bz + r * a.cos());
                    if world.sample(px, pz).standing_water().is_some() && way.is_none() {
                        way = Some((r, a));
                    }
                }
            }
            println!(
                "; most {} at ({bx:.0}, {bz:.0}), ground {:.1}, water {:?}",
                best.0,
                world.sample(bx, bz).height,
                way.map(|(r, a)| format!("{r:.0} m at bearing {a:.2} (x sin, z cos)"))
            );
        } else {
            println!();
        }
    }
    // What the reeds cost placement: 400 cells round the point and 100
    // round the nearest lake's thickest reeds, with the table as shipped and with
    // the reed's grid one slot a cell.
    let mut bare = table.species().to_vec();
    for s in &mut bare {
        if s.id == reed {
            s.spacing_m = 64.0;
        }
    }
    let bare = FloraTable::from_species(bare);
    let inland: Vec<RegionCoord> = (0..400)
        .map(|k| {
            let c = RegionCoord::containing(strata_core::WorldPos::from_xz(x, z), 0);
            RegionCoord::new(c.x + k % 20 - 10, c.z + k / 20 - 10, 0)
        })
        .collect();
    let shore: Vec<RegionCoord> = first_best
        .map(|[bx, bz]| {
            (0..100)
                .map(|k| {
                    let c = RegionCoord::containing(strata_core::WorldPos::from_xz(bx, bz), 0);
                    RegionCoord::new(c.x + k % 10 - 5, c.z + k / 10 - 5, 0)
                })
                .collect()
        })
        .unwrap_or_default();
    for (name, cells) in [("inland", &inland), ("lake", &shore)] {
        for (which, t) in [("with reeds", &table), ("reeds off", &bare)] {
            let started = std::time::Instant::now();
            let mut plants = 0;
            for c in cells {
                plants += place_cell(*c, &world, t, Seed(42), &mut Vec::new())
                    .plants
                    .len();
            }
            println!(
                "placement, {name} {which}: {:.3} ms a cell over {} cells, {plants} plants",
                started.elapsed().as_secs_f64() * 1000.0 / cells.len().max(1) as f64,
                cells.len()
            );
        }
    }
    let kit = RoadsideKit {
        marker: Some("marker".to_owned()),
        lamp: Some("lamp".to_owned()),
    };
    for network in &networks {
        for (r, road) in network.roads().iter().enumerate() {
            let near = road
                .points
                .iter()
                .map(|p| (p.x - x).hypot(p.z - z))
                .fold(f64::INFINITY, f64::min);
            if near > 3_000.0 {
                continue;
            }
            let id = RoadId {
                mass: network.key(),
                road: u32::try_from(r).expect("a road index"),
            };
            let started = std::time::Instant::now();
            let pieces = place_roadside(id, road, &kit, None, &world);
            let took = started.elapsed().as_secs_f64() * 1000.0;
            let markers: Vec<_> = pieces.iter().filter(|p| p.file == "marker").collect();
            println!(
                "road {r} {:?}, {:.0} m long, {near:.0} m off: {} markers, {} lamps, placed in {took:.2} ms",
                road.class,
                road.length_m(),
                markers.len(),
                pieces.len() - markers.len()
            );
            for m in markers.iter().take(6) {
                println!(
                    "  marker at ({:.1}, {:.1}, {:.1}) yaw {:.2}",
                    m.at[0], m.at[1], m.at[2], m.yaw
                );
            }
        }
    }
}
