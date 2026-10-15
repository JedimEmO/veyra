//! The index of the weapon models the content step bakes (ticket 0007, step 8).
//!
//! A weapon in the hand is the chassis and one baked piece per mount: the
//! module fitted there, or the part the mount is built with. The content
//! step writes each piece as a prop and this index beside them; the client
//! reads it to know which file is which piece and where a bolt leaves the
//! weapon. It lives here for the reason the pictures' index does: the client
//! must read it without linking the recipes, and the content step must write
//! it without linking Bevy.
//!
//! None of it is simulation state: it joins no world version and no
//! handshake, and a client with no models keeps its old art.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the index is written, under the content step's output root. The
/// props go beside it, a directory a chassis.
pub const MODELS_PATH: &str = "weapons/veyra/models.ron";

/// The version of the index a reader of this module understands.
pub const MODELS_VERSION: u32 = 1;

/// Every baked weapon model, by chassis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSet {
    /// [`MODELS_VERSION`] as the bake that wrote it knew it.
    pub version: u32,
    /// The material library the pieces' keys are defined in, relative to
    /// the generated root.
    pub materials: String,
    /// Each chassis's pieces, keyed by the weapon's stable key.
    pub chassis: BTreeMap<String, ChassisModels>,
}

/// The pieces of one chassis, all in the chassis's own frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChassisModels {
    /// Where a bolt leaves the chassis itself, in metres.
    pub muzzle: [f32; 3],
    /// The chassis first, then each mount's standard part and the modules
    /// that fit it.
    pub pieces: Vec<ModelPiece>,
}

/// One baked piece.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPiece {
    /// The mount it fills, by its stable key; none for the chassis.
    pub mount: Option<String>,
    /// The module it shows, by its catalogue id; none for the chassis and
    /// for the part a mount is built with.
    pub module: Option<String>,
    /// The prop file, relative to the generated root.
    pub file: String,
    /// Where a bolt leaves the weapon when this piece is its front; set on
    /// the pieces of the muzzle mount.
    pub tip: Option<[f32; 3]>,
}

/// Why an index could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelSetError {
    /// The text is not an index.
    #[error("the weapon models' index does not parse: {0}")]
    Parse(String),
    /// The index is of another version than this reader's.
    #[error(
        "the weapon models' index is version {found} and this build reads {expected}: run `just content`"
    )]
    Version {
        /// The version the index states.
        found: u32,
        /// [`MODELS_VERSION`].
        expected: u32,
    },
}

impl Default for ModelSet {
    /// An index of the current version with no chassis in it: what a client
    /// with no `generated/weapons` holds.
    fn default() -> Self {
        Self {
            version: MODELS_VERSION,
            materials: String::new(),
            chassis: BTreeMap::new(),
        }
    }
}

impl ModelSet {
    /// Read an index from the text the content step wrote.
    ///
    /// # Errors
    ///
    /// Text that is not an index, or an index of another version.
    pub fn from_ron(text: &str) -> Result<Self, ModelSetError> {
        let set: Self =
            ron::from_str(text).map_err(|error| ModelSetError::Parse(error.to_string()))?;
        if set.version != MODELS_VERSION {
            return Err(ModelSetError::Version {
                found: set.version,
                expected: MODELS_VERSION,
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
            .expect("a model index serialises")
    }
}

impl ChassisModels {
    /// The piece that fills `mount`: the module `module`, or the part the
    /// mount is built with when no module is named. `mount` none is the
    /// chassis's own body.
    #[must_use]
    pub fn piece(&self, mount: Option<&str>, module: Option<&str>) -> Option<&ModelPiece> {
        self.pieces
            .iter()
            .find(|piece| piece.mount.as_deref() == mount && piece.module.as_deref() == module)
    }

    /// Where a bolt leaves the weapon: the fitted muzzle piece's tip, else
    /// the muzzle mount's standard part's, else the chassis's own.
    #[must_use]
    pub fn tip(&self, fitted_muzzle: Option<&str>) -> [f32; 3] {
        fitted_muzzle
            .and_then(|module| self.piece(Some("muzzle"), Some(module)))
            .and_then(|piece| piece.tip)
            .or_else(|| self.piece(Some("muzzle"), None).and_then(|piece| piece.tip))
            .unwrap_or(self.muzzle)
    }
}

#[cfg(test)]
// Tips are compared with the exact values the test wrote, never computed.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn piece(mount: Option<&str>, module: Option<&str>, tip: Option<[f32; 3]>) -> ModelPiece {
        let stem = match (mount, module) {
            (None, _) => "chassis".to_owned(),
            (Some(mount), None) => format!("{mount}.standard"),
            (Some(mount), Some(module)) => format!("{mount}.{module}"),
        };
        ModelPiece {
            mount: mount.map(str::to_owned),
            module: module.map(str::to_owned),
            file: format!("weapons/veyra/patrol_carbine/{stem}.ashprop"),
            tip,
        }
    }

    fn set() -> ModelSet {
        let mut set = ModelSet {
            materials: "weapons/veyra/library.materials.ron".to_owned(),
            ..ModelSet::default()
        };
        set.chassis.insert(
            "patrol_carbine".to_owned(),
            ChassisModels {
                muzzle: [0.0, 0.07, 0.4],
                pieces: vec![
                    piece(None, None, None),
                    piece(Some("muzzle"), None, Some([0.0, 0.07, 0.49])),
                    piece(
                        Some("muzzle"),
                        Some("vigil_compensator"),
                        Some([0.0, 0.07, 0.55]),
                    ),
                    piece(Some("sight"), None, None),
                    piece(Some("sight"), Some("vigil_reflex"), None),
                ],
            },
        );
        set
    }

    #[test]
    fn an_index_round_trips_through_ron() {
        let set = set();
        assert_eq!(ModelSet::from_ron(&set.to_ron()), Ok(set));
    }

    #[test]
    fn a_piece_is_found_by_its_mount_and_module() {
        let set = set();
        let carbine = &set.chassis["patrol_carbine"];
        assert!(carbine.piece(None, None).is_some());
        assert!(
            carbine
                .piece(Some("sight"), None)
                .expect("a piece")
                .file
                .ends_with("sight.standard.ashprop")
        );
        assert!(
            carbine
                .piece(Some("sight"), Some("vigil_reflex"))
                .expect("a piece")
                .file
                .ends_with("sight.vigil_reflex.ashprop")
        );
        assert!(carbine.piece(Some("sight"), Some("nothing")).is_none());
    }

    #[test]
    fn the_tip_falls_back_from_the_fitted_muzzle_to_the_standard_to_the_chassis() {
        let set = set();
        let carbine = &set.chassis["patrol_carbine"];
        assert_eq!(carbine.tip(Some("vigil_compensator")), [0.0, 0.07, 0.55]);
        assert_eq!(carbine.tip(None), [0.0, 0.07, 0.49]);
        assert_eq!(carbine.tip(Some("unknown")), [0.0, 0.07, 0.49]);
        let mut bare = carbine.clone();
        bare.pieces
            .retain(|piece| piece.mount.as_deref() != Some("muzzle"));
        assert_eq!(bare.tip(Some("vigil_compensator")), [0.0, 0.07, 0.4]);
    }
}
