//! The bare faces of the geological provinces (ticket 0004, step 3): granite,
//! sandstone, basalt, slate and clay. Limestone karst keeps the shared
//! [`rock`](super::rock) layer.
//!
//! A face is seen on a cliff from two metres and from two hundred, and has to
//! be told from the other four at a glance from both, so each rock is drawn
//! round the one structure a geologist would name it by, big enough to
//! survive the painterly shader: granite's crystals and peeling sheets,
//! sandstone's cross-bedded sets, basalt's columns, slate's cleavage, clay's
//! colour bands. Beyond the detail distance only a layer's mean colour is
//! left, so the five means are kept apart too (pale grey, red-buff, near
//! black, blue-grey, ochre-grey).
//!
//! The terrain projects a face biplanar in world space with `v` running down a
//! vertical face, so a structure drawn along `u` lies level on a cliff and one
//! drawn along `v` hangs plumb. Every rock but granite is directional and is
//! read with the stochastic cells' quarter turn off (`oriented` in
//! `assets/terrain/materials.ron`), or a bed would stand on end every few
//! metres.
//!
//! Heights mean what they mean in every other layer: the rock stands high, so
//! a face wins its blend against the soil or scree beside it, and its joints
//! and grooves sink to where loose ground would lie. Every pattern here is a
//! whole number of repeats across the tile, so it meets itself at the seam.
use ashlar_material::{
    Channel, MaterialGraph, MathOp, PbrOutput,
    nodes::{Blur, Clamp, Colorize, Curvature, Curve, Decompose, Levels, Math, Mix, Noise, Uv},
};

use super::{
    cells,
    city::{fold, index_hash},
    finish, graph_key, hash, m, ramp, remap, repeat, wobble,
};

/// Distance to the nearest whole number of a phase, `0..=0.5`: where a joint,
/// a bed or a plate edge runs.
fn to_whole(phase: &str) -> Math {
    use MathOp::{Fract, Min, Sub};
    m(
        Min,
        Math::unary(Fract, phase),
        m(Sub, 1.0, Math::unary(Fract, phase)),
    )
}

/// Granite: a coarse mosaic of grey, white and pink feldspar and quartz with
/// flecks of black mica, broken by a few long joints and peeling in sheets.
/// 300 mm of relief over an 8 m repeat.
///
/// The crystals are Voronoi cells about six centimetres across, each one
/// colour, which is what granite is at arm's length. The sheets are the
/// contours of a slow noise cut into four levels: each contour is the lip of a
/// sheet lying on the next, rounded over and shadowed underneath, curving as
/// exfoliation sheets curve. A sheet a fresh break has exposed is a little
/// paler and pinker than one that has weathered. Granite has no grain, so this
/// is the one face read through the quarter-turned cells like the ground.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn granite() -> MaterialGraph {
    use MathOp::{Add, Floor, Fract, Max, Mul, Step, Sub};
    /// Sheets the slow field is cut into.
    const SHEETS: f32 = 4.0;
    let tile = repeat("granite");
    let g = MaterialGraph::builder(graph_key("granite"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(1101))
        .node("mottle", Noise::perlin().period(16).octaves(2).seed(1103))
        // Smooth, for sheets a few metres across, with a little fine noise
        // so the broken edge of each is ragged rather than drawn.
        .node(
            "sheet_field",
            m(
                Add,
                Noise::perlin().period(2).octaves(1).seed(1105),
                remap(
                    Noise::perlin().period(32).octaves(2).seed(1106),
                    -0.02,
                    0.02,
                ),
            ),
        )
        .node("swell", Noise::perlin().period(8).octaves(2).seed(1107))
        .node(
            "joint_gate",
            Noise::perlin().period(4).octaves(2).seed(1109),
        )
        .node(
            "lichen_field",
            Noise::perlin().period(8).octaves(3).seed(1111),
        )
        .node("grain", Noise::perlin().period(256).octaves(1).seed(1113));
    let g = wobble(g, "wobble", 16, 1115);
    let g = cells(g, "joint", [2, 2], 1117, 1.0, Some(("wobble", 0.008)));
    let g = cells(g, "xtal", [128, 128], 1119, 1.0, Some(("wobble", 0.002)))
        .node("sheet_phase", m(Mul, "sheet_field", SHEETS))
        .node("sheet_level", Math::unary(Floor, "sheet_phase"))
        .node("sheet_up", Math::unary(Fract, "sheet_phase"))
        // Just above a contour is the lip of the upper sheet, rounded over;
        // just below it, the shadowed gap under that lip.
        .node(
            "lip",
            Levels::new("sheet_up").in_range(0.0, 0.14).gamma(0.5),
        )
        .node(
            "under",
            Levels::new("sheet_up").in_range(0.8, 1.0).gamma(2.0),
        )
        .node("fresh", index_hash("sheet_level", 0.37))
        .node(
            "joint",
            m(
                Mul,
                ramp("joint_edge", 0.004, 0.0),
                ramp("joint_gate", 0.52, 0.58),
            ),
        )
        .node("xtal_pick", hash("xtal_id", 13.37))
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.32, m(Mul, "sheet_level", 0.07)),
                    m(Mul, "lip", 0.05),
                ),
                m(
                    Sub,
                    m(
                        Add,
                        remap("swell", -0.05, 0.05),
                        m(Mul, m(Step, "xtal_pick", 0.81), 0.008),
                    ),
                    m(
                        Add,
                        m(Mul, "joint", 0.07),
                        m(Sub, m(Mul, "under", 0.03), m(Mul, "grain", 0.01)),
                    ),
                ),
            )),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.004).strength(5.0),
        )
        .node(
            "crest",
            Curvature::peaks("height").radius(0.006).strength(8.0),
        )
        // Grey feldspar and quartz for the mass, then pink potassium
        // feldspar, milky white, and a few dark grains.
        .node(
            "xtal_color",
            Colorize::new("xtal_pick").gradient([
                (0.0, [0.300, 0.300, 0.304]),
                (0.5, [0.356, 0.356, 0.360]),
                (0.62, [0.390, 0.388, 0.390]),
                (0.63, [0.470, 0.305, 0.262]),
                (0.8, [0.520, 0.350, 0.300]),
                (0.81, [0.520, 0.510, 0.490]),
                (0.94, [0.560, 0.548, 0.525]),
                (0.95, [0.090, 0.088, 0.086]),
                (1.0, [0.070, 0.068, 0.068]),
            ]),
        )
        .node(
            "sheet_tint",
            Mix::new([0.8, 0.82, 0.86], [1.1, 1.03, 0.98], "fresh"),
        )
        .node(
            "stone",
            m(
                Mul,
                m(Mul, "xtal_color", "sheet_tint"),
                m(Mul, remap("tone", 0.92, 1.08), remap("mottle", 0.94, 1.06)),
            ),
        )
        // Dark crustose lichen in blots on the older sheets.
        .node(
            "lichen",
            m(Mul, ramp("lichen_field", 0.62, 0.7), m(Sub, 1.0, "fresh")),
        )
        .node(
            "color",
            Mix::new(
                m(
                    Mul,
                    "stone",
                    m(
                        Mul,
                        m(Mul, remap("cavity", 1.0, 0.55), remap("crest", 1.0, 1.2)),
                        m(
                            Sub,
                            1.0,
                            m(Max, m(Mul, "under", 0.35), m(Mul, "joint", 0.75)),
                        ),
                    ),
                ),
                [0.105, 0.108, 0.098],
                m(Mul, "lichen", 0.6),
            ),
        )
        .node(
            "roughness",
            Clamp::new(m(
                Add,
                remap(hash("xtal_id", 3.1), 0.62, 0.78),
                m(Mul, "lichen", 0.15),
            )),
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

/// Sandstone: five cross-bedded sets down the repeat in dark red, rust,
/// buff, cream and tan, each laminated at its own angle, the harder sets
/// standing proud of the softer, notched at the bedding planes, cut by the
/// odd vertical joint and streaked with dark varnish. 250 mm of relief over
/// an 8 m repeat.
///
/// A set is a band of `v`, level on a cliff; within it the laminae run at a
/// slope of a whole number of laminae per repeat of `u`, so they tile, and
/// they are truncated at the set's top and bottom as dune foresets are. The
/// laminae's light and dark come in a rhythm of four, which survives the
/// shift of a whole number of laminae across the seam where a hash of each
/// lamina's index would not.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn sandstone() -> MaterialGraph {
    use MathOp::{Abs, Add, Floor, Fract, Max, Mul, Step, Sub};
    /// Cross-bedded sets down one repeat.
    const SETS: f32 = 5.0;
    /// Laminae down one repeat.
    const LAMINAE: f32 = 64.0;
    let tile = repeat("sandstone");
    let g = MaterialGraph::builder(graph_key("sandstone"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(1201))
        .node("grain", Noise::perlin().period(256).octaves(1).seed(1203))
        .node("u", Decompose::new(Uv::new(), Channel::R))
        .node("v", Decompose::new(Uv::new(), Channel::G))
        .node(
            "set_phase",
            m(
                Add,
                m(Mul, "v", SETS),
                remap(
                    Noise::perlin().periods(4, 2).octaves(2).seed(1205),
                    -0.18,
                    0.18,
                ),
            ),
        )
        .node("set_index", Math::unary(Floor, "set_phase"))
        .node("set", fold("set_index", SETS))
        .node("set_across", Math::unary(Fract, "set_phase"))
        .node("hard", index_hash("set", 0.71))
        // A slope of -24..=24 laminae per repeat of u, in steps of eight
        // (up to 20 degrees on the face, as dune foresets lie); some sets
        // come out planar.
        .node(
            "slope",
            m(
                Mul,
                m(
                    Sub,
                    Math::unary(Floor, m(Mul, index_hash("set", 0.29), 6.999)),
                    3.0,
                ),
                8.0,
            ),
        )
        .node(
            "lam_phase",
            m(
                Add,
                m(Add, m(Mul, "v", LAMINAE), m(Mul, "u", "slope")),
                remap(
                    Noise::perlin().periods(8, 8).octaves(2).seed(1207),
                    -0.35,
                    0.35,
                ),
            ),
        )
        .node(
            "lam_rhythm",
            Curve::new(Math::unary(Fract, m(Mul, "lam_phase", 0.25))).points([
                [0.0, 0.2],
                [0.25, 0.9],
                [0.5, 0.45],
                [0.75, 1.0],
                [1.0, 0.2],
            ]),
        )
        .node("lam_groove", ramp(to_whole("lam_phase"), 0.15, 0.0))
        .node(
            "pits",
            ramp(Noise::perlin().period(32).octaves(2).seed(1213), 0.64, 0.8),
        )
        .node(
            "notch",
            ramp(
                m(Sub, 0.5, Math::unary(Abs, m(Sub, "set_across", 0.5))),
                0.035,
                0.0,
            ),
        )
        .node(
            "round",
            Levels::new(m(Sub, 0.5, Math::unary(Abs, m(Sub, "set_across", 0.5))))
                .in_range(0.0, 0.2)
                .gamma(0.6),
        )
        .node(
            "joint_phase",
            m(Add, m(Mul, "u", 2.0), index_hash("set", 0.83)),
        )
        // Two vertical joints to a set, in a quarter of the sets, and only
        // where a slow noise lets them through, so they end inside a set.
        .node(
            "joint",
            m(
                Mul,
                m(
                    Mul,
                    ramp(to_whole("joint_phase"), 0.005, 0.0),
                    ramp(
                        Noise::perlin().periods(4, 8).octaves(1).seed(1215),
                        0.45,
                        0.55,
                    ),
                ),
                m(Step, index_hash("set", 0.53), 0.75),
            ),
        )
        .node(
            "streak",
            m(
                Mul,
                ramp(
                    Noise::perlin().periods(16, 2).octaves(2).seed(1209),
                    0.63,
                    0.8,
                ),
                ramp(
                    Noise::perlin().periods(4, 2).octaves(2).seed(1211),
                    0.5,
                    0.65,
                ),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.4, m(Mul, "hard", 0.14)),
                    m(Mul, "round", m(Add, 0.08, m(Mul, "hard", 0.08))),
                ),
                m(
                    Sub,
                    m(Add, m(Mul, "grain", 0.012), remap("tone", -0.03, 0.03)),
                    m(
                        Add,
                        m(Add, m(Mul, "notch", 0.08), m(Mul, "joint", 0.07)),
                        m(Add, m(Mul, "lam_groove", 0.022), m(Mul, "pits", 0.05)),
                    ),
                ),
            )),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.004).strength(5.0),
        )
        .node(
            "set_color",
            Colorize::new(index_hash("set", 0.17)).gradient([
                (0.0, [0.340, 0.110, 0.055]),
                (0.25, [0.430, 0.170, 0.080]),
                (0.5, [0.540, 0.360, 0.210]),
                (0.7, [0.600, 0.500, 0.370]),
                (0.85, [0.470, 0.300, 0.160]),
                (1.0, [0.380, 0.140, 0.065]),
            ]),
        )
        .node(
            "color",
            m(
                Mul,
                m(
                    Mul,
                    "set_color",
                    m(
                        Mul,
                        m(
                            Mul,
                            remap("lam_rhythm", 0.8, 1.1),
                            remap("grain", 0.95, 1.05),
                        ),
                        remap("tone", 0.93, 1.07),
                    ),
                ),
                m(
                    Mul,
                    Mix::new(1.0, [0.42, 0.34, 0.32], m(Mul, "streak", 0.6)),
                    m(
                        Sub,
                        remap("cavity", 1.0, 0.6),
                        m(Mul, m(Max, "notch", "joint"), 0.4),
                    ),
                ),
            ),
        )
        .node("roughness", remap(index_hash("set", 0.41), 0.82, 0.92));
    finish(
        g,
        PbrOutput::new()
            .base_color("color")
            .roughness("roughness")
            .occlusion("ao"),
        tile,
        0.25,
        0.01,
    )
}

/// Basalt: near-black columns about a metre across standing plumb, each
/// showing two facets of its prism, split by curved cross-joints, pitted
/// with vesicles in places and weathered rusty brown. 300 mm of relief over
/// an 8 m repeat.
///
/// A column is a band of `u` whose borders lean and wander a little down the
/// face, so the eight in a repeat are of uneven width. Across it the height
/// is a tent peaked at a per-column place, which the normal turns into two
/// lit facets, the way a colonnade reads in low sun. The cross-joints are
/// bowed by the column's own across coordinate, as ball-and-socket joints
/// are, and each column breaks at heights of its own.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn basalt() -> MaterialGraph {
    use MathOp::{Add, Div, Floor, Fract, Max, Min, Mul, Step, Sub};
    /// Columns across one repeat.
    const COLUMNS: f32 = 8.0;
    /// Cross-joints down one repeat, on average, in each column.
    const JOINTS: f32 = 6.0;
    let tile = repeat("basalt");
    let g = MaterialGraph::builder(graph_key("basalt"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(1301))
        .node("hackly", Noise::perlin().period(32).octaves(3).seed(1303))
        .node("weather", Noise::perlin().period(4).octaves(3).seed(1305))
        .node(
            "vesicle_zone",
            Noise::perlin().periods(2, 4).octaves(2).seed(1307),
        )
        .node("u", Decompose::new(Uv::new(), Channel::R))
        .node("v", Decompose::new(Uv::new(), Channel::G))
        .node(
            "col_phase",
            m(
                Add,
                m(Mul, "u", COLUMNS),
                remap(
                    Noise::perlin().periods(8, 2).octaves(2).seed(1309),
                    -0.22,
                    0.22,
                ),
            ),
        )
        .node("col", fold(Math::unary(Floor, "col_phase"), COLUMNS))
        .node("across", Math::unary(Fract, "col_phase"))
        .node("peak", remap(index_hash("col", 0.61), 0.3, 0.7))
        .node(
            "tent",
            m(
                Min,
                m(Div, "across", "peak"),
                m(Div, m(Sub, 1.0, "across"), m(Sub, 1.0, "peak")),
            ),
        )
        .node("col_joint", ramp(to_whole("col_phase"), 0.022, 0.0))
        .node(
            "cj_phase",
            m(
                Add,
                m(Add, m(Mul, "v", JOINTS), index_hash("col", 0.23)),
                m(
                    Mul,
                    m(Mul, m(Sub, "across", 0.5), m(Sub, "across", 0.5)),
                    remap(index_hash("col", 0.91), -0.7, 0.7),
                ),
            ),
        )
        .node(
            "segment",
            m(
                Add,
                fold(Math::unary(Floor, "cj_phase"), JOINTS),
                m(Mul, "col", JOINTS),
            ),
        )
        .node(
            "cross_joint",
            m(
                Mul,
                ramp(to_whole("cj_phase"), 0.05, 0.0),
                m(Step, index_hash("segment", 0.47), 0.45),
            ),
        )
        .node("seg_tone", index_hash("segment", 0.13));
    let g = cells(g, "ves", [64, 64], 1311, 1.0, None)
        .node(
            "vesicle",
            m(
                Mul,
                Levels::new(m(Div, "ves_dist", remap(hash("ves_id", 23.7), 0.12, 0.3)))
                    .in_range(1.0, 0.7),
                m(
                    Step,
                    m(Mul, ramp("vesicle_zone", 0.4, 0.7), 0.85),
                    hash("ves_id", 7.13),
                ),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.42, m(Mul, "tent", 0.22)),
                    m(Mul, "seg_tone", 0.04),
                ),
                m(
                    Sub,
                    remap("hackly", -0.03, 0.03),
                    m(
                        Add,
                        m(Mul, m(Max, "col_joint", "cross_joint"), 0.16),
                        m(Mul, "vesicle", 0.05),
                    ),
                ),
            )),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.004).strength(5.0),
        )
        .node(
            "crest",
            Curvature::peaks("height").radius(0.006).strength(8.0),
        )
        .node(
            "stone",
            m(
                Mul,
                Colorize::new(Math::unary(
                    Fract,
                    m(Add, index_hash("col", 0.07), m(Mul, "seg_tone", 0.3)),
                ))
                .gradient([
                    (0.0, [0.040, 0.040, 0.044]),
                    (0.5, [0.052, 0.050, 0.052]),
                    (1.0, [0.064, 0.058, 0.056]),
                ]),
                m(
                    Mul,
                    remap("tone", 0.9, 1.1),
                    // The two facets of a prism painted a little apart, so
                    // the columns read under a flat sky too.
                    Mix::new(1.1, 0.9, m(Step, "across", "peak")),
                ),
            ),
        )
        // Rusty brown where the rock has weathered, running down from the
        // joints.
        .node(
            "rust",
            m(
                Mul,
                ramp("weather", 0.52, 0.72),
                ramp(
                    Noise::perlin().periods(16, 2).octaves(2).seed(1313),
                    0.4,
                    0.7,
                ),
            ),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    Mix::new("stone", [0.095, 0.064, 0.044], m(Mul, "rust", 0.8)),
                    [0.105, 0.102, 0.098],
                    m(Mul, "crest", 0.5),
                ),
                m(
                    Sub,
                    remap("cavity", 1.0, 0.5),
                    m(
                        Mul,
                        m(Max, "vesicle", m(Max, "col_joint", "cross_joint")),
                        0.45,
                    ),
                ),
            ),
        )
        .node(
            "roughness",
            Clamp::new(m(
                Add,
                remap(index_hash("col", 0.33), 0.66, 0.78),
                m(Mul, "rust", 0.12),
            )),
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

/// Slate: dark blue-grey plates cleaved steeply along one direction, each
/// rising to a sharp edge and breaking off into the next, some purple and
/// some green, with a paler fresh break on every edge and the odd rusty
/// pyrite. 300 mm of relief over an 8 m repeat.
///
/// The cleavage is a phase of 24 plates per repeat of `u` and 8 of `v`, so
/// its planes stand about 72 degrees from level and meet themselves at the
/// seam. Each plate is a sawtooth across the cleavage (a slab rising to its
/// edge and dropping sharply to the next), cut into pieces along it by a
/// second phase at right angles, and a piece's identity is its two indices
/// folded by the whole-number shifts the seam brings, so the hash of a piece
/// is the same from either side.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn slate() -> MaterialGraph {
    use MathOp::{Add, Floor, Fract, Max, Mul, Step, Sub};
    let tile = repeat("slate");
    let g = MaterialGraph::builder(graph_key("slate"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(1401))
        .node("sheen", Noise::perlin().period(8).octaves(2).seed(1403))
        .node("u", Decompose::new(Uv::new(), Channel::R))
        .node("v", Decompose::new(Uv::new(), Channel::G))
        .node(
            "cleave",
            m(
                Add,
                m(Add, m(Mul, "u", 24.0), m(Mul, "v", 8.0)),
                remap(Noise::perlin().period(8).octaves(2).seed(1405), -0.14, 0.14),
            ),
        )
        .node("plate_index", Math::unary(Floor, "cleave"))
        .node("rise", Math::unary(Fract, "cleave"))
        // At right angles to the cleavage: (24, 8) . (-1, 3) is zero.
        .node(
            "along",
            m(
                Add,
                m(Add, m(Mul, "u", -2.0), m(Mul, "v", 6.0)),
                m(
                    Add,
                    index_hash(fold("plate_index", 8.0), 0.57),
                    remap(
                        Noise::perlin().period(16).octaves(1).seed(1407),
                        -0.08,
                        0.08,
                    ),
                ),
            ),
        )
        .node(
            "piece",
            m(
                Add,
                fold("plate_index", 8.0),
                m(Mul, fold(Math::unary(Floor, "along"), 2.0), 8.0),
            ),
        )
        .node("piece_hash", index_hash("piece", 0.31))
        .node("slab", Levels::new("rise").in_range(0.0, 0.94).gamma(1.3))
        .node("edge", ramp("rise", 0.9, 1.0))
        .node("break", ramp(to_whole("along"), 0.03, 0.0))
        .node("fine_phase", m(Mul, "cleave", 4.0))
        .node("fine", ramp(to_whole("fine_phase"), 0.08, 0.0));
    let g = cells(g, "pyrite", [128, 128], 1409, 1.0, None)
        .node(
            "pyrite",
            m(
                Mul,
                ramp("pyrite_dist", 0.3, 0.18),
                m(Step, hash("pyrite_id", 7.13), 0.985),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.4, m(Mul, "piece_hash", 0.14)),
                    m(Mul, "slab", 0.14),
                ),
                m(
                    Sub,
                    remap("tone", -0.03, 0.03),
                    m(Add, m(Mul, "break", 0.08), m(Mul, "fine", 0.012)),
                ),
            )),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.004).strength(5.0),
        )
        .node(
            "stone",
            m(
                Mul,
                Colorize::new("piece_hash").gradient([
                    (0.0, [0.052, 0.058, 0.074]),
                    (0.35, [0.072, 0.082, 0.104]),
                    (0.6, [0.086, 0.094, 0.118]),
                    (0.75, [0.094, 0.074, 0.096]),
                    (0.88, [0.070, 0.086, 0.082]),
                    (1.0, [0.080, 0.090, 0.112]),
                ]),
                m(Mul, remap("tone", 0.9, 1.1), remap("slab", 0.92, 1.06)),
            ),
        )
        .node(
            "color",
            m(
                Mul,
                Mix::new(
                    Mix::new("stone", [0.150, 0.158, 0.178], m(Mul, "edge", 0.55)),
                    [0.240, 0.140, 0.050],
                    "pyrite",
                ),
                m(
                    Sub,
                    remap("cavity", 1.0, 0.5),
                    m(Mul, m(Max, "break", m(Mul, "fine", 0.4)), 0.35),
                ),
            ),
        )
        // Slate has a silky sheen on its cleavage and is rough where broken.
        .node(
            "roughness",
            Clamp::new(m(
                Add,
                remap("sheen", 0.48, 0.62),
                m(Mul, m(Max, "edge", "break"), 0.25),
            )),
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

/// Badlands clay: soft level bands of pale grey, blue-grey, mustard, ochre,
/// rust, buff and white ash, fluted by rills running straight down, crusted
/// with rounded popcorn lumps and crazed by shallow drying cracks, with no
/// hard edge anywhere. 200 mm of relief over an 8 m repeat.
///
/// A band is a phase of `v` pushed about by whole-frequency sines of `v`,
/// so the bands thicken and thin and stay level; the rills are a phase of
/// `u` wandering a little with `v`, so they hang plumb on a cliff as water
/// cuts them. The height is blurred as a last step, which is what keeps
/// every edge soft.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn clay() -> MaterialGraph {
    use MathOp::{Add, Cos, Floor, Fract, Mul, Sin, Sub};
    /// Bands down one repeat, before the sines thicken and thin them.
    const BANDS: f32 = 14.0;
    let tile = repeat("clay");
    let g = MaterialGraph::builder(graph_key("clay"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(1501))
        .node(
            "crack_gate",
            Noise::perlin().period(4).octaves(2).seed(1503),
        )
        .node(
            "rill_gate",
            Noise::perlin().periods(8, 2).octaves(2).seed(1505),
        )
        .node("u", Decompose::new(Uv::new(), Channel::R))
        .node("v", Decompose::new(Uv::new(), Channel::G))
        .node(
            "squeeze",
            m(
                Add,
                m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 2.0), 0.21)), 0.5),
                m(
                    Add,
                    m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 3.0), 0.67)), 0.3),
                    m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 5.0), 0.43)), 0.15),
                ),
            ),
        )
        .node(
            "band_phase",
            m(
                Add,
                m(Add, m(Mul, "v", BANDS), "squeeze"),
                remap(
                    Noise::perlin().periods(4, 4).octaves(2).seed(1507),
                    -0.2,
                    0.2,
                ),
            ),
        )
        .node("band", fold(Math::unary(Floor, "band_phase"), BANDS))
        .node("band_across", Math::unary(Fract, "band_phase"))
        .node("soft", index_hash("band", 0.43))
        .node(
            "rill_phase",
            m(
                Add,
                m(Mul, "u", 48.0),
                remap(
                    Noise::perlin().periods(8, 4).octaves(2).seed(1509),
                    -1.5,
                    1.5,
                ),
            ),
        )
        .node(
            "rill",
            m(
                Mul,
                m(Sub, 0.5, m(Mul, Math::unary(Cos, "rill_phase"), 0.5)),
                ramp("rill_gate", 0.3, 0.6),
            ),
        );
    let g = wobble(g, "wobble", 16, 1511);
    let g = cells(g, "pop", [64, 64], 1513, 1.0, Some(("wobble", 0.004)));
    let g = cells(g, "craze", [16, 16], 1515, 1.0, Some(("wobble", 0.01)))
        .node(
            "popcorn",
            Levels::new("pop_dist").in_range(0.95, 0.1).gamma(0.6),
        )
        .node(
            "crack",
            m(
                Mul,
                ramp("craze_edge", 0.03, 0.0),
                ramp("crack_gate", 0.45, 0.6),
            ),
        )
        .node(
            "raw_height",
            m(
                Add,
                m(
                    Add,
                    m(Add, 0.42, m(Mul, m(Sub, 1.0, "soft"), 0.1)),
                    m(Mul, "popcorn", 0.1),
                ),
                m(
                    Sub,
                    remap("tone", -0.04, 0.04),
                    m(
                        Add,
                        m(Mul, "rill", m(Add, 0.05, m(Mul, "soft", 0.08))),
                        m(Mul, "crack", 0.05),
                    ),
                ),
            ),
        )
        .node("height", Clamp::new(Blur::new("raw_height").radius(0.003)))
        .node(
            "band_color",
            Blur::new(Colorize::new(index_hash("band", 0.19)).gradient([
                (0.0, [0.400, 0.396, 0.380]),
                (0.1, [0.340, 0.350, 0.352]),
                (0.22, [0.520, 0.370, 0.120]),
                (0.4, [0.480, 0.290, 0.130]),
                (0.56, [0.420, 0.170, 0.085]),
                (0.72, [0.500, 0.400, 0.260]),
                (0.84, [0.560, 0.548, 0.510]),
                (1.0, [0.460, 0.300, 0.230]),
            ]))
            .radius(0.004),
        )
        .node(
            "cavity",
            Curvature::cavity("height").radius(0.006).strength(4.0),
        )
        .node(
            "color",
            m(
                Mul,
                m(
                    Mul,
                    "band_color",
                    m(
                        Mul,
                        remap("tone", 0.93, 1.07),
                        remap("band_across", 0.97, 1.03),
                    ),
                ),
                m(
                    Sub,
                    m(Mul, remap("cavity", 1.0, 0.7), remap("popcorn", 0.94, 1.03)),
                    m(Add, m(Mul, "rill", 0.08), m(Mul, "crack", 0.2)),
                ),
            ),
        )
        .node("roughness", remap("tone", 0.86, 0.95));
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
