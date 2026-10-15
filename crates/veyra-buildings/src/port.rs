//! Port fittings adapted from ashlar's city and frontier kits. Steel reveals,
//! utility services and restrained task lighting belong on the existing town.
//! Wall fittings face +Z, with their back at Z=0; furniture stands on Y=0.
use ashlar::{Collision, Element, Geometry, Part, Pose, UvMode, ValidationError};
use glam::{DQuat, DVec3};

/// Shared assemblies, kept separate so the distance policies drop small work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fitting {
    /// Dark inset paving and a drain along an Exchange footway.
    Paving,
    /// A short hazard-marked post at a pedestrian entrance.
    Bollard,
    /// A masonry planting bed, with a low, broad canopy.
    Bed,
    /// Compact stall fascia with a painted price-board and task light.
    MarketFascia,
    /// Steel cassette with recessed glazing and a light tucked under its head.
    Window,
    /// Lit shop fascia under a deep steel hood.
    Fascia,
    /// A utility corner's chamfered lamp housing, with a recessed light line.
    Corner,
    /// Meter, downpipe, air conditioner, drip tray and caged work lamp.
    Services,
    /// A projecting water-office sign with a tap pictogram.
    WaterSign,
    /// A near-ground kerb, wet drain channel and removable grate.
    Kerb,
    /// A small public job terminal, without a billboard or advertising screen.
    Terminal,
    /// A waiting shelter with a bench and a lit soffit.
    Shelter,
    /// Warm lamps on a sagging cable between two market posts.
    Lanterns,
    /// A place's name board at the edge of its pad, on two posts under a
    /// hood with a lamp: the board a road arrives at. Painted blank with
    /// stripes where the lettering goes, because one bake serves every
    /// place of a kind and a name is per place.
    Board,
    /// A survey marker: a concrete cairn with a banded steel post, a plate
    /// and a beacon lamp on top, what a surveyor leaves where a view or a
    /// fall is worth the walk.
    Marker,
}

impl Fitting {
    /// Every assembly, for the meshing and distance-budget review.
    pub const ALL: [Self; 15] = [
        Self::Paving,
        Self::Bollard,
        Self::Bed,
        Self::MarketFascia,
        Self::Window,
        Self::Fascia,
        Self::Corner,
        Self::Services,
        Self::WaterSign,
        Self::Kerb,
        Self::Terminal,
        Self::Shelter,
        Self::Lanterns,
        Self::Board,
        Self::Marker,
    ];

    /// Stable part identity shared across districts and generations.
    #[must_use]
    pub fn key(self) -> String {
        format!("port:{self:?}")
    }

    /// A reusable recipe on the city's semantic material slots.
    ///
    /// # Errors
    /// If an authored solid or element fails validation.
    pub fn part(self) -> Result<Part, ValidationError> {
        let elements = match self {
            Self::Paving => paving(),
            Self::Bollard => bollard(),
            Self::Bed => bed(),
            Self::MarketFascia => market_fascia(),
            Self::Window => window(),
            Self::Fascia => fascia(),
            Self::Corner => corner(),
            Self::Services => services(),
            Self::WaterSign => water_sign(),
            Self::Kerb => kerb(),
            Self::Terminal => terminal(),
            Self::Shelter => shelter(),
            Self::Lanterns => lanterns(),
            Self::Board => board(),
            Self::Marker => marker(),
        };
        let mut part = Part::builder(self.key());
        for element in elements {
            part = part.element(element);
        }
        part.build()
    }
}

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

fn window() -> Vec<Element> {
    let frame = Geometry::chamfered_cuboid([1.9, 1.65, 0.30], 0.045)
        .placed(Pose::at([-0.95, 0.0, 0.0]))
        .subtract(box_at([1.62, 1.35, 0.40], [-0.81, 0.15, -0.05]));
    vec![
        element("steel-reveal", frame, "metal"),
        element(
            "glass",
            box_at([1.64, 1.37, 0.04], [-0.82, 0.14, 0.035]),
            "pane",
        ),
        element(
            "mullion-sill",
            Geometry::union_all([
                box_at([0.065, 1.39, 0.14], [-0.0325, 0.13, 0.10]),
                box_at([2.02, 0.10, 0.43], [-1.01, -0.075, -0.02]),
            ]),
            "metal",
        ),
        element(
            "head-light",
            box_at([1.44, 0.045, 0.05], [-0.72, 1.44, 0.22]),
            "lantern",
        ),
    ]
}

fn fascia() -> Vec<Element> {
    vec![
        element(
            "hood",
            Geometry::union_all([
                box_at([3.6, 0.16, 0.8], [-1.8, 0.50, -0.08]),
                box_at([3.5, 0.46, 0.18], [-1.75, 0.05, 0.10]),
            ]),
            "metal",
        ),
        element(
            "painted-board",
            box_at([3.18, 0.30, 0.025], [-1.59, 0.12, 0.29]),
            "sign",
        ),
        element(
            "under-light",
            box_at([3.05, 0.055, 0.09], [-1.525, 0.02, 0.32]),
            "lantern",
        )
        .far(),
    ]
}

fn corner() -> Vec<Element> {
    vec![
        element(
            "chamfered-channel",
            Geometry::chamfered_cuboid([0.34, 2.8, 0.28], 0.06)
                .placed(Pose::at([-0.17, 0.0, 0.0]))
                .subtract(box_at([0.14, 2.44, 0.2], [-0.07, 0.18, 0.17])),
            "metal",
        ),
        element(
            "line",
            box_at([0.09, 2.35, 0.04], [-0.045, 0.22, 0.22]),
            "lantern",
        )
        .far(),
    ]
}

fn services() -> Vec<Element> {
    let pipe = rod([-0.62, 0.12, 0.10], [-0.62, 2.9, 0.10], 0.055);
    let mut steel = vec![pipe, box_at([1.12, 0.05, 0.57], [-0.48, 0.87, 0.015])];
    for y in [0.35, 1.8, 2.65] {
        steel.push(box_at([0.22, 0.07, 0.16], [-0.73, y, 0.015]));
    }
    for i in 0..7 {
        steel.push(box_at(
            [0.76, 0.025, 0.04],
            [-0.31, 1.04 + f64::from(i) * 0.065, 0.49],
        ));
    }
    // Four bars protect the bulb; the dark backplate gives it a real recess.
    for x in [-0.12, 0.12] {
        steel.push(rod([x, 2.25, 0.36], [x, 2.71, 0.36], 0.018));
    }
    for y in [2.28, 2.67] {
        steel.push(rod([-0.16, y, 0.36], [0.16, y, 0.36], 0.018));
    }
    vec![
        element("hardware", Geometry::union_all(steel), "metal"),
        element(
            "equipment",
            Geometry::union_all([
                Geometry::chamfered_cuboid([1.0, 0.58, 0.46], 0.04)
                    .placed(Pose::at([-0.42, 0.96, 0.01])),
                Geometry::chamfered_cuboid([0.38, 0.5, 0.16], 0.035)
                    .placed(Pose::at([0.17, 0.24, 0.01])),
            ]),
            "appliance",
        ),
        element(
            "recesses",
            Geometry::union_all([
                box_at([0.82, 0.45, 0.025], [-0.34, 1.025, 0.475]),
                box_at([0.38, 0.57, 0.08], [-0.19, 2.2, 0.02]),
            ]),
            "recess",
        ),
        element(
            "caged-lamp",
            box_at([0.18, 0.32, 0.13], [-0.09, 2.31, 0.17]),
            "lantern",
        ),
    ]
}

fn water_sign() -> Vec<Element> {
    vec![
        element(
            "brackets",
            Geometry::union_all([
                box_at([0.07, 0.07, 1.05], [-0.035, 0.08, 0.0]),
                box_at([0.07, 0.07, 1.05], [-0.035, 1.65, 0.0]),
            ]),
            "metal",
        ),
        element(
            "board",
            Geometry::chamfered_cuboid([0.12, 1.5, 0.85], 0.045)
                .placed(Pose::at([-0.06, 0.15, 0.17])),
            "sign",
        ),
        element(
            "tap-symbol",
            Geometry::union_all([-1.0, 1.0].map(|side| {
                let x = side * 0.073 - 0.009;
                Geometry::union_all([
                    box_at([0.018, 0.10, 0.48], [x, 1.12, 0.34]),
                    box_at([0.018, 0.28, 0.10], [x, 0.94, 0.72]),
                    box_at([0.018, 0.20, 0.10], [x, 1.21, 0.47]),
                    box_at([0.018, 0.055, 0.28], [x, 1.4, 0.38]),
                    box_at([0.018, 0.20, 0.10], [x, 0.58, 0.72]),
                ])
            })),
            "ink",
        ),
        element(
            "edge-lamp",
            box_at([0.14, 0.06, 0.71], [-0.07, 0.20, 0.24]),
            "lantern",
        )
        .far(),
    ]
}

fn kerb() -> Vec<Element> {
    vec![
        element(
            "kerb",
            Geometry::chamfered_cuboid([6.0, 0.18, 0.28], 0.035)
                .placed(Pose::at([-3.0, -0.02, -0.32])),
            "trim",
        )
        .collision(Collision::Bounds),
        element(
            "runoff-channel",
            box_at([6.0, 0.025, 0.36], [-3.0, 0.025, 0.0]),
            "recess",
        ),
        element(
            "grate",
            box_at([0.055, 0.023, 0.30], [-0.31, 0.055, 0.03])
                .arrayed(8, Pose::at([0.08, 0.0, 0.0])),
            "metal",
        ),
    ]
}

fn terminal() -> Vec<Element> {
    vec![
        element(
            "housing",
            Geometry::chamfered_cuboid([0.95, 1.8, 0.6], 0.10)
                .placed(Pose::at([-0.475, 0.05, -0.3]))
                .subtract(box_at([0.70, 0.64, 0.2], [-0.35, 0.99, 0.18])),
            "shell",
        )
        .collision(Collision::Bounds),
        element(
            "base-hood",
            Geometry::union_all([
                box_at([1.1, 0.15, 0.76], [-0.55, 0.01, -0.38]),
                box_at([1.0, 0.08, 0.85], [-0.50, 1.84, -0.32]),
            ]),
            "metal",
        ),
        element(
            "display",
            box_at([0.64, 0.56, 0.025], [-0.32, 1.03, 0.195]),
            "recess",
        ),
        element(
            "directory-lines",
            Geometry::union_all([
                box_at([0.45, 0.035, 0.015], [-0.24, 1.43, 0.232]),
                box_at([0.31, 0.025, 0.015], [-0.24, 1.31, 0.232]),
                box_at([0.36, 0.025, 0.015], [-0.24, 1.20, 0.232]),
            ]),
            "lantern",
        ),
    ]
}

fn shelter() -> Vec<Element> {
    vec![
        element(
            "left-post",
            box_at([0.12, 3.0, 0.14], [-1.85, 0.0, -1.05]),
            "metal",
        )
        .collision(Collision::Bounds),
        element(
            "right-post",
            box_at([0.12, 3.0, 0.14], [1.73, 0.0, -1.05]),
            "metal",
        )
        .collision(Collision::Bounds),
        element(
            "canopy",
            Geometry::chamfered_cuboid([4.3, 0.24, 2.8], 0.09)
                .placed(Pose::at([-2.15, 3.0, -1.25])),
            "shell",
        ),
        element(
            "bench",
            Geometry::union_all([
                box_at([3.2, 0.14, 0.6], [-1.6, 0.47, -0.8]),
                box_at([3.2, 0.5, 0.10], [-1.6, 0.62, -0.84]),
            ]),
            "boards",
        ),
        element(
            "bench-legs",
            Geometry::union_all([-1.4, 1.2].map(|x| box_at([0.2, 0.46, 0.48], [x, 0.02, -0.76]))),
            "metal",
        ),
        element(
            "soffit",
            box_at([2.8, 0.035, 0.12], [-1.4, 2.96, 0.85]),
            "lantern",
        )
        .far(),
    ]
}

fn lanterns() -> Vec<Element> {
    let y = |x: f64| 3.6 + 0.045 * x * x;
    let cable = (-4..4).map(|i| {
        let x = f64::from(i);
        rod([x, y(x), 0.0], [x + 1.0, y(x + 1.0), 0.0], 0.015)
    });
    let bulbs = [-3.0, -1.5, 0.0, 1.5, 3.0]
        .map(|x| Geometry::cylinder(0.12, 0.25, 8).placed(Pose::at([x, y(x) - 0.32, 0.0])));
    vec![
        element(
            "posts-cable",
            Geometry::union_all(
                cable.chain([-4.0, 4.0].map(|x| rod([x, 0.0, 0.0], [x, 4.4, 0.0], 0.055))),
            ),
            "metal",
        ),
        element("warm-lanterns", Geometry::union_all(bulbs), "lantern").far(),
    ]
}

fn paving() -> Vec<Element> {
    vec![
        element(
            "setts",
            box_at([5.98, 0.06, 2.0], [-2.99, 0.0, -1.0]),
            "trim",
        ),
        element(
            "drain",
            box_at([5.98, 0.03, 0.18], [-2.99, 0.07, 0.7]),
            "metal",
        ),
    ]
}

fn bollard() -> Vec<Element> {
    vec![
        element(
            "post",
            Geometry::chamfered_cuboid([0.26, 0.9, 0.26], 0.04)
                .placed(Pose::at([-0.13, 0.0, -0.13])),
            "metal",
        )
        .collision(Collision::Bounds),
        element(
            "marker",
            box_at([0.28, 0.15, 0.28], [-0.14, 0.66, -0.14]),
            "hazard",
        ),
    ]
}

fn bed() -> Vec<Element> {
    let rim = box_at([4.0, 0.5, 2.2], [-2.0, 0.0, -1.1])
        .subtract(box_at([3.6, 0.6, 1.8], [-1.8, 0.15, -0.9]));
    vec![
        element("rim", rim, "trim").collision(Collision::Bounds),
        element(
            "soil",
            box_at([3.62, 0.15, 1.82], [-1.81, 0.17, -0.91]),
            "garden",
        ),
        element(
            "shrubs",
            Geometry::union_all(
                [-1.2, 0.0, 1.2].map(|x| Geometry::ball(0.68, 6).placed(Pose::at([x, 0.74, 0.0]))),
            ),
            "leaves",
        ),
    ]
}

fn market_fascia() -> Vec<Element> {
    vec![
        element(
            "hood",
            box_at([2.3, 0.1, 0.35], [-1.15, 0.27, -0.12]),
            "metal",
        ),
        element(
            "board",
            box_at([2.15, 0.25, 0.065], [-1.075, 0.015, 0.08]),
            "sign",
        ),
        element(
            "labels",
            Geometry::union_all(
                [-0.6, 0.0, 0.6].map(|x| box_at([0.28, 0.12, 0.016], [x - 0.14, 0.07, 0.15])),
            ),
            "ink",
        ),
        element(
            "task-light",
            box_at([1.9, 0.045, 0.06], [-0.95, -0.045, 0.11]),
            "lantern",
        )
        .far(),
    ]
}

/// Two posts on concrete footings, a board between them facing `+z`, a hood
/// over it and a light under the hood. About 2.6 m wide and 3.2 m high, so
/// it reads at a road's arrival from a hundred metres.
fn board() -> Vec<Element> {
    // The posts stand in their footings, so no post's foot lies on a
    // footing's top face.
    let posts =
        Geometry::union_all([-1.2, 1.2].map(|x| box_at([0.12, 3.0, 0.12], [x - 0.06, 0.2, -0.06])));
    let footings = Geometry::union_all(
        [-1.2, 1.2].map(|x| box_at([0.36, 0.3, 0.36], [x - 0.18, -0.05, -0.18])),
    );
    vec![
        // The posts hold the board up at every distance: dropped as too thin
        // past 60 m, they would leave the board in the air.
        element("posts", posts, "metal")
            .collision(Collision::Bounds)
            .far(),
        element("footings", footings, "trim"),
        element(
            "board",
            Geometry::chamfered_cuboid([2.6, 1.05, 0.08], 0.02)
                .placed(Pose::at([-1.3, 1.65, 0.05])),
            "sign",
        )
        .collision(Collision::Bounds),
        element(
            "lettering",
            Geometry::union_all([
                box_at([1.9, 0.16, 0.016], [-0.95, 2.3, 0.126]),
                box_at([1.3, 0.1, 0.016], [-0.65, 2.02, 0.126]),
                box_at([0.7, 0.08, 0.016], [-0.35, 1.82, 0.126]),
            ]),
            "ink",
        ),
        element(
            "hood",
            box_at([2.8, 0.08, 0.42], [-1.4, 2.78, -0.06]),
            "metal",
        ),
        element(
            "lamp",
            box_at([1.8, 0.05, 0.06], [-0.9, 2.71, 0.24]),
            "lantern",
        )
        .far(),
    ]
}

/// A cairn of concrete, a steel post banded in hazard paint, a small plate
/// facing `+z` and a lamp on top: about 3.6 m, so it stands out of the
/// grass and shows its light at dusk.
fn marker() -> Vec<Element> {
    let cairn = Geometry::cylinder(0.75, 0.55, 6)
        .union(Geometry::cylinder(0.5, 0.35, 6).placed(Pose::at([0.0, 0.5, 0.0])));
    vec![
        element("cairn", cairn, "trim").collision(Collision::Bounds),
        element(
            "post",
            Geometry::cylinder(0.08, 3.0, 8).placed(Pose::at([0.0, 0.6, 0.0])),
            "metal",
        )
        .collision(Collision::Bounds)
        // It holds the lamp up at every distance: dropped as too thin past
        // 60 m, it would leave the lamp in the air over the cairn.
        .far(),
        element(
            "bands",
            Geometry::union_all(
                [1.45, 2.35]
                    .map(|y| Geometry::cylinder(0.105, 0.28, 8).placed(Pose::at([0.0, y, 0.0]))),
            ),
            "hazard",
        ),
        element(
            "plate",
            box_at([0.46, 0.32, 0.03], [-0.23, 1.75, 0.06]),
            "sign",
        ),
        element(
            "plate-marks",
            Geometry::union_all([
                box_at([0.3, 0.04, 0.012], [-0.15, 1.97, 0.084]),
                box_at([0.2, 0.03, 0.012], [-0.1, 1.86, 0.084]),
            ]),
            "ink",
        ),
        element(
            "lamp",
            Geometry::cylinder(0.13, 0.22, 8).placed(Pose::at([0.0, 3.55, 0.0])),
            "lantern",
        )
        .far(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever holds a far element up is kept as far as it is: a thin post
    /// dropped at a distance would leave the board or the lamp floating.
    #[test]
    fn nothing_kept_far_is_left_in_the_air() {
        for (fitting, holders) in [
            (Fitting::Board, &["posts", "board", "lamp"][..]),
            (Fitting::Marker, &["cairn", "post", "lamp"][..]),
        ] {
            let part = fitting.part().expect("a valid fitting");
            for id in holders {
                let element = part
                    .elements
                    .iter()
                    .find(|e| e.id == *id)
                    .unwrap_or_else(|| panic!("{fitting:?} has no {id}"));
                let [lo, hi] = element.geometry.bounds().expect("bounds");
                let size = hi - lo;
                // ashlar judges thin by the middle of the three extents; the
                // level from 60 m keeps nothing under 35 cm, so an element
                // that holds something up is at least that, or kept far.
                let mut extents = size.to_array();
                extents.sort_by(f64::total_cmp);
                assert!(
                    element.far || extents[1] >= 0.35,
                    "{fitting:?}: {id} is {size:?} and not kept far"
                );
            }
        }
    }
}
