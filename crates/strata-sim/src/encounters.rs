//! Encounters in the world (ticket 0012, step 2): the people of a place
//! stand up when a member comes near, fight as a group of their own, fold
//! away when the member has left and nobody is fighting, and a cleared
//! place stays quiet for a stretch of played time before it is taken again.
//!
//! Truth and rules live here and the game only turns them into bodies: the
//! pure [`plan`] says who stands where, [`refresh`] says which camps are
//! standing, [`Fights`] says which camp's people are engaged and how many
//! are left, [`Cleared`] and [`PlayedClock`] say what is quiet. Nothing
//! here samples ground: a slot is a place on the pad and the game finds the
//! ground under it, as it does for the staging fight.
//!
//! The tier and the roster are data (`assets/combat/tiers.ron`,
//! `assets/encounters/`); the geometry constants below are how a ring is
//! laid on a pad, not a balance dial.

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Plugin, Update};
use bevy::ecs::prelude::*;
use bevy::state::condition::in_state;
use serde::{Deserialize, Serialize};
use strata_core::Seed;
use strata_registry::{
    ClaimKind, Crew, EncounterDef, EncounterSet, FoeKind, GameRegistries, Person as Rostered,
};
use strata_worldgen::deposits::{Deposit, DepositId};
use strata_worldgen::factions::Claim;
use strata_worldgen::sites::{ProceduralSite, SiteId, SitePlacement};

use crate::combat::{Action, CombatImpact, Combatant};
use crate::combat_ai::OpponentKind;
use crate::combat_tables::current as combat_tables;
use crate::combat_tiers::tier_at;
use crate::deeds::{DeedKind, Deeds, Leases, Ledger, Stance, stance_of};
use crate::deposits::{NearDeposits, Survey};
use crate::factions::{FactionId, Factions};
use crate::factory::nominal_ticks;
use crate::loot::{Caches, lot_candidates, nearest_of, roll, seed as loot_seed};
use crate::progression::{CharacterSheet, Interaction, InteractionKind};
use crate::sites::{NearSite, NearSites};
use crate::{
    AppState, LocalPlayer, Registries, SIM_TICK_HZ, SimClock, SimSet, SimTick, WorldPosition,
    WorldSeed,
};

/// The team of a faction's people while the member has not crossed them:
/// neither the member's nor the outlaws', so nobody counts them an enemy
/// and a hit still lands on them, since physics only asks for another team.
pub const NEUTRAL_TEAM: u8 = 2;

/// Whether a combatant of team `a` and one of team `b` are enemies: other
/// teams, and neither the neutral one. The one rule a bike's call, an
/// ability's area and the scripts ask; a bolt or a cut still lands on any
/// other team.
#[must_use]
pub const fn enemies(a: u8, b: u8) -> bool {
    a != b && a != NEUTRAL_TEAM && b != NEUTRAL_TEAM
}

/// Which encounter a person belongs to.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EncounterId {
    /// The training fight west of Port Serein, with F5: never cleared, never
    /// folded.
    Staging,
    /// A camp, by its site's [`SiteId::key`].
    Camp(u64),
    /// A claim's post, by [`post_key`] of its deposit.
    Post(u64),
}

/// The key of the post at `deposit`, in the space [`Cleared`] shares with
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

/// Whose people an opponent is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allegiance {
    /// The training fight's.
    Staging,
    /// An outlaw band's, or none where no faction holds the camp.
    Outlaw(Option<FactionId>),
    /// A faction's guard or warden at a claim: neutral until its post is
    /// crossed.
    Faction(FactionId),
}

/// The team a person of `allegiance` fights on: the one rule of who is an
/// enemy. A faction's people are neutral until their post is `crossed`;
/// outlaws and the staging fight are always the member's enemies.
#[must_use]
pub const fn hostility(allegiance: Allegiance, crossed: bool) -> u8 {
    match allegiance {
        Allegiance::Staging | Allegiance::Outlaw(_) => 1,
        Allegiance::Faction(_) if crossed => 1,
        Allegiance::Faction(_) => NEUTRAL_TEAM,
    }
}

/// Played seconds since the household began: the clock a cleared camp's
/// quiet is measured on. Advanced in the tick by nominal time, so a faster
/// stepped sim does not shorten an hour. The game saves it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayedClock {
    /// Played seconds.
    pub seconds: f64,
}

/// The camps whose people are all down, by camp key, and the played second
/// each was cleared. The game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cleared {
    /// Played second of clearing, by [`SiteId::key`].
    pub at: BTreeMap<u64, f64>,
}

impl Cleared {
    /// Whether the camp `key` is quiet at played second `now`, given that
    /// one stays quiet `returns_h` played hours.
    #[must_use]
    pub fn is_quiet(&self, key: u64, now: f64, returns_h: f32) -> bool {
        self.at
            .get(&key)
            .is_some_and(|cleared| now - cleared < f64::from(returns_h) * 3600.0)
    }

    /// The camp `key` was cleared at `now`.
    pub fn clear(&mut self, key: u64, now: f64) {
        self.at.insert(key, now);
    }
}

/// Whether the site `key` of kind `kind` (`strata:wild_camp`) is cleared at
/// played second `now`: it has an encounter and its quiet has not run out.
/// The card and the map ask this, so neither words a second rule.
#[must_use]
pub fn is_cleared(
    encounters: &EncounterSet,
    cleared: &Cleared,
    now: f64,
    kind: &str,
    key: u64,
) -> bool {
    encounters
        .iter()
        .find(|def| def.at.site().is_some_and(|at| at.to_string() == kind))
        .is_some_and(|def| cleared.is_quiet(key, now, def.returns_h))
}

impl From<FoeKind> for OpponentKind {
    fn from(kind: FoeKind) -> Self {
        match kind {
            FoeKind::Sentry => Self::Sentry,
            FoeKind::Rusher => Self::Rusher,
            FoeKind::Marksman => Self::Marksman,
            FoeKind::Duelist => Self::Duelist,
            FoeKind::Bulwark => Self::Bulwark,
        }
    }
}

/// One person of a camp, before there is a body: what it is and where on
/// the pad it stands, world `(x, z)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// Its kind, which picks its brain and moveset.
    pub kind: OpponentKind,
    /// Where it stands.
    pub at: [f64; 2],
}

/// A hash of a camp's key and a person's index, the same on every platform.
fn mix(key: u64, index: usize) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in key
        .to_le_bytes()
        .into_iter()
        .chain((index as u64).to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// How far `p` is from the quad, metres, nought inside it.
fn distance_to_quad(p: [f64; 2], quad: &[[f64; 2]; 4]) -> f64 {
    let mut inside = false;
    let mut nearest = f64::MAX;
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let along = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / (dx * dx + dz * dz).max(1e-12))
            .clamp(0.0, 1.0);
        nearest = nearest.min((p[0] - a[0] - along * dx).hypot(p[1] - a[1] - along * dz));
    }
    if inside { 0.0 } else { nearest }
}

/// The people of a camp: the encounter's roster for `tier`, on a ring round
/// the pad's centre that starts at an angle drawn from the camp's key, each
/// moved off a building of the group when it stands on one. Pure: a server
/// and a client with the same world stand the same people in the same
/// places.
#[must_use]
pub fn plan(
    site: &ProceduralSite,
    placement: Option<&SitePlacement>,
    tier: u8,
    def: &EncounterDef,
) -> Vec<Slot> {
    let walls: Vec<&[[f64; 2]; 4]> = placement
        .map(|placement| placement.all().map(|b| &b.footprint).collect())
        .unwrap_or_default();
    ring_slots(
        roster_kinds(def, tier),
        site.id.key(),
        site.centre,
        site.radius_m,
        def,
        &walls,
    )
}

/// The people of a claim's post: the roster for `tier` on a ring of the
/// file's `post_m` metres round the deposit's centre, started at an angle
/// drawn from the deposit's [`post_key`]. A claim has no buildings, so the
/// first share of `ring` always stands; the game finds the ground under
/// each. Empty for a file with no `post_m`.
#[must_use]
pub fn plan_post(deposit: &Deposit, tier: u8, def: &EncounterDef) -> Vec<Slot> {
    let Some(radius) = def.post_m else {
        return Vec::new();
    };
    ring_slots(
        roster_kinds(def, tier),
        post_key(deposit.id),
        deposit.at,
        f64::from(radius),
        def,
        &[],
    )
}

fn roster_kinds(def: &EncounterDef, tier: u8) -> Vec<OpponentKind> {
    def.roster(tier)
        .iter()
        .flat_map(|line: &Rostered| {
            std::iter::repeat_n(OpponentKind::from(line.kind), line.count.into())
        })
        .collect()
}

/// `kinds` on a ring of `radius_m` round `centre`, each moved off a wall
/// when it stands on one.
fn ring_slots(
    kinds: Vec<OpponentKind>,
    key: u64,
    centre: [f64; 2],
    radius_m: f64,
    def: &EncounterDef,
    walls: &[&[[f64; 2]; 4]],
) -> Vec<Slot> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "a hash's top bits as a fraction"
    )]
    let start = (mix(key, usize::MAX) >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
    #[allow(clippy::cast_precision_loss, reason = "a roster of a few")]
    let count = kinds.len().max(1) as f64;
    let clear = |p: [f64; 2]| {
        walls
            .iter()
            .all(|quad| distance_to_quad(p, quad) >= f64::from(def.clear_m))
    };
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            #[allow(clippy::cast_precision_loss, reason = "a roster of a few")]
            let angle = start + std::f64::consts::TAU * i as f64 / count;
            let spot = |share: f64, nudge: f64| {
                let (sin, cos) = (angle + nudge).sin_cos();
                [
                    centre[0] + cos * share * radius_m,
                    centre[1] + sin * share * radius_m,
                ]
            };
            let at = def
                .ring
                .iter()
                .flat_map(|share| def.nudges.iter().map(move |nudge| (*share, *nudge)))
                .map(|(share, nudge)| spot(f64::from(share), f64::from(nudge)))
                .find(|p| clear(*p))
                .unwrap_or_else(|| spot(f64::from(def.ring[0]), 0.0));
            Slot { kind, at }
        })
        .collect()
}

/// A camp whose people are stood up.
#[derive(Debug, Clone, PartialEq)]
pub struct NearCamp {
    /// Its encounter.
    pub id: EncounterId,
    /// Its site.
    pub site: SiteId,
    /// The site's name.
    pub name: String,
    /// The pad's centre, world `(x, z)`.
    pub centre: [f64; 2],
    /// Its tier when it stood up: a camp keeps it until it is taken again.
    pub tier: u8,
    /// The band that holds it, where a faction does.
    pub band: Option<FactionId>,
    /// Who stands where.
    pub slots: Vec<Slot>,
    /// Its people were all down: [`mark_cleared`] set it, and a camp whose
    /// quiet has run out is stood down so the next pass stands it again.
    pub cleared: bool,
    /// A living person of this camp has been seen, so a corpse left by an
    /// earlier standing of the same camp, or people not yet stood for want
    /// of ground, never clear it.
    pub seen_alive: bool,
}

/// A claim's post whose people are stood up: guards at a concession,
/// wardens at a reserve.
#[derive(Debug, Clone, PartialEq)]
pub struct NearPost {
    /// Its encounter.
    pub id: EncounterId,
    /// The claimed deposit.
    pub deposit: DepositId,
    /// The deposit's place name, `Calder flats`.
    pub name: String,
    /// The deposit's centre, world `(x, z)`.
    pub centre: [f64; 2],
    /// How far the deposit's area reaches, metres: working inside it is
    /// working the claim.
    pub radius_m: f32,
    /// Its tier when it stood up.
    pub tier: u8,
    /// Whose claim it is.
    pub faction: FactionId,
    /// How the faction keeps it: a concession's guards or a reserve's
    /// wardens.
    pub kind: ClaimKind,
    /// Who stands where.
    pub slots: Vec<Slot>,
    /// Its people were all down, as for a camp.
    pub cleared: bool,
    /// A living person of this post has been seen, as for a camp.
    pub seen_alive: bool,
}

/// The camps and claim posts standing now: camps in site order, posts
/// nearest first.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct NearEncounters {
    /// The standing camps.
    pub camps: Vec<NearCamp>,
    /// The standing posts, at most [`MAX_POSTS`].
    pub posts: Vec<NearPost>,
    /// A tier every camp and post stands up at whatever its distance, for a
    /// still.
    pub forced_tier: Option<u8>,
}

/// The most posts stood up at once: the nearest claims of the member's
/// surroundings, a bound on how many bodies a walk through a rich district
/// asks the game for, not a balance dial.
pub const MAX_POSTS: usize = 2;

/// Which camps changed in one [`refresh`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changes {
    /// Camps stood up, whose people the game should make.
    pub stood: Vec<EncounterId>,
    /// Camps stood down, whose people and bolts the game should remove.
    pub folded: Vec<EncounterId>,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Stands up and folds away the camps of `sites` for a member at `player`.
///
/// A camp within its encounter's `stand_m` and not quiet stands up; one
/// beyond `fold_m` folds when its people are not fighting; one whose quiet
/// has run out is stood down and, if the member is still within `stand_m`,
/// stood up again fresh, at its tier of now. Expired quiet is forgotten.
#[allow(
    clippy::too_many_arguments,
    reason = "the world's four readings and the three ledgers"
)]
pub fn refresh(
    near: &mut NearEncounters,
    sites: &[NearSite],
    encounters: &EncounterSet,
    player: [f64; 2],
    now: f64,
    cleared: &mut Cleared,
    fights: &Fights,
    band_of: impl Fn(&SiteId) -> Option<FactionId>,
) -> Changes {
    let def_of = |near: &NearSite| {
        encounters
            .iter()
            .find(|e| e.at.site().is_some_and(|at| at.to_string() == near.kind))
    };
    let mut changes = Changes::default();
    near.camps.retain(|camp| {
        let Some(site) = sites.iter().find(|s| s.site.id == camp.site) else {
            return true;
        };
        let Some(def) = def_of(site) else {
            return true;
        };
        let returned = camp.cleared && !cleared.is_quiet(camp.site.key(), now, def.returns_h);
        let gone =
            distance(camp.centre, player) > f64::from(def.fold_m) && !fights.fighting(camp.id);
        if returned || gone {
            changes.folded.push(camp.id);
            return false;
        }
        true
    });
    for site in sites {
        let Some(def) = def_of(site) else {
            continue;
        };
        let key = site.site.id.key();
        let id = EncounterId::Camp(key);
        if distance(site.site.centre, player) > f64::from(def.stand_m)
            || cleared.is_quiet(key, now, def.returns_h)
            || near.camps.iter().any(|camp| camp.id == id)
        {
            continue;
        }
        let tier = near
            .forced_tier
            .unwrap_or_else(|| tier_at(site.site.centre[0], site.site.centre[1]));
        near.camps.push(NearCamp {
            id,
            site: site.site.id,
            name: site.site.name.clone(),
            centre: site.site.centre,
            tier,
            // Unaligned crews belong to nobody, whoever holds the ground.
            band: match def.crew {
                Crew::Holder => band_of(&site.site.id),
                Crew::Unaligned => None,
            },
            slots: plan(&site.site, site.placement.as_ref(), tier, def),
            cleared: false,
            seen_alive: false,
        });
        changes.stood.push(id);
    }
    near.camps.sort_by_key(|camp| camp.site);
    // Expired quiet is forgotten; a standing camp that was cleared reads a
    // missing record as run out, which is what sent it back above.
    let longest = encounters
        .iter()
        .map(|def| def.returns_h)
        .fold(0.0, f32::max);
    cleared
        .at
        .retain(|_, at| now - *at < f64::from(longest) * 3600.0);
    changes
}

/// Stands up and folds away the posts of the claims in `claimed` for a
/// member at `player`, as [`refresh`] does for camps.
///
/// A claim of a kind with an encounter file, within its `stand_m` and not
/// quiet, stands up at its tier, the nearest first and never past
/// [`MAX_POSTS`] standing, a farther post nobody has crossed, fights or is
/// warned by giving way to a nearer claim; a post beyond `fold_m` folds when nobody fights
/// it, and one whose quiet has run out is stood down. `claimed` is every
/// deposit near the member that is a claim.
#[allow(
    clippy::too_many_arguments,
    reason = "the world's readings and the two ledgers, as for refresh"
)]
pub fn refresh_posts(
    near: &mut NearEncounters,
    claimed: &[(Deposit, Claim)],
    encounters: &EncounterSet,
    player: [f64; 2],
    now: f64,
    cleared: &Cleared,
    fights: &Fights,
) -> Changes {
    let mut changes = Changes::default();
    near.posts.retain(|post| {
        let Some(def) = encounters.at_claim(post.kind) else {
            return true;
        };
        let returned =
            post.cleared && !cleared.is_quiet(post_key(post.deposit), now, def.returns_h);
        let gone =
            distance(post.centre, player) > f64::from(def.fold_m) && !fights.fighting(post.id);
        if returned || gone {
            changes.folded.push(post.id);
            return false;
        }
        true
    });
    let mut wanted: Vec<(f64, &Deposit, Claim, &EncounterDef)> = claimed
        .iter()
        .filter_map(|(deposit, claim)| {
            let def = encounters.at_claim(claim.kind)?;
            let away = distance(deposit.at, player);
            (away <= f64::from(def.stand_m)).then_some((away, deposit, *claim, def))
        })
        .collect();
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    for (away, deposit, claim, def) in wanted {
        let key = post_key(deposit.id);
        let id = EncounterId::Post(key);
        if cleared.is_quiet(key, now, def.returns_h) || near.posts.iter().any(|p| p.id == id) {
            continue;
        }
        if near.posts.len() >= MAX_POSTS {
            // The standing posts are the nearest: a farther one nobody has
            // crossed or is fighting gives way, so a claim the member walks
            // onto always has its post.
            let farthest = near
                .posts
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    !fights.crossed(p.id)
                        && !fights.fighting(p.id)
                        && fights.warning().is_none_or(|w| w.post != p.id)
                })
                .map(|(at, p)| (at, distance(p.centre, player)))
                .filter(|(_, d)| *d > away)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            let Some((at, _)) = farthest else {
                break;
            };
            changes.folded.push(near.posts.remove(at).id);
        }
        let tier = near
            .forced_tier
            .unwrap_or_else(|| tier_at(deposit.at[0], deposit.at[1]));
        near.posts.push(NearPost {
            id,
            deposit: deposit.id,
            name: deposit.place.clone(),
            centre: deposit.at,
            radius_m: deposit.radius_m,
            tier,
            faction: claim.holder,
            kind: claim.kind,
            slots: plan_post(deposit, tier, def),
            cleared: false,
            seen_alive: false,
        });
        changes.stood.push(id);
    }
    near.posts.sort_by(|a, b| {
        distance(a.centre, player)
            .total_cmp(&distance(b.centre, player))
            .then_with(|| a.deposit.cmp(&b.deposit))
    });
    changes
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

/// `SimTick`: advance the played clock by nominal time.
pub fn advance_clock(
    sim: Option<Res<SimClock>>,
    mut clock: ResMut<PlayedClock>,
    mut phase: Local<u32>,
) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "steps a second, a few tens"
    )]
    let stepped = sim
        .map_or(SIM_TICK_HZ, |sim| 1. / sim.0.step)
        .round()
        .max(1.) as u32;
    let ticks = nominal_ticks(&mut phase, stepped);
    clock.seconds += f64::from(ticks) / SIM_TICK_HZ;
}

/// `Update`: a standing camp whose people are all down is cleared at the
/// played second now. The staging fight is never recorded.
#[allow(
    clippy::too_many_arguments,
    reason = "the people, the clock, the ledgers, the world for a cache's draw and the deeds"
)]
pub fn mark_cleared(
    people: Query<(&EncounterId, &Combatant)>,
    clock: Res<PlayedClock>,
    mut near: ResMut<NearEncounters>,
    mut cleared: ResMut<Cleared>,
    registries: Option<Res<Registries>>,
    seed: Option<Res<WorldSeed>>,
    (survey, deposits): (Option<Res<Survey>>, Option<Res<NearDeposits>>),
    mut caches: ResMut<Caches>,
    mut ledger: Ledger,
) {
    if near.camps.iter().all(|camp| camp.cleared) && near.posts.iter().all(|post| post.cleared) {
        return;
    }
    let mut left: BTreeMap<EncounterId, (usize, usize)> = BTreeMap::new();
    for (id, actor) in &people {
        let entry = left.entry(*id).or_default();
        entry.0 += 1;
        entry.1 += usize::from(actor.action != Action::Dead);
    }
    for camp in near.camps.iter_mut().filter(|camp| !camp.cleared) {
        let Some((total, alive)) = left.get(&camp.id).copied() else {
            continue;
        };
        camp.seen_alive |= alive > 0;
        if camp.seen_alive && alive == 0 && total >= camp.slots.len() {
            camp.cleared = true;
            cleared.clear(camp.site.key(), clock.seconds);
            if let Some(registries) = &registries {
                ledger.cleared(&registries.0, camp.centre);
                leave_cache(
                    &registries.0,
                    camp.site.kind,
                    (camp.site.key(), camp.centre, camp.tier),
                    seed.as_deref().map_or(Seed(0), |seed| seed.0),
                    (survey.as_deref(), deposits.as_deref()),
                    clock.seconds,
                    &mut caches,
                );
            }
        }
    }
    // A post is quiet for its hours like a camp, but it was never an
    // outlaw camp: no deed for the factions about it.
    for post in near.posts.iter_mut().filter(|post| !post.cleared) {
        let Some((total, alive)) = left.get(&post.id).copied() else {
            continue;
        };
        post.seen_alive |= alive > 0;
        if post.seen_alive && alive == 0 && total >= post.slots.len() {
            post.cleared = true;
            cleared.clear(post_key(post.deposit), clock.seconds);
        }
    }
}

/// The cache a camp of site kind `kind`, at `key` and `centre` and of `tier`,
/// leaves when cleared at played second `now`: its encounter's table for the
/// tier, drawn from the seed of the camp and the clearing. A camp whose
/// encounter has no table leaves none. Public for a still that starts with
/// a camp already cleared.
#[allow(
    clippy::too_many_arguments,
    reason = "the camp's four facts, the world's seed and survey, the second and the ledger it writes"
)]
pub fn leave_cache(
    registries: &GameRegistries,
    kind: strata_registry::SiteKindId,
    (key, centre, tier): (u64, [f64; 2], u8),
    world: Seed,
    (survey, deposits): (Option<&Survey>, Option<&NearDeposits>),
    now: f64,
    caches: &mut Caches,
) {
    let Some(table) = registries
        .encounters
        .iter()
        .find(|def| def.at.site().and_then(|name| registries.site_kind_id(name)) == Some(kind))
        .and_then(|def| registries.loot.of(&def.id))
    else {
        return;
    };
    let candidates = survey.map_or_else(Vec::new, |survey| {
        lot_candidates(&survey.0, deposits, centre, table.lots_within_m)
    });
    let near = nearest_of(&candidates, &registries.resources, centre);
    let drawn = roll(table, tier, loot_seed(world, key, now), near, registries);
    caches.by.insert(key, drawn);
}

/// Folds apply before stands: a quiet post that returns while the member is
/// still by folds and stands again in one pass, and must end registered.
fn apply_post_changes(fights: &mut Fights, posts: &Changes) {
    for id in &posts.folded {
        fights.forget_post(*id);
    }
    for id in &posts.stood {
        fights.stand_post(*id);
    }
}

/// `Update`: [`refresh`] and [`refresh_posts`] for the local player. A
/// camp's candidates come from [`NearSites`], which is made off the frame;
/// what is decided here is a distance per camp. A post's are the deposits of
/// [`NearDeposits`] within reach of the member, whose claim is asked once
/// and kept.
#[allow(
    clippy::too_many_arguments,
    reason = "the readings and ledgers refresh takes"
)]
pub fn update_near_encounters(
    registries: Option<Res<Registries>>,
    sites: Option<Res<NearSites>>,
    deposits: Option<Res<NearDeposits>>,
    factions: Option<Res<Factions>>,
    clock: Res<PlayedClock>,
    mut fights: ResMut<Fights>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    mut near: ResMut<NearEncounters>,
    mut cleared: ResMut<Cleared>,
    mut caches: ResMut<Caches>,
    mut changes: MessageWriter<EncounterChanged>,
    mut claims: Local<BTreeMap<DepositId, Option<Claim>>>,
) {
    let (Some(registries), Some(sites), Ok(position)) = (registries, sites, player.single()) else {
        return;
    };
    let band = |id: &SiteId| {
        factions
            .as_ref()
            .and_then(|factions| factions.0.holder_of(id))
    };
    let here = [position.0.x(), position.0.z()];
    // Quiet is forgotten every pass but changes rarely: the ledger is marked
    // only when a record went, so what watches it is not woken every frame.
    let before = cleared.at.len();
    let mut made = refresh(
        &mut near,
        &sites.sites,
        &registries.0.encounters,
        here,
        clock.seconds,
        cleared.bypass_change_detection(),
        &fights,
        band,
    );
    if cleared.at.len() != before {
        cleared.set_changed();
    }
    // A camp that returned takes what it left with it.
    let held = caches.by.len();
    caches
        .bypass_change_detection()
        .prune(&cleared, clock.seconds, &registries.0.encounters);
    if caches.by.len() != held {
        caches.set_changed();
    }
    if let (Some(deposits), Some(factions)) = (deposits, factions.as_ref()) {
        let reach = registries
            .0
            .encounters
            .iter()
            .filter(|def| def.at.claim().is_some())
            .map(|def| f64::from(def.stand_m))
            .fold(0.0, f64::max);
        let claimed: Vec<(Deposit, Claim)> = deposits
            .deposits
            .iter()
            .filter(|deposit| distance(deposit.at, here) <= reach)
            .filter_map(|deposit| {
                let claim = *claims
                    .entry(deposit.id)
                    .or_insert_with(|| factions.0.claim_of(deposit));
                Some((deposit.clone(), claim?))
            })
            .collect();
        let posts = refresh_posts(
            &mut near,
            &claimed,
            &registries.0.encounters,
            here,
            clock.seconds,
            &cleared,
            &fights,
        );
        apply_post_changes(&mut fights, &posts);
        made.stood.extend(posts.stood);
        made.folded.extend(posts.folded);
    }
    if !made.stood.is_empty() || !made.folded.is_empty() {
        changes.write(EncounterChanged(made));
    }
}

/// Why a post turned on the member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnedBy {
    /// One of its people was struck.
    Struck,
    /// A warning ran out.
    Warning,
    /// Its faction is hostile and the member came within reach of its
    /// people.
    Sight,
}

/// A post turned hostile: the game words the toast from the faction.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostTurned {
    /// The post.
    pub post: EncounterId,
    /// Whose it is.
    pub faction: FactionId,
    /// Why.
    pub by: TurnedBy,
}

/// What [`watch`] reads of the world for one pass.
pub struct Watching<'a> {
    /// The standing posts.
    pub posts: &'a [NearPost],
    /// The member, world `(x, z)`.
    pub player: [f64; 2],
    /// The played second.
    pub now: f64,
    /// How near one of a post's people the member must come to be seen.
    pub engage_m: f64,
    /// Where each person of a post stands, world `(x, z)`.
    pub people: &'a [(EncounterId, [f64; 2])],
}

/// One pass over the warning and the sight rule: a warning lapses when its
/// post is gone, the member is beyond the deposit's area or a lease of its
/// faction runs, and turns the post when it runs out; a post whose faction
/// is hostile (`hostile`) turns when the member is within `engage_m` of one
/// of its people. Posts turned, in order.
pub fn watch(
    fights: &mut Fights,
    world: &Watching<'_>,
    lease_live: impl Fn(FactionId) -> bool,
    hostile: impl Fn(FactionId) -> bool,
) -> Vec<(EncounterId, FactionId, TurnedBy)> {
    let mut turned = Vec::new();
    if let Some(warning) = fights.warning() {
        let post = world.posts.iter().find(|post| post.id == warning.post);
        let away =
            post.is_none_or(|post| distance(post.centre, world.player) > f64::from(post.radius_m));
        if away || lease_live(warning.faction) || !fights.neutral(warning.post) {
            fights.lapse_warning();
        } else if world.now >= warning.until {
            fights.cross(warning.post);
            turned.push((warning.post, warning.faction, TurnedBy::Warning));
        }
    }
    for post in world.posts {
        if !fights.neutral(post.id) || !hostile(post.faction) {
            continue;
        }
        let seen = world
            .people
            .iter()
            .any(|(id, at)| *id == post.id && distance(*at, world.player) < world.engage_m);
        if seen && fights.cross(post.id) {
            turned.push((post.id, post.faction, TurnedBy::Sight));
        }
    }
    turned
}

/// `Update`: [`watch`] for the local player.
#[allow(
    clippy::too_many_arguments,
    reason = "the readings the pass takes and the message it writes"
)]
pub fn watch_posts(
    registries: Option<Res<Registries>>,
    factions: Option<Res<Factions>>,
    deeds: Option<Res<Deeds>>,
    leases: Option<Res<Leases>>,
    clock: Res<PlayedClock>,
    near: Res<NearEncounters>,
    mut fights: ResMut<Fights>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    people: Query<(&EncounterId, &WorldPosition)>,
    mut turned: MessageWriter<PostTurned>,
) {
    let (Some(registries), Some(factions), Ok(position)) = (registries, factions, player.single())
    else {
        return;
    };
    if near.posts.is_empty() {
        return;
    }
    let standing: Vec<(EncounterId, [f64; 2])> = people
        .iter()
        .filter(|(id, _)| matches!(id, EncounterId::Post(_)))
        .map(|(id, at)| (*id, [at.0.x(), at.0.z()]))
        .collect();
    let none = Leases::default();
    let leases = leases.as_deref().unwrap_or(&none);
    let books = deeds.as_deref().cloned().unwrap_or_default();
    let now = clock.seconds;
    let world = Watching {
        posts: &near.posts,
        player: [position.0.x(), position.0.z()],
        now,
        engage_m: f64::from(combat_tables().opponents.engage_distance),
        people: &standing,
    };
    // Reading the guards' stance costs a faction lookup, so only a post that
    // is neutral asks it.
    let hostile = |id: FactionId| {
        factions.0.faction(id).is_some_and(|faction| {
            stance_of(&registries.0, &books, &faction, now) == Stance::Hostile
        })
    };
    let made = watch(&mut fights, &world, |id| leases.live(id, now), hostile);
    for (post, faction, by) in made {
        turned.write(PostTurned { post, faction, by });
    }
}

/// `Update`: every uncrossed guard of a concession carries an
/// [`Interaction`] that sells its faction's lease within `guard_reach_m`;
/// one that turns, or a warden, carries none. The game stands the bodies;
/// the sim says what they offer.
pub fn tend_guards(
    registries: Option<Res<Registries>>,
    near: Res<NearEncounters>,
    fights: Res<Fights>,
    people: Query<(Entity, &EncounterId, &Allegiance, Has<Interaction>)>,
    mut commands: Commands,
) {
    let Some(reach) = registries
        .as_ref()
        .and_then(|registries| registries.0.factions.claims())
        .map(|claims| claims.guard_reach_m)
    else {
        return;
    };
    for (entity, id, allegiance, has) in &people {
        let Allegiance::Faction(faction) = allegiance else {
            continue;
        };
        let sells = near.posts.iter().any(|post| {
            post.id == *id && post.kind == ClaimKind::Concession && !fights.crossed(post.id)
        });
        if sells && !has {
            commands.entity(entity).insert(Interaction {
                kind: InteractionKind::Guard { faction: *faction },
                radius: reach,
            });
        } else if !sells && has {
            commands.entity(entity).remove::<Interaction>();
        }
    }
}

/// A person of a post the member has struck: the deed of hurting them is
/// written once.
#[derive(Component, Debug, Clone, Copy)]
pub struct Struck;

/// `SimTick`, `SimSet::Observe`: a household member's blow on a person of a
/// faction's post turns the post at once and writes a hurt deed once per
/// person and a killed deed on a lethal blow. Outlaws and the staging fight
/// write none: they are nobody's people.
pub fn strike_posts(
    registries: Option<Res<Registries>>,
    mut impacts: MessageReader<CombatImpact>,
    people: Query<(&EncounterId, &Allegiance, Has<Struck>)>,
    members: Query<(), (With<CharacterSheet>, Without<Allegiance>)>,
    mut commands: Commands,
    mut ledger: Ledger,
    mut turned: MessageWriter<PostTurned>,
) {
    let Some(registries) = registries else {
        return;
    };
    let mut hurt_now: BTreeSet<Entity> = BTreeSet::new();
    for impact in impacts.read() {
        let Some(victim) = impact.victim else {
            continue;
        };
        if impact.dealt <= 0. || !members.contains(impact.attacker) {
            continue;
        }
        let Ok((id, Allegiance::Faction(faction), struck)) = people.get(victim) else {
            continue;
        };
        if !struck && hurt_now.insert(victim) {
            ledger.deed(&registries.0, *faction, DeedKind::Hurt);
            commands.entity(victim).insert(Struck);
        }
        if impact.lethal {
            ledger.deed(&registries.0, *faction, DeedKind::Killed);
        }
        if ledger.cross(*id) {
            turned.write(PostTurned {
                post: *id,
                faction: *faction,
                by: TurnedBy::Struck,
            });
        }
    }
}

/// A camp stood up or folded, for the game to make or remove its people.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct EncounterChanged(pub Changes);

/// Registers the encounters' state and its systems. Added by `SimPlugin`; a
/// server and a harness have no encounter data worth a camp and touch
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct EncountersPlugin;

impl Plugin for EncountersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayedClock>()
            .init_resource::<Cleared>()
            .init_resource::<NearEncounters>()
            .init_resource::<Fights>()
            .add_message::<EncounterChanged>()
            .add_message::<PostTurned>()
            .add_systems(SimTick, advance_clock.in_set(SimSet::Input))
            .add_systems(SimTick, strike_posts.in_set(SimSet::Observe))
            .add_systems(
                Update,
                (
                    update_near_encounters,
                    mark_cleared,
                    watch_posts,
                    tend_guards,
                )
                    .chain()
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use std::sync::Arc;

    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;
    use strata_registry::SiteKindId;
    use strata_worldgen::sites::PlacedSiteBuilding;

    use super::*;
    use crate::deeds::CommonsTaken;

    fn encounters() -> EncounterSet {
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the assets")
            .encounters
    }

    /// The outlaw camp's file: the posts' files sort before it by id.
    fn camp_def() -> EncounterDef {
        let kind = strata_registry::Namespaced::parse("strata:wild_camp").unwrap();
        encounters().at(&kind).cloned().expect("the camp")
    }

    fn site(node: i32, centre: [f64; 2]) -> ProceduralSite {
        ProceduralSite {
            id: SiteId {
                kind: SiteKindId(6),
                mass: [0, 0],
                node: [node, 0],
            },
            name: format!("Camp {node}"),
            centre,
            level_m: 10.0,
            radius_m: 13.0,
            blend_m: 24.0,
            crown: 0.02,
            facing: [1.0, 0.0],
            arrival: [centre[0] + 13.0, centre[1]],
            by: None,
            score: 1.0,
            cut_m: 0.0,
        }
    }

    fn near(site: &ProceduralSite) -> NearSite {
        NearSite {
            site: site.clone(),
            kind: "strata:wild_camp".to_owned(),
            kind_name: "Outlaw camp".to_owned(),
            placement: None,
        }
    }

    /// A camp far from the city's centre, so tier 3 unless moved.
    const FAR: [f64; 2] = [30_000.0, 0.0];

    fn run(
        near_camps: &mut NearEncounters,
        sites: &[NearSite],
        player: [f64; 2],
        now: f64,
        cleared: &mut Cleared,
        fights: &Fights,
    ) -> Changes {
        refresh(
            near_camps,
            sites,
            &encounters(),
            player,
            now,
            cleared,
            fights,
            |_| None,
        )
    }

    fn person(id: EncounterId, hurt: bool, alive: bool, distance: f64) -> Present {
        Present {
            id,
            hurt,
            alive,
            passive: false,
            distance,
        }
    }

    /// A wreck's scavengers are of no band though a faction holds the ground:
    /// the holder is never their allegiance, a camp's band still is.
    #[test]
    fn an_unaligned_crew_stands_with_no_band_whoever_holds_the_ground() {
        let held = |kind: &str| {
            let mut place = near(&site(1, FAR));
            place.kind = kind.to_owned();
            let mut stood = NearEncounters::default();
            let _ = refresh(
                &mut stood,
                &[place],
                &encounters(),
                FAR,
                0.0,
                &mut Cleared::default(),
                &Fights::default(),
                |_| Some(FactionId(3)),
            );
            stood.camps.first().map(|camp| camp.band)
        };
        assert_eq!(held("strata:wreck"), Some(None));
        assert_eq!(held("strata:wreck_works"), Some(None));
        assert_eq!(held("strata:wild_camp"), Some(Some(FactionId(3))));
    }

    #[test]
    fn the_plan_stands_the_tiers_people_on_the_pad_the_same_every_time() {
        let def = camp_def();
        let s = site(1, FAR);
        for (tier, count) in [(1, 2), (2, 7), (3, 8)] {
            let people = plan(&s, None, tier, &def);
            assert_eq!(people.len(), count, "tier {tier}");
            assert_eq!(people, plan(&s, None, tier, &def));
            for slot in &people {
                assert!(distance(slot.at, s.centre) <= s.radius_m, "inside the pad");
            }
        }
        let kinds: Vec<_> = plan(&s, None, 3, &def).iter().map(|p| p.kind).collect();
        assert_eq!(kinds[0], OpponentKind::Duelist);
        assert!(kinds.contains(&OpponentKind::Bulwark));
        assert_ne!(plan(&s, None, 2, &def), plan(&site(2, FAR), None, 2, &def));
    }

    #[test]
    fn the_plan_keeps_off_the_groups_buildings() {
        let def = camp_def();
        let s = site(1, FAR);
        let wall = |at: [f64; 2]| PlacedSiteBuilding {
            file: "tent".to_owned(),
            at: [at[0], 0.0, at[1]],
            yaw: 0.0,
            footprint: [
                [at[0] - 2.0, at[1] - 2.0],
                [at[0] + 2.0, at[1] - 2.0],
                [at[0] + 2.0, at[1] + 2.0],
                [at[0] - 2.0, at[1] + 2.0],
            ],
        };
        // A tent on every free slot's first choice.
        let bare = plan(&s, None, 2, &def);
        let placement = SitePlacement {
            arrangement: 0,
            way_in_clear: true,
            buildings: bare.iter().map(|slot| wall(slot.at)).collect(),
            sign: None,
        };
        let moved = plan(&s, Some(&placement), 2, &def);
        for slot in &moved {
            assert!(
                placement
                    .all()
                    .all(|b| distance_to_quad(slot.at, &b.footprint) >= f64::from(def.clear_m)),
                "{slot:?} stands on a tent"
            );
        }
    }

    #[test]
    fn a_camp_stands_up_within_its_reach_and_folds_beyond_the_fold_only_when_nobody_fights() {
        let s = site(1, FAR);
        let sites = [near(&s)];
        let id = EncounterId::Camp(s.id.key());
        let mut camps = NearEncounters::default();
        let mut cleared = Cleared::default();
        let mut fights = Fights::default();
        let at = |d: f64| [FAR[0] + d, FAR[1]];

        let made = run(&mut camps, &sites, at(300.0), 0.0, &mut cleared, &fights);
        assert!(
            made.stood.is_empty() && camps.camps.is_empty(),
            "300 m is past 260"
        );
        let made = run(&mut camps, &sites, at(250.0), 0.0, &mut cleared, &fights);
        assert_eq!(made.stood, [id]);
        assert_eq!(camps.camps[0].tier, 3, "far from the city");
        assert_eq!(camps.camps[0].slots.len(), 8);
        // Between stand and fold it stays.
        let made = run(&mut camps, &sites, at(350.0), 0.0, &mut cleared, &fights);
        assert_eq!(made, Changes::default());
        // Beyond the fold, but a fight holds it.
        fights.observe([person(id, true, true, 500.0)], 11.0, false);
        let made = run(&mut camps, &sites, at(500.0), 0.0, &mut cleared, &fights);
        assert!(made.folded.is_empty() && camps.camps.len() == 1);
        fights.observe([person(id, true, false, 500.0)], 11.0, false);
        let made = run(&mut camps, &sites, at(500.0), 0.0, &mut cleared, &fights);
        assert_eq!(made.folded, [id]);
        assert!(camps.camps.is_empty());
    }

    #[test]
    fn one_camps_people_engage_without_engaging_another_or_the_staging_fight() {
        let (a, b) = (EncounterId::Camp(1), EncounterId::Camp(2));
        let mut fights = Fights::default();
        fights.observe(
            [
                person(a, false, true, 8.0),
                person(a, false, true, 30.0),
                person(b, false, true, 40.0),
                person(EncounterId::Staging, false, true, 50.0),
            ],
            11.0,
            false,
        );
        assert!(fights.engaged(a) && !fights.engaged(b));
        assert!(!fights.engaged(EncounterId::Staging));
        assert_eq!(fights.remaining(a), 2);
        assert_eq!(fights.headline(), (true, 2));
        // A hurt person of b engages b, and a is as it was.
        fights.observe(
            [
                person(a, false, true, 8.0),
                person(b, true, true, 40.0),
                person(b, false, false, 40.0),
            ],
            11.0,
            false,
        );
        assert!(fights.engaged(a) && fights.engaged(b));
        assert_eq!((fights.remaining(a), fights.remaining(b)), (1, 1));
        // Staging stays as it was: the grace holds it, camps have none.
        fights.observe([person(EncounterId::Staging, false, true, 3.0)], 11.0, true);
        assert!(!fights.engaged(EncounterId::Staging));
        assert!(!fights.engaged(a), "a folded camp is forgotten");
        fights.observe(
            [person(EncounterId::Staging, false, true, 3.0)],
            11.0,
            false,
        );
        assert_eq!(fights.headline(), (true, 1));
    }

    #[test]
    fn a_cleared_camp_stays_down_until_its_hours_have_played_and_then_returns() {
        let s = site(1, FAR);
        let sites = [near(&s)];
        let id = EncounterId::Camp(s.id.key());
        let key = s.id.key();
        let mut camps = NearEncounters::default();
        let mut cleared = Cleared::default();
        let fights = Fights::default();
        run(&mut camps, &sites, FAR, 100.0, &mut cleared, &fights);
        assert_eq!(camps.camps.len(), 1);
        // Its people all fall.
        camps.camps[0].cleared = true;
        cleared.clear(key, 200.0);
        // Still standing, bodies and all, while the member is by.
        let made = run(&mut camps, &sites, FAR, 300.0, &mut cleared, &fights);
        assert_eq!(made, Changes::default());
        // The member leaves: it folds and stays down.
        let far = [FAR[0] + 1_000.0, FAR[1]];
        let made = run(&mut camps, &sites, far, 400.0, &mut cleared, &fights);
        assert_eq!(made.folded, [id]);
        // Back within reach two hours on: nothing.
        let made = run(
            &mut camps,
            &sites,
            FAR,
            200.0 + 2.0 * 3600.0,
            &mut cleared,
            &fights,
        );
        assert_eq!(made, Changes::default());
        assert!(camps.camps.is_empty());
        // Past three played hours it is taken again, fresh.
        let made = run(
            &mut camps,
            &sites,
            FAR,
            200.0 + 3.0 * 3600.0 + 1.0,
            &mut cleared,
            &fights,
        );
        assert_eq!(made.stood, [id]);
        assert!(!camps.camps[0].cleared);
        // The ledger keeps a record for the longest quiet of any encounter
        // (the machine's 12 h), but the camp reads it as run out.
        assert!(!cleared.is_quiet(key, 200.0 + 3.0 * 3600.0 + 1.0, 3.0));
    }

    #[test]
    fn a_member_who_stays_by_a_cleared_camp_sees_it_return() {
        let s = site(1, FAR);
        let sites = [near(&s)];
        let id = EncounterId::Camp(s.id.key());
        let mut camps = NearEncounters::default();
        let mut cleared = Cleared::default();
        let fights = Fights::default();
        run(&mut camps, &sites, FAR, 0.0, &mut cleared, &fights);
        camps.camps[0].cleared = true;
        cleared.clear(s.id.key(), 10.0);
        let made = run(
            &mut camps,
            &sites,
            FAR,
            10.0 + 3.0 * 3600.0 + 1.0,
            &mut cleared,
            &fights,
        );
        assert_eq!(
            (made.folded.clone(), made.stood.clone()),
            (vec![id], vec![id])
        );
    }

    #[test]
    fn the_clock_counts_nominal_seconds() {
        let mut world = World::new();
        world.init_resource::<PlayedClock>();
        for _ in 0..40 {
            world
                .run_system_once(advance_clock)
                .expect("the clock runs");
        }
        assert!((world.resource::<PlayedClock>().seconds - 2.0).abs() < 1e-9);
    }

    fn camp_with_slots(s: &ProceduralSite, tier: u8) -> NearCamp {
        let def = camp_def();
        NearCamp {
            id: EncounterId::Camp(s.id.key()),
            site: s.id,
            name: s.name.clone(),
            centre: s.centre,
            tier,
            band: None,
            slots: plan(s, None, tier, &def),
            cleared: false,
            seen_alive: false,
        }
    }

    fn body(id: EncounterId, dead: bool) -> (EncounterId, Combatant) {
        let mut actor = Combatant::on_team(1, 10.0);
        if dead {
            actor.action = Action::Dead;
        }
        (id, actor)
    }

    #[test]
    fn a_camp_is_cleared_only_once_its_own_people_have_stood_and_fallen() {
        let s = site(1, FAR);
        let camp = camp_with_slots(&s, 1);
        let id = camp.id;
        let mut world = World::new();
        world.insert_resource(PlayedClock { seconds: 50.0 });
        world.insert_resource(Cleared::default());
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            camps: vec![camp],
            forced_tier: None,
            ..Default::default()
        });
        // Corpses of an earlier standing, with the new camp not yet seen
        // alive: nothing is cleared.
        let corpses: Vec<Entity> = (0..2).map(|_| world.spawn(body(id, true)).id()).collect();
        world.run_system_once(mark_cleared).expect("runs");
        assert!(world.resource::<Cleared>().at.is_empty());
        assert!(!world.resource::<NearEncounters>().camps[0].cleared);
        // The old bodies go and the new people stand, one not yet there:
        // killing the one that has stood clears nothing.
        for corpse in corpses {
            world.despawn(corpse);
        }
        let first = world.spawn(body(id, false)).id();
        world.run_system_once(mark_cleared).expect("runs");
        world.get_mut::<Combatant>(first).expect("body").action = Action::Dead;
        world.run_system_once(mark_cleared).expect("runs");
        assert!(
            world.resource::<Cleared>().at.is_empty(),
            "a person is still to stand"
        );
        // The last stands and falls: the camp is cleared at the clock.
        let second = world.spawn(body(id, false)).id();
        world.run_system_once(mark_cleared).expect("runs");
        world.get_mut::<Combatant>(second).expect("body").action = Action::Dead;
        world.run_system_once(mark_cleared).expect("runs");
        assert_eq!(world.resource::<Cleared>().at.get(&s.id.key()), Some(&50.0));
        assert!(world.resource::<NearEncounters>().camps[0].cleared);
    }

    /// A cleared camp leaves a cache of its table at its tier, kept while
    /// the camp is quiet and gone when it returns.
    #[test]
    fn a_cleared_camp_leaves_a_cache_that_goes_when_it_returns() {
        let registries = crate::deeds::tests::shipped_registries();
        let kind = registries
            .0
            .site_kind_id(&"strata:wild_camp".parse().unwrap())
            .expect("the camp kind ships");
        let mut s = site(1, FAR);
        s.id.kind = kind;
        let key = s.id.key();
        let camp = camp_with_slots(&s, 2);
        let id = camp.id;
        let slots = camp.slots.len();
        let mut world = World::new();
        world.insert_resource(registries.clone());
        world.insert_resource(PlayedClock { seconds: 50.0 });
        world.init_resource::<Cleared>();
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            camps: vec![camp],
            ..Default::default()
        });
        let people: Vec<Entity> = (0..slots)
            .map(|_| world.spawn(body(id, false)).id())
            .collect();
        world.run_system_once(mark_cleared).unwrap();
        assert!(world.resource::<Caches>().by.is_empty(), "still fighting");
        for person in people {
            world.get_mut::<Combatant>(person).unwrap().action = Action::Dead;
        }
        world.run_system_once(mark_cleared).unwrap();
        let caches = world.resource::<Caches>().clone();
        let cache = caches.by.get(&key).expect("a cache at the camp");
        let table = registries.0.loot.of(&cache.encounter).unwrap();
        let credits = table.tier(2).unwrap().credits;
        assert!((credits.0..=credits.1).contains(&cache.credits));
        // Quiet keeps it; the camp's return takes it.
        let encounters = &registries.0.encounters;
        let cleared = world.resource::<Cleared>().clone();
        let mut kept = caches.clone();
        kept.prune(&cleared, 50.0 + 3600.0, encounters);
        assert_eq!(kept, caches);
        kept.prune(&cleared, 50.0 + 4.0 * 3600.0, encounters);
        assert!(kept.by.is_empty());
    }

    #[test]
    fn a_camp_that_returns_while_the_member_is_by_is_not_cleared_again_by_its_old_bodies() {
        let s = site(1, FAR);
        let key = s.id.key();
        let id = EncounterId::Camp(key);
        let mut world = World::new();
        world.insert_resource(PlayedClock { seconds: 100.0 });
        let mut cleared = Cleared::default();
        cleared.clear(key, 100.0);
        world.insert_resource(cleared);
        world.init_resource::<Caches>();
        let mut standing = camp_with_slots(&s, 1);
        standing.cleared = true;
        standing.seen_alive = true;
        world.insert_resource(NearEncounters {
            camps: vec![standing],
            forced_tier: None,
            ..Default::default()
        });
        for _ in 0..2 {
            world.spawn(body(id, true));
        }
        // Three played hours on, the member still by: the camp folds and
        // stands again in one pass, and its old dead are still there.
        let hours = 3.0 * 3600.0 + 1.0;
        world.insert_resource(PlayedClock {
            seconds: 100.0 + hours,
        });
        let sites = [near(&s)];
        let mut near_camps = world.remove_resource::<NearEncounters>().expect("near");
        let mut cleared = world.remove_resource::<Cleared>().expect("cleared");
        let made = run(
            &mut near_camps,
            &sites,
            [FAR[0] + 20.0, FAR[1]],
            100.0 + hours,
            &mut cleared,
            &Fights::default(),
        );
        assert!(made.folded.contains(&id) && made.stood.contains(&id));
        world.insert_resource(near_camps);
        world.insert_resource(cleared);
        world.run_system_once(mark_cleared).expect("runs");
        assert!(!world.resource::<NearEncounters>().camps[0].cleared);
        // The record stays for the longest quiet of any encounter; the camp
        // reads it as run out, so it is not cleared again by old bodies.
        assert!(
            !world
                .resource::<Cleared>()
                .is_quiet(key, 100.0 + hours, 3.0)
        );
    }

    #[test]
    fn a_fall_at_a_camp_and_an_f5_let_go_of_its_fight_and_it_folds_once_the_member_is_far() {
        let s = site(1, FAR);
        let sites = [near(&s)];
        let id = EncounterId::Camp(s.id.key());
        let mut camps = NearEncounters::default();
        let mut cleared = Cleared::default();
        let mut fights = Fights::default();
        run(
            &mut camps,
            &sites,
            [FAR[0] + 10.0, FAR[1]],
            0.0,
            &mut cleared,
            &fights,
        );
        // A hurt person engages the camp.
        fights.observe([person(id, true, true, 10.0)], 11.0, false);
        assert_eq!(fights.headline(), (true, 1));
        // F5: the member is 6 km off. The hurt person does not engage again.
        fights.abandon_camps();
        fights.observe([person(id, true, true, 6000.0)], 11.0, false);
        assert!(!fights.engaged(id));
        assert_eq!(fights.headline(), (false, 1));
        let far = [FAR[0] + 6000.0, FAR[1]];
        let made = run(&mut camps, &sites, far, 1.0, &mut cleared, &fights);
        assert_eq!(made.folded, vec![id]);
    }

    #[test]
    fn a_camp_being_fought_is_read_before_the_staging_fight() {
        let camp = EncounterId::Camp(9);
        let mut fights = Fights::default();
        fights.observe(
            [
                person(EncounterId::Staging, true, true, 3.0),
                person(camp, true, true, 5.0),
                person(camp, false, true, 6.0),
            ],
            11.0,
            false,
        );
        assert_eq!(fights.headline(), (true, 2), "the camp, not the staging");
        // Nobody fights: a standing camp is ahead before staging hostiles.
        let mut calm = Fights::default();
        calm.observe(
            [
                person(EncounterId::Staging, false, true, 300.0),
                person(camp, false, true, 200.0),
                person(camp, false, true, 200.0),
            ],
            11.0,
            false,
        );
        assert_eq!(calm.headline(), (false, 2));
    }

    /// The city's concession (Pelagic Works', top grade) and an open
    /// deposit beside it, with the claims the faction map makes of them.
    fn claims_near_the_city() -> (Deposit, Claim, Deposit) {
        let (map, _, rich) = crate::deeds::tests::pelagic();
        let claim = map.claim_of(rich).expect("the city's deposit is a claim");
        let open = Deposit {
            id: DepositId([9_000, 9_000]),
            grade_pm: 100,
            at: [rich.at[0] + 50.0, rich.at[1]],
            ..rich.clone()
        };
        assert_eq!(map.claim_of(&open), None, "a poor deposit is open ground");
        (rich.clone(), claim, open)
    }

    fn post_of(rich: &Deposit, claim: Claim) -> NearPost {
        let mut near = NearEncounters::default();
        let made = refresh_posts(
            &mut near,
            &[(rich.clone(), claim)],
            &encounters(),
            rich.at,
            0.0,
            &Cleared::default(),
            &Fights::default(),
        );
        assert_eq!(made.stood, [EncounterId::Post(post_key(rich.id))]);
        near.posts.remove(0)
    }

    #[test]
    fn a_post_stands_at_a_claim_near_the_member_and_not_at_an_open_deposit() {
        let (rich, claim, open) = claims_near_the_city();
        let map = &crate::deeds::tests::pelagic().0;
        let claimed: Vec<(Deposit, Claim)> = [rich.clone(), open]
            .into_iter()
            .filter_map(|d| map.claim_of(&d).map(|c| (d, c)))
            .collect();
        assert_eq!(claimed.len(), 1);
        let mut near = NearEncounters::default();
        let cleared = Cleared::default();
        let fights = Fights::default();
        let refresh_at = |near: &mut NearEncounters, d: f64| {
            refresh_posts(
                near,
                &claimed,
                &encounters(),
                [rich.at[0] + d, rich.at[1]],
                0.0,
                &cleared,
                &fights,
            )
        };
        let made = refresh_at(&mut near, 300.0);
        assert!(made.stood.is_empty(), "300 m is past 220");
        let made = refresh_at(&mut near, 200.0);
        let id = EncounterId::Post(post_key(rich.id));
        assert_eq!(made.stood, [id]);
        let post = &near.posts[0];
        assert_eq!(
            (post.faction, post.kind),
            (claim.holder, ClaimKind::Concession)
        );
        assert_eq!(
            post.slots.len(),
            2,
            "tier 1 at the city: a duelist and a sentry"
        );
        let post_m = f64::from(
            encounters()
                .at_claim(ClaimKind::Concession)
                .unwrap()
                .post_m
                .unwrap(),
        );
        for slot in &post.slots {
            assert!(
                (distance(slot.at, rich.at) - post_m).abs() < 1e-6,
                "on the ring"
            );
        }
        // Between stand and fold it stays; past the fold, nobody fighting, it folds.
        assert!(refresh_at(&mut near, 300.0).folded.is_empty());
        assert_eq!(refresh_at(&mut near, 400.0).folded, [id]);
    }

    #[test]
    fn no_more_than_two_posts_stand_at_once_the_nearest() {
        let (rich, claim, _) = claims_near_the_city();
        let more = |n: i32| Deposit {
            id: DepositId([9_100 + n, 0]),
            at: [rich.at[0] + 30.0 * f64::from(n), rich.at[1]],
            ..rich.clone()
        };
        let claimed: Vec<_> = [more(3), more(1), more(2)]
            .into_iter()
            .map(|d| (d, claim))
            .collect();
        let mut near = NearEncounters::default();
        refresh_posts(
            &mut near,
            &claimed,
            &encounters(),
            rich.at,
            0.0,
            &Cleared::default(),
            &Fights::default(),
        );
        let ids: Vec<_> = near.posts.iter().map(|p| p.deposit).collect();
        assert_eq!(ids, [DepositId([9_101, 0]), DepositId([9_102, 0])]);
        // Walking on east, the third claim is nearer than the first: the
        // first gives way, as it is neither crossed nor fought.
        let changes = refresh_posts(
            &mut near,
            &claimed,
            &encounters(),
            [rich.at[0] + 95.0, rich.at[1]],
            0.0,
            &Cleared::default(),
            &Fights::default(),
        );
        let ids: Vec<_> = near.posts.iter().map(|p| p.deposit).collect();
        assert_eq!(ids, [DepositId([9_103, 0]), DepositId([9_102, 0])]);
        assert_eq!(
            changes.folded,
            [EncounterId::Post(post_key(DepositId([9_101, 0])))]
        );
        // A post being fought keeps its place.
        let mut fights = Fights::default();
        let mut near = NearEncounters::default();
        refresh_posts(
            &mut near,
            &claimed,
            &encounters(),
            rich.at,
            0.0,
            &Cleared::default(),
            &fights,
        );
        let first = EncounterId::Post(post_key(DepositId([9_101, 0])));
        fights.stand_post(first);
        assert!(fights.cross(first));
        refresh_posts(
            &mut near,
            &claimed,
            &encounters(),
            [rich.at[0] + 95.0, rich.at[1]],
            0.0,
            &Cleared::default(),
            &fights,
        );
        assert!(near.posts.iter().any(|p| p.id == first));
    }

    #[test]
    fn an_uncrossed_post_is_not_engaged_by_distance_or_by_a_hurt_person() {
        let id = EncounterId::Post(4);
        let mut fights = Fights::default();
        fights.stand_post(id);
        fights.observe(
            [person(id, false, true, 2.0), person(id, true, true, 30.0)],
            11.0,
            false,
        );
        assert!(!fights.engaged(id) && !fights.fighting(id));
        assert_eq!(fights.remaining(id), 2);
        assert_eq!(
            fights.headline(),
            (false, 0),
            "an uncrossed post is not ahead"
        );
        assert!(fights.neutral(id));
        // Crossed, it is a camp's fight.
        assert!(fights.cross(id));
        fights.observe([person(id, false, true, 40.0)], 11.0, false);
        assert!(fights.fighting(id));
        assert_eq!(fights.headline(), (true, 1));
        // Folded and standing again: uncrossed.
        fights.forget_post(id);
        fights.stand_post(id);
        assert!(fights.neutral(id) && !fights.engaged(id));
    }

    #[test]
    fn a_post_that_folds_and_stands_in_one_pass_stays_registered() {
        let id = EncounterId::Post(9);
        let mut fights = Fights::default();
        fights.stand_post(id);
        let posts = Changes {
            stood: vec![id],
            folded: vec![id],
        };
        apply_post_changes(&mut fights, &posts);
        assert!(fights.neutral(id) && !fights.engaged(id));
    }

    #[test]
    fn hostility_gives_a_factions_people_the_neutral_team_until_crossed() {
        let people = Allegiance::Faction(FactionId(3));
        assert_eq!(hostility(people, false), NEUTRAL_TEAM);
        assert_eq!(hostility(people, true), 1);
        assert_eq!(hostility(Allegiance::Outlaw(Some(FactionId(3))), false), 1);
    }

    #[test]
    fn a_neutral_is_nobodys_enemy() {
        assert!(enemies(0, 1) && enemies(1, 0));
        assert!(!enemies(0, NEUTRAL_TEAM) && !enemies(NEUTRAL_TEAM, 1));
        assert!(!enemies(NEUTRAL_TEAM, NEUTRAL_TEAM) && !enemies(1, 1));
    }

    fn striking_world() -> App {
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .insert_resource(PlayedClock { seconds: 100.0 })
            .init_resource::<Deeds>()
            .init_resource::<Fights>()
            .add_message::<CombatImpact>()
            .add_message::<PostTurned>()
            .add_systems(Update, strike_posts);
        app
    }

    fn blow(app: &mut App, attacker: Entity, victim: Entity, lethal: bool) {
        let point = strata_core::WorldPos::ZERO;
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(victim),
            from: point,
            point,
            melee: false,
            dealt: 5.0,
            lethal,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        app.update();
    }

    #[test]
    fn a_strike_crosses_the_post_at_once_and_writes_one_hurt_a_person_and_a_killed_on_a_kill() {
        let mut app = striking_world();
        let faction = FactionId(77);
        let id = EncounterId::Post(8);
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        let member = app
            .world_mut()
            .spawn((Combatant::default(), CharacterSheet::default()))
            .id();
        let person = |app: &mut App, id: EncounterId| {
            app.world_mut()
                .spawn((
                    id,
                    Allegiance::Faction(faction),
                    Combatant::on_team(NEUTRAL_TEAM, 30.),
                ))
                .id()
        };
        let (a, b) = (person(&mut app, id), person(&mut app, id));
        // Somebody who is not a household member crosses nothing.
        let stranger = app.world_mut().spawn(Combatant::on_team(1, 10.)).id();
        blow(&mut app, stranger, a, false);
        assert!(app.world().resource::<Fights>().neutral(id));
        assert!(app.world().resource::<Deeds>().by.is_empty());
        // The member's first blow turns the post and is a hurt deed.
        blow(&mut app, member, a, false);
        assert!(app.world().resource::<Fights>().crossed(id));
        assert!(app.world().resource::<Fights>().engaged(id));
        let turned: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<PostTurned>>()
            .drain()
            .collect();
        assert_eq!(
            turned,
            [PostTurned {
                post: id,
                faction,
                by: TurnedBy::Struck
            }]
        );
        // The same person again is not a second hurt; another person is, and
        // a kill is its own deed.
        blow(&mut app, member, a, false);
        let amount = |app: &App, kind: DeedKind| -> u32 {
            app.world().resource::<Deeds>().by[&faction.0]
                .iter()
                .filter(|d| d.kind == kind)
                .map(|d| d.amount)
                .sum()
        };
        assert_eq!(amount(&app, DeedKind::Hurt), 1);
        blow(&mut app, member, b, true);
        assert_eq!(amount(&app, DeedKind::Hurt), 2);
        assert_eq!(amount(&app, DeedKind::Killed), 1);
        // Outlaws are nobody's people: no deed.
        let before = app.world().resource::<Deeds>().clone();
        let outlaw = app
            .world_mut()
            .spawn((
                EncounterId::Camp(1),
                Allegiance::Outlaw(Some(faction)),
                Combatant::on_team(1, 30.),
            ))
            .id();
        blow(&mut app, member, outlaw, true);
        assert_eq!(*app.world().resource::<Deeds>(), before);
    }

    #[test]
    fn working_a_concession_without_a_lease_starts_a_warning_that_lapses_or_turns_the_post() {
        let (rich, claim, _) = claims_near_the_city();
        let id = EncounterId::Post(post_key(rich.id));
        let post = post_of(&rich, claim);
        let warn_s = crate::deeds::tests::shipped_registries()
            .0
            .factions
            .claims()
            .unwrap()
            .warn_s;
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .insert_resource(Factions(Arc::clone(&crate::deeds::tests::pelagic().0)))
            .insert_resource(PlayedClock { seconds: 1_000.0 })
            .init_resource::<Deeds>()
            .init_resource::<Leases>()
            .init_resource::<CommonsTaken>()
            .init_resource::<Fights>();
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        let trespass = |app: &mut App| {
            let deposit = rich.clone();
            app.world_mut()
                .run_system_once(move |mut ledger: Ledger, registries: Res<Registries>| {
                    ledger.trespass(&registries.0, &deposit);
                })
                .unwrap();
        };
        // A lease in force: working is lawful and nothing warns.
        app.world_mut()
            .resource_mut::<Leases>()
            .until
            .insert(claim.holder.0, 5_000.0);
        trespass(&mut app);
        assert_eq!(app.world().resource::<Fights>().warning(), None);
        app.world_mut().resource_mut::<Leases>().until.clear();
        trespass(&mut app);
        let warning = app
            .world()
            .resource::<Fights>()
            .warning()
            .expect("a warning runs");
        assert_eq!((warning.post, warning.faction), (id, claim.holder));
        assert!((warning.until - (1_000.0 + warn_s)).abs() < 1e-9);
        // It runs on one at a time.
        let mut fights = app.world().resource::<Fights>().clone();
        assert!(!fights.begin_warning(id, claim.holder, 9.0));
        let world = |player: [f64; 2], now: f64| Watching {
            posts: std::slice::from_ref(&post),
            player,
            now,
            engage_m: 11.0,
            people: &[],
        };
        let calm = |_: FactionId| false;
        // Leaving the claim lapses it.
        let mut left = fights.clone();
        let away = [rich.at[0] + f64::from(rich.radius_m) + 50.0, rich.at[1]];
        assert!(watch(&mut left, &world(away, 1_001.0), calm, calm).is_empty());
        assert_eq!(left.warning(), None);
        assert!(left.neutral(id));
        // A lease taken lapses it.
        let mut leased = fights.clone();
        assert!(watch(&mut leased, &world(rich.at, 1_001.0), |_| true, calm).is_empty());
        assert_eq!(leased.warning(), None);
        // Still working before it runs out: nothing; at the second, it turns.
        assert!(
            watch(
                &mut fights,
                &world(rich.at, 1_000.0 + warn_s - 1.0),
                calm,
                calm
            )
            .is_empty()
        );
        assert!(fights.warning().is_some() && fights.neutral(id));
        let turned = watch(&mut fights, &world(rich.at, 1_000.0 + warn_s), calm, calm);
        assert_eq!(turned, [(id, claim.holder, TurnedBy::Warning)]);
        assert!(fights.crossed(id) && fights.engaged(id) && fights.warning().is_none());
    }

    #[test]
    fn a_hostile_factions_post_engages_on_sight() {
        let (rich, claim, _) = claims_near_the_city();
        let id = EncounterId::Post(post_key(rich.id));
        let post = post_of(&rich, claim);
        let mut fights = Fights::default();
        fights.stand_post(id);
        let at = |d: f64| [(id, [rich.at[0] + d, rich.at[1]])];
        let run = |fights: &mut Fights, d: f64, hostile: bool| {
            let people = at(d);
            watch(
                fights,
                &Watching {
                    posts: std::slice::from_ref(&post),
                    player: rich.at,
                    now: 0.0,
                    engage_m: 11.0,
                    people: &people,
                },
                |_| false,
                move |_| hostile,
            )
        };
        assert!(
            run(&mut fights, 5.0, false).is_empty(),
            "wary or welcoming: ignored"
        );
        assert!(run(&mut fights, 40.0, true).is_empty(), "out of sight");
        assert!(fights.neutral(id));
        assert_eq!(
            run(&mut fights, 5.0, true),
            [(id, claim.holder, TurnedBy::Sight)]
        );
        assert!(fights.crossed(id));
        // Folded and stood again, it is uncrossed until seen again.
        fights.forget_post(id);
        fights.stand_post(id);
        assert!(fights.neutral(id));
    }

    #[test]
    fn a_cleared_post_is_quiet_and_writes_no_camp_deed() {
        let (rich, claim, _) = claims_near_the_city();
        let mut post = post_of(&rich, claim);
        post.cleared = false;
        let id = post.id;
        let slots = post.slots.len();
        let mut world = World::new();
        world.insert_resource(crate::deeds::tests::shipped_registries());
        world.insert_resource(Factions(Arc::clone(&crate::deeds::tests::pelagic().0)));
        world.insert_resource(PlayedClock { seconds: 60.0 });
        world.init_resource::<Deeds>();
        world.init_resource::<Cleared>();
        world.init_resource::<Caches>();
        world.insert_resource(NearEncounters {
            posts: vec![post],
            ..Default::default()
        });
        let people: Vec<Entity> = (0..slots)
            .map(|_| world.spawn(body(id, false)).id())
            .collect();
        world.run_system_once(mark_cleared).unwrap();
        for person in people {
            world.get_mut::<Combatant>(person).unwrap().action = Action::Dead;
        }
        world.run_system_once(mark_cleared).unwrap();
        assert!(world.resource::<NearEncounters>().posts[0].cleared);
        assert_eq!(
            world.resource::<Cleared>().at.get(&post_key(rich.id)),
            Some(&60.0)
        );
        assert!(
            world.resource::<Caches>().by.is_empty(),
            "a post leaves no cache"
        );
        assert!(
            world.resource::<Deeds>().by.is_empty(),
            "no ClearedCamp deed"
        );
        // It stays quiet for its hours.
        let hours = encounters()
            .at_claim(ClaimKind::Concession)
            .unwrap()
            .returns_h;
        assert!(
            world
                .resource::<Cleared>()
                .is_quiet(post_key(rich.id), 61.0, hours)
        );
        assert!(!is_cleared(
            &encounters(),
            world.resource::<Cleared>(),
            61.0,
            "strata:wild_camp",
            1
        ));
    }

    #[test]
    fn an_uncrossed_concession_guard_sells_a_lease_and_a_warden_or_a_turned_guard_does_not() {
        let (rich, claim, _) = claims_near_the_city();
        let id = EncounterId::Post(post_key(rich.id));
        let mut guards = post_of(&rich, claim);
        let mut wardens = guards.clone();
        wardens.id = EncounterId::Post(1);
        wardens.kind = ClaimKind::Reserve;
        guards.kind = ClaimKind::Concession;
        let mut app = App::new();
        app.insert_resource(crate::deeds::tests::shipped_registries())
            .init_resource::<Fights>()
            .insert_resource(NearEncounters {
                posts: vec![guards, wardens],
                ..Default::default()
            })
            .add_systems(Update, tend_guards);
        let faction = claim.holder;
        let person = |app: &mut App, id: EncounterId| {
            app.world_mut()
                .spawn((
                    id,
                    Allegiance::Faction(faction),
                    Combatant::on_team(NEUTRAL_TEAM, 30.),
                ))
                .id()
        };
        app.world_mut().resource_mut::<Fights>().stand_post(id);
        app.world_mut()
            .resource_mut::<Fights>()
            .stand_post(EncounterId::Post(1));
        let (guard, warden) = (person(&mut app, id), person(&mut app, EncounterId::Post(1)));
        app.update();
        assert_eq!(
            app.world()
                .get::<Interaction>(guard)
                .map(|i| i.kind.clone()),
            Some(InteractionKind::Guard { faction })
        );
        assert!(app.world().get::<Interaction>(warden).is_none());
        app.world_mut().resource_mut::<Fights>().cross(id);
        app.update();
        assert!(app.world().get::<Interaction>(guard).is_none());
    }

    #[test]
    fn hostility_puts_outlaws_on_the_players_enemy_team() {
        assert_eq!(hostility(Allegiance::Outlaw(None), false), 1);
        assert_eq!(hostility(Allegiance::Staging, false), 1);
    }
}
