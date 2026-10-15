//! Port Serein's blockout and its registry entry agree.
//!
//! The world shapes the ground from the registry and the content step lays
//! the city on it. If the plan reaches past the city's own ground, or leaves
//! by another road, buildings stand on the world's hillside or the road runs
//! into a wall, with nothing failing.

#[test]
fn the_blockout_stands_inside_the_shaped_ground_and_leaves_by_its_road() {
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let registries = strata_registry::load_from_dir(assets).expect("the shipped assets load");
    let def = registries
        .settlements
        .iter()
        .find(|s| s.id.as_ref() == "strata:port_serein")
        .expect("Port Serein is registered");
    let (_, layout, plan) = content::settlement(assets).expect("the plan builds");
    assert_eq!(def.layout, content::SETTLEMENT_LAYOUT);
    assert_eq!(layout.id, def.id.as_ref());
    // Fitted to the ground any world gives the city.
    let world = strata_worldgen::ProceduralWorld::new(strata_core::Seed(42), &registries);
    let site = &world.sites()[0];
    assert_eq!(layout.ground, site.fingerprint());
    // These are public arrival contracts, used by --at city and the
    // scripted/replicated walker, not a suggestion to search for a new spot.
    assert_eq!(
        layout.spawn.map(f64::to_bits),
        [-40.0_f64, 38.0, 62.0].map(f64::to_bits)
    );
    assert_eq!(
        layout.lobby.map(f64::to_bits),
        layout.spawn.map(f64::to_bits)
    );
    for [x, _, z] in [layout.spawn, layout.lobby] {
        for dx in [-1.0, 0.0, 1.0] {
            for dz in [-1.0, 0.0, 1.0] {
                assert!((site.city_ground([x + dx, z + dz]) - 38.0).abs() < 1e-9);
            }
        }
    }
    // A district's footprint is the box round all its masses, so it is held
    // to the extent's rectangle; each mass was placed on owned ground.
    let e = &def.extent;
    for building in &layout.buildings {
        let [x0, z0, x1, z1] = building.footprint;
        assert!(
            x0 >= -e.half_width_m
                && x1 <= e.half_width_m
                && z0 >= -e.uphill_m
                && z1 <= e.downhill_m,
            "{} reaches {:?}, past the city's own ground",
            building.file,
            building.footprint
        );
    }
    let [ex, ez] = def.road.exit;
    let [px, pz] = plan.road_exit;
    assert!(
        (ex - px).abs() < 0.01 && (ez - pz).abs() < 0.01,
        "the registry's road leaves at {:?}, the plan's at {:?}",
        def.road.exit,
        plan.road_exit
    );
}

/// Nothing the city builds lays an upward face on the ground itself.
///
/// The terrain draws the city's ground; a floor, a landing, a wall's top or a
/// podium laid exactly on it is two surfaces on one plane, which flicker as
/// the depth buffer picks one or the other. Every such face must stand at
/// least a centimetre proud (or sink below, hidden). Checked on every baked
/// building's finest level, placed as the layout places it.
#[test]
fn nothing_lies_flush_on_the_ground() {
    use ashlar::glam::{DQuat, DVec3};
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let registries = strata_registry::load_from_dir(assets).expect("the shipped assets load");
    let (buildings, layout, _) = content::settlement(assets).expect("the plan builds");
    let def = registries
        .settlements
        .iter()
        .find(|s| s.id.as_ref() == "strata:port_serein")
        .expect("Port Serein is registered");
    let site = strata_worldgen::Site::place(def, &|_, _| 0.0);
    let mut flush: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for ((file, building), placed) in buildings.iter().zip(&layout.buildings) {
        assert_eq!(file, &placed.file);
        let meshed = mesh_building(building, &ManifoldMesher::default()).expect("meshes");
        let turn = DQuat::from_rotation_y(placed.yaw);
        let origin = DVec3::from_array(placed.position);
        // The public walker arrives here. An added bench, kerb or shelter
        // must not enclose its standing capsule, including at the lobby.
        for collider in meshed.colliders() {
            let vertices: Vec<_> = collider
                .solid
                .vertices
                .iter()
                .map(|v| origin + turn * v)
                .collect();
            let min = vertices.iter().fold(DVec3::INFINITY, |a, b| a.min(*b));
            let max = vertices.iter().fold(DVec3::NEG_INFINITY, |a, b| a.max(*b));
            for [x, y, z] in [layout.spawn, layout.lobby] {
                let low = DVec3::new(x - 0.5, y + 0.2, z - 0.5);
                let high = DVec3::new(x + 0.5, y + 1.8, z + 0.5);
                assert!(
                    !(min.cmplt(high) & low.cmplt(max)).all(),
                    "arrival capsule inside {file}: {} / {}",
                    collider.instance,
                    collider.element
                );
            }
        }
        for piece in meshed.pieces() {
            let mesh = piece.mesh;
            for tri in mesh.indices.as_chunks::<3>().0 {
                let p = tri.map(|i| {
                    origin + turn * piece.pose.transform_point(mesh.positions[i as usize])
                });
                let normal = (p[1] - p[0]).cross(p[2] - p[0]);
                let area = normal.length() * 0.5;
                if area < 1e-4 || normal.y / (2.0 * area) < 0.99 {
                    continue;
                }
                let c = (p[0] + p[1] + p[2]) / 3.0;
                if !site.owns([c.x, c.z]) {
                    continue;
                }
                // Flush all over, not merely crossing the ground along a
                // line, as a stair's tread may where it passes a bank.
                let on = |q: DVec3| (q.y - site.city_ground([q.x, q.z])).abs() < 0.01;
                if on(c) && p.iter().all(|q| on(*q)) {
                    // Keep the instance and position so a bad fitted lot
                    // can be found without a renderer.
                    let label = piece.label();
                    let stem = file.rsplit('/').next().unwrap_or(file);
                    *flush
                        .entry(format!("{stem}: {label} near {:.1?}", c.to_array()))
                        .or_default() += area;
                }
            }
        }
    }
    let mut worst: Vec<_> = flush.into_iter().collect();
    worst.sort_by(|a, b| b.1.total_cmp(&a.1));
    let total: f64 = worst.iter().map(|w| w.1).sum();
    assert!(
        total < 1.0,
        "{total:.1} m² of faces lie flush on the ground, the largest: {:#?}",
        &worst[..worst.len().min(25)]
    );
}

/// No two surfaces of a baked building share a plane where they show.
///
/// ashlar's coplanar lint over every building's finest level: a pair of
/// faces on one plane, facing the same way and overlapping, in different
/// materials and not pressed against a face turned the other way, is a
/// visible z-fight. Same-material pairs look the same whichever wins, and
/// undersides resting on the ground are never seen; both are left alone.
#[test]
fn no_building_z_fights_itself() {
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let (buildings, _, _) = content::settlement(assets).expect("the plan builds");
    let mut fights = Vec::new();
    for (file, building) in &buildings {
        let meshed = mesh_building(building, &ManifoldMesher::default()).expect("meshes");
        let stem = file.rsplit('/').next().unwrap_or(file);
        for overlap in meshed.coplanar_overlaps() {
            // Undersides resting on the ground (a pad's, a pier's) are never
            // seen: the city is looked at from above.
            let underside = overlap.normal.y < -0.9;
            if !overlap.covered && !overlap.same_binding() && !underside {
                fights.push((
                    overlap.area,
                    format!(
                        "{stem}: {} ({}) / {} ({}) at {:.1?} facing {:.2?}",
                        overlap.first,
                        overlap.first_binding.material,
                        overlap.second,
                        overlap.second_binding.material,
                        overlap.point.to_array(),
                        overlap.normal.to_array(),
                    ),
                ));
            }
        }
    }
    fights.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = fights.iter().map(|f| f.0).sum();
    assert!(
        total < 0.5,
        "{total:.2} m^2 of faces z-fight across {} pairs, the largest: {:#?}",
        fights.len(),
        &fights[..fights.len().min(30)]
    );
}

/// Every material the settlement binds is defined, every definition names a
/// graph with the parameters it sets, and nothing is defined that no
/// building binds, which would be a bake for nothing.
#[test]
fn every_binding_has_a_definition_and_every_definition_a_graph() {
    use ashlar::Surface;
    use std::collections::BTreeSet;
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let (_, _, plan) = content::settlement(assets).expect("the plan builds");
    let ours = veyra_settlements::materials();
    let theirs = veyra_materials::buildings();
    for key in ours.materials.keys() {
        assert!(
            !theirs.materials.contains_key(key),
            "{key} is already Veyra's"
        );
    }
    // Checked against what the content step ships, the blockout's greys
    // included.
    let library = content::definitions();
    for planned in &plan.buildings {
        library
            .check_for(&planned.building)
            .unwrap_or_else(|error| panic!("{}: {}: {}", planned.name, error.path, error.reason));
    }
    let mut graphs = veyra_materials::graphs();
    graphs
        .extend(&veyra_settlements::graphs())
        .expect("no settlement graph collides with Veyra's");
    for (key, definition) in &ours.materials {
        if let Surface::Graph(bake) = &definition.surface {
            let graph = graphs
                .get(&bake.graph)
                .unwrap_or_else(|| panic!("{key} names no graph {}", bake.graph));
            for name in bake.params.keys() {
                assert!(
                    graph.params.iter().any(|param| &param.name == name),
                    "{key} sets {name}, which {} does not have",
                    bake.graph
                );
            }
        }
    }
    let bound: BTreeSet<String> = plan
        .buildings
        .iter()
        .flat_map(|planned| {
            planned
                .building
                .bindings()
                .map(|(_, _, b)| b.material.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    let unbound: Vec<&String> = ours
        .materials
        .keys()
        .filter(|key| !bound.contains(*key))
        .collect();
    assert!(unbound.is_empty(), "defined and never bound: {unbound:?}");
}

/// Density is a visual brief with a measurable floor: a future fitting or
/// clearance change must not silently turn the company town into suburbs.
#[test]
fn the_shipped_plan_keeps_dense_housing_and_lights_the_arrival() {
    use veyra_settlements::blockout::{Ground, diagram};
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let registries = strata_registry::load_from_dir(assets).expect("registries");
    let def = registries
        .settlements
        .iter()
        .find(|s| s.id.as_ref() == "strata:port_serein")
        .expect("city");
    let site = strata_worldgen::Site::place(def, &|_, _| 0.0);
    let platform = |id: &str| {
        let p = site
            .platforms
            .iter()
            .find(|p| p.id == id)
            .expect("platform");
        (p.at, p.half_size, p.height)
    };
    let ground = Ground {
        height: &|x, z| site.city_ground([x, z]),
        owns: &|x, z| site.owns([x, z]),
        starport: platform("starport"),
        exchange: platform("exchange"),
        processor: platform("processor"),
        headquarters: platform("headquarters"),
        quay: def.terraces.quay_m,
        road_exit: def.road.exit,
    };
    let plotted = diagram(&ground).expect("plan");
    let houses = plotted
        .lines()
        .filter(|line| line.starts_with("terrace-houses\t") || line.starts_with("split-houses\t"))
        .count();
    let split = plotted
        .lines()
        .filter(|line| line.starts_with("split-houses\t"))
        .count();
    assert!((650..=800).contains(&houses), "{houses} dwellings");
    assert!(split >= 20, "{split} dwellings joining levels");
    let (_, layout, plan) = content::settlement(assets).expect("layout");
    let [x, _, z] = layout.spawn;
    let arrival_lights = plan
        .lights
        .iter()
        .filter(|light| {
            let dx = light.position[0] - x;
            let dz = light.position[2] - z;
            dx * dx + dz * dz < 24.0 * 24.0 && light.lumens >= 1000.0
        })
        .count();
    assert!(
        arrival_lights >= 4,
        "only {arrival_lights} lights near the arrival"
    );
}
