//! Small, mechanically legible weapon recipes. Every replaceable assembly keeps
//! a named instance so the workbench can separate it without rebuilding meshes.
use std::collections::BTreeMap;

use ashlar::glam::DQuat;
use ashlar::{Building, Element, Geometry, Instance, Part, Pose, Socket, ValidationError};

use crate::{Assembly, Slot, Weapon};

const MATERIALS: [&str; 7] = [
    "shell", "metal", "grip", "accent", "ceramic", "lens", "light",
];

#[derive(Default)]
struct Solids(BTreeMap<&'static str, Vec<Geometry>>);

impl Solids {
    fn add(&mut self, material: &'static str, geometry: Geometry) {
        self.0.entry(material).or_default().push(geometry);
    }

    /// Every solid turned about the frame's origin, over the pose it has.
    fn turned(mut self, turn: Pose) -> Self {
        for solid in self.0.values_mut().flatten() {
            solid.pose = turn.compose(solid.pose);
        }
        self
    }

    fn part(self, id: &str) -> Result<Part, ValidationError> {
        let mut part = Part::builder(id).socket(Socket::new("mate", Pose::default()));
        for (material, mut solids) in self.0 {
            let geometry = if solids.len() == 1 {
                solids.remove(0)
            } else {
                Geometry::union_all(solids)
            };
            part = part.element(Element::new(material, geometry, material));
        }
        part.build()
    }
}

fn block(size: [f64; 3], at: [f64; 3], bevel: f64) -> Geometry {
    let bevel = bevel.min(size.into_iter().fold(f64::INFINITY, f64::min) * 0.45);
    Geometry::chamfered_cuboid(size, bevel).placed(Pose::at(at))
}

fn tube(radius: f64, bore: f64, length: f64, at: [f64; 3]) -> Geometry {
    Geometry::cylinder(radius, length, 32)
        .subtract(Geometry::cylinder(bore, length + 0.004, 32).placed(Pose::at([0.0, -0.002, 0.0])))
        .placed(Pose::at(at).rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)))
}

fn barrel(radius: f64, length: f64, at: [f64; 3]) -> Geometry {
    tube(radius, radius * 0.52, length, at)
}

fn grip(s: &mut Solids, width: f64, height: f64, z: f64) {
    let mut body = block(
        [width, height, 0.045],
        [-width / 2.0, -height / 2.0, z],
        0.003,
    );
    // Shallow moulded grooves preserve a continuous palm surface.
    for i in 0..7 {
        body = body.subtract(block(
            [width + 0.006, 0.0018, 0.035],
            [
                -width / 2.0 - 0.003,
                -height / 2.0 + 0.018 + f64::from(i) * 0.011,
                z + 0.012,
            ],
            0.0006,
        ));
    }
    s.add("grip", body);
    s.add(
        "metal",
        block(
            [width * 0.76, 0.017, 0.043],
            [-width * 0.38, height / 2.0 - 0.008, z],
            0.002,
        ),
    );
}

fn guard(s: &mut Solids, width: f64, z: f64) {
    let outside = block([width, 0.064, 0.065], [-width / 2.0, -0.028, z], 0.004);
    let opening = block(
        [width + 0.02, 0.048, 0.052],
        [-width / 2.0 - 0.01, -0.022, z + 0.006],
        0.003,
    );
    s.add("metal", outside.subtract(opening));
    s.add(
        "metal",
        Geometry::hull([
            [-0.003, 0.030, z + 0.021],
            [0.003, 0.030, z + 0.021],
            [-0.003, 0.030, z + 0.028],
            [0.003, 0.030, z + 0.028],
            [-0.003, -0.009, z + 0.031],
            [0.003, -0.009, z + 0.031],
            [-0.003, -0.009, z + 0.035],
            [0.003, -0.009, z + 0.035],
        ]),
    );
}

fn panel(s: &mut Solids, width: f64, y: f64, z: f64, length: f64) {
    for side in [-1.0, 1.0] {
        let x = if side > 0.0 {
            width / 2.0 - 0.001
        } else {
            -width / 2.0 - 0.002
        };
        let mut plate = block([0.003, 0.030, length], [x, y, z], 0.0008);
        for p in [z + 0.009, z + length - 0.013] {
            let screw = Geometry::cylinder(0.003, 0.004, 16)
                .subtract(block(
                    [0.001, 0.006, 0.006],
                    [-0.0005, -0.001, -0.003],
                    0.0002,
                ))
                .placed(
                    Pose::at([x - 0.0005, y + 0.014, p])
                        .rotated(DQuat::from_rotation_z(-std::f64::consts::FRAC_PI_2)),
                );
            plate = plate.subtract(
                Geometry::cylinder(0.0037, 0.007, 16).placed(
                    Pose::at([x - 0.002, y + 0.014, p])
                        .rotated(DQuat::from_rotation_z(-std::f64::consts::FRAC_PI_2)),
                ),
            );
            s.add("metal", screw);
        }
        s.add("shell", plate);
        s.add(
            "accent",
            block(
                [0.0035, 0.003, 0.020],
                [x - 0.00025, y + 0.023, z + length * 0.45],
                0.0007,
            ),
        );
    }
}

fn shoulder(s: &mut Solids, width: f64, rear: f64) {
    let length = -rear - 0.055;
    s.add(
        "metal",
        block(
            [width * 0.48, 0.025, length],
            [-width * 0.24, 0.048, rear + 0.012],
            0.002,
        ),
    );
    let shell = block(
        [width, 0.052, length * 0.67],
        [-width / 2.0, 0.021, rear + 0.009],
        0.003,
    )
    .subtract(block(
        [width + 0.01, 0.019, length * 0.34],
        [-width / 2.0 - 0.005, 0.027, rear + 0.04],
        0.003,
    ));
    s.add("shell", shell);
    s.add(
        "grip",
        block(
            [width + 0.004, 0.105, 0.016],
            [-width / 2.0 - 0.002, -0.031, rear],
            0.003,
        ),
    );
    s.add(
        "grip",
        block(
            [width * 0.87, 0.009, length * 0.48],
            [-width * 0.435, 0.071, rear + 0.018],
            0.002,
        ),
    );
    // The short adjustment latch is seated beneath the stock body.
    s.add(
        "metal",
        block([0.018, 0.007, 0.043], [-0.009, 0.015, rear + 0.082], 0.0015),
    );
}

#[allow(clippy::too_many_lines)] // Keep each chassis silhouette beside the related family dimensions.
fn chassis(weapon: Weapon) -> Solids {
    let mut s = Solids::default();
    match weapon {
        Weapon::Pistol => {
            s.add(
                "shell",
                block([0.044, 0.043, 0.215], [-0.022, 0.045, -0.043], 0.0025),
            );
            s.add(
                "metal",
                block([0.036, 0.012, 0.178], [-0.018, 0.034, -0.026], 0.004),
            );
            s.add("metal", barrel(0.012, 0.037, [0.0, 0.065, 0.145]));
            guard(&mut s, 0.016, 0.028);
            panel(&mut s, 0.044, 0.048, -0.022, 0.09);
            for i in 0..5 {
                s.add(
                    "metal",
                    block(
                        [0.046, 0.017, 0.003],
                        [-0.023, 0.062, -0.026 + f64::from(i) * 0.008],
                        0.001,
                    ),
                );
            }
        }
        Weapon::BurstRifle
        | Weapon::PrecisionRifle
        | Weapon::Scattergun
        | Weapon::SupportProjector => {
            let (w, length) = match weapon {
                Weapon::PrecisionRifle => (0.054, 0.32),
                Weapon::Scattergun => (0.074, 0.29),
                Weapon::SupportProjector => (0.095, 0.37),
                _ => (0.062, 0.29),
            };
            s.add(
                "shell",
                block([w, 0.074, length], [-w / 2.0, 0.028, -0.074], 0.003),
            );
            s.add(
                "metal",
                block(
                    [w * 0.58, 0.012, length + 0.016],
                    [-w * 0.325, 0.099, -0.08],
                    0.004,
                ),
            );
            grip(&mut s, 0.034, 0.115, -0.024);
            guard(&mut s, 0.02, 0.025);
            panel(&mut s, w, 0.046, -0.042, length * 0.57);
            s.add(
                "metal",
                block(
                    [0.004, 0.013, 0.061],
                    [-w / 2.0 - 0.002, 0.077, 0.019],
                    0.001,
                ),
            );
            s.add(
                "grip",
                block(
                    [0.012, 0.009, 0.017],
                    [-w / 2.0 - 0.011, 0.079, 0.023],
                    0.0015,
                ),
            );
            // A separate lower-receiver lip carries the controls into the grip datum.
            s.add(
                "metal",
                block([w * 0.72, 0.009, 0.09], [-w * 0.36, 0.026, -0.045], 0.002),
            );
            for i in 0..7 {
                s.add(
                    "metal",
                    block(
                        [w * 0.66, 0.006, 0.008],
                        [-w * 0.33, 0.110, -0.055 + f64::from(i) * 0.023],
                        0.002,
                    ),
                );
            }
            if weapon != Weapon::PrecisionRifle {
                shoulder(
                    &mut s,
                    w * 0.83,
                    if weapon == Weapon::SupportProjector {
                        -0.31
                    } else {
                        -0.29
                    },
                );
            }
            match weapon {
                Weapon::PrecisionRifle => {
                    s.add("metal", barrel(0.015, 0.46, [0.0, 0.074, 0.245]));
                    s.add("ceramic", tube(0.028, 0.019, 0.115, [0.0, 0.074, 0.225]));
                    s.add("metal", tube(0.032, 0.019, 0.023, [0.0, 0.074, 0.332]));
                    s.add(
                        "grip",
                        block([0.042, 0.031, 0.16], [-0.021, 0.012, 0.15], 0.008),
                    );
                }
                Weapon::Scattergun => {
                    for x in [-0.022, 0.022] {
                        s.add("metal", barrel(0.023, 0.20, [x, 0.073, 0.193]));
                    }
                    s.add(
                        "metal",
                        block([0.094, 0.009, 0.086], [-0.047, 0.096, 0.03], 0.003),
                    );
                }
                Weapon::SupportProjector => {
                    s.add("metal", barrel(0.042, 0.39, [0.0, 0.075, 0.277]));
                    s.add("metal", tube(0.055, 0.036, 0.034, [0.0, 0.075, 0.637]));
                    s.add("light", tube(0.037, 0.029, 0.006, [0.0, 0.075, 0.669]));
                }
                _ => {}
            }
        }
        Weapon::FieldBlade | Weapon::HeavyCutter | Weapon::ImpactBaton => {
            let heavy = weapon == Weapon::HeavyCutter;
            let rear = if heavy { -0.19 } else { -0.09 };
            let front = if heavy { 0.17 } else { 0.09 };
            s.add("metal", barrel(0.017, front - rear, [0.0, 0.0, rear]));
            s.add("metal", tube(0.027, 0.011, 0.023, [0.0, 0.0, rear]));
            s.add(
                "shell",
                block(
                    [if heavy { 0.14 } else { 0.075 }, 0.036, 0.026],
                    [if heavy { -0.07 } else { -0.0375 }, -0.018, front],
                    0.006,
                ),
            );
            s.add(
                "accent",
                tube(0.024, 0.015, 0.009, [0.0, 0.0, front + 0.025]),
            );
            if weapon == Weapon::ImpactBaton {
                s.add("metal", barrel(0.018, 0.23, [0.0, 0.0, 0.113]));
                s.add("shell", tube(0.026, 0.015, 0.023, [0.0, 0.0, 0.245]));
            }
            if heavy {
                s.add(
                    "metal",
                    block([0.031, 0.021, 0.39], [-0.0155, -0.0105, 0.18], 0.002),
                );
            }
        }
        Weapon::ReachGlaive => {
            // The shaft runs from behind the leading hand to the collar, so
            // the head and the counterweight stand off the same datum.
            s.add("metal", barrel(0.017, 1.6, [0.0, 0.0, -0.55]));
            s.add("metal", tube(0.027, 0.011, 0.04, [0.0, 0.0, 1.01]));
            s.add("accent", tube(0.024, 0.015, 0.009, [0.0, 0.0, 1.05]));
            s.add("shell", tube(0.024, 0.011, 0.05, [0.0, 0.0, -0.58]));
        }
        Weapon::CombatPistol | Weapon::PatrolCarbine | Weapon::MarksmanRifle | Weapon::Breacher => {
            unreachable!("military assemblies use their dedicated recipe")
        }
    }
    s
}

fn sight(s: &mut Solids, weapon: Weapon, id: &str) {
    let y = if weapon == Weapon::Pistol {
        0.089
    } else {
        0.117
    };
    if id.is_empty() {
        for z in [
            -0.02,
            if weapon == Weapon::Pistol {
                0.145
            } else {
                0.20
            },
        ] {
            s.add(
                "metal",
                block([0.026, 0.017, 0.016], [-0.013, y, z], 0.003).subtract(block(
                    [0.008, 0.014, 0.022],
                    [-0.004, y + 0.009, z - 0.003],
                    0.002,
                )),
            );
        }
    } else if id == "pistol_low_optic" {
        s.add(
            "metal",
            block([0.028, 0.006, 0.034], [-0.014, y, -0.016], 0.001),
        );
        s.add(
            "shell",
            block([0.029, 0.022, 0.015], [-0.0145, y + 0.005, -0.005], 0.002).subtract(block(
                [0.023, 0.017, 0.021],
                [-0.0115, y + 0.009, -0.008],
                0.001,
            )),
        );
        s.add(
            "lens",
            block([0.022, 0.016, 0.0015], [-0.011, y + 0.009, 0.002], 0.0003),
        );
    } else if id == "compact_optic" {
        s.add(
            "metal",
            block([0.037, 0.012, 0.052], [-0.0185, y, -0.023], 0.003),
        );
        s.add(
            "shell",
            block([0.038, 0.042, 0.025], [-0.019, y + 0.009, -0.012], 0.006).subtract(block(
                [0.027, 0.028, 0.033],
                [-0.0135, y + 0.019, -0.016],
                0.004,
            )),
        );
        s.add(
            "lens",
            block([0.026, 0.026, 0.002], [-0.013, y + 0.019, -0.001], 0.0007),
        );
        s.add(
            "accent",
            block([0.005, 0.011, 0.023], [0.02, y + 0.014, -0.01], 0.002),
        );
    } else {
        let long = id == "precision_long_optic";
        let length = if long { 0.24 } else { 0.17 };
        let z = -0.045;
        s.add(
            "metal",
            block([0.031, 0.008, 0.102], [-0.0155, y, -0.021], 0.0015),
        );
        s.add("shell", tube(0.022, 0.015, length, [0.0, y + 0.039, z]));
        for mount in [0.002, 0.070] {
            s.add(
                "metal",
                block([0.018, 0.017, 0.011], [-0.009, y + 0.006, mount], 0.001),
            );
            s.add("metal", tube(0.024, 0.021, 0.011, [0.0, y + 0.039, mount]));
        }
        for dz in [0.0, length - 0.025] {
            s.add("metal", tube(0.025, 0.018, 0.027, [0.0, y + 0.039, z + dz]));
        }
        s.add(
            "lens",
            Geometry::cylinder(0.0175, 0.002, 32).placed(
                Pose::at([0.0, y + 0.039, z + length - 0.003])
                    .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
            ),
        );
        s.add(
            "accent",
            block([0.012, 0.01, 0.033], [0.021, y + 0.04, 0.024], 0.002),
        );
        if !long {
            s.add(
                "ceramic",
                block([0.032, 0.003, 0.085], [-0.016, y + 0.008, -0.011], 0.0008),
            );
        }
    }
}

#[allow(clippy::too_many_lines)] // Feed families share a datum but retain distinct insertion silhouettes.
fn feed(s: &mut Solids, weapon: Weapon, id: &str) {
    let extended = id.contains("extended");
    match weapon {
        Weapon::Pistol => {
            let height = if extended { 0.035 } else { 0.014 };
            s.add(
                "shell",
                block(
                    [0.043, height, 0.052],
                    [-0.0215, -0.062 - height, -0.035],
                    0.005,
                ),
            );
            s.add(
                "accent",
                block(
                    [0.045, 0.005, 0.032],
                    [-0.0225, -0.059 - height, -0.025],
                    0.001,
                ),
            );
        }
        Weapon::SupportProjector => {
            let reserve = id == "support_reserve_cell";
            if reserve {
                s.add(
                    "shell",
                    block([0.070, 0.045, 0.135], [0.042, -0.053, 0.053], 0.003),
                );
                s.add(
                    "grip",
                    block([0.073, 0.009, 0.14], [0.0405, -0.056, 0.050], 0.002),
                );
                for z in [0.075, 0.12, 0.16] {
                    s.add(
                        "metal",
                        block([0.003, 0.031, 0.005], [0.111, -0.043, z], 0.001),
                    );
                }
            }
            s.add(
                "shell",
                block([0.074, 0.10, 0.14], [0.040, -0.015, 0.05], 0.003),
            );
            s.add(
                "metal",
                block([0.006, 0.103, 0.145], [0.109, -0.017, 0.048], 0.002),
            );
            s.add(
                "accent",
                block([0.008, 0.013, 0.072], [0.112, 0.041, 0.091], 0.003),
            );
        }
        Weapon::PrecisionRifle => {
            let width = if id.is_empty() { 0.075 } else { 0.046 };
            s.add(
                "metal",
                block([width, 0.047, 0.071], [0.018, 0.037, 0.045], 0.006),
            );
            s.add(
                "grip",
                block([0.009, 0.051, 0.076], [0.015 + width, 0.035, 0.042], 0.003),
            );
        }
        _ => {
            let h = if extended { 0.154 } else { 0.104 };
            let w = if weapon == Weapon::Scattergun {
                0.049
            } else {
                0.043
            };
            s.add(
                "metal",
                block(
                    [w + 0.006, 0.012, 0.084],
                    [-w / 2.0 - 0.003, 0.022, 0.091],
                    0.002,
                ),
            );
            s.add(
                "shell",
                block([w, h, 0.079], [-w / 2.0, 0.03 - h, 0.094], 0.008),
            );
            s.add(
                "grip",
                block(
                    [w + 0.008, 0.018, 0.088],
                    [-w / 2.0 - 0.004, 0.018 - h, 0.09],
                    0.004,
                ),
            );
            for i in 0..3 {
                s.add(
                    "metal",
                    block(
                        [w + 0.002, h * 0.63, 0.003],
                        [-w / 2.0 - 0.001, 0.039 - h, 0.107 + f64::from(i) * 0.019],
                        0.001,
                    ),
                );
            }
            s.add(
                "accent",
                block(
                    [w + 0.004, 0.009, 0.024],
                    [-w / 2.0 - 0.002, 0.022 - h * 0.45, 0.124],
                    0.002,
                ),
            );
        }
    }
}

fn fore_end(s: &mut Solids, id: &str) {
    let cooled = id == "rifle_cooled_fore_end";
    if id == "rifle_open_fore_end" {
        s.add("metal", barrel(0.018, 0.247, [0.0, 0.069, 0.201]));
        for z in [0.212, 0.326] {
            s.add("shell", tube(0.030, 0.023, 0.016, [0.0, 0.069, z]));
        }
        for x in [-0.026, 0.019] {
            s.add(
                "shell",
                block([0.007, 0.012, 0.13], [x, 0.062, 0.213], 0.0015),
            );
        }
        s.add(
            "grip",
            block([0.036, 0.016, 0.11], [-0.018, 0.027, 0.221], 0.002),
        );
        s.add("metal", tube(0.023, 0.013, 0.019, [0.0, 0.069, 0.430]));
        s.add("accent", tube(0.0305, 0.027, 0.004, [0.0, 0.069, 0.334]));
        return;
    }
    let shell = block([0.059, 0.068, 0.225], [-0.0295, 0.031, 0.206], 0.0025);
    let hollow = block([0.045, 0.047, 0.24], [-0.0225, 0.039, 0.20], 0.007);
    let mut shell = shell.subtract(hollow);
    for i in 0..5 {
        shell = shell.subtract(block(
            [0.085, 0.023, 0.011],
            [-0.0425, 0.058, 0.23 + f64::from(i) * 0.032],
            0.004,
        ));
    }
    s.add("shell", shell);
    s.add("metal", barrel(0.019, 0.247, [0.0, 0.069, 0.201]));
    s.add(
        "grip",
        block([0.049, 0.014, 0.135], [-0.0245, 0.020, 0.226], 0.002),
    );
    s.add("metal", tube(0.03, 0.019, 0.031, [0.0, 0.069, 0.418]));
    s.add("light", tube(0.017, 0.011, 0.005, [0.0, 0.069, 0.447]));
    if cooled {
        for i in 0..7 {
            s.add(
                "ceramic",
                tube(
                    0.036,
                    0.026,
                    0.008,
                    [0.0, 0.069, 0.229 + f64::from(i) * 0.024],
                ),
            );
        }
        s.add(
            "accent",
            block([0.076, 0.01, 0.032], [-0.038, 0.021, 0.381], 0.003),
        );
    }
}

fn melee_grip(s: &mut Solids, weapon: Weapon, id: &str) {
    let wrapped = id.contains("wrapped");
    let replacement = !id.is_empty();
    let (start, length) = if weapon == Weapon::HeavyCutter {
        (-0.165, 0.32)
    } else if weapon == Weapon::ReachGlaive {
        (-0.12, 0.30)
    } else {
        (-0.065, 0.143)
    };
    let radius = if wrapped {
        0.0205
    } else if replacement {
        0.022
    } else {
        0.020
    };
    s.add("grip", tube(radius, 0.016, length, [0.0, 0.0, start]));
    for i in 0..9 {
        s.add(
            if wrapped { "grip" } else { "metal" },
            tube(
                radius + 0.0015,
                0.017,
                if wrapped { 0.009 } else { 0.003 },
                [
                    0.0,
                    0.0,
                    start + 0.009 + f64::from(i) * (length - 0.02) / 9.0,
                ],
            ),
        );
    }
    if replacement {
        s.add(
            "ceramic",
            tube(0.027, 0.016, 0.024, [0.0, 0.0, start - 0.013]),
        );
    }
    if id == "glaive_counterweight" {
        // A fat butt weight the side view cannot mistake for the standard cap.
        s.add("metal", tube(0.044, 0.011, 0.10, [0.0, 0.0, -0.67]));
        s.add("accent", tube(0.047, 0.03, 0.012, [0.0, 0.0, -0.61]));
    }
}

/// The polearm's head: a leaf broad in the vertical so the side view, which
/// is the picture's, shows it, with a fuller inlaid down the flat.
fn glaive_edge(s: &mut Solids, id: &str) {
    let broad = id == "glaive_broad_edge";
    let belly = if broad { -0.115 } else { -0.065 };
    let heel = if broad { -0.035 } else { -0.02 };
    s.add(
        "metal",
        Geometry::hull([
            [-0.009, 0.02, 1.06],
            [0.009, 0.02, 1.06],
            [-0.009, heel, 1.06],
            [0.009, heel, 1.06],
            [-0.009, 0.02, 1.48],
            [0.009, 0.02, 1.48],
            [0.0, 0.012, 1.56],
            [0.0, belly, 1.24],
        ]),
    );
    s.add(
        "ceramic",
        block([0.021, 0.012, 0.30], [-0.0105, -0.004, 1.12], 0.001),
    );
    if broad {
        // The wider head carries a second inlay along the belly.
        s.add(
            "accent",
            block([0.019, 0.01, 0.16], [-0.0095, -0.052, 1.17], 0.001),
        );
    }
}

#[allow(clippy::too_many_lines)] // Each slot recipe is a cohesive, finite catalogue entry.
fn module(weapon: Weapon, slot: Slot, id: &str) -> Solids {
    let mut s = Solids::default();
    match slot {
        Slot::Sight => sight(&mut s, weapon, id),
        Slot::Feed => feed(&mut s, weapon, id),
        Slot::ForeEnd => fore_end(&mut s, id),
        Slot::Stock => {
            shoulder(&mut s, 0.042, -0.345);
            if id == "precision_braced_stock" {
                s.add(
                    "metal",
                    Geometry::hull([
                        [-0.009, -0.026, -0.325],
                        [0.009, -0.026, -0.325],
                        [-0.009, -0.015, -0.325],
                        [0.009, -0.015, -0.325],
                        [-0.009, 0.028, -0.080],
                        [0.009, 0.028, -0.080],
                        [-0.009, 0.038, -0.080],
                        [0.009, 0.038, -0.080],
                    ]),
                );
                s.add(
                    "grip",
                    block([0.038, 0.013, 0.11], [-0.019, 0.077, -0.314], 0.002),
                );
            } else if !id.is_empty() {
                s.add(
                    "ceramic",
                    block([0.033, 0.008, 0.125], [-0.0165, 0.078, -0.317], 0.002),
                );
            }
        }
        Slot::Grip => match weapon {
            Weapon::Pistol => {
                grip(
                    &mut s,
                    if id == "pistol_slim_grip" {
                        0.029
                    } else if id.is_empty() {
                        0.033
                    } else {
                        0.037
                    },
                    0.125,
                    -0.03,
                );
                if id == "pistol_slim_grip" {
                    for x in [-0.0155, 0.013] {
                        s.add(
                            "shell",
                            block([0.0025, 0.076, 0.030], [x, -0.045, -0.025], 0.0007),
                        );
                    }
                } else if !id.is_empty() {
                    s.add(
                        "grip",
                        block([0.040, 0.015, 0.059], [-0.023, -0.058, -0.041], 0.005),
                    );
                }
            }
            Weapon::Scattergun => {
                s.add(
                    "grip",
                    block([0.074, 0.040, 0.14], [-0.037, 0.017, 0.218], 0.003),
                );
                for i in 0..6 {
                    s.add(
                        if id == "scatter_wrapped_grip" {
                            "grip"
                        } else {
                            "metal"
                        },
                        block(
                            [
                                0.078,
                                0.036,
                                if id == "scatter_wrapped_grip" {
                                    0.012
                                } else {
                                    0.003
                                },
                            ],
                            [-0.039, 0.014, 0.23 + f64::from(i) * 0.021],
                            0.002,
                        ),
                    );
                }
            }
            _ => melee_grip(&mut s, weapon, id),
        },
        Slot::Muzzle => {
            let w = if id.is_empty() { 0.094 } else { 0.114 };
            let mut housing = block([w, 0.065, 0.061], [-w / 2.0, 0.04, 0.351], 0.01);
            for x in [-0.022, 0.022] {
                housing = housing.subtract(
                    barrel(0.021, 0.072, [x, 0.073, 0.346]).union(
                        Geometry::cylinder(0.015, 0.08, 16).placed(
                            Pose::at([x, 0.073, 0.34])
                                .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
                        ),
                    ),
                );
            }
            s.add("metal", housing);
            s.add(
                "accent",
                block([w * 0.6, 0.006, 0.015], [-w * 0.3, 0.106, 0.371], 0.002),
            );
        }
        Slot::Cooling => {
            let count = if id.is_empty() { 8 } else { 12 };
            let radius = if id.is_empty() { 0.06 } else { 0.071 };
            for i in 0..count {
                s.add(
                    "metal",
                    tube(
                        radius,
                        0.044,
                        0.009,
                        [0.0, 0.075, 0.305 + f64::from(i) * 0.027],
                    ),
                );
            }
            for x in [-0.053, 0.041] {
                s.add(
                    "shell",
                    block([0.012, 0.031, 0.29], [x, 0.095, 0.301], 0.004),
                );
            }
            if !id.is_empty() {
                s.add(
                    "ceramic",
                    block([0.073, 0.024, 0.238], [-0.0365, 0.127, 0.327], 0.006),
                );
            }
        }
        Slot::Support => {
            s.add(
                "metal",
                block([0.025, 0.074, 0.044], [-0.0125, -0.040, 0.286], 0.002),
            );
            s.add(
                "grip",
                block([0.032, 0.073, 0.042], [-0.016, -0.098, 0.287], 0.003),
            );
            if !id.is_empty() {
                for x in [-0.055, 0.038] {
                    s.add(
                        "metal",
                        block([0.017, 0.094, 0.028], [x, -0.097, 0.323], 0.004),
                    );
                }
                s.add(
                    "grip",
                    block([0.14, 0.017, 0.046], [-0.07, -0.107, 0.315], 0.005),
                );
            }
        }
        Slot::Edge if weapon == Weapon::ReachGlaive => glaive_edge(&mut s, id),
        Slot::Edge => {
            let hooked = id == "blade_hooked_edge";
            let tip_x = if hooked { -0.018 } else { 0.0 };
            // Thin diamond section: polished cutting bevels and a raised central flat.
            s.add(
                "metal",
                Geometry::hull([
                    [-0.027, 0.0, 0.122],
                    [0.027, 0.0, 0.122],
                    [-0.022, 0.0, 0.51],
                    [0.022, 0.0, 0.51],
                    [tip_x, 0.0, 0.63],
                    [-0.013, 0.0035, 0.122],
                    [0.013, 0.0035, 0.122],
                    [0.0, 0.0035, 0.55],
                    [-0.013, -0.0035, 0.122],
                    [0.013, -0.0035, 0.122],
                    [0.0, -0.0035, 0.55],
                ]),
            );
            s.add(
                "ceramic",
                block([0.009, 0.0078, 0.30], [-0.0045, -0.0039, 0.14], 0.001),
            );
            if hooked {
                s.add(
                    "metal",
                    Geometry::hull([
                        [-0.024, 0.0, 0.48],
                        [-0.016, 0.0, 0.61],
                        [-0.040, 0.0, 0.57],
                        [-0.005, 0.002, 0.48],
                        [-0.005, -0.002, 0.48],
                    ]),
                );
            } else if !id.is_empty() {
                // The replacement edge has a second narrow polished shoulder.
                s.add(
                    "metal",
                    block([0.019, 0.004, 0.23], [-0.0095, -0.002, 0.17], 0.0008),
                );
            }
        }
        Slot::Head => {
            if weapon == Weapon::HeavyCutter {
                let width = if id.is_empty() { 0.115 } else { 0.14 };
                let spine = block([0.055, 0.024, 0.36], [-0.021, -0.012, 0.44], 0.003);
                s.add("shell", spine);
                // The broad visible blade lies on the inspection-camera side.
                s.add(
                    "metal",
                    Geometry::hull([
                        [0.006, -0.008, 0.46],
                        [0.006, 0.008, 0.46],
                        [-width, 0.0, 0.50],
                        [0.006, -0.008, 0.79],
                        [0.006, 0.008, 0.79],
                        [-width * 0.85, 0.0, 0.83],
                    ]),
                );
                for side in [-1.0, 1.0] {
                    let y = side * 0.012 - 0.001;
                    let plate = block([0.041, 0.002, 0.28], [-0.017, y, 0.48], 0.0005).subtract(
                        block([0.017, 0.009, 0.19], [-0.005, y - 0.003, 0.52], 0.002),
                    );
                    s.add("ceramic", plate);
                }
                for z in [0.466, 0.767] {
                    s.add(
                        "metal",
                        block([0.057, 0.028, 0.008], [-0.022, -0.014, z], 0.002),
                    );
                }
                s.add(
                    "accent",
                    block([0.017, 0.027, 0.027], [0.012, -0.0135, 0.716], 0.002),
                );
            } else {
                let radius = if id.is_empty() { 0.033 } else { 0.041 };
                s.add(
                    "shell",
                    Geometry::revolve(
                        [
                            [0.0, 0.0],
                            [radius * 0.7, 0.0],
                            [radius, 0.012],
                            [radius, 0.117],
                            [radius * 0.75, 0.138],
                            [0.0, 0.138],
                        ],
                        32,
                    )
                    .placed(
                        Pose::at([0.0, 0.0, 0.289])
                            .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
                    ),
                );
                for z in [0.31, 0.37] {
                    s.add(
                        "metal",
                        tube(radius + 0.003, radius - 0.003, 0.009, [0.0, 0.0, z]),
                    );
                }
                s.add(
                    "accent",
                    tube(radius + 0.001, radius - 0.003, 0.007, [0.0, 0.0, 0.325]),
                );
            }
        }
        Slot::Power => {
            let compact = id == "cutter_compact_power";
            s.add(
                "shell",
                block(
                    [0.055, 0.045, if compact { 0.085 } else { 0.13 }],
                    [-0.0275, -0.0225, 0.225],
                    0.003,
                ),
            );
            s.add(
                "grip",
                block([0.059, 0.049, 0.012], [-0.0295, -0.0245, 0.231], 0.002),
            );
            s.add(
                "light",
                block(
                    [0.005, 0.046, 0.018],
                    [-0.0025, -0.023, if compact { 0.281 } else { 0.319 }],
                    0.001,
                ),
            );
        }
    }
    s
}

/// A recipe's solids as the weapon is held and pictured. The field blade and
/// the heavy cutter are written flat across the frame (broad in X, the belly
/// on -X) and are stood on their edge here, guard and all, by a quarter turn
/// about the long axis: broad in Y, belly down, as the glaive is written.
/// The picture's viewer looks along X, so a blade left flat bakes to a
/// hairline, and the hand leads a cut with the frame's -Y.
fn stood(weapon: Weapon, solids: Solids) -> Solids {
    if matches!(weapon, Weapon::FieldBlade | Weapon::HeavyCutter) {
        solids.turned(Pose::default().rotated(DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)))
    } else {
        solids
    }
}

/// Build a socket-based assembly with independent, removable slot instances.
/// Geometry is deterministic; finish selection changes the material bake only.
///
/// # Errors
/// Returns the precise recipe path if an authored solid or attachment is invalid.
pub fn build(assembly: &Assembly) -> Result<Building, ValidationError> {
    if matches!(
        assembly.weapon,
        Weapon::CombatPistol | Weapon::PatrolCarbine | Weapon::MarksmanRifle | Weapon::Breacher
    ) {
        return crate::military::build(assembly);
    }
    let mut building = Building::builder("veyra:weapon-assembly");
    for material in MATERIALS {
        building = building.material(material, format!("weapon:{material}"));
    }
    let mut body = stood(assembly.weapon, chassis(assembly.weapon)).part("chassis")?;
    // A shared datum preserves the grip frame when any module is exchanged.
    // Module geometry is authored in that datum, so mating frames coincide.
    for slot in assembly.weapon.slots() {
        body.sockets.push(Socket::new(slot.key(), Pose::default()));
    }
    building = building
        .part(body)
        .instance(Instance::new("chassis", "chassis"));
    for &slot in assembly.weapon.slots() {
        let id = assembly.selected(slot).map_or("", |module| module.id);
        let key = slot.key();
        building = building
            .part(stood(assembly.weapon, module(assembly.weapon, slot, id)).part(key)?)
            .instance(Instance::new(key, key).attach_aligned("mate", "chassis", key));
    }
    building.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MODULES;
    use ashlar_manifold::{ManifoldMesher, mesh_building};

    fn assert_mesh(assembly: &Assembly) -> ashlar::MeshedBuilding {
        let building = build(assembly).unwrap_or_else(|error| {
            panic!(
                "invalid {:?} {:?}: {error}",
                assembly.weapon, assembly.modules
            )
        });
        let meshes = mesh_building(&building, &ManifoldMesher::default())
            .expect("every boolean and profile must produce a mesh");
        assert_eq!(meshes.parts.len(), assembly.weapon.slots().len() + 1);
        let triangles: usize = meshes
            .parts
            .values()
            .flatten()
            .map(|element| {
                assert!(!element.mesh.indices.is_empty(), "empty {}", element.id);
                element.mesh.indices.len() / 3
            })
            .sum();
        assert!(triangles > 200, "missing detail: {triangles}");
        assert!(triangles < 20_000, "excessive detail: {triangles}");
        let mut min = ashlar::glam::DVec3::splat(f64::INFINITY);
        let mut max = ashlar::glam::DVec3::splat(f64::NEG_INFINITY);
        for element in meshes.parts.values().flatten() {
            for &position in &element.mesh.positions {
                assert!(position.is_finite(), "non-finite vertex in {}", element.id);
                min = min.min(position);
                max = max.max(position);
            }
        }
        // All component mating frames use the chassis datum, so local bounds
        // are also assembled bounds, including protruding optional components.
        let size = max - min;
        assert!(size.min_element() > 0.0, "degenerate assembly bounds");
        // A polearm is the one chassis longer than a metre and a half.
        let limit = if assembly.weapon == Weapon::ReachGlaive {
            2.5
        } else {
            1.5
        };
        assert!(
            size.max_element() < limit,
            "unexpected attachment offset: {size:?}"
        );
        eprintln!(
            "{:?} {:?}: {triangles} triangles, bounds {min:.4?}..{max:.4?}, size {size:.4?}",
            assembly.weapon, assembly.modules
        );
        meshes
    }

    #[test]
    fn every_stock_chassis_meshes() {
        for weapon in Weapon::ALL {
            assert_mesh(&Assembly {
                weapon,
                ..Assembly::default()
            });
        }
    }

    #[test]
    fn every_compatible_replacement_meshes_and_changes_geometry() {
        let stock: BTreeMap<_, _> = Weapon::ALL
            .into_iter()
            .map(|weapon| {
                let meshes = assert_mesh(&Assembly {
                    weapon,
                    ..Assembly::default()
                });
                (weapon, meshes)
            })
            .collect();
        for module in MODULES {
            for &weapon in module.weapons {
                let mut assembly = Assembly {
                    weapon,
                    ..Assembly::default()
                };
                assembly.modules.insert(module.slot, module.id.to_owned());
                let fitted = assert_mesh(&assembly);
                let original = &stock[&weapon].parts[module.slot.key()];
                let replacement = &fitted.parts[module.slot.key()];
                // Compare every position and index, not just counts: equally
                // tessellated components still need a genuinely different shape.
                let unchanged = original.len() == replacement.len()
                    && original.iter().zip(replacement).all(|(left, right)| {
                        left.mesh.positions == right.mesh.positions
                            && left.mesh.indices == right.mesh.indices
                    });
                assert!(!unchanged, "{} on {weapon:?} has stock geometry", module.id);
            }
        }
    }

    /// The reach glaive is a polearm: about 2.1 m end to end along the
    /// weapon's axis, the grip's datum a third of the way up the shaft, and
    /// each of its two parts is seen beyond the standard piece it replaces.
    #[test]
    fn the_reach_glaive_is_a_polearm_and_its_parts_are_seen() {
        let bounds = |assembly: &Assembly| {
            let meshes = assert_mesh(assembly);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for element in meshes.parts.values().flatten() {
                for position in &element.mesh.positions {
                    lo = lo.min(position.z);
                    hi = hi.max(position.z);
                }
            }
            (lo, hi)
        };
        let stock = Assembly {
            weapon: Weapon::ReachGlaive,
            ..Assembly::default()
        };
        let (rear, front) = bounds(&stock);
        assert!((2.0..2.25).contains(&(front - rear)), "{}", front - rear);
        assert!(
            rear < -0.5 && front > 1.5,
            "the head leads and the butt trails"
        );
        let weighted = Assembly {
            modules: [(Slot::Grip, "glaive_counterweight".to_owned())].into(),
            ..stock.clone()
        };
        assert!(
            bounds(&weighted).0 < rear - 0.05,
            "the counterweight stands out behind"
        );
        let breadth = |assembly: &Assembly| {
            let meshes = assert_mesh(assembly);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for element in meshes.parts.values().flatten() {
                for position in &element.mesh.positions {
                    lo = lo.min(position.y);
                    hi = hi.max(position.y);
                }
            }
            hi - lo
        };
        let broad = Assembly {
            modules: [(Slot::Edge, "glaive_broad_edge".to_owned())].into(),
            ..stock.clone()
        };
        assert!(
            breadth(&broad) > breadth(&stock) + 0.03,
            "the broad edge is a wider head"
        );
    }

    #[test]
    fn every_authored_preset_meshes() {
        for preset in crate::presets::PRESETS {
            eprintln!("Preset {}", preset.id);
            assert_mesh(
                &preset
                    .assembly(crate::Finish::Issued)
                    .expect("compatible preset"),
            );
        }
    }

    #[test]
    fn largest_rifle_combinations_mesh() {
        for optic in ["compact_optic", "adapted_morrow_optic"] {
            let mut assembly = Assembly::default();
            for (slot, id) in [
                (Slot::Sight, optic),
                (Slot::Feed, "rifle_extended_cell"),
                (Slot::ForeEnd, "rifle_cooled_fore_end"),
            ] {
                assembly.modules.insert(slot, id.to_owned());
            }
            assert_mesh(&assembly);
        }
    }
}
