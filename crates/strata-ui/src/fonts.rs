//! The theme's two typefaces as handles, for UI nodes that are spawned by
//! hand rather than painted by a theme role: the floating combat text,
//! the hit marker and the game's prop labels.
//!
//! A raw `TextFont` with no font is Bevy's default face, which the player
//! must never see; loading the theme's own files here, once, gives every
//! such node the same Manrope and Barlow Condensed the screens use. The
//! asset server dedupes by path, so these are the very fonts the theme
//! loaded, not second copies.

use bevy::asset::{AssetServer, Handle};
use bevy::ecs::prelude::*;
use bevy::text::Font;

/// Manrope, the prose face, at the path the theme's `body` font names.
pub const BODY_PATH: &str = "fonts/manrope/Manrope-Variable.ttf";

/// Barlow Condensed in its semibold cut, the numeral and label face, at the path the
/// theme's `display` font names.
pub const DISPLAY_PATH: &str = "fonts/barlow-condensed/BarlowCondensed-SemiBold.ttf";

/// The two faces. Inserted at `Startup` when there is an asset server;
/// a system that draws text reads it as an `Option` and draws nothing
/// without it.
#[derive(Resource, Debug, Clone)]
pub struct UiFonts {
    /// Manrope: names and prose.
    pub body: Handle<Font>,
    /// Barlow Condensed, semibold: numerals, counters and uppercase labels.
    pub display: Handle<Font>,
}

/// `Startup`: loads both faces.
pub fn load(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else {
        return;
    };
    commands.insert_resource(UiFonts {
        body: assets.load(BODY_PATH),
        display: assets.load(DISPLAY_PATH),
    });
}

/// Registers the loader. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_systems(bevy::app::Startup, load);
}
