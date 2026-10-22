//! The far ground is the near ground's value (review step six, visual.md F2).
//!
//! The terrain mixes to the biome's far colour past 400 m. Authored as a
//! second colour it was 0.42 to 0.61 luminance against 0.05 to 0.09 for the
//! layers drawn near, so every elevated view had a lit band at the horizon.
//! It is now the ground's hue at the luminance of the biome's surface layer.

use strata_registry::luminance;
use strata_terrain::BiomeLayers;

#[test]
fn every_biome_far_colour_is_within_a_sixth_of_its_near_layer_budget() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let layers = BiomeLayers::from_registries(&registries);
    let mut checked = 0;
    for id in registries.biome_ids() {
        let Some(surface) = registries.surface(id) else {
            continue;
        };
        let def = registries
            .materials
            .get(surface.material)
            .expect("a material");
        let Some(target) = registries.values.terrain_target(&def.texture_set) else {
            continue;
        };
        let far = luminance(layers.tint(id).map(strata_registry::srgb_to_linear));
        assert!(
            (far - target).abs() <= 0.15 * target,
            "{id:?}: far luminance {far:.3} against the {} layer's budget {target:.3}",
            def.texture_set
        );
        checked += 1;
    }
    assert!(checked >= 6, "the shipped biomes are held: {checked}");
}
