//! The fog at a point: baked basins plus a weather.
//!
//! [`FogField`] holds the tiles a caller has baked and the seed's slow
//! noises, and answers [`FogField::sample`] for any point over a tile. The
//! answer is cheap (a bilinear lookup and seven simplex octaves, 80 ns), so a
//! renderer can fill a texture from it every time the weather moves.
//!
//! Two kinds of fog are in the answer, and they add:
//!
//! - **Basin fog**, from the tile: cold air pooled to a level. Its density
//!   grows with how far the ground is under the top and with the ground's
//!   hydration, so a wet hollow fogs and a dry one hazes. Weaker weather
//!   lowers the top towards the floor and keeps only the strongest
//!   hollows.
//! - **Sea fog**, from [`Weather::sea_fog`]: banks over the sea from a slow
//!   noise that drifts with [`Weather::drift`], tens of metres deep, whose
//!   top sinks to the water at a bank's edge. It comes ashore up inlets and
//!   over low coast where the ground is under its top, and thins with the
//!   distance from the sea along low ground. The sea is never a basin: over
//!   open water the closing is the sea itself and only this weather fogs.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use strata_core::Seed;
use strata_worldgen::noise::{Fractal, Shape, smoothstep};

use crate::tile::{BasinTile, MalformedTile, Nodes, SEA_REACH_M, TileCoord};

/// How much fog the weather makes, now. A pure value: step 3's day curve
/// and slow weather are a function from `(seed, sim time)` to this, and
/// nothing else in the field changes when that arrives.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    /// Fog in basins, `0` none to `1` every hollow full to its top.
    pub fog: f32,
    /// Fog off the sea, `0` a clear sea to `1` banks over most of it.
    pub sea_fog: f32,
    /// How far the air has carried the fog, metres in world `(x, z)`:
    /// the banks' pattern is read this far upwind, so it moves with the
    /// wind while the basins stay where the ground put them.
    pub drift: [f64; 2],
}

impl Weather {
    /// No fog anywhere.
    pub const CLEAR: Self = Self {
        fog: 0.0,
        sea_fog: 0.0,
        drift: [0.0, 0.0],
    };
    /// Every basin full and banks over most of the sea: the worldmap's
    /// weather, the most the field can show.
    pub const FULL: Self = Self {
        fog: 1.0,
        sea_fog: 1.0,
        drift: [0.0, 0.0],
    };
}

/// The fog over one point of the ground: a layer from `base` to `top` of
/// `density`.
///
/// `density` is a share of the thickest fog a renderer draws (which it maps
/// to an extinction, forty metres' sight say, at `1`), constant through the
/// layer; the renderer softens the top itself. Where there is no fog
/// `density` is zero and `top` is at or under the ground. For ground fog
/// `base` is under the ground and a renderer may ignore it; low cloud
/// (ticket 0003, step 4) is a layer whose base is above it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FogSample {
    /// The layer's bottom, metres above sea level.
    pub base: f32,
    /// The layer's top, metres above sea level.
    pub top: f32,
    /// How thick, `0..=1`.
    pub density: f32,
}

impl FogSample {
    /// Nothing: what a point with no baked tile under it reads.
    pub const NONE: Self = Self {
        base: 0.0,
        top: 0.0,
        density: 0.0,
    };

    /// The density at height `y`: [`density`](Self::density) inside the
    /// layer, zero above its top and under its base.
    #[must_use]
    pub fn density_at(&self, y: f32) -> f32 {
        if y >= self.base && y <= self.top {
            self.density
        } else {
            0.0
        }
    }
}

/// Fog less than this deep under its top is not drawn at all: ground a
/// metre or two under the level is the noise of 128 m nodes over rough
/// ground, not a pool.
const MIN_DEPTH_M: f64 = 2.0;
/// The depth past [`MIN_DEPTH_M`] at which fog reaches half its full
/// density.
const HALF_DEPTH_M: f64 = 10.0;
/// A dry basin's share of a wet one's density: a haze, not a clear sky.
const DRY_SHARE: f64 = 0.25;
/// The hydration range over which a basin goes from dry to wet.
const DRY_HYDRATION: f64 = 0.15;
const WET_HYDRATION: f64 = 0.75;
/// Under the weakest fog the top sits this share of the way from the floor
/// to its full height.
const WEAK_TOP_SHARE: f64 = 0.5;
/// The wavelength of the drifting patches that thin basin fog in places,
/// and how much they thin it.
const PATCH_WAVELENGTH_M: f64 = 1600.0;
const PATCH_DEPTH: f64 = 0.3;
/// How far out over the water basin fog fades to nothing.
const SHORE_FADE_M: f64 = 400.0;
/// Sea fog banks: the wavelength of the pattern and of the top's swell,
/// and the range of the top at the heart of a bank.
const BANK_WAVELENGTH_M: f64 = 14_000.0;
const SWELL_WAVELENGTH_M: f64 = 5000.0;
const SEA_TOP_MIN_M: f64 = 25.0;
const SEA_TOP_MAX_M: f64 = 85.0;
/// How far a bank's edge runs in the pattern's units, from the water's
/// surface to the bank's full height.
const BANK_EDGE: f64 = 0.2;

/// Density of fog `depth` metres deep under its top: zero down to
/// [`MIN_DEPTH_M`], a half at [`HALF_DEPTH_M`] more, towards one. The onset
/// is quadratic, so where a bank of fog meets rising ground its density
/// fades in over tens of metres instead of starting at a crease.
fn depth_density(depth: f64) -> f64 {
    let d = (depth - MIN_DEPTH_M).max(0.0);
    let d2 = d * d;
    d2 / (d2 + HALF_DEPTH_M * HALF_DEPTH_M)
}

/// The fog field: the seed's noises and the tiles baked so far.
///
/// Tiles are shared (`Arc`) so a renderer can bake on a task pool and hand
/// the result over without a copy. Iteration is over a `BTreeMap`, so
/// nothing in an answer depends on hash order.
#[derive(Debug, Clone)]
pub struct FogField {
    banks: Fractal,
    swell: Fractal,
    patches: Fractal,
    tiles: BTreeMap<TileCoord, Arc<BasinTile>>,
}

impl FogField {
    /// An empty field for a seed: no tiles yet, so every point reads
    /// [`FogSample::NONE`] until its tile is inserted.
    #[must_use]
    pub fn new(seed: Seed) -> Self {
        Self {
            banks: Fractal::new(
                seed.derive("weather/sea-fog/banks"),
                Shape::Fbm,
                BANK_WAVELENGTH_M,
                3,
                0.5,
                2.0,
            ),
            swell: Fractal::new(
                seed.derive("weather/sea-fog/swell"),
                Shape::Fbm,
                SWELL_WAVELENGTH_M,
                2,
                0.5,
                2.0,
            ),
            patches: Fractal::new(
                seed.derive("weather/fog/patches"),
                Shape::Fbm,
                PATCH_WAVELENGTH_M,
                2,
                0.5,
                2.0,
            ),
            tiles: BTreeMap::new(),
        }
    }

    /// Adds (or replaces) a baked tile.
    ///
    /// # Errors
    ///
    /// A tile whose channels have the wrong length ([`BasinTile::check`])
    /// is refused and the field is left as it was: sampling indexes the
    /// channels unchecked, and a server must not panic on a bad tile.
    pub fn insert(&mut self, tile: impl Into<Arc<BasinTile>>) -> Result<(), MalformedTile> {
        let tile = tile.into();
        tile.check()?;
        self.tiles.insert(tile.coord, tile);
        Ok(())
    }

    /// Whether a tile is in the field.
    #[must_use]
    pub fn contains(&self, coord: TileCoord) -> bool {
        self.tiles.contains_key(&coord)
    }

    /// The tile at a coordinate, if baked.
    #[must_use]
    pub fn tile(&self, coord: TileCoord) -> Option<&Arc<BasinTile>> {
        self.tiles.get(&coord)
    }

    /// Drops the tiles `keep` says no to: how a renderer forgets tiles the
    /// observer has left.
    pub fn retain(&mut self, mut keep: impl FnMut(TileCoord) -> bool) {
        self.tiles.retain(|&c, _| keep(c));
    }

    /// The tiles `covering` a rectangle that are not baked yet, `x` fastest:
    /// what a renderer bakes before filling a texture over it.
    pub fn missing(&self, min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> Vec<TileCoord> {
        TileCoord::covering(min_x, min_z, max_x, max_z)
            .filter(|c| !self.contains(*c))
            .collect()
    }

    /// The fog at a world point under a weather, or `None` where the tile
    /// under it has not been baked.
    #[must_use]
    pub fn sample(&self, x: f64, z: f64, weather: &Weather) -> Option<FogSample> {
        let tile = self.tiles.get(&TileCoord::containing(x, z))?;
        Some(self.sample_tile(tile, x, z, weather))
    }

    /// Samples an `n` by `n` grid from `(origin_x, origin_z)` with `step`
    /// metres between points into `out` (cleared first), row-major with `x`
    /// fastest like [`strata_worldgen::WorldSource::sample_grid`]. Points
    /// with no baked tile read [`FogSample::NONE`]; returns how many did.
    pub fn sample_grid(
        &self,
        origin_x: f64,
        origin_z: f64,
        step: f64,
        n: usize,
        weather: &Weather,
        out: &mut Vec<FogSample>,
    ) -> usize {
        out.clear();
        out.reserve(n * n);
        let mut missing = 0;
        let mut cached: Option<&Arc<BasinTile>> = None;
        for j in 0..n {
            for i in 0..n {
                // Grid indices are small; the cast is exact.
                #[allow(clippy::cast_precision_loss)]
                let (x, z) = (origin_x + i as f64 * step, origin_z + j as f64 * step);
                let coord = TileCoord::containing(x, z);
                let tile = match cached {
                    Some(t) if t.coord == coord => Some(t),
                    _ => {
                        cached = self.tiles.get(&coord);
                        cached
                    }
                };
                if let Some(tile) = tile {
                    out.push(self.sample_tile(tile, x, z, weather));
                } else {
                    missing += 1;
                    out.push(FogSample::NONE);
                }
            }
        }
        missing
    }

    /// The fog at a point of a given tile, for a caller that keeps its own
    /// tiles. The point must lie in the tile.
    #[must_use]
    pub fn sample_tile(&self, tile: &BasinTile, x: f64, z: f64, weather: &Weather) -> FogSample {
        let nodes = tile.lerp(x, z);
        let (basin_top, basin_density) = self.basin(&nodes, x, z, weather);
        let (sea_top, sea_density) = self.sea(&nodes, x, z, weather);
        // Two layers into one slab. Its top is the higher layer's once that
        // layer is more than a trace (a union of the two), and slides to the
        // lower one's as the higher fades, weighted by the densities so a
        // layer with none never moves it. The densities add as media do: one
        // minus the product of what each lets through.
        let ((hi_top, hi_d), (lo_top, lo_d)) = if basin_top >= sea_top {
            ((basin_top, basin_density), (sea_top, sea_density))
        } else {
            ((sea_top, sea_density), (basin_top, basin_density))
        };
        let total = hi_d + lo_d;
        let top = if total > 0.0 {
            let lead = smoothstep(0.0, 0.1, hi_d);
            hi_top - (hi_top - lo_top) * lo_d * (1.0 - lead) / total
        } else {
            hi_top
        };
        let density = 1.0 - (1.0 - basin_density) * (1.0 - sea_density);
        // The layer starts under everything near it: the basin's floor or
        // the sea, whichever is lower, less a margin for the ground between
        // nodes.
        let base = nodes.floor.min(nodes.ground) - 10.0;
        // Metres and a unit share, stored as f32 like the tile.
        #[allow(clippy::cast_possible_truncation)]
        FogSample {
            base: base as f32,
            top: top as f32,
            density: density as f32,
        }
    }

    /// Basin fog: `(top, density)`.
    fn basin(&self, nodes: &Nodes, x: f64, z: f64, weather: &Weather) -> (f64, f64) {
        let fog = f64::from(weather.fog).clamp(0.0, 1.0);
        let top_share = WEAK_TOP_SHARE + (1.0 - WEAK_TOP_SHARE) * fog;
        let top = nodes.floor + (nodes.top - nodes.floor) * top_share;
        let wet = DRY_SHARE
            + (1.0 - DRY_SHARE) * smoothstep(DRY_HYDRATION, WET_HYDRATION, nodes.hydration);
        let potential = depth_density(top - nodes.ground) * wet;
        if potential <= 0.0 || fog <= 0.0 {
            return (top, 0.0);
        }
        // Weak weather keeps only the strongest pools: at `fog` 1 every
        // potential counts, at a half only the upper part of the range.
        let edge = 0.8 * (1.0 - fog);
        let presence = fog * smoothstep(edge - 0.2, edge, potential);
        let patch = 1.0
            - PATCH_DEPTH
                * (0.5 + 0.5 * self.patches.get(x - weather.drift[0], z - weather.drift[1]));
        // The sea is not a basin: a cove's fog fades out over its first
        // few hundred metres of water, so a strait between two headlands
        // stays clear and the fog on a bay's shore does not end in a wall.
        let off_shore = 1.0 - smoothstep(0.0, SHORE_FADE_M, nodes.offshore);
        (top, potential * presence * patch * off_shore)
    }

    /// Sea fog: `(top, density)`.
    fn sea(&self, nodes: &Nodes, x: f64, z: f64, weather: &Weather) -> (f64, f64) {
        let amount = f64::from(weather.sea_fog).clamp(0.0, 1.0);
        let (dx, dz) = (x - weather.drift[0], z - weather.drift[1]);
        if amount <= 0.0 || nodes.sea_distance >= f64::from(SEA_REACH_M) {
            return (0.0, 0.0);
        }
        // The pattern's threshold falls as the weather rises, so more of
        // the sea is under a bank; at a bank's edge the top sinks to the
        // water, so a bank ends in a slope and not a wall.
        let threshold = 0.35 - 0.55 * amount;
        let bank = smoothstep(threshold, threshold + BANK_EDGE, self.banks.get(dx, dz));
        let swell = 0.5 + 0.5 * self.swell.get(dx, dz);
        let top = (SEA_TOP_MIN_M + (SEA_TOP_MAX_M - SEA_TOP_MIN_M) * swell) * bank;
        // Inland it thins with the way it came, over low ground only.
        let reach = f64::from(SEA_REACH_M);
        let ashore = 1.0 - smoothstep(0.2 * reach, reach, nodes.sea_distance);
        let density = depth_density(top - nodes.ground) * ashore * (0.5 + 0.5 * amount);
        (top, density)
    }
}
