//! Roadside dressing (ticket 0005, step 8): what a frontier colony stands
//! along the roads it graded. A white marker post on the verge so a driver
//! sees where the road goes at dusk and in the valley fog, and a lamp
//! standard where a road arrives somewhere: at a site's pad and at each end
//! of a bridge, past the stone pilasters where the rails end. The ford
//! posts of the crossings' kit (`crate::bridge`) are the first markers; these
//! are the rest.
//!
//! As with the crossings, nothing here is fitted to one road: the world
//! stands the pieces along its network (`strata_worldgen::roadside`), and
//! the content step bakes each once.
//!
//! The frame of both pieces: the origin where the piece meets the ground,
//! `+y` up, and `+z` the way it faces. A marker's reflector faces `+z`, the
//! traffic it is seen by; a lamp's arm reaches out along `+z`, over the
//! road.

use ashlar::{
    Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode, ValidationError,
};

/// A marker post's height over the ground, metres: a driver's eye, and
/// over the verge's grass.
pub const MARKER_M: f64 = 1.15;

/// A lamp standard's pole height, metres, to the arm.
pub const LAMP_M: f64 = 6.2;

/// How far the lamp's arm reaches out over the road from the pole, metres.
pub const LAMP_REACH_M: f64 = 1.6;

/// The lantern's size under the arm's end, metres, and its corner nearest
/// the pole, low and to the `-x` side.
const LANTERN_SIZE: [f64; 3] = [0.24, 0.05, 0.46];
const LANTERN_CORNER: [f64; 3] = [-0.12, LAMP_M - 0.22, LAMP_REACH_M - 0.45];

/// Where the lamp's light comes from, in the piece's frame: the middle of
/// the lantern's underside. The game hangs its point light here, so the
/// light stays under the lantern whatever the piece's shape becomes.
pub const LANTERN_AT: [f64; 3] = [
    LANTERN_CORNER[0] + 0.5 * LANTERN_SIZE[0],
    LANTERN_CORNER[1],
    LANTERN_CORNER[2] + 0.5 * LANTERN_SIZE[2],
];

/// The material key each slot binds: the city's white steel and hazard
/// paint, its grid concrete for the feet, its dark steel for the lamp's
/// pole and the warm lantern its streets are lit by (whose emission the
/// game dims by day, as it dims every building's).
pub const MATERIALS: [(&str, &str); 5] = [
    ("white", "serein:white-steel"),
    ("dark", "serein:dark-steel"),
    ("hazard", "serein:hazard"),
    ("cast", "serein:grid-concrete"),
    ("lantern", "serein:lantern"),
];

/// One piece of the roadside's kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// A marker post on a road's verge.
    Marker,
    /// A lamp standard at a road's arrival.
    Lamp,
}

impl Piece {
    /// Every piece, in the order it is baked.
    pub const ALL: [Self; 2] = [Self::Marker, Self::Lamp];

    /// The baked file's name under the kit's directory.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Marker => "road-marker.ashlar",
            Self::Lamp => "road-lamp.ashlar",
        }
    }

    /// The piece's recipe, its slots bound to [`MATERIALS`].
    ///
    /// # Errors
    /// A solid that does not validate: a bug here.
    pub fn building(self) -> Result<Building, ValidationError> {
        let (id, elements) = match self {
            Self::Marker => ("roadside:marker", marker()),
            Self::Lamp => ("roadside:lamp", lamp()),
        };
        let mut part = Part::builder(id);
        let mut slots = std::collections::BTreeSet::new();
        for element in elements {
            slots.insert(element.material_slot.clone());
            part = part.element(element);
        }
        let mut builder = Building::builder(id)
            .part(part.build()?)
            .instance(Instance::new("piece", id));
        for (slot, key) in MATERIALS {
            if slots.contains(slot) {
                builder = builder.material(slot, key);
            }
        }
        builder.build()
    }
}

fn element(name: &str, geometry: Geometry, slot: &str) -> Element {
    Element::new(name, geometry, slot).uv(UvMode::Box)
}

fn box_at(size: [f64; 3], at: [f64; 3]) -> Geometry {
    Geometry::cuboid(size).placed(Pose::at(at))
}

/// A white steel post set in the ground, a hazard band under its top and a
/// reflector plate facing `+z`: a delineator, slim enough to read as a line
/// of posts along the verge and not as a fence.
fn marker() -> Vec<Element> {
    let below = 0.45;
    vec![
        // It is the line a driver follows at dusk: kept at every distance
        // the piece is drawn, or a far level would drop it as too thin.
        element(
            "post",
            Geometry::chamfered_cuboid([0.11, MARKER_M + below, 0.11], 0.02)
                .placed(Pose::at([-0.055, -below, -0.055])),
            "white",
        )
        .collision(Collision::Bounds)
        .far(),
        element(
            "band",
            box_at([0.125, 0.2, 0.125], [-0.0625, MARKER_M - 0.32, -0.0625]),
            "hazard",
        ),
        // Hazard paint and not a lamp: a reflector shines only in a
        // vehicle's lights, which the colony's roads do not draw yet.
        element(
            "reflector",
            box_at([0.07, 0.12, 0.02], [-0.035, MARKER_M - 0.3, 0.055]),
            "hazard",
        )
        .far(),
    ]
}

/// A dark steel pole on a cast foot, an arm reaching out over the road
/// along `+z` and a lantern under its end: the colony's street lamp, set
/// where the road arrives somewhere.
fn lamp() -> Vec<Element> {
    let pole = Geometry::cylinder(0.075, LAMP_M + 0.4, 8).placed(Pose::at([0.0, -0.4, 0.0]));
    let arm = box_at(
        [0.08, 0.08, LAMP_REACH_M + 0.1],
        [-0.04, LAMP_M - 0.08, -0.05],
    );
    let brace = Geometry::hull([
        [-0.03, LAMP_M - 0.75, 0.0],
        [0.03, LAMP_M - 0.75, 0.0],
        [-0.03, LAMP_M - 0.65, 0.0],
        [0.03, LAMP_M - 0.65, 0.0],
        [-0.03, LAMP_M - 0.08, 0.75],
        [0.03, LAMP_M - 0.08, 0.75],
        [-0.03, LAMP_M - 0.02, 0.75],
        [0.03, LAMP_M - 0.02, 0.75],
    ]);
    let head = Geometry::chamfered_cuboid([0.3, 0.14, 0.55], 0.03).placed(Pose::at([
        -0.15,
        LAMP_M - 0.18,
        LAMP_REACH_M - 0.5,
    ]));
    vec![
        // The pole and the arm hold the lantern up at every distance.
        element("pole", pole.union(arm), "dark")
            .collision(Collision::Bounds)
            .far(),
        element("brace", brace, "dark"),
        element(
            "foot",
            Geometry::cylinder(0.24, 0.5, 8).placed(Pose::at([0.0, -0.3, 0.0])),
            "cast",
        )
        .collision(Collision::Bounds),
        element("head", head, "dark").far(),
        element("lantern", box_at(LANTERN_SIZE, LANTERN_CORNER), "lantern").far(),
        element(
            "band",
            Geometry::cylinder(0.085, 0.3, 8).placed(Pose::at([0.0, 1.1, 0.0])),
            "hazard",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both pieces validate, bind only the kit's slots and stand on their
    /// origin: a post reaching down into the ground and up to its height,
    /// a lamp's lantern out over `+z`.
    #[test]
    fn the_pieces_build_and_stand_on_their_origin() {
        for piece in Piece::ALL {
            let building = piece
                .building()
                .unwrap_or_else(|e| panic!("{piece:?}: {e}"));
            for (_, slot, binding) in building.bindings() {
                assert!(
                    MATERIALS
                        .iter()
                        .any(|(s, k)| *s == slot && *k == binding.material),
                    "{piece:?} binds {slot}"
                );
            }
        }
        let lantern = lamp()
            .into_iter()
            .find(|e| e.id == "lantern")
            .expect("a lantern");
        let [lo, hi] = lantern.geometry.bounds().expect("bounds");
        assert!(lo.z > 0.5 && hi.y > LAMP_M - 0.3, "{lo:?} {hi:?}");
        let post = marker()
            .into_iter()
            .find(|e| e.id == "post")
            .expect("a post");
        let [lo, hi] = post.geometry.bounds().expect("bounds");
        assert!(
            lo.y < -0.3 && (hi.y - MARKER_M).abs() < 1e-9,
            "{lo:?} {hi:?}"
        );
    }

    /// Whatever holds a far element up is kept as far: the lamp's pole
    /// under its lantern, the marker's post under its reflector.
    #[test]
    fn nothing_kept_far_is_left_in_the_air() {
        for (elements, holders) in [
            (lamp(), &["pole", "head", "lantern"][..]),
            (marker(), &["post", "reflector"][..]),
        ] {
            for id in holders {
                let element = elements
                    .iter()
                    .find(|e| e.id == *id)
                    .unwrap_or_else(|| panic!("no {id}"));
                assert!(element.far, "{id} is not kept far");
            }
        }
    }
}
