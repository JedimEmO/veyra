// Terrain splat shader for `strata_terrain::material::TerrainMaterial`.
//
// An extension of Bevy's StandardMaterial: the vertex stage morphs the
// vertex towards the coarser LOD's grid with distance and carries the
// per-vertex layer shares through to the fragment stage; the fragment stage
// samples the texture arrays, blends the strongest layers by share, pushes steep
// faces towards the rock of their geological province, tints the far field
// with the biome colour, and then hands a PbrInput to the standard lighting
// so shadows, fog and the atmosphere come for free.
//
// The vertex contract is documented on the Rust side (material.rs): the
// vertex stage reads its region's records from the store (store.rs,
// terrain_common.wgsl's terrain_attributes). Every
// layer's share interpolates on its own, so a border between two layers is
// wherever their shares cross, not a triangle edge.
//
// Repetition. A photographed ground set repeats every few metres and from
// the air the repeats line up into a grid the eye finds at once. Every
// detail read here is stochastic: the ground is cut into a triangular
// lattice a quarter of a texture repeat wide (Heitz and Neyret's layout),
// each lattice vertex gets a random offset into the texture, and a
// fragment blends the reads of the three vertices of the triangle it lies
// in, so no two places show the same features in the same arrangement. The
// blend of three shifted reads would lose contrast, so it is renormalised
// about the layer's mean colour (the variance-preserving average of
// Quilez's "texture repetition" notes). The triangles are the halves of a
// row-sheared square lattice rather than equilateral, which costs nothing
// visible and keeps the lattice periodic in whole cells: the offsets come
// from an integer hash of the vertex that repeats every UV wrap period, so
// two regions wrapped to different bases agree on every vertex.
//
// Distance. Detail reads fade into the layer's mean colour by a kilometre
// and the normal map sooner, a slow albedo variation over a few hundred
// metres keeps the mid field from going flat, and from there the biome tint
// takes over, so the horizon is a map of biomes and no texture is asked to
// survive being seen from ten kilometres.
//
// Paint (ticket 0006, pass 1). The ground is drawn as painted country under
// the physical light: the grain is lowered about each layer's mean, the
// normal maps soft and the ground matte; the baked cavity shade is in the
// albedo (material.rs folds it in when it packs the arrays); two layers
// meet along a drawn edge; living cover takes colour shapes (lush and dry,
// warm and cool) and its biome's palette hue near; and from a couple of
// kilometres the air lifts and greys it. TerrainParams holds every knob.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    forward_io::VertexOutput,
    forward_io::FragmentOutput,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}

// The dither is called by its module's path: imported by name, naga_oil
// would rename every token `visibility_range_dither` in this file, the field
// of TerrainVertexOutput too, and the variant with the dither would not
// compile.
#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions
#endif

#import strata::terrain_common::{TerrainParams, TerrainTint, TerrainVertex, morph_vertex, terrain_attributes, terrain_slots, tint_parent, tiling_of, value_of, grain_of}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var diffuse_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var normal_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> terrain: TerrainParams;
// The rivers' and roads' lines painted at distance, a slot a region by the
// same tag as its records (strata_terrain::paint, whose module docs have
// the slot's layout and the rules the paint functions below mirror). Only
// with TERRAIN_FAR_PAINT (TerrainConfig::paint): without it the fragment
// carries none of the paint's code.
#ifdef TERRAIN_FAR_PAINT
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var<storage, read> terrain_paint: array<u32>;
#endif

// Mirrors forward_io::VertexOutput location for location so the fragment
// stage can rebuild one for the standard PBR path, plus the terrain data.
//
// Every location is taken: 0 to 14 here, and the fragment's front_facing
// counts as the sixteenth of wgpu's 16 (15 on WebGL2). Do not add one; the
// rock pair was packed into widened locations for that reason (ticket 0004,
// step 3d), and tests/shaders.rs in strata-terrain holds the count.
struct TerrainVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    // The geometric normal in xyz; in w the blue of the near ground's
    // colour (see rock_pair).
    @location(1) world_normal: vec4<f32>,
    // The wrapped world xz in xy; widened rather than given a location of
    // their own, as there is none: in z how wet the water's edge is, in w
    // how much of a cut wall over a river the ground is (mesh.rs, "Water's
    // edge").
    @location(2) uv: vec4<f32>,
    @location(6) @interpolate(flat) instance_index: u32,
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    // The rock, read through the triangle's corners (see rock_mix and
    // mesh.rs, "Rock"). In xyz the barycentric weight of the vertex at each
    // corner, in w the wild share, how much of the rock steep ground may
    // take.
    @location(8) rock_corner: vec4<f32>,
    // Each corner's packed rock pair times its weight in xyz. The w here,
    // in rock_share and in world_normal are the near ground's colour, red,
    // green and blue: the biomes' ground colour without the provinces' far
    // tint (the tint over its far factor, mesh.rs `far_factors`), which the
    // near living cover's palette is pulled to (ticket 0006). Widened
    // rather than given a location, as there is none.
    @location(9) rock_pair: vec4<f32>,
    // The share of each layer, four per vector. Locations 3 to 5 are free
    // here: forward_io's uv_b, tangent and colour, which terrain has none of.
    @location(3) weights_a: vec4<f32>,
    @location(4) weights_b: vec4<f32>,
    @location(5) weights_c: vec4<f32>,
    @location(14) weights_d: vec4<f32>,
    // Biome tint, linear RGB, and in w the slope at which rock bares as
    // 1 - normal.y. Interpolated: a province's bare_slope blends across its
    // border, and read flat it stepped along the triangles.
    @location(10) tint: vec4<f32>,
    // Each corner's higher-layer share times its weight; in w the near
    // ground's green (see rock_pair).
    @location(11) rock_share: vec4<f32>,
    @location(12) cover: vec4<f32>,
    // In x the height above sea level: the vertical texture coordinate of
    // steep faces, and stable when the render origin moves. In y the baked
    // sky visibility, in zw the bent normal's x and z (lighting plan phase
    // 3a), morphed with the height; widened rather than given a location
    // of their own, as there is none.
    @location(13) height_sky: vec4<f32>,
}

@vertex
fn vertex(vertex: TerrainVertex) -> TerrainVertexOutput {
    var out: TerrainVertexOutput;
    // Everything but the instance comes from the region store.
    let v = terrain_attributes(vertex);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let morphed = morph_vertex(v, world_from_local, terrain, view.lod_view_world_position);
    out.world_position = morphed.world_position;
    out.position = position_world_to_clip(out.world_position.xyz);
    let normal = mesh_functions::mesh_normal_local_to_world(morphed.normal, vertex.instance_index);
    // The sky's spare bytes, unmorphed: the wet edge fades over a metre of
    // height, the wall over metres, and a coarse level's vertex carries its
    // own sample's either way.
    out.uv = vec4<f32>(v.uv, v.sky.w, v.sky_parent.w);
    out.instance_index = vertex.instance_index;
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    // Each vertex writes into the slot of its corner and leaves the other
    // two at zero, so the rasteriser hands the fragment every corner's
    // weight and, over it, every corner's pair and share.
    let corner = u32(round(v.material.w * 255.0));
    let slot = vec3<f32>(
        select(0.0, 1.0, corner == 0u),
        select(0.0, 1.0, corner == 1u),
        select(0.0, 1.0, corner == 2u),
    );
    // The tint morphs with the height to where the vertex lies on the
    // parent level's mesh, so a level swap does not change it under the
    // eye: the near ground's palette is pulled to it at every distance.
    var own: TerrainTint;
    own.tint = v.tint;
    own.far_factor = v.far_factor;
    let parent = tint_parent(vertex, own);
    let tint = mix(v.tint, parent.tint, morphed.morph_t);
    let far_factor = mix(v.far_factor, parent.far_factor, morphed.morph_t);
    let ground = tint.rgb / max(far_factor, vec3<f32>(1e-3));
    out.world_normal = vec4<f32>(normal, ground.b);
    out.rock_corner = vec4<f32>(slot, v.material.y);
    out.rock_pair = vec4<f32>(slot * round(v.material.x * 255.0), ground.r);
    out.rock_share = vec4<f32>(slot * v.material.z, ground.g);
    out.weights_a = v.weights_a;
    out.weights_b = v.weights_b;
    out.weights_c = v.weights_c;
    out.weights_d = v.weights_d;
    out.tint = tint;
    out.cover = v.cover;
    // The morphed local height is the world height: regions are placed
    // horizontally by their transform and stand at sea level.
    // The sky morphs with the height, so a node and its coarse neighbour
    // meet in it as they do in the ground.
    let sky = mix(v.sky, v.sky_parent, morphed.morph_t);
    out.height_sky = vec4<f32>(morphed.local_position.y, sky.x, sky.yz * 2.0 - 1.0);
    return out;
}

struct LayerSample {
    albedo: vec3<f32>,
    // Tangent-space normal, decoded to [-1, 1].
    normal: vec3<f32>,
    roughness: f32,
    // The layer's height map, white on top, for the height blend.
    height: f32,
    // The layer's mean colour, blended as the albedo is: what the near tint
    // measures the ground's hue by.
    mean: vec3<f32>,
    // One for a layer the ground's palette paints (TerrainParams::palette),
    // blended as the albedo is, so a border with sand or a road fades the
    // palette out with the layer.
    paint: f32,
}

// Wraps a lattice index into [0, n) so a hash of it repeats every n cells.
fn wrap_cell(i: vec2<i32>, n: i32) -> vec2<i32> {
    return ((i % n) + n) % n;
}

// An integer hash of a lattice point in [0, 1); stable across the whole
// world, where a sine-based hash breaks down.
fn hash_cell(p: vec2<i32>) -> f32 {
    var h = bitcast<u32>(p.x) * 0x8da6b343u ^ bitcast<u32>(p.y) * 0xd8163841u;
    h = h ^ (h >> 13u);
    h = h * 0x27d4eb2du;
    h = h ^ (h >> 15u);
    return f32(h & 0xffffu) / 65535.0;
}

// Two independent hashes of a lattice point, as a random offset in [0, 1)^2.
fn hash_cell2(p: vec2<i32>) -> vec2<f32> {
    return vec2<f32>(hash_cell(p), hash_cell(p + vec2<i32>(7919, 104729)));
}

// Smooth value noise in [0, 1] on a lattice of `n` cells per UV period.
fn value_noise(p: vec2<f32>, n: i32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash_cell(wrap_cell(i, n));
    let b = hash_cell(wrap_cell(i + vec2<i32>(1, 0), n));
    let c = hash_cell(wrap_cell(i + vec2<i32>(0, 1), n));
    let d = hash_cell(wrap_cell(i + vec2<i32>(1, 1), n));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The slow albedo variation: two octaves of value noise at 210 m and 70 m
// (both divide the 5040 m period), centred on one.
fn macro_variation(world_xz: vec2<f32>) -> f32 {
    let n = value_noise(world_xz / (terrain.uv_period / 24.0), 24) * 0.65
        + value_noise(world_xz / (terrain.uv_period / 72.0), 72) * 0.35;
    return 1.0 + terrain.macro_strength * (n - 0.5) * 2.0;
}

// The ground's colour shapes (ticket 0006, pass 1). Rec. 709 luminance,
// which every hue shift below gives back, so a patch is a colour and never
// a light or a dark: the value is macro_variation's and the light's.
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// What each end of the two shifts multiplies the albedo by before its
// luminance is given back: lush is a fresh green, dry a straw gold, warm
// an ochre and cool a blue green, a painter's second colours over a field.
const COLOUR_LUSH: vec3<f32> = vec3<f32>(0.72, 1.14, 0.66);
const COLOUR_DRY: vec3<f32> = vec3<f32>(1.24, 1.03, 0.56);
const COLOUR_WARM: vec3<f32> = vec3<f32>(1.12, 1.0, 0.82);
const COLOUR_COOL: vec3<f32> = vec3<f32>(0.90, 1.0, 1.12);
// The spread (standard deviation) of shape_field's two octaves about their
// middle before it is divided out, measured over the period with this
// file's hash: one octave of value_noise spreads 0.22, and 0.6 and 0.4 of
// two unrelated ones 0.159 (tests/shaders.rs measures it again).
const SHAPE_SD: f32 = 0.16;
// The colour shapes' two scales, as lattice cells per UV period: 12 cells
// is 420 m, the broad wash a field lies in, and 168 is 30 m, the patches
// within it. Both divide the 5040 m period.
const SHAPE_BROAD_CELLS: i32 = 12;
const SHAPE_PATCH_CELLS: i32 = 168;

// A colour field about zero with a spread of one: a broad octave and a
// patchy one, shifted by `salt` metres so two fields of it are unrelated.
// A shift keeps the field periodic: the lattice wraps on its cells.
fn shape_field(world_xz: vec2<f32>, salt: vec2<f32>) -> f32 {
    let broad = value_noise(
        (world_xz + salt) / (terrain.uv_period / f32(SHAPE_BROAD_CELLS)),
        SHAPE_BROAD_CELLS,
    );
    let patches = value_noise(
        (world_xz - salt) / (terrain.uv_period / f32(SHAPE_PATCH_CELLS)),
        SHAPE_PATCH_CELLS,
    );
    return (0.6 * (broad - 0.5) + 0.4 * (patches - 0.5)) / SHAPE_SD;
}

// The albedo shifted `amount` of the way, -1 to 1, towards `plus` or
// `minus`, at its own luminance.
fn hue_shift(albedo: vec3<f32>, amount: f32, plus: vec3<f32>, minus: vec3<f32>) -> vec3<f32> {
    let a = clamp(amount, -1.0, 1.0);
    let towards = select(minus, plus, a >= 0.0);
    let shifted = albedo * mix(vec3<f32>(1.0), towards, abs(a));
    return shifted * (dot(albedo, LUMA) / max(dot(shifted, LUMA), 1e-5));
}

// An albedo seen through `air_t` of the air (ticket 0006): some of its
// saturation lost and lifted towards the air's colour, by TerrainParams'
// air_desaturate and air_lift.
fn aired(albedo: vec3<f32>, air_t: f32) -> vec3<f32> {
    let grey = vec3<f32>(dot(albedo, LUMA));
    let muted = mix(albedo, grey, air_t * terrain.air_desaturate);
    return mix(muted, terrain.air_colour.rgb, air_t * terrain.air_lift);
}

// A colour's hue and saturation without its value: the colour over its
// luminance.
fn chroma(c: vec3<f32>) -> vec3<f32> {
    return c / max(dot(c, LUMA), 1e-4);
}

// Lattice cells per texture repeat. Four keeps a cell smaller than the
// features of the aerial sets, so the blend zones read as grain rather
// than as a quilt of differently textured diamonds.
const CELLS_PER_REPEAT: f32 = 4.0;

// The three lattice vertices a point blends between and their weights.
// Vertex (i, j) sits at (i + j / 2, j) in cell units: every row is shifted
// half a cell, so the cell is a parallelogram whose two halves are near
// equilateral triangles. The weights are the fragment's barycentric
// coordinates in its triangle.
struct StochasticCells {
    corner: array<vec2<i32>, 3>,
    weight: vec3<f32>,
}

fn stochastic_cells(p: vec2<f32>) -> StochasticCells {
    let j = floor(p.y);
    let u = p.x - 0.5 * j;
    let i = floor(u);
    let f = vec2<f32>(u - i, p.y - j);
    let base = vec2<i32>(i32(i), i32(j));
    var cells: StochasticCells;
    if f.x + f.y < 1.0 {
        cells.corner[0] = base;
        cells.corner[1] = base + vec2<i32>(1, 0);
        cells.corner[2] = base + vec2<i32>(0, 1);
        cells.weight = vec3<f32>(1.0 - f.x - f.y, f.x, f.y);
    } else {
        cells.corner[0] = base + vec2<i32>(1, 0);
        cells.corner[1] = base + vec2<i32>(0, 1);
        cells.corner[2] = base + vec2<i32>(1, 1);
        cells.weight = vec3<f32>(1.0 - f.y, 1.0 - f.x, f.x + f.y - 1.0);
    }
    return cells;
}

// The lattice vertex (i, j) as its canonical index inside one period of n
// cells (n even). Moving up n rows shifts the sheared lattice by n / 2
// cells, so the same world point one period north is vertex
// (i - n / 2, j + n); folding that back is what makes the hash periodic.
fn wrap_vertex(v: vec2<i32>, n: i32) -> vec2<i32> {
    let jw = ((v.y % n) + n) % n;
    let q = (v.y - jw) / n;
    let iw = (((v.x + q * (n / 2)) % n) + n) % n;
    return vec2<i32>(iw, jw);
}

// The layer's mean colour: the last mip of its slice. The level is clamped
// by the sampler to the smallest mip there is.
fn layer_mean_baked(layer: u32) -> vec3<f32> {
    return textureSampleLevel(diffuse_array, diffuse_sampler, vec2<f32>(0.5), layer, 24.0).rgb;
}

// What `mean` must be multiplied by for the layer to be drawn at its value
// budget (assets/look/values.ron): its target luminance over the luminance
// of the baked mean, or 1 for a layer with no target.
fn layer_gain(layer: u32, baked: vec3<f32>) -> f32 {
    let want = value_of(terrain, layer);
    return select(1.0, want / max(dot(baked, LUMA), 1e-4), want > 0.0);
}

// The layer's mean as drawn: the baked mean at its budgeted value.
fn layer_mean(layer: u32) -> vec3<f32> {
    let baked = layer_mean_baked(layer);
    return baked * layer_gain(layer, baked);
}

// The laid layers' stones (review step six, F7). The graph that bakes a
// paving (veyra_materials::terrain::city, `courses`) lays whole stones in
// courses, each course shifted by its own fraction of a stone and every joint
// moved a little; this repeats that layout, hash for hash, to name the stone a
// point is on, so each stone can take a value of its own on top of the baked
// colour. The joints are the bake's own (the check is in tools/content's
// terrain test), so a stone's tone steps exactly where its joint is drawn.

// Hoskins' sine-free hash of a whole-number index, as the bake writes it.
fn laid_hash(index: f32, salt: f32) -> f32 {
    var p = fract(index * 0.1031 + salt);
    p = p * (p + 33.33);
    return fract(p * (p + p));
}

// A whole number folded into 0..count, exactly.
fn laid_fold(index: f32, count: f32) -> f32 {
    return floor(fract(index / count) * count + 0.5);
}

// The stone a point of a layer's texture repeats lies on, as one number a
// stone: the bake's `flag_id`-style index hash, from its course and place in
// it.
fn laid_stone(uv: vec2<f32>, laid: vec4<f32>) -> f32 {
    let rows = laid.x;
    let stones = laid.y;
    let row = laid_fold(floor(uv.y * rows), rows);
    let across = uv.x * stones + laid_hash(row, 0.71);
    let slot = floor(across);
    let t = across - slot;
    let joint = (laid_hash(laid_fold(slot, stones) + row * stones, 0.53) - 0.5) * 2.0 * laid.z;
    let joint_next = (laid_hash(laid_fold(slot + 1.0, stones) + row * stones, 0.53) - 0.5) * 2.0 * laid.z;
    // A joint moved past the point puts it on the stone before, one moved
    // short of it on the stone after.
    let back = select(0.0, 1.0, joint >= t);
    let forward = select(0.0, 1.0, t >= 1.0 + joint_next);
    let column = laid_fold(slot + forward - back, stones);
    return column + row * stones;
}

// The value factor of the stone under a point: one, plus or minus the layer's
// tone, by a hash of the stone that is not the bake's own colour hash.
fn laid_tone(uv: vec2<f32>, laid: vec4<f32>) -> f32 {
    let h = laid_hash(laid_stone(uv, laid), 0.913);
    return 1.0 + (h - 0.5) * 2.0 * laid.w;
}

// A quarter-turn rotation `k` of a 2D vector, anticlockwise.
fn rotate_quarter(v: vec2<f32>, k: u32) -> vec2<f32> {
    switch k {
        case 1u: { return vec2<f32>(-v.y, v.x); }
        case 2u: { return -v; }
        case 3u: { return vec2<f32>(v.y, -v.x); }
        default: { return v; }
    }
}

// One layer on one plane: stochastic detail while it lasts, the mean colour
// beyond. `uv` is in metres on the plane, stable in the world. `detail_t`
// and `normal_t` are how much of the albedo detail and the normal map
// remain at this distance. Explicit gradients keep mip selection right
// across the cell borders, where the per-corner offsets would otherwise
// make the implicit derivatives jump, and let the reads sit inside a branch
// so the far field costs no texture bandwidth.
//
// Each lattice corner reads the texture shifted and turned by a quarter turn
// of its own, and the three reads are weighted by their heights as well as
// by distance, as in Mikkelsen's hex tiling: where two reads overlap, the
// stone that stands higher wins, so the blend zones read as ground and not
// as a double exposure.
fn sample_layer(
    metres: vec2<f32>,
    duv_dx: vec2<f32>,
    duv_dy: vec2<f32>,
    layer: u32,
    detail_t: f32,
    normal_t: f32,
) -> LayerSample {
    let tiling = tiling_of(terrain, layer);
    let uv = metres / tiling;
    let mean = layer_mean_baked(layer);
    // The value budget (assets/look/values.ron): the layer is drawn at its
    // target luminance, its detail scaled with it, so ground baked at 0.05
    // reads at the family's band and the far field blends from the same
    // value. The remap is one multiply at each return, on the albedo and the
    // mean alike; a layer with no target is drawn as baked.
    let gain = layer_gain(layer, mean);
    // A laid pattern (setts, flagstones) keeps its sheen and its contrast:
    // its joints are its shapes, and a low sun along a wet or worn street
    // is the look the dusk fixture was framed on. Loose ground goes matte
    // (TerrainParams::matte) and its grain recedes (grain).
    let patterned = ((terrain.patterned >> layer) & 1u) == 1u;
    let matte = select(terrain.matte, 0.0, patterned);
    var s: LayerSample;
    s.albedo = mean * gain;
    s.mean = mean * gain;
    s.paint = f32((terrain.palette >> layer) & 1u);
    s.normal = vec3<f32>(0.0, 0.0, 1.0);
    s.roughness = mix(
        textureSampleLevel(diffuse_array, diffuse_sampler, vec2<f32>(0.5), layer, 24.0).a,
        1.0,
        matte,
    );
    s.height = textureSampleLevel(normal_array, normal_sampler, vec2<f32>(0.5), layer, 24.0).a;
    if detail_t <= 0.0 {
        return s;
    }

    let ddx = duv_dx / tiling;
    let ddy = duv_dy / tiling;
    // A laid pattern (setts, flagstones) is read plainly: offset and
    // quarter-turned cells would turn its rows every few metres and double
    // its joints where they blend. Its own irregularity hides its repeat.
    if patterned {
        let colour = textureSampleGrad(diffuse_array, diffuse_sampler, uv, layer, ddx, ddy);
        let texel = textureSampleGrad(normal_array, normal_sampler, uv, layer, ddx, ddy);
        s.albedo = mix(mean, colour.rgb, detail_t) * gain;
        // Each laid stone takes a value of its own (assets/terrain/materials.ron
        // `laid`), faded in with the detail so the far plaza keeps its mean.
        let laid = terrain.laid[layer];
        if laid.w > 0.0 {
            s.albedo *= mix(1.0, laid_tone(uv, laid), detail_t);
        }
        if normal_t > 0.0 {
            let xy = texel.xy * 2.0 - 1.0;
            s.normal = vec3<f32>(
                xy * terrain.normal_strength * normal_t,
                max(texel.z * 2.0 - 1.0, 0.05),
            );
        }
        s.roughness = mix(s.roughness, colour.a, detail_t);
        // A laid pattern blends into its neighbours along one line, flat:
        // by its own height it would break out whole stones along its rows,
        // a sawtooth one flag deep, where loose ground scatters pebbles.
        s.height = 0.55;
        return s;
    }
    let grain = grain_of(terrain, layer);
#ifdef TERRAIN_SINGLE_TAP
    // The Low preset (review step six, F10): one plain read a map, the
    // repeat left to the macro variation and the colour shapes. One read has
    // no blend to lose contrast to, so the grain applies as it stands.
    {
        let colour = textureSampleGrad(diffuse_array, diffuse_sampler, uv, layer, ddx, ddy);
        let texel = textureSampleGrad(normal_array, normal_sampler, uv, layer, ddx, ddy);
        let a = mean + (colour.rgb - mean) * grain;
        s.albedo = mix(mean, max(a, vec3<f32>(0.0)), detail_t) * gain;
        if normal_t > 0.0 {
            let xy = texel.xy * 2.0 - 1.0;
            s.normal = vec3<f32>(
                xy * terrain.normal_strength * normal_t,
                max(texel.z * 2.0 - 1.0, 0.05),
            );
        }
        s.roughness = mix(s.roughness, mix(colour.a, 1.0, matte), detail_t);
        s.height = mix(s.height, texel.a, detail_t);
        return s;
    }
#endif
    let cells = stochastic_cells(uv * CELLS_PER_REPEAT);
    // Cells per period: CELLS_PER_REPEAT times a whole number of repeats,
    // so it is even, as wrap_vertex needs.
    let n = i32(CELLS_PER_REPEAT) * i32(round(terrain.uv_period / tiling));
    var albedo: array<vec3<f32>, 3>;
    var texel_normal: array<vec4<f32>, 3>;
    var roughness: array<f32, 3>;
    var weight: vec3<f32>;
    // A layer with a grain (a rock's beds, columns or cleavage) keeps its
    // direction: each cell shifts it along u alone and never turns it, so a
    // bed drawn level stays level on a cliff and runs on into the next cell.
    let oriented = ((terrain.oriented >> layer) & 1u) == 1u;
    for (var k = 0; k < 3; k++) {
        let corner = wrap_vertex(cells.corner[k], n);
        let shift = hash_cell2(corner);
        let offset = select(shift, vec2<f32>(shift.x, 0.0), oriented);
        let turn = select(u32(hash_cell(corner + vec2<i32>(3571, 6311)) * 3.999), 0u, oriented);
        let cuv = rotate_quarter(uv, turn) + offset;
        let cdx = rotate_quarter(ddx, turn);
        let cdy = rotate_quarter(ddy, turn);
        let colour = textureSampleGrad(diffuse_array, diffuse_sampler, cuv, layer, cdx, cdy);
        albedo[k] = colour.rgb;
        roughness[k] = colour.a;
        texel_normal[k] = textureSampleGrad(normal_array, normal_sampler, cuv, layer, cdx, cdy);
        // The normal's slope turns back with the read.
        let xy = rotate_quarter(texel_normal[k].xy * 2.0 - 1.0, (4u - turn) % 4u);
        texel_normal[k] = vec4<f32>(xy, texel_normal[k].z * 2.0 - 1.0, texel_normal[k].a);
        let w = cells.weight[k];
        weight[k] = w * w * w * (texel_normal[k].a + 0.1);
    }
    weight = weight / max(weight.x + weight.y + weight.z, 1e-5);
    var a = vec3<f32>(0.0);
    var nrm = vec3<f32>(0.0);
    var r = 0.0;
    var h = 0.0;
    for (var k = 0; k < 3; k++) {
        a += weight[k] * albedo[k];
        nrm += weight[k] * texel_normal[k].xyz;
        r += weight[k] * roughness[k];
        h += weight[k] * texel_normal[k].a;
    }
    // Variance-preserving blend: the weights sum to one, so the plain
    // average loses contrast by the norm of the weight vector. The grain
    // then lowers it to the painted ground's (TerrainParams::grain).
    let contrast = grain / max(length(weight), 1e-3);
    a = mean + (a - mean) * contrast;
    s.albedo = mix(mean, max(a, vec3<f32>(0.0)), detail_t) * gain;
    if normal_t > 0.0 {
        s.normal = vec3<f32>(nrm.xy * terrain.normal_strength * normal_t, max(nrm.z, 0.05));
    }
    s.roughness = mix(s.roughness, mix(r, 1.0, matte), detail_t);
    s.height = mix(s.height, h, detail_t);
    return s;
}

// A layer on steep ground, projected on the planes it faces rather than
// stretched down a cliff from above (Quilez's biplanar mapping). Each of the
// three world planes weighs in by how squarely the face looks at it, and a
// plane at under 1/sqrt(3) weighs nothing, so at most two are read and the
// choice of which two never jumps. `p` is the world point in stable
// coordinates (wrapped x and z, height above sea level). The detail normals
// are bent onto the surface in world space and handed back in the
// fragment's tangent frame (`t`, `b`, `nrm`).
//
// A layer with a grain that is level in the world (a sandstone's beds, a
// clay's bands, a basalt's colonnade) is read from the two upright planes
// alone, whose v is height: on a slope too gentle for them the top plane
// drew its beds as stripes straight across the ground, where beds outcrop
// along the contours, wider the gentler the slope. Only nearly level ground,
// which seldom bares, still takes the top plane.
fn sample_layer_biplanar(
    p: vec3<f32>,
    dp_dx: vec3<f32>,
    dp_dy: vec3<f32>,
    face: vec3<f32>,
    layer: u32,
    detail_t: f32,
    normal_t: f32,
    t: vec3<f32>,
    b: vec3<f32>,
    nrm: vec3<f32>,
) -> LayerSample {
    var w: vec3<f32>;
    if ((terrain.oriented >> layer) & 1u) == 1u {
        let side = vec2<f32>(abs(face.x), abs(face.z));
        var upright = side * side;
        upright = upright * upright;
        upright = upright / max(upright.x + upright.y, 1e-5);
        let level = 1.0 - smoothstep(0.1, 0.25, length(side));
        w = vec3<f32>(upright.x * (1.0 - level), level, upright.y * (1.0 - level));
    } else {
        let k = 0.57735;
        w = clamp((abs(face) - k) / (1.0 - k), vec3<f32>(0.0), vec3<f32>(1.0));
        w = w * w;
    }
    w = w / max(w.x + w.y + w.z, 1e-5);
    var out: LayerSample;
    out.albedo = vec3<f32>(0.0);
    out.mean = vec3<f32>(0.0);
    out.paint = 0.0;
    out.roughness = 0.0;
    out.height = 0.0;
    var bend = vec3<f32>(0.0);
    for (var axis = 0; axis < 3; axis++) {
        let weight = w[axis];
        if weight <= 0.0 {
            continue;
        }
        // Tangent and bitangent of the plane, right-handed about the face
        // it looks along, with green pointing at decreasing v as the maps
        // are drawn.
        var tangent: vec3<f32>;
        var bitangent: vec3<f32>;
        if axis == 0 {
            let side = sign(face.x);
            tangent = vec3<f32>(0.0, 0.0, -side);
            bitangent = vec3<f32>(0.0, 1.0, 0.0);
        } else if axis == 1 {
            let side = sign(face.y);
            tangent = vec3<f32>(1.0, 0.0, 0.0);
            bitangent = vec3<f32>(0.0, 0.0, -side);
        } else {
            let side = sign(face.z);
            tangent = vec3<f32>(side, 0.0, 0.0);
            bitangent = vec3<f32>(0.0, 1.0, 0.0);
        }
        let uv = vec2<f32>(dot(p, tangent), -dot(p, bitangent));
        let ddx = vec2<f32>(dot(dp_dx, tangent), -dot(dp_dx, bitangent));
        let ddy = vec2<f32>(dot(dp_dy, tangent), -dot(dp_dy, bitangent));
        let s = sample_layer(uv, ddx, ddy, layer, detail_t, normal_t);
        out.albedo += weight * s.albedo;
        out.mean += weight * s.mean;
        out.paint += weight * s.paint;
        out.roughness += weight * s.roughness;
        out.height += weight * s.height;
        bend += weight * (tangent * s.normal.x + bitangent * s.normal.y);
    }
    out.normal = vec3<f32>(dot(bend, t), dot(bend, b), 1.0);
    return out;
}

fn mix_layers(a: LayerSample, b: LayerSample, t: f32) -> LayerSample {
    var s: LayerSample;
    s.albedo = mix(a.albedo, b.albedo, t);
    s.mean = mix(a.mean, b.mean, t);
    s.paint = mix(a.paint, b.paint, t);
    s.normal = mix(a.normal, b.normal, t);
    s.roughness = mix(a.roughness, b.roughness, t);
    s.height = mix(a.height, b.height, t);
    return s;
}

// How much of `b` shows over `a` at a share `t` of `b`, by height: each layer
// stands at half its height map plus its share, and within a narrow band the
// higher one wins. A transition then follows the stones and plates of the
// two layers instead of fading one into the other across a triangle. Half
// the height, so a share of one always wins outright.
fn height_weight(a: LayerSample, b: LayerSample, t: f32) -> f32 {
    let band = 0.08;
    let ha = a.height * 0.5 + (1.0 - t);
    let hb = b.height * 0.5 + t;
    let top = max(ha, hb) - band;
    let wa = max(ha - top, 0.0);
    let wb = max(hb - top, 0.0);
    return wb / max(wa + wb, 1e-5);
}

fn height_mix(a: LayerSample, b: LayerSample, t: f32) -> LayerSample {
    return mix_layers(a, b, height_weight(a, b, t));
}

// The drawn edge where two layers meet (ticket 0006): 1 where the height
// blend is half and half, falling to nothing at either side faster than
// the blend does, so the line is narrower than the crossing. SEAM_POWER
// is how much faster: the blend's own 4t(1 - t) to this power.
const SEAM_POWER: f32 = 3.0;
// The change of the blend's weight across a pixel (fwidth) over which the
// drawn edge fades out: the line is under SEAM_LINE_W of the weight wide
// (seam_of over a half at 0.5 +- 0.23), so once a pixel spans a good part of
// the whole crossing the line is narrower than the pixel and a point sample
// of it would sparkle as the view moves, most at 20 to 300 m and grazing.
// There the layers meet by the blend alone, as before the drawn edge.
const SEAM_ALIAS_FADE: vec2<f32> = vec2<f32>(0.35, 0.9);

fn seam_of(w: f32) -> f32 {
    return pow(clamp(4.0 * w * (1.0 - w), 0.0, 1.0), SEAM_POWER);
}

// The drawn edge of a blend weight `w` whose screen change is `fw`
// (fwidth(w), taken by the caller in uniform control flow).
fn seam_resolved(w: f32, fw: f32) -> f32 {
    return seam_of(w) * (1.0 - smoothstep(SEAM_ALIAS_FADE.x, SEAM_ALIAS_FADE.y, fw));
}

// The three strongest layers at a fragment and their shares, strongest
// first. Three, because two is not enough where three laid materials meet
// (a lane, its kerb band and a square), and a fourth share is never more
// than a sliver.
struct Strongest {
    layer: vec3<u32>,
    share: vec3<f32>,
}

fn strongest(in: TerrainVertexOutput) -> Strongest {
    var shares = array<vec4<f32>, 4>(in.weights_a, in.weights_b, in.weights_c, in.weights_d);
    var out: Strongest;
    out.layer = vec3<u32>(0u);
    out.share = vec3<f32>(-1.0);
    for (var k = 0u; k < 16u; k = k + 1u) {
        let w = shares[k / 4u][k % 4u];
        if w > out.share.x {
            out.layer = vec3<u32>(k, out.layer.xy);
            out.share = vec3<f32>(w, out.share.xy);
        } else if w > out.share.y {
            out.layer = vec3<u32>(out.layer.x, k, out.layer.y);
            out.share = vec3<f32>(out.share.x, w, out.share.y);
        } else if w > out.share.z {
            out.layer.z = k;
            out.share.z = w;
        }
    }
    return out;
}

// Below this barycentric weight a corner's pair is not trusted, since it is
// recovered by dividing by that weight; its share fades out up to
// ROCK_DECODE_FULL so dropping it never steps. mesh.rs's DECODE_FROM and
// DECODE_FULL.
const ROCK_DECODE_FROM: f32 = 0.004;
const ROCK_DECODE_FULL: f32 = 0.02;

// The spread of the contact's three octaves of value noise about their
// middle, measured over a period (0.140; the quartiles are 0.40 and 0.60).
// The noise gathers round one half, so read as it is a rock at a share of
// 0.3 showed on a few islands and not on nearly a third of the face; the
// logistic curve through this spread makes it about uniform, so a rock
// covers about its share of a face, and never clamps, so a rock at no
// weight never shows.
const ROCK_CONTACT_SD: f32 = 0.14;

// The rock at a fragment: every layer its triangle's three corners name,
// with its weight. Three corners of two layers each is six at most.
struct RockMix {
    layer: array<u32, 6>,
    weight: array<f32, 6>,
    count: u32,
    total: f32,
}

fn rock_add(rocks: ptr<function, RockMix>, layer: u32, w: f32) {
    if w <= 0.0 {
        return;
    }
    for (var s = 0u; s < (*rocks).count; s++) {
        if (*rocks).layer[s] == layer {
            (*rocks).weight[s] += w;
            return;
        }
    }
    let n = (*rocks).count;
    (*rocks).layer[n] = layer;
    (*rocks).weight[n] = w;
    (*rocks).count = n + 1u;
}

// Decodes the three corners (mesh.rs, "Rock"; RockCorners::weights is the
// same on the CPU for the tests). Slot k of rock_corner is the barycentric
// weight of the vertex at corner k and slot k of rock_pair is that weight
// times its packed pair, so their quotient is the pair; the weights blend
// the three vertices' rock by where the fragment lies, and along an edge
// only its two vertices weigh, the same from either triangle.
fn rock_mix(in: TerrainVertexOutput) -> RockMix {
    var rocks: RockMix;
    rocks.count = 0u;
    rocks.total = 0.0;
    for (var k = 0; k < 3; k++) {
        let b = in.rock_corner[k];
        let trust = smoothstep(ROCK_DECODE_FROM, ROCK_DECODE_FULL, b);
        if trust <= 0.0 {
            continue;
        }
        let code = u32(clamp(round(in.rock_pair[k] / b), 0.0, 255.0));
        let hi_part = clamp(in.rock_share[k], 0.0, max(b, 0.0));
        rock_add(&rocks, code & 15u, trust * (b - hi_part));
        rock_add(&rocks, code >> 4u, trust * hi_part);
        rocks.total += trust * b;
    }
    return rocks;
}

// The layer a fragment draws: the layers stacked in index order, the one
// whose band of the cumulative weight the contact noise falls in. Between
// two rocks that is the higher where its share exceeds 1 - contact, so the
// border is sharp and ragged, as a real contact is, and wanders across the
// kilometre over which the weights blend.
fn rock_pick(rocks_in: RockMix, contact: f32) -> u32 {
    var rocks = rocks_in;
    let mark = contact * rocks.total;
    var best = 16u;
    var heaviest = 0u;
    var most = -1.0;
    for (var s = 0u; s < rocks.count; s++) {
        let layer = rocks.layer[s];
        var below = 0.0;
        for (var t = 0u; t < rocks.count; t++) {
            if rocks.layer[t] <= layer {
                below += rocks.weight[t];
            }
        }
        if mark < below && layer < best {
            best = layer;
        }
        if rocks.weight[s] > most {
            most = rocks.weight[s];
            heaviest = layer;
        }
    }
    return select(best, heaviest, best == 16u);
}

// The rock's colour from afar: its layers' mean colours by weight, so a
// border's two rocks blend smoothly where the contact is under a pixel.
fn rock_mean(rocks_in: RockMix) -> vec3<f32> {
    var rocks = rocks_in;
    var colour = vec3<f32>(0.0);
    for (var s = 0u; s < rocks.count; s++) {
        colour += rocks.weight[s] * layer_mean(rocks.layer[s]);
    }
    return colour / max(rocks.total, 1e-5);
}

// The contact noise in (0, 1): three octaves of value noise at 40, 12 and
// 4 m (all divide the 5040 m period), made about uniform (ROCK_CONTACT_SD).
// Two octaves left each island of the other rock a smooth oval, a decal on
// the face; the third frays its edge. Read on the planes the face looks
// along, as the rock itself is (sample_layer_biplanar), so a cliff's
// contact is as ragged up the face as across it: from x and z alone a
// cliff drew it as streaks down its whole height, and x and z leaned by
// the height still streaked the faces that look along the lean.
fn contact_octaves(p: vec2<f32>) -> f32 {
    return value_noise(p / (terrain.uv_period / 126.0), 126) * 0.58
        + value_noise(p / (terrain.uv_period / 420.0), 420) * 0.28
        + value_noise(p / (terrain.uv_period / 1260.0), 1260) * 0.14;
}

fn rock_contact(p: vec3<f32>, face: vec3<f32>) -> f32 {
    let k = 0.57735;
    var w = clamp((abs(face) - k) / (1.0 - k), vec3<f32>(0.0), vec3<f32>(1.0));
    w = w * w;
    w = w / max(w.x + w.y + w.z, 1e-5);
    var n = 0.0;
    if w.x > 0.0 {
        n += w.x * contact_octaves(p.zy);
    }
    if w.y > 0.0 {
        n += w.y * contact_octaves(p.xz);
    }
    if w.z > 0.0 {
        n += w.z * contact_octaves(p.xy);
    }
    // The logistic's 1.702 makes it the normal distribution's own curve to
    // within a hundredth.
    return 1.0 / (1.0 + exp(-1.702 * (n - 0.5) / ROCK_CONTACT_SD));
}

// How far a cut wall's beds wander, metres, as a share `wall` of one: three
// octaves of value noise along the ground at 37, 10 and 2.5 m (136, 504 and
// 2016 cells of the 5040 m period), so the bands rise and fall by metres
// along a gorge's wall, pinch where the middle octave crosses them, and
// break into offset lengths at the finest, as beds broken by joints do.
fn band_warp(world_xz: vec2<f32>, wall: f32) -> f32 {
    if wall <= 0.0 {
        return 0.0;
    }
    let slow = value_noise(world_xz / (terrain.uv_period / 136.0), 136) - 0.5;
    let middle = value_noise(world_xz / (terrain.uv_period / 504.0), 504) - 0.5;
    let fine = value_noise(world_xz / (terrain.uv_period / 2016.0), 2016) - 0.5;
    return wall * (slow * 12.0 + middle * 5.0 + fine * 3.0);
}

// How much of a cut wall's banding is let through: a gorge is mostly the
// rock's own colour with its beds faint in it, where every bed of the
// layer's texture at full contrast read as stripes a few metres apart.
const WALL_BANDS: f32 = 0.45;

// What water does to the ground at its edge: wet stone and sand are darker
// and less rough.
const WET_ALBEDO: f32 = 0.58;
const WET_ROUGHNESS: f32 = 0.32;

// The rock across the nearest edge of the picked rock's band of the
// cumulative weight, and how near that edge the contact noise lies, as the
// other rock's share at the contact: a half on the edge, none at
// ROCK_CONTACT_BAND from it.
struct RockEdge {
    other: u32,
    share: f32,
}

// How near an edge, in contact noise, the two rocks are height-mixed. So
// thin a band costs a second read on few fragments, and the contact follows
// the two rocks' own plates and grains instead of a smooth line.
const ROCK_CONTACT_BAND: f32 = 0.12;

fn rock_edge(rocks_in: RockMix, pick: u32, contact: f32) -> RockEdge {
    var rocks = rocks_in;
    let total = max(rocks.total, 1e-5);
    // The picked rock's band: the weight of every lower layer below it.
    var below = 0.0;
    var own = 0.0;
    for (var s = 0u; s < rocks.count; s++) {
        if rocks.layer[s] < pick {
            below += rocks.weight[s];
        } else if rocks.layer[s] == pick {
            own = rocks.weight[s];
        }
    }
    let start = below / total;
    let end = (below + own) / total;
    var edge: RockEdge;
    edge.other = pick;
    edge.share = 0.0;
    let down = contact - start;
    let up = end - contact;
    // The nearest layer below the band, or above it, whichever edge is nearer.
    var nearest = 16u;
    var d = 1.0;
    let has_down = start > 0.0;
    let has_up = end < 0.9999;
    if has_down && (!has_up || down < up) {
        d = down;
        var best = 0u;
        for (var s = 0u; s < rocks.count; s++) {
            let layer = rocks.layer[s];
            if layer < pick && layer >= best && rocks.weight[s] > 0.0 {
                best = layer;
                nearest = layer;
            }
        }
    } else if has_up {
        d = up;
        var best = 16u;
        for (var s = 0u; s < rocks.count; s++) {
            let layer = rocks.layer[s];
            if layer > pick && layer < best && rocks.weight[s] > 0.0 {
                best = layer;
                nearest = layer;
            }
        }
    }
    if nearest < 16u && d < ROCK_CONTACT_BAND {
        edge.other = nearest;
        edge.share = 0.5 * (1.0 - d / ROCK_CONTACT_BAND);
    }
    return edge;
}

#ifdef TERRAIN_FAR_PAINT
// Mirrors strata_terrain::paint: the slot's layout and the stroke's flags.
const PAINT_WORDS: u32 = 4356u;
const PAINT_GRID: u32 = 16u;
const PAINT_HEADER_WORDS: u32 = 4u;
const PAINT_ENTRY_WORD: u32 = 260u;
const PAINT_STROKE_WORDS: u32 = 6u;
const PAINT_KIND_RIVER: u32 = 1u;
const PAINT_FLAG_GAP: u32 = 16u;
const PAINT_FLAG_FORD: u32 = 32u;
// paint::DECK_DRAWN_M and DECK_FADE_M: within this a bridge's deck is drawn,
// and the paint leaves its gap to it.
const PAINT_DECK_DRAWN_M: f32 = 4000.0;
const PAINT_DECK_FADE_M: f32 = 200.0;
// river::RESOLVED_CELLS: how many spacings wide a river is for a level's
// water mesh to draw it.
const PAINT_RESOLVED_CELLS: f32 = 2.0;
// paint::BUCKET_MOST: the most strokes a bucket lists, which the packing
// holds every bucket to, so the bound never cuts one short.
const PAINT_MOST: u32 = 48u;

// The far water: the river's own dark colour (river_water.wgsl's absorption
// over a bed of a metre or two seen from above), and smooth enough that the
// sky shows in it at a low angle, as a river does from a ridge.
const PAINT_WATER_ALBEDO: vec3<f32> = vec3<f32>(0.018, 0.034, 0.040);
const PAINT_WATER_ROUGHNESS: f32 = 0.12;
// A road's carriageway is its material's mean colour where the paint takes
// it over from the splat, lifted with distance to this by PAINT_ROAD_LIFT_M:
// a laid or worn way is paler than the ground round it from a ridge, and
// the material's mean alone read as a dark thread under the far tint.
const PAINT_ROAD_LIFT: f32 = 1.5;
const PAINT_ROAD_LIFT_M: vec2<f32> = vec2<f32>(1000.0, 5000.0);

// What the paint covers at a fragment: the river's water, a road's
// carriageway under it (a ford's way) and over it, and that road's layer.
struct Paint {
    water: f32,
    road_under: f32,
    road_over: f32,
    road_layer: u32,
}

// paint::strip_cover: the share of a pixel `footprint` metres across that a
// strip `half_width` metres either side of a line covers at `d` from it.
fn strip_cover(d: f32, half_width: f32, footprint: f32) -> f32 {
    let lo = max(-half_width, d - 0.5 * footprint);
    let hi = min(half_width, d + 0.5 * footprint);
    return clamp((hi - lo) / footprint, 0.0, 1.0);
}

// paint::resolved_width.
fn paint_resolved(lod: u32, step: f32) -> f32 {
    return select(PAINT_RESOLVED_CELLS * step, step, lod == 0u);
}

// paint::mesh_share: how much of a river the water mesh draws here, which
// the paint leaves to it.
fn river_mesh_share(width: f32, lod: u32, step: f32, morph: f32) -> f32 {
    let parent_at = paint_resolved(lod + 1u, 2.0 * step);
    let here_at = paint_resolved(lod, step);
    let parent = smoothstep(parent_at, 1.2 * parent_at, width);
    let here = smoothstep(here_at, 1.2 * here_at, width);
    return max(parent, here * (1.0 - morph));
}

// paint::splat_share: how much of a road's carriageway the splat still
// draws here, which the paint leaves to it.
fn splat_share(width: f32, step: f32, morph: f32) -> f32 {
    if step >= width {
        return 0.0;
    }
    return smoothstep(1.0, 2.0, width / (step * (1.0 + morph)));
}

// The paint at a fragment of the region in slot `tag`: its bucket's strokes,
// each covering its share of the pixel across it (the pixel's footprint
// along the stroke's normal, from the UV derivatives), less what the near
// drawing still draws.
fn paint_at(
    tag: u32,
    world_xz: vec2<f32>,
    duv_dx: vec2<f32>,
    duv_dy: vec2<f32>,
    world_position: vec3<f32>,
) -> Paint {
    var out: Paint;
    out.water = 0.0;
    out.road_under = 0.0;
    out.road_over = 0.0;
    out.road_layer = 0u;
    let base = tag * PAINT_WORDS;
    let head = terrain_paint[base];
    if (head & 0xffffu) == 0u {
        return out;
    }
    let slot = terrain_slots[tag];
    let spot = world_xz - slot.uv_origin;
    let cell = vec2<u32>(clamp(
        floor(spot / slot.size * f32(PAINT_GRID)),
        vec2<f32>(0.0),
        vec2<f32>(f32(PAINT_GRID - 1u)),
    ));
    let bucket = terrain_paint[base + PAINT_HEADER_WORDS + cell.y * PAINT_GRID + cell.x];
    let start = bucket & 0xffffu;
    let count = min(bucket >> 16u, PAINT_MOST);
    if count == 0u {
        return out;
    }
    // The morph as the vertex stage reads it (terrain_common.wgsl's
    // morph_vertex), per fragment.
    let observer = view.lod_view_world_position;
    let horizontal = length(world_position.xz - observer.xz);
    let morph_start = terrain.morph_start * slot.size;
    let morph = clamp(
        (horizontal - morph_start) / max(terrain.morph_end * slot.size - morph_start, 1e-3),
        0.0,
        1.0,
    );
    // The buckets reach a spacing and a metre past a stroke's half width
    // (paint::bucket_margin_m), so the footprint is held to twice that.
    let reach = 2.0 * (slot.step + 1.0);
    // A bridge's deck is drawn by the observer's horizontal distance to
    // the crossing (veyra_sim::crossings::pieces_within), so its gap is
    // shown by the same measure: from altitude the distance to the camera
    // would show the road under a deck still drawn.
    let gap_shown = smoothstep(PAINT_DECK_DRAWN_M - PAINT_DECK_FADE_M, PAINT_DECK_DRAWN_M, horizontal);
    let stroke_word = base + (head >> 16u);
    var road_most = 0.0;
    for (var e = start; e < start + count; e++) {
        let word = terrain_paint[base + PAINT_ENTRY_WORD + e / 2u];
        let s = select(word >> 16u, word & 0xffffu, (e & 1u) == 0u);
        let at = stroke_word + s * PAINT_STROKE_WORDS;
        let a = vec2<f32>(bitcast<f32>(terrain_paint[at]), bitcast<f32>(terrain_paint[at + 1u]));
        let b = vec2<f32>(bitcast<f32>(terrain_paint[at + 2u]), bitcast<f32>(terrain_paint[at + 3u]));
        let widths = unpack2x16float(terrain_paint[at + 4u]);
        let flags = terrain_paint[at + 5u];
        let ab = b - a;
        let length2 = dot(ab, ab);
        let along = select(0.0, clamp(dot(spot - a, ab) / max(length2, 1e-12), 0.0, 1.0), length2 > 0.0);
        let d = length(spot - a - ab * along);
        let hw = mix(widths.x, widths.y, along);
        let normal = select(vec2<f32>(1.0, 0.0), vec2<f32>(-ab.y, ab.x) / sqrt(max(length2, 1e-12)), length2 > 1e-8);
        let footprint = clamp(abs(dot(duv_dx, normal)) + abs(dot(duv_dy, normal)), 0.01, reach);
        let cover = strip_cover(d, hw, footprint);
        if cover <= 0.0 {
            continue;
        }
        if (flags & 15u) == PAINT_KIND_RIVER {
            let left = 1.0 - river_mesh_share(2.0 * hw, slot.lod, slot.step, morph);
            out.water = max(out.water, cover * left);
        } else {
            let opacity = f32((flags >> 16u) & 255u) / 255.0;
            let carriageway = f32(flags >> 24u);
            var c = cover * opacity * (1.0 - splat_share(carriageway, slot.step, morph));
            if (flags & PAINT_FLAG_GAP) != 0u {
                c = c * gap_shown;
            }
            if (flags & PAINT_FLAG_FORD) != 0u {
                out.road_under = max(out.road_under, c);
            } else {
                out.road_over = max(out.road_over, c);
            }
            if c > road_most {
                road_most = c;
                out.road_layer = (flags >> 8u) & 255u;
            }
        }
    }
    return out;
}
#endif

@fragment
fn fragment(
    in: TerrainVertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    // The standard path wants a forward_io::VertexOutput. Our meshes carry
    // exactly position, normal and uv_0, so the optional fields of that
    // struct (uv_b, tangent, color) are compiled out and this is complete.
    var std_in: VertexOutput;
    std_in.position = in.position;
    std_in.world_position = in.world_position;
    std_in.world_normal = in.world_normal.xyz;
    std_in.uv = in.uv.xy;
    std_in.instance_index = in.instance_index;
#ifdef VISIBILITY_RANGE_DITHER
    std_in.visibility_range_dither = in.visibility_range_dither;
#endif
    var pbr_input = pbr_input_from_standard_material(std_in, is_front);

    let dist = length(in.world_position.xyz - view.world_position);
    let detail_t = 1.0 - smoothstep(terrain.detail_near, terrain.detail_far, dist);
    let normal_t = 1.0 - smoothstep(terrain.detail_near, terrain.normal_far, dist);
    let tint_t = smoothstep(terrain.tint_near, terrain.tint_far, dist) * terrain.tint_strength;
    let max_layer = 15u;

    // The UV derivatives, taken once in uniform control flow; every read
    // scales them by its layer's tiling. The world point in stable
    // coordinates (wrapped x and z, height above sea level) is what steep
    // faces are projected from.
    let world_xz = in.uv.xy;
    let duv_dx = dpdx(world_xz);
    let duv_dy = dpdy(world_xz);
    let stable = vec3<f32>(world_xz.x, in.height_sky.x, world_xz.y);
    let dp_dx = dpdx(stable);
    let dp_dy = dpdy(stable);

    // Splat. The strongest layers by their interpolated shares, blended by
    // height, then pushed towards the slope layer as the geometric slope
    // steepens. Most of the ground is one layer alone: read it once.
    let top = strongest(in);
    var surface = sample_layer(world_xz, duv_dx, duv_dy, top.layer.x, detail_t, normal_t);
    let sliver = 0.004;
    // Where two layers meet, the drawn edge (seam_of) darkens the ground.
    // The weights are kept past the branches so their screen change is
    // taken in uniform control flow, where a derivative is defined.
    var wb = 0.0;
    var wc = 0.0;
    if top.share.y > sliver {
        let b = sample_layer(world_xz, duv_dx, duv_dy, top.layer.y, detail_t, normal_t);
        wb = height_weight(surface, b, top.share.y / (top.share.x + top.share.y));
        surface = mix_layers(surface, b, wb);
        if top.share.z > sliver {
            let c = sample_layer(world_xz, duv_dx, duv_dy, top.layer.z, detail_t, normal_t);
            wc = height_weight(surface, c, top.share.z / (top.share.x + top.share.y + top.share.z));
            surface = mix_layers(surface, c, wc);
        }
    }
    let seam = max(seam_resolved(wb, fwidth(wb)), seam_resolved(wc, fwidth(wc)));
    surface.albedo *= 1.0 - terrain.seam_shade * seam * detail_t;

    // The colour shapes on living cover (ticket 0006): lush and dry, warm
    // and cool, in patches; lush in the hollows the baked sky visibility
    // finds and along damp banks, dry on the open crests. Laid ground, the
    // water's beds, sand, snow and rock take none (TerrainParams::palette).
    let living = surface.paint;
    // The value the near pull lifts the living ground by, kept for the far
    // field so the plain matches the ground as it is drawn, not the layers
    // before the pull.
    var near_lift = 1.0;
    if living > 0.0 {
        let hollow = clamp((0.95 - in.height_sky.y) * 3.0, -0.15, 0.8);
        // Through a soft knee (tanh) rather than the shift's clamp, so the
        // strongest patches round off instead of flattening into plateaus
        // of the one end colour.
        let lush = tanh(
            (0.8 * shape_field(world_xz, vec2<f32>(0.0)) + hollow + 0.5 * in.uv.z) * terrain.colour_lush,
        ) * living;
        let warm = tanh(0.8 * shape_field(world_xz, vec2<f32>(1733.0, 2511.0)) * terrain.colour_warm) * living;
        surface.albedo = hue_shift(surface.albedo, lush, COLOUR_LUSH, COLOUR_DRY);
        surface.albedo = hue_shift(surface.albedo, warm, COLOUR_WARM, COLOUR_COOL);
        // The palette near: the layers' hue, measured by their mean, moved
        // towards the biomes' ground colour, and their value a share of
        // that way (near_value), as a factor on the albedo, so the
        // texture's detail and the shapes above survive it. The ground
        // colour is the tint without the provinces' far tint, which is
        // the bedrock's colour from afar and no colour of grass or moss.
        let pull = terrain.near_tint * living;
        if pull > 0.0 {
            let ground = vec3<f32>(in.rock_pair.w, in.rock_share.w, in.world_normal.w);
            let hue = clamp(
                chroma(ground) / max(chroma(surface.mean), vec3<f32>(0.05)),
                vec3<f32>(0.25),
                vec3<f32>(4.0),
            );
            let value = clamp(
                dot(ground, LUMA) / max(dot(surface.mean, LUMA), 1e-4),
                0.5,
                2.5,
            );
            near_lift = mix(1.0, value, pull * terrain.near_value);
            surface.albedo *= mix(vec3<f32>(1.0), hue, pull) * near_lift;
        }
    }

    // Past the drawn trees, the ground under a forest is its canopy seen
    // from above: the trees fade in over ground already their colour.
    let cover_t = smoothstep(terrain.cover_near, terrain.cover_far, dist) * in.cover.a;
    surface.albedo = mix(surface.albedo, in.cover.rgb, cover_t);
    // The far field: the biome's own colour takes over so the horizon reads
    // as a map of biomes. Applied before the slope blend so cliffs keep
    // their own colour at any distance.
    // The biome's colour is a hue only: scaled to the luminance of the layers
    // drawn near (their blended mean, lifted as the near pull lifts them), so
    // the plain does not turn a lit band several times brighter than the
    // ground it blends from, nor a step darker than it.
    let far_hue = in.tint.rgb
        * (near_lift * dot(surface.mean, LUMA) / max(dot(in.tint.rgb, LUMA), 1e-4));
    surface.albedo = mix(surface.albedo, mix(far_hue, in.cover.rgb, in.cover.a), tint_t);

    // Planar top-down tangent frame: u runs along +x, and the OpenGL-style
    // normal maps have green pointing at decreasing v, so the bitangent is
    // -z. On slopes the frame is bent onto the face by Gram-Schmidt so the
    // detail still reads as bumps rather than shear.
    let N = pbr_input.N;
    var T = vec3<f32>(1.0, 0.0, 0.0);
    T = normalize(T - N * dot(T, N));
    let B = normalize(cross(N, T));

    let geometric_normal = normalize(in.world_normal.xyz);
    let slope = 1.0 - clamp(geometric_normal.y, 0.0, 1.0);
    // Laid ground takes no rock (a wild share of 0; see material.rs).
    let bare = in.tint.w;
    let slope_weight = smoothstep(
        terrain.slope_start * bare,
        terrain.slope_end * bare,
        slope,
    ) * in.rock_corner.w;
    // The rock's blend weight, kept past the branch for its drawn edge.
    var wr = 0.0;
    if slope_weight > 0.0 {
        // The province's rock, one of the few its corners name, by the
        // contact noise; read once, and twice beside a contact.
        let rocks = rock_mix(in);
        let contact = rock_contact(stable, geometric_normal);
        let pick = rock_pick(rocks, contact);
        // On a river valley's cut wall the beds are read at a height that
        // wanders with the wall, so a gorge's banded rock folds and pinches
        // along it instead of standing in ruled lines.
        let banded = vec3<f32>(stable.x, stable.y + band_warp(world_xz, in.uv.w), stable.z);
        var rock = sample_layer_biplanar(
            banded, dp_dx, dp_dy, geometric_normal,
            min(pick, max_layer), detail_t, normal_t,
            T, B, N,
        );
        // Beside the contact, the rock across it too, by height.
        let edge = rock_edge(rocks, pick, contact);
        if edge.share > 0.0 && detail_t > 0.0 {
            let other = sample_layer_biplanar(
                banded, dp_dx, dp_dy, geometric_normal,
                min(edge.other, max_layer), detail_t, normal_t,
                T, B, N,
            );
            rock = height_mix(rock, other, edge.share);
        }
        // As the detail fades the contact's islands turn to their rocks'
        // mean colours by weight: their finest octave is under a pixel by a
        // kilometre and the pick would shimmer between two means. Far faces
        // keep that colour, a quarter of the way to the tint and darkened,
        // so a sandstone country's scarps are red from ten kilometres and a
        // basalt one's dark, and a range's faces still contrast with its
        // snow.
        let settled = 1.0 - smoothstep(0.0, 0.3, detail_t);
        if settled > 0.0 || tint_t > 0.0 {
            let mean = rock_mean(rocks);
            rock.albedo = mix(rock.albedo, mean, settled);
            let far = mix(mean, in.tint.rgb, 0.25) * terrain.slope_tint;
            rock.albedo = mix(rock.albedo, far, tint_t);
        }
        // On a cut wall the beds fade towards the rocks' mean colour.
        let wall = in.uv.w;
        if wall > 0.0 {
            rock.albedo = mix(rock.albedo, rock_mean(rocks), wall * (1.0 - WALL_BANDS));
        }
        // By height, so rock breaks through along its own plates rather
        // than along the triangles the slope is measured on.
        wr = height_weight(surface, rock, slope_weight);
        surface = mix_layers(surface, rock, wr);
    }
    // With the ground's drawn edge where it does.
    surface.albedo *= 1.0 - terrain.seam_shade * seam_resolved(wr, fwidth(wr)) * detail_t;

    surface.albedo *= macro_variation(world_xz);
    // The water's edge: darker and glossier within a metre of the water
    // line, and the bed under the water with it; past the line the wet
    // bank's damp soil, a share of that, out to a few metres (mesh.rs,
    // `bank_of`).
    let wet = in.uv.z;
    // Matte (ticket 0006) is in each layer's roughness (sample_layer): only
    // water and laid stone make the ground shine.
    surface.albedo *= mix(1.0, WET_ALBEDO, wet);
    surface.roughness = mix(surface.roughness, WET_ROUGHNESS, wet);

    // Air over paint (ticket 0006): from a couple of kilometres the ground
    // loses some of its saturation and lifts towards the air's colour,
    // after the biome tint, so the far country reads as distance. The far
    // paint's roads take the same air below; its water does not, as the
    // river's own mesh draws a wide river unaired out to several
    // kilometres (a level draws a river two of its spacings wide, so one
    // of width w to about 160 w metres) and the paint must match it where
    // it takes over.
    let air_t = smoothstep(terrain.air_near, terrain.air_far, dist);
    surface.albedo = aired(surface.albedo, air_t);

    // The rivers and roads at distance, over everything the ground drew
    // (strata_terrain::paint): a ford's way, then the water, then the road
    // over it. Water is level and smooth, so the sky shows in it.
    var level = 0.0;
#ifdef TERRAIN_FAR_PAINT
    let paint = paint_at(
        mesh_functions::get_tag(in.instance_index),
        world_xz, duv_dx, duv_dy, in.world_position.xyz,
    );
    if paint.water > 0.0 || paint.road_under > 0.0 || paint.road_over > 0.0 {
        let lift = mix(1.0, PAINT_ROAD_LIFT, smoothstep(PAINT_ROAD_LIFT_M.x, PAINT_ROAD_LIFT_M.y, dist));
        let road = aired(layer_mean(paint.road_layer) * lift, air_t);
        // Matte as the splat draws the same way where the paint takes it
        // over (sample_layer), laid stone excepted.
        let road_laid = ((terrain.patterned >> paint.road_layer) & 1u) == 1u;
        let road_roughness = mix(
            textureSampleLevel(diffuse_array, diffuse_sampler, vec2<f32>(0.5), paint.road_layer, 24.0).a,
            1.0,
            select(terrain.matte, 0.0, road_laid),
        );
        let up_tangent = vec3<f32>(0.0, 0.0, 1.0);
        surface.albedo = mix(surface.albedo, road, paint.road_under);
        surface.roughness = mix(surface.roughness, road_roughness, paint.road_under);
        surface.albedo = mix(surface.albedo, PAINT_WATER_ALBEDO, paint.water);
        surface.roughness = mix(surface.roughness, PAINT_WATER_ROUGHNESS, paint.water);
        surface.normal = mix(surface.normal, up_tangent, max(paint.water, paint.road_under));
        surface.albedo = mix(surface.albedo, road, paint.road_over);
        surface.roughness = mix(surface.roughness, road_roughness, paint.road_over);
        surface.normal = mix(surface.normal, up_tangent, paint.road_over);
        level = paint.water * (1.0 - paint.road_over);
    }
#endif
    let nt = normalize(surface.normal);
    let mapped = normalize(mix(nt.x * T + nt.y * B + nt.z * N, vec3<f32>(0.0, 1.0, 0.0), level));

    pbr_input.material.base_color = vec4<f32>(surface.albedo, 1.0) * pbr_input.material.base_color;
    pbr_input.material.perceptual_roughness = clamp(surface.roughness, 0.05, 1.0);
    pbr_input.material.metallic = 0.0;
    pbr_input.N = mapped;

    // Sky visibility (lighting plan phase 3a). The baked share of the sky
    // the vertex sees scales the sky's fill, which is all the light shade
    // has, so a gully's floor is darker than the ridge above it. Trees are
    // not in the heightfield: under the drawn trees the ground loses the
    // share of the sky its canopy cover takes, fading as the ground turns
    // into the canopy's own colour past them. Nothing goes under a
    // quarter, so deep shade keeps the sky's colour.
    let canopy = 1.0 - in.cover.a * (1.0 - smoothstep(terrain.cover_near, terrain.cover_far, dist));
    let seen = max(in.height_sky.y * canopy, 0.25);
    let sky_occlusion = mix(1.0, seen, terrain.sky_visibility);
    // The sun keeps the mapped normal; the sky's light comes from where the
    // sky is open. Bending the lookup is, under a sky brighter above than
    // below, the ratio of a uniform sky's irradiance about the bent normal
    // to that about the mapped one, and it only bends where the ground is
    // occluded, so open ground keeps its normal map's shading.
    let bent_xz = in.height_sky.zw;
    let bent = normalize(vec3<f32>(bent_xz.x, sqrt(max(1.0 - dot(bent_xz, bent_xz), 0.0)), bent_xz.y));
    let sky_normal = normalize(mix(mapped, bent, terrain.sky_visibility * (1.0 - in.height_sky.y)));
    let turn = clamp((1.0 + sky_normal.y) / max(1.0 + mapped.y, 0.05), 0.5, 2.0);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * (sky_occlusion * turn) * terrain.shade_tint.xyz;
    pbr_input.specular_occlusion = pbr_input.specular_occlusion * sky_occlusion;

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
