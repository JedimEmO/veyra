//! A whole settlement as one building, for the distance at which a city is a
//! silhouette (ticket 0002).
//!
//! Past the distance its buildings stream to, a city is a few hundred
//! pixels wide, and every building drawn as its own pieces would cost
//! entities for detail nobody can see. The skyline is every planned building
//! simplified hard and placed in the settlement's frame in one recipe, baked
//! with a single concatenated level ([`LodCell::Whole`]), so it draws as one
//! batch per material binding and side: a few tens of entities for a city.
//!
//! The recipe carries no collision proxy, no portal, no room and no merge
//! group. The world's colliders come from the placed buildings, so a skyline
//! that carried any would spawn a second, invisible city of them.
use std::collections::{BTreeMap, BTreeSet};

use ashlar::glam::{DQuat, DVec3};
use ashlar::{
    Binding, Building, BuildingRecipe, Collision, Element, Geometry, Instance, LodCell, LodPolicy,
    MaterialLibrary, Pose, SCHEMA_VERSION, Shape, ValidationError,
};

use crate::SettlementPlan;

/// How hard the skyline simplifies each building before it is joined.
///
/// Nothing under 4 m: at the game's default 45-degree field of view a
/// 1440-line screen spans about a metre a pixel at 1.7 km, where the game
/// hands over to the buildings, so 4 m is about four pixels there and under
/// three at 2.5 km. Walls, slabs, roofs and towers stay, and a storey's
/// openings go to the facade texture. A quarter of the segments round, and
/// no interior. Elements marked `far` survive by ashlar's own rule, and
/// [`prepared`] keeps two more kinds whole at the buildings' farthest level:
/// what glows, so the city is as lit either side of the switch, and the
/// long ones [`LINE_LENGTH`] keeps. A starting point, to be tuned by eye
/// from the captures.
pub const POLICY: LodPolicy = LodPolicy {
    until: None,
    min_feature: 4.0,
    segment_scale: 0.25,
    drop_interior: true,
    cell: None,
};

/// The ladder the skyline is baked with: one level, as the recipe already is
/// (it was simplified by [`POLICY`] when it was built, so nothing is meshed
/// at full detail twice), joined into one cell.
pub const LADDER: [LodPolicy; 1] = [LodPolicy {
    until: None,
    min_feature: 0.0,
    segment_scale: 1.0,
    drop_interior: false,
    cell: Some(LodCell::Whole),
}];

/// An element this long, metres, is kept by the skyline however thin
/// [`POLICY`] finds it, as long as it is at least [`LINE_WIDTH`] across.
///
/// Ashlar judges an element by the middle of its three extents, so a pipe
/// 2.4 m round and 70 m long is 2.4 m of anything and goes with the sills.
/// Seen from the switch it is a line two or three pixels wide running down
/// the hill, and when the city hands over to its buildings the line appears
/// all at once (the processor's pipelines did, in the first captures). Four
/// times the feature size: shorter than that it is a dot of a few pixels,
/// which the switch may drop.
pub const LINE_LENGTH: f64 = 16.0;

/// The feature size of the buildings' own farthest level, metres: the last
/// policy of the ladder the content step bakes them with (ashlar's default,
/// joined into cells), which a content test holds this to.
///
/// What the skyline keeps of a lit or a long element it keeps as that level
/// does, so what the buildings draw of it when the city hands over is what
/// the skyline drew.
pub const FARTHEST_FEATURE: f64 = 1.5;

/// How wide a long element must be for the skyline to keep it, metres: the
/// feature size of the buildings' own farthest level, so what the skyline
/// keeps as a line is what the buildings draw when the city hands over, and
/// a rail five centimetres round stays out however long it runs.
pub const LINE_WIDTH: f64 = FARTHEST_FEATURE;

/// What separates a source building's name from its own ids in the
/// skyline's part and instance ids.
pub const SEPARATOR: char = '/';

/// The planned building an id of the skyline came from: its name, the part
/// of the id before [`SEPARATOR`].
#[must_use]
pub fn source_of(id: &str) -> Option<&str> {
    id.split_once(SEPARATOR).map(|(name, _)| name)
}

/// The skyline of `plan`: every building simplified by [`POLICY`] as
/// [`prepared`] says, and placed where the plan stands it, in the
/// settlement's frame.
///
/// A building stands in the frame as the game places it: at its `position`,
/// turned by `yaw` about `+y` as `from_rotation_y` turns, so the skyline
/// placed at the frame's origin and turned by the frame's own yaw draws each
/// building where its own file draws. Every part and instance id is the
/// building's name, [`SEPARATOR`], and its own id, so the kits two buildings
/// share do not collide and a face's operand names the building it came
/// from. Each instance carries the binding it resolved to in its own
/// building for every slot its part uses, because every building has its
/// own palette. A building nothing of which survives the policy is left out.
/// `library` holds the definitions the bindings name, which say what glows
/// (see [`prepared`]).
///
/// # Errors
///
/// A name holding [`SEPARATOR`], a simplification that does not validate, or
/// a joined recipe that does not validate (two plan names equal).
pub fn skyline(
    plan: &SettlementPlan,
    library: &MaterialLibrary,
) -> Result<Building, ValidationError> {
    skyline_with(plan, &POLICY, library)
}

/// [`skyline`] under another policy than [`POLICY`], for tuning it: what a
/// coarser or finer skyline would cost is a bake of this, not an edit.
///
/// # Errors
///
/// As [`skyline`].
pub fn skyline_with(
    plan: &SettlementPlan,
    policy: &LodPolicy,
    library: &MaterialLibrary,
) -> Result<Building, ValidationError> {
    let mut parts = Vec::new();
    let mut instances = Vec::new();
    for planned in &plan.buildings {
        if planned.name.contains(SEPARATOR) {
            return Err(ValidationError {
                path: format!("buildings[{}]", planned.name),
                reason: format!("a skyline source's name may not hold {SEPARATOR:?}"),
            });
        }
        let Some(simple) = prepared(&planned.building, policy, library)? else {
            continue;
        };
        let id = |own: &str| format!("{}{SEPARATOR}{own}", planned.name);
        let placement = Pose {
            translation: DVec3::from_array(planned.position),
            rotation: DQuat::from_rotation_y(planned.yaw),
        };
        let mut bindings: BTreeMap<&str, BTreeMap<String, Binding>> = BTreeMap::new();
        for (instance, slot, binding) in simple.bindings() {
            bindings
                .entry(instance)
                .or_default()
                .insert(slot.to_owned(), binding.clone());
        }
        let recipe = simple.recipe();
        for part in &recipe.parts {
            let mut part = part.clone();
            part.id = id(&part.id);
            for element in &mut part.elements {
                element.collision = Collision::None;
                unportal(&mut element.geometry);
            }
            parts.push(part);
        }
        for instance in &recipe.instances {
            let mut placed = Instance::new(id(&instance.id), id(&instance.part));
            placed.pose = placement.compose(instance.pose);
            placed.materials = bindings.remove(instance.id.as_str()).unwrap_or_default();
            instances.push(placed);
        }
    }
    BuildingRecipe {
        version: SCHEMA_VERSION,
        id: format!("{}:skyline", plan.id),
        grid: None,
        parts,
        instances,
        merged: false,
        groups: Vec::new(),
        rooms: Vec::new(),
        materials: BTreeMap::new(),
    }
    .build()
}

/// One building as the skyline draws it, in its own frame: simplified by
/// `policy`, with three changes first. Every chamfered box is a plain one,
/// since a bevel of a few centimetres is nothing at the distance the skyline
/// draws and a chamfered box is 44 triangles where a box is 12 (the
/// Terraces' houses were two thirds of the first skyline's triangles, and a
/// third of theirs were bevels). Every element that glows ([`glows`], for
/// any instance placing it) is kept as the buildings' farthest level keeps
/// it ([`FARTHEST_FEATURE`]), since the city's lights are panes, slits and
/// strips far under the policy's feature size and would otherwise come on
/// all at once when the city hands over (the housing slabs' lit slits and
/// the market's shop fronts did, in the captures of 2026-09-29). And every
/// element that reads as a line ([`LINE_LENGTH`], [`LINE_WIDTH`]) is kept
/// the same way, so the switch does not draw it in either.
///
/// Deciding by the material rather than by the `far` mark: `far` keeps an
/// element whole at every level, down to its hairline members, which is
/// right for a lamp and wrong for a slab whose lit slits are cut into its
/// structure. A lit element the farthest level drops (a house's small
/// panes) the skyline drops too.
///
/// # Errors
///
/// A building that no longer validates, which would be a bug here.
pub fn prepared(
    building: &Building,
    policy: &LodPolicy,
    library: &MaterialLibrary,
) -> Result<Option<Building>, ValidationError> {
    let lit = lit_elements(building, library);
    let farthest = LodPolicy {
        min_feature: FARTHEST_FEATURE,
        // The skyline's own policy coarsens the segments afterwards, far
        // or not; scaling them here too would scale them twice.
        segment_scale: 1.0,
        ..*policy
    };
    let mut recipe = building.recipe().clone();
    for part in &mut recipe.parts {
        for element in &mut part.elements {
            plain(&mut element.geometry);
            if element.far {
                continue;
            }
            let glowing = lit.contains(&(part.id.clone(), element.id.clone()))
                && !thin(&element.geometry, FARTHEST_FEATURE);
            if (glowing || reads_as_line(&element.geometry, policy))
                && let Some(geometry) = element.geometry.simplified(&farthest)
            {
                // Far, so the policy keeps it and only coarsens its segments.
                element.geometry = geometry;
                element.far = true;
            }
        }
    }
    recipe.build()?.simplified(policy)
}

/// Whether a binding's definition glows: a positive emissive colour, which
/// every emissive map multiplies (a definition without one draws no light
/// whatever its maps say). A binding naming a key `library` does not hold
/// does not glow.
#[must_use]
pub fn glows(library: &MaterialLibrary, binding: &Binding) -> bool {
    library
        .materials
        .get(&binding.material)
        .is_some_and(|definition| definition.emissive.iter().any(|&v| v > 0.0))
}

/// Every element, `(part, element)`, one of whose slots is bound to a
/// glowing definition by some instance placing its part.
fn lit_elements(building: &Building, library: &MaterialLibrary) -> BTreeSet<(String, String)> {
    let mut lit = BTreeSet::new();
    let recipe = building.recipe();
    for instance in &recipe.instances {
        let Some(part) = recipe.parts.iter().find(|part| part.id == instance.part) else {
            continue;
        };
        for element in &part.elements {
            let glowing = slots(element).any(|slot| {
                building
                    .binding(&instance.id, slot)
                    .is_some_and(|binding| glows(library, binding))
            });
            if glowing {
                lit.insert((part.id.clone(), element.id.clone()));
            }
        }
    }
    lit
}

/// Every slot an element's faces may wear: its own, its cut faces', and
/// each cutter's.
fn slots(element: &Element) -> impl Iterator<Item = &str> {
    std::iter::once(element.material_slot.as_str())
        .chain(element.cut_material_slot.as_deref())
        .chain(
            element
                .geometry
                .cutters()
                .into_iter()
                .filter_map(|cutter| cutter.cut_slot.as_deref()),
        )
}

/// Whether a solid is thinner than `feature` as ashlar judges it: the middle
/// of its three extents.
fn thin(geometry: &Geometry, feature: f64) -> bool {
    geometry.bounds().is_none_or(|[low, high]| {
        let mut extents = (high - low).to_array();
        extents.sort_by(f64::total_cmp);
        extents[1] < feature
    })
}

/// Whether an element the policy would drop as thin is long and wide enough
/// to read as a line from where the skyline hands over.
fn reads_as_line(geometry: &Geometry, policy: &LodPolicy) -> bool {
    geometry.bounds().is_some_and(|[low, high]| {
        let mut extents = (high - low).to_array();
        extents.sort_by(f64::total_cmp);
        extents[1] < policy.min_feature && extents[1] >= LINE_WIDTH && extents[2] >= LINE_LENGTH
    })
}

/// Make every chamfered box in an expression a plain box of the same size;
/// both stand from the origin to their size, so the bounds do not move.
fn plain(geometry: &mut Geometry) {
    match &mut geometry.shape {
        Shape::ChamferedCuboid { size, .. } => geometry.shape = Shape::Cuboid { size: *size },
        Shape::Difference { solid, cutters } => {
            plain(solid);
            cutters.iter_mut().for_each(plain);
        }
        Shape::Union { solids } => solids.iter_mut().for_each(plain),
        Shape::Array { solid, .. } | Shape::Mirror { solid, .. } => plain(solid),
        _ => {}
    }
}

/// Clear every portal mark in an expression. Only a cutter root may carry
/// one, but clearing every node is as cheap and cannot miss one.
fn unportal(geometry: &mut Geometry) {
    geometry.portal = None;
    match &mut geometry.shape {
        Shape::Difference { solid, cutters } => {
            unportal(solid);
            cutters.iter_mut().for_each(unportal);
        }
        Shape::Union { solids } => solids.iter_mut().for_each(unportal),
        Shape::Array { solid, .. } | Shape::Mirror { solid, .. } => unportal(solid),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use ashlar::{Element, Part, Pose};

    use super::*;
    use crate::PlannedBuilding;

    /// A library in which nothing glows.
    fn unlit() -> MaterialLibrary {
        MaterialLibrary::default()
    }

    /// A library in which `kit:lamp` glows and `kit:dark` does not.
    fn lamps() -> MaterialLibrary {
        let mut library = MaterialLibrary::default();
        library.materials.insert(
            "kit:lamp".to_owned(),
            ashlar::MaterialDefinition {
                emissive: [2.0, 1.5, 0.5],
                ..ashlar::MaterialDefinition::default()
            },
        );
        library
            .materials
            .insert("kit:dark".to_owned(), ashlar::MaterialDefinition::default());
        library
    }

    /// A building of one part placed twice, one instance overriding its
    /// paint, with a collider and a portal wide enough to survive
    /// [`POLICY`].
    fn house(paint: &str) -> Building {
        let mut door = Geometry::cuboid([4.5, 5.0, 8.0]).placed(Pose::at([0.0, 2.5, 0.0]));
        door.portal = Some("door".to_owned());
        let body = Element::new(
            "body",
            Geometry::cuboid([6.0, 6.0, 6.0])
                .placed(Pose::at([0.0, 3.0, 0.0]))
                .subtract(door),
            "paint",
        )
        .collision(Collision::Bounds);
        let part = Part::builder("kit:house")
            .element(body)
            .element(Element::new(
                "sill",
                Geometry::cuboid([1.0, 0.1, 0.2]),
                "paint",
            ))
            .build()
            .expect("the part validates");
        let mut second = Instance::new("b", "kit:house");
        second.pose = Pose::at([10.0, 0.0, 0.0]);
        second
            .materials
            .insert("paint".to_owned(), Binding::new("kit:red"));
        Building::builder("kit:houses")
            .part(part)
            .instance(Instance::new("a", "kit:house"))
            .instance(second)
            .material("paint", paint)
            .build()
            .expect("the building validates")
    }

    fn plan() -> SettlementPlan {
        let planned = |name: &str, paint: &str, position: [f64; 3], yaw: f64| PlannedBuilding {
            name: name.to_owned(),
            building: house(paint),
            position,
            yaw,
            footprint: [0.0; 4],
        };
        SettlementPlan {
            id: "test:town",
            buildings: vec![
                planned("west", "kit:cream", [-50.0, 12.0, 0.0], 0.0),
                planned("east", "kit:blue", [50.0, 20.0, 5.0], 0.7),
            ],
            spawn: [0.0; 3],
            lobby: [0.0; 3],
            road_exit: [0.0; 2],
            lights: Vec::new(),
            plants: Vec::new(),
        }
    }

    #[test]
    fn every_building_is_prefixed_placed_and_keeps_its_own_palette() {
        let skyline = skyline(&plan(), &unlit()).expect("the skyline validates");
        let recipe = skyline.recipe();
        let ids: Vec<&str> = recipe.instances.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["west/a", "west/b", "east/a", "east/b"]);
        assert_eq!(recipe.parts.len(), 2, "one part per source building");
        assert!(recipe.materials.is_empty(), "no shared palette");
        let paint = |id: &str| {
            skyline
                .binding(id, "paint")
                .expect("bound")
                .material
                .clone()
        };
        assert_eq!(paint("west/a"), "kit:cream");
        assert_eq!(paint("west/b"), "kit:red");
        assert_eq!(paint("east/a"), "kit:blue");
        assert_eq!(paint("east/b"), "kit:red");
        // East's second house: ten metres along its own x, turned by 0.7
        // about +y, from east's origin.
        let turned = DQuat::from_rotation_y(0.7) * DVec3::new(10.0, 0.0, 0.0);
        let pose = skyline.instance("east/b").expect("placed").pose;
        assert!(
            (pose.translation - (DVec3::new(50.0, 20.0, 5.0) + turned)).length() < 1e-9,
            "{:?}",
            pose.translation
        );
        assert!(pose.rotation.angle_between(DQuat::from_rotation_y(0.7)) < 1e-9);
        assert_eq!(source_of("east/b"), Some("east"));
    }

    #[test]
    fn the_skyline_carries_no_collider_no_portal_and_nothing_small() {
        // The portal and the collider survive the policy on their own, so
        // it is the skyline that takes them off.
        let simple = house("kit:cream")
            .simplified(&POLICY)
            .expect("validates")
            .expect("the body survives");
        let body = &simple.recipe().parts[0].elements[0];
        assert_eq!(body.collision, Collision::Bounds);
        assert!(body.geometry.cutters().iter().any(|c| c.portal.is_some()));
        let skyline = skyline(&plan(), &unlit()).expect("the skyline validates");
        for part in &skyline.recipe().parts {
            let ids: Vec<&str> = part.elements.iter().map(|e| e.id.as_str()).collect();
            assert_eq!(ids, ["body"], "the sill is under the feature size");
            for element in &part.elements {
                assert_eq!(element.collision, Collision::None);
                assert!(
                    element
                        .geometry
                        .cutters()
                        .iter()
                        .all(|c| c.portal.is_none()),
                    "{}: a portal survived",
                    part.id
                );
            }
        }
        assert!(skyline.rooms().is_empty());
        assert!(skyline.merge_groups().is_empty());
    }

    /// A building of one part holding `elements`, placed once.
    fn kit(elements: Vec<Element>) -> Building {
        let mut part = Part::builder("kit:bits");
        for element in elements {
            part = part.element(element);
        }
        Building::builder("kit:bits")
            .part(part.build().expect("the part validates"))
            .instance(Instance::new("a", "kit:bits"))
            .material("paint", "kit:cream")
            .build()
            .expect("the building validates")
    }

    fn element_ids(building: &Building) -> Vec<&str> {
        building.recipe().parts[0]
            .elements
            .iter()
            .map(|e| e.id.as_str())
            .collect()
    }

    #[test]
    fn a_chamfered_box_is_drawn_as_a_plain_one_of_the_same_size() {
        let building = kit(vec![Element::new(
            "block",
            Geometry::chamfered_cuboid([8.0, 6.0, 5.0], 0.05).placed(Pose::at([1.0, 0.0, 2.0])),
            "paint",
        )]);
        let prepared = prepared(&building, &POLICY, &unlit())
            .expect("validates")
            .expect("the block survives");
        let geometry = &prepared.recipe().parts[0].elements[0].geometry;
        assert_eq!(
            geometry.shape,
            Shape::Cuboid {
                size: [8.0, 6.0, 5.0]
            }
        );
        assert_eq!(geometry.pose, Pose::at([1.0, 0.0, 2.0]));
    }

    #[test]
    fn a_long_element_is_kept_however_thin_but_a_short_or_hairline_one_is_not() {
        let bar = |id: &str, size: [f64; 3]| Element::new(id, Geometry::cuboid(size), "paint");
        let building = kit(vec![
            bar("pipe", [40.0, 2.4, 2.4]),
            bar("stub", [8.0, 2.4, 2.4]),
            bar("rail", [40.0, 0.1, 0.1]),
            bar("wall", [20.0, 6.0, 0.5]),
        ]);
        // As ashlar alone would have it: the pipe goes with the rest.
        let plain = building
            .simplified(&POLICY)
            .expect("validates")
            .expect("the wall survives");
        assert_eq!(element_ids(&plain), ["wall"]);
        let prepared = prepared(&building, &POLICY, &unlit())
            .expect("validates")
            .expect("something survives");
        assert_eq!(element_ids(&prepared), ["pipe", "wall"]);
    }

    #[test]
    fn a_building_nothing_of_which_survives_is_left_out() {
        let mut plan = plan();
        plan.buildings.push(PlannedBuilding {
            name: "kiosk".to_owned(),
            building: kit(vec![Element::new(
                "sign",
                Geometry::cuboid([1.0, 2.0, 0.1]),
                "paint",
            )]),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            footprint: [0.0; 4],
        });
        let skyline = skyline(&plan, &unlit()).expect("the skyline validates");
        let recipe = skyline.recipe();
        assert!(
            recipe
                .instances
                .iter()
                .all(|i| source_of(&i.id) != Some("kiosk")),
            "the kiosk has nothing over the feature size"
        );
        assert!(
            recipe
                .parts
                .iter()
                .all(|p| source_of(&p.id) != Some("kiosk"))
        );
        assert_eq!(recipe.instances.len(), 4);
    }

    #[test]
    fn a_name_holding_the_separator_is_refused() {
        let mut plan = plan();
        plan.buildings[0].name = "west/side".to_owned();
        assert!(skyline(&plan, &unlit()).is_err());
    }

    #[test]
    fn glowing_is_the_definitions_emissive() {
        let library = lamps();
        assert!(glows(&library, &Binding::new("kit:lamp")));
        assert!(!glows(&library, &Binding::new("kit:dark")));
        assert!(!glows(&library, &Binding::new("kit:missing")));
    }

    /// A storey of lit slits 1.8 m tall, each a member of one frame
    /// element, and 50 cm lamps: the policy would drop the slits and keep
    /// the frame, and drop the lamps whole.
    fn slab(slits: &str, lamp: &str) -> Building {
        let slit = |x: f64| Geometry::cuboid([3.0, 1.8, 0.3]).placed(Pose::at([x, 0.0, 0.0]));
        let frame = Geometry::union_all([
            Geometry::cuboid([20.0, 6.0, 0.5]).placed(Pose::at([0.0, 2.0, 0.0])),
            slit(0.0),
            slit(5.0),
            slit(10.0),
        ]);
        let part = Part::builder("kit:slab")
            .element(Element::new("frame", frame, "slits"))
            .element(Element::new(
                "lamp",
                Geometry::cuboid([0.5, 0.5, 0.5]),
                "lamp",
            ))
            .build()
            .expect("the part validates");
        Building::builder("kit:slab")
            .part(part)
            .instance(Instance::new("a", "kit:slab"))
            .material("slits", slits)
            .material("lamp", lamp)
            .build()
            .expect("the building validates")
    }

    /// The members of the one element named `id`, or one for a solid that
    /// is not a union.
    fn members(building: &Building, id: &str) -> usize {
        let element = building.recipe().parts[0]
            .elements
            .iter()
            .find(|e| e.id == id)
            .expect("the element survives");
        match &element.geometry.shape {
            Shape::Union { solids } => solids.len(),
            _ => 1,
        }
    }

    #[test]
    fn a_lit_element_is_kept_as_the_farthest_level_keeps_it() {
        let farthest = LodPolicy {
            min_feature: FARTHEST_FEATURE,
            ..POLICY
        };
        let lit = slab("kit:lamp", "kit:lamp");
        let own = lit
            .simplified(&farthest)
            .expect("validates")
            .expect("the frame survives");
        // The farthest level keeps the slits and drops the half-metre lamp.
        assert_eq!(element_ids(&own), ["frame"]);
        assert_eq!(members(&own, "frame"), 4);
        let prepared = prepared(&lit, &POLICY, &lamps())
            .expect("validates")
            .expect("the frame survives");
        assert_eq!(element_ids(&prepared), ["frame"], "the lamp is under 1.5 m");
        assert_eq!(members(&prepared, "frame"), 4, "every slit is kept");
    }

    #[test]
    fn an_unlit_element_is_simplified_by_the_policy_alone() {
        for (slits, library) in [("kit:dark", lamps()), ("kit:lamp", unlit())] {
            let prepared = prepared(&slab(slits, "kit:dark"), &POLICY, &library)
                .expect("validates")
                .expect("the frame survives");
            assert_eq!(members(&prepared, "frame"), 1, "{slits}: the slits went");
        }
    }

    #[test]
    fn an_element_lit_by_one_instance_is_kept_for_all() {
        let mut recipe = slab("kit:dark", "kit:dark").recipe().clone();
        let mut second = Instance::new("b", "kit:slab");
        second.pose = Pose::at([0.0, 10.0, 0.0]);
        second
            .materials
            .insert("slits".to_owned(), Binding::new("kit:lamp"));
        recipe.instances.push(second);
        let building = recipe.build().expect("validates");
        let prepared = prepared(&building, &POLICY, &lamps())
            .expect("validates")
            .expect("the frame survives");
        assert_eq!(members(&prepared, "frame"), 4);
        assert_eq!(
            prepared.binding("a", "slits").map(|b| b.material.as_str()),
            Some("kit:dark")
        );
    }
}
