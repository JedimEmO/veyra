//! The baked library: what the content step made of each tree, bush and
//! rock species.
//!
//! Tree shapes are no longer grown at runtime. `just content` grows every
//! variant of every tree species (`veyra-flora`), bakes it to an ashlar prop,
//! and writes this library beside the props: per species, the variants in
//! order, each with its prop file, its height and its trunk. Placement reads
//! the variant count and the trunk from here, on the server as on the client,
//! so a tree's collider is the trunk its mesh was grown with.
//!
//! The world is therefore the seed plus this library (ADR 0005, 2026-09-26
//! amendment). Its `version` is a hash of its contents, and a client whose
//! library differs from the server's is refused at the handshake.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use strata_data::{AssetSource, DirSource, SourceError};

/// Where the library is written, under the content step's output root.
pub const LIBRARY_PATH: &str = "flora/library.ron";

/// Every baked tree species, by flora registry id.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeLibrary {
    /// A hash of everything else in the library: two libraries with the same
    /// version place and collide the same trees.
    pub version: u64,
    /// Each species' variants, keyed by its flora id (`strata:oak`).
    pub species: BTreeMap<String, SpeciesEntry>,
}

/// One tree species in the library.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesEntry {
    /// The variants in placement order: a placed plant's `variant` indexes
    /// this.
    pub variants: Vec<TreeVariant>,
}

/// One baked variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeVariant {
    /// The prop file, relative to the content step's output root.
    pub prop: String,
    /// Metres from the ground to the top of the crown, at scale one.
    pub height: f32,
    /// What the server collides with, at scale one, in the prop's frame.
    pub collider: Collider,
}

/// A baked variant's collider at scale one, in the prop's frame: `y` up,
/// the root at the origin, before the plant's yaw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Collider {
    /// Nothing: a flower or a fern is walked through.
    None,
    /// An upright cylinder standing on the ground: a trunk.
    Cylinder(Trunk),
    /// A capsule between two points: a fallen log.
    Capsule {
        /// One end.
        a: [f32; 3],
        /// The other end.
        b: [f32; 3],
        /// Metres from the segment to the surface.
        radius: f32,
    },
    /// Convex hulls: a rock, or the pieces of a cliff.
    Hulls(Vec<Vec<[f32; 3]>>),
}

impl Collider {
    /// The collider of a plant at `scale`.
    #[must_use]
    pub fn scaled(&self, scale: f32) -> Self {
        let point = |p: &[f32; 3]| p.map(|v| v * scale);
        match self {
            Self::None => Self::None,
            Self::Cylinder(trunk) => Self::Cylinder(Trunk {
                radius: trunk.radius * scale,
                height: trunk.height * scale,
            }),
            Self::Capsule { a, b, radius } => Self::Capsule {
                a: point(a),
                b: point(b),
                radius: radius * scale,
            },
            Self::Hulls(hulls) => Self::Hulls(
                hulls
                    .iter()
                    .map(|hull| hull.iter().map(point).collect())
                    .collect(),
            ),
        }
    }

    /// The upright cylinder, for a trunk.
    #[must_use]
    pub fn trunk(&self) -> Option<Trunk> {
        match self {
            Self::Cylinder(trunk) => Some(*trunk),
            _ => None,
        }
    }
}

/// A trunk collider: an upright cylinder standing on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trunk {
    /// Metres.
    pub radius: f32,
    /// Metres from the ground to the top of the collider.
    pub height: f32,
}

/// Why the library could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// The file could not be read.
    #[error("reading the tree library {path}: {source}; run `just content`")]
    Io {
        /// The file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// The file is not a library.
    #[error("parsing the tree library {path}: {message}")]
    Parse {
        /// The file.
        path: String,
        /// Why.
        message: String,
    },
    /// The file's version does not match its contents.
    #[error("the tree library {path} says version {stated} but hashes to {actual}")]
    Version {
        /// The file.
        path: String,
        /// What the file says.
        stated: u64,
        /// What its contents hash to.
        actual: u64,
    },
}

impl TreeLibrary {
    /// Read the library under `root`, the content step's output directory,
    /// through the native directory adapter. A process with an installed
    /// source reads [`TreeLibrary::read_from`] instead.
    ///
    /// # Errors
    ///
    /// As [`TreeLibrary::read_from`].
    pub fn read(root: &Path) -> Result<Self, LibraryError> {
        Self::read_from(&DirSource::new(root))
    }

    /// Read the library from `generated`, a source laid out as the content
    /// step's output directory, and check its version against its contents.
    ///
    /// # Errors
    ///
    /// A file that is missing, does not parse, or whose version is not its
    /// contents' hash.
    pub fn read_from(generated: &dyn AssetSource) -> Result<Self, LibraryError> {
        let display = LIBRARY_PATH.to_owned();
        let text = generated
            .read(LIBRARY_PATH)
            .and_then(|bytes| {
                String::from_utf8(bytes).map_err(|error| SourceError::Io {
                    path: display.clone(),
                    message: error.to_string(),
                })
            })
            .map_err(|source| LibraryError::Io {
                path: display.clone(),
                source: std::io::Error::other(source.to_string()),
            })?;
        let library: Self = ron::from_str(&text).map_err(|error| LibraryError::Parse {
            path: display.clone(),
            message: error.to_string(),
        })?;
        let actual = library.content_hash();
        if library.version != actual {
            return Err(LibraryError::Version {
                path: display,
                stated: library.version,
                actual,
            });
        }
        Ok(library)
    }

    /// This library with its version set to its contents' hash.
    #[must_use]
    pub fn versioned(mut self) -> Self {
        self.version = self.content_hash();
        self
    }

    /// The library as the content step writes it.
    ///
    /// # Panics
    ///
    /// Never: every field serialises.
    #[must_use]
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a tree library serialises")
    }

    /// The variant `variant` of `species`, if the library has it.
    #[must_use]
    pub fn variant(&self, species: &str, variant: u8) -> Option<&TreeVariant> {
        self.species
            .get(species)?
            .variants
            .get(usize::from(variant))
    }

    /// FNV-1a over the library's text with the version zeroed: stable across
    /// runs and platforms, which a `Hash` of the struct is not promised to be.
    fn content_hash(&self) -> u64 {
        let text = Self {
            version: 0,
            species: self.species.clone(),
        }
        .to_ron();
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> TreeLibrary {
        TreeLibrary {
            version: 0,
            species: BTreeMap::from([(
                "strata:oak".to_owned(),
                SpeciesEntry {
                    variants: vec![TreeVariant {
                        prop: "flora/oak-0.ashprop".to_owned(),
                        height: 9.5,
                        collider: Collider::Cylinder(Trunk {
                            radius: 0.3,
                            height: 5.7,
                        }),
                    }],
                },
            )]),
        }
        .versioned()
    }

    #[test]
    fn a_written_library_reads_back_and_checks_its_version() {
        let root = std::env::temp_dir().join(format!("tree-library-{}", std::process::id()));
        std::fs::create_dir_all(root.join("flora")).expect("a scratch directory");
        std::fs::write(root.join(LIBRARY_PATH), library().to_ron()).expect("written");
        assert_eq!(TreeLibrary::read(&root).expect("reads"), library());

        let mut tampered = library();
        tampered
            .species
            .get_mut("strata:oak")
            .expect("oak")
            .variants[0]
            .height = 12.0;
        std::fs::write(root.join(LIBRARY_PATH), tampered.to_ron()).expect("written");
        assert!(matches!(
            TreeLibrary::read(&root),
            Err(LibraryError::Version { .. })
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_library_says_to_run_the_content_step() {
        let error = TreeLibrary::read(Path::new("/nonexistent")).expect_err("no file");
        assert!(error.to_string().contains("just content"), "{error}");
    }

    #[test]
    fn the_version_is_a_function_of_the_contents() {
        assert_eq!(library().version, library().version);
        assert_ne!(library().version, 0);
    }
}

#[cfg(test)]
mod collider_tests {
    use super::*;

    #[test]
    fn a_collider_scales_with_its_plant() {
        let hull = Collider::Hulls(vec![vec![[1.0, 2.0, 3.0]]]);
        assert_eq!(
            hull.scaled(2.0),
            Collider::Hulls(vec![vec![[2.0, 4.0, 6.0]]])
        );
        let log = Collider::Capsule {
            a: [-1.0, 0.2, 0.0],
            b: [1.0, 0.2, 0.0],
            radius: 0.2,
        };
        let Collider::Capsule { a, radius, .. } = log.scaled(1.5) else {
            panic!("a capsule stays a capsule");
        };
        assert!((a[0] + 1.5).abs() < 1e-6 && (radius - 0.3).abs() < 1e-6);
        assert!(Collider::None.trunk().is_none());
    }
}
