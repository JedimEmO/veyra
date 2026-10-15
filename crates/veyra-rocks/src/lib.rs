//! Veyra's rocks as ashlar props: boulders, scatter stones and cliff kits.
//!
//! The content step shapes every rock here from a [`RockRecipe`] and a
//! variant number and bakes the result: [`grow`] answers an
//! [`ashlar::prop::Prop`] with three mesh levels, convex hull colliders and
//! the one still wind pivot the tree shader expects, and the step appends
//! the impostor. Nothing here runs in the game; the game and the server read
//! the baked files, so the generator may take its time and use the
//! platform's trigonometry.
//!
//! A rock is a signed distance field, not a mesh: a rounded box or a few of
//! them, cut by fracture planes, warped a little at low frequency and, for
//! sandstone, quantised into strata. Each level of detail is the same field
//! meshed by dual contouring on a coarser grid, which is why a far level
//! keeps the near level's silhouette, and every vertex's shading data is
//! read from the field rather than from the level's own triangles, so a
//! level switch does not move the light.
//!
//! Cliff pieces face `+Z`: their fronts are the worked faces, and the
//! placement turns them to look down the slope they are sunk into.
pub mod review;

mod build;
mod field;
mod hull;
mod mesher;
mod noise;
mod rng;
mod shape;
mod species;

use ashlar::prop::Prop;

pub use hull::Hull;
pub use species::recipes;

/// Variants shaped per species. A cliff species' variants cycle through its
/// kit pieces: wall, buttress, ledge, then again with other seeds.
pub const VARIANTS: u32 = 8;

/// The kit pieces a cliff variant is, in the order variants cycle through.
pub const CLIFF_PIECES: [CliffPiece; 3] =
    [CliffPiece::Wall, CliffPiece::Buttress, CliffPiece::Ledge];

/// One species: what it is called, what it wears, and how it is shaped.
#[derive(Clone, Debug, PartialEq)]
pub struct RockRecipe {
    /// The registry id the runtime places this species by.
    pub species: &'static str,
    /// The rock material: the key is `rock:<material>`.
    pub material: &'static str,
    /// A loose rock or a cliff kit piece.
    pub kind: RockKind,
    /// Which rock it is, which picks the shape rules.
    pub geology: Geology,
    /// Which level of detail table it gets: a stone is gone long before a
    /// boulder, and a cliff is seen from kilometres away.
    pub detail: Detail,
    /// Size range in metres, spread over the variants: the longest
    /// horizontal extent of a boulder or stone, the height of a cliff piece
    /// above its base.
    pub size: [f64; 2],
    /// How much moss grows on it, 0 to 1.
    pub moss: f64,
}

impl RockRecipe {
    /// The species' short name, the part of its id after the namespace, which
    /// the prop id and the review files use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.species
            .rsplit_once(':')
            .map_or(self.species, |(_, name)| name)
    }

    /// The material key every piece binds.
    #[must_use]
    pub fn material_key(&self) -> String {
        format!("rock:{}", self.material)
    }

    /// The prop id of one variant.
    #[must_use]
    pub fn prop_id(&self, variant: u32) -> String {
        format!("rock:{}-{variant}", self.name())
    }

    /// Whether the content step appends an impostor level: a boulder or a
    /// cliff is still seen past its last mesh level, where a stone just
    /// fades out.
    #[must_use]
    pub fn impostor(&self) -> bool {
        self.detail != Detail::Stone
    }

    /// The kit piece a cliff variant is; `None` for a loose rock.
    #[must_use]
    pub fn piece(&self, variant: u32) -> Option<CliffPiece> {
        (self.kind == RockKind::Cliff).then(|| CLIFF_PIECES[variant as usize % CLIFF_PIECES.len()])
    }
}

/// What a rock is to the placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RockKind {
    /// A loose rock sitting in the ground, stones included.
    Boulder,
    /// A kit piece sunk into a slope and overlapped with its neighbours.
    Cliff,
}

/// The rock a species is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Geology {
    /// Rounded, weathered masses with a few fracture planes.
    Granite,
    /// Layered, stepped slabs with strata ledges and undercuts.
    Sandstone,
}

/// Which level of detail table a species gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Scatter under a metre.
    Stone,
    /// A boulder of a metre or more.
    Boulder,
    /// A cliff kit piece.
    Cliff,
}

impl Detail {
    /// Where each mesh level stops drawing, metres: inside the trees'
    /// bands, because the runtime streams level slots by them.
    #[must_use]
    pub fn until(self) -> [f32; 3] {
        match self {
            Self::Stone => [25.0, 60.0, 150.0],
            Self::Boulder | Self::Cliff => [60.0, 150.0, 350.0],
        }
    }

    /// Triangles per mesh level, at most.
    #[must_use]
    pub fn budget(self) -> [usize; 3] {
        match self {
            Self::Stone => [800, 250, 80],
            Self::Boulder => [3000, 900, 250],
            Self::Cliff => [12_000, 3_500, 1_000],
        }
    }

    /// Whether mesh level `level` casts shadows: the nearest always, and a
    /// cliff's second too, because a cliff's shadow is a landscape feature
    /// well past the distance a boulder's stops mattering.
    #[must_use]
    pub fn casts_shadows(self, level: usize) -> bool {
        level == 0 || (self == Self::Cliff && level == 1)
    }
}

/// A cliff kit piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliffPiece {
    /// A tall, wide face of jointed columns.
    Wall,
    /// A narrower, deeper prow that tapers to its top.
    Buttress,
    /// A wide, low piece with a broad top and a step in front.
    Ledge,
}

/// A shaped variant: the prop the content step writes, and what the
/// placement needs without reading a mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct GrownRock {
    /// Three mesh levels, the hulls and the `pivots` table.
    pub prop: Prop,
    /// The top of the nearest level above the ground, metres.
    pub height: f64,
    /// The horizontal radius of the nearest level at and above the ground,
    /// metres: what the placement keeps other things out of.
    pub radius: f64,
}

/// Shape variant `variant` (below [`VARIANTS`]) of a species. The same recipe
/// and variant give the same bytes every run.
///
/// # Panics
///
/// If `variant` is not below [`VARIANTS`].
#[must_use]
pub fn grow(recipe: &RockRecipe, variant: u32) -> GrownRock {
    assert!(variant < VARIANTS, "variant {variant} of {VARIANTS}");
    build::build(recipe, variant)
}

#[cfg(test)]
mod tests;
