//! Metropolitan corporate kit: podium, curtain bays and a separate service spine.
//! Proportions and finishes belong to this content, never to the geometry backend.
use ashlar::{
    Building, BuildingBuilder, Collision, Element, Geometry, GroundRect, GroundingPolicy,
    GroundingSpec, Instance, Part, Pose, Socket, ValidationError,
};
use glam::{DQuat, DVec3};

const STOREY: f64 = 3.8;
const PODIUM: f64 = 6.45;

/// A detail that is solid to walk into; see the study kit's `solid`.
fn solid(id: &str, size: [f64; 3], at: [f64; 3], slot: &str) -> Element {
    detail(id, size, at, slot).collision(Collision::Bounds)
}

fn detail(id: &str, size: [f64; 3], at: [f64; 3], slot: &str) -> Element {
    let bevel = size
        .into_iter()
        .fold(f64::INFINITY, f64::min)
        .mul_add(0.18, 0.0)
        .min(0.045);
    super::block(id, size, at, bevel, slot)
}

fn glazed_bay(podium: bool) -> Result<Part, ValidationError> {
    let (id, height, bottom, opening) = if podium {
        ("metro:podium", 6.0, 0.55, 4.5)
    } else {
        ("metro:curtain", STOREY, 0.4, 2.95)
    };
    let body = Geometry::chamfered_cuboid([4.0, height, 0.8], 0.045)
        .subtract(Geometry::cuboid([3.16, opening, 1.3]).placed(Pose::at([0.42, bottom, -0.2])));
    let mut part = Part::builder(id)
        .element(
            Element::new("frame", body, "stone")
                .cut_material("reveal")
                .collision(Collision::Bounds),
        )
        .element(solid(
            "glass",
            [3.12, opening - 0.04, 0.065],
            [0.44, bottom + 0.02, 0.64],
            "glass",
        ))
        .element(detail(
            "mullion",
            [0.085, opening, 0.42],
            [1.9575, bottom, 0.25],
            "metal",
        ))
        .element(detail(
            "sill",
            [3.3, 0.12, 0.65],
            [0.35, bottom - 0.08, -0.1],
            "metal",
        ));
    for (name, x) in [("left-fin", 0.1), ("right-fin", 3.72)] {
        part = part.element(detail(name, [0.18, height, 1.25], [x, 0.0, -0.3], "spine"));
    }
    if !podium {
        part = part.element(detail(
            "occupancy-strip",
            [2.75, 0.065, 0.08],
            [0.62, 0.27, -0.08],
            "light",
        ));
    }
    part.build()
}

fn lobby() -> Result<Part, ValidationError> {
    let body = Geometry::chamfered_cuboid([8.0, 6.0, 1.6], 0.065)
        .subtract(Geometry::cuboid([5.6, 4.8, 2.1]).placed(Pose::at([1.2, -0.02, -0.2])));
    Part::builder("metro:lobby")
        .element(
            Element::new("portal", body, "stone")
                .cut_material("reveal")
                .collision(Collision::Bounds),
        )
        .element(solid(
            "glass-wall",
            [5.6, 4.5, 0.12],
            [1.2, 0.13, 1.25],
            "glass",
        ))
        .element(detail(
            "left-door-frame",
            [0.1, 4.5, 0.25],
            [2.38, 0.13, 1.05],
            "metal",
        ))
        .element(detail(
            "right-door-frame",
            [0.1, 4.5, 0.25],
            [5.52, 0.13, 1.05],
            "metal",
        ))
        .element(detail(
            "door-meeting",
            [0.08, 4.5, 0.25],
            [3.96, 0.13, 1.05],
            "metal",
        ))
        .element(solid(
            "threshold",
            [5.8, 0.15, 1.95],
            [1.1, -0.02, -0.6],
            "reveal",
        ))
        .element(detail(
            "canopy",
            [8.6, 0.3, 3.2],
            [-0.3, 4.92, -2.15],
            "metal",
        ))
        .element(detail(
            "canopy-light",
            [5.7, 0.055, 0.16],
            [1.15, 4.865, -1.85],
            "light",
        ))
        .element(detail(
            "access-terminal",
            [0.28, 0.9, 0.18],
            [1.21, 1.0, 0.3],
            "metal",
        ))
        .element(detail(
            "terminal-screen",
            [0.19, 0.34, 0.03],
            [1.255, 1.47, 0.27],
            "light",
        ))
        .socket(Socket::new(
            "entrance",
            Pose::at([4.0, 0.13, -0.6]).rotated(DQuat::from_rotation_y(std::f64::consts::PI)),
        ))
        .build()
}

fn service_riser() -> Result<Part, ValidationError> {
    let mut part = Part::builder("metro:service-riser")
        .element(solid("body", [2.6, STOREY, 6.7], [0.0, 0.0, 0.0], "dark"))
        .element(detail(
            "spine",
            [0.3, STOREY, 7.1],
            [2.3, 0.0, -0.2],
            "spine",
        ));
    for row in 0..5 {
        part = part.element(detail(
            &format!("louvre-{row}"),
            [1.65, 0.13, 0.22],
            [0.3, 0.65 + f64::from(row) * 0.37, -0.14],
            "metal",
        ));
    }
    part.element(detail(
        "service-light",
        [0.09, 2.8, 0.06],
        [2.1, 0.5, -0.055],
        "signal",
    ))
    .build()
}

fn identity() -> Result<Part, ValidationError> {
    let mut part = Part::builder("metro:identity").element(detail(
        "backplate",
        [7.0, 1.25, 0.28],
        [0.0, 0.0, 0.0],
        "dark",
    ));
    for (i, x) in [0.4, 0.72, 1.04].into_iter().enumerate() {
        part = part.element(detail(
            &format!("mark-{i}"),
            [0.18, 0.77, 0.05],
            [x, 0.24, -0.05],
            "ink",
        ));
    }
    for (i, (x, width)) in [(1.6, 2.15), (3.95, 1.0), (5.15, 1.4)]
        .into_iter()
        .enumerate()
    {
        part = part.element(detail(
            &format!("designation-{i}"),
            [width, 0.12, 0.05],
            [x, 0.84, -0.05],
            "ink",
        ));
    }
    part.element(detail(
        "status-bar",
        [4.95, 0.045, 0.04],
        [1.6, 0.36, -0.04],
        "signal",
    ))
    .build()
}

fn kit(id: &str) -> Result<BuildingBuilder, ValidationError> {
    let mut builder = Building::builder(id);
    for slot in [
        "stone", "spine", "reveal", "metal", "dark", "glass", "light", "signal", "ink", "paving",
    ] {
        builder = builder.material(slot, format!("metro:{slot}"));
    }
    builder = builder
        .part(glazed_bay(false)?)
        .part(glazed_bay(true)?)
        .part(lobby()?)
        .part(service_riser()?)
        .part(identity()?);
    // A parapet rail and a paving slab are walked past and over, not into.
    for (id, size, at, slot, collision) in [
        (
            "metro:base",
            [16.65, 0.45, 12.65],
            [-0.325, -0.47, -0.325],
            "stone",
            Collision::Bounds,
        ),
        (
            "metro:podium-cap",
            [17.2, 0.45, 13.2],
            [-0.6, 0.0, -0.6],
            "spine",
            Collision::Bounds,
        ),
        (
            "metro:tower-cap",
            [12.9, 0.5, 8.9],
            [-0.45, 0.0, -0.45],
            "spine",
            Collision::Bounds,
        ),
        (
            "metro:core-base",
            [2.6, PODIUM, 6.7],
            [0.0, 0.0, 0.0],
            "dark",
            Collision::Bounds,
        ),
        (
            "metro:core-crown",
            [2.95, 2.6, 7.1],
            [-0.175, 0.0, -0.2],
            "dark",
            Collision::Bounds,
        ),
        (
            "metro:rail",
            [4.0, 0.8, 0.22],
            [0.0, 0.0, -0.05],
            "metal",
            Collision::None,
        ),
        (
            "metro:paving",
            [8.0, 0.18, 8.0],
            [0.0, -0.2, 0.0],
            "paving",
            Collision::None,
        ),
    ] {
        builder = builder.part(
            Part::builder(id)
                .element(detail("solid", size, at, slot).collision(collision))
                .build()?,
        );
    }
    let mut plant = Part::builder("metro:plant").element(solid(
        "body",
        [4.0, 2.4, 3.0],
        [0.0, 0.0, 0.0],
        "metal",
    ));
    for row in 0..6 {
        plant = plant.element(detail(
            &format!("cooling-{row}"),
            [3.5, 0.12, 0.2],
            [0.25, 0.4 + f64::from(row) * 0.28, -0.12],
            "reveal",
        ));
    }
    builder = builder.part(plant.build()?);
    Ok(builder)
}

fn ring(
    mut builder: BuildingBuilder,
    level: u32,
    origin: [f64; 3],
    bays: [u32; 2],
    podium: bool,
) -> BuildingBuilder {
    let width = f64::from(bays[0]) * 4.0;
    let depth = f64::from(bays[1]) * 4.0;
    for (side, count, corner, angle) in [
        ("front", bays[0], [0.0, 0.0, 0.0], 0.0),
        ("back", bays[0], [width, 0.0, depth], std::f64::consts::PI),
        (
            "left",
            bays[1],
            [0.0, 0.0, depth],
            std::f64::consts::FRAC_PI_2,
        ),
        (
            "right",
            bays[1],
            [width, 0.0, 0.0],
            -std::f64::consts::FRAC_PI_2,
        ),
    ] {
        let rotation = DQuat::from_rotation_y(angle);
        for bay in 0..count {
            if podium && side == "front" && (bay == 1 || bay == 2) {
                continue;
            }
            let at = DVec3::from_array(origin)
                + DVec3::from_array(corner)
                + rotation * DVec3::new(f64::from(bay) * 4.0, 0.0, 0.0);
            let mut instance = Instance::new(
                format!("{level}-{side}-{bay}"),
                if podium {
                    "metro:podium"
                } else {
                    "metro:curtain"
                },
            )
            .placed(Pose::at(at.to_array()).rotated(rotation));
            if !podium && (bay + level).is_multiple_of(3) {
                instance = instance.material("glass", "metro:occupied");
            }
            builder = builder.instance(instance);
        }
    }
    builder
}

/// Assemble one to eight upper storeys over a double-height public podium.
pub fn building(storeys: u32) -> Result<Building, ValidationError> {
    if !(1..=8).contains(&storeys) {
        return Err(ValidationError {
            path: "corporate.storeys".into(),
            reason: "expected 1..=8 upper storeys".into(),
        });
    }
    let mut builder = ring(
        kit(&format!("metro:tower-{storeys}"))?,
        0,
        [0.0, 0.0, 0.0],
        [4, 3],
        true,
    )
    .instance(Instance::new("base", "metro:base"))
    .instance(Instance::new("lobby", "metro:lobby").placed(Pose::at([4.0, 0.0, 0.0])))
    .instance(Instance::new("podium-cap", "metro:podium-cap").placed(Pose::at([0.0, 6.0, 0.0])))
    .instance(Instance::new("core-base", "metro:core-base").placed(Pose::at([13.35, 0.0, 5.0])))
    .instance(Instance::new("identity", "metro:identity").placed(Pose::at([4.5, 6.48, -0.52])));
    for floor in 0..storeys {
        let y = PODIUM + f64::from(floor) * STOREY;
        builder = ring(builder, floor + 1, [2.0, y, 2.0], [3, 2], false).instance(
            Instance::new(format!("core-{floor}"), "metro:service-riser")
                .placed(Pose::at([13.35, y, 5.0])),
        );
    }
    let roof = PODIUM + f64::from(storeys) * STOREY;
    builder = builder
        .instance(Instance::new("tower-cap", "metro:tower-cap").placed(Pose::at([2.0, roof, 2.0])))
        .instance(
            Instance::new("core-crown", "metro:core-crown").placed(Pose::at([13.35, roof, 5.0])),
        )
        .instance(Instance::new("roof-plant", "metro:plant").placed(Pose::at([
            5.0,
            roof + 0.5,
            4.0,
        ])));
    for (i, (at, yaw)) in [
        ([2.0, roof + 0.5, 2.0], 0.0),
        ([6.0, roof + 0.5, 2.0], 0.0),
        ([10.0, roof + 0.5, 2.0], 0.0),
        ([14.0, roof + 0.5, 10.0], std::f64::consts::PI),
        ([10.0, roof + 0.5, 10.0], std::f64::consts::PI),
        ([6.0, roof + 0.5, 10.0], std::f64::consts::PI),
    ]
    .into_iter()
    .enumerate()
    {
        builder = builder.instance(
            Instance::new(format!("rail-{i}"), "metro:rail")
                .placed(Pose::at(at).rotated(DQuat::from_rotation_y(yaw))),
        );
    }
    builder.build()
}

/// A tower and two lower wings around a paved forecourt, reusing one part library.
pub fn block() -> Result<Building, ValidationError> {
    let mut builder = kit("metro:city-block")?;
    for (name, floors, at, yaw) in [
        ("tower", 4, [0.0, 0.0, 16.0], 0.0),
        ("annex", 1, [-4.0, 0.0, -4.0], -std::f64::consts::FRAC_PI_2),
        ("wing", 2, [32.0, 0.0, 12.0], std::f64::consts::FRAC_PI_2),
    ] {
        let pose = Pose::at(at).rotated(DQuat::from_rotation_y(yaw));
        for mut instance in building(floors)?.into_recipe().instances {
            instance.id = format!("{name}/{}", instance.id);
            instance.pose = Pose {
                translation: pose.transform_point(instance.pose.translation),
                rotation: pose.rotation * instance.pose.rotation,
            };
            builder = builder.instance(instance);
        }
    }
    for x in -1..4 {
        for z in -1..2 {
            builder = builder.instance(
                Instance::new(format!("paving-{x}-{z}"), "metro:paving").placed(Pose::at([
                    f64::from(x) * 8.0,
                    0.0,
                    f64::from(z) * 8.0,
                ])),
            );
        }
    }
    builder.build()
}

/// The podium's actual slab and front entrance, independent of upper storey count.
pub fn grounding_spec() -> GroundingSpec {
    GroundingSpec {
        footprint: GroundRect::new([-0.325, -0.325], [16.325, 12.325]),
        floor: -0.02,
        slab_bottom: -0.47,
        entrance: Socket::new(
            "entrance",
            Pose::at([8.0, 0.13, -0.6]).rotated(DQuat::from_rotation_y(std::f64::consts::PI)),
        ),
    }
}
/// Corporate podium limits; still the same pure fitting algorithm.
pub fn grounding_policy() -> GroundingPolicy {
    GroundingPolicy {
        max_depth: 5.0,
        max_ramp_length: 24.0,
        landing_length: 2.0,
        ..super::grounding_policy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tower_annex_and_block_share_the_same_parts() {
        let tower = building(4).expect("tower");
        let annex = building(1).expect("annex");
        let district = block().expect("block");
        assert_eq!(tower.recipe().parts, annex.recipe().parts);
        assert_eq!(tower.recipe().parts, district.recipe().parts);
        assert!(tower.recipe().instances.len() > annex.recipe().instances.len());
        assert!(district.recipe().instances.len() > tower.recipe().instances.len() * 2);
        assert!(building(0).is_err() && building(9).is_err());
    }
}
