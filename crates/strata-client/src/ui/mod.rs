//! The engine client's slotted UI: the settings and graphics screens, the
//! look panel, and the wiring that makes a stored setting true in the world.
//!
//! The screen framework (the `SimScreen` kit, the pause flow, the scale, the
//! typefaces) is `strata-ui`, re-exported here under its old paths; the
//! game's screens and their words are `veyra-ui`. What is left in this module
//! is what has to see the cameras, the terrain, the atmosphere and the
//! foliage: [`settings`] applies the field of view, the mouse, the HUD
//! switch, the camera shake, the interface scale and the foliage distance,
//! [`graphics`] is the preset and what it turns on, and [`render`] is the
//! look panel and the look wiring into terrain, foliage, water and probes.
//!
//! The simulation does not pause. This is an MMO: the world goes on for
//! everyone else while one player reads a menu, so the pause screen is a
//! modal over a running world and only the local input stops.
//!
//! Nothing here runs without Bevy's UI plugins. A headless app that adds
//! `ClientPlugin` over `MinimalPlugins` (the facade's smoke test) gets no
//! screens and no HUD, which is what `register` reports.

pub mod graphics;
pub mod render;
pub mod settings;

use bevy::app::{App, Startup, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use slotted::menu::{MenuConfig, Settings, SettingsStorage};
use slotted::ui::{LocKey, Presentation, PresentationMode, Screens};

pub use settings::{SettingsFile, default_settings_path};
pub use strata_ui::{
    QUIT_CONFIRM, ScreenWork, fonts, game_input_allowed, open_pause, painted, pause, scale,
    screen_on_top, screen_open, screen_up, sim_screen, world_ui,
};

pub(crate) fn register(app: &mut App) -> bool {
    if !strata_ui::register(app) {
        return false;
    }
    if !app.world().contains_resource::<SettingsStorage>() {
        let path = app
            .world()
            .get_resource::<SettingsFile>()
            .map_or_else(default_settings_path, |file| file.0.clone());
        tracing::info!(path = %path.display(), "settings file");
        app.insert_resource(SettingsStorage::new(settings::StoredSettings::new(&path)));
    }
    graphics::register(app);
    app.insert_resource(MenuConfig {
        settings_kind: settings::kind(),
        title: LocKey(strata_ui::TITLE.to_owned()),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        ..MenuConfig::default()
    })
    .insert_resource(Settings {
        spec: settings::spec(),
    })
    .add_systems(Startup, (register_settings_screen, render::install_strings))
    .add_systems(
        Update,
        (
            settings::apply_field_of_view,
            settings::apply_mouse_sensitivity,
            settings::apply_hud_toggle,
            settings::apply_camera_shake,
            settings::apply_ui_scale,
            settings::apply_foliage_distance
                .after(graphics::resolve)
                .before(strata_foliage::FoliageSet::Dispatch),
            graphics::apply_shadow_map,
            graphics::apply_fog_and_detail.after(graphics::resolve),
        ),
    );
    render::register(app);
    true
}

/// `Startup`: the settings screen registered as a page rather than the
/// modal its template is. slotted generates the screen itself in
/// `PostStartup` over the modal template and keeps the template's mode,
/// and a modal over the pause modal leaves the pause readable through the
/// glass panel, its buttons across the rows. A page hides everything
/// under it and Back brings the pause back. Registered first, so slotted
/// finds the kind taken and generates nothing; the scrim is restated
/// because the merge with the template keeps only what is written.
fn register_settings_screen(mut screens: ResMut<Screens>, settings: Res<Settings>) {
    let mut def = settings.spec.screen_def();
    def.presentation = Presentation {
        mode: PresentationMode::Page,
        scrim: Some(true),
        ..Presentation::default()
    };
    screens.register(def);
}
