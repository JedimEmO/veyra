//! What crosses the wire, registered in one place in one order.
//!
//! `bevy_replicon` hashes the registration order of components and events
//! into a protocol id and refuses a client whose order differs, so the
//! server and the client both call [`register_protocol`] and nothing else
//! registers anything. The types themselves are the sim's where they can
//! be (`WorldPosition`, `Player`, `PlayerIntent` derive serde in
//! `strata-sim`); the three defined here are the adapter's own: the clock
//! carrier, the greeting and the intent envelope. Last comes
//! [`Versions`], which a server sends a client it
//! refuses so the client can say why.

use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy_replicon::bytes::Bytes;
use bevy_replicon::prelude::{
    AppMarkerExt, AppRuleExt, Channel, ClientEventAppExt, RuleFns, ServerEventAppExt,
};
use bevy_replicon::shared::replication::deferred_entity::DeferredEntity;
use bevy_replicon::shared::replication::registry::ctx::WriteCtx;
use bevy_replicon::shared::replication::registry::receive_fns;
use strata_core::{SimTime, WorldPos};
use strata_sim::{Player, PlayerId, PlayerIntent, WorldPosition};

use crate::handshake::Versions;
use serde::{Deserialize, Serialize};

/// The server's clock, as a component on one replicated entity.
///
/// Replicon 0.44 can replicate a resource, but only onto a client that does
/// not already hold it, and every client holds a `SimClock` from the moment
/// `SimPlugin` builds. So the clock rides on a singleton entity the server
/// spawns and the client copies it into its own `SimClock` (and day
/// settings) whenever it arrives. The day settings travel with it because
/// the sim owns them (PLAN 4.1): a server that starts at dawn tells every
/// client so.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReplicatedClock {
    /// The server's simulation time at the tick it was sent.
    pub time: SimTime,
    /// The server's `DayLength`.
    pub day_length: f64,
    /// The server's `DayStart`.
    pub day_start: f64,
}

/// Sent to a client once its player exists: who it is and what world it
/// should have loaded.
///
/// The client already knows its own id (it is the network id it connected
/// with, see [`crate::client`]), so this confirms rather than informs, and
/// the seed lets a client that loaded a different world say so in its log
/// instead of walking on terrain the server does not have.
#[derive(Event, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    /// The id of the player the server spawned for this client.
    pub player: PlayerId,
    /// The server's world seed.
    pub seed: u64,
}

/// A client's intent for its player, one per sim tick, client to server.
///
/// The command the client's own tick consumed rather than the raw intent,
/// so the jump edge the local tick saw is the one the server sees, with
/// the sequence number the client gave that tick. The server applies one
/// per tick in order and reports the last one it applied in
/// [`AppliedInput`], which is what lets the client compare its prediction
/// for that tick with the server's result instead of with "now".
#[derive(Event, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerIntentEvent {
    /// The command.
    pub intent: PlayerIntent,
    /// The client's count of intents sent, from one.
    pub seq: u64,
}

/// On a player, the sequence number of the last [`PlayerIntentEvent`] the
/// server's tick consumed. Zero until the first one. Replicated with the
/// position so the two arrive together.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AppliedInput {
    /// The sequence number.
    pub seq: u64,
}

/// On a client, the entity whose `WorldPosition` this process predicts:
/// the local player. Received positions land in [`ServerPosition`] instead
/// of overwriting the prediction (a receive marker in replicon's terms).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Predicted;

/// The last position the server sent for a [`Predicted`] entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ServerPosition(pub WorldPos);

/// Registers every replicated component, event and receive marker, in the
/// order the protocol hash expects. Called by both roles.
pub fn register_protocol(app: &mut App) {
    app.replicate::<WorldPosition>()
        .replicate::<Player>()
        .replicate::<ReplicatedClock>()
        .replicate::<AppliedInput>()
        .add_server_event::<Welcome>(Channel::Ordered)
        .add_client_event::<PlayerIntentEvent>(Channel::Ordered)
        .register_marker::<Predicted>()
        .set_marker_fns::<Predicted, WorldPosition>(
            write_server_position,
            receive_fns::default_remove::<WorldPosition>,
        )
        // Independent: it goes to a client replicon has not authorised
        // and is about to disconnect, as its own `ProtocolMismatch` does.
        .add_server_event::<Versions>(Channel::Ordered)
        .make_event_independent::<Versions>();
}

/// The receive function for `WorldPosition` on a [`Predicted`] entity:
/// the value goes into [`ServerPosition`] and the predicted position is
/// left to the reconciliation in [`crate::client`].
fn write_server_position(
    ctx: &mut WriteCtx,
    rule_fns: &RuleFns<WorldPosition>,
    entity: &mut DeferredEntity,
    message: &mut Bytes,
) -> bevy::ecs::error::Result<()> {
    let position: WorldPosition = rule_fns.deserialize(ctx, message)?;
    if let Some(mut server) = entity.get_mut::<ServerPosition>() {
        server.0 = position.0;
    } else {
        entity.insert(ServerPosition(position.0));
    }
    Ok(())
}
