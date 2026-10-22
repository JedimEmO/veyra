//! Names the render-side crates share: the camera markers the render crates
//! read and the screens put on a camera of their own, and the client's set
//! names.
//!
//! They live here, below every render crate, so a screen crate that draws a
//! model on a stage can mark its camera without linking the terrain or the
//! atmosphere it asks to leave that camera alone, and a screen that joins
//! [`ClientSet::Follow`] needs not link the cameras that configure it.

use bevy::ecs::component::Component;
use bevy::ecs::schedule::SystemSet;

/// The client sets, in order, in `Update`, after streaming has decided.
/// `strata-client` configures them; the screens and the combat view join them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientSet {
    /// Camera input, the player's intent, new player visuals.
    Camera,
    /// The debug overlay.
    Hud,
    /// In `PostUpdate` after the origin sync: the orbit camera and the
    /// player visuals placed from the interpolated sim positions.
    Follow,
}

/// Marks a `Camera3d` that does not look at the world: a model turning on
/// a screen, a portrait render.
///
/// The terrain dresses and reads cameras by `Camera3d` alone, because the
/// world has one. A second camera drawing something else would otherwise
/// be given the water's depth prepass and the probes' cluster bound, and,
/// drawn before the world's, be the one the probes' fallback and the
/// foliage's levels follow. The crates above the terrain tell the world's
/// camera from another by this too.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OffWorld;

/// Opt a `Camera3d` out of the sky, exposure and tonemapping.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NoSky;
