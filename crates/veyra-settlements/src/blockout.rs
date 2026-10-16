//! Port Serein's blockout: plain massing for the slice's share of the city
//! (`docs/design/port-serein.md`, section 11), fitted to the city's ground so
//! its routes, sight lines and scale can be walked before any detail exists.
//!
//! The ground is the settlement's own (ADR 0009): its landform, terraced, with
//! its platforms. The plan reads nothing else, so it fits whatever landform
//! the registry describes, and a change to the world's generator never
//! reaches it.
//!
//! Nothing here stands on a grid. What the design fixes is fixed: the
//! platforms, the main stair's line, the pipelines' gully. Everything else is
//! found by reading the ground, in the order a town grows. The quay wall and
//! the cranes follow the shoreline. The transit deck follows a contour on its
//! piers. Stairs zig-zag down the fall line wherever a terrace wall can take a
//! flight. Pelagic's slabs take the long terraces east of the pipelines. The
//! Terraces' houses fill what is left, in rows against each wall, of uneven
//! widths and heights, with gaps for gardens and cisterns.
//!
//! Three greys say which generation a mass belongs to: dark for Pelagic's,
//! light for the Landing's and the new, mid for the Terraces'. The processor
//! and the pads wear an accent so the landmarks read.

#![allow(
    clippy::many_single_char_names,
    reason = "frame coordinates x, y, z and the ground g read as the geometry does"
)]

use std::collections::BTreeMap;

use anyhow::Context;
use ashlar::glam::{DMat3, DQuat, DVec3};
use ashlar::{Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode};

use crate::types::{self, Kit, TerraceHouse, TreeKind};
use crate::{PlannedBuilding, PlannedLight, PlannedPlant, SettlementPlan, landmarks};

mod diagram;
mod dressing;
pub use diagram::diagram;

/// A level platform: its centre and half size in the frame, and its level.
pub type Level = ([f64; 2], [f64; 2], f64);

/// The city's ground and its platforms, in the settlement frame.
pub struct Ground<'a> {
    /// The ground's height at a frame point `(x, z)`.
    pub height: &'a dyn Fn(f64, f64) -> f64,
    /// Whether a frame point is the city's own ground, where it may build.
    pub owns: &'a dyn Fn(f64, f64) -> bool,
    /// The starport's platform, a level above the Exchange.
    pub starport: Level,
    /// The Exchange's square.
    pub exchange: Level,
    /// The processor's platform on the ridge.
    pub processor: Level,
    /// The headquarters' own terrace in the Heights.
    pub headquarters: Level,
    /// The quay's level.
    pub quay: f64,
    /// Where the road leaves the city, frame `(x, z)`.
    pub road_exit: [f64; 2],
}

/// The blockout's material keys, by generation. `tools/content` defines
/// them as plain colours.
pub const MATERIALS: [(&str, &str, [f32; 3]); 4] = [
    ("pelagic", "blockout:pelagic", [0.16, 0.16, 0.17]),
    ("landing", "blockout:landing", [0.72, 0.70, 0.66]),
    ("terrace", "blockout:terrace", [0.46, 0.44, 0.41]),
    ("accent", "blockout:accent", [0.55, 0.32, 0.18]),
];

/// The structure's slots and the finished materials they bind: the city's
/// frame is the first thing past the blockout to get real surfaces.
pub const STRUCTURE: [(&str, &str); 12] = [
    ("wall", "serein:terrace-wall"),
    ("masonry", "serein:terrace-masonry"),
    ("bed", "serein:hedge"),
    ("company", "serein:company-concrete"),
    ("sea-wall", "serein:sea-wall"),
    ("steel", "serein:dark-steel"),
    ("pipe", "serein:pipe"),
    ("pad", "serein:pad-deck"),
    ("hazard", "serein:hazard"),
    ("leaves", "serein:canopy"),
    ("pad-light", "serein:pad-light"),
    ("rail-lamp", "serein:rail-lamp"),
];

/// How far a surface that meets the terrain stands proud of it, metres: a
/// face laid exactly on the ground flickers with it.
const PROUD: f64 = 0.02;

/// The main stair's line: down the hillside from the Exchange's downhill
/// edge to the quay.
const STAIR_X: f64 = -40.0;
/// Its width, metres.
const STAIR_WIDTH: f64 = 8.0;
/// A lane stair's width between terraces, metres.
const LANE_STAIR_WIDTH: f64 = 2.6;
/// A stair's riser and tread, metres (the style's scale).
const RISER: f64 = 0.18;
const TREAD: f64 = 0.40;
/// The pipelines' line down the hillside, in the gully east of the
/// starport.
const PIPE_X: f64 = 150.0;
/// The contour the transit line follows, and how far above it the deck
/// runs: low enough under the Exchange that the square looks over it to the
/// bay, and sees the station at the stair's foot.
const TRANSIT_CONTOUR_M: f64 = 22.0;
const TRANSIT_CLEARANCE_M: f64 = 8.0;
/// The deck's width, and how far its slab hangs under its level, metres.
const TRANSIT_WIDTH_M: f64 = 7.0;
const TRANSIT_SLAB_M: f64 = 0.4;
/// The room a walker on a stair under the deck has to its slab, metres: a
/// tall walker and a hand.
const TRANSIT_HEADROOM_M: f64 = 2.3;

/// The blockout of Port Serein over `ground`.
///
/// # Errors
///
/// If a recipe does not validate, which would be a bug here.
pub fn port_serein(ground: &Ground<'_>) -> anyhow::Result<SettlementPlan> {
    let mut city = layout_city(ground)?;
    let spawn = spawn(&city, ground);
    let mut buildings = std::mem::take(&mut city.landmarks);
    for (name, kits) in std::mem::take(&mut city.kits) {
        buildings.push(kits_building(name, &kits).with_context(|| name)?);
    }
    for (district, masses) in city.districts {
        buildings.push(district_building(&district, &masses).with_context(|| district.clone())?);
    }
    Ok(SettlementPlan {
        id: crate::PORT_SEREIN,
        buildings,
        spawn,
        lobby: spawn,
        road_exit: ground.road_exit,
        lights: city.lights,
        plants: city.plants,
    })
}

fn layout_city(ground: &Ground<'_>) -> anyhow::Result<City> {
    let mut city = City::default();
    // The landmarks' recipes are the fixed structure's heart.
    // The fixed structure first, then movement, then the lots: a house takes
    // whatever level ground the rest left.
    starport(&mut city, ground).context("the terminal")?;
    exchange(&mut city, ground).context("the beacon")?;
    main_stair(&mut city, ground);
    quay(&mut city, ground).context("the quay")?;
    processor(&mut city, ground).context("the processor")?;
    transit(&mut city, ground);
    gate(&mut city, ground);
    let headquarters = headquarters(&mut city, ground).context("the headquarters")?;
    lane_stairs(&mut city, ground);
    cross_courts(&mut city, ground);
    walls(&mut city, ground);
    let slabs = pelagic(&mut city, ground, headquarters.as_ref()).context("the slabs")?;
    if let Some(hq) = headquarters {
        bridge(&mut city, &hq, &slabs);
    }
    split_houses(&mut city, ground);
    terraces(&mut city, ground);
    dressing::dress(&mut city, ground);
    verges(&mut city, ground);

    Ok(city)
}

/// One mass: a solid at a pose, in a generation's grey, with the finer
/// elements that dress it.
struct Mass {
    geometry: Geometry,
    pose: Pose,
    slot: &'static str,
    footprint: [f64; 4],
    /// Elements in the mass's frame with no collider of their own (trim,
    /// drains, planting beds), each its own mesh so a coarse level can drop
    /// the fine ones.
    details: Vec<Detail>,
}

/// A dressing element of a [`Mass`].
struct Detail {
    name: &'static str,
    geometry: Geometry,
    slot: &'static str,
}

/// The masses so far, by district (a district is one baked building), and
/// the ground kept open for lanes.
#[derive(Default)]
struct City {
    districts: BTreeMap<String, Vec<Mass>>,
    open: Vec<[f64; 4]>,
    landmarks: Vec<PlannedBuilding>,
    kits: BTreeMap<&'static str, Vec<PlacedKit>>,
    lots: Vec<Rect>,
    lights: Vec<PlannedLight>,
    plants: Vec<PlannedPlant>,
    /// Small squares claimed before houses, so all later rows respect the
    /// same cross-lane instead of filling each other's gaps.
    courts: Vec<(Lot, f64)>,
    /// The foot of every retaining wall piece: its two ends in the frame
    /// and the level of the terrace below, for the verges planted there
    /// once the houses stand.
    feet: Vec<([f64; 2], [f64; 2], f64)>,
    /// The walked surface of every stair so far, a piece a landing and a
    /// piece a flight, for what is built over them to leave a walker room.
    walks: Vec<Walk>,
}

/// A piece of a stair's walked surface: over `x`, from `z[0]` to `z[1]` down
/// the frame's `+z`, its height running from `y[0]` to `y[1]`.
#[derive(Clone, Copy, Debug)]
struct Walk {
    x: [f64; 2],
    z: [f64; 2],
    y: [f64; 2],
}

impl Walk {
    /// Its highest point between `z0` and `z1`, if it reaches them.
    fn highest(&self, z0: f64, z1: f64) -> Option<f64> {
        let (a, b) = (z0.max(self.z[0]), z1.min(self.z[1]));
        if a > b {
            return None;
        }
        let at = |z: f64| {
            let t = (z - self.z[0]) / (self.z[1] - self.z[0]).max(1e-9);
            self.y[0] + (self.y[1] - self.y[0]) * t
        };
        Some(at(a).max(at(b)))
    }
}

impl City {
    /// A box of `size`, its footprint centred on `(x, z)` in the frame,
    /// standing on `base`, turned by `yaw`.
    fn block(
        &mut self,
        district: &str,
        [x, base, z]: [f64; 3],
        size: [f64; 3],
        yaw: f64,
        slot: &'static str,
    ) {
        let (sin, cos) = yaw.sin_cos();
        let hx = f64::midpoint(cos.abs() * size[0], sin.abs() * size[2]);
        let hz = f64::midpoint(sin.abs() * size[0], cos.abs() * size[2]);
        self.push(
            district,
            Mass {
                geometry: Geometry::cuboid(size).placed(Pose::at([
                    -size[0] * 0.5,
                    0.0,
                    -size[2] * 0.5,
                ])),
                pose: Pose::at([x, base, z]).rotated(DQuat::from_rotation_y(yaw)),
                slot,
                footprint: [x - hx, z - hz, x + hx, z + hz],
                details: Vec::new(),
            },
        );
    }

    /// Whether a lot, grown by `margin`, is clear of every mass, lane and
    /// lot so far: its box against masses and lanes, its rectangle against
    /// the other lots.
    fn fits(&self, lot: &Lot, margin: f64) -> bool {
        let [x0, z0, x1, z1] = lot.footprint;
        let apart = |[a0, b0, a1, b1]: [f64; 4]| {
            x1 + margin <= a0 || x0 - margin >= a1 || z1 + margin <= b0 || z0 - margin >= b1
        };
        self.districts
            .iter()
            .filter(|(district, _)| *district != "walls")
            .flat_map(|(_, masses)| masses)
            .all(|mass| apart(mass.footprint))
            && self.open.iter().all(|&lane| apart(lane))
            && self
                .lots
                .iter()
                .all(|other| !other.overlaps(&lot.rect, margin))
    }

    /// An upright cylinder of `radius` and `height` on `base` at `(x, z)`.
    fn column(
        &mut self,
        district: &str,
        [x, base, z]: [f64; 3],
        radius: f64,
        height: f64,
        slot: &'static str,
    ) {
        self.push(
            district,
            Mass {
                geometry: Geometry::cylinder(radius, height, 24),
                pose: Pose::at([x, base, z]),
                slot,
                footprint: [x - radius, z - radius, x + radius, z + radius],
                details: Vec::new(),
            },
        );
    }

    /// A cylinder of `radius` from `a` to `b`, in the frame.
    fn pipe(&mut self, district: &str, a: DVec3, b: DVec3, radius: f64, slot: &'static str) {
        let along = b - a;
        let length = along.length();
        if length < 0.1 {
            return;
        }
        let turn = DQuat::from_rotation_arc(DVec3::Y, along / length);
        self.push(
            district,
            Mass {
                geometry: Geometry::cylinder(radius, length, 12),
                pose: Pose::at(a.to_array()).rotated(turn),
                slot,
                footprint: [
                    a.x.min(b.x) - radius,
                    a.z.min(b.z) - radius,
                    a.x.max(b.x) + radius,
                    a.z.max(b.z) + radius,
                ],
                details: Vec::new(),
            },
        );
    }

    /// A landmark, placed whole, whose `footprints` the lots keep off.
    fn landmark(&mut self, planned: PlannedBuilding, footprints: &[[f64; 4]]) {
        self.open.extend_from_slice(footprints);
        self.landmarks.push(planned);
    }

    fn push(&mut self, district: &str, mass: Mass) {
        self.districts
            .entry(district.to_owned())
            .or_default()
            .push(mass);
    }

    /// Whether `footprint`, grown by `margin`, is clear of every mass and
    /// every lane so far.
    /// Whether a verge plant may stand on `footprint` at a wall's foot: off
    /// every mass but the walls (by a stride, so a stair's foot stays
    /// free), every lot, and every spot a kit or a landing keeps open. The
    /// corridors kept open between stairs span whole terraces, and a plant
    /// against their back wall blocks nobody's way, so those are allowed.
    fn verge_clear(&self, [x0, z0, x1, z1]: [f64; 4]) -> bool {
        const CORRIDOR: f64 = 150.0;
        let apart = |[a0, b0, a1, b1]: [f64; 4], margin: f64| {
            x1 + margin <= a0 || x0 - margin >= a1 || z1 + margin <= b0 || z0 - margin >= b1
        };
        let rect = Rect::from_box([x0, z0, x1, z1]);
        self.districts
            .iter()
            .filter(|(district, _)| *district != "walls")
            .flat_map(|(_, masses)| masses)
            .all(|mass| apart(mass.footprint, 1.2))
            && self
                .open
                .iter()
                .filter(|[a0, b0, a1, b1]| (a1 - a0) * (b1 - b0) < CORRIDOR)
                .all(|&spot| apart(spot, 0.3))
            && self.lots.iter().all(|lot| !lot.overlaps(&rect, 0.3))
            && self.landmarks.iter().all(|l| apart(l.footprint, 0.5))
    }

    fn clear(&self, [x0, z0, x1, z1]: [f64; 4], margin: f64) -> bool {
        let apart = |[a0, b0, a1, b1]: [f64; 4]| {
            x1 + margin <= a0 || x0 - margin >= a1 || z1 + margin <= b0 || z0 - margin >= b1
        };
        // The retaining walls stand where the ground is not level, so a lot
        // that passed its level test is off them already; a house backs
        // onto one.
        let rect = Rect::from_box([x0, z0, x1, z1]);
        self.districts
            .iter()
            .filter(|(district, _)| *district != "walls")
            .flat_map(|(_, masses)| masses)
            .all(|mass| apart(mass.footprint))
            && self.open.iter().all(|&lane| apart(lane))
            && self.lots.iter().all(|lot| !lot.overlaps(&rect, margin))
    }
}

/// A rod of `radius` from `a` to `b`.
fn bar(a: [f64; 3], b: [f64; 3], radius: f64) -> Geometry {
    let from = DVec3::from_array(a);
    let along = DVec3::from_array(b) - from;
    Geometry::cylinder(radius, along.length(), 6)
        .placed(Pose::at(a).rotated(DQuat::from_rotation_arc(DVec3::Y, along.normalize())))
}

/// The handrails of one flight, in its profile's frame (`x` down the run,
/// `y` across, `z` up): a rail each side, and one down the middle of a wide
/// stair, 0.9 m over the line of the nosings and level over the landing,
/// on posts at its ends.
fn handrails(landing: f64, run_end: f64, riser: f64, width: f64) -> Geometry {
    const HEIGHT: f64 = 0.9;
    let slope = riser / TREAD;
    let end = run_end - TREAD * 0.5;
    let low = -(end - landing) * slope;
    let mut sides = vec![0.12, width - 0.12];
    if width > 4.5 {
        sides.push(width * 0.5);
    }
    let mut parts = Vec::new();
    for y in sides {
        parts.push(bar([landing, y, HEIGHT], [end, y, low + HEIGHT], 0.03));
        parts.push(bar(
            [end, y, low - riser],
            [end, y, low + HEIGHT + 0.03],
            0.025,
        ));
        parts.push(bar([landing, y, 0.0], [landing, y, HEIGHT + 0.03], 0.025));
        if landing > 0.6 {
            parts.push(bar([0.15, y, HEIGHT], [landing, y, HEIGHT], 0.03));
            parts.push(bar([0.15, y, 0.0], [0.15, y, HEIGHT + 0.03], 0.025));
        }
    }
    Geometry::union_all(parts)
}

/// A straight stair down the frame's `+z` from `(x, top, z)`: `drop` metres
/// in flights of at most three, each after a landing of `landing` metres,
/// filled solid down to `base`. Returns the `z` of its foot.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "step counts of a few hundred"
)]
#[allow(clippy::too_many_arguments, reason = "a stair's few dimensions")]
fn stair(
    city: &mut City,
    district: &str,
    slot: &'static str,
    [x, top, z]: [f64; 3],
    drop: f64,
    width: f64,
    landing: f64,
    base: f64,
) -> f64 {
    let steps = (drop / RISER).ceil().max(1.0) as usize;
    let riser = drop / steps as f64;
    let per_flight = (3.0 / riser).round().max(1.0) as usize;
    // Each flight's profile lies in the stair's own (run, rise) plane and is
    // extruded across its width: local x runs down the frame's z, local z is
    // up, local y is across.
    let turn = DQuat::from_mat3(&DMat3::from_cols(DVec3::Z, DVec3::X, DVec3::Y));
    let (mut z, mut y, mut left) = (z, top, steps);
    while left > 0 {
        let count = per_flight.min(left);
        left -= count;
        let mut profile = vec![[0.0, 0.0]];
        for s in 0..count {
            let run = landing + s as f64 * TREAD;
            let rise = -(s as f64) * riser;
            profile.push([run, rise]);
            profile.push([run, rise - riser]);
        }
        let run_end = landing + count as f64 * TREAD;
        let fall = count as f64 * riser;
        let bottom = (base - y).min(-fall - 0.5);
        let across = [x - width * 0.5, x + width * 0.5];
        city.walks.push(Walk {
            x: across,
            z: [z, z + landing],
            y: [y, y],
        });
        city.walks.push(Walk {
            x: across,
            z: [z + landing, z + run_end],
            y: [y, y - fall],
        });
        profile.push([run_end, -fall]);
        profile.push([run_end, bottom]);
        profile.push([0.0, bottom]);
        city.push(
            district,
            Mass {
                geometry: Geometry::extrude(profile, width),
                pose: Pose::at([x - width * 0.5, y + PROUD, z]).rotated(turn),
                slot,
                footprint: [x - width * 0.5, z, x + width * 0.5, z + run_end],
                details: vec![Detail {
                    name: "rails",
                    geometry: handrails(landing, run_end, riser, width),
                    slot: "steel",
                }],
            },
        );
        z += run_end;
        y -= fall;
    }
    z
}

/// A small generator for the blockout's choices, so a plan is the same
/// every bake.
struct Dice(u64);

impl Dice {
    #[allow(clippy::cast_precision_loss, reason = "53 bits into a unit float")]
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }
}

/// The starport, a level above the Exchange: two pads in cradles, tanks
/// behind, and the terminal on its western edge, whose podium is the wall
/// between the pads and the square and whose grand stair comes down into
/// it.
fn starport(city: &mut City, g: &Ground<'_>) -> anyhow::Result<()> {
    let ([cx, cz], [hx, _], y) = g.starport;
    let ([ex, _], [ehx, _], below) = g.exchange;
    for pad in [[cx + 8.0, cz - 26.0], [cx + 14.0, cz + 27.0]] {
        cradle(city, [pad[0], y, pad[1]]);
    }
    for (i, dz) in [-4.0, 9.0, 22.0].into_iter().enumerate() {
        let r = if i == 1 { 5.5 } else { 4.5 };
        city.column("starport", [cx + hx - 10.0, y, cz + dz], r, 9.0, "pelagic");
    }
    let (x, z) = (ex + ehx, cz + 4.0);
    city.landmark(
        PlannedBuilding {
            name: "terminal".to_owned(),
            building: landmarks::terminal(y - below)?,
            position: [x, below, z],
            yaw: 0.0,
            footprint: [x - 20.0, z - 46.0, x + 34.0, z + 46.0],
        },
        &[
            [x, z - 46.0, x + 34.0, z + 46.0],
            [x - 20.0, z - 7.0, x, z + 7.0],
        ],
    );
    Ok(())
}

/// A landing pad in its cradle: a raised deck, a hazard band round its
/// rim, and blast walls on the uphill side, open towards the terminal and
/// the bay.
fn cradle(city: &mut City, [x, y, z]: [f64; 3]) {
    city.column("starport", [x, y, z], 21.5, 0.45, "hazard");
    city.column("starport", [x, y, z], 20.0, 0.6, "pad");
    // Lights set in the rim, every thirty degrees.
    for i in 0..12 {
        let (s, c) = (std::f64::consts::TAU * f64::from(i) / 12.0).sin_cos();
        city.block(
            "starport",
            [x + c * 20.8, y + 0.45, z + s * 20.8],
            [0.5, 0.2, 0.5],
            0.0,
            "pad-light",
        );
    }
    for i in 0..5 {
        // Round the uphill half, from the west to the east.
        let angle = std::f64::consts::PI * (1.1 + 0.2 * f64::from(i));
        let (s, c) = angle.sin_cos();
        city.block(
            "starport",
            [x + c * 25.0, y, z + s * 25.0],
            [10.5, 4.0, 1.2],
            (-c).atan2(-s),
            "company",
        );
    }
}

/// The Exchange: the beacon tower off the square's centre, old modules
/// along its uphill and western edges with lanes between them into the
/// Terraces, stalls along two curving aisles, and a canteen and the clinic
/// on the bay side, leaving the view down the main stair open.
#[allow(clippy::too_many_lines, reason = "one square, edge by edge")]
fn exchange(city: &mut City, g: &Ground<'_>) -> anyhow::Result<()> {
    let ([cx, cz], [hx, hz], y) = g.exchange;
    let mut dice = Dice(0x51ab_1e5e_ed00_0001);
    let beacon = [cx + 12.0, cz - 10.0];
    city.landmark(
        PlannedBuilding {
            name: "beacon".to_owned(),
            building: landmarks::beacon(48.0)?,
            position: [beacon[0], y, beacon[1]],
            // Its door towards the stair's head and the bay.
            yaw: 0.3,
            footprint: [
                beacon[0] - 5.0,
                beacon[1] - 5.0,
                beacon[0] + 5.0,
                beacon[1] + 5.0,
            ],
        },
        &[],
    );
    // The square is paved by the ground itself (the platform is an apron,
    // laid in flagstones): a slab laid over it met the terrain's facets at a
    // grazing angle where the ground rises past the platform's edge, and cut
    // a sawtooth there.
    // The Landing's modules along the uphill edge, west to east, with lanes
    // between them up into the Terraces, and down the western edge; each
    // turned to the square.
    let module = |dice: &mut Dice, width: f64, depth: f64| {
        let shop = dice.unit() < 0.6;
        types::LandingModule {
            width: width.round(),
            depth: depth.round(),
            storeys: if dice.unit() < 0.35 { 2 } else { 1 },
            shop,
            awning: if shop {
                dice.unit() < 0.75
            } else {
                dice.unit() < 0.25
            },
            dome: dice.unit() < 0.35,
        }
    };
    let colours = |dice: &mut Dice| {
        // Most of the Exchange trades into the evening: its portholes and
        // its openings lit.
        let mut bindings = vec![
            ("shell", pick(dice, &[3, 2], &types::SHELLS)),
            ("sign", pick(dice, &[1, 1, 1, 1, 1], &types::SHUTTERS)),
            ("fabric", pick(dice, &[1, 1, 1], &types::FABRICS)),
            ("pane", pick(dice, &[3, 6, 1], &types::PANES)),
        ];
        if dice.unit() < 0.75 {
            bindings.push(("recess", types::SHOP_LIT));
        }
        bindings
    };
    let mut x = cx - hx + 14.0;
    while x < cx + hx - 30.0 {
        let width = dice.range(8.0, 15.0);
        let depth = dice.range(8.0, 12.0);
        if dice.unit() < 0.2 {
            x += dice.range(4.0, 6.0);
            continue;
        }
        let m = module(&mut dice, width, depth);
        let z = cz - hz + m.depth * 0.5 + dice.range(1.5, 3.5);
        let bindings = colours(&mut dice);
        let at = [x + m.width * 0.5, z];
        place_kit(
            city,
            "exchange-market",
            Kit::Module(m),
            at,
            y,
            dice.range(-0.04, 0.04),
            m.width.max(m.depth) * 0.5,
            bindings,
        );
        x += m.width + dice.range(0.5, 2.0);
    }
    let mut z = cz - hz + 20.0;
    while z < cz + hz - 24.0 {
        let width = dice.range(9.0, 14.0);
        let depth = dice.range(8.0, 11.0);
        let m = module(&mut dice, width, depth);
        let bindings = colours(&mut dice);
        let at = [
            cx - hx + m.depth * 0.5 + dice.range(1.5, 3.0),
            z + m.width * 0.5,
        ];
        place_kit(
            city,
            "exchange-market",
            Kit::Module(m),
            at,
            y,
            std::f64::consts::FRAC_PI_2 + dice.range(-0.04, 0.04),
            m.width.max(m.depth) * 0.5,
            bindings,
        );
        z += m.width + dice.range(1.0, 4.0);
    }
    // The canteen and the clinic on the bay side, facing the square either
    // side of the stair's head, the clinic in the new white.
    let canteen = types::LandingModule {
        width: 17.0,
        depth: 11.0,
        storeys: 1,
        shop: true,
        awning: true,
        dome: false,
    };
    // Two metres further out than the clinic's mirror was: the game's bench
    // row stands its foreman at frame (-52, 58), and a canteen nearer than
    // this put him on its roof (the walkability audit in the game's tests).
    place_kit(
        city,
        "exchange-market",
        Kit::Module(canteen),
        [STAIR_X - 22.0, cz + hz - 8.0],
        y,
        std::f64::consts::PI + 0.05,
        9.0,
        vec![
            ("shell", "serein:composite"),
            ("sign", "serein:paint-oxide"),
            ("fabric", "serein:awning-red"),
            ("pane", "serein:pane-warm"),
            ("recess", types::SHOP_LIT),
        ],
    );
    let clinic = types::LandingModule {
        width: 14.0,
        depth: 11.0,
        storeys: 2,
        shop: false,
        awning: true,
        dome: true,
    };
    place_kit(
        city,
        "exchange-market",
        Kit::Module(clinic),
        [STAIR_X + 22.0, cz + hz - 9.0],
        y,
        std::f64::consts::PI - 0.04,
        8.0,
        vec![
            ("shell", "serein:composite-new"),
            ("pane", types::PANES[1]),
            ("sign", "serein:paint-teal"),
            ("fabric", "serein:awning-teal"),
        ],
    );
    // Stalls along two aisles that bow across the square, each facing its
    // aisle.
    for (offset, bend) in [(-12.0, 6.0), (13.0, 4.0)] {
        let mut u = -1.0;
        while u < 1.0 {
            let ax = cx + u * (hx - 26.0);
            let az = cz + offset + bend * (u * u - 0.35);
            let slope = 2.0 * bend * u / (hx - 26.0);
            let yaw = (-slope).atan();
            for side in [-1.0, 1.0] {
                if dice.unit() < 0.18 {
                    continue;
                }
                let stall = types::Stall {
                    width: (dice.range(2.6, 4.2) * 2.0).round() * 0.5,
                    depth: (dice.range(2.4, 3.2) * 2.0).round() * 0.5,
                    canopy: match dice.unit() {
                        r if r < 0.4 => types::Canopy::Peak,
                        r if r < 0.75 => types::Canopy::Slope,
                        _ => types::Canopy::Flat,
                    },
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "0 to 3"
                    )]
                    goods: 1 + (dice.unit() * 4.0).floor() as u32,
                    lift: (dice.range(-0.3, 0.5) * 10.0).round() * 0.1,
                    sign: dice.unit() < 0.4,
                };
                let off = side * (2.2 + stall.depth * 0.5);
                let at = [ax - off * yaw.sin(), az + off * yaw.cos()];
                if (at[0] - beacon[0]).hypot(at[1] - beacon[1]) < 9.0 {
                    continue;
                }
                // Its front to the aisle.
                let turn = if side < 0.0 {
                    yaw
                } else {
                    yaw + std::f64::consts::PI
                };
                let bindings = vec![
                    ("fabric", pick(&mut dice, &[1, 1, 1], &types::FABRICS)),
                    ("goods", pick(&mut dice, &[1, 1, 1, 1, 1], &types::SHUTTERS)),
                    ("sign", pick(&mut dice, &[1, 1, 1, 1, 1], &types::SHUTTERS)),
                ];
                place_kit(
                    city,
                    "exchange-market",
                    Kit::Stall(stall),
                    at,
                    y,
                    turn + dice.range(-0.08, 0.08),
                    stall.width.max(stall.depth) * 0.5,
                    bindings,
                );
            }
            if dice.unit() < 0.3 {
                city.lights.push(PlannedLight {
                    position: [ax, y + 2.6, az],
                    color: [1.0, 0.66, 0.34],
                    lumens: 2500.0,
                    range: 10.0,
                });
            }
            u += dice.range(4.0, 5.5) / (hx - 26.0);
        }
    }
    // Shade trees along the modules' fronts and either side of the stair's
    // head.
    let mut shade = |at: [f64; 2], dice: &mut Dice| {
        plant_at(city, dice, at, y, TreeKind::Shade);
    };
    let mut courts = Vec::new();
    let mut x = cx - hx + 22.0;
    while x < cx + hx - 20.0 {
        if (x - beacon[0]).abs() > 9.0 {
            shade([x, cz - hz + 20.0], &mut dice);
            courts.push([x, cz - hz + 20.0]);
        }
        x += dice.range(18.0, 26.0);
    }
    shade([STAIR_X - 10.0, cz + hz - 20.0], &mut dice);
    shade([STAIR_X + 11.0, cz + hz - 21.0], &mut dice);
    courts.extend([
        [STAIR_X - 10.0, cz + hz - 20.0],
        [STAIR_X + 11.0, cz + hz - 21.0],
    ]);
    // A court round each tree: benches facing out from its shade, three or
    // four of them, and a planter or two at its edge.
    for &[tx, tz] in &courts {
        let turn = dice.range(0.0, std::f64::consts::TAU);
        let count = if dice.unit() < 0.5 { 3 } else { 4 };
        for k in 0..count {
            let a = turn + std::f64::consts::TAU * f64::from(k) / f64::from(count);
            let (sa, ca) = a.sin_cos();
            let at = [tx + ca * 2.6, tz + sa * 2.6];
            place_kit(
                city,
                "exchange-market",
                Kit::Furniture(types::Furniture::Bench),
                at,
                y,
                ca.atan2(sa),
                1.0,
                Vec::new(),
            );
        }
        if dice.unit() < 0.7 {
            let a = turn + std::f64::consts::PI / f64::from(count);
            let at = [tx + a.cos() * 4.6, tz + a.sin() * 4.6];
            place_kit(
                city,
                "exchange-market",
                Kit::Furniture(types::Furniture::Planter),
                at,
                y,
                a.cos().atan2(a.sin()),
                0.8,
                Vec::new(),
            );
        }
        city.open.push([tx - 5.0, tz - 5.0, tx + 5.0, tz + 5.0]);
        // A string of lamps in the tree's branches over the benches.
        city.lights.push(PlannedLight {
            position: [tx, y + 3.6, tz],
            color: [1.0, 0.7, 0.38],
            lumens: 5000.0,
            range: 14.0,
        });
    }
    // Lamps along the bay side of the square and down its east edge, by the
    // terminal's stair.
    let mut lx = cx - hx + 10.0;
    while lx < cx + hx - 6.0 {
        let at = [lx, cz + hz - 2.5];
        if (lx - STAIR_X).abs() > 6.0
            && city.clear([at[0] - 0.3, at[1] - 0.3, at[0] + 0.3, at[1] + 0.3], 0.2)
        {
            place_kit(
                city,
                "exchange-market",
                Kit::Furniture(types::Furniture::Lamp),
                at,
                y,
                std::f64::consts::PI,
                0.5,
                Vec::new(),
            );
            lamp_light(city, at, y, std::f64::consts::PI);
        }
        lx += 14.0;
    }
    // The Borough's water kiosk at the square's western edge, where a queue
    // can form without blocking the aisles.
    for at in [
        [cx - hx + 16.0, cz + hz - 20.0],
        [cx - hx + 16.0, cz + 6.0],
        [cx - 10.0, cz + hz - 12.0],
    ] {
        let fp = [at[0] - 2.2, at[1] - 1.8, at[0] + 3.2, at[1] + 1.8];
        if city.clear(fp, 0.5)
            && courts
                .iter()
                .all(|c| (c[0] - at[0]).hypot(c[1] - at[1]) > 7.0)
        {
            place_kit(
                city,
                "exchange-market",
                Kit::Furniture(types::Furniture::Kiosk),
                at,
                y,
                0.0,
                2.5,
                vec![
                    ("paint", "serein:paint-teal"),
                    ("sign", "serein:paint-white"),
                ],
            );
            city.open.push(fp);
            break;
        }
    }
    Ok(())
}

/// Cargo on the quay: stacks of containers in a row between the cranes'
/// landward legs and the warehouses, along the shore, with gaps where a
/// forklift would turn.
fn cargo(city: &mut City, g: &Ground<'_>, dice: &mut Dice) {
    const WIDE: f64 = 2.44;
    let mut x = -280.0 + dice.range(0.0, 10.0);
    while x < 280.0 {
        if dice.unit() < 0.2 {
            x += dice.range(8.0, 15.0);
            continue;
        }
        let length = if dice.unit() < 0.7 { 12.2 } else { 6.1 };
        let mid = x + length * 0.5;
        let (Some(a), Some(b), Some(here)) = (
            shoreline(g, mid - 5.0),
            shoreline(g, mid + 5.0),
            shoreline(g, mid),
        ) else {
            x += 5.0;
            continue;
        };
        // Along the shore, halfway between the cranes' landward legs and the
        // warehouses' fronts.
        let (dx, dz) = (10.0_f64, b - a);
        let run = dx.hypot(dz);
        let (ux, uz) = (dx / run, dz / run);
        let yaw = (-uz).atan2(ux);
        let at = [mid, here - 13.5];
        let reach_x = f64::midpoint(ux.abs() * length, uz.abs() * WIDE);
        let reach_z = f64::midpoint(uz.abs() * length, ux.abs() * WIDE);
        let fp = [
            at[0] - reach_x,
            at[1] - reach_z,
            at[0] + reach_x,
            at[1] + reach_z,
        ];
        let level = [
            (fp[0], fp[1]),
            (fp[2], fp[1]),
            (fp[0], fp[3]),
            (fp[2], fp[3]),
        ]
        .into_iter()
        .all(|(x, z)| ((g.height)(x, z) - g.quay).abs() < 0.3);
        if !level || !owned(g, fp) || !city.clear(fp, 0.3) {
            x += 3.0;
            continue;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "1 to 3"
        )]
        let high = 1 + (dice.unit() * 2.99) as u32;
        let stack = types::Containers {
            length,
            high,
            slide: (dice.range(-0.15, 0.15) * 10.0).round() * 0.1,
        };
        let bindings = (0..high)
            .map(|level| {
                let slot = ["container-0", "container-1", "container-2"][level as usize];
                (slot, pick(dice, &[3, 2, 2, 3, 1], &types::CONTAINER_PAINTS))
            })
            .collect();
        place_kit(
            city,
            "quay-cargo",
            Kit::Containers(stack),
            at,
            g.quay,
            yaw,
            length * 0.5,
            bindings,
        );
        city.open.push(fp);
        x += length + dice.range(0.4, 3.0);
    }
}

/// The shortest landing between a main stair's flights, metres: two treads,
/// so a flight of three metres still pauses.
const MAIN_LANDING_MIN: f64 = 0.8;

/// How far the ground may stand over a main stair's flight, metres: where a
/// platform's edge rounds off over the first treads, the walker crosses that
/// ground and steps down onto the stair, which a step this high allows.
const MAIN_BURIED_MAX: f64 = 0.4;

/// A straight flight's run down the frame's `+z` from `(top, z)`, `drop`
/// metres with `landing` before each flight, as [`stair`] lays it, and the
/// height of its walked surface at `at`: the landing, then the line of its
/// nosings, which is the top of the flight's hull.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "step counts of a few hundred"
)]
fn flight_profile(top: f64, z: f64, drop: f64, landing: f64) -> (f64, impl Fn(f64) -> f64) {
    let steps = (drop / RISER).ceil().max(1.0) as usize;
    let riser = drop / steps as f64;
    let per_flight = (3.0 / riser).round().max(1.0) as usize;
    let flights = steps.div_ceil(per_flight);
    let run = flights as f64 * landing + steps as f64 * TREAD;
    let surface = move |at: f64| {
        let (mut from, mut y, mut left) = (z, top, steps);
        while left > 0 {
            let count = per_flight.min(left);
            left -= count;
            let fall = count as f64 * riser;
            let to = from + landing + count as f64 * TREAD;
            if at <= from + landing {
                return y;
            }
            if at <= to {
                return y - (at - from - landing) / (count as f64 * TREAD) * fall;
            }
            from = to;
            y -= fall;
        }
        y
    };
    (run, surface)
}

/// The main stair from the Exchange's downhill edge to the quay, fitted to
/// the terraces it crosses: a flight over each bank, from the upper
/// terrace's edge to the lower terrace's level, and a landing across each
/// terrace flush with its lane, so every terrace it crosses can step onto it
/// and it never runs into a bank. A terrace too short to land on is passed
/// over; the landings of a flight are stretched so it lands where the
/// ground does.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "step counts of a few hundred"
)]
fn main_stair(city: &mut City, g: &Ground<'_>) {
    let ([_, cz], [_, hz], top) = g.exchange;
    let start = cz + hz;
    let shore = shoreline(g, STAIR_X).unwrap_or(start + 400.0);
    let runs: Vec<(f64, f64, f64)> = level_runs(g, STAIR_X, start, shore)
        .into_iter()
        .filter(|&(from, to, level)| {
            from > start && to > from + 1.0 && level < top - 0.5 && level > g.quay - 0.1
        })
        .collect();
    // The flight's surface stands on the ground or over it everywhere across
    // the stair, or under it by no more than a step.
    let over_ground = |z0: f64, z1: f64, surface: &dyn Fn(f64) -> f64| {
        let mut z = z0;
        while z <= z1 {
            let ok = [-0.45, 0.0, 0.45].into_iter().all(|side| {
                (g.height)(STAIR_X + side * STAIR_WIDTH, z) <= surface(z) + MAIN_BURIED_MAX
            });
            if !ok {
                return false;
            }
            z += 0.5;
        }
        true
    };
    let (mut leave, mut level) = (start, top);
    let mut next = 0;
    let mut foot = start;
    while next < runs.len() {
        // The nearest terrace below that a flight from here lands on.
        let landed = runs[next..]
            .iter()
            .enumerate()
            .find_map(|(k, &(from, to, lower))| {
                let drop = level - lower;
                let steps = (drop / RISER).ceil().max(1.0);
                let per_flight = (3.0 / (drop / steps)).round().max(1.0);
                let flights = (steps / per_flight).ceil();
                // Stretched to land a little past the bank's foot, never less
                // than the shortest landing.
                let landing =
                    ((from + 0.5 - leave - steps * TREAD) / flights).max(MAIN_LANDING_MIN);
                let (run, surface) = flight_profile(level, leave, drop, landing);
                let land = leave + run;
                (land >= from && land + 1.0 <= to && over_ground(leave, land, &surface))
                    .then_some((next + k, drop, landing, land))
            });
        let Some((k, drop, landing, land)) = landed else {
            break;
        };
        stair(
            city,
            "stair",
            "company",
            [STAIR_X, level, leave],
            drop,
            STAIR_WIDTH,
            landing,
            g.quay - 1.0,
        );
        let (_, to, lower) = runs[k];
        level = lower;
        foot = land;
        // Across the terrace on its own level to its downhill edge, where
        // the next flight leaves; on the quay, a step past the foot.
        leave = if k + 1 < runs.len() {
            to - 0.25
        } else {
            land + 2.0
        };
        landing_slab(city, [STAIR_X, level, land], leave - land, g.quay - 1.0);
        next = k + 1;
    }
    // Ground no flight fits: the rest straight down to the quay, its landings
    // stretched so it meets the quay where the ground does.
    if level > g.quay + 0.1 {
        let mut quay = leave;
        while quay < leave + 600.0 && (g.height)(STAIR_X, quay) > g.quay + 0.05 {
            quay += 1.0;
        }
        let drop = level - g.quay;
        let steps = (drop / RISER).ceil();
        let flights = (steps * RISER / 3.0).ceil();
        let landing = ((quay - leave - steps * TREAD) / flights).max(2.0);
        foot = stair(
            city,
            "stair",
            "company",
            [STAIR_X, level, leave],
            drop,
            STAIR_WIDTH,
            landing,
            g.quay - 1.0,
        );
    }
    city.open.push([
        STAIR_X - STAIR_WIDTH,
        start - 4.0,
        STAIR_X + STAIR_WIDTH,
        foot + 4.0,
    ]);
}

/// The main stair's paving across a terrace it crosses: flush with the
/// terrace's own level, `length` metres down the frame's `+z` from `(x,
/// level, z)`, filled down to `base`, with no rails, so the terrace's lane
/// walks straight across it.
fn landing_slab(city: &mut City, [x, level, z]: [f64; 3], length: f64, base: f64) {
    if length < 0.1 {
        return;
    }
    let depth = level + PROUD - base;
    city.walks.push(Walk {
        x: [x - STAIR_WIDTH * 0.5, x + STAIR_WIDTH * 0.5],
        z: [z, z + length],
        y: [level, level],
    });
    city.push(
        "stair",
        Mass {
            geometry: Geometry::cuboid([STAIR_WIDTH, depth, length]).placed(Pose::at([
                -STAIR_WIDTH * 0.5,
                -depth,
                0.0,
            ])),
            pose: Pose::at([x, level + PROUD, z]),
            slot: "company",
            footprint: [x - STAIR_WIDTH * 0.5, z, x + STAIR_WIDTH * 0.5, z + length],
            details: Vec::new(),
        },
    );
}

/// Where the water starts along the frame's `x`: the first `z` downhill of
/// the platforms where the ground drops below the sea.
fn shoreline(g: &Ground<'_>, x: f64) -> Option<f64> {
    let mut z = 0.0;
    while z < 400.0 {
        if (g.height)(x, z) < 0.0 {
            return Some(z);
        }
        z += 1.0;
    }
    None
}

/// The quay wall along the shoreline, the crane line on it, and warehouses
/// behind, where the quay is deep enough for one.
#[allow(
    clippy::too_many_lines,
    reason = "the quay's edge, cranes and sheds in one pass along the shore"
)]
fn quay(city: &mut City, g: &Ground<'_>) -> anyhow::Result<()> {
    let mut dice = Dice(0x9a1e_5eed_0000_0003);
    let mut previous: Option<DVec3> = None;
    let mut next_crane = -140.0;
    let mut next_shed = -230.0;
    for step in -30..=30 {
        let x = f64::from(step) * 10.0;
        let Some(shore) = shoreline(g, x).filter(|&z| (g.owns)(x, z)) else {
            previous = None;
            continue;
        };
        let top = DVec3::new(x, g.quay, shore);
        if let Some(prev) = previous {
            // The wall: a slab from the quay's level down into the water,
            // turned along the shore.
            let along = top - prev;
            let yaw = (-along.z).atan2(along.x);
            let mid = (top + prev) * 0.5;
            city.block(
                "quay",
                [mid.x, g.quay - 8.0, mid.z],
                [along.length() + 0.5, 8.0 - PROUD, 2.0],
                yaw,
                "sea-wall",
            );
            // The coping along the edge, covering the wall's top, and a
            // bollard on every piece.
            city.block(
                "quay",
                [mid.x, g.quay, mid.z],
                [along.length() + 0.5, 0.35, 2.3],
                yaw,
                "company",
            );
            city.column(
                "quay",
                [prev.x, g.quay + 0.35, prev.z - 0.9],
                0.3,
                0.7,
                "steel",
            );
        }
        previous = Some(top);
        if x >= next_crane && x < 220.0 {
            // A portal crane, its seaward legs two metres in from the edge.
            let crane = types::Crane {
                reach: dice.range(20.0, 25.0).round(),
                height: dice.range(22.0, 26.0).round(),
            };
            let z = shore - 2.0 - types::Crane::GAUGE;
            // Its floodlights over the quay.
            city.lights.push(PlannedLight {
                position: [x, g.quay + crane.height - 0.6, z + 1.5],
                color: [0.85, 0.92, 1.0],
                lumens: 30_000.0,
                range: 40.0,
            });
            place_kit(
                city,
                "quay-cranes",
                Kit::Crane(crane),
                [x, z],
                g.quay,
                dice.range(-0.05, 0.05),
                5.0,
                Vec::new(),
            );
            next_crane = x + dice.range(30.0, 55.0);
        }
        if x >= next_shed && (x - STAIR_X).abs() > 22.0 && x < 240.0 {
            let (width, depth) = (dice.range(20.0, 34.0), dice.range(14.0, 22.0));
            let back = shore - 14.0 - depth * 0.5;
            let level = |dx: f64, dz: f64| ((g.height)(x + dx, back + dz) - g.quay).abs() < 0.3;
            // The quay is a strip a couple of dozen metres deep: a warehouse
            // stands on it at its front and middle and backs onto the
            // terrace behind, as a harbour's sheds back onto its hill, which
            // its closed walls hide.
            let backs_on = |dx: f64| (g.height)(x + dx, back - depth * 0.5) < g.quay + 5.0;
            let fp = [
                x - width * 0.5,
                back - depth * 0.5,
                x + width * 0.5,
                back + depth * 0.5,
            ];
            if level(-width * 0.5, depth * 0.5)
                && level(width * 0.5, depth * 0.5)
                && level(0.0, 0.0)
                && backs_on(-width * 0.5)
                && backs_on(width * 0.5)
                && city.clear(fp, 2.0)
            {
                let (width, depth) = (width.round(), depth.round());
                let fp = [
                    x - width * 0.5,
                    back - depth * 0.5,
                    x + width * 0.5,
                    back + depth * 0.5,
                ];
                let name = format!("warehouse-{}", city.landmarks.len());
                city.landmark(
                    PlannedBuilding {
                        name,
                        building: types::warehouse(width, dice.range(9.0, 13.0).round(), depth)?,
                        position: [x, g.quay, back],
                        yaw: 0.0,
                        footprint: fp,
                    },
                    &[fp],
                );
                // Crates on pallets beside a door or two.
                for side in [-0.3, 0.3] {
                    if dice.unit() < 0.6 {
                        let at = [
                            x + side * width + dice.range(-1.0, 1.0),
                            back + depth * 0.5 + 1.0,
                        ];
                        place_kit(
                            city,
                            "street",
                            Kit::Furniture(types::Furniture::Crates),
                            at,
                            g.quay,
                            dice.range(-0.3, 0.3),
                            0.8,
                            Vec::new(),
                        );
                        city.open
                            .push([at[0] - 0.8, at[1] - 0.8, at[0] + 0.8, at[1] + 0.8]);
                    }
                }
                next_shed = x + width + dice.range(4.0, 20.0);
            }
        }
    }
    cargo(city, g, &mut dice);
    Ok(())
}

/// The processor on the ridge (its recipe is [`landmarks::processor`]), a
/// manifold from its hall to the gully, and the pipelines down the gully to
/// the bay.
fn processor(city: &mut City, g: &Ground<'_>) -> anyhow::Result<()> {
    let ([cx, cz], [hx, hz], y) = g.processor;
    let hall_width = hx * 1.6;
    let recipe = landmarks::Processor {
        towers: vec![
            landmarks::Tower {
                at: [-45.0, -15.0],
                height: 160.0,
                radius: 17.0,
            },
            landmarks::Tower {
                at: [20.0, -25.0],
                height: 180.0,
                radius: 17.0,
            },
            landmarks::Tower {
                at: [70.0, 5.0],
                height: 120.0,
                radius: 12.0,
            },
        ],
        hall_width,
        hall_front: hz - 5.0,
        tanks: (-hx + 12.0, [-hz + 10.0, 20.0]),
    };
    let footprint = [cx - hx, cz - hz, cx + hx, cz + hz];
    city.landmark(
        PlannedBuilding {
            name: "processor".to_owned(),
            building: landmarks::processor(&recipe)?,
            position: [cx, y, cz],
            yaw: 0.0,
            footprint,
        },
        &[footprint],
    );
    // Two pipelines from the hall down the gully to the water, on saddles
    // every twenty metres, each span clear of the highest ground under it.
    let mut z = cz + hz;
    let mut nodes = Vec::new();
    while (g.height)(PIPE_X, z) >= 0.0 && z < 400.0 {
        let mut high = f64::NEG_INFINITY;
        let mut k = -10.0;
        while k <= 10.0 {
            high = high.max((g.height)(PIPE_X, z + k));
            k += 1.0;
        }
        nodes.push((z, high + 2.5));
        z += 20.0;
    }
    for pair in nodes.windows(2) {
        let ((za, ya), (zb, yb)) = (pair[0], pair[1]);
        for offset in [-1.8, 1.8] {
            let x = PIPE_X + offset;
            city.pipe(
                "processor",
                DVec3::new(x, ya, za),
                DVec3::new(x, yb, zb),
                1.2,
                "pipe",
            );
        }
    }
    for &(z, y) in &nodes {
        let ground = (g.height)(PIPE_X, z);
        city.block(
            "processor",
            [PIPE_X, ground - 0.5, z],
            [6.4, y - ground - 0.7, 1.2],
            0.0,
            "company",
        );
    }
    // The manifold from the hall's east end along its front to the gully.
    if let Some(&(z0, y0)) = nodes.first() {
        let manifold = cz + hz - 8.0;
        for offset in [-1.8, 1.8] {
            let a = DVec3::new(cx + hall_width * 0.5, y + 3.0, manifold + offset);
            let b = DVec3::new(PIPE_X + offset, y + 3.0, manifold + offset);
            city.pipe("processor", a, b, 1.2, "pipe");
            city.pipe(
                "processor",
                b,
                DVec3::new(PIPE_X + offset, y0, z0),
                1.2,
                "pipe",
            );
        }
    }
    city.open.push([PIPE_X - 8.0, cz + hz, PIPE_X + 8.0, 400.0]);
    Ok(())
}

/// The transit line: a deck level above one contour of the hillside, on
/// piers to the ground, bending with the contour round the headland, and a
/// station where the main stair passes under it.
fn transit(city: &mut City, g: &Ground<'_>) {
    // The first point downhill of the platforms where the ground falls to
    // the contour.
    let contour = |x: f64| {
        let mut z = -120.0;
        while z < 300.0 {
            if (g.height)(x, z) <= TRANSIT_CONTOUR_M {
                return Some(z);
            }
            z += 1.0;
        }
        None
    };
    // High enough over every stair it crosses for a walker to pass under its
    // slab, which hangs `TRANSIT_SLAB_M` under the deck's level.
    let deck = city
        .walks
        .iter()
        .filter_map(|walk| {
            let z = contour(f64::midpoint(walk.x[0], walk.x[1]))?;
            let half = TRANSIT_WIDTH_M * 0.5 + 0.5;
            walk.highest(z - half, z + half)
        })
        .map(|top| top + TRANSIT_HEADROOM_M + TRANSIT_SLAB_M)
        .fold(TRANSIT_CONTOUR_M + TRANSIT_CLEARANCE_M, f64::max);
    // No pier on a stair's treads.
    let walks = city.walks.clone();
    let on_stair = |x: f64, z: f64| {
        walks.iter().any(|walk| {
            x > walk.x[0] - 1.5 && x < walk.x[1] + 1.5 && z > walk.z[0] - 1.5 && z < walk.z[1] + 1.5
        })
    };
    let mut previous: Option<DVec3> = None;
    let mut x = -280.0;
    while x <= 280.0 {
        match contour(x).filter(|&z| (g.owns)(x, z)) {
            Some(z) if on_stair(x, z) => {}
            Some(z) => {
                let here = DVec3::new(x, deck, z);
                let ground = (g.height)(x, z);
                // A pier and its crosshead under the deck.
                city.column(
                    "transit",
                    [x, ground - 0.5, z],
                    1.0,
                    deck - ground - 0.5,
                    "company",
                );
                city.block(
                    "transit",
                    [x, deck - TRANSIT_SLAB_M - 1.0, z],
                    [2.0, 1.0, 8.0],
                    0.0,
                    "company",
                );
                if let Some(prev) = previous {
                    let along = here - prev;
                    let mid = (here + prev) * 0.5;
                    let yaw = (-along.z).atan2(along.x);
                    let length = along.length() + 0.4;
                    city.block(
                        "transit",
                        [mid.x, deck - TRANSIT_SLAB_M, mid.z],
                        [length, 1.6, TRANSIT_WIDTH_M],
                        yaw,
                        "company",
                    );
                    // A lamp on the seaward parapet at every pier.
                    let (sin, cos) = yaw.sin_cos();
                    city.block(
                        "transit",
                        [prev.x + sin * 3.3, deck + 1.7, prev.z + cos * 3.3],
                        [0.3, 0.3, 0.3],
                        yaw,
                        "rail-lamp",
                    );
                    // Parapets either side of the track.
                    for side in [-3.3, 3.3] {
                        city.block(
                            "transit",
                            [mid.x + sin * side, deck + 1.2, mid.z + cos * side],
                            [length, 1.0, 0.25],
                            yaw,
                            "steel",
                        );
                    }
                }
                previous = Some(here);
            }
            None => previous = None,
        }
        x += 20.0;
    }
    if let Some(z) = contour(STAIR_X + 14.0) {
        city.block(
            "transit",
            [STAIR_X + 16.0, deck + 1.2, z],
            [22.0, 4.0, 9.0],
            0.0,
            "landing",
        );
    }
}

/// The pass gate where the road leaves over the ridge, just inside the
/// city's ground so it stands level.
fn gate(city: &mut City, g: &Ground<'_>) {
    let [x, exit] = g.road_exit;
    let z = exit + 15.0;
    let y = (g.height)(x, z);
    city.block("gate", [x - 8.0, y, z], [3.0, 9.0, 4.0], 0.0, "pelagic");
    city.block("gate", [x + 8.0, y, z], [3.0, 9.0, 4.0], 0.0, "pelagic");
    city.block("gate", [x, y + 9.0, z], [19.0, 2.0, 4.0], 0.0, "pelagic");
    city.block(
        "gate",
        [x + 22.0, y, z + 6.0],
        [14.0, 6.8, 10.0],
        0.0,
        "pelagic",
    );
}

/// The level runs of ground along the frame's `z` at `x`, uphill first:
/// `(from, to, level)`.
fn level_runs(g: &Ground<'_>, x: f64, from: f64, to: f64) -> Vec<(f64, f64, f64)> {
    let mut runs = Vec::new();
    let mut start: Option<(f64, f64)> = None;
    let mut z = from;
    let mut h = (g.height)(x, z);
    while z < to {
        let next = (g.height)(x, z + 1.0);
        // A gentle bank is still a bank. Recording its first sample as a
        // bench level sank whole rows below the actual flat further along.
        let level = (next - h).abs() < 0.0001;
        match (start, level) {
            (None, true) => start = Some((z, h)),
            (Some((s, lh)), false) => {
                runs.push((s, z, lh));
                start = None;
            }
            _ => {}
        }
        z += 1.0;
        h = next;
    }
    if let Some((s, lh)) = start {
        runs.push((s, to, lh));
    }
    runs
}

/// The stairs between terraces: lines down the fall line at uneven
/// spacing, each flight shifted along its terrace from the one above, so a
/// path zig-zags down the hill and the rows it breaks do not line up.
#[allow(
    clippy::too_many_lines,
    reason = "trace, fit and connect each flight together"
)]
fn lane_stairs(city: &mut City, g: &Ground<'_>) {
    let mut dice = Dice(0x57a1_a5e5_0000_0005);
    let ([_, pz], [_, phz], _) = g.processor;
    let mut lines = vec![PIPE_X - 14.0];
    let mut x = -300.0 + dice.range(5.0, 30.0);
    while x < 300.0 {
        if (x - PIPE_X).abs() > 30.0 && (x - STAIR_X).abs() > 25.0 {
            lines.push(x);
        }
        x += dice.range(40.0, 85.0);
    }
    for x0 in lines {
        let mut previous: Option<(f64, f64)> = None;
        let runs = level_runs(g, x0, pz + phz + 40.0, 300.0);
        for pair in runs.windows(2) {
            let target = pair[0].2;
            // Trace again at the chosen x. Reusing the original column's
            // levels after shifting a stair put its landings inside banks.
            let x = x0 + dice.range(-5.0, 5.0);
            let shifted = level_runs(g, x, pair[0].0 - 2.0, pair[1].1 + 2.0);
            let Some(pair) = shifted.windows(2).find(|p| (p[0].2 - target).abs() < 0.08) else {
                previous = None;
                continue;
            };
            let ((_, upper_to, upper), (lower_from, lower_to, lower)) = (pair[0], pair[1]);
            if lower >= upper - 0.5 || lower < g.quay - 0.1 {
                continue;
            }
            let drop = upper - lower;
            let steps = (drop / RISER).ceil();
            let riser = drop / steps;
            let per_flight = (3.0 / riser).round().max(1.0);
            let run = (steps / per_flight).ceil() * 1.2 + steps * TREAD;
            let top = upper_to - 0.25;
            let level_at = |z, level: f64| {
                [-1.0, 0.0, 1.0].into_iter().all(|side| {
                    ((g.height)(x + side * LANE_STAIR_WIDTH * 0.5, z) - level).abs() < 0.20
                })
            };
            if top + run < lower_from
                || top + run + 1.0 > lower_to
                || !level_at(top, upper)
                || !level_at(top + run, lower)
            {
                previous = None;
                continue;
            }
            let fp = [
                x - LANE_STAIR_WIDTH,
                top - 1.0,
                x + LANE_STAIR_WIDTH,
                top + run + 1.0,
            ];
            if !owned(g, fp) || reserved(g, fp) || !city.clear(fp, 0.5) {
                previous = None;
                continue;
            }
            stair(
                city,
                "lanes",
                "wall",
                [x, upper, top],
                drop,
                LANE_STAIR_WIDTH,
                1.2,
                lower - 0.5,
            );
            // The lane along the terrace from the flight above's foot to this
            // one's head.
            if let Some((px, foot)) = previous
                && foot < top
            {
                city.open
                    .push([px.min(x) - 2.0, foot, px.max(x) + 2.0, top]);
            }
            city.open.push([fp[0], top + run, fp[2], top + run + 4.0]);
            // The Borough's signpost at the stair's head, on the upper lane.
            let sign = [x + LANE_STAIR_WIDTH * 0.5 + 0.8, top - 0.9];
            let spot = [sign[0] - 0.5, sign[1] - 0.5, sign[0] + 0.5, sign[1] + 0.5];
            if ((g.height)(sign[0], sign[1]) - upper).abs() < 0.2 && city.clear(spot, 0.2) {
                let colour = pick(&mut dice, &[1, 1, 1, 1, 1], &types::SHUTTERS);
                place_kit(
                    city,
                    "street",
                    Kit::Furniture(types::Furniture::Signpost),
                    sign,
                    upper,
                    dice.range(-0.4, 0.4),
                    0.5,
                    vec![("sign", colour)],
                );
                city.open.push(spot);
            }
            // The upper lane before the stair's head, kept as its foot's is,
            // so a row along that terrace does not close it.
            city.open.push([
                x - LANE_STAIR_WIDTH * 0.5 - 0.3,
                top - 4.0,
                x + LANE_STAIR_WIDTH * 0.5 + 0.3,
                top,
            ]);
            // A cypress beside some stairs' feet, marking the way up.
            if dice.unit() < 0.35 {
                let side = if dice.unit() < 0.5 { -3.4 } else { 3.4 };
                plant(
                    city,
                    g,
                    &mut dice,
                    [x + side, top + run - 1.5],
                    lower,
                    TreeKind::Cypress,
                );
            }
            previous = Some((x, top + run));
        }
    }
}

/// The column spacing walls are traced at, metres.
const WALL_COLUMN: f64 = 2.0;
/// The longest straight piece a wall is built of, metres: short enough that
/// a piece's convex hull, which is its collider, hugs a curving wall.
const WALL_PIECE: f64 = 12.0;
/// A parapet's height and thickness, metres.
const PARAPET: [f64; 2] = [1.05, 0.35];

/// One retaining wall between two levels, traced column by column: where
/// the upper terrace's flat ends (`top`) and the lower one's begins
/// (`foot`) at each `x`.
struct Wall {
    upper: f64,
    lower: f64,
    /// `(x, top, foot)` in the frame.
    edge: Vec<[f64; 3]>,
}

/// Every retaining wall between `x0` and `x1`, traced like the terraces:
/// each pair of consecutive level runs down a column is a wall, joined to
/// the same pair in the next column where it continues.
#[allow(
    clippy::cast_possible_truncation,
    reason = "levels of a few hundred metres"
)]
fn walls_between(g: &Ground<'_>, x0: f64, x1: f64) -> Vec<Wall> {
    let ([_, pz], [_, phz], _) = g.processor;
    let mut open: BTreeMap<(i64, i64), Wall> = BTreeMap::new();
    let mut done = Vec::new();
    let mut x = x0;
    while x <= x1 {
        let mut seen = Vec::new();
        let runs = level_runs(g, x, pz - phz - 30.0, 300.0);
        for pair in runs.windows(2) {
            let ((_, top, upper), (foot, _, lower)) = (pair[0], pair[1]);
            // A wall faces downhill onto dry ground; the quay's edge is the
            // quay wall's.
            if upper < lower + 0.5 || lower < g.quay - 0.1 {
                continue;
            }
            // Consecutive flats may now be separated by a whole uncut
            // gully. It is a bank, not a retaining wall hundreds of metres
            // deep connecting the processor to the starport.
            let bank_run = foot - top;
            if bank_run > 40.0 || (upper - lower) / bank_run.max(0.5) < 0.42 {
                continue;
            }
            let key = ((upper * 2.0).round() as i64, (lower * 2.0).round() as i64);
            seen.push(key);
            let point = [x, top, foot.max(top + 0.5)];
            match open.get_mut(&key) {
                Some(w) if (w.edge[w.edge.len() - 1][1] - top).abs() < 4.0 => w.edge.push(point),
                _ => {
                    if let Some(w) = open.insert(
                        key,
                        Wall {
                            upper,
                            lower,
                            edge: vec![point],
                        },
                    ) {
                        done.push(w);
                    }
                }
            }
        }
        let ended: Vec<_> = open.keys().copied().filter(|k| !seen.contains(k)).collect();
        for key in ended {
            done.extend(open.remove(&key));
        }
        x += WALL_COLUMN;
    }
    done.extend(open.into_values());
    done.retain(|w| w.edge.len() >= 2);
    done
}

/// The terraces' retaining walls: each a solid from the lower terrace's
/// back edge to the upper one's front, standing on the lower level and
/// flush with the upper, which covers the terrain's own steep bank, with a
/// parapet along its top. They are cut where a stair comes down through
/// them and in the pipelines' gully.
fn walls(city: &mut City, g: &Ground<'_>) {
    // A stair cuts the wall it comes down through exactly as wide as the
    // stair: any wider and the terrace's bare bank shows beside it.
    let cuts: Vec<[f64; 4]> = ["stair", "lanes"]
        .iter()
        .filter_map(|d| city.districts.get(*d))
        .flatten()
        .map(|m| m.footprint)
        .collect();
    let cut_at = |x: f64, top: f64, foot: f64| {
        cuts.iter()
            .copied()
            .find(|&[a0, b0, a1, b1]| x > a0 && x < a1 && foot > b0 - 1.0 && top < b1 + 1.0)
    };
    // The edge between two traced columns, at `x`.
    let between = |a: [f64; 3], b: [f64; 3], x: f64| {
        let t = ((x - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0);
        [x, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    for wall in walls_between(g, -330.0, 330.0) {
        // Split the traced edge where it is cut or leaves the city's ground,
        // ending each stretch at the cut's own edge, then into pieces no
        // longer than a piece may be.
        let mut stretch: Vec<[f64; 3]> = Vec::new();
        let mut stretches = Vec::new();
        let mut previous: Option<([f64; 3], Option<[f64; 4]>)> = None;
        for &point in &wall.edge {
            let [x, top, foot] = point;
            let cut = cut_at(x, top, foot);
            let keep = cut.is_none() && (g.owns)(x, top) && (g.owns)(x, foot);
            if keep {
                // Coming out of a cut: start at its far edge.
                if let Some((last, Some([_, _, a1, _]))) = previous
                    && stretch.is_empty()
                    && a1 > last[0]
                    && a1 < x
                {
                    stretch.push(between(last, point, a1));
                }
                stretch.push(point);
            } else if !stretch.is_empty() {
                // Going into a cut: end at its near edge.
                if let (Some([a0, _, _, _]), Some((last, _))) = (cut, previous)
                    && a0 > last[0]
                    && a0 < x
                {
                    stretch.push(between(last, point, a0));
                }
                stretches.push(std::mem::take(&mut stretch));
            }
            previous = Some((point, cut));
        }
        stretches.push(stretch);
        for stretch in stretches.iter().filter(|s| s.len() >= 2) {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a few columns"
            )]
            let per_piece = ((WALL_PIECE / WALL_COLUMN) as usize).max(1);
            let mut i = 0;
            while i + 1 < stretch.len() {
                let j = (i + per_piece).min(stretch.len() - 1);
                wall_piece(city, &wall, stretch[i], stretch[j]);
                i = j;
            }
        }
    }
}

/// A wall this tall or taller steps back halfway up, metres.
const SETBACK_FROM: f64 = 6.0;
/// The spacing of the weep holes' drains along a wall's foot, metres.
const DRAIN_EVERY: f64 = 2.4;

/// One straight piece of a retaining wall between two traced columns.
///
/// Coursed stone battered back as it rises, on a concrete plinth, with a
/// stone parapet under a concrete coping. A tall wall steps back halfway
/// up onto a ledge planted behind a kerb, as terraced towns build them: one
/// sheer face of ten metres is a dam, two of five with green between are a
/// street. Drains stand out of the weep holes along the foot, where the
/// water behind a retaining wall has to leave it.
#[allow(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "one wall piece, part by part, and its trim's `_t` ends beside its own"
)]
fn wall_piece(city: &mut City, wall: &Wall, [xa, ta, fa]: [f64; 3], [xb, tb, fb]: [f64; 3]) {
    // Masonry courses are laid to five centimetres. Rounding outward
    // avoids sliver faces when a sampled level nearly meets a course.
    let base = (wall.lower * 20.0).floor() / 20.0 - 1.0;
    // Two centimetres proud of the terrace above: flush, its top and the
    // terrain's would be one plane and flicker.
    let tall = (wall.upper * 20.0).ceil() / 20.0 - base + PROUD;
    let depth = (fa - ta).min(fb - tb);
    // A face battered back by `batter` between heights `y0` and `y1`, its
    // foot at `fa`/`fb` less `back`.
    let block = |y0: f64, y1: f64, back: f64, batter: f64| {
        Geometry::hull([
            [xa, y0, ta],
            [xb, y0, tb],
            [xb, y0, fb - back],
            [xa, y0, fa - back],
            [xa, y1, ta],
            [xb, y1, tb],
            [xb, y1, fb - back - batter],
            [xa, y1, fa - back - batter],
        ])
    };
    let setback = tall >= SETBACK_FROM && depth >= 2.6;
    let (body, ledge_y, back, batter) = if setback {
        let ledge = (depth - 1.4).min(1.3);
        let y1 = ((tall - 1.0) * 0.5).round() + 1.0;
        let lower = block(0.0, y1, 0.0, (y1 * 0.04).min(0.2));
        let upper_batter = ((tall - y1) * 0.05).min(0.3);
        let upper = block(y1 - 0.01, tall, ledge, upper_batter);
        (lower.union(upper), Some((y1, ledge)), ledge, upper_batter)
    } else {
        let batter = (tall * 0.06).min(0.5).min(depth * 0.5);
        (block(0.0, tall, 0.0, batter), None, 0.0, batter)
    };
    // The trim runs a centimetre past both ends of the piece, into its
    // neighbour's, so no end of it lies on the plane where the stone ends.
    let run = (xb - xa).hypot(fb - fa);
    let e = 0.01 / run;
    let (xa_t, fa_t) = (xa - (xb - xa) * e, fa - (fb - fa) * e);
    let (xb_t, fb_t) = (xb + (xb - xa) * e, fb + (fb - fa) * e);
    let (fa2_t, fb2_t) = (fa_t - back - batter, fb_t - back - batter);
    // The parapet along the front of the top, where the upper terrace's
    // lane meets the drop, and its coping.
    let [height, thick] = [PARAPET[0], PARAPET[1]];
    let (fa2, fb2) = (fa - back - batter, fb - back - batter);
    let parapet = Geometry::extrude(
        [[xa, fa2 - thick], [xb, fb2 - thick], [xb, fb2], [xa, fa2]],
        height - 0.12,
    )
    .placed(Pose::at([0.0, tall, 0.0]));
    // Sunk a centimetre into the parapet, so their faces never share a
    // plane.
    let coping = Geometry::extrude(
        [
            [xa_t, fa2_t - thick - 0.08],
            [xb_t, fb2_t - thick - 0.08],
            [xb_t, fb2_t + 0.08],
            [xa_t, fa2_t + 0.08],
        ],
        0.13,
    )
    .placed(Pose::at([0.0, tall + height - 0.13, 0.0]));
    // A buttress at the piece's start, where it meets the last: a wedge
    // standing out from the foot and dying into the lower face.
    let reach = ledge_y.map_or(tall * 0.8, |(y1, _)| y1 * 0.85);
    let buttress = Geometry::hull([
        [xa - 0.45, 0.0, fa - 0.3],
        [xa + 0.45, 0.0, fa - 0.3],
        [xa - 0.45, 0.0, fa + 0.9],
        [xa + 0.45, 0.0, fa + 0.9],
        [xa - 0.45, reach, fa - 0.3],
        [xa + 0.45, reach, fa - 0.3],
        [xa - 0.45, reach, fa + 0.05],
        [xa + 0.45, reach, fa + 0.05],
    ]);
    // The plinth: a concrete course six centimetres proud of the stone,
    // from below the ground to knee height, which the splash and the
    // street's dirt reach instead of the masonry. Its buried back is off
    // the buttress's, which the lint would otherwise pair with it.
    let plinth = Geometry::hull([
        [xa_t, 0.0, fa_t - 0.25],
        [xb_t, 0.0, fb_t - 0.25],
        [xb_t, 0.0, fb_t + 0.06],
        [xa_t, 0.0, fa_t + 0.06],
        [xa_t, 1.55, fa_t - 0.25],
        [xb_t, 1.55, fb_t - 0.25],
        [xb_t, 1.55, fb_t + 0.06],
        [xa_t, 1.55, fa_t + 0.06],
    ]);
    let mut trim = coping.union(plinth);
    let mut details = Vec::new();
    // The ledge of a stepped wall: a soil bed behind a kerb, and shrubs of
    // the wilderness's own species in it.
    if let Some((y1, ledge)) = ledge_y {
        let front = (y1 * 0.04).min(0.2);
        let (ka, kb) = (fa_t - front, fb_t - front);
        let kerb = Geometry::hull([
            [xa_t, y1 - 0.02, ka - 0.22],
            [xb_t, y1 - 0.02, kb - 0.22],
            [xb_t, y1 - 0.02, kb + 0.03],
            [xa_t, y1 - 0.02, ka + 0.03],
            [xa_t, y1 + 0.4, ka - 0.22],
            [xb_t, y1 + 0.4, kb - 0.22],
            [xb_t, y1 + 0.4, kb + 0.03],
            [xa_t, y1 + 0.4, ka + 0.03],
        ]);
        trim = trim.union(kerb);
        // The bed stops a centimetre short of both ends, where the kerb and
        // the stone end.
        let (xa_b, xb_b) = (xa + (xb - xa) * e, xb - (xb - xa) * e);
        details.push(Detail {
            name: "bed",
            geometry: Geometry::hull([
                [xa_b, y1 - 0.02, fa_t - ledge + 0.02],
                [xb_b, y1 - 0.02, fb_t - ledge + 0.02],
                [xb_b, y1 - 0.02, kb - 0.2],
                [xa_b, y1 - 0.02, ka - 0.2],
                [xa_b, y1 + 0.3, fa_t - ledge + 0.02],
                [xb_b, y1 + 0.3, fb_t - ledge + 0.02],
                [xb_b, y1 + 0.3, kb - 0.2],
                [xa_b, y1 + 0.3, ka - 0.2],
            ]),
            slot: "bed",
        });
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few metres of ledge"
        )]
        let count = (run / 4.5) as usize;
        for i in 0..count {
            #[allow(clippy::cast_precision_loss, reason = "a few plants")]
            let t = (i as f64 + 0.5) / count as f64;
            let x = xa + (xb - xa) * t;
            let z = fa + (fb - fa) * t - front - (ledge - front) * 0.5;
            let mut dice =
                Dice((x.to_bits() ^ z.to_bits()).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            // Clumps with bare soil between: every plant near the camera
            // is drawn as its own entity.
            if dice.unit() < 0.4 {
                continue;
            }
            plant_at(city, &mut dice, [x, z], base + y1 + 0.28, TreeKind::Shrub);
        }
    }
    // The drains: short pipes out of the weep holes above the plinth,
    // turned square to the face.
    let yaw = (fb - fa).atan2(xb - xa);
    let out = DQuat::from_rotation_y(-yaw) * DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2);
    let mut drains: Option<Geometry> = None;
    let mut s = DRAIN_EVERY * 0.5 + 0.6;
    while s < run - 0.3 {
        let t = s / run;
        let pipe = Geometry::cylinder(0.07, 0.4, 8)
            .placed(Pose::at([xa + (xb - xa) * t, 1.9, fa + (fb - fa) * t - 0.2]).rotated(out));
        drains = Some(match drains {
            Some(d) => d.union(pipe),
            None => pipe,
        });
        s += DRAIN_EVERY;
    }
    if let Some(drains) = drains {
        details.push(Detail {
            name: "drains",
            geometry: drains,
            slot: "pipe",
        });
    }
    details.push(Detail {
        name: "trim",
        geometry: trim,
        slot: "wall",
    });
    city.push(
        "walls",
        Mass {
            geometry: body.union(parapet).union(buttress),
            pose: Pose::at([0.0, base, 0.0]),
            slot: "masonry",
            footprint: [xa.min(xb), ta.min(tb), xa.max(xb), fa.max(fb)],
            details,
        },
    );
    city.feet.push(([xa, fa], [xb, fb], wall.lower));
    // Now and then the parapet is overgrown: bushes of the wilderness's own
    // species rooted along its top, spilling over both sides.
    let pick = (xa * 7.31 + wall.upper * 3.7).sin().abs();
    if pick < 0.3 && tall > 2.5 {
        let count = if pick < 0.1 { 2 } else { 3 };
        for i in 0..count {
            let t = 0.2 + 0.6 * f64::from(i) / f64::from(count) + pick * 0.3;
            let (x, z) = (xa + (xb - xa) * t, fa2 + (fb2 - fa2) * t - thick * 0.5);
            let mut dice =
                Dice((x.to_bits() ^ z.to_bits()).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            plant_at(
                city,
                &mut dice,
                [x, z],
                base + tall + height - 0.05,
                TreeKind::Shrub,
            );
        }
    }
}

/// One terrace as the ground left it: its level, and its back edge (where
/// the wall above meets it) as a line along the contour, with the terrace's
/// depth at each point.
struct Terrace {
    level: f64,
    /// `(x, z, depth)` in the frame, along the contour.
    edge: Vec<[f64; 3]>,
}

impl Terrace {
    /// The terrace's length along its back edge.
    fn length(&self) -> f64 {
        self.edge
            .windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
            .sum()
    }

    /// The point `s` metres along the back edge and the depth there.
    fn at(&self, s: f64) -> ([f64; 2], f64) {
        let mut left = s.max(0.0);
        for p in self.edge.windows(2) {
            let step = (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]);
            if left <= step && step > 0.0 {
                let t = left / step;
                let lerp = |i: usize| p[0][i] + (p[1][i] - p[0][i]) * t;
                return ([lerp(0), lerp(1)], lerp(2));
            }
            left -= step;
        }
        let last = self.edge[self.edge.len() - 1];
        ([last[0], last[1]], last[2])
    }
}

/// Every terrace between `x0` and `x1` above the quay, found by scanning
/// the ground in columns and joining each level's runs where they meet
/// column to column.
#[allow(
    clippy::cast_possible_truncation,
    reason = "levels of a few hundred metres"
)]
fn terraces_between(g: &Ground<'_>, x0: f64, x1: f64) -> Vec<Terrace> {
    const COLUMN: f64 = 2.0;
    let ([_, pz], [_, phz], _) = g.processor;
    let mut open: BTreeMap<i64, Terrace> = BTreeMap::new();
    let mut done = Vec::new();
    let mut x = x0;
    while x <= x1 {
        let mut seen = Vec::new();
        for (from, to, level) in level_runs(g, x, pz - phz, 300.0) {
            if level < g.quay + 1.0 || to - from < 8.0 {
                continue;
            }
            let key = (level * 1000.0).round() as i64;
            seen.push(key);
            let point = [x, from, to - from];
            match open.get_mut(&key) {
                Some(t) if (t.edge[t.edge.len() - 1][1] - from).abs() < 4.0 => t.edge.push(point),
                _ => {
                    if let Some(t) = open.insert(
                        key,
                        Terrace {
                            level,
                            edge: vec![point],
                        },
                    ) {
                        done.push(t);
                    }
                }
            }
        }
        // A level missing from this column has ended.
        let ended: Vec<i64> = open.keys().copied().filter(|k| !seen.contains(k)).collect();
        for key in ended {
            done.extend(open.remove(&key));
        }
        x += COLUMN;
    }
    done.extend(open.into_values());
    done.retain(|t| t.edge.len() >= 3);
    done
}

/// Whether every corner of a frame footprint is the city's own ground.
fn owned(g: &Ground<'_>, [x0, z0, x1, z1]: [f64; 4]) -> bool {
    [(x0, z0), (x1, z0), (x0, z1), (x1, z1)]
        .into_iter()
        .all(|(x, z)| (g.owns)(x, z))
}

/// Whether a frame footprint crosses ground the lots keep off though no mass
/// stands on it yet: the platforms and the company's yards below the
/// processor.
fn reserved(g: &Ground<'_>, [x0, z0, x1, z1]: [f64; 4]) -> bool {
    let inside = |([cx, cz], [hx, hz], _): Level, pad: f64| {
        x1 > cx - hx - pad && x0 < cx + hx + pad && z1 > cz - hz - pad && z0 < cz + hz + pad
    };
    inside(g.starport, 6.0) || inside(g.exchange, 6.0) || inside(g.processor, 45.0)
}

/// A lot along a terrace: its centre, its turn, the box round it, and the
/// rectangle itself.
struct Lot {
    centre: [f64; 2],
    yaw: f64,
    footprint: [f64; 4],
    rect: Rect,
}

/// A rectangle turned in the frame: its centre, its two unit axes and its
/// half extents along them. Lots along a diagonal terrace are turned, and the
/// box round a turned lot is much bigger than the lot, so neighbours are
/// tested rectangle against rectangle.
#[derive(Clone, Copy, Debug)]
struct Rect {
    centre: [f64; 2],
    axes: [[f64; 2]; 2],
    half: [f64; 2],
}

impl Rect {
    /// An axis-aligned box as a rectangle.
    fn from_box([x0, z0, x1, z1]: [f64; 4]) -> Self {
        Self {
            centre: [f64::midpoint(x0, x1), f64::midpoint(z0, z1)],
            axes: [[1.0, 0.0], [0.0, 1.0]],
            half: [(x1 - x0) * 0.5, (z1 - z0) * 0.5],
        }
    }

    /// Whether the two, each grown by `margin`, overlap: the separating
    /// axis test on the four axes.
    fn overlaps(&self, other: &Self, margin: f64) -> bool {
        let d = [
            other.centre[0] - self.centre[0],
            other.centre[1] - self.centre[1],
        ];
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        let reach = |r: &Self, axis: [f64; 2]| {
            r.half[0] * dot(r.axes[0], axis).abs() + r.half[1] * dot(r.axes[1], axis).abs()
        };
        self.axes.iter().chain(other.axes.iter()).all(|&axis| {
            dot(d, axis).abs() < reach(self, axis) + reach(other, axis) + margin - 1e-6
        })
    }
}

/// The lot `width` wide and `depth` deep whose back edge runs along the
/// terrace from `s`, set `back` metres off the wall's foot; `None` where the
/// ground under its corners is not the terrace's level.
fn lot(
    g: &Ground<'_>,
    terrace: &Terrace,
    s: f64,
    width: f64,
    depth: f64,
    back: f64,
) -> Option<Lot> {
    let (a, _) = terrace.at(s);
    let (b, _) = terrace.at(s + width);
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let run = dx.hypot(dz);
    if run < width * 0.8 {
        return None;
    }
    let (dx, dz) = (dx / run, dz / run);
    // Downhill, away from the wall: the contour's normal towards +z.
    let (nx, nz) = (-dz, dx);
    let off = back + depth * 0.5;
    let centre = [
        f64::midpoint(a[0], b[0]) + nx * off,
        f64::midpoint(a[1], b[1]) + nz * off,
    ];
    let mut footprint = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (u, v) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
        let x = centre[0] + dx * u * width + nx * v * depth;
        let z = centre[1] + dz * u * width + nz * v * depth;
        if !(g.owns)(x, z) || ((g.height)(x, z) - terrace.level).abs() > 0.4 {
            return None;
        }
        footprint = [
            footprint[0].min(x),
            footprint[1].min(z),
            footprint[2].max(x),
            footprint[3].max(z),
        ];
    }
    Some(Lot {
        centre,
        yaw: (-dz).atan2(dx),
        footprint,
        rect: Rect {
            centre,
            axes: [[dx, dz], [nx, nz]],
            half: [width * 0.5, depth * 0.5],
        },
    })
}

/// Turn only where the entire footprint still sits on the same bench.
/// Sampling the interior also catches a gully running between four corners.
fn turned_lot(g: &Ground<'_>, mut lot: Lot, level: f64, turn: f64) -> Option<Lot> {
    lot.yaw += turn;
    let (sin, cos) = lot.yaw.sin_cos();
    lot.rect.axes = [[cos, -sin], [sin, cos]];
    lot.footprint = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for u in [-1.0, 0.0, 1.0] {
        for v in [-1.0, 0.0, 1.0] {
            let [x, z] = [0, 1].map(|i| {
                lot.centre[i]
                    + lot.rect.axes[0][i] * lot.rect.half[0] * u
                    + lot.rect.axes[1][i] * lot.rect.half[1] * v
            });
            if !(g.owns)(x, z) || ((g.height)(x, z) - level).abs() > 0.04 {
                return None;
            }
            lot.footprint[0] = lot.footprint[0].min(x);
            lot.footprint[1] = lot.footprint[1].min(z);
            lot.footprint[2] = lot.footprint[2].max(x);
            lot.footprint[3] = lot.footprint[3].max(z);
        }
    }
    Some(lot)
}

/// A few widened benches become water courts. A full-depth cross-lane is
/// claimed at the same time, before any of the three infill passes runs.
fn cross_courts(city: &mut City, g: &Ground<'_>) {
    let mut dice = Dice(0x6372_6f73_735f_6c6e);
    for terrace in terraces_between(g, -300.0, 300.0) {
        if terrace.length() < 45.0 || dice.unit() > 0.65 {
            continue;
        }
        let s = terrace.length() * dice.range(0.25, 0.65);
        let (_, depth) = terrace.at(s + 6.0);
        if depth < 19.0 {
            continue;
        }
        let Some(court) = lot(g, &terrace, s, 12.0, 11.0, (depth - 11.0) * 0.5)
            .and_then(|l| turned_lot(g, l, terrace.level, 0.0))
        else {
            continue;
        };
        let Some(lane) = lot(g, &terrace, s + 3.5, 4.5, depth - 3.0, 1.5) else {
            continue;
        };
        if reserved(g, court.footprint) || !city.fits(&court, 1.0) || !city.fits(&lane, 0.2) {
            continue;
        }
        city.open.extend([court.footprint, lane.footprint]);
        city.courts.push((court, terrace.level));
    }
}

/// Pelagic Heights, east of the pipelines: the company's housing slabs on
/// every other terrace, staggered so each looks over the one below, with the
/// Terraces' houses filling in between them afterwards.
fn pelagic(city: &mut City, g: &Ground<'_>, hq: Option<&Slab>) -> anyhow::Result<Vec<Slab>> {
    // The headquarters looks down over the city: no slab stands in its
    // outlook, the strip downhill of it as wide as it and a little more.
    let outlook = hq.map(|hq| {
        let [x, z] = hq.centre;
        [x - 40.0, z + 10.0, x + 40.0, z + 90.0]
    });
    let in_outlook = |[x0, z0, x1, z1]: [f64; 4]| {
        outlook.is_some_and(|[a0, b0, a1, b1]| x1 > a0 && x0 < a1 && z1 > b0 && z0 < b1)
    };
    let mut dice = Dice(0x2545_f491_4f6c_dd1d);
    let mut slabs = Vec::new();
    let mut terraces = terraces_between(g, PIPE_X + 12.0, 300.0);
    terraces.sort_by(|a, b| a.level.total_cmp(&b.level));
    for (i, terrace) in terraces.iter().enumerate().filter(|(i, _)| i % 2 == 0) {
        let length = terrace.length();
        // Narrow enough to stand between two lines of lane stairs.
        let width = dice.range(34.0, 48.0);
        let stagger = if i % 4 == 0 { 0.15 } else { 0.55 };
        let room = length - width;
        if room < 0.0 {
            continue;
        }
        // The staggered spot first, then outwards from it a few metres at a
        // time: a lane's stair or the transit line crossing the terrace
        // moves a slab along it rather than off it.
        let preferred = room * stagger;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few dozen steps"
        )]
        let steps = (room / 6.0) as i32 + 1;
        let found = (0..=2 * steps)
            .map(|k| preferred + f64::from(if k % 2 == 0 { k / 2 } else { -(k + 1) / 2 }) * 6.0)
            .filter(|s| (0.0..=room).contains(s))
            .find_map(|s| {
                let (_, depth_here) = terrace.at(s + width * 0.5);
                let depth = (depth_here - 6.0).min(14.0);
                if depth < 10.0 {
                    return None;
                }
                lot(g, terrace, s, width, depth, 0.0)
                    .filter(|l| {
                        !reserved(g, l.footprint)
                            && !in_outlook(l.footprint)
                            && city.clear(l.footprint, 1.0)
                    })
                    .map(|l| (l, depth))
            });
        if let Some((l, depth)) = found {
            let storeys = dice.range(5.0, 10.0).round();
            let [x, z] = l.centre;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "5 to 10"
            )]
            let count = storeys as u32;
            let name = format!("slab-{}", slabs.len());
            city.landmark(
                PlannedBuilding {
                    name,
                    building: types::slab_block(width.round(), depth.floor(), count)?,
                    position: [x, terrace.level, z],
                    yaw: l.yaw,
                    footprint: l.footprint,
                },
                &[l.footprint],
            );
            slabs.push(Slab {
                centre: [x, z],
                base: terrace.level,
                top: terrace.level + storeys * 4.2,
            });
        }
    }
    Ok(slabs)
}

/// A housing slab as the headquarters' bridges see it.
struct Slab {
    centre: [f64; 2],
    base: f64,
    top: f64,
}

/// Pelagic's headquarters, square on its own terrace, its entrance
/// downhill.
///
/// It needs a lot sixty metres by thirty, which no terrace of a hillside
/// stepped every few metres has, so the settlement gives it a platform the
/// way it gives the Exchange one, and the building stands in its middle.
fn headquarters(city: &mut City, g: &Ground<'_>) -> anyhow::Result<Option<Slab>> {
    const STOREYS: u32 = 7;
    let ([x, z], [hx, hz], level) = g.headquarters;
    // The block and the entrance's fins, which stand out 3 m in front.
    let footprint = [x - 30.0, z - 13.0, x + 30.0, z + 15.0];
    if hx < 30.0 || hz < 15.0 || !city.clear(footprint, 1.0) {
        return Ok(None);
    }
    city.landmark(
        PlannedBuilding {
            name: "headquarters".to_owned(),
            building: landmarks::headquarters(STOREYS)?,
            position: [x, level, z],
            yaw: 0.0,
            footprint,
        },
        &[footprint],
    );
    Ok(Some(Slab {
        centre: [x, z],
        base: level,
        top: level + 7.0 + f64::from(STOREYS) * 4.2,
    }))
}

/// An enclosed bridge from the headquarters to the nearest slab it can
/// reach, at a floor both have.
fn bridge(city: &mut City, hq: &Slab, slabs: &[Slab]) {
    let Some(slab) = slabs
        .iter()
        .filter(|s| (s.centre[0] - hq.centre[0]).hypot(s.centre[1] - hq.centre[1]) < 90.0)
        .min_by(|a, b| {
            let d = |s: &Slab| (s.centre[0] - hq.centre[0]).hypot(s.centre[1] - hq.centre[1]);
            d(a).total_cmp(&d(b))
        })
    else {
        return;
    };
    let y = (hq.base + 15.4).max(slab.base + 4.2);
    if y + 4.0 > hq.top.min(slab.top) - 1.0 {
        return;
    }
    let (a, b) = (hq.centre, slab.centre);
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    city.block(
        "pelagic",
        [f64::midpoint(a[0], b[0]), y, f64::midpoint(a[1], b[1])],
        [dx.hypot(dz), 4.0, 4.5],
        (-dz).atan2(dx),
        "pelagic",
    );
}

/// Bank dwellings occupy a short piece of retaining edge, never a gully.
/// Each entrance is sampled across its full width on its own level.
fn split_houses(city: &mut City, g: &Ground<'_>) {
    let mut dice = Dice(0x6261_6e6b_686f_7573);
    for ix in -13..=12 {
        let x = f64::from(ix) * 16.0 + 3.0;
        let runs = level_runs(g, x, -290.0, 190.0);
        for pair in runs.windows(2) {
            let (upper, lower) = (pair[0], pair[1]);
            let rise = upper.2 - lower.2;
            if !(3.8..10.0).contains(&rise) || lower.0 - upper.1 > 14.0 {
                continue;
            }
            let front = (lower.0 + 2.0).ceil();
            let back = (upper.1 - 8.0).floor();
            let depth = front - back;
            if !(11.0..28.0).contains(&depth) || lower.1 < front + 2.5 {
                continue;
            }
            let upper_z = back + 3.5;
            let entrances_level = [-3.2, 0.0, 3.2].into_iter().all(|dx| {
                [back - 1.0, back + 0.5, back + 6.5]
                    .into_iter()
                    .all(|z| ((g.height)(x + dx, z) - upper.2).abs() < 0.01)
                    && [front, front + 2.0]
                        .into_iter()
                        .all(|z| ((g.height)(x + dx, z) - lower.2).abs() < 0.01)
            });
            let footprint = [x - 3.0, back, x + 3.0, front + 1.5];
            if !entrances_level
                || !owned(g, footprint)
                || reserved(g, footprint)
                || !city.clear(footprint, 1.0)
            {
                continue;
            }
            let centre = [x, f64::midpoint(back, front)];
            let l = Lot {
                centre,
                yaw: 0.0,
                footprint,
                rect: Rect::from_box(footprint),
            };
            let mut house = owner(&mut dice, (6.0, None), 6.0, false);
            house.lower = 1;
            house.upper = None;
            house.balcony = false;
            place_house(city, &mut dice, house, lower.2, &l);
            let placed = city
                .kits
                .get_mut("terrace-houses")
                .expect("place_house created the district")
                .last_mut()
                .expect("place_house appended a dwelling");
            placed.kit = Kit::SplitHouse {
                house,
                rise,
                depth,
                upper_z: upper_z - centre[1],
            };
            // The dwelling's buried floor does the retaining here. Remove
            // only wall pieces it actually covers, keeping the ends visible.
            if let Some(walls) = city.districts.get_mut("walls") {
                walls.retain(|wall| !l.rect.overlaps(&Rect::from_box(wall.footprint), 0.0));
            }
        }
    }
}

/// The Terraces: on every terrace, houses in rows against the wall behind,
/// a lane along the front. A row runs until a stair, a garden, a cistern or
/// the ground breaks it; within it the owners built to different widths,
/// depths and heights, some set back, a few wide workshops.
#[allow(clippy::too_many_lines, reason = "one terrace's rows, gap by gap")]
fn terraces(city: &mut City, g: &Ground<'_>) {
    let mut dice = Dice(0x9e37_79b9_7f4a_7c15);
    for terrace in terraces_between(g, -320.0, 320.0) {
        let length = terrace.length();
        let mut s = dice.range(0.0, 1.5);
        let mut setback = dice.range(0.8, 1.4);
        let (mut in_cluster, mut cluster) = (0_u32, 0_u32);
        let mut turn = dice.range(-0.025, 0.025);
        while s < length - 5.0 {
            let roll = if in_cluster > 0 { 0.5 } else { dice.unit() };
            if roll < 0.10 && (terrace.level > 105.0 || terrace.at(s).0[0].abs() > 275.0) {
                // A garden: a gap with a tree or two in it, and the next row
                // starts elsewhere.
                let gap = dice.range(6.0, 14.0);
                if let Some(spot) = lot(g, &terrace, s + gap * 0.5 - 1.0, 2.0, 2.0, 1.5) {
                    let kind = if dice.unit() < 0.6 {
                        TreeKind::Shade
                    } else {
                        TreeKind::Cypress
                    };
                    plant(city, g, &mut dice, spot.centre, terrace.level, kind);
                }
                if let Some(spot) = lot(g, &terrace, s + gap - 2.5, 1.5, 1.5, 0.5) {
                    plant(
                        city,
                        g,
                        &mut dice,
                        spot.centre,
                        terrace.level,
                        TreeKind::Shrub,
                    );
                }
                s += gap;
                setback = dice.range(-0.5, 2.5);
                continue;
            }
            if roll < 0.008 {
                if let Some(tank) = lot(g, &terrace, s + 0.5, 4.5, 4.5, 0.5)
                    && city.clear(tank.footprint, 0.5)
                    && !reserved(g, tank.footprint)
                {
                    let [x, z] = tank.centre;
                    let r = dice.range(1.6, 2.4);
                    city.column(
                        "terraces",
                        [x, terrace.level, z],
                        r,
                        dice.range(4.0, 7.0),
                        "wall",
                    );
                }
                s += 5.5;
                continue;
            }
            let workshop = roll > 0.95;
            // The catalogue's widths, and a shed's strip beside some.
            let frontage = frontage(&mut dice, workshop);
            let width = frontage.0 + frontage.1.map_or(0.0, |_| types::house::SHED);
            // Within a cluster each house steps a little forward or back
            // of its neighbour.
            let stagger = if in_cluster > 0 {
                dice.range(-0.15, 0.15)
            } else {
                0.0
            };
            let back = (setback + stagger).clamp(0.7, 1.8);
            let (_, depth_here) = terrace.at(s + width * 0.5);
            // A lane of at least 4 m along the front, and the deepest module
            // the owner wanted that leaves it.
            let Some(depth) = fit_depth(
                (depth_here - 4.5 - back).min(dice.range(8.0, 14.0)),
                workshop,
            ) else {
                s += width;
                in_cluster = 0;
                continue;
            };
            // Closely aligned neighbours share a short side gap; the
            // front lane carries balconies, awnings and people.
            let margin = 0.35;
            match lot(g, &terrace, s, width, depth, back)
                .and_then(|l| turned_lot(g, l, terrace.level, turn))
            {
                Some(l) if !reserved(g, l.footprint) && city.fits(&l, margin) => {
                    if in_cluster == 0 {
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "5 to 8"
                        )]
                        let size = dice.range(5.0, 8.99) as u32;
                        cluster = size;
                    }
                    let house = owner(&mut dice, frontage, depth, workshop);
                    let [x, z] = l.centre;
                    let old_quarter = (-145.0..125.0).contains(&x) && (-100.0..90.0).contains(&z);
                    if old_quarter && dice.unit() < 0.32 {
                        let module = types::LandingModule {
                            width: house.width,
                            depth: house.depth,
                            storeys: 1,
                            shop: true,
                            awning: true,
                            dome: dice.unit() < 0.3,
                        };
                        city.lots.push(l.rect);
                        place_kit(
                            city,
                            "landing-infill",
                            Kit::Module(module),
                            l.centre,
                            terrace.level,
                            l.yaw,
                            house.width.max(house.depth) * 0.5,
                            vec![
                                ("shell", pick(&mut dice, &[3, 1], &types::SHELLS)),
                                ("pane", types::PANES[1]),
                                ("recess", types::SHOP_LIT),
                            ],
                        );
                    } else {
                        place_house(city, &mut dice, house, terrace.level, &l);
                        dress_front(city, g, &mut dice, &l, &house, terrace.level);
                    }
                    s += width;
                    in_cluster += 1;
                    if in_cluster >= cluster {
                        // The cluster ends: an alley, and the next one sits
                        // at its own depth.
                        s += dice.range(1.0, 2.0);
                        turn = dice.range(-0.025, 0.025);
                        in_cluster = 0;
                        setback = dice.range(0.8, 1.4);
                    }
                }
                _ => {
                    s += 0.5;
                    in_cluster = 0;
                }
            }
        }
        front_row(city, g, &mut dice, &terrace);
        middle_row(city, g, &mut dice, &terrace);
        edge_furniture(city, g, &mut dice, &terrace);
    }
}

/// A kit (a house, a module, a stall, a tree) as the plan placed it, with
/// the colours its owner chose.
pub(crate) struct PlacedKit {
    pub(crate) kit: Kit,
    pub(crate) at: [f64; 3],
    pub(crate) yaw: f64,
    pub(crate) footprint: [f64; 4],
    pub(crate) bindings: Vec<(&'static str, &'static str)>,
}

/// Records a house on its lot with its owner's colours.
fn place_house(city: &mut City, dice: &mut Dice, house: TerraceHouse, level: f64, l: &Lot) {
    city.lots.push(l.rect);
    let paint = pick(dice, &[25, 18, 14, 12, 11, 11, 9], &types::PAINTS);
    // The joinery in a colour of its own: never green on green, teal on
    // teal or white on white.
    let mut joinery = pick(dice, &[4, 3, 3, 3, 2, 2], &types::JOINERY);
    let colour = |key: &str| key.rsplit('-').next().unwrap_or(key).to_owned();
    if colour(joinery) == colour(paint) {
        let at = types::JOINERY
            .iter()
            .position(|j| *j == joinery)
            .unwrap_or(0);
        joinery = types::JOINERY[(at + 1) % types::JOINERY.len()];
    }
    // The trim against the walls: never cream on cream or white on white,
    // and bare concrete on bare concrete reads as one mass, which is right.
    let mut trim = pick(dice, &[4, 4, 2], &types::TRIMS);
    if trim == paint && trim != types::TRIMS[0] {
        trim = types::TRIMS[0];
    }
    // Every house's glass is lit or dark at night.
    let pane = pick(dice, &[45, 45, 10], &types::PANES);
    let mut bindings = vec![
        ("paint", paint),
        ("trim", trim),
        ("joinery", joinery),
        ("pane", pane),
    ];
    // The awning's cloth, and the washing's.
    bindings.push(("fabric", pick(dice, &[1, 1, 1], &types::FABRICS)));
    city.kits
        .entry("terrace-houses")
        .or_default()
        .push(PlacedKit {
            kit: Kit::House(house),
            at: [l.centre[0], level, l.centre[1]],
            yaw: l.yaw,
            footprint: l.footprint,
            bindings,
        });
}

/// The front row: where a terrace is deep enough for two rows and a lane,
/// houses along its front edge behind the parapet, a face to the view and
/// one to the lane.
fn front_row(city: &mut City, g: &Ground<'_>, dice: &mut Dice, terrace: &Terrace) {
    let length = terrace.length();
    // The lane is kept from the back row; this row's houses may touch.
    let behind = city.lots.len();
    let mut s = dice.range(0.0, 1.5);
    while s < length - 5.0 {
        if dice.unit() < 0.18 && (terrace.level > 105.0 || terrace.at(s).0[0].abs() > 275.0) {
            // A gap on the edge: a cypress against the view, or a bush.
            let gap = dice.range(6.0, 16.0);
            let (_, depth_here) = terrace.at(s + gap * 0.5);
            if let Some(spot) = lot(g, terrace, s + gap * 0.5 - 1.0, 2.0, 2.0, depth_here - 3.5) {
                let kind = if dice.unit() < 0.5 {
                    TreeKind::Cypress
                } else {
                    TreeKind::Shrub
                };
                plant(city, g, dice, spot.centre, terrace.level, kind);
            }
            s += gap;
            continue;
        }
        let frontage = frontage(dice, false);
        let width = frontage.0 + frontage.1.map_or(0.0, |_| types::house::SHED);
        let (_, depth_here) = terrace.at(s + width * 0.5);
        let depth = fit_depth(dice.range(6.0, 9.5), false).unwrap_or(6.0);
        // The back row's houses, a lane of four and a half metres, this
        // row, and a metre to the parapet.
        if depth_here < depth + 1.0 + 4.5 + 7.0 {
            s += width;
            continue;
        }
        match lot(g, terrace, s, width, depth, depth_here - depth - 1.0)
            .and_then(|l| turned_lot(g, l, terrace.level, std::f64::consts::PI))
        {
            Some(l)
                if !reserved(g, l.footprint)
                    && city.fits(&l, 0.35)
                    && city.lots[..behind]
                        .iter()
                        .all(|o| !o.overlaps(&l.rect, 2.2)) =>
            {
                let house = owner(dice, frontage, depth, false);
                place_house(city, dice, house, terrace.level, &l);
                // Mostly attached; now and then an alley down to the edge.
                s += width
                    + if dice.unit() < 0.2 {
                        dice.range(1.8, 3.0)
                    } else {
                        0.0
                    };
            }
            _ => s += 0.5,
        }
    }
}

/// Street furniture along a terrace's front edge: a lamp every twenty
/// metres or so where the parapet is clear, its arm over the lane, and now
/// and then a bench beside it looking out.
fn edge_furniture(city: &mut City, g: &Ground<'_>, dice: &mut Dice, terrace: &Terrace) {
    let length = terrace.length();
    let mut s = dice.range(4.0, 14.0);
    while s < length - 3.0 {
        let (_, depth_here) = terrace.at(s);
        if let Some(spot) = lot(g, terrace, s, 0.6, 0.6, depth_here - 1.4)
            && city.clear(spot.footprint, 0.3)
            && !reserved(g, spot.footprint)
        {
            let lamp = Kit::Furniture(types::Furniture::Lamp);
            let yaw = spot.yaw + std::f64::consts::PI;
            lamp_light(city, spot.centre, terrace.level, yaw);
            place_kit(
                city,
                "street",
                lamp,
                spot.centre,
                terrace.level,
                yaw,
                0.5,
                Vec::new(),
            );
            city.open.push(spot.footprint);
            if dice.unit() < 0.35
                && let Some(seat) = lot(g, terrace, s + 1.2, 1.9, 0.6, depth_here - 1.6)
                && city.clear(seat.footprint, 0.2)
            {
                let bench = Kit::Furniture(types::Furniture::Bench);
                place_kit(
                    city,
                    "street",
                    bench,
                    seat.centre,
                    terrace.level,
                    seat.yaw,
                    1.0,
                    Vec::new(),
                );
                city.open.push(seat.footprint);
            }
        }
        s += dice.range(16.0, 24.0);
    }
}

/// A planter or crates by a house's front, on the lane.
fn dress_front(
    city: &mut City,
    g: &Ground<'_>,
    dice: &mut Dice,
    l: &Lot,
    house: &TerraceHouse,
    level: f64,
) {
    let chance = if house.workshop { 0.7 } else { 0.3 };
    if dice.unit() > chance {
        return;
    }
    let [a, n] = l.rect.axes;
    let side = if dice.unit() < 0.5 { -1.0 } else { 1.0 } * (house.lot_width() * 0.5 - 0.9);
    let out = house.depth * 0.5 + 0.7;
    let at = [
        l.centre[0] + a[0] * side + n[0] * out,
        l.centre[1] + a[1] * side + n[1] * out,
    ];
    let spot = [at[0] - 0.7, at[1] - 0.7, at[0] + 0.7, at[1] + 0.7];
    let level_here = ((g.height)(at[0], at[1]) - level).abs() < 0.3;
    if !level_here || !owned(g, spot) || !city.clear(spot, 0.1) {
        return;
    }
    let kind = if house.workshop {
        types::Furniture::Crates
    } else {
        types::Furniture::Planter
    };
    place_kit(
        city,
        "street",
        Kit::Furniture(kind),
        at,
        level,
        l.yaw,
        0.7,
        Vec::new(),
    );
    city.open.push(spot);
}

/// The middle row: on a terrace deep enough that a lane between two rows
/// would still be a field, a row down its middle, faced both ways, a lane
/// kept on either side.
fn middle_row(city: &mut City, g: &Ground<'_>, dice: &mut Dice, terrace: &Terrace) {
    let behind = city.lots.len();
    let mut s = dice.range(0.0, 2.0);
    while s < terrace.length() - 6.0 {
        let width = if dice.unit() < 0.7 { 6.0 } else { 8.0 };
        let (_, depth_here) = terrace.at(s + width * 0.5);
        let paired = depth_here >= 40.0;
        if depth_here < 28.0 {
            s += 1.0;
            continue;
        }
        let occupied = if paired { 13.0 } else { 6.0 };
        let back = (depth_here - occupied) * 0.5;
        let mut placed = false;
        for row in 0..=u32::from(paired) {
            // Joined backs, with doors onto lanes on either side of the pair.
            let turn = if row == 0 { std::f64::consts::PI } else { 0.0 };
            if let Some(l) = lot(g, terrace, s, width, 6.0, back + f64::from(row) * 7.0)
                .and_then(|l| turned_lot(g, l, terrace.level, turn))
                && !reserved(g, l.footprint)
                && city.fits(&l, 0.35)
                && city.lots[..behind]
                    .iter()
                    .all(|o| !o.overlaps(&l.rect, 2.4))
            {
                let house = owner(dice, (width, None), 6.0, false);
                place_house(city, dice, house, terrace.level, &l);
                placed = true;
            }
        }
        s += if placed { width + 0.5 } else { 0.5 };
    }
}

/// How wide a house's module is and on which side a shed stands, which
/// together are its lot's width: the catalogue's widths, most often the
/// middle one, and a shed beside a wider module now and then.
fn frontage(dice: &mut Dice, workshop: bool) -> (f64, Option<bool>) {
    use types::house::{WIDTHS, WORKSHOP};
    let width = if workshop {
        WORKSHOP
    } else {
        match dice.unit() {
            r if r < 0.60 => WIDTHS[0],
            r if r < 0.90 => WIDTHS[1],
            _ => WIDTHS[2],
        }
    };
    let shed = (width >= 8.0 && dice.unit() < 0.12).then(|| dice.unit() < 0.5);
    (width, shed)
}

/// The deepest module of the catalogue no deeper than `room`, if any fits.
fn fit_depth(room: f64, workshop: bool) -> Option<f64> {
    let depths = &types::house::DEPTHS[usize::from(workshop)..];
    depths.iter().rev().copied().find(|d| *d <= room + 1e-9)
}

/// What one owner built on a lot for a module `width` by `depth`: how many
/// storeys, what they added on top, on the walls and to the side, what
/// stands on the roof.
fn owner(
    dice: &mut Dice,
    (width, shed): (f64, Option<bool>),
    depth: f64,
    workshop: bool,
) -> TerraceHouse {
    use types::house::{UPPER_DEPTHS, UPPER_WIDTHS, roof};
    let lower = if !workshop && dice.unit() < 0.55 {
        2
    } else {
        1
    };
    let style = if dice.unit() < 0.55 {
        types::Style::Shuttered
    } else {
        types::Style::Casement
    };
    // A module on the roof stands inside the parapet, against its back, and
    // leaves a terrace of at least a metre and a half at the front.
    let upper = (!workshop && dice.unit() < 0.6)
        .then(|| {
            let widths: Vec<f64> = UPPER_WIDTHS
                .into_iter()
                .filter(|w| *w <= width - 1.9 && *w >= width - 4.1)
                .collect();
            let depths: Vec<f64> = UPPER_DEPTHS
                .into_iter()
                .filter(|d| *d <= depth - 1.75)
                .collect();
            if widths.is_empty() || depths.is_empty() {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "an index into a few"
            )]
            let pick = |dice: &mut Dice, of: &[f64]| of[(dice.unit() * of.len() as f64) as usize];
            let w = pick(dice, &widths);
            let d = pick(dice, &depths);
            // Shifted to one side or the other, or centred, in half metres.
            let room = ((width - 0.5 - w) * 0.5 - 0.1).max(0.0);
            let shift = ((dice.range(-1.0, 1.0) * room) * 2.0).round() * 0.5;
            Some(types::Upper {
                width: w,
                depth: d,
                shift: shift.clamp(-room, room),
            })
        })
        .flatten();
    let awning = dice.unit() < if workshop { 0.6 } else { 0.4 };
    let top = match dice.unit() {
        r if r < 0.66 || workshop => types::Top::Flat,
        r if r < 0.84 => types::Top::Vault,
        _ => types::Top::Lean,
    };
    let balcony = !workshop && lower >= 2 && dice.unit() < 0.4;
    let mut bits = 0;
    for (bit, chance) in [
        (roof::GARDEN, 0.35),
        (roof::TANK, 0.4),
        (roof::HUTCH, 0.3),
        (roof::SOLAR, 0.25),
        (roof::DISH, 0.25),
        (roof::AERIAL, 0.3),
        // Washing, more often where people live than where they work.
        (roof::WASHING, if workshop { 0.05 } else { 0.35 }),
        (roof::AC, 0.35),
    ] {
        if dice.unit() < chance {
            bits |= bit;
        }
    }
    TerraceHouse {
        width,
        depth,
        lower,
        style,
        workshop,
        upper,
        shed,
        awning,
        balcony,
        top,
        roof: bits,
    }
}

/// One of `choices`, by `weights`.
fn pick(dice: &mut Dice, weights: &[u32], choices: &[&'static str]) -> &'static str {
    let total: u32 = weights.iter().sum();
    let mut roll = dice.unit() * f64::from(total);
    for (weight, choice) in weights.iter().zip(choices) {
        roll -= f64::from(*weight);
        if roll < 0.0 {
            return choice;
        }
    }
    choices[choices.len() - 1]
}

/// Kits as one building: a part per distinct shape, an instance per kit
/// with its own colours.
pub(crate) fn kits_building(name: &str, kits: &[PlacedKit]) -> anyhow::Result<PlannedBuilding> {
    let mut builder = Building::builder(format!("serein:{name}"));
    // Each shape's slots, so an instance binds only what its part has.
    let mut shapes: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    let mut slots = std::collections::BTreeSet::new();
    let mut footprint = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (i, placed) in kits.iter().enumerate() {
        let [x, y, z] = placed.at;
        let at = Pose::at([x, y, z]).rotated(DQuat::from_rotation_y(placed.yaw));
        let pieces = placed.kit.pieces();
        let single = pieces.len() == 1;
        for (j, piece) in pieces.into_iter().enumerate() {
            let key = piece.key.clone();
            if !shapes.contains_key(&key) {
                let part = piece.part()?;
                let own: std::collections::BTreeSet<String> = part
                    .elements
                    .iter()
                    .flat_map(|e| {
                        std::iter::once(e.material_slot.clone()).chain(e.cut_material_slot.clone())
                    })
                    .collect();
                slots.extend(own.iter().cloned());
                shapes.insert(key.clone(), own);
                builder = builder.part(part);
            }
            let own = &shapes[&key];
            // A kit of one part keeps the id it always had.
            let id = if single {
                format!("{name}-{i}")
            } else {
                format!("{name}-{i}-{j}")
            };
            let mut instance = Instance::new(id, key).placed(at.compose(piece.pose));
            for &(slot, material) in &placed.bindings {
                if own.contains(slot) {
                    instance = instance.material(slot, material);
                }
            }
            builder = builder.instance(instance);
        }
        let [a0, b0, a1, b1] = placed.footprint;
        footprint = [
            footprint[0].min(a0),
            footprint[1].min(b0),
            footprint[2].max(a1),
            footprint[3].max(b1),
        ];
    }
    for (slot, key) in types::KIT_MATERIALS {
        if slots.contains(slot) {
            builder = builder.material(slot, key);
        }
    }
    Ok(PlannedBuilding {
        name: name.to_owned(),
        building: builder.build()?,
        position: [0.0; 3],
        yaw: 0.0,
        footprint,
    })
}

/// A kit placed at `(x, z)` on `level`, turned by `yaw`, claiming a square
/// of `reach` round it.
#[allow(clippy::too_many_arguments, reason = "a placement's few numbers")]
fn place_kit(
    city: &mut City,
    building: &'static str,
    kit: Kit,
    [x, z]: [f64; 2],
    level: f64,
    yaw: f64,
    reach: f64,
    bindings: Vec<(&'static str, &'static str)>,
) {
    let footprint = [x - reach, z - reach, x + reach, z + reach];
    city.kits.entry(building).or_default().push(PlacedKit {
        kit,
        at: [x, level, z],
        yaw,
        footprint,
        bindings,
    });
}

/// A lamp's warm light: [`types::Furniture::Lamp`]'s lantern hangs its arm's
/// length along its turned `+z`, at 3.2 m.
fn lamp_light(city: &mut City, [x, z]: [f64; 2], level: f64, yaw: f64) {
    let (s, c) = yaw.sin_cos();
    city.lights.push(PlannedLight {
        position: [x + 0.85 * s, level + 3.2, z + 0.85 * c],
        color: [1.0, 0.62, 0.3],
        lumens: 4000.0,
        range: 16.0,
    });
}

/// A planting of `kind` from the wilderness's own species, its size and
/// variant drawn by the plan: a broad tree (oak, now and then a birch), a
/// narrow dark one (a fir, the city's cypress), or a bush.
fn species_for(dice: &mut Dice, kind: TreeKind) -> (&'static str, f32, f64) {
    #[allow(clippy::cast_possible_truncation, reason = "a scale near one")]
    let scale = |dice: &mut Dice, lo: f64, hi: f64| dice.range(lo, hi) as f32;
    match kind {
        TreeKind::Shade if dice.unit() < 0.8 => ("strata:oak", scale(dice, 0.75, 0.95), 3.0),
        TreeKind::Shade => ("strata:birch", scale(dice, 0.8, 0.95), 2.5),
        TreeKind::Cypress => ("strata:fir", scale(dice, 0.75, 0.9), 1.6),
        TreeKind::Shrub if dice.unit() < 0.5 => {
            ("strata:woodland_shrub", scale(dice, 0.9, 1.2), 1.0)
        }
        TreeKind::Shrub => ("strata:scrub", scale(dice, 0.8, 1.2), 1.0),
    }
}

/// Plants a `kind` at `(x, z)` on `level`, unchecked.
fn plant_at(city: &mut City, dice: &mut Dice, [x, z]: [f64; 2], level: f64, kind: TreeKind) {
    let (species, scale, _) = species_for(dice, kind);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a turn and a variant index"
    )]
    city.plants.push(PlannedPlant {
        species,
        position: [x, level, z],
        yaw: dice.unit() as f32,
        scale,
        variant: (dice.unit() * 16.0) as u8,
    });
}

/// Low planting where the retaining walls meet the terraces below them:
/// ferns, wildflowers and scrub from the wilderness's own library, now and
/// then, wherever the foot is level ground nothing else took.
///
/// After the houses, so a verge never takes a lot; the walls' feet are the
/// strip nobody builds on, and green there ties the stone to the ground.
fn verges(city: &mut City, g: &Ground<'_>) {
    const SPECIES: [(&str, u32); 4] = [
        ("strata:woodland_fern", 9),
        ("strata:wildflower_amber", 3),
        ("strata:wildflower_violet", 3),
        ("strata:scrub", 5),
    ];
    for ([xa, za], [xb, zb], level) in std::mem::take(&mut city.feet) {
        let run = (xb - xa).hypot(zb - za);
        if run < 1.0 {
            continue;
        }
        // Out from the face, square to it.
        let (nx, nz) = (-(zb - za) / run, (xb - xa) / run);
        let mut dice = Dice(
            (xa.to_bits() ^ za.to_bits().rotate_left(17)).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
        );
        let mut s = dice.range(0.5, 4.0);
        while s < run - 0.5 {
            let t = s / run;
            let out = dice.range(0.9, 1.4);
            let (x, z) = (xa + (xb - xa) * t + nx * out, za + (zb - za) * t + nz * out);
            // A clump of one or two, now and then.
            let clump = if dice.unit() < 0.55 {
                1 + usize::from(dice.unit() < 0.5)
            } else {
                0
            };
            for k in 0..clump {
                #[allow(clippy::cast_precision_loss, reason = "0 or 1")]
                let along = k as f64 * 0.7;
                let (x, z) = (x + (xb - xa) / run * along, z + (zb - za) / run * along);
                let trunk = [x - 0.5, z - 0.5, x + 0.5, z + 0.5];
                let level_here = [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)]
                    .into_iter()
                    .all(|(dx, dz)| ((g.height)(x + dx, z + dz) - level).abs() < 0.3);
                if level_here && owned(g, trunk) && !reserved(g, trunk) && city.verge_clear(trunk) {
                    let total: u32 = SPECIES.iter().map(|(_, w)| w).sum();
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a pick among a few"
                    )]
                    let mut pick = (dice.unit() * f64::from(total)) as u32;
                    let species = SPECIES
                        .iter()
                        .find(|(_, w)| {
                            let hit = pick < *w;
                            pick = pick.saturating_sub(*w);
                            hit
                        })
                        .map_or(SPECIES[0].0, |(key, _)| *key);
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a turn, a scale near one and a variant index"
                    )]
                    city.plants.push(PlannedPlant {
                        species,
                        position: [x, level, z],
                        yaw: dice.unit() as f32,
                        scale: dice.range(0.8, 1.15) as f32,
                        variant: (dice.unit() * 16.0) as u8,
                    });
                }
            }
            s += dice.range(3.0, 6.5);
        }
    }
}

/// Plants a `kind` at `(x, z)` if the ground there is `level` and clear.
fn plant(
    city: &mut City,
    g: &Ground<'_>,
    dice: &mut Dice,
    [x, z]: [f64; 2],
    level: f64,
    kind: TreeKind,
) -> bool {
    let (_, _, reach) = species_for(&mut Dice(dice.0), kind);
    let trunk = [x - 0.8, z - 0.8, x + 0.8, z + 0.8];
    let level_here = [(-0.8, -0.8), (0.8, -0.8), (-0.8, 0.8), (0.8, 0.8)]
        .into_iter()
        .all(|(dx, dz)| ((g.height)(x + dx, z + dz) - level).abs() < 0.3);
    let crown = [x - reach, z - reach, x + reach, z + reach];
    if !level_here || !owned(g, crown) || reserved(g, trunk) || !city.clear(trunk, 0.4) {
        return false;
    }
    plant_at(city, dice, [x, z], level, kind);
    city.open.push(trunk);
    true
}

/// Where a walker starts: on the Exchange between the stalls and the
/// stair's head, on the first clear spot.
fn spawn(city: &City, g: &Ground<'_>) -> [f64; 3] {
    let ([cx, cz], [_, hz], y) = g.exchange;
    let candidates = [
        [STAIR_X, cz + hz - 6.0],
        [STAIR_X, cz + hz - 14.0],
        [cx, cz + hz - 8.0],
        [cx, cz],
    ];
    let [x, z] = candidates
        .into_iter()
        .find(|&[x, z]| {
            city.districts.values().flatten().all(|m| {
                let [a0, b0, a1, b1] = m.footprint;
                x < a0 - 1.0 || x > a1 + 1.0 || z < b0 - 1.0 || z > b1 + 1.0
            })
        })
        .unwrap_or([cx, cz]);
    [x, y, z]
}

/// One district's masses as a building: a part per mass, placed in the
/// frame, colliding by its hull.
fn district_building(name: &str, masses: &[Mass]) -> anyhow::Result<PlannedBuilding> {
    let mut builder = Building::builder(format!("serein:blockout-{name}"));
    let slots = MATERIALS
        .iter()
        .map(|(slot, key, _)| (*slot, *key))
        .chain(STRUCTURE);
    for (slot, key) in slots {
        if masses
            .iter()
            .any(|m| m.slot == slot || m.details.iter().any(|d| d.slot == slot))
        {
            builder = builder.material(slot, key);
        }
    }
    let mut footprint = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (i, mass) in masses.iter().enumerate() {
        let mut part = Part::builder(format!("{name}-{i}")).element(
            Element::new("mass", mass.geometry.clone(), mass.slot)
                .collision(Collision::Hull)
                .uv(UvMode::Box),
        );
        for detail in &mass.details {
            part = part.element(
                Element::new(detail.name, detail.geometry.clone(), detail.slot).uv(UvMode::Box),
            );
        }
        let part = part.build()?;
        builder = builder.part(part).instance(
            Instance::new(format!("{name}-{i}"), format!("{name}-{i}")).placed(mass.pose),
        );
        footprint[0] = footprint[0].min(mass.footprint[0]);
        footprint[1] = footprint[1].min(mass.footprint[1]);
        footprint[2] = footprint[2].max(mass.footprint[2]);
        footprint[3] = footprint[3].max(mass.footprint[3]);
    }
    Ok(PlannedBuilding {
        name: format!("blockout-{name}"),
        building: builder.build()?,
        position: [0.0; 3],
        yaw: 0.0,
        footprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hillside rising up-frame (towards -z) at a fifth, terraced in 6 m
    /// steps, the sea past z = 180, with the three platforms.
    fn height(x: f64, z: f64) -> f64 {
        if (x - 70.0).abs() < 65.0 && (z + 10.0).abs() < 62.0 {
            return 44.0;
        }
        if (x + 60.0).abs() < 58.0 && (z - 20.0).abs() < 48.0 {
            return 38.0;
        }
        if x.abs() < 100.0 && (z + 390.0).abs() < 70.0 {
            return 100.0;
        }
        if (x - 235.0).abs() < 36.0 && (z - 20.0).abs() < 20.0 {
            return 33.0;
        }
        let natural = (180.0 - z) * 0.2;
        if natural < 0.0 {
            return natural;
        }
        if natural < 4.0 {
            return 4.0;
        }
        4.0 + ((natural - 4.0) / 6.0).floor() * 6.0
    }

    fn owns(x: f64, z: f64) -> bool {
        x.abs() < 320.0 && (-460.0..250.0).contains(&z)
    }

    fn ground() -> Ground<'static> {
        Ground {
            height: &height,
            owns: &owns,
            starport: ([70.0, -10.0], [65.0, 62.0], 44.0),
            exchange: ([-60.0, 20.0], [58.0, 48.0], 38.0),
            processor: ([0.0, -390.0], [100.0, 70.0], 100.0),
            headquarters: ([235.0, 20.0], [36.0, 20.0], 33.0),
            quay: 4.0,
            road_exit: [0.0, -470.0],
        }
    }

    #[test]
    fn the_blockout_builds_every_district() {
        let plan = port_serein(&ground()).expect("the blockout builds");
        let names: Vec<_> = plan.buildings.iter().map(|b| b.name.as_str()).collect();
        // The Exchange has no blockout mass: the ground paves its square
        // and its buildings are kits.
        for district in [
            "starport",
            "stair",
            "lanes",
            "quay",
            "terraces",
            "processor",
            "transit",
            "gate",
        ] {
            assert!(
                names.contains(&format!("blockout-{district}").as_str()),
                "{district}: {names:?}"
            );
        }
    }

    #[test]
    fn a_gully_between_flats_is_left_as_a_bank() {
        let hill = |_: f64, z: f64| 30.0 + (-z * 0.18).clamp(0.0, 30.0);
        let g = Ground {
            height: &hill,
            ..ground()
        };
        assert!(
            walls_between(&g, -200.0, -160.0).is_empty(),
            "a gentle unbuilt slope must not become a deep retaining solid"
        );
    }

    /// No kit building of the plan lays two surfaces on one plane where
    /// they show; the content step checks the same over the real ground.
    #[test]
    #[ignore = "meshes every kit building of the plan"]
    fn no_kit_building_z_fights_itself() {
        use ashlar_manifold::{ManifoldMesher, mesh_building};
        let plan = port_serein(&ground()).expect("the blockout builds");
        let mut fights = Vec::new();
        let mut total = 0.0;
        for planned in &plan.buildings {
            if planned.name.starts_with("blockout-") {
                continue;
            }
            let meshed =
                mesh_building(&planned.building, &ManifoldMesher::default()).expect("meshes");
            for o in meshed.coplanar_overlaps() {
                if !o.covered && !o.same_binding() && o.normal.y > -0.9 {
                    total += o.area;
                    fights.push(format!(
                        "{}: {} ({}) / {} ({}) {:.3} m² at {:.2?} facing {:.1?}",
                        planned.name,
                        o.first,
                        o.first_binding.material,
                        o.second,
                        o.second_binding.material,
                        o.area,
                        o.point.to_array(),
                        o.normal.to_array()
                    ));
                }
            }
        }
        // The content step's threshold: a stagger may by chance line a
        // shed's back up with a neighbour's plinth for a few square
        // centimetres.
        assert!(total < 0.5, "{total:.2} m²: {fights:#?}");
    }

    #[test]
    fn split_level_entries_land_on_both_benches() {
        let g = ground();
        let mut city = City::default();
        split_houses(&mut city, &g);
        let houses = &city.kits["terrace-houses"];
        assert!(!houses.is_empty());
        for placed in houses {
            let Kit::SplitHouse {
                house,
                rise,
                depth,
                upper_z,
            } = placed.kit
            else {
                panic!("a bank dwelling");
            };
            let [x, low, z] = placed.at;
            let low_door = z + depth * 0.5 + 1.0;
            let high_door = z + upper_z - house.depth * 0.5 - 1.0;
            for dx in [-house.width * 0.5, 0.0, house.width * 0.5] {
                assert!(((g.height)(x + dx, low_door) - low).abs() < 0.01);
                assert!(((g.height)(x + dx, high_door) - low - rise).abs() < 0.01);
            }
        }
    }

    #[test]
    fn terrace_houses_keep_off_the_stairs_and_the_line() {
        let g = ground();
        let mut city = City::default();
        main_stair(&mut city, &g);
        transit(&mut city, &g);
        lane_stairs(&mut city, &g);
        terraces(&mut city, &g);
        let houses = &city.kits["terrace-houses"];
        assert!(houses.len() > 40, "{} houses", houses.len());
        let apart = |[x0, z0, x1, z1]: [f64; 4], [a0, b0, a1, b1]: [f64; 4]| {
            x1 <= a0 || x0 >= a1 || z1 <= b0 || z0 >= b1
        };
        for house in houses {
            let Kit::House(h) = house.kit else { continue };
            let [x, _, z] = house.at;
            // The house's own reach round its centre, turned any way.
            let r = h.lot_width().min(h.depth) * 0.5 - 0.5;
            let core = [x - r, z - r, x + r, z + r];
            for (district, masses) in &city.districts {
                if district != "terraces" && district != "walls" {
                    for other in masses {
                        assert!(apart(core, other.footprint), "a house on the {district}");
                    }
                }
            }
        }
    }
}
