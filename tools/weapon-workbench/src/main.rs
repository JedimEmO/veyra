//! An interactive assembly studio over reusable ashlar weapon recipes.
mod catalogue;
mod export;
mod materials;
mod studio;
mod surfaces;
mod ui;

use std::{collections::VecDeque, fmt::Write as _, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{RenderPlugin, pipelined_rendering::PipelinedRenderingPlugin},
    window::ExitCondition,
    winit::WinitPlugin,
};
use clap::{Parser, ValueEnum};
use veyra_weapons::{Assembly, Finish, Maker, Slot, Weapon};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum ViewMode {
    #[default]
    Studio,
    Side,
    Top,
}

#[derive(Parser, Resource, Clone)]
#[command(about = "Veyra procedural weapon assembly studio")]
struct Options {
    /// Write the complete current design catalogue to RON and exit without rendering.
    #[arg(long)]
    catalogue: Option<PathBuf>,
    /// Initial weapon's stable key, such as `burst_rifle` or `field_blade`.
    #[arg(long, default_value = "patrol_carbine")]
    weapon: String,
    /// Start from a named configuration, selecting its chassis automatically.
    #[arg(long, conflicts_with_all = ["config", "weapon"])]
    preset: Option<String>,
    /// Override the saved or default finish: issued, field or bare.
    #[arg(long, value_parser = ["issued", "field", "bare"])]
    finish: Option<String>,
    /// Start from the studio, side or top inspection angle.
    #[arg(long, value_enum, default_value = "studio")]
    view: ViewMode,
    /// Compare the stock geometry while retaining the fitted selection.
    #[arg(long)]
    compare: bool,
    /// Install a compatible module; may be repeated.
    #[arg(long = "module")]
    modules: Vec<String>,
    /// Read an existing assembly; also used by the Save and Load buttons.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Directory for saved assemblies and exported models.
    #[arg(long, default_value = "scratch/weapon-workbench")]
    output: PathBuf,
    /// Render the full workbench to a PNG without opening a window, then exit.
    #[arg(long, conflicts_with = "gallery")]
    capture: Option<PathBuf>,
    /// Render every stock and fitted chassis plus an exploded rifle into this directory.
    #[arg(long)]
    gallery: Option<PathBuf>,
    /// Start with separated components.
    #[arg(long)]
    exploded: bool,
    /// Drive native UI buttons before capture and verify their effect.
    #[arg(long)]
    exercise: bool,
    /// Output/window width, at least 1120.
    #[arg(long, default_value_t = 1440)]
    width: u32,
    /// Output/window height, at least 800.
    #[arg(long, default_value_t = 960)]
    height: u32,
}

#[derive(Resource)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent inspection toggles are not mutually exclusive states"
)]
pub(crate) struct Workbench {
    pub assembly: Assembly,
    pub maker_filter: Option<Maker>,
    pub compare_stock: bool,
    pub exploded: bool,
    pub auto_rotate: bool,
    pub clay: bool,
    pub status: String,
    pub mesh_count: usize,
    pub triangle_count: usize,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Weapon(Weapon),
    Maker(Option<Maker>),
    Preset(String),
    ToggleCompare,
    ViewSide,
    ViewTop,
    Module(Slot, Option<String>),
    Finish(Finish),
    ToggleExploded,
    ToggleRotate,
    ToggleClay,
    ResetView,
    ResetBuild,
    Save,
    Load,
    Export,
}

#[derive(Resource, Default)]
pub(crate) struct StudioActions(pub Vec<Action>);

struct CaptureCase {
    assembly: Assembly,
    exploded: bool,
    path: PathBuf,
}

#[derive(Resource, Default)]
struct Captures {
    cases: VecDeque<CaptureCase>,
    target: Option<Handle<Image>>,
    frames: u32,
    pending: bool,
    exercise_step: usize,
}

fn read_assembly(path: &std::path::Path) -> Result<Assembly> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let assembly: Assembly = ron::from_str(&text).context("reading assembly RON")?;
    assembly.validate()?;
    Ok(assembly)
}

fn main() -> Result<()> {
    let options = Options::parse();
    if let Some(path) = &options.catalogue {
        return catalogue::write(path);
    }
    if options.width < 1120 || options.height < 800 {
        bail!("the assembly studio needs at least 1120 x 800 pixels");
    }
    let mut assembly = initial_assembly(&options)?;
    let captures = capture_cases(&options, &assembly)?;
    if let Some(case) = captures.cases.front() {
        assembly.clone_from(&case.assembly);
    }
    let headless = !captures.cases.is_empty();
    if options.exercise && (options.capture.is_none() || options.gallery.is_some()) {
        bail!("--exercise requires --capture without --gallery");
    }
    let exploded = captures
        .cases
        .front()
        .map_or(options.exploded, |c| c.exploded);
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .set(AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            ..default()
        })
        .set(bevy::app::TaskPoolPlugin {
            task_pool_options: bevy::app::TaskPoolOptions::with_num_threads(4),
        });
    if headless {
        app.add_plugins(
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>()
                .disable::<PipelinedRenderingPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)));
    } else {
        app.add_plugins(plugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Veyra | Weapon workbench".into(),
                resolution: (options.width, options.height).into(),
                resize_constraints: bevy::window::WindowResizeConstraints {
                    min_width: 1120.0,
                    min_height: 800.0,
                    ..default()
                },
                ..default()
            }),
            ..default()
        }));
    }
    let maker_filter = Some(assembly.weapon.maker());
    let compare_stock = options.compare;
    app.insert_resource(options)
        .insert_resource(captures)
        .insert_resource(Workbench {
            assembly,
            maker_filter,
            compare_stock,
            exploded,
            auto_rotate: false,
            clay: false,
            status: "Select a chassis, then fit compatible components.".into(),
            mesh_count: 0,
            triangle_count: 0,
            revision: 1,
        })
        .init_resource::<StudioActions>()
        .init_resource::<materials::MaterialBank>();
    studio::plugin(&mut app);
    ui::plugin(&mut app);
    if app.run().is_error() {
        bail!("workbench verification or capture failed");
    }
    Ok(())
}

fn capture_cases(options: &Options, assembly: &Assembly) -> Result<Captures> {
    let mut captures = Captures::default();
    if let Some(directory) = &options.gallery {
        std::fs::create_dir_all(directory)?;
        for weapon in Weapon::ALL {
            let stock = Assembly {
                weapon,
                ..Assembly::default()
            };
            captures.cases.push_back(CaptureCase {
                assembly: stock.clone(),
                exploded: false,
                path: directory.join(format!("{}-stock.png", weapon.key())),
            });
            let mut fitted = stock;
            for &slot in weapon.slots() {
                if let Some(module) = veyra_weapons::modules(weapon, slot).last() {
                    fitted.select(slot, Some(module.id))?;
                }
            }
            captures.cases.push_back(CaptureCase {
                assembly: fitted,
                exploded: false,
                path: directory.join(format!("{}-fitted.png", weapon.key())),
            });
            for preset in veyra_weapons::presets::for_weapon(weapon) {
                captures.cases.push_back(CaptureCase {
                    assembly: preset.assembly(Finish::Issued)?,
                    exploded: false,
                    path: directory.join(format!("{}.png", preset.id)),
                });
            }
        }
        let mut exploded = Assembly::default();
        for &slot in exploded.weapon.slots() {
            if let Some(module) = veyra_weapons::modules(exploded.weapon, slot).last() {
                exploded.select(slot, Some(module.id))?;
            }
        }
        captures.cases.push_back(CaptureCase {
            assembly: exploded,
            exploded: true,
            path: directory.join("burst_rifle-exploded.png"),
        });
        let recon = veyra_weapons::presets::PRESETS
            .iter()
            .find(|preset| preset.id == "vigil_recon")
            .context("recon preset")?;
        captures.cases.push_back(CaptureCase {
            assembly: recon.assembly(Finish::Issued)?,
            exploded: true,
            path: directory.join("vigil-recon-exploded.png"),
        });
        for (finish, name) in [(Finish::Field, "field"), (Finish::Bare, "bare")] {
            captures.cases.push_back(CaptureCase {
                assembly: recon.assembly(finish)?,
                exploded: false,
                path: directory.join(format!("vigil-recon-{name}.png")),
            });
        }
        let mut index = String::from(
            "<!doctype html><meta charset=utf-8><title>Veyra weapon assemblies</title><style>body{background:#12191e;color:#eee;font:16px system-ui;margin:2rem}main{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:1rem}img{width:100%}figure{margin:0}a{color:#e4ae60}</style><h1>Veyra weapon assemblies</h1><p>Actual Rust workbench renders. Stock, fitted and exploded procedural geometry.</p><main>",
        );
        for case in &captures.cases {
            let name = case
                .path
                .file_name()
                .context("capture name")?
                .to_string_lossy();
            write!(
                index,
                "<figure><a href='{name}'><img src='{name}'></a><figcaption>{name}</figcaption></figure>"
            )?;
        }
        index.push_str("</main>");
        std::fs::write(directory.join("index.html"), index)?;
    } else if let Some(path) = &options.capture {
        captures.cases.push_back(CaptureCase {
            assembly: assembly.clone(),
            exploded: options.exploded,
            path: path.clone(),
        });
    }
    Ok(captures)
}

fn initial_assembly(options: &Options) -> Result<Assembly> {
    let mut assembly = if let Some(id) = &options.preset {
        veyra_weapons::presets::PRESETS
            .iter()
            .find(|preset| preset.id == id)
            .with_context(|| format!("unknown preset {id}"))?
            .assembly(Finish::Issued)?
    } else if let Some(path) = &options.config {
        read_assembly(path)?
    } else {
        let weapon = Weapon::ALL
            .into_iter()
            .find(|w| w.key() == options.weapon)
            .with_context(|| {
                format!(
                    "unknown weapon {}; use one of {}",
                    options.weapon,
                    Weapon::ALL.map(Weapon::key).join(", ")
                )
            })?;
        Assembly {
            weapon,
            ..Assembly::default()
        }
    };
    if let Some(finish) = &options.finish {
        assembly.finish = match finish.as_str() {
            "field" => Finish::Field,
            "bare" => Finish::Bare,
            _ => Finish::Issued,
        };
    }
    for id in &options.modules {
        let module = veyra_weapons::MODULES
            .iter()
            .find(|m| m.id == id)
            .with_context(|| format!("unknown module {id}"))?;
        assembly.select(module.slot, Some(id))?;
    }
    Ok(assembly)
}
