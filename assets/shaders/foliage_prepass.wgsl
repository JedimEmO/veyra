// The tree vertex stage for Bevy's prepass and shadow pipelines: the same
// pivot bend as the main pass, so shadows move with the wind and motion vectors
// see where a leaf was last frame. The fragment stage is Bevy's default,
// which applies the material's alpha mask.

#import bevy_pbr::{
    mesh_functions,
    prepass_io::VertexOutput,
    view_transformations::position_world_to_clip,
}

#import strata::foliage_common::{FoliageVertex, bend_vertex, wind}

@vertex
fn vertex(vertex: FoliageVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let bent = bend_vertex(vertex, world_from_local, wind[0], wind[0].time);

    out.world_position = bent.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(bent.normal, vertex.instance_index);
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, bent.tangent, vertex.instance_index);
#endif
#endif

#ifdef MOTION_VECTOR_PREPASS
    let prev_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    var previous_wind = wind[0];
    previous_wind.origin_x = wind[0].prev_origin_x;
    previous_wind.origin_z = wind[0].prev_origin_z;
    let prev = bend_vertex(vertex, prev_model, previous_wind, wind[0].prev_time);
    out.previous_world_position = prev.world_position;
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
