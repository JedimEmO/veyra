//! Fixed, world-anchored locations for repeatable atmosphere comparisons.

use crate::look::RenderLook;
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::light::FogVolume;
use bevy::math::Vec3;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::transform::components::Transform;
use strata_app::{WorldPosition, WorldSourceRes};

/// Stable fixture names used by the CLI.
pub const SCENES: [&str; 4] = ["woodland", "grove", "valley", "coast"];

/// Preview overrides are render-only; simulation time remains authoritative.
#[derive(Resource, Clone, Debug, Default)]
pub struct StudySettings {
    /// Selects an authored fog fixture. None leaves the world without local fog.
    pub scene: Option<String>,
    /// A frozen sun time in [0, 1), used only for standalone comparisons.
    pub visual_time: Option<f64>,
}

/// Fixture spawn coordinates, in the seed-42 world.
///
/// Each is a place that is what its name says, measured, and judged as the
/// bench and `tools/render-study.py` film it: eye height, looking along
/// yaw -1.25 (forward is `+x` and a third as much `-z`) with a frame about
/// 73 degrees wide. Ticket 0004's step 2 moved the ranges and re-sited
/// three of them, its review re-sited two again after finding they had
/// been judged from every direction rather than the frame's, its step 2b,
/// which made the ranges lines and moved the climate's equator, re-sited
/// three, and its step 4, which gave the ranges a foot and the provinces
/// their shapes, re-sited the woodland; the ticket's fixture tables give
/// each before and after. Cover is
/// the mean over a 300 m disc on a 50 m lattice.
/// - `woodland`: open forest (forest at full weight, cover 0.37 underfoot,
///   0.52 over the disc) on level ground at 200 m (206 m until the review
///   of step 4), rising ground 13 degrees high at the frame's right edge
///   (13.4 on step 5's ground), since ticket 0004's step 4. At step
///   2b's site, (-7000, 19500), step 4's ground raised a hill 37 degrees
///   high 500 m off at the frame's left edge, and the capture was ground and
///   a fern; step 2b's cooler `+z` had left the site before it, (-8750,
///   23500), half tundra.
/// - `grove`: closed forest (cover 1.0 underfoot, 0.82 over the disc) at
///   176 m (180 m until the review of step 4, 162 m until step 4, which
///   tilted it to a slope of 0.18), with
///   land ahead to the frame's end. The review's site, (-42500, 31000), is
///   tundra since step 2b (cover 0.22).
/// - `valley`: a wooded floor 61 m up between the range at `z` 13 km and
///   the hills across it, the ground within 2 km rising over 1 500 m; in the
///   frame the range's wall stands 25 degrees high at the right edge (24
///   on step 4's ground, 28 on step 2b's) and the far side 7 degrees at the
///   left, the head 31 degrees ahead 3 km off. It does not read as a valley
///   at eye height: the near ground hides both walls (ticket 0004, step 4's
///   captures). No floor under 100 m with 1 500 m of relief within 2 km,
///   within 45 km of the origin, has walls between 6 and 25 degrees at both
///   edges of this frame and a head under 24 degrees (step 4's search, and
///   step 5's on its taller crests; step 2b found the same on its ground). The review's site, (-8500, -41000), stands since
///   step 2b where the frame's walls are 7 and 12 degrees high 10 km off.
/// - `coast`: a beach under a steep shore, 8 m up, the frame looking out
///   over the sea; unchanged since before step 2.
pub fn location(name: &str) -> Option<(f64, f64)> {
    match name {
        "woodland" => Some((-7500.0, 21500.0)),
        "grove" => Some((-43500.0, -2500.0)),
        "valley" => Some((34270.0, 13030.0)),
        "coast" => Some((21949.0, -15945.0)),
        _ => None,
    }
}

/// This volume is stationary even when the observer leaves the study site.
#[derive(Component)]
pub struct StudyFog;

/// Soft edges prevent a visible box, with broad variation within the volume.
fn density(x: f32, y: f32, z: f32) -> f32 {
    let edge = |v: f32| {
        let t = (v.min(1.0 - v) * 10.0).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let variation = 0.7 + 0.15 * (x * 19.0 + z * 7.0).sin() + 0.15 * (z * 23.0 - x * 5.0).cos();
    edge(x) * edge(y) * edge(z) * (-2.0 * y).exp() * variation
}

/// A small 3D texture shared by the one active fixture, rebuilt only on scene changes.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "density is clamped into an R8 texture"
)]
fn density_image() -> Image {
    const SIZE: u32 = 32;
    let mut bytes = Vec::with_capacity((SIZE * SIZE * SIZE) as usize);
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let p = |v: u32| f32::from(u16::try_from(v).expect("small texture")) / 31.0;
                bytes.push((density(p(x), p(y), p(z)).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: SIZE,
        },
        TextureDimension::D3,
        bytes,
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    )
}

/// Changes density without moving the volume or rebuilding its texture.
pub(crate) fn update_volumes(
    mut commands: Commands,
    settings: Res<StudySettings>,
    look: Res<RenderLook>,
    source: Option<Res<WorldSourceRes>>,
    images: Option<ResMut<Assets<Image>>>,
    mut volumes: Query<(Entity, &mut FogVolume), With<StudyFog>>,
    mut installed: Local<Option<String>>,
) {
    if settings.scene == *installed {
        if look.is_changed() {
            for (_, mut fog) in &mut volumes {
                fog.density_factor = look.fog_density;
                fog.scattering = look.fog_scattering;
                fog.light_intensity = look.fog_light_intensity;
                fog.scattering_asymmetry = look.fog_asymmetry;
            }
        }
        return;
    }
    let (Some(source), Some(mut images)) = (source, images) else {
        return;
    };
    for (entity, _) in &volumes {
        commands.entity(entity).despawn();
    }
    if let Some((x, z)) = settings.scene.as_deref().and_then(location) {
        let y = f64::from(source.source().sample(x, z).height);
        tracing::info!(x, y, z, density = look.fog_density, "installed study fog");
        commands.spawn((
            StudyFog,
            Name::new("study mist"),
            WorldPosition::new(x, y + 14.0, z),
            Transform::from_scale(Vec3::new(320.0, 48.0, 320.0)),
            FogVolume {
                fog_color: Color::srgb(0.7, 0.8, 0.95),
                density_factor: look.fog_density,
                density_texture: Some(images.add(density_image())),
                absorption: 0.1,
                scattering: look.fog_scattering,
                scattering_asymmetry: look.fog_asymmetry,
                light_intensity: look.fog_light_intensity,
                ..Default::default()
            },
        ));
    }
    (*installed).clone_from(&settings.scene);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fog_keeps_its_world_anchor_and_scale_across_rebases() {
        use bevy::app::App;
        use strata_app::{PrimaryObserver, RenderOrigin};
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, strata_app::AppPlugin));
        for _ in 0..3 {
            app.update();
        }
        let anchor = WorldPosition::new(100.0, 25.0, -50.0);
        let fog = app
            .world_mut()
            .spawn((
                StudyFog,
                FogVolume::default(),
                anchor,
                Transform::from_scale(Vec3::new(320.0, 48.0, 320.0)),
            ))
            .id();
        app.world_mut()
            .spawn((PrimaryObserver, WorldPosition::new(5000.0, 300.0, 0.0)));
        app.update();
        let origin = app.world().resource::<RenderOrigin>();
        assert_eq!(
            *app.world().get::<WorldPosition>(fog).expect("fog anchor"),
            anchor
        );
        let transform = app.world().get::<Transform>(fog).expect("fog transform");
        assert!(
            transform
                .translation
                .abs_diff_eq(origin.to_render(anchor.0), 1e-4)
        );
        assert_eq!(transform.scale, Vec3::new(320.0, 48.0, 320.0));
    }

    /// Each scene is still what its name says on seed 42's ground, as
    /// measured when it was sited (ticket 0004's fixture tables): a retune
    /// that moves a range under one shows here and not first in a capture.
    #[test]
    fn every_scene_is_what_its_name_says() {
        use strata_worldgen::WorldSource;
        let registries =
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load");
        let world = strata_worldgen::ProceduralWorld::new(strata_core::Seed(42), &registries);
        let biome = |name: &str| {
            registries
                .biome_id(&strata_registry::Namespaced::parse(name).expect("valid"))
                .expect("ships")
        };
        let at = |name: &str| {
            let (x, z) = location(name).expect("a scene");
            (x, z, world.sample(x, z))
        };
        // The range of heights on a lattice 2 km to each side.
        let relief = |x: f64, z: f64| {
            let mut range = (f32::MAX, f32::MIN);
            for j in -10..=10 {
                for i in -10..=10 {
                    let h = world
                        .sample(x + f64::from(i) * 200.0, z + f64::from(j) * 200.0)
                        .height
                        .max(0.0);
                    range = (range.0.min(h), range.1.max(h));
                }
            }
            range.1 - range.0
        };
        let (_, _, woodland) = at("woodland");
        assert!(
            woodland.dominant_biome() == biome("strata:forest")
                && (0.3..0.8).contains(&woodland.cover)
                && woodland.landform.slope < 0.15,
            "woodland is not open forest on level ground: {woodland:?}"
        );
        let (_, _, grove) = at("grove");
        assert!(
            grove.dominant_biome() == biome("strata:forest") && grove.cover > 0.9,
            "grove is not closed forest: {grove:?}"
        );
        let (x, z, valley) = at("valley");
        assert!(
            valley.height > 0.0
                && valley.height < 100.0
                && valley.landform.slope < 0.1
                && relief(x, z) > 1_500.0,
            "valley is not a low floor between high walls: {valley:?}, relief {}",
            relief(x, z)
        );
        let (_, _, coast) = at("coast");
        assert!(
            coast.dominant_biome() == biome("strata:coast") && (0.0..20.0).contains(&coast.height),
            "coast is not a beach: {coast:?}"
        );
    }

    #[test]
    fn volume_edges_are_empty_and_inside_is_finite() {
        for i in 0u16..=20 {
            let t = f32::from(i) / 20.0;
            for edge in [0.0, 1.0] {
                assert!(density(edge, t, t).abs() < 1e-6);
                assert!(density(t, edge, t).abs() < 1e-6);
                assert!(density(t, t, edge).abs() < 1e-6);
            }
            assert!((0.0..=1.0).contains(&density(t, 0.3, 0.5)));
        }
        assert!(density(0.5, 0.3, 0.5) > 0.1);
    }
}
