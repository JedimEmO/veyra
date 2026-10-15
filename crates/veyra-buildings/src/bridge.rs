//! Crossings (ticket 0005, step 7; decided 6): what a frontier colony puts
//! where a road meets a river. Local stone for the abutments and piers, cut
//! where the road was cut; a prefabricated deck brought out on a truck, its
//! kerbs and rails bolted on; a box culvert through a creek's fill between
//! two stone headwalls; banded posts at a ford and stones beside a trail's.
//!
//! Nothing here is fitted to one crossing. A crossing moves with the
//! world's generator, so the kit is baked in a few span classes and the
//! world stands the smallest class that covers a crossing's gap, centred,
//! its abutments bedded into the level shelves the road's grade leaves
//! behind each bank (`strata_worldgen::crossings`). What varies from one
//! crossing to the next (where the piers stand, how tall they are, how far
//! out a culvert's headwall is) is a piece of its own, placed by the world.
//!
//! The frame of every piece: `+z` along the road (a bridge) or out from it
//! (a headwall), `+x` to its right, `+y` up. A bridge's origin is the top of
//! its deck at the middle of its span, a pier's the middle of its cap's top,
//! a headwall's the floor of the culvert at its mouth, a post's and a
//! stone's where it meets the ground and the water's surface respectively.
//!
//! Collision is declared per element and derived from the evaluated solid,
//! as for every kit: the deck, the kerbs and the rails are what a walker
//! stands on and is stopped by, so each is its own convex proxy and none
//! fills the space under the deck.

use ashlar::{
    Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode, ValidationError,
};
use glam::{DQuat, DVec3};

/// The class of way a crossing carries: the road classes' widths (the
/// world's `RoadClass`), with a trail's footbridge narrower than its way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Way {
    /// A paved service road: two lanes.
    Service,
    /// A gravel track: one lane and its shoulders.
    Track,
    /// A foot trail.
    Foot,
}

impl Way {
    /// Every class, in order.
    pub const ALL: [Self; 3] = [Self::Service, Self::Track, Self::Foot];

    /// Its name in file names.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Track => "track",
            Self::Foot => "foot",
        }
    }

    /// Half the width between the kerbs, metres: the road's carriageway
    /// (`strata_worldgen::sites::way_in_half_m`), so a vehicle on the road
    /// is a vehicle on the deck; a footbridge's walk is narrower than its
    /// trail's way.
    #[must_use]
    pub const fn half_m(self) -> f64 {
        match self {
            Self::Service => 4.0,
            Self::Track => 3.0,
            Self::Foot => 0.8,
        }
    }

    /// How far over the water the road's grade holds a bridge's deck,
    /// metres (`strata_worldgen`'s grade, `Way::freeboard_m`): the room the
    /// structure under the deck has.
    #[must_use]
    pub const fn freeboard_m(self) -> f64 {
        match self {
            Self::Service => 2.5,
            Self::Track => 2.0,
            Self::Foot => 1.5,
        }
    }

    /// The clear spans the kit is baked in, metres: from the shortest
    /// creek to what the gaps measured within 200 km of Port Serein call
    /// for (ticket 0005, step 7's record).
    #[must_use]
    pub fn spans(self) -> Vec<f64> {
        let most = match self {
            Self::Service | Self::Track => 64,
            Self::Foot => 32,
        };
        (1..=most / 4).map(|k| f64::from(k * 4)).collect()
    }

    /// Half the deck's width, metres: the carriageway, the kerbs and the
    /// edge the rails are fixed to.
    #[must_use]
    pub fn deck_half_m(self) -> f64 {
        match self {
            Self::Service | Self::Track => self.half_m() + KERB_W_M + 0.1,
            Self::Foot => self.half_m() + 0.3,
        }
    }
}

/// How far a deck's top stands over the road's level, metres: a face laid
/// exactly on the ground flickers with it, and two centimetres is nothing
/// to a foot or a wheel.
pub const PROUD_M: f64 = 0.02;

/// Over its last this many metres a deck's top falls into the road, so its
/// end is buried in the shelf and the road meets it with no lip.
pub const RAMP_M: f64 = 1.2;

/// How far it falls there, metres: under the road's crown at the kerb.
pub const RAMP_DROP_M: f64 = 0.12;

/// How far each abutment's face may stand out into the gap the grade
/// leaves under a bridge, metres, at least and at most. The gap's edge
/// falls away as a steep bank; a face standing out from it shows as a stone
/// wall over the river's ground, and the classes are spaced by twice the
/// range so every gap between the shortest and the longest has one.
pub const INSET_M: [f64; 2] = [0.25, 2.25];

/// How far past the gap's edge the deck runs at least, metres, bedded in
/// the shelf. The grade's shelf is level across the carriageway only from
/// about two and a half metres back from the gap's edge: nearer it, the
/// abutment's face, which the grade rounds in plan, has already begun to
/// fall away at the kerbs, and a deck ending there stood over that hollow
/// at its corners (the step 7 review measured 0.56 m at a lane's middle
/// 0.9 m past the edge). From here the deck's end is on level shelf for
/// every inset, and short of a track's 4 m shelf by at most half a metre at
/// the least.
pub const PAST_EDGE_M: f64 = 2.5;

/// The longest clear span a vehicle bridge crosses without a pier, metres:
/// a precast deck with an arched soffit, the low arch a country bridge has.
pub const SINGLE_SPAN_M: f64 = 24.0;

/// The longest a girder runs between two supports, metres, as the piers
/// are first spaced; the world may shift them a little off a channel.
pub const GIRDER_SPAN_M: f64 = 22.0;

/// The longest a girder may run once the world has shifted the piers off a
/// channel, metres: a quarter more than the first spacing, which the
/// girders' depth carries. The layout says it to the world, and the one
/// number here keeps the content step and the tests from each choosing.
pub const GIRDER_MOST_M: f64 = GIRDER_SPAN_M * 1.25;

/// The tallest piers are baked in, metres: from the girders down to below
/// a river's bed. A gorge deeper than the last is not bridged yet.
pub const PIER_HEIGHTS_M: [f64; 4] = [6.0, 12.0, 24.0, 48.0];

/// How deep an abutment reaches under the deck's seat, metres: below the
/// steep bank the grade leaves at the gap's edge, wherever the class's
/// inset puts its face.
pub const ABUTMENT_DEPTH_M: f64 = 20.0;

/// How far an abutment reaches back under the deck, metres: to the deck's
/// end, so the deck bears on stone over its whole buried length.
const ABUTMENT_BACK_M: f64 = INSET_M[1] + PAST_EDGE_M;

/// A kerb's width and height, metres: steppable (under the character's
/// half-metre step) and a wheel's guide.
const KERB_W_M: f64 = 0.5;
const KERB_H_M: f64 = 0.22;

/// The rails' top over the deck, metres: over a walker's hip.
pub const RAIL_M: f64 = 1.15;

/// A footbridge truss's top over its deck, metres; the truss is its rail.
pub const TRUSS_M: f64 = 1.25;

/// A culvert's opening, metres: the widest creek a road culverts (2 m).
pub const BOX_W_M: f64 = 2.0;
/// The box heights a culvert is baked in, metres. A road stands anything
/// from half a metre to a few metres over a creek, and one box tall enough
/// for the deep fills put a headwall over the road on the shallow ones (the
/// step 7 review: two thirds of the culverts within 200 km), so the world
/// takes the tallest whose headwall stays under the road.
pub const BOX_HEIGHTS_M: [f64; 3] = [0.45, 0.65, 0.85];

/// How far the headwall's wall stands over its box, metres, under its
/// coping.
pub const HEADWALL_OVER_BOX_M: f64 = 0.25;
/// The coping's height, metres.
pub const COPING_M: f64 = 0.15;
/// Its thickness, metres: how far its face stands in front of where it is
/// placed.
pub const HEADWALL_THICK_M: f64 = 0.5;

/// A stepping stone's top over the water, metres.
pub const STONE_PROUD_M: f64 = 0.12;
/// And how far it reaches below that, metres: under the deepest ford's bed
/// (`FORD_DEPTH_M`, 0.65 m) wherever in the water it stands. It narrows to
/// its foot, so what shows through clear water is a boulder's rounded
/// underside and not a column.
pub const STONE_DEPTH_M: f64 = 1.1;
/// The stepping-stone variants: enough that a row of a dozen does not
/// show the same stone twice side by side often.
pub const STONES: u32 = 5;

/// A ford post's height over the ground, metres.
pub const POST_M: f64 = 1.6;

/// The material key each slot of the kit binds. The masonry is coursed
/// local stone, darker and more weathered than the city's terrace walls
/// (`bridge:masonry`, `veyra_materials::crossings`); the decks are the
/// pads' pale precast concrete; the rails the city's painted steel; the
/// girders its dark steel.
pub const MATERIALS: [(&str, &str); 8] = [
    ("stone", "bridge:masonry"),
    ("deck", "serein:pad-deck"),
    ("cast", "serein:grid-concrete"),
    ("steel", "serein:painted-steel"),
    ("girder", "serein:dark-steel"),
    ("hazard", "serein:hazard"),
    ("recess", "serein:recess"),
    ("boards", "serein:boards"),
];

/// The stepping stones' rock: the granite of the rock kits, darkened as a
/// stone in a stream is (`bridge:ford-stone`).
pub const ROCK: (&str, &str) = ("rock", "bridge:ford-stone");

fn box_at(size: [f64; 3], at: [f64; 3]) -> Geometry {
    Geometry::cuboid(size).placed(Pose::at(at))
}

fn element(name: &str, geometry: Geometry, slot: &str) -> Element {
    Element::new(name, geometry, slot).uv(UvMode::Box)
}

fn rod(from: [f64; 3], to: [f64; 3], radius: f64) -> Geometry {
    let along = DVec3::from_array(to) - DVec3::from_array(from);
    Geometry::cylinder(radius, along.length(), 6)
        .placed(Pose::at(from).rotated(DQuat::from_rotation_arc(DVec3::Y, along.normalize())))
}

/// The prism between two cross-sections of a deck: at `z0` from `bottom0`
/// to `top0`, at `z1` from `bottom1` to `top1`, `half` either side.
fn slice(
    half: f64,
    [z0, z1]: [f64; 2],
    [bottom0, top0]: [f64; 2],
    [bottom1, top1]: [f64; 2],
) -> Geometry {
    Geometry::hull([
        [-half, bottom0, z0],
        [half, bottom0, z0],
        [-half, top0, z0],
        [half, top0, z0],
        [-half, bottom1, z1],
        [half, bottom1, z1],
        [-half, top1, z1],
        [half, top1, z1],
    ])
}

/// One piece of the crossings' kit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// A bridge of a class: its deck, its rails and both abutments.
    Bridge(Bridge),
    /// A pier, set under a bridge's girders by the world.
    Pier(Pier),
    /// A culvert's mouth in its headwall, one at each side of the road, by
    /// its box's height.
    Headwall(Headwall),
    /// A banded depth post at a ford's bank.
    FordPost,
    /// A stepping stone, of a few shapes.
    Stone(u32),
}

impl Piece {
    /// The baked file's name under the kit's directory.
    #[must_use]
    pub fn file_name(self) -> String {
        match self {
            Self::Bridge(b) => format!("bridge-{}-{:02.0}.ashlar", b.way.name(), b.clear_m),
            Self::Pier(p) => format!("pier-{}-{:02.0}.ashlar", p.way.name(), p.height_m),
            Self::Headwall(h) => format!("headwall-{:03.0}.ashlar", h.box_m * 100.0),
            Self::FordPost => "ford-post.ashlar".to_owned(),
            Self::Stone(k) => format!("stone-{k}.ashlar"),
        }
    }

    /// The piece's recipe, its slots bound to [`MATERIALS`].
    ///
    /// # Errors
    /// A solid that does not validate: a bug here.
    pub fn building(self) -> Result<Building, ValidationError> {
        let (id, elements) = match self {
            Self::Bridge(b) => (
                format!("crossing:bridge-{}-{}", b.way.name(), b.clear_m),
                b.elements(),
            ),
            Self::Pier(p) => (
                format!("crossing:pier-{}-{}", p.way.name(), p.height_m),
                p.elements(),
            ),
            Self::Headwall(h) => (
                format!("crossing:headwall-{:.0}", h.box_m * 100.0),
                h.elements(),
            ),
            Self::FordPost => ("crossing:ford-post".to_owned(), ford_post()),
            Self::Stone(k) => (format!("crossing:stone-{k}"), stone(k)),
        };
        let mut part = Part::builder(id.clone());
        let mut slots = std::collections::BTreeSet::new();
        for element in elements {
            slots.insert(element.material_slot.clone());
            part = part.element(element);
        }
        let mut builder = Building::builder(id.clone())
            .part(part.build()?)
            .instance(Instance::new("piece", id));
        for (slot, key) in MATERIALS.into_iter().chain([ROCK]) {
            if slots.contains(slot) {
                builder = builder.material(slot, key);
            }
        }
        builder.build()
    }
}

/// Every piece of the kit, in the order it is baked.
#[must_use]
pub fn catalogue() -> Vec<Piece> {
    let mut pieces = Vec::new();
    for way in Way::ALL {
        for clear_m in way.spans() {
            pieces.push(Piece::Bridge(Bridge { way, clear_m }));
        }
    }
    for way in [Way::Service, Way::Track] {
        for height_m in PIER_HEIGHTS_M {
            pieces.push(Piece::Pier(Pier { way, height_m }));
        }
    }
    pieces.extend(
        BOX_HEIGHTS_M
            .into_iter()
            .map(|box_m| Piece::Headwall(Headwall { box_m })),
    );
    pieces.push(Piece::FordPost);
    pieces.extend((0..STONES).map(Piece::Stone));
    pieces
}

/// A bridge of one class: the way it carries and its clear span between
/// the abutments' faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bridge {
    /// The way it carries.
    pub way: Way,
    /// Metres between its abutments' faces.
    pub clear_m: f64,
}

impl Bridge {
    /// The deck's length, metres: the span, each face's most inset into
    /// the gap and the length past the gap's edge it is bedded by.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.clear_m + 2.0 * (INSET_M[1] + PAST_EDGE_M)
    }

    /// The gaps it serves, metres: a gap of these puts each face between
    /// [`INSET_M`]'s two insets into it. The shortest class serves gaps a
    /// little shorter than itself too, its faces set back into the shelf.
    #[must_use]
    pub fn gaps_m(&self) -> [f64; 2] {
        let lo = if self.clear_m <= 4.0 {
            self.clear_m - 1.0
        } else {
            self.clear_m + 2.0 * INSET_M[0]
        };
        [lo, self.clear_m + 2.0 * INSET_M[1]]
    }

    /// How many piers stand under it: none up to [`SINGLE_SPAN_M`] (none
    /// under a footbridge), then one more for each [`GIRDER_SPAN_M`].
    #[must_use]
    pub fn piers(&self) -> u32 {
        if self.way == Way::Foot || self.clear_m <= SINGLE_SPAN_M {
            0
        } else {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a span over a girder's is a handful"
            )]
            let spans = (self.clear_m / GIRDER_SPAN_M).ceil() as u32;
            spans.max(2) - 1
        }
    }

    /// Whether the deck rides on girders (it has piers).
    #[must_use]
    pub fn girders(&self) -> bool {
        self.piers() > 0
    }

    /// The deck slab's thickness at the middle, metres.
    fn crown_m(&self) -> f64 {
        match self.way {
            Way::Service => 0.5,
            Way::Track => 0.45,
            Way::Foot => 0.12,
        }
    }

    /// The rise of an arched soffit, metres: a low arch, deeper as the span
    /// grows, within the room the freeboard leaves.
    fn rise_m(&self) -> f64 {
        if self.girders() || self.way == Way::Foot {
            0.0
        } else {
            let most = match self.way {
                Way::Service => 1.1,
                _ => 0.7,
            };
            (self.clear_m * 0.055).clamp(0.2, most)
        }
    }

    /// The girders' depth under the slab, metres.
    fn girder_m(&self) -> f64 {
        match self.way {
            Way::Service => 0.95,
            Way::Track => 0.8,
            Way::Foot => 0.0,
        }
    }

    /// The slab's thickness on girders, metres.
    const SLAB_M: f64 = 0.32;

    /// The footbridge's cross beams under its boards, metres.
    const FOOT_BEAM_M: f64 = 0.15;

    /// How far under the deck's top the structure reaches where it is
    /// lowest over the span, metres: the arch at its springing, the girders'
    /// bottom flanges, a footbridge's cross beams. A pier's top is set here.
    #[must_use]
    pub fn underside_m(&self) -> f64 {
        if self.girders() {
            Self::SLAB_M + self.girder_m()
        } else if self.way == Way::Foot {
            self.crown_m() + Self::FOOT_BEAM_M
        } else {
            self.crown_m() + self.rise_m()
        }
    }

    /// The soffit's depth under the deck's top `z` metres from the middle,
    /// metres: what clears the water there.
    #[must_use]
    pub fn soffit_m(&self, z: f64) -> f64 {
        if self.girders() || self.way == Way::Foot {
            self.underside_m()
        } else {
            let t = (2.0 * z / self.clear_m).clamp(-1.0, 1.0);
            self.crown_m() + self.rise_m() * t * t
        }
    }

    /// The deck top's height `z` metres from the middle, metres: level,
    /// then falling into the road over its last [`RAMP_M`].
    #[must_use]
    pub fn top_m(&self, z: f64) -> f64 {
        let end = 0.5 * self.length_m();
        let into = (z.abs() - (end - RAMP_M)).max(0.0);
        PROUD_M - RAMP_DROP_M * into / RAMP_M
    }

    fn elements(&self) -> Vec<Element> {
        let mut out = self.deck();
        out.extend(self.edges());
        out.extend(self.abutments());
        out
    }

    /// The deck in slices, each its own convex proxy, so the space under
    /// an arched soffit or between girders stays open to a walker.
    fn deck(&self) -> Vec<Element> {
        let half = self.way.deck_half_m();
        let end = 0.5 * self.length_m();
        let span = 0.5 * self.clear_m;
        let slot = if self.way == Way::Foot {
            "boards"
        } else {
            "deck"
        };
        let bottom = |z: f64| {
            if self.girders() {
                -Self::SLAB_M
            } else if self.way == Way::Foot {
                -self.crown_m()
            } else {
                -self.soffit_m(z.clamp(-span, span))
            }
        };
        let mut cuts = vec![-end, -(end - RAMP_M)];
        if !self.girders() && self.way != Way::Foot {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a span of tens of metres"
            )]
            let pieces = ((self.clear_m / 2.0).round() as u32).clamp(4, 12);
            cuts.push(-span);
            for k in 1..pieces {
                cuts.push(-span + self.clear_m * f64::from(k) / f64::from(pieces));
            }
            cuts.push(span);
        }
        cuts.extend([end - RAMP_M, end]);
        let mut elements = Vec::new();
        for (k, pair) in cuts.windows(2).enumerate() {
            let (z0, z1) = (pair[0], pair[1]);
            let geometry = slice(
                half,
                [z0, z1],
                [bottom(z0), self.top_m(z0)],
                [bottom(z1), self.top_m(z1)],
            );
            let mut e = element(&format!("deck-{k}"), geometry, slot).collision(Collision::Hull);
            if self.way != Way::Foot {
                e = e.far();
            }
            elements.push(e);
        }
        if self.girders() {
            elements.extend(self.girder_elements());
        } else if self.way == Way::Foot {
            // Cross beams under the boards, every metre and a half.
            let beams = (1..)
                .map(|k| -end + 0.4 + 1.5 * f64::from(k - 1))
                .take_while(|z| *z < end - 0.4)
                .map(|z| {
                    box_at(
                        [2.0 * (half + 0.15), Self::FOOT_BEAM_M, 0.14],
                        [
                            -(half + 0.15),
                            -self.crown_m() - Self::FOOT_BEAM_M,
                            z - 0.07,
                        ],
                    )
                });
            elements.push(element("cross-beams", Geometry::union_all(beams), "girder"));
        } else {
            // The arch's ring: a band of lighter cast concrete on each face,
            // following the soffit, so the arch reads from the bank.
            for side in [-1.0, 1.0] {
                let mut ring = Vec::new();
                for pair in cuts.windows(2) {
                    let (z0, z1) = (pair[0].max(-span), pair[1].min(span));
                    if z1 <= z0 {
                        continue;
                    }
                    let x = side * half;
                    let (b0, b1) = (bottom(z0), bottom(z1));
                    ring.push(Geometry::hull([
                        [x - 0.06, b0 - 0.02, z0],
                        [x + 0.06, b0 - 0.02, z0],
                        [x - 0.06, b0 + 0.4, z0],
                        [x + 0.06, b0 + 0.4, z0],
                        [x - 0.06, b1 - 0.02, z1],
                        [x + 0.06, b1 - 0.02, z1],
                        [x - 0.06, b1 + 0.4, z1],
                        [x + 0.06, b1 + 0.4, z1],
                    ]));
                }
                elements.push(
                    element(
                        if side < 0.0 {
                            "arch-ring-left"
                        } else {
                            "arch-ring-right"
                        },
                        Geometry::union_all(ring),
                        "cast",
                    )
                    .far(),
                );
            }
        }
        elements
    }

    /// The girders' positions across the deck, metres.
    fn girder_xs(way: Way) -> &'static [f64] {
        match way {
            Way::Service => &[-3.2, 0.0, 3.2],
            Way::Track => &[-2.2, 2.2],
            Way::Foot => &[],
        }
    }

    /// Steel plate girders under the slab, bearing a metre onto each
    /// abutment, braced every six metres, and a cast edge beam along each
    /// side so the deck shows its depth from the bank.
    fn girder_elements(&self) -> Vec<Element> {
        let depth = self.girder_m();
        let reach = 0.5 * self.clear_m + 1.0;
        let top = -Self::SLAB_M;
        let girders = Self::girder_xs(self.way).iter().map(|&x| {
            Geometry::union_all([
                box_at([0.45, 0.05, 2.0 * reach], [x - 0.225, top - 0.05, -reach]),
                box_at(
                    [0.03, depth - 0.1, 2.0 * reach],
                    [x - 0.015, top - depth + 0.05, -reach],
                ),
                box_at([0.5, 0.05, 2.0 * reach], [x - 0.25, top - depth, -reach]),
            ])
        });
        let xs = Self::girder_xs(self.way);
        let (x0, x1) = (xs[0], xs[xs.len() - 1]);
        let braces = (0..)
            .map(|k| -reach + 0.5 + 6.0 * f64::from(k))
            .take_while(|z| *z <= reach - 0.5)
            .map(|z| {
                box_at(
                    [x1 - x0, depth * 0.6, 0.12],
                    [x0, top - depth * 0.8, z - 0.06],
                )
            });
        let half = self.way.deck_half_m();
        let end = 0.5 * self.length_m();
        let edges = [-1.0, 1.0].map(|side: f64| {
            let x = if side < 0.0 { -half } else { half - 0.22 };
            box_at([0.22, 0.42, 2.0 * end - 0.1], [x, top - 0.42, -end + 0.05])
        });
        vec![
            element("girders", Geometry::union_all(girders), "girder")
                .collision(Collision::Hull)
                .far(),
            element("braces", Geometry::union_all(braces), "girder"),
            element("edge-beams", Geometry::union_all(edges), "cast").far(),
        ]
    }

    /// The kerbs and the rails (a road bridge's), or the trusses (a
    /// footbridge's, which are its rails), each side one convex proxy from
    /// the deck to the rail's top, so nothing slips between them.
    fn edges(&self) -> Vec<Element> {
        let end = 0.5 * self.length_m();
        let level = end - RAMP_M;
        let mut out = Vec::new();
        for (side, name) in [(-1.0_f64, "left"), (1.0, "right")] {
            if self.way == Way::Foot {
                out.push(self.truss(side, name));
                continue;
            }
            let x0 = side * self.way.half_m();
            let x1 = side * (self.way.half_m() + KERB_W_M);
            let (lo, hi) = (x0.min(x1), x0.max(x1));
            // Bedded into the deck; falling with its ramp at each end.
            let kerb = Geometry::hull([
                [lo, -0.15, -end],
                [hi, -0.15, -end],
                [lo, self.top_m(end) + 0.03, -end],
                [hi, self.top_m(end) + 0.03, -end],
                [lo, PROUD_M + KERB_H_M, -level],
                [hi, PROUD_M + KERB_H_M, -level],
                [lo, PROUD_M + KERB_H_M, level],
                [hi, PROUD_M + KERB_H_M, level],
                [lo, -0.15, end],
                [hi, -0.15, end],
                [lo, self.top_m(end) + 0.03, end],
                [hi, self.top_m(end) + 0.03, end],
            ]);
            out.push(element(&format!("kerb-{name}"), kerb, "cast").collision(Collision::Hull));
            let x = side * (self.way.half_m() + 0.32);
            let reach = end - 0.2;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a deck of tens of metres"
            )]
            let bays = ((2.0 * reach / 2.0).ceil() as u32).max(1);
            let mut steel = Vec::new();
            for k in 0..=bays {
                let z = -reach + 2.0 * reach * f64::from(k) / f64::from(bays);
                // From inside the kerb, so a post on its falling end is
                // still bedded.
                steel.push(box_at(
                    [0.1, RAIL_M + 0.15, 0.1],
                    [x - 0.05, -0.15, z - 0.05],
                ));
            }
            steel.push(box_at(
                [0.14, 0.1, 2.0 * reach + 0.1],
                [x - 0.07, RAIL_M - 0.1, -reach - 0.05],
            ));
            steel.push(box_at([0.08, 0.08, 2.0 * reach], [x - 0.04, 0.62, -reach]));
            out.push(
                element(&format!("rail-{name}"), Geometry::union_all(steel), "steel")
                    .collision(Collision::Hull),
            );
            // A hazard-painted cap on each end post, for a driver at dusk.
            let caps = [-1.0, 1.0].map(|s: f64| {
                box_at(
                    [0.16, 0.3, 0.16],
                    [x - 0.08, RAIL_M - 0.35, s * reach - 0.08],
                )
            });
            out.push(element(
                &format!("rail-ends-{name}"),
                Geometry::union_all(caps),
                "hazard",
            ));
        }
        out
    }

    /// A Warren pony truss along one side of a footbridge: its chords, its
    /// verticals at the ends and its diagonals between, standing on the
    /// cross beams' ends.
    fn truss(&self, side: f64, name: &str) -> Element {
        let end = 0.5 * self.length_m() - 0.15;
        let x = side * (self.way.half_m() + 0.12);
        let bottom = -self.crown_m();
        let top = TRUSS_M;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a span of tens of metres"
        )]
        let panels = ((2.0 * end / 1.6).round() as u32).max(2);
        let step = 2.0 * end / f64::from(panels);
        let mut members = vec![
            box_at([0.12, 0.12, 2.0 * end], [x - 0.06, bottom, -end]),
            box_at([0.14, 0.1, 2.0 * end], [x - 0.07, top - 0.1, -end]),
            box_at([0.1, top - bottom, 0.1], [x - 0.05, bottom, -end]),
            box_at([0.1, top - bottom, 0.1], [x - 0.05, bottom, end - 0.1]),
        ];
        for k in 0..panels {
            let z0 = -end + step * f64::from(k);
            let (a, b) = if k % 2 == 0 {
                (bottom + 0.06, top - 0.05)
            } else {
                (top - 0.05, bottom + 0.06)
            };
            members.push(rod([x, a, z0], [x, b, z0 + step], 0.035));
        }
        // A mid rail a hand can reach: the truss's diagonals are wide apart.
        members.push(box_at([0.06, 0.06, 2.0 * end], [x - 0.03, 0.55, -end]));
        element(
            &format!("truss-{name}"),
            Geometry::union_all(members),
            "steel",
        )
        .collision(Collision::Hull)
        .far()
    }

    /// A stone abutment under each end, its face at the clear span's edge,
    /// reaching back under the deck's buried end and down into the bank,
    /// with a cast seat along its top the deck bears on.
    fn abutments(&self) -> Vec<Element> {
        let foot = self.way == Way::Foot;
        // Wide enough to carry a pilaster outside each rail.
        let half = self.way.deck_half_m() + if foot { 0.35 } else { 0.85 };
        let span = 0.5 * self.clear_m;
        let seat = -self.underside_m();
        let depth = if self.way == Way::Foot {
            3.0
        } else {
            ABUTMENT_DEPTH_M
        };
        let mut out = Vec::new();
        for (sign, name) in [(-1.0_f64, "near"), (1.0, "far")] {
            // From the face back to the deck's end, `sign` the side.
            let z = if sign < 0.0 {
                -span - ABUTMENT_BACK_M
            } else {
                span
            };
            let body = box_at(
                [2.0 * half, depth + seat - 0.3, ABUTMENT_BACK_M],
                [-half, -depth, z],
            );
            out.push(
                element(&format!("abutment-{name}"), body, "stone")
                    .collision(Collision::Bounds)
                    .far(),
            );
            // The seat: a cast course over the whole top, a little proud of
            // the face, the deck bearing on it.
            let seat_z = if sign < 0.0 { z } else { span - 0.12 };
            out.push(
                element(
                    &format!("seat-{name}"),
                    box_at(
                        [2.0 * half + 0.1, 0.3, ABUTMENT_BACK_M + 0.12],
                        [-half - 0.05, seat - 0.3, seat_z],
                    ),
                    "cast",
                )
                .collision(Collision::Bounds),
            );
            if !foot {
                out.extend(self.stonework(sign, name, half, depth));
            }
            if self.girders() {
                // The backwall behind the girders' ends, up to the slab.
                let back_z = if sign < 0.0 {
                    -span - ABUTMENT_BACK_M
                } else {
                    span + 1.05
                };
                out.push(
                    element(
                        &format!("backwall-{name}"),
                        box_at(
                            [2.0 * half - 0.2, self.girder_m(), ABUTMENT_BACK_M - 1.05],
                            [-half + 0.1, seat, back_z],
                        ),
                        "stone",
                    )
                    .collision(Collision::Bounds),
                );
            }
        }
        out
    }
}

impl Bridge {
    /// The stone a road bridge shows at each end: a pilaster outside each
    /// rail over the abutment's face, where the rail ends against stone,
    /// and a wing wall flaring back from each corner into the road's bank,
    /// so the local stone reads from the bank and the road's fill is held.
    fn stonework(&self, sign: f64, name: &str, half: f64, depth: f64) -> Vec<Element> {
        let span = 0.5 * self.clear_m;
        let seat = -self.underside_m();
        let inner = self.way.deck_half_m() + 0.05;
        let mut out = Vec::new();
        for (side, which) in [(-1.0_f64, "left"), (1.0, "right")] {
            let (x0, x1) = if side < 0.0 {
                (-half, -inner)
            } else {
                (inner, half)
            };
            let (z0, z1) = if sign < 0.0 {
                (-span - 1.2, -span + 0.1)
            } else {
                (span - 0.1, span + 1.2)
            };
            out.push(
                element(
                    &format!("pilaster-{name}-{which}"),
                    Geometry::chamfered_cuboid([x1 - x0, RAIL_M + 0.25 - seat, z1 - z0], 0.04)
                        .placed(Pose::at([x0, seat, z0])),
                    "stone",
                )
                .collision(Collision::Bounds)
                .far(),
            );
            // The wing: from the face's outer corner back and out at a
            // third of a right angle, its top falling with the bank.
            let (s30, c30) = (0.5_f64, 0.866_f64);
            let root = [side * (half - 0.6), sign * (span + 0.6)];
            let along = [side * s30, sign * c30];
            let thick = [side * c30 * 0.6, -sign * s30 * 0.6];
            let length = 5.0;
            let mut points = Vec::new();
            for (t, top) in [(0.0, RAIL_M - 0.2), (length, -1.6)] {
                for w in [0.0, 1.0] {
                    let x = root[0] + along[0] * t + thick[0] * w;
                    let z = root[1] + along[1] * t + thick[1] * w;
                    points.push([x, top, z]);
                    points.push([x, -0.6 * depth, z]);
                }
            }
            out.push(
                element(
                    &format!("wing-{name}-{which}"),
                    Geometry::hull(points),
                    "stone",
                )
                .collision(Collision::Hull)
                .far(),
            );
        }
        out
    }
}

/// A pier under a bridge's girders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pier {
    /// The way of the bridge it carries.
    pub way: Way,
    /// From its cap's top to its foot, metres.
    pub height_m: f64,
}

impl Pier {
    /// Its shaft's width across the road, metres: under every girder with
    /// half a metre to spare.
    fn width_m(self) -> f64 {
        let xs = Bridge::girder_xs(self.way);
        2.0 * xs.iter().fold(0.0_f64, |m, x| m.max(x.abs())) + 1.0
    }

    /// Its shaft's thickness along the road, metres: thicker as it is
    /// taller, so a 24 m pier under a girder deck reads as masonry that
    /// carries it and not a post (the step 7 review: "pencil-thin").
    fn thick_m(self) -> f64 {
        let base = match self.way {
            Way::Service => 1.4,
            _ => 1.2,
        };
        base + self.height_m / 30.0
    }

    /// How far the shaft spreads across the road at its foot past its top,
    /// metres a side per metre down: a batter, which only the cutwaters'
    /// faces show, so it keeps clear of the channel along the road.
    const BATTER: f64 = 1.0 / 24.0;

    /// The cap's height, metres.
    const CAP_M: f64 = 0.6;

    /// Half its thickness along the road, cap and all, metres.
    #[must_use]
    pub fn half_along_m(self) -> f64 {
        0.5 * self.thick_m() + 0.2
    }

    /// A cast cap the girders bear on and a masonry shaft with pointed
    /// cutwaters up- and downstream (across the road, `x`), battered out
    /// across the road to its foot.
    fn elements(self) -> Vec<Element> {
        let (top_w, t) = (0.5 * self.width_m(), 0.5 * self.thick_m());
        let mut points = Vec::new();
        for y in [-Self::CAP_M, -self.height_m] {
            let w = top_w + Self::BATTER * (-Self::CAP_M - y);
            for [x, z] in [
                [-w, -t],
                [w, -t],
                [w, t],
                [-w, t],
                [-w - t, 0.0],
                [w + t, 0.0],
            ] {
                points.push([x, y, z]);
            }
        }
        let w = top_w;
        vec![
            element(
                "cap",
                Geometry::chamfered_cuboid([2.0 * w + 0.4, Self::CAP_M, 2.0 * t + 0.4], 0.04)
                    .placed(Pose::at([-w - 0.2, -Self::CAP_M, -t - 0.2])),
                "cast",
            )
            .collision(Collision::Bounds)
            .far(),
            element("shaft", Geometry::hull(points), "stone")
                .collision(Collision::Hull)
                .far(),
        ]
    }
}

/// A culvert's mouth of one box height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Headwall {
    /// The box's opening's height, metres.
    pub box_m: f64,
}

impl Headwall {
    /// Its coping's top over the box's floor, metres: how far over the
    /// ground at the creek it stands.
    #[must_use]
    pub fn top_m(self) -> f64 {
        self.box_m + HEADWALL_OVER_BOX_M + COPING_M
    }

    /// A stone headwall with the box's opening, a cast frame proud of it,
    /// the box's dark inside, an apron at the floor and a wing wall flaring
    /// out at each side. Its face is `+z`, at `z = 0`.
    fn elements(self) -> Vec<Element> {
        let (w, h) = (0.5 * BOX_W_M, self.box_m);
        let wall_top = h + HEADWALL_OVER_BOX_M;
        let half = w + 1.3;
        let wall = box_at(
            [2.0 * half, wall_top + 0.45, HEADWALL_THICK_M],
            [-half, -0.45, -HEADWALL_THICK_M],
        )
        .subtract(box_at([2.0 * w, h, 2.0], [-w, 0.0, -1.5]));
        let frame = box_at([2.0 * w + 0.3, h + 0.15, 0.3], [-w - 0.15, -0.05, -0.1])
            .subtract(box_at([2.0 * w, h, 1.0], [-w, 0.0, -0.5]));
        // A wall 0.4 m thick and 1.6 m long from the headwall's end, turned
        // out from the opening by about a third of a right angle, its top
        // falling with the fill's toe.
        let wing = |side: f64| {
            let turn = DQuat::from_rotation_y(side * 0.6);
            let root = DVec3::new(side * (half - 0.2), 0.0, -0.05);
            let mut points = Vec::new();
            for (along, top) in [(0.0, wall_top - 0.1), (1.6, 0.25)] {
                for across in [0.0, 0.4] {
                    for y in [-0.45, top] {
                        let p = root + turn * DVec3::new(side * across, y, along);
                        points.push(p.to_array());
                    }
                }
            }
            Geometry::hull(points)
        };
        vec![
            element("headwall", wall, "stone").collision(Collision::Bounds),
            element(
                "coping",
                box_at(
                    [2.0 * half + 0.1, COPING_M, HEADWALL_THICK_M + 0.12],
                    [-half - 0.05, wall_top, -HEADWALL_THICK_M - 0.02],
                ),
                "cast",
            ),
            element("frame", frame, "cast"),
            element(
                "inside",
                Geometry::union_all([
                    box_at([2.0 * w, 0.05, 1.6], [-w, -0.05, -1.6]),
                    box_at([2.0 * w, h, 0.05], [-w, 0.0, -1.6]),
                ]),
                "recess",
            ),
            element(
                "apron",
                box_at([2.0 * w + 0.8, 0.15, 1.1], [-w - 0.4, -0.15, 0.0]),
                "cast",
            ),
            element("wing-left", wing(-1.0), "stone").collision(Collision::Hull),
            element("wing-right", wing(1.0), "stone").collision(Collision::Hull),
        ]
    }
}

/// A steel depth post on a cast foot, banded in hazard paint every 30 cm
/// so the water's depth reads off it, a reflective cap on top.
fn ford_post() -> Vec<Element> {
    vec![
        element(
            "post",
            Geometry::chamfered_cuboid([0.12, POST_M + 0.6, 0.12], 0.02)
                .placed(Pose::at([-0.06, -0.6, -0.06])),
            "steel",
        )
        .collision(Collision::Bounds)
        .far(),
        // Flush with the ground it is set in: a foot standing proud of a
        // bank at the waterline read as a pale disc on the water.
        element(
            "foot",
            Geometry::cylinder(0.2, 0.47, 8).placed(Pose::at([0.0, -0.45, 0.0])),
            "cast",
        ),
        element(
            "bands",
            Geometry::union_all([0.3, 0.9].map(|y| box_at([0.14, 0.3, 0.14], [-0.07, y, -0.07]))),
            "hazard",
        ),
        element(
            "cap",
            box_at([0.16, 0.12, 0.16], [-0.08, POST_M, -0.08]),
            "hazard",
        )
        .far(),
    ]
}

/// A stone set in a ford, its top [`STONE_PROUD_M`] over the water and
/// its foot [`STONE_DEPTH_M`] under that: a low dome to stand on, its
/// widest a hand under the waterline, narrowing to its foot, so what the
/// water shows is a boulder and not a column. The variants differ in plan
/// and in how they lean, so a row of them does not repeat.
fn stone(variant: u32) -> Vec<Element> {
    let shape = variant % STONES;
    let (half_x, half_z) = [
        (0.3, 0.24),
        (0.26, 0.29),
        (0.34, 0.22),
        (0.28, 0.25),
        (0.31, 0.27),
    ][shape as usize];
    let skew = 0.37 * f64::from(shape);
    let tilt = 0.03 * (f64::from(shape) - 2.0);
    let mut points = Vec::new();
    let rings = [
        (0.0, 0.55),
        (-0.04, 0.85),
        (-0.12, 1.0),
        (-0.4, 0.95),
        (-STONE_DEPTH_M, 0.5),
    ];
    for (ring, (depth, grow)) in rings.into_iter().enumerate() {
        for step in 0..10 {
            let angle = std::f64::consts::TAU * f64::from(step) / 10.0 + skew;
            let wobble = 1.0 + 0.1 * (f64::from(step) * 2.3 + skew * 7.0).sin();
            let across = half_x * grow * wobble * angle.cos();
            let along = half_z * grow * wobble * angle.sin();
            // The top leans a little, so the row's tops do not line up.
            let height = if ring == 0 {
                (tilt * across / half_x).min(0.0)
            } else {
                depth
            };
            points.push([across, height, along]);
        }
    }
    vec![
        element("stone", Geometry::hull(points), "rock")
            .collision(Collision::Hull)
            .far(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every piece validates, its name is unique, and every slot it binds
    /// is one of the kit's.
    #[test]
    fn every_piece_builds_with_a_name_of_its_own() {
        let mut names = std::collections::BTreeSet::new();
        for piece in catalogue() {
            let building = piece
                .building()
                .unwrap_or_else(|e| panic!("{piece:?}: {e}"));
            assert!(names.insert(piece.file_name()), "{piece:?}");
            for (_, slot, binding) in building.bindings() {
                assert!(
                    MATERIALS
                        .iter()
                        .chain([&ROCK])
                        .any(|(s, k)| *s == slot && *k == binding.material),
                    "{piece:?} binds {slot}"
                );
            }
        }
    }

    /// Each class's gaps start where the last's end, so every gap from the
    /// shortest to the longest has a class, and each class's faces stand
    /// within the insets for the gaps it serves.
    #[test]
    fn the_classes_tile_the_gaps() {
        for way in Way::ALL {
            let classes: Vec<Bridge> = way
                .spans()
                .into_iter()
                .map(|clear_m| Bridge { way, clear_m })
                .collect();
            for pair in classes.windows(2) {
                assert!(
                    (pair[0].gaps_m()[1] - pair[1].gaps_m()[0]).abs() < 1e-9,
                    "{way:?}: {:?} then {:?}",
                    pair[0].gaps_m(),
                    pair[1].gaps_m()
                );
            }
            for b in &classes {
                let [lo, hi] = b.gaps_m();
                assert!(lo < hi);
                assert!(((hi - b.clear_m) * 0.5 - INSET_M[1]).abs() < 1e-9);
            }
        }
    }

    /// The world's road search closes a crossing the kit cannot stand, by
    /// the limits `strata_worldgen::crossings` states: those must be the
    /// catalogue's own for a service road and a track.
    #[test]
    fn the_world_knows_the_kit_limits() {
        use strata_worldgen::crossings::{BRIDGE_GAP_MOST_M, PIER_MOST_M, PIERLESS_GAP_MOST_M};
        for way in [Way::Service, Way::Track] {
            let classes: Vec<Bridge> = way
                .spans()
                .into_iter()
                .map(|clear_m| Bridge { way, clear_m })
                .collect();
            let longest = classes.iter().map(|b| b.gaps_m()[1]).fold(0.0, f64::max);
            assert!(
                (longest - BRIDGE_GAP_MOST_M).abs() < 1e-9,
                "{way:?}: {longest}"
            );
            let pierless = classes
                .iter()
                .filter(|b| b.piers() == 0)
                .map(|b| b.gaps_m()[1])
                .fold(0.0, f64::max);
            assert!(
                (pierless - PIERLESS_GAP_MOST_M).abs() < 1e-9,
                "{way:?}: {pierless}"
            );
        }
        let tallest = PIER_HEIGHTS_M.iter().copied().fold(0.0, f64::max);
        assert!((tallest - PIER_MOST_M).abs() < 1e-9);
    }

    /// The structure under a deck leaves the water room under it: the
    /// deck's top stands its freeboard over the water, and the soffit is at
    /// least 0.8 m over that wherever the span is, at least 0.4 m under a
    /// footbridge.
    #[test]
    fn the_deck_clears_the_water_by_the_freeboard() {
        for piece in catalogue() {
            let Piece::Bridge(b) = piece else { continue };
            let least = if b.way == Way::Foot { 0.4 } else { 0.8 };
            for k in 0..=20 {
                let z = -0.5 * b.clear_m + b.clear_m * f64::from(k) / 20.0;
                let room = b.way.freeboard_m() - b.soffit_m(z);
                assert!(room >= least, "{b:?}: {room:.2} m at {z:.1}");
            }
        }
    }
}
