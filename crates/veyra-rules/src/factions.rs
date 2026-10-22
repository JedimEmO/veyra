//! Who holds the ground the local player is on (ticket 0012, step 1).
//!
//! The world's factions are `veyra_world::factions`, a pure function of
//! the seed, the placed sites and the faction data; the game publishes them
//! as [`Factions`] wherever the world exists. [`HeldHere`] is what a screen
//! reads of them: the territory at the local player and the holder of the
//! site they stand on, made on the task pool, never on a frame, because a
//! land mass no one has baked is baked by whoever asks.

use std::sync::Arc;

use bevy_ecs::prelude::*;

pub use veyra_world::factions::{Faction, FactionId, FactionMap, Holding, Territory};

use strata_types::NearSite;
use strata_types::NearSites;
use strata_worldgen::sites::SKIRT_M;
use strata_worldgen::sites::SiteId;

/// How far the player moves before [`HeldHere`] is made again, metres,
/// unless they cross onto or off a site first. Far under a holding's reach,
/// so a border is noticed within a short walk.
pub const HELD_REFRESH_M: f64 = 100.0;

/// The world's factions, published by the game once its world exists. A
/// server and a harness have none.
#[derive(Resource, Debug, Clone)]
pub struct Factions(pub Arc<FactionMap>);

/// What holds the ground the local player stands on.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct HeldHere {
    /// Where it was made, `(x, z)`; `None` before the first.
    pub at: Option<[f64; 2]>,
    /// Whose ground that is, and how strongly.
    pub territory: Option<Territory>,
    /// The holder of the ground: `None` for unclaimed wilds.
    pub holder: Option<Faction>,
    /// The site the player stands on (its pad and the skirt past it).
    pub site: Option<SiteId>,
    /// The holder of that site: `None` for a site of a kind no faction
    /// holds, or no site.
    pub site_holder: Option<Faction>,
}

/// The near site whose laid ground `(x, z)` stands on: within its pad's
/// radius and the skirt past it. The place card asks this same function, so
/// the card and [`HeldHere`] never disagree about where the member is.
#[must_use]
pub fn site_at(near: &NearSites, x: f64, z: f64) -> Option<&NearSite> {
    near.sites.iter().find(|near| {
        let (dx, dz) = (x - near.site.centre[0], z - near.site.centre[1]);
        let reach = near.site.radius_m + SKIRT_M;
        dx * dx + dz * dz <= reach * reach
    })
}

/// The id of [`site_at`].
#[must_use]
pub fn standing_on(near: &NearSites, x: f64, z: f64) -> Option<SiteId> {
    site_at(near, x, z).map(|near| near.site.id)
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use strata_types::NearSite;

    use strata_worldgen::sites::ProceduralSite;

    pub fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    pub fn near_site(site: ProceduralSite) -> NearSite {
        NearSite {
            site,
            kind: String::new(),
            kind_name: String::new(),
            placement: None,
        }
    }
}
