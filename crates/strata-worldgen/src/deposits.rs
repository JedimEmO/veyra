//! Deposits: where the ground holds what a household draws (ticket 0007,
//! step 3; `docs/design/crafting.md`, 3.3).
//!
//! A deposit is a pure function of the seed, the resource registry and the
//! world's samples. It is a matter of record, not a find: it never runs
//! out, its numbers never change, and asking twice, on any thread, gives
//! the same deposit to the bit. Nothing here is baked, cached or hashed
//! into the world's version, and no deposit moves the ground: the river
//! bake and its canaries know nothing of it.
//!
//! Candidates stand on a square lattice, one a cell, off the cell's centre
//! by a hash of the cell. A cell is kept or not by another hash; a kept one
//! tries a few points and takes the first that is not in water, at the
//! sea's edge, on steep ground, on a road's cleared way or on a pad or a
//! settlement's ground. The point's type is chosen by the weights the
//! resource files give the provinces, the biomes and the water record
//! there, with a roll that is low in discrepancy over neighbouring cells,
//! so the types of a stretch of country come up in their proportions and
//! not in runs. Its figures are one roll the whole deposit shares plus each
//! attribute's own, inside the type's floor and cap, so most deposits are
//! high in one line and plain in the rest. Its name is dealt from the
//! roots so that one repeats a block of cells on and no nearer. It is an
//! area: a radius and a concentration at its centre, each a roll of its
//! cell inside the dials, with the concentration falling to nothing at the
//! radius.
//!
//! A deposit's id is its cell, so it is stable for a seed and a lattice: a
//! changed `cell_m` is every deposit renamed.
//!
//! Every dial is `resources/deposits.ron` (`strata_registry::resource`).
//! The arithmetic is adds, multiplies, divides, `sqrt` and `floor` alone, as the
//! rest of this crate's per-sample code is.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use strata_core::{Seed, mix64};
use strata_registry::{Attribute, GameRegistries, ResourceSet, ResourceTypeId};

use crate::hydrology::WaterKind;
use crate::{ProceduralWorld, Sample, WorldSource};

/// A deposit's identity for a seed and a lattice: the cell it stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DepositId(pub [i32; 2]);

impl std::fmt::Display for DepositId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deposit {},{}", self.0[0], self.0[1])
    }
}

/// One deposit, whole: nothing about it is looked up afterwards.
#[derive(Debug, Clone, PartialEq)]
pub struct Deposit {
    /// Its identity.
    pub id: DepositId,
    /// What it holds.
    pub ty: ResourceTypeId,
    /// Its name, `Calder`: a lot of it is "Calder salt".
    pub root: String,
    /// The place it lies, `Calder flats`.
    pub place: String,
    /// Where, world `(x, z)`.
    pub at: [f64; 2],
    /// The ground there as the world samples it, metres.
    pub height_m: f32,
    /// One value for each attribute its class carries, in the class's
    /// order. An attribute the class does not carry is not here.
    pub figures: Vec<(Attribute, u16)>,
    /// How far its area reaches from `at`, metres.
    pub radius_m: f32,
    /// The concentration at `at`, per-mille.
    pub peak: u16,
    /// The shared grade roll, per-mille: what ranks a deposit against the
    /// rest, which a claim reads (ticket 0012, step 3).
    pub grade_pm: u16,
}

impl Deposit {
    /// What a lot of it is called: `Calder salt`.
    #[must_use]
    pub fn lot_name(&self, set: &ResourceSet) -> String {
        match set.get(self.ty) {
            Some(def) => format!("{} {}", self.root, def.short),
            None => self.root.clone(),
        }
    }

    /// The figure of `attribute`, or `None` for one its class does not
    /// carry.
    #[must_use]
    pub fn figure(&self, attribute: Attribute) -> Option<u16> {
        self.figures
            .iter()
            .find(|(a, _)| *a == attribute)
            .map(|(_, value)| *value)
    }

    /// Metres from `(x, z)`, over the ground.
    #[must_use]
    pub fn distance_m(&self, x: f64, z: f64) -> f64 {
        let (dx, dz) = (self.at[0] - x, self.at[1] - z);
        (dx * dx + dz * dz).sqrt()
    }

    /// The concentration at `(x, z)`, per-mille: the peak at the centre,
    /// falling smoothly to nothing at the radius and nought past it. The
    /// distance is never taken as a root, so it is the same bits anywhere.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a peak of at most a thousand, rounded"
    )]
    pub fn concentration(&self, x: f64, z: f64) -> u16 {
        let (dx, dz) = (self.at[0] - x, self.at[1] - z);
        let radius = f64::from(self.radius_m);
        let t2 = (dx * dx + dz * dz) / (radius * radius);
        // Not `t2 < 1.0`: a NaN (a non-finite point) is no concentration.
        if t2.partial_cmp(&1.0) != Some(std::cmp::Ordering::Less) {
            return 0;
        }
        let fall = 1.0 - t2;
        (f64::from(self.peak) * fall * fall + 0.5).floor() as u16
    }
}

/// The two rotations of the plastic number's lattice: a cell's type roll
/// is the fraction of its coordinates times these, which spreads the rolls
/// of any patch of neighbouring cells evenly over the types.
const ROLL_X: f64 = 0.754_877_666_246_692_7;
const ROLL_Z: f64 = 0.569_840_290_998_053_2;

/// The step the roots are dealt by within a block. Odd and prime, so it
/// shares no factor with a block of any size a file would give.
const ROOT_STRIDE: usize = 389;

/// What tells the world's one type offset from its other hashes.
const TYPE_SALT: u64 = 0x7479_7065;

/// What a hash of a cell is for.
#[derive(Clone, Copy)]
enum Salt {
    Keep,
    Grade,
    Ground,
    Radius,
    Peak,
    /// An attribute's own roll, by its index.
    Figure(usize),
    /// A try's offset along `x` and along `z`.
    Try(u32, bool),
}

impl Salt {
    fn value(self) -> u64 {
        match self {
            Self::Keep => 0,
            Self::Grade => 2,
            Self::Ground => 3,
            Self::Radius => 4,
            Self::Peak => 5,
            Self::Figure(index) => 16 + index as u64,
            Self::Try(n, along_z) => 64 + 2 * u64::from(n) + u64::from(along_z),
        }
    }
}

/// 53 bits of a hash as a float in `0..1`.
#[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1_u64 << 53) as f64
}

/// A cell's deposit before its figures.
struct Found {
    ty: ResourceTypeId,
    at: [f64; 2],
    height_m: f32,
}

/// The deposits of a world.
///
/// Holds the world, the registries and the seed and nothing else: every
/// query works its answer out again, which is what makes one the same on
/// any thread and on a second call. A cell's candidate is a sample of the
/// ground, which bakes the land mass it meets if no one has; a process
/// prepares its water before it asks, as it does before it samples.
#[derive(Clone)]
pub struct DepositField {
    world: Arc<ProceduralWorld>,
    registries: Arc<GameRegistries>,
    seed: u64,
}

impl std::fmt::Debug for DepositField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepositField")
            .field("seed", &self.seed)
            .field("types", &self.set().types().len())
            .finish_non_exhaustive()
    }
}

impl DepositField {
    /// The deposits of `world`, which was built from `seed` and
    /// `registries`.
    #[must_use]
    pub fn new(world: Arc<ProceduralWorld>, seed: Seed, registries: Arc<GameRegistries>) -> Self {
        Self {
            world,
            registries,
            seed: seed.derive("deposits"),
        }
    }

    /// The resource registry the deposits are of.
    #[must_use]
    pub fn set(&self) -> &ResourceSet {
        &self.registries.resources
    }

    /// The world the deposits lie in.
    #[must_use]
    pub fn world(&self) -> &Arc<ProceduralWorld> {
        &self.world
    }

    /// The cell `(x, z)` is in: the id of the deposit there, if the cell
    /// holds one.
    #[must_use]
    pub fn cell_of(&self, x: f64, z: f64) -> DepositId {
        let cell = self.set().rules().lattice.cell_m;
        if cell > 0.0 {
            DepositId([cell_index(x / cell), cell_index(z / cell)])
        } else {
            DepositId([0, 0])
        }
    }

    /// Every deposit within `radius_m` of `(x, z)`, nearest first, then by
    /// id.
    #[must_use]
    pub fn near(&self, x: f64, z: f64, radius_m: f64) -> Vec<Deposit> {
        let asked = x.is_finite() && z.is_finite() && radius_m >= 0.0;
        if self.set().is_empty() || !asked {
            return Vec::new();
        }
        let (lo, hi) = (
            self.cell_of(x - radius_m, z - radius_m).0,
            self.cell_of(x + radius_m, z + radius_m).0,
        );
        let mut found: Vec<(f64, Deposit)> = Vec::new();
        for cz in lo[1]..=hi[1] {
            for cx in lo[0]..=hi[0] {
                let id = DepositId([cx, cz]);
                let Some(candidate) = self.find(id) else {
                    continue;
                };
                let (dx, dz) = (candidate.at[0] - x, candidate.at[1] - z);
                let distance = (dx * dx + dz * dz).sqrt();
                if distance <= radius_m {
                    found.push((distance, self.finish(id, &candidate)));
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        found.into_iter().map(|(_, deposit)| deposit).collect()
    }

    /// The deposit `id`, or `None` for a cell that holds none.
    #[must_use]
    pub fn get(&self, id: DepositId) -> Option<Deposit> {
        if self.set().is_empty() {
            return None;
        }
        let found = self.find(id)?;
        Some(self.finish(id, &found))
    }

    /// Every deposit whose area holds `(x, z)`, with its concentration
    /// there in per-mille: strongest first, then by id.
    #[must_use]
    pub fn covering(&self, x: f64, z: f64) -> Vec<(Deposit, u16)> {
        let reach = self.set().rules().area.radius_m.1;
        let mut held: Vec<(Deposit, u16)> = self
            .near(x, z, reach)
            .into_iter()
            .filter_map(|deposit| {
                let strength = deposit.concentration(x, z);
                (strength > 0).then_some((deposit, strength))
            })
            .collect();
        held.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        held
    }

    /// The nearest deposit within `radius_m` of `(x, z)` that `wanted`
    /// takes.
    #[must_use]
    pub fn nearest(
        &self,
        x: f64,
        z: f64,
        radius_m: f64,
        wanted: impl Fn(&Deposit) -> bool,
    ) -> Option<Deposit> {
        self.near(x, z, radius_m).into_iter().find(wanted)
    }

    /// A hash of the seed, a cell and what it is for. The cell is packed
    /// whole, so two cells never share a hash by construction.
    fn hash(&self, cell: [i32; 2], salt: Salt) -> u64 {
        let packed =
            u64::from(cell[0].cast_unsigned()) | (u64::from(cell[1].cast_unsigned()) << 32);
        mix64(self.seed ^ salt.value().wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ mix64(packed))
    }

    /// Whether a candidate may not stand on `sample`.
    fn refuses(&self, sample: &Sample) -> bool {
        let refuse = &self.set().rules().refuse;
        let sea = f64::from(self.world.sea_level());
        sample.in_water()
            || f64::from(sample.height) < sea + refuse.shore_m
            || f64::from(sample.landform.slope) > refuse.slope_max
            || f64::from(sample.landform.cleared) > refuse.cleared_max
            || f64::from(sample.landform.developed) > refuse.developed_max
    }

    /// The type a point holds: each type's weight there, and the cell's
    /// roll through them in id order. `None` where no type has any weight.
    fn type_at(&self, sample: &Sample, cell: [i32; 2]) -> Option<ResourceTypeId> {
        let set = self.set();
        let weight = |ty: ResourceTypeId| {
            let occurs = set.occurs(ty);
            let mut weight = occurs.anywhere;
            for (province, share) in sample.provinces {
                weight += occurs.province(province) * f64::from(share);
            }
            for (biome, share) in sample.biomes {
                weight += occurs.biome(biome) * f64::from(share);
            }
            weight
                + match sample.water.map(|water| water.kind) {
                    Some(WaterKind::Lake) => occurs.water.lake,
                    Some(WaterKind::Channel | WaterKind::Fall | WaterKind::Mouth) => {
                        occurs.water.river
                    }
                    None => 0.0,
                }
        };
        let total: f64 = set.type_ids().map(weight).sum();
        if total.is_nan() || total <= 0.0 {
            return None;
        }
        // One offset a world, and the lattice's turn a cell: a roll of its
        // own a cell would be white noise, which leaves a type out of a
        // whole district as often as not.
        let turned = unit(mix64(self.seed ^ TYPE_SALT))
            + f64::from(cell[0]) * ROLL_X
            + f64::from(cell[1]) * ROLL_Z;
        let roll = (turned - turned.floor()) * total;
        let mut passed = 0.0;
        let mut last = None;
        for ty in set.type_ids() {
            let weight = weight(ty);
            if weight <= 0.0 {
                continue;
            }
            passed += weight;
            last = Some(ty);
            if roll < passed {
                break;
            }
        }
        last
    }

    /// The cell's deposit, if it holds one: kept, on ground that takes it,
    /// of a type the ground gives a weight.
    fn find(&self, id: DepositId) -> Option<Found> {
        let cell = id.0;
        let lattice = &self.set().rules().lattice;
        if unit(self.hash(cell, Salt::Keep)) >= lattice.keep {
            return None;
        }
        for n in 0..lattice.tries {
            let off = |along_z: bool| {
                (unit(self.hash(cell, Salt::Try(n, along_z))) - 0.5) * lattice.jitter
            };
            let at = [
                (f64::from(cell[0]) + 0.5 + off(false)) * lattice.cell_m,
                (f64::from(cell[1]) + 0.5 + off(true)) * lattice.cell_m,
            ];
            let sample = self.world.sample(at[0], at[1]);
            if self.refuses(&sample) {
                continue;
            }
            // The first ground that takes a candidate is the cell's; where
            // nothing occurs on it the cell is empty, not moved.
            return Some(Found {
                ty: self.type_at(&sample, cell)?,
                at,
                height_m: sample.height,
            });
        }
        None
    }

    /// The found deposit with its figures, its names and its area.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a span of at most a thousand, rounded and held to the span; a radius is f32 by design"
    )]
    fn finish(&self, id: DepositId, found: &Found) -> Deposit {
        let set = self.set();
        let rules = set.rules();
        let cell = id.0;
        let shared = rules.grade.shared;
        let grade = unit(self.hash(cell, Salt::Grade));
        let class = set.class_of(found.ty);
        let def = set.get(found.ty);
        let figures = class
            .carries
            .iter()
            .filter_map(|attribute| {
                let span = *def?.ranges.get(attribute)?;
                let roll = unit(self.hash(cell, Salt::Figure(attribute.index())));
                let mut own = 1.0;
                for _ in 0..rules.grade.spike {
                    own *= roll;
                }
                let share = shared * grade + (1.0 - shared) * own;
                let width = f64::from(span.cap - span.floor);
                let over = (width * share + 0.5).floor().clamp(0.0, width) as u16;
                Some((*attribute, span.floor + over))
            })
            .collect();
        let root = self.root(cell);
        let place = match def {
            Some(def) if !def.grounds.is_empty() => {
                let pick = self.hash(cell, Salt::Ground) % def.grounds.len() as u64;
                format!("{root} {}", def.grounds[pick as usize])
            }
            _ => root.clone(),
        };
        let (radius, peak) = (rules.area.radius_m, rules.area.peak);
        let radius_m =
            (radius.0 + (radius.1 - radius.0) * unit(self.hash(cell, Salt::Radius))) as f32;
        let span = f64::from(peak.1 - peak.0);
        let peak = peak.0 + (span * unit(self.hash(cell, Salt::Peak)) + 0.5).floor() as u16;
        Deposit {
            id,
            ty: found.ty,
            root,
            place,
            at: found.at,
            height_m: found.height_m,
            figures,
            radius_m,
            peak,
            grade_pm: (grade * 1000.0).floor().clamp(0.0, 999.0) as u16,
        }
    }

    /// The cell's name: the roots are dealt once over each block of cells,
    /// by a stride that visits every root, from a place the seed picks.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "a block's side is the root of a list's length, and a cell's place in it is under that"
    )]
    fn root(&self, cell: [i32; 2]) -> String {
        let rules = self.set().rules();
        let side = rules.root_block();
        if side == 0 {
            return String::new();
        }
        let count = side * side;
        let within = |c: i32| c.rem_euclid(side as i32) as usize;
        let place = within(cell[0]) + side * within(cell[1]);
        let offset = (mix64(self.seed ^ 0x726f_6f74) % count as u64) as usize;
        rules.roots[(place * ROOT_STRIDE + offset) % count].clone()
    }
}

/// A coordinate in cells as the cell it is in.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a world coordinate over a cell of fifty metres or more is far inside an i32"
)]
fn cell_index(cells: f64) -> i32 {
    cells
        .floor()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}
