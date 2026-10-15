// The wind, shared by every foliage shader: a global direction and
// strength, a travelling gust field sampled in world coordinates (the
// render origin's wrapped offset is added so a rebase does not move the
// gusts), and the bend of one vertex about its root.

#define_import_path strata::foliage_wind

// Mirrors strata_foliage::grass::WindParams field for field.
struct FoliageWind {
    direction: vec2<f32>,
    strength: f32,
    time: f32,
    prev_time: f32,
    origin_x: f32,
    origin_z: f32,
    gust_scale: f32,
    prev_origin_x: f32,
    prev_origin_z: f32,
    // How much the placer's sky term darkens the sky's fill, 0 to 1
    // (lighting plan phase 3a).
    sky_visibility: f32,
    // How much sunlight comes through a blade of grass seen against the
    // sun and rims its edges, 0 to 2 (lighting plan phase 5b).
    grass_rim: f32,
}

fn hash_cell(p: vec2<i32>) -> f32 {
    var h = bitcast<u32>(p.x) * 0x8da6b343u ^ bitcast<u32>(p.y) * 0xd8163841u;
    h = h ^ (h >> 13u);
    h = h * 0x27d4eb2du;
    h = h ^ (h >> 15u);
    return f32(h & 0xffffu) / 65535.0;
}

fn periodic_cell(p: vec2<i32>, period: i32) -> vec2<i32> {
    return ((p % vec2<i32>(period)) + vec2<i32>(period)) % vec2<i32>(period);
}

fn vnoise(p: vec2<f32>, period: i32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash_cell(periodic_cell(i, period));
    let b = hash_cell(periodic_cell(i + vec2<i32>(1, 0), period));
    let c = hash_cell(periodic_cell(i + vec2<i32>(0, 1), period));
    let d = hash_cell(periodic_cell(i + vec2<i32>(1, 1), period));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The gust at a world position, in [0, 1]: two octaves scrolling along
// the wind.
fn gust(world_xz: vec2<f32>, wind: FoliageWind, time: f32) -> f32 {
    // The CPU wraps origin at 4096 metres. Both lattice periods must
    // divide that physical interval, including the high-frequency octave,
    // or a rebase changes the gust even at a stationary tree.
    let cells = max(1.0, round(4096.0 / max(wind.gust_scale, 1.0)));
    let detail_cells = max(1.0, round(cells * 2.7));
    let travel = wind.direction * time * 0.35;
    let p = world_xz * (cells / 4096.0) - travel;
    let detail = world_xz * (detail_cells / 4096.0) - travel * (detail_cells / cells) + vec2<f32>(11.3, 7.1);
    return 0.65 * vnoise(p, i32(cells)) + 0.35 * vnoise(detail, i32(detail_cells));
}

// Where a point `d` above its root (both in the same frame) moves to at
// `time`: a lean along the wind growing with the square of the height
// fraction `v`, a per-plant sway from `phase`, and the length kept about
// the root (the Crysis renormalisation), so a strong gust leans the plant
// instead of stretching it.
fn bend(
    d: vec3<f32>,
    v: f32,
    height: f32,
    phase: f32,
    stiffness: f32,
    gust_here: f32,
    wind: FoliageWind,
    time: f32,
) -> vec3<f32> {
    let strength = wind.strength * (0.3 + 1.4 * gust_here) * (1.0 - stiffness);
    let sway = sin(time * 1.975 + phase * 6.2832) * 0.06
             + sin(time * 0.793 + phase * 4.1) * 0.05
             + sin(time * 0.375 + phase * 2.3) * 0.04;
    let amp = (strength + sway * wind.strength * (1.0 - stiffness)) * v * v * height;
    let side = vec2<f32>(-wind.direction.y, wind.direction.x);
    let offset = wind.direction * amp + side * amp * 0.25 * sin(time * 1.3 + phase * 9.0);
    let len = length(d);
    var moved = d + vec3<f32>(offset.x, 0.0, offset.y);
    if len > 1e-4 {
        moved = normalize(moved) * len;
    }
    return moved;
}

// Full elapsed spawn time is stored per grass material. Saturation is
// permanent: long-lived cells never replay growth after a packed-clock wrap.
// Previous-frame time before birth naturally clamps to zero.
fn grow_in(spawn: f32, now: f32) -> f32 {
    return smoothstep(0.0, 1.0, now - spawn);
}
