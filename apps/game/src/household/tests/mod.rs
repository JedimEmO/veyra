//! The household's tests, by subject, and the helpers more than one of them
//! uses. Separate files because the one module they sat in was 2,800 lines.

use super::*;
use clap::Parser;
use strata::registry::progression::{AuthoredSheets, XpKind};

mod bar;
mod fall;
mod harvest;
mod props;
mod rework;
mod runs;
mod save;
mod script;
mod slots;

pub(super) fn shipped() -> GameRegistries {
    strata::registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        .expect("assets/ loads")
}

pub(super) const FEED: &str = "veyra:module.patrol_extended_feed";

/// A stack as `(item, count, batch)`.
pub(super) fn stack(record: Option<&Record>) -> Option<(String, u32, u32)> {
    record.map(|record| (record.item.to_string(), record.count, record.batch))
}

pub(super) fn beside(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().expect("a file").to_string_lossy();
    path.with_file_name(format!("{name}{suffix}"))
}

/// A lot of `resource` as a draw would make it, from a deposit made up
/// for the test: every figure a fifth of the way up its span.
pub(super) fn lot_of(
    registries: &GameRegistries,
    resource: &str,
    cell: [i32; 2],
    units: u32,
) -> Record {
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
