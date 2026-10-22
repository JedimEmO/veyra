//! The game's registries at their old paths: a facade over `strata-data`
//! (the engine's data layer and the world's schemas) and `veyra-data`
//! (Veyra's own schemas and the loader that joins them into
//! [`GameRegistries`]).
//!
//! The world crates (`strata-worldgen`, `strata-flora`, `strata-weather`)
//! link `strata-data` directly so a game-schema edit does not rebuild them;
//! everything above them keeps naming `strata_registry::...` and sees both
//! halves as one. No Bevy.
//!
//! ```no_run
//! let registries = strata_registry::load_from_dir("assets")?;
//! let plains = registries
//!     .biome_id(&strata_registry::Namespaced::parse("strata:plains")?)
//!     .expect("shipped in assets/biomes");
//! let surface = registries.surface(plains).expect("resolved at freeze");
//! println!("plains paint texture layer {}", surface.material.layer());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub use strata_data::*;
pub use veyra_data::*;

// Both halves have a `load` module and a `LoadError`; the game's is the
// superset (it converts from the world's), so it takes the plain names.
pub use veyra_data::{LoadError, load};

/// The pre-split `registries` module: the world's open set and the
/// game's frozen one.
pub mod registries {
    pub use strata_data::defs::*;
    pub use veyra_data::registries::*;
}
