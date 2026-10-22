//! The province field: which geological province each point is made of
//! (ticket 0004, step 3; `strata-registry`'s `geology` holds the
//! provinces themselves).
//!
//! A jittered cellular field on a lattice turned to an angle the seed
//! picks, read at the continent stage's warped point (which that stage
//! computes anyway, so the coastline's bends reach the borders for free).
//! The field has nothing to do with the climate: a province is tens of
//! kilometres of one rock, whatever grows on it.
//!
//! - **Cells.** One point per 24 km cell, jittered within the middle half
//!   of its cell, so the 3 by 3 cells round a point hold its nearest point
//!   and every neighbour within the blend.
//! - **The deal.** Cells are dealt provinces a block at a time (3 by 2
//!   cells for six provinces of one frequency): each block's slots hold
//!   every province by its frequency and are shuffled by the block's hash,
//!   so every province appears in every block (72 by 48 km for the six)
//!   and none takes a land mass alone. The block is the one of 6, 8, 9, 12
//!   or 16 slots whose whole slots come nearest the frequencies with a
//!   slot for every province, so a share is exact only to a slot. An
//!   independent draw per cell gave land shares of 10.5 to 30.3 % where the
//!   deal gives 13.9 to 21.0 (the design's prototype, on the world before
//!   step 2b).
//! - **The score.** A cell's score is minus half its squared distance to
//!   the point, in cells: continuous everywhere, since the least squared
//!   distance is, and linear across each border, so a border blends over
//!   one width along its length. The exact distance to a pair's bisector
//!   was tried and turned down: a third cell's weight jumped where the
//!   nearest point changed.
//! - **The wiggle.** One octave of noise moves each cell's score by the
//!   cell's own weight (a checkerboard of signs, so side neighbours always
//!   pull opposite ways, and a magnitude of 0.6 to 1 so no two borders bend
//!   alike): borders bend at 5 to 10 km for 12 ns. Without it they were
//!   long straight segments; a curl push looked better and cost 30 ns more.
//! - **The blend.** Every cell within a blend's width of the best score
//!   weighs `1 - smoothstep` of how far behind it is.
//! - **The ranges.** Where a mountain range rises (the range field from
//!   `province_range_lo` to `province_range_hi`, 0.35 to 0.6 by default)
//!   a province may yield to the one its file names: sandstone
//!   to granite, basalt and badlands to folded rock, so ranges stand in the
//!   hard rocks and the plateaus stay plateaus.
//!
//! The float rule of the crate holds: `+ - * /`, `floor`, `abs`, integer
//! hashing and one octave of the crate's simplex per point; the lattice's
//! angle is the one transcendental, through `libm`, in the constructor.
//! Everything is per point and pure, and a point costs about 60 ns on the
//! mesher's grids.

use strata_core::{Seed, mix64};
use strata_data::ProvinceId;

use crate::noise::{Fractal, Shape, smoothstep};
use crate::params::WorldParams;

/// At most this many provinces weigh at one point: every province there
/// is, once. Up to nine cells may weigh, each split by the range
/// substitution, so no smaller bound holds for any set of provinces; on
/// seed 42's six the review of step 3 counted four weighing at about one
/// point in a hundred and five at one in three thousand.
pub const MAX_MIX: usize = strata_data::MAX_PROVINCES;

/// What the field needs of one province.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProvinceRule {
    /// Its share of the block's slots against the others'. It must win its
    /// province a slot of some block: [`ProvinceField::new`] panics where
    /// none does, and the registry's freeze refuses a frequency under a
    /// sixteenth of the whole, which always wins one of sixteen.
    pub frequency: f64,
    /// Its place in the order provinces are dealt into a block's slots,
    /// lowest first, ties by index (the registry's `deal_order`).
    pub deal_order: u32,
    /// The index of the province it yields to in the ranges; its own for
    /// none.
    pub in_ranges: u8,
}

/// The provinces at a point with their weights, normalised to sum to one,
/// heaviest first (ties by id, so the order is the point's alone).
#[derive(Clone, Copy, PartialEq)]
pub struct ProvinceMix {
    entries: [(ProvinceId, f64); MAX_MIX],
    len: usize,
}

impl core::fmt::Debug for ProvinceMix {
    /// The provinces with a weight only; the unused slots are not the
    /// mix's.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("ProvinceMix").field(&self.entries()).finish()
    }
}

impl ProvinceMix {
    /// The provinces with a weight, heaviest first.
    #[must_use]
    pub fn entries(&self) -> &[(ProvinceId, f64)] {
        &self.entries[..self.len]
    }

    /// The weight of `province` here, zero where it has none.
    #[must_use]
    pub fn weight_of(&self, province: ProvinceId) -> f64 {
        self.entries()
            .iter()
            .find(|(id, _)| *id == province)
            .map_or(0.0, |&(_, w)| w)
    }

    /// The heaviest province.
    #[must_use]
    pub fn dominant(&self) -> ProvinceId {
        self.entries[0].0
    }

    /// The two heaviest and their shares of the pair, as [`Sample`]
    /// carries them and as its biome pair is: with one province alone it
    /// fills both slots at one and zero.
    ///
    /// [`Sample`]: crate::Sample
    #[must_use]
    pub fn pair(&self) -> [(ProvinceId, f32); 2] {
        let (first, w1) = self.entries[0];
        if self.len < 2 {
            return [(first, 1.0), (first, 0.0)];
        }
        let (second, w2) = self.entries[1];
        let share = w1 / (w1 + w2);
        // f64 to f32 is what a sample carries.
        #[allow(clippy::cast_possible_truncation)]
        [(first, share as f32), (second, (1.0 - share) as f32)]
    }

    /// Adds `weight` to `id`'s. Never full: `id` is one of at most
    /// [`MAX_MIX`] provinces, each held once.
    fn add(&mut self, id: ProvinceId, weight: f64) {
        if let Some(entry) = self.entries[..self.len].iter_mut().find(|e| e.0 == id) {
            entry.1 += weight;
        } else {
            debug_assert!(self.len < MAX_MIX, "more provinces than a field holds");
            self.entries[self.len] = (id, weight);
            self.len += 1;
        }
    }
}

/// A block of cells the deal fills, as compile-time constants: a runtime
/// divisor cost four integer divisions a cell in the design's prototype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    B3x2,
    B4x2,
    B3x3,
    B4x3,
    B4x4,
}

impl Block {
    const ALL: [Self; 5] = [Self::B3x2, Self::B4x2, Self::B3x3, Self::B4x3, Self::B4x4];

    /// The block whose slots, dealt by [`slot_counts`], give every province
    /// at least one and come nearest the frequencies (the least worst
    /// error of a province's share), the smaller on a tie; `None` when no
    /// block gives every province a slot. Equal frequencies take the first
    /// block with a slot each, as six provinces take the 3 by 2.
    ///
    /// Choosing by count alone dealt a province a share of none (a
    /// frequency of 3 among six split six slots 2, 1, 1, 1, 1, 0, and the
    /// last province had no cell anywhere in the world) or ignored a
    /// frequency under one (0.4 among six took a slot of six, as 1 would).
    fn for_rules(rules: &[ProvinceRule]) -> Option<Self> {
        let total: f64 = rules.iter().map(|r| r.frequency).sum();
        let mut best: Option<(Self, f64)> = None;
        for block in Self::ALL {
            if block.slots() < rules.len() {
                continue;
            }
            let counts = slot_counts(rules, block.slots());
            if counts.contains(&0) {
                continue;
            }
            #[allow(clippy::cast_precision_loss, reason = "at most 16 slots")]
            let error = rules
                .iter()
                .zip(&counts)
                .map(|(rule, &count)| {
                    (count as f64 / block.slots() as f64 - rule.frequency / total).abs()
                })
                .fold(0.0, f64::max);
            // A larger block only for a clearly nearer deal, not for a
            // rounding's worth: equal frequencies are exact in the first.
            if best.is_none_or(|(_, least)| error < least - 1e-9) {
                best = Some((block, error));
            }
        }
        best.map(|(block, _)| block)
    }

    const fn slots(self) -> usize {
        match self {
            Self::B3x2 => 6,
            Self::B4x2 => 8,
            Self::B3x3 => 9,
            Self::B4x3 => 12,
            Self::B4x4 => 16,
        }
    }
}

/// The field. Built once per world from the seed, the provinces in id
/// order and the [`WorldParams`]; sampled per point.
#[derive(Debug, Clone)]
pub struct ProvinceField {
    inv_cell: f64,
    jitter: f64,
    /// Half the blend's width, in cells.
    half: f64,
    /// The wiggle's extreme move of a score, in cells.
    wiggle_gain: f64,
    range_lo: f64,
    range_hi: f64,
    /// `(cos, sin)` of the lattice's angle.
    axis: (f64, f64),
    cells: u64,
    wiggle: Fractal,
    block: Block,
    /// The province of every slot of a block before its shuffle.
    slots: [u8; 16],
    in_ranges: [u8; 16],
}

/// The 3 by 3 cells round a point in the order they are scored: the own
/// cell first, then the sides, then the corners, so that a cell whose
/// nearest possible point is already a blend's width behind the best so
/// far is never hashed. It could not have weighed, so skipping it leaves
/// the answer as it would be.
const ORDER: [(i64, i64); 9] = [
    (0, 0),
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Multipliers of the cell and block hashes: the golden ratio's and two
/// of xxHash's and splitmix's primes, odd and well spread.
const HASH_I: u64 = 0x9E37_79B9_7F4A_7C15;
const HASH_J: u64 = 0xC2B2_AE3D_27D4_EB4F;
const HASH_MIX: u64 = 0xD6E8_FEB8_6659_FD93;
const HASH_BLOCK: u64 = 0x2545_F491_4F6C_DD1D;

impl ProvinceField {
    /// The field over `rules`, one per province in id order.
    ///
    /// # Panics
    ///
    /// If `rules` is empty or holds more than 16 provinces, a rule yields
    /// to a province out of range, or a frequency is so small against the
    /// others' that no block gives its province a slot; if `province_jitter` is over 0.5,
    /// or the wiggle and the blend together are so wide that the 3 by 3
    /// search could miss a cell with a weight (a point two cells away is
    /// at least 0.22 of a squared cell behind).
    #[must_use]
    pub fn new(seed: Seed, rules: &[ProvinceRule], params: &WorldParams) -> Self {
        assert!(
            (1..=16).contains(&rules.len()),
            "a province field needs 1 to 16 provinces, not {}",
            rules.len()
        );
        assert!(
            rules
                .iter()
                .all(|rule| usize::from(rule.in_ranges) < rules.len()),
            "a province yields to one that is not there"
        );
        let inv_cell = 1.0 / params.province_cell_m;
        let half = 0.5 * params.province_blend_m * inv_cell;
        let wiggle_gain = 0.5 * params.province_wiggle_m * inv_cell;
        assert!(
            (0.0..=0.5).contains(&params.province_jitter),
            "province_jitter {} is over 0.5",
            params.province_jitter
        );
        assert!(
            2.0 * wiggle_gain + half < 0.22,
            "the province wiggle ({} m) and blend ({} m) are too wide for a {} m cell",
            params.province_wiggle_m,
            params.province_blend_m,
            params.province_cell_m
        );
        let block = Block::for_rules(rules).unwrap_or_else(|| {
            panic!(
                "a province's frequency is so small against the others' that no block gives it \
                 a cell (the registry's freeze refuses under a sixteenth of the whole): {:?}",
                rules.iter().map(|r| r.frequency).collect::<Vec<_>>()
            )
        });
        let angle = seed.rng("geology/axis").next_f64() * core::f64::consts::PI;
        let mut in_ranges = [0; 16];
        for (slot, rule) in in_ranges.iter_mut().zip(rules) {
            *slot = rule.in_ranges;
        }
        Self {
            inv_cell,
            jitter: params.province_jitter,
            half,
            wiggle_gain,
            range_lo: params.province_range_lo,
            range_hi: params.province_range_hi,
            // `libm` rather than `f64::cos`, as for the ranges' axis: the
            // platform's is not correctly rounded.
            axis: (libm::cos(angle), libm::sin(angle)),
            cells: seed.derive("geology/cells"),
            wiggle: Fractal::new(
                seed.derive("geology/wiggle"),
                Shape::Fbm,
                params.province_wiggle_wavelength_m,
                1,
                0.5,
                2.0,
            ),
            block,
            slots: deal_slots(rules, block.slots()),
            in_ranges,
        }
    }

    /// The provinces at the continent stage's warped point `(wx, wz)`,
    /// where the range field is `range`.
    #[must_use]
    #[allow(
        clippy::similar_names,
        clippy::many_single_char_names,
        reason = "the lattice's u and v, in pairs, and the design's names"
    )]
    #[allow(
        clippy::manual_midpoint,
        reason = "half a squared distance, term for term as the design has it"
    )]
    pub fn sample(&self, wx: f64, wz: f64, range: f64) -> ProvinceMix {
        let (cos, sin) = self.axis;
        let u = (cos * wx + sin * wz) * self.inv_cell;
        let v = (-sin * wx + cos * wz) * self.inv_cell;
        let (fu, fv) = (u.floor(), v.floor());
        // Cells are tens of kilometres: far inside i64 anywhere a
        // coordinate keeps its precision.
        #[allow(clippy::cast_possible_truncation)]
        let (iu, iv) = (fu as i64, fv as i64);
        let (lu, lv) = (u - fu, v - fv);
        // The wiggle in cells: a cell's score moves by its own weight
        // (magnitude 0.6 to 1) times this.
        let wiggle = self.wiggle.get(wx, wz) * self.wiggle_gain;
        let mut score = [f64::NEG_INFINITY; 9];
        let mut hashes = [0u64; 9];
        let mut best = 4;
        let reach = 0.5 * self.jitter;
        let slack = wiggle.abs();
        // The least distance along one axis from `l` to a point of cell `d`.
        let near = |d: i64, l: f64| -> f64 {
            #[allow(clippy::cast_precision_loss, reason = "d is -1, 0 or 1")]
            let d = d as f64;
            (d + 0.5 - reach - l).max(l - (d + 0.5 + reach)).max(0.0)
        };
        for &(di, dj) in &ORDER {
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "di and dj are -1, 0 or 1"
            )]
            let k = ((dj + 1) * 3 + (di + 1)) as usize;
            if k != 4 {
                let (nu, nv) = (near(di, lu), near(dj, lv));
                if score[best] + 0.5 * (nu * nu + nv * nv) - 2.0 * slack >= self.half {
                    continue;
                }
            }
            let (ci, cj) = (iu + di, iv + dj);
            // One multiply-xorshift round: the jitter and the weight need
            // well spread bits, not a full avalanche.
            let mut h = self.cells
                ^ ci.cast_unsigned().wrapping_mul(HASH_I)
                ^ cj.cast_unsigned().wrapping_mul(HASH_J);
            h ^= h >> 32;
            h = h.wrapping_mul(HASH_MIX);
            h ^= h >> 29;
            // Sixteen bits each, exact in f64.
            #[allow(clippy::cast_precision_loss)]
            let (ju, jv, m) = (
                (h & 0xFFFF) as f64 * (1.0 / 65_536.0),
                ((h >> 16) & 0xFFFF) as f64 * (1.0 / 65_536.0),
                (h >> 48) as f64 * (1.0 / 65_536.0),
            );
            let magnitude = 0.6 + 0.4 * m;
            let weight = if (ci + cj) & 1 == 0 {
                magnitude
            } else {
                -magnitude
            };
            #[allow(clippy::cast_precision_loss, reason = "di and dj are -1, 0 or 1")]
            let (du, dv) = (
                di as f64 + 0.5 + (ju - 0.5) * self.jitter - lu,
                dj as f64 + 0.5 + (jv - 0.5) * self.jitter - lv,
            );
            score[k] = weight * wiggle - 0.5 * (du * du + dv * dv);
            hashes[k] = h;
            if score[k] > score[best] {
                best = k;
            }
        }
        let r = smoothstep(self.range_lo, self.range_hi, range);
        let mut mix = ProvinceMix {
            entries: [(ProvinceId::NONE, 0.0); MAX_MIX],
            len: 0,
        };
        let mut total = 0.0;
        for k in 0..9 {
            let behind = score[best] - score[k];
            if behind >= self.half {
                continue;
            }
            let w = 1.0 - smoothstep(0.0, self.half, behind);
            #[allow(clippy::cast_possible_wrap, reason = "k is under nine")]
            let (di, dj) = ((k % 3) as i64 - 1, (k / 3) as i64 - 1);
            let own = self.province_of(iu + di, iv + dj);
            let yields = self.in_ranges[usize::from(own)];
            for (index, part) in [(own, w * (1.0 - r)), (yields, w * r)] {
                if part > 0.0 {
                    total += part;
                    mix.add(ProvinceId(u32::from(index)), part);
                }
            }
        }
        // Normalise and order heaviest first, ties by id, by insertion. A
        // division rather than a reciprocal's product, so a province alone
        // weighs exactly one.
        for a in 0..mix.len {
            mix.entries[a].1 /= total;
            let mut b = a;
            while b > 0 {
                let (x, y) = (mix.entries[b - 1], mix.entries[b]);
                // Exact ties go to the lower id, so the order is the point's.
                #[allow(clippy::float_cmp, reason = "an exact tie is what is broken")]
                let tie = y.1 == x.1;
                if y.1 > x.1 || (tie && y.0 < x.0) {
                    mix.entries.swap(b - 1, b);
                    b -= 1;
                } else {
                    break;
                }
            }
        }
        mix
    }

    /// The province (index) dealt to cell `(i, j)`.
    fn province_of(&self, i: i64, j: i64) -> u8 {
        match self.block {
            Block::B3x2 => self.deal::<3, 2>(i, j),
            Block::B4x2 => self.deal::<4, 2>(i, j),
            Block::B3x3 => self.deal::<3, 3>(i, j),
            Block::B4x3 => self.deal::<4, 3>(i, j),
            Block::B4x4 => self.deal::<4, 4>(i, j),
        }
    }

    /// The cell's slot of its block's shuffled deal: Fisher-Yates over the
    /// block's slots with a multiply-high in place of a modulo, stopped once
    /// the slot is settled. Any count of slots, no division.
    fn deal<const W: i64, const H: i64>(&self, i: i64, j: i64) -> u8 {
        let (bi, bj) = (i.div_euclid(W), j.div_euclid(H));
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "rem_euclid is never negative, and under 16"
        )]
        let slot = (i.rem_euclid(W) + j.rem_euclid(H) * W) as usize;
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "a block's size, 6 to 16"
        )]
        let n = (W * H) as usize;
        let mut deal = self.slots;
        let mut state = mix64(
            self.cells
                ^ HASH_BLOCK
                ^ bi.cast_unsigned().wrapping_mul(HASH_I)
                ^ bj.cast_unsigned().wrapping_mul(HASH_J),
        );
        for k in 0..=slot.min(n - 2) {
            state = mix64(state.wrapping_add(HASH_I));
            // `(n - k) * (state >> 32) >> 32` is below `n - k`.
            #[allow(clippy::cast_possible_truncation)]
            let pick = k + (((state >> 32) * (n - k) as u64) >> 32) as usize;
            deal.swap(k, pick);
        }
        deal[slot]
    }
}

/// A block's slots filled by frequency, in deal order, each province's
/// count of them by [`slot_counts`].
fn deal_slots(rules: &[ProvinceRule], slots: usize) -> [u8; 16] {
    let counts = slot_counts(rules, slots);
    let mut order: Vec<usize> = (0..rules.len()).collect();
    order.sort_by_key(|&k| (rules[k].deal_order, k));
    let mut out = [0u8; 16];
    let mut at = 0;
    for k in order {
        for _ in 0..counts[k] {
            out[at] = u8::try_from(k).expect("at most 16 provinces");
            at += 1;
        }
    }
    out
}

/// How many of a block's `slots` each province takes, in index order:
/// its exact share of the slots rounded down, then the slots left over to
/// the largest remainders (ties to the earlier in the deal).
fn slot_counts(rules: &[ProvinceRule], slots: usize) -> Vec<usize> {
    let total: f64 = rules.iter().map(|r| r.frequency).sum();
    #[allow(clippy::cast_precision_loss, reason = "at most 16 slots")]
    let n = slots as f64;
    let mut order: Vec<usize> = (0..rules.len()).collect();
    order.sort_by_key(|&k| (rules[k].deal_order, k));
    let mut counts: Vec<(usize, f64, usize)> = order
        .iter()
        .map(|&k| {
            let rule = &rules[k];
            let exact = rule.frequency / total * n;
            let whole = exact.floor();
            // At most 16, exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = whole as usize;
            (k, exact - whole, count)
        })
        .collect();
    let mut left = slots - counts.iter().map(|c| c.2).sum::<usize>();
    // Stable, so equal remainders keep index order.
    counts.sort_by(|a, b| b.1.total_cmp(&a.1));
    for count in &mut counts {
        if left == 0 {
            break;
        }
        count.2 += 1;
        left -= 1;
    }
    let mut out = vec![0; rules.len()];
    for (k, _, count) in counts {
        out[k] = count;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(n: u8) -> Vec<ProvinceRule> {
        (0..n)
            .map(|k| ProvinceRule {
                frequency: 1.0,
                deal_order: 0,
                in_ranges: k,
            })
            .collect()
    }

    #[test]
    fn slots_follow_the_frequencies() {
        assert_eq!(deal_slots(&rules(6), 6)[..6], [0, 1, 2, 3, 4, 5]);
        let mut weighted = rules(3);
        weighted[1].frequency = 4.0;
        // 6 slots at 1:4:1 are 1, 4, 1 exactly.
        assert_eq!(deal_slots(&weighted, 6)[..6], [0, 1, 1, 1, 1, 2]);
        // Eight slots for seven leave one over to the first.
        assert_eq!(deal_slots(&rules(7), 8)[..8], [0, 0, 1, 2, 3, 4, 5, 6]);
        // The deal's order, not the index's, and its leftover slot.
        let mut ordered = rules(7);
        for (rule, order) in ordered.iter_mut().zip([6, 5, 4, 3, 2, 1, 0]) {
            rule.deal_order = order;
        }
        assert_eq!(deal_slots(&ordered, 8)[..8], [6, 6, 5, 4, 3, 2, 1, 0]);
    }

    fn weighted(frequencies: &[f64]) -> Vec<ProvinceRule> {
        let mut out = rules(u8::try_from(frequencies.len()).expect("a few"));
        for (rule, &f) in out.iter_mut().zip(frequencies) {
            rule.frequency = f;
        }
        out
    }

    /// The block is chosen by what its slots deal, not by the count of
    /// provinces: every province gets a slot, and a frequency moves its
    /// share whether it is over one or under.
    #[test]
    fn the_block_gives_every_province_its_share() {
        let cases: [(&[f64], Block, &[usize]); 7] = [
            (&[1.0; 6], Block::B3x2, &[1; 6]),
            // By count this was the 3 by 2 with the last province dealt none.
            (
                &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                Block::B4x2,
                &[3, 1, 1, 1, 1, 1],
            ),
            // By count, 0.4 was dealt a slot of six as 1 would be.
            (
                &[1.0, 1.0, 1.0, 1.0, 1.0, 0.4],
                Block::B4x4,
                &[3, 3, 3, 3, 3, 1],
            ),
            // Seven alike are nearer their shares in sixteen (3 or 2 of 16
            // against a seventh) than in eight (2 or 1 of 8).
            (&[1.0; 7], Block::B4x4, &[3, 3, 2, 2, 2, 2, 2]),
            (&[1.0; 9], Block::B3x3, &[1; 9]),
            (&[1.0; 16], Block::B4x4, &[1; 16]),
            (&[2.0, 1.0, 1.0], Block::B4x2, &[4, 2, 2]),
        ];
        for (frequencies, block, counts) in cases {
            let rules = weighted(frequencies);
            assert_eq!(Block::for_rules(&rules), Some(block), "{frequencies:?}");
            assert_eq!(
                slot_counts(&rules, block.slots()),
                counts,
                "{frequencies:?}"
            );
        }
        // A thirtieth of the whole may still take a slot of sixteen by its
        // remainder (the registry refuses under a sixteenth, which always
        // takes one); a hundred and sixtieth takes none in any block.
        assert_eq!(
            Block::for_rules(&weighted(&[1.0, 1.0, 1.0, 0.1])),
            Some(Block::B4x4)
        );
        let mut starved = vec![1.0; 8];
        starved.push(0.05);
        assert_eq!(Block::for_rules(&weighted(&starved)), None);
    }

    /// The share of the world a province takes follows its frequency: no
    /// province is dealt out of the world, and one under 1 takes less.
    #[test]
    fn a_frequency_moves_its_province_share_of_the_world() {
        let shares = |frequencies: &[f64]| -> Vec<f64> {
            let field =
                ProvinceField::new(Seed(42), &weighted(frequencies), &WorldParams::default());
            let mut counts = vec![0u32; frequencies.len()];
            let n = 400;
            for k in 0..n * n {
                let (x, z) = (
                    f64::from(k % n) * 5_003.0 - 1.0e6,
                    f64::from(k / n) * 4_999.0 - 1.0e6,
                );
                counts[field.sample(x, z, 0.0).dominant().0 as usize] += 1;
            }
            counts
                .iter()
                .map(|&c| f64::from(c) / f64::from(n * n))
                .collect()
        };
        let heavy = shares(&[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!((heavy[0] - 0.375).abs() < 0.03, "{heavy:?}");
        for &share in &heavy[1..] {
            assert!((share - 0.125).abs() < 0.02, "{heavy:?}");
        }
        let light = shares(&[1.0, 1.0, 1.0, 1.0, 1.0, 0.4]);
        assert!((light[5] - 0.0625).abs() < 0.015, "{light:?}");
    }

    /// Up to sixteen provinces, every one of them may weigh at a point
    /// without the mix overflowing: with a blend as wide as the search
    /// allows and each province yielding to the next in the ranges, seven
    /// or more weigh at once somewhere (six was the mix's old capacity,
    /// and the sample panicked there).
    #[test]
    fn a_mix_holds_every_province_that_weighs() {
        let params = WorldParams {
            province_blend_m: 10_000.0,
            province_wiggle_m: 0.0,
            ..WorldParams::default()
        };
        for count in [7_u8, 9, 16] {
            let mut rules = rules(count);
            for rule in &mut rules {
                rule.in_ranges = (rule.in_ranges + 1) % count;
            }
            let field = ProvinceField::new(Seed(42), &rules, &params);
            let mut fullest = 0;
            let n = 300;
            for k in 0..n * n {
                let (x, z) = (
                    f64::from(k % n) * 1_009.0 - 150_000.0,
                    f64::from(k / n) * 997.0 - 150_000.0,
                );
                let mix = field.sample(x, z, 0.475);
                let sum: f64 = mix.entries().iter().map(|e| e.1).sum();
                assert!((sum - 1.0).abs() < 1e-12, "{count}: {mix:?}");
                fullest = fullest.max(mix.entries().len());
            }
            assert!(
                fullest > 6,
                "{count} provinces: at most {fullest} weighed at once"
            );
        }
    }

    /// Every block deals every province once when there are six: the deal
    /// is a permutation of the slots.
    #[test]
    fn a_block_holds_every_province() {
        let field = ProvinceField::new(Seed(42), &rules(6), &WorldParams::default());
        for bi in -20..20 {
            for bj in -20..20 {
                let mut seen = [false; 6];
                for dj in 0..2 {
                    for di in 0..3 {
                        seen[usize::from(field.province_of(bi * 3 + di, bj * 2 + dj))] = true;
                    }
                }
                assert!(seen.iter().all(|&s| s), "block ({bi}, {bj}) misses one");
            }
        }
    }

    #[test]
    fn one_province_is_everywhere() {
        let field = ProvinceField::new(Seed(1), &rules(1), &WorldParams::default());
        for k in 0..1000 {
            let x = f64::from(k) * 377.0 - 180_000.0;
            let mix = field.sample(x, -x * 0.7, f64::from(k % 7) / 6.0);
            assert_eq!(mix.entries(), &[(ProvinceId(0), 1.0)]);
            assert_eq!(mix.pair(), [(ProvinceId(0), 1.0), (ProvinceId(0), 0.0)]);
        }
    }

    /// In the ranges a province yields to the one it names, all of it once
    /// the range field is over its top.
    #[test]
    fn a_province_yields_in_the_ranges() {
        let mut yielding = rules(2);
        yielding[1].in_ranges = 0;
        let field = ProvinceField::new(Seed(5), &yielding, &WorldParams::default());
        let mut lowland = 0;
        for k in 0..4000 {
            let x = f64::from(k) * 97.0 - 190_000.0;
            let z = f64::from(k) * 61.0;
            assert_eq!(field.sample(x, z, 0.9).entries(), &[(ProvinceId(0), 1.0)]);
            let low = field.sample(x, z, 0.0);
            if low.weight_of(ProvinceId(1)) > 0.0 {
                lowland += 1;
            }
            let part = field.sample(x, z, 0.425);
            let sum: f64 = part.entries().iter().map(|e| e.1).sum();
            assert!((sum - 1.0).abs() < 1e-12);
        }
        assert!(
            lowland > 1000,
            "the yielding province stands in the lowland"
        );
    }

    #[test]
    #[should_panic(expected = "too wide")]
    fn a_blend_too_wide_for_the_search_is_refused() {
        let params = WorldParams {
            province_blend_m: 8_000.0,
            ..WorldParams::default()
        };
        let _ = ProvinceField::new(Seed(1), &rules(6), &params);
    }
}
