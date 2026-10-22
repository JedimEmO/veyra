// A 2D cloud deck of strata_atmosphere::clouds (the lighting plan's
// phase 2b): a horizontal quad over the observer, drawn in the transparent
// pass with premultiplied alpha and no depth write. Two decks share this
// shader, the cumulus at 1.8 km and a thinner, faster one at 2.6 km drawn
// behind it, each with its own uniform.
//
// The cloud is the density image over the cover's threshold, its opacity
// as strata_atmosphere::clouds::opacity has it (the CPU's shadow mask and
// the sun's veil use the same function): the coverage (the image's green
// channel, read in its own frame several tiles across) gates the detail
// density (red) so clouds gather in groups with clear sky between them,
// and widens the edge's ramp and thins the cloud at a group's fringe; every
// edge fades over the same distance by the density's slope (blue), however
// steep the noise is there. A finer read frays the edge; it slides through
// the cloud across the wind (the uniform's relief.yz), so a drifting cloud
// changes shape as well as place.
//
// The volume is faked twice over. The coarse density, over the threshold,
// is a height field (relief.x metres tall at a heart): a single parallax
// step reads the image where the eye's ray meets the field's surface, so a
// base bulges towards the eye, and the field's gradient, from three coarse
// reads round the point, gives the surface a normal. Seen from the side
// the light is on, a face is lit by that normal, wrapped, and shaded by the
// cloud just towards the light; seen from the other side, the light comes
// through the body, diffused by multiple scattering (dim through a deep
// heart, bright through a thin rim) with a Henyey-Greenstein forward lobe
// through the thinnest parts, the silver lining. The sky lights the face
// from above and, under a deck, through it; the sunlit ground bounces a
// touch of its colour onto a base. Every light is reached through the
// atmosphere's transmittance at the cloud's own height, so a low sun lights
// it dim and red. Between the camera and the cloud the air's transmittance
// takes both its light and its opacity, so a far cloud sinks into the sky
// behind it, which already holds the haze. The quad's edge never shows:
// the cloud fades to nothing over the last ten degrees above (or below)
// the horizon, and the footprint of every read widens there, so the band
// does not shimmer.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
}

#ifdef ATMOSPHERE
#import bevy_pbr::atmosphere::bruneton_functions::transmittance_lut_r_mu_to_uv
#endif

// Mirrors strata_atmosphere::clouds::CloudParams field for field.
struct CloudParams {
    // The image's coordinate offset in xy, one over the tile in metres in
    // z, the layer's height in render space in w.
    layer: vec4<f32>,
    // The cover's threshold in x, its ramp in y, half the quad's side in z,
    // the cover in w.
    shape: vec4<f32>,
    // The unit vector towards the sun in xyz.
    sun: vec4<f32>,
    // The ramp's width at a group's fringe over its width in the middle in
    // x, the slope channel's range in y, the edge's fade in tiles in z, the
    // optical depth at a group's fringe over its middle's in w.
    edge: vec4<f32>,
    // Optical depth at a cloud's edge in x and its densest in y, the
    // horizon's band (a sine) in z, the deck's own scale on both in w.
    depth: vec4<f32>,
    // The coverage field's frame, which repeats every few tiles: its
    // coordinate offset in xy, one over its size in metres in z; the share
    // of the detail density a gap between groups keeps in w.
    cover: vec4<f32>,
    // The height field's height at a heart in metres in x, the fraying
    // read's offset in its own repeats in yz, the base's bulge as a share of
    // the height in w.
    relief: vec4<f32>,
    // The sky's share of the light on a deck in xyz, the body's optical
    // depth multiplier in w (assets/look/clouds.ron, light).
    sky: vec4<f32>,
    // The albedo's tint on a face turned up, in xyz.
    top: vec4<f32>,
    // The albedo's tint on a base seen from below, in xyz.
    base: vec4<f32>,
    // The finer read that frays the edge: how far it moves the density in
    // x, its frequency over the image's in y, the share of that it keeps in
    // a group's middle in z.
    fray: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> clouds: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var density_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var density_sampler: sampler;

// The density image's side in texels (clouds::DENSITY_SIZE).
const IMAGE_SIZE: f32 = 512.0;
// A cloud's albedo over pi: what a lit cloud returns per unit of light.
const ALBEDO_OVER_PI: f32 = 0.27;
// The ground's albedo as the base sees it, a mix of grass, rock and sand;
// what the sunlit land bounces up onto a deck's underside.
const GROUND_ALBEDO: vec3<f32> = vec3<f32>(0.16, 0.15, 0.11);
// The share of that bounce that reaches a base: some of the land under a
// deck is in its shadow.
const BOUNCE: f32 = 0.6;
// The finest the shape reads go: mip 3 is 94 m a texel at the default
// 6 km tile, so the shading follows a cloud's mass, not its noise, and a
// big cloud darkens smoothly towards its middle.
const SHAPE_LOD: f32 = 3.0;
// How many mips coarser every read is at the horizon than its footprint
// asks: a cloud a hundred kilometres off is a few pixels of many texels,
// and anisotropic filtering alone left the band shimmering as it drifted.
const GRAZE_BIAS: f32 = 1.0;
// The parallax step: the lift as a share of the face's height, and the
// most the step runs per metre of lift. A single step folds the image over
// itself once its run changes faster than the ground under it, which at a
// heap's edge drew sharp shards; a tenth of the height and a run of at most
// twice the lift keep the fold away.
const PARALLAX: f32 = 0.1;
const PARALLAX_RUN: f32 = 2.0;
// Wrapped diffuse: a cloud scatters light round its curve, so a face turned
// a little from the light is still lit.
const WRAP: f32 = 0.35;
// Multiple scattering through the body: the diffuse transmittance through
// an optical depth tau is about 1 / (1 + MULTIPLE tau), a fifth through a
// heart, not the e^-10 of single scattering.
const MULTIPLE: f32 = 0.2;
// The optical depth of the cloud lying just towards the light, at full
// thickness: the face's self-shadow.
const TOWARD_TAU: f32 = 5.0;
// The body's optical depth top to bottom at a heart, over the alpha's, is
// clouds.sky.w (the look's `body`): the alpha says how much sky a cloud
// hides, this how much light gets through it.
// How much the heaps inside a body darken the light through it: the full
// image's density over the coarse reads' mean, a heap's core over the
// trough beside it, a few hundred metres across.
const BILLOW: f32 = 1.5;
// The silver lining: the forward lobe's asymmetry and strength.
const LINING_G: f32 = 0.7;
const LINING: f32 = 0.5;
// How upright a face seen edge on stands, as the vertical part of its
// normal: a cloud low over the horizon shows its sides, not its base.
const SIDE: f32 = 0.45;

// The air's transmittance from radius r along mu to the top of the
// atmosphere, or nothing if the ray meets the ground first. Bevy's own table
// and mapping, through the atmosphere the mesh view binds.
fn transmittance(r: f32, mu: f32) -> vec3<f32> {
#ifdef ATMOSPHERE
    let atmosphere = view_bindings::atmosphere;
    let inner = atmosphere.inner_radius;
    if mu < 0.0 && r * r * (mu * mu - 1.0) + inner * inner >= 0.0 {
        return vec3<f32>(0.0);
    }
    let uv = transmittance_lut_r_mu_to_uv(atmosphere, r, mu);
    return textureSampleLevel(
        view_bindings::atmosphere_transmittance_texture,
        view_bindings::atmosphere_transmittance_sampler,
        uv,
        0.0,
    ).rgb;
#else
    return vec3<f32>(1.0);
#endif
}

// A render-space position in the atmosphere's frame, centred on the planet.
fn atmosphere_position(p: vec3<f32>) -> vec3<f32> {
#ifdef ATMOSPHERE
    let q = (view_bindings::atmosphere.world_to_atmosphere * vec4<f32>(p, 1.0)).xyz;
    let r = max(length(q), view_bindings::atmosphere.inner_radius + 1.0);
    return normalize(q) * r;
#else
    return vec3<f32>(0.0, 6360000.0 + p.y, 0.0);
#endif
}

// The air's transmittance between the camera and the cloud, after the
// segment in Bevy's functions.wgsl: the ratio of two reads of the table
// along the ray, taken upwards from whichever end is lower.
fn view_transmittance(camera: vec3<f32>, cloud: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let c = atmosphere_position(camera);
    let p = atmosphere_position(cloud);
    let r_c = length(c);
    let r_p = length(p);
    let mu_c = dot(v, c / r_c);
    let mu_p = dot(v, p / r_p);
    if mu_c >= 0.0 {
        return min(transmittance(r_c, mu_c) / max(transmittance(r_p, mu_p), vec3<f32>(1e-6)), vec3<f32>(1.0));
    }
    return min(transmittance(r_p, -mu_p) / max(transmittance(r_c, -mu_c), vec3<f32>(1e-6)), vec3<f32>(1.0));
}

// Henyey-Greenstein, scaled so isotropic scattering is one.
fn forward_lobe(cos_theta: f32, g: f32) -> f32 {
    let denom = 1.0 + g * g - 2.0 * g * cos_theta;
    return (1.0 - g * g) / (denom * sqrt(denom));
}

// The height field's profile, 0 to 1, from the coarse density: zero at the
// threshold, rising steeply at the edge and flattening over the heart, as
// a heap of cumulus does. clouds::relief_height is the CPU's copy.
fn relief_height(density: f32, gate: f32, threshold: f32) -> f32 {
    let m = saturate((density * gate - threshold) / max(1.0 - threshold, 0.05));
    return m * (2.0 - m);
}

// One of the three coarse reads round a point, as a height.
fn shape_at(uv: vec2<f32>, lod: f32, gate: f32, threshold: f32) -> f32 {
    return relief_height(textureSampleLevel(density_texture, density_sampler, uv, lod).r, gate, threshold);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let camera = view_bindings::view.world_position;
    let to_p = p - camera;
    let v = normalize(to_p);
    let s = clouds.layer.z;
    let threshold = clouds.shape.x;
    let thickness = clouds.relief.x;
    // +1 seen from above, -1 from below.
    let side = select(1.0, -1.0, camera.y < clouds.layer.w);

    // The reads that decide the alpha come first, while every invocation
    // still runs, with the footprint the hardware measures (explicit
    // gradients cost NVIDIA's texture units several times a plain read,
    // and three of them cost more than the whole layer once did), widened
    // at the horizon where the fade takes the cloud anyway. The shading's
    // reads after the clear pixels have left name their mip outright, from
    // the same footprint.
    let uv0 = p.xz * s + clouds.layer.xy;
    let cover_uv = p.xz * clouds.cover.z + clouds.cover.xy;
    let bias = GRAZE_BIAS * (1.0 - smoothstep(0.05, 0.3, abs(v.y)));
    let lod = log2(max(length(dpdx(uv0)), length(dpdy(uv0))) * IMAGE_SIZE) + bias;
    let shape_lod = max(lod + 1.5, SHAPE_LOD);

    // The coverage is kilometres across and read where the quad is; the
    // parallax step does not move it far enough to matter.
    let group = textureSampleBias(density_texture, density_sampler, cover_uv, bias).g;
    let gate = mix(clouds.cover.w, 1.0, group);

    // The parallax step: the eye's ray meets the face (the base from below,
    // bulge times the height; the top from above) before the quad, nearer
    // the eye, so the image is read there. Straight up it reads in place;
    // towards the horizon the base is pushed back and compressed, and the
    // step fades out before the band.
    let face = select(1.0, clouds.relief.w, side < 0.0) * thickness * PARALLAX;
    let lift = shape_at(uv0, shape_lod, gate, threshold) * face;
    let lean = -v.xz / max(abs(v.y), 1e-3);
    let lean_len = length(lean);
    let shift = lean * (min(lean_len, PARALLAX_RUN) / max(lean_len, 1e-3)) * lift
        * smoothstep(0.1, 0.25, abs(v.y));
    let uv = uv0 + shift * s;

    let texel = textureSampleBias(density_texture, density_sampler, uv, bias);
    let detail = textureSampleBias(
        density_texture,
        density_sampler,
        uv * clouds.fray.y + vec2<f32>(0.31, 0.67) + clouds.relief.yz,
        bias,
    ).r;

    let base = texel.r * gate;
    let ramp = clouds.shape.y * mix(clouds.edge.x, 1.0, group);
    // The finer read frays the edge, less so in a group's full middle.
    let density = base + (detail - 0.5) * clouds.fray.x * mix(1.0, clouds.fray.z, group);
    let excess = density - threshold;
    let grown = smoothstep(0.0, ramp, excess);
    // How far inside the edge, in tiles: the density over the threshold over
    // its slope.
    let inside = excess / max(texel.b * gate * clouds.edge.y, 1e-3);
    let fade = smoothstep(0.0, clouds.edge.z, inside);
    // How deep into the cloud, from the coarse read alone: the finer one
    // frays the edge but would mottle the body.
    let heart = smoothstep(0.0, 1.0, saturate((base - threshold) / max(1.0 - threshold, 0.05)));

    // Nothing at the horizon, nothing near the quad's edge.
    let horizon = smoothstep(0.0, clouds.depth.z, abs(v.y));
    let rim = 1.0 - smoothstep(0.8, 1.0, length(to_p.xz) / clouds.shape.z);
    let tau = grown * fade * mix(clouds.depth.x, clouds.depth.y, heart)
        * mix(clouds.edge.w, 1.0, group) * clouds.depth.w;
    let alpha = (1.0 - exp(-tau)) * horizon * rim;
    if alpha <= 1e-4 {
        return vec4<f32>(0.0);
    }

    // The shape: three coarse reads round the point, one towards the sun,
    // two a third of a turn either side. Their mean is the height here and
    // their spread its gradient; the one towards the sun is also how much
    // cloud the light crosses to reach this point.
    let sun = clouds.sun.xyz;
    let e0 = sun.xz / max(length(sun.xz), 1e-4);
    let e1 = vec2<f32>(-0.5 * e0.x - 0.866025 * e0.y, 0.866025 * e0.x - 0.5 * e0.y);
    let e2 = vec2<f32>(-0.5 * e0.x + 0.866025 * e0.y, -0.866025 * e0.x - 0.5 * e0.y);
    // A texel and a half of the mip read, and never under 60 m.
    let step_m = max(1.5 * exp2(shape_lod) / (IMAGE_SIZE * s), 60.0);
    let c0 = textureSampleLevel(density_texture, density_sampler, uv + e0 * (step_m * s), shape_lod).r;
    let c1 = textureSampleLevel(density_texture, density_sampler, uv + e1 * (step_m * s), shape_lod).r;
    let c2 = textureSampleLevel(density_texture, density_sampler, uv + e2 * (step_m * s), shape_lod).r;
    let h0 = relief_height(c0, gate, threshold);
    let h1 = relief_height(c1, gate, threshold);
    let h2 = relief_height(c2, gate, threshold);
    // How much cloud stands over the point, linear in the density over the
    // threshold, so it keeps rising into a big cloud's middle where the
    // height has flattened.
    let span = max(1.0 - threshold, 0.05);
    let mass = saturate(((c0 + c1 + c2) * gate / 3.0 - threshold) / span);
    // Least squares over the three: the slope, metres per metre.
    let slope = (h0 * e0 + h1 * e1 + h2 * e2) * (2.0 / (3.0 * step_m)) * thickness;
    // The face the eye sees: the top's normal from above, the base's (its
    // bulge a share of the height) from below, and low in the sky the
    // heap's flanks at their full slope, laid over towards the horizontal:
    // a cloud low over the horizon shows its sides, and the side turned to
    // the sun catches it. (A wall facing the eye, tried in its place, lit
    // every heap alike and flattened the dawn.)
    let edge_on = 1.0 - saturate(abs(v.y) * 3.0);
    let bulge = mix(select(1.0, clouds.relief.w, side < 0.0), 1.0, edge_on);
    let n = normalize(vec3<f32>(
        -slope.x * bulge,
        side * mix(1.0, SIDE, edge_on),
        -slope.y * bulge,
    ));
    // Cloud just towards the light, for the face's self-shadow.
    let toward = saturate((c0 * gate - threshold) / max(3.0 * ramp, 1e-3));
    // A farther cloud between this one and the sun shades it.
    let reach = clamp(150.0 / max(sun.y, 0.05), 100.0, 1500.0);
    let sunward = textureSampleLevel(density_texture, density_sampler, uv + e0 * (reach * s), shape_lod).r;
    let shaded = saturate(sunward * gate - threshold) / max(ramp, 1e-3);
    let other = mix(0.5, 1.0, exp(-1.5 * shaded));
    // The body's optical depth top to bottom.
    let tau_body = clouds.depth.y * clouds.depth.w * mass * clouds.sky.w;
    // The heaps in the body: a core denser than its surroundings passes
    // less light, the trough between two heaps more.
    let billow = saturate(1.0 - BILLOW * (texel.r - (c0 + c1 + c2) / 3.0) * gate / span
        * smoothstep(0.0, 0.3, mass));

    let cloud_r = length(atmosphere_position(p));
    let up = atmosphere_position(p) / cloud_r;
#ifdef ATMOSPHERE
    let ground_r = view_bindings::atmosphere.inner_radius + 1.0;
#else
    let ground_r = 6360001.0;
#endif
    var light = vec3<f32>(0.0);
    for (var i = 0u; i < view_bindings::lights.n_directional_lights; i += 1u) {
        let directional = view_bindings::lights.directional_lights[i];
        let to_light = directional.direction_to_light;
        let mu = dot(to_light, up);
        let reaching = directional.color.rgb * transmittance(cloud_r, mu);
        // A face turned to the light reflects it, wrapped by its normal and
        // dimmed by the cloud the light crosses just before it; one turned
        // away (a base under a high sun) is lit through the body.
        let lit = saturate((dot(n, to_light) + WRAP) / (1.0 + WRAP));
        let reflected = lit / (1.0 + MULTIPLE * TOWARD_TAU * toward);
        // Through the body: the path is longer for a low light and for one
        // that enters through the cloud just towards it.
        let path = tau_body * mix(1.0, 1.6, 1.0 - saturate(abs(mu))) + TOWARD_TAU * toward;
        let through = billow / (1.0 + MULTIPLE * path);
        let lining = forward_lobe(dot(v, to_light), LINING_G) * exp(-path) * LINING;
        let direct = reaching * other * (reflected + (1.0 - lit) * (through + lining));
        // The sky over the deck, on a top or through a base; the sunlit
        // ground under it, on a base only.
        let sky_light = directional.color.rgb * clouds.sky.xyz * saturate(mu * 1.5 + 0.1);
        let sky = sky_light * select(0.6 + 0.4 * n.y, mix(1.0, 0.4, mass), side < 0.0);
        let ground = directional.color.rgb * transmittance(ground_r, mu) * saturate(mu)
            * GROUND_ALBEDO * (BOUNCE * select(0.0, 1.0, side < 0.0));
        light += direct + sky + ground;
    }
    let tint = select(clouds.top.xyz, clouds.base.xyz, side < 0.0);
    let radiance = light * tint * ALBEDO_OVER_PI * view_bindings::view.exposure;

    let air = view_transmittance(camera, p, v);
    let seen = alpha * dot(air, vec3<f32>(1.0 / 3.0));
    return vec4<f32>(radiance * air * alpha, seen);
}
