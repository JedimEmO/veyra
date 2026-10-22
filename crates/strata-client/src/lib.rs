//! The client: cameras, input, a scripted flight, the placeholder player,
//! and the numbers a HUD will show.
//!
//! Everything here is a view over the simulation. The local player is a
//! sim entity: input becomes a `PlayerIntent` ([`input`]), the sim moves
//! the player, and the client draws it where the last two ticks say it is
//! ([`visual`]) with an orbit camera behind it ([`orbit`]). The player is
//! the `PrimaryObserver`, so the streamer and the render origin follow it,
//! and the camera is a plain render-space entity placed after the origin
//! sync (ADR 0002). A connected client also draws the players the server
//! sends it ([`net`]), which need no network code here to be recognised.
//!
//! The flycam stays for flights and debugging: a client-side entity that
//! does not go through the sim's authority (PLAN section 4.1) but moves in
//! world space, writing its `WorldPosition` in `f64` and, being the
//! observer when there is no player, pulling the world along.

#[cfg(feature = "dev")]
pub mod debug;
pub mod flight;
pub mod flycam;
#[cfg(feature = "ui")]
pub mod hud;
pub mod input;
pub mod net;
pub mod orbit;
#[cfg(any(feature = "dev", feature = "ui"))]
pub mod stats;
#[cfg(feature = "ui")]
pub mod ui;
pub mod view;
pub mod visual;

use bevy::app::{App, Plugin, PostUpdate, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::state::condition::in_state;
use bevy::transform::TransformSystems;
use strata_stream::StreamSet;
use veyra_sim::{AppState, OriginSet};

pub use flight::FlightPath;
pub use flycam::FlyCam;
pub use orbit::OrbitCamera;
pub use visual::PlayerVisual;

pub use strata_app::ClientSet;

/// Registers the client sets, the cameras, the input, the player visuals
/// (local and remote) and the flight systems; with the `ui` feature the
/// slotted UI, the HUD layers and the frame statistics they show; with the
/// `dev` feature the statistics log and the debug keys.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        // The steering systems: the keyboard and the mouse into the
        // cameras and the player's intent. With the UI on they stand down
        // while a screen is open, and the menu has the keys.
        let steer = (
            flycam::grab_cursor,
            flycam::look,
            flycam::adjust_speed,
            flycam::fly,
            orbit::look,
            orbit::zoom,
            // After the camera turned, so the intent's yaw is this
            // frame's.
            input::write_player_intent,
        )
            .chain();
        #[cfg(feature = "ui")]
        let steer = steer.run_if(strata_ui::game_input_allowed);
        app.configure_sets(
            Update,
            (ClientSet::Camera, ClientSet::Hud)
                .chain()
                .after(StreamSet::Diff)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(
            Update,
            (
                steer,
                // After the keyboard, so a scripted path overrides it.
                flight::fly_path,
                visual::spawn_player_visuals,
                // After the local visuals, so the two never race over one
                // player.
                net::spawn_remote_player_visuals,
            )
                .chain()
                .in_set(ClientSet::Camera),
        )
        .configure_sets(
            PostUpdate,
            ClientSet::Follow
                .after(OriginSet::Sync)
                .before(TransformSystems::Propagate)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(
            PostUpdate,
            (orbit::follow, visual::place_player_visuals).in_set(ClientSet::Follow),
        );
        view::install_ride(app);
        #[cfg(any(feature = "dev", feature = "ui"))]
        {
            app.init_resource::<stats::FrameStats>().add_systems(
                Update,
                (stats::log_frame_stats, stats::record_bench)
                    .chain()
                    .in_set(ClientSet::Hud),
            );
            // GPU timestamps per render pass, for the statistics line: the
            // wall clock says nothing about the GPU once vsync caps it.
            // Bevy only adds this plugin itself under its Tracy feature.
            if app.get_sub_app(bevy::render::RenderApp).is_some() {
                app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
            }
            stats::WorkClock::register(app);
        }
        #[cfg(feature = "ui")]
        if ui::register(app) {
            hud::register(app);
            app.add_systems(
                Update,
                (hud::toggle_hud, hud::refresh_hud)
                    .chain()
                    .after(stats::log_frame_stats)
                    .in_set(ClientSet::Hud),
            );
        }
        #[cfg(feature = "dev")]
        {
            debug::register(app);
            app.add_systems(
                Update,
                (debug::toggle_wireframe, debug::scrub_time_of_day).in_set(ClientSet::Hud),
            );
        }
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn builds_over_the_streamer() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::input::InputPlugin,
            veyra_sim::SimPlugin,
            strata_stream::StreamPlugin,
            ClientPlugin,
        ));
        app.update();
    }
}
