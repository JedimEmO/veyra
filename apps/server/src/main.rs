//! The dedicated server.
//!
//! ```text
//! cargo run --release -p server                       listen on UDP 5000, seed 42
//! cargo run --release -p server -- --port 5001 --seed 7
//! cargo run --release -p server -- --tick-log         a line per sim tick as well
//! cargo run --release -p server -- --frames 600       exit after 600 frames (a smoke run)
//! ```
//!
//! `MinimalPlugins` with a 60 Hz schedule runner, the log plugin, the
//! headless facade (the sim at 20 Hz, the streamer, the physics, the net
//! port) and the replication adapter as the server. No window, no
//! renderer: `just server-check` asserts it of this binary's dependency
//! graph. The world loads through the same `WorldLoadPlugin` the game uses,
//! from the same asset directory, so the ground a client walks on is the
//! ground this process collides it with.
//!
//! Every two seconds a line says who is connected, where their players are
//! and what is loaded around them, which is how a headless run shows the
//! world streaming around every connected player. The line also carries
//! what a simulation tick cost (mean and worst over the window, measured
//! around the `SimTick` schedule: intents, physics, observers; replication
//! sends outside it), which is the number a server budget is written
//! against.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::MinimalPlugins;
use bevy::app::{App, AppExit, PluginGroup, ScheduleRunnerPlugin, Update};
use bevy::diagnostic::FrameCount;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::log::{info, warn};
use bevy::state::condition::in_state;
use bevy::state::state::OnEnter;
use clap::Parser;
use strata::net_replicon::bevy_replicon::prelude::{ConnectedClient, ConnectedClientStats};
use strata::net_replicon::bevy_replicon::shared::backend::connected_client::NetworkId;
use strata::net_replicon::{DEFAULT_PORT, InputQueue};
use strata::prelude::*;

/// The workspace `assets/` directory, for development runs from any
/// working directory.
const WORKSPACE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// The workspace `generated/` directory, where `just content` writes.
const WORKSPACE_GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");

/// Frames between the status lines: two seconds at 60 Hz.
const LOG_EVERY: u32 = 120;

/// Command line.
#[derive(Parser, Resource, Debug, Clone)]
#[command(name = "sci-fi-scrolls-server", version, about)]
struct Cli {
    /// The world seed. Clients must load the same one.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// The UDP port to listen on.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Log a line per simulation tick (tick number, every player's
    /// position and queued intents) on top of the two-second status.
    #[arg(long)]
    tick_log: bool,
    /// Exit after this many frames, for a smoke run. Runs forever without.
    #[arg(long)]
    frames: Option<u32>,
    /// The asset root holding `biomes/` and `terrain/`.
    #[arg(long, default_value = WORKSPACE_ASSETS)]
    assets: PathBuf,
    /// The content step's output, holding the baked tree library the world's
    /// trees and trunk colliders come from. `just content` writes it.
    #[arg(long, default_value = WORKSPACE_GENERATED)]
    generated: PathBuf,
    /// Put new players at a named place of the first settlement: `city`, its
    /// street, or `road`, the far end of its road. Near the origin without.
    #[arg(long, value_parser = ["city", "road"])]
    at: Option<String>,
}

fn main() -> AppExit {
    let cli = Cli::parse();
    // Read once here for the handshake; the world loader reads it again for
    // placement, from the same file.
    let library = match strata_flora::TreeLibrary::read(&cli.generated) {
        Ok(library) => library.version,
        Err(error) => {
            eprintln!("{error}");
            return AppExit::error();
        }
    };
    // The world's version joins the handshake the same way: the loader
    // builds the world from these registries again.
    let world = match strata::sim::world_version(&cli.assets) {
        Ok(version) => version,
        Err(error) => {
            eprintln!("{error}");
            return AppExit::error();
        }
    };
    // The combat tables the same way: installed process-wide, since the
    // sim's numerical core reads them without a world, and in the handshake.
    let combat = match strata::sim::combat_tables::CombatTables::load(cli.assets.join("combat")) {
        Ok(tables) => {
            let version = tables.version();
            strata::sim::combat_tables::install(tables);
            version
        }
        Err(error) => {
            eprintln!("{error}");
            return AppExit::error();
        }
    };
    // What the content step stood on the procedural sites, the same way: a
    // client with other baked groups would collide with other walls.
    let site_groups = match strata::sim::sites::site_groups_version(&cli.assets, &cli.generated) {
        Ok(version) => version,
        Err(error) => {
            eprintln!("{error}");
            return AppExit::error();
        }
    };
    // New players spawn near the origin or at a settlement place: the
    // rivers round both are baked before the server builds its app and its
    // task pools (ADR 0010), and the loader's world shares the bake.
    let prepare = strata::sim::PrepareWater {
        places: vec![(0.0, 0.0)],
        ..Default::default()
    };
    match strata::sim::prepare_water_before_start(&cli.assets, Seed(cli.seed), &prepare) {
        Ok((masses, seconds)) => {
            println!("rivers: {masses} land masses baked in {seconds:.2} s");
        }
        Err(error) => {
            eprintln!("{error}");
            return AppExit::error();
        }
    }
    let mut app = App::new();
    app.insert_resource(WorldSeed(Seed(cli.seed)));
    app.insert_resource(prepare);
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        ))),
        bevy::log::LogPlugin::default(),
        StrataPlugins::headless(),
        WorldLoadPlugin::new(&cli.assets).with_generated(&cli.generated),
        ReplicationPlugin::server(cli.port)
            .with_tree_library(library)
            .with_world_version(world)
            .with_seed(cli.seed)
            .with_combat_tables(combat)
            .with_site_groups(site_groups),
    ))
    .init_resource::<TickCost>()
    .add_systems(
        SimTick,
        (
            start_tick.before(SimSet::Input),
            end_tick.after(SimSet::Observe),
        ),
    )
    .add_systems(
        Update,
        (log_status, exit_when_done).run_if(in_state(AppState::InWorld)),
    )
    .add_systems(OnEnter(AppState::InWorld), spawn_at_place);
    if cli.tick_log {
        app.add_systems(SimTick, log_tick.in_set(SimSet::Observe));
    }
    app.insert_resource(cli);
    app.run()
}

/// Moves where new players spawn to the settlement place `--at` names, once
/// the world has placed its settlements.
fn spawn_at_place(
    cli: Res<Cli>,
    settlements: Option<Res<strata::sim::Settlements>>,
    mut spawn: ResMut<strata::net_replicon::SpawnNear>,
) {
    let Some(name) = cli.at.as_deref() else {
        return;
    };
    if let Some((x, z)) = settlements.and_then(|s| s.place(name)) {
        info!(place = name, x, z, "new players spawn at the settlement");
        *spawn = strata::net_replicon::SpawnNear(x, z);
    } else {
        warn!(
            place = name,
            "no such settlement place; spawning near the origin"
        );
    }
}

/// What the simulation ticks cost since the last status line.
#[derive(Resource, Debug, Default)]
struct TickCost {
    started: Option<Instant>,
    ticks: u32,
    total: Duration,
    worst: Duration,
}

impl TickCost {
    /// Mean and worst tick in milliseconds over the window, then the
    /// window starts again.
    fn take(&mut self) -> (f64, f64) {
        let mean = if self.ticks == 0 {
            0.0
        } else {
            self.total.as_secs_f64() * 1_000.0 / f64::from(self.ticks)
        };
        let worst = self.worst.as_secs_f64() * 1_000.0;
        self.ticks = 0;
        self.total = Duration::ZERO;
        self.worst = Duration::ZERO;
        (mean, worst)
    }
}

/// First in the tick: the clock starts.
fn start_tick(mut cost: ResMut<TickCost>) {
    cost.started = Some(Instant::now());
}

/// Last in the tick: the clock stops and the tick is counted.
fn end_tick(mut cost: ResMut<TickCost>) {
    let Some(started) = cost.started.take() else {
        return;
    };
    let took = started.elapsed();
    cost.ticks += 1;
    cost.total += took;
    cost.worst = cost.worst.max(took);
}

/// Two seconds of the server in one line: the tick and what it costs, the
/// clients with their players' positions and round trips, and what is
/// loaded around them.
#[allow(
    clippy::too_many_arguments,
    reason = "one status line, one set of things to say"
)]
fn log_status(
    frames: Res<FrameCount>,
    clock: Res<SimClock>,
    clients: Query<
        (
            Entity,
            Option<&NetworkId>,
            &ConnectedClientStats,
            Option<&ClientInterest>,
        ),
        With<ConnectedClient>,
    >,
    players: Query<(&OwnedBy, &Player, &WorldPosition, &CharacterController)>,
    loaded: Res<LoadedRegions>,
    colliders: Res<TerrainColliderStats>,
    mut cost: ResMut<TickCost>,
) {
    if frames.0 == 0 || !frames.0.is_multiple_of(LOG_EVERY) {
        return;
    }
    let (mean_ms, worst_ms) = cost.take();
    let mut who = String::new();
    for (client, id, stats, interest) in &clients {
        let player = players.iter().find(|(owner, _, _, _)| owner.0 == client);
        let label = id.map_or_else(|| client.to_string(), |id| id.get().to_string());
        match player {
            Some((_, player, position, controller)) => {
                let _ = write!(
                    who,
                    " [{label}: player {} at ({:.1}, {:.1}, {:.1}) grounded {} rtt {:.0} ms interest {} regions]",
                    player.id.0,
                    position.0.x(),
                    position.0.y(),
                    position.0.z(),
                    controller.grounded,
                    stats.rtt * 1000.0,
                    interest.map_or(0, |i| i.regions().len()),
                );
            }
            None => {
                let _ = write!(who, " [{label}: no player yet]");
            }
        }
    }
    tracing::info!(
        "frame {} tick {} ({mean_ms:.2} ms mean, {worst_ms:.2} ms worst) | clients {}{} | regions {} ({} pending) | colliders {} ({} in flight, {} built)",
        frames.0,
        clock.tick(),
        clients.iter().count(),
        who,
        loaded.len(),
        loaded.pending().count(),
        colliders.loaded,
        colliders.in_flight,
        colliders.built,
    );
}

/// `--tick-log`: every tick, every player.
fn log_tick(
    clock: Res<SimClock>,
    players: Query<(&Player, &WorldPosition, &InputQueue, &PlayerCommand)>,
) {
    for (player, position, queue, command) in &players {
        tracing::info!(
            "tick {} player {} ({:.2}, {:.2}, {:.2}) move {:?} queued {}",
            clock.tick(),
            player.id.0,
            position.0.x(),
            position.0.y(),
            position.0.z(),
            command.0.move_dir,
            queue.0.len(),
        );
    }
}

/// `--frames`: exit once they have run.
fn exit_when_done(cli: Res<Cli>, frames: Res<FrameCount>, mut exit: MessageWriter<AppExit>) {
    if cli.frames.is_some_and(|limit| frames.0 >= limit) {
        tracing::info!("done after {} frames, exiting", frames.0);
        exit.write(AppExit::Success);
    }
}
