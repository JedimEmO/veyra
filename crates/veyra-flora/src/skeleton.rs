//! The grown skeleton: nodes joined into a tree, the botany applied to them,
//! and the decomposition into branches the mesher and the wind walk.
//!
//! Nodes are kept in growth order, so a parent always has a smaller index
//! than its children. Every pass here relies on that: radii run backwards,
//! bending runs forwards, and neither needs a traversal stack.
use ashlar::glam::{DQuat, DVec3};

/// The root's parent.
pub(crate) const NONE: usize = usize::MAX;

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub pos: DVec3,
    pub parent: usize,
    pub children: Vec<usize>,
    pub radius: f64,
    /// Bark only: no cards grow here or beyond.
    pub dead: bool,
    /// A broken or pruned end, closed with a flat cap.
    pub capped: bool,
    pub removed: bool,
    /// The random walk on growth direction this shoot inherited.
    pub wander: DVec3,
    /// Path length to the farthest tip beyond: how long ago this part grew.
    pub age: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Skeleton {
    pub nodes: Vec<Node>,
    /// Several stems from the root and no trunk between them.
    pub multi_stem: bool,
}

/// A run of nodes meshed as one tube. `nodes[0]` is where it attaches: the
/// root for the trunk, a node of the parent branch otherwise.
#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub nodes: Vec<usize>,
    pub parent: Option<usize>,
    /// 0 for the trunk (or a shrub's stems' root), then one more per fork.
    pub order: u32,
    pub length: f64,
}

impl Skeleton {
    pub(crate) fn root(pos: DVec3) -> Self {
        let mut skeleton = Self::default();
        skeleton.nodes.push(Node {
            pos,
            parent: NONE,
            children: Vec::new(),
            radius: 0.0,
            dead: false,
            capped: false,
            removed: false,
            wander: DVec3::ZERO,
            age: 0.0,
        });
        skeleton
    }

    pub(crate) fn add(&mut self, parent: usize, pos: DVec3, wander: DVec3) -> usize {
        let index = self.nodes.len();
        let dead = self.nodes[parent].dead;
        self.nodes.push(Node {
            pos,
            parent,
            children: Vec::new(),
            radius: 0.0,
            dead,
            capped: false,
            removed: false,
            wander,
            age: 0.0,
        });
        self.nodes[parent].children.push(index);
        index
    }

    /// The direction a node grew in.
    pub(crate) fn direction(&self, index: usize) -> DVec3 {
        let node = &self.nodes[index];
        if node.parent == NONE {
            return DVec3::Y;
        }
        (node.pos - self.nodes[node.parent].pos).normalize_or(DVec3::Y)
    }

    /// Mark a node and everything beyond it removed.
    pub(crate) fn remove_subtree(&mut self, index: usize) {
        let mut stack = vec![index];
        while let Some(i) = stack.pop() {
            self.nodes[i].removed = true;
            stack.extend(self.nodes[i].children.iter().copied());
        }
    }

    /// Cut a branch just past `index`: everything beyond goes, and the end
    /// is capped as a break.
    pub(crate) fn cut_after(&mut self, index: usize) {
        for child in self.nodes[index].children.clone() {
            self.remove_subtree(child);
        }
        self.nodes[index].capped = true;
    }

    pub(crate) fn mark_dead(&mut self, index: usize) {
        let mut stack = vec![index];
        while let Some(i) = stack.pop() {
            self.nodes[i].dead = true;
            stack.extend(self.nodes[i].children.iter().copied());
        }
    }

    /// Drop removed nodes, keeping growth order and renumbering links.
    pub(crate) fn compact(&mut self) {
        let mut map = vec![NONE; self.nodes.len()];
        let mut kept = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            if node.removed || (node.parent != NONE && map[node.parent] == NONE) {
                continue;
            }
            map[index] = kept.len();
            kept.push(node.clone());
        }
        for node in &mut kept {
            if node.parent != NONE {
                node.parent = map[node.parent];
            }
            node.children = node
                .children
                .iter()
                .filter_map(|&c| (map[c] != NONE).then_some(map[c]))
                .collect();
        }
        self.nodes = kept;
    }

    pub(crate) fn segment_length(&self, index: usize) -> f64 {
        let node = &self.nodes[index];
        if node.parent == NONE {
            0.0
        } else {
            node.pos.distance(self.nodes[node.parent].pos)
        }
    }

    pub(crate) fn compute_ages(&mut self) {
        for index in (0..self.nodes.len()).rev() {
            let age = self.nodes[index]
                .children
                .iter()
                .map(|&c| self.nodes[c].age + self.segment_length(c))
                .fold(0.0, f64::max);
            self.nodes[index].age = age;
        }
    }

    /// Pipe-model radii from the tips down: a parent's radius to the power
    /// `exponent` is the sum over its children, plus `taper` per metre of its
    /// own length so an unbranched stretch still thickens towards the root.
    /// The scale is set so `target` has radius `radius`.
    pub(crate) fn pipe_radii(&mut self, exponent: f64, taper: f64, target: usize, radius: f64) {
        let mut pipe = vec![0.0_f64; self.nodes.len()];
        for index in (0..self.nodes.len()).rev() {
            let children: f64 = self.nodes[index].children.iter().map(|&c| pipe[c]).sum();
            let own = taper * self.segment_length(index);
            pipe[index] = if children == 0.0 { 1.0 } else { children } + own;
        }
        let tip = radius / pipe[target].powf(1.0 / exponent);
        for (node, p) in self.nodes.iter_mut().zip(&pipe) {
            node.radius = tip * p.powf(1.0 / exponent);
        }
    }

    /// Gravity bending, accumulated from the root out: each segment turns
    /// towards the ground by its age and horizontal lever, and carries
    /// everything beyond it along. Young shoots have no age and stay up,
    /// old limbs sag, and thick wood resists by `stiff`.
    pub(crate) fn bend(&mut self, bending: &Bending) {
        let original: Vec<DVec3> = self.nodes.iter().map(|n| n.pos).collect();
        // The trunk does not hang: a leader bends only under load.
        let mut trunk = vec![false; self.nodes.len()];
        let mut at = Some(0);
        while let Some(i) = at {
            trunk[i] = true;
            at = self.continuation(i);
        }
        let mut rotation = vec![DQuat::IDENTITY; self.nodes.len()];
        for index in 1..self.nodes.len() {
            let parent = self.nodes[index].parent;
            let segment = original[index] - original[parent];
            let length = segment.length();
            if length < 1e-9 {
                self.nodes[index].pos = self.nodes[parent].pos;
                rotation[index] = rotation[parent];
                continue;
            }
            let d = (rotation[parent] * segment) / length;
            let horizontal = (d.x * d.x + d.z * d.z).sqrt();
            let node = &self.nodes[index];
            // A trunk is built to stand: it sags a quarter as much as a limb.
            let stand = if trunk[index] { 0.25 } else { 1.0 };
            let load = stand * (node.age / bending.age).min(1.5)
                / (1.0 + (node.radius / bending.stiff).powi(2));
            let mut angle = bending.droop * length * horizontal * load;
            if bending.tip > 0.0 && !node.dead && !trunk[index] {
                let youth = (1.0 - node.age / bending.tip_age).max(0.0);
                angle += bending.tip * length * youth * (1.0 + d.y) * 0.5;
            }
            let angle = angle.min(bending.max_step);
            let mut axis = d.cross(DVec3::NEG_Y);
            if axis.length_squared() < 1e-8 {
                let w = node.wander;
                axis = DVec3::new(w.z, 0.0, -w.x);
                if axis.length_squared() < 1e-8 {
                    axis = DVec3::X;
                }
            }
            let local = DQuat::from_axis_angle(axis.normalize(), angle);
            rotation[index] = local * rotation[parent];
            let mut pos = self.nodes[parent].pos + rotation[index] * segment;
            // Nothing droops through the ground.
            if !trunk[index] && pos.y < bending.floor {
                pos.y = bending
                    .floor
                    .max(self.nodes[parent].pos.y.min(bending.floor));
            }
            self.nodes[index].pos = pos;
        }
    }

    /// The child a stem continues into: the thickest, then the oldest.
    pub(crate) fn continuation(&self, index: usize) -> Option<usize> {
        if self.multi_stem && index == 0 {
            return None;
        }
        let nodes = &self.nodes;
        nodes[index].children.iter().copied().reduce(|best, c| {
            let (b, n) = (&nodes[best], &nodes[c]);
            if n.radius > b.radius * 1.000_001
                || (n.radius >= b.radius * 0.999_999 && n.age > b.age)
            {
                c
            } else {
                best
            }
        })
    }

    /// Relax kinks: each node inside a stem moves towards the mean of its
    /// neighbours, which turns a colonisation's zig-zag into a curve.
    pub(crate) fn smooth(&mut self, passes: u32, strength: f64) {
        let continuation: Vec<Option<usize>> = (0..self.nodes.len())
            .map(|i| self.continuation(i))
            .collect();
        for _ in 0..passes {
            let before: Vec<DVec3> = self.nodes.iter().map(|n| n.pos).collect();
            for index in 1..self.nodes.len() {
                let parent = self.nodes[index].parent;
                if parent == 0 && self.multi_stem {
                    continue;
                }
                let Some(next) = continuation[index] else {
                    continue;
                };
                let mean = (before[parent] + before[next]) * 0.5;
                self.nodes[index].pos = before[index].lerp(mean, strength);
            }
        }
    }

    /// Split the skeleton into branches: each stem follows its
    /// [`continuation`](Self::continuation), and every other child starts a
    /// branch of its own. Parents come before their children.
    pub(crate) fn branches(&self) -> Vec<Branch> {
        let mut out = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((0_usize, None::<usize>, 0_u32, NONE));
        while let Some((start, parent, order, attach)) = queue.pop_front() {
            let mut nodes = Vec::new();
            if attach != NONE {
                nodes.push(attach);
            }
            let mut at = start;
            loop {
                nodes.push(at);
                match self.continuation(at) {
                    Some(next) => at = next,
                    None => break,
                }
            }
            let length = nodes
                .windows(2)
                .map(|w| self.nodes[w[0]].pos.distance(self.nodes[w[1]].pos))
                .sum();
            let index = out.len();
            for (k, &node) in nodes.iter().enumerate() {
                if k == 0 && attach != NONE {
                    continue;
                }
                let next = self.continuation(node);
                for &child in &self.nodes[node].children {
                    if Some(child) != next {
                        queue.push_back((child, Some(index), order + 1, node));
                    }
                }
            }
            out.push(Branch {
                nodes,
                parent,
                order,
                length,
            });
        }
        out
    }
}

/// The dials of [`Skeleton::bend`].
pub(crate) struct Bending {
    /// Radians per metre of horizontal, fully aged wood.
    pub droop: f64,
    /// The age at which a segment carries its full load, metres.
    pub age: f64,
    /// The radius at which wood resists half the bend.
    pub stiff: f64,
    /// Radians per metre the youngest shoots hang by.
    pub tip: f64,
    /// Age below which a shoot counts as young, metres.
    pub tip_age: f64,
    /// The most one segment turns, radians.
    pub max_step: f64,
    /// The lowest a branch may hang, metres above the ground.
    pub floor: f64,
}
