//! The UI scale: Bevy's [`UiScale`] from the window's height and the
//! player's choice.
//!
//! Every screen, layer and token was laid out on a 1600 by 900 window in
//! fixed pixels. On 1440p that is a small HUD in a corner and a sheet with
//! 12 px labels; on 4K it is unreadable. `UiScale` multiplies every `Node`
//! pixel on its way to the screen, so one number fixes all of it without
//! touching a layout. The automatic rule follows the window's logical
//! height (the OS's own display scale is already divided out of it) and
//! snaps to quarter steps, because a glyph rasterised at 1.6 times its
//! size lands between pixels and blurs where 1.5 stays crisp.
//!
//! Applied like the other settings, by reading the store every frame
//! rather than answering a change: a resize, a setting moved, or a file
//! edited by hand all come out right, and a scale already in force costs
//! one comparison.
//!
//! Anything that writes a `Node` position from something Bevy measured in
//! window pixels (a projected world point, the cursor) divides by the
//! scale first: slotted's `UiUnits` is that conversion.

use bevy::ecs::prelude::*;
use bevy::ui::{Node, UiScale, Val};
use bevy::window::{PrimaryWindow, Window};
use slotted::ui::hud::HudAnchored;
use slotted::ui::{HudAnchor, NineAnchor, Value, ValueStore};

use super::settings::keys;

/// The height every layout was designed at: one UI unit is one logical
/// pixel here.
pub const DESIGN_HEIGHT: f32 = 900.0;

/// The smallest automatic scale. A window below the design height keeps
/// the design's pixels rather than shrinking text under its legible size.
pub const MIN_AUTO: f32 = 1.0;

/// The largest automatic scale: past this the HUD crowds the world.
pub const MAX_AUTO: f32 = 2.0;

/// Scales snap to this, so text stays on whole pixels more often.
pub const STEP: f32 = 0.25;

/// The setting's option ids, as stored. `auto` follows the window.
pub mod choice {
    /// The rule in [`auto_scale`](super::auto_scale).
    pub const AUTO: &str = "auto";
    /// The design's own pixels.
    pub const P100: &str = "100";
    /// A quarter larger.
    pub const P125: &str = "125";
    /// Half again.
    pub const P150: &str = "150";
    /// Twice the design.
    pub const P200: &str = "200";
}

/// The automatic scale for a window `logical_height` pixels tall:
/// `height / 900`, clamped to [`MIN_AUTO`] and [`MAX_AUTO`], rounded to the
/// nearest [`STEP`]. 1080p is 1.25, 1440p 1.5, 4K 2.0.
#[must_use]
pub fn auto_scale(logical_height: f32) -> f32 {
    if !logical_height.is_finite() || logical_height <= 0.0 {
        return MIN_AUTO;
    }
    let raw = (logical_height / DESIGN_HEIGHT).clamp(MIN_AUTO, MAX_AUTO);
    (raw / STEP).round() * STEP
}

/// The scale a stored choice means for a window `logical_height` tall. An
/// unknown id (a file from a later build) falls back to the automatic rule.
#[must_use]
pub fn scale_for(choice: &str, logical_height: f32) -> f32 {
    match choice {
        choice::P100 => 1.0,
        choice::P125 => 1.25,
        choice::P150 => 1.5,
        choice::P200 => 2.0,
        _ => auto_scale(logical_height),
    }
}

/// `Update`: the stored choice and the primary window's height onto
/// [`UiScale`]. Written only when it differs, so the UI's change detection
/// fires on a resize or a new choice and not every frame. Without a
/// window (a headless app) there is nothing to scale.
pub fn apply_ui_scale(
    store: Res<ValueStore>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<ResMut<UiScale>>,
) {
    let (Some(mut scale), Ok(window)) = (scale, windows.single()) else {
        return;
    };
    let choice = store
        .get(keys::UI_SCALE)
        .and_then(Value::as_str)
        .unwrap_or(choice::AUTO);
    let wanted = scale_for(choice, window.height());
    if (scale.0 - wanted).abs() > f32::EPSILON {
        scale.0 = wanted;
    }
}

/// Where a HUD layer centred on an axis starts on it, in UI units: the
/// window's midpoint with the layer's offset. `None` for an axis the
/// anchor holds to an edge.
#[must_use]
pub fn centred(anchor: &HudAnchor, window: bevy::math::Vec2, scale: f32) -> [Option<f32>; 2] {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let mid = window * 0.5 / scale + anchor.offset;
    let across = matches!(
        anchor.anchor,
        NineAnchor::Top | NineAnchor::Center | NineAnchor::Bottom
    );
    let down = matches!(
        anchor.anchor,
        NineAnchor::Left | NineAnchor::Center | NineAnchor::Right
    );
    [across.then_some(mid.x), down.then_some(mid.y)]
}

/// `Update`, after slotted has anchored its HUD layers: a layer centred on
/// an axis is put at the window's midpoint in UI units.
///
/// Slotted writes that midpoint as half the window's logical pixels, which
/// [`UiScale`] then multiplies with every other `Node` pixel: at any scale
/// but one the layer stands off centre, three quarters of the way across
/// at 1440p. An edge anchor is an offset from the edge and scales rightly.
/// Slotted writes its own figure again whenever the node differs from it,
/// so this holds the right one every frame the scale is not one, and
/// writes nothing when it is.
pub fn centre_hud_layers(
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
    mut wrappers: Query<(&HudAnchor, &mut Node), With<HudAnchored>>,
) {
    let (Some(scale), Ok(window)) = (scale, windows.single()) else {
        return;
    };
    let size = bevy::math::Vec2::new(window.width(), window.height());
    for (anchor, mut node) in &mut wrappers {
        let [left, top] = centred(anchor, size, scale.0);
        if let Some(left) = left.map(Val::Px)
            && node.left != left
        {
            node.left = left;
        }
        if let Some(top) = top.map(Val::Px)
            && node.top != top
        {
            node.top = top;
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "every scale here is a sum of quarters, exact in an f32"
)]
mod tests {
    use super::*;

    #[test]
    fn the_automatic_rule_snaps_to_quarters_between_one_and_two() {
        assert_eq!(auto_scale(720.0), 1.0);
        assert_eq!(auto_scale(900.0), 1.0);
        assert_eq!(auto_scale(1080.0), 1.25);
        assert_eq!(auto_scale(1200.0), 1.25);
        assert_eq!(auto_scale(1440.0), 1.5);
        assert_eq!(auto_scale(1600.0), 1.75);
        assert_eq!(auto_scale(2160.0), 2.0);
        assert_eq!(auto_scale(4320.0), 2.0);
        assert_eq!(auto_scale(0.0), 1.0);
        assert_eq!(auto_scale(f32::NAN), 1.0);
    }

    /// A centred HUD layer starts at the window's midpoint in UI units,
    /// which the scale then puts at the middle of the screen; an edge
    /// anchor is left to its edge.
    #[test]
    fn a_centred_layer_is_centred_at_any_scale() {
        use bevy::math::Vec2;
        let window = Vec2::new(2560.0, 1440.0);
        let at = |anchor, offset| HudAnchor {
            anchor,
            offset,
            scale: 1.0,
        };
        let bottom = at(NineAnchor::Bottom, Vec2::new(0.0, -150.0));
        let [left, top] = centred(&bottom, window, 1.5);
        assert_eq!(top, None, "held to the bottom edge");
        // 853.33 units at a scale of 1.5 is pixel 1280, the middle.
        assert!((left.expect("centred across") * 1.5 - 1280.0).abs() < 0.01);
        assert_eq!(centred(&bottom, window, 1.0), [Some(1280.0), None]);
        let centre = at(NineAnchor::Center, Vec2::new(10.0, -20.0));
        assert_eq!(centred(&centre, window, 2.0), [Some(650.0), Some(340.0)]);
        let corner = at(NineAnchor::TopLeft, Vec2::new(16.0, 16.0));
        assert_eq!(centred(&corner, window, 1.5), [None, None]);
        assert_eq!(
            centred(&at(NineAnchor::Right, Vec2::ZERO), window, 1.5),
            [None, Some(480.0)]
        );
        // A scale that is no scale is taken as one.
        assert_eq!(centred(&bottom, window, 0.0), [Some(1280.0), None]);
    }

    #[test]
    fn a_fixed_choice_ignores_the_window() {
        assert_eq!(scale_for(choice::P100, 2160.0), 1.0);
        assert_eq!(scale_for(choice::P125, 720.0), 1.25);
        assert_eq!(scale_for(choice::P150, 1080.0), 1.5);
        assert_eq!(scale_for(choice::P200, 900.0), 2.0);
        assert_eq!(scale_for(choice::AUTO, 1440.0), 1.5);
        assert_eq!(scale_for("from a later build", 1440.0), 1.5);
    }
}
