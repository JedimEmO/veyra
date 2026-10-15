// Rivers and lakes for `strata_terrain::river::RiverMaterial` (ticket 0005,
// step 3): the sea's water (`water_common.wgsl`) over a region's own surface.
//
// - The vertex stage morphs the surface towards the parent level's as the
//   terrain's does (`terrain_common.wgsl`'s morph_vertex, by the same band
//   in node sizes), so the water meets the ground at every level of detail;
//   its depth over the ground morphs with it.
// - The surface's normal is the river's own slope with ripples on it: the
//   wave image read along the flow by a flow map (two phases half a cycle
//   apart, each sliding the image downstream at the water's speed and
//   fading out as it starts over, so the ripples run with the river without
//   stretching), and the sea's chop at a fraction of its slope, which is all
//   a lake's still water has.
// - Foam where the surface is steep (rapids, and a fall's ramp, which is all
//   white) and in a thin band at the bank, both frayed by the foam noise
//   carried with the flow.
// - The surface writes its depth, so the sea plane does not draw in a
//   channel whose bed is under the sea's level; where the river comes down
//   to that level it is discarded and the sea draws the mouth. At the
//   waterline it fades in over a few centimetres of depth, so the line the
//   depth test cuts is never a hard flicker.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
}

#import strata::water_common::{water, wave_texture, wave_sampler, perp, frame_uv, floor_behind, water_light}

// Mirrors strata_terrain::river::RiverParams field for field.
struct RiverParams {
    // The terrain's morph band in node sizes in xy, the sea's level in z.
    morph: vec4<f32>,
    // Absorption per metre in x, the flow's ripple slope in y, the still
    // water's in z, the bank's foam band in metres of depth in w.
    look: vec4<f32>,
    // Where steep water starts to foam and where it is all foam, as rise
    // over run, in xy; the flow cycle's seconds in z.
    foam: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> river: RiverParams;

// How far from the camera the rivers' foam is drawn, metres: rapids and
// falls, and the bank's flecks.
const FOAM_RANGE: f32 = 600.0;
const BANK_FOAM_RANGE: f32 = 160.0;

// Mirrors strata_terrain::river::FADES: added to the kind's code where a
// creek fades out over the morph.
const FADES: f32 = 4.0;

struct RiverVertex {
    @builtin(instance_index) instance_index: u32,
    // Metres from the region's corner, the surface's height in y.
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The parent level's surface and its depth, the region's side, the kind.
    @location(2) morph: vec4<f32>,
    // The flow, m/s, in xy; the depth over the ground in z; the slope in w.
    @location(3) flow: vec4<f32>,
}

struct RiverOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    // The flow in xy, the morphed depth in z, the slope in w.
    @location(2) flow: vec4<f32>,
    // The kind in x, the surface's height over sea level in y, in z how
    // much of a creek the parent level does not draw is left.
    @location(3) body: vec3<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
}

@vertex
fn vertex(v: RiverVertex) -> RiverOutput {
    var out: RiverOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    // The terrain's morph: horizontal distance from the observer that
    // drives the levels, in node sizes.
    let unmorphed = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(v.position, 1.0),
    );
    let observer = view_bindings::view.lod_view_world_position;
    let distance = length(unmorphed.xz - observer.xz);
    let size = v.morph.z;
    let start = river.morph.x * size;
    let end = river.morph.y * size;
    let t = clamp((distance - start) / max(end - start, 1e-3), 0.0, 1.0);
    let height = mix(v.position.y, v.morph.x, t);
    let local = vec3<f32>(v.position.x, height, v.position.z);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
    out.flow = vec4<f32>(v.flow.xy, mix(v.flow.z, v.morph.y, t), v.flow.w);
    // Regions stand at sea level, so the local height is the world's. A
    // kind over FADES is a creek the parent level leaves out: it fades as
    // the vertex morphs towards that level.
    let fades = v.morph.w > FADES - 0.5;
    let kind = select(v.morph.w, v.morph.w - FADES, fades);
    out.body = vec3<f32>(kind, height - river.morph.z, select(1.0, 1.0 - t, fades));
    out.instance_index = v.instance_index;
    return out;
}

@fragment
fn fragment(
    in: RiverOutput,
    @builtin(front_facing) is_front: bool,
#ifdef MULTISAMPLED
    @builtin(sample_index) sample_index: u32,
#endif
) -> FragmentOutput {
#ifdef MULTISAMPLED
    let sample = sample_index;
#else
    let sample = 0u;
#endif
    // The standard path's input, from the fields this mesh has.
    var std_in: VertexOutput;
    std_in.position = in.position;
    std_in.world_position = in.world_position;
    std_in.world_normal = in.world_normal;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    std_in.instance_index = in.instance_index;
#endif
    let pbr_input = pbr_input_from_standard_material(std_in, is_front);
    let p = in.world_position.xyz;
    let camera = view_bindings::view.world_position;
    let range = length(p - camera);

    // The flow map: two phases half a cycle apart, each the wave image slid
    // downstream by the flow times its time into the cycle, weighted by a
    // tent so each is gone as it jumps back. The coordinate is the world's
    // (the origin wrapped to the tile), so a rebase moves nothing.
    let flow = in.flow.xy;
    let speed = length(flow);
    let inv_tile = water.flow.z;
    let base = (p.xz + water.flow.xy) * inv_tile;
    let phase_a = water.flow.w;
    let phase_b = fract(phase_a + 0.5);
    let cycle = river.foam.z;
    let uv_a = base - flow * (phase_a * cycle * inv_tile);
    let uv_b = base - flow * (phase_b * cycle * inv_tile) + vec2<f32>(0.37, 0.61);
    let weight_a = 1.0 - abs(2.0 * phase_a - 1.0);
    // Every read before any branch, so the mips' derivatives hold.
    let ripple_a = textureSample(wave_texture, wave_sampler, uv_a);
    let ripple_b = textureSample(wave_texture, wave_sampler, uv_b);
    let chop_heading = water.axes.zw;
    let chop = textureSample(wave_texture, wave_sampler, frame_uv(p.xz, chop_heading, water.chop));
    let ripple = mix(ripple_b, ripple_a, weight_a);
    let slow_noise = textureSample(wave_texture, wave_sampler, frame_uv(p.xz, water.axes.xy, water.foam)).a;

    // The surface: the river's own slope, and on it the flow's ripples (as
    // strong as the water is fast) and the wind's chop scaled down, faded
    // out with distance as the sea's waves are.
    let detail = 1.0 - smoothstep(water.fade.x, water.fade.y, range);
    let flowing = smoothstep(0.05, 0.4, speed);
    let gr = (ripple.rg * 2.0 - 1.0) * river.look.y * flowing;
    let gc = (chop.rg * 2.0 - 1.0) * water.chop.w * river.look.z;
    let gradient = (gr + gc.x * chop_heading + gc.y * perp(chop_heading)) * detail;
    let surface_normal = normalize(in.world_normal);
    let N = normalize(surface_normal - vec3<f32>(gradient.x, 0.0, gradient.y));
    let roughness = mix(water.fade.z, pbr_input.material.perceptual_roughness, detail);

    let behind = floor_behind(in.position, p, sample);

    // Foam: a thin band at the bank, by the depth over the ground, which is
    // nothing exactly where the surface meets the ground; steep water, more
    // of it the steeper; and a fall, all of it.
    // The flow's foam noise repeats with its ripples' few-metre tile, which
    // read as dashes down a creek; the sea's slow foam frame over it breaks
    // the repeat.
    let noise = 0.5 * (mix(ripple_b.a, ripple_a.a, weight_a) + slow_noise);
    let depth = in.flow.z;
    let bank = 1.0 - saturate(depth / max(river.look.w, 1e-3));
    // A river's bank foams in flecks the flow carries, not in the sea's
    // unbroken band: whole, it read as a line painted on a road, and with
    // the bank's nearness in the threshold the last centimetres of depth
    // still foamed in an unbroken thread. The noise alone picks the flecks;
    // still water's bank barely foams. Only near: a creek narrower than a
    // coarse level's spacing is drawn as a zigzag of cells, and a band of
    // foam along it read as dashes.
    let bank_foam = smoothstep(0.62, 0.85, noise) * bank * (0.15 + 0.85 * flowing)
        * (1.0 - smoothstep(BANK_FOAM_RANGE * 0.5, BANK_FOAM_RANGE, range));
    let steep = smoothstep(river.foam.x, river.foam.y, in.flow.w);
    let fall = saturate(1.0 - abs(in.body.x - 2.0));
    let rapids = smoothstep(0.0, 0.25, noise + steep - 0.75) * steep;
    // Foam is a near thing: a creek's rapids and falls are a metre or two
    // wide, and from afar the white of a coarse level's wide groove reads as
    // a sheet standing on the hillside.
    let foam = saturate(max(max(bank_foam, rapids), fall * (0.7 + 0.3 * smoothstep(0.3, 0.7, noise))))
        * water.optics.w * (1.0 - smoothstep(FOAM_RANGE * 0.5, FOAM_RANGE, range));

    var color = water_light(pbr_input, p, N, roughness, behind.x, river.look.x, foam);
    // Down at the sea's level the sea draws the water: the river is
    // discarded there, so its depth does not hide the sea. At the
    // waterline the surface fades in over a few centimetres. Premultiplied,
    // so the whole colour scales.
    let sea_fade = smoothstep(0.0, 0.03, in.body.y);
    // A creek faded out as its vertices morph to a level that does not
    // draw it writes no depth either.
    let creek = in.body.z;
    if sea_fade <= 0.0 || creek <= 0.02 {
        discard;
    }
    color *= sea_fade * creek * smoothstep(-0.005, 0.03, depth);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
