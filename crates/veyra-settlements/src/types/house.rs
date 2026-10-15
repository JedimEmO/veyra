//! The Terraces' house as a catalogue of shared parts
//! (`docs/design/port-serein-fidelity.md`, Pass 3).
//!
//! A house used to be one part baked from all its owner's choices, so six
//! hundred houses were six hundred meshes and none could afford detail. Here a
//! house is a handful of instances of a few dozen richly detailed parts: a
//! ground module, perhaps an upper one set back on its roof, a roof over the
//! top, and what an owner adds (a shed, an awning, a balcony, a tank, a stair
//! hutch, panels, a dish, washing, an air conditioner). The owner's choices
//! pick parts and where they stand; the paint, the joinery's colour, the
//! awning's cloth and whether the glass is lit are bound per instance, so a
//! part is shared by every house that uses it however it is painted.
//!
//! A module is a solid block, since nobody goes inside, carved with recessed
//! windows and a door, and dressed in four elements: the painted walls; the
//! concrete trim (plinth, string courses, a two-step cornice, corner
//! pilasters, the parapet's coping, sills, hoods and surrounds); the painted
//! joinery (frames with mullions, louvred shutters, a panelled door, rails,
//! downpipes); and the glass. Fine members sit in the trim and the joinery as
//! their own union members, so the level past 60 m drops them and keeps the
//! frames, pilasters and cornices. Nothing projects past a module's sides,
//! so attached neighbours never lay two faces on one plane.

use ashlar::glam::{DMat3, DQuat, DVec3};
use ashlar::{Collision, Element, Geometry, Part, Pose, UvMode};

use super::STOREY;

/// The widths a ground module comes in, metres along the row.
pub const WIDTHS: [f64; 3] = [6.0, 8.0, 10.0];

/// The depths a ground module comes in, metres front to back.
pub const DEPTHS: [f64; 3] = [6.0, 8.0, 10.0];

/// The widths of the module an owner builds on the roof: always narrower than
/// the one below, so it stands inside its parapet.
pub const UPPER_WIDTHS: [f64; 3] = [4.0, 6.0, 8.0];

/// The depths of the module on the roof.
pub const UPPER_DEPTHS: [f64; 2] = [4.0, 6.0];

/// A workshop's width; it comes in [`DEPTHS`]' two larger depths.
pub const WORKSHOP: f64 = 12.0;

/// The strip a shed takes beside a module, metres.
pub const SHED: f64 = 2.5;

/// The parapet's height above the roof, and its thickness.
const PARAPET: f64 = 0.8;
const INSET: f64 = 0.25;

/// The coping's thickness on the parapet.
const COPING: f64 = 0.1;

/// The plinth's height.
const PLINTH: f64 = 0.45;

/// The floor at a door: a step above the lane.
const THRESHOLD: f64 = 0.16;

/// A workshop's one tall storey.
const WORKSHOP_HEIGHT: f64 = 5.6;

/// A door's opening, fanlight or vision panel included.
const DOOR: [f64; 2] = [1.2, 2.45];

/// How a module's windows and door are made: what its builder had to hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    /// Tall windows with six panes and louvred shutters folded back, a
    /// panelled door with a fanlight under a hood on corbels.
    Shuttered,
    /// Wide windows in deep concrete box frames with a mullion and a top
    /// light, a flush door under a concrete canopy: the company's way.
    Casement,
}

impl Style {
    /// A window's opening, width and height, and its sill over the floor.
    fn window(self) -> (f64, f64, f64) {
        match self {
            Self::Shuttered => (0.9, 1.5, 0.9),
            Self::Casement => (1.6, 1.4, 0.95),
        }
    }

    /// The window bays across a front `width` wide: their centres. A front
    /// with a door keeps at least two, the door's and a window's.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a handful of bays"
    )]
    fn bays(self, width: f64, door: bool) -> Vec<f64> {
        let pitch = match self {
            Self::Shuttered => 2.1,
            Self::Casement => 2.6,
        };
        // Clear of the corner pilasters and the downpipe beside one.
        let span = width - 1.4;
        let count = ((span / pitch).floor() as u32).max(if door { 2 } else { 1 });
        let step = span / f64::from(count);
        (0..count)
            .map(|i| -span * 0.5 + step * (f64::from(i) + 0.5))
            .collect()
    }

    /// Which of `bays` bays is the door, on the front or at the back.
    fn door_bay(self, bays: usize, front: bool) -> usize {
        match (self, front) {
            (Self::Shuttered, true) | (Self::Casement, false) => 0,
            (Self::Shuttered, false) => bays - 1,
            (Self::Casement, true) => bays / 2,
        }
    }

    /// A string course's height and how far it stands proud: a slim band
    /// in render, or the company's bold slab edge.
    fn course(self) -> (f64, f64) {
        match self {
            Self::Shuttered => (0.16, 0.08),
            Self::Casement => (0.24, 0.14),
        }
    }

    fn letter(self) -> char {
        match self {
            Self::Shuttered => 's',
            Self::Casement => 'c',
        }
    }
}

/// A block of storeys with a flat roof behind a parapet, windows front and
/// back, and on the ground a door each side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Module {
    /// Along the row, metres: one of [`WIDTHS`] or [`UPPER_WIDTHS`].
    pub width: f64,
    /// Front to back, metres.
    pub depth: f64,
    /// Its storeys, one or two.
    pub storeys: u32,
    /// Its windows and doors.
    pub style: Style,
    /// On the ground, with a door front and back; on a roof, windows only.
    pub door: bool,
}

impl Module {
    /// The window bays across its front, in its own frame.
    #[must_use]
    pub fn bays(&self) -> Vec<f64> {
        self.style.bays(self.width, self.door)
    }

    /// Where its front door is along the row, in its own frame.
    #[must_use]
    pub fn door_at(&self) -> f64 {
        let bays = self.bays();
        bays[self.style.door_bay(bays.len(), true)]
    }

    /// Its roof, the top of the membrane inside the parapet.
    fn roof(&self) -> f64 {
        f64::from(self.storeys) * STOREY + 0.04
    }
}

/// The owner's cover over the top module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Top {
    /// The flat roof behind its parapet, which collects what the owner put
    /// there.
    Flat,
    /// A shallow barrel vault of sheet across the depth, on the parapet.
    Vault,
    /// A lean-to of sheet falling to the front.
    Lean,
}

/// What an owner put on a flat roof and the walls, as bits.
pub mod roof {
    /// Planters.
    pub const GARDEN: u8 = 1;
    /// A water tank on a stand.
    pub const TANK: u8 = 2;
    /// A stair hutch.
    pub const HUTCH: u8 = 4;
    /// Solar panels.
    pub const SOLAR: u8 = 8;
    /// A satellite dish.
    pub const DISH: u8 = 16;
    /// An aerial.
    pub const AERIAL: u8 = 32;
    /// Washing on a line.
    pub const WASHING: u8 = 64;
    /// An air conditioner, on a wall under an upper window or on the roof.
    pub const AC: u8 = 128;
}

/// The module an owner stacked on the ground one, stood against the back of
/// its roof so the front is a terrace behind the parapet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Upper {
    /// One of [`UPPER_WIDTHS`], at most the ground's width less a metre.
    pub width: f64,
    /// One of [`UPPER_DEPTHS`], leaving a terrace of at least a metre and a
    /// half in front.
    pub depth: f64,
    /// How far it is shifted along the row from the ground's middle.
    pub shift: f64,
}

/// A terrace house, in its own frame: origin at the middle of its lot on its
/// terrace, `+z` its front, `x` along the row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerraceHouse {
    /// The ground module's width, one of [`WIDTHS`], or [`WORKSHOP`].
    pub width: f64,
    /// The ground module's depth, one of [`DEPTHS`], and the lot's.
    pub depth: f64,
    /// The ground module's storeys.
    pub lower: u32,
    /// Its windows and doors, and the upper module's.
    pub style: Style,
    /// A workshop: one tall storey with a roller door, and no upper module.
    pub workshop: bool,
    /// The module on the roof, if the owner built one.
    pub upper: Option<Upper>,
    /// A shed in a strip on the `+x` (`Some(true)`) or `-x` side.
    pub shed: Option<bool>,
    /// An awning over the door (a workshop's over its roller door).
    pub awning: bool,
    /// A balcony over the door, on a two-storey ground module.
    pub balcony: bool,
    /// The cover over the top module.
    pub top: Top,
    /// What stands on the roof and hangs on the walls, as [`roof`] bits.
    pub roof: u8,
}

/// One part of the catalogue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// Retaining lower floor, with a door onto the lower lane and a roof at the upper bench.
    Bank(f64, f64, f64),
    /// A module.
    Module(Module),
    /// A workshop of this depth.
    Workshop(f64),
    /// A barrel vault over a module this wide and deep.
    Vault(f64, f64),
    /// A lean-to over a module this wide and deep.
    Lean(f64, f64),
    /// A balcony on corbels with a railing.
    Balcony,
    /// An awning on brackets, over a door or (wide) a roller door.
    Awning(bool),
    /// A corrugated shed.
    Shed,
    /// A water tank on a stand.
    Tank,
    /// An air conditioner on brackets.
    Ac,
    /// A satellite dish on a short mast.
    Dish,
    /// Solar panels on a frame.
    Solar,
    /// Washing on a line between two posts.
    Washing,
    /// A stair hutch.
    Hutch,
    /// An aerial.
    Aerial,
    /// Planters along a parapet.
    Garden,
}

impl Piece {
    /// A key two pieces share exactly when their geometry is the same.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Module(m) => format!(
                "house-{}{:.0}x{:.0}-{}{}",
                if m.door { "ground" } else { "upper" },
                m.width,
                m.depth,
                m.storeys,
                m.style.letter()
            ),
            Self::Bank(w, d, h) => format!("house-bank-{w:.0}x{d:.2}x{h:.3}"),
            Self::Workshop(d) => format!("house-workshop-{d:.0}"),
            Self::Vault(w, d) => format!("house-vault-{w:.0}x{d:.0}"),
            Self::Lean(w, d) => format!("house-lean-{w:.0}x{d:.0}"),
            Self::Awning(wide) => format!("house-awning{}", if *wide { "-wide" } else { "" }),
            other => format!("house-{}", format!("{other:?}").to_lowercase()),
        }
    }

    /// The part, named `id`.
    ///
    /// # Errors
    ///
    /// If the recipe does not validate, which would be a bug here.
    pub fn part(&self, id: &str) -> anyhow::Result<Part> {
        let elements = match *self {
            Self::Module(m) => module(m),
            Self::Bank(w, d, h) => bank(w, d, h),
            Self::Workshop(d) => workshop(d),
            Self::Vault(w, d) => vault(w, d),
            Self::Lean(w, d) => lean(w, d),
            Self::Balcony => balcony(),
            Self::Awning(wide) => awning(if wide { 5.0 } else { 2.0 }),
            Self::Shed => shed(),
            Self::Tank => tank(),
            Self::Ac => ac(),
            Self::Dish => dish(),
            Self::Solar => solar(),
            Self::Washing => washing(),
            Self::Hutch => hutch(),
            Self::Aerial => aerial(),
            Self::Garden => garden(),
        };
        let mut part = Part::builder(id);
        for element in elements {
            part = part.element(element);
        }
        Ok(part.build()?)
    }

    /// Every part the catalogue has, for a contact sheet: each module in both
    /// styles, the covers over every top a house can have, the add-ons.
    #[must_use]
    pub fn catalogue() -> Vec<Self> {
        let mut pieces = Vec::new();
        for style in [Style::Shuttered, Style::Casement] {
            for storeys in [1, 2] {
                for width in WIDTHS {
                    for depth in DEPTHS {
                        pieces.push(Self::Module(Module {
                            width,
                            depth,
                            storeys,
                            style,
                            door: true,
                        }));
                    }
                }
            }
            for width in UPPER_WIDTHS {
                for depth in UPPER_DEPTHS {
                    pieces.push(Self::Module(Module {
                        width,
                        depth,
                        storeys: 1,
                        style,
                        door: false,
                    }));
                }
            }
        }
        for depth in &DEPTHS[1..] {
            pieces.push(Self::Workshop(*depth));
        }
        for (width, depth) in WIDTHS
            .iter()
            .flat_map(|w| DEPTHS.iter().map(move |d| (*w, *d)))
            .chain(
                UPPER_WIDTHS
                    .iter()
                    .flat_map(|w| UPPER_DEPTHS.iter().map(move |d| (*w, *d))),
            )
        {
            // A ground module and an upper one may share a size.
            if !pieces.contains(&Self::Vault(width, depth)) {
                pieces.push(Self::Vault(width, depth));
                pieces.push(Self::Lean(width, depth));
            }
        }
        pieces.extend([
            Self::Balcony,
            Self::Awning(false),
            Self::Awning(true),
            Self::Shed,
            Self::Tank,
            Self::Ac,
            Self::Dish,
            Self::Solar,
            Self::Washing,
            Self::Hutch,
            Self::Aerial,
            Self::Garden,
        ]);
        pieces
    }

    /// The footprint it needs on a roof, `x` by `z`, metres, for the add-ons
    /// that stand on one.
    fn footprint(self) -> [f64; 2] {
        match self {
            Self::Tank => [1.4, 1.4],
            Self::Hutch => [2.2, 3.0],
            Self::Solar => [2.3, 1.3],
            Self::Dish => [0.9, 0.9],
            Self::Aerial => [0.5, 0.5],
            Self::Washing => [3.8, 0.8],
            Self::Garden => [3.2, 0.7],
            Self::Ac => [1.0, 0.8],
            _ => [0.0, 0.0],
        }
    }
}

impl TerraceHouse {
    /// The lot's width along the row: the module and any shed beside it.
    #[must_use]
    pub fn lot_width(&self) -> f64 {
        self.width + if self.shed.is_some() { SHED } else { 0.0 }
    }

    /// The ground module's height to its roof, metres.
    fn ground_height(&self) -> f64 {
        if self.workshop {
            WORKSHOP_HEIGHT
        } else {
            f64::from(self.lower) * STOREY
        }
    }

    /// The house's height to the top of its highest parapet, metres.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.ground_height() + self.upper.map_or(0.0, |_| STOREY) + PARAPET + COPING
    }

    /// The ground module, or `None` for a workshop.
    fn ground(&self) -> Option<Module> {
        (!self.workshop).then_some(Module {
            width: self.width,
            depth: self.depth,
            storeys: self.lower,
            style: self.style,
            door: true,
        })
    }

    /// The parts this house is made of and where each stands in its frame.
    #[must_use]
    #[allow(clippy::too_many_lines, reason = "one house, part by part")]
    pub fn pieces(&self) -> Vec<(Piece, Pose)> {
        let at = |x: f64, y: f64, z: f64| Pose::at([x, y, z]);
        let turned = |x: f64, y: f64, z: f64, yaw: f64| {
            Pose::at([x, y, z]).rotated(DQuat::from_rotation_y(yaw))
        };
        let (w, d) = (self.width, self.depth);
        // The shed takes a strip off one side of the lot.
        let side = self.shed.map(|plus| if plus { 1.0 } else { -1.0 });
        let mx = side.map_or(0.0, |s| -s * SHED * 0.5);
        let mut pieces = Vec::new();
        let ground = self.ground();
        let gh = self.ground_height();
        pieces.push((
            ground.map_or(Piece::Workshop(d), Piece::Module),
            at(mx, 0.0, 0.0),
        ));
        if let Some(s) = side {
            pieces.push((
                Piece::Shed,
                at(mx + s * (w * 0.5 + SHED * 0.5), 0.0, d * 0.5 - 2.3),
            ));
        }
        // The top module and its roof well: centre, width, depth, floor.
        let mut top = (mx, 0.0, w, d, gh + 0.04);
        // The terrace a setback leaves on the ground module's roof.
        let mut terrace = None;
        if let (Some(u), Some(_)) = (self.upper, ground) {
            let (ux, uz) = (mx + u.shift, -d * 0.5 + INSET + u.depth * 0.5);
            let upper = Module {
                width: u.width,
                depth: u.depth,
                storeys: 1,
                style: self.style,
                door: false,
            };
            pieces.push((Piece::Module(upper), at(ux, gh, uz)));
            terrace = Some((mx, uz + u.depth * 0.5, w, d * 0.5 - INSET, gh + 0.04));
            top = (ux, uz, u.width, u.depth, gh + upper.roof());
        }
        let (tx, tz, tw, td, ty) = top;
        let crown = ty - 0.04 + PARAPET + COPING;
        match self.top {
            Top::Vault => pieces.push((Piece::Vault(tw, td), at(tx, crown, tz))),
            Top::Lean => pieces.push((Piece::Lean(tw, td), at(tx, crown, tz))),
            Top::Flat => {}
        }
        // What goes on the ground module's walls.
        if let Some(g) = ground {
            let door = mx + g.door_at();
            if self.balcony && g.storeys >= 2 {
                pieces.push((Piece::Balcony, at(door, STOREY + 0.14, d * 0.5)));
            }
            if self.awning && self.style == Style::Shuttered {
                pieces.push((Piece::Awning(false), at(door, 3.06, d * 0.5)));
            }
        } else if self.awning {
            pieces.push((Piece::Awning(true), at(mx - 2.65, 4.62, d * 0.5)));
        }
        // An air conditioner hangs under an upper window at the back, or
        // stands on the roof of a house with no upper window.
        let mut roof_ac = false;
        if self.roof & roof::AC != 0 {
            match ground {
                Some(g) if g.storeys >= 2 => {
                    let bays = g.bays();
                    let u = bays[bays.len() - 1];
                    pieces.push((
                        Piece::Ac,
                        turned(mx - u, STOREY + 0.28, -d * 0.5, std::f64::consts::PI),
                    ));
                }
                _ => roof_ac = true,
            }
        }
        // The setback terrace takes the planters, where there is one; the
        // top's flat roof what is left.
        let mut garden = self.roof & roof::GARDEN != 0;
        if let Some((cx, z0, cw, z1, floor)) = terrace
            && garden
            && z1 - z0 >= 0.9
            && cw >= 5.0
        {
            pieces.push((Piece::Garden, at(cx, floor, z1 - 0.4)));
            garden = false;
        }
        if self.top != Top::Flat {
            return pieces;
        }
        // The roof well's corners, and what already stands in it.
        let (x0, x1) = (tx - tw * 0.5 + INSET, tx + tw * 0.5 - INSET);
        let (z0, z1) = (tz - td * 0.5 + INSET, tz + td * 0.5 - INSET);
        let mut taken: Vec<[f64; 4]> = Vec::new();
        let mut place = |piece: Piece, [u, v]: [f64; 2], yaw: f64| -> Option<(Piece, Pose)> {
            // `u` and `v` in 0..1 across the well, the footprint's own
            // corner nearest that way.
            let [fw, fd] = piece.footprint();
            let cx = x0 + fw * 0.5 + 0.1 + u * (x1 - x0 - fw - 0.2);
            let cz = z0 + fd * 0.5 + 0.1 + v * (z1 - z0 - fd - 0.2);
            let rect = [cx - fw * 0.5, cz - fd * 0.5, cx + fw * 0.5, cz + fd * 0.5];
            let fits = x1 - x0 >= fw + 0.2
                && z1 - z0 >= fd + 0.2
                && taken.iter().all(|o| {
                    rect[2] <= o[0] || rect[0] >= o[2] || rect[3] <= o[1] || rect[1] >= o[3]
                });
            fits.then(|| {
                taken.push(rect);
                (piece, turned(cx, ty, cz, yaw))
            })
        };
        let bits = self.roof;
        let wants = |bit: u8| bits & bit != 0;
        let mut add = |piece: Piece, spot: [f64; 2], yaw: f64, wanted: bool| {
            if wanted && let Some(p) = place(piece, spot, yaw) {
                pieces.push(p);
            }
        };
        // Stair hutch at the back, its door to the front; tank in the other
        // back corner; panels and planters to the front; washing across the
        // middle; the dish and the aerial where there is room.
        add(Piece::Hutch, [0.0, 0.0], 0.0, wants(roof::HUTCH));
        add(Piece::Tank, [1.0, 0.0], 0.0, wants(roof::TANK));
        add(Piece::Solar, [1.0, 1.0], 0.0, wants(roof::SOLAR));
        add(Piece::Garden, [0.0, 1.0], 0.0, garden);
        add(Piece::Washing, [0.5, 0.55], 0.0, wants(roof::WASHING));
        add(Piece::Ac, [1.0, 0.5], std::f64::consts::FRAC_PI_2, roof_ac);
        add(Piece::Dish, [0.0, 0.6], 0.6, wants(roof::DISH));
        add(Piece::Aerial, [0.6, 0.0], 0.0, wants(roof::AERIAL));
        pieces
    }
}

/// Review scenes of the catalogue: every part on a grid, a street of
/// composed houses, and single houses, for the preview's gallery.
pub mod review {
    use ashlar::glam::DQuat;
    use ashlar::{Building, Instance, Pose};

    use super::{Module, Piece, Style, TerraceHouse, Top, Upper, roof};
    use crate::types::{FABRICS, JOINERY, KIT_MATERIALS, PAINTS, PANES, TRIMS};

    /// Builds a review building from parts placed with their bindings.
    fn assemble(
        id: &str,
        placed: impl IntoIterator<Item = (Piece, Pose, [&'static str; 5])>,
    ) -> anyhow::Result<Building> {
        let mut builder = Building::builder(id);
        // Each part's slots, so an instance binds only what its part has.
        let mut parts = std::collections::BTreeMap::new();
        let mut used = std::collections::BTreeSet::new();
        for (i, (piece, pose, [paint, joinery, pane, fabric, trim])) in
            placed.into_iter().enumerate()
        {
            let key = piece.key();
            if !parts.contains_key(&key) {
                let part = piece.part(&key)?;
                let slots: std::collections::BTreeSet<String> = part
                    .elements
                    .iter()
                    .map(|e| e.material_slot.clone())
                    .collect();
                used.extend(slots.iter().cloned());
                parts.insert(key.clone(), slots);
                builder = builder.part(part);
            }
            let mut instance = Instance::new(format!("piece-{i}"), key.clone()).placed(pose);
            for (slot, material) in [
                ("paint", paint),
                ("trim", trim),
                ("joinery", joinery),
                ("pane", pane),
                ("fabric", fabric),
            ] {
                if parts[&key].contains(slot) {
                    instance = instance.material(slot, material);
                }
            }
            builder = builder.instance(instance);
        }
        for (slot, key) in KIT_MATERIALS {
            if used.contains(slot) {
                builder = builder.material(slot, key);
            }
        }
        Ok(builder.build()?)
    }

    /// The colours of the `i`th house of a review, cycled through the
    /// palettes.
    fn colours(i: usize) -> [&'static str; 5] {
        [
            PAINTS[i % PAINTS.len()],
            JOINERY[(i * 2 + 1) % JOINERY.len()],
            PANES[0],
            FABRICS[i % FABRICS.len()],
            TRIMS[(i + 1) % TRIMS.len()],
        ]
    }

    /// Every part of the catalogue on a grid: the modules in rows by style
    /// and height, then the covers, then the add-ons.
    ///
    /// # Errors
    ///
    /// If a part does not validate, which would be a bug.
    pub fn catalogue() -> anyhow::Result<Building> {
        let mut rows: Vec<Vec<Piece>> = Vec::new();
        let mut covers = Vec::new();
        let mut addons = Vec::new();
        for piece in Piece::catalogue() {
            match piece {
                Piece::Module(m) => {
                    let row = usize::from(m.style == Style::Casement) * 3
                        + if m.door { m.storeys as usize - 1 } else { 2 };
                    if rows.len() <= row {
                        rows.resize(row + 1, Vec::new());
                    }
                    rows[row].push(piece);
                }
                Piece::Workshop(_) => rows[0].push(piece),
                Piece::Vault(..) | Piece::Lean(..) => covers.push(piece),
                _ => addons.push(piece),
            }
        }
        rows.push(covers);
        rows.push(addons);
        // Rows wrap at sixty metres, so the sheet is square enough for one
        // camera to frame.
        let mut placed = Vec::new();
        let mut z = 0.0;
        for (r, row) in rows.iter().enumerate() {
            let mut x = 0.0;
            let mut deepest = 0.0_f64;
            for (i, piece) in row.iter().enumerate() {
                if x > 60.0 {
                    x = 0.0;
                    z -= deepest + 4.0;
                    deepest = 0.0;
                }
                let (w, d) = match piece {
                    Piece::Module(m) => (m.width, m.depth),
                    Piece::Workshop(d) => (super::WORKSHOP, *d),
                    Piece::Vault(w, d) | Piece::Lean(w, d) => (*w, *d),
                    other => {
                        let [w, d] = other.footprint();
                        (w.max(2.6), d.max(2.0))
                    }
                };
                placed.push((
                    *piece,
                    Pose::at([x + w * 0.5, 0.0, z - d * 0.5]),
                    colours(r * 7 + i),
                ));
                x += w + 2.0;
                deepest = deepest.max(d);
            }
            z -= deepest + 4.0;
        }
        assemble("serein:house-catalogue", placed)
    }

    /// Sample owners: the choices a street of the Terraces shows.
    fn owners() -> Vec<TerraceHouse> {
        let base = TerraceHouse {
            width: 8.0,
            depth: 10.0,
            lower: 2,
            style: Style::Shuttered,
            workshop: false,
            upper: None,
            shed: None,
            awning: false,
            balcony: false,
            top: Top::Flat,
            roof: 0,
        };
        vec![
            TerraceHouse {
                upper: Some(Upper {
                    width: 6.0,
                    depth: 6.0,
                    shift: 0.5,
                }),
                balcony: true,
                roof: roof::TANK | roof::WASHING | roof::GARDEN,
                ..base
            },
            TerraceHouse {
                width: 6.0,
                lower: 1,
                awning: true,
                roof: roof::HUTCH | roof::DISH | roof::AC,
                ..base
            },
            TerraceHouse {
                width: 10.0,
                style: Style::Casement,
                roof: roof::SOLAR | roof::TANK | roof::AERIAL | roof::AC,
                ..base
            },
            TerraceHouse {
                style: Style::Casement,
                lower: 1,
                shed: Some(true),
                upper: Some(Upper {
                    width: 6.0,
                    depth: 4.0,
                    shift: -0.5,
                }),
                top: Top::Vault,
                roof: roof::GARDEN,
                ..base
            },
            TerraceHouse {
                width: 6.0,
                depth: 8.0,
                awning: true,
                top: Top::Lean,
                ..base
            },
            TerraceHouse {
                width: super::WORKSHOP,
                workshop: true,
                lower: 1,
                awning: true,
                roof: roof::TANK | roof::AC,
                ..base
            },
            TerraceHouse {
                width: 10.0,
                balcony: true,
                upper: Some(Upper {
                    width: 8.0,
                    depth: 6.0,
                    shift: 0.0,
                }),
                roof: roof::HUTCH | roof::SOLAR | roof::WASHING,
                ..base
            },
        ]
    }

    /// A street of attached houses with staggered fronts, as the plan lays
    /// them.
    ///
    /// # Errors
    ///
    /// If a part does not validate, which would be a bug.
    pub fn street() -> anyhow::Result<Building> {
        let mut placed = Vec::new();
        let mut x = 0.0;
        for (i, house) in owners().into_iter().enumerate() {
            let w = house.lot_width();
            #[allow(clippy::cast_precision_loss, reason = "a handful of houses")]
            let stagger = [0.0, 0.6, -0.3, 0.4, -0.5, 0.2, 0.0][i % 7] - house.depth * 0.5;
            let at = Pose::at([x + w * 0.5, 0.0, stagger]);
            for (piece, pose) in house.pieces() {
                placed.push((piece, at.compose(pose), colours(i)));
            }
            x += w;
        }
        assemble("serein:house-street", placed)
    }

    /// One sample owner's house on its own, `index` into the street's.
    ///
    /// # Errors
    ///
    /// If a part does not validate, which would be a bug.
    pub fn house(index: usize) -> anyhow::Result<Building> {
        let owners = owners();
        let house = owners[index % owners.len()];
        let turn = Pose::at([0.0; 3]).rotated(DQuat::IDENTITY);
        assemble(
            &format!("serein:house-{index}"),
            house
                .pieces()
                .into_iter()
                .map(|(piece, pose)| (piece, turn.compose(pose), colours(index))),
        )
    }

    /// A module on its own, for a close look at its dressing.
    ///
    /// # Errors
    ///
    /// If a part does not validate, which would be a bug.
    pub fn module(width: f64, depth: f64, storeys: u32, style: Style) -> anyhow::Result<Building> {
        assemble(
            "serein:house-module",
            [(
                Piece::Module(Module {
                    width,
                    depth,
                    storeys,
                    style,
                    door: true,
                }),
                Pose::default(),
                colours(0),
            )],
        )
    }
}

/// A box from its low corner.
fn cube(at: [f64; 3], size: [f64; 3]) -> Geometry {
    Geometry::cuboid(size).placed(Pose::at(at))
}

/// A box from its low corner with its arrises softened, as anything a hand
/// could touch is.
fn soft(at: [f64; 3], size: [f64; 3]) -> Geometry {
    let bevel = (size.iter().copied().fold(f64::INFINITY, f64::min) * 0.3).min(0.025);
    if bevel < 0.006 {
        return cube(at, size);
    }
    Geometry::chamfered_cuboid(size, bevel).placed(Pose::at(at))
}

/// The union of `solids`, which may be one.
fn merge(mut solids: Vec<Geometry>) -> Geometry {
    if solids.len() == 1 {
        solids.remove(0)
    } else {
        Geometry::union_all(solids)
    }
}

/// A cylinder of `radius` from `at` up by `height`.
fn post(at: [f64; 3], radius: f64, height: f64, segments: u32) -> Geometry {
    Geometry::cylinder(radius, height, segments).placed(Pose::at(at))
}

/// One face of a module, in its own coordinates: `u` across it as a person
/// facing it sees (left to right), `y` up, `d` outward from its plane. The
/// back is the front turned half round, so one routine dresses both.
#[derive(Clone, Copy)]
struct Face {
    /// The face's distance from the module's middle.
    half: f64,
    /// `1` for the front (`+z`), `-1` for the back.
    out: f64,
}

impl Face {
    fn front(depth: f64) -> Self {
        Self {
            half: depth * 0.5,
            out: 1.0,
        }
    }

    fn rear(depth: f64) -> Self {
        Self {
            half: depth * 0.5,
            out: -1.0,
        }
    }

    fn point(self, [u, y, d]: [f64; 3]) -> [f64; 3] {
        [self.out * u, y, self.out * (self.half + d)]
    }

    /// The low corner of a box centred on `u`, standing on `y`, from `d`
    /// outward `t` thick.
    fn corner(self, u: f64, y: f64, d: f64, [w, _, t]: [f64; 3]) -> [f64; 3] {
        if self.out > 0.0 {
            [u - w * 0.5, y, self.half + d]
        } else {
            [-u - w * 0.5, y, -(self.half + d + t)]
        }
    }

    fn cube(self, u: f64, y: f64, d: f64, size: [f64; 3]) -> Geometry {
        cube(self.corner(u, y, d, size), size)
    }

    fn soft(self, u: f64, y: f64, d: f64, size: [f64; 3]) -> Geometry {
        soft(self.corner(u, y, d, size), size)
    }

    /// A rectangular frame `size` round an opening, its bars `bar` wide, in
    /// joinery: four square-cut bars rather than one solid,
    /// so each is judged thin on its own and the level past 60 m drops the
    /// frame and keeps what it holds. The rails sit a hair inside the
    /// stiles' faces, so no two bars share a plane.
    fn ring(self, u: f64, y: f64, d: f64, [w, h, t]: [f64; 3], bar: f64) -> Vec<Geometry> {
        let rail = [w - 2.0 * bar + 0.01, bar, t - 0.004];
        vec![
            self.cube(u - (w - bar) * 0.5, y, d, [bar, h, t]),
            self.cube(u + (w - bar) * 0.5, y, d, [bar, h, t]),
            self.cube(u, y + 0.002, d + 0.002, rail),
            self.cube(u, y + h - bar - 0.002, d + 0.002, rail),
        ]
    }

    fn hull(self, points: impl IntoIterator<Item = [f64; 3]>) -> Geometry {
        Geometry::hull(points.into_iter().map(|p| self.point(p)))
    }

    /// A vertical pipe of `radius` at `(u, d)` from `y` up by `height`.
    fn pipe(self, u: f64, y: f64, d: f64, radius: f64, height: f64) -> Geometry {
        post(self.point([u, y, d]), radius, height, 8)
    }
}

/// What a module's openings and dressing add up to, by element.
#[derive(Default)]
struct Dress {
    /// Recesses in the walls.
    cuts: Vec<Geometry>,
    /// The same through the plinth, a centimetre lower so the plinth's top
    /// in a doorway never lies on the wall's.
    low_cuts: Vec<Geometry>,
    trim: Vec<Geometry>,
    joinery: Vec<Geometry>,
    glass: Vec<Geometry>,
    metal: Vec<Geometry>,
}

impl Dress {
    /// A window of the style with its sill at `y`.
    fn window(&mut self, f: Face, u: f64, y: f64, style: Style) {
        let (w, h, _) = style.window();
        self.opening(f, u, y, [w, h], style);
    }

    /// An opening `w` by `h` from `y`, with its surround, frame, glass and, in
    /// the shuttered style, shutters.
    fn opening(&mut self, f: Face, u: f64, y: f64, [w, h]: [f64; 2], style: Style) {
        self.cuts.push(f.cube(u, y, -0.3, [w, h, 0.6]));
        match style {
            Style::Shuttered => {
                // A sill a centimetre under the reveal's floor, and a hood
                // with a cap on the wall's face: nothing of the trim lies on
                // a face of the reveal. Shutters take the sides.
                self.trim
                    .push(f.soft(u, y - 0.09, -0.04, [w + 0.24, 0.08, 0.28]));
                self.trim.push(f.soft(u, y + h, 0.0, [w + 0.3, 0.14, 0.13]));
                self.trim
                    .push(f.soft(u, y + h + 0.14, 0.0, [w + 0.42, 0.05, 0.18]));
                // Six panes: a frame, a mullion, two glazing bars.
                self.joinery.extend(f.ring(u, y, -0.2, [w, h, 0.07], 0.06));
                self.joinery
                    .push(f.cube(u, y + 0.03, -0.19, [0.045, h - 0.06, 0.05]));
                for k in [1.0, 2.0] {
                    self.joinery
                        .push(f.cube(u, y + h * k / 3.0, -0.19, [w - 0.06, 0.035, 0.05]));
                }
                for side in [-1.0, 1.0] {
                    self.shutter(f, u + side * (w * 0.5 + 0.255), y, h);
                }
                self.glass
                    .push(f.cube(u, y + 0.02, -0.17, [w - 0.04, h - 0.04, 0.012]));
            }
            Style::Casement => {
                // A concrete box frame standing 16 cm proud, square-cut, since
                // a level past 60 m keeps it, and a shelf of a sill under it.
                self.trim.push(
                    f.cube(u, y - 0.18, 0.0, [w + 0.36, h + 0.36, 0.16])
                        .subtract(f.cube(u, y, -0.01, [w, h, 0.18])),
                );
                self.trim
                    .push(f.soft(u, y - 0.27, -0.02, [w + 0.5, 0.09, 0.38]));
                self.joinery.extend(f.ring(u, y, -0.24, [w, h, 0.08], 0.07));
                self.joinery
                    .push(f.cube(u, y + 0.035, -0.235, [0.06, h * 0.72, 0.07]));
                self.joinery
                    .push(f.cube(u, y + h * 0.72, -0.235, [w - 0.07, 0.06, 0.07]));
                self.glass
                    .push(f.cube(u, y + 0.03, -0.2, [w - 0.06, h - 0.06, 0.012]));
            }
        }
    }

    /// A louvred shutter folded back against the wall, centred on `u`: a
    /// frame, a board behind, and tilted slats, each a wedge of eight
    /// triangles, which the level past 60 m drops.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a dozen slats"
    )]
    fn shutter(&mut self, f: Face, u: f64, y: f64, h: f64) {
        self.joinery
            .extend(f.ring(u, y, 0.012, [0.46, h, 0.042], 0.055));
        self.joinery
            .push(f.cube(u, y + 0.01, 0.015, [0.44, h - 0.02, 0.012]));
        let slats = ((h - 0.13) / 0.12).floor() as u32;
        let pitch = (h - 0.13) / f64::from(slats);
        for k in 0..slats {
            let y0 = y + 0.065 + f64::from(k) * pitch;
            let (a, b) = (u - 0.18, u + 0.18);
            self.joinery.push(f.hull([
                [a, y0, 0.052],
                [b, y0, 0.052],
                [a, y0 + 0.02, 0.052],
                [b, y0 + 0.02, 0.052],
                [a, y0 + 0.085, 0.03],
                [b, y0 + 0.085, 0.03],
            ]));
        }
    }

    /// A ground-floor door at `u`, up a step: its surround, a hood or a
    /// canopy, its frame, leaf and glass.
    #[allow(clippy::too_many_lines, reason = "one door, member by member")]
    fn door(&mut self, f: Face, u: f64, style: Style) {
        let [w, h] = DOOR;
        let head = THRESHOLD + h;
        self.cuts.push(f.cube(u, THRESHOLD, -0.4, [w, h, 0.7]));
        self.low_cuts
            .push(f.cube(u, THRESHOLD - 0.01, -0.5, [w + 0.02, h, 0.8]));
        // The step, its top below the threshold's so the two never share a
        // plane.
        self.trim
            .push(f.soft(u, -0.03, 0.08, [w + 0.6, 0.17, 0.42]));
        // Jambs from the plinth, and a head.
        let (band, proud) = match style {
            Style::Shuttered => (0.18, 0.08),
            Style::Casement => (0.18, 0.16),
        };
        for side in [-1.0, 1.0] {
            self.trim.push(f.soft(
                u + side * (w + band) * 0.5,
                PLINTH,
                0.0,
                [band, head + band - PLINTH, proud],
            ));
        }
        self.trim
            .push(f.soft(u, head, 0.0, [w + 2.0 * band, band, proud]));
        // The frame in the reveal, one solid: members of a union that only
        // touch meet on faces the kernel may split into slivers.
        self.joinery
            .push(f.cube(u, THRESHOLD, -0.36, [w, h, 0.09]).subtract(f.cube(
                u,
                THRESHOLD - 0.1,
                -0.4,
                [w - 0.2, h + 0.02, 0.2],
            )));
        match style {
            Style::Shuttered => {
                // A hood on two corbels over the head.
                let top = head + band;
                self.trim.push(f.soft(u, top, -0.02, [w + 0.7, 0.12, 0.32]));
                for side in [-1.0, 1.0] {
                    let x = u + side * (w * 0.5 + 0.25);
                    self.trim.push(f.hull([
                        [x - 0.06, top, 0.0],
                        [x + 0.06, top, 0.0],
                        [x - 0.06, top, 0.26],
                        [x + 0.06, top, 0.26],
                        [x - 0.06, top - 0.34, 0.0],
                        [x + 0.06, top - 0.34, 0.0],
                    ]));
                }
                // A four-panel leaf under a transom and a fanlight.
                let leaf = h - 0.45;
                self.joinery
                    .push(f.cube(u, THRESHOLD + leaf, -0.355, [w - 0.1, 0.08, 0.08]));
                self.joinery
                    .push(f.cube(u, THRESHOLD, -0.35, [w - 0.16, leaf + 0.02, 0.05]));
                for (dx, y) in [(-0.23, 0.2), (0.23, 0.2), (-0.23, 1.05), (0.23, 1.05)] {
                    self.joinery
                        .push(f.soft(u + dx, THRESHOLD + y, -0.305, [0.34, 0.72, 0.03]));
                }
                self.joinery
                    .push(f.cube(u + 0.36, THRESHOLD + 1.0, -0.305, [0.04, 0.14, 0.05]));
                self.glass.push(f.cube(
                    u,
                    THRESHOLD + leaf + 0.08,
                    -0.33,
                    [w - 0.16, h - leaf - 0.16, 0.012],
                ));
            }
            Style::Casement => {
                // A concrete canopy on two brackets.
                let top = head + band;
                self.trim.push(f.soft(u, top, -0.02, [w + 1.2, 0.18, 1.15]));
                for side in [-1.0, 1.0] {
                    let x = u + side * (w * 0.5 + 0.35);
                    self.trim.push(f.hull([
                        [x - 0.05, top, 0.0],
                        [x + 0.05, top, 0.0],
                        [x - 0.05, top, 0.95],
                        [x + 0.05, top, 0.95],
                        [x - 0.05, top - 0.5, 0.0],
                        [x + 0.05, top - 0.5, 0.0],
                    ]));
                }
                // A flush leaf with a tall vision slot.
                let leaf = f
                    .cube(u, THRESHOLD, -0.35, [w - 0.16, h - 0.06, 0.05])
                    .subtract(f.cube(u + 0.2, THRESHOLD + 1.0, -0.4, [0.16, 1.05, 0.2]));
                self.joinery.push(leaf);
                self.joinery
                    .push(f.cube(u - 0.33, THRESHOLD + 1.0, -0.305, [0.14, 0.04, 0.05]));
                self.glass
                    .push(f.cube(u + 0.2, THRESHOLD + 1.0, -0.345, [0.16, 1.05, 0.012]));
            }
        }
    }

    /// A French window over the door, on the storey from `y`, with a rail
    /// across it: where an owner may hang a balcony.
    fn french(&mut self, f: Face, u: f64, y: f64, style: Style) {
        let (w, h) = match style {
            Style::Shuttered => (0.9, 2.3),
            Style::Casement => (1.3, 2.3),
        };
        let bottom = y + 0.2;
        match style {
            Style::Shuttered => {
                self.cuts.push(f.cube(u, bottom, -0.3, [w, h, 0.6]));
                self.trim
                    .push(f.soft(u, bottom + h, 0.0, [w + 0.3, 0.14, 0.13]));
                self.trim
                    .push(f.soft(u, bottom + h + 0.14, 0.0, [w + 0.42, 0.05, 0.18]));
                for side in [-1.0, 1.0] {
                    self.shutter(f, u + side * (w * 0.5 + 0.255), bottom, h);
                }
            }
            Style::Casement => {
                self.cuts.push(f.cube(u, bottom, -0.3, [w, h, 0.6]));
                // The box frame's sides and head: the floor is its sill.
                for side in [-1.0, 1.0] {
                    self.trim.push(f.soft(
                        u + side * (w + 0.16) * 0.5,
                        bottom - 0.02,
                        0.0,
                        [0.16, h + 0.18, 0.12],
                    ));
                }
                self.trim
                    .push(f.soft(u, bottom + h, 0.0, [w + 0.32, 0.16, 0.12]));
            }
        }
        self.joinery
            .extend(f.ring(u, bottom, -0.22, [w, h, 0.08], 0.07));
        self.joinery
            .push(f.cube(u, bottom + 0.035, -0.215, [0.06, h - 0.07, 0.07]));
        self.joinery
            .push(f.cube(u, bottom + h - 0.5, -0.215, [w - 0.07, 0.06, 0.07]));
        self.glass
            .push(f.cube(u, bottom + 0.03, -0.19, [w - 0.06, h - 0.06, 0.012]));
        // A rail across the opening: two rails and bars.
        let rail = w + 0.2;
        for ry in [bottom + 0.1, bottom + 0.95] {
            self.joinery.push(f.cube(u, ry, 0.02, [rail, 0.04, 0.04]));
        }
        let bars = 7;
        for k in 0..=bars {
            let x = u - rail * 0.5 + 0.03 + (rail - 0.06) * f64::from(k) / f64::from(bars);
            self.joinery
                .push(f.cube(x, bottom + 0.12, 0.029, [0.022, 0.85, 0.022]));
        }
    }

    /// A downpipe at `u` from the plinth to a hopper under a spout through
    /// the parapet, with its brackets.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few brackets"
    )]
    fn downpipe(&mut self, f: Face, u: f64, roof: f64) {
        self.joinery
            .push(f.pipe(u, PLINTH, 0.085, 0.055, roof + 0.05 - PLINTH));
        self.joinery
            .push(f.soft(u, roof + 0.02, 0.0, [0.26, 0.3, 0.24]));
        self.joinery
            .push(f.cube(u, roof + 0.2, -0.1, [0.1, 0.08, 0.24]));
        let brackets = ((roof - PLINTH) / 1.6).floor() as u32;
        for k in 1..=brackets {
            self.joinery.push(f.cube(
                u,
                PLINTH + f64::from(k) * 1.6 - 0.8,
                -0.02,
                [0.15, 0.05, 0.12],
            ));
        }
    }

    /// The trim every module wears: a plinth, a string course at each floor,
    /// a two-step cornice at the roof, pilasters at the front and back
    /// corners, the parapet's coping and the roof's membrane. Each stands a
    /// little prouder than a builder would make it, so it still reads under
    /// flat light and from across a terrace.
    fn shell(&mut self, width: f64, depth: f64, roof: f64, storeys: u32, style: Style) {
        let w = width - 0.02;
        let (course, proud) = style.course();
        for f in [Face::front(depth), Face::rear(depth)] {
            let plinth = f.soft(0.0, -0.03, -0.3, [w, PLINTH + 0.03, 0.38]);
            self.trim.push(if self.low_cuts.is_empty() {
                plinth
            } else {
                plinth.subtract(merge(self.low_cuts.clone()))
            });
            for k in 1..storeys {
                let y = f64::from(k) * STOREY;
                self.trim
                    .push(f.cube(0.0, y - course * 0.5, -0.25, [w, course, 0.25 + proud]));
            }
            self.trim
                .push(f.cube(0.0, roof - 0.4, -0.25, [w, 0.16, 0.39]));
            self.trim
                .push(f.soft(0.0, roof - 0.24, -0.25, [w, 0.22, 0.59]));
            for side in [-1.0, 1.0] {
                self.trim.push(f.cube(
                    side * (width * 0.5 - 0.24),
                    PLINTH,
                    -0.36,
                    [0.46, roof - 0.4 - PLINTH, 0.44],
                ));
            }
        }
        let top = roof + PARAPET;
        self.trim.push(
            soft(
                [-w * 0.5, top, -depth * 0.5 - 0.08],
                [w, COPING, depth + 0.16],
            )
            .subtract(cube(
                [
                    -width * 0.5 + INSET + 0.06,
                    top - 0.1,
                    -depth * 0.5 + INSET + 0.06,
                ],
                [
                    width - 2.0 * INSET - 0.12,
                    COPING + 0.2,
                    depth - 2.0 * INSET - 0.12,
                ],
            )),
        );
        self.trim.push(cube(
            [-width * 0.5 + INSET, roof, -depth * 0.5 + INSET],
            [width - 2.0 * INSET, 0.04, depth - 2.0 * INSET],
        ));
    }

    /// Panels sunk into both sides between the corner pilasters, the plinth,
    /// each floor and the cornice: the side of a house at the end of a row
    /// reads as its front does, and a neighbour against it covers them.
    fn sides(&mut self, width: f64, depth: f64, roof: f64, storeys: u32, style: Style) {
        let (course, _) = style.course();
        for storey in 0..storeys {
            let y0 = if storey == 0 {
                PLINTH
            } else {
                f64::from(storey) * STOREY + course * 0.5
            };
            let y1 = if storey + 1 == storeys {
                roof - 0.4
            } else {
                f64::from(storey + 1) * STOREY - course * 0.5
            };
            for side in [-1.0, 1.0] {
                let x = side * width * 0.5 - if side > 0.0 { 0.04 } else { 0.06 };
                self.cuts.push(cube(
                    [x, y0, -depth * 0.5 + 0.42],
                    [0.1, y1 - y0, depth - 0.84],
                ));
            }
        }
    }

    /// The elements: the solid `body` with its recesses, and the dressing.
    fn elements(self, body: Geometry) -> Vec<Element> {
        let walls = if self.cuts.is_empty() {
            body
        } else {
            body.subtract(merge(self.cuts))
        };
        let mut elements = vec![
            Element::new("walls", walls, "paint")
                .uv(UvMode::Box)
                .collision(Collision::Bounds),
        ];
        for (id, solids, slot) in [
            ("trim", self.trim, "trim"),
            ("joinery", self.joinery, "joinery"),
            ("glass", self.glass, "pane"),
            ("metal", self.metal, "metal"),
        ] {
            if !solids.is_empty() {
                // Glass box-mapped, so each pane shows its own part of the
                // room behind (see `serein:pane`).
                let uv = if slot == "pane" {
                    UvMode::Box
                } else {
                    UvMode::Planar
                };
                elements.push(Element::new(id, merge(solids), slot).uv(uv));
            }
        }
        elements
    }
}

/// A module's solid: a block to the parapet's top with the roof's well cut
/// from it.
fn block(width: f64, depth: f64, roof: f64) -> Geometry {
    Geometry::chamfered_cuboid([width, roof + PARAPET, depth], 0.05)
        .placed(Pose::at([-width * 0.5, 0.0, -depth * 0.5]))
        .subtract(cube(
            [-width * 0.5 + INSET, roof, -depth * 0.5 + INSET],
            [width - 2.0 * INSET, PARAPET + 1.0, depth - 2.0 * INSET],
        ))
}

/// Only the downhill face is habitable; the buried back retains the bank.
fn bank(width: f64, depth: f64, rise: f64) -> Vec<Element> {
    let style = Style::Casement;
    let mut dress = Dress::default();
    let face = Face::front(depth);
    let bays = style.bays(width, true);
    for (i, &u) in bays.iter().enumerate() {
        if i == style.door_bay(bays.len(), true) {
            dress.door(face, u, style);
        } else {
            dress.window(face, u, 0.95, style);
        }
    }
    dress.downpipe(face, width * 0.5 - 0.58, rise);
    // The terrace deck stands proud of the upper ground, and the upper
    // house stands on it. No scaled doors or stretched window openings.
    dress.elements(cube(
        [-width * 0.5, 0.0, -depth * 0.5],
        [width, rise + 0.06, depth],
    ))
}

fn module(m: Module) -> Vec<Element> {
    let (w, d) = (m.width, m.depth);
    let roof = f64::from(m.storeys) * STOREY;
    let (_, _, sill) = m.style.window();
    let bays = m.bays();
    let mut dress = Dress::default();
    for f in [Face::front(d), Face::rear(d)] {
        let front = f.out > 0.0;
        let door = m.door.then(|| m.style.door_bay(bays.len(), front));
        for storey in 0..m.storeys {
            let y = f64::from(storey) * STOREY;
            for (i, &u) in bays.iter().enumerate() {
                match (storey, Some(i) == door) {
                    (0, true) => dress.door(f, u, m.style),
                    (1, true) if front => dress.french(f, u, y, m.style),
                    _ => dress.window(f, u, y + sill, m.style),
                }
            }
        }
        dress.downpipe(f, w * 0.5 - 0.58, roof);
    }
    dress.shell(w, d, roof, m.storeys, m.style);
    dress.sides(w, d, roof, m.storeys, m.style);
    dress.elements(block(w, d, roof))
}

/// A workshop: one tall storey, a roller door and a person's door on the
/// front under high windows, high windows at the back.
fn workshop(depth: f64) -> Vec<Element> {
    let w = WORKSHOP;
    let roof = WORKSHOP_HEIGHT;
    let style = Style::Casement;
    let bays = style.bays(w, true);
    let mut dress = Dress::default();
    let (front, rear) = (Face::front(depth), Face::rear(depth));
    // The roller door across the first two bays.
    let u = f64::midpoint(bays[0], bays[1]);
    let (rw, rh) = (4.0, 3.9);
    dress.cuts.push(front.cube(u, 0.03, -0.3, [rw, rh, 0.6]));
    dress
        .low_cuts
        .push(front.cube(u, 0.015, -0.4, [rw + 0.02, rh, 0.7]));
    dress.metal.push(front.cube(u, 0.03, -0.28, [rw, rh, 0.04]));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a curtain's ribs"
    )]
    let ribs = (rh / 0.16).floor() as u32;
    for k in 1..ribs {
        dress
            .metal
            .push(front.cube(u, 0.03 + f64::from(k) * 0.16, -0.245, [rw, 0.035, 0.025]));
    }
    dress
        .metal
        .push(front.soft(u, 0.03 + rh + 0.02, -0.02, [rw + 0.4, 0.5, 0.44]));
    for side in [-1.0, 1.0] {
        dress.metal.push(front.cube(
            u + side * (rw * 0.5 + 0.06),
            0.03,
            0.005,
            [0.12, rh + 0.02, 0.12],
        ));
    }
    // A person's door beside it, a window, and high windows over both.
    dress.door(front, bays[2], style);
    dress.window(front, bays[3], 0.95, style);
    for &b in &bays[2..] {
        dress.opening(front, b, 3.75, [1.6, 1.05], style);
    }
    for &b in &bays {
        dress.opening(rear, b, 3.75, [1.6, 1.05], style);
    }
    dress.window(rear, bays[1], 0.95, style);
    for f in [front, rear] {
        dress.downpipe(f, w * 0.5 - 0.58, roof);
    }
    dress.shell(w, depth, roof, 1, style);
    dress.sides(w, depth, roof, 1, style);
    dress.elements(block(w, depth, roof))
}

/// Turns an `extrude` profile drawn across the depth (profile `x` to `z`,
/// profile `z` to `y`) so it runs along `x`.
fn across_depth() -> DQuat {
    DQuat::from_mat3(&DMat3::from_cols(DVec3::Z, DVec3::X, DVec3::Y))
}

/// A shallow barrel vault of sheet across a module `w` by `d`, standing on
/// its parapet's coping (the origin), with a painted gable at each end.
#[allow(clippy::cast_precision_loss, reason = "a dozen steps")]
fn vault(w: f64, d: f64) -> Vec<Element> {
    const STEPS: u32 = 12;
    let rise = 0.6 + 0.06 * d;
    let arc = |half: f64, lift: f64| {
        (0..=STEPS).map(move |i| {
            let t = f64::from(i) / f64::from(STEPS) * 2.0 - 1.0;
            [t * half, rise * (1.0 - t * t) + lift]
        })
    };
    let half = d * 0.5 + 0.22;
    let profile: Vec<[f64; 2]> = arc(half, 0.0).chain(arc(half, 0.08).rev()).collect();
    let sheet = Geometry::extrude(profile, w + 0.3)
        .placed(Pose::at([-w * 0.5 - 0.15, 0.0, 0.0]).rotated(across_depth()));
    // The gables under the arc, their top a little under the sheet's.
    let inner = d * 0.5 - 0.06;
    let gable: Vec<[f64; 2]> = (0..=STEPS)
        .map(|i| {
            let t = f64::from(i) / f64::from(STEPS) * 2.0 - 1.0;
            let z = t * inner;
            [z, rise * (1.0 - (z / half).powi(2)) - 0.03]
        })
        .rev()
        .chain([[-inner, -0.12], [inner, -0.12]])
        .collect();
    let gables = [-1.0, 1.0].map(|side: f64| {
        let x = side * (w * 0.5 - 0.03) - if side > 0.0 { 0.22 } else { 0.0 };
        Geometry::extrude(gable.clone(), 0.22)
            .placed(Pose::at([x, 0.0, 0.0]).rotated(across_depth()))
    });
    vec![
        Element::new("sheet", sheet, "sheet"),
        Element::new("gables", merge(gables.to_vec()), "paint").uv(UvMode::Box),
    ]
}

/// A lean-to of sheet over a module `w` by `d`, standing on its coping (the
/// origin), high at the back and falling to an overhang at the front, walled
/// in under its pitch.
fn lean(w: f64, d: f64) -> Vec<Element> {
    let (x0, x1) = (-w * 0.5 - 0.12, w * 0.5 + 0.12);
    let (zb, zf) = (-d * 0.5 - 0.15, d * 0.5 + 0.4);
    let (yb, yf) = (1.2, 0.2);
    let under = |z: f64| yb + (yf - yb) * (z - zb) / (zf - zb);
    let sheet = Geometry::hull([
        [x0, yb, zb],
        [x1, yb, zb],
        [x0, yf, zf],
        [x1, yf, zf],
        [x0, yb + 0.08, zb],
        [x1, yb + 0.08, zb],
        [x0, yf + 0.08, zf],
        [x1, yf + 0.08, zf],
    ]);
    // Inside the coping's ends, which stop a centimetre short of the
    // module's sides.
    let (wx0, wx1) = (-w * 0.5 + 0.03, w * 0.5 - 0.03);
    let (wz0, wz1) = (-d * 0.5 + 0.03, d * 0.5 - 0.03);
    let wall = Geometry::hull([
        [wx0, -0.12, wz0],
        [wx1, -0.12, wz0],
        [wx0, -0.12, wz1],
        [wx1, -0.12, wz1],
        [wx0, under(wz0) - 0.02, wz0],
        [wx1, under(wz0) - 0.02, wz0],
        [wx0, under(wz1) - 0.02, wz1],
        [wx1, under(wz1) - 0.02, wz1],
    ]);
    // A fascia along the front edge.
    let fascia = cube([x0, yf - 0.14, zf - 0.04], [x1 - x0, 0.2, 0.05]);
    vec![
        Element::new("sheet", sheet.union(fascia), "sheet"),
        Element::new("walls", wall, "paint").uv(UvMode::Box),
    ]
}

/// A balcony on two corbels, its origin at the wall's face at its floor, its
/// railing painted like the joinery.
#[allow(clippy::cast_precision_loss, reason = "a few dozen bars")]
fn balcony() -> Vec<Element> {
    let (w, out) = (2.4, 1.1);
    let mut slab = vec![soft([-w * 0.5, -0.2, -0.02], [w, 0.2, out + 0.02])];
    for x in [-0.85, 0.85] {
        slab.push(Geometry::hull([
            [x - 0.07, -0.2, 0.0],
            [x + 0.07, -0.2, 0.0],
            [x - 0.07, -0.2, out - 0.2],
            [x + 0.07, -0.2, out - 0.2],
            [x - 0.07, -0.75, 0.0],
            [x + 0.07, -0.75, 0.0],
        ]));
    }
    let mut rail = Vec::new();
    let (x0, x1, z1) = (-w * 0.5 + 0.03, w * 0.5 - 0.03, out - 0.03);
    for [x, z] in [[x0, z1], [x1, z1], [x0, 0.05], [x1, 0.05]] {
        rail.push(soft([x - 0.03, 0.0, z - 0.03], [0.06, 1.05, 0.06]));
    }
    for y in [0.1, 1.0] {
        rail.push(cube([x0, y, z1 - 0.025], [x1 - x0, 0.05, 0.05]));
        for x in [x0, x1] {
            rail.push(cube([x - 0.025, y, 0.05], [0.05, 0.05, z1 - 0.05]));
        }
    }
    let bars = 18;
    for k in 1..bars {
        let x = x0 + (x1 - x0) * f64::from(k) / f64::from(bars);
        rail.push(cube([x - 0.011, 0.15, z1 - 0.011], [0.022, 0.85, 0.022]));
    }
    for k in 1..8 {
        let z = 0.05 + (z1 - 0.05) * f64::from(k) / 8.0;
        for x in [x0, x1] {
            rail.push(cube([x - 0.011, 0.15, z - 0.011], [0.022, 0.85, 0.022]));
        }
    }
    vec![
        Element::new("slab", merge(slab), "trim"),
        Element::new("railing", merge(rail), "joinery"),
    ]
}

/// An awning `w` wide, its origin at the wall's face at its top edge: cloth
/// on a sloping frame with a valance, on two brackets.
fn awning(w: f64) -> Vec<Element> {
    let (out, drop) = (1.3, 0.38);
    let plate = |x0: f64, x1: f64, t: f64| {
        Geometry::hull([
            [x0, 0.0, 0.02],
            [x1, 0.0, 0.02],
            [x0, -drop, out],
            [x1, -drop, out],
            [x0, t, 0.02],
            [x1, t, 0.02],
            [x0, t - drop, out],
            [x1, t - drop, out],
        ])
    };
    let cloth = plate(-w * 0.5, w * 0.5, 0.04)
        .union(cube([-w * 0.5, -drop - 0.24, out - 0.02], [w, 0.26, 0.025]));
    let mut frame = vec![cube([-w * 0.5 - 0.02, -0.06, 0.0], [w + 0.04, 0.08, 0.06])];
    for x in [-w * 0.5 + 0.12, w * 0.5 - 0.12] {
        frame.push(Geometry::hull([
            [x - 0.02, -0.75, 0.0],
            [x + 0.02, -0.75, 0.0],
            [x - 0.02, -0.7, 0.0],
            [x + 0.02, -0.7, 0.0],
            [x - 0.02, -drop - 0.02, out - 0.05],
            [x + 0.02, -drop - 0.02, out - 0.05],
            [x - 0.02, -drop - 0.06, out - 0.05],
            [x + 0.02, -drop - 0.06, out - 0.05],
        ]));
    }
    vec![
        Element::new("cloth", cloth, "fabric"),
        Element::new("brackets", merge(frame), "metal"),
    ]
}

/// A corrugated shed, its origin at the middle of its footprint: a lean-to
/// roof falling to the front over a plank door.
fn shed() -> Vec<Element> {
    let (w, d) = (2.36, 3.4);
    let (x0, x1, z0, z1) = (-w * 0.5, w * 0.5, -d * 0.5, d * 0.5);
    let body = Geometry::hull([
        [x0, 0.0, z0],
        [x1, 0.0, z0],
        [x0, 0.0, z1],
        [x1, 0.0, z1],
        [x0, 2.45, z0],
        [x1, 2.45, z0],
        [x0, 2.1, z1],
        [x1, 2.1, z1],
    ]);
    let roof = Geometry::hull([
        [x0 - 0.08, 2.45, z0 - 0.12],
        [x1 + 0.08, 2.45, z0 - 0.12],
        [x0 - 0.08, 2.08, z1 + 0.25],
        [x1 + 0.08, 2.08, z1 + 0.25],
        [x0 - 0.08, 2.52, z0 - 0.12],
        [x1 + 0.08, 2.52, z0 - 0.12],
        [x0 - 0.08, 2.15, z1 + 0.25],
        [x1 + 0.08, 2.15, z1 + 0.25],
    ]);
    let mut door = vec![soft([-0.45, 0.05, z1], [0.9, 1.85, 0.04])];
    for y in [0.35, 1.5] {
        door.push(cube([-0.42, y, z1 + 0.04], [0.84, 0.1, 0.025]));
    }
    door.push(Geometry::hull([
        [-0.4, 0.45, z1 + 0.04],
        [-0.3, 0.45, z1 + 0.04],
        [0.3, 1.5, z1 + 0.04],
        [0.4, 1.5, z1 + 0.04],
        [-0.4, 0.45, z1 + 0.065],
        [0.4, 1.5, z1 + 0.065],
    ]));
    vec![
        Element::new("body", body.union(roof), "sheet").collision(Collision::Bounds),
        Element::new("door", merge(door), "joinery"),
    ]
}

/// A water tank on a steel stand, its origin at the stand's feet.
fn tank() -> Vec<Element> {
    let mut stand = Vec::new();
    for x in [-0.55, 0.55] {
        for z in [-0.55, 0.55] {
            stand.push(cube([x - 0.04, 0.0, z - 0.04], [0.08, 1.0, 0.08]));
        }
        stand.push(Geometry::hull([
            [x - 0.02, 0.1, -0.55],
            [x + 0.02, 0.1, -0.55],
            [x - 0.02, 0.14, -0.55],
            [x - 0.02, 0.9, 0.55],
            [x + 0.02, 0.9, 0.55],
            [x - 0.02, 0.94, 0.55],
        ]));
    }
    stand.push(
        cube([-0.62, 1.0, -0.62], [1.24, 0.08, 1.24])
            .subtract(cube([-0.52, 0.9, -0.52], [1.04, 0.3, 1.04])),
    );
    let body = post([0.0, 1.08, 0.0], 0.6, 1.25, 16)
        .union(post([0.0, 1.2, 0.0], 0.62, 0.05, 16))
        .union(post([0.0, 1.9, 0.0], 0.62, 0.05, 16))
        .union(post([0.0, 2.33, 0.0], 0.28, 0.08, 12));
    vec![
        Element::new("tank", body, "sheet"),
        Element::new("stand", merge(stand), "metal"),
    ]
}

/// An air conditioner on two brackets, its origin at the wall's face under
/// it: a box with a fan grille and louvres at the side.
fn ac() -> Vec<Element> {
    let (w, h, z0, z1) = (0.85, 0.6, 0.1, 0.42);
    let fan = Geometry::cylinder(0.22, 0.2, 16).placed(
        Pose::at([-0.12, h * 0.5, z1 - 0.03])
            .rotated(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
    );
    let mut parts = vec![soft([-w * 0.5, 0.0, z0], [w, h, z1 - z0]).subtract(fan)];
    for k in 0..4 {
        parts.push(cube(
            [-0.34, h * 0.5 - 0.15 + f64::from(k) * 0.1, z1 - 0.03],
            [0.44, 0.015, 0.02],
        ));
    }
    for k in 0..5 {
        parts.push(cube(
            [0.18, 0.1 + f64::from(k) * 0.09, z1],
            [0.18, 0.025, 0.015],
        ));
    }
    for x in [-0.3, 0.3] {
        parts.push(cube([x - 0.02, -0.04, 0.0], [0.04, 0.04, z1 + 0.02]));
        parts.push(cube([x - 0.02, -0.04, 0.0], [0.04, 0.35, 0.04]));
    }
    vec![Element::new("unit", merge(parts), "appliance")]
}

/// A satellite dish on a short mast, facing `+z`, its origin at its foot.
#[allow(clippy::cast_precision_loss, reason = "a few rings")]
fn dish() -> Vec<Element> {
    // A shallow bowl, turned up and to the front.
    let mut profile = vec![[0.0, 0.0]];
    for i in 1..=6 {
        let r = 0.42 * f64::from(i) / 6.0;
        profile.push([r, 0.35 * r * r]);
    }
    for i in (0..=6).rev() {
        let r = 0.42 * f64::from(i) / 6.0;
        profile.push([r, 0.35 * r * r + 0.025]);
    }
    profile.dedup();
    let bowl = Geometry::revolve(profile, 16)
        .placed(Pose::at([0.0, 1.05, 0.05]).rotated(DQuat::from_rotation_x(1.1)));
    let mast = post([0.0, 0.0, 0.0], 0.035, 1.05, 8)
        .union(cube([-0.2, 0.0, -0.2], [0.4, 0.05, 0.4]))
        .union(Geometry::hull([
            [-0.012, 1.05, 0.05],
            [0.012, 1.05, 0.05],
            [0.0, 1.07, 0.05],
            [-0.012, 1.22, 0.45],
            [0.012, 1.22, 0.45],
            [0.0, 1.24, 0.45],
        ]));
    vec![Element::new("dish", bowl.union(mast), "appliance")]
}

/// Two solar panels on a tilted frame, facing `+z`, from its feet.
fn solar() -> Vec<Element> {
    let tilt = |x0: f64, x1: f64, lift: f64, t: f64| {
        Geometry::hull([
            [x0, 0.35 + lift, 0.5],
            [x1, 0.35 + lift, 0.5],
            [x0, 0.9 + lift, -0.45],
            [x1, 0.9 + lift, -0.45],
            [x0, 0.35 + lift + t, 0.5],
            [x1, 0.35 + lift + t, 0.5],
            [x0, 0.9 + lift + t, -0.45],
            [x1, 0.9 + lift + t, -0.45],
        ])
    };
    let panels = tilt(-1.05, -0.02, 0.04, 0.04).union(tilt(0.02, 1.05, 0.04, 0.04));
    let mut frame = vec![tilt(-1.1, 1.1, 0.0, 0.04)];
    for x in [-0.95, 0.95] {
        frame.push(cube([x - 0.03, 0.0, 0.42], [0.06, 0.38, 0.06]));
        frame.push(cube([x - 0.03, 0.0, -0.5], [0.06, 0.92, 0.06]));
        frame.push(cube([x - 0.03, 0.0, -0.5], [0.06, 0.05, 1.0]));
    }
    vec![
        Element::new("panels", panels, "solar"),
        Element::new("frame", merge(frame), "metal"),
    ]
}

/// Washing on a line between two posts, from the middle of its feet.
fn washing() -> Vec<Element> {
    let (x0, x1, line) = (-1.75, 1.75, 1.7);
    let mut posts = Vec::new();
    for x in [x0, x1] {
        posts.push(cube([x - 0.03, 0.0, -0.03], [0.06, line + 0.05, 0.06]));
        posts.push(cube([x - 0.15, 0.0, -0.15], [0.3, 0.06, 0.3]));
    }
    posts.push(cube([x0, line, -0.01], [x1 - x0, 0.02, 0.02]));
    let mut cloth = Vec::new();
    let mut x = x0 + 0.3;
    let mut k = 0_u32;
    while x + 0.5 < x1 {
        let (w, h) = (
            0.42 + 0.22 * f64::from(k % 3),
            0.45 + 0.28 * f64::from((k * 7) % 3),
        );
        cloth.push(cube([x, line - h, -0.012], [w, h, 0.02]));
        x += w + 0.16;
        k += 1;
    }
    vec![
        Element::new("posts", merge(posts), "metal"),
        Element::new("cloth", merge(cloth), "fabric"),
    ]
}

/// A stair hutch, from the middle of its footprint, its door to `+z` onto a
/// clear strip in front.
fn hutch() -> Vec<Element> {
    let (w, h, d) = (2.1, 2.5, 2.1);
    let z1 = -0.45 + d * 0.5;
    let body = Geometry::chamfered_cuboid([w, h, d], 0.04)
        .placed(Pose::at([-w * 0.5, 0.0, -0.45 - d * 0.5]))
        .union(soft(
            [-w * 0.5 - 0.1, h, -0.45 - d * 0.5 - 0.1],
            [w + 0.2, 0.12, d + 0.2],
        ))
        .subtract(cube([-0.45, 0.12, z1 - 0.12], [0.9, 2.0, 0.3]));
    let mut door = vec![cube([-0.43, 0.12, z1 - 0.1], [0.86, 1.98, 0.05])];
    for y in [0.3, 1.2] {
        door.push(soft([-0.33, 0.12 + y, z1 - 0.05], [0.66, 0.7, 0.02]));
    }
    vec![
        Element::new("walls", body, "paint").uv(UvMode::Box),
        Element::new("door", merge(door), "joinery"),
    ]
}

/// An aerial on a mast with two crossbars and a foot.
fn aerial() -> Vec<Element> {
    let geometry = Geometry::union_all([
        cube([-0.2, 0.0, -0.2], [0.4, 0.05, 0.4]),
        post([0.0, 0.0, 0.0], 0.03, 3.2, 8),
        cube([-0.6, 2.55, -0.02], [1.2, 0.04, 0.04]),
        cube([-0.42, 2.95, -0.02], [0.84, 0.04, 0.04]),
        cube([-0.02, 2.75, -0.35], [0.04, 0.04, 0.7]),
    ]);
    vec![Element::new("mast", geometry, "metal")]
}

/// Two concrete planters along a parapet, from the middle of their feet,
/// with bushes in them.
fn garden() -> Vec<Element> {
    let mut boxes = Vec::new();
    let mut bushes = Vec::new();
    for x in [-0.8, 0.8] {
        boxes.push(
            soft([x - 0.72, 0.0, -0.3], [1.44, 0.5, 0.6])
                .subtract(cube([x - 0.64, 0.4, -0.22], [1.28, 0.3, 0.44])),
        );
        bushes.push(cube([x - 0.64, 0.4, -0.22], [1.28, 0.06, 0.44]));
        for (dx, r) in [(-0.35, 0.32), (0.1, 0.36), (0.45, 0.28)] {
            bushes.push(Geometry::ball(r, 5).placed(Pose::at([x + dx, 0.55 + r * 0.5, 0.0])));
        }
    }
    vec![
        Element::new("boxes", merge(boxes), "trim"),
        Element::new("plants", merge(bushes), "garden"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_of_the_catalogue_validates() {
        let catalogue = Piece::catalogue();
        let mut keys = std::collections::BTreeSet::new();
        for piece in &catalogue {
            let key = piece.key();
            piece
                .part(&key)
                .unwrap_or_else(|e| panic!("{piece:?}: {e}"));
            assert!(keys.insert(key), "{piece:?} shares a key");
        }
        assert!(catalogue.len() < 120, "{} parts", catalogue.len());
    }

    /// Every part meshes, element by element, and reports what it costs.
    #[test]
    #[ignore = "meshes the whole catalogue; run when changing it"]
    fn every_part_of_the_catalogue_meshes() {
        use ashlar::Building;
        use ashlar_manifold::{ManifoldMesher, mesh_building};
        let mut failures = Vec::new();
        let mut total = 0;
        let level = std::env::var("HOUSE_LEVEL")
            .ok()
            .and_then(|l| l.parse::<usize>().ok())
            .unwrap_or(0);
        let policy = &ashlar::LodPolicy::ladder()[level];
        for piece in Piece::catalogue() {
            let key = piece.key();
            let Some(part) = piece.part(&key).expect("validates").simplified(policy) else {
                continue;
            };
            for element in &part.elements {
                let single = Part::builder("one")
                    .element(element.clone())
                    .build()
                    .expect("validates");
                let building = Building::builder("one")
                    .part(single)
                    .material(element.material_slot.clone(), "x")
                    .instance(ashlar::Instance::new("one", "one"))
                    .build()
                    .expect("validates");
                match mesh_building(&building, &ManifoldMesher::default()) {
                    Ok(meshed) => {
                        let triangles: usize =
                            meshed.pieces().map(|p| p.mesh.indices.len() / 3).sum();
                        total += triangles;
                        println!("{key} {}: {triangles}", element.id);
                    }
                    Err(e) => failures.push(format!("{key} {}: {e}", element.id)),
                }
            }
        }
        println!("{total} triangles in all");
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// No part lays two of its surfaces on one plane where they show, with
    /// every slot bound to its own material, as the content step's check
    /// over the whole city would find.
    #[test]
    #[ignore = "meshes the whole catalogue; the content step checks the city"]
    fn no_part_of_the_catalogue_z_fights_itself() {
        use ashlar::Building;
        use ashlar_manifold::{ManifoldMesher, mesh_building};
        let mut fights = Vec::new();
        for piece in Piece::catalogue() {
            let key = piece.key();
            let part = piece.part(&key).expect("validates");
            let mut builder = Building::builder("one").part(part.clone());
            for element in &part.elements {
                builder = builder.material(
                    element.material_slot.clone(),
                    format!("own:{}", element.material_slot),
                );
            }
            let building = builder
                .instance(ashlar::Instance::new("one", key.clone()))
                .build()
                .expect("validates");
            let meshed = mesh_building(&building, &ManifoldMesher::default()).expect("meshes");
            for overlap in meshed.coplanar_overlaps() {
                if !overlap.covered && !overlap.same_binding() && overlap.normal.y > -0.9 {
                    fights.push(format!(
                        "{key}: {} / {} {:.3} m² at {:.2?} facing {:.1?}",
                        overlap.first,
                        overlap.second,
                        overlap.area,
                        overlap.point.to_array(),
                        overlap.normal.to_array()
                    ));
                }
            }
        }
        assert!(fights.is_empty(), "{fights:#?}");
    }

    /// Nor do a house's pieces where they meet, on the review street.
    #[test]
    #[ignore = "meshes a street of houses"]
    fn the_review_street_does_not_z_fight() {
        use ashlar_manifold::{ManifoldMesher, mesh_building};
        let street = review::street().expect("builds");
        let meshed = mesh_building(&street, &ManifoldMesher::default()).expect("meshes");
        let fights: Vec<String> = meshed
            .coplanar_overlaps()
            .into_iter()
            .filter(|o| !o.covered && !o.same_binding() && o.normal.y > -0.9)
            .map(|o| {
                format!(
                    "{} / {} {:.3} m² at {:.2?} facing {:.1?}",
                    o.first,
                    o.second,
                    o.area,
                    o.point.to_array(),
                    o.normal.to_array()
                )
            })
            .collect();
        assert!(fights.is_empty(), "{fights:#?}");
    }

    #[test]
    fn a_house_is_a_few_pieces_of_the_catalogue() {
        let house = TerraceHouse {
            width: 10.0,
            depth: 10.0,
            lower: 2,
            style: Style::Shuttered,
            workshop: false,
            upper: Some(Upper {
                width: 6.0,
                depth: 6.0,
                shift: 1.0,
            }),
            shed: Some(true),
            awning: true,
            balcony: true,
            top: Top::Flat,
            roof: 0xff,
        };
        let catalogue: Vec<String> = Piece::catalogue().iter().map(Piece::key).collect();
        let pieces = house.pieces();
        for (piece, _) in &pieces {
            assert!(catalogue.contains(&piece.key()), "{piece:?}");
        }
        assert!((5..=14).contains(&pieces.len()), "{pieces:?}");
        assert!((house.lot_width() - 12.5).abs() < 1e-9);
    }
}
