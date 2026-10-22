//! What every world-anchored piece of UI shares, the combat view's and the
//! screens' alike: the scale a projected point is divided by, whether the
//! local player is down, and whether the world's UI may be drawn at all.
//! The engine owns it so a screen over the world (the tracker, the map's
//! marks) needs nothing from the game's combat view.

use bevy::prelude::*;
use slotted::ui::ScreenStack;

/// A point the camera projected, in logical window pixels, as UI units.
///
/// `Camera::world_to_viewport` answers in window pixels, but a node's
/// `left` and `top` are UI units that `UiScale` multiplies; without the
/// division a label at scale 1.5 drifts outward from the top-left corner
/// by half its distance from it. Sizes and offsets inside the node are UI
/// units already and need nothing.
#[must_use]
pub fn ui_point(screen: Vec2, scale: f32) -> Vec2 {
    if scale > 0.0 { screen / scale } else { screen }
}

/// The UI's scale, 1 without the resource.
#[must_use]
pub fn ui_scale(scale: Option<&bevy::ui::UiScale>) -> f32 {
    scale.map_or(1.0, |scale| scale.0)
}

/// Whether the world's UI is up: the nameplates, the floating numbers
/// over the world, the reticle and the controls card. It goes under a
/// screen like every HUD layer, and when the local player is down, so
/// the down card is the one thing over the world besides the vitals.
#[must_use]
pub fn world_ui_shown(stack: Option<Res<ScreenStack>>, down: bool) -> bool {
    !down && hud_shown(stack)
}

/// Whether the game has the screen: no page or modal is up.
pub fn hud_shown(stack: Option<Res<ScreenStack>>) -> bool {
    crate::game_input_allowed(stack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projected_point_is_divided_by_the_ui_scale() {
        let screen = Vec2::new(1280.0, 720.0);
        assert_eq!(ui_point(screen, 2.0), Vec2::new(640.0, 360.0));
        assert_eq!(ui_point(screen, 1.0), screen);
        assert_eq!(ui_point(screen, 0.0), screen, "a broken scale is ignored");
        assert!((ui_scale(None) - 1.0).abs() < f32::EPSILON);
        assert!((ui_scale(Some(&bevy::ui::UiScale(1.5))) - 1.5).abs() < f32::EPSILON);
    }
}
