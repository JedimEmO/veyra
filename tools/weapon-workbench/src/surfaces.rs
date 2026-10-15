//! Fine manufactured surfaces, authored at weapon scale rather than wall scale.
use ashlar_material::nodes::{Clamp, Levels, Math, Mix, Noise, Scratches};
use ashlar_material::{Input, MaterialGraph, MaterialGraphLibrary, MathOp, Param, PbrOutput};

fn math(op: MathOp, a: impl Into<Input>, b: impl Into<Input>) -> Math {
    Math::new(op, a, b)
}

/// These self-contained graphs are also exported beside the material definitions.
pub(crate) fn graphs() -> MaterialGraphLibrary {
    let mut library = MaterialGraphLibrary::default();
    library.insert(coating());
    library.insert(metal());
    library.insert(polymer());
    library
}

fn coating() -> MaterialGraph {
    use MathOp::{Add, Mul, Sub};
    MaterialGraph::builder("weapon:coating")
        .param(Param::color("color", [0.04, 0.05, 0.05]))
        .param(Param::float("wear", 0.08).range(0.0, 1.0))
        .param(Param::float("roughness", 0.53).range(0.0, 1.0))
        .node("grain", Noise::perlin().period(128).octaves(2).seed(901))
        .node(
            "tone",
            Levels::new(Noise::perlin().period(16).octaves(2).seed(903)).out_range(0.96, 1.04),
        )
        .node(
            "scratches",
            Scratches::new()
                .count(160)
                .length(0.065)
                .width(0.0012)
                .angle(85.0)
                .angle_spread(35.0)
                .seed(907),
        )
        .node("scuffed", math(Mul, "scratches", Input::param("wear")))
        .node(
            "color",
            Mix::new(
                math(Mul, Input::param("color"), "tone"),
                [0.19, 0.20, 0.21],
                math(Mul, "scuffed", 0.65),
            ),
        )
        .node(
            "roughness",
            Clamp::new(math(
                Sub,
                math(Add, Input::param("roughness"), math(Mul, "grain", 0.045)),
                math(Mul, "scuffed", 0.16),
            )),
        )
        .node(
            "height",
            math(
                Sub,
                math(Add, 0.48, math(Mul, "grain", 0.04)),
                math(Mul, "scuffed", 0.025),
            ),
        )
        .tile_metres([0.16; 2])
        .output(
            PbrOutput::new()
                .base_color("color")
                .roughness("roughness")
                .metallic(math(Mul, "scuffed", 0.8))
                .height("height")
                .normal_strength(0.0008),
        )
        .into_graph()
}

fn metal() -> MaterialGraph {
    use MathOp::{Add, Mul};
    MaterialGraph::builder("weapon:machined-metal")
        .param(Param::color("color", [0.2, 0.21, 0.22]))
        .param(Param::float("roughness", 0.32).range(0.0, 1.0))
        .node("brush", Noise::perlin().periods(512, 4).seed(919))
        .node("tone", Levels::new("brush").out_range(0.91, 1.04))
        .node("color", math(Mul, Input::param("color"), "tone"))
        .node("height", math(Add, 0.49, math(Mul, "brush", 0.02)))
        .tile_metres([0.16; 2])
        .output(
            PbrOutput::new()
                .base_color("color")
                .roughness(math(
                    Add,
                    Input::param("roughness"),
                    math(Mul, "brush", 0.05),
                ))
                .metallic(1.0)
                .height("height")
                .normal_strength(0.0005),
        )
        .into_graph()
}

fn polymer() -> MaterialGraph {
    use MathOp::{Add, Mul};
    MaterialGraph::builder("weapon:polymer")
        .param(Param::color("color", [0.006, 0.008, 0.009]))
        .node("grain", Noise::perlin().period(128).octaves(2).seed(929))
        .node("tone", Levels::new("grain").out_range(0.92, 1.05))
        .node("height", math(Add, 0.46, math(Mul, "grain", 0.08)))
        .tile_metres([0.10; 2])
        .output(
            PbrOutput::new()
                .base_color(math(Mul, Input::param("color"), "tone"))
                .roughness(math(Add, 0.70, math(Mul, "grain", 0.12)))
                .metallic(0.0)
                .height("height")
                .normal_strength(0.0012),
        )
        .into_graph()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manufactured_graphs_resolve_with_no_external_library() {
        let graphs = graphs();
        for graph in graphs.graphs.values() {
            graph
                .clone()
                .build_in(&graphs)
                .expect("self-contained weapon surface resolves");
        }
        let text = ron::to_string(&graphs).expect("graphs serialize");
        let decoded: MaterialGraphLibrary = ron::from_str(&text).expect("graphs round trip");
        assert_eq!(graphs, decoded);
    }
}
