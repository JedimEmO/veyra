//! The collision files the content step writes beside the baked buildings
//! (ashlar `collision_file`) hold exactly the solids the `.ashlar` reads to,
//! which is what keeps the colliders physics builds identical now that no
//! reader parses the render file.
#![allow(clippy::unwrap_used, reason = "a test over baked files")]

use std::path::PathBuf;

use ashlar::collision_file::{self, CollisionFile};
use ashlar::{BakedBuilding, CookedBuilding, InstanceCollider};

fn generated() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated"))
}

/// A baked file's level-zero proxies as the file's own reader places them, from
/// either version of the file.
fn proxies(file: &std::path::Path) -> Vec<InstanceCollider> {
    let bytes = std::fs::read(file).unwrap();
    if CookedBuilding::is_cooked(&bytes) {
        return CookedBuilding::read(&bytes)
            .unwrap()
            .colliders()
            .into_iter()
            .map(|c| InstanceCollider {
                instance: c.instance,
                element: c.element,
                solid: c.solid,
            })
            .collect();
    }
    BakedBuilding::read(&bytes)
        .unwrap()
        .level(0)
        .unwrap()
        .colliders()
}

/// Every `.ashlar` under `dir`, with its collision file beside it.
fn buildings(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            buildings(&path, out);
        } else if path.extension().is_some_and(|e| e == "ashlar") {
            out.push(path);
        }
    }
}

#[test]
fn the_city_collides_with_the_same_solids_from_the_new_file() {
    let mut files = Vec::new();
    buildings(&generated().join("settlements/port_serein"), &mut files);
    assert!(
        files.len() > 30,
        "run `just content` first: {} buildings",
        files.len()
    );
    let mut solids = 0;
    for file in files {
        let path = file.to_str().unwrap();
        let old = proxies(&file);
        let new =
            CollisionFile::read(&std::fs::read(collision_file::path_for(path)).unwrap()).unwrap();
        assert_eq!(new.colliders, old, "{path}");
        solids += old.len();
    }
    assert!(solids > 100, "{solids} solids");
}

#[test]
fn a_site_group_and_a_crossing_kit_match_their_baked_files() {
    for dir in ["sites/veyra", "crossings/veyra", "buildings"] {
        let mut files = Vec::new();
        buildings(&generated().join(dir), &mut files);
        assert!(!files.is_empty(), "run `just content` first: {dir}");
        for file in files {
            let path = file.to_str().unwrap();
            let new = CollisionFile::read(&std::fs::read(collision_file::path_for(path)).unwrap())
                .unwrap();
            assert_eq!(new.colliders, proxies(&file), "{path}");
        }
    }
}

#[test]
fn the_version_names_the_file_as_the_content_step_does() {
    for path in ["a.ashlar", "sites/veyra/house-a.ashlar", "plain"] {
        assert_eq!(
            strata_app::sites::collision_file_of(path),
            collision_file::path_for(path)
        );
    }
}
