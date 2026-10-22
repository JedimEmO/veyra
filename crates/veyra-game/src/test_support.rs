//! Fixtures the household's tests share, here and in the game binary's tests
//! (behind the `test-support` feature, which a dev-dependency turns on): the
//! shipped data, a lot as a draw would make it, a stack as a triple, and a
//! baked building's collision proxies.

use std::path::{Path, PathBuf};

use strata::registry::GameRegistries;
use strata::registry::item::{ItemKind, Record};

/// A building file's level-zero proxies placed in the building's frame, as
/// `(instance, element, hull points)`, from either version of the file: the
/// cooked file the content step writes for the game, or the full one
/// `content --full-buildings` writes. The names are what a walkability
/// test filters stairs by, which the `.collision` file does not carry.
#[must_use]
#[allow(clippy::unwrap_used, reason = "the baked files of the layout")]
pub fn placed_proxies(path: &Path) -> Vec<(String, String, Vec<ashlar::glam::DVec3>)> {
    let bytes = std::fs::read(path).unwrap();
    if ashlar::CookedBuilding::is_cooked(&bytes) {
        return ashlar::CookedBuilding::read(&bytes)
            .unwrap()
            .colliders()
            .into_iter()
            .map(|c| (c.instance, c.element, c.solid.vertices))
            .collect();
    }
    ashlar::BakedBuilding::read(&bytes)
        .unwrap()
        .level(0)
        .unwrap()
        .colliders()
        .into_iter()
        .map(|c| (c.instance, c.element, c.solid.vertices))
        .collect()
}

/// The shipped `assets/`, loaded from disk.
///
/// # Panics
/// When the data does not load.
#[must_use]
pub fn shipped() -> GameRegistries {
    strata::registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("assets/ loads")
}

/// A module the bench tests fit.
pub const FEED: &str = "veyra:module.patrol_extended_feed";

/// A stack as `(item, count, batch)`.
pub fn stack(record: Option<&Record>) -> Option<(String, u32, u32)> {
    record.map(|record| (record.item.to_string(), record.count, record.batch))
}

/// `path` with `suffix` after its file name: the backup a mend keeps.
#[must_use]
pub fn beside(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().expect("a file").to_string_lossy();
    path.with_file_name(format!("{name}{suffix}"))
}

/// A lot of `resource` as a draw would make it, from a deposit made up
/// for the test: every figure a fifth of the way up its span.
pub fn lot_of(registries: &GameRegistries, resource: &str, cell: [i32; 2], units: u32) -> Record {
    let def = registries
        .items
        .lot_of(&resource.parse().expect("a valid id"))
        .expect("a lot a resource type");
    let ItemKind::Lot { ranges, .. } = &def.kind else {
        panic!("{resource} has a lot");
    };
    Record {
        item: def.id.clone(),
        count: units,
        batch: 0,
        by: None,
        lines: ranges
            .iter()
            .map(|(attribute, span)| {
                let value = span.floor + (span.cap - span.floor) / 5;
                (attribute.key().to_owned(), value)
            })
            .collect(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: Some(strata::registry::item::LotOrigin {
            deposit: cell,
            root: "Calder".to_owned(),
            place: "Calder flats".to_owned(),
        }),
    }
}
