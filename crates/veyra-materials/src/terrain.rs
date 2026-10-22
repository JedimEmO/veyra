//! Veyra's terrain layers: six wild ground graphs, three for the city's
//! ground ([`setts`], [`flagstones`], [`soil`]) and five provinces' bare rock
//! faces ([`granite`], [`sandstone`], [`basalt`], [`slate`], [`clay`]), drawn
//! for a painterly shader.
//!
//! The game is stylised rather than photographic, so every layer here is
//! built out of shapes a viewer can name at a glance (tufts, leaves, pebbles,
//! plates, ripples, drifts) and keeps fine noise to a whisper: a soft shader
//! smears high-frequency grain into mush, while a clump or a plate survives
//! it. Broad light and dark stays under about a third of the repeat, because
//! the terrain shader hides the lattice with hex tiling and a macro colour of
//! its own, and a blotch the size of the repeat is exactly what would show
//! that lattice from the air.
//!
//! The shader blends two layers by their heights and packs each height into
//! its normal map's alpha, so the heights mean the same thing in every layer:
//! gravel stones and rock plates stand high, grass tufts in the middle, and
//! sand and snow lie low and fill, so a stone poking through snow or a tuft
//! out of sand is decided by the relief rather than painted.
use ashlar_material::{
    Channel, Input, MaterialGraph, MaterialGraphBuilder, MathOp, Node, PbrOutput,
    nodes::{
        Blur, Clamp, Colorize, Combine2, Curvature, Curve, Decompose, Levels, Math, Mix, Noise,
        OcclusionFromHeight, Scratches, Uv, Voronoi, VoronoiOutput, Warp,
    },
};

use crate::TERRAIN;

mod city;
mod faces;

pub use city::{FLAGSTONES_LAYOUT, flagstones, setts, soil};
pub use faces::{basalt, clay, granite, sandstone, slate};

fn m(op: MathOp, a: impl Into<Input>, b: impl Into<Input>) -> Math {
    Math::new(op, a, b)
}

fn remap(input: impl Into<Input>, low: f32, high: f32) -> Levels {
    Levels::new(input).out_range(low, high)
}

fn ramp(input: impl Into<Input>, from: f32, to: f32) -> Levels {
    Levels::new(input).in_range(from, to)
}

/// A second, independent number off a cell's one random float.
fn hash(id: impl Into<Input>, salt: f32) -> Math {
    Math::unary(MathOp::Fract, m(MathOp::Mul, id, salt))
}

/// The repeat a set was authored against, read off the table so the graph and
/// the definition cannot disagree.
fn repeat(set: &str) -> f32 {
    TERRAIN
        .iter()
        .find_map(|(name, metres)| (*name == set).then_some(*metres))
        .unwrap_or_else(|| unreachable!("{set} is a terrain set"))
}

/// One population of Voronoi cells with everything a shape drawn per cell
/// needs: `{prefix}_id`, `_edge`, `_dist`, `_off` (the offset to the cell
/// point in UV, for gathering a field there), `_dx`, `_dy` (the same in cell
/// units), and `_x`, `_y`, that offset turned by a per-cell angle so no two
/// leaves or pebbles lie the same way. Every output reads the lattice through
/// the same `wobble` displacement, so outlines bend without the pieces coming
/// apart.
fn cells(
    g: MaterialGraphBuilder,
    prefix: &str,
    periods: [u16; 2],
    seed: u32,
    jitter: f32,
    wobble: Option<(&str, f32)>,
) -> MaterialGraphBuilder {
    use MathOp::{Add, Cos, Mul, Sin, Sub};
    let n = |s: &str| format!("{prefix}_{s}");
    let lattice = Voronoi::new()
        .periods(u32::from(periods[0]), u32::from(periods[1]))
        .jitter(jitter)
        .seed(seed);
    let read = |output: VoronoiOutput| -> Node {
        match wobble {
            Some((offset, amount)) => Warp::new(lattice.output(output), offset)
                .amount(amount)
                .into(),
            None => lattice.output(output).into(),
        }
    };
    g.node(n("id"), read(VoronoiOutput::Cell))
        .node(n("edge"), read(VoronoiOutput::Edge))
        .node(n("dist"), read(VoronoiOutput::Distance))
        .node(n("off"), read(VoronoiOutput::Offset))
        .node(
            n("dx"),
            m(
                Mul,
                Decompose::new(n("off"), Channel::R),
                f32::from(periods[0]),
            ),
        )
        .node(
            n("dy"),
            m(
                Mul,
                Decompose::new(n("off"), Channel::G),
                f32::from(periods[1]),
            ),
        )
        .node(n("spin"), hash(n("id"), 91.7))
        .node(n("cos"), Math::unary(Cos, n("spin")))
        .node(n("sin"), Math::unary(Sin, n("spin")))
        .node(
            n("x"),
            m(Add, m(Mul, n("dx"), n("cos")), m(Mul, n("dy"), n("sin"))),
        )
        .node(
            n("y"),
            m(Sub, m(Mul, n("dy"), n("cos")), m(Mul, n("dx"), n("sin"))),
        )
}

/// A two-axis displacement in `-1..=1` from two octaves of Perlin at one
/// period, for [`cells`] and anything else that should not be drawn with a
/// ruler.
fn wobble(g: MaterialGraphBuilder, id: &str, period: u32, seed: u32) -> MaterialGraphBuilder {
    g.node(
        format!("{id}_u"),
        remap(
            Noise::perlin().period(period).octaves(2).seed(seed),
            -1.0,
            1.0,
        ),
    )
    .node(
        format!("{id}_v"),
        remap(
            Noise::perlin().period(period).octaves(2).seed(seed + 1),
            -1.0,
            1.0,
        ),
    )
    .node(id, Combine2::new(format!("{id}_u"), format!("{id}_v")))
}

/// Stack layers of pieces: whichever stands highest owns the texel's colour
/// and roughness. Leaves `{prefix}_top`, `_color` and `_rough`.
fn stack(
    mut g: MaterialGraphBuilder,
    prefix: &str,
    layers: &[(String, String, String)],
) -> MaterialGraphBuilder {
    use MathOp::{Max, Step};
    let (mut top, mut color, mut rough) = layers[0].clone();
    for (index, (height, layer_color, layer_rough)) in layers.iter().enumerate().skip(1) {
        let take = format!("{prefix}_take{index}");
        let next = (
            format!("{prefix}_top{index}"),
            format!("{prefix}_color{index}"),
            format!("{prefix}_rough{index}"),
        );
        g = g
            .node(take.clone(), m(Step, height.as_str(), top.as_str()))
            .node(next.0.clone(), m(Max, top.as_str(), height.as_str()))
            .node(
                next.1.clone(),
                Mix::new(color.as_str(), layer_color.as_str(), take.as_str()),
            )
            .node(
                next.2.clone(),
                Mix::new(rough.as_str(), layer_rough.as_str(), take.as_str()),
            );
        (top, color, rough) = next;
    }
    g.node(format!("{prefix}_top"), Clamp::new(top.as_str()))
        .node(format!("{prefix}_color"), Clamp::new(color.as_str()))
        .node(format!("{prefix}_rough"), Clamp::new(rough.as_str()))
}

/// Bind the height every layer must carry, derive occlusion from it, and
/// declare the repeat. `relief` is the physical depth of the `0..=1` height
/// in metres, so `relief / tile` is the UV-scale normal strength.
fn finish(
    g: MaterialGraphBuilder,
    output: PbrOutput,
    tile: f32,
    relief: f32,
    ao_radius: f32,
) -> MaterialGraph {
    let strength = relief / tile;
    g.node("physical_height", m(MathOp::Mul, "height", strength))
        .node(
            "ao",
            OcclusionFromHeight::new("physical_height").radius(ao_radius),
        )
        .tile_metres([tile; 2])
        .output(
            output
                .height("height")
                .metallic(0.0)
                .normal_strength(strength),
        )
        .into_graph()
}

/// Every terrain graph, keyed `veyra:terrain-<set>`.
pub(crate) fn graphs() -> [MaterialGraph; 14] {
    [
        basalt(),
        clay(),
        flagstones(),
        forest_floor(),
        grass(),
        granite(),
        gravel(),
        rock(),
        sand(),
        sandstone(),
        setts(),
        slate(),
        snow(),
        soil(),
    ]
}

/// The key a set's graph is registered under.
#[must_use]
pub fn graph_key(set: &str) -> String {
    format!("veyra:terrain-{}", set.replace('_', "-"))
}

/// Meadow ground: bold painted blades, dark at the root and light at the tip,
/// leaning this way and that in clumps that each carry their own warm or cool
/// green, with darker ground between clumps and a few straw-coloured dry
/// patches. 50 mm of relief over an 8 m repeat.
///
/// A blade is a tapered wedge about its Voronoi point in a lattice twice as
/// tall as it is wide, so a blade fits its own cell and is almost never cut by
/// a neighbour's edge. Six such populations are stacked by height, so blades
/// overlap rather than blend. Each blade reads its clump at its own point (a
/// warp by its cell offset gathers the clump field there), which is what
/// gives a clump one green and thins the blades towards its edge. The 3D
/// grass the game grows stands over this, so it has to read as a lawn at
/// distance and as grass underfoot.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn grass() -> MaterialGraph {
    use MathOp::{Abs, Add, Div, Mul, Step, Sub};
    let tile = repeat("grass");
    let g = MaterialGraph::builder(graph_key("grass"))
        .node("mound", Noise::perlin().period(8).octaves(2).seed(401))
        .node("tone", Noise::perlin().period(4).octaves(3).seed(403))
        .node("dry_field", Noise::perlin().period(8).octaves(2).seed(405))
        .node("ground", m(Add, 0.18, m(Mul, "mound", 0.1)))
        .node(
            "ground_color",
            m(Mul, [0.028, 0.050, 0.017], remap("mound", 0.85, 1.15)),
        )
        .node("ground_rough", Clamp::new(0.95));
    let g = wobble(g, "wobble", 16, 407);
    let g = cells(g, "clump", [16, 16], 409, 1.0, Some(("wobble", 0.01)))
        .node("clump_dome", Levels::new("clump_dist").in_range(0.95, 0.15));
    let mut g = g;
    // (prefix, seed, base height)
    let populations: [(&str, u32, f32); 6] = [
        ("blade_a", 411, 0.26),
        ("blade_b", 419, 0.29),
        ("blade_c", 421, 0.32),
        ("blade_d", 431, 0.35),
        ("blade_e", 433, 0.38),
        ("blade_f", 439, 0.41),
    ];
    let mut layers = vec![(
        "ground".to_owned(),
        "ground_color".to_owned(),
        "ground_rough".to_owned(),
    )];
    for (prefix, seed, base) in populations {
        let n = |s: &str| format!("{prefix}_{s}");
        let at_root = |field: &str| Warp::new(field, n("off")).amount(1.0);
        g = cells(g, prefix, [64, 32], seed, 1.0, Some(("wobble", 0.004)))
            .node(n("clump"), at_root("clump_dome"))
            .node(n("clump_id"), at_root("clump_id"))
            .node(n("t"), m(Div, m(Add, n("dy"), 0.42), 0.84))
            .node(
                n("across"),
                m(
                    Add,
                    n("dx"),
                    m(Mul, n("dy"), remap(hash(n("id"), 57.9), -0.6, 0.6)),
                ),
            )
            .node(
                n("width"),
                m(
                    Mul,
                    remap(hash(n("id"), 3.1), 0.3, 0.42),
                    Clamp::new(m(Sub, 1.0, n("t"))),
                ),
            )
            .node(
                n("inside"),
                m(Sub, 1.0, m(Div, Math::unary(Abs, n("across")), n("width"))),
            )
            .node(
                n("present"),
                m(
                    Step,
                    m(Add, 0.45, m(Mul, n("clump"), 0.6)),
                    hash(n("id"), 7.13),
                ),
            )
            .node(
                n("mask"),
                m(
                    Mul,
                    m(Mul, ramp(n("inside"), 0.0, 0.35), ramp(n("t"), 0.0, 0.3)),
                    // Past the tip the width is zero and the division above
                    // answers zero, which would fill the rest of the cell.
                    m(Mul, ramp(n("t"), 1.0, 0.97), n("present")),
                ),
            )
            .node(
                n("height"),
                m(
                    Mul,
                    n("mask"),
                    m(
                        Add,
                        m(Add, base, m(Mul, n("clump"), 0.2)),
                        m(
                            Add,
                            m(Mul, Clamp::new(n("t")), 0.06),
                            m(Mul, ramp(n("inside"), 0.0, 0.7), 0.05),
                        ),
                    ),
                ),
            )
            .node(
                n("hue"),
                m(
                    Mul,
                    Colorize::new(hash(n("clump_id"), 3.3)).gradient([
                        (0.0, [0.046, 0.115, 0.040]),
                        (0.45, [0.072, 0.140, 0.034]),
                        (0.8, [0.102, 0.150, 0.032]),
                        (1.0, [0.130, 0.150, 0.036]),
                    ]),
                    remap(hash(n("id"), 19.1), 0.85, 1.15),
                ),
            )
            .node(
                n("color"),
                Mix::new(
                    [0.030, 0.060, 0.020],
                    Mix::new(
                        n("hue"),
                        [0.20, 0.29, 0.07],
                        m(
                            Mul,
                            Levels::new(n("t")).in_range(0.35, 1.0).gamma(1.3),
                            0.65,
                        ),
                    ),
                    ramp(n("t"), 0.0, 0.4),
                ),
            )
            .node(n("rough"), remap(hash(n("id"), 13.3), 0.8, 0.9));
        layers.push((n("height"), n("color"), n("rough")));
    }
    let g = stack(g, "sward", &layers)
        .node("height", Clamp::new("sward_top"))
        .node("dry", ramp("dry_field", 0.65, 0.75))
        .node(
            "straw",
            Mix::new(
                [0.085, 0.072, 0.03],
                [0.30, 0.245, 0.10],
                ramp(m(Sub, "sward_top", "ground"), 0.0, 0.4),
            ),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new("sward_color", "straw", m(Mul, "dry", 0.45)),
                remap("tone", 0.9, 1.1),
            ),
        )
        .node("roughness", Clamp::new("sward_rough"));
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.05,
        0.004,
    )
}

/// Woodland floor: overlapping autumn leaves, each a pointed lens turned its
/// own way with a pale midrib, over dark soil, with fallen twigs, drifts of
/// needles and a few low moss patches. 40 mm of relief over a 6 m repeat.
///
/// Six leaf populations are stacked by height, so a leaf lies over or under
/// its neighbour rather than blending into it, and the lower layers are
/// darker, which is the depth of the litter read without a shadow map. A
/// drift field gathered at each leaf's point thins whole leaves out, so the
/// litter lies deep in places and leaves bare soil in others without a leaf
/// ever being cut in half by the field.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn forest_floor() -> MaterialGraph {
    use MathOp::{Abs, Add, Div, Max, Mul, Step, Sub};
    let tile = repeat("forest_floor");
    let g = MaterialGraph::builder(graph_key("forest_floor"))
        .node(
            "soil_noise",
            Noise::perlin().period(16).octaves(2).seed(501),
        )
        .node("tone", Noise::perlin().period(4).octaves(3).seed(503))
        .node(
            "moss_field",
            Noise::perlin().period(16).octaves(2).seed(505),
        )
        .node(
            "moss_lumps",
            Noise::perlin().period(128).octaves(1).seed(506),
        )
        .node(
            "needle_field",
            Noise::perlin().period(8).octaves(2).seed(507),
        )
        .node("moss", ramp("moss_field", 0.64, 0.7))
        // Where the litter has drifted deep and where the soil is bare.
        .node(
            "drift",
            remap(Noise::perlin().period(8).octaves(2).seed(508), 0.3, 1.3),
        )
        .node(
            "bed",
            m(
                Add,
                m(Add, 0.18, m(Mul, "soil_noise", 0.08)),
                m(Mul, "moss", remap("moss_lumps", 0.06, 0.16)),
            ),
        )
        .node(
            "bed_color",
            Mix::new(
                m(Mul, [0.045, 0.030, 0.019], remap("soil_noise", 0.8, 1.2)),
                m(Mul, [0.050, 0.080, 0.020], remap("moss_lumps", 0.7, 1.3)),
                "moss",
            ),
        );
    let g = wobble(g, "wobble", 16, 509);
    let mut g = g;
    // (prefix, period, seed, presence, base height, shade)
    let leaves: [(&str, u16, u32, f32, f32, f32); 6] = [
        ("under", 64, 510, 0.9, 0.2, 0.55),
        ("low", 32, 511, 0.9, 0.24, 0.62),
        ("mid", 32, 513, 0.85, 0.30, 0.74),
        ("small", 64, 517, 0.8, 0.33, 0.85),
        ("high", 32, 523, 0.8, 0.37, 0.92),
        ("top", 32, 529, 0.6, 0.42, 1.0),
    ];
    let mut layers = vec![(
        "bed".to_owned(),
        "bed_color".to_owned(),
        "bed_rough".to_owned(),
    )];
    g = g.node("bed_rough", Clamp::new(0.95));
    for (prefix, period, seed, presence, base, shade) in leaves {
        let n = |s: &str| format!("{prefix}_{s}");
        g = cells(g, prefix, [period; 2], seed, 1.0, Some(("wobble", 0.004)))
            .node(n("a"), remap(hash(n("id"), 23.7), 0.34, 0.48))
            .node(
                n("b"),
                m(Mul, n("a"), remap(hash(n("id"), 41.3), 0.4, 0.58)),
            )
            .node(n("u"), m(Div, n("x"), n("a")))
            .node(
                n("lens"),
                m(
                    Sub,
                    m(Sub, 1.0, m(Mul, n("u"), n("u"))),
                    m(Div, Math::unary(Abs, n("y")), n("b")),
                ),
            )
            .node(
                n("present"),
                m(
                    Step,
                    m(Mul, presence, Warp::new("drift", n("off")).amount(1.0)),
                    hash(n("id"), 7.13),
                ),
            )
            .node(n("mask"), m(Mul, ramp(n("lens"), 0.0, 0.05), n("present")))
            .node(
                n("curl"),
                Levels::new(n("lens")).in_range(0.0, 0.6).gamma(0.6),
            )
            .node(
                n("rib"),
                m(
                    Mul,
                    ramp(Math::unary(Abs, n("y")), 0.03, 0.0),
                    ramp(Math::unary(Abs, n("u")), 0.95, 0.6),
                ),
            )
            .node(
                n("height"),
                m(
                    Mul,
                    n("mask"),
                    m(
                        Add,
                        m(Add, base, m(Mul, hash(n("id"), 57.9), 0.14)),
                        m(Add, m(Mul, n("curl"), 0.08), m(Mul, n("rib"), 0.02)),
                    ),
                ),
            )
            .node(
                n("color"),
                m(
                    Mul,
                    Mix::new(
                        Colorize::new(hash(n("id"), 13.37)).gradient([
                            (0.0, [0.070, 0.035, 0.016]),
                            (0.3, [0.150, 0.055, 0.018]),
                            (0.55, [0.240, 0.090, 0.022]),
                            (0.8, [0.260, 0.150, 0.040]),
                            (1.0, [0.110, 0.095, 0.035]),
                        ]),
                        [0.30, 0.22, 0.10],
                        m(Mul, n("rib"), 0.5),
                    ),
                    m(Mul, remap(n("curl"), 0.75, 1.05), shade),
                ),
            )
            .node(n("rough"), remap(hash(n("id"), 3.1), 0.7, 0.85));
        layers.push((n("height"), n("color"), n("rough")));
    }
    let g = stack(g, "litter", &layers)
        .node(
            "twigs",
            Scratches::new()
                .count(24)
                .length(0.07)
                .width(0.0045)
                .seed(531),
        )
        .node(
            "needles",
            m(
                Mul,
                Scratches::new()
                    .count(4096)
                    .length(0.011)
                    .width(0.0018)
                    .seed(533),
                ramp("needle_field", 0.52, 0.64),
            ),
        )
        .node("twig_height", m(Mul, "twigs", m(Add, "litter_top", 0.12)))
        .node(
            "needle_height",
            m(Mul, "needles", m(Add, "litter_top", 0.04)),
        )
        .node(
            "height",
            Clamp::new(m(Max, "litter_top", m(Max, "twig_height", "needle_height"))),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    Mix::new(
                        "litter_color",
                        [0.19, 0.085, 0.028],
                        ramp("needles", 0.3, 0.8),
                    ),
                    [0.14, 0.09, 0.05],
                    ramp("twigs", 0.3, 0.8),
                ),
                remap("tone", 0.88, 1.12),
            ),
        )
        .node(
            "roughness",
            Mix::new("litter_rough", 0.9, ramp("twigs", 0.3, 0.8)),
        );
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.04,
        0.006,
    )
}

/// Loose gravel: rounded pebbles of three sizes bedded in dark soil, each an
/// ellipse turned its own way with a lit crown and a dusty foot. 60 mm of
/// relief over a 6 m repeat, the stones standing near the top of the height
/// so they win a blend against sand, snow or grass.
///
/// A pebble is an ellipse about its Voronoi point, capped by the cell's edge
/// distance so a stone never runs into its neighbour with a wall, and sunk
/// into the bed by a per-stone amount, so the three populations interlock
/// rather than lying on a floor.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn gravel() -> MaterialGraph {
    use MathOp::{Add, Div, Min, Mul, Sqrt, Step, Sub};
    let tile = repeat("gravel");
    let g = MaterialGraph::builder(graph_key("gravel"))
        .node(
            "soil_noise",
            Noise::perlin().period(16).octaves(2).seed(601),
        )
        .node("tone", Noise::perlin().period(4).octaves(3).seed(603))
        .node("bed", m(Add, 0.16, m(Mul, "soil_noise", 0.1)))
        .node(
            "bed_color",
            m(Mul, [0.075, 0.062, 0.046], remap("soil_noise", 0.8, 1.2)),
        )
        .node("bed_rough", Clamp::new(0.95))
        // Where the stones lie thick and where they have been scuffed thin.
        .node(
            "cluster",
            remap(Noise::perlin().period(8).octaves(2).seed(607), 0.55, 1.35),
        );
    let g = wobble(g, "wobble", 32, 605);
    let mut g = g;
    // (prefix, period, seed, presence, rise)
    let stones: [(&str, u16, u32, f32, f32); 3] = [
        ("large", 32, 611, 0.85, 0.7),
        ("medium", 64, 617, 0.6, 0.55),
        ("small", 128, 619, 0.25, 0.25),
    ];
    let mut layers = vec![(
        "bed".to_owned(),
        "bed_color".to_owned(),
        "bed_rough".to_owned(),
    )];
    for (prefix, period, seed, presence, rise) in stones {
        let n = |s: &str| format!("{prefix}_{s}");
        g = cells(g, prefix, [period; 2], seed, 0.75, Some(("wobble", 0.002)))
            .node(n("a"), remap(hash(n("id"), 23.7), 0.36, 0.47))
            .node(
                n("b"),
                m(Mul, n("a"), remap(hash(n("id"), 41.3), 0.65, 0.95)),
            )
            .node(
                n("r2"),
                m(
                    Add,
                    m(Mul, m(Div, n("x"), n("a")), m(Div, n("x"), n("a"))),
                    m(Mul, m(Div, n("y"), n("b")), m(Div, n("y"), n("b"))),
                ),
            )
            .node(
                n("dome"),
                m(
                    Min,
                    Math::unary(Sqrt, Clamp::new(m(Sub, 1.0, n("r2")))),
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
                    m(Mul, ramp(n("dome"), 0.0, 0.08), n("present")),
                    m(
                        Add,
                        "bed",
                        m(
                            Mul,
                            m(Sub, n("dome"), remap(hash(n("id"), 57.9), 0.0, 0.35)),
                            rise,
                        ),
                    ),
                ),
            )
            .node(
                n("color"),
                m(
                    Mul,
                    Colorize::new(hash(n("id"), 13.37)).gradient([
                        (0.0, [0.095, 0.092, 0.088]),
                        (0.25, [0.195, 0.195, 0.192]),
                        (0.5, [0.265, 0.235, 0.195]),
                        (0.7, [0.185, 0.200, 0.225]),
                        (0.85, [0.340, 0.285, 0.205]),
                        (1.0, [0.400, 0.385, 0.355]),
                    ]),
                    remap(n("dome"), 0.7, 1.12),
                ),
            )
            .node(n("rough"), remap(hash(n("id"), 3.1), 0.55, 0.75));
        layers.push((n("height"), n("color"), n("rough")));
    }
    let g = stack(g, "stones", &layers)
        .node("height", Clamp::new("stones_top"))
        // Soil dust on each stone's foot, where it meets the bed.
        .node("foot", ramp(m(Sub, "stones_top", "bed"), 0.08, 0.0))
        .node(
            "color",
            m(
                Mul,
                Mix::new("stones_color", "bed_color", m(Mul, "foot", 0.6)),
                remap("tone", 0.92, 1.08),
            ),
        )
        .node("roughness", Mix::new("stones_rough", 0.95, "foot"));
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.06,
        0.008,
    )
}

/// Bare rock: blocky slabs a metre long and half as wide, each tilted its
/// own way so neighbours meet in stepped ledges, chipped along their edges,
/// split by a few bold cracks and scored by faint bedding lines, with dark
/// cavities and rosettes of ochre and sage lichen on the tops. 300 mm of relief over an
/// 8 m repeat, the slabs standing high so rock wins its blends.
///
/// A slab's top is planar because it is the cell offset dotted with a
/// per-slab tilt, and the offset is linear across a cell. The edge only
/// chamfers down a little rather than to a floor, so two slabs of different
/// heights meet as a ledge rather than as two cobbles with a joint between.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn rock() -> MaterialGraph {
    use MathOp::{Add, Mul, Sin, Step, Sub};
    let tile = repeat("rock");
    let g = MaterialGraph::builder(graph_key("rock"))
        .node("swell", Noise::perlin().period(4).octaves(2).seed(701))
        .node("chip", Noise::perlin().period(64).octaves(2).seed(703))
        .node("tone", Noise::perlin().period(16).octaves(2).seed(705))
        .node(
            "lichen_region",
            Noise::perlin().period(8).octaves(2).seed(709),
        )
        .node("crack_gate", Noise::perlin().period(8).octaves(2).seed(711));
    let g = wobble(g, "wobble", 16, 713);
    let g = cells(g, "plate", [8, 16], 721, 0.9, Some(("wobble", 0.012)));
    let g = cells(g, "shard", [16, 16], 727, 1.0, Some(("wobble", 0.01)));
    let g = cells(g, "lichen", [32, 32], 731, 1.0, Some(("wobble", 0.004)))
        .node(
            "top",
            m(
                Add,
                m(Add, 0.5, m(Mul, hash("plate_id", 23.7), 0.34)),
                m(
                    Add,
                    m(Mul, "plate_dx", remap(hash("plate_id", 41.3), -0.2, 0.2)),
                    m(Mul, "plate_dy", remap(hash("plate_id", 57.9), -0.2, 0.2)),
                ),
            ),
        )
        .node(
            "bevel",
            Levels::new(m(Sub, "plate_edge", m(Mul, "chip", 0.06)))
                .in_range(0.0, 0.09)
                .gamma(0.6),
        )
        // Bedding scored across about half the slabs, each at its own angle.
        .node(
            "bedding",
            m(
                Mul,
                ramp(Math::unary(Sin, m(Mul, "plate_x", 4.0)), 0.85, 1.0),
                m(Step, hash("plate_id", 71.1), 0.5),
            ),
        )
        .node(
            "crack",
            m(
                Mul,
                ramp("shard_edge", 0.035, 0.0),
                ramp("crack_gate", 0.52, 0.62),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Sub,
                m(
                    Add,
                    m(Sub, "top", m(Mul, m(Sub, 1.0, "bevel"), 0.08)),
                    remap("swell", -0.06, 0.06),
                ),
                m(Add, m(Mul, "crack", 0.12), m(Mul, "bedding", 0.025)),
            )),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.006).strength(4.0),
        )
        .node(
            "peak",
            Curvature::peaks("height").radius(0.006).strength(6.0),
        )
        .node(
            "stone",
            m(
                Mul,
                Colorize::new(hash("plate_id", 13.37)).gradient([
                    (0.0, [0.125, 0.132, 0.145]),
                    (0.5, [0.165, 0.170, 0.182]),
                    (0.95, [0.205, 0.207, 0.215]),
                    (1.0, [0.215, 0.200, 0.180]),
                ]),
                remap("tone", 0.92, 1.08),
            ),
        )
        .node(
            "lichen",
            m(
                Mul,
                m(
                    Mul,
                    Levels::new(m(Mul, "lichen_dist", remap("chip", 0.7, 1.4))).in_range(0.5, 0.35),
                    m(
                        Step,
                        m(Mul, ramp("lichen_region", 0.45, 0.7), 0.7),
                        hash("lichen_id", 7.13),
                    ),
                ),
                m(Mul, ramp("bevel", 0.8, 1.0), m(Sub, 1.0, "crack")),
            ),
        )
        .node(
            "lichen_color",
            Mix::new(
                [0.30, 0.21, 0.055],
                [0.20, 0.22, 0.155],
                m(Step, hash("lichen_id", 3.3), 0.6),
            ),
        )
        .node(
            "color",
            Mix::new(
                m(
                    Mul,
                    m(Mul, "stone", remap("cavity", 1.0, 0.45)),
                    m(Mul, remap("peak", 1.0, 1.35), remap("bedding", 1.0, 0.9)),
                ),
                "lichen_color",
                m(Mul, "lichen", 0.9),
            ),
        )
        .node(
            "roughness",
            Mix::new(
                remap(hash("plate_id", 3.1), 0.62, 0.78),
                0.92,
                m(Add, "lichen", m(Mul, "cavity", 0.5)),
            ),
        );
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.3,
        0.01,
    )
}

/// Wind-rippled sand: soft asymmetric ripples that wander, fade and fork,
/// with a gentle swell under them and a scatter of pebbles. 30 mm of
/// relief over a 12 m repeat, lying low so almost anything wins a blend over
/// it.
///
/// A ripple is a phase down the repeat: `v` times the ripple count, plus three
/// scales of noise, through a curve with a long windward slope and a short
/// lee. The count is whole, so the phase meets itself at the seam.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn sand() -> MaterialGraph {
    use MathOp::{Add, Div, Fract, Max, Mul, Step, Sub};
    let tile = repeat("sand");
    let g = MaterialGraph::builder(graph_key("sand"))
        .node("swell", Noise::perlin().period(4).octaves(2).seed(801))
        .node("fade", Noise::perlin().period(8).octaves(2).seed(803))
        .node("tone", Noise::perlin().period(4).octaves(3).seed(805))
        .node("grain", Noise::perlin().period(128).octaves(1).seed(807))
        .node("warmth", Noise::perlin().period(8).octaves(2).seed(813))
        .node("scatter", Noise::perlin().period(8).octaves(2).seed(815))
        .node("v", Decompose::new(Uv::new(), Channel::G))
        .node(
            "phase",
            m(
                Add,
                m(Mul, "v", 48.0),
                m(
                    Add,
                    remap(Noise::perlin().period(4).octaves(2).seed(809), -3.0, 3.0),
                    m(
                        Add,
                        remap(Noise::perlin().period(8).octaves(1).seed(811), -1.0, 1.0),
                        remap(Noise::perlin().period(16).octaves(1).seed(812), -0.3, 0.3),
                    ),
                ),
            ),
        )
        .node(
            "ripple",
            Curve::new(Math::unary(Fract, "phase")).points([[0.0, 0.0], [0.72, 1.0], [1.0, 0.0]]),
        )
        .node("strength", remap(ramp("fade", 0.3, 0.7), 0.25, 1.0))
        .node(
            "dune",
            m(
                Add,
                m(Add, 0.1, m(Mul, "swell", 0.14)),
                m(Mul, m(Mul, "ripple", "strength"), 0.16),
            ),
        );
    let g = cells(g, "pebble", [32, 32], 821, 0.8, None)
        .node(
            "pebble_dome",
            m(
                Mul,
                Levels::new(m(
                    Div,
                    "pebble_dist",
                    remap(hash("pebble_id", 23.7), 0.14, 0.22),
                ))
                .in_range(1.0, 0.0)
                .gamma(0.5),
                m(
                    Step,
                    m(
                        Mul,
                        Warp::new(ramp("scatter", 0.55, 0.8), "pebble_off").amount(1.0),
                        0.3,
                    ),
                    hash("pebble_id", 7.13),
                ),
            ),
        )
        .node(
            "pebble_height",
            m(Add, m(Add, "dune", m(Mul, "pebble_dome", 0.12)), -0.03),
        )
        .node("height", Clamp::new(m(Max, "dune", "pebble_height")))
        .node("pebble", ramp(m(Sub, "pebble_height", "dune"), 0.0, 0.015))
        .node(
            "sand_color",
            m(
                Mul,
                Mix::new(
                    Mix::new([0.48, 0.32, 0.15], [0.59, 0.42, 0.21], "ripple"),
                    [0.66, 0.49, 0.27],
                    m(Mul, ramp("ripple", 0.8, 1.0), "strength"),
                ),
                m(
                    Mul,
                    m(Mul, remap("tone", 0.94, 1.06), remap("grain", 0.97, 1.03)),
                    Mix::new([1.03, 1.0, 0.95], [0.97, 1.0, 1.06], "warmth"),
                ),
            ),
        )
        .node(
            "color",
            Mix::new(
                "sand_color",
                m(
                    Mul,
                    Colorize::new(hash("pebble_id", 13.37)).gradient([
                        (0.0, [0.14, 0.12, 0.10]),
                        (0.5, [0.30, 0.24, 0.17]),
                        (1.0, [0.42, 0.37, 0.30]),
                    ]),
                    remap("pebble_dome", 0.75, 1.1),
                ),
                "pebble",
            ),
        )
        .node("roughness", Mix::new(0.93, 0.7, "pebble"));
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

/// Wind-packed snow: soft pillowed drifts, patches of sastrugi (the long
/// sharp-crested ridges the wind carves) running across them, and a very
/// faint blue where the snow folds into hollows. 200 mm of relief over a
/// 10 m repeat, lying low so rock and gravel poke through it.
///
/// Bright but held under about 0.85 linear, so a sunlit field does not clip
/// once the shader and the exposure are done with it, and the blue is kept to
/// the hollows so the field reads white at a distance.
#[must_use]
pub fn snow() -> MaterialGraph {
    use MathOp::{Abs, Add, Max, Mul, Sub};
    let tile = repeat("snow");
    let g = MaterialGraph::builder(graph_key("snow"))
        .node("drift", Noise::perlin().period(4).octaves(3).seed(901))
        .node("patch", Noise::perlin().period(8).octaves(2).seed(903))
        .node("crust", Noise::perlin().period(64).octaves(2).seed(905));
    let g = wobble(g, "wobble", 8, 907)
        .node(
            "sastrugi_noise",
            Warp::new(
                Noise::perlin().periods(4, 16).octaves(2).seed(911),
                "wobble",
            )
            .amount(0.04),
        )
        .node(
            "ridge",
            Levels::new(m(
                Sub,
                1.0,
                Math::unary(Abs, m(Sub, m(Mul, "sastrugi_noise", 2.0), 1.0)),
            ))
            .in_range(0.55, 1.0)
            .gamma(1.8),
        )
        .node("carved", ramp("patch", 0.45, 0.65));
    let g = cells(g, "pillow", [8, 8], 913, 1.0, Some(("wobble", 0.03)))
        // Blurred, so pillows meet in soft folds rather than creases.
        .node(
            "pillows",
            Blur::new(Levels::new("pillow_dist").in_range(0.9, 0.0).gamma(0.7)).radius(0.02),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.1, m(Mul, "drift", 0.16)),
                    m(Mul, "pillows", 0.12),
                ),
                m(
                    Add,
                    m(Mul, "ridge", m(Mul, "carved", 0.1)),
                    m(Mul, "crust", 0.015),
                ),
            )),
        )
        .node(
            "hollow",
            Curvature::cavity("height").radius(0.015).strength(40.0),
        )
        .node("shade", m(Max, "hollow", ramp("height", 0.22, 0.1)))
        .node(
            "color",
            Mix::new(
                Mix::new(
                    [0.78, 0.80, 0.84],
                    [0.85, 0.86, 0.88],
                    m(Add, m(Mul, "ridge", "carved"), m(Mul, "pillows", 0.4)),
                ),
                [0.50, 0.60, 0.78],
                m(Mul, "shade", 0.6),
            ),
        )
        .node("roughness", remap(m(Mul, "ridge", "carved"), 0.82, 0.6));
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.2,
        0.01,
    )
}
