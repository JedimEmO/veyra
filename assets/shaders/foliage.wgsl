// Tree shader for `strata_foliage::tree::TreeMaterial`: the pivot bend in
// the vertex stage, then Bevy's standard lighting (bark opaque, leaves
// masked by the material) with the baked occlusion and height gradient.

#import bevy_pbr::{
    mesh_functions,
    mesh_bindings::mesh,
    view_transformations::position_world_to_clip,
    forward_io::VertexOutput,
    forward_io::FragmentOutput,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::{STANDARD_MATERIAL_FLAGS_UNLIT_BIT, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_MASK},
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions
#endif

#import strata::foliage_common::{FoliageVertex, bend_vertex, tree_wind}
#import strata::canopy::canopy_rim

// The locations of forward_io::VertexOutput for the fields carried.
struct FoliageVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) world_tangent: vec4<f32>,
    @location(6) @interpolate(flat) instance_index: u32,
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    // A per-tree tint from the mesh tag: brightness and a warm-cool lean;
    // in w the share of the sky the placer found past the neighbours'
    // crowns (the tag's top byte, lighting plan phase 3a).
    @location(8) @interpolate(flat) tint: vec4<f32>,
    // ambient occlusion, height over the tree, curvature, crown depth.
    @location(9) shade: vec4<f32>,
#ifdef VERTEX_COLORS
    @location(10) color: vec4<f32>,
#endif
}

fn tag_tint(tag: u32) -> vec3<f32> {
    var h = (tag & 0xffffu) * 0x9e3779b1u;
    h = h ^ (h >> 15u);
    let a = f32(h & 0xffu) / 255.0;
    let b = f32((h >> 8u) & 0xffu) / 255.0;
    let bright = 0.8 + 0.4 * a;
    return vec3<f32>(bright * (1.0 + 0.12 * (b - 0.5)), bright, bright * (1.0 - 0.12 * (b - 0.5)));
}

@vertex
fn vertex(vertex: FoliageVertex) -> FoliageVertexOutput {
    var out: FoliageVertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let wind_now = tree_wind(vertex.instance_index);
    let bent = bend_vertex(vertex, world_from_local, wind_now, wind_now.time);
    out.world_position = bent.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(bent.normal, vertex.instance_index);
    out.uv = vertex.uv;
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, bent.tangent, vertex.instance_index);
    out.instance_index = vertex.instance_index;
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    let tag = mesh[vertex.instance_index].tag;
    out.tint = vec4<f32>(tag_tint(tag), 1.0 - f32(tag >> 24u) / 255.0);
    out.shade = vertex.shade;
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
    return out;
}

@fragment
fn fragment(
    in: FoliageVertexOutput,
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
#ifdef VERTEX_TANGENTS
    std_in.world_tangent = in.world_tangent;
#endif
    std_in.instance_index = in.instance_index;
    // Bevy multiplies the base colour by the vertex colour of a mesh that
    // has one (a rock's moss, a flower's petals), reading it from here.
#ifdef VERTEX_COLORS
    std_in.color = in.color;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    std_in.visibility_range_dither = in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(std_in, is_front);
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * in.tint.rgb, pbr_input.material.base_color.a);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    // The crown is lit as soft masses: its inside darker by the baked
    // occlusion, its top a little lighter than its skirt.
    let occlusion = mix(0.55, 1.0, in.shade.x);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * occlusion;
    pbr_input.specular_occlusion = pbr_input.specular_occlusion * occlusion;
    // Sky under crowns (lighting plan phase 3a): the placer's share of the
    // sky past the neighbours' crowns, taken at the crown's underside; the
    // crown's top sees over them, so it opens towards the top. Nothing goes
    // under a quarter, so the deepest shade keeps the sky's colour.
    let plant_sky = mix(in.tint.w, 1.0, 0.6 * in.shade.y);
    let sky_occlusion = mix(1.0, max(plant_sky, 0.25), tree_wind(in.instance_index).sky_visibility);
    let shade_wind = tree_wind(in.instance_index);
    let shade_tint = vec3<f32>(shade_wind.shade_r, shade_wind.shade_g, shade_wind.shade_b);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * sky_occlusion * shade_tint;
    pbr_input.specular_occlusion = pbr_input.specular_occlusion * sky_occlusion;
    let lift = mix(0.85, 1.1, in.shade.y);
    // The crown's grouping (visual.md F5): a dark cool core inside by the
    // crown depth (0 outside, 1 deep) and lighter clumps by the curvature,
    // the factor the impostor bake has in its atlas. Only the masked pieces
    // (leaves) wear it: bark and rocks draw opaque and read the same lanes
    // for other things (a rock's curvature is centred at 0.5 and its fourth
    // lane is moss), and the bake grades cut-outs alone to match.
    let leaves = (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_MASK;
    let core = vec3<f32>(shade_wind.canopy_r, shade_wind.canopy_g, shade_wind.canopy_b);
    var group = vec3<f32>(1.0);
    if leaves {
        group = mix(vec3<f32>(1.0), core, in.shade.w) * max(1.0 + shade_wind.canopy_lift * in.shade.z, 0.0);
    }
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * (lift * group), pbr_input.material.base_color.a);

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
        // Sun through the outer leaves seen against it.
        let rim_open = mix(0.5, 1.0, 1.0 - in.shade.w) * sky_occlusion;
        out.color = vec4<f32>(out.color.rgb + canopy_rim(pbr_input.V, pbr_input.material.base_color.rgb, rim_open, select(0.0, shade_wind.canopy_rim, leaves)), out.color.a);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
