//! Rivers and roads painted at distance (ticket 0005, step 8a; ticket 0004,
//! step 10): the network's own lines, per region, drawn by the terrain's
//! fragment shader where the near drawing stops.
//!
//! # Why lines and not the vertex record
//!
//! A river or a road is metres wide and a far region's vertices are tens to
//! hundreds of metres apart, so a share carried per vertex either misses the
//! line between two vertices or smears it over two spacings: a creek drew as
//! a dotted blur, and a winding river as a straight chord with gaps where it
//! swung back between two vertices. The network already knows each line to
//! the metre (`River::centreline`, `Road::points`), so the meshing task
//! gathers the lines that cross its region, simplified to a tolerance the
//! level's spacing sets, and packs them with a small grid of buckets into a
//! fixed-size slot of a third store buffer ([`crate::store`]). The fragment
//! finds its bucket (each lists at most [`BUCKET_MOST`] strokes, the
//! shader's bound), measures its distance to the few lines in it and covers
//! the line's real width, antialiased by the pixel's own footprint across
//! the line. The line is the same at every level, so nothing moves where two
//! levels meet; only the tolerance changes, under a pixel.
//!
//! # Where the near drawing stops
//!
//! - **Rivers.** The water mesh ([`crate::river`]) draws a river at a level
//!   whose spacing resolves it ([`crate::river::RESOLVED_CELLS`]) and fades
//!   one the parent level leaves out over the morph band. The shader paints
//!   the river by the same rule turned round, so as the mesh fades out over
//!   its band the paint fades in over the same band, and where the mesh
//!   draws, the bed under its clear water is left alone.
//! - **Roads.** The carriageway near is the splat's: the road's material by
//!   how developed each vertex is. A road narrower than a level's spacing
//!   only shows where a vertex happens to land on it, a dotted blur, so a
//!   level whose spacing is the carriageway's width or more ([`way_dropped`])
//!   leaves the roads its paint holds out of its layer shares, keyed on the
//!   painted line ([`PaintedWays`], which the region's task hands the
//!   mesher), and the paint draws them; on the levels below, the paint
//!   fades in as the vertex spacing, morph included, grows from half the
//!   width to all of it. An authored settlement's own road is its laid
//!   ground, which the splat draws at every level, and is not painted.
//! - **Bridges.** Between a bridge's abutments the ground is the river's
//!   valley and the deck is the crossing's, drawn to [`DECK_DRAWN_M`]: the
//!   paint stops at the abutments there, and past that range, where no deck
//!   stands, it carries the road across, which is how a bridge reads from
//!   kilometres. A ford's way is painted under the river, a culvert's road
//!   over its creek.
//!
//! # Layout of a slot
//!
//! [`PAINT_WORDS`] words a region, little-endian:
//!
//! | Words | What |
//! | --- | --- |
//! | 0 | strokes (low 16 bits), the word the strokes start at (high 16) |
//! | 1 | bucket entries |
//! | 2, 3 | spare |
//! | 4 to 259 | the [`PAINT_GRID`] by [`PAINT_GRID`] buckets, row by row: first entry (low 16), entries (high 16) |
//! | 260 on | the entries, a stroke index in each 16 bits |
//! | then | the strokes, [`STROKE_WORDS`] each |
//!
//! A stroke is `[ax, az, bx, bz]` as `f32` metres from the region's minimum
//! corner, its half widths at `a` and `b` as two halves, and a word of
//! [`flags`](Stroke::flags). A region with nothing to paint writes word 0 as
//! nought and nothing else, and the fragment stops there.

// Grid indices, word offsets and stroke counts are small; positions are
// metres within a region's reach, well inside f32.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "grid indices, word offsets and region-local metres are small and exact in every width used"
)]
// The geometry uses the conventional single letters for points and vectors.
#![allow(clippy::many_single_char_names, reason = "segment and vector maths")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use strata_core::RegionCoord;
use strata_registry::RoadClass;
use strata_worldgen::ProceduralWorld;
use strata_worldgen::hydrology::{Network, River, RiverParams};
use strata_worldgen::roads::CrossingKind;

/// Buckets a side of a region's paint.
pub const PAINT_GRID: usize = 16;

/// Words before the buckets.
pub const HEADER_WORDS: usize = 4;

/// The word the bucket entries start at.
pub const ENTRY_WORD: usize = HEADER_WORDS + PAINT_GRID * PAINT_GRID;

/// Words of one stroke.
pub const STROKE_WORDS: usize = 6;

/// Words of one region's paint: the header, the buckets and room for 512
/// strokes with four bucket entries each, or fewer strokes in more buckets.
/// The river country round the showcase holds up to 270 strokes a region
/// at [`tolerance_m`]; a slot of half this widened the tolerance in a
/// quarter of its level-6 regions.
pub const PAINT_WORDS: usize = ENTRY_WORD + 4096;

/// Bytes of one region's paint slot: 17 424, under seven percent of its
/// vertex records; a region uploads only the words it uses.
pub const PAINT_BYTES: u64 = (PAINT_WORDS * 4) as u64;

/// The most strokes one bucket lists: the fragment reads no more than this
/// (the shader's `PAINT_MOST`), so the fit leaves out a line that would
/// list more in any bucket rather than let the shader drop strokes from
/// the middle of one.
pub const BUCKET_MOST: usize = 48;

/// A stroke's kind in the low bits of its flags: a river's water.
pub const KIND_RIVER: u32 = 1;

/// A road's carriageway.
pub const KIND_ROAD: u32 = 2;

/// Set on a road's stroke between a bridge's abutments: drawn only past
/// [`DECK_DRAWN_M`].
pub const FLAG_GAP: u32 = 1 << 4;

/// Set on a road's stroke between a ford's wetted edges: drawn under the
/// river's paint.
pub const FLAG_FORD: u32 = 1 << 5;

/// How far from the observer a crossing's deck is drawn, metres: the game's
/// crossings are spawned within this (`apps/game`'s `crossings::DRAW_M`,
/// which is this constant), and the paint carries a road over a bridge's gap
/// only past it.
pub const DECK_DRAWN_M: f64 = 4_000.0;

/// Over this many metres inside [`DECK_DRAWN_M`] the paint over a bridge's
/// gap fades in, so the deck's own range edge is not a hard line.
pub const DECK_FADE_M: f64 = 200.0;

/// A river's point the shader paints only where the water mesh does not
/// draw, as `crate::river` decides: the narrowest river a level of `lod`
/// with vertices `step` apart draws, metres. The mirror the tests hold to
/// the shader's.
#[must_use]
pub fn resolved_width(lod: u8, step: f32) -> f32 {
    if lod == 0 {
        step
    } else {
        crate::river::RESOLVED_CELLS * step
    }
}

/// How much of a river `width` metres wide the water mesh draws at a level
/// of `lod` with vertices `step` apart, `morph` of the way to its parent:
/// all of it where the parent level draws it too, fading out over the morph
/// where only this level does, none where neither does. A fifth of the
/// width either side softens the rule, since the mesh reads the record's
/// mean width and the paint the channel's own at each point; erring towards
/// the paint leaves no gap between the two.
#[must_use]
pub fn mesh_share(width: f32, lod: u8, step: f32, morph: f32) -> f32 {
    let resolved = |threshold: f32| smoothstep(threshold, 1.2 * threshold, width);
    let parent = resolved(resolved_width(lod.saturating_add(1), 2.0 * step));
    let here = resolved(resolved_width(lod, step));
    parent.max(here * (1.0 - morph.clamp(0.0, 1.0)))
}

/// How much of a road with a carriageway `width` metres wide the splat
/// still draws at a level with vertices `step` apart, `morph` of the way to
/// its parent: none on a level that leaves the road out ([`way_dropped`]),
/// and below that all of it while two spacings span the carriageway, fading
/// to none at one. The spacing morphs from the level's to its parent's, so
/// the share is continuous where a level meets its parent.
#[must_use]
pub fn splat_share(width: f32, step: f32, morph: f32) -> f32 {
    if step >= width {
        return 0.0;
    }
    let spacing = step * (1.0 + morph.clamp(0.0, 1.0));
    smoothstep(1.0, 2.0, width / spacing)
}

/// Whether a level with vertices `step` metres apart leaves a road of
/// `class` out of its layer shares: its spacing is the carriageway's width
/// or more, so the paint draws it instead ([`splat_share`]).
#[must_use]
pub fn way_dropped(class: RoadClass, step: f64) -> bool {
    step >= f64::from(carriageway_m(class))
}

/// The width of the carriageway a road's stroke carries in its flags,
/// metres (bits 24 to 31): what the shader's [`splat_share`] reads, and so
/// what [`PaintedWays`] drops the splat by, the two agreeing exactly.
#[must_use]
pub fn stroke_carriageway_m(flags: u32) -> f32 {
    (flags >> 24) as f32
}

/// How far past a carriageway's half width the mesher still gives a
/// sample's development to the road the paint draws, metres: the grade
/// breaks the carriageway's edge up over about a metre either side.
pub const WAY_EDGE_M: f32 = 1.5;

/// The roads a region's paint draws because its level is too coarse for
/// the splat to ([`way_dropped`]), for the mesher: a sample on one of them
/// is wild ground in the layer shares, the rock and the tint. Keyed on the
/// painted line itself and not on the sample's material, which a road
/// shares with a site's gravel yard, a settlement's road and a channel's
/// bed; and a line the slot left out keeps its splat, so a road is never
/// dropped from both.
#[derive(Debug, Clone, Default)]
pub struct PaintedWays {
    /// Region-local ends of each stroke, and how far either side of it the
    /// road's ground reaches: half its carriageway and [`WAY_EDGE_M`].
    segments: Vec<([f32; 2], [f32; 2], f32)>,
}

impl PaintedWays {
    /// The road strokes of `paint` whose carriageway a level with vertices
    /// `step` metres apart leaves to the paint.
    #[must_use]
    pub fn of(paint: &RegionPaint, step: f64) -> Self {
        let segments = paint
            .strokes
            .iter()
            .filter(|s| s.kind() == KIND_ROAD)
            .filter_map(|s| {
                let width = stroke_carriageway_m(s.flags);
                (f64::from(width) <= step).then_some((s.a, s.b, 0.5 * width + WAY_EDGE_M))
            })
            .collect();
        Self { segments }
    }

    /// Whether no road is left to the paint here.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// How much of region-local point `p` is the ground of a road the paint
    /// draws: one within its reach, none a metre past.
    #[must_use]
    pub fn share(&self, p: [f32; 2]) -> f32 {
        self.segments
            .iter()
            .map(|&(a, b, reach)| 1.0 - smoothstep(reach, reach + 1.0, segment_distance(p, a, b).0))
            .fold(0.0, f32::max)
    }
}

/// The width of a class's carriageway as the grade lays it, metres: what
/// the splat draws and what decides [`way_dropped`].
#[must_use]
pub fn carriageway_m(class: RoadClass) -> f32 {
    (2.0 * strata_worldgen::sites::way_in_half_m(class)) as f32
}

/// The half width a class is painted at, metres: the carriageway's, and a
/// trail's worn line, which is fully worn only near its middle.
#[must_use]
pub fn painted_half_m(class: RoadClass) -> f32 {
    match class {
        RoadClass::Service | RoadClass::Track => 0.5 * carriageway_m(class),
        RoadClass::Trail => 1.0,
    }
}

/// How opaque a class's paint is: a trail is a worn line, not a laid one.
#[must_use]
pub fn painted_opacity(class: RoadClass) -> f32 {
    match class {
        RoadClass::Service | RoadClass::Track => 1.0,
        RoadClass::Trail => 0.75,
    }
}

/// The share of a pixel `footprint` metres across that a strip `half`
/// metres either side of a line covers, at `distance` metres from the line:
/// the overlap of the pixel's box with the strip, so a strip narrower than
/// a pixel still covers its share of it. The shader's `strip_cover`.
#[must_use]
pub fn strip_cover(distance: f32, half: f32, footprint: f32) -> f32 {
    let footprint = footprint.max(1e-3);
    let lo = (-half).max(distance - 0.5 * footprint);
    let hi = half.min(distance + 0.5 * footprint);
    ((hi - lo) / footprint).clamp(0.0, 1.0)
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One straight piece of a painted line, in metres from its region's
/// minimum corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Its first end.
    pub a: [f32; 2],
    /// Its second end.
    pub b: [f32; 2],
    /// The half width at each end, metres.
    pub half: [f32; 2],
    /// The kind in the low four bits ([`KIND_RIVER`], [`KIND_ROAD`]), then
    /// [`FLAG_GAP`] and [`FLAG_FORD`]; a road's material layer in bits 8 to
    /// 15, its opacity in 255ths in bits 16 to 23, and the width of its
    /// carriageway in metres in bits 24 to 31, which the shader's
    /// [`splat_share`] reads.
    pub flags: u32,
}

impl Stroke {
    /// The stroke's kind.
    #[must_use]
    pub fn kind(&self) -> u32 {
        self.flags & 0xf
    }
}

/// A region's paint before it is packed: its strokes and what was left out
/// for want of room.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionPaint {
    /// The strokes, most important line first.
    pub strokes: Vec<Stroke>,
    /// Lines left out because the slot, or one of its buckets, was full.
    pub dropped: u32,
    /// The most strokes any one bucket lists, at most [`BUCKET_MOST`]: how
    /// near the fullest bucket came to the shader's bound.
    pub fullest: u32,
    /// How many times the level's [`tolerance_m`] the lines were simplified
    /// to: 1, or 2 or 4 where they did not all fit at it.
    pub widened: u8,
}

/// A line gathered from the network, in world metres, before it is
/// simplified to the region's level.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    points: Vec<[f64; 2]>,
    half: Vec<f64>,
    flags: u32,
    /// Lines are packed most important first: a wide one before a narrow.
    weight: f64,
}

/// A run of a network's line whose box can reach some tile.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Piece {
    /// Segments `first..=last` of a reach: from vertex `first` to `last + 1`.
    River { river: u32, first: u32, last: u32 },
    /// Points `first..=last` of a road.
    Road { road: u32, first: u32, last: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Chunk {
    piece: Piece,
    /// `[min_x, min_z, max_x, max_z]`, the line's reach included.
    bounds: [f64; 4],
}

/// The side of a [`NetworkLines`] tile, metres.
const TILE_M: f64 = 1024.0;

/// Segments of a reach in one chunk.
const RIVER_CHUNK: usize = 8;

/// Points of a road in one chunk.
const ROAD_CHUNK: usize = 32;

/// How far a channel's water may stand off its reach's line beyond its
/// meander and half its mean width, metres: a riffle widens the channel by
/// a fraction, and the carve's settling puts it a few metres off.
const RIVER_SLACK_M: f64 = 8.0;

/// One network's lines, chunked and tiled, so a region finds the few that
/// reach it without walking every reach and road of a mass.
#[derive(Debug, Default)]
pub struct NetworkLines {
    chunks: Vec<Chunk>,
    tiles: HashMap<[i32; 2], Vec<u32>>,
}

impl NetworkLines {
    /// The chunks of every reach and road of `network`.
    #[must_use]
    pub fn of(network: &Network) -> Self {
        let mut lines = Self::default();
        for (r, river) in network.rivers().iter().enumerate() {
            let v = &river.vertices;
            for first in (0..v.len().saturating_sub(1)).step_by(RIVER_CHUNK) {
                let last = (first + RIVER_CHUNK - 1).min(v.len() - 2);
                let mut bounds = [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ];
                let mut reach = 0.0_f64;
                for vertex in &v[first..=last + 1] {
                    grow(&mut bounds, [vertex.x, vertex.z]);
                    reach =
                        reach.max(f64::from(vertex.meander_m) + 0.75 * f64::from(vertex.width_m));
                }
                lines.push(Chunk {
                    piece: Piece::River {
                        river: r as u32,
                        first: first as u32,
                        last: last as u32,
                    },
                    bounds: widen(bounds, reach + RIVER_SLACK_M),
                });
            }
        }
        for (r, road) in network.roads().iter().enumerate() {
            let p = &road.points;
            if p.len() < 2 {
                continue;
            }
            let reach = f64::from(painted_half_m(road.class)) + 1.0;
            let mut first = 0;
            while first + 1 < p.len() {
                let last = (first + ROAD_CHUNK).min(p.len() - 1);
                let mut bounds = [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ];
                for point in &p[first..=last] {
                    grow(&mut bounds, [point.x, point.z]);
                }
                lines.push(Chunk {
                    piece: Piece::Road {
                        road: r as u32,
                        first: first as u32,
                        last: last as u32,
                    },
                    bounds: widen(bounds, reach),
                });
                first = last;
            }
        }
        lines
    }

    fn push(&mut self, chunk: Chunk) {
        let id = self.chunks.len() as u32;
        let [x0, z0, x1, z1] = chunk.bounds.map(|v| (v / TILE_M).floor() as i32);
        for tz in z0..=z1 {
            for tx in x0..=x1 {
                self.tiles.entry([tx, tz]).or_default().push(id);
            }
        }
        self.chunks.push(chunk);
    }

    /// The pieces whose box meets `bounds`, each once, in chunk order.
    fn pieces_in(&self, bounds: [f64; 4]) -> Vec<Piece> {
        let [x0, z0, x1, z1] = bounds.map(|v| (v / TILE_M).floor() as i32);
        let mut ids: Vec<u32> = Vec::new();
        for tz in z0..=z1 {
            for tx in x0..=x1 {
                if let Some(list) = self.tiles.get(&[tx, tz]) {
                    ids.extend_from_slice(list);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .map(|id| self.chunks[id as usize])
            .filter(|chunk| overlaps(chunk.bounds, bounds))
            .map(|chunk| chunk.piece)
            .collect()
    }
}

fn grow(bounds: &mut [f64; 4], p: [f64; 2]) {
    bounds[0] = bounds[0].min(p[0]);
    bounds[1] = bounds[1].min(p[1]);
    bounds[2] = bounds[2].max(p[0]);
    bounds[3] = bounds[3].max(p[1]);
}

fn widen(bounds: [f64; 4], by: f64) -> [f64; 4] {
    [
        bounds[0] - by,
        bounds[1] - by,
        bounds[2] + by,
        bounds[3] + by,
    ]
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Every network's [`NetworkLines`], built once on first use by whichever
/// task asks first and shared by all: a network is baked once and kept, so
/// its lines never change.
#[derive(Debug, Default)]
pub struct PaintCache {
    lines: Mutex<HashMap<[i32; 2], Arc<OnceLock<NetworkLines>>>>,
}

impl PaintCache {
    /// The lines of `network`, built now if no task has.
    pub fn lines(&self, network: &Network) -> Arc<OnceLock<NetworkLines>> {
        let cell = {
            let mut map = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(map.entry(network.key()).or_default())
        };
        cell.get_or_init(|| NetworkLines::of(network));
        cell
    }
}

/// What a region's paint is gathered with: the world whose network it
/// reads, the cache of its lines and the layers the roads' classes are laid
/// in, `[service, track, trail]`.
#[derive(Debug, Clone)]
pub struct PaintSource {
    /// The world.
    pub world: Arc<ProceduralWorld>,
    /// The lines of its networks.
    pub cache: Arc<PaintCache>,
    /// The texture layer of each class's carriageway, or `None` where the
    /// registry has no such material: the class is then not painted.
    pub ways: [Option<u8>; 3],
}

/// Which `[service, track, trail]` slot a class is.
#[must_use]
pub fn class_slot(class: RoadClass) -> usize {
    match class {
        RoadClass::Service => 0,
        RoadClass::Track => 1,
        RoadClass::Trail => 2,
    }
}

/// The narrowest line a level with vertices `step` apart paints, metres. A
/// region of a level is selected only from two and a half of its sides
/// away (the stream's `split_factor`), 160 of its spacings, where a pixel
/// of a 45 degree view 1 440 pixels high is about an eleventh of a spacing:
/// this is about a quarter of a pixel at the region's nearest, and a
/// narrower line would cover a few hundredths of a pixel for the cost of
/// its strokes.
#[must_use]
pub fn narrowest_m(step: f64) -> f64 {
    step / 40.0
}

/// How far a simplified line may stand off the network's, metres: a
/// thirty-second of the level's spacing, about a third of a pixel at the
/// region's nearest ([`narrowest_m`] has the arithmetic). A meander whose
/// swing is near the tolerance keeps only its peaks and draws as a
/// sawtooth, so the tolerance is held well under the pixel the swing is
/// seen at; the two sides of a level's border then differ by under a
/// pixel too.
#[must_use]
pub fn tolerance_m(step: f64) -> f64 {
    (step / 32.0).max(0.1)
}

/// The paint of region `coord`: the lines of every network that reaches it,
/// simplified to its level, most important first, as many as its slot
/// holds.
#[must_use]
pub fn region_paint(coord: RegionCoord, source: &PaintSource) -> RegionPaint {
    let lines = region_lines(coord, source);
    fit(coord, lines)
}

/// The lines that reach `coord`'s square, in world metres, before they are
/// simplified: each reach's channel as its centreline winds, each road's
/// centreline split at its crossings.
fn region_lines(coord: RegionCoord, source: &PaintSource) -> Vec<Line> {
    let world = &*source.world;
    let step = coord.vertex_spacing();
    let min = coord.min_corner();
    let size = coord.size();
    let margin = step + 2.0;
    let bounds = [
        min.x() - margin,
        min.z() - margin,
        min.x() + size + margin,
        min.z() + size + margin,
    ];
    let narrowest = narrowest_m(step);
    let hydrology = world.hydrology();
    let params = *hydrology.params();
    let networks = hydrology.bake_area_on(world, (bounds[0], bounds[1]), (bounds[2], bounds[3]), 1);
    let mut out = Vec::new();
    for network in &networks {
        let cell = source.cache.lines(network);
        let Some(lines) = cell.get() else {
            continue;
        };
        let pieces = lines.pieces_in(bounds);
        // Runs of each reach's segments and each road's points, merged where
        // two chunks meet.
        let mut rivers: Vec<(u32, u32, u32)> = Vec::new();
        let mut roads: Vec<(u32, u32, u32)> = Vec::new();
        for piece in pieces {
            let (runs, (id, first, last)) = match piece {
                Piece::River { river, first, last } => (&mut rivers, (river, first, last)),
                Piece::Road { road, first, last } => (&mut roads, (road, first, last)),
            };
            match runs.last_mut() {
                Some(run) if run.0 == id && first <= run.2 + 1 => run.2 = run.2.max(last),
                _ => runs.push((id, first, last)),
            }
        }
        for (r, first, last) in rivers {
            let river = &network.rivers()[r as usize];
            if let Some(line) = river_line(river, &params, (first as usize, last as usize), step)
                && line.weight >= narrowest
            {
                out.push(line);
            }
        }
        for (r, first, last) in roads {
            let road = &network.roads()[r as usize];
            // An authored settlement's own road is the settlement's laid
            // ground, which the grade leaves alone and the splat draws at
            // every level in the settlement's material; painted in its
            // class's, it drew twice in two materials.
            if road.is_settlements() {
                continue;
            }
            let Some(layer) = source.ways[class_slot(road.class)] else {
                continue;
            };
            if f64::from(2.0 * painted_half_m(road.class)) < narrowest {
                continue;
            }
            out.extend(road_lines(road, layer, (first as usize, last as usize)));
        }
    }
    out
}

/// A reach's channel over segments `first..=last` as its centreline winds
/// (`River::centreline`), sampled finely enough for its meanders: the
/// reach's vertices one either side of the run come along, so every point
/// is the one the whole reach's centreline has there.
fn river_line(
    river: &River,
    params: &RiverParams,
    (first, last): (usize, usize),
    step: f64,
) -> Option<Line> {
    let v = &river.vertices;
    let n = v.len();
    if n < 2 || first > last || last + 1 >= n {
        return None;
    }
    let from = first.saturating_sub(1);
    let to = (last + 2).min(n - 1);
    let narrowest = v[first..=last + 1]
        .iter()
        .map(|vertex| f64::from(vertex.width_m))
        .fold(f64::INFINITY, f64::min);
    // The meanders' wavelength is ten to fourteen widths: eight points a
    // wavelength or better, and never finer than the region needs.
    let sample = (1.25 * narrowest).min(0.5 * step).clamp(1.0, 64.0);
    let sub = River {
        vertices: v[from..=to].to_vec(),
        order: river.order,
        end: river.end,
        from_lake: river.from_lake,
    };
    // Where the run starts and ends along the sub-reach, as `centreline`
    // measures it.
    let mut along = 0.0;
    let (mut start, mut end) = (0.0, 0.0);
    for (k, pair) in sub.vertices.windows(2).enumerate() {
        if from + k == first {
            start = along;
        }
        let (dx, dz) = (pair[1].x - pair[0].x, pair[1].z - pair[0].z);
        along += (dx * dx + dz * dz).sqrt();
        if from + k == last {
            end = along;
        }
    }
    let mut line = Line {
        points: Vec::new(),
        half: Vec::new(),
        flags: KIND_RIVER,
        weight: 0.0,
    };
    for point in sub.centreline(params, sample) {
        if point.along_m < start - 1e-6 || point.along_m > end + 1e-6 {
            continue;
        }
        line.points.push([point.x, point.z]);
        line.half.push(0.5 * point.width_m);
        line.weight = line.weight.max(point.width_m);
    }
    (line.points.len() >= 2).then_some(line)
}

/// A road's centreline over points `first..=last`, split where it meets a
/// bridge's abutments or a ford's wetted edges into lines flagged
/// [`FLAG_GAP`] and [`FLAG_FORD`] and lines of plain road.
fn road_lines(
    road: &strata_worldgen::roads::Road,
    layer: u8,
    (first, last): (usize, usize),
) -> Vec<Line> {
    let p = &road.points;
    let last = last.min(p.len().saturating_sub(1));
    if last <= first {
        return Vec::new();
    }
    let half = f64::from(painted_half_m(road.class));
    let opacity = (painted_opacity(road.class) * 255.0).round() as u32;
    let width = carriageway_m(road.class).round().clamp(0.0, 255.0) as u32;
    let base = KIND_ROAD | (u32::from(layer) << 8) | (opacity << 16) | (width << 24);
    // The stretches along the road that are not plain road.
    let marked: Vec<(f64, f64, u32)> = road
        .crossings
        .iter()
        .filter_map(|c| {
            let flag = match c.kind {
                CrossingKind::Bridge => FLAG_GAP,
                CrossingKind::Ford => FLAG_FORD,
                CrossingKind::Culvert => return None,
            };
            let [a, b] = c.abutments_m;
            Some((a.min(b), a.max(b), flag))
        })
        .collect();
    let flag_at = |s: f64| {
        marked
            .iter()
            .find(|(a, b, _)| s > *a && s < *b)
            .map_or(0, |m| m.2)
    };
    let mut out: Vec<Line> = Vec::new();
    for k in first..last {
        let (a, b) = (&p[k], &p[k + 1]);
        // Cuts inside this segment, in order along it.
        let mut cuts: Vec<f64> = marked
            .iter()
            .flat_map(|(s0, s1, _)| [*s0, *s1])
            .filter(|s| *s > a.along_m && *s < b.along_m)
            .collect();
        cuts.sort_by(f64::total_cmp);
        let span = (b.along_m - a.along_m).max(1e-9);
        let at = |s: f64| {
            let t = ((s - a.along_m) / span).clamp(0.0, 1.0);
            [a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t]
        };
        let mut s = a.along_m;
        if k == first {
            let next = cuts.first().copied().unwrap_or(b.along_m);
            out.push(Line {
                points: vec![[a.x, a.z]],
                half: vec![half],
                flags: base | flag_at(f64::midpoint(s, next)),
                weight: 2.0 * half,
            });
        }
        for cut in cuts.into_iter().chain(std::iter::once(b.along_m)) {
            let flags = base | flag_at(f64::midpoint(s, cut));
            extend(&mut out, at(cut), half, flags);
            s = cut;
        }
    }
    out.retain(|line| line.points.len() >= 2);
    out
}

/// Adds `point` to the last line if it has `flags`, or starts a new line of
/// `flags` from where the last one ends, so the two meet.
fn extend(out: &mut Vec<Line>, point: [f64; 2], half: f64, flags: u32) {
    match out.last() {
        Some(line) if line.flags == flags => {}
        last => {
            let start = last.map(|line| line.points[line.points.len() - 1]);
            out.push(Line {
                points: start.into_iter().collect(),
                half: start.map(|_| half).into_iter().collect(),
                flags,
                weight: 2.0 * half,
            });
        }
    }
    if let Some(line) = out.last_mut() {
        line.points.push(point);
        line.half.push(half);
    }
}

/// The points of `points` Douglas and Peucker keep at `tolerance`: the
/// ends, and every point further than that from the chord of the points
/// kept either side of it, by index.
#[must_use]
pub fn simplify(points: &[[f64; 2]], tolerance: f64) -> Vec<usize> {
    let n = points.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    while let Some((i, j)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (mut worst, mut at) = (0.0, i);
        for k in i + 1..j {
            let d = distance_to_segment(points[k], points[i], points[j]);
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > tolerance {
            keep[at] = true;
            stack.push((i, at));
            stack.push((at, j));
        }
    }
    (0..n).filter(|&k| keep[k]).collect()
}

fn distance_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dz * dz;
    let t = if length2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ez) = (p[0] - a[0] - dx * t, p[1] - a[1] - dz * t);
    (ex * ex + ez * ez).sqrt()
}

/// How far past a stroke's half width its buckets reach, metres: the
/// fragment's footprint across it, which the shader holds under this.
#[must_use]
pub fn bucket_margin_m(step: f32) -> f32 {
    step + 1.0
}

/// Simplifies `lines` to `coord`'s level and keeps as many as its slot
/// holds, widest first. A region whose lines do not all fit at the level's
/// tolerance tries twice and four times it, which still holds them within
/// a pixel and a half at the region's nearest, before it leaves the
/// narrowest out.
fn fit(coord: RegionCoord, mut lines: Vec<Line>) -> RegionPaint {
    lines.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let tolerance = tolerance_m(coord.vertex_spacing());
    let mut best = fit_at(coord, &lines, tolerance);
    best.widened = 1;
    for widen in [2_u8, 4] {
        if best.dropped == 0 {
            break;
        }
        let mut wider = fit_at(coord, &lines, tolerance * f64::from(widen));
        wider.widened = widen;
        if wider.dropped < best.dropped {
            best = wider;
        }
    }
    best
}

/// [`fit`] at one `tolerance`, the lines already in order: a line goes in
/// whole or not at all, and only while the slot has room for its entries
/// and strokes and no bucket would list more than [`BUCKET_MOST`].
fn fit_at(coord: RegionCoord, lines: &[Line], tolerance: f64) -> RegionPaint {
    let step = coord.vertex_spacing();
    let min = coord.min_corner();
    let size = coord.size() as f32;
    let margin = bucket_margin_m(step as f32);
    let mut paint = RegionPaint::default();
    let mut entries = 0usize;
    let mut listed = [0_u16; PAINT_GRID * PAINT_GRID];
    let mut touched: Vec<usize> = Vec::new();
    for line in lines {
        let kept = simplify(&line.points, tolerance);
        let mut strokes = Vec::with_capacity(kept.len());
        touched.clear();
        for pair in kept.windows(2) {
            let (i, j) = (pair[0], pair[1]);
            let local = |k: usize| {
                [
                    (line.points[k][0] - min.x()) as f32,
                    (line.points[k][1] - min.z()) as f32,
                ]
            };
            let stroke = Stroke {
                a: local(i),
                b: local(j),
                half: [line.half[i] as f32, line.half[j] as f32],
                flags: line.flags,
            };
            let reach = stroke.half[0].max(stroke.half[1]) + margin;
            let buckets = buckets_of(&stroke, size, reach);
            if !buckets.is_empty() {
                touched.extend_from_slice(&buckets);
                strokes.push(stroke);
            }
        }
        if strokes.is_empty() {
            continue;
        }
        let words = ENTRY_WORD
            + (entries + touched.len()).div_ceil(2)
            + STROKE_WORDS * (paint.strokes.len() + strokes.len());
        let mut over =
            words > PAINT_WORDS || paint.strokes.len() + strokes.len() > usize::from(u16::MAX);
        if !over {
            for &cell in &touched {
                listed[cell] += 1;
            }
            over = listed.iter().any(|&n| usize::from(n) > BUCKET_MOST);
            if over {
                for &cell in &touched {
                    listed[cell] -= 1;
                }
            }
        }
        if over {
            paint.dropped += 1;
            continue;
        }
        entries += touched.len();
        paint.strokes.extend(strokes);
    }
    paint.fullest = listed.iter().copied().map(u32::from).max().unwrap_or(0);
    paint
}

/// The buckets of a region `size` metres a side that `stroke` reaches
/// within `reach` metres, row-major indices: those whose square, half a
/// metre of rounding round it, the stroke passes within `reach` of. The
/// fragment reads every stroke its bucket lists, so a bucket listing a
/// stroke that cannot cover it costs a loop for nothing.
fn buckets_of(stroke: &Stroke, size: f32, reach: f32) -> Vec<usize> {
    let cell = size / PAINT_GRID as f32;
    let lo = [
        stroke.a[0].min(stroke.b[0]) - reach,
        stroke.a[1].min(stroke.b[1]) - reach,
    ];
    let hi = [
        stroke.a[0].max(stroke.b[0]) + reach,
        stroke.a[1].max(stroke.b[1]) + reach,
    ];
    let last = (PAINT_GRID - 1) as f32;
    if hi[0] < 0.0 || hi[1] < 0.0 || lo[0] > size || lo[1] > size {
        return Vec::new();
    }
    let range = |lo: f32, hi: f32| {
        let a = (lo / cell).floor().clamp(0.0, last) as usize;
        let b = (hi / cell).floor().clamp(0.0, last) as usize;
        a..=b
    };
    let mut out = Vec::new();
    for j in range(lo[1], hi[1]) {
        for i in range(lo[0], hi[0]) {
            let (x0, x1) = (i as f32 * cell - 0.5, (i + 1) as f32 * cell + 0.5);
            let (z0, z1) = (j as f32 * cell - 0.5, (j + 1) as f32 * cell + 0.5);
            if segment_box_distance(stroke.a, stroke.b, [x0, z0], [x1, z1]) <= reach {
                out.push(j * PAINT_GRID + i);
            }
        }
    }
    out
}

/// The distance from the segment `a` to `b` to the box `lo` to `hi`, nought
/// where they meet: in the plane the nearest pair is an end of the segment
/// and the box, or a corner of the box and the segment.
fn segment_box_distance(a: [f32; 2], b: [f32; 2], lo: [f32; 2], hi: [f32; 2]) -> f32 {
    let inside = |p: [f32; 2]| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1];
    if inside(a) || inside(b) || segment_crosses_box(a, b, lo, hi) {
        return 0.0;
    }
    let to_box = |p: [f32; 2]| {
        let dx = (lo[0] - p[0]).max(p[0] - hi[0]).max(0.0);
        let dz = (lo[1] - p[1]).max(p[1] - hi[1]).max(0.0);
        (dx * dx + dz * dz).sqrt()
    };
    let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
    corners
        .iter()
        .map(|&c| segment_distance(c, a, b).0)
        .fold(to_box(a).min(to_box(b)), f32::min)
}

/// Whether the segment `a` to `b` passes through the box `lo` to `hi`
/// (Liang and Barsky's clip).
fn segment_crosses_box(a: [f32; 2], b: [f32; 2], lo: [f32; 2], hi: [f32; 2]) -> bool {
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for axis in 0..2 {
        let d = b[axis] - a[axis];
        if d.abs() < 1e-12 {
            if a[axis] < lo[axis] || a[axis] > hi[axis] {
                return false;
            }
            continue;
        }
        let (mut near, mut far) = ((lo[axis] - a[axis]) / d, (hi[axis] - a[axis]) / d);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// The distance from `p` to the segment `a` to `b` and how far along it
/// the nearest point lies, `0..=1`: the shader's reading.
#[must_use]
pub fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if length2 > 0.0 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let e = [ap[0] - ab[0] * t, ap[1] - ab[1] * t];
    ((e[0] * e[0] + e[1] * e[1]).sqrt(), t)
}

/// The words a region's paint writes: the module docs' layout, as long as
/// it needs (one word where there is nothing to paint), never past
/// [`PAINT_WORDS`].
#[must_use]
pub fn pack_paint(paint: &RegionPaint, coord: RegionCoord) -> Vec<u32> {
    if paint.strokes.is_empty() {
        return vec![0];
    }
    let size = coord.size() as f32;
    let margin = bucket_margin_m(coord.vertex_spacing() as f32);
    let mut lists: Vec<Vec<u16>> = vec![Vec::new(); PAINT_GRID * PAINT_GRID];
    for (s, stroke) in paint.strokes.iter().enumerate() {
        let reach = stroke.half[0].max(stroke.half[1]) + margin;
        for cell in buckets_of(stroke, size, reach) {
            lists[cell].push(s as u16);
        }
    }
    let entries: usize = lists.iter().map(Vec::len).sum();
    let stroke_word = ENTRY_WORD + entries.div_ceil(2);
    let words = stroke_word + STROKE_WORDS * paint.strokes.len();
    debug_assert!(
        words <= PAINT_WORDS,
        "the fit keeps a region's paint in its slot"
    );
    let mut out = vec![0_u32; words.min(PAINT_WORDS)];
    out[0] = paint.strokes.len() as u32 | ((stroke_word as u32) << 16);
    out[1] = entries as u32;
    let mut next = 0usize;
    let mut flat: Vec<u16> = Vec::with_capacity(entries);
    for (cell, list) in lists.iter().enumerate() {
        out[HEADER_WORDS + cell] = next as u32 | ((list.len() as u32) << 16);
        next += list.len();
        flat.extend_from_slice(list);
    }
    for (k, pair) in flat.chunks(2).enumerate() {
        let hi = pair.get(1).copied().unwrap_or(0);
        out[ENTRY_WORD + k] = u32::from(pair[0]) | (u32::from(hi) << 16);
    }
    for (s, stroke) in paint.strokes.iter().enumerate() {
        let at = stroke_word + s * STROKE_WORDS;
        if at + STROKE_WORDS > out.len() {
            break;
        }
        out[at] = stroke.a[0].to_bits();
        out[at + 1] = stroke.a[1].to_bits();
        out[at + 2] = stroke.b[0].to_bits();
        out[at + 3] = stroke.b[1].to_bits();
        out[at + 4] = crate::store::pack_half2(stroke.half[0], stroke.half[1]);
        out[at + 5] = stroke.flags;
    }
    out
}

/// What the paint covers at a point, as the shader reads it, before the
/// near drawing's shares and the deck's range: the most of any river's
/// stroke and of any road's (fords and gaps included), for the tests.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Covered {
    /// The share of the pixel a river's water covers.
    pub water: f32,
    /// The share a road's carriageway covers, times its opacity.
    pub road: f32,
    /// The flags of the road's stroke that covers most.
    pub road_flags: u32,
}

/// The CPU mirror of the shader's lookup: region-local point `p` of a
/// region `size` metres a side, its paint `words`, and a pixel `footprint`
/// metres across in every direction.
#[must_use]
pub fn covered_at(words: &[u32], size: f32, p: [f32; 2], footprint: f32) -> Covered {
    let mut out = Covered::default();
    let head = words.first().copied().unwrap_or(0);
    let strokes = head & 0xffff;
    if strokes == 0 {
        return out;
    }
    let stroke_word = (head >> 16) as usize;
    let cell = |v: f32| {
        ((v / size * PAINT_GRID as f32)
            .floor()
            .clamp(0.0, (PAINT_GRID - 1) as f32)) as usize
    };
    let bucket = words[HEADER_WORDS + cell(p[1]) * PAINT_GRID + cell(p[0])];
    let (start, count) = ((bucket & 0xffff) as usize, (bucket >> 16) as usize);
    for e in start..start + count {
        let word = words[ENTRY_WORD + e / 2];
        let s = if e % 2 == 0 {
            word & 0xffff
        } else {
            word >> 16
        } as usize;
        let at = stroke_word + s * STROKE_WORDS;
        let a = [f32::from_bits(words[at]), f32::from_bits(words[at + 1])];
        let b = [f32::from_bits(words[at + 2]), f32::from_bits(words[at + 3])];
        let [ha, hb] = crate::store::unpack_half2(words[at + 4]);
        let flags = words[at + 5];
        let (d, t) = segment_distance(p, a, b);
        let cover = strip_cover(d, ha + (hb - ha) * t, footprint);
        if flags & 0xf == KIND_RIVER {
            out.water = out.water.max(cover);
        } else {
            let cover = cover * ((flags >> 16) & 0xff) as f32 / 255.0;
            if cover > out.road {
                out.road = cover;
                out.road_flags = flags;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use strata_core::Seed;
    use strata_worldgen::WorldSource;

    use super::*;

    fn line(points: &[[f64; 2]], half: f64, flags: u32) -> Line {
        Line {
            points: points.to_vec(),
            half: vec![half; points.len()],
            flags,
            weight: 2.0 * half,
        }
    }

    #[test]
    fn a_strip_covers_its_share_of_a_pixel() {
        // Wider than the pixel: whole inside, none well outside, half at the edge.
        assert!((strip_cover(0.0, 4.0, 1.0) - 1.0).abs() < 1e-6);
        assert!(strip_cover(6.0, 4.0, 1.0).abs() < 1e-6);
        assert!((strip_cover(4.0, 4.0, 1.0) - 0.5).abs() < 1e-6);
        // Narrower: its width over the pixel's, wherever the pixel holds it.
        assert!((strip_cover(0.0, 1.0, 8.0) - 0.25).abs() < 1e-6);
        assert!((strip_cover(2.0, 1.0, 8.0) - 0.25).abs() < 1e-6);
        assert!(strip_cover(6.0, 1.0, 8.0).abs() < 1e-6);
    }

    #[test]
    fn a_segment_measures_to_a_box() {
        let (lo, hi) = ([0.0, 0.0], [10.0, 10.0]);
        // Through it, and with an end inside.
        assert!(segment_box_distance([-5.0, 5.0], [15.0, 5.0], lo, hi).abs() < 1e-6);
        assert!(segment_box_distance([5.0, 5.0], [50.0, 50.0], lo, hi).abs() < 1e-6);
        // Beside a side, past a corner, and a diagonal past a corner.
        assert!((segment_box_distance([-3.0, -20.0], [-3.0, 30.0], lo, hi) - 3.0).abs() < 1e-5);
        assert!((segment_box_distance([13.0, 14.0], [20.0, 14.0], lo, hi) - 5.0).abs() < 1e-5);
        let d = segment_box_distance([13.0, 9.0], [9.0, 13.0], lo, hi);
        assert!((d - 2.0_f32.sqrt()).abs() < 1e-5, "{d}");
        // A point segment.
        assert!((segment_box_distance([-4.0, 5.0], [-4.0, 5.0], lo, hi) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn simplifying_keeps_the_ends_and_holds_the_tolerance() {
        let points: Vec<[f64; 2]> = (0..200)
            .map(|k| {
                let x = f64::from(k);
                [x, 6.0 * (x / 20.0).sin()]
            })
            .collect();
        for tolerance in [0.1, 1.0, 4.0] {
            let kept = simplify(&points, tolerance);
            assert_eq!(kept[0], 0);
            assert_eq!(*kept.last().expect("kept"), 199);
            assert!(kept.len() < points.len());
            for pair in kept.windows(2) {
                for k in pair[0]..pair[1] {
                    let d = distance_to_segment(points[k], points[pair[0]], points[pair[1]]);
                    assert!(d <= tolerance + 1e-9, "{d} over {tolerance}");
                }
            }
        }
        // A straight line keeps its ends alone.
        let straight: Vec<[f64; 2]> = (0..50).map(|k| [f64::from(k), 0.0]).collect();
        assert_eq!(simplify(&straight, 0.01), vec![0, 49]);
    }

    /// A river and a road across a region, packed: the fragment's lookup
    /// finds each where it runs, at its width, and nothing between.
    #[test]
    fn packed_lines_are_found_where_they_run() {
        let coord = RegionCoord::new(0, 0, 6);
        let size = coord.size() as f32;
        let river = line(&[[-100.0, 1000.0], [5000.0, 1000.0]], 10.0, KIND_RIVER);
        let road_flags = KIND_ROAD | (3 << 8) | (255 << 16) | (8 << 24);
        let road = line(&[[2000.0, -50.0], [2000.0, 5000.0]], 4.0, road_flags);
        let paint = fit(coord, vec![road, river]);
        assert_eq!(paint.strokes.len(), 2);
        assert_eq!(paint.dropped, 0);
        // The wider line first.
        assert_eq!(paint.strokes[0].kind(), KIND_RIVER);
        let words = pack_paint(&paint, coord);
        assert!(words.len() <= PAINT_WORDS);
        let at = |x: f32, z: f32| covered_at(&words, size, [x, z], 1.0);
        assert!((at(300.0, 1000.0).water - 1.0).abs() < 1e-6);
        assert!((at(300.0, 1009.0).water - 1.0).abs() < 1e-6);
        assert!(at(300.0, 1012.0).water.abs() < 1e-6);
        assert!(at(300.0, 1012.0).road.abs() < 1e-6);
        assert!((at(2000.0, 3000.0).road - 1.0).abs() < 1e-6);
        assert_eq!(at(2000.0, 3000.0).road_flags, road_flags);
        assert!(at(2006.0, 3000.0).road.abs() < 1e-6);
        // Where they cross, both.
        let both = at(2000.0, 1000.0);
        assert!(both.water > 0.99 && both.road > 0.99);
        // Far from both, every bucket that holds neither is empty.
        assert_eq!(at(3500.0, 3500.0), Covered::default());
        // An empty region writes one word.
        assert_eq!(pack_paint(&RegionPaint::default(), coord), vec![0]);
    }

    /// More lines than a slot holds: the widest are kept, the rest counted,
    /// and what is packed fits.
    #[test]
    fn a_full_slot_keeps_the_widest_lines() {
        let coord = RegionCoord::new(0, 0, 4);
        let size = coord.size();
        // Zigzags no tolerance the fit tries flattens, a hundred of them.
        let lines: Vec<Line> = (0..100)
            .map(|k| {
                let z = f64::from(k) * size / 100.0;
                let points: Vec<[f64; 2]> = (0..=40)
                    .map(|i| {
                        let x = f64::from(i) * size / 40.0;
                        [x, z + if i % 2 == 0 { 0.0 } else { 40.0 }]
                    })
                    .collect();
                line(&points, 1.0 + f64::from(k) * 0.01, KIND_RIVER)
            })
            .collect();
        let paint = fit(coord, lines);
        assert!(paint.dropped > 0);
        let words = pack_paint(&paint, coord);
        assert!(words.len() <= PAINT_WORDS);
        // The widest line (the last made) is in.
        let widest = 1.0 + 0.99_f32;
        assert!(
            paint
                .strokes
                .iter()
                .any(|s| (s.half[0] - widest).abs() < 1e-4)
        );
    }

    #[test]
    fn the_mesh_and_the_paint_share_a_river_between_them() {
        // A river the level and its parent both draw: no paint.
        assert!((mesh_share(40.0, 2, 4.0, 0.5) - 1.0).abs() < 1e-6);
        // One only this level draws: it fades out with the morph.
        assert!((mesh_share(10.0, 2, 4.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((mesh_share(10.0, 2, 4.0, 0.75) - 0.25).abs() < 1e-6);
        // One no level here draws: all paint.
        assert!(mesh_share(5.0, 2, 4.0, 0.0).abs() < 1e-6);
        // The finest level draws anything a spacing wide.
        assert!((mesh_share(1.5, 0, 1.0, 0.0) - 1.0).abs() < 1e-6);
        // Where a level meets its parent the share is the same from both:
        // the fine level fully morphed, the parent unmorphed.
        for width in [3.0, 5.0, 9.0, 17.0, 33.0] {
            let fine = mesh_share(width, 2, 4.0, 1.0);
            let coarse = mesh_share(width, 3, 8.0, 0.0);
            assert!(
                (fine - coarse).abs() < 1e-6,
                "{width}: {fine} against {coarse}"
            );
        }
    }

    #[test]
    fn the_splat_hands_a_road_to_the_paint_without_a_step() {
        for class in [RoadClass::Service, RoadClass::Track, RoadClass::Trail] {
            let width = carriageway_m(class);
            for lod in 0..6_u8 {
                let step = RegionCoord::size_at(lod) / 64.0;
                let fine = splat_share(width, step as f32, 1.0);
                let coarse = splat_share(width, 2.0 * step as f32, 0.0);
                assert!(
                    (fine - coarse).abs() < 1e-6,
                    "{class:?} at lod {lod}: {fine} against {coarse}"
                );
                // The mesher leaves the road out exactly where the splat
                // would draw none.
                assert_eq!(
                    way_dropped(class, step),
                    splat_share(width, step as f32, 0.0) == 0.0
                );
            }
        }
        // Near, the splat draws a service road whole.
        assert!((splat_share(8.0, 1.0, 0.0) - 1.0).abs() < 1e-6);
    }

    /// The showcase's crossing on seed 42: a region round the bridge at
    /// (7 404, -4 940) paints its river where the ground's channel is and
    /// its service road, the road's stroke over the gap flagged so the
    /// shader leaves it to the deck.
    #[test]
    fn the_showcase_paints_its_river_and_stops_its_road_at_the_deck() {
        let registries = strata_registry::load_from_dir(crate::material::dev_asset_root())
            .expect("the shipped assets load");
        let world = Arc::new(ProceduralWorld::new(Seed(42), &registries));
        let source = PaintSource {
            world,
            cache: Arc::new(PaintCache::default()),
            ways: [Some(1), Some(2), Some(3)],
        };
        // A level-5 region (2 km, 32 m spacing) holding the bridge.
        let lod = 5;
        let size = RegionCoord::size_at(lod);
        let coord = RegionCoord::new(
            (7_404.0 / size).floor() as i32,
            (-4_940.0 / size).floor() as i32,
            lod,
        );
        let paint = region_paint(coord, &source);
        assert_eq!(paint.dropped, 0);
        assert!(paint.strokes.iter().any(|s| s.kind() == KIND_RIVER));
        let service = |s: &&Stroke| s.kind() == KIND_ROAD && (s.flags >> 8) & 0xff == 1;
        assert!(paint.strokes.iter().filter(service).count() > 1);
        let min = coord.min_corner();
        let bridge = [(7_404.0 - min.x()) as f32, (-4_940.0 - min.z()) as f32];
        let gap = paint
            .strokes
            .iter()
            .filter(service)
            .filter(|s| s.flags & FLAG_GAP != 0)
            .map(|s| segment_distance(bridge, s.a, s.b).0)
            .fold(f32::INFINITY, f32::min);
        assert!(gap < 40.0, "the gap's stroke is {gap} m from the bridge");
        // Packed and read back against the ground: on a grid round the
        // bridge, the paint covers the sample's own channel and nothing
        // outside it past the tolerance and the pixel's half.
        let words = pack_paint(&paint, coord);
        let tolerance = tolerance_m(coord.vertex_spacing()) as f32;
        let (mut inside, mut covered, mut outside, mut stray) = (0, 0, 0, 0);
        let mut worst_stray = (0.0_f32, [0.0_f32; 2]);
        for j in -75..=75 {
            for i in -75..=75 {
                let (x, z) = (7_404.0 + 4.0 * f64::from(i), -4_940.0 + 4.0 * f64::from(j));
                let local = [(x - min.x()) as f32, (z - min.z()) as f32];
                let water = covered_at(&words, size as f32, local, 1.0).water;
                match source.world.sample(x, z).water {
                    Some(w) if w.kind == strata_worldgen::hydrology::WaterKind::Lake => {}
                    Some(w) => {
                        let off = w.signed_distance_m.abs() - 0.5 * w.width_m;
                        if off < -(tolerance + 1.0) {
                            inside += 1;
                            covered += usize::from(water > 0.5);
                        } else if off > tolerance + 1.5 {
                            outside += 1;
                            if water > 0.0 {
                                stray += 1;
                                if off > worst_stray.0 {
                                    worst_stray = (off, local);
                                }
                            }
                        }
                    }
                    None => {
                        outside += 1;
                        stray += usize::from(water > 0.0);
                    }
                }
            }
        }
        println!(
            "channel: {covered} of {inside} painted; outside: {stray} of {outside} painted, worst {worst_stray:?}"
        );
        assert!(inside > 200, "the grid holds the channel ({inside} points)");
        assert!(
            covered * 100 >= inside * 98,
            "{covered} of {inside} channel points painted"
        );
        assert_eq!(
            stray, 0,
            "{stray} of {outside} points off the channel painted"
        );
    }
}
