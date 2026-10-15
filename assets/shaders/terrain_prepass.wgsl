// The terrain's vertex stage for Bevy's prepass and shadow pipelines.
//
// Bevy's own prepass vertex shader reads position from location 0 and would
// draw the unmorphed grid, so shadows would be cast by a surface that sits a
// few metres off the one on screen and self-shadowing would speckle every
// slope. This mirrors bevy_pbr's prepass.wgsl vertex stage (without
// skinning and morph targets, which terrain has none of), reads the vertex
// from the region store as the main pass does (terrain_common.wgsl,
// terrain_attributes) and applies the same morph. The fragment stage is
// Bevy's default.

#import bevy_pbr::{
    mesh_functions,
    prepass_io::VertexOutput,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

#import strata::terrain_common::{TerrainParams, TerrainVertex, morph_vertex, terrain_attributes}

@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> terrain: TerrainParams;

@vertex
fn vertex(vertex: TerrainVertex) -> VertexOutput {
    var out: VertexOutput;
    let v = terrain_attributes(vertex);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let morphed = morph_vertex(v, world_from_local, terrain, view.lod_view_world_position);

    out.world_position = morphed.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        morphed.normal,
        vertex.instance_index,
    );
#endif

#ifdef MOTION_VECTOR_PREPASS
    // Terrain does not move; the previous frame's position is the same
    // morphed vertex under the previous transform.
    let prev_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        prev_model,
        vec4<f32>(morphed.local_position, 1.0),
    );
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif

    return out;
}
