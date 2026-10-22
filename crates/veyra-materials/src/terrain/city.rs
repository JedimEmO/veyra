//! Port Serein's ground: stone setts for the terrace lanes, flagstones for
//! the quay apron and the wide terraces, and tilled soil for the gardens.
//!
//! The two pavings are laid, not scattered, so they are drawn on courses
//! rather than on Voronoi cells: rows of whole stones down v, each row
//! shifted along u by its own amount and every joint in it moved a little,
//! so the stones are of uneven length the way a paviour's are.
//!
//! A laid pattern only reads as one if it is read straight. The terrain
//! shader's stochastic tiling turns and shifts every read, which suits the
//! scattered layers and would turn a course of setts on its end, so these
//! two want a plain read; soil is scattered and takes the stochastic one.
//! Their heights follow the other layers' meaning: the stone faces stand
//! high and the joints lie with the sand and the snow, so where a paving
//! meets gravel or soil in the terrain's height blend the loose ground
//! settles into the joints first and the stones stay proud.
use ashlar_material::{
    Channel, Input, MaterialGraph, MaterialGraphBuilder, MathOp, PbrOutput,
    nodes::{Blur, Clamp, Colorize, Decompose, Levels, Math, Mix, Noise, Scratches, Uv, Warp},
};

use super::{cells, finish, graph_key, hash, m, ramp, remap, repeat, stack, wobble};

/// A number in `0..=1` off a whole-number index, for a row, a joint or a
/// stone, which has no Voronoi id of its own. Hoskins' hash without sine: a
/// sine of an index in the hundreds loses its low bits in `f32`, and what is
/// left of them runs in diagonal bands across the courses.
pub(super) fn index_hash(index: impl Into<Input>, salt: f32) -> Math {
    use MathOp::{Add, Fract, Mul};
    let p = Math::unary(Fract, m(Add, m(Mul, index, 0.1031), salt));
    let p = m(Mul, p.clone(), m(Add, p, 33.33));
    Math::unary(Fract, m(Mul, p.clone(), m(Add, p.clone(), p)))
}

/// A whole number folded into `0..n`, exactly: the half before the floor
/// absorbs the rounding of the division, so the same stone reached from
/// either side of the seam hashes the same.
pub(super) fn fold(index: impl Into<Input>, count: f32) -> Math {
    use MathOp::{Add, Div, Floor, Fract, Mul};
    Math::unary(
        Floor,
        m(
            Add,
            m(Mul, Math::unary(Fract, m(Div, index, count)), count),
            0.5,
        ),
    )
}

/// Stones laid in `rows` courses down v with `stones` to a course on
/// average, read through the `wobble` displacement so no joint is ruled.
///
/// Every course is shifted along u by its own random fraction of a stone,
/// and every joint in it moved along by up to `jitter` of a stone either way,
/// so the stones in a course are of uneven length the way a paviour's are.
/// A texel works out which stone owns it from the four joints about it: the
/// two of its own slot, and the ones beyond them for when a neighbour's joint
/// has moved into the slot. Leaves `{prefix}_row` and `_col` (the stone's
/// indices), `_id` (a float per stone), `_lu`, `_lv` (the position inside
/// the stone, `0..1`), and `_dx`, `_dy`, the distance to the stone's nearest
/// end and side in metres.
fn courses(
    g: MaterialGraphBuilder,
    prefix: &str,
    [rows, stones]: [u16; 2],
    jitter: f32,
    tile: f32,
    wobble: (&str, f32),
) -> MaterialGraphBuilder {
    use MathOp::{Add, Div, Floor, Fract, Min, Mul, Step, Sub};
    let n = |s: &str| format!("{prefix}_{s}");
    let (rows, stones) = (f32::from(rows), f32::from(stones));
    let (offset, amount) = wobble;
    let joint = |slot: &str| {
        m(
            Mul,
            m(
                Sub,
                index_hash(m(Add, fold(slot, stones), m(Mul, n("row"), stones)), 0.53),
                0.5,
            ),
            2.0 * jitter,
        )
    };
    let pick = |mid: &str, back: &str, fwd: &str| {
        m(
            Add,
            mid,
            m(
                Add,
                m(Mul, n("back"), m(Sub, back, mid)),
                m(Mul, n("fwd"), m(Sub, fwd, mid)),
            ),
        )
    };
    g.node(
        n("u"),
        m(
            Add,
            Decompose::new(Uv::new(), Channel::R),
            m(Mul, Decompose::new(offset, Channel::R), amount),
        ),
    )
    .node(
        n("v"),
        m(
            Add,
            Decompose::new(Uv::new(), Channel::G),
            m(Mul, Decompose::new(offset, Channel::G), amount),
        ),
    )
    .node(n("course"), m(Mul, n("v"), rows))
    .node(n("row"), fold(Math::unary(Floor, n("course")), rows))
    .node(
        n("across"),
        m(Add, m(Mul, n("u"), stones), index_hash(n("row"), 0.71)),
    )
    .node(n("slot"), Math::unary(Floor, n("across")))
    .node(n("t"), m(Sub, n("across"), n("slot")))
    .node(n("slot_prev"), m(Sub, n("slot"), 1.0))
    .node(n("slot_next"), m(Add, n("slot"), 1.0))
    .node(n("slot_after"), m(Add, n("slot"), 2.0))
    .node(n("j_prev"), joint(&n("slot_prev")))
    .node(n("j"), joint(&n("slot")))
    .node(n("j_next"), joint(&n("slot_next")))
    .node(n("j_after"), joint(&n("slot_after")))
    // Whether the joint at the slot's start has moved past the texel, which
    // then belongs to the stone before, or the one at its end has moved short
    // of it, which then belongs to the stone after.
    .node(n("back"), m(Step, n("j"), n("t")))
    .node(n("fwd"), m(Step, n("t"), m(Add, 1.0, n("j_next"))))
    .node(n("mid_end"), m(Add, 1.0, n("j_next")))
    .node(n("back_start"), m(Sub, n("j_prev"), 1.0))
    .node(n("fwd_end"), m(Add, 2.0, n("j_after")))
    .node(n("start"), pick(&n("j"), &n("back_start"), &n("mid_end")))
    .node(n("end"), pick(&n("mid_end"), &n("j"), &n("fwd_end")))
    .node(
        n("col"),
        fold(m(Add, n("slot"), m(Sub, n("fwd"), n("back"))), stones),
    )
    .node(
        n("id"),
        index_hash(m(Add, n("col"), m(Mul, n("row"), stones)), 0.391),
    )
    .node(
        n("lu"),
        m(
            Div,
            m(Sub, n("t"), n("start")),
            m(Sub, n("end"), n("start")),
        ),
    )
    .node(n("lv"), Math::unary(Fract, n("course")))
    .node(
        n("dx"),
        m(
            Mul,
            m(Min, m(Sub, n("t"), n("start")), m(Sub, n("end"), n("t"))),
            tile / stones,
        ),
    )
    .node(
        n("dy"),
        m(Mul, m(Min, n("lv"), m(Sub, 1.0, n("lv"))), tile / rows),
    )
}

/// How far inside a stone's outline a texel lies, in metres: negative in
/// the joint. The stone's ends are taken in by `inset[0]` and its sides by
/// `inset[1]`, so the joints along a course and across it can differ, and
/// its corners are rounded by `radius`.
fn outline(
    g: MaterialGraphBuilder,
    prefix: &str,
    radius: &str,
    inset: [&str; 2],
) -> MaterialGraphBuilder {
    use MathOp::{Add, Max, Min, Mul, Sqrt, Sub};
    let n = |s: &str| format!("{prefix}_{s}");
    g.node(n("ex"), m(Sub, n("dx"), inset[0]))
        .node(n("ey"), m(Sub, n("dy"), inset[1]))
        .node(n("corner_x"), m(Max, m(Sub, radius, n("ex")), 0.0))
        .node(n("corner_y"), m(Max, m(Sub, radius, n("ey")), 0.0))
        .node(
            n("inside"),
            m(
                Add,
                m(
                    Sub,
                    radius,
                    Math::unary(
                        Sqrt,
                        m(
                            Add,
                            m(Mul, n("corner_x"), n("corner_x")),
                            m(Mul, n("corner_y"), n("corner_y")),
                        ),
                    ),
                ),
                // Away from a corner this restores the plain distance to the
                // nearest side, which a stone narrower than its two radii
                // has nowhere.
                m(Max, m(Sub, m(Min, n("ex"), n("ey")), radius), 0.0),
            ),
        )
}

/// Stone setts for the terrace lanes: small blocks about 20 cm by 10 cm in
/// running courses, of uneven length, with worn rounded tops and dark gritty
/// joints that turn to moss in the damp. Granite and basalt in muted warm
/// greys, each stone its own value. 30 mm of relief over a 4 m repeat, the
/// joints deep enough that a neighbouring layer's gravel fills them.
///
/// Forty courses of ten centimetres and twenty stones to a course: a sett
/// is some 25 by 50 texels at the bake, enough for its shoulder and its joint
/// to survive the terrain's filtering down to a few metres away.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn setts() -> MaterialGraph {
    use MathOp::{Add, Max, Mul, Sub};
    let tile = repeat("setts");
    let g = MaterialGraph::builder(graph_key("setts"))
        .node("tone", Noise::perlin().period(4).octaves(3).seed(1001))
        .node("grit", Noise::perlin().period(64).octaves(2).seed(1003))
        .node(
            "moss_field",
            Noise::perlin().period(8).octaves(2).seed(1005),
        )
        .node("chip", Noise::perlin().period(128).octaves(2).seed(1007))
        .node("wear", Noise::perlin().period(16).octaves(2).seed(1009))
        .node("grain", Noise::perlin().period(256).octaves(1).seed(1013));
    let g = wobble(g, "wobble", 8, 1011);
    let g = courses(g, "sett", [40, 20], 0.22, tile, ("wobble", 0.002))
        .node("radius", remap(hash("sett_id", 23.7), 0.012, 0.032))
        .node(
            "inset_end",
            m(
                Add,
                remap(hash("sett_id", 7.13), 0.004, 0.008),
                m(Mul, "chip", 0.006),
            ),
        )
        .node(
            "inset_side",
            m(
                Add,
                remap(hash("sett_id", 5.37), 0.003, 0.007),
                m(Mul, "chip", 0.006),
            ),
        );
    let g = outline(g, "sett", "radius", ["inset_end", "inset_side"])
        .node(
            "shoulder",
            Levels::new("sett_inside").in_range(0.0, 0.03).gamma(0.55),
        )
        .node("face", ramp("sett_inside", 0.0, 0.0015))
        // Each sett has settled its own way: tipped a little along and
        // across, and sunk or proud of its neighbours by a few millimetres.
        .node(
            "settle",
            m(
                Add,
                m(
                    Add,
                    m(
                        Mul,
                        remap(hash("sett_id", 41.3), -0.08, 0.08),
                        m(Sub, "sett_lu", 0.5),
                    ),
                    m(
                        Mul,
                        remap(hash("sett_id", 57.9), -0.1, 0.1),
                        m(Sub, "sett_lv", 0.5),
                    ),
                ),
                remap(hash("sett_id", 71.1), -0.09, 0.05),
            ),
        )
        // A sett's top is domed, highest in its middle.
        .node(
            "crown",
            m(
                Mul,
                m(
                    Sub,
                    1.0,
                    m(
                        Mul,
                        m(Sub, m(Mul, "sett_lu", 2.0), 1.0),
                        m(Sub, m(Mul, "sett_lu", 2.0), 1.0),
                    ),
                ),
                m(
                    Sub,
                    1.0,
                    m(
                        Mul,
                        m(Sub, m(Mul, "sett_lv", 2.0), 1.0),
                        m(Sub, m(Mul, "sett_lv", 2.0), 1.0),
                    ),
                ),
            ),
        )
        .node(
            "top",
            m(
                Add,
                m(Add, 0.7, "settle"),
                m(Add, m(Mul, "crown", 0.07), remap("tone", -0.03, 0.03)),
            ),
        )
        .node(
            "stone_height",
            m(Mul, "face", Mix::new(0.22, "top", "shoulder")),
        )
        .node("moss", ramp("moss_field", 0.58, 0.7))
        .node(
            "bed",
            m(
                Add,
                m(Add, 0.12, m(Mul, "grit", 0.07)),
                m(Mul, "moss", 0.05),
            ),
        )
        .node("height", Clamp::new(m(Max, "stone_height", "bed")))
        .node(
            "stone",
            m(
                Mul,
                Colorize::new(hash("sett_id", 13.37)).gradient([
                    (0.0, [0.085, 0.085, 0.088]),
                    (0.2, [0.125, 0.120, 0.115]),
                    (0.45, [0.175, 0.160, 0.142]),
                    (0.65, [0.200, 0.180, 0.160]),
                    (0.8, [0.225, 0.190, 0.168]),
                    (1.0, [0.255, 0.248, 0.235]),
                ]),
                m(
                    Mul,
                    remap("shoulder", 0.72, 1.08),
                    remap("grain", 0.95, 1.05),
                ),
            ),
        )
        // Polished where the feet go: the crowns of the stones in the worn
        // stretches are a touch lighter and smoother.
        .node(
            "polish",
            m(Mul, ramp("shoulder", 0.7, 1.0), ramp("wear", 0.45, 0.75)),
        )
        .node(
            "joint_color",
            Mix::new(
                m(Mul, [0.070, 0.061, 0.048], remap("grit", 0.8, 1.2)),
                [0.042, 0.066, 0.024],
                m(Mul, "moss", 0.85),
            ),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    Mix::new(
                        "stone",
                        "joint_color",
                        m(Mul, ramp("shoulder", 0.3, 0.0), 0.5),
                    ),
                    "joint_color",
                    m(Sub, 1.0, "face"),
                ),
                m(Mul, remap("tone", 0.92, 1.08), remap("polish", 1.0, 1.1)),
            ),
        )
        .node(
            "roughness",
            Mix::new(
                Mix::new(remap(hash("sett_id", 3.1), 0.68, 0.78), 0.52, "polish"),
                0.95,
                m(Sub, 1.0, "face"),
            ),
        );
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.03,
        0.004,
    )
}

/// The flagstones' courses and stones to a course, and how far a joint moves
/// as a share of a stone: the layout `assets/terrain/materials.ron` repeats
/// for the terrain shader (`laid`), so it can tell one flag from the next.
pub const FLAGSTONES_LAYOUT: ([u16; 2], f32) = ([13, 9], 0.2);

/// Flagstones for the quay apron and the wide terraces: pale cast flags
/// about 90 cm by 60 cm in random courses, narrow dark joints, each slab
/// settled a few millimetres its own way, a few cracked through, with damp
/// stains and dirt gathered at the joints. 25 mm of relief over an 8 m
/// repeat: the slabs are flat and it is their tilt and their arrises that
/// catch a low sun.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn flagstones() -> MaterialGraph {
    use MathOp::{Add, Max, Mul, Step, Sub};
    let tile = repeat("flagstones");
    let g = MaterialGraph::builder(graph_key("flagstones"))
        .node("tone", Noise::perlin().period(8).octaves(3).seed(1101))
        .node("swell", Noise::perlin().period(16).octaves(2).seed(1103))
        .node("chip", Noise::perlin().period(256).octaves(1).seed(1105))
        .node(
            "stain_field",
            Noise::perlin().period(8).octaves(3).seed(1107),
        )
        .node(
            "bleach_field",
            Noise::perlin().period(16).octaves(2).seed(1109),
        )
        .node(
            "crack_gate",
            Noise::perlin().period(8).octaves(2).seed(1111),
        )
        .node("grit", Noise::perlin().period(128).octaves(2).seed(1113))
        .node("aggregate", Noise::value().period(256).seed(1115))
        .node(
            "weed_field",
            Noise::perlin().period(16).octaves(2).seed(1117),
        );
    let g = wobble(g, "wobble", 16, 1121);
    let g = courses(
        g,
        "flag",
        FLAGSTONES_LAYOUT.0,
        FLAGSTONES_LAYOUT.1,
        tile,
        ("wobble", 0.0006),
    )
    .node("radius", remap(hash("flag_id", 23.7), 0.004, 0.015))
    .node(
        "inset",
        m(
            Add,
            remap(hash("flag_id", 7.13), 0.005, 0.008),
            m(Mul, "chip", 0.004),
        ),
    );
    let g = outline(g, "flag", "radius", ["inset", "inset"])
        .node(
            "arris",
            Levels::new("flag_inside").in_range(0.0, 0.014).gamma(0.5),
        )
        .node("face", ramp("flag_inside", 0.0, 0.0025))
        .node(
            "settle",
            m(
                Add,
                m(
                    Add,
                    m(
                        Mul,
                        remap(hash("flag_id", 41.3), -0.06, 0.06),
                        m(Sub, "flag_lu", 0.5),
                    ),
                    m(
                        Mul,
                        remap(hash("flag_id", 57.9), -0.06, 0.06),
                        m(Sub, "flag_lv", 0.5),
                    ),
                ),
                remap(hash("flag_id", 71.1), -0.06, 0.03),
            ),
        );
    // Cracks run across some of the slabs, about one in three, along the edges of a
    // coarser cell lattice so each is a bent line rather than a straight cut.
    let g = cells(g, "shard", [16, 16], 1123, 1.0, Some(("wobble", 0.008)))
        .node(
            "crack",
            m(
                Mul,
                m(
                    Mul,
                    ramp("shard_edge", 0.012, 0.0),
                    ramp("crack_gate", 0.5, 0.62),
                ),
                m(
                    Mul,
                    m(Step, hash("flag_id", 91.3), 0.7),
                    ramp("flag_inside", 0.01, 0.03),
                ),
            ),
        )
        .node(
            "top",
            m(
                Sub,
                m(Add, m(Add, 0.68, "settle"), remap("swell", -0.03, 0.03)),
                m(Mul, "crack", 0.08),
            ),
        )
        .node("slab_height", m(Mul, "face", Mix::new(0.3, "top", "arris")))
        .node("weeds", ramp("weed_field", 0.62, 0.72))
        .node(
            "bed",
            m(
                Add,
                m(Add, 0.14, m(Mul, "grit", 0.08)),
                m(Mul, "weeds", 0.06),
            ),
        )
        .node("height", Clamp::new(m(Max, "slab_height", "bed")))
        .node(
            "slab",
            m(
                Mul,
                Colorize::new(hash("flag_id", 13.37)).gradient([
                    (0.0, [0.255, 0.250, 0.240]),
                    (0.35, [0.300, 0.290, 0.270]),
                    (0.65, [0.330, 0.305, 0.270]),
                    (0.85, [0.350, 0.340, 0.320]),
                    (1.0, [0.300, 0.300, 0.295]),
                ]),
                remap("aggregate", 0.95, 1.05),
            ),
        )
        .node(
            "weather",
            m(
                Mul,
                m(
                    Mul,
                    // Damp stains, darker and a touch warm, and paler salt
                    // bloom where the sun bakes the slab dry.
                    Mix::new(
                        [1.0, 1.0, 1.0],
                        [0.78, 0.76, 0.72],
                        ramp("stain_field", 0.5, 0.75),
                    ),
                    remap(ramp("bleach_field", 0.55, 0.8), 1.0, 1.1),
                ),
                m(
                    Mul,
                    remap(ramp("flag_inside", 0.04, 0.0), 1.0, 0.8),
                    remap("arris", 0.8, 1.0),
                ),
            ),
        )
        .node(
            "joint_color",
            Mix::new(
                m(Mul, [0.080, 0.072, 0.058], remap("grit", 0.8, 1.2)),
                [0.050, 0.075, 0.028],
                m(Mul, "weeds", 0.8),
            ),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    Mix::new(
                        m(Mul, "slab", "weather"),
                        [0.06, 0.055, 0.05],
                        m(Mul, "crack", 0.8),
                    ),
                    "joint_color",
                    m(Sub, 1.0, "face"),
                ),
                remap("tone", 0.94, 1.06),
            ),
        )
        .node(
            "roughness",
            Mix::new(
                m(
                    Add,
                    remap(hash("flag_id", 3.1), 0.74, 0.84),
                    m(Mul, ramp("stain_field", 0.5, 0.75), -0.08),
                ),
                0.95,
                m(Sub, 1.0, "face"),
            ),
        );
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.025,
        0.004,
    )
}

/// Tilled garden soil: dark crumbled earth turned into clods of two sizes,
/// a scatter of small stones, and a few bits of dry stalk and husk. 50 mm of
/// relief over a 6 m repeat, the clods standing below a paving's faces and
/// above its joints, so soil at a bed's edge settles into the setts' joints
/// and never climbs over the stones.
///
/// A clod is a squashed, flat-topped lump about its Voronoi point, drawn like
/// the gravel's pebbles but with a ragged outline and a dry, paler crown,
/// which is what reads as turned earth rather than as mud.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn soil() -> MaterialGraph {
    use MathOp::{Add, Div, Max, Min, Mul, Sqrt, Step, Sub};
    let tile = repeat("soil");
    let g = MaterialGraph::builder(graph_key("soil"))
        .node("mound", Noise::perlin().period(8).octaves(2).seed(1201))
        .node("tone", Noise::perlin().period(4).octaves(3).seed(1203))
        .node("damp", Noise::perlin().period(8).octaves(2).seed(1205))
        .node("crumb", Noise::perlin().period(128).octaves(2).seed(1207))
        .node("rag", Noise::perlin().period(128).octaves(2).seed(1209))
        .node(
            "cluster",
            remap(Noise::perlin().period(8).octaves(2).seed(1211), 0.6, 1.3),
        )
        .node("bed_rough", Clamp::new(0.96));
    let g = wobble(g, "wobble", 32, 1213);
    // Spadefuls: the soft lumps the earth was turned in, blurred so they
    // meet in hollows rather than creases, which is most of what makes the
    // bed read as dug rather than as a gravel of crumbs.
    let g = cells(g, "turn", [32, 32], 1215, 1.0, Some(("wobble", 0.012)))
        .node(
            "spadeful",
            Blur::new(Levels::new("turn_dist").in_range(0.95, 0.0).gamma(0.6)).radius(0.008),
        )
        .node(
            "bed",
            m(
                Add,
                m(
                    Add,
                    m(Add, 0.1, m(Mul, "mound", 0.06)),
                    m(Mul, "spadeful", remap(hash("turn_id", 23.7), 0.18, 0.3)),
                ),
                m(Mul, "crumb", 0.04),
            ),
        )
        // Damp and dark in the hollows, drying paler on the crowns.
        .node(
            "bed_color",
            m(
                Mul,
                Mix::new(
                    [0.030, 0.022, 0.015],
                    [0.088, 0.067, 0.047],
                    Levels::new("spadeful").gamma(1.4),
                ),
                m(Mul, remap("mound", 0.9, 1.1), remap("crumb", 0.85, 1.15)),
            ),
        );
    let mut g = g;
    // (prefix, period, seed, presence, rise, a clod rather than a stone)
    let lumps: [(&str, u16, u32, f32, f32, bool); 4] = [
        ("clod_large", 64, 1221, 0.3, 0.22, true),
        ("clod", 128, 1223, 0.4, 0.18, true),
        ("clod_small", 128, 1227, 0.3, 0.14, true),
        ("pebble", 64, 1229, 0.1, 0.3, false),
    ];
    let mut layers = vec![(
        "bed".to_owned(),
        "bed_color".to_owned(),
        "bed_rough".to_owned(),
    )];
    for (prefix, period, seed, presence, rise, clod) in lumps {
        let n = |s: &str| format!("{prefix}_{s}");
        let (size, jitter, wobble_amount, rag) = if clod {
            ((0.34, 0.5), 1.0, 0.004, 0.5)
        } else {
            ((0.16, 0.26), 0.8, 0.001, 0.0)
        };
        g = cells(
            g,
            prefix,
            [period; 2],
            seed,
            jitter,
            Some(("wobble", wobble_amount)),
        )
        .node(n("a"), remap(hash(n("id"), 23.7), size.0, size.1))
        .node(
            n("b"),
            m(Mul, n("a"), remap(hash(n("id"), 41.3), 0.6, 0.95)),
        )
        .node(
            n("r2"),
            m(
                Add,
                m(Mul, m(Div, n("x"), n("a")), m(Div, n("x"), n("a"))),
                m(Mul, m(Div, n("y"), n("b")), m(Div, n("y"), n("b"))),
            ),
        )
        // A clod's outline is torn; a pebble's is worn smooth.
        .node(n("r2_ragged"), m(Add, n("r2"), remap("rag", -rag, rag)))
        .node(
            n("dome"),
            m(
                Min,
                Levels::new(Math::unary(Sqrt, Clamp::new(m(Sub, 1.0, n("r2_ragged")))))
                    .gamma(if clod { 0.5 } else { 1.0 }),
                Levels::new(n("edge")).in_range(0.0, 0.2).gamma(0.6),
            ),
        )
        .node(
            n("present"),
            m(
                Step,
                m(Mul, presence, Warp::new("cluster", n("off")).amount(1.0)),
                hash(n("id"), 7.13),
            ),
        )
        .node(
            n("height"),
            m(
                Mul,
                m(Mul, ramp(n("dome"), 0.0, 0.1), n("present")),
                m(
                    Add,
                    "bed",
                    m(
                        Mul,
                        m(Sub, n("dome"), remap(hash(n("id"), 57.9), 0.0, 0.3)),
                        rise,
                    ),
                ),
            ),
        )
        .node(
            n("color"),
            if clod {
                m(
                    Mul,
                    Colorize::new(hash(n("id"), 13.37)).gradient([
                        (0.0, [0.052, 0.038, 0.026]),
                        (0.5, [0.070, 0.052, 0.036]),
                        (0.85, [0.086, 0.065, 0.045]),
                        (1.0, [0.098, 0.076, 0.054]),
                    ]),
                    m(
                        Mul,
                        // Dry and pale on the crown, damp at the foot.
                        remap(n("dome"), 0.9, 1.2),
                        remap("crumb", 0.88, 1.12),
                    ),
                )
            } else {
                m(
                    Mul,
                    Colorize::new(hash(n("id"), 13.37)).gradient([
                        (0.0, [0.090, 0.086, 0.080]),
                        (0.5, [0.190, 0.175, 0.150]),
                        (1.0, [0.270, 0.255, 0.230]),
                    ]),
                    remap(n("dome"), 0.7, 1.1),
                )
            },
        )
        .node(
            n("rough"),
            if clod {
                remap(hash(n("id"), 3.1), 0.9, 0.97)
            } else {
                remap(hash(n("id"), 3.1), 0.6, 0.75)
            },
        );
        layers.push((n("height"), n("color"), n("rough")));
    }
    let g = stack(g, "tilth", &layers)
        .node(
            "stalks",
            Scratches::new()
                .count(160)
                .length(0.009)
                .width(0.0016)
                .seed(1231),
        )
        .node(
            "husks",
            Scratches::new()
                .count(90)
                .length(0.004)
                .width(0.002)
                .seed(1233),
        )
        .node("litter", m(Max, "stalks", "husks"))
        .node("litter_height", m(Mul, "litter", m(Add, "tilth_top", 0.05)))
        .node("height", Clamp::new(m(Max, "tilth_top", "litter_height")))
        .node(
            "litter_on",
            ramp(m(Sub, "litter_height", "tilth_top"), 0.0, 0.02),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    "tilth_color",
                    Mix::new([0.22, 0.17, 0.09], [0.15, 0.12, 0.07], "husks"),
                    "litter_on",
                ),
                m(
                    Mul,
                    remap("tone", 0.9, 1.1),
                    remap(ramp("damp", 0.5, 0.75), 1.0, 0.78),
                ),
            ),
        )
        .node("roughness", Mix::new("tilth_rough", 0.85, "litter_on"));
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.05,
        0.006,
    )
}
