//! The server: owns the simulation, a player per connection, intents in,
//! state out to the clients that can see it.
//!
//! Nothing here moves a player. A client's intent is written into the
//! player's `PlayerIntent` exactly as the local input systems write it in
//! a single-process game, and the sim's `SimSet::Input` consumes it into
//! the command the physics steps (PLAN Phase 4a). The server's only extra
//! jobs are lifetime (spawn on authorization, despawn on disconnect),
//! the clock carrier, and deciding per client which entities it sees
//! ([`crate::interest`]).

use std::collections::VecDeque;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::SystemTime;

use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use bevy::state::state::State;
use bevy_replicon::prelude::{
    AuthorizedClient, ClientId, ConnectedClient, FromClient, Replicated, RepliconChannels,
    SendTargets, ServerState, ToClients,
};
use bevy_replicon::server::visibility::client_visibility::ClientVisibility;
use bevy_replicon::server::visibility::filters_mask::FilterBit;
use bevy_replicon::shared::backend::connected_client::NetworkId;
use bevy_replicon_renet::netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use bevy_replicon_renet::renet::ConnectionConfig;
use bevy_replicon_renet::{RenetChannelsExt, RenetServer};
use strata_net::NetStatus;
use strata_physics::CharacterController;
use strata_sim::{
    DayLength, DayStart, Player, PlayerId, PlayerIntent, SimClock, WorldPosition, WorldSeed,
    WorldSourceRes,
};
use strata_stream::StreamConfig;
use strata_worldgen::find_land_near;

use crate::interest::{ClientInterest, InterestConfig};
use crate::protocol::{AppliedInput, PlayerIntentEvent, ReplicatedClock, Welcome};

/// The protocol id renet's netcode handshake checks. Replicon checks the
/// real protocol (the registration order) after connection; this only
/// keeps a client of another game off the socket.
pub const PROTOCOL_ID: u64 = 0x5343_524f_4c4c_5301;

/// How many clients the netcode transport accepts at once.
pub const MAX_CLIENTS: usize = 64;

/// Height of a new player's feet above the ground at spawn. Under the
/// controller's snap distance on purpose: the first step with ground under
/// it stands the character on it in one tick, instead of a fall of several
/// ticks that the client, still building its own collider for the region,
/// predicts as standing still and then has to correct.
pub const PLAYER_DROP: f64 = 0.3;

/// How close to a spawn slot a player may stand before the slot counts as
/// taken. Two capsules are 0.7 m across; a metre keeps a newcomer off
/// anyone still standing where they spawned.
pub const SPAWN_CLEARANCE: f64 = 1.0;

/// Where new players are put: dry land near this `(x, z)`. Defaults to the
/// world origin, the local game's default too.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct SpawnNear(pub f64, pub f64);

impl Default for SpawnNear {
    fn default() -> Self {
        Self(0.0, 0.0)
    }
}

/// The UDP port the server listens on.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenPort(pub u16);

/// On a player entity: the connected client entity that drives it. The
/// player outlives nothing: when the client entity goes, so does the
/// player.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedBy(pub Entity);

/// Intents a client sent and the server's tick has not consumed yet, on
/// the player. One is consumed per tick so the server runs the same
/// sequence of commands the client predicted with; when the queue backs up
/// past [`Self::MAX_LAG`] (a burst after a stall) the oldest are dropped,
/// with their jumps folded into the one kept, rather than letting the
/// player fall a growing number of ticks behind its client. The trim
/// happens as intents arrive, so a client that floods the channel grows
/// nothing on the server.
#[derive(Component, Debug, Default)]
pub struct InputQueue(pub VecDeque<PlayerIntentEvent>);

impl InputQueue {
    /// How many intents may wait behind the one the next tick takes; two
    /// is a tick of jitter either way.
    pub const MAX_LAG: usize = 2;

    /// Queues `event` unless its sequence is not newer than the last one
    /// queued (or `applied`, when nothing is queued): a replay or a
    /// reordered duplicate is dropped. Trims to the newest
    /// `MAX_LAG + 1`, folding a dropped jump into the next kept intent so a
    /// press is never lost to the trim. Returns whether it was queued.
    pub fn push(&mut self, event: PlayerIntentEvent, applied: u64) -> bool {
        let last = self.0.back().map_or(applied, |e| e.seq);
        if event.seq <= last {
            return false;
        }
        self.0.push_back(event);
        while self.0.len() > Self::MAX_LAG + 1 {
            let Some(dropped) = self.0.pop_front() else {
                break;
            };
            if let Some(next) = self.0.front_mut() {
                next.intent.jump |= dropped.intent.jump;
            }
        }
        true
    }

    /// The intent the next tick applies, if any arrived.
    pub fn pop(&mut self) -> Option<PlayerIntentEvent> {
        self.0.pop_front()
    }
}

/// The visibility bit interest management sets per client and entity,
/// registered with replicon's filter registry when the plugin builds.
#[derive(Resource, Debug, Clone, Copy)]
pub struct InterestBit(pub FilterBit);

/// Binds the socket and inserts renet's server. Runs on entering the world,
/// so no client can connect before there is ground to spawn it on.
pub fn listen(mut commands: Commands, port: Res<ListenPort>, channels: Res<RepliconChannels>) {
    let server = RenetServer::new(ConnectionConfig {
        server_channels_config: channels.server_configs(),
        client_channels_config: channels.client_configs(),
        ..Default::default()
    });
    let current_time = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("the clock is before 1970, not listening: {e}");
            return;
        }
    };
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port.0)) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("could not bind UDP port {}: {e}", port.0);
            return;
        }
    };
    let config = ServerConfig {
        current_time,
        max_clients: MAX_CLIENTS,
        protocol_id: PROTOCOL_ID,
        authentication: ServerAuthentication::Unsecure,
        public_addresses: Vec::new(),
    };
    match NetcodeServerTransport::new(config, socket) {
        Ok(transport) => {
            tracing::info!("listening on UDP port {}", port.0);
            commands.insert_resource(server);
            commands.insert_resource(transport);
        }
        Err(e) => tracing::error!("could not start the netcode transport: {e}"),
    }
}

/// Spawns the clock carrier, one per world.
pub fn spawn_clock(
    mut commands: Commands,
    clock: Res<SimClock>,
    day: Res<DayLength>,
    start: Res<DayStart>,
) {
    commands.spawn((
        strata_net::Replicated,
        ReplicatedClock {
            time: clock.0,
            day_length: day.0,
            day_start: start.0,
        },
    ));
}

/// Keeps the clock carrier at the sim's clock, once per tick.
pub fn publish_clock(
    clock: Res<SimClock>,
    day: Res<DayLength>,
    start: Res<DayStart>,
    mut carriers: Query<&mut ReplicatedClock>,
) {
    for mut carrier in &mut carriers {
        let next = ReplicatedClock {
            time: clock.0,
            day_length: day.0,
            day_start: start.0,
        };
        if *carrier != next {
            *carrier = next;
        }
    }
}

/// Every entity the game marks with the port's `Replicated` is replicated
/// by the adapter's library.
pub fn mark_replicated(add: On<Add, strata_net::Replicated>, mut commands: Commands) {
    commands.entity(add.entity).insert(Replicated);
}

/// Where the `n`th player goes relative to the spawn point: a sunflower
/// spread two metres apart, because two capsules spawned on the same spot
/// are each other's wall and neither can take a step.
#[must_use]
pub fn spawn_offset(n: usize) -> (f64, f64) {
    #[allow(
        clippy::cast_precision_loss,
        reason = "a player count is far below 2^53"
    )]
    let n = n as f64;
    let radius = 2.0 * n.sqrt();
    let angle = n * 2.399_963_229_728_653; // the golden angle
    (radius * angle.cos(), radius * angle.sin())
}

/// The first spawn slot (by [`spawn_offset`] index) further than
/// [`SPAWN_CLEARANCE`] from everyone in `occupied`. Counting the players
/// instead would hand a newcomer the slot of a player who left while
/// another still stands on it, the blocking case the spread exists to
/// avoid; a search over the slots is right whoever came and went. Bounded
/// at four slots per client the transport accepts; past that everyone is
/// standing on every slot and the player count is as good a guess as any.
#[must_use]
pub fn free_spawn_slot(spawn: (f64, f64), occupied: &[(f64, f64)]) -> usize {
    (0..4 * MAX_CLIENTS)
        .find(|&n| {
            let (dx, dz) = spawn_offset(n);
            let (x, z) = (spawn.0 + dx, spawn.1 + dz);
            occupied
                .iter()
                .all(|(ox, oz)| (x - ox).hypot(z - oz) > SPAWN_CLEARANCE)
        })
        .unwrap_or(occupied.len())
}

/// A player for every client that passed the protocol check, its feet
/// [`PLAYER_DROP`] over dry land near [`SpawnNear`] (spread out by
/// [`spawn_offset`], on the first slot nobody stands on), owned by the
/// client. The client's id is the player's id, so the client knows its
/// player the moment it sees it; the [`Welcome`] confirms it and names the
/// seed.
pub fn spawn_player_for_client(
    add: On<Add, AuthorizedClient>,
    mut commands: Commands,
    source: Option<Res<WorldSourceRes>>,
    seed: Res<WorldSeed>,
    spawn: Res<SpawnNear>,
    clients: Query<Option<&NetworkId>, With<ConnectedClient>>,
    players: Query<&WorldPosition, With<OwnedBy>>,
) {
    let client = add.entity;
    let Some(source) = source else {
        tracing::warn!("client {client} authorized before the world exists; no player spawned");
        return;
    };
    let Ok(network_id) = clients.get(client) else {
        return;
    };
    let id = PlayerId(network_id.map_or_else(|| client.to_bits(), NetworkId::get));
    let occupied: Vec<(f64, f64)> = players.iter().map(|p| (p.0.x(), p.0.z())).collect();
    let (dx, dz) = spawn_offset(free_spawn_slot((spawn.0, spawn.1), &occupied));
    let (near_x, near_z) = (spawn.0 + dx, spawn.1 + dz);
    let (x, z) = find_land_near(source.source(), near_x, near_z, 5.0, 250.0, 30_000.0)
        .unwrap_or((near_x, near_z));
    let ground = f64::from(
        source
            .source()
            .sample(x, z)
            .height
            .max(source.source().sea_level()),
    );
    let controller = CharacterController::default();
    let at = WorldPosition::new(
        x,
        ground + PLAYER_DROP + f64::from(controller.half_height()),
        z,
    );
    let player = commands
        .spawn((
            Player { id },
            controller,
            at,
            OwnedBy(client),
            InputQueue::default(),
            AppliedInput::default(),
            strata_net::Replicated,
        ))
        .id();
    commands.entity(client).insert(ClientInterest::default());
    commands.server_trigger_welcome(client, id, seed.0.0);
    tracing::info!(
        "client {client} (id {}) joined as player {player} at ({:.0}, {:.0}, {:.0})",
        id.0,
        at.0.x(),
        at.0.y(),
        at.0.z()
    );
}

/// A small extension so the spawn reads as what it does.
trait WelcomeExt {
    fn server_trigger_welcome(&mut self, client: Entity, player: PlayerId, seed: u64);
}

impl WelcomeExt for Commands<'_, '_> {
    fn server_trigger_welcome(&mut self, client: Entity, player: PlayerId, seed: u64) {
        use bevy_replicon::prelude::ServerTriggerExt;
        self.server_trigger(ToClients {
            targets: SendTargets::Single(ClientId::Client(client)),
            message: Welcome { player, seed },
        });
    }
}

/// The player of a client that left. The despawn replicates to whoever
/// could see it.
pub fn despawn_player_for_client(
    remove: On<Remove, ConnectedClient>,
    mut commands: Commands,
    players: Query<(Entity, &OwnedBy)>,
) {
    for (player, owner) in &players {
        if owner.0 == remove.entity {
            tracing::info!("client {} left; player {player} despawned", remove.entity);
            commands.entity(player).despawn();
        }
    }
}

/// A client's intent joins its own player's queue, and nobody else's: the
/// sender is the connection, not a field in the message. The direction is
/// clamped, a non-finite value is dropped and a sequence that does not
/// advance is ignored ([`InputQueue::push`]), which is all the validation
/// a movement intent needs; the physics decides where it gets.
pub fn apply_client_intent(
    event: On<FromClient<PlayerIntentEvent>>,
    mut players: Query<(&OwnedBy, &mut InputQueue, &AppliedInput)>,
) {
    let Some(client) = event.client_id.entity() else {
        return;
    };
    let mut wanted = event.message;
    if !wanted.intent.move_dir.is_finite() || !wanted.intent.yaw.is_finite() {
        tracing::debug!("client {client} sent a non-finite intent, dropped");
        return;
    }
    wanted.intent.move_dir = wanted.intent.move_dir.clamp_length_max(1.0);
    for (owner, mut queue, applied) in &mut players {
        if owner.0 == client && !queue.push(wanted, applied.seq) {
            tracing::debug!(
                "client {client} sent sequence {} after {}, ignored",
                wanted.seq,
                queue.0.back().map_or(applied.seq, |e| e.seq)
            );
        }
    }
}

/// Once per tick, before the sim consumes intents: the next queued intent
/// becomes the player's `PlayerIntent` and its sequence the
/// [`AppliedInput`]. An empty queue (a late packet) holds the player still
/// and leaves the ack where it was, so the position the server reports for
/// that sequence is the one it had when it applied it, and the client's
/// comparison against its prediction for the same sequence stays exact
/// through the stall. Letting the last intent run on would have moved the
/// player under an ack that did not move, which the client can only read
/// as a wrong prediction. A stall in the air still lets gravity finish the
/// arc; that shows as one small correction, not a cascade.
pub fn dequeue_inputs(mut players: Query<(&mut InputQueue, &mut PlayerIntent, &mut AppliedInput)>) {
    for (mut queue, mut intent, mut applied) in &mut players {
        let Some(next) = queue.pop() else {
            if intent.move_dir != Vec2::ZERO || intent.sprint || intent.jump {
                intent.move_dir = Vec2::ZERO;
                intent.sprint = false;
                intent.jump = false;
            }
            continue;
        };
        let jump = intent.jump || next.intent.jump;
        *intent = next.intent;
        intent.jump = jump;
        applied.seq = next.seq;
    }
}

/// Decides, per authorized client, which replicated entities with a
/// position it sees: its own player always, everything else when it is in
/// the client's region interest. Runs before replication sends so a client
/// that was authorized this frame never receives an entity it should not.
pub fn update_interest(
    stream: Res<StreamConfig>,
    interest: Res<InterestConfig>,
    bit: Res<InterestBit>,
    mut clients: Query<(Entity, &mut ClientInterest, &mut ClientVisibility)>,
    players: Query<(&OwnedBy, &WorldPosition)>,
    entities: Query<(Entity, &WorldPosition, Option<&OwnedBy>), With<Replicated>>,
) {
    for (client, mut regions, mut visibility) in &mut clients {
        let Some(observer) = players
            .iter()
            .find(|(owner, _)| owner.0 == client)
            .map(|(_, pos)| pos.0)
        else {
            // No player yet: nothing with a position is theirs to see.
            for (entity, _, _) in &entities {
                visibility.set(entity, bit.0, false);
            }
            continue;
        };
        regions.refresh(observer, &stream, &interest);
        for (entity, position, owner) in &entities {
            let own = owner.is_some_and(|o| o.0 == client);
            let visible = own || regions.contains(position.0, &interest);
            visibility.set(entity, bit.0, visible);
        }
    }
}

/// The port's [`NetStatus`] on the server: listening or not, and how many
/// clients are connected. The server has no round trip of its own (each
/// client's is on its `ConnectedClientStats`) and corrects no prediction.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a client count is a display figure and far below 2^32"
)]
pub fn publish_status(
    state: Res<State<ServerState>>,
    clients: Query<(), With<ConnectedClient>>,
    mut status: ResMut<NetStatus>,
) {
    let next = NetStatus {
        connected: *state.get() == ServerState::Running,
        rtt_ms: 0.0,
        players: clients.iter().count() as u32,
        corrections: 0,
    };
    if *status != next {
        *status = next;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use bevy::app::App;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::math::Vec2;

    fn intent(seq: u64, x: f32, jump: bool) -> PlayerIntentEvent {
        PlayerIntentEvent {
            intent: PlayerIntent {
                move_dir: Vec2::new(x, 0.0),
                yaw: 0.0,
                jump,
                sprint: false,
            },
            seq,
        }
    }

    /// Two clients with a player each, and the receive observer.
    fn two_players() -> (App, Entity, Entity, Entity, Entity) {
        let mut app = App::new();
        app.add_observer(apply_client_intent);
        let c1 = app.world_mut().spawn_empty().id();
        let c2 = app.world_mut().spawn_empty().id();
        let player = |app: &mut App, client| {
            app.world_mut()
                .spawn((
                    OwnedBy(client),
                    InputQueue::default(),
                    AppliedInput::default(),
                    PlayerIntent::default(),
                ))
                .id()
        };
        let p1 = player(&mut app, c1);
        let p2 = player(&mut app, c2);
        (app, c1, c2, p1, p2)
    }

    fn send(app: &mut App, client: Entity, message: PlayerIntentEvent) {
        app.world_mut().trigger(FromClient {
            client_id: ClientId::Client(client),
            message,
        });
    }

    fn queued(app: &App, player: Entity) -> Vec<u64> {
        app.world()
            .get::<InputQueue>(player)
            .unwrap()
            .0
            .iter()
            .map(|e| e.seq)
            .collect()
    }

    #[test]
    fn an_intent_reaches_only_the_senders_player() {
        let (mut app, c1, _c2, p1, p2) = two_players();
        send(&mut app, c1, intent(1, 1.0, false));
        assert_eq!(queued(&app, p1), vec![1]);
        assert!(queued(&app, p2).is_empty());
        // A message with no client entity behind it (a listen server) goes
        // nowhere.
        app.world_mut().trigger(FromClient {
            client_id: ClientId::Server,
            message: intent(2, 1.0, false),
        });
        assert_eq!(queued(&app, p1), vec![1]);
    }

    #[test]
    fn a_bad_intent_is_dropped_or_clamped() {
        let (mut app, c1, _c2, p1, _p2) = two_players();
        let mut nan = intent(1, 1.0, false);
        nan.intent.yaw = f32::NAN;
        send(&mut app, c1, nan);
        let mut inf = intent(2, 1.0, false);
        inf.intent.move_dir = Vec2::new(f32::INFINITY, 0.0);
        send(&mut app, c1, inf);
        assert!(
            queued(&app, p1).is_empty(),
            "non-finite intents are dropped"
        );
        send(&mut app, c1, intent(3, 40.0, false));
        let queue = app.world().get::<InputQueue>(p1).unwrap();
        let dir = queue.0[0].intent.move_dir;
        assert!((dir.length() - 1.0).abs() < 1e-6, "clamped to unit: {dir}");
    }

    #[test]
    fn a_sequence_that_does_not_advance_is_ignored() {
        let (mut app, c1, _c2, p1, _p2) = two_players();
        app.world_mut().get_mut::<AppliedInput>(p1).unwrap().seq = 5;
        send(&mut app, c1, intent(5, 1.0, false));
        send(&mut app, c1, intent(3, 1.0, false));
        assert!(queued(&app, p1).is_empty(), "not newer than the ack");
        send(&mut app, c1, intent(6, 1.0, false));
        send(&mut app, c1, intent(6, 1.0, false));
        send(&mut app, c1, intent(7, 1.0, false));
        send(&mut app, c1, intent(6, 1.0, false));
        assert_eq!(queued(&app, p1), vec![6, 7]);
    }

    #[test]
    fn a_flood_is_trimmed_on_arrival_with_the_jump_kept() {
        let (mut app, c1, _c2, p1, _p2) = two_players();
        for seq in 1..=10 {
            send(&mut app, c1, intent(seq, 1.0, seq == 2));
        }
        assert_eq!(
            queued(&app, p1),
            vec![8, 9, 10],
            "the newest MAX_LAG + 1 survive"
        );
        let queue = app.world().get::<InputQueue>(p1).unwrap();
        assert!(
            queue.0[0].intent.jump,
            "the jump in a dropped intent folds forward into the next kept"
        );
    }

    #[test]
    fn an_empty_queue_holds_the_player_still_and_the_ack_where_it_was() {
        let (mut app, c1, _c2, p1, _p2) = two_players();
        send(&mut app, c1, intent(1, 1.0, true));
        app.world_mut().run_system_once(dequeue_inputs).unwrap();
        let applied = app.world().get::<PlayerIntent>(p1).unwrap();
        assert_eq!(applied.move_dir, Vec2::X);
        assert!(applied.jump);
        assert_eq!(app.world().get::<AppliedInput>(p1).unwrap().seq, 1);
        // The sim would have consumed it; the queue is empty for the next
        // tick, so the player stops and the ack stands.
        app.world_mut().run_system_once(dequeue_inputs).unwrap();
        let held = app.world().get::<PlayerIntent>(p1).unwrap();
        assert_eq!(held.move_dir, Vec2::ZERO);
        assert!(!held.jump && !held.sprint);
        assert_eq!(app.world().get::<AppliedInput>(p1).unwrap().seq, 1);
        // Then the burst arrives and one per tick is applied again.
        send(&mut app, c1, intent(2, 1.0, false));
        send(&mut app, c1, intent(3, -1.0, false));
        app.world_mut().run_system_once(dequeue_inputs).unwrap();
        assert_eq!(app.world().get::<AppliedInput>(p1).unwrap().seq, 2);
        assert_eq!(queued(&app, p1), vec![3]);
    }

    #[test]
    fn the_first_free_slot_skips_anyone_standing_on_one() {
        let spawn = (100.0, -50.0);
        assert_eq!(free_spawn_slot(spawn, &[]), 0);
        let at = |n: usize| {
            let (dx, dz) = spawn_offset(n);
            (spawn.0 + dx, spawn.1 + dz)
        };
        // Players on slots 0 and 2 (slot 1 left): the newcomer takes 1.
        assert_eq!(free_spawn_slot(spawn, &[at(0), at(2)]), 1);
        // Someone half a metre off slot 1 still blocks it.
        let (x, z) = at(1);
        assert_eq!(free_spawn_slot(spawn, &[at(0), (x + 0.5, z), at(2)]), 3);
        // Slots are further apart than the clearance, so a full row of
        // players does not block the next slot.
        let row: Vec<_> = (0..8).map(at).collect();
        assert_eq!(free_spawn_slot(spawn, &row), 8);
    }
}
