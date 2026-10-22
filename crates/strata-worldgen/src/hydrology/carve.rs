//! The carve (ADR 0010 decision 3, ticket 0005 step 2): the last height
//! stage of a sample, from the network round the point.
//!
//! - **A lake** levels the ground under it to the eased coarse ground of its
//!   nodes (a smooth basin whose rim is the level), and keeps a ring of that
//!   basin round it (a metre of coarse height and a fifth of the band over
//!   the level), so no water leaks past its shore; the ground blends back
//!   over [`LAKE_BAND_M`] from the shore.
//! - **A river** takes a channel, flat-bottomed with banks eased up to a
//!   freeboard over its surface, and a valley out to a half-width that grows
//!   with the square root of discharge, whose floor and walls are the
//!   provinces' section blended by their weights. The channel replaces the
//!   ground whatever it was (cut or fill), so the bed never rises
//!   downstream; out to a band past the bank the ground is raised to the
//!   freeboard where it was lower, so the water stays in; the valley only
//!   cuts, easing ground above its profile down to it and blending to
//!   nothing at its half-width. The fill stops over the sea and in a lake.
//!
//! Cut and fill share one smooth ramp, `sp`, for which `sp(x) - sp(-x) =
//! x`: where both weigh one and aim at the same profile the ground is that
//! profile exactly, and everywhere else every term is continuous with its
//! gradient. Every number is carried with its gradient
//! ([`dual`](super::dual)), so the gradient a sample carries is the carved
//! height's own, the discharge, the cut, the order and the grain changing
//! along a reach included; only the provinces' blend of sections and a
//! lake's coarse slope are held fixed at the point (they change over
//! kilometres and the lattice's spacing).

use strata_data::WaterSection;

use super::dual::{D, smooth, smooth_c, soft};
use super::index::{Blend, Found, LakeHere, Near, RiverHere, SurfaceReading, lake_water};
use super::{Cell, RiverParams, Water, WaterKind, single};

/// What a sample reads from the network at its point.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Gathered {
    pub river: Option<RiverHere>,
    pub lake: Option<(LakeHere, Water)>,
}

impl Cell {
    /// The rivers and the lake round `(x, z)`. The rivers are blended only
    /// where a valley can reach: a point farther from every segment than
    /// any of their valleys reaches ([`Run::valley_m`]) is not carved by
    /// them, whatever the blend says, and pays for one pass over its
    /// bucket's segments.
    pub(crate) fn gather(&self, params: &RiverParams, x: f64, z: f64) -> Gathered {
        gather(&self.runs, &self.lakes, params, x, z)
    }
}

/// [`Cell::gather`] over any handles to the networks: a cell's shared ones,
/// or the borrowed one a bake places its sites through, which no thread then
/// counts in and out of an `Arc` per sample.
pub(crate) fn gather<N: std::borrow::Borrow<super::Network>>(
    runs: &[(N, super::index::Run)],
    lakes: &[N],
    params: &RiverParams,
    x: f64,
    z: f64,
) -> Gathered {
    let mut gathered = Gathered::default();
    if !runs.is_empty() {
        SCRATCH.with(|scratch| {
            let (found, readings) = &mut *scratch.borrow_mut();
            found.clear();
            let mut near = Near::default();
            let mut valley = 0.0_f64;
            for (k, (network, run)) in runs.iter().enumerate() {
                let tag = u16::try_from(k).expect("a few networks reach a cell");
                network
                    .borrow()
                    .index
                    .scan(*run, (x, z), params.reach_m, &mut near, tag, found);
                valley = valley.max(f64::from(run.valley_m));
            }
            if near.min_c2 >= valley * valley {
                return;
            }
            readings.clear();
            let mut blend = Blend::default();
            for one in found.iter() {
                runs[usize::from(one.network)].0.borrow().index.blend(
                    one,
                    params.reach_m,
                    (x, z),
                    near,
                    &mut blend,
                    readings,
                );
            }
            gathered.river = blend.finish(readings, params);
        });
    }
    for network in lakes {
        let network = network.borrow();
        if let Some(here) = network.shore.lake_here(&network.lakes, x, z)
            && gathered
                .lake
                .is_none_or(|(held, _)| here.depth() > held.depth())
        {
            let water = lake_water(&network.lakes[here.id as usize], &here);
            gathered.lake = Some((here, water));
        }
    }
    gathered
}

thread_local! {
    /// The segments a sample's scan read, and the surfaces its blend read,
    /// kept for the soft minimum: one pair of buffers a thread, so a sample
    /// allocates nothing.
    static SCRATCH: std::cell::RefCell<(Vec<Found>, Vec<SurfaceReading>)> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// How water cuts a province, as numbers: each [`WaterSection`]'s, blended
/// by the provinces' weights at a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// The valley floor's half-width past the bank, metres.
    pub floor_m: f64,
    /// What the floor's half-width gains per metre of channel width.
    pub floor_per_width: f64,
    /// Over how many metres a wall eases up from the floor.
    pub ease_m: f64,
    /// The wall's rise over run where the valley is shallow: a valley whose
    /// cut is deeper steepens its walls so they reach the ground by six
    /// tenths of its half-width, up to [`steepest`](Self::steepest).
    pub wall: f64,
    /// The steepest the rock stands a valley's wall: a cut deeper than its
    /// half-width allows at this widens the valley instead, up to
    /// [`RiverParams::valley_max_m`].
    pub steepest: f64,
    /// The valley's half-width as a share of the world's.
    pub width: f64,
    /// A tributary's cut as a share of its main stem's (the province's
    /// `hang`): a first-order stream's valley is this wide of what its
    /// discharge gives, a third-order one's whole.
    pub hang: f64,
    /// How much the rock's grain steers its valleys (the province's
    /// `along_grain`): a river across the fold axis cuts a narrower valley,
    /// a water gap, by up to three quarters of this, so the ridges a
    /// folded belt runs in stay long.
    pub grain: f64,
    /// How much a deep cut wall takes the rock's beds' steps, `0..=1`
    /// (ticket 0005, step 3b): a stepped canyon's whole, a slot's half,
    /// none in the soft rock of a V, a trough or a gully, whose walls stay
    /// open slopes.
    pub steps: f64,
}

/// The most a valley across the grain narrows, as a share of its
/// half-width, where the rock's grain is whole.
pub(crate) const GRAIN_NARROWING: f64 = 0.75;

/// The length of the averaged direction downstream under which the grain's
/// narrowing stops reading its direction ([`RiverHere::course`]).
const COURSE_SOFT: f64 = 0.3;

impl Section {
    /// A V, the section a world without geology takes.
    pub const V: Self = Self {
        floor_m: 4.0,
        floor_per_width: 0.5,
        ease_m: 4.0,
        wall: 0.08,
        steepest: 0.8,
        width: 1.0,
        hang: 1.0,
        grain: 0.0,
        steps: 0.0,
    };

    /// The numbers of a section, with a province's `hang` and
    /// `along_grain`. A trough is a
    /// wide floor whose walls ease up over tens of metres (a U); a stepped
    /// canyon a modest floor and steep walls, which the beds already
    /// standing in the ground step; a slot a floor barely wider than its
    /// water between sheer walls; a gully a small V.
    #[must_use]
    pub fn of(section: WaterSection, hang: f64, grain: f64) -> Self {
        let base = match section {
            WaterSection::V => Self::V,
            WaterSection::Trough => Self {
                floor_m: 20.0,
                floor_per_width: 3.0,
                ease_m: 20.0,
                wall: 0.05,
                steepest: 0.9,
                width: 1.25,
                ..Self::V
            },
            WaterSection::Stepped => Self {
                floor_m: 8.0,
                floor_per_width: 1.0,
                ease_m: 6.0,
                wall: 0.15,
                steepest: 1.4,
                width: 1.0,
                steps: 1.0,
                ..Self::V
            },
            WaterSection::Slot => Self {
                floor_m: 2.0,
                floor_per_width: 0.2,
                ease_m: 2.0,
                wall: 1.0,
                steepest: 5.0,
                width: 0.55,
                steps: 0.5,
                ..Self::V
            },
            WaterSection::Gully => Self {
                floor_m: 3.0,
                floor_per_width: 0.5,
                ease_m: 3.0,
                wall: 0.2,
                steepest: 1.2,
                width: 0.7,
                ..Self::V
            },
        };
        Self {
            hang: hang.clamp(0.0, 1.0),
            grain: grain.clamp(0.0, 1.0),
            ..base
        }
    }

    /// Nothing, to add sections to by weight.
    pub(crate) const ZERO: Self = Self {
        floor_m: 0.0,
        floor_per_width: 0.0,
        ease_m: 0.0,
        wall: 0.0,
        steepest: 0.0,
        width: 0.0,
        hang: 0.0,
        grain: 0.0,
        steps: 0.0,
    };

    /// `self + other * weight`, field by field.
    #[must_use]
    pub(crate) fn add(self, other: &Self, weight: f64) -> Self {
        Self {
            floor_m: self.floor_m + other.floor_m * weight,
            floor_per_width: self.floor_per_width + other.floor_per_width * weight,
            ease_m: self.ease_m + other.ease_m * weight,
            wall: self.wall + other.wall * weight,
            steepest: self.steepest + other.steepest * weight,
            width: self.width + other.width * weight,
            hang: self.hang + other.hang * weight,
            grain: self.grain + other.grain * weight,
            steps: self.steps + other.steps * weight,
        }
    }

    /// `self / total`, field by field.
    #[must_use]
    pub(crate) fn scaled(self, k: f64) -> Self {
        Self {
            floor_m: self.floor_m * k,
            floor_per_width: self.floor_per_width * k,
            ease_m: self.ease_m * k,
            wall: self.wall * k,
            steepest: self.steepest * k,
            width: self.width * k,
            hang: self.hang * k,
            grain: self.grain * k,
            steps: self.steps * k,
        }
    }
}

/// What the bed under water is laid in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bed {
    /// A channel's or a fall's: gravel.
    Gravel,
    /// A lake's and a mouth's: sand.
    Sand,
}

/// The carve's answer at a point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Carved {
    pub height: f64,
    pub gradient: (f64, f64),
    /// The water the point is in or by, for the sample.
    pub water: Option<Water>,
    /// Whether the ground is under the water's surface.
    pub under: bool,
    /// The bed's material where the ground is under water or on a wet bank.
    pub bed: Option<Bed>,
}

/// How far from a lake's shore its bed's blend back to the natural ground
/// reaches, metres; the bed is whole a quarter of the way out.
const LAKE_BAND_M: f64 = 80.0;

/// Over how much coarse height over its level a lake's blend is shut,
/// metres: under the lift a node not of the lake is read at
/// (`index::SHORE_LIFT_M`), so it ends inside the cells a node of the lake
/// stands at the corner of.
pub(crate) const SHORE_GATE_M: f64 = 10.0;

/// Over what depth of cut a river's carve fades out, metres. The network
/// is routed on the coarse field, which leaves out the provinces' shapes
/// and the beds; where those raise a range's spur hundreds of metres over
/// the field, a creek routed across it would be held at the level before it
/// and cut a slot up to 1 700 m deep (seed 42, 57 km south-west of Port
/// Serein; one vertex in a hundred within 100 km is cut over 200 m). No
/// river cuts that: its valley fades out instead, and the creek runs into
/// the hillside and out again on the far side, as one sinking into karst
/// would. Routing on the ground those provinces shape is the fix.
const DEEPEST_M: (f64, f64) = (150.0, 250.0);

/// The softness of the cut and the fill's ramp, metres.
const RAMP_M: f64 = 0.5;

/// How long a valley wall's wander is, metres: a few times a creek
/// valley's half-width, so its edge bends in and out along it.
const WALL_WANDER_M: f64 = 90.0;

/// The most a valley wall's foot moves in towards its river, metres.
const WALL_SHIFT_MOST_M: f64 = 14.0;

/// How much steeper a valley wall stands where its wander says, as a share.
const WALL_STEEPEN: f64 = 0.35;

const WALL_SALT: u64 = 0x7761_6c6c_7761_6e64;

/// The channel's numbers for a discharge: width, depth, where the bank's
/// ease starts and ends, and how much it rises; each with its gradient.
struct Channel {
    width: D,
    depth: D,
    bank_foot: D,
    bank_top: D,
    rise: D,
}

impl Channel {
    /// The channel of `discharge` whose width is `riffle` times its mean
    /// (`shape::riffle`) and its depth the mean's over that, so its section
    /// keeps its area: a riffle between two bends wide and shallow, a
    /// bend's pool narrow and deep.
    fn of(params: &RiverParams, discharge: D, riffle: D) -> Self {
        Self::pair(params, discharge, riffle).0
    }

    /// The channel of `discharge` at a riffle of `riffle`, and the mean
    /// one, from one reading of the hydraulic geometry.
    fn pair(params: &RiverParams, discharge: D, riffle: D) -> (Self, Self) {
        // `RiverParams::width` and `depth`, as numbers with gradients.
        let q = discharge.at_least(0.0);
        let width = q.sqrt() * params.width_coefficient;
        let depth = (q * q * q).sqrt().sqrt().sqrt() * params.depth_coefficient;
        let riffle = riffle.at_least(0.1);
        (
            Self::shaped(params, width * riffle, depth / riffle),
            Self::shaped(params, width, depth),
        )
    }

    /// The channel `width` wide and `depth` deep.
    fn shaped(params: &RiverParams, width: D, depth: D) -> Self {
        // `RiverParams::bank_top_m`, its two larger-ofs softened over
        // `BANK_SOFT_M`: a riffle's width and depth swing them along the
        // channel, and a hard switch creased the bank at every swing.
        let rise = depth + params.freeboard_m;
        let k = D::c(super::BANK_SOFT_M);
        let run = soft(rise * params.bank_run - 2.0, k) + 2.0;
        let bank_top = soft(width * 0.5 - run * 0.6, k) + run;
        Self {
            width,
            depth,
            bank_foot: bank_top - run,
            bank_top,
            rise,
        }
    }
}

/// The section's numbers no province's exceed: the widest share, floor and
/// floor per width, and the gentlest steepest wall.
fn widest_section() -> Section {
    WaterSection::ALL
        .iter()
        .map(|&kind| Section::of(kind, 1.0, 0.0))
        .fold(Section::of(WaterSection::V, 1.0, 0.0), |held, s| Section {
            floor_m: held.floor_m.max(s.floor_m),
            floor_per_width: held.floor_per_width.max(s.floor_per_width),
            width: held.width.max(s.width),
            steepest: held.steepest.min(s.steepest),
            ..held
        })
}

/// The farthest from its line a river of at most `discharge`, `cut` and
/// meander `amplitude` carves anything, in any province, metres:
/// [`carve`]'s half-width with every term at its largest (a riffle's width
/// too), and a metre over.
pub(crate) fn valley_bound(params: &RiverParams, discharge: f64, cut: f64, amplitude: f64) -> f64 {
    let s = widest_section();
    let channel = Channel::of(params, D::c(discharge), D::c(1.0 + params.riffle_share));
    let (width, top) = (channel.width.v, channel.bank_top.v);
    let amplitude = amplitude.max(0.0);
    let base = top + s.floor_m + s.floor_per_width * width + amplitude;
    let half = ((params.valley_base_m + params.valley_per_root_m * discharge.max(0.0).sqrt())
        * s.width)
        .max((base + cut.max(0.0) / s.steepest.max(0.05)) / 0.6)
        .min(params.valley_max_m)
        .max(top + 2.0 * width.max(4.0) + 10.0 + amplitude);
    half + 1.0
}

/// How the shore of a lake runs (see [`carve`]): the slope its bed falls
/// at off the coarse shore at the least, rise over run, and outside it the
/// slope its rim rises at.
const SHORE_SLOPE: f64 = 0.04;

/// The steepest the bed falls inside the coarse shore where the fine
/// ground would drop faster, rise over run.
const SHORE_DROP: f64 = 0.2;

/// Metres of height the shore's strip spans: the fine ground decides the
/// shore where it lies within this over the slope off the coarse shore, so
/// the shore wanders up to this over [`SHORE_SLOPE`] (150 m) inside it.
const SHORE_SPAN_M: f64 = 6.0;

/// The softness of the bounds a lake's bed is held between, metres: a
/// shore that meets one bends over this much height.
const SHORE_RAMP_M: f64 = 1.5;

/// The least depth of a lake's water past the shore's strip, metres.
const LAKE_FLOOR_M: f64 = 2.0;

/// Over how far outside a lake's coarse shore the floor that keeps dry
/// hollows out of its rim comes in, metres.
const HOLLOW_OUT_M: f64 = 10.0;

/// How far inside a lake's coarse shore, and outside it, no river's valley
/// may lower the ground under the lake's level, metres: inside it is the
/// lake's own bed, which a river entering or leaving may deepen; outside,
/// over [`RIM_OUT_M`] and fading out over [`RIM_FADE_M`] more, the rim
/// that keeps the water in. The reach leaving a lake is held at its level
/// past both (`rivers::LAKE_HOLD_M`).
const RIM_IN_M: f64 = 20.0;
pub(crate) const RIM_OUT_M: f64 = 100.0;
pub(crate) const RIM_FADE_M: f64 = 40.0;

/// How far over a lake's level a river's valley may leave its rim,
/// metres: the valley floor's own freeboard.
const RIM_M: f64 = 0.3;

/// The softness of the rim's give and take, metres: where what the river
/// took and what the rim wants back cross.
const RIM_SOFT_M: f64 = 0.1;

/// How much wider than its bank's band a channel's gravel reaches on a
/// bend's inside, in bands at a full bend: the bar.
const BAR_BANDS: f64 = 2.5;

/// The cut over which a wall in bedded rock takes its beds' steps, metres:
/// a shallow valley's wall is a slope, a gorge's a stair of benches and
/// risers.
const STEPS_DEEP_M: (f64, f64) = (15.0, 45.0);

/// How high a wall must have risen over its floor for its steps to show
/// whole, metres: the floor and the wall's foot stay as they are.
const STEPS_RISEN_M: f64 = 4.0;

/// The provinces' beds, for the steps a deep cut wall in their rock
/// takes: the provinces weighing at the point, each province's beds and
/// section by its id, the point (whose dip fields are read only where a
/// wall takes steps, noises every carved point would otherwise pay for)
/// and the world's bed numbers. Each province's steps are added by its
/// weight, as the natural ground's beds are, so a wall does not step
/// where the heaviest province changes.
pub(crate) struct Steps<'a> {
    pub provinces: &'a [(strata_data::ProvinceId, f64)],
    pub beds: &'a [Option<crate::strata::Beds>],
    pub sections: &'a [Section],
    pub at: (f64, f64),
    pub params: &'a crate::params::WorldParams,
}

impl Steps<'_> {
    /// What the provinces' beds make of a wall whose height is `cut` and
    /// whose face stands at `steep`, rise over run: each bedded province's
    /// offset, by its weight and its section's [`Section::steps`].
    ///
    /// The beds fade on a face over twice as steep as they fade on natural
    /// ground (`WorldParams::bed_fade`), so a gorge's wall takes its
    /// benches and no riser on it stands over a few in one. The fade reads
    /// the face's own slope, which is continuous, and not the cut's
    /// gradient: that jumps wherever a clamp in the valley's half-width or
    /// its wall's slope switches, and a fade that jumped stepped the wall
    /// by up to a bed, 6 m in a centimetre (ticket 0005, the review of step
    /// 3b). The fade is carried with its gradient, so the offset's is its
    /// own.
    fn offset(&self, cut: D, steep: D) -> D {
        let total: f64 = self.provinces.iter().map(|&(_, w)| w).sum();
        let mut sum = D::c(0.0);
        if total <= 0.0 {
            return sum;
        }
        let fade = 1.0
            - smooth_c(
                2.0 * self.params.bed_fade.0,
                2.0 * self.params.bed_fade.1,
                steep,
            );
        if fade.v <= 0.0 {
            return sum;
        }
        for &(id, w) in self.provinces {
            let i = id.0 as usize;
            let (Some(Some(beds)), Some(section)) = (self.beds.get(i), self.sections.get(i)) else {
                continue;
            };
            let share = w / total * section.steps.clamp(0.0, 1.0);
            if share <= 0.0 {
                continue;
            }
            // No coarse slope: the face's fade is this function's own,
            // above; only the dip's slope is left to `apply`'s, and it is
            // continuous.
            let bedded = beds.apply(
                beds.dip(self.at.0, self.at.1),
                cut.v,
                (cut.g[0], cut.g[1]),
                (1.0, (0.0, 0.0)),
                self.params,
            );
            sum = sum + D::new(bedded.offset, [bedded.gradient.0, bedded.gradient.1]) * share;
        }
        sum * fade
    }
}

/// The ground at a point whose natural ground is `height` with `gradient`,
/// carved by the water `here` with the provinces' `section`, the fold axis
/// (the ranges' direction) being `axis`. `coarse` is the natural ground's
/// coarse part at the point (the continent and the relief's first
/// octaves): what a lake's bed takes the fine ground's shape from. `steps`
/// are the beds of the provinces at the point, each by its weight, where
/// any of their sections steps a wall ([`Steps`]). Every number is
/// carried with its gradient ([`D`]), so the carved gradient is the carved
/// height's.
///
/// - **A lake.** Its bed is the fine ground's shape (the natural ground
///   less its coarse part and its mean over the lake) set on the coarse
///   basin, held between two bounds off the coarse shore: under the level
///   past a strip [`SHORE_SPAN_M`] over [`SHORE_SLOPE`] wide inside the
///   shore, over it everywhere outside, rising outward at that slope. So
///   the shore follows the fine ground's bays and spurs within the strip,
///   and the water never stands over a lower ground outside it. Blended
///   back to the natural ground beyond the rim.
/// - **A river.** Its channel winds about the reach's line by its meanders
///   (`shape`), so the channel's profile reads the distance from the
///   meandering centreline and is as wide as the riffle there makes it;
///   the valley's floor makes room for the meanders' belt and its walls
///   stand off the line. A deep wall in bedded rock steps by its beds.
/// - **The rim.** Last, no river's valley lowers a lake's rim under its
///   level but the channel of a river at the lake's level (its outlet,
///   held at the level past the rim, and its inlets).
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "the lake, then the channel and the valley, then the rim"
)]
#[allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "the profile's own letters: height, distance, surface"
)]
pub(crate) fn carve(
    params: &RiverParams,
    height: f64,
    gradient: (f64, f64),
    coarse: f64,
    here: &Gathered,
    section: &Section,
    axis: (f64, f64),
    steps: Option<&Steps>,
    at: (f64, f64),
) -> Carved {
    let mut h = D::new(height, [gradient.0, gradient.1]);
    let mut in_lake = D::c(0.0);
    let mut stream = None;
    let mut rim = None;
    if let Some((lake, _)) = here.lake {
        let ground = D::new(lake.ground, [lake.gradient.0, lake.gradient.1]);
        let depth = lake.level - ground;
        // Metres inside the coarse shore: the depth over the eased
        // ground's slope, held fixed at the point (it changes over the
        // lattice's spacing).
        let inside = depth / lake.slope();
        // The blend back to the natural ground runs over a band of that
        // distance outside the shore, wider where the natural ground stands
        // far off the level, so the rim meets it in a slope and not a
        // cliff; and it is shut by a metre of coarse height over the level,
        // which every cell without a node of the lake stands above, so it
        // ends where the lake's cells do.
        // Whole to half the gate's height: a rim narrower than that, over
        // ground far under the level, held the water on a crest a few
        // metres wide.
        let gate = smooth_c(-SHORE_GATE_M, -SHORE_GATE_M * 0.5, depth);
        let off = h - lake.level;
        let reach = (off * off + 1.0).sqrt() * 4.0 + LAKE_BAND_M;
        let band = smooth(-reach, reach * -0.25, inside);
        let keep = gate * band;
        if keep.v > 0.0 {
            // The fine ground's shape on the coarse basin: its gradient the
            // natural ground's, the coarse part's and the basin's being
            // alike over the lattice's spacing.
            let fine = D::new(height - coarse - lake.detail + lake.ground, h.g);
            let lo = lake.level
                - inside * SHORE_SLOPE
                - soft(inside - 5.0, D::c(5.0)) * (SHORE_DROP - SHORE_SLOPE);
            let hi = soft(
                D::c(SHORE_SPAN_M + LAKE_FLOOR_M) - inside * SHORE_SLOPE,
                D::c(1.0),
            ) + (lake.level - LAKE_FLOOR_M);
            let ramp = D::c(SHORE_RAMP_M);
            let bed = fine + soft(lo - fine, ramp) - soft(fine - hi, ramp);
            h = h + (bed - h) * keep;
        }
        // No dry hollow under the level beside the water: outside the
        // coarse shore, out to twice the blend's band, a dip in the natural
        // ground that the blend gives back under the level is raised to the
        // rim's slope. Water stands only inside the coarse shore, so such a
        // dip was a dry crater beside the lake whose rim, at the level,
        // drew as a ring of beach (the review of step 3b). Only a shallow
        // dip: ground falling away far under the level is a perched lake's
        // outer slope, which the rim already stands over.
        let outside = smooth_c(0.0, HOLLOW_OUT_M, -inside);
        if outside.v > 0.0 {
            let natural = D::new(height, [gradient.0, gradient.1]);
            let shallow = 1.0 - smooth_c(SHORE_SPAN_M, 2.0 * SHORE_SPAN_M, lake.level - natural);
            let near = smooth(reach * -2.0, -reach, inside);
            let w = gate * outside * shallow * near;
            if w.v > 0.0 {
                let floor = soft(-inside, D::c(HOLLOW_OUT_M)) * SHORE_SLOPE + (lake.level + RIM_M);
                h = h + soft(floor - h, D::c(RAMP_M)) * w;
            }
        }
        in_lake = smooth_c(-0.5, 0.5, depth);
        rim = Some((lake.level, inside, gate, h));
    }
    let mut channel_here = None;
    if let Some(river) = here.river {
        // The channel as the riffle or the pool here makes it; the valley
        // is the mean channel's, so its walls do not swing in and out with
        // every riffle.
        let (channel, mean) = Channel::pair(params, river.discharge, river.riffle);
        // The valley's distance is from the reach's line, the channel's
        // from its meandering centreline.
        let d = river.distance;
        let cd = river.channel;
        let a = river.amplitude.at_least(0.0);
        let s = river.surface;
        // How far the valley reaches: by discharge and section, and wider
        // where its cut is deeper than its rock's steepest wall allows.
        let hang = smooth_c(1.0, 3.0, river.order) * (1.0 - section.hang) + section.hang;
        let band = channel.width.at_least(4.0);
        let floor = mean.width * section.floor_per_width + section.floor_m;
        // The floor makes room for the meanders' belt either side of the
        // line.
        let base = mean.bank_top + floor + a;
        let deep = river.cut.at_least(0.0);
        // How square the flow runs to the fold axis, from the averaged
        // direction downstream: along one reach the unit's square, and
        // where reaches running different ways nearly cancel it eases to
        // across the grain instead of turning with the unit vector, which
        // there swings through a right angle in centimetres.
        let along = river.course[0] * axis.0 + river.course[1] * axis.1;
        let length = river.course[0] * river.course[0] + river.course[1] * river.course[1];
        let soft2 = COURSE_SOFT * COURSE_SOFT;
        let along2 = along * along * (1.0 + soft2) / (length + soft2);
        let grain = 1.0 - (1.0 - along2) * (GRAIN_NARROWING * section.grain);
        let half = ((river.discharge.at_least(0.0).sqrt() * params.valley_per_root_m
            + params.valley_base_m)
            * section.width
            * hang
            * grain)
            .max((base + deep / section.steepest.max(0.05)) / 0.6)
            .at_most(params.valley_max_m)
            .max(mean.bank_top + mean.width.at_least(4.0) * 2.0 + 10.0 + a);
        // The wall reaches the cut's depth by six tenths of the half-width.
        let wall_slope = (deep / (half * 0.6 - base).at_least(1.0))
            .at_least(section.wall)
            .at_most(section.steepest.max(section.wall));
        let ease = floor.at_most(section.ease_m);
        // The walls wander (ticket 0005, step 6): their foot and top move in
        // towards the river and back over tens of metres and their angle
        // steepens and eases, so neither edge is a line parallel to the
        // river and the wall reads as a hillside the river cut, not a
        // cutting. Only inwards and only steeper, so the valley never
        // reaches past the query's bound nor its wall past its fade.
        // Read only where it can reach a wall: on the floor short of the
        // wall's foot by more than the most it moves in, it moves nothing.
        let foot = (mean.bank_top + a + floor - ease).v.min(half.v * 0.55);
        let (wall_d, wall_slope) = if d.v + WALL_SHIFT_MOST_M > foot {
            let (shift, steepen) = super::grade::wander_pair(at.0, at.1, WALL_WANDER_M, WALL_SALT);
            // Held down by the rock's grain, as the sway is, and gone where
            // the grain is half or more: a folded belt's valleys run
            // straight along their strike (its elongation gate stands at
            // 2.0 and the wander took it under).
            let held = (1.0 - 2.0 * section.grain).clamp(0.0, 1.0);
            let inward = (floor * 0.35 + 4.0).at_most(WALL_SHIFT_MOST_M) * held;
            (
                d + (shift * 0.5 + 0.5) * inward,
                wall_slope * ((steepen * 0.5 + 0.5) * (WALL_STEEPEN * held) + 1.0),
            )
        } else {
            (d, wall_slope)
        };
        // The profile the cut aims at, and the one the fill does.
        let (cut, fill) = if cd.v < channel.bank_top.v {
            let t = smooth(channel.bank_foot.at_least(0.0), channel.bank_top, cd);
            let v = s - channel.depth + channel.rise * t;
            (v, v)
        } else {
            // The floor is level across the meanders' belt and rises gently
            // from its edge, measured from the line and not the channel, so
            // it does not swell and sink along the valley with every bend;
            // the walls stand off the line past the belt. Both rise only
            // once the channel's bank is behind (where the channel swings
            // out to the belt's edge, its bank meets the wall's foot).
            let behind = smooth(channel.bank_top, channel.bank_top + ease, cd);
            let away = d - mean.bank_top - a;
            let rise = soft(away - 4.0, D::c(4.0)) * behind;
            let wall = soft(wall_d - mean.bank_top - a - floor, ease) * behind;
            let top = s + params.freeboard_m;
            let risen = wall_slope * wall;
            let mut cut = top + rise * 0.03 + risen;
            // A deep wall in bedded rock: its beds' benches and risers,
            // where the wall has risen off its floor.
            if let Some(steps) = steps
                && section.steps > 0.0
                && risen.v > 0.0
            {
                let weight = smooth_c(STEPS_DEEP_M.0, STEPS_DEEP_M.1, deep)
                    * smooth_c(0.0, STEPS_RISEN_M, risen);
                if weight.v > 0.0 {
                    cut = cut + steps.offset(cut, wall_slope) * weight;
                }
            }
            (cut, top)
        };
        let valley = smooth(half * 0.55, half, wall_d);
        let bank = smooth(channel.bank_top + band, channel.bank_top + band * 2.0, cd);
        // Everything fades out over the last stretch of the query's reach,
        // and where the river would have to cut deeper than any river does
        // (see [`DEEPEST_M`]).
        let fade =
            (1.0 - smooth_c(
                params.reach_m - params.carve_fade_m,
                params.reach_m,
                river.nearest,
            )) * (1.0 - smooth_c(DEEPEST_M.0, DEEPEST_M.1, river.cut));
        let cut_w = (1.0 - valley) * fade;
        // No fill over the sea or in a lake: no causeway out to a mouth's
        // end or a reach's end in a lake.
        let sea = smooth_c(-1.0, 0.0, h);
        let fill_w = (1.0 - bank) * fade * sea * (1.0 - in_lake);
        if cut_w.v > 0.0 || fill_w.v > 0.0 {
            // The ramp softens out on the valley's walls, so a wall meets
            // the ground above it in a rounded shoulder and not an edge;
            // over the channel and its banks it is the one the fill shares.
            let shoulder = (deep * 0.3).at_least(RAMP_M).at_most(20.0);
            let edge = mean.bank_top + mean.width.at_least(4.0) * 2.0 + a;
            let soften = smooth(edge, edge + 30.0, d);
            let k = (shoulder - RAMP_M) * soften + RAMP_M;
            let over = soft(h - cut, k);
            let under = soft(fill - h, k);
            h = h - cut_w * over + fill_w * under;
        }
        channel_here = Some((cd, channel.bank_top, band, s));
        if d.v < half.v && fade.v > 0.0 {
            let depth = channel.depth.v;
            let water = Water {
                surface_m: single(s.v),
                bed_m: single(s.v - depth),
                depth_m: single(depth),
                width_m: single(channel.width.v),
                signed_distance_m: single(cd.v * river.channel_side),
                flow_dir: river.flow.map(single),
                speed_mps: single(params.speed(river.discharge.v)),
                discharge_m3s: single(river.discharge.v),
                order: river.nearest_order,
                kind: river.kind,
            };
            let bed = match river.kind {
                WaterKind::Mouth | WaterKind::Lake => Bed::Sand,
                WaterKind::Channel | WaterKind::Fall => Bed::Gravel,
            };
            // On a bend's inside the gravel reaches farther up the bank:
            // the bend's bar, over the water where the floor stands.
            let inner = (-river.channel_side * river.bend).clamp(0.0, 1.0);
            let bar = band.v * (1.0 + BAR_BANDS * inner);
            stream = Some((
                water,
                s.v,
                cd.v < channel.bank_top.v,
                cd.v < channel.bank_top.v + bar,
                bed,
            ));
        }
    }
    // The rim: what a river's valley cut out of it under the lake's level
    // is put back, but for the channel of a river at the level.
    if let Some((level, inside, gate, held)) = rim {
        let mut w = (1.0 - smooth_c(0.0, RIM_IN_M, inside))
            * smooth_c(-RIM_OUT_M - RIM_FADE_M, -RIM_OUT_M, inside)
            * gate;
        if let Some((cd, top, band, s)) = channel_here {
            let at_level = smooth_c(level - 0.5, level - 0.05, s);
            w = w * (1.0 - (1.0 - smooth(top, top + band, cd)) * at_level);
        }
        if w.v > 0.0 {
            // What the river took (never more, and nothing where it raised
            // the ground), and what the rim wants back (never more): each
            // bound a soft ramp, so the ground bends where one gives way to
            // the other and does not crease. A hard "nothing where it
            // raised" creased the lake's bed where a river's bank crossed
            // it (ticket 0005, the review of step 3b).
            let k = D::c(RIM_SOFT_M);
            let lowered = held - h;
            if lowered.v > -RIM_SOFT_M {
                let wanting = soft(D::c(level + RIM_M) - h, D::c(RAMP_M));
                let raise = soft(lowered, k) - soft(lowered - wanting, k);
                h = h + raise * w;
            }
        }
    }
    let mut out = Carved {
        height: h.v,
        gradient: (h.g[0], h.g[1]),
        water: None,
        under: false,
        bed: None,
    };
    // Which water the point is in, or by: a lake over it first, then a
    // river's valley, then a lake's shore.
    let lake = here
        .lake
        .filter(|(lake, _)| lake.depth() > 0.0 && h.v < lake.level);
    if let Some((_, water)) = lake {
        out.water = Some(water);
        out.under = true;
        out.bed = Some(Bed::Sand);
    } else if let Some((water, surface, channel, bank, bed)) = stream {
        // Ground under the surface outside the banks is dry: a floodplain
        // the banks keep the water off.
        out.water = Some(water);
        out.under = channel && h.v < surface;
        out.bed = (out.under || bank).then_some(bed);
    } else if let Some((lake, water)) = here.lake
        && h.v >= lake.level
    {
        // A lake's shore, over its level. Ground under the level outside
        // the coarse shore is no lake's water: the rim stands at the level
        // over every such point the lake's carve keeps, and past it the
        // ground the blend gives back is another hollow, dry.
        out.water = Some(water);
        out.bed = (lake.depth() > -0.5).then_some(Bed::Sand);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The water a body wades in reads the bank's top from
    /// `RiverParams::bank_top_m`; the carve cuts it with its own numbers
    /// carried with their gradients. The two must be the one bank.
    #[test]
    fn the_bank_top_is_the_carves() {
        let params = RiverParams::DEFAULT;
        for q in [0.0, 0.01, 0.1, 0.7, 2.0, 15.0, 120.0] {
            let channel = Channel::of(&params, D::c(q), D::c(1.0));
            let top = params.bank_top_m(params.width(q), params.depth(q));
            assert!(
                (channel.bank_top.v - top).abs() < 1e-9,
                "{q} m3/s: the carve's bank tops at {}, the query's at {top}",
                channel.bank_top.v
            );
        }
    }
}
