//! Debug keys: `F4` for wireframe, `[` and `]` to scrub the time of day.
//!
//! Wireframe is Bevy's own (`bevy_pbr::wireframe`), toggled globally, and
//! needs the app to have asked wgpu for `POLYGON_MODE_LINE`; without it
//! the plugin logs and the key does nothing. The plugin is only added when
//! the app inserts [`WireframeWanted`] first: its per-entity specialization
//! check walks every mesh entity every frame whether or not F4 was ever
//! pressed, 0.5 to 0.7 ms of main-thread CPU over the woodland's foliage
//! (`docs/perf/profile-2026-10-03.md`, section 4.1), and Bevy cannot add a
//! render plugin once the app runs. The time scrub moves the
//! sim's `DayStart`, which is sim state rather than a camera setting: the
//! sun, the sky and every client agree on the result.

use bevy::app::App;
use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::time::Time;
use veyra_sim::DayStart;

/// Toggles the wireframe.
pub const WIREFRAME_KEY: KeyCode = KeyCode::F4;

/// Turns the day back.
pub const TIME_BACK_KEY: KeyCode = KeyCode::BracketLeft;

/// Turns the day forward.
pub const TIME_FORWARD_KEY: KeyCode = KeyCode::BracketRight;

/// Day fraction per second while a bracket is held: a full day in ten
/// seconds, slow enough to stop where wanted.
pub const SCRUB_PER_SECOND: f64 = 0.1;

/// Insert before the client plugin builds to have `F4` draw wireframes.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct WireframeWanted;

/// Registers the wireframe plugin when the app asked for it
/// ([`WireframeWanted`]) and has an asset server (the plugin embeds a
/// shader and needs one).
pub(crate) fn register(app: &mut App) {
    if app.world().contains_resource::<WireframeWanted>()
        && app.world().contains_resource::<bevy::asset::AssetServer>()
        && !app.is_plugin_added::<WireframePlugin>()
    {
        app.add_plugins(WireframePlugin::default());
    }
}

/// `F4` flips the global wireframe, or says why it cannot.
pub fn toggle_wireframe(keys: Res<ButtonInput<KeyCode>>, config: Option<ResMut<WireframeConfig>>) {
    if !keys.just_pressed(WIREFRAME_KEY) {
        return;
    }
    if let Some(mut config) = config {
        config.global = !config.global;
    } else {
        tracing::info!("F4: no wireframe in this run; start the game with --wireframe");
    }
}

/// `[` and `]` move the start of the day while held.
pub fn scrub_time_of_day(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut start: ResMut<DayStart>,
) {
    let mut direction = 0.0;
    if keys.pressed(TIME_BACK_KEY) {
        direction -= 1.0;
    }
    if keys.pressed(TIME_FORWARD_KEY) {
        direction += 1.0;
    }
    if direction == 0.0 {
        return;
    }
    start.0 =
        (start.0 + direction * SCRUB_PER_SECOND * f64::from(time.delta_secs())).rem_euclid(1.0);
}
