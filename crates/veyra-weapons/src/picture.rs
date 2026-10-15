//! Side-on pictures of a weapon, cut into its chassis and its mounts.
//!
//! The interface draws a weapon as a stack of flat pieces it can tint and
//! pick, and an inventory slot wants the same pieces as icons. Drawing them
//! by hand would drift from the model the first time a recipe changed, so
//! they are rasterised here from the triangles the mesher already returns
//! for [`crate::build`]: no kernel, no image crate and no Bevy, only
//! arithmetic over an [`ashlar::MeshedBuilding`], which keeps this crate in
//! the workspace's Bevy-free row. Whoever holds the kernel meshes the
//! assemblies of [`plan`] and hands them to [`render`]; writing files is
//! theirs too.
//!
//! A picture has two channels, luminance then alpha. Drawn with its
//! luminance a piece is a small technical drawing; drawn from its alpha
//! alone it is the flat silhouette, so a screen can choose either without a
//! rebake. Pieces carry no finish and no maker colour for the same reason:
//! the interface tints them by state.
//!
//! A mount's piece is drawn behind the chassis: what the body hides of a
//! part (a tenon inside the receiver, a grip's top under a side panel) is
//! not in its picture, so the pieces laid over the chassis give the weapon
//! and not its parts seen through each other. A part the body hides whole
//! is marked [`Piece::hidden`], and one the eye cannot tell from another of
//! its mount is marked [`Piece::same_as`]: this side is not every part's
//! best, and a screen must know which pictures say nothing.
use std::collections::BTreeMap;

use ashlar::glam::{DQuat, DVec3};
use ashlar::{ElementMesh, MeshedBuilding, Pose};
use serde::{Deserialize, Serialize};

use crate::{Assembly, Finish, Slot, Weapon, modules};

/// The look of every picture: tuning values, not derived ones.
///
/// These are meant to be changed by a person after looking at the contact
/// sheets (ticket 0008, step 3), and nothing else in this module holds a
/// tone or a weight. The whole table travels in [`Settings`], so a cache
/// keyed on the settings is stale the moment a value here moves.
pub const TUNING: Tuning = Tuning {
    // Tones are what a surface square to the view comes out as, 0 black to
    // 1 white. They only need to separate the materials from each other:
    // the interface multiplies its own colour over the result.
    shell: 0.70,
    metal: 0.48,
    grip: 0.28,
    accent: 0.88,
    ceramic: 0.80,
    lens: 0.18,
    light: 1.0,
    other: 0.60,
    // The facing term. A surface square to the view gets floor plus square;
    // one seen edge on gets the floor alone, which is what turns a barrel
    // round and draws a bevel as a line.
    facing_floor: 0.35,
    facing_square: 0.65,
    // Added for a surface facing up and taken away from one facing down, so
    // the top and the underside of a round part do not shade alike.
    facing_above: 0.15,
    // Clear margin round the envelope, as a share of the frame's long side,
    // so no piece's soft edge is cut by the frame.
    padding: 0.02,
    // How far two pictures of one mount may differ at any pixel, as a share
    // of full tone, and still be one picture to the eye. A hooked edge seen
    // edge on shades a few levels off the plain one and no more.
    alike: 0.05,
};

/// The frame the recipes are authored in has Z along the barrel; this is the
/// sign of Z the muzzle points to.
///
/// Read off the meshes (every barrel is a tube laid from its start towards
/// positive Z) and held by the test that puts the patrol carbine's muzzle
/// right of its stock. The picture has the muzzle to the right and up as
/// up, which fixes the side the viewer stands on: negative X when this is
/// positive.
const MUZZLE_Z: f64 = 1.0;

/// Bytes per pixel of an [`Image`]: luminance, then alpha.
pub const CHANNELS: usize = 2;

/// The smallest long side a frame may be asked for, in pixels.
///
/// Below this the padding rounds to nothing and a small mount can miss
/// every sample, which would be reported as an empty piece rather than as
/// the bad setting it is.
pub const MIN_LONG_SIDE: u32 = 32;

/// The most samples a pixel may have along one side.
///
/// Sixteen by sixteen is the most coverage levels eight bits of alpha can
/// tell apart, and it keeps one covered sample from rounding to no alpha.
pub const MAX_SAMPLES: u32 = 16;

/// Every tone and weight a picture is shaded with. See [`TUNING`] for the
/// values and what each is for.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    /// Tone of the `shell` material slot.
    pub shell: f64,
    /// Tone of the `metal` material slot.
    pub metal: f64,
    /// Tone of the `grip` material slot.
    pub grip: f64,
    /// Tone of the `accent` material slot.
    pub accent: f64,
    /// Tone of the `ceramic` material slot.
    pub ceramic: f64,
    /// Tone of the `lens` material slot.
    pub lens: f64,
    /// Tone of the `light` material slot.
    pub light: f64,
    /// Tone of a slot none of the above names, so a new material in a
    /// recipe draws as something rather than failing the bake.
    pub other: f64,
    /// Share of its tone a surface keeps when seen edge on.
    pub facing_floor: f64,
    /// Share of its tone a surface gains as it turns square to the view.
    pub facing_square: f64,
    /// Share of its tone a surface gains facing straight up and loses
    /// facing straight down.
    pub facing_above: f64,
    /// Margin round the envelope as a share of the frame's long side.
    pub padding: f64,
    /// The most two pictures of one mount may differ at any pixel, in tone
    /// or in coverage as a share of the full range, and still be marked as
    /// the same picture ([`Piece::same_as`]).
    pub alike: f64,
}

impl Tuning {
    fn tone(&self, material_slot: &str) -> f64 {
        // The bike's slots (hull, vane, fairing, trim, glow: `side_picture`)
        // read as the nearest weapon material; no weapon recipe uses them.
        match material_slot {
            "shell" | "fairing" => self.shell,
            "metal" | "hull" | "vane" => self.metal,
            "grip" | "trim" => self.grip,
            "accent" => self.accent,
            "ceramic" => self.ceramic,
            "lens" => self.lens,
            "light" | "glow" => self.light,
            _ => self.other,
        }
    }

    /// How much of its tone a surface with this unit normal shows.
    fn facing(&self, normal: DVec3) -> f64 {
        let square = (-MUZZLE_Z * normal.x).max(0.0);
        (self.facing_floor + self.facing_square * square + self.facing_above * normal.y)
            .clamp(0.0, 1.0)
    }
}

/// What a caller chooses about a bake. Together with the meshes this decides
/// every byte of the result.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Pixels on the frame's long side. One scale per chassis fits its
    /// whole envelope to this, so every piece of a weapon shares a scale.
    pub long_side: u32,
    /// Samples along one side of a pixel; a pixel is this squared.
    pub samples: u32,
    /// The tones and weights.
    pub tuning: Tuning,
}

impl Default for Settings {
    /// The bake's own: 1024 pixels covers the fitting drawing at the 4K
    /// interface scale, and sixteen samples a pixel is where an edge stops
    /// showing steps.
    fn default() -> Self {
        Self {
            long_side: 1024,
            samples: 4,
            tuning: TUNING,
        }
    }
}

impl Settings {
    /// The default look at another size and sample count, for a test that
    /// wants a fast debug run or a tool that wants a thumbnail.
    #[must_use]
    pub fn sized(long_side: u32, samples: u32) -> Self {
        Self {
            long_side,
            samples,
            tuning: TUNING,
        }
    }

    fn validate(&self) -> Result<(), PictureError> {
        if self.long_side < MIN_LONG_SIDE {
            return Err(PictureError::Settings("the long side is too small"));
        }
        if !(1..=MAX_SAMPLES).contains(&self.samples) {
            return Err(PictureError::Settings("samples must be 1 to 16"));
        }
        let padding = self.tuning.padding;
        if !(0.0..=0.4).contains(&padding) {
            return Err(PictureError::Settings("padding must be 0 to 0.4"));
        }
        Ok(())
    }
}

/// The picture space every piece of one chassis is placed in.
///
/// One per chassis, not per piece: a client lays pieces out by their
/// rectangles in this frame, so a module swapped in lands where the model
/// would put it with no placement data of its own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Size of a pixel in the weapon's own metres.
    pub metres_per_pixel: f64,
    /// Where the frame's top-left corner is in the chassis's frame, in
    /// metres: the distance along the barrel towards the muzzle, then the
    /// height. Kept so a point on the model can be found on the picture.
    pub origin: [f64; 2],
}

impl Frame {
    /// The larger of the width and the height, which is what the settings
    /// asked for.
    #[must_use]
    pub fn long_side(&self) -> u32 {
        self.width.max(self.height)
    }
}

/// Two bytes a pixel, rows from the top: luminance, then alpha.
///
/// Luminance is straight, not premultiplied: it is the mean tone of the
/// covered samples alone, so an edge pixel keeps the tone of its surface
/// and only its alpha falls off. A pixel with no coverage is two zeros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * CHANNELS` bytes.
    pub data: Vec<u8>,
}

impl Image {
    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| (y as usize * self.width as usize + x as usize) * CHANNELS)
    }

    /// Coverage at a pixel, or nothing outside the image.
    #[must_use]
    pub fn alpha(&self, x: u32, y: u32) -> u8 {
        self.offset(x, y)
            .and_then(|offset| self.data.get(offset + 1))
            .copied()
            .unwrap_or(0)
    }

    /// Tone at a pixel, or black outside the image.
    #[must_use]
    pub fn luminance(&self, x: u32, y: u32) -> u8 {
        self.offset(x, y)
            .and_then(|offset| self.data.get(offset))
            .copied()
            .unwrap_or(0)
    }
}

/// Which part of a weapon a piece shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum PieceKind {
    /// The body alone, with every mount empty.
    Chassis,
    /// The part a mount is built with when nothing is fitted.
    Standard(Slot),
    /// A module fitted to a mount, by its catalogue id.
    Module(Slot, &'static str),
    /// The stock weapon complete: the chassis and every standard part.
    Whole,
}

impl PieceKind {
    /// The mount this piece fills, if it fills one.
    #[must_use]
    pub fn mount(self) -> Option<Slot> {
        match self {
            Self::Standard(slot) | Self::Module(slot, _) => Some(slot),
            Self::Chassis | Self::Whole => None,
        }
    }

    /// The module this piece shows, if it shows one.
    #[must_use]
    pub fn module(self) -> Option<&'static str> {
        match self {
            Self::Module(_, id) => Some(id),
            Self::Chassis | Self::Standard(_) | Self::Whole => None,
        }
    }

    /// The name ticket 0008 gives this piece's file, without its
    /// extension: `chassis`, `<mount>.standard`, `<mount>.<module>`,
    /// `whole`. Here so the bake and the index cannot spell it two ways.
    #[must_use]
    pub fn file_stem(self) -> String {
        match self {
            Self::Chassis => "chassis".to_owned(),
            Self::Standard(slot) => format!("{}.standard", slot.key()),
            Self::Module(slot, id) => format!("{}.{id}", slot.key()),
            Self::Whole => "whole".to_owned(),
        }
    }
}

/// One picture, cropped to what it covers, and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// What it shows.
    pub kind: PieceKind,
    /// `[x, y, width, height]` of the image in the chassis's [`Frame`], in
    /// pixels from the frame's top-left corner.
    pub rect: [u32; 4],
    /// The pixels, `rect[2]` by `rect[3]`.
    pub image: Image,
    /// The chassis hides all of this part from the picture's side.
    ///
    /// Its picture is then of the part alone, so there is still something
    /// to show for it in a slot, but it has no place in the stack: laid over
    /// the chassis it would show what the weapon does not. Never set for
    /// the chassis or the whole.
    pub hidden: bool,
    /// The earliest piece of the same mount this picture cannot be told
    /// from by eye: the same rectangle, and no pixel further apart than
    /// [`Tuning::alike`].
    ///
    /// A mount whose alternatives differ only across the weapon or on its
    /// far side has them, and a screen that drew each as if it were a
    /// different picture would offer a choice nobody can see.
    pub same_as: Option<PieceKind>,
}

impl Piece {
    /// Whether no pixel of the two pictures is further apart than
    /// `tolerance`, in tone or in coverage, and they sit in one rectangle.
    fn alike(&self, other: &Self, tolerance: u8) -> bool {
        self.rect == other.rect
            && self
                .image
                .data
                .iter()
                .zip(&other.image.data)
                .all(|(own, theirs)| own.abs_diff(*theirs) <= tolerance)
    }
}

/// Every picture of one chassis.
#[derive(Clone, Debug, PartialEq)]
pub struct Pictures {
    /// The chassis they are of.
    pub weapon: Weapon,
    /// The frame every rectangle is in.
    pub frame: Frame,
    /// The chassis first, then for each mount in [`Weapon::slots`] order
    /// its standard part and each module [`modules`] lists, in that order.
    ///
    /// Laid over each other in this order, one piece a mount and the
    /// [`Piece::hidden`] ones left out, they give the weapon: each mount's
    /// piece already lacks what the chassis hides of it. Two mounts are not
    /// sorted against each other, so where their parts overlap the later
    /// lies over the earlier whatever their depths.
    pub pieces: Vec<Piece>,
    /// The stock weapon in one picture, for its icon. Drawn with one depth
    /// buffer across its parts, so it is also right where two mounts
    /// overlap, which the stack is not.
    pub whole: Piece,
}

impl Pictures {
    /// The piece of this kind, [`PieceKind::Whole`] included.
    #[must_use]
    pub fn piece(&self, kind: PieceKind) -> Option<&Piece> {
        if kind == PieceKind::Whole {
            return Some(&self.whole);
        }
        self.pieces.iter().find(|piece| piece.kind == kind)
    }
}

/// Why a set of pictures could not be made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PictureError {
    /// A setting is outside what the rasteriser can honour.
    #[error("picture settings: {0}")]
    Settings(&'static str),
    /// No build was given, so there is no chassis to draw.
    #[error("no builds of {0:?} were given")]
    NoBuilds(Weapon),
    /// A build is of another chassis than the one asked for.
    #[error("a build of {found:?} was given for {expected:?}")]
    WrongWeapon {
        /// The chassis asked for.
        expected: Weapon,
        /// The chassis the build is of.
        found: Weapon,
    },
    /// No build given has this piece in it. The assemblies of [`plan`]
    /// cover every piece between them.
    #[error("no build of {weapon:?} holds {piece:?}")]
    NotCovered {
        /// The chassis.
        weapon: Weapon,
        /// The piece nothing covers.
        piece: PieceKind,
    },
    /// A build has no meshed instance of this name. A merged building has
    /// none: its parts are unioned and no longer separable.
    #[error("a build of {weapon:?} has no meshed part {instance}")]
    MissingPart {
        /// The chassis.
        weapon: Weapon,
        /// The instance looked for.
        instance: String,
    },
    /// A piece covers no pixel, so there is nothing to crop it to.
    #[error("{piece:?} of {weapon:?} covers no pixel")]
    Empty {
        /// The chassis.
        weapon: Weapon,
        /// The piece.
        piece: PieceKind,
    },
}

/// The assemblies whose meshes hold every piece of a chassis between them.
///
/// A mount's part is meshed on its own, whatever sits in the other mounts,
/// so there is no need to mesh one assembly per module, still less their
/// product. The first assembly is the stock weapon; the k-th after it fits
/// the k-th module of every mount that has that many at once. That is the
/// longest module list plus one, which is the number of kernel runs a bake
/// pays for a chassis.
#[must_use]
pub fn plan(weapon: Weapon) -> Vec<Assembly> {
    let lists: Vec<_> = weapon
        .slots()
        .iter()
        .map(|&slot| (slot, modules(weapon, slot)))
        .collect();
    let longest = lists.iter().map(|(_, list)| list.len()).max().unwrap_or(0);
    (0..=longest)
        .map(|build| {
            let mut assembly = Assembly {
                weapon,
                modules: BTreeMap::new(),
                finish: Finish::Issued,
            };
            if let Some(index) = build.checked_sub(1) {
                for (slot, list) in &lists {
                    if let Some(module) = list.get(index) {
                        assembly.modules.insert(*slot, module.id.to_owned());
                    }
                }
            }
            assembly
        })
        .collect()
}

/// Rasterise every picture of a chassis from its meshed builds.
///
/// `builds` pairs each assembly with the mesh of [`crate::build`] for it,
/// normally the assemblies of [`plan`] in order. A piece is taken from the
/// first build that holds it, so the order only matters between builds that
/// would mesh the same part, and those are equal.
///
/// The same builds and settings give the same bytes: one thread, no hashed
/// order, and only correctly rounded float operations.
///
/// # Errors
/// Returns the first setting out of range, build of another chassis, piece
/// no build holds, or piece that covers no pixel.
pub fn render(
    weapon: Weapon,
    builds: &[(Assembly, MeshedBuilding)],
    settings: &Settings,
) -> Result<Pictures, PictureError> {
    settings.validate()?;
    let sources = sources(weapon, builds)?;
    let frame = frame(weapon, &sources, settings)?;
    // The chassis is the first source, and the one part every build shares.
    let body = sources.first().map(|(_, part)| part);
    let mut pieces = sources
        .iter()
        .map(|(kind, part)| {
            let behind = body.filter(|_| *kind != PieceKind::Chassis);
            piece(
                weapon,
                *kind,
                &frame,
                settings,
                std::slice::from_ref(part),
                behind,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    mark_alike(&mut pieces, byte(settings.tuning.alike));
    let stock: Vec<Placed<'_>> = sources
        .iter()
        .filter(|(kind, _)| matches!(kind, PieceKind::Chassis | PieceKind::Standard(_)))
        .map(|(_, part)| *part)
        .collect();
    let whole = piece(weapon, PieceKind::Whole, &frame, settings, &stock, None)?;
    Ok(Pictures {
        weapon,
        frame,
        pieces,
        whole,
    })
}

/// Name, for each piece, the first of its mount it cannot be told from.
///
/// Against the first only, and never against one that is itself named as
/// another's like: the marks then point at one picture a group, not along a
/// chain whose ends are further apart than the tolerance.
fn mark_alike(pieces: &mut [Piece], tolerance: u8) {
    for index in 0..pieces.len() {
        let (earlier, rest) = pieces.split_at_mut(index);
        let Some(piece) = rest.first_mut() else {
            break;
        };
        let Some(mount) = piece.kind.mount() else {
            continue;
        };
        piece.same_as = earlier
            .iter()
            .find(|other| {
                other.kind.mount() == Some(mount)
                    && other.same_as.is_none()
                    && piece.alike(other, tolerance)
            })
            .map(|other| other.kind);
    }
}

/// One part's triangles and where its instance stands.
///
/// The mesher leaves an instance's pose out of its part's mesh, because a
/// part is meshed once however often it is placed. Every part here sits at
/// the chassis's datum, so the pose is the identity today; it is applied
/// all the same, so a recipe that one day offsets a socket still draws its
/// part where the model has it.
#[derive(Clone, Copy)]
struct Placed<'a> {
    pose: Pose,
    elements: &'a [ElementMesh],
}

fn placed<'a>(
    weapon: Weapon,
    meshed: &'a MeshedBuilding,
    instance: &str,
) -> Result<Placed<'a>, PictureError> {
    meshed
        .building
        .instance(instance)
        .and_then(|placement| {
            meshed.parts.get(&placement.part).map(|elements| Placed {
                pose: placement.pose,
                elements,
            })
        })
        .ok_or_else(|| PictureError::MissingPart {
            weapon,
            instance: instance.to_owned(),
        })
}

/// Find each piece's part among the builds, in the order the pieces are
/// returned in.
fn sources(
    weapon: Weapon,
    builds: &[(Assembly, MeshedBuilding)],
) -> Result<Vec<(PieceKind, Placed<'_>)>, PictureError> {
    let (_, first) = builds.first().ok_or(PictureError::NoBuilds(weapon))?;
    if let Some((other, _)) = builds.iter().find(|(build, _)| build.weapon != weapon) {
        return Err(PictureError::WrongWeapon {
            expected: weapon,
            found: other.weapon,
        });
    }
    let mut sources = vec![(PieceKind::Chassis, placed(weapon, first, "chassis")?)];
    for &slot in weapon.slots() {
        let mut kinds = vec![(PieceKind::Standard(slot), None)];
        kinds.extend(
            modules(weapon, slot)
                .into_iter()
                .map(|module| (PieceKind::Module(slot, module.id), Some(module.id))),
        );
        for (kind, wanted) in kinds {
            // `selected` is what the recipe itself asks, so a build counts
            // as holding the standard part exactly when the recipe built it.
            let (_, meshed) = builds
                .iter()
                .find(|(build, _)| build.selected(slot).map(|module| module.id) == wanted)
                .ok_or(PictureError::NotCovered {
                    weapon,
                    piece: kind,
                })?;
            sources.push((kind, placed(weapon, meshed, slot.key())?));
        }
    }
    Ok(sources)
}

/// The side-on extent of something, in metres: along the barrel towards the
/// muzzle, then up.
#[derive(Clone, Copy)]
struct Bounds {
    min: [f64; 2],
    max: [f64; 2],
}

impl Bounds {
    fn union(self, other: Self) -> Self {
        Self {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    fn of(parts: &[Placed<'_>]) -> Option<Self> {
        parts
            .iter()
            .flat_map(|part| {
                part.elements.iter().flat_map(move |element| {
                    element.mesh.positions.iter().map(move |&position| {
                        let [along, up, _] = side_on(part.pose.transform_point(position));
                        Self {
                            min: [along, up],
                            max: [along, up],
                        }
                    })
                })
            })
            .reduce(Self::union)
    }
}

/// A point of the chassis's frame as the viewer has it: to the right, up,
/// and away.
fn side_on(point: DVec3) -> [f64; 3] {
    [MUZZLE_Z * point.z, point.y, MUZZLE_Z * point.x]
}

/// Fit the envelope of every piece to the long side asked for.
///
/// Every module that fits is in the envelope, not the stock weapon alone:
/// a long barrel or a deep magazine must land inside the same frame as the
/// part it replaces, or a client would have to resize the drawing on a swap.
fn frame(
    weapon: Weapon,
    sources: &[(PieceKind, Placed<'_>)],
    settings: &Settings,
) -> Result<Frame, PictureError> {
    let envelope = sources
        .iter()
        .filter_map(|(_, part)| Bounds::of(std::slice::from_ref(part)))
        .reduce(Bounds::union);
    fit(envelope, settings).ok_or(PictureError::Empty {
        weapon,
        piece: PieceKind::Chassis,
    })
}

/// The frame that fits `envelope` to the settings' long side, or nothing
/// for an envelope with no extent.
fn fit(envelope: Option<Bounds>, settings: &Settings) -> Option<Frame> {
    let envelope = envelope?;
    let span = [
        envelope.max[0] - envelope.min[0],
        envelope.max[1] - envelope.min[1],
    ];
    let long = span[0].max(span[1]);
    if !(long.is_finite() && long > 0.0) {
        return None;
    }
    let long_side = f64::from(settings.long_side);
    let margin = settings.tuning.padding * long_side;
    let metres_per_pixel = long / (long_side - 2.0 * margin);
    let short = whole_pixels(
        (span[0].min(span[1]) / metres_per_pixel + 2.0 * margin).ceil(),
        settings.long_side,
    )
    .max(1);
    let (width, height) = if span[0] >= span[1] {
        (settings.long_side, short)
    } else {
        (short, settings.long_side)
    };
    // Centred, so the margin is the same on both sides of each axis.
    let centre = [
        f64::midpoint(envelope.min[0], envelope.max[0]),
        f64::midpoint(envelope.min[1], envelope.max[1]),
    ];
    Some(Frame {
        width,
        height,
        metres_per_pixel,
        origin: [
            centre[0] - f64::from(width) * metres_per_pixel / 2.0,
            centre[1] + f64::from(height) * metres_per_pixel / 2.0,
        ],
    })
}

/// A pixel count or index from a float already rounded, held to `0..=limit`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a u32 range first; a NaN becomes 0"
)]
fn whole_pixels(value: f64, limit: u32) -> u32 {
    value.clamp(0.0, f64::from(limit)) as u32
}

/// A share of 0 to 1 as a byte.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 first"
)]
fn byte(share: f64) -> u8 {
    (share.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "depths are metres and tones are 0 to 1; f32 holds both"
)]
fn narrow(value: f64) -> f32 {
    value as f32
}

/// A rectangle of whole pixels of the frame.
#[derive(Clone, Copy)]
struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Region {
    /// The pixels a side-on extent can touch, inside the frame.
    fn around(frame: &Frame, bounds: Bounds) -> Self {
        let scale = frame.metres_per_pixel;
        let left = whole_pixels(
            ((bounds.min[0] - frame.origin[0]) / scale).floor(),
            frame.width,
        );
        let right = whole_pixels(
            ((bounds.max[0] - frame.origin[0]) / scale).ceil(),
            frame.width,
        );
        let top = whole_pixels(
            ((frame.origin[1] - bounds.max[1]) / scale).floor(),
            frame.height,
        );
        let bottom = whole_pixels(
            ((frame.origin[1] - bounds.min[1]) / scale).ceil(),
            frame.height,
        );
        Self {
            x: left,
            y: top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        }
    }
}

/// The tone of a sample that shows nothing of the piece: one no surface
/// reached, or one whose nearest surface only hides.
const NOTHING: f32 = -1.0;

/// What a part drawn into a canvas is there for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// It is the picture: its nearest surfaces are shaded and covered.
    Shown,
    /// It only stands in the way: its surfaces take the depth and leave the
    /// sample uncovered, so what lies behind them is cut out of the picture.
    Hiding,
}

/// The samples of one region: a depth and a tone each.
///
/// A sample is covered when its tone is one a surface can have. One canvas
/// is one depth buffer, so what is drawn into the same canvas hides what
/// lies behind it and what is drawn into another does not.
struct Canvas {
    region: Region,
    samples: u32,
    columns: u32,
    rows: u32,
    depth: Vec<f32>,
    tone: Vec<f32>,
}

impl Canvas {
    fn new(region: Region, samples: u32) -> Self {
        let columns = region.width * samples;
        let rows = region.height * samples;
        let count = columns as usize * rows as usize;
        Self {
            region,
            samples,
            columns,
            rows,
            depth: vec![f32::INFINITY; count],
            tone: vec![NOTHING; count],
        }
    }

    fn draw(&mut self, frame: &Frame, part: &Placed<'_>, tuning: &Tuning, layer: Layer) {
        let scale = f64::from(self.samples) / frame.metres_per_pixel;
        let left = frame.origin[0] + f64::from(self.region.x) * frame.metres_per_pixel;
        let top = frame.origin[1] - f64::from(self.region.y) * frame.metres_per_pixel;
        let pose = part.pose;
        for element in part.elements {
            let tone = tuning.tone(&element.material_slot);
            let mesh = &element.mesh;
            let corner = |index: u32| {
                let index = index as usize;
                let position = pose.transform_point(*mesh.positions.get(index)?);
                // A mesh without a normal a vertex shades by its faces.
                let normal = (mesh.normals.len() == mesh.positions.len())
                    .then(|| mesh.normals.get(index).copied())
                    .flatten();
                let [along, up, away] = side_on(position);
                Some(Corner {
                    position,
                    normal,
                    x: (along - left) * scale,
                    y: (top - up) * scale,
                    away,
                })
            };
            for &[first, second, third] in mesh.indices.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (corner(first), corner(second), corner(third))
                else {
                    continue;
                };
                self.fill(&[a, b, c], pose.rotation, tone, tuning, layer);
            }
        }
    }

    /// Rasterise one triangle against the depth buffer.
    fn fill(
        &mut self,
        corners: &[Corner; 3],
        rotation: DQuat,
        tone: f64,
        tuning: &Tuning,
        layer: Layer,
    ) {
        let [a, b, c] = corners;
        let face = (b.position - a.position).cross(c.position - a.position);
        // Solids are closed, so a face turned away is always behind one
        // turned towards the viewer and drawing it would only cost time.
        if -MUZZLE_Z * face.x <= 0.0 {
            return;
        }
        let area = edge(a, b, c.x, c.y);
        if !area.is_finite() || area == 0.0 {
            return;
        }
        let face = face.normalize_or_zero();
        let normals = match (a.normal, b.normal, c.normal) {
            (Some(a), Some(b), Some(c)) => Some([rotation * a, rotation * b, rotation * c]),
            _ => None,
        };
        // A sample sits at the centre of its cell, half a cell in.
        let first = |low: f64, limit: u32| whole_pixels((low - 0.5).ceil(), limit);
        let end = |high: f64, limit: u32| whole_pixels((high - 0.5).floor() + 1.0, limit);
        let columns =
            first(a.x.min(b.x).min(c.x), self.columns)..end(a.x.max(b.x).max(c.x), self.columns);
        let rows = first(a.y.min(b.y).min(c.y), self.rows)..end(a.y.max(b.y).max(c.y), self.rows);
        for row in rows {
            let down = f64::from(row) + 0.5;
            for column in columns.clone() {
                let across = f64::from(column) + 0.5;
                let towards_a = edge(b, c, across, down) / area;
                let towards_b = edge(c, a, across, down) / area;
                let towards_c = edge(a, b, across, down) / area;
                if towards_a < 0.0 || towards_b < 0.0 || towards_c < 0.0 {
                    continue;
                }
                // Measured from one corner, so a face at one depth gives
                // exactly that depth at every sample and two materials
                // sharing a plane do not speckle through each other.
                let away =
                    narrow(a.away + towards_b * (b.away - a.away) + towards_c * (c.away - a.away));
                let index = row as usize * self.columns as usize + column as usize;
                // Strictly nearer: of two surfaces at one depth the first
                // drawn keeps the sample, which is a fixed order. A mount's
                // piece and the whole both draw the chassis first, so a
                // mount flush with the body loses the same samples in both.
                if away >= self.depth[index] {
                    continue;
                }
                self.depth[index] = away;
                if layer == Layer::Hiding {
                    self.tone[index] = NOTHING;
                    continue;
                }
                let normal = normals
                    .map(|[na, nb, nc]| {
                        (na * towards_a + nb * towards_b + nc * towards_c).normalize_or_zero()
                    })
                    .filter(|normal| *normal != DVec3::ZERO)
                    .unwrap_or(face);
                self.tone[index] = narrow(tone * tuning.facing(normal));
            }
        }
    }

    /// Average the samples into pixels and crop to the covered ones.
    fn resolve(&self) -> Option<([u32; 4], Image)> {
        let Region { width, height, .. } = self.region;
        let per_pixel = f64::from(self.samples * self.samples);
        let mut pixels = vec![0_u8; width as usize * height as usize * CHANNELS];
        // The covered extent as [left, top, right, bottom], right and
        // bottom one past the last covered pixel.
        let mut covered_extent: Option<[u32; 4]> = None;
        for y in 0..height {
            for x in 0..width {
                let mut covered = 0_u32;
                let mut sum = 0.0_f64;
                for row in y * self.samples..(y + 1) * self.samples {
                    for column in x * self.samples..(x + 1) * self.samples {
                        let index = row as usize * self.columns as usize + column as usize;
                        if self.tone[index] >= 0.0 {
                            covered += 1;
                            sum += f64::from(self.tone[index]);
                        }
                    }
                }
                if covered == 0 {
                    continue;
                }
                let offset = (y as usize * width as usize + x as usize) * CHANNELS;
                pixels[offset] = byte(sum / f64::from(covered));
                // One sample of the most a pixel may have still rounds to
                // an alpha of one, so covered pixels are exactly the ones
                // with alpha.
                pixels[offset + 1] = byte(f64::from(covered) / per_pixel).max(1);
                covered_extent = Some(covered_extent.map_or([x, y, x + 1, y + 1], |extent| {
                    [
                        extent[0].min(x),
                        extent[1].min(y),
                        extent[2].max(x + 1),
                        extent[3].max(y + 1),
                    ]
                }));
            }
        }
        let [left, top, right, bottom] = covered_extent?;
        let image_width = right - left;
        let image_height = bottom - top;
        let mut data = Vec::with_capacity(image_width as usize * image_height as usize * CHANNELS);
        for y in top..bottom {
            let start = (y as usize * width as usize + left as usize) * CHANNELS;
            data.extend_from_slice(&pixels[start..start + image_width as usize * CHANNELS]);
        }
        Some((
            [
                self.region.x + left,
                self.region.y + top,
                image_width,
                image_height,
            ],
            Image {
                width: image_width,
                height: image_height,
                data,
            },
        ))
    }
}

/// One corner of a triangle, in the model and on the sample grid.
#[derive(Clone, Copy)]
struct Corner {
    position: DVec3,
    normal: Option<DVec3>,
    x: f64,
    y: f64,
    away: f64,
}

/// Twice the signed area of the triangle from `from` to `to` to a point.
fn edge(from: &Corner, to: &Corner, x: f64, y: f64) -> f64 {
    (to.x - from.x) * (y - from.y) - (to.y - from.y) * (x - from.x)
}

/// Draw parts into one depth buffer and crop the result.
///
/// With something `behind` which they stand, the parts are cut where it is
/// nearer the viewer. Parts it hides whole are drawn again without it and
/// the piece says so: a picture of the part alone is worth more to a slot
/// than no picture, and the mark keeps it out of the stack.
fn draw_parts(
    frame: &Frame,
    settings: &Settings,
    parts: &[Placed<'_>],
    behind: Option<&Placed<'_>>,
) -> Option<(bool, [u32; 4], Image)> {
    let bounds = Bounds::of(parts)?;
    let region = Region::around(frame, bounds);
    let draw = |behind: Option<&Placed<'_>>| {
        let mut canvas = Canvas::new(region, settings.samples);
        if let Some(body) = behind {
            canvas.draw(frame, body, &settings.tuning, Layer::Hiding);
        }
        for part in parts {
            canvas.draw(frame, part, &settings.tuning, Layer::Shown);
        }
        canvas.resolve()
    };
    match draw(behind) {
        Some((rect, image)) => Some((false, rect, image)),
        None if behind.is_some() => draw(None).map(|(rect, image)| (true, rect, image)),
        None => None,
    }
}

/// The side-on picture of one instance of any meshed building, muzzle (here
/// the front, `+z`) to the right, as the weapon pictures are drawn: two
/// channels, luminance then straight alpha, cropped to what it covers. The
/// frame fits the instance to `settings.long_side` with the weapons' margin.
/// For the vehicle icon, which is not a weapon and so has no `Weapon` to
/// hang a [`Pictures`] on.
///
/// # Errors
/// An instance the building lacks, or one that covers no pixel.
pub fn side_picture(
    meshed: &MeshedBuilding,
    instance: &str,
    settings: &Settings,
) -> Result<SidePicture, SidePictureError> {
    settings.validate().map_err(SidePictureError::Settings)?;
    let part = meshed
        .building
        .instance(instance)
        .and_then(|placement| {
            meshed.parts.get(&placement.part).map(|elements| Placed {
                pose: placement.pose,
                elements,
            })
        })
        .ok_or_else(|| SidePictureError::MissingPart(instance.to_owned()))?;
    let frame = fit(Bounds::of(&[part]), settings).ok_or(SidePictureError::Empty)?;
    let (_, rect, image) =
        draw_parts(&frame, settings, &[part], None).ok_or(SidePictureError::Empty)?;
    Ok(SidePicture { rect, image })
}

/// What [`side_picture`] draws: the image and where it sits in its frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidePicture {
    /// `[x, y, width, height]` in the frame, in pixels.
    pub rect: [u32; 4],
    /// The cropped picture.
    pub image: Image,
}

/// Why [`side_picture`] drew nothing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SidePictureError {
    /// A setting is outside what the rasteriser can honour.
    #[error(transparent)]
    Settings(PictureError),
    /// The building has no meshed instance of this name.
    #[error("no meshed part {0}")]
    MissingPart(String),
    /// The instance covers no pixel.
    #[error("the part covers no pixel")]
    Empty,
}

/// One piece of a chassis, drawn by [`draw_parts`].
fn piece(
    weapon: Weapon,
    kind: PieceKind,
    frame: &Frame,
    settings: &Settings,
    parts: &[Placed<'_>],
    behind: Option<&Placed<'_>>,
) -> Result<Piece, PictureError> {
    let empty = PictureError::Empty {
        weapon,
        piece: kind,
    };
    let (hidden, rect, image) = draw_parts(frame, settings, parts, behind).ok_or(empty)?;
    Ok(Piece {
        kind,
        rect,
        image,
        hidden,
        same_as: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashlar_manifold::{ManifoldMesher, mesh_building};

    /// Small enough that a debug run of every weapon stays quick, large
    /// enough that the smallest mount still covers whole pixels.
    const TEST: Settings = Settings {
        long_side: 256,
        samples: 2,
        tuning: TUNING,
    };

    fn meshed(weapon: Weapon) -> Vec<(Assembly, MeshedBuilding)> {
        plan(weapon)
            .into_iter()
            .map(|assembly| {
                let building = crate::build(&assembly).expect("a planned assembly builds");
                let mesh = mesh_building(&building, &ManifoldMesher::default())
                    .expect("a planned assembly meshes");
                (assembly, mesh)
            })
            .collect()
    }

    fn pictures(weapon: Weapon) -> Pictures {
        render(weapon, &meshed(weapon), &TEST).expect("the plan's builds render")
    }

    fn centre(piece: &Piece) -> [f64; 2] {
        let [x, y, width, height] = piece.rect.map(f64::from);
        [x + width / 2.0, y + height / 2.0]
    }

    #[test]
    fn the_plan_covers_every_piece_in_the_longest_list_plus_one() {
        for weapon in Weapon::ALL {
            let plan = plan(weapon);
            let longest = weapon
                .slots()
                .iter()
                .map(|&slot| modules(weapon, slot).len())
                .max()
                .expect("every chassis has a mount");
            assert_eq!(plan.len(), longest + 1, "{weapon:?}");
            assert!(plan[0].modules.is_empty(), "{weapon:?} starts stock");
            for assembly in &plan {
                assert_eq!(assembly.weapon, weapon);
                assembly.validate().expect("a planned assembly is legal");
            }
            for &slot in weapon.slots() {
                for module in modules(weapon, slot) {
                    assert!(
                        plan.iter().any(|assembly| assembly
                            .selected(slot)
                            .is_some_and(|fitted| fitted.id == module.id)),
                        "{weapon:?}/{slot:?} never fits {}",
                        module.id
                    );
                }
            }
        }
    }

    #[test]
    fn every_weapon_renders_to_the_size_asked_for() {
        for weapon in Weapon::ALL {
            let pictures = pictures(weapon);
            let frame = pictures.frame;
            assert_eq!(frame.long_side(), TEST.long_side, "{weapon:?}");
            // The service pistol with its tallest sight and deepest cell is
            // taller than it is long, so the long side is not always the width.
            assert!(
                frame.width > 0 && frame.height > 0 && frame.metres_per_pixel > 0.0,
                "{weapon:?} has a degenerate frame {frame:?}"
            );
            let mut expected = vec![PieceKind::Chassis];
            for &slot in weapon.slots() {
                expected.push(PieceKind::Standard(slot));
                expected.extend(
                    modules(weapon, slot)
                        .into_iter()
                        .map(|module| PieceKind::Module(slot, module.id)),
                );
            }
            let kinds: Vec<_> = pictures.pieces.iter().map(|piece| piece.kind).collect();
            assert_eq!(kinds, expected, "{weapon:?}");
            assert_eq!(pictures.whole.kind, PieceKind::Whole);
            for piece in pictures.pieces.iter().chain([&pictures.whole]) {
                assert_inside(&pictures.frame, piece, weapon);
            }
        }
    }

    fn assert_inside(frame: &Frame, piece: &Piece, weapon: Weapon) {
        let [x, y, width, height] = piece.rect;
        let name = piece.kind.file_stem();
        assert!(width > 0 && height > 0, "{weapon:?} {name} is empty");
        assert!(
            x + width <= frame.width && y + height <= frame.height,
            "{weapon:?} {name} at {:?} leaves the {}x{} frame",
            piece.rect,
            frame.width,
            frame.height
        );
        assert_eq!([piece.image.width, piece.image.height], [width, height]);
        assert_eq!(
            piece.image.data.len(),
            width as usize * height as usize * CHANNELS
        );
        // The crop is tight: each edge row and column has a covered pixel.
        let row = |y: u32| (0..width).any(|x| piece.image.alpha(x, y) > 0);
        let column = |x: u32| (0..height).any(|y| piece.image.alpha(x, y) > 0);
        assert!(
            row(0) && row(height - 1) && column(0) && column(width - 1),
            "{weapon:?} {name} is not cropped to its coverage"
        );
    }

    #[test]
    fn the_patrol_carbine_is_laid_out_muzzle_right_and_up_as_up() {
        let weapon = Weapon::PatrolCarbine;
        let pictures = pictures(weapon);
        let piece = |kind| pictures.piece(kind).expect("the carbine has this piece");
        let muzzle = piece(PieceKind::Standard(Slot::Muzzle));
        let stock = piece(PieceKind::Standard(Slot::Stock));
        assert!(
            centre(muzzle)[0] > centre(stock)[0],
            "muzzle {:?} is not right of stock {:?}",
            muzzle.rect,
            stock.rect
        );
        // The sight rides on top and the feed hangs below.
        let sight = piece(PieceKind::Standard(Slot::Sight));
        let feed = piece(PieceKind::Standard(Slot::Feed));
        assert!(
            centre(sight)[1] < centre(feed)[1],
            "sight {:?} is not above feed {:?}",
            sight.rect,
            feed.rect
        );
        let extended = piece(PieceKind::Module(Slot::Feed, "patrol_extended_feed"));
        assert!(
            extended.rect[3] > feed.rect[3],
            "extended feed {:?} is no taller than standard {:?}",
            extended.rect,
            feed.rect
        );
        for piece in pictures.pieces.iter().chain([&pictures.whole]) {
            assert_inside(&pictures.frame, piece, weapon);
        }
        // The padding leaves a clear margin all round the envelope.
        let whole = pictures.whole.rect;
        assert!(whole[0] > 0 && whole[0] + whole[2] < pictures.frame.width);
        assert!(whole[1] > 0 && whole[1] + whole[3] < pictures.frame.height);
    }

    #[test]
    fn a_part_is_drawn_where_its_instance_stands() {
        let weapon = Weapon::PatrolCarbine;
        let mut builds = meshed(weapon);
        let before = render(weapon, &builds, &TEST).expect("the plan's builds render");
        // Every recipe mates at the datum today, so the meshes alone would
        // pass with the pose ignored. Stand the stock sight somewhere else.
        let (lift, push) = (0.02, 0.03);
        let stock = &mut builds[0].1;
        let mut recipe = stock.building.clone().into_recipe();
        let sight = recipe
            .instances
            .iter_mut()
            .find(|instance| instance.id == Slot::Sight.key())
            .expect("the carbine has a sight");
        sight.attach = None;
        sight.pose = Pose::at([0.0, lift, push]);
        stock.building = recipe
            .build()
            .expect("a moved sight is still a valid recipe");
        let after = render(weapon, &builds, &TEST).expect("the moved build renders");
        // The move stays inside the envelope the taller optics already made.
        assert_eq!(before.frame, after.frame);
        let kind = PieceKind::Standard(Slot::Sight);
        let (was, is) = (
            before
                .piece(kind)
                .expect("a sight piece")
                .rect
                .map(f64::from),
            after
                .piece(kind)
                .expect("a sight piece")
                .rect
                .map(f64::from),
        );
        let scale = before.frame.metres_per_pixel;
        assert!(
            (is[0] - was[0] - push / scale).abs() <= 1.0,
            "pushed {push} m towards the muzzle: {was:?} to {is:?}"
        );
        assert!(
            (was[1] - is[1] - lift / scale).abs() <= 1.0,
            "lifted {lift} m: {was:?} to {is:?}"
        );
        assert!((is[2] - was[2]).abs() <= 1.0 && (is[3] - was[3]).abs() <= 1.0);
        assert_eq!(
            before.piece(PieceKind::Chassis),
            after.piece(PieceKind::Chassis),
            "nothing else moved"
        );
    }

    #[test]
    fn two_runs_give_the_same_bytes() {
        let weapon = Weapon::PatrolCarbine;
        // Meshed twice too: the bytes are only as steady as the kernel's.
        let first = pictures(weapon);
        let second = pictures(weapon);
        assert_eq!(first, second);
    }

    #[test]
    fn the_whole_covers_at_least_the_chassis() {
        for weapon in [Weapon::PatrolCarbine, Weapon::Pistol, Weapon::FieldBlade] {
            let pictures = pictures(weapon);
            let chassis = pictures.piece(PieceKind::Chassis).expect("a chassis piece");
            let whole = &pictures.whole;
            let mut covered = 0;
            for y in 0..chassis.rect[3] {
                for x in 0..chassis.rect[2] {
                    let own = chassis.image.alpha(x, y);
                    if own == 0 {
                        continue;
                    }
                    covered += 1;
                    // Both rectangles are in the frame, so the frame's
                    // pixel is where the two pictures meet.
                    let frame_x = chassis.rect[0] + x;
                    let frame_y = chassis.rect[1] + y;
                    let seen = frame_x
                        .checked_sub(whole.rect[0])
                        .zip(frame_y.checked_sub(whole.rect[1]))
                        .map_or(0, |(x, y)| whole.image.alpha(x, y));
                    assert!(
                        seen >= own,
                        "{weapon:?} whole has {seen} where the chassis has {own} at {frame_x},{frame_y}"
                    );
                }
            }
            assert!(covered > 0, "{weapon:?} chassis covers nothing");
        }
    }

    #[test]
    fn shading_separates_materials_and_rounds_surfaces() {
        let pictures = pictures(Weapon::PatrolCarbine);
        let whole = &pictures.whole.image;
        let mut tones = std::collections::BTreeSet::new();
        for y in 0..whole.height {
            for x in 0..whole.width {
                if whole.alpha(x, y) == u8::MAX {
                    tones.insert(whole.luminance(x, y));
                } else if whole.alpha(x, y) == 0 {
                    assert_eq!(whole.luminance(x, y), 0, "uncovered pixels are zero");
                }
            }
        }
        assert!(
            tones.len() > 8,
            "a flat picture: only {} tones in the whole",
            tones.len()
        );
        // The receiver's flat side is square to the view, which no surface
        // seen from behind or edge on can be as bright as, whatever its
        // material: the faces drawn are the ones turned to the viewer.
        let edge_on = byte(TUNING.facing_floor + TUNING.facing_above);
        assert!(
            tones.iter().max() > Some(&edge_on),
            "nothing in the whole is brighter than an edge-on surface ({edge_on})"
        );
    }

    /// The stock pieces laid over each other by their rectangles in the
    /// order they are returned in, the hidden ones left out, as a client
    /// draws the weapon: tone and coverage per pixel of the frame.
    fn stacked(pictures: &Pictures) -> Vec<[f64; 2]> {
        let frame = pictures.frame;
        let mut stack = vec![[0.0_f64; 2]; frame.width as usize * frame.height as usize];
        let drawn = pictures.pieces.iter().filter(|piece| {
            matches!(piece.kind, PieceKind::Chassis | PieceKind::Standard(_)) && !piece.hidden
        });
        for piece in drawn {
            for y in 0..piece.rect[3] {
                for x in 0..piece.rect[2] {
                    let alpha = f64::from(piece.image.alpha(x, y)) / 255.0;
                    if alpha == 0.0 {
                        continue;
                    }
                    let tone = f64::from(piece.image.luminance(x, y));
                    let at = (piece.rect[1] + y) as usize * frame.width as usize
                        + (piece.rect[0] + x) as usize;
                    let [under, covered] = stack[at];
                    let total = alpha + covered * (1.0 - alpha);
                    stack[at] = [
                        (tone * alpha + under * covered * (1.0 - alpha)) / total,
                        total,
                    ];
                }
            }
        }
        stack
    }

    /// A piece's coverage at a pixel of the frame, nothing outside it.
    fn alpha_in_frame(piece: &Piece, x: u32, y: u32) -> u8 {
        x.checked_sub(piece.rect[0])
            .zip(y.checked_sub(piece.rect[1]))
            .map_or(0, |(x, y)| piece.image.alpha(x, y))
    }

    /// Where the stack and the whole picture disagree, over the pixels the
    /// whole covers fully.
    struct Misses {
        /// Pixels the whole covers fully.
        opaque: usize,
        /// Of those, the ones the stack draws in another tone.
        differing: usize,
        /// Of those, the ones that are not an edge (every neighbour differs
        /// too) and not where two mounts lie over each other.
        wrong: usize,
    }

    fn stack_misses(pictures: &Pictures) -> Misses {
        // Levels of 255 a pixel may be off before it counts: what one
        // pixel's worth of blending two tones in another order comes to.
        const TOLERANCE: f64 = 12.0;
        let stack = stacked(pictures);
        let whole = &pictures.whole;
        let width = pictures.frame.width;
        let differs = |x: u32, y: u32| {
            alpha_in_frame(whole, x, y) == u8::MAX && {
                let [tone, _] = stack[y as usize * width as usize + x as usize];
                let seen = whole.image.luminance(x - whole.rect[0], y - whole.rect[1]);
                (tone - f64::from(seen)).abs() > TOLERANCE
            }
        };
        let mounts_over = |x: u32, y: u32| {
            pictures
                .pieces
                .iter()
                .filter(|piece| matches!(piece.kind, PieceKind::Standard(_)) && !piece.hidden)
                .filter(|piece| alpha_in_frame(piece, x, y) > 0)
                .count()
        };
        let mut misses = Misses {
            opaque: 0,
            differing: 0,
            wrong: 0,
        };
        // The padding keeps the whole off the frame's edge, so every
        // covered pixel has all eight neighbours.
        for y in whole.rect[1]..whole.rect[1] + whole.rect[3] {
            for x in whole.rect[0]..whole.rect[0] + whole.rect[2] {
                if alpha_in_frame(whole, x, y) != u8::MAX {
                    continue;
                }
                misses.opaque += 1;
                if !differs(x, y) {
                    continue;
                }
                misses.differing += 1;
                let inside = (y - 1..=y + 1).all(|ny| (x - 1..=x + 1).all(|nx| differs(nx, ny)));
                if inside && mounts_over(x, y) < 2 {
                    misses.wrong += 1;
                }
            }
        }
        misses
    }

    #[test]
    fn the_stock_pieces_stack_into_the_whole() {
        for weapon in Weapon::ALL {
            let pictures = pictures(weapon);
            let misses = stack_misses(&pictures);
            assert!(misses.opaque > 0, "{weapon:?} whole covers nothing");
            // A mount drawn alone, and so over the chassis where it stands
            // behind it, fails here by whole areas: the service pistol's
            // grip covered a twelfth of the weapon.
            assert_eq!(
                misses.wrong, 0,
                "{weapon:?}: the stack shows another surface than the whole over an area"
            );
            // What is left is edge pixels blended in another order and the
            // slivers where two mounts overlap, which the stack does not
            // sort. More than this is a recipe whose mounts now lie over
            // each other, and the fitting drawing would show it.
            assert!(
                misses.differing * 200 <= misses.opaque,
                "{weapon:?}: {} of {} pixels stack to another tone than the whole",
                misses.differing,
                misses.opaque
            );
        }
    }

    /// The carbine with its stock feed stood `across` metres along X, which
    /// is the view's own axis: the picture's rectangle does not move, only
    /// what is in front of what.
    fn carbine_with_feed_at(across: f64) -> Pictures {
        let weapon = Weapon::PatrolCarbine;
        let mut builds = meshed(weapon);
        let stock = &mut builds[0].1;
        let mut recipe = stock.building.clone().into_recipe();
        let feed = recipe
            .instances
            .iter_mut()
            .find(|instance| instance.id == Slot::Feed.key())
            .expect("the carbine has a feed");
        feed.attach = None;
        feed.pose = Pose::at([across, 0.0, 0.0]);
        stock.building = recipe
            .build()
            .expect("a moved feed is still a valid recipe");
        render(weapon, &builds, &TEST).expect("the moved build renders")
    }

    #[test]
    fn a_mount_is_cut_where_the_chassis_stands_in_front_of_it() {
        // Pixels both the chassis and the feed cover fully, and the feed's
        // own covered pixels.
        let measure = |pictures: &Pictures| {
            let chassis = pictures.piece(PieceKind::Chassis).expect("a chassis");
            let feed = pictures
                .piece(PieceKind::Standard(Slot::Feed))
                .expect("a feed");
            assert!(!feed.hidden, "the feed hangs below the receiver");
            let (mut over, mut covered) = (0, 0);
            for y in 0..feed.rect[3] {
                for x in 0..feed.rect[2] {
                    if feed.image.alpha(x, y) == 0 {
                        continue;
                    }
                    covered += 1;
                    let (frame_x, frame_y) = (feed.rect[0] + x, feed.rect[1] + y);
                    if feed.image.alpha(x, y) == u8::MAX
                        && alpha_in_frame(chassis, frame_x, frame_y) == u8::MAX
                    {
                        over += 1;
                    }
                }
            }
            (over, covered)
        };
        // The viewer stands on the negative side of X, so a metre that way
        // is in front of everything and a metre the other way behind it.
        let (over_in_front, covered_in_front) = measure(&carbine_with_feed_at(-1.0));
        let (over_behind, covered_behind) = measure(&carbine_with_feed_at(1.0));
        assert!(
            over_in_front > 0,
            "the feed's top does not reach up over the receiver, so this test shows nothing"
        );
        assert_eq!(
            over_behind, 0,
            "a feed behind the chassis is drawn over it: the nearer surface did not win"
        );
        assert!(
            covered_behind < covered_in_front,
            "the chassis hid nothing of the feed behind it: {covered_behind} of {covered_in_front}"
        );
    }

    #[test]
    fn a_part_the_chassis_hides_whole_is_marked_and_drawn_alone() {
        // The precision rifle's feed is authored on the far side of its
        // receiver. If the recipe moves it to the near side this weapon is
        // no longer the example, and the first assertion says so.
        let weapon = Weapon::PrecisionRifle;
        let pictures = pictures(weapon);
        let piece = |kind| pictures.piece(kind).expect("the rifle has this piece");
        let feed = piece(PieceKind::Standard(Slot::Feed));
        assert!(
            feed.hidden,
            "the far-side feed is drawn through the receiver, or the recipe moved it"
        );
        assert_inside(&pictures.frame, feed, weapon);
        // Nothing of it shows in the weapon: the stack without it is the
        // whole, which `the_stock_pieces_stack_into_the_whole` holds, and
        // the whole is no larger for it.
        let chassis = piece(PieceKind::Chassis);
        let whole = &pictures.whole;
        for y in 0..feed.rect[3] {
            for x in 0..feed.rect[2] {
                if feed.image.alpha(x, y) == u8::MAX {
                    let (frame_x, frame_y) = (feed.rect[0] + x, feed.rect[1] + y);
                    assert_eq!(
                        alpha_in_frame(chassis, frame_x, frame_y),
                        u8::MAX,
                        "a hidden feed shows past the chassis at {frame_x},{frame_y}"
                    );
                    assert_eq!(alpha_in_frame(whole, frame_x, frame_y), u8::MAX);
                }
            }
        }
        for piece in pictures.pieces.iter().chain([&pictures.whole]) {
            assert!(
                !piece.hidden || piece.kind.mount() == Some(Slot::Feed),
                "{} is hidden",
                piece.kind.file_stem()
            );
        }
        // A weapon of `geometry.rs` is laid out as the military ones are:
        // the butt to the left of the sight, and the sight on top.
        let sight = piece(PieceKind::Standard(Slot::Sight));
        let stock = piece(PieceKind::Standard(Slot::Stock));
        assert!(
            centre(stock)[0] < centre(sight)[0] && centre(sight)[1] < centre(stock)[1],
            "stock {:?} is not left of and below sight {:?}",
            stock.rect,
            sight.rect
        );
        assert!(chassis.rect[0] + chassis.rect[2] > stock.rect[0] + stock.rect[2]);
    }

    #[test]
    fn pictures_the_eye_cannot_tell_apart_are_marked() {
        // The precision rifle's compact feed sits inside the chassis as the
        // standard one does, so from the side the two are one picture.
        let rifle = self::pictures(Weapon::PrecisionRifle);
        let feed = PieceKind::Standard(Slot::Feed);
        assert_eq!(rifle.piece(feed).expect("the rifle's feed").same_as, None);
        assert_eq!(
            rifle
                .piece(PieceKind::Module(Slot::Feed, "precision_compact_feed"))
                .expect("the rifle's compact feed")
                .same_as,
            Some(feed),
            "the compact feed now reads apart from the standard one"
        );
        // The field blade stands on its edge to the viewer, so its hooked
        // edge shows its hook and is a picture of its own.
        let weapon = Weapon::FieldBlade;
        let pictures = pictures(weapon);
        let piece = |kind| pictures.piece(kind).expect("the blade has this piece");
        let plain = PieceKind::Standard(Slot::Edge);
        assert_eq!(piece(plain).same_as, None);
        assert_eq!(
            piece(PieceKind::Module(Slot::Edge, "blade_hooked_edge")).same_as,
            None,
            "the hooked edge is the plain one's picture again: the blade lies flat to the viewer"
        );
        // A melee weapon is laid out working end to the right too.
        let grip = piece(PieceKind::Standard(Slot::Grip));
        assert!(
            centre(piece(plain))[0] > centre(grip)[0],
            "edge {:?} is not right of grip {:?}",
            piece(plain).rect,
            grip.rect
        );

        // Parts that do look different are left unmarked.
        let carbine = self::pictures(Weapon::PatrolCarbine);
        let extended = carbine
            .piece(PieceKind::Module(Slot::Feed, "patrol_extended_feed"))
            .expect("the carbine's extended feed");
        assert_eq!(extended.same_as, None);

        // A mark names an earlier piece of the same mount that is itself
        // unmarked, so a group points at one picture.
        for weapon in Weapon::ALL {
            let pictures = self::pictures(weapon);
            for (index, piece) in pictures.pieces.iter().enumerate() {
                let Some(like) = piece.same_as else {
                    continue;
                };
                let earlier = pictures.pieces[..index]
                    .iter()
                    .find(|other| other.kind == like)
                    .unwrap_or_else(|| panic!("{weapon:?} {like:?} is not an earlier piece"));
                assert_eq!(earlier.kind.mount(), piece.kind.mount(), "{weapon:?}");
                assert_eq!(earlier.same_as, None, "{weapon:?}");
                assert_eq!(earlier.rect, piece.rect, "{weapon:?}");
            }
            assert_eq!(pictures.whole.same_as, None);
            assert!(!pictures.whole.hidden);
        }
    }

    #[test]
    fn bad_input_is_refused_by_name() {
        let weapon = Weapon::Pistol;
        let builds = meshed(weapon);
        assert_eq!(
            render(weapon, &[], &TEST),
            Err(PictureError::NoBuilds(weapon))
        );
        assert_eq!(
            render(Weapon::Breacher, &builds, &TEST),
            Err(PictureError::WrongWeapon {
                expected: Weapon::Breacher,
                found: weapon
            })
        );
        // The stock build alone holds no module.
        let first = modules(weapon, weapon.slots()[0])[0];
        assert_eq!(
            render(weapon, &builds[..1], &TEST),
            Err(PictureError::NotCovered {
                weapon,
                piece: PieceKind::Module(weapon.slots()[0], first.id)
            })
        );
        for settings in [Settings::sized(8, 2), Settings::sized(256, 0)] {
            assert!(matches!(
                render(weapon, &builds, &settings),
                Err(PictureError::Settings(_))
            ));
        }
    }

    #[test]
    fn file_stems_follow_the_ticket() {
        assert_eq!(PieceKind::Chassis.file_stem(), "chassis");
        assert_eq!(PieceKind::Whole.file_stem(), "whole");
        assert_eq!(
            PieceKind::Standard(Slot::ForeEnd).file_stem(),
            "fore_end.standard"
        );
        assert_eq!(
            PieceKind::Module(Slot::Feed, "patrol_extended_feed").file_stem(),
            "feed.patrol_extended_feed"
        );
    }
}
