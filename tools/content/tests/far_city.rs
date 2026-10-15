//! The city past 250 m: its buildings' farthest level and its skyline, held
//! to budgets, and the skyline standing where the game stands each building.
//!
//! The far city costs the renderer by its pieces, so a kit that adds a
//! district must fail here and not in the ledger. And the skyline is a
//! second placement of every building, so a sign wrong in it would put a
//! mirrored city on the horizon with nothing failing.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use ashlar::glam::{DQuat, DVec3};
use ashlar::{BakedBuilding, Building, MeshedBuilding};
use ashlar_manifold::{ManifoldMesher, mesh_building};
use content::{budget, building_stats};
use veyra_settlements::skyline;

/// Port Serein as the content step bakes it, and its skyline baked as it
/// ships: built once for every test in this file.
struct City {
    buildings: Vec<(String, Building)>,
    layout: strata_worldgen::SettlementLayout,
    site: strata_worldgen::Site,
    skyline: BakedBuilding,
    /// The library the skyline's flattened bindings name: the shipped
    /// definitions and one per override, each its base's with the
    /// parameters written in, the emissive colour among them.
    flattened: ashlar::MaterialLibrary,
}

fn city() -> &'static City {
    static CITY: OnceLock<City> = OnceLock::new();
    CITY.get_or_init(|| {
        let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let registries = strata_registry::load_from_dir(assets).expect("the shipped assets load");
        let def = registries
            .settlements
            .iter()
            .find(|s| s.id.as_ref() == veyra_settlements::PORT_SEREIN)
            .expect("Port Serein is registered");
        let (buildings, layout, plan) = content::settlement(assets).expect("the plan builds");
        let recipe = content::skyline(&plan).expect("the skyline validates");
        let skyline = ashlar_manifold::bake(&recipe, &skyline::LADDER, &ManifoldMesher::default())
            .expect("the skyline meshes");
        let definitions = content::definitions();
        let unflattened = skyline::skyline(&plan, &definitions).expect("the skyline validates");
        let (_, flattened) = ashlar_content::flatten_overrides(&[unflattened], &definitions)
            .expect("the overrides flatten");
        City {
            buildings,
            layout,
            site: strata_worldgen::Site::place(def, &|_, _| 0.0),
            skyline,
            flattened,
        }
    })
}

/// A box round points, `[low, high]`.
fn grow(bounds: &mut Option<[DVec3; 2]>, point: DVec3) {
    let [low, high] = bounds.get_or_insert([point, point]);
    *low = low.min(point);
    *high = high.max(point);
}

/// Where the game draws a point of a building standing at `position` with
/// `yaw`, both in the settlement's frame: at `Site::to_world` of the
/// position, turned by `from_rotation_y` of `Site::world_yaw(yaw)`, the two
/// functions `PlacedSettlement::to_world` and `world_yaw` are.
fn in_world(site: &strata_worldgen::Site, position: [f64; 3], yaw: f64, point: DVec3) -> DVec3 {
    DVec3::from_array(site.to_world(position)) + DQuat::from_rotation_y(site.world_yaw(yaw)) * point
}

/// The world bounds of every non-empty piece of `meshed`, placed as the
/// game places a building at `position` with `yaw`.
fn placed_bounds(
    site: &strata_worldgen::Site,
    meshed: &MeshedBuilding,
    position: [f64; 3],
    yaw: f64,
) -> Option<[DVec3; 2]> {
    let mut bounds = None;
    for piece in meshed.pieces() {
        if piece.mesh.indices.is_empty() {
            continue;
        }
        for &p in &piece.mesh.positions {
            grow(
                &mut bounds,
                in_world(site, position, yaw, piece.pose.transform_point(p)),
            );
        }
    }
    bounds
}

/// The world bounds of what the skyline draws for each source building, by
/// its name: every triangle is traced to the building it came from through
/// its face's operand, whose instance id is the building's name first.
fn skyline_bounds(city: &City) -> BTreeMap<String, [DVec3; 2]> {
    let level = city.skyline.level(0).expect("one level");
    let mut bounds: BTreeMap<String, Option<[DVec3; 2]>> = BTreeMap::new();
    for group in &level.groups {
        for batch in &group.batches {
            let mesh = &batch.mesh;
            for (triangle, source) in mesh.indices.as_chunks::<3>().0.iter().zip(&mesh.sources) {
                let operand = &group.operands[source.operand as usize];
                let name = skyline::source_of(&operand.instance).expect("a prefixed instance");
                let entry = bounds.entry(name.to_owned()).or_default();
                for &i in triangle {
                    // The skyline stands at the frame's origin, turned by
                    // the frame's yaw: a building at the origin with no yaw.
                    let point = mesh.positions[i as usize];
                    grow(entry, in_world(&city.site, [0.0; 3], 0.0, point));
                }
            }
        }
    }
    bounds
        .into_iter()
        .filter_map(|(name, b)| Some((name, b?)))
        .collect()
}

fn name_of(file: &str) -> &str {
    file.rsplit('/')
        .next()
        .and_then(|stem| stem.strip_suffix(".ashlar"))
        .expect("a baked file")
}

#[test]
fn the_far_city_is_within_budget() {
    let city = city();
    let far = *content::ladder().last().expect("a ladder");
    let cell = far.cell.expect("the farthest level is joined into cells");
    let mut total = 0;
    let mut largest = Vec::new();
    for (file, building) in &city.buildings {
        // What `bake` stores for the level: the building simplified by its
        // policy, meshed, and joined by cell, binding and side.
        let Some(simple) = building.simplified(&far).expect("simplifies") else {
            continue;
        };
        let meshed = mesh_building(&simple, &ManifoldMesher::default()).expect("meshes");
        let pieces: usize = meshed
            .concatenated(cell)
            .expect("joins")
            .iter()
            .map(|group| group.batches.len())
            .sum();
        total += pieces;
        largest.push((pieces, name_of(file).to_owned()));
    }
    largest.sort_unstable_by(|a, b| b.cmp(a));
    println!("the far city: {total} pieces; the largest {largest:?}");
    assert!(
        total <= budget::SETTLEMENT_FAR_PIECES,
        "the far city is {total} pieces; the largest: {:?}",
        &largest[..largest.len().min(10)]
    );
}

#[test]
fn the_skyline_collides_with_nothing() {
    let city = city();
    let stats = building_stats(content::SETTLEMENT_SKYLINE, &city.skyline);
    assert_eq!(stats.levels.len(), 1);
    assert_eq!(stats.colliders, 0, "the skyline would collide");
    assert!(
        city.skyline.part_colliders.values().all(Vec::is_empty),
        "the skyline carries collision proxies"
    );
    assert!(
        city.skyline.part_portals.values().all(Vec::is_empty),
        "the skyline carries portals"
    );
    assert_eq!(
        city.layout.skyline.as_deref(),
        Some(content::SETTLEMENT_SKYLINE)
    );
    assert!(
        city.layout
            .buildings
            .iter()
            .all(|b| b.file != content::SETTLEMENT_SKYLINE),
        "the skyline is not a building the physics collides with"
    );
    // Not in the buildings' directory, where a building named for it would
    // be written over by it.
    let directory = |file: &str| std::path::Path::new(file).parent().map(ToOwned::to_owned);
    assert!(
        city.layout
            .buildings
            .iter()
            .all(|b| directory(&b.file) != directory(content::SETTLEMENT_SKYLINE)),
        "the skyline shares a directory with the buildings"
    );
}

/// The skyline brings no material of its own: every key it binds is one a
/// shipped building binds once its overrides are flattened, so the library
/// the content step bakes for the buildings dresses it.
#[test]
fn the_skyline_binds_only_what_the_buildings_bind() {
    let city = city();
    let mut shipped: Vec<Building> = content::buildings()
        .expect("the buildings build")
        .into_iter()
        .map(|(_, building)| building)
        .collect();
    shipped.extend(city.buildings.iter().map(|(_, building)| building.clone()));
    let (flat, _) = ashlar_content::flatten_overrides(&shipped, &content::definitions())
        .expect("the overrides flatten");
    let bound: std::collections::BTreeSet<&str> = flat
        .iter()
        .flat_map(|building| building.bindings().map(|(_, _, b)| b.material.as_str()))
        .collect();
    let skyline = city.skyline.building();
    let mut keys = 0;
    for (instance, slot, binding) in skyline.bindings() {
        assert!(
            bound.contains(binding.material.as_str()),
            "the skyline's {instance} binds {} to {slot}, which no building binds",
            binding.material
        );
        keys += 1;
    }
    assert!(keys > 0, "a skyline that binds nothing");
}

#[test]
fn the_skyline_is_within_budget() {
    let city = city();
    let stats = building_stats(content::SETTLEMENT_SKYLINE, &city.skyline);
    let level = &stats.levels[0];
    assert!(
        level.pieces <= budget::SKYLINE_PIECES,
        "the skyline is {} pieces",
        level.pieces
    );
    assert!(
        level.drawn_triangles <= budget::SKYLINE_TRIANGLES,
        "the skyline is {} triangles",
        level.drawn_triangles
    );
}

/// Every turned building of the skyline stands where the game stands its
/// own file.
///
/// Compared in the world, through the site's own frame functions, for each
/// building with a yaw: the skyline's triangles traced back to it must fill
/// the box that building simplified by the skyline's policy fills when the
/// game places it, to rounding. Against the building's own farthest level
/// the skyline may only be smaller, and by less than its feature size on
/// every side: what the coarser policy drops is an element between 1.5 m
/// and 4 m standing proud of the rest, never a displacement (on
/// 2026-09-28 the most was 1.3 m, off the beacon's top). The same buildings turned the other way must not fit, or the
/// test could not tell the sign of a yaw.
#[test]
fn the_skyline_stands_where_the_game_places_each_building() {
    let city = city();
    let skyline = skyline_bounds(city);
    let far = *content::ladder().last().expect("a ladder");
    let kernel = ManifoldMesher::default();
    let mut turned = 0;
    let mut mirrored_misfits = 0;
    for ((file, building), placed) in city.buildings.iter().zip(&city.layout.buildings) {
        assert_eq!(file, &placed.file);
        if placed.yaw.abs() < 1e-6 {
            continue;
        }
        let name = name_of(file);
        let drawn = skyline
            .get(name)
            .unwrap_or_else(|| panic!("{name} has a yaw and nothing in the skyline"));
        let simple = skyline::prepared(building, &skyline::POLICY, &content::definitions())
            .expect("simplifies")
            .expect("drawn in the skyline, so something survives");
        let meshed = mesh_building(&simple, &kernel).expect("meshes");
        let own = placed_bounds(&city.site, &meshed, placed.position, placed.yaw)
            .expect("something to draw");
        let apart = (drawn[0] - own[0])
            .abs()
            .max((drawn[1] - own[1]).abs())
            .max_element();
        assert!(
            apart < 1e-6,
            "{name}: the skyline draws {drawn:?}, its own file placed {own:?}"
        );
        let mirrored = placed_bounds(&city.site, &meshed, placed.position, -placed.yaw)
            .expect("something to draw");
        let apart = (drawn[0] - mirrored[0])
            .abs()
            .max((drawn[1] - mirrored[1]).abs())
            .max_element();
        if apart > 0.5 {
            mirrored_misfits += 1;
        }
        let coarse = mesh_building(
            &building
                .simplified(&far)
                .expect("simplifies")
                .expect("a far level"),
            &kernel,
        )
        .expect("meshes");
        let coarse = placed_bounds(&city.site, &coarse, placed.position, placed.yaw)
            .expect("something to draw");
        let low = coarse[0] - drawn[0];
        let high = drawn[1] - coarse[1];
        println!(
            "{name} yaw {:.3}: skyline {:.1?} to {:.1?}, farthest level {:.1?} to {:.1?}",
            placed.yaw,
            drawn[0].to_array(),
            drawn[1].to_array(),
            coarse[0].to_array(),
            coarse[1].to_array()
        );
        assert!(
            low.max_element() < 0.01 && high.max_element() < 0.01,
            "{name}: the skyline reaches past its farthest level's box, by {low:?} low and {high:?} high"
        );
        // What the skyline drops is thinner than its feature size, so no
        // side of the box moves in by more.
        let shrunk = (-low).max(-high).max_element();
        assert!(
            shrunk < skyline::POLICY.min_feature,
            "{name}: the skyline's box is {shrunk:.2} m inside its farthest level's"
        );
        turned += 1;
    }
    assert!(turned >= 3, "only {turned} turned buildings to test with");
    assert!(
        mirrored_misfits > 0,
        "turning the other way fits as well, so the test cannot tell the sign"
    );
}

/// The area of one triangle of a mesh, square metres. Poses only turn and
/// move, so a triangle's area is the same wherever it is placed.
fn triangle_area(mesh: &ashlar::TriangleMesh, triangle: [u32; 3]) -> f64 {
    let [a, b, c] = triangle.map(|i| mesh.positions[i as usize]);
    0.5 * (b - a).cross(c - a).length()
}

/// The area of a whole mesh, square metres.
fn area(mesh: &ashlar::TriangleMesh) -> f64 {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&triangle| triangle_area(mesh, triangle))
        .sum()
}

/// The city is as lit from the bay as it is close by: for each source
/// building, what the skyline draws under glowing bindings covers at least
/// nine tenths of what the building's own farthest level draws under them.
///
/// By area and not by count, since the two are meshed differently: the
/// skyline's chamfered boxes are plain and its segments are the policy's.
/// On 2026-09-29 the skyline kept a seventh of the housing slabs' lit slits
/// and a fortieth of the market's shop fronts, and the city's lights came on
/// all at once when it handed over to its buildings.
#[test]
fn the_skyline_is_as_lit_as_the_farthest_level() {
    let city = city();
    let definitions = content::definitions();
    let level = city.skyline.level(0).expect("one level");
    let mut drawn: BTreeMap<String, f64> = BTreeMap::new();
    for group in &level.groups {
        for batch in &group.batches {
            if !skyline::glows(&city.flattened, &batch.binding) {
                continue;
            }
            let mesh = &batch.mesh;
            for (triangle, source) in mesh.indices.as_chunks::<3>().0.iter().zip(&mesh.sources) {
                let operand = &group.operands[source.operand as usize];
                let name = skyline::source_of(&operand.instance).expect("a prefixed instance");
                *drawn.entry(name.to_owned()).or_default() += triangle_area(mesh, *triangle);
            }
        }
    }
    let far = *content::ladder().last().expect("a ladder");
    let kernel = ManifoldMesher::default();
    let mut lit = 0;
    let mut short = Vec::new();
    for (file, building) in &city.buildings {
        let Some(simple) = building.simplified(&far).expect("simplifies") else {
            continue;
        };
        let meshed = mesh_building(&simple, &kernel).expect("meshes");
        let own: f64 = meshed
            .pieces()
            .filter(|piece| skyline::glows(&definitions, piece.binding))
            .map(|piece| area(piece.mesh))
            .sum();
        if own <= 0.0 {
            continue;
        }
        lit += 1;
        let name = name_of(file);
        let kept = drawn.get(name).copied().unwrap_or_default();
        println!("{name}: lit {own:.0} m2 at the farthest level, {kept:.0} m2 in the skyline");
        if kept < 0.9 * own {
            short.push(format!("{name}: {kept:.0} of {own:.0} m2"));
        }
    }
    assert!(lit >= 5, "only {lit} lit buildings to test with");
    assert!(
        short.is_empty(),
        "the skyline is darker than the buildings it stands for: {short:?}"
    );
}

/// The skyline keeps what glows and what reads as a line as the buildings'
/// farthest level keeps it, so the two must be one policy.
#[test]
fn the_skyline_simplifies_what_it_keeps_as_the_farthest_level_does() {
    let far = *content::ladder().last().expect("a ladder");
    assert!((far.min_feature - skyline::FARTHEST_FEATURE).abs() < f64::EPSILON);
    assert!((far.segment_scale - skyline::POLICY.segment_scale).abs() < f64::EPSILON);
    assert_eq!(far.drop_interior, skyline::POLICY.drop_interior);
}
