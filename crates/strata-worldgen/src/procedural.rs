//! [`ProceduralWorld`]: the stack of stages from PLAN section 4.4.
//!
//! Every stage is a pure function of the point and the seed, evaluated in
//! order: continents give the land mask and base elevation, climate gives
//! temperature and moisture, the classifier turns those into the two best
//! biomes and a weight, relief adds each biome's noise scaled by its weight,
//! and the surface picks a material from the winner and the slope. Nothing
//! is cached between points, so the sampler is `Send + Sync` and free of
//! allocation per sample, which ADR 0003 needs for the compute pool.
//!
//! **A change here that moves the ground bumps
//! [`GENERATOR_VERSION`](crate::GENERATOR_VERSION) in the same commit**, or
//! a client and a server built either side of it will shake hands over two
//! different worlds.

use strata_core::Seed;
use strata_data::{BiomeId, ClimateWindow, ProvinceShape, ReliefKind, UnitRange, WorldDefs};

use crate::erosion::Erosion;
use crate::geology::{ProvinceField, ProvinceMix, ProvinceRule};
use crate::noise::{Fractal, Shape, Tune, shape_mean, smoothstep};
use crate::params::WorldParams;
use crate::settlement::Site;
use crate::strata::{BedAt, Beds};
use crate::{Landform, Sample, SampleWater, WorldSource};

/// The continent stage's answer for one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Continent {
    /// The biased continent field: negative at sea, positive on land,
    /// growing inland. The climate stage uses it as distance to the coast.
    pub land: f64,
    /// How much of a mountain range the point is on, `0..=1`. It lifts
    /// the elevation, and the mountains' relief grows with the elevation
    /// (see [`WorldParams::mountain_rise`]).
    pub range: f64,
    /// Normalised elevation, `0.5` at sea level. This is what the biome
    /// windows are authored against.
    pub elevation: f64,
    /// Base height in metres before biome relief: the elevation's, which
    /// the biomes and the climate's lapse read.
    pub base_m: f64,
    /// Metres a mountain range adds to the ground under the biomes' relief
    /// over and above the elevation's share of it: [`WorldParams::
    /// range_height_m`] by the range's lift, so a range's height rises over
    /// its whole bell, kilometres of foot, rather than with the mountains'
    /// relief where they stand alone (ticket 0004, step 4). Not in the
    /// elevation, so no biome window and no temperature moves with it.
    pub range_m: f64,
    /// How far up a range's flank the point is, `0..=1`: zero at the foot
    /// of its own metres and beyond, one on the crest line. The foothills'
    /// relief lies down by it ([`WorldParams::flank_damping`]).
    pub flank: f64,
    /// The point after the domain warp, where every continent-scale field
    /// is read. The province field reads it too (ticket 0004, step 3), so
    /// the coast's bends reach its borders and the warp is not computed
    /// twice, which would cost about 0.1 us a sample.
    pub warped: (f64, f64),
}

/// The gradients [`ProceduralWorld::continent_with_gradient`] carries, per
/// metre.
#[derive(Debug, Clone, Copy)]
struct ContinentSlopes {
    /// Of the normalised elevation: the mountains' relief grows with it.
    elevation: (f64, f64),
    /// Of [`Continent::range_m`], which joins the landform's slope as the
    /// relief's does: a range's flank is ground a character stands on and
    /// a tree cannot.
    range_m: (f64, f64),
    /// Of [`Continent::flank`].
    flank: (f64, f64),
    /// Of [`Continent::base_m`]: the beds read the whole ground's slope.
    base_m: (f64, f64),
}

/// The climate stage's answer for one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    /// Cold (`0`) to hot (`1`).
    pub temperature: f64,
    /// Arid (`0`) to wet (`1`).
    pub moisture: f64,
    /// [`Continent::elevation`], carried along so the classifier has all
    /// three fields in one place.
    pub elevation: f64,
}

/// The classifier's answer: the two heaviest biomes and the winner's share
/// of the pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Classification {
    /// Index into the sampler's biome list of the best fit.
    pub best: usize,
    /// Index of the runner-up. Equal to `best` when nothing else is within
    /// the blend gap.
    pub second: usize,
    /// The best biome's weight over the pair, `0.5..=1.0`. The ground
    /// itself may blend more than two biomes near a triple point; this is
    /// the pair a vertex colour or a HUD can show.
    pub weight: f64,
}

/// One biome, prepared for sampling: its window, its noise and its
/// materials resolved to layer indices.
#[derive(Debug, Clone)]
struct BiomeSampler {
    id: BiomeId,
    window: ClimateWindow,
    kind: ReliefKind,
    noise: Fractal,
    /// The relief's first two octaves, for the taps a couple of hundred
    /// metres out that hydration and erosion read. A hollow that would hold
    /// water, or the way a gully runs, is a shape of the land, not of its
    /// grain, and five taps of a six-octave stack would cost more than the
    /// rain march does.
    coarse_noise: Fractal,
    amplitude_m: f64,
    lift_m: f64,
    material: u8,
    slope_material: u8,
    slope_threshold: f64,
    /// How deeply the world's gullies cut into this relief.
    erosion: f64,
    /// The mountain kind's relief, as a share of its full height, grows
    /// over `rise.0..rise.1` of normalised elevation: from
    /// [`WorldParams::mountain_rise_lead`] under where every other biome
    /// has left the blend ([`alone_from`]), over
    /// [`WorldParams::mountain_rise`]. Unused by the other kinds.
    rise: (f64, f64),
    /// The shapes the provinces give this biome's relief class other than
    /// its own (ticket 0004, step 4), each once: variant `v` of a
    /// [`ProvinceRelief`] is `variants[v - 1]`, and variant 0 is the biome's
    /// own `noise`.
    variants: Vec<Variant>,
    /// The relief's octaves the river network's lattice can resolve, at
    /// the full stack's normalisation: every octave at least three lattice
    /// spacings long, and the first always ([`ProceduralWorld::
    /// coarse_ground`]).
    lowest: Fractal,
    /// What each province, by id, makes of this biome's relief; empty in a
    /// world without geology.
    provinces: Vec<ProvinceRelief>,
    /// The biome's own shape's mean value ([`shape_mean`]), which a
    /// province's shape is centred to ([`centred_relief`]).
    own_mean: f64,
    /// Whether its window tops out at the shore step (the ocean and the
    /// coast): its relief stays its own under every province, and no
    /// province's beds shape it (ticket 0004, step 5).
    shore: bool,
    /// Whether any province makes anything else of it. If none does, every
    /// sample takes the biome's own relief by the same instructions as
    /// before provinces had shapes, so a province with no rules is the old
    /// ground to the bit.
    shaped: bool,
}

/// A shape a province gives a biome's relief: the noise, and its first two
/// octaves for the taps (as [`BiomeSampler::coarse_noise`]).
#[derive(Debug, Clone)]
struct Variant {
    noise: Fractal,
    coarse: Fractal,
    /// The shape's mean value ([`shape_mean`]), which the relief centres it
    /// on ([`centred_relief`]).
    mean: f64,
}

/// What one province makes of one biome's relief: which of the biome's
/// variants (0 its own), and the numbers that blend across a border.
#[derive(Debug, Clone, Copy)]
struct ProvinceRelief {
    variant: usize,
    /// Multiplies the biome's amplitude.
    amplitude: f64,
    /// The biome's erosion times the province's multiplier, at most 4.
    erosion: f64,
    tune: Tune,
}

/// The most variants a biome can have: its own and one a province.
const MAX_VARIANTS: usize = strata_data::MAX_PROVINCES + 1;

/// A biome's relief at a point with the provinces' shapes blended: what the
/// sample's relief stage accumulates.
struct BiomeRelief {
    /// Metres.
    here: f64,
    gradient: (f64, f64),
    /// Where between the relief's valley (0) and peak (1) the point sits.
    share: f64,
    /// How deeply the gullies cut.
    erosion: f64,
    /// The coarse relief at the point and the four hydration taps.
    coarse: [f64; 5],
}

/// Metres of relief for a noise value `n` of a shape whose values are
/// signed (`-1..=1`) or not (`0..=1`), in a relief class: plains and hills
/// roll above and below their lift, ridges, dunes and mountains rise from
/// it, and the mountain kind's relief by the share of it that has risen.
/// A biome's own shape is of its class's sign; a province's shape, of
/// either, goes through [`centred_relief`], which reads this for the middle
/// of the biome's own relief.
fn class_relief(
    kind: ReliefKind,
    signed: bool,
    n: f64,
    amplitude: f64,
    lift: f64,
    rise: f64,
) -> f64 {
    match (kind, signed) {
        (ReliefKind::Plains | ReliefKind::Hills, true) => n * 0.5 * amplitude + lift,
        (ReliefKind::Plains | ReliefKind::Hills, false) => (n - 0.5) * amplitude + lift,
        (ReliefKind::Ridges | ReliefKind::Dunes, true) => f64::midpoint(n, 1.0) * amplitude + lift,
        (ReliefKind::Ridges | ReliefKind::Dunes, false) => n * amplitude + lift,
        (ReliefKind::Mountains, true) => (f64::midpoint(n, 1.0) * amplitude + lift) * rise,
        (ReliefKind::Mountains, false) => (n * amplitude + lift) * rise,
    }
}

/// Metres of relief for a value `n` of a province's shape (signed or not,
/// with mean `mean`) in a biome whose own shape is `own` (signed or not, with
/// its mean): the shape's variation about its own mean, by
/// [`class_relief`]'s scale for its sign, over the mean relief the biome's
/// own shape would make at the same amplitude. So a province changes the
/// form of a biome's relief and, through the amplitude, how tall it is, but
/// not where its middle lies.
///
/// Until the review of ticket 0004's step 4 an unsigned shape on plains or
/// hills was centred on 0.5 and a signed one on a rising class on -1 (its
/// floor), and neither is where the shape's values lie: a ridged or
/// billowed shape's mean is about a third, and fbm rarely leaves the middle
/// fifth of its range. The granite and folded highlands stood 66 and 40 m
/// lower on average for it, a folded trench 1 km wide was cut 46 m under the
/// sea inland, and the sandstone and basalt tundra stood 134 and 74 m higher.
fn centred_relief(
    kind: ReliefKind,
    (signed, mean): (bool, f64),
    (own_signed, own_mean): (bool, f64),
    n: f64,
    (amplitude, lift): (f64, f64),
    rise: f64,
) -> f64 {
    let middle = class_relief(kind, own_signed, own_mean, amplitude, lift, 1.0);
    let scale = if signed { 0.5 } else { 1.0 };
    let relief = scale * amplitude * (n - mean) + middle;
    if kind == ReliefKind::Mountains {
        relief * rise
    } else {
        relief
    }
}

/// How much of the mountain kind's relief stands at a point, `0..=1`, and
/// its gradient per metre.
///
/// The share is held constant for the relief's own gradient and for the
/// taps a couple of hundred metres out, but its own gradient joins the
/// slope: across the rise it is the steepest thing on the ground, and a
/// slope that left it out would read a mountain's flank as gentle and grow
/// grass and trees on it.
#[derive(Debug, Clone, Copy)]
struct Rise {
    share: f64,
    gradient: (f64, f64),
}

impl BiomeSampler {
    /// Metres of relief, its gradient, and where between the relief's
    /// valley (`0`) and peak (`1`) the point sits, which erosion fades its
    /// gullies towards. The gradient is the noise's own, so it costs no
    /// extra evaluations, plus the mountain kind's [`Rise`].
    fn relief_gradient_height(&self, x: f64, z: f64, rise: Rise) -> (f64, (f64, f64), f64) {
        let (n, (dx, dz)) = self.noise.get_with_gradient(x, z);
        let height = match self.kind {
            ReliefKind::Plains | ReliefKind::Hills => f64::midpoint(n, 1.0),
            ReliefKind::Ridges | ReliefKind::Dunes | ReliefKind::Mountains => n,
        };
        // Relief is linear in the noise, so its slope is the same factor
        // times the noise's.
        let scale = self.relief_of(1.0, rise.share) - self.relief_of(0.0, rise.share);
        let mut gradient = (dx * scale, dz * scale);
        if self.kind == ReliefKind::Mountains {
            let full = n * self.amplitude_m + self.lift_m;
            gradient.0 += full * rise.gradient.0;
            gradient.1 += full * rise.gradient.1;
        }
        (
            self.relief_of(n, rise.share),
            gradient,
            height.clamp(0.0, 1.0),
        )
    }

    /// This biome's relief at a point under the provinces of `mix`: each
    /// province's shape for the biome's class, with the blendable numbers
    /// (amplitude, damping, offset) blended among the provinces that share
    /// a shape and the heights blended across shapes, by the provinces'
    /// weights; and the erosion blended. Without a mix, or where no province
    /// changes this biome's relief, the biome's own relief exactly.
    ///
    /// A shape's wavelength and stretch are not blended: the same noise at
    /// a wavelength that changes across a border moves its phase by the
    /// distance from the origin times the change in frequency, tens of
    /// cycles inside a kilometre's blend 50 km out, so two wavelengths are
    /// two variants and their heights blend. The gradient leaves out that of
    /// the provinces' weights, as the biomes' blend leaves out its own.
    fn relief_at(
        &self,
        x: f64,
        z: f64,
        rise: Rise,
        radius: f64,
        mix: Option<&ProvinceMix>,
    ) -> BiomeRelief {
        let taps = [
            (0.0, 0.0),
            (radius, 0.0),
            (-radius, 0.0),
            (0.0, radius),
            (0.0, -radius),
        ];
        let Some(mix) = mix.filter(|_| self.shaped) else {
            let (here, gradient, share) = self.relief_gradient_height(x, z, rise);
            return BiomeRelief {
                here,
                gradient,
                share,
                erosion: self.erosion,
                coarse: taps.map(|(dx, dz)| self.coarse_relief(x + dx, z + dz, rise.share)),
            };
        };
        // Per variant: weight, and the weighted amplitude, damping and offset.
        let mut acc = [(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); MAX_VARIANTS];
        let (mut erosion, mut total) = (0.0, 0.0);
        let (mut erosion_lo, mut erosion_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(id, w) in mix.entries() {
            let rule = self.provinces[id.0 as usize];
            let slot = &mut acc[rule.variant];
            slot.0 += w;
            slot.1 += w * rule.amplitude;
            slot.2 += w * rule.tune.damping;
            slot.3 += w * rule.tune.offset;
            erosion += w * rule.erosion;
            total += w;
            erosion_lo = erosion_lo.min(rule.erosion);
            erosion_hi = erosion_hi.max(rule.erosion);
        }
        // One erosion everywhere in the mix is that erosion, not a sum that
        // rounds its way near it.
        #[allow(clippy::float_cmp, reason = "the same value, to the bit")]
        let erosion = if erosion_lo == erosion_hi {
            erosion_lo
        } else {
            erosion / total
        };
        let used = acc.iter().filter(|slot| slot.0 > 0.0).count();
        let mut out = BiomeRelief {
            here: 0.0,
            gradient: (0.0, 0.0),
            share: 0.0,
            erosion,
            coarse: [0.0; 5],
        };
        for (v, &(w, amplitude, damping, offset)) in acc.iter().enumerate() {
            if w <= 0.0 {
                continue;
            }
            // A variant alone weighs one, exactly.
            let weight = if used == 1 { 1.0 } else { w / total };
            let tune = Tune {
                damping: damping / w,
                offset: offset / w,
            };
            let amplitude = self.amplitude_m * (amplitude / w);
            let (noise, coarse) = if v == 0 {
                (&self.noise, &self.coarse_noise)
            } else {
                (&self.variants[v - 1].noise, &self.variants[v - 1].coarse)
            };
            let signed = noise.shape().signed();
            // The biome's own noise by the old instructions, to the bit; a
            // province's shape centred on the mean the biome's own would have.
            let mean = if v == 0 {
                None
            } else {
                Some((self.variants[v - 1].mean, self.own_mean))
            };
            let relief = |n: f64, rise: f64| match mean {
                None => class_relief(self.kind, signed, n, amplitude, self.lift_m, rise),
                Some((mean, own)) => centred_relief(
                    self.kind,
                    (signed, mean),
                    (self.noise.shape().signed(), own),
                    n,
                    (amplitude, self.lift_m),
                    rise,
                ),
            };
            let (n, (dx, dz)) = noise.get_with_gradient_tuned(x, z, tune);
            let scale = relief(1.0, rise.share) - relief(0.0, rise.share);
            let mut gradient = (dx * scale, dz * scale);
            if self.kind == ReliefKind::Mountains {
                let full = relief(n, 1.0);
                gradient.0 += full * rise.gradient.0;
                gradient.1 += full * rise.gradient.1;
            }
            let share = if signed { f64::midpoint(n, 1.0) } else { n };
            out.here += relief(n, rise.share) * weight;
            out.gradient.0 += gradient.0 * weight;
            out.gradient.1 += gradient.1 * weight;
            out.share += share.clamp(0.0, 1.0) * weight;
            for (sum, n) in out
                .coarse
                .iter_mut()
                .zip(coarse.ring_tuned(x, z, radius, tune))
            {
                *sum += relief(n, rise.share) * weight;
            }
        }
        out
    }

    /// Metres of the relief's two coarsest octaves, the rise included: the
    /// large-scale shape of the ground, without the creases its fine octaves
    /// fold into it.
    fn coarse_relief(&self, x: f64, z: f64, rise: f64) -> f64 {
        self.relief_of(self.coarse_noise.get(x, z), rise)
    }

    /// Metres of relief for a noise value. `rise` is the share of the
    /// mountain kind's relief standing here ([`Rise`]), which only that
    /// kind reads.
    fn relief_of(&self, n: f64, rise: f64) -> f64 {
        match self.kind {
            // fbm is signed: plains and hills roll above and below the base.
            ReliefKind::Plains | ReliefKind::Hills => n * 0.5 * self.amplitude_m + self.lift_m,
            // Ridged and billow are unsigned: ridges and dunes only ever
            // rise from the base, so a valley floor is the base itself.
            ReliefKind::Ridges | ReliefKind::Dunes => n * self.amplitude_m + self.lift_m,
            // A range's relief is nothing until the mountains stand alone
            // and grows with the elevation after that, so its border with
            // the foothills is a slope and not a scarp: see `rise`.
            ReliefKind::Mountains => (n * self.amplitude_m + self.lift_m) * rise,
        }
    }

    /// The mountain kind's [`Rise`] at an elevation and its gradient: a
    /// smoothstep over the rise's band raised to `power`, by repeated
    /// multiplication (the float rule), so a power over one keeps the
    /// relief low where it enters and brings most of it in near the top.
    fn rise(&self, elevation: f64, gradient: (f64, f64), power: usize) -> Rise {
        let (low, high) = self.rise;
        let t = (elevation - low) / (high - low);
        if self.kind != ReliefKind::Mountains || t <= 0.0 {
            return Rise {
                share: 0.0,
                gradient: (0.0, 0.0),
            };
        }
        if t >= 1.0 {
            return Rise {
                share: 1.0,
                gradient: (0.0, 0.0),
            };
        }
        // smoothstep and its derivative, 6t(1 - t) per unit of t.
        let (gx, gz) = gradient;
        let s = t * t * (3.0 - 2.0 * t);
        let power = power.max(1);
        let mut below = 1.0;
        for _ in 1..power {
            below *= s;
        }
        #[allow(clippy::cast_precision_loss, reason = "a power of a few")]
        let d = power as f64 * below * 6.0 * t * (1.0 - t) / (high - low);
        Rise {
            share: below * s,
            gradient: (gx * d, gz * d),
        }
    }
}

/// The elevation from which a mountain biome stands alone: every other
/// biome is at least `gap` behind it.
///
/// A mountain window is open on temperature and moisture, so its score is
/// `e - min`; any other biome's score is at most `max - e` for the top of
/// its elevation window. The two part by `gap` once
/// `e >= (min + max + gap) / 2`. A biome whose window is open at the top
/// (only another mountain kind) can always compete and is left out.
fn alone_from(window: &ClimateWindow, others: &[ClimateWindow], gap: f64) -> f64 {
    let min = f64::from(window.elevation.min);
    others
        .iter()
        .filter(|other| other.elevation.max < 1.0)
        .map(|other| (min + f64::from(other.elevation.max) + gap) * 0.5)
        .fold(min, f64::max)
}

/// The procedural world source: a seed and the frozen registries in, a
/// [`Sample`] per point out.
#[derive(Debug, Clone)]
pub struct ProceduralWorld {
    params: WorldParams,
    warp_x: Fractal,
    warp_z: Fractal,
    continent: Fractal,
    ranges: Fractal,
    /// `(cos, sin)` of the direction ranges run in, chosen by the seed.
    range_axis: (f64, f64),
    /// Where the range field's lattice sits, in the stretched and turned
    /// frame, chosen by the seed: simplex noise is zero at a lattice
    /// vertex, every octave's lattice has one at the origin, and a signed
    /// field would otherwise run a range through the origin of every world.
    range_offset: (f64, f64),
    temperature: Fractal,
    moisture: Fractal,
    /// `(cos, sin)` of the prevailing wind, chosen by the seed.
    wind_axis: (f64, f64),
    wind_turn: Fractal,
    /// The continent field the rain march walks, at
    /// [`WorldParams::rain_coarse_octaves`] octaves and with no domain
    /// warp: a tap needs the land mask and the broad elevation, and a
    /// march is only affordable if a tap is a fraction of a real sample.
    coarse_continent: Fractal,
    /// The range field at [`WorldParams::rain_coarse_octaves`] octaves, for
    /// the same reason. A rain shadow comes from where the ranges are, so
    /// the march cannot skip them. At the shipped two octaves, with the
    /// range field itself two since ticket 0004's step 2b, this is the
    /// whole field and saves nothing; it keeps the march's cost fixed if
    /// the range field gains octaves.
    coarse_ranges: Fractal,
    cover_patches: Fractal,
    erosion: Erosion,
    biomes: Vec<BiomeSampler>,
    /// The settlements' sites, chosen from the natural ground when the
    /// world is made; every sample near one is shaped by it.
    sites: Vec<Site>,
    /// The layer a road's carriageway is drawn with, when the registries
    /// have one.
    road_layer: Option<u8>,
    /// Each site's ground layers, `[lanes, aprons, banks]`, where its
    /// registry entry names them and the terrain registry has them.
    ground_layers: Vec<Option<[u8; 3]>>,
    /// The layers a road of each class is laid in (ticket 0005, step 6):
    /// a service road's setts, a track's gravel, a trail's worn soil.
    way_layers: [Option<u8>; 3],
    /// The province field and each province's `rock.cover`, in id order;
    /// `None` when the registries hold no provinces, and then every sample
    /// is what it was before provinces existed.
    geology: Option<Geology>,
    /// The river network's lazy cache, one bake per land mass (ADR 0010).
    /// Shared by clones, and by every world of the same inputs in the
    /// process, which are the same world. The carve, the last height stage
    /// of every sample, reads it.
    hydrology: std::sync::Arc<crate::hydrology::Hydrology>,
    /// The layer the bed under water and a wet bank is laid in, by the
    /// bed's kind: `[gravel, sand]`, where the registries have them.
    bed_layers: [Option<u8>; 2],
    /// The site kinds (ADR 0012) in the order the bake places them; empty
    /// for a world that places no sites.
    site_kinds: std::sync::Arc<[crate::sites::SiteKind]>,
    /// What a map calls each authored settlement: no procedural site is
    /// given one of these names.
    settlement_names: Vec<String>,
}

/// How much of the ground a sample is of: the natural ground (the
/// settlements' shaping in it), or carved by the rivers and laid by the
/// sites' pads as every consumer sees it, or carved by one network alone
/// with no pad, which is what the bake of that network places its sites on
/// (the cache would bake the mass again to read it).
#[derive(Clone, Copy)]
pub(crate) enum Ground<'a> {
    Natural,
    Carved,
    /// Carved and laid as every consumer sees it, but for the roads' grade.
    Ungraded,
    By(&'a crate::hydrology::Network),
}

/// The province field with what the sampler reads of each province.
#[derive(Debug, Clone)]
struct Geology {
    field: ProvinceField,
    cover: Vec<f64>,
    /// Each province's beds, by id; `None` for one with none (ticket 0004,
    /// step 5).
    beds: Vec<Option<Beds>>,
    /// Whether any province has beds: if none does, no sample reads them.
    bedded: bool,
    /// Where a range's own ramp opens the beds' gate
    /// ([`WorldParams::bed_patch_wavelength_m`]).
    patches: Fractal,
    /// Each province's water rules by id, `(keep_pits_m, sink,
    /// along_grain)`: how deep a pit must be to stay a lake, what share of
    /// the rain goes under and how much the routing prefers the fold axis,
    /// which the river network reads (ADR 0010).
    water: Vec<(f64, f64, f64)>,
    /// Each province's valley section by id, with its `hang`: what the
    /// carve cuts there (ticket 0005, step 2).
    sections: Vec<crate::hydrology::Section>,
}

impl Geology {
    /// How much of a range's own ramp the beds' gate reads at a point,
    /// `0..=1` ([`WorldParams::bed_patch`]). Kept out of line: inlined, the
    /// noise's every shape came into the sampler's hot path with it and a
    /// sample in a bedded province cost 0.1 us more for a 16 ns noise (the
    /// review of step 5, measured pinned).
    #[inline(never)]
    fn patch(&self, x: f64, z: f64, params: &WorldParams) -> f64 {
        smoothstep(
            params.bed_patch.0,
            params.bed_patch.1,
            self.patches.get(x, z),
        )
    }

    fn new(
        seed: Seed,
        registries: &WorldDefs,
        params: &WorldParams,
        range_axis: (f64, f64),
    ) -> Option<Self> {
        if registries.provinces.is_empty() {
            return None;
        }
        // The dip is laid along the fold grain as far as the province's
        // shapes are: its largest stretch.
        let beds: Vec<Option<Beds>> = registries
            .provinces
            .iter()
            .map(|(_, name, def)| {
                let stretch = def
                    .relief
                    .iter()
                    .map(|rule| f64::from(rule.stretch))
                    .fold(1.0, f64::max);
                Beds::new(seed, name.as_ref(), &def.beds, range_axis, stretch)
            })
            .collect();
        let (rules, cover): (Vec<ProvinceRule>, Vec<f64>) = registries
            .provinces
            .iter()
            .map(|(id, _, def)| {
                let traits = registries
                    .province(id)
                    .expect("every frozen province is resolved");
                (
                    ProvinceRule {
                        frequency: f64::from(def.frequency),
                        deal_order: def.deal_order,
                        in_ranges: u8::try_from(traits.in_ranges.0).expect("at most 16 provinces"),
                    },
                    f64::from(traits.rock.cover),
                )
            })
            .unzip();
        let water = registries
            .provinces
            .iter()
            .map(|(_, _, def)| {
                (
                    f64::from(def.water.keep_pits_m),
                    f64::from(def.water.sink).clamp(0.0, 1.0),
                    f64::from(def.water.along_grain).clamp(0.0, 1.0),
                )
            })
            .collect();
        let sections = registries
            .provinces
            .iter()
            .map(|(id, _, def)| {
                let traits = registries
                    .province(id)
                    .expect("every frozen province is resolved");
                crate::hydrology::Section::of(
                    traits.section,
                    f64::from(def.water.hang),
                    f64::from(def.water.along_grain),
                )
            })
            .collect();
        Some(Self {
            field: ProvinceField::new(seed, &rules, params),
            water,
            sections,
            cover,
            bedded: beds.iter().any(Option::is_some),
            beds,
            patches: Fractal::new(
                seed.derive("beds/patches"),
                Shape::Fbm,
                params.bed_patch_wavelength_m,
                1,
                0.5,
                2.0,
            ),
        })
    }
}

/// The terrain material a road's carriageway is laid in, and a settlement's
/// developed ground: compacted gravel, where the wild ground would be dune
/// sand or grass. A sample anywhere a site touches carries it, and the
/// terrain blends towards it by how developed the ground is.
const ROAD_MATERIAL: &str = "strata:gravel";

/// The terrain material a lake's and a mouth's bed is laid in, the
/// channel's being [`ROAD_MATERIAL`]'s gravel.
const SAND_MATERIAL: &str = "strata:sand";

/// The materials a road of each class is laid in, service, track and trail
/// (ticket 0005, step 6): a service road paved in the city's setts, so it
/// reads as the road out of a city; a track in gravel; a trail in the soil
/// its feet wear bare. Public for the terrain, whose far paint draws each
/// class in its material.
pub const WAY_MATERIALS: [&str; 3] = ["strata:setts", "strata:gravel", "strata:soil"];

impl ProceduralWorld {
    /// A world from a seed and the frozen registries, with default
    /// [`WorldParams`].
    ///
    /// # Panics
    ///
    /// If `registries` holds no biomes. A frozen registry set cannot be
    /// empty (the freeze refuses it), so this only fires on a
    /// `WorldDefs::default()`.
    #[must_use]
    pub fn new(seed: Seed, registries: &WorldDefs) -> Self {
        Self::with_params(seed, registries, WorldParams::default())
    }

    /// A world with explicit parameters, for the map tool and the tests.
    ///
    /// # Panics
    ///
    /// If `registries` holds no biomes.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "every stage's noise, then the sites chosen over them"
    )]
    pub fn with_params(seed: Seed, registries: &WorldDefs, params: WorldParams) -> Self {
        assert!(
            !registries.biomes.is_empty(),
            "a world needs at least one biome; freeze the registries first"
        );
        let stage = |name: &str| seed.derive(name);
        // A seed-chosen direction, so every world's ranges have a grain of
        // their own rather than all running east to west. `libm` rather
        // than `f64::cos`: the platform libm is not correctly rounded and a
        // Windows client and a Linux server would disagree by an ulp, which
        // every ranges sample would then carry (see the crate docs). A
        // province's fold grain runs the same way (ticket 0004, step 4).
        let range_axis = {
            let angle = seed.rng("ranges/axis").next_f64() * core::f64::consts::PI;
            (libm::cos(angle), libm::sin(angle))
        };
        let biomes = biome_samplers(seed, registries, &params, range_axis);
        let continent = Fractal::new(
            stage("continents"),
            Shape::Fbm,
            params.continent_wavelength_m,
            params.continent_octaves,
            0.5,
            2.0,
        );
        // Signed, so the fold in `continent` raises a range along the
        // field's zero contours, which are long lines (ticket 0004, step
        // 2b). Until then it was ridged noise, whose values lie in `0..1`
        // and near zero only round the extremes of the noise beneath, so
        // the fold raised massifs round those.
        let ranges = Fractal::new(
            stage("ranges"),
            Shape::Fbm,
            params.range_wavelength_m,
            params.range_octaves,
            params.range_persistence,
            2.0,
        );
        let mut world = Self {
            params,
            warp_x: Fractal::new(
                stage("warp/x"),
                Shape::Fbm,
                params.warp_wavelength_m,
                3,
                0.5,
                2.0,
            ),
            warp_z: Fractal::new(
                stage("warp/z"),
                Shape::Fbm,
                params.warp_wavelength_m,
                3,
                0.5,
                2.0,
            ),
            coarse_continent: continent.truncated(params.rain_coarse_octaves),
            coarse_ranges: ranges.truncated(params.rain_coarse_octaves),
            continent,
            ranges,
            range_axis,
            range_offset: {
                // Anywhere in a square of a hundred kilometres: far enough
                // from every lattice vertex that the origin is nothing
                // special, near enough that no coordinate loses precision.
                let mut rng = seed.rng("ranges/offset");
                (rng.next_f64() * 100_000.0, rng.next_f64() * 100_000.0)
            },
            temperature: Fractal::new(
                stage("climate/temperature"),
                Shape::Fbm,
                params.temperature_wavelength_m,
                3,
                0.5,
                2.0,
            ),
            moisture: Fractal::new(
                stage("climate/moisture"),
                Shape::Fbm,
                params.moisture_wavelength_m,
                3,
                0.5,
                2.0,
            ),
            wind_axis: {
                // A seed-chosen direction over the whole circle, so which
                // coast of a continent is the wet one is the seed's
                // business. `libm` rather than `f64::cos` for the reason
                // the range axis gives.
                let angle = seed.rng("climate/wind").next_f64() * core::f64::consts::TAU;
                (libm::cos(angle), libm::sin(angle))
            },
            wind_turn: Fractal::new(
                stage("climate/wind-turn"),
                Shape::Fbm,
                params.wind_turn_wavelength_m,
                1,
                0.5,
                2.0,
            ),
            cover_patches: Fractal::new(
                stage("cover/patches"),
                Shape::Fbm,
                params.cover_wavelength_m,
                2,
                0.5,
                2.0,
            ),
            erosion: Erosion::new(
                (0..params.erosion_octaves)
                    .map(|octave| stage(&format!("erosion/{octave}")))
                    .collect(),
                params.erosion_wavelength_m,
                params.erosion_slope_reference,
                params.erosion_detail,
            ),
            biomes,
            sites: Vec::new(),
            ground_layers: Vec::new(),
            geology: Geology::new(seed, registries, &params, range_axis),
            hydrology: crate::hydrology::Hydrology::shared(
                seed,
                crate::version::network_key(seed, &params, registries),
            ),
            road_layer: strata_data::Namespaced::parse(ROAD_MATERIAL)
                .ok()
                .and_then(|name| registries.material_id(&name))
                .map(strata_data::TerrainMaterialId::layer),
            way_layers: WAY_MATERIALS.map(|material| {
                strata_data::Namespaced::parse(material)
                    .ok()
                    .and_then(|name| registries.material_id(&name))
                    .map(strata_data::TerrainMaterialId::layer)
            }),
            bed_layers: [ROAD_MATERIAL, SAND_MATERIAL].map(|material| {
                strata_data::Namespaced::parse(material)
                    .ok()
                    .and_then(|name| registries.material_id(&name))
                    .map(strata_data::TerrainMaterialId::layer)
            }),
            site_kinds: {
                let mut kinds: Vec<crate::sites::SiteKind> = registries
                    .sites
                    .iter()
                    .map(|(id, name, def)| crate::sites::SiteKind {
                        id,
                        name: name.clone(),
                        def: def.clone(),
                        traits: registries
                            .site_kind(id)
                            .expect("every frozen kind is resolved")
                            .clone(),
                    })
                    .collect();
                kinds.sort_by_key(|kind| kind.traits.order);
                kinds.into()
            },
            settlement_names: registries
                .settlements
                .iter()
                .map(|def| def.name.clone())
                .collect(),
        };
        world.ground_layers = registries
            .settlements
            .iter()
            .map(|def| {
                let ground = def.ground.as_ref()?;
                let layer = |name: &strata_data::Namespaced| {
                    registries
                        .material_id(name)
                        .map(strata_data::TerrainMaterialId::layer)
                };
                Some([
                    layer(&ground.lanes)?,
                    layer(&ground.aprons)?,
                    layer(&ground.banks)?,
                ])
            })
            .collect();
        let sites = registries
            .settlements
            .iter()
            .map(|def| {
                // The road is traced over the natural ground: no site is
                // placed yet, and no river carved, since the network is
                // routed round the sites and so baked after them.
                Site::place(def, &|x, z| {
                    f64::from(world.sample_with(x, z, Ground::Natural).height)
                })
            })
            .collect();
        world.sites = sites;
        world
    }

    /// The settlements' sites, in the registries' order.
    #[must_use]
    pub fn sites(&self) -> &[Site] {
        &self.sites
    }

    /// What a map calls each authored settlement, in the registries' order.
    #[must_use]
    pub fn settlement_names(&self) -> &[String] {
        &self.settlement_names
    }

    /// The site kinds the bake places (ADR 0012), in placement order.
    #[must_use]
    pub fn site_kinds(&self) -> &[crate::sites::SiteKind] {
        &self.site_kinds
    }

    /// Every procedural site within `radius_m` of `(x, z)`, nearest first,
    /// over every land mass that reaches there (baked first, on this thread
    /// alone, if no one has: [`prepare_water`](Self::prepare_water) first).
    #[must_use]
    pub fn sites_near(&self, x: f64, z: f64, radius_m: f64) -> Vec<crate::sites::ProceduralSite> {
        let networks = self.hydrology.bake_area_on(
            self,
            (x - radius_m, z - radius_m),
            (x + radius_m, z + radius_m),
            1,
        );
        let mut found: Vec<(f64, crate::sites::ProceduralSite)> = networks
            .iter()
            .flat_map(|network| network.sites().iter())
            .filter_map(|site| {
                let (dx, dz) = (site.centre[0] - x, site.centre[1] - z);
                let d = (dx * dx + dz * dz).sqrt();
                (d <= radius_m).then(|| (d, site.clone()))
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        found.into_iter().map(|(_, site)| site).collect()
    }

    /// The nearest road to `(x, z)` within `reach_m` over every land mass
    /// that reaches there (ticket 0005, step 5): its class, how far its
    /// centreline is and which way it runs there. Reads each network's
    /// bucket grid and no sample; a mass no one has baked is baked first,
    /// on this thread alone ([`prepare_water`](Self::prepare_water) first).
    /// Ties go to the lower mass key and road.
    #[must_use]
    pub fn nearest_road(&self, x: f64, z: f64, reach_m: f64) -> Option<crate::roads::RoadHere> {
        let networks = self.hydrology.bake_area_on(
            self,
            (x - reach_m, z - reach_m),
            (x + reach_m, z + reach_m),
            1,
        );
        networks
            .iter()
            .filter_map(|network| network.nearest_road(x, z, reach_m))
            .min_by(|a, b| {
                a.distance_m
                    .total_cmp(&b.distance_m)
                    .then(a.mass.cmp(&b.mass))
                    .then(a.road.cmp(&b.road))
            })
    }

    /// Every road's crossing of a river within `radius_m` of `(x, z)` over
    /// every land mass that reaches there (ticket 0005, step 7), in id
    /// order, each with its road's class and its centreline at the
    /// crossing's ends. A mass no one has baked is baked first, on this
    /// thread alone, as [`sites_near`](Self::sites_near) does.
    #[must_use]
    pub fn crossings_near(
        &self,
        x: f64,
        z: f64,
        radius_m: f64,
    ) -> Vec<crate::crossings::ProceduralCrossing> {
        let networks = self.hydrology.bake_area_on(
            self,
            (x - radius_m, z - radius_m),
            (x + radius_m, z + radius_m),
            1,
        );
        let mut found: Vec<crate::crossings::ProceduralCrossing> = networks
            .iter()
            .flat_map(|network| {
                crate::crossings::of_network(network, |[cx, cz]| {
                    let (dx, dz) = (cx - x, cz - z);
                    dx * dx + dz * dz <= radius_m * radius_m
                })
            })
            .collect();
        found.sort_by_key(|c| c.id);
        found
    }

    /// The procedural site whose ground `(x, z)` is on (its pad or the band
    /// round it), if any.
    #[must_use]
    pub fn site_at(&self, x: f64, z: f64) -> Option<crate::sites::ProceduralSite> {
        // No band is wider than the registry allows a pad and its blend.
        self.sites_near(x, z, 500.0).into_iter().find(|site| {
            let (dx, dz) = (site.centre[0] - x, site.centre[1] - z);
            dx * dx + dz * dz < site.outer_m() * site.outer_m()
        })
    }

    /// The parameters this world was built with.
    #[must_use]
    pub const fn params(&self) -> &WorldParams {
        &self.params
    }

    /// The province field, or `None` in a world without geology.
    #[must_use]
    pub fn province_field(&self) -> Option<&ProvinceField> {
        self.geology.as_ref().map(|geology| &geology.field)
    }

    /// Every province weighing at a point, heaviest first: the whole blend
    /// of which [`Sample::provinces`] is the top two. `None` in a world
    /// without geology. Costs a continent stage and the field.
    #[must_use]
    pub fn provinces(&self, x: f64, z: f64) -> Option<ProvinceMix> {
        let geology = self.geology.as_ref()?;
        let continent = self.continent(x, z);
        Some(
            geology
                .field
                .sample(continent.warped.0, continent.warped.1, continent.range),
        )
    }

    /// The biome at index `i` of the classifier's list.
    #[must_use]
    pub fn biome_id(&self, index: usize) -> BiomeId {
        self.biomes[index].id
    }

    /// Stage 1: land, sea and base elevation.
    #[must_use]
    pub fn continent(&self, x: f64, z: f64) -> Continent {
        let params = &self.params;
        let wx = x + self.warp_x.get(x, z) * params.warp_amplitude_m;
        let wz = z + self.warp_z.get(x, z) * params.warp_amplitude_m;
        let field = self.continent.get(wx, wz) + params.land_bias;
        // The shore is a step: the field alone would give a beach several
        // kilometres wide, because a 45 km wavelength crosses zero slowly.
        let shore = (field / params.shore_width).clamp(-1.0, 1.0) * params.shore_step;
        // A range runs along each zero contour of the signed range field, a
        // long line (ticket 0004, step 2b; until then the field was ridged
        // noise and the ranges massifs). Its profile across the line is read
        // from the distance to that contour (step 4; until then from the
        // field's value, folded into `1 - n^2`, whose rate of change along a
        // range made the same profile a wall in one place and a plateau in
        // another), rounded at the crest so there is no roof line. The
        // stretch along a seed-chosen axis gives the ranges a grain, and
        // `range_lift` their profile.
        let (cos, sin) = self.range_axis;
        let along = (cos * wx + sin * wz) / params.range_anisotropy + self.range_offset.0;
        let across = -sin * wx + cos * wz + self.range_offset.1;
        let frame = self.ridge_frame(along, across);
        let (d, _) = crest_distance(&frame);
        let fold = across_profile(d, params.range_width_m, params.range_round_m).0;
        let range = range_lift(params, fold, params.range_foot).0 * range_coast(params, field).0;
        let height = range_height(params, &frame, field);
        let elevation = (0.5 + shore + field * params.land_rise + range * params.range_strength)
            .clamp(0.0, 1.0);
        let above = (elevation - 0.5) * 2.0;
        let base_m = if above < 0.0 {
            above * params.ocean_depth_m
        } else {
            above * params.land_base_m
        };
        Continent {
            land: field,
            range,
            elevation,
            base_m,
            range_m: height.metres * range_rooting(params, elevation).0,
            flank: height.flank,
            warped: (wx, wz),
        }
    }

    /// [`continent`](Self::continent) and the gradient of its elevation per
    /// metre, `(d/dx, d/dz)`: the mountains' relief grows with the
    /// elevation, and its slope has to know how fast.
    ///
    /// Analytic, from each noise's own derivative carried through the
    /// shore, the fold, the smoothsteps and the warp (which bends distances
    /// by up to about a half, so its derivatives are needed too). The values
    /// are the same operations in the same order as `continent`, so the
    /// same bits (a test holds it); a second body rather than one because
    /// the derivatives cost the sampler about 0.1 us, and the other callers
    /// need none.
    #[allow(
        clippy::similar_names,
        reason = "the chain rule's x and z partials, named in pairs"
    )]
    fn continent_with_gradient(&self, x: f64, z: f64) -> (Continent, ContinentSlopes) {
        let params = &self.params;
        let amp = params.warp_amplitude_m;
        let (warp_x, (dwx_dx, dwx_dz)) = self.warp_x.get_with_gradient(x, z);
        let (warp_z, (dwz_dx, dwz_dz)) = self.warp_z.get_with_gradient(x, z);
        let wx = x + warp_x * amp;
        let wz = z + warp_z * amp;
        let (noise, (dn_dwx, dn_dwz)) = self.continent.get_with_gradient(wx, wz);
        let field = noise + params.land_bias;
        let shore_t = field / params.shore_width;
        let shore = shore_t.clamp(-1.0, 1.0) * params.shore_step;
        let d_shore = if shore_t.abs() < 1.0 {
            params.shore_step / params.shore_width
        } else {
            0.0
        };
        let (coast, d_coast) = range_coast(params, field);
        let (cos, sin) = self.range_axis;
        let along = (cos * wx + sin * wz) / params.range_anisotropy + self.range_offset.0;
        let across = -sin * wx + cos * wz + self.range_offset.1;
        let frame = self.ridge_frame_with_gradient(along, across);
        // How far in from the edge of the range's profile, and its gradient
        // in the warped frame.
        let (d, dd) = crest_distance(&frame);
        let (fold, dt) = across_profile(d, params.range_width_m, params.range_round_m);
        let fold_gradient = (dt * dd.0, dt * dd.1);
        let (lift, d_lift) = range_lift(params, fold, params.range_foot);
        let range = lift * coast;
        let elevation_raw = 0.5 + shore + field * params.land_rise + range * params.range_strength;
        let elevation = elevation_raw.clamp(0.0, 1.0);
        let above = (elevation - 0.5) * 2.0;
        let base_m = if above < 0.0 {
            above * params.ocean_depth_m
        } else {
            above * params.land_base_m
        };

        // The same stages differentiated by the warped point, then carried
        // back through the warp to the point itself.
        let unwarp = |(gx, gz): (f64, f64)| {
            (
                gx * (1.0 + amp * dwx_dx) + gz * amp * dwz_dx,
                gx * amp * dwx_dz + gz * (1.0 + amp * dwz_dz),
            )
        };
        // The range's lift by the field (the coastal fade) and across it.
        let dl_field = lift * d_coast;
        let dl_across = (
            d_lift * coast * fold_gradient.0,
            d_lift * coast * fold_gradient.1,
        );
        let elevation_gradient = if elevation_raw <= 0.0 || elevation_raw >= 1.0 {
            (0.0, 0.0)
        } else {
            // The continent field's own slope stays in: on a range's crest,
            // where the range is full, it is all the elevation's slope, and
            // a mountain relief thousands of metres tall multiplies it.
            let d_field = d_shore + params.land_rise + dl_field * params.range_strength;
            let k = params.range_strength;
            unwarp((
                d_field * dn_dwx + k * dl_across.0,
                d_field * dn_dwz + k * dl_across.1,
            ))
        };
        // The range's own metres are not clamped with the elevation, so
        // their slope runs on over a crest the elevation is flat on.
        let height = range_height(params, &frame, field);
        // Rooted in the foothills: none of it where the ground is still the
        // lowland's, so the plains keep their height.
        let (root, d_root) = range_rooting(params, elevation);
        let bare = unwarp((
            height.gradient.0 + height.d_field * dn_dwx,
            height.gradient.1 + height.d_field * dn_dwz,
        ));
        let range_gradient = (
            bare.0 * root + height.metres * d_root * elevation_gradient.0,
            bare.1 * root + height.metres * d_root * elevation_gradient.1,
        );
        let flank_gradient = unwarp((
            height.flank_gradient.0 + height.flank_d_field * dn_dwx,
            height.flank_gradient.1 + height.flank_d_field * dn_dwz,
        ));
        (
            Continent {
                land: field,
                range,
                elevation,
                base_m,
                range_m: height.metres * root,
                flank: height.flank,
                warped: (wx, wz),
            },
            ContinentSlopes {
                elevation: elevation_gradient,
                range_m: range_gradient,
                flank: flank_gradient,
                base_m: {
                    let metres = if above < 0.0 {
                        params.ocean_depth_m
                    } else {
                        params.land_base_m
                    };
                    (
                        2.0 * metres * elevation_gradient.0,
                        2.0 * metres * elevation_gradient.1,
                    )
                },
            },
        )
    }

    /// The signed range field at a point of its own stretched and turned
    /// frame, and the smoothed size of its gradient in the warped frame the
    /// continent stage works in: what a range's distance to its crest line
    /// is read from ([`crest_distance`]). No gradients: the continent's
    /// values alone. The same bits as
    /// [`ridge_frame_with_gradient`](Self::ridge_frame_with_gradient)'s.
    fn ridge_frame(&self, along: f64, across: f64) -> RangeFrame {
        let (cos, sin) = self.range_axis;
        let a = self.params.range_anisotropy;
        let step_along = RANGE_SCALE_STEP_M / a;
        let ridge = self.ranges.get(along, across);
        let ahead = self.ranges.get(along + step_along, across);
        let aside = self.ranges.get(along, across + RANGE_SCALE_STEP_M);
        let (da, dc) = (
            (ahead - ridge) / step_along,
            (aside - ridge) / RANGE_SCALE_STEP_M,
        );
        let (j00, j01, j10, j11) = (cos / a, sin / a, -sin, cos);
        let spread = (da * j00 + dc * j10, da * j01 + dc * j11);
        RangeFrame {
            ridge,
            gradient: (0.0, 0.0),
            scale: (spread.0 * spread.0 + spread.1 * spread.1).sqrt(),
            scale_gradient: (0.0, 0.0),
        }
    }

    /// [`ridge_frame`](Self::ridge_frame) with the field's gradient and the
    /// gradient of the size of its gradient, `(d/dwx, d/dwz)`.
    ///
    /// The size is read from differences of the field over
    /// [`RANGE_SCALE_STEP_M`], not from its gradient: the kernel's falloff
    /// `2 t^2 + t^4` has a second derivative that jumps where each corner's
    /// reach ends, so the gradient's size has a kink there, and a distance
    /// divided by it put a crease into every range's flank and, through the
    /// slope the gullies follow, steps of up to 15 m in a centimetre (the
    /// review of ticket 0004's step 4). A difference of a field whose
    /// gradient is continuous has a continuous gradient, so the distance is
    /// smooth, and its slope needs the field's gradient at the two taps and
    /// nothing of second order.
    #[allow(
        clippy::similar_names,
        reason = "the chain rule's partials, named by their two axes"
    )]
    fn ridge_frame_with_gradient(&self, along: f64, across: f64) -> RangeFrame {
        let (cos, sin) = self.range_axis;
        let a = self.params.range_anisotropy;
        let step_along = RANGE_SCALE_STEP_M / a;
        let [(ridge, (ra, rc)), (ahead, (ra1, rc1)), (aside, (ra2, rc2))] =
            self.ranges.fbm_with_gradient_many([
                (along, across),
                (along + step_along, across),
                (along, across + RANGE_SCALE_STEP_M),
            ]);
        let (da, dc) = (
            (ahead - ridge) / step_along,
            (aside - ridge) / RANGE_SCALE_STEP_M,
        );
        // d(along, across) / d(wx, wz): rows along, across.
        let (j00, j01, j10, j11) = (cos / a, sin / a, -sin, cos);
        let spread = (da * j00 + dc * j10, da * j01 + dc * j11);
        let scale = (spread.0 * spread.0 + spread.1 * spread.1).sqrt();
        let gradient = (ra * j00 + rc * j10, ra * j01 + rc * j11);
        // How the differences change along and across, rows (da, dc).
        let (m00, m01) = ((ra1 - ra) / step_along, (rc1 - rc) / step_along);
        let (m10, m11) = (
            (ra2 - ra) / RANGE_SCALE_STEP_M,
            (rc2 - rc) / RANGE_SCALE_STEP_M,
        );
        // grad |J^T D| = J^T M^T J (J^T D) / |J^T D|, in three products.
        let scale_gradient = if scale > 0.0 {
            let u = (
                (j00 * spread.0 + j01 * spread.1) / scale,
                (j10 * spread.0 + j11 * spread.1) / scale,
            );
            let v = (u.0 * m00 + u.1 * m10, u.0 * m01 + u.1 * m11);
            (v.0 * j00 + v.1 * j10, v.0 * j01 + v.1 * j11)
        } else {
            (0.0, 0.0)
        };
        RangeFrame {
            ridge,
            gradient,
            scale,
            scale_gradient,
        }
    }

    /// The prevailing wind at a point, as a unit vector in `(x, z)`.
    ///
    /// The seed's direction, swung by a noise of a few hundred kilometres
    /// so that one continent's windward coast is not every continent's. The
    /// swing is a perturbation renormalised by `sqrt` rather than a
    /// rotation by an angle, because a rotation would need a `sin` and a
    /// `cos` per sample and the platform's are not correctly rounded.
    #[must_use]
    pub fn wind_at(&self, x: f64, z: f64) -> (f64, f64) {
        let (cos, sin) = self.wind_axis;
        let t = self.wind_turn.get(x, z) * self.params.wind_turn;
        let (ux, uz) = (cos - t * sin, sin + t * cos);
        let inv = 1.0 / (ux * ux + uz * uz).sqrt();
        (ux * inv, uz * inv)
    }

    /// The coarse continent the rain march walks: the land mask and the
    /// land height in metres above sea level, from cheap noise and without
    /// the domain warp.
    ///
    /// Same shape as [`continent`](Self::continent) at a fraction of its
    /// cost, which is what makes a fixed number of taps per sample
    /// affordable. The warp displaces a point by up to a few kilometres
    /// against a tap spacing of three. That moved a coast but not a range
    /// while the ranges were massifs tens of kilometres across; since ticket
    /// 0004's step 2b they are lines a few kilometres wide, and dropping the
    /// warp can move one in the march by about its own width (not measured;
    /// a possible part of that step's weaker rain shadow).
    /// Depth below sea level is not reported: air over water is air over
    /// water, and a shelf rising to a beach is not orographic lift.
    fn coarse(&self, x: f64, z: f64) -> (f64, f64) {
        let p = &self.params;
        let field = self.coarse_continent.get(x, z) + p.land_bias;
        let land = smoothstep(0.02, 0.15, field);
        // Most taps of most marches are over open sea, and there the range
        // field is multiplied by a hard zero anyway, so the second noise
        // stack is skipped rather than computed and thrown away. Skipping
        // is bit-identical to evaluating it, which matters: the rest of the
        // tap must stay continuous across the coast or the step lands in
        // moisture and a biome border turns into an edge.
        let range = if land == 0.0 {
            0.0
        } else {
            let (cos, sin) = self.range_axis;
            let along = (cos * x + sin * z) / p.range_anisotropy + self.range_offset.0;
            let across = -sin * x + cos * z + self.range_offset.1;
            // The fold of the field's value, not the distance to its zero
            // contour the real continent reads (ticket 0004, step 4): the
            // distance needs the field's gradient at every tap, which cost
            // the march 0.2 us a sample; the crest lines are the same, and
            // with the bell step 2b tuned the rain on, the rain is where it
            // was.
            let folded =
                (self.coarse_ranges.get(along, across) * p.range_sharpness).clamp(-1.0, 1.0);
            range_lift(p, 1.0 - folded * folded, p.rain_range_foot).0 * range_coast(p, field).0
        };
        let shore = (field / p.shore_width).clamp(-1.0, 1.0) * p.shore_step;
        let elevation =
            (0.5 + shore + field * p.land_rise + range * p.range_strength).clamp(0.0, 1.0);
        let above = (elevation - 0.5) * 2.0;
        (land, above.max(0.0) * p.land_base_m)
    }

    /// Rainfall at a point, arid (`0`) to soaked (`1`).
    ///
    /// A parcel of air is walked from the farthest upwind tap to the
    /// sample, carrying moisture: full over sea, losing
    /// [`WorldParams::rain_land_decay`] of what is left per tap over land,
    /// and losing [`WorldParams::rain_orographic_rate`] of it where the
    /// ground climbs under it. That is the whole of the model, and it is
    /// what puts a wet side and a dry side on a range instead of leaving
    /// moisture to a noise (ADR 0006). Marching per sample rather than
    /// baking a grid keeps the sampler a pure function of the seed.
    #[must_use]
    pub fn rainfall(&self, x: f64, z: f64) -> f64 {
        let p = &self.params;
        // Taps are a handful; the cast is exact.
        #[allow(clippy::cast_precision_loss)]
        let reach = p.rain_tap_spacing_m * p.rain_taps as f64;
        let (wx, wz) = self.wind_at(x, z);
        let (mut px, mut pz) = (x - wx * reach, z - wz * reach);
        let (_, mut base) = self.coarse(px, pz);
        let mut carry = 1.0;
        let mut rained = 0.0;
        for tap in 1..=p.rain_taps {
            px += wx * p.rain_tap_spacing_m;
            pz += wz * p.rain_tap_spacing_m;
            // The mask is smooth, not a sea test: a hard one would draw a
            // step into moisture a few kilometres off every coast, and the
            // biome borders would step with it.
            let (land, next_base) = self.coarse(px, pz);
            let over_land = carry * (1.0 - p.rain_land_decay);
            carry = over_land * land + (1.0 - land);
            let rise = ((next_base - base) / p.rain_rise_scale_m).clamp(0.0, 1.0);
            rained = carry * rise * p.rain_orographic_rate * land;
            // The rain-out at the sample itself is added to the result
            // instead, so a windward slope is wetter than the flat coast
            // the same distance inland.
            if tap < p.rain_taps {
                carry -= rained;
            }
            base = next_base;
        }
        (carry + rained).clamp(0.0, 1.0)
    }

    /// Stage 2: temperature and moisture.
    #[must_use]
    pub fn climate(&self, x: f64, z: f64, continent: &Continent) -> Climate {
        self.climate_with_rain(x, z, continent, self.rainfall(x, z))
    }

    /// [`climate`](Self::climate) given the point's
    /// [`rainfall`](Self::rainfall), so a caller that needs the rain too
    /// marches it once.
    fn climate_with_rain(&self, x: f64, z: f64, continent: &Continent, rainfall: f64) -> Climate {
        let p = &self.params;
        let latitude = 1.0 - ((z - p.equator_z).abs() / p.pole_distance_m).min(1.0);
        let lapse = continent.base_m.max(0.0) * p.lapse_per_m;
        let noise = self.temperature.get(x, z) * p.temperature_noise;
        let temperature = (latitude - lapse + noise).clamp(0.0, 1.0);

        // fbm rarely leaves the middle half of its range; stretching it is
        // what lets a desert and a rainforest both exist.
        let noise = (0.5 + self.moisture.get(x, z) * p.moisture_stretch).clamp(0.0, 1.0);
        // Mostly the march, with a share of the old noise left in: rainfall
        // over a flat interior is the same everywhere, and one uniform
        // biome over a hundred kilometres is worse than a slightly
        // arbitrary patchwork.
        let moisture =
            (rainfall * (1.0 - p.rain_noise_share) + noise * p.rain_noise_share).clamp(0.0, 1.0);

        Climate {
            temperature,
            moisture,
            elevation: continent.elevation,
        }
    }

    /// Stage 3: the two biomes whose windows fit the climate best.
    ///
    /// A biome's score is its worst margin: how far inside its window the
    /// point is on the tightest axis, negative by the distance outside.
    /// Every biome within [`WorldParams::blend_gap`] of the best gets a
    /// weight that falls smoothly to zero at the gap, so a biome entering
    /// or leaving the blend never moves the ground, and two biomes swapping
    /// first place do so at equal weight. Two slots with a weight for the
    /// winner alone would step at every point where three windows meet.
    #[must_use]
    pub fn classify(&self, climate: &Climate) -> Classification {
        let mut best = (0, 0.0);
        let mut second = (0, 0.0);
        self.blend(climate, |index, weight| {
            if weight > best.1 {
                second = best;
                best = (index, weight);
            } else if weight > second.1 {
                second = (index, weight);
            }
        });
        let (second, weight) = pair(best, second);
        Classification {
            best: best.0,
            second,
            weight,
        }
    }

    /// Calls `visit(index, weight)` for every biome with a non-zero blend
    /// weight. The weights are unnormalised: the best biome's is one.
    fn blend(&self, climate: &Climate, mut visit: impl FnMut(usize, f64)) {
        let gap = self.params.blend_gap;
        let best = self
            .biomes
            .iter()
            .map(|biome| window_score(&biome.window, climate))
            .fold(f64::NEG_INFINITY, f64::max);
        for (index, biome) in self.biomes.iter().enumerate() {
            let behind = best - window_score(&biome.window, climate);
            if behind < gap {
                visit(index, 1.0 - smoothstep(0.0, gap, behind));
            }
        }
    }

    /// The ground the river network is routed over at one lattice node
    /// (ADR 0010): the continent's base with the range's own metres, and
    /// each biome's relief octaves of three lattice spacings or more (the
    /// first at least) by the biome blend, the mountains'
    /// rise, a range's crest growth and the foothills' flank damping as the
    /// sample applies them; with the climate's moisture for the rain and
    /// the dominant province's water rules.
    ///
    /// What is left out, and why: the relief's finer octaves, which a
    /// 512 m lattice cannot resolve and would only alias into pits (the
    /// mountains keep two, 6 and 3 km, so a range's flank has spurs and
    /// valleys to gather water in; with one it was a smooth ramp whose
    /// water ran down it in parallel threads and made no river); the
    /// provinces' shapes, the beds and the erosion filter, which make the
    /// metre-scale form of a slope and cost most of a sample; and the
    /// settlement, whose ground the network routes round instead. Below
    /// [`DEEP_SEA_M`](crate::hydrology::DEEP_SEA_M) of base the node is sea
    /// by definition and nothing else is computed, so the open sea costs a
    /// continent stage.
    pub(crate) fn coarse_ground(&self, x: f64, z: f64) -> crate::hydrology::CoarseGround {
        let continent = self.continent(x, z);
        let base = continent.base_m + continent.range_m;
        if base < -crate::hydrology::DEEP_SEA_M {
            return crate::hydrology::CoarseGround {
                height: base,
                range_m: continent.range_m,
                rain: 0.0,
                province: None,
            };
        }
        let rain = self.rainfall(x, z);
        let climate = self.climate_with_rain(x, z, &continent, rain);
        let (mut relief, mut total) = (0.0, 0.0);
        self.blend(&climate, |index, weight| {
            let biome = &self.biomes[index];
            let rise = biome
                .rise(
                    continent.elevation,
                    (0.0, 0.0),
                    self.params.mountain_rise_power,
                )
                .share;
            let mut here = biome.relief_of(biome.lowest.get(x, z), rise);
            if biome.kind == ReliefKind::Mountains && self.params.crest_relief_gain > 0.0 {
                here *= crest_growth(&self.params, continent.flank).0;
            }
            if biome.kind != ReliefKind::Mountains && self.params.flank_damping > 0.0 {
                here *= 1.0 - self.params.flank_damping * continent.flank;
            }
            relief += here * weight;
            total += weight;
        });
        let province = self.geology.as_ref().map(|geology| {
            let id = geology
                .field
                .sample(continent.warped.0, continent.warped.1, continent.range)
                .dominant();
            geology.water[id.0 as usize]
        });
        crate::hydrology::CoarseGround {
            height: base + relief / total,
            range_m: continent.range_m,
            rain: climate.moisture,
            province,
        }
    }

    /// The ranges' direction, a unit `(x, z)`: the fold axis a province's
    /// grain runs along.
    pub(crate) const fn range_axis(&self) -> (f64, f64) {
        self.range_axis
    }

    /// The river network's cache: the networks baked so far and the query
    /// that bakes the rest on demand (ADR 0010).
    #[must_use]
    pub fn hydrology(&self) -> &crate::hydrology::Hydrology {
        &self.hydrology
    }

    /// This world with an empty river cache of its own in place of the one
    /// every world of the same inputs in this process shares: it bakes its
    /// networks and places its sites again, as a server and a client in
    /// two processes do, so a test can hold the two bakes to each other.
    #[must_use]
    pub fn with_own_water(mut self) -> Self {
        self.hydrology = std::sync::Arc::new(self.hydrology.empty_like());
        self
    }

    /// Whether `other` reads its rivers from the same cache as this world.
    #[must_use]
    pub fn shares_water_with(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.hydrology, &other.hydrology)
    }

    /// The water at `(x, z)`: the nearest river within reach or the lake
    /// over the point, `None` where it is dry, from the network alone (a
    /// sample's [`Sample::water`] is the same water with the carved ground
    /// under it). Bakes the land masses that reach the point first if no
    /// one has asked about them yet, on this thread alone, which takes up
    /// to a few seconds: [`prepare_water`](Self::prepare_water) first.
    #[must_use]
    pub fn water_at(&self, x: f64, z: f64) -> Option<crate::hydrology::Water> {
        self.hydrology.water_at(self, x, z)
    }

    /// Bakes every land mass whose rivers can reach the square of
    /// `radius_m` round `(x, z)`, on up to
    /// [`MAX_THREADS`](crate::hydrology::MAX_THREADS) threads (as
    /// [`Hydrology::set_threads`](crate::hydrology::Hydrology::set_threads)
    /// has them), and returns how many masses that is. A sample that meets a
    /// mass no one has baked bakes it itself, on its own thread alone and
    /// correctly, but it may be inside a task pool or on a frame: a process
    /// calls this for the places it will sample before it samples them.
    pub fn prepare_water(&self, x: f64, z: f64, radius_m: f64) -> usize {
        self.hydrology
            .bake_area(
                self,
                (x - radius_m, z - radius_m),
                (x + radius_m, z + radius_m),
            )
            .len()
    }

    /// [`prepare_water`](Self::prepare_water) on `threads` (at most
    /// [`MAX_THREADS`](crate::hydrology::MAX_THREADS)): what a process that
    /// is already running asks for ahead of a moving observer, on a few.
    pub fn prepare_water_on(&self, x: f64, z: f64, radius_m: f64, threads: usize) -> usize {
        self.hydrology
            .bake_area_on(
                self,
                (x - radius_m, z - radius_m),
                (x + radius_m, z + radius_m),
                threads,
            )
            .len()
    }

    /// The same world reading its rivers from `hydrology`: the version's
    /// canary samples through a cache that bakes nothing.
    pub(crate) fn with_hydrology(
        &self,
        hydrology: std::sync::Arc<crate::hydrology::Hydrology>,
    ) -> Self {
        Self {
            hydrology,
            ..self.clone()
        }
    }

    /// The sample at `(x, z)` as it was before the rivers' carve (ticket
    /// 0005, step 2), the settlements' shaping in it: what a test or a
    /// measure compares when it isolates a stage above the carve between
    /// two worlds whose river networks differ (a world without geology
    /// routes its rivers otherwise), and the ground the settlements' roads
    /// were traced over. Reads no network and bakes nothing.
    #[must_use]
    pub fn sample_before_carve(&self, x: f64, z: f64) -> Sample {
        self.sample_with(x, z, Ground::Natural)
    }

    /// Every stage but the roads' grade (ticket 0005, step 6): the ground
    /// as it was before the roads moved it, the rivers carved and the
    /// sites' pads laid. What a measure compares to isolate the grade's
    /// cost and what it moved.
    #[must_use]
    pub fn sample_without_roads(&self, x: f64, z: f64) -> Sample {
        self.sample_with(x, z, Ground::Ungraded)
    }

    /// The sample carved by `network` alone, no pad laid: the ground the
    /// bake of that network places its sites on.
    pub(crate) fn sample_by(&self, x: f64, z: f64, network: &crate::hydrology::Network) -> Sample {
        self.sample_with(x, z, Ground::By(network))
    }

    /// The natural ground's height at `(x, z)`, before the carve and any
    /// settlement: where a mouth's shore is.
    pub(crate) fn natural_height(&self, x: f64, z: f64) -> f64 {
        self.natural(x, z, |_, _, _, _| {}).height
    }

    /// The natural ground's height at `(x, z)` less its coarse part (the
    /// continent and the relief's first octaves, [`Natural::coarse`]): the
    /// fine ground a lake's shore takes its shape from.
    pub(crate) fn natural_detail(&self, x: f64, z: f64) -> f64 {
        let natural = self.natural(x, z, |_, _, _, _| {});
        natural.height - natural.coarse()
    }

    /// The provinces' valley section at a point, by their weights; a V in a
    /// world without geology.
    fn section(&self, mix: Option<&ProvinceMix>) -> crate::hydrology::Section {
        let (Some(geology), Some(mix)) = (self.geology.as_ref(), mix) else {
            return crate::hydrology::Section::V;
        };
        let mut sum = crate::hydrology::Section::ZERO;
        let mut total = 0.0;
        for &(id, w) in mix.entries() {
            sum = sum.add(&geology.sections[id.0 as usize], w);
            total += w;
        }
        if total > 0.0 {
            sum.scaled(1.0 / total)
        } else {
            crate::hydrology::Section::V
        }
    }
}

/// Every frozen biome prepared for sampling, in registry order: the order is
/// what a [`Classification`]'s indices mean.
fn biome_samplers(
    seed: Seed,
    registries: &WorldDefs,
    params: &WorldParams,
    range_axis: (f64, f64),
) -> Vec<BiomeSampler> {
    let windows: Vec<ClimateWindow> = registries
        .biomes
        .iter()
        .map(|(_, _, def)| def.climate)
        .collect();
    registries
        .biomes
        .iter()
        .enumerate()
        .map(|(index, (id, name, def))| {
            let surface = registries
                .surface(id)
                .expect("every frozen biome has a resolved surface");
            let shape = match def.relief.kind {
                ReliefKind::Plains | ReliefKind::Hills => Shape::Fbm,
                ReliefKind::Mountains | ReliefKind::Ridges => Shape::Ridged,
                ReliefKind::Dunes => Shape::Billow,
            };
            let noise = Fractal::new(
                seed.derive(&format!("relief/{name}")),
                shape,
                f64::from(def.relief.wavelength_m),
                def.relief.octaves as usize,
                f64::from(def.relief.persistence),
                f64::from(def.relief.lacunarity),
            );
            // The sea floor's and the shore's relief stay the biome's own:
            // a biome whose window tops out at the shore step is under the
            // water or on the beach, where a province's shape would only
            // move the coastline (a beach rises a metre in about 80), and
            // with it every place fixed by the sea.
            let shore_top = 0.5 + params.shore_step;
            // Compared in the window's own precision: 0.55 as an f32 is a
            // hair over 0.55.
            #[allow(clippy::cast_possible_truncation, reason = "the window's own f32")]
            let keep_own = def.climate.elevation.max <= shore_top as f32;
            let (variants, provinces) = province_reliefs(
                seed,
                registries,
                def,
                name.as_str(),
                (shape, keep_own),
                range_axis,
            );
            #[allow(clippy::float_cmp, reason = "the identity, to the bit")]
            let shaped = provinces.iter().any(|p| {
                p.variant != 0 || p.amplitude != 1.0 || p.erosion != f64::from(def.relief.erosion)
            });
            // Read only by a province's shape; measured only when there is one.
            let own_mean = if variants.is_empty() {
                0.0
            } else {
                shape_mean(
                    shape,
                    def.relief.octaves as usize,
                    f64::from(def.relief.persistence),
                    f64::from(def.relief.lacunarity),
                    Tune::DEFAULT,
                )
            };
            BiomeSampler {
                shore: keep_own,
                own_mean,
                variants,
                provinces,
                shaped,
                id,
                window: def.climate,
                kind: def.relief.kind,
                coarse_noise: noise.truncated(2),
                lowest: noise.truncated({
                    // Constructor arithmetic: no sample reads it.
                    let shortest = crate::hydrology::RELIEF_SPACINGS * crate::hydrology::LATTICE_M;
                    let mut wavelength = f64::from(def.relief.wavelength_m);
                    let mut kept = 0;
                    while kept < def.relief.octaves as usize && wavelength >= shortest {
                        kept += 1;
                        wavelength /= f64::from(def.relief.lacunarity).max(1.0 + f64::EPSILON);
                    }
                    kept.max(1)
                }),
                noise,
                amplitude_m: f64::from(def.relief.amplitude_m),
                lift_m: f64::from(def.relief.lift_m),
                material: surface.material.layer(),
                slope_material: surface.slope_material.layer(),
                slope_threshold: f64::from(surface.slope_threshold),
                erosion: f64::from(def.relief.erosion),
                rise: {
                    let others: Vec<ClimateWindow> = windows
                        .iter()
                        .enumerate()
                        .filter(|&(other, _)| other != index)
                        .map(|(_, window)| *window)
                        .collect();
                    let low = alone_from(&def.climate, &others, params.blend_gap)
                        - params.mountain_rise_lead;
                    (low, low + params.mountain_rise)
                },
            }
        })
        .collect()
}

/// What each province makes of one biome's relief: the variants (shapes
/// other than the biome's own, each once) and, per province in id order,
/// its variant and numbers. A province with no rule for the biome's class
/// takes the identity: variant 0, amplitude 1, the biome's erosion; so does
/// every province for a biome that `keep_own`s its relief.
fn province_reliefs(
    seed: Seed,
    registries: &WorldDefs,
    def: &strata_data::BiomeDef,
    name: &str,
    (own, keep_own): (Shape, bool),
    range_axis: (f64, f64),
) -> (Vec<Variant>, Vec<ProvinceRelief>) {
    let erosion = f64::from(def.relief.erosion);
    let mut keys: Vec<(Shape, u32, u32)> = Vec::new();
    let mut variants = Vec::new();
    let provinces = registries
        .provinces
        .iter()
        .map(|(id, _, _)| {
            let traits = registries
                .province(id)
                .expect("every frozen province is resolved");
            let Some(rule) = traits.rule(def.relief.kind).filter(|_| !keep_own) else {
                return ProvinceRelief {
                    variant: 0,
                    amplitude: 1.0,
                    erosion,
                    tune: Tune::DEFAULT,
                };
            };
            let shape = match rule.shape {
                ProvinceShape::Fbm => Shape::Fbm,
                ProvinceShape::Ridged => Shape::Ridged,
                ProvinceShape::Billow => Shape::Billow,
                ProvinceShape::Damped => Shape::Damped,
                ProvinceShape::Hybrid => Shape::Hybrid,
                ProvinceShape::Cells => Shape::Cells,
            };
            let key = (
                shape,
                rule.wavelength.to_bits(),
                rule.stretch.max(1.0).to_bits(),
            );
            // The biome's own shape at its own wavelength, round, is its
            // own noise.
            let variant = if key == (own, 1.0_f32.to_bits(), 1.0_f32.to_bits()) {
                0
            } else if let Some(k) = keys.iter().position(|k| *k == key) {
                k + 1
            } else {
                keys.push(key);
                let noise = Fractal::new(
                    seed.derive(&format!("relief/{name}/{}", rule.shape)),
                    shape,
                    f64::from(def.relief.wavelength_m) * f64::from(rule.wavelength),
                    def.relief.octaves as usize,
                    f64::from(def.relief.persistence),
                    f64::from(def.relief.lacunarity),
                )
                .stretched(range_axis, f64::from(rule.stretch));
                let tune = Tune {
                    damping: f64::from(rule.damping),
                    offset: f64::from(rule.offset),
                };
                variants.push(Variant {
                    coarse: noise.truncated(2),
                    noise,
                    mean: shape_mean(
                        shape,
                        def.relief.octaves as usize,
                        f64::from(def.relief.persistence),
                        f64::from(def.relief.lacunarity),
                        tune,
                    ),
                });
                keys.len()
            };
            ProvinceRelief {
                variant,
                amplitude: f64::from(rule.amplitude),
                erosion: (erosion * f64::from(rule.erosion)).min(4.0),
                tune: Tune {
                    damping: f64::from(rule.damping),
                    offset: f64::from(rule.offset),
                },
            }
        })
        .collect();
    (variants, provinces)
}

/// How much of a range stands at a profile value, `0..=1`, and its
/// derivative by the value, for a power `foot`.
///
/// The value between [`WorldParams::range_floor`] and
/// [`WorldParams::range_crest`], mapped to `0..=1`, raised to the power. The
/// continent's value is how far in from the edge of the range's width its
/// point is ([`across_profile`], ticket 0004's step 4), which rises about
/// linearly to a rounded crest; at [`WorldParams::range_foot`] 3 the lift is
/// concave all the way up, a long gentle foot steepening towards the crest.
/// The rain march's value is the fold of the field (`1 - n^2`), at
/// [`WorldParams::rain_range_foot`] 6 the bell step 2b tuned the rain on.
/// Round at the crest is the lift's shape, not always the ground's: the
/// elevation is clamped at 1, which a crest reaches wherever the continent
/// field is over about 0.15, and there its base is flat; the range's own
/// metres ([`Continent::range_m`]) are not clamped and rise on to the
/// line. A power by repeated multiplication, which is correctly rounded
/// at every step, not `powi`, whose precision is unspecified; a power of 0
/// counts as 1.
fn range_lift(params: &WorldParams, fold: f64, foot: usize) -> (f64, f64) {
    let span = params.range_crest - params.range_floor;
    let t = (fold - params.range_floor) / span;
    if t <= 0.0 {
        return (0.0, 0.0);
    }
    if t >= 1.0 {
        return (1.0, 0.0);
    }
    let foot = foot.max(1);
    let mut below = 1.0;
    for _ in 1..foot {
        below *= t;
    }
    #[allow(clippy::cast_precision_loss, reason = "a power of a few")]
    let power = foot as f64;
    (below * t, power * below / span)
}

/// How much of a range's height stands at a continent field `field`, and
/// its derivative by the field: none at the land's edge (0.02, where the
/// land mask starts), all from [`WorldParams::range_coast`] inland, eased by
/// [`WorldParams::range_coast_power`].
///
/// A range has to be gone at the shore or it would move the coast, and it
/// used to be multiplied by the land mask, whose ramp is about 1.3 km wide:
/// a range meeting the coast stood at full height 1.5 km from the beach
/// and fell to nothing across that ramp, and the mountains' relief, which
/// rises over a narrow band of elevation, switched on across a few hundred
/// metres of it (ticket 0004's review of step 2b). The eased fade is steep
/// only low down, where the elevation is under the mountains' band, and
/// long near its top. Powers by repeated multiplication, as in
/// [`range_lift`].
fn range_coast(params: &WorldParams, field: f64) -> (f64, f64) {
    let s = smoothstep(0.02, params.range_coast, field);
    let ds = smoothstep_slope(0.02, params.range_coast, field);
    let power = params.range_coast_power.max(1);
    let mut below = 1.0;
    for _ in 1..power {
        below *= 1.0 - s;
    }
    #[allow(clippy::cast_precision_loss, reason = "a power of a few")]
    let n = power as f64;
    (1.0 - below * (1.0 - s), n * below * ds)
}

/// A range's own metres at a point, from the range field's [`RangeFrame`]
/// and the continent field (ticket 0004, step 4).
///
/// Read from the distance to the crest line, not from the field's value:
/// `d = |r| / G` ([`crest_distance`]), the first-order distance to the zero
/// contour the range runs along, in metres. The elevation's bell was built
/// on the field's value, whose rate of change varies threefold along a
/// range, so the same bell was a flank of 2 km in one place and a plateau
/// of 5 km in another. The first-order distance varies much less, though it
/// is no exact distance: against a distance transform of the contour
/// (the review of step 4, where the true distance is 2 to 8 km) it reads
/// 0.78 of it at the 5th percentile, 1.16 at the median and 2.39 at the
/// 95th, so a range's metres are several hundred off in places. Across the
/// line it is `(1 - (q - c) / D)^p` with `q = sqrt(d^2 + c^2)`: out to
/// [`WorldParams::range_height_width`] `D`, concave for a power over one,
/// so the ground steepens all the way up to a crest rounded over
/// [`WorldParams::range_height_round`] `c` (the `sqrt` bends the roof line
/// the absolute value would leave into a curve). Faded to the coast by a
/// smoothstep of the field from 0.02 to [`WorldParams::range_height_coast`].
/// Powers by repeated multiplication, `sqrt` correctly rounded (the float
/// rule).
fn range_height(params: &WorldParams, frame: &RangeFrame, field: f64) -> RangeHeight {
    let h = params.range_height_m;
    if h == 0.0 {
        return RangeHeight::NONE;
    }
    let (d, dd) = crest_distance(frame);
    let (t, dt) = across_profile(d, params.range_height_width, params.range_height_round);
    if t <= 0.0 {
        return RangeHeight::NONE;
    }
    let power = params.range_height_power.max(1);
    let mut below = 1.0;
    for _ in 1..power {
        below *= t;
    }
    let shape = below * t;
    #[allow(clippy::cast_precision_loss, reason = "a power of a few")]
    let d_shape = power as f64 * below * dt;
    let coast = smoothstep(0.02, params.range_height_coast, field);
    let d_coast = smoothstep_slope(0.02, params.range_height_coast, field);
    RangeHeight {
        metres: h * shape * coast,
        gradient: (h * d_shape * coast * dd.0, h * d_shape * coast * dd.1),
        d_field: h * shape * d_coast,
        flank: t * coast,
        flank_gradient: (dt * coast * dd.0, dt * coast * dd.1),
        flank_d_field: t * d_coast,
    }
}

/// How far apart, in metres of the warped frame, the range field is read
/// for the size of its gradient
/// ([`ProceduralWorld::ridge_frame_with_gradient`]): under a hundredth of the
/// finest octave's wavelength (17.5 km), so the difference is the gradient
/// half a step on, and the change in the distance's slope where the
/// kernel's second derivative jumps is spread over a hundred metres. At 500
/// m that half step lowered the city's upper terrace's view from 6.06 to
/// 5.91 degrees (`tests/port_serein.rs` holds 6); at 100 m it is 6.03.
const RANGE_SCALE_STEP_M: f64 = 100.0;

/// The range field at a point and what its distance to its crest line is
/// read from, in the warped frame.
#[derive(Debug, Clone, Copy)]
struct RangeFrame {
    /// The signed field.
    ridge: f64,
    /// Its gradient per metre; zero where only values were asked for.
    gradient: (f64, f64),
    /// The size of its gradient, smoothed: from differences over
    /// [`RANGE_SCALE_STEP_M`].
    scale: f64,
    /// The gradient of `scale`; zero where only values were asked for.
    scale_gradient: (f64, f64),
}

/// The first-order distance, in metres, from a point to the zero contour
/// of the range field, `|r| / G`, and its gradient: `grad d = s grad r / G -
/// |r| grad G / G^2`, `s` the sign of `r` and `G` the smoothed size of its
/// gradient ([`RangeFrame::scale`]). Where that is nearly nil (a saddle of
/// the field) the distance is taken over a floor of it, which is
/// continuous.
fn crest_distance(frame: &RangeFrame) -> (f64, (f64, f64)) {
    /// The gradient of the range field under which the distance stops
    /// trusting it, per metre: a hundredth of its typical size, so a
    /// distance over 100 km is read, well beyond any flank.
    const FLOOR: f64 = 1e-6;
    let RangeFrame {
        ridge,
        gradient: (gx, gz),
        scale: g,
        scale_gradient: (sx, sz),
    } = *frame;
    let sign = if ridge < 0.0 { -1.0 } else { 1.0 };
    let r = ridge.abs();
    if g > FLOOR {
        let g2 = g * g;
        (
            r / g,
            (sign * gx / g - r * sx / g2, sign * gz / g - r * sz / g2),
        )
    } else {
        (r / FLOOR, (sign * gx / FLOOR, sign * gz / FLOOR))
    }
}

/// How far in from the edge of a profile `width` metres wide a point
/// `d` metres from the crest line is, `1 - (q - c) / width` with `q =
/// sqrt(d^2 + c^2)`, and its derivative by `d`: one on the line, falling
/// about linearly, rounded within `c` of the line (the `sqrt` bends the
/// roof line an absolute value would leave), negative beyond the edge.
fn across_profile(d: f64, width: f64, c: f64) -> (f64, f64) {
    let q = (d * d + c * c).sqrt();
    (1.0 - (q - c) / width, -d / (q * width))
}

/// How much of a range's own metres stand at an elevation, and its
/// derivative by the elevation: a smoothstep from
/// [`WorldParams::range_root`]'s first elevation to its second. The plains
/// and the other lowland biomes are authored against the elevation, which
/// the range's own metres are not in; without this the metres lifted the
/// plains at a range's foot 90 m over the 400 they are held to
/// (`tests/coverage.rs`).
fn range_rooting(params: &WorldParams, elevation: f64) -> (f64, f64) {
    let (lo, hi) = params.range_root;
    (
        smoothstep(lo, hi, elevation),
        smoothstep_slope(lo, hi, elevation),
    )
}

/// [`range_height`]'s answer. Gradients are in the warped frame; the parts
/// by the continent field are apart, for the caller to carry.
#[derive(Debug, Clone, Copy)]
struct RangeHeight {
    /// Metres.
    metres: f64,
    /// The metres' gradient by the range field's distance, per metre.
    gradient: (f64, f64),
    /// The metres' derivative by the continent field.
    d_field: f64,
    /// How far up the flank, `0..=1`: the profile's base before its power,
    /// faded to the coast as the metres are. Zero at the foot and beyond,
    /// one on the crest line.
    flank: f64,
    flank_gradient: (f64, f64),
    flank_d_field: f64,
}

impl RangeHeight {
    const NONE: Self = Self {
        metres: 0.0,
        gradient: (0.0, 0.0),
        d_field: 0.0,
        flank: 0.0,
        flank_gradient: (0.0, 0.0),
        flank_d_field: 0.0,
    };
}

/// The derivative of [`smoothstep`] by `x`.
fn smoothstep_slope(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = (x - edge0) / (edge1 - edge0);
    if t <= 0.0 || t >= 1.0 {
        0.0
    } else {
        6.0 * t * (1.0 - t) / (edge1 - edge0)
    }
}

/// The runner-up slot and the winner's share of the pair. With no
/// runner-up within the gap the winner fills both slots at full weight.
fn pair(best: (usize, f64), second: (usize, f64)) -> (usize, f64) {
    if second.1 <= 0.0 {
        (best.0, 1.0)
    } else {
        (second.0, best.1 / (best.1 + second.1))
    }
}

/// The margin of `value` inside `range`, or the negative distance outside.
///
/// An edge at `0` or `1` is open: a biome that spans the whole axis should
/// not lose points for a value near the end of it.
fn margin(range: UnitRange, value: f64) -> f64 {
    let lower = if range.min <= 0.0 {
        f64::INFINITY
    } else {
        value - f64::from(range.min)
    };
    let upper = if range.max >= 1.0 {
        f64::INFINITY
    } else {
        f64::from(range.max) - value
    };
    lower.min(upper)
}

fn window_score(window: &ClimateWindow, climate: &Climate) -> f64 {
    margin(window.temperature, climate.temperature)
        .min(margin(window.moisture, climate.moisture))
        .min(margin(window.elevation, climate.elevation))
}

/// Each stage's part in the height of the natural ground at a point, as
/// [`ProceduralWorld::terms`] gives it: what a flank's profile is made of
/// (ticket 0004, step 4).
#[derive(Debug, Clone, PartialEq)]
pub struct Terms {
    /// The continent stage: the base height with the range's lift in it.
    pub continent: Continent,
    /// Each biome in the blend: its id, its share of the blend (the shares
    /// sum to one), the metres of relief it makes here before its share,
    /// and for the mountain kind the share of its relief that has risen
    /// ([`WorldParams::mountain_rise`]), zero for every other kind.
    pub biomes: Vec<(BiomeId, f64, f64, f64)>,
    /// The blended relief, metres: the biomes' relief by their shares.
    pub relief_m: f64,
    /// What the provinces' beds added, metres (ticket 0004, step 5): up for
    /// a hard bed's bench, down for a soft bed's eased foot.
    pub beds_m: f64,
    /// What the erosion filter cut, metres (zero or less).
    pub erosion_m: f64,
    /// The natural ground: the base, the relief and the erosion, before a
    /// settlement shapes it.
    pub height_m: f64,
}

/// What [`ProceduralWorld::natural`] works out for a point: the stages up to
/// the erosion, before a settlement shapes the ground.
struct Natural {
    continent: Continent,
    climate: Climate,
    /// The provinces' pair and their blended `rock.cover`, in a world with
    /// geology.
    provinces: Option<([(strata_data::ProvinceId, f32); 2], f64)>,
    /// Every province weighing at the point, in a world with geology.
    mix: Option<ProvinceMix>,
    best: usize,
    second: usize,
    /// The best biome's share of the pair.
    weight: f64,
    relief: f64,
    eroded: crate::erosion::Eroded,
    /// What the beds added, metres.
    beds_m: f64,
    /// Where the surface lies in the heaviest bedded province's beds, and
    /// that province.
    bed: Option<(BedAt, strata_data::ProvinceId)>,
    height: f64,
    gradient: (f64, f64),
    /// The blended coarse relief at the point and at the four hydration taps
    /// round it.
    centre: f64,
    ring: f64,
}

impl Natural {
    /// The natural ground's coarse part: the continent's base, the range's
    /// own metres and the relief's first octaves at the point, the ground
    /// before the shapes, the beds and the gullies.
    fn coarse(&self) -> f64 {
        self.continent.base_m + self.continent.range_m + self.centre
    }
}

impl ProceduralWorld {
    /// Each stage's part in the height of the natural ground at `(x, z)`.
    ///
    /// A diagnostic, for the measures and the probes that ask which term
    /// makes a shape (ticket 0004, step 4, took a range's foot apart with
    /// it): the same arithmetic as [`sample`](WorldSource::sample), so
    /// `height_m` is the natural ground's height to the bit. Allocates.
    #[must_use]
    pub fn terms(&self, x: f64, z: f64) -> Terms {
        let mut biomes = Vec::new();
        let natural = self.natural(x, z, |index, weight, relief, rise| {
            biomes.push((self.biomes[index].id, weight, relief, rise));
        });
        let total: f64 = biomes.iter().map(|b| b.1).sum();
        for biome in &mut biomes {
            biome.1 /= total;
        }
        Terms {
            continent: natural.continent,
            biomes,
            relief_m: natural.relief,
            beds_m: natural.beds_m,
            erosion_m: natural.eroded.offset_m,
            height_m: natural.height,
        }
    }

    /// The stages of a sample up to the erosion: continent, climate,
    /// provinces, the biomes' relief and the gullies. `on_biome(index,
    /// weight, relief_m, rise)` is told each biome's unnormalised weight,
    /// the metres of relief it makes and the share of the mountain kind's
    /// relief that has risen; the sampler passes a closure that does
    /// nothing.
    #[allow(clippy::too_many_lines, reason = "the stages of one sample, in order")]
    fn natural(&self, x: f64, z: f64, mut on_biome: impl FnMut(usize, f64, f64, f64)) -> Natural {
        // The elevation's gradient is read only part-way up a mountain
        // relief's rise, but a second continent stage there would cost more
        // than carrying the derivatives through the first everywhere.
        let (continent, slopes) = self.continent_with_gradient(x, z);
        let elevation_gradient = slopes.elevation;
        let climate = self.climate(x, z, &continent);
        // The provinces: what the ground is made of, from a field of their
        // own at the continent's warped point (ticket 0004, step 3). Read
        // by the cover below and carried in the sample; no height reads it
        // yet.
        let mix = self.geology.as_ref().map(|geology| {
            geology
                .field
                .sample(continent.warped.0, continent.warped.1, continent.range)
        });
        let provinces = self
            .geology
            .as_ref()
            .zip(mix.as_ref())
            .map(|(geology, mix)| {
                let cover: f64 = mix
                    .entries()
                    .iter()
                    .map(|&(id, w)| geology.cover[id.0 as usize] * w)
                    .sum();
                (mix.pair(), cover)
            });

        // Stages 3 and 4 fused: relief is accumulated as the blend weights
        // are produced, so nothing per biome has to be stored. The slope is
        // the blend's too, from each biome's analytic gradient, so it does
        // not jump where two biomes swap first place.
        let radius = self.params.hydration_radius_m;
        // The coarse relief at the point and four taps around it, blended:
        // hydration reads whether the point is a hollow from them, and
        // erosion the large-scale direction its gullies run in.
        let mut coarse = [0.0; 5];
        let mut relief = 0.0;
        let mut gradient = (0.0, 0.0);
        let mut height_share = 0.0;
        let mut strength = 0.0;
        let mut total = 0.0;
        let mut inland = 0.0;
        let mut best = (0, 0.0);
        let mut second = (0, 0.0);
        self.blend(&climate, |index, weight| {
            let biome = &self.biomes[index];
            if !biome.shore {
                inland += weight;
            }
            let rise = biome.rise(
                continent.elevation,
                elevation_gradient,
                self.params.mountain_rise_power,
            );
            // Each province's shape for the biome's class (ticket 0004,
            // step 4), the biome's own where the province names none.
            let mut here = biome.relief_at(x, z, rise, radius, mix.as_ref());
            // Towards a range's crest line the mountains' relief grows, so the
            // crests are serrated rock while the foot, where the flank is
            // nothing, keeps its soft rise (ticket 0004, step 5).
            if biome.kind == ReliefKind::Mountains && self.params.crest_relief_gain > 0.0 {
                let (grow, d_grow) = crest_growth(&self.params, continent.flank);
                here.gradient = (
                    here.gradient.0 * grow + here.here * d_grow * slopes.flank.0,
                    here.gradient.1 * grow + here.here * d_grow * slopes.flank.1,
                );
                here.here *= grow;
                for tap in &mut here.coarse {
                    *tap *= grow;
                }
            }
            // Up a range's flank the foothills' relief lies down, so the
            // flank rises as one slope and not as hills with shelves between
            // them (ticket 0004, step 4); the mountains' own relief is theirs.
            if biome.kind != ReliefKind::Mountains && self.params.flank_damping > 0.0 {
                let k = self.params.flank_damping;
                let keep = 1.0 - k * continent.flank;
                here.gradient = (
                    here.gradient.0 * keep - here.here * k * slopes.flank.0,
                    here.gradient.1 * keep - here.here * k * slopes.flank.1,
                );
                here.here *= keep;
                for tap in &mut here.coarse {
                    *tap *= keep;
                }
            }
            on_biome(index, weight, here.here, rise.share);
            relief += here.here * weight;
            gradient.0 += here.gradient.0 * weight;
            gradient.1 += here.gradient.1 * weight;
            height_share += here.share * weight;
            strength += here.erosion * weight;
            for (sum, tap) in coarse.iter_mut().zip(here.coarse) {
                *sum += tap * weight;
            }
            total += weight;
            if weight > best.1 {
                second = best;
                best = (index, weight);
            } else if weight > second.1 {
                second = (index, weight);
            }
        });
        let (second, w) = pair(best, second);
        let relief = relief / total;
        let gradient = (gradient.0 / total, gradient.1 / total);

        // Stage 5: water's carving, then the surface. The gullies follow the
        // large-scale slope, not the local one: ridged relief folds sharp
        // creases into the ground, and a gully steered by the local
        // direction would swing round with it and carve a cliff. The coarse
        // slope is smallest exactly where it turns, at a crest, and the
        // filter fades out there. Its strength is blended, so gullies fade
        // across a border rather than stopping at it.
        let strength = strength / total;
        let [centre, east_tap, west_tap, south_tap, north_tap] = coarse.map(|sum| sum / total);
        // A range's own metres are part of the large-scale slope, so its
        // flank is carved as the relief's would be (ticket 0004, step 4).
        let carve_share = self.params.range_erosion_share;
        let steer = (
            (east_tap - west_tap) / (2.0 * radius) + slopes.range_m.0 * carve_share,
            (south_tap - north_tap) / (2.0 * radius) + slopes.range_m.1 * carve_share,
        );
        let eroded = self
            .erosion
            .carve(x, z, steer, strength, height_share / total);
        let ring = (east_tap + west_tap + south_tap + north_tap) * 0.25;
        // The provinces' beds, before the gullies so that these cut through
        // them (ticket 0004, step 5).
        let beds = self.beds(
            (x, z),
            mix.as_ref(),
            continent.base_m + continent.range_m + relief,
            (
                gradient.0 + slopes.range_m.0 + slopes.base_m.0,
                gradient.1 + slopes.range_m.1 + slopes.base_m.1,
            ),
            (
                (
                    (east_tap - west_tap) / (2.0 * radius),
                    (south_tap - north_tap) / (2.0 * radius),
                ),
                slopes.range_m,
                (centre - ring).max(0.0) / radius,
            ),
            inland / total,
        );
        let height = continent.base_m + continent.range_m + relief + beds.offset + eroded.offset_m;
        let gradient = (
            gradient.0 + beds.gradient.0 + eroded.gradient.0 + slopes.range_m.0,
            gradient.1 + beds.gradient.1 + eroded.gradient.1 + slopes.range_m.1,
        );
        let bed = beds.bed;
        Natural {
            continent,
            climate,
            provinces,
            mix,
            best: best.0,
            second,
            weight: w,
            relief,
            eroded,
            beds_m: beds.offset,
            bed,
            height,
            gradient,
            centre,
            ring,
        }
    }

    /// What the provinces' beds at a point make of ground `height` metres
    /// high with the whole gradient `gradient` (the continent's base in it),
    /// `relief` being the relief's coarse gradient, `range` the range's own
    /// metres' and `stands` how far the ground stands over the ground round
    /// it as a slope ([`crate::strata`]'s gate), and
    /// `inland` the share of the biomes' blend that is not the sea's or the
    /// shore's, which the beds are faded by as the provinces' shapes are
    /// kept off those biomes (a beach is not terraced): each bedded province's by its weight, so
    /// beds fade across a border as the shapes do, and nothing at all where
    /// no province weighing has beds or where they are gated out, so that
    /// ground is left to the bit.
    fn beds(
        &self,
        (x, z): (f64, f64),
        mix: Option<&ProvinceMix>,
        height: f64,
        gradient: (f64, f64),
        (relief, range, stands): ((f64, f64), (f64, f64), f64),
        inland: f64,
    ) -> BedsHere {
        let mut out = BedsHere {
            offset: 0.0,
            gradient: (0.0, 0.0),
            bed: None,
        };
        let (Some(geology), Some(mix)) = (self.geology.as_ref(), mix) else {
            return out;
        };
        if !geology.bedded || inland <= 0.0 {
            return out;
        }
        let total: f64 = mix.entries().iter().map(|&(_, w)| w).sum();
        // The gate opens on the relief's own flanks everywhere, and on a
        // range's ramp only in patches: that ramp is smooth and runs for tens
        // of kilometres along a range, and beds opened on all of it banded
        // every bedded range foot with risers along its contours, unbroken
        // (the review of step 5). The fade reads the range's whole slope
        // with the relief's, since it asks whether the ground is a cliff
        // already.
        let coarse = (relief.0 + range.0, relief.1 + range.1);
        let length = |(a, b): (f64, f64)| (a * a + b * b).sqrt();
        // `|relief + m range|` for `m` in `0..=1` is at most the larger of
        // its ends, so where the gates are shut at that the patch is not
        // read.
        let most = length(relief).max(length(coarse));
        let mut slope = None;
        let mut heaviest = 0.0;
        for &(id, w) in mix.entries() {
            let Some(beds) = geology.beds[id.0 as usize].as_ref() else {
                continue;
            };
            if beds.gate(most, stands) <= 0.0 {
                continue;
            }
            let slope = *slope.get_or_insert_with(|| {
                let m = geology.patch(x, z, &self.params);
                length((relief.0 + m * range.0, relief.1 + m * range.1))
            });
            let gate = beds.gate(slope, stands) * inland;
            if gate <= 0.0 {
                continue;
            }
            let dip = beds.dip(x, z);
            let bedded = beds.apply(dip, height, gradient, (gate, coarse), &self.params);
            let share = w / total;
            out.offset += bedded.offset * share;
            out.gradient.0 += bedded.gradient.0 * share;
            out.gradient.1 += bedded.gradient.1 * share;
            if let Some(bed) = bedded.bed.filter(|_| w > heaviest) {
                heaviest = w;
                out.bed = Some((bed, id));
            }
        }
        out
    }
}

/// [`ProceduralWorld::beds`]'s answer.
struct BedsHere {
    /// Metres the beds add.
    offset: f64,
    /// Added to the gradient.
    gradient: (f64, f64),
    /// The bed that shaped the ground, of the heaviest province whose beds
    /// did, and that province.
    bed: Option<(BedAt, strata_data::ProvinceId)>,
}

/// How much a range's mountain relief is multiplied by at `flank`
/// ([`WorldParams::crest_relief_gain`]), and its derivative by the flank.
/// The power by repeated multiplication (the float rule).
fn crest_growth(params: &WorldParams, flank: f64) -> (f64, f64) {
    let power = params.crest_relief_power.max(1);
    let flank = flank.clamp(0.0, 1.0);
    let mut below = 1.0;
    for _ in 1..power {
        below *= flank;
    }
    #[allow(clippy::cast_precision_loss, reason = "a power of a few")]
    let d = params.crest_relief_gain * power as f64 * below;
    (1.0 + params.crest_relief_gain * below * flank, d)
}

impl WorldSource for ProceduralWorld {
    /// Every stage for one point. Anything that changes what this returns
    /// bumps [`GENERATOR_VERSION`](crate::GENERATOR_VERSION).
    fn sample(&self, x: f64, z: f64) -> Sample {
        self.sample_with(x, z, Ground::Carved)
    }

    fn sea_level(&self) -> f32 {
        0.0
    }
}

impl ProceduralWorld {
    /// Every stage for one point, the carve left out while the world is
    /// made: the sites are placed over the uncarved ground, and the network
    /// is routed round the sites.
    fn sample_with(&self, x: f64, z: f64, ground: Ground<'_>) -> Sample {
        let natural = self.natural(x, z, |_, _, _, _| {});
        let params = self.hydrology.params();
        match ground {
            Ground::Natural => self.finish_sample(x, z, natural, None, None, false),
            // A site's pad and the roads' grade are read from the cell the
            // carve reads: one lookup for all three, and a cell nothing
            // reaches holds none.
            Ground::Carved => self.hydrology.with_cell(self, x, z, |cell| {
                let here = cell.gather(params, x, z);
                self.finish_sample(x, z, natural, Some(cell), Some(&here), true)
            }),
            Ground::Ungraded => self.hydrology.with_cell(self, x, z, |cell| {
                let here = cell.gather(params, x, z);
                self.finish_sample(x, z, natural, Some(cell), Some(&here), false)
            }),
            Ground::By(network) => {
                let here = crate::hydrology::Cell::gather_one(network, params, x, z);
                self.finish_sample(x, z, natural, None, Some(&here), false)
            }
        }
    }

    /// The stages after the natural ground, over the cell the point is in
    /// (`None` while the world is made, and in a bake, which samples the
    /// carve alone) and what the carve gathered there; the roads' grade
    /// where `graded`.
    #[allow(clippy::too_many_lines, reason = "the stages of one sample, in order")]
    fn finish_sample(
        &self,
        x: f64,
        z: f64,
        natural: Natural,
        cell: Option<&crate::hydrology::Cell>,
        gathered: Option<&crate::hydrology::carve::Gathered>,
        graded: bool,
    ) -> Sample {
        let coarse = natural.coarse();
        let Natural {
            climate,
            provinces,
            mix,
            best,
            second,
            weight: w,
            eroded,
            height,
            gradient: (gx, gz),
            centre,
            ring,
            bed,
            ..
        } = natural;
        let (mut height, mut gx, mut gz) = (height, gx, gz);
        let mut bed = bed;
        let first = &self.biomes[best];
        let other = &self.biomes[second];
        // The carve (ticket 0005, step 2): the rivers' valleys and channels
        // and the lakes' beds, from the network round the point. A point no
        // river reaches reads one cell and leaves.
        let mut water = None;
        let mut under = false;
        let mut bed_layer = None;
        let pad = cell.and_then(|cell| cell.pad_at(x, z));
        let params = self.hydrology.params();
        if let Some(here) = gathered
            && (here.river.is_some() || here.lake.is_some())
        {
            {
                let section = self.section(mix.as_ref());
                // The beds a deep cut wall steps into: every province's
                // there by its weight, where the blended section steps at
                // all.
                let steps = (section.steps > 0.0)
                    .then(|| {
                        let geology = self.geology.as_ref()?;
                        Some(crate::hydrology::carve::Steps {
                            provinces: mix.as_ref()?.entries(),
                            beds: &geology.beds,
                            sections: &geology.sections,
                            at: (x, z),
                            params: &self.params,
                        })
                    })
                    .flatten();
                let carved = crate::hydrology::carve::carve(
                    params,
                    height,
                    (gx, gz),
                    coarse,
                    here,
                    &section,
                    self.range_axis,
                    steps.as_ref(),
                    (x, z),
                );
                // The bands a cut wall shows are the beds at its own
                // height, not those of the ground cut away.
                if let (Some((at, id)), Some(geology)) = (bed, self.geology.as_ref())
                    && carved.height.to_bits() != height.to_bits()
                    && let Some(beds) = geology.beds[id.0 as usize].as_ref()
                {
                    bed = Some((beds.relabel(at, carved.height - height), id));
                }
                // How far the carve lowered the point, in the record's
                // decimetres: what marks a valley's cut wall.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to the u16's range first"
                )]
                let cut_dm = ((height - carved.height) * 10.0)
                    .round()
                    .clamp(0.0, 65_535.0) as u16;
                height = carved.height;
                (gx, gz) = carved.gradient;
                water = carved.water.map(|water| SampleWater {
                    cut_dm,
                    ..SampleWater::from(water)
                });
                under = carved.under;
                bed_layer = carved.bed.and_then(|bed| match bed {
                    crate::hydrology::carve::Bed::Gravel => self.bed_layers[0],
                    crate::hydrology::carve::Bed::Sand => self.bed_layers[1],
                });
            }
        }
        // The roads' grade (ticket 0005, step 6), on the carved ground and
        // under a site's pad, which a road arrives at its level.
        let mut road_cut = 0.0;
        let mut cleared = 0.0;
        let mut way: Option<(f64, strata_data::RoadClass)> = None;
        if graded
            && let Some(grid) = cell.and_then(|cell| cell.roads.as_ref())
            && let Some(mut graded) =
                grid.shape(x, z, crate::hydrology::dual::D::new(height, [gx, gz]))
        {
            // By a river's channel a road's ground is let in only at one of
            // its crossings: a fill beside the water does not spill into it.
            if let (Some(river), Some(w)) = (gathered.and_then(|g| g.river.as_ref()), water)
                && w.kind != crate::hydrology::WaterKind::Lake
            {
                let (width, depth) = (f64::from(w.width_m), f64::from(w.surface_m - w.bed_m));
                let share = crate::hydrology::grade::channel_share(
                    river.channel,
                    0.5 * width,
                    params.bank_top_m(width, depth),
                    graded.crossing,
                );
                if share.v < 1.0 {
                    let carved = crate::hydrology::dual::D::new(height, [gx, gz]);
                    graded.height = carved + (graded.height - carved) * share;
                }
            }
            road_cut = height - graded.height.v;
            if road_cut != 0.0 {
                // The bands a cut bank shows are the beds at its own height.
                if let (Some((at, id)), Some(geology)) = (bed, self.geology.as_ref())
                    && let Some(beds) = geology.beds[id.0 as usize].as_ref()
                {
                    bed = Some((beds.relabel(at, graded.height.v - height), id));
                }
                height = graded.height.v;
                (gx, gz) = (graded.height.g[0], graded.height.g[1]);
                // A culvert's fill or a ford's ramp: under water only where
                // the graded ground still lies under the surface.
                under &= water.is_some_and(|w: SampleWater| height < f64::from(w.surface_m));
            }
            cleared = graded.cleared;
            way = graded.class.map(|class| (graded.developed, class));
        }
        // A procedural site's pad (ADR 0012), on the carved ground: no pad
        // is placed where water stands or a channel runs, so the record the
        // carve left stays true.
        let mut natural = 1.0_f64;
        let mut gravel = false;
        // A road's carriageway or a trail's worn line takes the point by
        // how developed it is, but not where water stands over it (a ford's
        // bed keeps the river's gravel).
        let mut paved = None;
        if let Some((developed, class)) = way
            && developed > 0.0
            && !under
        {
            natural = 1.0 - developed;
            paved = self.way_layers[match class {
                strata_data::RoadClass::Service => 0,
                strata_data::RoadClass::Track => 1,
                strata_data::RoadClass::Trail => 2,
            }];
        }
        if let Some(pad) = pad
            && let Some(shaped) = pad.shape(x, z, height, (gx, gz))
        {
            height = shaped.height;
            (gx, gz) = shaped.gradient;
            natural = natural.min(1.0 - shaped.developed);
            gravel = shaped.laid > crate::sites::gravel_threshold(x, z);
        }
        // Stage 6: a settlement's platform and road. One bounds test per
        // site for a sample away from them.
        let mut on_road = false;
        let mut laid = None;
        for (i, site) in self.sites.iter().enumerate() {
            if let Some(shaped) = site.shape(x, z, height, (gx, gz)) {
                height = shaped.height;
                (gx, gz) = shaped.gradient;
                natural = natural.min(shaped.natural);
                gravel |= shaped.natural < 1.0;
                on_road |= shaped.road;
                if let (Some(kind), Some(Some(layers))) = (shaped.ground, self.ground_layers.get(i))
                {
                    laid = Some(match kind {
                        crate::settlement::GroundKind::Lane => layers[0],
                        crate::settlement::GroundKind::Apron => layers[1],
                        crate::settlement::GroundKind::Bank => layers[2],
                    });
                }
            }
        }
        // `sqrt` is correctly rounded on every platform; `hypot` is not.
        let slope = (gx * gx + gz * gz).sqrt();
        let aspect = if slope > 1e-9 {
            [-gx / slope, -gz / slope]
        } else {
            [0.0, 0.0]
        };
        let material = match (laid, self.road_layer, paved, bed_layer) {
            (Some(layer), _, _, _) if !on_road => layer,
            (_, Some(road), _, _) if on_road || gravel => road,
            (_, _, Some(way), _) => way,
            (_, _, _, Some(bed)) => bed,
            _ if slope > first.slope_threshold => first.slope_material,
            _ => first.material,
        };

        // Hydration: four taps of the coarse relief a couple of hundred
        // metres out say whether the point is a hollow water collects in or
        // a ridge it runs off. The base elevation moves by centimetres over
        // this radius, so the relief alone says it.
        let hollow = ((ring - centre) / self.params.hydration_relief_scale_m).clamp(-1.0, 1.0);
        // The blended moisture rather than the raw rainfall: it is one march
        // per sample, and the noise share is what keeps a flat interior
        // from being one flat hydration too.
        // Water gathers at the bottom of a gully and runs off its ridges.
        let drainage = (-eroded.ridge).max(0.0) * self.params.drainage_gain;
        let hydration = if under {
            1.0
        } else {
            (climate.moisture + hollow * self.params.hydration_hollow_gain + drainage
                - slope * self.params.hydration_slope_loss)
                .clamp(0.0, 1.0)
        };

        // Cover: the hydration curve times a patch noise, so the biome says
        // what grows and the patches say where the clearings are.
        let patch =
            (0.5 + self.cover_patches.get(x, z) * self.params.cover_contrast).clamp(0.0, 1.0);
        // Nothing grows on a platform or a carriageway: cover follows how
        // much of the natural ground the site left.
        let mut cover =
            smoothstep(self.params.cover_dry, self.params.cover_wet, hydration) * patch * natural;
        // And the rock under it: badlands shed soil too fast for anything
        // to hold. By the provinces' weights, so it fades across a border.
        if let Some((_, rock_cover)) = provinces {
            cover *= rock_cover;
        }
        // Nothing grows under water.
        if under {
            cover = 0.0;
        }

        // f64 to f32 is the whole point of this boundary (ADR 0002).
        #[allow(clippy::cast_possible_truncation)]
        Sample {
            height: height as f32,
            biomes: [(first.id, w as f32), (other.id, (1.0 - w) as f32)],
            provinces: provinces.map_or(Sample::NO_PROVINCES, |(pair, _)| pair),
            material,
            moisture: climate.moisture as f32,
            hydration: hydration as f32,
            cover: cover as f32,
            landform: Landform {
                slope: slope as f32,
                aspect: aspect.map(|a| a as f32),
                hollow: hollow as f32,
                ridge: eroded.ridge as f32,
                developed: (1.0 - natural) as f32,
                bed: bed.map_or(0, |bed| {
                    // Beds are metres thick within tens of kilometres of
                    // height and dip: far inside an i32.
                    i32::try_from(bed.0.index).unwrap_or(0)
                }),
                bed_phase: bed.map_or(0.0, |bed| bed.0.phase as f32),
                bed_hard: bed.is_some_and(|bed| bed.0.hard),
                cut_m: road_cut as f32,
                cleared: cleared as f32,
            },
            water,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> ProceduralWorld {
        let registries =
            strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load");
        ProceduralWorld::new(Seed(42), &registries)
    }

    fn registries() -> WorldDefs {
        strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load")
    }

    /// `range_octaves` and `range_persistence` shape the range field; until
    /// ticket 0004's step 2b nothing read them.
    #[test]
    fn the_range_parameters_move_the_ranges() {
        let registries = registries();
        let base = ProceduralWorld::new(Seed(42), &registries);
        let params = *base.params();
        for changed in [
            WorldParams {
                range_octaves: params.range_octaves + 1,
                ..params
            },
            WorldParams {
                range_persistence: params.range_persistence * 0.5,
                ..params
            },
        ] {
            let other = ProceduralWorld::with_params(Seed(42), &registries, changed);
            // Over land within reach of a range in either world: since
            // ticket 0004's step 4 a range's lift is nothing further than
            // `range_width_m` from its crest line, and the ground there
            // stays where it was whatever the field does.
            let land: Vec<(f64, f64)> = (0..2_000)
                .map(|k| (f64::from(k) * 50.0 - 50_000.0, f64::from(k) * 31.0))
                .filter(|&(x, z)| {
                    base.continent(x, z).land > 0.02
                        && (base.continent(x, z).range > 0.0 || other.continent(x, z).range > 0.0)
                })
                .collect();
            let moved = land
                .iter()
                .filter(|&&(x, z)| {
                    (base.continent(x, z).range - other.continent(x, z).range).abs() > 1e-12
                })
                .count();
            assert!(land.len() > 200, "only {} land points", land.len());
            assert!(
                moved * 10 > land.len() * 9,
                "only {moved} of {} land points moved",
                land.len()
            );
        }
    }

    /// The range field's lattice sits where the seed puts it, so no world
    /// runs a range through its origin (simplex noise is zero at a lattice
    /// vertex).
    #[test]
    fn each_seed_puts_the_ranges_lattice_elsewhere() {
        let registries = registries();
        let offsets: Vec<(f64, f64)> = (0..8)
            .map(|seed| ProceduralWorld::new(Seed(seed), &registries).range_offset)
            .collect();
        for (i, a) in offsets.iter().enumerate() {
            assert!((0.0..100_000.0).contains(&a.0) && (0.0..100_000.0).contains(&a.1));
            for b in &offsets[i + 1..] {
                assert!(a != b, "two seeds share the offset {a:?}");
            }
        }
    }

    /// A range's lift is a bell across its line: nothing at the floor, all
    /// at the crest, round there (no slope, so no roof line) and flat at
    /// the foot, and its derivative is its slope.
    #[test]
    fn a_range_is_a_bell_on_a_long_foot() {
        let params = WorldParams::default();
        assert_eq!(
            range_lift(&params, params.range_floor, params.range_foot),
            (0.0, 0.0)
        );
        assert_eq!(
            range_lift(&params, params.range_crest, params.range_foot),
            (1.0, 0.0)
        );
        let mut previous = 0.0;
        for k in 1..100 {
            let fold = f64::from(k) / 100.0;
            let (lift, slope) = range_lift(&params, fold, params.range_foot);
            assert!(lift > previous, "not rising at {fold}");
            previous = lift;
            let h = 1e-6;
            let numeric = (range_lift(&params, fold + h, params.range_foot).0
                - range_lift(&params, fold - h, params.range_foot).0)
                / (2.0 * h);
            assert!(
                (numeric - slope).abs() < 1e-5,
                "{numeric} against {slope} at {fold}"
            );
        }
        // A tenth of the way in from the profile's edge, a thousandth of the
        // height: the foot. Half way, an eighth: most of the height is near
        // the crest.
        assert!(range_lift(&params, 0.1, params.range_foot).0 < 1.5e-3);
        assert!(range_lift(&params, 0.5, params.range_foot).0 < 0.13);
    }

    /// A range's own metres and its flank carry their gradients, the
    /// distance to the crest line's second-derivative term included:
    /// against central differences over half a metre on land within reach
    /// of a range, nearly everywhere (a kink, the floor of the field's
    /// gradient or the profile's edge, may sit inside a difference).
    #[test]
    fn a_ranges_own_metres_carry_their_slope() {
        let world = world();
        let mut rng = strata_core::Rng::new(9);
        let (mut agreeing, mut total) = (0_u32, 0_u32);
        while total < 3_000 {
            let x = rng.range_f64(-60_000.0, 60_000.0);
            let z = rng.range_f64(-60_000.0, 60_000.0);
            let (continent, slopes) = world.continent_with_gradient(x, z);
            if continent.range_m <= 1.0 || continent.flank <= 0.0 {
                continue;
            }
            total += 1;
            let h = 0.5;
            let check = |f: &dyn Fn(&Continent) -> f64, (gx, gz): (f64, f64)| {
                let fx =
                    (f(&world.continent(x + h, z)) - f(&world.continent(x - h, z))) / (2.0 * h);
                let fz =
                    (f(&world.continent(x, z + h)) - f(&world.continent(x, z - h))) / (2.0 * h);
                (gx - fx).hypot(gz - fz) <= 1e-3 * fx.hypot(fz).max(1e-6)
            };
            if check(&|c| c.range_m, slopes.range_m) && check(&|c| c.flank, slopes.flank) {
                agreeing += 1;
            }
        }
        assert!(agreeing * 100 >= total * 99, "{agreeing} of {total} agree");
    }

    /// The distance to a crest line is the first-order distance: exact for
    /// a field that changes linearly across its line.
    #[test]
    fn the_distance_to_a_straight_crest_line_is_exact() {
        // r = 0.001 (x - 300): the line x = 300, a thousandth a metre.
        for x in [-500.0, 0.0, 250.0, 300.0, 1_000.0, 4_300.0] {
            let (d, (ddx, ddz)) = crest_distance(&RangeFrame {
                ridge: 0.001 * (x - 300.0),
                gradient: (0.001, 0.0),
                scale: 0.001,
                scale_gradient: (0.0, 0.0),
            });
            assert!((d - (x - 300.0_f64).abs()).abs() < 1e-9, "{d} at {x}");
            if (x - 300.0_f64).abs() > 1.0 {
                assert!((ddx.abs() - 1.0).abs() < 1e-12 && ddz == 0.0);
            }
        }
        // Rounded at the line, falling about linearly beyond the rounding.
        let (width, round) = (10_000.0, 400.0);
        assert!((across_profile(0.0, width, round).0 - 1.0).abs() < 1e-12);
        assert!(across_profile(0.0, width, round).1.abs() < 1e-12);
        // Beyond the rounding the edge is about `round` further out than
        // `width` from the line.
        assert!((across_profile(5_000.0, width, round).0 - 0.54).abs() < 0.005);
        assert!(across_profile(10_500.0, width, round).0 < 0.0);
    }

    /// The terms of a point add up to its natural ground, to the bit, and
    /// the biomes' shares to one.
    #[test]
    fn the_terms_are_the_sample() {
        let world = world();
        let mut rng = strata_core::Rng::new(5);
        for _ in 0..2_000 {
            let x = rng.range_f64(-60_000.0, 60_000.0);
            let z = rng.range_f64(-60_000.0, 60_000.0);
            let t = world.terms(x, z);
            let natural = world.natural(x, z, |_, _, _, _| {});
            assert_eq!(t.height_m.to_bits(), natural.height.to_bits());
            let sum =
                t.continent.base_m + t.continent.range_m + t.relief_m + t.beds_m + t.erosion_m;
            assert!(
                (sum - t.height_m).abs() < 1e-9,
                "{sum} against {}",
                t.height_m
            );
            let shares: f64 = t.biomes.iter().map(|b| b.1).sum();
            assert!((shares - 1.0).abs() < 1e-12);
            // Away from a settlement the sample before the rivers' carve is
            // the natural ground (`tests/carve.rs` holds the carve's own).
            if world.sites().iter().all(|site| !site.touches(x, z)) {
                #[allow(clippy::cast_possible_truncation, reason = "a sample's f32")]
                let height = t.height_m as f32;
                assert_eq!(
                    world.sample_with(x, z, Ground::Natural).height.to_bits(),
                    height.to_bits()
                );
            }
        }
    }

    /// The coastal fade is nothing at the land's edge, all from
    /// `range_coast`, and rises all the way; its top three tenths, where
    /// the mountains' relief switches on, take at least three times the
    /// field that the land mask's ramp (a smoothstep from 0.02 to 0.15,
    /// which faded the ranges until ticket 0004's review of step 2b) took.
    /// At 0.1 of field a kilometre, typical by a coast, 1.8 km where it was
    /// 0.5.
    #[test]
    fn a_range_fades_to_the_coast_gently_at_its_top() {
        let params = WorldParams::default();
        assert_eq!(range_coast(&params, 0.02), (0.0, 0.0));
        assert_eq!(range_coast(&params, params.range_coast), (1.0, 0.0));
        let span = params.range_coast - 0.02;
        let mut previous = 0.0;
        for k in 1..100 {
            let field = 0.02 + span * f64::from(k) / 100.0;
            let (fade, slope) = range_coast(&params, field);
            assert!(fade > previous, "not rising at {field}");
            previous = fade;
            let h = 1e-7;
            let numeric =
                (range_coast(&params, field + h).0 - range_coast(&params, field - h).0) / (2.0 * h);
            assert!(
                (numeric - slope).abs() < 1e-4 * slope.max(1.0),
                "{numeric} against {slope} at {field}"
            );
        }
        let top_span = |fade: &dyn Fn(f64) -> f64, to: f64| {
            // The lowest field, on a fine scan, from which the fade is over
            // 0.7, to where it is whole.
            let from = (0..=100_000)
                .map(|k| 0.02 + (to - 0.02) * f64::from(k) / 100_000.0)
                .find(|&f| fade(f) > 0.7)
                .expect("the fade passes 0.7");
            to - from
        };
        let eased = top_span(&|f| range_coast(&params, f).0, params.range_coast);
        let mask = top_span(&|f| smoothstep(0.02, 0.15, f), 0.15);
        assert!(eased > 3.0 * mask, "{eased} against the land mask's {mask}");
    }

    #[test]
    fn the_continent_with_its_gradient_is_the_same_continent() {
        let world = world();
        let mut rng = strata_core::Rng::new(3);
        for _ in 0..20_000 {
            let x = rng.range_f64(-60_000.0, 60_000.0);
            let z = rng.range_f64(-60_000.0, 60_000.0);
            assert_eq!(
                world.continent(x, z),
                world.continent_with_gradient(x, z).0,
                "at ({x}, {z})"
            );
        }
    }

    /// The analytic gradient against central differences over half a
    /// metre, on land. A point where the difference straddles a kink (a
    /// clamp, a smoothstep's end, the ridge's crease) may disagree, so the
    /// test asks it of nearly every point, not every one.
    #[test]
    fn the_elevation_gradient_is_the_elevations_slope() {
        let world = world();
        let mut rng = strata_core::Rng::new(4);
        let (mut agreeing, mut total) = (0_u32, 0_u32);
        let mut worst = (0.0_f64, 0.0, 0.0);
        while total < 5_000 {
            let x = rng.range_f64(-60_000.0, 60_000.0);
            let z = rng.range_f64(-60_000.0, 60_000.0);
            let (continent, slopes) = world.continent_with_gradient(x, z);
            let (gx, gz) = slopes.elevation;
            if continent.elevation <= 0.5 || continent.elevation >= 1.0 {
                continue;
            }
            let h = 0.5;
            let e = |x, z| world.continent(x, z).elevation;
            let fx = (e(x + h, z) - e(x - h, z)) / (2.0 * h);
            let fz = (e(x, z + h) - e(x, z - h)) / (2.0 * h);
            let error = (gx - fx).hypot(gz - fz);
            let scale = fx.hypot(fz).max(1e-7);
            total += 1;
            if error <= 1e-3 * scale {
                agreeing += 1;
            } else if error / scale > worst.0 {
                worst = (error / scale, x, z);
            }
        }
        assert!(
            agreeing * 1000 >= total * 995,
            "{agreeing} of {total} land points agree; worst relative error {:.3} at ({:.0}, {:.0})",
            worst.0,
            worst.1,
            worst.2
        );
    }
}
