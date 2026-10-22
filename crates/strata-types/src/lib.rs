//! The plain data the engine core writes and the game's rules read: what a
//! player wants to do ([`PlayerIntent`], [`PlayerCommand`]), the sites near
//! the observers ([`NearSite`], [`NearSites`]), the water at a point
//! ([`WaterHere`]), the frozen registries ([`Registries`]) and the combat
//! layer's hold on the character motor ([`CombatMotion`]).
//!
//! They live below `strata-app` so the game's plain rules crate can take them
//! without linking `bevy_app`; `strata-app` re-exports each at its old path.
//! Only `bevy_ecs`, `bevy_math` and the Bevy-free world crate are linked.

use bevy_ecs::prelude::*;
use bevy_math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strata_registry::GameRegistries;
use strata_worldgen::sites::{ProceduralSite, SitePlacement};

/// What a player wants to do, as written by the client's input or the
/// network. Overwritten freely; consumed once per tick into a
/// [`PlayerCommand`]. Serialisable because a networked client sends it to
/// the server as it is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayerIntent {
    /// Where to go in the player's own frame: `x` to the right, `y`
    /// forward, at most unit length. Zero stands still.
    pub move_dir: Vec2,
    /// The heading the frame is relative to, radians about `y`: zero faces
    /// `-z`, positive turns towards `-x`, the same convention as the
    /// cameras.
    pub yaw: f32,
    /// A jump was asked for since the last tick. Cleared when consumed.
    pub jump: bool,
    /// Run rather than walk.
    pub sprint: bool,
}

impl PlayerIntent {
    /// The unit horizontal direction a yaw faces.
    #[must_use]
    pub fn forward(yaw: f32) -> Vec3 {
        Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
    }

    /// The unit horizontal direction to the right of a yaw.
    #[must_use]
    pub fn right(yaw: f32) -> Vec3 {
        Vec3::new(yaw.cos(), 0.0, -yaw.sin())
    }

    /// The wanted direction in world axes, at most unit length.
    #[must_use]
    pub fn world_direction(&self) -> Vec3 {
        (Self::forward(self.yaw) * self.move_dir.y + Self::right(self.yaw) * self.move_dir.x)
            .clamp_length_max(1.0)
    }
}

/// The intent the current tick acts on: a copy of [`PlayerIntent`] taken in
/// `SimSet::Input`. Systems in
/// `SimSet::Step` read this and never the intent, so
/// a client writing its intent mid-tick cannot change a tick half way.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerCommand(pub PlayerIntent);

/// One site near an observer and what stands on it.
#[derive(Debug, Clone, PartialEq)]
pub struct NearSite {
    /// The site.
    pub site: ProceduralSite,
    /// Its kind's registry name.
    pub kind: String,
    /// What its kind is called, for a player.
    pub kind_name: String,
    /// Its group stood on it, or `None` when its kind's group is missing or
    /// has no arrangement that fits.
    pub placement: Option<SitePlacement>,
}

/// The sites within `NEAR_SITES_M` of the observers, by id, and where the
/// list was last made round.
#[derive(Resource, Debug, Default, Clone)]
pub struct NearSites {
    /// The sites, ordered by id: the same order everywhere.
    pub sites: Vec<NearSite>,
    /// The observers' places when it was made, `(x, z)`.
    pub around: Vec<[f64; 2]>,
}

/// The water at one point: its surface, the ground under it and how it
/// moves. Metres and metres a second, in the world's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterHere {
    /// The surface, metres. The sea's level where no river or lake stands
    /// higher, which may be under the ground: a body above it is dry.
    pub surface_m: f32,
    /// The ground the sample puts under the point, metres.
    pub ground_m: f32,
    /// Downstream times the mean speed, `(x, z)`; zero in the sea and in a
    /// lake.
    pub flow: [f32; 2],
    /// Whether the water is a fall's: it carries what floats in it down.
    pub fall: bool,
}

impl WaterHere {
    /// How deep the water stands over the ground, metres; at most zero
    /// where the ground is dry.
    #[must_use]
    pub fn depth_m(&self) -> f32 {
        self.surface_m - self.ground_m
    }
}

/// The frozen game registries, shared read-only across threads.
///
/// Inserted by the loader before it releases its `LoadingHold`, or as an
/// empty set on entering `InWorld` when nothing loaded anything, so systems
/// in the world can take `Res<Registries>` unconditionally. An `Arc` because
/// terrain meshing tasks on the compute pool need to read it as well.
#[derive(Resource, Debug, Clone)]
pub struct Registries(pub Arc<GameRegistries>);

impl Default for Registries {
    fn default() -> Self {
        Self(Arc::new(GameRegistries::default()))
    }
}

/// A layer above the character motor constrains it without changing its collision
/// policy: the combat layer scales the walk and lends an authored displacement. It
/// lives here, beside the intent the motor reads, so the physics crate takes it
/// without linking the game's rules.
#[derive(Component, Debug, Clone, Copy)]
pub struct CombatMotion {
    /// Analog locomotion multiplier when no authored displacement is active.
    pub speed_scale: f32,
    /// Exact average velocity of this tick's authored displacement curve.
    pub velocity: Option<Vec3>,
}

impl Default for CombatMotion {
    fn default() -> Self {
        Self {
            speed_scale: 1.,
            velocity: None,
        }
    }
}
