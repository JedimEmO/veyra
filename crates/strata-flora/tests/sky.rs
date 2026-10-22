//! The sky under crowns (lighting plan phase 3a): a plant under a tree's
//! crown sees less of the sky than one in the open, a lone plant sees all
//! of it, grass under a crown carries the shade in its record, and the
//! terms are the same bytes on every call.

mod common;

use strata_core::{RegionCoord, Seed};
use strata_data::{FloraId, Namespaced, WorldDefs};
use strata_flora::{FloraTable, Plant, PlantId, SKY_FLOOR, place_cell, sky_terms};

fn id(registries: &WorldDefs, name: &str) -> FloraId {
    registries
        .flora_id(&Namespaced::parse(name).expect("valid"))
        .expect("a shipped species")
}

fn plant(species: FloraId, slot: u32, x: f32, z: f32, scale: f32) -> Plant {
    Plant {
        id: PlantId {
            species_key: 7,
            cell: RegionCoord::new(0, 0, 0),
            slot: [slot, 0],
        },
        species,
        x,
        y: 10.0,
        z,
        yaw: 0.0,
        scale,
        variant: 0,
        sky: u8::MAX,
    }
}

#[test]
fn a_plant_under_a_crown_sees_less_sky_than_one_in_the_open() {
    let registries = common::registries();
    let table = FloraTable::from_registries(&registries);
    let oak = id(&registries, "strata:oak");
    let shrub = id(&registries, "strata:woodland_shrub");
    let tall = plant(oak, 1, 20.0, 20.0, 1.3);
    let mut plants = vec![
        tall,
        // Under the oak's crown, beside its trunk.
        plant(shrub, 2, 21.0, 20.0, 1.0),
        // A small oak in its shade.
        plant(oak, 3, 23.0, 20.0, 0.75),
        // Far out in the open.
        plant(shrub, 4, 60.0, 60.0, 1.0),
    ];
    let neighbours = plants.clone();
    sky_terms(&mut plants, &neighbours, &table);
    assert_eq!(
        plants[0].sky,
        u8::MAX,
        "the tallest sees past its neighbour"
    );
    assert!(plants[1].sky < 200, "under the crown: {}", plants[1].sky);
    assert!(plants[2].sky < u8::MAX, "in the tall one's shade");
    assert!(plants[1].sky < plants[2].sky, "deeper under it");
    assert_eq!(plants[3].sky, u8::MAX, "in the open");
    assert!(plants.iter().all(|p| f32::from(p.sky) >= SKY_FLOOR * 255.0));
}

#[test]
fn a_forest_shades_its_floor_and_its_grass_the_same_on_every_call() {
    let (world, table) = common::world_and_table(42);
    // The canopy fixture's woodland (`study::SCENES`).
    let coord = RegionCoord::new(-118, 335, 0);
    let mut scratch = Vec::new();
    let a = place_cell(coord, &world, &table, Seed(42), &mut scratch);
    let b = place_cell(coord, &world, &table, Seed(42), &mut scratch);
    assert_eq!(a, b);
    let shaded = a.plants.iter().filter(|p| p.sky < u8::MAX).count();
    // Open forest: a few trees a cell, and the bushes round them shaded
    // (9 of 52 when this was written), the rest in the open.
    assert!(
        shaded > 0 && shaded < a.plants.len(),
        "{shaded} of {} plants shaded in a forest",
        a.plants.len()
    );
    let under = a.grass.iter().filter(|g| g.crown_sky() < 1.0).count();
    assert!(
        under > 0 && under < a.grass.len(),
        "{under} of {} blades",
        a.grass.len()
    );
    assert!(
        a.grass
            .iter()
            .all(|g| usize::from(g.palette()) < strata_data::flora::MAX_GRASS_SPECIES)
    );
    assert!(a.grass.iter().all(|g| g.crown_sky() >= SKY_FLOOR));
}
