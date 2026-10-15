//! Veyra's trees: five barks and five leaf cards, drawn for a painterly shader.
//!
//! The same brief as the terrain: stylised rather than photographic, so every
//! graph is built from shapes a viewer can name (plates, fissures, dashes,
//! leaves, needles) and fine noise is kept to a whisper that a soft shader
//! would smear anyway.
//!
//! Bark runs `u` around the trunk and `v` along it, over a repeat twice as
//! tall as it is wide, and the generator lays a whole number of repeats round
//! every ring, so every field tiles in `u` and most relief runs along `v`.
//!
//! A leaf card is one picture rather than a repeat: a cluster on its twig,
//! drawn in card space from the leaves' own frames, with every piece kept a
//! margin inside the edge so the cut-out never meets the next texel of a
//! repeat and no mip reaches across it. The cluster is painted back to front,
//! each piece over the ones before it, which is what lets a leaf lie over its
//! neighbour with its own colour instead of blending into it.
use ashlar_material::{
    Channel, Input, MaterialGraph, MaterialGraphBuilder, MathOp, Node, PbrOutput,
    nodes::{
        Clamp, Colorize, Combine2, Decompose, Levels, Math, Mix, Noise, OcclusionFromHeight, Uv,
        Voronoi, VoronoiOutput, Warp,
    },
};

use crate::{FLORA_BARK, FLORA_LEAF_RESOLUTION, LEAF_CARD_STEM};

pub(crate) fn m(op: MathOp, a: impl Into<Input>, b: impl Into<Input>) -> Math {
    Math::new(op, a, b)
}

pub(crate) fn remap(input: impl Into<Input>, low: f32, high: f32) -> Levels {
    Levels::new(input).out_range(low, high)
}

pub(crate) fn ramp(input: impl Into<Input>, from: f32, to: f32) -> Levels {
    Levels::new(input).in_range(from, to)
}

/// A second, independent number off a cell's one random float.
pub(crate) fn hash(id: impl Into<Input>, salt: f32) -> Math {
    Math::unary(MathOp::Fract, m(MathOp::Mul, id, salt))
}

/// Every tree graph: the five barks, then the five leaf cards.
pub(crate) fn graphs() -> [MaterialGraph; 11] {
    [
        oak_bark(),
        pine_bark(),
        birch_bark(),
        fir_bark(),
        shrub_bark(),
        oak_leaves(),
        pine_leaves(),
        birch_leaves(),
        fir_leaves(),
        shrub_leaves(),
        fern_leaves(),
    ]
}

/// The key a bark set's graph is registered under.
#[must_use]
pub fn bark_key(set: &str) -> String {
    format!("veyra:bark-{set}")
}

/// The key a leaf set's graph is registered under.
#[must_use]
pub fn leaves_key(set: &str) -> String {
    format!("veyra:leaves-{set}")
}

// ---------------------------------------------------------------------------
// Bark
// ---------------------------------------------------------------------------

/// The repeat a bark set was authored against, read off the table so the graph
/// and the definition cannot disagree.
fn bark_repeat(set: &str) -> [f32; 2] {
    FLORA_BARK
        .iter()
        .find_map(|(name, metres)| (*name == set).then_some(*metres))
        .unwrap_or_else(|| unreachable!("{set} is a bark set"))
}

/// A two-axis displacement in `-1..=1` from two octaves of Perlin, for
/// outlines that should not be drawn with a ruler.
pub(crate) fn wobble(
    g: MaterialGraphBuilder,
    id: &str,
    periods: [u32; 2],
    seed: u32,
) -> MaterialGraphBuilder {
    let noise = |seed| {
        remap(
            Noise::perlin()
                .periods(periods[0], periods[1])
                .octaves(2)
                .seed(seed),
            -1.0,
            1.0,
        )
    };
    g.node(format!("{id}_u"), noise(seed))
        .node(format!("{id}_v"), noise(seed + 1))
        .node(id, Combine2::new(format!("{id}_u"), format!("{id}_v")))
}

/// One population of Voronoi cells read through a displacement: `{prefix}_id`,
/// `_edge`, `_dist`, and `_dx`, `_dy`, the offset to the cell point in cell
/// units, for a shape drawn about it.
pub(crate) fn cells(
    g: MaterialGraphBuilder,
    prefix: &str,
    periods: [u32; 2],
    seed: u32,
    jitter: f32,
    wobble: (&str, f32),
) -> MaterialGraphBuilder {
    let n = |s: &str| format!("{prefix}_{s}");
    let lattice = Voronoi::new()
        .periods(periods[0], periods[1])
        .jitter(jitter)
        .seed(seed);
    let read = |output: VoronoiOutput| -> Node {
        Warp::new(lattice.output(output), wobble.0)
            .amount(wobble.1)
            .into()
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a period is at most 4096, exact in an f32"
    )]
    let scale = periods.map(|p| p as f32);
    g.node(n("id"), read(VoronoiOutput::Cell))
        .node(n("edge"), read(VoronoiOutput::Edge))
        .node(n("dist"), read(VoronoiOutput::Distance))
        .node(n("off"), read(VoronoiOutput::Offset))
        .node(
            n("dx"),
            m(MathOp::Mul, Decompose::new(n("off"), Channel::R), scale[0]),
        )
        .node(
            n("dy"),
            m(MathOp::Mul, Decompose::new(n("off"), Channel::G), scale[1]),
        )
}

/// Bind the height, derive occlusion from it and declare the repeat.
///
/// `relief` is the physical depth of the `0..=1` height in metres. The normal
/// takes one strength for both axes, so it is set against the repeat's width
/// around the trunk, which is where bark's relief lives: slopes along the
/// trunk come out twice as steep as they are, and bark barely slopes that way.
fn finish_bark(
    g: MaterialGraphBuilder,
    output: PbrOutput,
    tile: [f32; 2],
    relief: f32,
    ao_radius: f32,
) -> MaterialGraph {
    let strength = relief / tile[0];
    g.node("physical_height", m(MathOp::Mul, "height", strength))
        .node(
            "ao",
            OcclusionFromHeight::new("physical_height").radius(ao_radius),
        )
        .tile_metres(tile)
        .output(
            output
                .height("height")
                .occlusion("ao")
                .metallic(0.0)
                .normal_strength(strength),
        )
        .into_graph()
}

/// Oak: dark warm brown split into long ridged plates by deep fissures that
/// run up the trunk, fork and meet again, the ridges checked across here and
/// there, with dark moss down in the cracks where the trunk stays damp. 30 mm
/// of relief.
///
/// The ridges are Voronoi cells four times as tall as wide in UV, which over
/// a repeat twice as tall as wide is a ridge eight times as long as it is
/// broad; warped, their edges become the braided fissures oak is known by. A
/// low gamma on the edge distance shoulders each ridge steeply and leaves its
/// top flat, so it reads blocky rather than domed. The checks are shallow
/// wavy lines round the trunk, cut only into ridge tops and only in patches,
/// which is what breaks a ridge into plates without a second set of deep
/// cracks. The tallest ridges are lit paler, which is the value structure a
/// painterly shader keeps at distance.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn oak_bark() -> MaterialGraph {
    use MathOp::{Abs, Add, Fract, Mul, Sub};
    let tile = bark_repeat("oak");
    let g = MaterialGraph::builder(bark_key("oak"))
        .node("tone", Noise::perlin().periods(4, 2).octaves(2).seed(1101))
        .node(
            "fibre",
            Noise::perlin().periods(64, 4).octaves(2).seed(1103),
        )
        .node(
            "moss_field",
            Noise::perlin().periods(4, 4).octaves(2).seed(1105),
        )
        .node(
            "moss_clump",
            Noise::perlin().periods(32, 16).octaves(2).seed(1106),
        )
        .node(
            "check_gate",
            Noise::perlin().periods(16, 8).octaves(2).seed(1111),
        )
        .node("v", Decompose::new(Uv::new(), Channel::G));
    let g = wobble(g, "wobble", [8, 4], 1107);
    let g = cells(g, "plate", [16, 4], 1109, 1.0, ("wobble", 0.02))
        .node(
            "shoulder",
            Levels::new("plate_edge").in_range(0.03, 0.26).gamma(0.45),
        )
        // A whole number of checks down the repeat, wandering by a noise, so
        // the phase meets itself at the seam.
        .node(
            "check_phase",
            m(
                Add,
                m(Mul, "v", 12.0),
                remap(
                    Noise::perlin().periods(16, 4).octaves(1).seed(1113),
                    -0.5,
                    0.5,
                ),
            ),
        )
        .node(
            "check",
            m(
                Mul,
                m(
                    Mul,
                    ramp(
                        Math::unary(Abs, m(Sub, Math::unary(Fract, "check_phase"), 0.5)),
                        0.07,
                        0.02,
                    ),
                    ramp("check_gate", 0.45, 0.55),
                ),
                ramp("shoulder", 0.35, 0.6),
            ),
        )
        .node(
            "face",
            m(
                Sub,
                m(
                    Add,
                    m(Add, 0.6, m(Mul, hash("plate_id", 23.7), 0.24)),
                    m(Mul, "plate_dy", remap(hash("plate_id", 41.3), -0.03, 0.03)),
                ),
                m(Add, m(Mul, "fibre", 0.08), m(Mul, "check", 0.2)),
            ),
        )
        .node("height", Clamp::new(Mix::new(0.04, "face", "shoulder")))
        .node("fissure", ramp("shoulder", 0.6, 0.0))
        .node(
            "wood",
            m(
                Mul,
                Colorize::new(hash("plate_id", 13.37)).gradient([
                    (0.0, [0.055, 0.032, 0.018]),
                    (0.6, [0.080, 0.047, 0.026]),
                    (1.0, [0.104, 0.064, 0.036]),
                ]),
                remap("tone", 0.88, 1.12),
            ),
        )
        .node(
            "lit_wood",
            Mix::new(
                m(Mul, "wood", remap("fibre", 0.9, 1.1)),
                [0.160, 0.105, 0.062],
                m(Mul, ramp("height", 0.55, 0.85), 0.55),
            ),
        )
        // Moss down in the fissures and a little way up their walls, in
        // patches round the trunk rather than in every crack.
        .node(
            "moss",
            m(
                Mul,
                ramp("shoulder", 0.85, 0.25),
                m(
                    Mul,
                    ramp("moss_field", 0.42, 0.62),
                    ramp("moss_clump", 0.3, 0.6),
                ),
            ),
        )
        .node(
            "color",
            Mix::new(
                Mix::new(
                    Mix::new("lit_wood", [0.030, 0.020, 0.013], "check"),
                    [0.016, 0.010, 0.007],
                    "fissure",
                ),
                m(Mul, [0.034, 0.056, 0.016], remap("moss_clump", 0.8, 1.25)),
                m(Mul, "moss", 0.9),
            ),
        )
        .node(
            "roughness",
            Mix::new(remap(hash("plate_id", 3.1), 0.78, 0.9), 0.96, "fissure"),
        );
    finish_bark(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.03,
        0.01,
    )
}

/// Pine: red-brown plates, broad and irregular, flaking in layers and
/// separated by deep dark grooves. 25 mm of relief.
///
/// Two populations: the plates, whose edges are the grooves, and the flakes
/// on them, each tilted its own way so the layers step down like shingles
/// rather than being outlined. The top layer is weathered grey-brown and the
/// layers under it are the fresh orange a fallen scale uncovers, so the
/// colour follows the height and the plates read as stacked.
#[must_use]
pub fn pine_bark() -> MaterialGraph {
    use MathOp::{Add, Mul, Sub};
    let tile = bark_repeat("pine");
    let g = MaterialGraph::builder(bark_key("pine"))
        .node("tone", Noise::perlin().periods(4, 2).octaves(2).seed(1201))
        .node(
            "grain",
            Noise::perlin().periods(64, 16).octaves(2).seed(1203),
        );
    let g = wobble(g, "wobble", [8, 8], 1205);
    let g = cells(g, "plate", [8, 8], 1207, 0.9, ("wobble", 0.03));
    let g = cells(g, "flake", [16, 16], 1209, 1.0, ("wobble", 0.01))
        .node(
            "groove",
            Levels::new("plate_edge").in_range(0.03, 0.18).gamma(0.6),
        )
        .node(
            "flake_top",
            m(
                Add,
                m(Mul, hash("flake_id", 23.7), 0.16),
                m(
                    Add,
                    m(Mul, "flake_dx", remap(hash("flake_id", 41.3), -0.08, 0.08)),
                    m(Mul, "flake_dy", remap(hash("flake_id", 57.9), -0.12, 0.0)),
                ),
            ),
        )
        .node(
            "flake_step",
            Levels::new("flake_edge").in_range(0.0, 0.14).gamma(0.5),
        )
        .node(
            "layer",
            Clamp::new(m(Mul, "flake_step", m(Add, "flake_top", 0.12))),
        )
        .node(
            "height",
            Clamp::new(Mix::new(
                0.03,
                m(
                    Sub,
                    m(
                        Add,
                        m(Add, 0.42, m(Mul, hash("plate_id", 7.7), 0.14)),
                        m(Mul, "layer", 1.2),
                    ),
                    m(Mul, "grain", 0.04),
                ),
                "groove",
            )),
        )
        .node(
            "fresh",
            Colorize::new(hash("flake_id", 13.37)).gradient([
                (0.0, [0.200, 0.075, 0.032]),
                (0.6, [0.290, 0.115, 0.048]),
                (1.0, [0.330, 0.150, 0.068]),
            ]),
        )
        .node(
            "weathered",
            m(
                Mul,
                [0.125, 0.085, 0.068],
                remap(hash("flake_id", 31.1), 0.85, 1.15),
            ),
        )
        .node(
            "plate_color",
            Mix::new("fresh", "weathered", ramp("layer", 0.12, 0.22)),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    "plate_color",
                    [0.022, 0.012, 0.009],
                    ramp("groove", 0.7, 0.0),
                ),
                remap("tone", 0.88, 1.12),
            ),
        )
        .node(
            "roughness",
            Mix::new(
                remap(hash("flake_id", 3.1), 0.72, 0.86),
                0.95,
                ramp("groove", 0.6, 0.0),
            ),
        );
    finish_bark(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.025,
        0.008,
    )
}

/// Birch: chalky white, crossed by thin dark lenticels lying round the trunk,
/// with a few black diamonds where branches were and faint papery bands
/// peeling round it. 3 mm of relief, most of it in the scars.
///
/// The lenticels are a flattened ellipse per cell in a lattice sixteen times
/// finer along the trunk than round it, each its own length, and only about
/// half the cells hold one, so they fall in loose rows rather than a grid.
/// The diamonds are a Manhattan distance about the points of a sparse
/// lattice, wider than tall, gated so only two or three show in a repeat.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn birch_bark() -> MaterialGraph {
    use MathOp::{Abs, Add, Div, Max, Mul, Step, Sub};
    let tile = bark_repeat("birch");
    let g = MaterialGraph::builder(bark_key("birch"))
        .node("tone", Noise::perlin().periods(2, 4).octaves(2).seed(1301))
        .node(
            "bands",
            Noise::perlin().periods(2, 32).octaves(2).seed(1303),
        )
        .node(
            "chalk",
            Noise::perlin().periods(32, 64).octaves(2).seed(1305),
        );
    let g = wobble(g, "wobble", [8, 16], 1307);
    let g = cells(g, "dash", [4, 64], 1309, 0.8, ("wobble", 0.003));
    let g = cells(g, "scar", [2, 4], 1311, 0.7, ("wobble", 0.012))
        .node("dash_len", remap(hash("dash_id", 23.7), 0.1, 0.42))
        .node(
            "dash_shape",
            m(
                Sub,
                1.0,
                m(
                    Add,
                    m(
                        Mul,
                        m(Div, "dash_dx", "dash_len"),
                        m(Div, "dash_dx", "dash_len"),
                    ),
                    m(Mul, m(Div, "dash_dy", 0.14), m(Div, "dash_dy", 0.14)),
                ),
            ),
        )
        .node(
            "dash",
            m(
                Mul,
                ramp("dash_shape", 0.0, 0.3),
                m(Step, hash("dash_id", 7.13), 0.45),
            ),
        )
        .node("scar_w", remap(hash("scar_id", 31.1), 0.2, 0.32))
        .node(
            "scar_shape",
            m(
                Sub,
                1.0,
                m(
                    Add,
                    m(Div, Math::unary(Abs, "scar_dx"), "scar_w"),
                    m(Div, Math::unary(Abs, "scar_dy"), m(Mul, "scar_w", 0.45)),
                ),
            ),
        )
        .node(
            "scar",
            m(
                Mul,
                ramp("scar_shape", 0.0, 0.1),
                m(Step, hash("scar_id", 7.13), 0.68),
            ),
        )
        .node("peel", ramp("bands", 0.58, 0.64))
        .node(
            "height",
            Clamp::new(m(
                Sub,
                m(
                    Add,
                    m(Add, 0.5, m(Mul, "peel", 0.06)),
                    m(Mul, "chalk", 0.05),
                ),
                m(
                    Max,
                    m(Mul, "dash", 0.2),
                    m(Mul, ramp("scar_shape", 0.0, 0.5), m(Mul, "scar", 0.3)),
                ),
            )),
        )
        .node(
            "white",
            m(
                Mul,
                Mix::new(
                    Mix::new([0.62, 0.61, 0.57], [0.72, 0.71, 0.67], "chalk"),
                    [0.60, 0.50, 0.42],
                    m(Mul, "peel", 0.45),
                ),
                remap("tone", 0.92, 1.05),
            ),
        )
        .node(
            "scar_color",
            Mix::new(
                [0.016, 0.014, 0.013],
                [0.07, 0.062, 0.055],
                ramp("chalk", 0.5, 0.9),
            ),
        )
        .node(
            "color",
            Mix::new(
                Mix::new("white", [0.060, 0.050, 0.046], "dash"),
                "scar_color",
                "scar",
            ),
        )
        .node("roughness", Mix::new(0.85, 0.93, m(Max, "dash", "scar")));
    finish_bark(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.003,
        0.006,
    )
}

/// Fir: smooth grey-brown bark, mottled and patched with pale lichen, lined
/// round the trunk with short dark lenticels, split by the odd shallow crack
/// along it, and raised here and there in a resin blister with a dark core.
/// 4 mm of relief.
///
/// The lenticels are the birch's dashes, fainter and fewer; the blisters are
/// domes about sparse Voronoi points, each its own size, the bark's own
/// colour at the rim and dark glossy resin at the heart, so they read as
/// raised bumps under a low sun rather than as holes.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn fir_bark() -> MaterialGraph {
    use MathOp::{Add, Div, Max, Mul, Step, Sub};
    let tile = bark_repeat("fir");
    let g = MaterialGraph::builder(bark_key("fir"))
        .node("tone", Noise::perlin().periods(2, 4).octaves(2).seed(1401))
        .node(
            "bands",
            Noise::perlin().periods(4, 64).octaves(2).seed(1403),
        )
        .node(
            "mottle",
            Noise::perlin().periods(8, 16).octaves(2).seed(1405),
        )
        .node(
            "lichen",
            Noise::perlin().periods(8, 8).octaves(3).seed(1406),
        )
        .node(
            "crack_gate",
            Noise::perlin().periods(4, 2).octaves(2).seed(1408),
        );
    let g = wobble(g, "wobble", [8, 8], 1407);
    let g = cells(g, "blister", [8, 16], 1409, 0.8, ("wobble", 0.004));
    let g = cells(g, "dash", [4, 128], 1411, 0.8, ("wobble", 0.002));
    let g = cells(g, "crack", [16, 4], 1413, 1.0, ("wobble", 0.02))
        .node("blister_size", remap(hash("blister_id", 31.1), 0.18, 0.34))
        .node(
            "blister_r",
            m(
                Sub,
                1.0,
                m(
                    Add,
                    m(
                        Mul,
                        m(Div, "blister_dx", "blister_size"),
                        m(Div, "blister_dx", "blister_size"),
                    ),
                    m(
                        Mul,
                        m(Div, "blister_dy", m(Mul, "blister_size", 0.8)),
                        m(Div, "blister_dy", m(Mul, "blister_size", 0.8)),
                    ),
                ),
            ),
        )
        .node(
            "blister",
            m(
                Mul,
                Levels::new("blister_r").gamma(0.6),
                m(Step, hash("blister_id", 7.13), 0.85),
            ),
        )
        .node(
            "resin",
            m(
                Mul,
                ramp("blister_r", 0.45, 0.7),
                m(Step, hash("blister_id", 7.13), 0.85),
            ),
        )
        .node("dash_len", remap(hash("dash_id", 23.7), 0.12, 0.4))
        .node(
            "dash",
            m(
                Mul,
                ramp(
                    m(
                        Sub,
                        1.0,
                        m(
                            Add,
                            m(
                                Mul,
                                m(Div, "dash_dx", "dash_len"),
                                m(Div, "dash_dx", "dash_len"),
                            ),
                            m(Mul, m(Div, "dash_dy", 0.2), m(Div, "dash_dy", 0.2)),
                        ),
                    ),
                    0.0,
                    0.35,
                ),
                m(Step, hash("dash_id", 7.13), 0.6),
            ),
        )
        .node(
            "crack",
            m(
                Mul,
                ramp("crack_edge", 0.04, 0.0),
                ramp("crack_gate", 0.55, 0.65),
            ),
        )
        .node("band", ramp("bands", 0.55, 0.8))
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(Add, 0.35, m(Mul, "mottle", 0.12)),
                m(
                    Sub,
                    m(Mul, "blister", 0.32),
                    m(
                        Add,
                        m(Max, m(Mul, "dash", 0.06), m(Mul, "band", 0.03)),
                        m(Mul, "crack", 0.14),
                    ),
                ),
            )),
        )
        .node(
            "bark",
            m(
                Mul,
                Mix::new(
                    Mix::new([0.165, 0.150, 0.130], [0.245, 0.225, 0.195], "mottle"),
                    [0.27, 0.29, 0.22],
                    m(Mul, ramp("lichen", 0.55, 0.66), 0.75),
                ),
                remap("tone", 0.9, 1.1),
            ),
        )
        .node(
            "color",
            Mix::new(
                Mix::new(
                    Mix::new(
                        "bark",
                        [0.100, 0.090, 0.080],
                        m(Max, "dash", m(Mul, "band", 0.4)),
                    ),
                    [0.040, 0.034, 0.030],
                    "crack",
                ),
                [0.080, 0.042, 0.016],
                m(Mul, "resin", 0.9),
            ),
        )
        .node("roughness", Mix::new(0.8, 0.3, "resin"));
    finish_bark(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.004,
        0.006,
    )
}

/// Shrub: thin brown bark with fine vertical ridges and the odd pale
/// lenticel lying across the stem. 2 mm of relief.
#[must_use]
pub fn shrub_bark() -> MaterialGraph {
    use MathOp::{Add, Div, Mul, Step, Sub};
    let tile = bark_repeat("shrub");
    let g = MaterialGraph::builder(bark_key("shrub"))
        .node("tone", Noise::perlin().periods(2, 2).octaves(2).seed(1501))
        .node(
            "streaks",
            Noise::perlin().periods(64, 4).octaves(2).seed(1503),
        )
        .node(
            "fine",
            Noise::perlin().periods(128, 8).octaves(1).seed(1505),
        );
    let g = wobble(g, "wobble", [8, 8], 1507);
    let g = cells(g, "dot", [16, 32], 1509, 0.8, ("wobble", 0.004))
        .node("ridge", Levels::new("streaks").in_range(0.35, 0.65))
        .node(
            "dot",
            m(
                Mul,
                ramp(
                    m(
                        Sub,
                        1.0,
                        m(
                            Add,
                            m(Mul, m(Div, "dot_dx", 0.28), m(Div, "dot_dx", 0.28)),
                            m(Mul, m(Div, "dot_dy", 0.12), m(Div, "dot_dy", 0.12)),
                        ),
                    ),
                    0.0,
                    0.3,
                ),
                m(Step, hash("dot_id", 7.13), 0.9),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(Add, 0.35, m(Mul, "ridge", 0.3)),
                m(Add, m(Mul, "fine", 0.08), m(Mul, "dot", 0.05)),
            )),
        )
        .node(
            "color",
            Mix::new(
                m(
                    Mul,
                    Mix::new([0.052, 0.029, 0.019], [0.140, 0.080, 0.050], "ridge"),
                    m(Mul, remap("tone", 0.88, 1.12), remap("fine", 0.92, 1.06)),
                ),
                [0.22, 0.18, 0.13],
                "dot",
            ),
        )
        .node("roughness", remap("fine", 0.72, 0.85));
    finish_bark(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.002,
        0.006,
    )
}

// ---------------------------------------------------------------------------
// Leaf cards
// ---------------------------------------------------------------------------

/// The width of a cut edge in card UV: two texels at the shipped resolution,
/// so the silhouette is anti-aliased at level 0 and the coverage-preserving
/// mips have a slope to set their line on.
#[expect(
    clippy::cast_precision_loss,
    reason = "a bake resolution is at most 4096, exact in an f32"
)]
const EDGE: f32 = 2.0 / FLORA_LEAF_RESOLUTION as f32;

/// A cut-out from a signed distance in card UV, positive inside: the edge
/// ramp is centred on the outline, so a needle a few texels across keeps its
/// width rather than losing a texel down each side.
fn cut(distance: impl Into<Input>) -> Levels {
    ramp(distance, -0.5 * EDGE, 0.5 * EDGE)
}

/// The height of the card outside the cluster and of a leaf's rim. The card
/// is cut away there, so the only thing it decides is that a leaf's own edge
/// is not a cliff down to zero in the normal.
const RIM: f32 = 0.3;

/// Leaves' physical relief in metres over the card's one-metre repeat.
const LEAF_RELIEF: f32 = 0.006;

/// A small deterministic generator for the layout, so a card is the same
/// picture on every machine without a table of hand-typed numbers.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9);
        let mut z = self.0;
        z = (z ^ (z >> 16)).wrapping_mul(0x21f0_aaad);
        z = (z ^ (z >> 15)).wrapping_mul(0x735a_2d97);
        z ^= z >> 15;
        f32::from(u16::try_from(z >> 16).unwrap_or(0)) / 65536.0
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

/// Everything a painted piece leaves for the stack: its coverage, its height,
/// its colour and its roughness, as node ids.
struct Layer {
    mask: String,
    height: String,
    color: String,
    rough: String,
}

/// A leaf card under construction: the builder, the stack painted so far,
/// and the box it covers. Every piece reads the `u` and `v` nodes, which are
/// only defined at [`Card::finish`], once the box says how to fit it.
struct Card {
    g: MaterialGraphBuilder,
    top: Option<Layer>,
    count: usize,
    /// A box round everything painted, in drawing space: least `u`, least
    /// `v`, most `u`, most `v`. Traced from each piece's outline, never
    /// smaller than it, because it is what keeps the cluster off the edges.
    extent: [f32; 4],
}

/// How far inside the card's edge the cluster's box is fitted: the 2% a mip
/// chain needs clear, and a texel of slack for the anti-aliased edge.
const MARGIN: f32 = 0.03;

impl Card {
    fn new(key: String, background: [f32; 3]) -> Self {
        let g = MaterialGraph::builder(key).node("background", Clamp::new(background));
        Self {
            g,
            top: None,
            count: 0,
            extent: [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
        }
    }

    /// Grow the box to take in a disc of `radius` about `point`.
    fn reach(&mut self, point: [f32; 2], radius: f32) {
        self.extent[0] = self.extent[0].min(point[0] - radius);
        self.extent[1] = self.extent[1].min(point[1] - radius);
        self.extent[2] = self.extent[2].max(point[0] + radius);
        self.extent[3] = self.extent[3].max(point[1] + radius);
    }

    /// How much the drawing must shrink about the stem point for its box to
    /// sit `MARGIN` inside every edge: one where it already fits.
    fn fit(&self) -> f32 {
        let [x, y] = LEAF_CARD_STEM;
        let [low_u, low_v, high_u, high_v] = self.extent;
        let limit = 1.0 - MARGIN;
        [
            (x - low_u) / (x - MARGIN),
            (y - low_v) / (y - MARGIN),
            (high_u - x) / (limit - x),
            (high_v - y) / (limit - y),
        ]
        .into_iter()
        .fold(1.0, f32::max)
    }

    fn node(mut self, id: impl Into<String>, node: impl Into<Node>) -> Self {
        self.g = self.g.node(id, node);
        self
    }

    /// Paint a piece over everything before it. Its mask is the weight, so a
    /// leaf's anti-aliased edge blends over what lies under it rather than
    /// stepping, and the opacity is the union.
    fn paint(mut self, layer: &Layer) -> Self {
        let index = self.count;
        let n = |s: &str| format!("stack{index}_{s}");
        let (under_mask, under_height, under_color, under_rough) = match &self.top {
            Some(top) => (
                Input::from(top.mask.as_str()),
                Input::from(top.height.as_str()),
                Input::from(top.color.as_str()),
                Input::from(top.rough.as_str()),
            ),
            None => (
                Input::from(0.0),
                Input::from(RIM),
                Input::from("background"),
                Input::from(0.8),
            ),
        };
        self.g = self
            .g
            .node(n("mask"), m(MathOp::Max, under_mask, layer.mask.as_str()))
            .node(
                n("height"),
                Mix::new(under_height, layer.height.as_str(), layer.mask.as_str()),
            )
            .node(
                n("color"),
                Mix::new(under_color, layer.color.as_str(), layer.mask.as_str()),
            )
            .node(
                n("rough"),
                Mix::new(under_rough, layer.rough.as_str(), layer.mask.as_str()),
            );
        self.top = Some(Layer {
            mask: n("mask"),
            height: n("height"),
            color: n("color"),
            rough: n("rough"),
        });
        self.count += 1;
        self
    }

    /// A frame about `origin` pointing `angle` degrees clockwise from straight
    /// up the card, in units of `scale`: `{prefix}_t` along it and
    /// `{prefix}_s` across it, to the right of the direction.
    fn frame(self, prefix: &str, origin: [f32; 2], angle: f32, scale: f32) -> Self {
        use MathOp::{Add, Mul};
        let (sin, cos) = angle.to_radians().sin_cos();
        // Up the card is -v.
        let (dx, dy) = (sin, -cos);
        let k = 1.0 / scale;
        self.node(
            format!("{prefix}_t"),
            m(
                Add,
                m(Add, m(Mul, "u", dx * k), m(Mul, "v", dy * k)),
                -(origin[0] * dx + origin[1] * dy) * k,
            ),
        )
        .node(
            format!("{prefix}_s"),
            m(
                Add,
                m(Add, m(Mul, "u", -dy * k), m(Mul, "v", dx * k)),
                (origin[0] * dy - origin[1] * dx) * k,
            ),
        )
    }

    /// A round stem from `a` to `b`, `radius[0]` thick at `a` and
    /// `radius[1]` at `b`, as a raised cylinder in `color`.
    fn stem(
        mut self,
        prefix: &str,
        a: [f32; 2],
        b: [f32; 2],
        radius: [f32; 2],
        color: [f32; 3],
    ) -> Self {
        use MathOp::{Add, Div, Mul, Sqrt, Sub};
        let n = |s: &str| format!("{prefix}_{s}");
        self.reach(a, radius[0]);
        self.reach(b, radius[1]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len2 = d[0] * d[0] + d[1] * d[1];
        let height = RIM + 0.02 + self.depth();
        self = self
            .node(
                n("h"),
                Clamp::new(m(
                    Add,
                    m(Add, m(Mul, "u", d[0] / len2), m(Mul, "v", d[1] / len2)),
                    -(a[0] * d[0] + a[1] * d[1]) / len2,
                )),
            )
            .node(n("px"), m(Sub, m(Sub, "u", a[0]), m(Mul, n("h"), d[0])))
            .node(n("py"), m(Sub, m(Sub, "v", a[1]), m(Mul, n("h"), d[1])))
            .node(
                n("dist"),
                Math::unary(
                    Sqrt,
                    m(Add, m(Mul, n("px"), n("px")), m(Mul, n("py"), n("py"))),
                ),
            )
            .node(
                n("r"),
                m(Add, radius[0], m(Mul, n("h"), radius[1] - radius[0])),
            )
            .node(n("mask"), cut(m(Sub, n("r"), n("dist"))))
            .node(
                n("round"),
                Math::unary(
                    Sqrt,
                    Clamp::new(m(
                        Sub,
                        1.0,
                        m(Mul, m(Div, n("dist"), n("r")), m(Div, n("dist"), n("r"))),
                    )),
                ),
            )
            .node(n("height"), m(Add, height, m(Mul, n("round"), 0.12)))
            .node(n("color"), m(Mul, color, remap(n("round"), 0.6, 1.15)))
            .node(n("rough"), Clamp::new(0.7));
        self.paint(&Layer {
            mask: n("mask"),
            height: n("height"),
            color: n("color"),
            rough: n("rough"),
        })
    }

    /// A polyline of stems through `points`, tapering from `radius[0]` to
    /// `radius[1]` along its whole length.
    fn twig(
        mut self,
        prefix: &str,
        points: &[[f32; 2]],
        radius: [f32; 2],
        color: [f32; 3],
    ) -> Self {
        let lengths: Vec<f32> = points
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .collect();
        let total: f32 = lengths.iter().sum();
        let mut run = 0.0;
        for (index, (pair, length)) in points.windows(2).zip(&lengths).enumerate() {
            let at = |s: f32| radius[0] + (radius[1] - radius[0]) * s / total;
            let r = [at(run), at(run + length)];
            run += length;
            self = self.stem(&format!("{prefix}{index}"), pair[0], pair[1], r, color);
        }
        self
    }

    /// How far down the stack the next piece lies, as a small height step so
    /// later pieces stand over earlier ones.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a card paints well under a thousand pieces"
    )]
    fn depth(&self) -> f32 {
        (self.count as f32).min(60.0) * 0.003
    }

    /// Bind the outputs: the stack's colour darkened by the occlusion its own
    /// overlaps cast, so the cluster reads as layered even under flat light,
    /// and the stack's union as the opacity.
    ///
    /// The occlusion reads the height as though it were `DEPTH` metres deep
    /// rather than the normal's few millimetres: the leaves of a real cluster
    /// lie centimetres apart, which the flat card has squashed, and that
    /// spacing is what shades one leaf under the next.
    fn finish(self) -> MaterialGraph {
        use MathOp::{Add, Mul};
        const DEPTH: f32 = 0.06;
        let top = self.top.as_ref().expect("a card paints something");
        let strength = LEAF_RELIEF;
        // Every piece reads `u` and `v`, so scaling them about the stem point
        // shrinks the whole drawing into the card at once.
        let fit = self.fit();
        let [x, y] = LEAF_CARD_STEM;
        self.g
            .node(
                "u",
                m(
                    Add,
                    m(Mul, Decompose::new(Uv::new(), Channel::R), fit),
                    x * (1.0 - fit),
                ),
            )
            .node(
                "v",
                m(
                    Add,
                    m(Mul, Decompose::new(Uv::new(), Channel::G), fit),
                    y * (1.0 - fit),
                ),
            )
            .node("height", Clamp::new(top.height.as_str()))
            .node("spaced_height", m(Mul, "height", DEPTH))
            .node(
                "ao",
                OcclusionFromHeight::new("spaced_height").radius(0.025),
            )
            .node("tone", Noise::perlin().period(4).octaves(2).seed(77))
            .node(
                "color",
                m(
                    Mul,
                    m(Mul, top.color.as_str(), remap("ao", 0.35, 1.0)),
                    remap("tone", 0.92, 1.08),
                ),
            )
            .node("opacity", Clamp::new(top.mask.as_str()))
            .node("roughness", Clamp::new(top.rough.as_str()))
            .tile_metres([1.0, 1.0])
            .output(
                PbrOutput::new()
                    .base_color("color")
                    .roughness("roughness")
                    .occlusion("ao")
                    .metallic(0.0)
                    .height("height")
                    .normal_strength(strength)
                    .opacity("opacity"),
            )
            .into_graph()
    }
}

/// How a broad leaf's margin is cut.
#[derive(Clone, Copy)]
enum Margin {
    /// Smooth, as a shrub's round leaf.
    Entire,
    /// Rounded lobes, `count` a side, cut `depth` of the half-width in.
    Lobed { count: f32, depth: f32 },
    /// Fine forward-pointing teeth, `count` along the leaf, `depth` deep.
    Serrate { count: f32, depth: f32 },
}

/// A broad leaf's outline and palette, shared by a species.
#[derive(Clone, Copy)]
struct LeafForm {
    /// The half-width at the widest, over the length.
    width: f32,
    /// Exponents of the base and the tip in `t^base * (1 - t)^tip`: a small
    /// base exponent is a rounded base, a tip exponent near one a pointed tip.
    base: f32,
    tip: f32,
    margin: Margin,
    /// Colour of the pale midrib and lateral veins.
    rib: [f32; 3],
    /// Lateral veins down each side, zero for none.
    veins: f32,
}

/// One broad leaf in card space.
struct Leaf {
    /// Where the blade starts (the petiole ends), in card UV.
    at: [f32; 2],
    /// Degrees clockwise from straight up the card.
    angle: f32,
    /// Blade length in card UV.
    length: f32,
    /// How far the midrib curls sideways at the tip, over the length.
    bend: f32,
    color: [f32; 3],
}

impl Card {
    /// Paint one broad leaf: a blade whose half-width is a profile along the
    /// midrib, cut by its margin, folded along the midrib and arched along its
    /// length, dark at the base, lighter at the tip, with a pale midrib.
    #[expect(clippy::too_many_lines, reason = "one leaf, in field dependency order")]
    fn leaf(mut self, prefix: &str, leaf: &Leaf, form: &LeafForm) -> Self {
        use MathOp::{Abs, Add, Cos, Div, Fract, Mul, Pow, Step, Sub};
        let n = |s: &str| format!("{prefix}_{s}");
        let widest = form.base / (form.base + form.tip);
        let peak = widest.powf(form.base) * (1.0 - widest).powf(form.tip);
        // The outline's envelope, traced along the midrib: the margin only
        // ever cuts into it.
        for step in 0..=16u8 {
            let t = f32::from(step) / 16.0;
            let half = form.width / peak * t.powf(form.base) * (1.0 - t).powf(form.tip);
            let mid = toward(leaf.at, leaf.angle, t * leaf.length);
            for side in [-half, half] {
                let across = (side + leaf.bend * t * t) * leaf.length;
                self.reach(toward(mid, leaf.angle + 90.0, across), 0.0);
            }
        }
        let height = RIM + 0.02 + self.depth();
        self = self
            .frame(prefix, leaf.at, leaf.angle, leaf.length)
            .node(
                n("n"),
                m(Sub, n("s"), m(Mul, m(Mul, n("t"), n("t")), leaf.bend)),
            )
            .node(n("tc"), Clamp::new(n("t")))
            .node(
                n("profile"),
                m(
                    Mul,
                    m(
                        Mul,
                        m(Pow, n("tc"), form.base),
                        m(Pow, m(Sub, 1.0, n("tc")), form.tip),
                    ),
                    form.width / peak,
                ),
            )
            .node(n("an"), Math::unary(Abs, n("n")));
        let margin: Node = match form.margin {
            Margin::Entire => Clamp::new(1.0).into(),
            Margin::Lobed { count, depth } => {
                // |cos| has rounded crests and sharp troughs: rounded lobes
                // and narrow sinuses. The far side is shifted half a lobe, as
                // an oak's lobes alternate.
                let wave = Math::unary(
                    Abs,
                    Math::unary(
                        Cos,
                        m(
                            Add,
                            m(Mul, n("t"), count * 0.5),
                            m(Add, m(Mul, m(Step, n("n"), 0.0), 0.25), 0.1),
                        ),
                    ),
                );
                m(
                    Sub,
                    1.0,
                    m(
                        Mul,
                        m(Sub, 1.0, Levels::new(wave).gamma(0.4)),
                        m(Mul, depth, ramp(n("t"), 0.08, 0.3)),
                    ),
                )
                .into()
            }
            Margin::Serrate { count, depth } => m(
                Sub,
                1.0,
                m(Mul, Math::unary(Fract, m(Mul, n("t"), count)), depth),
            )
            .into(),
        };
        self = self
            .node(n("margin"), margin)
            .node(n("w"), m(Mul, n("profile"), n("margin")))
            // Gated to the blade's own length: past either end the width is
            // zero, and a centred edge would draw its axis as a hairline.
            .node(
                n("mask"),
                m(
                    Mul,
                    cut(m(Mul, m(Sub, n("w"), n("an")), leaf.length)),
                    m(Mul, ramp(n("t"), 0.0, 0.01), ramp(n("t"), 1.0, 0.99)),
                ),
            )
            .node(n("across"), Clamp::new(m(Div, n("an"), n("profile"))))
            .node(
                n("rib"),
                m(
                    Mul,
                    ramp(m(Mul, n("an"), leaf.length), 0.0035, 0.001),
                    ramp(n("t"), 0.92, 0.75),
                ),
            );
        let vein: Node = if form.veins > 0.0 {
            m(
                Mul,
                m(
                    Mul,
                    ramp(
                        Math::unary(
                            Abs,
                            m(
                                Sub,
                                Math::unary(
                                    Fract,
                                    m(Mul, m(Sub, n("t"), m(Mul, n("across"), 0.18)), form.veins),
                                ),
                                0.5,
                            ),
                        ),
                        0.06,
                        0.0,
                    ),
                    ramp(n("across"), 0.85, 0.5),
                ),
                ramp(n("t"), 0.9, 0.7),
            )
            .into()
        } else {
            Clamp::new(0.0).into()
        };
        self = self
            .node(n("vein"), vein)
            .node(
                n("height"),
                m(
                    Add,
                    m(
                        Add,
                        height,
                        m(Mul, Levels::new(n("across")).gamma(1.5), 0.12),
                    ),
                    m(
                        Sub,
                        m(Mul, m(Mul, n("tc"), m(Sub, 1.0, n("tc"))), 0.5),
                        m(Mul, n("rib"), 0.03),
                    ),
                ),
            )
            .node(
                n("body"),
                Mix::new(
                    Mix::new(m(Mul, leaf.color, 0.5), leaf.color, ramp(n("t"), 0.0, 0.45)),
                    m(Mul, leaf.color, [1.45, 1.4, 1.2]),
                    m(Mul, ramp(n("t"), 0.55, 1.0), 0.6),
                ),
            )
            .node(
                n("color"),
                m(
                    Mul,
                    Mix::new(
                        Mix::new(n("body"), form.rib, m(Mul, n("rib"), 0.6)),
                        form.rib,
                        m(Mul, n("vein"), 0.22),
                    ),
                    remap(n("across"), 1.05, 0.9),
                ),
            )
            .node(n("rough"), remap(n("across"), 0.62, 0.72));
        self.paint(&Layer {
            mask: n("mask"),
            height: n("height"),
            color: n("color"),
            rough: n("rough"),
        })
    }

    /// Paint needles either side of a straight shoot from `origin`, `length`
    /// long at `angle` degrees: two ranks offset half a needle, shorter
    /// towards the tip, raked forward and curving, with the new growth at the
    /// tip paler. A fir's flat spray and a pine's brush are the same drawing
    /// with other numbers.
    #[expect(
        clippy::too_many_lines,
        reason = "one shoot, in field dependency order"
    )]
    fn needles(
        mut self,
        prefix: &str,
        origin: [f32; 2],
        angle: f32,
        length: f32,
        form: &Needles,
    ) -> Self {
        use MathOp::{Abs, Add, Div, Floor, Fract, Mul, Sqrt, Step, Sub};
        let n = |s: &str| format!("{prefix}_{s}");
        let height = RIM + 0.02 + self.depth();
        let pitch = form.pitch;
        // Half the spacing between needles of a rank, square to them: where
        // one needle's cell meets the next, and the groove between them.
        let cell = 0.5 * pitch / (1.0 + form.rake * form.rake).sqrt();
        // The needles' roots and tips, traced along the shoot: each tip
        // leans forward of its root by the rake and the curve.
        for step in 0..=16u8 {
            let root = form.start + (1.0 - form.start) * f32::from(step) / 16.0;
            let reach = form.length * (1.0 - (1.0 - form.taper) * root);
            let base = toward(origin, angle, root * length);
            self.reach(base, form.width);
            let tip = toward(base, angle, reach * form.rake + reach * reach * form.curve);
            for side in [-reach, reach] {
                self.reach(toward(tip, angle + 90.0, side), form.width);
            }
        }
        self = self
            .frame(prefix, origin, angle, 1.0)
            .node(n("as"), Math::unary(Abs, n("s")))
            .node(n("f"), m(Div, n("t"), length))
            .node(
                n("q"),
                m(
                    Add,
                    m(
                        Div,
                        m(
                            Sub,
                            m(Sub, n("t"), m(Mul, n("as"), form.rake)),
                            m(Mul, m(Mul, n("as"), n("as")), form.curve),
                        ),
                        pitch,
                    ),
                    m(Mul, m(Step, n("s"), 0.0), 0.5),
                ),
            )
            .node(n("id"), Math::unary(Floor, n("q")))
            .node(
                n("hash"),
                hash(
                    m(
                        Add,
                        m(Add, n("id"), m(Mul, m(Step, n("s"), 0.0), 17.0)),
                        0.37,
                    ),
                    0.618_034,
                ),
            )
            // Where the needle leaves the shoot, as a share of the shoot: what
            // shortens the needles and pales them towards the tip.
            .node(
                n("root"),
                Clamp::new(m(Mul, m(Add, n("id"), 0.5), pitch / length)),
            )
            .node(
                n("perp"),
                m(
                    Mul,
                    Math::unary(Abs, m(Sub, Math::unary(Fract, n("q")), 0.5)),
                    pitch / (1.0 + form.rake * form.rake).sqrt(),
                ),
            )
            .node(
                n("len"),
                m(
                    Mul,
                    m(Mul, form.length, remap(n("hash"), form.ragged, 1.0)),
                    remap(n("root"), 1.0, form.taper),
                ),
            )
            .node(n("along"), Clamp::new(m(Div, n("as"), n("len"))))
            .node(
                n("w"),
                m(
                    Mul,
                    form.width,
                    Math::unary(
                        Sqrt,
                        Clamp::new(m(Sub, 1.0, Levels::new(n("along")).gamma(3.0))),
                    ),
                ),
            )
            .node(
                n("mask"),
                m(
                    Mul,
                    m(
                        Mul,
                        cut(m(Sub, n("w"), n("perp"))),
                        ramp(n("along"), 1.0, 0.97),
                    ),
                    m(
                        Mul,
                        ramp(n("root"), form.start, form.start + 0.02),
                        ramp(n("root"), 1.0, 0.97),
                    ),
                ),
            )
            .node(n("across"), Clamp::new(m(Div, n("perp"), n("w"))))
            .node(
                n("height"),
                m(
                    Add,
                    m(Add, height, m(Mul, n("hash"), 0.03)),
                    m(
                        Mul,
                        Math::unary(Sqrt, m(Sub, 1.0, m(Mul, n("across"), n("across")))),
                        0.08,
                    ),
                ),
            )
            .node(
                n("color"),
                m(
                    Mul,
                    Mix::new(
                        Mix::new(
                            m(Mul, form.colors[0], remap(n("along"), 0.6, 1.0)),
                            m(Mul, form.colors[0], 1.3),
                            m(Mul, n("hash"), m(Sub, 1.0, n("along"))),
                        ),
                        form.colors[1],
                        m(Mul, ramp(n("root"), 0.72, 0.95), ramp(n("along"), 0.1, 0.5)),
                    ),
                    m(
                        Mul,
                        remap(n("across"), 1.1, 0.8),
                        remap(ramp(n("perp"), 0.65 * cell, cell), 1.0, 0.55),
                    ),
                ),
            )
            .node(n("rough"), Clamp::new(0.6));
        self.paint(&Layer {
            mask: n("mask"),
            height: n("height"),
            color: n("color"),
            rough: n("rough"),
        })
    }
}

/// How a shoot's needles grow.
#[derive(Clone, Copy)]
struct Needles {
    /// Reach of a needle away from the shoot, measured square to it, at the
    /// shoot's base.
    length: f32,
    /// What that reach falls to at the shoot's tip, as a share.
    taper: f32,
    /// Distance along the shoot between needles of one rank.
    pitch: f32,
    /// How far forward a needle leans: along the shoot per unit out from it.
    rake: f32,
    /// How much further forward it curves towards its tip.
    curve: f32,
    /// Half-width at the base of a needle.
    width: f32,
    /// Share of the shoot at its base left bare.
    start: f32,
    /// The shortest a needle is drawn, as a share of the longest: a ragged
    /// outline rather than a clipped one.
    ragged: f32,
    /// The needle's own green, and the pale new growth at the shoot's tip.
    colors: [[f32; 3]; 2],
}

/// A leaf colour: a pick along the species' greens, a little lighter or
/// darker, and on the one leaf a card chooses, turning yellow. Chosen rather
/// than rolled, so a card never comes out with a third of its leaves turned.
fn leaf_color(
    rng: &mut Rng,
    greens: [[f32; 3]; 3],
    yellow: Option<[f32; 3]>,
    shade: f32,
) -> [f32; 3] {
    let t = rng.next();
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    let green = if t < 0.5 {
        mix(greens[0], greens[1], t * 2.0)
    } else {
        mix(greens[1], greens[2], t * 2.0 - 1.0)
    };
    let turned = yellow.map_or(green, |yellow| mix(green, yellow, 0.6));
    let value = rng.range(0.85, 1.15) * shade;
    turned.map(|c| c * value)
}

/// A point `s` of the way along a polyline, and the heading there in degrees
/// clockwise from straight up the card.
fn along(points: &[[f32; 2]], s: f32) -> ([f32; 2], f32) {
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .collect();
    let mut rest = s * lengths.iter().sum::<f32>();
    let last = lengths.len() - 1;
    for (index, (pair, length)) in points.windows(2).zip(&lengths).enumerate() {
        if rest <= *length || index == last {
            let f = (rest / length).clamp(0.0, 1.0);
            let d = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
            let heading = d[0].atan2(-d[1]).to_degrees();
            return ([pair[0][0] + d[0] * f, pair[0][1] + d[1] * f], heading);
        }
        rest -= length;
    }
    unreachable!("a polyline has a segment")
}

/// The point `distance` along `angle` degrees clockwise from up, from `at`.
fn toward(at: [f32; 2], angle: f32, distance: f32) -> [f32; 2] {
    let (sin, cos) = angle.to_radians().sin_cos();
    [at[0] + sin * distance, at[1] - cos * distance]
}

const TWIG: [f32; 3] = [0.060, 0.040, 0.026];

/// The thinnest stem a card draws: two texels across, so a petiole is a line
/// rather than a row of dots.
const HAIR: f32 = 0.0036;

/// Oak: a short twig ending in a spray of lobed leaves, as oak leaves crowd
/// at a shoot's tip, and a pair lower down. Each leaf is obovate, narrow at
/// the base and broad past the middle, with four or five rounded lobes a
/// side cut by narrow sinuses.
#[must_use]
pub fn oak_leaves() -> MaterialGraph {
    let greens = [
        [0.026, 0.058, 0.018],
        [0.042, 0.088, 0.022],
        [0.062, 0.112, 0.028],
    ];
    let form = LeafForm {
        width: 0.34,
        base: 1.2,
        tip: 0.7,
        margin: Margin::Lobed {
            count: 4.5,
            depth: 0.58,
        },
        rib: [0.15, 0.19, 0.08],
        veins: 4.5,
    };
    let stem = LEAF_CARD_STEM;
    let twig = [stem, [0.485, 0.80], [0.51, 0.66], [0.50, 0.55]];
    let tip = twig[3];
    let mut rng = Rng(2101);
    let mut card = Card::new(leaves_key("oak"), [0.022, 0.045, 0.014]);
    // (where along the twig, angle, length, shade), back to front.
    let leaves: [(f32, f32, f32, f32); 9] = [
        (0.36, -98.0, 0.36, 0.7),
        (0.44, 94.0, 0.36, 0.72),
        (1.0, -70.0, 0.44, 0.8),
        (1.0, 66.0, 0.44, 0.82),
        (0.72, -104.0, 0.4, 0.86),
        (0.78, 100.0, 0.4, 0.88),
        (1.0, -34.0, 0.44, 0.95),
        (1.0, 30.0, 0.44, 1.0),
        (1.0, -4.0, 0.4, 1.05),
    ];
    card = card.twig("twig", &twig, [0.009, 0.006], TWIG);
    for (index, (s, angle, length, shade)) in leaves.into_iter().enumerate() {
        let at = if s >= 1.0 { tip } else { along(&twig, s).0 };
        let angle = angle + rng.range(-6.0, 6.0);
        let petiole = rng.range(0.02, 0.03);
        let blade = toward(at, angle, petiole);
        let yellow = (index == 5).then_some([0.17, 0.15, 0.03]);
        let color = leaf_color(&mut rng, greens, yellow, shade);
        card = card
            .stem(
                &format!("pet{index}"),
                at,
                blade,
                [0.0045, HAIR],
                [0.07, 0.065, 0.02],
            )
            .leaf(
                &format!("leaf{index}"),
                &Leaf {
                    at: blade,
                    angle,
                    length: length * rng.range(0.92, 1.04),
                    bend: rng.range(-0.08, 0.08),
                    color,
                },
                &form,
            );
    }
    card.finish()
}

/// Birch: a thin drooping shoot and two side shoots with small leaves on
/// short stalks, alternating down both sides. A birch leaf is broadest near
/// its base and drawn out to a point, with a fine toothed margin, and a
/// brighter, yellower green than the oak.
#[must_use]
pub fn birch_leaves() -> MaterialGraph {
    let greens = [
        [0.040, 0.085, 0.016],
        [0.068, 0.122, 0.022],
        [0.095, 0.150, 0.028],
    ];
    let form = LeafForm {
        width: 0.4,
        base: 0.5,
        tip: 1.25,
        margin: Margin::Serrate {
            count: 15.0,
            depth: 0.1,
        },
        rib: [0.19, 0.23, 0.09],
        veins: 7.0,
    };
    let stem = LEAF_CARD_STEM;
    let shoot = [stem, [0.47, 0.76], [0.46, 0.56], [0.51, 0.38], [0.54, 0.27]];
    let side = [[0.465, 0.68], [0.30, 0.56], [0.20, 0.40]];
    let side2 = [[0.48, 0.48], [0.64, 0.42], [0.76, 0.32]];
    let mut rng = Rng(2203);
    let mut card = Card::new(leaves_key("birch"), [0.03, 0.06, 0.012]);
    card = card
        .twig("shoot", &shoot, [0.006, 0.004], TWIG)
        .twig("side", &side, [0.0045, HAIR], TWIG)
        .twig("sidb", &side2, [0.0045, HAIR], TWIG);
    let mut index = 0;
    let mut place = |card: Card, line: &[[f32; 2]], stops: &[f32], length: f32, rng: &mut Rng| {
        let mut card = card;
        for (k, s) in stops.iter().enumerate() {
            let (at, heading) = along(line, *s);
            let side = if k % 2 == 0 { -1.0 } else { 1.0 };
            let angle = if *s >= 1.0 {
                heading + rng.range(-8.0, 8.0)
            } else {
                heading + side * rng.range(45.0, 70.0)
            };
            let petiole = rng.range(0.03, 0.045);
            let blade = toward(at, angle, petiole);
            let shade = rng.range(0.8, 1.05);
            let yellow = (index == 9).then_some([0.20, 0.17, 0.03]);
            let color = leaf_color(rng, greens, yellow, shade);
            card = card
                .stem(
                    &format!("pet{index}"),
                    at,
                    blade,
                    [HAIR, HAIR],
                    [0.075, 0.07, 0.025],
                )
                .leaf(
                    &format!("leaf{index}"),
                    &Leaf {
                        at: blade,
                        angle,
                        length: length * rng.range(0.88, 1.05),
                        bend: rng.range(-0.1, 0.1),
                        color,
                    },
                    &form,
                );
            index += 1;
        }
        card
    };
    card = place(card, &side, &[0.3, 0.55, 0.8, 1.0], 0.29, &mut rng);
    card = place(card, &side2, &[0.3, 0.55, 0.8, 1.0], 0.29, &mut rng);
    card = place(
        card,
        &shoot,
        &[0.16, 0.28, 0.4, 0.52, 0.64, 0.76, 0.88, 1.0],
        0.3,
        &mut rng,
    );
    card.finish()
}

/// Shrub: a clump of small round leaves on five short stems spreading from
/// the base, crowded towards their ends.
#[must_use]
pub fn shrub_leaves() -> MaterialGraph {
    let greens = [
        [0.026, 0.066, 0.024],
        [0.044, 0.096, 0.030],
        [0.066, 0.120, 0.032],
    ];
    let form = LeafForm {
        width: 0.44,
        base: 0.7,
        tip: 0.8,
        margin: Margin::Entire,
        rib: [0.13, 0.18, 0.08],
        veins: 0.0,
    };
    let stem = LEAF_CARD_STEM;
    let stems: [[[f32; 2]; 3]; 5] = [
        [stem, [0.40, 0.72], [0.16, 0.52]],
        [stem, [0.44, 0.64], [0.28, 0.28]],
        [stem, [0.50, 0.62], [0.52, 0.22]],
        [stem, [0.56, 0.64], [0.72, 0.28]],
        [stem, [0.60, 0.72], [0.84, 0.52]],
    ];
    let mut rng = Rng(2301);
    let mut card = Card::new(leaves_key("shrub"), [0.022, 0.045, 0.016]);
    for (k, line) in stems.iter().enumerate() {
        card = card.twig(&format!("stem{k}"), line, [0.0055, HAIR], TWIG);
    }
    let mut index = 0;
    let stops = [0.35, 0.47, 0.58, 0.68, 0.78, 0.88, 1.0];
    // Round the stems in turn, so the leaves of neighbouring stems overlap
    // both ways rather than one stem lying wholly over the next.
    for (j, s) in stops.iter().enumerate() {
        for (k, line) in stems.iter().enumerate() {
            let (at, heading) = along(line, *s);
            let side = if (j + k) % 2 == 0 { -1.0 } else { 1.0 };
            let angle = if *s >= 1.0 {
                heading + rng.range(-10.0, 10.0)
            } else {
                heading + side * rng.range(40.0, 75.0)
            };
            let blade = toward(at, angle, 0.012);
            let shade = rng.range(0.78, 1.05) * (0.85 + 0.15 * s);
            let yellow = (index == 12 || index == 27).then_some([0.16, 0.15, 0.03]);
            let color = leaf_color(&mut rng, greens, yellow, shade);
            card = card.leaf(
                &format!("leaf{index}"),
                &Leaf {
                    at: blade,
                    angle,
                    length: rng.range(0.15, 0.18),
                    bend: rng.range(-0.06, 0.06),
                    color,
                },
                &form,
            );
            index += 1;
        }
    }
    card.finish()
}

/// Fern: one whole frond, a rachis from the stem up the card with pinnae
/// alternating off it, longest a third of the way up and tapering to the tip
/// so the frond is a narrow lance, each pinna cut into small rounded lobes,
/// in a bright, slightly yellow green.
///
/// A whole frond rather than a section of one, so the rachis ends inside the
/// card like every other set's twig: a frond continued into the next card
/// would have to reach the top edge, which is exactly the margin a mip chain
/// needs clear, and would show a seam where two cards' pinnae meet.
#[must_use]
pub fn fern_leaves() -> MaterialGraph {
    /// Pinnae a side.
    const PAIRS: u8 = 11;
    let greens = [
        [0.080, 0.150, 0.024],
        [0.115, 0.185, 0.030],
        [0.150, 0.210, 0.036],
    ];
    let form = LeafForm {
        width: 0.15,
        base: 0.5,
        tip: 0.9,
        margin: Margin::Lobed {
            count: 8.0,
            depth: 0.45,
        },
        rib: [0.20, 0.26, 0.09],
        veins: 0.0,
    };
    let stem = LEAF_CARD_STEM;
    let top = [0.5, 0.06];
    let rachis = [stem, [0.495, 0.5], top];
    let mut rng = Rng(2501);
    let mut card = Card::new(leaves_key("fern"), [0.035, 0.07, 0.014]);
    card = card.twig("rachis", &rachis, [0.0055, HAIR], [0.075, 0.085, 0.03]);
    // Bottom to top, so the upper pinnae lie over the ones below them, as
    // they do on a frond seen from above.
    for pair in 0..PAIRS {
        let s = 0.1 + 0.86 * f32::from(pair) / f32::from(PAIRS - 1);
        // Longest a third of the way up, falling to a stub at the tip.
        let reach = if s < 0.33 {
            0.72 + 0.28 * s / 0.33
        } else {
            1.0 - 0.82 * ((s - 0.33) / 0.67).powf(1.2)
        };
        for side in [-1.0_f32, 1.0] {
            // Alternate: the right pinna of a pair sits a half step higher.
            let (at, heading) = along(&rachis, (s + 0.03 * (side + 1.0) * 0.5).min(1.0));
            let angle = heading + side * rng.range(68.0, 76.0);
            let shade = rng.range(0.85, 1.05) * (0.8 + 0.2 * s);
            let color = leaf_color(&mut rng, greens, None, shade);
            card = card.leaf(
                &format!("pinna{pair}_{}", u8::from(side > 0.0)),
                &Leaf {
                    at,
                    angle,
                    length: 0.46 * reach * rng.range(0.95, 1.05),
                    // Curled a little up towards the tip of the frond.
                    bend: -side * rng.range(0.02, 0.05),
                    color,
                },
                &form,
            );
        }
    }
    card.finish()
}

/// Pine: a shoot with side shoots branching up it, each a brush of long
/// needles raked forward and curving, round a brown bud at its tip,
/// blue-green, darker at the base and paler at the tips.
///
/// Every shoot is painted twice: a back rank raked out wide and darker, then
/// the shoot itself, then a front rank swept close along it, so needles cross
/// the shoot and each other the way a pine's bundles do rather than lying in
/// a comb.
#[must_use]
pub fn pine_leaves() -> MaterialGraph {
    let dark = [0.020, 0.046, 0.030];
    let light = [0.052, 0.100, 0.048];
    let stem = LEAF_CARD_STEM;
    let main = 0.78;
    // (where along the main shoot, angle, length), back to front, and the
    // main shoot itself last.
    let shoots: [(f32, f32, f32); 7] = [
        (0.1, -64.0, 0.32),
        (0.18, 62.0, 0.32),
        (0.32, -52.0, 0.36),
        (0.4, 50.0, 0.36),
        (0.54, -38.0, 0.34),
        (0.62, 36.0, 0.3),
        (0.0, 0.0, main),
    ];
    let back = Needles {
        length: 0.17,
        taper: 0.7,
        pitch: 0.02,
        rake: 0.8,
        curve: 1.5,
        width: 0.0042,
        start: 0.12,
        ragged: 0.6,
        colors: [dark, light],
    };
    let middle = Needles {
        rake: 1.25,
        curve: 1.1,
        pitch: 0.019,
        length: 0.18,
        colors: [[0.024, 0.053, 0.033], light],
        ..back
    };
    let front = Needles {
        rake: 1.8,
        curve: 0.8,
        pitch: 0.017,
        width: 0.0037,
        colors: [[0.028, 0.060, 0.036], light],
        ..back
    };
    let mut rng = Rng(2401);
    let mut card = Card::new(leaves_key("pine"), [0.018, 0.04, 0.026]);
    for (k, (s, angle, length)) in shoots.into_iter().enumerate() {
        let origin = toward(stem, 0.0, s * main);
        let tip = toward(origin, angle, length);
        let shade = rng.range(0.82, 1.12);
        let tint = |form: &Needles| Needles {
            colors: [form.colors[0].map(|c| c * shade), form.colors[1]],
            ..*form
        };
        card = card
            .needles(&format!("back{k}"), origin, angle, length, &tint(&back))
            .needles(&format!("mid{k}"), origin, angle, length, &tint(&middle))
            .stem(&format!("shoot{k}"), origin, tip, [0.008, 0.005], TWIG)
            .needles(&format!("front{k}"), origin, angle, length, &tint(&front))
            .stem(
                &format!("bud{k}"),
                toward(origin, angle, length * 0.98),
                toward(origin, angle, length + 0.025),
                [0.009, 0.004],
                [0.13, 0.075, 0.04],
            );
    }
    card.finish()
}

/// Fir: a flat frond, a central shoot with side shoots alternating off it,
/// each combed with short flat needles either side and tipped with paler new
/// growth.
#[must_use]
pub fn fir_leaves() -> MaterialGraph {
    let dark = [0.018, 0.044, 0.028];
    let light = [0.070, 0.135, 0.045];
    let stem = LEAF_CARD_STEM;
    let main_len = 0.82;
    let needles = Needles {
        length: 0.105,
        taper: 0.6,
        pitch: 0.011,
        rake: 0.35,
        curve: 1.0,
        width: 0.0068,
        start: 0.0,
        ragged: 0.8,
        colors: [dark, light],
    };
    let mut card = Card::new(leaves_key("fir"), [0.018, 0.04, 0.024]);
    // (where along the main shoot, angle, length, shade), back to front.
    let sides: [(f32, f32, f32, f32); 8] = [
        (0.12, -64.0, 0.42, 0.8),
        (0.19, 62.0, 0.44, 0.85),
        (0.29, -58.0, 0.42, 0.9),
        (0.39, 56.0, 0.40, 0.95),
        (0.49, -52.0, 0.36, 1.0),
        (0.59, 50.0, 0.32, 1.0),
        (0.68, -46.0, 0.28, 1.05),
        (0.77, 42.0, 0.22, 1.05),
    ];
    for (k, (s, angle, length, shade)) in sides.into_iter().enumerate() {
        let at = toward(stem, 0.0, s * main_len);
        let form = Needles {
            colors: [dark.map(|c| c * shade), light],
            ..needles
        };
        card = card
            .needles(&format!("side{k}"), at, angle, length, &form)
            .stem(
                &format!("sidestem{k}"),
                at,
                toward(at, angle, length * 0.97),
                [0.0045, HAIR],
                [0.07, 0.06, 0.03],
            );
    }
    card = card
        .needles(
            "main",
            stem,
            0.0,
            main_len,
            &Needles {
                length: 0.11,
                ..needles
            },
        )
        .stem(
            "mainstem",
            stem,
            toward(stem, 0.0, main_len * 0.98),
            [0.006, 0.004],
            [0.07, 0.06, 0.03],
        );
    card.finish()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, num::NonZeroUsize};

    use ashlar::{Alpha, Surface};
    use ashlar_material::{
        Exposure,
        bake::{BakeRequest, rasterise},
    };

    use super::*;
    use crate::{
        FLORA_BARK_RESOLUTION, FLORA_LEAF_CUTOFF, FLORA_LEAF_TRANSMISSION, FLORA_LEAVES, flora,
        graphs as all_graphs,
    };

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "every compared number is a copy of one literal, equal bit for bit"
    )]
    fn every_flora_definition_validates_against_its_graph() {
        let graphs = all_graphs();
        let library = flora();
        assert_eq!(
            library.materials.len(),
            FLORA_BARK.len() + FLORA_LEAVES.len()
        );
        let check = |key: &str, graph_key: &str| {
            let definition = library
                .materials
                .get(key)
                .unwrap_or_else(|| panic!("{key} is missing"));
            definition
                .check(&format!("materials[{key}]"))
                .unwrap_or_else(|error| panic!("{key}: {error}"));
            let Surface::Graph(bake) = &definition.surface else {
                panic!("{key} is not a graph bake");
            };
            assert_eq!(bake.graph, graph_key, "{key}");
            assert!(bake.params.is_empty(), "{key} binds parameters");
            let graph = graphs
                .get(&bake.graph)
                .unwrap_or_else(|| panic!("{key} names a graph the library lacks"));
            assert!(
                graph
                    .params
                    .iter()
                    .all(|param| param.exposure != Exposure::Live),
                "{key} has a live parameter"
            );
            assert!(graph.output.height.is_some(), "{key} binds no height");
            assert!(
                graph.output.normal_strength > 0.0,
                "{key} would bake a flat normal"
            );
            assert_eq!(
                graph.tile_metres,
                Some(definition.tile_metres),
                "{key} was drawn for another repeat"
            );
            (
                definition.clone(),
                bake.resolution,
                graph.output.opacity.is_some(),
            )
        };
        for (set, metres) in FLORA_BARK {
            let (definition, resolution, opacity) =
                check(&format!("flora:bark-{set}"), &bark_key(set));
            assert_eq!(definition.tile_metres, metres, "{set}");
            assert_eq!(resolution, FLORA_BARK_RESOLUTION, "{set}");
            assert_eq!(definition.alpha, Alpha::Opaque, "{set}");
            assert!(!definition.double_sided, "{set}");
            assert_eq!(definition.diffuse_transmission, 0.0, "{set}");
            assert!(!opacity, "bark {set} binds an opacity");
        }
        for set in FLORA_LEAVES {
            let (definition, resolution, opacity) =
                check(&format!("flora:leaves-{set}"), &leaves_key(set));
            assert_eq!(definition.tile_metres, [1.0, 1.0], "{set}");
            assert_eq!(resolution, FLORA_LEAF_RESOLUTION, "{set}");
            assert_eq!(
                definition.alpha,
                Alpha::Mask {
                    cutoff: FLORA_LEAF_CUTOFF
                },
                "{set}"
            );
            assert!(definition.double_sided, "{set}");
            assert!(
                (0.35..=0.5).contains(&definition.diffuse_transmission),
                "{set}"
            );
            assert_eq!(definition.diffuse_transmission, FLORA_LEAF_TRANSMISSION);
            assert!(opacity, "leaves {set} bind no opacity");
        }
    }

    #[test]
    fn every_flora_graph_builds_clean_and_tiles_at_its_resolution() {
        let graphs = all_graphs();
        let keys = FLORA_BARK
            .iter()
            .map(|(set, _)| (bark_key(set), FLORA_BARK_RESOLUTION))
            .chain(
                FLORA_LEAVES
                    .iter()
                    .map(|set| (leaves_key(set), FLORA_LEAF_RESOLUTION)),
            );
        for (key, resolution) in keys {
            let material = graphs
                .build(&key)
                .unwrap_or_else(|error| panic!("{key}: {error}"));
            assert!(
                material.warnings().is_empty(),
                "{key}: {:?}",
                material.warnings()
            );
            let repeats = material
                .period()
                .repeats()
                .unwrap_or_else(|| panic!("{key} does not tile"));
            assert!(
                repeats.iter().all(|r| r.is_power_of_two()),
                "{key} repeats {repeats:?}"
            );
            let lattice = material.finest_lattice();
            assert!(
                lattice.iter().all(|&l| l <= resolution),
                "{key} lays a {lattice:?} lattice"
            );
        }
    }

    /// Baked at half the shipped resolution: the silhouette barely moves and
    /// the test stays quick in a debug build.
    #[test]
    fn every_leaf_card_covers_about_half_and_keeps_off_its_edges() {
        let graphs = all_graphs();
        let resolution = 256;
        for set in FLORA_LEAVES {
            let key = leaves_key(set);
            let graph = graphs.get(&key).expect("every set has a graph");
            let (planes, report) = rasterise(&BakeRequest {
                graph,
                library: &graphs,
                params: &BTreeMap::new(),
                resolution,
                mips: false,
                threads: NonZeroUsize::new(4),
            })
            .unwrap_or_else(|error| panic!("{key}: {error}"));
            let opacity = planes.opacity.expect("a leaf card binds an opacity");
            let coverage = ashlar_material::mips::coverage(&opacity);
            assert!(
                (0.3..=0.7).contains(&coverage),
                "{key} covers {coverage} of its card"
            );
            let (low, high) = report.height_range.expect("a leaf card binds a height");
            assert!(
                low >= 0.0 && high <= 1.0,
                "{key} clips its height: {low}..{high}"
            );
            // The outermost 2% of the card on every side.
            let n = resolution as usize;
            let border = n.div_ceil(50);
            for y in 0..n {
                for x in 0..n {
                    let edge = x < border || y < border || x >= n - border || y >= n - border;
                    if edge {
                        assert!(
                            opacity[y * n + x] <= 0.0,
                            "{key} reaches its border at {x}, {y}: {}",
                            opacity[y * n + x]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_bark_height_stays_in_the_unit() {
        let graphs = all_graphs();
        for (set, _) in FLORA_BARK {
            let key = bark_key(set);
            let graph = graphs.get(&key).expect("every set has a graph");
            // A quarter of the shipped resolution: the extremes a lattice
            // reaches barely move, and the test stays fast in a debug build.
            let (_, report) = rasterise(&BakeRequest {
                graph,
                library: &graphs,
                params: &BTreeMap::new(),
                resolution: FLORA_BARK_RESOLUTION / 4,
                mips: false,
                threads: NonZeroUsize::new(4),
            })
            .unwrap_or_else(|error| panic!("{key}: {error}"));
            let (low, high) = report.height_range.expect("bark binds a height");
            assert!(
                low >= 0.0 && high <= 1.0,
                "{key} clips its height: {low}..{high}"
            );
            assert!(high - low > 0.15, "{key} is nearly flat: {low}..{high}");
        }
    }
}
