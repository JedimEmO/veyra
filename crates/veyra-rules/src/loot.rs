//! What a cleared place leaves behind (ticket 0012, step 4a): a cache at the
//! camp's centre that F empties into the pack.
//!
//! [`roll`] is a pure function of its seed and the table, so a cache is the
//! same for a seed however many times it is drawn and the game never has to
//! save how it was made, only what is left. The table is data
//! (`assets/loot/`); salvage is existing components with lines drawn low and
//! wide under the maker `salvage`, so it feeds the same schematics and the
//! smith's rework as anything bought.
//!
//! [`Caches`] holds what is left by camp key. A `CacheRequest::Take` in
//! the `CacheInbox` is consumed in `SimSet::Input`: credits go to the
//! purse, records into the pack as far as they fit, the rest stays. A cache
//! with nothing left is gone, and one goes when its camp returns
//! ([`Caches::prune`]). Local-only, like the stock: the game saves [`Caches`]
//! with the household.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use strata_core::{Rng, Seed, mix64};

use strata_registry::item::{ItemCatalogue, Record};

use strata_registry::{
    EncounterSet, GameRegistries, LootDef, Namespaced, Pick, ResourceSet, SALVAGE_MAKER,
};

use veyra_world::deposits::{Deposit, DepositField};

use crate::deposits::{NearDeposits, lot};

use crate::encounters::Cleared;

use crate::items::{self, Hands, Pack, Stock};

/// What one clearing left: credits and records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    /// The encounter whose table made it, which says how long its camp stays
    /// quiet and so how long the cache lasts.
    pub encounter: Namespaced,
    /// Credits.
    pub credits: u32,
    /// What else, a record a stack.
    pub records: Vec<Record>,
}

impl Cache {
    /// Whether nothing is left in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.credits == 0 && self.records.is_empty()
    }
}

/// The caches left standing, by the key of the camp that left them. The
/// game saves it.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Caches {
    /// Each camp's cache.
    pub by: BTreeMap<u64, Cache>,
}

impl Caches {
    /// Forget the caches of camps that are no longer quiet at played second
    /// `now`: the camp has returned, and what it left is gone with the last
    /// one's clearing. A cache whose encounter is unknown goes too.
    pub fn prune(&mut self, cleared: &Cleared, now: f64, encounters: &EncounterSet) {
        let quiet = |key: &u64, cache: &Cache| {
            encounters
                .get(&cache.encounter)
                .is_some_and(|def| cleared.is_quiet(*key, now, def.returns_h))
        };
        if self.by.iter().any(|(key, cache)| !quiet(key, cache)) {
            self.by.retain(|key, cache| quiet(key, cache));
        }
    }
}

/// The seed of the cache the camp `key`, cleared at played second `at`, left:
/// one stream a camp and a clearing, the same on every machine.
#[must_use]
pub fn seed(world: Seed, key: u64, at: f64) -> u64 {
    world.derive("loot") ^ mix64(key) ^ mix64(at.max(0.0).to_bits())
}

/// The deposits a lot may come from: every deposit within `within_m` of
/// `centre`. When `near` was made round an observer whose register disc
/// holds that whole circle (the member at the camp they cleared, always)
/// the answer is read from it, so a clearing costs no world sample; else
/// one search of the field, which is what the list would have held.
#[must_use]
pub fn lot_candidates(
    field: &DepositField,
    near: Option<&NearDeposits>,
    centre: [f64; 2],
    within_m: f64,
) -> Vec<Deposit> {
    let range = field.set().rules().register.range_m;
    let held = |near: &&NearDeposits| {
        near.around
            .iter()
            .any(|&[x, z]| (x - centre[0]).hypot(z - centre[1]) + within_m <= range)
    };
    match near.filter(held) {
        Some(near) => near
            .deposits
            .iter()
            .filter(|deposit| {
                (deposit.at[0] - centre[0]).hypot(deposit.at[1] - centre[1]) <= within_m
            })
            .cloned()
            .collect(),
        None => field.near(centre[0], centre[1], within_m),
    }
}

/// A lot's deposit of each class: the nearest of `candidates` to `centre`
/// (ties by id), the `near` of [`roll`].
pub fn nearest_of<'a>(
    candidates: &'a [Deposit],
    resources: &'a ResourceSet,
    centre: [f64; 2],
) -> impl Fn(&Namespaced) -> Option<Deposit> + 'a {
    move |class| {
        candidates
            .iter()
            .filter(|deposit| resources.class_of(deposit.ty).id == *class)
            .min_by(|a, b| {
                let d = |deposit: &Deposit| {
                    (deposit.at[0] - centre[0]).hypot(deposit.at[1] - centre[1])
                };
                d(a).total_cmp(&d(b)).then_with(|| a.id.cmp(&b.id))
            })
            .cloned()
    }
}

/// A number in `lo..=hi`.
fn between(rng: &mut Rng, lo: u32, hi: u32) -> u32 {
    lo + rng.below(hi - lo + 1)
}

/// Whether a draw of per-mille `chance` came up.
fn chance(rng: &mut Rng, per_mille: u16) -> bool {
    rng.below(1000) < u32::from(per_mille)
}

/// The record a pick came to, its lines drawn in its range.
fn salvage(items: &ItemCatalogue, pick: &Pick, batch: u32, rng: &mut Rng) -> Option<Record> {
    let def = items.get(&pick.item)?;
    let lines = pick.lines.map_or_else(BTreeMap::new, |range| {
        def.lines
            .iter()
            .map(|line| {
                let figure = between(rng, range.0, range.1);
                (line.id.clone(), u16::try_from(figure).unwrap_or(u16::MAX))
            })
            .collect()
    });
    Some(Record {
        item: pick.item.clone(),
        count: between(rng, pick.count.0, pick.count.1),
        batch,
        by: Some(SALVAGE_MAKER.to_owned()),
        lines,
        condition: None,
        fitted: BTreeMap::new(),
        origin: None,
    })
}

/// What a camp of `tier` leaves, a pure function of `seed`: each pick, lot and
/// rare find is drawn in the table's order, so a table edited at its end
/// leaves the earlier draws as they were. `near` gives the nearest deposit of
/// a resource class; a class with none leaves no lot.
#[must_use]
pub fn roll(
    table: &LootDef,
    tier: u8,
    seed: u64,
    near: impl Fn(&Namespaced) -> Option<Deposit>,
    registries: &GameRegistries,
) -> Cache {
    let mut cache = Cache {
        encounter: table.encounter.clone(),
        credits: 0,
        records: Vec::new(),
    };
    let Some(tier) = table.tier(tier) else {
        return cache;
    };
    let mut rng = Rng::new(seed);
    // One batch a cache, named by its seed and never nought: a bench batch
    // is a small counter, so a salvage batch does not meet one.
    let batch = 1 + u32::try_from(mix64(seed) >> 33).unwrap_or(0);
    cache.credits = between(&mut rng, tier.credits.0, tier.credits.1);
    for pick in &tier.picks {
        if chance(&mut rng, pick.chance) {
            cache
                .records
                .extend(salvage(&registries.items, pick, batch, &mut rng));
        }
    }
    let stack = registries.resources.rules().lots.stack;
    for found in &tier.lots {
        if !chance(&mut rng, found.chance) {
            continue;
        }
        let mut units = between(&mut rng, found.units.0, found.units.1);
        let Some(deposit) = near(&found.class) else {
            continue;
        };
        while units > 0 {
            let part = units.min(stack);
            cache
                .records
                .extend(lot(&deposit, &registries.resources, part));
            units -= part;
        }
    }
    for pick in &tier.rare {
        if chance(&mut rng, pick.chance) {
            cache
                .records
                .extend(salvage(&registries.items, pick, batch, &mut rng));
        }
    }
    cache
}

/// What a take came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTaken {
    /// What went into the pack, as it went: a stack that only partly fit
    /// is here by the part that did.
    pub took: Vec<Record>,
    /// Credits that went to the purse.
    pub credits: u32,
    /// Records still in the cache, which is gone when this is nought.
    pub left: usize,
}

/// Why nothing was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CacheRefusal {
    /// No player to take it.
    #[error("nobody is there")]
    NoPlayer,
    /// The cache is gone.
    #[error("there is nothing left")]
    NoCache,
    /// It is not the prop the player stands beside.
    #[error("it is out of reach")]
    NotInReach,
    /// The pack has no room for any of it, and there are no credits.
    #[error("there is no room in the pack")]
    NoRoom,
}

/// Put what fits of `cache` into `pack`: its credits and whatever records
/// find room, a stack that only partly fits leaving the rest behind. The
/// pack changes only for what moved.
///
/// # Errors
/// [`CacheRefusal::NoRoom`] when nothing moved at all.
pub fn take(
    items: &ItemCatalogue,
    pack: &mut Pack,
    cache: &mut Cache,
) -> Result<CacheTaken, CacheRefusal> {
    let mut took = Vec::new();
    let mut kept = Vec::new();
    let mut next = pack.clone();
    for record in std::mem::take(&mut cache.records) {
        // A record the catalogue now refuses (an item changed since the
        // save) can never be placed, so it goes rather than hold the chest.
        let Ok(whole) = items::write(&record, items) else {
            continue;
        };
        let mut hands = Hands::default();
        let mut stock = Stock {
            saddle: None,
            pack: &mut next,
            hands: &mut hands,
            store: None,
        };
        let over = items::place(items, &mut stock, whole).map_or(0, |rest| rest.count);
        let placed = record.count - over.min(record.count);
        if placed > 0 {
            took.push(Record {
                count: placed,
                ..record.clone()
            });
        }
        if over > 0 {
            kept.push(Record {
                count: over,
                ..record
            });
        }
    }
    cache.records = kept;
    let credits = std::mem::take(&mut cache.credits);
    if took.is_empty() && credits == 0 {
        return Err(CacheRefusal::NoRoom);
    }
    *pack = next;
    Ok(CacheTaken {
        took,
        credits,
        left: cache.records.len(),
    })
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::deposits::NearDeposits;

    use crate::items::Pack;

    use crate::procurement::grade_of;
    use crate::procurement::pay_for;
    use crate::procurement::pays;

    use strata_core::Seed;

    use strata_registry::GameRegistries;

    use strata_registry::Namespaced;

    use strata_registry::SALVAGE_MAKER;
    use strata_registry::Wants;

    use strata_registry::item::Record;
    use strata_registry::load_from_dir;

    use veyra_world::deposits::Deposit;
    use veyra_world::deposits::DepositField;
    use veyra_world::deposits::DepositId;

    pub fn registries() -> GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap()
    }

    pub fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    /// A deposit of `class` mid-way in its type's ranges, which is all a
    /// roll reads of the ground.
    pub fn deposit_of(registries: &GameRegistries, class: &Namespaced) -> Option<Deposit> {
        let set = &registries.resources;
        let ty = set.type_ids().find(|ty| set.class_of(*ty).id == *class)?;
        let def = set.get(ty)?;
        Some(Deposit {
            id: DepositId([3, 4]),
            ty,
            root: "Test".to_owned(),
            place: "Test flats".to_owned(),
            at: [0.0, 0.0],
            height_m: 0.0,
            figures: def
                .ranges
                .iter()
                .map(|(attribute, span)| (*attribute, u16::midpoint(span.floor, span.cap)))
                .collect(),
            radius_m: 100.0,
            peak: 800,
            grade_pm: 500,
        })
    }

    pub fn drawn(registries: &GameRegistries, tier: u8, seed: u64) -> Cache {
        drawn_of(registries, "strata:outlaw_camp", tier, seed)
    }

    pub fn drawn_of(registries: &GameRegistries, encounter: &str, tier: u8, seed: u64) -> Cache {
        let table = registries.loot.of(&id(encounter)).unwrap();
        roll(
            table,
            tier,
            seed,
            |class| deposit_of(registries, class),
            registries,
        )
    }

    /// The same seed is the same cache, every record is one the catalogue
    /// takes, and a drawn line lies inside the table's range.
    #[test]
    fn a_cache_is_a_pure_function_of_its_seed() {
        let registries = registries();
        let table = registries.loot.of(&id("strata:outlaw_camp")).unwrap();
        for tier in 1..=3_u8 {
            let range = table.tier(tier).unwrap();
            let mut found = 0;
            for seed in 0..200_u64 {
                let cache = drawn(&registries, tier, seed);
                assert_eq!(cache, drawn(&registries, tier, seed));
                assert!(
                    (range.credits.0..=range.credits.1).contains(&cache.credits),
                    "credits {} at tier {tier}",
                    cache.credits
                );
                for record in &cache.records {
                    registries.items.check_record(record).unwrap();
                    let salvaged = record.by.as_deref() == Some(SALVAGE_MAKER);
                    if let Some(pick) = range
                        .picks
                        .iter()
                        .chain(&range.rare)
                        .find(|pick| pick.item == record.item)
                    {
                        found += 1;
                        assert!(salvaged);
                        assert!((pick.count.0..=pick.count.1).contains(&record.count));
                        if let Some(lines) = pick.lines {
                            assert!(
                                record.lines.values().all(|line| {
                                    (lines.0..=lines.1).contains(&u32::from(*line))
                                })
                            );
                        }
                    } else {
                        assert!(record.origin.is_some(), "a lot of a deposit");
                    }
                }
            }
            assert!(found > 0, "tier {tier} left salvage");
        }
        assert_ne!(drawn(&registries, 1, 1), drawn(&registries, 1, 2));
        let world = Seed(42);
        assert_eq!(seed(world, 7, 100.0), seed(world, 7, 100.0));
        assert_ne!(seed(world, 7, 100.0), seed(world, 8, 100.0));
        assert_ne!(seed(world, 7, 100.0), seed(world, 7, 101.0));
    }

    /// A class with no deposit near leaves no lot, and a tier past the list
    /// reads the last.
    #[test]
    fn no_ground_leaves_no_lot() {
        let registries = registries();
        let table = registries.loot.of(&id("strata:outlaw_camp")).unwrap();
        for seed in 0..100 {
            let cache = roll(table, 9, seed, |_| None, &registries);
            assert!(cache.records.iter().all(|record| record.origin.is_none()));
        }
    }

    /// A clearing reads its lots' deposits from the survey's list round the
    /// member when that list holds the whole circle, and finds exactly what a
    /// search of the field finds; a list made elsewhere is not read.
    #[test]
    fn lots_come_from_the_kept_list_and_match_the_field() {
        let registries = std::sync::Arc::new(registries());
        let world = std::sync::Arc::new(strata_worldgen::ProceduralWorld::with_params(
            Seed(42),
            &registries,
            strata_worldgen::WorldParams::default(),
        ));
        let city = world.sites()[0].centre;
        let field = DepositField::new(world, Seed(42), std::sync::Arc::clone(&registries));
        let range = field.set().rules().register.range_m;
        let within = registries
            .loot
            .of(&id("strata:outlaw_camp"))
            .unwrap()
            .lots_within_m;
        assert!(
            within * 2.0 <= range,
            "a member at the camp always holds the circle"
        );
        let camp = [city[0] + 300.0, city[1] - 200.0];
        let searched = field.near(camp[0], camp[1], within);
        assert!(!searched.is_empty());
        let kept = NearDeposits {
            deposits: crate::deposits::near_deposits(&field, &[city], range),
            around: vec![city],
        };
        let mut from_kept = lot_candidates(&field, Some(&kept), camp, within);
        let mut wanted = searched.clone();
        from_kept.sort_by_key(|deposit| deposit.id);
        wanted.sort_by_key(|deposit| deposit.id);
        assert_eq!(from_kept, wanted);
        let far = NearDeposits {
            deposits: Vec::new(),
            around: vec![[city[0] + 50_000.0, city[1]]],
        };
        assert_eq!(lot_candidates(&field, Some(&far), camp, within), searched);
        for class in registries
            .resources
            .classes()
            .iter()
            .map(|class| class.id.clone())
        {
            assert_eq!(
                nearest_of(&from_kept, &registries.resources, camp)(&class),
                nearest_of(&searched, &registries.resources, camp)(&class),
            );
        }
    }

    /// What a cache is worth at Pelagic's board, a clearing's average: the
    /// credits, and every record a line buys at or above its minimum. The
    /// figures the table's comment and the ticket quote.
    #[test]
    fn a_tier_is_worth_more_than_the_one_below() {
        let registries = registries();
        let buyer = registries
            .buyers
            .get(&id("veyra:buyer.pelagic_works"))
            .unwrap();
        let worth_of = |encounter: &str, tier: u8| {
            let runs = 600_u64;
            let total: u64 = (0..runs)
                .map(|seed| {
                    let cache = drawn_of(&registries, encounter, tier, seed);
                    let sold: u32 = cache
                        .records
                        .iter()
                        .map(|record| {
                            buyer
                                .lines()
                                .filter(|(_, line)| {
                                    matches!(line.wants, Wants::Item(_) | Wants::Class(_))
                                })
                                .filter_map(|(_, line)| {
                                    let grade = grade_of(&registries.items, line, record)?;
                                    (grade >= line.min)
                                        .then(|| pay_for(pays(buyer, line, grade), record.count))
                                })
                                .max()
                                .unwrap_or(0)
                        })
                        .sum();
                    u64::from(cache.credits + sold)
                })
                .sum();
            total / runs
        };
        let worth = |tier: u8| worth_of("strata:outlaw_camp", tier);
        let (one, two, three) = (worth(1), worth(2), worth(3));
        eprintln!("worth at the board: tier 1 {one}, tier 2 {two}, tier 3 {three}");
        let wreck = [1, 2, 3].map(|tier| worth_of("strata:wreck_scavengers", tier));
        let machine = [1, 2, 3].map(|tier| worth_of("strata:machine_security", tier));
        eprintln!("worth at the board: wreck {wreck:?}, machine {machine:?}");
        // A wreck's cargo is worth more than a camp's of the tier; a machine
        // pays in credits and fine parts, so its board worth sits below.
        for tier in 1..=3 {
            assert!(
                wreck[usize::from(tier) - 1] > worth(tier),
                "a wreck's cargo beats a camp's at tier {tier}"
            );
        }
        assert!((250..450).contains(&wreck[0]) && (650..1000).contains(&wreck[1]));
        assert!((1400..2100).contains(&wreck[2]));
        assert!((60..160).contains(&machine[0]) && (180..380).contains(&machine[1]));
        assert!((400..700).contains(&machine[2]));
        assert!(wreck[0] < wreck[1] && wreck[1] < wreck[2]);
        assert!(machine[0] < machine[1] && machine[1] < machine[2]);
        assert!((150..350).contains(&one), "tier 1 is about one board sale");
        assert!((450..850).contains(&two) && (1000..1800).contains(&three));
    }

    /// A take puts in what fits and leaves the rest, credits always go, and
    /// a pack with no room and no credits takes nothing.
    #[test]
    fn a_take_moves_what_fits_and_leaves_the_rest() {
        let registries = registries();
        let items = &registries.items;
        let cache = (0..50)
            .map(|seed| drawn(&registries, 3, seed))
            .max_by_key(|cache| cache.records.len())
            .unwrap();
        assert!(cache.records.len() > 2);
        let count: u32 = cache.records.iter().map(|record| record.count).sum();
        let sum = |records: &[Record]| records.iter().map(|record| record.count).sum::<u32>();
        // A pack with room takes it all.
        let mut whole = cache.clone();
        let mut pack = Pack::empty(items.carry());
        let taken = take(items, &mut pack, &mut whole).unwrap();
        assert_eq!(taken.credits, cache.credits);
        assert_eq!(taken.left, 0);
        assert!(whole.is_empty());
        assert_eq!(sum(&taken.took), count);
        // A pack of one cell takes part of it: what went and what is left
        // are the whole, and the credits went.
        let mut small = Pack::empty(items.carry());
        small.cells = slotted_model::Inventory::new(1);
        let mut part = cache.clone();
        let taken = take(items, &mut small, &mut part).unwrap();
        assert!(taken.left > 0 && taken.credits == cache.credits);
        assert_eq!(sum(&part.records) + sum(&taken.took), count);
        // The cell is full and the credits are spent: nothing moves.
        let before = small.clone();
        assert_eq!(
            take(items, &mut small, &mut part),
            Err(CacheRefusal::NoRoom)
        );
        assert_eq!(small, before);
    }
}
