//! Beds: the hard and soft layers a province's rock lies in, and what they
//! make of the ground (ticket 0004, step 5).
//!
//! A slope cut through beds of unequal hardness does not stay a slope: a
//! hard bed stands as a cliff with a bench on its top, the soft bed over it
//! weathers back to a gentler slope, and where a hill's top lies inside a
//! hard bed the bed caps it flat, a mesa. So the relief is passed through a
//! function of height before the erosion filter cuts its gullies into it:
//!
//! - **The sequence.** Bed boundaries lie on a lattice of the province's
//!   mean thickness, each moved by a hash of its index by up to the share
//!   of that mean that keeps every bed inside the province's thickness
//!   range; a second hash says whether the bed is hard. A point's bed is a
//!   `floor` and three hashes, whatever its height.
//! - **The dip.** The beds are not level: the bed coordinate is the height
//!   plus a one-octave noise whose gradient is the dip, sized so that its
//!   typical slope is the province's middle dip ([`shape_slope`]), and laid
//!   along the range axis where the province's shapes are (the fold grain).
//! - **The profile.** In a hard bed the lowest [`WorldParams::bed_riser`]
//!   of the bed's run, or more if that would stand the riser more than
//!   [`WorldParams::bed_steepening`] times as steep as the bed coordinate
//!   (the ground's slope with the dip's) at full strength, rises the whole
//!   bed by a smoothstep (the cliff) and
//!   the rest is the bed's top (the bench, or a mesa's cap); in a soft bed
//!   the slope is eased at its foot and steeper under the next bed. Both
//!   meet the neighbouring beds at their boundaries, so the ground is
//!   continuous whichever bed a point is in; a province's `strength` is the
//!   share of that profile applied. The bound is on the bed coordinate's
//!   slope, not the ground's: where the dip is steep (folded rock's middle
//!   dip is 37.5 degrees) a riser stands at the ground's slope plus up to
//!   `4 strength` times the bed coordinate's, which the fade holds under
//!   [`WorldParams::bed_fade`]'s top on the coarse slope; on a sharp crease
//!   the ground's own slope, which `u`'s gradient reads, can exceed the
//!   coarse one, and the riser that multiple with it.
//! - **The gate.** Gentle ground keeps its shape (terraces over a smooth
//!   slope draw contour lines): the profile fades in over the province's
//!   `slope_gate` of the larger of the coarse slope and how far the ground
//!   stands over the ground [`WorldParams::hydration_radius_m`] round it (so
//!   a hilltop, flat but standing proud, keeps the cap its bed gives it),
//!   and fades out again where the coarse slope with the dip's is already
//!   steeper than [`WorldParams::bed_fade`] (a riser there would stand more
//!   than 8 m in a metre, and the ground is a cliff already). The coarse
//!   slope is the one the erosion is steered by: the relief's first two
//!   octaves read at the four hydration taps, and a range's own metres,
//!   which the gate reads only where a noise a few kilometres across
//!   ([`WorldParams::bed_patch`]) says (a range's ramp is smooth and runs
//!   for tens of kilometres, and beds opened on all of it stood the same
//!   risers along a range foot's contours for 20 km; the fade reads all of
//!   it). It is continuous, where the ground's own slope is not (a ridged octave's
//!   weight clamps, and its gradient jumps there): a weight read from the
//!   ground's own slope stepped the ground by up to a bed's height in a
//!   centimetre. And the beds fade with the share of the biomes' blend that
//!   is the sea's or the shore's, as the provinces' shapes keep off them: a
//!   beach is not terraced.
//!
//! The gradient is carried by the chain rule,
//! `grad h' = grad h + w (g'(t) - 1) grad u`, with `u` the bed coordinate,
//! `t` the place in the bed, `g` its profile and `w` its weight; the gate's
//! own gradient is left out, as the biomes' and the provinces' weights' are
//! (it would need the ground's second derivatives, which the review of step
//! 4 took out of the sampler where the kernel's second derivative jumps).
//! Everything per point is `+ - * /`, `sqrt`, `floor`
//! and integer hashing; the one transcendental, the tangent of the middle
//! dip, is in the constructor through `libm` (the crate's float rule).

use strata_core::{Seed, mix64};
use strata_data::Beds as BedsDef;

use crate::noise::{Fractal, Shape, smoothstep};
use crate::params::WorldParams;

/// A province's beds, resolved for sampling.
#[derive(Debug, Clone)]
pub(crate) struct Beds {
    /// The share of the bed profile applied, `0..=1`.
    strength: f64,
    /// The share of beds that are hard.
    hard_share: f64,
    /// The mean thickness, metres: the lattice the boundaries sit on.
    cell: f64,
    /// How far a boundary moves from its lattice point, as a share of the
    /// cell: every bed is `cell * (1 +- jitter)` thick.
    jitter: f64,
    /// The dip field and its size in metres of bed offset; `None` for level
    /// beds.
    dip: Option<(Fractal, f64)>,
    /// The slope gate, rise over run.
    gate: (f64, f64),
    /// The seed of the bed sequence.
    seed: u64,
}

/// What a province's beds make of the ground at a point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bedded {
    /// Metres added to the height.
    pub offset: f64,
    /// Added to the height's gradient.
    pub gradient: (f64, f64),
    /// The bed that shaped the ground here, `None` where the beds fade out
    /// on ground already steep: the bed the height before the gullies lies
    /// in, so a riser is labelled as the hard bed's riser even where a
    /// gully has since cut the surface down into an older bed.
    pub bed: Option<BedAt>,
}

/// Where the ground's surface lies in a province's beds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BedAt {
    /// The bed's index, counting up.
    pub index: i64,
    /// `0` at the bed's base, towards `1` at its top: a hard bed's riser
    /// is its lowest [`Beds::riser`], its bench the rest.
    pub phase: f64,
    /// Whether it is hard.
    pub hard: bool,
    /// The bed coordinate the surface stood at: its height and the dip's
    /// offset.
    pub coordinate: f64,
}

/// The mean size of the gradient of a dip field's fbm, per lattice unit of
/// its wavelength, measured once over 64 by 64 points: what a dip field's
/// metres are scaled by so that its typical slope is the dip asked for.
fn shape_slope(stretch: f64) -> f64 {
    const SIDE: u32 = 64;
    let fractal = Fractal::new(0x0bed_d1e5, Shape::Fbm, 1_000.0, DIP_OCTAVES, 0.5, 2.0)
        .stretched((1.0, 0.0), stretch);
    let mut sum = 0.0;
    for j in 0..SIDE {
        for i in 0..SIDE {
            let (_, (gx, gz)) =
                fractal.get_with_gradient(f64::from(i) * 370.0 + 13.7, f64::from(j) * 370.0 - 51.1);
            sum += (gx * gx + gz * gz).sqrt() * 1_000.0;
        }
    }
    sum / f64::from(SIDE * SIDE)
}

/// Octaves of a dip field: one, since a dip turns over tens of kilometres
/// and a second octave cost 25 ns a sample in every bedded province.
const DIP_OCTAVES: usize = 1;

/// A soft bed's slope at its foot, as a share of the ground's, is `1 -
/// SOFT_EASE` (and `1 + SOFT_EASE` under the next bed): the bench a hard
/// bed's top makes runs on into the soft bed over it.
const SOFT_EASE: f64 = 0.25;

impl Beds {
    /// A province's beds, or `None` where it has none (`strength` 0), so
    /// its ground is left to the bit. `label` names the province for the
    /// seed; `axis` and `stretch` are the fold grain the dip is laid along.
    pub(crate) fn new(
        seed: Seed,
        label: &str,
        def: &BedsDef,
        axis: (f64, f64),
        stretch: f64,
    ) -> Option<Self> {
        let strength = f64::from(def.strength);
        if strength <= 0.0 {
            return None;
        }
        let (lo, hi) = (
            f64::from(def.thickness_m.min),
            f64::from(def.thickness_m.max),
        );
        let cell = f64::midpoint(lo, hi);
        let middle_dip = f64::midpoint(f64::from(def.dip_deg.min), f64::from(def.dip_deg.max));
        let dip = (middle_dip > 0.0).then(|| {
            let wavelength = f64::from(def.dip_wavelength_m);
            let field = Fractal::new(
                seed.derive(&format!("beds/dip/{label}")),
                Shape::Fbm,
                wavelength,
                DIP_OCTAVES,
                0.5,
                2.0,
            )
            .stretched(axis, stretch);
            // The constructor is where the float rule allows a
            // transcendental, through `libm`.
            let slope = libm::tan(middle_dip.to_radians());
            (field, slope * wavelength / shape_slope(stretch))
        });
        Some(Self {
            strength,
            hard_share: f64::from(def.hard_share),
            cell,
            jitter: (hi - lo) / (hi + lo),
            dip,
            gate: (f64::from(def.slope_gate.min), f64::from(def.slope_gate.max)),
            seed: seed.derive(&format!("beds/{label}")),
        })
    }

    /// Where boundary `k` lies in the bed coordinate, metres.
    fn boundary(&self, k: i64) -> f64 {
        // An index is far inside 2^53; its hash's 53 high bits are a unit
        // value exactly.
        #[allow(clippy::cast_precision_loss, reason = "exact, see above")]
        let (kf, r) = (
            k as f64,
            (mix64(self.seed ^ k.cast_unsigned().wrapping_mul(0x9E37_79B9_7F4A_7C15)) >> 11) as f64
                / (1_u64 << 53) as f64,
        );
        (kf + self.jitter * (r - 0.5)) * self.cell
    }

    /// The bed at bed coordinate `u`: its index, base and thickness.
    fn bed(&self, u: f64) -> (i64, f64, f64) {
        // The coordinate is metres within tens of kilometres; the index is
        // exact.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "tens of thousands of beds at most"
        )]
        let k = (u / self.cell).floor() as i64;
        // Only the boundaries the answer needs, two hashes in most cases:
        // a sample in a bedded province pays for each (the review of step 5).
        let here = self.boundary(k + 1);
        if u >= here {
            return (k + 1, here, self.boundary(k + 2) - here);
        }
        let below = self.boundary(k);
        if u >= below {
            return (k, below, here - below);
        }
        let under = self.boundary(k - 1);
        (k - 1, under, below - under)
    }

    /// The share of a hard bed's run its riser takes: wide enough that its
    /// steepest point is at most `1 + strength (bed_steepening - 1)` times
    /// the slope of the bed coordinate (a smoothstep's steepest is 1.5 over
    /// its width, and `strength` of the profile is applied), and at least
    /// `bed_riser`: 0.3 with the shipped 0.2 and 5.
    pub(crate) fn riser(params: &WorldParams) -> f64 {
        (1.5 / params.bed_steepening).clamp(params.bed_riser, 1.0)
    }

    /// Whether bed `k` is hard.
    fn hard(&self, k: i64) -> bool {
        #[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
        let r = (mix64(
            (self.seed ^ 0x5851_F42D_4C95_7F2D)
                ^ k.cast_unsigned().wrapping_mul(0xD6E8_FEB8_6659_FD93),
        ) >> 11) as f64
            / (1_u64 << 53) as f64;
        r < self.hard_share
    }

    /// The dip field's offset of the bed coordinate at a point, and its
    /// gradient: what [`apply`](Self::apply) and [`at`](Self::at) take, read
    /// once for both.
    pub(crate) fn dip(&self, x: f64, z: f64) -> (f64, (f64, f64)) {
        match &self.dip {
            None => (0.0, (0.0, 0.0)),
            Some((field, size)) => {
                let (n, (dx, dz)) = field.get_with_gradient(x, z);
                (n * size, (dx * size, dz * size))
            }
        }
    }

    /// How far the beds may shape ground of coarse slope `slope` standing
    /// `stands` over the ground round it (see the module's gate), `0..=1`:
    /// zero on gentle ground, where the ground is left as it is and the dip
    /// is not read.
    pub(crate) fn gate(&self, slope: f64, stands: f64) -> f64 {
        smoothstep(self.gate.0, self.gate.1, slope.max(stands))
    }

    /// The bed `by` metres under or over `at`: where a surface cut down by
    /// the rivers' carve (ticket 0005, step 2) lies in the same beds, so
    /// the bands a valley's walls show are the beds at their own height and
    /// not those of the ground cut away. Shifted from the ground's own
    /// coordinate, so where the carve cuts nothing the bed is the ground's.
    pub(crate) fn relabel(&self, at: BedAt, by: f64) -> BedAt {
        let u = at.coordinate + by;
        let (k, base, thickness) = self.bed(u);
        BedAt {
            index: k,
            phase: ((u - base) / thickness).clamp(0.0, 1.0),
            hard: self.hard(k),
            coordinate: u,
        }
    }

    /// What the beds make of ground `height` metres high with gradient
    /// `gradient` (the whole ground's, the continent's base in it) where
    /// the dip field reads `dip` ([`dip`](Self::dip)), the
    /// [`gate`](Self::gate) is `gate` and the coarse slope `coarse` (see
    /// the module's gate). `params` gives the riser and the fade.
    #[allow(
        clippy::many_single_char_names,
        reason = "the chain rule's u, t, g and w, as the module's docs name them"
    )]
    pub(crate) fn apply(
        &self,
        dip: (f64, (f64, f64)),
        height: f64,
        gradient: (f64, f64),
        (gate, coarse): (f64, (f64, f64)),
        params: &WorldParams,
    ) -> Bedded {
        let (offset, d_offset) = dip;
        let u = height + offset;
        let du = (gradient.0 + d_offset.0, gradient.1 + d_offset.1);
        // The fade reads the coarse slope, which is continuous, and not the
        // ground's own, which jumps where a ridged octave's weight clamps:
        // a weight that jumped would step the ground by up to a bed.
        let cu = (coarse.0 + d_offset.0, coarse.1 + d_offset.1);
        let steep = (cu.0 * cu.0 + cu.1 * cu.1).sqrt();
        let fade = 1.0 - smoothstep(params.bed_fade.0, params.bed_fade.1, steep);
        let w = gate * fade * self.strength;
        if w <= 0.0 {
            return Bedded::NONE;
        }
        let (k, base, thickness) = self.bed(u);
        let t = (u - base) / thickness;
        let hard = self.hard(k);
        let (g, dg) = if hard {
            let r = Self::riser(params);
            if t < r {
                let s = t / r;
                (s * s * (3.0 - 2.0 * s), 6.0 * s * (1.0 - s) / r)
            } else {
                (1.0, 0.0)
            }
        } else {
            (
                t - SOFT_EASE * t * (1.0 - t),
                1.0 - SOFT_EASE + 2.0 * SOFT_EASE * t,
            )
        };
        let k_slope = w * (dg - 1.0);
        Bedded {
            offset: w * thickness * (g - t),
            gradient: (k_slope * du.0, k_slope * du.1),
            bed: Some(BedAt {
                index: k,
                phase: t.clamp(0.0, 1.0),
                hard,
                coordinate: u,
            }),
        }
    }
}

impl Bedded {
    /// The ground as it was.
    pub(crate) const NONE: Self = Self {
        offset: 0.0,
        gradient: (0.0, 0.0),
        bed: None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_data::MetreRange;

    fn beds(dip: (f32, f32)) -> Beds {
        Beds::new(
            Seed(7),
            "test",
            &BedsDef {
                thickness_m: MetreRange::new(8.0, 40.0),
                hard_share: 0.5,
                strength: 0.9,
                dip_deg: MetreRange::new(dip.0, dip.1),
                dip_wavelength_m: 20_000.0,
                slope_gate: MetreRange::new(0.15, 0.5),
            },
            (1.0, 0.0),
            1.0,
        )
        .expect("beds")
    }

    #[test]
    fn no_strength_is_no_beds() {
        let def = BedsDef::default();
        assert!(Beds::new(Seed(7), "test", &def, (1.0, 0.0), 1.0).is_none());
    }

    #[test]
    fn every_bed_is_inside_the_thickness_range_and_the_beds_tile_the_column() {
        let b = beds((0.0, 0.0));
        let mut last = b.bed(-5_000.0);
        let mut u = -5_000.0;
        while u < 5_000.0 {
            let (k, base, thickness) = b.bed(u);
            assert!(
                (8.0 - 1e-9..=40.0 + 1e-9).contains(&thickness),
                "{thickness}"
            );
            assert!(
                base <= u && u < base + thickness,
                "{u} in {base} + {thickness}"
            );
            if k != last.0 {
                assert_eq!(k, last.0 + 1, "no bed skipped");
                assert!((base - (last.1 + last.2)).abs() < 1e-9, "beds meet");
            }
            last = (k, base, thickness);
            u += 0.37;
        }
    }

    #[test]
    fn the_profile_is_continuous_and_its_gradient_is_the_heights() {
        let b = beds((0.0, 3.0));
        let params = WorldParams::default();
        // A steep uniform slope along x, so the gate is open.
        let slope = 0.6;
        let height = |x: f64| slope * x + 300.0;
        let at = |x: f64| {
            let z = 1_234.0;
            let gate = b.gate(slope, 0.0);
            let bed = b.apply(
                b.dip(x, z),
                height(x),
                (slope, 0.0),
                (gate, (slope, 0.0)),
                &params,
            );
            (height(x) + bed.offset, bed.gradient.0 + slope)
        };
        let mut worst_step: f64 = 0.0;
        let (mut risers, mut treads) = (0, 0);
        let mut x = 0.0;
        while x < 2_000.0 {
            let (h0, g0) = at(x);
            let (h1, _) = at(x + 0.01);
            worst_step = worst_step.max((h1 - h0).abs());
            // The analytic gradient against a difference over a centimetre,
            // away from the kinks at bed boundaries.
            let (hm, _) = at(x - 0.005);
            let (hp, _) = at(x + 0.005);
            let diff = (hp - hm) / 0.01;
            if (diff - g0).abs() > 0.05 {
                // Only at a kink: the gradient either side differs.
                let (_, ga) = at(x - 0.02);
                let (_, gb) = at(x + 0.02);
                assert!(
                    (ga - gb).abs() > 0.05,
                    "gradient {g0} against {diff} at {x}"
                );
            }
            risers += usize::from(g0 > 1.0);
            treads += usize::from(g0 < 0.27);
            x += 0.5;
        }
        assert!(worst_step < 0.1, "a step of {worst_step} m in a centimetre");
        assert!(
            risers > 100 && treads > 100,
            "{risers} risers, {treads} treads"
        );
    }
}
