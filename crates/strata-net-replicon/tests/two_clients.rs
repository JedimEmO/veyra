//! A server and two clients in one process, wired through replicon's test
//! backend instead of a socket: each client gets a player, sees the other,
//! moves its own, and loses sight of the other when it is far away.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test distances are small"
)]

use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use bevy::state::state::State;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::Remote;
use bevy_replicon::test_app::{ServerTestAppExt, TestClientEntity};
use strata_app::{
    AppPlugin, AppState, LocalPlayer, Player, PlayerId, PlayerIntent, SimClock, WorldPosition,
    WorldSeed, WorldSourceRes,
};
use strata_core::{Seed, WorldPos};
use strata_net::{NetRole, NetStatus};
use strata_net_replicon::{
    LocalPlayerId, NoTransport, OwnedBy, RemotePlayer, ReplicationPlugin, ReplicationRole,
};
use strata_physics::PhysicsPlugin;
use strata_registry::BiomeId;
use strata_stream::{StreamConfig, StreamPlugin};
use strata_worldgen::{Sample, WorldSource};

/// A flat world three metres above the sea.
struct Flat;

impl WorldSource for Flat {
    fn sample(&self, _x: f64, _z: f64) -> Sample {
        Sample {
            height: 3.0,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: strata_worldgen::Landform::default(),
            water: None,
        }
    }

    fn sea_level(&self) -> f32 {
        0.0
    }
}

/// A short horizon keeps the per-client selection small; interest reaches
/// about 1.3 km with the default LOD limit, well inside it.
fn stream_config() -> StreamConfig {
    StreamConfig {
        max_lod: 6,
        view_distance: 8_000.0,
        max_loads_per_frame: 64,
        max_unloads_per_frame: 64,
        ..StreamConfig::default()
    }
}

/// An app in the world over the flat ground, one sim tick per update, no
/// socket.
fn app(role: ReplicationRole) -> App {
    let mut app = App::new();
    app.insert_resource(NoTransport);
    app.insert_resource(stream_config());
    app.insert_resource(WorldSeed(Seed(7)));
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        AppPlugin,
        StreamPlugin,
        PhysicsPlugin,
        ReplicationPlugin {
            role,
            tree_library: None,
            world: None,
            seed: None,
        },
    ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(WorldSourceRes(Arc::new(Flat)));
    app.finish();
    app.cleanup();
    app.update();
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::InWorld
    );
    app
}

fn client() -> App {
    app(ReplicationRole::Client {
        server: "127.0.0.1:5000".parse().unwrap(),
    })
}

/// One round: the server ticks, messages cross both ways, the clients tick.
fn round(server: &mut App, clients: &mut [&mut App]) {
    server.update();
    for client in clients.iter_mut() {
        server.exchange_with_client(client);
    }
    for client in clients.iter_mut() {
        client.update();
    }
}

fn rounds(n: usize, server: &mut App, clients: &mut [&mut App]) {
    for _ in 0..n {
        round(server, clients);
    }
}

/// The player the test backend's client entity owns on the server.
fn server_player(server: &mut App, client: &App) -> Entity {
    let client_entity = **client.world().resource::<TestClientEntity>();
    let mut players = server.world_mut().query::<(Entity, &OwnedBy)>();
    players
        .iter(server.world())
        .find(|(_, owner)| owner.0 == client_entity)
        .map(|(e, _)| e)
        .expect("a player per client")
}

fn world_pos(app: &mut App, entity: Entity) -> WorldPos {
    app.world().get::<WorldPosition>(entity).unwrap().0
}

fn local_player(app: &mut App) -> Entity {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<LocalPlayer>>();
    q.single(app.world()).expect("one local player")
}

fn remote_players(app: &mut App) -> Vec<Entity> {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, (With<RemotePlayer>, With<Remote>)>();
    q.iter(app.world()).collect()
}

/// Connects a client and gives it the id the server will derive for it:
/// the test backend hands out no network id, so the server falls back to
/// the client entity's bits, which the client learns here as it would
/// from its own connection.
fn connect(server: &mut App, client: &mut App) {
    server.connect_client(client);
    let client_entity = **client.world().resource::<TestClientEntity>();
    client.insert_resource(LocalPlayerId(PlayerId(client_entity.to_bits())));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one session from connect to disconnect, in the order it happens"
)]
fn two_clients_see_each_other_and_move() {
    let mut server = app(ReplicationRole::Server { port: 0 });
    let mut client1 = client();
    let mut client2 = client();
    assert_eq!(*server.world().resource::<NetRole>(), NetRole::Server);
    assert_eq!(*client1.world().resource::<NetRole>(), NetRole::Client);

    connect(&mut server, &mut client1);
    connect(&mut server, &mut client2);
    rounds(5, &mut server, &mut [&mut client1, &mut client2]);

    // Each client has its own player and the other's.
    let p1 = server_player(&mut server, &client1);
    let p2 = server_player(&mut server, &client2);
    assert_ne!(p1, p2);
    for (client, name) in [(&mut client1, "client 1"), (&mut client2, "client 2")] {
        let mut players = client.world_mut().query::<&Player>();
        assert_eq!(
            players.iter(client.world()).count(),
            2,
            "{name}: two players"
        );
        let local = local_player(client);
        assert_eq!(
            client.world().get::<Player>(local).unwrap().id,
            client.world().resource::<LocalPlayerId>().0,
            "{name}: the local player is its own"
        );
        assert_eq!(remote_players(client).len(), 1, "{name}: one remote player");
        assert!(
            client
                .world()
                .get::<strata_physics::CharacterController>(local)
                .is_some(),
            "{name}: the local player is predicted with a controller"
        );
        let remote = remote_players(client)[0];
        assert!(
            client
                .world()
                .get::<strata_physics::CharacterController>(remote)
                .is_none(),
            "{name}: the remote player is not stepped"
        );
        assert!(
            client
                .world()
                .get::<strata_app::WorldObserver>(remote)
                .is_none(),
            "{name}: the remote player is not an observer"
        );
    }

    // The port's status says what the HUD will: two players in sight on
    // each client, two clients on the server, no correction yet.
    for (client, name) in [(&client1, "client 1"), (&client2, "client 2")] {
        let status = *client.world().resource::<NetStatus>();
        assert!(status.connected, "{name}: connected");
        assert_eq!(status.players, 2, "{name}: two players in sight");
        assert_eq!(status.corrections, 0, "{name}: no snap yet");
    }
    let server_status = *server.world().resource::<NetStatus>();
    assert!(server_status.connected, "the server is listening");
    assert_eq!(server_status.players, 2, "two clients connected");

    // The clock arrives.
    let server_tick = server.world().resource::<SimClock>().tick();
    let client_tick = client1.world().resource::<SimClock>().tick();
    assert!(
        server_tick.abs_diff(client_tick) <= 2,
        "client clock {client_tick} follows server clock {server_tick}"
    );

    // Let both land (the ground streams in around them on every app).
    rounds(60, &mut server, &mut [&mut client1, &mut client2]);
    let start = world_pos(&mut server, p1);
    assert!(
        (start.y() - 3.0 - 0.85).abs() < 0.3,
        "player 1 stands on the flat ground: {start:?}"
    );

    // Client 1 walks north for two seconds; the server moves its player
    // and client 2 sees the remote player move.
    let local1 = local_player(&mut client1);
    let remote_on_2 = remote_players(&mut client2)[0];
    let seen_before = world_pos(&mut client2, remote_on_2);
    client1
        .world_mut()
        .get_mut::<PlayerIntent>(local1)
        .unwrap()
        .move_dir = Vec2::Y;
    for _ in 0..40 {
        client1
            .world_mut()
            .get_mut::<PlayerIntent>(local1)
            .unwrap()
            .move_dir = Vec2::Y;
        round(&mut server, &mut [&mut client1, &mut client2]);
    }
    let end = world_pos(&mut server, p1);
    let walked = start.z() - end.z();
    assert!(
        (5.0..12.0).contains(&walked),
        "the server walked player 1 north {walked} m in two seconds"
    );
    let predicted = world_pos(&mut client1, local1);
    assert!(
        predicted.distance(end) < 1.0,
        "client 1's prediction {predicted:?} tracks the server {end:?}"
    );
    let seen_after = world_pos(&mut client2, remote_on_2);
    assert!(
        seen_before.z() - seen_after.z() > 4.0,
        "client 2 saw player 1 walk: {seen_before:?} -> {seen_after:?}"
    );
    // Player 2 is teleported five kilometres away: out of client 1's
    // interest, so client 1 loses the entity; back, and it returns.
    let far = WorldPos::new(5_000.0, 3.85, 0.0);
    server.world_mut().get_mut::<WorldPosition>(p2).unwrap().0 = far;
    rounds(10, &mut server, &mut [&mut client1, &mut client2]);
    assert!(
        remote_players(&mut client1).is_empty(),
        "client 1 no longer sees player 2 five kilometres away"
    );
    assert_eq!(
        remote_players(&mut client2).len(),
        0,
        "client 2 no longer sees player 1 either"
    );
    assert!(
        local_player(&mut client2) != Entity::PLACEHOLDER,
        "client 2 still has its own player"
    );
    let near = WorldPos::new(20.0, 3.85, 0.0);
    server.world_mut().get_mut::<WorldPosition>(p2).unwrap().0 = near;
    rounds(10, &mut server, &mut [&mut client1, &mut client2]);
    assert_eq!(
        remote_players(&mut client1).len(),
        1,
        "player 2 is back in sight"
    );
    assert_eq!(remote_players(&mut client2).len(), 1);

    // A client leaves; its player goes with it, everywhere.
    server.disconnect_client(&mut client2);
    rounds(5, &mut server, &mut [&mut client1]);
    assert!(
        remote_players(&mut client1).is_empty(),
        "player 2 despawned"
    );
    let mut owned = server.world_mut().query::<&OwnedBy>();
    assert_eq!(owned.iter(server.world()).count(), 1);
    assert_eq!(
        client1.world().resource::<NetStatus>().players,
        1,
        "client 1 sees only itself now"
    );
    assert_eq!(server.world().resource::<NetStatus>().players, 1);
}

#[test]
fn a_wrong_prediction_snaps_to_the_server() {
    let mut server = app(ReplicationRole::Server { port: 0 });
    let mut client1 = client();
    connect(&mut server, &mut client1);
    rounds(70, &mut server, &mut [&mut client1]);
    let p1 = server_player(&mut server, &client1);
    let local1 = local_player(&mut client1);

    // The client wanders off on its own: no intent reached the server, so
    // the server's player stands still and the prediction is wrong by
    // metres. It snaps back.
    client1
        .world_mut()
        .get_mut::<WorldPosition>(local1)
        .unwrap()
        .0 += bevy::math::DVec3::X * 30.0;
    rounds(6, &mut server, &mut [&mut client1]);
    let server_pos = world_pos(&mut server, p1);
    let client_pos = world_pos(&mut client1, local1);
    assert!(
        client_pos.distance(server_pos) < 0.5,
        "snapped: client {client_pos:?} vs server {server_pos:?}"
    );
    assert!(
        client1
            .world()
            .resource::<strata_net_replicon::PredictionStats>()
            .snaps
            >= 1
    );
}

/// Sets the local player's intent for the next tick: sprinting north, so a
/// tick of movement is 0.45 m and any slip of a tick shows over the half
/// metre correction distance.
fn sprint_north(client: &mut App, player: Entity) {
    let mut intent = client.world_mut().get_mut::<PlayerIntent>(player).unwrap();
    intent.move_dir = Vec2::Y;
    intent.sprint = true;
}

/// Rounds in which the client keeps sprinting.
fn sprint_rounds(n: usize, server: &mut App, client: &mut App, player: Entity) {
    for _ in 0..n {
        sprint_north(client, player);
        round(server, &mut [client]);
    }
}

/// Rounds with no exchange: the intents pile up on the client and nothing
/// reaches it either, as when a packet is late on the wire.
fn stalled_rounds(n: usize, server: &mut App, client: &mut App, player: Entity) {
    for _ in 0..n {
        sprint_north(client, player);
        server.update();
        client.update();
    }
}

fn snaps(client: &App) -> u32 {
    client
        .world()
        .resource::<strata_net_replicon::PredictionStats>()
        .snaps
}

/// A connected client, landed, with its player on both ends.
fn connected_and_landed() -> (App, App, Entity, Entity) {
    let mut server = app(ReplicationRole::Server { port: 0 });
    let mut client1 = client();
    connect(&mut server, &mut client1);
    rounds(70, &mut server, &mut [&mut client1]);
    let p1 = server_player(&mut server, &client1);
    let local1 = local_player(&mut client1);
    (server, client1, p1, local1)
}

#[test]
fn a_late_packet_does_not_snap_and_a_long_stall_corrects_once() {
    let (mut server, mut client1, p1, local1) = connected_and_landed();
    let before = snaps(&client1);

    // Two ticks with nothing arriving, mid-sprint: the server holds the
    // player and its ack, the burst that follows fits the queue, and the
    // prediction for every sequence matches what the server did with it.
    sprint_rounds(20, &mut server, &mut client1, local1);
    stalled_rounds(2, &mut server, &mut client1, local1);
    sprint_rounds(40, &mut server, &mut client1, local1);
    assert_eq!(
        snaps(&client1),
        before,
        "a stall of two ticks is absorbed without a correction"
    );
    // The server now runs the queue's two ticks behind the client on top
    // of the two in flight; four ticks of sprint is under two metres, and
    // that lag is the queue's, not a divergence.
    let server_pos = world_pos(&mut server, p1);
    let client_pos = world_pos(&mut client1, local1);
    assert!(
        client_pos.distance(server_pos) < 2.5,
        "the prediction {client_pos:?} tracks the server {server_pos:?}"
    );

    // Six ticks of stall: more intents than the queue keeps, so the server
    // drops the oldest and the client's path really diverged by their
    // motion. That is one correction, after which the two agree again.
    stalled_rounds(6, &mut server, &mut client1, local1);
    sprint_rounds(40, &mut server, &mut client1, local1);
    assert_eq!(
        snaps(&client1),
        before + 1,
        "dropped intents cost exactly one correction"
    );
    let server_pos = world_pos(&mut server, p1);
    let client_pos = world_pos(&mut client1, local1);
    assert!(
        client_pos.distance(server_pos) < 2.5,
        "the prediction {client_pos:?} tracks the server {server_pos:?} after the correction"
    );
}

#[test]
fn a_correction_while_moving_does_not_cascade() {
    let (mut server, mut client1, p1, local1) = connected_and_landed();
    sprint_rounds(20, &mut server, &mut client1, local1);
    let before = snaps(&client1);

    // A real divergence while sprinting: the client is put thirty metres
    // off and keeps going. The correction must shift it back and leave the
    // unacked ticks in place; setting it to the server's older position
    // would make every later ack disagree by those ticks, forever.
    client1
        .world_mut()
        .get_mut::<WorldPosition>(local1)
        .unwrap()
        .0 += bevy::math::DVec3::X * 30.0;
    sprint_rounds(60, &mut server, &mut client1, local1);
    assert_eq!(
        snaps(&client1),
        before + 1,
        "one divergence, one correction, then exact again"
    );
    let server_pos = world_pos(&mut server, p1);
    let client_pos = world_pos(&mut client1, local1);
    assert!(
        client_pos.distance(server_pos) < 1.5,
        "the prediction {client_pos:?} tracks the server {server_pos:?}"
    );
    assert!(
        (client_pos.x() - server_pos.x()).abs() < 0.1,
        "the thirty metres were taken back: {client_pos:?} vs {server_pos:?}"
    );
}
