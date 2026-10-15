// The ground fog pass of ticket 0003 (strata_atmosphere::fog_pass).
//
// One full-screen triangle after the transparent pass. Per pixel: the view
// ray to the depth buffer's surface (or `far` for the sky), clipped to the
// band the fog can occupy anywhere in the clipmap and to the clipmap's
// square, marched in `steps` steps packed towards the camera. Each step
// reads the field's layer (top, base, density) under it from the clipmap
// and integrates its extinction exactly. The sun's light at each step is
// what the sun's cascaded shadow map lets through there, so hills and
// buildings cut shafts through the layer, times what the cloud layer's
// shadow mask lets through where the ray to the sun crosses the layer. Under
// and around it all lies a thin height fog (the air floor), integrated in
// closed form along the whole ray, its light dimmed by the air in front of
// it. The result is blended as `out = rgb + dst * a`: the light the fog
// adds, and the share of what is behind it that gets through.
//
// Positions are render space (relative to the render origin, which
// rebases). Heights are world heights (the origin's `y` added back); the
// noise is sampled at world `x` and `z` through the origin wrapped to its
// lattice period, and the clipmap's corners arrive in render space every
// frame, so nothing moves when the origin rebases.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

// Mirrors strata_atmosphere::fog_pass::FogPassUniform field for field.
struct FogPass {
    origin: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_color: vec4<f32>,
    moon_direction: vec4<f32>,
    moon_color: vec4<f32>,
    // corner x, corner z (render space), 1 / extent, valid
    fine: vec4<f32>,
    coarse: vec4<f32>,
    // lowest base, highest top with noise and falloff, sea level, noise max
    slab: vec4<f32>,
    // extinction, albedo, asymmetry, light
    medium: vec4<f32>,
    // ambient, self shadow, 1 / noise cell, noise share
    shape: vec4<f32>,
    // far, step curve, soft, trough
    march: vec4<f32>,
    // steps, frame, unused, unused
    counts: vec4<u32>,
    // inner radius, outer radius, aerial max distance, aerial slices
    atmosphere: vec4<f32>,
    // multiple-scattering table size (bound, not read), sky-view table size
    lut_sizes: vec4<f32>,
    // shadow strength, air floor extinction per metre, the world height it
    // is full up to, the metres it thins by e over above that
    lighting: vec4<f32>,
    // the cloud shadow: 1 / tile, that times the sun's run in x and in z per
    // metre climbed, 1 while the sun carries the mask
    clouds: vec4<f32>,
    // the mask's offset in u and v, its mean (the air floor's share), unused
    cloud_shift: vec4<f32>,
}

// The head of Bevy 0.19.1's `Lights` (bevy_pbr `render/mesh_view_types.wgsl`,
// `GpuLights` in `render/light.rs`), with the array sizes its shader defs
// give on desktop. Only the directional lights are read: the sun's cascades.
// A light is declared whole so the array's stride is Bevy's; fog_pass.rs's
// tests check both against Bevy's own types.
const MAX_DIRECTIONAL_LIGHTS: u32 = 10u;

struct ShadowCascade {
    clip_from_world: mat4x4<f32>,
    texel_size: f32,
    far_bound: f32,
}

struct ShadowLight {
    cascades: array<ShadowCascade, 4u>,
    color: vec4<f32>,
    direction_to_light: vec3<f32>,
    flags: u32,
    soft_shadow_size: f32,
    shadow_depth_bias: f32,
    shadow_normal_bias: f32,
    num_cascades: u32,
    cascades_overlap_proportion: f32,
    depth_texture_base_index: u32,
    decal_index: u32,
    sun_disk_angular_size: f32,
    sun_disk_intensity: f32,
}

struct ShadowLights {
    directional_lights: array<ShadowLight, 10u>,
    ambient_color: vec4<f32>,
    cluster_dimensions: vec4<u32>,
    cluster_factors: vec4<f32>,
    n_directional_lights: u32,
}

// DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT in mesh_view_types.wgsl.
const SHADOWS_ENABLED: u32 = 1u;
const NO_LIGHT: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> fog: FogPass;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var transmittance_lut: texture_2d<f32>;
@group(0) @binding(4) var multiscattering_lut: texture_2d<f32>;
@group(0) @binding(5) var aerial_view_lut: texture_3d<f32>;
@group(0) @binding(6) var lut_sampler: sampler;
@group(0) @binding(7) var fine_level: texture_2d<f32>;
@group(0) @binding(8) var coarse_level: texture_2d<f32>;
@group(0) @binding(9) var noise_texture: texture_2d<f32>;
@group(0) @binding(10) var noise_sampler: sampler;
@group(0) @binding(11) var<uniform> lights: ShadowLights;
@group(0) @binding(12) var directional_shadow_textures: texture_depth_2d_array;
@group(0) @binding(13) var shadow_sampler: sampler_comparison;
@group(0) @binding(14) var sky_view_lut: texture_2d<f32>;
@group(0) @binding(15) var cloud_mask: texture_2d<f32>;
@group(0) @binding(16) var cloud_sampler: sampler;

const NOISE_TEXELS: f32 = 256.0;

const FRAC_4_PI: f32 = 0.07957747;
const PI: f32 = 3.14159265;
const HALF_PI: f32 = 1.57079633;
// Half of the phase is the forward lobe, half isotropic. The isotropic half
// carries the gain `light`: it stands for the light a dense fog scatters
// more than once, which evens the lobe out, so the gain must not brighten
// the forward peak (it did, and a dawn fog seen towards the sun glowed
// brighter than the sky round the sun).
const FORWARD_SHARE: f32 = 0.5;
// The share of each level's side over which it fades out at its edges: the
// fine level hands over to the coarse one across this band, and the coarse
// one fades to no fog.
const FINE_BAND: f32 = 0.12;
const COARSE_BAND: f32 = 0.06;
// Rec. 709 luminance weights.
const LUMINANCE: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// The sun's share of a step's light under which its shadow is not read: a
// shadow there would change the step by under this share.
const SHADOW_SHARE: f32 = 0.03;
// What is left to see through behind which the shadow is not read again.
const SHADOW_THROUGHPUT: f32 = 0.1;
// A column whose density is under this is no fog.
const TRACE: f32 = 0.001;
// How far towards the sun the relief is read again for the slope, metres.
const SLOPE_STEP: f32 = 12.0;
// Reads of the clipmap alone along each ray that find where the fog can be.
const SEARCH: u32 = 12u;
// Metres above a coarse texel's top the search still counts as fog: the
// coarse texel's top is a mean over 250 m, under the highest top in it.
const SEARCH_MARGIN: f32 = 10.0;

// ---- noise ---------------------------------------------------------------

// Value noise from one filtered read of the noise texture: the read is
// moved inside its cell along a smoothstep, which makes the hardware's
// bilinear blend the smooth one. The texture repeats every 256 cells, which
// is the period the render origin is wrapped to.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    return textureSampleLevel(noise_texture, noise_sampler, (i + u + 0.5) / NOISE_TEXELS, 0.0).r;
}

// The top's relief in [-1, 1]: broad swells, billows a quarter their size on
// them and an eighth for the texture near the camera. `xz` is world metres
// modulo the period. A height field, so a trough is a trough all the way
// down its column and the light that reaches a column can follow the shape
// of the surface over it.
fn top_noise(xz: vec2<f32>) -> f32 {
    let p = xz * fog.shape.z;
    let n1 = vnoise(p);
    let n2 = vnoise(p * 4.0 + vec2<f32>(11.3, 7.7));
    let n3 = vnoise(p * 8.0 + vec2<f32>(3.7, 19.1));
    return (0.5 * n1 + 0.32 * n2 + 0.18 * n3) * 2.0 - 1.0;
}

// ---- the field -----------------------------------------------------------

struct Column {
    top: f32,
    base: f32,
    density: f32,
    // How far the noise moves the top here, metres.
    amp: f32,
}

// 1 inside the unit square less `band` from its edges, falling to 0 at them.
fn inside(uv: vec2<f32>, band: f32) -> f32 {
    let e = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y));
    return smoothstep(0.0, band, e);
}

// The field's layer under a render-space point: the fine level where it
// covers, the coarse beyond, blended over a band, and no fog past the
// coarse level's edge.
fn column(xz: vec2<f32>) -> Column {
    var s = vec4<f32>(0.0);
    var w_fine = 0.0;
    if fog.fine.w > 0.5 {
        let uv = (xz - fog.fine.xy) * fog.fine.z;
        w_fine = inside(uv, FINE_BAND);
        if w_fine > 0.0 {
            s = textureSampleLevel(fine_level, lut_sampler, uv, 0.0);
        }
    }
    if w_fine < 1.0 {
        var c = vec4<f32>(0.0);
        if fog.coarse.w > 0.5 {
            let uv = (xz - fog.coarse.xy) * fog.coarse.z;
            let w = inside(uv, COARSE_BAND);
            if w > 0.0 {
                c = textureSampleLevel(coarse_level, lut_sampler, uv, 0.0);
                c.z *= w;
            }
        }
        s = mix(c, s, w_fine);
    }
    var col: Column;
    col.top = s.x;
    col.base = s.y;
    col.density = s.z;
    col.amp = clamp(fog.shape.w * (s.x - s.y), 1.0, fog.slab.w);
    return col;
}

// ---- the medium ----------------------------------------------------------

struct Medium {
    // Extinction per metre.
    extinction: f32,
    // The local top, the noise applied, metres.
    top: f32,
    // The noise, -1 in a trough to 1 on a crest.
    n: f32,
}

// The falloff through the local top. The 0.35 is `SOFT_SHARE` in
// fog_pass.rs, whose band (`slab.y`) assumes it; change both together.
fn soft_of(col: Column) -> f32 {
    return max(fog.march.z, 0.35 * col.amp);
}

fn medium(col: Column, world_xz: vec2<f32>, world_y: f32) -> Medium {
    var m: Medium;
    m.extinction = 0.0;
    m.top = col.top;
    m.n = 0.0;
    let soft = soft_of(col);
    if col.density <= TRACE || world_y < max(col.base, fog.slab.z) || world_y > col.top + col.amp + soft {
        return m;
    }
    m.n = top_noise(world_xz);
    m.top = col.top + col.amp * m.n;
    let fade = 1.0 - smoothstep(m.top - soft, m.top + soft, world_y);
    m.extinction = fog.medium.x * col.density * fade;
    return m;
}

// ---- the atmosphere's tables ---------------------------------------------
//
// These lookups read Bevy's own atmosphere tables, so they must map (r, mu)
// to a texel exactly as Bevy wrote it. They are copied from bevy_pbr 0.19.1,
// `src/atmosphere/bruneton_functions.wgsl` and `src/atmosphere/functions.wgsl`,
// and a Bevy upgrade must diff them against the new version:
//
//   distance_to_top        bruneton_functions  distance_to_top_atmosphere_boundary
//   ray_intersects_ground  bruneton_functions  ray_intersects_ground
//   transmittance_uv       bruneton_functions  transmittance_lut_r_mu_to_uv
//   transmittance_segment  functions           sample_transmittance_lut_segment
//                                              (with get_local_r inlined)
//   sky_view_uv            functions           sky_view_lut_r_mu_azimuth_to_uv
//                                              and unit_to_sub_uvs (exact acos
//                                              for fast_acos_4)
//   aerial                 functions           sample_aerial_view_lut, times the
//                                              view's exposure, which Bevy applies
//                                              where it composites the haze
//
// Bevy's atmosphere uniform is not bound here: its radii and table sizes come
// through this pass's own uniform (`atmosphere`, `lut_sizes`), filled from
// the camera's `AtmosphereSettings` in fog_pass.rs. The multiple-scattering
// table is still bound (binding 4) but no longer read: the sky's light comes
// from the sky-view table instead.
//
// The transmittance mapping and segment are Eric Bruneton's and INRIA's, from
// "Precomputed Atmospheric Scattering", under the licence Bevy reproduces in
// bruneton_functions.wgsl, retained here as it requires:
//
// Copyright (c) 2017 Eric Bruneton
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
// 3. Neither the name of the copyright holders nor the names of its
//    contributors may be used to endorse or promote products derived from
//    this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
// THE POSSIBILITY OF SUCH DAMAGE.
//
// Precomputed Atmospheric Scattering
// Copyright (c) 2008 INRIA
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
// 3. Neither the name of the copyright holders nor the names of its
//    contributors may be used to endorse or promote products derived from
//    this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
// THE POSSIBILITY OF SUCH DAMAGE.


fn distance_to_top(r: f32, mu: f32) -> f32 {
    let d = max(r * r * (mu * mu - 1.0) + fog.atmosphere.y * fog.atmosphere.y, 0.0);
    return max(-r * mu + sqrt(d), 0.0);
}

fn ray_intersects_ground(r: f32, mu: f32) -> bool {
    return mu < 0.0 && r * r * (mu * mu - 1.0) + fog.atmosphere.x * fog.atmosphere.x >= 0.0;
}

fn transmittance_uv(r: f32, mu: f32) -> vec2<f32> {
    let inner = fog.atmosphere.x;
    let outer = fog.atmosphere.y;
    let big_h = sqrt(outer * outer - inner * inner);
    let rho = sqrt(max(r * r - inner * inner, 0.0));
    let d = distance_to_top(r, mu);
    let d_min = outer - r;
    let d_max = rho + big_h;
    return vec2<f32>((d - d_min) / (d_max - d_min), rho / big_h);
}

fn transmittance(r: f32, mu: f32) -> vec3<f32> {
    return textureSampleLevel(transmittance_lut, lut_sampler, transmittance_uv(r, mu), 0.0).rgb;
}

fn transmittance_segment(r: f32, mu: f32, t: f32) -> vec3<f32> {
    let r_t = sqrt(t * t + 2.0 * r * mu * t + r * r);
    let mu_t = clamp((r * mu + t) / r_t, -1.0, 1.0);
    if ray_intersects_ground(r, mu) {
        return min(transmittance(r_t, -mu_t) / transmittance(r, -mu), vec3<f32>(1.0));
    }
    return min(transmittance(r, mu) / transmittance(r_t, mu_t), vec3<f32>(1.0));
}

// Where the sky-view table keeps the sky's radiance towards zenith cosine
// `mu` and `azimuth` (radians) for an observer at radius `r`. The table is
// in the atmosphere's own frame, turned about the vertical from the world's;
// the only reader here averages over azimuth, so that turn does not matter.
fn sky_view_uv(r: f32, mu: f32, azimuth: f32) -> vec2<f32> {
    let res = fog.lut_sizes.zw;
    let u = azimuth / (2.0 * PI) + 0.5;
    let inner = fog.atmosphere.x;
    let v_horizon = sqrt(max(r * r - inner * inner, 0.0));
    let horizon_zenith = PI - acos(clamp(v_horizon / r, -1.0, 1.0));
    let l = acos(clamp(mu, -1.0, 1.0)) - horizon_zenith;
    let v = 0.5 + 0.5 * sign(l) * sqrt(abs(l) / HALF_PI);
    return (vec2<f32>(u, v) + 0.5 / res) * (res / (res + 1.0));
}

// The air's in-scattered light between the camera and `t` along this
// pixel's ray, exposed.
fn aerial(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let slices = fog.atmosphere.w;
    let w = saturate(t / fog.atmosphere.z - 0.5 / slices);
    let s = textureSampleLevel(aerial_view_lut, lut_sampler, vec3<f32>(uv, w), 0.0);
    let fade = saturate(t / (fog.atmosphere.z / slices));
    return exp(s.rgb) * fade * view.exposure;
}

// ---- light ---------------------------------------------------------------

// The sky's light from above, exposed: the radiance of the upper hemisphere
// weighted by the cosine to the zenith, which is the irradiance a level
// surface gets over pi, the light that enters the fog through its top. Read
// from the sky-view table, so it is the sky the frame draws at this hour:
// bright at noon, dim and blue at dawn, moonlit at night. Two rings of four
// reads, one in each half of the cosine-weighted hemisphere (split at 45
// degrees from the zenith), at that half's middle in sine squared (30 and 60
// degrees), so the eight reads weigh the same; the second ring is turned by
// half a step.
fn sky_light(r: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var ring = 0u; ring < 2u; ring++) {
        let mu = select(0.5, 0.8660254, ring == 0u);
        for (var k = 0u; k < 4u; k++) {
            let azimuth = (f32(k) + 0.5 * f32(ring)) * HALF_PI - PI;
            let uv = sky_view_uv(r, mu, azimuth);
            sum += textureSampleLevel(sky_view_lut, lut_sampler, uv, 0.0).rgb;
        }
    }
    return sum * 0.125 * view.exposure;
}

// The sun's entry in Bevy's directional lights, found by its direction (Bevy
// sorts them by its own key, and the moon is one too), or `NO_LIGHT` when it
// casts no shadow map in this view.
fn sun_light_index() -> u32 {
    let n = min(lights.n_directional_lights, MAX_DIRECTIONAL_LIGHTS);
    for (var i = 0u; i < n; i++) {
        let light = &lights.directional_lights[i];
        if dot((*light).direction_to_light, fog.sun_direction.xyz) > 0.9999
            && ((*light).flags & SHADOWS_ENABLED) != 0u
            && (*light).num_cascades > 0u {
            return i;
        }
    }
    return NO_LIGHT;
}

// How much of the sun reaches a render-space point `view_depth` metres in
// front of the camera, from the cascade Bevy's own shading would read there
// (`fetch_directional_shadow` in bevy_pbr's shadows.wgsl), with one
// hardware-compared tap. Where two cascades overlap, `pick` (uniform in 0
// to 1, moved every frame) chooses between them in proportion, so TAA
// averages the seam that a fixed switch would draw as a line in the fog.
// Over the last cascade's overlap band the shadow fades out, and beyond it
// the sun reaches every point: the fog's own shadow is all there is there.
fn sun_visibility(index: u32, p: vec3<f32>, view_depth: f32, pick: f32) -> f32 {
    let light = &lights.directional_lights[index];
    let n = (*light).num_cascades;
    var c = 0u;
    while c < n && view_depth >= (*light).cascades[c].far_bound {
        c++;
    }
    if c >= n {
        return 1.0;
    }
    let far = (*light).cascades[c].far_bound;
    let near_next = (1.0 - (*light).cascades_overlap_proportion) * far;
    let blend = saturate((view_depth - near_next) / max(far - near_next, 1e-3));
    var fade = 0.0;
    if c + 1u < n {
        if pick < blend {
            c++;
        }
    } else {
        fade = blend;
    }
    let cascade = &(*light).cascades[c];
    // Bevy's depth bias, and its normal bias along the light (a point in the
    // air has no normal), so a point just under a lit surface is not
    // shadowed by the surface itself.
    let bias = (*light).shadow_depth_bias + (*light).shadow_normal_bias * (*cascade).texel_size;
    let clip = (*cascade).clip_from_world * vec4<f32>(p + (*light).direction_to_light * bias, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if any(ndc.xy < vec2<f32>(-1.0)) || any(ndc > vec3<f32>(1.0)) || ndc.z < 0.0 {
        return 1.0;
    }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    let layer = i32((*light).depth_texture_base_index + c);
    let lit = textureSampleCompareLevel(directional_shadow_textures, shadow_sampler, uv, layer, ndc.z);
    return mix(lit, 1.0, fade);
}

// How much of the sun the cloud layer lets through to a render-space point:
// the shadow mask where the ray from the point towards the sun crosses the
// layer, the projection Bevy's light texture uses on the ground
// (strata_atmosphere::clouds::fog_shadow), so fog and ground under one
// cloud are shaded alike. One while the sun carries no mask.
fn cloud_light(p: vec3<f32>) -> f32 {
    if fog.clouds.w <= 0.0 {
        return 1.0;
    }
    let s = fog.clouds.x;
    let uv = vec2<f32>(s * p.x - fog.clouds.y * p.y, s * p.z - fog.clouds.z * p.y)
        + fog.cloud_shift.xy;
    return textureSampleLevel(cloud_mask, cloud_sampler, uv, 0.0).r;
}

// The mean over a stretch of the ray of the air's transmittance from the
// camera, given the transmittance to its near end and across it, as for an
// even air: what dims light scattered evenly along the stretch on its way
// to the eye.
fn air_mean(to_start: vec3<f32>, across: vec3<f32>) -> vec3<f32> {
    let tau = -log(clamp(across, vec3<f32>(1e-4), vec3<f32>(1.0)));
    let mean = select((vec3<f32>(1.0) - across) / max(tau, vec3<f32>(1e-6)), vec3<f32>(1.0), tau < vec3<f32>(1e-4));
    return to_start * mean;
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let denom = 1.0 + g * g - 2.0 * g * cos_theta;
    return FRAC_4_PI * (1.0 - g * g) / (denom * sqrt(denom));
}

// The phase in two parts: the forward lobe of light scattered once, and
// the isotropic rest, which stands for the light scattered many times and
// so behaves like the sky's: it is dimmed in the troughs of the top.
fn phase_once(cos_theta: f32) -> f32 {
    return FORWARD_SHARE * henyey_greenstein(cos_theta, fog.medium.z);
}

const PHASE_MANY: f32 = 0.0397887; // (1 - FORWARD_SHARE) / (4 pi)

// What a directional light puts into the air at radius `r`, exposed: its
// illuminance through the atmosphere's transmittance from the top of the
// atmosphere down to that height at its own elevation, so a low sun is dim
// and red, and nothing once the planet stands in the way.
fn direct(r: f32, direction: vec3<f32>, color: vec3<f32>) -> vec3<f32> {
    let mu = direction.y;
    let reaches = select(1.0, 0.0, ray_intersects_ground(r, mu));
    return color * transmittance(r, mu) * reaches * view.exposure;
}

// Interleaved gradient noise, moved every frame for TAA to average.
fn jitter(pixel: vec2<f32>) -> f32 {
    let p = pixel + 5.588238 * f32(fog.counts.y % 64u);
    return fract(52.9829189 * fract(0.06711056 * p.x + 0.00583715 * p.y));
}

// The distances along a ray at which it is inside the box between `lo` and
// `hi`, as (enter, leave); enter > leave when it misses.
fn clip_axis(o: f32, d: f32, lo: f32, hi: f32, range: vec2<f32>) -> vec2<f32> {
    if abs(d) < 1e-6 {
        if o < lo || o > hi {
            return vec2<f32>(1.0, 0.0);
        }
        return range;
    }
    let a = (lo - o) / d;
    let b = (hi - o) / d;
    return vec2<f32>(max(range.x, min(a, b)), min(range.y, max(a, b)));
}

// Whether the fog could reach a height over a render-space point, from the
// coarse level alone (one read, where the fine level would blend two): its
// 250 m texels blur a small hollow's layer out over its neighbours, which
// errs towards searching where there is nothing, and its tops get the
// noise's most and a margin besides.
fn could_hold(xz: vec2<f32>, world_y: f32) -> bool {
    var level = fog.coarse;
    if level.w < 0.5 {
        level = fog.fine;
    }
    let uv = (xz - level.xy) * level.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return false;
    }
    var s: vec4<f32>;
    if fog.coarse.w < 0.5 {
        s = textureSampleLevel(fine_level, lut_sampler, uv, 0.0);
    } else {
        s = textureSampleLevel(coarse_level, lut_sampler, uv, 0.0);
    }
    // The noise's most (1) and the falloff over it (`SOFT_SHARE`, 0.35, in
    // fog_pass.rs), as the band there assumes.
    let reach = fog.slab.w * 1.35 + fog.march.z + SEARCH_MARGIN;
    return s.z > TRACE && world_y >= max(s.y, fog.slab.z) && world_y <= s.x + reach;
}

// The stretch of `range` that can hold fog, as (enter, leave), from `SEARCH`
// reads of the clipmap spaced as the square of their index: from the read
// before the first that could hold fog to the read after the last.
fn find_fog(camera: vec3<f32>, dir: vec3<f32>, cam_y: f32, range: vec2<f32>, j: f32) -> vec2<f32> {
    let len = range.y - range.x;
    var enter = range.y;
    var leave = range.x;
    var prev = range.x;
    for (var k = 0u; k <= SEARCH; k++) {
        // The last read is at the end of the range, where the ray meets
        // the ground under the fog; the others are jittered.
        var x = 1.0;
        if k < SEARCH {
            x = (f32(k) + j) / f32(SEARCH);
            x = x * x;
        }
        let t = range.x + len * x;
        let p = camera + dir * t;
        if could_hold(p.xz, cam_y + dir.y * t) {
            enter = min(enter, prev);
            let x_next = min((f32(k + 1u) + j) / f32(SEARCH), 1.0);
            leave = max(leave, range.x + len * x_next * x_next);
        }
        prev = t;
    }
    return vec2<f32>(enter, min(leave, range.y));
}

// ---- the air floor ---------------------------------------------------------

// The air floor's optical depth between distances `a` and `b` along a ray
// from world height `y0` climbing `dy` per metre: full extinction up to its
// height, thinning by e over its falloff above it. In closed form, split
// where the ray crosses the floor's height; the part above is
// `sigma falloff (e(a) - e(b)) / dy` with `e` the floor's thinning at each
// end, taken as `sigma e(a) length (1 - x / 2)` where it climbs too few
// falloff lengths `x` for the difference to keep its digits (and for a
// level ray).
fn floor_depth(y0: f32, dy: f32, a: f32, b: f32) -> f32 {
    let sigma = fog.lighting.y;
    let h0 = fog.lighting.z;
    let falloff = fog.lighting.w;
    if b <= a {
        return 0.0;
    }
    // The stretch under the floor's height and the stretch over it.
    var under = vec2<f32>(a, a);
    var over = vec2<f32>(a, b);
    if abs(dy) > 1e-6 {
        let cross = (h0 - y0) / dy;
        if dy > 0.0 {
            under = vec2<f32>(a, clamp(cross, a, b));
            over = vec2<f32>(under.y, b);
        } else {
            over = vec2<f32>(a, clamp(cross, a, b));
            under = vec2<f32>(over.y, b);
        }
    } else if y0 <= h0 {
        under = vec2<f32>(a, b);
        over = vec2<f32>(b, b);
    }
    var tau = sigma * (under.y - under.x);
    let len = over.y - over.x;
    if len > 0.0 {
        let e_start = exp(-max(y0 + dy * over.x - h0, 0.0) / falloff);
        let x = dy * len / falloff;
        if abs(x) > 1e-3 {
            let e_end = exp(-max(y0 + dy * over.y - h0, 0.0) / falloff);
            tau += sigma * falloff * (e_start - e_end) / dy;
        } else {
            tau += sigma * e_start * len * (1.0 - 0.5 * x);
        }
    }
    return tau;
}

// What the air floor scatters towards the eye per unit of optical depth: the
// sun and the moon once, through a forward lobe like the fog's, and the
// sky's light. A thin haze between a dark ground and the sky sees about
// half the sky's radiance in the mean over the sphere; it gets no sky fill,
// as the atmosphere's own haze gets none.
fn floor_source(dir: vec3<f32>, sun: vec3<f32>, moon: vec3<f32>, sky: vec3<f32>) -> vec3<f32> {
    let g = fog.medium.z;
    let phase_sun = henyey_greenstein(dot(dir, fog.sun_direction.xyz), g);
    let phase_moon = henyey_greenstein(dot(dir, fog.moon_direction.xyz), g);
    return fog.medium.y * (sun * phase_sun + moon * phase_moon + 0.5 * sky);
}

// The basin fog along one ray: the light it adds (with the air in front of
// it), the share of what is behind that gets through, and the distance the
// air floor is split at.
struct Basin {
    rgb: vec3<f32>,
    throughput: f32,
    t_fog: f32,
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let nothing = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let floor_on = fog.lighting.y > 0.0;
    let basins = fog.slab.y > fog.slab.x;
    if !floor_on && !basins {
        return nothing;
    }
    let pixel = in.position.xy;
    let depth = textureLoad(depth_texture, vec2<i32>(pixel), 0);
    let ndc_xy = in.uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    let camera = view.world_position;

    let near = view.world_from_clip * vec4<f32>(ndc_xy, 1.0, 1.0);
    let dir = normalize(near.xyz / near.w - camera);
    var t_end = fog.march.x;
    if depth > 0.0 {
        let p = view.world_from_clip * vec4<f32>(ndc_xy, depth, 1.0);
        t_end = min(length(p.xyz / p.w - camera), fog.march.x);
    }
    let cam_y = camera.y + fog.origin.y;
    let r_cam = fog.atmosphere.x + max(cam_y, 1.0);

    var basin: Basin;
    basin.rgb = vec3<f32>(0.0);
    basin.throughput = 1.0;
    basin.t_fog = t_end;
    // The sky's light, read once a pixel and only where something uses it.
    var sky = vec3<f32>(0.0);
    var have_sky = false;
    if floor_on {
        sky = sky_light(r_cam);
        have_sky = true;
    }

    // Clip to the band the fog can occupy and to the clipmap's square.
    var range = clip_axis(cam_y, dir.y, fog.slab.x, fog.slab.y, vec2<f32>(0.0, t_end));
    var square = fog.coarse;
    if square.w < 0.5 {
        square = fog.fine;
    }
    let side = 1.0 / square.z;
    range = clip_axis(camera.x, dir.x, square.x, square.x + side, range);
    range = clip_axis(camera.z, dir.z, square.y, square.y + side, range);
    // Where along the ray the fog can be: a cheap search that reads only
    // the clipmap, spaced as the square of the step, for the stretch
    // between the first and the last point under a layer's reach. The
    // march then spends its steps there, not on the kilometres of clear
    // air above the fog that a ray from high ground crosses first.
    var search = vec2<f32>(1.0, 0.0);
    if basins && range.y > range.x {
        search = find_fog(camera, dir, cam_y, range, jitter(pixel + vec2<f32>(17.0, 31.0)));
    }
    let t0 = search.x;
    let t1 = search.y;
    if t1 > t0 {
        if !have_sky {
            sky = sky_light(r_cam);
        }
        let sun_dir = fog.sun_direction.xyz;
        let moon_dir = fog.moon_direction.xyz;
        let once_sun = phase_once(dot(dir, sun_dir));
        let once_moon = phase_once(dot(dir, moon_dir));
        let many = PHASE_MANY * fog.medium.w;
        // The moon at the band's middle height stands for the whole band;
        // the sun is read at each step's own height, where a low sun's
        // light changes most.
        let r_mid = fog.atmosphere.x + max(0.5 * (fog.slab.x + fog.slab.y), 0.0);
        let moon = direct(r_mid, moon_dir, fog.moon_color.rgb);
        let sun_up = max(sun_dir.y, 0.05);
        let sun_flat = length(sun_dir.xz);
        let moon_up = max(moon_dir.y, 0.05);
        // Light scattered many times entered through the layer's top, so
        // there is as much of it as falls on a level surface: the light's
        // illuminance times the cosine of its zenith angle. At noon that is
        // most of the sun; at dawn a tenth of it, and the layer is no longer
        // lit as if by a noon sun.
        let sun_level = max(sun_dir.y, 0.0);
        let moon_level = max(moon_dir.y, 0.0);
        let self_shadow = fog.shape.y;
        let trough = fog.march.w;
        let ambient = sky * fog.shape.x;
        // The world's shadow on the sun's light: the sun's entry in Bevy's
        // lights, and the camera's forward axis, which the cascades' bounds
        // are distances along.
        let strength = fog.lighting.x;
        var sun_index = NO_LIGHT;
        if strength > 0.0 {
            sun_index = sun_light_index();
        }
        let forward = -view.world_from_view[2].xyz;
        let ahead = dot(dir, forward);

        let steps = fog.counts.x;
        let length_clipped = t1 - t0;
        let j = jitter(pixel);
        var throughput = 1.0;
        var light = vec3<f32>(0.0);
        var weight = 0.0;
        var weighted_t = 0.0;
        var prev = 0.0;
        // The world's shadow at the last step that read it, and which of
        // the steps read it this frame.
        var seen = 1.0;
        let parity = fog.counts.y & 1u;
        for (var i = 0u; i < steps; i++) {
            let x_next = pow(f32(i + 1u) / f32(steps), fog.march.y);
            let seg_start = t0 + length_clipped * prev;
            let seg_len = length_clipped * (x_next - prev);
            let x_sample = pow((f32(i) + j) / f32(steps), fog.march.y);
            let t = t0 + length_clipped * x_sample;
            prev = x_next;

            let p = camera + dir * t;
            let world_xz = p.xz + fog.origin.xz;
            let world_y = p.y + fog.origin.y;
            let col = column(p.xz);
            let m = medium(col, world_xz, world_y);
            if m.extinction <= 0.0 {
                continue;
            }
            let full = fog.medium.x * col.density;
            let under = max(m.top - world_y, 0.0);
            // The sun's way out of the fog. Deep in the layer the fog over the
            // point stands for it; near the top a second read a step along the
            // sun finds whether a billow of the top stands in the way, whatever
            // the sun's height, which is what shades the top's relief.
            let tau_sun = full * under / sun_up * self_shadow;
            // How the top over this column faces the sun, from a second read of
            // the relief a step towards it: the side of a billow that faces the
            // sun takes more of its light and the lee side less, whatever the
            // sun's height (at noon by a few tens of percent, at dawn until the
            // lee side is in the billow's shadow). Only near the top, where the
            // light that leaves the fog enters it.
            var facing = 1.0;
            if under < col.amp + soft_of(col) && sun_flat > 0.01 {
                let toward = fog.sun_direction.xz / sun_flat;
                let n_q = top_noise(world_xz + toward * SLOPE_STEP);
                let slope = col.amp * (n_q - m.n) / SLOPE_STEP;
                facing = clamp((sun_up - slope * sun_flat) / sun_up, 0.0, 2.0);
            }
            // The sky's light from above: dimmed by the fog over the point and
            // by how far down a trough of the top it lies, where less of the
            // sky is in view.
            let tau_up = full * under * self_shadow;
            let open_sky = mix(trough, 1.0, 0.5 + 0.5 * m.n);
            let scatter = m.extinction * fog.medium.y;
            // Light scattered once comes straight from the sun, through all
            // the fog along the sun's path. Light scattered many times has
            // forgotten where it came from: it enters through the top and
            // spreads down, so it is dimmed by the fog over the point, not by
            // the long slant to a low sun (which left a hollow's fog black at
            // dawn), and it follows the top's facing only by half.
            let over = exp(-tau_up);
            let sun_weight = exp(-tau_sun) * facing * once_sun
                + (0.5 + 0.5 * facing) * sun_level * many * over * open_sky;
            // The sun at this height, and what the world's shadow leaves of
            // it: one tap of the cascade this step's view depth falls in.
            // The tap is the pass's dearest read (its texels are scattered
            // over a large map), so it is taken on every other step only,
            // the steps alternating by frame for TAA to blend (not by pixel:
            // neighbours that disagree run the tap in one warp anyway), and
            // skipped where it cannot be seen: where the sun gives this step
            // under `SHADOW_SHARE` of what the sky gives it, and once what is
            // left to see through is under `SHADOW_THROUGHPUT`. The last
            // tap's answer stands in for a step that takes none.
            var sun = direct(fog.atmosphere.x + max(world_y, 0.0), sun_dir, fog.sun_color.rgb)
                * cloud_light(p);
            if sun_index != NO_LIGHT {
                let lum_sun = dot(sun, LUMINANCE) * sun_weight;
                let lum_sky = dot(ambient, LUMINANCE) * over * open_sky;
                let turn = ((i + parity) & 1u) == 0u || i == 0u;
                if turn && throughput > SHADOW_THROUGHPUT && lum_sun > SHADOW_SHARE * lum_sky {
                    let pick = fract(j * 7.31 + f32(i) * 0.618034);
                    seen = mix(1.0, sun_visibility(sun_index, p, t * ahead, pick), strength);
                }
                sun *= seen;
            }
            let sun_once = sun * exp(-tau_sun) * facing;
            let moon_once = moon * exp(-tau_up / moon_up);
            let spread = (sun * (0.5 + 0.5 * facing) * sun_level + moon * moon_level) * many;
            let source = scatter * (
                sun_once * once_sun
                + moon_once * once_moon
                + (spread + ambient) * over * open_sky
            );
            let step_t = exp(-m.extinction * seg_len);
            let gained = throughput * source * (1.0 - step_t) / m.extinction;
            light += gained;
            let w = dot(gained, LUMINANCE);
            weight += w;
            weighted_t += w * (seg_start + 0.5 * seg_len);
            throughput *= step_t;
            if throughput < 0.002 {
                throughput = 0.0;
                break;
            }
        }

        // The air in front of the fog: its transmittance over the fog's light,
        // and its own light where the fog hides what was behind.
        let t_fog = select(t0, weighted_t / max(weight, 1e-12), weight > 0.0);
        let air = transmittance_segment(r_cam, dir.y, t_fog);
        basin.rgb = light * air + aerial(in.uv, t_fog) * (1.0 - throughput);
        basin.throughput = throughput;
        if throughput < 1.0 {
            basin.t_fog = t_fog;
        }
    }

    if !floor_on {
        return vec4<f32>(basin.rgb, basin.throughput);
    }
    // The air floor over the whole ray, split where the basin fog lies: the
    // part in front lies over the fog, the part behind it under the fog.
    // A ray that meets nothing (the sky, and the sea, which writes no depth)
    // ends where it meets the sea, or where the planet's curve has lifted
    // it out of the floor (two falloffs over its height), which a flat world
    // never would.
    var t_floor = t_end;
    if depth <= 0.0 {
        let lift = max(fog.lighting.z + 2.0 * fog.lighting.w - min(cam_y, fog.lighting.z), fog.lighting.w);
        t_floor = min(t_floor, sqrt(2.0 * fog.atmosphere.x * lift));
        if dir.y < 0.0 {
            t_floor = min(t_floor, max(cam_y - fog.slab.z, 0.0) / -dir.y);
        }
    }
    let t_split = min(basin.t_fog, t_floor);
    let tau_front = floor_depth(cam_y, dir.y, 0.0, t_split);
    let tau_back = floor_depth(cam_y, dir.y, t_split, t_floor);
    let r_floor = fog.atmosphere.x + max(fog.lighting.z, 0.0);
    // The sun through the clouds' mean: the floor's light is gathered over
    // kilometres, many clouds and gaps.
    let source = floor_source(
        dir,
        direct(r_floor, fog.sun_direction.xyz, fog.sun_color.rgb) * fog.cloud_shift.z,
        direct(r_floor, fog.moon_direction.xyz, fog.moon_color.rgb),
        sky,
    );
    // The floor's light reaches the eye through the air in front of it, as
    // the atmosphere's own in-scattering does (the basin fog's light is
    // dimmed the same way above): the air's mean transmittance over each of
    // the two stretches, the floor being near even along them.
    let air_split = transmittance_segment(r_cam, dir.y, t_split);
    let air_end = transmittance_segment(r_cam, dir.y, t_floor);
    let seen_front = air_mean(vec3<f32>(1.0), air_split);
    let seen_back = air_mean(air_split, air_end / max(air_split, vec3<f32>(1e-4)));
    let front = exp(-tau_front);
    let back = exp(-tau_back);
    let rgb = source * seen_front * (1.0 - front)
        + front * (basin.rgb + basin.throughput * source * seen_back * (1.0 - back));
    return vec4<f32>(rgb, front * basin.throughput * back);
}
