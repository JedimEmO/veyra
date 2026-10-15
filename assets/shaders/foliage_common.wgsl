// Shared by the tree main-pass and prepass vertex shaders: the vertex
// contract of strata_foliage::tree, the variant's pivot table, and the bend
// of a vertex through its chain of pivots, so shadows and motion vectors come
// from the same branch the main pass draws.

#define_import_path strata::foliage_common

#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_bindings::mesh
#import strata::foliage_wind::{FoliageWind, gust}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage, read> wind: array<FoliageWind>;
// Three rows per pivot: [x, y, z, parent] [axis, length]
// [base_radius, frequency_hz, stiffness, 0], in the mesh's frame.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage, read> pivots: array<vec4<f32>>;

// The vertex contract of tree.rs, in its shader locations.
struct FoliageVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) tangent: vec4<f32>,
    // pivot index, hierarchy level, flutter weight, phase.
    @location(8) wind_data: vec4<f32>,
    // ambient occlusion, height over the tree, curvature, crown depth.
    @location(9) shade: vec4<f32>,
#ifdef VERTEX_COLORS
    // A tint the base colour is multiplied by: a rock's moss.
    @location(5) color: vec4<f32>,
#endif
}

struct BentVertex {
    world_position: vec4<f32>,
    normal: vec3<f32>,
    tangent: vec4<f32>,
}

// A branch deeper than this shares an ancestor's motion: the generator
// never makes one, and a bound keeps the loop cheap for a bad table.
const MAX_PIVOTS: u32 = 4u;

fn rotate(v: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return v * c + cross(axis, v) * s + axis * dot(axis, v) * (1.0 - c);
}

// A per-tree phase from the mesh tag's low half, which the spawner fills
// from the plant's place, so copies of one variant do not sway in step.
fn tree_phase(instance_index: u32) -> f32 {
    var h = (mesh[instance_index].tag & 0xffffu) * 0x9e3779b1u;
    h = h ^ (h >> 15u);
    return f32(h & 0xffffu) / 65535.0;
}

// How far one pivot's branch turns at `time`: a lean with the gust and a
// sway at the branch's own frequency, less for stiff, thick limbs.
fn pivot_angle(index: u32, row: vec4<f32>, strength: f32, g: f32, phase: f32, time: f32) -> f32 {
    let frequency = max(row.y, 0.05);
    let stiffness = max(row.z, 0.05);
    let own = fract(f32(index) * 0.618034 + phase);
    let lean = strength * (0.15 + 0.6 * g);
    let sway = sin(time * 6.2832 * frequency + own * 6.2832) * strength * (0.25 + 0.5 * g)
        + sin(time * 6.2832 * frequency * 2.3 + own * 11.0) * strength * 0.08;
    return (lean + sway) * 0.08 / stiffness;
}

// The bend at `time`: each pivot from the vertex's own out to the trunk
// turns what it carries about its attachment. A pivot's turn is weighted by
// the square of how far along the branch the point it carries sits, and for
// an ancestor that point is the child's attachment, so a child moves exactly
// as the parent's surface where it grows: junctions stay closed.
fn bend_vertex(
    vertex: FoliageVertex,
    world_from_local: mat4x4<f32>,
    wind_now: FoliageWind,
    time: f32,
) -> BentVertex {
    let root_world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(pivots[0].xyz, 1.0));
    let g = gust(root_world.xz + vec2<f32>(wind_now.origin_x, wind_now.origin_z), wind_now, time);
    // Gusts travel in world space; the pivots are mesh local.
    let world_direction = vec3<f32>(wind_now.direction.x, 0.0, wind_now.direction.y);
    let local_direction = vec3<f32>(
        dot(world_direction, normalize(world_from_local[0].xyz)),
        dot(world_direction, normalize(world_from_local[1].xyz)),
        dot(world_direction, normalize(world_from_local[2].xyz)));
    let has_wind = length(local_direction) > 1e-5;
    let push = select(vec3<f32>(1.0, 0.0, 0.0), normalize(local_direction), has_wind);
    let strength = select(0.0, wind_now.strength, has_wind);
    let phase = tree_phase(vertex.instance_index);

    var position = vertex.position;
    var normal = vertex.normal;
    var tangent = vertex.tangent.xyz;
    var carried = vertex.position;
    var index = u32(max(vertex.wind_data.x, 0.0));
    for (var step = 0u; step < MAX_PIVOTS; step++) {
        let origin = pivots[index * 3u];
        let axis_row = pivots[index * 3u + 1u];
        let dynamics = pivots[index * 3u + 2u];
        let axis = normalize(axis_row.xyz);
        let along = clamp(dot(carried - origin.xyz, axis) / max(axis_row.w, 0.01), 0.0, 1.0);
        var turn_axis = cross(axis, push);
        if length(turn_axis) < 1e-4 {
            turn_axis = cross(vec3<f32>(0.0, 1.0, 0.0), push);
        }
        turn_axis = normalize(turn_axis);
        let angle = pivot_angle(index, dynamics, strength, g, phase, time) * along * along;
        position = origin.xyz + rotate(position - origin.xyz, turn_axis, angle);
        normal = rotate(normal, turn_axis, angle);
        tangent = rotate(tangent, turn_axis, angle);
        carried = origin.xyz;
        if origin.w < 0.0 || index == 0u {
            break;
        }
        index = u32(origin.w);
    }
    // Leaf cards flutter along their normal, most at their tips and edges.
    let flutter = vertex.wind_data.z;
    position += vertex.normal * (sin(time * 5.1 + vertex.wind_data.w * 6.2832 + phase * 3.0)
        * flutter * strength * (0.4 + g) * 0.035);

    var out: BentVertex;
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.normal = normal;
    out.tangent = vec4<f32>(tangent, vertex.tangent.w);
    return out;
}
