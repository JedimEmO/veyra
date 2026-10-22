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
mod grounding;
mod household;
mod pieces;
#[cfg_attr(
    not(target_arch = "wasm32"),
    allow(
        dead_code,
        reason = "the web entry calls it; a native run only tests it"
    )
)]
mod query;
mod roadside;
mod route;
mod settlement;
mod shutdown;
mod sites;
mod water_fetch;
#[cfg(target_arch = "wasm32")]
mod web;

use std::net::SocketAddr;
#[cfg(feature = "dev-tools")]
use std::path::Path;
use std::path::PathBuf;
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

use bevy::window::ExitSystems;
use bevy::window::{MonitorSelection, PresentMode, WindowMode, WindowResolution};
use clap::Parser;
use strata::client::flight::{MotionHold, motion_held};
use strata::foliage::FoliageConfig;
use strata::prelude::*;
use strata::registry::files::Files;
use strata::registry::progression::SkillGraph;
use strata::sim::Wind;
use strata::sim::combat_tables::CombatTables;
use veyra_game::deposit_start as deposits;
use veyra_game::{GamePlugin, GameSet, Launch};

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
    #[arg(long, conflicts_with_all = ["combat"])]
    no_combat: bool,
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
    /// Run the simulation with no window, no renderer.
    #[arg(long)]
    headless: bool,
    /// Exit after this many frames.
    #[arg(long)]
    frames: Option<u32>,
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
    /// the two directories: how the web build's data is checked natively. The
    /// rivers it does not carry are fetched from `--generated` as the browser
    /// fetches them from the page.
    #[arg(long)]
    data_bundle: Option<PathBuf>,
    /// Window size in pixels, as `WxH`. The Phase 2 and 3 targets are
    /// measured at 1440p.
    #[arg(long, default_value = "2560x1440", value_parser = parse_size)]
    window: (u32, u32),
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
    /// Every outlaw camp stands up at this tier whatever its distance from
    /// Port Serein, for a still of the bigger crowd.
    #[arg(long, value_name = "TIER", value_parser = clap::value_parser!(u8).range(1..=3))]
    camp_tier: Option<u8>,
    /// Start the encounter with no hostiles in it: the character, the
    /// household and the world as they are, with no fight and no
    /// nameplates, for a screenshot of a place.
    #[arg(long, conflicts_with_all = ["no_combat"])]
    no_hostiles: bool,
    /// Replace the encounter's hostiles with these, `kind[@metres]` each,
    /// comma separated: `marksman@60`, `duelist@6`, `sentry,rusher@10`.
    /// Kinds are sentry, rusher, marksman and duelist; a missing distance is
    /// the kind's own, straight ahead of the start. For a still or a test.
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = combat::parse_hostile,
        conflicts_with_all = ["no_hostiles", "no_combat"],
    )]
    hostiles: Vec<combat::HostileSpec>,
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
    /// The graphics preset for this run, for stills and the bench: it wins
    /// over the saved choice and is not kept.
    #[arg(long, value_parser = ["low", "medium", "high"])]
    graphics: Option<String>,
    /// The share of the window's width and height the 3D camera renders at,
    /// 0.5 to 1 (the look's `render_scale`); a graphics preset's ceiling
    /// still applies. The UI stays at the window's size and a screenshot is
    /// the window as presented.
    #[arg(long, value_parser = parse_render_scale)]
    render_scale: Option<f32>,
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
    /// The development flags: stills, scripts, the benchmark and the look lab
    /// (`veyra-dev`), absent from a build that leaves `dev-tools` off.
    #[cfg(feature = "dev-tools")]
    #[command(flatten)]
    dev: veyra_dev::DevArgs,
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

fn parse_render_scale(s: &str) -> Result<f32, String> {
    let v = s
        .trim()
        .parse::<f32>()
        .map_err(|e| format!("{s:?} is not a number: {e}"))?;
    if (0.5..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(format!("the render scale is 0.5 to 1, not {v}"))
    }
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
    if let Some(scale) = cli.render_scale {
        file.look.render_scale = scale;
    }
    file.validate()?;
    Ok(file)
}

impl Cli {
    /// Why this run may not start, when it is a still that spends stock on
    /// the household the game is played from.
    #[cfg(feature = "dev-tools")]
    fn spends_the_played_household(&self) -> Option<String> {
        let flag = self.dev.spends_stock()?;
        (self.household == Path::new(WORKSPACE_HOUSEHOLD)).then(|| {
            format!(
                "{flag} spends the stock of the household it is run on and moves its roll and batch counters: pass --household <path> (with --fresh for a first day), not the default {WORKSPACE_HOUSEHOLD}"
            )
        })
    }

    /// Whether `--script-survey` is on.
    #[cfg_attr(
        not(feature = "dev-tools"),
        allow(
            clippy::unused_self,
            reason = "the flags it reads are the development half's"
        )
    )]
    fn script_survey_class(&self) -> Option<&str> {
        #[cfg(feature = "dev-tools")]
        return self.dev.script_survey.as_deref();
        #[cfg(not(feature = "dev-tools"))]
        None
    }

    fn script_survey(&self) -> bool {
        self.script_survey_class().is_some()
    }

    /// `--orbit-distance`, `--orbit-yaw` and `--orbit-pitch`, for stills.
    #[cfg_attr(
        not(feature = "dev-tools"),
        allow(
            clippy::unused_self,
            reason = "the flags it reads are the development half's"
        )
    )]
    fn orbit(&self) -> (Option<f32>, Option<f32>, Option<f32>) {
        #[cfg(feature = "dev-tools")]
        return (
            self.dev.orbit_distance,
            self.dev.orbit_yaw,
            self.dev.orbit_pitch,
        );
        #[cfg(not(feature = "dev-tools"))]
        (None, None, None)
    }

    /// Whether a screenshot or a benchmark summary is being taken.
    #[cfg_attr(
        not(feature = "dev-tools"),
        allow(
            clippy::unused_self,
            reason = "the flags it reads are the development half's"
        )
    )]
    fn recorded(&self) -> bool {
        #[cfg(feature = "dev-tools")]
        return self.dev.screenshot.is_some() || self.dev.bench_out.is_some();
        #[cfg(not(feature = "dev-tools"))]
        false
    }

    /// `--cache`: a cleared camp leaves its cache.
    #[cfg_attr(
        not(feature = "dev-tools"),
        allow(
            clippy::unused_self,
            reason = "the flags it reads are the development half's"
        )
    )]
    fn cache(&self) -> bool {
        #[cfg(feature = "dev-tools")]
        return self.dev.cache;
        #[cfg(not(feature = "dev-tools"))]
        false
    }

    /// Whether a script drives the run on a manual clock, one tick a frame, so
    /// pacing the loop would only make it slower.
    #[cfg_attr(
        not(feature = "dev-tools"),
        allow(
            clippy::unused_self,
            reason = "the flags it reads are the development half's"
        )
    )]
    fn scripted_clock(&self) -> bool {
        #[cfg(feature = "dev-tools")]
        return self.dev.combat_script
            || self.dev.script_survey.is_some()
            || self.dev.script_quest.is_some();
        #[cfg(not(feature = "dev-tools"))]
        false
    }

    fn resolve_play_mode(&mut self) {
        #[cfg(feature = "dev-tools")]
        {
            if self.dev.script_survey.is_some() {
                // Nobody to fight on the way: the script is about the ground.
                self.no_hostiles = true;
            }
            if self.dev.script_ride && self.at.is_none() {
                self.at = Some("road".to_owned());
            }
            self.dev.resolve();
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
        if at_deposit(cli).is_some() || cli.script_survey() {
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
    #[cfg(feature = "dev-tools")]
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
    {
        web::phase("wasm started");
        web::install_ports();
        web::phase("bundle read");
    }
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
        water_cache(&cli).as_deref(),
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
    #[cfg(target_arch = "wasm32")]
    web::phase("rivers ready");
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

/// Where a native start keeps the baked rivers, a directory beside the
/// generated assets (an index and a file a mass; gitignored, regenerated,
/// shared with the server). None in the browser and with a data bundle, which
/// carries its own.
fn water_cache(cli: &Cli) -> Option<std::path::PathBuf> {
    if cfg!(target_arch = "wasm32") || cli.data_bundle.is_some() {
        return None;
    }
    Some(cli.generated.join("hydrology"))
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
        let wait = if cli.scripted_clock() {
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
                (camps::resolve_at, camps::sync_launch)
                    .chain()
                    .after(GameSet::World)
                    .before(GameSet::Household),
                spawn_headless_player.after(GameSet::Household),
            ),
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
        if let Some(graphics) = cli
            .graphics
            .as_deref()
            .and_then(strata::client::ui::graphics::Graphics::from_id)
        {
            app.insert_resource(strata::client::ui::graphics::Forced(graphics));
        }
        // Before `StrataPlugins::client()` reads the settings path.
        #[cfg(target_arch = "wasm32")]
        web::insert_resources(&mut app);
        #[cfg(feature = "dev-tools")]
        if cli.dev.wireframe {
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
            veyra_ui::VeyraUiPlugin,
            world_load(&cli),
            // The content step validates what it bakes; the preflight would read every map twice
            // and, on the web, fetch all of them in turn before the city could draw.
            AshlarPlugin {
                preflight: false,
                ..default()
            },
        ))
        .add_systems(
            OnEnter(AppState::InWorld),
            (
                (camps::resolve_at, camps::sync_launch)
                    .chain()
                    .after(GameSet::World)
                    .before(GameSet::Household),
                spawn_camera.after(GameSet::Household),
            ),
        );
        if cfg!(target_arch = "wasm32") || cli.data_bundle.is_some() {
            water_fetch::install(&mut app);
        }
        let generated = data_files(&cli).under("generated");
        // The weapon pictures' index, read once; without it the diagram
        // draws boxes. A window's business only: nothing below the client
        // knows the pictures exist. After the plugins, so the log plugin
        // is there to say which it was.
        app.insert_resource(veyra_ui::weapon_diagram::WeaponPictures::read(&generated));
        // The weapon models' index likewise: without it the hand keeps the
        // old art.
        app.insert_resource(veyra_ui::weapon_model::WeaponModels::read(&generated));
        // The bike's likewise: without it a box stands under the rider.
        app.insert_resource(strata::client::view::BikeModels::read(&generated));
        // The props' likewise: without it the cuboids stand where the kit's
        // pieces would.
        app.insert_resource(household::PropKit::read(&generated));
        app.insert_resource(veyra_ui::fitting::WellView::Model);
        settlement::install(&mut app);
        sites::install(&mut app);
        crossings::install(&mut app);
        roadside::install(&mut app);
        shutdown::install(&mut app);
        // A scripted run (a benchmark, a flight, a capture, a frame count)
        // must not depend on whether its window happened to get focus:
        // Bevy's game default drops an unfocused window to 60 Hz, which a
        // benchmark would record as the game's frame rate.
        if cli.frames.is_some() || cli.flight || cli.recorded() {
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
            let Some((replication, parts)) = client_replication(server, &cli) else {
                return Err("could not configure replication".to_owned());
            };
            // The game's numbers in the handshake, before the plugin reads them.
            app.insert_resource(parts)
                .add_plugins(replication)
                .add_systems(Update, script_connected_player);
        }
    }

    install_scenarios(&mut app, &cli, prepared);
    insert_overrides(&mut app, &cli);
    combat::install(&mut app, &cli);
    app.add_plugins(GamePlugin::new(
        launch_of(&cli),
        household::household_file(&cli),
    ))
    .configure_sets(Last, ExitSystems.before(GameSet::SaveOnExit));
    household::install(&mut app, &cli);
    #[cfg(feature = "dev-tools")]
    veyra_dev::install(
        &mut app,
        &cli.dev,
        cli.headless,
        cli.combat,
        cli.look,
        veyra_dev::RunFacts {
            frames: cli.frames,
            flight: cli.flight,
            flight_secs: cli.flight_secs,
        },
    );
    #[cfg(not(feature = "dev-tools"))]
    if let Some(frames) = cli.frames {
        app.add_systems(
            Update,
            move |count: Res<FrameCount>, mut exit: MessageWriter<AppExit>| {
                if count.0 >= frames {
                    exit.write(AppExit::Success);
                }
            },
        );
    }
    let auto_walk = cli.auto_walk;
    app.insert_resource(cli).add_systems(
        Update,
        report_stalled_walker.run_if(any_with_component::<ScriptedIntent>),
    );
    if auto_walk {
        app.add_systems(Update, release_scripted_walk);
    }
    Ok(app)
}

/// What the headless side reads of the command line.
fn launch_of(cli: &Cli) -> Launch {
    #[allow(unused_mut, reason = "only the development flags add to it")]
    let mut launch = Launch {
        combat: cli.combat,
        headless: cli.headless,
        frames: cli.frames,
        at: cli.at.clone(),
        spawn: cli.spawn,
        camp_tier: cli.camp_tier,
        deposits: cli.combat
            || at_deposit(cli).is_some()
            || cli.at.as_deref().and_then(camps::claim_spec).is_some(),
        ..default()
    };
    #[cfg(feature = "dev-tools")]
    cli.dev.launch(&mut launch);
    launch
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
    // Masses that come over the network are read (or fetched) as far as the
    // first view samples; a start with every file at hand bakes or reads out
    // to the whole 128 km. The rounds ahead keep the whole reach, and the
    // first one asks for the 128 km square at once, so a sample past 96 km
    // finds its mass fetched, not baked on the browser's one thread.
    let web = cfg!(target_arch = "wasm32") || cli.data_bundle.is_some();
    let reach = if web {
        strata::sim::WEB_PREPARE_RADIUS_M
    } else {
        strata::sim::PREPARE_WATER_RADIUS_M
    };
    strata::sim::PrepareWater {
        places: vec![cli.spawn.unwrap_or((0.0, 0.0))],
        radius_m: reach + flown,
        ahead_radius_m: web.then_some(strata::sim::PREPARE_WATER_RADIUS_M + flown),
    }
}

/// Height of the flycam above the ground at spawn and on the flight.
const CAMERA_ALTITUDE: f64 = 120.0;

pub(crate) use veyra_game::PLAYER_DROP;

pub(crate) use veyra_game::PLAYER_HEIGHT_M;

/// Where `--spawn` (or dry land near the origin) puts things: the dry
/// point and the ground height there.
fn spawn_point(
    cli: &Cli,
    source: &WorldSourceRes,
    settlements: Option<&strata::sim::Settlements>,
    survey: Option<&strata::sim::deposits::Survey>,
) -> (f64, f64, f64) {
    if let Some(class) = cli.script_survey_class()
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
    quest: Option<&household::people::QuestStart>,
    scripted: bool,
    held: bool,
) {
    let (x, ground, z) = if let Some(start) = quest {
        let ground = f64::from(
            source
                .source()
                .sample(start.at.0, start.at.1)
                .height
                .max(source.source().sea_level()),
        );
        (start.at.0, ground, start.at.1)
    } else {
        spawn_point(cli, source, settlements, survey)
    };
    let heading = quest.map_or_else(|| initial_heading(cli, settlements), |start| start.yaw);
    let controller = if cli.combat {
        let actor = &strata::sim::combat_tables::current().actor;
        CharacterController {
            responsive: true,
            height: PLAYER_HEIGHT_M,
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
        let staging = if quest.is_some() {
            // The questline places the household on the quay, and the
            // training ground stays where it always was.
            combat::default_spawn(source, settlements).map(|(sx, sg, sz)| {
                let yaw = initial_heading(cli, settlements);
                commands.insert_resource(combat::StagingSpawn {
                    at: WorldPos::new(sx, sg, sz),
                    yaw,
                });
                ((sx, sz), yaw)
            })
        } else {
            (cli.at.is_none() && cli.spawn.is_none())
                .then(|| ((x, z), initial_heading(cli, settlements)))
        };
        commands.insert_resource(household::sites(staging, settlements));
    }
    let mut player = commands.spawn((
        Player { id: PlayerId(1) },
        LocalPlayer,
        controller,
        WorldPosition(at),
        PlayerIntent {
            yaw: heading,
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
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn spawn_headless_player(
    mut commands: Commands,
    cli: Res<Cli>,
    source: Res<WorldSourceRes>,
    settlements: Option<Res<strata::sim::Settlements>>,
    registries: Res<strata::sim::Registries>,
    household: Option<Res<household::Household>>,
    survey: Option<Res<strata::sim::deposits::Survey>>,
    quest: Option<Res<household::people::QuestStart>>,
) {
    spawn_player(
        &mut commands,
        &cli,
        &source,
        settlements.as_deref(),
        &registries,
        household.as_deref(),
        survey.as_deref(),
        quest.as_deref(),
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
    quest: Option<Res<household::people::QuestStart>>,
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
            quest.as_deref(),
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
    let (orbit_distance, orbit_yaw, orbit_pitch) = cli.orbit();
    if cli.combat {
        commands.entity(camera).insert((
            veyra_view::combat::CombatCamera {
                hold: orbit_distance,
                yaw_offset: orbit_yaw.unwrap_or(0.).to_radians(),
                ..default()
            },
            OrbitCamera {
                pitch: orbit_pitch.map_or(-0.16, f32::to_radians),
                yaw: quest.as_deref().map_or_else(
                    || initial_heading(&cli, settlements.as_deref()),
                    |start| start.yaw,
                ),
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

#[cfg(feature = "net")]
/// The adapter as a client of `server`, with the versions of the tree
/// library and the site groups under `--generated` and of the world the
/// assets under `--assets` build, the `--seed` and the installed combat
/// tables, folded into its protocol hash, so a server on other trees, other
/// ground, other numbers or other site buildings refuses this client and
/// says which. `None`
/// having said why a version cannot be read (most often: run `just
/// content`).
fn client_replication(
    server: std::net::SocketAddr,
    cli: &Cli,
) -> Option<(ReplicationPlugin, strata::net_replicon::VersionParts)> {
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
    let parts =
        strata::game_version_parts(strata::sim::combat_tables::current().version(), site_groups);
    Some((
        ReplicationPlugin::client(server)
            .with_tree_library(library)
            .with_world_version(world)
            .with_seed(cli.seed),
        parts,
    ))
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

#[cfg(all(test, feature = "dev-tools"))]
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
        assert_eq!(Cli::parse_from(["game"]).dev.open_fitting, None);
        let cli = Cli::parse_from(["game", "--open-fitting"]);
        assert_eq!(cli.dev.open_fitting.as_deref(), Some("feed"));
        assert_eq!((cli.dev.fitting_hand, cli.dev.fitting_pick), (None, false));
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
        assert_eq!(cli.dev.open_fitting.as_deref(), Some("muzzle"));
        assert_eq!(
            (cli.dev.fitting_hand, cli.dev.fitting_pick),
            (Some(1), true)
        );
        assert_eq!(cli.dev.fitted, ["veyra:module.patrol_extended_feed"]);
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
        assert_eq!(cli.dev.open_crafting, None);
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Pick);
        let cli = Cli::parse_from(["game", "--open-crafting"]);
        assert_eq!(cli.dev.open_crafting.as_deref(), Some("cell_core"));
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Pick);
        let cli = Cli::parse_from([
            "game",
            "--open-crafting",
            "patrol_extended_feed",
            "--crafting-state",
            "hand",
        ]);
        assert_eq!(
            cli.dev.open_crafting.as_deref(),
            Some("patrol_extended_feed")
        );
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Hand);
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "done"]);
        assert_eq!(cli.dev.open_crafting.as_deref(), Some("cell_core"));
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Done);
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "empty"]);
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Empty);
        // A thing made first, for a still of what takes it or of its card.
        let cli = Cli::parse_from([
            "game",
            "--craft-first",
            "cell_core",
            "--open-crafting",
            "patrol_extended_feed",
        ]);
        assert_eq!(cli.dev.craft_first.as_deref(), Some("cell_core"));
        assert_eq!(
            cli.dev.open_crafting.as_deref(),
            Some("patrol_extended_feed")
        );
        let cli = Cli::parse_from([
            "game",
            "--craft-first",
            "cell_core",
            "--open-stores",
            "--stores-filter",
            "parts",
        ]);
        assert!(cli.dev.open_stores && cli.dev.craft_first.is_some());
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
        assert!(cli.dev.script_progression && cli.dev.open_crafting.is_some());
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
    #[allow(clippy::too_many_lines, reason = "one flag family in one test")]
    fn the_factory_flags_parse() {
        let cli = Cli::parse_from(["game"]);
        assert!(!cli.dev.open_factory && !cli.dev.factory_worker);
        assert_eq!((cli.dev.factory_run, cli.dev.record_derate), (None, 85));
        assert!(!cli.dev.factory_finish && !cli.dev.factory_poor_lot);
        assert_eq!(cli.dev.record_first, None);
        let cli = Cli::parse_from(["game", "--open-factory"]);
        assert!(cli.dev.open_factory);
        assert_eq!(cli.dev.factory_run, None, "a still of the idle screen");
        // A batch of twenty unless another is named.
        let cli = Cli::parse_from(["game", "--open-factory", "--factory-run"]);
        assert_eq!(cli.dev.factory_run, Some(20));
        let cli = Cli::parse_from([
            "game",
            "--open-factory",
            "--factory-run",
            "5",
            "--factory-worker",
        ]);
        assert_eq!(
            (cli.dev.factory_run, cli.dev.factory_worker),
            (Some(5), true)
        );
        // The bench's still of who is working takes a run and no factory.
        let cli = Cli::parse_from(["game", "--open-bench", "--factory-run", "--factory-worker"]);
        assert!(cli.dev.open_bench && !cli.dev.open_factory);
        assert_eq!(
            (cli.dev.factory_run, cli.dev.factory_worker),
            (Some(20), true)
        );
        // Record is a state of the crafting screen, at 85 % unless told.
        let cli = Cli::parse_from(["game", "--open-crafting", "--crafting-state", "record"]);
        assert_eq!(cli.dev.crafting_state, veyra_dev::CraftingShot::Record);
        assert_eq!(cli.dev.record_derate, 85);
        let cli = Cli::parse_from([
            "game",
            "--open-crafting",
            "--crafting-state",
            "record",
            "--record-derate",
            "70",
        ]);
        assert_eq!(cli.dev.record_derate, 70);
        {
            use veyra_ui::crafting::Still;
            assert_eq!(cli.dev.crafting_still(), Still::Record(700), "per-mille");
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
        assert_eq!(cli.dev.record_first, Some(85));
        assert!(cli.dev.factory_poor_lot);
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
        assert_eq!(
            (cli.dev.record_first, cli.dev.factory_finish),
            (Some(100), true)
        );
        // A paused run's still and one of the bench at a terminal with
        // everyone on it.
        let cli = Cli::parse_from([
            "game",
            "--open-factory",
            "--factory-run",
            "3",
            "--factory-full-store",
        ]);
        assert!(cli.dev.factory_full_store && !cli.dev.join_all);
        let cli = Cli::parse_from(["game", "--at", "terminal", "--open-bench", "--join-all"]);
        assert!(cli.dev.join_all && cli.dev.open_bench);
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
        assert!(cli.dev.script_progression && cli.dev.factory_run.is_none());
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
        assert!(!Cli::parse_from(["game"]).dev.open_loadout);
        let cli = Cli::parse_from(["game", "--open-loadout", "--grant", "heavy_cut"]);
        assert!(cli.dev.open_loadout);
        assert_eq!(cli.dev.grant, ["heavy_cut"]);
    }

    /// The stills of step 3: the survey screen, the register, a tracked deposit, a
    /// start before the bench place, and the stores on one kind of stock.
    #[test]
    fn the_survey_flags_read() {
        use veyra_ui::stores::Filter;
        let cli = Cli::parse_from(["game"]);
        assert!(!cli.dev.open_survey && !cli.dev.open_register);
        assert!(cli.dev.ping.is_none() && !cli.dev.sampling);
        assert_eq!((cli.dev.track, cli.dev.stores_filter), (None, None));
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
        assert!(cli.dev.open_survey);
        assert_eq!(cli.dev.ping.as_deref(), Some("conductor"));
        assert_eq!(cli.at.as_deref(), Some("deposit:sulphur"));
        assert_eq!(cli.dev.track.as_deref(), Some("light_alloy_ore"));
        let cli = Cli::parse_from(["game", "--at", "bench", "--open-register"]);
        assert_eq!(cli.at.as_deref(), Some("bench"));
        assert!(cli.dev.open_register);
        assert!(at_deposit(&cli).is_none());
        let cli = Cli::parse_from(["game", "--open-stores", "--stores-filter", "lots"]);
        assert_eq!(
            (cli.dev.stores_filter, cli.dev.stores_pin),
            (Some(Filter::Lots), 0)
        );
        let cli = Cli::parse_from([
            "game",
            "--open-stores",
            "--stores-filter",
            "parts",
            "--stores-pin",
            "2",
        ]);
        assert_eq!(cli.dev.stores_pin, 2);
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
