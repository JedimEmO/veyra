//! What stands on a procedural site (ADR 0012, decision 6; ticket 0005,
//! step 4): for each site kind's building group, a few arrangements of the
//! existing kits on the kind's level pad, so a place can be seen from a
//! distance and found again.
//!
//! A procedural site moves with the world's generator, so nothing here is
//! fitted to one site. An arrangement is laid out in the site's frame
//! (origin at the pad's centre, `+z` the site's facing, `+x` to its right,
//! as a settlement's frame is) on the pad alone. The pad's shape is the
//! kind's (its radius and crown); only its level differs from site to site,
//! and the game and the server stand each building on the pad's own height
//! where it stands. So one bake serves every site of the kind.
//!
//! Each design is also offered mirrored across the facing: the road's
//! arrival can be on either side, and a design that leaves one side open
//! leaves the other open mirrored. Which arrangement a site gets is the
//! world's choice (`strata_worldgen::sites::place_group`), seeded by the
//! site's id, so the server and every client make the same one.
//!
//! The pieces are the city's: the Terraces' houses and workshop, the
//! Landing's composite modules, the beacon tower as a survey mast, cargo
//! containers and street furniture, the study kit's outpost as a depot,
//! and two wayside fittings (`veyra_buildings::port::Fitting::Board` and
//! `Marker`). Nothing here is a settlement generator: every design is a
//! handful of hand-placed kits round a yard.

use std::collections::BTreeMap;

use anyhow::Context;
use ashlar::Building;
use ashlar::glam::DVec3;
use veyra_buildings::port::Fitting;

use crate::blockout::{PlacedKit, kits_building};
use crate::landmarks;
use crate::types::house::roof;
use crate::types::{self, Containers, Furniture, Kit, LandingModule, Style, TerraceHouse, Top};

/// Where the groups' own files are written, under `generated/`.
pub const DIR: &str = "sites/veyra";

/// The outpost the study kit bakes for the grounding fixture, reused as a
/// depot: it is already baked, so a town pays nothing for it.
pub const DEPOT: &str = "buildings/outpost.ashlar";

/// One kind's building group, ready to bake.
#[derive(Clone, Debug)]
pub struct SiteGroup {
    /// The group's name, as a site kind's `group` names it.
    pub group: &'static str,
    /// The pad radius the designs were laid out for, metres: every footprint
    /// keeps a metre inside it. The content step checks it against the
    /// kind's own `footprint_m`.
    pub designed_radius_m: f64,
    /// The open ground round the pad's centre a road ends in, metres.
    pub yard_m: f64,
    /// Every file the arrangements stand, by its path under `generated/`,
    /// with its recipe. Files are shared between groups; the same path is
    /// always the same recipe.
    pub files: Vec<(String, Building)>,
    /// The arrangements, each design followed by its mirror image.
    pub arrangements: Vec<Arrangement>,
    /// The name board stood at the road's arrival, if the group has one.
    pub sign: Option<Sign>,
}

/// One arrangement of a group.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrangement {
    /// Its name, for logs and reviews.
    pub name: String,
    /// Its buildings.
    pub buildings: Vec<Placed>,
}

/// One building of an arrangement, in the site's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The baked file, under `generated/`.
    pub file: String,
    /// Where the file's origin stands, `(x, z)`.
    pub position: [f64; 2],
    /// Its turn about `+y`, radians, as `Quat::from_rotation_y` turns.
    pub yaw: f64,
    /// The corners of the ground it covers, `(x, z)`, in order round it:
    /// its plan bounds, turned. Conservative: never smaller than the solid.
    pub footprint: [[f64; 2]; 4],
}

/// The name board's file and the ground it covers about its origin.
#[derive(Clone, Debug, PartialEq)]
pub struct Sign {
    /// The baked file, under `generated/`.
    pub file: String,
    /// Half its plan bounds along its `x` and `z`, about its origin.
    pub half_size: [f64; 2],
}

/// A file of the catalogue: where it is written, its recipe and its plan
/// bounds in its own frame, `[min_x, min_z, max_x, max_z]`.
struct File {
    path: String,
    building: Building,
    bounds: [f64; 4],
}

/// A building's plan bounds in its own frame, `[min_x, min_z, max_x,
/// max_z]`: every element's conservative box, through its instance's pose.
///
/// # Errors
///
/// An instance naming a part the building lacks, or an element whose
/// geometry does not check.
pub fn plan_bounds(building: &Building) -> anyhow::Result<[f64; 4]> {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for instance in &building.recipe().instances {
        let part = building
            .part(&instance.part)
            .with_context(|| format!("instance {} has no part", instance.id))?;
        for element in &part.elements {
            let [lo, hi] = element
                .geometry
                .bounds()
                .with_context(|| format!("{}/{} has no bounds", part.id, element.id))?;
            for x in [lo.x, hi.x] {
                for y in [lo.y, hi.y] {
                    for z in [lo.z, hi.z] {
                        let p = instance.pose.transform_point(DVec3::new(x, y, z));
                        b = [b[0].min(p.x), b[1].min(p.z), b[2].max(p.x), b[3].max(p.z)];
                    }
                }
            }
        }
    }
    anyhow::ensure!(b[0] <= b[2], "an empty building");
    Ok(b)
}

/// `p` turned by `yaw` about `+y`, as `Quat::from_rotation_y` turns `(x, 0,
/// z)`.
fn turn(p: [f64; 2], yaw: f64) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [p[0] * c + p[1] * s, -p[0] * s + p[1] * c]
}

impl File {
    fn new(path: String, building: Building) -> anyhow::Result<Self> {
        let bounds = plan_bounds(&building).with_context(|| path.clone())?;
        Ok(Self {
            path,
            building,
            bounds,
        })
    }

    /// The file stood with the middle of its plan bounds at `centre`,
    /// turned by `yaw`.
    fn place(&self, centre: [f64; 2], yaw: f64) -> Placed {
        let [x0, z0, x1, z1] = self.bounds;
        let middle = turn([f64::midpoint(x0, x1), f64::midpoint(z0, z1)], yaw);
        let position = [centre[0] - middle[0], centre[1] - middle[1]];
        let footprint = [[x0, z0], [x1, z0], [x1, z1], [x0, z1]].map(|corner| {
            let t = turn(corner, yaw);
            [position[0] + t[0], position[1] + t[1]]
        });
        Placed {
            file: self.path.clone(),
            position,
            yaw,
            footprint,
        }
    }
}

/// A building of a design: a file of the catalogue, the middle of its plan
/// at `(x, z)` in the site frame, turned by a yaw. A kit's front is its
/// `+z`, so a yaw of nought faces the site's facing, `PI` its back, a
/// quarter turn `+x`.
type Spec = (&'static str, [f64; 2], f64);

/// One design: its name and its buildings.
type Design = (&'static str, Vec<Spec>);

/// A kit as a file of its own, with its owner's colours.
fn kit_file(
    name: &str,
    kit: Kit,
    bindings: Vec<(&'static str, &'static str)>,
) -> anyhow::Result<File> {
    let planned = kits_building(
        &format!("site-{name}"),
        &[PlacedKit {
            kit,
            at: [0.0; 3],
            yaw: 0.0,
            footprint: [0.0; 4],
            bindings,
        }],
    )?;
    File::new(format!("{DIR}/{name}.ashlar"), planned.building)
}

/// A frontier house of the Terraces' catalogue, its owner's colours bound.
fn house(
    name: &str,
    house: TerraceHouse,
    [paint, trim, joinery, pane, fabric]: [&'static str; 5],
) -> anyhow::Result<File> {
    kit_file(
        name,
        Kit::House(house),
        vec![
            ("paint", paint),
            ("trim", trim),
            ("joinery", joinery),
            ("pane", pane),
            ("fabric", fabric),
        ],
    )
}

/// Every file the groups stand, by its short name.
#[allow(clippy::too_many_lines, reason = "the catalogue, file by file")]
fn catalogue() -> anyhow::Result<BTreeMap<&'static str, File>> {
    let mut files = BTreeMap::new();
    let plain = TerraceHouse {
        width: 8.0,
        depth: 8.0,
        lower: 1,
        style: Style::Shuttered,
        workshop: false,
        upper: None,
        shed: None,
        awning: false,
        balcony: false,
        top: Top::Flat,
        roof: 0,
    };
    files.insert(
        "house-a",
        house(
            "house-a",
            TerraceHouse {
                lower: 2,
                shed: Some(true),
                awning: true,
                balcony: true,
                roof: roof::TANK | roof::SOLAR | roof::WASHING | roof::AERIAL,
                ..plain
            },
            [
                "serein:paint-cream",
                "serein:terrace-wall",
                "serein:joinery-green",
                "serein:pane-warm",
                "serein:awning-sand",
            ],
        )?,
    );
    files.insert(
        "house-b",
        house(
            "house-b",
            TerraceHouse {
                width: 10.0,
                depth: 6.0,
                style: Style::Casement,
                upper: Some(types::Upper {
                    width: 6.0,
                    depth: 4.0,
                    shift: 1.0,
                }),
                roof: roof::DISH | roof::AC | roof::GARDEN,
                ..plain
            },
            [
                "serein:paint-ochre",
                "serein:paint-white",
                "serein:joinery-blue",
                "serein:pane",
                "serein:awning-teal",
            ],
        )?,
    );
    files.insert(
        "house-c",
        house(
            "house-c",
            TerraceHouse {
                width: 6.0,
                lower: 2,
                balcony: true,
                top: Top::Vault,
                roof: roof::AC,
                ..plain
            },
            [
                "serein:paint-white",
                "serein:paint-cream",
                "serein:joinery-teal",
                "serein:pane-cool",
                "serein:awning-red",
            ],
        )?,
    );
    files.insert(
        "house-d",
        house(
            "house-d",
            TerraceHouse {
                depth: 6.0,
                style: Style::Casement,
                shed: Some(false),
                awning: true,
                top: Top::Lean,
                roof: roof::SOLAR | roof::DISH,
                ..plain
            },
            [
                "serein:paint-teal",
                "serein:terrace-wall",
                "serein:joinery-oxide",
                "serein:pane-warm",
                "serein:awning-sand",
            ],
        )?,
    );
    files.insert(
        "workshop",
        house(
            "workshop",
            TerraceHouse {
                width: types::house::WORKSHOP,
                workshop: true,
                awning: true,
                roof: roof::SOLAR | roof::TANK,
                ..plain
            },
            [
                "serein:paint-oxide",
                "serein:terrace-wall",
                "serein:joinery-white",
                "serein:pane",
                "serein:awning-sand",
            ],
        )?,
    );
    let module = |width: f64, depth: f64, shop: bool, dome: bool| LandingModule {
        width,
        depth,
        storeys: 1,
        shop,
        awning: true,
        dome,
    };
    files.insert(
        "hut",
        kit_file(
            "hut",
            Kit::Module(module(6.0, 5.0, false, true)),
            vec![
                ("shell", "serein:composite"),
                ("sign", "serein:paint-oxide"),
            ],
        )?,
    );
    files.insert(
        "office",
        kit_file(
            "office",
            Kit::Module(module(10.0, 8.0, true, false)),
            vec![
                ("shell", "serein:composite-teal"),
                ("sign", "serein:paint-ochre"),
                ("pane", "serein:pane-warm"),
            ],
        )?,
    );
    files.insert(
        "containers-a",
        kit_file(
            "containers-a",
            Kit::Containers(Containers {
                length: 6.1,
                high: 2,
                slide: 0.4,
            }),
            vec![
                ("container-0", "serein:container-rust"),
                ("container-1", "serein:container-blue"),
            ],
        )?,
    );
    files.insert(
        "containers-b",
        kit_file(
            "containers-b",
            Kit::Containers(Containers {
                length: 12.2,
                high: 1,
                slide: 0.0,
            }),
            vec![("container-0", "serein:container-green")],
        )?,
    );
    for (name, furniture) in [
        ("kiosk", Furniture::Kiosk),
        ("bench", Furniture::Bench),
        ("lamp", Furniture::Lamp),
        ("crates", Furniture::Crates),
    ] {
        files.insert(name, kit_file(name, Kit::Furniture(furniture), Vec::new())?);
    }
    for (name, furniture, cloth) in [
        ("tent-a", Furniture::Tent, "serein:awning-sand"),
        ("tent-b", Furniture::Tent, "serein:awning-red"),
        ("tent-c", Furniture::Tent, "serein:awning-teal"),
    ] {
        files.insert(
            name,
            kit_file(name, Kit::Furniture(furniture), vec![("fabric", cloth)])?,
        );
    }
    for (name, furniture) in [
        ("fire", Furniture::FirePit),
        ("lookout", Furniture::Lookout),
        ("pipes", Furniture::Pipes),
        ("mast-stub", Furniture::BrokenMast),
    ] {
        files.insert(name, kit_file(name, Kit::Furniture(furniture), Vec::new())?);
    }
    // Burnt plating, rust and not new sheet.
    for (name, furniture) in [
        ("hull", Furniture::HullSection),
        ("plate", Furniture::HullPlate),
    ] {
        files.insert(
            name,
            kit_file(
                name,
                Kit::Furniture(furniture),
                vec![("sheet", "serein:container-rust")],
            )?,
        );
    }
    for (name, fitting) in [
        ("terminal", Fitting::Terminal),
        ("bollard", Fitting::Bollard),
        ("board", Fitting::Board),
        ("marker", Fitting::Marker),
    ] {
        files.insert(name, kit_file(name, Kit::Dressing(fitting), Vec::new())?);
    }
    files.insert(
        "mast",
        File::new(format!("{DIR}/mast.ashlar"), landmarks::beacon(MAST_M)?)?,
    );
    files.insert(
        "depot",
        File::new(
            DEPOT.to_owned(),
            veyra_buildings::building(veyra_buildings::Scene::Outpost)?,
        )?,
    );
    Ok(files)
}

/// The survey mast's height, metres: the city's beacon at well under half
/// its height, tall enough to mark a station on a hill from kilometres off.
const MAST_M: f64 = 20.0;

/// A quarter turn.
const QUARTER: f64 = std::f64::consts::FRAC_PI_2;
/// A half turn.
const HALF: f64 = std::f64::consts::PI;

/// The study kit's outpost has its front on `-z`: this turn faces it
/// forward.
const DEPOT_FORWARD: f64 = HALF;

/// The groups' designs: `(group, designed radius, yard, sign, designs)`.
///
/// Each group's designs leave different sides open: a way in from the back
/// and the sides, one from the front, one from a side (and mirrored, the
/// other), so wherever a site's road arrives one of them keeps the way to
/// the yard clear (`content`'s `a_road_from_any_side_finds_a_way_in`).
#[allow(clippy::too_many_lines, reason = "the groups, design by design")]
fn designs() -> Vec<(&'static str, f64, f64, bool, Vec<Design>)> {
    vec![
        (
            // A small depot and prefabricated houses round a yard, the
            // depot's containers and crates beside it, lamps over the yard.
            "veyra:town_site",
            50.0,
            14.0,
            true,
            vec![
                (
                    // Depot across the back, two houses down each side,
                    // open to the water.
                    "yard",
                    vec![
                        ("depot", [0.0, -24.0], DEPOT_FORWARD),
                        ("containers-a", [14.0, -25.0], 0.0),
                        ("containers-b", [-16.0, -26.0], 0.0),
                        ("crates", [-9.0, -16.0], 0.0),
                        ("house-a", [-24.0, -6.0], QUARTER),
                        ("house-c", [-25.0, 6.0], QUARTER),
                        ("house-b", [23.0, -4.0], -QUARTER),
                        ("house-d", [25.0, 7.0], -QUARTER),
                        ("kiosk", [-14.0, 20.0], QUARTER),
                        ("bench", [12.0, 18.0], 0.0),
                        ("lamp", [-9.0, 10.0], 0.0),
                        ("lamp", [9.0, 10.0], 0.0),
                    ],
                ),
                (
                    // A row along the water, the depot at its end, the yard
                    // behind open to the back and both sides.
                    "row",
                    vec![
                        ("house-a", [-20.0, 22.0], 0.0),
                        ("house-b", [-6.0, 24.0], 0.0),
                        ("house-c", [6.0, 22.0], 0.0),
                        ("depot", [20.0, 24.0], DEPOT_FORWARD),
                        ("containers-a", [-30.0, 10.0], QUARTER),
                        ("crates", [27.0, 14.0], 0.0),
                        ("lamp", [-8.0, 12.0], 0.0),
                        ("lamp", [8.0, 12.0], 0.0),
                    ],
                ),
                (
                    // The depot and an office across the back, houses down
                    // one side, open to the water and the other side.
                    "corner",
                    vec![
                        ("depot", [-16.0, -20.0], DEPOT_FORWARD),
                        ("office", [2.0, -22.0], 0.0),
                        ("containers-b", [-2.0, -32.0], 0.0),
                        ("crates", [-8.0, -11.0], 0.0),
                        ("house-d", [-24.0, 5.0], QUARTER),
                        ("house-b", [-22.0, 20.0], QUARTER),
                        ("kiosk", [-11.0, 31.0], 0.0),
                        ("bench", [-14.0, 11.0], QUARTER),
                        ("lamp", [-9.0, 8.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // Three houses by the water, a water kiosk, a bench and lamps.
            "veyra:hamlet",
            34.0,
            9.0,
            true,
            vec![
                (
                    "waterfront",
                    vec![
                        ("house-a", [-14.0, 13.0], 0.0),
                        ("house-b", [0.0, 15.0], 0.0),
                        ("house-c", [13.0, 12.0], 0.0),
                        ("kiosk", [-17.0, -3.0], QUARTER),
                        ("bench", [-6.0, 23.0], 0.0),
                        ("crates", [18.0, 5.0], 0.0),
                        ("lamp", [-6.0, 6.0], 0.0),
                        ("lamp", [7.0, 5.5], 0.0),
                    ],
                ),
                (
                    "lane",
                    vec![
                        ("house-c", [-12.0, 6.0], QUARTER),
                        ("house-a", [-12.0, -8.0], QUARTER),
                        ("house-d", [12.0, 0.0], -QUARTER),
                        ("kiosk", [7.0, -13.0], -QUARTER),
                        ("bench", [6.0, 10.0], -QUARTER),
                        ("lamp", [-6.0, 12.0], 0.0),
                    ],
                ),
                (
                    "corner",
                    vec![
                        ("house-b", [-6.0, -15.0], 0.0),
                        ("workshop", [10.0, -16.0], 0.0),
                        ("house-a", [-18.0, 3.0], QUARTER),
                        ("kiosk", [-15.0, 14.0], QUARTER),
                        ("bench", [-8.0, 12.0], QUARTER),
                        ("lamp", [2.0, -8.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // A survey mast and a hut, a public terminal by its door, a
            // container of equipment.
            "veyra:monitoring_station",
            28.0,
            7.0,
            true,
            vec![
                (
                    "pair",
                    vec![
                        ("mast", [-10.0, -9.0], 0.0),
                        ("hut", [10.0, -9.0], 0.0),
                        ("terminal", [6.0, -4.0], 0.0),
                        ("containers-a", [-14.0, 6.0], QUARTER),
                        ("lamp", [4.5, 6.0], 0.0),
                    ],
                ),
                (
                    "mast-behind",
                    vec![
                        ("mast", [0.0, -14.0], 0.0),
                        ("hut", [-12.0, 3.0], QUARTER),
                        ("terminal", [-8.0, 8.5], QUARTER),
                        ("crates", [-9.0, -6.0], 0.0),
                        ("lamp", [5.0, 9.0], 0.0),
                    ],
                ),
                (
                    "mast-aside",
                    vec![
                        ("mast", [14.0, 2.0], -QUARTER),
                        ("hut", [-6.0, -12.0], 0.0),
                        ("containers-a", [-17.0, -2.0], QUARTER),
                        ("lamp", [-1.0, -8.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // A shed by the water, with its gear and its bollards.
            "veyra:landing",
            32.0,
            7.0,
            true,
            vec![
                (
                    "slip",
                    vec![
                        ("workshop", [0.0, 15.0], 0.0),
                        ("bollard", [-5.0, 24.0], 0.0),
                        ("bollard", [5.0, 24.0], 0.0),
                        ("containers-a", [-13.0, 9.0], 0.0),
                        ("crates", [-10.0, 3.0], 0.0),
                        ("lamp", [9.0, 8.0], 0.0),
                    ],
                ),
                (
                    "aside",
                    vec![
                        ("workshop", [-13.0, 11.0], 0.0),
                        ("containers-a", [13.0, 7.0], QUARTER),
                        ("bollard", [-11.0, 24.0], 0.0),
                        ("bollard", [-6.0, 25.0], 0.0),
                        ("crates", [-4.0, 4.0], 0.0),
                        ("lamp", [-4.5, 18.0], 0.0),
                    ],
                ),
                (
                    "end-on",
                    vec![
                        ("workshop", [-14.0, 2.0], QUARTER),
                        ("containers-a", [-12.0, -16.0], 0.0),
                        ("bollard", [5.0, 25.0], 0.0),
                        ("bollard", [10.0, 24.0], 0.0),
                        ("crates", [-7.0, -9.0], 0.0),
                        ("lamp", [-6.0, 14.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // A refuge hut in the saddle, a bench and a lamp.
            "veyra:pass_shelter",
            20.0,
            5.0,
            true,
            vec![
                (
                    "back",
                    vec![
                        ("hut", [0.0, -10.0], 0.0),
                        ("bench", [5.5, -4.0], 0.0),
                        ("lamp", [-5.0, -5.5], 0.0),
                    ],
                ),
                (
                    "side",
                    vec![
                        ("hut", [-10.0, 0.0], QUARTER),
                        ("bench", [-6.0, 7.0], QUARTER),
                        ("lamp", [-6.0, -6.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // Tents round a fire, crates and a container as a barricade, a
            // lookout over the way in. No board: nobody posts a name.
            "veyra:outlaw_camp",
            26.0,
            8.0,
            false,
            vec![
                (
                    // Tents in an arc round the fire, the lookout and the
                    // containers behind it, open to the front and sides.
                    "ring",
                    vec![
                        ("fire", [0.0, 0.0], 0.0),
                        ("tent-a", [-8.0, -3.0], QUARTER),
                        ("tent-b", [8.5, -2.0], -QUARTER),
                        ("tent-c", [-3.0, -10.0], 0.0),
                        ("lookout", [10.0, -12.0], 0.0),
                        ("containers-a", [-1.0, -19.0], 0.0),
                        ("crates", [5.0, -8.0], 0.0),
                    ],
                ),
                (
                    // A line of tents down one side, the barricade across
                    // the back, open to the front and the other side.
                    "line",
                    vec![
                        ("fire", [4.0, 3.0], 0.0),
                        ("tent-b", [-10.0, 12.0], QUARTER),
                        ("tent-a", [-10.0, 3.0], QUARTER),
                        ("tent-c", [-10.0, -6.0], QUARTER),
                        ("containers-b", [2.0, -17.0], 0.0),
                        ("lookout", [-14.0, -15.0], 0.0),
                        ("crates", [-3.0, -10.0], 0.0),
                    ],
                ),
                (
                    // Tents between the fire and the way out front, the
                    // lookout aside, open at the back and the sides.
                    "hollow",
                    vec![
                        ("fire", [0.0, -4.0], 0.0),
                        ("tent-c", [-6.0, 8.0], 0.0),
                        ("tent-a", [6.0, 8.0], 0.0),
                        ("tent-b", [0.0, 14.0], HALF),
                        ("lookout", [-15.0, 4.0], 0.0),
                        ("crates", [12.0, 2.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // A freighter's hull in pieces, half sunk, its cargo thrown
            // about and a container or two still upright.
            "veyra:wreck",
            40.0,
            9.0,
            false,
            vec![
                (
                    // Two sections nose to tail across the back, containers
                    // and crates strewn toward the front, open at the sides.
                    "broken-back",
                    vec![
                        ("hull", [-9.0, -20.0], 0.2),
                        ("plate", [8.0, -22.0], -0.35),
                        ("containers-a", [-20.0, -8.0], 0.7),
                        ("containers-b", [18.0, -6.0], -0.4),
                        ("crates", [-4.0, -9.0], 0.5),
                        ("crates", [6.0, 4.0], 1.9),
                        ("hut", [-14.0, -30.0], 1.1),
                    ],
                ),
                (
                    // A section on its side down the left, a split one
                    // beyond, cargo spilled across the middle; open to the
                    // front and the right.
                    "long-side",
                    vec![
                        ("hull", [-22.0, 4.0], QUARTER + 0.15),
                        ("plate", [-18.0, -16.0], QUARTER - 0.5),
                        ("containers-b", [0.0, -22.0], 0.3),
                        ("containers-a", [10.0, -12.0], -1.0),
                        ("crates", [-8.0, -4.0], 2.4),
                        ("crates", [4.0, -6.0], 0.4),
                    ],
                ),
                (
                    // Everything thrown forward of the pad's middle, so a
                    // road from behind finds the yard clear.
                    "thrown-forward",
                    vec![
                        ("hull", [-14.0, 20.0], 0.3),
                        ("plate", [14.0, 24.0], -0.6),
                        ("containers-a", [-24.0, 2.0], 1.2),
                        ("containers-b", [22.0, 0.0], 0.2),
                        ("crates", [-6.0, 12.0], 0.9),
                        ("crates", [8.0, 11.0], 2.2),
                    ],
                ),
                (
                    // One great section across the way in's far side with
                    // a cockpit module thrown clear; open at the front and
                    // the sides.
                    "nose-and-cargo",
                    vec![
                        ("hull", [0.0, -24.0], 0.1),
                        ("hut", [18.0, -16.0], -0.8),
                        ("plate", [-22.0, -22.0], 0.6),
                        ("containers-a", [-18.0, -14.0], 0.5),
                        ("containers-b", [-12.0, 2.0], -0.2),
                        ("crates", [8.0, -8.0], 1.2),
                        ("crates", [-3.0, 8.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // An old project's machine: a pump house with its pipes and a
            // mast broken off short, a fence of bollards.
            "veyra:derelict_machine",
            30.0,
            8.0,
            false,
            vec![
                (
                    // Mast and pump house across the back, pipes down one
                    // side, bollards across the front corners.
                    "pump",
                    vec![
                        ("mast-stub", [-12.0, -10.0], 0.0),
                        ("hut", [8.0, -12.0], 0.0),
                        ("pipes", [4.0, -2.0], 0.0),
                        ("containers-b", [-18.0, 6.0], QUARTER),
                        ("bollard", [-9.0, 14.0], 0.0),
                        ("bollard", [-4.0, 15.0], 0.0),
                        ("bollard", [9.0, 14.0], 0.0),
                    ],
                ),
                (
                    // A processor's mast with the workshop beside it, pipes
                    // across the back; open at the front and the right.
                    "processor",
                    vec![
                        ("mast-stub", [10.0, -4.0], 0.0),
                        ("workshop", [-10.0, -8.0], 0.0),
                        ("pipes", [0.0, -20.0], QUARTER),
                        ("crates", [-6.0, 4.0], 0.8),
                        ("bollard", [-14.0, 12.0], 0.0),
                        ("bollard", [-9.0, 13.0], 0.0),
                    ],
                ),
                (
                    // The stub alone with a hut and a lamp left on, pipes
                    // beside; open at the back and the sides.
                    "stub",
                    vec![
                        ("mast-stub", [0.0, 6.0], 0.0),
                        ("hut", [-12.0, 8.0], QUARTER),
                        ("pipes", [8.0, 16.0], 0.0),
                        ("lamp", [-4.0, 14.0], 0.0),
                        ("bollard", [12.0, 4.0], 0.0),
                    ],
                ),
            ],
        ),
        (
            // A surveyor's marker, alone or with a bench to look out from.
            "veyra:landmark",
            10.0,
            4.0,
            false,
            vec![
                ("marker", vec![("marker", [0.0, 2.0], 0.0)]),
                (
                    "lookout",
                    vec![("marker", [-3.0, 3.0], 0.0), ("bench", [2.0, 1.5], 0.0)],
                ),
            ],
        ),
    ]
}

/// A design mirrored across the facing: every building's middle reflected
/// in `x` and its turn reversed. The kits are not mirrored, only where they
/// stand, so a mirrored house still has its door on its front.
fn mirrored(specs: &[Spec]) -> Vec<Spec> {
    specs
        .iter()
        .map(|&(file, [x, z], yaw)| (file, [-x, z], -yaw))
        .collect()
}

/// Every site kind's building group.
///
/// # Errors
///
/// A recipe that does not validate, or a design naming a file the
/// catalogue lacks: both bugs here.
pub fn groups() -> anyhow::Result<Vec<SiteGroup>> {
    let catalogue = catalogue()?;
    designs()
        .into_iter()
        .map(|(group, radius, yard, sign, designs)| {
            let mut arrangements = Vec::new();
            let mut used = std::collections::BTreeSet::new();
            for (name, specs) in designs {
                for (suffix, specs) in [("", specs.clone()), ("-mirrored", mirrored(&specs))] {
                    let mut buildings = Vec::new();
                    for (file, centre, yaw) in specs {
                        let file = catalogue
                            .get(file)
                            .with_context(|| format!("{group} stands {file}, which is not made"))?;
                        used.insert(file.path.clone());
                        buildings.push(file.place(centre, yaw));
                    }
                    arrangements.push(Arrangement {
                        name: format!("{name}{suffix}"),
                        buildings,
                    });
                }
            }
            let sign = if sign {
                let board = &catalogue["board"];
                used.insert(board.path.clone());
                let [x0, z0, x1, z1] = board.bounds;
                // The board stands about its origin; its half size covers
                // whichever side reaches further.
                Some(Sign {
                    file: board.path.clone(),
                    half_size: [x0.abs().max(x1.abs()), z0.abs().max(z1.abs())],
                })
            } else {
                None
            };
            let files = catalogue
                .values()
                .filter(|file| used.contains(&file.path))
                .map(|file| (file.path.clone(), file.building.clone()))
                .collect();
            Ok(SiteGroup {
                group,
                designed_radius_m: radius,
                yard_m: yard,
                files,
                arrangements,
                sign,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether two convex quads overlap, by separating axes.
    fn overlap(a: &[[f64; 2]; 4], b: &[[f64; 2]; 4]) -> bool {
        for quad in [a, b] {
            for i in 0..4 {
                let (p, q) = (quad[i], quad[(i + 1) % 4]);
                let axis = [q[1] - p[1], p[0] - q[0]];
                let span = |s: &[[f64; 2]; 4]| {
                    s.iter()
                        .map(|v| v[0] * axis[0] + v[1] * axis[1])
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
                            (lo.min(d), hi.max(d))
                        })
                };
                let (a0, a1) = span(a);
                let (b0, b1) = span(b);
                if a1 <= b0 || b1 <= a0 {
                    return false;
                }
            }
        }
        true
    }

    /// Every arrangement keeps a metre inside the pad it was laid out for,
    /// and no two of its buildings stand on the same ground.
    #[test]
    fn every_arrangement_fits_its_pad_and_nothing_overlaps() {
        for group in groups().expect("the groups") {
            assert!(group.arrangements.len() >= 4, "{}", group.group);
            for arrangement in &group.arrangements {
                for b in &arrangement.buildings {
                    for [x, z] in b.footprint {
                        let d = x.hypot(z);
                        assert!(
                            d <= group.designed_radius_m - 1.0,
                            "{} {}: {} reaches {d:.1} m of {}",
                            group.group,
                            arrangement.name,
                            b.file,
                            group.designed_radius_m
                        );
                    }
                }
                for (i, a) in arrangement.buildings.iter().enumerate() {
                    for b in &arrangement.buildings[i + 1..] {
                        assert!(
                            !overlap(&a.footprint, &b.footprint),
                            "{} {}: {} {:?} and {} {:?} overlap",
                            group.group,
                            arrangement.name,
                            a.file,
                            a.footprint,
                            b.file,
                            b.footprint
                        );
                    }
                }
            }
        }
    }

    /// A mirrored arrangement is its design reflected across the facing:
    /// the same files, every footprint's corners reflected in `x`.
    #[test]
    fn a_mirror_is_its_design_reflected() {
        for group in groups().expect("the groups") {
            for pair in group.arrangements.chunks(2) {
                let [design, mirror] = pair else {
                    panic!("{} has a design without its mirror", group.group);
                };
                assert_eq!(mirror.name, format!("{}-mirrored", design.name));
                for (a, b) in design.buildings.iter().zip(&mirror.buildings) {
                    assert_eq!(a.file, b.file);
                    assert!((a.yaw + b.yaw).abs() < 1e-12);
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "micrometres of a pad tens of metres across"
                    )]
                    let reflect = |q: &[[f64; 2]; 4]| {
                        let mut xs: Vec<[i64; 2]> = q
                            .iter()
                            .map(|[x, z]| [(x * 1e6).round() as i64, (z * 1e6).round() as i64])
                            .collect();
                        xs.sort_unstable();
                        xs
                    };
                    let flipped = a.footprint.map(|[x, z]| [-x, z]);
                    assert_eq!(reflect(&flipped), reflect(&b.footprint));
                }
            }
        }
    }

    /// A kit turned by a yaw faces where the yaw says: the turn the game
    /// stands it by, `Quat::from_rotation_y`, takes its `+z` to `(sin, cos)`.
    #[test]
    fn a_turn_is_the_games_turn() {
        let q = ashlar::glam::DQuat::from_rotation_y(0.7);
        let p = q * DVec3::new(1.5, 0.0, -2.0);
        let t = turn([1.5, -2.0], 0.7);
        assert!((p.x - t[0]).abs() < 1e-12 && (p.z - t[1]).abs() < 1e-12);
    }
}
