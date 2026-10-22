//! The roadside's markers and lamps as colliders, on the server as on the
//! client (ticket 0005, step 8).
//!
//! [`NearRoadside`] says which piece of the roadside kit stands where along
//! the roads near an observer, the same list the client draws from. A piece
//! within [`COLLIDE_M`] of an observer, measured to the piece, gets one
//! static body from its baked file's proxies (a marker's post, a lamp's
//! pole and foot), and loses it past [`RELEASE_M`], as a crossing's pieces
//! do through the same `Streamed` helper.

use std::collections::BTreeSet;

use bevy::ecs::prelude::*;
use bevy::math::Quat;
use strata_app::roadside::{NearRoadside, RoadsidePieceKey, pieces_within};
use strata_app::{GeneratedRoot, Observers, RenderOrigin};
use strata_core::WorldPos;

use crate::sites::{Streamed, Wanted};

/// A roadside piece collides within this of an observer, metres, measured
/// to the piece: a walker meets a post long before this, and a flight
/// holds a few dozen.
pub const COLLIDE_M: f64 = 400.0;

/// And stops colliding past this, metres.
pub const RELEASE_M: f64 = 550.0;

/// A roadside piece's static body.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct RoadsideCollider {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// The piece's key: its road's id key and its index along the road.
    pub key: RoadsidePieceKey,
}

/// Where a roadside piece stands and how it is turned: the one pose its
/// body here and its drawing on a client are both given.
#[must_use]
pub fn piece_pose(piece: &strata_worldgen::crossings::PlacedPiece) -> (WorldPos, Quat) {
    crate::crossings::piece_pose(piece)
}

/// The roadside bodies spawned, the files read, and the read in flight.
#[derive(Resource, Default)]
pub struct RoadsideBodies(Streamed<RoadsidePieceKey>);

/// Keeps a body for every roadside piece within [`COLLIDE_M`] of an
/// observer and none past [`RELEASE_M`], reading the files they need first.
pub(crate) fn stream_roadside_colliders(
    mut commands: Commands,
    near: Option<Res<NearRoadside>>,
    observers: Res<Observers>,
    generated: Option<Res<GeneratedRoot>>,
    origin: Res<RenderOrigin>,
    mut state: ResMut<RoadsideBodies>,
) {
    let Some(near) = near else {
        return;
    };
    let positions: &[WorldPos] = &observers.positions;
    let keep: BTreeSet<RoadsidePieceKey> = pieces_within(&near, positions, RELEASE_M)
        .map(|p| p.key)
        .collect();
    let wanted = pieces_within(&near, positions, COLLIDE_M).map(|p| {
        let (at, rotation) = piece_pose(&p.piece);
        let key = p.key;
        Wanted {
            key,
            file: p.piece.file.as_str(),
            at,
            rotation,
            marker: move || RoadsideCollider {
                file: p.piece.file.clone(),
                key,
            },
        }
    });
    let root = generated.and_then(|g| g.0.clone());
    state.0.stream(
        &mut commands,
        &keep,
        wanted,
        root,
        &origin,
        "roadside piece",
    );
}
