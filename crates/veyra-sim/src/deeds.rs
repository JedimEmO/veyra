//! The adapter half of the `deeds` rules: its requests, systems and plugin.

pub use veyra_rules::deeds::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use bevy::ecs::system::SystemParam;

use strata_registry::{ClaimKind, GameRegistries, Namespaced, Nature};

use veyra_world::deposits::Deposit;

use veyra_world::factions::{Claim, Faction, FactionId, FactionMap};

use crate::factions::Factions;

use crate::household::Purse;

use crate::standing::{EncounterId, Fights, PlayedClock, post_key};

/// Everything a rule that writes a deed or reads a stance asks, as one
/// parameter. Each part is optional so a world with no factions (a server, a
/// harness, a bare test world) is left alone.
#[derive(SystemParam)]
pub struct Ledger<'w> {
    factions: Option<Res<'w, Factions>>,
    clock: Option<Res<'w, PlayedClock>>,
    leases: Option<ResMut<'w, Leases>>,
    deeds: Option<ResMut<'w, Deeds>>,
    commons: Option<ResMut<'w, CommonsTaken>>,
    fights: Option<ResMut<'w, Fights>>,
}

impl Ledger<'_> {
    /// The played second now.
    #[must_use]
    pub fn now(&self) -> f64 {
        self.clock.as_ref().map_or(0.0, |clock| clock.seconds)
    }

    /// The world's factions, where there are any.
    #[must_use]
    pub fn map(&self) -> Option<&FactionMap> {
        self.factions.as_deref().map(|factions| &*factions.0)
    }

    /// The claim `deposit` is, if any.
    #[must_use]
    pub fn claim_of(&self, deposit: &Deposit) -> Option<Claim> {
        self.map()?.claim_of(deposit)
    }

    /// Working `deposit` without leave while its post stands uncrossed
    /// starts that post's warning, unless one runs. The deed is
    /// [`worked`]'s and [`planted`]'s; this is the post's answer to the same
    /// act, so it asks the same [`without_leave`].
    pub fn trespass(&mut self, registries: &GameRegistries, deposit: &Deposit) {
        let (Some(rules), Some(claim)) = (Rules::of(registries), self.claim_of(deposit)) else {
            return;
        };
        let now = self.now();
        let none = Leases::default();
        let leases = self.leases.as_deref().unwrap_or(&none);
        if !without_leave(claim, leases, now) {
            return;
        }
        if let Some(fights) = self.fights.as_mut() {
            fights.begin_warning(
                EncounterId::Post(post_key(deposit.id)),
                claim.holder,
                now + rules.claims.warn_s,
            );
        }
    }

    /// One `kind` deed against `faction`, written now.
    pub fn deed(&mut self, registries: &GameRegistries, faction: FactionId, kind: DeedKind) {
        self.deed_units(registries, faction, kind, 1);
    }

    /// `units` of a `kind` deed for or against `faction`, written now: what
    /// a quest's reply writes (ticket 0014).
    pub fn deed_units(
        &mut self,
        registries: &GameRegistries,
        faction: FactionId,
        kind: DeedKind,
        units: u32,
    ) {
        let Some(rules) = Rules::of(registries) else {
            return;
        };
        let now = self.now();
        if let Some(deeds) = self.deeds.as_mut() {
            deeds.write(faction, kind, units, now, rules.deeds);
        }
    }

    /// The post `id` is crossed. Whether it was not before.
    pub fn cross(&mut self, id: EncounterId) -> bool {
        self.fights.as_mut().is_some_and(|fights| fights.cross(id))
    }

    /// `units` of `deposit` worked: written as [`worked`] says.
    pub fn worked(&mut self, registries: &GameRegistries, deposit: &Deposit, units: u32) {
        self.trespass(registries, deposit);
        let (Some(rules), Some(claim)) = (Rules::of(registries), self.claim_of(deposit)) else {
            return;
        };
        let now = self.now();
        let none = Leases::default();
        let leases = self.leases.as_deref().unwrap_or(&none);
        let (Some(deeds), Some(commons)) = (self.deeds.as_mut(), self.commons.as_mut()) else {
            return;
        };
        // Marked changed only when something was written, so the save that
        // watches the books is not woken by a lawful cycle.
        let wrote = worked(
            deeds.bypass_change_detection(),
            commons.bypass_change_detection(),
            leases,
            rules,
            claim,
            deposit.id,
            units,
            now,
        );
        if wrote {
            deeds.set_changed();
        }
        if claim.kind == ClaimKind::Commons {
            commons.set_changed();
        }
    }

    /// A harvester planted on `deposit`.
    pub fn planted(&mut self, registries: &GameRegistries, deposit: &Deposit) {
        self.trespass(registries, deposit);
        let (Some(rules), Some(claim)) = (Rules::of(registries), self.claim_of(deposit)) else {
            return;
        };
        let now = self.now();
        let none = Leases::default();
        let leases = self.leases.as_deref().unwrap_or(&none);
        let Some(deeds) = self.deeds.as_mut() else {
            return;
        };
        if planted(deeds.bypass_change_detection(), leases, rules, claim, now) {
            deeds.set_changed();
        }
    }

    /// Buy a lease of `faction` at its board: the price leaves the purse and
    /// the lease runs the data's hours from now, or from where a running one
    /// ends. One transaction: a refusal has changed nothing.
    ///
    /// # Errors
    /// What [`check_lease`] refuses.
    pub fn lease(
        &mut self,
        registries: &GameRegistries,
        in_reach: bool,
        purse: &mut Purse,
        faction: &Faction,
    ) -> Result<Leased, LeaseRefusal> {
        let terms = self.faction_terms(registries, faction);
        let quote = check_lease(registries, in_reach, terms, purse, faction)?;
        let now = self.now();
        let leases = self.leases.as_mut().ok_or(LeaseRefusal::NoLease)?;
        let from = now + leases.left_s(faction.id, now);
        let until = from + quote.hours * SECONDS_PER_HOUR;
        leases.until.insert(faction.id.0, until);
        purse.0 -= quote.price;
        Ok(Leased {
            faction: faction.id,
            paid: quote.price,
            until,
        })
    }

    /// The leases held, where the world keeps them.
    #[must_use]
    pub fn leases(&self) -> Option<&Leases> {
        self.leases.as_deref()
    }

    /// The faction whose board is `buyer`'s, where the world has one.
    #[must_use]
    pub fn faction_of_buyer(
        &self,
        registries: &GameRegistries,
        buyer: &Namespaced,
    ) -> Option<Faction> {
        anchor_faction(self.map()?, registries, buyer)
    }

    /// The terms `faction`'s stance sets now, wherever it sells: its board's
    /// and its guards'. Neutral where no deeds or rules are kept.
    #[must_use]
    pub fn faction_terms(&self, registries: &GameRegistries, faction: &Faction) -> Terms {
        match (self.deeds.as_deref(), registries.factions.deeds()) {
            (Some(deeds), Some(rules)) => {
                Terms::of(stance_of(registries, deeds, faction, self.now()), rules)
            }
            _ => Terms::NEUTRAL,
        }
    }

    /// The terms of `buyer`'s board now: neutral where no faction keeps it.
    #[must_use]
    pub fn terms(&self, registries: &GameRegistries, buyer: &Namespaced) -> Terms {
        let (Some(map), Some(deeds)) = (self.map(), self.deeds.as_deref()) else {
            return Terms::NEUTRAL;
        };
        board_terms(map, registries, deeds, buyer, self.now()).1
    }

    /// A sale of `paid` credits at `buyer`'s board, a deed for its holder.
    pub fn sold(&mut self, registries: &GameRegistries, buyer: &Namespaced, paid: u32) {
        let (Some(rules), Some(faction)) = (
            Rules::of(registries),
            self.faction_of_buyer(registries, buyer),
        ) else {
            return;
        };
        let now = self.now();
        if let Some(deeds) = self.deeds.as_mut() {
            deeds.write(faction.id, DeedKind::Sold, paid, now, rules.deeds);
        }
    }

    /// A camp at `centre` cleared: a deed for each faction other than an
    /// outlaw band that holds a site within the rules' reach of it.
    pub fn cleared(&mut self, registries: &GameRegistries, centre: [f64; 2]) {
        let (Some(rules), Some(map)) = (Rules::of(registries), self.map()) else {
            return;
        };
        let mut holders: Vec<FactionId> = map
            .holdings_near(centre[0], centre[1], rules.deeds.camp_reach_m)
            .into_iter()
            .map(|holding| holding.faction)
            .filter(|id| map.faction(*id).is_some_and(|f| f.nature != Nature::Outlaw))
            .collect();
        holders.sort();
        holders.dedup();
        let now = self.now();
        if let Some(deeds) = self.deeds.as_mut() {
            for holder in holders {
                deeds.write(holder, DeedKind::ClearedCamp, 1, now, rules.deeds);
            }
        }
    }
}

/// Registers the ledgers. Added by `SimPlugin`; with no faction data they
/// stay empty.
#[derive(Debug, Clone, Copy)]
pub struct DeedsPlugin;

impl Plugin for DeedsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Deeds>()
            .init_resource::<Leases>()
            .init_resource::<CommonsTaken>();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use std::sync::Arc;

    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    use crate::crafting::tests::first_day;

    use crate::factions::Factions;

    pub(crate) use veyra_rules::deeds::test_support::*;

    /// The default app with the city's factions published.
    fn app_with_factions() -> bevy::app::App {
        let mut app = first_day();
        app.insert_resource(Factions(Arc::clone(&pelagic().0)));
        app
    }

    #[test]
    fn sampling_a_concession_writes_a_deed_against_its_holder_and_a_lease_none() {
        let registries = registries();
        let (_, faction, deposit) = pelagic();
        let mut app = app_with_factions();
        let work = |app: &mut bevy::app::App| {
            let (registries, deposit) = (registries.clone(), deposit.clone());
            app.world_mut()
                .run_system_once(move |mut ledger: Ledger| {
                    ledger.worked(&registries.0, &deposit, 4);
                })
                .unwrap();
        };
        work(&mut app);
        let deeds = app.world().resource::<Deeds>().clone();
        assert_eq!(deeds.by[&faction.id.0][0].kind, DeedKind::WorkedConcession);
        let rules = registries.0.factions.deeds().unwrap();
        assert!(score(&deeds, faction.id, 0.0, rules) < 0.0);
        let mut leased = app_with_factions();
        leased
            .world_mut()
            .resource_mut::<Leases>()
            .until
            .insert(faction.id.0, 6.0 * HOUR);
        work(&mut leased);
        assert!(leased.world().resource::<Deeds>().by.is_empty());
    }

    #[test]
    fn a_cleared_camp_writes_a_deed_for_the_neighbour_whose_ground_it_is_near() {
        let registries = registries();
        let (map, faction, _) = pelagic();
        let mut app = app_with_factions();
        let city = map.world().sites()[0].centre;
        let near = [city[0] + 4_000.0, city[1]];
        let regs = registries.clone();
        app.world_mut()
            .run_system_once(move |mut ledger: Ledger| ledger.cleared(&regs.0, near))
            .unwrap();
        let deeds = app.world().resource::<Deeds>();
        let list = &deeds.by[&faction.id.0];
        assert_eq!(list[0].kind, DeedKind::ClearedCamp);
        assert_eq!(list[0].amount, 1);
    }
}
