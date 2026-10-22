//! The baked cache as files, for a platform that cannot afford to bake.
//!
//! Two layouts share one record. [`Hydrology::save`] writes the whole cache
//! as one file, which only an empty cache takes back ([`Hydrology::load`]).
//! [`Hydrology::save_masses`] writes one file a land mass and a small
//! [`WaterIndex`], and [`Hydrology::load_mass`] merges one mass into a cache
//! that holds others, so a start reads only the masses it will sample, the
//! browser fetches the rest as the observer nears them, and a mass whose
//! file is missing is baked as it always was (ticket 0015, step 3; review
//! 2026-10-16, F6). What the two layouts must agree on is the labels: a
//! tile's label is an index into the cache's own list of networks, so a file
//! carries no label, only the nodes its mass claims, and a merge gives them
//! the index the network lands on, exactly as a bake does.
//!
//! What a file weighs is the road points and the reach vertices (a big mass
//! is two thirds roads and a quarter rivers), so those are stored as columns
//! of `f64` and `f32` words, each word as its miss from the straight line
//! through the two before it, byte planes side by side ([`planes`]): lossless,
//! and the gzip of the bundle folds the high planes. A mass is still one
//! file and one network: a network's indexes, its junctions and its sites
//! name its roads and reaches by position, and the sample, the placement and
//! the world's version all read a mass whole, so a file a field tile would
//! hand a sample a mass with holes.
//!
//! A browser runs the bake on one thread, and seed 42's home mass alone is
//! half a minute of it (ticket 0015, step 3). The same bake run natively
//! writes its networks and the tiles' labels here; reading them back is
//! the cost of decoding, plus the field's tiles (their heights are
//! recomputed, a second or so on one thread), and the cache then holds
//! exactly what the bake left. Nothing that is baked changes: a network
//! is written whole, its indexes (derived from it) are rebuilt by the code
//! that builds them in a bake, and a file that does not belong to this
//! world is refused, so the caller bakes as it always did.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use strata_core::Seed;

use super::index::{SegmentIndex, Shore};
use super::{
    BakeStats, FIRST, Fall, Hydrology, Lake, Network, River, RiverEnd, RiverVertex, TILE_NODES,
    UNKNOWN, field::Tile, pads_of,
};
use crate::ProceduralWorld;
use crate::roads::{Crossing, Junction, Road, RoadClass, RoadEnd, RoadIndex, RoadPoint};
use crate::sites::ProceduralSite;

/// The layout of the file; a reader refuses any other. 3: the river
/// vertices and the road points are columns predicted from the two words
/// before them (see [`planes`]).
const FORMAT: u32 = 3;

/// The layout of a per-mass file and of the index; a reader refuses any
/// other, so a start that finds the files of an older layout bakes and
/// writes the new ones over them. 2: as [`FORMAT`] 3.
const MASS_FORMAT: u32 = 2;

/// Where the index of the per-mass files lives, under the logical root of the
/// source `Files` is over (a native start keeps it beside the generated
/// assets, `xtask bundle` carries it in the data bundle).
pub const WATER_INDEX: &str = "generated/hydrology/water.index";

/// The directory of the per-mass files, [`MassEntry::path`] under it.
pub const MASS_DIR: &str = "generated/hydrology/mass";

/// Why a file was not taken.
#[derive(Debug)]
pub enum WaterFileError {
    /// Not a file of this layout, or cut short.
    Unreadable(String),
    /// Written for another seed, parameter set, asset set or generator.
    Stale,
    /// The cache already holds baked masses; a whole-cache file only fills an
    /// empty one (a per-mass file merges, see [`Hydrology::load_mass`]).
    NotEmpty,
}

impl std::fmt::Display for WaterFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "the baked rivers file is unreadable: {why}"),
            Self::Stale => f.write_str("the baked rivers file is for another world"),
            Self::NotEmpty => f.write_str("the river cache already holds baked masses"),
        }
    }
}

impl std::error::Error for WaterFileError {}

/// One network as the file holds it: what a bake made, less what is derived
/// from it.
#[derive(Serialize)]
struct RecordRef<'a> {
    key: [i32; 2],
    capped: bool,
    rivers: Vec<RiverShell>,
    vertices: Vec<u8>,
    lakes: &'a [Lake],
    falls: &'a [Fall],
    hash: u64,
    stats: BakeStats,
    sites: &'a [ProceduralSite],
    shore: Vec<([i32; 2], u32, f32)>,
    roads: Vec<Shell>,
    columns: Vec<u8>,
    junctions: &'a [Junction],
}

/// [`RecordRef`], read.
#[derive(Deserialize)]
struct Record {
    key: [i32; 2],
    capped: bool,
    rivers: Vec<RiverShell>,
    vertices: Vec<u8>,
    lakes: Vec<Lake>,
    falls: Vec<Fall>,
    hash: u64,
    stats: BakeStats,
    sites: Vec<ProceduralSite>,
    shore: Vec<([i32; 2], u32, f32)>,
    roads: Vec<Shell>,
    columns: Vec<u8>,
    junctions: Vec<Junction>,
}

#[derive(Serialize)]
struct SavedRef<'a> {
    format: u32,
    seed: u64,
    stamp: u64,
    networks: Vec<RecordRef<'a>>,
    /// Per field tile that any mass claimed a node of, its labels.
    tiles: Vec<([i32; 2], Vec<u32>)>,
}

#[derive(Deserialize)]
struct Saved {
    format: u32,
    seed: u64,
    stamp: u64,
    networks: Vec<Record>,
    tiles: Vec<([i32; 2], Vec<u32>)>,
}

/// One mass's file: its network and the nodes it claims.
#[derive(Serialize)]
struct MassRef<'a> {
    format: u32,
    seed: u64,
    stamp: u64,
    record: RecordRef<'a>,
    /// Per field tile the mass has a node in, those nodes' slots in the
    /// tile, each as the step from the one before.
    nodes: &'a [([i32; 2], Vec<u32>)],
}

/// [`MassRef`], read.
#[derive(Deserialize)]
struct MassFile {
    format: u32,
    seed: u64,
    stamp: u64,
    record: Record,
    nodes: Vec<([i32; 2], Vec<u32>)>,
}

/// What the index says of one mass: enough to choose its file without
/// reading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MassEntry {
    /// The mass's name, its first node (the network's key).
    pub key: [i32; 2],
    /// Whether this is one cell's part of a mass over the cap.
    pub capped: bool,
    /// Lattice nodes the mass claims.
    pub nodes: u32,
    /// Bytes of the file, before any compression the bundle adds.
    pub bytes: u32,
    /// The lowest and the highest node it claims, per axis.
    pub bounds: [[i32; 2]; 2],
    /// The field tiles it has a node in, sorted.
    pub tiles: Vec<[i32; 2]>,
}

impl MassEntry {
    /// The file's path under the logical root, in [`MASS_DIR`]. The key is
    /// unique to a mass (its nodes are its own), so the name needs nothing
    /// else, and it is the same name on every machine.
    #[must_use]
    pub fn path(&self) -> String {
        format!("{MASS_DIR}/{}_{}.mass", self.key[0], self.key[1])
    }

    /// How far `(x, z)` is from the box of the mass's nodes, metres: nought
    /// inside it. A lower bound on the distance to the mass itself, which is
    /// what a choice of what to ship first wants.
    #[must_use]
    pub fn distance_m(&self, x: f64, z: f64) -> f64 {
        let reach = |at: f64, lo: i32, hi: i32| {
            let (lo, hi) = (super::metres(lo), super::metres(hi + 1));
            (lo - at).max(at - hi).max(0.0)
        };
        reach(x, self.bounds[0][0], self.bounds[1][0]).hypot(reach(
            z,
            self.bounds[0][1],
            self.bounds[1][1],
        ))
    }
}

/// The small file that names every per-mass file of a baked set, with the
/// stamp they all share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterIndex {
    format: u32,
    seed: u64,
    stamp: u64,
    /// By key.
    pub masses: Vec<MassEntry>,
}

impl WaterIndex {
    /// An index of `masses`, for `seed` and `stamp` (see
    /// [`water_stamp`](crate::version::water_stamp)).
    #[must_use]
    pub fn new(seed: Seed, stamp: u64, mut masses: Vec<MassEntry>) -> Self {
        masses.sort_by_key(|m| m.key);
        Self {
            format: MASS_FORMAT,
            seed: seed.0,
            stamp,
            masses,
        }
    }

    /// The file's bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("an index serialises to memory")
    }

    /// Reads an index and checks it belongs to `seed` and `stamp`.
    ///
    /// # Errors
    /// `WaterFileError`: unreadable, or for another world.
    pub fn read(bytes: &[u8], seed: Seed, stamp: u64) -> Result<Self, WaterFileError> {
        let index: Self = postcard::from_bytes(bytes)
            .map_err(|error| WaterFileError::Unreadable(error.to_string()))?;
        if index.format != MASS_FORMAT {
            return Err(WaterFileError::Unreadable(format!(
                "layout {}, this build reads {MASS_FORMAT}",
                index.format
            )));
        }
        if index.seed != seed.0 || index.stamp != stamp {
            return Err(WaterFileError::Stale);
        }
        Ok(index)
    }

    /// The masses with a node in a field tile that the square from `from` to
    /// `to` (world metres) reaches, `margin` lattice nodes out: a superset of
    /// what a bake of that square would meet, never less, because a tile is
    /// 64 km and the bake's own reach is a few nodes.
    pub fn near(
        &self,
        from: (f64, f64),
        to: (f64, f64),
        margin: i32,
    ) -> impl Iterator<Item = &MassEntry> {
        let tile =
            |metres: f64, plus: i32| (super::node_of(metres) + plus).div_euclid(super::TILE_NODES);
        let (lo, hi) = (
            [tile(from.0, -margin), tile(from.1, -margin)],
            [tile(to.0, margin + 1), tile(to.1, margin + 1)],
        );
        self.masses.iter().filter(move |m| {
            m.tiles
                .iter()
                .any(|t| (lo[0]..=hi[0]).contains(&t[0]) && (lo[1]..=hi[1]).contains(&t[1]))
        })
    }
}

/// What [`Hydrology::load_mass`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassLoad {
    /// The mass was added to the cache.
    Merged,
    /// The cache held the mass already (a bake or an earlier load): nothing
    /// changed.
    Held,
}

/// A road without its per-point numbers, which [`pack_roads`] lays in columns.
#[derive(Serialize, Deserialize)]
struct Shell {
    class: RoadClass,
    ends: [RoadEnd; 2],
    crossings: Vec<Crossing>,
    points: u32,
    profile: u32,
}

/// Each number of every road's points and profile as a column ([`planes`]).
/// Roads were four fifths of the file (ticket 0015 step 3) and are still
/// two thirds of a big mass's. Lossless: [`unpack_roads`] returns every
/// bit.
fn pack_roads(roads: &[Road]) -> (Vec<Shell>, Vec<u8>) {
    let shells = roads
        .iter()
        .map(|r| Shell {
            class: r.class,
            ends: r.ends,
            crossings: r.crossings.clone(),
            points: u32::try_from(r.points.len()).expect("a road's points fit 32 bits"),
            profile: u32::try_from(r.profile.len()).expect("a road's profile fits 32 bits"),
        })
        .collect();
    let wide = |pick: fn(&RoadPoint) -> f64| -> Vec<u64> {
        roads
            .iter()
            .flat_map(|r| r.points.iter().map(pick))
            .map(f64::to_bits)
            .collect()
    };
    let narrow = |values: Vec<f32>| -> Vec<u32> { values.into_iter().map(f32::to_bits).collect() };
    let mut out = Vec::new();
    for column in [wide(|p| p.x), wide(|p| p.z), wide(|p| p.along_m)] {
        planes(&column, &mut out);
    }
    planes(
        &narrow(
            roads
                .iter()
                .flat_map(|r| r.points.iter().map(|p| p.ground_m))
                .collect(),
        ),
        &mut out,
    );
    for k in 0..2 {
        planes(
            &narrow(
                roads
                    .iter()
                    .flat_map(|r| r.profile.iter().map(|q| q[k]))
                    .collect(),
            ),
            &mut out,
        );
    }
    (shells, out)
}

/// [`pack_roads`] undone.
fn unpack_roads(shells: Vec<Shell>, columns: &[u8]) -> Vec<Road> {
    let total: usize = shells.iter().map(|s| s.points as usize).sum();
    let graded: usize = shells.iter().map(|s| s.profile as usize).sum();
    let mut at = 0;
    let mut take = |count: usize, width: usize| {
        let part = &columns[at..at + count * width];
        at += count * width;
        part
    };
    let xs: Vec<u64> = unplanes(take(total, 8));
    let zs: Vec<u64> = unplanes(take(total, 8));
    let alongs: Vec<u64> = unplanes(take(total, 8));
    let grounds: Vec<u32> = unplanes(take(total, 4));
    let rises: [Vec<u32>; 2] = [unplanes(take(graded, 4)), unplanes(take(graded, 4))];
    let (mut first, mut slot) = (0, 0);
    shells
        .into_iter()
        .map(|s| {
            let (n, m) = (s.points as usize, s.profile as usize);
            let points = (first..first + n)
                .map(|i| RoadPoint {
                    x: f64::from_bits(xs[i]),
                    z: f64::from_bits(zs[i]),
                    along_m: f64::from_bits(alongs[i]),
                    ground_m: f32::from_bits(grounds[i]),
                })
                .collect();
            let profile = (slot..slot + m)
                .map(|i| [f32::from_bits(rises[0][i]), f32::from_bits(rises[1][i])])
                .collect();
            first += n;
            slot += m;
            Road {
                class: s.class,
                points,
                ends: s.ends,
                crossings: s.crossings,
                profile,
            }
        })
        .collect()
}

/// A word a column holds, for [`planes`]: the bits of an `f32` or an `f64`.
trait Word: Copy + Default {
    const BYTES: usize;
    fn byte(self, k: usize) -> u8;
    fn with(self, k: usize, byte: u8) -> Self;
    /// The word a straight line through the two before it would reach,
    /// worked in the float the bits are, so every platform guesses alike.
    /// Anything not finite guesses `prev`, because the payload of a NaN is
    /// not the same on every platform.
    fn guess(prev: Self, before: Self) -> Self;
    /// How far `self` is from `guess`, as a zigzag number: small when the
    /// guess was close.
    fn miss(self, guess: Self) -> Self;
    /// [`miss`](Self::miss) undone.
    fn hit(miss: Self, guess: Self) -> Self;
}

macro_rules! word {
    ($t:ty, $float:ty, $signed:ty) => {
        impl Word for $t {
            const BYTES: usize = std::mem::size_of::<$t>();
            fn byte(self, k: usize) -> u8 {
                self.to_le_bytes()[k]
            }
            fn with(self, k: usize, byte: u8) -> Self {
                self | (<$t>::from(byte) << (8 * k))
            }
            fn guess(prev: Self, before: Self) -> Self {
                let (a, b) = (<$float>::from_bits(prev), <$float>::from_bits(before));
                let line = a * 2.0 - b;
                if a.is_finite() && b.is_finite() && line.is_finite() {
                    line.to_bits()
                } else {
                    prev
                }
            }
            fn miss(self, guess: Self) -> Self {
                let d: $signed = self.wrapping_sub(guess).cast_signed();
                ((d << 1) ^ (d >> (<$t>::BITS - 1))).cast_unsigned()
            }
            fn hit(miss: Self, guess: Self) -> Self {
                let d = (miss >> 1) ^ (miss & 1).wrapping_neg();
                guess.wrapping_add(d)
            }
        }
    };
}
word!(u32, f32, i32);
word!(u64, f64, i64);

/// Appends `column` as how far each word is from the straight line through
/// the two before it (the first from nought, the second from the first), one
/// byte plane after another, the highest byte first. A road's points and a
/// river's vertices are metres apart along smooth lines, so the guesses are
/// close, the high planes are runs of zeros and the bundle's gzip folds them
/// (a predicted column was 13 percent smaller than the same words as xor
/// steps, which itself was a third smaller than the postcard fields side by
/// side). Lossless: [`unplanes`] returns every bit.
fn planes<W: Word>(column: &[W], out: &mut Vec<u8>) {
    let mut misses = Vec::with_capacity(column.len());
    for (k, &word) in column.iter().enumerate() {
        let guess = match k {
            0 => W::default(),
            1 => column[0],
            _ => W::guess(column[k - 1], column[k - 2]),
        };
        misses.push(word.miss(guess));
    }
    for k in (0..W::BYTES).rev() {
        out.extend(misses.iter().map(|d| d.byte(k)));
    }
}

/// [`planes`] undone.
fn unplanes<W: Word>(bytes: &[u8]) -> Vec<W> {
    let count = bytes.len() / W::BYTES;
    let mut column = vec![W::default(); count];
    for (plane, chunk) in bytes.chunks(count.max(1)).enumerate() {
        let k = W::BYTES - 1 - plane;
        for (d, &b) in column.iter_mut().zip(chunk) {
            *d = d.with(k, b);
        }
    }
    for k in 0..count {
        let guess = match k {
            0 => W::default(),
            1 => column[0],
            _ => W::guess(column[k - 1], column[k - 2]),
        };
        column[k] = W::hit(column[k], guess);
    }
    column
}

/// A reach without its vertices, which [`pack_rivers`] lays in columns.
#[derive(Serialize, Deserialize)]
struct RiverShell {
    vertices: u32,
    order: u8,
    end: RiverEnd,
    from_lake: Option<u32>,
}

/// Each number of every reach's vertices as a column ([`planes`]), the
/// positions as `f64` and the rest as `f32`, then the fall flags a byte
/// each. Rivers were a quarter of a big mass's file.
fn pack_rivers(rivers: &[River]) -> (Vec<RiverShell>, Vec<u8>) {
    let shells = rivers
        .iter()
        .map(|r| RiverShell {
            vertices: u32::try_from(r.vertices.len()).expect("a reach's vertices fit 32 bits"),
            order: r.order,
            end: r.end,
            from_lake: r.from_lake,
        })
        .collect();
    let all = || rivers.iter().flat_map(|r| r.vertices.iter());
    let wide =
        |pick: fn(&RiverVertex) -> f64| -> Vec<u64> { all().map(|v| pick(v).to_bits()).collect() };
    let narrow =
        |pick: fn(&RiverVertex) -> f32| -> Vec<u32> { all().map(|v| pick(v).to_bits()).collect() };
    let mut out = Vec::new();
    for column in [wide(|v| v.x), wide(|v| v.z)] {
        planes(&column, &mut out);
    }
    for column in [
        narrow(|v| v.surface_m),
        narrow(|v| v.discharge_m3s),
        narrow(|v| v.width_m),
        narrow(|v| v.depth_m),
        narrow(|v| v.cut_m),
        narrow(|v| v.meander_m),
        narrow(|v| v.phase),
    ] {
        planes(&column, &mut out);
    }
    out.extend(all().map(|v| u8::from(v.fall)));
    (shells, out)
}

/// [`pack_rivers`] undone.
fn unpack_rivers(shells: Vec<RiverShell>, columns: &[u8]) -> Vec<River> {
    let total: usize = shells.iter().map(|s| s.vertices as usize).sum();
    let mut at = 0;
    let mut take = |count: usize, width: usize| {
        let part = &columns[at..at + count * width];
        at += count * width;
        part
    };
    let xs: Vec<u64> = unplanes(take(total, 8));
    let zs: Vec<u64> = unplanes(take(total, 8));
    let mut narrow = || -> Vec<u32> { unplanes(take(total, 4)) };
    let surface = narrow();
    let discharge = narrow();
    let width = narrow();
    let depth = narrow();
    let cut = narrow();
    let meander = narrow();
    let phase = narrow();
    let falls = take(total, 1);
    let mut first = 0;
    shells
        .into_iter()
        .map(|s| {
            let n = s.vertices as usize;
            let vertices = (first..first + n)
                .map(|i| RiverVertex {
                    x: f64::from_bits(xs[i]),
                    z: f64::from_bits(zs[i]),
                    surface_m: f32::from_bits(surface[i]),
                    discharge_m3s: f32::from_bits(discharge[i]),
                    width_m: f32::from_bits(width[i]),
                    depth_m: f32::from_bits(depth[i]),
                    cut_m: f32::from_bits(cut[i]),
                    fall: falls[i] != 0,
                    meander_m: f32::from_bits(meander[i]),
                    phase: f32::from_bits(phase[i]),
                })
                .collect();
            first += n;
            River {
                vertices,
                order: s.order,
                end: s.end,
                from_lake: s.from_lake,
            }
        })
        .collect()
}

/// A network as the record a file holds.
fn record_of(n: &Network) -> RecordRef<'_> {
    let (rivers, vertices) = pack_rivers(&n.rivers);
    let (roads, columns) = pack_roads(&n.roads);
    RecordRef {
        key: n.key,
        capped: n.capped,
        rivers,
        vertices,
        lakes: &n.lakes,
        falls: &n.falls,
        hash: n.hash,
        stats: n.stats,
        sites: &n.sites,
        shore: n.shore.entries(),
        roads,
        columns,
        junctions: &n.junctions,
    }
}

impl Hydrology {
    /// Every network baked so far and which mass each node is on, as a file
    /// [`load`](Self::load) reads, stamped with `stamp` (see
    /// [`water_stamp`](crate::version::water_stamp)).
    #[must_use]
    pub fn save(&self, stamp: u64) -> Vec<u8> {
        let networks = self.networks.read().unwrap_or_else(PoisonError::into_inner);
        let tiles = self.tiles.read().unwrap_or_else(PoisonError::into_inner);
        let mut labelled: Vec<([i32; 2], Vec<u32>)> = tiles
            .iter()
            .map(|(&key, tile)| {
                let labels = tile
                    .label
                    .iter()
                    .map(|label| label.load(Ordering::Acquire))
                    .collect::<Vec<_>>();
                (key, labels)
            })
            .filter(|(_, labels)| labels.iter().any(|&label| label != UNKNOWN))
            .collect();
        labelled.sort_by_key(|(key, _)| *key);
        let saved = SavedRef {
            format: FORMAT,
            seed: self.seed.0,
            stamp,
            networks: networks.iter().map(|n| record_of(n)).collect(),
            tiles: labelled,
        };
        postcard::to_allocvec(&saved).expect("a baked cache serialises to memory")
    }

    /// Fills an empty cache from a file [`save`](Self::save) wrote, on
    /// `threads` for the field's tiles, and returns how many masses it holds.
    ///
    /// # Errors
    /// `WaterFileError`; the cache is untouched then.
    pub fn load(
        &self,
        world: &ProceduralWorld,
        bytes: &[u8],
        stamp: u64,
        threads: usize,
    ) -> Result<usize, WaterFileError> {
        let saved: Saved = postcard::from_bytes(bytes)
            .map_err(|error| WaterFileError::Unreadable(error.to_string()))?;
        if saved.format != FORMAT {
            return Err(WaterFileError::Unreadable(format!(
                "layout {}, this build reads {FORMAT}",
                saved.format
            )));
        }
        if saved.seed != self.seed.0 || saved.stamp != stamp {
            return Err(WaterFileError::Stale);
        }
        let _one = self.bake.lock().unwrap_or_else(PoisonError::into_inner);
        if !self
            .networks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
        {
            return Err(WaterFileError::NotEmpty);
        }
        let count = saved.networks.len();
        let rebuilt: Vec<std::sync::Arc<Network>> = saved
            .networks
            .into_iter()
            .map(|r| std::sync::Arc::new(self.rebuild(r)))
            .collect();
        let mut held: Vec<(std::sync::Arc<Tile>, Vec<u32>)> = Vec::new();
        for (key, labels) in saved.tiles {
            let tile = self.tile(world, key, threads);
            if tile.label.len() != labels.len() {
                return Err(WaterFileError::Unreadable(format!(
                    "tile {key:?} holds {} nodes, the file {}",
                    tile.label.len(),
                    labels.len()
                )));
            }
            held.push((tile, labels));
        }
        // The networks first: a label is an index into them.
        *self
            .networks
            .write()
            .unwrap_or_else(PoisonError::into_inner) = rebuilt;
        for (tile, labels) in &held {
            for (slot, &label) in tile.label.iter().zip(labels) {
                slot.store(label, Ordering::Release);
            }
        }
        for (tile, _) in &held {
            tile.drop_if_baked();
        }
        Ok(count)
    }

    /// A network from its record, its indexes (derived from it) made by the
    /// code a bake uses.
    fn rebuild(&self, r: Record) -> Network {
        let roads = unpack_roads(r.roads, &r.columns);
        let rivers = unpack_rivers(r.rivers, &r.vertices);
        let index = SegmentIndex::new(&rivers, &self.params);
        Network {
            key: r.key,
            capped: r.capped,
            index,
            shore: Shore::from_entries(&r.shore),
            hash: r.hash,
            stats: r.stats,
            pads: pads_of(&r.sites),
            road_index: RoadIndex::new(&roads),
            rivers,
            lakes: r.lakes,
            falls: r.falls,
            params: self.params,
            sites: r.sites,
            roads,
            junctions: r.junctions,
        }
    }

    /// Whether the cache holds the mass named `key`, baked or loaded.
    #[must_use]
    pub fn holds_mass(&self, key: [i32; 2], capped: bool) -> bool {
        self.networks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|n| n.key == key && n.capped == capped)
    }

    /// The masses of `index` that the square from `from` to `to` can meet and
    /// the cache does not hold: what a start or the browser's fetch ahead
    /// asks for before it would bake them.
    #[must_use]
    pub fn masses_wanted<'a>(
        &self,
        index: &'a WaterIndex,
        from: (f64, f64),
        to: (f64, f64),
    ) -> Vec<&'a MassEntry> {
        index
            .near(from, to, super::cell_margin(&self.params))
            .filter(|m| !self.holds_mass(m.key, m.capped))
            .collect()
    }

    /// Every network baked so far as one file a mass, each with its
    /// [`MassEntry`], for the masses `keep` accepts (the index of what is on
    /// disk already says which a writer may skip). Sorted by key. The files
    /// carry no label: a mass is its network and the nodes it claims, so the
    /// same cache written in any bake order gives the same bytes.
    #[must_use]
    pub fn save_masses(
        &self,
        stamp: u64,
        keep: impl Fn(&MassEntry) -> bool,
    ) -> Vec<(MassEntry, Vec<u8>)> {
        let networks = self.networks.read().unwrap_or_else(PoisonError::into_inner);
        let tiles = self.tiles.read().unwrap_or_else(PoisonError::into_inner);
        let mut keys: Vec<[i32; 2]> = tiles.keys().copied().collect();
        keys.sort_unstable();
        // Per label, per tile in key order, the slots it holds.
        let mut claimed: Vec<Vec<([i32; 2], Vec<u32>)>> = vec![Vec::new(); networks.len()];
        for key in keys {
            let mut here: std::collections::BTreeMap<u32, Vec<u32>> =
                std::collections::BTreeMap::new();
            for (k, label) in tiles[&key].label.iter().enumerate() {
                let label = label.load(Ordering::Acquire);
                if label >= FIRST {
                    here.entry(label)
                        .or_default()
                        .push(u32::try_from(k).expect("a slot fits 32 bits"));
                }
            }
            for (label, slots) in here {
                let mut before = 0;
                let deltas = slots
                    .into_iter()
                    .map(|slot| {
                        let step = slot - before;
                        before = slot;
                        step
                    })
                    .collect();
                if let Some(held) = claimed.get_mut((label - FIRST) as usize) {
                    held.push((key, deltas));
                }
            }
        }
        let mut out: Vec<(MassEntry, Vec<u8>)> = networks
            .iter()
            .zip(&claimed)
            .filter_map(|(n, nodes)| {
                let mut entry = MassEntry {
                    key: n.key,
                    capped: n.capped,
                    nodes: 0,
                    bytes: 0,
                    bounds: [[i32::MAX; 2], [i32::MIN; 2]],
                    tiles: nodes.iter().map(|(key, _)| *key).collect(),
                };
                for (key, deltas) in nodes {
                    let mut slot = 0;
                    for step in deltas {
                        slot += step;
                        let (i, j) = (slot % TILE_NODES as u32, slot / TILE_NODES as u32);
                        let at = [
                            key[0] * TILE_NODES + i32::try_from(i).expect("inside a tile"),
                            key[1] * TILE_NODES + i32::try_from(j).expect("inside a tile"),
                        ];
                        let [low, high] = &mut entry.bounds;
                        *low = [low[0].min(at[0]), low[1].min(at[1])];
                        *high = [high[0].max(at[0]), high[1].max(at[1])];
                        entry.nodes += 1;
                    }
                }
                if !keep(&entry) {
                    return None;
                }
                let saved = MassRef {
                    format: MASS_FORMAT,
                    seed: self.seed.0,
                    stamp,
                    record: record_of(n),
                    nodes,
                };
                let bytes = postcard::to_allocvec(&saved).expect("a mass serialises to memory");
                entry.bytes = u32::try_from(bytes.len()).expect("a mass file fits 32 bits");
                Some((entry, bytes))
            })
            .collect();
        out.sort_by_key(|(entry, _)| entry.key);
        out
    }

    /// Merges the mass in `bytes` (a file [`save_masses`](Self::save_masses)
    /// wrote) into the cache, whatever else it holds, and returns what it did.
    ///
    /// The cache's shared state is its list of networks and the labels of the
    /// field's tiles, which index it. The merge takes the bake's own lock, so
    /// a mass is in the cache once, whether a bake or a file brought it, and
    /// pushes the network where a bake would (its label is then its place in
    /// the list), writes that label into the tiles at the mass's nodes only,
    /// and drops a tile's rain when its land is all labelled. Nothing it
    /// leaves is read by a sample before the labels are set, because a
    /// sample takes a label's network from the list the label was made from.
    /// The cell cache needs no change: a cell is made by resolving the labels
    /// round it, which bakes (or finds) every mass it reaches, so a cell that
    /// exists already holds this mass if it reaches it.
    ///
    /// # Errors
    /// `WaterFileError`; the cache is untouched then. A mass whose nodes some
    /// other mass has claimed is `Unreadable`, and a file of another world
    /// `Stale`.
    pub fn load_mass(
        &self,
        world: &ProceduralWorld,
        bytes: &[u8],
        stamp: u64,
        threads: usize,
    ) -> Result<MassLoad, WaterFileError> {
        let file: MassFile = postcard::from_bytes(bytes)
            .map_err(|error| WaterFileError::Unreadable(error.to_string()))?;
        if file.format != MASS_FORMAT {
            return Err(WaterFileError::Unreadable(format!(
                "layout {}, this build reads {MASS_FORMAT}",
                file.format
            )));
        }
        if file.seed != self.seed.0 || file.stamp != stamp {
            return Err(WaterFileError::Stale);
        }
        let _one = self.bake.lock().unwrap_or_else(PoisonError::into_inner);
        if self.holds_mass(file.record.key, file.record.capped) {
            return Ok(MassLoad::Held);
        }
        let mut held: Vec<(std::sync::Arc<Tile>, Vec<u32>)> = Vec::new();
        for (key, deltas) in file.nodes {
            let tile = self.tile(world, key, threads);
            let mut slot = 0_usize;
            let mut slots = Vec::with_capacity(deltas.len());
            for step in deltas {
                slot += step as usize;
                if slot >= tile.label.len() {
                    return Err(WaterFileError::Unreadable(format!(
                        "tile {key:?} has no node {slot}"
                    )));
                }
                if tile.label[slot].load(Ordering::Acquire) >= FIRST {
                    return Err(WaterFileError::Unreadable(format!(
                        "node {slot} of tile {key:?} is another mass's"
                    )));
                }
                slots.push(u32::try_from(slot).expect("a slot fits 32 bits"));
            }
            held.push((tile, slots));
        }
        let network = std::sync::Arc::new(self.rebuild(file.record));
        let mut networks = self
            .networks
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let label = FIRST + u32::try_from(networks.len()).expect("fewer than 4 billion masses");
        networks.push(network);
        drop(networks);
        for (tile, slots) in &held {
            for &slot in slots {
                tile.label[slot as usize].store(label, Ordering::Release);
            }
        }
        for (tile, _) in &held {
            tile.drop_if_baked();
        }
        Ok(MassLoad::Merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_comes_back_bit_for_bit() {
        let wide: Vec<u64> = [0.0_f64, 1.5e5, 150_003.25, -7.0e-3, f64::MAX, 150_003.25]
            .iter()
            .map(|v| v.to_bits())
            .collect();
        let mut out = Vec::new();
        planes(&wide, &mut out);
        assert_eq!(unplanes::<u64>(&out), wide);
        let narrow: Vec<u32> = vec![0, 1, 0x3f80_0000, u32::MAX, 7];
        let mut out = Vec::new();
        planes(&narrow, &mut out);
        assert_eq!(unplanes::<u32>(&out), narrow);
        assert!(unplanes::<u64>(&[]).is_empty());
    }

    /// Every bit pattern, the ones a float guess would trip on (NaNs with
    /// payloads, infinities, subnormals, the largest and the sign of
    /// nought) among them, in runs of every length from none.
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the generator's words are narrowed and indexed on purpose"
    )]
    fn a_column_of_any_bits_comes_back() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let odd64 = [
            f64::NAN.to_bits(),
            f64::NAN.to_bits() | 0x7ff,
            f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(),
            f64::MAX.to_bits(),
            f64::MIN.to_bits(),
            f64::MIN_POSITIVE.to_bits() / 2,
            (-0.0_f64).to_bits(),
            0,
            u64::MAX,
        ];
        let odd32 = [
            f32::NAN.to_bits(),
            f32::INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
            f32::MAX.to_bits(),
            f32::MIN.to_bits(),
            f32::MIN_POSITIVE.to_bits() / 2,
            (-0.0_f32).to_bits(),
            0,
            u32::MAX,
        ];
        for len in 0..40 {
            let wide: Vec<u64> = (0..len)
                .map(|k| {
                    if k % 3 == 0 {
                        odd64[(next() % odd64.len() as u64) as usize]
                    } else {
                        next()
                    }
                })
                .collect();
            let mut out = Vec::new();
            planes(&wide, &mut out);
            assert_eq!(unplanes::<u64>(&out), wide, "{len} wide words");
            let narrow: Vec<u32> = (0..len)
                .map(|k| {
                    if k % 3 == 1 {
                        odd32[(next() % odd32.len() as u64) as usize]
                    } else {
                        next() as u32
                    }
                })
                .collect();
            let mut out = Vec::new();
            planes(&narrow, &mut out);
            assert_eq!(unplanes::<u32>(&out), narrow, "{len} narrow words");
        }
    }

    /// A smooth line is what the guess is for: its high planes are zeros.
    #[test]
    fn a_straight_line_costs_almost_nothing() {
        let line: Vec<u64> = (0..1000)
            .map(|k| (150_000.0 + f64::from(k) * 14.25).to_bits())
            .collect();
        let mut out = Vec::new();
        planes(&line, &mut out);
        let zeros = out
            .iter()
            .copied()
            .map(usize::from)
            .filter(|&b| b == 0)
            .count();
        assert!(zeros * 10 > out.len() * 6, "{zeros} of {}", out.len());
        assert_eq!(unplanes::<u64>(&out), line);
    }

    /// A start that finds the files of the layout before this one must
    /// bake, not read them as this one: the index and a mass file each name
    /// their layout, and a reader refuses any other.
    #[test]
    fn files_of_the_older_layout_are_refused() {
        let mut index = WaterIndex::new(Seed(42), STAMP, Vec::new());
        assert!(WaterIndex::read(&index.to_bytes(), Seed(42), STAMP).is_ok());
        index.format = MASS_FORMAT - 1;
        assert!(matches!(
            WaterIndex::read(&index.to_bytes(), Seed(42), STAMP),
            Err(WaterFileError::Unreadable(_))
        ));
        let record = || RecordRef {
            key: [0, 0],
            capped: false,
            rivers: Vec::new(),
            vertices: Vec::new(),
            lakes: &[],
            falls: &[],
            hash: 0,
            stats: BakeStats {
                nodes: 0,
                seconds: 0.0,
                tiles: 0,
                segments: 0,
                bucket_entries: 0,
                sites: 0,
                sites_seconds: 0.0,
                roads: 0,
                road_km: 0.0,
                unreached: 0,
                roads_seconds: 0.0,
                grade_seconds: 0.0,
            },
            sites: &[],
            shore: Vec::new(),
            roads: Vec::new(),
            columns: Vec::new(),
            junctions: &[],
        };
        let file = |format| {
            postcard::to_allocvec(&MassRef {
                format,
                seed: 42,
                stamp: STAMP,
                record: record(),
                nodes: &[],
            })
            .expect("a mass serialises to memory")
        };
        let world = world();
        let cache = Hydrology::new(Seed(42));
        assert!(matches!(
            cache.load_mass(&world, &file(MASS_FORMAT - 1), STAMP, 1),
            Err(WaterFileError::Unreadable(_))
        ));
        assert!(cache.networks().is_empty());
        assert_eq!(
            cache
                .load_mass(&world, &file(MASS_FORMAT), STAMP, 1)
                .expect("the current layout reads"),
            MassLoad::Merged
        );
    }

    fn world() -> ProceduralWorld {
        let registries =
            strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load");
        ProceduralWorld::new(Seed(42), &registries)
    }

    const STAMP: u64 = 0x5eed;

    /// The 1 600 km2 island under a cap of 2 000 nodes in cells of 32 is two
    /// masses here (parts of one), a mass over the cap whose parts are baked
    /// one at a time.
    fn cut_cache() -> Hydrology {
        Hydrology::with_cap(Seed(42), 2_000, 32)
    }

    fn files(h: &Hydrology) -> Vec<(MassEntry, Vec<u8>)> {
        h.save_masses(STAMP, |_| true)
    }

    fn load_all(h: &Hydrology, world: &ProceduralWorld, files: &[(MassEntry, Vec<u8>)]) {
        for (_, bytes) in files {
            assert_eq!(
                h.load_mass(world, bytes, STAMP, 4).expect("a file loads"),
                MassLoad::Merged
            );
        }
    }

    #[test]
    fn a_cache_written_whole_and_read_per_mass_is_equal() {
        let world = world();
        // The canary island, whole: no cap in the way.
        let source = Hydrology::new(Seed(42));
        let island = source
            .network_at(
                &world,
                super::super::metres(super::super::CANARY_MASS[0]),
                super::super::metres(super::super::CANARY_MASS[1]),
            )
            .expect("the canary island is land");
        let written = files(&source);
        assert_eq!(written.len(), 1);
        let entry = &written[0].0;
        assert_eq!(entry.key, island.key());
        assert!(!entry.capped && entry.nodes > 5_000, "{entry:?}");
        assert_eq!(entry.bytes as usize, written[0].1.len());
        // Read per mass into an empty cache, the whole file's way too.
        let split = Hydrology::new(Seed(42));
        load_all(&split, &world, &written);
        let whole = Hydrology::new(Seed(42));
        whole
            .load(&world, &source.save(STAMP), STAMP, 4)
            .expect("the whole file loads");
        assert_eq!(files(&split), written);
        assert_eq!(files(&whole), written);
        // The loaded mass answers as the baked one, and no sample bakes it.
        let (x, z) = (
            super::super::metres(entry.key[0]),
            super::super::metres(entry.key[1]),
        );
        let again = split.network_at(&world, x, z).expect("land");
        assert_eq!(again.content_hash(), island.content_hash());
        assert_eq!(split.networks().len(), 1);
        // Twice is a no-op; another world's file is refused and changes nothing.
        assert_eq!(
            split
                .load_mass(&world, &written[0].1, STAMP, 4)
                .expect("held"),
            MassLoad::Held
        );
        assert!(matches!(
            Hydrology::new(Seed(42)).load_mass(&world, &written[0].1, STAMP + 1, 4),
            Err(WaterFileError::Stale)
        ));
        assert!(matches!(
            Hydrology::new(Seed(7)).load_mass(&world, &written[0].1, STAMP, 4),
            Err(WaterFileError::Stale)
        ));
        assert!(matches!(
            Hydrology::new(Seed(42)).load_mass(&world, &written[0].1[..40], STAMP, 4),
            Err(WaterFileError::Unreadable(_))
        ));
    }

    #[test]
    fn a_merge_of_two_masses_equals_a_bake_of_both() {
        let world = world();
        let (a, b) = ([-74, 120], [-50, 120]);
        let at = |h: &Hydrology, n: [i32; 2]| {
            h.network_at(
                &world,
                super::super::metres(n[0]),
                super::super::metres(n[1]),
            )
            .expect("land")
        };
        let source = cut_cache();
        let (first, second) = (at(&source, a), at(&source, b));
        assert!(first.capped() && second.capped());
        assert_ne!(first.key(), second.key());
        let written = files(&source);
        assert_eq!(written.len(), 2);
        // Into an empty cache, in either order, and one at a time into a cache
        // that already holds the other (which it baked itself).
        let forward = cut_cache();
        load_all(&forward, &world, &written);
        let backward = cut_cache();
        let reversed: Vec<_> = written.iter().rev().cloned().collect();
        load_all(&backward, &world, &reversed);
        assert_eq!(files(&forward), written);
        assert_eq!(files(&backward), written);
        let mixed = cut_cache();
        let own = at(&mixed, a);
        assert_eq!(own.key(), first.key());
        let index = WaterIndex::new(
            Seed(42),
            STAMP,
            written.iter().map(|(e, _)| e.clone()).collect(),
        );
        let wanted = mixed.masses_wanted(&index, (-40_000.0, 60_000.0), (-20_000.0, 70_000.0));
        assert_eq!(wanted.len(), 1, "the baked part is held, the other wanted");
        let (held_entry, held_file) = &written[usize::from(written[0].0.key == first.key())];
        assert_eq!(wanted[0].key, held_entry.key);
        assert_eq!(
            mixed
                .load_mass(&world, held_file, STAMP, 4)
                .expect("merges"),
            MassLoad::Merged
        );
        // The part the cache baked is Held when its own file comes.
        let (own_entry, own_file) = &written[usize::from(written[0].0.key != first.key())];
        assert_eq!(own_entry.key, first.key());
        assert_eq!(
            mixed.load_mass(&world, own_file, STAMP, 4).expect("held"),
            MassLoad::Held
        );
        // A bake records how long it took, so the baked part's bytes are its
        // own; what the index says of it, and the merged part's bytes, agree.
        let mixed_files = files(&mixed);
        let names = |f: &[(MassEntry, Vec<u8>)]| -> Vec<_> {
            f.iter()
                .map(|(e, _)| (e.key, e.capped, e.nodes, e.bounds, e.tiles.clone()))
                .collect()
        };
        assert_eq!(names(&mixed_files), names(&written));
        let merged = usize::from(mixed_files[0].0.key != held_entry.key);
        assert_eq!(mixed_files[merged].1, held_file.clone());
        // Each node of each part resolves to its own part, and no sample
        // baked anything the files had.
        for h in [&forward, &backward, &mixed] {
            assert_eq!(at(h, a).content_hash(), first.content_hash());
            assert_eq!(at(h, b).content_hash(), second.content_hash());
            assert_eq!(h.networks().len(), 2);
        }
        // The index lists each part with the one tile it stands in.
        assert!(
            index
                .masses
                .iter()
                .all(|m| m.tiles.len() <= 4 && m.nodes > 0)
        );
    }

    #[test]
    fn the_index_names_the_masses_a_square_can_meet() {
        let tile = |i: i32, j: i32| [i, j];
        let entry = |key: [i32; 2], tiles: Vec<[i32; 2]>| MassEntry {
            key,
            capped: false,
            nodes: 1,
            bytes: 1,
            bounds: [key, key],
            tiles,
        };
        let index = WaterIndex::new(
            Seed(1),
            2,
            vec![
                entry([300, 0], vec![tile(2, 0)]),
                entry([0, 0], vec![tile(0, 0), tile(1, 0)]),
            ],
        );
        // 65 536 m a tile; the first square sits in tile 0, a margin of 4
        // nodes (2 048 m) from tile 1.
        let near = |from: (f64, f64), to: (f64, f64)| -> Vec<[i32; 2]> {
            index.near(from, to, 4).map(|m| m.key).collect()
        };
        assert_eq!(near((0.0, 0.0), (1_000.0, 1_000.0)), vec![[0, 0]]);
        assert_eq!(near((63_000.0, 0.0), (64_000.0, 1_000.0)), vec![[0, 0]]);
        assert_eq!(
            near((0.0, 0.0), (140_000.0, 1_000.0)),
            vec![[0, 0], [300, 0]]
        );
        assert!(near((0.0, 900_000.0), (1_000.0, 901_000.0)).is_empty());
        assert_eq!(index.masses[0].key, [0, 0], "sorted by key");
        let round = WaterIndex::read(&index.to_bytes(), Seed(1), 2).expect("reads");
        assert_eq!(round, index);
        assert!(matches!(
            WaterIndex::read(&index.to_bytes(), Seed(1), 3),
            Err(WaterFileError::Stale)
        ));
        assert!((index.masses[0].distance_m(-1_000.0, 0.0) - 1_000.0).abs() < 1e-9);
        assert_eq!(
            entry([5, 5], vec![]).path(),
            "generated/hydrology/mass/5_5.mass"
        );
    }
}
