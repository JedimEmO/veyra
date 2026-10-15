//! The client's screens and HUD, headless: no window, no GPU, virtual
//! time, through `slotted-test`'s harness over the same `ClientPlugin` the
//! window runs.
//!
//! The harness brings Bevy's UI stack without a renderer; the sim and the
//! streamer come with no world loader, so the app falls through to
//! `InWorld` on its own and the HUD systems run. The theme is the real
//! file under `assets/themes/`, so a broken copy fails here before it
//! fails in a window.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

mod board;
mod card;
mod common;
mod crafting;
mod crafting_edges;
mod factory;
mod fitting;
mod loadout;
mod map;
mod pause;
mod rework;
mod ride;
mod stores;
mod survey;
mod weapon_diagram;
mod wear;
