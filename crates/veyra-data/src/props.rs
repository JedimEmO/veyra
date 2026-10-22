//! The index of the baked prop kit (review step six, visual.md F6): which
//! file draws which interaction prop, written by the content step and read
//! by the game. Art only: in no world version and no handshake, and a game
//! without it stands the plain boxes.

use serde::{Deserialize, Serialize};

/// Where the content step writes the index, under its output root; the
/// props and their material library go beside it.
pub const MODELS_PATH: &str = "props/veyra/models.ron";

/// Which baked prop draws which interaction prop.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropModels {
    /// The material library the props' keys are defined in, relative to the
    /// generated root.
    pub materials: String,
    /// The prop file of each piece, by the piece's name, relative to the
    /// generated root.
    pub files: std::collections::BTreeMap<String, String>,
}

impl PropModels {
    /// Read an index from the text the content step wrote.
    ///
    /// # Errors
    /// Text that is not an index.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// The index as the content step writes it.
    ///
    /// # Panics
    /// Never: every field serialises.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a model index serialises")
    }

    /// The file that draws `piece`, if baked.
    #[must_use]
    pub fn file(&self, piece: &str) -> Option<&str> {
        self.files.get(piece).map(String::as_str)
    }
}
