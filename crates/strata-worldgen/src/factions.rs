//! Factions: who holds a world's sites and whose ground a point is on
//! (ticket 0012, step 1; `docs/tickets/0012-factions-and-the-world-at-arms.md`).
//!
//! A world makes its own factions, a handful to a land mass, each of a kind
//! `assets/factions/*.faction.ron` defines, with a generated name and colour.
//! Like the deposits they are a pure function of the seed, the placed sites
//! and that data, and in no world version: no faction moves the ground, and
//! the river bake and its canaries know nothing of them.
//!
//! A land mass's factions are made once from its network's sites alone and
//! kept. For each kind in id order the sites of a kind it holds are
//! eligible, the count is the eligible over the kind's `sites_each` rounded
//! and clamped, and the seats spread: the first is the best-weighted
//! eligible site, each next the one farthest from the seats already taken.
//! A site is a seat of one faction only. Every site of a held kind then goes
//! to the faction whose seat is nearest once the distance is divided by that
//! kind's weight for the site's kind.
//!
//! Territory is read from holdings: a held site's ground reaches the holder
//! kind's `reach_m` for the site's kind, strongest at the site and nothing
//! at the reach. The authored holders (Port Serein's Pelagic Works) are
//! factions of no mass whose one holding is their settlement.
//!
//! Nothing here iterates a hash map: every order is a sorted one, so the
//! same inputs give the same factions on any thread.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use strata_core::{Rng, Seed};
pub use strata_registry::ClaimKind;
use strata_registry::{
    FactionKindDef, GameRegistries, Namespaced, Nature, SiteKindId, parse_colour,
};

use crate::ProceduralWorld;
use crate::deposits::Deposit;
use crate::hydrology::Network;
use crate::sites::{ProceduralSite, SiteId};

/// A faction's identity: a hash of the seed and its seat (a site's key, or
/// an authored holder's settlement), the same on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FactionId(pub u64);

/// One faction, whole: nothing about it is looked up afterwards, except its
/// kind's word, which `kind` names in the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    /// Its identity.
    pub id: FactionId,
    /// Its kind, `strata:corporate`.
    pub kind: Namespaced,
    /// Its name, `Halvorn Extraction`.
    pub name: String,
    /// Its colour, sRGB bytes.
    pub colour: [u8; 3],
    /// What sort of body its kind is.
    pub nature: Nature,
    /// Where it sits, world `(x, z)`: its seat site's centre, or its
    /// settlement's.
    pub seat: [f64; 2],
}

/// One held place and how far its ground reaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Holding {
    /// Where, world `(x, z)`.
    pub at: [f64; 2],
    /// How far its ground reaches, metres.
    pub reach_m: f64,
    /// Who holds it.
    pub faction: FactionId,
}

/// Whose ground a point is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Territory {
    /// The strongest holder there, or `None` for unclaimed wilds.
    pub holder: Option<FactionId>,
    /// The holder's strength, `0..=1`: nought where no one reaches.
    pub strength: f64,
    /// Whether another faction is within the rules' margin of the holder.
    pub contested: bool,
}

impl Territory {
    /// Ground no holding reaches.
    pub const UNCLAIMED: Self = Self {
        holder: None,
        strength: 0.0,
        contested: false,
    };
}

/// A rich deposit in a faction's ground, and what the faction makes of it
/// (ticket 0012, step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// Whose it is.
    pub holder: FactionId,
    /// How the holder keeps it.
    pub kind: ClaimKind,
}

/// A land mass's factions, made once.
#[derive(Debug, Default)]
struct MassFactions {
    factions: Vec<Faction>,
    holdings: Vec<Holding>,
    holders: BTreeMap<SiteId, FactionId>,
}

/// The factions of a world.
///
/// Holds the world, the registries and the seed, and the masses already
/// made. A query that meets a land mass no one has baked bakes it, on the
/// calling thread alone; a process prepares its water before it asks.
#[derive(Clone)]
pub struct FactionMap {
    world: Arc<ProceduralWorld>,
    registries: Arc<GameRegistries>,
    seed: u64,
    masses: Arc<RwLock<BTreeMap<[i32; 2], Arc<MassFactions>>>>,
    anchors: Arc<Vec<(Faction, Holding)>>,
    /// Each anchor's home ground: who, where and how far every site is its.
    cores: Arc<Vec<(FactionId, [f64; 2], f64)>>,
    max_reach_m: f64,
}

impl std::fmt::Debug for FactionMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactionMap")
            .field("seed", &self.seed)
            .field("kinds", &self.registries.factions.kinds().len())
            .field("anchors", &self.anchors.len())
            .finish_non_exhaustive()
    }
}

/// FNV-1a of a string, for ids and salts.
fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for b in strata_core::stable_label(text).bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn distance2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dz * dz
}

/// What one faction kind holds, resolved to the site kinds' ids.
struct KindHolds<'a> {
    def: &'a FactionKindDef,
    holds: Vec<(SiteKindId, f64, f64, bool)>,
}

impl KindHolds<'_> {
    fn hold(&self, kind: SiteKindId) -> Option<(f64, f64)> {
        self.holds
            .iter()
            .find(|(k, _, _, _)| *k == kind)
            .map(|&(_, weight, reach, _)| (weight, reach))
    }

    /// Whether a site of `kind` can be a seat of this faction kind.
    fn seats(&self, kind: SiteKindId) -> bool {
        self.holds.iter().any(|&(k, _, _, seat)| k == kind && seat)
    }
}

impl FactionMap {
    /// The factions of `world`, which was built from `seed` and
    /// `registries`.
    #[must_use]
    pub fn new(world: Arc<ProceduralWorld>, seed: Seed, registries: Arc<GameRegistries>) -> Self {
        let seed = seed.derive("factions");
        let set = &registries.factions;
        let mut anchors = Vec::new();
        let mut cores = Vec::new();
        for anchor in set.anchors() {
            let name = anchor.settlement.to_string();
            let Some(site) = world.sites().iter().find(|site| site.id == name) else {
                continue;
            };
            let Some(kind) = set.kind(&anchor.kind) else {
                continue;
            };
            let id = FactionId(strata_core::mix64(seed ^ fnv(&name)));
            cores.push((id, site.centre, anchor.core_m));
            anchors.push((
                Faction {
                    id,
                    kind: kind.id.clone(),
                    name: anchor.name.clone(),
                    colour: parse_colour(&anchor.colour).unwrap_or([128; 3]),
                    nature: kind.nature,
                    seat: site.centre,
                },
                Holding {
                    at: site.centre,
                    reach_m: anchor.reach_m,
                    faction: id,
                },
            ));
        }
        let max_reach_m = set
            .kinds()
            .iter()
            .flat_map(|kind| kind.holds.iter().map(|hold| hold.reach_m))
            .chain(set.anchors().iter().map(|anchor| anchor.reach_m))
            .fold(0.0, f64::max);
        Self {
            world,
            registries,
            seed,
            masses: Arc::default(),
            anchors: Arc::new(anchors),
            cores: Arc::new(cores),
            max_reach_m,
        }
    }

    /// The world the factions are of.
    #[must_use]
    pub fn world(&self) -> &Arc<ProceduralWorld> {
        &self.world
    }

    /// The registries the factions are made from.
    #[must_use]
    pub fn registries(&self) -> &Arc<GameRegistries> {
        &self.registries
    }

    /// The farthest any holding reaches, metres: the radius that gathers
    /// every holding which can touch a point.
    #[must_use]
    pub fn max_reach_m(&self) -> f64 {
        self.max_reach_m
    }

    /// The faction `id` as the set of masses made so far knows it, or an
    /// authored holder.
    #[must_use]
    pub fn faction(&self, id: FactionId) -> Option<Faction> {
        if let Some((faction, _)) = self.anchors.iter().find(|(f, _)| f.id == id) {
            return Some(faction.clone());
        }
        let masses = self.masses.read().unwrap_or_else(PoisonError::into_inner);
        masses
            .values()
            .find_map(|mass| mass.factions.iter().find(|f| f.id == id).cloned())
    }

    /// Every faction with a holding whose ground touches the circle of
    /// `radius_m` round `(x, z)`, authored holders first, then by id.
    #[must_use]
    pub fn factions_near(&self, x: f64, z: f64, radius_m: f64) -> Vec<Faction> {
        let touches = |at: [f64; 2], reach: f64| distance2(at, [x, z]).sqrt() - reach <= radius_m;
        let mut found: Vec<Faction> = self
            .anchors
            .iter()
            .filter(|(_, holding)| touches(holding.at, holding.reach_m))
            .map(|(faction, _)| faction.clone())
            .collect();
        let mut made: BTreeMap<FactionId, Faction> = BTreeMap::new();
        for mass in self.masses_near(x, z, radius_m) {
            for holding in &mass.holdings {
                if touches(holding.at, holding.reach_m)
                    && let Some(faction) = mass.factions.iter().find(|f| f.id == holding.faction)
                {
                    made.entry(faction.id).or_insert_with(|| faction.clone());
                }
            }
        }
        found.extend(made.into_values());
        found
    }

    /// The faction that holds the site `id`, or `None` for a site of a kind
    /// no faction holds (or a mass no one has baked).
    #[must_use]
    pub fn holder_of(&self, id: &SiteId) -> Option<FactionId> {
        let network = self
            .world
            .hydrology()
            .networks()
            .into_iter()
            .find(|network| network.key() == id.mass)?;
        self.mass(&network).holders.get(id).copied()
    }

    /// Every holding whose place is within `radius_m` of `(x, z)`: the
    /// authored holders' and those of the masses that reach there, in a
    /// fixed order.
    #[must_use]
    pub fn holdings_near(&self, x: f64, z: f64, radius_m: f64) -> Vec<Holding> {
        let near = |at: [f64; 2]| distance2(at, [x, z]) <= radius_m * radius_m;
        let mut found: Vec<Holding> = self
            .anchors
            .iter()
            .map(|(_, holding)| *holding)
            .filter(|holding| near(holding.at))
            .collect();
        for mass in self.masses_near(x, z, radius_m) {
            found.extend(mass.holdings.iter().copied().filter(|h| near(h.at)));
        }
        found
    }

    /// Whose ground `(x, z)` is on.
    #[must_use]
    pub fn territory(&self, x: f64, z: f64) -> Territory {
        let holdings = self.holdings_near(x, z, self.max_reach_m);
        self.territory_among(&holdings, x, z)
    }

    /// The claim `deposit` is, if any: its shared grade is in the top
    /// `share` of the grades, its centre is held with at least
    /// `min_strength`, and the holder's nature makes a claim. A pure
    /// function of the deposit, the holdings and `claims.ron`; a data set
    /// without that file claims nothing.
    #[must_use]
    pub fn claim_of(&self, deposit: &Deposit) -> Option<Claim> {
        let rules = self.registries.factions.claims()?;
        if f64::from(deposit.grade_pm) < (1.0 - rules.share) * 1000.0 {
            return None;
        }
        let territory = self.territory(deposit.at[0], deposit.at[1]);
        let holder = territory.holder?;
        if territory.strength < rules.min_strength {
            return None;
        }
        let kind = rules.kind_of(self.faction(holder)?.nature)?;
        Some(Claim { holder, kind })
    }

    /// [`territory`](Self::territory) over holdings already gathered, so a
    /// picture asks it a pixel at a time with no site lookup. `holdings`
    /// must hold every holding within [`max_reach_m`](Self::max_reach_m) of
    /// the point.
    #[must_use]
    pub fn territory_among(&self, holdings: &[Holding], x: f64, z: f64) -> Territory {
        let margin = self
            .registries
            .factions
            .rules()
            .map_or(0.0, |rules| rules.contested_margin);
        // Each faction's strongest holding, then the strongest faction.
        let mut best: BTreeMap<FactionId, f64> = BTreeMap::new();
        for holding in holdings {
            let strength = 1.0 - distance2(holding.at, [x, z]).sqrt() / holding.reach_m;
            if strength > 0.0 {
                let entry = best.entry(holding.faction).or_insert(0.0);
                if strength > *entry {
                    *entry = strength;
                }
            }
        }
        let mut ranked: Vec<(FactionId, f64)> = best.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let Some(&(holder, strength)) = ranked.first() else {
            return Territory::UNCLAIMED;
        };
        let contested = ranked
            .get(1)
            .is_some_and(|&(_, next)| strength - next <= margin);
        Territory {
            holder: Some(holder),
            strength,
            contested,
        }
    }

    /// The masses that reach `radius_m` round `(x, z)` and the farthest
    /// holding's ground beyond it, each made once.
    fn masses_near(&self, x: f64, z: f64, radius_m: f64) -> Vec<Arc<MassFactions>> {
        let wide = radius_m + self.max_reach_m;
        self.world
            .hydrology()
            .bake_area_on(&self.world, (x - wide, z - wide), (x + wide, z + wide), 1)
            .iter()
            .map(|network| self.mass(network))
            .collect()
    }

    /// A mass's factions, made on the first ask and kept.
    fn mass(&self, network: &Arc<Network>) -> Arc<MassFactions> {
        let key = network.key();
        if let Some(made) = self
            .masses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Arc::clone(made);
        }
        // Made outside the lock: it is a pure function, so two threads that
        // race make the same value and one is kept.
        let made = Arc::new(self.make_mass(network));
        let mut masses = self.masses.write().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(masses.entry(key).or_insert(made))
    }

    /// The anchor whose home ground `site` stands in, the nearest if two do.
    fn core_holder(&self, site: &ProceduralSite) -> Option<FactionId> {
        self.cores
            .iter()
            .map(|&(id, at, core)| (distance2(at, site.centre), id, core))
            .filter(|&(d2, _, core)| d2 <= core * core)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id, _)| id)
    }

    /// A seat's place name with its kind's words and any possessive taken
    /// off ("Krot Station" gives "Krot", "Wack's Hold" gives "Wack"), or
    /// empty when nothing is left.
    fn place_root(&self, seat: &ProceduralSite) -> String {
        let Some(def) = self.registries.sites.get(seat.kind()) else {
            return String::new();
        };
        let names = &def.names;
        let words: BTreeSet<&str> = names
            .patterns
            .iter()
            .chain(names.by_feature.iter().flat_map(|f| f.patterns.iter()))
            .flat_map(|pattern| pattern.split_whitespace())
            .filter(|word| !word.contains("{root}"))
            .collect();
        seat.name
            .split_whitespace()
            .filter(|word| !words.contains(word))
            .map(|word| word.strip_suffix("'s").unwrap_or(word))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "seats, names, colours, holders and holdings in the order they depend on each other"
    )]
    fn make_mass(&self, network: &Network) -> MassFactions {
        let set = &self.registries.factions;
        let sites = network.sites();
        let kinds: Vec<KindHolds<'_>> = set
            .kinds()
            .iter()
            .map(|def| KindHolds {
                def,
                holds: def
                    .holds
                    .iter()
                    .filter_map(|hold| {
                        self.registries
                            .sites
                            .id_of(&hold.kind)
                            .map(|id| (id, hold.weight, hold.reach_m, hold.seat))
                    })
                    .collect(),
            })
            .collect();
        let mass_salt = fnv(&format!("{},{}", network.key()[0], network.key()[1]));
        let mut taken_names: BTreeSet<String> = set
            .anchors()
            .iter()
            .map(|anchor| anchor.name.clone())
            .collect();
        let mut seated: BTreeSet<u64> = BTreeSet::new();
        // Faction, the index of its kind in `kinds`.
        let mut made: Vec<(Faction, usize)> = Vec::new();
        for (index, kind) in kinds.iter().enumerate() {
            let eligible: Vec<&ProceduralSite> = sites
                .iter()
                .filter(|site| kind.seats(site.kind()) && self.core_holder(site).is_none())
                .collect();
            let per = kind.def.per_mass;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a count of sites over a positive dial, rounded"
            )]
            let wanted = if eligible.is_empty() {
                0
            } else {
                ((eligible.len() as f64 / per.sites_each).round() as u32).clamp(per.least, per.most)
            };
            let seats = pick_seats(kind, &eligible, &seated, wanted as usize);
            let mut order: Vec<usize> = (0..kind.def.palette.len()).collect();
            let mut shuffle = Rng::new(self.seed ^ mass_salt ^ fnv(kind.def.id.as_ref()));
            for i in (1..order.len()).rev() {
                let j = shuffle.below(u32::try_from(i + 1).unwrap_or(u32::MAX)) as usize;
                order.swap(i, j);
            }
            for (n, seat) in seats.into_iter().enumerate() {
                seated.insert(seat.id.key());
                let mut rng = Rng::new(self.seed ^ seat.id.key());
                let patterns = &kind.def.names.patterns;
                // The seat's own place gives the root when it has one, so a
                // faction reads as of the ground it sits on.
                let root = self.place_root(seat);
                let mut name = if root.is_empty() {
                    crate::sites::name(&kind.def.names, patterns, &mut rng)
                } else {
                    let n = u32::try_from(patterns.len()).unwrap_or(1);
                    patterns[rng.below(n) as usize].replace("{root}", &root)
                };
                let mut tries = 0;
                // A faction never wears its seat's own name ("X Watch" at X Watch).
                while taken_names.contains(&name) || name == seat.name {
                    tries += 1;
                    name = if tries < 64 {
                        crate::sites::name(&kind.def.names, patterns, &mut rng)
                    } else {
                        format!("{name} {tries}")
                    };
                }
                taken_names.insert(name.clone());
                let colour = kind.def.palette[order[n % order.len()]].as_str();
                made.push((
                    Faction {
                        id: FactionId(strata_core::mix64(self.seed ^ seat.id.key())),
                        kind: kind.def.id.clone(),
                        name,
                        colour: parse_colour(colour).unwrap_or([128; 3]),
                        nature: kind.def.nature,
                        seat: seat.centre,
                    },
                    index,
                ));
            }
        }
        let mut holders = BTreeMap::new();
        let mut holdings = Vec::new();
        for site in sites {
            // Home ground is the anchor's whatever the site's kind; its own
            // holding already covers it, so no second one is added.
            if let Some(anchor) = self.core_holder(site) {
                holders.insert(site.id, anchor);
                continue;
            }
            let mut best: Option<(f64, FactionId, f64)> = None;
            for (faction, index) in &made {
                let Some((weight, reach)) = kinds[*index].hold(site.kind()) else {
                    continue;
                };
                let score = distance2(faction.seat, site.centre).sqrt() / weight;
                let better = best.is_none_or(|(held, id, _)| {
                    score
                        .total_cmp(&held)
                        .then_with(|| faction.id.cmp(&id))
                        .is_lt()
                });
                if better {
                    best = Some((score, faction.id, reach));
                }
            }
            if let Some((_, faction, reach_m)) = best {
                holders.insert(site.id, faction);
                holdings.push(Holding {
                    at: site.centre,
                    reach_m,
                    faction,
                });
            }
        }
        MassFactions {
            factions: made.into_iter().map(|(faction, _)| faction).collect(),
            holdings,
            holders,
        }
    }
}

/// The seats of one kind on a mass: the best-weighted eligible site, then
/// each time the one farthest from every seat taken, among the sites no
/// other faction sits on. Ties go to the lower key.
fn pick_seats<'a>(
    kind: &KindHolds<'_>,
    eligible: &[&'a ProceduralSite],
    seated: &BTreeSet<u64>,
    wanted: usize,
) -> Vec<&'a ProceduralSite> {
    let weight = |site: &ProceduralSite| kind.hold(site.kind()).map_or(0.0, |(w, _)| w);
    let mut free: Vec<&ProceduralSite> = eligible
        .iter()
        .copied()
        .filter(|site| !seated.contains(&site.id.key()))
        .collect();
    let mut seats: Vec<&ProceduralSite> = Vec::new();
    // How far each free site is from its nearest seat, kept as seats are
    // added so a big mass costs one pass a seat.
    let mut apart: Vec<f64> = vec![f64::INFINITY; free.len()];
    while seats.len() < wanted && !free.is_empty() {
        let pick = if seats.is_empty() {
            free.iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    weight(a)
                        .total_cmp(&weight(b))
                        .then(a.score.total_cmp(&b.score))
                        .then(b.id.key().cmp(&a.id.key()))
                })
                .map(|(i, _)| i)
        } else {
            (0..free.len()).max_by(|&a, &b| {
                apart[a]
                    .total_cmp(&apart[b])
                    .then(free[b].id.key().cmp(&free[a].id.key()))
            })
        };
        let Some(pick) = pick else { break };
        let seat = free.remove(pick);
        apart.remove(pick);
        for (site, d) in free.iter().zip(apart.iter_mut()) {
            *d = d.min(distance2(seat.centre, site.centre));
        }
        seats.push(seat);
    }
    seats
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    /// How far round Port Serein the tests bake, metres: its land mass and
    /// the ones beside it that a holding of its reaches into.
    const BAKED_M: f64 = 25_000.0;

    fn registries() -> Arc<GameRegistries> {
        Arc::new(
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load"),
        )
    }

    /// A map over its own world, baked round the city on four threads: two
    /// of these share no cache, so they make everything twice.
    fn map(registries: &Arc<GameRegistries>) -> FactionMap {
        map_on(registries, 4)
    }

    fn map_on(registries: &Arc<GameRegistries>, threads: usize) -> FactionMap {
        let world = ProceduralWorld::new(Seed(42), registries).with_own_water();
        world.hydrology().set_threads(threads);
        let [x, z] = world.sites()[0].centre;
        world.prepare_water(x, z, BAKED_M);
        FactionMap::new(Arc::new(world), Seed(42), Arc::clone(registries))
    }

    fn city(map: &FactionMap) -> [f64; 2] {
        map.world().sites()[0].centre
    }

    /// Every faction near the city with its holdings, by id.
    fn everything(map: &FactionMap) -> (Vec<Faction>, Vec<Holding>) {
        let [x, z] = city(map);
        let mut factions = map.factions_near(x, z, BAKED_M);
        factions.sort_by_key(|f| f.id);
        (factions, map.holdings_near(x, z, BAKED_M))
    }

    #[test]
    fn two_maps_make_the_same_factions_and_holders() {
        let registries = registries();
        let (first, second) = (map(&registries), map_on(&registries, 1));
        let from_thread = {
            let other = second.clone();
            std::thread::spawn(move || everything(&other))
                .join()
                .unwrap()
        };
        let (a, holdings_a) = everything(&first);
        let (b, holdings_b) = everything(&second);
        assert!(a.len() >= 3, "only {} factions round the city", a.len());
        assert_eq!(a, b);
        assert_eq!(holdings_a, holdings_b);
        assert_eq!((a, holdings_a), from_thread);
        let [x, z] = city(&first);
        for site in first.world().sites_near(x, z, BAKED_M) {
            assert_eq!(first.holder_of(&site.id), second.holder_of(&site.id));
        }
    }

    #[test]
    fn a_wreck_or_a_machine_is_held_but_never_a_seat() {
        let registries = registries();
        let map = map(&registries);
        let [x, z] = city(&map);
        let (factions, _) = everything(&map);
        let id = |word: &str| registries.sites.id_of(&word.parse().unwrap()).unwrap();
        let (wreck, machine) = (id("strata:wreck"), id("strata:wreck_works"));
        let mut seen = 0;
        for site in map.world().sites_near(x, z, BAKED_M) {
            let kind = site.kind();
            if kind != wreck && kind != machine {
                continue;
            }
            seen += 1;
            assert!(
                factions
                    .iter()
                    .all(|f| (f.seat[0] - site.centre[0]).hypot(f.seat[1] - site.centre[1]) > 1.0),
                "{} is a seat",
                site.name
            );
            let holder = map.faction(map.holder_of(&site.id).unwrap()).unwrap();
            let corporate = holder.kind.to_string() == "strata:corporate";
            let conservation = holder.kind.to_string() == "strata:conservation";
            assert!(
                corporate || (kind == machine && conservation),
                "{} is held by {}",
                site.name,
                holder.kind
            );
        }
        assert!(seen > 0, "no wreck or machine round the city");
    }

    #[test]
    fn port_serein_is_held_by_pelagic_works() {
        let map = map(&registries());
        let [x, z] = city(&map);
        let territory = map.territory(x, z);
        let holder = map.faction(territory.holder.unwrap()).unwrap();
        assert_eq!(holder.name, "Pelagic Works");
        assert_eq!(holder.nature, Nature::Corporate);
        assert!(territory.strength > 0.99, "{}", territory.strength);
    }

    #[test]
    fn every_site_of_a_held_kind_has_a_holder_and_names_are_unique() {
        let registries = registries();
        let map = map(&registries);
        let held: BTreeSet<SiteKindId> = registries
            .factions
            .kinds()
            .iter()
            .flat_map(|kind| kind.holds.iter())
            .filter_map(|hold| registries.sites.id_of(&hold.kind))
            .collect();
        let hamlet = registries
            .sites
            .id_of(&"strata:hamlet".parse().unwrap())
            .unwrap();
        let [cx, cz] = city(&map);
        let core = registries.factions.anchors()[0].core_m;
        let in_core = |site: &ProceduralSite| distance2(site.centre, [cx, cz]) <= core * core;
        let mut hamlets = 0;
        let mut checked = 0;
        for network in map.world().hydrology().networks() {
            let mut names = BTreeSet::new();
            for faction in &map.mass(&network).factions {
                assert!(names.insert(faction.name.clone()), "{} twice", faction.name);
                assert!(
                    registries
                        .factions
                        .anchors()
                        .iter()
                        .all(|a| a.name != faction.name),
                    "{} is an anchor's name",
                    faction.name
                );
            }
            for site in network.sites() {
                let holder = map.holder_of(&site.id);
                assert_eq!(
                    holder.is_some(),
                    held.contains(&site.kind()) || in_core(site),
                    "{}",
                    site.id
                );
                checked += 1;
                if site.kind() == hamlet && !in_core(site) {
                    let faction = map.faction(holder.unwrap()).unwrap();
                    assert_eq!(faction.nature, Nature::Assembly, "{}", site.name);
                    hamlets += 1;
                }
            }
        }
        assert!(
            checked > 5 && hamlets > 0,
            "{checked} sites, {hamlets} hamlets"
        );
    }

    #[test]
    fn far_ground_is_unclaimed_and_strength_falls_with_distance() {
        let map = map(&registries());
        let [x, z] = city(&map);
        // Past the baked disc and every reach beyond it: no gathered
        // holding touches the point, and none is baked to find.
        let all = map.holdings_near(x, z, BAKED_M);
        let far = BAKED_M + map.max_reach_m() + 1.0;
        assert_eq!(map.territory_among(&all, x + far, z), Territory::UNCLAIMED);
        // Real ground too: some point inside the baked disc is no one's.
        let wild = (1..=20).find_map(|step| {
            let d = f64::from(step) * 1_000.0;
            let t = map.territory(x - d, z + d * 0.3);
            t.holder.is_none().then_some(t)
        });
        assert_eq!(wild, Some(Territory::UNCLAIMED), "no wilds within 20 km");
        let anchor = map.holdings_near(x, z, 1.0)[0];
        let at = |d: f64| map.territory_among(&[anchor], x + d, z).strength;
        assert!(at(0.0) > at(2_000.0) && at(2_000.0) > at(6_000.0) && at(6_000.0) > 0.0);
        assert!(at(anchor.reach_m + 1.0) <= 0.0);
        assert_eq!(
            map.territory_among(&[anchor], x + anchor.reach_m + 1.0, z)
                .holder,
            None
        );
    }

    #[test]
    fn two_close_holdings_read_as_contested() {
        let map = map(&registries());
        let [x, z] = city(&map);
        let (a, b) = (FactionId(1), FactionId(2));
        let holding = |at: f64, faction| Holding {
            at: [at, z],
            reach_m: 1_000.0,
            faction,
        };
        let holdings = [holding(x - 500.0, a), holding(x + 510.0, b)];
        let between = map.territory_among(&holdings, x, z);
        assert!(between.contested);
        let near_a = map.territory_among(&holdings, x - 450.0, z);
        assert_eq!(near_a.holder, Some(a));
        assert!(!near_a.contested);
    }

    #[test]
    fn a_site_within_an_anchors_core_is_the_anchors_whatever_its_kind() {
        let registries = registries();
        let map = map(&registries);
        let [x, z] = city(&map);
        let core = registries.factions.anchors()[0].core_m;
        let pelagic = map.territory(x, z).holder.unwrap();
        let inside: Vec<_> = map
            .world()
            .sites_near(x, z, core)
            .into_iter()
            .filter(|site| distance2(site.centre, [x, z]) <= core * core)
            .collect();
        assert!(!inside.is_empty(), "no site within {core} m of the city");
        for site in &inside {
            assert_eq!(map.holder_of(&site.id), Some(pelagic), "{}", site.name);
        }
        // No seat is taken in the core: nothing else sits there.
        for faction in map.factions_near(x, z, BAKED_M) {
            if faction.id != pelagic {
                assert!(
                    distance2(faction.seat, [x, z]) > core * core,
                    "{} sits in Pelagic's core",
                    faction.name
                );
            }
        }
    }

    #[test]
    fn a_factions_name_holds_its_seats_root() {
        let registries = registries();
        let map = map(&registries);
        let [x, z] = city(&map);
        let mut rooted = 0;
        for faction in map.factions_near(x, z, BAKED_M) {
            if registries
                .factions
                .anchors()
                .iter()
                .any(|a| a.name == faction.name)
            {
                continue;
            }
            let seat = map
                .world()
                .sites_near(faction.seat[0], faction.seat[1], 1.0)
                .into_iter()
                .find(|site| distance2(site.centre, faction.seat) < 1.0)
                .unwrap();
            let root = map.place_root(&seat);
            if !root.is_empty() && faction.name.contains(&root) {
                rooted += 1;
            }
        }
        assert!(
            rooted >= 3,
            "only {rooted} factions carry their seat's root"
        );
        let krot = map
            .world()
            .sites_near(x, z, BAKED_M)
            .into_iter()
            .find(|site| site.name.ends_with("Station"))
            .unwrap();
        assert!(!map.place_root(&krot).contains("Station"));
    }

    fn deposits(
        map: &FactionMap,
        registries: &Arc<GameRegistries>,
    ) -> crate::deposits::DepositField {
        crate::deposits::DepositField::new(
            Arc::clone(map.world()),
            Seed(42),
            Arc::clone(registries),
        )
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the shipped share's floor, 800, as a per-mille grade"
    )]
    fn a_claim_is_a_top_grade_deposit_in_held_ground_of_its_holders_kind() {
        let registries = registries();
        let map = map(&registries);
        let field = deposits(&map, &registries);
        let [x, z] = city(&map);
        let pelagic = map.territory(x, z).holder.unwrap();
        let share = registries.factions.claims().unwrap().share;
        let floor = (1.0 - share) * 1000.0;
        let base = field.near(x, z, 6_000.0).into_iter().next().unwrap();
        // Standing in the city, so the ground is Pelagic's at full strength.
        let at_city = |grade_pm: u16| Deposit {
            at: [x, z],
            grade_pm,
            ..base.clone()
        };
        assert_eq!(
            map.claim_of(&at_city(0)),
            None,
            "the bottom is never claimed"
        );
        let below = floor as u16 - 1;
        assert_eq!(map.claim_of(&at_city(below)), None, "just under the share");
        let top = map.claim_of(&at_city(floor as u16)).unwrap();
        assert_eq!(top.holder, pelagic);
        assert_eq!(
            top.kind,
            ClaimKind::Concession,
            "Pelagic Works' ground has concessions"
        );
        assert_eq!(
            map.claim_of(&at_city(999)).unwrap().kind,
            ClaimKind::Concession
        );
        // Wilds hold nothing, whatever the grade.
        let wild = (1..=20)
            .map(|step| f64::from(step) * 1_000.0)
            .find(|d| map.territory(x - d, z + d * 0.3).holder.is_none())
            .unwrap();
        let far = Deposit {
            at: [x - wild, z + wild * 0.3],
            grade_pm: 999,
            ..base.clone()
        };
        assert_eq!(map.claim_of(&far), None);
        // The thin edge of a reach is not guarded.
        let anchor = registries.factions.anchors()[0].reach_m;
        let min = registries.factions.claims().unwrap().min_strength;
        let edge = Deposit {
            at: [x + anchor * (1.0 - min * 0.5), z],
            grade_pm: 999,
            ..base
        };
        let edge_territory = map.territory(edge.at[0], edge.at[1]);
        if edge_territory.holder == Some(pelagic) {
            assert_eq!(map.claim_of(&edge), None, "{}", edge_territory.strength);
        }
    }

    /// What the field within 6 km of the city holds: the claims by kind.
    /// Printed with `--nocapture`; the assertion is the half the first hour
    /// walks to being open ground.
    #[test]
    fn most_of_the_deposits_round_the_city_are_open_ground() {
        let registries = registries();
        let map = map(&registries);
        let field = deposits(&map, &registries);
        let [x, z] = city(&map);
        let all = field.near(x, z, 6_000.0);
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for deposit in &all {
            let key = match map.claim_of(deposit) {
                None => "open".to_owned(),
                Some(claim) => format!(
                    "{:?} of {}",
                    claim.kind,
                    map.faction(claim.holder).unwrap().name
                ),
            };
            *by_kind.entry(key).or_default() += 1;
        }
        eprintln!("{} deposits within 6 km: {by_kind:?}", all.len());
        let open = by_kind.get("open").copied().unwrap_or(0);
        assert!(!all.is_empty() && open * 2 > all.len(), "{by_kind:?}");
    }
}
