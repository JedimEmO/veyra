//! The site kinds' building groups and the shipped registry agree.
//!
//! A kind names its group and gives its pad's radius; the content step lays
//! the group out for a radius of its own. If a group is missing, or reaches
//! past its kind's pad, or leaves no way in from some side, every site of
//! the kind is empty or blocks its road, with nothing failing.

use strata_registry::SiteKindId;
use strata_worldgen::sites::{
    FIT_MARGIN_M, ProceduralSite, SiteGroupLayout, SiteId, group_layout_path, place_group,
};

fn assets() -> &'static std::path::Path {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

/// Every kind has its group, and every arrangement of it fits the kind's
/// own pad.
#[test]
fn every_kind_has_a_group_that_fits_its_pad() {
    let registries = strata_registry::load_from_dir(assets()).expect("the shipped assets load");
    let (_, layouts) = content::site_groups(&[]).expect("the groups build");
    assert!(registries.sites.len() >= 6);
    for (_, name, def) in registries.sites.iter() {
        let layout = layouts
            .iter()
            .find(|l| l.group == def.group)
            .unwrap_or_else(|| panic!("{name} names group {}, which is not made", def.group));
        assert!(
            group_layout_path(&layout.group).starts_with("sites/"),
            "{}",
            layout.group
        );
        for arrangement in &layout.arrangements {
            assert!(
                arrangement.reach_m() <= def.footprint_m - FIT_MARGIN_M,
                "{name}: {} reaches {:.1} m on a {} m pad",
                arrangement.name,
                arrangement.reach_m(),
                def.footprint_m
            );
        }
    }
}

/// A site of the kind with its road arriving at `angle` round the pad.
fn site(def: &strata_registry::SiteKindDef, angle: f64, node: i32) -> ProceduralSite {
    let mut site = ProceduralSite {
        id: SiteId {
            kind: SiteKindId(0),
            mass: [0, 0],
            node: [node, 0],
        },
        name: "Test".to_owned(),
        centre: [0.0, 0.0],
        level_m: 10.0,
        radius_m: def.footprint_m,
        blend_m: def.blend_m,
        crown: def.crown,
        facing: [0.0, 1.0],
        arrival: [0.0; 2],
        by: None,
        score: 1.0,
        cut_m: 0.0,
    };
    let (s, c) = angle.sin_cos();
    site.arrival = site.to_world_xz([def.footprint_m * s, def.footprint_m * c]);
    site
}

/// From whichever of sixteen directions the road arrives, some arrangement
/// of the group keeps the way in to the yard clear, and every placement
/// stands its name board where a group has one.
#[test]
fn a_road_from_any_side_finds_a_way_in() {
    let registries = strata_registry::load_from_dir(assets()).expect("the shipped assets load");
    let (_, layouts) = content::site_groups(&[]).expect("the groups build");
    let mut blocked = Vec::new();
    for (id, name, def) in registries.sites.iter() {
        let layout: &SiteGroupLayout = layouts
            .iter()
            .find(|l| l.group == def.group)
            .expect("the group");
        let road = registries.site_kind(id).expect("resolved").road;
        for step in 0..16 {
            let angle = std::f64::consts::TAU * f64::from(step) / 16.0;
            let placed = place_group(&site(def, angle, step), road, layout).expect("a placement");
            if !placed.way_in_clear {
                blocked.push(format!("{name} way in from {:.0}", angle.to_degrees()));
            }
            if placed.sign.is_some() != layout.sign.is_some() {
                blocked.push(format!("{name} board from {:.0}", angle.to_degrees()));
            }
        }
    }
    assert!(blocked.is_empty(), "{blocked:#?}");
}

/// Every file a layout stands is one the content step bakes.
#[test]
fn every_file_a_group_stands_is_baked() {
    let baked = content::buildings().expect("the buildings build");
    let (files, layouts) = content::site_groups(&baked).expect("the groups build");
    let written: std::collections::BTreeSet<&str> = baked
        .iter()
        .chain(&files)
        .map(|(path, _)| path.as_str())
        .collect();
    for layout in &layouts {
        let stood = layout
            .arrangements
            .iter()
            .flat_map(|a| a.buildings.iter().map(|b| b.file.as_str()))
            .chain(layout.sign.as_ref().map(|s| s.file.as_str()));
        for file in stood {
            assert!(written.contains(file), "{} stands {file}", layout.group);
        }
    }
}
