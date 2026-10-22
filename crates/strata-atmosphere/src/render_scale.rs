//! Render scale (review 2026-10-16, F8): the 3D camera draws to an image at
//! a share of the window's size and a second camera stretches it over the
//! window and sharpens it.
//!
//! Bevy has no render-scale setting (`MainPassResolutionOverride` leaves the
//! post stack at full size), so the working route is a second camera. The
//! 3D camera keeps everything it has, TAA, SSAO, both fogs, bloom and
//! tonemapping, and each of those reads its own view's size, so none of them
//! changes. The window camera carries the UI by Bevy's default-UI-camera
//! rule (the highest-order camera on the primary window).
//!
//! The image target's `scale_factor` is set so that the 3D camera's
//! *logical* viewport is the window's logical size: `world_to_viewport`
//! then answers in the same units it did before, and the nameplates, the
//! tracker, the combat text and the map marks need no conversion by the
//! scale.
//!
//! At a scale of 1 there is no second camera and no image: the 3D camera
//! draws to the window as it always did.

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::app::{App, AppExit, Last, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    Camera, Camera2d, Camera3d, ImageRenderTarget, OrthographicProjection, Projection,
    RenderTarget, ScalingMode,
};
use bevy::ecs::prelude::*;
use bevy::image::{Image, ImageSampler};
use bevy::math::{UVec2, Vec2};
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::render::view::Msaa;
use bevy::sprite::Sprite;
use bevy::window::{PrimaryWindow, Window};

use crate::look::RenderLook;
use crate::{NoSky, SkyCamera};

/// The smallest scale the look accepts, mirrored by its validation.
pub const MIN_SCALE: f32 = 0.5;

/// The render layer of the window camera and the one sprite it draws, far
/// from the weapon stage's and the world's, so neither sees the other.
const PRESENT_LAYER: usize = 30;

/// The window camera draws after every camera of the world.
const PRESENT_ORDER: isize = 1000;

/// How hard the upscaled image is sharpened. Bevy's default is 0.6, made for
/// a ratio near 1; a quarter of the pixels is a mild softening that wants a
/// little less than a TAA blur does.
const SHARPENING: f32 = 0.5;

/// Marks a 3D camera that draws to an image, with what it needs.
#[derive(Component, Debug, Clone)]
pub struct Scaled {
    /// The image the camera renders to.
    pub image: Handle<Image>,
    /// The camera that draws it to the window.
    pub window_camera: Entity,
    /// The sprite that holds the image in front of that camera.
    pub sprite: Entity,
}

/// Marks the window camera and its sprite, so a scale back to 1 removes both.
#[derive(Component, Debug, Clone, Copy)]
struct Present;

/// The scale the look asks for: out of range or not a number is full size.
#[must_use]
pub fn wanted(look: &RenderLook) -> f32 {
    let scale = look.render_scale;
    if scale.is_finite() {
        scale.clamp(MIN_SCALE, 1.0)
    } else {
        1.0
    }
}

/// The size of the image for a window of `physical` pixels at `scale`.
#[must_use]
pub fn image_size(physical: UVec2, scale: f32) -> UVec2 {
    let scaled = (physical.as_vec2() * scale).round().as_uvec2();
    scaled.max(UVec2::ONE).min(physical.max(UVec2::ONE))
}

/// The `scale_factor` that makes the camera's logical viewport the window's
/// logical size, whatever the image's rounding.
#[must_use]
pub fn target_scale_factor(window_scale_factor: f32, image: UVec2, physical: UVec2) -> f32 {
    window_scale_factor * image.as_vec2().x / physical.max(UVec2::ONE).as_vec2().x
}

fn new_image(size: UVec2) -> Image {
    // sRGB so the sprite's read undoes the camera's write exactly.
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    // Linear, or the stretch is blocks. The usage keeps the main-world copy
    // the camera system reads the target's size from.
    image.sampler = ImageSampler::linear();
    image.asset_usage = RenderAssetUsages::default();
    image
}

/// `Update`: follows the look's scale and the window's size.
fn apply(
    mut commands: Commands,
    look: Res<RenderLook>,
    images: Option<ResMut<Assets<Image>>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<
        (Entity, &mut RenderTarget, Option<&Scaled>),
        (With<Camera3d>, With<SkyCamera>, Without<NoSky>),
    >,
    present: Query<Entity, With<Present>>,
) {
    let (Some(mut images), Ok(window)) = (images, window.single()) else {
        return;
    };
    let scale = wanted(&look);
    let physical = window.physical_size();
    for (camera, mut target, scaled) in &mut cameras {
        if scale >= 1.0 {
            let Some(scaled) = scaled else { continue };
            // Back to the window: the camera, the sprite and the image go.
            *target = RenderTarget::default();
            commands.entity(camera).remove::<Scaled>();
            for entity in &present {
                commands.entity(entity).despawn();
            }
            images.remove(&scaled.image);
            continue;
        }
        if physical.x == 0 || physical.y == 0 {
            continue;
        }
        let size = image_size(physical, scale);
        let factor = target_scale_factor(window.scale_factor(), size, physical);
        match scaled {
            None => {
                let image = images.add(new_image(size));
                let layers = RenderLayers::layer(PRESENT_LAYER);
                let sprite = commands
                    .spawn((
                        Present,
                        Sprite {
                            custom_size: Some(Vec2::splat(2.0)),
                            ..Sprite::from_image(image.clone())
                        },
                        layers.clone(),
                    ))
                    .id();
                let window_camera = commands
                    .spawn((
                        Present,
                        Camera2d,
                        // One sprite needs no multisampled colour and depth
                        // at window size; that bandwidth is what the scale
                        // is there to save.
                        Msaa::Off,
                        Camera {
                            order: PRESENT_ORDER,
                            ..Default::default()
                        },
                        // The sprite is two units square at the origin and the
                        // view is two units square, so it fills the window
                        // whatever its aspect.
                        Projection::Orthographic(OrthographicProjection {
                            scaling_mode: ScalingMode::Fixed {
                                width: 2.0,
                                height: 2.0,
                            },
                            ..OrthographicProjection::default_2d()
                        }),
                        ContrastAdaptiveSharpening {
                            sharpening_strength: SHARPENING,
                            ..Default::default()
                        },
                        layers,
                    ))
                    .id();
                *target = RenderTarget::Image(ImageRenderTarget {
                    handle: image.clone(),
                    scale_factor: factor,
                });
                commands.entity(camera).insert(Scaled {
                    image,
                    window_camera,
                    sprite,
                });
            }
            Some(scaled) => {
                let Some(image) = images.get(&scaled.image) else {
                    continue;
                };
                let same_size = image.size() == size;
                let same_factor = matches!(
                    &*target,
                    RenderTarget::Image(t) if (t.scale_factor - factor).abs() < 1e-6
                );
                if same_size && same_factor {
                    continue;
                }
                // The camera re-reads its target when the image changes, so
                // the factor and the size move together.
                if let Some(mut image) = images.get_mut(&scaled.image) {
                    image.resize(Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    });
                }
                *target = RenderTarget::Image(ImageRenderTarget {
                    handle: scaled.image.clone(),
                    scale_factor: factor,
                });
            }
        }
    }
}

/// `Last`: on the frame the app exits, no camera draws.
///
/// Pipelined rendering drops the main world by waiting for the render world
/// to come back from its last frame, and a camera that draws to an image
/// still runs its schedule in that frame (a window camera is skipped because
/// its window is gone), whose systems that need the main thread then wait for
/// a thread that is waiting for them. Nothing is worth drawing on the way
/// out.
fn quiet_on_exit(mut exits: MessageReader<AppExit>, mut cameras: Query<&mut Camera>) {
    if exits.read().next().is_none() {
        return;
    }
    for mut camera in &mut cameras {
        camera.is_active = false;
    }
}

/// Puts the systems in.
pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, apply)
        .add_systems(Last, quiet_on_exit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_is_a_share_of_the_window_and_never_empty() {
        assert_eq!(
            image_size(UVec2::new(2560, 1440), 0.75),
            UVec2::new(1920, 1080)
        );
        assert_eq!(image_size(UVec2::new(1, 1), 0.5), UVec2::ONE);
        assert_eq!(image_size(UVec2::new(1001, 3), 0.5), UVec2::new(501, 2));
    }

    #[test]
    fn the_logical_viewport_is_the_windows_whatever_the_rounding() {
        let physical = UVec2::new(2561, 1441);
        let image = image_size(physical, 0.75);
        let factor = target_scale_factor(1.5, image, physical);
        let logical = image.as_vec2().x / factor;
        assert!((logical - physical.as_vec2().x / 1.5).abs() < 1e-3);
    }

    #[test]
    fn a_bad_scale_is_full_size_and_the_range_is_held() {
        let mut look = RenderLook::default();
        for (asked, wanted_scale) in [(f32::NAN, 1.0), (0.1, 0.5), (2.0, 1.0), (0.75, 0.75)] {
            look.render_scale = asked;
            assert!((wanted(&look) - wanted_scale).abs() < f32::EPSILON);
        }
    }
}
