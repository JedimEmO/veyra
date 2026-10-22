// Shared by the terrain's main-pass and prepass vertex shaders: the vertex
// as the region store holds it, the material uniform and the LOD morph, so
// the shadow map is cast by exactly the surface that is drawn.

#define_import_path strata::terrain_common

#import bevy_pbr::mesh_functions

// Mirrors strata_terrain::material::TerrainParams field for field.
struct TerrainParams {
    // Metres per texture repeat, one f32 per layer, packed four to a vec4
    // because uniform arrays of scalars pad every element to 16 bytes.
    tiling: array<vec4<f32>, 4>,
    // The linear luminance each layer is drawn at (the value budget), packed
    // like tiling; zero leaves a layer as baked.
    value: array<vec4<f32>, 4>,
    // The grain each layer keeps, packed like tiling; zero reads `grain`.
    layer_grain: array<vec4<f32>, 4>,
    // A laid layer's stones, one vec4 a layer: courses, stones to a course,
    // the joints' share of a stone, the value a stone may differ by.
    laid: array<vec4<f32>, 16>,
    slope_start: f32,
    slope_end: f32,
    normal_strength: f32,
    morph_start: f32,
    morph_end: f32,
    detail_near: f32,
    detail_far: f32,
    normal_far: f32,
    tint_near: f32,
    tint_far: f32,
    tint_strength: f32,
    macro_strength: f32,
    slope_tint: f32,
    uv_period: f32,
    cover_near: f32,
    cover_far: f32,
    // A bit per layer: a laid pattern, read plainly (see sample_layer).
    patterned: u32,
    // A bit per layer: a grain that keeps its direction, shifted along u
    // only and never turned (see sample_layer).
    oriented: u32,
    // How much the baked sky visibility darkens the sky's fill, 0 to 1.
    sky_visibility: f32,
    // A bit per layer: living cover, which the ground's palette paints.
    palette: u32,
    // The ground painted (ticket 0006, pass 1; material.rs has each one's
    // meaning). `cavity` is read when the arrays are packed, not here.
    colour_lush: f32,
    colour_warm: f32,
    near_tint: f32,
    near_value: f32,
    grain: f32,
    matte: f32,
    seam_shade: f32,
    cavity: f32,
    air_near: f32,
    air_far: f32,
    air_lift: f32,
    air_desaturate: f32,
    air_colour: vec4<f32>,
    // The shade's colour, a multiplier on the indirect diffuse (visual.md F4).
    shade_tint: vec4<f32>,
}

// The region store (strata_terrain::store): every resident region's grid
// records, a slot each, and a header per slot. A region entity's MeshTag is
// its slot. The layout is the store module's docs, word for word.
struct TerrainSlot {
    // The UV of grid vertex (0, 0): the region's minimum corner less its
    // UV wrap base.
    uv_origin: vec2<f32>,
    // Metres between grid vertices.
    step: f32,
    // The region's side, metres, which scales the morph band.
    size: f32,
    // How far below its edge vertex a skirt vertex hangs, metres.
    skirt_depth: f32,
    lod: u32,
    spare_a: u32,
    spare_b: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(107) var<storage, read> terrain_vertices: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var<storage, read> terrain_slots: array<TerrainSlot>;

// strata_core::GRID_N and strata_terrain::store::VERTEX_WORDS.
const TERRAIN_GRID_N: u32 = 65u;
const TERRAIN_VERTEX_WORDS: u32 = 16u;
// strata_terrain::store::FAR_FACTOR_RANGE, 255 / 128: what a far factor
// byte of 255 is, so one of 128 is a factor of one exactly.
const TERRAIN_FAR_FACTOR_RANGE: f32 = 1.9921875;

// A record's far factor word: each byte over 128 (store::unpack_far_factor),
// exact where pack4x8unorm's reading over 255 times the range is not.
fn unpack_far_factor(word: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(word & 0xffu),
        f32((word >> 8u) & 0xffu),
        f32((word >> 16u) & 0xffu),
    ) * (TERRAIN_FAR_FACTOR_RANGE / 255.0);
}

// A vertex of the shared region mesh: where it sits, nothing else.
struct TerrainVertex {
    @builtin(instance_index) instance_index: u32,
    // (i, skirt, j): the grid index, and on a skirt the step its rock corner
    // turns by (1 under a row edge, 2 under a column edge), 0 on the grid.
    @location(0) place: vec3<f32>,
}

// The vertex rebuilt from the store, in the old vertex contract's terms
// (material.rs, "Vertex contract").
struct TerrainAttributes {
    // Metres from the region's minimum corner.
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    // Visibility, bent normal x and z mapped to 0..1, at the vertex and at
    // its point on the parent level's mesh.
    sky: vec4<f32>,
    sky_parent: vec4<f32>,
    // The rock: pair / 255, wild share, the pair's higher-layer share,
    // corner / 255. See mesh.rs, "Rock".
    material: vec4<f32>,
    // Where the vertex sits on the parent LOD's mesh, and the region's side.
    parent_height: f32,
    parent_normal: vec3<f32>,
    node_size: f32,
    // Biome tint, linear RGB, and the slope at which rock bares as
    // 1 - normal.y.
    tint: vec4<f32>,
    // The factor of the tint that is the provinces' far tint on wild
    // ground: the tint over it is the biomes' own ground colour.
    far_factor: vec3<f32>,
    // Canopy colour, linear RGB, and the fraction of ground it covers.
    cover: vec4<f32>,
    // The share of each layer, four per vector.
    weights_a: vec4<f32>,
    weights_b: vec4<f32>,
    weights_c: vec4<f32>,
    weights_d: vec4<f32>,
}

// The octahedral pair of a unit vector folded about y (store::oct_decode is
// the same on the CPU for the tests).
fn oct_decode(p: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(p.x, 1.0 - abs(p.x) - abs(p.y), p.y);
    let t = max(-n.y, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.z += select(t, -t, n.z >= 0.0);
    return normalize(n);
}

// Reads a shared-mesh vertex's record and its region's header and rebuilds
// what the vertex used to carry as attributes (store::unpack_vertex is the
// CPU mirror the tests hold to the mesher's own streams). A skirt vertex is
// its edge vertex dropped by the skirt depth, its morph target with it, and
// its rock corner turned so its wall's triangles keep three corners.
fn terrain_attributes(vertex: TerrainVertex) -> TerrainAttributes {
    let slot_index = mesh_functions::get_tag(vertex.instance_index);
    let slot = terrain_slots[slot_index];
    let i = u32(vertex.place.x);
    let skirt = u32(vertex.place.y);
    let j = u32(vertex.place.z);
    let base = (slot_index * TERRAIN_GRID_N * TERRAIN_GRID_N + j * TERRAIN_GRID_N + i) * TERRAIN_VERTEX_WORDS;

    var out: TerrainAttributes;
    var height = bitcast<f32>(terrain_vertices[base]);
    out.weights_a = unpack4x8unorm(terrain_vertices[base + 1u]);
    out.weights_b = unpack4x8unorm(terrain_vertices[base + 2u]);
    out.weights_c = unpack4x8unorm(terrain_vertices[base + 3u]);
    out.weights_d = unpack4x8unorm(terrain_vertices[base + 4u]);
    out.material = unpack4x8unorm(terrain_vertices[base + 5u]);
    out.sky = unpack4x8unorm(terrain_vertices[base + 6u]);
    out.sky_parent = unpack4x8unorm(terrain_vertices[base + 7u]);
    out.normal = oct_decode(unpack2x16snorm(terrain_vertices[base + 8u]));
    var parent_height = bitcast<f32>(terrain_vertices[base + 9u]);
    out.parent_normal = oct_decode(unpack2x16snorm(terrain_vertices[base + 10u]));
    out.tint = vec4<f32>(
        unpack2x16float(terrain_vertices[base + 11u]),
        unpack2x16float(terrain_vertices[base + 12u]),
    );
    out.cover = vec4<f32>(
        unpack2x16float(terrain_vertices[base + 13u]),
        unpack2x16float(terrain_vertices[base + 14u]),
    );
    out.far_factor = unpack_far_factor(terrain_vertices[base + 15u]);
    if skirt != 0u {
        height = height - slot.skirt_depth;
        parent_height = parent_height - slot.skirt_depth;
        let corner = u32(round(out.material.w * 255.0));
        out.material.w = f32((corner + skirt) % 3u) / 255.0;
    }
    let x = f32(i) * slot.step;
    let z = f32(j) * slot.step;
    out.position = vec3<f32>(x, height, z);
    out.uv = slot.uv_origin + vec2<f32>(x, z);
    out.parent_height = parent_height;
    out.node_size = slot.size;
    return out;
}

// A record's tint and far factor (words 11, 12 and 15).
struct TerrainTint {
    tint: vec4<f32>,
    far_factor: vec3<f32>,
}

fn terrain_tint_at(slot_index: u32, i: u32, j: u32) -> TerrainTint {
    let base = (slot_index * TERRAIN_GRID_N * TERRAIN_GRID_N + j * TERRAIN_GRID_N + i) * TERRAIN_VERTEX_WORDS;
    var out: TerrainTint;
    out.tint = vec4<f32>(
        unpack2x16float(terrain_vertices[base + 11u]),
        unpack2x16float(terrain_vertices[base + 12u]),
    );
    out.far_factor = unpack_far_factor(terrain_vertices[base + 15u]);
    return out;
}

// The tint and far factor at the vertex's point on the parent level's mesh,
// read from its neighbours' records as mesh::Grid::parent places the height
// (store::tint_parent is the CPU mirror the tests hold to the coarse
// level): an even vertex is its own and the caller keeps it, an odd one the
// midpoint of the two parent vertices it lies between. The mesher stores no
// parent tint, so the record did not grow for it. On a region a detail zone
// pins this also blurs the tint at distance, as the coarser level would.
fn tint_parent(vertex: TerrainVertex, own: TerrainTint) -> TerrainTint {
    let i = u32(vertex.place.x);
    let j = u32(vertex.place.z);
    let odd_i = (i & 1u) == 1u;
    let odd_j = (j & 1u) == 1u;
    if !odd_i && !odd_j {
        return own;
    }
    let slot_index = mesh_functions::get_tag(vertex.instance_index);
    var a = vec2<u32>(i - 1u, j);
    var b = vec2<u32>(i + 1u, j);
    if !odd_i {
        a = vec2<u32>(i, j - 1u);
        b = vec2<u32>(i, j + 1u);
    } else if odd_j {
        a = vec2<u32>(i + 1u, j - 1u);
        b = vec2<u32>(i - 1u, j + 1u);
    }
    let ta = terrain_tint_at(slot_index, a.x, a.y);
    let tb = terrain_tint_at(slot_index, b.x, b.y);
    var out: TerrainTint;
    out.tint = 0.5 * (ta.tint + tb.tint);
    out.far_factor = 0.5 * (ta.far_factor + tb.far_factor);
    return out;
}

struct MorphedVertex {
    // Local space, morphed.
    local_position: vec3<f32>,
    world_position: vec4<f32>,
    // Local space, unit length.
    normal: vec3<f32>,
    // How far towards the parent grid, 0 to 1, for the attributes that
    // morph with the height.
    morph_t: f32,
}

fn tiling_of(params: TerrainParams, layer: u32) -> f32 {
    let v = params.tiling[layer / 4u];
    let i = layer % 4u;
    if i == 0u { return v.x; }
    if i == 1u { return v.y; }
    if i == 2u { return v.z; }
    return v.w;
}

// The luminance layer `layer` is drawn at, or 0 for as baked.
fn value_of(params: TerrainParams, layer: u32) -> f32 {
    let v = params.value[layer / 4u];
    let i = layer % 4u;
    if i == 0u { return v.x; }
    if i == 1u { return v.y; }
    if i == 2u { return v.z; }
    return v.w;
}

// The contrast layer `layer` keeps about its mean: its own, or the look's.
fn grain_of(params: TerrainParams, layer: u32) -> f32 {
    let v = params.layer_grain[layer / 4u];
    let i = layer % 4u;
    var g = v.w;
    if i == 0u { g = v.x; }
    if i == 1u { g = v.y; }
    if i == 2u { g = v.z; }
    return select(params.grain, g, g > 0.0);
}

// The CDLOD morph. A vertex further than morph_end node sizes from the
// observer sits on the parent LOD's grid, one closer than morph_start sits
// on its own, and between them it slides. The distance is horizontal and
// measured from the observer that drives LOD selection, which for a shadow
// view is still the camera (view.lod_view_world_position), so every pass
// morphs the same way. The horizontal position is not morphed, so the
// distance is the same whichever grid the vertex is on.
fn morph_vertex(
    vertex: TerrainAttributes,
    world_from_local: mat4x4<f32>,
    params: TerrainParams,
    observer: vec3<f32>,
) -> MorphedVertex {
    let unmorphed = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    let distance = length(unmorphed.xz - observer.xz);
    let size = vertex.node_size;
    let start = params.morph_start * size;
    let end = params.morph_end * size;
    let t = clamp((distance - start) / max(end - start, 1e-3), 0.0, 1.0);

    let height = mix(vertex.position.y, vertex.parent_height, t);

    var out: MorphedVertex;
    out.local_position = vec3<f32>(vertex.position.x, height, vertex.position.z);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(out.local_position, 1.0),
    );
    out.normal = normalize(mix(vertex.normal, vertex.parent_normal, t));
    out.morph_t = t;
    return out;
}
