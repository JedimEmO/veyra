//! Architectural content for the material study; all style and dimensions live here.
use glam::DQuat;
pub mod bridge;
pub mod corporate;
mod example;
mod grounding;
pub mod port;
pub mod roadside;
pub mod vehicle;
use ashlar::{
    Building, BuildingBuilder, Collision, Element, Geometry, Instance, Part, PartBuilder, Pose,
    Socket, ValidationError,
};
pub use grounding::{grounding_policy, grounding_spec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Available authored study layouts.
pub enum Scene {
    /// Original primitive fixture.
    Basic,
    /// Single facade module.
    Facade,
    /// Single entrance module.
    Entrance,
    /// One-storey outpost.
    Outpost,
    /// Three-storey office.
    Office,
    /// Metropolitan corporate tower.
    Corporate,
    /// Lower corporate annex sharing the tower kit.
    CorporateAnnex,
    /// Three corporate structures around a paved forecourt.
    CorporateBlock,
}

impl Scene {
    /// Whether this scene binds Veyra's materials (`veyra-materials`); the
    /// primitive fixture wears the preview's diagnostic colours instead.
    #[must_use]
    pub fn dressed(self) -> bool {
        !matches!(self, Self::Basic)
    }
}

/// A block that is solid to walk into. Structure, glazing and door leaves are;
/// trim, slats, lights and signage are not, so they carry no proxy.
fn solid(id: &str, size: [f64; 3], at: [f64; 3], bevel: f64, slot: &str) -> Element {
    block(id, size, at, bevel, slot).collision(Collision::Bounds)
}

fn block(id: &str, size: [f64; 3], at: [f64; 3], bevel: f64, slot: &str) -> Element {
    let geometry = if bevel > 0.0 {
        Geometry::chamfered_cuboid(size, bevel)
    } else {
        Geometry::cuboid(size)
    };
    Element::new(id, geometry.placed(Pose::at(at)), slot)
}

fn panel(entrance: bool) -> PartBuilder {
    let (x, y, width, height) = if entrance {
        (0.65, -0.1, 2.7, 3.45)
    } else {
        (0.6, 1.8, 2.8, 1.65)
    };
    let mut body = Geometry::chamfered_cuboid([3.94, 4.74, 0.85], 0.035)
        .placed(Pose::at([0.03, 0.03, 0.0]))
        .subtract(Geometry::cuboid([width, height, 1.3]).placed(Pose::at([x, y, -0.2])));
    for x in [0.38, 3.62] {
        for y in [0.85, 3.85] {
            body = body.subtract(
                Geometry::cylinder(0.045, 0.11, 12).placed(
                    Pose::at([x, y, -0.03])
                        .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
                ),
            );
        }
    }
    let mut part = Part::builder(if entrance {
        "study:entrance"
    } else {
        "study:facade"
    })
    // Bay edges, facing outward along the wall, so the next bay along attaches
    // to this one rather than being placed by multiplying its index.
    .socket(Socket::new(
        "left",
        Pose::default().rotated(DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2)),
    ))
    .socket(Socket::new(
        "right",
        Pose::at([4.0, 0.0, 0.0]).rotated(DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2)),
    ))
    .element(
        Element::new("panel", body, "panel")
            .cut_material("reveal")
            .collision(Collision::Bounds),
    )
    .element(solid(
        "top-course",
        [3.98, 0.25, 1.25],
        [0.01, 4.53, -0.25],
        0.025,
        "concrete",
    ));
    for (id, x) in [("left", 0.08), ("right", 3.69)] {
        part = part.element(solid(
            id,
            [0.23, 4.4, 1.0],
            [x, 0.1, -0.25],
            0.025,
            "concrete",
        ));
    }
    part
}

fn facade() -> Result<Part, ValidationError> {
    let frame = Geometry::chamfered_cuboid([3.04, 1.91, 0.16], 0.025)
        .placed(Pose::at([0.48, 1.67, -0.055]))
        .subtract(Geometry::cuboid([2.75, 1.61, 0.4]).placed(Pose::at([0.625, 1.82, -0.15])));
    let mut part = panel(false)
        .element(Element::new("window-frame", frame, "metal"))
        .element(solid(
            "recessed-glass",
            [2.78, 1.64, 0.045],
            [0.61, 1.805, 0.69],
            0.008,
            "glass",
        ))
        .element(block(
            "mullion",
            [0.065, 1.65, 0.67],
            [1.97, 1.8, 0.04],
            0.01,
            "metal",
        ))
        .element(block(
            "sill",
            [3.06, 0.13, 0.46],
            [0.47, 1.62, -0.28],
            0.025,
            "metal",
        ))
        .element(solid(
            "plinth",
            [3.95, 0.44, 0.99],
            [0.025, 0.025, -0.14],
            0.035,
            "concrete",
        ))
        .element(block(
            "vent-recess",
            [1.68, 0.55, 0.035],
            [1.16, 0.76, -0.045],
            0.01,
            "dark",
        ));
    for index in 0..6 {
        part = part.element(block(
            &format!("vent-fin-{index}"),
            [1.72, 0.045, 0.11],
            [1.14, 0.77 + f64::from(index) * 0.1, -0.11],
            0.009,
            "metal",
        ));
    }
    part.build()
}

fn entrance() -> Result<Part, ValidationError> {
    let mut part = panel(true)
        .element(block(
            "lintel",
            [3.1, 0.35, 1.7],
            [0.45, 3.45, -0.9],
            0.055,
            "concrete",
        ))
        .element(block(
            "light-housing",
            [2.5, 0.14, 0.24],
            [0.75, 3.25, -0.35],
            0.025,
            "metal",
        ))
        .element(block(
            "light",
            [2.25, 0.045, 0.04],
            [0.88, 3.29, -0.375],
            0.008,
            "light",
        ))
        .element(solid(
            "threshold",
            [2.85, 0.12, 1.15],
            [0.575, -0.02, -0.48],
            0.025,
            "metal",
        ))
        .element(block(
            "header-sign",
            [1.05, 0.45, 0.06],
            [1.475, 3.9, -0.07],
            0.012,
            "metal",
        ))
        .element(block(
            "terminal",
            [0.23, 0.52, 0.16],
            [3.39, 1.3, -0.19],
            0.025,
            "metal",
        ))
        .element(block(
            "terminal-screen",
            [0.15, 0.15, 0.025],
            [3.43, 1.59, -0.21],
            0.005,
            "light",
        ));
    for (id, x) in [("left", 0.63), ("right", 3.16)] {
        part = part.element(solid(
            &format!("jamb-{id}"),
            [0.21, 3.35, 0.97],
            [x, 0.0, -0.08],
            0.025,
            "metal",
        ));
    }
    for (id, x) in [("left", 0.88), ("right", 2.035)] {
        part = part
            .element(solid(
                &format!("door-{id}"),
                [1.085, 3.15, 0.14],
                [x, 0.12, 0.69],
                0.025,
                "metal",
            ))
            .element(block(
                &format!("door-inset-{id}"),
                [0.77, 1.6, 0.025],
                [x + 0.15, 0.64, 0.66],
                0.007,
                "dark",
            ))
            .element(block(
                &format!("door-stripe-{id}"),
                [0.065, 2.7, 0.03],
                [x + 0.12, 0.3, 0.65],
                0.006,
                "paint",
            ));
    }
    sign(part).build()
}

fn sign(mut part: PartBuilder) -> PartBuilder {
    // Small raised industrial "07" identifier, built from reusable strip geometry.
    for (index, (x, y, w, h)) in [
        (1.64, 3.99, 0.05, 0.25),
        (1.84, 3.99, 0.05, 0.25),
        (1.64, 3.99, 0.25, 0.045),
        (1.64, 4.20, 0.25, 0.045),
        (2.08, 4.20, 0.25, 0.045),
        (2.28, 3.99, 0.05, 0.25),
    ]
    .into_iter()
    .enumerate()
    {
        part = part.element(block(
            &format!("sign-strip-{index}"),
            [w, h, 0.02],
            [4.0 - x - w, y, -0.085],
            0.004,
            "paint",
        ));
    }
    part
}

fn kit(id: &str) -> Result<BuildingBuilder, ValidationError> {
    let mut builder = Building::builder(id).part(facade()?).part(entrance()?);
    for slot in [
        "concrete", "metal", "glass", "dark", "light", "paint", "rust",
    ] {
        builder = builder.material(slot, format!("study:{slot}"));
    }
    builder = builder
        .material("panel", "study:concrete")
        .material("reveal", "study:cut");
    let rail = Part::builder("study:parapet")
        .element(solid(
            "wall",
            [3.97, 0.85, 0.28],
            [0.015, 0.0, 0.0],
            0.025,
            "concrete",
        ))
        .element(solid(
            "cap",
            [3.98, 0.09, 0.42],
            [0.01, 0.85, -0.075],
            0.015,
            "metal",
        ))
        .build()?;
    let mut plant = Part::builder("study:plant")
        .element(solid(
            "housing",
            [3.6, 1.9, 2.8],
            [0.2, 0.0, 0.1],
            0.1,
            "metal",
        ))
        .element(solid(
            "cap",
            [3.95, 0.17, 3.0],
            [0.025, 1.9, 0.0],
            0.035,
            "metal",
        ))
        .element(block(
            "vent-dark",
            [3.1, 1.05, 0.035],
            [0.45, 0.42, 0.065],
            0.01,
            "dark",
        ));
    for i in 0..10 {
        plant = plant.element(block(
            &format!("louvre-{i}"),
            [3.16, 0.055, 0.16],
            [0.42, 0.45 + f64::from(i) * 0.105, 0.0],
            0.012,
            "rust",
        ));
    }
    Ok(builder.part(rail).part(plant.build()?))
}

/// Build the selected content recipe without renderer or filesystem dependencies.
pub fn building(scene: Scene) -> Result<Building, ValidationError> {
    match scene {
        Scene::Corporate => return corporate::building(4),
        Scene::CorporateAnnex => return corporate::building(1),
        Scene::CorporateBlock => return corporate::block(),
        _ => {}
    }
    if scene == Scene::Basic {
        return example::building();
    }
    let mut builder = kit(match scene {
        Scene::Facade => "study:facade-specimen",
        Scene::Entrance => "study:entrance-specimen",
        Scene::Outpost => "study:outpost",
        Scene::Office => "study:office",
        Scene::Basic | Scene::Corporate | Scene::CorporateAnnex | Scene::CorporateBlock => {
            unreachable!()
        }
    })?;
    if matches!(scene, Scene::Facade | Scene::Entrance) {
        return builder
            .instance(Instance::new(
                "specimen",
                if scene == Scene::Facade {
                    "study:facade"
                } else {
                    "study:entrance"
                },
            ))
            .build();
    }
    let (bays, depth_bays, floors) = if scene == Scene::Office {
        (4, 3, 3)
    } else {
        (3, 2, 1)
    };
    let width = f64::from(bays) * 4.0;
    let depth = f64::from(depth_bays) * 4.0;
    builder = walls(builder, bays, depth_bays, floors);
    let height = f64::from(floors) * 4.8;
    for (id, y) in [("foundation", -0.32), ("roof", height)] {
        let part = Part::builder(id)
            .element(solid(
                "slab",
                [width + 0.65, 0.3, depth + 0.65],
                [-0.325, 0.0, -0.325],
                0.055,
                "concrete",
            ))
            .build()?;
        builder = builder
            .part(part)
            .instance(Instance::new(id, id).placed(Pose::at([0.0, y, 0.0])));
    }
    let canopy = Part::builder("canopy")
        .element(solid(
            "slab",
            [4.6, 0.24, 2.3],
            [0.0, 0.0, 0.0],
            0.055,
            "concrete",
        ))
        .element(block(
            "edge",
            [4.62, 0.095, 0.16],
            [-0.01, 0.16, -0.035],
            0.02,
            "metal",
        ))
        .build()?;
    builder = builder
        .part(canopy)
        .instance(Instance::new("canopy", "canopy").placed(Pose::at([3.7, 3.8, -2.2])));
    for index in 0..if scene == Scene::Office { 2 } else { 1 } {
        builder = builder.instance(
            Instance::new(format!("plant-{index}"), "study:plant").placed(Pose::at([
                1.0 + f64::from(index) * 6.0,
                height + 0.3,
                depth - 4.0,
            ])),
        );
    }
    builder.build()
}

fn walls(mut builder: BuildingBuilder, bays: i32, depth_bays: i32, floors: i32) -> BuildingBuilder {
    let width = f64::from(bays) * 4.0;
    let depth = f64::from(depth_bays) * 4.0;
    for floor in 0..floors {
        for (side, count, origin, angle) in [
            ("front", bays, [0.0, 0.0, 0.0], 0.0),
            ("back", bays, [width, 0.0, depth], std::f64::consts::PI),
            (
                "left",
                depth_bays,
                [0.0, 0.0, depth],
                std::f64::consts::FRAC_PI_2,
            ),
            (
                "right",
                depth_bays,
                [width, 0.0, 0.0],
                -std::f64::consts::FRAC_PI_2,
            ),
        ] {
            let rotation = DQuat::from_rotation_y(angle);
            for bay in 0..count {
                let position = glam::DVec3::from_array(origin)
                    + rotation
                        * glam::DVec3::new(f64::from(bay) * 4.0, f64::from(floor) * 4.8, 0.0);
                let entrance = side == "front" && floor == 0 && bay == 1;
                let mut instance = Instance::new(
                    format!("{side}-{floor}-{bay}"),
                    if entrance {
                        "study:entrance"
                    } else {
                        "study:facade"
                    },
                );
                instance = if bay == 0 {
                    instance.placed(Pose::at(position.to_array()).rotated(rotation))
                } else {
                    // Each bay meets the previous one edge to edge; only the
                    // corner bay of a side is placed by hand.
                    instance.attach("left", format!("{side}-{floor}-{}", bay - 1), "right")
                };
                if side == "front" && floor == 0 && bay == 0 {
                    instance = instance.material("panel", "study:repair");
                }
                builder = builder.instance(instance);
                if floor == floors - 1 {
                    builder = builder.instance(
                        Instance::new(format!("parapet-{side}-{bay}"), "study:parapet").placed(
                            Pose::at((position + glam::DVec3::Y * 4.96).to_array())
                                .rotated(rotation),
                        ),
                    );
                }
            }
        }
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_layouts_reuse_identical_facade_and_entrance_definitions() {
        let outpost = building(Scene::Outpost).expect("outpost");
        let office = building(Scene::Office).expect("office");
        for id in ["study:facade", "study:entrance"] {
            let a = outpost
                .recipe()
                .parts
                .iter()
                .find(|p| p.id == id)
                .expect("part");
            let b = office
                .recipe()
                .parts
                .iter()
                .find(|p| p.id == id)
                .expect("part");
            assert_eq!(a, b);
        }
        assert!(office.recipe().instances.len() > outpost.recipe().instances.len());
        assert_eq!(outpost.material("front-0-0", "panel"), Some("study:repair"));
        assert_eq!(
            outpost.material("front-0-2", "panel"),
            Some("study:concrete")
        );
    }

    #[test]
    fn attached_bays_land_exactly_where_the_index_arithmetic_put_them() {
        let office = building(Scene::Office).expect("office");
        for (side, count, origin, angle) in [
            ("front", 4, [0.0, 0.0, 0.0], 0.0),
            ("back", 4, [16.0, 0.0, 12.0], std::f64::consts::PI),
            ("left", 3, [0.0, 0.0, 12.0], std::f64::consts::FRAC_PI_2),
            ("right", 3, [16.0, 0.0, 0.0], -std::f64::consts::FRAC_PI_2),
        ] {
            let rotation = DQuat::from_rotation_y(angle);
            for floor in 0..3 {
                for bay in 0..count {
                    let expected = glam::DVec3::from_array(origin)
                        + rotation
                            * glam::DVec3::new(f64::from(bay) * 4.0, f64::from(floor) * 4.8, 0.0);
                    let pose = office
                        .instance(&format!("{side}-{floor}-{bay}"))
                        .expect("bay")
                        .pose;
                    assert!(
                        pose.translation.distance(expected) < 1e-9,
                        "{side}-{floor}-{bay} at {} not {expected}",
                        pose.translation
                    );
                    assert!(pose.rotation.angle_between(rotation) < 1e-9);
                }
            }
        }
    }
}
