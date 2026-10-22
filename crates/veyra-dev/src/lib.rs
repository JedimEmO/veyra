//! The development half of the game: the flags of the stills, the scripts
//! and the benchmark runs ([`DevArgs`], flattened into the game's command
//! line), the systems that serve them ([`stills`], [`household`],
//! [`survey_script`]) and [`install`], which adds them to an app.
//!
//! The game links this behind its `dev-tools` feature, on by default natively
//! and off in the browser build, so a shipped or web binary carries none of it.
//! The household's tests drive the scripts through it, which is why they
//! name its items. No server links it: `just server-check` holds it out of
//! the headless graphs.

#![allow(
    missing_docs,
    reason = "scripts and stills are tools, each file's header says what it is, and the game's tests drive their internals"
)]

pub mod args;
pub mod factions;
pub mod household;
pub mod ride;
pub mod setups;
pub mod standing;
pub mod stills;
pub mod survey_script;

pub use args::*;

use bevy::prelude::*;

/// What the development systems read of the game's own command line, which
/// the game fills in: the flags that stay in its `Cli` because the game
/// reads them too.
#[derive(Resource, Debug, Clone, Default)]
pub struct RunFacts {
    /// `--frames`.
    pub frames: Option<u32>,
    /// `--flight`.
    pub flight: bool,
    /// `--flight-secs`.
    pub flight_secs: f64,
}

/// Adds everything the flags in `dev` ask for to `app`: the stills' systems,
/// the household's scripts, the benchmark recorder and the exit rule. The game
/// calls it once, after its own plugins.
///
/// `headless` is `--headless` and `combat` the game's combat play, both of
/// which the game decides; `look` is `--look`.
pub fn install(
    app: &mut App,
    dev: &DevArgs,
    headless: bool,
    combat: bool,
    look: Option<f32>,
    facts: RunFacts,
) {
    use std::time::Duration;

    use strata::client::flight::MotionHold;
    use strata::client::stats::BenchRecorder;

    if !headless {
        stills::install(app, dev);
        app.init_resource::<veyra_game::GuardHold>();
        app.insert_resource(if dev.fitting_view == "diagram" {
            veyra_ui::fitting::WellView::Diagram
        } else {
            veyra_ui::fitting::WellView::Model
        });
        // The debug statistics only when asked for, and never over a
        // screenshot of a screen, whose corner they would cover.
        app.insert_resource(strata::client::hud::HudVisible(
            dev.stats && !dev.opens_a_screen(),
        ));
    }
    stills::install_access(app, dev);
    if dev.bench_out.is_some() && !headless {
        let floor = Duration::from_secs_f64(dev.bench_warmup.max(0.0));
        let ceiling = Duration::from_secs_f64(dev.bench_warm_ceiling.max(0.0));
        let recorder = match dev.bench_warm.as_str() {
            "idle" => BenchRecorder::until_settled(floor, ceiling),
            "hold" => BenchRecorder::holding(
                floor,
                ceiling,
                Duration::from_secs_f64(dev.bench_lead.max(0.0)),
            ),
            _ => BenchRecorder::new(floor),
        };
        app.insert_resource(recorder);
        if dev.bench_warm == "hold" {
            // The flight path and the scripted walkers read it; the
            // recorder lets it go.
            app.insert_resource(MotionHold::default());
        }
    }
    if combat {
        household::install(app, dev, headless);
        setups::install(app, dev);
    }
    ride::install(app, dev, look);
    survey_script::install(app, dev);
    app.insert_resource(dev.clone())
        .insert_resource(facts)
        .init_resource::<stills::Shots>()
        .add_systems(Update, stills::exit_when_done);
}
