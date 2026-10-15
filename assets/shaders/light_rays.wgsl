// Screen-space light rays (strata_atmosphere::light_rays).
//
// `march`, at half resolution: from each pixel towards the sun's position on
// screen in `steps` steps, summing a glow around the sun wherever the depth
// buffer holds sky above the horizon, each step weighted `decay` times the
// one before, so what stands between the pixel and the sun carves a shadow
// into its ray. The sum is normalised by the weights, so the step count
// changes the quality and not the brightness. The rays take the sun's light
// through the atmosphere's transmittance at the camera, and are scaled by how
// much of the sun's neighbourhood on screen is open sky, so a sun behind a
// trunk throws none.
//
// `composite`, at full resolution: a small tent over the march's texture,
// blended additively over the view.
//
// The dither is fixed per pixel: the pass runs after TAA, so a pattern that
// moved per frame would shimmer rather than average away.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// Mirrors strata_atmosphere::light_rays::LightRays field for field.
struct LightRays {
    // sun uv (y down), strength, viewport aspect
    sun: vec4<f32>,
    // colour at full strength before transmittance, decay per step
    color: vec4<f32>,
    // inner radius, outer radius, camera's distance from the centre, sun's mu
    atmosphere: vec4<f32>,
    // horizon line (above where dot(xyz, (u, v, 1)) > 0), glow radius
    horizon: vec4<f32>,
    // steps, unused, unused, unused
    counts: vec4<u32>,
}

@group(0) @binding(0) var<uniform> rays: LightRays;
@group(0) @binding(1) var depth_texture: texture_depth_2d;
@group(0) @binding(2) var transmittance_lut: texture_2d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;

// The composite's own bindings, numbered after the march's: one module holds
// both entry points and its bindings must not collide.
@group(0) @binding(4) var rays_texture: texture_2d<f32>;
@group(0) @binding(5) var rays_sampler: sampler;

// The transmittance table's mapping is Bevy's
// (`bruneton_functions::transmittance_lut_r_mu_to_uv`), which a Bevy upgrade
// must diff against the new version. It is Eric Bruneton's and INRIA's, from
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
    let d = max(r * r * (mu * mu - 1.0) + rays.atmosphere.y * rays.atmosphere.y, 0.0);
    return max(-r * mu + sqrt(d), 0.0);
}

fn transmittance(r: f32, mu: f32) -> vec3<f32> {
    let inner = rays.atmosphere.x;
    let outer = rays.atmosphere.y;
    let big_h = sqrt(outer * outer - inner * inner);
    let rho = sqrt(max(r * r - inner * inner, 0.0));
    let d = distance_to_top(r, mu);
    let d_min = outer - r;
    let d_max = rho + big_h;
    let uv = vec2<f32>((d - d_min) / (d_max - d_min), rho / big_h);
    return textureSampleLevel(transmittance_lut, lut_sampler, uv, 0.0).rgb;
}

// 1 where the depth buffer holds sky above the horizon at `uv`, else 0.
// Bevy's depth is reverse z with an infinite far plane: the sky is 0.
fn sky(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_texture));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let depth = textureLoad(depth_texture, texel, 0);
    let above = dot(rays.horizon.xyz, vec3<f32>(uv, 1.0)) > 0.0;
    return select(0.0, 1.0, depth <= 0.0 && above);
}

// The halo the rays carry, distances in screen heights: an inverse-square
// core a few sun widths across, many times the lit ground's brightness, as
// the sun and the bright sky about it are, so a gap in the leaves a dozen
// pixels wide still throws a ray across the frame; cut to nothing at the
// glow's radius, so open sky far from the sun adds none.
const CORE: f32 = 0.03;
const GAIN: f32 = 8.0;
fn glow(uv: vec2<f32>) -> f32 {
    let offset = (uv - rays.sun.xy) * vec2<f32>(rays.sun.w, 1.0);
    let d = length(offset);
    let q = d / CORE;
    let t = clamp(1.0 - d / rays.horizon.w, 0.0, 1.0);
    return GAIN * t * t / (1.0 + q * q);
}

// Interleaved gradient noise: a fixed per-pixel offset for the march's
// first step, which turns the steps' bands into a fine grain.
fn dither(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(0.06711056 * pixel.x + 0.00583715 * pixel.y));
}

// How much of the sun's neighbourhood on screen is open sky: the centre and
// two rings of eight, out to a twelfth of the screen's height, so a sun
// glimpsed through leaves throws rays in proportion and a swaying leaf does
// not switch them on and off.
fn visibility() -> f32 {
    var open = sky(clamp(rays.sun.xy, vec2<f32>(0.0), vec2<f32>(1.0)));
    for (var i = 0u; i < 8u; i++) {
        let angle = f32(i) * 0.7853982;
        let direction = vec2<f32>(cos(angle) / rays.sun.w, sin(angle));
        for (var ring = 1u; ring <= 2u; ring++) {
            let at = rays.sun.xy + direction * (0.04 * f32(ring));
            open += sky(clamp(at, vec2<f32>(0.0), vec2<f32>(1.0)));
        }
    }
    return open / 17.0;
}

@fragment
fn march(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let steps = max(rays.counts.x, 1u);
    let step = (rays.sun.xy - in.uv) / f32(steps);
    let start = dither(in.position.xy);
    let decay = rays.color.w;
    var weight = 1.0;
    var total = 0.0;
    var sum = 0.0;
    for (var i = 0u; i < steps; i++) {
        let at = in.uv + step * (f32(i) + start);
        let on_screen = all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0));
        if on_screen {
            sum += weight * sky(at) * glow(at);
        }
        total += weight;
        weight *= decay;
    }
    let sun = transmittance(rays.atmosphere.z, rays.atmosphere.w) * rays.color.rgb;
    let shafts = sum / max(total, 1e-6);
    let open = mix(0.25, 1.0, visibility());
    return vec4<f32>(sun * (shafts * rays.sun.z * open), 1.0);
}

// Four bilinear reads a little under a texel off each diagonal: a tent over
// the march's three by three texels, which smooths the dither's grain that
// the pass cannot leave to TAA.
@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = 0.75 / vec2<f32>(textureDimensions(rays_texture));
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < 4u; i++) {
        let corner = vec2<f32>(f32(i & 1u), f32(i >> 1u)) * 2.0 - 1.0;
        sum += textureSampleLevel(rays_texture, rays_sampler, in.uv + corner * texel, 0.0).rgb;
    }
    return vec4<f32>(sum * 0.25, 0.0);
}
