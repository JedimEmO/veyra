//! Print nearby ground-detail locations for reproducible close-up inspection.
use std::collections::BTreeMap;
use strata_core::{RegionCoord, Seed};
use strata_flora::{FloraTable, place_cell};
use strata_worldgen::ProceduralWorld;

fn main() {
    let registries =
        strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("shipped assets");
    let table = FloraTable::from_registries(&registries);
    let world = ProceduralWorld::new(Seed(42), &registries);
    let mut nearest = BTreeMap::new();
    for z in -582..=-578 {
        for x in -42..=-38 {
            let coord = RegionCoord::new(x, z, 0);
            let flora = place_cell(coord, &world, &table, Seed(42), &mut Vec::new());
            for plant in &flora.plants {
                let species = &table.species()[plant.species.0 as usize];
                if species.tree.form == "tree" {
                    continue;
                }
                let wx = f64::from(x) * 64.0 + f64::from(plant.x);
                let wz = f64::from(z) * 64.0 + f64::from(plant.z);
                let distance = (wx + 2528.0).powi(2) + (wz + 37088.0).powi(2);
                let entry = nearest
                    .entry(species.tree.form.as_str())
                    .or_insert((f64::INFINITY, String::new()));
                if distance < entry.0 {
                    *entry = (
                        distance,
                        format!(
                            "{}: x={wx:.2}, y={:.2}, z={wz:.2}, variant={}",
                            species.tree.form, plant.y, plant.variant
                        ),
                    );
                }
            }
        }
    }
    for (_, text) in nearest.values() {
        println!("{text}");
    }
}
