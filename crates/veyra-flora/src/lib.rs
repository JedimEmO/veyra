//! Veyra's plants as ashlar props.
//!
//! The content step grows every tree here from a [`TreeRecipe`] and a variant
//! number and bakes the result: [`grow`] answers an [`ashlar::prop::Prop`]
//! with three mesh levels, a trunk capsule and the per-variant wind pivot
//! table, and the step appends the impostor. Nothing here runs in the game;
//! the game and the server read the baked files, so the generator may take
//! its time and use the platform's trigonometry.
//!
//! Broadleaves and shrubs grow by space colonisation inside an art-directed
//! crown envelope; conifers grow from whorls on a monopodial leader. Both
//! then share the botany that makes a tree read as grown rather than drawn:
//! pipe-model radii computed from the tips down, gravity bending
//! accumulated by age and load, shade pruning into dead stubs, and leaf
//! cards on the terminal shoots with normals taken from the crown's masses.
//!
//! The understory is baked the same way and keeps the same prop contract,
//! at two levels and with no impostor ([`TreeRecipe::impostor`]): a bush
//! grows as a shrub does, a fern is a rosette of fronds with leaflet cards
//! bent along each rachis, a flower clump is stems with petal geometry
//! coloured per vertex on [`PLAIN`], and a fallen log lies along +X with
//! moss in its vertex colours and one capsule for a collider.
//!
//! The [`capacity`] module is the synthetic fixture of the world slice
//! plan's Phase 8, kept so the runtime can still be measured at the budget
//! with trees that do not change when the generator does.
pub mod capacity;
pub mod review;

mod bark;
mod broadleaf;
mod conifer;
mod crown;
mod fern;
mod flower;
mod foliage;
mod levels;
mod log;
mod reed;
mod rng;
mod skeleton;
mod species;

use ashlar::prop::Prop;

pub use species::recipes;

/// Variants grown per species: 0 to 3 mature, 4 young, 5 old.
pub const VARIANTS: u32 = 6;

/// The first variant that is not mature: smaller, thinner and sparser.
pub const YOUNG: u32 = 4;

/// The old variant: bigger, gnarled, with dead and broken limbs.
pub const OLD: u32 = 5;

/// One species: what it is called, what it wears, and how it grows.
///
/// Every length is a fraction of the tree's height unless its name says
/// metres, so one recipe grows its young and its old variants alike.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRecipe {
    /// The flora registry id the runtime places this species by.
    pub species: &'static str,
    /// The bark set: the material key is `flora:bark-<bark>`.
    pub bark: &'static str,
    /// The leaf set: the material key is `flora:leaves-<leaves>`.
    pub leaves: &'static str,
    /// Which generator grows it.
    pub habit: Habit,
    /// Mature height range in metres (a log's length). The four mature
    /// variants spread over it, the young one sits below it and the old one
    /// above.
    pub height: [f64; 2],
    /// Trunk radius at breast height over the height, for a mature variant.
    pub trunk_ratio: f64,
    /// The crown envelope the growth fills.
    pub crown: CrownShape,
    /// The growth rules.
    pub growth: Growth,
    /// The leaf cards.
    pub foliage: Foliage,
    /// The axes an alien species will mutate. Every Veyra species today
    /// keeps them at [`Alien::EARTHLIKE`].
    pub alien: Alien,
    /// A flower's colours and petals; `None` for everything that does not
    /// bloom.
    pub bloom: Option<Bloom>,
}

impl TreeRecipe {
    /// The species' short name, the part of its id after the namespace, which
    /// the prop id and the review files use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.species
            .rsplit_once(':')
            .map_or(self.species, |(_, name)| name)
    }

    /// The bark material key the prop binds. A flower's and a reed's stems
    /// wear [`PLAIN`], coloured by their vertices.
    #[must_use]
    pub fn bark_material(&self) -> String {
        match self.habit {
            Habit::Flower | Habit::Reed => PLAIN.to_owned(),
            _ => format!("flora:bark-{}", self.bark),
        }
    }

    /// The leaf material key the prop binds. A flower's petals and leaves,
    /// and a reed's blades, wear [`PLAIN`], coloured by their vertices.
    #[must_use]
    pub fn leaf_material(&self) -> String {
        match self.habit {
            Habit::Flower | Habit::Reed => PLAIN.to_owned(),
            _ => format!("flora:leaves-{}", self.leaves),
        }
    }

    /// Whether the content step bakes an impostor for this species. Only
    /// trees and scrub are seen far enough away to need one; the understory
    /// fades out before any impostor distance.
    #[must_use]
    pub fn impostor(&self) -> bool {
        matches!(self.habit, Habit::Broadleaf | Habit::Conifer | Habit::Shrub)
    }

    /// The prop id of one variant.
    #[must_use]
    pub fn prop_id(&self, variant: u32) -> String {
        format!("flora:{}-{variant}", self.name())
    }
}

/// How a species grows, which picks the generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habit {
    /// One trunk that forks into a rounded crown: space colonisation.
    Broadleaf,
    /// A leader with whorls of branches: the whorl generator.
    Conifer,
    /// Several stems from the root into a low mound: space colonisation
    /// with no trunk, at the scrub's own levels of detail.
    Shrub,
    /// An understory bush: grown as a shrub, with the understory's two
    /// levels, no impostor and no collider.
    Bush,
    /// A rosette of arching fronds, each a chain of leaflet cards bent along
    /// its rachis.
    Fern,
    /// A clump of stems with petal geometry, coloured by vertex.
    Flower,
    /// A clump of tall strap blades and a few stems with seed spikes,
    /// coloured by vertex: the reeds of a wet bank.
    Reed,
    /// A fallen trunk lying along +X, with broken ends, a few stubs and moss.
    Log,
}

/// The envelope a crown fills, as fractions of the height.
#[derive(Clone, Debug, PartialEq)]
pub struct CrownShape {
    /// Height of the lowest foliage.
    pub base: f64,
    /// Horizontal radius at the widest point.
    pub radius: f64,
    /// Where the widest point sits between the base (0) and the top (1).
    pub widest: f64,
    /// Exponent of the envelope above the widest point: 2 is an ellipsoid,
    /// more is flatter-topped, less is pointed.
    pub top: f64,
    /// The same below the widest point.
    pub bottom: f64,
    /// How many foliage masses a crown gathers into, fewest and most. The
    /// masses and the gaps between them are the stylised read.
    pub clumps: [u32; 2],
    /// How far the outline wanders from the envelope, over its radius.
    pub lobes: f64,
}

/// Growth rules. Lengths are fractions of the height.
#[derive(Clone, Debug, PartialEq)]
pub struct Growth {
    /// Length of one growth step, the spacing of skeleton nodes.
    pub segment: f64,
    /// Attraction points space colonisation grows towards; unused by
    /// conifers.
    pub attractors: u32,
    /// Stems from the root; 1 is a tree with a trunk.
    pub stems: u32,
    /// Height the trunk rises to before the crown takes over.
    pub trunk: f64,
    /// Whorl spacing along a conifer's leader.
    pub whorl_spacing: f64,
    /// Branches per whorl, fewest and most.
    pub whorl_branches: [u32; 2],
    /// Side shoots per metre along a conifer's branch.
    pub laterals: f64,
    /// Angle of a side shoot off its branch, degrees.
    pub lateral_angle_deg: f64,
    /// Pull of growth towards the tropism target, per step.
    pub tropism: f64,
    /// Strength of the random walk on growth direction.
    pub wander: f64,
    /// The pipe model's exponent: a parent's radius to this power is the sum
    /// of its children's.
    pub pipe_exponent: f64,
    /// Gravity bending of old, loaded limbs.
    pub droop: f64,
    /// Hang of the youngest shoots: birch's weeping twigs, fir's tips.
    pub tip_droop: f64,
    /// Branches the shade kills into stubs on a mature tree.
    pub dead_stubs: u32,
    /// Chance that a variant's leader was lost and a side shoot took over.
    pub leader_change: f64,
    /// Root flare lobes, fewest and most; zero for a plain flare.
    pub buttress: [u32; 2],
    /// How far the root flare's lobes stand out, over the trunk radius.
    pub buttress_amplitude: f64,
}

/// The key both pieces of a flower bind: a plain white, two-sided
/// definition the content step adds, which the vertex colours paint.
pub const PLAIN: &str = "flora:plain";

/// A flower's colours and petals. Colours are linear RGB, multiplied into
/// [`PLAIN`]'s white by the runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloom {
    /// The petals.
    pub petal: [f32; 3],
    /// The disc in the middle of a flower.
    pub centre: [f32; 3],
    /// Stems and leaves.
    pub stem: [f32; 3],
    /// Petals per flower, fewest and most.
    pub petals: [u32; 2],
    /// Petal length over the plant's height.
    pub petal_length: f64,
    /// How far the petals cup up: 0 is a flat daisy, 1 a closed bell.
    pub cup: f64,
}

/// The leaf cards of a species.
#[derive(Clone, Debug, PartialEq)]
pub struct Foliage {
    /// Card edge over the height.
    pub card_size: f64,
    /// Cards at the nearest level of a mature variant, before the budget.
    pub cards: u32,
    /// How flat the cards lie: 0 faces out of the crown, 1 faces the sky
    /// (a fir's sprays).
    pub flatness: f64,
    /// Cards per shoot tip, as a pine's needle tufts are.
    pub tuft: u32,
}

/// The axes of an alien species. Earthlike values reproduce the familiar
/// botany; the game changes one or two per alien species, never all of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Alien {
    /// Angle between successive branches around a stem, degrees. The golden
    /// angle packs them without overlap, as most plants do.
    pub phyllotaxis_deg: f64,
    /// The direction growth bends towards: up, for light.
    pub tropism_target: [f64; 3],
    /// Gravity over Veyra's standard, feeding droop.
    pub gravity: f64,
    /// The stem cross-section.
    pub section: Superformula,
}

impl Alien {
    /// Familiar botany: the golden angle, light from above, standard gravity
    /// and round stems.
    pub const EARTHLIKE: Self = Self {
        phyllotaxis_deg: 137.507_764,
        tropism_target: [0.0, 1.0, 0.0],
        gravity: 1.0,
        section: Superformula::CIRCLE,
    };
}

/// Gielis' superformula, the stem cross-section as radius over angle:
/// `r(t) = (|cos(m t / 4) / a|^n2 + |sin(m t / 4) / b|^n3)^(-1 / n1)`,
/// normalised so its largest radius is one.
#[derive(Clone, Debug, PartialEq)]
pub struct Superformula {
    /// Rotational symmetry: lobes around the stem.
    pub m: f64,
    /// Overall exponent.
    pub n1: f64,
    /// Cosine exponent.
    pub n2: f64,
    /// Sine exponent.
    pub n3: f64,
    /// Cosine scale.
    pub a: f64,
    /// Sine scale.
    pub b: f64,
}

impl Superformula {
    /// A circle: no lobes at all.
    pub const CIRCLE: Self = Self {
        m: 0.0,
        n1: 2.0,
        n2: 2.0,
        n3: 2.0,
        a: 1.0,
        b: 1.0,
    };

    /// Radius at angle `t`, unnormalised.
    #[must_use]
    pub fn radius(&self, t: f64) -> f64 {
        let q = self.m * t / 4.0;
        let c = (q.cos() / self.a).abs().powf(self.n2);
        let s = (q.sin() / self.b).abs().powf(self.n3);
        (c + s).max(1e-9).powf(-1.0 / self.n1)
    }
}

/// A grown variant: the prop the content step writes and the height the
/// placement scales against.
#[derive(Clone, Debug, PartialEq)]
pub struct GrownTree {
    /// Three mesh levels, the trunk capsule and the `pivots` table.
    pub prop: Prop,
    /// The top of the nearest level above the ground, metres.
    pub height: f64,
}

/// Grow variant `variant` (below [`VARIANTS`]) of a species. The same recipe
/// and variant give the same bytes every run.
///
/// # Panics
///
/// If `variant` is not below [`VARIANTS`].
#[must_use]
pub fn grow(recipe: &TreeRecipe, variant: u32) -> GrownTree {
    assert!(variant < VARIANTS, "variant {variant} of {VARIANTS}");
    levels::grow(recipe, variant)
}

#[cfg(test)]
mod tests;
