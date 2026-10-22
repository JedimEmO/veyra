//! The world as a pure function of the seed.
//!
//! `sample(seed, x, z)` lives here, with no Bevy and no IO, so the server,
//! the client, the tests and the map tool all get the same bits (PLAN
//! principle 1). [`WorldSource`] is the port; [`ProceduralWorld`] is the
//! stack of stages from PLAN section 4.4 (continents, climate, biome,
//! relief, surface). A persisted-edits source later wraps a
//! `ProceduralWorld` behind the same trait.
//!
//! Design constraint from ADR 0003: the sampler is called from many
//! compute-pool threads at once, so it is `Send + Sync` and allocates
//! nothing per sample. Coordinates are `f64` metres with `y` up and sea
//! level at zero; `x` and `z` here are the horizontal components of a
//! `WorldPos`.
//!
//! "The same bits" has to hold across platforms too, so the stages use only
//! operations IEEE 754 makes correctly rounded: `+ - * /`, `sqrt`, `floor`,
//! `abs`, `min`, `max`, `mul_add` avoided, and integer hashing. The
//! platform libm's `sin`, `cos`, `exp`, `powf`, `hypot` and friends differ
//! by an ulp between glibc, musl, MSVC and Apple, so they are not used per
//! sample; the transcendentals are only in the constructor, for the
//! seed-chosen axis of the mountain ranges and the seed-chosen prevailing
//! wind, and they go through the pure-Rust `libm` crate, which is
//! bit-identical everywhere.
//!
//! A change to this crate that moves the ground bumps
//! [`GENERATOR_VERSION`] in the same commit: a client and a server compare
//! [`world_version`] before they share a world, and the code is the one
//! input it cannot hash for itself.

pub mod crossings;
pub mod erosion;
pub mod geology;
pub mod hydrology;
pub mod measure;
pub mod noise;
pub mod params;
pub mod procedural;
pub mod roads;
pub mod roadside;
pub mod settlement;
pub mod sites;
pub mod spawn;
mod strata;
pub mod version;

pub use geology::{ProvinceField, ProvinceMix, ProvinceRule};
pub use params::WorldParams;
pub use procedural::{Classification, Climate, Continent, ProceduralWorld, Terms};
use serde::{Deserialize, Serialize};
pub use settlement::{
    GroundKind, LayoutBuilding, LayoutLight, LayoutPlant, SettlementLayout, Site,
};
pub use spawn::find_land_near;
use strata_data::{BiomeId, ProvinceId};
pub use version::{GENERATOR_VERSION, world_version};

/// What the world is at one point.
///
/// No normal: the terrain derives normals from neighbouring height samples
/// so they are continuous across nodes and LODs (ADR 0003), and the map
/// tool does not need one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Height in metres, sea level at zero.
    pub height: f32,
    /// The two heaviest biomes and their share of the pair, best first;
    /// the shares sum to one. Where three windows meet the height blends a
    /// third biome too, so this is the pair to colour a vertex with, not
    /// the exact recipe of the height.
    pub biomes: [(BiomeId, f32); 2],
    /// The two heaviest geological provinces and their share of the pair,
    /// best first, the shares summing to one, as [`biomes`](Self::biomes)
    /// is (ticket 0004, step 3). A province is what the ground is made of:
    /// its bare faces, and from later steps the shape of its relief, its
    /// beds and how water cuts it. Where three provinces meet the field
    /// blends a third too, so this is the pair to paint with, not the
    /// whole recipe. [`Sample::NO_PROVINCES`] in a world without geology.
    pub provinces: [(ProvinceId, f32); 2],
    /// The texture array layer of the surface material, from the winning
    /// biome and the slope.
    pub material: u8,
    /// The climate's moisture at the point, arid (`0`) to wet (`1`):
    /// mostly the rain marched upwind through the relief (ADR 0006). Not
    /// used by the terrain; placement reads it (ADR 0005).
    pub moisture: f32,
    /// How wet the ground itself is, parched (`0`) to waterlogged (`1`):
    /// the climate's rainfall raised in hollows and lowered on slopes
    /// (ADR 0006). Grass reads it for its lushness, the terrain tint for
    /// its far colour, so a valley is greener than the ridge above it.
    pub hydration: f32,
    /// How much the ground can carry, bare (`0`) to thicket (`1`):
    /// [`hydration`](Self::hydration) through a curve, broken up by a
    /// patch noise of a few hundred metres so a forest has groves and
    /// clearings (ADR 0006), times the provinces' `rock.cover` by their
    /// weights (badlands shed soil too fast to hold). Placement multiplies
    /// a species' fit by it.
    pub cover: f32,
    /// The shape of the ground around the point, for materials and
    /// placement: how steep, which way it faces, and whether it is a hollow,
    /// a gully or a ridge.
    pub landform: Landform,
    /// The water the point is in or beside (ticket 0005, step 2): the lake
    /// over it, or the river whose valley it is in, or the lake whose shore
    /// it is on; `None` where no water is near. The point is under water
    /// where [`height`](Self::height) is below its surface
    /// ([`Sample::in_water`]); the sea is [`WorldSource::sea_level`]'s.
    #[serde(default)]
    pub water: Option<SampleWater>,
}

/// The water at a point, as a sample carries it: ADR 0010's record in 32
/// bytes, the depth and the speed folded into the bed and the flow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SampleWater {
    /// The water's surface, metres.
    pub surface_m: f32,
    /// The bed under a river's centreline, or a lake's coarse bed at the
    /// point: the surface less the water's depth there.
    pub bed_m: f32,
    /// A river's channel width, metres; a lake's, the side of a square of
    /// its area.
    pub width_m: f32,
    /// Metres from a river's centreline, positive on the side `(-flow.z,
    /// flow.x)` points to; for a lake, from its shore, negative inside.
    pub signed_distance_m: f32,
    /// Downstream times the mean speed, m/s; zero on a lake.
    pub flow: [f32; 2],
    /// Discharge, m3/s (a lake's: what leaves it).
    pub discharge_m3s: f32,
    /// Strahler order; zero on a lake.
    pub order: u8,
    /// Channel, fall, mouth or lake.
    pub kind: hydrology::WaterKind,
    /// How far the carve lowered the ground at the point, decimetres
    /// (ticket 0005, step 3): a valley's wall cut into a hillside by metres
    /// to hundreds, a natural cliff the valley left standing by nothing.
    /// What tells the one from the other, for the terrain's look and the
    /// placement. It sits in the record's padding, so the record stays 32
    /// bytes, and it is not serialized: the pinned samples are the record
    /// step 2 blessed, and this is derived from the heights they pin.
    #[serde(skip)]
    pub cut_dm: u16,
}

// The record rides in every sample; the cut fits its padding.
const _: () = assert!(core::mem::size_of::<Option<SampleWater>>() == 32);

impl SampleWater {
    /// Past a river's banks, how far out the water's edge reaches, metres:
    /// a floodplain under the surface is dry, and a creek's eased bank runs
    /// on a little past where its record says it tops out. The terrain lays
    /// a bed and wets the bank over it, and placement keeps off it, so it
    /// is said once, here, for both.
    pub const EDGE_REACH_M: f32 = 2.5;

    /// How high over the water's surface its bed is wetted where its edge
    /// reaches, metres: the band the terrain lays whole in the bed's gravel
    /// or sand and darkens most, and nothing but a plant of the wet bank
    /// grows on.
    pub const WETTED_BED_M: f32 = 0.35;

    /// How far the carve lowered the ground at the point, metres
    /// ([`cut_dm`](Self::cut_dm)).
    #[must_use]
    pub fn cut_m(&self) -> f32 {
        f32::from(self.cut_dm) * 0.1
    }

    /// The water's depth over its bed at the centreline, metres.
    #[must_use]
    pub fn depth_m(&self) -> f32 {
        self.surface_m - self.bed_m
    }

    /// The mean speed, m/s.
    #[must_use]
    pub fn speed_mps(&self) -> f32 {
        (self.flow[0] * self.flow[0] + self.flow[1] * self.flow[1]).sqrt()
    }

    /// How far from a river's centreline its banks top out, metres: the
    /// carve's channel ([`hydrology::RiverParams::bank_top_m`], the one
    /// formula both use) with the shipped numbers. Past it the record is
    /// the valley's: ground there under the surface is a floodplain the
    /// banks keep the water off. A lake's water has no bank; this is its
    /// width.
    #[must_use]
    pub fn bank_m(&self) -> f32 {
        if self.kind == hydrology::WaterKind::Lake {
            return self.width_m;
        }
        let top = hydrology::RiverParams::DEFAULT
            .bank_top_m(f64::from(self.width_m), f64::from(self.depth_m()));
        // Metres to a river's bank, small and well inside f32.
        #[allow(clippy::cast_possible_truncation)]
        let top = top as f32;
        top
    }

    /// Whether the record's water can stand at the point: anywhere in a
    /// lake, and within a river's banks ([`bank_m`](Self::bank_m)).
    #[must_use]
    pub fn holds_water_at(&self) -> bool {
        self.kind == hydrology::WaterKind::Lake || self.signed_distance_m.abs() <= self.bank_m()
    }

    /// Whether the water's edge reaches the point: anywhere round a lake,
    /// and within a river's banks and [`EDGE_REACH_M`](Self::EDGE_REACH_M).
    #[must_use]
    pub fn edge_reaches(&self) -> bool {
        self.kind == hydrology::WaterKind::Lake
            || self.signed_distance_m.abs() <= self.bank_m() + Self::EDGE_REACH_M
    }
}

impl From<hydrology::Water> for SampleWater {
    fn from(water: hydrology::Water) -> Self {
        Self {
            surface_m: water.surface_m,
            bed_m: water.bed_m,
            width_m: water.width_m,
            signed_distance_m: water.signed_distance_m,
            flow: [
                water.flow_dir[0] * water.speed_mps,
                water.flow_dir[1] * water.speed_mps,
            ],
            discharge_m3s: water.discharge_m3s,
            order: water.order,
            kind: water.kind,
            cut_dm: 0,
        }
    }
}

/// The shape of the ground around a point.
///
/// Everything a rule about where rock shows, where water gathers or where a
/// tree grows would otherwise have to rebuild from neighbouring samples.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Landform {
    /// Rise over run of the ground: the biomes' relief, the eroded gullies
    /// and, since ticket 0004's step 4, a range's own metres
    /// ([`Continent::range_m`]), so a range's flank reads as steep ground.
    /// The continent's base elevation (the coast's ramp and the elevation's
    /// share of a range) is left out.
    pub slope: f32,
    /// The downhill direction as a unit `(x, z)`, or zero where the ground
    /// is flat. A slope facing the sun is `aspect` against its direction.
    pub aspect: [f32; 2],
    /// Where the point sits against the ground a couple of hundred metres
    /// around it: `-1` on a knoll, `1` in a hollow water collects in.
    pub hollow: f32,
    /// The finest erosion gullies at the point: `-1` at the bottom of one,
    /// `1` on the ridge between two, `0` where the ground was not carved.
    /// Drainage runs where it is negative.
    pub ridge: f32,
    /// How much of the point a settlement has taken, `0` wild ground, `1`
    /// its platform or its road: nothing is placed where it is over a half.
    #[serde(default)]
    pub developed: f32,
    /// Which of its province's beds shaped the ground here, counting up
    /// (ticket 0004, step 5): a riser for the rock kit to stand its pieces
    /// on, a hard bed for a river to fall over, a colour band a bed for the
    /// terrain. The bed the ground lay in before the gullies, the one whose
    /// profile made the riser or the bench, and not the older bed a gully
    /// has since cut down to (the review of step 5 found a label read after
    /// the gullies named 22 % of bed risers a hard bed's riser). Of the
    /// heaviest province whose beds shape the ground at the point. A bed's
    /// index can be `0` or below (the bed coordinate is the height plus the
    /// dip), so it alone does not say whether beds act: `bed == 0 &&
    /// bed_phase == 0.0` is ground no bed shapes (no province there has
    /// beds, or the ground is too gentle or too near the sea for them).
    #[serde(default)]
    pub bed: i32,
    /// Where in that bed the ground lay, `0` at its base towards `1` at its
    /// top: a hard bed's riser is its lowest `max(WorldParams::bed_riser,
    /// 1.5 / WorldParams::bed_steepening)`, 0.3 with the shipped numbers,
    /// its bench the rest. `0` where there are no beds.
    #[serde(default)]
    pub bed_phase: f32,
    /// Whether that bed is hard: a cliff and a bench, or a slope.
    #[serde(default)]
    pub bed_hard: bool,
    /// How far a road's grade moved the ground at the point, metres
    /// (ticket 0005, step 6): positive where it cut the ground down,
    /// negative where it filled it up. What tells a road's bank from a
    /// slope the land made, for the terrain's look (rock only where a cut is
    /// deep and steep) and the placement. Derived from the heights, so not
    /// serialized, as [`SampleWater::cut_dm`] is not.
    #[serde(skip)]
    pub cut_m: f32,
    /// How much of the point is a road's cleared way, `0..=1` (ticket 0005,
    /// step 6): one on its carriageway and verge (a trail's line and a
    /// strip either side), fading to nothing past them. Nothing but grass is
    /// placed where it is over a half, on the server as on the client. Not
    /// serialized.
    #[serde(skip)]
    pub cleared: f32,
}

impl Sample {
    /// The provinces of a sample in a world without geology.
    pub const NO_PROVINCES: [(ProvinceId, f32); 2] =
        [(ProvinceId::NONE, 1.0), (ProvinceId::NONE, 0.0)];

    /// The province with the larger weight, [`ProvinceId::NONE`] in a world
    /// without geology.
    #[must_use]
    pub fn dominant_province(&self) -> ProvinceId {
        self.provinces[0].0
    }

    /// The biome with the larger weight.
    #[must_use]
    pub fn dominant_biome(&self) -> BiomeId {
        self.biomes[0].0
    }

    /// Whether the point is under the sea, given the source's sea level.
    #[must_use]
    pub fn is_submerged(&self, sea_level: f32) -> bool {
        self.height < sea_level
    }

    /// Whether the ground is under a river's or a lake's surface.
    #[must_use]
    pub fn in_water(&self) -> bool {
        self.water
            .is_some_and(|water| self.height < water.surface_m)
    }

    /// The river's or the lake's water standing over the ground at the
    /// point: [`in_water`](Self::in_water), and within a river's banks
    /// ([`SampleWater::holds_water_at`]). A floodplain lower than its
    /// river's level is under the surface but dry; what a body wades in and
    /// what the terrain draws is this.
    #[must_use]
    pub fn standing_water(&self) -> Option<&SampleWater> {
        self.water
            .as_ref()
            .filter(|water| self.height < water.surface_m && water.holds_water_at())
    }
}

/// The port every consumer of the world samples through.
///
/// `Send + Sync` because meshing tasks call it from the compute pool; an
/// implementation must not allocate or lock per sample.
pub trait WorldSource: Send + Sync {
    /// The world at horizontal position `(x, z)` in metres.
    fn sample(&self, x: f64, z: f64) -> Sample;

    /// Samples an `n` by `n` grid starting at `(origin_x, origin_z)` with
    /// `step` metres between points into `out`, which is cleared first.
    /// Row-major with `x` fastest: `out[j * n + i]` is the point at
    /// `(origin_x + i * step, origin_z + j * step)`.
    ///
    /// The default samples point by point; an implementation may do better,
    /// but must return exactly what [`sample`](Self::sample) would.
    fn sample_grid(
        &self,
        origin_x: f64,
        origin_z: f64,
        step: f64,
        n: usize,
        out: &mut Vec<Sample>,
    ) {
        out.clear();
        out.reserve(n * n);
        for j in 0..n {
            for i in 0..n {
                // Grid indices are small; the cast is exact far beyond any
                // grid the terrain asks for.
                #[allow(clippy::cast_precision_loss)]
                let (i, j) = (i as f64, j as f64);
                out.push(self.sample(origin_x + i * step, origin_z + j * step));
            }
        }
    }

    /// Height of the sea, in metres.
    fn sea_level(&self) -> f32;
}
