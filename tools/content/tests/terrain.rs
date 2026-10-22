//! The terrain registry and the baked terrain layers agree.
//!
//! The registry says how many metres a layer's repeat covers; the layer's
//! graph was drawn for one repeat of its own size. If the two drift, every
//! stone and tuft on the ground is the wrong size, with nothing failing.
use std::collections::BTreeMap;

use content::definitions;
use veyra_materials::TERRAIN;

#[test]
fn every_terrain_material_is_baked_at_the_repeat_the_registry_tiles_it() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let sets: BTreeMap<&str, f32> = TERRAIN.iter().copied().collect();
    let library = definitions();
    for (_, name, def) in registries.materials.iter() {
        let metres = sets
            .get(def.texture_set.as_str())
            .unwrap_or_else(|| panic!("{name} names {}, which is not baked", def.texture_set));
        assert!(
            (def.tiling_m - metres).abs() < 1e-6,
            "{name} tiles every {} m but its set is drawn for {metres} m",
            def.tiling_m
        );
        let definition = &library.materials[&format!("terrain:{}", def.texture_set)];
        assert!(
            definition
                .tile_metres
                .iter()
                .all(|side| (side - metres).abs() < 1e-6),
            "{name}: its definition lays {:?}",
            definition.tile_metres
        );
    }
}

#[test]
fn every_terrain_set_bakes_maps_of_its_own() {
    // The content step bakes one set per distinct graph and parameters, named
    // after the first definition that asks for it. The terrain loads each
    // set from `terrain/<set>/`, so no two sets may share a bake.
    let library = definitions();
    let mut seen = BTreeMap::new();
    for (set, _) in TERRAIN {
        let definition = &library.materials[&format!("terrain:{set}")];
        let ashlar::Surface::Graph(bake) = &definition.surface else {
            panic!("terrain:{set} is not a graph");
        };
        let key = format!("{}{:?}{}", bake.graph, bake.params, bake.resolution);
        if let Some(other) = seen.insert(key, set) {
            panic!("terrain:{set} bakes the same maps as terrain:{other}");
        }
    }
}

/// The shader repeats the flagstones' course layout to tell one flag from the
/// next (`laid` in the registry); it must be the layout the graph draws.
#[test]
fn the_flagstones_layout_in_the_registry_is_the_graphs() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let ([rows, stones], joint) = veyra_materials::terrain::FLAGSTONES_LAYOUT;
    let (_, _, def) = registries
        .materials
        .iter()
        .find(|(_, _, def)| def.texture_set == "flagstones")
        .expect("the flagstones are registered");
    let laid = def.laid.expect("the flagstones carry their layout");
    assert_eq!((laid.rows, laid.stones), (rows, stones));
    assert!((laid.joint - joint).abs() < 1e-6, "{}", laid.joint);
}
