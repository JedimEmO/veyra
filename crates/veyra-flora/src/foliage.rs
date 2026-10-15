//! Leaf cards on the terminal shoots.
//!
//! A card is one painted cluster of leaves on a twig, so it hangs from a
//! skeleton node by its stem (`veyra_materials::LEAF_CARD_STEM` in card UV)
//! and points out along the shoot. Cards follow the skeleton, which is what
//! makes the crown clumpy: they sit where the colonisation's masses put
//! shoots, and nowhere else.
use ashlar::glam::{DQuat, DVec3};

use crate::rng::Rng;
use crate::skeleton::Skeleton;
use crate::species::Form;
use crate::{Habit, TreeRecipe};

/// Where the stem enters a card, in card UV: the bottom centre, a little
/// inside the edge, where the leaf graphs paint it.
pub(crate) const STEM: [f64; 2] = [
    veyra_materials::LEAF_CARD_STEM[0] as f64,
    veyra_materials::LEAF_CARD_STEM[1] as f64,
];

#[derive(Clone, Debug)]
pub(crate) struct Card {
    /// Where the stem meets its shoot.
    pub attach: DVec3,
    /// From the stem towards the far edge of the card, unit length.
    pub axis: DVec3,
    /// Along the card's `u`, unit length and square to `axis`.
    pub across: DVec3,
    /// Edge length, metres.
    pub size: f64,
    /// The skeleton node it hangs from.
    pub node: usize,
}

impl Card {
    /// The point at card UV `(u, v)`, on the flat card.
    pub(crate) fn at(&self, u: f64, v: f64) -> DVec3 {
        self.attach
            + self.across * ((u - STEM[0]) * self.size)
            + self.axis * ((STEM[1] - v) * self.size)
    }

    pub(crate) fn centre(&self) -> DVec3 {
        self.at(0.5, 0.5)
    }

    /// The face normal the card is built with, `across` by `axis`.
    pub(crate) fn face(&self) -> DVec3 {
        self.across.cross(self.axis).normalize_or(DVec3::Y)
    }
}

/// Which way is out of the crown at a point: from the crown's middle for a
/// rounded crown, from the trunk and a little up for a conifer's cone.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Outward {
    Centre(DVec3),
    Axis { lean_per_metre: [f64; 2] },
}

impl Outward {
    pub(crate) fn at(self, p: DVec3) -> DVec3 {
        match self {
            Self::Centre(c) => (p - c).normalize_or(DVec3::Y),
            Self::Axis { lean_per_metre } => {
                let axis = DVec3::new(lean_per_metre[0] * p.y, p.y, lean_per_metre[1] * p.y);
                let h = DVec3::new(p.x - axis.x, 0.0, p.z - axis.z);
                (h.normalize_or(DVec3::X) + DVec3::Y * 0.45).normalize()
            }
        }
    }
}

/// The nearest level's cards, at most `form.cards`.
pub(crate) fn place(
    recipe: &TreeRecipe,
    form: &Form,
    skeleton: &Skeleton,
    outward: Outward,
) -> Vec<Card> {
    let mut rng = form.rng("cards");
    let d = form.segment;
    let foliage = &recipe.foliage;
    let trunk = trunk_nodes(skeleton);
    let max_age = match (recipe.habit, foliage.tuft) {
        (Habit::Conifer, 1) => f64::INFINITY,
        (Habit::Conifer, _) => d * 3.5,
        _ => d * 4.0,
    };
    // Needles and leaves are shed near the trunk inside a conifer.
    let clear = if recipe.habit == Habit::Conifer {
        form.crown_radius * 0.18
    } else {
        0.0
    };
    let mut candidates: Vec<(u64, usize)> = skeleton
        .nodes
        .iter()
        .enumerate()
        .filter(|&(i, n)| {
            // A conifer's leader carries needles on its last metre or two.
            let leader = recipe.habit == Habit::Conifer && n.pos.y > form.height * 0.88;
            i > 0
                && !n.dead
                && n.pos.y > form.crown_base * 0.5
                && (!trunk[i] || leader)
                && (leader || (n.age <= max_age && horizontal_from_trunk(skeleton, n.pos) >= clear))
        })
        .map(|(i, n)| {
            // Tips first, then the rest in a fixed shuffle.
            let tip = u64::from(!n.children.is_empty());
            (tip << 63 | (rng.next_u64() >> 1), i)
        })
        .collect();
    candidates.sort_unstable();

    let mut cards: Vec<Card> = Vec::new();
    let limit = form.cards as usize;
    // A sparse skeleton gets a second and third round of cards, packed a
    // little closer, before a crown is left short of its leaf.
    'passes: for (pass, gap) in [0.28, 0.22, 0.17].into_iter().enumerate() {
        let mut grid = Spacing::new(form.card_size * gap);
        for card in &cards {
            grid.admit(card.centre());
        }
        for &(_, node) in &candidates {
            let pos = skeleton.nodes[node].pos;
            let shoot = skeleton.direction(node);
            for k in 0..foliage.tuft {
                if cards.len() >= limit {
                    break 'passes;
                }
                #[allow(clippy::cast_possible_truncation)]
                let k = k + pass as u32 * foliage.tuft;
                let size = form.card_size * rng.range(0.8, 1.2);
                let out = outward.at(pos);
                let mut axis = if foliage.tuft > 1 {
                    // A tuft splays around its shoot like a brush.
                    let across = shoot.any_orthonormal_vector();
                    let turn = f64::from(k) * core::f64::consts::TAU / f64::from(foliage.tuft)
                        + rng.signed() * 0.4;
                    let spread = DQuat::from_axis_angle(shoot, turn) * across;
                    if recipe.habit == Habit::Conifer {
                        (shoot * 0.9 + spread * 0.75 + out * 0.2).normalize()
                    } else {
                        // A broadleaf's tuft leans out of the crown as it splays.
                        (shoot * 0.45
                            + out * 0.45
                            + spread * 0.6
                            + DVec3::Y * 0.2 * (1.0 - foliage.flatness))
                            .normalize()
                    }
                } else {
                    (shoot * 0.55 + out * 0.45 + DVec3::Y * 0.25 * (1.0 - foliage.flatness))
                        .normalize()
                };
                axis = (axis + rng.direction() * 0.3 + DVec3::NEG_Y * form.tip_droop * 0.8)
                    .normalize_or(DVec3::Y);
                if recipe.habit != Habit::Conifer {
                    // Lie mostly along the crown's surface, so the card faces
                    // out and is not seen edge on from outside.
                    axis = (axis - out * axis.dot(out) * 0.55).normalize_or(axis);
                }
                let centre = pos + axis * size * 0.45;
                if !grid.admit(centre) {
                    continue;
                }
                // Face out of the crown, or up for a flat spray, square to the axis.
                let flat = DVec3::Y - axis * axis.y;
                let facing = out - axis * out.dot(axis);
                let mut face = facing
                    .normalize_or(flat.normalize_or(DVec3::X))
                    .lerp(flat.normalize_or(DVec3::Y), foliage.flatness)
                    .normalize_or(DVec3::Y);
                face = DQuat::from_axis_angle(axis, rng.signed() * 0.6) * face;
                let across = axis.cross(face).normalize_or(axis.any_orthonormal_vector());
                cards.push(Card {
                    attach: pos,
                    axis,
                    across,
                    size,
                    node,
                });
            }
        }
    }
    cards
}

/// Which nodes are the trunk: the root's continuation chain.
pub(crate) fn trunk_nodes(skeleton: &Skeleton) -> Vec<bool> {
    let mut trunk = vec![false; skeleton.nodes.len()];
    let mut at = Some(0);
    while let Some(i) = at {
        trunk[i] = true;
        at = skeleton.continuation(i);
    }
    trunk
}

/// Horizontal distance from the trunk at the point's height.
fn horizontal_from_trunk(skeleton: &Skeleton, p: DVec3) -> f64 {
    let mut at = Some(0);
    let mut best = f64::INFINITY;
    while let Some(i) = at {
        let n = skeleton.nodes[i].pos;
        if (n.y - p.y).abs() < 0.6 {
            best = best.min(DVec3::new(n.x - p.x, 0.0, n.z - p.z).length());
        }
        at = skeleton.continuation(i);
    }
    best
}

/// Keeps card centres apart, so a crown's density comes from its shoots and
/// not from cards stacking on one node.
struct Spacing {
    gap: f64,
    cells: std::collections::BTreeMap<(i64, i64, i64), Vec<DVec3>>,
}

impl Spacing {
    fn new(gap: f64) -> Self {
        Self {
            gap,
            cells: std::collections::BTreeMap::new(),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn key(&self, p: DVec3) -> (i64, i64, i64) {
        let q = (p / self.gap).floor();
        (q.x as i64, q.y as i64, q.z as i64)
    }

    fn admit(&mut self, p: DVec3) -> bool {
        let (x, y, z) = self.key(p);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(points) = self.cells.get(&(x + dx, y + dy, z + dz))
                        && points.iter().any(|q| q.distance(p) < self.gap)
                    {
                        return false;
                    }
                }
            }
        }
        self.cells.entry((x, y, z)).or_default().push(p);
        true
    }
}

/// Merge cards into at most `target` larger ones for a coarser level:
/// cards are grouped on a grid whose cell is searched for the largest count
/// that fits, and each group becomes one card with the group's area (a
/// little more, since a big card's gaps are bigger too) at its middle.
pub(crate) fn merge(cards: &[Card], target: usize, outward: Outward, rng: &mut Rng) -> Vec<Card> {
    if cards.len() <= target {
        return cards.to_vec();
    }
    let groups = |cell: f64| {
        let mut map: std::collections::BTreeMap<(i64, i64, i64), Vec<usize>> =
            std::collections::BTreeMap::new();
        for (i, card) in cards.iter().enumerate() {
            let q = (card.centre() / cell).floor();
            #[allow(clippy::cast_possible_truncation)]
            map.entry((q.x as i64, q.y as i64, q.z as i64))
                .or_default()
                .push(i);
        }
        map
    };
    let (mut low, mut high) = (0.01_f64, 20.0_f64);
    for _ in 0..40 {
        let mid = (low * high).sqrt();
        if groups(mid).len() > target {
            low = mid;
        } else {
            high = mid;
        }
    }
    groups(high)
        .into_values()
        .map(|members| {
            let area: f64 = members.iter().map(|&i| cards[i].size * cards[i].size).sum();
            let weight = |i: usize| cards[i].size * cards[i].size / area;
            let centre = members
                .iter()
                .map(|&i| cards[i].centre() * weight(i))
                .sum::<DVec3>();
            let out = outward.at(centre);
            let axis = members
                .iter()
                .map(|&i| cards[i].axis * weight(i))
                .sum::<DVec3>()
                .normalize_or(DVec3::Y);
            let axis = (axis - out * axis.dot(out) * 0.6).normalize_or(axis);
            let biggest = members
                .iter()
                .copied()
                .max_by(|&a, &b| cards[a].size.total_cmp(&cards[b].size).then(b.cmp(&a)))
                .expect("a group has members");
            let face = members
                .iter()
                .map(|&i| cards[i].face() * weight(i))
                .sum::<DVec3>();
            // A merged card faces out of the crown, so it is not seen edge
            // on from outside, whichever way its members faced.
            let face = face.normalize_or(DVec3::Y) * 0.3 + out * 0.7;
            let face = (face - axis * face.dot(axis)).normalize_or(axis.any_orthonormal_vector());
            let face = DQuat::from_axis_angle(axis, rng.signed() * 0.3) * face;
            #[allow(clippy::cast_precision_loss)]
            let size = (area * (1.0 + 0.12 * (members.len() as f64).ln())).sqrt();
            let size = size.min(cards[biggest].size * 2.2);
            let across = axis.cross(face).normalize_or(axis.any_orthonormal_vector());
            let mut card = Card {
                attach: DVec3::ZERO,
                axis,
                across,
                size,
                node: cards[biggest].node,
            };
            card.attach = centre - (card.centre() - card.attach);
            card
        })
        .collect()
}
