//! The index of the weapon pictures the content step bakes (ticket 0008).
//!
//! A weapon is drawn in the interface as a stack of flat pieces, one for the
//! chassis and one for each mount, and the same pieces are the inventory's
//! icons. The content step rasterises them from the weapon recipes and
//! writes this index beside them; the client reads it to know which file
//! shows which mount and where in the chassis's frame it goes. It lives
//! here, not with the rasteriser, because the client must read it without
//! linking the weapon recipes, and not in the client, because the content
//! step must write it without linking Bevy.
//!
//! None of it is simulation state: it joins no world version and no
//! handshake, and a client with no pictures draws boxes.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the index is written, under the content step's output root. The
/// pictures go beside it, a directory a chassis.
pub const PICTURES_PATH: &str = "weapons/veyra/pictures.ron";

/// The version of the index a reader of this module understands.
///
/// Bumped when a field changes its meaning or the pictures change how they
/// are to be drawn (their channels, what a rectangle is measured from), so
/// a `generated/` left over from an older bake is refused by name and not
/// drawn wrongly. Two since every picture has a square icon beside it: an
/// index without them would leave the cells their stand-in shapes with no
/// word of why.
pub const PICTURES_VERSION: u32 = 2;

/// Every baked weapon picture, by chassis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureSet {
    /// [`PICTURES_VERSION`] as the bake that wrote it knew it.
    pub version: u32,
    /// Each chassis's pictures, keyed by the weapon's stable key
    /// (`patrol_carbine`).
    pub chassis: BTreeMap<String, ChassisPictures>,
}

/// The pictures of one chassis, all in one frame.
///
/// One frame and one scale for the chassis and everything that fits it, so
/// a piece is placed by its rectangle alone and a module swapped in lands
/// where the model would put it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChassisPictures {
    /// The frame's width in pixels.
    pub width: u32,
    /// The frame's height in pixels.
    pub height: u32,
    /// The size of a pixel in the weapon's own metres, so two chassis can
    /// be drawn to a common scale.
    pub metres_per_pixel: f64,
    /// Where the frame's top-left corner is on the model, in metres: the
    /// distance along the barrel towards the muzzle, then the height. With
    /// the scale it finds a point of the model on the picture.
    pub origin: [f64; 2],
    /// The stock weapon in one picture, for its icon. Not a piece of the
    /// stack: it is the chassis and every standard part drawn together.
    pub whole: Placed,
    /// The chassis first, then each mount's standard part followed by the
    /// modules that fit it.
    ///
    /// Drawn over each other in this order, one piece a mount and the
    /// [`PiecePicture::hidden`] ones left out, they give the weapon: a
    /// mount's picture already lacks what the chassis hides of it. Two
    /// mounts are not sorted against each other, so where their parts
    /// overlap the later is drawn over the earlier whatever their depths;
    /// `whole` is right there too and the stack is off by that sliver.
    pub pieces: Vec<PiecePicture>,
}

/// A picture's file and where it sits in its chassis's frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    /// `[x, y, width, height]` in the frame, in pixels from its top-left
    /// corner. The file is cropped to this, so its size is the last two.
    pub rect: [u32; 4],
    /// The file, relative to the generated root: two channels of eight
    /// bits, luminance then straight alpha.
    pub file: String,
    /// The same picture fitted into a square for a cell of a grid, relative
    /// to the generated root and in the same two channels. A side-on weapon
    /// is far wider than a cell, so the cell's file is its own.
    #[serde(default)]
    pub icon: Option<String>,
}

/// One piece of a chassis's stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiecePicture {
    /// The mount it fills, by its stable key (`muzzle`); none for the
    /// chassis itself.
    pub mount: Option<String>,
    /// The module it shows, by its catalogue id; none for the chassis and
    /// for the part a mount is built with.
    pub module: Option<String>,
    /// `[x, y, width, height]` in the frame, in pixels from its top-left
    /// corner. The file is cropped to this, so its size is the last two.
    pub rect: [u32; 4],
    /// The file, relative to the generated root: two channels of eight
    /// bits, luminance then straight alpha.
    pub file: String,
    /// The same picture fitted into a square for a cell of a grid, as
    /// [`Placed::icon`]. A part the chassis hides has one too: its file
    /// shows the part alone.
    #[serde(default)]
    pub icon: Option<String>,
    /// The chassis hides all of this part from the picture's side.
    ///
    /// The file then shows the part alone, which is still its icon, but it
    /// is no piece of the stack and no region to pick: drawn over the
    /// chassis it would show what the weapon does not.
    pub hidden: bool,
    /// The file of an earlier piece of the same mount that this picture
    /// cannot be told from by eye, when there is one.
    ///
    /// A mount whose alternatives differ only across the weapon or on its
    /// far side has them. A screen that leans on the picture to show which
    /// is fitted, or draws the two as different icons, offers a difference
    /// nobody can see, and should say it in words there.
    pub same_as: Option<String>,
}

/// Why an index could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PictureSetError {
    /// The text is not an index.
    #[error("the weapon pictures' index does not parse: {0}")]
    Parse(String),
    /// The index is of another version than this reader's.
    #[error(
        "the weapon pictures' index is version {found} and this build reads {expected}: run `just content`"
    )]
    Version {
        /// The version the index states.
        found: u32,
        /// [`PICTURES_VERSION`].
        expected: u32,
    },
}

impl Default for PictureSet {
    /// An index of the current version with no chassis in it: what a bake
    /// starts from, and what a client with no `generated/weapons` holds.
    fn default() -> Self {
        Self {
            version: PICTURES_VERSION,
            chassis: BTreeMap::new(),
        }
    }
}

impl PictureSet {
    /// Read an index from the text the content step wrote.
    ///
    /// # Errors
    ///
    /// Text that is not an index, or an index of another version.
    pub fn from_ron(text: &str) -> Result<Self, PictureSetError> {
        let set: Self =
            ron::from_str(text).map_err(|error| PictureSetError::Parse(error.to_string()))?;
        if set.version != PICTURES_VERSION {
            return Err(PictureSetError::Version {
                found: set.version,
                expected: PICTURES_VERSION,
            });
        }
        Ok(set)
    }

    /// The index as the content step writes it.
    ///
    /// # Panics
    ///
    /// Never: every field serialises.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a picture index serialises")
    }

    /// Every file the index names, the whole pictures and the icons
    /// included, in the chassis's order.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.chassis.values().flat_map(|chassis| {
            std::iter::once(chassis.whole.file.as_str())
                .chain(chassis.whole.icon.as_deref())
                .chain(chassis.pieces.iter().flat_map(|piece| {
                    std::iter::once(piece.file.as_str()).chain(piece.icon.as_deref())
                }))
        })
    }

    /// The square icon of the picture filed as `file`, when the bake made
    /// one: an item names its picture by that file and a cell draws this.
    #[must_use]
    pub fn icon_of(&self, file: &str) -> Option<&str> {
        self.chassis.values().find_map(|chassis| {
            if chassis.whole.file == file {
                return chassis.whole.icon.as_deref();
            }
            chassis
                .pieces
                .iter()
                .find(|piece| piece.file == file)
                .and_then(|piece| piece.icon.as_deref())
        })
    }
}

impl ChassisPictures {
    /// The chassis's own piece, the body with every mount empty.
    #[must_use]
    pub fn body(&self) -> Option<&PiecePicture> {
        self.pieces.iter().find(|piece| piece.mount.is_none())
    }

    /// The piece that fills `mount`: the module `module`, or the part the
    /// mount is built with when no module is named.
    #[must_use]
    pub fn piece(&self, mount: &str, module: Option<&str>) -> Option<&PiecePicture> {
        self.pieces.iter().find(|piece| {
            piece.mount.as_deref() == Some(mount) && piece.module.as_deref() == module
        })
    }

    /// The pieces that drawn in this order give the stock weapon: the
    /// body, then every mount's standard part the chassis does not hide
    /// whole. See [`Self::pieces`] for how far the stack is the weapon.
    pub fn stock(&self) -> impl Iterator<Item = &PiecePicture> {
        self.pieces
            .iter()
            .filter(|piece| piece.module.is_none() && !piece.hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn set() -> PictureSet {
        let piece = |mount: Option<&str>, module: Option<&str>, rect, stem: &str| PiecePicture {
            mount: mount.map(str::to_owned),
            module: module.map(str::to_owned),
            rect,
            file: format!("weapons/veyra/patrol_carbine/{stem}.png"),
            icon: Some(format!("weapons/veyra/patrol_carbine/icon.{stem}.png")),
            hidden: false,
            same_as: None,
        };
        let mut set = PictureSet::default();
        set.chassis.insert(
            "patrol_carbine".to_owned(),
            ChassisPictures {
                width: 1024,
                height: 362,
                // Not a short decimal, so the round trip is of every bit.
                metres_per_pixel: 0.78 / 923.0,
                origin: [-0.412_345_678_901_234_5, 0.2],
                whole: Placed {
                    rect: [20, 30, 900, 300],
                    file: "weapons/veyra/patrol_carbine/whole.png".to_owned(),
                    icon: Some("weapons/veyra/patrol_carbine/icon.whole.png".to_owned()),
                },
                pieces: vec![
                    piece(None, None, [120, 40, 500, 210], "chassis"),
                    piece(Some("muzzle"), None, [800, 90, 60, 40], "muzzle.standard"),
                    piece(
                        Some("muzzle"),
                        Some("vigil_suppressor"),
                        [800, 84, 190, 52],
                        "muzzle.vigil_suppressor",
                    ),
                ],
            },
        );
        set
    }

    #[test]
    fn the_index_round_trips_through_ron() {
        let set = set();
        let text = set.to_ron();
        assert_eq!(PictureSet::from_ron(&text).expect("reads"), set);
        // Written twice it is the same text, so a warm bake rewrites nothing.
        assert_eq!(PictureSet::from_ron(&text).expect("reads").to_ron(), text);
    }

    #[test]
    fn a_piece_is_found_by_its_mount_and_module() {
        let set = set();
        let chassis = &set.chassis["patrol_carbine"];
        assert_eq!(
            chassis.body().map(|piece| piece.rect),
            Some([120, 40, 500, 210])
        );
        assert_eq!(
            chassis.piece("muzzle", None).map(|piece| piece.rect),
            Some([800, 90, 60, 40])
        );
        assert_eq!(
            chassis
                .piece("muzzle", Some("vigil_suppressor"))
                .map(|piece| piece.file.as_str()),
            Some("weapons/veyra/patrol_carbine/muzzle.vigil_suppressor.png")
        );
        assert_eq!(chassis.piece("stock", None), None);
        assert_eq!(chassis.stock().count(), 2, "the body and the muzzle");
        assert_eq!(
            set.files().count(),
            8,
            "three pieces and the whole, and an icon each"
        );
        assert_eq!(
            set.icon_of("weapons/veyra/patrol_carbine/whole.png"),
            Some("weapons/veyra/patrol_carbine/icon.whole.png")
        );
        assert_eq!(
            set.icon_of("weapons/veyra/patrol_carbine/muzzle.vigil_suppressor.png"),
            Some("weapons/veyra/patrol_carbine/icon.muzzle.vigil_suppressor.png")
        );
        assert_eq!(
            set.icon_of("weapons/veyra/patrol_carbine/nothing.png"),
            None
        );
    }

    #[test]
    fn a_part_the_chassis_hides_is_found_and_not_stacked() {
        let mut set = set();
        let chassis = set.chassis.get_mut("patrol_carbine").expect("the carbine");
        let standard = chassis.pieces[1].file.clone();
        chassis.pieces[1].hidden = true;
        chassis.pieces[2].same_as = Some(standard);
        // Still found by its mount, and still a file of the index: it is
        // the part's icon. Only the stack leaves it out.
        assert!(
            chassis
                .piece("muzzle", None)
                .is_some_and(|piece| piece.hidden)
        );
        let stock: Vec<_> = chassis
            .stock()
            .map(|piece| piece.mount.as_deref())
            .collect();
        assert_eq!(stock, [None], "the body alone");
        let text = set.to_ron();
        assert_eq!(PictureSet::from_ron(&text).expect("reads"), set);
        assert_eq!(set.files().count(), 8);
    }

    #[test]
    fn another_version_and_other_text_are_refused_by_name() {
        let mut old = set();
        old.version = PICTURES_VERSION + 1;
        assert_eq!(
            PictureSet::from_ron(&old.to_ron()),
            Err(PictureSetError::Version {
                found: PICTURES_VERSION + 1,
                expected: PICTURES_VERSION,
            })
        );
        assert!(matches!(
            PictureSet::from_ron("(version: 2, chassis: {}, extra: 2)"),
            Err(PictureSetError::Parse(_))
        ));
        assert!(
            PictureSetError::Version {
                found: 0,
                expected: PICTURES_VERSION
            }
            .to_string()
            .contains("just content")
        );
    }

    /// An index baked before the icons has no `icon` on anything. It still
    /// parses, so what it is refused for is its version, which is the
    /// message that says what to run.
    #[test]
    fn an_older_index_is_refused_by_name() {
        let old = r#"(
            version: 1,
            chassis: {
                "patrol_carbine": (
                    width: 1024,
                    height: 362,
                    metres_per_pixel: 0.001,
                    origin: (0.0, 0.2),
                    whole: (rect: (20, 30, 900, 300), file: "weapons/veyra/patrol_carbine/whole.png"),
                    pieces: [(
                        mount: None,
                        module: None,
                        rect: (120, 40, 500, 210),
                        file: "weapons/veyra/patrol_carbine/chassis.png",
                        hidden: false,
                        same_as: None,
                    )],
                ),
            },
        )"#;
        assert_eq!(
            PictureSet::from_ron(old),
            Err(PictureSetError::Version {
                found: 1,
                expected: PICTURES_VERSION,
            })
        );
        // The same text at this version reads, its pictures without icons.
        let now = old.replacen("version: 1", &format!("version: {PICTURES_VERSION}"), 1);
        let set = PictureSet::from_ron(&now).expect("reads");
        assert_eq!(set.chassis["patrol_carbine"].whole.icon, None);
        assert_eq!(
            set.icon_of("weapons/veyra/patrol_carbine/chassis.png"),
            None
        );
    }
}
