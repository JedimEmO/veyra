//! Interest management on the region grid: a client sees the entities in
//! the regions it would have loaded at a fine LOD.
//!
//! The streamer's selection is a pure function of an observer position and
//! a [`StreamConfig`] (ADR 0003), and the server runs it here once more per
//! client, so "what this client sees" and "what this client has loaded"
//! are the same walk with the same hysteresis. An entity is in a client's
//! interest when the selected region containing its position is at
//! [`InterestConfig::max_lod`] or finer; with the default config that is
//! everything within about 1.3 km, which is where a walking player's
//! terrain is fine enough to show another player standing on it.
//!
//! The decision is [`in_interest`], a pure function, so it is tested
//! without a socket; the systems that apply it live in [`crate::server`].

use std::collections::HashSet;
use std::hash::BuildHasher;

use bevy::ecs::prelude::*;
use strata_core::{RegionCoord, WorldPos};
use strata_stream::{Selector, StreamConfig};

/// The knobs of interest management. Insert before the plugin to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct InterestConfig {
    /// The coarsest region LOD an entity may sit in and still be seen. A
    /// region at LOD `n` is selected while the observer is within
    /// `split_factor * size(n + 1)` of its parent, so 2 reaches about
    /// 1.3 km with the default streamer.
    pub max_lod: u8,
    /// How far a client's player moves before its selection is walked
    /// again. Regions are 64 m at the finest; eight metres keeps the set
    /// current without a walk per frame per client.
    pub reselect_distance: f64,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            max_lod: 2,
            reselect_distance: 8.0,
        }
    }
}

/// A connected client's own selection of regions, on the client entity.
///
/// Refreshed by the server when the client's player has moved
/// [`InterestConfig::reselect_distance`] since the last walk. The
/// `Selector` keeps the streamer's hysteresis so an entity on a region
/// boundary does not flicker in and out.
#[derive(Component, Debug, Default)]
pub struct ClientInterest {
    selector: Selector,
    regions: HashSet<RegionCoord>,
    at: Option<WorldPos>,
}

impl ClientInterest {
    /// The selected regions, disjoint, at every LOD.
    #[must_use]
    pub fn regions(&self) -> &HashSet<RegionCoord> {
        &self.regions
    }

    /// Where the last walk was made from.
    #[must_use]
    pub fn at(&self) -> Option<WorldPos> {
        self.at
    }

    /// Walks the selection from `observer` when it has moved far enough
    /// since the last walk (or there was none). Returns whether it did.
    pub fn refresh(
        &mut self,
        observer: WorldPos,
        cfg: &StreamConfig,
        interest: &InterestConfig,
    ) -> bool {
        if self
            .at
            .is_some_and(|last| last.distance(observer) < interest.reselect_distance)
        {
            return false;
        }
        self.selector.select(&[observer], cfg);
        self.regions.clear();
        self.regions.extend(self.selector.regions().iter().copied());
        self.at = Some(observer);
        true
    }

    /// Whether `pos` is in a selected region at [`InterestConfig::max_lod`]
    /// or finer.
    #[must_use]
    pub fn contains(&self, pos: WorldPos, interest: &InterestConfig) -> bool {
        in_interest(&self.regions, pos, interest.max_lod)
    }
}

/// Whether `pos` falls in one of `regions` at LOD `max_lod` or finer.
///
/// The regions are a disjoint selection, so at most one of them contains
/// the point; walking the LODs from the finest finds it in `max_lod + 1`
/// hash lookups. A point beyond the horizon is in no region at all.
#[must_use]
pub fn in_interest<S: BuildHasher>(
    regions: &HashSet<RegionCoord, S>,
    pos: WorldPos,
    max_lod: u8,
) -> bool {
    (0..=max_lod).any(|lod| regions.contains(&RegionCoord::containing(pos, lod)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> StreamConfig {
        StreamConfig {
            max_lod: 9,
            view_distance: 48_000.0,
            ..StreamConfig::default()
        }
    }

    fn selection(at: WorldPos) -> HashSet<RegionCoord> {
        let mut out = Vec::new();
        strata_stream::select(&[at], &cfg(), &mut out);
        out.into_iter().collect()
    }

    #[test]
    fn near_is_in_and_far_is_out() {
        let me = WorldPos::new(1_000.0, 10.0, -2_000.0);
        let regions = selection(me);
        assert!(in_interest(&regions, me, 2), "the observer's own cell");
        assert!(in_interest(
            &regions,
            WorldPos::new(1_050.0, 0.0, -2_030.0),
            2
        ));
        assert!(in_interest(
            &regions,
            WorldPos::new(1_400.0, 0.0, -2_000.0),
            2
        ));
        assert!(!in_interest(
            &regions,
            WorldPos::new(6_000.0, 0.0, -2_000.0),
            2
        ));
        assert!(
            !in_interest(&regions, WorldPos::new(200_000.0, 0.0, 0.0), 2),
            "beyond the horizon is in no region"
        );
    }

    #[test]
    fn the_lod_limit_sets_the_radius() {
        let me = WorldPos::ZERO;
        let regions = selection(me);
        let two_km = WorldPos::new(2_000.0, 0.0, 0.0);
        assert!(!in_interest(&regions, two_km, 2));
        assert!(in_interest(&regions, two_km, 4));
        // The reach grows with the limit and never shrinks.
        let mut last = 0.0;
        for lod in 0..=6 {
            let mut reach = 0.0;
            let mut d = 0.0;
            while d < 20_000.0 {
                if in_interest(&regions, WorldPos::new(d, 0.0, 0.0), lod) {
                    reach = d;
                }
                d += 16.0;
            }
            assert!(reach >= last, "lod {lod}: {reach} < {last}");
            last = reach;
        }
    }

    #[test]
    fn a_client_interest_walks_only_when_moved() {
        let mut interest = ClientInterest::default();
        let icfg = InterestConfig::default();
        assert!(interest.refresh(WorldPos::ZERO, &cfg(), &icfg));
        assert!(!interest.refresh(WorldPos::new(3.0, 0.0, 0.0), &cfg(), &icfg));
        assert!(interest.refresh(WorldPos::new(30.0, 0.0, 0.0), &cfg(), &icfg));
        assert_eq!(interest.at(), Some(WorldPos::new(30.0, 0.0, 0.0)));
        assert!(interest.contains(WorldPos::new(100.0, 0.0, 100.0), &icfg));
        assert!(!interest.contains(WorldPos::new(5_000.0, 0.0, 100.0), &icfg));
    }
}
