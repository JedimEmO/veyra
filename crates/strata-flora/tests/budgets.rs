//! Exercise the renderer contract with dense, hand-built placement inputs.

use strata_core::{RegionCoord, Seed};
use strata_flora::{FloraTable, GRID, HeightGrid, place_on_grid};
use strata_registry::flora::MAX_GRASS_BLADES_PER_CELL;
use strata_registry::{BiomeId, FloraFit, FloraId, FloraKind};
use strata_worldgen::Sample;

#[test]
fn dense_grass_thins_all_species_and_uses_grass_only_palette_slots() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("shipped registry");
    let original = FloraTable::from_registries(&registries);
    let template = original
        .species()
        .iter()
        .find(|s| s.kind == FloraKind::Grass)
        .expect("grass");
    let species = (0..2)
        .map(|index| {
            let mut species = template.clone();
            // Large global IDs must not leak into the compact grass-only palette.
            species.id = FloraId(40 + index);
            species.species_seed = u64::from(index);
            species.density_per_m2 = 8.0;
            species.altitude_m = (0.0, 100.0);
            species.moisture = (0.0, 1.0);
            species.sea_clearance_m = 0.0;
            // Force every habitat test to pass, independently of patch noise.
            // This deliberately exercises the hard ceiling through the test seam.
            species.fit = FloraFit::from_weights(vec![1000.0]);
            species
        })
        .collect();
    let flora = FloraTable::from_species(species);
    let samples = vec![
        Sample {
            height: 10.0,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 1.0,
            cover: 1.0,
            landform: strata_worldgen::Landform::default(),
            water: None,
        };
        GRID * GRID
    ];
    let grid = HeightGrid::new(&samples);
    let coord = RegionCoord::new(-1, 2, 0);
    let first = place_on_grid(coord, &grid, 0.0, &flora, Seed(42));
    let second = place_on_grid(coord, &grid, 0.0, &flora, Seed(42));
    assert_eq!(first.grass, second.grass);
    assert_eq!(first.grass.len(), MAX_GRASS_BLADES_PER_CELL);
    let counts = [0, 1].map(|palette| first.grass.iter().filter(|b| b.species == palette).count());
    assert_eq!(counts, [MAX_GRASS_BLADES_PER_CELL / 2; 2]);
}
