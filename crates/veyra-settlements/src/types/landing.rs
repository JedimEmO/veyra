//! The Landing's composite module, dressed as the first settlers' kit was
//! delivered (`docs/design/port-serein-fidelity.md`, Pass 3).
//!
//! A module is a rounded composite shell with a band at every storey, and
//! the rest is what reads as a building rather than a container: a hatch door
//! in a frame with its step and lamp, a glazed shopfront with mullions, a
//! kick plate and a shutter box where it trades, a hood over every porthole,
//! conduits, a meter box and an air conditioner on its back, a ladder up one
//! side, and a railed roof with a mast. Everything is in the module's own
//! frame: origin at the middle of its footprint, `+z` its front.
//!
//! Every detail stands proud of the shell or sits inside a cut two
//! centimetres inside its edges, so no face of it lies on the shell's.

use std::f64::consts::{FRAC_PI_2, TAU};

use ashlar::glam::DQuat;
use ashlar::{Collision, Element, Geometry, Pose, UvMode};

use super::{LandingModule, along_z, bays, merge, rounded, slab};

/// A module's storey, metres: taller than a house's, for the plant under
/// its floors.
const STOREY: f64 = 3.4;

/// A door's opening, width and height.
const DOOR: [f64; 2] = [1.2, 2.3];

/// Everything a module is made of besides its shell, by material.
#[derive(Default)]
struct Dress {
    cuts: Vec<Geometry>,
    /// The band material: bands, porthole rings and hoods, door frames.
    bands: Vec<Geometry>,
    panes: Vec<Geometry>,
    /// Dark steel: door leaves, mullions, conduits, the ladder, railings.
    metal: Vec<Geometry>,
    /// White steel: the air conditioner, the meter box, the shutter box.
    appliance: Vec<Geometry>,
    /// Lamps that must read from afar.
    lamps: Vec<Geometry>,
}

/// A bar from `a` to `b`, `radius` thick: a rail, a rung, a pipe.
fn bar(a: [f64; 3], b: [f64; 3], radius: f64) -> Geometry {
    let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    let dir = ashlar::glam::DVec3::new(dx, dy, dz) / length;
    Geometry::cylinder(radius, length, 6)
        .placed(Pose::at(a).rotated(DQuat::from_rotation_arc(ashlar::glam::DVec3::Y, dir)))
}

/// A stretch of a rounded rectangle's outline: a straight run from a point
/// to a point, or a corner a quarter turn round a centre from an angle.
enum Run {
    Line([f64; 2], [f64; 2]),
    Corner([f64; 2], f64),
}

/// Points `spacing` apart round a rounded rectangle, `w` by `d` with corners
/// of `r`, as `(x, z)`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a few dozen posts"
)]
fn round_the(w: f64, d: f64, r: f64, spacing: f64) -> Vec<[f64; 2]> {
    let (a, b) = (w * 0.5 - r, d * 0.5 - r);
    let arc = FRAC_PI_2 * r;
    // Clockwise in plan from the middle of the front.
    let runs = [
        (a, Run::Line([0.0, d * 0.5], [a, d * 0.5])),
        (arc, Run::Corner([a, b], FRAC_PI_2)),
        (2.0 * b, Run::Line([w * 0.5, b], [w * 0.5, -b])),
        (arc, Run::Corner([a, -b], 0.0)),
        (2.0 * a, Run::Line([a, -d * 0.5], [-a, -d * 0.5])),
        (arc, Run::Corner([-a, -b], -FRAC_PI_2)),
        (2.0 * b, Run::Line([-w * 0.5, -b], [-w * 0.5, b])),
        (arc, Run::Corner([-a, b], -TAU * 0.5)),
        (a, Run::Line([-a, d * 0.5], [0.0, d * 0.5])),
    ];
    let perimeter: f64 = runs.iter().map(|(length, _)| length).sum();
    let count = (perimeter / spacing).round().max(4.0) as usize;
    let step = perimeter / count as f64;
    let mut points = Vec::with_capacity(count);
    for k in 0..count {
        let mut s = k as f64 * step;
        for (length, run) in &runs {
            if s > *length {
                s -= length;
                continue;
            }
            let t = if *length > 0.0 { s / length } else { 0.0 };
            points.push(match *run {
                Run::Line(p, q) => [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t],
                Run::Corner(c, from) => {
                    let angle = from - FRAC_PI_2 * t;
                    [c[0] + r * angle.cos(), c[1] + r * angle.sin()]
                }
            });
            break;
        }
    }
    points
}

impl Dress {
    /// A hatch door in the front at `x`: its opening, a frame of three bars
    /// standing proud, a leaf set back with a handle, a step and a lamp.
    fn hatch(&mut self, x: f64, front: f64) {
        let [w, h] = DOOR;
        self.cuts.push(slab([x, 0.05, front], [w, h, 0.6]));
        // The frame overlaps the opening by two centimetres a side, so its
        // faces are never the cut's.
        let (fw, ft, depth) = (0.16, 0.14, 0.1);
        let z = front - 0.02;
        self.bands.push(
            slab(
                [x - w * 0.5 - fw * 0.5 + 0.02, 0.0, z + depth * 0.5],
                [fw, h + 0.05 + ft - 0.02, depth],
            )
            .union(slab(
                [x + w * 0.5 + fw * 0.5 - 0.02, 0.0, z + depth * 0.5],
                [fw, h + 0.05 + ft - 0.02, depth],
            ))
            .union(slab(
                [x, 0.05 + h - 0.02, z + depth * 0.5],
                [w + 2.0 * fw - 0.04, ft, depth],
            )),
        );
        // The leaf a hand's depth into the opening, with a vision slot, a
        // bar handle and a kick plate.
        let leaf = slab([x, 0.07, front - 0.3], [w - 0.1, h - 0.06, 0.06]);
        self.metal.push(leaf);
        self.metal.push(bar(
            [x + w * 0.3, 0.9, front - 0.22],
            [x + w * 0.3, 1.35, front - 0.22],
            0.018,
        ));
        self.panes
            .push(slab([x, 1.55, front - 0.26], [0.18, 0.5, 0.02]));
        // A step under the threshold.
        self.bands
            .push(slab([x, 0.0, front + 0.28], [w + 0.5, 0.14, 0.56]));
        // A lamp over the door: a housing and its lens.
        self.metal
            .push(slab([x, h + 0.35, front + 0.1], [0.34, 0.14, 0.2]));
        self.lamps
            .push(slab([x, h + 0.3, front + 0.1], [0.28, 0.05, 0.16]));
    }

    /// A shopfront across the front: a recess glazed to the floor over a
    /// kick plate, with mullions and a transom, and a shutter box over it.
    fn shopfront(&mut self, width: f64, front: f64) {
        let (h, deep) = (2.7, 0.45);
        self.cuts
            .push(slab([0.0, 0.05, front], [width, h, deep * 2.0]));
        let back = front - deep;
        let inner = width - 0.04;
        // The glass stands a little in from the recess's back and sides; a
        // trading module binds it lit.
        self.panes
            .push(slab([0.0, 0.55, back + 0.1], [inner, h - 0.52, 0.04]));
        self.metal
            .push(slab([0.0, 0.07, back + 0.12], [inner + 0.02, 0.50, 0.1]));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few bays"
        )]
        let count = ((inner / 1.3).round() as u32).max(2);
        for i in 1..count {
            let x = -inner * 0.5 + inner * f64::from(i) / f64::from(count);
            self.metal
                .push(slab([x, 0.53, back + 0.14], [0.07, h - 0.48, 0.08]));
        }
        // The transom caps the mullions. A real overlap avoids coplanar
        // boolean slivers for the smaller infill modules' bay spacing.
        self.metal
            .push(slab([0.0, 2.2, back + 0.155], [inner + 0.02, 0.07, 0.10]));
        self.appliance.push(slab(
            [0.0, h + 0.07, front + 0.11],
            [width + 0.2, 0.24, 0.22],
        ));
    }

    /// A porthole at `(x, y)` in the front: its hole and glass, its ring,
    /// and a hood over it against the rain.
    fn porthole(&mut self, x: f64, y: f64, front: f64) {
        self.cuts.push(along_z([x, y, front - 0.3], 0.5, 0.6, 16));
        self.panes
            .push(along_z([x, y, front - 0.28], 0.5, 0.04, 16));
        // The ring's hole a little wider than the porthole, whose cut face it
        // would otherwise share.
        self.bands.push(
            along_z([x, y, front - 0.05], 0.66, 0.14, 16).subtract(along_z(
                [x, y, front - 0.1],
                0.53,
                0.3,
                16,
            )),
        );
        self.bands.push(Geometry::hull([
            [x - 0.62, y + 0.72, front - 0.02],
            [x + 0.62, y + 0.72, front - 0.02],
            [x - 0.62, y + 0.8, front - 0.02],
            [x + 0.62, y + 0.8, front - 0.02],
            [x - 0.55, y + 0.62, front + 0.3],
            [x + 0.55, y + 0.62, front + 0.3],
            [x - 0.55, y + 0.67, front + 0.3],
            [x + 0.55, y + 0.67, front + 0.3],
        ]));
    }

    /// The back's services: two conduits up to the roof, a meter box and an
    /// air conditioner on brackets.
    fn services(&mut self, flat: f64, back: f64, h: f64) {
        let z = back - 0.13;
        for x in [flat * 0.5 - 0.4, flat * 0.5 - 0.62] {
            self.metal.push(bar([x, 0.1, z], [x, h + 0.35, z], 0.06));
            self.metal
                .push(bar([x, h + 0.35, z], [x, h + 0.35, back + 0.6], 0.06));
        }
        let x = -flat * 0.5 + 0.8;
        self.appliance
            .push(slab([x, 1.1, back - 0.12], [0.6, 0.8, 0.24]));
        self.metal
            .push(bar([x, 1.1, back - 0.1], [x, 0.1, back - 0.1], 0.03));
        if flat > 3.0 {
            let x = flat * 0.1;
            self.appliance
                .push(slab([x, 2.2, back - 0.33], [0.95, 0.7, 0.62]));
            self.metal
                .push(along_z([x, 2.55, back - 0.66], 0.27, 0.03, 16));
            for dx in [-0.35, 0.35] {
                self.metal.push(bar(
                    [x + dx, 2.15, back - 0.02],
                    [x + dx, 2.15, back - 0.6],
                    0.025,
                ));
            }
        }
    }

    /// A ladder up the right side to the roof, its rails bent over the
    /// parapet.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few rungs"
    )]
    fn ladder(&mut self, side: f64, h: f64) {
        let x = side + 0.22;
        for z in [-0.24, 0.24] {
            self.metal.push(bar([x, 0.3, z], [x, h + 1.0, z], 0.025));
            self.metal
                .push(bar([x, h + 1.0, z], [side - 0.4, h + 1.0, z], 0.025));
            self.metal
                .push(bar([x, 1.8, z], [side + 0.01, 1.8, z], 0.02));
            self.metal
                .push(bar([x, h - 0.5, z], [side + 0.01, h - 0.5, z], 0.02));
        }
        let rungs = ((h + 0.6) / 0.3) as u32;
        for i in 0..rungs {
            let y = 0.4 + f64::from(i) * 0.3;
            self.metal.push(bar([x, y, -0.24], [x, y, 0.24], 0.016));
        }
    }

    /// A railing round the roof's edge and a mast with its lamp.
    fn roof(&mut self, w: f64, d: f64, r: f64, h: f64) {
        let inset = 0.35;
        let (rw, rd, rr) = (w - 2.0 * inset, d - 2.0 * inset, (r - inset).max(0.2));
        let top = h + 0.12;
        for [x, z] in round_the(rw, rd, rr, 1.7) {
            self.metal.push(bar([x, top, z], [x, top + 1.0, z], 0.025));
        }
        let rail = |y: f64, thick: f64| {
            Geometry::extrude(rounded(rw + thick, rd + thick, rr + thick * 0.5), 0.05)
                .subtract(
                    Geometry::extrude(rounded(rw - thick, rd - thick, rr - thick * 0.5), 0.07)
                        .placed(Pose::at([0.0, -0.01, 0.0])),
                )
                .placed(Pose::at([0.0, y, 0.0]))
        };
        self.metal.push(rail(top + 1.0, 0.05));
        self.metal.push(rail(top + 0.5, 0.035));
        let (x, z) = (-w * 0.5 + r + 0.6, -d * 0.5 + r + 0.6);
        self.metal.push(bar([x, top, z], [x, top + 3.2, z], 0.045));
        self.metal
            .push(bar([x - 0.4, top + 2.6, z], [x + 0.4, top + 2.6, z], 0.02));
        self.lamps
            .push(Geometry::ball(0.08, 4).placed(Pose::at([x, top + 3.28, z])));
    }

    fn elements(
        self,
        shell: Geometry,
        sign: Option<Geometry>,
        extra: Vec<Element>,
    ) -> Vec<Element> {
        let Self {
            cuts,
            bands,
            panes,
            metal,
            appliance,
            lamps,
        } = self;
        let mut elements = extra;
        elements.push(
            Element::new("shell", shell.subtract(merge(cuts)), "shell")
                .cut_material("recess")
                .collision(Collision::Hull),
        );
        elements.push(Element::new("bands", merge(bands), "band"));
        if !panes.is_empty() {
            elements.push(Element::new("panes", merge(panes), "pane").uv(UvMode::Box));
        }
        elements.push(Element::new("metal", merge(metal), "metal"));
        if !appliance.is_empty() {
            elements.push(Element::new("appliance", merge(appliance), "appliance"));
        }
        if !lamps.is_empty() {
            elements.push(Element::new("lamps", merge(lamps), "lantern").far());
        }
        if let Some(sign) = sign {
            elements.push(Element::new("sign", sign, "sign"));
        }
        elements
    }
}

impl LandingModule {
    pub(super) fn elements(&self) -> Vec<Element> {
        let (w, d) = (self.width, self.depth);
        let r = 1.4_f64.min(w * 0.25).min(d * 0.25);
        let h = f64::from(self.storeys) * STOREY;
        let front = d * 0.5;
        let flat = w - 2.0 * r;
        let holes = bays(0.0, flat.max(1.2));
        let mut dress = Dress::default();
        for storey in 0..self.storeys {
            if storey == 0 && self.shop {
                continue;
            }
            let y = f64::from(storey) * STOREY + 1.7;
            for (i, &x) in holes.iter().enumerate() {
                if storey == 0 && i == 0 {
                    continue;
                }
                dress.porthole(x, y, front);
            }
        }
        let shop_width = flat.max(2.4) * 0.9;
        let sign = if self.shop {
            dress.shopfront(shop_width, front);
            Some(slab(
                [0.0, 3.0, front + 0.12],
                [flat.max(2.4) * 0.8, 0.55, 0.08],
            ))
        } else {
            dress.hatch(holes[0], front);
            None
        };
        // A band at the foot, at every storey and round the roof.
        for k in 0..=self.storeys {
            let y = (f64::from(k) * STOREY - 0.12).max(0.0);
            dress.bands.push(
                Geometry::extrude(rounded(w + 0.16, d + 0.16, r + 0.08), 0.24)
                    .placed(Pose::at([0.0, y, 0.0])),
            );
        }
        dress.services(flat, -d * 0.5, h);
        if d - 2.0 * r >= 1.0 {
            dress.ladder(w * 0.5, h);
        }
        dress.roof(w, d, r, h);
        // Vents on their bases, each with a cap.
        let vent = |x: f64, z: f64, size: f64, tall: f64| {
            slab([x, h, z], [size + 0.3, 0.15, size + 0.3])
                .union(slab([x, h + 0.15, z], [size, tall, size]))
                .union(slab([x, h + 0.15 + tall, z], [size + 0.2, 0.1, size + 0.2]))
        };
        let mut extra = Vec::new();
        let mut roof = vec![vent(w * 0.3, -d * 0.25, 0.6, 1.0)];
        if self.dome {
            let radius = (w.min(d) * 0.3).min(3.5);
            let mut profile = vec![[0.0, 0.0]];
            for i in 0..=6 {
                let a = FRAC_PI_2 * f64::from(i) / 6.0;
                profile.push([radius * a.cos(), radius * 0.55 * a.sin()]);
            }
            profile.pop();
            profile.push([0.0, radius * 0.55]);
            extra.push(Element::new(
                "dome",
                Geometry::revolve(profile, 24).placed(Pose::at([w * 0.1, h, d * 0.05])),
                "shell",
            ));
        } else {
            roof.push(vent(-w * 0.2, -d * 0.1, 0.9, 0.7));
        }
        if self.awning {
            let half = if self.shop { shop_width * 0.53 } else { 1.0 };
            let x = if self.shop { 0.0 } else { holes[0] };
            let low = if self.shop { 2.35 } else { 2.55 };
            extra.push(Element::new(
                "awning",
                Geometry::hull([
                    [x - half, 2.82, front + 0.24],
                    [x + half, 2.82, front + 0.24],
                    [x - half, low, front + 1.9],
                    [x + half, low, front + 1.9],
                    [x - half, 2.88, front + 0.24],
                    [x + half, 2.88, front + 0.24],
                    [x - half, low + 0.06, front + 1.9],
                    [x + half, low + 0.06, front + 1.9],
                ]),
                "fabric",
            ));
        }
        extra.push(Element::new("roof", merge(roof), "band"));
        let shell = Geometry::extrude(rounded(w, d, r), h);
        dress.elements(shell, sign, extra)
    }
}
