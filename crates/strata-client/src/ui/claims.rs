//! Whose a found deposit is and how a faction stands towards the household,
//! in words (ticket 0012, step 3). The sim holds the truth
//! (`strata_worldgen::factions::FactionMap::claim_of`, `strata_sim::deeds`);
//! this reads it and decides no rule. The ledger behind a stance is hidden by
//! design: nothing here returns, and no screen shows, a score.

use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use strata_registry::{ClaimKind, GameRegistries};
use strata_sim::Registries;
use strata_sim::deeds::{Deeds, Leases, Stance, Terms, stance_of};
use strata_sim::deposits::Deposit;
use strata_sim::encounters::PlayedClock;
use strata_sim::factions::{Faction, FactionMap, Factions};

use super::strings::english;

/// What the words are read from: the faction map, the ledger, the leases and
/// the played clock. Borrowed, so a screen built from the `World` and a
/// system with parameters ask the same functions.
#[derive(Clone, Copy)]
pub struct Books<'a> {
    /// The data.
    pub registries: &'a GameRegistries,
    /// The world's factions; none in a world without any.
    pub map: Option<&'a FactionMap>,
    /// The hidden ledger.
    pub deeds: &'a Deeds,
    /// The leases held.
    pub leases: &'a Leases,
    /// The played second now.
    pub now: f64,
}

/// A name that may open with "the" set to open a sentence.
fn capital(name: &str) -> String {
    let mut name = name.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    name
}

/// `name` as a possessive, with a capital to open a line: `Pelagic Works'`,
/// `The Moher Trust's`.
#[must_use]
pub fn possessive(name: &str) -> String {
    let name = capital(name);
    if name.ends_with('s') {
        format!("{name}'")
    } else {
        format!("{name}'s")
    }
}

impl Books<'_> {
    /// The claim a deposit is: its kind and its holder.
    #[must_use]
    pub fn claim(&self, deposit: &Deposit) -> Option<(ClaimKind, Faction)> {
        let map = self.map?;
        let claim = map.claim_of(deposit)?;
        Some((claim.kind, map.faction(claim.holder)?))
    }

    /// `Concession of Pelagic Works` (with `leased` while a lease runs),
    /// `Commons of ...`, `Reserve of ...`; `None` for open ground.
    #[must_use]
    pub fn claim_words(&self, deposit: &Deposit) -> Option<String> {
        let (kind, holder) = self.claim(deposit)?;
        let leased = kind == ClaimKind::Concession && self.leases.live(holder.id, self.now);
        let key = match (kind, leased) {
            (ClaimKind::Concession, true) => "strata.claim.concession.leased",
            (ClaimKind::Concession, false) => "strata.claim.concession",
            (ClaimKind::Commons, _) => "strata.claim.commons",
            (ClaimKind::Reserve, _) => "strata.claim.reserve",
        };
        Some(english(key)?.replace("{faction}", &holder.name))
    }

    /// What working the deposit would mean, where it would count against the
    /// household: a concession with no lease running, a reserve, a commons.
    #[must_use]
    pub fn warning(&self, deposit: &Deposit) -> Option<String> {
        let (kind, holder) = self.claim(deposit)?;
        let key = match kind {
            ClaimKind::Concession if self.leases.live(holder.id, self.now) => return None,
            ClaimKind::Concession => "strata.claim.warn.concession",
            ClaimKind::Commons => "strata.claim.warn.commons",
            ClaimKind::Reserve => "strata.claim.warn.reserve",
        };
        Some(english(key)?.replace("{faction}", &possessive(&holder.name)))
    }

    /// How `faction` is towards the household, as words.
    #[must_use]
    pub fn stance_words(&self, faction: &Faction) -> Option<String> {
        let stance = stance_of(self.registries, self.deeds, faction, self.now);
        let key = match stance {
            Stance::Welcoming => "strata.stance.welcoming",
            Stance::Wary => "strata.stance.wary",
            Stance::Hostile => "strata.stance.hostile",
        };
        english(key).map(str::to_owned)
    }
}

/// The parts of the world the words are read from, as one system parameter.
#[derive(SystemParam)]
pub struct Standing<'w> {
    registries: Option<Res<'w, Registries>>,
    factions: Option<Res<'w, Factions>>,
    deeds: Option<Res<'w, Deeds>>,
    leases: Option<Res<'w, Leases>>,
    clock: Option<Res<'w, PlayedClock>>,
}

impl Standing<'_> {
    /// The books, where the world has them all.
    #[must_use]
    pub fn books(&self) -> Option<Books<'_>> {
        Some(Books {
            registries: &self.registries.as_deref()?.0,
            map: self.factions.as_deref().map(|factions| &*factions.0),
            deeds: self.deeds.as_deref()?,
            leases: self.leases.as_deref()?,
            now: self.clock.as_deref().map_or(0.0, |clock| clock.seconds),
        })
    }

    /// Whether a lease changed since the last run, so a claim line is read
    /// again.
    #[must_use]
    pub fn leases_changed(&self) -> bool {
        self.leases
            .as_ref()
            .is_some_and(bevy::ecs::change_detection::DetectChanges::is_changed)
    }
}

/// The books of a `World`, for a screen built from one.
#[must_use]
pub fn books_of(world: &World) -> Option<Books<'_>> {
    Some(Books {
        registries: &world.get_resource::<Registries>()?.0,
        map: world
            .get_resource::<Factions>()
            .map(|factions| &*factions.0),
        deeds: world.get_resource::<Deeds>()?,
        leases: world.get_resource::<Leases>()?,
        now: world
            .get_resource::<PlayedClock>()
            .map_or(0.0, |clock| clock.seconds),
    })
}

impl Books<'_> {
    /// The faction whose board is `buyer`'s and what its stance makes of the
    /// board's money; neutral terms and no faction where nobody keeps it.
    #[must_use]
    pub fn board(&self, buyer: &strata_registry::Namespaced) -> (Option<Faction>, Terms) {
        match self.map {
            Some(map) => {
                strata_sim::deeds::board_terms(map, self.registries, self.deeds, buyer, self.now)
            }
            None => (None, Terms::NEUTRAL),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use std::sync::{Arc, OnceLock};

    use strata_core::Seed;
    use strata_worldgen::ProceduralWorld;

    use super::*;
    use crate::ui::testing::{deposit, shipped};

    /// Port Serein's factions, and a deposit of the top grade in the city,
    /// which is Pelagic Works' concession. Made once: the world's water is a
    /// bake.
    pub(crate) fn city() -> &'static (Arc<FactionMap>, Deposit) {
        static MADE: OnceLock<(Arc<FactionMap>, Deposit)> = OnceLock::new();
        MADE.get_or_init(|| {
            let registries = Arc::new(shipped());
            let world = Arc::new(ProceduralWorld::new(Seed(42), &registries).with_own_water());
            world.hydrology().set_threads(4);
            let centre = world.sites()[0].centre;
            world.prepare_water(centre[0], centre[1], 20_000.0);
            let map = Arc::new(FactionMap::new(world, Seed(42), Arc::clone(&registries)));
            let mut lot = deposit(&registries, "strata:evaporite_salt", "Test", [0, 0], centre);
            lot.grade_pm = 999;
            (map, lot)
        })
    }

    #[test]
    fn a_found_claim_is_said_in_words_and_a_lease_changes_them() {
        let registries = shipped();
        let (map, lot) = city();
        let deeds = Deeds::default();
        let mut held = Leases::default();
        let plain = Books {
            registries: &registries,
            map: Some(map),
            deeds: &deeds,
            leases: &held,
            now: 0.0,
        };
        let claim = plain.claim_words(lot).expect("a claim");
        let warning = plain.warning(lot).expect("a warning");
        assert!(claim.starts_with("Concession of Pelagic Works"), "{claim}");
        assert!(!claim.contains("leased"));
        assert!(warning.contains("lease"), "{warning}");
        let holder = map.claim_of(lot).unwrap().holder;
        held.until.insert(holder.0, 3_600.0);
        let leased = Books {
            registries: &registries,
            map: Some(map),
            deeds: &deeds,
            leases: &held,
            now: 0.0,
        };
        assert!(leased.claim_words(lot).unwrap().contains("leased"));
        assert_eq!(leased.warning(lot), None);
    }

    #[test]
    fn the_register_and_the_map_carry_the_claim() {
        use strata_sim::deposits::{NearDeposits, Surveyed};

        use crate::ui::bench::RegisterView;

        let registries = shipped();
        let (map, lot) = city();
        let deeds = Deeds::default();
        let leases = Leases::default();
        let books = Books {
            registries: &registries,
            map: Some(map),
            deeds: &deeds,
            leases: &leases,
            now: 0.0,
        };
        let near = NearDeposits {
            deposits: vec![lot.clone()],
            around: Vec::new(),
        };
        let mut surveyed = Surveyed::default();
        surveyed.found.insert(
            lot.id,
            strata_sim::deposits::Found {
                reading: strata_registry::Reading::Type,
                sampled: false,
                best_pm: 0,
                best_at: [0., 0.],
            },
        );
        let view = RegisterView::of(&registries, &near, &surveyed, [0., 0.], None, true)
            .with_claims(Some(&books), &near);
        let row = &view.rows[0];
        assert!(row.claim.starts_with("Concession of Pelagic Works"));
        assert!(row.card.notes.iter().any(|note| note.contains("lease")));
        let panel = crate::ui::map::deposit_words(
            &registries,
            &surveyed,
            lot,
            [0., 0.],
            false,
            Some(&books),
        );
        assert!(panel.lines[2].contains("Concession of Pelagic Works"));
    }
}
