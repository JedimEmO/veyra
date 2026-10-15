//! The roadside kit (ticket 0005, step 8) as the content step bakes it:
//! every piece binds a material the shipped library holds, and the layout
//! names each baked file once, beside the crossings' kit.

use strata_worldgen::roadside::ROADSIDE_PATH;

#[test]
fn every_piece_binds_a_shipped_material_and_is_named() {
    let library = content::definitions();
    let (files, kit) = content::roadside().expect("the kit builds");
    for (path, building) in &files {
        for (instance, slot, binding) in building.bindings() {
            assert!(
                library.materials.contains_key(&binding.material),
                "{path}: {instance} binds {} to {slot}, which the library does not hold",
                binding.material
            );
        }
    }
    let dir = ROADSIDE_PATH.rsplit_once('/').expect("a directory").0;
    let baked: Vec<&String> = files.iter().map(|(p, _)| p).collect();
    assert_eq!(kit.files().len(), files.len(), "one file a piece");
    assert!(kit.marker.is_some() && kit.lamp.is_some());
    for file in kit.files() {
        assert!(file.starts_with(dir), "{file}");
        assert!(baked.contains(&&file), "{file} is named and not baked");
    }
}

/// Writes the roadside's files and their layout into `generated/` alone,
/// as `just content` would: for looking at a change to the kit in the game
/// without baking everything else again.
#[test]
#[ignore = "writes generated/: by hand, while iterating on the kit"]
fn rebake_roadside() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");
    let content = ashlar_content::Content::new(root);
    let (files, kit) = content::roadside().expect("the kit builds");
    for (path, building) in &files {
        content
            .write_building(path, building, &content::ladder())
            .expect("written");
    }
    content
        .write_file(
            ROADSIDE_PATH,
            ron::ser::to_string_pretty(&kit, ron::ser::PrettyConfig::default())
                .expect("serialises")
                .as_bytes(),
        )
        .expect("written");
}
