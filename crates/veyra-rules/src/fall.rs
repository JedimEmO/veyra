//! A fall in the world (ticket 0012, step 4a): the member is carried home,
//! out of play for played hours, and the bench puts the next able member
//! out. Nothing carried is lost and nobody walks back for anything.
//!
//! The rules are pure over a bench of [`Member`]s in bench order; the game
//! owns the household and the fade, and the dials are
//! `assets/household/fall.ron` ([`FallRules`]). Times are played seconds on
//! the [`PlayedClock`](crate::encounters::PlayedClock).

use strata_registry::FallRules;

/// What the fall's rules need to know of one member of the bench.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Member {
    /// The played second the member is well again, `None` when not hurt.
    pub injured_until: Option<f64>,
    /// Whether the member is the fabricator's worker, who cannot go out.
    pub worker: bool,
}

impl Member {
    /// Played seconds of injury left at `now`; `None` when well.
    #[must_use]
    pub fn left_s(&self, now: f64) -> Option<f64> {
        self.injured_until
            .map(|until| until - now)
            .filter(|left| *left > 0.0)
    }

    /// Whether the member can go out at `now`: well, and not at the
    /// fabricator.
    #[must_use]
    pub fn is_able(&self, now: f64) -> bool {
        !self.worker && self.left_s(now).is_none()
    }
}

/// The member after `fallen` in bench order who is able at `now`, wrapping
/// round the bench and never the fallen themselves.
#[must_use]
pub fn able(members: &[Member], fallen: usize, now: f64) -> Option<usize> {
    after(members, fallen).find(|index| members[*index].is_able(now))
}

/// The member other than `fallen`, not at the fabricator, who will be well
/// soonest; the first after `fallen` in bench order on a tie. What the bench
/// falls back on when [`able`] finds nobody.
#[must_use]
pub fn soonest(members: &[Member], fallen: usize, now: f64) -> Option<usize> {
    after(members, fallen)
        .filter(|index| !members[*index].worker)
        .min_by(|a, b| {
            let left = |index: &usize| members[*index].left_s(now).unwrap_or(0.0);
            left(a).total_cmp(&left(b))
        })
}

/// The indices after `fallen`, wrapping, `fallen` itself left out.
fn after(members: &[Member], fallen: usize) -> impl Iterator<Item = usize> {
    let count = members.len();
    (1..count).map(move |step| (fallen + step) % count)
}

/// The played second a member who falls at `now` is well again.
#[must_use]
pub fn injured_until(rules: &FallRules, now: f64) -> f64 {
    now + rules.out_s()
}

/// The played second a member stood up early at `now` is well again: the
/// dial's minutes from now, nought being at once.
#[must_use]
pub fn healed_early_until(rules: &FallRules, now: f64) -> f64 {
    now + rules.early_s()
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::FallRules;

    fn rules() -> FallRules {
        FallRules::parse(include_str!("../../../assets/household/fall.ron")).expect("shipped")
    }

    fn hurt(until: f64) -> Member {
        Member {
            injured_until: Some(until),
            worker: false,
        }
    }

    /// The next able member after the fallen is chosen in bench order, round
    /// the end, skipping the hurt and the worker.
    #[test]
    fn the_next_able_member_goes_out() {
        let well = Member::default();
        let worker = Member {
            worker: true,
            ..Member::default()
        };
        let members = [well, hurt(500.0), worker, well];
        assert_eq!(able(&members, 0, 100.0), Some(3));
        assert_eq!(able(&members, 3, 100.0), Some(0), "wraps");
        assert_eq!(able(&members, 3, 600.0), Some(0));
        // An injury that has run out is no injury.
        let healed = [well, hurt(500.0)];
        assert_eq!(able(&healed, 0, 100.0), None);
        assert_eq!(able(&healed, 0, 500.0), Some(1));
        assert_eq!(able(&[well], 0, 0.0), None, "nobody else");
    }

    /// With nobody able the soonest to heal is chosen, never the worker.
    #[test]
    fn the_soonest_to_heal_goes_early() {
        let worker = Member {
            worker: true,
            ..Member::default()
        };
        let members = [hurt(9_000.0), hurt(4_000.0), worker, hurt(4_000.0)];
        assert_eq!(able(&members, 0, 100.0), None);
        assert_eq!(soonest(&members, 0, 100.0), Some(1));
        assert_eq!(soonest(&members, 1, 100.0), Some(3));
        assert_eq!(soonest(&[hurt(5.0), worker], 0, 0.0), None);
    }

    /// The dials make the injury.
    #[test]
    fn the_dials_set_the_injury() {
        let rules = rules();
        assert!((injured_until(&rules, 100.0) - (100.0 + rules.out_s())).abs() < 1e-9);
        assert!((healed_early_until(&rules, 100.0) - (100.0 + rules.early_s())).abs() < 1e-9);
    }
}
