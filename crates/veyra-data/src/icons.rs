//! `icons/catalogue.json`: the art library's index, read so that every icon
//! the game names resolves to a file that exists. The art tools write the
//! file; the game only reads it. Client art only: in no world version and
//! no handshake.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::item::{ItemDefinition, ItemKind};
use crate::pictures::{PICTURES_PATH, PictureSet};
use crate::vehicle::VehicleModels;

/// Where the catalogue lives under the asset root.
pub const CATALOGUE_FILE: &str = "icons/catalogue.json";

/// The catalogue version this reader understands.
pub const CATALOGUE_VERSION: u32 = 2;

/// What an icon is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// A coloured square, drawn as it is.
    #[serde(rename = "ability")]
    Painted,
    /// A white mark on clear, meant to be tinted.
    Symbol,
}

/// One entry, with what a game needs of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    /// The catalogue id, `veyra:icon...`.
    pub id: String,
    /// The art tool's label.
    pub label: String,
    /// Painted square or symbol.
    pub family: Family,
    /// Whether it is meant to be tinted.
    pub tintable: bool,
    /// Path under the asset root, by the export's size in pixels.
    pub exports: BTreeMap<u32, String>,
}

/// An error in the catalogue file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IconError {
    /// The text is not the JSON the reader expects.
    #[error("not a catalogue: {0}")]
    Json(String),
    /// A version this reader does not know.
    #[error("catalogue version {found}, this reader knows {CATALOGUE_VERSION}")]
    Version {
        /// The file's version.
        found: u32,
    },
    /// Two entries share an id.
    #[error("`{0}` is in the catalogue twice")]
    Duplicate(String),
    /// An entry is not marked ready.
    #[error("`{0}` is not ready")]
    NotReady(String),
    /// An entry exports nothing.
    #[error("`{0}` has no export")]
    NoExport(String),
    /// An export size is no number.
    #[error("`{id}`: export size `{size}` is no number")]
    BadSize {
        /// The entry.
        id: String,
        /// The key.
        size: String,
    },
}

#[derive(Deserialize)]
struct RawFile {
    version: u32,
    icons: Vec<RawIcon>,
}

#[derive(Deserialize)]
struct RawIcon {
    id: String,
    label: String,
    family: Family,
    #[serde(default)]
    tintable: bool,
    status: String,
    #[serde(default)]
    exports: BTreeMap<String, String>,
}

/// The art library by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IconCatalogue {
    icons: BTreeMap<String, Icon>,
}

impl IconCatalogue {
    /// Reads and checks the catalogue's text.
    ///
    /// # Errors
    /// The first thing wrong: JSON, version, a duplicate id, an entry not
    /// ready or without an export.
    pub fn parse(text: &str) -> Result<Self, IconError> {
        let raw: RawFile =
            serde_json::from_str(text).map_err(|error| IconError::Json(error.to_string()))?;
        if raw.version != CATALOGUE_VERSION {
            return Err(IconError::Version { found: raw.version });
        }
        let mut seen = BTreeSet::new();
        let mut icons = BTreeMap::new();
        for entry in raw.icons {
            if !seen.insert(entry.id.clone()) {
                return Err(IconError::Duplicate(entry.id));
            }
            if entry.status != "ready" {
                return Err(IconError::NotReady(entry.id));
            }
            if entry.exports.is_empty() {
                return Err(IconError::NoExport(entry.id));
            }
            let mut exports = BTreeMap::new();
            for (size, path) in entry.exports {
                let Ok(pixels) = size.parse::<u32>() else {
                    return Err(IconError::BadSize { id: entry.id, size });
                };
                exports.insert(pixels, path);
            }
            icons.insert(
                entry.id.clone(),
                Icon {
                    id: entry.id,
                    label: entry.label,
                    family: entry.family,
                    tintable: entry.tintable,
                    exports,
                },
            );
        }
        Ok(Self { icons })
    }

    /// An entry by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Icon> {
        self.icons.get(id)
    }

    /// Every entry, by id.
    pub fn iter(&self) -> impl Iterator<Item = &Icon> {
        self.icons.values()
    }

    /// How many entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.icons.len()
    }

    /// Whether the catalogue holds nothing (a data set without the file).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// The path, under the asset root, of the smallest export that is at
    /// least `at_least` pixels, or of the largest when none is.
    #[must_use]
    pub fn file(&self, id: &str, at_least: u32) -> Option<&str> {
        let icon = self.icons.get(id)?;
        icon.exports
            .range(at_least..)
            .next()
            .or_else(|| icon.exports.iter().next_back())
            .map(|(_, path)| path.as_str())
    }

    /// Whether `id` is a painted square with a 64 and a 256 export.
    #[must_use]
    pub fn is_square(&self, id: &str) -> bool {
        self.icons.get(id).is_some_and(|icon| {
            icon.family == Family::Painted
                && icon.exports.contains_key(&64)
                && icon.exports.contains_key(&256)
        })
    }

    /// Whether `id` is a tintable symbol.
    #[must_use]
    pub fn is_symbol(&self, id: &str) -> bool {
        self.icons
            .get(id)
            .is_some_and(|icon| icon.family == Family::Symbol && icon.tintable)
    }
}

/// Which asset source an [`ItemImage`]'s path is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    /// The content step's output (`generated://`).
    Generated,
    /// The shipped assets, where the icon library lives.
    Assets,
}

/// The image an item draws, as the one rule picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemImage {
    /// Where `path` is rooted.
    pub source: ImageSource,
    /// The file under that source's root.
    pub path: String,
    /// Whether the image takes the item's colour. A painted square carries
    /// its own and is drawn as it is.
    pub tinted: bool,
}

/// The one rule that says which image an item draws, for a cell, a card
/// and every list alike: the baked picture's square icon where the content
/// step made one (a bike's own, else a weapon's or part's), else the item's
/// library symbol at the export of at least `px` pixels, else `None`, which
/// is the stand-in shape and the rule's last arm. Pure, so a test can walk
/// every shipped item through it.
#[must_use]
pub fn item_image(
    def: &ItemDefinition,
    pictures: &PictureSet,
    vehicles: &VehicleModels,
    icons: &IconCatalogue,
    px: u32,
) -> Option<ItemImage> {
    let baked = match &def.kind {
        ItemKind::Vehicle { vehicle } => vehicles.icon(vehicle).map(str::to_owned),
        _ => None,
    }
    .or_else(|| {
        let picture = def.icon.picture.as_ref()?;
        let dir = PICTURES_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
        pictures
            .icon_of(&format!("{dir}/{picture}"))
            .map(str::to_owned)
    });
    if let Some(path) = baked {
        return Some(ItemImage {
            source: ImageSource::Generated,
            path,
            tinted: true,
        });
    }
    let id = def.icon.symbol.as_deref()?;
    let path = icons.file(id, px)?;
    Some(ItemImage {
        source: ImageSource::Assets,
        path: path.to_owned(),
        tinted: icons.is_symbol(id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemKind;

    fn text(version: u32, extra: &str) -> String {
        format!(
            r#"{{"version": {version}, "icons": [
              {{"id": "a:one", "label": "One", "family": "symbol", "tintable": true,
                "status": "ready", "exports": {{"16": "s.16.png", "64": "s.png"}}}}
              {extra}]}}"#
        )
    }

    #[test]
    fn file_picks_the_smallest_export_that_is_big_enough() {
        let catalogue = IconCatalogue::parse(&text(2, "")).expect("a good catalogue");
        assert_eq!(catalogue.file("a:one", 10), Some("s.16.png"));
        assert_eq!(catalogue.file("a:one", 17), Some("s.png"));
        assert_eq!(catalogue.file("a:one", 500), Some("s.png"));
        assert_eq!(catalogue.file("a:none", 1), None);
        assert!(catalogue.is_symbol("a:one"));
        assert!(!catalogue.is_square("a:one"));
    }

    #[test]
    fn the_loader_refuses_what_it_should() {
        let entry = |id: &str, status: &str, exports: &str| {
            format!(
                r#", {{"id": "{id}", "label": "L", "family": "ability", "status": "{status}",
                  "exports": {exports}}}"#
            )
        };
        let refused = |text: String| IconCatalogue::parse(&text).expect_err("refused");
        assert_eq!(
            refused(text(3, "")),
            IconError::Version { found: 3 },
            "an unknown version"
        );
        assert_eq!(
            refused(text(2, &entry("a:one", "ready", r#"{"64": "x"}"#))),
            IconError::Duplicate("a:one".into())
        );
        assert_eq!(
            refused(text(2, &entry("a:two", "ready", "{}"))),
            IconError::NoExport("a:two".into())
        );
        assert_eq!(
            refused(text(2, &entry("a:two", "draft", r#"{"64": "x"}"#))),
            IconError::NotReady("a:two".into())
        );
        assert!(matches!(
            refused(text(2, &entry("a:two", "ready", r#"{"big": "x"}"#))),
            IconError::BadSize { .. }
        ));
        assert!(matches!(refused("{".into()), IconError::Json(_)));
    }

    #[test]
    fn every_shipped_export_exists_on_disk() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        let text = std::fs::read_to_string(format!("{root}/{CATALOGUE_FILE}"))
            .expect("the catalogue is committed");
        let catalogue = IconCatalogue::parse(&text).expect("the shipped catalogue loads");
        assert!(catalogue.len() > 200, "{} entries", catalogue.len());
        for icon in catalogue.iter() {
            for path in icon.exports.values() {
                assert!(
                    std::path::Path::new(&format!("{root}/{path}")).is_file(),
                    "`{}`: `{path}` is missing",
                    icon.id
                );
            }
        }
    }

    fn shipped() -> crate::GameRegistries {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        crate::load_from_dir(root).expect("assets/ loads and freezes")
    }

    #[test]
    fn every_shipped_item_draws_an_image_baked_or_not() {
        let registries = shipped();
        let dir = PICTURES_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut baked = PictureSet::default();
        for def in registries.items.iter() {
            let Some(picture) = &def.icon.picture else {
                continue;
            };
            let placed = crate::pictures::Placed {
                rect: [0, 0, 1, 1],
                file: format!("{dir}/{picture}"),
                icon: Some(format!("{dir}/icon.{picture}")),
            };
            baked.chassis.insert(
                picture.clone(),
                crate::pictures::ChassisPictures {
                    width: 1,
                    height: 1,
                    metres_per_pixel: 1.0,
                    origin: [0.0, 0.0],
                    whole: placed,
                    pieces: Vec::new(),
                },
            );
        }
        let bare = VehicleModels::default();
        let mut with_bike = VehicleModels::default();
        with_bike
            .icons
            .insert("veyra:vehicle.courier_bike".into(), "v/icon.png".into());
        for def in registries.items.iter() {
            let name = def.id.to_string();
            let plain = item_image(def, &PictureSet::default(), &bare, &registries.icons, 32)
                .unwrap_or_else(|| panic!("{name} draws no image with nothing baked"));
            assert_eq!(plain.source, ImageSource::Assets, "{name}");
            let id = def.icon.symbol.as_deref().expect("a symbol");
            assert_eq!(
                plain.tinted,
                registries.icons.is_symbol(id),
                "{name}: a painted square is never tinted"
            );
            let full = item_image(def, &baked, &with_bike, &registries.icons, 32)
                .unwrap_or_else(|| panic!("{name} draws no image when baked"));
            if def.icon.picture.is_some() || matches!(def.kind, ItemKind::Vehicle { .. }) {
                assert_eq!(full.source, ImageSource::Generated, "{name}");
            } else {
                assert_eq!(full, plain, "{name}");
            }
        }
    }

    #[test]
    fn a_lot_draws_its_classes_symbol_and_kinds_differ() {
        let registries = shipped();
        let mut seen = BTreeSet::new();
        for def in registries.items.iter() {
            if let ItemKind::Lot { class, .. } = &def.kind {
                let own = registries
                    .resources
                    .classes()
                    .iter()
                    .find(|own| own.id == *class)
                    .expect("a lot's class");
                assert!(own.symbol.is_some(), "{} has no symbol", own.id);
                assert_eq!(def.icon.symbol, own.symbol, "{}", def.id);
            }
            seen.insert(def.icon.symbol.clone());
        }
        assert!(seen.len() >= 12, "only {} different symbols", seen.len());
    }

    #[test]
    fn a_symbol_the_library_lacks_is_refused_by_name() {
        let registries = shipped();
        let other = IconCatalogue::parse(&text(2, "")).expect("a catalogue");
        let error = registries
            .items
            .check_symbols(&other)
            .expect_err("the shipped symbols are not in a.one");
        let said = error.to_string();
        assert!(said.contains("which the library does not hold"));
        assert!(
            registries
                .items
                .iter()
                .any(|def| def.icon.symbol.is_some() && said.contains(&def.id.to_string())),
            "{said}"
        );
    }
}
