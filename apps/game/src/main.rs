//! The slice binary.
//!
//! ```text
//! cargo run -p game                               a window, walk around
//! cargo run -p game -- --flycam                    a window, fly around
//! cargo run -p game -- --spawn 50000,50000         start far from the world origin
//! cargo run -p game -- --frames 60 --screenshot shot.png
//! cargo run -p game -- --auto-walk --frames 900 --screenshot walk-{frame}.png --shot-frames 200,500,900
//! cargo run -p game -- --flight --flight-heading -2.2 --screenshot path-{frame}.png --shot-frames 300,1800,3600
//! cargo run -p game -- --headless --frames 600    the simulation alone, a player walking, exit 0
//! cargo run -p game -- --headless --script-progression --fresh   the fight, a lesson, a swap, saved to scratch/household.ron
//! cargo run -p game -- --open-sheet --frames 200 --screenshot sheet.png   the character sheet over the world (--open-bench the bench, --open-loadout the sheet's Loadout tab)
//! cargo run -p game -- --open-stores --frames 240 --screenshot stores.png   the stores over the household's store (--open-inventory the pack alone)
//! cargo run -p game -- --open-fitting --frames 240 --screenshot fitting.png   the fitting screen on the weapon in hand (--fitted <item> for a fitted one)
//! cargo run -p game -- --open-crafting --frames 360 --screenshot craft.png   the crafting tab on the cell core (--crafting-state empty|pick)
//! cargo run -p game -- --open-crafting --crafting-state hand --fresh --household scratch/crafting/shot.ron --frames 360 --screenshot hand.png
//!                                                 the bench hand (or `done`, or --craft-first <schematic>): these spend stock, so they name a household of their own
//! cargo run -p game -- --stats                    start with the debug statistics (F3) shown
//! cargo run -p game -- --connect 127.0.0.1:5000   a client of `apps/server`; the server owns the player
//! ```
//!
//! `--headless` is `MinimalPlugins` plus `StrataPlugins::headless()`, the
//! same stack a dedicated server runs, so the split in ADR 0001 is
//! exercised from Phase 0; it spawns a player with a scripted intent (walk
//! north) at the spawn point so the streamer selects, loads and unloads
//! regions around a moving observer the way it does for a connected player,
//! the physics builds colliders under it and steps it, and `--frames N` is
//! then a server benchmark. The client is `DefaultPlugins` plus
//! `StrataPlugins::client()`, a player driven by the keyboard and an orbit
//! camera behind it; the player is the world's observer, so the terrain
//! streams around it and the render origin follows it. `--flycam` (and
//! `--flight`, which is a scripted flycam) puts the free camera back as the
//! observer with no player.
//!
//! Both modes load the world through `WorldLoadPlugin`; the world exists
//! before the player does, so the player is spawned on entering `InWorld`
//! two metres over a point the world says is dry.

mod camps;
mod combat;
mod crossings;
mod deposits;
mod factions;
mod grounding;
mod household;
#[cfg_attr(
    not(target_arch = "wasm32"),
    allow(
        dead_code,
        reason = "the web entry calls it; a native run only tests it"
    )
)]
mod query;
mod ride;
mod roadside;
mod route;
mod settlement;
mod shutdown;
mod sites;
mod standing;
mod survey_script;
#[cfg(target_arch = "wasm32")]
mod web;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ashlar_bevy::prelude::AshlarPlugin;
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::io::AssetSourceBuilder;
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
#[cfg(not(target_arch = "wasm32"))]
use bevy::render::settings::WgpuFeatures;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use strata::atmosphere::{
    fog_pass::GroundFog,
    look::{AaOverride, LookFile},
    study::{StudySettings, location},
};

use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::{MonitorSelection, PresentMode, WindowMode, WindowResolution};
use clap::Parser;
use strata::client::flight::{MotionHold, motion_held};
use strata::client::stats::BenchRecorder;
use strata::foliage::FoliageConfig;
use strata::prelude::*;
use strata::registry::files::Files;
use strata::registry::progression::SkillGraph;
use strata::sim::Wind;
use strata::sim::combat_tables::CombatTables;

/// The workspace `assets/` directory, for development runs from any
/// working directory; on the web the page's own `assets/` URL.
#[cfg(not(target_arch = "wasm32"))]
const WORKSPACE_ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
#[cfg(target_arch = "wasm32")]
const WORKSPACE_ASSETS: &str = "assets";

/// The workspace `generated/` directory, where `just content` writes; on the
/// web the page's own `generated/` URL.
#[cfg(not(target_arch = "wasm32"))]
const WORKSPACE_GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");
#[cfg(target_arch = "wasm32")]
const WORKSPACE_GENERATED: &str = "generated";

/// Where the household is saved when `--household` does not say; on the web
/// the `localStorage` key.
#[cfg(not(target_arch = "wasm32"))]
const WORKSPACE_HOUSEHOLD: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../scratch/household.ron");
#[cfg(target_arch = "wasm32")]
const WORKSPACE_HOUSEHOLD: &str = "veyra/household.ron";

/// Command line. Absent flags mean "a window, forever".
#[derive(Parser, Resource, Debug, Clone)]
#[command(name = "sci-fi-scrolls", version, about)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a switch on the command line, which is what a bool is"
)]
struct Cli {
    /// Explicitly select combat (already the default for local character play).
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk"])]
    combat: bool,
    /// Use the original controller without the local combat encounter.
    #[arg(long, conflicts_with_all = ["combat", "combat_script", "combat_trace"])]
    no_combat: bool,
    /// Reproduce the combat timing capture with scripted inputs and a 60 Hz frame clock.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk"])]
    combat_script: bool,
    /// Record the local combatant's fixed-tick state to CSV for feel analysis.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk"])]
    combat_trace: Option<PathBuf>,
    /// The combat script, then the household's path by script: the
    /// rusher is taken with the blade, the player walks to the blade
    /// trainer, learns One hand 1 if the fight earned it, walks to the
    /// terminal, the recruits join and the pistol marksman comes out.
    /// Implies `--combat-script`; without `--frames` the run exits when
    /// the path is walked.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat"])]
    script_progression: bool,
    /// The scripted path with a stop at the workbench after the lesson:
    /// the carbine is drawn and fired, the store's patrol magazine fitted,
    /// the weapon reloaded and fired again, and the magazine taken off;
    /// then a cell core is made from the first lots, the patrol magazine
    /// made from it and fitted, and a second core made from the same lots,
    /// which is recorded as a blueprint; a batch of three is started off
    /// it at the fabricator and waited for at the path's end, all by the
    /// requests a screen would send. Implies `--script-progression`.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat"])]
    script_crafting: bool,
    /// With `--script-crafting`: write what the `Combatant` read at the
    /// workbench (the carbine's row bare, fitted and restored, and the
    /// rounds either side of the fit and the removal) and what was made
    /// there (each finished thing's lines, and the made magazine's rounds
    /// as the bench showed them and as the `Combatant` read them) to this
    /// file, as RON.
    /// With `--script-ride`: write what the ride measured to this file.
    #[arg(long, value_name = "PATH")]
    script_report: Option<PathBuf>,
    /// The household owns a courier bike (ticket 0009, step 2), which V
    /// calls. It changes the household it is run on, so it is refused on
    /// the default `--household`.
    #[arg(long)]
    bike: bool,
    /// With `--bike`, or a household that has one: start with the stabled
    /// bike worn to this condition (0 to 1000), for a still. It is refused
    /// on the default `--household`.
    #[arg(long, value_name = "N")]
    bike_condition: Option<u16>,
    /// Start with this many credits in the purse, for a still. It changes
    /// the household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "CREDITS")]
    purse: Option<u32>,
    /// With a bike: start with three lots and a part in the saddle bags,
    /// copied from the store, for a still. It is refused on the default
    /// `--household`.
    #[arg(long)]
    saddle_stock: bool,
    /// Plant a field harvester where the run starts (`--at deposit`), on the
    /// richest deposit there, for the member who is out and ignoring their
    /// count and the least reading, for a still. It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long)]
    harvester: bool,
    /// Advance every planted field harvester by this many played hours at
    /// startup, for a still. It changes the household it is run on, so it
    /// is refused on the default `--household`.
    #[arg(long, value_name = "HOURS")]
    harvest_hours: Option<f64>,
    /// Open the nearest planted field harvester's screen on frame 120, as
    /// its F does.
    #[arg(long)]
    open_harvester: bool,
    /// Start mounted on the bike, without the call.
    #[arg(long, requires = "bike")]
    mounted: bool,
    /// Headless or windowed: start at the road's far end (`--at road`, the
    /// default here), call the bike, ride the road in for forty seconds and
    /// write what was measured to the log and to `--script-report`.
    #[arg(long, requires = "bike", conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "script_progression", "script_crafting", "combat_script"])]
    script_ride: bool,
    /// Headless or windowed (ticket 0009, step 3): start on dry ground a few
    /// hundred metres from the nearest deposit of this resource class
    /// (`conductor` when none is named, or a full id), walk a straight
    /// heading pinging with the survey tool every hundred metres until a
    /// ping meets something, walk to the strongest point of each ping
    /// until it is under the player's feet, and sample fifteen units. What
    /// it did goes to the log and to `--script-report`.
    #[arg(long, value_name = "CLASS", num_args = 0..=1, default_missing_value = "conductor", conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "script_progression", "script_crafting", "combat_script", "script_ride", "at"])]
    script_survey: Option<String>,
    /// Start with a bought example of this module (an item id,
    /// `veyra:module.patrol_extended_feed`) fitted to the weapon in hand
    /// it is made for, taken from the household's store as a workbench
    /// would take it; repeatable. For a still of a fitted weapon: it
    /// changes the household it is run on.
    #[arg(long, value_name = "ITEM")]
    fitted: Vec<String>,
    /// Start with store grade weapons of these items (comma separated, an
    /// item id each, `-` leaves a slot as it is) in weapon slot one and
    /// slot two of whoever starts out, for a still of a blade in hand. The
    /// member out must be certified on each, or the flag says why not and
    /// changes nothing. It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "ITEM", value_delimiter = ',')]
    in_hand: Vec<String>,
    /// Start with whoever starts out having learned these boxes (comma
    /// separated ids), past their prerequisites and without spending
    /// anything, so a still can show a weapon nobody starts certified on
    /// (`blade.breaking.1` for the greatblade). It changes the household
    /// it is run on, so it is refused on the default `--household`.
    #[arg(long, value_name = "BOX", value_delimiter = ',')]
    learn: Vec<String>,
    /// Start with whoever starts out knowing these abilities (comma
    /// separated ids of `assets/combat/abilities.ron`) and having them
    /// slotted on the bar, where the sim puts a newly known one, for the
    /// stills of the bar and a headless run that presses one
    /// (`precision_shot`). It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "ABILITY", value_delimiter = ',')]
    grant: Vec<String>,
    /// Press a place of the bar at a frame, `place@frame` (comma
    /// separated): 0 to 2 are the weapon in hand's places and 3 and 4 the
    /// utility's, written where the keys write them. It changes the
    /// household it is run on (what it spends), so it is refused on the
    /// default `--household`.
    #[arg(long, value_name = "PLACE@FRAME", value_delimiter = ',', value_parser = parse_press)]
    press: Vec<(usize, u32)>,
    /// Start with the carbine in hand worn down to this condition (0 to
    /// 1000), for a still of the wear. It changes the household it is run
    /// on, so it is refused on the default `--household`.
    #[arg(long, value_name = "POINTS")]
    condition: Option<u16>,
    /// Start with this recruit (`"Tovan Reyes"`) joined to the bench and
    /// out, for a still of what a member who knows a trade sees. It changes
    /// the household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long = "as", value_name = "MEMBER")]
    member: Option<String>,
    /// Start with a patrol carbine made by Tovan Reyes (batch 7, this
    /// moddability, the other lines 650) in the arrival's pack; with
    /// `--open-fitting` it is in hand instead, the bought one in the pack.
    /// It changes the household it is run on, so it is refused on the
    /// default `--household`.
    #[arg(long, value_name = "MODDABILITY")]
    crafted_carbine: Option<u16>,
    /// Start with this many store-grade mount blanks in the pack of
    /// whoever starts out, for a still of a rework. It changes the
    /// household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "N")]
    blanks: Option<u32>,
    /// Start with one of each of these items (comma separated, an item id
    /// each) in the pack of whoever starts out, for a still of the pack
    /// with every kind of stack in it. A lot is copied from the store when
    /// the store holds one. It changes the household it is run on, so it
    /// is refused on the default `--household`.
    #[arg(long, value_name = "ITEM", value_delimiter = ',')]
    give: Vec<String>,
    /// With `--open-fitting`: rework the weapon in the first hand once, by
    /// the request the screen's button sends, before the screen opens. It
    /// spends a blank, so it is refused on the default `--household`.
    #[arg(long, requires = "open_fitting")]
    rework_first: bool,
    /// Hold the combat camera this many metres behind the character, for a
    /// still of what is in the hand.
    #[arg(long, value_name = "METRES")]
    orbit_distance: Option<f32>,
    /// Turn the combat camera this many degrees round the character from
    /// behind it at startup, for a still from the side. The character keeps
    /// facing the view's heading.
    #[arg(long, value_name = "DEGREES")]
    orbit_yaw: Option<f32>,
    /// Start the combat camera at this pitch in degrees (negative looks
    /// down; the default is about -9), for a still of something far off on
    /// ground the default pitch looks over.
    #[arg(long, value_name = "DEGREES", allow_hyphen_values = true)]
    orbit_pitch: Option<f32>,
    /// Where the household (the bench, every sheet and its XP, who is
    /// out) is saved on every change and read from at startup. Singleplayer
    /// and local: nothing of it is replicated.
    #[arg(long, default_value = WORKSPACE_HOUSEHOLD)]
    household: PathBuf,
    /// Ignore a saved household and start the first day over.
    #[arg(long)]
    fresh: bool,
    /// Compile shader pipelines serially when investigating driver compiler crashes.
    #[arg(long)]
    serial_pipelines: bool,
    /// The world seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Place the Rust outpost at three fitted seed-42 sites; choose the arrival site.
    #[arg(long, value_parser = ["flat", "slope", "uneven"], conflicts_with_all = ["connect", "headless", "study_scene", "flight"])]
    grounding_study: Option<String>,
    /// Architectural kit to fit at the study sites.
    #[arg(long, value_enum, requires = "grounding_study")]
    grounding_style: Option<grounding::BuildingStyle>,
    /// Override the saved rendering look with a preset (baseline bypasses saved settings).
    #[arg(long, value_parser = strata::atmosphere::look::PRESETS)]
    render_look: Option<String>,
    /// Reproduce a complete RON render-look export.
    #[arg(long)]
    render_look_file: Option<PathBuf>,
    /// Export effective preset and CLI overrides without opening a window.
    #[arg(long)]
    export_render_look: Option<PathBuf>,
    /// Fixed seed-42 fixture; also selects its initial camera position.
    #[arg(long, value_parser = strata::atmosphere::study::SCENES, conflicts_with_all = ["connect", "headless", "spawn"])]
    study_scene: Option<String>,
    /// Freeze only the preview sun at this day fraction, without pausing the sim.
    #[arg(long, conflicts_with_all = ["connect", "headless"])]
    visual_time: Option<f64>,
    /// Open the live rendering panel on frame 120.
    #[arg(long)]
    open_render_panel: bool,
    /// Run the simulation with no window, no renderer.
    #[arg(long)]
    headless: bool,
    /// Exit after this many frames.
    #[arg(long)]
    frames: Option<u32>,
    /// Save screenshots here. `{frame}` in the name is replaced by the
    /// frame number, which `--shot-frames` needs to keep them apart.
    /// Implies a window; the app exits once every shot is on disk.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Frames to capture on, comma separated. Defaults to `--frames`, or
    /// 60.
    #[arg(long, value_delimiter = ',')]
    shot_frames: Vec<u32>,
    /// Where to start, as `x,z` metres. Defaults to dry land near the
    /// origin.
    #[arg(long, value_parser = parse_xz, allow_hyphen_values = true)]
    spawn: Option<(f64, f64)>,
    /// Start at a named place instead: `city`, the first settlement's
    /// street; `road`, the far end of its road, where the slice's route
    /// begins; `deposit`, at 0.8 of the radius of the deposit
    /// nearest the default start, facing north past it; `deposit:<type>`,
    /// the nearest of one resource type (`deposit:sulphur`); `bench`, in
    /// front of the city's workbench and fabricator, facing them; or
    /// `terminal`, within reach of the city's terminal, facing it; or
    /// `board`, within reach of the city's procurement board, facing it; or
    /// `survey`, within reach of the survey trainer, facing it (`--open-sheet`
    /// there opens on its tree); or `foreman`, within reach of the works
    /// foreman, facing him; `camp`, `wreck` or `machine` on the trail about 120 m from the
    /// one of that kind nearest Port Serein, on its trail, facing it; `claim`, 40 m from
    /// the concession nearest Port Serein, facing it, and `claim:<faction>`
    /// the nearest concession of that faction (`claim:Moment Works`);
    /// Wins over `--spawn`.
    #[arg(long, value_parser = parse_at)]
    at: Option<String>,
    /// The free camera instead of the character: no player, the camera is
    /// the observer.
    #[arg(long)]
    flycam: bool,
    /// Flycam height above terrain, useful for repeatable canopy inspection.
    #[arg(long, default_value_t = CAMERA_ALTITUDE)]
    camera_altitude: f64,
    /// Initial flycam pitch in radians, positive looks upward.
    #[arg(long, default_value_t = -0.22, allow_hyphen_values = true)]
    camera_pitch: f32,
    /// Keep a flycam screenshot view fixed despite live keyboard/mouse input.
    #[arg(long, requires = "flycam", conflicts_with = "flight")]
    lock_camera: bool,
    /// Fly a scripted path at 200 m/s instead of taking input. Implies
    /// `--flycam`.
    #[arg(long)]
    flight: bool,
    /// Walk the character forward (along `--look`) without input, for
    /// screenshots and benchmarks.
    #[arg(long)]
    auto_walk: bool,
    /// Walk the slice's route without input: from the far end of Port
    /// Serein's road into the city, into the lobby, and back out. Implies
    /// `--at road`.
    #[arg(long)]
    route: bool,
    /// Initial camera yaw in radians: 0 looks north (`-z`), positive turns
    /// towards `-x`. The flight heading overrides it.
    #[arg(long, allow_hyphen_values = true)]
    look: Option<f32>,
    /// How long the scripted flight lasts before the app exits, when
    /// `--frames` does not say otherwise.
    #[arg(long, default_value_t = 60.0)]
    flight_secs: f64,
    /// Heading of the scripted flight in radians: 0 flies towards `-z`
    /// (north), positive turns towards `-x`.
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    flight_heading: f64,
    /// The asset root holding `biomes/`, `flora/`, `terrain/` and
    /// `shaders/`.
    #[arg(long, default_value = WORKSPACE_ASSETS)]
    assets: PathBuf,
    /// The content step's output, mounted as the `generated://` asset source:
    /// baked buildings, trees, the tree library and the material library.
    /// `just content` writes it.
    #[arg(long, default_value = WORKSPACE_GENERATED)]
    generated: PathBuf,
    /// Read every file of `assets/` and `generated/` that the game loads
    /// through the files port from this uncompressed data bundle
    /// (`cargo run -p xtask -- bundle <out.gz> --raw <file>`) instead of
    /// the two directories: how the web build's data is checked natively.
    #[arg(long)]
    data_bundle: Option<PathBuf>,
    /// Window size in pixels, as `WxH`. The Phase 2 and 3 targets are
    /// measured at 1440p.
    #[arg(long, default_value = "2560x1440", value_parser = parse_size)]
    window: (u32, u32),
    /// Write a benchmark summary here as JSON when the run finishes: every
    /// frame after `--bench-warmup`, as percentiles, with the GPU time and
    /// what was loaded. `just bench` reads it.
    #[arg(long)]
    bench_out: Option<PathBuf>,
    /// Seconds of real time a benchmark ignores while the world streams in;
    /// with `--bench-warm idle`, the least it waits.
    #[arg(long, default_value_t = 10.0)]
    bench_warmup: f64,
    /// When a benchmark starts measuring: `fixed` after `--bench-warmup`
    /// seconds, `idle` once the foliage streamer has nothing in flight and
    /// has placed nothing for two seconds (after `--bench-warmup`, and by
    /// `--bench-warm-ceiling` at the latest). A static view takes about
    /// 50 s to fill its far trees, so `fixed` measures that streaming.
    /// `hold`, for a moving fixture (`--flight`, `--auto-walk`, `--route`):
    /// the motion waits at its start until terrain (no region waiting for a
    /// mesh, no meshing task held) and foliage are idle, within the same
    /// floor and ceiling, then starts, and measuring begins
    /// `--bench-lead` seconds later.
    #[arg(long, default_value = "fixed", value_parser = ["fixed", "idle", "hold"])]
    bench_warm: String,
    /// The most seconds `--bench-warm idle` or `hold` waits for the
    /// streamers before it goes on anyway; the summary then says `ceiling`.
    #[arg(long, default_value_t = 90.0)]
    bench_warm_ceiling: f64,
    /// With `--bench-warm hold`: seconds the motion runs before measuring,
    /// so the walker is up to speed and the streamers have met the first
    /// ground the motion brings.
    #[arg(long, default_value_t = 5.0)]
    bench_lead: f64,
    /// Exit once a benchmark has measured this many frames after its
    /// warm-up, whatever the warm-up took; `--frames` counts from the start.
    #[arg(long)]
    bench_frames: Option<usize>,
    /// Exit once a benchmark has measured this many seconds of real time
    /// after its warm-up. A walk's length is distance, which a frame count
    /// is not at a frame rate that varies.
    #[arg(long)]
    bench_secs: Option<f64>,
    /// Add Bevy's wireframe plugin, so F4 draws wireframes. Off by default:
    /// the plugin checks every mesh entity every frame for specialization
    /// whether or not F4 is ever pressed.
    #[arg(long)]
    wireframe: bool,
    /// Borderless fullscreen on the primary monitor, for native-resolution captures.
    #[arg(long)]
    fullscreen: bool,
    /// Where in the day the world starts, in `[0, 1)`: 0.25 is sunrise,
    /// 0.5 noon, 0.75 sunset. `[` and `]` scrub it while running.
    #[arg(long)]
    time: Option<f64>,
    /// Camera exposure, EV100. Lower is brighter.
    #[arg(long)]
    ev100: Option<f32>,
    /// Density of the air relative to Earth's; lower is clearer.
    #[arg(long)]
    haze: Option<f32>,
    /// Turn the sun's shadows off, to tell shadow artefacts from material
    /// ones.
    #[arg(long)]
    no_shadows: bool,
    /// Open the pause menu once the world has been drawn for a while
    /// (frame 120), for a screenshot of the screens over it.
    #[arg(long)]
    open_pause: bool,
    /// Open the character sheet on frame 120 (or, with
    /// `--script-progression`, at the blade trainer instead of the lesson,
    /// where the path then ends), for a screenshot.
    #[arg(long)]
    open_sheet: bool,
    /// Open the character sheet on its Loadout tab on frame 120, for a
    /// screenshot.
    #[arg(long)]
    open_loadout: bool,
    /// Open the bench on frame 120 (or, with `--script-progression`, at
    /// the terminal once the recruits joined, instead of the swap), for a
    /// screenshot.
    #[arg(long)]
    open_bench: bool,
    /// Open the stores on frame 120 with the household's store beside the
    /// pack, as a locker's F does, for a screenshot. No locker need be in
    /// reach: the run reaches the store from where it stands.
    #[arg(long)]
    open_stores: bool,
    /// Open the pack on frame 120 without the household's store, as key I
    /// does, for a screenshot.
    #[arg(long)]
    open_inventory: bool,
    /// Open the fitting screen on frame 120 with this mount in focus (the
    /// feed when none is named), with its buttons live as at a workbench,
    /// for a screenshot: no workbench need be in reach. With
    /// `--script-crafting` the script walks to the workbench and uses it as
    /// F would instead, where the path then ends.
    #[arg(long, value_name = "MOUNT", num_args = 0..=1, default_missing_value = "feed")]
    open_fitting: Option<String>,
    /// With `--open-fitting`: the hand whose weapon is on the bench, 0 the
    /// carbine's and 1 the pistol's; the weapon in hand when not given.
    #[arg(long, value_name = "HAND", requires = "open_fitting")]
    fitting_hand: Option<usize>,
    /// With `--open-fitting`: pick the first part the stock has for the
    /// mount, so the still shows what fitting it would do.
    #[arg(long, requires = "open_fitting")]
    fitting_pick: bool,
    /// How the fitting screen draws its weapon: `model`, the baked pieces
    /// turning on their stage, or `diagram`, the flat one. For a still of
    /// either; the model needs `just content` to have run.
    #[arg(long, value_name = "VIEW", default_value = "model", value_parser = ["model", "diagram"])]
    fitting_view: String,
    /// Open the workbench's crafting tab on frame 120 on this schematic
    /// (`cell_core` when none is named, or a full id), with its buttons
    /// live as at a workbench, for a screenshot: no workbench need be in
    /// reach. With `--script-crafting` the script walks to the workbench
    /// and opens it there instead, where the path then ends.
    #[arg(long, value_name = "SCHEMATIC", num_args = 0..=1, default_missing_value = "cell_core")]
    open_crafting: Option<String>,
    /// With `--open-crafting`: which of the screen's states the still
    /// shows. `pick` is the slots with the best stock in them and what
    /// comes out, as the screen opens; `empty` is the same with nothing
    /// picked; `hand` assembles the best stock first, for the bench hand;
    /// `done` also spends the points and finishes, for the finished card;
    /// `record` is that card with Record as blueprint open at
    /// `--record-derate`. The last three change the household they are run
    /// on, so they are refused on the default `--household`: name another.
    #[arg(
        long,
        value_name = "STATE",
        value_enum,
        default_value_t,
        requires = "open_crafting"
    )]
    crafting_state: CraftingShot,
    /// With `--open-crafting`: what the schematics list shows, as its chips
    /// do (Known is the default).
    #[arg(long, value_name = "FILTER", value_enum, requires = "open_crafting")]
    crafting_filter: Option<CraftingListShot>,
    /// Before a still's screen opens, make one of this schematic
    /// (`cell_core`, or a full id) from the best stock, every point spent
    /// steady, by the requests a screen would send: so a still of the
    /// magazine's schematic has a core to pick, and one of the stores a
    /// made thing to pin. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(long, value_name = "SCHEMATIC", conflicts_with = "script_crafting")]
    craft_first: Option<String>,
    /// With `--open-crafting --crafting-state record`: what the blueprint
    /// is drafted at, percent of the prototype, one of the dial's steps.
    #[arg(
        long,
        value_name = "PERCENT",
        default_value_t = 85,
        requires = "open_crafting"
    )]
    record_derate: u16,
    /// With `--craft-first`: record what it made as a blueprint at this
    /// percent of it (85 when none is named), by the request the Record
    /// box would send, so a still of the factory has a blueprint to show.
    #[arg(
        long,
        value_name = "PERCENT",
        num_args = 0..=1,
        default_missing_value = "85",
        requires = "craft_first"
    )]
    record_first: Option<u16>,
    /// Open the factory on frame 120, as a fabricator's F does, for a
    /// screenshot: no fabricator need be in reach.
    #[arg(long)]
    open_factory: bool,
    /// Open Pelagic Works' procurement board on frame 120, as a board's F
    /// does, for a screenshot: no board need be in reach.
    #[arg(long)]
    open_board: bool,
    /// Before a still's board opens, put 100 cell cores of batch 412 by
    /// Tovan Reyes at charge density 724 in the store, so the board has
    /// something to quote. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(long, requires = "open_board", conflicts_with = "script_crafting")]
    board_stock: bool,
    /// With `--open-board`: a few frames after the board opens, its first
    /// row's sale is sent, by the request its button would send. It
    /// changes the household it is run on, so it is refused on the default
    /// `--household`: name another.
    #[arg(long, requires = "open_board", conflicts_with = "script_crafting")]
    board_sell: bool,
    /// Before a still's screen opens, start a run of this many (20 when
    /// none is named, and no more than the stock feeds) of the first
    /// blueprint in reach from the cheapest stock that meets it, by the
    /// request a screen would send: so a still of the factory or of the
    /// bench has a run on. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(
        long,
        value_name = "N",
        num_args = 0..=1,
        default_missing_value = "20",
        conflicts_with = "script_crafting"
    )]
    factory_run: Option<u32>,
    /// With `--factory-run`: somebody on the bench works the run, the
    /// first recruit joining when nobody is there to.
    #[arg(long, requires = "factory_run")]
    factory_worker: bool,
    /// With `--factory-run`: the still's screen opens once the run has
    /// finished and not when it has started, for a still of what it made.
    /// The run takes its played time, so the screenshot's frame must be
    /// late enough for it.
    #[arg(long, requires = "factory_run")]
    factory_finish: bool,
    /// With `--factory-run`: once the run has started, every empty cell of
    /// the store is taken, so the run stands for want of room when its
    /// first unit is made, for a still of the factory paused. The
    /// screenshot's frame must be a unit's played time after the start.
    #[arg(long, requires = "factory_run", conflicts_with = "factory_finish")]
    factory_full_store: bool,
    /// Before a still's screen opens, everyone who can joins the bench, as
    /// at a terminal: so a still of the bench has more than one member on
    /// it. It changes the household it is run on, so it is refused on the
    /// default `--household`: name another.
    #[arg(long, conflicts_with = "script_crafting")]
    join_all: bool,
    /// Before a still of the factory opens, put in the store sixty units
    /// of the nearest deposit's lot that fits a slot of the first
    /// blueprint in reach and does not meet it, so the still shows a lot
    /// refused in place with its number. It changes the household it is
    /// run on, so it is refused on the default `--household`: name
    /// another.
    #[arg(long, conflicts_with = "script_crafting")]
    factory_poor_lot: bool,
    /// With `--open-stores` or `--open-inventory`: the filter chip in
    /// force (`all`, `lots`, `parts`, `mods`, `blueprints`, `gear`, `bags`),
    /// with the first stack it shows pinned, for a screenshot of one kind
    /// of stock.
    #[arg(long, value_name = "FILTER", value_parser = parse_filter)]
    stores_filter: Option<strata::client::ui::stores::Filter>,
    /// With `--stores-filter`: pin the stack this many after the first the
    /// filter shows, in the order of the screen's cells (the last when
    /// there are fewer), for a still of a stack that is not the first of
    /// its kind.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        requires = "stores_filter"
    )]
    stores_pin: usize,
    /// Open the survey screen on frame 120, for a screenshot: the tool's
    /// ping, its last grid and the deposits it met.
    #[arg(long)]
    open_survey: bool,
    /// Open the map on frame 120 at a scale (`near`, `region` or `far`;
    /// the region's when none is named), for a screenshot.
    #[arg(long, value_name = "SCALE", num_args = 0..=1, default_missing_value = "region")]
    open_map: Option<String>,
    /// Lay the map's Holders layer on, for a screenshot of whose ground it is.
    #[arg(long, requires = "open_map")]
    map_holders: bool,
    /// Ping for a class of resource (`conductor`, or a full id) once the
    /// player stands in the world, as the survey screen's Ping would, so a
    /// still has a grid to show. The channel plays out in the first
    /// seconds; the household finds what it meets.
    #[arg(long, value_name = "CLASS")]
    ping: Option<String>,
    /// Start sampling where the player stands once a ping has found a
    /// deposit there (`--ping`), as Sample here would. It puts lots in the
    /// pack, so it is refused on the default `--household`.
    #[arg(long)]
    sampling: bool,
    /// Open the terminal's survey register on frame 120, read-only as the
    /// pause's Bench button gives it, for a screenshot.
    #[arg(long)]
    open_register: bool,
    /// Track the nearest deposit of this resource type to the default
    /// start (`light_alloy_ore`, or a full id), as Accept on its row of
    /// the register does, so the HUD's marker is up for a screenshot.
    #[arg(long, value_name = "TYPE")]
    track: Option<String>,
    /// Hold the trigger down from frame 120 for this many seconds, as the
    /// left button would, for a screenshot of the reticle as the cone
    /// grows. It fires the player's rounds, so it is refused on the
    /// default `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    hold_fire: Option<f32>,
    /// Hold the player's guard (the aim button with a melee weapon in
    /// hand) for this many seconds from frame 120, for a screenshot of the
    /// raised blade. With `--script-progression --script-hurt n` a hostile's
    /// blow meets it. Refused on the default `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    hold_guard: Option<f32>,
    /// Hold the player's aim button for this many seconds from frame 120,
    /// for a screenshot of a cone closing and of the scoped view. With a
    /// melee weapon in hand it is the guard. Refused on the default
    /// `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    hold_aim: Option<f32>,
    /// Every outlaw camp stands up at this tier whatever its distance from
    /// Port Serein, for a still of the bigger crowd.
    #[arg(long, value_name = "TIER", value_parser = clap::value_parser!(u8).range(1..=3))]
    camp_tier: Option<u8>,
    /// The camp nearest the start is cleared from the first frame, for a
    /// still of the card on a quiet camp. It writes the household, so it is
    /// refused on the default `--household`.
    #[arg(long)]
    cleared: bool,
    /// The camp nearest the start is cleared from the first frame with its
    /// cache standing, for a still of the chest and its F chip. It writes
    /// the household, so it is refused on the default `--household`.
    #[arg(long)]
    cache: bool,
    /// Take this posting (1 is the board's first) of Pelagic's board from the
    /// first frame, for a still of the work and of the area heard of. It
    /// writes the household, so it is refused on the default `--household`.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(usize))]
    take_job: Option<usize>,
    /// With `--take-job`: the taken job's camp counts cleared after the take,
    /// for a still of the board's Collect.
    #[arg(long, requires = "take_job")]
    job_done: bool,
    /// The camp of this name, or the nearest to the start, starts found, for
    /// a still of its point on the map. It writes the household, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "NAME|nearest")]
    found: Option<String>,
    /// The member who is out is carried home at frame 200 wherever they are,
    /// through the path a fall in the world takes. It writes the household,
    /// so it is refused on the default `--household`.
    #[arg(long)]
    fall: bool,
    /// This member starts injured for the fall's hours, for a still of the
    /// bench card. It writes the household, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "MEMBER")]
    injured: Option<String>,
    /// Start with a faction's stance set, `name=welcoming|wary|hostile`, by
    /// a deed written at the start so the score sits in the band. Repeat for
    /// more. Refused on the default `--household`: name another.
    #[arg(long, value_name = "FACTION=STANCE")]
    stance: Vec<String>,
    /// Start with a live lease of Pelagic Works' concessions, for a still of
    /// the board and of working a concession lawfully. Refused on the
    /// default `--household`: name another.
    #[arg(long)]
    lease: bool,
    /// Start the encounter with no hostiles in it: the character, the
    /// household and the world as they are, with no fight and no
    /// nameplates, for a screenshot of a place.
    #[arg(long, conflicts_with_all = ["no_combat", "script_progression", "combat_script"])]
    no_hostiles: bool,
    /// Replace the encounter's hostiles with these, `kind[@metres]` each,
    /// comma separated: `marksman@60`, `duelist@6`, `sentry,rusher@10`.
    /// Kinds are sentry, rusher, marksman and duelist; a missing distance is
    /// the kind's own, straight ahead of the start. For a still or a test.
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = combat::parse_hostile,
        conflicts_with_all = ["no_hostiles", "no_combat", "script_progression", "combat_script"],
    )]
    hostiles: Vec<combat::HostileSpec>,
    /// With `--script-progression`: the nearest hostile lands this many
    /// hits on the player from four seconds into the fight, a second
    /// apart, which no scripted run otherwise does. One shows the hurt
    /// path for a screenshot; eight bring the low-health pulse and the
    /// fall.
    #[arg(long, default_value_t = 0)]
    script_hurt: u32,
    /// Start with the debug statistics shown (F3 toggles them either
    /// way). Off by default so a screenshot is of the game; the numbers
    /// reach the log and `--bench-out` without the overlay.
    #[arg(long)]
    stats: bool,
    /// Connect to a server at `host:port` instead of running the
    /// simulation locally: the server owns the world and spawns the
    /// player, this process renders and predicts it. `--seed` must match
    /// the server's.
    #[arg(long)]
    connect: Option<SocketAddr>,
    /// No grass, no trees.
    #[arg(long)]
    no_foliage: bool,
    /// No rivers' or lakes' water drawn (the sea stays): the regions'
    /// tasks build none. For a benchmark or a comparison of what the water
    /// costs.
    #[arg(long)]
    no_rivers: bool,
    /// No rivers or roads painted on the far terrain: the regions' tasks
    /// gather none and the terrain's shader is compiled without the paint.
    /// For a comparison of what the paint costs, or a GPU that cannot
    /// spare it.
    #[arg(long)]
    no_far_paint: bool,
    /// No falls drawn over the rivers' ramps (ticket 0005, step 8): no
    /// sheet, foam or spray. For a comparison of what they cost.
    #[arg(long)]
    no_falls: bool,
    /// Wind strength, metres of tip displacement for a one metre plant at
    /// full gust. Zero is a still day.
    #[arg(long)]
    wind: Option<f32>,
    /// Anti-aliasing: `auto` follows the preset; explicit choices override it.
    #[arg(long, default_value = "auto", value_parser = ["auto", "msaa4", "off", "taa"])]
    aa: String,
    /// Ground fog (ticket 0003): scale on the fog field's density. `0`
    /// draws none and registers no fog pass and no fog system (what the
    /// benches run with). The render look can also leave it out (F6,
    /// "Valley fog").
    #[arg(long, default_value_t = 1.0, value_parser = parse_fog_scale)]
    fog: f32,
    /// Pin the weather the fog is drawn under, as `fog,sea_fog`, each 0 to
    /// 1: basin fog from none to every hollow full, sea fog from a clear sea
    /// to banks over most of it. Until the weather moves with time the
    /// default is fixed: `1,0.5`.
    #[arg(long, value_parser = parse_weather)]
    weather: Option<(f32, f32)>,
    /// Ground fog raymarch steps per pixel, 1 to 128 (default 12), the
    /// first thing to trade for its cost. Not the F6 panel's "Raymarch
    /// steps", which are Bevy's volumetric fog's in the study scenes.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=128))]
    fog_steps: Option<u32>,
}

/// The ground fog's settings from the command line. Inserted before the
/// plugins, because the pass registers its render systems only when it
/// finds them at build time.
fn ground_fog(cli: &Cli) -> GroundFog {
    let mut fog = GroundFog {
        scale: cli.fog,
        ..default()
    };
    if let Some((basins, sea)) = cli.weather {
        fog.weather.fog = basins;
        fog.weather.sea_fog = sea;
    }
    if let Some(steps) = cli.fog_steps {
        fog.steps = steps;
    }
    fog
}

fn parse_fog_scale(s: &str) -> Result<f32, String> {
    let v = s
        .trim()
        .parse::<f32>()
        .map_err(|e| format!("{s:?} is not a number: {e}"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!(
            "the fog scale must be finite and at least 0, not {v}"
        ));
    }
    Ok(v)
}

fn parse_weather(s: &str) -> Result<(f32, f32), String> {
    let (fog, sea) = s
        .split_once(',')
        .ok_or_else(|| format!("expected fog,sea_fog but got {s:?}"))?;
    let parse = |v: &str| {
        let v = v
            .trim()
            .parse::<f32>()
            .map_err(|e| format!("{v:?} is not a number: {e}"))?;
        if (0.0..=1.0).contains(&v) {
            Ok(v)
        } else {
            Err(format!("weather values are 0 to 1, not {v}"))
        }
    };
    Ok((parse(fog)?, parse(sea)?))
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WxH but got {s:?}"))?;
    let parse = |v: &str| {
        v.trim()
            .parse::<u32>()
            .map_err(|e| format!("{v:?} is not a size: {e}"))
    };
    Ok((parse(w)?, parse(h)?))
}

fn parse_xz(s: &str) -> Result<(f64, f64), String> {
    let (x, z) = s
        .split_once(',')
        .ok_or_else(|| format!("expected x,z but got {s:?}"))?;
    let parse = |v: &str| {
        v.trim()
            .parse::<f64>()
            .map_err(|e| format!("{v:?} is not a number: {e}"))
    };
    Ok((parse(x)?, parse(z)?))
}

fn resolve_render_look(cli: &Cli) -> Result<LookFile, String> {
    let saved_path = strata::client::ui::default_settings_path().with_file_name("render-look.ron");
    resolve_render_look_at(cli, &saved_path)
}

fn saved_render_look(path: &std::path::Path) -> LookFile {
    let text = match strata::sim::store::current().read(&path.to_string_lossy()) {
        Ok(Some(text)) => text,
        Ok(None) => return LookFile::default(),
        Err(error) => {
            eprintln!(
                "could not load saved look {}: {error}; using baseline",
                path.display()
            );
            return LookFile::default();
        }
    };
    LookFile::from_ron(&text).unwrap_or_else(|error| {
        eprintln!(
            "invalid saved look {}: {error}; using baseline",
            path.display()
        );
        LookFile::default()
    })
}

fn resolve_render_look_at(cli: &Cli, saved_path: &std::path::Path) -> Result<LookFile, String> {
    let mut file = if let Some(path) = &cli.render_look_file {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        LookFile::from_ron(&text)?
    } else if let Some(name) = &cli.render_look {
        LookFile::preset(name)?
    } else {
        saved_render_look(saved_path)
    };
    if let Some(ev100) = cli.ev100 {
        file.sky.ev100 = ev100;
    }
    if let Some(haze) = cli.haze {
        file.sky.haze = haze;
    }
    if cli.no_shadows {
        file.sky.shadows = false;
    }
    file.validate()?;
    Ok(file)
}

/// `--crafting-state`: the state of the crafting screen a still is of.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CraftingShot {
    /// The slots with nothing picked.
    Empty,
    /// The slots, their candidates and what comes out.
    #[default]
    Pick,
    /// After Assemble: the bench hand.
    Hand,
    /// After Finish: the finished card.
    Done,
    /// The finished card with Record as blueprint open.
    Record,
}

/// `--crafting-filter`: the chip of the crafting list a still is of.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum CraftingListShot {
    /// What the member was taught.
    Known,
    /// What can be made now.
    Ready,
    /// Everything.
    All,
}

impl CraftingListShot {
    /// The list's filter this stands for.
    const fn filter(self) -> strata::client::ui::crafting::ListFilter {
        use strata::client::ui::crafting::ListFilter;
        match self {
            Self::Known => ListFilter::Known,
            Self::Ready => ListFilter::Ready,
            Self::All => ListFilter::All,
        }
    }
}

impl CraftingShot {
    /// What the screen is asked to bring itself to; `derate` is what a
    /// Record box opens at, percent of the prototype.
    const fn still(self, derate: u16) -> strata::client::ui::crafting::Still {
        use strata::client::ui::crafting::Still;
        match self {
            Self::Empty => Still::Empty,
            Self::Pick => Still::Pick,
            Self::Hand => Still::Hand,
            Self::Done => Still::Done,
            // The sim counts a share in per-mille.
            Self::Record => Still::Record(derate.saturating_mul(10)),
        }
    }
}

impl Cli {
    /// `--crafting-state` as the crafting screen takes it.
    const fn crafting_still(&self) -> strata::client::ui::crafting::Still {
        self.crafting_state.still(self.record_derate)
    }

    /// Why this run may not start, when it is a still that spends stock
    /// (assembles, rolls, finishes, starts a run) on the household the
    /// game is played from: the flags send the requests a player's presses
    /// would, and what they do is saved. The scripted walk is no still and
    /// is not held to this.
    fn spends_the_played_household(&self) -> Option<String> {
        let flag = if self.craft_first.is_some() {
            "--craft-first"
        } else if self.factory_run.is_some() {
            "--factory-run"
        } else if self.factory_poor_lot {
            "--factory-poor-lot"
        } else if self.cleared {
            "--cleared"
        } else if self.cache {
            "--cache"
        } else if self.take_job.is_some() {
            "--take-job"
        } else if self.found.is_some() {
            "--found"
        } else if self.fall {
            "--fall"
        } else if self.injured.is_some() {
            "--injured"
        } else if !self.stance.is_empty() {
            "--stance"
        } else if self.lease {
            "--lease"
        } else if self.hold_fire.is_some() {
            "--hold-fire"
        } else if self.hold_guard.is_some() {
            "--hold-guard"
        } else if self.hold_aim.is_some() {
            "--hold-aim"
        } else if self.board_stock {
            "--board-stock"
        } else if self.board_sell {
            "--board-sell"
        } else if self.join_all {
            "--join-all"
        } else if !self.learn.is_empty() {
            "--learn"
        } else if !self.in_hand.is_empty() {
            "--in-hand"
        } else if !self.grant.is_empty() {
            "--grant"
        } else if !self.press.is_empty() {
            "--press"
        } else if self.condition.is_some() {
            "--condition"
        } else if self.member.is_some() {
            "--as"
        } else if self.crafted_carbine.is_some() {
            "--crafted-carbine"
        } else if self.blanks.is_some() {
            "--blanks"
        } else if !self.give.is_empty() {
            "--give"
        } else if self.purse.is_some() {
            "--purse"
        } else if self.saddle_stock {
            "--saddle-stock"
        } else if self.harvester {
            "--harvester"
        } else if self.harvest_hours.is_some() {
            "--harvest-hours"
        } else if self.bike_condition.is_some() {
            "--bike-condition"
        } else if self.rework_first {
            "--rework-first"
        } else if self.sampling {
            "--sampling"
        } else if self.ping.is_some() {
            "--ping"
        } else if self.script_survey.is_some() {
            "--script-survey"
        } else if self.open_crafting.is_some()
            && !self.script_crafting
            && matches!(
                self.crafting_state,
                CraftingShot::Hand | CraftingShot::Done | CraftingShot::Record
            )
        {
            "--crafting-state hand|done|record"
        } else {
            return None;
        };
        (self.household == Path::new(WORKSPACE_HOUSEHOLD)).then(|| {
            format!(
                "{flag} spends the stock of the household it is run on and moves its roll and batch counters: pass --household <path> (with --fresh for a first day), not the default {WORKSPACE_HOUSEHOLD}"
            )
        })
    }

    fn resolve_play_mode(&mut self) {
        if self.script_survey.is_some() {
            // Nobody to fight on the way: the script is about the ground.
            self.no_hostiles = true;
        }
        if self.script_ride && self.at.is_none() {
            self.at = Some("road".to_owned());
        }
        if self.script_crafting {
            self.script_progression = true;
        }
        if self.script_progression {
            self.combat_script = true;
        }
        self.combat = !self.no_combat
            && self.connect.is_none()
            && !self.flycam
            && !self.flight
            && !self.route
            && !self.auto_walk
            && self.grounding_study.is_none()
            && self.study_scene.is_none();
    }
}

/// `--at`'s value: one of the settlement's places, or a deposit.
fn parse_at(text: &str) -> Result<String, String> {
    if matches!(
        text,
        "city"
            | "road"
            | "bench"
            | "terminal"
            | "board"
            | "survey"
            | "foreman"
            | "camp"
            | "wreck"
            | "machine"
    ) || deposits::is_deposit(text)?
        || camps::claim_spec(text).is_some()
    {
        Ok(text.to_owned())
    } else {
        Err(
            "expected `city`, `road`, `bench`, `terminal`, `board`, `survey`, `foreman`, `camp`, `wreck`, `machine`, `claim`, `claim:<faction>`, `deposit` or `deposit:<type>`"
                .to_owned(),
        )
    }
}

/// `--stores-filter`'s value: a filter chip of the stores screen, by the
/// word in its tag.
fn parse_filter(text: &str) -> Result<strata::client::ui::stores::Filter, String> {
    use strata::client::ui::stores::Filter;
    Filter::ALL
        .into_iter()
        .find(|filter| filter.id() == text)
        .ok_or_else(|| {
            let known: Vec<&str> = Filter::ALL.iter().map(|filter| filter.id()).collect();
            format!("expected one of {}", known.join(", "))
        })
}

/// `--at bench` and `--at terminal`: where to stand and which way to look.
fn at_bench(
    cli: &Cli,
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    match cli.at.as_deref() {
        Some("bench") => household::bench_view(settlements),
        Some("terminal") => household::terminal_view(settlements),
        Some("board") => household::board_view(settlements),
        Some("survey") => household::survey_view(settlements),
        Some("foreman") => household::foreman_view(settlements),
        _ => None,
    }
}

/// Whether `--at` names a deposit.
fn at_deposit(cli: &Cli) -> Option<&str> {
    cli.at
        .as_deref()
        .filter(|at| deposits::is_deposit(at) == Ok(true))
}

fn initial_heading(cli: &Cli, settlements: Option<&strata::sim::Settlements>) -> f32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "camera heading is render-space f32"
    )]
    cli.look.unwrap_or_else(|| {
        if at_deposit(cli).is_some() || cli.script_survey.is_some() {
            // A start at a deposit is south of it and looks north.
            std::f32::consts::PI
        } else if let Some((_, yaw)) = at_bench(cli, settlements) {
            yaw
        } else if cli.combat && cli.spawn.is_none() && cli.at.is_none() {
            settlements
                .and_then(|s| s.0.first())
                .map_or(0., |s| s.world_yaw(-std::f64::consts::FRAC_PI_2) as f32)
        } else {
            0.
        }
    })
}

fn main() -> AppExit {
    #[cfg(not(target_arch = "wasm32"))]
    let mut cli = Cli::parse();
    #[cfg(target_arch = "wasm32")]
    let mut cli = web::cli();
    cli.resolve_play_mode();
    if let Some(refused) = cli.spends_the_played_household() {
        fail(&refused);
        return AppExit::error();
    }
    if let Some(scene) = cli.study_scene.as_deref() {
        if cli.seed != 42 {
            fail("study scenes require --seed 42");
            return AppExit::error();
        }
        cli.spawn = location(scene);
    }
    if cli
        .visual_time
        .is_some_and(|t| !t.is_finite() || !(0.0..1.0).contains(&t))
    {
        fail("--visual-time must be finite and in [0, 1)");
        return AppExit::error();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &cli.export_render_look {
        let result = resolve_render_look(&cli)
            .and_then(|f| f.to_ron())
            .and_then(|text| std::fs::write(path, text).map_err(|e| e.to_string()));
        return match result {
            Ok(()) => AppExit::Success,
            Err(error) => {
                eprintln!("export: {error}");
                AppExit::error()
            }
        };
    }
    // Every read of game data below goes through this source, so the same
    // code runs where there is no directory to read (ticket 0013).
    #[cfg(not(target_arch = "wasm32"))]
    install_files(&cli);
    #[cfg(target_arch = "wasm32")]
    web::install_ports();
    if let Err(error) = install_combat_tables(&cli) {
        fail(&error);
        return AppExit::error();
    }
    // The rivers round the spawn and the settlements, before any task pool
    // starts (ADR 0010): the loader's world shares the bake.
    match strata::sim::prepare_water_before_start_from(
        &Files::installed(),
        Seed(cli.seed),
        &prepare_water(&cli),
    ) {
        Ok((masses, seconds)) => {
            report(&format!(
                "rivers: {masses} land masses baked in {seconds:.2} s"
            ));
        }
        Err(error) => {
            fail(&error.to_string());
            return AppExit::error();
        }
    }
    let prepared = if cli.grounding_study.is_some() {
        match grounding::prepare(&mut cli) {
            Ok(value) => Some(value),
            Err(error) => {
                fail(&format!("grounding: {error:#}"));
                return AppExit::error();
            }
        }
    } else {
        None
    };
    run(cli, prepared)
}

/// A progress line before the logger exists: stdout natively, the page's
/// console in a browser, where stdout goes nowhere.
fn report(line: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    web::console(line);
}

/// An error that ends the start-up: stderr natively; in a browser the
/// console and the page, whose loading line would otherwise stay up for ever.
fn fail(line: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{line}");
    #[cfg(target_arch = "wasm32")]
    web::fail(line);
}

/// Install the native adapters of the two ports the game reads and writes
/// through: `--assets` and `--generated` as the logical `assets/` and
/// `generated/`, and files on disk as the store of saves and settings.
///
/// The web entry installs its own adapters instead (step 3).
#[cfg(not(target_arch = "wasm32"))]
fn install_files(cli: &Cli) {
    if let Some(path) = &cli.data_bundle {
        match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                strata::registry::bundle::source_owned(bytes).map_err(|e| e.to_string())
            }) {
            Ok(source) => strata::registry::files::install(Box::new(source)),
            Err(error) => {
                // A bundle that cannot be read leaves no honest fallback.
                eprintln!("--data-bundle {}: {error}", path.display());
                std::process::exit(2);
            }
        }
    } else {
        strata::registry::files::install(Box::new(
            strata::registry::files::MountedDirs::new()
                .mount("assets", &cli.assets)
                .mount("generated", &cli.generated),
        ));
    }
    strata::sim::store::install(Box::new(strata::sim::store::FileStore));
}

/// The installed source, or, in a test, which installs none, the two
/// directories the command line names.
fn data_files(cli: &Cli) -> Files {
    if strata::registry::files::is_installed() {
        Files::installed()
    } else {
        Files::new(
            strata::registry::files::MountedDirs::new()
                .mount("assets", &cli.assets)
                .mount("generated", &cli.generated),
        )
    }
}

/// The loader over the installed source; a test, which installs none, reads
/// its directories.
fn world_load(cli: &Cli) -> WorldLoadPlugin {
    let plugin = WorldLoadPlugin::new(&cli.assets).with_generated(&cli.generated);
    plugin.with_files(data_files(cli))
}

/// Load `assets/combat/` and install it before any plugin builds: the
/// local sim, the prediction and the encounter all read the tables through
/// `Action::spec` and friends, and the handshake carries their version
/// (`client_replication` reads it back from the installed tables).
fn install_combat_tables(cli: &Cli) -> Result<(), String> {
    let assets = data_files(cli).under("assets");
    let tables = CombatTables::load_from(&assets).map_err(|error| error.to_string())?;
    // Where the skill graph meets the abilities: a box that grants or
    // changes an ability the table has no row for is a plain error here.
    if let Ok(text) = assets.read_string(SkillGraph::FILE)
        && let Ok(graph) = SkillGraph::parse(&text)
    {
        strata::sim::ability::check_grants(&graph, &tables.abilities)?;
    }
    strata::sim::combat_tables::install(tables);
    Ok(())
}

/// The app a command line asks for, built and not yet run, so a test can
/// run one to its end in process.
///
/// # Errors
/// What was wrong with the command line, to print before exiting.
#[allow(
    clippy::too_many_lines,
    reason = "the two plugin stacks and every flag's resource, in one place"
)]
fn build(cli: Cli, prepared: Option<grounding::Prepared>) -> Result<App, String> {
    let mut app = App::new();
    app.insert_resource(WorldSeed(Seed(cli.seed)));
    app.insert_resource(prepare_water(&cli));

    if cli.headless {
        // A scripted run's clock is manual (one tick a frame), so pacing
        // the loop would only make it slower.
        let wait = if cli.combat_script || cli.script_survey.is_some() {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(1.0 / 60.0)
        };
        app.add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(wait)),
            bevy::log::LogPlugin::default(),
            StrataPlugins::headless(),
            world_load(&cli),
        ))
        .add_systems(
            OnEnter(AppState::InWorld),
            (
                deposits::init,
                factions::init,
                camps::resolve_at,
                household::init.run_if(resource_exists::<household::HouseholdFile>),
                spawn_headless_player,
            )
                .chain(),
        )
        .add_systems(Update, log_headless_progress);
    } else {
        let selected =
            resolve_render_look(&cli).map_err(|error| format!("render look: {error}"))?;
        let sky = selected.sky;
        app.insert_resource(selected.look);
        app.insert_resource(sky);
        app.insert_resource(StudySettings {
            scene: cli.study_scene.clone(),
            visual_time: cli.visual_time,
        });
        app.insert_resource(FoliageConfig {
            enabled: !cli.no_foliage,
            ..default()
        });
        app.insert_resource(TerrainConfig {
            water: !cli.no_rivers,
            paint: !cli.no_far_paint,
            falls: !cli.no_falls,
            ..default()
        });
        app.insert_resource(ground_fog(&cli));
        // Before `StrataPlugins::client()` reads the settings path.
        #[cfg(target_arch = "wasm32")]
        web::insert_resources(&mut app);
        if cli.wireframe {
            app.insert_resource(strata::client::debug::WireframeWanted);
        }
        // A source has to exist before the asset plugin builds its server.
        app.register_asset_source(
            "generated",
            AssetSourceBuilder::platform_default(&cli.generated.to_string_lossy(), None),
        );
        app.add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Veyra".into(),
                        // The page's canvas, sized by its parent; native
                        // windows have neither.
                        #[cfg(target_arch = "wasm32")]
                        canvas: Some("#veyra".into()),
                        #[cfg(target_arch = "wasm32")]
                        fit_canvas_to_parent: true,
                        // Normal previews are uncapped. The grounding study uses
                        // vsync to bound GPU work during verification.
                        resolution: WindowResolution::new(cli.window.0, cli.window.1),
                        present_mode: if cli.grounding_study.is_some() {
                            PresentMode::Fifo
                        } else {
                            PresentMode::AutoNoVsync
                        },
                        mode: if cli.fullscreen {
                            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
                        } else {
                            WindowMode::Windowed
                        },
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: cli.assets.to_string_lossy().into_owned(),
                    // No `.meta` file sits beside an asset, and over HTTP
                    // every look for one is a request that answers 404.
                    #[cfg(target_arch = "wasm32")]
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: cli.serial_pipelines,
                    render_creation: RenderCreation::Automatic(Box::new(wgpu_settings())),
                    ..default()
                }),
            StrataPlugins::client(),
            world_load(&cli),
            AshlarPlugin::default(),
        ))
        .add_systems(
            OnEnter(AppState::InWorld),
            (
                deposits::init,
                factions::init,
                camps::resolve_at,
                household::init.run_if(resource_exists::<household::HouseholdFile>),
                spawn_camera,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                take_screenshots,
                open_pause_for_shot,
                open_screens_for_shot,
                open_render_for_shot,
                track_for_shot,
                ping_for_shot,
                sampling_for_shot,
            ),
        )
        // After the camera set, which writes the trigger from the mouse (and
        // clears it in an unfocused window), so the held trigger is the
        // last word.
        .init_resource::<GuardHold>()
        .add_systems(
            Update,
            (hold_fire_for_shot, hold_guard_for_shot, hold_aim_for_shot)
                .after(strata::client::ClientSet::Camera),
        );
        let generated = data_files(&cli).under("generated");
        // The weapon pictures' index, read once; without it the diagram
        // draws boxes. A window's business only: nothing below the client
        // knows the pictures exist. After the plugins, so the log plugin
        // is there to say which it was.
        app.insert_resource(strata::client::ui::weapon_diagram::WeaponPictures::read(
            &generated,
        ));
        // The weapon models' index likewise: without it the hand keeps the
        // old art.
        app.insert_resource(strata::client::weapon_model::WeaponModels::read(&generated));
        // The bike's likewise: without it a box stands under the rider.
        app.insert_resource(strata::client::ride_art::BikeModels::read(&generated));
        app.insert_resource(if cli.fitting_view == "diagram" {
            strata::client::ui::fitting::WellView::Diagram
        } else {
            strata::client::ui::fitting::WellView::Model
        });
        settlement::install(&mut app);
        sites::install(&mut app);
        crossings::install(&mut app);
        roadside::install(&mut app);
        shutdown::install(&mut app);
        // A scripted run (a benchmark, a flight, a capture, a frame count)
        // must not depend on whether its window happened to get focus:
        // Bevy's game default drops an unfocused window to 60 Hz, which a
        // benchmark would record as the game's frame rate.
        if cli.frames.is_some() || cli.flight || cli.screenshot.is_some() || cli.bench_out.is_some()
        {
            app.insert_resource(bevy::winit::WinitSettings::continuous());
        }
        #[cfg(not(feature = "net"))]
        if cli.connect.is_some() {
            return Err("this build has no network support (the `net` feature)".to_owned());
        }
        #[cfg(feature = "net")]
        if let Some(server) = cli.connect {
            // The adapter as the client, after the facade so it finds the
            // sim's states and sets, and the scripted walk for a player
            // that arrives from the server. The other players' capsules
            // are the client plugin's own business.
            let Some(replication) = client_replication(server, &cli) else {
                return Err("could not configure replication".to_owned());
            };
            app.add_plugins(replication)
                .add_systems(Update, script_connected_player);
        }
    }

    // The debug statistics only when asked for, and never over a
    // screenshot of a screen, whose corner they would cover.
    if !cli.headless {
        app.insert_resource(strata::client::hud::HudVisible(
            cli.stats
                && !(cli.open_sheet
                    || cli.open_loadout
                    || cli.open_bench
                    || cli.open_stores
                    || cli.open_inventory
                    || cli.open_survey
                    || cli.open_map.is_some()
                    || cli.open_register
                    || cli.open_fitting.is_some()
                    || cli.open_crafting.is_some()
                    || cli.open_factory
                    || cli.open_board),
        ));
    }
    // A still of the stores is taken wherever the run stands, which is
    // seldom at a locker, so for that run the store is reached from
    // anywhere. The rule itself is the sim's and stays the rule.
    if cli.open_stores {
        app.insert_resource(strata::sim::items::StoreAccess::Anywhere);
    }
    // The same for a still of the fitting screen or the crafting screen and
    // the workbench, and of the factory and the fabricator, which is held
    // to the same lifted rule, unless the script walks to one.
    if (cli.open_fitting.is_some()
        || cli.open_crafting.is_some()
        || cli.craft_first.is_some()
        || cli.open_factory
        || cli.open_board
        || cli.factory_run.is_some()
        || cli.factory_poor_lot)
        && !cli.script_progression
        && !cli.script_crafting
    {
        app.insert_resource(strata::sim::fitting::WorkbenchAccess::Anywhere);
    }
    if cli.rework_first && !cli.script_progression {
        app.add_systems(Update, rework_first_for_shot);
    }
    if cli.craft_first.is_some() && !cli.script_progression {
        app.init_resource::<CraftFirst>()
            .add_systems(Update, craft_first_for_shot);
    }
    install_scenarios(&mut app, &cli, prepared);
    insert_overrides(&mut app, &cli);
    combat::install(&mut app, &cli);
    if !cli.press.is_empty() {
        app.add_systems(
            Update,
            press_for_shot.after(strata::client::ClientSet::Camera),
        );
    }
    household::install(&mut app, &cli);
    ride::install(&mut app, &cli);
    survey_script::install(&mut app, &cli);
    app.insert_resource(cli)
        .init_resource::<Shots>()
        .add_systems(
            Update,
            (exit_when_done, report_stalled_walker, release_scripted_walk),
        );
    Ok(app)
}

/// The adapter features asked for beyond what Bevy enables itself. The
/// wireframe toggle (F4) draws with line polygon mode, which wgpu only
/// enables on request and WebGPU does not have; nothing here draws with
/// immediates directly, and Bevy takes them where the adapter has them.
fn wgpu_settings() -> WgpuSettings {
    WgpuSettings {
        #[cfg(not(target_arch = "wasm32"))]
        features: WgpuFeatures::POLYGON_MODE_LINE | WgpuFeatures::IMMEDIATES,
        ..default()
    }
}

fn run(cli: Cli, prepared: Option<grounding::Prepared>) -> AppExit {
    match build(cli, prepared) {
        Ok(mut app) => {
            #[cfg(target_arch = "wasm32")]
            web::install(&mut app);
            app.run()
        }
        Err(error) => {
            fail(&error);
            AppExit::error()
        }
    }
}

/// The resources a flag overrides or turns on: the benchmark recorder, the
/// time of day and the wind.
fn insert_overrides(app: &mut App, cli: &Cli) {
    if cli.bench_out.is_some() && !cli.headless {
        let floor = Duration::from_secs_f64(cli.bench_warmup.max(0.0));
        let ceiling = Duration::from_secs_f64(cli.bench_warm_ceiling.max(0.0));
        let recorder = match cli.bench_warm.as_str() {
            "idle" => BenchRecorder::until_settled(floor, ceiling),
            "hold" => BenchRecorder::holding(
                floor,
                ceiling,
                Duration::from_secs_f64(cli.bench_lead.max(0.0)),
            ),
            _ => BenchRecorder::new(floor),
        };
        app.insert_resource(recorder);
        if cli.bench_warm == "hold" {
            // The flight path and the scripted walkers read it; the
            // recorder lets it go.
            app.insert_resource(MotionHold::default());
        }
    }
    if let Some(time) = cli.time {
        app.insert_resource(DayStart(time));
    }
    if let Some(strength) = cli.wind {
        app.insert_resource(Wind {
            strength,
            ..default()
        });
    }
}

/// Where the loader bakes the rivers before the world shows (ADR 0010): round
/// the spawn (the settlement's places are its own), and for a scripted
/// flight as much farther as a minute and a quarter at 200 m/s goes.
fn prepare_water(cli: &Cli) -> strata::sim::PrepareWater {
    let flown = if cli.flight { 15_000.0 } else { 0.0 };
    strata::sim::PrepareWater {
        places: vec![cli.spawn.unwrap_or((0.0, 0.0))],
        radius_m: strata::sim::PREPARE_WATER_RADIUS_M + flown,
    }
}

/// Height of the flycam above the ground at spawn and on the flight.
const CAMERA_ALTITUDE: f64 = 120.0;

/// Height of the player's feet above the ground at spawn: a short drop, so
/// the first thing the character does is land.
const PLAYER_DROP: f64 = 2.0;

/// Where `--spawn` (or dry land near the origin) puts things: the dry
/// point and the ground height there.
fn spawn_point(
    cli: &Cli,
    source: &WorldSourceRes,
    settlements: Option<&strata::sim::Settlements>,
    survey: Option<&strata::sim::deposits::Survey>,
) -> (f64, f64, f64) {
    if let Some(class) = cli.script_survey.as_deref()
        && cli.spawn.is_none()
    {
        let from =
            combat::default_spawn(source, settlements).map_or((0.0, 0.0), |(x, _, z)| (x, z));
        if let Some((x, z)) = deposits::survey_start(class, survey, from) {
            let ground = f64::from(
                source
                    .source()
                    .sample(x, z)
                    .height
                    .max(source.source().sea_level()),
            );
            return (x, ground, z);
        }
        warn!("no deposit for --script-survey {class}; spawning by --spawn");
    }
    if let Some(spec) = at_deposit(cli) {
        // Measured from the default start, so the deposit a still or a
        // test names is the same one whatever else the run asks for.
        let from =
            combat::default_spawn(source, settlements).map_or((0.0, 0.0), |(x, _, z)| (x, z));
        if let Some(((x, z), _)) = deposits::start_at(spec, survey, from) {
            let ground = f64::from(
                source
                    .source()
                    .sample(x, z)
                    .height
                    .max(source.source().sea_level()),
            );
            return (x, ground, z);
        }
        warn!("no deposit for --at {spec}; spawning by --spawn");
    }
    if let Some(((x, z), _)) = at_bench(cli, settlements) {
        // On the square's own paving, which the sample knows: no search
        // for dry land, which would move the view off the props' fronts.
        let ground = f64::from(
            source
                .source()
                .sample(x, z)
                .height
                .max(source.source().sea_level()),
        );
        return (x, ground, z);
    }
    let at = cli
        .at
        .as_deref()
        .filter(|_| at_deposit(cli).is_none())
        .or(cli.route.then_some("road"));
    let named = at.and_then(|name| settlements?.place(name));
    if cli.combat
        && at.is_none()
        && cli.spawn.is_none()
        && let Some(point) = combat::default_spawn(source, settlements)
    {
        return point;
    }
    if at.is_some() && named.is_none() {
        warn!("no settlement place {:?}; spawning by --spawn", cli.at);
    }
    let (near_x, near_z) = named.or(cli.spawn).unwrap_or((0.0, 0.0));
    // A free camera goes where it is told, over the sea too: views of a
    // coast are taken from the water.
    if cli.flycam && named.is_none() && cli.spawn.is_some() {
        let ground = f64::from(source.source().sample(near_x, near_z).height)
            .max(f64::from(source.source().sea_level()));
        return (near_x, ground, near_z);
    }
    let (x, z) = find_land_near(source.source(), near_x, near_z, 5.0, 250.0, 30_000.0)
        .unwrap_or_else(|| {
            warn!("no land within 30 km of ({near_x}, {near_z}); spawning over the sea");
            (near_x, near_z)
        });
    let ground = f64::from(
        source
            .source()
            .sample(x, z)
            .height
            .max(source.source().sea_level()),
    );
    (x, ground, z)
}

/// The scripted walk of `--auto-walk` and the headless run: forward along
/// the initial heading, or standing facing it while `held` (a benchmark's
/// [`MotionHold`]; [`release_scripted_walk`] starts it).
fn scripted_walk(cli: &Cli, held: bool) -> ScriptedIntent {
    ScriptedIntent(PlayerIntent {
        move_dir: if held { Vec2::ZERO } else { Vec2::Y },
        yaw: cli.look.unwrap_or(0.0),
        jump: false,
        sprint: false,
    })
}

/// `--auto-walk` under a benchmark's [`MotionHold`]: the walk starts once the
/// hold is let go. The route steers itself and reads the hold there.
fn release_scripted_walk(
    cli: Res<Cli>,
    hold: Option<Res<MotionHold>>,
    mut walkers: Query<&mut ScriptedIntent, With<LocalPlayer>>,
    mut started: Local<bool>,
) {
    if *started || !cli.auto_walk || hold.is_none() || motion_held(hold.as_deref()) {
        return;
    }
    for mut intent in &mut walkers {
        intent.0.move_dir = Vec2::Y;
        *started = true;
    }
}

/// The local player, its feet [`PLAYER_DROP`] over the spawn point. The
/// primary observer: the streamer, the render origin and, in a window,
/// the camera follow it.
#[allow(
    clippy::too_many_arguments,
    reason = "the world, the flags and the household it is spawned from"
)]
fn spawn_player(
    commands: &mut Commands,
    cli: &Cli,
    source: &WorldSourceRes,
    settlements: Option<&strata::sim::Settlements>,
    registries: &strata::sim::Registries,
    household: Option<&household::Household>,
    survey: Option<&strata::sim::deposits::Survey>,
    scripted: bool,
    held: bool,
) {
    let (x, ground, z) = spawn_point(cli, source, settlements, survey);
    let controller = if cli.combat {
        let actor = &strata::sim::combat_tables::current().actor;
        CharacterController {
            responsive: true,
            height: 1.8,
            walk_speed: actor.walk_speed,
            sprint_speed: actor.sprint_speed,
            jump_speed: actor.jump_speed,
            gravity: actor.gravity,
            ..default()
        }
    } else {
        CharacterController::default()
    };
    let at = WorldPos::new(
        x,
        ground + PLAYER_DROP + f64::from(controller.half_height()),
        z,
    );
    info!("player at ({:.0}, {:.0}, {:.0})", at.x(), at.y(), at.z());
    if cli.combat {
        // The staging bench place stands relative to the default start;
        // the city's is the city's.
        let staging = (cli.at.is_none() && cli.spawn.is_none())
            .then(|| ((x, z), initial_heading(cli, settlements)));
        commands.insert_resource(household::sites(staging, settlements));
    }
    let mut player = commands.spawn((
        Player { id: PlayerId(1) },
        LocalPlayer,
        controller,
        WorldPosition(at),
        PlayerIntent {
            yaw: initial_heading(cli, settlements),
            ..default()
        },
    ));
    if cli.combat {
        // The household's active character: the sheet, the actor fitted
        // to it (what it may swing, fire and draw is what its boxes say),
        // the name on the HUD, the body's colour, and what they carry.
        let household = household.expect("the household is read before the player spawns");
        player.insert(household.stats(&registries.0));
        player.insert(household.bar());
    }
    if scripted && !cli.combat {
        player.insert(scripted_walk(cli, held));
    }
}

/// The headless player: what a connected player is to a server. Nothing
/// renders, but the streamer selects regions around it as it walks, the
/// physics builds the ground under it, and the terrain crate is absent, so
/// the sim, the loader, the streamer and the physics run the same path a
/// dedicated server would.
fn spawn_headless_player(
    mut commands: Commands,
    cli: Res<Cli>,
    source: Res<WorldSourceRes>,
    settlements: Option<Res<strata::sim::Settlements>>,
    registries: Res<strata::sim::Registries>,
    household: Option<Res<household::Household>>,
    survey: Option<Res<strata::sim::deposits::Survey>>,
) {
    spawn_player(
        &mut commands,
        &cli,
        &source,
        settlements.as_deref(),
        &registries,
        household.as_deref(),
        survey.as_deref(),
        true,
        false,
    );
}

/// A line every two seconds of the headless run: where the player is and
/// what is loaded, so a log shows the world streaming around a walker.
fn log_headless_progress(
    frames: Res<FrameCount>,
    players: Query<(&WorldPosition, &CharacterController), With<LocalPlayer>>,
    loaded: Res<LoadedRegions>,
    colliders: Res<TerrainColliderStats>,
) {
    if frames.0 == 0 || !frames.0.is_multiple_of(120) {
        return;
    }
    let Ok((position, controller)) = players.single() else {
        return;
    };
    info!(
        "frame {}: player ({:.1}, {:.1}, {:.1}) grounded {} | regions {} ({} pending) | colliders {} ({} in flight, {} built)",
        frames.0,
        position.0.x(),
        position.0.y(),
        position.0.z(),
        controller.grounded,
        loaded.len(),
        loaded.pending().count(),
        colliders.loaded,
        colliders.in_flight,
        colliders.built,
    );
}

/// The camera and, unless `--flycam` or `--flight`, the player it orbits.
///
/// Spawned once the world exists because the spawn point comes from the
/// world. No light and no ground here: the atmosphere plugin owns the sun
/// and the terrain streams in around the observer.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn spawn_camera(
    mut commands: Commands,
    cli: Res<Cli>,
    source: Res<WorldSourceRes>,
    settlements: Option<Res<strata::sim::Settlements>>,
    registries: Res<strata::sim::Registries>,
    household: Option<Res<household::Household>>,
    survey: Option<Res<strata::sim::deposits::Survey>>,
    hold: Option<Res<MotionHold>>,
) {
    // The streamer's horizon is 48 km and the sea plane reaches 200 km so
    // it meets the sky past the planet's horizon; the far plane only culls.
    let projection = Projection::Perspective(PerspectiveProjection {
        far: 300_000.0,
        ..default()
    });
    if cli.flycam || cli.flight {
        let (x, ground, z) = spawn_point(&cli, &source, settlements.as_deref(), survey.as_deref());
        let at = WorldPos::new(x, ground + cli.camera_altitude, z);
        info!("flycam at ({:.0}, {:.0}, {:.0})", at.x(), at.y(), at.z());
        let mut camera = commands.spawn((
            Camera3d::default(),
            projection,
            FlyCam::looking(cli.look.unwrap_or(0.0), cli.camera_pitch),
            WorldPosition(at),
        ));
        if cli.lock_camera {
            camera.remove::<FlyCam>().insert(Transform::from_rotation(
                FlyCam::looking(cli.look.unwrap_or(0.0), cli.camera_pitch).rotation(),
            ));
        }
        if cli.flight {
            camera.insert(FlightPath {
                altitude: cli.camera_altitude,
                heading: cli.flight_heading,
                ..default()
            });
        }
        let camera = camera.id();
        apply_aa(&mut commands, camera, &cli.aa);
        return;
    }
    // Connected, the server spawns the player and replication delivers
    // it; the camera waits for it.
    if cli.connect.is_none() {
        spawn_player(
            &mut commands,
            &cli,
            &source,
            settlements.as_deref(),
            &registries,
            household.as_deref(),
            survey.as_deref(),
            cli.auto_walk || cli.route,
            motion_held(hold.as_deref()),
        );
    }
    let camera = commands
        .spawn((
            Camera3d::default(),
            projection,
            OrbitCamera::looking(cli.look.unwrap_or(0.0)),
            Transform::default(),
        ))
        .id();
    if cli.combat {
        commands.entity(camera).insert((
            {
                let mut camera = strata::client::combat::CombatCamera::default();
                camera.hold = cli.orbit_distance;
                camera.yaw_offset = cli.orbit_yaw.unwrap_or(0.).to_radians();
                camera
            },
            OrbitCamera {
                pitch: cli.orbit_pitch.map_or(-0.16, f32::to_radians),
                yaw: initial_heading(&cli, settlements.as_deref()),
                ..default()
            },
        ));
    }
    apply_aa(&mut commands, camera, &cli.aa);
}

/// Explicit AA choices remain in force when the live preset changes.
fn apply_aa(commands: &mut Commands, camera: Entity, aa: &str) {
    let choice = match aa {
        "msaa4" => AaOverride::Msaa4,
        "off" => AaOverride::Off,
        "taa" => AaOverride::Taa,
        _ => return,
    };
    commands.entity(camera).insert(choice);
}

fn open_render_for_shot(mut commands: Commands, cli: Res<Cli>, frame: Res<FrameCount>) {
    if cli.open_render_panel && frame.0 == 120 {
        strata::client::ui::render::open(&mut commands);
    }
}

/// How far a scripted walker must get in a second before it counts as
/// stopped, metres: a fifth of the slowest walk.
const STALLED_M: f64 = 1.0;

/// Which kind of body a stalled walker met, by the component that names it.
fn body_kind(
    (ground, tree, building, name): (
        Option<&strata::physics::TerrainCollider>,
        Option<&strata::physics::TreeCollider>,
        Option<&strata::physics::settlement::BuildingCollider>,
        Option<&Name>,
    ),
) -> String {
    if let Some(ground) = ground {
        format!("the ground collider of region {:?}", ground.coord)
    } else if let Some(tree) = tree {
        format!(
            "tree {:?} of species {:?} in {:?}",
            tree.id, tree.species, tree.cell
        )
    } else if let Some(building) = building {
        format!("building {} of {}", building.file, building.settlement)
    } else {
        name.map_or_else(|| "an unnamed body".to_owned(), |name| format!("{name}"))
    }
}

/// A scripted walker that has stopped says what it walks into, once per
/// stop: the body its capsule meets a metre ahead, which kind of collider
/// that is, and the ground under it. A benchmark whose walker stands still
/// measures a static view under a moving fixture's name, and the log is
/// where that is noticed and explained.
#[allow(
    clippy::type_complexity,
    reason = "one query of the walker's state and one of the kinds a hit can be"
)]
fn report_stalled_walker(
    time: Res<Time<Real>>,
    spatial: strata::physics::avian3d::prelude::SpatialQuery,
    walkers: Query<
        (
            Entity,
            &WorldPosition,
            &CharacterController,
            &ScriptedIntent,
            &strata::physics::avian3d::prelude::Position,
            &strata::physics::avian3d::prelude::Collider,
        ),
        With<LocalPlayer>,
    >,
    kinds: Query<(
        Option<&strata::physics::TerrainCollider>,
        Option<&strata::physics::TreeCollider>,
        Option<&strata::physics::settlement::BuildingCollider>,
        Option<&Name>,
    )>,
    mut last: Local<Option<(f64, WorldPos)>>,
    mut reported: Local<bool>,
) {
    use strata::physics::avian3d::prelude::{ShapeCastConfig, SpatialQueryFilter};
    let Ok((entity, at, controller, intent, position, collider)) = walkers.single() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let Some((then, before)) = *last else {
        *last = Some((now, at.0));
        return;
    };
    if now - then < 1.0 {
        return;
    }
    *last = Some((now, at.0));
    let moved = (at.0 - before).with_y(0.0).length();
    if intent.0.move_dir == Vec2::ZERO || moved >= STALLED_M {
        *reported = false;
        return;
    }
    if *reported {
        return;
    }
    *reported = true;
    let filter = SpatialQueryFilter::from_excluded_entities([entity]);
    let forward = intent.0.world_direction();
    let ahead = Dir3::new(forward).ok().and_then(|direction| {
        spatial.cast_shape(
            collider,
            position.0,
            Quat::IDENTITY,
            direction,
            &ShapeCastConfig::from_max_distance(1.0),
            &filter,
        )
    });
    let what = ahead.map_or_else(
        || "nothing within a metre ahead".to_owned(),
        |hit| {
            let kind = kinds
                .get(hit.entity)
                .map_or_else(|_| "an unknown entity".to_owned(), body_kind);
            format!(
                "{kind} ({:?}) {:.2} m ahead, its normal ({:.2}, {:.2}, {:.2}) ({:.0} degrees from up)",
                hit.entity,
                hit.distance,
                hit.normal1.x,
                hit.normal1.y,
                hit.normal1.z,
                hit.normal1.y.clamp(-1.0, 1.0).acos().to_degrees(),
            )
        },
    );
    // The ground half a metre past the capsule's front, measured from
    // above: a step taller than the controller's step height is a wall to
    // it, whatever the slope under the feet.
    let feet = position.0.y - controller.half_height();
    let probe = position.0 + forward * (controller.radius + 0.5) + Vec3::Y * 20.0;
    let step = spatial
        .cast_ray(probe, Dir3::NEG_Y, 60.0, true, &filter)
        .map_or_else(
            || "no ground".to_owned(),
            |hit| format!("{:+.2} m", probe.y - hit.distance - feet),
        );
    let n = controller.ground_normal;
    warn!(
        "scripted walker stopped at ({:.1}, {:.1}, {:.1}): {moved:.2} m in the last second, grounded {}, ground normal ({:.2}, {:.2}, {:.2}) ({:.0} degrees), the ground just ahead {step} from the feet (steps up to {:.2} m), walking at yaw {:.3} into {what}",
        at.0.x(),
        at.0.y(),
        at.0.z(),
        controller.grounded,
        n.x,
        n.y,
        n.z,
        n.y.clamp(-1.0, 1.0).acos().to_degrees(),
        controller.step_height,
        intent.0.yaw,
    );
}

/// `--connect --auto-walk`: the scripted walk goes on the player once the
/// server has handed it to us, since it was not spawned here.
#[cfg(feature = "net")]
fn script_connected_player(
    mut commands: Commands,
    cli: Res<Cli>,
    players: Query<Entity, (Added<LocalPlayer>, Without<ScriptedIntent>)>,
) {
    if !cli.auto_walk {
        return;
    }
    for player in &players {
        commands.entity(player).insert(scripted_walk(&cli, false));
    }
}

/// The frame `--open-pause` pushes the pause screen on: late enough for
/// the terrain around the spawn to be drawn under it.
pub(crate) const OPEN_PAUSE_FRAME: u32 = 120;

/// `--open-pause`: the pause screen over the world, once, as Escape would.
fn open_pause_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    mut commands: Commands,
    mut opened: Local<bool>,
) {
    if !cli.open_pause || *opened || frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    *opened = true;
    strata::client::ui::open_pause(&mut commands);
}

/// `--press`: one `place@frame`.
fn parse_press(text: &str) -> Result<(usize, u32), String> {
    let (place, frame) = text.split_once('@').ok_or("want place@frame, as 0@200")?;
    Ok((
        place.parse().map_err(|error| format!("place: {error}"))?,
        frame.parse().map_err(|error| format!("frame: {error}"))?,
    ))
}

/// `--press`: the places asked for are pressed on their frames, written
/// where the keys write them, so the sim's rule answers a press as it
/// answers a hand's.
fn press_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    for (place, _) in cli.press.iter().filter(|(_, frame)| *frame == frames.0) {
        for mut input in &mut players {
            input.ability = Some(*place);
        }
    }
}

/// `--hold-fire`: the player's trigger is down for the given seconds from
/// frame 120, written where the mouse button would write it.
fn hold_fire_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut players: Query<
        (
            &mut strata::sim::combat::CombatIntent,
            &strata::sim::WorldPosition,
        ),
        With<LocalPlayer>,
    >,
    hostiles: Query<
        (&strata::sim::WorldPosition, &strata::sim::combat::Combatant),
        Without<LocalPlayer>,
    >,
) {
    let Some(seconds) = cli.hold_fire else {
        return;
    };
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    for (mut input, at) in &mut players {
        // A window with no focus gives the read of the mouse no aim to keep,
        // so a still's shots go at the nearest hostile's chest.
        if let Some((target, _)) = hostiles
            .iter()
            .filter(|(_, actor)| actor.health > 0.)
            .min_by(|(a, _), (b, _)| a.0.distance(at.0).total_cmp(&b.0.distance(at.0)))
        {
            input.aim_point = WorldPos::new(target.0.x(), target.0.y() + 0.2, target.0.z());
        }
        input.fire = true;
    }
}

/// `--hold-aim`: the player's aim button is down for the given seconds from
/// frame 120, written where the right mouse button would write it.
fn hold_aim_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    let Some(seconds) = cli.hold_aim else {
        return;
    };
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    for mut input in &mut players {
        input.aim = true;
    }
}

/// Whether `--hold-guard` is holding the button this frame, for the scripted
/// fight to read instead of its own last output.
#[derive(Resource, Default)]
struct GuardHold(bool);

/// `--hold-guard`: the player's aim button is down for the given seconds
/// from frame 120, which with a melee weapon in hand is the guard.
fn hold_guard_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    time: Res<Time>,
    mut began: Local<Option<f32>>,
    mut hold: ResMut<GuardHold>,
    mut players: Query<&mut strata::sim::combat::CombatIntent, With<LocalPlayer>>,
) {
    let Some(seconds) = cli.hold_guard else {
        return;
    };
    hold.0 = false;
    if frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    let now = time.elapsed_secs();
    let start = *began.get_or_insert(now);
    if now - start > seconds {
        return;
    }
    hold.0 = true;
    for mut input in &mut players {
        input.aim = true;
    }
}

/// `--track`: the nearest deposit of the type to the default start is
/// tracked, once the survey is there to ask. Asked of the sim as the
/// register's row asks, so what is tracked is the sim's and is saved with
/// the household like any other.
fn track_for_shot(
    cli: Res<Cli>,
    survey: Option<Res<strata::sim::deposits::Survey>>,
    source: Option<Res<WorldSourceRes>>,
    settlements: Option<Res<strata::sim::Settlements>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut asked: Local<bool>,
) {
    let (Some(wanted), Some(survey), Some(source), false) =
        (cli.track.as_deref(), survey, source, *asked)
    else {
        return;
    };
    *asked = true;
    let from = combat::default_spawn(&source, settlements.as_deref())
        .map_or((0.0, 0.0), |(x, _, z)| (x, z));
    let Some(deposit) = deposits::nearest_of(&survey, wanted, from) else {
        warn!("--track {wanted}: no such deposit near the default start");
        return;
    };
    info!(
        "deposits: tracking {} at ({:.0}, {:.0}), {:.0} m from the default start",
        deposit.place,
        deposit.at[0],
        deposit.at[1],
        deposit.distance_m(from.0, from.1)
    );
    inbox
        .0
        .push(strata::sim::deposits::SurveyRequest::Track(Some(
            deposit.id,
        )));
}

/// The frame `--ping` waits for before it asks: the player is spawned and
/// stands in the world.
const PING_FRAME: u32 = 70;

/// `--ping <class>`: the ping the survey screen's button would send, once
/// the player is in the world. The class is an id, taken as the `strata`
/// namespace's when it names none. Sent once, asked of the sim like any
/// other, so the channel plays and the household learns what it meets.
fn ping_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    players: Query<(), With<LocalPlayer>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut answers: MessageReader<strata::sim::deposits::PingAnswer>,
    mut asked: Local<bool>,
) {
    for answer in answers.read() {
        match &answer.outcome {
            Ok(strata::sim::deposits::PingDone::Met(result)) => info!(
                "--ping: {} deposits met, the strongest {}",
                result.found.len(),
                result.found.first().map_or(0, |met| met.pm)
            ),
            Ok(strata::sim::deposits::PingDone::Started) => {}
            Err(refusal) => warn!("--ping: {refusal}"),
        }
    }
    let Some(class) = cli.ping.as_deref() else {
        return;
    };
    if *asked || frames.0 < PING_FRAME || players.is_empty() {
        return;
    }
    *asked = true;
    let full = if class.contains(':') {
        class.to_owned()
    } else {
        format!("strata:{class}")
    };
    if let Ok(class) = full.parse() {
        inbox
            .0
            .push(strata::sim::deposits::SurveyRequest::Ping { class });
    } else {
        warn!("--ping {class}: not a class id");
    }
}

/// `--sampling`: Sample is asked once, when the ping `--ping` asked for has
/// been answered (or, with no `--ping`, once the player stands in the
/// world), from where the player stands; the sim says whether there is
/// anything found to sample here.
fn sampling_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    last: Res<strata::sim::deposits::LastPing>,
    players: Query<(), With<LocalPlayer>>,
    mut inbox: ResMut<strata::sim::deposits::SurveyInbox>,
    mut answers: MessageReader<strata::sim::deposits::SampleAnswer>,
    mut asked: Local<bool>,
) {
    for answer in answers.read() {
        if let Err(refusal) = answer.outcome {
            warn!("--sampling: {refusal}");
        }
    }
    if !cli.sampling || *asked || players.is_empty() {
        return;
    }
    let ready = if cli.ping.is_some() {
        last.0.is_some()
    } else {
        frames.0 >= PING_FRAME
    };
    if ready {
        *asked = true;
        inbox.0.push(strata::sim::deposits::SurveyRequest::Sample);
    }
}

/// `--rework-first`: the first hand's weapon is reworked once, by the request
/// the Rework button sends, and the sim's answer is said in the log.
fn rework_first_for_shot(
    frames: Res<FrameCount>,
    mut inbox: ResMut<strata::sim::rework::ReworkInbox>,
    mut answers: MessageReader<strata::sim::rework::ReworkAnswer>,
    mut asked: Local<bool>,
) {
    use strata::sim::rework::ReworkRequest;
    for answer in answers.read() {
        match &answer.result {
            Ok(done) => info!(
                "--rework-first: {:?}, moddability {} to {}",
                done.outcome, done.from, done.to
            ),
            Err(refusal) => warn!("--rework-first: {refusal}"),
        }
    }
    if !*asked && frames.0 >= CRAFT_FIRST_FRAME {
        inbox.0.push(ReworkRequest::Rework { hand: 0 });
        *asked = true;
    }
}

/// `--craft-first`: where the making stands.
#[derive(Resource, Debug, Default)]
struct CraftFirst {
    /// A request is with the sim.
    asked: bool,
    /// The thing is made, and recorded when `--record-first` asks for it,
    /// or cannot be.
    done: bool,
}

/// The frame `--craft-first` starts on: the world is up and the household
/// has handed its stock to the sim.
const CRAFT_FIRST_FRAME: u32 = 40;

/// `--craft-first`: one of the named schematic is made before the still's
/// screen opens, by the requests a screen would send and the sim's own
/// choice of stock: Assemble from the best stock, each point steady on the
/// first line with room, Finish, and with `--record-first` Record. Each
/// request waits for its answer; one the sim refuses ends it, said in the
/// log.
#[allow(
    clippy::too_many_arguments,
    reason = "the flag, the clock, the stock the sim would pick from and the two ends of a request"
)]
fn craft_first_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    registries: Option<Res<strata::sim::Registries>>,
    craft: Res<strata::sim::crafting::CraftState>,
    store: Res<strata::sim::items::HouseholdStore>,
    players: Query<&strata::sim::items::Pack, With<strata::sim::LocalPlayer>>,
    mut inbox: ResMut<strata::sim::crafting::CraftInbox>,
    mut answers: MessageReader<strata::sim::crafting::CraftAnswer>,
    mut first: ResMut<CraftFirst>,
) {
    use strata::registry::schematic::Hand;
    use strata::sim::crafting::{self, CraftDone, CraftRequest};
    for answer in answers.read() {
        first.asked = false;
        match &answer.outcome {
            Ok(CraftDone::Finished { product }) => {
                info!(
                    "--craft-first: made {} batch {}",
                    product.item, product.batch
                );
                // `--record-first`: the thing just made is recorded by
                // the request the Record box sends, and that is waited on.
                if let Some(percent) = cli.record_first {
                    inbox.0.push(CraftRequest::Record {
                        batch: product.batch,
                        derate: percent.saturating_mul(10),
                    });
                    first.asked = true;
                } else {
                    first.done = true;
                }
            }
            Ok(CraftDone::Recorded {
                blueprint, runs, ..
            }) => {
                info!("--record-first: blueprint {blueprint}, {runs} runs");
                first.done = true;
            }
            Ok(_) => {}
            Err(refusal) => {
                warn!("--craft-first: {refusal}");
                first.done = true;
            }
        }
    }
    if first.done || first.asked || frames.0 < CRAFT_FIRST_FRAME {
        return;
    }
    let (Some(name), Some(registries), Ok(pack)) =
        (cli.craft_first.as_deref(), registries, players.single())
    else {
        return;
    };
    let registries = &registries.0;
    let request = match &craft.session {
        None => {
            let schematic = strata::client::ui::crafting::schematic_named(registries, name)
                .and_then(|id| registries.schematics.get(&id));
            let Some(schematic) = schematic else {
                warn!("--craft-first {name}: no such schematic");
                first.done = true;
                return;
            };
            CraftRequest::Assemble {
                schematic: schematic.def.id.clone(),
                picks: crafting::best_stock(registries, pack, Some(&store), schematic),
            }
        }
        Some(session) => session
            .lines
            .iter()
            .find(|line| session.points > 0 && !line.at_ceiling())
            .map_or(CraftRequest::Finish, |line| CraftRequest::Roll {
                line: line.line.clone(),
                hand: Hand::Steady,
            }),
    };
    inbox.0.push(request);
    first.asked = true;
}

/// `--open-sheet`, `--open-bench`, `--open-stores`, `--open-inventory`,
/// `--open-survey`, `--open-register`, `--open-fitting`, `--open-crafting`
/// and `--open-factory` without the script: the screen over the world on
/// the same frame as the pause would be, or once `--craft-first` has made
/// its thing and `--factory-run` has started its run (or, with
/// `--factory-finish`, finished it) and `--factory-poor-lot` has stocked
/// its lot. With the script the
/// script opens the sheet, the bench, the fitting and the crafting where
/// they belong, and has no stop for the rest.
#[allow(
    clippy::too_many_arguments,
    reason = "the flag, the clock, the four things a still waits on, and the reach a bench opens by"
)]
fn open_screens_for_shot(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    first: Option<Res<CraftFirst>>,
    run: Option<Res<household::FactoryShot>>,
    lot: Option<Res<household::PoorLotShot>>,
    joined: Option<Res<household::JoinShot>>,
    board: Option<Res<household::BoardShot>>,
    reach: Option<Res<strata::sim::progression::Reach>>,
    mut commands: Commands,
    mut opened: Local<bool>,
) {
    if cli.script_progression || *opened || frames.0 < OPEN_PAUSE_FRAME {
        return;
    }
    if first.is_some_and(|first| !first.done)
        || run.is_some_and(|run| !run.done)
        || lot.is_some_and(|lot| !lot.done)
        || joined.is_some_and(|joined| !joined.done)
        || board.is_some_and(|board| !board.stocked)
    {
        return;
    }
    *opened = true;
    if cli.open_survey {
        strata::client::ui::survey::open(&mut commands);
    }
    if let Some(scale) = &cli.open_map {
        if let Some(scale) = strata::client::ui::map::Scale::parse(scale) {
            strata::client::ui::map::set_holders(&mut commands, cli.map_holders);
            strata::client::ui::map::open_at(&mut commands, Some(scale));
        } else {
            warn!("--open-map takes near, region or far, not {scale}");
        }
    }
    if cli.open_register {
        strata::client::ui::bench::open_on(
            &mut commands,
            false,
            strata::client::ui::bench::BenchTab::Register,
        );
    }
    if let Some(filter) = cli.stores_filter {
        strata::client::ui::stores::show_nth(&mut commands, filter, cli.stores_pin);
    }
    if cli.open_sheet {
        // On the tree of the trainer stood before, as their F opens it.
        let tree = reach.as_ref().and_then(|reach| match &reach.nearest {
            Some((_, strata::sim::progression::InteractionKind::Trainer { trees })) => {
                trees.first().cloned()
            }
            _ => None,
        });
        strata::client::ui::sheet::open(&mut commands, tree);
    }
    if cli.open_loadout {
        strata::client::ui::sheet::open_loadout(&mut commands);
    }
    if cli.open_bench {
        // Live where a terminal's F would open it (`--at terminal` stands
        // at one), a reading anywhere else.
        let at_terminal = reach.is_some_and(|reach| {
            matches!(
                reach.nearest,
                Some((_, strata::sim::progression::InteractionKind::Terminal))
            )
        });
        strata::client::ui::bench::open(&mut commands, at_terminal);
    }
    if cli.open_stores {
        strata::client::ui::stores::open(&mut commands, true);
    } else if cli.open_inventory {
        strata::client::ui::stores::open(&mut commands, false);
    }
    if let Some(mount) = &cli.open_fitting {
        strata::client::ui::fitting::open_at(&mut commands, cli.fitting_hand, Some(mount.clone()));
        if cli.fitting_pick {
            strata::client::ui::fitting::pick_first(&mut commands);
        }
    }
    if let Some(filter) = cli.crafting_filter {
        strata::client::ui::crafting::set_filter(&mut commands, filter.filter());
    }
    if let Some(schematic) = &cli.open_crafting {
        strata::client::ui::crafting::open_for_still(
            &mut commands,
            schematic,
            cli.crafting_still(),
        );
    }
    if cli.open_factory {
        strata::client::ui::factory::open(&mut commands);
    }
    if cli.open_board {
        strata::client::ui::board::open(&mut commands, household::pelagic_works());
    }
}

/// Screenshot bookkeeping: how many were requested and how many are on
/// disk.
#[derive(Resource, Default)]
struct Shots {
    requested: u32,
    saved: u32,
}

/// The frame a screenshot is taken on when nothing says otherwise.
const DEFAULT_SHOT_FRAME: u32 = 60;

impl Cli {
    /// The frames `--screenshot` captures on.
    fn shot_frames(&self) -> Vec<u32> {
        if !self.shot_frames.is_empty() {
            return self.shot_frames.clone();
        }
        vec![self.frames.unwrap_or(DEFAULT_SHOT_FRAME)]
    }

    /// The screenshot path for `frame`, with `{frame}` substituted.
    fn shot_path(&self, frame: u32) -> Option<PathBuf> {
        let pattern = self.screenshot.as_ref()?.to_string_lossy().into_owned();
        Some(PathBuf::from(
            pattern.replace("{frame}", &frame.to_string()),
        ))
    }
}

/// `--screenshot`: capture on each requested frame.
///
/// The capture is asynchronous (a GPU readback), so completion is counted
/// by the observer rather than assumed a fixed number of frames later.
/// `save_to_disk` writes synchronously inside the observer, so when the
/// count goes up the file exists.
fn take_screenshots(
    mut commands: Commands,
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    mut shots: ResMut<Shots>,
) {
    if cli.screenshot.is_none() {
        return;
    }
    let due = cli
        .shot_frames()
        .into_iter()
        .filter(|f| *f <= frames.0)
        .count();
    // Frames are counted, not summed; the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    let due = due as u32;
    while shots.requested < due {
        let frame = cli.shot_frames()[shots.requested as usize];
        shots.requested += 1;
        let Some(path) = cli.shot_path(frame) else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        info!("capturing frame {} to {}", frames.0, path.display());
        let mut save = save_to_disk(path);
        commands.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>, mut shots: ResMut<Shots>| {
                save(captured);
                shots.saved += 1;
            },
        );
    }
}

/// Exits once everything that was asked for has happened: `--frames` have
/// run, every screenshot is saved, and a scripted flight has lasted
/// `--flight-secs`. With none of those asked for, runs forever.
#[cfg_attr(
    target_arch = "wasm32",
    allow(unused_variables, reason = "only the native bench summary reads them")
)]
fn exit_when_done(
    cli: Res<Cli>,
    frames: Res<FrameCount>,
    shots: Res<Shots>,
    flights: Query<&FlightPath>,
    script: Option<Res<household::ProgressionScript>>,
    // What the benchmark summary reads when the run ends, and the scripted
    // ride's report.
    (loaded, bench, entities, clock, ride, survey): (
        Option<Res<LoadedRegions>>,
        Option<Res<BenchRecorder>>,
        Query<Entity>,
        Option<Res<strata::client::stats::WorkClock>>,
        Option<Res<ride::ScriptRide>>,
        Option<Res<survey_script::ScriptSurvey>>,
    ),
    mut exit: MessageWriter<AppExit>,
) {
    let mut asked = false;
    let mut done = true;
    if let Some(limit) = cli.frames {
        asked = true;
        done &= frames.0 >= limit;
    }
    if cli.screenshot.is_some() {
        asked = true;
        // Frames are counted, not summed; the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let wanted = cli.shot_frames().len() as u32;
        done &= shots.saved >= wanted;
    }
    if cli.flight && cli.frames.is_none() {
        asked = true;
        done &= flights.iter().any(|f| f.elapsed >= cli.flight_secs);
    }
    if cli.script_progression && cli.frames.is_none() {
        asked = true;
        done &= script.is_some_and(|s| s.done());
    }
    if cli.script_ride && cli.frames.is_none() {
        asked = true;
        done &= ride.is_some_and(|ride| ride.done());
    }
    if cli.script_survey.is_some() && cli.frames.is_none() {
        asked = true;
        done &= survey.is_some_and(|survey| survey.done());
    }
    if let Some(wanted) = cli.bench_frames {
        asked = true;
        done &= bench
            .as_deref()
            .is_some_and(|b| b.measured_frames() >= wanted);
    }
    if let Some(wanted) = cli.bench_secs {
        asked = true;
        done &= bench
            .as_deref()
            .is_some_and(|b| b.measured_secs() >= wanted);
    }
    if asked && done {
        let regions = loaded.map_or(0, |l| l.len());
        info!(
            "done after {} frames with {regions} regions loaded, exiting",
            frames.0
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(path), Some(bench)) = (&cli.bench_out, bench)
            && let Err(error) = write_bench(
                path,
                &bench,
                clock.as_deref(),
                regions,
                entities.iter().count(),
            )
        {
            error!(
                "writing the benchmark summary to {}: {error}",
                path.display()
            );
            exit.write(AppExit::error());
            return;
        }
        exit.write(AppExit::Success);
    }
}

/// The benchmark summary as a flat JSON object, for `tools/bench.py`.
#[cfg(not(target_arch = "wasm32"))]
fn write_bench(
    path: &std::path::Path,
    bench: &BenchRecorder,
    clock: Option<&strata::client::stats::WorkClock>,
    regions: usize,
    entities: usize,
) -> std::io::Result<()> {
    let summary = bench.summary_with(clock).ok_or_else(|| {
        std::io::Error::other("no frame was measured; the run ended inside its warm-up")
    })?;
    let mut fields = vec![
        ("frames", summary.frames.to_string()),
        ("mean_ms", format!("{:.3}", summary.mean_ms)),
        ("p50_ms", format!("{:.3}", summary.p50_ms)),
        ("p95_ms", format!("{:.3}", summary.p95_ms)),
        ("p99_ms", format!("{:.3}", summary.p99_ms)),
        ("worst_ms", format!("{:.3}", summary.worst_ms)),
        ("over_33ms", summary.over_33ms.to_string()),
        ("gpu_mean_ms", format!("{:.3}", summary.gpu_mean_ms)),
        ("main_p50_ms", format!("{:.3}", summary.main_ms[0])),
        ("main_p95_ms", format!("{:.3}", summary.main_ms[1])),
        ("main_p99_ms", format!("{:.3}", summary.main_ms[2])),
        ("main_max_ms", format!("{:.3}", summary.main_ms[4])),
        ("render_p50_ms", format!("{:.3}", summary.render_ms[0])),
        ("render_p95_ms", format!("{:.3}", summary.render_ms[1])),
        ("render_p99_ms", format!("{:.3}", summary.render_ms[2])),
        ("render_max_ms", format!("{:.3}", summary.render_ms[4])),
        ("regions", regions.to_string()),
        ("entities", entities.to_string()),
        ("warmup_s", format!("{:.2}", summary.warmup_s)),
        ("warm_up", format!("\"{}\"", summary.warm_up.name())),
    ];
    if let Some(held) = summary.held_s {
        fields.push(("held_s", format!("{held:.2}")));
    }
    // The terrain and collider streamers over the whole measured stretch:
    // rates from totals, not medians of the log's two-second intervals.
    let s = &summary.stream;
    fields.extend([
        (
            "regions_pending_mean",
            format!("{:.2}", s.regions_pending_mean),
        ),
        ("regions_pending_max", s.regions_pending_max.to_string()),
        ("terrain_tasks_mean", format!("{:.2}", s.terrain_tasks_mean)),
        ("terrain_tasks_max", s.terrain_tasks_max.to_string()),
        (
            "terrain_uploads_per_frame",
            format!("{:.3}", s.uploads_per_frame),
        ),
        ("terrain_uploads_max_frame", s.uploads_max_frame.to_string()),
        ("terrain_uploads_per_s", format!("{:.2}", s.uploads_per_s)),
        (
            "terrain_cancelled_per_s",
            format!("{:.2}", s.cancelled_per_s),
        ),
        ("heightfields_per_s", format!("{:.2}", s.heightfields_per_s)),
        (
            "colliders_added_per_s",
            format!("{:.2}", s.colliders_added_per_s),
        ),
        (
            "colliders_removed_per_s",
            format!("{:.2}", s.colliders_removed_per_s),
        ),
    ]);
    // How far the observer went, which `tools/bench.py` holds a moving
    // fixture to.
    if let Some(travel) = summary.travel {
        fields.push(("travel_m", format!("{:.1}", travel.metres)));
        fields.push(("speed_mps", format!("{:.3}", travel.speed_mps)));
        if let Some(slowest) = travel.slowest_span_m {
            fields.push(("slowest_5s_m", format!("{slowest:.2}")));
        }
    }
    // The cost line's medians over the measured stretch, when one was
    // written after the warm-up.
    if let Some(cost) = summary.cost {
        for (key, value) in strata::client::stats::COST_KEYS.iter().zip(cost) {
            fields.push((*key, format!("{value:.3}")));
        }
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("\"{key}\": {value}"))
        .collect();
    let json = format!("{{{}}}\n", body.join(", "));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

#[cfg(feature = "net")]
/// The adapter as a client of `server`, with the versions of the tree
/// library and the site groups under `--generated` and of the world the
/// assets under `--assets` build, the `--seed` and the installed combat
/// tables, folded into its protocol hash, so a server on other trees, other
/// ground, other numbers or other site buildings refuses this client and
/// says which. `None`
/// having said why a version cannot be read (most often: run `just
/// content`).
fn client_replication(server: std::net::SocketAddr, cli: &Cli) -> Option<ReplicationPlugin> {
    let files = Files::installed();
    let library = strata_flora::TreeLibrary::read_from(&files.under("generated"))
        .map(|library| library.version)
        .map_err(|error| eprintln!("{error}"))
        .ok()?;
    let world = strata::sim::world_version_from(&files)
        .map_err(|error| eprintln!("{error}"))
        .ok()?;
    let site_groups = strata::sim::sites::site_groups_version_from(&files)
        .map_err(|error| eprintln!("{error}"))
        .ok()?;
    Some(
        ReplicationPlugin::client(server)
            .with_tree_library(library)
            .with_world_version(world)
            .with_seed(cli.seed)
            .with_combat_tables(strata::sim::combat_tables::current().version())
            .with_site_groups(site_groups),
    )
}

/// The scripted scenarios a run may carry: the slice's route and the
/// grounding study's fixtures.
fn install_scenarios(app: &mut App, cli: &Cli, prepared: Option<grounding::Prepared>) {
    if cli.route {
        route::install(app);
    }
    if let Some(prepared) = prepared {
        grounding::install(app, prepared);
    }
}

#[cfg(test)]
mod fog_flag_tests {
    use super::*;

    #[test]
    fn the_fog_flags_parse_and_default_to_the_fixed_weather() {
        let fog = ground_fog(&Cli::parse_from(["game"]));
        assert_eq!(fog, GroundFog::default());
        assert!((fog.scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(fog.weather, GroundFog::WEATHER);
        let fog = ground_fog(&Cli::parse_from([
            "game",
            "--fog",
            "0.5",
            "--weather",
            "0.25, 1",
            "--fog-steps",
            "12",
        ]));
        assert!((fog.scale - 0.5).abs() < f32::EPSILON);
        assert!((fog.weather.fog - 0.25).abs() < f32::EPSILON);
        assert!((fog.weather.sea_fog - 1.0).abs() < f32::EPSILON);
        assert_eq!(fog.steps, 12);
        let off = ground_fog(&Cli::parse_from(["game", "--fog", "0"]));
        assert!(off.scale.abs() < f32::EPSILON);
    }

    #[test]
    fn bad_fog_flags_are_refused() {
        for args in [
            &["game", "--fog", "-1"][..],
            &["game", "--fog", "NaN"],
            &["game", "--fog", "much"],
            &["game", "--weather", "1"],
            &["game", "--weather", "1,2"],
            &["game", "--weather", "-0.5,0"],
            &["game", "--weather", "a,b"],
            &["game", "--fog-steps", "0"],
            &["game", "--fog-spike", "pass"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }

    /// `--open-fitting` opens on the feed unless a mount is named, its two
    /// companions mean nothing without it, and the preview screen's flag
    /// went with the screen.
    #[test]
    fn the_fitting_flags_name_a_mount_and_a_hand() {
        assert_eq!(Cli::parse_from(["game"]).open_fitting, None);
        let cli = Cli::parse_from(["game", "--open-fitting"]);
        assert_eq!(cli.open_fitting.as_deref(), Some("feed"));
        assert_eq!((cli.fitting_hand, cli.fitting_pick), (None, false));
        let cli = Cli::parse_from([
            "game",
            "--open-fitting",
            "muzzle",
            "--fitting-hand",
            "1",
            "--fitting-pick",
            "--fitted",
            "veyra:module.patrol_extended_feed",
        ]);
        assert_eq!(cli.open_fitting.as_deref(), Some("muzzle"));
        assert_eq!((cli.fitting_hand, cli.fitting_pick), (Some(1), true));
        assert_eq!(cli.fitted, ["veyra:module.patrol_extended_feed"]);
        for args in [
            &["game", "--fitting-hand", "1"][..],
            &["game", "--fitting-pick"],
            &["game", "--open-weapon-diagram"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }

    /// `--open-crafting` opens on the cell core unless a schematic is named
    /// and on the picking state unless another is, and the state means
    /// nothing without the screen.
    #[test]
    fn the_crafting_flags_parse() {
        let cli = Cli::parse_from(["game"]);
        assert_eq!(cli.open_crafting, None);
        assert_eq!(cli.crafting_state, CraftingShot::Pick);
        let cli = Cli::parse_from(["game", "--open-crafting"]);
        assert_eq!(cli.open_crafting.as_deref(), Some("cell_core"));
        assert_eq!(cli.crafting_state, CraftingShot::Pick);
        let cli = Cli::parse_from([
            "game",
            "--open-crafting",
            "patrol_extended_feed",
            "--crafting-state",
            "hand",
        ]);
        assert_eq!(cli.open_crafting.as_deref(), Some("patrol_extended_feed"));
        assert_eq!(cli.crafting_state, CraftingShot::Hand);
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "done"]);
        assert_eq!(cli.open_crafting.as_deref(), Some("cell_core"));
        assert_eq!(cli.crafting_state, CraftingShot::Done);
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "empty"]);
        assert_eq!(cli.crafting_state, CraftingShot::Empty);
        // A thing made first, for a still of what takes it or of its card.
        let cli = Cli::parse_from([
            "game",
            "--craft-first",
            "cell_core",
            "--open-crafting",
            "patrol_extended_feed",
        ]);
        assert_eq!(cli.craft_first.as_deref(), Some("cell_core"));
        assert_eq!(cli.open_crafting.as_deref(), Some("patrol_extended_feed"));
        let cli = Cli::parse_from([
            "game",
            "--craft-first",
            "cell_core",
            "--open-stores",
            "--stores-filter",
            "parts",
        ]);
        assert!(cli.open_stores && cli.craft_first.is_some());
        assert!(
            Cli::try_parse_from(["game", "--script-crafting", "--craft-first", "cell_core"])
                .is_err()
        );
        // A still that spends stock is refused on the household the game
        // is played from, and taken on one of its own; a still that only
        // looks is taken anywhere.
        let refused = |args: &[&str]| Cli::parse_from(args).spends_the_played_household();
        for spends in [
            &["game", "--open-crafting", "--crafting-state", "hand"][..],
            &["game", "--no-hostiles", "--hold-fire", "2"],
            &["game", "--no-hostiles", "--hold-guard", "2"],
            &["game", "--no-hostiles", "--hold-aim", "2"],
            &["game", "--open-crafting", "--crafting-state", "done"],
            &["game", "--craft-first", "cell_core", "--open-stores"],
            &["game", "--ping", "conductor", "--sampling"],
            &["game", "--ping", "conductor"],
            &["game", "--script-survey"],
            &["game", "--grant", "precision_shot"],
            &["game", "--press", "0@200"],
            &[
                "game",
                "--open-crafting",
                "--crafting-state",
                "done",
                "--fresh",
            ],
        ] {
            let said = refused(spends).unwrap_or_else(|| panic!("{spends:?} is refused"));
            assert!(said.contains("--household"), "{said}");
            let own = [spends, &["--household", "scratch/crafting/shot.ron"]].concat();
            assert_eq!(refused(&own), None, "{own:?}");
        }
        for looks in [
            &["game"][..],
            &["game", "--open-crafting"],
            &["game", "--open-crafting", "--crafting-state", "empty"],
            &["game", "--open-crafting", "--crafting-state", "pick"],
            &["game", "--open-stores"],
        ] {
            assert_eq!(refused(looks), None, "{looks:?}");
        }
        // With the script the screen opens at the workbench the script
        // walks to, so the two go together.
        let mut cli = Cli::parse_from(["game", "--script-crafting", "--open-crafting"]);
        cli.resolve_play_mode();
        assert!(cli.script_progression && cli.open_crafting.is_some());
        for args in [
            &["game", "--crafting-state", "hand"][..],
            &["game", "--open-crafting", "--crafting-state", "rolled"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }

    /// The stills of step 5: the factory screen, a run started before it
    /// opens with somebody on it, and the finished card with Record open
    /// at a derating.
    #[test]
    fn the_factory_flags_parse() {
        let cli = Cli::parse_from(["game"]);
        assert!(!cli.open_factory && !cli.factory_worker);
        assert_eq!((cli.factory_run, cli.record_derate), (None, 85));
        assert!(!cli.factory_finish && !cli.factory_poor_lot);
        assert_eq!(cli.record_first, None);
        let cli = Cli::parse_from(["game", "--open-factory"]);
        assert!(cli.open_factory);
        assert_eq!(cli.factory_run, None, "a still of the idle screen");
        // A batch of twenty unless another is named.
        let cli = Cli::parse_from(["game", "--open-factory", "--factory-run"]);
        assert_eq!(cli.factory_run, Some(20));
        let cli = Cli::parse_from([
            "game",
            "--open-factory",
            "--factory-run",
            "5",
            "--factory-worker",
        ]);
        assert_eq!((cli.factory_run, cli.factory_worker), (Some(5), true));
        // The bench's still of who is working takes a run and no factory.
        let cli = Cli::parse_from(["game", "--open-bench", "--factory-run", "--factory-worker"]);
        assert!(cli.open_bench && !cli.open_factory);
        assert_eq!((cli.factory_run, cli.factory_worker), (Some(20), true));
        // Record is a state of the crafting screen, at 85 % unless told.
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "record"]);
        assert_eq!(cli.crafting_state, CraftingShot::Record);
        assert_eq!(cli.record_derate, 85);
        let cli = Cli::parse_from([
            "game",
            "--open-crafting",
            "--crafting-state",
            "record",
            "--record-derate",
            "70",
        ]);
        assert_eq!(cli.record_derate, 70);
        {
            use strata::client::ui::crafting::Still;
            assert_eq!(cli.crafting_still(), Still::Record(700), "per-mille");
        }
        // What a still of the factory needs first: a blueprint recorded
        // off a thing made, a lot it refuses, and for the stores the run
        // finished.
        let cli = Cli::parse_from([
            "game",
            "--open-factory",
            "--craft-first",
            "cell_core",
            "--record-first",
            "--factory-poor-lot",
        ]);
        assert_eq!(cli.record_first, Some(85));
        assert!(cli.factory_poor_lot);
        let cli = Cli::parse_from([
            "game",
            "--open-stores",
            "--craft-first",
            "cell_core",
            "--record-first",
            "100",
            "--factory-run",
            "2",
            "--factory-finish",
        ]);
        assert_eq!((cli.record_first, cli.factory_finish), (Some(100), true));
        // A paused run's still and one of the bench at a terminal with
        // everyone on it.
        let cli = Cli::parse_from([
            "game",
            "--open-factory",
            "--factory-run",
            "3",
            "--factory-full-store",
        ]);
        assert!(cli.factory_full_store && !cli.join_all);
        let cli = Cli::parse_from(["game", "--at", "terminal", "--open-bench", "--join-all"]);
        assert!(cli.join_all && cli.open_bench);
        assert_eq!(cli.at.as_deref(), Some("terminal"));
        for args in [
            &["game", "--record-first"][..],
            &["game", "--factory-finish"],
            &["game", "--factory-full-store"],
            &[
                "game",
                "--factory-run",
                "--factory-finish",
                "--factory-full-store",
            ],
            &["game", "--script-crafting", "--join-all"],
            &["game", "--script-crafting", "--factory-poor-lot"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
        for args in [
            &["game", "--factory-worker"][..],
            &["game", "--record-derate", "70"],
            &["game", "--factory-run", "many"],
            &["game", "--script-crafting", "--factory-run"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
        // The script walks to the fabricator itself, so the rule is not
        // lifted for it; a still's run has none to stand at.
        let mut cli = Cli::parse_from(["game", "--script-crafting"]);
        cli.resolve_play_mode();
        assert!(cli.script_progression && cli.factory_run.is_none());
    }

    /// A still that starts a run or records spends the household it is run
    /// on, so it is refused on the one the game is played from and taken
    /// on one of its own; a still that only opens the factory is taken
    /// anywhere.
    #[test]
    fn a_factory_run_is_refused_on_the_played_household() {
        let refused = |args: &[&str]| Cli::parse_from(args).spends_the_played_household();
        for spends in [
            &["game", "--factory-run"][..],
            &["game", "--open-factory", "--factory-run", "3"],
            &["game", "--open-bench", "--factory-run", "--factory-worker"],
            &["game", "--open-factory", "--factory-run", "--fresh"],
            &["game", "--open-crafting", "--crafting-state", "record"],
            &["game", "--open-factory", "--factory-poor-lot"],
            &["game", "--open-bench", "--join-all"],
        ] {
            let said = refused(spends).unwrap_or_else(|| panic!("{spends:?} is refused"));
            assert!(said.contains("--household"), "{said}");
            let own = [spends, &["--household", "scratch/crafting/shot.ron"]].concat();
            assert_eq!(refused(&own), None, "{own:?}");
        }
        let said = refused(&["game", "--factory-run"]).unwrap_or_default();
        assert!(said.starts_with("--factory-run"), "{said}");
        for looks in [
            &["game", "--open-factory"][..],
            &["game", "--open-bench"],
            &["game", "--open-loadout"],
        ] {
            assert_eq!(refused(looks), None, "{looks:?}");
        }
    }

    /// The still of step 6: the sheet on its Loadout tab, which changes
    /// nothing in the household.
    #[test]
    fn the_loadout_flag_reads() {
        assert!(!Cli::parse_from(["game"]).open_loadout);
        let cli = Cli::parse_from(["game", "--open-loadout", "--grant", "heavy_cut"]);
        assert!(cli.open_loadout);
        assert_eq!(cli.grant, ["heavy_cut"]);
    }

    /// The stills of step 3: the survey screen, the register, a tracked deposit, a
    /// start before the bench place, and the stores on one kind of stock.
    #[test]
    fn the_survey_flags_read() {
        use strata::client::ui::stores::Filter;
        let cli = Cli::parse_from(["game"]);
        assert!(!cli.open_survey && !cli.open_register);
        assert!(cli.ping.is_none() && !cli.sampling);
        assert_eq!((cli.track, cli.stores_filter), (None, None));
        let cli = Cli::parse_from([
            "game",
            "--at",
            "deposit:sulphur",
            "--open-survey",
            "--ping",
            "conductor",
            "--track",
            "light_alloy_ore",
        ]);
        assert!(cli.open_survey);
        assert_eq!(cli.ping.as_deref(), Some("conductor"));
        assert_eq!(cli.at.as_deref(), Some("deposit:sulphur"));
        assert_eq!(cli.track.as_deref(), Some("light_alloy_ore"));
        let cli = Cli::parse_from(["game", "--at", "bench", "--open-register"]);
        assert_eq!(cli.at.as_deref(), Some("bench"));
        assert!(cli.open_register);
        assert!(at_deposit(&cli).is_none());
        let cli = Cli::parse_from(["game", "--open-stores", "--stores-filter", "lots"]);
        assert_eq!((cli.stores_filter, cli.stores_pin), (Some(Filter::Lots), 0));
        let cli = Cli::parse_from([
            "game",
            "--open-stores",
            "--stores-filter",
            "parts",
            "--stores-pin",
            "2",
        ]);
        assert_eq!(cli.stores_pin, 2);
        assert!(Cli::try_parse_from(["game", "--open-stores", "--stores-pin", "2"]).is_err());
        assert!(Cli::parse_from(["game", "--at", "deposit", "--no-hostiles"]).no_hostiles);
        assert_eq!(
            Cli::parse_from(["game", "--hostiles", "marksman@60,duelist"])
                .hostiles
                .len(),
            2
        );
        for args in [
            &["game", "--hostiles", "marksman", "--no-hostiles"][..],
            &["game", "--hostiles", "archer"],
            &["game", "--hostiles", "marksman@far"],
            &["game", "--hostiles", "duelist@0"],
            &["game", "--stores-filter", "ore"][..],
            &["game", "--at", "benches"],
            &["game", "--track"],
            &["game", "--no-hostiles", "--script-progression"],
            &["game", "--no-hostiles", "--no-combat"],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }
}

#[cfg(test)]
mod render_look_tests {
    use super::*;

    #[test]
    fn combat_is_default_but_diagnostics_and_network_modes_keep_their_controller() {
        for (args, enabled) in [
            (vec!["game"], true),
            (vec!["game", "--combat-script"], true),
            (vec!["game", "--no-combat"], false),
            (vec!["game", "--connect", "127.0.0.1:5000"], false),
            (vec!["game", "--auto-walk"], false),
            (vec!["game", "--flycam"], false),
            (vec!["game", "--flight"], false),
        ] {
            let mut cli = Cli::parse_from(args);
            cli.resolve_play_mode();
            assert_eq!(cli.combat, enabled);
        }
        assert!(Cli::try_parse_from(["game", "--combat-script", "--no-combat"]).is_err());
    }

    #[test]
    fn saved_look_reloads_with_explicit_cli_precedence_and_safe_fallback() {
        let dir = std::env::temp_dir().join(format!("strata-look-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test directory");
        let saved = dir.join("render-look.ron");
        let explicit = dir.join("explicit.ron");
        let missing = dir.join("missing.ron");
        let cli = Cli::parse_from(["game"]);
        assert_eq!(
            resolve_render_look_at(&cli, &missing).expect("missing fallback"),
            LookFile::default()
        );
        let mut edited = LookFile::preset("cinematic-woodland").expect("preset");
        edited.sky.ev100 = 15.1;
        std::fs::write(&saved, edited.to_ron().expect("export")).expect("write export");
        assert_eq!(
            resolve_render_look_at(&cli, &saved).expect("reload"),
            edited
        );
        let preset = Cli::parse_from(["game", "--render-look", "baseline"]);
        assert_eq!(
            resolve_render_look_at(&preset, &saved).expect("explicit preset"),
            LookFile::default()
        );
        let override_cli = Cli::parse_from(["game", "--ev100", "14.0"]);
        let mut overridden = edited.clone();
        overridden.sky.ev100 = 14.0;
        assert_eq!(
            resolve_render_look_at(&override_cli, &saved).expect("CLI override"),
            overridden
        );
        let clear = LookFile::preset("clear-contrast").expect("preset");
        std::fs::write(&explicit, clear.to_ron().expect("export")).expect("write explicit");
        let file_cli = Cli::parse_from([
            "game",
            "--render-look",
            "baseline",
            "--render-look-file",
            explicit.to_str().expect("path"),
        ]);
        assert_eq!(
            resolve_render_look_at(&file_cli, &saved).expect("explicit file"),
            clear
        );
        // A look saved before the ground fog existed has no entry for it
        // and draws it.
        let old = edited
            .to_ron()
            .expect("export")
            .replace("ground_fog: true,", "");
        assert!(!old.contains("ground_fog"));
        std::fs::write(&saved, old).expect("write old export");
        assert!(
            resolve_render_look_at(&cli, &saved)
                .expect("old export")
                .look
                .ground_fog
        );
        std::fs::write(&saved, "not valid RON").expect("invalid saved file");
        assert_eq!(
            resolve_render_look_at(&cli, &saved).expect("invalid fallback"),
            LookFile::default()
        );
        std::fs::write(&explicit, "not valid RON").expect("invalid explicit file");
        assert!(resolve_render_look_at(&file_cli, &saved).is_err());
        std::fs::remove_dir_all(&dir).expect("test cleanup");
    }
}
