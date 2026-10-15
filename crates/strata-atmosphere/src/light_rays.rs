//! Screen-space light rays: the low sun streaming past trunks, crowns and
//! roofs, Guild Wars 2's signature (the lighting plan's phase 1).
//!
//! Two full-screen passes in the `Core3d` schedule, after bloom and before
//! tonemapping. The first, at half resolution, marches from each pixel
//! towards the sun's position on screen and sums a glow around the sun
//! wherever the depth buffer holds sky above the horizon, decaying per
//! step, so anything between the pixel and the sun carves a shadow into
//! the ray. The second adds that half-resolution texture over the view's
//! colour. The source is a glow of the sun's screen distance rather than
//! the frame's own colour: the frame's colour is the whole blue sky at
//! about the brightness of lit ground and the sun's disc at ten thousand
//! times it, so any clamp of it decides the look on its own and moves with
//! the exposure; a glow the shape of the sun's halo carves the same shafts
//! and holds at every exposure. The rays take the sun's colour as the
//! atmosphere's transmittance table gives it at the camera, so a low sun's
//! rays are as red as its light, times [`RenderLook::light_rays_tint`].
//! Behind the cloud layer the strength is scaled by
//! [`crate::clouds::SunVeil`], the layer's clearness on the line of sight to
//! the sun, worked out once a frame on the CPU, so the shafts go when the
//! sun goes behind a cloud and the march reads no cloud texture.
//!
//! `bevy_core_pipeline::fullscreen_material` does not fit, for the same
//! reason it did not fit the ground fog: its bind group is the colour
//! target, a sampler and one uniform, with no depth, and it draws at full
//! resolution only. The march needs the depth buffer and is cheap enough at
//! half resolution that the full-resolution part is one filtered read.
//!
//! The pass runs after Bevy's TAA, which resolves in `EarlyPostProcess`, so
//! its dither cannot move per frame (TAA would not average it away and it
//! would shimmer); it is a fixed per-pixel pattern instead, and the
//! composite's small tent softens it.
//!
//! What it does not handle, like the fog: MSAA (the depth is multisampled;
//! a multisampled camera gets no rays), a camera without Bevy's atmosphere
//! (no transmittance table; skipped), viewports. The sea is a blended plane
//! that writes no depth, so below the horizon the depth buffer cannot tell
//! sea from sky; the march only counts sky above the horizon's line on
//! screen, which [`horizon_line`] gives it.

use crate::look::RenderLook;
use crate::{Planet, SkyCamera, Sun};
use bevy::app::{App, PostUpdate, Update};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::{Camera, Camera3d, CameraUpdateSystems, Exposure};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::light::DirectionalLight;
use bevy::math::{Mat4, UVec4, Vec2, Vec3, Vec4, Vec4Swizzles};
use bevy::pbr::resources::AtmosphereTextures;
use bevy::post_process::bloom::bloom;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    Extent3d, FilterMode, FragmentState, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, Msaa, ViewDepthTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bevy::transform::components::{GlobalTransform, Transform};
use std::collections::HashMap;

/// The shader, loaded from disk like the fog's.
pub const LIGHT_RAYS_SHADER_PATH: &str = "shaders/light_rays.wgsl";

/// The half-resolution texture the march writes. Sixteen-bit float: the
/// rays near the sun are several times the lit ground's brightness.
const RAYS_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The radius of the glow around the sun the march sums, in screen heights.
/// Wide enough that a sun just behind a crown still throws shafts across
/// the frame, narrow enough that open sky far from the sun adds nothing.
const GLOW_RADIUS: f32 = 0.6;

/// The pass's per-camera uniform, and the component whose presence on a
/// sky camera asks for the rays. Mirrored field for field by `LightRays` in
/// `shaders/light_rays.wgsl`.
#[derive(Component, Clone, Copy, Default, Debug, PartialEq, ShaderType, ExtractComponent)]
pub struct LightRays {
    /// The sun's position on screen in `xy` (uv, `y` down), the rays'
    /// strength in `z` (zero draws nothing), the viewport's width over its
    /// height in `w`.
    sun: Vec4,
    /// The rays' colour at full strength in `rgb`: the tint times the
    /// sun's light as the camera's exposure scales it, before the
    /// atmosphere's transmittance; the per-step decay in `w`.
    color: Vec4,
    /// The atmosphere's inner and outer radius, the camera's distance from
    /// the planet's centre, and the cosine of the sun's zenith angle there.
    atmosphere: Vec4,
    /// The horizon's line on screen, `x * u + y * v + z > 0` above it; the
    /// glow's radius in `w`.
    horizon: Vec4,
    /// Steps, and three unused.
    counts: UVec4,
}

impl LightRays {
    /// Whether the rays draw anything this frame.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.sun.z > 0.0
    }
}

/// Where the sun stands on a camera's screen and how much of the rays'
/// strength it keeps there: `None` when it is behind the camera.
///
/// `clip_from_world` is the camera's projection times its view, and
/// `to_sun` the unit vector towards the sun. The sun is at infinity, so it
/// is projected as a direction (`w` zero) and the camera's position does
/// not move it. The strength fades out as the sun leaves the screen (gone
/// a little over half a screen beyond the edge, where nothing on screen
/// lies between a pixel and it any more) and as it sets.
#[must_use]
pub fn sun_on_screen(clip_from_world: Mat4, to_sun: Vec3) -> Option<(Vec2, f32)> {
    let clip = clip_from_world * to_sun.extend(0.0);
    // `w` is the distance in front of the camera; a sun almost square to
    // the view lands so far off screen that it is faded already.
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc = clip.xy() / clip.w;
    let uv = Vec2::new(0.5 + 0.5 * ndc.x, 0.5 - 0.5 * ndc.y);
    let beyond = ndc.x.abs().max(ndc.y.abs());
    let on_screen = 1.0 - smoothstep(1.0, 1.6, beyond);
    let risen = smoothstep(-0.02, 0.06, to_sun.y);
    Some((uv, on_screen * risen))
}

/// The horizon's line on screen as `(a, b, c)` with `a * u + b * v + c > 0`
/// above it, from two horizontal directions in front of the camera.
/// A camera looking straight up or down sees no horizon; then every point
/// counts as above.
#[must_use]
pub fn horizon_line(clip_from_world: Mat4, forward: Vec3) -> Vec3 {
    const EVERYWHERE: Vec3 = Vec3::new(0.0, 0.0, 1.0);
    let flat = Vec3::new(forward.x, 0.0, forward.z);
    if flat.length_squared() < 1e-6 {
        return EVERYWHERE;
    }
    let flat = flat.normalize();
    let right = Vec3::new(-flat.z, 0.0, flat.x);
    let project = |direction: Vec3| {
        let clip = clip_from_world * direction.extend(0.0);
        (clip.w > 1e-4).then(|| Vec2::new(0.5 + 0.5 * clip.x / clip.w, 0.5 - 0.5 * clip.y / clip.w))
    };
    let (Some(a), Some(b), Some(up)) = (
        project(flat),
        project((flat + right * 0.5).normalize()),
        project((flat + Vec3::Y * 0.05).normalize()),
    ) else {
        return EVERYWHERE;
    };
    let along = b - a;
    let normal = Vec2::new(-along.y, along.x);
    if normal.length_squared() < 1e-12 {
        return EVERYWHERE;
    }
    let line = normal.extend(-normal.dot(a));
    if line.dot(up.extend(1.0)) < 0.0 {
        -line
    } else {
        line
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Registers the rays' passes.
pub(crate) fn register(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<LightRays>::default(),
        UniformComponentPlugin::<LightRays>::default(),
    ))
    .add_systems(
        PostUpdate,
        update_uniforms
            .after(bevy::transform::TransformSystems::Propagate)
            .after(CameraUpdateSystems)
            .after(crate::clouds::update_layer),
    )
    .add_systems(Update, log_gpu_time);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            (
                readable_depth.in_set(RenderSystems::PrepareViews),
                prepare_pipelines.in_set(RenderSystems::Prepare),
                prepare_textures.in_set(RenderSystems::PrepareResources),
            ),
        )
        .add_systems(
            Core3d,
            light_rays
                .in_set(Core3dSystems::PostProcess)
                .after(bloom)
                .before(tonemapping),
        );
}

/// Writes every sky camera's uniform from this frame's sun, camera and
/// look, or takes it away when the look has no rays or the camera is
/// multisampled.
#[allow(
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn update_uniforms(
    mut commands: Commands,
    look: Res<RenderLook>,
    cameras: Query<
        (
            Entity,
            &Camera,
            &GlobalTransform,
            Option<&Exposure>,
            Option<&Msaa>,
            Has<LightRays>,
        ),
        With<SkyCamera>,
    >,
    suns: Query<(&Transform, &DirectionalLight), With<Sun>>,
    planets: Query<(&bevy::light::Atmosphere, &GlobalTransform), With<Planet>>,
    veil: Option<Res<crate::clouds::SunVeil>>,
) {
    let wanted = look.light_rays_strength > 0.0;
    // Under a thousandth of the look's strength the rays are invisible, so
    // a sun behind a full cloud draws no rays and skips both passes.
    let clear = veil.map_or(1.0, |v| v.clear).clamp(0.0, 1.0);
    let clear = if clear < 1e-3 { 0.0 } else { clear };
    let sun = suns.iter().next();
    for (entity, camera, transform, exposure, msaa, has) in &cameras {
        let multisampled = msaa.is_some_and(|m| *m != Msaa::Off);
        let Some((sun_transform, light)) = sun.filter(|_| wanted && !multisampled) else {
            if has {
                commands.entity(entity).remove::<LightRays>();
            }
            continue;
        };
        let to_sun = sun_transform.rotation * Vec3::Z;
        let clip_from_world = camera.clip_from_view() * transform.to_matrix().inverse();
        let (uv, strength) = sun_on_screen(clip_from_world, to_sun).unwrap_or((Vec2::ZERO, 0.0));
        let size = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
        let aspect = if size.y > 0.0 { size.x / size.y } else { 1.0 };
        let exposure = exposure.copied().unwrap_or_default().exposure();
        let linear = light.color.to_linear();
        // The radiance of white ground under the sun: what the rays are
        // measured against, so at strength 1 a ray next to the sun is as
        // bright as lit white paint.
        let scale = light.illuminance * exposure / core::f32::consts::PI;
        let [r, g, b] = look.light_rays_tint;
        let camera_position = transform.translation();
        let (inner, outer, radius, mu) = planets.iter().next().map_or(
            (6_360_000.0, 6_460_000.0, 6_360_000.0, to_sun.y),
            |(atmosphere, planet)| {
                let up = camera_position - planet.translation();
                (
                    atmosphere.inner_radius,
                    atmosphere.outer_radius,
                    up.length(),
                    to_sun.dot(up.normalize_or(Vec3::Y)),
                )
            },
        );
        commands.entity(entity).insert(LightRays {
            sun: Vec4::new(
                uv.x,
                uv.y,
                strength * look.light_rays_strength * clear,
                aspect,
            ),
            color: Vec4::new(
                linear.red * r * scale,
                linear.green * g * scale,
                linear.blue * b * scale,
                look.light_rays_decay,
            ),
            atmosphere: Vec4::new(inner, outer, radius, mu),
            horizon: horizon_line(clip_from_world, transform.forward().as_vec3())
                .extend(GLOW_RADIUS),
            counts: UVec4::new(look.light_rays_steps.max(1), 0, 0, 0),
        });
    }
}

/// The two pipelines' layouts, sampler and shader, and the composite's
/// pipelines by target format.
#[derive(Resource)]
struct LightRaysPipeline {
    march_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
    march: Option<CachedRenderPipelineId>,
    composites: HashMap<TextureFormat, CachedRenderPipelineId>,
}

/// The pipelines chosen for a view: the march and the composite.
#[derive(Component)]
struct LightRaysPipelineIds {
    march: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}

/// The view's half-resolution rays.
#[derive(Component)]
struct LightRaysTexture(CachedTexture);

fn init_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
) {
    let march_layout = BindGroupLayoutDescriptor::new(
        "light_rays_march_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<LightRays>(true),
                texture_depth_2d(),
                // The atmosphere's transmittance table.
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "light_rays_composite_layout",
        // Bindings 4 and 5: the shader holds both entry points, and a
        // module's bindings must not collide.
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (4, texture_2d(TextureSampleType::Float { filterable: true })),
                (5, sampler(SamplerBindingType::Filtering)),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("light_rays_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    commands.insert_resource(LightRaysPipeline {
        march_layout,
        composite_layout,
        sampler,
        fullscreen: fullscreen.clone(),
        shader: asset_server.load(LIGHT_RAYS_SHADER_PATH),
        march: None,
        composites: HashMap::new(),
    });
}

/// The march samples the depth attachment, which is only readable when the
/// camera asks for it. Bevy's atmosphere asks already; this keeps the pass
/// from depending on that.
fn readable_depth(mut cameras: Query<&mut Camera3d, With<LightRays>>) {
    for mut camera in &mut cameras {
        let usage = TextureUsages::TEXTURE_BINDING.bits();
        if camera.depth_texture_usages.0 & usage == 0 {
            camera.depth_texture_usages.0 |= usage;
        }
    }
}

fn prepare_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<LightRaysPipeline>,
    views: Query<(Entity, &ExtractedView), With<LightRays>>,
) {
    for (entity, view) in &views {
        let LightRaysPipeline {
            march_layout,
            composite_layout,
            fullscreen,
            shader,
            march,
            composites,
            ..
        } = &mut *pipeline;
        let march = *march.get_or_insert_with(|| {
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("light_rays_march".into()),
                layout: vec![march_layout.clone()],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some("march".into()),
                    targets: vec![Some(ColorTargetState {
                        format: RAYS_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..Default::default()
                }),
                ..Default::default()
            })
        });
        let composite = *composites.entry(view.target_format).or_insert_with(|| {
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("light_rays_composite".into()),
                layout: vec![composite_layout.clone()],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some("composite".into()),
                    targets: vec![Some(ColorTargetState {
                        format: view.target_format,
                        // The rays are light added to the frame.
                        blend: Some(BlendState {
                            color: BlendComponent {
                                src_factor: BlendFactor::One,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                            alpha: BlendComponent {
                                src_factor: BlendFactor::Zero,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                        }),
                        write_mask: ColorWrites::ALL,
                    })],
                    ..Default::default()
                }),
                ..Default::default()
            })
        });
        commands
            .entity(entity)
            .insert(LightRaysPipelineIds { march, composite });
    }
}

fn prepare_textures(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera, &LightRays)>,
) {
    for (entity, camera, rays) in &views {
        let Some(size) = camera.physical_viewport_size.filter(|_| rays.visible()) else {
            continue;
        };
        let texture = texture_cache.get(
            &render_device,
            TextureDescriptor {
                label: Some("light_rays_texture"),
                size: Extent3d {
                    width: size.x.div_ceil(2).max(1),
                    height: size.y.div_ceil(2).max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: RAYS_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(entity).insert(LightRaysTexture(texture));
    }
}

/// Marches the rays at half resolution, then adds them over the view.
#[allow(
    clippy::type_complexity,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn light_rays(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &LightRays,
        &DynamicUniformIndex<LightRays>,
        &LightRaysPipelineIds,
        Option<&LightRaysTexture>,
        Option<&AtmosphereTextures>,
        &Msaa,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<LightRaysPipeline>,
    uniforms: Res<ComponentUniforms<LightRays>>,
    mut ctx: RenderContext,
) {
    let (target, depth, rays, offset, ids, texture, atmosphere, msaa) = view.into_inner();
    if *msaa != Msaa::Off || !rays.visible() {
        return;
    }
    let (Some(texture), Some(atmosphere)) = (texture, atmosphere) else {
        return;
    };
    let (Some(march), Some(composite), Some(binding)) = (
        pipeline_cache.get_render_pipeline(ids.march),
        pipeline_cache.get_render_pipeline(ids.composite),
        uniforms.uniforms().binding(),
    ) else {
        return;
    };
    let march_group = ctx.render_device().create_bind_group(
        "light_rays_march_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.march_layout),
        &BindGroupEntries::sequential((
            binding,
            depth.view(),
            &atmosphere.transmittance_lut.default_view,
            &pipeline.sampler,
        )),
    );
    let composite_group = ctx.render_device().create_bind_group(
        "light_rays_composite_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.composite_layout),
        &BindGroupEntries::with_indices(((4, &texture.0.default_view), (5, &pipeline.sampler))),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let encoder = ctx.command_encoder();
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("light_rays_march"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &texture.0.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    // The triangle covers every texel.
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let span = diagnostics.pass_span(&mut pass, "light_rays_march");
        pass.set_pipeline(march);
        pass.set_bind_group(0, &march_group, &[offset.index()]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
    }
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("light_rays_composite"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target.main_texture_view(),
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let span = diagnostics.pass_span(&mut pass, "light_rays_composite");
    pass.set_pipeline(composite);
    pass.set_bind_group(0, &composite_group, &[]);
    pass.draw(0..3, 0..1);
    span.end(&mut pass);
}

/// Every two seconds, the GPU time of the sun's passes (bloom and the
/// rays' two), when Bevy's render diagnostics are on (the client's
/// statistics turn them on): what the phase's 0.6 ms ceiling is measured
/// against.
fn log_gpu_time(
    diagnostics: Option<Res<DiagnosticsStore>>,
    time: Res<bevy::time::Time<bevy::time::Real>>,
    veil: Option<Res<crate::clouds::SunVeil>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let ms = |span: &str| {
        let suffix = format!("/{span}/elapsed_gpu");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&suffix))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    };
    let (bloom, march, composite) = (
        ms("bloom"),
        ms("light_rays_march"),
        ms("light_rays_composite"),
    );
    if bloom.is_none() && march.is_none() {
        return;
    }
    let show = |v: Option<f64>| v.map_or_else(|| "-".to_owned(), |v| format!("{v:.3}"));
    tracing::info!(
        "sun passes, ms of GPU time: bloom {}, light rays march {} composite {}; sun clear of cloud {:.3}",
        show(bloom),
        show(march),
        show(composite),
        veil.map_or(1.0, |v| v.clear)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Quat;

    /// A camera at the origin looking along `forward`, 45 degrees high and
    /// 16:9, as Bevy's perspective builds it (infinite reverse z).
    fn clip_from_world(forward: Vec3) -> Mat4 {
        let projection = Mat4::perspective_infinite_reverse_rh(45f32.to_radians(), 16.0 / 9.0, 0.1);
        let view = Transform::IDENTITY.looking_to(forward, Vec3::Y);
        projection * view.to_matrix().inverse()
    }

    #[test]
    fn a_sun_behind_the_camera_gives_no_rays() {
        let to_sun = Vec3::new(0.0, 0.3, 1.0).normalize();
        let clip = clip_from_world(Vec3::NEG_Z);
        assert!(sun_on_screen(clip, to_sun).is_none());
        // Square to the view is no better.
        assert!(sun_on_screen(clip, Vec3::new(1.0, 0.0, 0.0)).is_none_or(|(_, s)| s.abs() < 1e-6));
    }

    #[test]
    fn a_sun_at_the_centre_gives_full_strength() {
        let to_sun = Vec3::new(0.2, 0.5, -1.0).normalize();
        let (uv, strength) = sun_on_screen(clip_from_world(to_sun), to_sun).expect("in front");
        assert!(uv.abs_diff_eq(Vec2::splat(0.5), 1e-5), "{uv}");
        assert!((strength - 1.0).abs() < 1e-6, "{strength}");
    }

    #[test]
    fn the_rays_fade_as_the_sun_leaves_the_screen_and_sets() {
        let forward = Vec3::new(0.0, 0.3, -1.0).normalize();
        let clip = clip_from_world(forward);
        // About 43 degrees to the side of a 73 degree wide frame: just off.
        let off = Quat::from_rotation_y(0.75) * forward;
        let (uv, edge) = sun_on_screen(clip, off).expect("in front");
        assert!(uv.x < 0.0, "{uv}");
        assert!(edge > 0.0 && edge < 1.0, "{edge}");
        let far_off = Quat::from_rotation_y(1.2) * forward;
        let (_, gone) = sun_on_screen(clip, far_off).expect("in front");
        assert!(gone.abs() < 1e-6, "{gone}");
        // A sun just under the horizon, in the middle of the frame.
        let set = Vec3::new(0.0, -0.05, -1.0).normalize();
        let (_, strength) = sun_on_screen(clip_from_world(Vec3::NEG_Z), set).expect("in front");
        assert!(strength.abs() < 1e-6, "{strength}");
    }

    #[test]
    fn the_horizon_line_has_the_sky_above_it() {
        // Looking level: the horizon is the screen's middle row.
        let clip = clip_from_world(Vec3::NEG_Z);
        let line = horizon_line(clip, Vec3::NEG_Z);
        let side = |u: f32, v: f32| line.dot(Vec3::new(u, v, 1.0));
        assert!(side(0.5, 0.3) > 0.0 && side(0.1, 0.45) > 0.0);
        assert!(side(0.5, 0.7) < 0.0 && side(0.9, 0.55) < 0.0);
        assert!(side(0.3, 0.5).abs() < 1e-4);
        // Looking up: the horizon drops down the screen.
        let up = Vec3::new(0.0, 0.3, -1.0).normalize();
        let line = horizon_line(clip_from_world(up), up);
        assert!(line.dot(Vec3::new(0.5, 0.6, 1.0)) > 0.0);
        // Straight down: no horizon, everything counts.
        let line = horizon_line(clip_from_world(Vec3::NEG_Y), Vec3::NEG_Y);
        assert!(line.dot(Vec3::new(0.5, 0.5, 1.0)) > 0.0);
    }

    /// `light_rays.wgsl` with Bevy's import replaced by the part of it it
    /// uses, as Bevy 0.19.1 declares it.
    fn shader_module() -> naga::Module {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/shaders/light_rays.wgsl"
        );
        let source = std::fs::read_to_string(path).expect("the shader is on disk");
        let stubs = "struct FullscreenVertexOutput {\n\
             @builtin(position) position: vec4<f32>,\n\
             @location(0) uv: vec2<f32>,\n\
             };\n";
        let body: String = source
            .lines()
            .filter(|line| !line.starts_with("#import"))
            .collect::<Vec<_>>()
            .join("\n");
        naga::front::wgsl::parse_str(&format!("{stubs}{body}")).unwrap_or_else(|e| {
            panic!(
                "light_rays.wgsl does not parse: {}",
                e.emit_to_string(&body)
            )
        })
    }

    #[test]
    fn the_shader_parses_validates_and_matches_the_uniform() {
        let module = shader_module();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("light_rays.wgsl does not validate: {e:?}"));
        for entry in ["march", "composite"] {
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry}"
            );
        }
        let (members, span) = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, span }
                    if ty.name.as_deref() == Some("LightRays") =>
                {
                    Some((members.clone(), *span))
                }
                _ => None,
            })
            .expect("light_rays.wgsl declares LightRays");
        let shader: Vec<&str> = members.iter().filter_map(|m| m.name.as_deref()).collect();
        assert_eq!(shader, ["sun", "color", "atmosphere", "horizon", "counts"]);
        assert_eq!(
            u64::from(span),
            <LightRays as ShaderType>::min_size().get(),
            "the shader's struct and the uniform differ in size"
        );
    }
}
