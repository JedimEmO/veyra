//! The baked cache as one file, for a platform that cannot afford to bake.
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

use super::index::{SegmentIndex, Shore};
use super::{BakeStats, Fall, Hydrology, Lake, Network, River, UNKNOWN, field::Tile, pads_of};
use crate::ProceduralWorld;
use crate::roads::{Crossing, Junction, Road, RoadClass, RoadEnd, RoadIndex, RoadPoint};
use crate::sites::ProceduralSite;

/// The layout of the file; a reader refuses any other.
const FORMAT: u32 = 2;

/// Why a file was not taken.
#[derive(Debug)]
pub enum WaterFileError {
    /// Not a file of this layout, or cut short.
    Unreadable(String),
    /// Written for another seed, parameter set, asset set or generator.
    Stale,
    /// The cache already holds baked masses; a file only fills an empty one.
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
    rivers: &'a [River],
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
    rivers: Vec<River>,
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

/// A road without its per-point numbers, which [`pack_roads`] lays in columns.
#[derive(Serialize, Deserialize)]
struct Shell {
    class: RoadClass,
    ends: [RoadEnd; 2],
    crossings: Vec<Crossing>,
    points: u32,
    profile: u32,
}

/// Each number of every road's points as the bits of the one before it
/// xor-ed away, then the bytes of those words side by side by position. A
/// road's points are metres apart, so the high bytes come out as runs of
/// zeros that the bundle's gzip folds to almost nothing (roads were four
/// fifths of the file, ticket 0015 step 3). Lossless: [`unpack_roads`]
/// returns every bit.
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

/// A word a column holds, for [`planes`].
trait Word: Copy + std::ops::BitXor<Output = Self> + Default {
    const BYTES: usize;
    fn byte(self, k: usize) -> u8;
    fn with(self, k: usize, byte: u8) -> Self;
}

macro_rules! word {
    ($t:ty) => {
        impl Word for $t {
            const BYTES: usize = std::mem::size_of::<$t>();
            fn byte(self, k: usize) -> u8 {
                self.to_le_bytes()[k]
            }
            fn with(self, k: usize, byte: u8) -> Self {
                self | (<$t>::from(byte) << (8 * k))
            }
        }
    };
}
word!(u32);
word!(u64);

/// Appends `column` as XOR deltas, one byte plane after another, the
/// highest byte first.
fn planes<W: Word>(column: &[W], out: &mut Vec<u8>) {
    let mut deltas = Vec::with_capacity(column.len());
    let mut before = W::default();
    for &word in column {
        deltas.push(word ^ before);
        before = word;
    }
    for k in (0..W::BYTES).rev() {
        out.extend(deltas.iter().map(|d| d.byte(k)));
    }
}

/// [`planes`] undone.
fn unplanes<W: Word>(bytes: &[u8]) -> Vec<W> {
    let count = bytes.len() / W::BYTES;
    let mut deltas = vec![W::default(); count];
    for (plane, chunk) in bytes.chunks(count.max(1)).enumerate() {
        let k = W::BYTES - 1 - plane;
        for (d, &b) in deltas.iter_mut().zip(chunk) {
            *d = d.with(k, b);
        }
    }
    let mut before = W::default();
    for d in &mut deltas {
        *d = *d ^ before;
        before = *d;
    }
    deltas
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
            networks: networks
                .iter()
                .map(|n| {
                    let (roads, columns) = pack_roads(&n.roads);
                    RecordRef {
                        key: n.key,
                        capped: n.capped,
                        rivers: &n.rivers,
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
                })
                .collect(),
            tiles: labelled,
        };
        postcard::to_allocvec(&saved).expect("a baked cache serialises to memory")
    }

    /// Fills an empty cache from a file [`save`](Self::save) wrote, on
    /// `threads` for the field's tiles, and returns how many masses it holds.
    ///
    /// # Errors
    /// [`WaterFileError`]; the cache is untouched then.
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
            .map(|r| {
                let roads = unpack_roads(r.roads, &r.columns);
                let index = SegmentIndex::new(&r.rivers, &self.params);
                std::sync::Arc::new(Network {
                    key: r.key,
                    capped: r.capped,
                    index,
                    shore: Shore::from_entries(&r.shore),
                    hash: r.hash,
                    stats: r.stats,
                    pads: pads_of(&r.sites),
                    road_index: RoadIndex::new(&roads),
                    rivers: r.rivers,
                    lakes: r.lakes,
                    falls: r.falls,
                    params: self.params,
                    sites: r.sites,
                    roads,
                    junctions: r.junctions,
                })
            })
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
}
