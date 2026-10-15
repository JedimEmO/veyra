//! Basins: where cold air would pool, as a property of the ground.
//!
//! A [`BasinTile`] is what the ground says about fog over one square of the
//! world, whatever the weather: the level cold air would fill a hollow to,
//! the hollow's floor, the ground, how wet it is and how far the sea is. It
//! is baked once per tile from the [`WorldSource`] and never changes, so a
//! caller bakes it off the frame and keeps it like a region's placement.
//!
//! # The rule
//!
//! Cold air runs downhill and pools like water, so a fog sea has a level
//! top; but a valley that drains still fills, because the air is too heavy
//! to leave faster than it gathers. So the question is not "is this a closed
//! depression" (a spill-point fill finds nothing in a valley that opens
//! downhill, and the bay head at (-5950, -13650) that ticket 0003's first
//! captures filled is open to the sea) but "is the ground here below its
//! surroundings at the scale of a
//! valley". The answer is the grey *closing* of the height field by a disk
//! of radius [`POOL_RADIUS_CELLS`] (2 km): a dilation (the highest ground
//! within the disk) followed by an erosion (the lowest of those). At a
//! point the closing is the lowest level a disk that covers the point can
//! reach without its highest ground rising above; inside a hollow narrower
//! than the disk every such disk reaches the rim, so the closing there is
//! the rim's lowest part, one level across the hollow. On a plane, a ridge
//! or a plateau the closing is the ground itself, so nothing pools there.
//!
//! The fog's top is set from that rim, lower than it:
//! `floor + TOP_FRACTION * (rim - floor)`, where the floor is the rim less
//! the deepest the ground lies under it within half a disk. The margin is a
//! share of the basin's depth, so a 12 m trough, a 25 m hollow and a 100 m
//! bay head get proportionate tops from one rule with nothing tuned per
//! place. Two basins have two rims and so two levels; a long valley's rim
//! follows the lower of its two sides, so its level runs gently down it.
//!
//! The sea's surface counts as ground at sea level, so a bay head's slopes
//! are a basin whose floor is the water. But the sea is not a basin: the
//! open sea is flat, so its closing is the sea itself, and over any water
//! the field fades basin fog out within a few hundred metres of the shore
//! ([`BasinTile::offshore`]), so a strait between two headlands, narrower
//! than the disk, stays clear. Fog over the sea is a weather of its own
//! ([`crate::FogField`]).
//!
//! # The apron
//!
//! Every step is position-local (see `grid`), so a node's values depend only
//! on heights within [`APRON_CELLS`] of it. A tile bakes its square plus
//! that apron and keeps only its square: neighbouring tiles compute their
//! shared edge nodes to the same bits, and the field has no seam.

use serde::{Deserialize, Serialize};
use strata_worldgen::WorldSource;

use crate::grid::{Grid, box_blur, capped_distance, disk_extreme};

/// Metres between the nodes of a tile.
pub const CELL_M: f64 = 128.0;
/// [`CELL_M`] as an integer, for the path lengths.
const CELL_WHOLE_M: u32 = 128;
/// Cells along a tile's side; it holds one more node than this per side,
/// sharing its edge nodes with its neighbours.
pub const TILE_CELLS: usize = 64;
/// A tile's side in metres (8192).
// 64 * 128 is exact in f64.
#[allow(clippy::cast_precision_loss)]
pub const TILE_M: f64 = TILE_CELLS as f64 * CELL_M;
/// Nodes along a tile's side.
pub const TILE_NODES: usize = TILE_CELLS + 1;
/// The closing disk's radius in cells (2048 m): the scale of a valley.
/// Hollows and valleys narrower than twice this fill; a basin wider than
/// that keeps only its narrower arms.
pub const POOL_RADIUS_CELLS: usize = 16;
/// The radius within which a basin's deepest point sets its depth, in
/// cells (1024 m): half the disk.
pub const DEPTH_RADIUS_CELLS: usize = POOL_RADIUS_CELLS / 2;
/// The box radius of the two passes that smooth the rim and the depth, in
/// cells: the steps a disk leaves at 128 m become slopes a few hundred
/// metres long, without spreading a hollow's level far past its edge.
pub const SMOOTH_RADIUS_CELLS: usize = 2;
/// The box radius hydration is averaged over, in cells: a sample's
/// hydration is noisy at 128 m and a basin is wet or dry as a whole.
pub const HYDRATION_RADIUS_CELLS: usize = 2;
/// How far the smoothed rim may stand over the ground, in multiples of the
/// smoothed depth under the raw rim (see the bake).
const EDGE_GAIN: f64 = 2.0;
/// Where a basin's fog stops, as a share of the way from its floor to its
/// rim.
pub const TOP_FRACTION: f64 = 0.8;
/// How far inland sea fog can reach, in metres of low ground from the sea.
pub const SEA_REACH_M: u32 = 3000;
/// Ground at or above this height blocks sea fog's way inland: no sea
/// fog's top is this high (85 m at most, [`crate::FogField`]).
pub const SEA_PASS_M: f64 = 100.0;
/// How far out over the water [`BasinTile::offshore`] counts, metres.
pub const OFFSHORE_CAP_M: u32 = 1024;
/// How far outside its square a tile reads heights: the closing reads
/// twice the disk, the depth half a disk more, the smoothing its two
/// passes, and the sea distance its reach.
pub const APRON_CELLS: usize = {
    let pool = 2 * POOL_RADIUS_CELLS + DEPTH_RADIUS_CELLS + 2 * SMOOTH_RADIUS_CELLS;
    // Ceiling division: a path of `SEA_REACH_M` spans at most this many cells.
    let sea = SEA_REACH_M.div_ceil(CELL_WHOLE_M) as usize;
    if pool > sea { pool } else { sea }
};
/// Nodes along the side of the square a tile bakes, apron included.
pub const DOMAIN_NODES: usize = TILE_NODES + 2 * APRON_CELLS;

/// Which tile: the square from `(x * TILE_M, z * TILE_M)` to one tile
/// further along both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    /// Tile index along world `x`.
    pub x: i32,
    /// Tile index along world `z`.
    pub z: i32,
}

impl TileCoord {
    /// The tile a world point lies in. A point on a shared edge belongs to
    /// the tile on its `+x` / `+z` side; both tiles give it the same value.
    #[must_use]
    pub fn containing(x: f64, z: f64) -> Self {
        // Floor of a world coordinate over 8 km fits an i32 for any world
        // this engine streams (f64 positions to 17 billion km).
        #[allow(clippy::cast_possible_truncation)]
        Self {
            x: (x / TILE_M).floor() as i32,
            z: (z / TILE_M).floor() as i32,
        }
    }

    /// Every tile a rectangle of the world touches, `x` fastest. What a
    /// renderer asks for before it fills a texture over that rectangle.
    pub fn covering(min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> impl Iterator<Item = Self> {
        let lo = Self::containing(min_x, min_z);
        let hi = Self::containing(max_x, max_z);
        (lo.z..=hi.z).flat_map(move |z| (lo.x..=hi.x).map(move |x| Self { x, z }))
    }

    /// The world position of the tile's first node, its `-x -z` corner.
    #[must_use]
    pub fn origin(self) -> (f64, f64) {
        (f64::from(self.x) * TILE_M, f64::from(self.z) * TILE_M)
    }
}

/// What the ground says about fog over one tile, whatever the weather.
///
/// [`TILE_NODES`] squared nodes, row-major with `x` fastest, [`CELL_M`]
/// apart from [`TileCoord::origin`]. A pure function of the world and the
/// coordinate: the same world gives the same bits on every machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasinTile {
    /// Which tile this is.
    pub coord: TileCoord,
    /// Ground height at each node in metres, the sea's surface where the
    /// ground is under it.
    pub ground: Vec<f32>,
    /// The fog's top under full weather, metres: a level across a basin,
    /// under its rim. At or under `ground` where nothing pools.
    pub top: Vec<f32>,
    /// The basin's floor, metres: its rim less the deepest the ground lies
    /// under the rim nearby. Weaker weather lowers the top towards it.
    pub floor: Vec<f32>,
    /// The ground's hydration averaged over a few hundred metres, `0..=1`.
    pub hydration: Vec<f32>,
    /// How far the sea is along low ground (under [`SEA_PASS_M`]), metres,
    /// [`SEA_REACH_M`] where it is further or walled off.
    pub sea_distance: Vec<f32>,
    /// How far the land is, metres: zero on land, the distance out over
    /// the water, [`OFFSHORE_CAP_M`] past that. Basin fog fades out over
    /// the first few hundred metres of water, because the sea is not a
    /// basin.
    pub offshore: Vec<f32>,
}

/// What a bake cost, for the log and the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BakeStats {
    /// Calls to [`WorldSource::sample`] (through `sample_grid`).
    pub samples: usize,
}

impl BasinTile {
    /// Bakes a tile: samples its square and the apron on the 128 m lattice
    /// and runs the basin rule. About 23 thousand samples (34 ms on one
    /// core of the dev machine, 27 of them sampling); pure, so any
    /// thread may run it.
    #[must_use]
    pub fn bake(world: &dyn WorldSource, coord: TileCoord) -> Self {
        Self::bake_with_stats(world, coord).0
    }

    /// [`bake`](Self::bake), also saying what it cost.
    #[must_use]
    pub fn bake_with_stats(world: &dyn WorldSource, coord: TileCoord) -> (Self, BakeStats) {
        let n = DOMAIN_NODES;
        let (ox, oz) = coord.origin();
        // The domain starts `APRON_CELLS` nodes before the tile: a whole
        // number of cells, so every node is at an exact multiple of 128 m
        // and a neighbour samples the same points.
        #[allow(clippy::cast_precision_loss)]
        let apron_m = APRON_CELLS as f64 * CELL_M;
        let mut samples = Vec::new();
        world.sample_grid(ox - apron_m, oz - apron_m, CELL_M, n, &mut samples);
        let sea = f64::from(world.sea_level());

        let heights: Vec<f64> = samples.iter().map(|s| f64::from(s.height)).collect();
        let ground = Grid::new(n, heights.iter().map(|&h| h.max(sea)).collect());
        let hydration = Grid::new(n, samples.iter().map(|s| f64::from(s.hydration)).collect());

        let r = POOL_RADIUS_CELLS;
        let s = SMOOTH_RADIUS_CELLS;
        let dilated = disk_extreme(&ground, r, true);
        let rim_raw = disk_extreme(&dilated, r, false);
        let smooth = |g: &Grid| box_blur(&box_blur(g, s), s);
        // Smoothing lifts the rim wherever it bends upwards, which it does
        // at every coast (the flat sea, then the land rising) and at the
        // foot of every slope, and a lifted rim is fog where nothing pools.
        // So the smoothed rim may stand over the ground by at most twice
        // the smoothed depth under the raw rim: nothing where the raw rim
        // is the ground, a slope of a few hundred metres where a basin
        // ends, and no effect inside one, where the depth is the larger.
        let depth_raw = Grid::new(
            n,
            rim_raw
                .v
                .iter()
                .zip(&ground.v)
                .map(|(&r, &g)| r - g)
                .collect(),
        );
        let depth = smooth(&depth_raw);
        let bound = Grid::new(
            n,
            ground
                .v
                .iter()
                .zip(&depth.v)
                .map(|(&g, &d)| g + EDGE_GAIN * d)
                .collect(),
        );
        let rim = smooth(&rim_raw).min(&bound);
        // The basin's depth is the deepest the ground lies under the rim
        // within half the disk. Inside a hollow that is one number, so the
        // floor (the rim less it) is as level as the rim; the lowest ground
        // within the disk would be the plain beyond the hollow, which
        // slopes with the region and would tilt the top.
        let deepest = smooth(&disk_extreme(&depth_raw, DEPTH_RADIUS_CELLS, true));
        let floor = Grid::new(
            n,
            rim.v.iter().zip(&deepest.v).map(|(&r, &d)| r - d).collect(),
        );
        let hydration = box_blur(&hydration, HYDRATION_RADIUS_CELLS);

        let source: Vec<bool> = heights.iter().map(|&h| h < sea).collect();
        let passable: Vec<bool> = ground.v.iter().map(|&g| g < sea + SEA_PASS_M).collect();
        let sea_distance = capped_distance(n, &source, &passable, CELL_WHOLE_M, SEA_REACH_M);
        let land: Vec<bool> = source.iter().map(|&s| !s).collect();
        let open = vec![true; n * n];
        let offshore = capped_distance(n, &land, &open, CELL_WHOLE_M, OFFSHORE_CAP_M);

        let len = TILE_NODES * TILE_NODES;
        let mut tile = Self {
            coord,
            ground: Vec::with_capacity(len),
            top: Vec::with_capacity(len),
            floor: Vec::with_capacity(len),
            hydration: Vec::with_capacity(len),
            sea_distance: Vec::with_capacity(len),
            offshore: Vec::with_capacity(len),
        };
        for j in 0..TILE_NODES {
            for i in 0..TILE_NODES {
                let idx = (j + APRON_CELLS) * n + i + APRON_CELLS;
                // No basin's floor is under the sea's surface: over water
                // the smoothing of the depth can take it there.
                let (rim, floor) = (rim.v[idx], floor.v[idx].max(sea).min(rim.v[idx]));
                let top = floor + TOP_FRACTION * (rim - floor);
                // Heights of metres and a unit share: f32 is the storage
                // precision of every field the renderer reads.
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                {
                    tile.ground.push(ground.v[idx] as f32);
                    tile.top.push(top as f32);
                    tile.floor.push(floor as f32);
                    tile.hydration.push(hydration.v[idx] as f32);
                    tile.sea_distance.push(sea_distance[idx] as f32);
                    tile.offshore.push(offshore[idx] as f32);
                }
            }
        }
        (tile, BakeStats { samples: n * n })
    }

    /// Whether every channel holds [`TILE_NODES`] squared values, which
    /// sampling indexes without checking. A bake always does; a tile that
    /// came from elsewhere (the fields are public and the type
    /// deserializes) is checked by [`crate::FogField::insert`], so a bad
    /// one is refused there and not a panic in the middle of a sample.
    ///
    /// # Errors
    ///
    /// The first channel whose length is wrong.
    pub fn check(&self) -> Result<(), MalformedTile> {
        let want = TILE_NODES * TILE_NODES;
        for (channel, values) in [
            ("ground", &self.ground),
            ("top", &self.top),
            ("floor", &self.floor),
            ("hydration", &self.hydration),
            ("sea_distance", &self.sea_distance),
            ("offshore", &self.offshore),
        ] {
            if values.len() != want {
                return Err(MalformedTile {
                    coord: self.coord,
                    channel,
                    len: values.len(),
                });
            }
        }
        Ok(())
    }

    /// The six channels bilinearly interpolated at a world point, which
    /// must lie in this tile (edges included).
    pub(crate) fn lerp(&self, x: f64, z: f64) -> Nodes {
        let (ox, oz) = self.coord.origin();
        let cells = TILE_M / CELL_M;
        let across = ((x - ox) / CELL_M).clamp(0.0, cells);
        let down = ((z - oz) / CELL_M).clamp(0.0, cells);
        // In 0..=64 after the clamp.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (col, row) = (
            (across.floor() as usize).min(TILE_CELLS - 1),
            (down.floor() as usize).min(TILE_CELLS - 1),
        );
        #[allow(clippy::cast_precision_loss)]
        let (fx, fz) = (across - col as f64, down - row as f64);
        let at = |ch: &[f32]| {
            let first = row * TILE_NODES + col;
            let second = first + TILE_NODES;
            let near =
                f64::from(ch[first]) + (f64::from(ch[first + 1]) - f64::from(ch[first])) * fx;
            let far =
                f64::from(ch[second]) + (f64::from(ch[second + 1]) - f64::from(ch[second])) * fx;
            near + (far - near) * fz
        };
        Nodes {
            ground: at(&self.ground),
            top: at(&self.top),
            floor: at(&self.floor),
            hydration: at(&self.hydration),
            sea_distance: at(&self.sea_distance),
            offshore: at(&self.offshore),
        }
    }

    /// A 64-bit FNV-1a hash of every channel's bits, in a fixed order: what
    /// the determinism test records.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for channel in [
            &self.ground,
            &self.top,
            &self.floor,
            &self.hydration,
            &self.sea_distance,
            &self.offshore,
        ] {
            for value in channel {
                for byte in value.to_bits().to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        hash
    }
}

/// A tile whose channels are not [`TILE_NODES`] squared long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedTile {
    /// Which tile.
    pub coord: TileCoord,
    /// The first channel with the wrong length.
    pub channel: &'static str,
    /// Its length.
    pub len: usize,
}

impl std::fmt::Display for MalformedTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fog tile ({}, {}): {} holds {} values, not {}",
            self.coord.x,
            self.coord.z,
            self.channel,
            self.len,
            TILE_NODES * TILE_NODES
        )
    }
}

impl std::error::Error for MalformedTile {}

/// A tile's channels at one point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Nodes {
    pub ground: f64,
    pub top: f64,
    pub floor: f64,
    pub hydration: f64,
    pub sea_distance: f64,
    pub offshore: f64,
}
