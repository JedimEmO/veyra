//! The scoped view of a zoomed weapon: when it applies, the field of view
//! and the mouse's factor it brings, and (with the `ui` feature) the dark
//! surround drawn over the world. The sim decides whether the aim has
//! closed ([`Combatant::aim_share`]) and what the weapon's `zoom` is; this
//! module only turns those into a view.

use strata_sim::combat::{Action, Combatant};

/// The least `zoom` a weapon needs for its aimed view to be a scope. Below
/// it the ordinary aim view serves, so a carbine and a pistol behave as
/// they always did.
pub(crate) const MIN_ZOOM: f32 = 1.5;

/// The aim view's field of view as a share of the walking one.
const AIM_VIEW: f32 = 52. / 68.;

/// The magnification of the scope the actor looks through, if it is in one:
/// a zoomed weapon in a Ready hand with aim held and the aim closed all the
/// way. While the aim is still closing, or after any action that resets it,
/// there is none.
#[must_use]
pub(crate) fn scoped_zoom(actor: &Combatant) -> Option<f32> {
    let zoom = actor.weapon_spec().zoom;
    (zoom > MIN_ZOOM && actor.aiming && actor.action == Action::Ready && actor.aim_share() >= 1.)
        .then_some(zoom)
}

/// The vertical field of view the camera eases to.
#[must_use]
pub(crate) fn view_fov(base: f32, aiming: bool, scoped: Option<f32>) -> f32 {
    let aim_view = base * if aiming { AIM_VIEW } else { 1. };
    aim_view / scoped.unwrap_or(1.)
}

/// What the mouse's turn is multiplied by: the view is magnified, so the
/// same hand movement turns it that much less.
#[must_use]
pub(crate) fn look_factor(actor: &Combatant) -> f32 {
    scoped_zoom(actor).map_or(1., f32::recip)
}

#[cfg(feature = "ui")]
pub(crate) use overlay::install;

#[cfg(feature = "ui")]
mod overlay {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    use strata_sim::LocalPlayer;
    use strata_sim::combat::Combatant;

    use crate::ClientSet;

    /// The picture's side, pixels; it is drawn once and scaled to the window.
    const SIDE: u32 = 512;
    /// The round window's radius as a share of the picture's half side.
    const WINDOW: f32 = 0.92;
    /// The soft edge of the window, pixels.
    const EDGE: f32 = 2.0;
    /// The cross lines' half width, pixels, and the gap left at the centre.
    const LINE: f32 = 0.75;
    const GAP: f32 = 10.0;
    /// The cross lines' strength.
    const LINE_ALPHA: f32 = 0.85;
    /// Over the vignette and every layer projected from the world (the
    /// nameplates at -3, the markers and the floating numbers at -2), so none
    /// floats on the black surround; under the reticle and the HUD layers,
    /// which sit at zero and above.
    const SCOPE_Z: i32 = -1;

    /// The whole overlay, shown while the local player is scoped.
    #[derive(Component)]
    struct ScopeOverlay;

    /// Registers the overlay and its system.
    pub(crate) fn install(app: &mut App) {
        app.add_systems(Startup, spawn)
            .add_systems(Update, show.in_set(ClientSet::Hud));
    }

    /// The surround as a picture: opaque black outside a round window, with
    /// fine dark cross lines through it and a clear gap at the centre.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a 512 pixel picture: coordinates are exact in f32 and alpha is clamped to 0..=1"
    )]
    fn picture() -> Image {
        let centre = (SIDE as f32 - 1.) * 0.5;
        let radius = centre * WINDOW;
        let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (dx, dy) = (x as f32 - centre, y as f32 - centre);
                let outside = ((dx.hypot(dy) - radius) / EDGE + 0.5).clamp(0., 1.);
                let on_line =
                    (dx.abs() < LINE && dy.abs() > GAP) || (dy.abs() < LINE && dx.abs() > GAP);
                let line = if on_line && outside < 1. {
                    LINE_ALPHA
                } else {
                    0.
                };
                let alpha = outside.max(line);
                data.extend_from_slice(&[0, 0, 0, (alpha * 255.).round() as u8]);
            }
        }
        Image::new(
            Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }

    /// `Startup`: a square of the picture as tall as the window, between two
    /// black bars that take the rest of its width.
    fn spawn(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
        let Some(mut images) = images else {
            return;
        };
        let image = images.add(picture());
        let bar = || {
            (
                Node {
                    flex_grow: 1.,
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::BLACK),
                Pickable::IGNORE,
            )
        };
        commands
            .spawn((
                ScopeOverlay,
                Name::new("hud.scope"),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(SCOPE_Z),
                Pickable::IGNORE,
                Visibility::Hidden,
            ))
            .with_children(|root| {
                root.spawn(bar());
                root.spawn((
                    Node {
                        height: percent(100),
                        aspect_ratio: Some(1.),
                        ..default()
                    },
                    ImageNode::new(image),
                    Pickable::IGNORE,
                ));
                root.spawn(bar());
            });
    }

    /// `Update`: the overlay shows exactly while the local player is scoped.
    fn show(
        local: Query<&Combatant, With<LocalPlayer>>,
        mut overlays: Query<&mut Visibility, With<ScopeOverlay>>,
    ) {
        let scoped = local
            .iter()
            .next()
            .is_some_and(|actor| super::scoped_zoom(actor).is_some());
        for mut visibility in &mut overlays {
            let wanted = if scoped {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != wanted {
                *visibility = wanted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_sim::PlayerIntent;
    use strata_sim::combat::{CombatIntent, CombatMotion, Weapon};

    const TICK: f32 = 1. / 60.;

    fn drive(actor: &mut Combatant, ticks: u32, set: impl Fn(&mut CombatIntent)) {
        let mut motion = CombatMotion::default();
        for _ in 0..ticks {
            let mut input = CombatIntent::default();
            set(&mut input);
            actor.step(&mut input, &PlayerIntent::default(), TICK, &mut motion);
        }
    }

    fn rifleman() -> Combatant {
        Combatant::default().carrying([Some(Weapon::Rifle), Some(Weapon::Carbine)], 0)
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ticks"
    )]
    fn the_scope_is_entered_only_when_the_aim_has_closed_on_a_zoomed_weapon() {
        let mut actor = rifleman();
        assert!(actor.weapon_spec().zoom > MIN_ZOOM);
        assert_eq!(scoped_zoom(&actor), None, "not aiming");
        let whole = (actor.weapon_spec().aim_time / TICK).ceil() as u32;
        drive(&mut actor, whole / 2, |input| input.aim = true);
        assert_eq!(scoped_zoom(&actor), None, "still closing: the aim view");
        drive(&mut actor, whole, |input| input.aim = true);
        assert_eq!(scoped_zoom(&actor), Some(actor.weapon_spec().zoom));
    }

    #[test]
    fn dropping_the_aim_a_swap_a_reload_a_dodge_and_a_stagger_each_leave_it() {
        let scoped = || {
            let mut actor = rifleman();
            drive(&mut actor, 120, |input| input.aim = true);
            assert!(scoped_zoom(&actor).is_some());
            actor.ammo = 1;
            actor
        };
        let mut actor = scoped();
        drive(&mut actor, 1, |_| {});
        assert_eq!(scoped_zoom(&actor), None, "aim dropped");
        let mut actor = scoped();
        drive(&mut actor, 1, |input| {
            input.aim = true;
            input.reload = true;
        });
        assert_eq!(scoped_zoom(&actor), None, "reload");
        let mut actor = scoped();
        drive(&mut actor, 1, |input| {
            input.aim = true;
            input.weapon = Some(1);
        });
        assert_eq!(scoped_zoom(&actor), None, "swap");
        let mut actor = scoped();
        drive(&mut actor, 1, |input| {
            input.aim = true;
            input.dodge = true;
        });
        assert_eq!(scoped_zoom(&actor), None, "dodge");
        let mut actor = scoped();
        actor.hit(strata_sim::combat::Blow::bare(0., 10_000.));
        assert_eq!(scoped_zoom(&actor), None, "stagger");
    }

    #[test]
    fn the_view_is_the_aim_view_over_the_zoom_and_the_mouse_turns_by_the_same_factor() {
        let base = 45_f32.to_radians();
        let mut actor = rifleman();
        drive(&mut actor, 120, |input| input.aim = true);
        let zoom = scoped_zoom(&actor).expect("scoped");
        let aim_view = view_fov(base, true, None);
        assert!((view_fov(base, true, Some(zoom)) - aim_view / zoom).abs() < 1e-6);
        assert!((look_factor(&actor) - 1. / zoom).abs() < 1e-6);
        assert!(look_factor(&actor) < 1.);
    }

    #[test]
    fn a_weapon_with_no_zoom_keeps_the_ordinary_camera() {
        let base = 45_f32.to_radians();
        for gun in [Weapon::Carbine, Weapon::Pistol] {
            let mut actor = Combatant::default().carrying([Some(gun), None], 0);
            drive(&mut actor, 120, |input| input.aim = true);
            assert!(actor.aiming);
            assert_eq!(scoped_zoom(&actor), None);
            assert_eq!(look_factor(&actor).to_bits(), 1_f32.to_bits());
            assert_eq!(
                view_fov(base, true, scoped_zoom(&actor)).to_bits(),
                (base * (52. / 68.)).to_bits()
            );
            assert_eq!(view_fov(base, false, None).to_bits(), base.to_bits());
        }
    }
}
