//! The crossings' kit (ticket 0005, step 7) as the content step bakes it:
//! every piece binds a material the shipped library holds, and the layout
//! the world stands crossings from names every file once and leaves no gap
//! between one span class and the next.

use strata_worldgen::crossings::KIT_PATH;
use strata_worldgen::roads::RoadClass;

#[test]
fn every_piece_binds_a_shipped_material() {
    let library = content::definitions();
    let (files, _) = content::crossings().expect("the kit builds");
    for (path, building) in &files {
        for (instance, slot, binding) in building.bindings() {
            assert!(
                library.materials.contains_key(&binding.material),
                "{path}: {instance} binds {} to {slot}, which the library does not hold",
                binding.material
            );
        }
    }
}

#[test]
fn the_layout_names_every_file_and_tiles_the_gaps() {
    let (files, kit) = content::crossings().expect("the kit builds");
    let dir = KIT_PATH.rsplit_once('/').expect("a directory").0;
    let baked: Vec<&String> = files.iter().map(|(p, _)| p).collect();
    assert_eq!(kit.files().len(), files.len(), "one file a piece");
    for file in kit.files() {
        assert!(file.starts_with(dir), "{file}");
        assert!(baked.contains(&&file), "{file} is named and not baked");
    }
    for road in [RoadClass::Service, RoadClass::Track, RoadClass::Trail] {
        let mut classes: Vec<_> = kit.bridges.iter().filter(|b| b.road == road).collect();
        classes.sort_by(|a, b| a.clear_m.total_cmp(&b.clear_m));
        assert!(classes.len() >= 8, "{road:?}");
        for pair in classes.windows(2) {
            assert!((pair[0].gaps_m[1] - pair[1].gaps_m[0]).abs() < 1e-9);
        }
        // Every gap in range has the class that serves it, and no class
        // is shorter than its gap between its faces nor shorter overall.
        let (lo, hi) = (classes[0].gaps_m[0], classes[classes.len() - 1].gaps_m[1]);
        let mut gap = lo;
        while gap <= hi {
            let class = kit.bridge_for(road, gap).expect("a class");
            assert!(class.clear_m <= gap + 1.0 && class.length_m > gap + 1.0);
            gap += 0.1;
        }
        if road != RoadClass::Trail {
            assert!(kit.piers.iter().any(|p| p.road == road));
        }
    }
    // Headwalls in more than one box height, each standing taller than its
    // box, so the world can fit one under a low road.
    assert!(kit.headwalls.len() >= 2);
    for h in &kit.headwalls {
        assert!(h.top_m > h.box_m, "{h:?}");
    }
}

/// Writes the crossings' files and their layout into `generated/` alone, as
/// `just content` would: for looking at a change to the kit in the game
/// without baking everything else again.
#[test]
#[ignore = "writes generated/: by hand, while iterating on the kit"]
fn rebake_crossings() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");
    let content = ashlar_content::Content::new(root);
    let (files, kit) = content::crossings().expect("the kit builds");
    for (path, building) in &files {
        content
            .write_building(path, building, &content::ladder())
            .expect("written");
    }
    content
        .write_file(
            strata_worldgen::crossings::KIT_PATH,
            ron::ser::to_string_pretty(&kit, ron::ser::PrettyConfig::default())
                .expect("serialises")
                .as_bytes(),
        )
        .expect("written");
}
