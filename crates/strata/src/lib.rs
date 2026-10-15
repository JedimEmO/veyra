//! The facade: one dependency for a game, two plugin groups.
//!
//! [`StrataPlugins::headless`] is the simulation, the streamer, the
//! physics and the net port with no renderer, what a dedicated server
//! runs. [`StrataPlugins::client`] is that plus terrain, atmosphere and the
//! camera. The split is a launch flag, not a refactor (PLAN principle 3),
//! and `just server-check` keeps `bevy_render` out of the headless graph.
//! The `net-replicon` feature adds the replication adapter for either end
//! of a wire; without it every build is `NetRole::Standalone`.
//!
//! The crates are re-exported under short names so a game can reach any
//! layer, and the [`prelude`] carries what an app touches every day.

pub use strata_core as core;
pub use strata_net as net;
#[cfg(feature = "net-replicon")]
pub use strata_net_replicon as net_replicon;
pub use strata_physics as physics;
pub use strata_registry as registry;
pub use strata_sim as sim;
pub use strata_stream as stream;
pub use strata_weather as weather;
pub use strata_worldgen as worldgen;

#[cfg(feature = "client")]
pub use strata_atmosphere as atmosphere;
#[cfg(feature = "client")]
pub use strata_client as client;
#[cfg(feature = "client")]
pub use strata_foliage as foliage;
#[cfg(feature = "client")]
pub use strata_terrain as terrain;

mod plugins;
pub use plugins::{HeadlessPlugins, StrataPlugins};

#[cfg(feature = "client")]
pub use plugins::ClientPlugins;

/// What an app over strata uses every day.
pub mod prelude {
    pub use crate::{HeadlessPlugins, StrataPlugins};
    pub use strata_core::{GRID_N, REGION_BASE_SIZE, RegionCoord, Rng, Seed, SimTime, WorldPos};
    pub use strata_net::{NetPlugin, NetRole, NetStatus, Replicated, Transport};
    pub use strata_physics::{
        CharacterController, PhysicsPlugin, PhysicsSet, TerrainCollider, TerrainColliderStats,
    };
    pub use strata_registry::{BiomeId, BiomeStore, GameRegistries};
    pub use strata_sim::{
        AppState, DayLength, DayStart, LocalPlayer, Player, PlayerCommand, PlayerId, PlayerIntent,
        PreviousWorldPosition, PrimaryObserver, RenderOrigin, ScriptedIntent, SimClock, SimPlugin,
        SimSet, SimTick, TimeOfDay, WorldLoadPlugin, WorldObserver, WorldPosition, WorldSeed,
        WorldSourceRes,
    };
    pub use strata_stream::{LoadedRegions, StreamConfig, StreamPlugin, StreamSet};
    pub use strata_worldgen::{Sample, WorldSource, find_land_near};

    #[cfg(feature = "net-replicon")]
    pub use strata_net_replicon::{
        ClientInterest, InterestConfig, LocalPlayerId, OwnedBy, PredictionStats, RemotePlayer,
        ReplicationPlugin, ReplicationRole,
    };

    #[cfg(feature = "client")]
    pub use crate::ClientPlugins;
    #[cfg(feature = "client")]
    pub use strata_atmosphere::{AtmospherePlugin, DayCycle, SkySettings};
    #[cfg(feature = "client")]
    pub use strata_client::{ClientPlugin, ClientSet, FlightPath, FlyCam, OrbitCamera};
    #[cfg(feature = "client")]
    pub use strata_terrain::material::dev_asset_root;
    #[cfg(feature = "client")]
    pub use strata_terrain::{
        TerrainConfig, TerrainLook, TerrainMaterialPlugin, TerrainPlugin, TerrainRegion,
        TerrainSet, TerrainStats, WaterConfig, WaterPlugin,
    };
}
