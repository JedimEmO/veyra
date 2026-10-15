//! The pure selection: observers and a config in, a disjoint set of regions
//! out (ADR 0003).
//!
//! Nothing here touches the ECS: the config is a `Resource` only so the
//! plugin can hold it, and the walk allocates nothing beyond the output and
//! its history, so it is tested headless and can run on the server for
//! interest management.

use std::collections::{BTreeSet, HashSet};

use bevy::ecs::resource::Resource;
use strata_core::{MAX_LOD, RegionCoord, WorldPos};

/// The knobs of the streamer. Insert it before `StreamPlugin` to override
/// the defaults.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// A node splits while the nearest observer is closer than
    /// `split_factor * node_size`. Higher means finer detail further out
    /// and more nodes; 2.5 keeps the finest 64 m nodes within 320 m.
    pub split_factor: f64,
    /// The LOD of the root nodes. The world is tiled by roots at this LOD
    /// and nothing coarser is ever selected. 9 is a 32 km root.
    pub max_lod: u8,
    /// Roots further than this from every observer do not exist: the
    /// horizon. Measured from the observer to the nearest point of the
    /// root. 48 km is past the sea horizon seen from a few hundred metres
    /// up, so the terrain, not the sea plane, ends the view.
    pub view_distance: f64,
    /// At most this many `RegionLoad`s per frame, nearest first. Bounds the
    /// meshing work queued per frame, not the number in flight.
    pub max_loads_per_frame: usize,
    /// At most this many `RegionUnload`s per frame, farthest first.
    pub max_unloads_per_frame: usize,
    /// How much further an observer has to retreat before a split node
    /// merges or a root drops, as a fraction of the split distance. Keeps
    /// a node from flipping every frame when the observer sits on a
    /// threshold.
    pub hysteresis: f64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            split_factor: 2.5,
            max_lod: 9,
            view_distance: 48_000.0,
            max_loads_per_frame: 8,
            max_unloads_per_frame: 32,
            hysteresis: 0.1,
        }
    }
}

/// A place that keeps finer terrain than distance alone would give it.
///
/// A settlement's ground is shaped at a metre's scale (terraces, walls,
/// quays), and its buildings stand on it at that scale; a 16 m grid laid over
/// a 3 m terrace bank cuts through the walls on it. While an observer is
/// within `reach` of the zone, every node overlapping it is split down to at
/// least `lod`, whatever its distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailZone {
    /// World `[min_x, min_z, max_x, max_z]`.
    pub bounds: [f64; 4],
    /// The coarsest LOD a node overlapping the zone may be.
    pub lod: u8,
    /// How near an observer has to be for the zone to apply, metres from
    /// the zone's edge.
    pub reach: f64,
}

impl DetailZone {
    /// Whether `region` overlaps the zone.
    #[must_use]
    pub fn overlaps(&self, region: RegionCoord) -> bool {
        let min = region.min_corner();
        let size = region.size();
        min.x() < self.bounds[2]
            && min.x() + size > self.bounds[0]
            && min.z() < self.bounds[3]
            && min.z() + size > self.bounds[1]
    }

    /// Horizontal distance from `observer` to the zone; zero inside it.
    #[must_use]
    pub fn distance(&self, observer: WorldPos) -> f64 {
        let dx = (self.bounds[0] - observer.x())
            .max(observer.x() - self.bounds[2])
            .max(0.0);
        let dz = (self.bounds[1] - observer.z())
            .max(observer.z() - self.bounds[3])
            .max(0.0);
        dx.hypot(dz)
    }
}

/// The detail zones the streamer honours, as the world placed them.
#[derive(bevy::ecs::resource::Resource, Debug, Clone, Default, PartialEq)]
pub struct DetailZones(pub Vec<DetailZone>);

impl StreamConfig {
    /// The root LOD, clamped to what a [`RegionCoord`] can name.
    #[must_use]
    pub fn root_lod(&self) -> u8 {
        self.max_lod.min(MAX_LOD)
    }
}

/// Horizontal distance from `observer` to the nearest point of `region`'s
/// square; zero inside it. LOD selection ignores height: a mountain top and
/// its valley mesh at the same detail (ADR 0003).
#[must_use]
pub fn distance_to_region(observer: WorldPos, region: RegionCoord) -> f64 {
    let min = region.min_corner();
    let size = region.size();
    let dx = (min.x() - observer.x())
        .max(observer.x() - (min.x() + size))
        .max(0.0);
    let dz = (min.z() - observer.z())
        .max(observer.z() - (min.z() + size))
        .max(0.0);
    dx.hypot(dz)
}

/// Distance from the nearest observer to `region`; infinite with none.
#[must_use]
pub fn nearest_distance(observers: &[WorldPos], region: RegionCoord) -> f64 {
    observers
        .iter()
        .map(|o| distance_to_region(*o, region))
        .fold(f64::INFINITY, f64::min)
}

/// Selection with memory, so the hysteresis has something to remember.
///
/// Holds the last result and the set of nodes that were split to produce
/// it. A fresh selector is a stateless selection; the plugin keeps one in
/// a resource across frames.
#[derive(Debug, Clone, Default)]
pub struct Selector {
    regions: Vec<RegionCoord>,
    /// Strict ancestors of last frame's regions: the nodes that were split.
    split: HashSet<RegionCoord>,
    /// The roots last frame's regions sit under.
    roots: HashSet<RegionCoord>,
    /// The previous walk's `split` and `roots`, kept so a walk can tell
    /// whether its tiling changed without allocating a second pair of sets.
    spare_split: HashSet<RegionCoord>,
    spare_roots: HashSet<RegionCoord>,
}

impl Selector {
    /// The last selection, nearest first.
    #[must_use]
    pub fn regions(&self) -> &[RegionCoord] {
        &self.regions
    }

    /// Whether `region` was split last frame.
    #[must_use]
    pub fn was_split(&self, region: RegionCoord) -> bool {
        self.split.contains(&region)
    }

    /// Whether `region` is in the last selection.
    ///
    /// Answered from the split nodes and the roots, which describe the
    /// tiling exactly: a node is selected when it was not split and either
    /// it is a root or its parent was split. Two lookups, so a caller that
    /// asks per loaded region needs no set of its own.
    #[must_use]
    pub fn contains(&self, region: RegionCoord) -> bool {
        !self.split.contains(&region)
            && (self.roots.contains(&region)
                || region.parent().is_some_and(|p| self.split.contains(&p)))
    }

    /// Selects for these observers, remembering the result. Returns the
    /// selection, disjoint and sorted nearest first.
    pub fn select(&mut self, observers: &[WorldPos], cfg: &StreamConfig) -> &[RegionCoord] {
        self.select_with_zones(observers, cfg, &[])
    }

    /// [`Self::select`], honouring `zones`.
    pub fn select_with_zones(
        &mut self,
        observers: &[WorldPos],
        cfg: &StreamConfig,
        zones: &[DetailZone],
    ) -> &[RegionCoord] {
        self.reselect(observers, cfg, zones);
        &self.regions
    }

    /// [`Self::select_with_zones`], saying whether the selected set differs
    /// from the previous walk's. The order may change while the set does
    /// not (it is nearest first and the observers moved); that is not a
    /// change here, since a consumer that diffs sets has nothing to redo.
    pub fn reselect(
        &mut self,
        observers: &[WorldPos],
        cfg: &StreamConfig,
        zones: &[DetailZone],
    ) -> bool {
        let mut out = core::mem::take(&mut self.regions);
        select_with_history(observers, cfg, zones, self, &mut out);
        let mut split = core::mem::take(&mut self.spare_split);
        let mut roots = core::mem::take(&mut self.spare_roots);
        split.clear();
        roots.clear();
        for region in &out {
            let mut node = *region;
            while let Some(parent) = node.parent() {
                if parent.lod > cfg.root_lod() {
                    break;
                }
                // Ancestors of a sibling were already walked.
                if !split.insert(parent) {
                    break;
                }
                node = parent;
            }
            if let Some(root) = region.ancestor_at(cfg.root_lod()) {
                roots.insert(root);
            }
        }
        // The split nodes and the roots determine the tiling, so comparing
        // them compares the selected sets, at a third of the size.
        let changed = split != self.split || roots != self.roots;
        self.spare_split = core::mem::replace(&mut self.split, split);
        self.spare_roots = core::mem::replace(&mut self.roots, roots);
        self.regions = out;
        changed
    }
}

/// Stateless selection: the same walk with no hysteresis. `out` is cleared
/// first and comes back disjoint and sorted nearest first.
pub fn select(observers: &[WorldPos], cfg: &StreamConfig, out: &mut Vec<RegionCoord>) {
    select_with_history(observers, cfg, &[], &Selector::default(), out);
}

/// The walk. Roots within `view_distance` of any observer are collected,
/// then each is split while the nearest observer is closer than
/// `split_factor * size`. A node that was split last frame keeps its
/// children until the observer is `(1 + hysteresis)` times further; a root
/// that existed last frame likewise lingers.
fn select_with_history(
    observers: &[WorldPos],
    cfg: &StreamConfig,
    zones: &[DetailZone],
    history: &Selector,
    out: &mut Vec<RegionCoord>,
) {
    out.clear();
    if observers.is_empty() {
        return;
    }
    let root_lod = cfg.root_lod();
    let root_size = RegionCoord::size_at(root_lod);
    let keep_factor = 1.0 + cfg.hysteresis.max(0.0);
    let far_reach = cfg.view_distance * keep_factor;

    // Candidate roots: every root square within reach of some observer,
    // deduplicated across observers. A BTreeSet so the walk order, and with
    // it the tie order of equidistant nodes, is deterministic.
    let mut roots = BTreeSet::new();
    for observer in observers {
        let lo = RegionCoord::containing(
            WorldPos::from_xz(observer.x() - far_reach, observer.z() - far_reach),
            root_lod,
        );
        let hi = RegionCoord::containing(
            WorldPos::from_xz(observer.x() + far_reach, observer.z() + far_reach),
            root_lod,
        );
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                roots.insert(RegionCoord::new(x, z, root_lod));
            }
        }
    }
    debug_assert!(root_size > 0.0);

    let mut selected: Vec<(f64, RegionCoord)> = Vec::new();
    let mut stack: Vec<RegionCoord> = Vec::new();
    for root in roots {
        let reach = if history.roots.contains(&root) {
            far_reach
        } else {
            cfg.view_distance
        };
        if nearest_distance(observers, root) < reach {
            stack.push(root);
        }
    }

    while let Some(node) = stack.pop() {
        let distance = nearest_distance(observers, node);
        let mut split_at = cfg.split_factor * node.size();
        if history.split.contains(&node) {
            split_at *= keep_factor;
        }
        // A detail zone within reach of an observer keeps its nodes split
        // down to its LOD; the reach carries the same hysteresis.
        let in_detail = zones.iter().any(|zone| {
            let reach = if history.split.contains(&node) {
                zone.reach * keep_factor
            } else {
                zone.reach
            };
            node.lod > zone.lod
                && zone.overlaps(node)
                && observers.iter().any(|o| zone.distance(*o) < reach)
        });
        match node.children() {
            Some(children) if distance < split_at || in_detail => stack.extend(children),
            _ => selected.push((distance, node)),
        }
    }

    selected.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out.extend(selected.into_iter().map(|(_, region)| region));
}

/// Whether `a` and `b` overlap: equal, or one an ancestor of the other.
#[must_use]
pub fn overlaps(a: RegionCoord, b: RegionCoord) -> bool {
    if a.lod <= b.lod {
        a.ancestor_at(b.lod) == Some(b)
    } else {
        b.ancestor_at(a.lod) == Some(a)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use std::collections::HashMap;
    use strata_core::Rng;

    /// Area in LOD 0 cells; exact in `u64` up to LOD 31.
    fn cells(region: RegionCoord) -> u64 {
        1u64 << (2 * u32::from(region.lod))
    }

    /// Disjoint: no duplicates, no node nested in another. Covering: every
    /// root that has a selected descendant is tiled exactly.
    fn assert_disjoint_and_covering(regions: &[RegionCoord], cfg: &StreamConfig) {
        let set: HashSet<_> = regions.iter().copied().collect();
        assert_eq!(set.len(), regions.len(), "duplicates");
        for r in regions {
            assert!(r.lod <= cfg.root_lod());
            let mut node = *r;
            while let Some(parent) = node.parent() {
                if parent.lod > cfg.root_lod() {
                    break;
                }
                assert!(!set.contains(&parent), "{r:?} is nested in {parent:?}");
                node = parent;
            }
        }
        let mut per_root: HashMap<RegionCoord, u64> = HashMap::new();
        for r in regions {
            *per_root
                .entry(r.ancestor_at(cfg.root_lod()).unwrap())
                .or_default() += cells(*r);
        }
        let root_cells = cells(RegionCoord::new(0, 0, cfg.root_lod()));
        for (root, total) in per_root {
            assert_eq!(total, root_cells, "root {root:?} is not fully tiled");
        }
    }

    fn small() -> StreamConfig {
        StreamConfig {
            max_lod: 5,
            view_distance: 3_000.0,
            ..StreamConfig::default()
        }
    }

    #[test]
    fn nearest_point_distance() {
        let r = RegionCoord::new(1, 1, 0);
        // Inside, edge, corner, and height ignored.
        assert!(distance_to_region(WorldPos::new(100.0, 500.0, 100.0), r).abs() < 1e-12);
        assert!((distance_to_region(WorldPos::from_xz(64.0, 100.0), r)).abs() < 1e-12);
        assert!((distance_to_region(WorldPos::from_xz(54.0, 100.0), r) - 10.0).abs() < 1e-12);
        assert!((distance_to_region(WorldPos::from_xz(61.0, 60.0), r) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn no_observers_selects_nothing() {
        let mut out = vec![RegionCoord::new(0, 0, 0)];
        select(&[], &StreamConfig::default(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_detail_zone_keeps_its_ground_fine_while_an_observer_is_in_reach() {
        let cfg = StreamConfig::default();
        let zone = DetailZone {
            bounds: [0.0, 0.0, 300.0, 300.0],
            lod: 0,
            reach: 2_000.0,
        };
        let far = WorldPos::from_xz(1_500.0, 150.0);
        let mut selector = Selector::default();
        let regions = selector.select_with_zones(&[far], &cfg, &[zone]).to_vec();
        let covering: Vec<_> = regions.iter().filter(|r| zone.overlaps(**r)).collect();
        assert!(!covering.is_empty());
        assert!(covering.iter().all(|r| r.lod == 0), "{covering:?}");
        // Out of reach, distance alone decides again.
        let away = WorldPos::from_xz(5_000.0, 150.0);
        let regions = Selector::default()
            .select_with_zones(&[away], &cfg, &[zone])
            .to_vec();
        assert!(
            regions
                .iter()
                .filter(|r| zone.overlaps(**r))
                .all(|r| r.lod > 0)
        );
    }

    #[test]
    fn selection_is_disjoint_and_covers_for_random_observers() {
        let cfg = StreamConfig::default();
        let mut rng = Rng::new(7);
        let mut out = Vec::new();
        for _ in 0..20 {
            let n = 1 + rng.below(3) as usize;
            let observers: Vec<_> = (0..n)
                .map(|_| {
                    WorldPos::new(
                        rng.range_f64(-60_000.0, 60_000.0),
                        rng.range_f64(0.0, 3_000.0),
                        rng.range_f64(-60_000.0, 60_000.0),
                    )
                })
                .collect();
            select(&observers, &cfg, &mut out);
            assert!(!out.is_empty());
            assert_disjoint_and_covering(&out, &cfg);
        }
    }

    #[test]
    fn fine_near_and_coarse_far_sorted_nearest_first() {
        let cfg = StreamConfig::default();
        let observer = WorldPos::from_xz(1_000.0, 1_000.0);
        let mut out = Vec::new();
        select(&[observer], &cfg, &mut out);
        assert_eq!(out[0], RegionCoord::containing(observer, 0));
        let mut last = 0.0;
        for r in &out {
            let d = distance_to_region(observer, *r);
            assert!(d >= last, "not sorted nearest first");
            last = d;
            // A node that exists was not split: the observer is at least
            // split_factor * size away, or it is at LOD 0.
            assert!(
                r.lod == 0 || d >= cfg.split_factor * r.size(),
                "{r:?} at {d} should have split"
            );
            // And its parent was split, so the parent was within reach.
            let parent = r.parent().unwrap();
            if parent.lod <= cfg.root_lod() {
                assert!(distance_to_region(observer, parent) < cfg.split_factor * parent.size());
            }
        }
        let max_lod_seen = out.iter().map(|r| r.lod).max().unwrap();
        assert!(
            max_lod_seen >= 5,
            "expected coarse nodes at the horizon, got lod {max_lod_seen}"
        );
        // Every root within the view distance is present through its tiles.
        let roots: HashSet<_> = out
            .iter()
            .map(|r| r.ancestor_at(cfg.root_lod()).unwrap())
            .collect();
        assert!(roots.len() >= 4, "{}", roots.len());
    }

    #[test]
    fn node_count_stays_roughly_constant_along_a_path() {
        let cfg = StreamConfig::default();
        let mut selector = Selector::default();
        let mut counts = Vec::new();
        let steps = 400;
        for i in 0..=steps {
            // 20 km diagonally, so both axes cross node boundaries.
            let t = f64::from(i) / f64::from(steps) * 20_000.0;
            let observer = WorldPos::new(t, 100.0, t * 0.5 + 300.0);
            let regions = selector.select(&[observer], &cfg);
            assert_disjoint_and_covering(regions, &cfg);
            counts.push(regions.len());
        }
        let min = *counts.iter().min().unwrap();
        let max = *counts.iter().max().unwrap();
        assert!(min > 50, "too few nodes: {min}");
        assert!(max < 2_000, "too many nodes: {max}");
        assert!(
            f64::from(u32::try_from(max).unwrap()) < 1.5 * f64::from(u32::try_from(min).unwrap()),
            "node count swings from {min} to {max} along the path"
        );
    }

    #[test]
    fn hysteresis_stops_a_boundary_from_flickering() {
        let cfg = small();
        let set = |v: &[RegionCoord]| v.iter().copied().collect::<HashSet<_>>();
        // Find two positions a metre apart where a stateless selection
        // differs: an observer crossing a split threshold.
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut flip = None;
        for i in 0..4_000 {
            let x = f64::from(i);
            select(&[WorldPos::from_xz(x, 33.0)], &cfg, &mut a);
            select(&[WorldPos::from_xz(x + 1.0, 33.0)], &cfg, &mut b);
            if set(&a) != set(&b) {
                flip = Some(x);
                break;
            }
        }
        let x = flip.expect("a stateless selection flips somewhere along 4 km");

        // With history the set settles after the first crossing (an
        // approach still splits at once; only the merge is delayed) and
        // then stays put however often the observer crosses the metre.
        let mut selector = Selector::default();
        selector.select(&[WorldPos::from_xz(x, 33.0)], &cfg);
        let first = set(selector.select(&[WorldPos::from_xz(x + 1.0, 33.0)], &cfg));
        for i in 0..10 {
            let pos = if i % 2 == 0 { x } else { x + 1.0 };
            let now = set(selector.select(&[WorldPos::from_xz(pos, 33.0)], &cfg));
            assert_eq!(now, first, "selection flickered at step {i}");
        }
        // And without history the same walk flickers, so the test means
        // something.
        select(&[WorldPos::from_xz(x, 33.0)], &cfg, &mut a);
        select(&[WorldPos::from_xz(x + 1.0, 33.0)], &cfg, &mut b);
        assert_ne!(set(&a), set(&b));
    }

    #[test]
    fn a_split_node_merges_only_after_retreating_past_the_band() {
        let cfg = StreamConfig {
            hysteresis: 0.5,
            ..small()
        };
        let mut selector = Selector::default();
        let node = RegionCoord::new(10, 0, 3);
        let split_at = cfg.split_factor * node.size();
        // Approach from the east until just inside the split distance.
        let inside = WorldPos::from_xz(node.min_corner().x() + node.size() + split_at - 1.0, 100.0);
        selector.select(&[inside], &cfg);
        assert!(selector.was_split(node));
        // Step back to 1.2 times the split distance: still split.
        let band = WorldPos::from_xz(node.min_corner().x() + node.size() + split_at * 1.2, 100.0);
        selector.select(&[band], &cfg);
        assert!(
            selector.was_split(node),
            "merged inside the hysteresis band"
        );
        // Past 1.5 times: merged.
        let out = WorldPos::from_xz(node.min_corner().x() + node.size() + split_at * 1.6, 100.0);
        selector.select(&[out], &cfg);
        assert!(!selector.was_split(node));
    }

    #[test]
    fn two_observers_share_one_tiling() {
        let cfg = small();
        let mut out = Vec::new();
        let a = WorldPos::from_xz(0.0, 0.0);
        let b = WorldPos::from_xz(4_000.0, 0.0);
        select(&[a, b], &cfg, &mut out);
        assert_disjoint_and_covering(&out, &cfg);
        assert!(out.contains(&RegionCoord::containing(a, 0)));
        assert!(out.contains(&RegionCoord::containing(b, 0)));
    }

    #[test]
    fn contains_and_the_change_flag_agree_with_the_selected_set() {
        let cfg = small();
        let mut selector = Selector::default();
        let mut last: HashSet<RegionCoord> = HashSet::new();
        for i in 0..200 {
            let observer = WorldPos::from_xz(f64::from(i) * 23.0, 33.0 + f64::from(i) * 7.0);
            let changed = selector.reselect(&[observer], &cfg, &[]);
            let now: HashSet<_> = selector.regions().iter().copied().collect();
            assert_eq!(changed, now != last, "change flag wrong at step {i}");
            for r in &now {
                assert!(selector.contains(*r), "{r:?} selected but not contained");
                // Neither its parent nor its children are selected.
                if let Some(parent) = r.parent() {
                    assert!(!selector.contains(parent));
                }
                if let Some(children) = r.children() {
                    assert!(children.iter().all(|c| !selector.contains(*c)));
                }
            }
            for r in &last {
                assert_eq!(selector.contains(*r), now.contains(r));
            }
            last = now;
        }
        // The same observer again: the same set, no change.
        let observer = WorldPos::from_xz(199.0 * 23.0, 33.0 + 199.0 * 7.0);
        assert!(!selector.reselect(&[observer], &cfg, &[]));
    }

    #[test]
    fn overlap_is_nesting() {
        let a = RegionCoord::new(5, 3, 0);
        assert!(overlaps(a, a));
        assert!(overlaps(a, a.parent().unwrap()));
        assert!(overlaps(a.parent().unwrap(), a));
        assert!(!overlaps(a, RegionCoord::new(6, 3, 0)));
        assert!(!overlaps(a, RegionCoord::new(3, 1, 1)));
    }
}
