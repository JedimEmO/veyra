//! Port Serein's building types (`docs/design/port-serein-style.md`,
//! section 3): recipes reusable anywhere their lot fits, each with the
//! numbers an owner would have chosen differently, so no two neighbours
//! match.
//!
//! The first is the Terraces' house, which is a catalogue of shared parts
//! ([`house`]): modules stacked on a terrace, the upper ones set back or
//! shifted on the roof of the one below; windows with frames, surrounds,
//! glass and sometimes louvred shutters; a door with its step; a balcony; a
//! shed, an awning, planters, a tank and the aerials and panels a roof
//! collects; a flat roof behind a parapet, a barrel vault or a lean-to of
//! corrugated sheet. The paint, the joinery's colour and the awning's cloth
//! are bound per instance, so a part is shared by every house that uses it
//! however it is painted.

#![allow(
    clippy::many_single_char_names,
    reason = "coordinates x, y, z read as the geometry does"
)]

use ashlar::glam::{DQuat, DVec3};
use ashlar::{Building, Collision, Element, Geometry, Part, Pose};

use crate::landmarks;

pub mod house;
mod landing;

pub use house::{Style, TerraceHouse, Top, Upper};

/// A storey of the Terraces, metres.
pub const STOREY: f64 = 3.2;

/// Every slot a kit (a house, a module, a stall, a tree) binds, and its
/// material when an instance binds nothing of its own.
pub const KIT_MATERIALS: [(&str, &str); 25] = [
    ("paint", "serein:paint-cream"),
    ("trim", "serein:terrace-wall"),
    ("pane", "serein:pane"),
    ("joinery", "serein:joinery-green"),
    ("appliance", "serein:white-steel"),
    ("solar", "serein:solar"),
    ("fabric", "serein:awning-sand"),
    ("garden", "serein:hedge"),
    ("sheet", "serein:corrugated"),
    ("shell", "serein:composite"),
    ("band", "serein:composite-new"),
    ("recess", "serein:recess"),
    ("sign", "serein:paint-teal"),
    ("metal", "serein:dark-steel"),
    ("boards", "serein:boards"),
    ("goods", "serein:awning-red"),
    ("bark", "flora:bark-oak"),
    ("leaves", "serein:canopy"),
    ("hazard", "serein:hazard"),
    ("container-0", "serein:container-grey"),
    ("container-1", "serein:container-rust"),
    ("container-2", "serein:container-blue"),
    ("light", "serein:floodlight"),
    ("lantern", "serein:lantern"),
    ("ink", "serein:paint-cream"),
];

/// What a window's glass is at night, most common first: dark, a warm
/// lamp, a cold screen.
pub const PANES: [&str; 3] = ["serein:pane", "serein:pane-warm", "serein:pane-cool"];

/// A lit shopfront, for the modules that trade.
pub const SHOP_LIT: &str = "serein:shop-lit";

/// What the Landing's composite came in, as the years left it.
pub const SHELLS: [&str; 2] = ["serein:composite", "serein:composite-teal"];

/// What an owner paints a house with, most common first. Bare concrete is
/// the owner who could not afford paint.
pub const PAINTS: [&str; 7] = [
    "serein:paint-cream",
    "serein:paint-white",
    "serein:terrace-wall",
    "serein:paint-ochre",
    "serein:paint-green",
    "serein:paint-teal",
    "serein:paint-oxide",
];

/// What shutters and doors are painted, against the walls.
pub const SHUTTERS: [&str; 5] = [
    "serein:paint-green",
    "serein:paint-teal",
    "serein:paint-oxide",
    "serein:paint-white",
    "serein:paint-ochre",
];

/// What a house's joinery (frames, shutters, doors, railings, downpipes) is
/// painted, against the walls.
pub const JOINERY: [&str; 6] = [
    "serein:joinery-green",
    "serein:joinery-teal",
    "serein:joinery-oxide",
    "serein:joinery-white",
    "serein:joinery-ochre",
    "serein:joinery-blue",
];

/// What a house's trim (plinth, courses, cornice, pilasters, sills, hoods,
/// coping) is: bare concrete, or rendered and painted white or cream so it
/// stands out from the walls.
pub const TRIMS: [&str; 3] = [
    "serein:terrace-wall",
    "serein:paint-white",
    "serein:paint-cream",
];

/// An awning's cloth.
pub const FABRICS: [&str; 3] = [
    "serein:awning-sand",
    "serein:awning-red",
    "serein:awning-teal",
];

/// A box of `size` centred on `(x, z)`, standing on `y`.
fn slab([x, y, z]: [f64; 3], size: [f64; 3]) -> Geometry {
    Geometry::cuboid(size).placed(Pose::at([x - size[0] * 0.5, y, z - size[2] * 0.5]))
}

/// The union of `solids`, which may be one.
fn merge(mut solids: Vec<Geometry>) -> Geometry {
    if solids.len() == 1 {
        solids.remove(0)
    } else {
        Geometry::union_all(solids)
    }
}

/// A rounded rectangle `w` by `d` round the origin with corners of `r`, as
/// a plan profile.
#[allow(clippy::cast_precision_loss, reason = "a few points a corner")]
pub(crate) fn rounded(w: f64, d: f64, r: f64) -> Vec<[f64; 2]> {
    const STEPS: u32 = 4;
    let mut points = Vec::new();
    for (cx, cz, start) in [
        (w * 0.5 - r, d * 0.5 - r, 0.0),
        (-w * 0.5 + r, d * 0.5 - r, 0.25),
        (-w * 0.5 + r, -d * 0.5 + r, 0.5),
        (w * 0.5 - r, -d * 0.5 + r, 0.75),
    ] {
        for i in 0..=STEPS {
            let a = std::f64::consts::TAU * (start + 0.25 * f64::from(i) / f64::from(STEPS));
            points.push([cx + r * a.cos(), cz + r * a.sin()]);
        }
    }
    points
}

/// The window bays across a front `width` wide: their centres.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a handful of bays"
)]
fn bays(centre: f64, width: f64) -> Vec<f64> {
    let count = ((width - 0.4) / 2.2).floor().max(1.0) as u32;
    let step = width / f64::from(count);
    (0..count)
        .map(|i| centre - width * 0.5 + step * (f64::from(i) + 0.5))
        .collect()
}

/// One of the Landing's composite modules, reused on the Exchange as a
/// shop, an office, a canteen or a clinic, in its own frame: origin at the
/// middle of its footprint, `+z` its front onto the square. A rounded shell
/// with a band at every storey, portholes, a shopfront with a painted
/// signboard and an awning where it trades, a low dome and vents on the
/// roof.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandingModule {
    /// Along its front, metres.
    pub width: f64,
    /// Front to back, metres.
    pub depth: f64,
    /// Its storeys.
    pub storeys: u32,
    /// A shopfront across the ground floor, with its signboard.
    pub shop: bool,
    /// An awning over the shopfront or the door.
    pub awning: bool,
    /// A low dome on the roof.
    pub dome: bool,
}

/// A market stall's canopy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canopy {
    /// Pitched to a ridge along the front.
    Peak,
    /// Sloping to the front.
    Slope,
    /// Flat, with a valance.
    Flat,
}

/// A market stall on the Exchange, in its own frame: origin at the middle
/// of its footprint, `+z` the aisle. Four posts, a cloth canopy, a counter
/// of boards, goods on it and crates behind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stall {
    /// Along the aisle, metres.
    pub width: f64,
    /// Front to back, metres.
    pub depth: f64,
    /// Its canopy.
    pub canopy: Canopy,
    /// How much is laid out, `0..=3`.
    pub goods: u32,
    /// How much higher than the lowest the canopy stands, metres.
    pub lift: f64,
    /// A board hanging from the canopy's front.
    pub sign: bool,
}

/// What kind of plant a spot in the plan wants; the plan picks a species of
/// the flora library for it, so the city's plants are the wilderness's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// A broad shade tree for a square or a garden.
    Shade,
    /// A narrow dark spire.
    Cypress,
    /// A bush.
    Shrub,
}

/// Anything the plan places from a kit: a part shared by every copy of the
/// same shape, colours bound per instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kit {
    /// One dwelling spanning a bank: lower lane door, roof deck and upper entry.
    SplitHouse {
        /// The upper dwelling, facing uphill from its deck.
        house: TerraceHouse,
        /// Height from the lower lane to the upper bench.
        rise: f64,
        /// Full depth including the buried retaining floor.
        depth: f64,
        /// Upper module centre relative to the retaining floor centre.
        upper_z: f64,
    },
    /// Shared port fittings from the renderer-free building parts library.
    Dressing(veyra_buildings::port::Fitting),
    /// A terrace house.
    House(TerraceHouse),
    /// A Landing module.
    Module(LandingModule),
    /// A market stall.
    Stall(Stall),
    /// A quay crane.
    Crane(Crane),
    /// A piece of street furniture.
    Furniture(Furniture),
    /// A stack of cargo containers.
    Containers(Containers),
}

/// A stack of cargo containers, in its own frame: origin at the middle of
/// the bottom one's footprint, its length along `x`. Each is ribbed sheet
/// with doors at one end, corner castings, and its own paint, bound per
/// stack level (`container-0` for the bottom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Containers {
    /// Twenty-foot or forty-foot, metres.
    pub length: f64,
    /// How many are stacked, `1..=3`.
    pub high: u32,
    /// How far each above is slid along, metres, the way a stack is never
    /// quite square.
    pub slide: f64,
}

/// The paints containers come in.
pub const CONTAINER_PAINTS: [&str; 5] = [
    "serein:container-rust",
    "serein:container-blue",
    "serein:container-green",
    "serein:container-grey",
    "serein:container-white",
];

impl Containers {
    fn elements(&self) -> Vec<Element> {
        const HIGH: f64 = 2.6;
        const WIDE: f64 = 2.44;
        let mut elements = Vec::new();
        for level in 0..self.high.min(3) {
            let y = f64::from(level) * HIGH;
            let x = self.slide * f64::from(level);
            let body = slab([x, y, 0.0], [self.length, HIGH, WIDE]);
            elements.push(
                Element::new(format!("box-{level}"), body, format!("container-{level}"))
                    .collision(Collision::Bounds),
            );
            // Corner castings and the door end's frame.
            let mut trim = Vec::new();
            for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                // Two centimetres proud of the box on every face they share.
                let (cx, cz) = (x + dx * (self.length * 0.5 - 0.1), dz * (WIDE * 0.5 - 0.1));
                trim.push(slab([cx, y + 0.02, cz], [0.24, 0.2, 0.24]));
                trim.push(slab([cx, y + HIGH - 0.19, cz], [0.24, 0.21, 0.24]));
            }
            trim.push(slab(
                [x + self.length * 0.5 + 0.02, y + 0.1, 0.0],
                [0.05, HIGH - 0.2, 0.06],
            ));
            trim.push(slab(
                [x + self.length * 0.5 + 0.02, y + 0.8, -0.6],
                [0.05, 0.05, 0.05],
            ));
            elements.push(Element::new(format!("trim-{level}"), merge(trim), "metal"));
        }
        elements
    }
}

/// Street furniture: what makes a lane a lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Furniture {
    /// A lamp on a post with an arm, its lantern over the lane.
    Lamp,
    /// A bench of boards on two concrete ends, looking out.
    Bench,
    /// A concrete planter with a bush in it.
    Planter,
    /// Crates stacked by a door.
    Crates,
    /// The Borough's hand-painted signpost at a stair's head: a post and two
    /// boards pointing different ways.
    Signpost,
    /// A Borough water kiosk: a hut with a hatch, a tank on legs beside it,
    /// a tap and a trough.
    Kiosk,
    /// A ridge tent of cloth on two poles, for a camp that moves on.
    Tent,
    /// A ring of stones round embers and two crossed logs.
    FirePit,
    /// A lookout: a decked platform on four legs under a sheet roof, with a
    /// ladder up its front.
    Lookout,
    /// A freighter's hull section: a ribbed half-pipe of plating, split open
    /// along one side, rolled and sunk into the ground.
    HullSection,
    /// A torn slab of hull plating driven into the ground at a slant, with a
    /// bent strip leaning on it.
    HullPlate,
    /// A mast's drum with its shaft canted and cut off at a slant.
    BrokenMast,
    /// A run of two pipes on low saddles with a valve block, for a pump
    /// left standing.
    Pipes,
}

impl Furniture {
    #[allow(clippy::too_many_lines, reason = "one arm a piece of furniture")]
    fn elements(self) -> Vec<Element> {
        match self {
            Self::Lamp => vec![
                Element::new(
                    "post",
                    Geometry::cylinder(0.07, 3.8, 8)
                        .union(slab([0.0, 3.6, 0.45], [0.06, 0.06, 0.9]))
                        .union(slab([0.0, 0.0, 0.0], [0.3, 0.3, 0.3])),
                    "metal",
                ),
                Element::new(
                    "lantern",
                    slab([0.0, 3.2, 0.85], [0.3, 0.4, 0.3]),
                    "lantern",
                )
                .far(),
            ],
            Self::Bench => vec![
                Element::new(
                    "ends",
                    slab([-0.75, 0.0, 0.0], [0.15, 0.42, 0.45])
                        .union(slab([0.75, 0.0, 0.0], [0.15, 0.42, 0.45])),
                    "trim",
                )
                .collision(Collision::Bounds),
                Element::new(
                    "seat",
                    slab([0.0, 0.42, 0.0], [1.8, 0.06, 0.5])
                        .union(slab([0.0, 0.5, -0.24], [1.8, 0.4, 0.05])),
                    "boards",
                ),
            ],
            Self::Planter => vec![
                Element::new("box", slab([0.0, 0.0, 0.0], [1.3, 0.55, 0.7]), "trim")
                    .collision(Collision::Bounds),
                Element::new(
                    "bush",
                    Geometry::union_all([
                        Geometry::ball(0.42, 5).placed(Pose::at([-0.3, 0.75, 0.0])),
                        Geometry::ball(0.38, 5).placed(Pose::at([0.28, 0.72, 0.05])),
                        Geometry::ball(0.3, 5).placed(Pose::at([0.0, 0.95, -0.05])),
                    ]),
                    "leaves",
                ),
            ],
            Self::Crates => vec![
                Element::new(
                    "crates",
                    Geometry::union_all([
                        slab([-0.35, 0.0, 0.0], [0.6, 0.5, 0.5]),
                        slab([0.35, 0.0, 0.05], [0.6, 0.5, 0.5]),
                        slab([0.0, 0.5, 0.0], [0.55, 0.45, 0.45]),
                    ]),
                    "boards",
                )
                .collision(Collision::Bounds),
            ],
            Self::Signpost => vec![
                Element::new("post", slab([0.0, 0.0, 0.0], [0.1, 2.5, 0.1]), "trim"),
                Element::new(
                    "boards",
                    Geometry::union_all([
                        slab([0.45, 2.05, 0.0], [0.8, 0.22, 0.04]),
                        slab([0.0, 1.7, 0.0], [0.8, 0.22, 0.04])
                            .placed(Pose::at([0.0; 3]).rotated(DQuat::from_rotation_y(2.2))),
                    ]),
                    "sign",
                ),
            ],
            Self::Kiosk => vec![
                Element::new(
                    "hut",
                    slab([0.0, 0.0, 0.0], [2.4, 2.6, 2.2])
                        .subtract(slab([0.0, 1.0, 1.1], [1.4, 0.9, 0.4])),
                    "paint",
                )
                .cut_material("trim")
                .collision(Collision::Bounds),
                Element::new(
                    "roof",
                    slab([0.0, 2.6, 0.2], [2.8, 0.12, 2.8])
                        .union(slab([0.0, 0.95, 1.25], [1.6, 0.08, 0.35])),
                    "trim",
                ),
                Element::new(
                    "tank",
                    Geometry::cylinder(0.8, 1.6, 16)
                        .placed(Pose::at([2.1, 1.3, -0.3]))
                        .union(slab([2.1, 0.0, -0.3], [1.2, 1.3, 1.2])),
                    "sheet",
                )
                .collision(Collision::Bounds),
                Element::new(
                    "tap",
                    slab([1.7, 0.0, 0.8], [0.9, 0.4, 0.5])
                        .union(slab([1.7, 0.4, 0.62], [0.06, 0.7, 0.06])),
                    "metal",
                ),
                Element::new("sign", slab([0.0, 2.15, 1.12], [1.8, 0.35, 0.04]), "sign"),
            ],
            Self::Tent => {
                let (half_width, half_length, ridge) = (1.3, 1.7, 1.9);
                vec![
                    Element::new(
                        "cloth",
                        Geometry::hull([
                            [-half_width, 0.0, -half_length],
                            [half_width, 0.0, -half_length],
                            [-half_width, 0.0, half_length],
                            [half_width, 0.0, half_length],
                            [0.0, ridge, -half_length],
                            [0.0, ridge, half_length],
                        ]),
                        "fabric",
                    )
                    .collision(Collision::Bounds),
                    Element::new(
                        "poles",
                        Geometry::union_all([-half_length - 0.05, half_length + 0.05].map(|z| {
                            Geometry::cylinder(0.04, ridge + 0.25, 6)
                                .placed(Pose::at([0.0, 0.0, z]))
                        })),
                        "metal",
                    ),
                ]
            }
            Self::FirePit => vec![
                Element::new(
                    "ring",
                    Geometry::cylinder(0.8, 0.3, 10).subtract(
                        Geometry::cylinder(0.55, 0.4, 10).placed(Pose::at([0.0, 0.1, 0.0])),
                    ),
                    "trim",
                )
                .collision(Collision::Bounds),
                Element::new(
                    "embers",
                    Geometry::cylinder(0.5, 0.12, 10).placed(Pose::at([0.0, 0.08, 0.0])),
                    "lantern",
                )
                .far(),
                Element::new(
                    "logs",
                    slab([0.0, 0.14, 0.0], [0.9, 0.1, 0.14])
                        .union(slab([0.0, 0.24, 0.0], [0.14, 0.1, 0.9])),
                    "boards",
                ),
            ],
            Self::Lookout => vec![
                Element::new(
                    "legs",
                    Geometry::union_all([(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(
                        |(x, z)| Geometry::cylinder(0.09, 5.4, 6).placed(Pose::at([x, 0.0, z])),
                    )),
                    "metal",
                )
                .collision(Collision::Bounds),
                Element::new(
                    "deck",
                    Geometry::union_all([
                        slab([0.0, 3.6, 0.0], [2.6, 0.14, 2.6]),
                        slab([0.0, 3.74, -1.25], [2.6, 0.9, 0.06]),
                        slab([-1.25, 3.74, 0.0], [0.06, 0.9, 2.6]),
                        slab([1.25, 3.74, 0.0], [0.06, 0.9, 2.6]),
                        slab([0.0, 0.0, 1.15], [0.5, 3.7, 0.05]),
                    ]),
                    "boards",
                ),
                Element::new("roof", slab([0.0, 5.4, 0.0], [3.0, 0.08, 3.0]), "sheet"),
            ],
            Self::HullSection => {
                let (radius, length) = (3.4, 9.0);
                // Built on its own axis, then rolled and pitched into the
                // ground so it lies where it fell and not like a shed.
                let along = |r: f64, extra: f64| {
                    Geometry::cylinder(r, length + extra, 18).placed(
                        Pose::at([0.0, 0.0, -length * 0.5 - extra * 0.5])
                            .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
                    )
                };
                let shell = along(radius, 0.0)
                    .subtract(along(radius - 0.22, 0.4))
                    // Torn open along the upper right.
                    .subtract(slab([2.2, 0.8, 0.0], [4.0, 5.0, length * 0.6]));
                let ribs = Geometry::union_all([-3.4, -0.4, 2.6].map(|z| {
                    let ring = |r: f64, thick: f64, dz: f64| {
                        Geometry::cylinder(r, thick, 18).placed(
                            Pose::at([0.0, 0.0, z + dz])
                                .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
                        )
                    };
                    ring(radius - 0.1, 0.25, 0.0).subtract(ring(radius - 0.5, 0.45, -0.1))
                }));
                let lie = Pose::at([0.0, -0.8, 0.0])
                    .rotated(DQuat::from_rotation_z(0.42) * DQuat::from_rotation_x(0.1));
                vec![
                    Element::new("plating", shell.placed(lie), "sheet")
                        .collision(Collision::Bounds),
                    Element::new("ribs", ribs.placed(lie), "metal"),
                ]
            }
            Self::HullPlate => {
                // A torn slab of plating driven into the ground at a slant,
                // with a bent strip leaning on it.
                let lean = Pose::at([-3.5, -0.6, -2.5])
                    .rotated(DQuat::from_rotation_x(-0.32) * DQuat::from_rotation_z(0.12));
                let strip = Pose::at([2.6, -0.4, -1.0])
                    .rotated(DQuat::from_rotation_z(-0.6) * DQuat::from_rotation_y(0.4));
                vec![
                    Element::new(
                        "plating",
                        Geometry::cuboid([7.0, 0.3, 5.0])
                            .placed(lean)
                            .union(Geometry::cuboid([0.3, 3.2, 1.4]).placed(strip)),
                        "sheet",
                    )
                    .collision(Collision::Bounds),
                ]
            }
            Self::BrokenMast => {
                // A drum with the mast's shaft canting out of it, cut off
                // at a slant: no gallery, lamp or aerial left.
                let cut = Geometry::cuboid([3.0, 3.0, 3.0])
                    .placed(Pose::at([-1.5, 5.6, -1.5]).rotated(DQuat::from_rotation_x(0.5)));
                let shaft = Geometry::cylinder(0.45, 6.5, 10)
                    .subtract(cut)
                    .placed(Pose::at([0.0, 2.8, 0.0]).rotated(DQuat::from_rotation_z(0.28)));
                vec![
                    Element::new("drum", Geometry::cylinder(2.1, 3.0, 16), "sheet")
                        .collision(Collision::Bounds),
                    Element::new("shaft", shaft, "metal"),
                ]
            }
            Self::Pipes => vec![
                Element::new(
                    "pipes",
                    Geometry::union_all([-0.5, 0.5].map(|z| {
                        Geometry::cylinder(0.3, 8.0, 10).placed(
                            Pose::at([4.0, 0.9, z])
                                .rotated(DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)),
                        )
                    })),
                    "metal",
                )
                .collision(Collision::Bounds),
                Element::new(
                    "saddles",
                    Geometry::union_all(
                        [-3.0, 0.0, 3.0].map(|x| slab([x, 0.0, 0.0], [0.4, 0.7, 1.8])),
                    ),
                    "trim",
                ),
                Element::new("valve", slab([-1.0, 0.0, 0.0], [1.0, 1.5, 1.4]), "sheet"),
            ],
        }
    }
}

/// One part a kit places, and where in the kit's frame. A kit is one part
/// in its own frame; a house is several from its catalogue.
#[derive(Clone, Debug)]
pub struct KitPiece {
    /// A key two pieces share exactly when their geometry is the same.
    pub key: String,
    /// Where it stands in the kit's frame.
    pub pose: Pose,
    source: Source,
}

#[derive(Clone, Debug)]
enum Source {
    Whole(Kit),
    House(house::Piece),
}

impl KitPiece {
    /// The part, named by its key.
    ///
    /// # Errors
    ///
    /// If the recipe does not validate, which would be a bug here.
    pub fn part(&self) -> anyhow::Result<Part> {
        match &self.source {
            Source::Whole(kit) => kit.whole(&self.key),
            Source::House(piece) => piece.part(&self.key),
        }
    }
}

impl Kit {
    /// The parts it is made of.
    #[must_use]
    pub fn pieces(&self) -> Vec<KitPiece> {
        let key = match self {
            Self::SplitHouse {
                house,
                rise,
                depth,
                upper_z,
            } => {
                let bank = house::Piece::Bank(house.width, *depth, *rise);
                let mut pieces = vec![KitPiece {
                    key: bank.key(),
                    pose: Pose::default(),
                    source: Source::House(bank),
                }];
                let upper = Pose::at([0.0, rise + 0.06, *upper_z])
                    .rotated(DQuat::from_rotation_y(std::f64::consts::PI));
                pieces.extend(house.pieces().into_iter().map(|(piece, pose)| KitPiece {
                    key: piece.key(),
                    pose: upper.compose(pose),
                    source: Source::House(piece),
                }));
                return pieces;
            }
            Self::Dressing(fitting) => fitting.key(),
            Self::House(h) => {
                return h
                    .pieces()
                    .into_iter()
                    .map(|(piece, pose)| KitPiece {
                        key: piece.key(),
                        pose,
                        source: Source::House(piece),
                    })
                    .collect();
            }
            Self::Module(m) => format!(
                "module-w{:.1}d{:.1}s{}p{}a{}d{}",
                m.width, m.depth, m.storeys, m.shop, m.awning, m.dome
            ),
            Self::Stall(s) => format!(
                "stall-w{:.1}d{:.1}c{:?}g{}l{:.1}s{}",
                s.width, s.depth, s.canopy, s.goods, s.lift, s.sign
            ),
            Self::Crane(c) => format!("crane-r{:.1}h{:.1}", c.reach, c.height),
            Self::Furniture(f) => format!("furniture-{f:?}"),
            Self::Containers(c) => format!("containers-l{:.1}h{}s{:.1}", c.length, c.high, c.slide),
        };
        vec![KitPiece {
            key,
            pose: Pose::default(),
            source: Source::Whole(*self),
        }]
    }

    /// A kit made of one part: that part, named `id`.
    fn whole(&self, id: &str) -> anyhow::Result<Part> {
        let elements = match self {
            Self::Dressing(fitting) => return Ok(fitting.part()?),
            Self::House(_) | Self::SplitHouse { .. } => {
                anyhow::bail!("a house is placed as its catalogue's parts")
            }
            Self::Module(m) => m.elements(),
            Self::Stall(s) => s.elements(),
            Self::Crane(c) => c.elements(),
            Self::Furniture(f) => f.elements(),
            Self::Containers(c) => c.elements(),
        };
        let mut part = Part::builder(id);
        for element in elements {
            part = part.element(element);
        }
        Ok(part.build()?)
    }
}

/// A cylinder of `radius` and `length` along `+z` from `(x, y, z)`.
fn along_z([x, y, z]: [f64; 3], radius: f64, length: f64, segments: u32) -> Geometry {
    Geometry::cylinder(radius, length, segments)
        .placed(Pose::at([x, y, z]).rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)))
}

impl Stall {
    /// The canopy's underside height at `z` across the stall (front at
    /// `+ez`), the cloth sagging a hand's depth between its frame bars.
    fn canopy_at(&self, z: f64, ez: f64) -> f64 {
        const SAG: f64 = 0.1;
        let up = self.lift;
        // The share of the way across the panel `z` is in, for the sag.
        let across = |from: f64, to: f64| ((z - from) / (to - from)).clamp(0.0, 1.0);
        match self.canopy {
            Canopy::Peak => {
                let u = if z < 0.0 {
                    across(-ez, 0.0)
                } else {
                    across(0.0, ez)
                };
                2.95 + up - 0.6 * (z.abs() / ez) - SAG * 4.0 * u * (1.0 - u)
            }
            Canopy::Slope => {
                let u = across(-ez, ez);
                2.75 + up - 0.45 * u - SAG * 4.0 * u * (1.0 - u)
            }
            Canopy::Flat => {
                let u = across(-ez, ez);
                2.45 + up - SAG * 4.0 * u * (1.0 - u)
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "one stall, element by element, with a few dozen folds"
    )]
    fn elements(&self) -> Vec<Element> {
        const CLOTH: f64 = 0.035;
        let (w, d) = (self.width, self.depth);
        let (hx, hz) = (w * 0.5 - 0.1, d * 0.5 - 0.1);
        let (ex, ez) = (w * 0.5 + 0.2, d * 0.5 + 0.25);
        let posts = merge(
            [(-hx, -hz), (hx, -hz), (-hx, hz), (hx, hz)]
                .into_iter()
                .map(|(x, z)| {
                    Geometry::cylinder(0.05, self.canopy_at(z, ez) + 0.02, 6)
                        .placed(Pose::at([x, 0.0, z]))
                })
                .collect(),
        );
        // The cloth: its section across the stall, sagging between the
        // frame bars, extruded along the stall's width.
        let steps = 12;
        let zs: Vec<f64> = (0..=steps)
            .map(|k| -ez + 2.0 * ez * f64::from(k) / f64::from(steps))
            .chain(if self.canopy == Canopy::Peak {
                Some(0.0)
            } else {
                None
            })
            .collect();
        let mut zs = zs;
        zs.sort_by(f64::total_cmp);
        zs.dedup();
        let mut section: Vec<[f64; 2]> = zs.iter().map(|&z| [z, self.canopy_at(z, ez)]).collect();
        section.extend(zs.iter().rev().map(|&z| [z, self.canopy_at(z, ez) + CLOTH]));
        let canopy = Geometry::extrude(section, 2.0 * ex).placed(
            Pose::at([-ex, 0.0, 0.0]).rotated(DQuat::from_mat3(&ashlar::glam::DMat3::from_cols(
                DVec3::Z,
                DVec3::X,
                DVec3::Y,
            ))),
        );
        // A scalloped valance along the front edge, each scallop a half
        // disc of cloth hanging from the hem.
        let hem = self.canopy_at(ez, ez);
        let count = ((2.0 * ex) / 0.45).floor().max(2.0) as u32;
        let span = 2.0 * ex / f64::from(count);
        let mut valance = Vec::new();
        for i in 0..count {
            let x0 = -ex + span * f64::from(i);
            let deep = if i % 2 == 0 { 0.2 } else { 0.16 };
            let mut points = Vec::new();
            for k in 0..=6 {
                let t = f64::from(k) / 6.0;
                let x = x0 + span * t;
                let y = hem + 0.02 - deep * (std::f64::consts::PI * t).sin();
                points.push([x, y, ez - 0.01]);
                points.push([x, y, ez + 0.01]);
            }
            points.push([x0, hem + 0.03, ez - 0.01]);
            points.push([x0 + span, hem + 0.03, ez + 0.01]);
            valance.push(Geometry::hull(points));
        }
        // The frame's bars under the cloth's front and back edges.
        let bar = |z: f64| {
            let y = self.canopy_at(z, ez) - 0.04;
            Geometry::cylinder(0.03, 2.0 * hx, 6).placed(
                Pose::at([-hx, y, z]).rotated(DQuat::from_rotation_z(-std::f64::consts::FRAC_PI_2)),
            )
        };
        let rails = merge(vec![bar(-hz), bar(hz)]);
        let counter = slab([0.0, 0.0, d * 0.5 - 0.4], [w - 0.3, 0.9, 0.6]);
        // A pleated skirt down the counter's front, in the canopy's cloth.
        let front = d * 0.5 - 0.1;
        let half = (w - 0.3) * 0.5 - 0.02;
        let folds = ((2.0 * half) / 0.12).floor().max(2.0) as u32;
        let pitch = 2.0 * half / f64::from(folds);
        let mut pleats: Vec<[f64; 2]> = (0..=folds)
            .map(|k| {
                let z = if k % 2 == 0 { 0.015 } else { 0.055 };
                [-half + pitch * f64::from(k), front + z]
            })
            .collect();
        pleats.extend((0..=folds).rev().map(|k| {
            let z = if k % 2 == 0 { 0.015 } else { 0.055 };
            [-half + pitch * f64::from(k), front + z + 0.015]
        }));
        let skirt = Geometry::extrude(pleats, 0.78).placed(Pose::at([0.0, 0.06, 0.0]));
        // Goods: crates of fruit, trays of greens and tins on the counter,
        // crates behind it, sacks slumped at the back, bunches hanging from
        // the front bar.
        let mut crates = Vec::new();
        let mut fruit = Vec::new();
        let mut greens = Vec::new();
        let mut tins = Vec::new();
        let mut sacks = Vec::new();
        for i in 0..self.goods {
            let x = -w * 0.32 + f64::from(i) * w * 0.64 / f64::from(self.goods.max(2) - 1);
            let z = d * 0.5 - 0.4;
            // A second tier behind the first, so a counter reads as full.
            crates.push(slab([x + 0.15, 0.9, z - 0.15], [0.45, 0.32, 0.3]));
            match (i + self.goods) % 3 {
                0 => {
                    crates.push(slab([x, 0.9, z], [0.5, 0.14, 0.4]));
                    for (k, (dx, dz)) in [
                        (-0.14, -0.1),
                        (0.0, -0.1),
                        (0.14, -0.1),
                        (-0.14, 0.1),
                        (0.0, 0.1),
                        (0.14, 0.1),
                        (-0.07, 0.0),
                        (0.07, 0.0),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let y = if k < 6 { 1.1 } else { 1.19 };
                        fruit.push(Geometry::ball(0.075, 4).placed(Pose::at([x + dx, y, z + dz])));
                    }
                }
                1 => {
                    crates.push(slab([x, 0.9, z], [0.6, 0.06, 0.45]));
                    for dx in [-0.18, 0.0, 0.18] {
                        greens.push(Geometry::hull([
                            [x + dx - 0.09, 0.96, z - 0.18],
                            [x + dx + 0.09, 0.96, z - 0.18],
                            [x + dx - 0.09, 0.96, z + 0.18],
                            [x + dx + 0.09, 0.96, z + 0.18],
                            [x + dx, 1.12, z - 0.12],
                            [x + dx, 1.12, z + 0.12],
                        ]));
                    }
                }
                _ => {
                    for (dx, dz) in [
                        (-0.15, -0.08),
                        (0.0, -0.08),
                        (0.15, -0.08),
                        (-0.08, 0.1),
                        (0.08, 0.1),
                    ] {
                        tins.push(Geometry::cylinder(0.055, 0.14, 8).placed(Pose::at([
                            x + dx,
                            0.9,
                            z + dz,
                        ])));
                    }
                }
            }
            crates.push(slab([x + 0.1, 0.0, -d * 0.3], [0.6, 0.5, 0.5]));
            sacks.push(Geometry::ball(0.28, 5).placed(Pose::at([x - 0.35, 0.22, -d * 0.3 + 0.1])));
            // A bunch hanging from the front bar.
            let hang = self.canopy_at(hz, ez) - 0.1;
            fruit.push(
                Geometry::ball(0.09, 4)
                    .placed(Pose::at([x, hang - 0.35, hz]))
                    .union(Geometry::ball(0.08, 4).placed(Pose::at([x + 0.07, hang - 0.25, hz])))
                    .union(slab([x, hang - 0.3, hz], [0.01, 0.3, 0.01])),
            );
        }
        let mut elements = vec![
            Element::new("posts", merge(vec![posts, rails]), "metal"),
            Element::new(
                "canopy",
                canopy.union(merge(valance)).union(skirt),
                "fabric",
            ),
            Element::new("counter", counter, "boards").collision(Collision::Bounds),
        ];
        if !crates.is_empty() {
            elements.push(Element::new("crates", merge(crates), "boards"));
            elements.push(Element::new("sacks", merge(sacks), "boards"));
            elements.push(Element::new("fruit", merge(fruit), "goods"));
        }
        if !greens.is_empty() {
            elements.push(Element::new("greens", merge(greens), "garden"));
        }
        if !tins.is_empty() {
            elements.push(Element::new("tins", merge(tins), "appliance"));
        }
        // A bulb hung under the canopy, for trading after dark.
        let under = self.canopy_at(0.2, ez);
        elements.push(
            Element::new(
                "bulb",
                Geometry::ball(0.09, 4)
                    .placed(Pose::at([0.0, under - 0.4, 0.2]))
                    .union(slab([0.0, under - 0.32, 0.2], [0.01, 0.33, 0.01])),
                "lantern",
            )
            .far(),
        );
        if self.sign {
            let y = hem - 0.62;
            elements.push(Element::new(
                "sign",
                slab([0.0, y, ez + 0.03], [w * 0.5, 0.3, 0.04])
                    .union(slab([-w * 0.2, y + 0.3, ez + 0.03], [0.02, 0.12, 0.02]))
                    .union(slab([w * 0.2, y + 0.3, ez + 0.03], [0.02, 0.12, 0.02])),
                "sign",
            ));
        }
        elements
    }
}

/// A quay's portal crane, in its own frame: origin between its landward
/// legs on the quay, `+z` out over the water. Two braced portal frames
/// straddle the quay's rail, a machinery house sits on them, and a boom of
/// two chords and diagonals reaches over the water with its trolley, cables
/// and hook.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crane {
    /// How far the boom reaches past the seaward legs, metres.
    pub reach: f64,
    /// The portal's height to the machinery house, metres.
    pub height: f64,
}

impl Crane {
    /// The distance between its landward and seaward legs, metres.
    pub const GAUGE: f64 = 9.0;

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few bays"
    )]
    fn elements(&self) -> Vec<Element> {
        const SPAN: f64 = 7.0;
        let gauge = Self::GAUGE;
        let h = self.height;
        let member = |a: [f64; 3], b: [f64; 3], r: f64| {
            let (a, b) = (DVec3::from_array(a), DVec3::from_array(b));
            let along = b - a;
            Geometry::cuboid([r, along.length(), r])
                .placed(Pose::at([-r * 0.5, 0.0, -r * 0.5]))
                .placed(
                    Pose::at(a.to_array())
                        .rotated(DQuat::from_rotation_arc(DVec3::Y, along.normalize())),
                )
        };
        // The portal: four legs, a sill beam each side, cross-bracing.
        let mut frame = Vec::new();
        for x in [-SPAN * 0.5, SPAN * 0.5] {
            frame.push(member([x, 0.0, 0.0], [x, h, 0.0], 0.7));
            frame.push(member([x, 0.0, gauge], [x, h, gauge], 0.7));
            frame.push(member([x, h * 0.35, 0.0], [x, h * 0.35, gauge], 0.4));
            frame.push(member([x, h * 0.35, 0.0], [x, h - 0.5, gauge], 0.25));
            frame.push(member([x, h * 0.35, gauge], [x, h - 0.5, 0.0], 0.25));
        }
        frame.push(member([-SPAN * 0.5, h, 0.0], [SPAN * 0.5, h, 0.0], 0.6));
        frame.push(member([-SPAN * 0.5, h, gauge], [SPAN * 0.5, h, gauge], 0.6));
        // The boom: two chords from behind the portal out over the water,
        // diagonals between them.
        let (z0, z1) = (-4.0, gauge + self.reach);
        let top = h + 5.0;
        let mut boom = Vec::new();
        for x in [-1.1, 1.1] {
            boom.push(member([x, h + 0.4, z0], [x, h + 0.4, z1], 0.35));
            boom.push(member([x, top, z0], [x, h + 1.8, z1], 0.3));
            let bays = ((z1 - z0) / 3.5) as u32;
            for i in 0..bays {
                let za = z0 + f64::from(i) * 3.5;
                let t = (za + 3.5 - z0) / (z1 - z0);
                let yb = top + (h + 1.8 - top) * t;
                boom.push(member([x, h + 0.4, za], [x, yb, za + 3.5], 0.14));
            }
        }
        // The machinery house on the portal, its counterweight behind.
        let house = slab([0.0, h + 0.4, 1.5], [5.0, 3.2, 6.0])
            .union(slab([0.0, h + 0.4, z0 - 1.0], [3.4, 2.4, 2.4]));
        // The trolley near the boom's tip, the cables and the hook.
        let tz = z1 - 5.0;
        let hook_y = h * 0.45;
        let gear = merge(vec![
            // Hung a little below the boom's lower chord, not on it.
            slab([0.0, h - 0.45, tz], [2.8, 0.62, 1.6]),
            slab([-0.3, hook_y, tz], [0.04, h - 0.4 - hook_y, 0.04]),
            slab([0.3, hook_y, tz], [0.04, h - 0.4 - hook_y, 0.04]),
            slab([0.0, hook_y - 0.9, tz], [0.9, 0.9, 0.5]),
        ]);
        // Floodlights under the machinery house, lighting the quay below.
        let lights = merge(vec![
            slab([-1.6, h - 0.35, 1.5], [0.7, 0.3, 0.7]),
            slab([1.6, h - 0.35, 1.5], [0.7, 0.3, 0.7]),
        ]);
        vec![
            Element::new("floodlights", lights, "light").far(),
            Element::new("frame", merge(frame), "hazard").collision(Collision::Hull),
            Element::new("boom", merge(boom), "hazard"),
            Element::new("house", house, "metal"),
            Element::new("gear", gear, "metal"),
        ]
    }
}

/// A quay warehouse, in its own frame: origin at the middle of its
/// footprint, `+z` its front onto the quay.
///
/// Corrugated sheet on a concrete plinth a loading dock high, under a gable
/// with a ridge cap and rooflights. Each bay's opening has a steel frame, a
/// shutter box and a roller shutter, some rolled half up, with dock bumpers
/// under it and hazard posts either side. Steel pilasters mark the portal
/// frames, gutters run along both eaves into downpipes, and floodlights and
/// gable louvres finish it.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "one warehouse, element by element, with a few bays"
)]
pub fn warehouse(width: f64, height: f64, depth: f64) -> anyhow::Result<Building> {
    const PLINTH: f64 = 1.0;
    const OPENING: [f64; 2] = [5.0, 5.5];
    let front = depth * 0.5;
    let doors = ((width - 4.0) / 9.0).floor().max(1.0) as u32;
    let step = width / f64::from(doors);
    let centres: Vec<f64> = (0..doors)
        .map(|k| -width * 0.5 + step * (f64::from(k) + 0.5))
        .collect();
    let door = Geometry::cuboid([OPENING[0], OPENING[1], 0.6])
        .placed(Pose::at([
            centres[0] - OPENING[0] * 0.5,
            PLINTH,
            front - 0.35,
        ]))
        .arrayed(doors, Pose::at([step, 0.0, 0.0]));
    let walls = slab([0.0, PLINTH, 0.0], [width, height - PLINTH, depth]).subtract(door);
    let plinth = slab([0.0, 0.0, 0.0], [width + 0.3, PLINTH, depth + 0.3]);
    // The gable across the depth, extruded along the width.
    let rise = depth * 0.12;
    let eave = front + 0.4;
    let roof_y = |z: f64| height + rise + 0.3 - z.abs() * rise / eave;
    let roof =
        Geometry::extrude(
            [
                [-eave, 0.0],
                [eave, 0.0],
                [eave, 0.3],
                [0.0, rise + 0.3],
                [-eave, 0.3],
            ],
            width + 0.6,
        )
        // Profile x along the depth, profile z up, the extrusion along the
        // width.
        .placed(Pose::at([-width * 0.5 - 0.3, height, 0.0]).rotated(
            ashlar::glam::DQuat::from_mat3(&ashlar::glam::DMat3::from_cols(
                ashlar::glam::DVec3::Z,
                ashlar::glam::DVec3::X,
                ashlar::glam::DVec3::Y,
            )),
        ));
    let mut steel = Vec::new();
    let mut shutters = Vec::new();
    let mut hazard = Vec::new();
    let mut lamps = Vec::new();
    let mut glass = Vec::new();
    let top = PLINTH + OPENING[1];
    for (k, &x) in centres.iter().enumerate() {
        let half = OPENING[0] * 0.5;
        // The frame: two jambs and a head of channel, proud of the sheet and
        // two centimetres over the opening, so no face of it is the cut's.
        for side in [-1.0, 1.0] {
            steel.push(slab(
                [x + side * (half + 0.1), PLINTH, front + 0.07],
                [0.24, OPENING[1] + 0.1, 0.18],
            ));
        }
        steel.push(slab(
            [x, top - 0.02, front + 0.07],
            [OPENING[0] + 0.44, 0.22, 0.18],
        ));
        // The shutter's box over the head.
        steel.push(slab(
            [x, top + 0.12, front + 0.22],
            [OPENING[0] + 0.3, 0.62, 0.5],
        ));
        // The shutter in the opening, down or rolled half up.
        let open = (k + doors as usize) % 3 == 1;
        let (from, tall) = if open {
            (PLINTH + 3.4, OPENING[1] - 3.4)
        } else {
            (PLINTH + 0.02, OPENING[1] - 0.04)
        };
        shutters.push(slab(
            [x, from, front - 0.28],
            [OPENING[0] - 0.06, tall, 0.06],
        ));
        // Dock bumpers on the plinth's face under the opening.
        for dx in [-1.8, 1.8] {
            steel.push(slab([x + dx, 0.3, front + 0.27], [0.35, 0.55, 0.24]));
        }
        // Hazard posts either side, clear of the frame.
        for dx in [-(half + 0.9), half + 0.9] {
            hazard.push(Geometry::cylinder(0.13, 1.1, 10).placed(Pose::at([
                x + dx,
                0.0,
                front + 1.1,
            ])));
        }
    }
    // Pilasters: the portal frames' columns, at the corners and between the
    // bays in front, every six metres behind.
    // The corner ones two centimetres in, so none ends on a side's plane.
    let mut columns: Vec<(f64, f64)> =
        vec![(-width * 0.5 + 0.22, front), (width * 0.5 - 0.22, front)];
    columns.extend((1..doors).map(|k| (-width * 0.5 + step * f64::from(k), front)));
    let back_bays = (width / 6.0).round().max(1.0) as u32;
    columns.extend((0..=back_bays).map(|k| {
        (
            (-width * 0.5 + 0.22) + (width - 0.44) * f64::from(k) / f64::from(back_bays),
            -front,
        )
    }));
    for &(x, z) in &columns {
        let out = z.signum();
        steel.push(slab(
            [x, PLINTH, z + out * 0.1],
            [0.4, height - PLINTH - 0.02, 0.24],
        ));
    }
    // Floodlights over every pilaster in front.
    for &(x, z) in columns.iter().filter(|(_, z)| *z > 0.0) {
        steel.push(slab([x, height - 1.0, z + 0.36], [0.5, 0.3, 0.3]));
        lamps.push(slab([x, height - 1.04, z + 0.41], [0.42, 0.05, 0.22]));
    }
    // Gutters along both eaves, into downpipes at the ends.
    for z in [front, -front] {
        let out = z.signum();
        steel.push(slab(
            [0.0, height - 0.12, z + out * 0.42],
            [width + 0.5, 0.26, 0.3],
        ));
        for x in [-width * 0.5 + 0.6, width * 0.5 - 0.6] {
            steel.push(Geometry::cylinder(0.07, height - 0.2, 8).placed(Pose::at([
                x,
                0.1,
                z + out * 0.4,
            ])));
        }
    }
    // The ridge cap, sunk into the roof's slopes.
    let side = 0.4;
    steel.push(Geometry::hull([
        [-width * 0.5 - 0.35, roof_y(side) - 0.02, side],
        [width * 0.5 + 0.35, roof_y(side) - 0.02, side],
        [-width * 0.5 - 0.35, roof_y(side) - 0.02, -side],
        [width * 0.5 + 0.35, roof_y(side) - 0.02, -side],
        [-width * 0.5 - 0.35, roof_y(0.0) + 0.12, 0.0],
        [width * 0.5 + 0.35, roof_y(0.0) + 0.12, 0.0],
    ]));
    // Rooflights on both slopes over every bay.
    for &x in &centres {
        for sign in [-1.0, 1.0] {
            let (z0, z1) = (sign * eave * 0.3, sign * eave * 0.7);
            let corner = |x: f64, z: f64, up: f64| [x, roof_y(z) + up, z];
            glass.push(Geometry::hull([
                corner(x - 1.2, z0, -0.02),
                corner(x + 1.2, z0, -0.02),
                corner(x - 1.2, z1, -0.02),
                corner(x + 1.2, z1, -0.02),
                corner(x - 1.2, z0, 0.05),
                corner(x + 1.2, z0, 0.05),
                corner(x - 1.2, z1, 0.05),
                corner(x + 1.2, z1, 0.05),
            ]));
        }
    }
    // A louvre high in each gable.
    let louvre = (rise * 0.55).min(1.2);
    for sign in [-1.0, 1.0] {
        steel.push(slab(
            [sign * (width * 0.5 + 0.35), height + 0.25, 0.0],
            [0.14, louvre, 1.8],
        ));
    }
    landmarks::building(
        "serein:warehouse",
        vec![
            Element::new("walls", walls, "sheet")
                .cut_material("recess")
                .collision(Collision::Bounds),
            Element::new("plinth", plinth, "concrete").collision(Collision::Bounds),
            Element::new("roof", roof, "sheet"),
            Element::new("steel", merge(steel), "steel"),
            Element::new("shutters", merge(shutters), "teal"),
            Element::new("posts", merge(hazard), "hazard"),
            Element::new("rooflights", merge(glass), "glass"),
            Element::new("floodlights", merge(lamps), "lamp").far(),
        ],
    )
}

/// One of Pelagic's housing slabs, in its own frame: origin at the middle
/// of its footprint, `+z` its front over the terrace below.
///
/// Charcoal concrete on pilotis. The front is a grid of lighter concrete:
/// fins between the bays and a band at every floor, the windows set deep
/// between them with a mullion and a transom each. The back has an access
/// gallery at every floor behind a solid balustrade, with a door and a
/// window to each flat. A stair core stands at one end with a glazed slot
/// the height of the building and the lobby's glass at its foot; the roof
/// has a parapet with its coping, a plant room, tanks and masts.
///
/// # Errors
///
/// If the recipe does not validate, which would be a bug here.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "one slab, element by element, with a few dozen bays"
)]
pub fn slab_block(width: f64, depth: f64, storeys: u32) -> anyhow::Result<Building> {
    const STOREY: f64 = 4.2;
    const BAY: f64 = 2.4;
    let lift = STOREY;
    let floors = storeys - 1;
    let tall = f64::from(floors) * STOREY;
    let (front, back) = (depth * 0.5, -depth * 0.5);
    let roof = lift + tall;
    // The core at the +x end; the bays run from the other end up to it.
    let core_x = width * 0.5 - 3.5;
    let run = (core_x - 2.8) - (-width * 0.5 + 0.8);
    let bays = (run / BAY).floor().max(1.0) as u32;
    let start = -width * 0.5 + 0.8 + (run - f64::from(bays) * BAY) * 0.5;
    let every_bay = Pose::at([BAY, 0.0, 0.0]);
    let every_floor = Pose::at([0.0, STOREY, 0.0]);
    // A window per bay and floor in front; a door and a small window per
    // bay on the gallery behind.
    let window = Geometry::cuboid([1.5, 2.2, 0.7])
        .placed(Pose::at([start + 0.45, lift + 0.75, front - 0.35]))
        .arrayed(bays, every_bay)
        .arrayed(floors, every_floor);
    let door = Geometry::cuboid([1.0, 2.3, 0.7])
        .placed(Pose::at([start + 0.25, lift + 0.05, back - 0.35]))
        .arrayed(bays, every_bay)
        .arrayed(floors, every_floor);
    let small = Geometry::cuboid([0.9, 1.1, 0.7])
        .placed(Pose::at([start + 1.35, lift + 1.2, back - 0.35]))
        .arrayed(bays, every_bay)
        .arrayed(floors, every_floor);
    let parapet = slab([0.0, roof, 0.0], [width, 1.0, depth]).subtract(slab(
        [0.0, roof - 0.1, 0.0],
        [width - 0.5, 1.3, depth - 0.5],
    ));
    let block = slab([0.0, lift, 0.0], [width, tall, depth])
        .union(parapet)
        .subtract(merge(vec![window, door, small]));
    // The front's grid: a fin at every bay line, a band at every floor, in
    // the lighter concrete. The fins rise into the parapet's coping.
    let fins = Geometry::cuboid([0.24, tall + 0.95, 0.6])
        .placed(Pose::at([start - 0.12, lift, front - 0.05]))
        .arrayed(bays + 1, every_bay);
    let grid_end = start + f64::from(bays) * BAY + 0.12;
    let bands = Geometry::cuboid([grid_end - (-width * 0.5 + 0.02), 0.42, 0.34])
        .placed(Pose::at([-width * 0.5 + 0.02, lift - 0.21, front - 0.04]))
        .arrayed(floors, every_floor);
    let coping = slab([0.0, roof + 0.93, 0.0], [width + 0.12, 0.12, depth + 0.12]).subtract(slab(
        [0.0, roof + 0.8, 0.0],
        [width - 0.62, 0.5, depth - 0.62],
    ));
    // The galleries: a deck at every floor behind, with a solid balustrade,
    // from the far end to the core.
    let deck_len = (core_x - 2.5) - (-width * 0.5 + 0.3);
    let deck_x = -width * 0.5 + 0.3;
    let deck = Geometry::cuboid([deck_len, 0.28, 1.7])
        .placed(Pose::at([deck_x, lift - 0.28, back - 1.65]))
        .arrayed(floors, every_floor);
    // A centimetre proud of the deck's edge and past its ends, so no face of
    // it lies on the deck's in the union.
    let balustrade = Geometry::cuboid([deck_len + 0.02, 1.05, 0.16])
        .placed(Pose::at([deck_x - 0.01, lift - 0.01, back - 1.66]))
        .arrayed(floors, every_floor);
    // Brackets under the decks at every other bay line.
    let brackets = Geometry::hull([
        [0.0, 0.0, back + 0.02],
        [0.2, 0.0, back + 0.02],
        [0.0, 0.0, back - 1.6],
        [0.2, 0.0, back - 1.6],
        [0.0, -0.9, back + 0.02],
        [0.2, -0.9, back + 0.02],
    ])
    .placed(Pose::at([start - 0.1, lift - 0.27, 0.0]))
    .arrayed(bays / 2 + 1, Pose::at([BAY * 2.0, 0.0, 0.0]))
    .arrayed(floors, every_floor);
    // Mullions and transoms in the windows, and a rail on every balustrade.
    let mullions = Geometry::cuboid([0.07, 2.2, 0.08])
        .placed(Pose::at([start + 1.165, lift + 0.75, front - 0.32]))
        .arrayed(bays, every_bay)
        .arrayed(floors, every_floor)
        .union(
            Geometry::cuboid([1.5, 0.07, 0.08])
                .placed(Pose::at([start + 0.45, lift + 2.25, front - 0.32]))
                .arrayed(bays, every_bay)
                .arrayed(floors, every_floor),
        );
    let rails = Geometry::cuboid([deck_len, 0.06, 0.1])
        .placed(Pose::at([deck_x, lift + 1.04, back - 1.62]))
        .arrayed(floors, every_floor);
    // Pilotis: a pier every eight metres, clear of the core and of the
    // lobby beside it.
    let lobby_x0 = core_x - 2.5 - 9.0;
    let piers = merge(
        (0..=((width - 4.0) / 8.0).floor() as u32)
            .map(|i| -width * 0.5 + 2.0 + f64::from(i) * 8.0)
            .filter(|x| x + 1.8 < lobby_x0 - 0.3)
            .map(|x| {
                Geometry::cuboid([1.8, lift, 1.8])
                    .placed(Pose::at([x, 0.0, back + 1.5]))
                    .arrayed(2, Pose::at([0.0, 0.0, depth - 4.8]))
            })
            .collect(),
    );
    let core_top = roof + 3.5;
    let core = slab([core_x, 0.0, 0.0], [5.0, core_top, depth + 1.2]);
    // The stair's glazed slot up the core's front, and the lobby's glass at
    // its foot under the block.
    let slot = slab([core_x, 1.0, front + 0.62], [1.2, core_top - 2.2, 0.06]);
    let lobby = slab(
        [lobby_x0 + 4.5 - 0.01, 0.0, 0.0],
        [9.0 - 0.02, lift, depth - 5.0],
    );
    let lobby_frame = Geometry::cuboid([0.1, lift, 0.12])
        .placed(Pose::at([lobby_x0 + 0.5, 0.0, front - 2.5 - 0.02]))
        .arrayed(7, Pose::at([1.4, 0.0, 0.0]));
    // Soffit lamps under the block along the lobby's front.
    let lamps = Geometry::cuboid([0.5, 0.06, 0.5])
        .placed(Pose::at([-width * 0.5 + 5.75, lift - 0.06, front - 1.5]))
        .arrayed(
            ((width - 12.0) / 8.0).floor().max(1.0) as u32,
            Pose::at([8.0, 0.0, 0.0]),
        );
    // The roof: a plant room with louvres, two tanks and masts.
    let plant_x = -width * 0.15;
    let plant = slab(
        [plant_x, roof, -depth * 0.1],
        [width * 0.3, 2.6, depth * 0.5],
    );
    let louvres = Geometry::cuboid([width * 0.3 - 1.0, 0.08, 0.12])
        .placed(Pose::at([
            plant_x - width * 0.15 + 0.5,
            roof + 0.5,
            -depth * 0.1 + depth * 0.25,
        ]))
        .arrayed(12, Pose::at([0.0, 0.14, 0.0]));
    let tanks = merge(
        [-width * 0.35, -width * 0.35 + 3.2]
            .into_iter()
            .map(|x| Geometry::cylinder(1.2, 2.4, 16).placed(Pose::at([x, roof, 0.0])))
            .collect(),
    );
    let masts = merge(
        [(width * 0.2, -depth * 0.25), (width * 0.3, depth * 0.2)]
            .into_iter()
            .map(|(x, z)| {
                Geometry::cylinder(0.06, 5.0, 6)
                    .placed(Pose::at([x, roof, z]))
                    .union(slab([x, roof + 3.8, z], [1.2, 0.05, 0.05]))
            })
            .collect(),
    );
    landmarks::building(
        "serein:slab",
        vec![
            Element::new("block", block, "charcoal")
                .cut_material("slit")
                .collision(Collision::Bounds),
            Element::new("grid", merge(vec![fins, bands, coping]), "grid"),
            Element::new("galleries", merge(vec![deck, balustrade, brackets]), "grid")
                .collision(Collision::Bounds),
            Element::new("piers", piers, "charcoal").collision(Collision::Bounds),
            Element::new("core", core, "concrete").collision(Collision::Bounds),
            Element::new("glass", merge(vec![slot, lobby]), "glass"),
            Element::new(
                "steel",
                merge(vec![mullions, rails, lobby_frame, louvres, masts]),
                "steel",
            ),
            Element::new("plant", plant, "steel"),
            Element::new("tanks", tanks, "white-steel"),
            Element::new("lamps", lamps, "lamp").far(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_house_validates() {
        let base = TerraceHouse {
            width: 10.0,
            depth: 10.0,
            lower: 2,
            style: Style::Shuttered,
            workshop: false,
            upper: Some(Upper {
                width: 6.0,
                depth: 6.0,
                shift: 0.5,
            }),
            shed: Some(true),
            awning: true,
            balcony: true,
            top: Top::Flat,
            roof: 0xff,
        };
        let houses = [
            base,
            TerraceHouse {
                upper: None,
                shed: None,
                roof: 0,
                style: Style::Casement,
                ..base
            },
            TerraceHouse {
                width: 6.0,
                depth: 6.0,
                lower: 1,
                upper: Some(Upper {
                    width: 4.0,
                    depth: 4.0,
                    shift: 0.0,
                }),
                top: Top::Vault,
                ..base
            },
            TerraceHouse {
                width: house::WORKSHOP,
                workshop: true,
                lower: 1,
                upper: None,
                top: Top::Lean,
                ..base
            },
        ];
        for house in houses {
            for piece in Kit::House(house).pieces() {
                piece
                    .part()
                    .unwrap_or_else(|e| panic!("{house:?}: {}: {e}", piece.key));
            }
        }
    }

    #[test]
    fn modules_stalls_and_trees_validate() {
        let kits = [
            Kit::Module(LandingModule {
                width: 12.0,
                depth: 10.0,
                storeys: 2,
                shop: true,
                awning: true,
                dome: true,
            }),
            Kit::Module(LandingModule {
                width: 8.0,
                depth: 8.0,
                storeys: 1,
                shop: false,
                awning: true,
                dome: false,
            }),
            Kit::Module(LandingModule {
                width: 15.0,
                depth: 12.0,
                storeys: 1,
                shop: true,
                awning: false,
                dome: false,
            }),
            Kit::Stall(Stall {
                width: 3.4,
                depth: 2.6,
                canopy: Canopy::Peak,
                goods: 3,
                lift: 0.3,
                sign: true,
            }),
            Kit::Stall(Stall {
                width: 2.6,
                depth: 2.4,
                canopy: Canopy::Slope,
                goods: 0,
                lift: 0.0,
                sign: false,
            }),
            Kit::Stall(Stall {
                width: 4.0,
                depth: 3.0,
                canopy: Canopy::Flat,
                goods: 2,
                lift: 0.5,
                sign: true,
            }),
            Kit::Crane(Crane {
                reach: 22.0,
                height: 24.0,
            }),
        ];
        for kit in kits {
            for piece in kit.pieces() {
                piece.part().unwrap_or_else(|e| panic!("{kit:?}: {e}"));
            }
        }
    }

    /// No kit lays two of its surfaces on one plane where they show, with
    /// every slot bound to its own material: the content step's check over
    /// the city, one kit at a time.
    #[test]
    #[ignore = "meshes every kind of kit; the content step checks the city"]
    fn no_kit_z_fights_itself() {
        use ashlar_manifold::{ManifoldMesher, mesh_building};
        let mut kits = vec![
            Kit::Crane(Crane {
                reach: 22.0,
                height: 24.0,
            }),
            Kit::Crane(Crane {
                reach: 18.0,
                height: 20.0,
            }),
        ];
        for (width, depth, storeys, shop, awning, dome) in [
            (12.0, 10.0, 2, true, true, true),
            (8.0, 8.0, 1, false, true, false),
            (15.0, 12.0, 3, true, false, false),
        ] {
            kits.push(Kit::Module(LandingModule {
                width,
                depth,
                storeys,
                shop,
                awning,
                dome,
            }));
        }
        for (length, high, slide) in [(6.1, 3, 0.4), (12.2, 2, -0.6)] {
            kits.push(Kit::Containers(Containers {
                length,
                high,
                slide,
            }));
        }
        let mut fights = Vec::new();
        for kit in kits {
            for piece in kit.pieces() {
                let part = piece.part().expect("validates");
                let mut builder = Building::builder("one").part(part.clone());
                for element in &part.elements {
                    for slot in std::iter::once(&element.material_slot)
                        .chain(element.cut_material_slot.as_ref())
                    {
                        builder = builder.material(slot.clone(), format!("own:{slot}"));
                    }
                }
                let building = builder
                    .instance(ashlar::Instance::new("one", piece.key.clone()))
                    .build()
                    .expect("validates");
                let meshed = mesh_building(&building, &ManifoldMesher::default()).expect("meshes");
                for o in meshed.coplanar_overlaps() {
                    if !o.covered && !o.same_binding() && o.normal.y > -0.9 {
                        fights.push(format!(
                            "{}: {} ({}) / {} ({}) {:.3} m² at {:.2?} facing {:.1?}",
                            piece.key,
                            o.first,
                            o.first_binding.material,
                            o.second,
                            o.second_binding.material,
                            o.area,
                            o.point.to_array(),
                            o.normal.to_array()
                        ));
                    }
                }
            }
        }
        assert!(fights.is_empty(), "{fights:#?}");
    }

    #[test]
    fn warehouses_and_slabs_validate() {
        warehouse(28.0, 11.0, 18.0).expect("a warehouse");
        warehouse(20.0, 9.0, 14.0).expect("a small warehouse");
        slab_block(52.0, 13.0, 8).expect("a slab");
        slab_block(45.0, 10.0, 5).expect("a low slab");
    }
}
