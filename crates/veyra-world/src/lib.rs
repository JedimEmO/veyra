//! Veyra's rules over the world: where the ground holds a resource and who
//! holds a place.
//!
//! These read the game's sets (`veyra-data`: the resource types and the
//! faction kinds) over the engine's world (`strata-worldgen`), so they sit
//! above both and the world crates link none of the game's schemas. Neither
//! is in `world_version` or the hydrology bake: a deposit is a pure function
//! of the seed, that data and the samples, and the factions are made per
//! land mass from the sites. No Bevy.

pub mod deposits;
pub mod factions;

pub use deposits::{Deposit, DepositField, DepositId};
pub use factions::{Faction, FactionId, FactionMap, Holding, Territory};
