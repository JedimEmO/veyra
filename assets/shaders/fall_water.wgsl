// Falls for `strata_terrain::falls::FallMaterial` (ticket 0005, step 8):
// the sheet down a fall, the foam at its lip and its foot, and the spray.
//
// - The sheet's coordinate down the fall is the water's time of travel, so
//   streaks scrolled by the clock speed up as the water falls. Over the lip
//   the water is a clear tongue for its first fraction of a second, then
//   breaks white.
// - A far sheet is widened across to a least width in pixels and its
//   opacity lowered by the share it was widened, never under a floor, so a
//   fall is a white mark on its hillside to the end of its range.
// - Past 150 m a sheet's vertex is drawn nearer the camera along its own
//   ray by a share of its distance, which moves nothing on the screen and
//   keeps the sheet in front of the coarse far ground over its channel.
//   The foam is meant to sink under the banks and is not pulled.
// - Foam churns outward from the foot and gathers toward the lip; the
//   spray's puffs rise, grow and fade on their own cycles, facing the
//   camera.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

// Mirrors strata_terrain::falls::FallParams field for field.
struct FallParams {
    // Aerated water's colour in rgb; the tongue's darkness against it in w.
    white: vec4<f32>,
    // The fade's start and end, metres, in xy; the least width, pixels, in
    // z; the pull toward the camera as a share of the distance in w.
    range: vec4<f32>,
    // A widened sheet's least opacity in x, the spray's in y, the foam's
    // in z.
    far: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> fall: FallParams;

// Mirrors strata_terrain::falls::PART_*.
const PART_FOAM: f32 = 1.0;
const PART_SPRAY: f32 = 2.0;

// Past this the pull starts, metres.
const PULL_FROM: f32 = 150.0;

// Mirrors strata_terrain::falls::LIP_OVER_M: the lip's foam runs on this
// far past the lip, fading out under the sheet.
const LIP_OVER_M: f32 = 1.2;

// Mirrors strata_terrain::falls::FOOT_UPSTREAM: the foot's foam reaches
// this share of its reach up the ramp.
const FOOT_UPSTREAM: f32 = 0.3;

// The sheet fades in over its first this many seconds of travel, out of
// the lip's foam.
const LIP_FADE_S: f32 = 0.35;

struct FallVertex {
    @builtin(instance_index) instance_index: u32,
    // Metres from the lip; a puff's centre.
    @location(0) position: vec3<f32>,
    // A puff's holds the way downstream.
    @location(1) normal: vec3<f32>,
    // The part, then the sheet's across axis and half width, or a puff's
    // corner and size.
    @location(2) shape: vec4<f32>,
    // By part: the sheet's across, travel, down and drop; the foam's
    // across, metres, reach and end; a puff's seed, cycle, rise and drift.
    @location(3) flow: vec4<f32>,
}

struct FallOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) flow: vec4<f32>,
    // The part in x; the opacity the vertex stage leaves (the widening, the
    // fade, a puff's cycle) in y; a puff's corner in zw.
    @location(3) look: vec4<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
}

@vertex
fn vertex(v: FallVertex) -> FallOutput {
    var out: FallOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    var normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
    let part = v.shape.x;
    let distance = max(length(view.world_position - world), 1e-3);
    // Metres one pixel covers at this distance.
    let pixel = 2.0 * distance / (view.clip_from_view[1][1] * view.viewport.w);
    var opacity = 1.0;
    var corner = vec2<f32>(0.0);
    if part < PART_FOAM - 0.5 {
        // The sheet: widened across to the least width, its opacity by the
        // share it was widened.
        let across = vec3<f32>(v.shape.y, 0.0, v.shape.z);
        let half = max(v.shape.w, 1e-3);
        let widen = max(1.0, 0.5 * fall.range.z * pixel / half);
        world += across * (v.flow.x * half * (widen - 1.0));
        opacity = max(1.0 / widen, fall.far.x);
    } else if part > PART_SPRAY - 0.5 {
        // A puff: rises, drifts downstream, grows and fades over its cycle.
        let phase = fract(globals.time / v.flow.y + v.flow.x);
        let size = v.shape.w * (0.45 + 0.9 * phase);
        let downstream = normalize(vec3<f32>(normal.x, 0.0, normal.z) + vec3<f32>(1e-4, 0.0, 0.0));
        let centre = world + vec3<f32>(0.0, v.flow.z * phase, 0.0) + downstream * (v.flow.w * phase);
        let to_camera = normalize(view.world_position - centre);
        let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), to_camera) + vec3<f32>(1e-4, 0.0, 0.0));
        let up = cross(to_camera, right);
        corner = v.shape.yz;
        world = centre + (right * corner.x + up * corner.y) * size;
        // Lit as mist under the sky, not as a card turned to the camera,
        // which took the grass's green from below and read as a teal haze.
        normal = vec3<f32>(0.0, 1.0, 0.0);
        // A camera inside the spray sees through the puffs nearest it.
        opacity = sin(3.14159265 * phase) * smoothstep(0.4, 1.4, length(view.world_position - centre) / size);
    }
    let fade = 1.0 - smoothstep(fall.range.x, fall.range.y, distance);
    var pull = 0.0;
    if part < PART_FOAM - 0.5 {
        pull = min(fall.range.w * max(distance - PULL_FROM, 0.0), 0.3 * distance);
    }
    let drawn = world + normalize(view.world_position - world) * pull;
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(drawn);
    out.world_normal = normal;
    out.flow = v.flow;
    out.look = vec4<f32>(part, opacity * fade, corner);
    out.instance_index = v.instance_index;
    return out;
}

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// Value noise, 0..1.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return 0.55 * noise(p) + 0.3 * noise(p * 2.03 + vec2<f32>(5.2, 1.3)) + 0.15 * noise(p * 4.11 + vec2<f32>(1.7, 9.2));
}

@fragment
fn fragment(
    in: FallOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var std_in: VertexOutput;
    std_in.position = in.position;
    std_in.world_position = in.world_position;
    std_in.world_normal = in.world_normal;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    std_in.instance_index = in.instance_index;
#endif
    var pbr_input = pbr_input_from_standard_material(std_in, is_front);
    let p = in.world_position.xyz;
    let distance = length(view.world_position - p);
    // Detail fades with distance, toward each pattern's mean.
    let detail = 1.0 - smoothstep(250.0, 1500.0, distance);
    let time = globals.time;
    let part = in.look.x;
    var white = 1.0;
    var alpha = 0.0;
    var roughness = 0.5;
    if part < PART_FOAM - 0.5 {
        let across = in.flow.x;
        let travel = in.flow.y;
        let down = in.flow.z;
        let drop = in.flow.w;
        // Streaks: fine across, long down, carried at the water's speed.
        let flowing = travel - time;
        let streak = fbm(vec2<f32>(across * 3.5, flowing * 1.6));
        let fine = fbm(vec2<f32>(across * 11.0, flowing * 6.0));
        let breaks = smoothstep(0.08, 0.45, travel);
        let shade = mix(0.85, mix(0.5, 1.08, streak) * mix(0.78, 1.06, fine), detail);
        white = mix(fall.white.w, 1.0, breaks) * shade;
        // Ragged edges that wander down the fall with the water, each side
        // on its own: an edge set by the streaks alone ran ruler-straight.
        let rag = fbm(vec2<f32>(sign(across) * 7.3 + across * 2.0, flowing * 2.4));
        let inner = 0.5 + 0.3 * mix(0.5, rag, detail);
        let edge = 1.0 - smoothstep(inner, min(inner + 0.25, 1.0), abs(across));
        // Clear runs where the water is thin, opaque where it broke.
        let body = mix(0.97, mix(0.6, 1.0, smoothstep(0.3, 0.65, 0.6 * streak + 0.4 * fine)), detail);
        alpha = mix(0.85, 1.0, breaks) * edge * body;
        // Over the lip it rises out of the gathering foam, not from a line.
        alpha *= mix(0.3, 1.0, smoothstep(0.0, LIP_FADE_S, travel));
        // Where it lands the sheet thins into the churn.
        alpha *= 1.0 - 0.25 * smoothstep(0.9, 1.0, down) * step(4.0, drop);
        roughness = mix(0.2, 0.8, breaks);
    } else if part < PART_SPRAY - 0.5 {
        let across = in.flow.x;
        let metres = in.flow.y;
        let reach = max(in.flow.z, 1e-3);
        if in.flow.w > 0.5 {
            // Round the foot: churn carried outward from where it lands,
            // to nothing at both ends of the patch, the upstream one nearer
            // (the noise takes up to 0.15 off the radius, so it ends at
            // 0.85 of it).
            let along = select(metres / reach, metres / (FOOT_UPSTREAM * reach), metres < 0.0);
            let r = length(vec2<f32>(along, across));
            let churn = fbm(vec2<f32>(across * 4.0, (metres - time * 1.8) * 0.5));
            let lumps = fbm(vec2<f32>(across * 9.0 + 3.0, (metres - time * 1.2) * 1.4));
            let body = (1.0 - smoothstep(0.25, 0.85, r + 0.3 * (churn - 0.5)))
                * (1.0 - smoothstep(0.45 + 0.3 * churn, 1.0, abs(across)));
            alpha = body * mix(0.7, smoothstep(0.25, 0.65, 0.6 * churn + 0.4 * lumps + 0.3 * body), detail);
            white = mix(0.8, 1.0, lumps);
        } else {
            // Above the lip: the river whitening as it gathers over the
            // edge, then fading under the sheet's first metre.
            let gather = smoothstep(-reach, 0.0, metres) * (1.0 - smoothstep(0.0, LIP_OVER_M, metres));
            let churn = fbm(vec2<f32>(across * 5.0, (metres - time * 2.5) * 0.7));
            let side = 1.0 - smoothstep(0.2 + 0.35 * churn, 0.9, abs(across));
            alpha = gather * gather * side * mix(0.6, smoothstep(0.3, 0.7, churn + 0.3 * gather), detail);
            white = mix(0.75, 1.0, churn);
        }
        alpha *= fall.far.z;
        roughness = 0.7;
    } else {
        // A puff: soft, lumpy, its own noise drifting.
        let r = length(in.look.zw);
        let wisp = fbm(in.look.zw * 1.7 + vec2<f32>(in.flow.x * 17.0, time * 0.3));
        // To nothing inside the quad's edge whatever the noise (which
        // takes up to 0.175 off the radius): cut by the quad, a puff drew
        // its outline.
        alpha = (1.0 - smoothstep(0.1, 0.8, r + 0.35 * (wisp - 0.5))) * fall.far.y;
        white = 0.95;
        roughness = 1.0;
    }
    alpha *= in.look.y;
    // The body writes its depth, so its faintest fringe is discarded rather
    // than hiding what is drawn behind it later. The spray writes none, and
    // a step from a few hundredths to nothing drew each puff's outline.
    if alpha < select(0.02, 0.002, part > PART_SPRAY - 0.5) {
        discard;
    }
    pbr_input.material.base_color = vec4<f32>(fall.white.rgb * white, alpha);
    pbr_input.material.perceptual_roughness = roughness;
    var n = normalize(in.world_normal);
    if !is_front && part < PART_SPRAY - 0.5 {
        n = -n;
    }
    pbr_input.N = n;
    pbr_input.world_normal = n;
    var color = apply_pbr_lighting(pbr_input);
    color.a = alpha;
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
