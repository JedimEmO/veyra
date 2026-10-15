//! Deeds, leases and stance (ticket 0012, step 3; the design is section 7 of
//! `docs/tickets/0012-factions-and-the-world-at-arms.md`).
//!
//! A faction keeps a hidden ledger of what the household did to it and for
//! it: [`Deeds`]. One pure rule, [`score`], weighs the entries by
//! `assets/factions/deeds.ron` and halves them over played time; [`stance`]
//! reads the sum against the bands and the anchor's start. Nothing here is
//! ever a number on a screen: the client shows a stance as words and a
//! board's prices.
//!
//! The rules that see a deed write it: sampling, planting and collecting on
//! a claim, a sale at a board and a camp cleared near a faction's ground.
//! They reach the ledger through [`Ledger`], one parameter that is quiet when
//! the game has no factions (a server, a harness, most tests). The claim
//! itself is `strata_worldgen::factions::FactionMap::claim_of`, a pure
//! function of the deposit and the holdings.

use std::collections::BTreeMap;

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use serde::{Deserialize, Serialize};
use strata_registry::{
    AnchorDef, ClaimKind, ClaimRules, DeedRules, DeedWeights, GameRegistries, Namespaced, Nature,
};
use strata_worldgen::deposits::{Deposit, DepositId};
use strata_worldgen::factions::{Claim, Faction, FactionId, FactionMap};

use crate::deposits::Purse;
use crate::encounters::{EncounterId, Fights, PlayedClock, post_key};
use crate::factions::Factions;

/// Seconds in a played hour: a unit of time, not a dial.
const SECONDS_PER_HOUR: f64 = 3600.0;

/// How many half lives old a deed is before it is forgotten outright: its
/// weight is then under a millionth.
const FORGOTTEN_HALF_LIVES: f64 = 20.0;

/// How close in played seconds two deeds of one kind must be to be kept as
/// one entry, so a long sampling is a few entries and not one a cycle.
const MERGE_WITHIN_S: f64 = 60.0;

/// What a deed was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DeedKind {
    /// Units worked on its concession without a lease.
    WorkedConcession,
    /// Units worked on its reserve.
    WorkedReserve,
    /// Units taken from its commons past the share of a played day.
    TookPastShare,
    /// A harvester planted on its concession without a lease or on its
    /// reserve.
    Planted,
    /// Credits paid at its board.
    Sold,
    /// An outlaw camp cleared near its ground.
    ClearedCamp,
    /// A person of its post struck, once a person.
    Hurt,
    /// A person of its post killed.
    Killed,
    /// A posting of its board collected.
    JobDone,
}

impl DeedKind {
    /// What one unit of the deed weighs.
    #[must_use]
    pub fn weight(self, weights: &DeedWeights) -> f64 {
        match self {
            Self::WorkedConcession => weights.worked_concession,
            Self::WorkedReserve => weights.worked_reserve,
            Self::TookPastShare => weights.took_past_share,
            Self::Planted => weights.planted,
            Self::Sold => weights.sold_per_credit,
            Self::ClearedCamp => weights.cleared_camp,
            Self::JobDone => weights.job_done,
            Self::Hurt => weights.hurt,
            Self::Killed => weights.killed,
        }
    }
}

/// One entry of a faction's ledger.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Deed {
    /// What was done.
    pub kind: DeedKind,
    /// How much of it: units, credits, or one.
    pub amount: u32,
    /// The played second it was done (the latest, of a merged entry).
    pub at: f64,
}

/// Every faction's deeds, by [`FactionId`]'s number. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Deeds {
    /// The entries, oldest first.
    pub by: BTreeMap<u64, Vec<Deed>>,
}

impl Deeds {
    /// Write `amount` of `kind` against or for `faction` at played second
    /// `now`: joined to the last entry of its kind if that was within a
    /// minute, and old entries past forgetting dropped.
    pub fn write(
        &mut self,
        faction: FactionId,
        kind: DeedKind,
        amount: u32,
        now: f64,
        rules: &DeedRules,
    ) {
        if amount == 0 {
            return;
        }
        let list = self.by.entry(faction.0).or_default();
        let forgotten = FORGOTTEN_HALF_LIVES * rules.half_life_h * SECONDS_PER_HOUR;
        list.retain(|deed| now - deed.at <= forgotten);
        match list
            .iter_mut()
            .rev()
            .find(|deed| deed.kind == kind && (now - deed.at).abs() <= MERGE_WITHIN_S)
        {
            // The entry keeps its first date, so a long sampling is an entry
            // a minute rather than one whose date runs ahead of its units.
            Some(joined) => joined.amount = joined.amount.saturating_add(amount),
            None => list.push(Deed {
                kind,
                amount,
                at: now,
            }),
        }
    }
}

/// The sum of `faction`'s deeds at played second `now`: each weighs its
/// kind's weight a unit and has halved once for every half life since.
#[must_use]
pub fn score(deeds: &Deeds, faction: FactionId, now: f64, rules: &DeedRules) -> f64 {
    let half = rules.half_life_h * SECONDS_PER_HOUR;
    deeds.by.get(&faction.0).map_or(0.0, |list| {
        list.iter()
            .map(|deed| {
                let age = (now - deed.at).max(0.0);
                deed.kind.weight(&rules.weights) * f64::from(deed.amount) * 0.5_f64.powf(age / half)
            })
            .sum()
    })
}

/// How a faction is towards the household: a word, never a figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stance {
    /// It knows the household well.
    Welcoming,
    /// It is wary of strangers.
    Wary,
    /// It wants the household gone.
    Hostile,
}

/// The stance a `standing` (the start plus the deeds' score) falls in.
#[must_use]
pub fn stance(standing: f64, rules: &DeedRules) -> Stance {
    if standing >= rules.bands.welcoming_at {
        Stance::Welcoming
    } else if standing <= rules.bands.hostile_at {
        Stance::Hostile
    } else {
        Stance::Wary
    }
}

/// The authored holder `faction` is, if it is one: the anchors' names are
/// unique among factions.
#[must_use]
pub fn anchor_of<'a>(registries: &'a GameRegistries, faction: &Faction) -> Option<&'a AnchorDef> {
    registries
        .factions
        .anchors()
        .iter()
        .find(|anchor| anchor.name == faction.name)
}

/// Where the household's standing with `faction` starts: an anchor's own
/// figure, nought for a procedural faction.
#[must_use]
pub fn start_of(registries: &GameRegistries, faction: &Faction) -> f64 {
    anchor_of(registries, faction).map_or(0.0, |anchor| anchor.start)
}

/// The standing with `faction` at `now`: its start plus its deeds' score.
/// `None` for a data set with no deed rules.
#[must_use]
pub fn standing(
    registries: &GameRegistries,
    deeds: &Deeds,
    faction: &Faction,
    now: f64,
) -> Option<f64> {
    let rules = registries.factions.deeds()?;
    Some(start_of(registries, faction) + score(deeds, faction.id, now, rules))
}

/// How `faction` is towards the household at `now`: wary where nothing is
/// known.
#[must_use]
pub fn stance_of(
    registries: &GameRegistries,
    deeds: &Deeds,
    faction: &Faction,
    now: f64,
) -> Stance {
    let Some(rules) = registries.factions.deeds() else {
        return Stance::Wary;
    };
    standing(registries, deeds, faction, now).map_or(Stance::Wary, |value| stance(value, rules))
}

/// What a stance does to a board's money, in whole thousandths so a sale
/// and a price stay in whole numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    /// What a sale's pay is multiplied by, thousandths.
    pub pays_pm: u32,
    /// What a purchase's price is multiplied by, thousandths.
    pub sells_pm: u32,
    /// Whether the board trades at all.
    pub refuses: bool,
}

impl Terms {
    /// A board with no stance to read: prices as the data say.
    pub const NEUTRAL: Self = Self {
        pays_pm: PER_MILLE,
        sells_pm: PER_MILLE,
        refuses: false,
    };

    /// The terms of a `stance` under `rules`.
    #[must_use]
    pub fn of(stance: Stance, rules: &DeedRules) -> Self {
        let per_mille = |factor: f64| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a checked positive factor of about one, in thousandths"
            )]
            let pm = (factor * f64::from(PER_MILLE)).round() as u32;
            pm
        };
        let from = |factor: strata_registry::PriceFactor| Self {
            pays_pm: per_mille(factor.pays),
            sells_pm: per_mille(factor.sells),
            refuses: false,
        };
        match stance {
            Stance::Welcoming => from(rules.welcoming),
            Stance::Wary => from(rules.wary),
            Stance::Hostile => Self {
                refuses: true,
                ..Self::NEUTRAL
            },
        }
    }

    /// `amount` scaled by `pm` thousandths, rounded down.
    #[must_use]
    pub fn scale(amount: u32, pm: u32) -> u32 {
        u32::try_from(u64::from(amount) * u64::from(pm) / u64::from(PER_MILLE)).unwrap_or(u32::MAX)
    }
}

/// One in thousandths.
const PER_MILLE: u32 = 1000;

/// The leases the household holds: the played second each faction's runs
/// out. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Leases {
    /// Expiry in played seconds, by [`FactionId`]'s number.
    pub until: BTreeMap<u64, f64>,
}

impl Leases {
    /// Whether a lease of `faction` is running at `now`.
    #[must_use]
    pub fn live(&self, faction: FactionId, now: f64) -> bool {
        self.left_s(faction, now) > 0.0
    }

    /// Played seconds a lease of `faction` has left at `now`; nought when
    /// none runs.
    #[must_use]
    pub fn left_s(&self, faction: FactionId, now: f64) -> f64 {
        self.until
            .get(&faction.0)
            .map_or(0.0, |until| (until - now).max(0.0))
    }
}

/// What the household has taken from each commons today, by deposit: the
/// played day and the units taken in it. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommonsTaken {
    /// `(day, units)` by deposit; a new day starts the tally again.
    pub by: BTreeMap<DepositId, (u64, u32)>,
}

/// The two rule files a deed is written under.
#[derive(Debug, Clone, Copy)]
pub struct Rules<'a> {
    /// Which deposits are claims, and the commons' share.
    pub claims: &'a ClaimRules,
    /// What deeds weigh.
    pub deeds: &'a DeedRules,
}

impl<'a> Rules<'a> {
    /// The rules of `registries`; `None` where either file is missing, so
    /// nothing is claimed and nothing remembered.
    #[must_use]
    pub fn of(registries: &'a GameRegistries) -> Option<Self> {
        Some(Self {
            claims: registries.factions.claims()?,
            deeds: registries.factions.deeds()?,
        })
    }

    /// The played day `now` falls in.
    #[must_use]
    pub fn day(&self, now: f64) -> u64 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a played clock is never negative and a day count is small"
        )]
        let day = (now.max(0.0) / (self.claims.day_h * SECONDS_PER_HOUR)) as u64;
        day
    }
}

/// Work `units` of a claimed deposit: a concession without a live lease or
/// a reserve writes a deed against its holder, and a commons only for the
/// units past its share in the played day. Whether anything was written.
#[allow(
    clippy::too_many_arguments,
    reason = "the three books, the rules, the claim and the work: each its own thing"
)]
pub fn worked(
    deeds: &mut Deeds,
    commons: &mut CommonsTaken,
    leases: &Leases,
    rules: Rules<'_>,
    claim: Claim,
    deposit: DepositId,
    units: u32,
    now: f64,
) -> bool {
    let (kind, amount) = match claim.kind {
        ClaimKind::Concession if !leases.live(claim.holder, now) => {
            (DeedKind::WorkedConcession, units)
        }
        ClaimKind::Concession => return false,
        ClaimKind::Reserve => (DeedKind::WorkedReserve, units),
        ClaimKind::Commons => {
            let day = rules.day(now);
            let tally = commons.by.entry(deposit).or_insert((day, 0));
            if tally.0 != day {
                *tally = (day, 0);
            }
            let share = rules.claims.commons_share;
            let before = tally.1.saturating_sub(share);
            tally.1 = tally.1.saturating_add(units);
            (
                DeedKind::TookPastShare,
                tally.1.saturating_sub(share) - before,
            )
        }
    };
    if amount == 0 {
        return false;
    }
    deeds.write(claim.holder, kind, amount, now, rules.deeds);
    true
}

/// Whether working `claim` now is working it without leave: a concession
/// with no live lease, or a reserve at all. The rule that writes the deed
/// and the one that starts a post's warning.
#[must_use]
pub fn without_leave(claim: Claim, leases: &Leases, now: f64) -> bool {
    match claim.kind {
        ClaimKind::Concession => !leases.live(claim.holder, now),
        ClaimKind::Reserve => true,
        ClaimKind::Commons => false,
    }
}

/// Plant a harvester on a claim: a deed on a concession without a lease or
/// on a reserve. A commons is open to a machine as to a hand. Whether
/// anything was written.
pub fn planted(
    deeds: &mut Deeds,
    leases: &Leases,
    rules: Rules<'_>,
    claim: Claim,
    now: f64,
) -> bool {
    let against = without_leave(claim, leases, now);
    if against {
        deeds.write(claim.holder, DeedKind::Planted, 1, now, rules.deeds);
    }
    against
}

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
        let Some(rules) = Rules::of(registries) else {
            return;
        };
        let now = self.now();
        if let Some(deeds) = self.deeds.as_mut() {
            deeds.write(faction, kind, 1, now, rules.deeds);
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

/// The faction whose board is `buyer`'s: the anchor that keeps it, as the
/// map knows it.
#[must_use]
pub fn anchor_faction(
    map: &FactionMap,
    registries: &GameRegistries,
    buyer: &Namespaced,
) -> Option<Faction> {
    let anchor = registries.factions.anchor_of_buyer(buyer)?;
    let name = anchor.settlement.to_string();
    let site = map.world().sites().iter().find(|site| site.id == name)?;
    map.factions_near(site.centre[0], site.centre[1], 0.0)
        .into_iter()
        .find(|faction| faction.name == anchor.name)
}

/// The faction whose board is `buyer`'s and the terms its stance sets there:
/// the one rule a sale, a purchase and a screen all ask. Neutral terms and no
/// faction where nobody keeps the board.
#[must_use]
pub fn board_terms(
    map: &FactionMap,
    registries: &GameRegistries,
    deeds: &Deeds,
    buyer: &Namespaced,
    now: f64,
) -> (Option<Faction>, Terms) {
    let faction = anchor_faction(map, registries, buyer);
    let terms = match (&faction, registries.factions.deeds()) {
        (Some(faction), Some(rules)) => {
            Terms::of(stance_of(registries, deeds, faction, now), rules)
        }
        _ => Terms::NEUTRAL,
    };
    (faction, terms)
}

/// Why a lease is not sold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseRefusal {
    /// No board of the faction is in reach.
    NotAtBoard,
    /// The faction has no concession to lease, or no board to lease it at.
    NoLease,
    /// The faction wants the household gone.
    Hostile,
    /// The purse is short of the price.
    Short {
        /// The price, in credits.
        needs: u32,
        /// The purse.
        have: u32,
    },
}

/// What a lease costs and runs: the pure answer a request and a screen both
/// ask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaseQuote {
    /// Credits, after the stance.
    pub price: u32,
    /// Played hours it runs.
    pub hours: f64,
}

/// What a bought lease did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leased {
    /// Whose concessions it opens.
    pub faction: FactionId,
    /// What it cost, in credits.
    pub paid: u32,
    /// The played second it runs out.
    pub until: f64,
}

/// The buyer whose board sells `faction`'s lease: its anchor's, if the
/// faction is one with a board and claims concessions.
#[must_use]
pub fn lease_buyer(registries: &GameRegistries, faction: &Faction) -> Option<Namespaced> {
    let claims = registries.factions.claims()?;
    (claims.kind_of(faction.nature) == Some(ClaimKind::Concession))
        .then(|| anchor_of(registries, faction)?.buyer.clone())
        .flatten()
}

/// Whether `faction` leases its concessions at all: its claims are
/// concessions, whether it keeps a board or only guards.
#[must_use]
pub fn leases_concessions(registries: &GameRegistries, faction: &Faction) -> bool {
    registries
        .factions
        .claims()
        .is_some_and(|claims| claims.kind_of(faction.nature) == Some(ClaimKind::Concession))
}

/// Whether a lease of `faction` can be bought now, and at what price: the
/// member must be where it is sold (`in_reach`: its anchor's board, or one
/// of its guards at a concession), the stance must not be hostile and the
/// purse must hold the price, which the stance's sell factor moves as it
/// does a ware's.
///
/// # Errors
/// A [`LeaseRefusal`], the first that applies in the order it is declared.
pub fn check_lease(
    registries: &GameRegistries,
    in_reach: bool,
    terms: Terms,
    purse: &Purse,
    faction: &Faction,
) -> Result<LeaseQuote, LeaseRefusal> {
    let Some(claims) = registries.factions.claims() else {
        return Err(LeaseRefusal::NoLease);
    };
    if !leases_concessions(registries, faction) {
        return Err(LeaseRefusal::NoLease);
    }
    if !in_reach {
        return Err(LeaseRefusal::NotAtBoard);
    }
    if terms.refuses {
        return Err(LeaseRefusal::Hostile);
    }
    let price = Terms::scale(claims.lease.price, terms.sells_pm);
    if price > purse.0 {
        return Err(LeaseRefusal::Short {
            needs: price,
            have: purse.0,
        });
    }
    Ok(LeaseQuote {
        price,
        hours: claims.lease.hours,
    })
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

    use std::sync::{Arc, OnceLock};

    use bevy::ecs::system::RunSystemOnce;
    use strata_core::Seed;
    use strata_worldgen::ProceduralWorld;
    use strata_worldgen::deposits::DepositField;

    use super::*;
    use crate::Registries;
    use crate::crafting::tests::{first_day, shipped};

    const HOUR: f64 = SECONDS_PER_HOUR;

    fn registries() -> Registries {
        shipped()
    }

    /// The shipped assets, for the encounter tests that read the ledger.
    pub(crate) fn shipped_registries() -> Registries {
        shipped()
    }

    /// Port Serein's world with its factions, made once for every test: the
    /// map, Pelagic Works, and a deposit standing in the city (so the
    /// ground is Pelagic's at full strength) of the top grade.
    pub(crate) fn pelagic() -> &'static (Arc<FactionMap>, Faction, Deposit) {
        static MADE: OnceLock<(Arc<FactionMap>, Faction, Deposit)> = OnceLock::new();
        MADE.get_or_init(|| {
            let registries = Arc::new(shipped().0.as_ref().clone());
            let world = Arc::new(ProceduralWorld::new(Seed(42), &registries).with_own_water());
            world.hydrology().set_threads(4);
            let city = world.sites()[0].centre;
            world.prepare_water(city[0], city[1], 20_000.0);
            let map = Arc::new(FactionMap::new(
                Arc::clone(&world),
                Seed(42),
                Arc::clone(&registries),
            ));
            let field = DepositField::new(world, Seed(42), Arc::clone(&registries));
            let deposit = Deposit {
                at: city,
                grade_pm: 999,
                ..field.near(city[0], city[1], 6_000.0).remove(0)
            };
            let faction = map.faction(map.territory(city[0], city[1]).holder.unwrap());
            (map, faction.unwrap(), deposit)
        })
    }

    /// A claim of `kind` held by a made-up faction.
    fn claim(kind: ClaimKind) -> Claim {
        Claim {
            holder: FactionId(77),
            kind,
        }
    }

    #[test]
    fn a_deeds_score_halves_over_the_half_life() {
        let registries = registries();
        let rules = *registries.0.factions.deeds().unwrap();
        let mut deeds = Deeds::default();
        let faction = FactionId(1);
        deeds.write(faction, DeedKind::Planted, 1, 0.0, &rules);
        let fresh = score(&deeds, faction, 0.0, &rules);
        assert!((fresh - rules.weights.planted).abs() < 1e-9);
        let half = score(&deeds, faction, rules.half_life_h * HOUR, &rules);
        assert!((half - fresh / 2.0).abs() < 1e-9, "{half}");
        let quarter = score(&deeds, faction, 2.0 * rules.half_life_h * HOUR, &rules);
        assert!((quarter - fresh / 4.0).abs() < 1e-9);
        assert!(score(&deeds, FactionId(2), 0.0, &rules).abs() < 1e-12);
        // Many cycles in a minute are one entry; a later hour a second.
        for second in 1..30 {
            deeds.write(
                faction,
                DeedKind::WorkedConcession,
                3,
                f64::from(second),
                &rules,
            );
        }
        deeds.write(faction, DeedKind::WorkedConcession, 3, HOUR, &rules);
        assert_eq!(deeds.by[&faction.0].len(), 3);
    }

    #[test]
    fn the_bands_give_three_stances() {
        let registries = registries();
        let rules = registries.0.factions.deeds().unwrap();
        let bands = rules.bands;
        assert_eq!(stance(bands.welcoming_at, rules), Stance::Welcoming);
        assert_eq!(stance(bands.welcoming_at + 50.0, rules), Stance::Welcoming);
        assert_eq!(stance(0.0, rules), Stance::Wary);
        assert_eq!(stance(bands.hostile_at + 0.01, rules), Stance::Wary);
        assert_eq!(stance(bands.hostile_at, rules), Stance::Hostile);
        assert_eq!(stance(bands.hostile_at - 50.0, rules), Stance::Hostile);
        let welcoming = Terms::of(Stance::Welcoming, rules);
        assert_eq!((welcoming.pays_pm, welcoming.sells_pm), (1050, 950));
        assert_eq!(Terms::of(Stance::Wary, rules), Terms::NEUTRAL);
        assert!(Terms::of(Stance::Hostile, rules).refuses);
    }

    /// Work `units` of `claim` at `now` on the books, returning whether a
    /// deed was written.
    fn work(
        books: &mut (Deeds, CommonsTaken, Leases),
        registries: &Registries,
        kind: ClaimKind,
        units: u32,
        now: f64,
    ) -> bool {
        let rules = Rules::of(&registries.0).unwrap();
        worked(
            &mut books.0,
            &mut books.1,
            &books.2,
            rules,
            claim(kind),
            DepositId([3, 4]),
            units,
            now,
        )
    }

    #[test]
    fn a_concession_is_worked_without_a_deed_only_under_a_lease() {
        let registries = registries();
        let mut books = (Deeds::default(), CommonsTaken::default(), Leases::default());
        assert!(work(
            &mut books,
            &registries,
            ClaimKind::Concession,
            5,
            10.0
        ));
        assert_eq!(books.0.by[&77][0].kind, DeedKind::WorkedConcession);
        assert_eq!(books.0.by[&77][0].amount, 5);
        let mut leased = (Deeds::default(), CommonsTaken::default(), Leases::default());
        leased.2.until.insert(77, 10.0 + HOUR);
        assert!(!work(
            &mut leased,
            &registries,
            ClaimKind::Concession,
            5,
            10.0
        ));
        assert!(leased.0.by.is_empty());
        // The lease runs out: the next cycle is a deed again.
        assert!(work(
            &mut leased,
            &registries,
            ClaimKind::Concession,
            5,
            2.0 * HOUR
        ));
    }

    #[test]
    fn a_reserve_always_writes_a_deed_even_under_a_lease() {
        let registries = registries();
        let mut books = (Deeds::default(), CommonsTaken::default(), Leases::default());
        books.2.until.insert(77, 100.0 * HOUR);
        assert!(work(&mut books, &registries, ClaimKind::Reserve, 2, 1.0));
        assert_eq!(books.0.by[&77][0].kind, DeedKind::WorkedReserve);
    }

    #[test]
    fn a_commons_counts_only_past_its_share_and_anew_the_next_day() {
        let registries = registries();
        let rules = Rules::of(&registries.0).unwrap();
        let share = rules.claims.commons_share;
        let day = rules.claims.day_h * HOUR;
        let mut books = (Deeds::default(), CommonsTaken::default(), Leases::default());
        assert!(!work(
            &mut books,
            &registries,
            ClaimKind::Commons,
            share - 10,
            1.0
        ));
        assert!(books.0.by.is_empty());
        // Twenty more: ten of them past the share.
        assert!(work(&mut books, &registries, ClaimKind::Commons, 20, 2.0));
        assert_eq!(books.0.by[&77][0].kind, DeedKind::TookPastShare);
        assert_eq!(books.0.by[&77][0].amount, 10);
        // And every unit after that counts whole.
        assert!(work(&mut books, &registries, ClaimKind::Commons, 4, 3.0));
        assert_eq!(books.0.by[&77][0].amount, 14);
        // The next played day starts the tally again.
        let mut next = books.clone();
        assert!(!work(
            &mut next,
            &registries,
            ClaimKind::Commons,
            share,
            day + 1.0
        ));
        assert_eq!(next.0.by[&77][0].amount, 14);
        assert!(work(
            &mut next,
            &registries,
            ClaimKind::Commons,
            1,
            day + 2.0
        ));
    }

    #[test]
    fn planting_on_a_concession_without_a_lease_or_a_reserve_is_a_deed() {
        let registries = registries();
        let rules = Rules::of(&registries.0).unwrap();
        let leases = Leases::default();
        let mut deeds = Deeds::default();
        assert!(!planted(
            &mut deeds,
            &leases,
            rules,
            claim(ClaimKind::Commons),
            0.0
        ));
        assert!(planted(
            &mut deeds,
            &leases,
            rules,
            claim(ClaimKind::Concession),
            0.0
        ));
        assert!(planted(
            &mut deeds,
            &leases,
            rules,
            claim(ClaimKind::Reserve),
            0.0
        ));
        let mut running = Leases::default();
        running.until.insert(77, HOUR);
        assert!(!planted(
            &mut deeds,
            &running,
            rules,
            claim(ClaimKind::Concession),
            0.0
        ));
    }

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

    #[test]
    fn pelagic_starts_welcoming_and_a_lease_follows_the_stance() {
        let registries = registries();
        let (_, faction, _) = pelagic();
        let rules = registries.0.factions.deeds().unwrap();
        let mut deeds = Deeds::default();
        assert_eq!(
            stance_of(&registries.0, &deeds, faction, 0.0),
            Stance::Welcoming
        );
        let terms = Terms::of(Stance::Welcoming, rules);
        let lease = registries.0.factions.claims().unwrap().lease;
        let price = Terms::scale(lease.price, terms.sells_pm);
        // Short of credits.
        assert_eq!(
            check_lease(&registries.0, true, terms, &Purse(price - 1), faction),
            Err(LeaseRefusal::Short {
                needs: price,
                have: price - 1
            })
        );
        let quote = check_lease(&registries.0, true, terms, &Purse(price), faction).unwrap();
        assert_eq!(quote.price, price);
        assert!((quote.hours - lease.hours).abs() < 1e-9);
        assert_eq!(
            check_lease(&registries.0, false, terms, &Purse(price), faction),
            Err(LeaseRefusal::NotAtBoard)
        );
        // Crossed enough, Pelagic is hostile and sells no lease.
        for _ in 0..4 {
            deeds.write(faction.id, DeedKind::Planted, 1, 0.0, rules);
        }
        assert_eq!(
            stance_of(&registries.0, &deeds, faction, 0.0),
            Stance::Hostile
        );
        let terms = Terms::of(Stance::Hostile, rules);
        assert_eq!(
            check_lease(&registries.0, true, terms, &Purse(10_000), faction),
            Err(LeaseRefusal::Hostile)
        );
    }
}
