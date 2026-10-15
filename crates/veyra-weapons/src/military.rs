//! Vigil's shared machined fittings and restrained, human-scale silhouettes.
//! Profiles establish the large shapes; recesses and small bevels catch light.
use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

use ashlar::glam::DQuat;
use ashlar::{Building, Element, Geometry, Instance, Part, Pose, Socket, ValidationError};

use crate::{Assembly, Slot, Weapon};

#[derive(Default)]
struct Solids(BTreeMap<&'static str, Vec<Geometry>>);

impl Solids {
    fn add(&mut self, material: &'static str, geometry: Geometry) {
        self.0.entry(material).or_default().push(geometry);
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
    Geometry::chamfered_cuboid(size, bevel).placed(Pose::at(at))
}

// Convex side profiles are extruded across X. Slopes are silhouette geometry,
// rather than a large bevel trying to disguise rectangular primitives.
fn profile(width: f64, yz: &[[f64; 2]]) -> Geometry {
    Geometry::hull(
        yz.iter()
            .flat_map(|&[y, z]| [[-width / 2.0, y, z], [width / 2.0, y, z]]),
    )
}

fn cylinder(radius: f64, length: f64, at: [f64; 3]) -> Geometry {
    Geometry::cylinder(radius, length, 24)
        .placed(Pose::at(at).rotated(DQuat::from_rotation_x(FRAC_PI_2)))
}

fn tube(radius: f64, bore: f64, length: f64, at: [f64; 3]) -> Geometry {
    Geometry::cylinder(radius, length, 24)
        .subtract(Geometry::cylinder(bore, length + 0.004, 24).placed(Pose::at([0.0, -0.002, 0.0])))
        .placed(Pose::at(at).rotated(DQuat::from_rotation_x(FRAC_PI_2)))
}

fn cross_pin(radius: f64, length: f64, at: [f64; 3]) -> Geometry {
    Geometry::cylinder(radius, length, 16)
        .placed(Pose::at(at).rotated(DQuat::from_rotation_z(-FRAC_PI_2)))
}

// Thin fastened cover, with real bored counterseats and slotted recessed heads.
fn cover(s: &mut Solids, width: f64, y: f64, z: f64, height: f64, length: f64) {
    for side in [-1.0, 1.0] {
        let x = side * width / 2.0;
        let panel_x = if side > 0.0 { x - 0.001 } else { x - 0.002 };
        let mut panel = block([0.003, height, length], [panel_x, y, z], 0.001);
        for dz in [0.007, length - 0.007] {
            let py = y + height / 2.0;
            let pz = z + dz;
            panel = panel.subtract(cross_pin(0.0033, 0.011, [x - 0.005, py, pz]));
            let head_x = if side > 0.0 { x - 0.001 } else { x - 0.0015 };
            let head = cross_pin(0.0026, 0.0025, [head_x, py, pz]).subtract(block(
                [0.012, 0.0008, 0.0036],
                [x - 0.006, py - 0.0004, pz - 0.0018],
                0.0002,
            ));
            s.add("metal", head);
        }
        s.add("shell", panel);
    }
}

fn rail(s: &mut Solids, y: f64, z: f64, length: f64) {
    s.add(
        "metal",
        block([0.020, 0.007, length], [-0.010, y, z], 0.001),
    );
    let mut offset = 0.003;
    while offset + 0.006 < length {
        s.add(
            "metal",
            block(
                [0.025, 0.004, 0.006],
                [-0.0125, y + 0.005, z + offset],
                0.001,
            ),
        );
        offset += 0.017;
    }
}

fn guard(s: &mut Solids) {
    let outside = profile(
        0.014,
        &[
            [0.027, 0.004],
            [0.027, 0.089],
            [-0.023, 0.090],
            [-0.034, 0.073],
            [-0.034, 0.012],
            [-0.023, 0.004],
        ],
    );
    let opening = profile(
        0.034,
        &[
            [0.023, 0.011],
            [0.023, 0.082],
            [-0.019, 0.082],
            [-0.027, 0.069],
            [-0.027, 0.018],
            [-0.020, 0.011],
        ],
    );
    s.add("metal", outside.subtract(opening));
    s.add(
        "metal",
        profile(
            0.007,
            &[
                [0.026, 0.032],
                [0.024, 0.041],
                [-0.013, 0.035],
                [-0.020, 0.028],
                [-0.014, 0.026],
            ],
        ),
    );
}

fn main_grip(s: &mut Solids, id: &str) {
    let angled = id == "vigil_angled_grip";
    let firm = id == "vigil_firm_grip";
    let heel = if angled { -0.058 } else { -0.044 };
    let width = if firm { 0.035 } else { 0.032 };
    let body = profile(
        width,
        &[
            [0.031, -0.025],
            [0.025, 0.010],
            [-0.076, heel + 0.039],
            [-0.087, heel + 0.027],
            [-0.082, heel],
            [-0.062, heel - 0.002],
        ],
    );
    s.add("grip", body);
    for side in [-1.0, 1.0] {
        for i in 0..5 {
            let t = f64::from(i);
            s.add(
                "shell",
                block(
                    [0.002, 0.003, 0.023],
                    [
                        side * width / 2.0 - 0.001,
                        -0.011 - t * 0.012,
                        -0.026 - t * if angled { 0.004 } else { 0.002 },
                    ],
                    0.0007,
                ),
            );
        }
    }
    if firm {
        s.add(
            "grip",
            block([0.040, 0.008, 0.044], [-0.020, -0.088, -0.047], 0.002),
        );
        cover(s, 0.035, -0.021, -0.030, 0.017, 0.026);
    }
}

// These details occupy a recessed service cover and the adjacent selector
// land. The broad flats remain clean instead of being filled with noise.
fn receiver_controls(s: &mut Solids, width: f64) {
    let left = -width / 2.0;
    s.add(
        "metal",
        block(
            [0.0027, 0.013, 0.120],
            [left + 0.0007, 0.0715, -0.048],
            0.0007,
        ),
    );
    s.add(
        "grip",
        block(
            [0.0015, 0.002, 0.084],
            [left + 0.0003, 0.079, -0.032],
            0.0005,
        ),
    );
    // Small countersunk cover screws sit inside the service pocket.
    for z in [-0.042, 0.065] {
        s.add(
            "metal",
            cross_pin(0.0027, 0.003, [left + 0.0001, 0.077, z]).subtract(block(
                [0.008, 0.0007, 0.0034],
                [left - 0.002, 0.07665, z - 0.0017],
                0.0002,
            )),
        );
    }
    s.add(
        "metal",
        cross_pin(0.0047, 0.006, [left - 0.0004, 0.058, 0.019]),
    );
    s.add(
        "metal",
        block(
            [0.0027, 0.003, 0.012],
            [left - 0.002, 0.0565, 0.014],
            0.0007,
        ),
    );
    for z in [0.012, 0.024] {
        s.add(
            "accent",
            block([0.001, 0.002, 0.0025], [left - 0.0005, 0.0645, z], 0.0003),
        );
    }
    // A restrained recognition patch is part of the maker's issued finish.
    s.add(
        "accent",
        block(
            [0.0012, 0.008, 0.021],
            [left - 0.0005, 0.056, 0.122],
            0.0004,
        ),
    );
    for z in [0.126, 0.133] {
        s.add(
            "shell",
            block([0.0015, 0.0045, 0.001], [left - 0.0007, 0.0577, z], 0.0003),
        );
    }
}

#[allow(clippy::too_many_lines)] // Receiver recesses and lower fittings share the same dimensional datum.
fn receiver(s: &mut Solids, width: f64, length: f64, pistol: bool) {
    let rear = if pistol { -0.043 } else { -0.081 };
    let top = if pistol { 0.077 } else { 0.098 };
    let lower = if pistol { 0.038 } else { 0.031 };
    let outline = [
        [lower, rear + 0.010],
        [lower, rear + length - 0.008],
        [lower + 0.009, rear + length],
        [top - 0.010, rear + length],
        [top, rear + length - 0.018],
        [top, rear + 0.010],
        [top - 0.009, rear],
        [lower + 0.008, rear],
    ];
    // A narrow top land and broad side flats give the extrusion actual bevel
    // planes in cross-section, while preserving every attachment datum.
    let half = width / 2.0;
    let mut body = Geometry::hull(outline.into_iter().flat_map(|[y, z]| {
        let side_y = y.clamp(lower + 0.003, top - 0.005);
        let mut points = vec![[-half, side_y, z], [half, side_y, z]];
        // Interior points on the side flats can create zero-area hull facets
        // for certain widths. Only the top and bottom need inset vertices.
        if y <= lower || y >= top {
            points.extend([[-half + 0.004, y, z], [half - 0.004, y, z]]);
        }
        points
    }));
    if pistol {
        s.add(
            "accent",
            block(
                [0.0012, 0.004, 0.016],
                [-half - 0.0005, 0.051, 0.105],
                0.0004,
            ),
        );
    } else {
        body = body.subtract(block(
            [0.009, 0.018, 0.126],
            [-half - 0.006, 0.069, -0.051],
            0.002,
        ));
        body = body.subtract(cross_pin(0.0057, 0.011, [-half - 0.007, 0.058, 0.019]));
        receiver_controls(s, width);
    }
    let port_z = if pistol { 0.038 } else { 0.041 };
    body = body.subtract(block(
        [0.012, 0.019, 0.047],
        [width / 2.0 - 0.006, top - 0.030, port_z],
        0.002,
    ));
    s.add("shell", body);
    s.add(
        "metal",
        block(
            [0.002, 0.013, 0.039],
            [width / 2.0 - 0.005, top - 0.027, port_z + 0.004],
            0.0007,
        ),
    );
    // The bolt's axial stripe stays inside the recess, below the cover plane.
    s.add(
        "grip",
        block(
            [0.0025, 0.002, 0.026],
            [width / 2.0 - 0.004, top - 0.021, port_z + 0.009],
            0.0005,
        ),
    );
    cover(
        s,
        width,
        lower + 0.004,
        rear + 0.012,
        0.020,
        if pistol { 0.055 } else { 0.079 },
    );
    for z in [rear + 0.020, 0.096] {
        s.add(
            "metal",
            cross_pin(
                0.0032,
                width + 0.001,
                [-width / 2.0 - 0.0005, lower + 0.009, z],
            ),
        );
    }
    if pistol {
        for i in 0..5 {
            let z = rear + 0.010 + f64::from(i) * 0.007;
            s.add(
                "metal",
                block(
                    [width + 0.0015, 0.012, 0.002],
                    [-width / 2.0 - 0.00075, top - 0.018, z],
                    0.0005,
                ),
            );
        }
    } else {
        rail(s, top - 0.001, rear + 0.018, length - 0.036);
        s.add(
            "metal",
            cross_pin(0.004, 0.011, [-width / 2.0 - 0.010, 0.049, -0.031]),
        );
        s.add(
            "metal",
            block(
                [0.006, 0.005, 0.020],
                [-width / 2.0 - 0.010, 0.047, -0.042],
                0.001,
            ),
        );
        // A stepped lower bridges grip, trigger guard and feed well.
        s.add(
            "metal",
            profile(
                width * 0.78,
                &[
                    [0.041, -0.045],
                    [0.041, 0.164],
                    [0.013, 0.164],
                    [0.003, 0.102],
                    [0.024, 0.071],
                    [0.024, -0.022],
                ],
            ),
        );
        s.add(
            "shell",
            block(
                [width + 0.004, 0.018, 0.061],
                [-width / 2.0 - 0.002, -0.002, 0.099],
                0.002,
            ),
        );
    }
    guard(s);
}

fn handguard(s: &mut Solids, end: f64, id: &str) {
    let start = 0.172;
    let length = end - start;
    let guarded = id == "patrol_guarded_fore_end";
    let vented = id == "patrol_vented_fore_end";
    let width = if guarded { 0.048 } else { 0.043 };
    let mut shell =
        block([width, 0.054, length], [-width / 2.0, 0.040, start], 0.003).subtract(block(
            [width - 0.012, 0.039, length + 0.008],
            [-width / 2.0 + 0.006, 0.048, start - 0.004],
            0.002,
        ));
    for i in 0..if vented { 6 } else { 4 } {
        shell = shell.subtract(block(
            [width + 0.008, if vented { 0.017 } else { 0.010 }, 0.022],
            [
                -width / 2.0 - 0.004,
                0.062,
                start + 0.021 + f64::from(i) * if vented { 0.026 } else { 0.037 },
            ],
            0.002,
        ));
    }
    s.add("shell", shell);
    rail(s, 0.093, start + 0.004, length - 0.008);
    s.add(
        "grip",
        block(
            [0.034, 0.007, length - 0.035],
            [-0.017, 0.035, start + 0.016],
            0.002,
        ),
    );
    if guarded {
        for side in [-1.0, 1.0] {
            s.add(
                "grip",
                block(
                    [0.005, 0.017, length - 0.044],
                    [side * 0.022 - 0.0025, 0.043, start + 0.023],
                    0.0015,
                ),
            );
        }
        s.add(
            "metal",
            profile(
                0.020,
                &[
                    [0.042, end - 0.044],
                    [0.042, end - 0.017],
                    [0.016, end - 0.017],
                    [0.019, end - 0.028],
                ],
            ),
        );
    }
    cover(s, width, 0.044, start + 0.001, 0.016, 0.024);
}

fn chassis(weapon: Weapon) -> Solids {
    let mut s = Solids::default();
    match weapon {
        Weapon::CombatPistol => {
            receiver(&mut s, 0.031, 0.197, true);
            s.add("metal", tube(0.0095, 0.005, 0.100, [0.0, 0.058, 0.054]));
            s.add(
                "metal",
                block([0.026, 0.010, 0.078], [-0.013, 0.029, 0.051], 0.0015),
            );
            s.add(
                "metal",
                block([0.024, 0.010, 0.067], [-0.012, 0.026, -0.040], 0.002),
            );
        }
        Weapon::PatrolCarbine => {
            receiver(&mut s, 0.040, 0.262, false);
            s.add("metal", tube(0.012, 0.0055, 0.286, [0.0, 0.071, 0.172]));
            s.add("metal", tube(0.018, 0.0125, 0.020, [0.0, 0.071, 0.170]));
        }
        Weapon::MarksmanRifle => {
            receiver(&mut s, 0.042, 0.310, false);
            main_grip(&mut s, "vigil_firm_grip");
            s.add("metal", tube(0.014, 0.006, 0.472, [0.0, 0.071, 0.212]));
            s.add("metal", tube(0.021, 0.0145, 0.035, [0.0, 0.071, 0.215]));
            handguard(&mut s, 0.395, "");
            s.add(
                "grip",
                profile(
                    0.041,
                    &[
                        [0.044, 0.208],
                        [0.044, 0.391],
                        [0.025, 0.373],
                        [0.025, 0.218],
                    ],
                ),
            );
            s.add("metal", cross_pin(0.005, 0.016, [0.020, 0.080, 0.005]));
            s.add("grip", cross_pin(0.009, 0.013, [0.033, 0.080, 0.005]));
        }
        Weapon::Breacher => {
            receiver(&mut s, 0.047, 0.258, false);
            s.add("metal", tube(0.019, 0.011, 0.201, [0.0, 0.072, 0.165]));
            s.add("metal", cylinder(0.012, 0.162, [0.0, 0.040, 0.166]));
            let mut pump = profile(
                0.049,
                &[
                    [0.066, 0.191],
                    [0.066, 0.317],
                    [0.031, 0.327],
                    [0.009, 0.312],
                    [0.009, 0.202],
                    [0.027, 0.188],
                ],
            );
            for i in 0..6 {
                pump = pump.subtract(block(
                    [0.058, 0.025, 0.004],
                    [-0.029, 0.001, 0.211 + f64::from(i) * 0.017],
                    0.001,
                ));
            }
            s.add("grip", pump);
            rail(&mut s, 0.089, 0.201, 0.110);
            s.add("metal", tube(0.022, 0.0193, 0.014, [0.0, 0.072, 0.329]));
        }
        _ => unreachable!("military recipe dispatched only for Vigil chassis"),
    }
    s
}

#[allow(clippy::too_many_lines)] // Optical envelopes and clamps share one rail datum across three sight types.
fn optic(s: &mut Solids, weapon: Weapon, id: &str) {
    let pistol = weapon == Weapon::CombatPistol;
    let y = if pistol { 0.077 } else { 0.108 };
    let scope = id == "vigil_long_optic" || (id.is_empty() && weapon == Weapon::MarksmanRifle);
    let prism = id == "vigil_prism";
    if id.is_empty() && !scope {
        for z in [-0.025, if pistol { 0.120 } else { 0.146 }] {
            s.add(
                "metal",
                block([0.015, 0.013, 0.010], [-0.0075, y - 0.001, z], 0.001).subtract(block(
                    [0.005, 0.009, 0.014],
                    [-0.0025, y + 0.006, z - 0.002],
                    0.0007,
                )),
            );
        }
        return;
    }
    let base_length = if scope {
        0.112
    } else if prism {
        0.070
    } else {
        0.039
    };
    s.add(
        "metal",
        block(
            [0.030, 0.009, base_length],
            [-0.015, y - 0.004, -0.036],
            0.0015,
        ),
    );
    // Clamps straddle the rail rather than floating above it.
    for z in [-0.031, base_length - 0.047] {
        s.add(
            "metal",
            block([0.034, 0.009, 0.009], [-0.017, y - 0.005, z], 0.001),
        );
        s.add("metal", cross_pin(0.003, 0.037, [-0.0185, y, z + 0.004]));
    }
    if scope || prism {
        let length = if id == "vigil_long_optic" {
            0.251
        } else if scope {
            0.190
        } else {
            0.100
        };
        let z = if scope { -0.082 } else { -0.044 };
        let cy = y + 0.029;
        s.add("shell", tube(0.016, 0.011, length, [0.0, cy, z]));
        let front = if id == "vigil_long_optic" {
            0.026
        } else if scope {
            0.021
        } else {
            0.020
        };
        s.add(
            "shell",
            tube(front, front - 0.004, 0.035, [0.0, cy, z + length - 0.025]),
        );
        s.add("grip", tube(0.020, 0.013, 0.025, [0.0, cy, z - 0.002]));
        s.add(
            "lens",
            cylinder(front - 0.0038, 0.0015, [0.0, cy, z + length + 0.006]),
        );
        s.add("lens", cylinder(0.0125, 0.0015, [0.0, cy, z + 0.002]));
        for dz in [0.0, if scope { 0.062 } else { 0.030 }] {
            s.add(
                "metal",
                block(
                    [0.017, 0.016, 0.010],
                    [-0.0085, y + 0.002, -0.018 + dz],
                    0.001,
                ),
            );
            s.add("metal", tube(0.018, 0.0155, 0.011, [0.0, cy, -0.018 + dz]));
        }
        s.add(
            "metal",
            Geometry::cylinder(0.010, 0.010, 20).placed(Pose::at([0.0, cy + 0.013, 0.020])),
        );
        s.add("metal", cross_pin(0.009, 0.009, [0.012, cy, 0.020]));
    } else {
        s.add(
            "shell",
            block([0.029, 0.027, 0.020], [-0.0145, y + 0.003, -0.024], 0.002).subtract(block(
                [0.022, 0.020, 0.026],
                [-0.011, y + 0.008, -0.027],
                0.0015,
            )),
        );
        s.add(
            "lens",
            block(
                [0.0222, 0.0202, 0.0015],
                [-0.0111, y + 0.0079, -0.014],
                0.0005,
            ),
        );
        s.add(
            "metal",
            block([0.008, 0.010, 0.017], [0.012, y + 0.001, -0.025], 0.001),
        );
    }
}

fn feed(s: &mut Solids, weapon: Weapon, id: &str) {
    if weapon == Weapon::CombatPistol {
        let height = if id.is_empty() { 0.011 } else { 0.030 };
        s.add(
            "metal",
            block([0.028, 0.025, 0.028], [-0.014, -0.095, -0.039], 0.001),
        );
        s.add(
            "grip",
            block(
                [0.035, height, 0.043],
                [-0.0175, -0.087 - height, -0.047],
                0.002,
            ),
        );
        if !id.is_empty() {
            s.add(
                "shell",
                block([0.026, 0.014, 0.030], [-0.013, -0.112, -0.040], 0.001),
            );
        }
        return;
    }
    let compact = id == "marksman_compact_feed";
    let extended = id.contains("extended");
    let height = if compact {
        0.049
    } else if extended {
        0.146
    } else if weapon == Weapon::MarksmanRifle {
        0.072
    } else {
        0.104
    };
    let width = if weapon == Weapon::Breacher {
        0.038
    } else {
        0.031
    };
    let front = if weapon == Weapon::Breacher {
        0.175
    } else {
        0.157
    };
    let sweep = if weapon == Weapon::PatrolCarbine {
        0.019
    } else {
        0.007
    };
    s.add(
        "shell",
        profile(
            width,
            &[
                [0.025, 0.106],
                [0.025, front],
                [-height + 0.030, front + sweep],
                [-height, front + sweep - 0.007],
                [-height, 0.110 + sweep],
                [-height + 0.018, 0.105 + sweep],
            ],
        ),
    );
    s.add(
        "grip",
        block(
            [width + 0.005, 0.009, front - 0.096],
            [-width / 2.0 - 0.0025, -height - 0.004, 0.104 + sweep],
            0.002,
        ),
    );
    for side in [-1.0, 1.0] {
        for i in 0..3 {
            s.add(
                "metal",
                block(
                    [0.0016, height * 0.65, 0.0025],
                    [
                        side * width / 2.0 - 0.0008,
                        -height + 0.011,
                        0.115 + sweep + f64::from(i) * 0.011,
                    ],
                    0.0005,
                ),
            );
        }
    }
}

fn stock(s: &mut Solids, weapon: Weapon, id: &str) {
    let precision =
        id == "vigil_precision_stock" || (id.is_empty() && weapon == Weapon::MarksmanRifle);
    let wire = id == "vigil_wire_stock";
    let rear = if weapon == Weapon::MarksmanRifle {
        -0.320
    } else {
        -0.290
    };
    let tube_start = rear + 0.028;
    s.add(
        "metal",
        tube(0.012, 0.006, -0.065 - tube_start, [0.0, 0.070, tube_start]),
    );
    s.add("metal", tube(0.017, 0.0125, 0.018, [0.0, 0.070, -0.084]));
    if wire {
        for x in [-0.016, 0.011] {
            s.add(
                "metal",
                block([0.005, 0.006, 0.180], [x, 0.021, rear + 0.014], 0.001),
            );
        }
        s.add(
            "metal",
            block([0.030, 0.070, 0.010], [-0.015, 0.011, rear + 0.010], 0.002),
        );
    } else {
        let shape = profile(
            if precision { 0.040 } else { 0.035 },
            &[
                [0.085, rear + 0.020],
                [0.085, -0.109],
                [0.065, -0.095],
                [0.003, rear + 0.044],
                [-0.035, rear + 0.020],
            ],
        );
        let opening = profile(
            0.060,
            &[
                [0.056, rear + 0.042],
                [0.056, -0.153],
                [0.022, rear + 0.063],
                [0.005, rear + 0.042],
            ],
        );
        s.add("shell", shape.subtract(opening));
        s.add(
            "grip",
            block(
                [
                    if precision { 0.044 } else { 0.036 },
                    0.015,
                    if id == "vigil_precision_stock" {
                        0.125
                    } else if precision {
                        0.100
                    } else {
                        0.085
                    },
                ],
                [if precision { -0.022 } else { -0.018 }, 0.081, rear + 0.032],
                0.002,
            ),
        );
        s.add(
            "metal",
            block([0.012, 0.011, 0.026], [-0.006, 0.040, -0.154], 0.002),
        );
        cover(
            s,
            if precision { 0.040 } else { 0.035 },
            0.058,
            rear + 0.030,
            0.020,
            0.046,
        );
    }
    s.add(
        "grip",
        block([0.041, 0.128, 0.018], [-0.0205, -0.039, rear], 0.003),
    );
    if id == "vigil_precision_stock" {
        s.add(
            "metal",
            block([0.014, 0.039, 0.014], [-0.007, -0.049, rear + 0.027], 0.001),
        );
        s.add(
            "grip",
            block(
                [0.027, 0.007, 0.030],
                [-0.0135, -0.052, rear + 0.019],
                0.002,
            ),
        );
    }
}

fn muzzle(s: &mut Solids, weapon: Weapon, id: &str) {
    let (start, cy, bore, radius) = match weapon {
        Weapon::CombatPistol => (0.150, 0.058, 0.005, 0.011),
        Weapon::PatrolCarbine => (0.452, 0.071, 0.0055, 0.016),
        Weapon::MarksmanRifle => (0.677, 0.071, 0.006, 0.018),
        Weapon::Breacher => (0.355, 0.072, 0.011, 0.024),
        _ => unreachable!("military muzzle"),
    };
    let shrouded = id == "vigil_shrouded_muzzle";
    let compensated = id == "vigil_compensator";
    let length = if shrouded {
        0.112
    } else if compensated {
        0.057
    } else if weapon == Weapon::CombatPistol {
        0.027
    } else if weapon == Weapon::PatrolCarbine {
        0.038
    } else if weapon == Weapon::MarksmanRifle {
        0.033
    } else {
        0.035
    };
    let outer = if shrouded { radius + 0.006 } else { radius };
    let mut shell = tube(outer, bore + 0.001, length, [0.0, cy, start]);
    if !shrouded {
        for dz in if compensated {
            &[0.018, 0.035][..]
        } else {
            &[0.014][..]
        } {
            shell = shell.subtract(block(
                [outer * 2.0 + 0.012, outer * 0.65, 0.006],
                [-outer - 0.006, cy - outer * 0.325, start + dz],
                0.001,
            ));
        }
    }
    s.add("metal", shell);
    s.add(
        "metal",
        tube(outer + 0.002, bore + 0.001, 0.006, [0.0, cy, start]),
    );
    if shrouded {
        s.add(
            "grip",
            tube(
                outer + 0.0008,
                outer - 0.001,
                length * 0.56,
                [0.0, cy, start + 0.028],
            ),
        );
        s.add(
            "metal",
            tube(
                outer + 0.001,
                bore + 0.001,
                0.007,
                [0.0, cy, start + length - 0.007],
            ),
        );
    }
}

fn support(s: &mut Solids, id: &str) {
    s.add(
        "metal",
        block([0.028, 0.012, 0.039], [-0.014, 0.016, 0.324], 0.0015),
    );
    if id.is_empty() {
        s.add(
            "grip",
            profile(
                0.019,
                &[
                    [0.021, 0.329],
                    [0.021, 0.359],
                    [-0.002, 0.356],
                    [0.001, 0.348],
                ],
            ),
        );
    } else {
        s.add("metal", cross_pin(0.008, 0.047, [-0.0235, 0.019, 0.344]));
        for side in [-1.0, 1.0] {
            let angle = side * 0.32;
            s.add(
                "metal",
                Geometry::chamfered_cuboid([0.009, 0.102, 0.014], 0.0015).placed(
                    Pose::at([side * 0.024, -0.078, 0.337]).rotated(DQuat::from_rotation_z(angle)),
                ),
            );
            s.add(
                "grip",
                block(
                    [0.023, 0.010, 0.027],
                    [side * 0.025 - 0.0115, -0.084, 0.331],
                    0.002,
                ),
            );
        }
    }
}

fn module(weapon: Weapon, slot: Slot, id: &str) -> Solids {
    let mut s = Solids::default();
    match slot {
        Slot::Sight => optic(&mut s, weapon, id),
        Slot::Feed => feed(&mut s, weapon, id),
        Slot::Grip => main_grip(&mut s, id),
        Slot::ForeEnd => handguard(&mut s, 0.388, id),
        Slot::Stock => stock(&mut s, weapon, id),
        Slot::Muzzle => muzzle(&mut s, weapon, id),
        Slot::Support => support(&mut s, id),
        _ => unreachable!("slot is not offered by a Vigil chassis"),
    }
    s
}

pub(crate) fn build(assembly: &Assembly) -> Result<Building, ValidationError> {
    let mut building = Building::builder("veyra:weapon-assembly");
    for material in [
        "shell", "metal", "grip", "accent", "ceramic", "lens", "light",
    ] {
        building = building.material(material, format!("weapon:{material}"));
    }
    let mut body = chassis(assembly.weapon).part("chassis")?;
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
            .part(module(assembly.weapon, slot, id).part(key)?)
            .instance(Instance::new(key, key).attach_aligned("mate", "chassis", key));
    }
    building.build()
}
