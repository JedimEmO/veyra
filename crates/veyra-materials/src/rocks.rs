//! Veyra's rocks: granite and sandstone for boulders and cliffs, drawn for a
//! painterly shader.
//!
//! Rocks are box-projected in metres over meshes from a 0.3 m stone to a 25 m
//! cliff, so one repeat has to read on both: the geometry carries the big
//! shapes, and the texture adds facets, bands and a few marks a viewer can
//! name, with fine grain kept to a whisper. The meshes multiply the base
//! colour by a vertex colour (moss green on the tops), so both stones keep a
//! mid-to-light albedo and gentle contrast, or the tint would read as black.
//!
//! `v` runs down a vertical face, so the sandstone's strata lie across it and
//! its weathering streaks run along it.
use ashlar_material::{
    Channel, MaterialGraph, MaterialGraphBuilder, MathOp, PbrOutput,
    nodes::{
        Clamp, Colorize, Curvature, Decompose, Levels, Math, Mix, Noise, OcclusionFromHeight, Uv,
        Warp,
    },
};

use crate::{
    ROCKS,
    flora::{cells, hash, m, ramp, remap, wobble},
};

/// Every rock graph, keyed `veyra:rock-<set>`.
pub(crate) fn graphs() -> [MaterialGraph; 2] {
    [granite(), sandstone()]
}

/// The key a rock set's graph is registered under.
#[must_use]
pub fn graph_key(set: &str) -> String {
    format!("veyra:rock-{set}")
}

/// The repeat a set was authored against, read off the table so the graph and
/// the definition cannot disagree.
fn repeat(set: &str) -> f32 {
    ROCKS
        .iter()
        .find_map(|(name, metres)| (*name == set).then_some(*metres))
        .unwrap_or_else(|| unreachable!("{set} is a rock set"))
}

/// Bind the height, derive occlusion from it and declare the repeat.
/// `relief` is the physical depth of the `0..=1` height in metres, so
/// `relief / tile` is the UV-scale normal strength.
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
                .occlusion("ao")
                .metallic(0.0)
                .normal_strength(strength),
        )
        .into_graph()
}

/// Granite: warm light grey broken into soft chipped facets, flecked with
/// pink and cream feldspar and the odd dark mica, crossed by a few thin dark
/// cracks. 40 mm of relief over a 2 m repeat.
///
/// A facet is a Voronoi cell raised into a flat-topped cushion by a wide,
/// low-gamma ramp on its edge distance, so neighbours meet in a soft crease
/// rather than a joint; a smaller population of chips scoops shallow
/// conchoidal hollows out of some of them. The crests are lit a little paler
/// and the hollows a little darker, painted into the colour so they hold
/// under soft light.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn granite() -> MaterialGraph {
    use MathOp::{Add, Max, Mul, Step, Sub};
    let tile = repeat("granite");
    let g = MaterialGraph::builder(graph_key("granite"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(3101))
        .node("mottle", Noise::perlin().period(16).octaves(2).seed(3103))
        .node("grain", Noise::perlin().period(128).octaves(2).seed(3105))
        .node(
            "crack_gate",
            Noise::perlin().period(4).octaves(2).seed(3107),
        );
    let g = wobble(g, "wobble", [8, 8], 3109);
    let g = cells(g, "facet", [4, 4], 3111, 1.0, ("wobble", 0.03));
    let g = cells(g, "chip", [8, 8], 3113, 1.0, ("wobble", 0.015));
    let g = cells(g, "crack", [4, 4], 3115, 1.0, ("wobble", 0.03));
    let g = cells(g, "fleck", [32, 32], 3117, 1.0, ("wobble", 0.006))
        .node(
            "cushion",
            Levels::new("facet_edge").in_range(0.0, 0.4).gamma(0.5),
        )
        .node(
            "chip",
            m(
                Mul,
                Levels::new("chip_dist").in_range(0.6, 0.05).gamma(1.4),
                m(Step, hash("chip_id", 7.13), 0.65),
            ),
        )
        .node(
            "crack",
            m(
                Mul,
                ramp("crack_edge", 0.012, 0.0),
                ramp("crack_gate", 0.56, 0.62),
            ),
        )
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(
                    Add,
                    m(Add, 0.4, m(Mul, "cushion", 0.22)),
                    m(Mul, "tone", 0.14),
                ),
                m(
                    Sub,
                    m(Mul, "grain", 0.015),
                    m(Add, m(Mul, "chip", 0.07), m(Mul, "crack", 0.06)),
                ),
            )),
        )
        .node(
            "crest",
            Curvature::peaks("height").radius(0.012).strength(12.0),
        )
        .node(
            "hollow",
            Curvature::cavity("height").radius(0.012).strength(12.0),
        )
        .node(
            "stone",
            m(
                Mul,
                Colorize::new(hash("facet_id", 13.37)).gradient([
                    (0.0, [0.330, 0.302, 0.272]),
                    (0.5, [0.365, 0.334, 0.300]),
                    (1.0, [0.400, 0.362, 0.322]),
                ]),
                m(Mul, remap("tone", 0.94, 1.06), remap("mottle", 0.95, 1.05)),
            ),
        )
        // Feldspar in two warm colours and mica in a dark one, each fleck a
        // soft blot of its own size in its own cell so they never line up.
        .node("fleck_size", remap(hash("fleck_id", 23.1), 0.1, 0.32))
        .node(
            "fleck",
            m(
                Mul,
                Levels::new(m(Sub, "fleck_dist", "fleck_size")).in_range(0.08, 0.0),
                m(Step, hash("fleck_id", 7.13), 0.5),
            ),
        )
        .node(
            "fleck_color",
            Colorize::new(hash("fleck_id", 31.7)).gradient([
                (0.0, [0.46, 0.31, 0.27]),
                (0.45, [0.46, 0.31, 0.27]),
                (0.5, [0.47, 0.42, 0.34]),
                (0.9, [0.47, 0.42, 0.34]),
                (0.93, [0.15, 0.135, 0.125]),
                (1.0, [0.15, 0.135, 0.125]),
            ]),
        )
        .node(
            "color",
            Mix::new(
                m(
                    Mul,
                    Mix::new("stone", "fleck_color", m(Mul, "fleck", 0.4)),
                    m(Sub, remap("crest", 1.0, 1.14), m(Mul, "hollow", 0.14)),
                ),
                [0.090, 0.084, 0.078],
                m(Mul, "crack", 0.7),
            ),
        )
        .node(
            "roughness",
            Clamp::new(m(
                Max,
                remap(hash("facet_id", 3.1), 0.72, 0.82),
                m(Mul, "crack", 0.95),
            )),
        );
    finish(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.04,
        0.012,
    )
}

/// Sandstone: warm ochre and peach laid in horizontal strata of varied
/// thickness and value, their edges eroded soft, the harder beds standing
/// proud of the softer, with sparse darker weathering streaks running down
/// the face. 30 mm of relief over a 2 m repeat.
///
/// A stratum is a whole-number phase down the repeat, pushed about by slow
/// sines of `v` so the beds thicken and thin, and by a gentle noise round
/// the repeat so they undulate. Each bed's identity is its index modulo the bed
/// count, which is what lets the phase meet itself at the seam; a hash of it
/// picks the bed's colour and how hard it is, and a hard bed is rounded off
/// at both edges and a soft one recessed, which is how wind leaves it.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one graph, in field dependency order"
)]
pub fn sandstone() -> MaterialGraph {
    use MathOp::{Add, Div, Floor, Fract, Min, Mul, Sin, Sub};
    /// Beds down one repeat, before the noise thickens and thins them.
    const BEDS: f32 = 18.0;
    let tile = repeat("sandstone");
    let g = MaterialGraph::builder(graph_key("sandstone"))
        .node("tone", Noise::perlin().period(4).octaves(2).seed(3201))
        .node("grain", Noise::perlin().period(128).octaves(2).seed(3203))
        .node("pits", Noise::perlin().period(32).octaves(2).seed(3205))
        .node(
            "streak_noise",
            Warp::new(
                Noise::perlin().periods(16, 2).octaves(2).seed(3207),
                "streak_wander",
            )
            .amount(0.02),
        )
        .node(
            "streak_gate",
            Noise::perlin().periods(4, 2).octaves(2).seed(3209),
        )
        .node(
            "streak_wander",
            remap(
                Noise::perlin().periods(4, 8).octaves(1).seed(3215),
                -1.0,
                1.0,
            ),
        )
        .node("v", Decompose::new(Uv::new(), Channel::G))
        // Thickens and thins the beds down the face. Sines of `v` alone, at
        // whole frequencies, so every bed stays level across the repeat and
        // the phase meets itself at the seam; their summed slope stays under
        // the bed count, so the phase never runs backwards and folds a bed.
        .node(
            "squeeze",
            m(
                Add,
                m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 2.0), 0.13)), 0.4),
                m(
                    Add,
                    m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 3.0), 0.61)), 0.25),
                    m(Mul, Math::unary(Sin, m(Add, m(Mul, "v", 5.0), 0.37)), 0.12),
                ),
            ),
        )
        .node(
            "phase",
            m(
                Add,
                m(Mul, "v", BEDS),
                m(
                    Add,
                    "squeeze",
                    remap(
                        Noise::perlin().periods(2, 2).octaves(2).seed(3213),
                        -0.22,
                        0.22,
                    ),
                ),
            ),
        )
        .node(
            "bed",
            Math::unary(Fract, m(Div, Math::unary(Floor, "phase"), BEDS)),
        )
        .node("across", Math::unary(Fract, "phase"))
        .node("hard", hash("bed", 7.77))
        // Distance to the nearer bed edge, in beds: what the wind rounds.
        .node("edge", m(Min, "across", m(Sub, 1.0, "across")))
        .node("round", Levels::new("edge").in_range(0.0, 0.3).gamma(0.55))
        .node(
            "height",
            Clamp::new(m(
                Add,
                m(Add, 0.3, m(Mul, "round", m(Add, 0.18, m(Mul, "hard", 0.3)))),
                m(
                    Sub,
                    m(Mul, "grain", 0.03),
                    m(Mul, ramp("pits", 0.62, 0.8), 0.05),
                ),
            )),
        )
        .node(
            "bed_color",
            Colorize::new(hash("bed", 13.37)).gradient([
                (0.0, [0.330, 0.165, 0.075]),
                (0.2, [0.440, 0.250, 0.115]),
                (0.45, [0.500, 0.300, 0.175]),
                (0.7, [0.520, 0.375, 0.240]),
                (0.85, [0.400, 0.205, 0.100]),
                (1.0, [0.490, 0.330, 0.190]),
            ]),
        )
        .node(
            "streak",
            m(
                Mul,
                ramp("streak_noise", 0.58, 0.76),
                ramp("streak_gate", 0.5, 0.65),
            ),
        )
        .node(
            "hollow",
            Curvature::cavity("height").radius(0.01).strength(10.0),
        )
        .node(
            "color",
            m(
                Mul,
                m(
                    Mul,
                    "bed_color",
                    m(Mul, remap("round", 0.82, 1.0), remap("tone", 0.94, 1.06)),
                ),
                m(
                    Mul,
                    Mix::new(1.0, [0.62, 0.56, 0.54], m(Mul, "streak", 0.85)),
                    m(Sub, 1.0, m(Mul, "hollow", 0.12)),
                ),
            ),
        )
        .node("roughness", remap(hash("bed", 3.1), 0.82, 0.92));
    finish(
        g,
        PbrOutput::new().base_color("color").roughness("roughness"),
        tile,
        0.03,
        0.012,
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, num::NonZeroUsize};

    use ashlar::{Alpha, Surface};
    use ashlar_material::bake::{BakeRequest, rasterise};

    use super::*;
    use crate::{ROCK_RESOLUTION, graphs as all_graphs, rocks};

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "every compared repeat is a copy of one table literal"
    )]
    fn every_rock_definition_names_its_own_graph() {
        let graphs = all_graphs();
        let library = rocks();
        assert_eq!(library.materials.len(), ROCKS.len());
        for (set, metres) in ROCKS {
            let key = format!("rock:{set}");
            let definition = library
                .materials
                .get(&key)
                .unwrap_or_else(|| panic!("{key} is missing"));
            definition
                .check(&format!("materials[{key}]"))
                .unwrap_or_else(|error| panic!("{key}: {error}"));
            assert_eq!(definition.tile_metres, [metres; 2], "{key}");
            assert_eq!(definition.alpha, Alpha::Opaque, "{key}");
            let Surface::Graph(bake) = &definition.surface else {
                panic!("{key} is not a graph bake");
            };
            assert_eq!(bake.resolution, ROCK_RESOLUTION, "{key}");
            assert_eq!(bake.graph, graph_key(set), "{key}");
            assert!(bake.params.is_empty(), "{key} binds parameters");
            let graph = graphs
                .get(&bake.graph)
                .unwrap_or_else(|| panic!("{key} names a graph the library lacks"));
            assert_eq!(graph.tile_metres, Some([metres; 2]), "{}", bake.graph);
            assert!(graph.params.is_empty(), "{} has parameters", bake.graph);
            assert!(graph.output.opacity.is_none(), "{} cuts out", bake.graph);
            assert!(
                graph.output.height.is_some(),
                "{} binds no height",
                bake.graph
            );
            assert!(
                graph.output.normal_strength > 0.0,
                "{} would bake a flat normal",
                bake.graph
            );
        }
    }

    #[test]
    fn every_rock_graph_builds_clean_and_tiles_at_its_resolution() {
        let graphs = all_graphs();
        for (set, _) in ROCKS {
            let key = graph_key(set);
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
                lattice.iter().all(|&l| l <= ROCK_RESOLUTION),
                "{key} lays a {lattice:?} lattice"
            );
        }
    }

    /// The albedo stays mid-to-light, because the meshes multiply it by a
    /// moss tint, and the height stays in the unit with some relief to it.
    #[test]
    fn every_rock_bakes_mid_light_with_its_height_in_the_unit() {
        let graphs = all_graphs();
        for (set, _) in ROCKS {
            let key = graph_key(set);
            let graph = graphs.get(&key).expect("every set has a graph");
            // A quarter of the shipped resolution: the extremes barely move,
            // and the test stays fast in a debug build.
            let (planes, report) = rasterise(&BakeRequest {
                graph,
                library: &graphs,
                params: &BTreeMap::new(),
                resolution: ROCK_RESOLUTION / 4,
                mips: false,
                threads: NonZeroUsize::new(4),
            })
            .unwrap_or_else(|error| panic!("{key}: {error}"));
            let (low, high) = report.height_range.expect("a rock binds a height");
            assert!(
                low >= 0.0 && high <= 1.0,
                "{key} clips its height: {low}..{high}"
            );
            assert!(high - low > 0.15, "{key} is nearly flat: {low}..{high}");
            #[expect(
                clippy::cast_precision_loss,
                reason = "a texel count at 256 is exact in an f32"
            )]
            let texels = planes.base_color.len() as f32;
            let mean = planes
                .base_color
                .iter()
                .map(|c| ashlar_material::luminance(*c))
                .sum::<f32>()
                / texels;
            assert!(
                (0.2..=0.45).contains(&mean),
                "{key} has a mean albedo of {mean}"
            );
        }
    }
}
