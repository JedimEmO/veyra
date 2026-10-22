//! The value budget (`assets/look/values.ron`) and the baked base maps agree.
//!
//! Every shipped material belongs to a family with a band of mean luminance
//! and a ceiling on its spread, as the tiling test holds the terrain's
//! repeats. A member out of band says so with `pending` and the step that
//! lands it; one that is out without saying fails, and so does one that says
//! so while in band, so the list only ever shrinks.
use std::path::Path;

use content::values::{baked_maps, stats_of_glb, stats_of_ktx2};
use strata_data::{ValueBudget, luminance};
use veyra_materials::TERRAIN;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");
const CHARACTER: &str = "characters/frontier_ranger";

/// The most a terrain layer's mean may be lifted by the shader's remap: past
/// it the texture's own detail is stretched to a different look and the
/// layer should be repainted instead.
const MAX_GAIN: f32 = 4.0;

fn budget() -> ValueBudget {
    let text = std::fs::read_to_string(format!("{ASSETS}/look/values.ron")).expect("the file");
    ValueBudget::parse(&text).expect("assets/look/values.ron is valid")
}

#[test]
fn every_terrain_set_has_a_value_target() {
    let budget = budget();
    let registries = strata_registry::load_from_dir(ASSETS).expect("the shipped assets load");
    for (_, name, def) in registries.materials.iter() {
        assert!(
            budget.terrain_target(&def.texture_set).is_some(),
            "{name}: terrain/{} is not in assets/look/values.ron",
            def.texture_set
        );
    }
    for (set, _) in TERRAIN {
        assert!(
            budget.materials.contains_key(&format!("terrain/{set}")),
            "terrain/{set} is baked but not in the budget"
        );
    }
}

#[test]
fn every_baked_base_map_is_in_its_families_band_or_says_why_not() {
    let generated = Path::new(GENERATED);
    if !generated.join("materials").is_dir() {
        eprintln!("generated/materials is missing: run `just content` to hold the budget");
        return;
    }
    let budget = budget();
    let mut maps = baked_maps(generated);
    let character = Path::new(ASSETS).join("characters/combat/frontier_ranger.glb");
    let mut failures = Vec::new();
    let mut table = String::new();
    let mut seen = Vec::new();
    for (key, file) in &maps {
        seen.push(key.clone());
        let stats = stats_of_ktx2(file).unwrap_or_else(|e| panic!("{key}: {e}"));
        judge(
            &budget,
            key,
            stats.mean,
            stats.spread,
            &mut table,
            &mut failures,
        );
    }
    if character.exists() {
        let stats = stats_of_glb(&character).expect("the character's texture");
        seen.push(CHARACTER.to_owned());
        judge(
            &budget,
            CHARACTER,
            stats.mean,
            stats.spread,
            &mut table,
            &mut failures,
        );
    }
    maps.clear();
    for key in budget.materials.keys() {
        if !seen.contains(key) && !key.starts_with("characters/") {
            failures.push(format!("{key}: in the budget but not baked (stale entry)"));
        }
    }
    eprintln!("{table}");
    assert!(
        failures.is_empty(),
        "the value budget:\n{}",
        failures.join("\n")
    );
}

fn judge(
    budget: &ValueBudget,
    key: &str,
    mean: f32,
    spread: f32,
    table: &mut String,
    failures: &mut Vec<String>,
) {
    use std::fmt::Write;
    let Some(member) = budget.materials.get(key) else {
        failures.push(format!("{key}: baked (mean {mean:.3}) but in no family"));
        return;
    };
    let family = &budget.families[&member.family];
    let target = budget.target_of(key).expect("a member has a target");
    let terrain = key.starts_with("terrain/");
    let (shown_mean, shown_spread) = if terrain {
        let gain = target / mean.max(1e-4);
        if gain > MAX_GAIN {
            failures.push(format!(
                "{key}: the runtime remap would lift {mean:.3} to {target:.3}, a gain of {gain:.1} (limit {MAX_GAIN})"
            ));
        }
        (target, spread * gain)
    } else {
        (mean, spread)
    };
    let out = !(family.min..=family.max).contains(&shown_mean) || shown_spread > family.spread;
    writeln!(
        table,
        "{key:36} {:10} mean {mean:.3} -> {shown_mean:.3} spread {shown_spread:.3}{}",
        member.family,
        if out { "  OUT" } else { "" }
    )
    .expect("a string");
    match (&member.pending, out) {
        (None, true) => failures.push(format!(
            "{key} ({}): mean {shown_mean:.3} / spread {shown_spread:.3} is outside {:.2} to {:.2} / {:.2}; fix it or mark it pending",
            member.family, family.min, family.max, family.spread
        )),
        (Some(why), false) => failures.push(format!(
            "{key}: marked pending ({why}) but now in band; drop `pending`"
        )),
        _ => {}
    }
}

#[test]
fn the_terrain_targets_are_in_their_bands() {
    // `ValueBudget::parse` holds a member's own target to its family; this
    // holds the family's target to the luminance the far colours are
    // derived at.
    let budget = budget();
    for set in TERRAIN.iter().map(|(set, _)| set) {
        let target = budget.terrain_target(set).expect("a target");
        let far = strata_data::far_colour([0.5, 0.6, 0.3], target);
        assert!((luminance(far.map(strata_data::srgb_to_linear)) - target).abs() < 1e-4);
    }
}

#[test]
fn the_flora_remap_reaches_its_target_inside_the_spread() {
    use content::values::{Stats, remap_for};
    // A dark leaf rising: the scale is held down by the family's spread and
    // the rest of the lift is a floor, so the mean still lands on target.
    let fir = remap_for(
        Stats {
            mean: 0.036,
            spread: 0.02,
        },
        0.15,
        0.06,
    );
    assert!(fir.scale < 0.15 / 0.036);
    assert!(fir.floor > 0.0);
    assert!((fir.scale * 0.036 + fir.floor - 0.15).abs() < 1e-5);
    assert!(fir.scale * 0.02 <= 0.06);
    // A pale bark coming down is a plain scale with no floor.
    let birch = remap_for(
        Stats {
            mean: 0.594,
            spread: 0.156,
        },
        0.19,
        0.1,
    );
    assert!(birch.scale < 1.0);
    assert!(birch.floor.abs() < 1e-6);
}
