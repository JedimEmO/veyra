// The grass vertex stage for Bevy's prepass pipelines: the same placement
// and bend as the main pass, and the previous frame's bend for motion
// vectors. Grass casts no shadow, so this runs for depth (and, under TAA,
// motion vectors) only. The fragment stage is Bevy's default.

#import bevy_pbr::{
    mesh_functions,
    prepass_io::VertexOutput,
    view_transformations::position_world_to_clip,
}

#import strata::grass_common::{Blade, GrassCellRecord, GrassParams, GrassVertex, PlacedVertex, blade_index, collapsed_vertex, place_vertex}
#import strata::foliage_wind::FoliageWind

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> grass: GrassParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage, read> blades: array<Blade>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<storage, read> wind: array<FoliageWind>;
// Every cell's record; an entity's MeshTag is its cell's slot.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<storage, read> cells: array<GrassCellRecord>;

@vertex
fn vertex(vertex: GrassVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let cell = cells[mesh_functions::get_tag(vertex.instance_index)];
    let index = blade_index(vertex);
    let alive = index < cell.count;
    var placed: PlacedVertex;
    if alive {
        placed = place_vertex(vertex, blades[cell.start + index], cell, grass, wind[0], world_from_local, wind[0].time);
    } else {
        placed = collapsed_vertex(world_from_local);
    }

    out.world_position = placed.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(placed.normal, vertex.instance_index);
#endif

#ifdef MOTION_VECTOR_PREPASS
    let prev_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    var previous_wind = wind[0];
    previous_wind.origin_x = wind[0].prev_origin_x;
    previous_wind.origin_z = wind[0].prev_origin_z;
    var prev: PlacedVertex;
    if alive {
        prev = place_vertex(vertex, blades[cell.start + index], cell, grass, previous_wind, prev_model, wind[0].prev_time);
    } else {
        prev = collapsed_vertex(prev_model);
    }
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
