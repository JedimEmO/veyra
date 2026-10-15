//! Port Serein's four landmarks (`docs/design/port-serein-style.md`, section
//! 6): the processor, the beacon tower, the starport terminal with its
//! control tower, and Pelagic's headquarters.
//!
//! Each is a recipe with the few numbers its site decides (heights, the
//! towers' places, the drop from the pads to the square), in its own frame:
//! origin on the ground it stands on, `+z` its front, which the plan turns
//! downhill. They are built for their silhouettes first, because three of
//! them are seen from the whole basin: what reads at a kilometre is a single
//! element each (a tower's body, a hall's block), and what only reads close
//! (ribs, slits, rails) is its own element, which the coarse levels drop.
//! Lights that must be seen at night from the bay are marked `far`.

#![allow(
    clippy::many_single_char_names,
    reason = "coordinates x, y, z and a recipe p read as the geometry does"
)]

use std::f64::consts::TAU;

use ashlar::glam::{DMat3, DQuat, DVec3};
use ashlar::{Axis, Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode};

/// The landmarks' slots and the materials they bind.
pub const MATERIALS: [(&str, &str); 19] = [
    ("concrete", "serein:company-concrete"),
    ("neon", "serein:neon-teal"),
    ("grid", "serein:grid-concrete"),
    ("tower", "serein:tower-concrete"),
    ("charcoal", "serein:charcoal-concrete"),
    ("recess", "serein:recess"),
    ("steel", "serein:dark-steel"),
    ("pipe", "serein:pipe"),
    ("hazard", "serein:hazard"),
    ("warning", "serein:warning"),
    ("composite", "serein:composite"),
    ("teal", "serein:painted-steel"),
    ("lamp", "serein:beacon-lamp"),
    ("new", "serein:composite-new"),
    ("curtain", "serein:curtain"),
    ("white-steel", "serein:white-steel"),
    ("glass", "serein:glass"),
    ("sheet", "serein:corrugated"),
    ("slit", "serein:slit-lit"),
];

/// One exchange tower of the processor.
#[derive(Clone, Copy, Debug)]
pub struct Tower {
    /// Its axis, in the processor's frame, `(x, z)`.
    pub at: [f64; 2],
    /// Its height to the mouth, metres.
    pub height: f64,
    /// Its radius at the foot, metres; the waist and mouth follow.
    pub radius: f64,
}

/// The processor: exchange towers on the ridge, a buttressed intake hall
/// along the platform's downhill edge, the desalination tanks, and gantries
/// between the towers.
#[derive(Clone, Debug)]
pub struct Processor {
    /// The towers.
    pub towers: Vec<Tower>,
    /// The hall's width along `x`, metres.
    pub hall_width: f64,
    /// Where the hall's front face stands, `z`.
    pub hall_front: f64,
    /// Where the tanks' row stands, `x`, and the `z` it runs between.
    pub tanks: (f64, [f64; 2]),
}

/// Builds `elements` into one part placed once, bound to [`MATERIALS`].
pub(crate) fn building(id: &str, elements: Vec<Element>) -> anyhow::Result<Building> {
    let used: Vec<String> = elements
        .iter()
        .flat_map(|e| std::iter::once(e.material_slot.clone()).chain(e.cut_material_slot.clone()))
        .collect();
    let mut part = Part::builder(format!("{id}-part"));
    for element in elements {
        part = part.element(element);
    }
    let mut builder = Building::builder(id).part(part.build()?);
    for (slot, key) in MATERIALS {
        if used.iter().any(|u| u == slot) {
            builder = builder.material(slot, key);
        }
    }
    Ok(builder
        .instance(Instance::new(format!("{id}-0"), format!("{id}-part")))
        .build()?)
}

/// A walkway between two towers, `length` long along `x` and centred on the
/// origin: a deck between two trusses of chords, verticals every four
/// metres and alternating diagonals, braced across overhead, so it reads as
/// steelwork against the sky rather than as a box. The chords and verticals
/// are thick enough (a coarse level drops members under 35 cm across) to
/// stay with the diagonals from afar.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a few dozen panels"
)]
fn truss(length: f64) -> Geometry {
    const HIGH: f64 = 3.0;
    const SIDE: f64 = 1.7;
    let x0 = -length * 0.5;
    let panels = (length / 4.0).round().max(1.0) as u32;
    let step = length / f64::from(panels);
    let mut members = vec![slab([0.0, 0.0, 0.0], [length, 0.3, SIDE * 2.0 + 0.1])];
    for z in [-SIDE, SIDE] {
        for y in [0.3, HIGH] {
            members.push(rod(DVec3::new(x0, y, z), DVec3::new(-x0, y, z), 0.2, 6));
        }
        for k in 0..=panels {
            let x = x0 + step * f64::from(k);
            members.push(rod(DVec3::new(x, 0.3, z), DVec3::new(x, HIGH, z), 0.18, 6));
            if k < panels {
                let (a, b) = if k % 2 == 0 { (0.3, HIGH) } else { (HIGH, 0.3) };
                members.push(rod(
                    DVec3::new(x, a, z),
                    DVec3::new(x + step, b, z),
                    0.07,
                    6,
                ));
            }
        }
    }
    for k in 0..=panels {
        let x = x0 + step * f64::from(k);
        members.push(rod(
            DVec3::new(x, HIGH, -SIDE),
            DVec3::new(x, HIGH, SIDE),
            0.07,
            6,
        ));
    }
    Geometry::union_all(members)
}

/// The union of `solids`, which may be one.
fn merge_all(mut solids: Vec<Geometry>) -> Geometry {
    if solids.len() == 1 {
        solids.remove(0)
    } else {
        Geometry::union_all(solids)
    }
}

/// A rod of `radius` from `a` to `b`.
fn rod(a: DVec3, b: DVec3, radius: f64, segments: u32) -> Geometry {
    let along = b - a;
    let length = along.length();
    Geometry::cylinder(radius, length, segments)
        .placed(Pose::at(a.to_array()).rotated(DQuat::from_rotation_arc(DVec3::Y, along / length)))
}

/// A box of `size` centred on `(x, z)`, standing on `y`.
fn slab([x, y, z]: [f64; 3], size: [f64; 3]) -> Geometry {
    Geometry::cuboid(size).placed(Pose::at([x - size[0] * 0.5, y, z - size[2] * 0.5]))
}

impl Tower {
    /// The radius at height `h`: a flared foot, a waist at seven tenths of
    /// the height, and a mouth opening again.
    fn radius_at(&self, h: f64) -> f64 {
        let (foot, waist, mouth) = (self.radius, self.radius * 0.52, self.radius * 0.66);
        let at = self.height * 0.7;
        if h < at {
            let t = (at - h) / at;
            waist + (foot - waist) * t * t
        } else {
            let t = (h - at) / (self.height - at);
            waist + (mouth - waist) * t * t
        }
    }

    /// The tower's elements, in the processor's frame.
    #[allow(
        clippy::cast_precision_loss,
        clippy::too_many_lines,
        reason = "a few dozen profile points; one tower, element by element"
    )]
    fn elements(&self, id: &str) -> Vec<Element> {
        let h = self.height;
        let at = |g: Geometry| g.placed(Pose::at([self.at[0], 0.0, self.at[1]]));
        let mouth = self.radius_at(h);
        // The body: a plinth, the curve, and the mouth hollowed eight metres
        // down, so its top reads as an opening.
        let mut profile = vec![[0.0, 0.0], [self.radius, 0.0], [self.radius, 5.0]];
        for i in 1..=18 {
            let y = 5.0 + (h - 5.0) * f64::from(i) / 18.0;
            profile.push([self.radius_at(y), y]);
        }
        profile.extend([[mouth - 1.5, h], [mouth - 1.5, h - 8.0], [0.0, h - 8.0]]);
        let body = Element::new(
            format!("{id}-body"),
            at(Geometry::revolve(profile, 40)),
            "tower",
        )
        .uv(UvMode::Cylindrical { axis: Axis::Y })
        .collision(Collision::Hull);
        // The hazard ring round the mouth.
        let lip = Element::new(
            format!("{id}-lip"),
            at(Geometry::revolve(
                [
                    [mouth - 1.5, h],
                    [mouth + 1.3, h],
                    [mouth + 1.3, h + 1.6],
                    [mouth - 1.5, h + 1.6],
                ],
                40,
            )),
            "hazard",
        )
        .far();
        // Bands every eighteen metres: the ribs.
        let mut bands = Vec::new();
        let mut y = 22.0;
        while y < h - 12.0 {
            let r = self.radius_at(y);
            bands.push(Geometry::revolve(
                [
                    [r - 0.4, y],
                    [r + 0.55, y],
                    [r + 0.55, y + 1.2],
                    [r - 0.4, y + 1.2],
                ],
                40,
            ));
            y += 18.0;
        }
        let bands = Element::new(
            format!("{id}-bands"),
            at(Geometry::union_all(bands)),
            "charcoal",
        );
        // Eight buttresses round the foot, a quarter of the height, their
        // inner edge following the curve.
        let rise = h * 0.25;
        let fin = Geometry::extrude(
            [
                [self.radius - 1.5, 0.0],
                [self.radius + 7.0, 0.0],
                [self.radius_at(rise) - 1.5, rise],
                [self.radius_at(rise * 0.5) - 1.5, rise * 0.5],
            ],
            1.4,
        )
        // Profile x out along the radius, profile z up, the extrusion across.
        .placed(
            Pose::at([0.0, 0.0, 0.7]).rotated(DQuat::from_mat3(&DMat3::from_cols(
                DVec3::X,
                DVec3::NEG_Z,
                DVec3::Y,
            ))),
        );
        let fins = Geometry::union_all((0..8).map(|i| {
            fin.clone().placed(
                Pose::at([0.0; 3]).rotated(DQuat::from_rotation_y(TAU * f64::from(i) / 8.0 + 0.2)),
            )
        }));
        let fins =
            Element::new(format!("{id}-buttresses"), at(fins), "tower").collision(Collision::Hull);
        // Red lights at the mouth and at the waist, for the night and the
        // shuttles.
        let mut lights = Vec::new();
        for (y, r) in [
            (h + 1.6, mouth + 0.9),
            (h * 0.7, self.radius_at(h * 0.7) + 0.2),
        ] {
            for i in 0..4 {
                let (s, c) = (TAU * f64::from(i) / 4.0).sin_cos();
                lights.push(slab([c * r, y, s * r], [0.7, 0.7, 0.7]));
            }
        }
        let lights = Element::new(
            format!("{id}-lights"),
            at(Geometry::union_all(lights)),
            "warning",
        )
        .far();
        // The intake: a dark band of louvres round the foot above the
        // buttresses, where the air the tower exchanges is drawn in, its
        // vanes standing proud of the recess.
        let (i0, i1) = (h * 0.25 + 2.0, h * 0.25 + 12.0);
        let (r0, r1) = (self.radius_at(i1), self.radius_at(i0));
        let recess = Geometry::revolve(
            [
                [r1 - 0.8, i0],
                [r1 + 0.05, i0],
                [r0 + 0.05, i1],
                [r0 - 0.8, i1],
            ],
            40,
        );
        // Inset 5 cm from the recess's ends, whose faces they would
        // otherwise share.
        let (v0, v1) = (i0 + 0.05, i1 - 0.05);
        let vane = Geometry::hull([
            [r1 - 0.3, v0, -0.12],
            [r1 + 0.6, v0, -0.12],
            [r1 - 0.3, v0, 0.12],
            [r1 + 0.6, v0, 0.12],
            [r0 - 0.3, v1, -0.12],
            [r0 + 0.6, v1, -0.12],
            [r0 - 0.3, v1, 0.12],
            [r0 + 0.6, v1, 0.12],
        ]);
        #[allow(clippy::cast_precision_loss, reason = "a few dozen vanes")]
        let vanes = Geometry::union_all((0..36).map(|k| {
            vane.clone().placed(
                Pose::at([0.0; 3]).rotated(DQuat::from_rotation_y(TAU * f64::from(k) / 36.0)),
            )
        }));
        let intake = Element::new(format!("{id}-intake"), at(recess), "recess");
        let vanes = Element::new(format!("{id}-vanes"), at(vanes), "steel");
        // The crown: a gallery ring under the mouth on brackets, its
        // railing, and the exhaust's stiffening fins rising past the lip, so
        // the top reads as a machine rather than a chimney.
        let g = h - 6.0;
        let rg = self.radius_at(g);
        let gallery = Geometry::union_all([
            Geometry::revolve(
                [[rg, g], [rg + 2.4, g], [rg + 2.4, g + 0.35], [rg, g + 0.35]],
                40,
            ),
            Geometry::revolve(
                [
                    [rg + 2.3, g + 0.35],
                    [rg + 2.4, g + 0.35],
                    [rg + 2.4, g + 1.45],
                    [rg + 2.3, g + 1.45],
                ],
                40,
            ),
        ]);
        #[allow(clippy::cast_precision_loss, reason = "a few dozen brackets and fins")]
        let crown = Geometry::union_all((0..16).flat_map(|k| {
            let turn = DQuat::from_rotation_y(TAU * f64::from(k) / 16.0);
            let bracket = Geometry::hull([
                [rg - 0.2, g - 2.2, -0.15],
                [rg - 0.2, g - 2.2, 0.15],
                [rg - 0.2, g, -0.15],
                [rg - 0.2, g, 0.15],
                [rg + 2.2, g, -0.15],
                [rg + 2.2, g, 0.15],
            ])
            .placed(Pose::at([0.0; 3]).rotated(turn));
            let fin = Geometry::hull([
                [mouth - 0.3, h - 10.0, -0.2],
                [mouth - 0.3, h - 10.0, 0.2],
                [mouth - 0.3, h + 3.2, -0.2],
                [mouth - 0.3, h + 3.2, 0.2],
                [mouth + 1.1, h + 3.2, -0.2],
                [mouth + 1.1, h + 3.2, 0.2],
                [mouth + 0.2, h - 10.0, -0.2],
                [mouth + 0.2, h - 10.0, 0.2],
            ])
            .placed(
                Pose::at([0.0; 3])
                    .rotated(DQuat::from_rotation_y(TAU * (f64::from(k) + 0.5) / 16.0)),
            );
            [bracket, fin]
        }));
        let gallery = Element::new(format!("{id}-gallery"), at(gallery), "steel");
        let crown = Element::new(format!("{id}-crown"), at(crown), "steel");
        // A caged ladder up the tower's seaward face, from the buttresses to
        // the gallery.
        let (l0, l1) = (h * 0.25 + 12.5, g);
        let r_mid = self.radius_at(f64::midpoint(l0, l1));
        let ladder = Geometry::union_all([
            slab([-0.35, l0, r_mid + 0.9], [0.08, l1 - l0, 0.08]),
            slab([0.35, l0, r_mid + 0.9], [0.08, l1 - l0, 0.08]),
            slab([0.0, l0, r_mid + 1.6], [0.9, l1 - l0, 0.06]),
        ]);
        let ladder = Element::new(format!("{id}-ladder"), at(ladder), "steel");
        vec![
            body, lip, bands, fins, lights, intake, vanes, gallery, crown, ladder,
        ]
    }
}

/// The processor, in its platform's frame: origin at the platform's centre
/// on its level, `+z` downhill.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
pub fn processor(p: &Processor) -> anyhow::Result<Building> {
    let mut elements = Vec::new();
    for (i, tower) in p.towers.iter().enumerate() {
        elements.extend(tower.elements(&format!("tower-{i}")));
    }
    // The intake hall: a long block with buttresses on its front and an
    // intake between each pair, and plant on its roof.
    let (w, depth, tall) = (p.hall_width, 30.0, 24.0);
    let front = p.hall_front;
    let bays = (w / 20.0).floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few bays"
    )]
    let count = bays as u32;
    let intakes = Geometry::cuboid([12.0, 9.0, 2.0])
        .placed(Pose::at([-w * 0.5 + 4.0, 4.0, front - 1.0]))
        .arrayed(count, Pose::at([20.0, 0.0, 0.0]));
    let hall = slab([0.0, 0.0, front - depth * 0.5], [w, tall, depth]).subtract(intakes);
    elements.push(
        Element::new("hall", hall, "concrete")
            .cut_material("recess")
            .collision(Collision::Bounds),
    );
    let buttress = Geometry::extrude([[0.0, 0.0], [2.6, 0.0], [0.0, 22.0]], 2.2)
        // Profile x out from the face, profile z up, the extrusion along it.
        .placed(
            Pose::at([-1.1, 0.0, front]).rotated(DQuat::from_mat3(&DMat3::from_cols(
                DVec3::Z,
                DVec3::X,
                DVec3::Y,
            ))),
        );
    elements.push(Element::new(
        "hall-buttresses",
        buttress
            .placed(Pose::at([-w * 0.5, 0.0, 0.0]))
            .arrayed(count + 1, Pose::at([20.0, 0.0, 0.0])),
        "concrete",
    ));
    // Louvres in every intake, their blades tilted down, and a cornice
    // along the hall's top.
    let blade = Geometry::hull([
        [0.0, 0.0, 0.0],
        [12.0, 0.0, 0.0],
        [0.0, 0.0, 0.5],
        [12.0, 0.0, 0.5],
        [0.0, 0.1, 0.0],
        [12.0, 0.1, 0.0],
        [0.0, -0.35, 0.5],
        [12.0, -0.35, 0.5],
    ]);
    let louvres = blade
        .placed(Pose::at([-w * 0.5 + 4.0, 4.6, front - 0.9]))
        .arrayed(12, Pose::at([0.0, 0.7, 0.0]))
        .arrayed(count, Pose::at([20.0, 0.0, 0.0]));
    elements.push(Element::new("hall-louvres", louvres, "steel"));
    elements.push(Element::new(
        "hall-cornice",
        slab(
            [0.0, tall - 1.2, front - depth * 0.5],
            [w + 1.2, 1.25, depth + 1.2],
        ),
        "tower",
    ));
    let plant = Geometry::union_all([
        slab([-w * 0.3, tall, front - 15.0], [18.0, 5.0, 12.0]),
        slab([w * 0.1, tall, front - 12.0], [26.0, 3.5, 9.0]),
        slab([w * 0.35, tall, front - 18.0], [10.0, 6.0, 10.0]),
    ]);
    elements.push(Element::new("hall-plant", plant, "steel"));
    // The desalination tanks in a row.
    let (tx, [z0, z1]) = p.tanks;
    let mut tanks = Vec::new();
    let mut z = z0;
    while z <= z1 {
        tanks.push(Geometry::cylinder(5.5, 11.0, 24).placed(Pose::at([tx, 0.0, z])));
        z += 15.0;
    }
    elements.push(
        Element::new("tanks", Geometry::union_all(tanks), "pipe")
            .uv(UvMode::Cylindrical { axis: Axis::Y })
            .collision(Collision::Bounds),
    );
    // Gantries between neighbouring towers, at two fifths of the lower one.
    let mut gantries = Vec::new();
    for pair in p.towers.windows(2) {
        let y = pair[0].height.min(pair[1].height) * 0.4;
        let a = DVec3::new(pair[0].at[0], y, pair[0].at[1]);
        let b = DVec3::new(pair[1].at[0], y, pair[1].at[1]);
        let along = b - a;
        let yaw = (-along.z).atan2(along.x);
        let mid = (a + b) * 0.5;
        gantries.push(
            truss(along.length())
                .placed(Pose::at(mid.to_array()).rotated(DQuat::from_rotation_y(yaw))),
        );
    }
    if !gantries.is_empty() {
        elements.push(Element::new(
            "gantries",
            Geometry::union_all(gantries),
            "steel",
        ));
    }
    building("serein:processor", elements)
}

/// The beacon tower, in its own frame: origin at its foot on the square.
/// The Landing's survey mast, rebuilt: a composite drum and shaft, the
/// company's steel gallery added halfway up, the Terraces' equipment pod
/// and aerials clamped on since, and the lamp.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
pub fn beacon(height: f64) -> anyhow::Result<Building> {
    let top = height - 4.0;
    let drum = Geometry::revolve(
        [
            [0.0, 0.0],
            [5.0, 0.0],
            [5.0, 3.2],
            [4.6, 3.8],
            [3.6, 4.2],
            [0.0, 4.2],
        ],
        32,
    )
    .subtract(Geometry::cuboid([1.4, 2.6, 1.6]).placed(Pose::at([-0.7, 0.2, 4.2])));
    let shaft = Geometry::revolve([[0.0, 4.0], [1.9, 4.0], [1.35, top], [0.0, top]], 24);
    let collars = Geometry::union_all([0.3, 0.62].map(|t| {
        let y = 4.0 + (top - 4.0) * t;
        let r = 1.9 - 0.55 * t;
        Geometry::revolve(
            [
                [r - 0.2, y],
                [r + 0.3, y],
                [r + 0.3, y + 0.8],
                [r - 0.2, y + 0.8],
            ],
            24,
        )
    }));
    let gallery_y = top * 0.55;
    let gallery = Geometry::union_all([
        Geometry::revolve(
            [
                [1.4, gallery_y],
                [4.2, gallery_y],
                [4.2, gallery_y + 0.4],
                [1.4, gallery_y + 0.4],
            ],
            24,
        ),
        Geometry::revolve(
            [
                [4.0, gallery_y + 0.4],
                [4.2, gallery_y + 0.4],
                [4.2, gallery_y + 1.5],
                [4.0, gallery_y + 1.5],
            ],
            24,
        ),
    ]);
    let struts = Geometry::union_all((0..4).map(|i| {
        let (s, c) = (TAU * f64::from(i) / 4.0 + 0.4).sin_cos();
        rod(
            DVec3::new(c * 1.55, gallery_y - 6.0, s * 1.55),
            DVec3::new(c * 3.9, gallery_y, s * 3.9),
            0.14,
            6,
        )
    }));
    let pod =
        Geometry::chamfered_cuboid([1.6, 2.4, 1.2], 0.12).placed(Pose::at([1.2, top * 0.75, -0.6]));
    let aerials = Geometry::union_all([
        rod(
            DVec3::new(0.3, top + 2.5, 0.2),
            DVec3::new(0.3, height + 3.0, 0.2),
            0.06,
            6,
        ),
        rod(
            DVec3::new(-0.4, top + 2.5, -0.1),
            DVec3::new(-0.6, height + 1.0, -0.2),
            0.05,
            6,
        ),
    ]);
    let cap = Geometry::revolve([[0.0, top + 1.5], [1.3, top + 1.5], [0.0, top + 2.8]], 16);
    let lamp = Geometry::cylinder(0.9, 1.5, 16).placed(Pose::at([0.0, top, 0.0]));
    building(
        "serein:beacon",
        vec![
            Element::new("drum", drum, "composite")
                .cut_material("recess")
                .collision(Collision::Hull),
            Element::new("shaft", shaft, "composite")
                .uv(UvMode::Cylindrical { axis: Axis::Y })
                .collision(Collision::Hull),
            Element::new("collars", collars, "composite"),
            Element::new("gallery", gallery, "steel"),
            Element::new("struts", struts, "steel"),
            Element::new("pod", pod, "teal"),
            Element::new("aerials", aerials, "steel"),
            Element::new("cap", cap, "composite"),
            Element::new("lamp", lamp, "lamp").far(),
        ],
    )
}

/// The starport terminal, in its own frame: origin on the square's level at
/// the middle of the podium's west face, which is the retaining wall between
/// the square and the pads `rise` metres above it. `+x` runs east to the
/// pads. An arrival crosses the hall from the pads, leaves by its west door
/// and comes down the grand stair into the Exchange. The control tower
/// stands at the north end, its cab over the square.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "one building, element by element; a stair's step count"
)]
pub fn terminal(rise: f64) -> anyhow::Result<Building> {
    const LENGTH: f64 = 92.0;
    const HALL: [f64; 2] = [20.0, 72.0];
    const TALL: f64 = 11.0;
    const DOOR: f64 = 12.0;
    let podium = slab([12.0, -1.0, 0.0], [24.0, rise + 1.0, LENGTH]);
    // The grand stair west into the square, two flights and a landing.
    let steps = (rise / 0.18).ceil() as usize;
    let riser = rise / steps as f64;
    let half = steps / 2;
    let mut profile = vec![[0.0, 0.0], [0.6, 0.0]];
    let mut run = 0.6;
    for s in 0..steps {
        if s == half {
            // The landing: the tread above it runs on.
            run += 2.4;
            if let Some(last) = profile.last_mut() {
                last[0] = run;
            }
        }
        profile.push([run, -((s + 1) as f64) * riser]);
        run += 0.4;
        profile.push([run, -((s + 1) as f64) * riser]);
    }
    profile.push([run, -rise - 1.0]);
    profile.push([0.0, -rise - 1.0]);
    profile.dedup();
    let stair =
        Geometry::extrude(profile, 14.0)
            // Profile x west, profile z up, the extrusion along the face.
            .placed(Pose::at([0.0, rise + 0.02, -7.0]).rotated(DQuat::from_mat3(
                &DMat3::from_cols(DVec3::NEG_X, DVec3::Z, DVec3::Y),
            )));
    let y = rise;
    let (x0, x1) = (2.0, 2.0 + HALL[0]);
    let z_half = HALL[1] * 0.5;
    let glass_run =
        |x: f64, from: f64, to: f64| slab([x, y, f64::midpoint(from, to)], [0.3, TALL, to - from]);
    // The podium's dress, on its face to the square: a coping ring round
    // its edge, pilasters every six metres either side of the stair, a
    // dark plinth, and a teal strip under the coping.
    // Open over the stair's head, whose top tread is the coping's level.
    let coping = slab([12.0, rise - 0.3, 0.0], [24.6, 0.32, LENGTH + 0.6])
        .subtract(slab([12.0, rise - 0.5, 0.0], [23.4, 1.0, LENGTH - 0.6]))
        .subtract(slab([-0.1, rise - 1.0, 0.0], [0.9, 2.0, 14.2]));
    let mut pilasters = Vec::new();
    let mut z = 10.0;
    while z < LENGTH * 0.5 - 1.0 {
        for side in [-1.0, 1.0] {
            pilasters.push(slab([-0.12, -1.0, side * z], [0.28, rise + 0.7, 0.8]));
        }
        z += 6.0;
    }
    // Its buried back a centimetre off the pilasters'.
    let plinth = slab([-0.105, -1.0, 0.0], [0.23, 1.6, LENGTH])
        .subtract(slab([-0.1, -1.5, 0.0], [1.0, 3.0, 15.0]));
    let strip = slab([-0.31, rise - 0.42, 0.0], [0.04, 0.06, LENGTH])
        .subtract(slab([-0.31, rise - 1.0, 0.0], [1.0, 2.0, 14.5]));
    // The grand stair's handrails, down both cheeks and its middle, 0.9 m
    // over its nosings and level across its landing.
    let top_x = -0.6;
    let landing_from = -(0.6 + half as f64 * 0.4);
    let landing_to = landing_from - 2.4;
    let foot_x = -(run - 0.3);
    let landing_y = rise - half as f64 * riser;
    let mut rails = Vec::new();
    for z in [-6.85, 0.0, 6.85] {
        let at = |x: f64, y: f64| DVec3::new(x, y, z);
        let line = [
            at(top_x, rise + 0.9),
            at(landing_from, landing_y + 0.9),
            at(landing_to, landing_y + 0.9),
            at(foot_x, riser + 0.9),
        ];
        for pair in line.windows(2) {
            rails.push(rod(pair[0], pair[1], 0.035, 6));
        }
        for point in line {
            rails.push(rod(point - DVec3::new(0.0, 0.93, 0.0), point, 0.03, 6));
        }
    }
    let mut elements = vec![
        Element::new("podium", podium, "grid").collision(Collision::Bounds),
        Element::new("grand-stair", stair, "grid").collision(Collision::Hull),
        Element::new(
            "podium-dress",
            Geometry::union_all([coping, merge_all(pilasters)]),
            "new",
        ),
        Element::new("podium-plinth", plinth, "concrete"),
        Element::new("stair-rails", Geometry::union_all(rails), "steel"),
        Element::new("podium-strip", strip, "neon").far(),
    ];
    // Glazing west and east, each a door wide open in its middle.
    for (i, (x, door)) in [(x0, DOOR), (x1, DOOR + 4.0)].into_iter().enumerate() {
        elements.push(
            Element::new(
                format!("glass-{i}-n"),
                glass_run(x, -z_half, -door * 0.5),
                "curtain",
            )
            .collision(Collision::Bounds),
        );
        elements.push(
            Element::new(
                format!("glass-{i}-s"),
                glass_run(x, door * 0.5, z_half),
                "curtain",
            )
            .collision(Collision::Bounds),
        );
    }
    // Mullions every two metres and a transom at door height, standing
    // proud of the glazing on both faces: the frame that makes glass read as
    // a curtain wall.
    let mut frame = Vec::new();
    for x in [x0, x1] {
        let mut z = -z_half + 2.0;
        while z < z_half - 0.5 {
            frame.push(slab([x, y, z], [0.5, TALL, 0.14]));
            z += 2.0;
        }
        for (from, to) in [(-z_half, -DOOR * 0.5), (DOOR * 0.5, z_half)] {
            frame.push(slab(
                [x, y + 3.6, f64::midpoint(from, to)],
                [0.5, 0.14, to - from],
            ));
        }
        frame.push(slab([x, y + TALL - 0.5, 0.0], [0.6, 0.5, HALL[1]]));
    }
    elements.push(Element::new(
        "glazing-frame",
        Geometry::union_all(frame),
        "white-steel",
    ));
    for (i, z) in [-z_half, z_half].into_iter().enumerate() {
        elements.push(
            Element::new(
                format!("end-{i}"),
                slab([f64::midpoint(x0, x1), y, z], [HALL[0], TALL, 0.4]),
                "new",
            )
            .collision(Collision::Bounds),
        );
    }
    let roof = Geometry::union_all([
        slab(
            [f64::midpoint(x0, x1), y + TALL, 0.0],
            [HALL[0] + 8.0, 0.9, HALL[1] + 8.0],
        ),
        // The door canopy over the stair's head.
        slab([x0 - 2.5, y + 4.6, 0.0], [5.0, 0.4, DOOR + 4.0]),
    ]);
    elements.push(Element::new("roof", roof, "new"));
    // The pad-side canopy on slim columns.
    let canopy = slab([x1 + 5.0, y + 7.0, 0.0], [10.0, 0.5, 52.0]);
    let columns = Geometry::cylinder(0.25, 7.0, 10)
        .placed(Pose::at([x1 + 9.5, y, -25.0]))
        .arrayed(6, Pose::at([0.0, 0.0, 10.0]));
    elements.push(Element::new("canopy", canopy, "white-steel"));
    elements.push(Element::new("canopy-columns", columns, "white-steel"));
    // The control tower at the north end: a white shaft, a glazed cab that
    // widens outwards, and its roof.
    let (tx, tz) = (12.0, -LENGTH * 0.5 + 5.0);
    let shaft_top = y + 26.0;
    let cab = Geometry::hull(
        [
            [-3.0, 0.0, -3.0],
            [3.0, 0.0, -3.0],
            [3.0, 0.0, 3.0],
            [-3.0, 0.0, 3.0],
            [-4.3, 4.5, -4.3],
            [4.3, 4.5, -4.3],
            [4.3, 4.5, 4.3],
            [-4.3, 4.5, 4.3],
        ]
        .map(|[a, b, c]| [a + tx, b + shaft_top, c + tz]),
    );
    elements.push(
        Element::new(
            "tower-shaft",
            Geometry::cylinder(2.3, 26.0, 20).placed(Pose::at([tx, y, tz])),
            "new",
        )
        .uv(UvMode::Cylindrical { axis: Axis::Y })
        .collision(Collision::Hull),
    );
    // The cab in the terminal's own curtain wall: plain glass read as a
    // black slab against the sky.
    elements.push(Element::new("tower-cab", cab, "curtain"));
    // Dark steel rings up the shaft every four metres, where its sections
    // join, and a catwalk round the foot of the cab on brackets.
    let rings = Geometry::cylinder(2.42, 0.3, 20)
        .placed(Pose::at([tx, y + 3.0, tz]))
        .arrayed(6, Pose::at([0.0, 4.0, 0.0]));
    // Wide enough to clear the cab's corners, which splay past 4.4 m.
    let catwalk = Geometry::cylinder(4.7, 0.18, 24)
        .placed(Pose::at([tx, shaft_top - 0.4, tz]))
        .subtract(Geometry::cylinder(2.2, 0.5, 20).placed(Pose::at([tx, shaft_top - 0.5, tz])));
    let railing = Geometry::cylinder(4.66, 0.06, 24)
        .placed(Pose::at([tx, shaft_top + 0.6, tz]))
        .subtract(Geometry::cylinder(4.58, 0.2, 24).placed(Pose::at([tx, shaft_top + 0.5, tz])));
    let posts = Geometry::union_all((0..12).map(|i| {
        let a = std::f64::consts::TAU * f64::from(i) / 12.0;
        Geometry::cylinder(0.03, 0.85, 6).placed(Pose::at([
            tx + 4.62 * a.cos(),
            shaft_top - 0.22,
            tz + 4.62 * a.sin(),
        ]))
    }));
    elements.push(
        Element::new(
            "tower-steel",
            Geometry::union_all([rings, catwalk, railing, posts]),
            "steel",
        )
        .uv(UvMode::Cylindrical { axis: Axis::Y }),
    );
    elements.push(Element::new(
        "tower-roof",
        slab([tx, shaft_top + 4.5, tz], [9.8, 0.6, 9.8]).union(rod(
            DVec3::new(tx + 2.0, shaft_top + 5.1, tz),
            DVec3::new(tx + 2.0, shaft_top + 10.0, tz),
            0.08,
            6,
        )),
        "new",
    ));
    elements.push(
        Element::new(
            "tower-light",
            slab([tx + 2.0, shaft_top + 10.0, tz], [0.5, 0.5, 0.5]),
            "warning",
        )
        .far(),
    );
    building("serein:terminal", elements)
}

/// Pelagic's headquarters, in its own frame: origin at the middle of its
/// footprint on its terrace, `+z` its front. The darkest and heaviest
/// building in the city: a block of slit windows raised on piers over its
/// own terrace, a monumental recessed entrance framed by two fins, and a
/// crown.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
pub fn headquarters(storeys: u32) -> anyhow::Result<Building> {
    const WIDTH: f64 = 56.0;
    const DEPTH: f64 = 22.0;
    const LIFT: f64 = 7.0;
    const STOREY: f64 = 4.2;
    const BAY: f64 = 4.4;
    const BAYS: u32 = 12;
    let tall = f64::from(storeys) * STOREY;
    let top = LIFT + tall;
    // The two executive floors under the crown are glass, behind the fins.
    let glazed = 2.min(storeys - 1);
    let solid = storeys - glazed;
    let solid_top = LIFT + f64::from(solid) * STOREY;
    let start = -f64::from(BAYS) * BAY * 0.5;
    let piers = Geometry::cuboid([2.4, LIFT, 2.4])
        .placed(Pose::at([-WIDTH * 0.5 + 3.8, 0.0, -8.2]))
        .arrayed(5, Pose::at([12.0, 0.0, 0.0]))
        .arrayed(2, Pose::at([0.0, 0.0, 14.0]));
    // Two lit slits to a bay on both long faces, a storey apart.
    let slits = |z: f64| {
        Geometry::cuboid([0.6, 2.8, 1.2])
            .placed(Pose::at([start + 1.1 - 0.3, LIFT + 0.7, z - 0.6]))
            .arrayed(2, Pose::at([2.2, 0.0, 0.0]))
            .arrayed(BAYS, Pose::at([BAY, 0.0, 0.0]))
            .arrayed(solid, Pose::at([0.0, STOREY, 0.0]))
    };
    let block = slab([0.0, LIFT, 0.0], [WIDTH, solid_top - LIFT, DEPTH])
        .subtract(slits(DEPTH * 0.5))
        .subtract(slits(-DEPTH * 0.5));
    let glass = slab(
        [0.0, solid_top - 0.01, 0.0],
        [WIDTH - 2.4, top - solid_top + 0.02, DEPTH - 2.4],
    );
    // A fin at every bay line on both long faces, from the piers' top into
    // the crown: over the glass floors they stand free, a colonnade. Cast a
    // shade lighter than the charcoal, so by day the facade has relief.
    let fins = |z: f64, out: f64| {
        // Two tenths of a metre into the face and a metre proud of it.

        Geometry::cuboid([0.7, tall + 1.0, 1.2])
            .placed(Pose::at([
                start - 0.35,
                LIFT,
                if out > 0.0 { z - 0.2 } else { z - 1.0 },
            ]))
            .arrayed(BAYS + 1, Pose::at([BAY, 0.0, 0.0]))
    };
    let crown = slab([0.0, top, 0.0], [WIDTH + 3.0, 1.8, DEPTH + 3.0]);
    // A teal strip under the crown's edge, the company's colour at night.
    let strip = slab(
        [0.0, top - 0.08, DEPTH * 0.5 + 1.2],
        [WIDTH + 2.6, 0.08, 0.14],
    )
    .union(slab(
        [0.0, top - 0.08, -DEPTH * 0.5 - 1.2],
        [WIDTH + 2.6, 0.08, 0.14],
    ));
    // The entrance: a lobby under the block, set well back, its door deep in
    // its face under a steel canopy, and the two fins that make it the
    // city's front door, lit up their faces.
    let lobby = slab([0.0, 0.0, -1.0], [20.0, LIFT, 10.0])
        .subtract(Geometry::cuboid([8.0, 5.5, 1.2]).placed(Pose::at([-4.0, 0.0, 3.2])));
    let entrance = Geometry::union_all(
        [-7.0, 7.0].map(|x| slab([x, 0.0, DEPTH * 0.5 + 1.5], [1.6, 20.0, 3.0])),
    );
    let canopy = slab([0.0, 5.6, 6.5], [12.0, 0.35, 5.0]);
    let fin_lights = Geometry::union_all(
        [-7.0, 7.0].map(|x| slab([x, 2.0, DEPTH * 0.5 + 3.02], [0.18, 16.0, 0.06])),
    );
    let plant = slab([-WIDTH * 0.2, top + 1.8, -2.0], [14.0, 3.0, 9.0]);
    let mast = rod(
        DVec3::new(WIDTH * 0.3, top + 1.8, 0.0),
        DVec3::new(WIDTH * 0.3, top + 12.0, 0.0),
        0.12,
        6,
    );
    building(
        "serein:headquarters",
        vec![
            Element::new("piers", piers, "charcoal").collision(Collision::Bounds),
            Element::new("block", block, "charcoal")
                .cut_material("slit")
                .collision(Collision::Bounds),
            Element::new("glass", glass, "curtain").collision(Collision::Bounds),
            Element::new(
                "fins",
                Geometry::union_all([fins(DEPTH * 0.5, 1.0), fins(-DEPTH * 0.5, -1.0)]),
                "tower",
            ),
            Element::new("crown", crown, "charcoal"),
            Element::new("lobby", lobby, "charcoal")
                .cut_material("recess")
                .collision(Collision::Bounds),
            Element::new("entrance", entrance, "charcoal").collision(Collision::Hull),
            Element::new("steel", Geometry::union_all([canopy, plant, mast]), "steel"),
            Element::new("neon", strip.union(fin_lights), "neon").far(),
            Element::new(
                "mast-light",
                slab([WIDTH * 0.3, top + 12.0, 0.0], [0.5, 0.5, 0.5]),
                "warning",
            )
            .far(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_landmark_validates() {
        processor(&Processor {
            towers: vec![
                Tower {
                    at: [-45.0, -15.0],
                    height: 160.0,
                    radius: 17.0,
                },
                Tower {
                    at: [20.0, -25.0],
                    height: 180.0,
                    radius: 17.0,
                },
                Tower {
                    at: [70.0, 5.0],
                    height: 120.0,
                    radius: 12.0,
                },
            ],
            hall_width: 160.0,
            hall_front: 65.0,
            tanks: (-88.0, [-60.0, 20.0]),
        })
        .expect("the processor");
        beacon(48.0).expect("the beacon");
        terminal(6.0).expect("the terminal");
        headquarters(7).expect("the headquarters");
    }
}
