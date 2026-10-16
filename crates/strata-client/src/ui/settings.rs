//! The settings: a spec `slotted-menu` turns into a screen, a store and a
//! file, and the systems that make the stored values true in the world.
//!
//! The spec is the whole declaration: every row is a control bound to a
//! key with a default and a rule, the crate generates the screen over its
//! settings frame, seeds the value store from the file, and saves when a
//! change settles. What is left to the game is applying a value, and that
//! is done by reading the store rather than by answering a change message:
//! a camera spawned after the settings loaded, a second camera, or a file
//! edited by hand all end up right without a special case, and a value
//! that is already applied costs one comparison a frame.

use std::path::PathBuf;

use bevy::camera::Projection;
use bevy::ecs::prelude::*;
use slotted::menu::{SettingsRow, SettingsSpec};
use slotted::ui::hud::builtin;
use slotted::ui::{
    HudLayers, LocKey, ScreenKind, SelectOption, Tags, TextOpts, TextRole, ToggleStyle, UiNodeDef,
    Value, ValueStore,
};

use crate::flycam::FlyCam;
use crate::orbit::OrbitCamera;
use crate::ui::graphics::Graphics;

/// The generated settings screen's kind.
pub const KIND: &str = "strata:settings";

/// The value store keys the spec declares.
pub mod keys {
    /// Vertical field of view in degrees (`Float`).
    pub const FIELD_OF_VIEW: &str = "settings.fov";
    /// Foliage rendering and streaming distance as a percentage (`Float`).
    pub const FOLIAGE_DISTANCE: &str = "settings.foliage_distance";
    /// Show the game HUD, the reticle and the hotbar (`Bool`). The debug
    /// statistics are F3's, not this one's.
    pub const SHOW_HUD: &str = "settings.show_hud";
    /// Mouse look speed as a multiple of the cameras' base (`Float`).
    pub const MOUSE_SENSITIVITY: &str = "settings.mouse_sensitivity";
    /// Master volume in percent (`Float`): every sound, the fight's and the
    /// interface's.
    pub const MASTER_VOLUME: &str = "settings.master_volume";
    /// Interface volume in percent of the master (`Float`): the screens'
    /// own sounds only.
    pub const UI_VOLUME: &str = "settings.ui_volume";
    /// The UI scale (`Text`): one of [`crate::ui::scale::choice`]'s ids.
    pub const UI_SCALE: &str = "settings.ui_scale";
    /// The camera shakes a little when the member is hurt (`Bool`).
    pub const CAMERA_SHAKE: &str = "settings.camera_shake";
    /// The graphics preset (`Text`): one of [`crate::ui::graphics::Graphics`]'s ids.
    pub const GRAPHICS: &str = "settings.graphics";
}

/// The field of view the cameras start with: Bevy's own default, so the
/// world looks as it did before there was a setting.
pub const DEFAULT_FOV_DEGREES: f64 = 45.0;

/// Radians per pixel of mouse motion at a sensitivity of `1.0`: the
/// cameras' own default.
pub const BASE_SENSITIVITY: f32 = 0.002;

/// The file the settings persist to when the app named none. A
/// [`SettingsFile`] inserted before `ClientPlugin` wins.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct SettingsFile(pub PathBuf);

/// `<platform config dir>/sci-fi-scrolls/settings.ron`: `~/.config` on
/// Linux, `Application Support` on macOS, `AppData\Roaming` on Windows.
/// With no such directory (a bare container) the working directory.
#[must_use]
pub fn default_settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sci-fi-scrolls")
        .join("settings.ron")
}

/// The settings in the sim's installed [`strata_sim::store::Store`] under the
/// file's path as the key: the same RON file natively, the browser's storage
/// on the web, and nothing when no store can keep it.
#[derive(Debug, Clone)]
pub struct StoredSettings {
    key: String,
}

impl StoredSettings {
    /// Settings kept under `path`.
    #[must_use]
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            key: path.to_string_lossy().into_owned(),
        }
    }
}

impl slotted::menu::SettingsStore for StoredSettings {
    fn load(&self) -> Option<slotted::menu::SavedSettings> {
        let text = match strata_sim::store::current().read(&self.key) {
            Ok(Some(text)) => text,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(key = %self.key, %e, "reading the settings");
                return None;
            }
        };
        match ron::from_str(&text) {
            Ok(loaded) => Some(loaded),
            Err(e) => {
                tracing::warn!(key = %self.key, %e, "the settings are not RON");
                None
            }
        }
    }

    fn save(&self, settings: &slotted::menu::SavedSettings) {
        let text = match ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default()) {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(%e, "serialising the settings");
                return;
            }
        };
        if let Err(e) = strata_sim::store::current().write(&self.key, &text) {
            tracing::warn!(key = %self.key, %e, "writing the settings");
        }
    }
}

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

fn key(s: &str) -> LocKey {
    LocKey(s.to_owned())
}

/// The spec: display, controls and audio.
#[must_use]
pub fn spec() -> SettingsSpec {
    SettingsSpec::new(kind())
        .tab("display", "strata.settings.display")
        .row(SettingsRow::Select {
            key: keys::GRAPHICS.into(),
            label: key("strata.settings.graphics"),
            options: graphics_options(),
            default: Graphics::High.id().into(),
        })
        .row(SettingsRow::Slider {
            key: keys::FIELD_OF_VIEW.into(),
            label: key("strata.settings.fov"),
            min: 40.0,
            max: 100.0,
            step: 1.0,
            default: DEFAULT_FOV_DEGREES,
            format: "{value} deg".into(),
        })
        .row(SettingsRow::Toggle {
            key: keys::SHOW_HUD.into(),
            label: key("strata.settings.show_hud"),
            default: true,
            style: ToggleStyle::Switch,
        })
        .row(SettingsRow::Slider {
            key: keys::FOLIAGE_DISTANCE.into(),
            label: key("strata.settings.foliage_distance"),
            min: 25.0,
            max: 200.0,
            step: 5.0,
            default: 100.0,
            format: "{value}%".into(),
        })
        .row(SettingsRow::Select {
            key: keys::UI_SCALE.into(),
            label: key("strata.settings.ui_scale"),
            options: ui_scale_options(),
            default: crate::ui::scale::choice::AUTO.into(),
        })
        .row(SettingsRow::Toggle {
            key: keys::CAMERA_SHAKE.into(),
            label: key("strata.settings.camera_shake"),
            default: true,
            style: ToggleStyle::Switch,
        })
        .tab("controls", "strata.settings.controls")
        .row(SettingsRow::Slider {
            key: keys::MOUSE_SENSITIVITY.into(),
            label: key("strata.settings.mouse_sensitivity"),
            min: 0.2,
            max: 3.0,
            step: 0.1,
            default: 1.0,
            format: "{value:.1}x".into(),
        })
        .tab("audio", "strata.settings.audio")
        .row(SettingsRow::Slider {
            key: keys::MASTER_VOLUME.into(),
            label: key("strata.settings.master_volume"),
            min: 0.0,
            max: 100.0,
            step: 5.0,
            default: 80.0,
            format: "{value}%".into(),
        })
        .row(SettingsRow::Slider {
            key: keys::UI_VOLUME.into(),
            label: key("strata.settings.ui_volume"),
            min: 0.0,
            max: 100.0,
            step: 5.0,
            default: 80.0,
            format: "{value}%".into(),
        })
        .row(SettingsRow::Custom(UiNodeDef::Text {
            key: key("strata.settings.audio_note"),
            style: TextRole::Caption,
            opts: TextOpts::default(),
            tags: Tags::new().with(Tags::TEST_ID, "audio_note"),
        }))
}

/// The graphics presets, Low first.
fn graphics_options() -> Vec<SelectOption> {
    Graphics::ALL
        .into_iter()
        .map(|graphics| SelectOption {
            id: graphics.id().to_owned(),
            label: key(&format!("strata.settings.graphics.{}", graphics.id())),
        })
        .collect()
}

/// The UI scale's choices, Auto first: the one that is right on any
/// window. The ids are what the file stores.
fn ui_scale_options() -> Vec<SelectOption> {
    use crate::ui::scale::choice;
    [
        (choice::AUTO, "strata.settings.ui_scale.auto"),
        (choice::P100, "strata.settings.ui_scale.100"),
        (choice::P125, "strata.settings.ui_scale.125"),
        (choice::P150, "strata.settings.ui_scale.150"),
        (choice::P200, "strata.settings.ui_scale.200"),
    ]
    .into_iter()
    .map(|(id, label)| SelectOption {
        id: id.to_owned(),
        label: key(label),
    })
    .collect()
}

/// `Update`: the stored field of view onto every game camera's projection.
/// Only a perspective projection has one, and only a change is written, so
/// change detection on the projection fires when the setting moves and not
/// every frame.
pub fn apply_field_of_view(
    store: Res<ValueStore>,
    mut cameras: Query<
        (&mut Projection, Option<&mut crate::combat::CombatCamera>),
        Or<(With<OrbitCamera>, With<FlyCam>)>,
    >,
) {
    let Some(degrees) = store.get(keys::FIELD_OF_VIEW).and_then(Value::as_f64) else {
        return;
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a field of view in degrees fits an f32 exactly enough"
    )]
    let fov = (degrees as f32).to_radians();
    for (mut projection, combat) in &mut cameras {
        if let Some(mut combat) = combat {
            combat.base_fov = fov;
            continue;
        }
        if let Projection::Perspective(perspective) = projection.as_ref()
            && (perspective.fov - fov).abs() < 1e-6
        {
            continue;
        }
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = fov;
        }
    }
}

/// `Update`: the stored multiplier onto both cameras' sensitivity.
pub fn apply_mouse_sensitivity(
    store: Res<ValueStore>,
    mut orbits: Query<&mut OrbitCamera>,
    mut flycams: Query<&mut FlyCam>,
) {
    let Some(factor) = store.get(keys::MOUSE_SENSITIVITY).and_then(Value::as_f64) else {
        return;
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a slider value between 0.2 and 3 fits an f32"
    )]
    let wanted = BASE_SENSITIVITY * factor as f32;
    for mut cam in &mut orbits {
        if (cam.as_ref().sensitivity - wanted).abs() > 1e-9 {
            cam.sensitivity = wanted;
        }
    }
    for mut cam in &mut flycams {
        if (cam.as_ref().sensitivity - wanted).abs() > 1e-9 {
            cam.sensitivity = wanted;
        }
    }
}

/// `Update`: the HUD toggle onto the reticle and the hotbar layers. The
/// debug statistics layer is left to F3.
pub fn apply_hud_toggle(store: Res<ValueStore>, mut layers: ResMut<HudLayers>) {
    let Some(show) = store.get(keys::SHOW_HUD).and_then(Value::as_bool) else {
        return;
    };
    for id in [builtin::CROSSHAIR, builtin::HOTBAR] {
        if layers.get(&id).is_some_and(|def| def.visible != show) {
            layers.set_visible(&id, show);
        }
    }
}

/// `Update`: the Camera shake switch onto the camera's own resource.
pub fn apply_camera_shake(
    store: Res<ValueStore>,
    shake: Option<ResMut<crate::combat::CameraShake>>,
) {
    let (Some(on), Some(mut shake)) = (
        store.get(keys::CAMERA_SHAKE).and_then(Value::as_bool),
        shake,
    ) else {
        return;
    };
    if shake.0 != on {
        shake.0 = on;
    }
}

/// Apply distance changes before streaming, including already resident grass.
/// Far-tree batches spawned after the change fade out at the new distance.
/// Scale from the previous value so app-provided distance presets are preserved.
#[allow(
    clippy::cast_possible_truncation,
    reason = "bounded percentage fits f32"
)]
pub fn apply_foliage_distance(
    store: Res<ValueStore>,
    graphics: Res<crate::ui::graphics::Active>,
    config: Option<ResMut<strata_foliage::FoliageConfig>>,
    mut previous: Local<Option<f32>>,
    mut ranges: Query<
        &mut bevy::camera::visibility::VisibilityRange,
        With<bevy::pbr::MeshMaterial3d<strata_foliage::GrassMaterial>>,
    >,
) {
    let Some(mut config) = config else { return };
    let Some(percent) = store.get(keys::FOLIAGE_DISTANCE).and_then(Value::as_f64) else {
        return;
    };
    if !percent.is_finite() {
        return;
    }
    let scale = percent.clamp(25.0, 200.0) as f32 / 100.0 * graphics.preset.foliage_scale();
    let old = previous.unwrap_or(1.0);
    if (scale - old).abs() < f32::EPSILON {
        return;
    }
    let ratio = scale / old;
    let config = &mut *config;
    for pair in [&mut config.near_fade, &mut config.far_fade] {
        pair.0 *= ratio;
        pair.1 *= ratio;
    }
    // A tree's mesh levels are baked bands its impostor takes over from, so
    // the distance setting moves only where far trees fade out.
    config.tree_fades.far.0 *= ratio;
    config.tree_fades.far.1 *= ratio;
    for mut range in &mut ranges {
        range.start_margin.start *= ratio;
        range.start_margin.end *= ratio;
        range.end_margin.start *= ratio;
        range.end_margin.end *= ratio;
    }
    *previous = Some(scale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_declares_the_nine_keys_with_their_defaults() {
        let defaults = spec().defaults();
        assert_eq!(
            defaults.get(keys::FIELD_OF_VIEW),
            Some(&Value::Float(DEFAULT_FOV_DEGREES))
        );
        assert_eq!(defaults.get(keys::SHOW_HUD), Some(&Value::Bool(true)));
        assert_eq!(
            defaults.get(keys::MOUSE_SENSITIVITY),
            Some(&Value::Float(1.0))
        );
        assert_eq!(defaults.get(keys::MASTER_VOLUME), Some(&Value::Float(80.0)));
        assert_eq!(defaults.get(keys::UI_VOLUME), Some(&Value::Float(80.0)));
        assert_eq!(
            defaults.get(keys::FOLIAGE_DISTANCE),
            Some(&Value::Float(100.0))
        );
        assert_eq!(
            defaults.get(keys::UI_SCALE),
            Some(&Value::Text(crate::ui::scale::choice::AUTO.to_owned()))
        );
        assert_eq!(
            defaults.get(keys::CAMERA_SHAKE),
            Some(&Value::Bool(true)),
            "the shake is on until the member turns it off"
        );
        assert_eq!(
            defaults.get(keys::GRAPHICS),
            Some(&Value::Text("high".to_owned()))
        );
        assert_eq!(defaults.len(), 9);
    }

    #[test]
    fn the_default_path_ends_in_the_game_directory() {
        let path = default_settings_path();
        assert!(path.ends_with("sci-fi-scrolls/settings.ron"), "{path:?}");
    }
}
