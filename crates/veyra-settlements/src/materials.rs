//! Port Serein's finishes: every `serein:*` key the settlement's buildings
//! bind, each a default-library graph with our own values.
//!
//! Every key is ours, so a later art pass retunes the city here without
//! touching a recipe, and nothing we bake can collide with a key another
//! content crate ships.
//!
//! The look, after the corporate studies in `docs/research/veyra-style`:
//! charcoal concrete with a green cast, streaked and wet from the sea air;
//! dark steel and salt-bitten rust; wet streets; and light in two colours
//! only, cold teal for work and signage and sodium amber for the street and
//! the lived-in windows.
use std::collections::BTreeMap;

use ashlar::{AntiTiling, MaterialDefinition, MaterialLibrary, ParamValue, Surface};

/// Every `serein:*` definition.
///
/// # Panics
///
/// If the default library stops offering a graph named here, which the
/// crate's tests catch before any content step runs.
#[must_use]
#[expect(clippy::too_many_lines, reason = "one library, key by key")]
pub fn materials() -> MaterialLibrary {
    let library = ashlar_material::stdlib::materials();
    let graph = |base: &str, params: &[(&str, ParamValue)]| {
        let mut definition = library.materials[base].clone();
        if let Surface::Graph(bake) = &mut definition.surface {
            for (name, value) in params {
                bake.params.insert((*name).to_owned(), *value);
            }
        }
        definition
    };
    let color = |rgb: [f32; 3]| ParamValue::Color(rgb);
    let float = ParamValue::Float;
    let lit = |base: &str, base_color: [f32; 3], emissive: [f32; 3]| {
        let mut definition = library.materials[base].clone();
        definition.base_color = base_color;
        definition.emissive = emissive;
        definition
    };
    let window = |emissive: [f32; 3], shift: f32| {
        let mut definition = graph("library:office-window", &[("blinds", float(0.6))]);
        definition.emissive = emissive;
        // A quarter repeat apart, so a lit storey's blinds are not the dark
        // one's.
        definition.uv_offset = [shift, 0.0];
        definition
    };
    let constant = |base_color: [f32; 3], roughness: f32, emissive: [f32; 3]| MaterialDefinition {
        base_color,
        roughness,
        emissive,
        ..MaterialDefinition::default()
    };
    let mut hedge = graph(
        "library:grass",
        &[("lushness", float(0.5)), ("dryness", float(0.45))],
    );
    // A planter's crown seen from the street needs the surface, not strands.
    hedge.strands = None;
    // The Landing's composite: large calm panels, worn but not busy.
    let shell = |rgb: [f32; 3]| {
        let mut definition = graph(
            "library:hull-plating",
            &[("color", color(rgb)), ("wear", float(0.35))],
        );
        definition.tile_scale = Some(2.0);
        definition.tile_metres = [
            definition.tile_metres[0] * 2.0,
            definition.tile_metres[1] * 2.0,
        ];
        definition
    };
    let composite = shell([0.56, 0.52, 0.43]);
    let composite_teal = shell([0.26, 0.4, 0.38]);
    // A city tree's canopy, stylised into lobes: the same surface, lusher.
    let mut canopy = graph(
        "library:grass",
        &[("lushness", float(0.95)), ("dryness", float(0.1))],
    );
    canopy.strands = None;
    let definitions = vec![
        // Charcoal with a green cast, every streak and most of the grime:
        // what salt air leaves on a sea-facing wall.
        (
            "serein:charcoal-concrete",
            graph(
                "library:stained-concrete",
                &[
                    ("color", color([0.05, 0.058, 0.058])),
                    ("streaks", float(1.0)),
                    ("grime", float(0.85)),
                ],
            ),
        ),
        (
            "serein:sea-wall",
            graph(
                "library:stained-concrete",
                &[
                    ("color", color([0.03, 0.033, 0.03])),
                    ("streaks", float(1.0)),
                    ("grime", float(1.0)),
                ],
            ),
        ),
        ("serein:recess", graph("library:dark-recess", &[])),
        (
            "serein:dark-steel",
            graph(
                "library:painted-metal",
                &[("color", color([0.028, 0.038, 0.04])), ("wear", float(0.5))],
            ),
        ),
        (
            "serein:painted-steel",
            graph(
                "library:painted-metal",
                &[("color", color([0.03, 0.12, 0.11])), ("wear", float(0.65))],
            ),
        ),
        // Planters and the ledge beds, clipped.
        ("serein:hedge", hedge),
        // The company's teal neon, on the headquarters and the terminal.
        (
            "serein:neon-teal",
            lit("library:emissive-strip", [0.12; 3], [0.3, 6.5, 5.2]),
        ),
        // The structure of the blockout (`crate::blockout`): the settlers'
        // terrace walls in a warm grey concrete, streaked but kept; the
        // company's darker cast concrete; the pads' pale decking; pipes in
        // faded cream paint; and the orange Pelagic marks its hazards with.
        (
            "serein:terrace-wall",
            graph(
                "library:stained-concrete",
                &[
                    ("color", color([0.38, 0.355, 0.31])),
                    ("streaks", float(0.5)),
                    ("grime", float(0.3)),
                ],
            ),
        ),
        // The terrace walls' stone: the library's coursed limestone in the
        // warm grey of the hillside it was cut from.
        (
            "serein:terrace-masonry",
            graph(
                "library:ashlar-blocks",
                &[
                    ("variation", float(0.6)),
                    ("color", color([0.34, 0.31, 0.26])),
                ],
            ),
        ),
        (
            "serein:company-concrete",
            graph(
                "library:formed-concrete",
                &[("color", color([0.085, 0.09, 0.092]))],
            ),
        ),
        (
            "serein:pad-deck",
            graph(
                "library:formed-concrete",
                &[("color", color([0.2, 0.2, 0.19])), ("ties", float(0.0))],
            ),
        ),
        (
            "serein:pipe",
            graph(
                "library:painted-metal",
                &[("color", color([0.42, 0.38, 0.3])), ("wear", float(0.55))],
            ),
        ),
        (
            "serein:hazard",
            graph(
                "library:painted-metal",
                &[("color", color([0.6, 0.2, 0.025])), ("wear", float(0.3))],
            ),
        ),
        // The housing slabs' frame: fins, floor bands and galleries cast
        // paler than the charcoal they hold, so a slab reads as a grid.
        (
            "serein:grid-concrete",
            graph(
                "library:formed-concrete",
                &[("color", color([0.4, 0.4, 0.38]))],
            ),
        ),
        // The exchange towers in a mid-grey cast concrete, lighter than the
        // company's buildings so their curve reads against the sky.
        (
            "serein:tower-concrete",
            graph(
                "library:stained-concrete",
                &[
                    ("color", color([0.2, 0.2, 0.19])),
                    ("streaks", float(0.8)),
                    ("grime", float(0.5)),
                ],
            ),
        ),
        // The landmarks (`crate::landmarks`): the Landing's composite gone
        // ivory with age, the terminal's new white composite, its curtain
        // glazing and white steel.
        ("serein:composite", composite),
        (
            "serein:composite-new",
            graph(
                "library:hull-plating",
                &[("color", color([0.74, 0.76, 0.76])), ("wear", float(0.08))],
            ),
        ),
        (
            "serein:curtain",
            graph("library:curtain-wall", &[("lit", float(0.04))]),
        ),
        (
            "serein:white-steel",
            graph(
                "library:painted-metal",
                &[("color", color([0.72, 0.74, 0.74])), ("wear", float(0.15))],
            ),
        ),
        (
            "serein:glass",
            graph("library:glass", &[("smudge", float(0.4))]),
        ),
        // The Terraces' faded paint over render, owner by owner, and the
        // fabric of their awnings: the palette the design gives them.
        (
            "serein:paint-cream",
            graph("library:adobe", &[("color", color([0.55, 0.5, 0.4]))]),
        ),
        (
            "serein:paint-ochre",
            graph("library:adobe", &[("color", color([0.5, 0.32, 0.12]))]),
        ),
        (
            "serein:paint-oxide",
            graph("library:adobe", &[("color", color([0.36, 0.13, 0.07]))]),
        ),
        (
            "serein:paint-green",
            graph("library:adobe", &[("color", color([0.21, 0.29, 0.18]))]),
        ),
        (
            "serein:paint-teal",
            graph("library:adobe", &[("color", color([0.17, 0.3, 0.3]))]),
        ),
        (
            "serein:paint-white",
            graph("library:adobe", &[("color", color([0.62, 0.61, 0.56]))]),
        ),
        // Window glass with a room behind it: the library's office window,
        // dark reflective glazing over blinds and furniture, box-mapped so
        // each pane shows its own part of the room. Unlit, it is glass that
        // gives back the sky with a home behind it rather than a hole.
        ("serein:pane", window([0.0; 3], 0.0)),
        ("serein:composite-teal", composite_teal),
        (
            "serein:boards",
            graph(
                "library:painted-boards",
                // Young enough that the brown paint holds: older, the film
                // weathers off to silvered timber, which under the sky read
                // as a pale mint across every stall and bench.
                &[("color", color([0.42, 0.28, 0.16])), ("age", float(0.25))],
            ),
        ),
        ("serein:canopy", canopy),
        // Cargo containers, in the few colours a fleet comes in.
        (
            "serein:container-rust",
            graph(
                "library:painted-metal",
                &[("color", color([0.42, 0.14, 0.07])), ("wear", float(0.6))],
            ),
        ),
        (
            "serein:container-blue",
            graph(
                "library:painted-metal",
                &[("color", color([0.1, 0.2, 0.36])), ("wear", float(0.45))],
            ),
        ),
        (
            "serein:container-green",
            graph(
                "library:painted-metal",
                &[("color", color([0.14, 0.3, 0.18])), ("wear", float(0.5))],
            ),
        ),
        (
            "serein:container-grey",
            graph(
                "library:painted-metal",
                &[("color", color([0.36, 0.37, 0.37])), ("wear", float(0.4))],
            ),
        ),
        (
            "serein:container-white",
            graph(
                "library:painted-metal",
                &[("color", color([0.62, 0.62, 0.6])), ("wear", float(0.35))],
            ),
        ),
        // The night: lit glass and lamps at their full-night brightness. The
        // game dims every emissive material by day (Bevy draws emission
        // without the camera's exposure), so these are what a dark street
        // sees. A lamp-lit home and a screen-lit one are the same room, its
        // light the emissive map's, coloured and scaled here.
        //
        // Over the bloom threshold (lighting plan phase 5b): the camera's
        // bloom takes what is over 2 in exposed HDR units, and a material's
        // emission is in those units as it stands (its exposure weight is
        // Bevy's default of 0, so the night's EV 3 does not enter). A lit
        // window's map averages about 0.3, brighter towards the ceiling, so
        // these put a warm window's brightest channel at about 3.6 on
        // average and a cool one's at 3.0, eight and eleven times what they
        // were, when they sat at 0.5 and never reached the threshold. The
        // lamps under 3 are lifted into the same band and those over it are
        // left. The halo this buys round a window a few pixels wide is
        // faint at the look's bloom of 0.03 (the render look study's phase
        // 5b section has the measurements). The game keeps all of it under
        // the threshold by day (`DAY_GLOW` in the game's settlement module).
        ("serein:pane-warm", window([12.0, 6.56, 2.4], 0.5)),
        ("serein:pane-cool", window([6.05, 8.25, 9.9], 0.75)),
        (
            "serein:shop-lit",
            constant([0.2, 0.16, 0.12], 0.8, [3.6, 2.2, 0.9]),
        ),
        (
            "serein:slit-lit",
            constant([0.05, 0.06, 0.07], 0.9, [2.25, 3.0, 3.5]),
        ),
        (
            "serein:lantern",
            constant([0.9, 0.75, 0.5], 0.4, [7.0, 4.2, 1.5]),
        ),
        (
            "serein:rail-lamp",
            constant([0.8, 0.85, 0.9], 0.4, [2.7, 3.15, 3.6]),
        ),
        (
            "serein:pad-light",
            constant([0.5, 0.7, 0.9], 0.4, [1.2, 2.6, 5.0]),
        ),
        (
            "serein:floodlight",
            constant([0.9, 0.9, 0.9], 0.3, [6.0, 6.2, 6.6]),
        ),
        (
            "serein:awning-red",
            constant([0.5, 0.12, 0.06], 0.9, [0.0; 3]),
        ),
        (
            "serein:awning-teal",
            constant([0.08, 0.3, 0.3], 0.9, [0.0; 3]),
        ),
        (
            "serein:awning-sand",
            constant([0.62, 0.5, 0.26], 0.9, [0.0; 3]),
        ),
        (
            "serein:corrugated",
            graph("library:corrugated-steel", &[("age", float(0.15))]),
        ),
        (
            "serein:warning",
            constant([0.35, 0.02, 0.01], 0.5, [14.0, 0.5, 0.15]),
        ),
        (
            "serein:beacon-lamp",
            constant([0.9, 0.8, 0.6], 0.3, [24.0, 17.0, 8.0]),
        ),
        // The Terraces' joinery: window frames, louvred shutters, doors,
        // railings and downpipes in painted steel, owner by owner, a little
        // deeper than the walls they stand against. Boards read as brick at
        // the size of a door leaf.
        (
            "serein:joinery-green",
            graph(
                "library:painted-metal",
                &[("color", color([0.13, 0.24, 0.14])), ("wear", float(0.3))],
            ),
        ),
        (
            "serein:joinery-teal",
            graph(
                "library:painted-metal",
                &[("color", color([0.07, 0.24, 0.26])), ("wear", float(0.3))],
            ),
        ),
        (
            "serein:joinery-oxide",
            graph(
                "library:painted-metal",
                &[("color", color([0.32, 0.09, 0.05])), ("wear", float(0.35))],
            ),
        ),
        (
            "serein:joinery-white",
            graph(
                "library:painted-metal",
                &[("color", color([0.66, 0.66, 0.62])), ("wear", float(0.25))],
            ),
        ),
        (
            "serein:joinery-ochre",
            graph(
                "library:painted-metal",
                &[("color", color([0.52, 0.34, 0.09])), ("wear", float(0.3))],
            ),
        ),
        (
            "serein:joinery-blue",
            graph(
                "library:painted-metal",
                &[("color", color([0.1, 0.17, 0.32])), ("wear", float(0.3))],
            ),
        ),
        // Solar panels: dark blue glass, smooth enough to catch the sky.
        (
            "serein:solar",
            MaterialDefinition {
                base_color: [0.025, 0.04, 0.09],
                roughness: 0.12,
                metallic: 0.3,
                ..MaterialDefinition::default()
            },
        ),
    ];
    MaterialLibrary {
        materials: definitions
            .into_iter()
            .map(|(key, mut definition)| {
                definition.anti_tiling = anti_tiling(key);
                (key.to_owned(), definition)
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

/// How a city surface hides its repeat, which on a long wall or a whole
/// hillside of houses would otherwise show as a lattice.
///
/// Paint over render and planting have no structure at their repeat's scale,
/// so they are read through stochastic cells as the terrain is. Concrete,
/// panels, sheet and boards have joints, form lines or ribs that
/// are the architecture's own rhythm, so they only take the slow world-space
/// variation, which varies them building to building without cutting a
/// joint. Glass, paint on metal, cloth, lights and constants keep none.
fn anti_tiling(key: &str) -> Option<AntiTiling> {
    const STOCHASTIC: [&str; 3] = ["paint-", "serein:hedge", "serein:canopy"];
    const MACRO: [&str; 9] = [
        "concrete",
        "serein:terrace-wall",
        "serein:terrace-masonry",
        "serein:sea-wall",
        "serein:pad-deck",
        "serein:composite",
        "serein:corrugated",
        "serein:boards",
        "serein:container-",
    ];
    if STOCHASTIC.iter().any(|p| key.contains(p)) {
        Some(AntiTiling::default())
    } else if MACRO.iter().any(|p| key.contains(p)) {
        Some(AntiTiling::macro_only())
    } else {
        None
    }
}

/// The graphs [`materials`] needs beyond `veyra_materials::graphs()`: none,
/// because every `serein:*` definition parameterises a default-library graph,
/// and Veyra's graph library already carries the default one. It is kept as a
/// function so a settlement graph added later has a place to go that the
/// content step already reads.
#[must_use]
pub fn graphs() -> ashlar_material::MaterialGraphLibrary {
    ashlar_material::MaterialGraphLibrary::default()
}
