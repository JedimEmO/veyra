//! The client: connects, adopts the replicated player that is its own,
//! sends its intents, predicts its own movement and follows the server for
//! everyone else's.
//!
//! # Prediction, and what it is not
//!
//! The local player runs the same physics as on the server: it has a
//! `CharacterController`, its intent is consumed into a command by the
//! sim's tick and the physics steps it, so movement is immediate. What the
//! server sends for that entity lands in [`ServerPosition`] (a receive
//! marker, see [`crate::protocol`]) rather than over the prediction. Every
//! intent the client sends carries a sequence number, the server applies
//! one per tick and echoes the last sequence it applied with the position
//! ([`AppliedInput`]), and the client keeps the position it predicted after
//! each sequence for the last [`PredictionConfig::history`] ticks. So
//! [`reconcile_local_player`] compares the server's position with the
//! client's prediction for the same input, not with "now": the same
//! physics over the same colliders from the same commands gives the same
//! answer to within float noise, and an error over
//! [`PredictionConfig::snap_distance`] means the two really diverged (a
//! dropped input, a collider one side had and the other did not), at which
//! point the player is corrected by that error.
//!
//! The correction is a shift, not a set. The server's position is for an
//! input several ticks old; setting the player to it would rewind those
//! ticks of movement, and the next ack would then find the prediction
//! wrong by exactly what was rewound, and so on: one snap while walking
//! became a correction every few ticks for as long as the walk lasted.
//! Shifting the player and every recorded prediction by the error keeps
//! the unacked ticks and makes the next comparison exact again, so a
//! correction happens only when the two really diverged.
//!
//! That is still not a reconciliation. Full reconciliation would rewind
//! the local player to the server's state at the acked sequence and replay
//! the intents after it through the physics step, which means the
//! character step has to be callable outside the schedule for several
//! ticks in a row with the colliders of the rewind position present. The
//! ground streams in around the player, so the colliders are there; the
//! callable step is the missing piece and is noted in PLAN section 6.
//!
//! Remote players carry no controller and no physics on the client; their
//! `WorldPosition` is written by replication at the server's tick rate and
//! the client draws them interpolated between the last two ticks by the
//! same `PreviousWorldPosition` the local player uses.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::SystemTime;

use bevy::ecs::prelude::*;
use bevy::state::state::State;
use bevy_replicon::prelude::{
    ClientState, ClientStats, ClientTriggerExt, Remote, RepliconChannels,
};
use bevy_replicon_renet::netcode::{ClientAuthentication, NetcodeClientTransport};
use bevy_replicon_renet::renet::ConnectionConfig;
use bevy_replicon_renet::{RenetChannelsExt, RenetClient};
use strata_app::{
    DayLength, DayStart, LocalPlayer, Player, PlayerCommand, PlayerId, SimClock, WorldObserver,
    WorldPosition, WorldSeed,
};
use strata_core::WorldPos;
use strata_net::NetStatus;
use strata_physics::CharacterController;

use crate::protocol::{
    AppliedInput, PlayerIntentEvent, Predicted, ReplicatedClock, ServerPosition, Welcome,
};
use crate::server::PROTOCOL_ID;

/// The server to connect to.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerAddress(pub SocketAddr);

/// The id of the player this client drives: the network id it connected
/// with, inserted at connect time, so the player is recognised the frame it
/// arrives. A test without a transport inserts it by hand.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalPlayerId(pub PlayerId);

/// A replicated player that is somebody else's. Drawn, never stepped, not
/// an observer: the terrain streams around the local player only.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemotePlayer;

/// When a prediction counts as wrong.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PredictionConfig {
    /// A server position further than this from the prediction for the
    /// same input corrects the player. Half a metre: a tick of sprinting.
    pub snap_distance: f64,
    /// How many ticks of predictions are kept for the comparison; 32 is
    /// 1.6 s, more than a round trip on any link worth playing on.
    pub history: usize,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            snap_distance: 0.5,
            history: 32,
        }
    }
}

/// The local player's predicted position after each sent input, by
/// sequence, newest last.
#[derive(Component, Debug, Clone, Default)]
pub struct PredictionHistory(pub VecDeque<(u64, WorldPos)>);

/// How many intents this client has sent; the sequence of the next one is
/// one more.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputSequence(pub u64);

/// How many times the prediction was corrected to the server, for a HUD
/// or a log.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct PredictionStats {
    /// Corrections so far.
    pub snaps: u32,
    /// The error that caused the last correction, metres.
    pub last_error: f32,
}

/// Opens the socket and inserts renet's client. Runs on entering the world:
/// the terrain has to exist before a replicated player can be placed on it.
pub fn connect(
    mut commands: Commands,
    address: Res<ServerAddress>,
    channels: Res<RepliconChannels>,
) {
    let client = RenetClient::new(ConnectionConfig {
        server_channels_config: channels.server_configs(),
        client_channels_config: channels.client_configs(),
        ..Default::default()
    });
    let current_time = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("the clock is before 1970, not connecting: {e}");
            return;
        }
    };
    // The id is what netcode's unsecure authentication takes; with a
    // token server it would be assigned. Nanoseconds keep two clients
    // started in the same millisecond apart.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the low 64 bits of the epoch nanoseconds are as good an id as any"
    )]
    let client_id = current_time.as_nanos() as u64;
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("could not open a UDP socket: {e}");
            return;
        }
    };
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr: address.0,
        user_data: None,
    };
    match NetcodeClientTransport::new(current_time, authentication, socket) {
        Ok(transport) => {
            tracing::info!("connecting to {} as client {client_id}", address.0);
            commands.insert_resource(LocalPlayerId(PlayerId(client_id)));
            commands.insert_resource(client);
            commands.insert_resource(transport);
        }
        Err(e) => tracing::error!("could not start the netcode transport: {e}"),
    }
}

/// The server's greeting: confirms the id and warns when the worlds differ.
pub fn on_welcome(
    welcome: On<Welcome>,
    mut commands: Commands,
    local: Option<Res<LocalPlayerId>>,
    seed: Res<WorldSeed>,
) {
    match local {
        Some(local) if local.0 != welcome.player => tracing::warn!(
            "the server calls this client player {} but it connected as {}",
            welcome.player.0,
            local.0.0
        ),
        Some(_) => {}
        None => commands.insert_resource(LocalPlayerId(welcome.player)),
    }
    if welcome.seed == seed.0.0 {
        tracing::info!(
            "welcomed as player {}, seed {}",
            welcome.player.0,
            welcome.seed
        );
    } else {
        tracing::warn!(
            "the server runs seed {} but this client loaded seed {}; the ground will not match",
            welcome.seed,
            seed.0.0
        );
    }
}

/// Every replicated player is either ours (the local, predicted player
/// with its own controller: the same entity the input drives in a single
/// process) or somebody else's (drawn, not stepped, not an observer).
/// Untagged players rather than newly added ones, so a player that arrived
/// before the id was known is picked up when it is.
pub fn adopt_players(
    mut commands: Commands,
    local: Option<Res<LocalPlayerId>>,
    players: Query<(Entity, &Player), (With<Remote>, Without<LocalPlayer>, Without<RemotePlayer>)>,
) {
    let Some(local) = local else {
        return;
    };
    for (entity, player) in &players {
        if player.id == local.0 {
            tracing::info!(
                "player {entity} (id {}) is ours; predicting it",
                player.id.0
            );
            commands.entity(entity).insert((
                LocalPlayer,
                Predicted,
                CharacterController::default(),
                PredictionHistory::default(),
            ));
        } else {
            tracing::info!("player {entity} (id {}) is remote", player.id.0);
            commands
                .entity(entity)
                .insert(RemotePlayer)
                .remove::<WorldObserver>();
        }
    }
}

/// The command this tick consumed, to the server, numbered. Once per tick,
/// after `SimSet::Input`, so the server sees exactly the jumps the local
/// tick saw.
pub fn send_intent(
    mut commands: Commands,
    mut sequence: ResMut<InputSequence>,
    local: Query<&PlayerCommand, With<LocalPlayer>>,
) {
    for command in &local {
        sequence.0 += 1;
        commands.client_trigger(PlayerIntentEvent {
            intent: command.0,
            seq: sequence.0,
        });
    }
}

/// After the physics stepped: the prediction for this tick's input is
/// recorded and, when the server's position for an input it has acked
/// disagrees with the prediction made for that input, the player and every
/// prediction still on record are shifted by the difference (see the
/// module docs for why a shift and not a set). Predictions older than the
/// ack are dropped.
pub fn reconcile_local_player(
    config: Res<PredictionConfig>,
    sequence: Res<InputSequence>,
    mut stats: ResMut<PredictionStats>,
    mut players: Query<
        (
            &mut WorldPosition,
            &mut PredictionHistory,
            Option<&ServerPosition>,
            Option<&AppliedInput>,
        ),
        With<Predicted>,
    >,
) {
    for (mut position, mut history, server, applied) in &mut players {
        history.0.push_back((sequence.0, position.0));
        while history.0.len() > config.history.max(1) {
            history.0.pop_front();
        }
        let (Some(server), Some(applied)) = (server, applied) else {
            continue;
        };
        let Some(&(_, predicted)) = history.0.iter().find(|(seq, _)| *seq == applied.seq) else {
            continue;
        };
        let error = predicted.distance(server.0);
        while history.0.front().is_some_and(|(seq, _)| *seq < applied.seq) {
            history.0.pop_front();
        }
        if error > config.snap_distance {
            #[allow(clippy::cast_possible_truncation, reason = "a log figure")]
            let error_m = error as f32;
            tracing::debug!(error = error_m, "prediction corrected to the server");
            stats.snaps += 1;
            stats.last_error = error_m;
            let shift = server.0 - predicted;
            position.0 += shift;
            for (_, recorded) in &mut history.0 {
                *recorded += shift;
            }
        }
    }
}

/// The server's clock into the sim's, when it arrives. The local tick
/// advances it between arrivals, so the client's clock runs at the sim
/// rate and is pulled to the server's every replicated tick.
#[allow(
    clippy::float_cmp,
    reason = "the day settings are copied bit for bit; a write only when the bits differ"
)]
pub fn apply_clock(
    carriers: Query<&ReplicatedClock, Changed<ReplicatedClock>>,
    mut clock: ResMut<SimClock>,
    mut day: ResMut<DayLength>,
    mut start: ResMut<DayStart>,
) {
    for carrier in &carriers {
        clock.0 = carrier.time;
        if day.0 != carrier.day_length {
            day.0 = carrier.day_length;
        }
        if start.0 != carrier.day_start {
            start.0 = carrier.day_start;
        }
    }
}

/// The port's [`NetStatus`] from what replicon and the transport know:
/// whether the handshake is done, the round trip the backend measured,
/// how many players this client currently sees (its own included, so a
/// connected client alone reads one) and how many times the prediction
/// was snapped. Written only when something changed, so a HUD reading it
/// by change detection is not woken every frame.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a round trip in milliseconds and a player count are display figures"
)]
pub fn publish_status(
    state: Res<State<ClientState>>,
    backend: Option<Res<ClientStats>>,
    prediction: Res<PredictionStats>,
    players: Query<(), With<Player>>,
    mut status: ResMut<NetStatus>,
) {
    let next = NetStatus {
        connected: *state.get() == ClientState::Connected,
        rtt_ms: backend.map_or(0.0, |b| (b.rtt * 1000.0) as f32),
        players: players.iter().count() as u32,
        corrections: prediction.snaps,
    };
    if *status != next {
        *status = next;
    }
}
