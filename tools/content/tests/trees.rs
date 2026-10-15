//! The tree recipes, the flora registry and the material library agree.
//!
//! The content step bakes a tree per recipe and the game places it by the
//! registry's species id. A recipe for a species the registry does not have
//! is baked and never placed; a registry tree with no recipe is placed and
//! never drawn. Neither fails anywhere else, so it fails here.
use std::path::Path;

use content::definitions;
use strata_registry::FloraKind;

fn registries() -> strata_registry::GameRegistries {
    strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("the shipped assets load")
}

#[test]
fn every_recipe_is_a_registry_species_with_its_bark_and_variant_count() {
    let registries = registries();
    for recipe in veyra_flora::recipes() {
        let (_, _, def) = registries
            .flora
            .iter()
            .find(|(_, _, def)| def.id.as_ref() == recipe.species)
            .unwrap_or_else(|| panic!("{} is baked but not in the registry", recipe.species));
        assert!(
            matches!(def.kind, FloraKind::Tree | FloraKind::Bush),
            "{} is not a tree or a bush",
            recipe.species
        );
        assert_eq!(
            u32::from(def.tree.variants),
            veyra_flora::VARIANTS,
            "{}: the registry's variant count is the baked one",
            recipe.species
        );
        assert_eq!(def.tree.bark, recipe.bark, "{}", recipe.species);
    }
}

#[test]
fn every_registry_tree_has_a_recipe() {
    let registries = registries();
    let recipes = veyra_flora::recipes();
    for (_, _, def) in registries.flora.iter() {
        if def.kind != FloraKind::Tree {
            continue;
        }
        assert!(
            recipes.iter().any(|r| r.species == def.id.as_ref()),
            "{} is placed but nothing bakes it",
            def.id.as_ref()
        );
    }
}

#[test]
fn every_material_a_tree_binds_is_baked() {
    let library = definitions();
    for recipe in veyra_flora::recipes() {
        for key in [recipe.bark_material(), recipe.leaf_material()] {
            assert!(library.materials.contains_key(&key), "{key} is not baked");
        }
    }
}

#[test]
fn the_forest_mixes_at_least_three_tree_species() {
    let registries = registries();
    let trees = registries
        .flora
        .iter()
        .filter(|(_, _, def)| def.kind == FloraKind::Tree)
        .filter(|(_, _, def)| {
            def.biomes
                .iter()
                .any(|b| b.biome.as_ref() == "strata:forest" && b.weight > 0.0)
        })
        .count();
    assert!(trees >= 3, "the forest has {trees} tree species");
}

#[test]
fn no_photographed_texture_is_shipped() {
    fn walk(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("readable assets") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
            {
                found.push(path.display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    walk(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")),
        &mut found,
    );
    assert!(found.is_empty(), "every texture is generated: {found:?}");
}

#[test]
fn every_rock_recipe_is_a_registry_rock_and_every_registry_rock_is_baked() {
    let registries = registries();
    let recipes = veyra_rocks::recipes();
    for recipe in &recipes {
        let (_, _, def) = registries
            .flora
            .iter()
            .find(|(_, _, def)| def.id.as_ref() == recipe.species)
            .unwrap_or_else(|| panic!("{} is baked but not in the registry", recipe.species));
        assert_eq!(def.kind, FloraKind::Rock, "{}", recipe.species);
    }
    for (_, _, def) in registries.flora.iter() {
        if def.kind == FloraKind::Rock {
            assert!(
                recipes.iter().any(|r| r.species == def.id.as_ref()),
                "{} is placed but nothing bakes it",
                def.id.as_ref()
            );
        }
    }
}

#[test]
fn every_material_a_rock_binds_is_baked() {
    let library = definitions();
    for recipe in veyra_rocks::recipes() {
        let key = recipe.material_key();
        assert!(library.materials.contains_key(&key), "{key} is not baked");
    }
}
