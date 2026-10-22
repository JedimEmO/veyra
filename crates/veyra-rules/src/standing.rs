//! What the world's people and the member's standing with them keep as
//! state: which encounter a body belongs to, the played clock, and which
//! fights are engaged and which post is warning.
//!
//! Split out of `encounters` so the deeds ledger, which working a deposit
//! writes through, sits below the field rules instead of above them. The
//! rules that plan, stand and fold camps stay in `encounters`, which links
//! this module; nothing here samples ground or reads a registry.

use std::collections::{BTreeMap, BTreeSet};

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use veyra_world::deposits::DepositId;

use crate::factions::FactionId;

/// Which encounter a person belongs to.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EncounterId {
    /// The training fight west of Port Serein, with F5: never cleared, never
    /// folded.
    Staging,
    /// A camp, by its site's [`strata_worldgen::sites::SiteId::key`].
    Camp(u64),
    /// A claim's post, by [`post_key`] of its deposit.
    Post(u64),
    /// A person or crew of the first questline (ticket 0014), by a hash of
    /// its id: neutral until a reply turns it, then fought as a camp.
    Quest(u64),
}

/// The key of the post at `deposit`, in the space [`crate::encounters::Cleared`] shares with
/// site keys. A hash under a tag of its own so it is not a site's key in
/// any practical sense: two spaces, one table, no collision to reason about.
#[must_use]
pub fn post_key(deposit: DepositId) -> u64 {
    let mut hash = 0x706f_7374_6b65_7931_u64;
    for byte in deposit.0[0]
        .to_le_bytes()
        .into_iter()
        .chain(deposit.0[1].to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Played seconds since the household began: the clock a cleared camp's
/// quiet is measured on. Advanced in the tick by nominal time, so a faster
/// stepped sim does not shorten an hour. The game saves it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayedClock {
    /// Played seconds.
    pub seconds: f64,
}

/// A faction's warning to a member working its claim: the post that gave it,
/// whose it is, and the played second it runs out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warning {
    /// The post that warns.
    pub post: EncounterId,
    /// Whose it is.
    pub faction: FactionId,
    /// The played second the countdown ends and the post turns.
    pub until: f64,
}

/// One opponent as [`Fights::observe`] reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Present {
    /// Whose it is.
    pub id: EncounterId,
    /// It has been hurt.
    pub hurt: bool,
    /// It is not down.
    pub alive: bool,
    /// It is the passive training target, which never fights and is not
    /// counted.
    pub passive: bool,
    /// How far it is from the member, metres.
    pub distance: f64,
}

/// Which encounters are engaged and how many of each are left. Each
/// encounter engages on its own people alone: one hurt, or the member
/// within `engage_distance` of one.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct Fights {
    engaged: BTreeSet<EncounterId>,
    remaining: BTreeMap<EncounterId, usize>,
    /// Posts standing, crossed or not. An uncrossed post is never engaged
    /// by distance or by a hurt person, and neither the headline nor the
    /// UI locks count it.
    posts: BTreeSet<EncounterId>,
    /// Posts the member has crossed, until they fold.
    crossed: BTreeSet<EncounterId>,
    /// The warning running over a post, if any.
    warning: Option<Warning>,
    /// Camps whose fight the member left by a fall and an F5: their hurt
    /// people no longer engage them, only coming within reach again does,
    /// so they may fold once the member is far.
    abandoned: BTreeSet<EncounterId>,
}

impl Fights {
    /// Reads the people present. `staging_grace` holds the staging fight's
    /// first seconds, which no camp has. An encounter with nobody left
    /// present is forgotten, except the staging fight, which keeps its
    /// engagement until [`Fights::reset_staging`].
    pub fn observe(
        &mut self,
        people: impl IntoIterator<Item = Present>,
        engage_distance: f64,
        staging_grace: bool,
    ) {
        let mut remaining: BTreeMap<EncounterId, usize> = BTreeMap::new();
        for person in people {
            let count = remaining.entry(person.id).or_default();
            if person.passive {
                continue;
            }
            if person.alive {
                *count += 1;
            }
            if self.neutral(person.id) {
                continue;
            }
            let held = person.id == EncounterId::Staging && staging_grace;
            let near = person.distance < engage_distance;
            let abandoned = self.abandoned.contains(&person.id);
            if !held && (near || (person.hurt && !abandoned)) {
                self.engaged.insert(person.id);
                if near {
                    self.abandoned.remove(&person.id);
                }
            }
        }
        self.engaged
            .retain(|id| *id == EncounterId::Staging || remaining.contains_key(id));
        self.abandoned.retain(|id| remaining.contains_key(id));
        self.remaining = remaining;
    }

    /// `id` is a post standing: it starts uncrossed.
    pub fn stand_post(&mut self, id: EncounterId) {
        self.posts.insert(id);
    }

    /// `id` has folded away: it is no post and not crossed, and a warning
    /// over it is gone.
    pub fn forget_post(&mut self, id: EncounterId) {
        self.posts.remove(&id);
        self.crossed.remove(&id);
        self.engaged.remove(&id);
        if self.warning.is_some_and(|warning| warning.post == id) {
            self.warning = None;
        }
    }

    /// The member crosses the post `id`: its people turn and the post is
    /// engaged as a camp. Whether it was uncrossed before; a warning over it
    /// is over.
    pub fn cross(&mut self, id: EncounterId) -> bool {
        if !self.posts.contains(&id) {
            return false;
        }
        if self.warning.is_some_and(|warning| warning.post == id) {
            self.warning = None;
        }
        self.abandoned.remove(&id);
        if self.crossed.insert(id) {
            self.engaged.insert(id);
            true
        } else {
            false
        }
    }

    /// Whether `id` is a post that is standing.
    #[must_use]
    pub fn is_post(&self, id: EncounterId) -> bool {
        self.posts.contains(&id)
    }

    /// Whether the post `id` has been crossed.
    #[must_use]
    pub fn crossed(&self, id: EncounterId) -> bool {
        self.crossed.contains(&id)
    }

    /// Whether `id` is a post nobody has crossed: its people are neutral.
    #[must_use]
    pub fn neutral(&self, id: EncounterId) -> bool {
        self.posts.contains(&id) && !self.crossed.contains(&id)
    }

    /// Start a warning over the uncrossed post `post` unless one runs.
    /// Whether it started.
    pub fn begin_warning(&mut self, post: EncounterId, faction: FactionId, until: f64) -> bool {
        if self.warning.is_some() || !self.neutral(post) {
            return false;
        }
        self.warning = Some(Warning {
            post,
            faction,
            until,
        });
        true
    }

    /// The warning that runs, if one does.
    #[must_use]
    pub fn warning(&self) -> Option<Warning> {
        self.warning
    }

    /// The warning lapses: the member left the claim or took a lease.
    pub fn lapse_warning(&mut self) {
        self.warning = None;
    }

    /// Whether `id` is engaged.
    #[must_use]
    pub fn engaged(&self, id: EncounterId) -> bool {
        self.engaged.contains(&id)
    }

    /// The living people of `id` at the last reading.
    #[must_use]
    pub fn remaining(&self, id: EncounterId) -> usize {
        self.remaining.get(&id).copied().unwrap_or(0)
    }

    /// Whether `id` is engaged with someone left to fight.
    #[must_use]
    pub fn fighting(&self, id: EncounterId) -> bool {
        self.engaged(id) && self.remaining(id) > 0
    }

    /// What the HUD word and the UI locks read: the first camp being
    /// fought, else the staging fight if it is, else the first place with
    /// people standing (a camp before the staging fight), as `(engaged,
    /// remaining)`. A camp is read before the staging fight so a fight at a
    /// camp is not hidden by hostiles left at the start.
    #[must_use]
    pub fn headline(&self) -> (bool, usize) {
        let camps_first = |ids: &mut dyn Iterator<Item = EncounterId>| {
            let mut staging = false;
            let mut found = None;
            for id in ids {
                if id == EncounterId::Staging {
                    staging = true;
                } else if found.is_none() {
                    found = Some(id);
                }
            }
            found.or(staging.then_some(EncounterId::Staging))
        };
        let id = camps_first(
            &mut self
                .engaged
                .iter()
                .copied()
                .filter(|id| self.remaining(*id) > 0),
        )
        // Nobody is fighting: a place with people still standing reads as
        // ahead, not as clear.
        .or_else(|| {
            camps_first(
                &mut self
                    .remaining
                    .iter()
                    .filter(|(id, left)| **left > 0 && !self.neutral(**id))
                    .map(|(id, _)| *id),
            )
        })
        .unwrap_or(EncounterId::Staging);
        (self.engaged(id), self.remaining(id))
    }

    /// F5: the staging fight starts again, unengaged.
    pub fn reset_staging(&mut self) {
        self.engaged.remove(&EncounterId::Staging);
    }

    /// F5 after a fall at a camp: every camp's engagement is let go, so the
    /// HUD and the UI locks do not hold on a fight the member is no longer
    /// in, and a camp left far behind may fold.
    pub fn abandon_camps(&mut self) {
        let camps: Vec<EncounterId> = self
            .engaged
            .iter()
            .copied()
            .filter(|id| *id != EncounterId::Staging)
            .collect();
        for id in camps {
            self.engaged.remove(&id);
            self.abandoned.insert(id);
        }
    }
}
