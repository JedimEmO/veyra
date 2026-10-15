//! Encounters: who stands at a place when a member comes near (ticket 0012,
//! step 2), as `encounters/*.encounter.ron` spells them, one a file.
//!
//! The file names the site kind whose places hold the encounter, how near an
//! observer stands its people up and how far they fold away, how long a
//! cleared one stays quiet in played hours, and per tier the opponent kinds
//! and counts. The sim turns a kind name into a brain; this crate only knows
//! the names, so a typo is refused by name here. In no world version and no
//! handshake.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::faction::ClaimKind;

/// The directory of `.encounter.ron` files, relative to the asset root.
pub const ENCOUNTERS_DIR: &str = "encounters";
/// The suffix an encounter file must carry to be read.
pub const ENCOUNTER_SUFFIX: &str = ".encounter.ron";
/// The only version of the file this build reads.
pub const ENCOUNTER_VERSION: u32 = 1;

/// An encounter's id, `strata:outlaw_camp`.
pub type EncounterDefId = Namespaced;

/// The opponent kinds a person at a place can be, named as the sim's
/// `OpponentKind` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FoeKind {
    /// The ranged opponent.
    Sentry,
    /// The melee opponent.
    Rusher,
    /// The long-range opponent.
    Marksman,
    /// The guarding melee opponent.
    Duelist,
    /// The shield bearer.
    Bulwark,
}

/// Whose people stand at a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Crew {
    /// The band that holds the site, where one does: a camp's outlaws.
    #[default]
    Holder,
    /// Hostile to everyone and of no faction: scavengers at a wreck, the
    /// broken security of a derelict machine. The site's holder is a
    /// corporate or conservation owner that is not the enemy.
    Unaligned,
}

/// One line of a tier's roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    /// What it is.
    pub kind: FoeKind,
    /// How many of it.
    pub count: u8,
}

/// Where an encounter's people stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncounterAt {
    /// On the pad of every placed site of this kind.
    Site(Namespaced),
    /// Round the centre of every deposit a faction keeps as this kind of
    /// claim: a claim has no pad.
    Claim(ClaimKind),
}

impl EncounterAt {
    /// The site kind, where the people stand at a site.
    #[must_use]
    pub fn site(&self) -> Option<&Namespaced> {
        match self {
            Self::Site(kind) => Some(kind),
            Self::Claim(_) => None,
        }
    }

    /// The claim kind, where the people stand at a claim.
    #[must_use]
    pub fn claim(&self) -> Option<ClaimKind> {
        match self {
            Self::Claim(kind) => Some(*kind),
            Self::Site(_) => None,
        }
    }
}

/// One encounter file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterDef {
    /// Must equal [`ENCOUNTER_VERSION`].
    pub version: u32,
    /// The encounter's id.
    pub id: EncounterDefId,
    /// The place that holds it: a site kind or a kind of claim.
    pub at: EncounterAt,
    /// An observer within this many metres stands the people up.
    pub stand_m: f32,
    /// Beyond this many metres, with nobody fighting, they fold away.
    pub fold_m: f32,
    /// Played hours a cleared one stays quiet.
    pub returns_h: f32,
    /// How far from a pad's centre the people stand, as shares of its
    /// radius, tried in order until one is clear of every building.
    pub ring: Vec<f32>,
    /// How far an angle is turned, radians, when a spot is taken by a
    /// building.
    pub nudges: Vec<f32>,
    /// How far a person stands from a building's wall, metres.
    pub clear_m: f32,
    /// Whose people they are. The holder's band unless the file says
    /// otherwise.
    #[serde(default)]
    pub crew: Crew,
    /// The roster of each tier, tier 1 first.
    pub tiers: Vec<Vec<Person>>,
    /// For a claim's people: the metres from the deposit's centre of the
    /// ring they stand on, which `ring` and `nudges` then take shares of.
    /// Required at a claim, unused at a site.
    #[serde(default)]
    pub post_m: Option<f32>,
}

impl EncounterDef {
    /// The roster of a tier (1-based), the last one for a tier past the
    /// list.
    #[must_use]
    pub fn roster(&self, tier: u8) -> &[Person] {
        let index = usize::from(tier.max(1)) - 1;
        self.tiers
            .get(index)
            .or_else(|| self.tiers.last())
            .map_or(&[], Vec::as_slice)
    }
}

/// Why an encounter was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EncounterError {
    /// Not valid RON for the file, which includes an opponent kind that
    /// does not exist.
    #[error("{0}")]
    Ron(String),
    /// A file of another version than this build reads.
    #[error("version {found}, this build reads {expected}")]
    Version {
        /// The version the file states.
        found: u32,
        /// The version this build reads.
        expected: u32,
    },
    /// Two files define one encounter.
    #[error("`{0}` is defined twice")]
    Duplicate(EncounterDefId),
    /// The encounter names a site kind nobody registered.
    #[error("`{encounter}`: site kind `{kind}` does not exist")]
    UnknownSite {
        /// The encounter.
        encounter: EncounterDefId,
        /// The kind it named.
        kind: Namespaced,
    },
    /// A dial breaks a rule.
    #[error("`{encounter}`: {reason}")]
    Dial {
        /// The encounter.
        encounter: EncounterDefId,
        /// What is wrong.
        reason: String,
    },
}

impl EncounterDef {
    fn check(&self) -> Result<(), String> {
        if !(self.stand_m > 0.0 && self.stand_m.is_finite()) {
            return Err("stand_m must be above nought".to_owned());
        }
        if !(self.fold_m > self.stand_m && self.fold_m.is_finite()) {
            return Err("fold_m must be beyond stand_m".to_owned());
        }
        if !(self.returns_h >= 0.0 && self.returns_h.is_finite()) {
            return Err("returns_h must not be negative".to_owned());
        }
        if self.ring.is_empty() || self.ring.iter().any(|s| !(*s > 0.0 && *s <= 1.0)) {
            return Err(
                "ring needs shares of the pad's radius above nought and up to one".to_owned(),
            );
        }
        if self.nudges.first() != Some(&0.0) || self.nudges.iter().any(|n| !n.is_finite()) {
            return Err("nudges must start at nought and be finite".to_owned());
        }
        if !(self.clear_m >= 0.0 && self.clear_m.is_finite()) {
            return Err("clear_m must not be negative".to_owned());
        }
        if self.at.claim().is_some() && !self.post_m.is_some_and(|m| m > 0.0 && m.is_finite()) {
            return Err("a claim's people need post_m above nought".to_owned());
        }
        if self.tiers.is_empty() {
            return Err("tiers needs at least one roster".to_owned());
        }
        for (index, roster) in self.tiers.iter().enumerate() {
            if roster.iter().map(|p| u32::from(p.count)).sum::<u32>() == 0 {
                return Err(format!("tier {} has nobody in it", index + 1));
            }
        }
        Ok(())
    }
}

/// Every encounter, checked, in id order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EncounterSet {
    encounters: Vec<EncounterDef>,
}

impl EncounterSet {
    /// Check `defs`; `site_exists` says whether a site kind is registered.
    ///
    /// # Errors
    /// The path of the first file that breaks a rule, and which.
    pub fn build(
        mut defs: Vec<(String, EncounterDef)>,
        site_exists: impl Fn(&Namespaced) -> bool,
    ) -> Result<Self, (String, EncounterError)> {
        defs.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
        for pair in defs.windows(2) {
            if pair[0].1.id == pair[1].1.id {
                return Err((
                    pair[1].0.clone(),
                    EncounterError::Duplicate(pair[1].1.id.clone()),
                ));
            }
        }
        for (path, def) in &defs {
            if def.version != ENCOUNTER_VERSION {
                return Err((
                    path.clone(),
                    EncounterError::Version {
                        found: def.version,
                        expected: ENCOUNTER_VERSION,
                    },
                ));
            }
            if let Some(kind) = def.at.site().filter(|kind| !site_exists(kind)) {
                return Err((
                    path.clone(),
                    EncounterError::UnknownSite {
                        encounter: def.id.clone(),
                        kind: kind.clone(),
                    },
                ));
            }
            def.check().map_err(|reason| {
                (
                    path.clone(),
                    EncounterError::Dial {
                        encounter: def.id.clone(),
                        reason,
                    },
                )
            })?;
        }
        Ok(Self {
            encounters: defs.into_iter().map(|(_, def)| def).collect(),
        })
    }

    /// One encounter by id.
    #[must_use]
    pub fn get(&self, id: &EncounterDefId) -> Option<&EncounterDef> {
        self.encounters.iter().find(|e| e.id == *id)
    }

    /// The encounter held by places of site kind `kind`.
    #[must_use]
    pub fn at(&self, kind: &Namespaced) -> Option<&EncounterDef> {
        self.encounters.iter().find(|e| e.at.site() == Some(kind))
    }

    /// The encounter held by claims of `kind`.
    #[must_use]
    pub fn at_claim(&self, kind: ClaimKind) -> Option<&EncounterDef> {
        self.encounters.iter().find(|e| e.at.claim() == Some(kind))
    }

    /// Every encounter, in id order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &EncounterDef> {
        self.encounters.iter()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.encounters.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::load_from_dir;

    const FILE: &str = include_str!("../../../assets/encounters/outlaw_camp.encounter.ron");
    const POST: &str = include_str!("../../../assets/encounters/concession_post.encounter.ron");

    fn built(text: &str) -> Result<EncounterSet, String> {
        let def: EncounterDef = ron::from_str(text).map_err(|e| e.to_string())?;
        EncounterSet::build(
            vec![("encounters/test.encounter.ron".to_owned(), def)],
            |k| k.to_string() == "strata:wild_camp",
        )
        .map_err(|(_, error)| error.to_string())
    }

    #[track_caller]
    fn refused(from: &str, to: &str, reason: &str) {
        let changed = FILE.replace(from, to);
        assert_ne!(changed, FILE, "`{from}` is in the file");
        let error = built(&changed).expect_err("the edit is refused");
        assert!(error.contains(reason), "`{error}` should say `{reason}`");
    }

    #[test]
    fn the_shipped_camp_loads_with_three_tiers() {
        let registries =
            load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap();
        let kind = Namespaced::parse("strata:wild_camp").unwrap();
        let camp = registries.encounters.at(&kind).expect("the camp's people");
        assert_eq!(camp.tiers.len(), 3);
        assert_eq!(camp.roster(1).len(), 2);
        assert_eq!(camp.roster(9), camp.roster(3), "a tier past the list");
        assert!((camp.stand_m - 260.0).abs() < 1e-6);
        assert_eq!(registries.encounters.iter().len(), 5);
        let guards = registries
            .encounters
            .at_claim(ClaimKind::Concession)
            .unwrap();
        assert_eq!(guards.tiers.len(), 3);
        assert!(guards.post_m.is_some());
        let wardens = registries.encounters.at_claim(ClaimKind::Reserve).unwrap();
        assert_eq!(wardens.roster(1).len(), 2);
        assert!(registries.encounters.at_claim(ClaimKind::Commons).is_none());
    }

    #[test]
    fn a_claims_people_need_a_ring_and_a_known_claim_kind() {
        let removed = POST.replace("post_m: Some(14.0),", "");
        assert_ne!(removed, POST);
        let error = built(&removed).expect_err("no ring");
        assert!(error.contains("post_m"), "{error}");
        let bad = POST.replace("Claim(Concession)", "Claim(Fence)");
        let error = built(&bad).expect_err("unknown kind");
        assert!(error.contains("Fence"), "{error}");
    }

    #[test]
    fn an_unknown_kind_or_site_is_refused_by_name() {
        refused("kind: Rusher, count: 1", "kind: Brute, count: 1", "Brute");
        refused("strata:wild_camp", "strata:nowhere", "strata:nowhere");
    }

    #[test]
    fn a_dial_out_of_order_is_refused() {
        refused("fold_m: 420.0", "fold_m: 100.0", "fold_m");
        refused("stand_m: 260.0", "stand_m: 0.0", "stand_m");
        refused("returns_h: 3.0", "returns_h: -1.0", "returns_h");
        refused("version: 1", "version: 2", "version");
        refused("ring: [0.55, 0.4, 0.7, 0.25]", "ring: [1.5]", "ring");
        refused("clear_m: 1.5", "clear_m: -1.0", "clear_m");
    }
}
