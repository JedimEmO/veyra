//! How dramatic a world is, as numbers (ticket 0004, step 1).
//!
//! "The terrain is boring" became a plan only once it was measured, and a
//! plan that raises the ground is only held to its word by the same
//! measures run on every change. These are the research probe's
//! definitions (`scratch/research/terrain/relief-probe`), moved here so a
//! test can pin them and the map tool can draw them:
//!
//! - **the tallest thing in view**: from a standing eye, the elevation
//!   angle of the highest ground out to [`VIEW_RANGE_M`] in any direction;
//!   over a set of points its median and the share of land seeing at least
//!   5, 10 and 20 degrees ([`VIEW_THRESHOLDS_DEG`]);
//! - **relief**: the range of heights in a square about a point, 1 km and
//!   5 km to each side, medians over the set;
//! - **slope classes**: the share of land by the angle of its ground, and
//!   the share a character can walk on ([`WALKABLE_SLOPE_DEG`]);
//! - **biome shares**: the share of land each biome wins, and each biome's
//!   mean slope, both the ground's at a metre and its own relief's
//!   ([`Measures::biome_relief_slope`]), which is how "the ranges stand in
//!   foothills" became a number (ticket 0004, step 2);
//! - **provinces' landforms**: per geological province, the share of its
//!   ground that is flat ([`FLAT_DEG`]) and steep ([`STEEP_DEG`]), its
//!   relief within 1 km, and on tiles of its ground away from the
//!   mountains ([`province_tiles`]) the closed hollows a square kilometre
//!   and the elongation of its relief's grain: what tells a sandstone
//!   country of mesas from a karst of hollows from a folded belt of ridges
//!   (ticket 0004, step 4).
//!
//! The crate's float rule binds the world, not a measure, but a measure is
//! pinned by a snapshot too, so every transcendental here goes through
//! `libm` as the world's do: the same points give the same bits on every
//! machine.
//!
//! A measure is expensive (a point's view is about two thousand samples),
//! so [`measure`] spreads its points over every core with scoped threads
//! and gathers them back in order: the result does not depend on how many
//! cores ran it.

use strata_registry::{BiomeId, ProvinceId};

use crate::WorldSource;

/// Height of the eye over the ground, in metres: the character's capsule
/// is 1.7 m tall (`strata_physics::CharacterController::height`). The
/// probe used 1.8, a camera's; the difference moves the median view by a
/// few hundredths of a degree.
pub const EYE_HEIGHT_M: f64 = 1.7;

/// How far a view looks, in metres. Ground further out is lost in the haze
/// and under the curve of the planet, and the probe's baseline stops here.
pub const VIEW_RANGE_M: f64 = 12_000.0;

/// Directions a view looks in, evenly round the circle.
pub const VIEW_AZIMUTHS: u32 = 36;

/// The first step of a view's march, in metres. Nearer ground is the
/// ground the character stands on, not something it looks at.
pub const VIEW_FIRST_STEP_M: f64 = 20.0;

/// Each step of a view's march is this much further than the last: fine
/// near the eye, where a small rise is a large angle, coarse far out.
/// Fifty-seven steps reach [`VIEW_RANGE_M`].
pub const VIEW_STEP_GROWTH: f64 = 1.12;

/// The angles a share of land is counted against, in degrees.
pub const VIEW_THRESHOLDS_DEG: [f64; 3] = [5.0, 10.0, 20.0];

/// Half the side of the near relief square, in metres, and its lattice:
/// `2n + 1` points a side.
pub const RELIEF_NEAR: (f64, u32) = (1_000.0, 8);

/// Half the side of the far relief square, in metres, and its lattice.
pub const RELIEF_FAR: (f64, u32) = (5_000.0, 10);

/// The upper bounds of the slope classes, in degrees; the last class is
/// everything steeper than the last bound.
pub const SLOPE_CLASSES_DEG: [f64; 5] = [2.0, 5.0, 10.0, 30.0, 50.0];

/// Ground steeper than this, in degrees, is a wall to the character:
/// `CharacterController::max_slope` in `crates/strata-physics/src/
/// character.rs` is `50.0f32.to_radians()`, and `is_walkable` takes a
/// normal whose `y` is at least its cosine. Equal to the last slope class
/// bound, so the walkable share is the sum of every class but the last.
pub const WALKABLE_SLOPE_DEG: f64 = 50.0;

/// A point is land when its ground stands this far over the sea, in
/// metres: the probe's filter, which keeps the wet sand of a beach out of
/// the land's statistics.
pub const LAND_MIN_M: f64 = 2.0;

/// The distance of a slope's central difference, in metres: the finest
/// mesh and the collider are one metre apart, so this is the slope the
/// character stands on.
pub const SLOPE_STEP_M: f64 = 1.0;

/// Half the side of the square the standard set is scattered over, in
/// metres, centred on the origin: the probe's 50 km.
pub const STANDARD_HALF_EXTENT_M: f64 = 50_000.0;

/// Land points the standard set's views and reliefs are measured at. The
/// probe used 4 000; a debug test build has to finish in a few seconds, and
/// at this count a share of 14 % is known to about a percentage point.
pub const STANDARD_VIEW_POINTS: usize = 1_200;

/// Land points the standard set's slopes and biomes are measured at. They
/// cost five samples each, so they can be many more than the views.
pub const STANDARD_GROUND_POINTS: usize = 20_000;

/// Ground flatter than this, in degrees, is flat: a bench, a mesa's top, a
/// plateau (ticket 0004, step 4).
pub const FLAT_DEG: f64 = 2.0;

/// Ground steeper than this, in degrees, is steep: a bluff, an
/// escarpment, a hollow's wall.
pub const STEEP_DEG: f64 = 20.0;

/// Ground steeper than this, in degrees, is a riser: a cliff band a
/// character cannot climb, a bed's face (ticket 0004, step 5).
pub const RISER_DEG: f64 = 45.0;

/// How far [`riser_height_m`] walks up or down a riser, metres, at most.
pub const RISER_WALK_M: f64 = 200.0;

/// A point counts for a province where the province weighs at least this
/// there: its own ground, not a border's blend.
pub const PROVINCE_ALONE: f32 = 0.999;

/// The points a set of measures is taken at.
#[derive(Debug, Clone, PartialEq)]
pub struct PointSet {
    /// Where views and reliefs are measured, `[x, z]` in metres.
    pub view: Vec<[f64; 2]>,
    /// Where slopes and biomes are measured.
    pub ground: Vec<[f64; 2]>,
}

impl PointSet {
    /// A fixed scatter of land points over the square of half side
    /// `half_extent_m` about the origin: `view_count` for the views and
    /// reliefs, `ground_count` for the slopes and biomes.
    ///
    /// The scatter is a stream of hashed indices, the probe's own, filtered
    /// to land ([`LAND_MIN_M`]) and cut at the count, so a smaller set is
    /// the start of a larger one and the probe's baseline set is
    /// `scatter(world, 50_000.0, 4_000, _)`. Stops early, with fewer points,
    /// if a world has almost no land (after sixty-four candidates a point
    /// asked for).
    #[must_use]
    pub fn scatter<W: WorldSource + ?Sized>(
        world: &W,
        half_extent_m: f64,
        view_count: usize,
        ground_count: usize,
    ) -> Self {
        let land = |x: f64, z: f64| f64::from(world.sample(x, z).height) > LAND_MIN_M;
        let take = |count: usize, index: &dyn Fn(u64) -> (u64, u64)| {
            let mut points = Vec::with_capacity(count);
            let mut index_in_stream = 0_u64;
            let cap = u64::try_from(count).unwrap_or(u64::MAX).saturating_mul(64);
            while points.len() < count && index_in_stream < cap {
                let (hash_x, hash_z) = index(index_in_stream);
                let point = [
                    (unit_hash(hash_x) * 2.0 - 1.0) * half_extent_m,
                    (unit_hash(hash_z) * 2.0 - 1.0) * half_extent_m,
                ];
                if land(point[0], point[1]) {
                    points.push(point);
                }
                index_in_stream += 1;
            }
            points
        };
        Self {
            // The probe's `stats` and `slopes` streams.
            view: take(view_count, &|i| (2 * i, 2 * i + 1)),
            ground: take(ground_count, &|i| (7 * i + 3, 7 * i + 4)),
        }
    }

    /// The set the worldgen tests hold the world to: the standard counts
    /// over the standard square.
    #[must_use]
    pub fn standard<W: WorldSource + ?Sized>(world: &W) -> Self {
        Self::scatter(
            world,
            STANDARD_HALF_EXTENT_M,
            STANDARD_VIEW_POINTS,
            STANDARD_GROUND_POINTS,
        )
    }
}

/// The measures of a world over a [`PointSet`]. Shares are of the points
/// measured, `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Measures {
    /// View points measured.
    pub view_points: usize,
    /// Ground points measured.
    pub ground_points: usize,
    /// The median over the view points of the tallest thing in view, in
    /// degrees over level.
    pub view_median_deg: f64,
    /// The share of view points seeing at least each of
    /// [`VIEW_THRESHOLDS_DEG`].
    pub view_shares: [f64; 3],
    /// The median range of heights within [`RELIEF_NEAR`], in metres.
    pub relief_near_median_m: f64,
    /// The median range of heights within [`RELIEF_FAR`], in metres.
    pub relief_far_median_m: f64,
    /// The share of ground points in each slope class: under each bound of
    /// [`SLOPE_CLASSES_DEG`], then over the last.
    pub slope_shares: [f64; 6],
    /// The share of ground points at most [`WALKABLE_SLOPE_DEG`] steep.
    pub walkable_share: f64,
    /// The share of ground points each biome wins, by id, only the biomes
    /// that win any.
    pub biome_shares: Vec<(BiomeId, f64)>,
    /// Each biome's mean slope over the ground points it wins, in degrees,
    /// as [`slope_deg`] takes it: the whole ground a character stands on,
    /// a range's flank and the erosion's gullies included. In the order of
    /// [`biome_shares`](Self::biome_shares).
    pub biome_slopes_deg: Vec<(BiomeId, f64)>,
    /// Each biome's mean slope over the ground points it wins, in degrees,
    /// of [`Landform::slope`](crate::Landform::slope): the biome's own relief
    /// and its gullies, without the continent's gradient under them. It
    /// tells hills from a hillside: foothills on a range's flank measure
    /// steep in [`biome_slopes_deg`](Self::biome_slopes_deg) whether or not
    /// they are hills, and only this says whether they are. The ticket's
    /// "the highlands measure a mean slope of 2.8" is this measure.
    pub biome_relief_slopes_deg: Vec<(BiomeId, f64)>,
    /// Each province's own ground (where it weighs at least
    /// [`PROVINCE_ALONE`]), by id, only the provinces that have any.
    pub provinces: Vec<ProvinceLand>,
}

/// A province's ground in a set of measures.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvinceLand {
    /// Which.
    pub province: ProvinceId,
    /// Ground points on it.
    pub ground_points: usize,
    /// The share of them flatter than [`FLAT_DEG`].
    pub flat_share: f64,
    /// The share of them steeper than [`STEEP_DEG`].
    pub steep_share: f64,
    /// View points on it.
    pub view_points: usize,
    /// The median range of heights within [`RELIEF_NEAR`] over them, in
    /// metres; `NaN` with none.
    pub relief_near_median_m: f64,
    /// Of its steep ground ([`STEEP_DEG`]), the share that is a riser
    /// ([`RISER_DEG`]): where the steep ground of a country of beds is
    /// cliff bands and not slopes (ticket 0004, step 5). Zero with no steep
    /// ground.
    pub riser_share: f64,
    /// The mean height of the risers its riser points stand on
    /// ([`riser_height_m`]), metres; `NaN` with none. A point on a tall
    /// riser is likelier to be drawn, so this leans towards the tall ones.
    pub riser_mean_m: f64,
}

impl Measures {
    /// The share of land `biome` wins, zero if it wins none.
    #[must_use]
    pub fn biome_share(&self, biome: BiomeId) -> f64 {
        self.biome_shares
            .iter()
            .find(|(id, _)| *id == biome)
            .map_or(0.0, |(_, share)| *share)
    }

    /// `biome`'s mean slope at a metre, in degrees; zero if it wins none.
    #[must_use]
    pub fn biome_slope(&self, biome: BiomeId) -> f64 {
        lookup(&self.biome_slopes_deg, biome)
    }

    /// `biome`'s mean relief slope ([`Landform::slope`](crate::Landform::slope)),
    /// in degrees; zero if it wins none.
    #[must_use]
    pub fn biome_relief_slope(&self, biome: BiomeId) -> f64 {
        lookup(&self.biome_relief_slopes_deg, biome)
    }

    /// `province`'s ground, if the set has any.
    #[must_use]
    pub fn province(&self, province: ProvinceId) -> Option<&ProvinceLand> {
        self.provinces.iter().find(|p| p.province == province)
    }
}

fn lookup(values: &[(BiomeId, f64)], biome: BiomeId) -> f64 {
    values
        .iter()
        .find(|(id, _)| *id == biome)
        .map_or(0.0, |(_, value)| *value)
}

/// The most threads any measure runs on, whatever it is asked for. A debug
/// build on the development machine (an i9-13900KF) crashes under heavy
/// multi-thread load: a SIGSEGV with all 32 threads in the sampler, or with
/// several pools at once. That is the CPU, not the code: rerun, do not
/// trace it. So a measure never takes more than this, and only one runs at
/// a time in a process (the private `parallel_map` holds a lock).
pub use strata_core::platform::par::MAX_THREADS;

/// The measures of `world` over `points`, on every core up to
/// [`MAX_THREADS`].
#[must_use]
pub fn measure<W: WorldSource + ?Sized>(world: &W, points: &PointSet) -> Measures {
    measure_on(world, points, strata_core::platform::par::threads())
}

/// The measures of `world` over `points` on at most `threads` threads
/// (at least one, at most [`MAX_THREADS`]). The result is the same for any
/// count.
#[must_use]
pub fn measure_on<W: WorldSource + ?Sized>(
    world: &W,
    points: &PointSet,
    threads: usize,
) -> Measures {
    let alone = |sample: &crate::Sample| {
        let (id, w) = sample.provinces[0];
        (!id.is_none() && w >= PROVINCE_ALONE).then_some(id)
    };
    let views = parallel_map(threads, &points.view, |[x, z]| {
        (
            view_deg(world, x, z),
            relief_m(world, x, z, RELIEF_NEAR.0, RELIEF_NEAR.1),
            relief_m(world, x, z, RELIEF_FAR.0, RELIEF_FAR.1),
            alone(&world.sample(x, z)),
        )
    });
    let grounds = parallel_map(threads, &points.ground, |[x, z]| {
        let sample = world.sample(x, z);
        let slope = slope_deg(world, x, z);
        let province = alone(&sample);
        (
            slope,
            sample.dominant_biome(),
            libm::atan(f64::from(sample.landform.slope)).to_degrees(),
            province,
            (province.is_some() && slope > RISER_DEG).then(|| riser_height_m(world, x, z)),
        )
    });
    let provinces = province_lands(&views, &grounds);

    let mut view: Vec<f64> = views.iter().map(|v| v.0).collect();
    let mut near: Vec<f64> = views.iter().map(|v| v.1).collect();
    let mut far: Vec<f64> = views.iter().map(|v| v.2).collect();
    let view_shares =
        VIEW_THRESHOLDS_DEG.map(|t| share(views.len(), view.iter().filter(|&&v| v >= t).count()));

    let mut classes = [0_usize; 6];
    let mut walkable = 0;
    // Per biome: points won, and the sums of both slopes over them.
    let mut biomes: Vec<(BiomeId, usize, f64, f64)> = Vec::new();
    for &(slope, biome, relief_slope, ..) in &grounds {
        let class = SLOPE_CLASSES_DEG
            .iter()
            .position(|&bound| slope < bound)
            .unwrap_or(SLOPE_CLASSES_DEG.len());
        classes[class] += 1;
        walkable += usize::from(slope <= WALKABLE_SLOPE_DEG);
        match biomes.iter_mut().find(|(id, ..)| *id == biome) {
            Some((_, count, sum, relief_sum)) => {
                *count += 1;
                *sum += slope;
                *relief_sum += relief_slope;
            }
            None => biomes.push((biome, 1, slope, relief_slope)),
        }
    }
    biomes.sort_by_key(|(id, ..)| *id);
    // A mean over a point count; exact in f64 far beyond any set.
    #[allow(clippy::cast_precision_loss, reason = "point counts")]
    let mean = |sum: f64, count: usize| sum / count as f64;

    Measures {
        view_points: views.len(),
        ground_points: grounds.len(),
        view_median_deg: median(&mut view),
        view_shares,
        relief_near_median_m: median(&mut near),
        relief_far_median_m: median(&mut far),
        slope_shares: classes.map(|count| share(grounds.len(), count)),
        walkable_share: share(grounds.len(), walkable),
        biome_shares: biomes
            .iter()
            .map(|&(id, count, ..)| (id, share(grounds.len(), count)))
            .collect(),
        biome_slopes_deg: biomes
            .iter()
            .map(|&(id, count, sum, _)| (id, mean(sum, count)))
            .collect(),
        biome_relief_slopes_deg: biomes
            .iter()
            .map(|&(id, count, _, relief_sum)| (id, mean(relief_sum, count)))
            .collect(),
        provinces,
    }
}

/// Each province's ground from the view and ground points that stand on it
/// alone, by id.
fn province_lands(
    views: &[(f64, f64, f64, Option<ProvinceId>)],
    grounds: &[(f64, BiomeId, f64, Option<ProvinceId>, Option<f64>)],
) -> Vec<ProvinceLand> {
    let mut ids: Vec<ProvinceId> = grounds
        .iter()
        .filter_map(|g| g.3)
        .chain(views.iter().filter_map(|v| v.3))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let slopes: Vec<f64> = grounds
                .iter()
                .filter(|g| g.3 == Some(id))
                .map(|g| g.0)
                .collect();
            let risers: Vec<f64> = grounds
                .iter()
                .filter(|g| g.3 == Some(id))
                .filter_map(|g| g.4)
                .collect();
            let steep = slopes.iter().filter(|&&s| s > STEEP_DEG).count();
            #[allow(clippy::cast_precision_loss, reason = "point counts")]
            let riser_mean_m = if risers.is_empty() {
                f64::NAN
            } else {
                risers.iter().sum::<f64>() / risers.len() as f64
            };
            let mut reliefs: Vec<f64> = views
                .iter()
                .filter(|v| v.3 == Some(id))
                .map(|v| v.1)
                .collect();
            ProvinceLand {
                province: id,
                ground_points: slopes.len(),
                flat_share: share(
                    slopes.len(),
                    slopes.iter().filter(|&&s| s < FLAT_DEG).count(),
                ),
                steep_share: share(
                    slopes.len(),
                    slopes.iter().filter(|&&s| s > STEEP_DEG).count(),
                ),
                view_points: reliefs.len(),
                relief_near_median_m: median(&mut reliefs),
                riser_share: share(steep, risers.len()),
                riser_mean_m,
            }
        })
        .collect()
}

/// The side of a tile [`province_tiles`] measures, in metres.
pub const TILE_M: f64 = 4_000.0;

/// The lattice spacing within a tile, in metres: fine enough to resolve a
/// hollow a few hundred metres across.
pub const TILE_STEP_M: f64 = 50.0;

/// A closed hollow is at least this deep, in metres, from its bottom to
/// the lowest point of its rim: the dimples of rolling ground are not
/// hollows.
pub const HOLLOW_DEPTH_M: f64 = 5.0;

/// A province's tiles: what its ground's grain is.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvinceTiles {
    /// Which.
    pub province: ProvinceId,
    /// Tiles measured.
    pub tiles: usize,
    /// Their area, in square kilometres.
    pub area_km2: f64,
    /// Closed hollows at least [`HOLLOW_DEPTH_M`] deep on them.
    pub hollows: usize,
    /// ... a square kilometre.
    pub hollows_per_km2: f64,
    /// How much longer than wide the relief's features are: the square
    /// root of the ratio of the eigenvalues of the relief's gradient
    /// tensor ([`Landform::slope`](crate::Landform::slope) along its aspect,
    /// which leaves out the continent's base elevation and holds a range's
    /// own metres; the tiles are away from the mountains, where those are
    /// small), pooled over the tiles. Round ground is one; ridges all along
    /// one grain are several.
    pub elongation: f64,
}

/// Tiles of [`TILE_M`] of each province's ground within `radius_m` of the
/// origin, up to `per_province` each, and what is on them.
///
/// A tile is a province's when a 5 by 5 lattice over it stands on that
/// province alone ([`PROVINCE_ALONE`]) and every point of that lattice is
/// one `keep` accepts (land, away from the mountains, say, whose flanks
/// would read as grain). Tiles are taken in a hashed order, so the set
/// does not favour one corner. On each, a lattice of [`TILE_STEP_M`]:
/// closed hollows by a priority flood from the tile's edge (a hollow is a
/// connected pit at least [`HOLLOW_DEPTH_M`] deep), and the relief's
/// gradient tensor. On up to `threads` threads; the result does not depend
/// on the count.
#[must_use]
pub fn province_tiles<W: WorldSource + ?Sized>(
    world: &W,
    keep: impl Fn(&crate::Sample) -> bool + Sync,
    radius_m: f64,
    per_province: usize,
    threads: usize,
) -> Vec<ProvinceTiles> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a lattice of a few dozen tiles a side"
    )]
    let n = (radius_m / TILE_M).floor() as i64;
    #[allow(clippy::cast_precision_loss, reason = "small lattice indices")]
    let centres: Vec<[f64; 2]> = (-n..=n)
        .flat_map(|j| (-n..=n).map(move |i| [i as f64 * TILE_M, j as f64 * TILE_M]))
        .filter(|[x, z]| x * x + z * z <= radius_m * radius_m)
        .collect();
    let owners = parallel_map(threads, &centres, |[cx, cz]| {
        let mut owner = None;
        for j in 0..5 {
            for i in 0..5 {
                let (x, z) = (
                    cx + (f64::from(i) / 4.0 - 0.5) * TILE_M,
                    cz + (f64::from(j) / 4.0 - 0.5) * TILE_M,
                );
                let sample = world.sample(x, z);
                let (id, w) = sample.provinces[0];
                if id.is_none() || w < PROVINCE_ALONE || !keep(&sample) {
                    return None;
                }
                if owner.is_some_and(|o| o != id) {
                    return None;
                }
                owner = Some(id);
            }
        }
        owner
    });
    let mut ids: Vec<ProvinceId> = owners.iter().filter_map(|o| *o).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let mut mine: Vec<(u64, [f64; 2])> = centres
                .iter()
                .zip(&owners)
                .enumerate()
                .filter(|(_, (_, o))| **o == Some(id))
                .map(|(k, (c, _))| (unit_hash(k as u64 ^ 0x5EED).to_bits(), *c))
                .collect();
            mine.sort_unstable_by_key(|(h, _)| *h);
            mine.truncate(per_province);
            let tiles: Vec<[f64; 2]> = mine.into_iter().map(|(_, c)| c).collect();
            let measured = parallel_map(threads, &tiles, |c| tile(world, c));
            let hollows: usize = measured.iter().map(|m| m.0).sum();
            let mut tensor = [0.0; 3];
            for m in &measured {
                for (sum, part) in tensor.iter_mut().zip(m.1) {
                    *sum += part;
                }
            }
            #[allow(clippy::cast_precision_loss, reason = "tile counts")]
            let area_km2 = tiles.len() as f64 * TILE_M * TILE_M / 1e6;
            #[allow(clippy::cast_precision_loss, reason = "hollow counts")]
            let per_km2 = if area_km2 > 0.0 {
                hollows as f64 / area_km2
            } else {
                f64::NAN
            };
            ProvinceTiles {
                province: id,
                tiles: tiles.len(),
                area_km2,
                hollows,
                hollows_per_km2: per_km2,
                elongation: tensor_elongation(tensor),
            }
        })
        .collect()
}

/// One tile's closed hollows and its relief's gradient tensor, `[xx, xz,
/// zz]` summed.
fn tile<W: WorldSource + ?Sized>(world: &W, [cx, cz]: [f64; 2]) -> (usize, [f64; 3]) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "eighty steps"
    )]
    let steps = (TILE_M / TILE_STEP_M).round() as usize;
    let side = steps + 1;
    let sea = f64::from(world.sea_level());
    let mut heights = Vec::with_capacity(side * side);
    let mut tensor = [0.0; 3];
    for j in 0..side {
        for i in 0..side {
            #[allow(clippy::cast_precision_loss, reason = "small lattice indices")]
            let (x, z) = (
                cx - TILE_M * 0.5 + i as f64 * TILE_STEP_M,
                cz - TILE_M * 0.5 + j as f64 * TILE_STEP_M,
            );
            let sample = world.sample(x, z);
            heights.push(f64::from(sample.height).max(sea));
            let slope = f64::from(sample.landform.slope);
            let (ax, az) = (
                f64::from(sample.landform.aspect[0]),
                f64::from(sample.landform.aspect[1]),
            );
            let (gx, gz) = (-ax * slope, -az * slope);
            tensor[0] += gx * gx;
            tensor[1] += gx * gz;
            tensor[2] += gz * gz;
        }
    }
    (closed_hollows(&heights, side), tensor)
}

/// The square root of the ratio of a symmetric 2 by 2 tensor's eigenvalues,
/// `[xx, xz, zz]`; infinite for a degenerate one.
fn tensor_elongation([a, b, c]: [f64; 3]) -> f64 {
    let mid = f64::midpoint(a, c);
    let spread = ((a - c) * 0.5 * (a - c) * 0.5 + b * b).sqrt();
    ((mid + spread) / (mid - spread)).sqrt()
}

/// Closed hollows in a square lattice of heights `side` a side: a priority
/// flood from the edge fills every pit to its spill level, and a hollow is
/// an eight-connected set of filled points whose deepest fill is at least
/// [`HOLLOW_DEPTH_M`].
fn closed_hollows(heights: &[f64], side: usize) -> usize {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    /// A height ordered totally, for the heap.
    #[derive(PartialEq)]
    struct Level(f64);
    impl Eq for Level {}
    impl PartialOrd for Level {
        fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Level {
        fn cmp(&self, other: &Self) -> core::cmp::Ordering {
            self.0.total_cmp(&other.0)
        }
    }

    let neighbours = |k: usize| {
        let (i, j) = (k % side, k / side);
        let mut out = [None; 8];
        let mut n = 0;
        for dj in -1_isize..=1 {
            for di in -1_isize..=1 {
                if di == 0 && dj == 0 {
                    continue;
                }
                if let (Some(ni), Some(nj)) = (i.checked_add_signed(di), j.checked_add_signed(dj))
                    && ni < side
                    && nj < side
                {
                    out[n] = Some(nj * side + ni);
                }
                n += 1;
            }
        }
        out
    };
    let mut fill = heights.to_vec();
    let mut done = vec![false; heights.len()];
    let mut heap = BinaryHeap::new();
    for k in 0..heights.len() {
        let (i, j) = (k % side, k / side);
        if i == 0 || j == 0 || i + 1 == side || j + 1 == side {
            done[k] = true;
            heap.push(Reverse((Level(heights[k]), k)));
        }
    }
    while let Some(Reverse((Level(level), k))) = heap.pop() {
        for m in neighbours(k).into_iter().flatten() {
            if !done[m] {
                done[m] = true;
                fill[m] = heights[m].max(level);
                heap.push(Reverse((Level(fill[m]), m)));
            }
        }
    }
    let depth: Vec<f64> = fill.iter().zip(heights).map(|(f, h)| f - h).collect();
    let mut seen = vec![false; heights.len()];
    let mut count = 0;
    let mut stack = Vec::new();
    for start in 0..heights.len() {
        if seen[start] || depth[start] <= 0.0 {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut deepest = 0.0_f64;
        while let Some(k) = stack.pop() {
            deepest = deepest.max(depth[k]);
            for m in neighbours(k).into_iter().flatten() {
                if !seen[m] && depth[m] > 0.0 {
                    seen[m] = true;
                    stack.push(m);
                }
            }
        }
        count += usize::from(deepest >= HOLLOW_DEPTH_M);
    }
    count
}

/// The tallest thing in view from an eye [`EYE_HEIGHT_M`] over the ground
/// at `(x, z)`: the largest elevation angle, in degrees, of any ground
/// within [`VIEW_RANGE_M`] along [`VIEW_AZIMUTHS`] directions. The sea
/// counts as its surface, so a view over water is level at best, and from
/// a hilltop the answer is negative.
///
/// A lower bound, and the probe's definition: ten degrees between
/// directions and steps that grow with distance miss a peak between them.
/// Against a march of 720 directions at 5 m steps it read 19.7 degrees for
/// 25.6 on a plain at a massif's foot and 5.2 for 8.8 at another, and
/// within about a degree elsewhere (ticket 0004's review of step 2). The
/// shares and gates it feeds are conservative for that; an absolute angle
/// quoted from it is low.
#[must_use]
pub fn view_deg<W: WorldSource + ?Sized>(world: &W, x: f64, z: f64) -> f64 {
    view_deg_where(world, x, z, |_, _| true)
}

/// [`view_deg`] counting only the ground at points `looked_at` keeps.
///
/// For a view out of a place that brings its own ground: from Port Serein's
/// waterfront the city's own hillside, a hundred metres over the quay half
/// a kilometre off, stands higher in the view than anything the world has,
/// and the question is what the world gives the city to look at (ticket
/// 0004, step 2b). The eye still stands on the place's ground. Negative
/// infinity if nothing is kept.
#[must_use]
pub fn view_deg_where<W: WorldSource + ?Sized>(
    world: &W,
    x: f64,
    z: f64,
    looked_at: impl Fn(f64, f64) -> bool,
) -> f64 {
    let sea = f64::from(world.sea_level());
    let ground = |x: f64, z: f64| f64::from(world.sample(x, z).height).max(sea);
    let eye = ground(x, z) + EYE_HEIGHT_M;
    // The steepest rise over run seen, compared as a tangent so there is
    // one arctangent per point rather than one per step.
    let mut steepest = f64::NEG_INFINITY;
    for a in 0..VIEW_AZIMUTHS {
        let angle = f64::from(a) * core::f64::consts::TAU / f64::from(VIEW_AZIMUTHS);
        let (dx, dz) = (libm::cos(angle), libm::sin(angle));
        let mut d = VIEW_FIRST_STEP_M;
        while d < VIEW_RANGE_M {
            let (px, pz) = (x + dx * d, z + dz * d);
            if looked_at(px, pz) {
                steepest = steepest.max((ground(px, pz) - eye) / d);
            }
            d *= VIEW_STEP_GROWTH;
        }
    }
    if steepest == f64::NEG_INFINITY {
        // `atan` would make it -90 degrees, which reads as a view.
        return f64::NEG_INFINITY;
    }
    libm::atan(steepest).to_degrees()
}

/// The side of the lattice cells mountain ground is gathered into patches
/// on, in metres (ticket 0004, step 2b).
pub const PATCH_CELL_M: f64 = 500.0;

/// How far from the origin patches are gathered, in metres: the patches are
/// the cells of the lattice within this disc.
pub const PATCH_RADIUS_M: f64 = 100_000.0;

/// The smallest patch counted, in square kilometres: eighty cells. A knoll
/// of mountain ground is not a range and says nothing about its shape.
pub const PATCH_MIN_AREA_KM2: f64 = 20.0;

/// A patch over this many square kilometres is a range or a massif whatever
/// its shape; the gates ask such a patch to be long.
pub const PATCH_LARGE_AREA_KM2: f64 = 200.0;

/// One connected patch of ground, and its shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// Lattice cells in the patch.
    pub cells: usize,
    /// Its area, in square kilometres.
    pub area_km2: f64,
    /// The mean of its cells' positions, `[x, z]` in metres.
    pub centroid: [f64; 2],
    /// The greater over the lesser of the two principal standard deviations
    /// of its cells' positions: a disc is one, a line is large. Each cell
    /// counts as the square it stands for (its own variance, a twelfth of
    /// its side squared, is added on both axes), so a patch one cell wide is
    /// long and not infinitely so.
    pub elongation: f64,
    /// The highest of its lattice points, `[x, z, height]`.
    pub crest: [f64; 3],
    /// Whether it reaches the disc's edge, where it is cut and its shape is
    /// only the part inside.
    pub cut: bool,
}

/// The connected patches of ground that `keep` accepts, on a lattice of
/// `cell_m` about the origin within `radius_m`, four-connected, at least
/// `min_area_km2` each, largest first.
///
/// What a range is, as a number: the mountains of ticket 0004's step 2 were
/// massifs, blobs tens of kilometres across, and the step that made them
/// ranges had to show it by more than a picture. The lattice is sampled on
/// up to `threads` threads; the result does not depend on the count.
#[must_use]
pub fn patches<W: WorldSource + ?Sized>(
    world: &W,
    keep: impl Fn(&crate::Sample) -> bool + Sync,
    radius_m: f64,
    cell_m: f64,
    min_area_km2: f64,
    threads: usize,
) -> Vec<Patch> {
    // Lattice sizes are a few hundred; exact in f64 and in i64.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a lattice of a few hundred cells a side"
    )]
    let n = (radius_m / cell_m).floor() as usize;
    let side = 2 * n + 1;
    #[allow(clippy::cast_precision_loss, reason = "small lattice indices")]
    let at = |i: usize| (i as f64 - n as f64) * cell_m;
    let points: Vec<[f64; 2]> = (0..side * side)
        .map(|k| [at(k % side), at(k / side)])
        .collect();
    let radius2 = radius_m * radius_m;
    let inside = |[x, z]: [f64; 2]| x * x + z * z <= radius2;
    let heights = parallel_map(threads, &points, |p| {
        if !inside(p) {
            return None;
        }
        let sample = world.sample(p[0], p[1]);
        keep(&sample).then_some(f64::from(sample.height))
    });
    let cell_km2 = cell_m * cell_m / 1e6;
    let mut seen = vec![false; side * side];
    let mut found = Vec::new();
    let mut stack = Vec::new();
    for start in 0..side * side {
        if seen[start] || heights[start].is_none() {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut members = Vec::new();
        let mut cut = false;
        while let Some(k) = stack.pop() {
            members.push(k);
            let (i, j) = (k % side, k / side);
            let neighbours = [
                (i > 0).then(|| k - 1),
                (i + 1 < side).then(|| k + 1),
                (j > 0).then(|| k - side),
                (j + 1 < side).then(|| k + side),
            ];
            for next in neighbours {
                match next {
                    Some(m) if !inside(points[m]) => cut = true,
                    Some(m) if !seen[m] && heights[m].is_some() => {
                        seen[m] = true;
                        stack.push(m);
                    }
                    Some(_) => {}
                    None => cut = true,
                }
            }
        }
        #[allow(clippy::cast_precision_loss, reason = "cell counts")]
        let area = members.len() as f64 * cell_km2;
        if area < min_area_km2 {
            continue;
        }
        found.push(shape_of(&members, &points, &heights, cell_m, cut));
    }
    found.sort_by(|a, b| {
        b.cells
            .cmp(&a.cells)
            .then(a.centroid[0].total_cmp(&b.centroid[0]))
            .then(a.centroid[1].total_cmp(&b.centroid[1]))
    });
    found
}

/// A patch's area, centroid, elongation and crest from its cells.
fn shape_of(
    members: &[usize],
    points: &[[f64; 2]],
    heights: &[Option<f64>],
    cell_m: f64,
    cut: bool,
) -> Patch {
    #[allow(clippy::cast_precision_loss, reason = "cell counts")]
    let count = members.len() as f64;
    let (mut sx, mut sz) = (0.0, 0.0);
    let mut crest = [0.0, 0.0, f64::NEG_INFINITY];
    for &k in members {
        let [x, z] = points[k];
        sx += x;
        sz += z;
        let h = heights[k].unwrap_or(f64::NEG_INFINITY);
        if h > crest[2] {
            crest = [x, z, h];
        }
    }
    let (mx, mz) = (sx / count, sz / count);
    let (mut cxx, mut czz, mut cxz) = (0.0, 0.0, 0.0);
    for &k in members {
        let [x, z] = points[k];
        let (dx, dz) = (x - mx, z - mz);
        cxx += dx * dx;
        czz += dz * dz;
        cxz += dx * dz;
    }
    let own = cell_m * cell_m / 12.0;
    let (a, c, b) = (cxx / count + own, czz / count + own, cxz / count);
    let mid = f64::midpoint(a, c);
    let spread = ((a - c) * 0.5 * (a - c) * 0.5 + b * b).sqrt();
    let elongation = ((mid + spread) / (mid - spread)).sqrt();
    Patch {
        cells: members.len(),
        area_km2: count * cell_m * cell_m / 1e6,
        centroid: [mx, mz],
        elongation,
        crest,
        cut,
    }
}

/// The median elongation of the patches the disc does not cut, `NaN` of
/// none: the probe's median (the upper middle one of an even count).
///
/// A cut patch's elongation is that of the part inside the disc, which can
/// make a round massif read as long or a range as round, so it says nothing
/// about the shape of the ground (ticket 0004's review of step 2b).
#[must_use]
pub fn median_elongation(patches: &[Patch]) -> f64 {
    let mut values: Vec<f64> = patches
        .iter()
        .filter(|p| !p.cut)
        .map(|p| p.elongation)
        .collect();
    median(&mut values)
}

/// The least elongation of the patches over `area_km2` the disc does not
/// cut, infinite if there are none; cut patches left out as in
/// [`median_elongation`].
#[must_use]
pub fn least_elongation_over(patches: &[Patch], area_km2: f64) -> f64 {
    patches
        .iter()
        .filter(|p| !p.cut && p.area_km2 > area_km2)
        .map(|p| p.elongation)
        .fold(f64::INFINITY, f64::min)
}

/// How long a flank profile runs out from its crest, in metres.
pub const PROFILE_LENGTH_M: f64 = 8_000.0;

/// The spacing of a flank profile's heights, in metres.
pub const PROFILE_STEP_M: f64 = 250.0;

/// Directions a flank profile's line is chosen among, evenly round the
/// crest.
pub const PROFILE_AZIMUTHS: u32 = 32;

/// A shelf is at least this many profile steps long: 750 m.
pub const SHELF_MIN_STEPS: usize = 3;

/// A run is a shelf when it rises at most this share of the less steep of
/// the runs of [`SHELF_MIN_STEPS`] either side of it.
pub const SHELF_RATIO: f64 = 0.5;

/// ... and when both those runs are at least this steep, as a rise over
/// run (about five degrees): a flat between two gentle rises is rolling
/// ground, not a bench on a mountainside.
pub const SHELF_SIDES_MIN: f64 = 0.0875;

/// A rise of more than this, in metres, within one profile step where the
/// plain meets the range is a step at its foot: 150 m in 250 m, about 31
/// degrees.
pub const FOOT_STEP_M: f64 = 150.0;

/// The foot of a range, for [`FOOT_STEP_M`]: the profile from the plain up
/// to where it has made this share of its whole rise. Not where the
/// mountains' biome begins: their relief waits for them to stand alone
/// (`WorldParams::mountain_rise`), a kilometre or more inside it, and the
/// wall a massif stood on was there.
pub const FOOT_SHARE: f64 = 1.0 / 3.0;

/// The heights along one range flank, from the plain to the crest, and what
/// is wrong with them.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Where the profile starts, on the plain [`PROFILE_LENGTH_M`] out,
    /// `[x, z]`.
    pub plain: [f64; 2],
    /// The crest it climbs to, `[x, z]`.
    pub crest: [f64; 2],
    /// Heights every [`PROFILE_STEP_M`], the plain first and the crest last.
    pub heights: Vec<f64>,
    /// The first shelf, `(first step, steps)`: a run of at least
    /// [`SHELF_MIN_STEPS`] that rises at most [`SHELF_RATIO`] of the runs
    /// either side of it, both of which rise at least [`SHELF_SIDES_MIN`].
    pub shelf: Option<(usize, usize)>,
    /// The first step at the foot, by the index of the step's upper end: a
    /// rise over [`FOOT_STEP_M`] within one profile step before the profile
    /// has made [`FOOT_SHARE`] of its rise.
    pub foot_step: Option<usize>,
}

impl Profile {
    /// Whether the profile has a shelf or a step at its foot.
    #[must_use]
    pub fn flagged(&self) -> bool {
        self.shelf.is_some() || self.foot_step.is_some()
    }

    /// The largest rise within one profile step, in metres, below half the
    /// profile's rise: how much of a wall the lower flank is (ticket 0004,
    /// step 2b's "wall", gated since step 4). Zero for a profile that never
    /// rises.
    #[must_use]
    pub fn wall_m(&self) -> f64 {
        let h = &self.heights;
        let (Some(&plain), Some(&crest)) = (h.first(), h.last()) else {
            return 0.0;
        };
        let half = plain + (crest - plain) * 0.5;
        let mut wall = 0.0_f64;
        for k in 1..h.len() {
            if h[k - 1] >= half {
                break;
            }
            wall = wall.max(h[k] - h[k - 1]);
        }
        wall
    }
}

/// The median of the profiles' [`Profile::wall_m`], the probe's median (the
/// upper middle one of an even count); `NaN` of none.
#[must_use]
pub fn median_wall_m(profiles: &[Profile]) -> f64 {
    let mut walls: Vec<f64> = profiles.iter().map(Profile::wall_m).collect();
    median(&mut walls)
}

/// The profile of the flank below `crest`: of [`PROFILE_AZIMUTHS`] straight
/// lines [`PROFILE_LENGTH_M`] long that stay on land, the one that falls
/// furthest, which is the steepest line down on the scale of a flank; `None`
/// if every line reaches the sea.
///
/// A straight line and not a path down the gullies: a range's flank is
/// judged at the scale of kilometres, and a descent that follows the
/// erosion's creases would measure a gully's floor.
#[must_use]
pub fn flank_profile<W: WorldSource + ?Sized>(world: &W, crest: [f64; 2]) -> Option<Profile> {
    steepest_line(world, crest, |_, _| true)
}

/// The steepest of the [`PROFILE_AZIMUTHS`] land lines down from `crest`
/// whose direction `allowed` accepts.
fn steepest_line<W: WorldSource + ?Sized>(
    world: &W,
    crest: [f64; 2],
    allowed: impl Fn(f64, f64) -> bool,
) -> Option<Profile> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "thirty-two steps"
    )]
    let steps = (PROFILE_LENGTH_M / PROFILE_STEP_M).round() as usize;
    let mut best: Option<(f64, Vec<f64>, [f64; 2])> = None;
    for a in 0..PROFILE_AZIMUTHS {
        let angle = f64::from(a) * core::f64::consts::TAU / f64::from(PROFILE_AZIMUTHS);
        let (dx, dz) = (libm::cos(angle), libm::sin(angle));
        if !allowed(dx, dz) {
            continue;
        }
        // From the plain in, so the crest is last.
        let mut heights = Vec::with_capacity(steps + 1);
        let mut on_land = true;
        for k in (0..=steps).rev() {
            #[allow(clippy::cast_precision_loss, reason = "a few dozen steps")]
            let d = k as f64 * PROFILE_STEP_M;
            let h = f64::from(world.sample(crest[0] + dx * d, crest[1] + dz * d).height);
            if h <= LAND_MIN_M {
                on_land = false;
                break;
            }
            heights.push(h);
        }
        if !on_land {
            continue;
        }
        let low = heights[0];
        if best.as_ref().is_none_or(|(b, ..)| low < *b) {
            let plain = [
                crest[0] + dx * PROFILE_LENGTH_M,
                crest[1] + dz * PROFILE_LENGTH_M,
            ];
            best = Some((low, heights, plain));
        }
    }
    let (_, heights, plain) = best?;
    Some(Profile {
        plain,
        crest,
        shelf: shelf_in(&heights),
        foot_step: foot_step_in(&heights),
        heights,
    })
}

/// The first shelf in a profile's heights; see [`Profile::shelf`].
fn shelf_in(heights: &[f64]) -> Option<(usize, usize)> {
    let steps = heights.len().saturating_sub(1);
    #[allow(clippy::cast_precision_loss, reason = "a few dozen steps")]
    let rise = |a: usize, b: usize| (heights[b] - heights[a]) / ((b - a) as f64 * PROFILE_STEP_M);
    let side = SHELF_MIN_STEPS;
    for start in side..steps {
        // The longest shelf from here, so a long bench reads as one.
        let mut found = None;
        let mut len = SHELF_MIN_STEPS;
        while start + len + side <= steps {
            let below = rise(start - side, start);
            let above = rise(start + len, start + len + side);
            let sides = below.min(above);
            let run = rise(start, start + len);
            if sides >= SHELF_SIDES_MIN && run.abs() <= SHELF_RATIO * sides {
                found = Some((start, len));
            }
            len += 1;
        }
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The first step at the foot in a profile's heights; see
/// [`Profile::foot_step`].
fn foot_step_in(heights: &[f64]) -> Option<usize> {
    let (&plain, &crest) = (heights.first()?, heights.last()?);
    let foot_top = plain + (crest - plain) * FOOT_SHARE;
    for k in 1..heights.len() {
        if heights[k] - heights[k - 1] > FOOT_STEP_M {
            return Some(k);
        }
        if heights[k] >= foot_top {
            break;
        }
    }
    None
}

/// Up to `count` flank profiles of the largest of `patches`: the steepest
/// flank of each in turn, largest patch first, and when every patch has
/// given one, the steepest of the rest of each (a line at least a quarter
/// turn from those taken there), until `count` are found.
#[must_use]
pub fn flank_profiles<W: WorldSource + ?Sized>(
    world: &W,
    patches: &[Patch],
    count: usize,
) -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    for patch in patches {
        if out.len() >= count {
            break;
        }
        if let Some(profile) = flank_profile(world, [patch.crest[0], patch.crest[1]]) {
            out.push(profile);
        }
    }
    let firsts = out.clone();
    for first in &firsts {
        if out.len() >= count {
            break;
        }
        if let Some(profile) = flank_profile_away(world, first) {
            out.push(profile);
        }
    }
    out
}

/// For each of `profiles`, the steepest land line down from its crest
/// within 45 degrees of the reverse of its own line: the other flank of the
/// same range, which [`flank_profiles`] never chooses when the first flank
/// was the steeper (the review of ticket 0004's step 4 found five of ten such
/// flanks flagged where the chosen ten flagged three). `None` where every
/// such line reaches the sea.
#[must_use]
pub fn opposite_profiles<W: WorldSource + ?Sized>(
    world: &W,
    profiles: &[Profile],
) -> Vec<Option<Profile>> {
    profiles
        .iter()
        .map(|first| {
            let taken = (
                first.plain[0] - first.crest[0],
                first.plain[1] - first.crest[1],
            );
            let length = (taken.0 * taken.0 + taken.1 * taken.1).sqrt();
            let back = (-taken.0 / length, -taken.1 / length);
            // Within 45 degrees: the cosine at least a half root of two,
            // with a margin for the rounding of the azimuths' own.
            steepest_line(world, first.crest, |dx, dz| {
                dx * back.0 + dz * back.1 >= core::f64::consts::FRAC_1_SQRT_2 - 1e-6
            })
        })
        .collect()
}

/// For each of the first `count` of `patches`, the flank facing the sea: of
/// the [`PROFILE_AZIMUTHS`] straight lines down from the crest, the one that
/// reaches the sea soonest (and not within 2 km), its heights every
/// [`PROFILE_STEP_M`] from the last land point to the crest, `plain` that
/// point. [`flank_profile`] throws such lines away, so a range's sea-facing
/// flank was never measured; the review of ticket 0004's step 4 found eleven
/// of twelve flagged. `None` for a crest no line of [`PROFILE_LENGTH_M`]
/// takes to the sea.
#[must_use]
pub fn seaward_profiles<W: WorldSource + ?Sized>(
    world: &W,
    patches: &[Patch],
    count: usize,
) -> Vec<Option<Profile>> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "thirty-two steps"
    )]
    let steps = (PROFILE_LENGTH_M / PROFILE_STEP_M).round() as usize;
    let min_steps = 8;
    patches
        .iter()
        .take(count)
        .map(|patch| {
            let crest = [patch.crest[0], patch.crest[1]];
            let mut best: Option<(usize, Vec<f64>, [f64; 2])> = None;
            for a in 0..PROFILE_AZIMUTHS {
                let angle = f64::from(a) * core::f64::consts::TAU / f64::from(PROFILE_AZIMUTHS);
                let (dx, dz) = (libm::cos(angle), libm::sin(angle));
                let mut heights = Vec::with_capacity(steps + 1);
                let mut end = None;
                for k in 0..=steps {
                    #[allow(clippy::cast_precision_loss, reason = "a few dozen steps")]
                    let d = k as f64 * PROFILE_STEP_M;
                    let h = f64::from(world.sample(crest[0] + dx * d, crest[1] + dz * d).height);
                    if h <= LAND_MIN_M {
                        end = Some(k);
                        break;
                    }
                    heights.push(h);
                }
                let Some(end) = end.filter(|&k| k >= min_steps) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(b, ..)| end < *b) {
                    #[allow(clippy::cast_precision_loss, reason = "a few dozen steps")]
                    let d = (end - 1) as f64 * PROFILE_STEP_M;
                    heights.reverse();
                    best = Some((end, heights, [crest[0] + dx * d, crest[1] + dz * d]));
                }
            }
            let (_, heights, plain) = best?;
            Some(Profile {
                plain,
                crest,
                shelf: shelf_in(&heights),
                foot_step: foot_step_in(&heights),
                heights,
            })
        })
        .collect()
}

/// The steepest land line down from `first`'s crest at least a quarter turn
/// from `first`'s own.
fn flank_profile_away<W: WorldSource + ?Sized>(world: &W, first: &Profile) -> Option<Profile> {
    let taken = (
        first.plain[0] - first.crest[0],
        first.plain[1] - first.crest[1],
    );
    // A quarter turn or more: the dot product with the taken line is at
    // most zero (with a margin for the rounding of a right angle's cosine).
    steepest_line(world, first.crest, |dx, dz| {
        dx * taken.0 + dz * taken.1 <= 1e-6
    })
}

/// The range of heights, in metres, over a lattice of `2n + 1` points a
/// side covering the square `half` metres to each side of `(x, z)`. The sea
/// counts as its surface, so a cliff over the water is its height over the
/// water and not over the sea bed.
#[must_use]
pub fn relief_m<W: WorldSource + ?Sized>(world: &W, x: f64, z: f64, half: f64, n: u32) -> f64 {
    let sea = f64::from(world.sea_level());
    let n = i64::from(n.max(1));
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for j in -n..=n {
        for i in -n..=n {
            // Lattice indices are a few dozen; exact in f64.
            #[allow(clippy::cast_precision_loss, reason = "small lattice indices")]
            let (fi, fj, fnn) = (i as f64, j as f64, n as f64);
            let h = f64::from(
                world
                    .sample(x + half * fi / fnn, z + half * fj / fnn)
                    .height,
            )
            .max(sea);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    hi - lo
}

/// The angle of the ground at `(x, z)` from horizontal, in degrees, by
/// central differences [`SLOPE_STEP_M`] to each side.
///
/// Not `Landform::slope`: that leaves out the continent's base elevation
/// (the coast's ramp and the elevation's share of a range; it holds a
/// range's own metres since ticket 0004's step 4), and a character stands
/// on the whole of it.
#[must_use]
pub fn slope_deg<W: WorldSource + ?Sized>(world: &W, x: f64, z: f64) -> f64 {
    let h = |x: f64, z: f64| f64::from(world.sample(x, z).height);
    let e = SLOPE_STEP_M;
    let gx = (h(x + e, z) - h(x - e, z)) / (2.0 * e);
    let gz = (h(x, z + e) - h(x, z - e)) / (2.0 * e);
    libm::atan((gx * gx + gz * gz).sqrt()).to_degrees()
}

/// The height of the riser at `(x, z)`, metres: from the point, the ground
/// is walked a metre at a time up its steepest direction (by central
/// differences at [`SLOPE_STEP_M`]) while each metre rises more than
/// [`RISER_DEG`] allows, and down the same way, each at most
/// [`RISER_WALK_M`]; the riser is what lies between the two ends. Zero
/// where the point itself is not steeper than [`RISER_DEG`].
#[must_use]
pub fn riser_height_m<W: WorldSource + ?Sized>(world: &W, x: f64, z: f64) -> f64 {
    let height = |px: f64, pz: f64| f64::from(world.sample(px, pz).height);
    let step = SLOPE_STEP_M;
    let gx = (height(x + step, z) - height(x - step, z)) / (2.0 * step);
    let gz = (height(x, z + step) - height(x, z - step)) / (2.0 * step);
    let slope = (gx * gx + gz * gz).sqrt();
    let limit = libm::tan(RISER_DEG.to_radians());
    if slope <= limit {
        return 0.0;
    }
    let (ux, uz) = (gx / slope, gz / slope);
    let walk = |sign: f64| {
        let mut last = height(x, z);
        let mut run = 0.0;
        while run < RISER_WALK_M {
            let next = height(x + sign * ux * (run + 1.0), z + sign * uz * (run + 1.0));
            if (next - last) * sign <= limit {
                break;
            }
            last = next;
            run += 1.0;
        }
        last
    };
    walk(1.0) - walk(-1.0)
}

/// `count` of `total` as a share, zero of nothing.
fn share(total: usize, count: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    // Point counts; exact in f64 far beyond any set.
    #[allow(clippy::cast_precision_loss, reason = "point counts")]
    let s = count as f64 / total as f64;
    s
}

/// The probe's median: the element at `round((n - 1) / 2)` of the sorted
/// values, which for an even count is the upper middle one. `NaN` of
/// nothing.
fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// `f` over `items` on up to `threads` threads, the results in the items'
/// order.
///
/// Work is handed out one item at a time from a shared counter: a point in
/// the mountains costs more than one on the plain (the erosion filter), so
/// fixed chunks would leave most threads idle at the end.
///
/// At most [`MAX_THREADS`] threads, and one call at a time per process (the
/// harness runs tests at once, and two pools together are the load the
/// development machine's CPU cannot take). `f` must not call this again.
fn parallel_map<T: Send>(
    threads: usize,
    items: &[[f64; 2]],
    f: impl Fn([f64; 2]) -> T + Sync,
) -> Vec<T> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    strata_core::platform::par::map(threads, items, |&point| f(point))
}

/// The `SplitMix64` finaliser as a float in `0..1`: the probe's scatter.
fn unit_hash(i: u64) -> f64 {
    let mut h = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    #[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
    let u = (h >> 11) as f64 / (1_u64 << 53) as f64;
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Landform, Sample};

    /// A plane rising `rise` metres per metre along `x`, `base` at the
    /// origin, over a sea at zero.
    struct Ramp {
        base: f64,
        rise: f64,
    }

    impl WorldSource for Ramp {
        fn sample(&self, x: f64, _z: f64) -> Sample {
            #[allow(clippy::cast_possible_truncation, reason = "test heights")]
            let height = (self.base + self.rise * x) as f32;
            Sample {
                height,
                biomes: [(BiomeId(u32::from(x > 0.0)), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.0,
                hydration: 0.0,
                cover: 0.0,
                landform: Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    #[test]
    fn a_slope_of_one_is_forty_five_degrees() {
        let ramp = Ramp {
            base: 100.0,
            rise: 1.0,
        };
        assert!((slope_deg(&ramp, 0.0, 0.0) - 45.0).abs() < 1e-6);
        let flat = Ramp {
            base: 100.0,
            rise: 0.0,
        };
        assert!(slope_deg(&flat, 0.0, 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_view_up_a_ramp_sees_its_angle_and_a_flat_sees_under_level() {
        let ramp = Ramp {
            base: 1_000.0,
            rise: 0.1,
        };
        // Up the ramp the far ground approaches the ramp's own angle from
        // under it, the eye being 1.7 m over the ground.
        let up = view_deg(&ramp, 0.0, 0.0);
        let angle = libm::atan(0.1).to_degrees();
        assert!(up < angle && up > angle - 0.2, "{up} against {angle}");
        let flat = Ramp {
            base: 100.0,
            rise: 0.0,
        };
        assert!(view_deg(&flat, 0.0, 0.0) < 0.0);
    }

    #[test]
    fn relief_counts_the_sea_as_its_surface() {
        // From -50 m to +50 m across the square: the sea half is level.
        let ramp = Ramp {
            base: 0.0,
            rise: 0.05,
        };
        assert!((relief_m(&ramp, 0.0, 0.0, 1_000.0, 8) - 50.0).abs() < 1e-3);
    }

    #[test]
    fn measures_do_not_depend_on_the_cores_and_count_every_class() {
        let ramp = Ramp {
            base: 50.0,
            rise: 0.001,
        };
        let points = PointSet::scatter(&ramp, 20_000.0, 16, 64);
        assert_eq!(points.view.len(), 16);
        assert_eq!(points.ground.len(), 64);
        assert!(
            points
                .view
                .iter()
                .all(|&[x, _]| f64::from(ramp.sample(x, 0.0).height) > LAND_MIN_M)
        );
        let one = measure(&ramp, &points);
        assert_eq!(one, measure_on(&ramp, &points, 1));
        assert_eq!(one, measure_on(&ramp, &points, 3));
        let total: f64 = one.slope_shares.iter().sum();
        assert!((total - 1.0).abs() < 1e-12);
        assert!((one.walkable_share - 1.0).abs() < 1e-12);
        let biomes: f64 = one.biome_shares.iter().map(|(_, s)| s).sum();
        assert!((biomes - 1.0).abs() < 1e-12);
        // Every biome stands on the same ramp, whose relief is none: the
        // ground's slope is the ramp's, the relief's is zero.
        let ramp_deg = libm::atan(0.001).to_degrees();
        for &(id, _) in &one.biome_shares {
            assert!((one.biome_slope(id) - ramp_deg).abs() < 1e-3);
            assert!(one.biome_relief_slope(id).abs() < 1e-12);
        }
        // A smaller set is the start of a larger one.
        let fewer = PointSet::scatter(&ramp, 20_000.0, 8, 32);
        assert_eq!(fewer.view[..], points.view[..8]);
        assert_eq!(fewer.ground[..], points.ground[..32]);
    }

    /// Ground a closure keeps as mountains (`BiomeId(1)`), 100 m up, over a
    /// plain at 10 m.
    struct Shape<F: Fn(f64, f64) -> bool + Send + Sync>(F);

    impl<F: Fn(f64, f64) -> bool + Send + Sync> WorldSource for Shape<F> {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let inside = (self.0)(x, z);
            Sample {
                height: if inside { 100.0 } else { 10.0 },
                biomes: [(BiomeId(u32::from(inside)), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.0,
                hydration: 0.0,
                cover: 0.0,
                landform: Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    fn mountain_patches<F: Fn(f64, f64) -> bool + Send + Sync>(shape: &Shape<F>) -> Vec<Patch> {
        patches(
            shape,
            |s| s.dominant_biome() == BiomeId(1),
            20_000.0,
            250.0,
            1.0,
            2,
        )
    }

    #[test]
    fn a_line_is_long_a_disc_is_round_and_a_cut_patch_is_not_gated() {
        // A strip 2 km wide and 16 km long, a disc 3 km across, and a strip
        // running out through the 20 km disc's edge.
        let shape = Shape(|x: f64, z: f64| {
            let strip = x.abs() < 8_000.0 && (z - 5_000.0).abs() < 1_000.0;
            let disc = x * x + (z + 6_000.0) * (z + 6_000.0) < 1_500.0 * 1_500.0;
            let out = x > 12_000.0 && (z + 12_000.0).abs() < 1_000.0;
            strip || disc || out
        });
        let found = mountain_patches(&shape);
        assert_eq!(found.len(), 3, "{found:?}");
        let near = |cx: f64, cz: f64| {
            found
                .iter()
                .find(|p| (p.centroid[0] - cx).abs() < 500.0 && (p.centroid[1] - cz).abs() < 500.0)
                .unwrap_or_else(|| panic!("a patch near ({cx}, {cz}) in {found:?}"))
        };
        let (strip, disc) = (near(0.0, 5_000.0), near(0.0, -6_000.0));
        assert!(!strip.cut && !disc.cut);
        // 15.75 by 1.75 km on the lattice: a ratio of standard deviations
        // of about nine.
        assert!(
            strip.elongation > 7.0 && strip.elongation < 10.0,
            "{strip:?}"
        );
        assert!(disc.elongation < 1.1, "{disc:?}");
        let cut = found
            .iter()
            .find(|p| p.cut)
            .expect("the strip through the edge");
        assert!(cut.elongation > 2.0, "{cut:?}");
        // The cut strip, long as it is, is left out: of the two uncut the
        // median is the upper, the strip; of the uncut over 5 km2, the
        // least is the disc's and the cut one does not count.
        assert!((median_elongation(&found) - strip.elongation).abs() < 1e-12);
        assert!((least_elongation_over(&found, 5.0) - disc.elongation).abs() < 1e-12);
        let only_cut = [cut.clone()];
        assert!(median_elongation(&only_cut).is_nan());
        assert!(least_elongation_over(&only_cut, 0.0).is_infinite());
    }

    #[test]
    fn a_staircase_is_flagged_and_a_smooth_flank_is_not() {
        // A flank of 33 heights every 250 m rising 1 500 m: smooth, a bell
        // steepening towards the crest, 40 to 1 540 m.
        let smooth: Vec<f64> = (0..=32)
            .map(|k| {
                let t = f64::from(k) / 32.0;
                40.0 + 1_500.0 * t * t
            })
            .collect();
        assert_eq!(shelf_in(&smooth), None);
        assert_eq!(foot_step_in(&smooth), None);
        // The same rise with a level kilometre half way up, between runs
        // that climb 60 m a step on either side.
        let mut shelf = Vec::new();
        let mut h = 40.0;
        for k in 0..=32 {
            shelf.push(h);
            h += if (14..18).contains(&k) { 0.0 } else { 60.0 };
        }
        assert!(shelf_in(&shelf).is_some(), "{shelf:?}");
        // A wall of 300 m in one step at the foot.
        let mut wall = smooth.clone();
        for h in &mut wall[3..] {
            *h += 300.0;
        }
        assert_eq!(foot_step_in(&wall), Some(3));
        // The wall's size, below half the rise: the smooth flank's largest
        // step there is its own, the walled one's the 300 m it was given.
        let profile = |heights: Vec<f64>| Profile {
            plain: [0.0; 2],
            crest: [0.0; 2],
            shelf: shelf_in(&heights),
            foot_step: foot_step_in(&heights),
            heights,
        };
        let (a, b) = (profile(smooth), profile(wall));
        assert!(a.wall_m() < 70.0, "{}", a.wall_m());
        assert!(b.wall_m() > 300.0, "{}", b.wall_m());
        assert!((median_wall_m(&[a.clone(), b.clone()]) - b.wall_m()).abs() < 1e-12);
    }

    #[test]
    fn a_pit_is_a_hollow_and_a_slope_is_not() {
        // A 21 by 21 lattice tilted along x, with two pits 8 m and 3 m deep
        // and one that drains out through the edge.
        let side = 21;
        let mut h: Vec<f64> = (0..side * side)
            .map(|k| f64::from(u32::try_from(k % side).expect("small")) * 0.5)
            .collect();
        h[5 * side + 5] -= 8.0;
        h[5 * side + 6] -= 6.0;
        h[15 * side + 15] -= 3.0;
        h[10 * side] -= 20.0;
        assert_eq!(closed_hollows(&h, side), 1);
        let flat = vec![10.0; side * side];
        assert_eq!(closed_hollows(&flat, side), 0);
    }

    #[test]
    fn a_tensor_of_one_grain_is_long_and_of_every_grain_round() {
        assert!((tensor_elongation([1.0, 0.0, 1.0]) - 1.0).abs() < 1e-12);
        assert!((tensor_elongation([9.0, 0.0, 1.0]) - 3.0).abs() < 1e-12);
        assert!((tensor_elongation([5.0, 4.0, 5.0]) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_view_where_nothing_is_kept_sees_nothing() {
        let ramp = Ramp {
            base: 1_000.0,
            rise: 0.1,
        };
        assert!(view_deg_where(&ramp, 0.0, 0.0, |_, _| false).is_infinite());
        // Keeping only the ground behind (x < 0) sees the ramp falling away.
        let behind = view_deg_where(&ramp, 0.0, 0.0, |x, _| x < 0.0);
        assert!(behind < view_deg(&ramp, 0.0, 0.0));
    }
}
