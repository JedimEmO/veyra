// Shared by the grass main-pass and prepass vertex shaders: the blade
// record, the cell record, the species table and how a vertex of the shared
// mesh becomes a point of a blade in the cell.

#define_import_path strata::grass_common

#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_bindings::mesh
#import strata::foliage_wind::{FoliageWind, gust, bend, grow_in}

// Mirrors strata_flora::GrassBlade byte for byte: 16 bytes.
struct Blade {
    // x in the low 16 bits, z in the high, each 0..65535 over the 64 m cell.
    xz: u32,
    y: f32,
    // height_cm, phase, stiffness, species: a byte each, low to high.
    a: u32,
    // lush, shade, nx, nz: a byte each, the normal components signed.
    b: u32,
}

// Mirrors strata_foliage::grass::GrassCellRecord: where a cell's blades
// are in the shared buffer, and what the cell's own uniform carried when
// every cell was a material of its own. An entity finds its cell by its
// MeshTag.
struct GrassCellRecord {
    // The cell's first blade record.
    start: u32,
    // Its blade records; a slot past them collapses.
    count: u32,
    // The height of the mesh origin, so visibility and the roots share the
    // same elevated frame.
    origin_y: f32,
    // Seconds, when the cell was spawned, for the growth from the ground.
    spawn_time: f32,
}

// Mirrors strata_foliage::grass::GrassParams.
struct GrassParams {
    width: array<vec4<f32>, 8>,
    shape: array<vec4<f32>, 32>,
    tint_dry: array<vec4<f32>, 32>,
    tint_lush: array<vec4<f32>, 32>,
}

struct GrassVertex {
    @builtin(instance_index) instance_index: u32,
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position: vec3<f32>,
    // side (0 or 1), height fraction.
    @location(2) uv: vec2<f32>,
    // record stride, vertices per blade.
    @location(3) uv_b: vec2<f32>,
}

struct PlacedVertex {
    // Cell local (the mesh's frame), before the world transform.
    local_position: vec3<f32>,
    world_position: vec4<f32>,
    normal: vec3<f32>,
    tint: vec3<f32>,
    // Zero for a slot past the cell's count.
    alive: f32,
    // The gust at the root, 0..1, so a front reads as light crossing the
    // field. Not named after the imported function: naga_oil renames an
    // imported symbol wherever the name appears, field accessors included.
    gust_here: f32,
    // The share of the sky the blade sees past the crowns over it, 0.25 to
    // 1 (lighting plan phase 3a).
    sky: f32,
    // The normal of the blade's face, cell local and horizontal, which
    // `normal` (the ground's, so a field shades as its slope) is not: how
    // much sun a blade lets through and where its edges are (lighting plan
    // phase 5b).
    face: vec3<f32>,
}

fn byte(v: u32, i: u32) -> f32 {
    return f32((v >> (8u * i)) & 0xffu);
}

fn sbyte(v: u32, i: u32) -> f32 {
    let u = (v >> (8u * i)) & 0xffu;
    return f32(i32(u << 24u) >> 24u);
}

fn width_of(params: GrassParams, species: u32) -> f32 {
    let v = params.width[species / 4u];
    let i = species % 4u;
    if i == 0u { return v.x; }
    if i == 1u { return v.y; }
    if i == 2u { return v.z; }
    return v.w;
}

// Root-derived variation survives LOD changes and floating-origin rebases
// without adding bytes to the instance record.
fn blade_random(root_bits: u32, salt: u32) -> f32 {
    var h = root_bits ^ salt;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return f32((h ^ (h >> 16u)) >> 8u) / 16777216.0;
}

// The record index a vertex of the shared mesh reads. The builtin counts
// from the slab the mesh shares with others, not from this mesh; the mesh
// uniform says where this mesh starts.
fn blade_index(vertex: GrassVertex) -> u32 {
    let stride = u32(vertex.uv_b.x);
    let per_blade = max(u32(vertex.uv_b.y), 1u);
    let local_index = vertex.vertex_index - mesh[vertex.instance_index].first_vertex_index;
    return (local_index / per_blade) * stride;
}

// A vertex of a slot past the cell's count: collapsed to a point below
// the cell, so it rasterises nothing.
fn collapsed_vertex(world_from_local: mat4x4<f32>) -> PlacedVertex {
    var out: PlacedVertex;
    out.alive = 0.0;
    out.local_position = vec3<f32>(0.0, -1000.0, 0.0);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(out.local_position, 1.0));
    out.normal = vec3<f32>(0.0, 1.0, 0.0);
    out.tint = vec3<f32>(0.0);
    out.gust_here = 0.0;
    out.sky = 1.0;
    out.face = vec3<f32>(0.0, 0.0, 1.0);
    return out;
}

// The vertex of the shared mesh as a point of its blade.
fn place_vertex(
    vertex: GrassVertex,
    blade: Blade,
    cell: GrassCellRecord,
    params: GrassParams,
    wind: FoliageWind,
    world_from_local: mat4x4<f32>,
    time: f32,
) -> PlacedVertex {
    var out: PlacedVertex;
    // The mesh's origin is the cell centre; records are from the corner.
    let root = vec3<f32>(
        f32(blade.xz & 0xffffu) * (64.0 / 65535.0) - 32.0,
        blade.y - cell.origin_y,
        f32(blade.xz >> 16u) * (64.0 / 65535.0) - 32.0,
    );
    // Grown from the ground as the cell streams in.
    let height = byte(blade.a, 0u) * 0.01 * grow_in(cell.spawn_time, time);
    let phase = byte(blade.a, 1u) / 255.0;
    let stiffness = byte(blade.a, 2u) / 255.0;
    // The registry assigns grass-only slots and rejects palette overflow.
    // Malformed records must never substitute another species' appearance.
    // The palette in the low five bits, the crown shade over the blade in
    // the high three (strata_flora::GrassBlade::crown_sky).
    let species_byte = u32(byte(blade.a, 3u));
    let species = species_byte & 31u;
    if species >= 32u {
        return collapsed_vertex(world_from_local);
    }
    let lush = byte(blade.b, 0u) / 255.0;
    let shade = 0.75 + 0.5 * byte(blade.b, 1u) / 255.0;
    let nx = sbyte(blade.b, 2u) / 127.0;
    let nz = sbyte(blade.b, 3u) / 127.0;
    let normal = vec3<f32>(nx, sqrt(max(1.0 - nx * nx - nz * nz, 0.0)), nz);

    let shape = params.shape[species];
    let variation = blade_random(blade.xz, 193u);
    let dead = blade_random(blade.xz, 7919u) < shape.z;
    // Phase carries the tuft's shared lean. Individual blades fan around
    // it, mixing fine upright shoots with wider arching mature blades.
    let yaw = phase * 6.283185 + (variation - 0.5) * 1.2;
    let side = vec3<f32>(cos(yaw), 0.0, -sin(yaw));
    let bow = vec3<f32>(sin(yaw), 0.0, cos(yaw));
    let v = vertex.uv.y;
    let maturity = smoothstep(0.2, 0.8, variation);
    let taper = mix(1.0 - v, 1.0 - v * v, maturity);
    let half_w = width_of(params, species) * mix(0.55, 1.35, variation) * 0.5 * taper;
    let curve = shape.x * mix(0.25, 1.5, maturity) + select(0.0, 0.22, dead);
    let droop = shape.y * maturity + select(0.0, 0.22, dead);
    let rest = vec3<f32>(0.0, height * (v - droop * v * v * v), 0.0)
        + bow * (curve * height * v * v);
    let across = side * ((vertex.uv.x - 0.5) * 2.0 * half_w);

    let root_world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(root, 1.0));
    let world_xz = root_world.xz + vec2<f32>(wind.origin_x, wind.origin_z);
    let g = gust(world_xz, wind, time);
    let bent = bend(rest, v, height, phase, stiffness, g, wind, time);

    out.alive = 1.0;
    out.local_position = root + bent + across;
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(out.local_position, 1.0));
    out.normal = normal;
    out.tint = mix(params.tint_dry[species].rgb, params.tint_lush[species].rgb,
        select(lush, 0.0, dead)) * shade;
    out.gust_here = g;
    out.sky = 1.0 - f32(species_byte >> 5u) / 7.0 * 0.75;
    // `side` across a blade and up along it span its face.
    out.face = bow;
    return out;
}
