//! The chart (ticket 0011, step 3): the ground round a point as a picture,
//! and what the household has found on it, as plain data.
//!
//! Nothing here draws a window. [`ground`] fills a byte buffer a screen
//! turns into an image off the frame; [`marks`] lists what to lay over it.
//! Both are pure in the world and the square, so a picture is the same
//! bytes on any thread and a test can hold it. North is `+z` and is up, and
//! east is right, which is `-x`: the world is right-handed with `+y` up, so
//! seen from above with north at the top `+x` lies to the west, as the
//! sim's own [`Compass`](crate::deposits::Compass) has it.
//!
//! The colours are how a picture reads, not rules of play, so they are named
//! constants here and not data.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "pixel arithmetic: picture sides are a few hundred, colours are clamped to a byte first"
)]

use strata_registry::GameRegistries;
use strata_worldgen::hydrology::{Network, RiverParams};
use strata_worldgen::roads::RoadClass;
use strata_worldgen::{ProceduralWorld, Sample, WorldSource};

use strata_worldgen::factions::{FactionId, FactionMap, Holding};

use crate::deposits::{Deposit, DepositId, Found, Lead, Surveyed};
use crate::harvest::Harvesters;
use crate::sites::SiteId;
use strata_registry::Reading;

/// The sea in the shallows; deeper water darkens toward [`SEA_DEEP`].
pub const SEA_SHALLOW: [f32; 3] = [60.0, 130.0, 185.0];
/// The sea at [`SEA_DEEP_M`] and below.
pub const SEA_DEEP: [f32; 3] = [20.0, 55.0, 110.0];
/// How far under the surface the sea reaches [`SEA_DEEP`], metres.
pub const SEA_DEEP_M: f32 = 500.0;
/// A lake, and a river wide enough to fill a pixel, where a sample is in it.
pub const LAKE: [f32; 3] = [70.0, 140.0, 200.0];
/// A thin river by its discharge: light at a creek.
pub const RIVER_LIGHT: [f32; 3] = [110.0, 190.0, 250.0];
/// A thin river by its discharge: deep at a main stem.
pub const RIVER_DEEP: [f32; 3] = [20.0, 70.0, 180.0];
/// The pale line the land makes where it meets water, so a shore reads.
pub const SHORE: [f32; 3] = [232.0, 226.0, 196.0];
/// How much of [`SHORE`] a shore pixel takes, `0..=1`.
pub const SHORE_SHARE: f32 = 0.7;
/// A service road, over its casing.
pub const SERVICE: [f32; 3] = [225.0, 45.0, 35.0];
/// A track.
pub const TRACK: [f32; 3] = [245.0, 175.0, 40.0];
/// A trail, drawn dashed.
pub const TRAIL: [f32; 3] = [250.0, 250.0, 250.0];
/// The dark edge under a service road or a track, so it reads on any tint.
pub const CASING: [f32; 3] = [25.0, 20.0, 20.0];

/// Half widths of the roads in pixels: service, track, trail. A road keeps
/// its weight at every scale, as a printed map does.
const ROAD_HALF_PX: [f32; 3] = [1.3, 0.9, 0.6];
/// Casing added round a service road and a track, pixels.
const CASING_PX: f32 = 0.9;
/// A trail's dash: on, then off, in pixels.
const DASH_PX: [f32; 2] = [5.0, 4.0];
/// The least half width a river is drawn at, pixels.
const RIVER_MIN_HALF_PX: f32 = 0.7;
/// The most, so a lake-wide mouth does not smear the picture.
const RIVER_MAX_HALF_PX: f32 = 8.0;
/// Discharge at which a river reaches [`RIVER_DEEP`], m3/s (worldmap's).
const RIVER_DEEP_M3S: f32 = 5.0;
/// Discharge at which a river is [`RIVER_LIGHT`], m3/s.
const RIVER_LIGHT_M3S: f32 = 0.25;
/// How much taller than it is the relief is taken to be when it is lit.
const RELIEF_EXAGGERATION: f64 = 3.5;
/// The ambient floor of the shade, so a face turned from the sun is dim and
/// not black, as a share of a flat face.
const SHADE_FLOOR: f32 = 0.45;
/// How far the shade runs over a flat face's, per unit of light.
const SHADE_GAIN: f32 = 0.9;

/// The square of ground a picture shows: where it is centred, how many
/// metres it spans and how many pixels it has a side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chart {
    /// The middle, world `(x, z)`.
    pub centre: [f64; 2],
    /// The side, metres.
    pub span_m: f64,
    /// The side, pixels.
    pub px: u32,
}

impl Chart {
    /// The square of `span_m` metres centred on `centre`, `px` pixels a side.
    #[must_use]
    pub const fn square(centre: [f64; 2], span_m: f64, px: u32) -> Self {
        Self { centre, span_m, px }
    }

    /// Metres a pixel spans.
    #[must_use]
    pub fn metres_per_px(&self) -> f64 {
        self.span_m / f64::from(self.px.max(1))
    }

    /// A world point as a fractional pixel, `x` to the east (`-x` in the
    /// world) and `y` down the picture, so north is up. The picture's
    /// corner is `(0, 0)`.
    #[must_use]
    pub fn to_pixel(&self, x: f64, z: f64) -> [f64; 2] {
        let mpp = self.metres_per_px();
        let half = 0.5 * self.span_m;
        [
            ((self.centre[0] + half) - x) / mpp,
            ((self.centre[1] + half) - z) / mpp,
        ]
    }

    /// The world point at a fractional pixel: the inverse of
    /// [`to_pixel`](Self::to_pixel).
    #[must_use]
    pub fn to_world(&self, pixel: [f64; 2]) -> [f64; 2] {
        let mpp = self.metres_per_px();
        let half = 0.5 * self.span_m;
        [
            self.centre[0] + half - pixel[0] * mpp,
            self.centre[1] + half - pixel[1] * mpp,
        ]
    }

    /// A world point as a share of the picture, `0..=1` across and down,
    /// which a screen scales to its node.
    #[must_use]
    pub fn unit(&self, x: f64, z: f64) -> [f64; 2] {
        let [px, py] = self.to_pixel(x, z);
        let side = f64::from(self.px.max(1));
        [px / side, py / side]
    }

    /// Whether a disc of `radius_m` at `(x, z)` touches the square.
    #[must_use]
    pub fn touches(&self, x: f64, z: f64, radius_m: f64) -> bool {
        let reach = 0.5 * self.span_m + radius_m;
        (x - self.centre[0]).abs() <= reach && (z - self.centre[1]).abs() <= reach
    }
}

/// The picture of `square`: `px * px` RGBA bytes, rows from the north edge
/// down. Land is its biome's tint under a hill shade lit from the north
/// west, the sea darker with depth, lakes and wide rivers in water, a
/// lighter line along every shore; then thin rivers, then the roads by
/// class over their casing.
///
/// A sample a pixel, so a screen runs it on a task. A mass nobody has
/// baked is baked first, on this thread: prepare the water round the
/// square before asking.
#[must_use]
pub fn ground(world: &ProceduralWorld, registries: &GameRegistries, square: &Chart) -> Vec<u8> {
    let n = square.px as usize;
    let mpp = square.metres_per_px();
    let half = 0.5 * square.span_m;
    let tints: Vec<[u8; 3]> = registries
        .biomes
        .values()
        .map(|def| def.tint.to_rgb8())
        .collect();
    let sea = world.sea_level();

    // One pixel of margin all round, for the shade and the shore. Grid row
    // `j` is south to north, picture row `n - j` is its place.
    let side = n + 2;
    let mut grid = Vec::new();
    world.sample_grid(
        square.centre[0] - half - 0.5 * mpp,
        square.centre[1] - half - 0.5 * mpp,
        mpp,
        side,
        &mut grid,
    );
    // Picture coordinates run -1 to n, which the margin holds; picture row
    // `r` is grid row `n - r`.
    let cell = |col: isize, row: isize| {
        // The grid runs west to east in `x`, the picture east to west.
        let c = usize::try_from(n as isize - col).unwrap_or(0).min(side - 1);
        let r = usize::try_from(n as isize - row).unwrap_or(0).min(side - 1);
        grid[r * side + c]
    };

    let wet = |s: &Sample| s.is_submerged(sea) || s.standing_water().is_some();
    let mut canvas = Canvas {
        n,
        pixels: vec![[0.0; 3]; n * n],
    };
    for row in 0..n {
        for col in 0..n {
            let (c, r) = (col as isize, row as isize);
            let s = cell(c, r);
            let colour = if s.is_submerged(sea) {
                let depth = ((sea - s.height) / SEA_DEEP_M).clamp(0.0, 1.0);
                mix(SEA_SHALLOW, SEA_DEEP, depth)
            } else if s.standing_water().is_some() {
                LAKE
            } else {
                let tint = tints
                    .get(s.dominant_biome().0 as usize)
                    .copied()
                    .unwrap_or([128, 128, 128]);
                let lit = sunlight(&cell, c, r, sea, mpp) as f32;
                // A flat face is lit by the sun's elevation alone.
                let flat = std::f32::consts::FRAC_1_SQRT_2;
                let shade = SHADE_FLOOR + SHADE_GAIN * (lit / flat).min(1.6) * (1.0 - SHADE_FLOOR);
                let base = tint.map(f32::from);
                let mut colour = base.map(|v| v * shade);
                let shore = [(c - 1, r), (c + 1, r), (c, r - 1), (c, r + 1)]
                    .iter()
                    .any(|&(nc, nr)| wet(&cell(nc, nr)));
                if shore {
                    colour = mix(colour, SHORE, SHORE_SHARE);
                }
                colour
            };
            canvas.pixels[row * n + col] = colour;
        }
    }

    // The network under the square, baked on this thread when it is not.
    let reach = 0.5 * square.span_m + 500.0;
    let networks = world.hydrology().bake_area_on(
        world,
        (square.centre[0] - reach, square.centre[1] - reach),
        (square.centre[0] + reach, square.centre[1] + reach),
        1,
    );
    draw_rivers(&mut canvas, &networks, square);
    draw_roads(&mut canvas, &networks, square);

    let mut bytes = Vec::with_capacity(n * n * 4);
    for p in &canvas.pixels {
        bytes.extend(p.map(|v| v.round().clamp(0.0, 255.0) as u8));
        bytes.push(255);
    }
    bytes
}

/// A blend of two colours, `t` of the second.
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// How much of a sun in the north west, 45 degrees up, falls on the ground
/// at a pixel, `0..=1`, over its neighbours' heights (the sea taken as its
/// surface, so a coast is not a cliff into the sea bed).
fn sunlight(
    cell: &impl Fn(isize, isize) -> Sample,
    col: isize,
    row: isize,
    sea: f32,
    mpp: f64,
) -> f64 {
    let h = |c: isize, r: isize| f64::from(cell(c, r).height.max(sea)) * RELIEF_EXAGGERATION;
    let dx = (h(col + 1, row) - h(col - 1, row)) / (2.0 * mpp);
    // Rows grow southwards, so the slope northwards is the negative of the
    // one down the picture.
    let dy = (h(col, row - 1) - h(col, row + 1)) / (2.0 * mpp);
    let l = std::f64::consts::FRAC_1_SQRT_2;
    // North west and up. The picture's right is the world's west, so the
    // sun to the north west is to the picture's upper right: x right, y
    // north, z up.
    let light = [l * l, l * l, l];
    let length = (dx * dx + dy * dy + 1.0).sqrt();
    ((-dx * light[0] - dy * light[1] + light[2]) / length).max(0.0)
}

/// The picture being drawn, in floats.
struct Canvas {
    n: usize,
    pixels: Vec<[f32; 3]>,
}

impl Canvas {
    /// Covers the pixels within `radius` of `centre` (fractional pixels),
    /// each by how much of it the disc holds, so an edge is soft.
    fn disc(&mut self, centre: [f64; 2], radius: f32, colour: [f32; 3]) {
        let reach = f64::from(radius) + 1.0;
        let lo = |v: f64| (v - reach).floor().max(0.0) as usize;
        let hi = |v: f64| ((v + reach).ceil().max(0.0) as usize).min(self.n.saturating_sub(1));
        if centre[0] + reach < 0.0 || centre[1] + reach < 0.0 {
            return;
        }
        for py in lo(centre[1])..=hi(centre[1]) {
            for px in lo(centre[0])..=hi(centre[0]) {
                let dx = px as f64 + 0.5 - centre[0];
                let dy = py as f64 + 0.5 - centre[1];
                let d = (dx * dx + dy * dy).sqrt() as f32;
                let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let p = &mut self.pixels[py * self.n + px];
                    *p = mix(*p, colour, cover);
                }
            }
        }
    }

    /// A line of discs from `a` to `b`; `dash`, when given, is
    /// `(on, off, start)` in pixels and the distance already run, and the
    /// line only draws in its on stretches. Returns the length run.
    fn line(
        &mut self,
        a: [f64; 2],
        b: [f64; 2],
        radius: f32,
        colour: [f32; 3],
        dash: Option<([f32; 2], f32)>,
    ) -> f32 {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt();
        let steps = (length / 0.5).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            if let Some(([on, off], start)) = dash {
                let along = start + (t * length) as f32;
                if along % (on + off) >= on {
                    continue;
                }
            }
            self.disc([a[0] + dx * t, a[1] + dy * t], radius, colour);
        }
        length as f32
    }
}

/// Whether a segment of points is anywhere near the picture.
fn near(square: &Chart, x: f64, z: f64) -> bool {
    square.touches(x, z, 200.0)
}

/// Rivers by discharge, small first so the main stems lie on top, each
/// channel's winding centreline never thinner than a pixel.
fn draw_rivers(canvas: &mut Canvas, networks: &[std::sync::Arc<Network>], square: &Chart) {
    let mpp = square.metres_per_px();
    let params = RiverParams::DEFAULT;
    let step = (mpp * 0.75).max(1.0);
    let mut reaches: Vec<(f32, &strata_worldgen::hydrology::River)> = networks
        .iter()
        .flat_map(|net| net.rivers().iter())
        .filter(|river| river.vertices.iter().any(|v| near(square, v.x, v.z)))
        .map(|river| {
            (
                river.vertices.last().map_or(0.0, |v| v.discharge_m3s),
                river,
            )
        })
        .collect();
    reaches.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (q, river) in reaches {
        let t = ((q.max(0.0).sqrt() - RIVER_LIGHT_M3S.sqrt())
            / (RIVER_DEEP_M3S.sqrt() - RIVER_LIGHT_M3S.sqrt()))
        .clamp(0.0, 1.0);
        let colour = mix(RIVER_LIGHT, RIVER_DEEP, t);
        let points = river.centreline(&params, step);
        for pair in points.windows(2) {
            if !near(square, pair[0].x, pair[0].z) {
                continue;
            }
            let half_px = (pair[0].width_m / mpp * 0.5) as f32;
            let radius = half_px.clamp(RIVER_MIN_HALF_PX, RIVER_MAX_HALF_PX);
            let a = square.to_pixel(pair[0].x, pair[0].z);
            let b = square.to_pixel(pair[1].x, pair[1].z);
            canvas.line(a, b, radius, colour, None);
        }
    }
}

/// Roads by class, the faintest first so the backbone lies over its
/// branches; casing under a service road and a track.
fn draw_roads(canvas: &mut Canvas, networks: &[std::sync::Arc<Network>], square: &Chart) {
    for class in [RoadClass::Trail, RoadClass::Track, RoadClass::Service] {
        let (k, colour) = match class {
            RoadClass::Service => (0, SERVICE),
            RoadClass::Track => (1, TRACK),
            RoadClass::Trail => (2, TRAIL),
        };
        let half_px = ROAD_HALF_PX[k];
        let casing = class != RoadClass::Trail;
        for pass in 0..2 {
            if pass == 0 && !casing {
                continue;
            }
            for road in networks
                .iter()
                .flat_map(|net| net.roads().iter())
                .filter(|road| road.class == class)
                .filter(|road| road.points.iter().any(|p| near(square, p.x, p.z)))
            {
                let mut run = 0.0_f32;
                for pair in road.points.windows(2) {
                    let a = square.to_pixel(pair[0].x, pair[0].z);
                    let b = square.to_pixel(pair[1].x, pair[1].z);
                    if pass == 0 {
                        canvas.line(a, b, half_px + CASING_PX, CASING, None);
                    } else if class == RoadClass::Trail {
                        run += canvas.line(a, b, half_px, colour, Some((DASH_PX, run)));
                    } else {
                        canvas.line(a, b, half_px, colour, None);
                    }
                }
            }
        }
    }
}

/// A place on the chart that is not a deposit: a settlement or a site.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteMark {
    /// Its name.
    pub name: String,
    /// What its kind is called for a player; empty for a settlement, whose
    /// word the screen owns.
    pub kind_name: String,
    /// Its kind's id (`strata:town`), which a screen picks a symbol by;
    /// empty for a settlement.
    pub kind: String,
    /// Where, world `(x, z)`.
    pub at: [f64; 2],
    /// Whether it is an authored settlement (Port Serein) and not a
    /// procedural site.
    pub settlement: bool,
    /// The site's id, which its holder is asked by; none for a settlement.
    pub id: Option<SiteId>,
    /// The name of the faction that holds it, once [`name_holders`] has
    /// asked; none for a place of a kind no faction holds.
    pub held_by: Option<String>,
}

/// A deposit the household has found.
#[derive(Debug, Clone, PartialEq)]
pub struct DepositMark {
    /// The deposit, whole.
    pub deposit: Deposit,
    /// What is known of it.
    pub found: Found,
    /// Whether it is the one the HUD tracks.
    pub tracked: bool,
}

/// A planted harvester.
#[derive(Debug, Clone, PartialEq)]
pub struct HarvesterMark {
    /// Its number.
    pub id: u32,
    /// The deposit it stands on.
    pub deposit: DepositId,
    /// Where, world `(x, z)`.
    pub at: [f64; 2],
    /// Units in its hopper.
    pub held: u32,
    /// Units it holds at most.
    pub hopper: u32,
    /// Whether the hopper is at its mark ([`Harvester::is_full`]).
    pub full: bool,
}

/// What the chart marks in a square.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marks {
    /// Settlements first, then the sites, nearest the middle first.
    pub sites: Vec<SiteMark>,
    /// Found deposits whose area touches the square, by id.
    pub deposits: Vec<DepositMark>,
    /// The tracked deposit when it lies outside the square, for an arrow
    /// on the edge.
    pub tracked_away: Option<DepositMark>,
    /// The last ping's lead, when it lies in the square.
    pub lead: Option<Lead>,
    /// The household's harvesters in the square, in the order planted.
    pub harvesters: Vec<HarvesterMark>,
}

/// Every settlement and site in `square`: settlements first, then the
/// sites round it. A screen asks this on its picture's task and keeps the
/// answer with the picture, since the sites come off the baked network over
/// most of the square and a mass nobody has baked is baked first, on the
/// calling thread.
#[must_use]
pub fn site_marks(world: &ProceduralWorld, square: &Chart) -> Vec<SiteMark> {
    let mut out = Vec::new();
    for (site, name) in world.sites().iter().zip(world.settlement_names()) {
        if square.touches(site.centre[0], site.centre[1], 0.0) {
            out.push(SiteMark {
                name: name.clone(),
                kind_name: String::new(),
                kind: String::new(),
                at: site.centre,
                settlement: true,
                id: None,
                held_by: None,
            });
        }
    }
    let reach = 0.75 * square.span_m;
    for site in world.sites_near(square.centre[0], square.centre[1], reach) {
        if !square.touches(site.centre[0], site.centre[1], 0.0) {
            continue;
        }
        let kind = world
            .site_kinds()
            .iter()
            .find(|kind| kind.id == site.kind());
        out.push(SiteMark {
            name: site.name,
            kind_name: kind.map_or_else(String::new, |kind| kind.def.name.clone()),
            kind: kind.map_or_else(String::new, |kind| kind.name.to_string()),
            at: site.centre,
            settlement: false,
            id: Some(site.id),
            held_by: None,
        });
    }
    out
}

/// Writes into each of `sites` the name of the faction that holds it: a
/// procedural site by its id, a settlement by whose ground its centre is.
pub fn name_holders(map: &FactionMap, sites: &mut [SiteMark]) {
    for site in sites {
        let holder = match site.id {
            Some(id) => map.holder_of(&id),
            None => map.territory(site.at[0], site.at[1]).holder,
        };
        site.held_by = holder
            .and_then(|id| map.faction(id))
            .map(|faction| faction.name);
    }
}

/// The least alpha a held pixel is tinted at, the edge of a holding's reach.
pub const TERRITORY_ALPHA_MIN: f32 = 0.22;
/// The most alpha a held pixel is tinted at, at a seat: the ground has to
/// read under it.
pub const TERRITORY_ALPHA_MAX: f32 = 0.52;
/// How many pixels in from a change of holder the line reaches.
const BORDER_PX: usize = 2;
/// The alpha of the line where the holder changes.
const BORDER_ALPHA: f32 = 0.95;
/// The side of one stripe of the hatching on contested ground, pixels.
const HATCH_PX: u32 = 4;
/// The dark stripe on contested ground, over the holder's tint.
const HATCH_SHADE: [f32; 3] = [12.0, 12.0, 20.0];
/// How far a pixel's blue must lead its red for it to read as water.
const SEA_BLUE_LEAD: i32 = 40;
/// The alpha of that stripe.
const HATCH_ALPHA: f32 = 0.6;

/// A faction that holds some of a territory picture.
#[derive(Debug, Clone, PartialEq)]
pub struct HolderKey {
    /// Who.
    pub id: FactionId,
    /// Its name.
    pub name: String,
    /// Its colour.
    pub colour: [u8; 3],
    /// Pixels of the picture it holds.
    pub pixels: u32,
}

/// The territory over a square: the tint and the factions in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerritoryPicture {
    /// `px * px` RGBA bytes, rows from the north edge down, clear where no
    /// one's ground reaches.
    pub bytes: Vec<u8>,
    /// The factions with ground in it, the widest first.
    pub holders: Vec<HolderKey>,
}

/// The holdings whose ground can touch `square`, which [`territory`] needs
/// gathered once and not a pixel at a time.
#[must_use]
pub fn holdings_for(map: &FactionMap, square: &Chart) -> Vec<Holding> {
    let reach = 0.75 * square.span_m + map.max_reach_m();
    map.holdings_near(square.centre[0], square.centre[1], reach)
}

/// Whose ground each pixel of `square` is, as a tint a screen lays over
/// [`ground`]: the holder's colour at an alpha that rises with its strength,
/// stripes where the ground is contested, a line where the holder changes.
/// `holdings` is [`holdings_for`] the same square, and `ground` is the
/// [`ground`] bytes of it: no one holds the sea, so a tint over open water
/// would only mix with its blue into a colour no faction has.
#[must_use]
pub fn territory(
    map: &FactionMap,
    holdings: &[Holding],
    square: &Chart,
    ground: &[u8],
) -> TerritoryPicture {
    let n = square.px as usize;
    let mut cells = Vec::with_capacity(n * n);
    for row in 0..n {
        for col in 0..n {
            let [x, z] = square.to_world([col as f64 + 0.5, row as f64 + 0.5]);
            let t = map.territory_among(holdings, x, z);
            cells.push((t.holder, t.strength as f32, t.contested));
        }
    }
    let mut known: std::collections::BTreeMap<FactionId, ([u8; 3], u32)> =
        std::collections::BTreeMap::default();
    let mut names = std::collections::BTreeMap::new();
    let mut bytes = vec![0_u8; n * n * 4];
    for row in 0..n {
        for col in 0..n {
            let (holder, strength, contested) = cells[row * n + col];
            let Some(id) = holder else { continue };
            let at = (row * n + col) * 4;
            if ground
                .get(at..at + 3)
                .is_some_and(|px| i32::from(px[2]) - i32::from(px[0]) > SEA_BLUE_LEAD)
            {
                continue;
            }
            let entry = known.entry(id).or_insert_with(|| {
                let faction = map.faction(id);
                let colour = faction.as_ref().map_or([128; 3], |f| f.colour);
                names.insert(id, faction.map(|f| f.name).unwrap_or_default());
                (colour, 0)
            });
            entry.1 += 1;
            let colour = entry.0.map(f32::from);
            let differs = |c: usize, r: usize| cells[r * n + c].0 != holder;
            let edge = (1..=BORDER_PX).any(|d| {
                (col >= d && differs(col - d, row))
                    || (col + d < n && differs(col + d, row))
                    || (row >= d && differs(col, row - d))
                    || (row + d < n && differs(col, row + d))
            });
            let alpha = TERRITORY_ALPHA_MIN
                + (TERRITORY_ALPHA_MAX - TERRITORY_ALPHA_MIN) * strength.clamp(0.0, 1.0);
            let (rgb, a) = if edge {
                (colour, BORDER_ALPHA)
            } else if contested && ((col as u32 + row as u32) / HATCH_PX).is_multiple_of(2) {
                (HATCH_SHADE, HATCH_ALPHA)
            } else {
                (colour, alpha)
            };
            let k = (row * n + col) * 4;
            bytes[k] = rgb[0].round().clamp(0.0, 255.0) as u8;
            bytes[k + 1] = rgb[1].round().clamp(0.0, 255.0) as u8;
            bytes[k + 2] = rgb[2].round().clamp(0.0, 255.0) as u8;
            bytes[k + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    let mut holders: Vec<HolderKey> = known
        .into_iter()
        .map(|(id, (colour, pixels))| HolderKey {
            id,
            name: names.remove(&id).unwrap_or_default(),
            colour,
            pixels,
        })
        .collect();
    holders.sort_by(|a, b| b.pixels.cmp(&a.pixels).then(a.id.cmp(&b.id)));
    TerritoryPicture { bytes, holders }
}

/// What the chart marks in `square`: the `sites` that lie in it (from
/// [`site_marks`]), every deposit the household has found, the tracked one
/// and the lead, the harvesters. A deposit that is not in `surveyed.found`
/// is nowhere in it, bar the tracked one, which the HUD points to whether
/// it was found or not (the register lists it the same way).
///
/// `lookup` resolves a found id to its deposit (the survey field's `get`, or
/// the deposits listed near the household when a run has no field); the
/// tracked deposit, which `surveyed` holds whole, needs none.
#[must_use]
pub fn marks(
    sites: &[SiteMark],
    lookup: &dyn Fn(DepositId) -> Option<Deposit>,
    surveyed: &Surveyed,
    harvesters: &Harvesters,
    square: &Chart,
) -> Marks {
    let mut out = Marks {
        sites: sites
            .iter()
            .filter(|site| square.touches(site.at[0], site.at[1], 0.0))
            .cloned()
            .collect(),
        ..Marks::default()
    };

    let tracked = surveyed.tracked.as_ref().map(|deposit| deposit.id);
    let unfound = surveyed
        .tracked
        .iter()
        .filter(|held| !surveyed.found.contains_key(&held.id))
        .map(|held| {
            (
                held.id,
                Found {
                    reading: Reading::Type,
                    sampled: false,
                    best_pm: 0,
                    best_at: held.at,
                },
            )
        });
    for (id, found) in surveyed
        .found
        .iter()
        .map(|(&id, &found)| (id, found))
        .chain(unfound)
    {
        let deposit = match &surveyed.tracked {
            Some(held) if held.id == id => Some(held.clone()),
            _ => lookup(id),
        };
        let Some(deposit) = deposit else { continue };
        let mark = DepositMark {
            tracked: tracked == Some(id),
            deposit,
            found,
        };
        if square.touches(
            mark.deposit.at[0],
            mark.deposit.at[1],
            f64::from(mark.deposit.radius_m),
        ) {
            out.deposits.push(mark);
        } else if mark.tracked {
            out.tracked_away = Some(mark);
        }
    }
    out.lead = surveyed
        .lead
        .filter(|lead| square.touches(lead.at[0], lead.at[1], 0.0));
    out.harvesters = harvesters
        .list
        .iter()
        .filter(|each| square.touches(each.at[0], each.at[1], 0.0))
        .map(|each| HarvesterMark {
            id: each.id,
            deposit: each.deposit,
            at: each.at,
            held: each.held,
            hopper: each.hopper,
            full: each.is_full(),
        })
        .collect();
    out
}

/// Whether the water at a sample is a lake, for a test to tell one from a
/// river.
#[cfg(test)]
fn is_lake(sample: &Sample) -> bool {
    use strata_worldgen::hydrology::WaterKind;
    sample
        .standing_water()
        .is_some_and(|w| w.kind == WaterKind::Lake)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::many_single_char_names,
        clippy::float_cmp,
        clippy::unwrap_used,
        reason = "a test: channels and coordinates, exact mappings of round numbers"
    )]
    use super::*;
    use std::sync::{Arc, OnceLock};

    fn registries() -> &'static Arc<GameRegistries> {
        static R: OnceLock<Arc<GameRegistries>> = OnceLock::new();
        R.get_or_init(|| {
            let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
            Arc::new(strata_registry::load_from_dir(assets).expect("assets/ loads"))
        })
    }

    /// The seed 42 world with the water prepared round the city, 24 km out.
    fn world() -> &'static Arc<ProceduralWorld> {
        static W: OnceLock<Arc<ProceduralWorld>> = OnceLock::new();
        W.get_or_init(|| {
            let world = Arc::new(ProceduralWorld::with_params(
                strata_core::Seed(42),
                registries(),
                crate::world_params(),
            ));
            let city = world.sites()[0].centre;
            world.prepare_water_on(city[0], city[1], 30_000.0, 4);
            world
        })
    }

    fn city() -> [f64; 2] {
        world().sites()[0].centre
    }

    fn pixel(bytes: &[u8], n: u32, col: u32, row: u32) -> [u8; 3] {
        let k = ((row * n + col) * 4) as usize;
        [bytes[k], bytes[k + 1], bytes[k + 2]]
    }

    /// The pixel mapping puts north up and east (`-x`) right, and inverts.
    #[test]
    fn the_square_maps_a_point_to_a_pixel_and_back() {
        let square = Chart::square([1000.0, -2000.0], 4000.0, 400);
        assert_eq!(square.metres_per_px(), 10.0);
        assert_eq!(square.to_pixel(1000.0, -2000.0), [200.0, 200.0]);
        let north = square.to_pixel(1000.0, -1000.0);
        let plus_x = square.to_pixel(2000.0, -2000.0);
        let minus_x = square.to_pixel(0.0, -2000.0);
        assert!(north[1] < 200.0 && north[0] == 200.0, "north is up");
        assert!(
            plus_x[0] < 200.0 && plus_x[1] == 200.0,
            "+x lies to the west, the picture's left"
        );
        assert!(minus_x[0] > 200.0, "east is -x and is right");
        let back = square.to_world(square.to_pixel(-500.0, 300.0));
        assert!((back[0] + 500.0).abs() < 1e-9 && (back[1] - 300.0).abs() < 1e-9);
        assert_eq!(square.unit(1000.0, -2000.0), [0.5, 0.5]);
        assert!(square.touches(3000.0, -2000.0, 0.0));
        assert!(!square.touches(3001.0, -2000.0, 0.0));
        assert!(square.touches(3050.0, -2000.0, 100.0));
    }

    /// The same bytes twice, and from another thread.
    #[test]
    fn the_ground_is_the_same_bytes_twice_and_from_two_threads() {
        let square = Chart::square(city(), 6000.0, 96);
        let a = ground(world(), registries(), &square);
        assert_eq!(a.len(), 96 * 96 * 4);
        let b = ground(world(), registries(), &square);
        assert_eq!(a, b);
        let c = std::thread::scope(|s| {
            s.spawn(|| ground(world(), registries(), &square))
                .join()
                .unwrap()
        });
        assert_eq!(a, c);
    }

    /// The sea, a lake and a hill each read in their kind of colour: nearly
    /// every pixel of each (a road or a shore line may cover a few).
    #[test]
    fn sea_lake_and_hill_come_out_in_their_kind_of_colour() {
        let world = world();
        let square = Chart::square(city(), 48_000.0, 192);
        let bytes = ground(world, registries(), &square);
        let sea = world.sea_level();
        // (pixels of the kind, of them in the kind's colour), per kind.
        let mut tally = [(0_u32, 0_u32); 3];
        for row in 0..square.px {
            for col in 0..square.px {
                let [x, z] = square.to_world([f64::from(col) + 0.5, f64::from(row) + 0.5]);
                let s = world.sample(x, z);
                let [r, g, b] = pixel(&bytes, square.px, col, row).map(i32::from);
                let blue = b > r + 30 && b > g;
                if s.is_submerged(sea) {
                    tally[0].0 += 1;
                    tally[0].1 += u32::from(blue);
                } else if is_lake(&s) {
                    tally[1].0 += 1;
                    tally[1].1 += u32::from(blue);
                } else if s.height > sea + 400.0 && s.water.is_none() {
                    tally[2].0 += 1;
                    tally[2].1 += u32::from(b < r.max(g) + 40 && r + g + b > 60);
                }
            }
        }
        for (name, (total, ok)) in ["sea", "lake", "hill"].into_iter().zip(tally) {
            assert!(total > 0, "the square holds {name}");
            assert!(
                ok * 10 >= total * 9,
                "{name}: {ok} of {total} in its colour"
            );
        }
    }

    /// A road of the network leaves road coloured pixels along it.
    #[test]
    fn a_road_leaves_road_coloured_pixels_along_it() {
        let world = world();
        let city = city();
        let square = Chart::square(city, 6000.0, 384);
        let bytes = ground(world, registries(), &square);
        let networks = world.hydrology().bake_area_on(
            world,
            (city[0] - 3000.0, city[1] - 3000.0),
            (city[0] + 3000.0, city[1] + 3000.0),
            1,
        );
        let road = networks
            .iter()
            .flat_map(|n| n.roads().iter())
            .filter(|r| r.class == RoadClass::Service)
            .find(|r| {
                r.points
                    .iter()
                    .filter(|p| square.touches(p.x, p.z, -300.0))
                    .count()
                    > 20
            })
            .expect("a service road crosses the square");
        let mut on_road = 0;
        let mut total = 0;
        for p in road
            .points
            .iter()
            .filter(|p| square.touches(p.x, p.z, -300.0))
        {
            let [px, py] = square.to_pixel(p.x, p.z);
            let [r, g, b] = pixel(&bytes, square.px, px as u32, py as u32).map(i32::from);
            total += 1;
            // The centreline is the service colour itself, not its casing.
            let service = SERVICE.map(|v| v as i32);
            if (r - service[0]).abs() + (g - service[1]).abs() + (b - service[2]).abs() < 60 {
                on_road += 1;
            }
        }
        assert!(
            on_road * 10 >= total * 9,
            "{on_road} of {total} road points read as road"
        );
    }

    /// A found deposit is marked and an unfound one beside it is not; the
    /// tracked one is flagged; a harvester and the city are marked.
    #[test]
    fn marks_hold_a_found_deposit_and_not_an_unfound_one() {
        let world = world();
        let city = city();
        let field = crate::deposits::DepositField::new(
            Arc::clone(world),
            strata_core::Seed(42),
            Arc::clone(registries()),
        );
        let near = field.near(city[0], city[1], 4_000.0);
        assert!(near.len() >= 2, "two deposits near the city");
        let (first, second) = (near[0].clone(), near[1].clone());
        let mut surveyed = Surveyed::default();
        surveyed.found.insert(
            first.id,
            Found {
                reading: strata_registry::Reading::Exact,
                sampled: true,
                best_pm: first.peak,
                best_at: first.at,
            },
        );
        surveyed.tracked = Some(first.clone());
        let square = Chart::square(city, 12_000.0, 512);
        let sites = site_marks(world, &square);
        let marked = marks(
            &sites,
            &|id| field.get(id),
            &surveyed,
            &Harvesters::default(),
            &square,
        );
        let ids: Vec<_> = marked.deposits.iter().map(|m| m.deposit.id).collect();
        assert!(ids.contains(&first.id));
        assert!(!ids.contains(&second.id), "an unfound deposit has no mark");
        assert_eq!(ids.len(), 1);
        assert!(marked.deposits[0].tracked);
        assert!(marked.sites.iter().any(|s| s.settlement), "Port Serein");
        // Away: a square elsewhere has the tracked one as an arrow only.
        let far = Chart::square([city[0] + 60_000.0, city[1]], 3_000.0, 512);
        let away = marks(
            &site_marks(world, &far),
            &|id| field.get(id),
            &surveyed,
            &Harvesters::default(),
            &far,
        );
        assert!(away.deposits.is_empty());
        assert_eq!(away.tracked_away.map(|m| m.deposit.id), Some(first.id));
        // The tracked deposit is marked though it was never found.
        surveyed.found.clear();
        let unfound = marks(
            &sites,
            &|id| field.get(id),
            &surveyed,
            &Harvesters::default(),
            &square,
        );
        assert_eq!(unfound.deposits.len(), 1);
        assert!(unfound.deposits[0].tracked);
    }
}
