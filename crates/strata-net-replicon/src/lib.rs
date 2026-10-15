//! The `bevy_replicon` adapter for the `strata-net` port: a server that
//! owns the simulation and serves it over renet's UDP transport, and a
//! client that renders what it is sent and predicts its own player
//! (PLAN Phase 4b).
//!
//! The crate sits above the physics and below the render crates, so the
//! dedicated server runs it with no renderer and the game runs it with
//! one; `just server-check` holds the server binary's graph to that. The
//! port stays the vocabulary: game code marks a [`strata_net::Replicated`]
//! entity and reads [`strata_net::NetRole`], and this crate is what makes
//! either mean something.
//!
//! # Roles
//!
//! [`ReplicationPlugin`] takes a [`ReplicationRole`]:
//!
//! - `Server { port }` listens once the world exists, spawns a `Player`
//!   with a `CharacterController` for every client that passes the protocol
//!   check, writes the intents that client sends into it, publishes the
//!   sim clock, and per client shows only the entities in its region
//!   interest ([`interest`]).
//! - `Client { server }` connects once the world exists, tags the
//!   replicated player whose id is its own as `LocalPlayer` (so the camera,
//!   the input, the streamer and the render origin follow it as in a single
//!   process) and gives it a controller to predict with; every other player
//!   is a [`client::RemotePlayer`] the server moves.
//!
//! The registration of what crosses the wire is shared ([`protocol`]) and
//! order-sensitive, so both roles run the same function.
//!
//! # Schedules
//!
//! ```text
//! server
//!   PreUpdate    replicon receives; FromClient<PlayerIntentEvent> -> the owner's InputQueue
//!   SimTick      SimSet::Input dequeues one intent per player into PlayerIntent (AppliedInput
//!                takes its sequence), the sim consumes it, SimSet::Step moves players
//!                (strata-physics), SimSet::Observe publishes the clock carrier
//!   Update       publish_status: the port's NetStatus (listening, client count)
//!   PostUpdate   update_interest (before ServerSystems::Send), then replicon sends at the fixed rate
//! client
//!   PreUpdate    replicon receives; adopt_players, apply_clock (after ClientSystems::Receive)
//!   SimTick      SimSet::Input consumes the local intent; send_intent; SimSet::Step predicts;
//!                SimSet::Observe reconciles the prediction with ServerPosition
//!   Update       publish_status: the port's NetStatus (connected, round trip, players seen, snaps)
//!   PostUpdate   replicon sends
//! ```
//!
//! # Frames
//!
//! The server has no window and no camera, but it has a `RenderOrigin`,
//! because the physics runs in `f32` relative to it (ADR 0002): with no
//! `PrimaryObserver` the origin follows the first player and rebases when
//! that player walks two kilometres. A second player far from the first
//! is stepped in `f32` at that distance; the precision holds to a few
//! millimetres at fifty kilometres, and the per-player frame or the `f64`
//! physics build that would remove the limit is the open question in PLAN
//! section 6.

pub mod client;
pub mod handshake;
pub mod interest;
pub mod protocol;
pub mod server;

pub use bevy_replicon;
pub use bevy_replicon_renet;

use std::net::SocketAddr;

use bevy::app::{App, Plugin, PostUpdate, PreUpdate, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::state::app::StatesPlugin;
use bevy::state::condition::in_state;
use bevy::state::state::{OnEnter, OnExit, OnTransition};
use bevy_replicon::prelude::{
    ClientState, ClientSystems, RepliconPlugins, ScopeLifetime, ServerState, ServerSystems,
};
use bevy_replicon::server::visibility::registry::FilterRegistry;
use bevy_replicon::shared::replication::registry::ReplicationRegistry;
use bevy_replicon_renet::RepliconRenetPlugins;
use strata_net::{NetRole, NetStatus};
use strata_sim::{AppState, Authority, Intent, IntentRejected, SimAuthority, SimSet, SimTick};

pub use client::{
    InputSequence, LocalPlayerId, PredictionConfig, PredictionHistory, PredictionStats,
    RemotePlayer, ServerAddress,
};
pub use handshake::{Refused, Versions};
pub use interest::{ClientInterest, InterestConfig, in_interest};
pub use protocol::{
    AppliedInput, PlayerIntentEvent, Predicted, ReplicatedClock, ServerPosition, Welcome,
};
pub use server::{InputQueue, InterestBit, ListenPort, OwnedBy, SpawnNear};

/// Which end of the wire, with what it needs to reach the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplicationRole {
    /// Own the simulation and listen on this UDP port.
    Server {
        /// The port to bind.
        port: u16,
    },
    /// Render a replicated simulation from this server.
    Client {
        /// Where the server listens.
        server: SocketAddr,
    },
}

impl ReplicationRole {
    /// The port's role for this end.
    #[must_use]
    pub fn net_role(self) -> NetRole {
        match self {
            Self::Server { .. } => NetRole::Server,
            Self::Client { .. } => NetRole::Client,
        }
    }
}

/// Present before the plugin is added: no renet transport, no socket. For
/// tests that wire two apps together in one process with replicon's
/// `test_app`, which stands in for the backend.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct NoTransport;

/// The default UDP port.
pub const DEFAULT_PORT: u16 = 5000;

/// Registers replicon, the transport, the protocol and one role's systems.
/// Add after `StrataPlugins::headless()` (or `client()`); it needs the
/// sim's states and sets, and it overrides the port's [`NetRole`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicationPlugin {
    /// Which end this process is.
    pub role: ReplicationRole,
    /// The baked tree library's version, folded into the protocol hash: a
    /// client whose trees are not the server's would collide with trunks it
    /// does not draw, so the handshake refuses it like an older protocol.
    pub tree_library: Option<u64>,
    /// The world's version (`strata_worldgen::world_version`), folded into
    /// the protocol hash after the tree library: a client on other ground
    /// would predict its player over hills the server does not have, so it
    /// is refused, and told so ([`handshake`]).
    pub world: Option<u64>,
    /// The world's seed, folded into the protocol hash after the world's
    /// version: the version is the same for every seed, and a client on
    /// another seed stands on other ground just the same.
    pub seed: Option<u64>,
    /// The combat tables' version (`strata_sim::combat_tables::CombatTables::version`),
    /// folded in after the seed: a client predicting its player on other
    /// numbers would disagree with the server every tick, so it is refused
    /// and told so.
    pub combat_tables: Option<u64>,
    /// The baked site groups' version (`strata_sim::sites::site_groups_version`),
    /// folded in after the combat tables: a client standing other buildings
    /// on the procedural sites, or other bridges at the roads' crossings
    /// (the version hashes the crossings' kit too), would draw and collide
    /// with walls the server does not have, so it is refused and told so.
    pub site_groups: Option<u64>,
}

impl ReplicationPlugin {
    /// A server on `port`.
    #[must_use]
    pub fn server(port: u16) -> Self {
        Self {
            role: ReplicationRole::Server { port },
            tree_library: None,
            world: None,
            seed: None,
            combat_tables: None,
            site_groups: None,
        }
    }

    /// A client of `server`.
    #[must_use]
    pub fn client(server: SocketAddr) -> Self {
        Self {
            role: ReplicationRole::Client { server },
            tree_library: None,
            world: None,
            seed: None,
            combat_tables: None,
            site_groups: None,
        }
    }

    /// Refuse a peer whose tree library is not `version`
    /// (`strata_flora::TreeLibrary::version`).
    #[must_use]
    pub fn with_tree_library(mut self, version: u64) -> Self {
        self.tree_library = Some(version);
        self
    }

    /// Refuse a peer whose world is not `version`
    /// (`strata_worldgen::world_version` of the parameters and registries
    /// the world is built from).
    #[must_use]
    pub fn with_world_version(mut self, version: u64) -> Self {
        self.world = Some(version);
        self
    }

    /// Refuse a peer whose world seed is not `seed`.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Refuse a peer whose combat tables are not `version`
    /// (`CombatTables::version` of the tables it installed).
    #[must_use]
    pub fn with_combat_tables(mut self, version: u64) -> Self {
        self.combat_tables = Some(version);
        self
    }

    /// Refuse a peer whose baked site groups and crossings' kit are not
    /// `version` (`strata_sim::sites::site_groups_version` of its content).
    #[must_use]
    pub fn with_site_groups(mut self, version: u64) -> Self {
        self.site_groups = Some(version);
        self
    }
}

impl Plugin for ReplicationPlugin {
    #[allow(
        clippy::too_many_lines,
        reason = "the one place the protocol and its systems are registered, in order"
    )]
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.insert_resource(self.role.net_role())
            .init_resource::<NetStatus>();
        let transport = !app.world().contains_resource::<NoTransport>();
        app.add_plugins(RepliconPlugins);
        if transport {
            app.add_plugins(RepliconRenetPlugins);
        }
        protocol::register_protocol(app);
        // Hashed after the protocol, so both ends fold it in at the same
        // point; a peer without a library hashes nothing here and is
        // refused by one with one.
        if let Some(version) = self.tree_library {
            app.world_mut()
                .resource_mut::<bevy_replicon::shared::protocol::ProtocolHasher>()
                .add_custom(("tree library", version));
        }
        // The world last of all, appended as every later addition must be:
        // a peer that folds in nothing here is refused by one that does.
        if let Some(version) = self.world {
            app.world_mut()
                .resource_mut::<bevy_replicon::shared::protocol::ProtocolHasher>()
                .add_custom(("world", version));
        }
        if let Some(seed) = self.seed {
            app.world_mut()
                .resource_mut::<bevy_replicon::shared::protocol::ProtocolHasher>()
                .add_custom(("seed", seed));
        }
        if let Some(version) = self.combat_tables {
            app.world_mut()
                .resource_mut::<bevy_replicon::shared::protocol::ProtocolHasher>()
                .add_custom(("combat tables", version));
        }
        if let Some(version) = self.site_groups {
            app.world_mut()
                .resource_mut::<bevy_replicon::shared::protocol::ProtocolHasher>()
                .add_custom(("site groups", version));
        }
        app.insert_resource(Versions {
            tree_library: self.tree_library,
            world: self.world,
            seed: self.seed,
            combat_tables: self.combat_tables,
            site_groups: self.site_groups,
        });

        match self.role {
            ReplicationRole::Server { port } => {
                let bit =
                    app.world_mut()
                        .resource_scope(|world, mut filters: Mut<FilterRegistry>| {
                            world.resource_scope(|world, mut registry: Mut<ReplicationRegistry>| {
                                filters.register_scope::<Entity>(
                                    world,
                                    &mut registry,
                                    ScopeLifetime::WhileVisible,
                                )
                            })
                        });
                app.insert_resource(InterestBit(bit))
                    .insert_resource(ListenPort(port))
                    .init_resource::<SpawnNear>()
                    .init_resource::<InterestConfig>()
                    .add_observer(server::mark_replicated)
                    .add_observer(server::spawn_player_for_client)
                    .add_observer(server::despawn_player_for_client)
                    .add_observer(server::apply_client_intent)
                    .add_observer(handshake::explain_refusal)
                    .add_systems(OnEnter(AppState::InWorld), server::spawn_clock)
                    .add_systems(
                        SimTick,
                        (
                            server::dequeue_inputs
                                .in_set(SimSet::Input)
                                .before(strata_sim::player::consume_player_intents),
                            server::publish_clock.in_set(SimSet::Observe),
                        ),
                    )
                    .add_systems(
                        PostUpdate,
                        server::update_interest
                            .before(ServerSystems::Send)
                            .run_if(in_state(ServerState::Running)),
                    )
                    .add_systems(Update, server::publish_status);
                if transport {
                    app.add_systems(OnEnter(AppState::InWorld), server::listen);
                }
            }
            ReplicationRole::Client { server } => {
                app.insert_resource(ServerAddress(server))
                    .insert_resource(SimAuthority::new(RemoteAuthority))
                    .init_resource::<PredictionConfig>()
                    .init_resource::<PredictionStats>()
                    .init_resource::<InputSequence>()
                    .add_observer(client::on_welcome)
                    .add_observer(handshake::on_server_versions)
                    .add_systems(
                        PreUpdate,
                        (client::adopt_players, client::apply_clock)
                            .chain()
                            .after(ClientSystems::Receive)
                            .run_if(in_state(AppState::InWorld)),
                    )
                    .add_systems(
                        SimTick,
                        (
                            client::send_intent
                                .after(SimSet::Input)
                                .before(SimSet::Step)
                                .run_if(in_state(ClientState::Connected)),
                            client::reconcile_local_player.in_set(SimSet::Observe),
                        ),
                    )
                    .add_systems(Update, client::publish_status)
                    .add_systems(OnEnter(ClientState::Connected), || {
                        tracing::info!("connected");
                    })
                    // Leaving `Connected` is a disconnect; the state starts
                    // at `Disconnected`, so entering it would also log one
                    // before any connection was tried.
                    .add_systems(OnExit(ClientState::Connected), || {
                        tracing::info!("disconnected");
                    })
                    .add_systems(
                        OnTransition {
                            exited: ClientState::Connecting,
                            entered: ClientState::Disconnected,
                        },
                        || {
                            tracing::warn!("the connection attempt failed");
                        },
                    );
                if transport {
                    app.add_systems(OnEnter(AppState::InWorld), client::connect);
                }
            }
        }
    }
}

/// The client's authority: it decides nothing. The port's `Intent`s
/// (place, move) have no networked path in this phase, so they are refused
/// rather than applied locally and silently undone by the next update
/// from the server. Player movement does not go through them at all: it
/// is a `PlayerIntent` the server consumes (see [`client`]).
#[derive(Debug, Default)]
pub struct RemoteAuthority;

impl Authority for RemoteAuthority {
    fn is_authoritative(&self) -> bool {
        false
    }

    fn apply_intent(&mut self, _: Intent) -> Result<(), IntentRejected> {
        Err(IntentRejected::Unavailable)
    }

    fn drain_accepted(&mut self, _: &mut Vec<Intent>) {}
}

#[cfg(test)]
mod smoke {
    use super::*;
    use bevy::state::state::State;

    fn app(role: ReplicationRole) -> App {
        let mut app = App::new();
        app.insert_resource(NoTransport);
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            strata_physics::PhysicsPlugin,
            ReplicationPlugin {
                role,
                tree_library: None,
                world: None,
                seed: None,
                combat_tables: None,
                site_groups: None,
            },
        ));
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

    /// The protocol hash of a server built with `library`.
    fn hash_with(library: Option<u64>) -> bevy_replicon::shared::protocol::ProtocolHash {
        hash_of(library, None)
    }

    /// The protocol hash of a server built with `library` and `world`.
    fn hash_of(
        library: Option<u64>,
        world: Option<u64>,
    ) -> bevy_replicon::shared::protocol::ProtocolHash {
        hash_seeded(library, world, None)
    }

    /// The protocol hash of a server built with `library`, `world` and
    /// `seed`.
    fn hash_seeded(
        library: Option<u64>,
        world: Option<u64>,
        seed: Option<u64>,
    ) -> bevy_replicon::shared::protocol::ProtocolHash {
        let mut app = App::new();
        app.insert_resource(NoTransport);
        let mut plugin = ReplicationPlugin::server(0);
        plugin.tree_library = library;
        plugin.world = world;
        plugin.seed = seed;
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            strata_physics::PhysicsPlugin,
            plugin,
        ));
        app.finish();
        *app.world()
            .resource::<bevy_replicon::shared::protocol::ProtocolHash>()
    }

    #[test]
    fn peers_with_different_tree_libraries_do_not_shake_hands() {
        assert_eq!(hash_with(Some(7)), hash_with(Some(7)));
        assert_ne!(hash_with(Some(7)), hash_with(Some(8)));
        assert_ne!(hash_with(Some(7)), hash_with(None));
    }

    #[test]
    fn peers_on_different_worlds_do_not_shake_hands() {
        assert_eq!(hash_of(Some(7), Some(1)), hash_of(Some(7), Some(1)));
        assert_ne!(hash_of(Some(7), Some(1)), hash_of(Some(7), Some(2)));
        assert_ne!(hash_of(Some(7), Some(1)), hash_of(Some(7), None));
        // The two versions are told apart, not only summed.
        assert_ne!(hash_of(Some(1), Some(2)), hash_of(Some(2), Some(1)));
    }

    #[test]
    fn peers_on_different_seeds_do_not_shake_hands() {
        let seeded = |seed| hash_seeded(Some(7), Some(1), seed);
        assert_eq!(seeded(Some(42)), seeded(Some(42)));
        assert_ne!(seeded(Some(42)), seeded(Some(43)));
        assert_ne!(seeded(Some(42)), seeded(None));
        // The seed is told apart from the versions before it.
        assert_ne!(
            hash_seeded(Some(7), Some(1), Some(2)),
            hash_seeded(Some(7), Some(2), Some(1))
        );
    }

    /// A server and a client in one process with the given versions,
    /// connected through replicon's test backend.
    fn pair(server: Versions, client: Versions) -> (App, App) {
        use bevy_replicon::test_app::ServerTestAppExt;
        let build = |role: ReplicationRole, versions: Versions| {
            let mut app = App::new();
            app.insert_resource(NoTransport);
            app.insert_resource(strata_sim::WorldSeed(strata_core::Seed(7)));
            app.add_plugins((
                bevy::app::TaskPoolPlugin::default(),
                bevy::time::TimePlugin,
                strata_sim::SimPlugin,
                strata_stream::StreamPlugin,
                strata_physics::PhysicsPlugin,
                ReplicationPlugin {
                    role,
                    tree_library: versions.tree_library,
                    world: versions.world,
                    seed: versions.seed,
                    combat_tables: versions.combat_tables,
                    site_groups: versions.site_groups,
                },
            ));
            app.finish();
            app.cleanup();
            app.update();
            app
        };
        let mut server = build(ReplicationRole::Server { port: 0 }, server);
        let mut client = build(
            ReplicationRole::Client {
                server: "127.0.0.1:5000".parse().expect("an address"),
            },
            client,
        );
        server.connect_client(&mut client);
        for _ in 0..3 {
            server.update();
            server.exchange_with_client(&mut client);
            client.update();
        }
        (server, client)
    }

    #[test]
    fn a_client_on_another_world_is_told_so() {
        let ours = Versions {
            tree_library: Some(7),
            world: Some(1),
            seed: Some(42),
            combat_tables: Some(3),
            site_groups: Some(4),
        };
        let (_, client) = pair(
            ours,
            Versions {
                world: Some(2),
                ..ours
            },
        );
        let refused = client
            .world()
            .get_resource::<Refused>()
            .expect("the refused client heard why");
        assert!(refused.world_differs() && !refused.trees_differ());
        assert!(
            refused
                .message()
                .contains("this server's world is another version than yours")
        );
        // The same versions on both ends: no refusal.
        let (_, client) = pair(ours, ours);
        assert!(client.world().get_resource::<Refused>().is_none());
    }

    #[test]
    fn a_client_on_other_trees_or_another_seed_is_told_so() {
        let ours = Versions {
            tree_library: Some(7),
            world: Some(1),
            seed: Some(42),
            combat_tables: Some(3),
            site_groups: Some(4),
        };
        let refusal = |client: Versions| {
            let (_, client) = pair(ours, client);
            *client
                .world()
                .get_resource::<Refused>()
                .expect("the refused client heard why")
        };
        let trees = refusal(Versions {
            tree_library: Some(8),
            ..ours
        });
        assert!(trees.trees_differ() && !trees.world_differs() && !trees.seed_differs());
        assert!(trees.message().contains("trees are another version"));
        let seed = refusal(Versions {
            seed: Some(43),
            ..ours
        });
        assert!(seed.seed_differs() && !seed.world_differs() && !seed.trees_differ());
        assert!(
            seed.message().contains("another seed"),
            "{}",
            seed.message()
        );
        // Other buildings on the sites: refused like the rest, and told.
        let sites = refusal(Versions {
            site_groups: Some(5),
            ..ours
        });
        assert!(sites.site_groups_differ() && !sites.world_differs() && !sites.trees_differ());
        assert!(
            sites
                .message()
                .contains("site buildings are another version")
        );
    }

    #[test]
    fn a_server_builds_and_carries_the_clock() {
        let mut app = app(ReplicationRole::Server { port: 0 });
        assert_eq!(*app.world().resource::<NetRole>(), NetRole::Server);
        assert!(app.world().resource::<SimAuthority>().is_authoritative());
        app.update();
        let mut carriers = app.world_mut().query::<&ReplicatedClock>();
        let carrier = carriers.single(app.world()).expect("one clock carrier");
        assert_eq!(
            carrier.time,
            app.world().resource::<strata_sim::SimClock>().0
        );
    }

    #[test]
    fn a_client_builds_with_no_authority() {
        let app = app(ReplicationRole::Client {
            server: "127.0.0.1:5000".parse().expect("an address"),
        });
        assert_eq!(*app.world().resource::<NetRole>(), NetRole::Client);
        assert!(!app.world().resource::<SimAuthority>().is_authoritative());
    }
}
