// The water's look, shared by the sea (`water.wgsl`) and the rivers and
// lakes (`river_water.wgsl`, ticket 0005 step 3), so there is one water and
// not two: the parameter buffer and the wave image both materials bind, the
// floor behind the surface from the depth prepass, the foam's fray, and the
// lighting (Bevy's StandardMaterial lighting for the body and the sun's
// glitter, the sky's reflection read here, Schlick's Fresnel, the air in
// front). What each surface brings is its own normal, roughness, foam and
// absorption; `water.wgsl` says why each piece is as it is.

#define_import_path strata::water_common

#import bevy_pbr::{
    mesh_view_bindings as view_bindings,
    pbr_functions::apply_pbr_lighting,
    pbr_types::PbrInput,
    view_transformations::{frag_coord_to_ndc, ndc_to_frag_coord, position_ndc_to_world, position_world_to_ndc},
}

#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

#ifdef ATMOSPHERE
#import bevy_pbr::atmosphere::bruneton_functions::transmittance_lut_r_mu_to_uv
#endif

// Mirrors strata_terrain::water::WaterParams field for field. A buffer the
// render world rewrites each frame, not a uniform in the material: a
// material's uniform changing every frame re-prepared the whole material.
struct WaterParams {
    // Absorption per metre in x, the deepest water's opacity in y, the
    // look's reflection in z and its foam in w.
    optics: vec4<f32>,
    // The swell's frame: texture offset in xy, one over its tile in metres
    // in z, its steepest slope in w.
    swell: vec4<f32>,
    // The chop's frame, likewise.
    chop: vec4<f32>,
    // The swell's unit heading in xy, the chop's in zw.
    axes: vec4<f32>,
    // The foam noise's frame in xyz; the shore band's depth in metres in w.
    foam: vec4<f32>,
    // Where the waves start and finish fading in xy, the far roughness in
    // z, the foam's albedo in w.
    fade: vec4<f32>,
    // The crest's foam threshold in x; how far from the land the shore's
    // foam reaches, metres, in y.
    crest: vec4<f32>,
    // The rivers' flow map: the render origin's x and z wrapped to the
    // ripples' tile in xy, one over that tile in z, the flow cycle's phase
    // (0 to 1) in w.
    flow: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage, read> water: WaterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var wave_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var wave_sampler: sampler;

// The lowest a reflected ray or a haze read looks, as the sine of its
// elevation (about two degrees).
const HORIZON_LIFT: f32 = 0.035;
// The perceptual roughness the sky's reflection is read at: the near
// water's, so the far water's raised roughness widens the sun's lobe but
// not the sky's.
const NEAR_BLUR: f32 = 0.12;

fn perp(d: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-d.y, d.x);
}

// A layer's texture coordinate at render-space (x, z): the point in the
// layer's heading's frame, over its tile, plus the CPU's wrapped offset.
fn frame_uv(xz: vec2<f32>, heading: vec2<f32>, frame: vec4<f32>) -> vec2<f32> {
    return vec2<f32>(dot(xz, heading), dot(xz, perp(heading))) * frame.z + frame.xy;
}

// Bevy's own rotation of a direction by the environment map's quaternion
// (`environment_map::quat_rotate`), here so the module is not imported for
// one function.
fn quat_rotate(q: vec4<f32>, dir: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, dir);
    return dir + q.w * t + cross(q.xyz, t);
}

// The sky's radiance towards `dir`, unexposed, from the view's environment
// map at the mip Bevy would read for this roughness, at intensity one.
fn sky_radiance(dir: vec3<f32>, perceptual_roughness: f32) -> vec3<f32> {
#ifdef ENVIRONMENT_MAP
    if view_bindings::light_probes.view_cubemap_index < 0 {
        return vec3<f32>(0.0);
    }
    var d = quat_rotate(view_bindings::light_probes.view_rotation, dir);
    // Cube maps are left-handed.
    d.z = -d.z;
    let level = perceptual_roughness
        * f32(view_bindings::light_probes.smallest_specular_mip_level_for_view);
#ifdef MULTIPLE_LIGHT_PROBES_IN_ARRAY
    return textureSampleLevel(
        view_bindings::specular_environment_maps[view_bindings::light_probes.view_cubemap_index],
        view_bindings::environment_map_sampler,
        d,
        level,
    ).rgb;
#else
    return textureSampleLevel(
        view_bindings::specular_environment_map,
        view_bindings::environment_map_sampler,
        d,
        level,
    ).rgb;
#endif
#else
    return vec3<f32>(0.0);
#endif
}

// The air's transmittance from radius r along mu to the top of the
// atmosphere, or nothing if the ray meets the ground first; as clouds.wgsl
// has it, through Bevy's table.
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

// The air's transmittance between the camera and the water: the ratio of
// two reads of the table along the ray, taken upwards from the lower end.
fn view_transmittance(camera: vec3<f32>, point: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let c = atmosphere_position(camera);
    let p = atmosphere_position(point);
    let r_c = length(c);
    let r_p = length(p);
    let mu_c = dot(v, c / r_c);
    let mu_p = dot(v, p / r_p);
    if mu_c >= 0.0 {
        return min(transmittance(r_c, mu_c) / max(transmittance(r_p, mu_p), vec3<f32>(1e-6)), vec3<f32>(1.0));
    }
    return min(transmittance(r_p, -mu_p) / max(transmittance(r_c, -mu_c), vec3<f32>(1e-6)), vec3<f32>(1.0));
}

// Whether the ground the depth prepass shows where the water's plane point
// `q` lies on screen stands at or over the water's level close to `q`: land
// there. Ground standing in front of the water far from the point (a dune
// between the camera and a bay) is not counted.
fn is_land(q: vec3<f32>, water_y: f32, slack: f32, sample_index: u32) -> bool {
#ifdef DEPTH_PREPASS
    let size = view_bindings::view.viewport;
    let ndc = position_world_to_ndc(q);
    let frag = ndc_to_frag_coord(ndc.xy);
    if any(frag < size.xy) || any(frag >= size.xy + size.zw) {
        return false;
    }
    let d = prepass_depth(vec4<f32>(frag, 0.0, 1.0), sample_index);
    if d <= 0.0 {
        return false;
    }
    let ground = position_ndc_to_world(vec3<f32>(ndc.xy, d));
    return ground.y > water_y - 0.05 && length(ground.xz - q.xz) < slack;
#else
    return false;
#endif
}

// How near land is to the water at `p`, 0 to 1 (one at the water's edge,
// nothing `reach` metres out): along each of the four world axes, a point
// `reach` out on the water's plane is tested for land and, if it is, three
// halvings find the edge between. A coast that drops into deep water at once
// leaves no shallows for the depth's band to draw, so this is what puts
// the foam along such a shore. Open water costs four depth reads, the
// shore twelve to sixteen.
fn land_near(p: vec3<f32>, reach: f32, sample_index: u32) -> f32 {
    var near = 0.0;
    for (var i = 0u; i < 4u; i += 1u) {
        let angle = f32(i) * 1.5707963;
        let heading = vec3<f32>(cos(angle), 0.0, sin(angle));
        if !is_land(p + heading * reach, p.y, reach, sample_index) {
            continue;
        }
        var lo = 0.0;
        var hi = reach;
        for (var j = 0u; j < 3u; j += 1u) {
            let mid = 0.5 * (lo + hi);
            if is_land(p + heading * mid, p.y, reach, sample_index) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        near = max(near, 1.0 - 0.5 * (lo + hi) / reach);
    }
    return near;
}

// What lies behind the surface at fragment `position` (world point `p`):
// the view ray's path through the water to it in x and the water's depth
// straight down there in y, from the depth prepass. Open sky behind the
// surface means the water is as deep as it gets.
fn floor_behind(position: vec4<f32>, p: vec3<f32>, sample_index: u32) -> vec2<f32> {
    var path = 1e6;
    var depth = 1e6;
#ifdef DEPTH_PREPASS
    let scene_depth = prepass_depth(position, sample_index);
    if scene_depth > 0.0 {
        let ndc = frag_coord_to_ndc(position);
        let floor_position = position_ndc_to_world(vec3<f32>(ndc.xy, scene_depth));
        path = max(length(floor_position - p), 0.0);
        depth = max(p.y - floor_position.y, 0.0);
    }
#endif
    return vec2<f32>(path, depth);
}

// The foam of a shore band at `shore` (one at the waterline, nothing at the
// band's edge), frayed by the foam noise: at the waterline every patch of
// the noise foams, further out only its brightest, so the band frays into
// streaks towards open water.
fn shore_foam(shore: f32, noise: f32) -> f32 {
    return smoothstep(0.0, 0.2, noise + 0.3 - (1.0 - shore));
}

// The water lit: the body's light, premultiplied by the opacity the floor
// behind leaves it (`absorption` per metre over `path` metres), the sun's
// glitter and the sky's reflection off normal `N` at `roughness`, `foam`
// over it as a matte white body, and the air in front. `pbr_input` is the
// standard material's with its view vector; the result is premultiplied.
fn water_light(
    pbr_input_in: PbrInput,
    p: vec3<f32>,
    N: vec3<f32>,
    roughness: f32,
    path: f32,
    absorption: f32,
    foam: f32,
) -> vec4<f32> {
    var pbr_input = pbr_input_in;
    let camera = view_bindings::view.world_position;
    // The share of the floor's light that crosses the water.
    let through = 1.0 - water.optics.y * (1.0 - exp(-path * absorption));
    let clear = 1.0 - foam;

    // Schlick from the perturbed normal, with the material's reflectance
    // (already times its specular tint), scaled by the look.
    let V = pbr_input.V;
    let n_dot_v = max(dot(N, V), 0.0);
    let reflectance = pbr_input.material.reflectance;
    let f0 = 0.16 * reflectance * reflectance;
    let fresnel = (f0 + (1.0 - f0) * pow(1.0 - n_dot_v, 5.0)) * water.optics.z * clear;
    let fresnel_mean = dot(fresnel, vec3<f32>(1.0 / 3.0));

    // A reflected ray that dips under the horizon would meet the next wave;
    // the sky just over the horizon stands in for it. Kept a little over
    // it, and read at the near water's blur whatever the roughness: the
    // map under the horizon is the dark inscattering of a ray that meets
    // the ground within metres, and a blurred read there darkens the far
    // water's mirror of the brightest band of the sky.
    var r = reflect(-V, N);
    r.y = max(abs(r.y), HORIZON_LIFT);
    r = normalize(r);
    let sky = sky_radiance(r, NEAR_BLUR) * view_bindings::view.exposure;

    // Bevy lights the body, premultiplied by its own opacity, and the sun's
    // glitter; F0 in Bevy's lobe goes as the square of the reflectance, so
    // the look scales it by its square root.
    let body = pbr_input.material.base_color.rgb * (1.0 - through) * (1.0 - fresnel_mean);
    pbr_input.material.base_color = vec4<f32>(mix(body, vec3<f32>(water.fade.w), foam), 1.0);
    pbr_input.material.reflectance = reflectance * sqrt(water.optics.z) * clear;
    pbr_input.material.perceptual_roughness = roughness;
    pbr_input.N = N;
    pbr_input.specular_occlusion = 0.0;
    let lit = apply_pbr_lighting(pbr_input);

    // The air in front of the water: its light dimmed by the air's
    // transmittance and the haze the air scatters in, taken as the sky
    // just over the horizon in this direction (the inscattering of a path
    // long enough to be saturated) times what the air took away, over the
    // share of the pixel the water covers. What shows through the water
    // carries its own haze already.
    let alpha = 1.0 - through * (1.0 - fresnel_mean) * clear;
    let to_p = normalize(p - camera);
    let air = view_transmittance(camera, p, to_p);
    let horizon = normalize(vec3<f32>(to_p.x, HORIZON_LIFT, to_p.z));
    let haze = sky_radiance(horizon, 0.4) * view_bindings::view.exposure * (1.0 - air);
    return vec4<f32>((lit.rgb + sky * fresnel) * air + haze * alpha, alpha);
}
