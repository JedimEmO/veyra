//! The fog field on the GPU: two textures that follow the observer.
//!
//! `strata-weather` says where fog lies as a function of the world, and the
//! fog pass has to read it per raymarch step. It reads it from a clipmap of
//! two levels ([`LevelSpec::FINE`], 8 km at 32 m a texel, and
//! [`LevelSpec::COARSE`], 64 km at 250 m), each texel the field's
//! [`FogSample`] under the pinned weather, stored as four half floats: the
//! top, the base, the density and a one where the texel's tile was baked (a
//! zero, with no fog, where it was not; the shader needs only the density,
//! which is already zero there, and the flag is for reading a level back).
//!
//! Everything slow happens on the async compute pool, as the terrain meshes
//! its regions: a tile of the field is baked (34 ms of one core) in a task
//! of its own, and a level is sampled from the baked tiles (65 thousand
//! samples, about 5 ms) in another. The frame only polls, moves finished
//! tiles into the field and hands a finished level to the render world,
//! which copies it into its texture (512 KB) on the frame it arrives.
//!
//! That pool is Bevy's async compute pool, a quarter of the cores and at
//! most four threads, and the terrain's meshes and materials, the foliage
//! and the colliders are queued on it too. A start with nothing baked wants
//! 81 tiles (almost three seconds of one core), and queued all at once they
//! held the terrain and the player's ground back by most of a second on
//! this machine and would hold them for seconds on a machine with one pool
//! thread. So at most [`bake_budget`] bakes are in flight, the fine level's
//! tiles are asked for before the coarse level's, and a level's tiles
//! nearest the observer first.
//!
//! A level is centred on the observer snapped to its texel grid, so a texel
//! always samples the same world point and the fog does not swim as the
//! observer walks; it is rebuilt around a new centre once the observer is
//! [`RECENTRE_FRACTION`] of the way from its centre to its edge, and the old
//! texture is drawn until the new one is ready. Which texture the render
//! world holds and where the uniform says it lies are extracted in the same
//! extract from the same [`LevelTexels`], so they never disagree.

use std::collections::BTreeMap;
use std::sync::Arc;
use strata_core::platform::time::Instant;

use bevy::ecs::prelude::*;
use bevy::render::Extract;
use bevy::render::render_resource::{
    Extent3d, TexelCopyBufferLayout, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use strata_app::{Observers, WorldSeed, WorldSourceRes};
use strata_weather::{BasinTile, FogField, FogSample, TILE_M, TileCoord, Weather};

/// How far the observer may move from a level's centre, as a share of the
/// level's half extent, before the level is rebuilt around it. A quarter
/// keeps three quarters of the half extent ahead of the observer while the
/// new level builds, and rebuilds the fine level every kilometre.
pub const RECENTRE_FRACTION: f64 = 0.25;

/// A density under this is no fog: a texel that holds less does not raise
/// the level's [`LevelTexels::max_top`].
const FOG_TRACE: f32 = 1e-3;

/// The texture format of a level: four half floats, which filter linearly on
/// every GPU and hold a height of hundreds of metres to a fraction of one.
pub const TEXEL_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Bytes per texel in [`TEXEL_FORMAT`].
const TEXEL_BYTES: usize = 8;

/// One level of the clipmap: how many texels a side and how far apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelSpec {
    /// Texels along a side; even, so the centre falls on a texel corner.
    pub texels: u32,
    /// Metres between texel centres.
    pub texel_m: f64,
}

impl LevelSpec {
    /// Near the observer: 8 km across at 32 m, four texels to a node of the
    /// field, so the bilinear lookup in the shader follows the field's own.
    pub const FINE: Self = Self {
        texels: 256,
        texel_m: 32.0,
    };
    /// To the horizon: 64 km across at 250 m, where a 25 m hollow a
    /// kilometre wide is still four texels.
    pub const COARSE: Self = Self {
        texels: 256,
        texel_m: 250.0,
    };

    /// Side of the level, metres.
    #[must_use]
    pub fn extent_m(self) -> f64 {
        f64::from(self.texels) * self.texel_m
    }

    /// The centre of a level around an observer at `(x, z)`: the nearest
    /// texel corner, so texel centres stay on one world lattice whatever the
    /// observer does.
    #[must_use]
    pub fn snapped_centre(self, x: f64, z: f64) -> [f64; 2] {
        [
            (x / self.texel_m).round() * self.texel_m,
            (z / self.texel_m).round() * self.texel_m,
        ]
    }

    /// The world position of the `-x -z` corner of texel `(0, 0)`.
    #[must_use]
    pub fn origin(self, centre: [f64; 2]) -> [f64; 2] {
        let half = 0.5 * self.extent_m();
        [centre[0] - half, centre[1] - half]
    }

    /// Whether a level centred on `centre` (or none yet) has to be rebuilt
    /// for an observer at `(x, z)`: it has none, or the observer is more
    /// than [`RECENTRE_FRACTION`] of the half extent from its centre along
    /// either axis.
    #[must_use]
    pub fn needs_recentre(self, centre: Option<[f64; 2]>, x: f64, z: f64) -> bool {
        let Some(c) = centre else {
            return true;
        };
        let limit = RECENTRE_FRACTION * 0.5 * self.extent_m();
        (x - c[0]).abs() > limit || (z - c[1]).abs() > limit
    }

    /// The rectangle `(min_x, min_z, max_x, max_z)` whose texel centres a
    /// level samples: the tiles under it are what the level needs baked.
    #[must_use]
    pub fn sampled_rect(self, centre: [f64; 2]) -> [f64; 4] {
        let o = self.origin(centre);
        let first = 0.5 * self.texel_m;
        let last = self.extent_m() - 0.5 * self.texel_m;
        [o[0] + first, o[1] + first, o[0] + last, o[1] + last]
    }
}

/// An `f32` as IEEE half-float bits, rounded to nearest. Out of range
/// values clamp to the largest finite half, NaN reads as zero (a texel is a
/// height or a share and neither is ever NaN), and magnitudes under the
/// smallest normal half flush to zero.
#[must_use]
pub fn f16_bits(value: f32) -> u16 {
    if value.is_nan() {
        return 0;
    }
    let bits = value.to_bits();
    let sign = u16::try_from((bits >> 16) & 0x8000).unwrap_or(0);
    let magnitude = value.abs();
    if magnitude < 6.103_515_6e-5 {
        return sign;
    }
    if magnitude >= 65_504.0 {
        return sign | 0x7bff;
    }
    // Rebias the exponent from 127 to 15 and round the mantissa from 23 bits
    // to 10: adding half of the dropped part carries into the exponent when
    // the mantissa overflows, which is the correct rounding.
    let exponent = ((bits >> 23) & 0xff) + 15 - 127;
    let mantissa = bits & 0x007f_ffff;
    let rounded = ((exponent << 10) | (mantissa >> 13)) + ((mantissa >> 12) & 1);
    sign | u16::try_from(rounded.min(0x7bff)).unwrap_or(0x7bff)
}

/// Half-float bits as an `f32`: the inverse of [`f16_bits`] for every value
/// it produces.
#[must_use]
pub fn f16_value(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x03ff);
    let magnitude = if exponent == 0 {
        mantissa * (2f32).powi(-24)
    } else {
        (1.0 + mantissa / 1024.0) * (2f32).powi(exponent - 15)
    };
    sign * magnitude
}

/// A field sample as the four half floats of a texel: top, base, density,
/// and one where its tile was baked, zero where it was not.
#[must_use]
pub fn encode_texel(sample: &FogSample, baked: bool) -> [u16; 4] {
    [
        f16_bits(sample.top),
        f16_bits(sample.base),
        f16_bits(sample.density.clamp(0.0, 1.0)),
        f16_bits(if baked { 1.0 } else { 0.0 }),
    ]
}

/// A texel back to the sample it holds, to half-float precision.
#[must_use]
pub fn decode_texel(texel: [u16; 4]) -> FogSample {
    FogSample {
        top: f16_value(texel[0]),
        base: f16_value(texel[1]),
        density: f16_value(texel[2]),
    }
}

/// One level, sampled: the bytes of its texture and what the pass needs to
/// know about them.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelTexels {
    /// Which level.
    pub spec: LevelSpec,
    /// Its centre, world metres.
    pub centre: [f64; 2],
    /// The weather it was sampled under.
    pub weather: Weather,
    /// Row-major, `x` fastest, [`TEXEL_FORMAT`], little-endian.
    pub bytes: Vec<u8>,
    /// The highest top over any texel with fog or next to one (the shader
    /// filters between neighbours), metres; `f32::MIN` with no fog at all.
    pub max_top: f32,
    /// The lowest base over the same texels, metres; `f32::MAX` with none.
    pub min_base: f32,
    /// Texels whose tile was not baked (read as no fog).
    pub unbaked: usize,
    /// Tells a new level from the one before it, for the render world.
    pub serial: u64,
}

impl LevelTexels {
    /// Whether any texel has fog.
    #[must_use]
    pub fn has_fog(&self) -> bool {
        self.max_top >= self.min_base
    }
}

/// Samples a level from the field. Pure and slow (a sample is about 80 ns):
/// the clipmap runs it on the task pool.
#[must_use]
pub fn sample_level(
    field: &FogField,
    spec: LevelSpec,
    centre: [f64; 2],
    weather: Weather,
    serial: u64,
) -> LevelTexels {
    let n = spec.texels as usize;
    let rect = spec.sampled_rect(centre);
    let mut samples = Vec::with_capacity(n * n);
    let unbaked = field.sample_grid(rect[0], rect[1], spec.texel_m, n, &weather, &mut samples);
    let mut bytes = Vec::with_capacity(n * n * TEXEL_BYTES);
    for (k, sample) in samples.iter().enumerate() {
        // The texel's point as `sample_grid` computed it, so its tile is
        // the one that was (or was not) there.
        #[allow(clippy::cast_precision_loss, reason = "texel indices are small")]
        let (x, z) = (
            rect[0] + (k % n) as f64 * spec.texel_m,
            rect[1] + (k / n) as f64 * spec.texel_m,
        );
        let baked = unbaked == 0 || field.contains(TileCoord::containing(x, z));
        for half in encode_texel(sample, baked) {
            bytes.extend_from_slice(&half.to_le_bytes());
        }
    }
    let (mut max_top, mut min_base) = (f32::MIN, f32::MAX);
    for j in 0..n {
        for i in 0..n {
            if samples[j * n + i].density <= FOG_TRACE {
                continue;
            }
            for nj in j.saturating_sub(1)..=(j + 1).min(n - 1) {
                for ni in i.saturating_sub(1)..=(i + 1).min(n - 1) {
                    let s = &samples[nj * n + ni];
                    max_top = max_top.max(s.top);
                    min_base = min_base.min(s.base);
                }
            }
        }
    }
    LevelTexels {
        spec,
        centre,
        weather,
        bytes,
        max_top,
        min_base,
        unbaked,
        serial,
    }
}

/// The tiles to keep for levels centred on `centres`: every tile a level
/// samples, and one more ring, so a re-centre does not rebake what it just
/// dropped.
#[must_use]
pub fn keep_tile(coord: TileCoord, levels: &[(LevelSpec, [f64; 2])]) -> bool {
    levels.iter().any(|(spec, centre)| {
        let r = spec.sampled_rect(*centre);
        let lo = TileCoord::containing(r[0] - TILE_M, r[1] - TILE_M);
        let hi = TileCoord::containing(r[2] + TILE_M, r[3] + TILE_M);
        (lo.x..=hi.x).contains(&coord.x) && (lo.z..=hi.z).contains(&coord.z)
    })
}

/// How many tile bakes may be in flight at once on a pool: half its
/// threads, at least one, so the terrain, foliage and collider tasks that
/// share it always find a thread free or wait behind one bake, not eighty.
#[must_use]
pub fn bake_budget(pool: &TaskPool) -> usize {
    (pool.thread_num() / 2).max(1)
}

/// Squared distance from `(x, z)` to a tile's centre, for asking for the
/// nearest tiles first.
fn tile_distance2(coord: TileCoord, x: f64, z: f64) -> f64 {
    let (ox, oz) = coord.origin();
    let (dx, dz) = (ox + 0.5 * TILE_M - x, oz + 0.5 * TILE_M - z);
    dx * dx + dz * dz
}

/// A level being rebuilt.
enum Build {
    /// Waiting for the tiles under it.
    Baking {
        centre: [f64; 2],
        weather: Weather,
        started: Instant,
    },
    /// Sampling on the pool.
    Sampling {
        centre: [f64; 2],
        task: Task<LevelTexels>,
        started: Instant,
    },
}

impl Build {
    fn centre(&self) -> [f64; 2] {
        match self {
            Self::Baking { centre, .. } | Self::Sampling { centre, .. } => *centre,
        }
    }
}

/// One level in the main world: what is shown and what is being built.
struct Level {
    spec: LevelSpec,
    shown: Option<Arc<LevelTexels>>,
    build: Option<Build>,
}

impl Level {
    fn new(spec: LevelSpec) -> Self {
        Self {
            spec,
            shown: None,
            build: None,
        }
    }

    /// Shows a level whose sampling finished, at once.
    fn show_finished(&mut self, index: usize, bakes_in_flight: usize) {
        if let Some(Build::Sampling { task, started, .. }) = &mut self.build
            && task.is_finished()
        {
            if let Some(texels) = block_on(poll_once(task)) {
                tracing::info!(
                    level = index,
                    secs = started.elapsed().as_secs_f32(),
                    x = texels.centre[0],
                    z = texels.centre[1],
                    max_top = texels.max_top,
                    min_base = texels.min_base,
                    unbaked = texels.unbaked,
                    bakes_in_flight,
                    "fog clipmap level ready"
                );
                self.shown = Some(Arc::new(texels));
            }
            self.build = None;
        }
    }
}

/// The clipmap's main-world state: the field's baked tiles, the bakes in
/// flight and the two levels. Inserted once the world source exists.
#[derive(Resource)]
pub struct FogClipmap {
    field: FogField,
    bakes: BTreeMap<TileCoord, Task<BasinTile>>,
    levels: [Level; 2],
    serial: u64,
    /// Bakes in flight at most, when not [`bake_budget`] of the pool: a
    /// test's, whose pool is the machine's.
    budget: Option<usize>,
}

impl FogClipmap {
    /// An empty clipmap for a seed.
    #[must_use]
    pub fn new(field: FogField) -> Self {
        Self {
            field,
            bakes: BTreeMap::new(),
            levels: [Level::new(LevelSpec::FINE), Level::new(LevelSpec::COARSE)],
            serial: 0,
            budget: None,
        }
    }

    /// The level shown now, fine first, as the render world will draw it.
    #[must_use]
    pub fn shown(&self) -> [Option<Arc<LevelTexels>>; 2] {
        [self.levels[0].shown.clone(), self.levels[1].shown.clone()]
    }

    /// Tile bakes in flight.
    #[must_use]
    pub fn bakes_in_flight(&self) -> usize {
        self.bakes.len()
    }

    /// A clipmap whose levels are not the game's, so a test builds them
    /// from a few tiles, one bake at a time.
    #[cfg(test)]
    fn with_specs(field: FogField, specs: [LevelSpec; 2]) -> Self {
        Self {
            levels: specs.map(Level::new),
            budget: Some(1),
            ..Self::new(field)
        }
    }

    /// Where a level's build is centred, if one is under way.
    #[cfg(test)]
    fn build_centre(&self, level: usize) -> Option<[f64; 2]> {
        self.levels[level].build.as_ref().map(Build::centre)
    }

    /// Moves finished bakes into the field, in coordinate order.
    fn collect_bakes(&mut self) {
        let done: Vec<TileCoord> = self
            .bakes
            .iter()
            .filter(|(_, task)| task.is_finished())
            .map(|(c, _)| *c)
            .collect();
        for coord in done {
            if let Some(mut task) = self.bakes.remove(&coord)
                && let Some(tile) = block_on(poll_once(&mut task))
                && let Err(error) = self.field.insert(tile)
            {
                // A bake is always well formed; this is the field refusing
                // a tile it could not sample, which must not be a panic.
                tracing::error!(%error, "fog clipmap dropped a tile");
            }
        }
    }

    /// Starts bakes of the tiles under `rect` that are neither baked nor in
    /// flight, nearest `observer` first, while fewer than `budget` bakes are
    /// in flight. Whether any tile under it is still missing.
    fn bake_missing(
        &mut self,
        rect: [f64; 4],
        observer: (f64, f64),
        budget: usize,
        source: &WorldSourceRes,
        pool: &TaskPool,
    ) -> bool {
        let mut missing = self.field.missing(rect[0], rect[1], rect[2], rect[3]);
        if missing.is_empty() {
            return false;
        }
        let (x, z) = observer;
        missing.sort_by(|a, b| {
            tile_distance2(*a, x, z)
                .total_cmp(&tile_distance2(*b, x, z))
                .then(a.cmp(b))
        });
        for coord in missing {
            if self.bakes.contains_key(&coord) {
                continue;
            }
            if self.bakes.len() >= budget {
                break;
            }
            let world = source.0.clone();
            self.bakes.insert(
                coord,
                pool.spawn(async move { BasinTile::bake(world.as_ref(), coord) }),
            );
        }
        true
    }

    /// Forgets the tiles no level shown or being built will sample again.
    fn forget_far_tiles(&mut self) {
        let centres: Vec<(LevelSpec, [f64; 2])> = self
            .levels
            .iter()
            .flat_map(|l| {
                let shown = l.shown.as_ref().map(|s| (l.spec, s.centre));
                let building = l.build.as_ref().map(|b| (l.spec, b.centre()));
                shown.into_iter().chain(building)
            })
            .collect();
        if !centres.is_empty() {
            self.field.retain(|c| keep_tile(c, &centres));
        }
    }
}

/// Keeps the clipmap around the primary observer: starts bakes and level
/// builds, collects what finished. Never waits on a task.
pub(crate) fn update_clipmap(
    mut commands: Commands,
    settings: Res<crate::fog_pass::GroundFog>,
    observers: Option<Res<Observers>>,
    source: Option<Res<WorldSourceRes>>,
    seed: Option<Res<WorldSeed>>,
    clipmap: Option<ResMut<FogClipmap>>,
) {
    let Some(source) = source else {
        return;
    };
    let Some(mut clipmap) = clipmap else {
        if let Some(seed) = seed {
            commands.insert_resource(FogClipmap::new(FogField::new(seed.0)));
        }
        return;
    };
    let clipmap = &mut *clipmap;
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);

    clipmap.collect_bakes();

    let Some(observer) = observers.and_then(|o| o.primary) else {
        return;
    };
    let (x, z) = (observer.x(), observer.z());
    let weather = settings.weather;
    let budget = clipmap.budget.unwrap_or_else(|| bake_budget(pool));
    // The fine level first: it is what the observer stands in, and its
    // tiles are asked for before the coarse level's take the budget.
    for index in 0..clipmap.levels.len() {
        let bakes_in_flight = clipmap.bakes.len();
        let level = &mut clipmap.levels[index];
        level.show_finished(index, bakes_in_flight);
        let spec = level.spec;
        // A level still waiting for its tiles follows the observer: after a
        // jump it starts again around the new place instead of finishing,
        // sampling and showing the old one first. Bakes in flight for the
        // old place are left to land; they stay only if a level needs them.
        // It takes the weather of the moment too, which its tiles do not
        // depend on.
        if let Some(Build::Baking {
            centre,
            weather: building,
            ..
        }) = &mut level.build
        {
            if spec.needs_recentre(Some(*centre), x, z) {
                level.build = None;
            } else {
                *building = weather;
            }
        }
        let current = level
            .build
            .as_ref()
            .map(Build::centre)
            .or(level.shown.as_ref().map(|s| s.centre));
        let stale =
            level.shown.as_ref().is_some_and(|s| s.weather != weather) && level.build.is_none();
        if level.build.is_none() && (stale || spec.needs_recentre(current, x, z)) {
            level.build = Some(Build::Baking {
                centre: spec.snapped_centre(x, z),
                weather,
                started: Instant::now(),
            });
        }
        let Some(Build::Baking {
            centre,
            weather,
            started,
        }) = level.build
        else {
            continue;
        };
        if clipmap.bake_missing(spec.sampled_rect(centre), (x, z), budget, &source, pool) {
            continue;
        }
        clipmap.serial += 1;
        let serial = clipmap.serial;
        let field = clipmap.field.clone();
        let task = pool.spawn(async move { sample_level(&field, spec, centre, weather, serial) });
        clipmap.levels[index].build = Some(Build::Sampling {
            centre,
            task,
            started,
        });
    }
    clipmap.forget_far_tiles();
}

/// The levels the render world draws, extracted every frame (two `Arc`
/// clones).
#[derive(Resource, Default)]
pub(crate) struct ExtractedClipmap {
    pub levels: [Option<Arc<LevelTexels>>; 2],
}

pub(crate) fn extract_clipmap(
    mut extracted: ResMut<ExtractedClipmap>,
    clipmap: Extract<Option<Res<FogClipmap>>>,
) {
    extracted.levels = clipmap.as_ref().map_or([None, None], |c| c.shown());
}

/// A level's texture in the render world and the serial of what it holds.
pub(crate) struct GpuLevel {
    texture: Texture,
    pub view: TextureView,
    serial: u64,
}

/// The two textures, created once.
#[derive(Resource)]
pub(crate) struct ClipmapTextures {
    pub levels: [GpuLevel; 2],
}

pub(crate) fn init_textures(mut commands: Commands, device: Res<RenderDevice>) {
    let level = |spec: LevelSpec, label: &'static str| {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: spec.texels,
                height: spec.texels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TEXEL_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        GpuLevel {
            texture,
            view,
            serial: 0,
        }
    };
    commands.insert_resource(ClipmapTextures {
        levels: [
            level(LevelSpec::FINE, "fog_clipmap_fine"),
            level(LevelSpec::COARSE, "fog_clipmap_coarse"),
        ],
    });
}

/// Copies a level into its texture on the frame it is extracted.
pub(crate) fn upload_levels(
    extracted: Res<ExtractedClipmap>,
    textures: Option<ResMut<ClipmapTextures>>,
    queue: Res<RenderQueue>,
) {
    let Some(mut textures) = textures else {
        return;
    };
    for (gpu, level) in textures.levels.iter_mut().zip(&extracted.levels) {
        let Some(level) = level else {
            continue;
        };
        if gpu.serial == level.serial {
            continue;
        }
        let texels = level.spec.texels;
        queue.write_texture(
            gpu.texture.as_image_copy(),
            &level.bytes,
            TexelCopyBufferLayout {
                offset: 0,
                // 256 texels of 8 bytes: a multiple of the 256-byte row
                // alignment a copy needs.
                bytes_per_row: Some(texels * u32::try_from(TEXEL_BYTES).unwrap_or(8)),
                rows_per_image: Some(texels),
            },
            Extent3d {
                width: texels,
                height: texels,
                depth_or_array_layers: 1,
            },
        );
        gpu.serial = level.serial;
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    reason = "snapped centres and half floats are exact values, and test indices are small"
)]
mod tests {
    use super::*;

    #[test]
    fn a_level_snaps_to_its_texel_grid() {
        let spec = LevelSpec::FINE;
        assert_eq!(spec.snapped_centre(0.0, 0.0), [0.0, 0.0]);
        assert_eq!(spec.snapped_centre(15.9, -16.1), [0.0, -32.0]);
        assert_eq!(spec.snapped_centre(-5950.0, -13650.0), [-5952.0, -13664.0]);
        // Walking a few metres does not move a texel: the same centre.
        assert_eq!(
            spec.snapped_centre(100.0, 100.0),
            spec.snapped_centre(103.0, 97.0)
        );
        let coarse = LevelSpec::COARSE;
        assert_eq!(
            coarse.snapped_centre(-16_000.0, -13_700.0),
            [-16_000.0, -13_750.0]
        );
        // Texel centres fall on one world lattice whatever the centre.
        for (x, z) in [(0.0, 0.0), (1234.5, -987.6), (-40_000.0, 22_222.0)] {
            let o = spec.origin(spec.snapped_centre(x, z));
            let r = spec.sampled_rect(spec.snapped_centre(x, z));
            assert!((o[0] / spec.texel_m).fract().abs() < 1e-9, "{o:?}");
            assert!(((r[0] - 16.0) / spec.texel_m).fract().abs() < 1e-9, "{r:?}");
        }
        assert!((spec.extent_m() - 8192.0).abs() < 1e-9);
        assert!((coarse.extent_m() - 64_000.0).abs() < 1e-9);
    }

    #[test]
    fn a_level_is_rebuilt_once_the_observer_is_a_quarter_of_the_way_out() {
        let spec = LevelSpec::FINE;
        assert!(spec.needs_recentre(None, 0.0, 0.0));
        let c = spec.snapped_centre(0.0, 0.0);
        assert!(!spec.needs_recentre(Some(c), 0.0, 0.0));
        // A quarter of the half extent is 1024 m.
        assert!(!spec.needs_recentre(Some(c), 1000.0, -1000.0));
        assert!(spec.needs_recentre(Some(c), 1030.0, 0.0));
        assert!(spec.needs_recentre(Some(c), 0.0, -1030.0));
        let coarse = LevelSpec::COARSE;
        let c = coarse.snapped_centre(0.0, 0.0);
        assert!(!coarse.needs_recentre(Some(c), 7900.0, 7900.0));
        assert!(coarse.needs_recentre(Some(c), 8100.0, 0.0));
        // After the rebuild the observer is near the new centre again.
        let moved = coarse.snapped_centre(8100.0, 0.0);
        assert!(!coarse.needs_recentre(Some(moved), 8100.0, 0.0));
    }

    #[test]
    fn half_floats_round_trip_heights_and_shares() {
        for v in [
            0.0f32, 1.0, -1.0, 0.5, 0.25, 0.125, 2.0, 1024.0, 65_504.0, -10.0,
        ] {
            assert_eq!(f16_value(f16_bits(v)), v, "{v}");
        }
        // Heights keep a tenth of a metre under 256 m and a metre to 2 km;
        // shares keep a thousandth.
        for i in 0..4000 {
            let v = -100.0 + i as f32 * 0.537;
            let back = f16_value(f16_bits(v));
            let tolerance = if v.abs() < 256.0 { 0.125 } else { 1.0 };
            assert!((back - v).abs() <= tolerance, "{v} -> {back}");
            let share = i as f32 / 4000.0;
            let back = f16_value(f16_bits(share));
            assert!((back - share).abs() <= 5e-4, "{share} -> {back}");
        }
        assert_eq!(f16_value(f16_bits(1e9)), 65_504.0);
        assert_eq!(f16_value(f16_bits(-1e9)), -65_504.0);
        assert_eq!(f16_bits(f32::NAN), 0);
        assert_eq!(f16_value(f16_bits(1e-6)), 0.0);
        // Rounding is to nearest: 1 + 1.5 ulp rounds up to 1 + 2 ulp.
        let ulp = 1.0 / 1024.0;
        assert_eq!(f16_value(f16_bits(1.0 + 1.6 * ulp)), 1.0 + 2.0 * ulp);
        assert_eq!(f16_value(f16_bits(1.0 + 0.4 * ulp)), 1.0);
        // Its bits are the IEEE ones the GPU reads.
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(65_504.0), 0x7bff);
    }

    #[test]
    fn a_texel_round_trips_its_sample() {
        let sample = FogSample {
            base: 18.3,
            top: 74.7,
            density: 0.85,
        };
        let back = decode_texel(encode_texel(&sample, true));
        assert!((back.top - sample.top).abs() < 0.05, "{back:?}");
        assert!((back.base - sample.base).abs() < 0.01, "{back:?}");
        assert!((back.density - sample.density).abs() < 1e-3, "{back:?}");
        assert_eq!(
            decode_texel(encode_texel(&FogSample::NONE, false)),
            FogSample::NONE
        );
        // A density out of range is clamped; the flag says whether the
        // tile was baked.
        let loud = FogSample {
            density: 3.0,
            ..sample
        };
        assert_eq!(decode_texel(encode_texel(&loud, true)).density, 1.0);
        assert_eq!(f16_value(encode_texel(&sample, true)[3]), 1.0);
        assert_eq!(f16_value(encode_texel(&sample, false)[3]), 0.0);
    }

    #[test]
    fn an_unbaked_level_is_no_fog_and_says_so() {
        let field = FogField::new(strata_core::Seed(42));
        let spec = LevelSpec {
            texels: 8,
            texel_m: 32.0,
        };
        let level = sample_level(&field, spec, [0.0, 0.0], Weather::FULL, 7);
        assert_eq!(level.bytes.len(), 8 * 8 * TEXEL_BYTES);
        assert_eq!(level.unbaked, 64);
        assert!(!level.has_fog());
        assert_eq!(level.serial, 7);
        // Every texel decodes to nothing and says it was not baked.
        for texel in level.bytes.as_chunks::<TEXEL_BYTES>().0 {
            let halves = [0, 2, 4, 6].map(|i| u16::from_le_bytes([texel[i], texel[i + 1]]));
            assert_eq!(decode_texel(halves), FogSample::NONE);
            assert_eq!(f16_value(halves[3]), 0.0);
        }
    }

    /// A tile with no world behind it: a basin whose top rises one metre a
    /// node eastwards over wet ground, so every texel reads a different
    /// value.
    fn sloping_tile(coord: TileCoord) -> BasinTile {
        let n = strata_weather::TILE_NODES;
        let mut top = Vec::with_capacity(n * n);
        for _ in 0..n {
            for i in 0..n {
                top.push(100.0 + i as f32);
            }
        }
        BasinTile {
            coord,
            ground: vec![40.0; n * n],
            top,
            floor: vec![30.0; n * n],
            hydration: vec![1.0; n * n],
            sea_distance: vec![3000.0; n * n],
            offshore: vec![0.0; n * n],
        }
    }

    #[test]
    fn a_half_baked_level_flags_each_texel_by_its_tile() {
        let mut field = FogField::new(strata_core::Seed(42));
        field
            .insert(sloping_tile(TileCoord { x: 0, z: 0 }))
            .expect("well formed");
        // Sixteen texels 100 m apart across x = 0: the west half's tile is
        // not baked.
        let spec = LevelSpec {
            texels: 16,
            texel_m: 100.0,
        };
        let level = sample_level(&field, spec, [0.0, 1000.0], Weather::FULL, 1);
        assert_eq!(level.unbaked, 16 * 8);
        for (k, texel) in level.bytes.as_chunks::<TEXEL_BYTES>().0.iter().enumerate() {
            let flag = f16_value(u16::from_le_bytes([texel[6], texel[7]]));
            let density = f16_value(u16::from_le_bytes([texel[4], texel[5]]));
            if k % 16 < 8 {
                assert!(flag == 0.0 && density == 0.0, "{k}");
            } else {
                assert!(flag == 1.0 && density > 0.0, "{k}");
            }
        }
    }

    #[test]
    fn a_texel_holds_the_field_at_its_centre() {
        let mut field = FogField::new(strata_core::Seed(42));
        let coord = TileCoord { x: 0, z: 0 };
        field.insert(sloping_tile(coord)).expect("well formed");
        let spec = LevelSpec {
            texels: 16,
            texel_m: 32.0,
        };
        let centre = spec.snapped_centre(1000.0, 1000.0);
        let weather = Weather {
            fog: 1.0,
            sea_fog: 0.0,
            drift: [0.0, 0.0],
        };
        let level = sample_level(&field, spec, centre, weather, 1);
        assert_eq!(level.unbaked, 0);
        assert!(level.has_fog());
        let origin = spec.origin(centre);
        let n = spec.texels as usize;
        for (i, j) in [(0, 0), (15, 0), (3, 9), (15, 15)] {
            let at = (j * n + i) * TEXEL_BYTES;
            let halves = [0, 2, 4, 6]
                .map(|k| u16::from_le_bytes([level.bytes[at + k], level.bytes[at + k + 1]]));
            let texel = decode_texel(halves);
            // The shader's bilinear lookup puts texel (i, j)'s value at its
            // centre, half a texel in from its corner.
            let x = origin[0] + (i as f64 + 0.5) * spec.texel_m;
            let z = origin[1] + (j as f64 + 0.5) * spec.texel_m;
            let expected = field.sample(x, z, &weather).expect("baked");
            assert!(
                (texel.top - expected.top).abs() < 0.07,
                "{i},{j}: {texel:?} {expected:?}"
            );
            assert!(
                (texel.base - expected.base).abs() < 0.07,
                "{texel:?} {expected:?}"
            );
            assert!(
                (texel.density - expected.density).abs() < 1e-3,
                "{texel:?} {expected:?}"
            );
            assert!(
                expected.density > 0.5,
                "a wet basin 60 m deep: {expected:?}"
            );
        }
        assert!(
            level.max_top >= 100.0 && level.min_base <= 30.0,
            "{level:?}"
        );
    }

    /// A bowl 60 m deep and 3 km across round the origin, wet: fog in the
    /// middle, none on the plain round it.
    struct Bowl;

    impl strata_worldgen::WorldSource for Bowl {
        #[allow(clippy::cast_possible_truncation, reason = "test heights are small")]
        fn sample(&self, x: f64, z: f64) -> strata_worldgen::Sample {
            let r = (x * x + z * z).sqrt();
            strata_worldgen::Sample {
                height: (100.0 - 60.0 * (1.0 - (r / 1500.0).min(1.0))) as f32,
                biomes: [
                    (strata_registry::BiomeId(0), 1.0),
                    (strata_registry::BiomeId(0), 0.0),
                ],
                provinces: strata_worldgen::Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.8,
                hydration: 0.9,
                cover: 0.5,
                landform: strata_worldgen::Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    /// Small levels, so a test bakes four tiles and not eighty.
    const SMALL: [LevelSpec; 2] = [
        LevelSpec {
            texels: 16,
            texel_m: 32.0,
        },
        LevelSpec {
            texels: 16,
            texel_m: 250.0,
        },
    ];

    fn observe(app: &mut App, x: f64, z: f64) {
        let at = strata_core::WorldPos::new(x, 0.0, z);
        app.insert_resource(Observers {
            positions: vec![at],
            primary: Some(at),
        });
    }

    fn clipmap_app(x: f64, z: f64) -> App {
        let mut app = App::new();
        app.insert_resource(crate::fog_pass::GroundFog::default())
            .insert_resource(WorldSourceRes(Arc::new(Bowl)))
            .insert_resource(FogClipmap::with_specs(
                FogField::new(strata_core::Seed(7)),
                SMALL,
            ))
            .add_systems(bevy::app::Update, update_clipmap);
        observe(&mut app, x, z);
        app
    }

    /// Updates until `done` or a generous time is up, checking every frame
    /// that the bakes in flight keep to the budget.
    fn run_until(app: &mut App, done: impl Fn(&FogClipmap) -> bool) -> bool {
        for _ in 0..20_000 {
            app.update();
            let clipmap = app.world().resource::<FogClipmap>();
            // The coarse level alone needs four tiles.
            assert!(
                clipmap.bakes_in_flight() <= 1,
                "{} bakes in flight, budget 1",
                clipmap.bakes_in_flight()
            );
            if done(clipmap) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        false
    }

    fn shown_at(clipmap: &FogClipmap, x: f64, z: f64, weather: Weather) -> bool {
        clipmap.shown().iter().zip(SMALL).all(|(level, spec)| {
            level.as_ref().is_some_and(|l| {
                l.centre == spec.snapped_centre(x, z) && l.weather == weather && l.unbaked == 0
            })
        })
    }

    use bevy::app::App;

    #[test]
    fn the_clipmap_builds_round_the_observer_and_rebuilds_for_new_weather() {
        let mut app = clipmap_app(200.0, -300.0);
        let weather = crate::fog_pass::GroundFog::WEATHER;
        assert!(run_until(&mut app, |c| shown_at(c, 200.0, -300.0, weather)));
        let clipmap = app.world().resource::<FogClipmap>();
        let fine = clipmap.shown()[0].clone().expect("shown");
        assert!(fine.has_fog(), "the bowl holds fog: {fine:?}");
        // Each tile was baked once: nothing is in flight or building.
        assert_eq!(clipmap.bakes_in_flight(), 0);
        assert!(clipmap.build_centre(0).is_none() && clipmap.build_centre(1).is_none());

        let thin = Weather {
            fog: 0.3,
            ..weather
        };
        app.world_mut()
            .resource_mut::<crate::fog_pass::GroundFog>()
            .weather = thin;
        assert!(run_until(&mut app, |c| shown_at(c, 200.0, -300.0, thin)));
        let rebuilt = app.world().resource::<FogClipmap>().shown()[0]
            .clone()
            .expect("shown");
        assert!(rebuilt.serial > fine.serial);
        assert_ne!(rebuilt.bytes, fine.bytes, "thinner weather, other texels");
    }

    #[test]
    fn a_build_follows_the_observer_and_the_old_tiles_are_forgotten() {
        let mut app = clipmap_app(200.0, -300.0);
        // The first frame starts both levels baking round the start, with
        // the one bake the budget allows spent on the fine level's tile
        // nearest the observer (it needs two, x = -1 and 0).
        app.update();
        let start = SMALL.map(|s| s.snapped_centre(200.0, -300.0));
        let clipmap = app.world().resource::<FogClipmap>();
        assert_eq!(clipmap.build_centre(0), Some(start[0]));
        assert_eq!(clipmap.build_centre(1), Some(start[1]));
        assert_eq!(
            clipmap.bakes.keys().copied().collect::<Vec<_>>(),
            [TileCoord { x: 0, z: -1 }]
        );
        // A respawn 100 km away before those tiles are in: the builds start
        // again round the new place on the next frame, not after the old
        // place has been baked, sampled and shown.
        let (x, z) = (100_000.0, 100_000.0);
        observe(&mut app, x, z);
        app.update();
        let clipmap = app.world().resource::<FogClipmap>();
        for (level, spec) in SMALL.iter().enumerate() {
            assert_eq!(
                clipmap.build_centre(level),
                Some(spec.snapped_centre(x, z)),
                "level {level}"
            );
        }
        let weather = crate::fog_pass::GroundFog::WEATHER;
        assert!(run_until(&mut app, |c| shown_at(c, x, z, weather)));
        // The start's tiles landed after the jump and were dropped.
        assert!(run_until(&mut app, |c| c.bakes_in_flight() == 0));
        app.update();
        let clipmap = app.world().resource::<FogClipmap>();
        assert!(!clipmap.field.contains(TileCoord { x: 0, z: -1 }));
        assert!(clipmap.field.contains(TileCoord::containing(x, z)));
    }

    #[test]
    fn bakes_are_budgeted_by_the_pool() {
        for (threads, budget) in [(1, 1), (2, 1), (4, 2), (8, 4)] {
            let pool = bevy::tasks::TaskPoolBuilder::new()
                .num_threads(threads)
                .build();
            assert_eq!(bake_budget(&pool), budget, "{threads}");
        }
    }

    #[test]
    fn tiles_are_kept_around_every_level() {
        let fine = (LevelSpec::FINE, [0.0, 0.0]);
        // The fine level samples -4 km to 4 km: tiles -1 and 0, one more
        // ring each side.
        assert!(keep_tile(TileCoord { x: 0, z: 0 }, &[fine]));
        assert!(keep_tile(TileCoord { x: -2, z: 1 }, &[fine]));
        assert!(!keep_tile(TileCoord { x: 3, z: 0 }, &[fine]));
        let coarse = (LevelSpec::COARSE, [0.0, 0.0]);
        assert!(keep_tile(TileCoord { x: 3, z: -4 }, &[fine, coarse]));
        assert!(!keep_tile(TileCoord { x: 9, z: 0 }, &[fine, coarse]));
    }
}
