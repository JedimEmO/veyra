//! The crossings' pieces as colliders, on the server as on the client
//! (ticket 0005, step 7).
//!
//! [`NearCrossings`] says which piece of the kit stands where at every
//! crossing near an observer, the same list the client draws from. A piece
//! whose crossing is within [`COLLIDE_M`] of an observer gets one static
//! body, a compound of its baked file's nearest level's proxies (the deck
//! slices, the kerbs and the rails each their own, so a walker stands on
//! the deck, steps onto a kerb, is stopped by a rail and walks under the
//! deck where there is room), and loses it past [`RELEASE_M`]. A file is
//! read once, on the task pool, and shared by every crossing that stands
//! it; a file that is missing or unreadable is said once and its pieces do
//! not collide.

use std::collections::BTreeSet;

use bevy::ecs::prelude::*;
use bevy::math::Quat;
use strata_core::WorldPos;
use strata_sim::crossings::{CrossingPieceKey, NearCrossings, PlacedPiece, pieces_within};
use strata_sim::{GeneratedRoot, Observers, RenderOrigin};

use crate::sites::{Streamed, Wanted};

/// A crossing's piece collides within this of an observer, metres, measured
/// to the crossing: as a site's buildings do.
pub const COLLIDE_M: f64 = crate::sites::COLLIDE_M;

/// And stops colliding past this, metres.
pub const RELEASE_M: f64 = crate::sites::RELEASE_M;

/// A crossing's piece's static body.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct CrossingCollider {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// The piece's key: its crossing's id key and its index there.
    pub key: CrossingPieceKey,
}

/// Where a piece stands and how it is turned: the one pose its body here
/// and its drawing on a client are both given.
#[must_use]
pub fn piece_pose(piece: &PlacedPiece) -> (WorldPos, Quat) {
    #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
    let rotation = Quat::from_rotation_y(piece.yaw as f32);
    (
        WorldPos::new(piece.at[0], piece.at[1], piece.at[2]),
        rotation,
    )
}

/// The crossing bodies spawned, the files read, and the read in flight.
#[derive(Resource, Default)]
pub struct CrossingBodies(Streamed<CrossingPieceKey>);

/// Keeps a body for every crossing's piece within [`COLLIDE_M`] of an
/// observer and none past [`RELEASE_M`], reading the files they need first.
pub(crate) fn stream_crossing_colliders(
    mut commands: Commands,
    near: Option<Res<NearCrossings>>,
    observers: Res<Observers>,
    generated: Option<Res<GeneratedRoot>>,
    origin: Res<RenderOrigin>,
    mut state: ResMut<CrossingBodies>,
) {
    let Some(near) = near else {
        return;
    };
    let positions: &[WorldPos] = &observers.positions;
    let keep: BTreeSet<CrossingPieceKey> = pieces_within(&near, positions, RELEASE_M)
        .map(|(key, _, _)| key)
        .collect();
    let wanted = pieces_within(&near, positions, COLLIDE_M).map(|(key, _, piece)| {
        let (at, rotation) = piece_pose(piece);
        Wanted {
            key,
            file: piece.file.as_str(),
            at,
            rotation,
            marker: move || CrossingCollider {
                file: piece.file.clone(),
                key,
            },
        }
    });
    let root = generated.and_then(|g| g.0.clone());
    state
        .0
        .stream(&mut commands, &keep, wanted, root, &origin, "crossing");
}
