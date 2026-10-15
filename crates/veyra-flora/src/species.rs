//! Veyra's species, and what a variant number makes of one.
use ashlar::glam::DVec2;

use crate::rng::{Rng, hash};
use crate::{Alien, Bloom, CrownShape, Foliage, Growth, Habit, OLD, TreeRecipe, YOUNG};

/// Every species the content step grows: the trees and scrub, then the
/// understory.
#[must_use]
pub fn recipes() -> Vec<TreeRecipe> {
    vec![
        oak(),
        pine(),
        birch(),
        fir(),
        scrub(),
        fern(),
        amber(),
        violet(),
        woodland_shrub(),
        log(),
        reed(),
    ]
}

/// Reeds on the wet bank of a lake or a wide river (ticket 0005, step 8):
/// tall olive blades, some dried to straw, and dark brown seed spikes.
/// `growth.stems` is the spiked stems a mature clump has.
fn reed() -> TreeRecipe {
    TreeRecipe {
        species: "strata:reed",
        bark: "plain",
        leaves: "plain",
        habit: Habit::Reed,
        height: [1.4, 2.1],
        trunk_ratio: 0.0045,
        crown: CrownShape {
            base: 0.0,
            radius: 0.22,
            widest: 0.75,
            top: 2.0,
            bottom: 2.0,
            clumps: [3, 4],
            lobes: 0.0,
        },
        growth: Growth {
            segment: 0.1,
            stems: 6,
            ..UNUSED_GROWTH
        },
        foliage: Foliage {
            card_size: 0.3,
            cards: 0,
            flatness: 0.0,
            tuft: 1,
        },
        alien: Alien::EARTHLIKE,
        bloom: Some(Bloom {
            petal: [0.22, 0.12, 0.05],
            centre: [0.22, 0.12, 0.05],
            stem: [0.24, 0.32, 0.1],
            petals: [0, 0],
            petal_length: 0.0,
            cup: 0.0,
        }),
    }
}

fn oak() -> TreeRecipe {
    TreeRecipe {
        species: "strata:oak",
        bark: "oak",
        leaves: "oak",
        habit: Habit::Broadleaf,
        height: [8.0, 12.0],
        trunk_ratio: 0.034,
        crown: CrownShape {
            base: 0.27,
            radius: 0.46,
            widest: 0.42,
            top: 2.2,
            bottom: 1.6,
            clumps: [5, 8],
            lobes: 0.2,
        },
        growth: Growth {
            segment: 0.034,
            attractors: 2600,
            stems: 1,
            trunk: 0.30,
            whorl_spacing: 0.0,
            whorl_branches: [0, 0],
            laterals: 0.0,
            lateral_angle_deg: 0.0,
            tropism: 0.10,
            wander: 0.40,
            pipe_exponent: 2.5,
            droop: 0.10,
            tip_droop: 0.0,
            dead_stubs: 3,
            leader_change: 0.5,
            buttress: [3, 5],
            buttress_amplitude: 0.55,
        },
        foliage: Foliage {
            card_size: 0.085,
            cards: 1150,
            flatness: 0.15,
            tuft: 3,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

fn pine() -> TreeRecipe {
    TreeRecipe {
        species: "strata:pine",
        bark: "pine",
        leaves: "pine",
        habit: Habit::Conifer,
        height: [12.0, 17.0],
        trunk_ratio: 0.022,
        crown: CrownShape {
            base: 0.52,
            radius: 0.20,
            widest: 0.35,
            top: 1.6,
            bottom: 1.4,
            clumps: [5, 9],
            lobes: 0.3,
        },
        growth: Growth {
            segment: 0.025,
            attractors: 0,
            stems: 1,
            trunk: 1.0,
            whorl_spacing: 0.055,
            whorl_branches: [3, 5],
            laterals: 2.2,
            lateral_angle_deg: 50.0,
            tropism: 0.25,
            wander: 0.35,
            pipe_exponent: 2.4,
            droop: 0.10,
            tip_droop: 0.0,
            dead_stubs: 6,
            leader_change: 0.35,
            buttress: [0, 0],
            buttress_amplitude: 0.15,
        },
        foliage: Foliage {
            card_size: 0.052,
            cards: 1100,
            flatness: 0.25,
            tuft: 4,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

fn birch() -> TreeRecipe {
    TreeRecipe {
        species: "strata:birch",
        bark: "birch",
        leaves: "birch",
        habit: Habit::Broadleaf,
        height: [10.0, 14.0],
        trunk_ratio: 0.016,
        crown: CrownShape {
            base: 0.24,
            radius: 0.21,
            widest: 0.45,
            top: 1.7,
            bottom: 2.0,
            clumps: [12, 18],
            lobes: 0.22,
        },
        growth: Growth {
            segment: 0.021,
            attractors: 3400,
            stems: 1,
            trunk: 0.8,
            whorl_spacing: 0.0,
            whorl_branches: [0, 0],
            laterals: 0.0,
            lateral_angle_deg: 0.0,
            tropism: 0.20,
            wander: 0.30,
            pipe_exponent: 2.2,
            droop: 0.08,
            tip_droop: 0.8,
            dead_stubs: 3,
            leader_change: 0.2,
            buttress: [0, 0],
            buttress_amplitude: 0.1,
        },
        foliage: Foliage {
            card_size: 0.045,
            cards: 1500,
            flatness: 0.0,
            tuft: 2,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

fn fir() -> TreeRecipe {
    TreeRecipe {
        species: "strata:fir",
        bark: "fir",
        leaves: "fir",
        habit: Habit::Conifer,
        height: [11.0, 16.0],
        trunk_ratio: 0.026,
        crown: CrownShape {
            base: 0.07,
            radius: 0.24,
            widest: 0.1,
            top: 1.0,
            bottom: 2.0,
            clumps: [8, 12],
            lobes: 0.12,
        },
        growth: Growth {
            segment: 0.022,
            attractors: 0,
            stems: 1,
            trunk: 1.0,
            whorl_spacing: 0.04,
            whorl_branches: [4, 6],
            laterals: 1.6,
            lateral_angle_deg: 60.0,
            tropism: 0.05,
            wander: 0.2,
            pipe_exponent: 2.4,
            droop: 0.12,
            tip_droop: 0.5,
            dead_stubs: 2,
            leader_change: 0.1,
            buttress: [0, 0],
            buttress_amplitude: 0.2,
        },
        foliage: Foliage {
            card_size: 0.05,
            cards: 1300,
            flatness: 0.8,
            tuft: 1,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

fn scrub() -> TreeRecipe {
    TreeRecipe {
        species: "strata:scrub",
        bark: "shrub",
        leaves: "shrub",
        habit: Habit::Shrub,
        height: [1.0, 1.7],
        trunk_ratio: 0.018,
        crown: CrownShape {
            base: 0.12,
            radius: 0.62,
            widest: 0.45,
            top: 1.8,
            bottom: 1.5,
            clumps: [4, 7],
            lobes: 0.2,
        },
        growth: Growth {
            segment: 0.06,
            attractors: 900,
            stems: 5,
            trunk: 0.0,
            whorl_spacing: 0.0,
            whorl_branches: [0, 0],
            laterals: 0.0,
            lateral_angle_deg: 0.0,
            tropism: 0.15,
            wander: 0.45,
            pipe_exponent: 2.3,
            droop: 0.15,
            tip_droop: 0.0,
            dead_stubs: 2,
            leader_change: 0.0,
            buttress: [0, 0],
            buttress_amplitude: 0.0,
        },
        foliage: Foliage {
            card_size: 0.24,
            cards: 300,
            flatness: 0.1,
            tuft: 2,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

/// Growth rules that a generator does not read, for the understory's
/// recipes to fill in around the ones it does.
const UNUSED_GROWTH: Growth = Growth {
    segment: 0.1,
    attractors: 0,
    stems: 1,
    trunk: 0.0,
    whorl_spacing: 0.0,
    whorl_branches: [0, 0],
    laterals: 0.0,
    lateral_angle_deg: 0.0,
    tropism: 0.0,
    wander: 0.3,
    pipe_exponent: 2.4,
    droop: 0.0,
    tip_droop: 0.0,
    dead_stubs: 0,
    leader_change: 0.0,
    buttress: [0, 0],
    buttress_amplitude: 0.0,
};

/// A ground fern. `growth.stems` is its fronds, `growth.droop` how far they
/// arch over, `crown.radius` how far they spread, and `foliage.card_size`
/// one leaflet card of a frond.
fn fern() -> TreeRecipe {
    TreeRecipe {
        species: "strata:woodland_fern",
        bark: "shrub",
        leaves: "fern",
        habit: Habit::Fern,
        height: [0.5, 1.1],
        trunk_ratio: 0.009,
        crown: CrownShape {
            base: 0.0,
            radius: 0.9,
            widest: 0.5,
            top: 2.0,
            bottom: 2.0,
            clumps: [4, 6],
            lobes: 0.0,
        },
        growth: Growth {
            segment: 0.09,
            stems: 9,
            droop: 0.7,
            ..UNUSED_GROWTH
        },
        foliage: Foliage {
            card_size: 0.3,
            cards: 0,
            flatness: 0.0,
            tuft: 1,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

/// A wildflower clump: `growth.stems` flowering stems spread over
/// `crown.radius`, each with a head of `bloom` petals.
fn flower(species: &'static str, stems: u32, bloom: Bloom) -> TreeRecipe {
    TreeRecipe {
        species,
        bark: "plain",
        leaves: "plain",
        habit: Habit::Flower,
        height: [0.3, 0.6],
        trunk_ratio: 0.006,
        crown: CrownShape {
            base: 0.0,
            radius: 0.35,
            widest: 0.7,
            top: 2.0,
            bottom: 2.0,
            clumps: [3, 5],
            lobes: 0.0,
        },
        growth: Growth {
            segment: 0.15,
            stems,
            ..UNUSED_GROWTH
        },
        foliage: Foliage {
            card_size: 0.3,
            cards: 0,
            flatness: 0.0,
            tuft: 1,
        },
        alien: Alien::EARTHLIKE,
        bloom: Some(bloom),
    }
}

/// Amber daisies: many narrow petals, open flat round a dark centre.
fn amber() -> TreeRecipe {
    flower(
        "strata:wildflower_amber",
        7,
        Bloom {
            petal: [1.0, 0.55, 0.08],
            centre: [0.28, 0.12, 0.03],
            stem: [0.12, 0.3, 0.06],
            petals: [10, 13],
            petal_length: 0.1,
            cup: 0.12,
        },
    )
}

/// Violet cups: five or six broad petals round a yellow eye.
fn violet() -> TreeRecipe {
    flower(
        "strata:wildflower_violet",
        6,
        Bloom {
            petal: [0.45, 0.2, 0.85],
            centre: [0.9, 0.72, 0.12],
            stem: [0.12, 0.3, 0.06],
            petals: [5, 6],
            petal_length: 0.08,
            cup: 0.55,
        },
    )
}

/// A lush understory bush: the scrub's growth with more leaf, at the
/// understory's levels.
fn woodland_shrub() -> TreeRecipe {
    TreeRecipe {
        species: "strata:woodland_shrub",
        bark: "shrub",
        leaves: "shrub",
        habit: Habit::Bush,
        height: [0.5, 1.15],
        trunk_ratio: 0.02,
        crown: CrownShape {
            base: 0.08,
            radius: 0.7,
            widest: 0.4,
            top: 1.8,
            bottom: 1.4,
            clumps: [4, 6],
            lobes: 0.22,
        },
        growth: Growth {
            segment: 0.075,
            attractors: 1_000,
            stems: 5,
            tropism: 0.12,
            wander: 0.45,
            pipe_exponent: 2.3,
            droop: 0.2,
            dead_stubs: 1,
            ..UNUSED_GROWTH
        },
        foliage: Foliage {
            card_size: 0.28,
            cards: 260,
            flatness: 0.1,
            tuft: 3,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

/// A fallen log. `height` is its length, `trunk_ratio` its radius over the
/// length, `growth.stems` its stub branches, and `foliage` the few leafy
/// sprouts on it.
fn log() -> TreeRecipe {
    TreeRecipe {
        species: "strata:woodland_log",
        bark: "oak",
        leaves: "shrub",
        habit: Habit::Log,
        height: [2.0, 3.5],
        trunk_ratio: 0.075,
        crown: CrownShape {
            base: 0.0,
            radius: 0.1,
            widest: 0.5,
            top: 2.0,
            bottom: 2.0,
            clumps: [2, 3],
            lobes: 0.0,
        },
        growth: Growth {
            segment: 0.035,
            stems: 2,
            ..UNUSED_GROWTH
        },
        foliage: Foliage {
            card_size: 0.08,
            cards: 14,
            flatness: 0.0,
            tuft: 1,
        },
        alien: Alien::EARTHLIKE,
        bloom: None,
    }
}

/// How old a variant is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Age {
    Young,
    Mature,
    Old,
}

/// A recipe made concrete for one variant: metres instead of fractions, and
/// the variant's own character. The four mature variants each lean towards
/// an archetype (upright, leaning, tall and narrow, low and wide) so they
/// read apart at a distance and not only in their details.
#[derive(Clone, Debug)]
pub(crate) struct Form {
    pub age: Age,
    pub height: f64,
    pub trunk_radius: f64,
    pub trunk_height: f64,
    pub crown_base: f64,
    pub crown_radius: f64,
    pub widest: f64,
    pub top: f64,
    pub bottom: f64,
    /// Horizontal offset of the crown's top from the trunk, metres.
    pub lean: DVec2,
    pub lobes: [(f64, f64); 3],
    pub clumps: u32,
    pub attractors: u32,
    pub segment: f64,
    pub droop: f64,
    pub tip_droop: f64,
    pub dead_stubs: u32,
    pub broken: u32,
    pub dead_limbs: u32,
    pub gnarl: f64,
    pub cards: u32,
    pub card_size: f64,
    pub leader_change: bool,
    pub stems: u32,
    pub whorl_spacing: f64,
    pub buttress: u32,
    pub seed: u64,
}

impl Form {
    pub(crate) fn new(recipe: &TreeRecipe, variant: u32) -> Self {
        let seed = hash(recipe.species) ^ u64::from(variant).wrapping_mul(0x2545_F491_4F6C_DD1D);
        let mut rng = Rng::new(seed).fork("form");
        let age = match variant {
            YOUNG => Age::Young,
            OLD => Age::Old,
            _ => Age::Mature,
        };
        // Archetypes: place in the height range, crown width, crown base,
        // lean over the height.
        let archetype = match variant {
            0 => [0.45, 1.0, 1.0, 0.02],
            1 => [0.65, 0.95, 1.05, 0.10],
            2 => [0.95, 0.78, 1.2, 0.03],
            3 => [0.15, 1.22, 0.8, 0.05],
            YOUNG => [0.0, 0.8, 0.9, 0.04],
            _ => [1.0, 1.18, 0.95, 0.05],
        };
        let [low, high] = recipe.height;
        let height = match age {
            Age::Young => low * rng.range(0.45, 0.55),
            Age::Mature => {
                low + (high - low) * (archetype[0] + rng.signed() * 0.08).clamp(0.0, 1.0)
            }
            Age::Old => high * rng.range(1.08, 1.15),
        };
        let crown = &recipe.crown;
        let growth = &recipe.growth;
        let foliage = &recipe.foliage;
        let (trunk_scale, density, droop, dead, broken, dead_limbs, gnarl) = match age {
            Age::Young => (0.75, 0.55, 0.5, 0, 0, 0, 0.0),
            Age::Mature => (1.0, 1.0, 1.0, growth.dead_stubs, 0, 0, 0.15),
            Age::Old => (1.35, 1.05, 1.7, growth.dead_stubs * 2, 2, 1, 1.0),
        };
        let width = archetype[1] * rng.range(0.92, 1.08);
        let turn = rng.range(0.0, core::f64::consts::TAU);
        let lean =
            DVec2::new(turn.cos(), turn.sin()) * height * (archetype[3] + rng.range(0.0, 0.03));
        let lobes = [
            (
                crown.lobes * rng.range(0.6, 1.2),
                rng.range(0.0, core::f64::consts::TAU),
            ),
            (
                crown.lobes * rng.range(0.4, 0.9),
                rng.range(0.0, core::f64::consts::TAU),
            ),
            (
                crown.lobes * rng.range(0.2, 0.6),
                rng.range(0.0, core::f64::consts::TAU),
            ),
        ];
        let [fewest, most] = crown.clumps;
        let clumps = match age {
            Age::Young => fewest.saturating_sub(2).max(2),
            Age::Mature => rng.between(fewest, most),
            Age::Old => most + 1,
        };
        let [lobes_low, lobes_high] = growth.buttress;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let scaled = |count: u32, by: f64| (f64::from(count) * by).round() as u32;
        Self {
            age,
            height,
            trunk_radius: height * recipe.trunk_ratio * trunk_scale * rng.range(0.92, 1.08),
            trunk_height: height * (growth.trunk * archetype[2] * rng.range(0.9, 1.1)).min(0.85),
            crown_base: height * crown.base * archetype[2] * rng.range(0.9, 1.1),
            crown_radius: height * crown.radius * width,
            widest: crown.widest,
            top: crown.top,
            bottom: crown.bottom,
            lean,
            lobes,
            clumps,
            attractors: scaled(growth.attractors, density),
            segment: height * growth.segment,
            droop: growth.droop * droop * recipe.alien.gravity,
            tip_droop: growth.tip_droop * recipe.alien.gravity,
            dead_stubs: dead,
            broken,
            dead_limbs,
            gnarl,
            cards: scaled(foliage.cards, density),
            card_size: height * foliage.card_size * if age == Age::Young { 1.15 } else { 1.0 },
            leader_change: age != Age::Young && rng.chance(growth.leader_change),
            stems: match age {
                _ if growth.stems <= 1 => 1,
                Age::Young => growth.stems.saturating_sub(1).max(1),
                Age::Mature => growth.stems + rng.between(0, 2).saturating_sub(1),
                Age::Old => growth.stems + 2,
            },
            whorl_spacing: height * growth.whorl_spacing,
            buttress: if lobes_high == 0 {
                0
            } else {
                rng.between(lobes_low, lobes_high)
            },
            seed,
        }
    }

    /// The generator's stream for this variant.
    pub(crate) fn rng(&self, part: &str) -> Rng {
        Rng::new(self.seed).fork(part)
    }
}
