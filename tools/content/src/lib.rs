//! Veyra's content step as a library: what is baked, where it goes, and the
//! numbers a review reads off it.
//!
//! The binary is a thin command line over this, and the tests bake the same
//! tables with the same functions, so what is snapshotted is what ships.
use ashlar::{BakedBuilding, Building, CookedBuilding, LodCell, LodPolicy};
use serde::Serialize;
use veyra_buildings::Scene;

pub mod bc7;
pub mod sheet;
pub mod terrain;
pub mod values;

/// The buildings the game spawns, by the path under `generated/` it loads each from.
pub const BUILDINGS: [(&str, Scene); 2] = [
    ("buildings/outpost.ashlar", Scene::Outpost),
    ("buildings/corporate.ashlar", Scene::Corporate),
];

/// The directory the material library and its maps go to.
pub const MATERIALS: &str = "materials";

/// Where the stats of everything baked are written, under `generated/`.
pub const STATS: &str = "stats/buildings.ron";

/// Every material the game loads: the buildings' finishes, the terrain
/// layers, the plants' bark and leaves and the rocks, in one library, so a key is baked once whoever binds it.
#[must_use]
pub fn definitions() -> ashlar::MaterialLibrary {
    let mut library = veyra_materials::buildings();
    library
        .materials
        .extend(veyra_materials::terrain().materials);
    library.materials.extend(veyra_materials::flora().materials);
    library.materials.extend(veyra_materials::rocks().materials);
    library
        .materials
        .extend(veyra_materials::crossings().materials);
    library
        .materials
        .extend(veyra_settlements::materials().materials);
    // Flowers are coloured by their vertices: one plain white, two-sided
    // surface every petal and stem multiplies.
    library.materials.insert(
        "flora:plain".to_owned(),
        ashlar::MaterialDefinition {
            base_color: [1.0; 3],
            roughness: 0.8,
            double_sided: true,
            ..ashlar::MaterialDefinition::default()
        },
    );
    library.materials.extend(capacity_materials());
    // The blockout's greys: plain constants, no maps, so massing costs no bake.
    for (_, key, base_color) in veyra_settlements::blockout::MATERIALS {
        library.materials.insert(
            key.to_owned(),
            ashlar::MaterialDefinition {
                base_color,
                roughness: 0.9,
                ..ashlar::MaterialDefinition::default()
            },
        );
    }
    library
}

/// Every graph the content step bakes: Veyra's, and the capacity fixture's
/// leaf.
#[must_use]
pub fn graphs() -> ashlar_material::MaterialGraphLibrary {
    use ashlar_material::{
        Input, MaterialGraph, PbrOutput,
        nodes::{Mix, Shape, ShapeKind},
    };
    let mut library = veyra_materials::graphs();
    library.graphs.extend(veyra_settlements::graphs().graphs);
    let leaf = MaterialGraph::builder(CAPACITY_LEAF_GRAPH)
        .node(
            "blade",
            Shape::new(ShapeKind::Capsule)
                .size(0.22)
                .length(0.5)
                .edge(0.02),
        )
        .node(
            "colour",
            Mix::new(
                Input::color([0.05, 0.12, 0.03]),
                Input::color([0.10, 0.28, 0.05]),
                "blade",
            ),
        )
        .output(PbrOutput::new().base_color("colour").opacity("blade"))
        .into_graph();
    library.graphs.insert(CAPACITY_LEAF_GRAPH.to_owned(), leaf);
    budget_flora_graphs(&mut library);
    library
}

/// The crown's grouping (`assets/look/canopy.ron`, review step six, F5) as the
/// impostor bake applies it to a tree's `shade` attribute, so a far tree wears
/// the factor the near shader repeats. Read per call: it is a dozen bytes and
/// the cache key hashes what it holds, so an edit rebakes the atlases.
///
/// # Panics
///
/// When the file is missing or does not parse: the shipped look is part of the
/// checkout, and a bake with another factor would not match the near trees.
#[must_use]
pub fn canopy_grade() -> ashlar_content::VertexGrade<'static> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/look/canopy.ron"
    ))
    .expect("assets/look/canopy.ron");
    let look = strata_data::CanopyLook::parse(&text).expect("a valid canopy look");
    ashlar_content::VertexGrade {
        attribute: "shade",
        core: [
            look.core * look.cool[0],
            look.core * look.cool[1],
            look.core * look.cool[2],
        ],
        lift: look.lift,
    }
}

/// The flora graphs remapped to the value budget (`assets/look/values.ron`,
/// review step six, F1), measured once a process because every stage asks for
/// the library.
fn budget_flora_graphs(library: &mut ashlar_material::MaterialGraphLibrary) {
    use std::sync::OnceLock;
    static REMAPPED: OnceLock<ashlar_material::MaterialGraphLibrary> = OnceLock::new();
    let remapped = REMAPPED.get_or_init(|| {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/look/values.ron"
        ))
        .expect("assets/look/values.ron");
        let budget = strata_data::ValueBudget::parse(&text).expect("a valid value budget");
        let mut graphs = library.clone();
        let done = values::budget_flora(&mut graphs, &veyra_materials::flora(), &budget)
            .expect("the flora graphs bake");
        for (path, r) in &done {
            eprintln!(
                "value budget {path}: mean {:.3} spread {:.3} -> {:.3}, scale {:.2} floor {:.3}",
                r.from.mean, r.from.spread, r.target, r.scale, r.floor
            );
        }
        graphs
    });
    for (key, graph) in &remapped.graphs {
        library.graphs.insert(key.clone(), graph.clone());
    }
}

/// The capacity fixture's leaf graph: one capsule-shaped leaf per card,
/// which cuts out about half of it, as a cluster card does.
const CAPACITY_LEAF_GRAPH: &str = "capacity:leaf-card";

/// The capacity fixture's two materials: a plain bark and a cut-out,
/// two-sided, translucent leaf, so the fixture pays what foliage pays in
/// overdraw and alpha testing.
fn capacity_materials() -> [(String, ashlar::MaterialDefinition); 2] {
    let bark = ashlar::MaterialDefinition {
        base_color: [0.22, 0.16, 0.11],
        roughness: 0.9,
        ..ashlar::MaterialDefinition::default()
    };
    let leaf = ashlar::MaterialDefinition {
        alpha: ashlar::Alpha::Mask { cutoff: 0.5 },
        double_sided: true,
        diffuse_transmission: 0.3,
        roughness: 0.8,
        surface: ashlar::Surface::Graph(ashlar::Bake {
            graph: CAPACITY_LEAF_GRAPH.to_owned(),
            params: std::collections::BTreeMap::new(),
            resolution: 256,
        }),
        ..ashlar::MaterialDefinition::default()
    };
    [
        (veyra_flora::capacity::BARK.to_owned(), bark),
        (veyra_flora::capacity::LEAF.to_owned(), leaf),
    ]
}

/// Every building in [`BUILDINGS`], built.
///
/// # Errors
///
/// A recipe that does not validate, by its path.
pub fn buildings() -> anyhow::Result<Vec<(String, Building)>> {
    use anyhow::Context;
    BUILDINGS
        .iter()
        .map(|(path, scene)| {
            let building =
                veyra_buildings::building(*scene).with_context(|| format!("building {path}"))?;
            Ok(((*path).to_owned(), building))
        })
        .collect()
}

/// Where Port Serein's layout is written, under `generated/`; the registry's
/// `layout` names the same file.
pub const SETTLEMENT_LAYOUT: &str = "settlements/port_serein.ron";

/// Where Port Serein's skyline is written, under `generated/`; the layout's
/// `skyline` names the same file. Beside the buildings' directory and not in
/// it, so no planned building's name can be the skyline's and overwrite it.
pub const SETTLEMENT_SKYLINE: &str = "settlements/port_serein.skyline.ashlar";

/// The side, in metres, of the cells a building's farthest level is joined
/// into (ticket 0002).
///
/// A far level of placed pieces costs the renderer an entity per piece, and
/// the city's was 7 272 of them for 0.45 M triangles. Joined per cell,
/// binding and side it costs by the ground it covers: the census over the
/// city counted 851 batches at 80 m and 350 at 160 m. The larger side is
/// fewer entities and a longer overlap where a cell hands over to its
/// pieces (a piece is drawn until its cell's centre is in range, up to half
/// a diagonal later); the bench decides between them, so it is one number.
pub const CELL_M: f64 = 80.0;

/// The ladder every building the game ships is baked with.
///
/// Ashlar's default ladder, tuned for its showcase's houses, kits and
/// towers: as authored to 60 m, no interior and nothing under 35 cm to
/// 250 m, nothing under 1.5 m beyond. The one change is that the last level
/// is joined into [`CELL_M`] cells, because past 250 m the city's pieces are
/// what costs and not its triangles.
#[must_use]
pub fn ladder() -> Vec<LodPolicy> {
    let mut ladder = LodPolicy::ladder();
    if let Some(far) = ladder.last_mut() {
        far.cell = Some(LodCell::Side(CELL_M));
    }
    ladder
}

/// Port Serein as the content step bakes it: its buildings under the paths
/// they are written to, and the layout the game and the server place them by.
///
/// It places the site from the shipped registry under `assets` and fits the
/// blockout to the city's own ground (ADR 0009). No world is built: inside
/// the city the ground is the settlement's, whatever the world's generator
/// does, and the layout records that ground's fingerprint.
///
/// # Errors
///
/// When the registries do not load, Port Serein is not registered, or the
/// plan does not build.
pub fn settlement(
    assets: &std::path::Path,
) -> anyhow::Result<(
    Vec<(String, Building)>,
    strata_worldgen::SettlementLayout,
    veyra_settlements::SettlementPlan,
)> {
    use anyhow::Context;
    let registries = strata_registry::load_from_dir(assets).context("the registries")?;
    let def = registries
        .settlements
        .iter()
        .find(|def| def.id.as_ref() == veyra_settlements::PORT_SEREIN)
        .context("Port Serein is not registered")?;
    // The world outside only shapes the road, which the plan does not read.
    let site = strata_worldgen::Site::place(def, &|_, _| 0.0);
    let platform = |id: &str| {
        site.platforms
            .iter()
            .find(|p| p.id == id)
            .map(|p| (p.at, p.half_size, p.height))
            .with_context(|| format!("Port Serein has no {id} platform"))
    };
    let height = |x: f64, z: f64| site.city_ground([x, z]);
    let owns = |x: f64, z: f64| site.owns([x, z]);
    let ground = veyra_settlements::blockout::Ground {
        height: &height,
        owns: &owns,
        starport: platform("starport")?,
        exchange: platform("exchange")?,
        processor: platform("processor")?,
        headquarters: platform("headquarters")?,
        quay: def.terraces.quay_m,
        road_exit: def.road.exit,
    };
    let plan = veyra_settlements::blockout::port_serein(&ground)?;
    let mut buildings = Vec::with_capacity(plan.buildings.len());
    let mut placed = Vec::with_capacity(plan.buildings.len());
    for building in &plan.buildings {
        let file = format!("settlements/port_serein/{}.ashlar", building.name);
        buildings.push((file.clone(), building.building.clone()));
        placed.push(strata_worldgen::LayoutBuilding {
            file,
            position: building.position,
            yaw: building.yaw,
            footprint: building.footprint,
        });
    }
    let layout = strata_worldgen::SettlementLayout {
        id: plan.id.to_owned(),
        ground: site.fingerprint(),
        buildings: placed,
        spawn: plan.spawn,
        lobby: plan.lobby,
        lights: plan
            .lights
            .iter()
            .map(|l| strata_worldgen::LayoutLight {
                position: l.position,
                color: l.color,
                lumens: l.lumens,
                range: l.range,
            })
            .collect(),
        plants: plan
            .plants
            .iter()
            .map(|p| strata_worldgen::LayoutPlant {
                species: p.species.to_owned(),
                position: p.position,
                yaw: p.yaw,
                scale: p.scale,
                variant: p.variant,
            })
            .collect(),
        skyline: Some(SETTLEMENT_SKYLINE.to_owned()),
    };
    Ok((buildings, layout, plan))
}

/// Every site kind's building group as the content step bakes it (ADR
/// 0012, decision 6): the files the groups stand, under the paths they are
/// written to, and each group's layout, which the game and the server read
/// from `strata_worldgen::sites::group_layout_path` of the group's name.
///
/// A file two groups share is listed once. A file already in `baked` (the
/// study kit's outpost, which a town reuses as its depot) is not listed
/// again: the bake writes it either way.
///
/// # Errors
///
/// A recipe that does not validate.
pub fn site_groups(
    baked: &[(String, Building)],
) -> anyhow::Result<(
    Vec<(String, Building)>,
    Vec<strata_worldgen::sites::SiteGroupLayout>,
)> {
    use strata_worldgen::sites::{SiteArrangement, SiteBuilding, SiteGroupLayout, SiteSign};
    let groups = veyra_settlements::sites::groups()?;
    let mut files: Vec<(String, Building)> = Vec::new();
    let mut layouts = Vec::with_capacity(groups.len());
    for group in groups {
        for (path, building) in group.files {
            let known = baked.iter().chain(files.iter()).any(|(p, _)| *p == path);
            if !known {
                files.push((path, building));
            }
        }
        layouts.push(SiteGroupLayout {
            group: group.group.to_owned(),
            yard_m: group.yard_m,
            arrangements: group
                .arrangements
                .into_iter()
                .map(|a| SiteArrangement {
                    name: a.name,
                    buildings: a
                        .buildings
                        .into_iter()
                        .map(|b| SiteBuilding {
                            file: b.file,
                            position: b.position,
                            yaw: b.yaw,
                            footprint: b.footprint,
                        })
                        .collect(),
                })
                .collect(),
            sign: group.sign.map(|sign| SiteSign {
                file: sign.file,
                half_size: sign.half_size,
            }),
        });
    }
    Ok((files, layouts))
}

/// The crossings' kit as the content step bakes it (ticket 0005, step 7):
/// every piece of `veyra_buildings::bridge`'s catalogue under the kit's
/// directory, and the layout the game and the server stand them from
/// ([`strata_worldgen::crossings::KIT_PATH`]).
///
/// # Errors
///
/// A recipe that does not validate.
pub fn crossings() -> anyhow::Result<(
    Vec<(String, Building)>,
    strata_worldgen::crossings::CrossingKit,
)> {
    use anyhow::Context;
    use strata_worldgen::crossings::{
        BridgeClass, CrossingKit, DeckEnd, HeadwallFile, KIT_PATH, PierFile,
    };
    use strata_worldgen::roads::RoadClass;
    use veyra_buildings::bridge::{self, Piece, Way};
    let road = |way: Way| match way {
        Way::Service => RoadClass::Service,
        Way::Track => RoadClass::Track,
        Way::Foot => RoadClass::Trail,
    };
    let dir = KIT_PATH
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .to_owned();
    let mut files = Vec::new();
    let mut kit = CrossingKit {
        bridges: Vec::new(),
        deck_end: DeckEnd {
            proud_m: bridge::PROUD_M,
            ramp_m: bridge::RAMP_M,
            drop_m: bridge::RAMP_DROP_M,
        },
        piers: Vec::new(),
        headwalls: Vec::new(),
        ford_post: None,
        stones: Vec::new(),
        stone_proud_m: bridge::STONE_PROUD_M,
    };
    for piece in bridge::catalogue() {
        let file = format!("{dir}/{}", piece.file_name());
        let building = piece
            .building()
            .with_context(|| format!("crossing piece {file}"))?;
        files.push((file.clone(), building));
        match piece {
            Piece::Bridge(b) => kit.bridges.push(BridgeClass {
                road: road(b.way),
                file,
                clear_m: b.clear_m,
                gaps_m: b.gaps_m(),
                length_m: b.length_m(),
                underside_m: b.underside_m(),
                piers: b.piers(),
                girder_most_m: bridge::GIRDER_MOST_M,
            }),
            Piece::Pier(p) => kit.piers.push(PierFile {
                road: road(p.way),
                file,
                height_m: p.height_m,
                half_along_m: p.half_along_m(),
            }),
            Piece::Headwall(h) => kit.headwalls.push(HeadwallFile {
                file,
                box_m: h.box_m,
                top_m: h.top_m(),
            }),
            Piece::FordPost => kit.ford_post = Some(file),
            Piece::Stone(_) => kit.stones.push(file),
        }
    }
    Ok((files, kit))
}

/// The roadside kit as the content step bakes it (ticket 0005, step 8):
/// `veyra_buildings::roadside`'s marker post and lamp under the crossings'
/// directory, and the layout the game and the server stand them from
/// ([`strata_worldgen::roadside::ROADSIDE_PATH`]).
///
/// # Errors
///
/// A recipe that does not validate.
pub fn roadside() -> anyhow::Result<(
    Vec<(String, Building)>,
    strata_worldgen::roadside::RoadsideKit,
)> {
    use anyhow::Context;
    use strata_worldgen::roadside::{ROADSIDE_PATH, RoadsideKit};
    use veyra_buildings::roadside::Piece;
    let dir = ROADSIDE_PATH
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .to_owned();
    let mut files = Vec::new();
    let mut kit = RoadsideKit {
        marker: None,
        lamp: None,
    };
    for piece in Piece::ALL {
        let file = format!("{dir}/{}", piece.file_name());
        let building = piece
            .building()
            .with_context(|| format!("roadside piece {file}"))?;
        files.push((file.clone(), building));
        match piece {
            Piece::Marker => kit.marker = Some(file),
            Piece::Lamp => kit.lamp = Some(file),
        }
    }
    Ok((files, kit))
}

/// Pixels on the long side of a weapon's frame (ticket 0008): what the
/// fitting drawing needs at the 4K interface scale, and enough that no
/// mount's piece of any weapon is under 24 pixels on both sides.
pub const WEAPON_LONG_SIDE: u32 = 1024;

/// Samples along one side of a pixel of a weapon's picture, so sixteen a
/// pixel: where a slanted edge stops showing steps.
pub const WEAPON_SAMPLES: u32 = 4;

/// Pixels on a side of a weapon picture's square icon (ticket 0008, step
/// 5): twice the largest cell the interface draws, which is 44 units at a
/// scale of two.
pub const WEAPON_ICON_SIDE: u32 = 128;

/// Pixels the picture's long side takes of the icon's side, the rest being
/// the margin a cell's own marks (the quality pip, the count) stand in.
pub const WEAPON_ICON_FILL: u32 = 112;

/// The most an icon's tone is lifted towards white: enough to bring the
/// darkest part of the catalogue into a cell's reading range, and not so
/// much that a part with one bright edge keeps the rest of itself dark.
pub const WEAPON_ICON_LIFT: f64 = 3.0;

/// The most a small picture is enlarged to fill its icon. A muzzle cap is
/// forty pixels across in its frame and would be a blur at three times
/// that; at twice it is still the picture.
pub const WEAPON_ICON_GROWTH: f64 = 2.0;

/// A weapon picture fitted into its square icon: scaled so its long side
/// is [`WEAPON_ICON_FILL`], or by [`WEAPON_ICON_GROWTH`] when that is less,
/// and centred on a clear ground. `pixels` is luminance then straight
/// alpha, `width` by `height`, and so is what comes back,
/// [`WEAPON_ICON_SIDE`] square.
///
/// Shrunk by the mean of what each pixel covers and grown by the weighted
/// four nearest, both of the tone weighted by its coverage, so an edge is
/// not darkened by whatever tone lies under the clear pixels beside it.
///
/// The tone is then lifted so the icon's lightest pixel is white, by no
/// more than [`WEAPON_ICON_LIFT`]. A picture's tone is its material, and a
/// grip of dark rubber is right in the drawing of its weapon, where the
/// lighter parts round it show what it is; alone in a cell on dark glass
/// it is a smudge. The shading within the part is kept: every pixel is
/// lifted by the same share.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "pixel coordinates of a picture at most 1024 across, and tones of eight bits"
)]
pub fn weapon_icon(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let side = WEAPON_ICON_SIDE as usize;
    let mut out = vec![0_u8; side * side * 2];
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || pixels.len() < width * height * 2 {
        return out;
    }
    let scale = (f64::from(WEAPON_ICON_FILL) / width.max(height) as f64).min(WEAPON_ICON_GROWTH);
    let fitted = |length: usize| ((length as f64 * scale).round() as usize).clamp(1, side);
    let (wide, high) = (fitted(width), fitted(height));
    let (left, top) = ((side - wide) / 2, (side - high) / 2);
    // The tone and the coverage of one source pixel, the tone already
    // weighted by the coverage.
    let at = |x: usize, y: usize| {
        let i = (y.min(height - 1) * width + x.min(width - 1)) * 2;
        let alpha = f64::from(pixels[i + 1]);
        (f64::from(pixels[i]) * alpha, alpha)
    };
    for y in 0..high {
        for x in 0..wide {
            let (mut tone, mut alpha, mut weight) = (0., 0., 0.);
            if scale < 1. {
                let (x0, x1) = (x as f64 / scale, (x + 1) as f64 / scale);
                let (y0, y1) = (y as f64 / scale, (y + 1) as f64 / scale);
                for sy in y0.floor() as usize..(y1.ceil() as usize).min(height) {
                    let wy = (y1.min((sy + 1) as f64) - y0.max(sy as f64)).max(0.);
                    for sx in x0.floor() as usize..(x1.ceil() as usize).min(width) {
                        let w = wy * (x1.min((sx + 1) as f64) - x0.max(sx as f64)).max(0.);
                        let (t, a) = at(sx, sy);
                        tone += w * t;
                        alpha += w * a;
                        weight += w;
                    }
                }
            } else {
                let fx = ((x as f64 + 0.5) / scale - 0.5).clamp(0., (width - 1) as f64);
                let fy = ((y as f64 + 0.5) / scale - 0.5).clamp(0., (height - 1) as f64);
                let (sx, sy) = (fx.floor() as usize, fy.floor() as usize);
                let (tx, ty) = (fx - sx as f64, fy - sy as f64);
                for (dx, dy, w) in [
                    (0, 0, (1. - tx) * (1. - ty)),
                    (1, 0, tx * (1. - ty)),
                    (0, 1, (1. - tx) * ty),
                    (1, 1, tx * ty),
                ] {
                    let (t, a) = at(sx + dx, sy + dy);
                    tone += w * t;
                    alpha += w * a;
                    weight += w;
                }
            }
            if weight <= 0. || alpha <= 0. {
                continue;
            }
            let i = ((top + y) * side + left + x) * 2;
            out[i] = (tone / alpha).round().clamp(0., 255.) as u8;
            out[i + 1] = (alpha / weight).round().clamp(0., 255.) as u8;
        }
    }
    let lightest = out
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pixel| pixel[1] > 0)
        .map(|pixel| pixel[0])
        .max()
        .unwrap_or(0);
    if lightest > 0 {
        let gain = (255. / f64::from(lightest)).min(WEAPON_ICON_LIFT);
        for pixel in out.as_chunks_mut::<2>().0 {
            pixel[0] = (f64::from(pixel[0]) * gain).round().clamp(0., 255.) as u8;
        }
    }
    out
}

/// The weapon pictures as the content step bakes them (ticket 0008, step
/// 2): every chassis's body, each mount's standard part and every module
/// that fits it, and the stock weapon whole, as PNG files by the path under
/// `generated/` each is written to, and the index the client places them by
/// ([`strata_registry::pictures::PICTURES_PATH`]). Beside each picture is
/// its square icon ([`weapon_icon`]), `icon.<stem>.png`, which the index
/// names too: what a cell of a grid draws for the item.
///
/// A file is two channels of eight bits, luminance then straight alpha,
/// cropped to what it covers. A mount's picture lacks what the chassis
/// hides of it, so the pieces stack into the weapon; the index marks the
/// part the chassis hides whole and the one that looks the same as another
/// of its mount. The kernel runs once per assembly of
/// [`veyra_weapons::picture::plan`], which is the longest module list plus
/// one for a chassis and not a run per module.
///
/// Nothing here is cached: `ashlar_content`'s cache holds materials,
/// buildings and impostors and has no entry for a plain file. The whole
/// bake is a few seconds, and [`ashlar_content::Content::write_file`]
/// leaves a file alone whose bytes have not changed, so a second run
/// rewrites nothing either way.
///
/// # Errors
///
/// A recipe that does not validate or mesh, or a piece that covers no
/// pixel, by its chassis.
pub fn weapons() -> anyhow::Result<(
    Vec<(String, Vec<u8>)>,
    strata_registry::pictures::PictureSet,
)> {
    use anyhow::Context;
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use strata_registry::pictures::{
        ChassisPictures, PICTURES_PATH, PictureSet, PiecePicture, Placed,
    };
    use veyra_weapons::{Weapon, picture};
    let dir = PICTURES_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
    let settings = picture::Settings::sized(WEAPON_LONG_SIDE, WEAPON_SAMPLES);
    let mesher = ManifoldMesher::default();
    let mut files = Vec::new();
    let mut set = PictureSet::default();
    for weapon in Weapon::ALL {
        let key = weapon.key();
        let builds = picture::plan(weapon)
            .into_iter()
            .map(|assembly| {
                let building = veyra_weapons::build(&assembly)
                    .with_context(|| format!("the recipe of {key}"))?;
                let mesh = mesh_building(&building, &mesher)
                    .with_context(|| format!("the mesh of {key}"))?;
                Ok((assembly, mesh))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let pictures = picture::render(weapon, &builds, &settings)
            .with_context(|| format!("the pictures of {key}"))?;
        let path = |kind: picture::PieceKind| format!("{dir}/{key}/{}.png", kind.file_stem());
        // The picture's file and its icon's.
        let mut write = |piece: &picture::Piece| -> anyhow::Result<(String, String)> {
            let file = path(piece.kind);
            let image = &piece.image;
            let data = png(
                &image.data,
                image.width,
                image.height,
                image::ExtendedColorType::La8,
            )
            .with_context(|| file.clone())?;
            files.push((file.clone(), data));
            let icon = format!("{dir}/{key}/icon.{}.png", piece.kind.file_stem());
            let data = png(
                &weapon_icon(&image.data, image.width, image.height),
                WEAPON_ICON_SIDE,
                WEAPON_ICON_SIDE,
                image::ExtendedColorType::La8,
            )
            .with_context(|| icon.clone())?;
            files.push((icon.clone(), data));
            Ok((file, icon))
        };
        let pieces = pictures
            .pieces
            .iter()
            .map(|piece| {
                let (file, icon) = write(piece)?;
                Ok(PiecePicture {
                    mount: piece.kind.mount().map(|slot| slot.key().to_owned()),
                    module: piece.kind.module().map(str::to_owned),
                    rect: piece.rect,
                    file,
                    icon: Some(icon),
                    hidden: piece.hidden,
                    same_as: piece.same_as.map(path),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let (file, icon) = write(&pictures.whole)?;
        let whole = Placed {
            rect: pictures.whole.rect,
            file,
            icon: Some(icon),
        };
        set.chassis.insert(
            key.to_owned(),
            ChassisPictures {
                width: pictures.frame.width,
                height: pictures.frame.height,
                metres_per_pixel: pictures.frame.metres_per_pixel,
                origin: pictures.frame.origin,
                whole,
                pieces,
            },
        );
    }
    Ok((files, set))
}

/// Eight-bit pixels as a PNG file's bytes.
///
/// At the encoder's best compression: the pictures ship with the game, there
/// are few of them and they are small, so the time is nothing and the bytes
/// are kept.
fn png(
    pixels: &[u8],
    width: u32,
    height: u32,
    colour: image::ExtendedColorType,
) -> anyhow::Result<Vec<u8>> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive)
        .write_image(pixels, width, height, colour)?;
    Ok(out)
}

/// The skyline of a settlement's plan, what glows decided by [`definitions`],
/// with its per-instance overrides
/// resolved against [`definitions`] as [`ashlar_content::Content::ship`]
/// resolves the buildings', so it binds the same keys the shipped library
/// holds.
///
/// Placed in the settlement's frame: see [`veyra_settlements::skyline`].
///
/// # Errors
///
/// A skyline or an override that does not validate.
pub fn skyline(plan: &veyra_settlements::SettlementPlan) -> anyhow::Result<Building> {
    use anyhow::Context;
    let definitions = definitions();
    let skyline = veyra_settlements::skyline::skyline(plan, &definitions).context("the skyline")?;
    let (mut flat, _) = ashlar_content::flatten_overrides(&[skyline], &definitions)
        .context("the skyline's overrides")?;
    flat.pop().context("one skyline in, one out")
}

/// What a baked building costs, level by level.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildingStats {
    /// The path it is written to.
    pub path: String,
    /// Placed instances of parts.
    pub instances: usize,
    /// Convex collision proxies, placed: what physics builds.
    pub colliders: usize,
    /// Each level of detail, nearest first.
    pub levels: Vec<LevelStats>,
}

/// One level of detail of a baked building.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelStats {
    /// Where the level stops drawing, in metres; `None` draws to any distance.
    pub until: Option<f32>,
    /// Pieces: each is an entity at runtime, and entities are what a frame's
    /// CPU cost scales with.
    pub pieces: usize,
    /// Triangles in the level's meshes, each counted once: what is uploaded.
    pub unique_triangles: usize,
    /// Triangles over every placed piece: what is drawn.
    pub drawn_triangles: usize,
}

/// The stats of one baked building.
#[must_use]
pub fn building_stats(path: &str, baked: &BakedBuilding) -> BuildingStats {
    let unique = baked.triangles();
    let levels = (0..baked.levels.len())
        .filter_map(|index| {
            let level = baked.level(index)?;
            let (pieces, drawn) = level.pieces().fold((0, 0), |(pieces, drawn), piece| {
                (pieces + 1, drawn + piece.mesh.triangle_count())
            });
            Some(LevelStats {
                until: baked.levels[index].until,
                pieces,
                unique_triangles: unique[index],
                drawn_triangles: drawn,
            })
        })
        .collect();
    BuildingStats {
        path: path.to_owned(),
        instances: baked.building().recipe().instances.len(),
        colliders: baked.level(0).map_or(0, |level| level.colliders().len()),
        levels,
    }
}

/// The stats of one building file as written, whichever version it is: the
/// full file the content step caches or the cooked file the game draws. The
/// two give the same numbers, since cooking changes bytes and not triangles.
///
/// # Errors
///
/// A file neither reader accepts.
pub fn file_stats(path: &str, bytes: &[u8]) -> Result<BuildingStats, ashlar::BakedError> {
    if CookedBuilding::is_cooked(bytes) {
        return Ok(cooked_stats(path, &CookedBuilding::read(bytes)?));
    }
    Ok(building_stats(path, &BakedBuilding::read(bytes)?))
}

/// [`building_stats`] of a cooked building.
#[must_use]
pub fn cooked_stats(path: &str, cooked: &CookedBuilding) -> BuildingStats {
    let unique = cooked.triangles();
    let levels = (0..cooked.levels.len())
        .map(|index| {
            let (pieces, drawn) = cooked.pieces(index).fold((0, 0), |(pieces, drawn), piece| {
                let triangles = cooked
                    .meshes
                    .get(piece.mesh as usize)
                    .map_or(0, |mesh| mesh.indices.len() / 3);
                (pieces + 1, drawn + triangles)
            });
            LevelStats {
                until: cooked.levels[index].until,
                pieces,
                unique_triangles: unique[index],
                drawn_triangles: drawn,
            }
        })
        .collect();
    BuildingStats {
        path: path.to_owned(),
        instances: cooked.instances.len(),
        colliders: cooked.colliders().len(),
        levels,
    }
}

/// The budget every Veyra building is held to, so a kit change that would
/// cost a frame is a failing test and not a surprise in the ledger.
pub mod budget {
    /// Triangles drawn by the nearest level.
    pub const NEAR_DRAWN_TRIANGLES: usize = 60_000;
    /// Pieces, and so entities, in any one level. Set just above the corporate
    /// tower's 473 at its nearest level (1038 over all three) at the Phase 6
    /// baseline, so growth fails here; Phase 11, which puts a city of these in
    /// the world, owns bringing it down.
    pub const PIECES_PER_LEVEL: usize = 480;
    /// How much the farthest level keeps of the nearest level's drawn triangles.
    pub const FAR_SHARE: f64 = 0.35;
    /// Pieces, and so entities, of a settlement's farthest level summed over
    /// its buildings: what the city costs from anywhere past 250 m while its
    /// buildings are streamed in. The census of 2026-09-28 counted 851 at
    /// 80 m cells in the settlement's frame; baked building by building a
    /// city has at least that many, and a kit that adds a district fails
    /// here.
    pub const SETTLEMENT_FAR_PIECES: usize = 1_200;
    /// Pieces of a settlement's skyline: one per binding and side, and the
    /// city's far level binds 45 materials.
    pub const SKYLINE_PIECES: usize = 64;
    /// Triangles of a settlement's skyline, which draws to the horizon and is
    /// unique geometry, every copy of a kit its own triangles.
    ///
    /// A ceiling over what Port Serein measures, not the target: the ticket
    /// (0002) aims at 40 000 and this was 60 000 until the first bake came
    /// in at 249 153. Plain boxes for chamfered ones took it to 174 849
    /// (2026-09-29), and keeping every lit element as the buildings'
    /// farthest level keeps it (the housing slabs' slits, the market's shop
    /// fronts) to 180 973 the same day. What is left is boxes already: the
    /// Terraces' house trim (parapet caps and roof plates, about 41 000) and
    /// walls (32 000), the terrace walls' masses (27 000), all kept because
    /// ashlar judges a thin broad slab by its broad side. Bringing it down
    /// is a kit change or one to that rule, and ticket 0002 lists it as an
    /// open step; until then this holds the city from growing.
    pub const SKYLINE_TRIANGLES: usize = 181_000;
}

/// Where the weapon models' material library goes, under `generated/`: a
/// library of its own so the buildings' library, and everything hashed
/// from it, does not see a weapon.
pub const WEAPON_MATERIALS: &str = "weapons/veyra";

/// The library [`ashlar_content::Content::write_materials`] writes into
/// [`WEAPON_MATERIALS`].
pub const WEAPON_LIBRARY: &str = "weapons/veyra/library.materials.ron";

/// The recipe's seven material slots as the models bind them, each with its
/// roughness and metallic, which the in-game hand reads as a plain surface
/// (no baked graph).
///
/// The workbench draws shell, metal, accent and grip from baked graphs that
/// are too heavy to bake in the game; these numbers stand in for how those
/// graphs read at hand distance. The rest are the workbench's own.
const WEAPON_SURFACES: [(&str, f32, f32); 7] = [
    // A painted composite shell: satin, with a trace of metal in the paint.
    ("shell", 0.5, 0.1),
    // Machined metal: fully metallic, brushed rather than mirrored.
    ("metal", 0.35, 1.0),
    // The accent coating is the shell's finish in another colour.
    ("accent", 0.5, 0.1),
    // Grip polymer is matt and nowhere near metal.
    ("grip", 0.85, 0.0),
    // Ceramic is glazed but not shiny (the workbench's 0.5).
    ("ceramic", 0.5, 0.0),
    // Lens glass: nearly a mirror, with some metal in the coating.
    ("lens", 0.08, 0.6),
    // The light is emissive and slightly glossy (the workbench's 0.3).
    ("light", 0.3, 0.0),
];

/// A maker's colours as the workbench has them (sRGB): shell, accent, the
/// metal and the ceramic.
fn maker_colours(maker: veyra_weapons::Maker) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
    use veyra_weapons::Maker;
    let (shell, accent) = match maker {
        Maker::Kestrel => ([0.48, 0.53, 0.51], [0.67, 0.43, 0.17]),
        Maker::Morrow => ([0.20, 0.28, 0.31], [0.37, 0.60, 0.59]),
        Maker::Bracken => ([0.38, 0.25, 0.18], [0.66, 0.56, 0.35]),
        Maker::Vigil => ([0.17, 0.195, 0.205], [0.34, 0.37, 0.26]),
    };
    if maker == Maker::Vigil {
        (shell, accent, [0.34, 0.37, 0.40], [0.38, 0.40, 0.34])
    } else {
        (shell, accent, [0.48, 0.51, 0.54], [0.63, 0.65, 0.61])
    }
}

/// The material key a model's piece wears: the recipe's slot (`weapon:shell`)
/// qualified by its maker, because the colours differ by maker and a prop
/// has no palette to rebind them.
fn weapon_material(maker: veyra_weapons::Maker, slot: &str) -> String {
    format!(
        "weapon:{}.{}",
        maker.label().to_lowercase(),
        slot.strip_prefix("weapon:").unwrap_or(slot)
    )
}

/// The weapon models' materials: seven plain definitions a maker, the
/// issued finish only.
#[must_use]
pub fn weapon_definitions() -> ashlar::MaterialLibrary {
    use veyra_weapons::Maker;
    let mut library = ashlar::MaterialLibrary::default();
    for maker in Maker::ALL {
        let (shell, accent, metal, ceramic) = maker_colours(maker);
        for (slot, roughness, metallic) in WEAPON_SURFACES {
            let (base_color, emissive) = match slot {
                "shell" => (shell, [0.0; 3]),
                "accent" => (accent, [0.0; 3]),
                "metal" => (metal, [0.0; 3]),
                "grip" => ([0.08, 0.09, 0.10], [0.0; 3]),
                "ceramic" => (ceramic, [0.0; 3]),
                "lens" => ([0.025, 0.15, 0.16], [0.0; 3]),
                _ => ([0.15, 0.50, 0.51], [0.1, 1.0, 1.1]),
            };
            library.materials.insert(
                weapon_material(maker, slot),
                ashlar::MaterialDefinition {
                    base_color,
                    roughness,
                    metallic,
                    emissive,
                    ..Default::default()
                },
            );
        }
    }
    library
}

/// Where the vehicle models' material library is written, under `generated/`.
pub const VEHICLE_LIBRARY: &str = "vehicles/veyra/library.materials.ron";

/// Where the vehicle models' baked materials are written.
pub const VEHICLE_MATERIALS: &str = "vehicles/veyra";

/// The bike's materials: the plain issued greys, a dark trim and the
/// exhaust's glow, as plain definitions with no baked graph.
#[must_use]
pub fn vehicle_definitions() -> ashlar::MaterialLibrary {
    let mut library = ashlar::MaterialLibrary::default();
    // Key, colour (sRGB), roughness, metallic, emissive.
    let surfaces: [(&str, [f32; 3], f32, f32, [f32; 3]); 5] = [
        ("vehicle:hull", [0.42, 0.45, 0.46], 0.5, 0.2, [0.0; 3]),
        ("vehicle:fairing", [0.62, 0.64, 0.62], 0.4, 0.1, [0.0; 3]),
        ("vehicle:trim", [0.07, 0.08, 0.09], 0.85, 0.0, [0.0; 3]),
        ("vehicle:vane", [0.20, 0.22, 0.24], 0.35, 0.9, [0.0; 3]),
        (
            "vehicle:glow",
            [0.15, 0.50, 0.51],
            0.3,
            0.0,
            [0.1, 1.0, 1.1],
        ),
    ];
    for (key, base_color, roughness, metallic, emissive) in surfaces {
        library.materials.insert(
            key.to_owned(),
            ashlar::MaterialDefinition {
                base_color,
                roughness,
                metallic,
                emissive,
                ..Default::default()
            },
        );
    }
    library
}

/// The vehicle models: a prop a vehicle, by the path under `generated/` it is
/// written to, and their index.
pub type VehicleModelFiles = (
    Vec<(String, ashlar::prop::Prop)>,
    strata_registry::vehicle::VehicleModels,
);

/// Bake the courier bike (ticket 0009, step 2) as one prop of one level and
/// no collider, in the recipe's frame, and the index that names it.
///
/// # Errors
///
/// A recipe that does not validate or mesh.
pub fn vehicle_models() -> anyhow::Result<VehicleModelFiles> {
    use anyhow::Context;
    use ashlar::prop::{Prop, PropLevel, PropPiece};
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use strata_registry::vehicle::{MODELS_PATH, VehicleModels};
    let dir = MODELS_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
    let building = veyra_buildings::vehicle::courier_bike().context("the recipe of the bike")?;
    let built =
        mesh_building(&building, &ManifoldMesher::default()).context("the mesh of the bike")?;
    let part = &built
        .building
        .instance("bike")
        .context("the bike has no instance")?
        .part;
    let elements = built.parts.get(part).context("the bike has no part")?;
    let level = PropLevel {
        pieces: elements
            .iter()
            .filter(|element| !element.mesh.indices.is_empty())
            .map(|element| PropPiece {
                mesh: element.mesh.clone(),
                material: format!("vehicle:{}", element.material_slot),
            })
            .collect(),
        ..Default::default()
    };
    let file = format!("{dir}/courier_bike.ashprop");
    let mut index = VehicleModels {
        materials: VEHICLE_LIBRARY.to_owned(),
        files: std::collections::BTreeMap::new(),
        icons: std::collections::BTreeMap::new(),
    };
    index
        .files
        .insert("veyra:vehicle.courier_bike".to_owned(), file.clone());
    index.icons.insert(
        "veyra:vehicle.courier_bike".to_owned(),
        format!("{dir}/{BIKE_ICON}"),
    );
    Ok((
        vec![(
            file,
            Prop::new("veyra:vehicle.courier_bike", vec![level], Vec::new()),
        )],
        index,
    ))
}

/// Where the prop kit's material library is written, under `generated/`.
pub const PROP_LIBRARY: &str = "props/veyra/library.materials.ron";

/// Where the prop kit's baked materials are written.
pub const PROP_MATERIALS: &str = "props/veyra";

/// The prop kit's materials (review step six, visual.md F6): the ivory shell
/// at the middle of the value budget's `shell` family, the dark inset
/// machinery, the colony's orange and one panel a role. A panel is a muted
/// colour with a low glow, so it reads at dusk without the old strips' blaze.
#[must_use]
pub fn prop_definitions() -> ashlar::MaterialLibrary {
    let mut library = ashlar::MaterialLibrary::default();
    let mut add =
        |key: String, base_color: [f32; 3], roughness: f32, metallic: f32, emissive: [f32; 3]| {
            library.materials.insert(
                key,
                ashlar::MaterialDefinition {
                    base_color,
                    roughness,
                    metallic,
                    emissive,
                    ..Default::default()
                },
            );
        };
    add(
        "prop:shell".to_owned(),
        [0.74, 0.72, 0.66],
        0.55,
        0.05,
        [0.0; 3],
    );
    add(
        "prop:inset".to_owned(),
        [0.12, 0.13, 0.15],
        0.6,
        0.5,
        [0.0; 3],
    );
    add(
        "prop:accent".to_owned(),
        [0.86, 0.42, 0.14],
        0.5,
        0.1,
        [0.0; 3],
    );
    for (role, colour) in veyra_buildings::props::ROLES.iter().enumerate() {
        // The panel is the role colour pulled towards a muted teal-grey and
        // lit by a share of itself.
        let base = colour.map(|c| 0.3 + 0.5 * c);
        let glow = colour.map(|c| 0.5 * c * c);
        add(
            veyra_buildings::props::panel_key(role),
            base,
            0.35,
            0.0,
            glow,
        );
    }
    library
}

/// The prop kit, one prop a piece, and its index.
pub type PropKitFiles = (
    Vec<(String, ashlar::prop::Prop)>,
    strata_registry::props::PropModels,
);

/// Bake the household's interaction props (review step six): one prop of one
/// level and no collider a piece, in the recipe's frame, and the index.
///
/// # Errors
///
/// A recipe that does not validate or mesh.
pub fn prop_kit() -> anyhow::Result<PropKitFiles> {
    use anyhow::Context;
    use ashlar::prop::{Prop, PropLevel, PropPiece};
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use strata_registry::props::{MODELS_PATH, PropModels};
    use veyra_buildings::props::Piece;
    let dir = MODELS_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut index = PropModels {
        materials: PROP_LIBRARY.to_owned(),
        files: std::collections::BTreeMap::new(),
    };
    let mut props = Vec::new();
    for piece in Piece::ALL {
        let building = piece
            .building()
            .with_context(|| format!("the recipe of {}", piece.name()))?;
        let built = mesh_building(&building, &ManifoldMesher::default())
            .with_context(|| format!("the mesh of {}", piece.name()))?;
        let part = &built
            .building
            .instance("piece")
            .context("a prop piece has no instance")?
            .part;
        let elements = built.parts.get(part).context("a prop piece has no part")?;
        let level = PropLevel {
            pieces: elements
                .iter()
                .filter(|element| !element.mesh.indices.is_empty())
                .map(|element| PropPiece {
                    mesh: element.mesh.clone(),
                    material: match element.material_slot.as_str() {
                        "panel" => veyra_buildings::props::panel_key(piece.role()),
                        slot => format!("prop:{slot}"),
                    },
                })
                .collect(),
            ..Default::default()
        };
        let file = format!("{dir}/{}", piece.file_name());
        index.files.insert(piece.name().to_owned(), file.clone());
        props.push((
            file,
            Prop::new(format!("props:{}", piece.name()), vec![level], Vec::new()),
        ));
    }
    Ok((props, index))
}

/// The file of the bike's square icon beside its prop.
pub const BIKE_ICON: &str = "icon.courier_bike.png";
/// The tone, out of 255, that the body of the bike's icon is lifted to.
const BIKE_ICON_BODY_TONE: u32 = 225;

/// The bike's side-on picture and its square icon (the weapons' own
/// [`weapon_icon`]), as the PNG files [`vehicle_models`]'s index names, by
/// the path under `generated/` each is written to, and the picture's size.
///
/// # Errors
///
/// A recipe that does not validate or mesh, or a bike that covers no pixel.
pub fn vehicle_icons() -> anyhow::Result<(Vec<(String, Vec<u8>)>, [u32; 2])> {
    use anyhow::Context;
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use strata_registry::vehicle::MODELS_PATH;
    use veyra_weapons::picture::{Settings, side_picture};
    let dir = MODELS_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
    let building = veyra_buildings::vehicle::courier_bike().context("the recipe of the bike")?;
    let built =
        mesh_building(&building, &ManifoldMesher::default()).context("the mesh of the bike")?;
    let drawn = side_picture(
        &built,
        "bike",
        &Settings::sized(WEAPON_LONG_SIDE, WEAPON_SAMPLES),
    )
    .context("the picture of the bike")?;
    let image = &drawn.image;
    let file = format!("{dir}/{BIKE_ICON}");
    let mut square = weapon_icon(&image.data, image.width, image.height);
    // The bike's paint is darker than a weapon's, and a cell tints the icon
    // by the item's colour, which would leave it a dark smudge: lift its
    // tones until the body's usual tone is a light one, as the weapons'
    // are, and let the highlights clip.
    let (pairs, _) = square.as_chunks_mut::<2>();
    let mut tones: Vec<u32> = pairs
        .iter()
        .filter(|pixel| pixel[1] > 0)
        .map(|pixel| u32::from(pixel[0]))
        .collect();
    tones.sort_unstable();
    let body = tones.get(tones.len() / 2).copied().unwrap_or(0);
    for pixel in pairs.iter_mut() {
        let lifted = (u32::from(pixel[0]) * BIKE_ICON_BODY_TONE)
            .checked_div(body)
            .map_or(u32::from(pixel[0]), |tone| tone.min(255));
        pixel[0] = u8::try_from(lifted).unwrap_or(255);
    }
    let data = png(
        &square,
        WEAPON_ICON_SIDE,
        WEAPON_ICON_SIDE,
        image::ExtendedColorType::La8,
    )
    .with_context(|| file.clone())?;
    Ok((vec![(file, data)], [image.width, image.height]))
}

/// The weapon models: a prop a piece, by the path under `generated/` it is
/// written to, and their index.
pub type WeaponModelFiles = (
    Vec<(String, ashlar::prop::Prop)>,
    strata_registry::models::ModelSet,
);

/// Bake every weapon's pieces as props (ticket 0007, step 8): the chassis,
/// each mount's standard part and each module that fits it, one prop of one
/// level and no collider each. The pieces share the chassis's frame, so they
/// compose with no placement.
///
/// Where two pieces of a chassis hold the same meshes both are written;
/// sharing is not worth an index field.
///
/// # Errors
///
/// A recipe that does not validate or mesh, or a piece no build holds.
#[allow(clippy::too_many_lines)] // one pass over the catalogue, clearer whole than split
pub fn weapon_models() -> anyhow::Result<WeaponModelFiles> {
    use anyhow::Context;
    use ashlar::prop::{Prop, PropLevel, PropPiece};
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use strata_registry::models::{
        ChassisModels, MODELS_PATH, MODELS_VERSION, ModelPiece, ModelSet,
    };
    use veyra_weapons::{Slot, Weapon, model::muzzle_tip, modules, picture};
    let dir = MODELS_PATH.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mesher = ManifoldMesher::default();
    let mut props = Vec::new();
    let mut set = ModelSet {
        version: MODELS_VERSION,
        materials: WEAPON_LIBRARY.to_owned(),
        chassis: std::collections::BTreeMap::new(),
    };
    for weapon in Weapon::ALL {
        let key = weapon.key();
        let maker = weapon.maker();
        let builds = picture::plan(weapon)
            .into_iter()
            .map(|assembly| {
                let building = veyra_weapons::build(&assembly)
                    .with_context(|| format!("the recipe of {key}"))?;
                let mesh = mesh_building(&building, &mesher)
                    .with_context(|| format!("the mesh of {key}"))?;
                Ok((assembly, mesh))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        // The elements of `slot` (or the chassis) in the build that holds
        // `wanted` there; `selected` is what the recipe itself asks.
        let source = |slot: Option<Slot>, wanted: Option<&str>| {
            let (instance, build) = match slot {
                None => ("chassis", builds.first()),
                Some(slot) => (
                    slot.key(),
                    builds
                        .iter()
                        .find(|(a, _)| a.selected(slot).map(|m| m.id) == wanted),
                ),
            };
            let (_, holder) =
                build.with_context(|| format!("no build of {key} holds {instance}"))?;
            let part = &holder
                .building
                .instance(instance)
                .with_context(|| format!("{key} has no {instance}"))?
                .part;
            holder
                .parts
                .get(part)
                .with_context(|| format!("{key} has no part {part}"))
        };
        let mut kinds = vec![(picture::PieceKind::Chassis, None, None)];
        for &slot in weapon.slots() {
            kinds.push((picture::PieceKind::Standard(slot), Some(slot), None));
            for module in modules(weapon, slot) {
                kinds.push((
                    picture::PieceKind::Module(slot, module.id),
                    Some(slot),
                    Some(module.id),
                ));
            }
        }
        let mut pieces = Vec::new();
        let mut chassis_tip = [0.0; 3];
        for (kind, slot, module) in kinds {
            let elements = source(slot, module)?;
            let tip = muzzle_tip(elements.iter().map(|element| &element.mesh));
            if slot.is_none() {
                chassis_tip = tip.unwrap_or_default();
            }
            let level = PropLevel {
                pieces: elements
                    .iter()
                    .filter(|element| !element.mesh.indices.is_empty())
                    .map(|element| PropPiece {
                        mesh: element.mesh.clone(),
                        material: weapon_material(maker, &element.material_slot),
                    })
                    .collect(),
                ..Default::default()
            };
            let stem = kind.file_stem();
            let file = format!("{dir}/{key}/{stem}.ashprop");
            props.push((
                file.clone(),
                Prop::new(
                    format!("veyra:weapon.{key}.{stem}"),
                    vec![level],
                    Vec::new(),
                ),
            ));
            pieces.push(ModelPiece {
                mount: kind.mount().map(|slot| slot.key().to_owned()),
                module: kind.module().map(str::to_owned),
                file,
                tip: if slot == Some(Slot::Muzzle) {
                    tip
                } else {
                    None
                },
            });
        }
        set.chassis.insert(
            key.to_owned(),
            ChassisModels {
                muzzle: chassis_tip,
                pieces,
            },
        );
    }
    Ok((props, set))
}
