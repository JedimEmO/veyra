//! Where the weather is, below the render layer.
//!
//! Fog is part of the world: a server decides what a creature can see
//! through it, and two players standing together must see the same valley
//! full (ticket 0003). So where fog lies is a pure function of the world,
//! the seed and a [`Weather`], in a crate with no Bevy that the server links
//! as well as the client, and a renderer only draws what it says.
//!
//! The function has two halves with very different costs:
//!
//! - [`BasinTile::bake`] reads the ground of one 8 km tile (about 23
//!   thousand [`WorldSource::sample`](strata_worldgen::WorldSource::sample)
//!   calls) and finds where cold air would pool, the level it would fill
//!   to and how wet the ground is. It depends on nothing but the world, so
//!   it is baked once per tile, off the frame, and kept.
//! - [`FogField::sample`] turns the baked tiles and a [`Weather`] into a
//!   [`FogSample`] at any point: a bilinear lookup and a few noise octaves,
//!   cheap enough to fill a texture whenever the weather moves.
//!
//! A renderer's loop, around an observer at `(x, z)` that wants fog over
//! `r` metres:
//!
//! ```no_run
//! # use strata_weather::{BasinTile, FogField, Weather};
//! # fn run(world: &dyn strata_worldgen::WorldSource, seed: strata_core::Seed, x: f64, z: f64) {
//! let mut field = FogField::new(seed);
//! let r = 32_000.0;
//! for coord in field.missing(x - r, z - r, x + r, z + r) {
//!     // On a task pool in a real renderer; the bake is pure.
//!     field.insert(BasinTile::bake(world, coord)).expect("a bake is well formed");
//! }
//! let mut texels = Vec::new();
//! field.sample_grid(x - r, z - r, 250.0, 256, &Weather::FULL, &mut texels);
//! # }
//! ```
//!
//! Determinism: the bake and the sample use only `+ - * /`, `sqrt`,
//! `floor`, `min`, `max` and integer path lengths, and the noises are
//! `strata-worldgen`'s own, so the same seed gives the same bits on every
//! machine (the crate calls no transcendental at all). No answer depends on
//! a hash map's order. A test records a tile's hash, so a change that moves
//! the field fails until someone blesses it.

mod field;
mod grid;
mod tile;

pub use field::{FogField, FogSample, Weather};
pub use tile::{
    APRON_CELLS, BakeStats, BasinTile, CELL_M, DEPTH_RADIUS_CELLS, DOMAIN_NODES,
    HYDRATION_RADIUS_CELLS, MalformedTile, OFFSHORE_CAP_M, POOL_RADIUS_CELLS, SEA_PASS_M,
    SEA_REACH_M, SMOOTH_RADIUS_CELLS, TILE_CELLS, TILE_M, TILE_NODES, TOP_FRACTION, TileCoord,
};
