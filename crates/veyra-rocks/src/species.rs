//! Veyra's rock species, in registry order.
use crate::{Detail, Geology, RockKind, RockRecipe};

/// Every species the content step shapes, in registry order.
#[must_use]
pub fn recipes() -> Vec<RockRecipe> {
    vec![
        RockRecipe {
            species: "strata:granite_boulder",
            material: "granite",
            kind: RockKind::Boulder,
            geology: Geology::Granite,
            detail: Detail::Boulder,
            size: [1.2, 3.5],
            moss: 0.85,
        },
        RockRecipe {
            species: "strata:granite_stone",
            material: "granite",
            kind: RockKind::Boulder,
            geology: Geology::Granite,
            detail: Detail::Stone,
            size: [0.3, 0.9],
            moss: 0.6,
        },
        RockRecipe {
            species: "strata:sandstone_boulder",
            material: "sandstone",
            kind: RockKind::Boulder,
            geology: Geology::Sandstone,
            detail: Detail::Boulder,
            size: [1.0, 3.0],
            moss: 0.5,
        },
        RockRecipe {
            species: "strata:sandstone_stone",
            material: "sandstone",
            kind: RockKind::Boulder,
            geology: Geology::Sandstone,
            detail: Detail::Stone,
            size: [0.3, 0.8],
            moss: 0.35,
        },
        RockRecipe {
            species: "strata:granite_cliff",
            material: "granite",
            kind: RockKind::Cliff,
            geology: Geology::Granite,
            detail: Detail::Cliff,
            size: [8.0, 25.0],
            moss: 0.8,
        },
        RockRecipe {
            species: "strata:sandstone_cliff",
            material: "sandstone",
            kind: RockKind::Cliff,
            geology: Geology::Sandstone,
            detail: Detail::Cliff,
            size: [8.0, 25.0],
            moss: 0.45,
        },
    ]
}
