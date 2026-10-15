//! Veyra's materials: the definitions its content binds, over the graphs that
//! draw them.
//!
//! Everything here is procedural. A definition either names a graph with the
//! parameters that make it ours, or is a constant (glass, lights, ink), and
//! `tools/content` bakes the graphs into the KTX2 maps the game loads. The
//! keys are the ones the building recipes already bind, so a recipe never
//! learns where its materials come from.
use std::collections::BTreeMap;

use ashlar::{Alpha, Bake, MaterialDefinition, MaterialLibrary, ParamValue, Surface};
use ashlar_material::{MaterialGraphLibrary, stdlib};

pub mod flora;
pub mod rocks;
pub mod terrain;

/// Every graph a Veyra definition names: the default library and the game's
/// own terrain, tree and rock graphs.
///
/// # Panics
///
/// If a Veyra graph takes a key the default library already holds, which a
/// test catches before any content step runs.
#[must_use]
pub fn graphs() -> MaterialGraphLibrary {
    let mut library = stdlib::graphs();
    let mut own = MaterialGraphLibrary::default();
    for graph in terrain::graphs()
        .into_iter()
        .chain(flora::graphs())
        .chain(rocks::graphs())
    {
        own.insert(graph);
    }
    library
        .extend(&own)
        .unwrap_or_else(|error| panic!("a Veyra graph collides with the library: {error}"));
    library
}

/// The definitions Veyra's buildings bind: the recipe study's `study:*` and
/// the corporate kit's `metro:*`.
///
/// # Panics
///
/// If the default library stops offering a graph this names, which a test
/// catches before any content step runs.
#[must_use]
pub fn buildings() -> MaterialLibrary {
    let library = stdlib::materials();
    let graph = |key: &str, params: &[(&str, ParamValue)]| {
        let mut definition = library.materials[key].clone();
        let Surface::Graph(bake) = &mut definition.surface else {
            unreachable!("a default library definition names its graph");
        };
        for (name, value) in params {
            bake.params.insert((*name).to_owned(), *value);
        }
        definition
    };
    let concrete = |srgb| graph("library:formed-concrete", &[("color", linear(srgb))]);
    let painted = |srgb, wear| {
        graph(
            "library:painted-metal",
            &[("color", linear(srgb)), ("wear", ParamValue::Float(wear))],
        )
    };
    let constant = |srgb: [f32; 3], roughness, metallic, emissive| MaterialDefinition {
        base_color: srgb,
        roughness,
        metallic,
        emissive,
        ..MaterialDefinition::default()
    };
    let dark = [0.0; 3];
    let definitions = [
        // The recipe study: grey formed concrete, a warmer repair patch, and
        // blued steel with orange paint and rust on the fittings.
        ("study:concrete", concrete([0.36, 0.39, 0.40])),
        ("study:cut", concrete([0.34, 0.35, 0.36])),
        ("study:repair", concrete([0.50, 0.46, 0.39])),
        ("study:metal", painted([0.13, 0.17, 0.19], 0.35)),
        ("study:paint", painted([0.62, 0.39, 0.11], 0.55)),
        ("study:rust", graph("library:rusted-steel", &[])),
        (
            "study:dark",
            constant([0.025, 0.03, 0.033], 0.95, 0.0, dark),
        ),
        (
            "study:glass",
            constant([0.055, 0.11, 0.13], 0.18, 0.5, dark),
        ),
        (
            "study:light",
            constant([0.62, 0.82, 0.84], 0.8, 0.0, [2.0, 3.5, 4.0]),
        ),
        // The corporate kit: dark blue-grey concrete in four tones, dark
        // metal, and the constants for its glazing, lights and signage.
        ("metro:stone", concrete([0.24, 0.28, 0.31])),
        ("metro:spine", concrete([0.46, 0.49, 0.51])),
        ("metro:reveal", concrete([0.08, 0.105, 0.13])),
        ("metro:dark", concrete([0.075, 0.09, 0.11])),
        ("metro:paving", concrete([0.19, 0.22, 0.245])),
        ("metro:metal", painted([0.09, 0.125, 0.16], 0.25)),
        (
            "metro:glass",
            constant([0.055, 0.09, 0.125], 0.2, 0.7, dark),
        ),
        (
            "metro:occupied",
            constant([0.1, 0.15, 0.17], 0.35, 0.45, [0.06, 0.1, 0.12]),
        ),
        (
            "metro:light",
            constant([0.72, 0.84, 0.92], 0.3, 0.0, [2.5, 3.5, 4.0]),
        ),
        (
            "metro:signal",
            constant([0.55, 0.24, 0.045], 0.55, 0.0, [0.8, 0.3, 0.05]),
        ),
        ("metro:ink", constant([0.8, 0.83, 0.81], 0.7, 0.0, dark)),
    ];
    MaterialLibrary {
        materials: definitions
            .into_iter()
            .map(|(key, definition)| (key.to_owned(), definition))
            .collect(),
    }
}

/// The terrain layers, by the texture set name `assets/terrain/materials.ron`
/// gives each, and the metres one repeat covers on the ground. The terrain's
/// registry must say the same repeat; a test holds the two together.
pub const TERRAIN: [(&str, f32); 14] = [
    ("basalt", 8.0),
    ("clay", 8.0),
    ("flagstones", 8.0),
    ("forest_floor", 6.0),
    ("grass", 8.0),
    ("granite", 8.0),
    ("gravel", 4.0),
    ("rock", 8.0),
    ("sand", 12.0),
    ("sandstone", 8.0),
    ("setts", 4.0),
    ("slate", 8.0),
    ("snow", 10.0),
    ("soil", 6.0),
];

/// Texels per repeat the terrain layers bake at: they cover metres of ground
/// seen from a metre away, where a building's 512 is too coarse.
pub const TERRAIN_RESOLUTION: u32 = 1024;

/// The terrain layers as definitions keyed `terrain:<set>`, each drawn by
/// its own `veyra:terrain-<set>` graph with a height output the terrain
/// blends by.
#[must_use]
pub fn terrain() -> MaterialLibrary {
    MaterialLibrary {
        materials: TERRAIN
            .iter()
            .map(|(set, metres)| {
                let definition = MaterialDefinition {
                    tile_metres: [*metres; 2],
                    // The graph's own values stand: these multiply them.
                    roughness: 1.0,
                    metallic: 1.0,
                    // A terrain layer is a texture; the game grows its own
                    // grass.
                    strands: None,
                    surface: Surface::Graph(Bake {
                        graph: terrain::graph_key(set),
                        params: BTreeMap::new(),
                        resolution: TERRAIN_RESOLUTION,
                    }),
                    ..MaterialDefinition::default()
                };
                (format!("terrain:{set}"), definition)
            })
            .collect(),
    }
}

/// The bark sets and their repeat in metres, `u` around the trunk by `v`
/// along it. The tree generator lays bark UVs in metres with a whole number of
/// repeats around every ring, so each graph tiles in `u` and its relief runs
/// mostly along `v`.
pub const FLORA_BARK: [(&str, [f32; 2]); 5] = [
    ("oak", [0.8, 1.6]),
    ("pine", [0.8, 1.6]),
    ("birch", [0.6, 1.2]),
    ("fir", [0.7, 1.4]),
    ("shrub", [0.3, 0.6]),
];

/// The leaf card sets. Each card is one square picture of a cluster, laid
/// over `0..1` of the card, with the twig entering at [`LEAF_CARD_STEM`].
pub const FLORA_LEAVES: [&str; 6] = ["oak", "pine", "birch", "fir", "shrub", "fern"];

/// Where every leaf card's twig starts, in card UV: the bottom centre, a
/// little inside the edge, with `v` increasing down the picture. A generator
/// that hangs a card off a branch puts this point on the branch.
pub const LEAF_CARD_STEM: [f32; 2] = [0.5, 0.95];

/// Texels per repeat the bark bakes at: a trunk is seen from a metre away and
/// its repeat is most of a metre.
pub const FLORA_BARK_RESOLUTION: u32 = 1024;

/// Texels per card the leaves bake at. A card is a few tenths of a metre and
/// mostly seen from further than a trunk, so 512 holds its shapes with room.
pub const FLORA_LEAF_RESOLUTION: u32 = 512;

/// The alpha a leaf card is cut at: the half its coverage-preserving mip
/// chain holds the share of leaf to.
pub const FLORA_LEAF_CUTOFF: f32 = 0.5;

/// The share of diffuse light a leaf lets through, which is what makes a
/// canopy glow against a low sun.
pub const FLORA_LEAF_TRANSMISSION: f32 = 0.4;

/// The tree materials, keyed `flora:bark-<set>` and `flora:leaves-<set>`,
/// each drawn by its own `veyra:bark-<set>` or `veyra:leaves-<set>` graph.
///
/// Bark is an opaque surface with a height the normal comes from. Leaves are
/// cut-out cards drawn from both sides and lit through, so their opacity is
/// the silhouette of the cluster.
#[must_use]
pub fn flora() -> MaterialLibrary {
    let graph = |graph: String, resolution| {
        Surface::Graph(Bake {
            graph,
            params: BTreeMap::new(),
            resolution,
        })
    };
    let bark = FLORA_BARK.iter().map(|(set, metres)| {
        let definition = MaterialDefinition {
            tile_metres: *metres,
            // The graph's own values stand: these multiply them.
            roughness: 1.0,
            metallic: 1.0,
            surface: graph(flora::bark_key(set), FLORA_BARK_RESOLUTION),
            ..MaterialDefinition::default()
        };
        (format!("flora:bark-{set}"), definition)
    });
    let leaves = FLORA_LEAVES.iter().map(|set| {
        let definition = MaterialDefinition {
            tile_metres: [1.0; 2],
            roughness: 1.0,
            metallic: 1.0,
            surface: graph(flora::leaves_key(set), FLORA_LEAF_RESOLUTION),
            alpha: Alpha::Mask {
                cutoff: FLORA_LEAF_CUTOFF,
            },
            double_sided: true,
            diffuse_transmission: FLORA_LEAF_TRANSMISSION,
            ..MaterialDefinition::default()
        };
        (format!("flora:leaves-{set}"), definition)
    });
    MaterialLibrary {
        materials: bark.chain(leaves).collect(),
    }
}

/// The rock sets and the metres one repeat covers. Rocks are box-projected in
/// metres over everything from a 0.3 m stone to a 25 m cliff, so the repeat
/// is sized to read on both.
pub const ROCKS: [(&str, f32); 2] = [("granite", 2.0), ("sandstone", 2.0)];

/// Texels per repeat the rocks bake at: two millimetres a texel over a 2 m
/// repeat, which a boulder underfoot still resolves.
pub const ROCK_RESOLUTION: u32 = 1024;

/// The rocks as definitions keyed `rock:<set>`, each drawn by its own
/// `veyra:rock-<set>` graph with a height the normal comes from.
#[must_use]
pub fn rocks() -> MaterialLibrary {
    MaterialLibrary {
        materials: ROCKS
            .iter()
            .map(|(set, metres)| {
                let definition = MaterialDefinition {
                    tile_metres: [*metres; 2],
                    // The graph's own values stand: these multiply them.
                    roughness: 1.0,
                    metallic: 1.0,
                    surface: Surface::Graph(Bake {
                        graph: rocks::graph_key(set),
                        params: BTreeMap::new(),
                        resolution: ROCK_RESOLUTION,
                    }),
                    ..MaterialDefinition::default()
                };
                (format!("rock:{set}"), definition)
            })
            .collect(),
    }
}

/// The crossings' stone (ticket 0005, step 7), keyed `bridge:*`:
/// `bridge:masonry`, the abutments', piers' and headwalls' coursed blocks,
/// and `bridge:ford-stone`, the stepping stones.
///
/// The masonry is the library's ashlar blocks in a darker, weathered grey
/// and at three quarters of their repeat: the city's terrace masonry on a
/// bridge read as clean, oversized toy bricks paler than the banks round it
/// (the step 7 review). It takes the slow world-space variation, as the
/// city's masonry does, so one abutment is not the next one's tone. The
/// stepping stones are the rock kits' granite darkened, as a stone in a
/// stream is wet and stained.
///
/// # Panics
///
/// If the default library stops offering the ashlar blocks, which a test
/// catches before any content step runs.
#[must_use]
pub fn crossings() -> MaterialLibrary {
    let mut masonry = stdlib::materials().materials["library:ashlar-blocks"].clone();
    if let Surface::Graph(bake) = &mut masonry.surface {
        bake.params
            .insert("color".to_owned(), linear([0.43, 0.41, 0.37]));
        bake.params
            .insert("variation".to_owned(), ParamValue::Float(0.85));
    }
    masonry.tile_metres = masonry.tile_metres.map(|m| m * 0.75);
    masonry.tile_scale = Some(0.75);
    masonry.anti_tiling = Some(ashlar::AntiTiling::macro_only());
    let mut ford_stone = rocks().materials["rock:granite"].clone();
    ford_stone.base_color = [0.52, 0.5, 0.47];
    MaterialLibrary {
        materials: [
            ("bridge:masonry".to_owned(), masonry),
            ("bridge:ford-stone".to_owned(), ford_stone),
        ]
        .into_iter()
        .collect(),
    }
}

/// A graph colour parameter from an sRGB colour: graphs work in linear light,
/// and these colours were picked by eye as sRGB multipliers.
fn linear(srgb: [f32; 3]) -> ParamValue {
    ParamValue::Color(srgb.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_building_definition_validates_against_the_graphs() {
        let graphs = graphs();
        for (key, definition) in buildings().materials.iter().chain(&crossings().materials) {
            definition
                .check(&format!("materials[{key}]"))
                .unwrap_or_else(|error| panic!("{key}: {error}"));
            if let Surface::Graph(bake) = &definition.surface {
                let graph = graphs
                    .graphs
                    .get(&bake.graph)
                    .unwrap_or_else(|| panic!("{key} names a graph the library lacks"));
                for name in bake.params.keys() {
                    assert!(
                        graph.params.iter().any(|param| &param.name == name),
                        "{key} sets {name}, which {} does not declare",
                        bake.graph
                    );
                }
            }
        }
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "both repeats are copies of one table literal, equal bit for bit"
    )]
    fn every_terrain_definition_names_its_own_graph() {
        let graphs = graphs();
        let terrain = terrain();
        assert_eq!(terrain.materials.len(), TERRAIN.len());
        for (set, metres) in TERRAIN {
            let key = format!("terrain:{set}");
            let definition = terrain
                .materials
                .get(&key)
                .unwrap_or_else(|| panic!("{key} is missing"));
            definition
                .check(&format!("materials[{key}]"))
                .unwrap_or_else(|error| panic!("{key}: {error}"));
            assert_eq!(definition.tile_metres, [metres; 2], "{key}");
            assert!(definition.strands.is_none(), "{key} grows strands");
            let Surface::Graph(bake) = &definition.surface else {
                panic!("{key} is not a graph bake");
            };
            assert_eq!(bake.resolution, TERRAIN_RESOLUTION, "{key}");
            assert_eq!(bake.graph, terrain::graph_key(set), "{key}");
            let graph = graphs
                .get(&bake.graph)
                .unwrap_or_else(|| panic!("{key} names a graph the library lacks"));
            // The repeat the graph was authored against is the one the
            // terrain lays it at, or its relief would be scaled wrongly.
            assert_eq!(graph.tile_metres, Some([metres; 2]), "{}", bake.graph);
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
    fn every_terrain_graph_builds_clean_and_tiles_at_its_resolution() {
        let graphs = graphs();
        for (set, _) in TERRAIN {
            let key = terrain::graph_key(set);
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
                lattice.iter().all(|&l| l <= TERRAIN_RESOLUTION),
                "{key} lays a {lattice:?} lattice"
            );
        }
    }

    #[test]
    fn every_terrain_height_spans_the_unit_without_clipping() {
        use std::{collections::BTreeMap, num::NonZeroUsize};

        use ashlar_material::bake::{BakeRequest, rasterise};

        let graphs = graphs();
        for (set, _) in TERRAIN {
            let key = terrain::graph_key(set);
            let graph = graphs.get(&key).expect("every set has a graph");
            // A quarter of the shipped resolution: the extremes a lattice
            // reaches barely move, and the test stays fast in a debug build.
            let (_, report) = rasterise(&BakeRequest {
                graph,
                library: &graphs,
                params: &BTreeMap::new(),
                resolution: TERRAIN_RESOLUTION / 4,
                mips: false,
                threads: NonZeroUsize::new(4),
            })
            .unwrap_or_else(|error| panic!("{key}: {error}"));
            let (low, high) = report.height_range.expect("a terrain layer binds a height");
            assert!(
                low > 0.0 && high < 1.0,
                "{key} clips its height: {low}..{high}"
            );
            assert!(high - low > 0.2, "{key} is nearly flat: {low}..{high}");
        }
    }

    /// Beyond the detail distance a face is only its layer's mean colour,
    /// so the provinces' five rocks have to differ in that alone: pale
    /// grey, red-buff, near black, blue-grey and ochre-grey, each at least a
    /// visible step from the other four in sRGB. Granite and clay are the
    /// nearest pair (both pale); their provinces' far tints part them further.
    #[test]
    fn the_province_faces_differ_in_their_mean_colour() {
        use std::{collections::BTreeMap, num::NonZeroUsize};

        use ashlar_material::bake::{BakeRequest, linear_to_srgb, rasterise};

        const FACES: [&str; 5] = ["granite", "sandstone", "basalt", "slate", "clay"];
        let graphs = graphs();
        let means: Vec<[f32; 3]> = FACES
            .iter()
            .map(|set| {
                let key = terrain::graph_key(set);
                let graph = graphs.get(&key).expect("every face has a graph");
                let (planes, _) = rasterise(&BakeRequest {
                    graph,
                    library: &graphs,
                    params: &BTreeMap::new(),
                    resolution: TERRAIN_RESOLUTION / 4,
                    mips: false,
                    threads: NonZeroUsize::new(4),
                })
                .unwrap_or_else(|error| panic!("{key}: {error}"));
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a texel count at 256 is exact in an f32"
                )]
                let texels = planes.base_color.len() as f32;
                let mut sum = [0.0_f32; 3];
                for texel in &planes.base_color {
                    for (total, channel) in sum.iter_mut().zip(texel) {
                        *total += channel;
                    }
                }
                sum.map(|total| linear_to_srgb(total / texels))
            })
            .collect();
        for (i, a) in means.iter().enumerate() {
            for (j, b) in means.iter().enumerate().skip(i + 1) {
                let distance = a
                    .iter()
                    .zip(b)
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f32>()
                    .sqrt();
                assert!(
                    distance > 0.07,
                    "{} {a:?} and {} {b:?} are {distance} apart in sRGB",
                    FACES[i],
                    FACES[j]
                );
            }
        }
    }

    #[test]
    fn colours_are_linearised() {
        let ParamValue::Color([r, _, _]) = linear([0.5, 0.0, 1.0]) else {
            unreachable!();
        };
        assert!((r - 0.214).abs() < 1e-3, "{r}");
    }
}
