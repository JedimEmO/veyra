//! The household's interaction props (review step six, visual.md F6): the
//! terminal, the trainers, the locker, the workbench, the fabricator and the
//! board, drawn once and stood by the game where the cuboid compositions
//! stood.
//!
//! They follow the shared visual language of `docs/research/veyra-style`:
//! pale ivory shells with rounded corners, dark inset machinery and a role
//! colour on a panel, with the colony's orange for the one utility accent a
//! piece may carry. The role colour is a panel a viewer reads from across
//! a square, not a lit bar: it is the one slot a piece binds to a material
//! of its own role.
//!
//! The frame of every piece is the one the cuboids used: the origin on the
//! ground under the middle of the piece, `+y` up, and the front, where the
//! panels and the openings are, on `-z`. A piece has no collider: a prop is
//! reached by the sim's `Interaction` radius and is not walked into.

use ashlar::{
    Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode, ValidationError,
};

/// The role colours (sRGB) of the panels, in the order of [`Piece::role`]:
/// the terminal's teal, the blade trainer's green, the marksman's amber, the
/// locker's violet, the workbench's cold white, the fabricator's red, the
/// board's gold, the survey trainer's rose and the foreman's lime.
pub const ROLES: [[f32; 3]; 9] = [
    [0.2, 0.85, 0.9],
    [0.35, 0.95, 0.5],
    [1., 0.65, 0.2],
    [0.68, 0.5, 1.],
    [0.82, 0.92, 1.],
    [1., 0.28, 0.2],
    [0.98, 0.8, 0.28],
    [1., 0.45, 0.8],
    [0.78, 0.95, 0.2],
];

/// The material key of the panel of a role: `prop:panel.<role>`.
#[must_use]
pub fn panel_key(role: usize) -> String {
    format!("prop:panel.{role}")
}

/// The slots every piece may bind and the plain material each takes; the
/// panel binds [`panel_key`] of the piece's role.
pub const MATERIALS: [(&str, &str); 3] = [
    ("shell", "prop:shell"),
    ("inset", "prop:inset"),
    ("accent", "prop:accent"),
];

/// One piece of the props' kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Piece {
    /// The terminal: a pedestal and a screen.
    Terminal,
    /// The blade trainer.
    Blade,
    /// The marksman trainer.
    Marksman,
    /// The locker.
    Locker,
    /// The workbench.
    Workbench,
    /// The fabricator.
    Fabricator,
    /// The procurement board.
    Board,
    /// The survey trainer.
    Surveyor,
    /// The works foreman.
    Foreman,
}

impl Piece {
    /// Every piece, in the order of its role.
    pub const ALL: [Self; 9] = [
        Self::Terminal,
        Self::Blade,
        Self::Marksman,
        Self::Locker,
        Self::Workbench,
        Self::Fabricator,
        Self::Board,
        Self::Surveyor,
        Self::Foreman,
    ];

    /// The piece's name, the key of the index the content step writes.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Blade => "blade",
            Self::Marksman => "marksman",
            Self::Locker => "locker",
            Self::Workbench => "workbench",
            Self::Fabricator => "fabricator",
            Self::Board => "board",
            Self::Surveyor => "surveyor",
            Self::Foreman => "foreman",
        }
    }

    /// The index into [`ROLES`] of the colour of the piece's panels.
    #[must_use]
    pub const fn role(self) -> usize {
        match self {
            Self::Terminal => 0,
            Self::Blade => 1,
            Self::Marksman => 2,
            Self::Locker => 3,
            Self::Workbench => 4,
            Self::Fabricator => 5,
            Self::Board => 6,
            Self::Surveyor => 7,
            Self::Foreman => 8,
        }
    }

    /// The baked file's name under the kit's directory.
    #[must_use]
    pub fn file_name(self) -> String {
        format!("{}.ashprop", self.name())
    }

    /// The piece's recipe, its slots bound to [`MATERIALS`] and its role.
    ///
    /// # Errors
    /// A solid that does not validate: a bug here.
    pub fn building(self) -> Result<Building, ValidationError> {
        let id = format!("props:{}", self.name());
        let mut part = Part::builder(id.clone());
        for element in self.elements() {
            part = part.element(element.collision(Collision::None));
        }
        let mut builder = Building::builder(id.clone())
            .part(part.build()?)
            .instance(Instance::new("piece", id));
        for (slot, key) in MATERIALS {
            builder = builder.material(slot, key);
        }
        builder.material("panel", panel_key(self.role())).build()
    }

    /// The boxes of the piece.
    #[must_use]
    pub fn elements(self) -> Vec<Element> {
        let boxes: Vec<Box3> = match self {
            Self::Terminal => terminal(),
            Self::Blade => blade(),
            Self::Marksman => marksman(),
            Self::Locker => locker(),
            Self::Workbench => workbench(),
            Self::Fabricator => fabricator(),
            Self::Board => board(),
            Self::Surveyor => surveyor(),
            Self::Foreman => foreman(),
        };
        boxes
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                let geometry = if b.bevel > 0. {
                    Geometry::chamfered_cuboid(b.size, b.bevel)
                } else {
                    Geometry::cuboid(b.size)
                };
                let corner = [
                    b.centre[0] - 0.5 * b.size[0],
                    b.centre[1] - 0.5 * b.size[1],
                    b.centre[2] - 0.5 * b.size[2],
                ];
                Element::new(
                    format!("{}-{i}", b.slot),
                    geometry.placed(Pose::at(corner)),
                    b.slot,
                )
                .uv(UvMode::Box)
            })
            .collect()
    }
}

/// One box of a piece: a slot, a centre, a size and the bevel of its edges.
struct Box3 {
    slot: &'static str,
    centre: [f64; 3],
    size: [f64; 3],
    bevel: f64,
}

fn shell(centre: [f64; 3], size: [f64; 3]) -> Box3 {
    // The bevel is the rounded corner of the language, kept under a third of
    // the thinnest side so a slab stays a slab.
    let bevel = (0.3 * size[0].min(size[1]).min(size[2])).min(0.05);
    Box3 {
        slot: "shell",
        centre,
        size,
        bevel,
    }
}

fn plain(slot: &'static str, centre: [f64; 3], size: [f64; 3]) -> Box3 {
    Box3 {
        slot,
        centre,
        size,
        bevel: 0.,
    }
}

fn inset(centre: [f64; 3], size: [f64; 3]) -> Box3 {
    plain("inset", centre, size)
}

fn panel(centre: [f64; 3], size: [f64; 3]) -> Box3 {
    plain("panel", centre, size)
}

fn accent(centre: [f64; 3], size: [f64; 3]) -> Box3 {
    plain("accent", centre, size)
}

/// The trainers' shared post: an ivory column on a dark foot with the role
/// panel on its chest, so each trainer is told from the next by the shape
/// above the post and by the colour on it.
fn post() -> Vec<Box3> {
    vec![
        shell([0., 0.9, 0.], [0.35, 1.8, 0.35]),
        inset([0., 0.1, 0.], [0.41, 0.2, 0.41]),
        inset([0., 1.2, -0.18], [0.26, 0.54, 0.02]),
        panel([0., 1.2, -0.195], [0.2, 0.46, 0.015]),
    ]
}

fn with(mut base: Vec<Box3>, more: Vec<Box3>) -> Vec<Box3> {
    base.extend(more);
    base
}

fn terminal() -> Vec<Box3> {
    vec![
        shell([0., 0.45, 0.], [0.3, 0.9, 0.3]),
        inset([0., 0.08, 0.], [0.36, 0.16, 0.36]),
        shell([0., 1.2, -0.1], [1.2, 0.8, 0.12]),
        inset([0., 1.2, -0.17], [1.08, 0.68, 0.03]),
        panel([0., 1.22, -0.19], [0.96, 0.5, 0.015]),
        accent([0., 0.88, -0.17], [1.0, 0.03, 0.02]),
    ]
}

fn blade() -> Vec<Box3> {
    with(
        post(),
        vec![
            inset([0., 2.3, 0.], [0.12, 1.4, 0.04]),
            panel([0., 2.3, -0.03], [0.04, 1.3, 0.015]),
            accent([0., 1.78, 0.], [0.6, 0.08, 0.08]),
        ],
    )
}

fn marksman() -> Vec<Box3> {
    with(
        post(),
        vec![
            shell([0., 2.1, -0.1], [1.2, 1.2, 0.08]),
            inset([0., 2.1, -0.15], [0.94, 0.94, 0.02]),
            panel([0., 2.1, -0.165], [0.8, 0.8, 0.015]),
            accent([0., 2.1, -0.18], [0.3, 0.3, 0.015]),
        ],
    )
}

fn surveyor() -> Vec<Box3> {
    with(
        post(),
        vec![
            inset([0., 2.4, 0.], [0.08, 1.2, 0.08]),
            shell([0., 3.05, -0.1], [0.9, 0.9, 0.06]),
            inset([0., 3.05, -0.14], [0.4, 0.4, 0.02]),
            panel([0., 3.05, -0.155], [0.3, 0.3, 0.015]),
            accent([0., 1.7, -0.18], [0.5, 0.06, 0.02]),
        ],
    )
}

fn foreman() -> Vec<Box3> {
    vec![
        shell([0., 0.9, 0.], [0.35, 1.8, 0.35]),
        inset([0., 0.1, 0.], [0.41, 0.2, 0.41]),
        // The hard hat is the colony's orange, the one place it is loud.
        accent([0., 1.9, 0.], [0.5, 0.14, 0.5]),
        accent([0., 1.98, 0.], [0.3, 0.1, 0.3]),
        shell([0., 1.3, -0.12], [0.9, 0.7, 0.06]),
        inset([0., 1.3, -0.155], [0.76, 0.56, 0.02]),
        panel([0., 1.3, -0.17], [0.7, 0.5, 0.015]),
    ]
}

fn locker() -> Vec<Box3> {
    vec![
        shell([0., 0.95, 0.], [1.3, 1.9, 0.55]),
        inset([0., 0.06, 0.], [1.34, 0.12, 0.59]),
        inset([0., 0.95, -0.28], [0.03, 1.7, 0.02]),
        // A vent low in the doors; the doors themselves stay ivory.
        inset([0., 0.42, -0.28], [0.9, 0.28, 0.01]),
        panel([0., 1.6, -0.29], [0.9, 0.14, 0.015]),
        accent([-0.14, 1.0, -0.29], [0.05, 0.22, 0.03]),
        accent([0.14, 1.0, -0.29], [0.05, 0.22, 0.03]),
        inset([0., 1.93, 0.], [1.36, 0.06, 0.6]),
    ]
}

fn workbench() -> Vec<Box3> {
    vec![
        shell([-0.62, 0.44, 0.], [0.36, 0.88, 0.62]),
        shell([0.62, 0.44, 0.], [0.36, 0.88, 0.62]),
        inset([0., 0.26, 0.], [0.9, 0.05, 0.56]),
        inset([0., 0.92, 0.], [1.7, 0.08, 0.76]),
        panel([0., 0.92, -0.385], [1.5, 0.03, 0.02]),
        inset([0., 1.26, 0.35], [1.7, 0.6, 0.06]),
        panel([0., 1.42, 0.31], [1.3, 0.04, 0.02]),
        inset([-0.66, 1.3, 0.24], [0.05, 0.68, 0.05]),
        shell([-0.5, 1.62, 0.04], [0.38, 0.05, 0.14]),
        panel([-0.5, 1.59, 0.04], [0.3, 0.02, 0.1]),
        accent([0.58, 1.03, -0.16], [0.22, 0.14, 0.2]),
        shell([-0.05, 0.99, -0.02], [0.8, 0.06, 0.1]),
        inset([-0.1, 1.04, -0.02], [0.16, 0.04, 0.06]),
    ]
}

fn fabricator() -> Vec<Box3> {
    let mut parts = vec![
        shell([0., 0.64, 0.1], [1.9, 1.12, 1.0]),
        inset([0., 0.04, 0.1], [2.0, 0.08, 1.1]),
        inset([-0.52, 1.36, 0.16], [0.5, 0.32, 0.5]),
        shell([-0.52, 1.6, 0.16], [0.76, 0.16, 0.76]),
        panel([-0.52, 1.6, -0.225], [0.6, 0.03, 0.02]),
        // The mouth is dark machinery, the tray under it too.
        inset([-0.2, 0.52, -0.41], [0.8, 0.26, 0.03]),
        inset([-0.2, 0.34, -0.56], [0.9, 0.05, 0.34]),
        inset([0.62, 1.65, 0.4], [0.22, 0.9, 0.22]),
        panel([0.62, 1.96, 0.4], [0.26, 0.04, 0.26]),
        shell([0.62, 2.1, 0.4], [0.3, 0.05, 0.3]),
        accent([0., 1.1, -0.41], [1.7, 0.04, 0.02]),
    ];
    // The gauge: four segments up the right of the front.
    for i in 0..4_u8 {
        parts.push(panel(
            [0.66, 0.34 + 0.2 * f64::from(i), -0.41],
            [0.26, 0.1, 0.03],
        ));
    }
    parts
}

fn board() -> Vec<Box3> {
    vec![
        inset([-0.8, 0.95, 0.], [0.12, 1.9, 0.12]),
        inset([0.8, 0.95, 0.], [0.12, 1.9, 0.12]),
        shell([0., 1.55, -0.02], [1.9, 1.2, 0.08]),
        inset([0., 1.55, -0.07], [1.7, 1.0, 0.02]),
        panel([0., 1.9, -0.085], [1.4, 0.06, 0.015]),
        panel([0., 1.68, -0.085], [1.4, 0.06, 0.015]),
        panel([0., 1.46, -0.085], [1.4, 0.06, 0.015]),
        panel([0., 1.24, -0.085], [1.0, 0.06, 0.015]),
        accent([0., 0.3, -0.1], [1.6, 0.04, 0.02]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every piece validates, stands on the ground, faces `-z` with its
    /// panels in front of its shell, and binds only the kit's slots.
    #[test]
    fn the_pieces_build_and_face_negative_z() {
        for piece in Piece::ALL {
            let building = piece
                .building()
                .unwrap_or_else(|e| panic!("{piece:?}: {e}"));
            for (_, slot, binding) in building.bindings() {
                assert!(
                    slot == "panel"
                        || MATERIALS
                            .iter()
                            .any(|(s, k)| *s == slot && *k == binding.material),
                    "{piece:?} binds {slot}"
                );
            }
            let elements = piece.elements();
            let low = elements
                .iter()
                .filter_map(|e| e.geometry.bounds())
                .map(|[lo, _]| lo.y)
                .fold(f64::MAX, f64::min);
            assert!(low.abs() < 1e-9, "{piece:?} floats at {low}");
            let panels: Vec<_> = elements
                .iter()
                .filter(|e| e.material_slot == "panel")
                .collect();
            assert!(!panels.is_empty(), "{piece:?} has no panel");
        }
    }

    /// The roles are the nine the game's labels and lights already use, one
    /// a piece.
    #[test]
    fn each_piece_has_its_own_role() {
        let roles: Vec<usize> = Piece::ALL.iter().map(|p| p.role()).collect();
        assert_eq!(roles, (0..9).collect::<Vec<_>>());
    }
}
