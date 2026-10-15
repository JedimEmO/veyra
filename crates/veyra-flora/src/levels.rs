//! From a recipe to a prop: grow the skeleton, apply the botany, place the
//! cards, then mesh three levels from the one tree and give every vertex
//! its wind and shade data. The understory's own generators (`fern`,
//! `flower`, `log`) build their geometry and finish it through the same
//! [`Piece`], pivot table and shade pass here, so every prop keeps one
//! contract.
//!
//! Coarser levels are the same tree simplified, never a different one:
//! the same skeleton with fewer sides and rings and the thin branches
//! pruned, and the same cards merged into fewer, larger ones. Shade data
//! comes from the nearest level for all three, so a level switch does not
//! change how light falls on the crown.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::DVec3;
use ashlar::prop::{Prop, PropCollider, PropLevel, PropPiece};
use ashlar::{FaceSource, TriangleMesh, VertexData};

use crate::bark::{self, BarkDetail, BarkMesh};
use crate::crown::{CrownField, Occupancy};
use crate::foliage::{self, Card, Outward, STEM};
use crate::rng::Rng;
use crate::skeleton::{Bending, Branch, Skeleton};
use crate::species::Form;
use crate::{GrownTree, Habit, TreeRecipe, broadleaf, conifer, fern, flower, log, reed};

/// Most pivots per variant: the wind table indexes them with a byte.
pub(crate) const MAX_PIVOTS: usize = 255;

/// Floats per pivot in `tables["pivots"]`.
pub(crate) const PIVOT_STRIDE: usize = 12;

/// Where each level stops drawing, metres.
pub(crate) const TREE_UNTIL: [f32; 3] = [60.0, 150.0, 350.0];
/// See [`TREE_UNTIL`]; a shrub is small and gone sooner.
pub(crate) const SHRUB_UNTIL: [f32; 3] = [40.0, 100.0, 200.0];
/// Triangles per level for a tree, at most.
pub(crate) const TREE_BUDGET: [usize; 3] = [15_000, 5_500, 1_700];
/// Triangles per level for a shrub, at most.
pub(crate) const SHRUB_BUDGET: [usize; 3] = [4_000, 1_500, 500];
/// Where a bush's and a log's levels stop drawing: the understory has two
/// levels and fades out before any impostor distance.
pub(crate) const BUSH_UNTIL: [f32; 2] = [40.0, 150.0];
/// Triangles per level for a bush or a log, at most.
pub(crate) const BUSH_BUDGET: [usize; 2] = [2_500, 650];
/// Where a fern's and a flower's levels stop drawing.
pub(crate) const SMALL_UNTIL: [f32; 2] = [25.0, 90.0];
/// Triangles per level for a fern or a flower, at most.
pub(crate) const SMALL_BUDGET: [usize; 2] = [1_500, 400];
/// Where a reed's levels stop drawing: a lake shore's fringe reads from a
/// couple of hundred metres, where a flower's clump is long gone.
pub(crate) const REED_UNTIL: [f32; 2] = [60.0, 240.0];
/// Triangles per level for a reed, at most: its far level is a few blades
/// as strips, drawn by the hundred along a shore.
pub(crate) const REED_BUDGET: [usize; 2] = [1_500, 360];

/// The level distances and triangle budgets of a habit, nearest first.
pub(crate) fn plan(habit: Habit) -> (&'static [f32], &'static [usize]) {
    match habit {
        Habit::Broadleaf | Habit::Conifer => (&TREE_UNTIL, &TREE_BUDGET),
        Habit::Shrub => (&SHRUB_UNTIL, &SHRUB_BUDGET),
        Habit::Bush | Habit::Log => (&BUSH_UNTIL, &BUSH_BUDGET),
        Habit::Fern | Habit::Flower => (&SMALL_UNTIL, &SMALL_BUDGET),
        Habit::Reed => (&REED_UNTIL, &REED_BUDGET),
    }
}

/// How far card shading normals lean on the crown's envelope normal.
pub(crate) const ENVELOPE_SHARE: f64 = 0.85;

/// What one level is made of.
struct LevelSpec {
    budget: usize,
    bark: BarkDetail,
    /// Cards as a share of the nearest level's.
    cards: f64,
    /// Cupped cards (eight triangles) rather than flat quads (two).
    cupped: bool,
}

fn specs(recipe: &TreeRecipe, form: &Form) -> Vec<LevelSpec> {
    let (until, budget) = plan(recipe.habit);
    let small = matches!(recipe.habit, Habit::Shrub | Habit::Bush);
    let r = form.trunk_radius;
    let scale = form.height / 10.0;
    let near = LevelSpec {
        budget: budget[0],
        bark: BarkDetail {
            max_sides: 12,
            turn: 0.14,
            spacing: 6.0,
            spacing_max: 1.2 * scale,
            spacing_min: form.segment * 2.2,
            min_radius: 0.0,
            fine_base: true,
        },
        cards: 1.0,
        cupped: true,
    };
    let middle = LevelSpec {
        budget: budget[1],
        bark: BarkDetail {
            max_sides: 7,
            turn: 0.32,
            spacing: 12.0,
            spacing_max: 2.5 * scale,
            spacing_min: form.segment * 4.0,
            min_radius: r * if small { 0.35 } else { 0.14 },
            fine_base: false,
        },
        cards: 0.75,
        cupped: false,
    };
    let far = LevelSpec {
        budget: *budget.last().expect("a level"),
        bark: BarkDetail {
            max_sides: 5,
            turn: 0.7,
            spacing: 30.0,
            spacing_max: 6.0 * scale,
            spacing_min: form.segment * 8.0,
            min_radius: r * if small { 0.6 } else { 0.3 },
            fine_base: false,
        },
        cards: 0.3,
        cupped: false,
    };
    // A bush's second level is already the far one.
    if until.len() == 2 {
        vec![near, far]
    } else {
        vec![near, middle, far]
    }
}

/// A grown tree with what the tests check beside the prop.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the tests read what the prop does not carry")
)]
pub(crate) struct Built {
    pub tree: GrownTree,
    /// Per level, the bark rings: first vertex, vertex count, `u` repeats.
    pub rings: Vec<Vec<(usize, usize, f64)>>,
    pub field: CrownField,
    /// Pipe-model checks: per fork, the parent's radius to the exponent
    /// over the sum of its children's.
    pub pipe_ratios: Vec<f64>,
    /// The distance from each nearest-level card's stem to its node.
    pub card_offsets: Vec<f64>,
}

pub(crate) fn grow(recipe: &TreeRecipe, variant: u32) -> GrownTree {
    build(recipe, variant).tree
}

pub(crate) fn build(recipe: &TreeRecipe, variant: u32) -> Built {
    match recipe.habit {
        Habit::Fern => fern::build(recipe, variant),
        Habit::Flower => flower::build(recipe, variant),
        Habit::Reed => reed::build(recipe, variant),
        Habit::Log => log::build(recipe, variant),
        Habit::Broadleaf | Habit::Conifer | Habit::Shrub | Habit::Bush => tree(recipe, variant),
    }
}

#[allow(clippy::too_many_lines, reason = "the pipeline reads best in order")]
fn tree(recipe: &TreeRecipe, variant: u32) -> Built {
    let form = Form::new(recipe, variant);
    let exponent = recipe.growth.pipe_exponent;
    let taper = 0.3 / form.segment;

    // Growth and botany.
    let (mut skeleton, outward, crown_centre) = match recipe.habit {
        Habit::Broadleaf | Habit::Shrub | Habit::Bush => {
            let (mut skeleton, envelope) = broadleaf::grow(recipe, &form);
            skeleton.compute_ages();
            set_radii(&mut skeleton, &form, exponent, taper);
            broadleaf::prune(&mut skeleton, &form, &envelope);
            let centre = envelope.centre();
            (skeleton, Outward::Centre(centre), centre)
        }
        _ => {
            let mut skeleton = conifer::grow(recipe, &form);
            set_radii(&mut skeleton, &form, exponent, taper);
            let lean = [form.lean.x / form.height, form.lean.y / form.height];
            let mid = form.crown_base.midpoint(form.height);
            let centre = DVec3::new(lean[0] * mid, mid, lean[1] * mid);
            (
                skeleton,
                Outward::Axis {
                    lean_per_metre: lean,
                },
                centre,
            )
        }
    };
    let pipe_ratios = pipe_ratios(&skeleton, exponent);
    skeleton.bend(&Bending {
        droop: form.droop,
        age: form.height * 0.3,
        stiff: form.trunk_radius * 0.6,
        tip: form.tip_droop,
        tip_age: form.segment * 3.0,
        max_step: 0.15,
        floor: form.height * 0.02,
    });
    skeleton.smooth(2, 0.5);
    skeleton.compute_ages();
    let branches = skeleton.branches();
    let mut rng = form.rng("pivots");
    let pivots = Pivots::for_tree(&skeleton, &branches, &form, &mut rng);

    // Foliage and the crown it makes.
    let cards = foliage::place(recipe, &form, &skeleton, outward);
    let field = CrownField::new(
        &cards,
        crown_centre,
        recipe.habit == Habit::Conifer,
        form.clumps,
    );
    let card_offsets = cards
        .iter()
        .map(|c| c.attach.distance(skeleton.nodes[c.node].pos))
        .collect();

    let tree = bark::Tree {
        recipe,
        form: &form,
        skeleton: &skeleton,
        branches: &branches,
        pivots: &pivots.of_branch,
        tile: bark_tile(recipe),
        root_flare: true,
        cap_base: false,
        splinter: 0.0,
    };
    let node_branch = node_branches(&skeleton, &branches);
    let occupancy = occupancy(&skeleton, &cards, &form);

    let mut levels = Vec::with_capacity(3);
    let mut rings = Vec::with_capacity(3);
    let mut merge_rng = form.rng("merge");
    for (index, spec) in specs(recipe, &form).into_iter().enumerate() {
        let mut detail = spec.bark.clone();
        let per_card = if spec.cupped { 8 } else { 2 };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let wanted = ((cards.len() as f64 * spec.cards).round() as usize).max(1);
        // Coarsen the bark until the level leaves room for most of its cards.
        let mut bark_mesh = bark::mesh(&tree, &detail);
        for _ in 0..12 {
            let room = spec.budget.saturating_sub(bark_mesh.mesh.triangle_count()) / per_card;
            if room * 10 >= wanted * 8 {
                break;
            }
            detail.min_radius = (detail.min_radius * 1.5).max(form.trunk_radius * 0.03);
            detail.spacing *= 1.3;
            detail.spacing_max *= 1.3;
            bark_mesh = bark::mesh(&tree, &detail);
        }
        let room = spec.budget.saturating_sub(bark_mesh.mesh.triangle_count()) / per_card;
        let count = wanted.min(room);
        let level_cards = if index == 0 {
            cards[..count.min(cards.len())].to_vec()
        } else {
            foliage::merge(&cards, count, outward, &mut merge_rng)
        };
        let (bark_piece, level_rings) = Piece::from_bark(bark_mesh);
        rings.push(level_rings);
        let leaf_piece = card_mesh(&level_cards, spec.cupped, &node_branch, &pivots, &field);
        let shade = Shade {
            pivots: &pivots,
            field: &field,
            occupancy: &occupancy,
            height: form.height,
        };
        levels.push([bark_piece.finish(&shade), leaf_piece.finish(&shade)]);
    }

    let colliders = collider(recipe, &skeleton, &branches, &form)
        .into_iter()
        .collect();
    Built {
        tree: assemble(recipe, variant, levels, colliders, pivots),
        rings,
        field,
        pipe_ratios,
        card_offsets,
    }
}

/// The prop from its levels' two finished pieces each, bark then leaves,
/// at the habit's distances. Only the nearest level casts: a far level's
/// shadow is under a shadow texel and costs a pass (the foliage spike).
pub(crate) fn assemble(
    recipe: &TreeRecipe,
    variant: u32,
    levels: Vec<[TriangleMesh; 2]>,
    colliders: Vec<PropCollider>,
    pivots: Pivots,
) -> GrownTree {
    let (until, _) = plan(recipe.habit);
    // Only the nearest level casts, and only a tree's or a shrub's: the
    // understory (a bush, a fern, a flower, a log) throws a shadow a hand
    // across that ambient occlusion already draws, and every caster is drawn
    // again into each shadow cascade it reaches. A street of planted verges
    // cost the render thread 1.8 ms in its shadows.
    let casts = matches!(
        recipe.habit,
        Habit::Broadleaf | Habit::Conifer | Habit::Shrub
    );
    let levels = levels
        .into_iter()
        .zip(until)
        .enumerate()
        .map(|(index, ([bark, leaves], &until))| PropLevel {
            until: Some(until),
            cast_shadows: casts && index == 0,
            pieces: vec![
                PropPiece {
                    mesh: bark,
                    material: recipe.bark_material(),
                },
                PropPiece {
                    mesh: leaves,
                    material: recipe.leaf_material(),
                },
            ],
            impostor: None,
        })
        .collect();
    let mut prop = Prop::new(recipe.prop_id(variant), levels, colliders);
    prop.tables.insert("pivots".to_owned(), pivots.table);
    let height = prop.levels[0]
        .pieces
        .iter()
        .flat_map(|p| p.mesh.positions.iter())
        .fold(0.0_f64, |h, p| h.max(p.y));
    GrownTree { prop, height }
}

/// The bark tile of a species, `u` around by `v` along, metres. A flower's
/// stems wear a plain colour, so any tile will do; a short one keeps its
/// `u` repeats honest.
pub(crate) fn bark_tile(recipe: &TreeRecipe) -> [f64; 2] {
    if matches!(recipe.habit, Habit::Flower | Habit::Reed) {
        return [0.05, 0.1];
    }
    let bark = recipe.bark;
    veyra_materials::FLORA_BARK
        .iter()
        .find(|(name, _)| *name == bark)
        .map_or_else(
            || panic!("bark set {bark:?} is not in veyra_materials::FLORA_BARK"),
            |(_, [u, v])| [f64::from(*u), f64::from(*v)],
        )
}

/// Pipe radii scaled so the trunk has the form's radius at breast height,
/// or, for a shrub, so each stem does.
fn set_radii(skeleton: &mut Skeleton, form: &Form, exponent: f64, taper: f64) {
    if skeleton.multi_stem {
        #[allow(clippy::cast_precision_loss)]
        let stems = skeleton.nodes[0].children.len().max(1) as f64;
        skeleton.pipe_radii(
            exponent,
            taper,
            0,
            form.trunk_radius * stems.powf(1.0 / exponent),
        );
        return;
    }
    let breast = 1.3_f64.min(form.height * 0.15);
    let trunk = foliage::trunk_nodes(skeleton);
    let target = (0..skeleton.nodes.len())
        .filter(|&i| trunk[i])
        .min_by(|&a, &b| {
            (skeleton.nodes[a].pos.y - breast)
                .abs()
                .total_cmp(&(skeleton.nodes[b].pos.y - breast).abs())
        })
        .unwrap_or(0);
    skeleton.pipe_radii(exponent, taper, target, form.trunk_radius);
}

/// For every fork, `r^n` of the parent over the sum of `r^n` of its
/// children: at most 1 plus the parent's own taper.
fn pipe_ratios(skeleton: &Skeleton, exponent: f64) -> Vec<f64> {
    skeleton
        .nodes
        .iter()
        .filter(|n| n.children.len() > 1)
        .map(|n| {
            let children: f64 = n
                .children
                .iter()
                .map(|&c| skeleton.nodes[c].radius.powf(exponent))
                .sum();
            n.radius.powf(exponent) / children
        })
        .collect()
}

/// Which branch each node belongs to.
pub(crate) fn node_branches(skeleton: &Skeleton, branches: &[Branch]) -> Vec<usize> {
    let mut of = vec![0; skeleton.nodes.len()];
    for (b, branch) in branches.iter().enumerate() {
        let skip = usize::from(branch.parent.is_some());
        for &n in &branch.nodes[skip..] {
            of[n] = b;
        }
    }
    of
}

/// The wind pivots, as the `pivots` table and per branch.
#[derive(Default)]
pub(crate) struct Pivots {
    pub of_branch: Vec<u32>,
    level: Vec<u32>,
    phase: Vec<f32>,
    table: Vec<f32>,
}

/// One entry of the pivot table.
pub(crate) struct Pivot {
    pub pos: DVec3,
    pub parent: Option<u32>,
    pub axis: DVec3,
    pub length: f64,
    pub radius: f64,
    /// Bending stiffness: 0 to 1 over a species' wood, 1 for its trunk,
    /// and far above 1 for what does not move at all.
    pub stiffness: f64,
}

impl Pivots {
    /// An empty table for `count` branches, none of them with a pivot yet.
    pub(crate) fn for_branches(count: usize) -> Self {
        Self {
            of_branch: vec![u32::MAX; count],
            ..Self::default()
        }
    }

    /// Add a pivot, its phase drawn from `rng`. Its level is one below its
    /// parent's, and its natural frequency follows radius over length
    /// squared, so thin long wood sways faster.
    pub(crate) fn push(&mut self, pivot: &Pivot, rng: &mut Rng) -> u32 {
        let length = pivot.length.max(0.05);
        let frequency = (110.0 * pivot.radius / (length * length)).clamp(0.3, 3.0);
        let level = pivot.parent.map_or(0, |p| self.level[p as usize] + 1);
        // The table is f32 by contract; positions are metres in a prop.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        self.table.extend([
            pivot.pos.x as f32,
            pivot.pos.y as f32,
            pivot.pos.z as f32,
            pivot.parent.map_or(-1.0, |p| p as f32),
            pivot.axis.x as f32,
            pivot.axis.y as f32,
            pivot.axis.z as f32,
            length as f32,
            pivot.radius as f32,
            frequency as f32,
            pivot.stiffness as f32,
            0.0,
        ]);
        self.level.push(level);
        #[allow(clippy::cast_possible_truncation)]
        self.phase.push(rng.unit() as f32);
        debug_assert_eq!(self.table.len(), self.level.len() * PIVOT_STRIDE);
        u32::try_from(self.level.len() - 1).expect("at most 255 pivots")
    }

    /// A tree's pivots: the trunk at the ground, then first-order branches
    /// by size, then second-order ones, until the table is full. Anything
    /// smaller swings with its parent branch's pivot, so no chain is deeper
    /// than three.
    fn for_tree(skeleton: &Skeleton, branches: &[Branch], form: &Form, rng: &mut Rng) -> Self {
        let mut pivots = Self::for_branches(branches.len());
        let trunk_radius = form.trunk_radius.max(1e-3);
        let stiffness = |radius: f64| (radius / trunk_radius).sqrt().clamp(0.05, 1.0);
        let trunk = &branches[0];
        let base = skeleton.nodes[0].pos;
        let top = skeleton.nodes[*trunk.nodes.last().expect("a trunk")].pos;
        let (axis, length) = if skeleton.multi_stem {
            (DVec3::Y, form.height)
        } else {
            ((top - base).normalize_or(DVec3::Y), trunk.length)
        };
        pivots.of_branch[0] = pivots.push(
            &Pivot {
                pos: DVec3::new(base.x, 0.0, base.z),
                parent: None,
                axis,
                length,
                radius: form.trunk_radius,
                stiffness: 1.0,
            },
            rng,
        );
        for order in 1..=2 {
            let mut candidates: Vec<(f64, usize)> = branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.order == order && b.nodes.len() > 2)
                .filter(|(_, b)| b.parent.is_some_and(|p| pivots.of_branch[p] != u32::MAX))
                .filter(|(_, b)| order == 1 || b.length > form.segment * 3.0)
                .map(|(i, b)| {
                    let r = skeleton.nodes[b.nodes[1]].radius;
                    (r * r * b.length, i)
                })
                .collect();
            candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, i) in candidates {
                if pivots.level.len() >= MAX_PIVOTS {
                    break;
                }
                let b = &branches[i];
                let start = skeleton.nodes[b.nodes[0]].pos;
                let end = skeleton.nodes[*b.nodes.last().expect("nodes")].pos;
                let radius = skeleton.nodes[b.nodes[1]].radius;
                pivots.of_branch[i] = pivots.push(
                    &Pivot {
                        pos: start,
                        parent: Some(pivots.of_branch[b.parent.expect("filtered to children")]),
                        axis: (end - start).normalize_or(DVec3::Y),
                        length: b.length,
                        radius,
                        stiffness: stiffness(radius),
                    },
                    rng,
                );
            }
        }
        pivots.inherit(branches);
        pivots
    }

    /// A rosette's pivots: one at the root, and one for each stem or frond
    /// growing from it, parented to it.
    pub(crate) fn rosette(
        skeleton: &Skeleton,
        branches: &[Branch],
        form: &Form,
        stiffness: f64,
        rng: &mut Rng,
    ) -> Self {
        let mut pivots = Self::for_branches(branches.len());
        pivots.of_branch[0] = pivots.push(
            &Pivot {
                pos: DVec3::ZERO,
                parent: None,
                axis: DVec3::Y,
                length: form.height,
                radius: skeleton.nodes[0].radius,
                stiffness: 1.0,
            },
            rng,
        );
        for (index, branch) in branches.iter().enumerate().skip(1) {
            let base = skeleton.nodes[branch.nodes[1]].pos;
            let tip = skeleton.nodes[*branch.nodes.last().expect("a stem has nodes")].pos;
            pivots.of_branch[index] = pivots.push(
                &Pivot {
                    pos: base,
                    parent: Some(pivots.of_branch[0]),
                    axis: (tip - base).normalize_or(DVec3::Y),
                    length: branch.length,
                    radius: skeleton.nodes[branch.nodes[1]].radius,
                    stiffness,
                },
                rng,
            );
        }
        pivots
    }

    /// Every branch without a pivot of its own swings with its parent's.
    pub(crate) fn inherit(&mut self, branches: &[Branch]) {
        for (index, branch) in branches.iter().enumerate() {
            if self.of_branch[index] == u32::MAX {
                self.of_branch[index] =
                    self.of_branch[branch.parent.expect("the root branch has a pivot")];
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn wind(&self, pivot: u32, flutter: f64) -> [f32; 4] {
        let p = pivot as usize;
        #[allow(clippy::cast_possible_truncation)]
        [
            pivot as f32,
            self.level[p] as f32,
            flutter as f32,
            self.phase[p],
        ]
    }
}

/// The ambient occlusion grid: the nearest level's leaf area and wood.
pub(crate) fn occupancy(skeleton: &Skeleton, cards: &[Card], form: &Form) -> Occupancy {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for p in skeleton
        .nodes
        .iter()
        .map(|n| n.pos)
        .chain(cards.iter().map(Card::centre))
    {
        min = min.min(p);
        max = max.max(p);
    }
    let cell = form.height / 26.0;
    let mut grid = Occupancy::new(
        min - DVec3::splat(form.card_size),
        max + DVec3::splat(form.card_size),
        cell,
    );
    for card in cards {
        let area = card.size * card.size / 5.0;
        for (u, v) in [(0.5, 0.5), (0.2, 0.3), (0.8, 0.3), (0.3, 0.75), (0.7, 0.75)] {
            grid.add_area(card.at(u, v), area);
        }
    }
    for (i, node) in skeleton.nodes.iter().enumerate().skip(1) {
        let parent = skeleton.nodes[node.parent].pos;
        let length = skeleton.segment_length(i);
        grid.add_solid(
            (parent + node.pos) * 0.5,
            core::f64::consts::PI * node.radius * node.radius * length,
        );
    }
    grid
}

/// What the shade pass reads.
pub(crate) struct Shade<'a> {
    pub pivots: &'a Pivots,
    pub field: &'a CrownField,
    pub occupancy: &'a Occupancy,
    pub height: f64,
}

/// A piece under construction: its geometry, and per vertex the pivot it
/// swings about, how much it flutters and its curvature, which
/// [`Piece::finish`] turns into the `wind` and `shade` attributes.
#[derive(Default)]
pub(crate) struct Piece {
    pub mesh: TriangleMesh,
    pub pivot: Vec<u32>,
    pub flutter: Vec<f64>,
    pub curvature: Vec<f64>,
}

impl Piece {
    /// A bark mesh as a piece, and its rings for the tests.
    pub(crate) fn from_bark(bark: BarkMesh) -> (Self, Vec<(usize, usize, f64)>) {
        let flutter = vec![0.0; bark.pivot.len()];
        (
            Self {
                mesh: bark.mesh,
                pivot: bark.pivot,
                flutter,
                curvature: bark.curvature,
            },
            bark.rings,
        )
    }

    /// Add a vertex; answers its index.
    pub(crate) fn vertex(
        &mut self,
        p: DVec3,
        n: DVec3,
        uv: [f64; 2],
        pivot: u32,
        flutter: f64,
        curvature: f64,
    ) -> u32 {
        let index = u32::try_from(self.mesh.positions.len())
            .expect("a plant is far under 4 billion vertices");
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        self.mesh.uvs.push(uv);
        self.pivot.push(pivot);
        self.flutter.push(flutter);
        self.curvature.push(curvature);
        index
    }

    /// A grid of `columns` by `rows` vertices, row by row as [`vertex`]
    /// added them from `first`, as two triangles per cell.
    ///
    /// [`vertex`]: Self::vertex
    pub(crate) fn grid(&mut self, first: u32, columns: u32, rows: u32) {
        for row in 0..rows - 1 {
            for col in 0..columns - 1 {
                let a = first + row * columns + col;
                let b = a + 1;
                let c = a + columns;
                let d = c + 1;
                self.mesh.indices.extend([a, c, b, b, c, d]);
            }
        }
    }

    /// Face sources, and the `wind` and `shade` attributes on every vertex.
    pub(crate) fn finish(self, shade: &Shade<'_>) -> TriangleMesh {
        let Self {
            mut mesh,
            pivot,
            flutter,
            curvature,
        } = self;
        mesh.sources = vec![FaceSource::BODY; mesh.indices.len() / 3];
        let wind = pivot
            .iter()
            .zip(&flutter)
            .map(|(&p, &f)| shade.pivots.wind(p, f))
            .collect();
        let shades = mesh
            .positions
            .iter()
            .zip(&mesh.normals)
            .zip(&curvature)
            .map(|((p, n), c)| shade.at(*p, *n, *c))
            .collect();
        mesh.attributes
            .insert("wind".to_owned(), VertexData::F32x4(wind));
        mesh.attributes
            .insert("shade".to_owned(), VertexData::F32x4(shades));
        mesh
    }
}

impl Shade<'_> {
    #[allow(clippy::cast_possible_truncation)]
    fn at(&self, p: DVec3, n: DVec3, curvature: f64) -> [f32; 4] {
        [
            self.occupancy.ambient(p, n).clamp(0.0, 1.0) as f32,
            (p.y / self.height).clamp(0.0, 1.0) as f32,
            curvature.clamp(-1.0, 1.0) as f32,
            self.field.depth(p).clamp(0.0, 1.0) as f32,
        ]
    }
}

/// A card's shading normal: mostly the crown's envelope normal, a little
/// its own face turned to agree with it.
pub(crate) fn card_normal(field: &CrownField, p: DVec3, face: DVec3) -> DVec3 {
    let envelope = field.normal(p);
    let facing = if face.dot(envelope) < 0.0 {
        -face
    } else {
        face
    };
    (envelope * ENVELOPE_SHARE + facing * (1.0 - ENVELOPE_SHARE)).normalize()
}

/// How much a card vertex flutters: still at the stem, most at the tip and
/// the edges.
pub(crate) fn card_flutter(u: f64, v: f64) -> f64 {
    let along = ((STEM[1] - v) / STEM[1]).clamp(0.0, 1.0);
    let edge = (2.0 * u - 1.0).abs() * (0.3 + 0.7 * along);
    (0.75 * along + 0.35 * edge).clamp(0.0, 1.0)
}

/// The leaf piece: every card as a cupped 3 by 3 grid or a flat quad, with
/// shading normals taken mostly from the crown field.
pub(crate) fn card_mesh(
    cards: &[Card],
    cupped: bool,
    node_branch: &[usize],
    pivots: &Pivots,
    field: &CrownField,
) -> Piece {
    let mut piece = Piece::default();
    let steps: &[f64] = if cupped {
        &[0.0, 0.5, 1.0]
    } else {
        &[0.0, 1.0]
    };
    let n = u32::try_from(steps.len()).expect("a few steps");
    for card in cards {
        let pivot = pivots.of_branch[node_branch[card.node]];
        let face = card.face();
        let mut first = None;
        for &v in steps {
            for &u in steps {
                let mut p = card.at(u, v);
                if cupped {
                    // The edges curl to the front and the tip sags, so a
                    // card has some volume from the side.
                    let across = 2.0 * u - 1.0;
                    let along = STEM[1] - v;
                    p += face * (card.size * 0.14 * across * across)
                        + DVec3::NEG_Y * (card.size * 0.12 * along * along);
                }
                let normal = card_normal(field, p, face);
                let index = piece.vertex(
                    p,
                    normal,
                    [u, v],
                    pivot,
                    card_flutter(u, v),
                    field.curvature(p),
                );
                first.get_or_insert(index);
            }
        }
        piece.grid(first.expect("a card has vertices"), n, n);
    }
    piece
}

/// The trunk as a capsule from near the ground to about 60% of the height,
/// its radius the trunk's at breast height; for a shrub, one capsule around
/// the stems' base.
/// A bush is walked through, as today's understory is, so it has none.
fn collider(
    recipe: &TreeRecipe,
    skeleton: &Skeleton,
    branches: &[Branch],
    form: &Form,
) -> Option<PropCollider> {
    if recipe.habit == Habit::Bush {
        return None;
    }
    if recipe.habit == Habit::Shrub {
        let spread = skeleton.nodes[0]
            .children
            .iter()
            .map(|&c| {
                let n = &skeleton.nodes[c];
                DVec3::new(n.pos.x, 0.0, n.pos.z).length() + n.radius
            })
            .fold(0.05_f64, f64::max);
        let radius = spread.max(form.trunk_radius * 2.0);
        return Some(PropCollider::Capsule {
            a: DVec3::new(0.0, radius, 0.0),
            b: DVec3::new(0.0, (form.height * 0.4).max(radius * 1.5), 0.0),
            radius,
        });
    }
    let trunk = &branches[0];
    let at = |y: f64| {
        let nodes = &skeleton.nodes;
        let mut best = nodes[trunk.nodes[0]].pos;
        for w in trunk.nodes.windows(2) {
            let (a, b) = (nodes[w[0]].pos, nodes[w[1]].pos);
            if (a.y <= y && b.y >= y) || b.y < y {
                let t = if (b.y - a.y).abs() > 1e-9 {
                    ((y - a.y) / (b.y - a.y)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                best = a.lerp(b, t);
                if a.y <= y && b.y >= y {
                    break;
                }
            }
        }
        DVec3::new(best.x, y, best.z)
    };
    let radius = form.trunk_radius;
    Some(PropCollider::Capsule {
        a: at(radius),
        b: at(form.height * 0.6),
        radius,
    })
}
