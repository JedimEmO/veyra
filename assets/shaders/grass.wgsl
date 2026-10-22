// Grass for `strata_foliage::grass::GrassMaterial`: a vertex of the shared
// blade mesh finds its cell by the entity's tag, pulls its record from the
// cell's range of the shared buffer, stands on the ground and bends; the
// fragment is Bevy's standard lighting over the species' tint.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
    forward_io::VertexOutput,
    forward_io::FragmentOutput,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::{PbrInput, STANDARD_MATERIAL_FLAGS_UNLIT_BIT},
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    lighting,
    shadows,
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions
#endif

#import strata::grass_common::{Blade, GrassCellRecord, GrassParams, GrassVertex, PlacedVertex, blade_index, collapsed_vertex, place_vertex}
#import strata::foliage_wind::FoliageWind

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> grass: GrassParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage, read> blades: array<Blade>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<storage, read> wind: array<FoliageWind>;
// Every cell's record; an entity's MeshTag is its cell's slot.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<storage, read> cells: array<GrassCellRecord>;

// The locations of forward_io::VertexOutput for the fields carried, plus
// the tint.
struct GrassVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(6) @interpolate(flat) instance_index: u32,
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    // The species' tint and, in w, the crown shade's share of the sky.
    @location(8) tint: vec4<f32>,
    @location(9) gust_here: f32,
    // The blade's face (lighting plan phase 5b).
    @location(10) face: vec3<f32>,
}

// Grass against the low sun (lighting plan phase 5b). The share of the
// sunlight on a blade's face that comes through it towards an eye on the
// far side, at the most: a thin leaf passes about a third.
const TRANSMISSION: f32 = 0.35;
// How bright a blade's edge is at grazing view, against the sun, as a share
// of the light through a face turned to it.
const RIM: f32 = 0.5;
// How much light still comes through a blade whose edge points at the sun,
// as a wrap of its face's cosine: light scatters along a blade.
const WRAP: f32 = 0.5;

// Sunlight through a blade seen against the sun, and its rim (lighting plan
// phase 5b). A blade is lit by the ground's normal so a field shades as its
// slope, and Bevy's diffuse transmission is a lobe on the far side of that
// normal, which a sun above the ground never reaches; this is the thin-leaf
// term it cannot give. The light through a blade grows with how squarely its
// face meets the sun (wrapped, since light scatters along it) and how nearly
// the eye looks into the sun, and the edges of blades seen side on catch it
// as a rim. Bevy's own directional light gives the sun's colour through the
// air and under the cloud layer; the sun's shadow and the crowns' share of
// the sky darken it, so grass in shade does not glow.
fn backlight(pbr: PbrInput, face: vec3<f32>, sky: f32) -> vec3<f32> {
    var light = vec3<f32>(0.0);
    let strength = wind[0].grass_rim * sky;
    if strength <= 0.0 {
        return light;
    }
    let V = pbr.V;
    let blade = normalize(face);
    let view_z = dot(vec4<f32>(
        view_bindings::view.view_from_world[0].z,
        view_bindings::view.view_from_world[1].z,
        view_bindings::view.view_from_world[2].z,
        view_bindings::view.view_from_world[3].z
    ), pbr.world_position);
    let rim = pow(1.0 - abs(dot(blade, V)), 4.0);
    for (var i = 0u; i < view_bindings::lights.n_directional_lights; i = i + 1u) {
        let sun = &view_bindings::lights.directional_lights[i];
        let L = (*sun).direction_to_light;
        // The moon by day and a set sun light nothing.
        let against = dot(-V, L);
        if against <= 0.0 || L.y <= 0.0 || max((*sun).color.r, max((*sun).color.g, (*sun).color.b)) <= 0.0 {
            continue;
        }
        // A broad lobe for the field around the sun and a narrow one for
        // the blades right under it.
        let forward = against * against;
        let lobe = 0.4 * forward + 0.6 * pow(against, 8.0);
        let facing = (abs(dot(blade, L)) + WRAP) / (1.0 + WRAP);
        // Fades out as the sun reaches the horizon, where the ground
        // between takes it.
        let horizon = smoothstep(0.0, 0.06, L.y);
        let amount = (TRANSMISSION * facing * lobe + TRANSMISSION * RIM * rim * forward) * horizon;
        if amount <= 1e-4 {
            continue;
        }
        var shadow = 1.0;
        if (pbr.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
                && ((*sun).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            shadow = shadows::fetch_directional_shadow(i, pbr.world_position, pbr.world_normal, view_z, pbr.frag_coord.xy);
        }
        if shadow <= 0.0 {
            continue;
        }
        // A surface facing the light and the eye, rough and without
        // specular: Bevy's directional light is then the blade's colour
        // times the sun's through the air and the clouds, over pi.
        var input: lighting::LightingInput;
        input.layers[lighting::LAYER_BASE].NdotV = 1.0;
        input.layers[lighting::LAYER_BASE].N = L;
        input.layers[lighting::LAYER_BASE].R = vec3<f32>(0.0);
        input.layers[lighting::LAYER_BASE].perceptual_roughness = 1.0;
        input.layers[lighting::LAYER_BASE].roughness = 1.0;
        input.P = pbr.world_position.xyz;
        input.V = L;
        input.diffuse_color = pbr.material.base_color.rgb;
        input.metallic = 0.0;
        input.F0_dielectric = vec3<f32>(0.0);
        input.F0_metallic = vec3<f32>(0.0);
        input.F_ab = vec2<f32>(0.1);
        light += lighting::directional_light(i, &input, true) * (amount * shadow);
    }
    return light * (strength * view_bindings::view.exposure);
}

@vertex
fn vertex(vertex: GrassVertex) -> GrassVertexOutput {
    var out: GrassVertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let cell = cells[mesh_functions::get_tag(vertex.instance_index)];
    let index = blade_index(vertex);
    var placed: PlacedVertex;
    if index >= cell.count {
        placed = collapsed_vertex(world_from_local);
    } else {
        placed = place_vertex(vertex, blades[cell.start + index], cell, grass, wind[0], world_from_local, wind[0].time);
    }
    out.world_position = placed.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(placed.normal, vertex.instance_index);
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
    out.instance_index = vertex.instance_index;
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    out.tint = vec4<f32>(placed.tint, placed.sky);
    out.gust_here = placed.gust_here;
    out.face = mesh_functions::mesh_normal_local_to_world(placed.face, vertex.instance_index);
    return out;
}

@fragment
fn fragment(
    in: GrassVertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
#endif
    var std_in: VertexOutput;
    std_in.position = in.position;
    std_in.world_position = in.world_position;
    std_in.world_normal = in.world_normal;
    std_in.uv = in.uv;
#ifdef VERTEX_UVS_B
    std_in.uv_b = in.uv_b;
#endif
    std_in.instance_index = in.instance_index;
#ifdef VISIBILITY_RANGE_DITHER
    std_in.visibility_range_dither = in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(std_in, is_front);
    // Darker towards the root, as blades shade one another, and brighter
    // under a gust: the wind visible as light crossing the field.
    let ao = 0.55 + 0.45 * in.uv.y;
    let sheen = 0.85 + 0.35 * in.gust_here;
    pbr_input.material.base_color = vec4<f32>(in.tint.rgb * ao * sheen, 1.0) * pbr_input.material.base_color;
    // Sky under crowns (lighting plan phase 3a): a blade under a crown
    // takes the crown's shade on the sky's fill, never under a quarter.
    let sky_occlusion = mix(1.0, max(in.tint.w, 0.25), wind[0].sky_visibility);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * sky_occlusion * vec3<f32>(wind[0].shade_r, wind[0].shade_g, wind[0].shade_b);
    pbr_input.specular_occlusion = pbr_input.specular_occlusion * sky_occlusion;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
        // Grass against the low sun (lighting plan phase 5b).
        out.color = vec4<f32>(out.color.rgb + backlight(pbr_input, in.face, sky_occlusion), out.color.a);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
