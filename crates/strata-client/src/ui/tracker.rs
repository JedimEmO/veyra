//! The tracked deposit's marker: a small diamond over the world where the
//! deposit the household tracks lies, with its name and how far it is
//! (ticket 0007, step 3).
//!
//! What is tracked is the sim's (`strata_sim::deposits::Surveyed`), set
//! from the survey register and saved with the household; this only draws
//! it. Like a nameplate the marker keeps its anchor in the world, a little
//! over the survey post's lit top, and is projected through this frame's
//! camera in `PostUpdate`. A deposit is seldom in view, so a marker whose
//! place is off the screen or behind the camera is held just inside the
//! screen's edge on the side the deposit lies ([`pin`]): the marker always
//! says which way to turn. Its words stand on a dark plate, which is what
//! keeps them read over bright ground and sky.
//!
//! It hides with the rest of the world's UI: under a screen, and while the
//! local player is down. It also stands down at the deposit itself, where
//! the place card names the place ([`arrived`]). Its look is theme roles painted through
//! the combat HUD's `Painted`, as a nameplate's is.

use bevy::prelude::*;
use bevy::ui::{UiScale, UiSystems, UiTransform, Val2};
use slotted::ui::ScreenStack;
use strata_core::WorldPos;
use strata_sim::combat::Combatant;
use strata_sim::deposits::{NearDeposits, Surveyed};
use strata_sim::{LocalPlayer, Observers, Registries, RenderOrigin, WorldPosition};

use crate::ClientSet;
use crate::combat::CombatCamera;
use crate::combat_hud::{Painted, is_down, ui_point, ui_scale, world_ui_shown};
use crate::ui::fonts::UiFonts;

/// The name of the marker's root node.
pub const MARKER_NAME: &str = "hud.tracker";
/// How far inside the screen's edge a held marker stands, UI units.
pub const INSET: f32 = 48.0;
/// How far inside the left and right edges its centre is kept, UI units:
/// half the widest its plate gets, so a long name is not cut by the edge.
const SIDE_INSET: f32 = 120.0;
/// How far over the marker's base height it is anchored, metres, so it
/// points at the place and does not cover it.
const CLEAR_M: f64 = 1.5;
/// How high over the deposit's ground the marker stands, metres, now that no
/// post marks the place.
const POST_M: f64 = 12.0;
/// The depth of the marker: over the nameplates, which it must not be read
/// through when a hostile stands the way the deposit lies, and under the
/// HUD layers.
const MARKER_Z: i32 = -2;

/// Where the marker goes and whether it is held at the edge.
///
/// `projected` is the deposit's place on the screen in UI units, when the
/// camera can say (a point behind the camera has one too, mirrored through
/// the screen's centre, which `behind` says). A place inside the screen by
/// `inset` is where the marker goes. Any other is brought to the edge of
/// that inner rectangle along the line from the screen's centre, the way
/// the deposit lies. With no projection at all the marker is held at the
/// top centre.
#[must_use]
pub fn pin(projected: Option<Vec2>, behind: bool, window: Vec2, inset: f32) -> (Vec2, bool) {
    let centre = window * 0.5;
    let half = (centre - Vec2::splat(inset)).max(Vec2::ONE);
    let Some(at) = projected.filter(|at| at.is_finite()) else {
        return (Vec2::new(centre.x, inset), true);
    };
    let off = at - centre;
    if !behind && off.x.abs() <= half.x && off.y.abs() <= half.y {
        return (at, false);
    }
    // Behind the camera the projection is mirrored, and something straight
    // behind has no side: it is put at the bottom, where "turn round" reads.
    let mut way = if behind { -off } else { off };
    if way.length_squared() < 1e-6 {
        way = Vec2::Y;
    }
    if behind && way.y < 0.0 {
        way.y = -way.y;
    }
    let scale = (half.x / way.x.abs().max(1e-6)).min(half.y / way.y.abs().max(1e-6));
    (centre + way * scale, true)
}

/// Whether the player has arrived at the tracked deposit: within the
/// distance at which the HUD's place card names it (the dials' `place_m`).
/// There the card says where they are, at the top of the screen where a
/// marker held over a post overhead would lie across it, so the marker
/// stands down. Not arrived when either is unknown.
#[must_use]
pub fn arrived(metres: Option<f64>, place_m: Option<f64>) -> bool {
    metres
        .zip(place_m)
        .is_some_and(|(metres, within)| metres <= within)
}

/// The marker's words: `Calder flats · 1.2 km`.
#[must_use]
pub fn marker_words(place: &str, metres: f64) -> String {
    let (distance, km) = super::bench::distance_words(metres);
    super::strings::english(if km {
        "strata.tracker.km"
    } else {
        "strata.tracker.m"
    })
    .unwrap_or_default()
    .replace("{place}", place)
    .replace("{distance}", &distance)
}

/// The marker's root, and the text it writes.
#[derive(Component, Debug)]
pub struct TrackerMarker {
    /// The node that carries the words.
    pub text: Entity,
    /// Whether it was last held at the edge.
    pub held: bool,
    /// Whether it is the lead marker (the strongest point of the last ping)
    /// and not the tracked deposit's.
    pub lead: bool,
}

/// The name of the lead marker's root node.
pub const LEAD_NAME: &str = "hud.lead";

/// The marker's words for the lead: `Strongest reading · 80 m`.
#[must_use]
pub fn lead_words(metres: f64) -> String {
    let (distance, km) = super::bench::distance_words(metres);
    super::strings::english(if km {
        "strata.tracker.lead.km"
    } else {
        "strata.tracker.lead.m"
    })
    .unwrap_or_default()
    .replace("{distance}", &distance)
}

/// `Update`: the two markers, made once the typefaces exist, hidden until
/// something is tracked or a ping has a lead.
fn spawn_marker(
    mut commands: Commands,
    fonts: Option<Res<UiFonts>>,
    markers: Query<(), With<TrackerMarker>>,
) {
    let Some(fonts) = fonts else {
        return;
    };
    if !markers.is_empty() {
        return;
    }
    for lead in [false, true] {
        let text = commands
            .spawn((
                Painted::new("strata.tracker.text"),
                Text::new(String::new()),
                // The theme's face before the first paint, so not one frame
                // shows Bevy's default.
                TextFont {
                    font: fonts.display.clone().into(),
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ))
            .id();
        let plate = commands
            .spawn((
                Painted::new("strata.tracker.plate"),
                Node {
                    padding: UiRect::axes(px(8.0), px(3.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .add_child(text)
            .id();
        // A square turned an eighth: the diamond, which neither typeface
        // has. The lead's is left upright, a square, so the two markers are
        // told apart by shape and not by position alone.
        let mut diamond = commands.spawn((
            Painted::new("strata.tracker.pin"),
            Node {
                width: px(10.0),
                height: px(10.0),
                border: UiRect::all(px(1.0)),
                margin: UiRect::top(px(5.0)),
                ..default()
            },
            Pickable::IGNORE,
        ));
        if !lead {
            diamond.insert(UiTransform::from_rotation(Rot2::degrees(45.0)));
        }
        let diamond = diamond.id();
        commands
            .spawn((
                Name::new(if lead { LEAD_NAME } else { MARKER_NAME }),
                TrackerMarker {
                    text,
                    held: false,
                    lead,
                },
                Node {
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                // Its bottom centre on the anchor while the place is in view.
                UiTransform::from_translation(Val2::percent(-50.0, -100.0)),
                GlobalZIndex(MARKER_Z),
                Pickable::IGNORE,
                // Placed before it is first drawn.
                Visibility::Hidden,
            ))
            .add_children(&[plate, diamond]);
    }
}

/// Where a marker points and what it says.
struct Target {
    anchor: WorldPos,
    words: String,
}

/// `PostUpdate`: the markers follow what the sim tracks and what the last
/// ping led to. Each is hidden with nothing to point at and whenever the
/// world's UI is; otherwise at its place on the screen, or held at the edge
/// towards it. The tracked one says its name and how far the player is from
/// it, the lead how far the strongest reading lies, and goes when the
/// household stands within the dials' `lead_clear_m` of it.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "what is tracked, where the player is, the camera, the gate and the marker's nodes"
)]
fn place_marker(
    surveyed: Option<Res<Surveyed>>,
    near: Option<Res<NearDeposits>>,
    registries: Option<Res<Registries>>,
    origin: Option<Res<RenderOrigin>>,
    observers: Option<Res<Observers>>,
    stack: Option<Res<ScreenStack>>,
    ui_scale_res: Option<Res<UiScale>>,
    windows: Query<&Window>,
    players: Query<(&WorldPosition, Option<&Combatant>), With<LocalPlayer>>,
    cameras: Query<(&Camera, &Transform), With<CombatCamera>>,
    mut markers: Query<(
        &mut TrackerMarker,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
    )>,
    mut texts: Query<&mut Text>,
) {
    let player = players.iter().next();
    let allowed = world_ui_shown(stack, is_down(player.and_then(|(_, actor)| actor)));
    let from = player
        .map(|(position, _)| position.0)
        .or_else(|| observers.as_deref().and_then(|observers| observers.primary));
    let rules = registries
        .as_deref()
        .map(|registries| registries.0.resources.rules().place.place_m)
        .zip(
            registries
                .as_deref()
                .map(|registries| registries.0.resources.survey().ping.lead_clear_m),
        );
    let (place_m, lead_clear_m) = (rules.map(|r| r.0), rules.map(|r| r.1));
    let tracked = surveyed
        .as_deref()
        .and_then(|surveyed| surveyed.tracked.as_ref())
        .map(|deposit| {
            let metres = from.map_or(0.0, |from| deposit.distance_m(from.x(), from.z()));
            (deposit, metres)
        })
        .filter(|(_, metres)| allowed && !arrived(from.map(|_| *metres), place_m))
        .map(|(deposit, metres)| Target {
            anchor: WorldPos::new(
                deposit.at[0],
                f64::from(deposit.height_m) + POST_M + CLEAR_M,
                deposit.at[1],
            ),
            words: marker_words(&deposit.place, metres),
        });
    let lead = surveyed
        .as_deref()
        .and_then(|surveyed| surveyed.lead)
        .zip(from)
        .map(|(lead, from)| {
            let metres = (lead.at[0] - from.x()).hypot(lead.at[1] - from.z());
            (lead, metres, from)
        })
        .filter(|(_, metres, _)| allowed && !arrived(Some(*metres), lead_clear_m))
        .map(|(lead, metres, from)| {
            // The ground's height is the deposit's centre when it is near
            // enough to be listed, else the player's own.
            let ground = near
                .as_deref()
                .and_then(|near| near.get(lead.deposit))
                .map_or(from.y(), |deposit| f64::from(deposit.height_m));
            Target {
                anchor: WorldPos::new(lead.at[0], ground + CLEAR_M, lead.at[1]),
                words: lead_words(metres),
            }
        });
    let scale = ui_scale(ui_scale_res.as_deref());
    let camera = cameras
        .iter()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, transform)| (camera, GlobalTransform::from(*transform)));
    // The window in UI units: the camera's own viewport when there is one,
    // else the window's, which a screen with no world camera still has.
    let window = camera
        .and_then(|(camera, _)| camera.logical_viewport_size())
        .or_else(|| windows.iter().next().map(Window::size))
        .map_or(Vec2::new(1280.0, 720.0), |size| ui_point(size, scale));
    for (mut marker, mut node, mut transform, mut visibility) in &mut markers {
        let target = if marker.lead { &lead } else { &tracked };
        let wanted = if target.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let Some(target) = target else {
            continue;
        };
        let projected = camera
            .zip(origin.as_deref())
            .and_then(|((camera, view), origin)| {
                let render = origin.to_render(target.anchor);
                let behind = view.affine().inverse().transform_point3(render).z >= 0.0;
                let ndc = camera.world_to_ndc(&view, render)?;
                let size = camera.logical_viewport_size()?;
                let screen = Vec2::new(
                    f32::midpoint(ndc.x, 1.0) * size.x,
                    f32::midpoint(-ndc.y, 1.0) * size.y,
                );
                Some((ui_point(screen, scale), behind))
            });
        let (mut at, held) = pin(
            projected.map(|(at, _)| at),
            projected.is_some_and(|(_, behind)| behind),
            window,
            INSET,
        );
        let side = SIDE_INSET.min(window.x * 0.5);
        at.x = at.x.clamp(side, (window.x - side).max(side));
        if node.left != px(at.x) {
            node.left = px(at.x);
        }
        if node.top != px(at.y) {
            node.top = px(at.y);
        }
        // In view it stands on its place; held, it is centred on the edge's
        // point so no side of the screen cuts it.
        let anchored = Val2::percent(-50.0, if held { -50.0 } else { -100.0 });
        if transform.translation != anchored {
            transform.translation = anchored;
        }
        if marker.held != held {
            marker.held = held;
        }
        if let Ok(mut text) = texts.get_mut(marker.text)
            && text.0 != target.words
        {
            text.0.clone_from(&target.words);
        }
    }
}

/// Registers the marker's systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, spawn_marker).add_systems(
        PostUpdate,
        place_marker
            .after(ClientSet::Follow)
            .before(UiSystems::Prepare),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Vec2 = Vec2::new(1600.0, 900.0);

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 0.01
    }

    /// At the tracked deposit the place card names the place and the
    /// marker stands down; a step outside the card's distance it is back.
    #[test]
    fn the_marker_stands_down_at_the_deposit() {
        assert!(arrived(Some(2.0), Some(30.0)));
        assert!(arrived(Some(30.0), Some(30.0)));
        assert!(!arrived(Some(30.5), Some(30.0)));
        assert!(!arrived(None, Some(30.0)), "nobody to have arrived");
        assert!(!arrived(Some(2.0), None), "no dial, no card");
    }

    #[test]
    fn a_pin_in_view_stands_on_its_place() {
        let at = Vec2::new(900.0, 300.0);
        assert_eq!(pin(Some(at), false, WINDOW, INSET), (at, false));
        // Right up to the inner edge it is still its own place.
        let edge = Vec2::new(WINDOW.x - INSET, 450.0);
        assert_eq!(pin(Some(edge), false, WINDOW, INSET), (edge, false));
    }

    #[test]
    fn a_pin_off_screen_is_held_at_the_edge() {
        // Far to the right at the screen's own height: the right edge.
        let (at, held) = pin(Some(Vec2::new(5000.0, 450.0)), false, WINDOW, INSET);
        assert!(held);
        assert!(close(at, Vec2::new(WINDOW.x - INSET, 450.0)), "{at}");
        // Up and to the left: on the line from the centre, inside both
        // edges, on one of them.
        let (at, held) = pin(Some(Vec2::new(-800.0, -450.0)), false, WINDOW, INSET);
        assert!(held);
        assert!(at.x >= INSET - 0.01 && at.y >= INSET - 0.01, "{at}");
        assert!(
            (at.x - INSET).abs() < 0.01 || (at.y - INSET).abs() < 0.01,
            "{at}"
        );
        let centre = WINDOW * 0.5;
        let (way, was) = (at - centre, Vec2::new(-800.0, -450.0) - centre);
        assert!(way.perp_dot(was).abs() < 0.5, "it left the line: {at}");

        // Behind the camera the projection is mirrored: a point that
        // projects to the left lies to the right, and is held there, low.
        let (at, held) = pin(Some(Vec2::new(200.0, 450.0)), true, WINDOW, INSET);
        assert!(held);
        assert!(at.x > centre.x, "{at}");
        let (at, held) = pin(Some(Vec2::new(1300.0, 800.0)), true, WINDOW, INSET);
        assert!(held);
        assert!(at.x < centre.x && at.y >= centre.y, "{at}");
        // Straight behind, and a place the camera cannot project at all.
        let (at, held) = pin(Some(centre), true, WINDOW, INSET);
        assert!(held);
        assert!(close(at, Vec2::new(centre.x, WINDOW.y - INSET)), "{at}");
        assert_eq!(
            pin(None, false, WINDOW, INSET),
            (Vec2::new(centre.x, INSET), true)
        );
        assert_eq!(
            pin(Some(Vec2::NAN), false, WINDOW, INSET),
            (Vec2::new(centre.x, INSET), true)
        );
    }

    #[test]
    fn the_lead_says_how_far_the_strongest_reading_lies() {
        assert_eq!(lead_words(80.4), "Strongest reading · 80 m");
        assert_eq!(lead_words(1234.0), "Strongest reading · 1.2 km");
    }

    #[test]
    fn the_words_are_the_place_and_how_far() {
        assert_eq!(
            marker_words("Calder flats", 1234.0),
            "Calder flats · 1.2 km"
        );
        assert_eq!(marker_words("Rake edge", 513.0), "Rake edge · 510 m");
        assert_eq!(marker_words("Rake edge", 42.4), "Rake edge · 42 m");
    }
}
