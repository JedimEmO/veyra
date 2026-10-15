// Far trees: every tree of one species in one foliage cell drawn by one
// entity, a shared mesh of four corners per slot under the species' one
// material; a slot's tree is a record in the buffer every far tree shares,
// from the entity's tag on. Each quad is a hemi-octahedral impostor, read as
// `ashlar_bevy::impostor` reads one (three frames, depth parallax, lit by
// Bevy's standard lighting); what differs is where an instance comes from
// and that each tree fades in on its own distance with the dither pattern
// its mesh level fades out with, so the handover is exact per tree.

#import bevy_pbr::{
    mesh_functions,
    mesh_bindings::mesh,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::pbr_prepass_functions::calculate_motion_vector
#endif
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

// Mirrors strata_foliage::impostor::ImpostorBatchUniform.
struct TreeImpostor {
    // frames across, its reciprocal, texels across a frame, hemispherical.
    grid: vec4<f32>,
    // the bounding sphere's centre in the prop's frame, and its radius.
    sphere: vec4<f32>,
    // alpha cutoff, parallax strength, the sky term's strength (lighting
    // plan phase 3a), one unused.
    shading: vec4<f32>,
    // fade in from, fade in to, fade out from, fade out to: metres from the
    // tree's root, as Bevy's visibility ranges measure them.
    fade: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> impostor: TreeImpostor;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var impostor_albedo: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var impostor_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var impostor_normal_depth: texture_2d<f32>;

// Mirrors strata_foliage::impostor::record_bytes: 32 bytes. An empty slot
// has no scale.
struct FarTreeRecord {
    // The root in the cell's frame, and the scale.
    a: vec4<f32>,
    // cos yaw, sin yaw, and the tint hash in the low sixteen bits with what
    // the crowns take of the tree's sky in the next eight, as a whole
    // number; one unused.
    b: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var<storage, read> far_trees: array<FarTreeRecord>;

const POLE: f32 = 0.999;
const PARALLAX_STEPS: u32 = 2u;
// Below this height on screen a tree reads one frame with no parallax.
const SIMPLE_PIXELS: f32 = 48.0;
const DITHER_MAP: vec4<u32> = vec4(0x0a020800u, 0x060e040cu, 0x09010b03u, 0x050d070fu);

struct Placed {
    centre: vec3<f32>,
    scale: f32,
    rotation: mat3x3<f32>,
    root: vec3<f32>,
    tint: f32,
};

struct Basis {
    right: vec3<f32>,
    up: vec3<f32>,
};

struct Seen {
    albedo: vec4<f32>,
    normal: vec3<f32>,
};

struct Varying {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec4<f32>,
    @location(1) @interpolate(flat) sphere: vec4<f32>,
    @location(2) @interpolate(flat) axis_x: vec3<f32>,
    @location(3) @interpolate(flat) axis_y: vec3<f32>,
    @location(4) @interpolate(flat) axis_z: vec3<f32>,
    @location(5) @interpolate(flat) instance: u32,
    @location(6) @interpolate(flat) dither: i32,
    @location(7) @interpolate(flat) tint: f32,
    // The three frames this tree reads: the first's cell, then the grid
    // triangle's base cell.
    @location(8) @interpolate(flat) frames: vec4<f32>,
    // Their weights, first, beside and below, then one where the tree is
    // small enough on screen to read the heaviest frame alone.
    @location(9) @interpolate(flat) weights: vec4<f32>,
#ifdef MOTION_VECTOR_PREPASS
    @location(10) previous: vec4<f32>,
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(11) unclipped: f32,
#endif
};

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @builtin(vertex_index) vertex_index: u32,
    // The corner.
    @location(1) uv: vec2<f32>,
};

fn orthographic() -> bool {
    return view.clip_from_view[3].w == 1.0;
}

fn toward(point: vec3<f32>) -> vec3<f32> {
    if orthographic() {
        return normalize(view.world_from_view[2].xyz);
    }
    return normalize(view.world_position - point);
}

fn sign2(value: vec2<f32>) -> vec2<f32> {
    return select(vec2(1.0), vec2(-1.0), value < vec2(0.0));
}

fn encode(direction: vec3<f32>) -> vec2<f32> {
    if impostor.grid.w > 0.5 {
        let d = vec3(direction.x, max(direction.y, 0.0), direction.z);
        let norm = abs(d.x) + abs(d.y) + abs(d.z);
        let p = select(vec2(0.0), d.xz / norm, norm > 0.0);
        return (vec2(p.x + p.y, p.x - p.y) + 1.0) * 0.5;
    }
    let norm = abs(direction.x) + abs(direction.y) + abs(direction.z);
    var p = select(vec2(0.0), direction.xz / norm, norm > 0.0);
    if direction.y < 0.0 {
        p = (1.0 - abs(p.yx)) * sign2(p);
    }
    return (p + 1.0) * 0.5;
}

fn decode(grid: vec2<f32>) -> vec3<f32> {
    let g = grid * 2.0 - 1.0;
    if impostor.grid.w > 0.5 {
        let p = vec2((g.x + g.y) * 0.5, (g.x - g.y) * 0.5);
        let y = max(1.0 - abs(p.x) - abs(p.y), 0.0);
        return normalize(vec3(p.x, y, p.y));
    }
    let y = 1.0 - abs(g.x) - abs(g.y);
    var xz = g;
    if y < 0.0 {
        xz = (1.0 - abs(g.yx)) * sign2(g);
    }
    return normalize(vec3(xz.x, y, xz.y));
}

fn basis_of(direction: vec3<f32>) -> Basis {
    let reference = select(vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, -1.0), abs(direction.y) > POLE);
    let right = normalize(cross(reference, direction));
    return Basis(right, cross(direction, right));
}

// A tree of the cell, in the world.
fn place(world_from_local: mat4x4<f32>, a: vec4<f32>, b: vec4<f32>) -> Placed {
    let cell = mat3x3(
        normalize(world_from_local[0].xyz),
        normalize(world_from_local[1].xyz),
        normalize(world_from_local[2].xyz));
    // The yaw a mesh level's `Quat::from_rotation_y` gives the same tree.
    let yaw = mat3x3(vec3(b.x, 0.0, -b.y), vec3(0.0, 1.0, 0.0), vec3(b.y, 0.0, b.x));
    let rotation = cell * yaw;
    let root = (world_from_local * vec4(a.xyz, 1.0)).xyz;
    let centre = root + rotation * (impostor.sphere.xyz * a.w);
    return Placed(centre, a.w, rotation, root, b.z);
}

fn corner_of(placed: Placed, corner: vec2<f32>) -> vec3<f32> {
    let radius = impostor.sphere.w * placed.scale;
    let to_eye = toward(placed.centre);
    var right = cross(view.world_from_view[1].xyz, to_eye);
    if length(right) < 1e-4 {
        right = view.world_from_view[0].xyz;
    }
    right = normalize(right);
    let up = cross(to_eye, right);
    var grow = 1.0;
    if !orthographic() {
        let distance = length(view.world_position - placed.centre);
        let square = distance * distance;
        grow = min(distance / sqrt(max(square - radius * radius, 0.01 * square)), 4.0);
    }
    return placed.centre + (right * corner.x + up * corner.y) * (radius * grow);
}

// Bevy's visibility range mapping (`get_visibility_range_dither_level`), on
// the tree's own root rather than the entity's.
fn dither_level(root: vec3<f32>) -> i32 {
    let distance = length(view.lod_view_world_position.xyz - root);
    let range = impostor.fade;
    let fading_out = distance >= range.z;
    let offset = select(-16, 0, fading_out);
    let bounds = select(range.xy, range.zw, fading_out);
    let level = i32(round((distance - bounds.x) / max(bounds.y - bounds.x, 1e-3) * 16.0));
    return offset + clamp(level, 0, 16);
}

// Bevy's `visibility_range_dither`, so the two halves of a handover split
// every pixel between them.
fn apply_dither(frag_coord: vec4<f32>, level: i32) {
    if level == 0 {
        return;
    }
    if level <= -16 || level >= 16 {
        discard;
    }
    let coords = vec2<u32>(floor(frag_coord.xy)) % 4u;
    let threshold = i32((DITHER_MAP[coords.y] >> (coords.x * 8u)) & 0xffu);
    if (level >= 0 && level + threshold >= 16) || (level < 0 && 1 + level + threshold <= 0) {
        discard;
    }
}

fn frame_uv(point: vec3<f32>, basis: Basis) -> vec2<f32> {
    let across = 2.0 * impostor.sphere.w;
    return vec2(dot(point, basis.right) / across + 0.5, 0.5 - dot(point, basis.up) / across);
}

fn atlas_uv(frame: vec2<f32>, uv: vec2<f32>) -> vec2<f32> {
    let margin = 0.5 / impostor.grid.z;
    return (frame + clamp(uv, vec2(margin), vec2(1.0 - margin))) * impostor.grid.y;
}

// The gradients every read of one frame uses, so the parallax steps cannot
// jump the mip level at a silhouette. Taken for all three frames before the
// first `discard`: a browser's shader compiler (Tint) calls a derivative after
// a discard non-uniform and refuses the module. (dx, dy) as one vec4.
fn frame_grads(frame: vec2<f32>, point: vec3<f32>, ray: vec3<f32>) -> vec4<f32> {
    let direction = decode((frame + 0.5) * impostor.grid.y);
    let basis = basis_of(direction);
    let denominator = min(dot(ray, direction), -0.05);
    let along = dot(point, direction);
    let flat_uv = atlas_uv(frame, frame_uv(point - ray * (along / denominator), basis));
    return vec4(dpdx(flat_uv), dpdy(flat_uv));
}

fn read_frame(frame: vec2<f32>, weight: f32, point: vec3<f32>, ray: vec3<f32>, steps: u32, grads: vec4<f32>) -> Seen {
    let direction = decode((frame + 0.5) * impostor.grid.y);
    let basis = basis_of(direction);
    let denominator = min(dot(ray, direction), -0.05);
    let along = dot(point, direction);
    let dx = grads.xy;
    let dy = grads.zw;
    var uv = frame_uv(point - ray * (along / denominator), basis);
    for (var step = 0u; step < steps; step++) {
        let depth = textureSampleGrad(impostor_normal_depth, impostor_sampler, atlas_uv(frame, uv), dx, dy).a;
        let height = impostor.sphere.w * (1.0 - 2.0 * depth) * impostor.shading.y;
        uv = frame_uv(point + ray * ((height - along) / denominator), basis);
    }
    let atlas = atlas_uv(frame, uv);
    let albedo = textureSampleGrad(impostor_albedo, impostor_sampler, atlas, dx, dy);
    let normal = textureSampleGrad(impostor_normal_depth, impostor_sampler, atlas, dx, dy).xyz * 2.0 - 1.0;
    let inside = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let coverage = select(0.0, albedo.a, inside) * weight;
    return Seen(vec4(albedo.rgb * coverage, coverage), normal * coverage);
}

struct Choice {
    frames: vec4<f32>,
    weights: vec4<f32>,
};

// The three frames of the grid triangle the view of this tree lands in, and
// their barycentric weights: per tree, so every fragment reads the same.
fn choose(placed: Placed) -> Choice {
    let eye = transpose(placed.rotation) * toward(placed.centre);
    let frames = impostor.grid.x;
    let continuous = clamp(encode(eye) * frames - 0.5, vec2(0.0), vec2(frames - 1.0));
    let base = min(floor(continuous), vec2(frames - 2.0));
    let f = continuous - base;
    let lower = f.x + f.y < 1.0;
    let first = select(base + 1.0, base, lower);
    let first_weight = select(f.x + f.y - 1.0, 1.0 - f.x - f.y, lower);
    let beside = select(1.0 - f.y, f.x, lower);
    let below = select(1.0 - f.x, f.y, lower);
    return Choice(vec4(first, base), vec4(first_weight, beside, below, 0.0));
}

// The frame with the largest weight, of the three.
fn heaviest(frames: vec4<f32>, weights: vec4<f32>) -> vec2<f32> {
    var frame = frames.xy;
    var best = weights.x;
    if weights.y > best {
        frame = frames.zw + vec2(1.0, 0.0);
        best = weights.y;
    }
    if weights.z > best {
        frame = frames.zw + vec2(0.0, 1.0);
    }
    return frame;
}

fn read_impostor(world: vec3<f32>, placed: Placed, frames: vec4<f32>, weights: vec4<f32>) -> Seen {
    let inverse = transpose(placed.rotation);
    var ray = normalize(world - view.world_position);
    if orthographic() {
        ray = -normalize(view.world_from_view[2].xyz);
    }
    ray = inverse * ray;
    let point = inverse * (world - placed.centre) / placed.scale;
    let main = heaviest(frames, weights);
    let grads_a = frame_grads(frames.xy, point, ray);
    let grads_b = frame_grads(frames.zw + vec2(1.0, 0.0), point, ray);
    let grads_c = frame_grads(frames.zw + vec2(0.0, 1.0), point, ray);
    // The same pick `heaviest` makes.
    let main_grads = select(select(grads_a, grads_b, weights.y > weights.x), grads_c, weights.z > max(weights.x, weights.y));
    // Most of a quad is sky between branches: one read of the heaviest
    // frame on its centre plane throws those texels away before the blend.
    let probe = read_frame(main, 1.0, point, ray, 0u, main_grads);
    if probe.albedo.a < impostor.shading.x * 0.5 {
        discard;
    }
    // A tree a few dozen pixels tall shows no parallax and no blend.
    if weights.w > 0.5 {
        var normal = toward(placed.centre);
        if dot(probe.normal, probe.normal) > 1e-10 {
            normal = placed.rotation * normalize(probe.normal);
        }
        return Seen(vec4(probe.albedo.rgb / max(probe.albedo.a, 1e-5), probe.albedo.a), normal);
    }
    let a = read_frame(frames.xy, weights.x, point, ray, PARALLAX_STEPS, grads_a);
    let b = read_frame(frames.zw + vec2(1.0, 0.0), weights.y, point, ray, PARALLAX_STEPS, grads_b);
    let c = read_frame(frames.zw + vec2(0.0, 1.0), weights.z, point, ray, PARALLAX_STEPS, grads_c);
    let coverage = a.albedo.a + b.albedo.a + c.albedo.a;
    let colour = (a.albedo.rgb + b.albedo.rgb + c.albedo.rgb) / max(coverage, 1e-5);
    let local = a.normal + b.normal + c.normal;
    var normal = toward(placed.centre);
    if dot(local, local) > 1e-10 {
        normal = placed.rotation * normalize(local);
    }
    return Seen(vec4(colour, coverage), normal);
}

fn unpack(in: Varying) -> Placed {
    return Placed(in.sphere.xyz, in.sphere.w, mat3x3(in.axis_x, in.axis_y, in.axis_z), vec3(0.0), in.tint);
}

@vertex
fn vertex(vertex: Vertex) -> Varying {
    var out: Varying;
    // The slot this corner belongs to, in the range the entity's tag starts.
    let local_index = vertex.vertex_index - mesh[vertex.instance_index].first_vertex_index;
    let index = mesh_functions::get_tag(vertex.instance_index) + local_index / 4u;
    var record = FarTreeRecord(vec4(0.0), vec4(0.0));
    if index < arrayLength(&far_trees) {
        record = far_trees[index];
    }
    if record.a.w <= 0.0 {
        // An empty slot past the batch's trees.
        out.clip = vec4(0.0, 0.0, 2.0, 1.0);
        out.dither = 16;
        return out;
    }
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let a = record.a;
    let b = vec4(record.b.xyz, 0.0);
    let placed = place(world_from_local, a, b);
    let corner = vec2(vertex.uv.x * 2.0 - 1.0, 1.0 - vertex.uv.y * 2.0);
    let world = corner_of(placed, corner);
    out.clip = position_world_to_clip(world);
    out.world = vec4(world, 1.0);
    out.sphere = vec4(placed.centre, placed.scale);
    out.axis_x = placed.rotation[0];
    out.axis_y = placed.rotation[1];
    out.axis_z = placed.rotation[2];
    out.instance = vertex.instance_index;
    out.dither = dither_level(placed.root);
    out.tint = placed.tint;
    let choice = choose(placed);
    out.frames = choice.frames;
    out.weights = choice.weights;
    // Pixels the tree's height spans: the projection's focal length over
    // the distance, times half the screen.
    let distance = max(length(view.world_position - placed.centre), 1.0);
    let pixels = impostor.sphere.w * 2.0 * placed.scale * view.clip_from_view[1][1] / distance
        * view.viewport.w * 0.5;
    out.weights.w = select(0.0, 1.0, pixels < SIMPLE_PIXELS);
    if out.dither <= -16 || out.dither >= 16 {
        // Wholly the mesh level's, or past the far fade: nothing to draw.
        out.clip = vec4(0.0, 0.0, 2.0, 1.0);
    }
#ifdef MOTION_VECTOR_PREPASS
    let previous = place(mesh_functions::get_previous_world_from_local(vertex.instance_index), a, b);
    out.previous = vec4(world + previous.centre - placed.centre, 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped = out.clip.z;
    out.clip.z = min(out.clip.z, 1.0);
#endif
    return out;
}

// The tint hash as a small brightness and warmth lean, as a mesh level's
// `tag_tint` gives the same tree.
fn tint_of(tint: f32) -> vec3<f32> {
    var h = (u32(tint) & 0xffffu) * 0x9e3779b1u;
    h = h ^ (h >> 15u);
    let a = f32(h & 0xffu) / 255.0;
    let b = f32((h >> 8u) & 0xffu) / 255.0;
    let bright = 0.8 + 0.4 * a;
    return vec3<f32>(bright * (1.0 + 0.12 * (b - 0.5)), bright, bright * (1.0 - 0.12 * (b - 0.5)));
}

#ifdef PREPASS_PIPELINE
#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: Varying) -> prepass_io::FragmentOutput {
    let seen = read_impostor(in.world.xyz, unpack(in), in.frames, in.weights);
    apply_dither(in.clip, in.dither);
    if seen.albedo.a < impostor.shading.x {
        discard;
    }
    var out: prepass_io::FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped;
#endif
#ifdef NORMAL_PREPASS
    out.normal = vec4(seen.normal * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = calculate_motion_vector(in.world, in.previous);
#endif
    return out;
}
#else
@fragment
fn fragment(in: Varying) {
    let seen = read_impostor(in.world.xyz, unpack(in), in.frames, in.weights);
    apply_dither(in.clip, in.dither);
    if seen.albedo.a < impostor.shading.x {
        discard;
    }
}
#endif
#else
@fragment
fn fragment(in: Varying) -> FragmentOutput {
    let seen = read_impostor(in.world.xyz, unpack(in), in.frames, in.weights);
    apply_dither(in.clip, in.dither);
    if seen.albedo.a < impostor.shading.x {
        discard;
    }
    var standard: VertexOutput;
    standard.position = in.clip;
    standard.world_position = in.world;
    standard.world_normal = seen.normal;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    standard.instance_index = in.instance;
#endif
    var pbr_input = pbr_input_from_standard_material(standard, true);
    pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * seen.albedo.rgb * tint_of(in.tint), 1.0);
    // Sky under crowns (lighting plan phase 3a), as the mesh levels take
    // it: the placer's share at the crown's underside, opening towards the
    // top of the picture's sphere, never under a quarter.
    let crown_sky = 1.0 - f32((u32(in.tint) >> 16u) & 0xffu) / 255.0;
    let up = clamp(0.5 + (in.world.y - in.sphere.y) / max(2.0 * impostor.sphere.w * in.sphere.w, 1e-3), 0.0, 1.0);
    let sky_occlusion = mix(1.0, max(mix(crown_sky, 1.0, 0.6 * up), 0.25), impostor.shading.z);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * sky_occlusion;
    pbr_input.specular_occlusion = pbr_input.specular_occlusion * sky_occlusion;
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
#endif
